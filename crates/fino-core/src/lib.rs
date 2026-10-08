//! Fino's compression engine: perceptually lossless JPEG recompression.
//!
//! Iterate the encoder, judge every candidate with a perceptual metric tile by tile,
//! keep the smallest one that adds no visible artifacts — built on open components:
//! mozjpeg / libjpeg-turbo and zensim (SSIMULACRA 2 approximation).

pub mod codec;
mod error;
pub mod files;
pub mod heic;
pub mod jpeg;
pub mod lossless;
pub mod metric;
mod optimize;
mod options;
pub mod parallel;
pub mod requant;
pub mod resize;
pub mod search;

pub use error::{FinoError, Result, SkipReason};
pub use metric::Score;
pub use optimize::{optimize, prepare, Optimized, Outcome, Prepared};
pub use options::{OptimizeOptions, Resize, ResizeMode, Strength, Targets};

/// Written into every output's COM marker, e.g. `Fino/0.1.0 q=82 s=91.4`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
