//! Shows a bitmap of any size as a grid of GPU textures (GPUs cap texture sides at 8192 or so).

use egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use tiny_skia::Pixmap;

const TILE: u32 = 4096;

pub struct Tiles {
    tiles: Vec<(Rect, TextureHandle)>,
    width: u32,
    height: u32,
}

fn options() -> TextureOptions {
    TextureOptions {
        magnification: egui::TextureFilter::Linear,
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(egui::TextureFilter::Linear),
    }
}

fn tile_image(pix: &Pixmap, r: Rect) -> ColorImage {
    let (x0, y0) = (r.min.x as usize, r.min.y as usize);
    let (w, h) = (r.width() as usize, r.height() as usize);
    let stride = pix.width() as usize * 4;
    let src = pix.data();
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in y0..y0 + h {
        bytes.extend_from_slice(&src[y * stride + x0 * 4..y * stride + (x0 + w) * 4]);
    }
    // tiny-skia pixels are premultiplied, which is what egui's textures hold too.
    ColorImage::from_rgba_premultiplied([w, h], &bytes)
}

impl Tiles {
    pub fn new(ctx: &Context, name: &str, pix: &Pixmap) -> Self {
        let mut tiles = Vec::new();
        let (w, h) = (pix.width(), pix.height());
        for ty in (0..h).step_by(TILE as usize) {
            for tx in (0..w).step_by(TILE as usize) {
                let r = Rect::from_min_size(
                    pos2(tx as f32, ty as f32),
                    vec2((w - tx).min(TILE) as f32, (h - ty).min(TILE) as f32),
                );
                let tex = ctx.load_texture(format!("{name}-{tx}-{ty}"), tile_image(pix, r), options());
                tiles.push((r, tex));
            }
        }
        Self { tiles, width: w, height: h }
    }

    /// Re-uploads in place when the size is unchanged; otherwise rebuilds.
    pub fn update(&mut self, ctx: &Context, name: &str, pix: &Pixmap) {
        if pix.width() != self.width || pix.height() != self.height {
            *self = Self::new(ctx, name, pix);
            return;
        }
        for (r, tex) in &mut self.tiles {
            tex.set(tile_image(pix, *r), options());
        }
    }

    /// Paints the whole bitmap into `dest`.
    pub fn paint(&self, painter: &Painter, dest: Rect) {
        let sx = dest.width() / self.width as f32;
        let sy = dest.height() / self.height as f32;
        for (r, tex) in &self.tiles {
            let d = Rect::from_min_max(
                Pos2::new(dest.min.x + r.min.x * sx, dest.min.y + r.min.y * sy),
                Pos2::new(dest.min.x + r.max.x * sx, dest.min.y + r.max.y * sy),
            );
            painter.image(tex.id(), d, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
