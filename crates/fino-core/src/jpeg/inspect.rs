//! Lightweight JPEG header walker: frame type, sampling factors, quantization
//! tables and markers — everything needed to decide *how* to re-encode,
//! without decoding a single pixel.

use crate::error::{FinoError, Result};

/// Prefix of the COM segment Fino writes into every file it produces.
pub const FINO_MARKER_PREFIX: &[u8] = b"Fino/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Baseline,
    ExtendedSequential,
    Progressive,
    /// Lossless, hierarchical or arithmetic-coded: valid JPEG, but nothing we re-encode.
    Exotic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub id: u8,
    pub h: u8,
    pub v: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderInfo {
    pub width: u32,
    pub height: u32,
    pub precision: u8,
    pub kind: FrameKind,
    pub components: Vec<Component>,
    /// libjpeg-equivalent quality (1–100) estimated from the luma table.
    pub quality_estimate: Option<u8>,
    pub has_fino_marker: bool,
    /// Adobe APP14 present with 4 components → CMYK/YCCK.
    pub adobe_transform: Option<u8>,
}

impl HeaderInfo {
    pub fn is_cmyk(&self) -> bool {
        self.components.len() == 4
    }

    pub fn is_grayscale(&self) -> bool {
        self.components.len() == 1
    }

    /// Chroma subsampling as (horizontal, vertical) pixel block size of the chroma planes.
    /// 4:2:0 → (2, 2), 4:2:2 → (2, 1), 4:4:4 → (1, 1).
    pub fn chroma_block(&self) -> (u8, u8) {
        let Some(luma) = self.components.first() else {
            return (1, 1);
        };
        let chroma = self.components.get(1).copied().unwrap_or(*luma);
        let h = (luma.h / chroma.h.max(1)).clamp(1, 4);
        let v = (luma.v / chroma.v.max(1)).clamp(1, 4);
        (h, v)
    }
}

/// JPEG zig-zag scan position → natural (row-major) index.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// ITU-T T.81 Annex K luminance table (natural order), the base of libjpeg's quality scale.
const STD_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

pub fn inspect(data: &[u8]) -> Result<HeaderInfo> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(FinoError::Malformed("missing SOI"));
    }
    let mut info = HeaderInfo {
        width: 0,
        height: 0,
        precision: 8,
        kind: FrameKind::Baseline,
        components: Vec::new(),
        quality_estimate: None,
        has_fino_marker: false,
        adobe_transform: None,
    };
    let mut luma_table: Option<[u16; 64]> = None;
    let mut seen_frame = false;
    let mut pos = 2;

    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            return Err(FinoError::Malformed("expected marker"));
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1; // fill byte
            continue;
        }
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > data.len() {
            return Err(FinoError::Malformed("segment overruns file"));
        }
        let body = &data[pos + 4..pos + 2 + len];
        match marker {
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => {
                parse_frame(marker, body, &mut info)?;
                seen_frame = true;
            }
            0xDB => {
                if let Some(table) = parse_luma_table(body) {
                    luma_table.get_or_insert(table);
                }
            }
            0xEE if body.starts_with(b"Adobe") && body.len() >= 12 => {
                info.adobe_transform = Some(body[11]);
            }
            0xFE if body.starts_with(FINO_MARKER_PREFIX) => info.has_fino_marker = true,
            0xDA | 0xD9 => break,
            _ => {}
        }
        pos += 2 + len;
    }

    if !seen_frame {
        return Err(FinoError::Malformed("no frame header"));
    }
    info.quality_estimate = luma_table.map(|t| estimate_quality(&t));
    Ok(info)
}

fn parse_frame(marker: u8, body: &[u8], info: &mut HeaderInfo) -> Result<()> {
    if body.len() < 6 {
        return Err(FinoError::Malformed("short frame header"));
    }
    info.kind = match marker {
        0xC0 => FrameKind::Baseline,
        0xC1 => FrameKind::ExtendedSequential,
        0xC2 => FrameKind::Progressive,
        _ => FrameKind::Exotic,
    };
    info.precision = body[0];
    info.height = u16::from_be_bytes([body[1], body[2]]) as u32;
    info.width = u16::from_be_bytes([body[3], body[4]]) as u32;
    let count = body[5] as usize;
    if body.len() < 6 + count * 3 {
        return Err(FinoError::Malformed("short component list"));
    }
    info.components = body[6..6 + count * 3]
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| Component {
            id: c[0],
            h: (c[1] >> 4).max(1),
            v: (c[1] & 0x0F).max(1),
        })
        .collect();
    Ok(())
}

/// Returns table 0 (luma by convention) in natural order, if this DQT segment defines it.
fn parse_luma_table(body: &[u8]) -> Option<[u16; 64]> {
    let mut pos = 0;
    while pos < body.len() {
        let precision = body[pos] >> 4;
        let id = body[pos] & 0x0F;
        let size = if precision == 0 { 64 } else { 128 };
        let values = body.get(pos + 1..pos + 1 + size)?;
        if id == 0 {
            let mut table = [0u16; 64];
            for (scan, &natural) in ZIGZAG.iter().enumerate() {
                table[natural] = if precision == 0 {
                    values[scan] as u16
                } else {
                    u16::from_be_bytes([values[scan * 2], values[scan * 2 + 1]])
                };
            }
            return Some(table);
        }
        pos += 1 + size;
    }
    None
}

/// Inverts libjpeg's quality scaling using the median per-coefficient ratio, which is
/// robust to the custom tweaks camera makers apply to individual coefficients.
pub fn estimate_quality(table: &[u16; 64]) -> u8 {
    let mut ratios: Vec<f64> = table
        .iter()
        .zip(STD_LUMA.iter())
        .map(|(&q, &s)| q.max(1) as f64 * 100.0 / s as f64)
        .collect();
    ratios.sort_by(|a, b| a.total_cmp(b));
    let scale = ratios[ratios.len() / 2];
    let quality = if scale <= 100.0 {
        (200.0 - scale) / 2.0
    } else {
        5000.0 / scale
    };
    quality.round().clamp(1.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled_table(quality: u32) -> [u16; 64] {
        let scale = if quality < 50 {
            5000 / quality
        } else {
            200 - quality * 2
        };
        let mut t = [0u16; 64];
        for (i, &s) in STD_LUMA.iter().enumerate() {
            t[i] = ((s as u32 * scale + 50) / 100).clamp(1, 255) as u16;
        }
        t
    }

    #[test]
    fn estimates_libjpeg_quality_from_luma_table() {
        for q in [50u32, 75, 85, 92] {
            let est = estimate_quality(&scaled_table(q)) as i32;
            assert!((est - q as i32).abs() <= 1, "q{q} estimated as {est}");
        }
    }

    #[test]
    fn rejects_non_jpeg() {
        assert!(inspect(b"\x89PNG\r\n\x1a\n").is_err());
    }

    #[test]
    fn chroma_block_maps_sampling_factors() {
        let mut info = HeaderInfo {
            width: 8,
            height: 8,
            precision: 8,
            kind: FrameKind::Baseline,
            components: vec![
                Component { id: 1, h: 2, v: 2 },
                Component { id: 2, h: 1, v: 1 },
                Component { id: 3, h: 1, v: 1 },
            ],
            quality_estimate: None,
            has_fino_marker: false,
            adobe_transform: None,
        };
        assert_eq!(info.chroma_block(), (2, 2));
        info.components[0] = Component { id: 1, h: 2, v: 1 };
        assert_eq!(info.chroma_block(), (2, 1));
        info.components[0] = Component { id: 1, h: 1, v: 1 };
        assert_eq!(info.chroma_block(), (1, 1));
    }
}
