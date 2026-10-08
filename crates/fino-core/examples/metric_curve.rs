//! Our scores vs true SSIMULACRA 2 across qualities (probe encoder), for threshold calibration.
use fino_core::codec::{self, Effort, EncodeParams};
use fino_core::metric::Reference;
use std::time::Instant;
fn ssimulacra2(a: &codec::Pixels, b: &codec::Pixels) -> f64 {
    let (ra, rb) = (a.to_rgb(), b.to_rgb());
    let ia = imgref::ImgRef::new(&ra[..], a.width as usize, a.height as usize);
    let ib = imgref::ImgRef::new(&rb[..], b.width as usize, b.height as usize);
    fast_ssim2::compute_ssimulacra2(ia, ib).unwrap_or(f64::NAN)
}
fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let px = codec::decode(&data, false).unwrap();
        let r = Reference::new(&px).unwrap();
        let chroma = fino_core::jpeg::inspect(&data).unwrap().chroma_block();
        println!("{path}");
        for q in [70u8, 76, 80, 84, 87, 90, 92] {
            let jpeg =
                codec::encode(&px, EncodeParams::new(q as f32, chroma, Effort::Probe)).unwrap();
            let d = codec::decode(&jpeg, false).unwrap();
            let t = Instant::now();
            let g = r.global(&r.frame(&d).unwrap()).unwrap();
            let gt = t.elapsed().as_millis();
            let t = Instant::now();
            let score = r.compare(&d).unwrap();
            let (mean, worst) = (score.mean, score.worst);
            let tt = t.elapsed().as_millis();
            let truth = ssimulacra2(&px, &d);
            println!("  q{q} {:>5.1}%  half-global {g:5.1} ({gt}ms)  tiles mean {mean:5.1} worst {worst:5.1} ({tt}ms)  TRUE {truth:5.1}",
                100.0 * (1.0 - jpeg.len() as f64 / data.len() as f64));
        }
    }
}
