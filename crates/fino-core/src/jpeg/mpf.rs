//! Multi-Picture Format (CIPA DC-007): extra JPEGs appended after the primary image's EOI,
//! indexed by an APP2 `MPF\0` segment. Apple and Ultra HDR store gain maps (and depth or
//! portrait mattes) this way.
//!
//! Fino reads the secondaries of a carrier JPEG written by ImageIO and appends them to its
//! own encode with a fresh index, so HDR survives perceptual re-encoding.

use super::metadata::Segment;
use crate::error::{FinoError, Result};

const APP2: u8 = 0xE2;
pub const MPF_HEADER: &[u8] = b"MPF\0";
const TAG_VERSION: u16 = 0xB000;
const TAG_COUNT: u16 = 0xB001;
const TAG_ENTRIES: u16 = 0xB002;
const ENTRY_LEN: usize = 16;
/// Baseline MP primary image.
const PRIMARY_ATTRIBUTE: u32 = 0x0003_0000;
/// TIFF header + one IFD of three entries + next-IFD pointer, then the entry table.
const INDEX_FIXED_LEN: usize = 8 + 2 + 3 * 12 + 4;

/// One image of a multi-picture file other than the primary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpImage {
    pub attribute: u32,
    pub bytes: Vec<u8>,
}

fn malformed() -> FinoError {
    FinoError::Malformed("bad MPF index")
}

#[derive(Clone, Copy)]
struct Order(bool);

impl Order {
    fn u16(self, b: &[u8], at: usize) -> Result<u16> {
        let s: [u8; 2] = b
            .get(at..at + 2)
            .ok_or_else(malformed)?
            .try_into()
            .map_err(|_| malformed())?;
        Ok(if self.0 {
            u16::from_be_bytes(s)
        } else {
            u16::from_le_bytes(s)
        })
    }

    fn u32(self, b: &[u8], at: usize) -> Result<u32> {
        let s: [u8; 4] = b
            .get(at..at + 4)
            .ok_or_else(malformed)?
            .try_into()
            .map_err(|_| malformed())?;
        Ok(if self.0 {
            u32::from_be_bytes(s)
        } else {
            u32::from_le_bytes(s)
        })
    }
}

/// Offset in `jpeg` of the TIFF header inside the APP2 MPF segment, plus that header.
fn find_index(jpeg: &[u8]) -> Option<(usize, &[u8])> {
    let mut pos = 2;
    while pos + 4 <= jpeg.len() && jpeg[pos] == 0xFF {
        let marker = jpeg[pos + 1];
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if marker == 0xDA || len < 2 {
            return None;
        }
        let body = jpeg.get(pos + 4..pos + 2 + len)?;
        if marker == APP2 && body.starts_with(MPF_HEADER) {
            return Some((pos + 4 + MPF_HEADER.len(), &body[MPF_HEADER.len()..]));
        }
        pos += 2 + len;
    }
    None
}

/// The images after the primary in a multi-picture JPEG, in index order.
pub fn secondary_images(jpeg: &[u8]) -> Result<Vec<MpImage>> {
    let (tiff_at, tiff) = find_index(jpeg).ok_or(FinoError::Malformed("no MPF index"))?;
    let order = match tiff.get(..2) {
        Some(b"MM") => Order(true),
        Some(b"II") => Order(false),
        _ => return Err(malformed()),
    };
    let ifd = order.u32(tiff, 4)? as usize;
    let count = order.u16(tiff, ifd)? as usize;
    let (mut images, mut table) = (None, None);
    for i in 0..count {
        let entry = ifd + 2 + i * 12;
        match order.u16(tiff, entry)? {
            TAG_COUNT => images = Some(order.u32(tiff, entry + 8)? as usize),
            TAG_ENTRIES => table = Some(order.u32(tiff, entry + 8)? as usize),
            _ => {}
        }
    }
    let (images, table) = (images.ok_or_else(malformed)?, table.ok_or_else(malformed)?);
    (1..images)
        .map(|i| {
            let entry = table + i * ENTRY_LEN;
            let attribute = order.u32(tiff, entry)?;
            let size = order.u32(tiff, entry + 4)? as usize;
            let offset = order.u32(tiff, entry + 8)? as usize;
            let start = tiff_at + offset;
            let bytes = jpeg
                .get(start..start + size)
                .ok_or_else(malformed)?
                .to_vec();
            if !bytes.starts_with(&[0xFF, 0xD8]) {
                return Err(malformed());
            }
            Ok(MpImage { attribute, bytes })
        })
        .collect()
}

/// An index for `images + 1` pictures with zeroed sizes/offsets (filled by `append`).
fn index_segment(secondaries: &[MpImage]) -> Segment {
    let n = secondaries.len() + 1;
    let mut t = Vec::with_capacity(INDEX_FIXED_LEN + n * ENTRY_LEN);
    t.extend_from_slice(b"II*\0");
    t.extend_from_slice(&8u32.to_le_bytes());
    t.extend_from_slice(&3u16.to_le_bytes());
    let entries = [
        (TAG_VERSION, 7u16, 4u32, u32::from_le_bytes(*b"0100")),
        (TAG_COUNT, 4, 1, n as u32),
        (
            TAG_ENTRIES,
            7,
            (n * ENTRY_LEN) as u32,
            INDEX_FIXED_LEN as u32,
        ),
    ];
    for (tag, kind, count, value) in entries {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&kind.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&value.to_le_bytes());
    }
    t.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    let attributes =
        std::iter::once(PRIMARY_ATTRIBUTE).chain(secondaries.iter().map(|i| i.attribute));
    for attribute in attributes {
        t.extend_from_slice(&attribute.to_le_bytes());
        t.extend_from_slice(&[0; ENTRY_LEN - 4]);
    }
    let mut body = MPF_HEADER.to_vec();
    body.extend_from_slice(&t);
    Segment { marker: APP2, body }
}

/// Appends `secondaries` after `primary` (a complete JPEG whose metadata segments end
/// before its first non-APPn marker) and indexes them with a new APP2 MPF segment.
pub fn append(primary: &[u8], secondaries: &[MpImage]) -> Result<Vec<u8>> {
    if secondaries.is_empty() {
        return Ok(primary.to_vec());
    }
    let index = index_segment(secondaries);
    let mut segments = super::metadata::metadata_segments(primary)?;
    segments.retain(|s| !(s.marker == APP2 && s.body.starts_with(MPF_HEADER)));
    // Readers (ImageIO among them) expect the index right after APP0/APP1 (JFIF, EXIF, XMP),
    // as cameras write it — not after the ICC profile or comments.
    let at = segments
        .iter()
        .position(|s| s.marker > super::metadata::APP1)
        .unwrap_or(segments.len());
    segments.insert(at, index);
    let mut out = super::metadata::splice(primary, &segments)?;
    let (tiff_at, _) = find_index(&out).ok_or_else(malformed)?;
    let table = tiff_at + INDEX_FIXED_LEN;
    let primary_len = out.len();
    let put =
        |out: &mut Vec<u8>, at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, table + 4, primary_len as u32); // primary: size, offset 0
    let mut next = primary_len;
    for (i, image) in secondaries.iter().enumerate() {
        let entry = table + (i + 1) * ENTRY_LEN;
        put(&mut out, entry + 4, image.bytes.len() as u32);
        put(&mut out, entry + 8, (next - tiff_at) as u32);
        next += image.bytes.len();
    }
    for image in secondaries {
        out.extend_from_slice(&image.bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{self, Effort, EncodeParams, Pixels};

    fn tiny_jpeg(shade: u8) -> Vec<u8> {
        let px = Pixels {
            data: vec![shade; 16 * 8 * 3],
            width: 16,
            height: 8,
            channels: 3,
        };
        codec::encode(&px, EncodeParams::new(80.0, (2, 2), Effort::Probe)).unwrap()
    }

    #[test]
    fn appended_images_can_be_read_back_through_the_index() {
        let primary = tiny_jpeg(200);
        let gain = MpImage {
            attribute: 0,
            bytes: tiny_jpeg(40),
        };
        let depth = MpImage {
            attribute: 0x0002_0000,
            bytes: tiny_jpeg(90),
        };
        let joined = append(&primary, &[gain.clone(), depth.clone()]).unwrap();
        assert_eq!(secondary_images(&joined).unwrap(), vec![gain, depth]);
        let decoded = codec::decode(&joined, false).unwrap();
        assert_eq!(
            (decoded.width, decoded.height),
            (16, 8),
            "primary still decodes"
        );
    }

    #[test]
    fn appending_nothing_leaves_the_file_alone_and_plain_jpegs_have_no_index() {
        let primary = tiny_jpeg(10);
        assert_eq!(append(&primary, &[]).unwrap(), primary);
        assert!(secondary_images(&primary).is_err());
    }
}
