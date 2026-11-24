//! Undo and redo.

use serde_json::{Value, json};

use super::{CommandSpec, can_redo, can_undo};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "none", can_undo, undo),
        cmd!("edit.redo", "Redo", ["Edit"], Some("Cmd+Shift+Z"), "none", can_redo, redo),
    ]
}

/// Takes back the newest step → `{label}`, the step's name.
fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.step_history("edit.undo", true)?;
    Ok(json!({ "label": &*label }))
}

/// Puts again the step undo took back → `{label}`.
fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.step_history("edit.redo", false)?;
    Ok(json!({ "label": &*label }))
}
