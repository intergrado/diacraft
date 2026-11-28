//! Doubling and halving connection points in the window (a DiaCraft extra beyond the reference app: owner, 2026-11-26): **Ctrl+Alt+K doubles** the connection points of the edge under the
//! pointer, or of every edge of the shapes selected when the pointer is on none, and **Ctrl+Alt+Shift+K halves** them (Command+Option+K and Shift+Command+Option+K on a Mac). The Connection Points
//! submenu of a shape's right-click menu acts on the edge at the click, the menu of Home, Tools, Connection Point on every edge, and each row is greyed when it would do nothing
//! (design §10).
//!
//! **The window decides nothing from geometry of its own.** It reads what the engine says of the shapes selected, the answer of `shape.connection_edges` (`draw::conn_points::query_edges`, read from the
//! `&Session` as `operations.rs` reads `check_shape_ops`: the rows are drawn from a `&self`), keeps it for the drawing's generation and revision, the page, the shapes selected and the minimum distance
//! ([`PointsView`]), and **asks again only when one of them changes, never for a frame**; and it acts through the engine's `shape.connection_points`, with `id` and `edge` for the edge it picked or `ids` for
//! every edge. What it does itself is pick the edge nearest the pointer from the `path` the engine gave (at most 32 places along each edge), within five screen points as inches at the zoom
//! (`HIT_REACH`), the same pick the engine makes for a program's `at` and `reach`.
//!
//! **Who is the target** (`window.connectionPointsDo`'s `target`): the keys' is **the pointer** (the edge under it, else every edge of the shapes selected), the right-click rows' **the menu**
//! (the edge at the click that opened it, else every edge), the ribbon's **all** (every edge, whatever is under the pointer).
//!
//! **Enabled**: Double is on when the target holds an edge with room under the minimum distance (`adds` above 0) whose shape the doubling fits (the cap of 256 points), and for every edge when
//! some shape selected can double; Halve is on when the target edge has a point a doubling made (`newest`), and for every edge when some shape selected has one. **Past the window's caps** (more than
//! [`MAX_WINDOW_SHAPES`] shapes selected, more than [`MAX_WINDOW_EDGES`] edges in the answer, a shape with more edges than the answer lists, or a question the engine refused) **both are left on** and the
//! engine's words say what it did. A key that is off does nothing, **and still takes its press**: egui lets an extra Alt or Shift match a shortcut (`Ctrl+Alt+Shift+K` was Small Caps too, through
//! `Ctrl+Shift+K`), and a press that typed a character (AltGr+K is ł on a Czech, Hungarian or Slovak layout) is taken out of the frame before any shortcut runs, and is the text's (`shortcuts::let_typed_text_win`).
//!
//! **The minimum distance** is a window setting for now, in paper inches (`window.connectionMinGap`, File, Options, Advanced), behind [`DiacraftApp::connection_min_gap`]: when the settings store lands
//! it becomes a field of the interface preferences and these two functions read and write that.

use std::cell::RefCell;
use std::sync::Arc;

use diacraft_engine::doc::Document;
use diacraft_engine::draw::conn_points::{
    EdgeInfo, EdgeKind, MAX_MIN_GAP, MAX_POINTS_PER_SHAPE, MAX_SHAPES_PER_CALL, MIN_MIN_GAP, ShapeEdges, fits, query_edges,
};
use diacraft_engine::{EngineError, Result, shown};
use diacraft_units::Unit;
use serde_json::{Map, Value, json};

use super::HIT_REACH;
use super::geom::{P, distance_to_segment};
use crate::app::DiacraftApp;
use crate::format::SELECTION_ELSEWHERE;
use crate::measure::length_text;

/// The window command the keys, the right-click rows and the ribbon's rows run.
pub(crate) const COMMAND: &str = "window.connectionPointsDo";

/// The most shapes the window reads edges of, and the most edges in one answer it keeps (design §9): past either the rows stay on and the engine's words say what happened.
pub const MAX_WINDOW_SHAPES: usize = 256;
pub const MAX_WINDOW_EDGES: usize = 16_384;

/// Two edges as near as this are as near as each other: the earlier wins, as the engine's pick has it.
const TIE: f64 = 1.0e-12;

/// The words of a halving of an edge that has no point a doubling made (the engine's own).
const NOTHING_TO_HALVE: &str = "nothing to halve: the edge has no points a doubling made";

/// What is done to the points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Double,
    Halve,
}

impl Act {
    pub(crate) fn from_word(word: &str) -> Option<Act> {
        match word {
            "double" => Some(Act::Double),
            "halve" => Some(Act::Halve),
            _ => None,
        }
    }

    pub(crate) const fn word(self) -> &'static str {
        match self {
            Act::Double => "double",
            Act::Halve => "halve",
        }
    }

    /// The past tense, as the notice says it.
    const fn past(self) -> &'static str {
        match self {
            Act::Double => "doubled",
            Act::Halve => "halved",
        }
    }
}

/// Where the edge to change is looked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aim {
    /// Under the pointer, else every edge: the keys.
    Pointer,
    /// Every edge of the shapes selected: the ribbon.
    All,
    /// At the right click that opened the menu, else every edge: the right-click rows.
    Menu,
}

impl Aim {
    fn from_word(word: &str) -> Option<Aim> {
        match word {
            "pointer" => Some(Aim::Pointer),
            "all" => Some(Aim::All),
            "menu" => Some(Aim::Menu),
            _ => None,
        }
    }
}

/// The edges a call changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    /// Every edge of the shapes selected.
    Every,
    /// One edge of one shape.
    Edge { shape: u32, edge: u32 },
    /// The edge the engine finds nearest this place of the page within this reach, else every edge: for shapes the window did not read.
    Near { at: P, reach: f64 },
}

/// What the engine said of the shapes selected, as the window keeps it.
#[derive(Debug)]
pub(crate) enum Facts {
    /// Not read: past the window's caps (more shapes or edges than it keeps), or the engine refused the question. The rows stay on, and the engine's words say what happened.
    Unread,
    /// The shapes, as `shape.connection_edges` tells them.
    Read(Vec<ShapeEdges>),
}

/// What a read of the shapes selected is for: the drawing's generation, its revision, the page and the minimum distance (as its bits); the shapes are kept beside it.
type Key = (u64, u64, u32, u64);

/// What was read: for which key and shapes, and what the engine said.
struct Held {
    key: Key,
    ids: Vec<u32>,
    facts: Arc<Facts>,
}

/// The answer the engine gave for one drawing, revision, page, selection and minimum distance.
pub struct PointsView {
    held: RefCell<Option<Held>>,
    /// What a selection past the window's caps is told as: never read, so one answer serves them all.
    unread: Arc<Facts>,
}

impl Default for PointsView {
    fn default() -> Self {
        PointsView { held: RefCell::new(None), unread: Arc::new(Facts::Unread) }
    }
}

impl PointsView {
    /// The facts kept for `key` and the shapes `ids`, or what `read` says (once: it is kept). **A frame that finds them kept copies nothing**: the shapes are compared in place.
    fn facts(&self, key: Key, ids: &[u32], read: impl FnOnce() -> Facts) -> Arc<Facts> {
        if let Ok(held) = self.held.try_borrow()
            && let Some(kept) = held.as_ref()
            && kept.key == key
            && kept.ids == ids
        {
            return Arc::clone(&kept.facts);
        }
        let facts = Arc::new(read());
        if let Ok(mut held) = self.held.try_borrow_mut() {
            *held = Some(Held { key, ids: ids.to_vec(), facts: Arc::clone(&facts) });
        }
        facts
    }
}

/// What the engine says of the shapes `ids` of `page` under `min_gap`, or why the window does not read them.
fn read_facts(doc: &Document, page: u32, ids: &[u32], min_gap: f64) -> Facts {
    crate::work::count(|w| w.connection_edge_queries += 1);
    match query_edges(doc, page, ids, min_gap) {
        Ok(answer) => {
            let edges: usize = answer.shapes.iter().map(|s| s.edges.len()).sum();
            if edges > MAX_WINDOW_EDGES { Facts::Unread } else { Facts::Read(answer.shapes) }
        }
        Err(_) => Facts::Unread,
    }
}

/// A length of paper in millimetres, as the words of a refusal say it.
fn mm(inches: f64) -> String {
    length_text(inches, Unit::Millimetre)
}

/// The range of the minimum distance in words, in both units: "from 0.01 in to 2 in (0.254 mm to 50.8 mm)".
pub(crate) fn min_gap_range() -> String {
    format!("from {} to {} ({} to {})", length_text(MIN_MIN_GAP, Unit::Inch), length_text(MAX_MIN_GAP, Unit::Inch), mm(MIN_MIN_GAP), mm(MAX_MIN_GAP))
}

fn no_room(gap: f64, min_gap: f64) -> String {
    if gap.is_finite() && gap > 0.0 {
        format!("no edge has room: a doubling would put points {} apart, under the minimum of {} (Options)", mm(gap), mm(min_gap))
    } else {
        format!("no edge has room: a doubling would put points under the minimum of {} apart (Options)", mm(min_gap))
    }
}

fn past_cap(id: u32, points: u32, adds: u32) -> String {
    format!(
        "shape {id} would have {} connection points, past the {MAX_POINTS_PER_SHAPE} a connector looks at: it would be left as it was",
        points.saturating_add(adds)
    )
}

/// Whether a doubling of `edge` of `shape` can run: it has room under the minimum distance and the shape takes the points.
fn can_double_edge(shape: &ShapeEdges, edge: &EdgeInfo, min_gap: f64) -> std::result::Result<(), String> {
    if edge.adds == 0 {
        return Err(no_room(edge.gap, min_gap));
    }
    let points = u32::try_from(shape.points).unwrap_or(u32::MAX);
    if fits(points, edge.adds) { Ok(()) } else { Err(past_cap(shape.id, points, edge.adds)) }
}

/// Whether a doubling of every edge of `shapes` can run: some shape that can be read can take it. The reason when none can.
fn can_double_every(shapes: &[&ShapeEdges], min_gap: f64) -> std::result::Result<(), String> {
    if shapes.iter().any(|s| s.can_double) {
        return Ok(());
    }
    // Why: the smallest gap an edge without room would have made, else the first shape the cap stops.
    let gap =
        shapes.iter().flat_map(|s| s.edges.iter()).filter(|e| e.adds == 0).map(|e| e.gap).filter(|g| g.is_finite()).fold(f64::INFINITY, f64::min);
    if gap.is_finite() {
        return Err(no_room(gap, min_gap));
    }
    for shape in shapes {
        let points = u32::try_from(shape.points).unwrap_or(u32::MAX);
        if let Some(edge) = shape.edges.iter().find(|e| e.adds > 0 && !fits(points, e.adds)) {
            return Err(past_cap(shape.id, points, edge.adds));
        }
    }
    Err("no shape selected has an edge to put more connection points on".to_owned())
}

/// Whether `act` can run on `target` as the engine said of the shapes, and if not why. A target that is not in what the engine said, and every kind of answer that is not read, is left on.
fn verdict_of(facts: &Facts, act: Act, target: Target, min_gap: f64) -> std::result::Result<(), String> {
    let Facts::Read(shapes) = facts else { return Ok(()) };
    match target {
        Target::Edge { shape, edge } => {
            let found = shapes.iter().find(|s| s.id == shape).and_then(|s| s.edges.iter().find(|e| e.edge == edge).map(|e| (s, e)));
            let Some((shape, edge)) = found else { return Ok(()) };
            match act {
                Act::Double => can_double_edge(shape, edge, min_gap),
                Act::Halve if edge.newest.is_some() => Ok(()),
                Act::Halve => Err(NOTHING_TO_HALVE.to_owned()),
            }
        }
        Target::Every | Target::Near { .. } => {
            let usable: Vec<&ShapeEdges> = shapes.iter().filter(|s| s.refused.is_none()).collect();
            if usable.is_empty() {
                return Err(shapes
                    .iter()
                    .find_map(|s| s.refused.clone())
                    .unwrap_or_else(|| "no shape selected has an outline to put connection points on".to_owned()));
            }
            // A shape with more edges than the answer lists is not known to be unable: the engine's words say.
            if usable.iter().any(|s| s.truncated) {
                return Ok(());
            }
            match act {
                Act::Double => can_double_every(&usable, min_gap),
                Act::Halve if usable.iter().any(|s| s.can_halve) => Ok(()),
                Act::Halve => Err(NOTHING_TO_HALVE.to_owned()),
            }
        }
    }
}

/// The distance from `at` to the edge's path, if it is within `reach` (the box of the path is tried first).
fn edge_distance(edge: &EdgeInfo, at: P, reach: f64) -> Option<f64> {
    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in &edge.path {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    if !(at.0 >= lo.0 - reach && at.0 <= hi.0 + reach && at.1 >= lo.1 - reach && at.1 <= hi.1 + reach) {
        return None;
    }
    let mut best = f64::INFINITY;
    for pair in edge.path.windows(2) {
        if let [a, b] = pair {
            best = best.min(distance_to_segment(at, *a, *b));
        }
    }
    // A closed edge's places are joined last to first.
    if edge.kind == EdgeKind::Closed
        && let (Some(first), Some(last)) = (edge.path.first(), edge.path.last())
    {
        best = best.min(distance_to_segment(at, *last, *first));
    }
    if let [only] = edge.path.as_slice() {
        best = best.min(super::geom::distance(at, *only));
    }
    (best <= reach).then_some(best)
}

/// The edge of the shapes nearest `at` within `reach` (inches of the page), as (the shape, the edge): ties go to the earlier, as the engine's pick has it.
pub(crate) fn nearest_edge(shapes: &[ShapeEdges], at: P, reach: f64) -> Option<(u32, u32)> {
    if !(at.0.is_finite() && at.1.is_finite() && reach.is_finite() && reach > 0.0) {
        return None;
    }
    let mut best: Option<(f64, u32, u32)> = None;
    for shape in shapes.iter().filter(|s| s.refused.is_none()) {
        for edge in &shape.edges {
            if let Some(d) = edge_distance(edge, at, reach)
                && best.is_none_or(|(b, ..)| d < b - TIE)
            {
                best = Some((d, shape.id, edge.edge));
            }
        }
    }
    best.map(|(_, shape, edge)| (shape, edge))
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: COMMAND.to_owned(), msg: msg.into() }
}

/// `action` and `target` of a call of `window.connectionPointsDo`.
fn request_of(p: &Map<String, Value>) -> Result<(Act, Aim)> {
    let word = match p.get("action") {
        None | Some(Value::Null) => return Err(bad("missing `action`: double or halve")),
        Some(Value::String(word)) => word.as_str(),
        Some(_) => return Err(bad("`action` must be text: double or halve")),
    };
    let act = match Act::from_word(word) {
        Some(act) => act,
        None if word == "set_count" => return Err(bad("set_count was dropped: double or halve")),
        None => return Err(bad(format!("`action` must be `double` or `halve`, not `{}`", shown(word, 32)))),
    };
    let aim = match p.get("target") {
        None | Some(Value::Null) => Aim::Pointer,
        Some(Value::String(word)) => Aim::from_word(word).ok_or_else(|| {
            bad(format!(
                "`target` must be `pointer` (the edge under the pointer), `all` (every edge) or `menu` (the edge at the right click), not `{}`",
                shown(word, 32)
            ))
        })?,
        Some(_) => return Err(bad("`target` must be text: pointer, all or menu")),
    };
    Ok((act, aim))
}

impl DiacraftApp {
    /// The least distance between the points a doubling makes, in paper inches (the setting of File, Options, Advanced). **The one place the window reads it**: a window setting for now, and a field of
    /// the interface preferences when the settings store lands.
    pub(crate) fn connection_min_gap(&self) -> f64 {
        self.ui.connection_min_gap
    }

    /// Sets the least distance between doubled points, in paper inches, from [`MIN_MIN_GAP`] to [`MAX_MIN_GAP`] (the other place that touches the setting).
    pub(crate) fn set_connection_min_gap(&mut self, inches: f64) -> Result<()> {
        if !(inches.is_finite() && (MIN_MIN_GAP..=MAX_MIN_GAP).contains(&inches)) {
            return Err(EngineError::BadParams {
                cmd: "window.connectionMinGap".to_owned(),
                msg: format!("`inches` must be a number of paper inches {}", min_gap_range()),
            });
        }
        self.ui.connection_min_gap = inches;
        Ok(())
    }

    /// `window.connectionMinGap`.
    pub(crate) fn connection_min_gap_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        match p.get("inches").and_then(Value::as_f64) {
            Some(inches) => self.set_connection_min_gap(inches),
            None => Err(EngineError::BadParams {
                cmd: "window.connectionMinGap".to_owned(),
                msg: format!("`inches` must be a number of paper inches {}", min_gap_range()),
            }),
        }
    }

    /// What the engine says of the shapes selected: the facts kept for this drawing, revision, page, selection and minimum, or one read now (once). `Err` says in words why there is nothing to ask about.
    fn connection_facts(&self) -> std::result::Result<Arc<Facts>, String> {
        let Some(doc) = self.session.document() else { return Err("no document open".to_owned()) };
        let ids = self.ui.edit.selection();
        let Some(page) = self.ui.edit.selection_page().filter(|_| !ids.is_empty()) else { return Err("nothing is selected".to_owned()) };
        // The shapes selected are of the page the selection was made on, and a run is refused when another page is on view (`selection_target`).
        if self.ui.page.is_some_and(|shown| shown != page) {
            return Err(SELECTION_ELSEWHERE.to_owned());
        }
        // More shapes than the window reads are not read, and are not kept either: one answer serves every such selection.
        if ids.len() > MAX_WINDOW_SHAPES {
            return Ok(Arc::clone(&self.ui.points.unread));
        }
        let min_gap = self.connection_min_gap();
        let key = (self.ui.generation, self.session.revision(), page, min_gap.to_bits());
        Ok(self.ui.points.facts(key, ids, || read_facts(doc, page, ids, min_gap)))
    }

    /// How far the pointer reaches to take an edge, in inches of the page at the zoom: five screen points (`HIT_REACH`, as a shape is hit).
    fn connection_reach(&self) -> Option<f64> {
        let view = self.ui.page.and_then(|page| self.ui.views.get(&page))?;
        let reach = f64::from(HIT_REACH) / view.scale();
        (reach.is_finite() && reach > 0.0).then_some(reach)
    }

    /// The edges `aim` means, from what the engine said: the edge nearest the place (the pointer's, or the right click's) within the pointer's reach, else every edge.
    fn connection_target(&self, facts: &Facts, aim: Aim) -> Target {
        let place = match aim {
            Aim::All => None,
            Aim::Pointer => self.ui.edit.hover,
            Aim::Menu => self.ui.edit.context,
        };
        let (Some(at), Some(reach)) = (place, self.connection_reach()) else { return Target::Every };
        match facts {
            Facts::Read(shapes) => nearest_edge(shapes, at, reach).map_or(Target::Every, |(shape, edge)| Target::Edge { shape, edge }),
            Facts::Unread => Target::Near { at, reach },
        }
    }

    /// The edges `act` would change for `aim`, or why it cannot run now.
    fn connection_plan(&self, act: Act, aim: Aim) -> std::result::Result<Target, String> {
        let facts = self.connection_facts()?;
        let target = self.connection_target(&facts, aim);
        verdict_of(&facts, act, target, self.connection_min_gap())?;
        Ok(target)
    }

    /// Whether `act` can run for `aim` now, and if not why.
    pub(crate) fn connection_verdict(&self, act: Act, aim: Aim) -> std::result::Result<(), String> {
        self.connection_plan(act, aim).map(|_| ())
    }

    /// Whether `window.connectionPointsDo` can run at all: a doubling or a halving of every edge can. This is what the command's enabled state is for a caller that names no action.
    pub(crate) fn connection_verdict_any(&self) -> std::result::Result<(), String> {
        self.connection_verdict(Act::Double, Aim::All).or_else(|first| self.connection_verdict(Act::Halve, Aim::All).map_err(|_| first))
    }

    /// Whether a control or a key that runs `window.connectionPointsDo` with `args` (the text of a JSON object) can run now, and if not why.
    pub(crate) fn connection_verdict_for(&self, args: &str) -> std::result::Result<(), String> {
        let parsed = serde_json::from_str::<Value>(args).ok();
        let word = |key: &str| parsed.as_ref().and_then(|v| v.get(key)).and_then(Value::as_str);
        let Some(act) = word("action").and_then(Act::from_word) else { return self.connection_verdict_any() };
        self.connection_verdict(act, word("target").and_then(Aim::from_word).unwrap_or(Aim::Pointer))
    }

    /// The edge the window picks at a place of the page, in inches: (the shape, the edge) nearest it within the pointer's reach at the zoom, `None` for a place near none. For tests and agents.
    pub fn connection_edge_at(&self, at: (f64, f64)) -> Option<(u32, u32)> {
        let reach = self.connection_reach()?;
        match &*self.connection_facts().ok()? {
            Facts::Read(shapes) => nearest_edge(shapes, at, reach),
            Facts::Unread => None,
        }
    }

    /// `window.connectionPointsDo`: the engine's `shape.connection_points` on the edge `aim` means, or on every edge of the shapes selected.
    pub(crate) fn connection_points_window_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        let (act, aim) = request_of(p)?;
        let (page, ids) = self.selection_target(COMMAND)?;
        let target = self.connection_plan(act, aim).map_err(|reason| EngineError::Disabled { cmd: COMMAND.to_owned(), reason })?;
        let mut params = json!({"action": act.word(), "page": page, "min_gap": self.connection_min_gap()});
        let mut set = |key: &str, value: Value| {
            if let Some(map) = params.as_object_mut() {
                map.insert(key.to_owned(), value);
            }
        };
        match target {
            Target::Edge { shape, edge } => {
                set("id", json!(shape));
                set("edge", json!(edge));
            }
            Target::Every | Target::Near { .. } => {
                if ids.len() > MAX_SHAPES_PER_CALL {
                    return Err(bad(format!("at most {MAX_SHAPES_PER_CALL} shapes may be changed in one call: {} are selected", ids.len())));
                }
                set("ids", json!(ids));
                if let Target::Near { at, reach } = target {
                    set("at", json!({"x": at.0, "y": at.1}));
                    set("reach", json!(reach));
                }
            }
        }
        let answer = self.execute("shape.connection_points", &params)?;
        self.ui.points_note = Some(notice_of(act, &answer));
        Ok(())
    }
}

/// The words of the status bar for what a call did: "Connection points doubled on 1 edge of shape 3 (1 to 3)", the points of the shape in all when it was several edges, and a shape the engine left as it was.
/// **No arrow:** the window's bundled face has no glyph for U+2192, and a glyph no face has is a panic of egui's text, so the numbers are joined by a word (the design's example has the arrow).
fn notice_of(act: Act, answer: &Value) -> String {
    let shapes = answer["shapes"].as_array().map_or(&[][..], Vec::as_slice);
    let count = |v: &Value| v.as_array().map_or(0, Vec::len);
    let changed: Vec<&Value> = shapes.iter().filter(|s| count(&s["edges"]) > 0).collect();
    let edges: usize = changed.iter().map(|s| count(&s["edges"])).sum();
    let plural = |n: usize, one: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {one}s") };
    let mut text = match changed.as_slice() {
        [] => format!("No connection points were {}", act.past()),
        [only] => {
            let id = only["id"].as_u64().unwrap_or(0);
            match only["edges"].as_array().map(Vec::as_slice) {
                Some([edge]) => format!(
                    "Connection points {} on 1 edge of shape {id} ({} to {})",
                    act.past(),
                    edge["before"].as_u64().unwrap_or(0),
                    edge["after"].as_u64().unwrap_or(0)
                ),
                _ => format!(
                    "Connection points {} on {} of shape {id} ({} to {} points in all)",
                    act.past(),
                    plural(edges, "edge"),
                    only["points_before"].as_u64().unwrap_or(0),
                    only["points_after"].as_u64().unwrap_or(0)
                ),
            }
        }
        many => {
            let sum = |key: &str| many.iter().map(|s| s[key].as_u64().unwrap_or(0)).sum::<u64>();
            format!(
                "Connection points {} on {} of {} ({} to {} points in all)",
                act.past(),
                plural(edges, "edge"),
                plural(many.len(), "shape"),
                sum("points_before"),
                sum("points_after")
            )
        }
    };
    // What the engine left as it was: a shape that could not be changed, in its words, and the edges that had no room under the minimum distance.
    let left: Vec<&str> = shapes.iter().filter_map(|s| s["skipped"].as_str()).collect();
    if let Some(first) = left.first() {
        text.push_str(&format!(". {first}"));
        if left.len() > 1 {
            text.push_str(&format!(" ({} more shapes were left as they were)", left.len() - 1));
        }
    }
    let no_room: usize = shapes.iter().map(|s| count(&s["skipped_edges"])).sum();
    if no_room > 0 {
        text.push_str(&format!(". {} had no room under the minimum distance (Options)", plural(no_room, "edge")));
    }
    text
}

#[cfg(test)]
mod tests {
    use diacraft_engine::draw::conn_points::DEFAULT_MIN_GAP;

    use super::*;

    fn edge(number: u32, kind: EdgeKind, path: &[(f64, f64)], adds: u32, newest: Option<u8>) -> EdgeInfo {
        EdgeInfo {
            edge: number,
            kind,
            section: Some(0),
            rows: (0, 1),
            steps: 1,
            length: 1.0,
            from: path.first().copied().unwrap_or((0.0, 0.0)),
            to: path.last().copied().unwrap_or((0.0, 0.0)),
            path: path.to_vec(),
            points: Vec::new(),
            added: Vec::new(),
            newest,
            gap: 0.5,
            adds,
        }
    }

    fn shape(id: u32, edges: Vec<EdgeInfo>) -> ShapeEdges {
        let can_double = edges.iter().any(|e| e.adds > 0);
        let can_halve = edges.iter().any(|e| e.newest.is_some());
        ShapeEdges { id, refused: None, edges, other: Vec::new(), points: 4, can_double, can_halve, truncated: false }
    }

    #[test]
    fn the_nearest_edge_within_reach_is_picked_and_a_tie_goes_to_the_earlier() {
        let square = shape(
            1,
            vec![
                edge(0, EdgeKind::Straight, &[(0.0, 0.0), (1.0, 0.0)], 1, None),
                edge(1, EdgeKind::Straight, &[(1.0, 0.0), (1.0, 1.0)], 1, None),
                edge(2, EdgeKind::Straight, &[(1.0, 1.0), (0.0, 1.0)], 1, None),
                edge(3, EdgeKind::Straight, &[(0.0, 1.0), (0.0, 0.0)], 1, None),
            ],
        );
        let shapes = [square];
        assert_eq!(nearest_edge(&shapes, (0.5, 0.01), 0.05), Some((1, 0)));
        assert_eq!(nearest_edge(&shapes, (1.02, 0.5), 0.05), Some((1, 1)));
        assert_eq!(nearest_edge(&shapes, (0.0, 0.0), 0.05), Some((1, 0)), "the corner is on the first edge and the last: the earlier");
        assert_eq!(nearest_edge(&shapes, (0.5, 0.5), 0.05), None, "inside, past the reach");
        for odd in [f64::NAN, f64::INFINITY] {
            assert_eq!(nearest_edge(&shapes, (odd, 0.0), 0.05), None);
            assert_eq!(nearest_edge(&shapes, (0.5, 0.0), odd), None);
        }
        assert_eq!(nearest_edge(&shapes, (0.5, 0.0), 0.0), None);
    }

    #[test]
    fn a_closed_edge_is_joined_last_to_first() {
        let ring = shape(2, vec![edge(0, EdgeKind::Closed, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], 2, None)]);
        // The side from the last place back to the first is part of the edge.
        assert_eq!(nearest_edge(&[ring], (0.5, 0.5), 0.01), Some((2, 0)));
    }

    #[test]
    fn a_doubling_needs_room_and_a_shape_that_takes_the_points_and_a_halving_a_point_a_doubling_made() {
        let mut crowded = edge(1, EdgeKind::Straight, &[(1.0, 0.0), (1.0, 1.0)], 0, Some(1));
        crowded.gap = 0.05;
        let room = shape(1, vec![edge(0, EdgeKind::Straight, &[(0.0, 0.0), (1.0, 0.0)], 2, None), crowded]);
        let facts = Facts::Read(vec![room]);
        let on = |act, target| verdict_of(&facts, act, target, DEFAULT_MIN_GAP);
        assert_eq!(on(Act::Double, Target::Edge { shape: 1, edge: 0 }), Ok(()));
        let none = on(Act::Double, Target::Edge { shape: 1, edge: 1 }).unwrap_err();
        assert_eq!(none, "no edge has room: a doubling would put points 1.27 mm apart, under the minimum of 3 mm (Options)");
        assert_eq!(on(Act::Halve, Target::Edge { shape: 1, edge: 1 }), Ok(()));
        assert_eq!(on(Act::Halve, Target::Edge { shape: 1, edge: 0 }), Err(NOTHING_TO_HALVE.to_owned()));
        assert_eq!(on(Act::Double, Target::Every), Ok(()));
        assert_eq!(on(Act::Halve, Target::Every), Ok(()));
        // An edge the answer does not hold is not refused by the window: the engine's words say.
        assert_eq!(on(Act::Double, Target::Edge { shape: 9, edge: 0 }), Ok(()));
        // A shape at the cap: its doubling would pass 256.
        let mut full = shape(3, vec![edge(0, EdgeKind::Straight, &[(0.0, 0.0), (1.0, 0.0)], 2, None)]);
        full.points = 255;
        full.can_double = false;
        let words = verdict_of(&Facts::Read(vec![full]), Act::Double, Target::Every, DEFAULT_MIN_GAP).unwrap_err();
        assert!(words.contains("shape 3 would have 257 connection points"), "{words}");
    }

    #[test]
    fn what_the_window_did_not_read_is_left_on_and_a_connector_alone_is_not() {
        let unread = Facts::Unread;
        assert_eq!(verdict_of(&unread, Act::Halve, Target::Every, 0.118), Ok(()));
        let mut refused = shape(5, Vec::new());
        refused.refused = Some("shape 5 is a line or a connector: connection points are put on two-dimensional shapes".to_owned());
        let facts = Facts::Read(vec![refused]);
        let words = verdict_of(&facts, Act::Double, Target::Every, 0.118).unwrap_err();
        assert!(words.contains("connector"), "{words}");
        let mut truncated = shape(6, vec![edge(0, EdgeKind::Straight, &[(0.0, 0.0), (1.0, 0.0)], 0, None)]);
        truncated.truncated = true;
        assert_eq!(
            verdict_of(&Facts::Read(vec![truncated]), Act::Halve, Target::Every, 0.118),
            Ok(()),
            "a shape with more edges than are listed is not known to be unable"
        );
    }

    #[test]
    fn the_notice_says_what_changed() {
        let one = json!({"shapes": [{"id": 3, "edges": [{"edge": 2, "before": 1, "after": 3}], "points_before": 4, "points_after": 6, "skipped": null, "skipped_edges": []}]});
        assert_eq!(notice_of(Act::Double, &one), "Connection points doubled on 1 edge of shape 3 (1 to 3)");
        let edges = json!({"shapes": [{"id": 3, "edges": [{"edge": 0}, {"edge": 1}], "points_before": 4, "points_after": 8, "skipped": null, "skipped_edges": []}]});
        assert_eq!(notice_of(Act::Halve, &edges), "Connection points halved on 2 edges of shape 3 (4 to 8 points in all)");
        let two = json!({"shapes": [
            {"id": 3, "edges": [{"edge": 0}], "points_before": 4, "points_after": 6, "skipped": null, "skipped_edges": []},
            {"id": 4, "edges": [{"edge": 0}, {"edge": 1}], "points_before": 4, "points_after": 8, "skipped": null, "skipped_edges": [{"edge": 2}]},
            {"id": 5, "edges": [], "points_before": 4, "points_after": 4, "skipped": "shape 5 is a group", "skipped_edges": []},
        ]});
        let said = notice_of(Act::Double, &two);
        assert_eq!(
            said,
            "Connection points doubled on 3 edges of 2 shapes (8 to 14 points in all). shape 5 is a group. 1 edge had no room under the minimum distance (Options)"
        );
        assert_eq!(notice_of(Act::Double, &json!({"shapes": []})), "No connection points were doubled");
        // What the window words itself is drawable by any face: ASCII, with no arrow.
        for text in [notice_of(Act::Double, &one), notice_of(Act::Halve, &edges), no_room(0.05, DEFAULT_MIN_GAP), past_cap(3, 255, 2)] {
            assert!(text.is_ascii(), "{text}");
        }
    }

    #[test]
    fn a_call_names_an_action_and_a_target_in_words() {
        let parse = |v: Value| request_of(v.as_object().unwrap());
        assert_eq!(parse(json!({"action": "double"})).unwrap(), (Act::Double, Aim::Pointer));
        assert_eq!(parse(json!({"action": "halve", "target": "all"})).unwrap(), (Act::Halve, Aim::All));
        assert_eq!(parse(json!({"action": "double", "target": "menu"})).unwrap(), (Act::Double, Aim::Menu));
        for (params, words) in [
            (json!({}), "missing `action`"),
            (json!({"action": 5}), "must be text"),
            (json!({"action": "set_count"}), "set_count was dropped: double or halve"),
            (json!({"action": "triple"}), "`double` or `halve`"),
            (json!({"action": "double", "target": "left"}), "`pointer`"),
            (json!({"action": "double", "target": 3}), "must be text"),
        ] {
            let refused = parse(params.clone()).unwrap_err().to_string();
            assert!(refused.contains(words), "{params}: {refused}");
        }
    }
}
