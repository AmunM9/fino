//! Head-to-head against another optimizer's outputs of the same camera files: wall time
//! (app-like parallelism), size and true SSIMULACRA 2.
//!
//!     cargo run --release -p fino-core --example vs_reference -- <originals> <reference> [count] [strength]
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (orig, reference) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let count: usize = args.get(2).and_then(|c| c.parse().ok()).unwrap_or(12);
    let strength = match args.get(3).map(String::as_str) {
        Some("pristine") => Strength::Pristine,
        Some("compact") => Strength::Compact,
        _ => Strength::Identical,
    };
    let mut names: Vec<_> = std::fs::read_dir(&orig)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| reference.join(n).exists())
        .collect();
    names.sort();
    let names: Vec<_> = names.into_iter().take(count).collect();
    let threads = fino_core::parallel::default_in_flight();
    let options = OptimizeOptions {
        strength,
        ..Default::default()
    };

    let started = Instant::now();
    let hint = fino_core::search::QualityHint::default();
    let outputs = fino_core::parallel::map_bounded(&names, threads, |_, n| {
        let data = std::fs::read(orig.join(n)).unwrap();
        let t = Instant::now();
        let opts = OptimizeOptions {
            quality_hint: hint.get(),
            ..options.clone()
        };
        let out = optimize(&data, &opts).unwrap();
        if let Outcome::Optimized(o) = &out {
            if !o.lossless {
                hint.record(o.quality);
            }
        }
        (n.clone(), data, out, t.elapsed().as_secs_f64())
    });
    let wall = started.elapsed().as_secs_f64();

    let (mut o_sum, mut f_sum, mut j_sum, mut fq, mut jq) = (0usize, 0usize, 0usize, 0f64, 0f64);
    for (n, data, out, secs) in &outputs {
        let ref_bytes = std::fs::read(reference.join(n)).unwrap();
        let src = codec::decode(data, false).unwrap();
        let (fino_bytes, label) = match out {
            Outcome::Optimized(o) => (
                o.bytes.clone(),
                format!(
                    "q{} {}",
                    o.quality,
                    if o.lossless { "lossless" } else { "" }
                ),
            ),
            Outcome::Skipped(r) => (data.clone(), format!("skip {r:?}")),
        };
        let fino_px = codec::decode(&fino_bytes, false).unwrap();
        let full = fino_core::metric::Reference::new(&src)
            .unwrap()
            .compare(&fino_px)
            .unwrap();
        let t = strength.targets();
        let audit = if full.global >= t.global - 0.3 && full.worst >= t.worst_tile - 0.3 {
            "ok"
        } else {
            "BELOW TARGET"
        };
        let label = format!("{label} full-check worst {:.1} {audit}", full.worst);
        let f_s = ssimulacra2(&src, &fino_px);
        let j_s = ssimulacra2(&src, &codec::decode(&ref_bytes, false).unwrap());
        o_sum += data.len();
        f_sum += fino_bytes.len();
        j_sum += ref_bytes.len();
        fq += f_s;
        jq += j_s;
        println!("{:<14} fino {:>5.1}% ssim2 {:>5.1} ({label}, {secs:.1}s) | reference {:>5.1}% ssim2 {:>5.1}",
            n.to_string_lossy(), 100.0 * (1.0 - fino_bytes.len() as f64 / data.len() as f64), f_s,
            100.0 * (1.0 - ref_bytes.len() as f64 / data.len() as f64), j_s);
    }
    let k = outputs.len() as f64;
    println!(
        "\n{strength:?}: {} photos, {threads} files in parallel, wall {wall:.1}s ({:.2}s/photo)",
        outputs.len(),
        wall / k
    );
    println!(
        "Fino     saved {:.1}%  mean ssim2 {:.2}",
        100.0 * (1.0 - f_sum as f64 / o_sum as f64),
        fq / k
    );
    println!(
        "Ref      saved {:.1}%  mean ssim2 {:.2}",
        100.0 * (1.0 - j_sum as f64 / o_sum as f64),
        jq / k
    );
}
