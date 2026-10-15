use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::text;

/// sRGB, non-premultiplied, 0..1.
pub type Rgba = [f32; 4];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Tool {
    Select,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Pen,
    Highlight,
    Text,
    Stamp,
    Pixelate,
    Crop,
}

impl Tool {
    pub const ALL: [Tool; 11] = [
        Tool::Select,
        Tool::Arrow,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Pen,
        Tool::Highlight,
        Tool::Text,
        Tool::Stamp,
        Tool::Pixelate,
        Tool::Crop,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Arrow => "Arrow",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Pen => "Pen",
            Tool::Highlight => "Highlighter",
            Tool::Text => "Text",
            Tool::Stamp => "Numbered Stamp",
            Tool::Pixelate => "Pixelate",
            Tool::Crop => "Crop",
        }
    }

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as i;
        match self {
            Tool::Select => i::CURSOR,
            Tool::Arrow => i::ARROW_UP_RIGHT,
            Tool::Line => i::LINE_SEGMENT,
            Tool::Rect => i::RECTANGLE,
            Tool::Ellipse => i::CIRCLE,
            Tool::Pen => i::PENCIL_SIMPLE,
            Tool::Highlight => i::HIGHLIGHTER,
            Tool::Text => i::TEXT_T,
            Tool::Stamp => i::NUMBER_CIRCLE_ONE,
            Tool::Pixelate => i::CHECKERBOARD,
            Tool::Crop => i::CROP,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub kind: Tool,
    pub a: Pos2,
    pub b: Pos2,
    pub points: Vec<Pos2>,
    pub color: Rgba,
    pub width: f32,
    pub text: String,
    pub number: i32,
    /// Text wraps at this width (the space between the text origin and the canvas's right edge); set by the canvas.
    pub wrap_limit: f32,
    /// Set when the user drags a text block's side handles; None means "wrap at the canvas edge".
    pub custom_width: Option<f32>,
    /// Text only: white border plus soft shadow around the glyphs.
    pub outline: bool,
}

impl Annotation {
    pub fn new(kind: Tool, a: Pos2, b: Pos2, color: Rgba, width: f32) -> Self {
        Self {
            kind,
            a,
            b,
            points: Vec::new(),
            color,
            width,
            text: String::new(),
            number: 0,
            wrap_limit: 10_000.0,
            custom_width: None,
            outline: true,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_two_pos(self.a, self.b)
    }

    pub fn font_size(&self) -> f32 {
        font_size(self.width)
    }

    pub fn bounds(&self) -> Rect {
        match self.kind {
            Tool::Pen => {
                let Some(f) = self.points.first() else { return Rect::NOTHING };
                self.points.iter().fold(Rect::from_min_max(*f, *f), |r, p| r.union(Rect::from_min_max(*p, *p)))
            }
            Tool::Text => {
                // border/shadow are decoration; selection hugs the glyphs (or the user-set box)
                let ls = self.layout_size();
                Rect::from_min_size(self.a, vec2(self.custom_width.unwrap_or(ls.x), ls.y))
            }
            Tool::Stamp => Rect::from_center_size(self.a, vec2(30.0, 30.0)),
            _ => self.rect(),
        }
    }

    pub fn hit_test(&self, p: Pos2) -> bool {
        let slop = self.width.max(6.0);
        match self.kind {
            Tool::Line | Tool::Arrow => distance_to_segment(p, self.a, self.b) <= slop + 2.0,
            Tool::Pen => self.points.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= slop + 2.0),
            Tool::Rect | Tool::Ellipse => {
                let b = self.bounds();
                let outer = b.expand(slop);
                let inner = b.shrink(slop * 2.0);
                outer.contains(p) && !(inner.width() > 0.0 && inner.height() > 0.0 && inner.contains(p))
            }
            _ => self.bounds().expand(slop).contains(p),
        }
    }

    /// Resize handles: endpoints for lines/arrows, corners (TL, TR, BR, BL) for box-like shapes.
    pub fn handles(&self) -> Vec<Pos2> {
        match self.kind {
            Tool::Line | Tool::Arrow => vec![self.a, self.b],
            Tool::Text => {
                let r = self.bounds();
                vec![pos2(r.min.x, r.center().y), pos2(r.max.x, r.center().y)]
            }
            Tool::Rect | Tool::Ellipse | Tool::Highlight | Tool::Pixelate => {
                let r = self.rect();
                vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
            }
            _ => vec![],
        }
    }

    pub fn move_handle(&mut self, i: usize, p: Pos2) {
        match self.kind {
            Tool::Line | Tool::Arrow => {
                if i == 0 {
                    self.a = p
                } else {
                    self.b = p
                }
            }
            Tool::Text => {
                // 0 = left edge, 1 = right edge: changing the box width re-wraps the text.
                let right = self.a.x + self.custom_width.unwrap_or_else(|| self.layout_size().x);
                if i == 1 {
                    self.custom_width = Some((p.x - self.a.x).max(40.0));
                } else {
                    let new_x = p.x.min(right - 40.0);
                    self.a.x = new_x;
                    self.custom_width = Some(right - new_x);
                }
            }
            _ => {
                let h = self.handles();
                if i >= h.len() {
                    return;
                }
                self.a = h[(i + 2) % 4];
                self.b = p;
            }
        }
    }

    pub fn translate(&mut self, d: Vec2) {
        self.a += d;
        self.b += d;
        for p in &mut self.points {
            *p += d;
        }
    }

    /// Size of the text laid out within `wrap_limit` (wrapping on words, honouring newlines), in points.
    pub fn layout_size(&self) -> Vec2 {
        let (w, h) = text::measure(&self.text, self.font_size(), self.wrap_limit);
        vec2(w.ceil(), h.ceil())
    }
}

pub fn font_size(width: f32) -> f32 {
    14.0 + width * 3.0
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let l2 = d.length_sq();
    if l2 <= 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(d) / l2).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

// MARK: Clipboard serialisation (copy/paste of annotations within Zquig)

const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_POINTS: usize = 100_000;
const MAX_COORD: f32 = 100_000.0;
/// The thickest the style popover offers.
const MAX_WIDTH: f32 = 32.0;
/// Longest text accepted from the clipboard (pasted or inside a copied annotation).
pub const MAX_TEXT: usize = 20_000;

#[derive(Serialize, Deserialize)]
struct Stored {
    kind: Tool,
    a: [f32; 2],
    b: [f32; 2],
    points: Vec<[f32; 2]>,
    color: Rgba,
    width: f32,
    text: String,
    number: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    custom_width: Option<f32>,
    #[serde(default = "yes")]
    outline: bool,
}

fn yes() -> bool {
    true
}

impl Annotation {
    pub fn to_json(&self) -> String {
        let s = Stored {
            kind: self.kind,
            a: [self.a.x, self.a.y],
            b: [self.b.x, self.b.y],
            points: self.points.iter().map(|p| [p.x, p.y]).collect(),
            color: self.color,
            width: self.width,
            text: self.text.clone(),
            number: self.number,
            custom_width: self.custom_width,
            outline: self.outline,
        };
        serde_json::to_string(&s).unwrap_or_default()
    }

    /// Reads an annotation from the clipboard. Any app can put data in this format, so every value
    /// is clamped to what the editor itself can produce (a huge width or text could otherwise make
    /// rendering allocate gigabytes).
    pub fn from_json(json: &str) -> Option<Self> {
        if json.len() > MAX_JSON {
            return None;
        }
        let s: Stored = serde_json::from_str(json).ok()?;
        if s.kind == Tool::Select || s.kind == Tool::Crop || s.points.len() > MAX_POINTS {
            return None;
        }
        let coord = |v: f32| if v.is_finite() { v.clamp(-MAX_COORD, MAX_COORD) } else { 0.0 };
        let pt = |p: [f32; 2]| pos2(coord(p[0]), coord(p[1]));
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        let width = if s.width.is_finite() { s.width.clamp(1.0, MAX_WIDTH) } else { 4.0 };
        let color = [unit(s.color[0]), unit(s.color[1]), unit(s.color[2]), unit(s.color[3])];
        let mut a = Annotation::new(s.kind, pt(s.a), pt(s.b), color, width);
        a.points = s.points.into_iter().map(pt).collect();
        a.text = s.text.chars().take(MAX_TEXT).collect();
        a.number = s.number.clamp(0, 9999);
        a.custom_width = s.custom_width.filter(|w| w.is_finite()).map(|w| w.clamp(40.0, MAX_COORD));
        a.outline = s.outline;
        Some(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_round_trip() {
        let mut a = Annotation::new(Tool::Text, pos2(10.0, 20.0), pos2(10.0, 20.0), [1.0, 0.0, 0.0, 1.0], 8.0);
        a.text = "hi".into();
        a.custom_width = Some(120.0);
        let b = Annotation::from_json(&a.to_json()).unwrap();
        assert_eq!((b.kind, b.a, b.width, b.text.as_str(), b.custom_width), (Tool::Text, a.a, 8.0, "hi", Some(120.0)));
    }

    #[test]
    fn hostile_clipboard_data_is_clamped_or_rejected() {
        // Absurd sizes, out-of-range colours and a long text are clamped to what the editor can make.
        let long = "x".repeat(MAX_TEXT + 500);
        let json = format!(
            r#"{{"kind":"Text","a":[1e30,-1e30],"b":[0,0],"points":[],"color":[9,-1,0.5,2],"width":1e9,"text":"{long}","number":-5,"custom_width":1e20}}"#
        );
        let a = Annotation::from_json(&json).unwrap();
        assert_eq!(a.width, MAX_WIDTH);
        assert_eq!(a.a, pos2(MAX_COORD, -MAX_COORD));
        assert_eq!(a.color, [1.0, 0.0, 0.5, 1.0]);
        assert_eq!(a.text.chars().count(), MAX_TEXT);
        assert_eq!(a.number, 0);
        assert_eq!(a.custom_width, Some(MAX_COORD));

        // Kinds that aren't drawable, too many points, and garbage are rejected.
        assert!(Annotation::from_json(r#"{"kind":"Crop","a":[0,0],"b":[1,1],"points":[],"color":[0,0,0,1],"width":4,"text":"","number":0}"#).is_none());
        let many = vec!["[0,0]"; MAX_POINTS + 1].join(",");
        let json = format!(r#"{{"kind":"Pen","a":[0,0],"b":[0,0],"points":[{many}],"color":[0,0,0,1],"width":4,"text":"","number":0}}"#);
        assert!(Annotation::from_json(&json).is_none());
        assert!(Annotation::from_json("not json").is_none());
    }
}
