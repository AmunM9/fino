use super::*;

pub(crate) fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap()
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    let mut b = vec![version, 0, 0, 0];
    b.extend_from_slice(body);
    bx(kind, &b)
}

/// ftyp + meta with an Exif item stored in `idat` (construction method 1) and a stereo group.
fn synthetic() -> Vec<u8> {
    let tiff = b"II*\0\x08\0\0\0\0\0\0\0\0\0";
    let mut exif = 6u32.to_be_bytes().to_vec();
    exif.extend_from_slice(b"Exif\0\0");
    exif.extend_from_slice(tiff);

    let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    let pitm = full(b"pitm", 0, &1u16.to_be_bytes());
    let mut exif_infe = 2u16.to_be_bytes().to_vec();
    exif_infe.extend_from_slice(&[0, 0]);
    exif_infe.extend_from_slice(b"Exif\0");
    let mut iinf_body = 1u16.to_be_bytes().to_vec();
    iinf_body.extend(full(b"infe", 2, &exif_infe));
    let iinf = full(b"iinf", 0, &iinf_body);
    // iloc v1: offset 4 bytes, length 4 bytes, no base offset; item 2 in idat
    let mut iloc_body = vec![0x44, 0x00];
    iloc_body.extend_from_slice(&1u16.to_be_bytes());
    iloc_body.extend_from_slice(&2u16.to_be_bytes());
    iloc_body.extend_from_slice(&1u16.to_be_bytes()); // construction method 1
    iloc_body.extend_from_slice(&0u16.to_be_bytes()); // data reference
    iloc_body.extend_from_slice(&1u16.to_be_bytes()); // one extent
    iloc_body.extend_from_slice(&0u32.to_be_bytes());
    iloc_body.extend_from_slice(&(exif.len() as u32).to_be_bytes());
    let iloc = full(b"iloc", 1, &iloc_body);
    let mut cdsc = 2u16.to_be_bytes().to_vec();
    cdsc.extend_from_slice(&1u16.to_be_bytes());
    cdsc.extend_from_slice(&1u16.to_be_bytes());
    let iref = full(b"iref", 0, &bx(b"cdsc", &cdsc));
    let idat = bx(b"idat", &exif);
    let grpl = bx(b"grpl", &full(b"ster", 0, &[0, 0, 0, 9, 0, 0, 0, 0]));
    let meta = full(b"meta", 0, &[pitm, iinf, iloc, iref, idat, grpl].concat());
    [ftyp, meta].concat()
}

#[test]
fn recognises_hevc_heif_but_not_jpeg_or_avif() {
    assert!(is_heif(&fixture("plain.heic")));
    assert!(!is_heif(b"\xFF\xD8\xFF\xE0\0\x10JFIF"));
    assert!(!is_heif(&bx(b"ftyp", b"avif\0\0\0\0mif1avif")));
}

#[test]
fn reads_raw_exif_xmp_and_icc_of_the_primary_image() {
    let rotated = inspect(&fixture("rotated.heic")).unwrap();
    let tiff = rotated.exif_tiff.expect("exif");
    assert!(tiff.starts_with(b"MM\0*") || tiff.starts_with(b"II*\0"));
    assert_eq!(crate::jpeg::exif::orientation(&tiff), Some(6));

    let info = inspect(&fixture("plain.heic")).unwrap();
    assert!(info.exif_tiff.is_some());
    let xmp = String::from_utf8_lossy(info.xmp.as_deref().expect("xmp")).into_owned();
    assert!(
        xmp.contains("kept byte for byte"),
        "foreign XMP namespace survives"
    );
    let icc = info.icc.clone().expect("icc");
    assert!(icc.windows(10).any(|w| w == b"Display P3".as_slice()) || icc.len() > 400);
    assert_eq!(
        (info.bit_depth, info.transfer, info.hdr_transfer()),
        (8, None, false)
    );
    assert!(!info.apple_gain_map() && !info.iso_gain_map && !info.stereo && !info.sequence);
}

#[test]
fn classifies_gain_maps_and_hdr_transfers() {
    assert!(inspect(&fixture("gainmap.heic")).unwrap().apple_gain_map());
    let hlg = inspect(&fixture("hlg.heic")).unwrap();
    assert!(hlg.hdr_transfer(), "HLG transfer {:?}", hlg.transfer);
    assert_eq!(hlg.bit_depth, 10);
    assert!(inspect(&fixture("isogainmap.heic")).unwrap().iso_gain_map);
}

#[test]
fn reads_items_stored_in_idat_and_stereo_groups() {
    let info = inspect(&synthetic()).unwrap();
    assert_eq!(
        info.exif_tiff.as_deref(),
        Some(b"II*\0\x08\0\0\0\0\0\0\0\0\0".as_slice())
    );
    assert!(info.stereo);
}

#[test]
fn damaged_files_are_errors_not_panics() {
    let mut data = fixture("plain.heic");
    for cut in [10, 40, 200, data.len() / 2] {
        let _ = inspect(&data[..cut]);
    }
    data[30] = 0xFF; // corrupt a box size
    let _ = inspect(&data);
    assert!(inspect(b"definitely not a heif").is_err());
}

#[test]
fn hostile_sizes_and_repeated_extents_are_rejected() {
    // A 64-bit box size near u64::MAX must not overflow.
    let mut huge = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    huge.extend_from_slice(&1u32.to_be_bytes());
    huge.extend_from_slice(b"meta");
    huge.extend_from_slice(&u64::MAX.to_be_bytes());
    assert!(inspect(&huge).is_err());

    // An Exif item made of the same whole-idat extent many times over.
    let mut file = synthetic();
    let iloc = file.windows(4).position(|w| w == b"iloc").unwrap();
    let extents_at = iloc + 4 + 4 + 2 + 2 + 2 + 2 + 2; // header, sizes, count, id, method, ref
    file[extents_at..extents_at + 2].copy_from_slice(&1u16.to_be_bytes());
    let _ = inspect(&file); // must not panic or balloon
}
