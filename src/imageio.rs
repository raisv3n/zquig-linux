//! Loading and saving images, PDFs and clipboard images.

use std::io::Write;
use std::path::Path;

use image::{ImageEncoder, RgbaImage};
use tiny_skia::{IntSize, Pixmap};

pub fn pixmap_from_rgba(img: RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        if a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

pub fn rgba_from_pixmap(pix: &Pixmap) -> RgbaImage {
    RgbaImage::from_raw(pix.width(), pix.height(), pix.clone().take_demultiplied()).expect("pixmap size")
}

/// RGB over white, for formats without transparency.
fn rgb_on_white(pix: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity((pix.width() * pix.height() * 3) as usize);
    for p in pix.pixels() {
        let inv = 255 - p.alpha() as u16;
        out.push((p.red() as u16 + inv).min(255) as u8);
        out.push((p.green() as u16 + inv).min(255) as u8);
        out.push((p.blue() as u16 + inv).min(255) as u8);
    }
    out
}

pub fn blank(width: u32, height: u32) -> Pixmap {
    let mut p = Pixmap::new(width, height).expect("blank size");
    p.fill(tiny_skia::Color::WHITE);
    p
}

pub fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

pub fn load_image(path: &Path) -> Result<Pixmap, String> {
    let img = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    pixmap_from_rgba(img.to_rgba8()).ok_or_else(|| "The image is empty.".into())
}

pub fn encode_png(pix: &Pixmap) -> Vec<u8> {
    let img = rgba_from_pixmap(pix);
    let mut out = Vec::new();
    let _ = image::codecs::png::PngEncoder::new(&mut out).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    );
    out
}

pub fn encode_jpeg(pix: &Pixmap, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).write_image(
        &rgb_on_white(pix),
        pix.width(),
        pix.height(),
        image::ExtendedColorType::Rgb8,
    );
    out
}

fn encode_tiff(pix: &Pixmap) -> Result<Vec<u8>, String> {
    let img = rgba_from_pixmap(pix);
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::tiff::TiffEncoder::new(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// A one-page PDF holding the image. `scale` is pixels per point; the page keeps the
/// image's on-screen size (Windows points are 1/96 inch, PDF points 1/72).
fn encode_pdf(pix: &Pixmap, scale: f32) -> Vec<u8> {
    let (w, h) = (pix.width(), pix.height());
    let page_w = w as f32 / scale * 0.75;
    let page_h = h as f32 / scale * 0.75;
    let image = miniz_oxide::deflate::compress_to_vec_zlib(&rgb_on_white(pix), 6);
    let content = format!("q {page_w:.2} 0 0 {page_h:.2} 0 0 cm /Im0 Do Q\n");

    let mut out: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let _ = writeln!(out, "{} 0 obj", offsets.len());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.2} {page_h:.2}] \
             /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"
        )
        .as_bytes(),
    );
    let mut img_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        image.len()
    )
    .into_bytes();
    img_obj.extend_from_slice(&image);
    img_obj.extend_from_slice(b"\nendstream");
    obj(&mut out, &img_obj);
    obj(&mut out, format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).as_bytes());

    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(out, "{o:010} 00000 n ");
    }
    let _ = write!(out, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out
}

/// Encodes for the file's extension (PNG when unknown) and writes it atomically.
pub fn save(pix: &Pixmap, scale: f32, path: &Path) -> Result<(), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let data = match ext.as_str() {
        "jpg" | "jpeg" => encode_jpeg(pix, 90),
        "tif" | "tiff" => encode_tiff(pix)?,
        "pdf" => encode_pdf(pix, scale),
        _ => encode_png(pix),
    };
    write_atomic(path, &data)
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!(
        "{}.zquig-tmp",
        path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

// MARK: Clipboard images

pub fn clipboard_image() -> Option<Pixmap> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    // Same ceiling as file decoding (about 600 MB of pixels).
    if img.width.checked_mul(img.height).is_none_or(|n| n > 150_000_000) {
        return None;
    }
    let rgba = RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
    pixmap_from_rgba(rgba)
}

pub fn clipboard_has_image() -> bool {
    use clipboard_win::{formats, raw};
    raw::is_format_avail(formats::CF_DIB)
        || raw::is_format_avail(formats::CF_DIBV5)
        || raw::is_format_avail(formats::CF_BITMAP)
        || raw::register_format("PNG").is_some_and(|f| raw::is_format_avail(f.get()))
}

pub fn copy_image(pix: &Pixmap) -> Result<(), String> {
    let img = rgba_from_pixmap(pix);
    let data = arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| e.to_string())
}

// MARK: PDF (Windows' built-in renderer)

/// Stacks up to 50 pages vertically into a single image. Returns the image and its scale (pixels per point).
pub fn render_pdf(path: &Path) -> Result<(Pixmap, f32), String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::core::HSTRING;

    let _com = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
    let encrypted = |e: windows::core::Error| {
        // ERROR_WRONG_PASSWORD
        if e.code().0 as u32 == 0x8007_052B { "Cannot open encrypted PDF".to_string() } else { e.message().to_string() }
    };
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
        .and_then(|op| op.join())
        .map_err(|e| e.message().to_string())?;
    let doc = PdfDocument::LoadFromFileAsync(&file).and_then(|op| op.join()).map_err(encrypted)?;
    if doc.IsPasswordProtected().unwrap_or(false) {
        return Err("Cannot open encrypted PDF".into());
    }
    let count = doc.PageCount().map_err(|e| e.message().to_string())?.min(50);
    if count == 0 {
        return Err("The PDF has no pages.".into());
    }
    let mut pages = Vec::new();
    for i in 0..count {
        let page = doc.GetPage(i).map_err(|e| e.message().to_string())?;
        let size = page.Size().map_err(|e| e.message().to_string())?;
        pages.push((page, size.Width, size.Height));
    }
    let width = pages.iter().map(|p| p.1).fold(0.0, f32::max);
    let gap = 12.0;
    let height = pages.iter().map(|p| p.2).sum::<f32>() + gap * (pages.len() - 1) as f32;
    // 2x like the Mac app, but keep long documents and oversized pages within a sane bitmap
    // (at most 20,000 px on a side and about 600 MB of pixels).
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err("The PDF's page size is invalid.".into());
    }
    let scale = 2.0f32
        .min(20_000.0 / height)
        .min(20_000.0 / width)
        .min((150_000_000.0 / (width * height)).sqrt());
    if (width * scale) < 1.0 || (height * scale) < 1.0 {
        return Err("The PDF's pages are too large to open.".into());
    }
    let mut out = blank((width * scale).ceil() as u32, (height * scale).ceil() as u32);

    let mut top = 0.0;
    for (page, w, h) in &pages {
        let stream = InMemoryRandomAccessStream::new().map_err(|e| e.message().to_string())?;
        let opts = PdfPageRenderOptions::new().map_err(|e| e.message().to_string())?;
        let _ = opts.SetDestinationWidth((w * scale).round() as u32);
        let _ = opts.SetDestinationHeight((h * scale).round() as u32);
        page.RenderWithOptionsToStreamAsync(&stream, &opts)
            .and_then(|op| op.join())
            .map_err(|e| e.message().to_string())?;
        let len = stream.Size().map_err(|e| e.message().to_string())? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(|e| e.message().to_string())?)
            .map_err(|e| e.message().to_string())?;
        reader.LoadAsync(len).and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
        let mut bytes = vec![0u8; len as usize];
        reader.ReadBytes(&mut bytes).map_err(|e| e.message().to_string())?;
        let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
        if let Some(pm) = pixmap_from_rgba(img) {
            let x = ((width - w) / 2.0 * scale).round() as i32;
            out.draw_pixmap(
                x,
                (top * scale).round() as i32,
                pm.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }
        top += h + gap;
    }
    Ok((out, scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Pixmap {
        let mut p = blank(120, 80);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(200, 30, 30, 255);
        p.fill_rect(tiny_skia::Rect::from_xywh(10.0, 10.0, 50.0, 30.0).unwrap(), &paint, tiny_skia::Transform::identity(), None);
        p
    }

    #[test]
    fn save_and_reload_each_format() {
        let dir = std::env::temp_dir().join(format!("zquig-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = sample();
        for ext in ["png", "jpg", "tiff"] {
            let path = dir.join(format!("t.{ext}"));
            save(&img, 1.0, &path).unwrap();
            let back = load_image(&path).unwrap();
            assert_eq!((back.width(), back.height()), (120, 80), "{ext}");
            let px = back.pixel(20, 20).unwrap();
            assert!(px.red() > 150 && px.green() < 80, "{ext}: {px:?}");
        }
        // PDF: written by hand, read back with Windows' own PDF renderer.
        let pdf = dir.join("t.pdf");
        save(&img, 1.0, &pdf).unwrap();
        let (back, scale) = render_pdf(&pdf).unwrap();
        assert_eq!(scale, 2.0);
        assert_eq!((back.width(), back.height()), (240, 160));
        let px = back.pixel(40, 40).unwrap();
        assert!(px.red() > 150 && px.green() < 80, "pdf: {px:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn premultiply_round_trip() {
        let img = RgbaImage::from_raw(1, 1, vec![200, 100, 50, 128]).unwrap();
        let back = rgba_from_pixmap(&pixmap_from_rgba(img).unwrap());
        let p = back.get_pixel(0, 0).0;
        assert_eq!(p[3], 128);
        assert!((p[0] as i32 - 200).abs() <= 2 && (p[1] as i32 - 100).abs() <= 2);
    }
}
