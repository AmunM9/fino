//! End-to-end HEIC → JPEG on files written by ImageIO itself (tests/fixtures).

use super::container::tests::fixture;
use super::decode::aux_present;
use crate::jpeg::{self, exif, metadata};
use crate::{codec, optimize, OptimizeOptions, Outcome, SkipReason};

const GPS_IFD: u16 = 0x8825;

/// Conversion turned on, as the "HEIC to JPEG" setting does.
fn on() -> OptimizeOptions {
    OptimizeOptions {
        convert_heic: true,
        ..OptimizeOptions::default()
    }
}

fn convert(name: &str, options: &OptimizeOptions) -> Outcome {
    optimize(&fixture(name), options).unwrap()
}

fn converted(name: &str) -> Vec<u8> {
    match convert(name, &on()) {
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
fn rotation_without_an_exif_tag_gets_one_and_pixels_stay_as_stored() {
    let jpeg = converted("plain.heic");
    let pixels = codec::decode(&jpeg, false).unwrap();
    assert_eq!((pixels.width, pixels.height), (256, 192));
    let segments = metadata::metadata_segments(&jpeg).unwrap();
    let tiff = segment(&segments, metadata::EXIF_HEADER).expect("EXIF");
    assert_eq!(
        exif::orientation(tiff),
        Some(6),
        "tag added to the copied EXIF"
    );
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
        ..on()
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
fn conversions_drop_hdr_depth_and_portrait_data() {
    for name in ["gainmap.heic", "isogainmap.heic"] {
        let jpeg = converted(name);
        assert_eq!(
            aux_present(&jpeg),
            Vec::<String>::new(),
            "{name}: SDR JPEG only"
        );
        assert!(
            jpeg::mpf::secondary_images(&jpeg).is_err(),
            "{name}: no MPF images"
        );
        assert_eq!(codec::decode(&jpeg, false).unwrap().width, 256);
    }
}

#[test]
fn hdr_primaries_and_disabled_conversion_are_skipped_with_a_reason() {
    assert_eq!(
        convert("hlg.heic", &on()),
        Outcome::Skipped(SkipReason::HdrPhoto)
    );
    let off = OptimizeOptions::default(); // conversion is off unless asked for
    assert_eq!(
        convert("plain.heic", &off),
        Outcome::Skipped(SkipReason::ConversionOff)
    );
}

#[test]
fn conversions_always_use_the_compact_strength() {
    use crate::Strength::*;
    let outputs: Vec<Vec<u8>> = [Pristine, Identical, Compact]
        .into_iter()
        .map(|strength| {
            let options = OptimizeOptions { strength, ..on() };
            match convert("plain.heic", &options) {
                Outcome::Optimized(o) => o.bytes,
                other => panic!("{strength:?}: {other:?}"),
            }
        })
        .collect();
    assert!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "same output whatever the setting"
    );
}

#[test]
fn iphone_jpegs_with_a_gain_map_are_optimized_and_keep_their_hdr() {
    let source = fixture("gainmap.jpg");
    let Outcome::Optimized(o) = optimize(&source, &OptimizeOptions::default()).unwrap() else {
        panic!("an HDR JPEG is optimized now, not skipped");
    };
    assert!(!o.converted);
    let aux = aux_present(&o.bytes);
    assert!(aux.iter().any(|k| k.contains("HDRGainMap")), "aux: {aux:?}");
    assert_eq!(
        jpeg::mpf::secondary_images(&o.bytes).unwrap(),
        jpeg::mpf::secondary_images(&source).unwrap(),
        "gain map carried byte for byte"
    );
}
