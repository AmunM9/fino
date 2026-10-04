//! HEIC → JPEG: the system decodes the pixels, Fino's perceptual search encodes them, and the
//! original metadata (EXIF with Apple's MakerNote, XMP, ICC) plus any HDR gain map, depth or
//! portrait matte travel along byte for byte.

pub mod container;
#[cfg(target_os = "macos")]
pub mod decode;

pub use container::{inspect, is_heif, HeifInfo};

use crate::error::{Result, SkipReason};
use crate::options::OptimizeOptions;
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
    let info = inspect(data)?;
    Ok(Err(
        precheck(&info, options).unwrap_or(SkipReason::Unsupported)
    ))
}

#[cfg(target_os = "macos")]
pub fn prepare<'a>(
    data: &'a [u8],
    options: &OptimizeOptions,
) -> Result<std::result::Result<Prepared<'a>, SkipReason>> {
    use crate::jpeg::{exif, metadata, mpf};
    use crate::optimize::Converted;

    let info = inspect(data)?;
    if let Some(reason) = precheck(&info, options) {
        return Ok(Err(reason));
    }
    // The pixel budget is checked from the header, before anything is allocated.
    let Some(decoded) = decode::decode(data, true, options.max_pixels)? else {
        return Ok(Err(SkipReason::TooLarge));
    };
    if info.iso_gain_map && decoded.unreadable_iso_gain_map {
        return Ok(Err(SkipReason::HdrGainMap)); // this macOS cannot copy an ISO gain map
    }

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

    let secondaries = match decoded.carrier {
        Some(carrier) => mpf::secondary_images(&carrier)?,
        None => vec![],
    };
    Ok(Ok(Prepared::converted(
        pixels,
        orientation,
        options,
        Converted {
            segments,
            secondaries,
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
