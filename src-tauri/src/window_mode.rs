//! Full window vs. the compact "mini" droplet: a small square that floats above other
//! windows so photos can be dragged onto it from anywhere. The web UI renders the matching
//! layout from the same setting.
//!
//! Switching keeps the window's top-right corner — where the toggle button sits under the
//! pointer — and grows or shrinks around it with the system's own resize animation, clamped
//! so the window always stays inside the screen's visible area (no menu bar, no Dock).

use std::sync::{Arc, Mutex};
use tauri::{LogicalSize, Runtime, WebviewWindow};

/// Mini window, in points.
pub const MINI: (f64, f64) = (300.0, 324.0);
/// Full window defaults, matching tauri.conf.json.
const FULL: (f64, f64) = (1000.0, 660.0);
const FULL_MIN: (f64, f64) = (880.0, 580.0);

/// The full window's size before going mini, so expanding gives it back.
#[derive(Default, Clone)]
pub struct Remembered(Arc<Mutex<Option<(f64, f64)>>>);

impl Remembered {
    fn get(&self) -> Option<(f64, f64)> {
        self.0.lock().ok().and_then(|slot| *slot)
    }

    fn set(&self, size: (f64, f64)) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(size);
        }
    }
}

/// A rectangle in screen points with the origin at the bottom left (AppKit's convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the window goes: `size`, anchored at `current`'s top-right corner, then nudged (and
/// if need be shrunk) to lie entirely inside `visible`.
pub fn target_frame(current: Frame, size: (f64, f64), visible: Frame) -> Frame {
    let width = size.0.min(visible.width);
    let height = size.1.min(visible.height);
    let right = current.x + current.width;
    let top = current.y + current.height;
    Frame {
        x: (right - width).clamp(visible.x, visible.x + visible.width - width),
        y: (top - height).clamp(visible.y, visible.y + visible.height - height),
        width,
        height,
    }
}

fn logical((w, h): (f64, f64)) -> LogicalSize<f64> {
    LogicalSize::new(w, h)
}

/// Switches modes. `animate` is off at launch, while the window is still hidden.
pub fn apply<R: Runtime>(
    window: &WebviewWindow<R>,
    compact: bool,
    remembered: &Remembered,
    animate: bool,
) {
    // Tauri's setters and `run_on_main_thread` share one main-thread queue, so these run
    // in order: constraints that allow the new size, the move, then the final constraints.
    let outcome = (|| -> tauri::Result<()> {
        if compact {
            window.set_maximizable(false)?;
            window.set_min_size(Some(logical(MINI)))?;
        } else {
            window.set_always_on_top(false)?;
            window.set_resizable(true)?;
        }
        move_window(window, compact, remembered.clone(), animate)?;
        if compact {
            window.set_resizable(false)?;
            window.set_always_on_top(true)
        } else {
            window.set_maximizable(true)?;
            window.set_min_size(Some(logical(FULL_MIN)))
        }
    })();
    if let Err(e) = outcome {
        eprintln!("fino: could not change the window mode: {e}");
    }
}

#[cfg(target_os = "macos")]
fn move_window<R: Runtime>(
    window: &WebviewWindow<R>,
    compact: bool,
    remembered: Remembered,
    animate: bool,
) -> tauri::Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let ns_window = window.ns_window()? as usize;
    window.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // SAFETY: Tauri's NSWindow pointer stays valid while the window exists, and this
        // closure runs on the main thread (checked above), as AppKit requires.
        let ns_window: &NSWindow = unsafe { &*(ns_window as *const NSWindow) };
        let frame = ns_window.frame();
        let current = Frame {
            x: frame.origin.x,
            y: frame.origin.y,
            width: frame.size.width,
            height: frame.size.height,
        };
        if compact && current.width > MINI.0 + 1.0 {
            remembered.set((current.width, current.height));
        }
        let size = if compact {
            MINI
        } else {
            remembered.get().unwrap_or(FULL)
        };
        let visible = ns_window
            .screen()
            .or_else(|| NSScreen::mainScreen(mtm))
            .map(|screen| screen.visibleFrame())
            .unwrap_or(frame);
        let visible = Frame {
            x: visible.origin.x,
            y: visible.origin.y,
            width: visible.size.width,
            height: visible.size.height,
        };
        let t = target_frame(current, size, visible);
        let rect = NSRect::new(NSPoint::new(t.x, t.y), NSSize::new(t.width, t.height));
        ns_window.setFrame_display_animate(rect, true, animate);
    })
}

#[cfg(not(target_os = "macos"))]
fn move_window<R: Runtime>(
    window: &WebviewWindow<R>,
    compact: bool,
    remembered: Remembered,
    _animate: bool,
) -> tauri::Result<()> {
    let scale = window.scale_factor()?;
    let now = window.inner_size()?.to_logical::<f64>(scale);
    if compact && now.width > MINI.0 + 1.0 {
        remembered.set((now.width, now.height));
    }
    let size = if compact {
        MINI
    } else {
        remembered.get().unwrap_or(FULL)
    };
    window.set_size(logical(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1512×982 display with a 33 pt menu bar (AppKit: y grows upward).
    const SCREEN: Frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 949.0,
    };

    #[test]
    fn shrinking_keeps_the_top_right_corner_under_the_button() {
        let full = Frame {
            x: 200.0,
            y: 150.0,
            width: 1000.0,
            height: 660.0,
        };
        let mini = target_frame(full, MINI, SCREEN);
        assert_eq!(mini.x + mini.width, full.x + full.width);
        assert_eq!(mini.y + mini.height, full.y + full.height);
        assert_eq!((mini.width, mini.height), MINI);
    }

    #[test]
    fn growing_near_the_left_edge_stays_on_screen() {
        let mini = Frame {
            x: 10.0,
            y: 500.0,
            width: 300.0,
            height: 324.0,
        };
        let full = target_frame(mini, FULL, SCREEN);
        assert_eq!(
            full.x, 0.0,
            "pushed right instead of spilling off the left edge"
        );
        assert_eq!(
            full.y + full.height,
            mini.y + mini.height,
            "top edge stays put"
        );
    }

    #[test]
    fn growing_near_the_bottom_or_top_stays_on_screen() {
        let low = Frame {
            x: 900.0,
            y: 20.0,
            width: 300.0,
            height: 324.0,
        };
        let full = target_frame(low, FULL, SCREEN);
        assert_eq!(full.y, 0.0);
        let high = Frame {
            x: 900.0,
            y: 940.0,
            width: 300.0,
            height: 324.0,
        };
        let full = target_frame(high, FULL, SCREEN);
        assert_eq!(full.y + full.height, SCREEN.height, "under the menu bar");
    }

    #[test]
    fn a_window_bigger_than_the_screen_is_shrunk_to_fit() {
        let small = Frame {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let tiny_screen = Frame {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 500.0,
        };
        let fitted = target_frame(small, (1000.0, 660.0), tiny_screen);
        assert_eq!(
            (fitted.x, fitted.y, fitted.width, fitted.height),
            (0.0, 0.0, 900.0, 500.0)
        );
    }
}
