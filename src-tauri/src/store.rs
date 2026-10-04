//! JSON persistence under the app's data folder:
//!
//! ```text
//! settings.json
//! history.json            totals + recent session summaries
//! sessions/<id>.json      per-file results and backup manifest
//! backups/<id>/…          replaced originals, kept for undo
//! ```

use crate::model::{
    BackupEntry, FileResult, FileStatus, History, OutputMode, SessionRecord, SessionSummary,
    Settings,
};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_SESSIONS: usize = 250;

pub struct Store {
    root: PathBuf,
    lock: Mutex<()>,
}

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fino_core::files::write_atomic(path, &bytes).map_err(|e| e.to_string())
}

/// Session ids become folder names; accept only what we generate.
pub(crate) fn valid_id(id: &str) -> Result<&str, String> {
    let ok = !id.is_empty()
        && id.len() <= 32
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some(id)
        .ok_or_else(|| "invalid session id".to_string())
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: Mutex::new(()),
        }
    }

    pub fn settings(&self) -> Settings {
        read_json::<Settings>(&self.root.join("settings.json")).sanitized()
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        write_json(&self.root.join("settings.json"), settings)
    }

    pub fn history(&self) -> History {
        read_json(&self.root.join("history.json"))
    }

    pub fn backups_root(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn backup_dir(&self, session_id: &str) -> Result<PathBuf, String> {
        Ok(self.backups_root().join(valid_id(session_id)?))
    }

    pub fn record(&self, session_id: &str) -> Result<SessionRecord, String> {
        let path = self
            .root
            .join("sessions")
            .join(format!("{}.json", valid_id(session_id)?));
        let bytes = fs::read(path).map_err(|_| "session not found".to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    /// Persists a finished session and folds it into the all-time totals.
    pub fn add_session(&self, record: &SessionRecord) -> Result<History, String> {
        let _guard = self.lock.lock().map_err(|_| "store poisoned")?;
        let id = valid_id(&record.summary.id)?;
        write_json(
            &self.root.join("sessions").join(format!("{id}.json")),
            record,
        )?;
        let mut history = self.history();
        let s = &record.summary;
        history.totals.saved_bytes += s.saved_bytes;
        history.totals.original_bytes += s.original_bytes;
        history.totals.photos += s.optimized as u64;
        history.totals.sessions += 1;
        history.totals.since.get_or_insert(s.started_at);
        history.sessions.insert(0, s.clone());
        history.sessions.truncate(MAX_SESSIONS);
        write_json(&self.root.join("history.json"), &history)?;
        Ok(history)
    }

    /// Replaces a session's summary (after undo / pruning) and adjusts totals by the delta.
    pub fn update_session(
        &self,
        record: &SessionRecord,
        previous: &SessionSummary,
    ) -> Result<History, String> {
        let _guard = self.lock.lock().map_err(|_| "store poisoned")?;
        let id = valid_id(&record.summary.id)?;
        write_json(
            &self.root.join("sessions").join(format!("{id}.json")),
            record,
        )?;
        let mut history = self.history();
        let now = &record.summary;
        let t = &mut history.totals;
        t.saved_bytes = t.saved_bytes.saturating_sub(previous.saved_bytes) + now.saved_bytes;
        t.original_bytes =
            t.original_bytes.saturating_sub(previous.original_bytes) + now.original_bytes;
        t.photos = t.photos.saturating_sub(previous.optimized as u64) + now.optimized as u64;
        if let Some(slot) = history.sessions.iter_mut().find(|s| s.id == now.id) {
            *slot = now.clone();
        }
        write_json(&self.root.join("history.json"), &history)?;
        Ok(history)
    }

    /// Sessions interrupted mid-way (crash, power loss, force quit) never wrote their record,
    /// but every replaced original was journaled next to its backup. Rebuild those sessions so
    /// they appear in History and can be undone. Returns how many were recovered.
    pub fn recover_interrupted(&self) -> Result<usize, String> {
        let Ok(entries) = fs::read_dir(self.backups_root()) else {
            return Ok(0);
        };
        let mut recovered = 0;
        for entry in entries.filter_map(|e| e.ok()) {
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if valid_id(&id).is_err() || self.record(&id).is_ok() {
                continue;
            }
            let Ok(journal) = fs::read_to_string(entry.path().join("manifest.jsonl")) else {
                continue;
            };
            let backups: Vec<BackupEntry> = journal
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .filter(|b: &BackupEntry| b.backup.exists())
                .collect();
            if backups.is_empty() {
                continue;
            }
            self.add_session(&recovered_record(&id, backups))?;
            recovered += 1;
        }
        Ok(recovered)
    }
}

fn recovered_record(id: &str, backups: Vec<BackupEntry>) -> SessionRecord {
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let results: Vec<FileResult> = backups
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (before, after) = (size(&b.backup), size(&b.original));
            FileResult {
                id: i,
                path: b.original.clone(),
                name: b
                    .original
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                status: FileStatus::Done,
                original_bytes: before,
                output_bytes: after,
                outputs: vec![],
                original_path: Some(b.backup.clone()),
                width: 0,
                height: 0,
                score: None,
                quality: None,
                skip_reason: None,
                error: None,
                millis: 0,
                preview_path: None,
                lossless: false,
            }
        })
        .collect();
    let started = id.parse().unwrap_or(0);
    let (original, output): (u64, u64) = results.iter().fold((0, 0), |(a, b), r| {
        (a + r.original_bytes, b + r.output_bytes)
    });
    SessionRecord {
        summary: SessionSummary {
            id: id.to_string(),
            started_at: started,
            finished_at: started,
            photos: results.len(),
            optimized: results.len(),
            skipped: 0,
            failed: 0,
            original_bytes: original,
            output_bytes: output,
            saved_bytes: original.saturating_sub(output),
            output_mode: OutputMode::Replace,
            strength: fino_core::Strength::default(),
            can_undo: true,
            undone: false,
        },
        results,
        backups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OutputMode, Totals};
    use fino_core::Strength;

    fn summary(id: &str, saved: u64, finished_at: u64) -> SessionSummary {
        SessionSummary {
            id: id.into(),
            started_at: finished_at - 10,
            finished_at,
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

    #[test]
    fn sessions_accumulate_into_totals_and_updates_apply_deltas() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let a = SessionRecord {
            summary: summary("a1", 100, 1_000),
            results: vec![],
            backups: vec![],
        };
        let b = SessionRecord {
            summary: summary("b2", 50, 2_000),
            results: vec![],
            backups: vec![],
        };
        store.add_session(&a).unwrap();
        let history = store.add_session(&b).unwrap();
        assert_eq!(
            history.totals,
            Totals {
                saved_bytes: 150,
                original_bytes: 300,
                photos: 4,
                sessions: 2,
                since: Some(990)
            }
        );
        assert_eq!(history.sessions[0].id, "b2");

        let mut undone = a.clone();
        undone.summary = SessionSummary {
            saved_bytes: 0,
            optimized: 0,
            original_bytes: 0,
            can_undo: false,
            undone: true,
            ..a.summary.clone()
        };
        let history = store.update_session(&undone, &a.summary).unwrap();
        assert_eq!(history.totals.saved_bytes, 50);
        assert_eq!(history.totals.photos, 2);
        assert!(history.sessions[1].undone);
    }

    #[test]
    fn recovers_a_session_interrupted_before_its_record_was_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("data"));
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
        assert_eq!(store.history().sessions.len(), 1);
    }

    #[test]
    fn rejects_path_traversal_in_session_ids() {
        let store = Store::new(PathBuf::from("/tmp/x"));
        assert!(store.backup_dir("../../etc").is_err());
        assert!(store.record("a/b").is_err());
    }
}
