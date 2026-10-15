//! Draws annotations onto a bitmap. Annotation geometry is in points; `scale` is pixels per point.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Pos2, Rect};
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke, Transform,
};

use crate::annotation::{Annotation, Rgba, Tool};
use crate::text;

fn color(c: Rgba, alpha: f32) -> Color {
    Color::from_rgba(c[0], c[1], c[2], (c[3] * alpha).clamp(0.0, 1.0)).unwrap_or(Color::BLACK)
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c, 1.0));
    p.anti_alias = true;
    p
}

fn stroke(width: f32, round: bool) -> Stroke {
    Stroke {
        width,
        line_cap: if round { LineCap::Round } else { LineCap::Butt },
        line_join: if round { LineJoin::Round } else { LineJoin::Miter },
        ..Stroke::default()
    }
}

fn sk_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y)
}

fn rounded_rect(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    if rad <= 0.0 {
        return Some(PathBuilder::from_rect(sk_rect(r)?));
    }
    // Quarter circles as cubics (k = 0.5523), like NSBezierPath(roundedRect:).
    let k = rad * 0.552_284_8;
    let (l, t, rr, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(rr - rad, t);
    pb.cubic_to(rr - rad + k, t, rr, t + rad - k, rr, t + rad);
    pb.line_to(rr, b - rad);
    pb.cubic_to(rr, b - rad + k, rr - rad + k, b, rr - rad, b);
    pb.line_to(l + rad, b);
    pb.cubic_to(l + rad - k, b, l, b - rad + k, l, b - rad);
    pb.line_to(l, t + rad);
    pb.cubic_to(l, t + rad - k, l + rad - k, t, l + rad, t);
    pb.close();
    pb.finish()
}

/// Draws `ann` on `pix`. `base` is the unannotated image (pixelate samples from it).
pub fn draw(pix: &mut Pixmap, base: &Pixmap, ann: &Annotation, scale: f32) {
    let ts = Transform::from_scale(scale, scale);
    let c = ann.color;
    match ann.kind {
        Tool::Line => {
            let mut pb = PathBuilder::new();
            pb.move_to(ann.a.x, ann.a.y);
            pb.line_to(ann.b.x, ann.b.y);
            if let Some(path) = pb.finish() {
                pix.stroke_path(&path, &paint(c), &stroke(ann.width, true), ts, None);
            }
        }
        Tool::Arrow => draw_arrow(pix, ann, ts),
        Tool::Rect => {
            if let Some(path) = rounded_rect(ann.rect(), 2.0) {
                pix.stroke_path(&path, &paint(c), &stroke(ann.width, false), ts, None);
            }
        }
        Tool::Ellipse => {
            if let Some(path) = sk_rect(ann.rect()).and_then(PathBuilder::from_oval) {
                pix.stroke_path(&path, &paint(c), &stroke(ann.width, false), ts, None);
            }
        }
        Tool::Pen => {
            let Some(first) = ann.points.first() else { return };
            let mut pb = PathBuilder::new();
            pb.move_to(first.x, first.y);
            for p in &ann.points[1..] {
                pb.line_to(p.x, p.y);
            }
            if ann.points.len() == 1 {
                pb.line_to(first.x + 0.01, first.y);
            }
            if let Some(path) = pb.finish() {
                pix.stroke_path(&path, &paint(c), &stroke(ann.width, true), ts, None);
            }
        }
        Tool::Highlight => {
            if let Some(r) = sk_rect(ann.rect()) {
                let mut p = Paint::default();
                p.set_color(color(c, 0.45));
                p.blend_mode = BlendMode::Multiply;
                p.anti_alias = true;
                pix.fill_rect(r, &p, ts, None);
            }
        }
        Tool::Text => draw_text(pix, ann, scale),
        Tool::Stamp => {
            let r = ann.bounds();
            if let Some(path) = sk_rect(r).and_then(PathBuilder::from_oval) {
                pix.fill_path(&path, &paint(c), FillRule::Winding, ts, None);
            }
            let label = ann.number.to_string();
            let raster = text::rasterize(&label, 16.0 * scale, 10_000.0, 0);
            let cx = r.center().x * scale - raster.width as f32 / 2.0;
            let cy = r.center().y * scale - raster.height as f32 / 2.0;
            blend_mask(pix, &raster.mask, raster.width, raster.height, cx.round() as i32, cy.round() as i32, [1.0; 4], 1.0);
        }
        Tool::Pixelate => draw_pixelated(pix, base, ann, scale),
        Tool::Select | Tool::Crop => {}
    }
}

fn draw_arrow(pix: &mut Pixmap, ann: &Annotation, ts: Transform) {
    let (a, b, width) = (ann.a, ann.b, ann.width);
    let d = b - a;
    let len = d.length();
    if len <= 1.0 {
        return;
    }
    let u = d / len;
    let n = egui::vec2(-u.y, u.x);
    let head_len = len.min(10.0 + width * 3.0);
    let head_w = 5.0 + width * 2.0;
    let shaft = (width * 0.5).max(1.0);
    let neck = b - u * head_len;
    let pts = [
        a + n * 0.5,
        neck + n * shaft,
        neck + n * head_w,
        b,
        neck - n * head_w,
        neck - n * shaft,
        a - n * 0.5,
    ];
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].x, pts[0].y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        pix.fill_path(&path, &paint(ann.color), FillRule::Winding, ts, None);
    }
}

/// Averages the area under the rectangle into blocks, sampling the unannotated image.
fn draw_pixelated(pix: &mut Pixmap, base: &Pixmap, ann: &Annotation, scale: f32) {
    let img = Rect::from_min_size(Pos2::ZERO, egui::vec2(base.width() as f32 / scale, base.height() as f32 / scale));
    let r = ann.rect().intersect(img);
    if r.width() <= 2.0 || r.height() <= 2.0 {
        return;
    }
    let x0 = (r.min.x * scale).floor().max(0.0) as usize;
    let y0 = (r.min.y * scale).floor().max(0.0) as usize;
    let x1 = ((r.max.x * scale).ceil() as usize).min(base.width() as usize);
    let y1 = ((r.max.y * scale).ceil() as usize).min(base.height() as usize);
    let (pw, ph) = (x1 - x0, y1 - y0);
    let block = (ann.width * 2.5).max(6.0) * scale;
    let cols = ((pw as f32 / block) as usize).max(1);
    let rows = ((ph as f32 / block) as usize).max(1);
    let src = base.pixels();
    let bw = base.width() as usize;
    let dw = pix.width() as usize;
    let dst = pix.pixels_mut();
    for row in 0..rows {
        let ys = y0 + row * ph / rows;
        let ye = y0 + (row + 1) * ph / rows;
        for col in 0..cols {
            let xs = x0 + col * pw / cols;
            let xe = x0 + (col + 1) * pw / cols;
            let mut sum = [0u64; 4];
            for y in ys..ye {
                for p in &src[y * bw + xs..y * bw + xe] {
                    sum[0] += p.red() as u64;
                    sum[1] += p.green() as u64;
                    sum[2] += p.blue() as u64;
                    sum[3] += p.alpha() as u64;
                }
            }
            let n = ((ye - ys) * (xe - xs)).max(1) as u64;
            let avg = PremultipliedColorU8::from_rgba(
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                (sum[3] / n) as u8,
            )
            .unwrap_or(PremultipliedColorU8::TRANSPARENT);
            for y in ys..ye {
                dst[y * dw + xs..y * dw + xe].fill(avg);
            }
        }
    }
}

/// Source-over blends `color` through an 8-bit coverage mask at (ox, oy).
#[allow(clippy::too_many_arguments)]
fn blend_mask(pix: &mut Pixmap, mask: &[u8], mw: usize, mh: usize, ox: i32, oy: i32, c: Rgba, alpha: f32) {
    let (pw, ph) = (pix.width() as i32, pix.height() as i32);
    let data = pix.data_mut();
    let ca = c[3] * alpha;
    for my in 0..mh as i32 {
        let y = oy + my;
        if y < 0 || y >= ph {
            continue;
        }
        for mx in 0..mw as i32 {
            let x = ox + mx;
            if x < 0 || x >= pw {
                continue;
            }
            let m = mask[(my as usize) * mw + mx as usize];
            if m == 0 {
                continue;
            }
            let a = m as f32 / 255.0 * ca;
            let i = ((y * pw + x) * 4) as usize;
            let px = &mut data[i..i + 4];
            for k in 0..3 {
                px[k] = (c[k] * a * 255.0 + px[k] as f32 * (1.0 - a)).round() as u8;
            }
            px[3] = (a * 255.0 + px[3] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

/// The text decoration layers, cached per (text, size, wrap, scale) like the Mac app's shadow cache.
struct TextLayers {
    raster: text::Raster,
    /// Glyphs grown by the border radius (the white outline).
    border: Vec<u8>,
    /// `border`, blurred (the shadow).
    shadow: Vec<u8>,
}

type LayerCache = Mutex<HashMap<(String, u32, u32, u32), Arc<TextLayers>>>;

fn layer_cache() -> &'static LayerCache {
    static C: OnceLock<LayerCache> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn text_layers(ann: &Annotation, scale: f32) -> Arc<TextLayers> {
    let size = ann.font_size();
    let key = (ann.text.clone(), size.to_bits(), ann.wrap_limit.to_bits(), scale.to_bits());
    if let Some(hit) = layer_cache().lock().unwrap().get(&key) {
        return hit.clone();
    }
    let px = size * scale;
    let radius = (size * 0.05).max(1.0) * scale;
    let sigma = size * 0.07 * scale;
    let pad = (size * 0.6).ceil() as usize * scale.ceil() as usize;
    let raster = text::rasterize(&ann.text, px, ann.wrap_limit * scale, pad);
    let border = dilate(&raster.mask, raster.width, raster.height, radius);
    let shadow = blur(&border, raster.width, raster.height, sigma);
    let layers = Arc::new(TextLayers { raster, border, shadow });
    let mut cache = layer_cache().lock().unwrap();
    if cache.len() > 200 {
        cache.clear();
    }
    cache.insert(key, layers.clone());
    layers
}

/// White border drawn *outside* the glyphs, with a soft shadow underneath.
fn draw_text(pix: &mut Pixmap, ann: &Annotation, scale: f32) {
    if ann.text.is_empty() {
        return;
    }
    let size = ann.font_size();
    let layers = text_layers(ann, scale);
    let r = &layers.raster;
    let ox = (ann.a.x * scale).round() as i32 - r.pad as i32;
    let oy = (ann.a.y * scale).round() as i32 - r.pad as i32;
    if ann.outline {
        let dy = ((size * 0.09).max(1.5) * scale).round() as i32;
        blend_mask(pix, &layers.shadow, r.width, r.height, ox, oy + dy, [0.0, 0.0, 0.0, 1.0], 0.32);
        blend_mask(pix, &layers.border, r.width, r.height, ox, oy, [1.0; 4], 1.0);
    }
    match &r.color {
        None => blend_mask(pix, &r.mask, r.width, r.height, ox, oy, ann.color, 1.0),
        Some(layer) => {
            // Emoji keep their own colours; everything else takes the annotation colour.
            let plain: Vec<u8> = r.mask.iter().zip(layer).map(|(&m, c)| if c[3] > 0 { 0 } else { m }).collect();
            blend_mask(pix, &plain, r.width, r.height, ox, oy, ann.color, 1.0);
            blend_rgba(pix, layer, r.width, r.height, ox, oy);
        }
    }
}

fn blend_rgba(pix: &mut Pixmap, layer: &[[u8; 4]], w: usize, h: usize, ox: i32, oy: i32) {
    let (pw, ph) = (pix.width() as i32, pix.height() as i32);
    let data = pix.data_mut();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (dx, dy) = (ox + x, oy + y);
            if dx < 0 || dy < 0 || dx >= pw || dy >= ph {
                continue;
            }
            let s = layer[y as usize * w + x as usize];
            if s[3] == 0 {
                continue;
            }
            let a = s[3] as f32 / 255.0;
            let i = ((dy * pw + dx) * 4) as usize;
            for k in 0..3 {
                data[i + k] = (s[k] as f32 * a + data[i + k] as f32 * (1.0 - a)).round() as u8;
            }
            data[i + 3] = (s[3] as f32 + data[i + 3] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

/// Max of the mask shifted to 32 points on a circle (the Mac app draws the text 32 times in white).
fn dilate(mask: &[u8], w: usize, h: usize, radius: f32) -> Vec<u8> {
    let mut out = mask.to_vec();
    for i in 0..32 {
        let t = i as f32 / 32.0 * std::f32::consts::TAU;
        let (dx, dy) = (t.cos() * radius, t.sin() * radius);
        let (fx, fy) = (dx.floor(), dy.floor());
        let (ax, ay) = (dx - fx, dy - fy);
        let (ix, iy) = (fx as i32, fy as i32);
        let sample = |x: i32, y: i32| -> f32 {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 { 0.0 } else { mask[y as usize * w + x as usize] as f32 }
        };
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                // Bilinear sample of the mask shifted by (dx, dy).
                let (sx, sy) = (x - ix, y - iy);
                let v = sample(sx, sy) * (1.0 - ax) * (1.0 - ay)
                    + sample(sx - 1, sy) * ax * (1.0 - ay)
                    + sample(sx, sy - 1) * (1.0 - ax) * ay
                    + sample(sx - 1, sy - 1) * ax * ay;
                let o = &mut out[y as usize * w + x as usize];
                *o = (*o).max(v.round() as u8);
            }
        }
    }
    out
}

/// Gaussian blur approximated by three box blurs per axis.
fn blur(mask: &[u8], w: usize, h: usize, sigma: f32) -> Vec<u8> {
    let r = (((12.0 * sigma * sigma / 3.0) + 1.0).sqrt() / 2.0).round() as usize;
    if r == 0 {
        return mask.to_vec();
    }
    let mut a: Vec<f32> = mask.iter().map(|&v| v as f32).collect();
    let mut b = vec![0f32; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, w, h, r, true);
        box_pass(&b, &mut a, w, h, r, false);
    }
    a.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect()
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let idx = |o: usize, i: usize| if horizontal { o * w + i } else { i * w + o };
    let norm = 1.0 / (2 * r + 1) as f32;
    for o in 0..outer {
        let mut acc = 0.0;
        for i in 0..=r.min(inner - 1) {
            acc += src[idx(o, i)];
        }
        for i in 0..inner {
            dst[idx(o, i)] = acc * norm;
            if i + r + 1 < inner {
                acc += src[idx(o, i + r + 1)];
            }
            if i >= r {
                acc -= src[idx(o, i - r)];
            }
        }
    }
}

/// Base image plus annotations (in order), skipping `skip`, followed by `extra` (an in-progress shape).
pub fn composite(base: &Pixmap, scale: f32, anns: &[Annotation], skip: Option<usize>, extra: Option<&Annotation>) -> Pixmap {
    let mut pix = base.clone();
    for (i, a) in anns.iter().enumerate() {
        if Some(i) != skip {
            draw(&mut pix, base, a, scale);
        }
    }
    if let Some(e) = extra {
        draw(&mut pix, base, e, scale);
    }
    pix
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    /// Renders one of every annotation; set ZQUIG_TEST_OUT to a directory to look at the result.
    #[test]
    fn renders_every_tool() {
        let mut base = Pixmap::new(640, 420).unwrap();
        base.fill(Color::from_rgba8(235, 240, 245, 255));
        // Some detail for the pixelate and highlight tools to work on.
        for y in (0..420).step_by(12) {
            for x in (0..640).step_by(12) {
                if (x + y) % 24 == 0 {
                    base.fill_rect(tiny_skia::Rect::from_xywh(x as f32, y as f32, 12.0, 12.0).unwrap(), &paint([0.2, 0.3, 0.5, 1.0]), Transform::identity(), None);
                }
            }
        }
        let red = [1.0, 0.231, 0.188, 1.0];
        let blue = [0.0, 0.478, 1.0, 1.0];
        let mut anns = vec![
            Annotation::new(Tool::Arrow, pos2(30.0, 30.0), pos2(200.0, 110.0), red, 4.0),
            Annotation::new(Tool::Line, pos2(230.0, 30.0), pos2(380.0, 100.0), blue, 2.0),
            Annotation::new(Tool::Rect, pos2(400.0, 20.0), pos2(600.0, 110.0), red, 4.0),
            Annotation::new(Tool::Ellipse, pos2(30.0, 140.0), pos2(200.0, 240.0), blue, 8.0),
            Annotation::new(Tool::Highlight, pos2(230.0, 140.0), pos2(400.0, 180.0), [1.0, 0.85, 0.0, 1.0], 4.0),
            Annotation::new(Tool::Pixelate, pos2(420.0, 140.0), pos2(600.0, 240.0), red, 4.0),
        ];
        let mut pen = Annotation::new(Tool::Pen, pos2(0.0, 0.0), pos2(0.0, 0.0), [0.204, 0.78, 0.349, 1.0], 4.0);
        pen.points = (0..60).map(|i| pos2(230.0 + i as f32 * 3.0, 220.0 + (i as f32 / 6.0).sin() * 15.0)).collect();
        anns.push(pen);
        for n in 1..=3 {
            let mut s = Annotation::new(Tool::Stamp, pos2(40.0 + n as f32 * 40.0, 290.0), pos2(0.0, 0.0), red, 4.0);
            s.number = n;
            anns.push(s);
        }
        let mut t = Annotation::new(Tool::Text, pos2(230.0, 270.0), pos2(230.0, 270.0), red, 4.0);
        t.text = "Zquig on Windows \u{2014} wraps when the line gets long".into();
        t.wrap_limit = 380.0;
        anns.push(t);
        let mut plain = Annotation::new(Tool::Text, pos2(30.0, 360.0), pos2(30.0, 360.0), [0.0, 0.0, 0.0, 1.0], 2.0);
        plain.text = "No border".into();
        plain.outline = false;
        anns.push(plain);

        for scale in [1.0f32, 2.0] {
            let scaled = {
                let mut p = Pixmap::new((640.0 * scale) as u32, (420.0 * scale) as u32).unwrap();
                p.draw_pixmap(0, 0, base.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..Default::default() }, Transform::from_scale(scale, scale), None);
                p
            };
            let out = composite(&scaled, scale, &anns, None, None);
            assert_eq!((out.width(), out.height()), (scaled.width(), scaled.height()));
            assert_ne!(out.data(), scaled.data());
            if let Some(dir) = std::env::var_os("ZQUIG_TEST_OUT") {
                out.save_png(std::path::Path::new(&dir).join(format!("render-{scale}x.png"))).unwrap();
            }
        }
    }
}
