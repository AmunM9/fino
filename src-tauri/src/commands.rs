//! Tauri commands — the only surface the web UI can call.

use crate::model::{
    now_millis, FileResult, Overview, SessionEvent, SessionRecord, SessionSummary, Settings,
};
use crate::overview;
use crate::session::{self, Context};
use crate::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use tauri::{ipc::Channel, AppHandle, Manager, State};

pub struct AppState {
    pub store: Store,
    pub busy: AtomicBool,
    pub cancel: AtomicBool,
    pub opened: Mutex<Vec<String>>,
    /// Held by anything that deletes or restores backups (undo, discard, maintenance) so
    /// they never interleave.
    pub backups: Mutex<()>,
    pub window_size: crate::window_mode::Remembered,
}

impl AppState {
    fn lock_backups(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.backups
            .lock()
            .map_err(|_| "backup bookkeeping is unavailable; restart Fino".to_string())
    }

    /// Fresh History for the UI. Expired or hand-deleted backups are settled first, unless an
    /// undo or a discard is mid-way — that call returns its own fresh overview when done.
    fn overview(&self, limit: Option<u32>) -> Result<Overview, String> {
        if let Ok(_guard) = self.backups.try_lock() {
            let retention = self.store.settings().backup_retention_days;
            if let Err(e) = self.store.maintain_backups(retention, now_millis()) {
                eprintln!("fino: backup maintenance: {e}");
            }
        }
        overview::build(&self.store, page_size(limit))
    }
}

/// Sessions per History page; the UI asks for more as the user scrolls.
const HISTORY_PAGE: u32 = 50;
const HISTORY_MAX: u32 = 10_000;

fn page_size(limit: Option<u32>) -> usize {
    limit.unwrap_or(HISTORY_PAGE).clamp(1, HISTORY_MAX) as usize
}

/// Runs `job` off the main thread so disk work never freezes the window.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    job: impl FnOnce(&AppState) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || job(&app.state::<AppState>()))
        .await
        .map_err(|e| e.to_string())?
}

/// Lets the UI show `path` through the asset protocol (before/after viewer).
fn allow_asset(app: &AppHandle, path: &Path) {
    let _ = app.asset_protocol_scope().allow_file(path);
}

fn allow_record_assets(app: &AppHandle, record: &SessionRecord) {
    for r in &record.results {
        if let Some(original) = &r.original_path {
            allow_asset(app, original);
        }
        for out in &r.outputs {
            allow_asset(app, &out.path);
        }
    }
}

fn absolute_paths(paths: Vec<String>) -> Result<Vec<PathBuf>, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    if paths.iter().any(|p| !p.is_absolute()) {
        return Err("paths must be absolute".into());
    }
    Ok(paths)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.store.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    if settings
        .export_dir
        .as_ref()
        .is_some_and(|d| !d.is_absolute())
    {
        return Err("export folder must be an absolute path".into());
    }
    let settings = settings.sanitized();
    let previous = state.store.settings();
    state.store.save_settings(&settings)?;
    if let Some(window) = app.get_webview_window("main") {
        crate::appearance::apply(&window, settings.appearance);
        if previous.compact_window != settings.compact_window {
            crate::window_mode::apply(&window, settings.compact_window, &state.window_size);
        }
    }
    Ok(settings)
}

#[tauri::command]
pub async fn get_history(app: AppHandle, limit: Option<u32>) -> Result<Overview, String> {
    blocking(app, move |state| state.overview(limit)).await
}

/// A session's results with vanished files dropped, for the before/after viewer.
#[tauri::command]
pub fn session_results(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<Vec<FileResult>, String> {
    let record = state.store.record(&id)?;
    let record = SessionRecord {
        results: overview::with_available_files(record.results),
        ..record
    };
    allow_record_assets(&app, &record);
    Ok(record.results)
}

/// Frees one session's backups; it can no longer be undone.
#[tauri::command]
pub async fn discard_backup(
    app: AppHandle,
    id: String,
    limit: Option<u32>,
) -> Result<Overview, String> {
    blocking(app, move |state| {
        // Only finished sessions: a folder without a record is the session running now.
        state.store.record(&id)?;
        state
            .lock_backups()
            .and_then(|_guard| state.store.discard_backup(&id))?;
        state.overview(limit)
    })
    .await
}

/// Frees the backups of every finished session.
#[tauri::command]
pub async fn free_backups(app: AppHandle, limit: Option<u32>) -> Result<Overview, String> {
    blocking(app, move |state| {
        state
            .lock_backups()
            .and_then(|_guard| state.store.discard_all_backups())?;
        state.overview(limit)
    })
    .await
}

#[tauri::command]
pub fn cancel_session(state: State<AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// Optimizes the dropped `paths`, streaming progress over `on_event`.
#[tauri::command]
pub async fn optimize(
    app: AppHandle,
    paths: Vec<String>,
    on_event: Channel<SessionEvent>,
) -> Result<SessionSummary, String> {
    let paths = absolute_paths(paths)?;
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::AcqRel) {
        return Err("a session is already running".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    let worker_app = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let state = worker_app.state::<AppState>();
        let session_id = format!("{}", now_millis());
        let preview_dir = worker_app
            .path()
            .app_cache_dir()
            .ok()
            .map(|cache| cache.join("previews").join(&session_id));
        let ctx = Context {
            backup_dir: state.store.backup_dir(&session_id)?,
            session_id,
            settings: state.store.settings(),
            cancel: &state.cancel,
            hint: Default::default(),
            preview_dir,
        };
        let emit = |event: SessionEvent| {
            if let SessionEvent::File { result } = &event {
                if let Some(original) = &result.original_path {
                    allow_asset(&worker_app, original);
                }
                if let Some(preview) = &result.preview_path {
                    allow_asset(&worker_app, preview);
                }
                for out in &result.outputs {
                    allow_asset(&worker_app, &out.path);
                }
            }
            let _ = on_event.send(event);
        };
        let record = session::run(&ctx, &paths, &emit);
        if record.summary.photos > 0 {
            state.store.save_session(&record)?;
        }
        Ok::<_, String>(record.summary)
    })
    .await
    .map_err(|e| e.to_string());
    state.busy.store(false, Ordering::Release);
    outcome?
}

/// Puts every replaced original of a session back. Backups whose restore fails are kept
/// (and the session stays undoable) — undo must never lose an original.
#[tauri::command]
pub async fn undo_session(
    app: AppHandle,
    id: String,
    limit: Option<u32>,
) -> Result<Overview, String> {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::AcqRel) {
        return Err("Fino is busy; try again when the current task finishes".into());
    }
    let outcome = blocking(app.clone(), move |state| {
        state
            .lock_backups()
            .and_then(|_guard| restore_session(state, &id))?;
        state.overview(limit)
    })
    .await;
    state.busy.store(false, Ordering::Release);
    outcome
}

fn restore_session(state: &AppState, id: &str) -> Result<(), String> {
    let mut record = state.store.record(id)?;
    if !record.summary.can_undo {
        return Err("this session can no longer be undone".to_string());
    }
    let previous = record.summary.clone();
    let (restored, kept): (Vec<_>, Vec<_>) = record
        .backups
        .drain(..)
        .map(|b| {
            let outcome = match &b.output {
                Some(jpeg) => {
                    fino_core::files::restore_converted(&b.backup, &b.original, jpeg, b.written)
                }
                None => fino_core::files::restore(&b.backup, &b.original, b.written),
            };
            (b, outcome)
        })
        .partition(|(_, outcome)| outcome.is_ok());
    let failures: Vec<String> = kept
        .iter()
        .filter_map(|(b, outcome)| {
            outcome
                .as_ref()
                .err()
                .map(|e| format!("{}: {e}", b.original.display()))
        })
        .collect();
    record.backups = kept.into_iter().map(|(b, _)| b).collect();

    let restored_paths: std::collections::HashSet<_> =
        restored.iter().map(|(b, _)| b.original.clone()).collect();
    let undone_saving: u64 = record
        .results
        .iter()
        .filter(|r| restored_paths.contains(&r.path) && !r.converted) // conversions never counted
        .map(|r| r.original_bytes.saturating_sub(r.output_bytes))
        .sum();
    let fully_undone = record.backups.is_empty();
    if fully_undone {
        let _ = std::fs::remove_dir_all(state.store.backup_dir(id)?);
    }
    record.summary = SessionSummary {
        can_undo: !fully_undone,
        undone: fully_undone,
        saved_bytes: previous.saved_bytes.saturating_sub(undone_saving),
        output_bytes: previous.output_bytes + undone_saving,
        optimized: previous.optimized.saturating_sub(restored_paths.len()),
        ..previous.clone()
    };
    state.store.save_session(&record)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Some originals were kept in the backup instead of restored:\n{}",
            failures.join("\n")
        ))
    }
}

/// Paths handed to Fino by Finder before the UI was listening. The UI drains them on mount
/// and whenever `open-paths` fires, so nothing is lost on a cold "Open With" launch.
#[tauri::command]
pub fn take_opened_paths(state: State<AppState>) -> Vec<String> {
    state
        .opened
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

fn csv_field(value: &str) -> String {
    // Quote everything and neutralise spreadsheet formula injection.
    let value = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub fn session_csv(record: &SessionRecord) -> String {
    let mut out = String::from(
        "file,status,original_bytes,output_bytes,saved_percent,quality,score,reason,output\n",
    );
    for r in &record.results {
        let saved = if r.original_bytes > 0 {
            100.0 * (1.0 - r.output_bytes as f64 / r.original_bytes as f64)
        } else {
            0.0
        };
        let reason = r
            .error
            .clone()
            .unwrap_or_else(|| session::skip_label(r.skip_reason).to_string());
        let outputs: Vec<String> = r
            .outputs
            .iter()
            .map(|o| o.path.display().to_string())
            .collect();
        let row = [
            csv_field(&r.path.display().to_string()),
            csv_field(&format!("{:?}", r.status).to_lowercase()),
            r.original_bytes.to_string(),
            r.output_bytes.to_string(),
            format!("{saved:.1}"),
            r.quality.map(|q| q.to_string()).unwrap_or_default(),
            r.score.map(|s| format!("{s:.1}")).unwrap_or_default(),
            csv_field(&reason),
            csv_field(&outputs.join(" | ")),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

#[tauri::command]
pub fn export_log(state: State<AppState>, id: String, destination: String) -> Result<(), String> {
    let destination = PathBuf::from(destination);
    if !destination.is_absolute() {
        return Err("destination must be an absolute path".into());
    }
    let record = state.store.record(&id)?;
    fino_core::files::write_atomic(&destination, session_csv(&record).as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_and_defuses_formulas() {
        assert_eq!(csv_field("=cmd()"), "\"'=cmd()\"");
        assert_eq!(csv_field("a \"b\""), "\"a \"\"b\"\"\"");
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(absolute_paths(vec!["photos/a.jpg".into()]).is_err());
        assert!(absolute_paths(vec!["/photos/a.jpg".into()]).is_ok());
    }
}
