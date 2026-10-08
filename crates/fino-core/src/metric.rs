//! Tiled perceptual comparison.
//!
//! The eye jumps to the *worst* region: a sharp face over a blocky sky reads as a
//! bad photo. So instead of one global number we
//! split the frame into tiles, score each with zensim (a fast SSIMULACRA 2
//! approximation in XYB space) and report both the area-weighted mean and the
//! worst tile.

use crate::codec::Pixels;
use crate::error::{FinoError, Result};
use rayon::prelude::*;
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::OnceLock;
use zensim::{
    DiffmapOptions, DiffmapWeighting, PixelFormat, PrecomputedReference, StridedBytes, Zensim,
    ZensimProfile, ZensimScratch,
};

thread_local! {
    /// Per-thread working planes for zensim. Without reuse every comparison allocates and
    /// page-faults ~100 MB at 24 MP, which showed up as a third of all CPU time.
    static SCRATCH: RefCell<ZensimScratch> = RefCell::new(ZensimScratch::new());
}

fn with_scratch<T>(f: impl FnOnce(&mut ZensimScratch) -> T) -> T {
    SCRATCH.with(|cell| match cell.try_borrow_mut() {
        Ok(mut scratch) => f(&mut scratch),
        // Re-entrant use on one thread (rayon work stealing inside zensim): fall back to a
        // throwaway buffer rather than panic.
        Err(_) => f(&mut ZensimScratch::new()),
    })
}

const MIN_TILE: u32 = 256;
const MAX_TILE: u32 = 768;
/// An edge strip thinner than this is merged into its neighbour.
const MIN_EDGE: u32 = 96;
/// Above this short side the whole-frame score runs at half resolution: it exists to catch
/// low-frequency damage (banding, colour drift) which survives a 2× box downscale, while
/// pixel-level artifacts are the full-resolution tiles' job. Cuts the global pass ~4×.
const HALF_RES_GLOBAL_MIN_SIDE: u32 = 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Whole-frame score, SSIMULACRA 2 scale. Sees low-frequency defects (banding,
    /// colour shifts across a sky) that no single tile can.
    pub global: f64,
    /// Area-weighted mean of the scored tiles, SSIMULACRA 2 scale. The search scores the
    /// hardest tiles only; [`Reference::compare`] scores every tile.
    pub mean: f64,
    /// Lowest score among the scored tiles, SSIMULACRA 2 scale.
    pub worst: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Tile {
    fn scaled_down(self, scale: u32) -> Tile {
        Tile {
            x: self.x / scale,
            y: self.y / scale,
            w: self.w / scale,
            h: self.h / scale,
        }
    }

    fn area(self) -> f64 {
        self.w as f64 * self.h as f64
    }
}

/// Precomputed reference image, reused for every candidate of the search.
///
/// Full-resolution tile references are built lazily: the search only ever looks closely at
/// the hardest tiles, and each precomputed tile costs as much as a comparison.
pub struct Reference<'a> {
    width: u32,
    height: u32,
    /// Source pixels as interleaved RGB, for the lazily built full-resolution tiles.
    source: Cow<'a, [u8]>,
    tiles: Vec<Tile>,
    full: Vec<OnceLock<PrecomputedReference>>,
    global: PrecomputedReference,
    /// 1 = full resolution, 2 = half.
    global_scale: u32,
    zensim: Zensim,
    zensim_parallel: Zensim,
}

/// A candidate prepared for scoring: RGB at full resolution plus the global frame, both
/// computed once and shared by every score taken from it.
pub struct Frame<'a> {
    rgb: Cow<'a, [u8]>,
    /// The global frame when it is downscaled; `None` means it is `rgb` itself.
    small: Option<Vec<u8>>,
    width: u32,
}

impl Frame<'_> {
    fn global(&self) -> &[u8] {
        self.small.as_deref().unwrap_or(&self.rgb)
    }
}

fn spans(len: u32, tile: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < len {
        let mut size = tile.min(len - start);
        if len - start - size < MIN_EDGE {
            size = len - start;
        }
        out.push((start, size));
        start += size;
    }
    out
}

fn tiles_for(width: u32, height: u32) -> Vec<Tile> {
    let tile = (width.max(height) / 6).clamp(MIN_TILE, MAX_TILE);
    let cols = spans(width, tile);
    spans(height, tile)
        .into_iter()
        .flat_map(|(y, h)| cols.iter().map(move |&(x, w)| Tile { x, y, w, h }))
        .collect()
}

fn rgb_bytes(pixels: &Pixels) -> Cow<'_, [u8]> {
    match pixels.channels {
        1 => pixels
            .data
            .iter()
            .flat_map(|&y| [y, y, y])
            .collect::<Vec<u8>>()
            .into(),
        _ => (&pixels.data[..]).into(),
    }
}

/// 2×2 box average of interleaved RGB (odd last row/column dropped).
fn half_res(rgb: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (w2, h2) = (width as usize / 2, height as usize / 2);
    let stride = width as usize * 3;
    let mut out = vec![0u8; w2 * h2 * 3];
    out.par_chunks_mut(w2 * 3).enumerate().for_each(|(y, row)| {
        let top = &rgb[2 * y * stride..][..stride];
        let bottom = &rgb[(2 * y + 1) * stride..][..stride];
        for (x, px) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            for (c, value) in px.iter_mut().enumerate() {
                let i = 6 * x + c;
                let sum =
                    top[i] as u16 + top[i + 3] as u16 + bottom[i] as u16 + bottom[i + 3] as u16;
                *value = ((sum + 2) / 4) as u8;
            }
        }
    });
    (out, w2 as u32, h2 as u32)
}

fn view(rgb: &[u8], width: u32, t: Tile) -> Result<StridedBytes<'_>> {
    let stride = width as usize * 3;
    let start = t.y as usize * stride + t.x as usize * 3;
    StridedBytes::try_new(
        &rgb[start..],
        t.w as usize,
        t.h as usize,
        stride,
        PixelFormat::Srgb8Rgb,
    )
    .map_err(|e| FinoError::Metric(e.to_string()))
}

fn frame_of<'a>(rgb: impl Into<Cow<'a, [u8]>>, width: u32, height: u32, scale: u32) -> Frame<'a> {
    let rgb = rgb.into();
    let small = (scale == 2).then(|| half_res(&rgb, width, height).0);
    Frame { rgb, small, width }
}

fn metric_err(e: zensim::ZensimError) -> FinoError {
    FinoError::Metric(e.to_string())
}

impl<'a> Reference<'a> {
    pub fn new(pixels: &'a Pixels) -> Result<Self> {
        if pixels.width < 8 || pixels.height < 8 {
            return Err(FinoError::Metric("image smaller than 8×8".into()));
        }
        let zensim = Zensim::new(ZensimProfile::latest()).with_parallel(false);
        let zensim_parallel = Zensim::new(ZensimProfile::latest());
        let global_scale = if pixels.width.min(pixels.height) >= HALF_RES_GLOBAL_MIN_SIDE {
            2
        } else {
            1
        };
        let tiles = tiles_for(pixels.width, pixels.height);
        let source = rgb_bytes(pixels);
        let frame = frame_of(&source[..], pixels.width, pixels.height, global_scale);
        let (gw, gh) = (pixels.width / global_scale, pixels.height / global_scale);
        let global = zensim_parallel
            .precompute_reference(&view(
                frame.global(),
                gw,
                Tile {
                    x: 0,
                    y: 0,
                    w: gw,
                    h: gh,
                },
            )?)
            .map_err(metric_err)?;
        drop(frame);
        Ok(Self {
            width: pixels.width,
            height: pixels.height,
            source,
            full: tiles.iter().map(|_| OnceLock::new()).collect(),
            tiles,
            global,
            global_scale,
            zensim,
            zensim_parallel,
        })
    }

    fn global_size(&self) -> (u32, u32) {
        (
            self.width / self.global_scale,
            self.height / self.global_scale,
        )
    }

    /// Prepares `candidate` for scoring.
    pub fn frame<'c>(&self, candidate: &'c Pixels) -> Result<Frame<'c>> {
        if candidate.width != self.width || candidate.height != self.height {
            return Err(FinoError::Metric(
                "candidate size differs from reference".into(),
            ));
        }
        Ok(frame_of(
            rgb_bytes(candidate),
            candidate.width,
            candidate.height,
            self.global_scale,
        ))
    }

    /// Full verdict: whole-frame score plus statistics over every tile at full resolution.
    pub fn compare(&self, candidate: &Pixels) -> Result<Score> {
        let frame = self.frame(candidate)?;
        let global = self.global(&frame)?;
        let all: Vec<usize> = (0..self.tiles.len()).collect();
        let scores = self.tile_scores(&frame, &all)?;
        let (mean, worst) = self.summarize(&all, &scores);
        Ok(Score {
            global,
            mean,
            worst,
        })
    }

    fn global_view<'f>(&self, frame: &'f Frame) -> Result<StridedBytes<'f>> {
        let (w, h) = self.global_size();
        view(frame.global(), w, Tile { x: 0, y: 0, w, h })
    }

    /// Whole-frame score (half resolution for large photos).
    pub fn global(&self, frame: &Frame) -> Result<f64> {
        let distorted = self.global_view(frame)?;
        with_scratch(|scratch| {
            self.zensim_parallel
                .compute_with_ref_into(&self.global, &distorted, scratch)
                .map(|r| r.approx_ssim2())
                .map_err(metric_err)
        })
    }

    /// Whole-frame score plus how hard each tile is (higher = harder), pooled from the
    /// same pass's per-pixel error map. Ranks tiles nearly as well as scoring each one on
    /// the half-resolution frame, at a fraction of the cost.
    pub fn global_and_difficulty(&self, frame: &Frame) -> Result<(f64, Vec<f64>)> {
        let mut options = DiffmapOptions::from(DiffmapWeighting::Balanced);
        options.include_edge_mse = true;
        let result = self
            .zensim_parallel
            .compute_with_ref_and_diffmap(&self.global, &self.global_view(frame)?, options)
            .map_err(metric_err)?;
        let (map, width) = (result.diffmap(), result.width());
        let difficulty = self
            .tiles
            .iter()
            .map(|t| {
                let t = t.scaled_down(self.global_scale);
                let sum: f64 = (t.y..t.y + t.h)
                    .map(|y| {
                        let row = y as usize * width + t.x as usize;
                        map[row..row + t.w as usize]
                            .iter()
                            .map(|&v| v as f64)
                            .sum::<f64>()
                    })
                    .sum();
                sum / t.area().max(1.0)
            })
            .collect();
        Ok((result.result().approx_ssim2(), difficulty))
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    fn full_reference(&self, i: usize) -> Result<&PrecomputedReference> {
        if let Some(pre) = self.full[i].get() {
            return Ok(pre);
        }
        let pre = self
            .zensim
            .precompute_reference(&view(&self.source, self.width, self.tiles[i])?)
            .map_err(metric_err)?;
        Ok(self.full[i].get_or_init(|| pre))
    }

    /// Full-resolution scores of the tiles at `indices` (same order).
    pub fn tile_scores(&self, frame: &Frame, indices: &[usize]) -> Result<Vec<f64>> {
        indices
            .par_iter()
            .map(|&i| {
                let t = *self
                    .tiles
                    .get(i)
                    .ok_or_else(|| FinoError::Metric("tile index out of range".into()))?;
                let pre = self.full_reference(i)?;
                let distorted = view(&frame.rgb, frame.width, t)?;
                with_scratch(|scratch| {
                    self.zensim
                        .compute_with_ref_into(pre, &distorted, scratch)
                        .map(|r| r.approx_ssim2())
                        .map_err(metric_err)
                })
            })
            .collect()
    }

    /// (area-weighted mean, worst) of `scores` for the tiles at `indices`.
    pub fn summarize(&self, indices: &[usize], scores: &[f64]) -> (f64, f64) {
        let weighted: Vec<(f64, f64)> = indices
            .iter()
            .zip(scores)
            .map(|(&i, &s)| (s, self.tiles[i].area()))
            .collect();
        let area: f64 = weighted.iter().map(|(_, a)| a).sum();
        let mean = weighted.iter().map(|(s, a)| s * a).sum::<f64>() / area.max(1.0);
        let worst = weighted
            .iter()
            .map(|(s, _)| *s)
            .fold(f64::INFINITY, f64::min);
        (mean, worst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_res_averages_2x2_blocks() {
        // 4×2 image: left block all 10/20/30, right block alternating 0 and 200.
        let rgb = [
            10, 20, 30, 10, 20, 30, 0, 0, 0, 200, 200, 200, //
            10, 20, 30, 10, 20, 30, 200, 200, 200, 0, 0, 0,
        ];
        let (small, w, h) = half_res(&rgb, 4, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(small, vec![10, 20, 30, 100, 100, 100]);
    }

    #[test]
    fn tiles_cover_the_frame_exactly_once() {
        for (w, h) in [(768, 512), (6000, 4000), (300, 9000), (257, 255)] {
            let tiles = tiles_for(w, h);
            let area: u64 = tiles.iter().map(|t| t.w as u64 * t.h as u64).sum();
            assert_eq!(area, w as u64 * h as u64, "{w}x{h}");
            assert!(tiles.iter().all(|t| t.x + t.w <= w && t.y + t.h <= h));
            assert!(tiles
                .iter()
                .all(|t| t.w >= MIN_EDGE.min(w) && t.h >= MIN_EDGE.min(h)));
        }
    }

    #[test]
    fn identical_images_score_100_and_noise_lowers_the_worst_tile() {
        let mut data = Vec::new();
        for y in 0..512u32 {
            for x in 0..512u32 {
                data.extend_from_slice(&[(x / 2) as u8, (y / 2) as u8, 128]);
            }
        }
        let clean = Pixels {
            data,
            width: 512,
            height: 512,
            channels: 3,
        };
        let reference = Reference::new(&clean).unwrap();
        let same = reference.compare(&clean).unwrap();
        assert!(
            same.global > 98.5 && same.mean > 98.5 && same.worst > 98.5,
            "{same:?}"
        );

        let mut damaged = clean.clone();
        for (i, px) in damaged.data.iter_mut().enumerate().take(512 * 3 * 64) {
            *px = px.wrapping_add(((i * 7919) % 41) as u8);
        }
        let hurt = reference.compare(&damaged).unwrap();
        assert!(hurt.worst < hurt.mean, "{hurt:?}");
        assert!(hurt.worst < 90.0, "{hurt:?}");
    }
}
