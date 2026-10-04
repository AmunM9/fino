//! Metadata transplant: every APPn / COM segment of the original is carried over
//! byte-for-byte into the re-encoded file (EXIF, XMP, ICC, IPTC, maker notes…),
//! optionally with location data removed, plus Fino's own marker.

use super::inspect::FINO_MARKER_PREFIX;
use super::{exif, xmp_gps};
use crate::error::{FinoError, Result};

const COM: u8 = 0xFE;
const APP1: u8 = 0xE1;
const APP2: u8 = 0xE2;
const EXIF_HEADER: &[u8] = b"Exif\0\0";
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// Multi-Picture Format index: points at images appended after EOI, which a
/// re-encode cannot carry, so the index would dangle.
const MPF_HEADER: &[u8] = b"MPF\0";
const EXTENDED_XMP_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// Adobe APP14 describes the *original* colour transform; the re-encode is plain
/// YCbCr, so a copied `transform=0` would make viewers shift colours.
const APP14: u8 = 0xEE;
const MAX_SEGMENT_BODY: usize = 65_533;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub marker: u8,
    pub body: Vec<u8>,
}

fn is_metadata_marker(marker: u8) -> bool {
    (0xE0..=0xEF).contains(&marker) || marker == COM
}

/// Walks header segments up to SOS. Returns (segments, offset of first non-metadata marker).
fn walk(data: &[u8]) -> Result<(Vec<Segment>, usize)> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(FinoError::Malformed("missing SOI"));
    }
    let mut segments = Vec::new();
    let mut pos = 2;
    let mut first_other = None;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            return Err(FinoError::Malformed("expected marker"));
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > data.len() {
            return Err(FinoError::Malformed("segment overruns file"));
        }
        if is_metadata_marker(marker) {
            segments.push(Segment {
                marker,
                body: data[pos + 4..pos + 2 + len].to_vec(),
            });
        } else {
            first_other.get_or_insert(pos);
        }
        if marker == 0xDA {
            break;
        }
        pos += 2 + len;
    }
    let start = first_other.ok_or(FinoError::Malformed("no image data"))?;
    Ok((segments, start))
}

/// Metadata segments of `data`, in file order.
pub fn metadata_segments(data: &[u8]) -> Result<Vec<Segment>> {
    walk(data).map(|(segments, _)| segments)
}

/// Builds the segment list for the output file.
pub fn prepare_segments(
    original: &[u8],
    strip_location: bool,
    marker_text: &str,
) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for seg in metadata_segments(original)? {
        if seg.marker == COM && seg.body.starts_with(FINO_MARKER_PREFIX) {
            continue;
        }
        if (seg.marker == APP2 && seg.body.starts_with(MPF_HEADER)) || seg.marker == APP14 {
            continue;
        }
        if strip_location && seg.marker == APP1 && seg.body.starts_with(EXTENDED_XMP_HEADER) {
            continue; // extended XMP can carry GPS too; it is optional by design
        }
        if strip_location && seg.marker == APP1 {
            if seg.body.starts_with(EXIF_HEADER) {
                let mut body = seg.body;
                exif::strip_gps(&mut body[EXIF_HEADER.len()..])?;
                out.push(Segment { marker: APP1, body });
                continue;
            }
            if seg.body.starts_with(XMP_HEADER) {
                let body = xmp_gps::strip(&seg.body);
                out.push(Segment { marker: APP1, body });
                continue;
            }
        }
        out.push(seg);
    }
    let mut marker = FINO_MARKER_PREFIX.to_vec();
    marker.extend_from_slice(marker_text.as_bytes());
    out.push(Segment {
        marker: COM,
        body: marker,
    });
    Ok(out)
}

/// Replaces the metadata of a freshly encoded JPEG with `segments`.
pub fn splice(encoded: &[u8], segments: &[Segment]) -> Result<Vec<u8>> {
    let (_, image_start) = walk(encoded)?;
    let extra: usize = segments.iter().map(|s| s.body.len() + 4).sum();
    let mut out = Vec::with_capacity(encoded.len() + extra);
    out.extend_from_slice(&[0xFF, 0xD8]);
    for seg in segments {
        if seg.body.len() > MAX_SEGMENT_BODY {
            return Err(FinoError::Malformed("metadata segment too large"));
        }
        out.extend_from_slice(&[0xFF, seg.marker]);
        out.extend_from_slice(&((seg.body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&seg.body);
    }
    out.extend_from_slice(&encoded[image_start..]);
    Ok(out)
}

/// HDR gain maps (Ultra HDR, Adobe, Apple) live in auxiliary images after EOI.
/// Re-encoding would silently drop the HDR rendition, so such files are skipped.
pub fn has_gain_map(data: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 3] = [b"hdrgm", b"HDRGainMap", b"aux:hdrgainmap"];
    NEEDLES
        .iter()
        .any(|n| memchr::memmem::find(data, n).is_some())
}

/// Byte offset just past the EOI that closes the primary image, if found.
fn primary_image_end(data: &[u8]) -> Option<usize> {
    let (_, image_start) = walk(data).ok()?;
    let sos = image_start + memchr::memmem::find(&data[image_start..], &[0xFF, 0xDA])?;
    // Inside entropy-coded data 0xFF is always stuffed (FF 00) or a RST marker,
    // so the first FF D9 after the first SOS is the primary image's EOI.
    memchr::memmem::find(&data[sos..], &[0xFF, 0xD9]).map(|i| sos + i + 2)
}

/// Motion Photos and similar formats append an MP4 (or other media) after the image;
/// a re-encode would silently delete the video.
pub fn has_embedded_media(data: &[u8]) -> bool {
    const XMP_HINTS: [&[u8]; 4] = [
        b"MotionPhoto",
        b"MicroVideo",
        b"Container:Directory",
        b"MotionPhoto_Data",
    ];
    if XMP_HINTS
        .iter()
        .any(|n| memchr::memmem::find(data, n).is_some())
    {
        return true;
    }
    let Some(end) = primary_image_end(data) else {
        return false;
    };
    let trailer = &data[end.min(data.len())..];
    memchr::memmem::find(trailer, b"ftyp").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with(segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        for (m, body) in segments {
            v.extend_from_slice(&[0xFF, *m]);
            v.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            v.extend_from_slice(body);
        }
        // fake DQT + SOS + entropy + EOI
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x03, 0x00]);
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0xFF, 0xD9]);
        v
    }

    #[test]
    fn transplants_metadata_in_order_and_drops_encoder_headers() {
        let original = jpeg_with(&[
            (0xE1, b"Exif\0\0data"),
            (0xE2, b"ICC_PROFILE\0x"),
            (0xED, b"iptc"),
        ]);
        let encoded = jpeg_with(&[(0xE0, b"JFIF\0enc")]);
        let segs = prepare_segments(&original, false, "test").unwrap();
        let out = splice(&encoded, &segs).unwrap();
        let got = metadata_segments(&out).unwrap();
        let markers: Vec<u8> = got.iter().map(|s| s.marker).collect();
        assert_eq!(markers, vec![0xE1, 0xE2, 0xED, 0xFE]);
        assert_eq!(got[0].body, b"Exif\0\0data");
        assert!(got[3].body.starts_with(b"Fino/"));
        assert!(out.ends_with(&[0x12, 0x34, 0xFF, 0xD9]));
    }

    #[test]
    fn drops_stale_fino_marker_and_mpf_index() {
        let original = jpeg_with(&[(0xFE, b"Fino/old"), (0xE2, b"MPF\0index"), (0xFE, b"hello")]);
        let segs = prepare_segments(&original, false, "new").unwrap();
        let bodies: Vec<&[u8]> = segs.iter().map(|s| s.body.as_slice()).collect();
        assert_eq!(bodies, vec![b"hello".as_slice(), b"Fino/new".as_slice()]);
    }

    #[test]
    fn drops_adobe_app14_and_extended_xmp_when_stripping_location() {
        let original = jpeg_with(&[
            (0xEE, b"Adobe\0\x64\0\0\0\0\0"),
            (0xE1, b"http://ns.adobe.com/xmp/extension/\0gps"),
        ]);
        let kept = prepare_segments(&original, false, "x").unwrap();
        assert_eq!(
            kept.iter().map(|s| s.marker).collect::<Vec<_>>(),
            vec![0xE1, 0xFE]
        );
        let stripped = prepare_segments(&original, true, "x").unwrap();
        assert_eq!(
            stripped.iter().map(|s| s.marker).collect::<Vec<_>>(),
            vec![0xFE]
        );
    }

    #[test]
    fn detects_video_appended_after_the_image() {
        let mut motion = jpeg_with(&[]);
        motion.extend_from_slice(b"\0\0\0\x18ftypmp42");
        assert!(has_embedded_media(&motion));
        assert!(!has_embedded_media(&jpeg_with(&[])));
        assert!(has_embedded_media(&jpeg_with(&[(
            0xE1,
            b"http://ns.adobe.com/xap/1.0/\0GCamera:MotionPhoto=1"
        )])));
    }

    #[test]
    fn detects_gain_maps() {
        assert!(has_gain_map(
            b"....xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\""
        ));
        assert!(!has_gain_map(b"plain jpeg bytes"));
    }
}
