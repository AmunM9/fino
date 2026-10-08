//! The macOS menu bar in Fino's language. Same items as Tauri's default menu, plus
//! "Settings…" (⌘,), which opens the Settings view. Windows has no menu bar: its frameless
//! window draws its own controls.

use crate::locale::Language;
use tauri::menu::{
    AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Runtime};

/// Menu event id for "Settings…"; the web UI listens for the event of the same name.
pub const OPEN_SETTINGS: &str = "open-settings";

struct Labels {
    about: &'static str,
    settings: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
    file: &'static str,
    close_window: &'static str,
    edit: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    copy: &'static str,
    paste: &'static str,
    select_all: &'static str,
    view: &'static str,
    fullscreen: &'static str,
    window: &'static str,
    minimize: &'static str,
    zoom: &'static str,
    help: &'static str,
}

/// Wording of the system's own menus in each language, so Fino's read like any other app's.
fn labels(language: Language) -> Labels {
    match language {
        Language::En => Labels {
            about: "About Fino",
            settings: "Settings…",
            services: "Services",
            hide: "Hide Fino",
            hide_others: "Hide Others",
            show_all: "Show All",
            quit: "Quit Fino",
            file: "File",
            close_window: "Close Window",
            edit: "Edit",
            undo: "Undo",
            redo: "Redo",
            cut: "Cut",
            copy: "Copy",
            paste: "Paste",
            select_all: "Select All",
            view: "View",
            fullscreen: "Enter Full Screen",
            window: "Window",
            minimize: "Minimize",
            zoom: "Zoom",
            help: "Help",
        },
        Language::Es => Labels {
            about: "Acerca de Fino",
            settings: "Ajustes…",
            services: "Servicios",
            hide: "Ocultar Fino",
            hide_others: "Ocultar otros",
            show_all: "Mostrar todo",
            quit: "Salir de Fino",
            file: "Archivo",
            close_window: "Cerrar ventana",
            edit: "Edición",
            undo: "Deshacer",
            redo: "Rehacer",
            cut: "Cortar",
            copy: "Copiar",
            paste: "Pegar",
            select_all: "Seleccionar todo",
            view: "Visualización",
            fullscreen: "Entrar en pantalla completa",
            window: "Ventana",
            minimize: "Minimizar",
            zoom: "Zoom",
            help: "Ayuda",
        },
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>, language: Language) -> tauri::Result<Menu<R>> {
    let l = labels(language);
    let info = app.package_info();
    let config = app.config();
    // The standard About panel: name, version and the copyright from tauri.conf.json.
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        ..Default::default()
    };
    let separator = || PredefinedMenuItem::separator(app);

    let app_menu = Submenu::with_items(
        app,
        info.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, Some(l.about), Some(about))?,
            &separator()?,
            &MenuItem::with_id(app, OPEN_SETTINGS, l.settings, true, Some("CmdOrCtrl+,"))?,
            &separator()?,
            &PredefinedMenuItem::services(app, Some(l.services))?,
            &separator()?,
            &PredefinedMenuItem::hide(app, Some(l.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(l.hide_others))?,
            &PredefinedMenuItem::show_all(app, Some(l.show_all))?,
            &separator()?,
            &PredefinedMenuItem::quit(app, Some(l.quit))?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        l.file,
        true,
        &[&PredefinedMenuItem::close_window(
            app,
            Some(l.close_window),
        )?],
    )?;
    let edit = Submenu::with_items(
        app,
        l.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(l.undo))?,
            &PredefinedMenuItem::redo(app, Some(l.redo))?,
            &separator()?,
            &PredefinedMenuItem::cut(app, Some(l.cut))?,
            &PredefinedMenuItem::copy(app, Some(l.copy))?,
            &PredefinedMenuItem::paste(app, Some(l.paste))?,
            &PredefinedMenuItem::select_all(app, Some(l.select_all))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        l.view,
        true,
        &[&PredefinedMenuItem::fullscreen(app, Some(l.fullscreen))?],
    )?;
    // These ids make macOS list open windows and add its Help search field.
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        l.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(l.minimize))?,
            &PredefinedMenuItem::maximize(app, Some(l.zoom))?,
            &separator()?,
            &PredefinedMenuItem::close_window(app, Some(l.close_window))?,
        ],
    )?;
    let help = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, l.help, true, &[])?;

    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
}

/// Installs the menu for `language`; a failure only leaves the previous menu in place.
pub fn apply<R: Runtime>(app: &AppHandle<R>, language: Language) {
    match build(app, language) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                eprintln!("fino: could not set the menu: {e}");
            }
        }
        Err(e) => eprintln!("fino: could not build the menu: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_name_the_app_in_about_and_quit() {
        for language in [Language::En, Language::Es] {
            let l = labels(language);
            assert!(l.about.contains("Fino") && l.quit.contains("Fino") && l.hide.contains("Fino"));
            assert!(l.settings.ends_with('…'));
        }
    }
}
