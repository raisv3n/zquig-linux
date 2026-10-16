//! Preferences and single instance. The platform parts (start at login, "Open with") live in `os`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::annotation::Rgba;
use crate::os::data_dir;
pub use crate::os::{
    AUTOSTART_LABEL, print_screen_taken_by, register_open_with, set_starts_at_login, starts_at_login, watch_keyboard_settings,
};

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

pub const EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "ico", "pdf"];

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

/// A random per-user key kept in the data folder, readable only by this user. Named pipes (Windows)
/// and abstract sockets (Linux) are machine-wide, so a guessable name would let another user on the
/// same PC create the socket first, then receive the file paths this user opens or stop Zquig from starting.
fn instance_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    let dir = data_dir();
    let path = dir.join("instance-key");
    if let Ok(k) = std::fs::read_to_string(&path)
        && k.trim().len() == 32
        && k.trim().chars().all(|c| c.is_ascii_hexdigit())
    {
        // A key file restored from a backup or copied around may have lost its permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
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
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(&path) {
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
