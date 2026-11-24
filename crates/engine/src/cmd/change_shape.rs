//! Change Shape: `shape.change` swaps shapes for another master and keeps what they are. The rules are in `draw/change.rs` and
//! `docs/editing-commands.md`.

use serde_json::{Value, json};

use super::params::{optional_whole, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::{self, Glued, Replacement};
use crate::error::{Result, bad, failed, shown};
use crate::stencil;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "shape.change",
        "Change Shape",
        ["Home", "Editing"],
        None,
        "ids: array of whole numbers, the shapes to change, at most 512 (each keeps its ID, its place in the drawing order, its position and size, its text, its own formatting, \
         its shape data and the connectors glued to it, and takes the master's outline and connection points: a one-dimensional shape is changed only to a one-dimensional master \
         and a two-dimensional shape only to a two-dimensional one, and a shape that cannot be changed is listed with its reason in `skipped` while the others are changed, a selection \
         with nothing to change being refused); \
         master: text, the id or the name of a master of a built-in stencil, or of the stencil named by `stencil` (for a stencil opened from a file the \
         master's id, a whole number or its text, or its name: see stencil.list); \
         stencil: text, optional, the id of a stencil in stencil.list (the built-in stencils are searched when it is left out); \
         page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
        can_change,
        change
    )]
}

/// Swapping is on in this document: a document is open and its `DocLockReplace` is off.
fn can_change(s: &Session) -> std::result::Result<(), String> {
    has_document(s)?;
    match s.document().and_then(draw::swapping_locked) {
        Some(why) => Err(why.to_owned()),
        None => Ok(()),
    }
}

fn glue_json(g: &draw::Reglued) -> Value {
    let (how, point) = match g.now {
        Glued::Point(index) => ("point", Some(index)),
        Glued::Free => ("free", None),
        Glued::Left => ("left", None),
    };
    let mut out = json!({ "connector": g.connector, "end": if g.begin { "begin" } else { "end" }, "shape": g.shape, "glued": how });
    if let (Some(map), Some(index)) = (out.as_object_mut(), point) {
        map.insert("point".to_owned(), json!(index));
    }
    out
}

/// Changes the shapes → `{page, master, name, changed, unchanged, skipped: [{id, why}], glue: [{connector, end, shape, glued, point?}]}`; for a master of a stencil
/// opened from a file the answer adds `stencil`, `master_added`, `copied: {styles, colours, faces}` and `notes`, as `shape.drop` gives them.
fn change(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.change";
    let ids = required_ids(p, "ids", ID)?;
    let key = super::stencils::master_key(p, ID)?;
    let from = optional_text(p, "stencil", ID)?;
    let named = optional_whole(p, "page", ID)?;
    let page = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        draw::page_holding(doc, named, &ids).ok_or_else(|| bad(ID, "the document has no pages"))?
    };
    // The master, as `shape.drop` looks for it.
    let opened;
    let entry;
    let replacement = match from {
        Some(id) if stencil::builtin_stencil(id).is_none() => {
            opened = s
                .stencils()
                .get(id)
                .cloned()
                .ok_or_else(|| bad(ID, format!("no stencil `{}` is open (see stencil.list; stencil.open opens a file)", shown(id))))?;
            entry = opened
                .find(&key)
                .cloned()
                .ok_or_else(|| bad(ID, format!("no master `{}` in the stencil `{}` (see stencil.list)", shown(&key), opened.id())))?;
            Replacement::Opened { stencil: &opened, entry: &entry }
        }
        Some(id) => Replacement::Builtin(
            stencil::builtin_stencil(id)
                .and_then(|st| stencil::find_master_in(st, &key))
                .ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencil `{id}` (see stencil.list)", shown(&key))))?,
        ),
        None => Replacement::Builtin(
            stencil::find_master(&key).ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencils (see stencil.list)", shown(&key))))?,
        ),
    };
    // The `ID` of a member of a group master is above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    let made = s.edit_if("Change shape", |doc| {
        let made = draw::change_shapes(doc, page, &ids, &replacement, above).map_err(|problem| problem.into_engine(ID))?;
        let changed = !made.changed.is_empty();
        Ok((made, changed))
    })?;
    if made.highest_id > 0 {
        s.note_id(page, made.highest_id);
    }
    let skipped: Vec<Value> = made.skipped.iter().map(|k| json!({ "id": k.id, "why": k.why })).collect();
    let glue: Vec<Value> = made.reglued.iter().map(glue_json).collect();
    let mut out = json!({
        "page": page,
        "master": made.master,
        "name": made.name,
        "changed": made.changed,
        "unchanged": made.unchanged,
        "skipped": skipped,
        "glue": glue,
    });
    if let (Some(map), Some(b)) = (out.as_object_mut(), made.brought.as_ref())
        && let Replacement::Opened { stencil, .. } = &replacement
    {
        map.insert("stencil".to_owned(), json!(stencil.id()));
        map.insert("master_added".to_owned(), json!(b.added));
        map.insert("copied".to_owned(), json!({ "styles": b.styles_added, "colours": b.colours_added, "faces": b.faces_added }));
        map.insert("notes".to_owned(), json!(b.notes));
    }
    Ok(out)
}
