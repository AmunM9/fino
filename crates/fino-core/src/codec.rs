//! Pixel codecs: zune-jpeg to decode, mozjpeg (trellis quantization) to encode.

use crate::error::{FinoError, Result};
use mozjpeg::qtable::NRobidoux;
use std::panic::{catch_unwind, AssertUnwindSafe};
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

/// Interleaved 8-bit pixels; `channels` is 1 (gray) or 3 (RGB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub channels: u8,
}

impl Pixels {
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// RGB view for metrics; gray is expanded.
    pub fn to_rgb(&self) -> Vec<[u8; 3]> {
        match self.channels {
            1 => self.data.iter().map(|&y| [y, y, y]).collect(),
            _ => self
                .data
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
        }
    }
}

const MAX_DIMENSION: usize = u16::MAX as usize;

/// Decodes into `buffer`, reusing its allocation (search probes decode many candidates of
/// the same size).
pub fn decode_reusing(data: &[u8], grayscale: bool, buffer: Vec<u8>) -> Result<Pixels> {
    let out = if grayscale {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(out)
        .set_max_width(MAX_DIMENSION)
        .set_max_height(MAX_DIMENSION)
        .set_strict_mode(false);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder
        .decode_headers()
        .map_err(|e| FinoError::Decode(format!("{e:?}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| FinoError::Decode("missing image info".into()))?;
    let channels: u8 = if grayscale { 1 } else { 3 };
    let len = info.width as usize * info.height as usize * channels as usize;
    let mut data_out = buffer;
    data_out.resize(len, 0);
    decoder
        .decode_into(&mut data_out)
        .map_err(|e| FinoError::Decode(format!("{e:?}")))?;
    Ok(Pixels {
        data: data_out,
        width: info.width as u32,
        height: info.height as u32,
        channels,
    })
}

pub fn decode(data: &[u8], grayscale: bool) -> Result<Pixels> {
    let out = if grayscale {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(out)
        .set_max_width(MAX_DIMENSION)
        .set_max_height(MAX_DIMENSION)
        .set_strict_mode(false);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
    let data = decoder
        .decode()
        .map_err(|e| FinoError::Decode(format!("{e:?}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| FinoError::Decode("missing image info".into()))?;
    let pixels = Pixels {
        data,
        width: info.width as u32,
        height: info.height as u32,
        channels: if grayscale { 1 } else { 3 },
    };
    if pixels.data.len() != pixels.pixel_count() as usize * pixels.channels as usize {
        return Err(FinoError::Decode("unexpected pixel buffer size".into()));
    }
    Ok(pixels)
}

/// How hard the encoder works. Only `Max` changes quantization decisions (trellis), so
/// `Probe` and `Fast` produce identical pixels at the same quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// libjpeg-turbo speed, baseline, standard Huffman: ~7× faster, for search probes only.
    Probe,
    /// libjpeg-turbo quantization written progressive with optimal Huffman tables.
    Fast,
    /// mozjpeg trellis quantization, progressive, optimal Huffman: smallest files.
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncodeParams {
    pub quality: f32,
    /// Chroma block size: (2, 2) = 4:2:0, (2, 1) = 4:2:2, (1, 1) = 4:4:4.
    pub chroma_block: (u8, u8),
    pub effort: Effort,
}

impl EncodeParams {
    pub fn new(quality: f32, chroma_block: (u8, u8), effort: Effort) -> Self {
        Self {
            quality,
            chroma_block,
            effort,
        }
    }
}

/// Quantization table (natural order) used at `quality` for every channel. N. Robidoux's
/// table is mozjpeg's default (better than Annex K for photos).
pub fn quant_table(quality: f32) -> [u16; 64] {
    let table = NRobidoux.scaled(quality, quality);
    // SAFETY: a QTable holds exactly 64 entries.
    let steps = unsafe { std::slice::from_raw_parts(table.as_ptr(), 64) };
    std::array::from_fn(|k| steps[k] as u16)
}

/// mozjpeg reports libjpeg errors by unwinding; convert them into `FinoError`.
fn guarded<T>(f: impl FnOnce() -> std::io::Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(FinoError::Encode(e.to_string())),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "libjpeg error".into());
            Err(FinoError::Encode(msg))
        }
    }
}

/// Encodes `pixels` with the given effort (see [`Effort`]).
pub fn encode(pixels: &Pixels, params: EncodeParams) -> Result<Vec<u8>> {
    guarded(|| {
        let space = if pixels.channels == 1 {
            mozjpeg::ColorSpace::JCS_GRAYSCALE
        } else {
            mozjpeg::ColorSpace::JCS_RGB
        };
        let mut c = mozjpeg::Compress::new(space);
        if params.effort != Effort::Max {
            c.set_fastest_defaults();
        }
        c.set_size(pixels.width as usize, pixels.height as usize);
        c.set_quality(params.quality);
        // Same quantization tables in every effort, so "quality q" means the same thing in a
        // fast search probe and in the final trellis encode. N. Robidoux's table is mozjpeg's
        // default (better than Annex K for photos).
        let table = NRobidoux.scaled(params.quality, params.quality);
        c.set_luma_qtable(&table);
        if pixels.channels == 3 {
            c.set_chroma_qtable(&table);
            c.set_chroma_sampling_pixel_sizes(params.chroma_block, params.chroma_block);
        }
        // Order matters: disabling scan optimization clears the scan script, so the plain
        // progressive script must be installed afterwards. (optimize_scans doubles encode
        // time for ~0.7 % on photos.)
        c.set_optimize_scans(false);
        if params.effort != Effort::Probe {
            c.set_progressive_mode();
        }
        c.set_optimize_coding(params.effort != Effort::Probe);
        let mut started = c.start_compress(Vec::with_capacity(pixels.data.len() / 8))?;
        started.write_scanlines(&pixels.data)?;
        started.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn gradient(width: u32, height: u32) -> Pixels {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[
                    (x * 255 / width) as u8,
                    (y * 255 / height) as u8,
                    ((x ^ y) & 0xFF) as u8,
                ]);
            }
        }
        Pixels {
            data,
            width,
            height,
            channels: 3,
        }
    }

    #[test]
    fn round_trips_dimensions_through_encode_and_decode() {
        let src = gradient(96, 64);
        let jpeg = encode(&src, EncodeParams::new(85.0, (2, 2), Effort::Max)).unwrap();
        let back = decode(&jpeg, false).unwrap();
        assert_eq!((back.width, back.height, back.channels), (96, 64, 3));
    }

    #[test]
    fn higher_quality_produces_larger_files() {
        let src = gradient(128, 128);
        let lo = encode(&src, EncodeParams::new(50.0, (2, 2), Effort::Max)).unwrap();
        let hi = encode(&src, EncodeParams::new(95.0, (2, 2), Effort::Max)).unwrap();
        assert!(hi.len() > lo.len());
    }

    #[test]
    fn final_efforts_are_progressive_and_fast_matches_probe_pixels() {
        let src = gradient(128, 96);
        let probe = encode(&src, EncodeParams::new(80.0, (2, 2), Effort::Probe)).unwrap();
        let fast = encode(&src, EncodeParams::new(80.0, (2, 2), Effort::Fast)).unwrap();
        let max = encode(&src, EncodeParams::new(80.0, (2, 2), Effort::Max)).unwrap();
        use crate::jpeg::{inspect, FrameKind};
        assert_eq!(inspect(&probe).unwrap().kind, FrameKind::Baseline);
        assert_eq!(inspect(&fast).unwrap().kind, FrameKind::Progressive);
        assert_eq!(inspect(&max).unwrap().kind, FrameKind::Progressive);
        assert_eq!(
            decode(&probe, false).unwrap().data,
            decode(&fast, false).unwrap().data
        );
        assert!(fast.len() < probe.len());
    }

    #[test]
    fn decode_reusing_matches_decode() {
        let jpeg = encode(
            &gradient(96, 64),
            EncodeParams::new(85.0, (2, 2), Effort::Probe),
        )
        .unwrap();
        let fresh = decode(&jpeg, false).unwrap();
        let reused = decode_reusing(&jpeg, false, vec![7; 10]).unwrap();
        assert_eq!(fresh, reused);
    }

    #[test]
    fn corrupt_input_is_an_error_not_a_crash() {
        let mut jpeg = encode(
            &gradient(64, 64),
            EncodeParams::new(80.0, (1, 1), Effort::Max),
        )
        .unwrap();
        let len = jpeg.len();
        jpeg[len / 3..len / 2].fill(0xAA);
        jpeg.truncate(len * 2 / 3);
        let _ = decode(&jpeg, false); // must not panic
    }
}
