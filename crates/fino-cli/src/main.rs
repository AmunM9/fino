//! `fino` — perceptually lossless JPEG recompression (and HEIC → JPEG) from the terminal.
//!
//!     fino ~/Pictures/Trip                 # optimized copies in ~/Pictures/Trip/Fino
//!     fino --in-place --long-edge 2048 *.jpg
//!     fino ~/Downloads/IMG_0001.HEIC       # → ~/Downloads/Fino/IMG_0001.JPG

use clap::{Parser, ValueEnum};
use fino_core::files::{self, Job};
use fino_core::search::QualityHint;
use fino_core::{OptimizeOptions, Outcome, Resize, ResizeMode, Strength};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Copy, ValueEnum)]
enum Level {
    Pristine,
    Identical,
    Compact,
}

impl From<Level> for Strength {
    fn from(level: Level) -> Self {
        match level {
            Level::Pristine => Strength::Pristine,
            Level::Identical => Strength::Identical,
            Level::Compact => Strength::Compact,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "fino",
    version,
    about = "Smaller JPEGs that look exactly the same. HEIC photos become optimized JPEGs."
)]
struct Cli {
    /// Photos or folders (folders are searched recursively).
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// How far to go: pristine (flicker-proof), identical (default), compact.
    #[arg(short, long, value_enum, default_value_t = Level::Identical)]
    strength: Level,

    /// Overwrite the originals instead of writing copies.
    #[arg(long, conflicts_with = "out")]
    in_place: bool,

    /// Write copies here (default: a `Fino` folder next to each photo).
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Downscale so the longest side is at most N pixels.
    #[arg(long, value_name = "N", conflicts_with_all = ["max_width", "max_height"])]
    long_edge: Option<u32>,
    /// Downscale so the width is at most N pixels.
    #[arg(long, value_name = "N", conflicts_with = "max_height")]
    max_width: Option<u32>,
    /// Downscale so the height is at most N pixels.
    #[arg(long, value_name = "N")]
    max_height: Option<u32>,

    /// Remove GPS coordinates from EXIF and XMP.
    #[arg(long)]
    strip_location: bool,

    /// Re-process photos Fino has already optimized.
    #[arg(long)]
    force: bool,

    /// Leave HEIC photos alone instead of converting them to JPEG.
    #[arg(long)]
    no_heic: bool,

    /// Analyse and report, but write nothing.
    #[arg(long)]
    dry_run: bool,
}

impl Cli {
    fn resize(&self) -> Option<Resize> {
        let pick =
            |mode, px: Option<u32>| px.filter(|&p| p > 0).map(|pixels| Resize { mode, pixels });
        pick(ResizeMode::LongEdge, self.long_edge)
            .or_else(|| pick(ResizeMode::MaxWidth, self.max_width))
            .or_else(|| pick(ResizeMode::MaxHeight, self.max_height))
    }

    fn options(&self) -> OptimizeOptions {
        OptimizeOptions {
            strength: self.strength.into(),
            resize: self.resize(),
            strip_location: self.strip_location,
            skip_optimized: !self.force,
            convert_heic: !self.no_heic,
            ..Default::default()
        }
    }

    fn destination(&self, job: &Job, converted: bool) -> PathBuf {
        let source = if converted {
            files::converted_name(&job.path)
        } else {
            job.path.clone()
        };
        let name = source.file_name().unwrap_or_default();
        match (&self.out, &job.root) {
            (Some(out), Some(root)) => {
                let rel = job
                    .path
                    .parent()
                    .and_then(|p| p.strip_prefix(root).ok())
                    .unwrap_or(Path::new(""));
                let folder = root.file_name().map(PathBuf::from).unwrap_or_default();
                out.join(folder).join(rel).join(name)
            }
            (Some(out), None) => out.join(name),
            (None, _) => job.path.with_file_name("Fino").join(name),
        }
    }
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.2} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{:.0} KB", b as f64 / 1024.0),
    }
}

#[derive(Default)]
struct Totals {
    before: AtomicU64,
    after: AtomicU64,
    optimized: AtomicUsize,
    converted: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
}

fn process(
    cli: &Cli,
    options: &OptimizeOptions,
    hint: &QualityHint,
    job: &Job,
    totals: &Totals,
) -> Result<String, String> {
    let name = job.path.display();
    let data = std::fs::read(&job.path).map_err(|e| format!("{name}: {e}"))?;
    let options = OptimizeOptions {
        quality_hint: hint.get(),
        ..options.clone()
    };
    let outcome = fino_core::optimize(&data, &options).map_err(|e| format!("{name}: {e}"))?;
    if let Outcome::Optimized(o) = &outcome {
        if !o.lossless && options.resize.is_none() {
            hint.record(o.quality);
        }
    }
    match &outcome {
        Outcome::Optimized(o) if o.converted => {} // conversions stay out of the savings
        _ => {
            totals
                .before
                .fetch_add(data.len() as u64, Ordering::Relaxed);
        }
    }
    match outcome {
        Outcome::Skipped(reason) => {
            totals.after.fetch_add(data.len() as u64, Ordering::Relaxed);
            totals.skipped.fetch_add(1, Ordering::Relaxed);
            Ok(format!("  skip  {name}  ({reason:?})"))
        }
        Outcome::Optimized(o) if o.converted => {
            let written = if cli.dry_run {
                files::converted_name(&job.path)
            } else if cli.in_place {
                files::convert_in_place(&job.path, &o.bytes, None)
                    .map_err(|e| format!("{name}: {e}"))?
            } else {
                files::export(&job.path, &cli.destination(job, true), &o.bytes)
                    .map_err(|e| format!("{name}: {e}"))?
            };
            totals.converted.fetch_add(1, Ordering::Relaxed);
            let change = 100.0 * (o.bytes.len() as f64 / data.len() as f64 - 1.0);
            Ok(format!(
                "  heic  {name} → {}  {} → {} ({change:+.0}%)  q{}",
                written.display(),
                human(data.len() as u64),
                human(o.bytes.len() as u64),
                o.quality
            ))
        }
        Outcome::Optimized(o) => {
            if !cli.dry_run {
                let written = if cli.in_place {
                    files::replace(&job.path, &o.bytes, None).map(|_| job.path.clone())
                } else {
                    files::export(&job.path, &cli.destination(job, false), &o.bytes)
                };
                written.map_err(|e| format!("{name}: {e}"))?;
            }
            totals
                .after
                .fetch_add(o.bytes.len() as u64, Ordering::Relaxed);
            totals.optimized.fetch_add(1, Ordering::Relaxed);
            let saved = 100.0 * (1.0 - o.bytes.len() as f64 / data.len() as f64);
            Ok(format!(
                "  {saved:>4.0}%  {name}  {} → {}  q{} · score {:.1}",
                human(data.len() as u64),
                human(o.bytes.len() as u64),
                o.quality,
                o.score.global
            ))
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = cli.options();
    let jobs = files::collect(&cli.paths);
    if jobs.is_empty() {
        eprintln!("fino: no photos found");
        return ExitCode::FAILURE;
    }
    let totals = Totals::default();
    let hint = QualityHint::default();
    let in_flight = fino_core::parallel::default_in_flight();
    fino_core::parallel::map_bounded(&jobs, in_flight, |_, job| {
        match process(&cli, &options, &hint, job, &totals) {
            Ok(line) => println!("{line}"),
            Err(err) => {
                totals.failed.fetch_add(1, Ordering::Relaxed);
                eprintln!("  fail  {err}");
            }
        }
    });

    let before = totals.before.load(Ordering::Relaxed);
    let after = totals.after.load(Ordering::Relaxed);
    let pct = if before > 0 {
        100.0 * (1.0 - after as f64 / before as f64)
    } else {
        0.0
    };
    println!(
        "\n{} optimized · {} converted from HEIC · {} skipped · {} failed — saved {} ({pct:.1}%){}",
        totals.optimized.load(Ordering::Relaxed),
        totals.converted.load(Ordering::Relaxed),
        totals.skipped.load(Ordering::Relaxed),
        totals.failed.load(Ordering::Relaxed),
        human(before.saturating_sub(after)),
        if cli.dry_run { " [dry run]" } else { "" }
    );
    if totals.failed.load(Ordering::Relaxed) > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
