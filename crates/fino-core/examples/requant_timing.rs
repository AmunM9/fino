//! DCT-domain re-quantization vs pixel re-encode: time, size and score at equal quality.
use fino_core::codec::{self, Effort, EncodeParams};
use fino_core::metric::Reference;
use fino_core::requant::{Coding, Coefficients};
use std::time::Instant;

fn table(quality: f32) -> [u16; 64] {
    let t = mozjpeg::qtable::NRobidoux.scaled(quality, quality);
    // SAFETY: a QTable holds 64 entries.
    let values = unsafe { std::slice::from_raw_parts(t.as_ptr(), 64) };
    std::array::from_fn(|k| values[k] as u16)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let q: f32 = 78.0;
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let chroma = fino_core::jpeg::inspect(&data).unwrap().chroma_block();
        let px = codec::decode(&data, false).unwrap();
        let reference = Reference::new(&px).unwrap();
        let t = Instant::now();
        let mut coefficients = Coefficients::read(&data).unwrap();
        let read = ms(t);
        let tq = table(q);
        let t = Instant::now();
        let probe = coefficients.encode(&tq, &tq, Coding::Probe).unwrap();
        let probe_ms = ms(t);
        let t = Instant::now();
        let fin = coefficients.encode(&tq, &tq, Coding::Final).unwrap();
        let final_ms = ms(t);
        let t = Instant::now();
        let pixel_probe = codec::encode(&px, EncodeParams::new(q, chroma, Effort::Probe)).unwrap();
        let pixel_probe_ms = ms(t);
        let t = Instant::now();
        let pixel_final = codec::encode(&px, EncodeParams::new(q, chroma, Effort::Fast)).unwrap();
        let pixel_final_ms = ms(t);
        let dct = reference
            .compare(&codec::decode(&probe, false).unwrap())
            .unwrap();
        let pix = reference
            .compare(&codec::decode(&pixel_probe, false).unwrap())
            .unwrap();
        println!(
            "{} {chroma:?}: read {read:.0}ms | dct probe {probe_ms:.0}ms final {final_ms:.0}ms {} B global {:.2} worst {:.2} | pixel probe {pixel_probe_ms:.0}ms final {pixel_final_ms:.0}ms {} B global {:.2} worst {:.2}",
            path.rsplit('/').next().unwrap(), fin.len(), dct.global, dct.worst, pixel_final.len(), pix.global, pix.worst
        );
    }
}
