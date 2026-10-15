//! The little thumbnail that appears after a shortcut capture. Click it to annotate the shot.

use std::path::PathBuf;

use egui::{Color32, CornerRadius, CursorIcon, Rect, RichText, Sense, Ui, vec2};
use tiny_skia::Pixmap;

use crate::tiles::Tiles;

pub const SIZE: egui::Vec2 = vec2(340.0, 96.0);
const SECONDS: f64 = 5.0;

pub struct Toast {
    pub image: Pixmap,
    pub scale: f32,
    /// Where the shot was saved, if it was.
    pub path: Option<PathBuf>,
    message: String,
    tiles: Option<Tiles>,
    shown_at: Option<f64>,
    pub open: bool,
    pub done: bool,
}

impl Toast {
    pub fn new(image: Pixmap, scale: f32, path: Option<PathBuf>, message: String) -> Self {
        Self { image, scale, path, message, tiles: None, shown_at: None, open: false, done: false }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ui.input(|i| i.time);
        let start = *self.shown_at.get_or_insert(now);
        let rect = ui.max_rect();
        let resp = ui.interact(rect, ui.id().with("toast"), Sense::click());
        if resp.hovered() {
            self.shown_at = Some(now); // stay while the pointer is on it
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.open = true;
            self.done = true;
            return;
        }
        if now - start > SECONDS {
            self.done = true;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::ZERO, ui.visuals().window_fill);
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::ZERO, ui.visuals().window_stroke, egui::StrokeKind::Inside);
        let tiles = self.tiles.get_or_insert_with(|| Tiles::new(&ctx, "toast", &self.image));
        let (w, h) = (self.image.width() as f32, self.image.height() as f32);
        let thumb_box = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(120.0, rect.height() - 20.0));
        let s = (thumb_box.width() / w).min(thumb_box.height() / h);
        let thumb = Rect::from_center_size(thumb_box.center(), vec2(w * s, h * s));
        painter.rect_filled(thumb_box, CornerRadius::same(4), ui.visuals().extreme_bg_color);
        tiles.paint(painter, thumb);

        let text_rect = Rect::from_min_max(egui::pos2(thumb_box.max.x + 14.0, rect.min.y + 18.0), rect.max - vec2(10.0, 10.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        child.label(RichText::new(&self.message).strong().size(14.0));
        let detail = match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => format!("{} \u{00D7} {}", self.image.width(), self.image.height()),
        };
        child.add(egui::Label::new(RichText::new(detail).size(12.0).color(ui.visuals().weak_text_color())).truncate());
        child.label(RichText::new("Click to annotate").size(12.0).color(Color32::from_rgb(0, 120, 212)));
    }
}
