mod appearance;
mod backups;
mod commands;
mod model;
mod origin;
mod overview;
mod session;
mod store;
mod window_mode;

use commands::AppState;
use std::sync::atomic::AtomicBool;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::OpenedPaths::default())
        .setup(|app| {
            // Thumbnails only live for the session that made them.
            if let Ok(cache) = app.path().app_cache_dir() {
                let _ = std::fs::remove_dir_all(cache.join("previews"));
            }
            let root = app.path().app_data_dir()?;
            let store = store::Store::open(root)?;
            if let Err(e) = store.recover_interrupted() {
                eprintln!("fino: could not recover interrupted sessions: {e}");
            }
            let settings = store.settings();
            let window_size = window_mode::Remembered::default();
            if let Some(window) = app.get_webview_window("main") {
                appearance::apply(&window, settings.appearance);
                if settings.compact_window {
                    window_mode::apply(&window, true, &window_size, false);
                }
                // Created hidden so it never flashes at the wrong size or theme.
                let _ = window.show();
            }
            let retention = settings.backup_retention_days;
            if let Err(e) = store.maintain_backups(retention, model::now_millis()) {
                eprintln!("fino: backup maintenance: {e}");
            }
            app.manage(AppState {
                store,
                busy: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                backups: Default::default(),
                window_size,
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::ThemeChanged(_) = event {
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    appearance::sync_background(&webview);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_history,
            commands::session_results,
            commands::optimize,
            commands::cancel_session,
            commands::undo_session,
            commands::discard_backup,
            commands::free_backups,
            commands::export_log,
            commands::take_opened_paths,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Fino");

    app.run(|handle, event| {
        // Photos dropped on the Dock icon or opened with "Open With → Fino".
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = event {
            let paths: Vec<String> = urls
                .into_iter()
                .filter_map(|u| u.to_file_path().ok())
                .map(|p| p.display().to_string())
                .collect();
            handle.state::<commands::OpenedPaths>().push(paths);
            let _ = handle.emit("open-paths", ());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (handle, event);
    });
}
