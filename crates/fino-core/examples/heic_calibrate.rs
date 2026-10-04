//! HEIC → JPEG calibration: true SSIMULACRA 2 of each strength against the decoded HEIC,
//! and the size change versus the HEIC.
//!
//!     cargo run --release -p fino-core --example heic_calibrate -- a.heic b.heic …
#[cfg(target_os = "macos")]
fn main() {
    use fino_core::{codec, OptimizeOptions, Outcome, Strength};
    let ssim2 = |a: &codec::Pixels, b: &codec::Pixels| {
        let (ra, rb) = (a.to_rgb(), b.to_rgb());
        let ia = imgref::ImgRef::new(&ra[..], a.width as usize, a.height as usize);
        let ib = imgref::ImgRef::new(&rb[..], b.width as usize, b.height as usize);
        fast_ssim2::compute_ssimulacra2(ia, ib).unwrap_or(f64::NAN)
    };
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let reference = fino_core::heic::decode::decode(&data, false)
            .unwrap()
            .pixels;
        print!(
            "{:<28} {:>6} KB",
            path.rsplit('/').next().unwrap(),
            data.len() / 1000
        );
        for strength in [Strength::Pristine, Strength::Identical, Strength::Compact] {
            let options = OptimizeOptions {
                strength,
                ..Default::default()
            };
            let t = std::time::Instant::now();
            let Outcome::Optimized(o) = fino_core::optimize(&data, &options).unwrap() else {
                print!(" | {strength:?}: skipped");
                continue;
            };
            let secs = t.elapsed().as_secs_f64();
            let out = codec::decode(&o.bytes, false).unwrap();
            let score = if (out.width, out.height) == (reference.width, reference.height) {
                ssim2(&reference, &out)
            } else {
                f64::NAN
            };
            let change = 100.0 * (o.bytes.len() as f64 / data.len() as f64 - 1.0);
            print!(
                " | {strength:?} q{} {:+.0}% ssim2 {:.1} {:.1}s",
                o.quality, change, score, secs
            );
        }
        println!();
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
