//! Screen capture: the whole virtual desktop in one grab, plus the visible windows for window mode.

use std::path::PathBuf;

use egui::{Rect, pos2};
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClassNameW, GetSystemMetrics, GetWindowLongW, IsIconic, IsWindowVisible, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

/// What the hotkeys and menu commands ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Ctrl+Shift+1: region to the clipboard.
    RegionToClipboard,
    /// Ctrl+Shift+2: full screen to the clipboard.
    ScreenToClipboard,
    /// Ctrl+Shift+3: full screen saved as a JPEG.
    ScreenToFile,
    /// Ctrl+Shift+4: region (Space for a window) saved as a JPEG.
    RegionToFile,
    /// Menu commands: open the capture in an editor window.
    RegionToEditor,
    WindowToEditor,
    ScreenToEditor,
}

impl Mode {
    pub fn is_interactive(self) -> bool {
        matches!(self, Mode::RegionToClipboard | Mode::RegionToFile | Mode::RegionToEditor | Mode::WindowToEditor)
    }
}

/// A frozen copy of the desktop. Rectangles are in physical pixels relative to `origin`'s monitor layout.
pub struct Desktop {
    pub image: Pixmap,
    /// Top-left of the virtual screen (can be negative with monitors left of / above the primary).
    pub origin: (i32, i32),
    pub monitors: Vec<Monitor>,
    /// Visible top-level windows, front to back, in image coordinates.
    pub windows: Vec<Rect>,
}

pub struct Monitor {
    /// In image coordinates.
    pub rect: Rect,
    pub work: Rect,
    pub scale: f32,
}

impl Desktop {
    /// Monitor scale at the given image point (pixels per point), for annotation sizing.
    pub fn scale_at(&self, r: Rect) -> f32 {
        let c = r.center();
        self.monitors
            .iter()
            .find(|m| m.rect.contains(c))
            .or(self.monitors.first())
            .map_or(1.0, |m| m.scale)
    }

    pub fn crop(&self, r: Rect) -> Option<Pixmap> {
        let r = r.intersect(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(self.image.width() as f32, self.image.height() as f32)));
        let ir = tiny_skia::IntRect::from_ltrb(
            r.min.x.round() as i32,
            r.min.y.round() as i32,
            r.max.x.round() as i32,
            r.max.y.round() as i32,
        )?;
        self.image.clone_rect(ir)
    }

    pub fn full(&self) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(self.image.width() as f32, self.image.height() as f32))
    }
}

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
            let list = &mut *(data.0 as *mut Vec<(RECT, RECT, f32)>);
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
                list.push((info.rcMonitor, info.rcWork, dx as f32 / 96.0));
            }
            true.into()
        }
    }
    let mut list: Vec<(RECT, RECT, f32)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list.into_iter()
        .map(|(r, w, s)| Monitor { rect: to_rect(r, origin), work: to_rect(w, origin), scale: s })
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

/// "Screenshot 2026-10-15 at 22.09.33.jpg", like macOS, with " (2)" etc. on a clash.
pub fn screenshot_path(ext: &str) -> PathBuf {
    let dir = screenshots_dir();
    let stamp = local_timestamp();
    let mut path = dir.join(format!("Screenshot {stamp}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("Screenshot {stamp} ({n}).{ext}"));
        n += 1;
    }
    path
}

fn local_timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} at {:02}.{:02}.{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Monitors in absolute physical coordinates.
pub fn monitor_list() -> Vec<Monitor> {
    monitors((0, 0))
}
