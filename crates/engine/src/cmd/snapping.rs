//! Snap & Glue and guides: `snap.settings`, `snap.set`, `snap.at`, and `guide.add`, `guide.move`, `guide.delete`, `guide.list`.
//!
//! The settings are the document's (`diacraft_engine::snap::settings`: the words of `DocumentSettings`), and `snap.set` changes them and **is no undo step** (the
//! Which extends the owner's of 2026-11-22, "Unsaved, not undo"): the drawing is unsaved, so a save keeps them, and Ctrl+Z goes on undoing the drawing's
//! edits and leaves them as they are set ([`Session::edit_snap_setting`]). A guide is a page shape (`snap::guides`), and its commands are undo steps. Lengths are paper inches, as
//! the other editing commands have them.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids, required_number};
use super::snap_params::{self, Ask, resized_by, snapped_json};
use super::{CommandSpec, has_document, required_text, required_u32};
use crate::Session;
use crate::error::{Result, bad, failed};
use crate::snap::guides::{add_guide, delete_guide, list_guides, move_guide};
use crate::snap::{Axis, DEFAULT_ZOOM, Edges, Nudge, OPTIONS, OwnFrame, Rect, STRENGTH_MAX, STRENGTHS, SnapSettings, Subject, Use};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "snap.settings",
            "Snap & Glue Settings",
            ["View", "Visual Aids"],
            None,
            "none",
            has_document,
            settings
        ),
        cmd!(
            "snap.set",
            "Snap & Glue",
            ["View", "Visual Aids"],
            None,
            "snap: boolean, optional, the Snap switch (SnapToggle); glue: boolean, optional, the Glue switch (GlueToggle); \
             ruler: boolean, optional, snap to the ruler's subdivisions (flag 1 of SnapSettings); grid: boolean, optional, snap to the grid (2); guides: boolean, optional, snap to guides (4); \
             snap_handles: boolean, optional, a shape snaps to the corners and side middles of other shapes' boxes (8); snap_vertices: boolean, optional, a shape snaps to other shapes' vertices (16); \
             snap_connection_points: boolean, optional, a shape snaps to other shapes' connection points (32); snap_geometry: boolean, optional, a shape snaps to other shapes' outlines (256); \
             alignment_box: boolean, optional, snap to the edges and middles of other shapes (512); snap_extensions: boolean, optional, snap to shape extensions (1024); \
             intersections: boolean, optional, snap to where shape geometry crosses (65536); \
             glue_guides: boolean, optional, a connector's end glues to guides (flag 1 of GlueSettings); glue_handles: boolean, optional, to the corners and side middles of a shape's box (2); \
             glue_vertices: boolean, optional, to a shape's vertices (4); glue_connection_points: boolean, optional, to connection points (8); glue_geometry: boolean, optional, to a point on a shape's outline (32); \
             ext_alignment_box: boolean, optional, extension line (flag 1 of SnapExtensions); ext_centre_axes: boolean, optional, (2); ext_tangents: boolean, optional, (4); \
             ext_segment_ends: boolean, optional, (8); ext_segment_midpoints: boolean, optional, (16); ext_linear: boolean, optional, (32); ext_curved: boolean, optional, (64); \
             ext_perpendicular_end: boolean, optional, (128); ext_perpendicular_mid: boolean, optional, (256); ext_horizontal_end: boolean, optional, (512); \
             ext_vertical_end: boolean, optional, (1024); ext_ellipse_centres: boolean, optional, (2048); ext_isometric: boolean, optional, (4096); \
             dynamic_grid: boolean, optional, the dynamic grid (DynamicGridEnabled: alignment with other shapes, the page margins, equal spacing); \
             strength_ruler: whole number, optional, 0 to 999 screen pixels, how far the ruler's subdivisions pull a shape (4 at first, the Rulers slider of the Advanced tab); \
             strength_grid: whole number, optional, 0 to 999 pixels, how far the grid's lines pull (5 at first, Grid); strength_guides: whole number, optional, 0 to 999 pixels, guides (10 at first, Guides); \
             strength_points: whole number, optional, 0 to 999 pixels, connection points, vertices and handles (10 at first, Points); \
             strength_geometry: whole number, optional, 0 to 999 pixels, shape outlines and where they cross (8 at first, Geometry); \
             strength_extensions: whole number, optional, 0 to 999 pixels, the dynamic grid's alignment lines, margins and spacings (13 at first, Extensions)",
            has_document,
            set
        ),
        cmd!(
            query "snap.at",
            "Snap At",
            [],
            None,
            "page: whole number, the id of the page; \
             x: number, optional, a place in inches from the page's left edge (give x and y for a place, or left, bottom, right and top for a box); y: number, optional, inches from the bottom edge; \
             left: number, optional, a box's left edge; bottom: number, optional; right: number, optional; top: number, optional; \
             ids: array of whole numbers, optional, the shapes that move (never a target, and with a box their connection points are pulled with it, where the box would put them), one shape with keep or rotate; \
             connect: boolean, optional, the place is the end of a connector (glue targets as well), and goes with x and y alone: not with a box, edges, nudge, keep or rotate; \
             zoom: number, optional, screen points to an inch (default 96); \
             edges: array of text, optional, with a box: the sides of it that a resize moves, from left, bottom, right and top, and the answer's box is the one the resize comes to; \
             keep: text, optional, proportions, with edges: the resize keeps the proportions of the one shape in ids and snaps inside them, the answer's landed being the box; \
             lock: text, optional, x or y, with a place or a box: the move keeps to that page axis, the snap goes along it only, and the box is where the move keeps it; \
             nudge: object, optional, with a box: {direction, tick}, one press of an arrow key, direction being right, left, up or down, so the box goes to the next place it can snap to that way however far, \
             or by tick inches (the ruler's tick at the zoom when it is left out) when nothing is ahead, and dx and dy being the press; \
             view: array of four numbers, optional, [x0, y0, x1, y1], the visible part of the page in inches, so a resize stops at the width or height of a shape in it (every shape when it is left out); \
             rotate: object, optional, with one id and no place or box: {angle, radius}, a turn by the rotation handle, angle being the pointer's in degrees counterclockwise and radius how far it is from the pin in inches, \
             and the answer's angle being where the turn comes to",
            has_document,
            at
        ),
        cmd!(
            "guide.add",
            "Add Guide",
            ["View", "Visual Aids"],
            None,
            "page: whole number, the id of the page; orientation: text, vertical or horizontal; at: number, inches from the page's left edge for a vertical guide, from its bottom edge for a horizontal one",
            has_document,
            guide_add
        ),
        cmd!(
            "guide.move",
            "Move Guide",
            ["View", "Visual Aids"],
            None,
            "id: whole number, the guide; at: number, its new place in inches; page: whole number, optional, the id of the page (default: the first page with the guide)",
            has_document,
            guide_move
        ),
        cmd!(
            "guide.delete",
            "Delete Guide",
            ["View", "Visual Aids"],
            None,
            "id: whole number, the guide; page: whole number, optional, the id of the page (default: the first page with the guide)",
            has_document,
            guide_delete
        ),
        cmd!(
            query "guide.list",
            "List Guides",
            [],
            None,
            "page: whole number, optional, the id of the page (default: the first page)",
            has_document,
            guide_list
        ),
    ]
}

/// The most characters of a word that `snap.settings` shows.
const MAX_WORD_SHOWN: usize = 64;

/// The settings as JSON, and the words they are held in.
fn settings_json(s: &SnapSettings, words: &diacraft_doc::SnapWords) -> Value {
    // A word is whatever a file holds, up to a megabyte: it is escaped and cut as other text from a file is (`shown`), so that a hostile one costs little to read.
    let word = |w: &Option<std::sync::Arc<str>>| w.as_deref().map_or(Value::Null, |t| json!(crate::shown(t, MAX_WORD_SHOWN)));
    let mut out = serde_json::Map::new();
    for (index, option) in OPTIONS.iter().enumerate() {
        out.insert(option.name.to_owned(), Value::Bool(s.get(index)));
    }
    for (index, def) in STRENGTHS.iter().enumerate() {
        out.insert(def.name.to_owned(), json!(s.strength.get(index)));
    }
    out.insert(
        "words".to_owned(),
        json!({ "SnapSettings": word(&words.snap), "GlueSettings": word(&words.glue), "SnapExtensions": word(&words.extensions), "DynamicGridEnabled": word(&words.dynamic_grid) }),
    );
    Value::Object(out)
}

/// → the document's Snap & Glue settings, each option a boolean, the six snap strengths (`strength_ruler`, `strength_grid`, `strength_guides`, `strength_points`,
/// `strength_geometry` and `strength_extensions`: whole screen pixels from 0 to 999), and `words`: the text of `SnapSettings`, `GlueSettings`, `SnapExtensions` and
/// `DynamicGridEnabled` as the document holds them (`null` for one it lacks). The options are the flags of those words; only the
/// two switches, `snap` and `glue`, and the strengths are DiaCraft's own, kept in its part of the file.
fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed("snap.settings", "no document open"))?;
    Ok(settings_json(&SnapSettings::of(doc), &doc.snap_words()))
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The screen pixels `p` gives for the strength `key`: a whole number from 0 to 999, else a refusal in words.
fn pixels(p: &Value, key: &str, cmd: &str) -> Result<Option<u16>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n <= STRENGTH_MAX)
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number of screen pixels from 0 to {STRENGTH_MAX}"))),
    }
}

/// Changes the options and the snap strengths named → the settings as `snap.settings` has them. The drawing is changed (unsaved) and **no undo step is made** (they are
/// settings of View > Visual Aids, "unsaved, not undo"; undo and redo leave them as they are set); nothing changes when every value given is the one the drawing holds.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "snap.set";
    let mut given: Vec<(usize, bool)> = Vec::new();
    for (index, option) in OPTIONS.iter().enumerate() {
        if let Some(on) = flag(p, option.name, ID)? {
            given.push((index, on));
        }
    }
    let mut strengths: Vec<(usize, u16)> = Vec::new();
    for (index, def) in STRENGTHS.iter().enumerate() {
        if let Some(px) = pixels(p, def.name, ID)? {
            strengths.push((index, px));
        }
    }
    s.edit_snap_setting(ID, |doc| {
        let before = SnapSettings::of(doc);
        let mut now = before;
        for (index, on) in &given {
            now.put(*index, *on);
        }
        for (index, px) in &strengths {
            now.strength.put(*index, *px);
        }
        let changed = now != before;
        if changed && !now.apply(doc) {
            return Err(failed(
                ID,
                "a word of the file's DocumentSettings is text that is not a whole number or a boolean, or the file's DiaCraft data is of a version this build does not change, so these options cannot be changed here",
            ));
        }
        Ok((settings_json(&now, &doc.snap_words()), changed))
    })
}

/// The words of `snap.at` beyond a place or a box: the design's subjects `edges`, `nudge` and `rotate`, and the words `lock`, `keep` and `view` (each is the word of the same
/// name that a command takes in its `snap`, and the query tells what the command would do).
struct AtWords {
    /// The sides of the box that move: a resize.
    edges: Option<Edges>,
    keep: bool,
    lock: Option<Axis>,
    /// One press of an arrow key: the axis, whether it goes toward larger numbers, and its tick in inches (`None`: the ruler's at the zoom).
    nudge: Option<(Axis, bool, Option<f64>)>,
    view: Option<[f64; 4]>,
    /// A turn by the rotation handle: the pointer's angle in degrees, and how far it is from the pin in inches.
    rotate: Option<(f64, f64)>,
}

/// The most degrees a `rotate.angle` may be: past any angle a person types.
const MAX_ROTATE_DEGREES: f64 = 1.0e6;

impl AtWords {
    fn of(p: &Value, cmd: &str) -> Result<AtWords> {
        let edges = match p.get("edges") {
            None | Some(Value::Null) => None,
            Some(Value::Array(sides)) => {
                let refuse = || bad(cmd, "`edges` must name at least one of \"left\", \"bottom\", \"right\" and \"top\", as an array of text");
                let mut e = Edges::default();
                for side in sides {
                    match side.as_str() {
                        Some("left") => e.left = true,
                        Some("bottom") => e.bottom = true,
                        Some("right") => e.right = true,
                        Some("top") => e.top = true,
                        _ => return Err(refuse()),
                    }
                }
                if !(e.left || e.bottom || e.right || e.top) {
                    return Err(refuse());
                }
                Some(e)
            }
            Some(_) => return Err(bad(cmd, "`edges` must name at least one of \"left\", \"bottom\", \"right\" and \"top\", as an array of text")),
        };
        let nudge = match p.get("nudge") {
            None | Some(Value::Null) => None,
            Some(Value::Object(n)) => {
                if let Some(unknown) = n.keys().find(|k| !matches!(k.as_str(), "direction" | "tick")) {
                    return Err(bad(cmd, format!("`nudge` has an unknown key `{}`; it takes direction and tick", crate::error::shown(unknown))));
                }
                let (axis, forward) = match n.get("direction").and_then(Value::as_str) {
                    Some("right") => (Axis::X, true),
                    Some("left") => (Axis::X, false),
                    Some("up") => (Axis::Y, true),
                    Some("down") => (Axis::Y, false),
                    _ => return Err(bad(cmd, "`nudge.direction` must be \"right\", \"left\", \"up\" or \"down\": the way the arrow key goes")),
                };
                Some((axis, forward, snap_params::tick_word(n.get("tick"), cmd, "nudge.tick")?))
            }
            Some(_) => return Err(bad(cmd, "`nudge` must be an object {direction, tick}")),
        };
        let rotate = match p.get("rotate") {
            None | Some(Value::Null) => None,
            Some(Value::Object(r)) => {
                if let Some(unknown) = r.keys().find(|k| !matches!(k.as_str(), "angle" | "radius")) {
                    return Err(bad(cmd, format!("`rotate` has an unknown key `{}`; it takes angle and radius", crate::error::shown(unknown))));
                }
                let angle = r
                    .get("angle")
                    .and_then(Value::as_f64)
                    .filter(|a| a.is_finite() && a.abs() <= MAX_ROTATE_DEGREES)
                    .ok_or_else(|| bad(cmd, format!("`rotate.angle` must be a number of degrees, at most {MAX_ROTATE_DEGREES} either way")))?;
                let radius = r
                    .get("radius")
                    .and_then(Value::as_f64)
                    .filter(|r| r.is_finite() && *r >= 0.0)
                    .ok_or_else(|| bad(cmd, "`rotate.radius` must be a number, 0 or more: how far the pointer is from the pin, in inches"))?;
                Some((angle, radius))
            }
            Some(_) => return Err(bad(cmd, "`rotate` must be an object {angle, radius}")),
        };
        Ok(AtWords {
            edges,
            keep: snap_params::keep_word(p.get("keep"), cmd, "keep")?,
            lock: snap_params::lock_word(p.get("lock"), cmd, "lock")?,
            nudge,
            view: snap_params::view_word(p.get("view"), cmd, "view")?,
            rotate,
        })
    }
}

/// → `{x, y, dx, dy, hits, marks, glue, truncated, examined}` for a place, or `{left, bottom, right, top, dx, dy, ...}` for a box: where the snap would put it.
/// With `edges` the box is a resize and the answer's box is the one it comes to (and `landed`, with `keep`); with `nudge` it is one press of an arrow key and `dx` and `dy` are
/// the press; with `lock` the snap goes along that axis only; with `rotate` the answer is the turn's (`angle`, `hits`, `marks`). Nothing is changed.
fn at(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "snap.at";
    let page = required_u32(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    if doc.page(page).is_none() {
        return Err(bad(ID, format!("there is no page {page}")));
    }
    // The page as the session kept it (the window's reader made its index of the shapes and its targets there), else read here, as before.
    let read = snap_params::read_for(s, page);
    let n = |key: &str| optional_number(p, key, ID);
    let (x, y) = (n("x")?, n("y")?);
    let (left, bottom, right, top) = (n("left")?, n("bottom")?, n("right")?, n("top")?);
    let words = AtWords::of(p, ID)?;
    let moving: Vec<u32> = match p.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => required_ids(p, "ids", ID)?,
    };
    let connect = flag(p, "connect", ID)? == Some(true);
    let using = if connect { Use::Connect } else { Use::Place };
    let zoom = optional_number(p, "zoom", ID)?.unwrap_or(DEFAULT_ZOOM);
    if !crate::snap::takes_zoom(zoom) {
        return Err(bad(ID, format!("`zoom` must be a number above 0, {}", snap_params::ZOOM_WORDS)));
    }
    let has_box = left.is_some() || bottom.is_some() || right.is_some() || top.is_some();
    // The end of a connector is a place: with a box, a resize, a press, a kept ratio or a turn it would be taken for something it does not do (a box asked as the end of a connector is
    // pulled by no place of the shapes it lines up with, and the words of a resize, a press and a turn have no end to glue), so it is refused in words.
    if connect && (has_box || words.edges.is_some() || words.nudge.is_some() || words.keep || words.rotate.is_some()) {
        return Err(bad(
            ID,
            "`connect` is for a place, the end of a connector (give `x` and `y`): it does not go with a box (`left`, `bottom`, `right`, `top`), `edges`, `nudge`, `keep` or `rotate`",
        ));
    }
    if let Some((angle, radius)) = words.rotate {
        // A turn is a subject of its own: the one shape, and the pointer's angle and distance.
        if x.is_some() || y.is_some() || has_box || words.edges.is_some() || words.nudge.is_some() || words.lock.is_some() || words.keep {
            return Err(bad(
                ID,
                "`rotate` is a subject of its own: it takes `ids` and `zoom`, and no place, box, `edges`, `nudge`, `lock` or `keep`",
            ));
        }
        let [id] = moving.as_slice() else { return Err(bad(ID, "`rotate` turns one shape: name it in `ids`")) };
        let ask = Ask::at_zoom(zoom);
        let snapped = snap_params::run_turn(doc, read.as_deref(), page, *id, angle.to_radians(), radius, ask);
        return Ok(snapped_json(&snapped));
    }
    let subject = match ((x, y), (left, bottom, right, top)) {
        ((Some(x), Some(y)), (None, None, None, None)) => Subject::Point((x, y)),
        ((None, None), (Some(l), Some(b), Some(r), Some(t))) => {
            let rect = Rect::of((l, b), (r, t));
            words.edges.map_or(Subject::Box(rect), |e| Subject::Edges(rect, e))
        }
        _ => return Err(bad(ID, "give x and y for a place, or left, bottom, right and top for a box, and not both")),
    };
    if words.edges.is_some() && !matches!(subject, Subject::Edges(..)) {
        return Err(bad(ID, "`edges` goes with a box: the sides of it that a resize moves"));
    }
    if words.nudge.is_some() && !matches!(subject, Subject::Box(_)) {
        return Err(bad(
            ID,
            "`nudge` goes with a box, and not with a place or with `edges`: the box the shapes are in now, which one press of an arrow key moves",
        ));
    }
    if words.lock.is_some() && matches!(subject, Subject::Edges(..)) {
        return Err(bad(ID, "`lock` is a move's word and does not go with `edges`, which is a resize"));
    }
    if words.lock.is_some() && words.nudge.is_some() {
        return Err(bad(ID, "`nudge` and `lock` do not go together: a press goes along its own axis"));
    }
    if words.keep && !matches!(subject, Subject::Edges(..)) {
        return Err(bad(ID, "`keep` goes with `edges`: it keeps the proportions of a resize"));
    }
    // The proportions a resize keeps are the shape's own, as it is now (`snap_resize` takes them from the shape's box the same way).
    let ratio = if words.keep {
        let [id] = moving.as_slice() else { return Err(bad(ID, "`keep` keeps the proportions of the one shape named in `ids`")) };
        let now = snap_params::box_of(doc, page, &[*id]).ok_or_else(|| bad(ID, format!("page {page} has no shape {id}")))?;
        if !(now.width() > 0.0 && now.height() > 0.0) {
            return Err(bad(ID, "`keep` keeps proportions, and the shape named in `ids` has no width or no height"));
        }
        Some(now.width() / now.height())
    } else {
        None
    };
    let press = words.nudge.map(|(axis, forward, tick)| Nudge {
        axis,
        forward,
        tick: tick.unwrap_or_else(|| snap_params::ruler_tick(read.as_deref(), axis, zoom)),
    });
    // The box the shapes align by is the box `shape.move` snaps (the box round a turned shape's paths), and the connection points of the shapes named in `ids` are pulled
    // with a box, where the box would put them.
    let bounds = match subject {
        Subject::Box(_) if !moving.is_empty() && moving.len() <= snap_params::MAX_SNAPPED_SHAPES => snap_params::bounds_of(doc, page, &moving),
        _ => None,
    };
    let own = match (subject, &bounds) {
        (Subject::Box(r), Some(b)) => snap_params::own_points(doc, page, &moving, (r.x0 - b.rect.x0, r.y0 - b.rect.y0)),
        _ => Vec::new(),
    };
    // The one turned shape named in `ids` lines up by its own sides, its frame where the box is, as `shape.move` does it.
    let frame = match (subject, &bounds) {
        (Subject::Box(r), Some(b)) => snap_params::frame_of_shapes(doc, page, &moving)
            .map(|f| OwnFrame { centre: (f.centre.0 + r.x0 - b.rect.x0, f.centre.1 + r.y0 - b.rect.y0), ..f }),
        _ => None,
    };
    let ask = Ask { lock: words.lock, view: words.view, keep: words.keep, ratio, pressed: press, frame, ..Ask::at_zoom(zoom) };
    let mut snapped = snap_params::run_with(doc, read.as_deref(), subject, using, &moving, &own, ask);
    if let Some(b) = &bounds {
        snapped.corner_boxes = b.by_corners;
    }
    let mut out = snapped_json(&snapped);
    if let Some(map) = out.as_object_mut() {
        match subject {
            Subject::Point((x, y)) => {
                let (nx, ny) = snapped.apply((x, y));
                map.insert("x".into(), json!(nx));
                map.insert("y".into(), json!(ny));
            }
            Subject::Box(r) => {
                // A press is the distance the command would move the shapes: the snap's, or the tick when it says nothing.
                let (dx, dy) = press.map_or((snapped.dx, snapped.dy), |n| snap_params::nudged(Some(&snapped), n));
                map.insert("dx".into(), json!(dx));
                map.insert("dy".into(), json!(dy));
                map.insert("left".into(), json!(r.x0 + dx));
                map.insert("bottom".into(), json!(r.y0 + dy));
                map.insert("right".into(), json!(r.x1 + dx));
                map.insert("top".into(), json!(r.y1 + dy));
            }
            Subject::Edges(r, _) => {
                // The box the resize comes to, as `shape.resize` takes it: a snap that would collapse the box is not taken.
                let (bx, by, bw, bh) = resized_by((r.x0, r.y0, r.width(), r.height()), &snapped).unwrap_or((r.x0, r.y0, r.width(), r.height()));
                map.insert("left".into(), json!(bx));
                map.insert("bottom".into(), json!(by));
                map.insert("right".into(), json!(bx + bw));
                map.insert("top".into(), json!(by + bh));
            }
        }
    }
    Ok(out)
}

fn axis_of(p: &Value, cmd: &str) -> Result<Axis> {
    match required_text(p, "orientation", cmd)? {
        "vertical" => Ok(Axis::X),
        "horizontal" => Ok(Axis::Y),
        _ => Err(bad(cmd, "`orientation` must be `vertical` or `horizontal`")),
    }
}

fn guide_json(page: u32, g: &crate::snap::guides::GuideInfo) -> Value {
    json!({ "page": page, "id": g.id, "orientation": if g.axis == Axis::X { "vertical" } else { "horizontal" }, "at": g.at })
}

/// Adds a guide → `{page, id, orientation, at}`.
fn guide_add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "guide.add";
    let page = required_u32(p, "page", ID)?;
    let axis = axis_of(p, ID)?;
    let at = required_number(p, "at", ID)?;
    let above = s.id_floor(page);
    let info = s.edit("Add guide", |doc| add_guide(doc, page, axis, at, above).map_err(|e| e.into_engine(ID)))?;
    s.note_id(page, info.id);
    Ok(guide_json(page, &info))
}

fn guide_page(s: &Session, p: &Value, id: u32, cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, &[id], cmd)
}

/// Moves a guide → `{page, id, at, moved}`.
fn guide_move(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "guide.move";
    let id = required_u32(p, "id", ID)?;
    let at = required_number(p, "at", ID)?;
    let page = guide_page(s, p, id, ID)?;
    s.edit_if("Move guide", |doc| {
        let moved = move_guide(doc, page, id, at).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "at": at, "moved": moved }), moved))
    })
}

/// Deletes a guide → `{page, id}`.
fn guide_delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "guide.delete";
    let id = required_u32(p, "id", ID)?;
    let page = guide_page(s, p, id, ID)?;
    s.edit("Delete guide", |doc| delete_guide(doc, page, id).map_err(|e| e.into_engine(ID)))?;
    Ok(json!({ "page": page, "id": id }))
}

/// → `{page, guides: [{id, orientation, at}], left_out}`: the page's guides that lie along an axis, and how many do not (a guide at another angle is
/// kept in the file and is a snap target only for a turned shape one of whose sides is parallel to it).
fn guide_list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "guide.list";
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match optional_whole(p, "page", ID)? {
        Some(page) => page,
        None => doc.pages.iter().next().map(|pg| pg.id).ok_or_else(|| bad(ID, "the document has no pages"))?,
    };
    let (guides, left_out) = list_guides(doc, page).ok_or_else(|| bad(ID, format!("there is no page {page}")))?;
    let listed: Vec<Value> =
        guides.iter().map(|g| json!({ "id": g.id, "orientation": if g.axis == Axis::X { "vertical" } else { "horizontal" }, "at": g.at })).collect();
    Ok(json!({ "page": page, "guides": listed, "left_out": left_out }))
}
