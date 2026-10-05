//! Persistence under the app's data folder:
//!
//! ```text
//! settings.json      user preferences (small, hand-editable)
//! fino.db            SQLite (WAL): sessions, per-file results, undo manifests
//! backups/<id>/…     replaced originals, kept for undo
//! ```
//!
//! History has no size cap: the UI pages through it. Totals are derived from the sessions
//! table, so they can never drift from what is listed.

mod migrate;
mod schema;

use crate::model::{
    BackupEntry, FileResult, FileStatus, OutputMode, SessionRecord, SessionSummary, Settings,
    Totals,
};
use rusqlite::{Connection, OptionalExtension, Row};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const DB_FILE: &str = "fino.db";

pub struct Store {
    root: PathBuf,
    db: Mutex<Connection>,
}

fn sql_err(e: rusqlite::Error) -> String {
    format!("history database: {e}")
}

/// Session ids become folder names; accept only what we generate.
pub(crate) fn valid_id(id: &str) -> Result<&str, String> {
    let ok = !id.is_empty()
        && id.len() <= 32
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some(id)
        .ok_or_else(|| "invalid session id".to_string())
}

/// serde's wire name for a unit enum variant ("replace", "identical", …).
fn enum_text<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn enum_parse<T: serde::de::DeserializeOwned + Default>(text: &str) -> T {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).unwrap_or_default()
}

fn summary_from_row(row: &Row) -> rusqlite::Result<SessionSummary> {
    let output_mode: String = row.get("output_mode")?;
    let strength: String = row.get("strength")?;
    Ok(SessionSummary {
        id: row.get("id")?,
        started_at: row.get::<_, i64>("started_at")? as u64,
        finished_at: row.get::<_, i64>("finished_at")? as u64,
        photos: row.get::<_, i64>("photos")? as usize,
        optimized: row.get::<_, i64>("optimized")? as usize,
        skipped: row.get::<_, i64>("skipped")? as usize,
        failed: row.get::<_, i64>("failed")? as usize,
        original_bytes: row.get::<_, i64>("original_bytes")? as u64,
        output_bytes: row.get::<_, i64>("output_bytes")? as u64,
        saved_bytes: row.get::<_, i64>("saved_bytes")? as u64,
        output_mode: if output_mode == "export" {
            OutputMode::Export
        } else {
            OutputMode::Replace
        },
        strength: enum_parse(&strength),
        can_undo: row.get("can_undo")?,
        undone: row.get("undone")?,
    })
}

impl Store {
    /// Opens (or creates) the database and, the first time, imports the JSON history that
    /// earlier versions wrote.
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let mut conn = Connection::open(root.join(DB_FILE)).map_err(sql_err)?;
        schema::prepare(&mut conn).map_err(sql_err)?;
        let store = Self {
            root,
            db: Mutex::new(conn),
        };
        migrate::import_json_history(&store)?;
        Ok(store)
    }

    fn db(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.db
            .lock()
            .map_err(|_| "history database is unavailable; restart Fino".to_string())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> Settings {
        fs::read(self.root.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
        fino_core::files::write_atomic(&self.root.join("settings.json"), &bytes)
            .map_err(|e| e.to_string())
    }

    pub fn backups_root(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn backup_dir(&self, session_id: &str) -> Result<PathBuf, String> {
        Ok(self.backups_root().join(valid_id(session_id)?))
    }

    /// All-time totals over every stored session.
    pub fn totals(&self) -> Result<Totals, String> {
        let db = self.db()?;
        db.query_row(
            "SELECT saved_bytes, original_bytes, photos, sessions, since FROM totals",
            [],
            |row| {
                Ok(Totals {
                    saved_bytes: row.get::<_, i64>(0)? as u64,
                    original_bytes: row.get::<_, i64>(1)? as u64,
                    photos: row.get::<_, i64>(2)? as u64,
                    sessions: row.get::<_, i64>(3)? as u64,
                    since: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                })
            },
        )
        .map_err(sql_err)
    }

    /// The newest `limit` sessions, newest first.
    pub fn sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare_cached("SELECT * FROM sessions ORDER BY started_at DESC, id DESC LIMIT ?1")
            .map_err(sql_err)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map([limit], summary_from_row).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql_err)
    }

    /// Ids of sessions that can still be undone (their backups should exist).
    pub fn undoable_sessions(&self) -> Result<Vec<String>, String> {
        let db = self.db()?;
        let mut stmt = db
            .prepare_cached("SELECT id FROM sessions WHERE can_undo = 1")
            .map_err(sql_err)?;
        let rows = stmt.query_map([], |row| row.get(0)).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql_err)
    }

    /// Source paths of a session's files, in order.
    pub fn session_paths(&self, session_id: &str) -> Result<Vec<PathBuf>, String> {
        let id = valid_id(session_id)?;
        let db = self.db()?;
        let mut stmt = db
            .prepare_cached("SELECT path FROM files WHERE session_id = ?1 ORDER BY idx")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([id], |row| row.get::<_, String>(0))
            .map_err(sql_err)?;
        rows.map(|path| path.map(PathBuf::from))
            .collect::<rusqlite::Result<_>>()
            .map_err(sql_err)
    }

    pub fn record(&self, session_id: &str) -> Result<SessionRecord, String> {
        let id = valid_id(session_id)?;
        let db = self.db()?;
        let summary = db
            .query_row(
                "SELECT * FROM sessions WHERE id = ?1",
                [id],
                summary_from_row,
            )
            .optional()
            .map_err(sql_err)?
            .ok_or_else(|| "session not found".to_string())?;
        let results = {
            let mut stmt = db
                .prepare_cached("SELECT result FROM files WHERE session_id = ?1 ORDER BY idx")
                .map_err(sql_err)?;
            let rows = stmt
                .query_map([id], |row| row.get::<_, String>(0))
                .map_err(sql_err)?;
            rows.filter_map(|json| serde_json::from_str::<FileResult>(&json.ok()?).ok())
                .collect()
        };
        let backups = {
            let mut stmt = db
                .prepare_cached(
                    "SELECT original, backup, written, output FROM backups
                     WHERE session_id = ?1 ORDER BY idx",
                )
                .map_err(sql_err)?;
            let rows = stmt
                .query_map([id], |row| {
                    let written: Option<String> = row.get(2)?;
                    Ok(BackupEntry {
                        original: PathBuf::from(row.get::<_, String>(0)?),
                        backup: PathBuf::from(row.get::<_, String>(1)?),
                        written: written.and_then(|w| serde_json::from_str(&w).ok()),
                        output: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                    })
                })
                .map_err(sql_err)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql_err)?
        };
        Ok(SessionRecord {
            summary,
            results,
            backups,
        })
    }

    /// Inserts or replaces a session with all its file results and undo manifest, atomically.
    pub fn save_session(&self, record: &SessionRecord) -> Result<(), String> {
        let id = valid_id(&record.summary.id)?;
        let mut db = self.db()?;
        let tx = db.transaction().map_err(sql_err)?;
        schema::write_record(&tx, id, record).map_err(sql_err)?;
        tx.commit().map_err(sql_err)
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
            self.save_session(&recovered_record(&id, backups))?;
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
            let now_at = b.output.clone().unwrap_or_else(|| b.original.clone());
            FileResult {
                id: i,
                path: b.original.clone(),
                name: b
                    .original
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                status: FileStatus::Done,
                original_bytes: size(&b.backup),
                output_bytes: size(&now_at),
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
                converted: b.output.is_some(),
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
mod tests;
