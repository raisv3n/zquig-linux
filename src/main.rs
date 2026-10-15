#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annotation;
mod canvas;
mod capture;
mod document;
mod imageio;
mod overlay;
mod render;
#[cfg(debug_assertions)]
mod replay;
mod system;
mod text;
mod tiles;
mod toast;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{ViewportId, ViewportIdSet, ViewportInfo};
use egui_wgpu::winit::Painter;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tiny_skia::Pixmap;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Icon, Window, WindowAttributes, WindowId, WindowLevel};

use capture::Mode;
use document::{Action, Document};
use overlay::{Outcome, Overlay};
use system::{Prefs, Request};
use toast::Toast;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

enum UserEvent {
    Repaint(ViewportId, Duration),
    Hotkey(u32),
    TrayClick,
    Menu(MenuId),
    Remote(Request),
    PdfOpened(PathBuf, Result<(Pixmap, f32), String>),
    Capture(Mode),
    KeyboardSettingsChanged,
    RetakePrintScreen,
}

enum Kind {
    Doc(Box<Document>),
    Overlay(Box<Overlay>),
    Toast(Box<Toast>),
}

struct Win {
    window: Arc<Window>,
    state: egui_winit::State,
    viewport: ViewportId,
    info: ViewportInfo,
    kind: Kind,
    repaint_at: Option<Instant>,
}

struct TrayIds {
    new: MenuId,
    region: MenuId,
    window: MenuId,
    screen: MenuId,
    open: MenuId,
    folder: MenuId,
    autostart: MenuId,
    quit: MenuId,
}

struct App {
    proxy: Arc<Mutex<EventLoopProxy<UserEvent>>>,
    ctx: egui::Context,
    painter: Option<Painter>,
    wins: HashMap<WindowId, Win>,
    next_id: u64,
    counter: u32,
    prefs: Prefs,
    startup: Option<Request>,
    tray: Option<TrayIcon>,
    tray_ids: Option<TrayIds>,
    autostart_item: Option<CheckMenuItem>,
    hotkeys: Option<GlobalHotKeyManager>,
    hotkey_modes: HashMap<u32, Mode>,
    print_screen_item: MenuItem,
    /// Editor windows hidden while an editor capture is in progress.
    hidden: Vec<WindowId>,
    capturing: bool,
    #[cfg(debug_assertions)]
    replay: Option<replay::Replay>,
}

/// DirectX 12 (OpenGL as a fallback). wgpu would otherwise try Vulkan first, and Vulkan overlay
/// layers that some apps install (game recorders, chat overlays) can hang device creation.
/// A screenshot tool doesn't need the discrete GPU, so prefer the power-saving one.
fn gpu_config() -> egui_wgpu::WgpuConfiguration {
    use egui_wgpu::wgpu;
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::DX12 | wgpu::Backends::GL);
    setup.power_preference = wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
    egui_wgpu::WgpuConfiguration { wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() }
}

fn app_icon() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    let small = image::imageops::resize(&img, 64, 64, image::imageops::FilterType::Lanczos3);
    Some((small.into_raw(), 64, 64))
}

impl App {
    fn send(&self, e: UserEvent) {
        let _ = self.proxy.lock().unwrap().send_event(e);
    }

    // MARK: Windows

    fn add_window(&mut self, el: &ActiveEventLoop, attrs: WindowAttributes, kind: Kind) -> Option<WindowId> {
        let window = Arc::new(el.create_window(attrs.with_visible(false)).ok()?);
        let viewport = ViewportId::from_hash_of(("zquig", self.next_id));
        self.next_id += 1;
        let painter = self.painter.get_or_insert_with(|| {
            pollster::block_on(Painter::new(
                self.ctx.clone(),
                gpu_config(),
                false,
                egui_wgpu::RendererOptions::default(),
            ))
        });
        if let Err(e) = pollster::block_on(painter.set_window(viewport, Some(window.clone()))) {
            document::error_box(None, "Couldn't start the graphics renderer", &e.to_string());
            return None;
        }
        let state = egui_winit::State::new(
            self.ctx.clone(),
            viewport,
            el,
            Some(window.scale_factor() as f32),
            window.theme(),
            painter.max_texture_side(),
        );
        let mut info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
        let id = window.id();
        self.wins.insert(id, Win { window, state, viewport, info, kind, repaint_at: None });
        // Paint before showing, so the window never flashes white.
        self.redraw(el, id);
        self.redraw(el, id);
        if let Some(w) = self.wins.get(&id) {
            w.window.set_visible(true);
            if !matches!(w.kind, Kind::Toast(_)) {
                w.window.focus_window();
            }
            w.window.request_redraw();
        }
        Some(id)
    }

    fn remove_window(&mut self, id: WindowId) {
        self.wins.remove(&id);
        let active: ViewportIdSet = self.wins.values().map(|w| w.viewport).collect();
        if let Some(p) = &mut self.painter {
            p.gc_viewports(&active);
        }
    }

    fn primary_work_area() -> (egui::Rect, f32) {
        let monitors = capture::monitor_list();
        let primary = monitors.iter().find(|m| m.rect.contains(egui::pos2(0.0, 0.0))).or(monitors.first());
        primary.map_or((egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)), 1.0), |m| (m.work, m.scale))
    }

    fn next_title(&mut self) -> String {
        self.counter += 1;
        format!("Snap {}", self.counter)
    }

    fn present(&mut self, el: &ActiveEventLoop, image: Pixmap, scale: f32, title: String, path: Option<PathBuf>) {
        let doc = Document::new(image, scale, title.clone(), path, &self.prefs, self.next_id);
        let (work, mscale) = Self::primary_work_area();
        let size = doc.preferred_size(work.size() / mscale);
        let phys = size * mscale;
        let pos = work.center() - phys / 2.0;
        let mut attrs = Window::default_attributes()
            .with_title(doc.window_title())
            .with_inner_size(LogicalSize::new(size.x, size.y))
            .with_min_inner_size(LogicalSize::new(560.0, 360.0))
            .with_position(PhysicalPosition::new(pos.x.max(work.min.x) as i32, pos.y.max(work.min.y) as i32));
        if let Some((rgba, w, h)) = app_icon() {
            attrs = attrs.with_window_icon(Icon::from_rgba(rgba, w, h).ok());
        }
        self.add_window(el, attrs, Kind::Doc(Box::new(doc)));
    }

    fn new_from_clipboard_or_blank(&mut self, el: &ActiveEventLoop) {
        let title = self.next_title();
        match imageio::clipboard_image() {
            Some(img) => self.present(el, img, 1.0, title, None),
            None => self.present(el, imageio::blank(800, 500), 1.0, title, None),
        }
    }

    fn open_path(&mut self, el: &ActiveEventLoop, path: &Path) {
        let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if imageio::is_pdf(path) {
            let proxy = self.proxy.clone();
            let path = path.to_path_buf();
            std::thread::spawn(move || {
                let result = imageio::render_pdf(&path);
                let _ = proxy.lock().unwrap().send_event(UserEvent::PdfOpened(path, result));
            });
            return;
        }
        match imageio::load_image(path) {
            Ok(img) => self.present(el, img, 1.0, title, Some(path.to_path_buf())),
            Err(e) => document::error_box(None, &format!("Cannot open {}", path.display()), &e),
        }
    }

    fn open_dialog(&mut self, el: &ActiveEventLoop) {
        let mut exts: Vec<&str> = system::EXTENSIONS.to_vec();
        exts.push("jfif");
        if let Some(files) = rfd::FileDialog::new().add_filter("Images and PDFs", &exts).pick_files() {
            for f in files {
                self.open_path(el, &f);
            }
        }
    }

    fn show_all(&mut self, el: &ActiveEventLoop) {
        let docs: Vec<&Win> = self.wins.values().filter(|w| matches!(w.kind, Kind::Doc(_))).collect();
        if docs.is_empty() {
            self.new_from_clipboard_or_blank(el);
            return;
        }
        for w in docs {
            w.window.set_minimized(false);
            w.window.focus_window();
        }
    }

    fn close_doc(&mut self, id: WindowId) -> bool {
        let Some(w) = self.wins.get_mut(&id) else { return true };
        if let Kind::Doc(doc) = &mut w.kind
            && !doc.confirm_close(&w.window)
        {
            return false;
        }
        self.remove_window(id);
        true
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        let ids: Vec<WindowId> = self.wins.keys().copied().collect();
        for id in ids {
            if !self.close_doc(id) {
                return; // cancelled
            }
        }
        el.exit();
    }

    // MARK: Capture

    fn start_capture(&mut self, mode: Mode) {
        if self.capturing {
            return;
        }
        self.capturing = true;
        let editor = matches!(mode, Mode::RegionToEditor | Mode::WindowToEditor | Mode::ScreenToEditor);
        if !editor {
            self.send(UserEvent::Capture(mode));
            return;
        }
        // Menu captures hide Zquig's own windows first, like the Mac app.
        for (id, w) in &self.wins {
            if matches!(w.kind, Kind::Doc(_)) && w.window.is_visible().unwrap_or(true) {
                w.window.set_visible(false);
                self.hidden.push(*id);
            }
        }
        let proxy = self.proxy.clone();
        let wait = if self.hidden.is_empty() { 0 } else if mode == Mode::ScreenToEditor { 400 } else { 250 };
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(wait));
            let _ = proxy.lock().unwrap().send_event(UserEvent::Capture(mode));
        });
    }

    fn restore_hidden(&mut self) {
        for id in std::mem::take(&mut self.hidden) {
            if let Some(w) = self.wins.get(&id) {
                w.window.set_visible(true);
            }
        }
    }

    fn do_capture(&mut self, el: &ActiveEventLoop, mode: Mode) {
        let Some(desktop) = capture::grab() else {
            self.capturing = false;
            self.restore_hidden();
            document::error_box(None, "Couldn't capture the screen", "Windows didn't return an image.");
            return;
        };
        if !mode.is_interactive() {
            let full = desktop.full();
            self.finish_capture(el, mode, &desktop, full);
            return;
        }
        let (x, y) = desktop.origin;
        let (w, h) = (desktop.image.width(), desktop.image.height());
        let attrs = Window::default_attributes()
            .with_title("Zquig Capture")
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_skip_taskbar(true)
            .with_position(PhysicalPosition::new(x, y))
            .with_inner_size(PhysicalSize::new(w, h));
        if self.add_window(el, attrs, Kind::Overlay(Box::new(Overlay::new(desktop, mode)))).is_none() {
            self.capturing = false;
            self.restore_hidden();
        }
    }

    fn finish_capture(&mut self, el: &ActiveEventLoop, mode: Mode, desktop: &capture::Desktop, rect: egui::Rect) {
        self.capturing = false;
        self.restore_hidden();
        let Some(image) = desktop.crop(rect) else { return };
        let scale = desktop.scale_at(rect);
        match mode {
            Mode::RegionToClipboard | Mode::ScreenToClipboard => {
                if let Err(e) = imageio::copy_image(&image) {
                    document::error_box(None, "Couldn't copy the screenshot", &e);
                    return;
                }
                self.show_toast(el, desktop, Toast::new(image, scale, None, "Copied to clipboard".into()));
            }
            Mode::ScreenToFile | Mode::RegionToFile => {
                let path = capture::screenshot_path("jpg");
                let (img, target) = (image.clone(), path.clone());
                std::thread::spawn(move || {
                    if let Err(e) = imageio::write_atomic(&target, &imageio::encode_jpeg(&img, 90)) {
                        document::error_box(None, &format!("Couldn't save {}", target.display()), &e);
                    }
                });
                self.show_toast(el, desktop, Toast::new(image, scale, Some(path), "Saved to Screenshots".into()));
            }
            Mode::RegionToEditor | Mode::WindowToEditor | Mode::ScreenToEditor => {
                let title = self.next_title();
                self.present(el, image, scale, title, None);
            }
        }
    }

    fn show_toast(&mut self, el: &ActiveEventLoop, desktop: &capture::Desktop, toast: Toast) {
        let old: Vec<WindowId> = self.wins.iter().filter(|(_, w)| matches!(w.kind, Kind::Toast(_))).map(|(id, _)| *id).collect();
        for id in old {
            self.remove_window(id);
        }
        // Bottom-right of the primary monitor's work area.
        let (ox, oy) = desktop.origin;
        let primary = desktop
            .monitors
            .iter()
            .find(|m| m.rect.contains(egui::pos2(-ox as f32, -oy as f32)))
            .or(desktop.monitors.first());
        let (work, s) = primary.map_or((desktop.full(), 1.0), |m| (m.work, m.scale));
        let size = toast::SIZE * s;
        let margin = 16.0 * s;
        let pos = work.max - size - egui::vec2(margin, margin);
        let attrs = Window::default_attributes()
            .with_title("Zquig")
            .with_decorations(false)
            .with_resizable(false)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_skip_taskbar(true)
            .with_inner_size(PhysicalSize::new(size.x as u32, size.y as u32))
            .with_position(PhysicalPosition::new((pos.x as i32) + ox, (pos.y as i32) + oy));
        self.add_window(el, attrs, Kind::Toast(Box::new(toast)));
    }

    // MARK: Frame

    fn redraw(&mut self, el: &ActiveEventLoop, id: WindowId) {
        #[cfg(debug_assertions)]
        let overlay_open = self.wins.values().any(|w| matches!(w.kind, Kind::Overlay(_)));
        let Some(painter) = &mut self.painter else { return };
        let Some(w) = self.wins.get_mut(&id) else { return };
        w.repaint_at = None;
        egui_winit::update_viewport_info(&mut w.info, &self.ctx, &w.window, false);
        let mut input = w.state.take_egui_input(&w.window);
        input.viewports.insert(w.viewport, w.info.clone());
        #[cfg(debug_assertions)]
        let replaying = match w.kind {
            Kind::Overlay(_) => true,
            Kind::Doc(_) => !overlay_open,
            Kind::Toast(_) => false,
        } && self.replay.as_mut().is_some_and(|r| r.feed(&mut input));
        let mut actions = Vec::new();
        let window = w.window.clone();
        let mut out = self.ctx.run_ui(input, |ui| match &mut w.kind {
            Kind::Doc(doc) => doc.ui(ui, &window, &mut actions),
            Kind::Overlay(o) => o.ui(ui),
            Kind::Toast(t) => t.ui(ui),
        });
        w.state.handle_platform_output(&w.window, std::mem::take(&mut out.platform_output));
        let prims = self.ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        let clear = egui::Rgba::from(self.ctx.global_style().visuals.panel_fill).to_array();
        painter.paint_and_update_textures(w.viewport, out.pixels_per_point, clear, &prims, &mut out.textures_delta, vec![], &w.window);
        if let Some(vo) = out.viewport_output.get(&w.viewport) {
            self.schedule(id, vo.repaint_delay);
        }
        #[cfg(debug_assertions)]
        if replaying {
            self.schedule(id, Duration::ZERO);
        }
        self.after_frame(el, id, actions);
    }

    fn schedule(&mut self, id: WindowId, delay: Duration) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        if delay.is_zero() {
            w.window.request_redraw();
        } else if let Some(at) = Instant::now().checked_add(delay) {
            w.repaint_at = Some(w.repaint_at.map_or(at, |t| t.min(at)));
        }
    }

    fn after_frame(&mut self, el: &ActiveEventLoop, id: WindowId, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::NewFromClipboard => self.new_from_clipboard_or_blank(el),
                Action::OpenDialog => self.open_dialog(el),
                Action::Capture(m) => self.start_capture(m),
                Action::Close => {
                    self.close_doc(id);
                }
                Action::Quit => self.quit(el),
                Action::Prefs(p) => {
                    p.save();
                    self.prefs = p;
                }
            }
        }
        let Some(w) = self.wins.get_mut(&id) else { return };
        match &mut w.kind {
            Kind::Overlay(o) if o.outcome.is_some() => {
                let Some(Kind::Overlay(o)) = self.wins.remove(&id).map(|w| w.kind) else { return };
                self.remove_window(id);
                match o.outcome {
                    Some(Outcome::Picked(r)) => self.finish_capture(el, o.mode, &o.desktop, r),
                    _ => {
                        self.capturing = false;
                        self.restore_hidden();
                    }
                }
            }
            Kind::Toast(t) if t.done => {
                let Some(Kind::Toast(t)) = self.wins.remove(&id).map(|w| w.kind) else { return };
                self.remove_window(id);
                if t.open {
                    let title = match &t.path {
                        Some(p) => p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                        None => self.next_title(),
                    };
                    self.present(el, t.image, t.scale, title, t.path);
                }
            }
            _ => {}
        }
    }

    // MARK: Tray and hotkeys

    fn setup_tray(&mut self) {
        let new = MenuItem::new("New from Clipboard", true, None);
        let region = MenuItem::new("Capture Region\u{2026}", true, None);
        let window = MenuItem::new("Capture Window\u{2026}", true, None);
        let screen = MenuItem::new("Capture Full Screen", true, None);
        let open = MenuItem::new("Open\u{2026}", true, None);
        let folder = MenuItem::new("Open Screenshots Folder", true, None);
        let shortcuts = Submenu::with_items(
            "Shortcuts",
            true,
            &[
                &MenuItem::new("Ctrl+Shift+1    Region \u{2192} clipboard", false, None),
                &MenuItem::new("Ctrl+Shift+2    Full screen \u{2192} clipboard", false, None),
                &MenuItem::new("Ctrl+Shift+3    Full screen \u{2192} JPEG", false, None),
                &self.print_screen_item,
                &MenuItem::new("Ctrl+Shift+4    Region or window \u{2192} JPEG", false, None),
            ],
        )
        .ok();
        self.update_print_screen_item();
        let autostart = CheckMenuItem::new("Start with Windows", true, system::starts_with_windows(), None);
        let quit = MenuItem::new("Quit Zquig", true, None);
        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let _ = menu.append_items(&[&new, &sep, &region, &window, &screen, &sep2, &open, &folder]);
        if let Some(s) = &shortcuts {
            let _ = menu.append(s);
        }
        let _ = menu.append_items(&[&autostart, &sep3, &quit]);

        let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Zquig").with_menu_on_left_click(false);
        if let Some((rgba, w, h)) = app_icon()
            && let Ok(icon) = tray_icon::Icon::from_rgba(rgba, w, h)
        {
            builder = builder.with_icon(icon);
        }
        self.tray = builder.build().ok();
        self.tray_ids = Some(TrayIds {
            new: new.id().clone(),
            region: region.id().clone(),
            window: window.id().clone(),
            screen: screen.id().clone(),
            open: open.id().clone(),
            folder: folder.id().clone(),
            autostart: autostart.id().clone(),
            quit: quit.id().clone(),
        });
        self.autostart_item = Some(autostart);

        let p = self.proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = p.lock().unwrap().send_event(UserEvent::TrayClick);
            }
        }));
        let p = self.proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = p.lock().unwrap().send_event(UserEvent::Menu(e.id));
        }));
    }

    fn setup_hotkeys(&mut self) {
        let Ok(mgr) = GlobalHotKeyManager::new() else { return };
        let ctrl_shift = Some(Modifiers::CONTROL | Modifiers::SHIFT);
        let mut failed = Vec::new();
        for (mods, code, name, mode) in [
            (ctrl_shift, Code::Digit1, "Ctrl+Shift+1", Mode::RegionToClipboard),
            (ctrl_shift, Code::Digit2, "Ctrl+Shift+2", Mode::ScreenToClipboard),
            (ctrl_shift, Code::Digit3, "Ctrl+Shift+3", Mode::ScreenToFile),
            (ctrl_shift, Code::Digit4, "Ctrl+Shift+4", Mode::RegionToFile),
            // Print Screen is an alternative to Ctrl+Shift+3.
            (None, Code::PrintScreen, "Print Screen", Mode::ScreenToFile),
        ] {
            let hk = HotKey::new(mods, code);
            match mgr.register(hk) {
                Ok(()) => {
                    self.hotkey_modes.insert(hk.id(), mode);
                }
                // The tray's Shortcuts menu explains this case; no need for a message box.
                Err(_) if code == Code::PrintScreen && system::snipping_owns_print_screen() => {}
                Err(_) => failed.push(name),
            }
        }
        let p = self.proxy.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state == HotKeyState::Pressed {
                let _ = p.lock().unwrap().send_event(UserEvent::Hotkey(e.id));
            }
        }));
        self.hotkeys = Some(mgr);
        let p = self.proxy.clone();
        system::watch_keyboard_settings(move || {
            let _ = p.lock().unwrap().send_event(UserEvent::KeyboardSettingsChanged);
        });
        if !failed.is_empty() {
            document::error_box(
                None,
                "Some screenshot shortcuts are unavailable",
                &format!("{} {} already used by another app.", failed.join(", "), if failed.len() == 1 { "is" } else { "are" }),
            );
        }
    }

    /// When the "Print screen opens Snipping Tool" setting changes, Windows takes or releases the key
    /// a moment later, and a registration made while it was taken never receives the key. So take it
    /// again a few times over the next several seconds.
    fn on_keyboard_settings_changed(&mut self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            for ms in [300, 1000, 2500, 5000] {
                std::thread::sleep(Duration::from_millis(ms));
                let _ = proxy.lock().unwrap().send_event(UserEvent::RetakePrintScreen);
            }
        });
        self.update_print_screen_item();
    }

    fn retake_print_screen(&mut self) {
        if let Some(mgr) = &self.hotkeys {
            let hk = HotKey::new(None, Code::PrintScreen);
            let _ = mgr.unregister(hk);
            if mgr.register(hk).is_ok() {
                self.hotkey_modes.insert(hk.id(), Mode::ScreenToFile);
            }
        }
        self.update_print_screen_item();
    }

    fn update_print_screen_item(&self) {
        self.print_screen_item.set_text(if system::snipping_owns_print_screen() {
            "Print Screen    (taken by Snipping Tool \u{2014} see Settings > Accessibility > Keyboard)"
        } else {
            "Print Screen    Full screen \u{2192} JPEG"
        });
    }

    fn setup_fonts(&self) {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        if let Ok(bytes) = std::fs::read(dir.join("segoeui.ttf")) {
            fonts.font_data.insert("segoe".into(), Arc::new(egui::FontData::from_owned(bytes)));
            if let Some(f) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                f.insert(0, "segoe".into());
            }
        }
        let fallback = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
        let mut annotation = fallback.clone();
        if let Ok(bytes) = std::fs::read(dir.join("segoeuib.ttf")) {
            fonts.font_data.insert("segoe-bold".into(), Arc::new(egui::FontData::from_owned(bytes)));
            annotation.insert(0, "segoe-bold".into());
        }
        fonts.families.insert(egui::FontFamily::Name(document::ANNOTATION_FONT.into()), annotation);
        self.ctx.set_fonts(fonts);
        self.ctx.options_mut(|o| o.zoom_with_keyboard = false);
    }

    fn handle_request(&mut self, el: &ActiveEventLoop, req: Request) {
        match req {
            Request::Show => self.show_all(el),
            Request::Open(paths) => {
                for p in paths {
                    self.open_path(el, &p);
                }
            }
            Request::Ping => {}
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.tray.is_some() {
            return;
        }
        self.setup_fonts();
        self.setup_tray();
        self.setup_hotkeys();
        if let Some(req) = self.startup.take() {
            self.handle_request(el, req);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Repaint(vp, delay) => {
                if let Some(id) = self.wins.iter().find(|(_, w)| w.viewport == vp).map(|(id, _)| *id) {
                    self.schedule(id, delay);
                }
            }
            UserEvent::Hotkey(id) => {
                if let Some(&mode) = self.hotkey_modes.get(&id) {
                    self.start_capture(mode);
                }
            }
            UserEvent::Capture(mode) => self.do_capture(el, mode),
            UserEvent::KeyboardSettingsChanged => self.on_keyboard_settings_changed(),
            UserEvent::RetakePrintScreen => self.retake_print_screen(),
            UserEvent::TrayClick => self.show_all(el),
            UserEvent::Menu(id) => {
                let Some(ids) = &self.tray_ids else { return };
                if id == ids.new {
                    self.new_from_clipboard_or_blank(el);
                } else if id == ids.region {
                    self.start_capture(Mode::RegionToEditor);
                } else if id == ids.window {
                    self.start_capture(Mode::WindowToEditor);
                } else if id == ids.screen {
                    self.start_capture(Mode::ScreenToEditor);
                } else if id == ids.open {
                    self.open_dialog(el);
                } else if id == ids.folder {
                    let _ = std::process::Command::new("explorer").arg(capture::screenshots_dir()).spawn();
                } else if id == ids.autostart {
                    let on = !system::starts_with_windows();
                    system::set_starts_with_windows(on);
                    if let Some(item) = &self.autostart_item {
                        item.set_checked(system::starts_with_windows());
                    }
                } else if id == ids.quit {
                    self.quit(el);
                }
            }
            UserEvent::Remote(req) => self.handle_request(el, req),
            UserEvent::PdfOpened(path, result) => match result {
                Ok((img, scale)) => {
                    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    self.present(el, img, scale, title, None);
                }
                Err(e) => document::error_box(None, &format!("Cannot open {}", path.display()), &e),
            },
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        let response = w.state.on_window_event(&w.window, &event);
        match &event {
            WindowEvent::CloseRequested => {
                match &w.kind {
                    Kind::Doc(_) => {
                        self.close_doc(id);
                    }
                    Kind::Overlay(_) => {
                        if let Some(Win { kind: Kind::Overlay(o), .. }) = self.wins.get_mut(&id) {
                            o.outcome = Some(Outcome::Cancelled);
                        }
                        self.after_frame(el, id, Vec::new());
                    }
                    Kind::Toast(_) => self.remove_window(id),
                }
                return;
            }
            WindowEvent::Resized(size) => {
                if let (Some(p), Some(wd), Some(ht)) = (&mut self.painter, NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                    p.on_window_resized(w.viewport, wd, ht);
                }
                w.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { inner_size_writer, .. } => {
                // The capture overlay must stay exactly on the desktop even across monitors with different scaling.
                if let Kind::Overlay(o) = &w.kind {
                    let mut writer = inner_size_writer.clone();
                    let _ = writer.request_inner_size(PhysicalSize::new(o.desktop.image.width(), o.desktop.image.height()));
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw(el, id);
                return;
            }
            WindowEvent::DroppedFile(path) => {
                // Dropping a file on a window replaces its image.
                if let Kind::Doc(doc) = &mut w.kind {
                    let loaded = if imageio::is_pdf(path) { imageio::render_pdf(path) } else { imageio::load_image(path).map(|p| (p, 1.0)) };
                    match loaded {
                        Ok((img, scale)) => doc.replace_image(img, scale),
                        Err(e) => document::error_box(Some(&w.window), &format!("Cannot open {}", path.display()), &e),
                    }
                    w.window.request_redraw();
                }
            }
            _ => {}
        }
        if response.repaint
            && let Some(w) = self.wins.get(&id)
        {
            w.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for w in self.wins.values_mut() {
            if let Some(at) = w.repaint_at {
                if at <= now {
                    w.repaint_at = None;
                    w.window.request_redraw();
                } else {
                    next = Some(next.map_or(at, |n| n.min(at)));
                }
            }
        }
        el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let background = args.iter().any(|a| a == "--background");
    let files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(|a| system::absolute(Path::new(a))).collect();
    let request = if !files.is_empty() {
        Request::Open(files)
    } else if background {
        Request::Ping
    } else {
        Request::Show
    };

    // A second launch (e.g. "Open with" from Explorer) hands over to the running copy, which owns the hotkeys.
    if system::forward(&request) {
        return;
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    let p = proxy.clone();
    system::listen(move |req| {
        let _ = p.lock().unwrap().send_event(UserEvent::Remote(req));
    });
    system::register_open_with();
    text::warm_up();

    let ctx = egui::Context::default();
    let p = proxy.clone();
    ctx.set_request_repaint_callback(move |info| {
        let _ = p.lock().unwrap().send_event(UserEvent::Repaint(info.viewport_id, info.delay));
    });

    let mut app = App {
        proxy,
        ctx,
        painter: None,
        wins: HashMap::new(),
        next_id: 1,
        counter: 0,
        prefs: Prefs::load(),
        startup: Some(request),
        tray: None,
        tray_ids: None,
        autostart_item: None,
        hotkeys: None,
        hotkey_modes: HashMap::new(),
        print_screen_item: MenuItem::new("Print Screen    Full screen \u{2192} JPEG", false, None),
        hidden: Vec::new(),
        capturing: false,
        #[cfg(debug_assertions)]
        replay: replay::Replay::from_env(),
    };
    let _ = event_loop.run_app(&mut app);
}
