//! (Lock "stops shapes on the layer from being selected,
//! moved or edited"): **a command that changes the drawing refuses a shape it names by its `ID` that is on a locked layer**, in words that name the layer, and changes nothing.
//! The window cannot select such a shape; this is the same rule for a program that names one.
//!
//! The rule is the registry's, not each command's: it is asked of every command before it runs ([`refuse`], from `Session::check`), so a command added later cannot skip it.
//!
//! * **Which commands.** Every command that is journaled (a query, which only reads, still answers): the shapes it names are the `ids` it is given and its `id`, except for the
//!   commands of [`ID_NAMES_NO_SHAPE`], whose `id` is a page's or a stencil's, the `container` of a `container.*` command (it is changed: its members and its box) and the `lane` or `phase` of a `swimlane.*` command. A connector's ends (`from`, `to`, `target`) are not edits of the shapes they are glued to: the
//!   layers' Glue box governs them (`draw::layers::glue_refusal`).
//! * **Which page.** The page the command names, else the first page that holds every shape named, else the first page: the page the commands themselves work on (`page_for`).
//! * **Which shapes.** Each shape by its own `LayerMember`, as the window's pointer reads it ([`crate::draw::layers::first_locked`]).
//! * **What it costs.** A drawing none of whose pages has a locked layer costs the read of each page's table and no walk of any page.

use serde_json::Value;

use super::CommandSpec;
use crate::Session;
use crate::draw::{self, layers};
use crate::error::{Result, bad};

/// The commands whose `id` names no shape: a page's (`page.*`) or a stencil's (`stencil.*`). Their `ids`, if they had any, would be shapes.
pub(crate) const ID_NAMES_NO_SHAPE: &[&str] = &["page.rename", "page.duplicate", "page.delete", "page.move", "stencil.open", "stencil.close"];

/// The shapes `p` names by `ID`, for the command `spec`: the whole numbers of its `ids`, at most `MAX_IDS` of them, and its `id`. What is not a whole number
/// is left for the command to refuse.
fn named_shapes(spec: &CommandSpec, p: &Value) -> Vec<u32> {
    let whole = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
    let mut ids: Vec<u32> = match p.get("ids") {
        Some(Value::Array(items)) => items.iter().take(super::params::MAX_IDS).filter_map(whole).collect(),
        _ => Vec::new(),
    };
    if !ID_NAMES_NO_SHAPE.contains(&spec.id)
        && let Some(id) = p.get("id").and_then(whole)
    {
        ids.push(id);
    }
    // The container a container command names is changed too: its list of members and its box.
    if spec.id.starts_with("container.")
        && let Some(container) = p.get("container").and_then(whole)
    {
        ids.push(container);
    }
    // The lane or phase a `swimlane.*` command names is changed too: its place in the list and its box, and what it holds.
    if spec.id.starts_with("swimlane.") {
        for key in ["lane", "phase"] {
            if let Some(id) = p.get(key).and_then(whole) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Refuses `spec` run with `p` when it changes the drawing and names a shape that is on a locked layer: a bad-parameters error whose words name the shape
/// and the layer. Anything else passes: a query, a command that names no shape, no document, a page that is not there.
pub(crate) fn refuse(s: &Session, spec: &CommandSpec, p: &Value) -> Result<()> {
    if !spec.journal {
        return Ok(());
    }
    let ids = named_shapes(spec, p);
    if ids.is_empty() {
        return Ok(());
    }
    let Some(doc) = s.document() else { return Ok(()) };
    let named = match p.get("page") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(page) => Some(page),
            // The command refuses a page that is no whole number in its own words.
            None => return Ok(()),
        },
    };
    // No page of the drawing locks a layer: nothing to look for, and no walk.
    let locks = |page: u32| layers::has_locked_layer(doc, page);
    if !named.map_or_else(|| doc.pages.iter().any(|pg| locks(pg.id)), locks) {
        return Ok(());
    }
    let Some(page) = draw::page_holding(doc, named, &ids) else { return Ok(()) };
    match layers::first_locked(doc, page, &ids).map_err(|e| e.into_engine(spec.id))? {
        Some((id, layer)) => Err(bad(spec.id, layers::locked_words(id, &layer))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The description of the `id` parameter of a command's params doc, if it has one.
    fn id_doc(params: &str) -> Option<&str> {
        params.split(';').find_map(|entry| entry.trim().strip_prefix("id:")).map(str::trim)
    }

    /// The list of the commands whose `id` is no shape's is the registry's: every command whose `id` is said to be "the id of" a page or a stencil is in it, every other `id` is
    /// a shape's (a shape, a connector, a line, a guide, a callout), and every name in it is a registered command that takes an `id`.
    #[test]
    fn the_commands_whose_id_names_no_shape_are_those_whose_doc_says_so() {
        let mut no_shape: Vec<&str> = Vec::new();
        for spec in super::super::command_specs() {
            let Some(doc) = id_doc(spec.params) else { continue };
            if doc.contains("the id of") {
                no_shape.push(spec.id);
            } else {
                assert!(
                    ["shape", "connector", "line", "guide", "callout"].iter().any(|w| doc.contains(w)),
                    "{}: an `id` that names neither a page nor a shape: {doc}",
                    spec.id
                );
            }
        }
        no_shape.sort_unstable();
        let mut listed = ID_NAMES_NO_SHAPE.to_vec();
        listed.sort_unstable();
        assert_eq!(no_shape, listed);
    }
}
