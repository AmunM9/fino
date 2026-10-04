use super::*;
use crate::model::{OutputFile, Totals};
use fino_core::Strength;

fn summary(id: &str, saved: u64, started_at: u64) -> SessionSummary {
    SessionSummary {
        id: id.into(),
        started_at,
        finished_at: started_at + 10,
        photos: 2,
        optimized: 2,
        skipped: 0,
        failed: 0,
        original_bytes: saved * 2,
        output_bytes: saved,
        saved_bytes: saved,
        output_mode: OutputMode::Replace,
        strength: Strength::Identical,
        can_undo: true,
        undone: false,
    }
}

fn record(id: &str, saved: u64, started_at: u64) -> SessionRecord {
    SessionRecord {
        summary: summary(id, saved, started_at),
        results: vec![],
        backups: vec![],
    }
}

fn file_result(path: &str) -> FileResult {
    FileResult {
        id: 0,
        path: PathBuf::from(path),
        name: "a.jpg".into(),
        status: FileStatus::Skipped,
        original_bytes: 10,
        output_bytes: 10,
        outputs: vec![OutputFile {
            path: PathBuf::from("/out/a.jpg"),
            bytes: 4,
            width: 3,
            height: 2,
            size_label: "Original".into(),
        }],
        original_path: Some(PathBuf::from(path)),
        width: 3,
        height: 2,
        score: Some(86.5),
        quality: Some(80),
        skip_reason: Some(fino_core::SkipReason::NoGain),
        error: None,
        millis: 12,
        preview_path: None,
        lossless: true,
        converted: false,
    }
}

fn open(dir: &Path) -> Store {
    Store::open(dir.to_path_buf()).unwrap()
}

#[test]
fn totals_are_derived_from_sessions_and_follow_updates() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let a = record("a1", 100, 1_000);
    store.save_session(&a).unwrap();
    store.save_session(&record("b2", 50, 2_000)).unwrap();
    assert_eq!(
        store.totals().unwrap(),
        Totals {
            saved_bytes: 150,
            original_bytes: 300,
            photos: 4,
            sessions: 2,
            since: Some(1_000)
        }
    );
    assert_eq!(store.sessions(10).unwrap()[0].id, "b2", "newest first");

    let undone = SessionRecord {
        summary: SessionSummary {
            saved_bytes: 0,
            optimized: 0,
            can_undo: false,
            undone: true,
            ..a.summary.clone()
        },
        ..a
    };
    store.save_session(&undone).unwrap();
    let totals = store.totals().unwrap();
    assert_eq!(
        (totals.saved_bytes, totals.photos, totals.sessions),
        (50, 2, 2)
    );
    assert!(store.record("a1").unwrap().summary.undone);
}

#[test]
fn records_round_trip_with_results_and_backups() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let full = SessionRecord {
        results: vec![file_result("/photos/a.jpg")],
        backups: vec![BackupEntry {
            original: PathBuf::from("/photos/a.heic"),
            backup: PathBuf::from("/data/backups/1/00000-a.heic"),
            written: None,
            output: Some(PathBuf::from("/photos/a.jpg")),
        }],
        ..record("1", 6, 1)
    };
    store.save_session(&full).unwrap();
    assert_eq!(store.record("1").unwrap(), full);
    assert_eq!(store.undoable_sessions().unwrap(), vec!["1".to_string()]);
}

#[test]
fn history_is_not_capped_and_pages_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    for i in 0..300u64 {
        store.save_session(&record(&i.to_string(), 1, i)).unwrap();
    }
    assert_eq!(store.totals().unwrap().sessions, 300);
    let page = store.sessions(50).unwrap();
    assert_eq!(page.len(), 50);
    assert_eq!(page[0].id, "299");
    assert_eq!(store.sessions(1_000).unwrap().len(), 300);
}

#[test]
fn imports_legacy_json_history_once_and_retires_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let old = SessionRecord {
        results: vec![file_result("/photos/a.jpg")],
        ..record("1700000000000", 40, 1_700_000_000_000)
    };
    fs::write(
        sessions.join("1700000000000.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    fs::write(sessions.join("broken.json"), b"{ not json").unwrap();
    fs::write(dir.path().join("history.json"), b"{}").unwrap();

    let store = open(dir.path());
    assert_eq!(store.record("1700000000000").unwrap(), old);
    assert!(!sessions.exists() && dir.path().join("sessions.migrated").is_dir());
    assert!(dir.path().join("history.json.migrated").is_file());
    drop(store);

    let reopened = open(dir.path());
    assert_eq!(reopened.totals().unwrap().sessions, 1, "imported only once");
}

#[test]
fn recovers_a_session_interrupted_before_its_record_was_written() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir.path().join("data"));
    let photo = dir.path().join("p.jpg");
    fs::write(&photo, b"optimized").unwrap();
    let backup_dir = store.backup_dir("1700000000000").unwrap();
    fs::create_dir_all(&backup_dir).unwrap();
    let backup = backup_dir.join("00000-p.jpg");
    fs::write(&backup, b"the original, bigger").unwrap();
    let entry = BackupEntry {
        original: photo.clone(),
        backup: backup.clone(),
        written: None,
        output: None,
    };
    fs::write(
        backup_dir.join("manifest.jsonl"),
        serde_json::to_string(&entry).unwrap() + "\n",
    )
    .unwrap();

    assert_eq!(store.recover_interrupted().unwrap(), 1);
    assert_eq!(store.recover_interrupted().unwrap(), 0, "only once");
    let record = store.record("1700000000000").unwrap();
    assert!(record.summary.can_undo);
    assert_eq!(record.backups, vec![entry]);
    assert_eq!(record.summary.saved_bytes, 20 - 9);
}

#[test]
fn rejects_path_traversal_in_session_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert!(store.backup_dir("../../etc").is_err());
    assert!(store.record("a/b").is_err());
    assert!(store.record("x' OR '1'='1").is_err());
}
