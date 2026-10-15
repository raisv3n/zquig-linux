//! Full-desktop overlay for picking a region (drag) or a window (Space, then click), like macOS's Cmd+Shift+4.

use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::capture::{Desktop, Mode};
use crate::tiles::Tiles;

pub enum Outcome {
    Cancelled,
    /// In desktop image pixels.
    Picked(Rect),
}

pub struct Overlay {
    pub desktop: Desktop,
    pub mode: Mode,
    pub outcome: Option<Outcome>,
    tiles: Option<Tiles>,
    window_mode: bool,
    start: Option<Pos2>,
}

impl Overlay {
    pub fn new(desktop: Desktop, mode: Mode) -> Self {
        Self { desktop, mode, outcome: None, tiles: None, window_mode: mode == Mode::WindowToEditor, start: None }
    }

    fn window_at(&self, p: Pos2) -> Option<Rect> {
        self.desktop.windows.iter().find(|r| r.contains(p)).map(|r| r.intersect(self.desktop.full()))
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let tiles = self.tiles.get_or_insert_with(|| Tiles::new(&ctx, "desktop", &self.desktop.image));
        // The window sits exactly on the virtual desktop, so one image pixel is one physical pixel.
        let ppp = ctx.pixels_per_point();
        let to_px = |p: Pos2| pos2(p.x * ppp, p.y * ppp);
        let to_pt = |r: Rect| Rect::from_min_max(pos2(r.min.x / ppp, r.min.y / ppp), pos2(r.max.x / ppp, r.max.y / ppp));
        let full = to_pt(self.desktop.full());
        let painter = ui.painter().clone();
        tiles.paint(&painter, full);

        let (pressed, released, secondary, pos, space, esc) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.secondary_pressed(),
                i.pointer.hover_pos().or(i.pointer.interact_pos()),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Escape),
            )
        });
        if esc || secondary {
            self.outcome = Some(Outcome::Cancelled);
            return;
        }
        if space && self.start.is_none() {
            self.window_mode = !self.window_mode;
        }
        let Some(pos) = pos else {
            painter.rect_filled(full, 0.0, Color32::from_black_alpha(60));
            return;
        };
        let px = to_px(pos);

        let selection = if self.window_mode {
            self.window_at(px)
        } else {
            self.start.map(|s| Rect::from_two_pos(s, px))
        };

        // Dim everything except the selection.
        let dim = Color32::from_black_alpha(70);
        match selection.map(to_pt) {
            Some(r) => {
                for part in [
                    Rect::from_min_max(full.min, pos2(full.max.x, r.min.y)),
                    Rect::from_min_max(pos2(full.min.x, r.max.y), full.max),
                    Rect::from_min_max(pos2(full.min.x, r.min.y), pos2(r.min.x, r.max.y)),
                    Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(full.max.x, r.max.y)),
                ] {
                    if part.is_positive() {
                        painter.rect_filled(part, 0.0, dim);
                    }
                }
                if self.window_mode {
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0, 120, 212, 40));
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0, Color32::from_rgb(0, 120, 212)), StrokeKind::Inside);
                } else {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
                }
            }
            None => {
                painter.rect_filled(full, 0.0, dim);
            }
        }

        if !self.window_mode {
            // Crosshair guides and the size readout, like macOS.
            let guide = Stroke::new(1.0, Color32::from_white_alpha(120));
            painter.line_segment([pos2(full.min.x, pos.y), pos2(full.max.x, pos.y)], guide);
            painter.line_segment([pos2(pos.x, full.min.y), pos2(pos.x, full.max.y)], guide);
            let label = match selection {
                Some(r) => format!("{} \u{00D7} {}", r.width().round(), r.height().round()),
                None => format!("{}, {}", px.x.round(), px.y.round()),
            };
            let at = pos + vec2(14.0, 14.0);
            let galley = painter.layout_no_wrap(label, FontId::monospace(12.0), Color32::WHITE);
            let bg = Rect::from_min_size(at, galley.size() + vec2(10.0, 6.0));
            painter.rect_filled(bg, CornerRadius::same(4), Color32::from_black_alpha(170));
            painter.galley(at + vec2(5.0, 3.0), galley, Color32::WHITE);
        }

        let hint = if self.window_mode {
            "Click a window to capture it  \u{00B7}  Space: drag a region  \u{00B7}  Esc: cancel"
        } else {
            "Drag to capture a region  \u{00B7}  Space: pick a window  \u{00B7}  Esc: cancel"
        };
        if let Some(m) = self.desktop.monitors.iter().find(|m| m.rect.contains(px)).or(self.desktop.monitors.first()) {
            let top = to_pt(m.rect);
            let galley = painter.layout_no_wrap(hint.into(), FontId::proportional(13.0), Color32::WHITE);
            let size = galley.size() + vec2(24.0, 12.0);
            let r = Align2::CENTER_TOP.align_size_within_rect(size, top.shrink2(vec2(0.0, 24.0)));
            painter.rect_filled(r, CornerRadius::same(8), Color32::from_black_alpha(180));
            painter.galley(r.min + vec2(12.0, 6.0), galley, Color32::WHITE);
        }

        ctx.set_cursor_icon(if self.window_mode { CursorIcon::PointingHand } else { CursorIcon::Crosshair });

        if self.window_mode {
            if pressed && let Some(r) = selection {
                self.outcome = Some(Outcome::Picked(r));
            }
        } else if pressed {
            self.start = Some(px);
        } else if released && let Some(r) = selection {
            if r.width() >= 4.0 && r.height() >= 4.0 {
                self.outcome = Some(Outcome::Picked(r));
            } else {
                self.start = None; // a click without a drag: keep waiting
            }
        }
    }
}
