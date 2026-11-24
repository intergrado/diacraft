//! The `snap` parameter of the editing commands, and the JSON a snap is told in.
//!
//! `snap` is left out, `false` or `null` for no snapping (every command's behaviour before), `true` to snap by the document's own settings
//! (`snap.settings`), or `{zoom}`: the screen points to a paper inch, which a variable grid's pitch and how far a target pulls depend on (100% zoom,
//! 96, when it is not given). **What a command does with it** (the same function the window's drags call, [`crate::snap::snap`]): `shape.drop` snaps the
//! dropped shape's box, `shape.move` the box of the shapes moved, `shape.resize` the edges that the resize moves, `connector.add` and
//! `connector.reconnect` a free end given as `{x, y}`. The document's Snap switch off makes the parameter do nothing.
//!
//! **Each command takes the words it has** ([`Words`]): `zoom` and `view` (the window sends the view with every snapping command, one rule for all, and a size match is
//! the only thing that reads it) are every command's, and `lock` and `nudge` are `shape.move`'s, `keep` is `shape.resize`'s and `radius` is `shape.rotate`'s. A word of
//! another command is refused in words, and not taken in silence (a `lock` on a drop moved the shape by what it was meant to leave alone).

use std::sync::Arc;

use diacraft_doc::Document;
use serde_json::{Map, Value, json};

use crate::error::{Result, bad};
use crate::session::Session;
use crate::snap::{
    Axis, DEFAULT_ZOOM, Extras, Glue, Kind, Nudge, OwnFrame, P, Query, SnapMark, SnapPage, SnapSettings, Snapped, Subject, Turn, Use, make_ready,
    pin_of, snap_ext, snap_turn,
};

// The box of the shapes that move, their own connection points, the edges a resize moves and whether a shape is square to the page are the snap
// module's (`snap/subject.rs`), so that a window's preview calls the same functions.
pub(super) use crate::snap::{
    MAX_OWN_SHAPES, MAX_SNAPPED_SHAPES, bounds_each, bounds_of, box_of, frame_of_shapes, is_square, own_points, own_points_each, resized_by,
    snaps_in_its_own_axes,
};

/// What the caller asked of snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ask {
    pub zoom: f64,
    /// A move that keeps to this page axis (`lock`): the other axis does not move, and the free axis snaps.
    pub lock: Option<Axis>,
    /// The window's visible part of the page, `[x0, y0, x1, y1]` in paper inches (`view`): a size match looks only at the shapes that meet it.
    pub view: Option<[f64; 4]>,
    /// `keep: "proportions"`: a resize keeps the shape's own proportions and snaps inside them.
    pub keep: bool,
    /// The width over the height a resize keeps, when the caller has it (`snap.at`; a resize works it out from the shape's box).
    pub ratio: Option<f64>,
    /// How far the pointer is from the pin, in paper inches (`radius`): a turn needs it, for how far a shape's angle pulls and for the rotation handle's step.
    pub radius: Option<f64>,
    /// `nudge: {tick}`: the move is one press of an arrow key (the direction is `dx` and `dy`); the inner number is `tick`, how far it moves when nothing is ahead,
    /// in inches, and `None` is the ruler's tick at the zoom.
    pub nudge: Option<Option<f64>>,
    /// The press the command worked out from `nudge`, `dx` and `dy`: which way it goes and how far with nothing ahead.
    pub pressed: Option<Nudge>,
    /// The own frame of the one turned shape that moves (`snap::frame_of_shapes`), where the box the call snaps is: the command that has the shape sets it.
    pub frame: Option<OwnFrame>,
}

impl Ask {
    /// An ask of nothing but the zoom: every word left out.
    pub fn at_zoom(zoom: f64) -> Ask {
        Ask { zoom, lock: None, view: None, keep: false, ratio: None, radius: None, nudge: None, pressed: None, frame: None }
    }

    /// What the snap is told besides the query.
    pub fn extras(&self) -> Extras {
        Extras { lock: self.lock, view: self.view, keep: self.keep, ratio: self.ratio, nudge: self.pressed, frame: self.frame }
    }
}

/// Which of the words of `snap` a command has beyond `zoom` and `view`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Words {
    pub lock: bool,
    pub keep: bool,
    pub radius: bool,
    pub nudge: bool,
}

impl Words {
    /// `zoom` and `view` only: a drop, a place, the end of a connector.
    pub const PLACE: Words = Words { lock: false, keep: false, radius: false, nudge: false };
    /// `shape.move`: the move keeps to an axis, or is one press of an arrow key.
    pub const MOVE: Words = Words { lock: true, nudge: true, ..Words::PLACE };
    /// `shape.resize`: the resize keeps the shape's proportions.
    pub const RESIZE: Words = Words { keep: true, ..Words::PLACE };
    /// `shape.rotate`: the turn knows how far the pointer is from the pin.
    pub const ROTATE: Words = Words { radius: true, ..Words::PLACE };

    /// The words, in the order the docs give them.
    fn names(&self) -> Vec<&'static str> {
        let mut names = vec!["zoom", "view"];
        for (has, name) in [(self.lock, "lock"), (self.nudge, "nudge"), (self.keep, "keep"), (self.radius, "radius")] {
            if has {
                names.push(name);
            }
        }
        names
    }

    /// "zoom and view", "zoom, view, lock and nudge".
    fn listed(&self) -> String {
        let names = self.names();
        match names.split_last() {
            Some((last, [])) => (*last).to_owned(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        }
    }
}

/// Every word of `snap` there is.
const ALL_WORDS: [&str; 6] = ["zoom", "lock", "view", "keep", "radius", "nudge"];

/// The axis a `lock` names, `name` being the word as the caller wrote it.
pub(super) fn lock_word(v: Option<&Value>, cmd: &str, name: &str) -> Result<Option<Axis>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(a)) if a == "x" => Ok(Some(Axis::X)),
        Some(Value::String(a)) if a == "y" => Ok(Some(Axis::Y)),
        Some(_) => Err(bad(cmd, format!("`{name}` must be \"x\" or \"y\""))),
    }
}

/// The four numbers of a `view`: `[x0, y0, x1, y1]`, every one a finite number.
pub(super) fn view_word(v: Option<&Value>, cmd: &str, name: &str) -> Result<Option<[f64; 4]>> {
    let refuse = || bad(cmd, format!("`{name}` must be four numbers [x0, y0, x1, y1]"));
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => {
            let numbers: Option<Vec<f64>> = a.iter().map(|n| n.as_f64().filter(|n| n.is_finite())).collect();
            match numbers.as_deref() {
                Some([x0, y0, x1, y1]) => Ok(Some([*x0, *y0, *x1, *y1])),
                _ => Err(refuse()),
            }
        }
        Some(_) => Err(refuse()),
    }
}

/// Whether `keep` is asked: the one word it has is `"proportions"`.
pub(super) fn keep_word(v: Option<&Value>, cmd: &str, name: &str) -> Result<bool> {
    match v {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(k)) if k == "proportions" => Ok(true),
        Some(_) => Err(bad(cmd, format!("`{name}` must be \"proportions\""))),
    }
}

/// What a zoom must be beside a number above 0, for the words of a refusal: one at which the strongest pull is a distance on the page ([`crate::snap::takes_zoom`]).
pub(super) const ZOOM_WORDS: &str = "at which the strongest pull (999 points) is a distance on the page (5.6E-306 points to an inch or more)";

/// The `tick` of a nudge, in inches: above 0, or `None` for the ruler's at the zoom.
pub(super) fn tick_word(v: Option<&Value>, cmd: &str, name: &str) -> Result<Option<f64>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(t) => Ok(Some(t.as_f64().filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| bad(cmd, format!("`{name}` must be a number above 0")))?)),
    }
}

/// The `snap` parameter of `p`, for a command that has the words `words`: `None` for no snapping.
pub(super) fn parse(p: &Value, cmd: &str, words: Words) -> Result<Option<Ask>> {
    match p.get("snap") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
        Some(Value::Bool(true)) => Ok(Some(Ask::at_zoom(DEFAULT_ZOOM))),
        Some(Value::Object(map)) => {
            let takes = words.names();
            if let Some(unknown) = map.keys().find(|k| !takes.contains(&k.as_str())) {
                let shown = crate::error::shown(unknown);
                return Err(if ALL_WORDS.contains(&unknown.as_str()) {
                    bad(cmd, format!("`snap.{shown}` is not a word of `{cmd}`, which takes {}", words.listed()))
                } else {
                    bad(cmd, format!("`snap` has an unknown key `{shown}`, and `{cmd}` takes {}", words.listed()))
                });
            }
            let lock = lock_word(map.get("lock"), cmd, "snap.lock")?;
            let zoom = match map.get("zoom") {
                None | Some(Value::Null) => DEFAULT_ZOOM,
                Some(v) => v
                    .as_f64()
                    .filter(|z| crate::snap::takes_zoom(*z))
                    .ok_or_else(|| bad(cmd, format!("`snap.zoom` must be a number above 0, {ZOOM_WORDS}")))?,
            };
            let view = view_word(map.get("view"), cmd, "snap.view")?;
            let keep = keep_word(map.get("keep"), cmd, "snap.keep")?;
            let radius = match map.get("radius") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    Some(v.as_f64().filter(|r| r.is_finite() && *r >= 0.0).ok_or_else(|| bad(cmd, "`snap.radius` must be a number, 0 or more"))?)
                }
            };
            let nudge = match map.get("nudge") {
                None | Some(Value::Null) => None,
                Some(Value::Object(n)) => {
                    if let Some(unknown) = n.keys().find(|k| k.as_str() != "tick") {
                        return Err(bad(cmd, format!("`snap.nudge` has an unknown key `{}`; it takes tick", crate::error::shown(unknown))));
                    }
                    Some(tick_word(n.get("tick"), cmd, "snap.nudge.tick")?)
                }
                Some(_) => return Err(bad(cmd, "`snap.nudge` must be an object {tick}")),
            };
            Ok(Some(Ask { zoom, lock, view, keep, ratio: None, radius, nudge, pressed: None, frame: None }))
        }
        Some(_) => Err(bad(cmd, "`snap` must be true, false, or an object {zoom}")),
    }
}

/// How far a press of an arrow key goes, `snapped` being what the snap said of it (`None` when the call asked for no snap): to the next place it can snap to, however
/// far, and with nothing ahead (or with the Snap switch off, when the snap says nothing) one tick of the press. `shape.move` and `snap.at` both take it from here.
pub(super) fn nudged(snapped: Option<&Snapped>, press: Nudge) -> (f64, f64) {
    let by = |d: f64| if press.axis == Axis::X { (d, 0.0) } else { (0.0, d) };
    let tick = if press.forward { press.tick } else { -press.tick };
    match snapped {
        Some(sn) if sn.dx == 0.0 && sn.dy == 0.0 && sn.hits.is_empty() => by(tick),
        Some(sn) => (sn.dx, sn.dy),
        None => by(tick),
    }
}

/// The read of `page` that a command snaps on: **the one the session kept** (`Session::kept_snap_page`: the window's reader made its index of the shapes and its targets there,
/// so the command builds none), else a read of its own, which is what a command did before the cache and costs what it did (a cold page). `None` for a page that is not there.
///
/// **Ask it with the document the command has before its edit.** The read has an index of the shapes and a set of targets that are made once, lazily, by whoever asks first, and the
/// session keeps the read for its revision past a command that fails (a batch whose next step does puts the revision back): what was made of a draft of the document that was
/// thrown away would stay in it. A command that snaps inside its own edit, with the draft, takes [`read_before_edit`] instead.
pub(super) fn read_for(s: &Session, page: u32) -> Option<Arc<SnapPage>> {
    s.kept_snap_page(page).or_else(|| s.document().and_then(|doc| SnapPage::read(doc, page)).map(Arc::new))
}

/// A read of a page whose index of the shapes and set of targets were made of the document as it is before the edit that will ask it ([`read_before_edit`]), so that the edit's
/// draft can ask it and make nothing in it: the type is what a command that snaps inside its edit takes.
pub(super) struct Settled(Arc<SnapPage>);

impl Settled {
    pub(super) fn page(&self) -> &SnapPage {
        &self.0
    }
}

/// [`read_for`] for a command that snaps **inside its own edit**, with the draft of the document (a drop snaps the shape it has just made, which the draft holds and the document does
/// not): the page is made ready here, **before the edit, of the document as it is**, for the one query the drop makes (a box, put down, no frame: [`Kind::Box`], moving nothing the
/// index holds), as the window's reader makes it ready for a drop's preview. The draft then asks a read that has what it needs made, and makes nothing in it (
/// of the re-review: the index made of the draft held the new shape in a read the session kept past a batch that failed, and the next snapped move lined up with a shape that was not
/// there). Made ready, a page costs what the command's own query would have made of it, and nothing where the settings snap to no shape.
pub(super) fn read_before_edit(s: &Session, page: u32) -> Option<Settled> {
    let read = read_for(s, page)?;
    if let Some(doc) = s.document() {
        make_ready(doc, &read, &SnapSettings::of(doc), &[], Kind::Box { framed: false });
    }
    Some(Settled(read))
}

/// The snap of `subject` on the page read `read` (`None`: no such page, and the snap says nothing) by the document's settings, with the places of the shapes that move that are
/// pulled too (their connection points, in paper inches; none for a place). A resize's snap is `snap::snap_resize`.
pub(super) fn run_with(doc: &Document, read: Option<&SnapPage>, subject: Subject, using: Use, moving: &[u32], own_points: &[P], ask: Ask) -> Snapped {
    let Some(snap_page) = read else { return Snapped::default() };
    run_on(doc, snap_page, subject, using, moving, own_points, ask)
}

/// [`run_with`] on a page already read, so that a command with several things to snap reads the page and builds the index of its shapes once.
pub(super) fn run_on(doc: &Document, snap_page: &SnapPage, subject: Subject, using: Use, moving: &[u32], own_points: &[P], ask: Ask) -> Snapped {
    let settings = SnapSettings::of(doc);
    snap_ext(doc, snap_page, &settings, &Query { subject, using, moving, zoom: ask.zoom, own_points }, ask.extras())
}

/// The snap of a turn of the shape `id` to the angle `raw` (radians, what the pointer gives) with the pointer `radius` inches from the pin, on the page read `read`.
pub(super) fn run_turn(doc: &Document, read: Option<&SnapPage>, page: u32, id: u32, raw: f64, radius: f64, ask: Ask) -> Snapped {
    let Some(snap_page) = read else { return Snapped::default() };
    // The pin the shape turns about, where the angle mark is drawn (a shape's pin is not the middle of its box: `snap::pin_of`).
    let pivot = pin_of(doc, page, id).unwrap_or((0.0, 0.0));
    snap_turn(doc, snap_page, &SnapSettings::of(doc), &[id], Turn { pivot, angle: raw, radius, zoom: ask.zoom })
}

/// The ruler's tick on `axis` at `zoom` on the page read `read`, in paper inches: how far an arrow key moves a shape when nothing is ahead. A page whose
/// ruler has none gives the grid's pitch, and a page with neither (or none) an eighth of an inch.
pub(super) fn ruler_tick(read: Option<&SnapPage>, axis: Axis, zoom: f64) -> f64 {
    let Some(snap_page) = read else { return 0.125 };
    let k = snap_page.k;
    snap_page.grid.ruler(axis, k, zoom).or_else(|| snap_page.grid.grid(axis, k, zoom)).map_or(0.125, |m| m.pitch)
}

fn number(n: f64) -> Value {
    // A millionth of an inch: arithmetic noise does not reach a caller.
    json!((n * 1.0e6).round() / 1.0e6)
}

/// The marks of a snap, told: each is `{mark: "line" | "gap" | "place", ...}` with `hit` (it was snapped to, or only holds where the box lands).
fn marks_json(marks: &[SnapMark]) -> Vec<Value> {
    let at = |p: P| json!([number(p.0), number(p.1)]);
    marks
        .iter()
        .map(|m| match m {
            SnapMark::Line { axis, normal, at: line, from, to, targets, shape, count, hit } => json!({
                "mark": "line", "axis": normal.map_or(json!(axis.name()), |_| Value::Null), "normal": normal.map(|t| number(t.to_degrees())), "at": number(*line),
                "from": at(*from), "to": at(*to), "targets": targets.iter().map(|t| t.name()).collect::<Vec<_>>(), "shape": shape, "count": count, "hit": hit,
            }),
            SnapMark::Gap { axis, normal, from, to, value, label, chain, own, hit } => json!({
                "mark": "gap", "axis": normal.map_or(json!(axis.name()), |_| Value::Null), "normal": normal.map(|t| number(t.to_degrees())), "from": at(*from), "to": at(*to),
                "value": number(*value), "label": label, "chain": chain, "own": own, "hit": hit,
            }),
            SnapMark::Place { at: place, target, shape, point, hit } => json!({
                "mark": "place", "at": at(*place), "target": target.name(), "shape": shape, "point": point, "hit": hit,
            }),
            SnapMark::Angle { pivot, degrees, label, matched, step, hit } => json!({
                "mark": "angle", "pivot": at(*pivot), "degrees": number(*degrees), "label": label, "matched": matched, "step": step.map(number), "hit": hit,
            }),
            SnapMark::Size { value, label, side, matched, count, hit } => json!({
                "mark": "size", "value": number(*value), "label": label, "side": [at(side.0), at(side.1)],
                "matched": matched.map(|(id, l)| json!({"shape": id, "side": [at(l.0), at(l.1)]})), "count": count, "hit": hit,
            }),
        })
        .collect()
}

/// A snap, told: `{dx, dy, hits: [...], marks: [...], glue, truncated, examined}`.
pub(super) fn snapped_json(s: &Snapped) -> Value {
    let hits: Vec<Value> = s
        .hits
        .iter()
        .map(|h| {
            let mut m = Map::new();
            m.insert("target".into(), json!(h.target.name()));
            if let Some(axis) = h.axis {
                m.insert("axis".into(), json!(axis.name()));
                m.insert("line".into(), number(h.line));
            }
            // A line along no axis (a side of a turned shape's own, a slanted guide): told by its normal in degrees and its offset along it.
            if let Some(normal) = h.normal {
                m.insert("normal".into(), number(normal.to_degrees()));
                m.insert("line".into(), number(h.line));
            }
            if let Some((x, y)) = h.at {
                m.insert("at".into(), json!([number(x), number(y)]));
            }
            m.insert("anchor".into(), json!(h.anchor.name()));
            if let Some(shape) = h.shape {
                m.insert("shape".into(), json!(shape));
            }
            if let Some(point) = h.point {
                m.insert("point".into(), json!(point));
            }
            if let Some((a, b)) = h.span {
                m.insert("span".into(), json!([number(a), number(b)]));
            }
            if let Some(gap) = h.gap {
                m.insert("gap".into(), number(gap));
            }
            Value::Object(m)
        })
        .collect();
    let glue = s.glue.map_or(Value::Null, |g| match g {
        Glue::ConnectionPoint { shape, point } => json!({ "kind": "connection_point", "shape": shape, "point": point }),
        Glue::Vertex { shape } => json!({ "kind": "vertex", "shape": shape }),
        Glue::Handle { shape } => json!({ "kind": "handle", "shape": shape }),
        Glue::Outline { shape } => json!({ "kind": "outline", "shape": shape }),
    });
    // The guides a box is held to, with the alignment cell each glues.
    let holds: Vec<Value> = s
        .holds
        .iter()
        .map(|h| json!({ "guide": h.guide, "axis": h.axis.name(), "anchor": h.anchor.name(), "cell": h.cell().map_or(Value::Null, |c| json!(c)) }))
        .collect();
    json!({ "dx": number(s.dx), "dy": number(s.dy), "hits": hits, "marks": marks_json(&s.marks), "angle": s.turned.map(|a| number(a.to_degrees())), "landed": s.landed.map(|l| json!([number(l.x0), number(l.y0), number(l.x1), number(l.y1)])), "frame": s.frame.map(|f| json!({"centre": [number(f.centre.0), number(f.centre.1)], "width": number(f.width), "height": number(f.height), "angle": number(f.angle.to_degrees())})), "glue": glue, "holds": holds, "truncated": s.truncated, "corner_boxes": s.corner_boxes, "examined": s.examined })
}
