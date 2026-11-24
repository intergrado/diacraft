//! The window settings a drawing holds: `window.settings` and `window.settings.set`.
//!
//! The settings are the drawing's: the flags of the drawing window of its windows part, as a VSDX file holds them (`crate::view`). `window.settings.set` changes them and
//! **is no undo step** (the owner's ruling, 2026-11-22, "Unsaved, not undo"): it marks the drawing changed, so a save keeps them and closing asks, and Ctrl+Z goes on undoing
//! the drawing's edits and leaves the settings alone ([`Session::edit_view`]). It changes nothing at all when what it is given is what the drawing shows already. The window writes
//! through it when a box of the View tab is turned, and a program sets them the same way.

use serde_json::{Value, json};

use super::{CommandSpec, has_document};
use crate::Session;
use crate::error::{Result, bad, failed};
use crate::view::{ViewSettings, set as set_flags, view_settings};
use diacraft_doc::WindowFlags;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "window.settings", "Window Settings", ["View", "Show"], None, "none", has_document, settings),
        cmd!(
            "window.settings.set",
            "Show",
            ["View", "Show"],
            None,
            "rulers: boolean, optional, the rulers show (View, Show, Ruler: ShowRulers of the drawing window); \
             grid: boolean, optional, the grid shows (View, Show, Grid: ShowGrid); \
             guides: boolean, optional, the guides show (View, Show, Guides: ShowGuides); \
             connection_points: boolean, optional, the connection points of the shapes show whatever the tool (View, Visual Aids, Connection Points: ShowConnectionPoints)",
            has_document,
            set
        ),
    ]
}

/// The settings as JSON: what the drawing shows, and what its windows part holds (`null` with no part, and a flag `null` where it holds no value).
fn settings_json(shown: ViewSettings, held: Option<WindowFlags>) -> Value {
    json!({
        "rulers": shown.rulers,
        "grid": shown.grid,
        "guides": shown.guides,
        "connection_points": shown.connection_points,
        "held": held.map(|h| json!({ "rulers": h.rulers, "grid": h.grid, "guides": h.guides, "connection_points": h.connection_points })),
    })
}

/// → `{rulers, grid, guides, connection_points, held}`: what the drawing shows, each a boolean, and `held`, the flags its windows part holds (`null` when it holds none,
/// and then the drawing shows what a new drawing shows: the rulers, the grid and the guides on, the connection points off).
fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed("window.settings", "no document open"))?;
    Ok(settings_json(view_settings(doc), doc.window_flags()))
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// Sets the flags given → the settings as `window.settings` has them. The drawing is changed (unsaved) and **no undo step is made**; nothing changes when every value given is what
/// the drawing shows already.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "window.settings.set";
    let wanted = WindowFlags {
        rulers: flag(p, "rulers", ID)?,
        grid: flag(p, "grid", ID)?,
        // The Page Breaks box is `page_breaks.set`'s (makes it a flag of the same window).
        page_breaks: None,
        guides: flag(p, "guides", ID)?,
        connection_points: flag(p, "connection_points", ID)?,
    };
    s.edit_view(ID, |doc| {
        let changed = set_flags(doc, &wanted).map_err(|words| failed(ID, words))?;
        Ok((settings_json(view_settings(doc), doc.window_flags()), changed))
    })
}
