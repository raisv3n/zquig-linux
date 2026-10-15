//! The document being annotated: base image, annotations, undo history and mouse/keyboard behaviour.

use std::sync::Arc;

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use tiny_skia::{IntRect, Pixmap};

use crate::annotation::{Annotation, Rgba, Tool};
use crate::render;

#[derive(Clone)]
struct Snapshot {
    base: Arc<Pixmap>,
    scale: f32,
    annotations: Vec<Annotation>,
    stamp: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Create,
    Move,
    Resize(usize),
}

/// An open text box.
pub struct TextEditor {
    pub origin: Pos2,
    pub text: String,
    /// The annotation being edited, or None for a new one.
    pub index: Option<usize>,
    pub color: Rgba,
    pub width: f32,
    pub wrap: f32,
    pub needs_focus: bool,
}

pub struct Canvas {
    pub base: Arc<Pixmap>,
    /// Pixels per point. Captures use the monitor's scale so strokes look the same size as on screen.
    pub scale: f32,
    pub annotations: Vec<Annotation>,
    pub tool: Tool,
    pub color: Rgba,
    pub line_width: f32,
    pub text_outline: bool,
    pub current: Option<Annotation>,
    pub selected: Option<usize>,
    pub crop_rect: Option<Rect>,
    pub editor: Option<TextEditor>,
    /// Bumped on every visible change, so the window knows when to re-render.
    pub revision: u64,
    /// Set whenever the document content changes (cleared by saving).
    pub edited: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag: Drag,
    did_commit: bool,
    last_point: Pos2,
    next_stamp: i32,
    did_style_commit: bool,
    paste_count: i32,
}

impl Canvas {
    pub fn new(base: Pixmap, scale: f32) -> Self {
        Self {
            base: Arc::new(base),
            scale,
            annotations: Vec::new(),
            tool: Tool::Select,
            color: [1.0, 0.0, 0.0, 1.0],
            line_width: 4.0,
            text_outline: true,
            current: None,
            selected: None,
            crop_rect: None,
            editor: None,
            revision: 1,
            edited: false,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: Drag::None,
            did_commit: false,
            last_point: Pos2::ZERO,
            next_stamp: 1,
            did_style_commit: false,
            paste_count: 0,
        }
    }

    /// Image size in points.
    pub fn size(&self) -> Vec2 {
        vec2(self.base.width() as f32 / self.scale, self.base.height() as f32 / self.scale)
    }

    pub fn pixel_size(&self) -> (u32, u32) {
        (self.base.width(), self.base.height())
    }

    fn bounds(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.size())
    }

    fn touch(&mut self) {
        self.revision += 1;
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.end_text_editing(false);
        self.tool = t;
        self.touch();
    }

    // MARK: Rendering

    fn prepare_text(&mut self) {
        let w = self.size().x;
        for a in self.annotations.iter_mut().filter(|a| a.kind == Tool::Text) {
            a.wrap_limit = a.custom_width.unwrap_or_else(|| wrap_width(w, a.a.x));
        }
    }

    /// What the window shows: everything except the annotation in the text editor, plus the shape being drawn.
    pub fn render_view(&mut self) -> Pixmap {
        self.prepare_text();
        let extra = self.current.as_ref().filter(|c| c.kind != Tool::Crop);
        render::composite(&self.base, self.scale, &self.annotations, self.editor.as_ref().and_then(|e| e.index), extra)
    }

    /// The finished image: base plus every annotation, no selection or crop UI.
    pub fn flattened(&mut self) -> Pixmap {
        self.prepare_text();
        render::composite(&self.base, self.scale, &self.annotations, None, None)
    }

    /// Commits any open text box first, then flattens.
    pub fn export(&mut self) -> Pixmap {
        self.end_text_editing(true);
        self.flattened()
    }

    // MARK: Undo

    fn snapshot(&self) -> Snapshot {
        Snapshot { base: self.base.clone(), scale: self.scale, annotations: self.annotations.clone(), stamp: self.next_stamp }
    }

    fn restore(&mut self, s: Snapshot) {
        self.end_text_editing(true);
        self.base = s.base;
        self.scale = s.scale;
        self.annotations = s.annotations;
        self.next_stamp = s.stamp;
        self.select(None);
        self.crop_rect = None;
        self.edited = true;
        self.touch();
    }

    fn commit(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.edited = true;
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn delete_selection(&mut self) {
        let Some(i) = self.selected.filter(|&i| i < self.annotations.len()) else { return };
        self.commit();
        self.annotations.remove(i);
        self.select(None);
        self.touch();
    }

    // MARK: Clipboard (the window does the clipboard I/O)

    pub fn selected_annotation(&self) -> Option<&Annotation> {
        self.selected.and_then(|i| self.annotations.get(i))
    }

    pub fn reset_paste_offset(&mut self) {
        self.paste_count = 0;
    }

    /// Pastes a copied object, offset a little each time.
    pub fn paste_annotation(&mut self, mut a: Annotation) {
        self.end_text_editing(true);
        self.paste_count += 1;
        let off = 14.0 * self.paste_count as f32;
        a.translate(vec2(off, off));
        if a.kind == Tool::Stamp {
            a.number = self.next_stamp;
            self.next_stamp += 1;
        }
        self.commit();
        self.annotations.push(a);
        self.select(Some(self.annotations.len() - 1));
        self.touch();
    }

    /// Pastes text as a new text object near the top-left of what's visible.
    pub fn paste_text(&mut self, s: String, visible: Rect) {
        self.end_text_editing(true);
        let o = pos2(visible.min.x.max(0.0) + 24.0, visible.min.y.max(0.0) + 24.0);
        self.commit();
        let mut a = Annotation::new(Tool::Text, o, o, self.color, self.line_width);
        a.text = s.chars().take(crate::annotation::MAX_TEXT).collect();
        a.outline = self.text_outline;
        self.annotations.push(a);
        self.select(Some(self.annotations.len() - 1));
        self.touch();
    }

    // MARK: Selection and styling

    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i;
        self.did_style_commit = false;
        if let Some(a) = i.and_then(|i| self.annotations.get(i)) {
            self.color = a.color;
            self.line_width = a.width;
            if a.kind == Tool::Text {
                self.text_outline = a.outline;
            }
        }
        self.touch();
    }

    fn style_target(&mut self) -> Option<usize> {
        let i = self.selected.filter(|&i| i < self.annotations.len())?;
        if !self.did_style_commit {
            self.commit();
            self.did_style_commit = true;
        }
        Some(i)
    }

    /// Applies a style change to the selected annotation (and remembers it for new ones).
    pub fn apply_color(&mut self, c: Rgba) {
        self.color = c;
        if let Some(e) = &mut self.editor {
            e.color = c;
        }
        if let Some(i) = self.style_target() {
            self.annotations[i].color = c;
            self.touch();
        }
    }

    pub fn apply_width(&mut self, w: f32) {
        self.line_width = w;
        if let Some(e) = &mut self.editor {
            e.width = w;
        }
        if let Some(i) = self.style_target() {
            self.annotations[i].width = w;
            self.touch();
        }
    }

    pub fn apply_outline(&mut self, on: bool) {
        self.text_outline = on;
        if self.selected_annotation().is_some_and(|a| a.kind == Tool::Text)
            && let Some(i) = self.style_target()
        {
            self.annotations[i].outline = on;
            self.touch();
        }
    }

    pub fn end_style_change(&mut self) {
        self.did_style_commit = false;
    }

    /// Arrow keys: 1 point, or 10 with Shift.
    pub fn nudge(&mut self, d: Vec2) -> bool {
        let Some(i) = self.selected.filter(|&i| i < self.annotations.len()) else { return false };
        self.commit();
        self.annotations[i].translate(d);
        self.touch();
        true
    }

    // MARK: Mouse

    const HANDLE_SIZE: f32 = 9.0;

    fn handle_index(&self, p: Pos2, zoom: f32) -> Option<usize> {
        let a = self.selected_annotation()?;
        a.handles().iter().position(|h| h.distance(p) <= Self::HANDLE_SIZE / zoom)
    }

    /// `force_new` is Alt-drag: draw a new shape even on top of an existing one.
    pub fn mouse_down(&mut self, p: Pos2, click_count: u32, force_new: bool, zoom: f32) {
        // Finishing text returns to the Select tool; this click then acts as a Select click.
        self.end_text_editing(true);
        self.last_point = p;
        self.did_commit = false;
        self.drag = Drag::None;

        if !force_new && self.tool != Tool::Crop {
            if let Some(h) = self.handle_index(p, zoom) {
                self.drag = Drag::Resize(h);
                return;
            }
            if let Some(i) = self.annotations.iter().rposition(|a| a.hit_test(p)) {
                self.select(Some(i));
                if self.annotations[i].kind == Tool::Text && (click_count == 2 || self.tool == Tool::Text) {
                    self.begin_editing_index(i);
                } else {
                    self.drag = Drag::Move;
                }
                return;
            }
        }
        self.select(None);
        if click_count == 2 && self.tool != Tool::Crop {
            self.tool = Tool::Text;
            self.begin_editing_at(p);
            return;
        }
        let (c, w) = (self.color, self.line_width);
        match self.tool {
            Tool::Select => {}
            Tool::Text => self.begin_editing_at(p),
            Tool::Stamp => {
                self.commit();
                let mut a = Annotation::new(Tool::Stamp, p, p, c, w);
                a.number = self.next_stamp;
                self.next_stamp += 1;
                self.annotations.push(a);
                self.select(Some(self.annotations.len() - 1));
            }
            Tool::Crop => {
                self.crop_rect = Some(Rect::from_min_max(p, p));
                self.current = Some(Annotation::new(Tool::Crop, p, p, c, w));
                self.drag = Drag::Create;
            }
            Tool::Pen => {
                let mut a = Annotation::new(Tool::Pen, p, p, c, w);
                a.points.push(p);
                self.current = Some(a);
                self.drag = Drag::Create;
            }
            t => {
                self.current = Some(Annotation::new(t, p, p, c, w));
                self.drag = Drag::Create;
            }
        }
        self.touch();
    }

    pub fn mouse_dragged(&mut self, p: Pos2, shift: bool) {
        if p == self.last_point {
            return;
        }
        match self.drag {
            Drag::None => return,
            Drag::Move => {
                let Some(i) = self.selected else { return };
                if !self.did_commit {
                    self.commit();
                    self.did_commit = true;
                }
                self.annotations[i].translate(p - self.last_point);
            }
            Drag::Resize(h) => {
                let Some(i) = self.selected else { return };
                if !self.did_commit {
                    self.commit();
                    self.did_commit = true;
                }
                let a = &mut self.annotations[i];
                let q = if shift && matches!(a.kind, Tool::Line | Tool::Arrow) {
                    snapped(p, a.handles()[if h == 0 { 1 } else { 0 }])
                } else {
                    p
                };
                a.move_handle(h, q);
            }
            Drag::Create => {
                let bounds = self.bounds();
                let Some(c) = &mut self.current else { return };
                c.b = p;
                if c.kind == Tool::Pen {
                    c.points.push(p);
                }
                if c.kind == Tool::Crop {
                    self.crop_rect = Some(c.rect().intersect(bounds));
                }
                if shift && matches!(c.kind, Tool::Line | Tool::Arrow) {
                    c.b = snapped(p, c.a);
                }
            }
        }
        self.last_point = p;
        self.touch();
    }

    pub fn mouse_up(&mut self) {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        let current = self.current.take();
        self.touch();
        if drag != Drag::Create {
            return;
        }
        let Some(c) = current else { return };
        if c.kind == Tool::Crop {
            if let Some(r) = self.crop_rect.take()
                && r.width() > 4.0
                && r.height() > 4.0
            {
                self.apply_crop(r);
            }
            return;
        }
        let moved = c.a.distance(c.b);
        if !(moved > 3.0 || (c.kind == Tool::Pen && c.points.len() > 1)) {
            return;
        }
        self.commit();
        self.annotations.push(c);
        self.select(Some(self.annotations.len() - 1));
    }

    pub fn is_dragging(&self) -> bool {
        self.drag != Drag::None
    }

    fn apply_crop(&mut self, r: Rect) {
        let flat = self.flattened();
        let s = self.scale;
        let px = IntRect::from_ltrb(
            (r.min.x * s).round() as i32,
            (r.min.y * s).round() as i32,
            (r.max.x * s).round() as i32,
            (r.max.y * s).round() as i32,
        );
        let Some(cut) = px.and_then(|px| flat.clone_rect(px)) else { return };
        self.commit();
        self.base = Arc::new(cut);
        self.annotations.clear();
        self.next_stamp = 1;
        self.select(None);
    }

    // MARK: Text (new and existing)

    fn begin_editing_at(&mut self, p: Pos2) {
        let wrap = wrap_width(self.size().x, p.x);
        self.editor = Some(TextEditor {
            origin: p,
            text: String::new(),
            index: None,
            color: self.color,
            width: self.line_width,
            wrap,
            needs_focus: true,
        });
        self.touch();
    }

    fn begin_editing_index(&mut self, i: usize) {
        let a = &self.annotations[i];
        let wrap = a.custom_width.unwrap_or_else(|| wrap_width(self.size().x, a.a.x));
        self.editor = Some(TextEditor {
            origin: a.a,
            text: a.text.clone(),
            index: Some(i),
            color: a.color,
            width: a.width,
            wrap,
            needs_focus: true,
        });
        self.touch();
    }

    /// Commits the open text box. Unless the caller is itself changing tools, finishing text
    /// drops back to the Select tool so the next click doesn't open another text box.
    pub fn end_text_editing(&mut self, return_to_select: bool) {
        let Some(ed) = self.editor.take() else { return };
        if return_to_select && self.tool == Tool::Text {
            self.tool = Tool::Select;
        }
        self.touch();
        let s = ed.text;
        if let Some(i) = ed.index.filter(|&i| i < self.annotations.len()) {
            if s == self.annotations[i].text {
                return;
            }
            self.commit();
            if s.is_empty() {
                self.annotations.remove(i);
                self.select(None);
            } else {
                self.annotations[i].text = s;
            }
        } else if !s.is_empty() {
            self.commit();
            let mut a = Annotation::new(Tool::Text, ed.origin, ed.origin, self.color, self.line_width);
            a.text = s;
            a.outline = self.text_outline;
            self.annotations.push(a);
            self.select(Some(self.annotations.len() - 1));
        }
    }

    // MARK: Image replacement and drag-in

    pub fn set_image(&mut self, image: Pixmap, scale: f32) {
        self.end_text_editing(true);
        self.commit();
        self.base = Arc::new(image);
        self.scale = scale;
        self.annotations.clear();
        self.next_stamp = 1;
        self.select(None);
    }
}

fn wrap_width(image_width: f32, x: f32) -> f32 {
    (image_width - x - 8.0).max(40.0)
}

fn snapped(p: Pos2, a: Pos2) -> Pos2 {
    let d = p - a;
    if d.x.abs() > d.y.abs() * 2.0 {
        return pos2(p.x, a.y);
    }
    if d.y.abs() > d.x.abs() * 2.0 {
        return pos2(a.x, p.y);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Canvas {
        Canvas::new(crate::imageio::blank(400, 300), 1.0)
    }

    fn draw(c: &mut Canvas, tool: Tool, a: Pos2, b: Pos2) {
        c.set_tool(tool);
        c.mouse_down(a, 1, false, 1.0);
        c.mouse_dragged(b, false);
        c.mouse_up();
    }

    #[test]
    fn draw_select_move_undo_redo() {
        let mut c = canvas();
        draw(&mut c, Tool::Rect, pos2(10.0, 10.0), pos2(110.0, 60.0));
        assert_eq!(c.annotations.len(), 1);
        assert_eq!(c.selected, Some(0));
        assert!(c.edited);

        // Drag the edge of the selected rectangle to move it.
        c.set_tool(Tool::Select);
        c.mouse_down(pos2(10.0, 30.0), 1, false, 1.0);
        c.mouse_dragged(pos2(30.0, 50.0), false);
        c.mouse_up();
        assert_eq!(c.annotations[0].a, pos2(30.0, 30.0));

        c.undo();
        assert_eq!(c.annotations[0].a, pos2(10.0, 10.0));
        c.undo();
        assert!(c.annotations.is_empty());
        c.redo();
        c.redo();
        assert_eq!(c.annotations[0].a, pos2(30.0, 30.0));
    }

    #[test]
    fn tiny_drags_and_clicks_dont_create_shapes() {
        let mut c = canvas();
        draw(&mut c, Tool::Arrow, pos2(10.0, 10.0), pos2(11.0, 11.0));
        assert!(c.annotations.is_empty());
        assert!(!c.can_undo());
    }

    #[test]
    fn stamps_count_up_and_reset_after_crop() {
        let mut c = canvas();
        c.set_tool(Tool::Stamp);
        for x in [50.0, 100.0, 150.0] {
            c.mouse_down(pos2(x, 50.0), 1, true, 1.0);
            c.mouse_up();
        }
        let numbers: Vec<i32> = c.annotations.iter().map(|a| a.number).collect();
        assert_eq!(numbers, [1, 2, 3]);

        draw(&mut c, Tool::Crop, pos2(20.0, 20.0), pos2(220.0, 120.0));
        assert_eq!(c.pixel_size(), (200, 100));
        assert!(c.annotations.is_empty(), "crop flattens annotations into the image");
        c.set_tool(Tool::Stamp);
        c.mouse_down(pos2(10.0, 10.0), 1, false, 1.0);
        c.mouse_up();
        assert_eq!(c.annotations[0].number, 1);

        c.undo();
        c.undo();
        assert_eq!(c.pixel_size(), (400, 300));
        assert_eq!(c.annotations.len(), 3);
    }

    #[test]
    fn text_editing_lifecycle() {
        let mut c = canvas();
        c.set_tool(Tool::Text);
        c.mouse_down(pos2(40.0, 40.0), 1, false, 1.0);
        c.mouse_up();
        c.editor.as_mut().unwrap().text = "Hello".into();
        c.end_text_editing(true);
        assert_eq!(c.tool, Tool::Select, "finishing text returns to Select");
        assert_eq!(c.annotations[0].text, "Hello");

        // Double-click edits it; clearing the text deletes it.
        c.mouse_down(pos2(45.0, 45.0), 2, false, 1.0);
        c.mouse_up();
        assert_eq!(c.editor.as_ref().unwrap().index, Some(0));
        c.editor.as_mut().unwrap().text.clear();
        c.end_text_editing(true);
        assert!(c.annotations.is_empty());

        // An empty new text box leaves nothing behind and no undo step.
        c.set_tool(Tool::Text);
        let steps = c.undo.len();
        c.mouse_down(pos2(200.0, 200.0), 1, false, 1.0);
        c.mouse_up();
        c.end_text_editing(true);
        assert!(c.annotations.is_empty());
        assert_eq!(c.undo.len(), steps);
    }

    #[test]
    fn paste_offsets_and_renumbers() {
        let mut c = canvas();
        draw(&mut c, Tool::Line, pos2(10.0, 10.0), pos2(60.0, 10.0));
        let copied = Annotation::from_json(&c.annotations[0].to_json()).unwrap();
        c.paste_annotation(copied.clone());
        c.paste_annotation(copied);
        assert_eq!(c.annotations[1].a, pos2(24.0, 24.0));
        assert_eq!(c.annotations[2].a, pos2(38.0, 38.0));
        assert_eq!(c.selected, Some(2));
    }

    #[test]
    fn style_change_is_one_undo_step() {
        let mut c = canvas();
        draw(&mut c, Tool::Ellipse, pos2(10.0, 10.0), pos2(80.0, 80.0));
        let steps = c.undo.len();
        c.apply_color([0.0, 0.0, 1.0, 1.0]);
        c.apply_width(16.0);
        c.end_style_change();
        assert_eq!(c.undo.len(), steps + 1);
        assert_eq!(c.annotations[0].width, 16.0);
        c.undo();
        assert_eq!(c.annotations[0].width, 4.0);
    }

    #[test]
    fn shift_snaps_lines_and_handles_resize() {
        let mut c = canvas();
        c.set_tool(Tool::Line);
        c.mouse_down(pos2(10.0, 10.0), 1, false, 1.0);
        c.mouse_dragged(pos2(200.0, 18.0), true);
        c.mouse_up();
        assert_eq!(c.annotations[0].b, pos2(200.0, 10.0));
        // Drag the end handle.
        c.set_tool(Tool::Select);
        c.mouse_down(pos2(200.0, 10.0), 1, false, 1.0);
        c.mouse_dragged(pos2(250.0, 90.0), false);
        c.mouse_up();
        assert_eq!(c.annotations[0].b, pos2(250.0, 90.0));
    }
}
