//! Text layout and glyph rasterisation (cosmic-text), shared by annotation bounds and rendering.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight, Wrap};

/// Segoe UI's natural line height (ascent + descent) relative to its em size; close enough for
/// the Linux faces (Noto Sans, DejaVu Sans) too.
const LINE_HEIGHT: f32 = 1.33;

struct Engine {
    fonts: FontSystem,
    swash: SwashCache,
    measured: HashMap<(String, u32, u32), (f32, f32)>,
}

fn engine() -> &'static Mutex<Engine> {
    static E: OnceLock<Mutex<Engine>> = OnceLock::new();
    E.get_or_init(|| {
        Mutex::new(Engine { fonts: FontSystem::new(), swash: SwashCache::new(), measured: HashMap::new() })
    })
}

/// Loads the system font database in the background so the first text edit doesn't stall.
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = measure("Zquig", 14.0, 1000.0);
    });
}

fn layout(fonts: &mut FontSystem, text: &str, size: f32, wrap: f32) -> Buffer {
    let mut buf = Buffer::new(fonts, Metrics::new(size, size * LINE_HEIGHT));
    buf.set_wrap(Wrap::WordOrGlyph);
    buf.set_size(Some(wrap.max(1.0)), None);
    let attrs = Attrs::new().family(Family::Name(crate::os::text_family())).weight(Weight::BOLD);
    buf.set_text(text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(fonts, false);
    buf
}

fn extent(buf: &Buffer) -> (f32, f32) {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    for run in buf.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    (w, h)
}

/// Width and height of `text` wrapped at `wrap`, in the same units as `size`.
pub fn measure(text: &str, size: f32, wrap: f32) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let mut e = engine().lock().unwrap();
    let key = (text.to_owned(), size.to_bits(), wrap.to_bits());
    if let Some(&m) = e.measured.get(&key) {
        return m;
    }
    let m = extent(&layout(&mut e.fonts, text, size, wrap));
    if e.measured.len() > 500 {
        e.measured.clear();
    }
    e.measured.insert(key, m);
    m
}

/// A rasterised text block: coverage plus any colour (emoji) glyphs, `pad` pixels of margin on each side.
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub pad: usize,
    /// Glyph coverage, 0..255, `width * height`.
    pub mask: Vec<u8>,
    /// Straight-alpha RGBA for colour glyphs, if there were any.
    pub color: Option<Vec<[u8; 4]>>,
}

/// Rasterises `text` at `size` pixels, wrapped at `wrap` pixels.
pub fn rasterize(text: &str, size: f32, wrap: f32, pad: usize) -> Raster {
    let mut e = engine().lock().unwrap();
    let Engine { fonts, swash, .. } = &mut *e;
    let buf = layout(fonts, text, size, wrap);
    let (w, h) = extent(&buf);
    // Text taller or wider than this is cut off rather than allocating an enormous bitmap.
    const MAX_SIDE: usize = 16_384;
    let width = (w.ceil() as usize + 2 * pad).min(MAX_SIDE);
    let height = (h.ceil() as usize + 2 * pad).min(MAX_SIDE);
    let mut mask = vec![0u8; width * height];
    let mut color: Option<Vec<[u8; 4]>> = None;

    for run in buf.layout_runs() {
        for g in run.glyphs.iter() {
            let pg = g.physical((pad as f32, pad as f32 + run.line_y), 1.0);
            let Some(img) = swash.get_image(fonts, pg.cache_key) else { continue };
            let x0 = pg.x + img.placement.left;
            let y0 = pg.y - img.placement.top;
            let (gw, gh) = (img.placement.width as i32, img.placement.height as i32);
            for gy in 0..gh {
                let y = y0 + gy;
                if y < 0 || y >= height as i32 {
                    continue;
                }
                for gx in 0..gw {
                    let x = x0 + gx;
                    if x < 0 || x >= width as i32 {
                        continue;
                    }
                    let di = y as usize * width + x as usize;
                    let si = (gy * gw + gx) as usize;
                    match img.content {
                        SwashContent::Mask => mask[di] = mask[di].max(img.data[si]),
                        SwashContent::SubpixelMask => {
                            let s = &img.data[si * 4..si * 4 + 3];
                            let v = ((s[0] as u16 + s[1] as u16 + s[2] as u16) / 3) as u8;
                            mask[di] = mask[di].max(v);
                        }
                        SwashContent::Color => {
                            let s = &img.data[si * 4..si * 4 + 4];
                            let layer = color.get_or_insert_with(|| vec![[0; 4]; width * height]);
                            layer[di] = [s[0], s[1], s[2], s[3]];
                            mask[di] = mask[di].max(s[3]);
                        }
                    }
                }
            }
        }
    }
    Raster { width, height, pad, mask, color }
}
