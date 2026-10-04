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
use std::cell::RefCell;
use zensim::{
    PixelFormat, PrecomputedReference, StridedBytes, Zensim, ZensimProfile, ZensimScratch,
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
    /// Area-weighted mean of tile scores, SSIMULACRA 2 scale.
    pub mean: f64,
    /// Lowest tile score, SSIMULACRA 2 scale.
    pub worst: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Precomputed reference image, reused for every candidate of the search.
pub struct Reference {
    width: u32,
    height: u32,
    tiles: Vec<(Tile, PrecomputedReference)>,
    global: PrecomputedReference,
    /// 1 = full resolution, 2 = half.
    global_scale: u32,
    zensim: Zensim,
    zensim_parallel: Zensim,
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

fn rgb_bytes(pixels: &Pixels) -> std::borrow::Cow<'_, [u8]> {
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
    let (w2, h2) = (width / 2, height / 2);
    let stride = width as usize * 3;
    let rows: Vec<Vec<u8>> = (0..h2 as usize)
        .into_par_iter()
        .map(|y| {
            let top = &rgb[2 * y * stride..];
            let bottom = &rgb[(2 * y + 1) * stride..];
            let mut row = Vec::with_capacity(w2 as usize * 3);
            for x in 0..w2 as usize {
                for c in 0..3 {
                    let i = 6 * x + c;
                    let sum =
                        top[i] as u16 + top[i + 3] as u16 + bottom[i] as u16 + bottom[i + 3] as u16;
                    row.push(((sum + 2) / 4) as u8);
                }
            }
            row
        })
        .collect();
    (rows.concat(), w2, h2)
}

/// The frame the global score is computed on (possibly half resolution).
fn global_frame(
    rgb: &[u8],
    width: u32,
    height: u32,
    scale: u32,
) -> (std::borrow::Cow<'_, [u8]>, Tile) {
    if scale == 2 {
        let (small, w, h) = half_res(rgb, width, height);
        (small.into(), Tile { x: 0, y: 0, w, h })
    } else {
        (
            rgb.into(),
            Tile {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
        )
    }
}

fn view<'a>(rgb: &'a [u8], width: u32, t: Tile) -> Result<StridedBytes<'a>> {
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

impl Reference {
    pub fn new(pixels: &Pixels) -> Result<Self> {
        if pixels.width < 8 || pixels.height < 8 {
            return Err(FinoError::Metric("image smaller than 8×8".into()));
        }
        let zensim = Zensim::new(ZensimProfile::latest()).with_parallel(false);
        let rgb = rgb_bytes(pixels);
        let tiles = tiles_for(pixels.width, pixels.height)
            .into_par_iter()
            .map(|t| {
                let pre = zensim
                    .precompute_reference(&view(&rgb, pixels.width, t)?)
                    .map_err(|e| FinoError::Metric(e.to_string()))?;
                Ok((t, pre))
            })
            .collect::<Result<Vec<_>>>()?;
        let zensim_parallel = Zensim::new(ZensimProfile::latest());
        let global_scale = if pixels.width.min(pixels.height) >= HALF_RES_GLOBAL_MIN_SIDE {
            2
        } else {
            1
        };
        let (frame, area) = global_frame(&rgb, pixels.width, pixels.height, global_scale);
        let global = zensim_parallel
            .precompute_reference(&view(&frame, area.w, area)?)
            .map_err(|e| FinoError::Metric(e.to_string()))?;
        Ok(Self {
            width: pixels.width,
            height: pixels.height,
            tiles,
            global,
            global_scale,
            zensim,
            zensim_parallel,
        })
    }

    /// Full verdict: whole-frame score plus tile statistics.
    pub fn compare(&self, candidate: &Pixels) -> Result<Score> {
        let global = self.global(candidate)?;
        let (mean, worst) = self.tiles(candidate)?;
        Ok(Score {
            global,
            mean,
            worst,
        })
    }

    /// Whole-frame score only — cheap first gate inside the search.
    pub fn global(&self, candidate: &Pixels) -> Result<f64> {
        self.check_size(candidate)?;
        let rgb = rgb_bytes(candidate);
        let (frame, area) = global_frame(&rgb, self.width, self.height, self.global_scale);
        let distorted = view(&frame, area.w, area)?;
        with_scratch(|scratch| {
            self.zensim_parallel
                .compute_with_ref_into(&self.global, &distorted, scratch)
                .map(|r| r.approx_ssim2())
                .map_err(|e| FinoError::Metric(e.to_string()))
        })
    }

    /// (area-weighted mean, worst) over all tiles.
    pub fn tiles(&self, candidate: &Pixels) -> Result<(f64, f64)> {
        let all: Vec<usize> = (0..self.tiles.len()).collect();
        let scores = self.tile_scores(candidate, &all)?;
        Ok(self.summarize(&all, &scores))
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Scores of the tiles at `indices` (same order).
    pub fn tile_scores(&self, candidate: &Pixels, indices: &[usize]) -> Result<Vec<f64>> {
        self.check_size(candidate)?;
        let rgb = rgb_bytes(candidate);
        indices
            .par_iter()
            .map(|&i| {
                let (t, pre) = self
                    .tiles
                    .get(i)
                    .ok_or_else(|| FinoError::Metric("tile index out of range".into()))?;
                let distorted = view(&rgb, self.width, *t)?;
                with_scratch(|scratch| {
                    self.zensim
                        .compute_with_ref_into(pre, &distorted, scratch)
                        .map(|r| r.approx_ssim2())
                        .map_err(|e| FinoError::Metric(e.to_string()))
                })
            })
            .collect()
    }

    /// (area-weighted mean, worst) of `scores` for the tiles at `indices`.
    pub fn summarize(&self, indices: &[usize], scores: &[f64]) -> (f64, f64) {
        let weighted: Vec<(f64, f64)> = indices
            .iter()
            .zip(scores)
            .map(|(&i, &s)| (s, self.tiles[i].0.w as f64 * self.tiles[i].0.h as f64))
            .collect();
        let area: f64 = weighted.iter().map(|(_, a)| a).sum();
        let mean = weighted.iter().map(|(s, a)| s * a).sum::<f64>() / area.max(1.0);
        let worst = weighted
            .iter()
            .map(|(s, _)| *s)
            .fold(f64::INFINITY, f64::min);
        (mean, worst)
    }

    fn check_size(&self, candidate: &Pixels) -> Result<()> {
        if candidate.width != self.width || candidate.height != self.height {
            return Err(FinoError::Metric(
                "candidate size differs from reference".into(),
            ));
        }
        Ok(())
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
