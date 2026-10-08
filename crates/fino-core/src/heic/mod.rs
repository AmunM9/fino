//! HEIC → JPEG, for sharing: the system decodes the pixels and Fino's perceptual search
//! encodes them at the Compact strength. The original EXIF (with Apple's MakerNote), XMP and
//! ICC travel byte for byte; the HDR gain map, depth and portrait mattes are dropped — the
//! JPEG is meant to open anywhere and stay close to the HEIC's size.

pub mod container;
#[cfg(target_os = "macos")]
pub mod decode;

pub use container::{inspect, is_heif, HeifInfo};

use crate::error::{Result, SkipReason};
use crate::options::{OptimizeOptions, Strength};

/// HEIC needs a system decoder; only macOS ships one Fino can rely on (Windows' HEVC codec
/// is an optional Store extension). Elsewhere HEIC photos are not picked up from folders and
/// are reported as unsupported when dropped directly.
pub const CONVERSION_AVAILABLE: bool = cfg!(target_os = "macos");

/// Conversions always use this strength: a JPEG at the source's own quality weighs far more
/// than the HEIC (it is the less efficient format), Compact keeps it near the HEIC's size.
pub const CONVERSION_STRENGTH: Strength = Strength::Compact;
use crate::Prepared;

/// Why a HEIC cannot be converted, decided from the container alone (no decoding).
pub fn precheck(info: &HeifInfo, options: &OptimizeOptions) -> Option<SkipReason> {
    if !options.convert_heic {
        Some(SkipReason::ConversionOff)
    } else if info.sequence {
        Some(SkipReason::Unsupported)
    } else if info.stereo {
        Some(SkipReason::SpatialPhoto)
    } else if info.hdr_transfer() {
        Some(SkipReason::HdrPhoto)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn prepare<'a>(
    data: &'a [u8],
    options: &OptimizeOptions,
) -> Result<std::result::Result<Prepared<'a>, SkipReason>> {
    let _ = (data, options);
    Ok(Err(SkipReason::Unsupported))
}

#[cfg(target_os = "macos")]
pub fn prepare<'a>(
    data: &'a [u8],
    options: &OptimizeOptions,
) -> Result<std::result::Result<Prepared<'a>, SkipReason>> {
    use crate::jpeg::{exif, metadata};
    use crate::optimize::Converted;

    let info = inspect(data)?;
    if let Some(reason) = precheck(&info, options) {
        return Ok(Err(reason));
    }
    // The pixel budget is checked from the header, before anything is allocated. Auxiliary
    // images (gain map, depth, mattes) are not carried over: the JPEG is the SDR photo.
    let Some(decoded) = decode::decode(data, false, options.max_pixels)? else {
        return Ok(Err(SkipReason::TooLarge));
    };

    // The rotation always travels as an EXIF tag (pixels stay as stored), so auxiliary
    // images such as the gain map stay aligned with the primary.
    let orientation = decoded.orientation;
    let tiff = match info.exif_tiff {
        Some(tiff) => exif::with_orientation(&tiff, orientation)
            .ok_or(crate::FinoError::Malformed("unreadable EXIF in HEIC"))?,
        None => exif::orientation_only(orientation),
    };
    let pixels = decoded.pixels;

    let mut exif_body = metadata::EXIF_HEADER.to_vec();
    exif_body.extend_from_slice(&tiff);
    let mut segments = vec![metadata::Segment {
        marker: metadata::APP1,
        body: exif_body,
    }];
    if let Some(xmp) = info.xmp {
        let mut body = metadata::XMP_HEADER.to_vec();
        body.extend_from_slice(&xmp_orientation(&xmp, orientation));
        segments.push(metadata::Segment {
            marker: metadata::APP1,
            body,
        });
    }
    let icc = if decoded.native_space {
        info.icc.or(decoded.icc)
    } else {
        decoded.icc
    };
    segments.extend(
        icc.as_deref()
            .map(metadata::icc_segments)
            .unwrap_or_default(),
    );

    let options = OptimizeOptions {
        strength: CONVERSION_STRENGTH,
        ..options.clone()
    };
    Ok(Ok(Prepared::converted(
        pixels,
        orientation,
        &options,
        Converted {
            segments,
            secondaries: vec![],
        },
    )))
}

/// Keeps an XMP `tiff:Orientation` in step with the EXIF value, so there is one truth.
#[cfg(target_os = "macos")]
fn xmp_orientation(xmp: &[u8], value: u16) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(xmp) else {
        return xmp.to_vec();
    };
    let attribute = regex::Regex::new(r#"tiff:Orientation="\d""#).expect("valid regex");
    let element =
        regex::Regex::new(r"<tiff:Orientation>\d</tiff:Orientation>").expect("valid regex");
    let text = attribute.replace_all(text, format!(r#"tiff:Orientation="{value}""#).as_str());
    let text = element.replace_all(
        &text,
        format!("<tiff:Orientation>{value}</tiff:Orientation>").as_str(),
    );
    text.into_owned().into_bytes()
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
