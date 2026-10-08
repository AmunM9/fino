//! Perceptual quality search: the smallest JPEG that still looks like the reference,
//! both as a whole and in its worst tile.
//!
//! The search is seeded with a likely answer (usually the previous photo's), then jumps to
//! where the targets are predicted to be crossed. JPEG sources are re-quantized in the DCT
//! domain; probes are written baseline with standard Huffman tables and the winner
//! progressive with optimal ones, which changes no pixel — so its verdict carries over
//! unverified.
//!
//! Only the hardest tiles are scored at full resolution, picked from the per-pixel error map
//! of the half-resolution whole-frame pass. Near the answer the tiles bind more often than
//! the whole frame, so a candidate failing them never pays for the whole-frame pass; one
//! that passes is re-ranked at its own quality before its verdict is trusted.
//!
//! mozjpeg trellis was measured and dropped from the pipeline: at the quality needed to
//! pass the same perceptual targets it saved little, and verifying it cost ~2.5 s per
//! 24 MP photo.

use crate::codec::{self, Effort, EncodeParams, Pixels};
use crate::error::Result;
use crate::metric::{Reference, Score};
use crate::options::{Strength, Targets};
use crate::requant::{Coding, Coefficients};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub jpeg: Vec<u8>,
    pub quality: u8,
    pub score: Score,
    pub effort: Effort,
}

/// Tiles scored at full resolution: the hardest sixth of the frame by the whole-frame
/// error map, at least `WATCH_MIN`. Measured on 24 MP photos: the worst full-resolution tile
/// among the watched ones sits within ~0.1 points of the true worst on average.
const WATCH_SHARE: usize = 6;
const WATCH_MIN: usize = 8;
/// Typical change of the binding score (usually the worst tile) per quality step near the
/// answer, used to predict where the targets are crossed.
const SLOPE: f64 = 0.8;
/// A passing quality this close to its targets is the answer without probing the step
/// below, which is then all but certain to fail. Where it would not (the quality scale has
/// plateaus where neighbouring steps encode almost alike), the file is barely larger.
const SETTLE_SLACK: f64 = 0.7;
/// Two probes on the same side whose slack differs by less than this sit on a plateau.
const PLATEAU: f64 = 0.3;

/// Inclusive quality range explored for each strength.
pub fn quality_range(strength: Strength) -> (u8, u8) {
    match strength {
        Strength::Pristine => (55, 98),
        Strength::Identical => (45, 97),
        Strength::Compact => (35, 95),
    }
}

/// Where most photos land; starting here saves several probes per image.
fn prior(strength: Strength) -> u8 {
    match strength {
        Strength::Pristine => 88,
        Strength::Identical => 78,
        Strength::Compact => 72,
    }
}

/// The hardest tiles by difficulty (hardest first).
fn watch_list(difficulty: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..difficulty.len()).collect();
    order.sort_by(|&a, &b| difficulty[b].total_cmp(&difficulty[a]));
    order.truncate((difficulty.len() / WATCH_SHARE).max(WATCH_MIN));
    order
}

/// Where candidates come from.
pub enum Encoder<'a> {
    /// Pixels encoded afresh: other formats and resized output.
    Pixels {
        pixels: &'a Pixels,
        chroma_block: (u8, u8),
    },
    /// A JPEG's own coefficients re-quantized in the DCT domain: cheaper probes, and no
    /// generation loss beyond the new quantization.
    Requantize(&'a mut Coefficients),
}

impl Encoder<'_> {
    fn encode(&mut self, quality: u8, effort: Effort) -> Result<Vec<u8>> {
        match self {
            Encoder::Pixels {
                pixels,
                chroma_block,
            } => codec::encode(
                pixels,
                EncodeParams::new(quality as f32, *chroma_block, effort),
            ),
            Encoder::Requantize(coefficients) => {
                let table = codec::quant_table(quality as f32);
                let coding = match effort {
                    Effort::Probe => Coding::Probe,
                    Effort::Fast | Effort::Max => Coding::Final,
                };
                coefficients.encode(&table, &table, coding)
            }
        }
    }

    fn grayscale(&self) -> bool {
        match self {
            Encoder::Pixels { pixels, .. } => pixels.channels == 1,
            Encoder::Requantize(coefficients) => coefficients.is_grayscale(),
        }
    }
}

struct Probe<'a> {
    encoder: Encoder<'a>,
    reference: &'a Reference<'a>,
    targets: Targets,
    /// Hardest tiles: ranked on the first candidate, joined by those that rank among the
    /// hardest at any passing quality.
    watch: Option<Vec<usize>>,
    /// quality → slack (distance above the targets; negative = fails) and, when it passed,
    /// its score.
    seen: BTreeMap<u8, (f64, Option<Score>)>,
    /// Reused decode buffer.
    buffer: Vec<u8>,
}

impl<'a> Probe<'a> {
    fn new(encoder: Encoder<'a>, reference: &'a Reference, targets: Targets) -> Self {
        Self {
            encoder,
            reference,
            targets,
            watch: None,
            seen: BTreeMap::new(),
            buffer: Vec::new(),
        }
    }

    fn decoded(&mut self, quality: u8) -> Result<Pixels> {
        let jpeg = self.encoder.encode(quality, Effort::Probe)?;
        let buffer = std::mem::take(&mut self.buffer);
        codec::decode_reusing(&jpeg, self.encoder.grayscale(), buffer)
    }

    /// Score at `quality` and its slack (≥ 0 passes). With `gate`, a candidate failing the
    /// watched tiles returns early, without a score.
    fn score(&mut self, quality: u8, gate: bool) -> Result<(f64, Option<Score>)> {
        let decoded = self.decoded(quality)?;
        let frame = self.reference.frame(&decoded)?;
        // The first candidate learns which tiles are hardest from the whole-frame pass.
        let (global, mut watch) = match self.watch.take() {
            Some(watch) => (None, watch),
            None => {
                let (global, difficulty) = self.reference.global_and_difficulty(&frame)?;
                (Some(global), watch_list(&difficulty))
            }
        };
        let mut scores = self.reference.tile_scores(&frame, &watch)?;
        let worst = scores.iter().copied().fold(f64::INFINITY, f64::min);
        let tile_slack = worst - self.targets.worst_tile;
        let global_slack = |global: f64| global - self.targets.global;
        if gate && tile_slack < 0.0 {
            self.watch = Some(watch);
            drop(frame);
            self.buffer = decoded.data;
            let slack = global.map_or(tile_slack, |g| tile_slack.min(global_slack(g)));
            return Ok((slack, None));
        }
        // The ranking shifts with quality: a verdict that may be the answer is checked
        // against the hardest tiles at its own quality too.
        let global = match global {
            Some(global) => global,
            None => {
                let (global, difficulty) = self.reference.global_and_difficulty(&frame)?;
                let extra: Vec<usize> = watch_list(&difficulty)
                    .into_iter()
                    .filter(|i| !watch.contains(i))
                    .collect();
                scores.extend(self.reference.tile_scores(&frame, &extra)?);
                watch.extend(extra);
                global
            }
        };
        let (mean, worst) = self.reference.summarize(&watch, &scores);
        self.watch = Some(watch);
        let score = Score {
            global,
            mean,
            worst,
        };
        let result = (
            (worst - self.targets.worst_tile).min(global_slack(global)),
            Some(score),
        );
        drop(frame);
        self.buffer = decoded.data;
        Ok(result)
    }

    /// Slack at `quality` (≥ 0 passes), probing it once.
    fn slack(&mut self, quality: u8) -> Result<f64> {
        if let Some(&(slack, _)) = self.seen.get(&quality) {
            return Ok(slack);
        }
        let (slack, score) = self.score(quality, true)?;
        // A metric failure (NaN) counts as a clear fail rather than stalling the search.
        let slack = if slack.is_nan() {
            f64::NEG_INFINITY
        } else {
            slack
        };
        self.seen
            .insert(quality, (slack, score.filter(|_| slack >= 0.0)));
        Ok(slack)
    }

    /// Lowest passing quality in `[min_q, max_q]`, or `None` if even `max_q` fails.
    ///
    /// Each step jumps to where the slack is predicted to cross zero: from the typical slope
    /// at first, then by interpolating between the closest failing and passing qualities.
    /// The quality scale has plateaus where the score barely moves; walking along one, the
    /// step doubles instead.
    fn lowest_passing(&mut self, start: u8, min_q: u8, max_q: u8) -> Result<Option<u8>> {
        let predicted = |slack: f64| (slack.abs() / SLOPE).clamp(1.0, 20.0);
        // Closest failing and passing probes so far: (quality, slack).
        let mut low: Option<(u8, f64)> = None;
        let mut high: Option<(u8, f64)> = None;
        let mut quality = start;
        loop {
            let slack = self.slack(quality)?;
            // The previous probe on the same side, if the bracket is still open.
            let previous = if slack >= 0.0 {
                high.replace((quality, slack)).filter(|_| low.is_none())
            } else {
                low.replace((quality, slack)).filter(|_| high.is_none())
            };
            let step = |predicted: f64| {
                let step = (predicted as u8).max(1);
                match previous {
                    Some((q, s)) if (s - slack).abs() < PLATEAU => {
                        step.max(2 * q.abs_diff(quality))
                    }
                    _ => step,
                }
            };
            quality = match (low, high) {
                (Some((l, _)), Some((h, _))) if h - l <= 1 => return Ok(Some(h)),
                (_, Some((h, s))) if s < SETTLE_SLACK || h <= min_q => return Ok(Some(h)),
                (Some((l, ls)), Some((h, hs))) => {
                    let cross = l as f64 + -ls * (h - l) as f64 / (hs - ls);
                    (cross.ceil() as u8).clamp(l + 1, h - 1)
                }
                (None, Some((h, s))) => h.saturating_sub(step(predicted(s).floor())).max(min_q),
                (Some((l, _)), None) if l >= max_q => return Ok(None),
                (Some((l, s)), None) => l.saturating_add(step(predicted(s).ceil())).min(max_q),
                (None, None) => unreachable!("every probe sets one side"),
            };
        }
    }
}

/// Returns the smallest candidate meeting the strength's targets, or `None` if even the top
/// of the range fails (matching the source would cost more bytes than it saves).
///
/// `hint` (usually the previous photo's answer) seeds the search, so photos from one shoot
/// typically settle in two probes.
pub fn find_smallest(
    pixels: &Pixels,
    reference: &Reference,
    chroma_block: (u8, u8),
    strength: Strength,
    hint: Option<u8>,
) -> Result<Option<Candidate>> {
    let encoder = Encoder::Pixels {
        pixels,
        chroma_block,
    };
    find_smallest_with(encoder, reference, strength, hint, strength.targets())
}

/// How far below the best reachable score each strength may go, (global, worst tile).
fn ceiling_margin(strength: Strength) -> (f64, f64) {
    match strength {
        Strength::Pristine => (0.5, 1.0),
        Strength::Identical => (1.5, 3.0),
        Strength::Compact => (3.0, 6.0),
    }
}

/// Targets for sources that are not JPEGs (HEIC): every JPEG of them adds rounding noise, so
/// the absolute targets can be out of reach even at the top quality. Aim for the best score
/// actually reachable minus the strength's margin, never above the absolute targets.
pub fn ceiling_targets(
    pixels: &Pixels,
    reference: &Reference,
    chroma_block: (u8, u8),
    strength: Strength,
) -> Result<Targets> {
    let encoder = Encoder::Pixels {
        pixels,
        chroma_block,
    };
    let best = top_quality(encoder, reference, strength)?.score;
    let absolute = strength.targets();
    let (global, worst) = ceiling_margin(strength);
    Ok(Targets {
        global: absolute.global.min(best.global - global),
        worst_tile: absolute.worst_tile.min(best.worst - worst),
    })
}

/// `find_smallest` against explicit targets.
pub fn find_smallest_with(
    encoder: Encoder,
    reference: &Reference,
    strength: Strength,
    hint: Option<u8>,
    targets: Targets,
) -> Result<Option<Candidate>> {
    let (min_q, max_q) = quality_range(strength);
    let mut probe = Probe::new(encoder, reference, targets);
    let start = hint.unwrap_or(prior(strength)).clamp(min_q, max_q);
    let Some(quality) = probe.lowest_passing(start, min_q, max_q)? else {
        return Ok(None);
    };
    let Some(&(_, Some(score))) = probe.seen.get(&quality) else {
        return Ok(None);
    };
    Ok(Some(finalize(&mut probe.encoder, quality, score)?))
}

/// Same quantization as the passing probe, rewritten progressive with optimal Huffman
/// tables — the same pixels, so the probe's verdict carries over.
fn finalize(encoder: &mut Encoder, quality: u8, score: Score) -> Result<Candidate> {
    let jpeg = encoder.encode(quality, Effort::Fast)?;
    Ok(Candidate {
        jpeg,
        quality,
        score,
        effort: Effort::Fast,
    })
}

/// Remembers recent answers so the next photo of a batch can start its search there.
#[derive(Debug, Default)]
pub struct QualityHint(std::sync::atomic::AtomicU8);

impl QualityHint {
    pub fn get(&self) -> Option<u8> {
        Some(self.0.load(std::sync::atomic::Ordering::Relaxed)).filter(|&q| q > 0)
    }

    pub fn record(&self, quality: u8) {
        self.0.store(quality, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Best effort at the top of the range — used when a resize was requested and an output
/// must be produced even if the strict targets cannot be met.
pub fn top_quality(
    encoder: Encoder,
    reference: &Reference,
    strength: Strength,
) -> Result<Candidate> {
    let quality = quality_range(strength).1;
    let mut probe = Probe::new(encoder, reference, strength.targets());
    let score = probe
        .score(quality, false)?
        .1
        .ok_or_else(|| crate::FinoError::Metric("no score at the top quality".into()))?;
    finalize(&mut probe.encoder, quality, score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo_like(width: u32, height: u32) -> Pixels {
        // Smooth gradients plus deterministic "texture" so quality actually matters.
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let n = ((x * 7 + y * 13) ^ (x * y)) % 23;
                data.push((x * 200 / width + n) as u8);
                data.push((y * 200 / height + n) as u8);
                data.push((((x + y) / 4) % 200 + n) as u8);
            }
        }
        Pixels {
            data,
            width,
            height,
            channels: 3,
        }
    }

    fn camera_file() -> Pixels {
        // Like a camera file: the reference is itself a decoded high-quality JPEG.
        let jpeg = codec::encode(
            &photo_like(320, 240),
            EncodeParams::new(92.0, (2, 2), Effort::Max),
        )
        .unwrap();
        codec::decode(&jpeg, false).unwrap()
    }

    #[test]
    fn watch_list_keeps_the_hardest_sixth_hardest_first() {
        let mut difficulty = vec![1.0; 60];
        difficulty[7] = 9.0; // hardest
        difficulty[3] = 8.0;
        let watch = watch_list(&difficulty);
        assert_eq!(watch.len(), 10, "a sixth of 60");
        assert_eq!(&watch[..2], &[7, 3]);
        assert_eq!(watch_list(&[1.0; 30]).len(), 8, "at least WATCH_MIN");
        assert_eq!(
            watch_list(&[1.0; 5]).len(),
            5,
            "never more tiles than exist"
        );
    }

    #[test]
    fn any_hint_still_meets_the_targets() {
        let px = camera_file();
        let reference = Reference::new(&px).unwrap();
        let t = Strength::Identical.targets();
        for hint in [None, Some(50), Some(80), Some(97)] {
            let found = find_smallest(&px, &reference, (2, 2), Strength::Identical, hint)
                .unwrap()
                .unwrap();
            let score = reference
                .compare(&codec::decode(&found.jpeg, false).unwrap())
                .unwrap();
            assert!(
                score.global >= t.global && score.worst >= t.worst_tile,
                "hint {hint:?}: {score:?}"
            );
        }
    }

    #[test]
    fn quality_hint_starts_empty_and_remembers_the_last_answer() {
        let hint = QualityHint::default();
        assert_eq!(hint.get(), None);
        hint.record(77);
        assert_eq!(hint.get(), Some(77));
    }

    #[test]
    fn stricter_strength_never_picks_a_lower_quality() {
        let px = camera_file();
        let reference = Reference::new(&px).unwrap();
        let identical = find_smallest(&px, &reference, (2, 2), Strength::Pristine, None).unwrap();
        let compact = find_smallest(&px, &reference, (2, 2), Strength::Compact, None).unwrap();
        let (Some(identical), Some(compact)) = (identical, compact) else {
            panic!("both strengths should find a candidate");
        };
        assert!(identical.quality >= compact.quality);
        assert!(identical.jpeg.len() >= compact.jpeg.len());
        let t = Strength::Pristine.targets();
        assert!(identical.score.global >= t.global && identical.score.worst >= t.worst_tile);
    }

    #[test]
    fn result_is_the_lowest_passing_quality() {
        let px = camera_file();
        let reference = Reference::new(&px).unwrap();
        let found = find_smallest(&px, &reference, (2, 2), Strength::Identical, None)
            .unwrap()
            .unwrap();
        let below = found.quality - 1;
        let jpeg =
            codec::encode(&px, EncodeParams::new(below as f32, (2, 2), Effort::Max)).unwrap();
        let score = reference
            .compare(&codec::decode(&jpeg, false).unwrap())
            .unwrap();
        let t = Strength::Identical.targets();
        assert!(
            score.global < t.global || score.worst < t.worst_tile,
            "q{below} also passes: {score:?}"
        );
    }
}
