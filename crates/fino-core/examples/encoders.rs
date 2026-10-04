//! Encoder variants on one photo: size, time, and pixel identity.
use std::time::Instant;
fn enc(px: &fino_core::codec::Pixels, q: f32, mode: &str) -> (Vec<u8>, f64) {
    let s = Instant::now();
    let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    if mode == "fast" {
        c.set_fastest_defaults();
    }
    c.set_size(px.width as usize, px.height as usize);
    c.set_quality(q);
    c.set_chroma_sampling_pixel_sizes((1, 1), (1, 1));
    match mode {
        "baseline" => {
            c.set_optimize_scans(false);
        }
        "prog" => {
            c.set_optimize_scans(false);
            c.set_progressive_mode();
        }
        "progopt" => {
            c.set_progressive_mode();
            c.set_optimize_scans(true);
        }
        _ => {}
    }
    c.set_optimize_coding(true);
    let mut st = c.start_compress(Vec::new()).unwrap();
    st.write_scanlines(&px.data).unwrap();
    let out = st.finish().unwrap();
    (out, s.elapsed().as_secs_f64() * 1000.0)
}
fn main() {
    let data = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let px = fino_core::codec::decode(&data, false).unwrap();
    for q in [80.0f32] {
        let mut ref_px = None;
        for mode in ["baseline", "prog", "progopt", "fast"] {
            let (out, ms) = enc(&px, q, mode);
            let d = fino_core::codec::decode(&out, false).unwrap();
            let same = ref_px
                .as_ref()
                .map(|r: &fino_core::codec::Pixels| r.data == d.data);
            if ref_px.is_none() {
                ref_px = Some(d);
            }
            println!(
                "q{q} {mode:<10} {:>9} bytes {ms:>7.0} ms  same-pixels-as-baseline={same:?}",
                out.len()
            );
        }
    }
}
