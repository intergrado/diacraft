//! Keys a help text names (`key:Cmd+Shift+Z`): read, and shown the way each platform writes them.
//!
//! The text form is `Cmd`, `Ctrl`, `Alt` and `Shift` in any order (the canonical order is that one) and then one key, joined by `+`: a letter, a digit, `F1` to `F35`, one of
//! `Escape Tab Backspace Enter Space Insert Delete Home End PageUp PageDown Up Down Left Right`, or one of ``- = + [ ] \ ; ' ` , . /``. `Cmd` is the main modifier: Command on a
//! Mac, Ctrl elsewhere, so a chord names `Cmd` or `Ctrl`, never both.

/// The longest text of a chord that is read, in bytes.
pub const MAX_CHORD_BYTES: usize = 64;

/// A platform's way of writing a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Symbols for the modifiers, in the Mac's order (Control, Option, Shift, Command), joined with nothing.
    Mac,
    /// Words joined by `+`, `Cmd` as `Ctrl`.
    Windows,
    /// The same as Windows.
    Linux,
}

/// A key with its modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    /// `Cmd`: the main modifier (Command on a Mac, Ctrl elsewhere).
    pub cmd: bool,
    /// `Ctrl`.
    pub ctrl: bool,
    /// `Alt` (Option on a Mac).
    pub alt: bool,
    /// `Shift`.
    pub shift: bool,
    /// The key's canonical name: `Z`, `5`, `F1`, `PageUp`, `;`.
    pub key: String,
}

/// The named keys.
const NAMED: [&str; 15] =
    ["Escape", "Tab", "Backspace", "Enter", "Space", "Insert", "Delete", "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right"];

/// The keys that are one punctuation character.
const PUNCTUATION: &str = "-=+[]\\;'`,./";

impl Chord {
    /// Reads a chord such as `Cmd+Shift+Z`, `F1` or `Ctrl+Alt+Delete`. Anything that is not exactly the form above is `None`.
    pub fn parse(text: &str) -> Option<Chord> {
        if text.is_empty() || text.len() > MAX_CHORD_BYTES || !text.is_ascii() {
            return None;
        }
        // The key is after the last `+`; a `+` that is the key itself is the one at the end (`Cmd++`, or `+` alone).
        let (modifiers, key) = if text == "+" {
            ("", "+")
        } else if let Some(m) = text.strip_suffix("++") {
            if m.is_empty() {
                return None;
            }
            (m, "+")
        } else {
            match text.rsplit_once('+') {
                Some((m, k)) if !m.is_empty() => (m, k),
                Some(_) => return None,
                None => ("", text),
            }
        };
        if !is_key(key) {
            return None;
        }
        let mut chord = Chord { cmd: false, ctrl: false, alt: false, shift: false, key: key.to_owned() };
        if !modifiers.is_empty() {
            for name in modifiers.split('+') {
                let flag = match name {
                    "Cmd" => &mut chord.cmd,
                    "Ctrl" => &mut chord.ctrl,
                    "Alt" => &mut chord.alt,
                    "Shift" => &mut chord.shift,
                    _ => return None,
                };
                if std::mem::replace(flag, true) {
                    return None;
                }
            }
        }
        (!(chord.cmd && chord.ctrl)).then_some(chord)
    }

    /// The canonical text: `Cmd+Ctrl+Alt+Shift+Key`, with the modifiers that are there.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (on, name) in [(self.cmd, "Cmd"), (self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&self.key);
        out
    }

    /// The chord as `platform` writes it: `Ctrl+Shift+Z`, or `⇧⌘Z` on a Mac.
    pub fn display(&self, platform: Platform) -> String {
        if platform == Platform::Mac {
            let mut out = String::new();
            for (on, symbol) in [(self.ctrl, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.cmd, "⌘")] {
                if on {
                    out.push_str(symbol);
                }
            }
            out.push_str(mac_key(&self.key));
            return out;
        }
        let mut out = String::new();
        for (on, name) in [(self.cmd || self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(pc_key(&self.key));
        out
    }
}

/// Whether `key` is the canonical name of a key.
fn is_key(key: &str) -> bool {
    if key.len() == 1 {
        return key.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) || PUNCTUATION.contains(key);
    }
    if let Some(n) = key.strip_prefix('F') {
        return n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0') && n.parse::<u8>().is_ok_and(|n| (1..=35).contains(&n));
    }
    NAMED.contains(&key)
}

/// A key as a Mac writes it.
fn mac_key(key: &str) -> &str {
    match key {
        "Escape" => "⎋",
        "Enter" => "↩",
        "Tab" => "⇥",
        "Backspace" => "⌫",
        "Delete" => "⌦",
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        "PageUp" => "⇞",
        "PageDown" => "⇟",
        "Home" => "↖",
        "End" => "↘",
        other => other,
    }
}

/// A key as Windows and Linux write it.
fn pc_key(key: &str) -> &str {
    match key {
        "Escape" => "Esc",
        "PageUp" => "Page Up",
        "PageDown" => "Page Down",
        other => other,
    }
}
