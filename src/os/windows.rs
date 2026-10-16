//! Windows integration: GDI capture, the registry, the WinRT PDF renderer and the Win32 clipboard.

use std::path::{Path, PathBuf};

use egui::{Rect, pos2};
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, KEY_READ, KEY_WRITE, REG_NOTIFY_CHANGE_LAST_SET,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClassNameW, GetSystemMetrics, GetWindowLongW, IsIconic, IsWindowVisible, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::{BOOL, HSTRING, PCWSTR};
use winit::window::{Window, WindowAttributes};

use crate::capture::{Desktop, Monitor};
use crate::imageio::{blank, pixmap_from_rgba};

// MARK: Capture

pub fn grab() -> Option<Desktop> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut data = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(data.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok || lines == 0 {
            return None;
        }
        for px in data.as_chunks_mut::<4>().0 {
            px.swap(0, 2); // BGRA -> RGBA
            px[3] = 255;
        }
        let image = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w as u32, h as u32)?)?;
        let origin = (x, y);
        Some(Desktop { image, origin, monitors: monitors(origin), windows: windows(origin) })
    }
}

fn to_rect(r: RECT, origin: (i32, i32)) -> Rect {
    Rect::from_min_max(
        pos2((r.left - origin.0) as f32, (r.top - origin.1) as f32),
        pos2((r.right - origin.0) as f32, (r.bottom - origin.1) as f32),
    )
}

fn monitors(origin: (i32, i32)) -> Vec<Monitor> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(data.0 as *mut Vec<(RECT, RECT, f32, bool)>);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(m, &mut info).as_bool() {
                let mut dx = 96u32;
                let mut dy = 96u32;
                let _ = windows::Win32::UI::HiDpi::GetDpiForMonitor(
                    m,
                    windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
                    &mut dx,
                    &mut dy,
                );
                // MONITORINFOF_PRIMARY
                list.push((info.rcMonitor, info.rcWork, dx as f32 / 96.0, info.dwFlags & 1 != 0));
            }
            true.into()
        }
    }
    let mut list: Vec<(RECT, RECT, f32, bool)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list.into_iter()
        .map(|(r, w, s, p)| Monitor { rect: to_rect(r, origin), work: to_rect(w, origin), scale: s, primary: p })
        .collect()
}

fn windows(origin: (i32, i32)) -> Vec<Rect> {
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(data.0 as *mut Vec<RECT>);
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return true.into();
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_TRANSPARENT.0 != 0 {
                return true.into();
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4);
            if cloaked != 0 {
                return true.into();
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            if class == "Progman" || class == "WorkerW" {
                return true.into(); // the desktop itself
            }
            // The visible frame, without the invisible resize borders.
            let mut r = RECT::default();
            if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, (&mut r as *mut RECT).cast(), size_of::<RECT>() as u32)
                .is_err()
            {
                return true.into();
            }
            if r.right - r.left > 8 && r.bottom - r.top > 8 {
                list.push(r);
            }
            true.into()
        }
    }
    let mut list: Vec<RECT> = Vec::new();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list.into_iter().map(|r| to_rect(r, origin)).collect()
}

/// Monitors in absolute physical coordinates.
pub fn monitor_list() -> Vec<Monitor> {
    monitors((0, 0))
}

/// Where Windows keeps screenshots (Pictures\Screenshots), created if needed.
pub fn screenshots_dir() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, FOLDERID_Screenshots, KF_FLAG_CREATE, SHGetKnownFolderPath};
    let known = |id| unsafe {
        SHGetKnownFolderPath(id, KF_FLAG_CREATE, None).ok().map(|p| {
            let s = p.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            PathBuf::from(s)
        })
    };
    let dir = known(&FOLDERID_Screenshots)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| known(&FOLDERID_Pictures).map(|p| p.join("Screenshots")))
        .unwrap_or_else(|| std::env::temp_dir().join("Screenshots"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn local_timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} at {:02}.{:02}.{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

pub const CAPTURE_FAILED: &str = "Windows didn't return an image.";

// MARK: Windows (the on-screen kind)

/// DirectX 12 (OpenGL as a fallback). wgpu would otherwise try Vulkan first, and Vulkan overlay
/// layers that some apps install (game recorders, chat overlays) can hang device creation.
pub fn gpu_backends() -> egui_wgpu::wgpu::Backends {
    egui_wgpu::wgpu::Backends::DX12 | egui_wgpu::wgpu::Backends::GL
}

pub fn editor_attrs(attrs: WindowAttributes) -> WindowAttributes {
    attrs
}

/// The capture overlay and the thumbnail toast: no taskbar button.
pub fn overlay_attrs(attrs: WindowAttributes) -> WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true)
}

pub fn toast_attrs(attrs: WindowAttributes) -> WindowAttributes {
    overlay_attrs(attrs)
}

/// Windows already gives the topmost overlay the keyboard.
pub fn grab_keyboard(_window: &Window) -> bool {
    true
}

/// Segoe UI, the Windows UI face, in regular and bold.
pub fn ui_fonts() -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    (std::fs::read(dir.join("segoeui.ttf")).ok(), std::fs::read(dir.join("segoeuib.ttf")).ok())
}

/// The annotation text face. Bold matches the Mac app's `systemFont(weight: .bold)`.
pub fn text_family() -> &'static str {
    "Segoe UI"
}

pub fn open_folder(dir: &Path) {
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
}

pub fn beep() {
    unsafe {
        let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(windows::Win32::UI::WindowsAndMessaging::MB_OK);
    }
}

/// Whether Space in the file manager previews files (`watch_file_manager`).
pub const FILE_MANAGER_PREVIEW: bool = false;

/// Drags a file out of `window` into another app.
pub const CAN_DRAG_OUT: bool = true;

pub fn start_drag(window: &Window, path: PathBuf, thumb_png: Vec<u8>) {
    let _ = drag::start_drag(window, drag::DragItem::Files(vec![path]), drag::Image::Raw(thumb_png), |_, _| {}, drag::Options::default());
}

// MARK: Registry

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Zquig")
}

fn wide(s: &str) -> HSTRING {
    HSTRING::from(s)
}

fn set_value(path: &str, name: Option<&str>, value: &str) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &wide(path), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None)
            .is_err()
        {
            return;
        }
        let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
        let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
        let name = name.map(wide);
        let _ = RegSetValueExW(key, name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
    }
}

fn get_value(path: &str, name: Option<&str>) -> Option<String> {
    unsafe {
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let name = name.map(wide);
        RegGetValueW(
            HKEY_CURRENT_USER,
            &wide(path),
            name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .ok()
        .ok()?;
        let n = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

fn get_dword(path: &str, name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut len = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &wide(path),
            &wide(name),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut len),
        )
        .ok()
        .ok()?;
    }
    Some(value)
}

fn windows_build() -> u32 {
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            &wide("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .is_ok()
    };
    if !ok {
        return 0;
    }
    String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]).parse().unwrap_or(0)
}

const KEYBOARD_KEY: &str = r"Control Panel\Keyboard";

/// True when Windows' "Use the Print screen key to open screen capture" setting is on,
/// in which case the Snipping Tool gets the key before Zquig can. Windows 11 turns it on
/// by default (the value is absent); Windows 10 leaves it off.
fn snipping_owns_print_screen() -> bool {
    match get_dword(KEYBOARD_KEY, "PrintScreenKeyForSnippingEnabled") {
        Some(v) => v != 0,
        None => windows_build() >= 22000,
    }
}

/// Who has the Print Screen key, for the tray's Shortcuts menu, when it isn't Zquig.
/// `registered` is whether Zquig's own registration went through.
pub fn print_screen_taken_by(_registered: bool) -> Option<&'static str> {
    snipping_owns_print_screen().then_some("Snipping Tool \u{2014} see Settings > Accessibility > Keyboard")
}

/// Calls `on_change` (on a background thread) whenever the keyboard settings change.
pub fn watch_keyboard_settings(on_change: impl Fn() + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &wide(KEYBOARD_KEY), None, KEY_NOTIFY, &mut key).is_err() {
            return;
        }
        while RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false).is_ok() {
            on_change();
        }
        let _ = RegCloseKey(key);
    });
}

/// Space-to-preview in the file manager is Linux-only for now.
pub fn watch_file_manager(_on_peek: impl Fn(Vec<PathBuf>) + Send + 'static) {}

fn exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
}

/// Adds Zquig to Explorer's "Open with" list for images and PDFs (per user, no admin needed).
pub fn register_open_with() {
    let exe = exe();
    let command = format!("\"{exe}\" \"%1\"");
    const PROGID: &str = r"Software\Classes\Zquig.Image";
    if get_value(&format!(r"{PROGID}\shell\open\command"), None).as_deref() == Some(command.as_str()) {
        return; // already registered for this copy of the exe
    }
    set_value(PROGID, None, "Zquig Image");
    set_value(PROGID, Some("FriendlyTypeName"), "Zquig Image");
    set_value(&format!(r"{PROGID}\DefaultIcon"), None, &format!("\"{exe}\",0"));
    set_value(&format!(r"{PROGID}\shell\open\command"), None, &command);
    let app = r"Software\Classes\Applications\zquig.exe";
    set_value(app, Some("FriendlyAppName"), "Zquig");
    set_value(&format!(r"{app}\shell\open\command"), None, &command);
    for ext in crate::system::EXTENSIONS {
        set_value(&format!(r"{app}\SupportedTypes"), Some(&format!(".{ext}")), "");
        set_value(&format!(r"Software\Classes\.{ext}\OpenWithProgids"), Some("Zquig.Image"), "");
    }
    unsafe {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub const AUTOSTART_LABEL: &str = "Start with Windows";

pub fn starts_at_login() -> bool {
    get_value(RUN_KEY, Some("Zquig")).is_some()
}

pub fn set_starts_at_login(on: bool) {
    if on {
        set_value(RUN_KEY, Some("Zquig"), &format!("\"{}\" --background", exe()));
    } else {
        unsafe {
            let mut key = HKEY::default();
            if RegCreateKeyExW(HKEY_CURRENT_USER, &wide(RUN_KEY), None, None, REG_OPTION_NON_VOLATILE, KEY_READ | KEY_WRITE, None, &mut key, None)
                .is_ok()
            {
                let _ = RegDeleteValueW(key, &wide("Zquig"));
                let _ = RegCloseKey(key);
            }
        }
    }
}

// MARK: Clipboard

pub fn clipboard_has_image() -> bool {
    use clipboard_win::{formats, raw};
    raw::is_format_avail(formats::CF_DIB)
        || raw::is_format_avail(formats::CF_DIBV5)
        || raw::is_format_avail(formats::CF_BITMAP)
        || raw::register_format("PNG").is_some_and(|f| raw::is_format_avail(f.get()))
}

fn annotation_format() -> Option<u32> {
    clipboard_win::raw::register_format("Zquig.Annotation").map(|f| f.get())
}

/// Puts a copied annotation (as JSON) on the clipboard, plus its text for other apps if it has any.
pub fn copy_annotation(json: &str, text: Option<&str>) {
    let Some(fmt) = annotation_format() else { return };
    if let Ok(_clip) = clipboard_win::Clipboard::new_attempts(10) {
        let _ = clipboard_win::raw::empty();
        let _ = clipboard_win::raw::set_without_clear(fmt, json.as_bytes());
        if let Some(text) = text {
            let _ = clipboard_win::raw::set_string_with(text, clipboard_win::options::NoClear);
        }
    }
}

/// The JSON of an annotation copied by Zquig, if that's what the clipboard holds.
pub fn pasted_annotation() -> Option<String> {
    let fmt = annotation_format()?;
    if !clipboard_win::raw::is_format_avail(fmt) {
        return None;
    }
    let mut buf = Vec::new();
    let got = clipboard_win::Clipboard::new_attempts(10).is_ok_and(|_c| clipboard_win::raw::get_vec(fmt, &mut buf).is_ok());
    if !got {
        return None;
    }
    std::str::from_utf8(&buf).ok().map(|s| s.trim_end_matches('\0').to_owned())
}

// MARK: PDF (Windows' built-in renderer)

/// Stacks up to 50 pages vertically into a single image. Returns the image and its scale (pixels per point).
pub fn render_pdf(path: &Path) -> Result<(Pixmap, f32), String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};

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
