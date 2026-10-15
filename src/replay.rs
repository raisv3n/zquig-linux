//! Debug builds only: feeds scripted input to the first editor window, for UI testing without a real mouse.
//!
//! Set `ZQUIG_REPLAY` to a file of lines, one step per frame, in window points:
//!   click X Y | dclick X Y | drag X1 Y1 X2 Y2 | type TEXT | key NAME [ctrl] [shift] [alt] | copy | paste | wait FRAMES

use std::collections::VecDeque;

use egui::{Event, Key, Modifiers, PointerButton, pos2};

#[derive(Default)]
pub struct Replay {
    frames: VecDeque<(Vec<Event>, Modifiers)>,
}

impl Replay {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("ZQUIG_REPLAY")?;
        let script = std::fs::read_to_string(path).ok()?;
        let mut r = Replay::default();
        r.frames.extend((0..20).map(|_| (Vec::new(), Modifiers::NONE))); // let the window settle
        for line in script.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| parts.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            match parts.first().copied() {
                Some("click") => r.click(num(1), num(2)),
                Some("dclick") => {
                    r.click(num(1), num(2));
                    r.click(num(1), num(2));
                }
                Some("drag") => r.drag(num(1), num(2), num(3), num(4)),
                Some("type") => {
                    let text = line.trim_start().strip_prefix("type").unwrap_or("").trim_start().to_string();
                    r.frames.push_back((vec![Event::Text(text)], Modifiers::NONE));
                }
                Some("key") => {
                    let m = Modifiers {
                        ctrl: parts.contains(&"ctrl"),
                        command: parts.contains(&"ctrl"),
                        shift: parts.contains(&"shift"),
                        alt: parts.contains(&"alt"),
                        mac_cmd: false,
                    };
                    if let Some(key) = parts.get(1).and_then(|n| Key::from_name(n)) {
                        let ev = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: m };
                        r.frames.push_back((vec![ev(true)], m));
                        r.frames.push_back((vec![ev(false)], m));
                    }
                }
                Some("copy") => r.frames.push_back((vec![Event::Copy], Modifiers::NONE)),
                Some("paste") => r.frames.push_back((vec![Event::Paste(String::new())], Modifiers::NONE)),
                Some("wait") => r.frames.extend((0..num(1) as usize).map(|_| (Vec::new(), Modifiers::NONE))),
                _ => {}
            }
        }
        Some(r)
    }

    fn button(x: f32, y: f32, pressed: bool) -> Event {
        Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn click(&mut self, x: f32, y: f32) {
        self.frames.push_back((vec![Event::PointerMoved(pos2(x, y))], Modifiers::NONE));
        self.frames.push_back((vec![Self::button(x, y, true)], Modifiers::NONE));
        self.frames.push_back((vec![Self::button(x, y, false)], Modifiers::NONE));
        self.frames.push_back((Vec::new(), Modifiers::NONE));
    }

    fn drag(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.frames.push_back((vec![Event::PointerMoved(pos2(x1, y1))], Modifiers::NONE));
        self.frames.push_back((vec![Self::button(x1, y1, true)], Modifiers::NONE));
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            self.frames.push_back((vec![Event::PointerMoved(pos2(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t))], Modifiers::NONE));
        }
        self.frames.push_back((vec![Self::button(x2, y2, false)], Modifiers::NONE));
        self.frames.push_back((Vec::new(), Modifiers::NONE));
    }

    /// Adds the next frame's events. Returns true while there's more to play.
    pub fn feed(&mut self, input: &mut egui::RawInput) -> bool {
        if let Some((events, mods)) = self.frames.pop_front() {
            input.events.extend(events);
            let _ = mods; // egui reads modifiers from the key events themselves
            input.focused = true;
        }
        !self.frames.is_empty()
    }
}
