//! Stage timings on one photo.
use fino_core::{codec, metric::Reference};
use std::time::Instant;
fn t<R>(label: &str, f: impl FnOnce() -> R) -> R {
    let s = Instant::now();
    let r = f();
    println!("{label:<34} {:>7.0} ms", s.elapsed().as_secs_f64() * 1000.0);
    r
}
fn main() {
    let data = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let px = t("decode original", || codec::decode(&data, false).unwrap());
    let r = t("reference (global + tiles)", || {
        Reference::new(&px).unwrap()
    });
    let p = codec::EncodeParams::new(85.0, (1, 1), codec::Effort::Max);
    let jpeg = t("encode trellis progressive", || {
        codec::encode(&px, p).unwrap()
    });
    let d = t("decode candidate", || codec::decode(&jpeg, false).unwrap());
    let g = t("metric global", || r.global(&d).unwrap());
    let tl = t("metric tiles", || r.tiles(&d).unwrap());
    println!("global {g:.1} tiles {tl:?}");
}
