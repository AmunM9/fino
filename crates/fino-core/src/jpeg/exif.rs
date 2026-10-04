//! Minimal TIFF/EXIF access: read the orientation, remove the GPS IFD in place.
//!
//! The segment keeps its size so no other offset moves: the GPS pointer entry is
//! removed from IFD0 (entries shift up, count drops by one) and every byte that
//! belonged to the GPS IFD — entries and out-of-line values — is zeroed, so the
//! coordinates are gone rather than merely unreferenced.

use crate::error::{FinoError, Result};

const GPS_IFD_POINTER: u16 = 0x8825;
const ENTRY_SIZE: usize = 12;

#[derive(Clone, Copy)]
struct Tiff {
    big_endian: bool,
}

impl Tiff {
    fn u16(self, b: &[u8], at: usize) -> Result<u16> {
        let s = b
            .get(at..at + 2)
            .ok_or(FinoError::Malformed("EXIF truncated"))?;
        Ok(if self.big_endian {
            u16::from_be_bytes([s[0], s[1]])
        } else {
            u16::from_le_bytes([s[0], s[1]])
        })
    }

    fn u32(self, b: &[u8], at: usize) -> Result<u32> {
        let s = b
            .get(at..at + 4)
            .ok_or(FinoError::Malformed("EXIF truncated"))?;
        let a = [s[0], s[1], s[2], s[3]];
        Ok(if self.big_endian {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        })
    }

    fn put_u16(self, b: &mut [u8], at: usize, v: u16) {
        let bytes = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        b[at..at + 2].copy_from_slice(&bytes);
    }
}

fn type_size(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

fn zero(b: &mut [u8], start: usize, len: usize) -> Result<()> {
    let region = b
        .get_mut(start..start + len)
        .ok_or(FinoError::Malformed("EXIF value out of bounds"))?;
    region.fill(0);
    Ok(())
}

const ORIENTATION: u16 = 0x0112;

fn byte_order(tiff: &[u8]) -> Result<Tiff> {
    match tiff.get(0..2) {
        Some(b"MM") => Ok(Tiff { big_endian: true }),
        Some(b"II") => Ok(Tiff { big_endian: false }),
        _ => Err(FinoError::Malformed("bad TIFF byte order")),
    }
}

/// EXIF orientation (1–8) from IFD0, if present and valid.
pub fn orientation(tiff: &[u8]) -> Option<u16> {
    let t = byte_order(tiff).ok()?;
    let ifd0 = t.u32(tiff, 4).ok()? as usize;
    let count = t.u16(tiff, ifd0).ok()? as usize;
    (0..count).find_map(|i| {
        let entry = ifd0 + 2 + i * ENTRY_SIZE;
        (t.u16(tiff, entry).ok()? == ORIENTATION)
            .then(|| t.u16(tiff, entry + 8).ok())
            .flatten()
            .filter(|o| (1..=8).contains(o))
    })
}

/// `tiff` is the EXIF payload after the `Exif\0\0` header.
pub fn strip_gps(tiff: &mut [u8]) -> Result<()> {
    let t = byte_order(tiff)?;
    let ifd0 = t.u32(tiff, 4)? as usize;
    let count = t.u16(tiff, ifd0)? as usize;
    let entries = ifd0 + 2;

    let Some(index) =
        (0..count).find(|&i| t.u16(tiff, entries + i * ENTRY_SIZE).ok() == Some(GPS_IFD_POINTER))
    else {
        return Ok(());
    };
    let gps_ifd = t.u32(tiff, entries + index * ENTRY_SIZE + 8)? as usize;
    wipe_ifd(t, tiff, gps_ifd)?;

    // Remove the pointer entry from IFD0: shift later entries and the next-IFD
    // offset up by one entry, then clear the freed tail.
    let after = entries + (index + 1) * ENTRY_SIZE;
    let end = entries + count * ENTRY_SIZE + 4;
    if end > tiff.len() {
        return Err(FinoError::Malformed("IFD0 out of bounds"));
    }
    tiff.copy_within(after..end, after - ENTRY_SIZE);
    zero(tiff, end - ENTRY_SIZE, ENTRY_SIZE)?;
    t.put_u16(tiff, ifd0, (count - 1) as u16);
    Ok(())
}

fn wipe_ifd(t: Tiff, tiff: &mut [u8], ifd: usize) -> Result<()> {
    let count = t.u16(tiff, ifd)? as usize;
    for i in 0..count {
        let entry = ifd + 2 + i * ENTRY_SIZE;
        let kind = t.u16(tiff, entry + 2)?;
        let n = t.u32(tiff, entry + 4)? as usize;
        let len = type_size(kind).saturating_mul(n);
        if len > 4 {
            let offset = t.u32(tiff, entry + 8)? as usize;
            zero(tiff, offset, len)?;
        }
    }
    zero(tiff, ifd, 2 + count * ENTRY_SIZE + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little-endian TIFF: IFD0 with [Orientation, GPS pointer, Make], GPS IFD with one
    /// rational latitude stored out of line.
    fn sample() -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0..2].copy_from_slice(b"II");
        b[2..4].copy_from_slice(&42u16.to_le_bytes());
        b[4..8].copy_from_slice(&8u32.to_le_bytes());
        let put_entry = |b: &mut Vec<u8>, at: usize, tag: u16, kind: u16, n: u32, val: u32| {
            b[at..at + 2].copy_from_slice(&tag.to_le_bytes());
            b[at + 2..at + 4].copy_from_slice(&kind.to_le_bytes());
            b[at + 4..at + 8].copy_from_slice(&n.to_le_bytes());
            b[at + 8..at + 12].copy_from_slice(&val.to_le_bytes());
        };
        b[8..10].copy_from_slice(&3u16.to_le_bytes());
        put_entry(&mut b, 10, 0x0112, 3, 1, 6); // Orientation = 6
        put_entry(&mut b, 22, 0x8825, 4, 1, 60); // GPS IFD at 60
        put_entry(&mut b, 34, 0x010F, 2, 4, 0x0041_4243); // Make "CBA\0" inline
        b[46..50].copy_from_slice(&0u32.to_le_bytes()); // next IFD
        b[60..62].copy_from_slice(&1u16.to_le_bytes());
        put_entry(&mut b, 62, 0x0002, 5, 1, 100); // GPSLatitude rational at 100
        b[100..108].copy_from_slice(&[40, 0, 0, 0, 1, 0, 0, 0]);
        b
    }

    #[test]
    fn removes_gps_pointer_and_wipes_coordinates() {
        let mut b = sample();
        strip_gps(&mut b).unwrap();
        assert_eq!(u16::from_le_bytes([b[8], b[9]]), 2);
        assert_eq!(u16::from_le_bytes([b[10], b[11]]), 0x0112);
        assert_eq!(
            u16::from_le_bytes([b[22], b[23]]),
            0x010F,
            "Make shifted into GPS slot"
        );
        assert!(b[100..108].iter().all(|&x| x == 0), "latitude wiped");
        assert!(b[60..78].iter().all(|&x| x == 0), "GPS IFD wiped");
    }

    #[test]
    fn reads_orientation() {
        assert_eq!(orientation(&sample()), Some(6));
        assert_eq!(orientation(b"II*\0"), None);
    }

    #[test]
    fn exif_without_gps_is_untouched() {
        let mut b = sample();
        b[22..24].copy_from_slice(&0x9003u16.to_le_bytes());
        let before = b.clone();
        strip_gps(&mut b).unwrap();
        assert_eq!(b, before);
    }

    #[test]
    fn truncated_exif_is_an_error_not_a_panic() {
        let mut b = sample();
        b.truncate(30);
        assert!(strip_gps(&mut b).is_err());
    }
}
