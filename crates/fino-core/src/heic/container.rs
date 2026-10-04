//! Minimal HEIF (ISO/IEC 23008-12) container reader: just enough of ISOBMFF to find the
//! primary image's metadata — the raw EXIF and XMP items and the ICC profile — and to
//! classify the file (HDR transfer, gain maps, auxiliary images, stereo, sequences).
//!
//! Pixels are decoded by the operating system; this module never touches HEVC data.
//! Metadata is copied byte for byte: ImageIO's own export path drops unknown XMP namespaces.

use crate::error::{FinoError, Result};
use std::collections::HashMap;

const HEVC_BRANDS: [&[u8; 4]; 6] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx"];
const SEQUENCE_BRANDS: [&[u8; 4]; 2] = [b"msf1", b"hevc"];
const APPLE_GAIN_MAP_URN: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";
const XMP_CONTENT_TYPE: &str = "application/rdf+xml";
/// CICP transfer characteristics that a JPEG cannot carry: PQ (16) and HLG (18).
const HDR_TRANSFERS: [u16; 2] = [16, 18];

/// What Fino needs to know about a HEIF file, read without decoding any pixels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeifInfo {
    /// TIFF payload of the primary image's EXIF item (starts with `MM\0*` or `II*\0`).
    pub exif_tiff: Option<Vec<u8>>,
    /// Raw XMP packet describing the primary image.
    pub xmp: Option<Vec<u8>>,
    /// Embedded ICC profile (`colr` of type `prof`/`rICC`).
    pub icc: Option<Vec<u8>>,
    /// CICP transfer characteristic when colour is signalled as `nclx`.
    pub transfer: Option<u16>,
    /// Bits per channel of the primary image (`pixi`), 8 when unsignalled.
    pub bit_depth: u8,
    /// URNs of auxiliary images attached to the primary (gain map, depth, mattes…).
    pub aux_types: Vec<String>,
    /// An ISO 21496-1 gain map (`tmap` derived item).
    pub iso_gain_map: bool,
    /// Part of a stereo pair (spatial photo).
    pub stereo: bool,
    /// An image sequence (`.heics`) rather than a still.
    pub sequence: bool,
}

impl HeifInfo {
    pub fn apple_gain_map(&self) -> bool {
        self.aux_types.iter().any(|t| t == APPLE_GAIN_MAP_URN)
    }

    /// PQ/HLG primary image: HDR that an 8-bit JPEG cannot hold.
    pub fn hdr_transfer(&self) -> bool {
        self.transfer.is_some_and(|t| HDR_TRANSFERS.contains(&t))
    }
}

/// True for HEVC-coded HEIF stills and sequences (not AVIF).
pub fn is_heif(data: &[u8]) -> bool {
    brands(data).is_some_and(|b| b.iter().any(|brand| HEVC_BRANDS.contains(&brand)))
}

// ---------------------------------------------------------------------------------------
// Byte reading

#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

fn truncated() -> FinoError {
    FinoError::Malformed("HEIF box truncated")
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(truncated)?;
        let slice = self.data.get(self.pos..end).ok_or_else(truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Unsigned big-endian integer of 0, 4 or 8 bytes (iloc field sizes).
    fn uint(&mut self, size: u8) -> Result<u64> {
        match size {
            0 => Ok(0),
            4 => self.u32().map(u64::from),
            8 => {
                let b = self.bytes(8)?;
                Ok(u64::from_be_bytes(b.try_into().map_err(|_| truncated())?))
            }
            _ => Err(FinoError::Malformed("unsupported HEIF field size")),
        }
    }

    fn id(&mut self, wide: bool) -> Result<u32> {
        if wide {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }

    fn cstring(&mut self) -> Result<String> {
        let rest = &self.data[self.pos.min(self.data.len())..];
        let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let text = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += (len + 1).min(rest.len());
        Ok(text)
    }

    /// Version and flags of a FullBox.
    fn full_box(&mut self) -> Result<(u8, u32)> {
        let word = self.u32()?;
        Ok(((word >> 24) as u8, word & 0x00FF_FFFF))
    }
}

struct BoxRef<'a> {
    kind: [u8; 4],
    body: &'a [u8],
    /// Offset of `body` in the file (iloc offsets are absolute).
    start: usize,
}

/// Iterates the boxes laid out back to back in `data` (which begins at `base` in the file).
fn boxes(data: &[u8], base: usize) -> Result<Vec<BoxRef<'_>>> {
    let mut out = Vec::new();
    let mut r = Reader::new(data);
    while r.remaining() >= 8 {
        let at = r.pos;
        let size32 = r.u32()?;
        let kind: [u8; 4] = r.bytes(4)?.try_into().map_err(|_| truncated())?;
        let (header, size) = match size32 {
            1 => (16, usize::try_from(r.uint(8)?).unwrap_or(usize::MAX)),
            0 => (8, data.len() - at),
            n => (8, n as usize),
        };
        let end = at.checked_add(size).filter(|&end| end <= data.len());
        let Some(_) = end.filter(|_| size >= header) else {
            return Err(FinoError::Malformed("HEIF box overruns its parent"));
        };
        out.push(BoxRef {
            kind,
            body: &data[at + header..at + size],
            start: base + at + header,
        });
        r.pos = at + size;
    }
    Ok(out)
}

/// Major brand followed by the compatible brands of the leading `ftyp` box.
fn brands(data: &[u8]) -> Option<Vec<[u8; 4]>> {
    if data.get(4..8)? != b"ftyp" {
        return None;
    }
    let size = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
    let body = data.get(8..size)?;
    let mut list = vec![body.get(..4)?.try_into().ok()?];
    list.extend(body.get(8..)?.as_chunks::<4>().0.iter().copied());
    Some(list)
}

// ---------------------------------------------------------------------------------------
// meta box

#[derive(Default)]
struct Item {
    kind: [u8; 4],
    content_type: String,
}

struct Extent {
    offset: u64,
    length: u64,
}

struct Location {
    method: u16,
    base: u64,
    extents: Vec<Extent>,
}

#[derive(Default)]
struct Meta<'a> {
    primary: u32,
    items: HashMap<u32, Item>,
    locations: HashMap<u32, Location>,
    /// (reference type, from item, to items)
    refs: Vec<([u8; 4], u32, Vec<u32>)>,
    properties: Vec<BoxRef<'a>>,
    associations: HashMap<u32, Vec<usize>>,
    idat: Option<BoxRef<'a>>,
    stereo: bool,
}

fn parse_pitm(b: &BoxRef) -> Result<u32> {
    let mut r = Reader::new(b.body);
    let (version, _) = r.full_box()?;
    r.id(version >= 1)
}

fn parse_iinf(b: &BoxRef, items: &mut HashMap<u32, Item>) -> Result<()> {
    let mut r = Reader::new(b.body);
    let (version, _) = r.full_box()?;
    let _count = if version == 0 {
        r.u16()? as u32
    } else {
        r.u32()?
    };
    for infe in boxes(&b.body[r.pos..], b.start + r.pos)? {
        if &infe.kind != b"infe" {
            continue;
        }
        let mut e = Reader::new(infe.body);
        let (v, _) = e.full_box()?;
        if v < 2 {
            continue; // legacy entries carry no item type; nothing we use
        }
        let id = e.id(v >= 3)?;
        let _protection = e.u16()?;
        let kind: [u8; 4] = e.bytes(4)?.try_into().map_err(|_| truncated())?;
        let _name = e.cstring()?;
        let content_type = if &kind == b"mime" {
            e.cstring()?
        } else {
            String::new()
        };
        items.insert(id, Item { kind, content_type });
    }
    Ok(())
}

fn parse_iloc(b: &BoxRef, locations: &mut HashMap<u32, Location>) -> Result<()> {
    let mut r = Reader::new(b.body);
    let (version, _) = r.full_box()?;
    let sizes = r.u16()?;
    let (offset_size, length_size) = ((sizes >> 12) as u8, ((sizes >> 8) & 0xF) as u8);
    let base_offset_size = ((sizes >> 4) & 0xF) as u8;
    let index_size = if version >= 1 { (sizes & 0xF) as u8 } else { 0 };
    let count = if version < 2 {
        r.u16()? as u32
    } else {
        r.u32()?
    };
    for _ in 0..count {
        let id = r.id(version >= 2)?;
        let method = if version >= 1 { r.u16()? & 0xF } else { 0 };
        let _data_reference = r.u16()?;
        let base = r.uint(base_offset_size)?;
        let extent_count = r.u16()?;
        let mut extents = Vec::with_capacity(extent_count as usize);
        for _ in 0..extent_count {
            let _index = r.uint(index_size)?;
            let offset = r.uint(offset_size)?;
            let length = r.uint(length_size)?;
            extents.push(Extent { offset, length });
        }
        locations.insert(
            id,
            Location {
                method,
                base,
                extents,
            },
        );
    }
    Ok(())
}

fn parse_iref(b: &BoxRef, refs: &mut Vec<([u8; 4], u32, Vec<u32>)>) -> Result<()> {
    let mut r = Reader::new(b.body);
    let (version, _) = r.full_box()?;
    for reference in boxes(&b.body[r.pos..], b.start + r.pos)? {
        let mut e = Reader::new(reference.body);
        let from = e.id(version >= 1)?;
        let count = e.u16()?;
        let to = (0..count)
            .map(|_| e.id(version >= 1))
            .collect::<Result<Vec<_>>>()?;
        refs.push((reference.kind, from, to));
    }
    Ok(())
}

fn parse_ipma(b: &BoxRef, associations: &mut HashMap<u32, Vec<usize>>) -> Result<()> {
    let mut r = Reader::new(b.body);
    let (version, flags) = r.full_box()?;
    let count = r.u32()?;
    for _ in 0..count {
        let id = r.id(version >= 1)?;
        let n = r.u8()?;
        let mut indices = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let index = if flags & 1 != 0 {
                (r.u16()? & 0x7FFF) as usize
            } else {
                (r.u8()? & 0x7F) as usize
            };
            if index > 0 {
                indices.push(index - 1); // 1-based; 0 means "no property"
            }
        }
        associations.entry(id).or_default().extend(indices);
    }
    Ok(())
}

fn parse_meta<'a>(meta: &BoxRef<'a>) -> Result<Meta<'a>> {
    let mut r = Reader::new(meta.body);
    r.full_box()?;
    let mut out = Meta::default();
    for b in boxes(&meta.body[4..], meta.start + 4)? {
        match &b.kind {
            b"pitm" => out.primary = parse_pitm(&b)?,
            b"iinf" => parse_iinf(&b, &mut out.items)?,
            b"iloc" => parse_iloc(&b, &mut out.locations)?,
            b"iref" => parse_iref(&b, &mut out.refs)?,
            b"iprp" => {
                for child in boxes(b.body, b.start)? {
                    match &child.kind {
                        b"ipco" => out.properties = boxes(child.body, child.start)?,
                        b"ipma" => parse_ipma(&child, &mut out.associations)?,
                        _ => {}
                    }
                }
            }
            b"grpl" => out.stereo |= boxes(b.body, b.start)?.iter().any(|g| &g.kind == b"ster"),
            b"idat" => out.idat = Some(b),
            _ => {}
        }
    }
    Ok(out)
}

impl Meta<'_> {
    fn item_bytes(&self, file: &[u8], id: u32) -> Result<Vec<u8>> {
        let loc = self
            .locations
            .get(&id)
            .ok_or(FinoError::Malformed("HEIF item has no location"))?;
        let source: (&[u8], u64) = match loc.method {
            0 => (file, 0),
            1 => {
                let idat = self
                    .idat
                    .as_ref()
                    .ok_or(FinoError::Malformed("HEIF idat missing"))?;
                (idat.body, 0)
            }
            _ => return Err(FinoError::Malformed("HEIF item stored by reference")),
        };
        // A hostile index can repeat or overlap extents; no item may be larger than the
        // data it lives in, which bounds the allocation.
        let outside = || FinoError::Malformed("HEIF item outside the file");
        let limit = source.0.len();
        let mut out = Vec::new();
        for e in &loc.extents {
            let start = loc
                .base
                .checked_add(e.offset)
                .and_then(|v| v.checked_add(source.1))
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(outside)?;
            let end = if e.length == 0 {
                limit
            } else {
                usize::try_from(e.length)
                    .ok()
                    .and_then(|len| start.checked_add(len))
                    .ok_or_else(outside)?
            };
            let slice = source.0.get(start..end).ok_or_else(outside)?;
            if out.len() + slice.len() > limit {
                return Err(outside());
            }
            out.extend_from_slice(slice);
        }
        Ok(out)
    }

    fn properties_of(&self, id: u32) -> impl Iterator<Item = &BoxRef<'_>> {
        self.associations
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|&i| self.properties.get(i))
    }

    /// Items that `reference` points from to `to` (e.g. cdsc metadata of the primary).
    fn referencing(&self, reference: &[u8; 4], to: u32) -> Vec<u32> {
        self.refs
            .iter()
            .filter(|(kind, _, targets)| kind == reference && targets.contains(&to))
            .map(|(_, from, _)| *from)
            .collect()
    }

    /// The primary plus, for a grid, its first tile: colour and depth often live there.
    fn primary_and_tile(&self) -> Vec<u32> {
        let tile = self
            .refs
            .iter()
            .find(|(kind, from, _)| kind == b"dimg" && *from == self.primary)
            .and_then(|(_, _, to)| to.first().copied());
        std::iter::once(self.primary).chain(tile).collect()
    }
}

/// `exif_tiff_header_offset` (u32) precedes the TIFF header; Apple writes 6 and a literal
/// `Exif\0\0`. Fall back to scanning for the TIFF signature.
fn tiff_from_exif_item(payload: &[u8]) -> Option<Vec<u8>> {
    let is_tiff = |b: &[u8]| b.starts_with(b"MM\0*") || b.starts_with(b"II*\0");
    let offset = u32::from_be_bytes(payload.get(..4)?.try_into().ok()?) as usize;
    let declared = payload.get(4 + offset..).filter(|b| is_tiff(b));
    let found = declared.or_else(|| {
        let window = payload.get(..payload.len().min(64))?;
        let at = (0..window.len().saturating_sub(4)).find(|&i| is_tiff(&payload[i..]))?;
        payload.get(at..)
    });
    found.map(<[u8]>::to_vec)
}

fn colour(info: &mut HeifInfo, property: &BoxRef) -> Result<()> {
    let mut r = Reader::new(property.body);
    let kind = r.bytes(4)?;
    match kind {
        b"prof" | b"rICC" if info.icc.is_none() => {
            info.icc = Some(property.body[4..].to_vec());
        }
        b"nclx" if info.transfer.is_none() => {
            let _primaries = r.u16()?;
            info.transfer = Some(r.u16()?);
        }
        _ => {}
    }
    Ok(())
}

/// Reads everything Fino needs from a HEIF file without decoding pixels.
pub fn inspect(file: &[u8]) -> Result<HeifInfo> {
    let brands = brands(file).ok_or(FinoError::Malformed("not a HEIF file"))?;
    let top = boxes(file, 0)?;
    let meta = top.iter().find(|b| &b.kind == b"meta");
    let has_movie = top.iter().any(|b| &b.kind == b"moov");
    let major_is_sequence = SEQUENCE_BRANDS.contains(&&brands[0]);
    let Some(meta) = meta else {
        return Ok(HeifInfo {
            sequence: has_movie || major_is_sequence,
            bit_depth: 8,
            ..HeifInfo::default()
        });
    };
    let meta = parse_meta(meta)?;
    let mut info = HeifInfo {
        sequence: major_is_sequence && has_movie,
        stereo: meta.stereo,
        bit_depth: 8,
        iso_gain_map: meta.items.values().any(|i| &i.kind == b"tmap"),
        ..HeifInfo::default()
    };

    for id in meta.referencing(b"cdsc", meta.primary) {
        let Some(item) = meta.items.get(&id) else {
            continue;
        };
        match &item.kind {
            b"Exif" if info.exif_tiff.is_none() => {
                info.exif_tiff = tiff_from_exif_item(&meta.item_bytes(file, id)?);
            }
            b"mime" if info.xmp.is_none() && item.content_type == XMP_CONTENT_TYPE => {
                info.xmp = Some(meta.item_bytes(file, id)?);
            }
            _ => {}
        }
    }

    for id in meta.primary_and_tile() {
        for property in meta.properties_of(id) {
            match &property.kind {
                b"colr" => colour(&mut info, property)?,
                b"pixi" if info.bit_depth == 8 => {
                    let mut r = Reader::new(property.body);
                    r.full_box()?;
                    if r.u8()? > 0 {
                        info.bit_depth = r.u8()?;
                    }
                }
                _ => {}
            }
        }
    }

    for aux in meta.referencing(b"auxl", meta.primary) {
        for property in meta.properties_of(aux) {
            if &property.kind == b"auxC" {
                let mut r = Reader::new(property.body);
                r.full_box()?;
                info.aux_types.push(r.cstring()?);
            }
        }
    }
    Ok(info)
}

#[cfg(test)]
pub(crate) mod tests;
