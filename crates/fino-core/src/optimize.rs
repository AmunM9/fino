//! Bytes-in, bytes-out optimization of a single JPEG.

use crate::codec::{self, Pixels};
use crate::error::{FinoError, Result, SkipReason};
use crate::jpeg::{self, exif, metadata, FrameKind, HeaderInfo};
use crate::lossless;
use crate::metric::{Reference, Score};
use crate::options::{OptimizeOptions, Resize};
use crate::resize;
use crate::search;

const EXIF_HEADER: &[u8] = b"Exif\0\0";
/// A lossless transcode must save at least this much to be worth writing.
const LOSSLESS_MIN_GAIN: f64 = 0.01;
/// Sources whose quantization says they were already compressed this hard (estimated
/// libjpeg quality) only get the lossless pass: re-encoding them would stack a second
/// generation of loss on top of the first.
const ALREADY_COMPRESSED_QUALITY: u8 = 90;
/// Below this saving a perceptual re-encode is "marginal": try the lossless pass too and
/// prefer it when it gets close — same pixels beats a generation of loss.
const MARGINAL_GAIN: f64 = 0.10;
/// Lossless wins if it is within this factor of the perceptual size.
const LOSSLESS_PREFERENCE: f64 = 1.03;

#[derive(Debug, Clone, PartialEq)]
pub struct Optimized {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub score: Score,
    /// Coefficients untouched (entropy coding only): decoded pixels are bit-identical.
    pub lossless: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Optimized(Optimized),
    Skipped(SkipReason),
}

/// A decoded source, ready to be rendered at one or more sizes without decoding again.
pub struct Prepared<'a> {
    original: &'a [u8],
    header: HeaderInfo,
    pixels: Pixels,
    orientation: u16,
    options: OptimizeOptions,
}

fn is_jpeg(data: &[u8]) -> bool {
    data.len() > 3 && data[..3] == [0xFF, 0xD8, 0xFF]
}

fn read_orientation(data: &[u8]) -> u16 {
    metadata::metadata_segments(data)
        .ok()
        .and_then(|segs| {
            segs.into_iter()
                .find(|s| s.marker == 0xE1 && s.body.starts_with(EXIF_HEADER))
                .and_then(|s| exif::orientation(&s.body[EXIF_HEADER.len()..]))
        })
        .unwrap_or(1)
}

fn precheck(data: &[u8], header: &HeaderInfo, options: &OptimizeOptions) -> Option<SkipReason> {
    if header.kind == FrameKind::Exotic || header.precision != 8 {
        return Some(SkipReason::ExoticJpeg);
    }
    if header.is_cmyk() {
        return Some(SkipReason::Cmyk);
    }
    if header.width as u64 * header.height as u64 > options.max_pixels {
        return Some(SkipReason::TooLarge);
    }
    if metadata::has_gain_map(data) {
        return Some(SkipReason::HdrGainMap);
    }
    if metadata::has_embedded_media(data) {
        return Some(SkipReason::EmbeddedMedia);
    }
    None
}

/// Decodes `data`, or explains why it will be left alone.
pub fn prepare<'a>(
    data: &'a [u8],
    options: &OptimizeOptions,
) -> Result<std::result::Result<Prepared<'a>, SkipReason>> {
    if !is_jpeg(data) {
        return Ok(Err(SkipReason::Unsupported));
    }
    let header = jpeg::inspect(data)?;
    if let Some(reason) = precheck(data, &header, options) {
        return Ok(Err(reason));
    }
    let pixels = codec::decode(data, header.is_grayscale())?;
    if (pixels.width, pixels.height) != (header.width, header.height) {
        return Err(FinoError::Decode("decoded size differs from header".into()));
    }
    let orientation = read_orientation(data);
    Ok(Ok(Prepared {
        original: data,
        header,
        pixels,
        orientation,
        options: options.clone(),
    }))
}

impl Prepared<'_> {
    pub fn header(&self) -> &HeaderInfo {
        &self.header
    }

    /// Small upright JPEG of the photo for UI thumbnails — cheap, because the pixels are
    /// already decoded. Showing the real 24 MP files in the UI made it stutter.
    pub fn preview(&self, long_edge: u32) -> Result<Vec<u8>> {
        let fit = Resize {
            mode: crate::options::ResizeMode::LongEdge,
            pixels: long_edge,
        };
        let small =
            match resize::target_size(self.pixels.width, self.pixels.height, self.orientation, fit)
            {
                Some((w, h)) => resize::resize(&self.pixels, w, h)?,
                None => self.pixels.clone(),
            };
        let upright = resize::orient(&small, self.orientation);
        let params = codec::EncodeParams::new(80.0, (2, 2), codec::Effort::Fast);
        codec::encode(&upright, params)
    }

    /// Renders the source at `size` (`None` = original dimensions).
    pub fn render(&self, size: Option<Resize>) -> Result<Outcome> {
        let target = size.and_then(|r| {
            resize::target_size(self.pixels.width, self.pixels.height, self.orientation, r)
        });
        let resized = match target {
            Some((w, h)) => Some(resize::resize(&self.pixels, w, h)?),
            None => None,
        };
        let is_resized = resized.is_some();
        if !is_resized && self.header.has_fino_marker && self.options.skip_optimized {
            return Ok(Outcome::Skipped(SkipReason::AlreadyOptimized));
        }
        let already_compressed = self
            .header
            .quality_estimate
            .is_some_and(|q| q < ALREADY_COMPRESSED_QUALITY);
        if !is_resized && already_compressed {
            let original = self.original.len() as f64;
            let lossless = self
                .lossless()?
                .filter(|o| 1.0 - o.bytes.len() as f64 / original >= LOSSLESS_MIN_GAIN);
            return Ok(lossless.map_or(Outcome::Skipped(SkipReason::NoGain), Outcome::Optimized));
        }
        let pixels = resized.as_ref().unwrap_or(&self.pixels);
        let reference = Reference::new(pixels)?;
        let chroma = self.header.chroma_block();
        let strength = self.options.strength;

        let candidate = match search::find_smallest(
            pixels,
            &reference,
            chroma,
            strength,
            self.options.quality_hint,
        )? {
            Some(c) => Some(c),
            None if is_resized => Some(search::top_quality(pixels, &reference, chroma, strength)?),
            None => None,
        };
        let perceptual = match candidate {
            Some(c) => {
                let marker = format!("{} q={} s={:.1}", crate::VERSION, c.quality, c.score.global);
                Some(Optimized {
                    bytes: self.with_metadata(&c.jpeg, &marker)?,
                    width: pixels.width,
                    height: pixels.height,
                    quality: c.quality,
                    score: c.score,
                    lossless: false,
                })
            }
            None => None,
        };
        if is_resized {
            return Ok(perceptual.map_or(Outcome::Skipped(SkipReason::NoGain), Outcome::Optimized));
        }

        let original = self.original.len() as f64;
        let gain = |o: &Optimized| 1.0 - o.bytes.len() as f64 / original;
        let perceptual = perceptual.filter(|o| gain(o) >= self.options.min_gain);
        if perceptual
            .as_ref()
            .is_some_and(|o| gain(o) >= MARGINAL_GAIN)
        {
            return Ok(Outcome::Optimized(perceptual.expect("checked above")));
        }
        let lossless = self.lossless()?.filter(|o| gain(o) >= LOSSLESS_MIN_GAIN);
        let best = match (perceptual, lossless) {
            (Some(p), Some(l))
                if l.bytes.len() as f64 <= p.bytes.len() as f64 * LOSSLESS_PREFERENCE =>
            {
                Some(l)
            }
            (Some(p), _) => Some(p),
            (None, l) => l,
        };
        Ok(best.map_or(Outcome::Skipped(SkipReason::NoGain), Outcome::Optimized))
    }

    fn with_metadata(&self, jpeg: &[u8], marker: &str) -> Result<Vec<u8>> {
        let segments =
            metadata::prepare_segments(self.original, self.options.strip_location, marker)?;
        metadata::splice(jpeg, &segments)
    }

    /// DCT-domain recompression: optimal Huffman + progressive, pixels bit-identical.
    fn lossless(&self) -> Result<Option<Optimized>> {
        // RGB-coded JPEGs (Adobe transform 0) need their APP14 marker to decode correctly;
        // the transcode keeps their coefficients, but metadata splicing drops APP14.
        if self.header.adobe_transform == Some(0) && self.header.components.len() == 3 {
            return Ok(None);
        }
        let Ok(jpeg) = lossless::transcode(self.original) else {
            return Ok(None); // an odd-but-decodable file just doesn't get the lossless floor
        };
        let marker = format!("{} lossless", crate::VERSION);
        let identical = Score {
            global: 100.0,
            mean: 100.0,
            worst: 100.0,
        };
        Ok(Some(Optimized {
            bytes: self.with_metadata(&jpeg, &marker)?,
            width: self.pixels.width,
            height: self.pixels.height,
            quality: self.header.quality_estimate.unwrap_or(100),
            score: identical,
            lossless: true,
        }))
    }
}

/// One-shot convenience: optimize `data` at a single size.
pub fn optimize(data: &[u8], options: &OptimizeOptions) -> Result<Outcome> {
    match prepare(data, options)? {
        Ok(prepared) => prepared.render(options.resize),
        Err(reason) => Ok(Outcome::Skipped(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Effort, EncodeParams};

    fn photo(width: u32, height: u32) -> Pixels {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let n = ((x * 7 + y * 13) ^ (x * y)) % 23;
                data.extend_from_slice(&[
                    (x * 200 / width + n) as u8,
                    (y * 200 / height + n) as u8,
                    ((x + y) / 3 % 200 + n) as u8,
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

    /// Already-compressed file written like cameras and many optimizers do: baseline,
    /// standard Huffman.
    fn already_compressed() -> Vec<u8> {
        let probe = EncodeParams::new(72.0, (2, 2), Effort::Probe);
        codec::encode(&photo(320, 240), probe).unwrap()
    }

    #[test]
    fn already_compressed_file_gets_a_lossless_gain_instead_of_no_gain() {
        let original = already_compressed();
        let Outcome::Optimized(out) = optimize(&original, &OptimizeOptions::default()).unwrap()
        else {
            panic!("expected a gain");
        };
        assert!(out.bytes.len() < original.len());
        assert!(
            out.lossless,
            "a re-encode of an already-tight file should lose to lossless"
        );
        let a = codec::decode(&original, false).unwrap();
        let b = codec::decode(&out.bytes, false).unwrap();
        assert_eq!(
            a.data, b.data,
            "lossless output must decode to identical pixels"
        );
        assert_eq!(out.score.global, 100.0);
    }

    #[test]
    fn high_quality_camera_file_is_recompressed_perceptually() {
        let camera = codec::encode(
            &photo(320, 240),
            EncodeParams::new(98.0, (1, 1), Effort::Probe),
        )
        .unwrap();
        let Outcome::Optimized(out) = optimize(&camera, &OptimizeOptions::default()).unwrap()
        else {
            panic!("expected a gain");
        };
        assert!(!out.lossless);
        assert!(
            out.bytes.len() < camera.len() * 9 / 10,
            "{} vs {}",
            out.bytes.len(),
            camera.len()
        );
        assert_eq!(
            jpeg::inspect(&out.bytes).unwrap().kind,
            FrameKind::Progressive
        );
    }

    #[test]
    fn fino_output_is_skipped_on_a_second_pass() {
        let camera = codec::encode(
            &photo(320, 240),
            EncodeParams::new(98.0, (1, 1), Effort::Probe),
        )
        .unwrap();
        let Outcome::Optimized(once) = optimize(&camera, &OptimizeOptions::default()).unwrap()
        else {
            panic!("expected a gain");
        };
        let again = optimize(&once.bytes, &OptimizeOptions::default()).unwrap();
        assert_eq!(again, Outcome::Skipped(SkipReason::AlreadyOptimized));
    }
}
