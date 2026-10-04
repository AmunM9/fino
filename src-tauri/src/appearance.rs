//! Light / dark mode for the native window: title bar, macOS dialogs and the color shown
//! before the web view paints. The web UI follows through `prefers-color-scheme` plus a
//! `data-theme` attribute it sets itself.

use crate::model::Appearance;
use tauri::window::Color;
use tauri::{Runtime, Theme, WebviewWindow};

/// Matches `--color-bg` in src/styles/tokens.css for each theme.
const DARK_BACKGROUND: Color = Color(0x11, 0x13, 0x17, 0xff);
const LIGHT_BACKGROUND: Color = Color(0xf6, 0xf5, 0xf2, 0xff);

/// `None` lets the window follow the system.
pub fn native_theme(appearance: Appearance) -> Option<Theme> {
    match appearance {
        Appearance::System => None,
        Appearance::Light => Some(Theme::Light),
        Appearance::Dark => Some(Theme::Dark),
    }
}

fn background(theme: Theme) -> Color {
    match theme {
        Theme::Light => LIGHT_BACKGROUND,
        _ => DARK_BACKGROUND,
    }
}

pub fn apply<R: Runtime>(window: &WebviewWindow<R>, appearance: Appearance) {
    if let Err(e) = window.set_theme(native_theme(appearance)) {
        eprintln!("fino: could not set the window theme: {e}");
    }
    sync_background(window);
}

/// Keeps the color behind the web view (visible at launch and while resizing) in step
/// with the effective theme — also when macOS switches while Fino follows the system.
pub fn sync_background<R: Runtime>(window: &WebviewWindow<R>) {
    let theme = window.theme().unwrap_or(Theme::Dark);
    let _ = window.set_background_color(Some(background(theme)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;

    #[test]
    fn system_appearance_defers_to_macos() {
        assert_eq!(native_theme(Appearance::System), None);
        assert_eq!(native_theme(Appearance::Light), Some(Theme::Light));
        assert_eq!(native_theme(Appearance::Dark), Some(Theme::Dark));
        assert_eq!(background(Theme::Light), LIGHT_BACKGROUND);
    }

    #[test]
    fn settings_saved_before_the_option_existed_follow_the_system() {
        let old: Settings = serde_json::from_str(r#"{"outputMode":"replace"}"#).unwrap();
        assert_eq!(old.appearance, Appearance::System);
        let dark: Settings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
        assert_eq!(dark.appearance, Appearance::Dark);
    }
}
