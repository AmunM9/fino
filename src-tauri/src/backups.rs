//! Undo backups on disk (`backups/<session id>/…`): retention, staying in sync with what is
//! really there, and freeing space on demand. Callers serialize these with
//! `AppState::backups` so they never race an undo.

use crate::model::{BackupEntry, SessionRecord, SessionSummary};
use crate::store::{valid_id, Store};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

fn folder_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok()?.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Runs `op` for every id and reports the first failure only after trying them all, so one
/// stubborn folder never keeps the rest from being freed.
fn for_each(ids: Vec<String>, op: impl Fn(&str) -> Result<(), String>) -> Result<(), String> {
    let failures: Vec<String> = ids.iter().filter_map(|id| op(id).err()).collect();
    failures.into_iter().next().map_or(Ok(()), Err)
}

impl Store {
    /// Session backup folders and the bytes each one holds.
    pub fn backup_sizes(&self) -> HashMap<String, u64> {
        let Ok(entries) = fs::read_dir(self.backups_root()) else {
            return HashMap::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let id = e.file_name().to_str()?.to_owned();
                valid_id(&id).ok()?;
                Some((id, folder_bytes(&e.path())))
            })
            .collect()
    }

    /// Deletes a session's backups. The session stays in History but can no longer be undone;
    /// the optimized photos are not touched.
    pub fn discard_backup(&self, id: &str) -> Result<(), String> {
        let dir = self.backup_dir(id)?;
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not delete {}: {e}", dir.display())),
        }
        self.forget_backups(id)
    }

    /// Frees the backups of every finished session. A folder without a record belongs to the
    /// session that is running right now and is left alone.
    pub fn discard_all_backups(&self) -> Result<(), String> {
        let finished: Vec<String> = self
            .backup_sizes()
            .into_keys()
            .filter(|id| self.record(id).is_ok())
            .collect();
        let discarded = for_each(finished, |id| self.discard_backup(id));
        discarded.and(self.reconcile_backups())
    }

    /// Deletes backups older than the retention window (session ids are start timestamps).
    /// Walks the folder itself, so sessions that fell out of the History list are cleaned too.
    pub fn prune_backups(&self, retention_days: u32, now: u64) -> Result<(), String> {
        let cutoff = now.saturating_sub(retention_days as u64 * DAY_MS);
        let expired: Vec<String> = self
            .backup_sizes()
            .into_keys()
            .filter(|id| id.parse::<u64>().is_ok_and(|started| started < cutoff))
            .collect();
        for_each(expired, |id| self.discard_backup(id))
    }

    /// Brings undo state in line with the disk: backups deleted by hand (Finder, cleanup
    /// apps) are forgotten, and a session with none left can no longer be undone.
    pub fn reconcile_backups(&self) -> Result<(), String> {
        for_each(self.undoable_sessions()?, |id| {
            let Ok(record) = self.record(id) else {
                return Ok(());
            };
            let present: Vec<BackupEntry> = record
                .backups
                .iter()
                .filter(|b| b.backup.is_file())
                .cloned()
                .collect();
            if present.is_empty() {
                self.discard_backup(id)
            } else if present.len() < record.backups.len() {
                let updated = SessionRecord {
                    backups: present,
                    ..record.clone()
                };
                self.save_session(&updated)
            } else {
                Ok(())
            }
        })
    }

    /// Retention plus reconciliation — run at launch and whenever the UI refreshes History.
    pub fn maintain_backups(&self, retention_days: u32, now: u64) -> Result<(), String> {
        let pruned = self.prune_backups(retention_days, now);
        pruned.and(self.reconcile_backups())
    }

    fn forget_backups(&self, id: &str) -> Result<(), String> {
        let Ok(record) = self.record(id) else {
            return Ok(());
        };
        if !record.summary.can_undo && record.backups.is_empty() {
            return Ok(());
        }
        let updated = SessionRecord {
            summary: SessionSummary {
                can_undo: false,
                ..record.summary.clone()
            },
            backups: vec![],
            ..record.clone()
        };
        self.save_session(&updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OutputMode;
    use fino_core::Strength;
    use std::path::PathBuf;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        photos: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("data")).unwrap();
        let photos = dir.path().join("photos");
        fs::create_dir_all(&photos).unwrap();
        Fixture {
            _dir: dir,
            store,
            photos,
        }
    }

    /// Stores a finished replace session with one backup per name.
    fn replace_session(f: &Fixture, id: &str, names: &[&str]) -> Vec<BackupEntry> {
        let dir = f.store.backup_dir(id).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let backups: Vec<BackupEntry> = names
            .iter()
            .map(|name| {
                let original = f.photos.join(name);
                fs::write(&original, b"small").unwrap();
                let backup = dir.join(format!("00000-{name}"));
                fs::write(&backup, b"the bigger original").unwrap();
                BackupEntry {
                    original,
                    backup,
                    written: None,
                    output: None,
                }
            })
            .collect();
        let started: u64 = id.parse().unwrap();
        let summary = SessionSummary {
            id: id.into(),
            started_at: started,
            finished_at: started + 10,
            photos: names.len(),
            optimized: names.len(),
            skipped: 0,
            failed: 0,
            original_bytes: 100,
            output_bytes: 40,
            saved_bytes: 60,
            output_mode: OutputMode::Replace,
            strength: Strength::Identical,
            can_undo: true,
            undone: false,
        };
        f.store
            .save_session(&SessionRecord {
                summary,
                results: vec![],
                backups: backups.clone(),
            })
            .unwrap();
        backups
    }

    fn can_undo(store: &Store, id: &str) -> bool {
        let listed = store
            .sessions(usize::MAX)
            .unwrap()
            .iter()
            .any(|s| s.id == id && s.can_undo);
        assert_eq!(listed, store.record(id).unwrap().summary.can_undo);
        listed
    }

    #[test]
    fn sizes_count_each_session_folder() {
        let f = fixture();
        replace_session(&f, "1000", &["a.jpg", "b.jpg"]);
        replace_session(&f, "2000", &["c.jpg"]);
        fs::write(f.store.backups_root().join(".DS_Store"), b"finder").unwrap();
        let sizes = f.store.backup_sizes();
        assert_eq!(sizes.len(), 2, "loose files are not sessions");
        assert_eq!(sizes["1000"], 2 * 19);
        assert_eq!(sizes["2000"], 19);
    }

    #[test]
    fn discarding_frees_the_folder_and_ends_undo_but_keeps_the_savings() {
        let f = fixture();
        replace_session(&f, "1000", &["a.jpg"]);
        let before = f.store.totals().unwrap();
        f.store.discard_backup("1000").unwrap();
        assert!(!f.store.backup_dir("1000").unwrap().exists());
        assert!(!can_undo(&f.store, "1000"));
        assert!(f.store.record("1000").unwrap().backups.is_empty());
        assert_eq!(f.store.totals().unwrap(), before);
        assert!(f.photos.join("a.jpg").exists(), "optimized photo untouched");
        f.store.discard_backup("1000").unwrap(); // idempotent
    }

    #[test]
    fn discarding_everything_spares_the_running_session() {
        let f = fixture();
        replace_session(&f, "1000", &["a.jpg"]);
        replace_session(&f, "2000", &["b.jpg"]);
        let running = f.store.backup_dir("3000").unwrap(); // no record yet
        fs::create_dir_all(&running).unwrap();
        f.store.discard_all_backups().unwrap();
        assert!(!can_undo(&f.store, "1000") && !can_undo(&f.store, "2000"));
        assert_eq!(
            f.store.backup_sizes().into_keys().collect::<Vec<_>>(),
            ["3000"]
        );
    }

    #[test]
    fn backups_deleted_by_hand_end_undo() {
        let f = fixture();
        replace_session(&f, "1000", &["a.jpg"]);
        fs::remove_dir_all(f.store.backup_dir("1000").unwrap()).unwrap();
        f.store.reconcile_backups().unwrap();
        assert!(!can_undo(&f.store, "1000"));
    }

    #[test]
    fn partially_deleted_backups_keep_the_rest_undoable() {
        let f = fixture();
        let backups = replace_session(&f, "1000", &["a.jpg", "b.jpg"]);
        fs::remove_file(&backups[0].backup).unwrap();
        f.store.reconcile_backups().unwrap();
        assert!(can_undo(&f.store, "1000"));
        assert_eq!(
            f.store.record("1000").unwrap().backups,
            vec![backups[1].clone()]
        );
    }

    #[test]
    fn pruning_expires_old_backups_only() {
        let f = fixture();
        let new = (10 * DAY_MS).to_string();
        replace_session(&f, "1000", &["a.jpg"]);
        replace_session(&f, &new, &["b.jpg"]);
        let orphan = f.store.backup_dir("2000").unwrap(); // fell out of history long ago
        fs::create_dir_all(&orphan).unwrap();
        f.store.prune_backups(7, 10 * DAY_MS + 1).unwrap();
        assert!(!f.store.backup_dir("1000").unwrap().exists());
        assert!(!orphan.exists());
        assert!(f.store.backup_dir(&new).unwrap().exists());
        assert!(!can_undo(&f.store, "1000"));
        assert!(can_undo(&f.store, &new));
    }
}
