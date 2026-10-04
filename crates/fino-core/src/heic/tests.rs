//! End-to-end HEIC → JPEG on files written by ImageIO itself (tests/fixtures).

use super::container::tests::fixture;
use super::decode::aux_present;
use crate::jpeg::{self, exif, metadata};
use crate::{codec, optimize, OptimizeOptions, Outcome, SkipReason};

const GPS_IFD: u16 = 0x8825;

fn convert(name: &str, options: &OptimizeOptions) -> Outcome {
    optimize(&fixture(name), options).unwrap()
}

fn converted(name: &str) -> Vec<u8> {
    match convert(name, &OptimizeOptions::default()) {
        Outcome::Optimized(o) => {
            assert!(o.converted && !o.lossless);
            o.bytes
        }
        Outcome::Skipped(reason) => panic!("{name} skipped: {reason:?}"),
    }
}

fn segment<'a>(segments: &'a [metadata::Segment], header: &[u8]) -> Option<&'a [u8]> {
    segments
        .iter()
        .find(|s| s.body.starts_with(header))
        .map(|s| &s.body[header.len()..])
}

/// IFD0 tags of a TIFF block.
fn ifd0_tags(tiff: &[u8]) -> Vec<u16> {
    let be = tiff.starts_with(b"MM");
    let u16_at = |at: usize| {
        let b = [tiff[at], tiff[at + 1]];
        if be {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [tiff[at], tiff[at + 1], tiff[at + 2], tiff[at + 3]];
        if be {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    };
    let ifd = u32_at(4) as usize;
    (0..u16_at(ifd) as usize)
        .map(|i| u16_at(ifd + 2 + i * 12))
        .collect()
}

#[test]
fn rotation_travels_in_exif_like_an_iphone_jpeg() {
    let jpeg = converted("rotated.heic");
    let pixels = codec::decode(&jpeg, false).unwrap();
    assert_eq!(
        (pixels.width, pixels.height),
        (256, 192),
        "stored, not rotated"
    );
    let segments = metadata::metadata_segments(&jpeg).unwrap();
    let tiff = segment(&segments, metadata::EXIF_HEADER).expect("EXIF");
    assert_eq!(exif::orientation(tiff), Some(6));
}

#[test]
fn rotation_without_an_exif_tag_is_baked_into_the_pixels() {
    let jpeg = converted("plain.heic");
    let pixels = codec::decode(&jpeg, false).unwrap();
    assert_eq!((pixels.width, pixels.height), (192, 256), "upright pixels");
    let segments = metadata::metadata_segments(&jpeg).unwrap();
    let tiff = segment(&segments, metadata::EXIF_HEADER).expect("EXIF");
    assert!(matches!(exif::orientation(tiff), None | Some(1)));
}

#[test]
fn converts_to_a_jpeg_that_keeps_every_byte_of_metadata() {
    let jpeg = converted("plain.heic");
    let segments = metadata::metadata_segments(&jpeg).unwrap();
    let tiff = segment(&segments, metadata::EXIF_HEADER).expect("EXIF");
    assert!(
        ifd0_tags(tiff).contains(&GPS_IFD),
        "GPS kept unless asked otherwise"
    );
    let uuid = b"6F1E2D3C-4B5A-4978-8695-A4B3C2D1E0F9";
    assert!(
        tiff.windows(uuid.len()).any(|w| w == uuid),
        "Live Photo id in the MakerNote"
    );
    let xmp = segment(&segments, metadata::XMP_HEADER).expect("XMP");
    assert!(String::from_utf8_lossy(xmp).contains("kept byte for byte"));
    assert!(
        segment(&segments, b"ICC_PROFILE\0").is_some(),
        "Display P3 profile embedded"
    );
    assert!(
        jpeg::inspect(&jpeg).unwrap().has_fino_marker,
        "a second pass will skip it"
    );
}

#[test]
fn strip_location_removes_gps_from_converted_files() {
    let options = OptimizeOptions {
        strip_location: true,
        ..OptimizeOptions::default()
    };
    let Outcome::Optimized(o) = convert("rotated.heic", &options) else {
        panic!("not converted");
    };
    let segments = metadata::metadata_segments(&o.bytes).unwrap();
    let tiff = segment(&segments, metadata::EXIF_HEADER).unwrap();
    assert!(!ifd0_tags(tiff).contains(&GPS_IFD));
    assert_eq!(
        exif::orientation(tiff),
        Some(6),
        "the rest of EXIF is untouched"
    );
}

#[test]
fn hdr_gain_maps_survive_the_conversion() {
    let jpeg = converted("gainmap.heic");
    let aux = aux_present(&jpeg);
    assert!(
        aux.iter().any(|k| k.contains("HDRGainMap")),
        "aux found: {aux:?}"
    );
    assert_eq!(codec::decode(&jpeg, false).unwrap().width, 256);

    if super::decode::aux_present(&fixture("isogainmap.heic"))
        .iter()
        .any(|k| k.contains("ISOGainMap"))
    {
        let iso = converted("isogainmap.heic");
        assert!(aux_present(&iso).iter().any(|k| k.contains("ISOGainMap")));
    }
}

#[test]
fn hdr_primaries_and_disabled_conversion_are_skipped_with_a_reason() {
    assert_eq!(
        convert("hlg.heic", &OptimizeOptions::default()),
        Outcome::Skipped(SkipReason::HdrPhoto)
    );
    let off = OptimizeOptions {
        convert_heic: false,
        ..OptimizeOptions::default()
    };
    assert_eq!(
        convert("plain.heic", &off),
        Outcome::Skipped(SkipReason::ConversionOff)
    );
}

#[test]
fn every_strength_produces_a_jpeg_and_stricter_is_never_smaller() {
    use crate::Strength::*;
    let sizes: Vec<usize> = [Pristine, Identical, Compact]
        .into_iter()
        .map(|strength| {
            let options = OptimizeOptions {
                strength,
                ..OptimizeOptions::default()
            };
            match convert("plain.heic", &options) {
                Outcome::Optimized(o) => o.bytes.len(),
                other => panic!("{strength:?}: {other:?}"),
            }
        })
        .collect();
    assert!(sizes[0] >= sizes[1] && sizes[1] >= sizes[2], "{sizes:?}");
}
