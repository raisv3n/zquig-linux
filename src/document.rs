//! An editor window: menu bar, toolbar, the zoomable canvas, saving and export.

use std::path::{Path, PathBuf};

use egui::{
    Align, Color32, CornerRadius, CursorIcon, Event, FontFamily, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as icon;
use tiny_skia::Pixmap;
use winit::window::Window;

use crate::annotation::{Annotation, Rgba, Tool, font_size};
use crate::canvas::Canvas;
use crate::capture::Mode;
use crate::imageio;
use crate::system::Prefs;
use crate::tiles::Tiles;

pub const PALETTE: [Rgba; 9] = [
    [1.0, 0.231, 0.188, 1.0],   // red
    [1.0, 0.584, 0.0, 1.0],     // orange
    [0.84, 0.63, 0.08, 1.0],    // mustard
    [0.204, 0.78, 0.349, 1.0],  // green
    [0.0, 0.478, 1.0, 1.0],     // blue
    [0.686, 0.322, 0.871, 1.0], // purple
    [0.0, 0.0, 0.0, 1.0],       // black
    [1.0, 1.0, 1.0, 1.0],       // white
    [0.557, 0.557, 0.576, 1.0], // gray
];
const PALETTE_NAMES: [&str; 9] = ["Red", "Orange", "Mustard", "Green", "Blue", "Purple", "Black", "White", "Gray"];
const SIZES: [f32; 5] = [2.0, 4.0, 8.0, 16.0, 32.0];
const ACCENT: Color32 = Color32::from_rgb(0, 120, 212);
/// The text face, registered with egui so the text box looks like the finished text.
pub const ANNOTATION_FONT: &str = "annotation";

/// Things a window asks the app to do.
pub enum Action {
    NewFromClipboard,
    OpenDialog,
    Capture(Mode),
    Close,
    Quit,
    Prefs(Prefs),
}

pub struct Document {
    pub canvas: Canvas,
    pub title: String,
    /// The file this window was opened from or last saved to; Ctrl+S overwrites it.
    pub path: Option<PathBuf>,
    tiles: Option<Tiles>,
    uploaded: u64,
    tex_name: String,
    zoom: f32,
    fit_pending: bool,
    scroll_to: Option<Vec2>,
    /// The part of the image on screen, in image points.
    visible: Rect,
    last_press: Option<(f64, Pos2)>,
    click_count: u32,
    shown_title: String,
}

fn color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        (c[3] * 255.0).round() as u8,
    )
}

fn rgba(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

fn same(a: Rgba, b: Rgba) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

impl Document {
    pub fn new(image: Pixmap, scale: f32, title: String, path: Option<PathBuf>, prefs: &Prefs, id: u64) -> Self {
        let mut canvas = Canvas::new(image, scale);
        canvas.color = prefs.color;
        canvas.line_width = prefs.width;
        canvas.text_outline = prefs.text_outline;
        Self {
            canvas,
            title,
            path,
            tiles: None,
            uploaded: 0,
            tex_name: format!("doc{id}"),
            zoom: 1.0,
            fit_pending: true,
            scroll_to: None,
            visible: Rect::NOTHING,
            last_press: None,
            click_count: 0,
            shown_title: String::new(),
        }
    }

    fn prefs(&self) -> Prefs {
        Prefs { color: self.canvas.color, width: self.canvas.line_width, text_outline: self.canvas.text_outline }
    }

    pub fn window_title(&self) -> String {
        let (w, h) = self.canvas.pixel_size();
        let mark = if self.canvas.edited { "*" } else { "" };
        format!("{}{mark} \u{2014} {w} \u{00D7} {h}", self.title)
    }

    /// The window size (in points) that shows the whole image, within 90% x 85% of the work area.
    pub fn preferred_size(&self, work_area: Vec2) -> Vec2 {
        let size = self.canvas.size();
        let (max_w, max_h) = (work_area.x * 0.9, work_area.y * 0.85);
        let chrome = 64.0; // menu bar + toolbar
        let scale = 1.0f32.min((max_w - 40.0) / size.x).min((max_h - chrome - 40.0) / size.y);
        vec2(
            (size.x * scale + 40.0).clamp(640.0, max_w.max(640.0)),
            (size.y * scale + chrome + 40.0).clamp(400.0, max_h.max(400.0)),
        )
    }

    // MARK: Frame

    pub fn ui(&mut self, ui: &mut Ui, window: &Window, actions: &mut Vec<Action>) {
        let t = self.window_title();
        if t != self.shown_title {
            window.set_title(&t);
            self.shown_title = t;
        }
        self.menu_bar(ui, window, actions);
        self.toolbar(ui, window, actions);
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(ui.visuals().extreme_bg_color))
            .show(ui, |ui| self.canvas_view(ui, window, actions));
        self.shortcuts(ui, window, actions);
    }

    fn menu_bar(&mut self, ui: &mut Ui, window: &Window, actions: &mut Vec<Action>) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.add(egui::Button::new("New from Clipboard").shortcut_text("Ctrl+N")).clicked() {
                        actions.push(Action::NewFromClipboard);
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("Capture Region").shortcut_text("Ctrl+Alt+R")).clicked() {
                        actions.push(Action::Capture(Mode::RegionToEditor));
                    }
                    if ui.add(egui::Button::new("Capture Window").shortcut_text("Ctrl+Alt+W")).clicked() {
                        actions.push(Action::Capture(Mode::WindowToEditor));
                    }
                    if ui.add(egui::Button::new("Capture Full Screen").shortcut_text("Ctrl+Alt+F")).clicked() {
                        actions.push(Action::Capture(Mode::ScreenToEditor));
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("Open\u{2026}").shortcut_text("Ctrl+O")).clicked() {
                        actions.push(Action::OpenDialog);
                    }
                    if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                        self.save(window);
                    }
                    if ui.add(egui::Button::new("Save As\u{2026}").shortcut_text("Ctrl+Shift+S")).clicked() {
                        self.save_as(window);
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("Close").shortcut_text("Ctrl+W")).clicked() {
                        actions.push(Action::Close);
                    }
                    if ui.add(egui::Button::new("Quit Zquig").shortcut_text("Ctrl+Q")).clicked() {
                        actions.push(Action::Quit);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let c = &mut self.canvas;
                    if ui.add_enabled(c.can_undo(), egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                        c.undo();
                    }
                    if ui.add_enabled(c.can_redo(), egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                        c.redo();
                    }
                    ui.separator();
                    let has_sel = c.selected.is_some();
                    if ui.add_enabled(has_sel, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                        self.cut();
                    }
                    if ui.add(egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                        self.copy();
                    }
                    if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                        self.paste(actions);
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("Copy Whole Image").shortcut_text("Ctrl+Shift+C")).clicked() {
                        self.copy_image();
                    }
                    if ui.add_enabled(self.canvas.selected.is_some(), egui::Button::new("Delete Annotation").shortcut_text("Del")).clicked() {
                        self.canvas.delete_selection();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.add(egui::Button::new("Zoom In").shortcut_text("Ctrl+=")).clicked() {
                        self.zoom_by(1.25, None);
                    }
                    if ui.add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-")).clicked() {
                        self.zoom_by(0.8, None);
                    }
                    if ui.add(egui::Button::new("Actual Size").shortcut_text("Ctrl+1")).clicked() {
                        self.zoom_to(1.0, None);
                    }
                    if ui.add(egui::Button::new("Zoom to Fit").shortcut_text("Ctrl+0")).clicked() {
                        self.fit_pending = true;
                    }
                });
            });
        });
    }

    fn toolbar(&mut self, ui: &mut Ui, window: &Window, actions: &mut Vec<Action>) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for t in Tool::ALL {
                    let on = self.canvas.tool == t;
                    let b = egui::Button::selectable(on, RichText::new(t.icon()).size(18.0)).min_size(vec2(30.0, 28.0));
                    if ui.add(b).on_hover_text(t.title()).clicked() {
                        self.canvas.set_tool(t);
                    }
                }
                ui.add_space(8.0);
                self.style_button(ui, actions);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let drag = ui
                        .add(egui::Button::new(RichText::new(icon::EXPORT).size(18.0)).min_size(vec2(30.0, 28.0)).sense(Sense::click_and_drag()))
                        .on_hover_text("Drag the annotated image into another app");
                    if drag.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grab);
                    }
                    if drag.drag_started() {
                        self.drag_out(ui, window);
                    }
                    let save = egui::Button::new(RichText::new(icon::FLOPPY_DISK).size(18.0)).min_size(vec2(30.0, 28.0));
                    if ui.add(save).on_hover_text("Save (Ctrl+S)").clicked() {
                        self.save(window);
                    }
                    ui.add_space(8.0);
                    let redo = egui::Button::new(RichText::new(icon::ARROW_U_UP_RIGHT).size(18.0)).min_size(vec2(30.0, 28.0));
                    if ui.add_enabled(self.canvas.can_redo(), redo).on_hover_text("Redo").clicked() {
                        self.canvas.redo();
                    }
                    let undo = egui::Button::new(RichText::new(icon::ARROW_U_UP_LEFT).size(18.0)).min_size(vec2(30.0, 28.0));
                    if ui.add_enabled(self.canvas.can_undo(), undo).on_hover_text("Undo").clicked() {
                        self.canvas.undo();
                    }
                });
            });
            ui.add_space(3.0);
        });
    }

    // MARK: Style popover

    fn style_button(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::click());
        let resp = resp.on_hover_text("Colour and thickness");
        let v = ui.style().interact(&resp);
        if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(4), v.weak_bg_fill);
        }
        ui.painter().circle(rect.center(), 8.0, color32(self.canvas.color), Stroke::new(1.0, ui.visuals().weak_text_color()));
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.style_panel(ui, actions));
    }

    fn style_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.set_min_width(250.0);
        let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).size(11.0).strong().color(ui.visuals().weak_text_color()));
        label(ui, "COLOUR");
        let mut picked = None;
        for row in PALETTE.chunks(5).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (j, &c) in row.1.iter().enumerate() {
                    let i = row.0 * 5 + j;
                    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    let on = same(c, self.canvas.color);
                    ui.painter().circle_filled(r.center(), 11.0, color32(c));
                    let ring = if on { Stroke::new(3.0, ACCENT) } else { Stroke::new(1.0, ui.visuals().weak_text_color()) };
                    ui.painter().circle_stroke(r.center(), 11.0, ring);
                    if resp.on_hover_text(PALETTE_NAMES[i]).clicked() {
                        picked = Some(c);
                    }
                }
            });
        }
        let mut custom = color32(self.canvas.color);
        ui.horizontal(|ui| {
            if egui::color_picker::color_edit_button_srgba(ui, &mut custom, egui::color_picker::Alpha::Opaque).changed() {
                picked = Some(rgba(custom));
            }
            ui.label("Custom");
        });
        if let Some(c) = picked {
            self.canvas.apply_color(c);
            self.canvas.end_style_change();
            actions.push(Action::Prefs(self.prefs()));
        }
        ui.add_space(6.0);
        label(ui, "THICKNESS");
        let nearest = SIZES
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - self.canvas.line_width).abs().total_cmp(&(b.1 - self.canvas.line_width).abs()))
            .map_or(1, |(i, _)| i);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, &w) in SIZES.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
                let on = i == nearest;
                let fill = if on { ui.visuals().selection.bg_fill } else if resp.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { ui.visuals().widgets.inactive.weak_bg_fill };
                ui.painter().rect_filled(r, CornerRadius::same(4), fill);
                // log scale keeps the biggest dot inside the button
                let d = 2.0 + 2.8 * w.log2();
                ui.painter().circle_filled(r.center(), d / 2.0, ui.visuals().strong_text_color());
                if resp.on_hover_text(format!("{w} pt")).clicked() {
                    self.canvas.apply_width(w);
                    self.canvas.end_style_change();
                    actions.push(Action::Prefs(self.prefs()));
                }
            }
        });
        ui.add_space(6.0);
        label(ui, "TEXT");
        let mut outline = self.canvas.text_outline;
        if ui.checkbox(&mut outline, "Text border & shadow").changed() {
            self.canvas.apply_outline(outline);
            self.canvas.end_style_change();
            actions.push(Action::Prefs(self.prefs()));
        }
    }

    // MARK: Canvas

    fn zoom_to(&mut self, z: f32, anchor: Option<Pos2>) {
        let z = z.clamp(0.05, 8.0);
        // Keep the image point under the anchor (or the view centre) where it is.
        let anchor_img = anchor.unwrap_or(self.visible.center());
        let rel = (anchor_img - self.visible.min) / self.visible.size().max(vec2(1.0, 1.0));
        let view_pts = self.visible.size() * self.zoom; // viewport size in screen points
        let new_min = anchor_img - rel * view_pts / z;
        self.zoom = z;
        self.scroll_to = Some((new_min.to_vec2() * z).max(Vec2::ZERO));
    }

    fn zoom_by(&mut self, f: f32, anchor: Option<Pos2>) {
        self.zoom_to(self.zoom * f, anchor);
    }

    fn canvas_view(&mut self, ui: &mut Ui, window: &Window, actions: &mut Vec<Action>) {
        if self.uploaded != self.canvas.revision {
            let pix = self.canvas.render_view();
            match &mut self.tiles {
                Some(t) => t.update(ui.ctx(), &self.tex_name, &pix),
                None => self.tiles = Some(Tiles::new(ui.ctx(), &self.tex_name, &pix)),
            }
            self.uploaded = self.canvas.revision;
        }

        let avail = ui.available_size();
        let size = self.canvas.size();
        if self.fit_pending && avail.x > 0.0 {
            self.zoom = 1.0f32.min((avail.x - 16.0) / size.x).min((avail.y - 16.0) / size.y).max(0.05);
            self.fit_pending = false;
            self.scroll_to = Some(Vec2::ZERO);
        }

        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some(off) = self.scroll_to.take() {
            area = area.scroll_offset(off);
        }
        let zoom = self.zoom;
        area.show_viewport(ui, |ui, viewport| {
            let img = size * zoom;
            let content = img.max(viewport.size());
            let (rect, resp) = ui.allocate_exact_size(content, Sense::click_and_drag());
            let img_rect = Rect::from_min_size(rect.min + ((content - img) / 2.0).max(Vec2::ZERO), img);
            let to_img = |p: Pos2| ((p - img_rect.min) / zoom).to_pos2();
            let to_screen = |p: Pos2| img_rect.min + p.to_vec2() * zoom;
            let screen_view = Rect::from_min_size(rect.min + viewport.min.to_vec2(), viewport.size());
            self.visible = Rect::from_min_max(to_img(screen_view.min), to_img(screen_view.max));

            // Ctrl + wheel zooms around the pointer.
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if zoom_delta != 1.0 && resp.contains_pointer() {
                let anchor = ui.input(|i| i.pointer.hover_pos()).map(to_img);
                self.zoom_by(zoom_delta, anchor);
            }

            let painter = ui.painter_at(screen_view);
            if let Some(t) = &self.tiles {
                t.paint(&painter, img_rect);
            }
            self.pointer(ui, &resp, img_rect, zoom);
            self.selection_ui(&painter, to_screen, img_rect);
            self.text_editor(ui, to_screen, zoom);
            if resp.hovered() && img_rect.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
                let cursor = match self.canvas.tool {
                    Tool::Select | Tool::Stamp => CursorIcon::Default,
                    Tool::Text => CursorIcon::Text,
                    _ => CursorIcon::Crosshair,
                };
                ui.ctx().set_cursor_icon(cursor);
            }
        });

        // Files dropped on the window replace the image (handled by the app); nothing else to do here.
        let _ = (window, actions);
    }

    fn pointer(&mut self, ui: &Ui, resp: &egui::Response, img_rect: Rect, zoom: f32) {
        let (pressed, released, pos, mods, time) = ui.input(|i| {
            (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.interact_pos(), i.modifiers, i.time)
        });
        let Some(pos) = pos else { return };
        let p = ((pos - img_rect.min) / zoom).to_pos2();
        if pressed && resp.contains_pointer() && img_rect.contains(pos) {
            self.click_count = match self.last_press {
                Some((t, q)) if time - t < 0.5 && q.distance(pos) < 4.0 => self.click_count + 1,
                _ => 1,
            };
            self.last_press = Some((time, pos));
            ui.ctx().memory_mut(|m| m.stop_text_input());
            self.canvas.mouse_down(p, self.click_count, mods.alt, zoom);
        } else if self.canvas.is_dragging() {
            self.canvas.mouse_dragged(p, mods.shift);
        }
        if released && self.canvas.is_dragging() {
            self.canvas.mouse_up();
        }
    }

    fn selection_ui(&self, painter: &egui::Painter, to_screen: impl Fn(Pos2) -> Pos2, img_rect: Rect) {
        let c = &self.canvas;
        if let Some(i) = c.selected
            && let Some(a) = c.annotations.get(i)
            && c.editor.as_ref().and_then(|e| e.index) != Some(i)
        {
            let b = a.bounds();
            let r = Rect::from_min_max(to_screen(b.min), to_screen(b.max)).expand(4.0);
            let path = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&path, Stroke::new(1.0, ACCENT), 4.0, 3.0));
            for h in a.handles() {
                let s = to_screen(h);
                painter.circle(s, 4.5, Color32::WHITE, Stroke::new(1.5, ACCENT));
            }
        }
        if let Some(cr) = c.crop_rect {
            let r = Rect::from_min_max(to_screen(cr.min), to_screen(cr.max));
            let dim = Color32::from_black_alpha(115);
            for part in [
                Rect::from_min_max(img_rect.min, pos2(img_rect.max.x, r.min.y)),
                Rect::from_min_max(pos2(img_rect.min.x, r.max.y), img_rect.max),
                Rect::from_min_max(pos2(img_rect.min.x, r.min.y), pos2(r.min.x, r.max.y)),
                Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(img_rect.max.x, r.max.y)),
            ] {
                if part.is_positive() {
                    painter.rect_filled(part, 0.0, dim);
                }
            }
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Middle);
        }
    }

    /// The open text box: Return adds a line; Ctrl+Return, Esc or clicking elsewhere finishes.
    fn text_editor(&mut self, ui: &mut Ui, to_screen: impl Fn(Pos2) -> Pos2, zoom: f32) {
        let Some(ed) = &mut self.canvas.editor else { return };
        let finish = ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Enter));
        let font = FontId::new(font_size(ed.width) * zoom, FontFamily::Name(ANNOTATION_FONT.into()));
        let pos = to_screen(ed.origin);
        let width = ed.wrap * zoom;
        let out = egui::Area::new(egui::Id::new(("text-editor", ed.origin.x.to_bits(), ed.origin.y.to_bits())))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut ed.text)
                        .font(font)
                        .text_color(color32(ed.color))
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .desired_width(width)
                        .desired_rows(1)
                        .hint_text("Type\u{2026}"),
                )
            });
        let resp = out.inner;
        // Focus only once the click that opened the box is over; egui treats that click's
        // release as a click elsewhere and would take the focus straight back.
        let click_in_progress = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
        if ed.needs_focus {
            if !click_in_progress {
                resp.request_focus();
                ed.needs_focus = false;
            }
        } else if finish || resp.lost_focus() {
            self.canvas.end_text_editing(true);
        }
    }

    // MARK: Keyboard

    fn shortcuts(&mut self, ui: &mut Ui, window: &Window, actions: &mut Vec<Action>) {
        if ui.ctx().egui_wants_keyboard_input() {
            return; // typing in the text box (or another field)
        }
        let events = ui.input(|i| i.events.clone());
        let shift_held = ui.input(|i| i.modifiers.shift);
        for e in events {
            match e {
                Event::Copy => {
                    if shift_held {
                        self.copy_image()
                    } else {
                        self.copy()
                    }
                }
                Event::Cut => self.cut(),
                Event::Paste(_) => self.paste(actions),
                Event::Key { key, pressed: true, modifiers: m, .. } => self.key(key, m, window, actions),
                _ => {}
            }
        }
    }

    fn key(&mut self, key: Key, m: Modifiers, window: &Window, actions: &mut Vec<Action>) {
        let c = &mut self.canvas;
        if m.ctrl && m.alt {
            match key {
                Key::R => actions.push(Action::Capture(Mode::RegionToEditor)),
                Key::W => actions.push(Action::Capture(Mode::WindowToEditor)),
                Key::F => actions.push(Action::Capture(Mode::ScreenToEditor)),
                _ => {}
            }
            return;
        }
        if m.ctrl {
            match key {
                Key::N => actions.push(Action::NewFromClipboard),
                Key::O => actions.push(Action::OpenDialog),
                Key::S if m.shift => _ = self.save_as(window),
                Key::S => _ = self.save(window),
                Key::W => actions.push(Action::Close),
                Key::Q => actions.push(Action::Quit),
                Key::Z if m.shift => c.redo(),
                Key::Z => c.undo(),
                Key::Y => c.redo(),
                Key::Equals | Key::Plus => self.zoom_by(1.25, None),
                Key::Minus => self.zoom_by(0.8, None),
                Key::Num0 => self.fit_pending = true,
                Key::Num1 => self.zoom_to(1.0, None),
                _ => {}
            }
            return;
        }
        if m.alt {
            return;
        }
        let step = if m.shift { 10.0 } else { 1.0 };
        match key {
            Key::A if !m.shift => c.set_tool(Tool::Select),
            Key::T if !m.shift => c.set_tool(Tool::Text),
            Key::Q if !m.shift => c.set_tool(Tool::Arrow),
            Key::Delete | Key::Backspace => c.delete_selection(),
            Key::Escape => c.select(None),
            Key::ArrowLeft => _ = c.nudge(vec2(-step, 0.0)),
            Key::ArrowRight => _ = c.nudge(vec2(step, 0.0)),
            Key::ArrowUp => _ = c.nudge(vec2(0.0, -step)),
            Key::ArrowDown => _ = c.nudge(vec2(0.0, step)),
            _ => {}
        }
    }

    // MARK: Clipboard

    fn annotation_format() -> Option<u32> {
        clipboard_win::raw::register_format("Zquig.Annotation").map(|f| f.get())
    }

    /// Ctrl+C: copy the selected object; with nothing selected, copy the whole annotated image.
    fn copy(&mut self) {
        let Some(a) = self.canvas.selected_annotation().cloned() else { return self.copy_image() };
        let Some(fmt) = Self::annotation_format() else { return };
        if let Ok(_clip) = clipboard_win::Clipboard::new_attempts(10) {
            let _ = clipboard_win::raw::empty();
            let _ = clipboard_win::raw::set_without_clear(fmt, a.to_json().as_bytes());
            if a.kind == Tool::Text {
                let _ = clipboard_win::raw::set_string_with(&a.text, clipboard_win::options::NoClear);
            }
        }
        self.canvas.reset_paste_offset();
    }

    fn cut(&mut self) {
        if self.canvas.selected.is_some() {
            self.copy();
            self.canvas.delete_selection();
        }
    }

    /// Ctrl+V: paste a copied object, or text as a new text object, or open a clipboard image in a new window.
    fn paste(&mut self, actions: &mut Vec<Action>) {
        if let Some(fmt) = Self::annotation_format()
            && clipboard_win::raw::is_format_avail(fmt)
        {
            let mut buf = Vec::new();
            let got = clipboard_win::Clipboard::new_attempts(10).is_ok_and(|_c| clipboard_win::raw::get_vec(fmt, &mut buf).is_ok());
            if got && let Some(a) = std::str::from_utf8(&buf).ok().and_then(|s| Annotation::from_json(s.trim_end_matches('\0'))) {
                self.canvas.paste_annotation(a);
                return;
            }
        }
        if imageio::clipboard_has_image() {
            actions.push(Action::NewFromClipboard);
        } else if let Some(s) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).filter(|s| !s.is_empty()) {
            self.canvas.paste_text(s, self.visible);
        } else {
            beep();
        }
    }

    fn copy_image(&mut self) {
        let pix = self.canvas.export();
        if let Err(e) = imageio::copy_image(&pix) {
            error_box(None, "Couldn't copy the image", &e);
        }
    }

    // MARK: Drag out

    /// Drag the annotated image (as a PNG file) into Explorer, mail, chat apps, etc.
    fn drag_out(&mut self, ui: &Ui, window: &Window) {
        let png = imageio::encode_png(&self.canvas.export());
        let dir = std::env::temp_dir().join(format!("zquig-drag-{}", std::process::id() as u64 * 1_000_000 + ui.input(|i| (i.time * 1000.0) as u64)));
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(format!("{}.png", sanitize(&self.title)));
        if std::fs::write(&path, &png).is_err() {
            return;
        }
        let thumb = thumbnail_png(&self.canvas.flattened(), 96);
        let _ = drag::start_drag(window, drag::DragItem::Files(vec![path]), drag::Image::Raw(thumb), |_, _| {}, drag::Options::default());
        // The receiver may read the file lazily; tidy up a little later.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    // MARK: Saving

    /// Ctrl+S: overwrite the original file. Windows with no file yet (captures, clipboard) and PDFs
    /// (a multi-page PDF can't be rewritten from one flattened image) fall back to Save As.
    pub fn save(&mut self, window: &Window) -> bool {
        match self.path.clone() {
            Some(p) if !imageio::is_pdf(&p) => self.write(window, &p),
            _ => self.save_as(window),
        }
    }

    pub fn save_as(&mut self, window: &Window) -> bool {
        let dir = self.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(crate::capture::screenshots_dir);
        let picked = rfd::FileDialog::new()
            .set_parent(window)
            .set_directory(dir)
            .set_file_name(format!("{}.png", sanitize(&self.title)))
            .add_filter("PNG image", &["png"])
            .add_filter("JPEG image", &["jpg", "jpeg"])
            .add_filter("TIFF image", &["tif", "tiff"])
            .add_filter("PDF document", &["pdf"])
            .save_file();
        let Some(mut path) = picked else { return false };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        if !self.write(window, &path) {
            return false;
        }
        self.title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.path = Some(path);
        true
    }

    fn write(&mut self, window: &Window, path: &Path) -> bool {
        let pix = self.canvas.export();
        match imageio::save(&pix, self.canvas.scale, path) {
            Ok(()) => {
                self.canvas.edited = false;
                true
            }
            Err(e) => {
                error_box(Some(window), &format!("Couldn't save \u{201C}{}\u{201D}", path.display()), &e);
                false
            }
        }
    }

    /// Closing with unsaved edits asks first; a clean window closes silently.
    pub fn confirm_close(&mut self, window: &Window) -> bool {
        self.canvas.end_text_editing(true);
        if !self.canvas.edited {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_parent(window)
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Zquig")
            .set_description(format!(
                "Do you want to save the changes made to \u{201C}{}\u{201D}?\n\nYour changes will be lost if you don\u{2019}t save them.",
                self.title
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom("Save".into(), "Don\u{2019}t Save".into(), "Cancel".into()))
            .show();
        match answer {
            rfd::MessageDialogResult::Custom(s) if s == "Save" => self.save(window),
            rfd::MessageDialogResult::Custom(s) if s.starts_with("Don") => true,
            rfd::MessageDialogResult::Yes => self.save(window),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    pub fn replace_image(&mut self, image: Pixmap, scale: f32) {
        self.canvas.set_image(image, scale);
        self.fit_pending = true;
    }
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c }).collect();
    if s.trim().is_empty() { "Zquig".into() } else { s }
}

fn thumbnail_png(pix: &Pixmap, max: u32) -> Vec<u8> {
    let img = imageio::rgba_from_pixmap(pix);
    let s = (max as f32 / img.width().max(img.height()) as f32).min(1.0);
    let t = image::imageops::thumbnail(&img, ((img.width() as f32 * s) as u32).max(1), ((img.height() as f32 * s) as u32).max(1));
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = t.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

pub fn beep() {
    unsafe {
        let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(windows::Win32::UI::WindowsAndMessaging::MB_OK);
    }
}

pub fn error_box(parent: Option<&Window>, title: &str, detail: &str) {
    let mut d = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Zquig").set_description(format!("{title}\n\n{detail}"));
    if let Some(w) = parent {
        d = d.set_parent(w);
    }
    d.show();
}
