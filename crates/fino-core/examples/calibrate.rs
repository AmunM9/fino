//! Calibration harness: runs every strength over a folder of JPEGs and checks the
//! search's zensim verdict against true SSIMULACRA 2 (fast-ssim2).
//!
//!     cargo run --release -p fino-core --example calibrate -- <folder>

use fino_core::{codec, optimize, OptimizeOptions, Outcome, Strength};
use std::path::PathBuf;
use std::time::Instant;

fn ssimulacra2(a: &codec::Pixels, b: &codec::Pixels) -> f64 {
    let (ra, rb) = (a.to_rgb(), b.to_rgb());
    let ia = imgref::ImgRef::new(&ra[..], a.width as usize, a.height as usize);
    let ib = imgref::ImgRef::new(&rb[..], b.width as usize, b.height as usize);
    fast_ssim2::compute_ssimulacra2(ia, ib).unwrap_or(f64::NAN)
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: calibrate <folder>"));
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("readable folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| fino_core::files::has_jpeg_extension(p))
        .collect();
    files.sort();

    println!(
        "{:<16} {:>9} {:>9} {:>4} {:>7} {:>6} {:>6} {:>6} {:>7}",
        "file", "strength", "bytes", "q", "saved", "global", "worst", "ssim2", "ms"
    );
    for strength in [Strength::Pristine, Strength::Identical, Strength::Compact] {
        let (mut total_in, mut total_out) = (0usize, 0usize);
        let mut worst_ssim2 = f64::INFINITY;
        let started = Instant::now();
        for path in &files {
            let data = std::fs::read(path).expect("readable file");
            let options = OptimizeOptions {
                strength,
                ..Default::default()
            };
            let start = Instant::now();
            let outcome = optimize(&data, &options).expect("optimize");
            let ms = start.elapsed().as_millis();
            let name = path.file_name().unwrap().to_string_lossy();
            total_in += data.len();
            match outcome {
                Outcome::Optimized(o) => {
                    total_out += o.bytes.len();
                    let src = codec::decode(&data, false).unwrap();
                    let out = codec::decode(&o.bytes, false).unwrap();
                    let truth = ssimulacra2(&src, &out);
                    worst_ssim2 = worst_ssim2.min(truth);
                    println!(
                        "{name:<16} {strength:>9?} {:>9} {:>4} {:>6.1}% {:>6.1} {:>6.1} {:>6.1} {ms:>7}",
                        o.bytes.len(), o.quality, 100.0 * (1.0 - o.bytes.len() as f64 / data.len() as f64),
                        o.score.global, o.score.worst, truth
                    );
                }
                Outcome::Skipped(reason) => {
                    total_out += data.len();
                    println!("{name:<16} {strength:>9?} skipped: {reason:?} ({ms} ms)");
                }
            }
        }
        println!(
            "== {strength:?}: {:.1}% saved overall, lowest true SSIMULACRA2 {worst_ssim2:.1}, {:.1}s\n",
            100.0 * (1.0 - total_out as f64 / total_in as f64),
            started.elapsed().as_secs_f64()
        );
    }
}
