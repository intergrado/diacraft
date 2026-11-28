//! Shapes: the stencils, and dropping, moving, resizing, deleting and labelling shapes.
//!
//! Every length these commands take or give is in **paper inches** on the page (the units of `diacraft_render::View`), whatever the page's
//! scale: the command converts to the page's drawing units at its edge. On the pages new documents make the two are the same.

use serde_json::{Value, json};

use super::params::{optional_bool, optional_range, optional_whole, page_for, required_ids, required_number, units_per_inch};
use super::snap_params::{self, Ask};
use super::{CommandSpec, always, has_document, optional_text, required_text, required_u32};
use crate::Session;
use crate::draw::{self, Problem};
use crate::error::{Result, bad, failed};
use crate::snap::{Axis, OwnFrame, Rect, Subject, Use};
use crate::stencil::{self, PartKind};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "stencil.list",
            "List Stencils",
            [],
            None,
            "none",
            always,
            list
        ),
        cmd!(
            "shape.drop",
            "Drop Shape",
            ["Home", "Shape"],
            None,
            "master: text, the id or the name of a master of a built-in stencil, or of the stencil named by `stencil` (for a stencil opened from a file the \
             master's id, a whole number or its text, or its name: see stencil.list); \
             stencil: text, optional, the id of a stencil in stencil.list: basic, flowchart, or an opened file's such as file-1 (the built-in stencils \
             are searched when it is left out); page: whole number, the id of the page, whose drawing scale is compared with the master's (its page sheet's PageScale over DrawingScale): \
             within a factor of 8 the shape keeps the master's size in drawing units and nothing is written, and beyond it the shape keeps the master's size on paper, \
             its Width and Height being the master's times the ratio and its DropOnPageScale the ratio (a one-dimensional master has its length so scaled), and the result adds drop_on_page_scale, \
             while a ratio that cannot be used drops the master unscaled and the result's notes say why; \
             x: number, where the shape's centre goes, in inches from the page's left edge; y: number, in inches from the page's bottom edge; \
             snap: true or {zoom, view}, optional, snap the dropped shape by the document's Snap & Glue settings (snap.settings), zoom being the screen points to an inch, default 96, \
             and view [x0, y0, x1, y1] the visible part of the page in inches, which a drop has no use for and the window sends with every snapping command, \
             and a shape pulled to a guide with glue to guides on is glued to it and follows it (the result's snap.held). \
             A position of the orgchart stencil dropped with its centre inside another position becomes that position's report: a connector is made, the chart is laid out again in the same step, and the result adds reports_to and connector. \
             The Pool of the bpmn stencil makes a structure of swimlanes, a frame with its name in a strip on the left and one lane, centred on the point, and answers the frame's id with master null, and its Lane \
             is put into the pool under the point, before or after the lane it is dropped on (the lane's id with master null), and anywhere else is the shape the Lane master draws",
            has_document,
            drop_command
        ),
        cmd!(
            "shape.move",
            "Move Shapes",
            ["Home", "Shape"],
            None,
            "ids: array of whole numbers, the shapes to move; dx: number, inches to the right; dy: number, inches up; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             snap: true or {zoom, view, lock, nudge}, optional, snap the box of the shapes by the document's Snap & Glue settings (at most 512 shapes, see shape.drop, and one shape turned by other than whole quarter turns also lines up its own sides with the like sides of shapes turned alike and with slanted guides parallel to them), and lock: \"x\" or \"y\" keeps \
             the move to that page axis, so the other axis moves not at all and the free axis snaps, and nudge: {tick} makes the move one press of an arrow key, dx and dy giving the direction \
             (one of them is 0 and the other's sign is the way), so the shapes go to the next place they can snap to that way however far, or by tick inches (the ruler's tick at the zoom when it is left out) \
             when nothing is ahead, and view [x0, y0, x1, y1] is the visible part of the page in inches, which a move has no use for",
            has_document,
            move_command
        ),
        cmd!(
            "shape.resize",
            "Resize Shape",
            ["Home", "Shape"],
            None,
            "id: whole number, the shape (give id or ids, not both); \
             ids: array of whole numbers, instead of id: several shapes resized as one. Then x, y, width and height are the box on the page that the joint box of the shapes \
             (the box that holds their boxes, a connector counting by its two ends) is made, x and y its lower-left corner, and every shape is carried by the stretch that takes the old box to the new \
             one: its place by the stretch, its width and height by the lengths the stretch gives its own axes, its turn kept, a connector's free ends by the stretch, and a group is one shape whose members follow it. \
             A shape with LockHeight set keeps its height and is sized along its width and one with LockWidth the reverse, a shape with both locks or with one of them and LockAspect is left alone and listed in the result's locked, so is a connector with either lock, the resize is refused when every shape is left alone, and a shape with LockAspect alone is stretched by the same factor on both axes. With id a lock is read only for a size that is a formula, so a group of compartments keeps the formula of its height and is sized along its width, and any other shape is sized as before; \
             x: number, with id: inches from the page's left edge to the corner of the box that is at (0, 0) of the shape's own frame: the box's lower-left corner for a shape \
             that is not turned or flipped, but its lower-right corner for one flipped left to right (FlipX), its upper-left corner for one flipped top to bottom \
             (FlipY), and a corner that turns with the shape (after a quarter turn counterclockwise it is the lower-right corner on the page); \
             y: number, with id: inches from the page's bottom edge to that same corner; \
             width: number, more than 0, the box's size along the shape's own x axis (with ids, the width of the new joint box); height: number, more than 0, along its own y axis (with ids, its height); \
             page: whole number, optional, the id of the page (default: the first page with the shape); \
             snap: true or {zoom, view, keep}, optional, snap the edges the resize moves by the document's Snap & Glue settings (with ids, the edges of the joint box, which is square to the page, at most 512 shapes), where view [x0, y0, x1, y1] is the visible part of the page in inches and a width or height stops at that of a shape in it (all shapes when it is left out), and keep \"proportions\" keeps the shape's own proportions: from a corner the opposite corner stays, from a side the opposite side stays and the other axis grows about its middle (a shape that is turned or flipped is snapped along its own axes, its sides meeting the like sides of shapes turned alike and its corners and side middles meeting lines of the page, and one whose corners on the page are no rectangle is not snapped), \
             and a shape too thin or too flat for its proportions to fit the page is refused with keep",
            has_document,
            resize
        ),
        cmd!(
            "shape.delete",
            "Delete Shapes",
            ["Home", "Shape"],
            Some("Delete"),
            "ids: array of whole numbers, the shapes to delete; \
             connectors: true or false, optional (default true), true takes along the connectors glued to the shapes (and to the shapes inside them), as Delete does in the window, and says them in connectors_deleted, \
             except a connector on a locked layer or on hidden layers only, which stays, and at most 4096 connectors are taken, more is refused. False leaves each connector with the end that was glued a plain point where it was; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            delete
        ),
        cmd!(
            "shape.set_text",
            "Set Shape Text",
            ["Home", "Shape"],
            None,
            "id: whole number, the shape; text: text, the shape's new text (empty takes the text away); \
             start: whole number, optional, and with end: replace only the characters from start up to end of the shape's text, counted in characters from 0 \
             and not counting its fields, and keep its runs and fields (text is then what goes in their place, and start equal to end inserts it), \
             where the shape's text is the text it shows, its master's when it holds none of its own, which the shape then holds as its own with the change made; \
             end: whole number, optional, see start; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            set_text
        ),
    ]
}

/// → `{stencils: [{id, name, builtin, masters: [{id, name, width, height, connection_points, ..}]}]}`: the built-in stencils (Basic Shapes, then
/// Basic Flowchart), with each master's default size in inches, and then the stencils opened from files. The payload is `stencil::list_with`.
fn list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(stencil::list_with(s.stencils()))
}

/// Runs `f` on the open document as the undo step `label`, with the page's drawing units per paper inch.
fn edit_page<T>(
    s: &mut Session,
    cmd: &str,
    label: &str,
    page: u32,
    f: impl FnOnce(&mut diacraft_doc::Document, f64) -> std::result::Result<T, Problem>,
) -> Result<T> {
    let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
    s.edit(label, |doc| f(doc, k).map_err(|p| p.into_engine(cmd)))
}

/// Copies the master into the document if it is not there, and adds an instance of it centred at (`x`, `y`) → `{id, master, name, page,
/// width, height}`: the new shape's `ID`, the master's `ID` in the document, and the size in inches. A master of a stencil opened from a file
/// (`stencil` names it) is brought in by `stencil::bring_master` (the document's own when it has the same one, else a copy with the styles,
/// colours and faces it needs), and the answer adds `stencil`, `master_added`, `copied: {styles, colours, faces}` and `notes`.
fn drop_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.drop";
    let key = super::stencils::master_key(p, ID)?;
    let from = optional_text(p, "stencil", ID)?;
    let (page, x, y) = (required_u32(p, "page", ID)?, required_number(p, "x", ID)?, required_number(p, "y", ID)?);
    let ask = snap_params::parse(p, ID, snap_params::Words::PLACE)?;
    let spec = match from {
        Some(id) if stencil::builtin_stencil(id).is_none() => return drop_from_file(s, id, &key, (page, x, y), ask),
        Some(id) => stencil::builtin_stencil(id)
            .and_then(|st| stencil::find_master_in(st, &key))
            .ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencil `{id}` (see stencil.list)", crate::error::shown(&key))))?,
        None => stencil::find_master(&key)
            .ok_or_else(|| bad(ID, format!("no master `{}` in the built-in stencils (see stencil.list)", crate::error::shown(&key))))?,
    };
    // The Swimlane and the Separator of the Cross-Functional Flowchart stencil are inserted in the lanes and the phases, not dropped as shapes of their own.
    if stencil::xfunc::is_part(spec.id)
        && let Some(done) = super::swimlane::drop_part(s, spec.id, page, x, y)?
    {
        return Ok(done);
    }
    // BPMN's Pool makes a structure of swimlanes, and its Lane is put into the pool under it (`cmd/swimlane.rs`); a lane dropped anywhere else is the shape its master draws.
    match spec.part_kind() {
        Some(PartKind::Pool) => {
            // The Pool is made by code and not by an instance of its master, so the range of eight of the page's scale (`draw/drop_scale.rs`) is taken here, as the window's
            // preview takes it (`snap::drop_preview`): a built-in master is drawn at 1:1, and beyond the range of eight the pool keeps its size on paper (the master's times the ratio, which is `k`);
            // within it, its drawing size.
            let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
            // The size and the drawing units that one inch of it is in: the least of a lane is held to the size in its own unit.
            let (size, inch) = draw::drop_scale::built_in_fit(spec.width, spec.height, k);
            return super::swimlane::drop_pool(s, page, (x, y), size, inch);
        }
        Some(PartKind::Lane) => {
            if let Some(done) = super::swimlane::drop_lane(s, page, x, y)? {
                return Ok(done);
            }
        }
        _ => {}
    }
    // The `ID` is above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    // The page as the session kept it (the window's reader made its shapes' index and its targets there), read before the edit: the shape the drop makes is never a target.
    let read = ask.and_then(|_| snap_params::read_before_edit(s, page));
    let (info, k, snapped, notes) = edit_page(s, ID, "Drop shape", page, |doc, k| {
        let info = draw::drop_shape_above(doc, page, spec, x * k, y * k, above)?;
        // A page that has a theme gives it to the new shape (`theme/apply.rs`), in the same undo step.
        crate::theme::follow_page_theme(doc, page, info.id)?;
        crate::draw::layers::join_active(doc, page, info.id)?;
        // The shape is snapped where it fell, in the same undo step: its box against the page's targets, the shape itself never one.
        let snapped = snap_dropped(doc, read.as_ref(), page, k, (x, y), (info.id, info.width, info.height), ask)?;
        // A page with Auto Size on grows to take the shape in (`draw/page_fit.rs`), in the same undo step.
        draw::fit_page(doc, page, &[info.id])?;
        // Pulled to a guide, it is held to it.
        let snapped = snapped.map(|sn| hold(doc, page, &[info.id], sn)).transpose()?;
        // A shape dropped inside a container joins it; one on a locked layer is passed over, and noted.
        let fitted = draw::container::arrive(doc, page, &[info.id])?;
        let notes = draw::container::locked_notes(doc, page, &fitted.locked)?;
        // A position of an org chart dropped inside another becomes its report, and the chart is laid out again, in this same edit.
        let (linked, left) = draw::relayout::org::on_drop(doc, page, info.id, (x * k, y * k), above.max(info.id));
        let mut notes = notes;
        notes.extend(left);
        Ok((info, k, snapped, (notes, linked)))
    })?;
    let (notes, linked) = notes;
    // Every `ID` the drop gave, the members of a group master's instance included, is above the next shape's.
    let highest = s.document().and_then(|doc| draw::page_stats(doc, page)).map_or(info.id, |(highest, _)| highest.max(info.id));
    s.note_id(page, highest);
    if let Some(done) = &linked {
        s.note_id(page, done.linked.connector);
    }
    let mut out =
        json!({ "id": info.id, "master": info.master, "name": info.name, "page": page, "width": info.width / k, "height": info.height / k });
    // A drop beyond the range of eight of the page's drawing scale says the factor it wrote (`draw/drop_scale.rs`), and one whose ratio could not be used says why.
    let mut notes = notes;
    notes.extend(info.note.clone());
    if let Some(factor) = info.drop_scale {
        out["drop_on_page_scale"] = json!(factor);
    }
    if !notes.is_empty() {
        out["notes"] = json!(notes);
    }
    if let Some(done) = &linked {
        out["reports_to"] = json!(done.manager);
        out["connector"] = json!(done.linked.connector);
    }
    add_snap(&mut out, snapped.as_ref());
    Ok(out)
}

/// The snap of a shape just dropped: its box (`width` by `height` drawing units, centred at (`x`, `y`) paper inches) against the page's targets. When it
/// moves the shape, the move is part of the drop. `None` when the caller did not ask.
fn snap_dropped(
    doc: &mut diacraft_doc::Document,
    read: Option<&snap_params::Settled>,
    page: u32,
    k: f64,
    (x, y): (f64, f64),
    (id, width, height): (u32, f64, f64),
    ask: Option<Ask>,
) -> std::result::Result<Option<crate::snap::Snapped>, Problem> {
    let Some(ask) = ask else { return Ok(None) };
    let (w, h) = (width / k, height / k);
    let rect = Rect { x0: x - w / 2.0, y0: y - h / 2.0, x1: x + w / 2.0, y1: y + h / 2.0 };
    // The dropped shape's own connection points are pulled with its box.
    let own = snap_params::own_points(doc, page, &[id], (0.0, 0.0));
    let snapped = snap_params::run_with(doc, read.map(snap_params::Settled::page), Subject::Box(rect), Use::Place, &[id], &own, ask);
    if snapped.dx != 0.0 || snapped.dy != 0.0 {
        draw::move_shapes(doc, page, &[id], snapped.dx * k, snapped.dy * k)?;
    }
    Ok(Some(snapped))
}

/// A snap and the shapes it held to guides.
pub(super) struct Held {
    pub snapped: crate::snap::Snapped,
    pub holding: crate::snap::hold::Holding,
    /// Whether the command changed the document; a command that changed nothing tells no shape held.
    pub changed: bool,
}

impl Held {
    /// Whether holding wrote anything: a shape held already as the snap would hold it is not written, and is no change.
    pub fn wrote(&self) -> bool {
        !self.holding.written.is_empty()
    }
}

/// Holds the shapes `ids`, placed where the snap put them, to the guides it was pulled to (`snap::hold`), in the caller's edit.
pub(super) fn hold(doc: &mut diacraft_doc::Document, page: u32, ids: &[u32], snapped: crate::snap::Snapped) -> std::result::Result<Held, Problem> {
    let holding = crate::snap::hold::hold_shapes(doc, page, ids, &snapped.holds)?;
    Ok(Held { snapped, holding, changed: true })
}

/// Tells the snap in the result of a command, when there was one, and the shapes it leaves held to the guides it was pulled to (`held`): a hold the
/// command wrote, or one the edit renewed where the shape is (a move along the guide); a command that changed nothing tells none.
pub(super) fn add_snap(out: &mut Value, snapped: Option<&Held>) {
    if let (Some(map), Some(held)) = (out.as_object_mut(), snapped) {
        let mut snap = snap_params::snapped_json(&held.snapped);
        if let Some(inner) = snap.as_object_mut() {
            let told: &[u32] = if held.changed || held.wrote() { &held.holding.held } else { &[] };
            inner.insert("held".into(), json!(told));
        }
        map.insert("snap".into(), snap);
    }
}

/// [`drop_command`] for a master of the stencil opened from a file, `stencil_id`.
fn drop_from_file(s: &mut Session, stencil_id: &str, key: &str, (page, x, y): (u32, f64, f64), ask: Option<Ask>) -> Result<Value> {
    const ID: &str = "shape.drop";
    let opened =
        s.stencils().get(stencil_id).cloned().ok_or_else(|| {
            bad(ID, format!("no stencil `{}` is open (see stencil.list; stencil.open opens a file)", crate::error::shown(stencil_id)))
        })?;
    let entry = opened
        .find(key)
        .cloned()
        .ok_or_else(|| bad(ID, format!("no master `{}` in the stencil `{}` (see stencil.list)", crate::error::shown(key), opened.id())))?;
    let above = s.id_floor(page);
    let read = ask.and_then(|_| snap_params::read_before_edit(s, page));
    let (done, k, snapped, locked) = edit_page(s, ID, "Drop shape", page, |doc, k| {
        let done = stencil::drop_opened(doc, page, &opened, &entry, x * k, y * k, above)?;
        // The instance and its members follow the page's theme where the master gives them no style.
        crate::theme::follow_page_theme(doc, page, done.info.id)?;
        crate::draw::layers::join_active(doc, page, done.info.id)?;
        let snapped = snap_dropped(doc, read.as_ref(), page, k, (x, y), (done.info.id, done.info.width, done.info.height), ask)?;
        draw::fit_page(doc, page, &[done.info.id])?;
        let snapped = snapped.map(|sn| hold(doc, page, &[done.info.id], sn)).transpose()?;
        let fitted = draw::container::arrive(doc, page, &[done.info.id])?;
        let locked = draw::container::locked_notes(doc, page, &fitted.locked)?;
        Ok((done, k, snapped, locked))
    })?;
    s.note_id(page, done.highest_id);
    let b = &done.brought;
    let mut out = json!({
        "id": done.info.id,
        "master": done.info.master,
        "name": done.info.name,
        "page": page,
        "width": done.info.width / k,
        "height": done.info.height / k,
        "stencil": opened.id(),
        "master_added": b.added,
        "copied": { "styles": b.styles_added, "colours": b.colours_added, "faces": b.faces_added },
        "notes": b.notes.iter().chain(&locked).chain(&done.info.note).collect::<Vec<_>>(),
    });
    if let Some(factor) = done.info.drop_scale {
        out["drop_on_page_scale"] = json!(factor);
    }
    add_snap(&mut out, snapped.as_ref());
    Ok(out)
}

/// Moves shapes by (`dx`, `dy`) → `{page, moved}`.
fn move_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.move";
    let ids = required_ids(p, "ids", ID)?;
    let (dx, dy) = (required_number(p, "dx", ID)?, required_number(p, "dy", ID)?);
    let page = target_page(s, p, &ids, ID)?;
    let mut ask = snap_params::parse(p, ID, snap_params::Words::MOVE)?;
    if ask.is_some() && ids.len() > snap_params::MAX_SNAPPED_SHAPES {
        return Err(bad(ID, format!("`snap` takes at most {} shapes", snap_params::MAX_SNAPPED_SHAPES)));
    }
    // The page as the session kept it (the window's reader made its index of the shapes and its targets there), else read here, as before: the press of an arrow key and a
    // release both ask it for the ruler's tick and for the snap, and the page is found or read once.
    let read = ask.and_then(|_| snap_params::read_for(s, page));
    // An arrow key: `dx` and `dy` give the direction (one is 0), and the shape goes to the next place it can snap to that way, however far.
    let nudge = match (ask.and_then(|a| a.nudge), s.document()) {
        (Some(tick), Some(_)) => {
            let (axis, forward) = match (dx != 0.0, dy != 0.0) {
                (true, false) => (Axis::X, dx > 0.0),
                (false, true) => (Axis::Y, dy > 0.0),
                _ => return Err(bad(ID, "`snap.nudge` takes one direction: one of `dx` and `dy` is 0, and the other's sign is the way")),
            };
            let zoom = ask.map_or(crate::snap::DEFAULT_ZOOM, |a| a.zoom);
            let tick = tick.unwrap_or_else(|| snap_params::ruler_tick(read.as_deref(), axis, zoom));
            Some(crate::snap::Nudge { axis, forward, tick })
        }
        _ => None,
    };
    if let Some(a) = ask.as_mut() {
        a.pressed = nudge;
    }
    // A move that keeps to a page axis (`snap: {lock}`) does not move on the other, whatever its distance there.
    let (dx, dy) = match ask.and_then(|a| a.lock) {
        Some(Axis::X) => (dx, 0.0),
        Some(Axis::Y) => (0.0, dy),
        None => (dx, dy),
    };
    // The snap: the box of the shapes where the move would put it, pulled to the page's targets. It only adds to the distance asked for.
    let snapped = match (ask, s.document()) {
        (Some(ask), Some(doc)) => {
            // The box the shapes align by: round the paths of a turned shape, as the window's preview gets it.
            snap_params::bounds_of(doc, page, &ids).map(|b| {
                let (sx, sy) = if nudge.is_some() { (0.0, 0.0) } else { (dx, dy) };
                let r = b.rect;
                let moved = Rect { x0: r.x0 + sx, y0: r.y0 + sy, x1: r.x1 + sx, y1: r.y1 + sy };
                let own = snap_params::own_points(doc, page, &ids, (sx, sy));
                // One turned shape lines up by its own sides as well as by the box round its paths: its frame moves with the box.
                let frame = snap_params::frame_of_shapes(doc, page, &ids).map(|f| OwnFrame { centre: (f.centre.0 + sx, f.centre.1 + sy), ..f });
                let mut snapped = snap_params::run_with(doc, read.as_deref(), Subject::Box(moved), Use::Place, &ids, &own, Ask { frame, ..ask });
                // The shapes that kept the box round their corners are counted in the result. What their boxes cost is `b.cost`, paid by the budget of that function.
                snapped.corner_boxes = b.by_corners;
                snapped
            })
        }
        _ => None,
    };
    let (dx, dy) = match (&snapped, nudge) {
        // A press goes where the snap says. With the Snap switch off the snap says nothing, and the press is one tick (`snap_params::nudged`, which `snap.at` tells too).
        (_, Some(n)) => snap_params::nudged(snapped.as_ref(), n),
        (Some(sn), None) => (dx + sn.dx, dy + sn.dy),
        (None, None) => (dx, dy),
    };
    // A move that moves nothing (a connector with both ends glued) is no undo step and does not make the drawing unsaved.
    let k = page_scale(s, page);
    let (mut out, held) = s.edit_if("Move shapes", |doc| {
        // A member or a callout whose move a GUARD refuses is left where it is, and said.
        let draw::ShapesMoved { moved, left } = draw::move_shapes_leaving(doc, page, &ids, dx * k, dy * k).map_err(|p| p.into_engine(ID))?;
        let mut passed = Vec::new();
        if moved > 0 {
            draw::fit_page(doc, page, &ids).map_err(|p| p.into_engine(ID))?;
            // A shape moved into or out of a container joins or leaves it, and the containers are fitted; a container on a locked layer is
            // passed over, and noted.
            passed = draw::container::arrive(doc, page, &ids).map_err(|p| p.into_engine(ID))?.locked;
        }
        // The shapes whose own edge the snap put on a guide are held to it; the move broke any hold it did not renew.
        let mut held = snapped.map(|sn| hold(doc, page, &ids, sn)).transpose().map_err(|p| p.into_engine(ID))?;
        let changed = moved > 0 || held.as_ref().is_some_and(Held::wrote);
        if let Some(h) = held.as_mut() {
            h.changed = changed;
        }
        let mut out = json!({ "page": page, "moved": moved });
        super::container::note_locked(doc, page, &passed, &mut out).map_err(|p| p.into_engine(ID))?;
        note_left(&left, &mut out);
        Ok(((out, held), changed))
    })?;
    add_snap(&mut out, held.as_ref());
    Ok(out)
}

/// The followers a move left where they were (`draw::move_shapes_leaving`): their `ID`s under `left`, and the words that name them among the answer's `notes`.
fn note_left(left: &[(u32, &'static str)], out: &mut Value) {
    let Some(words) = draw::left_words(left) else { return };
    if let Some(map) = out.as_object_mut() {
        map.insert("left".to_owned(), json!(left.iter().map(|(id, _)| *id).collect::<Vec<u32>>()));
        let entry = map.entry("notes").or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(list) = entry {
            list.push(Value::String(words));
        }
    }
}

/// Sets the frame of a shape → `{page, id}`, or with `ids` makes the joint box of several shapes a new box → `{page, resized, locked?}`.
fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.resize";
    let named = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    match (named("id"), named("ids")) {
        (true, true) => Err(bad(ID, "give `id` (one shape, by its own frame) or `ids` (shapes resized as one, by their joint box), not both")),
        (false, true) => resize_set(s, p),
        _ => resize_one(s, p),
    }
}

/// Makes the joint box of the shapes `ids` the box `x`, `y`, `width`, `height` (paper inches on the page) → `{page, resized, locked?}`: how many shapes changed, and the
/// shapes their locks left alone. A resize to the box the shapes have is no undo step.
fn resize_set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.resize";
    let ids = required_ids(p, "ids", ID)?;
    let (x, y) = (required_number(p, "x", ID)?, required_number(p, "y", ID)?);
    let (width, height) = (required_number(p, "width", ID)?, required_number(p, "height", ID)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(bad(ID, "`width` and `height` must be more than 0"));
    }
    let page = target_page(s, p, &ids, ID)?;
    let ask = snap_params::parse(p, ID, snap_params::Words::RESIZE)?;
    if ask.is_some() && ids.len() > snap_params::MAX_SNAPPED_SHAPES {
        return Err(bad(ID, format!("`snap` takes at most {} shapes", snap_params::MAX_SNAPPED_SHAPES)));
    }
    // The snap: the edges of the joint box the resize moves, against the page's targets, the shapes that are resized never one.
    let snapped = match (ask, s.document()) {
        (Some(ask), Some(doc)) if width > 0.0 && height > 0.0 => {
            let read = snap_params::read_for(s, page);
            crate::snap::snap_resize_set(doc, page, read.as_deref(), &ids, (x, y, width, height), ask.zoom, ask.extras())
        }
        _ => None,
    };
    let (x, y, width, height) = snapped.as_ref().map_or((x, y, width, height), |(_, sized)| *sized);
    if snapped.as_ref().is_some_and(|(sn, _)| sn.landed.is_some()) {
        let limit = draw::MAX_COORD / page_scale(s, page);
        if ![x, y, x + width, y + height, width, height].iter().all(|v| v.abs() <= limit) {
            return Err(bad(
                ID,
                format!(
                    "`snap.keep` keeps the proportions of the shapes' joint box, and for these shapes they come to a box of {width} by {height} inches, past the {limit} the page allows: leave `keep` out"
                ),
            ));
        }
    }
    let k = page_scale(s, page);
    let (mut out, held) = s.edit_if("Resize shapes", |doc| {
        let scaled = draw::resize_shapes(doc, page, &ids, (x * k, y * k, width * k, height * k)).map_err(|p| p.into_engine(ID))?;
        let changed = scaled.resized > 0;
        let mut passed = Vec::new();
        if changed {
            draw::fit_page(doc, page, &ids).map_err(|p| p.into_engine(ID))?;
            // The containers that hold the shapes are fitted, and the shapes on the edges of a container that was resized go to them; a container on a locked
            // layer is passed over, and noted.
            passed = draw::container::changed(doc, page, &ids).map_err(|p| p.into_engine(ID))?.locked;
        }
        // The shapes whose own edge the snap put on a guide are held to it, as a move holds them.
        let mut held = snapped.map(|(sn, _)| hold(doc, page, &ids, sn)).transpose().map_err(|p| p.into_engine(ID))?;
        let changed = changed || held.as_ref().is_some_and(Held::wrote);
        if let Some(h) = held.as_mut() {
            h.changed = changed;
        }
        let mut out = json!({ "page": page, "resized": scaled.resized });
        if let (Some(map), false) = (out.as_object_mut(), scaled.locked.is_empty()) {
            map.insert("locked".into(), json!(scaled.locked));
        }
        super::container::note_locked(doc, page, &passed, &mut out).map_err(|p| p.into_engine(ID))?;
        Ok(((out, held), changed))
    })?;
    add_snap(&mut out, held.as_ref());
    Ok(out)
}

/// Sets the frame of one shape → `{page, id}`.
fn resize_one(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.resize";
    let id = required_u32(p, "id", ID)?;
    let (x, y) = (required_number(p, "x", ID)?, required_number(p, "y", ID)?);
    let (width, height) = (required_number(p, "width", ID)?, required_number(p, "height", ID)?);
    let page = target_page(s, p, &[id], ID)?;
    // The snap: the edges the resize moves, against the page's targets. A shape that is turned or flipped is snapped along its own axes; one whose corners on the page are
    // no rectangle is not snapped.
    let ask = snap_params::parse(p, ID, snap_params::Words::RESIZE)?;
    let (snapped, kept) = match (ask, s.document()) {
        (Some(ask), Some(doc))
            if width > 0.0 && height > 0.0 && (snap_params::is_square(doc, page, id) || snap_params::snaps_in_its_own_axes(doc, page, id)) =>
        {
            // The page as the session kept it (the window's reader made its index of the shapes and its targets there), else read by the snap, as before.
            let read = snap_params::read_for(s, page);
            (snap_resize(doc, read.as_deref(), page, id, (x, y, width, height), ask), crate::snap::hold::holds_of(doc, page, id))
        }
        _ => (None, Vec::new()),
    };
    let (x, y, width, height) = snapped.as_ref().map_or((x, y, width, height), |(_, sized)| *sized);
    // The proportions of a shape that is very thin or very flat are a ratio of millions, and the box they ask for is past the page's reach (a shape 1e-12 inch wide was
    // refused for a `y` it never was given): the refusal says what asked for it, when the snap gave a box that keeps them (`landed`).
    if snapped.as_ref().is_some_and(|(sn, _)| sn.landed.is_some() || (sn.frame.is_some() && ask.is_some_and(|a| a.keep))) {
        let limit = draw::MAX_COORD / page_scale(s, page);
        if ![x, y, x + width, y + height, width, height].iter().all(|v| v.abs() <= limit) {
            return Err(bad(
                ID,
                format!(
                    "`snap.keep` keeps the shape's own proportions, and for this shape they come to a box of {width} by {height} inches, past the {limit} the page allows: leave `keep` out"
                ),
            ));
        }
    }
    let (mut out, held) = edit_page(s, ID, "Resize shape", page, |doc, k| {
        draw::resize_shape(doc, page, id, x * k, y * k, width * k, height * k)?;
        draw::fit_page(doc, page, &[id])?;
        // A container on a locked layer that holds it is passed over, and noted.
        let passed = draw::container::changed(doc, page, &[id])?.locked;
        // Held to the guides the moved edges were pulled to, and on an axis none pulled, to those it was held to where the part of its box held is still on
        // the guide after the resize (`hold_shapes` holds a shape only where its edge or middle is on the line, so an edge the resize does not
        // move stays held, and one it moves off is not).
        let mut snapped = snapped.map(|(sn, _)| sn);
        let mut holds: Vec<crate::snap::Hold> = snapped.as_ref().map(|sn| sn.holds.clone()).unwrap_or_default();
        let pulled: Vec<Axis> = holds.iter().map(|h| h.axis).collect();
        holds.extend(kept.into_iter().filter(|h| !pulled.contains(&h.axis)));
        let holding = crate::snap::hold::hold_shapes(doc, page, &[id], &holds)?;
        if let Some(sn) = snapped.as_mut() {
            sn.holds = holds;
        }
        let mut out = json!({ "page": page, "id": id });
        super::container::note_locked(doc, page, &passed, &mut out)?;
        Ok((out, snapped.map(|snapped| Held { snapped, holding, changed: true })))
    })?;
    add_snap(&mut out, held.as_ref());
    Ok(out)
}

/// The snap of a resize: the box `(x, y, width, height)` paper inches, against the page's targets, by the edges that differ from the shape's box now
/// (`snap::snap_resize`, which a window's preview calls too). → the snap, and the box it comes to; `None` when no edge moves.
fn snap_resize(
    doc: &diacraft_doc::Document,
    read: Option<&crate::snap::SnapPage>,
    page: u32,
    id: u32,
    sized: (f64, f64, f64, f64),
    ask: Ask,
) -> Option<(crate::snap::Snapped, (f64, f64, f64, f64))> {
    crate::snap::snap_resize_ext(doc, page, read, id, sized, ask.zoom, ask.extras())
}

/// Deletes shapes → `{page, deleted, connectors_deleted}`: `deleted` counts every shape that went (the shapes named, what the containers among them held, and the connectors taken
/// along), `connectors_deleted` the `ID`s of those taken along. **A connector glued to a shape that goes goes with it** (the Delete of the specifications removes the
/// connectors attached to the selection), except one on a locked layer or on hidden ones only, which stays with a free end as in the window; `connectors: false` leaves every
/// connector with that end a plain point where it was.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.delete";
    let ids = required_ids(p, "ids", ID)?;
    let with_connectors = optional_bool(p, "connectors", ID)?.unwrap_or(true);
    let page = target_page(s, p, &ids, ID)?;
    edit_page(s, ID, "Delete shapes", page, |doc, _| {
        // A member of a locked container is not deleted (but the container is, with all it holds), and a container takes what it holds with it. The containers
        // that held what is deleted are fitted after.
        draw::container::check_delete(doc, page, &ids)?;
        let mut all = draw::container::with_contents(doc, page, &ids)?;
        // A member on a locked layer is not deleted with its container.
        if all.len() > ids.len() {
            let named: std::collections::HashSet<u32> = ids.iter().copied().collect();
            let members: Vec<u32> = all.iter().copied().filter(|id| !named.contains(id)).collect();
            if let Some((id, layer)) = draw::layers::first_locked(doc, page, &members)? {
                return Err(draw::Problem::bad(draw::layers::follower_words(id, &layer, "deleted")));
            }
        }
        // A group that holds a shape on a locked layer is not deleted, named or held by a container deleted.
        draw::layers::refuse_locked_in_group(doc, page, &all, "deleted")?;
        // The connectors glued to what goes go too: the shapes and what they hold are known, and a connector that is a member of a locked container is refused as one named.
        let connectors = if with_connectors { draw::glued_connectors(doc, page, &all)? } else { Vec::new() };
        if !connectors.is_empty() {
            all.extend(connectors.iter().copied());
            draw::container::check_delete(doc, page, &all)?;
        }
        let holders = draw::container::holding(doc, page, &all)?;
        let deleted = draw::delete_shapes(doc, page, &all)?;
        // No container or member keeps a tie to a shape that is gone, so that a new shape given its `ID` after a save is not a member of anything.
        draw::container::release(doc, page, &all.iter().copied().collect())?;
        let fitted = draw::container::fit_after(doc, page, &holders)?;
        let mut out = json!({ "page": page, "deleted": deleted, "connectors_deleted": connectors });
        super::container::note_locked(doc, page, &fitted.locked, &mut out)?;
        Ok(out)
    })
}

/// Replaces a shape's text, or the characters `start` to `end` of it, keeping its runs and fields → `{page, id}`. Text that is the text the shape
/// already has changes nothing and is no undo step.
fn set_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.set_text";
    let id = required_u32(p, "id", ID)?;
    let text = required_text(p, "text", ID)?;
    let range = optional_range(p, ID)?;
    let page = target_page(s, p, &[id], ID)?;
    s.edit_if("Set text", |doc| {
        let changed = match range {
            Some((start, end)) => draw::set_text_range(doc, page, id, start, end, text),
            None => draw::set_text(doc, page, id, text),
        }
        .map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id }), changed))
    })
}

/// The drawing units to a paper inch on a page (see [`edit_page`]).
fn page_scale(s: &Session, page: u32) -> f64 {
    s.document().map_or(1.0, |doc| units_per_inch(doc, page))
}

/// The page the command works on (see [`page_for`]).
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}
