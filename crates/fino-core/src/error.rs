use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why a file was left untouched. Skips are expected outcomes, not failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Carries the Fino marker from a previous run.
    AlreadyOptimized,
    /// Re-encoding would not save enough bytes to be worth a generation.
    NoGain,
    /// Not a JPEG.
    Unsupported,
    /// CMYK / YCCK JPEGs are passed through untouched.
    Cmyk,
    /// 12-bit, arithmetic-coded or lossless JPEG variants.
    ExoticJpeg,
    /// Exceeds the pixel budget.
    TooLarge,
    /// Carries an HDR gain map that a re-encode would silently drop.
    HdrGainMap,
    /// Embeds a video or other media after the image (Motion Photos, Samsung trailers).
    EmbeddedMedia,
    /// HEIC whose primary image is HDR (PQ/HLG): an 8-bit JPEG cannot hold it.
    HdrPhoto,
    /// One eye of a spatial (stereo) photo; a JPEG could keep only one.
    SpatialPhoto,
    /// HEIC conversion is turned off in the settings.
    ConversionOff,
}

#[derive(Debug, Error)]
pub enum FinoError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode image: {0}")]
    Decode(String),
    #[error("could not encode image: {0}")]
    Encode(String),
    #[error("image metric failed: {0}")]
    Metric(String),
    #[error("could not resize image: {0}")]
    Resize(String),
    #[error("malformed JPEG: {0}")]
    Malformed(&'static str),
}

pub type Result<T> = std::result::Result<T, FinoError>;
