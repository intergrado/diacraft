//! AutoConnect: `connector.autoconnect` drops a connected copy of a shape, or a chosen master, beside it in one command. The rules are
//! in `draw/autoconnect.rs`; the drop is `shape.drop`'s and the copy is `shape.duplicate`'s.
//!
//! **AutoConnect** adds the two ways an arrow joins a shape that is on the page already: `existing` (a click on an arrow: the nearest
//! neighbour in its direction, a copy when there is none) and `to` (a drag from an arrow onto a shape), and `connector.neighbour`, which says what a click would join.
//!
//! Lengths are paper inches, as for `shape.*`.

use serde_json::{Value, json};

use super::params::{optional_whole, page_for, units_per_inch};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::route::Dir;
use crate::draw::{self, Problem};
use crate::error::{Result, bad, failed, shown};
use crate::stencil;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "connector.autoconnect",
            "AutoConnect",
            [],
            None,
            "from: whole number, the shape whose arrow is taken; direction: text, right, left, up or down, the side of the shape the new one goes; \
             master: text or whole number, optional, the id or the name of a master to drop beside the shape, as shape.drop takes it (a copy of the shape, \
             without its text, is made when this is left out); stencil: text, optional, the stencil of the master, as shape.drop takes it; \
             existing: true or false, optional, a click on the arrow: join the nearest neighbour of the shape in that direction (see connector.neighbour) and make no \
             shape, or make a copy when there is none (not with master or to); \
             to: whole number, optional, a drag from the arrow onto a shape: join that shape wherever it is and make no shape (not with master or existing); \
             to_point: whole number, optional, the connection point of the shape to to glue to (default: the nearest one); \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            autoconnect
        ),
        cmd!(
            query "autoconnect.settings",
            "AutoConnect Settings",
            ["View", "Visual Aids"],
            None,
            "none",
            has_document,
            settings
        ),
        cmd!(
            "autoconnect.set",
            "AutoConnect",
            ["View", "Visual Aids"],
            None,
            "on: boolean, true or false, the AutoConnect box of the drawing (View, Visual Aids, AutoConnect): on shows the arrows on a shape the pointer is over",
            has_document,
            set_setting
        ),
        cmd!(
            query "connector.neighbour",
            "AutoConnect Neighbour",
            [],
            None,
            "from: whole number, the shape whose arrow is taken; direction: text, right, left, up or down, the side of the shape; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            neighbour
        ),
    ]
}

/// A boolean parameter: `None` where it is left out or null.
fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The word of a direction.
fn word_of(dir: Dir) -> &'static str {
    match dir {
        Dir::Right => "right",
        Dir::Left => "left",
        Dir::Up => "up",
        Dir::Down => "down",
    }
}

/// → `{on, held}`: whether AutoConnect is on for the drawing (it is, for one that holds nothing), and what the drawing holds (`null` for nothing).
fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed("autoconnect.settings", "no document open"))?;
    Ok(json!({ "on": crate::view::autoconnect_on(doc), "held": crate::view::autoconnect_held(doc) }))
}

/// Turns the AutoConnect box of the drawing -> the settings as `autoconnect.settings` has them. The drawing is changed (unsaved) and **no undo step is made** (the box is
/// a View setting, which is "unsaved, not undo"; undo and redo leave it as it is turned); nothing changes when the box is as asked.
fn set_setting(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "autoconnect.set";
    let on = flag(p, "on", ID)?.ok_or_else(|| bad(ID, "missing `on`"))?;
    s.edit_view(ID, |doc| {
        let changed = crate::view::set_autoconnect(doc, on).map_err(|words| failed(ID, words))?;
        Ok((json!({ "on": crate::view::autoconnect_on(doc), "held": crate::view::autoconnect_held(doc) }), changed))
    })
}

/// -> `{page, from, direction, neighbour}`: the shape a click on the arrow would join (`null` when it would make a copy). Writes nothing.
fn neighbour(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.neighbour";
    let from = required_u32(p, "from", ID)?;
    let dir = direction_of(p, ID)?;
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, named, &[from], ID)?;
    Ok(json!({ "page": page, "from": from, "direction": word_of(dir), "neighbour": draw::neighbour(doc, page, from, dir) }))
}

/// The direction a command names.
fn direction_of(p: &Value, cmd: &str) -> Result<Dir> {
    match optional_text(p, "direction", cmd)?.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        Some("right") => Ok(Dir::Right),
        Some("left") => Ok(Dir::Left),
        Some("up") => Ok(Dir::Up),
        Some("down") => Ok(Dir::Down),
        Some(other) => Err(bad(cmd, format!("`direction` must be right, left, up or down, not `{}`", shown(other)))),
        None => Err(bad(cmd, "missing `direction`")),
    }
}

/// What the new shape is made from.
enum Making {
    /// A copy of the source.
    Copy,
    /// A master of a built-in stencil.
    Builtin(&'static stencil::MasterSpec),
    /// A master of a stencil opened from a file: the stencil and the master in it.
    File(std::sync::Arc<stencil::OpenedStencil>, stencil::MasterEntry),
}

/// Finds what `master` and `stencil` name, as `shape.drop` does.
fn making_of(s: &Session, p: &Value) -> Result<Making> {
    const ID: &str = "connector.autoconnect";
    if p.get("master").is_none_or(Value::is_null) {
        if optional_text(p, "stencil", ID)?.is_some() {
            return Err(bad(ID, "`stencil` names the stencil of a `master`; leave it out to make a copy"));
        }
        return Ok(Making::Copy);
    }
    let key = super::stencils::master_key(p, ID)?;
    match optional_text(p, "stencil", ID)? {
        Some(id) if stencil::builtin_stencil(id).is_none() => {
            let opened = s
                .stencils()
                .get(id)
                .cloned()
                .ok_or_else(|| bad(ID, format!("no stencil `{}` is open (see stencil.list; stencil.open opens a file)", shown(id))))?;
            let entry = opened
                .find(&key)
                .cloned()
                .ok_or_else(|| bad(ID, format!("no master `{}` in the stencil `{}` (see stencil.list)", shown(&key), opened.id())))?;
            Ok(Making::File(opened, entry))
        }
        Some(id) => stencil::builtin_stencil(id)
            .and_then(|st| stencil::find_master_in(st, &key))
            .map(Making::Builtin)
            .ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencil `{id}` (see stencil.list)", shown(&key)))),
        None => stencil::find_master(&key)
            .map(Making::Builtin)
            .ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencils (see stencil.list)", shown(&key)))),
    }
}

/// A master that is a part of a structure is refused in words (`stencil/quick.rs`): a Swimlane or a Separator is put in a list of lanes or phases by `shape.drop`,
/// and a stencil file's master whose shape is typed `Container` or `List` is a container or a list; none is a shape to join with a connector, and the strip does not offer one.
fn refuse_part(making: &Making) -> Result<()> {
    const ID: &str = "connector.autoconnect";
    let words = |name: &str, what: &str| {
        bad(ID, format!("master `{}` is a part of a structure ({what}), and AutoConnect joins no part: drop it with shape.drop", shown(name)))
    };
    match making {
        Making::Builtin(spec) if spec.is_part() => Err(words(spec.name, "a Swimlane is inserted in a list of lanes, a Separator splits a phase")),
        Making::File(_, entry) if entry.part => Err(words(&entry.name, "its shape is typed `Container` or `List` by its `User.msvStructureType`")),
        _ => Ok(()),
    }
}

/// AutoConnect → `{page, from, direction, created, id, connector, begin, end, name, width, height, x, y}`: whether a shape was made (`false` for a join), the
/// `ID` of the new shape (or of the shape that was joined), the connector's `ID` and where it is glued (`{id, point}` at each end), the shape's name (of the master,
/// `null` for a copy or a join) and size, and where its centre is (inches).
fn autoconnect(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "connector.autoconnect";
    let from = required_u32(p, "from", ID)?;
    let dir = direction_of(p, ID)?;
    let existing = flag(p, "existing", ID)?.unwrap_or(false);
    let to = optional_whole(p, "to", ID)?;
    let to_point = optional_whole(p, "to_point", ID)?;
    if existing && to.is_some() {
        return Err(bad(ID, "`existing` joins the nearest neighbour and `to` names the shape to join: give one of them"));
    }
    if to_point.is_some() && to.is_none() {
        return Err(bad(ID, "`to_point` is a connection point of the shape `to`: give `to` as well"));
    }
    let makes_a_shape = !p.get("master").is_none_or(Value::is_null) || !p.get("stencil").is_none_or(Value::is_null);
    if makes_a_shape && (existing || to.is_some()) {
        return Err(bad(ID, "`master` makes a new shape, and `existing` and `to` join one that is there: give one or the other"));
    }
    let making = making_of(s, p)?;
    refuse_part(&making)?;
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let on_page: Vec<u32> = std::iter::once(from).chain(to).collect();
    let page = page_for(doc, named, &on_page, ID)?;
    let k = units_per_inch(doc, page);
    let above = s.id_floor(page);
    // The shape to join: the one named, or the nearest neighbour of a click; none makes a shape.
    let join: Option<(u32, Option<u32>)> = match (to, existing) {
        (Some(shape), _) => Some((shape, to_point)),
        (None, true) => draw::neighbour(doc, page, from, dir).map(|shape| (shape, None)),
        (None, false) => None,
    };
    // A shape made with no layers of its own joins the active layers: when none of them allows glue, the connector could not be glued to it.
    if join.is_none() {
        let own_list = matches!(making, Making::Copy)
            && doc
                .find_shape(diacraft_doc::Owner::Page(page), from)
                .and_then(|at| doc.shape(&at))
                .is_some_and(|shape| shape.sheet.cell("LayerMember").is_some());
        if let Some(layer) = crate::draw::layers::new_shape_glue_barred(doc, page).filter(|_| !own_list) {
            return Err(bad(
                ID,
                format!(
                    "the new shape would join the active layer {} (\"{}\"), whose Glue box is off, and a connector is not glued to a shape whose layers allow no glue: tick Glue for the layer, or make it inactive",
                    layer.ix,
                    shown(&layer.name)
                ),
            ));
        }
    }
    let mut dropped: Option<draw::DropInfo> = None;
    let made = s.edit("AutoConnect", |doc| {
        if let Some(target) = join {
            let made = draw::autoconnect_join(doc, page, from, dir, target, above).map_err(|e| e.into_engine(ID))?;
            // A page that has a theme gives it to the connector; the shape that was there keeps its own look.
            crate::theme::follow_page_theme(doc, page, made.connector.id).map_err(|e| e.into_engine(ID))?;
            crate::draw::layers::join_active(doc, page, made.connector.id).map_err(|e| e.into_engine(ID))?;
            return Ok(made);
        }
        let make = |doc: &mut diacraft_doc::Document, centre: (f64, f64)| -> std::result::Result<u32, Problem> {
            match &making {
                Making::Copy => {
                    let pasted = draw::duplicate_shapes(doc, page, &[from], (0.0, 0.0), above)?;
                    let id = pasted.ids.first().copied().ok_or_else(|| Problem::failed("the copy has no shape"))?;
                    // A new shape is made to be written in: the copy has the source's look and size and not its words.
                    let (has_text, is_group) = doc
                        .find_shape(diacraft_doc::Owner::Page(page), id)
                        .and_then(|at| doc.shape(&at))
                        .map_or((false, false), |shape| (shape.text.is_some(), !shape.children.is_empty()));
                    if has_text && !is_group {
                        draw::set_text(doc, page, id, "")?;
                    }
                    Ok(id)
                }
                Making::Builtin(spec) => {
                    let info = draw::drop_shape_above(doc, page, spec, centre.0, centre.1, above)?;
                    let id = info.id;
                    dropped = Some(info);
                    Ok(id)
                }
                Making::File(opened, entry) => {
                    let done = stencil::drop_opened(doc, page, opened, entry, centre.0, centre.1, above)?;
                    let id = done.info.id;
                    dropped = Some(done.info);
                    Ok(id)
                }
            }
        };
        let made = draw::autoconnect(doc, page, from, dir, above, make).map_err(|e| e.into_engine(ID))?;
        // A page that has a theme gives it to the new shape and its connector (`theme/apply.rs`).
        for id in [made.shape, made.connector.id] {
            crate::theme::follow_page_theme(doc, page, id).map_err(|e| e.into_engine(ID))?;
            crate::draw::layers::join_active(doc, page, id).map_err(|e| e.into_engine(ID))?;
        }
        // A page with Auto Size on grows to take the new shape in, in this same undo step (`draw/page_fit.rs`, as `shape.drop` and `shape.duplicate`
        // do). Growing on the left or the bottom moves everything on the page, the new shape and its connector with it, so the centre is
        // read again.
        if draw::fit_page(doc, page, &[made.shape]).map_err(|e| e.into_engine(ID))? {
            let centre = draw::shape_corners(doc, page, made.shape).map_or(made.centre, |c| {
                let (xs, ys) = (c.iter().map(|p| p.0), c.iter().map(|p| p.1));
                (
                    (xs.clone().fold(f64::INFINITY, f64::min) + xs.fold(f64::NEG_INFINITY, f64::max)) / 2.0,
                    (ys.clone().fold(f64::INFINITY, f64::min) + ys.fold(f64::NEG_INFINITY, f64::max)) / 2.0,
                )
            });
            return Ok(draw::AutoConnected { centre, ..made });
        }
        Ok(made)
    })?;
    // Every `ID` the command gave, the members of a group master's instance included, is above the next shape's.
    let highest = s.document().and_then(|doc| draw::page_stats(doc, page)).map_or(made.connector.id, |(highest, _)| highest);
    s.note_id(page, highest.max(made.connector.id).max(made.shape));
    let (width, height, name) = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        let corners = draw::shape_corners(doc, page, made.shape);
        let size = corners.map(|c| {
            let xs = c.iter().map(|p| p.0);
            let ys = c.iter().map(|p| p.1);
            (
                xs.clone().fold(f64::NEG_INFINITY, f64::max) - xs.fold(f64::INFINITY, f64::min),
                ys.clone().fold(f64::NEG_INFINITY, f64::max) - ys.fold(f64::INFINITY, f64::min),
            )
        });
        let name = dropped.as_ref().map(|d| d.name.clone());
        (size.map_or(0.0, |s| s.0 / k), size.map_or(0.0, |s| s.1 / k), name)
    };
    let glue = super::connector::glue_json;
    let mut out = json!({
        "page": page,
        "from": from,
        "direction": word_of(dir),
        "created": join.is_none(),
        "id": made.shape,
        "connector": made.connector.id,
        "begin": glue(made.connector.begin),
        "end": glue(made.connector.end),
        "name": name,
        "width": width,
        "height": height,
        "x": made.centre.0 / k,
        "y": made.centre.1 / k,
    });
    // A master dropped beyond the range of eight of the page's drawing scale says the factor it wrote, and one whose ratio could not be used says why (`draw/drop_scale.rs`).
    if let Some(info) = &dropped {
        if let Some(factor) = info.drop_scale {
            out["drop_on_page_scale"] = json!(factor);
        }
        if let Some(note) = &info.note {
            out["notes"] = json!([note]);
        }
    }
    Ok(out)
}
