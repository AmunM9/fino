//! Compares originals against another optimizer's and Fino's outputs of the same photos.
//!
//!     cargo run --release -p fino-core --example compare_sets -- <originals> <reference> <fino>

use fino_core::{codec, jpeg};
use std::path::{Path, PathBuf};

fn ssimulacra2(a: &codec::Pixels, b: &codec::Pixels) -> f64 {
    let (ra, rb) = (a.to_rgb(), b.to_rgb());
    let ia = imgref::ImgRef::new(&ra[..], a.width as usize, a.height as usize);
    let ib = imgref::ImgRef::new(&rb[..], b.width as usize, b.height as usize);
    fast_ssim2::compute_ssimulacra2(ia, ib).unwrap_or(f64::NAN)
}

fn describe(data: &[u8]) -> String {
    match jpeg::inspect(data) {
        Ok(h) => format!(
            "{:?} q{:?} {:?}",
            h.kind,
            h.quality_estimate.unwrap_or(0),
            h.chroma_block()
        ),
        Err(e) => format!("? {e}"),
    }
}

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let (orig, reference, fino) = (&args[0], &args[1], &args[2]);
    let mut names: Vec<_> = std::fs::read_dir(fino)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    names.sort();
    for name in names {
        let read = |d: &Path| std::fs::read(d.join(&name)).ok();
        let (Some(o), Some(j), Some(f)) = (read(orig), read(reference), read(fino)) else {
            continue;
        };
        let po = codec::decode(&o, false).unwrap();
        let pj = codec::decode(&j, false).unwrap();
        let pf = codec::decode(&f, false).unwrap();
        println!(
            "{:<14} {}x{}  orig {:>5.1}MB [{}]\n               Ref  {:>5.1}MB ({:>3.0}%) ssim2 {:.1} [{}]\n               Fino {:>5.1}MB ({:>3.0}%) ssim2 {:.1} [{}]",
            name.to_string_lossy(), po.width, po.height,
            o.len() as f64 / 1e6, describe(&o),
            j.len() as f64 / 1e6, 100.0 * (1.0 - j.len() as f64 / o.len() as f64), ssimulacra2(&po, &pj), describe(&j),
            f.len() as f64 / 1e6, 100.0 * (1.0 - f.len() as f64 / o.len() as f64), ssimulacra2(&po, &pf), describe(&f),
        );
    }
}
