//! One-time import of the JSON history written by earlier versions:
//! `history.json` (summaries + totals) and `sessions/<id>.json` (full records).
//!
//! Every record file is imported in a single transaction; only after it commits are the
//! JSON files renamed to `*.migrated`, so an interrupted import simply runs again.

use super::{schema, sql_err, valid_id, Store};
use crate::model::SessionRecord;
use std::fs;
use std::path::Path;

const LEGACY_SESSIONS: &str = "sessions";
const LEGACY_HISTORY: &str = "history.json";
const MIGRATED_SUFFIX: &str = ".migrated";

fn legacy_records(dir: &Path) -> Vec<SessionRecord> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|p| {
            let parsed = fs::read(&p)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<SessionRecord>(&bytes).ok());
            if parsed.is_none() {
                eprintln!("fino: skipped unreadable history file {}", p.display());
            }
            parsed
        })
        .filter(|r| valid_id(&r.summary.id).is_ok())
        .collect()
}

fn retire(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut retired = path.as_os_str().to_owned();
    retired.push(MIGRATED_SUFFIX);
    fs::rename(path, &retired).map_err(|e| format!("could not retire {}: {e}", path.display()))
}

/// Imports legacy JSON history if present. Returns how many sessions were imported.
pub fn import_json_history(store: &Store) -> Result<usize, String> {
    let sessions_dir = store.root().join(LEGACY_SESSIONS);
    let history_file = store.root().join(LEGACY_HISTORY);
    if !sessions_dir.is_dir() && !history_file.is_file() {
        return Ok(0);
    }
    let records = legacy_records(&sessions_dir);
    {
        let mut db = store.db()?;
        let tx = db.transaction().map_err(sql_err)?;
        for record in &records {
            schema::write_record(&tx, &record.summary.id, record).map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)?;
    }
    retire(&sessions_dir)?;
    retire(&history_file)?;
    Ok(records.len())
}
