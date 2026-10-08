//! Bytes-in, bytes-out optimization of a single JPEG.

use crate::codec::{self, Pixels};
use crate::error::{FinoError, Result, SkipReason};
use crate::jpeg::{self, exif, metadata, mpf, FrameKind, HeaderInfo};
use crate::lossless;
use crate::metric::{Reference, Score};
use crate::options::{OptimizeOptions, Resize};
use crate::requant::Coefficients;
use crate::resize;
use crate::search::{self, Encoder};

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
/// HEVC photos are 4:2:0; a 4:4:4 JPEG would spend bytes on chroma the source never had.
const CONVERTED_CHROMA: (u8, u8) = (2, 2);

#[derive(Debug, Clone, PartialEq)]
pub struct Optimized {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub score: Score,
    /// Coefficients untouched (entropy coding only): decoded pixels are bit-identical.
    pub lossless: bool,
    /// Made from another format (HEIC): a new JPEG rather than a smaller copy of the source.
    pub converted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Optimized(Optimized),
    Skipped(SkipReason),
}

/// A source in another format being converted to JPEG (see `heic`).
pub(crate) struct Converted {
    /// APPn segments for the output (EXIF, XMP, ICC) before location stripping and marker.
    pub segments: Vec<metadata::Segment>,
    /// Auxiliary images (HDR gain map, depth…) appended as MPF secondaries.
    pub secondaries: Vec<mpf::MpImage>,
}

enum Source<'a> {
    Jpeg {
        original: &'a [u8],
        header: HeaderInfo,
        /// MPF images after the primary (HDR gain map…), re-attached byte for byte.
        secondaries: Vec<mpf::MpImage>,
    },
    // Only built by the HEIC decoder, which needs macOS.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Converted(Converted),
}

/// A decoded source, ready to be rendered at one or more sizes without decoding again.
pub struct Prepared<'a> {
    source: Source<'a>,
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

/// RGB-coded JPEGs (Adobe transform 0) depend on their APP14 marker, which re-encoding drops.
fn requantizable(header: &HeaderInfo) -> bool {
    !(header.adobe_transform == Some(0) && header.components.len() == 3)
}

fn encoder<'a>(
    coefficients: &'a mut Option<Coefficients>,
    pixels: &'a Pixels,
    chroma_block: (u8, u8),
) -> Encoder<'a> {
    match coefficients {
        Some(c) => Encoder::Requantize(c),
        None => Encoder::Pixels {
            pixels,
            chroma_block,
        },
    }
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
    if metadata::has_embedded_media(data) {
        return Some(SkipReason::EmbeddedMedia);
    }
    None
}

/// Decodes `data` (JPEG, or HEIC to convert), or explains why it will be left alone.
pub fn prepare<'a>(
    data: &'a [u8],
    options: &OptimizeOptions,
) -> Result<std::result::Result<Prepared<'a>, SkipReason>> {
    if crate::heic::is_heif(data) {
        return crate::heic::prepare(data, options);
    }
    if !is_jpeg(data) {
        return Ok(Err(SkipReason::Unsupported));
    }
    let header = jpeg::inspect(data)?;
    if let Some(reason) = precheck(data, &header, options) {
        return Ok(Err(reason));
    }
    // A gain map can only travel if its MPF index is readable; otherwise leave the file be.
    let secondaries = if metadata::has_gain_map(data) {
        match mpf::secondary_images(data) {
            Ok(images) if !images.is_empty() => images,
            _ => return Ok(Err(SkipReason::HdrGainMap)),
        }
    } else {
        vec![]
    };
    let pixels = codec::decode(data, header.is_grayscale())?;
    if (pixels.width, pixels.height) != (header.width, header.height) {
        return Err(FinoError::Decode("decoded size differs from header".into()));
    }
    let orientation = read_orientation(data);
    Ok(Ok(Prepared {
        source: Source::Jpeg {
            original: data,
            header,
            secondaries,
        },
        pixels,
        orientation,
        options: options.clone(),
    }))
}

impl<'a> Prepared<'a> {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn converted(
        pixels: Pixels,
        orientation: u16,
        options: &OptimizeOptions,
        converted: Converted,
    ) -> Self {
        Self {
            source: Source::Converted(converted),
            pixels,
            orientation,
            options: options.clone(),
        }
    }

    /// Stored pixel dimensions (before EXIF orientation is applied).
    pub fn dimensions(&self) -> (u32, u32) {
        (self.pixels.width, self.pixels.height)
    }

    /// The output will be a JPEG made from another format.
    pub fn is_conversion(&self) -> bool {
        matches!(self.source, Source::Converted(_))
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
                Some((w, h)) => resize::thumbnail(&self.pixels, w, h)?,
                None => self.pixels.clone(),
            };
        let upright = resize::orient(&small, self.orientation);
        let params = codec::EncodeParams::new(80.0, (2, 2), codec::Effort::Fast);
        codec::encode(&upright, params)
    }

    /// Renders the source at `size` (`None` = original dimensions).
    pub fn render(&self, size: Option<Resize>) -> Result<Outcome> {
        match &self.source {
            Source::Jpeg {
                original,
                header,
                secondaries,
            } => self.render_jpeg(original, header, secondaries, size),
            Source::Converted(c) => self.render_converted(c, size),
        }
    }

    fn resized(&self, size: Option<Resize>) -> Result<Option<Pixels>> {
        let target = size.and_then(|r| {
            resize::target_size(self.pixels.width, self.pixels.height, self.orientation, r)
        });
        target
            .map(|(w, h)| resize::resize(&self.pixels, w, h))
            .transpose()
    }

    /// Another format to JPEG: always produces a file. Quality targets are relative to the
    /// best score a JPEG can reach here, since re-encoding a non-JPEG source can never be
    /// as transparent as re-quantizing a JPEG on its own 8×8 grid.
    fn render_converted(&self, c: &Converted, size: Option<Resize>) -> Result<Outcome> {
        let resized = self.resized(size)?;
        let pixels = resized.as_ref().unwrap_or(&self.pixels);
        let reference = Reference::new(pixels)?;
        let strength = self.options.strength;
        let chroma = CONVERTED_CHROMA;
        let targets = search::ceiling_targets(pixels, &reference, chroma, strength)?;
        let encoder = || Encoder::Pixels {
            pixels,
            chroma_block: chroma,
        };
        let found = search::find_smallest_with(
            encoder(),
            &reference,
            strength,
            self.options.quality_hint,
            targets,
        )?;
        let c_best = match found {
            Some(candidate) => candidate,
            None => search::top_quality(encoder(), &reference, strength)?,
        };
        let marker = format!(
            "{} q={} s={:.1} from=heic",
            crate::VERSION,
            c_best.quality,
            c_best.score.global
        );
        let segments = metadata::prepare_segments_from(
            c.segments.clone(),
            self.options.strip_location,
            &marker,
        )?;
        let primary = metadata::splice(&c_best.jpeg, &segments)?;
        Ok(Outcome::Optimized(Optimized {
            bytes: mpf::append(&primary, &c.secondaries)?,
            width: pixels.width,
            height: pixels.height,
            quality: c_best.quality,
            score: c_best.score,
            lossless: false,
            converted: true,
        }))
    }

    fn render_jpeg(
        &self,
        original: &[u8],
        header: &HeaderInfo,
        secondaries: &[mpf::MpImage],
        size: Option<Resize>,
    ) -> Result<Outcome> {
        let target = size.and_then(|r| {
            resize::target_size(self.pixels.width, self.pixels.height, self.orientation, r)
        });
        let resized = match target {
            Some((w, h)) => Some(resize::resize(&self.pixels, w, h)?),
            None => None,
        };
        let is_resized = resized.is_some();
        if !is_resized && header.has_fino_marker && self.options.skip_optimized {
            return Ok(Outcome::Skipped(SkipReason::AlreadyOptimized));
        }
        let already_compressed = header
            .quality_estimate
            .is_some_and(|q| q < ALREADY_COMPRESSED_QUALITY);
        if !is_resized && already_compressed {
            let original_len = original.len() as f64;
            let lossless = self
                .lossless(original, header, secondaries)?
                .filter(|o| 1.0 - o.bytes.len() as f64 / original_len >= LOSSLESS_MIN_GAIN);
            return Ok(lossless.map_or(Outcome::Skipped(SkipReason::NoGain), Outcome::Optimized));
        }
        let pixels = resized.as_ref().unwrap_or(&self.pixels);
        let reference = Reference::new(pixels)?;
        let chroma = header.chroma_block();
        let strength = self.options.strength;

        // At the original size the source's own coefficients are re-quantized; a resized
        // output has new pixels to encode.
        let mut coefficients = (!is_resized && requantizable(header))
            .then(|| Coefficients::read(original).ok())
            .flatten();
        let search = |coefficients: &mut Option<Coefficients>| {
            search::find_smallest_with(
                encoder(coefficients, pixels, chroma),
                &reference,
                strength,
                self.options.quality_hint,
                strength.targets(),
            )
        };
        // A file libjpeg reads but cannot re-encode as is still has its pixels.
        let found = match search(&mut coefficients) {
            Err(_) if coefficients.is_some() => {
                coefficients = None;
                search(&mut coefficients)
            }
            other => other,
        };
        let candidate = match found? {
            Some(c) => Some(c),
            None if is_resized => Some(search::top_quality(
                encoder(&mut coefficients, pixels, chroma),
                &reference,
                strength,
            )?),
            None => None,
        };
        let perceptual = match candidate {
            Some(c) => {
                let marker = format!("{} q={} s={:.1}", crate::VERSION, c.quality, c.score.global);
                Some(Optimized {
                    bytes: self.with_metadata(original, secondaries, &c.jpeg, &marker)?,
                    width: pixels.width,
                    height: pixels.height,
                    quality: c.quality,
                    score: c.score,
                    lossless: false,
                    converted: false,
                })
            }
            None => None,
        };
        if is_resized {
            return Ok(perceptual.map_or(Outcome::Skipped(SkipReason::NoGain), Outcome::Optimized));
        }

        let original_len = original.len() as f64;
        let gain = |o: &Optimized| 1.0 - o.bytes.len() as f64 / original_len;
        let perceptual = perceptual.filter(|o| gain(o) >= self.options.min_gain);
        if perceptual
            .as_ref()
            .is_some_and(|o| gain(o) >= MARGINAL_GAIN)
        {
            return Ok(Outcome::Optimized(perceptual.expect("checked above")));
        }
        let lossless = self
            .lossless(original, header, secondaries)?
            .filter(|o| gain(o) >= LOSSLESS_MIN_GAIN);
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

    /// The source's metadata plus Fino's marker on `jpeg`, with the source's MPF secondaries
    /// (HDR gain map) appended unchanged.
    fn with_metadata(
        &self,
        original: &[u8],
        secondaries: &[mpf::MpImage],
        jpeg: &[u8],
        marker: &str,
    ) -> Result<Vec<u8>> {
        let segments = metadata::prepare_segments(original, self.options.strip_location, marker)?;
        mpf::append(&metadata::splice(jpeg, &segments)?, secondaries)
    }

    /// DCT-domain recompression: optimal Huffman + progressive, pixels bit-identical.
    fn lossless(
        &self,
        original: &[u8],
        header: &HeaderInfo,
        secondaries: &[mpf::MpImage],
    ) -> Result<Option<Optimized>> {
        // The transcode keeps their coefficients, but metadata splicing drops APP14.
        if !requantizable(header) {
            return Ok(None);
        }
        let Ok(jpeg) = lossless::transcode(original) else {
            return Ok(None); // an odd-but-decodable file just doesn't get the lossless floor
        };
        let marker = format!("{} lossless", crate::VERSION);
        let identical = Score {
            global: 100.0,
            mean: 100.0,
            worst: 100.0,
        };
        Ok(Some(Optimized {
            bytes: self.with_metadata(original, secondaries, &jpeg, &marker)?,
            width: self.pixels.width,
            height: self.pixels.height,
            quality: header.quality_estimate.unwrap_or(100),
            score: identical,
            lossless: true,
            converted: false,
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
