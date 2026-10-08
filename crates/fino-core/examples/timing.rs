//! Stage timings on one photo, as the search runs them (use RAYON_NUM_THREADS=1 for CPU cost).
use fino_core::codec;
use fino_core::metric::Reference;
use fino_core::requant::{Coding, Coefficients};
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
    let mut coefficients = t("read coefficients", || Coefficients::read(&data).unwrap());
    let r = t("reference (global + coarse tiles)", || {
        Reference::new(&px).unwrap()
    });
    let table = codec::quant_table(78.0);
    let probe = t("requantize + probe encode", || {
        coefficients.encode(&table, &table, Coding::Probe).unwrap()
    });
    let d = t("decode probe", || codec::decode(&probe, false).unwrap());
    let frame = t("candidate frame (half res)", || r.frame(&d).unwrap());
    let (_, difficulty) = t("global + difficulty map", || {
        r.global_and_difficulty(&frame).unwrap()
    });
    let watch: Vec<usize> = (0..10).collect();
    t("10 full tiles (cold)", || {
        r.tile_scores(&frame, &watch).unwrap()
    });
    t("10 full tiles (warm)", || {
        r.tile_scores(&frame, &watch).unwrap()
    });
    let g = t("global", || r.global(&frame).unwrap());
    t("final encode", || {
        coefficients.encode(&table, &table, Coding::Final).unwrap()
    });
    println!("global {g:.1}, {} tiles", difficulty.len());
}
