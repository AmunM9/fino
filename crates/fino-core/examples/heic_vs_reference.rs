//! HEIC → JPEG, Fino against another converter's outputs of the same photos: size, true
//! SSIMULACRA 2 against the decoded HEIC, and whether HDR / auxiliary images survive.
//!
//!     cargo run --release -p fino-core --example heic_vs_reference -- <heic dir> <reference dir>
#[cfg(target_os = "macos")]
fn main() {
    use fino_core::heic::decode::{aux_present, decode};
    use fino_core::{codec, resize, Outcome};
    use std::path::PathBuf;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (heics, reference) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let ssim2 = |a: &codec::Pixels, b: &codec::Pixels| {
        if (a.width, a.height) != (b.width, b.height) {
            return f64::NAN;
        }
        let (ra, rb) = (a.to_rgb(), b.to_rgb());
        let ia = imgref::ImgRef::new(&ra[..], a.width as usize, a.height as usize);
        let ib = imgref::ImgRef::new(&rb[..], b.width as usize, b.height as usize);
        fast_ssim2::compute_ssimulacra2(ia, ib).unwrap_or(f64::NAN)
    };
    let short = |aux: Vec<String>| {
        aux.iter()
            .map(|k| {
                k.trim_start_matches("kCGImageAuxiliaryDataType")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("+")
    };
    let mut entries: Vec<_> = std::fs::read_dir(&heics)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    let (mut sum_h, mut sum_f, mut sum_r) = (0u64, 0u64, 0u64);
    for path in entries
        .into_iter()
        .filter(|p| fino_core::files::has_heic_extension(p))
    {
        let data = std::fs::read(&path).unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let info = fino_core::heic::inspect(&data).unwrap();
        let src = decode(&data, false, u64::MAX).unwrap().unwrap();
        let upright = resize::orient(&src.pixels, src.orientation);
        let t = std::time::Instant::now();
        let fino = match fino_core::optimize(
            &data,
            &fino_core::OptimizeOptions {
                convert_heic: true,
                ..Default::default()
            },
        )
        .unwrap()
        {
            Outcome::Optimized(o) => o.bytes,
            Outcome::Skipped(r) => {
                println!("{stem}: Fino skipped ({r:?})");
                continue;
            }
        };
        let secs = t.elapsed().as_secs_f64();
        let f_px = codec::decode(&fino, false).unwrap();
        let f_score = ssim2(&src.pixels, &f_px);
        let refd = ["jpeg", "jpg", "JPG", "JPEG"]
            .iter()
            .map(|e| reference.join(format!("{stem}.{e}")))
            .find(|p| p.exists());
        let (r_len, r_score, r_aux) = match refd {
            Some(p) => {
                let bytes = std::fs::read(p).unwrap();
                let px = codec::decode(&bytes, false).unwrap();
                let s = ssim2(&src.pixels, &px).max(ssim2(&upright, &px));
                (bytes.len(), s, short(aux_present(&bytes)))
            }
            None => (0, f64::NAN, String::new()),
        };
        sum_h += data.len() as u64;
        sum_f += fino.len() as u64;
        sum_r += r_len as u64;
        println!(
            "{stem:<10} {}x{} or{} aux[{}] | HEIC {:>5} KB | Fino {:>5} KB ssim2 {:>4.1} aux[{}] {secs:.1}s | Ref {:>5} KB ssim2 {:>4.1} aux[{}]",
            src.pixels.width, src.pixels.height, src.orientation,
            info.aux_types.iter().map(|a| a.rsplit(':').next().unwrap_or(a)).collect::<Vec<_>>().join("+"),
            data.len() / 1000, fino.len() / 1000, f_score, short(aux_present(&fino)),
            r_len / 1000, r_score, r_aux
        );
    }
    println!(
        "\ntotal HEIC {} MB | Fino {} MB | Ref {} MB",
        sum_h / 1_000_000,
        sum_f / 1_000_000,
        sum_r / 1_000_000
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}
