//! Group and ungroup. The rules are in `draw/group.rs` and `docs/editing-commands.md`.

use serde_json::{Value, json};

use super::params::{optional_whole, required_ids};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw;
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.group",
            "Group Shapes",
            ["Home", "Arrange"],
            Some("Cmd+Shift+G"),
            "ids: array of whole numbers, the shapes to group, two at least, all in the page's drawing or all members of one group: \
             the group is a new shape whose frame is the box that holds them, in the place of the topmost, and an instance among them is given a local copy of its master's geometry and holds its width, height and pin as formulas of the group, as a file's nested instance does; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            group
        ),
        cmd!(
            "shape.ungroup",
            "Ungroup Shapes",
            ["Home", "Arrange"],
            Some("Cmd+Shift+U"),
            "ids: array of whole numbers, the shapes to ungroup: each one that holds members is taken apart, one level, and its members take its place \
             and draw where they did (a shape with no members is left alone); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            ungroup
        ),
    ]
}

/// The page the command works on: the one named, else the first page holding all `ids`.
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    draw::page_holding(doc, named, ids).ok_or_else(|| bad(cmd, "the document has no pages"))
}

/// Makes a group of the shapes → `{page, id, members}`: the new group's `ID`, and how many shapes it holds.
fn group(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.group";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    // The `ID` is above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    let made = s.edit("Group shapes", |doc| draw::group_shapes(doc, page, &ids, above).map_err(|problem| problem.into_engine(ID)))?;
    s.note_id(page, made.id);
    Ok(json!({ "page": page, "id": made.id, "members": made.members }))
}

/// Takes the groups apart → `{page, ungrouped, ids}`: how many groups were taken apart, and the `ID`s of their members. When none was a group
/// the call is no undo step.
fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.ungroup";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Ungroup shapes", |doc| {
        let taken = draw::ungroup_shapes(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "ungrouped": taken.groups, "ids": taken.members }), taken.groups > 0))
    })
}
