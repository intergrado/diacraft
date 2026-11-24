//! Re-Layout Page: the shapes of a page placed by the connectors between them, as one edit.
//!
//! The rules are named here once and in `docs/editing-commands.md` ("Re-Layout Page, Auto Space and page breaks"):
//!
//! * [`relayout`] lays out the shapes of the page, or of the shapes named, in the
//!   style the Re-Layout Page gallery and its Configure Layout dialog choose (a flowchart in four directions, a hierarchy in four directions and three alignments, a compact
//!   tree in eight, radial, circular), and **writes the choice into the page's own cells, as the reference application does**, so that it rides in a VSDX file and in a
//!   `.diacraft` one: `PlaceStyle` (the number of the table, [`place_code`]), `AvenueSizeX` and `AvenueSizeY` (the spacing box's "space between shapes", which the
//!   dialog's one Spacing field sets both of: **ours**, the specifications give the field no cell), `ResizePage`, and what the dialog's other fields hold (`BlockSizeX`,
//!   `BlockSizeY`, `PlaceDepth`, `PlaceFlip`, the page's `RouteStyle` and `LineRouteExt`). A cell that holds what it is given is not written.
//! * Our rule, the reference application publishes no algorithm. The layout is `diacraft-layout`'s ([`diacraft_layout`]): the shapes keep their sizes, no
//!   two boxes overlap, the spaces are at least the page's `AvenueSizeX` and `AvenueSizeY` (along the flow and across it, as the flow's direction says), and the corner of
//!   the diagram stays where it was. The graph is the shapes and the connectors **glued at both ends** to two of them (a connector glued to a shape inside a group is glued to the
//!   group). A flow follows the way the connectors point, from the begin to the end.
//! * **One edit.** The shapes move, the connectors are routed again once (at most [`MAX_CONNECTORS`], refused in words before anything is
//!   written), the settings are written, and a page that grows (Auto Size, or `ResizePage`) does. A layout that changes nothing is no edit. **What moves with what**: a shape's
//!   callouts and a container's members go with it (`callouts::followers`); a shape on a locked layer is left where it is, and listed; a shape inside a group is the group's.
//! * **Apply routing style to connectors** writes `ShapeRouteStyle` and `ConLineRouteExt` on every connector of the scope, as the Connectors menu does
//!   (`style.rs`), and sets the page's `RouteStyle` and `LineRouteExt` to the same. A connector whose route was edited by hand (`ConFixedCode` 3) is routed again whatever
//!   the box says: a route made by hand for places that are no more is not one a layout can keep.
//! * Our rule. A connector glued at both ends to two shapes of the scope is **glued again at the pair of connection points that face each other**, as a new connector between
//!   the shapes would be, so that a flow down the page leaves a shape by its bottom and comes to the next by its top (one walk of the page and one pass over its glue for all of them).
//! * A page with more than [`MAX_NODES`] shapes to lay out, [`MAX_CONNECTORS`] connectors to route, [`MAX_PAGE_SHAPES`] shapes at its top or [`MAX_GLUE_ROWS`]
//!   glued ends is **refused in words**, naming the count and the limit and saying to select part of the diagram; nothing is made before the counts are checked. A layout whose
//!   places would pass `MAX_COORD` is refused too.
//!
//! Auto Space and the page breaks are in [`spacing`] and [`breaks`].

use std::collections::HashSet;

use diacraft_doc::{CellAddr, CellEdit, Document, ShapeRef, SheetRef, Value};
use diacraft_layout::{Align, Direction, Graph, Node, Settings, Side, Style, Work};

use super::read::Reader;
use super::route::P;
use super::settle::settle_forced;
use super::{MAX_COORD, Problem, callouts, connectors, page_fit, scan, shapes, write_cells};
use crate::stencil::number_text;

mod breaks;
#[cfg(test)]
mod glue_cost;
#[cfg(test)]
mod hostile;
pub mod org;
#[cfg(test)]
mod org_hostile;
pub mod org_picture;
mod scope;
mod spacing;
#[cfg(test)]
mod tests;

pub use breaks::{Breaks, MovedOff, move_off_breaks, page_breaks};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use breaks::{TOUCH as BREAK_TOUCH, off_breaks};
pub use scope::{MAX_CONNECTORS, MAX_GLUE_ROWS, MAX_LISTED, MAX_NODES, MAX_PAGE_SHAPES};
pub use spacing::{Spaced, autospace, set_spacing, spacing_of};

use scope::Scope;

/// The root style's spacing (`AvenueSizeX` and `AvenueSizeY`: 3/8 inch in a US file, 7.5 millimetres in a metric one), in inches, for a page that holds none.
pub const DEFAULT_SPACING_INCHES: f64 = 0.375;

/// The layout `PlaceStyle` code of a style (the table).
pub fn place_code(style: Style) -> u8 {
    match style {
        Style::Flowchart(dir) => match dir {
            Direction::TopToBottom => 1,
            Direction::LeftToRight => 2,
            Direction::BottomToTop => 4,
            Direction::RightToLeft => 5,
        },
        Style::Radial => 3,
        Style::Circular => 6,
        Style::CompactTree { run, level } => 7 + COMPACT.iter().position(|c| *c == (run, level)).map_or(0, |i| i as u8),
        Style::Hierarchy(dir, align) => {
            let base = match dir {
                Direction::TopToBottom => 16,
                Direction::BottomToTop => 19,
                Direction::LeftToRight => 22,
                Direction::RightToLeft => 25,
            };
            base + match align {
                Align::Start => 0,
                Align::Center => 1,
                Align::End => 2,
            }
        }
    }
}

/// The compact trees, in the order of `PlaceStyle` 7 to 14: down right, right down, right up, up right, up left, left up, left down, down left.
const COMPACT: [(Side, Side); 8] = [
    (Side::Down, Side::Right),
    (Side::Right, Side::Down),
    (Side::Right, Side::Up),
    (Side::Up, Side::Right),
    (Side::Up, Side::Left),
    (Side::Left, Side::Up),
    (Side::Left, Side::Down),
    (Side::Down, Side::Left),
];

/// The style a `PlaceStyle` code stands for. 0 (the page's default), 15 (the parent's, which a shape's cell may hold) and any number past 27 are a flowchart down the page.
pub fn style_of_code(code: u8) -> Style {
    match code {
        1 => Style::Flowchart(Direction::TopToBottom),
        2 => Style::Flowchart(Direction::LeftToRight),
        3 => Style::Radial,
        4 => Style::Flowchart(Direction::BottomToTop),
        5 => Style::Flowchart(Direction::RightToLeft),
        6 => Style::Circular,
        7..=14 => {
            let (run, level) = COMPACT.get(usize::from(code - 7)).copied().unwrap_or((Side::Down, Side::Right));
            Style::CompactTree { run, level }
        }
        16..=27 => {
            let dir = match (code - 16) / 3 {
                0 => Direction::TopToBottom,
                1 => Direction::BottomToTop,
                2 => Direction::LeftToRight,
                _ => Direction::RightToLeft,
            };
            let align = match (code - 16) % 3 {
                0 => Align::Start,
                1 => Align::Center,
                _ => Align::End,
            };
            Style::Hierarchy(dir, align)
        }
        _ => Style::Flowchart(Direction::TopToBottom),
    }
}

/// The name of a style's kind, as the command spells it.
pub fn kind_name(style: Style) -> &'static str {
    match style {
        Style::Flowchart(_) => "flowchart",
        Style::Hierarchy(..) => "hierarchy",
        Style::CompactTree { .. } => "compact_tree",
        Style::Radial => "radial",
        Style::Circular => "circular",
    }
}

fn flow_name(dir: Direction) -> &'static str {
    match dir {
        Direction::TopToBottom => "top_to_bottom",
        Direction::BottomToTop => "bottom_to_top",
        Direction::LeftToRight => "left_to_right",
        Direction::RightToLeft => "right_to_left",
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Up => "up",
        Side::Down => "down",
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// The name of a style's direction, as the command spells it; `None` for the styles with none (radial, circular).
pub fn direction_name(style: Style) -> Option<String> {
    match style {
        Style::Flowchart(dir) | Style::Hierarchy(dir, _) => Some(flow_name(dir).to_owned()),
        Style::CompactTree { run, level } => Some(format!("{}_{}", side_name(run), side_name(level))),
        Style::Radial | Style::Circular => None,
    }
}

/// The name of a hierarchy's alignment; `None` for any other style.
pub fn alignment_name(style: Style) -> Option<&'static str> {
    match style {
        Style::Hierarchy(_, Align::Start) => Some("start"),
        Style::Hierarchy(_, Align::Center) => Some("center"),
        Style::Hierarchy(_, Align::End) => Some("end"),
        _ => None,
    }
}

fn parse_flow(text: &str) -> Option<Direction> {
    Some(match text {
        "top_to_bottom" => Direction::TopToBottom,
        "bottom_to_top" => Direction::BottomToTop,
        "left_to_right" => Direction::LeftToRight,
        "right_to_left" => Direction::RightToLeft,
        _ => return None,
    })
}

fn parse_compact(text: &str) -> Option<(Side, Side)> {
    COMPACT.iter().copied().find(|(run, level)| text == format!("{}_{}", side_name(*run), side_name(*level)))
}

fn parse_alignment(text: &str) -> Option<Align> {
    Some(match text {
        "start" | "left" | "top" => Align::Start,
        "center" | "middle" => Align::Center,
        "end" | "right" | "bottom" => Align::End,
        _ => return None,
    })
}

/// The style a call asks for: its kind, direction and alignment, each taken from `current` (the page's) when the call leaves it out and `current` is of the same kind, else the
/// kind's own first choice (a flowchart and a hierarchy down the page, a hierarchy centred, a compact tree down and to the right). `Err` in words for a name that is not a choice, and
/// for a direction or an alignment on a kind that has none (Direction is "enabled only for Flowchart, Compact Tree or Hierarchy styles", Alignment "only for Hierarchy").
pub fn choose(kind: Option<&str>, direction: Option<&str>, alignment: Option<&str>, current: Style) -> Result<Style, String> {
    let kind = kind.unwrap_or_else(|| kind_name(current));
    let same = kind_name(current) == kind;
    if alignment.is_some() && kind != "hierarchy" {
        return Err("`alignment` is for a hierarchy: a parent over its children, at the start, the middle or the end of them".to_owned());
    }
    if direction.is_some() && !matches!(kind, "flowchart" | "hierarchy" | "compact_tree") {
        return Err(format!("`direction` is for a flowchart, a hierarchy or a compact tree; a {kind} layout has none"));
    }
    let flow = |text: Option<&str>, kept: Option<Direction>| match text {
        Some(t) => parse_flow(t).ok_or_else(|| format!("`direction` {t:?} is not one of top_to_bottom, bottom_to_top, left_to_right, right_to_left")),
        None => Ok(kept.unwrap_or(Direction::TopToBottom)),
    };
    match kind {
        "flowchart" => {
            let kept = if let (true, Style::Flowchart(d)) = (same, current) { Some(d) } else { None };
            Ok(Style::Flowchart(flow(direction, kept)?))
        }
        "hierarchy" => {
            let (kept_dir, kept_align) = if let (true, Style::Hierarchy(d, a)) = (same, current) { (Some(d), Some(a)) } else { (None, None) };
            let align = match alignment {
                Some(t) => parse_alignment(t)
                    .ok_or_else(|| format!("`alignment` {t:?} is not one of start, center, end (or left, middle, right, top, bottom)"))?,
                None => kept_align.unwrap_or(Align::Center),
            };
            Ok(Style::Hierarchy(flow(direction, kept_dir)?, align))
        }
        "compact_tree" => {
            let kept = if let (true, Style::CompactTree { run, level }) = (same, current) { Some((run, level)) } else { None };
            let (run, level) = match direction {
                Some(t) => parse_compact(t).ok_or_else(|| {
                    "`direction` of a compact tree is one of down_right, right_down, right_up, up_right, up_left, left_up, left_down, down_left"
                        .to_owned()
                })?,
                None => kept.unwrap_or((Side::Down, Side::Right)),
            };
            Ok(Style::CompactTree { run, level })
        }
        "radial" => Ok(Style::Radial),
        "circular" => Ok(Style::Circular),
        other => Err(format!("`style` {other:?} is not one of flowchart, hierarchy, compact_tree, radial, circular")),
    }
}

/// What the page's cells say of its layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Current {
    pub style: Style,
    /// `PlaceStyle` as the page holds it (0 for none).
    pub place_style: u8,
    /// `AvenueSizeX` and `AvenueSizeY` in drawing units, the root style's where the page holds none.
    pub avenue: (f64, f64),
    pub block: (f64, f64),
    pub depth: u8,
    pub flip: u8,
    pub resize_page: bool,
    pub avoid_page_breaks: bool,
    /// The page's `RouteStyle` and `LineRouteExt`.
    pub route: (u8, u8),
}

/// A cell of a whole number from 0 to 255, or 0.
fn small(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> u8 {
    let v = reader.number(sheet, name);
    if v.is_finite() && (0.0..=255.0).contains(&v) { v.round() as u8 } else { 0 }
}

/// What the cells of page `page` say of its layout.
pub fn current(doc: &Document, page: u32) -> Result<Current, Problem> {
    super::require_page(doc, page)?;
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let k = super::units_per_inch(doc, page);
    let length = |name: &str, default: f64| {
        let v = reader.number(&sheet, name);
        if v.is_finite() && v > 0.0 { v } else { default * k }
    };
    let place_style = small(&reader, &sheet, "PlaceStyle");
    Ok(Current {
        style: style_of_code(place_style),
        place_style,
        avenue: (length("AvenueSizeX", DEFAULT_SPACING_INCHES), length("AvenueSizeY", DEFAULT_SPACING_INCHES)),
        block: (length("BlockSizeX", 0.25), length("BlockSizeY", 0.25)),
        depth: small(&reader, &sheet, "PlaceDepth"),
        flip: small(&reader, &sheet, "PlaceFlip"),
        resize_page: reader.number(&sheet, "ResizePage") != 0.0,
        avoid_page_breaks: reader.number(&sheet, "AvoidPageBreaks") != 0.0,
        route: (small(&reader, &sheet, "RouteStyle"), small(&reader, &sheet, "LineRouteExt")),
    })
}

/// What a call asks of [`relayout`].
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub style: Style,
    /// The space between shapes along x and along y, in drawing units (`AvenueSizeX`, `AvenueSizeY`).
    pub avenue: (f64, f64),
    /// More page cells to write, by name (`BlockSizeX`, `PlaceDepth`, `PlaceFlip`, ...): numbers in the cell's own unit (lengths in drawing units).
    pub cells: Vec<(&'static str, f64)>,
    /// The page's connector style and appearance to write (`RouteStyle`, `LineRouteExt`), and whether every connector of the scope takes them too.
    pub route: Option<(f64, f64)>,
    pub apply_routing: bool,
    /// `Some` writes `ResizePage`; the page is enlarged to take in the drawing when the cell says so.
    pub resize_page: Option<bool>,
    /// The shapes are laid out as an org chart (`org.rs`): the style must be a hierarchy or a compact tree, and each shape's own `ShapePlaceStyle` says how its reports sit.
    pub org: bool,
}

/// What [`relayout`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Done {
    /// The shapes laid out.
    pub nodes: usize,
    /// The connectors joining two of them.
    pub joins: usize,
    /// The shapes that moved.
    pub moved: usize,
    /// The connectors that were routed again.
    pub routed: usize,
    /// The connectors that join two shapes of the scope and were not glued again at the points that face each other: a shape has no connection points, its layers allow no glue, or its
    /// points are past what one call reads. They keep the points they are glued to.
    pub not_reglued: usize,
    /// The shapes on a locked layer that were left where they are (the first [`MAX_LISTED`]).
    pub locked: Vec<u32>,
    /// How many shapes named are inside a group, and how many move along with another (a member of a container, a callout) and so were not laid out themselves.
    pub inside_groups: usize,
    pub followers: usize,
    /// The page cells that were written.
    pub cells: usize,
    /// Whether the page was enlarged to take the drawing in.
    pub enlarged: bool,
    /// Whether anything in the document differs from before.
    pub changed: bool,
}

/// The graph of a scope: its shapes, and the connectors glued at both ends to two of them (a shape joined to itself is left out by the layout).
fn graph_of(scope: &Scope) -> (Graph, usize) {
    let nodes: Vec<Node> = scope.nodes.iter().map(|n| Node { width: n.size.0, height: n.size.1, x: n.centre.0, y: n.centre.1 }).collect();
    let edges: Vec<(u32, u32)> = scope
        .wires
        .iter()
        .filter_map(|w| match w.ends {
            [Some(a), Some(b)] if a != b => Some((u32::try_from(a).ok()?, u32::try_from(b).ok()?)),
            _ => None,
        })
        .collect();
    let joins = edges.len();
    (Graph { nodes, edges }, joins)
}

/// Lays out the shapes (see the module documentation) → what was done. `ids` are the shapes to lay out (`None`: the page).
pub fn relayout(doc: &mut Document, page: u32, ids: Option<&[u32]>, req: &Request) -> Result<Done, Problem> {
    let before = doc.clone();
    let scope = scope::read(doc, page, ids)?;
    let (graph, joins) = graph_of(&scope);
    let mut work = Work::new();
    let settings = Settings { style: req.style, gap_x: req.avenue.0, gap_y: req.avenue.1 };
    let plan = if req.org { Some(org::plan(doc, &scope, req)?) } else { None };
    let places = if let Some(plan) = &plan {
        org::places(&graph, plan, req, &mut work)?
    } else {
        diacraft_layout::layout_with(&graph, &settings, &mut work).map_err(|e| match e {
            diacraft_layout::Error::Invalid(words) => Problem::bad(words),
            diacraft_layout::Error::TooLarge(words) => Problem::failed(words),
        })?
    };
    let paths = moves_of(&scope, &places)?;
    let mut done = Done {
        nodes: scope.nodes.len(),
        joins,
        moved: paths.len(),
        locked: scope.locked.clone(),
        inside_groups: scope.inside_groups,
        followers: scope.followers,
        ..Done::default()
    };
    done.cells = write_settings(doc, page, req)?;
    let wires: Vec<u32> = scope.wires.iter().map(|w| w.id).collect();
    let touched = move_all(doc, page, &paths)?;
    // The connectors: their style, a hand edit taken off, and both ends glued again at the points that face each other; then every connector of the scope is routed
    // again, whatever its own setting. A route is made once.
    if req.apply_routing || !wires.is_empty() {
        done.not_reglued = write_connectors(doc, page, &scope.wires, req, plan.as_ref())?;
    }
    if !touched.is_empty() || !wires.is_empty() || done.cells > 0 {
        settle_forced(doc, &before, page, &touched, &[], &wires)?;
    }
    done.routed = wires.len();
    done.enlarged = grow(doc, page, &touched, req)?;
    done.changed = *doc != before;
    Ok(done)
}

/// The shapes to move, each with where it is and how far it goes: only those that go. A place past `MAX_COORD` is refused in words.
fn moves_of(scope: &Scope, places: &[(f64, f64)]) -> Result<Vec<(u32, ShapeRef, P)>, Problem> {
    let mut paths = Vec::new();
    for (node, place) in scope.nodes.iter().zip(places) {
        if place.0.abs() > MAX_COORD || place.1.abs() > MAX_COORD {
            return Err(Problem::failed(format!(
                "the layout would place shape {} more than {MAX_COORD} units from the page's origin: use a smaller space between shapes, or lay out part of the diagram",
                node.id
            )));
        }
        let d = (place.0 - node.centre.0, place.1 - node.centre.1);
        if d.0.abs() > 1.0e-9 || d.1.abs() > 1.0e-9 {
            paths.push((node.id, node.at.clone(), d));
        }
    }
    Ok(paths)
}

/// Moves the shapes `paths` (each with where it is and how far it goes) with what follows them, and settles the page: what the edit changed is worked out again and the connectors
/// glued to the shapes that moved, and those `forced`, are routed again. `force_settle` settles when nothing moved (cells were written). → the `ID`s of every shape that moved.
pub(super) fn carry_out(
    doc: &mut Document,
    page: u32,
    before: &Document,
    paths: &[(u32, ShapeRef, P)],
    forced: &[u32],
    force_settle: bool,
) -> Result<Vec<u32>, Problem> {
    let touched = move_all(doc, page, paths)?;
    if !touched.is_empty() || !forced.is_empty() || force_settle {
        settle_forced(doc, before, page, &touched, &[], forced)?;
    }
    Ok(touched)
}

/// Moves the shapes with what follows them (callouts, a container's members), writing the cells and nothing else. → the `ID`s of every shape that moved.
fn move_all(doc: &mut Document, page: u32, paths: &[(u32, ShapeRef, P)]) -> Result<Vec<u32>, Problem> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut all: Vec<(u32, ShapeRef, P)> = paths.to_vec();
    all.extend(callouts::followers(doc, page, paths)?);
    shapes::write_moves(doc, page, &all)?;
    let ids: HashSet<u32> = all.iter().map(|(id, _, _)| *id).collect();
    let mut ids: Vec<u32> = ids.into_iter().collect();
    ids.sort_unstable();
    Ok(ids)
}

/// A page cell's edit for a number.
fn number_edit(n: f64) -> CellEdit {
    CellEdit::Value(Value::from_text(&number_text(n)))
}

/// Writes the page's cells the request holds, those that differ from what the page resolves them to. → how many were written.
fn write_settings(doc: &mut Document, page: u32, req: &Request) -> Result<usize, Problem> {
    let sheet = SheetRef::Page(page);
    let mut wanted: Vec<(&'static str, f64)> =
        vec![("PlaceStyle", f64::from(place_code(req.style))), ("AvenueSizeX", req.avenue.0), ("AvenueSizeY", req.avenue.1)];
    wanted.extend(req.cells.iter().copied());
    if let Some(on) = req.resize_page {
        wanted.push(("ResizePage", f64::from(u8::from(on))));
    }
    if let Some((style, appearance)) = req.route {
        wanted.push(("RouteStyle", style));
        wanted.push(("LineRouteExt", appearance));
    }
    let writes: Vec<(SheetRef, CellAddr, CellEdit)> = {
        let reader = Reader::new(doc);
        wanted
            .iter()
            .filter(|(name, v)| (reader.number(&sheet, name) - v).abs() > 1.0e-9 || (*v != 0.0 && !reader.is_set(&sheet, &CellAddr::direct(name))))
            .map(|(name, v)| (sheet.clone(), CellAddr::direct(name), number_edit(*v)))
            .collect()
    };
    let n = writes.len();
    if n > 0 {
        write_cells(doc, writes)?;
    }
    Ok(n)
}

/// The connectors' own cells: the style and appearance when the request applies them, the hand edit taken off a route that has one (`ConFixedCode` 3 becomes 0), as the Connectors menu
/// does (`style.rs`), and ours: a connector glued at both ends to two shapes of the scope is **glued again at the pair of connection points that face each other**, as
/// a new connector between them would be, so that a flow down the page leaves a shape by its bottom and comes to the next by its top (the reference application's connectors choose their
/// sides again when a page is laid out again).
fn write_connectors(doc: &mut Document, page: u32, wires: &[scope::Wire], req: &Request, plan: Option<&org::Plan>) -> Result<usize, Problem> {
    if wires.is_empty() {
        return Ok(0);
    }
    let wanted: HashSet<u32> = wires.iter().map(|w| w.id).collect();
    let found = scan::scan(doc, page, &wanted)?.found;
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    {
        let reader = Reader::new(doc);
        for at in found.values() {
            let sheet = SheetRef::Shape(at.clone());
            let held = |name: &str| reader.number(&sheet, name);
            if let (true, Some((style, appearance))) = (req.apply_routing, req.route) {
                for (name, v) in [("ShapeRouteStyle", style), ("ConLineRouteExt", appearance)] {
                    if (held(name) - v).abs() > 0.5 || held(name) == 0.0 {
                        writes.push((sheet.clone(), CellAddr::direct(name), number_edit(v)));
                    }
                }
            }
        }
    }
    if !writes.is_empty() {
        write_cells(doc, writes)?;
    }
    let mut plans: Vec<connectors::Reglue> = Vec::new();
    for wire in wires {
        let (Some(at), [Some(from), Some(to)]) = (found.get(&wire.id), wire.raw) else { continue };
        if matches!(wire.ends, [Some(_), Some(_)]) && from != to {
            let sides = plan.zip(wire.ends[0]).map(|(plan, manager)| org::sides(plan, manager));
            plans.push(connectors::Reglue { id: wire.id, at: at.clone(), from, to, sides });
        }
    }
    let reglued = connectors::reglue_all(doc, page, &plans)?;
    // A hand edit of a route that was not glued again is taken off too: a route made by hand for places that are no more cannot be kept.
    let mut resets: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    {
        let reader = Reader::new(doc);
        for at in found.values() {
            let sheet = SheetRef::Shape(at.clone());
            if (reader.number(&sheet, "ConFixedCode") - 3.0).abs() < 0.5 {
                resets.push((sheet, CellAddr::direct("ConFixedCode"), number_edit(0.0)));
            }
        }
    }
    if !resets.is_empty() {
        write_cells(doc, resets)?;
    }
    Ok(reglued.skipped)
}

/// Grows the page for the shapes that moved: by Auto Size, and by `ResizePage` ("Enlarge page to fit drawing") whatever Auto Size says. → whether it was enlarged
/// for `ResizePage`.
fn grow(doc: &mut Document, page: u32, moved: &[u32], req: &Request) -> Result<bool, Problem> {
    if moved.is_empty() {
        return Ok(false);
    }
    let by_size = page_fit::fit_page(doc, page, moved)?;
    let wanted = match req.resize_page {
        Some(on) => on,
        None => Reader::new(doc).number(&SheetRef::Page(page), "ResizePage") != 0.0,
    };
    let by_cell = wanted && page_fit::enlarge_page(doc, page, moved)?;
    Ok(by_size || by_cell)
}
