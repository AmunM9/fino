use serde::{Deserialize, Serialize};

/// How far Fino may go. Every level is a *perceptual* guarantee, not a quality number:
/// the encoder searches for the smallest file that still clears both thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strength {
    /// Visually lossless even when flickering between original and result.
    Pristine,
    /// Indistinguishable when viewed in place.
    #[default]
    Identical,
    /// Indistinguishable side by side at normal viewing size.
    Compact,
}

/// Perceptual targets on zensim's SSIMULACRA 2 approximation, calibrated so the *true*
/// SSIMULACRA 2 of the output lands at ≈ 89 (Pristine), ≈ 86 (Identical) and ≈ 82 (Compact)
/// on 24 MP camera files (see the `calibrate` and `vs_reference` examples).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Targets {
    /// Whole-frame score.
    pub global: f64,
    /// Score of the single worst tile — the eye goes straight to local defects.
    pub worst_tile: f64,
}

impl Strength {
    pub fn targets(self) -> Targets {
        match self {
            Strength::Pristine => Targets {
                global: 94.0,
                worst_tile: 91.0,
            },
            Strength::Identical => Targets {
                global: 91.5,
                worst_tile: 86.5,
            },
            Strength::Compact => Targets {
                global: 89.0,
                worst_tile: 82.5,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResizeMode {
    LongEdge,
    MaxWidth,
    MaxHeight,
}

/// Output dimensions, expressed in *displayed* orientation (after EXIF rotation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resize {
    pub mode: ResizeMode,
    pub pixels: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeOptions {
    pub strength: Strength,
    /// `None` keeps the original dimensions. Never upscales.
    pub resize: Option<Resize>,
    /// Remove GPS coordinates from EXIF and XMP.
    pub strip_location: bool,
    /// Pass through files that already carry the Fino marker.
    pub skip_optimized: bool,
    /// Minimum fractional saving (0.03 = 3 %) to accept a re-encode at original size.
    pub min_gain: f64,
    /// Where the quality search starts — typically the answer for the previous photo of the
    /// same batch, which shoots from one camera usually share. `None` uses the strength's
    /// default.
    #[serde(default)]
    pub quality_hint: Option<u8>,
    /// Refuse images above this many pixels. Each in-flight photo needs ~16 bytes per
    /// pixel (pixels + metric planes), so this bounds memory with parallel files.
    pub max_pixels: u64,
    /// HEIC photos become JPEGs (for sharing) instead of being left alone. Off by default:
    /// HEIC is already the lighter format, so converting makes files bigger.
    #[serde(default)]
    pub convert_heic: bool,
}

impl Default for OptimizeOptions {
    fn default() -> Self {
        Self {
            strength: Strength::Identical,
            resize: None,
            strip_location: false,
            skip_optimized: true,
            min_gain: 0.03,
            quality_hint: None,
            max_pixels: 120_000_000,
            convert_heic: false,
        }
    }
}
