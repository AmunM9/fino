//! Lossless transcode savings + speed over a folder.
use std::time::Instant;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let (mut before, mut after, mut ms) = (0usize, 0usize, 0f64);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| fino_core::files::has_jpeg_extension(p))
        .collect();
    files.sort();
    for p in files.iter().take(12) {
        let d = std::fs::read(p).unwrap();
        let s = Instant::now();
        let out = fino_core::lossless::transcode(&d).unwrap();
        ms += s.elapsed().as_secs_f64() * 1000.0;
        before += d.len();
        after += out.len();
        println!(
            "{:<14} {:>6.2}%",
            p.file_name().unwrap().to_string_lossy(),
            100.0 * (1.0 - out.len() as f64 / d.len() as f64)
        );
    }
    println!(
        "total {:.2}% (pixel data only, before metadata), {:.0} ms/file",
        100.0 * (1.0 - after as f64 / before as f64),
        ms / files.len().min(12) as f64
    );
}
