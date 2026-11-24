//! The command registry: every user-visible action is a command with a stable id, JSON params, and an
//! `enabled` check and a `run` function. Menus, the CLI, the control channel and the MCP server all
//! reach the same registry through [`crate::Session::execute`].
//!
//! A module per area declares its commands with the [`cmd!`] macro in a `specs()` function;
//! [`command_specs`] puts them together.

use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::Session;
use crate::error::{EngineError, Result, bad, failed, internal, shown, shown_panic};
use crate::guard;

/// Declares a [`CommandSpec`]: `cmd!("id", "Label", ["Menu", "Submenu"], Some("Cmd+K"), "params doc", enabled, run)`.
/// `cmd!(query …)` is for a command that only reads: it is not journaled.
macro_rules! cmd {
    ($id:literal, $label:literal, [$($menu:literal),*], $shortcut:expr, $params:literal, $enabled:expr, $run:expr) => {
        $crate::cmd::CommandSpec {
            id: $id,
            label: $label,
            menu: &[$($menu),*],
            shortcut: $shortcut,
            params: $params,
            enabled: $enabled,
            run: $run,
            journal: true,
        }
    };
    (query $id:literal, $label:literal, [$($menu:literal),*], $shortcut:expr, $params:literal, $enabled:expr, $run:expr) => {
        $crate::cmd::CommandSpec {
            id: $id,
            label: $label,
            menu: &[$($menu),*],
            shortcut: $shortcut,
            params: $params,
            enabled: $enabled,
            run: $run,
            journal: false,
        }
    };
}

mod app;
mod arrange;
mod autoconnect;
mod batch;
mod callout;
mod change_shape;
mod clipboard;
mod comment;
mod connector;
mod container;
mod draw_tools;
mod edit;
mod effects;
mod file;
mod find;
mod format;
mod group;
mod insert;
mod layer;
pub(crate) mod locked;
mod orgchart;
mod page;
mod page_setup;
mod params;
mod path;
mod placement;
mod print;
mod relayout;
mod select;
mod shape;
mod shapedata;
mod shapeops;
mod snap_params;
mod snapping;
mod stencils;
mod swimlane;
mod template;
mod text_box;
mod theme;
mod window;

pub use batch::MAX_BATCH_STEPS;
pub(crate) use file::save_as_allows;
#[cfg(kani)]
pub(crate) use file::{SavePath, Target, Why, odg_variant_of_extension, save_path, save_peak};

/// A command's implementation. It gets the session and its params, which [`Session::run_spec`] has
/// already checked: a JSON object whose keys are all named by the command's `params` doc.
pub type Run = fn(&mut Session, &Value) -> Result<Value>;

/// Whether a command can run now: `Err` carries the reason it cannot.
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

/// Everything about one command.
pub struct CommandSpec {
    /// Dotted words: `page.add`.
    pub id: &'static str,
    pub label: &'static str,
    /// Where the command sits in the menus, outermost first (`["Edit"]`); empty for a command that is not in a menu.
    pub menu: &'static [&'static str],
    /// The default shortcut (`Cmd+Shift+Z`), which the interface maps to each platform's.
    pub shortcut: Option<&'static str>,
    /// What the command takes, in prose for people and agents: `none`, or entries `name: description`
    /// separated by `;`. A description may use any character except `;`. [`param_keys`] reads the names,
    /// and a command is run only with params that use those.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    /// Whether a successful call is added to the journal: `false` for queries.
    pub journal: bool,
}

/// A command as a frontend lists it: what to show, and whether it can run now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandInfo {
    pub id: String,
    pub label: String,
    pub menu: Vec<String>,
    pub shortcut: Option<String>,
    pub params: String,
    pub enabled: bool,
    /// Why the command cannot run now; absent when it can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    /// This command as `session` sees it now. Every menu refresh and `engine.commands` call asks this of
    /// every command, so an `enabled` that panics (a bug) must not take the caller down: the command is
    /// listed as disabled, with the bug as its reason, and the panic is counted.
    pub fn info(&self, session: &Session) -> CommandInfo {
        let enabled = guard::catch_panic(|| (self.enabled)(session)).unwrap_or_else(|msg| {
            session.note_panic();
            Err(format!("internal error: {} (please report this bug)", shown_panic(&msg)))
        });
        CommandInfo {
            id: self.id.to_owned(),
            label: self.label.to_owned(),
            menu: self.menu.iter().map(|m| (*m).to_owned()).collect(),
            shortcut: self.shortcut.map(str::to_owned),
            params: self.params.to_owned(),
            enabled: enabled.is_ok(),
            disabled_reason: enabled.err(),
        }
    }
}

// ---------- enablement ----------

/// Always enabled.
pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

/// A document is open.
pub fn has_document(s: &Session) -> std::result::Result<(), String> {
    s.document().map(|_| ()).ok_or_else(|| "no document open".to_owned())
}

/// A document is open and has a step to undo.
pub fn can_undo(s: &Session) -> std::result::Result<(), String> {
    has_document(s)?;
    match s.history() {
        Some(h) if h.undo_len() > 0 => Ok(()),
        Some(h) => Err(h.undo_refusal()),
        None => Err("nothing to undo".to_owned()),
    }
}

/// A document is open and has a step to redo.
pub fn can_redo(s: &Session) -> std::result::Result<(), String> {
    has_document(s)?;
    match s.history() {
        Some(h) if h.redo_len() > 0 => Ok(()),
        Some(h) => Err(h.redo_refusal()),
        None => Err("nothing to redo".to_owned()),
    }
}

// ---------- the registry ----------

/// Every command, in the order a menu or palette lists them.
pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut specs = Vec::new();
        specs.extend(app::specs());
        specs.extend(file::specs());
        specs.extend(edit::specs());
        specs.extend(find::specs());
        specs.extend(page::specs());
        specs.extend(page_setup::specs());
        specs.extend(relayout::specs());
        specs.extend(orgchart::specs());
        specs.extend(print::specs());
        specs.extend(shape::specs());
        specs.extend(select::specs());
        specs.extend(draw_tools::specs());
        specs.extend(stencils::specs());
        specs.extend(connector::specs());
        specs.extend(autoconnect::specs());
        specs.extend(path::specs());
        specs.extend(arrange::specs());
        specs.extend(placement::specs());
        specs.extend(format::specs());
        specs.extend(effects::specs());
        specs.extend(clipboard::specs());
        specs.extend(group::specs());
        specs.extend(change_shape::specs());
        specs.extend(layer::specs());
        specs.extend(comment::specs());
        specs.extend(container::specs());
        specs.extend(swimlane::specs());
        specs.extend(insert::specs());
        specs.extend(shapedata::specs());
        specs.extend(callout::specs());
        specs.extend(shapeops::specs());
        specs.extend(text_box::specs());
        specs.extend(theme::specs());
        specs.extend(template::specs());
        specs.extend(snapping::specs());
        specs.extend(window::specs());
        specs.extend(batch::specs());
        specs
    })
}

/// The command with this id.
pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

// ---------- parameters ----------

/// The parameter names a `params` doc lists (see [`CommandSpec::params`]), in order. `none` lists
/// none. An entry that is not `name: description`, a name that is not lowercase letters, digits and
/// `_` starting with a letter, an empty description or a repeated name is an error.
pub fn param_keys(doc: &str) -> std::result::Result<Vec<&str>, String> {
    if doc == "none" {
        return Ok(Vec::new());
    }
    let mut keys: Vec<&str> = Vec::new();
    for entry in doc.split(';') {
        let entry = entry.trim();
        let Some((key, description)) = entry.split_once(':') else {
            return Err(format!("entry {entry:?} is not `name: description`"));
        };
        let key = key.trim();
        let mut chars = key.chars();
        let is_name = chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !is_name {
            return Err(format!("{key:?} is not a parameter name (lowercase letters, digits and `_`, starting with a letter)"));
        }
        if description.trim().is_empty() {
            return Err(format!("`{key}` has no description"));
        }
        if keys.contains(&key) {
            return Err(format!("`{key}` is listed twice"));
        }
        keys.push(key);
    }
    Ok(keys)
}

/// The params a command is run with: the params as given, or `{}` for `null`. Anything but an object is
/// refused, and so is a key the command's `params` doc does not list, with the nearest listed key.
pub(crate) fn checked_params<'a>(spec: &CommandSpec, params: &'a Value) -> Result<Cow<'a, Value>> {
    let keys = param_keys(spec.params).map_err(|e| internal(spec.id, format!("its params doc is malformed: {e}")))?;
    match params {
        Value::Null => Ok(Cow::Owned(Value::Object(Map::new()))),
        Value::Object(given) => match given.keys().find(|k| !keys.contains(&k.as_str())) {
            Some(unknown) => Err(bad(spec.id, unknown_key_message(unknown, &keys))),
            None => Ok(Cow::Borrowed(params)),
        },
        _ => Err(bad(spec.id, "parameters must be a JSON object")),
    }
}

/// Why `key` is not accepted among `keys`: with the nearest of them, when one is near.
fn unknown_key_message(key: &str, keys: &[&str]) -> String {
    let key = shown(key);
    match nearest(&key, keys) {
        Some(near) => format!("unknown parameter `{key}`; did you mean `{near}`?"),
        None if keys.is_empty() => format!("unknown parameter `{key}`; the command takes no parameters"),
        None => format!("unknown parameter `{key}`; the command takes: {}", keys.join(", ")),
    }
}

/// The edits (insert, delete, replace, swap of neighbours) that turn `a` into `b`, ignoring case.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().map(|c| c.to_ascii_lowercase()).collect(), b.chars().map(|c| c.to_ascii_lowercase()).collect());
    let (mut before, mut prev): (Vec<usize>, Vec<usize>) = (Vec::new(), (0..=b.len()).collect());
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let at = |r: &[usize], k: usize| r.get(k).copied().unwrap_or(usize::MAX / 2);
            let mut d = (at(&prev, j) + usize::from(ca != cb)).min(at(&prev, j + 1) + 1).min(at(&row, j) + 1);
            if i > 0 && j > 0 && a.get(i - 1) == Some(cb) && b.get(j - 1) == Some(ca) {
                d = d.min(at(&before, j - 1) + 1);
            }
            row.push(d);
        }
        before = std::mem::replace(&mut prev, row);
    }
    prev.last().copied().unwrap_or(0)
}

/// The key of `keys` a misspelt `key` most likely meant: the nearest one, when it is near.
pub(crate) fn nearest<'a>(key: &str, keys: &[&'a str]) -> Option<&'a str> {
    let len = key.chars().count();
    let most = (len / 3).max(1);
    // A key whose length differs by more than `most` is more than `most` edits away and isn't measured.
    let near = keys.iter().filter(|k| k.chars().count().abs_diff(len) <= most).map(|k| (distance(key, k), *k)).min()?;
    (near.0 <= most).then_some(near.1)
}

/// The text `p` gives for `key`; `None` when it leaves the key out or gives `null`.
pub(crate) fn optional_text<'a>(p: &'a Value, key: &str, cmd: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

/// The text `p` gives for `key`, which it must give.
pub(crate) fn required_text<'a>(p: &'a Value, key: &str, cmd: &str) -> Result<&'a str> {
    optional_text(p, key, cmd)?.ok_or_else(|| bad(cmd, format!("missing `{key}`")))
}

/// The whole number `p` gives for `key`, which it must give, from 0 to `u32::MAX`.
pub(crate) fn required_u32(p: &Value, key: &str, cmd: &str) -> Result<u32> {
    match p.get(key) {
        None | Some(Value::Null) => Err(bad(cmd, format!("missing `{key}`"))),
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from 0 to {}", u32::MAX))),
    }
}

/// The error a command reports for a failure of the document model.
pub(crate) fn from_doc(cmd: &str, error: diacraft_doc::DocError) -> EngineError {
    use diacraft_doc::DocError;
    match error {
        // The caller asked for something that cannot be.
        DocError::EmptyName | DocError::NameTooLong | DocError::ControlCharacter | DocError::NoSuchPage(_) | DocError::BadName(_) => {
            bad(cmd, error.to_string())
        }
        // The request was fine; the document cannot take it.
        DocError::TooManyPages
        | DocError::IdsExhausted
        | DocError::DuplicatePageId(_)
        | DocError::NoSuchSheet
        | DocError::Limit { .. }
        | DocError::Deleted(_)
        | DocError::EvaluationEdit => failed(cmd, error.to_string()),
    }
}
