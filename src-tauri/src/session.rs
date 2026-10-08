//! One optimization session: expand dropped paths, process photos in parallel,
//! write results (replace with backup, or export), report each file as it lands.

use crate::model::{
    now_millis, BackupEntry, FileResult, FileStatus, OutputFile, OutputMode, SessionEvent,
    SessionRecord, SessionSummary, Settings, SizePreset,
};
use fino_core::files::{self, Job};
use fino_core::search::QualityHint;
use fino_core::{OptimizeOptions, Outcome, SkipReason};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub const EXPORT_FOLDER: &str = "Fino";

pub struct Context<'a> {
    pub session_id: String,
    pub settings: Settings,
    pub backup_dir: PathBuf,
    pub cancel: &'a AtomicBool,
    /// Previous photo's answer, so the next search starts there.
    pub hint: QualityHint,
    /// Where UI thumbnails go (`None` = no previews).
    pub preview_dir: Option<PathBuf>,
}

const PREVIEW_LONG_EDGE: u32 = 720;

/// Appends a backup to `manifest.jsonl` the moment it exists, so even a crash mid-session
/// leaves a record of where every replaced original went.
fn journal(backup_dir: &Path, entry: &BackupEntry) {
    let line = match serde_json::to_string(entry) {
        Ok(l) => l,
        Err(_) => return,
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(backup_dir.join("manifest.jsonl"));
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{line}");
    }
}

/// A size preset's label as a folder name, valid on macOS and Windows alike.
fn safe_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    // Windows also refuses names ending in a dot or a space.
    let trimmed = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        "Size".into()
    } else {
        trimmed.to_string()
    }
}

/// Destination for an exported copy of `job` at `size` (`.jpg` when converting).
pub fn export_path(settings: &Settings, job: &Job, size: &SizePreset, converted: bool) -> PathBuf {
    let source = if converted {
        files::converted_name(&job.path)
    } else {
        job.path.clone()
    };
    let name = source.file_name().unwrap_or_default();
    let base = match (&settings.export_dir, &job.root) {
        (Some(dir), Some(root)) => {
            let rel = job
                .path
                .parent()
                .and_then(|p| p.strip_prefix(root).ok())
                .unwrap_or(Path::new(""));
            dir.join(root.file_name().unwrap_or_default()).join(rel)
        }
        (Some(dir), None) => dir.clone(),
        (None, _) => job
            .path
            .parent()
            .unwrap_or(Path::new("/"))
            .join(EXPORT_FOLDER),
    };
    let base = if settings.sizes.len() > 1 {
        base.join(safe_label(&size.label))
    } else {
        base
    };
    base.join(name)
}

struct Processed {
    result: FileResult,
    backup: Option<BackupEntry>,
}

fn failed(id: usize, job: &Job, original_bytes: u64, error: String, started: Instant) -> Processed {
    Processed {
        result: FileResult {
            id,
            path: job.path.clone(),
            name: file_name(&job.path),
            status: FileStatus::Failed,
            original_bytes,
            output_bytes: original_bytes,
            outputs: vec![],
            original_path: Some(job.path.clone()),
            width: 0,
            height: 0,
            score: None,
            quality: None,
            skip_reason: None,
            error: Some(error),
            millis: started.elapsed().as_millis() as u64,
            preview_path: None,
            lossless: false,
            converted: false,
        },
        backup: None,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn process(id: usize, job: &Job, ctx: &Context) -> Processed {
    let started = Instant::now();
    let data = match std::fs::read(&job.path) {
        Ok(d) => d,
        Err(e) => return failed(id, job, 0, e.to_string(), started),
    };
    // A decoder bug on one hostile file must not take the whole session (and its undo
    // record) down with it.
    match catch_unwind(AssertUnwindSafe(|| {
        process_bytes(id, job, &data, ctx, started)
    })) {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => failed(id, job, data.len() as u64, e, started),
        Err(_) => failed(
            id,
            job,
            data.len() as u64,
            "internal error while processing this photo".into(),
            started,
        ),
    }
}

fn process_bytes(
    id: usize,
    job: &Job,
    data: &[u8],
    ctx: &Context,
    started: Instant,
) -> Result<Processed, String> {
    let s = &ctx.settings;
    let options = OptimizeOptions {
        strength: s.strength,
        strip_location: s.strip_location,
        skip_optimized: s.skip_optimized,
        quality_hint: ctx.hint.get(),
        convert_heic: s.heic_to_jpeg,
        ..Default::default()
    };
    let original_bytes = data.len() as u64;
    let mut result = FileResult {
        id,
        path: job.path.clone(),
        name: file_name(&job.path),
        status: FileStatus::Skipped,
        original_bytes,
        output_bytes: original_bytes,
        outputs: vec![],
        original_path: Some(job.path.clone()),
        width: 0,
        height: 0,
        score: None,
        quality: None,
        skip_reason: None,
        error: None,
        millis: 0,
        preview_path: None,
        lossless: false,
        converted: false,
    };
    let prepared = match fino_core::prepare(data, &options).map_err(|e| e.to_string())? {
        Ok(p) => p,
        Err(reason) => {
            result.skip_reason = Some(reason);
            result.millis = started.elapsed().as_millis() as u64;
            return Ok(Processed {
                result,
                backup: None,
            });
        }
    };
    (result.width, result.height) = prepared.dimensions();
    let converting = prepared.is_conversion();
    result.converted = converting;
    result.preview_path = ctx.preview_dir.as_ref().and_then(|dir| {
        let bytes = prepared.preview(PREVIEW_LONG_EDGE).ok()?;
        std::fs::create_dir_all(dir).ok()?;
        let path = dir.join(format!("{id:05}.jpg"));
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    });

    let sizes = match s.output_mode {
        OutputMode::Replace => vec![SizePreset::original()],
        OutputMode::Export => s.sizes.clone(),
    };
    let mut backup = None;
    for (i, size) in sizes.iter().enumerate() {
        let outcome = prepared.render(size.resize()).map_err(|e| e.to_string())?;
        let (bytes, width, height): (&[u8], u32, u32) = match &outcome {
            Outcome::Optimized(o) => {
                result.status = FileStatus::Done;
                if i == 0 {
                    result.score = Some(o.score.global);
                    result.quality = Some(o.quality);
                    result.lossless = o.lossless;
                }
                if !o.lossless && size.resize().is_none() {
                    ctx.hint.record(o.quality);
                }
                (&o.bytes, o.width, o.height)
            }
            Outcome::Skipped(reason) => {
                result.skip_reason.get_or_insert(*reason);
                if s.output_mode == OutputMode::Replace {
                    continue;
                }
                // Exports stay complete: an un-improvable photo is copied as is.
                (data, result.width, result.height)
            }
        };
        let path = match s.output_mode {
            OutputMode::Replace if converting => {
                let backup_to = s
                    .keep_backups
                    .then(|| ctx.backup_dir.join(format!("{id:05}-{}", result.name)));
                let out = files::convert_in_place(&job.path, bytes, backup_to.as_deref())
                    .map_err(|e| e.to_string())?;
                result.original_path = backup_to.clone();
                backup = backup_to.map(|b| BackupEntry {
                    original: job.path.clone(),
                    backup: b,
                    written: files::fingerprint(&out).ok(),
                    output: Some(out.clone()),
                });
                if let Some(entry) = &backup {
                    journal(&ctx.backup_dir, entry);
                }
                out
            }
            OutputMode::Replace => {
                let backup_to = s
                    .keep_backups
                    .then(|| ctx.backup_dir.join(format!("{id:05}-{}", result.name)));
                files::replace(&job.path, bytes, backup_to.as_deref())
                    .map_err(|e| e.to_string())?;
                result.original_path = backup_to.clone();
                backup = backup_to.map(|b| BackupEntry {
                    original: job.path.clone(),
                    backup: b,
                    written: files::fingerprint(&job.path).ok(),
                    output: None,
                });
                if let Some(entry) = &backup {
                    journal(&ctx.backup_dir, entry);
                }
                job.path.clone()
            }
            OutputMode::Export => {
                let dest = export_path(
                    s,
                    job,
                    size,
                    converting && result.status == FileStatus::Done,
                );
                let written = files::export(&job.path, &dest, bytes).map_err(|e| e.to_string())?;
                if converting && size.resize().is_none() {
                    export_live_video(&job.path, &written);
                }
                written
            }
        };
        if i == 0 {
            result.output_bytes = bytes.len() as u64;
        }
        result.outputs.push(OutputFile {
            path,
            bytes: bytes.len() as u64,
            width,
            height,
            size_label: size.label.clone(),
        });
    }
    if result.status == FileStatus::Done {
        result.skip_reason = None;
    }
    result.millis = started.elapsed().as_millis() as u64;
    Ok(Processed { result, backup })
}

/// A converted Live Photo keeps its pair only next to a `.mov` of the same name: exporting
/// the photo alone would leave the video behind.
fn export_live_video(source: &Path, written: &Path) {
    let Some(video) = files::live_photo_video(source) else {
        return;
    };
    let extension = video.extension().unwrap_or_default();
    let dest = written.with_extension(extension);
    if !dest.exists() {
        if let Err(e) = files::copy_file(&video, &dest) {
            eprintln!(
                "fino: could not copy Live Photo video {}: {e}",
                video.display()
            );
        }
    }
}

pub fn summarize(
    id: &str,
    started_at: u64,
    mode: OutputMode,
    settings: &Settings,
    results: &[FileResult],
    can_undo: bool,
) -> SessionSummary {
    let count = |status| results.iter().filter(|r| r.status == status).count();
    // Conversions change format rather than shrink a file: they stay out of the savings.
    let optimized = || results.iter().filter(|r| !r.converted);
    let original_bytes: u64 = optimized().map(|r| r.original_bytes).sum();
    let output_bytes: u64 = optimized().map(|r| r.output_bytes).sum();
    SessionSummary {
        id: id.to_string(),
        started_at,
        finished_at: now_millis(),
        photos: results.len(),
        optimized: count(FileStatus::Done),
        skipped: count(FileStatus::Skipped),
        failed: count(FileStatus::Failed),
        original_bytes,
        output_bytes,
        saved_bytes: original_bytes.saturating_sub(output_bytes),
        output_mode: mode,
        strength: settings.strength,
        can_undo,
        undone: false,
    }
}

/// Runs a whole session, calling `emit` as each file finishes.
pub fn run(
    ctx: &Context,
    paths: &[PathBuf],
    emit: &(dyn Fn(SessionEvent) + Sync),
) -> SessionRecord {
    let started_at = now_millis();
    let jobs = files::collect(paths);
    emit(SessionEvent::Started {
        session_id: ctx.session_id.clone(),
        total: jobs.len(),
    });

    // A few photos at a time; each one's metric already spreads across every core.
    let in_flight = fino_core::parallel::default_in_flight();
    let processed: Vec<Processed> =
        fino_core::parallel::map_bounded(&jobs, in_flight, |id, job| {
            if ctx.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let processed = process(id, job, ctx);
            emit(SessionEvent::File {
                result: processed.result.clone(),
            });
            Some(processed)
        })
        .into_iter()
        .flatten()
        .collect();

    let backups: Vec<BackupEntry> = processed.iter().filter_map(|p| p.backup.clone()).collect();
    let results: Vec<FileResult> = processed.into_iter().map(|p| p.result).collect();
    let summary = summarize(
        &ctx.session_id,
        started_at,
        ctx.settings.output_mode,
        &ctx.settings,
        &results,
        !backups.is_empty(),
    );
    emit(SessionEvent::Finished {
        summary: summary.clone(),
    });
    SessionRecord {
        summary,
        results,
        backups,
    }
}

/// Reason a skipped file was left alone, used by the CSV log.
pub fn skip_label(reason: Option<SkipReason>) -> &'static str {
    match reason {
        None => "",
        Some(SkipReason::AlreadyOptimized) => "already optimized",
        Some(SkipReason::NoGain) => "no meaningful gain",
        Some(SkipReason::Unsupported) => "not a supported photo",
        Some(SkipReason::Cmyk) => "CMYK JPEG",
        Some(SkipReason::ExoticJpeg) => "unsupported JPEG variant",
        Some(SkipReason::TooLarge) => "too large",
        Some(SkipReason::HdrGainMap) => "HDR gain map",
        Some(SkipReason::EmbeddedMedia) => "embedded video or media",
        Some(SkipReason::HdrPhoto) => "HDR photo (PQ/HLG) a JPEG cannot hold",
        Some(SkipReason::SpatialPhoto) => "spatial (stereo) photo",
        Some(SkipReason::ConversionOff) => "HEIC conversion is off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(path: &str, root: Option<&str>) -> Job {
        Job {
            path: PathBuf::from(path),
            root: root.map(PathBuf::from),
        }
    }

    #[test]
    fn default_export_goes_to_fino_folder_next_to_photo() {
        let s = Settings::default();
        let p = export_path(
            &s,
            &job("/p/trip/a.jpg", None),
            &SizePreset::original(),
            false,
        );
        assert_eq!(p, PathBuf::from("/p/trip/Fino/a.jpg"));
    }

    #[test]
    fn export_dir_mirrors_dropped_folder_and_splits_sizes() {
        let s = Settings {
            export_dir: Some("/out".into()),
            sizes: vec![
                SizePreset::original(),
                SizePreset {
                    id: "w".into(),
                    label: "2048 px".into(),
                    mode: None,
                    pixels: None,
                },
            ],
            ..Settings::default()
        };
        let j = job("/p/trip/day1/a.jpg", Some("/p/trip"));
        assert_eq!(
            export_path(&s, &j, &s.sizes[1], false),
            PathBuf::from("/out/trip/day1/2048 px/a.jpg")
        );
    }

    fn write_camera_jpeg(path: &Path) {
        let (w, h) = (256u32, 192u32);
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7 + y * 13) ^ (x * y)) % 23;
                data.extend_from_slice(&[(x + n) as u8, (y + n) as u8, ((x + y) / 2) as u8]);
            }
        }
        let px = fino_core::codec::Pixels {
            data,
            width: w,
            height: h,
            channels: 3,
        };
        let jpeg = fino_core::codec::encode(
            &px,
            fino_core::codec::EncodeParams::new(98.0, (2, 2), fino_core::codec::Effort::Probe),
        )
        .unwrap();
        std::fs::write(path, jpeg).unwrap();
    }

    fn run_in(dir: &Path, settings: Settings) -> SessionRecord {
        let cancel = AtomicBool::new(false);
        let ctx = Context {
            session_id: "t1".into(),
            settings,
            backup_dir: dir.join("backups"),
            cancel: &cancel,
            hint: QualityHint::default(),
            preview_dir: Some(dir.join("previews")),
        };
        let events = std::sync::Mutex::new(Vec::new());
        let record = run(&ctx, &[dir.join("photos")], &|e| {
            events.lock().unwrap().push(e)
        });
        let events = events.into_inner().unwrap();
        assert!(matches!(
            events.first(),
            Some(SessionEvent::Started { total: 2, .. })
        ));
        assert!(matches!(events.last(), Some(SessionEvent::Finished { .. })));
        record
    }

    #[test]
    fn replace_session_shrinks_photos_and_keeps_restorable_backups() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("photos")).unwrap();
        let photo = dir.path().join("photos/a.jpg");
        write_camera_jpeg(&photo);
        std::fs::write(dir.path().join("photos/b.jpg"), b"not really a jpeg").unwrap();
        let before = std::fs::read(&photo).unwrap();

        let record = run_in(dir.path(), Settings::default());

        assert_eq!((record.summary.optimized, record.summary.skipped), (1, 1));
        assert!(std::fs::read(&photo).unwrap().len() < before.len());
        assert_eq!(record.backups.len(), 1);
        assert_eq!(std::fs::read(&record.backups[0].backup).unwrap(), before);
        assert_eq!(record.results[1].skip_reason, Some(SkipReason::Unsupported));
        let preview = record.results[0]
            .preview_path
            .as_ref()
            .expect("preview written");
        assert!(
            std::fs::metadata(preview).unwrap().len() < std::fs::metadata(&photo).unwrap().len()
        );
    }

    #[test]
    fn export_session_leaves_originals_untouched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("photos")).unwrap();
        let photo = dir.path().join("photos/a.jpg");
        write_camera_jpeg(&photo);
        std::fs::copy(&photo, dir.path().join("photos/b.jpg")).unwrap();
        let before = std::fs::read(&photo).unwrap();

        let settings = Settings {
            output_mode: OutputMode::Export,
            ..Settings::default()
        };
        let record = run_in(dir.path(), settings);

        assert_eq!(std::fs::read(&photo).unwrap(), before);
        assert!(dir.path().join("photos/Fino/a.jpg").exists());
        assert!(record.backups.is_empty() && !record.summary.can_undo);
    }

    #[cfg(target_os = "macos")]
    fn heic_fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/fino-core/tests/fixtures/rotated.heic"
        );
        std::fs::read(path).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn by_default_heic_photos_are_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        write_camera_jpeg(&photos.join("a.jpg"));
        let heic = photos.join("IMG_0003.HEIC");
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/fino-core/tests/fixtures/rotated.heic"
        ))
        .unwrap();
        std::fs::write(&heic, &bytes).unwrap();

        let record = run_in(dir.path(), Settings::default());

        assert_eq!(std::fs::read(&heic).unwrap(), bytes, "HEIC untouched");
        assert!(!photos.join("IMG_0003.JPG").exists());
        let skipped = record
            .results
            .iter()
            .find(|r| r.name == "IMG_0003.HEIC")
            .unwrap();
        assert_eq!(skipped.skip_reason, Some(SkipReason::ConversionOff));
    }

    /// Without a system HEIC decoder (Windows), folders don't pick HEIC photos up at all.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn without_a_heic_decoder_folders_skip_heic_photos() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        write_camera_jpeg(&photos.join("a.jpg"));
        write_camera_jpeg(&photos.join("b.jpg"));
        let heic = photos.join("IMG_0003.HEIC");
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/fino-core/tests/fixtures/rotated.heic"
        ))
        .unwrap();
        std::fs::write(&heic, &bytes).unwrap();

        let record = run_in(dir.path(), Settings::default());

        assert_eq!(std::fs::read(&heic).unwrap(), bytes, "HEIC untouched");
        assert!(!photos.join("IMG_0003.JPG").exists());
        assert!(record.results.iter().all(|r| r.name != "IMG_0003.HEIC"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replace_converts_heic_beside_it_and_keeps_it_for_undo() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        write_camera_jpeg(&photos.join("a.jpg"));
        let heic = photos.join("IMG_0001.HEIC");
        std::fs::write(&heic, heic_fixture()).unwrap();

        let settings = Settings {
            heic_to_jpeg: true,
            ..Settings::default()
        };
        let record = run_in(dir.path(), settings);

        let jpeg = photos.join("IMG_0001.JPG");
        assert!(jpeg.exists() && !heic.exists(), "HEIC replaced by a JPEG");
        let converted = record
            .results
            .iter()
            .find(|r| r.converted)
            .expect("converted");
        assert_eq!(converted.outputs[0].path, jpeg);
        let entry = record.backups.iter().find(|b| b.output.is_some()).unwrap();
        assert_eq!(std::fs::read(&entry.backup).unwrap(), heic_fixture());
        let jpeg_only = record.results.iter().find(|r| !r.converted).unwrap();
        assert_eq!(
            record.summary.saved_bytes,
            jpeg_only.original_bytes - jpeg_only.output_bytes,
            "conversions stay out of the savings"
        );

        files::restore_converted(&entry.backup, &heic, &jpeg, entry.written).unwrap();
        assert!(heic.exists() && !jpeg.exists(), "undo puts the HEIC back");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn export_converts_heic_and_keeps_live_photo_pairs_together() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        write_camera_jpeg(&photos.join("a.jpg"));
        std::fs::write(photos.join("IMG_0002.HEIC"), heic_fixture()).unwrap();
        std::fs::write(photos.join("IMG_0002.MOV"), b"live video").unwrap();
        let settings = Settings {
            output_mode: OutputMode::Export,
            heic_to_jpeg: true,
            ..Settings::default()
        };

        run_in(dir.path(), settings);

        assert!(photos.join("IMG_0002.HEIC").exists(), "original untouched");
        assert!(photos.join("Fino/IMG_0002.JPG").exists());
        assert_eq!(
            std::fs::read(photos.join("Fino/IMG_0002.MOV")).unwrap(),
            b"live video"
        );
    }

    #[test]
    fn size_labels_cannot_escape_the_folder() {
        assert_eq!(safe_label("../x/y"), "-x-y");
        assert_eq!(safe_label("  "), "Size");
        assert_eq!(safe_label(r#"2048 px: "web"?"#), "2048 px- -web--");
        assert_eq!(safe_label(r"a\b|c."), "a-b-c");
    }
}
