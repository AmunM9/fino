//! Perceptual quality search: the smallest JPEG that still looks like the reference,
//! both as a whole and in its worst tile.
//!
//! The search is seeded with a likely answer, then brackets and bisects the encoder
//! quality. Every probe uses the fast libjpeg-turbo path; the winner is written progressive with optimal
//! Huffman tables, which changes no pixel — so its verdict carries over unverified.
//! Candidates failing the cheap whole-frame gate never pay for the tile pass.
//!
//! mozjpeg trellis was measured and dropped from the pipeline: at the quality needed to
//! pass the same perceptual targets it saved little, and verifying it cost ~2.5 s per
//! 24 MP photo.

use crate::codec::{self, Effort, EncodeParams, Pixels};
use crate::error::Result;
use crate::metric::{Reference, Score};
use crate::options::{Strength, Targets};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub jpeg: Vec<u8>,
    pub quality: u8,
    pub score: Score,
    pub effort: Effort,
}

/// The first probe that clears the whole-frame gate scores every tile; later probes only
/// re-check the hardest ones — the worst quarter plus any tile within `WATCH_MARGIN` of the
/// worst. Before a watch-list verdict is accepted as the answer, the remaining tiles are
/// checked too, so the guarantee always covers every tile.
const WATCH_SHARE: usize = 4;
const WATCH_MIN: usize = 6;
const WATCH_MARGIN: f64 = 1.5;
/// If the full check rejects a watch-list winner, try this many qualities above it.
const CONFIRM_STEPS: u8 = 3;

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

fn passes(score: Score, targets: Targets) -> bool {
    score.global >= targets.global && score.worst >= targets.worst_tile
}

/// The hardest tiles by score (lowest first): the worst quarter, plus every tile within
/// `WATCH_MARGIN` of the worst.
fn watch_list(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    let worst = order.first().map_or(f64::INFINITY, |&i| scores[i]);
    let close = order
        .iter()
        .take_while(|&&i| scores[i] <= worst + WATCH_MARGIN)
        .count();
    order.truncate((scores.len() / WATCH_SHARE).max(WATCH_MIN).max(close));
    order
}

struct Probe<'a> {
    pixels: &'a Pixels,
    reference: &'a Reference,
    chroma_block: (u8, u8),
    targets: Targets,
    /// Hardest tiles, learned from the first full tile pass.
    watch: Option<Vec<usize>>,
    /// quality → score if it passed (failures are recorded as `None`).
    seen: BTreeMap<u8, Option<Score>>,
    /// Qualities whose passing verdict covered every tile.
    complete: BTreeSet<u8>,
    /// Decoded pixels of the lowest quality that passed on the watch list only, kept so its
    /// final check needs neither a re-encode nor a re-decode.
    lowest_watched: Option<(u8, Pixels)>,
    /// Reused decode buffer.
    buffer: Vec<u8>,
}

impl Probe<'_> {
    fn all_tiles(&self) -> Vec<usize> {
        (0..self.reference.tile_count()).collect()
    }

    fn decoded(&mut self, quality: u8) -> Result<Pixels> {
        let params = EncodeParams::new(quality as f32, self.chroma_block, Effort::Probe);
        let jpeg = codec::encode(self.pixels, params)?;
        let buffer = std::mem::take(&mut self.buffer);
        codec::decode_reusing(&jpeg, self.pixels.channels == 1, buffer)
    }

    /// Hands a decoded candidate's allocation back for the next probe.
    fn recycle(&mut self, decoded: Pixels) {
        self.buffer = decoded.data;
    }

    fn score_tiles(&self, decoded: &Pixels, global: f64, tiles: &[usize]) -> Result<Score> {
        let scores = self.reference.tile_scores(decoded, tiles)?;
        let (mean, worst) = self.reference.summarize(tiles, &scores);
        Ok(Score {
            global,
            mean,
            worst,
        })
    }

    /// First full pass: scores every tile and learns the watch list from it.
    fn full_verdict(&mut self, decoded: &Pixels, global: f64) -> Result<Option<Score>> {
        let all = self.all_tiles();
        let scores = self.reference.tile_scores(decoded, &all)?;
        let (mean, worst) = self.reference.summarize(&all, &scores);
        self.watch = Some(watch_list(&scores));
        let score = Score {
            global,
            mean,
            worst,
        };
        Ok(passes(score, self.targets).then_some(score))
    }

    fn passes(&mut self, quality: u8) -> Result<bool> {
        if let Some(seen) = self.seen.get(&quality) {
            return Ok(seen.is_some());
        }
        let decoded = self.decoded(quality)?;
        let global = self.reference.global(&decoded)?;
        let (verdict, complete) = if global < self.targets.global {
            (None, false)
        } else if let Some(watch) = &self.watch {
            let score = self.score_tiles(&decoded, global, watch)?;
            (passes(score, self.targets).then_some(score), false)
        } else {
            (self.full_verdict(&decoded, global)?, true)
        };
        if complete && verdict.is_some() {
            self.complete.insert(quality);
        }
        let watched_pass = verdict.is_some() && !complete;
        let lower = self
            .lowest_watched
            .as_ref()
            .is_none_or(|(q, _)| quality < *q);
        if watched_pass && lower {
            if let Some((_, old)) = self.lowest_watched.replace((quality, decoded)) {
                self.recycle(old);
            }
        } else {
            self.recycle(decoded);
        }
        self.seen.insert(quality, verdict);
        Ok(verdict.is_some())
    }

    /// Verdict at `quality` covering every tile. Reuses the watch-list result and decoded
    /// pixels when available, so only the tiles not yet checked are scored.
    fn confirm(&mut self, quality: u8) -> Result<Option<Score>> {
        if self.complete.contains(&quality) {
            return Ok(self.seen.get(&quality).copied().flatten());
        }
        let watched = self.seen.get(&quality).copied().flatten();
        let cached = match self.lowest_watched.take() {
            Some((q, px)) if q == quality => Some(px),
            other => {
                self.lowest_watched = other;
                None
            }
        };
        let (decoded, known) = match (cached, watched, &self.watch) {
            (Some(px), Some(score), Some(watch)) => (px, Some((score, watch.clone()))),
            _ => (self.decoded(quality)?, None),
        };
        let score = match known {
            Some((score, watch)) => {
                let rest: Vec<usize> = self
                    .all_tiles()
                    .into_iter()
                    .filter(|i| !watch.contains(i))
                    .collect();
                let rest_score = self.score_tiles(&decoded, score.global, &rest)?;
                let (tiles, total) = (watch.len() as f64, (watch.len() + rest.len()) as f64);
                Score {
                    global: score.global,
                    mean: (score.mean * tiles + rest_score.mean * (total - tiles)) / total,
                    worst: score.worst.min(rest_score.worst),
                }
            }
            None => {
                let global = self.reference.global(&decoded)?;
                self.score_tiles(&decoded, global, &self.all_tiles())?
            }
        };
        self.recycle(decoded);
        Ok(passes(score, self.targets).then_some(score))
    }

    /// Lowest passing quality in `[min_q, max_q]`, or `None` if even `max_q` fails.
    fn lowest_passing(
        &mut self,
        start: u8,
        first_step: u8,
        min_q: u8,
        max_q: u8,
    ) -> Result<Option<u8>> {
        // 1. Bracket: a failing `low` and a passing `high`, galloping away from `start`.
        let (mut low, mut high);
        if self.passes(start)? {
            high = start;
            let mut step = first_step;
            loop {
                let q = high.saturating_sub(step).max(min_q);
                if q == high {
                    return Ok(Some(high)); // already at the floor
                }
                if self.passes(q)? {
                    high = q;
                    step *= 2;
                } else {
                    low = q;
                    break;
                }
            }
        } else {
            low = start;
            let mut step = first_step;
            loop {
                let q = (low + step).min(max_q);
                if q == low {
                    return Ok(None); // even the ceiling fails
                }
                if self.passes(q)? {
                    high = q;
                    break;
                }
                low = q;
                step *= 2;
            }
        }
        // 2. Bisect the open interval (low, high).
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if self.passes(mid)? {
                high = mid;
            } else {
                low = mid;
            }
        }
        Ok(Some(high))
    }
}

/// Returns the smallest candidate meeting the strength's targets, or `None` if even the top
/// of the range fails (matching the source would cost more bytes than it saves).
///
/// `hint` (usually the previous photo's answer) seeds the search and makes the first
/// steps ±1, so photos from one shoot typically settle in two or three probes.
pub fn find_smallest(
    pixels: &Pixels,
    reference: &Reference,
    chroma_block: (u8, u8),
    strength: Strength,
    hint: Option<u8>,
) -> Result<Option<Candidate>> {
    let targets = strength.targets();
    find_smallest_with(pixels, reference, chroma_block, strength, hint, targets)
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
    let best = top_quality(pixels, reference, chroma_block, strength)?.score;
    let absolute = strength.targets();
    let (global, worst) = ceiling_margin(strength);
    Ok(Targets {
        global: absolute.global.min(best.global - global),
        worst_tile: absolute.worst_tile.min(best.worst - worst),
    })
}

/// `find_smallest` against explicit targets.
pub fn find_smallest_with(
    pixels: &Pixels,
    reference: &Reference,
    chroma_block: (u8, u8),
    strength: Strength,
    hint: Option<u8>,
    targets: Targets,
) -> Result<Option<Candidate>> {
    let (min_q, max_q) = quality_range(strength);
    let mut probe = Probe {
        pixels,
        reference,
        chroma_block,
        targets,
        watch: None,
        seen: BTreeMap::new(),
        complete: BTreeSet::new(),
        lowest_watched: None,
        buffer: Vec::new(),
    };
    let (start, first_step) = match hint {
        Some(q) => (q, 1),
        None => (prior(strength), 2),
    };
    let start = start.clamp(min_q, max_q);
    let Some(found) = probe.lowest_passing(start, first_step, min_q, max_q)? else {
        return Ok(None);
    };
    // The guarantee covers every tile: a watch-list verdict is confirmed in full first.
    let mut verified = None;
    for q in found..=found.saturating_add(CONFIRM_STEPS).min(max_q) {
        if let Some(score) = probe.confirm(q)? {
            verified = Some((q, score));
            break;
        }
    }
    let Some((quality, score)) = verified else {
        return Ok(None);
    };
    // Same quantization as the passing probe, written progressive with optimal Huffman tables.
    let jpeg = codec::encode(
        pixels,
        EncodeParams::new(quality as f32, chroma_block, Effort::Fast),
    )?;
    Ok(Some(Candidate {
        jpeg,
        quality,
        score,
        effort: Effort::Fast,
    }))
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
    pixels: &Pixels,
    reference: &Reference,
    chroma_block: (u8, u8),
    strength: Strength,
) -> Result<Candidate> {
    let quality = quality_range(strength).1;
    let jpeg = codec::encode(
        pixels,
        EncodeParams::new(quality as f32, chroma_block, Effort::Fast),
    )?;
    let score = reference.compare(&codec::decode(&jpeg, pixels.channels == 1)?)?;
    Ok(Candidate {
        jpeg,
        quality,
        score,
        effort: Effort::Fast,
    })
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
    fn watch_list_keeps_the_worst_quarter_and_everything_near_the_worst() {
        let mut scores = vec![95.0; 30];
        scores[7] = 80.0; // worst
        scores[3] = 81.0; // within the margin
        let watch = watch_list(&scores);
        assert_eq!(watch.len(), 7);
        assert_eq!(&watch[..2], &[7, 3]);

        let mut tight = vec![90.0; 30];
        tight[0] = 89.0; // every tile within the margin → all watched
        assert_eq!(watch_list(&tight).len(), 30);
        assert_eq!(
            watch_list(&[90.0; 5]).len(),
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
