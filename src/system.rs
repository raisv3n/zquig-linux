//! Windows integration: preferences, "Open with" registration, start-up entry and single instance.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, KEY_READ, KEY_WRITE, REG_NOTIFY_CHANGE_LAST_SET,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use crate::annotation::Rgba;

// MARK: Preferences (last-used colour and thickness, like the Mac app's UserDefaults)

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub color: Rgba,
    pub width: f32,
    pub text_outline: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { color: crate::document::PALETTE[0], width: 4.0, text_outline: true }
    }
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Zquig")
}

impl Prefs {
    pub fn load() -> Self {
        std::fs::read_to_string(data_dir().join("prefs.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("prefs.json"), s);
        }
    }
}

// MARK: Registry

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
pub fn snipping_owns_print_screen() -> bool {
    match get_dword(KEYBOARD_KEY, "PrintScreenKeyForSnippingEnabled") {
        Some(v) => v != 0,
        None => windows_build() >= 22000,
    }
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

fn exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
}

pub const EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "ico", "pdf"];

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
    for ext in EXTENSIONS {
        set_value(&format!(r"{app}\SupportedTypes"), Some(&format!(".{ext}")), "");
        set_value(&format!(r"Software\Classes\.{ext}\OpenWithProgids"), Some("Zquig.Image"), "");
    }
    unsafe {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn starts_with_windows() -> bool {
    get_value(RUN_KEY, Some("Zquig")).is_some()
}

pub fn set_starts_with_windows(on: bool) {
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

// MARK: Single instance

/// What a second launch hands to the running copy.
#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    /// Plain launch: show a window.
    Show,
    Open(Vec<PathBuf>),
    /// Background start (--background): just make sure one copy is running.
    Ping,
}

/// A random per-user key kept in %APPDATA% (which other accounts can't read). Named pipes are
/// machine-wide, so a guessable name would let another user on the same PC create the pipe first,
/// then receive the file paths this user opens or stop Zquig from starting.
fn instance_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    let dir = data_dir();
    let path = dir.join("instance-key");
    if let Ok(k) = std::fs::read_to_string(&path)
        && k.trim().len() == 32
        && k.trim().chars().all(|c| c.is_ascii_hexdigit())
    {
        return k.trim().to_owned();
    }
    // RandomState is seeded from the OS random number generator.
    let part = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        h.finish()
    };
    let key = format!("{:016x}{:016x}", part(), part());
    let _ = std::fs::create_dir_all(&dir);
    // If two copies start at once, the first to write wins and the other adopts its key.
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            let _ = f.write_all(key.as_bytes());
            key
        }
        Err(_) => std::fs::read_to_string(&path).map(|k| k.trim().to_owned()).unwrap_or(key),
    }
}

fn socket_name() -> interprocess::local_socket::Name<'static> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    format!("zquig-{}.sock", instance_key()).to_ns_name::<GenericNamespaced>().expect("socket name").into_owned()
}

/// Largest request accepted from another launch (a few hundred file paths).
const MAX_REQUEST: u64 = 64 * 1024;
const MAX_FILES: usize = 100;

/// Sends the request to an already-running Zquig. Returns false if there isn't one.
pub fn forward(req: &Request) -> bool {
    use interprocess::local_socket::{Stream, traits::Stream as _};
    let Ok(mut stream) = Stream::connect(socket_name()) else { return false };
    let line = serde_json::to_string(req).unwrap_or_default();
    stream.write_all(line.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok()
}

/// Listens for later launches; `on_request` runs on a background thread.
pub fn listen(on_request: impl Fn(Request) + Send + 'static) {
    use std::io::Read;

    use interprocess::local_socket::{ListenerOptions, traits::ListenerExt as _};
    let Ok(listener) = ListenerOptions::new().name(socket_name()).create_sync() else { return };
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(conn.take(MAX_REQUEST)).read_line(&mut line).is_ok()
                && let Ok(req) = serde_json::from_str::<Request>(line.trim())
            {
                on_request(match req {
                    // Only existing local files, and not an unbounded number of windows.
                    Request::Open(paths) => Request::Open(
                        paths.into_iter().filter(|p| p.is_absolute() && p.is_file()).take(MAX_FILES).collect(),
                    ),
                    other => other,
                });
            }
        }
    });
}

pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}
