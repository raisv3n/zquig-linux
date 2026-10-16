//! Quick Look-style preview, like the Mac's Space in Finder: Space on a file in the file manager
//! shows it in a light window. Space or Esc closes it, the arrow keys step through a multiple
//! selection, and Enter (or a double-click) opens the file in an editor window.

use std::path::{Path, PathBuf};

use egui::{Align, Color32, CursorIcon, Key, Layout, Rect, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;
use tiny_skia::Pixmap;

use crate::imageio;
use crate::tiles::Tiles;

/// Height of the bar above the image, in points.
pub const BAR: f32 = 40.0;

pub struct Peek {
    paths: Vec<PathBuf>,
    index: usize,
    /// The current file: its image and scale (pixels per point), or why it couldn't be shown.
    shown: Result<(Pixmap, f32), String>,
    tiles: Option<Tiles>,
    /// A new window title after stepping to another file.
    pub retitle: Option<String>,
    /// Set when the preview should close.
    pub done: bool,
    /// Set when the user asked to open the current file in an editor window.
    pub open: Option<PathBuf>,
}

fn load(path: &Path) -> Result<(Pixmap, f32), String> {
    if imageio::is_pdf(path) { imageio::render_pdf(path) } else { imageio::load_image(path).map(|p| (p, 1.0)) }
}

pub fn title(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Peek {
    /// Loads the first file straight away, so the window can be sized to it.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let shown = paths.first().map_or_else(|| Err("Nothing to preview.".into()), |p| load(p));
        Self { paths, index: 0, shown, tiles: None, retitle: None, done: false, open: None }
    }

    /// Nothing shown: the preview window while it's hidden.
    pub fn closed() -> Self {
        Self { paths: Vec::new(), index: 0, shown: Err(String::new()), tiles: None, retitle: None, done: true, open: None }
    }

    /// Whether this preview is showing something (as opposed to `closed`).
    pub fn is_open(&self) -> bool {
        !self.paths.is_empty()
    }

    pub fn window_title(&self) -> String {
        self.paths.get(self.index).map(|p| title(p)).unwrap_or_default()
    }

    /// The image's natural size in points, if it loaded.
    pub fn natural_size(&self) -> Option<egui::Vec2> {
        self.shown.as_ref().ok().map(|(p, s)| vec2(p.width() as f32 / s, p.height() as f32 / s))
    }

    fn step(&mut self, by: isize) {
        let n = self.paths.len() as isize;
        if n < 2 {
            return;
        }
        self.index = (self.index as isize + by).rem_euclid(n) as usize;
        self.shown = load(&self.paths[self.index]);
        self.tiles = None;
        self.retitle = Some(self.window_title());
    }

    fn open_current(&mut self) {
        self.open = self.paths.get(self.index).cloned();
        self.done = true;
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (close, prev, next, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::Space) || i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Enter),
            )
        });
        if close {
            self.done = true;
            return;
        }
        if enter {
            return self.open_current();
        }
        if prev {
            self.step(-1);
        } else if next {
            self.step(1);
        }

        let full = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(full, 0.0, Color32::from_gray(24));

        // The bar: name and position on the left, buttons on the right.
        let bar = Rect::from_min_size(full.min, vec2(full.width(), BAR));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 6.0))).layout(Layout::left_to_right(Align::Center)));
        let name = self.window_title();
        child.label(RichText::new(name).strong().size(14.0).color(Color32::from_gray(230)));
        if self.paths.len() > 1 {
            child.label(RichText::new(format!("{} of {}", self.index + 1, self.paths.len())).size(12.0).color(Color32::from_gray(150)));
        }
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let open = egui::Button::new(RichText::new(format!("{}  Open in Zquig", icon::PENCIL_SIMPLE)).size(13.0));
            if ui.add(open).on_hover_text("Annotate this file (Enter)").clicked() {
                self.open_current();
            }
            if self.paths.len() > 1 {
                if ui.button(RichText::new(icon::CARET_RIGHT).size(16.0)).on_hover_text("Next (\u{2192})").clicked() {
                    self.step(1);
                }
                if ui.button(RichText::new(icon::CARET_LEFT).size(16.0)).on_hover_text("Previous (\u{2190})").clicked() {
                    self.step(-1);
                }
            }
        });
        if self.done {
            return;
        }

        let area = Rect::from_min_max(bar.left_bottom(), full.max).shrink(12.0);
        match &self.shown {
            Ok((pix, scale)) => {
                // Fit inside the window, never larger than the image's own size.
                let size = vec2(pix.width() as f32 / scale, pix.height() as f32 / scale);
                let fit = (area.width() / size.x).min(area.height() / size.y).min(1.0);
                let rect = Rect::from_center_size(area.center(), size * fit);
                let tiles = self.tiles.get_or_insert_with(|| Tiles::new(&ctx, "peek", pix));
                tiles.paint(&painter, rect);
                let resp = ui.interact(rect, ui.id().with("peek-image"), Sense::click());
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.double_clicked() {
                    self.open_current();
                }
            }
            Err(e) => {
                let galley = painter.layout(format!("Can't preview this file.\n\n{e}"), egui::FontId::proportional(14.0), Color32::from_gray(200), area.width());
                painter.galley(area.center() - galley.size() / 2.0, galley, Color32::from_gray(200));
            }
        }
    }
}
