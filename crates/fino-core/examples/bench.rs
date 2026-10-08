//! Batch throughput like the app runs it: wall time, CPU time and total saving, no audit.
//!
//!     cargo run --release -p fino-core --example bench -- <folder> [count] [strength]
use fino_core::{prepare, OptimizeOptions, Outcome, Strength};
use std::path::PathBuf;
use std::time::Instant;

#[cfg(unix)]
fn cpu_seconds() -> f64 {
    // SAFETY: getrusage writes a plain struct we own.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    t(usage.ru_utime) + t(usage.ru_stime)
}

#[cfg(unix)]
fn peak_rss_mb() -> f64 {
    // SAFETY: getrusage writes a plain struct we own. macOS reports ru_maxrss in bytes.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    usage.ru_maxrss as f64 / 1e6
}

// Process CPU time and peak memory are only measured on macOS / Linux.
#[cfg(not(unix))]
fn cpu_seconds() -> f64 {
    f64::NAN
}

#[cfg(not(unix))]
fn peak_rss_mb() -> f64 {
    f64::NAN
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&args[0]);
    let count: usize = args
        .get(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(usize::MAX);
    let strength = match args.get(2).map(String::as_str) {
        Some("pristine") => Strength::Pristine,
        Some("compact") => Strength::Compact,
        _ => Strength::Identical,
    };
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
        })
        .collect();
    paths.sort();
    paths.truncate(count);
    let in_flight = std::env::var("FINO_IN_FLIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(fino_core::parallel::default_in_flight);
    let hint = fino_core::search::QualityHint::default();
    let with_preview = std::env::var("FINO_NO_PREVIEW").is_err();

    let (cpu0, started) = (cpu_seconds(), Instant::now());
    let outputs = fino_core::parallel::map_bounded(&paths, in_flight, |_, p| {
        let data = std::fs::read(p).unwrap();
        let options = OptimizeOptions {
            strength,
            quality_hint: hint.get(),
            ..Default::default()
        };
        // Like the app: decode once, a small preview for the UI, then the output.
        let out = match prepare(&data, &options).unwrap() {
            Ok(prepared) => {
                if with_preview {
                    prepared.preview(720).unwrap();
                }
                prepared.render(None).unwrap()
            }
            Err(reason) => Outcome::Skipped(reason),
        };
        let size = match &out {
            Outcome::Optimized(o) => {
                if !o.lossless {
                    hint.record(o.quality);
                }
                o.bytes.len()
            }
            Outcome::Skipped(_) => data.len(),
        };
        (data.len(), size)
    });
    let (wall, cpu) = (started.elapsed().as_secs_f64(), cpu_seconds() - cpu0);
    let original: usize = outputs.iter().map(|o| o.0).sum();
    let output: usize = outputs.iter().map(|o| o.1).sum();
    println!(
        "{strength:?}: {} photos, {in_flight} in flight — wall {wall:.1}s ({:.2}s/photo), cpu {cpu:.0}s, saved {:.1}%",
        outputs.len(),
        wall / outputs.len() as f64,
        100.0 * (1.0 - output as f64 / original as f64)
    );
    println!("peak memory {:.0} MB", peak_rss_mb());
}
