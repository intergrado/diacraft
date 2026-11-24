//! The organization chart: positions, the links between them, and the layout of a chart.
//!
//! The specifications give the feature in sentences, not in cells, so every rule here is **ours**
//! unless it names a source, and `docs/editing-commands.md` ("The Organization Chart") lists them.
//!
//! * A *position* is a shape at the top of a page that holds the user cell `User.DiaCraftOrgPosition` (the kind of position), of its own or through its master:
//!   every shape dropped from the Organization Chart Shapes stencil does (`stencil/orgchart.rs`). Only a shape at the top of the page is one: a position inside a group is the group's
//!   business.
//! * The hierarchy **is the glue**: a connector whose begin is glued to one position and whose end is glued to another makes the first the *manager* of the second ("dropping a
//!   subordinate shape onto a superior connects the two in the hierarchy, with no connector to draw"). A position has at most one manager through [`link`]
//!   (linking it again moves it to the new manager: the old connector goes); a drawing from a file may hold more, and the layout reads the first that reaches it (`diacraft-layout`'s forest).
//!   [`link`] refuses, in words, to make a manager of a position that reports to the shape, directly or through others: a reporting loop.
//! * The whole chart is laid out by `diacraft_layout::org_chart` as a tree: a flow (down, up, right or left) and, for each manager, how its reports sit: side by side under it
//!   (**across**) or in a column to one side (**hanging**). The page's `PlaceStyle` holds the chart's own choice, a hierarchy (16 to 27, across) or a compact tree (7 to 14, hanging), the
//!   six choices of the Layout gallery ([`ChartLayout`]); a manager's own choice is its `ShapePlaceStyle` (0 or 15, the page's; a hierarchy or any of 1 to 6, across; a compact tree, hanging on
//!   the side the compact tree's levels go), which is how **Arrange Subordinates** is kept and how **Re-Layout** keeps "layout options already applied, such as a vertical layout"
//!   A Layout choice for the whole chart clears every manager's own choice.
//! * A position dropped **inside the box of another position** becomes its report: [`dropped`] links it and lays the chart out again, in the drop's own edit. It goes
//!   last among the manager's reports (it is set past the others in the flow's frame before the layout reads the order).
//! * A page is read once (at most [`MAX_PAGE_SHAPES`] shapes at its top, at most [`MAX_GLUE_ROWS`] glued ends), a chart of more than [`MAX_NODES`] positions or
//!   [`MAX_CONNECTORS`] links is refused in words by the layout's own scope, and every lookup is by a map: a manager with 10,000 reports and a chain of 20,000 cost a pass each.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, RowKey, ShapeRef, ShapeType, SheetRef, Value as CellValue};
use diacraft_layout::{Align, Direction, OrgSettings, Side, Spread, Style, Work, org_chart_with};

use super::super::route::Dir;
use super::scope::{self, MAX_GLUE_ROWS, MAX_NODES, MAX_PAGE_SHAPES, Scope};
use super::{Current, Done, Request, current, place_code, relayout};
use crate::draw::read::Reader;
use crate::draw::{End, Problem, add_connector_above, delete_shapes, move_shapes, write_cells};
use crate::stencil::orgchart::MARKER_ROW;

/// The longest the kind of a position is read as, in characters.
const MAX_KIND_CHARS: usize = 64;

/// A position of an org chart: a shape at the top of a page that holds the marker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub id: u32,
    pub at: ShapeRef,
    /// The kind of position: `Executive`, `Manager`, `Position`, `Assistant`, `Consultant`, `Vacancy` for the stencil's, anything a file holds.
    pub kind: String,
}

/// The marker's address.
fn marker() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(MARKER_ROW), "Value")
}

/// The marker a shape holds of its own, as the kind it names.
fn own_kind(shape: &diacraft_doc::Shape) -> Option<String> {
    match shape.sheet.lookup(&marker()) {
        Lookup::Found(cell) => {
            let text = cell.value.as_ref()?.text().trim();
            (!text.is_empty()).then(|| text.chars().take(MAX_KIND_CHARS).collect())
        }
        _ => None,
    }
}

/// The kind each master of the document names: the marker its first shape holds.
fn master_kinds(doc: &Document) -> HashMap<u32, String> {
    doc.masters.iter().filter_map(|m| Some((m.id, own_kind(m.shapes.get(0)?)?))).collect()
}

/// The positions at the top of a page, in the page's order. A page with more than [`MAX_PAGE_SHAPES`] shapes at its top is refused in words, before any is read.
pub fn positions(doc: &Document, page: u32) -> Result<Vec<Position>, Problem> {
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if p.shapes.len() > MAX_PAGE_SHAPES {
        return Err(Problem::failed(format!(
            "the page has {} shapes at its top, more than the {MAX_PAGE_SHAPES} the org chart reads: split the drawing over pages",
            p.shapes.len()
        )));
    }
    let masters = master_kinds(doc);
    let mut out = Vec::new();
    for (i, shape) in p.shapes.iter().enumerate() {
        if shape.kind == ShapeType::Guide {
            continue;
        }
        let kind = own_kind(shape).or_else(|| shape.master.and_then(|m| masters.get(&m).cloned()));
        if let Some(kind) = kind {
            out.push(Position { id: shape.id, at: ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(i).unwrap_or(u32::MAX)] }, kind });
        }
    }
    Ok(out)
}

/// How many positions each page of the document holds, and the total, stopping at the first page past `limit` shapes read in all: `(pages with positions, total, complete)`. What the
/// window asks to know whether the drawing has an org chart (the Org Chart tab). With `quick` it stops at the first page that holds a position (the totals are then that page's, and the
/// answer is not `complete`): the tab needs to know there is a chart and no more.
pub fn summary(doc: &Document, limit: usize, quick: bool) -> (Vec<(u32, usize)>, usize, bool) {
    let masters = master_kinds(doc);
    let (mut pages, mut total, mut read) = (Vec::new(), 0_usize, 0_usize);
    for p in doc.pages.iter() {
        let mut here = 0_usize;
        for shape in p.shapes.iter() {
            read += 1;
            if read > limit {
                return (pages, total, false);
            }
            if shape.kind != ShapeType::Guide && (own_kind(shape).is_some() || shape.master.is_some_and(|m| masters.contains_key(&m))) {
                here += 1;
            }
        }
        if here > 0 {
            pages.push((p.id, here));
            total += here;
            if quick {
                return (pages, total, false);
            }
        }
    }
    (pages, total, true)
}

/// A connector that makes `manager` the manager of `report`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub connector: u32,
    pub manager: u32,
    pub report: u32,
}

/// The positions of a page and the links between them, with the lookups a call asks of them **made once, when the chart is read** (each position by its `ID`, the links by manager and by
/// report): a lookup for each position or each link would be a pass over all of them for each.
#[derive(Clone, Debug, Default)]
pub struct Chart {
    pub positions: Vec<Position>,
    pub links: Vec<Link>,
    place: HashMap<u32, usize>,
    of_manager: HashMap<u32, Vec<usize>>,
    of_report: HashMap<u32, Vec<usize>>,
}

/// Reads the chart of a page: its positions and the connectors glued (begin and end) to two of them. A page with more glued ends than [`MAX_GLUE_ROWS`] is refused in words.
pub fn chart(doc: &Document, page: u32) -> Result<Chart, Problem> {
    let positions = positions(doc, page)?;
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if p.connects.len() > MAX_GLUE_ROWS {
        return Err(Problem::failed(format!(
            "the page has {} glued ends, more than the {MAX_GLUE_ROWS} the org chart reads: split the drawing over pages",
            p.connects.len()
        )));
    }
    let ids: HashSet<u32> = positions.iter().map(|p| p.id).collect();
    let mut links: Vec<Link> = scope::glue_ends(doc, page)
        .into_iter()
        .filter_map(|(connector, ends)| match ends {
            [Some(manager), Some(report)] if manager != report && ids.contains(&manager) && ids.contains(&report) => {
                Some(Link { connector, manager, report })
            }
            _ => None,
        })
        .collect();
    links.sort_unstable_by_key(|l| l.connector);
    let place: HashMap<u32, usize> = positions.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let (mut of_manager, mut of_report): (HashMap<u32, Vec<usize>>, HashMap<u32, Vec<usize>>) = (HashMap::new(), HashMap::new());
    for (i, l) in links.iter().enumerate() {
        of_manager.entry(l.manager).or_default().push(i);
        of_report.entry(l.report).or_default().push(i);
    }
    Ok(Chart { positions, links, place, of_manager, of_report })
}

impl Chart {
    /// How many positions are in a reporting loop or report to one: the positions that peeling off, over and over, the positions nothing reports to leaves. 0 for a chart that is a forest.
    pub fn in_loops(&self) -> usize {
        let index = &self.place;
        let mut waiting = vec![0_usize; self.positions.len()];
        let mut reports: Vec<Vec<usize>> = vec![Vec::new(); self.positions.len()];
        for l in &self.links {
            if let (Some(&m), Some(&r)) = (index.get(&l.manager), index.get(&l.report)) {
                waiting[r] += 1;
                reports[m].push(r);
            }
        }
        let mut queue: Vec<usize> = (0..waiting.len()).filter(|&i| waiting[i] == 0).collect();
        let mut done = 0;
        while let Some(i) = queue.pop() {
            done += 1;
            for &r in &reports[i] {
                waiting[r] -= 1;
                if waiting[r] == 0 {
                    queue.push(r);
                }
            }
        }
        self.positions.len() - done
    }

    /// The position with this `ID`.
    pub fn position(&self, id: u32) -> Option<&Position> {
        self.place.get(&id).and_then(|i| self.positions.get(*i))
    }

    /// The links that make position `id` a manager, in the order of their connectors.
    pub fn reports_of(&self, id: u32) -> impl Iterator<Item = &Link> {
        self.of_manager.get(&id).into_iter().flatten().filter_map(|i| self.links.get(*i))
    }

    /// The links that make position `id` a report (one for a position that has a manager).
    pub fn managers_of(&self, id: u32) -> impl Iterator<Item = &Link> {
        self.of_report.get(&id).into_iter().flatten().filter_map(|i| self.links.get(*i))
    }

    /// The manager of a position (the first link that reaches it), and how many reports it has.
    pub fn manager_and_reports(&self, id: u32) -> (Option<u32>, usize) {
        (self.managers_of(id).next().map(|l| l.manager), self.of_manager.get(&id).map_or(0, Vec::len))
    }

    /// The first `limit` positions nothing reports to.
    pub fn roots(&self, limit: usize) -> Vec<u32> {
        self.positions.iter().map(|p| p.id).filter(|id| !self.of_report.contains_key(id)).take(limit).collect()
    }
}

// ---- the layouts ------------------------------------------------------------------------------------------------------

/// The six layouts of the Org Chart tab's gallery for a whole chart: the four directions, each with the reports of a manager side by side, and the two that hang them
/// in a column. The specifications say a Layout choice "rearranges the chart's hierarchy into another pattern" and do not list the patterns: these are ours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartLayout {
    TopToBottom,
    BottomToTop,
    LeftToRight,
    RightToLeft,
    HangingRight,
    HangingLeft,
}

/// The layouts, in the order of the gallery.
pub const LAYOUTS: [ChartLayout; 6] = [
    ChartLayout::TopToBottom,
    ChartLayout::BottomToTop,
    ChartLayout::LeftToRight,
    ChartLayout::RightToLeft,
    ChartLayout::HangingRight,
    ChartLayout::HangingLeft,
];

impl ChartLayout {
    /// The word a command spells it.
    pub fn name(self) -> &'static str {
        match self {
            ChartLayout::TopToBottom => "top_to_bottom",
            ChartLayout::BottomToTop => "bottom_to_top",
            ChartLayout::LeftToRight => "left_to_right",
            ChartLayout::RightToLeft => "right_to_left",
            ChartLayout::HangingRight => "hanging_right",
            ChartLayout::HangingLeft => "hanging_left",
        }
    }

    /// The layout a word names.
    pub fn parse(word: &str) -> Option<ChartLayout> {
        LAYOUTS.into_iter().find(|l| l.name() == word)
    }

    /// The `PlaceStyle` the page holds for it.
    pub fn style(self) -> Style {
        match self {
            ChartLayout::TopToBottom => Style::Hierarchy(Direction::TopToBottom, Align::Center),
            ChartLayout::BottomToTop => Style::Hierarchy(Direction::BottomToTop, Align::Center),
            ChartLayout::LeftToRight => Style::Hierarchy(Direction::LeftToRight, Align::Center),
            ChartLayout::RightToLeft => Style::Hierarchy(Direction::RightToLeft, Align::Center),
            ChartLayout::HangingRight => Style::CompactTree { run: Side::Down, level: Side::Right },
            ChartLayout::HangingLeft => Style::CompactTree { run: Side::Down, level: Side::Left },
        }
    }

    /// The layout a page's `PlaceStyle` stands for, when it is one of the six (a hierarchy of any alignment counts for its direction).
    pub fn of_style(style: Style) -> Option<ChartLayout> {
        match style {
            Style::Hierarchy(Direction::TopToBottom, _) => Some(ChartLayout::TopToBottom),
            Style::Hierarchy(Direction::BottomToTop, _) => Some(ChartLayout::BottomToTop),
            Style::Hierarchy(Direction::LeftToRight, _) => Some(ChartLayout::LeftToRight),
            Style::Hierarchy(Direction::RightToLeft, _) => Some(ChartLayout::RightToLeft),
            Style::CompactTree { run: Side::Down, level: Side::Right } => Some(ChartLayout::HangingRight),
            Style::CompactTree { run: Side::Down, level: Side::Left } => Some(ChartLayout::HangingLeft),
            _ => None,
        }
    }
}

/// How one manager's reports are set out (the choices of Arrange Subordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    /// The page's choice (`ShapePlaceStyle` 0).
    Inherit,
    /// Side by side under the manager.
    Horizontal,
    /// In a column, a space to the right of the manager's middle line (for a flow across the page: below it).
    VerticalRight,
    /// In a column to the left (for a flow across the page: above).
    VerticalLeft,
}

/// The arrangements, in the order of the menu.
pub const ARRANGEMENTS: [Arrangement; 4] = [Arrangement::Horizontal, Arrangement::VerticalRight, Arrangement::VerticalLeft, Arrangement::Inherit];

impl Arrangement {
    pub fn name(self) -> &'static str {
        match self {
            Arrangement::Inherit => "inherit",
            Arrangement::Horizontal => "horizontal",
            Arrangement::VerticalRight => "vertical_right",
            Arrangement::VerticalLeft => "vertical_left",
        }
    }

    pub fn parse(word: &str) -> Option<Arrangement> {
        ARRANGEMENTS.into_iter().find(|a| a.name() == word)
    }

    /// The `ShapePlaceStyle` code that holds it, in a chart that flows `flow`.
    pub fn code(self, flow: Direction) -> u8 {
        match self {
            Arrangement::Inherit => 0,
            Arrangement::Horizontal => place_code(Style::Hierarchy(flow, Align::Center)),
            Arrangement::VerticalRight => place_code(Style::CompactTree { run: Side::Down, level: Side::Right }),
            Arrangement::VerticalLeft => place_code(Style::CompactTree { run: Side::Down, level: Side::Left }),
        }
    }

    /// What a `ShapePlaceStyle` code says (0 and 15: the page's; 1 to 6 and 16 to 27: across; 7 to 14: hanging, to the right for levels that go right or down, else to the left).
    pub fn of_code(code: u8) -> Arrangement {
        match code {
            7..=14 => match super::style_of_code(code) {
                Style::CompactTree { level: Side::Left | Side::Up, .. } => Arrangement::VerticalLeft,
                _ => Arrangement::VerticalRight,
            },
            1..=6 | 16..=27 => Arrangement::Horizontal,
            _ => Arrangement::Inherit,
        }
    }
}

/// The flow of a chart and how its managers set out their reports when their own cell says nothing, from the page's style. `None` for a style that is no hierarchy or compact tree.
pub fn flow_of(style: Style) -> Option<(Direction, Spread)> {
    match style {
        Style::Hierarchy(dir, _) => Some((dir, Spread::Across)),
        Style::CompactTree { run, level } => {
            let dir = match run {
                Side::Down => Direction::TopToBottom,
                Side::Up => Direction::BottomToTop,
                Side::Right => Direction::LeftToRight,
                Side::Left => Direction::RightToLeft,
            };
            Some((dir, spread_of_level(dir, level)))
        }
        _ => None,
    }
}

/// The side of a flow's across axis (right of a flow down or up the page, below a flow across it) a compact tree's `level` side is on.
fn spread_of_level(flow: Direction, level: Side) -> Spread {
    // The across axis of a flow, as a direction on the page, `y` up: x for a flow down or up, minus y for a flow across.
    let across = if matches!(flow, Direction::TopToBottom | Direction::BottomToTop) { (1.0, 0.0) } else { (0.0, -1.0) };
    let (lx, ly) = match level {
        Side::Right => (1.0, 0.0),
        Side::Left => (-1.0, 0.0),
        Side::Up => (0.0, 1.0),
        Side::Down => (0.0, -1.0),
    };
    if across.0 * lx + across.1 * ly < 0.0 { Spread::Before } else { Spread::After }
}

/// How a shape's `ShapePlaceStyle` code sets out its reports: the page's choice for 0 and 15, across for a hierarchy or a flowchart code, hanging for a compact tree.
fn spread_of_code(code: u8, flow: Direction, page: Spread) -> Spread {
    match code {
        0 | 15 => page,
        7..=14 => match super::style_of_code(code) {
            Style::CompactTree { level, .. } => spread_of_level(flow, level),
            _ => page,
        },
        _ => Spread::Across,
    }
}

/// A whole number of a shape's cell from 0 to 255, or 0.
fn code_of(reader: &Reader<'_>, at: &ShapeRef) -> u8 {
    let v = reader.number(&SheetRef::Shape(at.clone()), "ShapePlaceStyle");
    if v.is_finite() && (0.0..=255.0).contains(&v) { v.round() as u8 } else { 0 }
}

/// How an org chart of a scope is laid out: the flow, and how each shape of the scope (by its place in the scope) sets out its reports.
pub(super) struct Plan {
    flow: Direction,
    spreads: Vec<Spread>,
}

/// The plan of a scope: the flow and the page's choice from `req.style`, each shape's own choice from its `ShapePlaceStyle`.
pub(super) fn plan(doc: &Document, scope: &Scope, req: &Request) -> Result<Plan, Problem> {
    let (flow, page) = flow_of(req.style).ok_or_else(|| Problem::bad("an org chart is laid out as a hierarchy or a compact tree"))?;
    let reader = Reader::new(doc);
    let spreads: Vec<Spread> = scope.nodes.iter().map(|n| spread_of_code(code_of(&reader, &n.at), flow, page)).collect();
    Ok(Plan { flow, spreads })
}

/// The sides a connector from the manager at place `manager` of the scope is glued at (ours): out of the manager along the flow and into the report from the side it
/// is set off, which is the top of a report under a manager that sets its reports side by side (a flow down the page), and the left side of a report in a column to the right of the
/// manager's middle line (the right side for one to the left of it).
pub(super) fn sides(plan: &Plan, manager: usize) -> (Dir, Dir) {
    let spread = plan.spreads.get(manager).copied().unwrap_or(Spread::Across);
    match (plan.flow, spread) {
        (Direction::TopToBottom, Spread::Across) => (Dir::Down, Dir::Up),
        (Direction::TopToBottom, Spread::After) => (Dir::Down, Dir::Left),
        (Direction::TopToBottom, Spread::Before) => (Dir::Down, Dir::Right),
        (Direction::BottomToTop, Spread::Across) => (Dir::Up, Dir::Down),
        (Direction::BottomToTop, Spread::After) => (Dir::Up, Dir::Left),
        (Direction::BottomToTop, Spread::Before) => (Dir::Up, Dir::Right),
        (Direction::LeftToRight, Spread::Across) => (Dir::Right, Dir::Left),
        (Direction::LeftToRight, Spread::After) => (Dir::Down, Dir::Left),
        (Direction::LeftToRight, Spread::Before) => (Dir::Up, Dir::Left),
        (Direction::RightToLeft, Spread::Across) => (Dir::Left, Dir::Right),
        (Direction::RightToLeft, Spread::After) => (Dir::Down, Dir::Right),
        (Direction::RightToLeft, Spread::Before) => (Dir::Up, Dir::Right),
    }
}

/// The places of the shapes of a scope laid out as an org chart by `plan`.
pub(super) fn places(graph: &diacraft_layout::Graph, plan: &Plan, req: &Request, work: &mut Work) -> Result<Vec<(f64, f64)>, Problem> {
    // The page's spaces are along x and along y, whichever way the flow runs: the layout turns them.
    let settings = OrgSettings { direction: plan.flow, spreads: &plan.spreads, gap_x: req.avenue.0, gap_y: req.avenue.1 };
    org_chart_with(graph, &settings, work).map_err(|e| match e {
        diacraft_layout::Error::Invalid(words) => Problem::bad(words),
        diacraft_layout::Error::TooLarge(words) => Problem::failed(words),
    })
}

/// What a call lays out and how.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChartRequest {
    /// A layout for the whole chart (clearing every manager's own choice), or `None` to keep what the page and its managers say.
    pub layout: Option<ChartLayout>,
    /// The spaces along x and along y in drawing units, or `None` for the page's.
    pub spacing: Option<(f64, f64)>,
}

/// What laying a chart out did.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    /// How many positions the chart has, and the layout's own account.
    pub positions: usize,
    pub done: Done,
    /// Whether the chart's choices (the page's and the managers') were written, apart from the move.
    pub changed: bool,
}

/// The page's own style when it is one an org chart is laid out in, else a hierarchy down the page.
fn page_style(now: &Current) -> Style {
    if flow_of(now.style).is_some() && now.place_style != 0 { now.style } else { ChartLayout::TopToBottom.style() }
}

/// Lays the chart of a page out (see the module documentation). A page with no position is refused in words.
pub fn lay_out(doc: &mut Document, page: u32, req: &ChartRequest) -> Result<Laid, Problem> {
    let before = doc.clone();
    let ids: Vec<u32> = positions(doc, page)?.into_iter().map(|p| p.id).collect();
    if ids.is_empty() {
        return Err(Problem::bad("the page has no org chart: drop a position from the Organization Chart Shapes onto it"));
    }
    if ids.len() > MAX_NODES {
        return Err(Problem::failed(format!(
            "the chart has {} positions, more than the {MAX_NODES} a layout places at once: split the drawing over pages",
            ids.len()
        )));
    }
    if req.layout.is_some() {
        clear_arrangements(doc, page, &ids)?;
    }
    let now = current(doc, page)?;
    let style = req.layout.map_or_else(|| page_style(&now), ChartLayout::style);
    let avenue = req.spacing.unwrap_or(now.avenue);
    let request = Request { style, avenue, cells: Vec::new(), route: None, apply_routing: false, resize_page: None, org: true };
    let done = relayout(doc, page, Some(&ids), &request)?;
    Ok(Laid { positions: ids.len(), changed: *doc != before, done })
}

/// Writes 0 to the `ShapePlaceStyle` of each position that holds another number: the page's choice is theirs again.
fn clear_arrangements(doc: &mut Document, page: u32, ids: &[u32]) -> Result<(), Problem> {
    let writes: Vec<(SheetRef, CellAddr, CellEdit)> = {
        let reader = Reader::new(doc);
        let found = crate::draw::scan::named(doc, page, ids)?.1;
        found
            .into_iter()
            .filter(|(_, at)| code_of(&reader, at) != 0)
            .map(|(_, at)| (SheetRef::Shape(at), CellAddr::direct("ShapePlaceStyle"), CellEdit::Value(CellValue::from_text("0"))))
            .collect()
    };
    if writes.is_empty() { Ok(()) } else { write_cells(doc, writes) }
}

/// Sets how the reports of position `id` are set out: its `ShapePlaceStyle`. → whether the cell changed. The layout is not run: [`lay_out`] does it.
pub fn arrange(doc: &mut Document, page: u32, id: u32, how: Arrangement) -> Result<bool, Problem> {
    let found = positions(doc, page)?.into_iter().find(|p| p.id == id).ok_or_else(|| not_a_position(page, id))?;
    let now = current(doc, page)?;
    let flow = flow_of(page_style(&now)).map_or(Direction::TopToBottom, |(d, _)| d);
    let want = how.code(flow);
    let have = {
        let reader = Reader::new(doc);
        code_of(&reader, &found.at)
    };
    if have == want {
        return Ok(false);
    }
    write_cells(
        doc,
        vec![(SheetRef::Shape(found.at), CellAddr::direct("ShapePlaceStyle"), CellEdit::Value(CellValue::from_text(&want.to_string())))],
    )?;
    Ok(true)
}

/// The arrangement a position holds (its `ShapePlaceStyle`, as [`Arrangement::of_code`] reads it).
pub fn arrangement_of(doc: &Document, at: &ShapeRef) -> Arrangement {
    Arrangement::of_code(code_of(&Reader::new(doc), at))
}

fn not_a_position(page: u32, id: u32) -> Problem {
    Problem::bad(format!("shape {id} of page {page} is not a position of an org chart (a shape dropped from the Organization Chart Shapes)"))
}

// ---- links ------------------------------------------------------------------------------------------------------------

/// What [`link`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linked {
    /// The connector that joins the manager to the report, new or the one that was there.
    pub connector: u32,
    /// The connectors that linked the report to another manager and were taken away.
    pub replaced: Vec<u32>,
    /// Whether anything changed: false when the manager had the report already.
    pub changed: bool,
}

/// Makes `manager` the manager of `report`: a connector from the one to the other, glued, and the report's old link, if it had one to another position, taken away. A position
/// that is the report itself, or reports (through any number of others) to the report, is refused in words: it would close a loop. The `ID` of the new connector is above `above`.
pub fn link(doc: &mut Document, page: u32, report: u32, manager: u32, above: u32) -> Result<Linked, Problem> {
    let chart = chart(doc, page)?;
    for id in [report, manager] {
        if chart.position(id).is_none() {
            return Err(not_a_position(page, id));
        }
    }
    if report == manager {
        return Err(Problem::bad(format!("position {report} cannot be its own manager")));
    }
    if let Some(same) = chart.managers_of(report).find(|l| l.manager == manager) {
        return Ok(Linked { connector: same.connector, replaced: Vec::new(), changed: false });
    }
    if reaches(&chart, report, manager) {
        return Err(Problem::bad(format!(
            "position {manager} reports to position {report}, directly or through others: making it {report}'s manager would close a reporting loop"
        )));
    }
    let replaced: Vec<u32> = chart.managers_of(report).map(|l| l.connector).collect();
    if !replaced.is_empty() {
        delete_shapes(doc, page, &replaced)?;
    }
    let made = add_connector_above(doc, page, &End::Shape { id: manager, point: None }, &End::Shape { id: report, point: None }, above)?;
    // The new report goes last among the manager's: it is set past the others in the layout's frame, where the layout reads the order from.
    let siblings: Vec<u32> = chart.reports_of(manager).filter(|l| l.report != report).map(|l| l.report).collect();
    if !siblings.is_empty() {
        let flow = flow_of(page_style(&current(doc, page)?)).map_or(Direction::TopToBottom, |(d, _)| d);
        set_last(doc, page, report, &siblings, flow)?;
    }
    Ok(Linked { connector: made.id, replaced, changed: true })
}

/// Whether `to` is among the reports of `from`, directly or through others: a walk down the links, each position once.
fn reaches(chart: &Chart, from: u32, to: u32) -> bool {
    let mut seen: HashSet<u32> = HashSet::from([from]);
    let mut stack = vec![from];
    while let Some(v) = stack.pop() {
        for l in chart.reports_of(v) {
            if l.report == to {
                return true;
            }
            if seen.insert(l.report) {
                stack.push(l.report);
            }
        }
    }
    false
}

/// The manager a drop at `at` (the page's drawing units) lands on: the position, other than `except`, whose box holds the point; the smallest box, and the later one in the page's order
/// where two are as small.
fn position_under(doc: &Document, positions: &[Position], at: (f64, f64), except: u32) -> Option<u32> {
    let reader = Reader::new(doc);
    let mut best: Option<(f64, u32)> = None;
    for p in positions.iter().filter(|p| p.id != except) {
        let corners = reader.corners(&p.at);
        if corners.iter().any(|(x, y)| !(x.is_finite() && y.is_finite())) {
            continue;
        }
        let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for (x, y) in corners {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
        if at.0 >= lo.0 && at.0 <= hi.0 && at.1 >= lo.1 && at.1 <= hi.1 {
            let area = (hi.0 - lo.0) * (hi.1 - lo.1);
            if best.is_none_or(|(a, _)| area <= a) {
                best = Some((area, p.id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// What a drop onto a position did.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropped {
    pub manager: u32,
    pub linked: Linked,
    pub laid: Laid,
}

/// Whether the shape `id` just dropped on `page` is a position: the shape a drop makes is the last of the page's list, so it is looked at first and no walk of the page is made for a shape that
/// is no position (a drop on a page of 100,000 shapes costs this and no more).
fn is_new_position(doc: &Document, page: u32, id: u32) -> bool {
    let Some(p) = doc.page(page) else { return false };
    let Some(shape) = p.shapes.len().checked_sub(1).and_then(|last| p.shapes.get(last)).filter(|s| s.id == id) else { return false };
    own_kind(shape).is_some()
        || shape.master.is_some_and(|m| doc.masters.iter().any(|mm| mm.id == m && mm.shapes.get(0).is_some_and(|s| own_kind(s).is_some())))
}

/// [`dropped`] for the command that drops a shape, which **never fails for it**: a position that cannot be linked (a page past what the chart reads, a chart past what a layout places) is
/// left where it was dropped and the words say why. The drop's own document is changed only when the whole link and layout were made. → what was done, and the words if the drop was left alone.
pub fn on_drop(doc: &mut Document, page: u32, id: u32, at: (f64, f64), above: u32) -> (Option<Dropped>, Option<String>) {
    if !is_new_position(doc, page, id) {
        return (None, None);
    }
    let mut trial = doc.clone();
    match dropped(&mut trial, page, id, at, above) {
        Ok(Some(done)) => {
            *doc = trial;
            (Some(done), None)
        }
        Ok(None) => (None, None),
        Err(problem) => (None, Some(format!("the position was left where it was dropped and not linked to a manager: {}", problem.message))),
    }
}

/// A position `id` was just dropped at `at` (the page's drawing units): when the point is inside another position's box, links it as that position's report and lays the chart out again
/// `None` when the shape is no position or the point is on no other one. The `ID` of the connector is above `above`.
pub fn dropped(doc: &mut Document, page: u32, id: u32, at: (f64, f64), above: u32) -> Result<Option<Dropped>, Problem> {
    let all = positions(doc, page)?;
    if !all.iter().any(|p| p.id == id) {
        return Ok(None);
    }
    let Some(manager) = position_under(doc, &all, at, id) else { return Ok(None) };
    let linked = link(doc, page, id, manager, above)?;
    let laid = lay_out(doc, page, &ChartRequest::default())?;
    Ok(Some(Dropped { manager, linked, laid }))
}

/// Moves `id` to a place that is past every shape of `siblings` along the flow and across it (in the layout's frame): the last of them in any order the layout reads.
fn set_last(doc: &mut Document, page: u32, id: u32, siblings: &[u32], flow: Direction) -> Result<(), Problem> {
    // Each shape's place comes from the page's one list of positions, read once now (the edits before this one moved shapes in the page's list): a search from the top of the page for each
    // of a manager's reports would be a walk of the page for each.
    let fresh = positions(doc, page)?;
    let at_of: HashMap<u32, &ShapeRef> = fresh.iter().map(|p| (p.id, &p.at)).collect();
    let reader = Reader::new(doc);
    let centre = |shape: u32| -> Option<(f64, f64)> {
        let corners = reader.corners(at_of.get(&shape)?);
        let (x, y) = corners.iter().fold((0.0, 0.0), |a, c| (a.0 + c.0 / 4.0, a.1 + c.1 / 4.0));
        (x.is_finite() && y.is_finite()).then_some((x, y))
    };
    let to_frame = |(x, y): (f64, f64)| match flow {
        Direction::TopToBottom => (x, -y),
        Direction::BottomToTop => (x, y),
        Direction::LeftToRight => (-y, x),
        Direction::RightToLeft => (-y, -x),
    };
    let to_page = |(c, m): (f64, f64)| match flow {
        Direction::TopToBottom => (c, -m),
        Direction::BottomToTop => (c, m),
        Direction::LeftToRight => (m, -c),
        Direction::RightToLeft => (-m, -c),
    };
    let Some(own) = centre(id) else { return Ok(()) };
    let (mut c, mut m) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for s in siblings {
        if let Some(p) = centre(*s) {
            let (sc, sm) = to_frame(p);
            c = c.max(sc);
            m = m.max(sm);
        }
    }
    if !(c.is_finite() && m.is_finite()) {
        return Ok(());
    }
    let (past_c, past_m) = (c + 1.0, m + 1.0);
    let want = to_page((past_c, past_m));
    let (dx, dy) = (want.0 - own.0, want.1 - own.1);
    if dx.abs() > 1.0e-9 || dy.abs() > 1.0e-9 {
        move_shapes(doc, page, &[id], dx, dy)?;
    }
    Ok(())
}
