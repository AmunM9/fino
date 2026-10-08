mod appearance;
mod backups;
mod commands;
mod locale;
#[cfg(target_os = "macos")]
mod menu;
mod model;
mod origin;
mod overview;
mod session;
mod store;
mod window_mode;

use commands::AppState;
use std::sync::atomic::AtomicBool;
use tauri::{Emitter, Manager};

/// Photos passed on the command line: how Windows and Linux hand over files opened with
/// Fino ("Open with", a drop on the taskbar or a shortcut). Flags and anything that is not
/// an existing file or folder are ignored.
#[cfg(not(target_os = "macos"))]
fn paths_from_args<A: AsRef<std::ffi::OsStr>>(
    args: impl IntoIterator<Item = A>,
    cwd: &std::path::Path,
) -> Vec<String> {
    args.into_iter()
        .skip(1) // the executable
        .filter(|arg| !arg.as_ref().to_string_lossy().starts_with('-'))
        .map(|arg| cwd.join(arg.as_ref())) // `join` keeps absolute paths as they are
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect()
}

/// A second launch (opening more photos with Fino) hands its paths to this window.
#[cfg(not(target_os = "macos"))]
fn forward_to_running(app: &tauri::AppHandle, args: Vec<String>, cwd: String) {
    let paths = paths_from_args(args, std::path::Path::new(&cwd));
    if !paths.is_empty() {
        app.state::<commands::OpenedPaths>().push(paths);
        let _ = app.emit("open-paths", ());
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // Must be the first plugin: a second instance exits before anything else starts.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(forward_to_running));
    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::OpenedPaths::default())
        .setup(|app| {
            #[cfg(not(target_os = "macos"))]
            if let Ok(cwd) = std::env::current_dir() {
                // `args_os`: a file name need not be valid Unicode.
                let paths = paths_from_args(std::env::args_os(), &cwd);
                app.state::<commands::OpenedPaths>().push(paths);
            }
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
            #[cfg(target_os = "macos")]
            menu::apply(app.handle(), locale::resolve(settings.language));
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
        .on_menu_event(|app, event| {
            #[cfg(target_os = "macos")]
            if event.id() == menu::OPEN_SETTINGS {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                let _ = app.emit(menu::OPEN_SETTINGS, ());
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
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
            commands::ui_language,
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
        // macOS: photos dropped on the Dock icon or opened with "Open With → Fino".
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

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn command_line_paths_skip_the_executable_flags_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();
        let args = ["fino.exe", "--flag", "a.jpg", "missing.jpg"].map(String::from);
        assert_eq!(
            paths_from_args(args, dir.path()),
            vec![photo.display().to_string()]
        );
        let absolute = ["fino.exe".to_string(), photo.display().to_string()];
        assert_eq!(
            paths_from_args(absolute, std::path::Path::new("/elsewhere")).len(),
            1
        );
    }
}
