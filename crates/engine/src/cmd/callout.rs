//! The Insert tab's callouts: the gallery (`callout.styles`), adding a callout (`callout.insert`), tying one to a shape and untying it (`callout.attach`, `callout.detach`, which is what dropping the
//! yellow handle on a shape and on the page does) and asking what a shape is to callouts (`callout.list`). The rules are named in `draw/callouts.rs` and `stencil/callout.rs`, and in `docs/editing-commands.md`.
//!
//! Lengths are paper inches on the page, as the other editing commands have them; the cells hold the page's drawing units, and the command turns them at its edge.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids, units_per_inch};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::{self, callouts};
use crate::error::{Result, bad, failed};
use crate::stencil::{self, CALLOUT_STYLES, CALLOUT_TEXT, CalloutStyle};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "callout.styles",
            "Callout Gallery",
            ["Insert", "Diagram Parts"],
            None,
            "none",
            has_document,
            list_styles
        ),
        cmd!(
            "callout.insert",
            "Callout",
            ["Insert", "Diagram Parts"],
            None,
            "style: text, optional, the id of a tile of callout.styles (default: the first); \
             ids: array of whole numbers, optional, the shapes to add a callout for: one callout for each, tied to it, up and to the right of it (below or to the left when the page has no room), \
             and a shape of the page's top level, not one inside a group (give ids or x and y, not both); \
             x: number, optional, inches from the page's left edge to the middle of a callout tied to nothing (default: the middle of the page, with y); \
             y: number, optional, inches from the page's bottom edge; \
             text: text, optional, what the callout says (default: Text); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes of ids, else the first page that is not a background). \
             The callouts are shapes with no master that take the page's theme, on the active layers and in front, and are one undo step. A callout tied to a shape moves, is copied and is deleted with it",
            has_document,
            insert
        ),
        cmd!(
            "callout.attach",
            "Tie Callout to Shape",
            ["Insert", "Diagram Parts"],
            None,
            "id: whole number, the callout; target: whole number, the shape it points at, of the page's top level: its tie to the shape it pointed at is taken away, and the end of its leader is put at the edge \
             of the target nearest the callout, where it follows the target; \
             page: whole number, optional, the id of the page (default: the first page holding both)",
            has_document,
            attach
        ),
        cmd!(
            "callout.detach",
            "Untie Callout",
            ["Insert", "Diagram Parts"],
            None,
            "id: whole number, the callout; \
             x: number, optional, inches from the page's left edge to where the end of its leader goes (default: where it is now, with y); y: number, optional, inches from the page's bottom edge; \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             The callout is tied to no shape, so it no longer moves, is copied or is deleted with one, and the end of its leader stays where it is put",
            has_document,
            detach
        ),
        cmd!(
            query "callout.list",
            "List Callouts",
            ["Insert", "Diagram Parts"],
            None,
            "id: whole number, a shape; page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            list
        ),
    ]
}

fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

/// The tiles of the gallery → `{styles: [{id, name}], width, height}`, the size a callout is made at in inches.
fn list_styles(_: &mut Session, _: &Value) -> Result<Value> {
    let styles: Vec<Value> = CALLOUT_STYLES.iter().map(|st| json!({ "id": st.id, "name": st.name })).collect();
    Ok(json!({ "styles": styles, "width": stencil::CALLOUT_WIDTH, "height": stencil::CALLOUT_HEIGHT }))
}

fn style_of(p: &Value, cmd: &str) -> Result<&'static CalloutStyle> {
    match optional_text(p, "style", cmd)? {
        None => CALLOUT_STYLES.first().ok_or_else(|| failed(cmd, "the gallery has no tiles")),
        Some(id) => stencil::find_callout_style(id).ok_or_else(|| {
            let known: Vec<&str> = CALLOUT_STYLES.iter().map(|st| st.id).collect();
            bad(cmd, format!("there is no callout style `{}`; the styles are {}", crate::error::shown(id), known.join(", ")))
        }),
    }
}

/// Adds callouts → `{page, id, ids, targets, style}`: the first callout, all of them, and the shape each is tied to (null for a free one).
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "callout.insert";
    let style = style_of(p, ID)?;
    let ids: Vec<u32> = if p.get("ids").is_none_or(Value::is_null) { Vec::new() } else { required_ids(p, "ids", ID)? };
    let (x, y) = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    if !ids.is_empty() && (x.is_some() || y.is_some()) {
        return Err(bad(ID, "give `ids` (a callout for each shape) or `x` and `y` (one callout tied to nothing), not both"));
    }
    if x.is_some() != y.is_some() {
        return Err(bad(ID, "`x` and `y` go together: give both or neither"));
    }
    let text = optional_text(p, "text", ID)?.unwrap_or(CALLOUT_TEXT);
    let page = target_page(s, p, &ids, ID)?;
    let above = s.id_floor(page);
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    let at = x.zip(y).map(|(x, y)| (x * k, y * k));
    let added = s.edit("Add callout", |doc| {
        let before = doc.clone();
        let added = callouts::add_callouts(doc, page, style, &ids, at, text, above).map_err(|problem| problem.into_engine(ID))?;
        let made: Vec<u32> = added.iter().map(|a| a.id).collect();
        // A page that has a theme gives it to the callouts (`theme/apply.rs`), and they join the active layers: all of them in one pass over their places, found
        // in one walk of the page, where a pass for each walked the page twice for each to find it. A callout is one shape with no members of its own.
        let named = draw::Named::of(doc, page, made.iter().copied()).map_err(|problem| problem.into_engine(ID))?;
        let mut refs = Vec::with_capacity(made.len());
        for id in &made {
            refs.push(named.require(*id).map_err(|problem| problem.into_engine(ID))?);
        }
        crate::theme::follow_page_theme_refs(doc, page, &refs).map_err(|problem| problem.into_engine(ID))?;
        crate::draw::layers::join_active_at(doc, page, &refs).map_err(|problem| problem.into_engine(ID))?;
        draw::settle(doc, &before, page, &made, &[]).map_err(|problem| problem.into_engine(ID))?;
        // A page with Auto Size on grows to take the callouts in, in the same undo step.
        draw::fit_page(doc, page, &made).map_err(|problem| problem.into_engine(ID))?;
        Ok(added)
    })?;
    for a in &added {
        s.note_id(page, a.id);
    }
    let all: Vec<u32> = added.iter().map(|a| a.id).collect();
    let targets: Vec<Value> = added.iter().map(|a| json!(a.target)).collect();
    Ok(json!({ "page": page, "id": all.first(), "ids": all, "targets": targets, "style": style.id }))
}

/// Ties a callout to a shape → `{page, id, target, changed}`.
fn attach(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "callout.attach";
    let id = required_u32(p, "id", ID)?;
    let target = required_u32(p, "target", ID)?;
    let page = target_page(s, p, &[id, target], ID)?;
    s.edit_if("Tie callout", |doc| {
        let before = doc.clone();
        let already = callouts::info(doc, page, id).map_err(|problem| problem.into_engine(ID))?.target == Some(target);
        if already {
            // Still checked: a tie that is made already is not refused for a rule that has changed since, but a target that is no shape is.
            draw::require_shape_ref(doc, page, target).map_err(|problem| problem.into_engine(ID))?;
            return Ok((json!({ "page": page, "id": id, "target": target, "changed": false }), false));
        }
        callouts::tie(doc, page, id, target).map_err(|problem| problem.into_engine(ID))?;
        draw::settle(doc, &before, page, &[id, target], &[]).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "target": target, "changed": true }), true))
    })
}

/// Ties a callout to nothing, and puts the end of its leader at a place when one is given → `{page, id, changed}`.
fn detach(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "callout.detach";
    let id = required_u32(p, "id", ID)?;
    let (x, y) = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    if x.is_some() != y.is_some() {
        return Err(bad(ID, "`x` and `y` go together: give both or neither"));
    }
    let page = target_page(s, p, &[id], ID)?;
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    let at = x.zip(y).map(|(x, y)| (x * k, y * k));
    s.edit_if("Untie callout", |doc| {
        let before = doc.clone();
        let changed = callouts::untie(doc, page, id, at).map_err(|problem| problem.into_engine(ID))?;
        if changed {
            draw::settle(doc, &before, page, &[id], &[]).map_err(|problem| problem.into_engine(ID))?;
        }
        // The end of the leader put at the place it is at, on a callout tied to nothing, wrote the cells it holds: the drawing is as it was, and the answer says so.
        let changed = changed && *doc != before;
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}

/// What a shape is to callouts → `{page, id, is_callout, callouts, target, style, tip}`: the callouts tied to it, the shape it points at, the gallery tile it was made from and where the end of its
/// leader is on the page in inches (the last two for a callout DiaCraft made).
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "callout.list";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, &[id], ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let k = units_per_inch(doc, page);
    let info = callouts::info(doc, page, id).map_err(|problem| problem.into_engine(ID))?;
    let tip = info.tip.map(|(x, y)| json!([x / k, y / k]));
    Ok(json!({
        "page": page,
        "id": id,
        "is_callout": info.is_callout,
        "callouts": info.callouts,
        "target": info.target,
        "style": info.style,
        "tip": tip,
    }))
}
