//! What a change of the settings changed, named (design call: "each answer names what it changed").
//!
//! A program changes the person's settings as the window does, and what it gets back is a list of [`Change`]s: the key (`proofing.ignore_uppercase`), what it was and what it is now.
//! **The names are read off the keys, not listed**: [`diff`] serialises the value before and the value after and walks both, so a key someone adds to a section later is named with
//! no change here.

use craft_settings::Settings;
use serde::Serialize;
use serde_json::Value;
use toml_edit::Item;

/// The most changes one call names. Settings have a few dozen keys; the cap is for a value that is not what it should be.
pub const MAX_CHANGES: usize = 256;

/// The most characters of a value a change shows, cut with `…` past it.
const MAX_SHOWN_CHARS: usize = 200;

/// One thing a change made different.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    /// The key of `config.toml` (`proofing.ignore_uppercase`, `language.editing`), or the file (`dictionaries/Custom.dic`).
    pub setting: String,
    /// What it was, as TOML writes it (`true`, `"de-DE"`, `["a", "b"]`); `None` when it was not there.
    pub from: Option<String>,
    /// What it is now; `None` when it is not there any more.
    pub to: Option<String>,
}

impl Change {
    /// One line for a person: `proofing.ignore_uppercase: true -> false`, `dictionaries/Custom.dic: added alpha`, `dictionaries/Custom.dic: removed alpha`.
    pub fn describe(&self) -> String {
        match (&self.from, &self.to) {
            (Some(from), Some(to)) => format!("{}: {from} -> {to}", self.setting),
            (None, Some(to)) => format!("{}: added {to}", self.setting),
            (Some(from), None) => format!("{}: removed {from}", self.setting),
            (None, None) => self.setting.clone(),
        }
    }
}

/// The changes as JSON for an answer: an array of `{setting, from, to}`, with `null` for what was not there.
pub fn changes_json(changes: &[Change]) -> Value {
    serde_json::to_value(changes).unwrap_or(Value::Null)
}

/// The keys that differ between `before` and `after`, in the order the sections write them, at most [`MAX_CHANGES`]. A list is one key, whole.
pub fn diff<T: Settings>(before: &T, after: &T) -> Vec<Change> {
    let (Ok(before), Ok(after)) = (toml_edit::ser::to_document(before), toml_edit::ser::to_document(after)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk("", Some(before.as_item()), Some(after.as_item()), &mut out);
    out
}

fn walk(path: &str, before: Option<&Item>, after: Option<&Item>, out: &mut Vec<Change>) {
    if out.len() >= MAX_CHANGES {
        return;
    }
    let (a, b) = (before.and_then(Item::as_table_like), after.and_then(Item::as_table_like));
    // Two tables, or a table and nothing: key by key. A table and a value that is not one is a change of that key.
    let table_or_absent = |item: Option<&Item>| item.is_none_or(Item::is_table_like);
    if table_or_absent(before) && table_or_absent(after) && (a.is_some() || b.is_some()) {
        let mut keys: Vec<&str> = a.iter().flat_map(|t| t.iter().map(|(k, _)| k)).collect();
        for (key, _) in b.iter().flat_map(|t| t.iter()) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            let child = if path.is_empty() { key.to_owned() } else { format!("{path}.{key}") };
            walk(&child, a.and_then(|t| t.get(key)), b.and_then(|t| t.get(key)), out);
        }
        return;
    }
    let (from, to) = (before.and_then(render), after.and_then(render));
    if from != to {
        out.push(Change { setting: path.to_owned(), from, to });
    }
}

/// An item as TOML writes it, cut; `None` for no item.
fn render(item: &Item) -> Option<String> {
    if item.is_none() {
        return None;
    }
    let text = item.to_string();
    let text = text.trim();
    if text.chars().count() <= MAX_SHOWN_CHARS {
        return Some(text.to_owned());
    }
    let mut cut: String = text.chars().take(MAX_SHOWN_CHARS - 1).collect();
    cut.push('…');
    Some(cut)
}
