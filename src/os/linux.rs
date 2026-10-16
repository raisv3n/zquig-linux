//! Linux integration (X11): screen capture through the X server, XDG folders and desktop entries,
//! and poppler (`pdftoppm`) for PDFs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui::{Rect, pos2, vec2};
use tiny_skia::Pixmap;
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{Window, WindowAttributes};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, GrabMode, ImageFormat, ImageOrder, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MapState, ModMask, Window as XWindow,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::CURRENT_TIME;
use x11rb::rust_connection::RustConnection;

use crate::capture::{Desktop, Monitor};
use crate::imageio::{blank, pixmap_from_rgba, with_clipboard};

const ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

// MARK: Capture

struct X {
    conn: RustConnection,
    root: XWindow,
    width: u16,
    height: u16,
}

fn connect() -> Option<X> {
    let (conn, n) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(n)?;
    let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
    Some(X { conn, root, width, height })
}

impl X {
    fn atom(&self, name: &str) -> u32 {
        self.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.atom)
    }

    /// A 32-bit list property (CARDINAL, WINDOW or ATOM); empty when it isn't set.
    fn list(&self, win: XWindow, prop: u32) -> Vec<u32> {
        self.conn
            .get_property(false, win, prop, AtomEnum::ANY, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }
}

/// Zquig reads the screen from the X server. Under Wayland that only shows X11 apps, so don't pretend.
fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

pub const CAPTURE_FAILED: &str = "Zquig captures the screen through X11. If you're in a Wayland session, log in to an \
     X11 (Xorg) session instead; opening and annotating images works either way.";

pub fn grab() -> Option<Desktop> {
    if is_wayland() {
        return None;
    }
    let x = connect()?;
    let reply = x.conn.get_image(ImageFormat::Z_PIXMAP, x.root, 0, 0, x.width, x.height, !0).ok()?.reply().ok()?;
    let setup = x.conn.setup();
    let bpp = setup.pixmap_formats.iter().find(|f| f.depth == reply.depth)?.bits_per_pixel;
    let (w, h) = (x.width as u32, x.height as u32);
    let len = (w * h * 4) as usize;
    // 24- and 32-bit TrueColor, which is every desktop today; 16-bit displays aren't handled.
    if bpp != 32 || reply.data.len() < len {
        return None;
    }
    let msb = setup.image_byte_order == ImageOrder::MSB_FIRST;
    let mut data = reply.data;
    data.truncate(len);
    for px in data.as_chunks_mut::<4>().0 {
        // Each pixel is a 0x__RRGGBB word in the server's byte order.
        *px = if msb { [px[1], px[2], px[3], 255] } else { [px[2], px[1], px[0], 255] };
    }
    let image = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)?;
    // X11 puts the top-left of the whole desktop at 0,0.
    Some(Desktop { image, origin: (0, 0), monitors: monitors(&x), windows: windows(&x) })
}

fn monitors(x: &X) -> Vec<Monitor> {
    let mut list: Vec<Monitor> = x
        .conn
        .randr_get_monitors(x.root, true)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            r.monitors
                .iter()
                .map(|m| {
                    let rect = Rect::from_min_size(pos2(m.x as f32, m.y as f32), vec2(m.width as f32, m.height as f32));
                    // winit's scale is filled in later (`sync_scales`).
                    Monitor { rect, work: rect, scale: 1.0, primary: m.primary }
                })
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(x.width as f32, x.height as f32));
        list.push(Monitor { rect, work: rect, scale: 1.0, primary: true });
    }
    let (sw, sh) = (x.width as f32, x.height as f32);
    for s in struts(x) {
        let [l, r, t, b, ly0, ly1, ry0, ry1, tx0, tx1, bx0, bx1] = s.map(|v| v as f32);
        for m in &mut list {
            let (mr, w) = (m.rect, &mut m.work);
            let spans_x = |a: f32, b: f32| a < mr.max.x && b >= mr.min.x;
            let spans_y = |a: f32, b: f32| a < mr.max.y && b >= mr.min.y;
            if t > mr.min.y && spans_x(tx0, tx1) {
                w.min.y = w.min.y.max(t.min(mr.max.y));
            }
            if sh - b < mr.max.y && spans_x(bx0, bx1) {
                w.max.y = w.max.y.min((sh - b).max(mr.min.y));
            }
            if l > mr.min.x && spans_y(ly0, ly1) {
                w.min.x = w.min.x.max(l.min(mr.max.x));
            }
            if sw - r < mr.max.x && spans_y(ry0, ry1) {
                w.max.x = w.max.x.min((sw - r).max(mr.min.x));
            }
        }
    }
    for m in &mut list {
        if !m.work.is_positive() {
            m.work = m.rect;
        }
    }
    list
}

/// Screen edges that panels reserve, as `_NET_WM_STRUT_PARTIAL` values.
fn struts(x: &X) -> Vec<[u32; 12]> {
    let (partial, plain) = (x.atom("_NET_WM_STRUT_PARTIAL"), x.atom("_NET_WM_STRUT"));
    let (w, h) = (x.width as u32, x.height as u32);
    let mut out = Vec::new();
    for win in x.list(x.root, x.atom("_NET_CLIENT_LIST")) {
        let v = x.list(win, partial);
        if let Ok(s) = <[u32; 12]>::try_from(v.as_slice()) {
            out.push(s);
        } else if let [l, r, t, b] = x.list(win, plain)[..] {
            out.push([l, r, t, b, 0, h, 0, h, 0, w, 0, w]);
        }
    }
    out
}

/// Visible top-level windows, front to back, including their frames.
fn windows(x: &X) -> Vec<Rect> {
    let desktop = x.list(x.root, x.atom("_NET_CURRENT_DESKTOP")).first().copied();
    let (wm_desktop, state, hidden) = (x.atom("_NET_WM_DESKTOP"), x.atom("_NET_WM_STATE"), x.atom("_NET_WM_STATE_HIDDEN"));
    let (kind, dock, desk) = (x.atom("_NET_WM_WINDOW_TYPE"), x.atom("_NET_WM_WINDOW_TYPE_DOCK"), x.atom("_NET_WM_WINDOW_TYPE_DESKTOP"));
    let frame = x.atom("_NET_FRAME_EXTENTS");

    let mut clients = x.list(x.root, x.atom("_NET_CLIENT_LIST_STACKING"));
    let ewmh = !clients.is_empty();
    if !ewmh {
        // No EWMH window manager: fall back to the root's children (already in stacking order).
        clients = x.conn.query_tree(x.root).ok().and_then(|c| c.reply().ok()).map(|r| r.children).unwrap_or_default();
    }
    let mut out = Vec::new();
    for &win in clients.iter().rev() {
        if ewmh {
            if let Some(&d) = x.list(win, wm_desktop).first()
                && Some(d) != desktop
                && d != u32::MAX
            {
                continue; // on another workspace
            }
            if x.list(win, state).contains(&hidden) {
                continue; // minimised
            }
            if x.list(win, kind).iter().any(|&t| t == dock || t == desk) {
                continue; // panels and the desktop itself
            }
        }
        let Some(attrs) = x.conn.get_window_attributes(win).ok().and_then(|c| c.reply().ok()) else { continue };
        if attrs.map_state != MapState::VIEWABLE || attrs.override_redirect {
            continue;
        }
        let Some(geo) = x.conn.get_geometry(win).ok().and_then(|c| c.reply().ok()) else { continue };
        let Some(at) = x.conn.translate_coordinates(win, x.root, 0, 0).ok().and_then(|c| c.reply().ok()) else { continue };
        let [l, r, t, b] = <[u32; 4]>::try_from(x.list(win, frame).as_slice()).unwrap_or([0; 4]).map(|v| v as f32);
        let (px, py) = (at.dst_x as f32, at.dst_y as f32);
        let rect = Rect::from_min_max(pos2(px - l, py - t), pos2(px + geo.width as f32 + r, py + geo.height as f32 + b));
        if rect.width() > 8.0 && rect.height() > 8.0 {
            out.push(rect);
        }
    }
    out
}

/// Monitors in absolute physical coordinates.
pub fn monitor_list() -> Vec<Monitor> {
    connect().map(|x| monitors(&x)).unwrap_or_default()
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(fallback))
}

fn config_home() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

fn data_home() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// The user's Pictures folder from `user-dirs.dirs` (it may be localised), or ~/Pictures.
fn pictures_dir() -> PathBuf {
    let dirs = std::fs::read_to_string(config_home().join("user-dirs.dirs")).unwrap_or_default();
    for line in dirs.lines() {
        if let Some(v) = line.trim().strip_prefix("XDG_PICTURES_DIR=") {
            let v = v.trim().trim_matches('"');
            let path = match v.strip_prefix("$HOME") {
                Some(rest) => home().join(rest.trim_start_matches('/')),
                None => PathBuf::from(v),
            };
            if path.is_absolute() && path != home() {
                return path;
            }
        }
    }
    home().join("Pictures")
}

/// Pictures/Screenshots, created if needed.
pub fn screenshots_dir() -> PathBuf {
    let dir = pictures_dir().join("Screenshots");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn local_timestamp() -> String {
    // SAFETY: time and localtime_r only write to the locals passed in.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02} at {:02}.{:02}.{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

// MARK: Windows (the on-screen kind)

/// Vulkan, with OpenGL for older GPUs and virtual machines.
pub fn gpu_backends() -> egui_wgpu::wgpu::Backends {
    egui_wgpu::wgpu::Backends::VULKAN | egui_wgpu::wgpu::Backends::GL
}

/// WM_CLASS, which docks and the desktop entry's StartupWMClass match on.
pub fn editor_attrs(attrs: WindowAttributes) -> WindowAttributes {
    attrs.with_name("Zquig", "zquig")
}

/// The capture overlay bypasses the window manager, so it can cover every monitor and the panels
/// exactly and never shows up in the taskbar. It takes the keyboard itself (`grab_keyboard`).
pub fn overlay_attrs(attrs: WindowAttributes) -> WindowAttributes {
    editor_attrs(attrs).with_override_redirect(true)
}

/// The thumbnail toast: placed exactly where asked, no taskbar entry, never takes focus.
pub fn toast_attrs(attrs: WindowAttributes) -> WindowAttributes {
    overlay_attrs(attrs)
}

fn xlib() -> Option<&'static x11_dl::xlib::Xlib> {
    static XLIB: OnceLock<Option<x11_dl::xlib::Xlib>> = OnceLock::new();
    XLIB.get_or_init(|| x11_dl::xlib::Xlib::open().ok()).as_ref()
}

/// Gives the overlay the keyboard. Window managers don't focus override-redirect windows, so grab
/// it on winit's own connection (the key events then reach winit as usual). Fails while another
/// client holds a grab, e.g. the hotkey that started the capture until its keys are released.
pub fn grab_keyboard(window: &Window) -> bool {
    use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let (Ok(d), Ok(w)) = (window.display_handle(), window.window_handle()) else { return true };
    let (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(w)) = (d.as_raw(), w.as_raw()) else { return true };
    let (Some(display), Some(xlib)) = (d.display, xlib()) else { return true };
    use x11_dl::xlib::{CurrentTime, GrabModeAsync, GrabSuccess, RevertToParent, True};
    // SAFETY: winit's display and window are alive while `window` is, and this runs on the event-loop thread.
    unsafe {
        let dpy = display.as_ptr().cast();
        let ok = (xlib.XGrabKeyboard)(dpy, w.window, True, GrabModeAsync, GrabModeAsync, CurrentTime) == GrabSuccess;
        if ok {
            (xlib.XSetInputFocus)(dpy, w.window, RevertToParent, CurrentTime);
        }
        (xlib.XFlush)(dpy);
        ok
    }
}

fn fc_match(pattern: &str, format: &str) -> Option<String> {
    let out = Command::new("fc-match").args(["-f", format, pattern]).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !s.is_empty()).then_some(s)
}

/// The desktop's sans-serif face (usually Noto Sans or DejaVu Sans), in regular and bold.
pub fn ui_fonts() -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let read = |pattern: &str| {
        fc_match(pattern, "%{file}")
            .filter(|f| [".ttf", ".otf", ".ttc"].iter().any(|e| f.to_ascii_lowercase().ends_with(e)))
            .and_then(|f| std::fs::read(f).ok())
    };
    (read("sans-serif"), read("sans-serif:bold"))
}

/// The annotation text face: the desktop's bold sans-serif.
pub fn text_family() -> &'static str {
    static FAMILY: OnceLock<String> = OnceLock::new();
    FAMILY.get_or_init(|| fc_match("sans-serif:bold", "%{family[0]}").unwrap_or_else(|| "DejaVu Sans".into()))
}

pub fn open_folder(dir: &Path) {
    let _ = Command::new("xdg-open").arg(dir).spawn().map(|mut c| std::thread::spawn(move || c.wait()));
}

pub fn beep() {
    if let Some(x) = connect() {
        let _ = x.conn.bell(0);
        let _ = x.conn.flush();
    }
}

/// Whether Space in the file manager previews files (`watch_file_manager`).
pub const FILE_MANAGER_PREVIEW: bool = true;

/// Dragging files out needs an XDND drag source, which winit doesn't provide yet.
pub const CAN_DRAG_OUT: bool = false;

pub fn start_drag(_window: &Window, _path: PathBuf, _thumb_png: Vec<u8>) {}

// MARK: Settings and desktop integration

pub fn data_dir() -> PathBuf {
    config_home().join("zquig")
}

/// Quotes one Exec= argument (Desktop Entry spec: quoting, then the string escapes on top).
fn exec_arg(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '\\' => out.push_str("\\\\\\\\"),
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Paths that go into a desktop entry. A line break (or other control character) would start
/// a new key, so such paths are refused.
fn entry_safe(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

/// Writes `data` unless the file already holds exactly that. Returns whether it wrote.
fn write_if_changed(path: &Path, data: &[u8]) -> bool {
    if std::fs::read(path).is_ok_and(|old| old == data) {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, data).is_ok()
}

/// Installed from the .deb (or another package): the package provides the desktop entry and icon.
fn packaged() -> bool {
    std::env::current_exe().is_ok_and(|p| p.starts_with("/usr/"))
}

/// The icon for desktop entries: the packaged theme icon, or a copy in the user's data folder.
fn icon_path() -> PathBuf {
    if packaged() {
        return PathBuf::from("zquig");
    }
    let path = data_home().join("zquig").join("zquig.png");
    write_if_changed(&path, ICON_PNG);
    path
}

fn exe() -> Option<String> {
    std::env::current_exe().ok().map(|p| p.display().to_string())
}

const MIME_TYPES: &str = "image/png;image/jpeg;image/bmp;image/gif;image/tiff;image/webp;\
                          image/vnd.microsoft.icon;image/x-icon;application/pdf;";

/// Adds Zquig to the file manager's "Open with" list for images and PDFs, and to the app menu
/// (a desktop entry in ~/.local/share/applications; it doesn't become the default app).
pub fn register_open_with() {
    let apps = data_home().join("applications");
    if packaged() {
        // A per-user entry left by an earlier unpackaged copy would hide the package's own.
        let user = apps.join("zquig.desktop");
        if std::fs::read_to_string(&user).is_ok_and(|s| s.contains("GenericName=Screenshot Annotator")) {
            let _ = std::fs::remove_file(user);
            std::thread::spawn(move || Command::new("update-desktop-database").arg(apps).status());
        }
        return;
    }
    let Some(exe) = exe().filter(|e| entry_safe(e)) else { return };
    if !entry_safe(&icon_path().display().to_string()) {
        return;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Zquig\nGenericName=Screenshot Annotator\n\
         Comment=Capture the screen and mark up images and PDFs\nExec={} %F\nIcon={}\nTerminal=false\n\
         Categories=Graphics;2DGraphics;RasterGraphics;\nMimeType={MIME_TYPES}\nStartupWMClass=Zquig\n",
        exec_arg(&exe),
        icon_path().display()
    );
    if write_if_changed(&apps.join("zquig.desktop"), entry.as_bytes()) {
        // Refreshes the MIME cache so "Open with" picks Zquig up straight away.
        std::thread::spawn(move || Command::new("update-desktop-database").arg(apps).status());
    }
}

pub const AUTOSTART_LABEL: &str = "Start at Login";

fn autostart_file() -> PathBuf {
    config_home().join("autostart").join("zquig.desktop")
}

pub fn starts_at_login() -> bool {
    std::fs::read_to_string(autostart_file()).is_ok_and(|s| !s.lines().any(|l| l.trim() == "Hidden=true"))
}

pub fn set_starts_at_login(on: bool) {
    let path = autostart_file();
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Some(exe) = exe().filter(|e| entry_safe(e)) else { return };
    if !entry_safe(&icon_path().display().to_string()) {
        return;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Zquig\nComment=Screenshot shortcuts in the tray\n\
         Exec={} --background\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_arg(&exe),
        icon_path().display()
    );
    write_if_changed(&path, entry.as_bytes());
}

/// Who has the Print Screen key, for the tray's Shortcuts menu, when it isn't Zquig. Desktops
/// usually bind it to their own screenshot tool.
pub fn print_screen_taken_by(registered: bool) -> Option<&'static str> {
    if registered {
        return None;
    }
    let lxqt = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_ascii_lowercase().contains("lxqt"));
    Some(if lxqt {
        "another app \u{2014} see LXQt Settings > Shortcut Keys"
    } else {
        "another app \u{2014} see your desktop's keyboard shortcuts"
    })
}

/// Nothing to watch: Print Screen is retaken only on Windows, where a setting hands it to the Snipping Tool.
pub fn watch_keyboard_settings(_on_change: impl Fn() + Send + 'static) {}

// MARK: Clipboard

pub fn clipboard_has_image() -> bool {
    with_clipboard(|c| c.get_image().is_ok()).unwrap_or(false)
}

/// The last annotation Zquig copied, with the plain text it put on the clipboard alongside.
/// X11 has no cheap way to offer a private format through arboard, and every Zquig window lives in
/// this one process, so the annotation stays here; the clipboard text tells whether it's still current.
static COPIED: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn copy_annotation(json: &str, text: Option<&str>) {
    let text = text.unwrap_or_default().to_owned();
    with_clipboard(|c| if text.is_empty() { c.clear() } else { c.set_text(text.clone()) });
    *COPIED.lock().unwrap_or_else(|e| e.into_inner()) = Some((json.to_owned(), text));
}

pub fn pasted_annotation() -> Option<String> {
    let (json, text) = COPIED.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let current = with_clipboard(|c| c.get_text().unwrap_or_default()).unwrap_or_default();
    (current == text && !clipboard_has_image()).then_some(json)
}

// MARK: PDF (poppler)

/// Runs a poppler tool on an untrusted PDF, giving up after a minute so a crafted file can't
/// keep it busy forever. stdout and stderr are drained on threads, so a chatty tool can't stall.
fn run_limited(cmd: &mut Command) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_| POPPLER_MISSING.to_string())?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                // Keep at most 1 MB; poppler only prints a few lines.
                let _ = p.by_ref().take(1 << 20).read_to_end(&mut buf);
                let _ = std::io::copy(&mut p, &mut std::io::sink());
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The PDF took too long to open.".into());
            }
        }
    };
    Ok(std::process::Output { status, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

/// A new folder in /tmp, with a random name, that only this user can open. ($XDG_RUNTIME_DIR is
/// private too, but it's a small RAM disk, and 50 rendered pages can be large.)
fn private_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    use std::hash::{BuildHasher, Hasher};
    use std::os::unix::fs::DirBuilderExt;
    let base = std::env::temp_dir();
    let mut last = String::new();
    for attempt in 0..10u32 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(attempt);
        let dir = base.join(format!("{prefix}-{}-{:016x}", std::process::id(), h.finish()));
        // create, not create_all: fails rather than reuse anything already there.
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// Page sizes in screen points (1/96 inch), as displayed (rotation applied).
fn pdf_pages(path: &Path) -> Result<Vec<(f32, f32)>, String> {
    let out = run_limited(Command::new("pdfinfo").args(["-f", "1", "-l", "50"]).arg(path))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        if stderr.contains("password") {
            return Err("Cannot open encrypted PDF".into());
        }
        return Err(stderr.lines().next().unwrap_or("pdfinfo couldn't read the file.").to_string());
    }
    let mut pages: Vec<(f32, f32)> = Vec::new();
    for line in stdout.lines() {
        // "Page    1 size: 612 x 792 pts (letter)" and "Page    1 rot:  90"
        let Some(rest) = line.strip_prefix("Page") else { continue };
        let mut parts = rest.split_whitespace();
        let (Some(_n), Some(field)) = (parts.next(), parts.next()) else { continue };
        let nums: Vec<f32> = parts.filter_map(|p| p.parse().ok()).collect();
        match (field, nums.as_slice()) {
            ("size:", [w, h, ..]) => pages.push((w * 96.0 / 72.0, h * 96.0 / 72.0)),
            ("rot:", [r, ..]) if (*r as i32).rem_euclid(180) == 90 => {
                if let Some(p) = pages.last_mut() {
                    *p = (p.1, p.0);
                }
            }
            _ => {}
        }
    }
    Ok(pages)
}

const POPPLER_MISSING: &str = "Opening PDFs needs poppler-utils (pdfinfo and pdftoppm). Install it with: \
                               sudo apt install poppler-utils";

/// Stacks up to 50 pages vertically into a single image. Returns the image and its scale (pixels per point).
pub fn render_pdf(path: &Path) -> Result<(Pixmap, f32), String> {
    // A path starting with "-" would read as an option.
    let path = crate::system::absolute(path);
    let pages = pdf_pages(&path)?;
    if pages.is_empty() {
        return Err("The PDF has no pages.".into());
    }
    let width = pages.iter().map(|p| p.0).fold(0.0, f32::max);
    let gap = 12.0;
    let height = pages.iter().map(|p| p.1).sum::<f32>() + gap * (pages.len() - 1) as f32;
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

    let dir = private_temp_dir("zquig-pdf")?;
    let result = render_pages(&path, &dir, pages.len(), scale, width, height, gap);
    let _ = std::fs::remove_dir_all(&dir);
    result.map(|img| (img, scale))
}

fn render_pages(path: &Path, dir: &Path, count: usize, scale: f32, width: f32, height: f32, gap: f32) -> Result<Pixmap, String> {
    let out = run_limited(
        Command::new("pdftoppm")
            .args(["-png", "-f", "1", "-l", &count.to_string(), "-r", &format!("{:.3}", 96.0 * scale)])
            .arg(path)
            .arg(dir.join("page")),
    )?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("pdftoppm couldn't render the file.").to_string());
    }
    // page-1.png, page-01.png, ... (padded to the page count's width), so sort by page number.
    let mut files: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let n = p.file_stem()?.to_str()?.strip_prefix("page-")?.parse().ok()?;
            Some((n, p))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err("pdftoppm didn't produce any pages.".into());
    }
    let mut out = blank((width * scale).ceil() as u32, (height * scale).ceil() as u32);
    let mut top = 0.0;
    for (_, file) in files {
        let img = image::open(&file).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (img.width() as f32, img.height() as f32);
        if let Some(pm) = pixmap_from_rgba(img) {
            let x = ((out.width() as f32 - w) / 2.0).round() as i32;
            out.draw_pixmap(x, top as i32, pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
        top += h + (gap * scale).round();
    }
    Ok(out)
}

// MARK: Space to preview in the file manager

/// File managers whose Ctrl+C puts the selected files on the clipboard as a URI list (WM_CLASS).
const FILE_MANAGERS: [&str; 6] = ["pcmanfm-qt", "pcmanfm", "thunar", "nemo", "caja", "dolphin"];

/// While a file manager is focused, Space on files that Zquig opens by default previews them
/// (`on_peek`, on a background thread). Anything else gets an ordinary Space.
///
/// File managers don't publish their selection, so Zquig asks for it the way a user would: it
/// sends Ctrl+C, reads the copied files and puts the previous clipboard back. When Ctrl+C copies
/// nothing (a text field without a selection), or copies something else, the clipboard is left
/// alone or restored, and the Space and anything typed while Zquig was checking are replayed in order.
pub fn watch_file_manager(on_peek: impl Fn(Vec<PathBuf>) + Send + 'static) {
    if is_wayland() {
        return;
    }
    std::thread::spawn(move || {
        if let Some(mut w) = Watcher::new() {
            w.run(&on_peek);
        }
    });
}

struct Watcher {
    x: X,
    active_window: u32,
    clipboard: u32,
    space: u8,
    ctrl: u8,
    c: u8,
    grabbed: bool,
}

/// Caps Lock and Num Lock combinations, so Space is caught whatever their state.
fn lock_states() -> [ModMask; 4] {
    [ModMask::from(0u16), ModMask::LOCK, ModMask::M2, ModMask::LOCK | ModMask::M2]
}

fn keycode(x: &X, keysym: u32) -> Option<u8> {
    let setup = x.conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = x.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
    let per = map.keysyms_per_keycode.max(1) as usize;
    map.keysyms.chunks(per).position(|syms| syms.first() == Some(&keysym)).map(|i| min + i as u8)
}

impl Watcher {
    fn new() -> Option<Self> {
        let x = connect()?;
        x.conn.xfixes_query_version(5, 0).ok()?.reply().ok()?;
        x.conn.xtest_get_version(2, 2).ok()?.reply().ok()?;
        let (active_window, clipboard) = (x.atom("_NET_ACTIVE_WINDOW"), x.atom("CLIPBOARD"));
        // XK_space, XK_Control_L, XK_c
        let (space, ctrl, c) = (keycode(&x, 0x20)?, keycode(&x, 0xffe3)?, keycode(&x, 0x63)?);
        x.conn.change_window_attributes(x.root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE)).ok()?;
        x.conn.xfixes_select_selection_input(x.root, clipboard, SelectionEventMask::SET_SELECTION_OWNER).ok()?;
        x.conn.flush().ok()?;
        Some(Self { x, active_window, clipboard, space, ctrl, c, grabbed: false })
    }

    fn run(&mut self, on_peek: &dyn Fn(Vec<PathBuf>)) {
        self.refresh();
        while let Ok(event) = self.x.conn.wait_for_event() {
            match event {
                Event::PropertyNotify(e) if e.atom == self.active_window => self.refresh(),
                Event::KeyPress(e) if e.detail == self.space => self.on_space(on_peek),
                _ => {}
            }
        }
    }

    fn file_manager_focused(&self) -> bool {
        let Some(&win) = self.x.list(self.x.root, self.active_window).first() else { return false };
        let class = self
            .x
            .conn
            .get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default();
        class.split(|&b| b == 0).any(|part| FILE_MANAGERS.iter().any(|f| f.as_bytes().eq_ignore_ascii_case(part)))
    }

    /// Takes Space while a file manager is focused, and gives it back otherwise.
    fn refresh(&mut self) {
        let want = self.file_manager_focused();
        if want != self.grabbed {
            self.set_grab(want);
        }
    }

    fn set_grab(&mut self, on: bool) {
        let (conn, root) = (&self.x.conn, self.x.root);
        for m in lock_states() {
            let _ = if on {
                conn.grab_key(false, root, m, self.space, GrabMode::ASYNC, GrabMode::ASYNC)
            } else {
                conn.ungrab_key(self.space, root, m)
            };
        }
        let _ = conn.flush();
        self.grabbed = on;
    }

    fn fake(&self, keys: &[(bool, u8)]) {
        for &(press, key) in keys {
            let kind = if press { KEY_PRESS_EVENT } else { KEY_RELEASE_EVENT };
            let _ = self.x.conn.xtest_fake_input(kind, key, CURRENT_TIME, self.x.root, 0, 0, 0);
        }
        let _ = self.x.conn.flush();
    }

    /// A round trip, so everything sent so far has been handled by the server.
    fn sync(&self) {
        let _ = self.x.conn.get_input_focus().ok().and_then(|c| c.reply().ok());
    }

    fn on_space(&mut self, on_peek: &dyn Fn(Vec<PathBuf>)) {
        let (conn, root) = (&self.x.conn, self.x.root);
        // Clipboard changes from an earlier round (our own restore) mustn't count as this copy.
        self.sync();
        while let Ok(Some(_)) = conn.poll_for_event() {}

        // The Space press holds the keyboard; let the file manager have it for the Ctrl+C.
        let _ = conn.ungrab_keyboard(CURRENT_TIME);
        let Some(saved) = Saved::take() else {
            // The clipboard's owner isn't answering, so it couldn't be put back: no preview this time.
            return self.pass_on(Vec::new());
        };
        self.fake(&[(true, self.ctrl), (true, self.c), (false, self.c), (false, self.ctrl)]);
        self.sync();
        // Hold on to anything typed while waiting, to replay it in order.
        let _ = conn.grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
        let _ = conn.flush();
        let mut typed = Vec::new();
        let copied = self.wait_for_copy(&mut typed);
        let files = if copied { copied_files() } else { Vec::new() };
        let _ = conn.ungrab_keyboard(CURRENT_TIME);
        let _ = conn.flush();
        // Put the clipboard back once the keyboard is free again (it can wait on the clipboard lock).
        let restore = move || {
            if copied {
                saved.restore();
            }
        };

        let previewable: Vec<PathBuf> = files.into_iter().filter(|p| p.is_file() && zquig_is_default(p)).collect();
        if !previewable.is_empty() {
            on_peek(previewable);
            restore();
            return;
        }
        // Not ours: the file manager gets its Space, then whatever was typed meanwhile. If focus
        // has moved on (a dialog, another app), drop them rather than type into the wrong window.
        if !self.file_manager_focused() {
            self.refresh();
            restore();
            return;
        }
        self.pass_on(typed);
        restore();
    }

    /// Gives the focused window the Space Zquig took, then `typed`.
    fn pass_on(&mut self, typed: Vec<(bool, u8)>) {
        self.set_grab(false);
        let mut keys = vec![(true, self.space), (false, self.space)];
        keys.extend(typed);
        self.fake(&keys);
        self.sync();
        // Let the replayed Space go through before Space is taken again.
        std::thread::sleep(Duration::from_millis(30));
        self.refresh();
    }

    /// Waits briefly for the file manager to take the clipboard, collecting keys typed meanwhile.
    fn wait_for_copy(&self, typed: &mut Vec<(bool, u8)>) -> bool {
        let deadline = Instant::now() + Duration::from_millis(200);
        let mut copied = false;
        while !copied && Instant::now() < deadline {
            while let Ok(Some(event)) = self.x.conn.poll_for_event() {
                match event {
                    Event::XfixesSelectionNotify(e) if e.selection == self.clipboard => copied = true,
                    Event::KeyPress(e) => typed.push((true, e.detail)),
                    Event::KeyRelease(e) => typed.push((false, e.detail)),
                    _ => {}
                }
            }
            if !copied {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        copied
    }
}

/// The files the file manager just copied. A file manager that doesn't answer could keep the
/// clipboard read waiting for seconds, with the keyboard held, so give up after 0.3 s.
fn copied_files() -> Vec<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(with_clipboard(|c| c.get().file_list().ok()).flatten().unwrap_or_default());
    });
    rx.recv_timeout(Duration::from_millis(300)).unwrap_or_default()
}

/// What was on the clipboard before Zquig's Ctrl+C, to put back afterwards.
enum Saved {
    Files(Vec<PathBuf>),
    Text(String),
    Image(arboard::ImageData<'static>),
    Empty,
}

impl Saved {
    /// None if the clipboard's owner doesn't hand its contents over within 0.3 s.
    fn take() -> Option<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Self::read());
        });
        rx.recv_timeout(Duration::from_millis(300)).ok()
    }

    fn read() -> Self {
        with_clipboard(|c| {
            if let Ok(files) = c.get().file_list()
                && !files.is_empty()
            {
                return Saved::Files(files);
            }
            if let Ok(text) = c.get_text() {
                return Saved::Text(text);
            }
            match c.get_image() {
                Ok(img) => Saved::Image(img),
                Err(_) => Saved::Empty,
            }
        })
        .unwrap_or(Saved::Empty)
    }

    fn restore(self) {
        with_clipboard(|c| {
            let _ = match self {
                Saved::Files(f) => c.set().file_list(&f),
                Saved::Text(t) => c.set_text(t),
                Saved::Image(i) => c.set_image(i),
                Saved::Empty => c.clear(),
            };
        });
    }
}

// MARK: Default apps

fn mime_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "ico" => "image/vnd.microsoft.icon",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

fn dirs_var(var: &str, default: &str) -> Vec<PathBuf> {
    let v = std::env::var(var).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.into());
    v.split(':').map(PathBuf::from).filter(|p| p.is_absolute()).collect()
}

/// The `[Default Applications]` entry for `mime`, searched in the XDG mimeapps.list order
/// (desktop-specific lists such as lxqt-mimeapps.list first). The first installed app wins.
fn default_app(mime: &str) -> Option<String> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let app_dirs: Vec<PathBuf> =
        std::iter::once(data_home()).chain(dirs_var("XDG_DATA_DIRS", "/usr/local/share:/usr/share")).map(|d| d.join("applications")).collect();
    let config_dirs = std::iter::once(config_home()).chain(dirs_var("XDG_CONFIG_DIRS", "/etc/xdg"));
    let mut lists = Vec::new();
    for dir in config_dirs.chain(app_dirs.iter().cloned()) {
        lists.extend(desktops.iter().map(|d| dir.join(format!("{d}-mimeapps.list"))));
        lists.push(dir.join("mimeapps.list"));
    }
    let installed = |id: &str| app_dirs.iter().any(|d| d.join(id).is_file());
    for list in lists {
        let Ok(text) = std::fs::read_to_string(&list) else { continue };
        let mut section = "";
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line;
            } else if section == "[Default Applications]"
                && let Some((key, value)) = line.split_once('=')
                && key.trim() == mime
                && let Some(app) = value.split(';').map(str::trim).find(|a| !a.is_empty() && installed(a))
            {
                return Some(app.to_owned());
            }
        }
    }
    None
}

/// Whether double-clicking the file in the file manager would open it in Zquig.
fn zquig_is_default(path: &Path) -> bool {
    mime_type(path).is_some_and(|m| default_app(m).as_deref() == Some("zquig.desktop"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_args_are_quoted_and_escaped() {
        assert_eq!(exec_arg("/usr/bin/zquig"), "\"/usr/bin/zquig\"");
        assert_eq!(exec_arg("/a b/$x\"`%"), "\"/a b/\\\\$x\\\\\"\\\\`%%\"");
        assert_eq!(exec_arg("/a\\b"), "\"/a\\\\\\\\b\"");
        assert!(entry_safe("/home/me/Zquig/zquig"));
        assert!(!entry_safe("/tmp/x\nExec=evil"));
    }

    #[test]
    fn private_temp_dirs_are_new_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let a = private_temp_dir("zquig-test").unwrap();
        let b = private_temp_dir("zquig-test").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir(a);
        let _ = std::fs::remove_dir(b);
    }

    #[test]
    fn mime_types_for_default_app_lookup() {
        assert_eq!(mime_type(Path::new("/x/A.JPG")), Some("image/jpeg"));
        assert_eq!(mime_type(Path::new("/x/a.pdf")), Some("application/pdf"));
        assert_eq!(mime_type(Path::new("/x/a.mp4")), None);
    }
}
