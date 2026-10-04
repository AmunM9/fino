//! What the History and Settings screens show: stored summaries checked against the disk,
//! so actions whose files moved or vanished are not offered.

use crate::model::{FileResult, FileStatus, Overview, SessionView};
use crate::store::Store;
use std::path::Path;

/// Both sides of the before/after view are still where Fino left them. Exported copies can
/// be moved or deleted, and a replaced original lives in a backup that expires or is freed.
pub fn is_comparable(result: &FileResult) -> bool {
    result.status == FileStatus::Done
        && result.original_path.as_deref().is_some_and(Path::is_file)
        && result.outputs.first().is_some_and(|o| o.path.is_file())
}

/// Results as they can be viewed now: a vanished original or primary output is dropped so
/// the viewer only offers pairs it can actually load.
pub fn with_available_files(results: Vec<FileResult>) -> Vec<FileResult> {
    results
        .into_iter()
        .map(|r| {
            let output_present = r.outputs.first().is_some_and(|o| o.path.is_file());
            FileResult {
                original_path: r.original_path.filter(|p| p.is_file()),
                outputs: if output_present { r.outputs } else { vec![] },
                ..r
            }
        })
        .collect()
}

fn session_comparable(store: &Store, id: &str) -> bool {
    store
        .record(id)
        .is_ok_and(|record| record.results.iter().any(is_comparable))
}

pub fn build(store: &Store) -> Overview {
    let history = store.history();
    let sizes = store.backup_sizes();
    let sessions = history
        .sessions
        .into_iter()
        .map(|summary| SessionView {
            comparable: !summary.undone && session_comparable(store, &summary.id),
            backup_bytes: sizes.get(&summary.id).copied().unwrap_or(0),
            summary,
        })
        .collect();
    Overview {
        totals: history.totals,
        sessions,
        backup_bytes: sizes.values().sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OutputFile, OutputMode, SessionRecord, SessionSummary};
    use fino_core::Strength;
    use std::fs;
    use std::path::PathBuf;

    fn result(original: Option<PathBuf>, output: PathBuf) -> FileResult {
        FileResult {
            id: 0,
            path: output.clone(),
            name: "a.jpg".into(),
            status: FileStatus::Done,
            original_bytes: 10,
            output_bytes: 4,
            outputs: vec![OutputFile {
                path: output,
                bytes: 4,
                width: 1,
                height: 1,
                size_label: "Original".into(),
            }],
            original_path: original,
            width: 1,
            height: 1,
            score: None,
            quality: None,
            skip_reason: None,
            error: None,
            millis: 0,
            preview_path: None,
            lossless: false,
        }
    }

    fn summary(id: &str, undone: bool) -> SessionSummary {
        SessionSummary {
            id: id.into(),
            started_at: 1,
            finished_at: 2,
            photos: 1,
            optimized: 1,
            skipped: 0,
            failed: 0,
            original_bytes: 10,
            output_bytes: 4,
            saved_bytes: 6,
            output_mode: OutputMode::Export,
            strength: Strength::Identical,
            can_undo: false,
            undone,
        }
    }

    #[test]
    fn comparing_needs_both_files_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (original, output) = (dir.path().join("a.jpg"), dir.path().join("Fino-a.jpg"));
        fs::write(&original, b"o").unwrap();
        fs::write(&output, b"f").unwrap();
        let r = result(Some(original.clone()), output.clone());
        assert!(is_comparable(&r));

        fs::rename(&output, dir.path().join("moved.jpg")).unwrap();
        assert!(!is_comparable(&r), "output moved away");
        let shown = with_available_files(vec![r.clone()]);
        assert!(shown[0].outputs.is_empty() && shown[0].original_path.is_some());

        fs::write(&output, b"f").unwrap();
        fs::remove_file(&original).unwrap();
        assert!(!is_comparable(&r), "original gone");
        assert_eq!(with_available_files(vec![r])[0].original_path, None);
        assert!(!is_comparable(&result(None, output)));
    }

    #[test]
    fn overview_reports_comparability_and_backup_space() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("data"));
        let (original, output) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
        fs::write(&original, b"o").unwrap();
        fs::write(&output, b"f").unwrap();
        for (id, undone, files) in [
            ("1", false, Some(original.clone())),
            ("2", false, None),
            ("3", true, Some(original.clone())),
        ] {
            store
                .add_session(&SessionRecord {
                    summary: summary(id, undone),
                    results: vec![result(files, output.clone())],
                    backups: vec![],
                })
                .unwrap();
        }
        let backup = store.backup_dir("1").unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("00000-a.jpg"), b"12345").unwrap();

        let overview = build(&store);
        let by_id = |id: &str| {
            overview
                .sessions
                .iter()
                .find(|s| s.summary.id == id)
                .unwrap()
        };
        assert!(by_id("1").comparable);
        assert!(!by_id("2").comparable, "original missing");
        assert!(
            !by_id("3").comparable,
            "undone sessions have nothing to compare"
        );
        assert_eq!(by_id("1").backup_bytes, 5);
        assert_eq!(by_id("2").backup_bytes, 0);
        assert_eq!(overview.backup_bytes, 5);

        let wire = serde_json::to_value(by_id("1")).unwrap();
        assert_eq!(wire["id"], "1", "summary fields are flattened");
        assert_eq!(wire["backupBytes"], 5);
        assert_eq!(wire["comparable"], true);
    }
}
