//! SQLite layout and record writes. Bump `VERSION` and add a step in `prepare` for every
//! schema change.

use super::enum_text;
use crate::model::SessionRecord;
use rusqlite::{params, Connection, Transaction};

const VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id             TEXT PRIMARY KEY,
    started_at     INTEGER NOT NULL,
    finished_at    INTEGER NOT NULL,
    photos         INTEGER NOT NULL,
    optimized      INTEGER NOT NULL,
    skipped        INTEGER NOT NULL,
    failed         INTEGER NOT NULL,
    original_bytes INTEGER NOT NULL,
    output_bytes   INTEGER NOT NULL,
    saved_bytes    INTEGER NOT NULL,
    output_mode    TEXT    NOT NULL,
    strength       TEXT    NOT NULL,
    can_undo       INTEGER NOT NULL,
    undone         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_by_start ON sessions (started_at DESC, id DESC);

-- One row per processed file. `result` is the full JSON the UI receives; the other columns
-- make the log queryable (CSV export, skip reasons) without parsing it.
CREATE TABLE IF NOT EXISTS files (
    session_id     TEXT    NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    idx            INTEGER NOT NULL,
    path           TEXT    NOT NULL,
    status         TEXT    NOT NULL,
    original_bytes INTEGER NOT NULL,
    output_bytes   INTEGER NOT NULL,
    skip_reason    TEXT,
    error          TEXT,
    result         TEXT    NOT NULL,
    PRIMARY KEY (session_id, idx)
);

-- Undo manifest: where each replaced original was moved and what Fino wrote in its place.
CREATE TABLE IF NOT EXISTS backups (
    session_id TEXT    NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    original   TEXT    NOT NULL,
    backup     TEXT    NOT NULL,
    output     TEXT,
    written    TEXT,
    PRIMARY KEY (session_id, idx)
);

-- All-time totals, always derived: they cannot drift from the listed sessions.
CREATE VIEW IF NOT EXISTS totals AS
    SELECT COALESCE(SUM(saved_bytes), 0)    AS saved_bytes,
           COALESCE(SUM(original_bytes), 0) AS original_bytes,
           COALESCE(SUM(optimized), 0)      AS photos,
           COUNT(*)                         AS sessions,
           MIN(started_at)                  AS since
    FROM sessions;
";

pub fn prepare(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_V1)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
    }
    Ok(())
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Replaces everything stored for `id` with `record` (caller owns the transaction).
pub fn write_record(tx: &Transaction, id: &str, record: &SessionRecord) -> rusqlite::Result<()> {
    let s = &record.summary;
    tx.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
    tx.execute(
        "INSERT INTO sessions (id, started_at, finished_at, photos, optimized, skipped, failed,
             original_bytes, output_bytes, saved_bytes, output_mode, strength, can_undo, undone)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            id,
            s.started_at as i64,
            s.finished_at as i64,
            s.photos as i64,
            s.optimized as i64,
            s.skipped as i64,
            s.failed as i64,
            s.original_bytes as i64,
            s.output_bytes as i64,
            s.saved_bytes as i64,
            enum_text(&s.output_mode),
            enum_text(&s.strength),
            s.can_undo,
            s.undone,
        ],
    )?;
    let mut file = tx.prepare_cached(
        "INSERT INTO files (session_id, idx, path, status, original_bytes, output_bytes,
             skip_reason, error, result)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (idx, r) in record.results.iter().enumerate() {
        let json = serde_json::to_string(r).unwrap_or_default();
        file.execute(params![
            id,
            idx as i64,
            path_text(&r.path),
            enum_text(&r.status),
            r.original_bytes as i64,
            r.output_bytes as i64,
            r.skip_reason.map(|reason| enum_text(&reason)),
            r.error,
            json,
        ])?;
    }
    let mut backup = tx.prepare_cached(
        "INSERT INTO backups (session_id, idx, original, backup, output, written)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (idx, b) in record.backups.iter().enumerate() {
        backup.execute(params![
            id,
            idx as i64,
            path_text(&b.original),
            path_text(&b.backup),
            b.output.as_deref().map(path_text),
            b.written
                .as_ref()
                .and_then(|w| serde_json::to_string(w).ok()),
        ])?;
    }
    Ok(())
}
