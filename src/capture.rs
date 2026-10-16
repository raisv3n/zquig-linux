//! Screen capture: the whole virtual desktop in one grab, plus the visible windows for window mode.

use std::path::PathBuf;

use egui::{Rect, pos2};
use tiny_skia::Pixmap;

pub use crate::os::{grab, monitor_list, screenshots_dir};

/// What the hotkeys and menu commands ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Ctrl+Shift+1: region to the clipboard.
    RegionToClipboard,
    /// Ctrl+Shift+2: full screen to the clipboard.
    ScreenToClipboard,
    /// Ctrl+Shift+3: full screen saved as a JPEG.
    ScreenToFile,
    /// Ctrl+Shift+4: region (Space for a window) saved as a JPEG.
    RegionToFile,
    /// Menu commands: open the capture in an editor window.
    RegionToEditor,
    WindowToEditor,
    ScreenToEditor,
}

impl Mode {
    pub fn is_interactive(self) -> bool {
        matches!(self, Mode::RegionToClipboard | Mode::RegionToFile | Mode::RegionToEditor | Mode::WindowToEditor)
    }
}

/// A frozen copy of the desktop. Rectangles are in physical pixels relative to `origin`'s monitor layout.
pub struct Desktop {
    pub image: Pixmap,
    /// Top-left of the virtual screen (can be negative with monitors left of / above the primary).
    pub origin: (i32, i32),
    pub monitors: Vec<Monitor>,
    /// Visible top-level windows, front to back, in image coordinates.
    pub windows: Vec<Rect>,
}

pub struct Monitor {
    /// In image coordinates.
    pub rect: Rect,
    pub work: Rect,
    pub scale: f32,
    pub primary: bool,
}

/// The primary monitor, or the first one if none is marked primary.
pub fn primary(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors.iter().find(|m| m.primary).or(monitors.first())
}

impl Desktop {
    /// Monitor scale at the given image point (pixels per point), for annotation sizing.
    pub fn scale_at(&self, r: Rect) -> f32 {
        let c = r.center();
        self.monitors
            .iter()
            .find(|m| m.rect.contains(c))
            .or(self.monitors.first())
            .map_or(1.0, |m| m.scale)
    }

    pub fn crop(&self, r: Rect) -> Option<Pixmap> {
        let r = r.intersect(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(self.image.width() as f32, self.image.height() as f32)));
        let ir = tiny_skia::IntRect::from_ltrb(
            r.min.x.round() as i32,
            r.min.y.round() as i32,
            r.max.x.round() as i32,
            r.max.y.round() as i32,
        )?;
        self.image.clone_rect(ir)
    }

    pub fn full(&self) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(self.image.width() as f32, self.image.height() as f32))
    }
}

/// "Screenshot 2026-10-15 at 22.09.33.jpg", like macOS, with " (2)" etc. on a clash.
pub fn screenshot_path(ext: &str) -> PathBuf {
    let dir = screenshots_dir();
    let stamp = crate::os::local_timestamp();
    let mut path = dir.join(format!("Screenshot {stamp}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("Screenshot {stamp} ({n}).{ext}"));
        n += 1;
    }
    path
}
