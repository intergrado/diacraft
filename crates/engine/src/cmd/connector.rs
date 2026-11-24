//! Connectors: adding one glued to shapes, and moving one end.
//!
//! Lengths are paper inches, as for `shape.*`.

use serde_json::{Value, json};

use super::params::{end_of, optional_whole, page_for, units_per_inch};
use super::snap_params::{self, Ask};
use super::{CommandSpec, has_document, required_text, required_u32};
use crate::Session;
use crate::draw::{self, ConnectorInfo, End, Which};
use crate::error::{Result, bad, failed};
use crate::snap::{Glue, SnapPage, Subject, Use};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "connector.add",
            "Add Connector",
            ["Home", "Tools"],
            None,
            "from: object, the begin: {id, point} for a shape (point: the connection point's number from 0, optional, else the nearest) or {x, y} for a place in inches; \
             to: object, the end, the same way; page: whole number, optional, the id of the page (default: the first page with the shapes named, else the first page); \
             snap: true or {zoom, view}, optional, snap an end given as {x, y} by the document's Snap & Glue settings: onto a connection point it is glued to (snap.settings)",
            has_document,
            add
        ),
        cmd!(
            "connector.reconnect",
            "Reconnect Connector",
            ["Home", "Tools"],
            None,
            "id: whole number, the connector; end: text, begin or end; target: object, where that end goes: {id, point} or {x, y}, as connector.add's from; \
             page: whole number, optional, the id of the page (default: the first page with the connector); \
             snap: true or {zoom, view}, optional, snap a target given as {x, y}, as connector.add's does",
            has_document,
            reconnect
        ),
    ]
}

/// The glue as JSON: `{id, point}` or `null`.
fn glue_json(glue: Option<(u32, u32)>) -> Value {
    glue.map_or(Value::Null, |(id, point)| json!({ "id": id, "point": point }))
}

/// An end given as a place, snapped as the end of a connector (the glue targets and the snap targets of the document's settings): onto a connection point
/// it becomes the glue to that shape's point. Any other end is returned as it was. → the end, and the snap when there was one.
fn snapped_end(
    doc: &diacraft_doc::Document,
    snap_page: Option<&SnapPage>,
    end: End,
    k: f64,
    ask: Option<Ask>,
    moving: &[u32],
) -> (End, Option<crate::snap::Snapped>) {
    let (Some(ask), Some(snap_page), End::Point { x, y }) = (ask, snap_page, end) else { return (end, None) };
    let (px, py) = (x / k, y / k);
    let snapped = snap_params::run_on(doc, snap_page, Subject::Point((px, py)), Use::Connect, moving, &[], ask);
    let end = match snapped.glue {
        Some(Glue::ConnectionPoint { shape, point }) => End::Shape { id: shape, point: Some(point) },
        _ => {
            let (nx, ny) = snapped.apply((px, py));
            End::Point { x: nx * k, y: ny * k }
        }
    };
    (end, Some(snapped))
}

fn snap_json(snapped: &Option<crate::snap::Snapped>) -> Value {
    snapped.as_ref().map_or(Value::Null, snap_params::snapped_json)
}

fn info_json(page: u32, info: &ConnectorInfo) -> Value {
    json!({ "page": page, "id": info.id, "begin": glue_json(info.begin), "end": glue_json(info.end) })
}

/// Adds a connector → `{page, id, begin, end}`, where each of `begin` and `end` is `{id, point}` of what it is glued to, or `null`.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.add";
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    // The ends are read once the page is known, since its scale turns inches into drawing units.
    let probe = |key: &str| end_of(p, key, ID, 1.0);
    let (first, second) = (probe("from")?, probe("to")?);
    let named_ids: Vec<u32> = [first, second].iter().filter_map(|e| if let End::Shape { id, .. } = e { Some(*id) } else { None }).collect();
    let page = page_for(doc, named, &named_ids, ID)?;
    let k = units_per_inch(doc, page);
    let (from, to) = (end_of(p, "from", ID, k)?, end_of(p, "to", ID, k)?);
    let ask = snap_params::parse(p, ID, snap_params::Words::PLACE)?;
    // The page the session kept (the window's reader made its index of the shapes and its targets there), else read here; the index is built once for both ends.
    let snap_page = ask.and_then(|_| snap_params::read_for(s, page));
    let ((from, snapped_from), (to, snapped_to)) =
        (snapped_end(doc, snap_page.as_deref(), from, k, ask, &[]), snapped_end(doc, snap_page.as_deref(), to, k, ask, &[]));
    let above = s.id_floor(page);
    let info = s.edit("Add connector", |doc| {
        let info = draw::add_connector_above(doc, page, &from, &to, above).map_err(|e| e.into_engine(ID))?;
        // A page that has a theme gives it to the connector (`theme/apply.rs`), in the same undo step.
        crate::theme::follow_page_theme(doc, page, info.id).map_err(|e| e.into_engine(ID))?;
        crate::draw::layers::join_active(doc, page, info.id).map_err(|e| e.into_engine(ID))?;
        Ok(info)
    })?;
    s.note_id(page, info.id);
    let mut out = info_json(page, &info);
    if let (Some(map), true) = (out.as_object_mut(), ask.is_some()) {
        map.insert("snap".into(), json!({ "from": snap_json(&snapped_from), "to": snap_json(&snapped_to) }));
    }
    // (A curved connector on a drawing saved as an `.odg` was told so here, while the save refused it: the writer writes it as cubics.)
    Ok(out)
}

/// Moves one end of a connector → `{page, id, begin, end}`, as `connector.add`.
fn reconnect(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.reconnect";
    let id = required_u32(p, "id", ID)?;
    let which = match required_text(p, "end", ID)? {
        "begin" => Which::Begin,
        "end" => Which::End,
        _ => return Err(bad(ID, "`end` must be `begin` or `end`")),
    };
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, named, &[id], ID)?;
    let k = units_per_inch(doc, page);
    let target = end_of(p, "target", ID, k)?;
    let ask = snap_params::parse(p, ID, snap_params::Words::PLACE)?;
    let snap_page = ask.and_then(|_| snap_params::read_for(s, page));
    let (target, snapped) = snapped_end(doc, snap_page.as_deref(), target, k, ask, &[id]);
    let info = s.edit("Reconnect connector", |doc| draw::reconnect_connector(doc, page, id, which, &target).map_err(|e| e.into_engine(ID)))?;
    let mut out = info_json(page, &info);
    if let (Some(map), true) = (out.as_object_mut(), ask.is_some()) {
        map.insert("snap".into(), snap_json(&snapped));
    }
    Ok(out)
}
