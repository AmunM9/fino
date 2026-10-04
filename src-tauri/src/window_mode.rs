//! Full window vs. the compact "mini" droplet: a small square that floats above other
//! windows so photos can be dragged onto it from anywhere. The web UI renders the matching
//! layout from the same setting.

use std::sync::Mutex;
use tauri::{LogicalSize, Runtime, WebviewWindow};

/// Mini window, in points.
pub const MINI: (f64, f64) = (300.0, 324.0);
/// Full window defaults, matching tauri.conf.json.
const FULL: (f64, f64) = (1000.0, 660.0);
const FULL_MIN: (f64, f64) = (880.0, 580.0);

/// The full window's size before going mini, so expanding puts it back as it was.
#[derive(Default)]
pub struct Remembered(Mutex<Option<(f64, f64)>>);

fn logical((w, h): (f64, f64)) -> LogicalSize<f64> {
    LogicalSize::new(w, h)
}

fn current_size<R: Runtime>(window: &WebviewWindow<R>) -> Option<(f64, f64)> {
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some((size.width, size.height))
}

pub fn apply<R: Runtime>(window: &WebviewWindow<R>, compact: bool, remembered: &Remembered) {
    let outcome = if compact {
        enter_mini(window, remembered)
    } else {
        leave_mini(window, remembered)
    };
    if let Err(e) = outcome {
        eprintln!("fino: could not change the window mode: {e}");
    }
}

fn enter_mini<R: Runtime>(window: &WebviewWindow<R>, remembered: &Remembered) -> tauri::Result<()> {
    let size = current_size(window).filter(|&(w, h)| w > MINI.0 && h > MINI.1);
    if let (Some(size), Ok(mut slot)) = (size, remembered.0.lock()) {
        *slot = Some(size);
    }
    window.set_maximizable(false)?;
    window.set_min_size(Some(logical(MINI)))?;
    window.set_size(logical(MINI))?;
    window.set_resizable(false)?;
    window.set_always_on_top(true)
}

fn leave_mini<R: Runtime>(window: &WebviewWindow<R>, remembered: &Remembered) -> tauri::Result<()> {
    if current_size(window).is_some_and(|(w, h)| w > MINI.0 + 1.0 || h > MINI.1 + 1.0) {
        return Ok(()); // already full size
    }
    let size = remembered
        .0
        .lock()
        .ok()
        .and_then(|slot| *slot)
        .unwrap_or(FULL);
    window.set_always_on_top(false)?;
    window.set_resizable(true)?;
    window.set_maximizable(true)?;
    window.set_size(logical(size))?;
    window.set_min_size(Some(logical(FULL_MIN)))
}
