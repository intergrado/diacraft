//! The cells of a page (the design's §1.9): the flat list of a page's `root` read into a [`Graph`] of [`Cell`]s with
//! their roles, depths, repaired parents and boxes on the page, by **one walk** that visits each cell once.
//!
//! A cell is any element child of the page's `root`: a plain `mxCell`, or a wrapper (`UserObject`, `object` or any other element) whose attributes are the cell's user
//! object and whose first `mxCell` child carries the rest. The list is flat, and each cell names its parent. This module makes it a tree, the way draw.io's own decoder repairs
//! a malformed one: a cell with no parent, a parent that is not in the page, a parent that is itself or in a cycle of parents, or a chain that is too deep, is moved to the
//! end of the first layer, in document order, and counted; a repeated id is kept under a new id of the reader's; a cell that no layer holds when the page has none is left out.
//!
//! **One linear walk** (the design's amendment of R24): the parents are followed once, by an explicit stack, each cell unvisited, on the path or done, never by a walk of each cell's
//! parents with a step cap, which is quadratic on a chain of a million. Every pass charges the cells it visits to the read's work before it runs ([`Unit::Cells`]); nothing recurses.
//!
//! **The passes**, each charged `n` cells but the lists (`2n`): the ids (the first cell with an id owns it; a repeat gets `<id>~2`, `<id>~3`, probing past ids the file has, once
//! for each string), the parents and the ends, the cycles (cut at each cell of a cycle), the root and the orphans, the levels from the root down (cutting a chain at the depth cap),
//! and the lists of children and the places. **A cell's place is the sum of its own and its ancestors'**, held within the cap at each step.

use std::collections::HashMap;

use diacraft_container::xml::{El, Node};

use crate::decode::warn as file_warn;
use crate::error::{DrawioError, shown};
use crate::geom::{Geometry, Pt, Rect, clamp_to, page_box, parse_px};
use crate::report::{Reading, Unit, page_case};

/// The class words of the warnings of the cells (the ones that are new say what they count).
pub mod warn {
    /// A cell whose parent is missing: moved to the first layer.
    pub const PARENT_MISSING: &str = "drawio_parent_missing";
    /// A cell that is left out: no layer holds it.
    pub const CELL_DROPPED: &str = "drawio_cell_dropped";
    /// A cell in a cycle of parents, cut.
    pub const PARENT_CYCLE: &str = "drawio_parent_cycle";
    /// A cell whose id an earlier cell has, given an id of the reader's.
    pub const DUPLICATE_ID: &str = "drawio_duplicate_id";
    /// A cell with both `vertex` and `edge`: a connector.
    pub const VERTEX_AND_EDGE: &str = "drawio_cell_vertex_and_edge";
    /// A wrapper and the cell in it that both have an id: the wrapper's wins.
    pub const WRAPPER_INNER_ID: &str = "drawio_wrapper_inner_id";
    /// A cell that hangs below the depth cap, moved to the first layer.
    pub const TOO_DEEP_TREE: &str = "drawio_too_deep_tree";
    /// A connector with more waypoints than the cap: the rest are dropped.
    pub const TOO_MANY_POINTS: &str = "drawio_too_many_points";
    /// A number that is no number, or past the cap, or a size below 0, repaired: one warning a page, with how many.
    pub const BAD_NUMBER: &str = "drawio_bad_number";
    /// An attribute that the table does not list, skipped: one warning a page, with the first names. **A class of this reader's; R19 asks for the count.**
    pub const UNKNOWN_ATTRIBUTE: &str = "drawio_unknown_attribute";
    /// A JSON attribute (`vars`, `backgroundImage`) that is not JSON of the shape it should be, not read. **A class of this reader's.**
    pub const ATTRIBUTE_UNREADABLE: &str = "drawio_attribute_unreadable";
    /// More of the file's `vars` than the model holds as properties: the rest are dropped.
    pub const TOO_MANY_PROPERTIES: &str = "drawio_too_many_properties";
}

/// A cell's place in the page's cell list: the index into [`Graph::cells`], from 0, in document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(u32);

impl CellId {
    /// The index into [`Graph::cells`].
    pub fn index(self) -> usize {
        self.0 as usize
    }

    fn at(i: usize) -> Option<CellId> {
        u32::try_from(i).ok().map(CellId)
    }
}

/// What a cell is, after the repairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The page's root cell: its own data, never drawn.
    Root,
    /// A cell whose parent is the root: a layer, whatever its flags say.
    Layer,
    /// `vertex="1"`, in a layer, a group or a container. It is a group or a container when it has [`Graph::children`].
    Shape,
    /// `edge="1"` (a cell with both flags too, with the warning `drawio_cell_vertex_and_edge`).
    Connector,
    /// A shape inside a connector with a relative geometry: the connector's label. Its place needs the connector's path.
    Label,
    /// A shape inside a shape (or a label or a port) with a relative geometry: "a relatively positioned, connectable child cell".
    Port,
    /// Neither vertex nor edge, below a layer: drawn as nothing, its children walked as if it were a group.
    Neither,
    /// Left out: no layer holds the cell, or the page has no root. It is in [`Graph::cells`] and in no list of children.
    Dropped,
}

/// What a connector's `source` or `target` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The cell names no end.
    None,
    /// The first cell with that id: the end is connected to it.
    Cell(CellId),
    /// The cell names an id that no cell of the page has.
    Missing,
}

/// A wrapper's attributes: the cell's user object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserObject {
    /// `tooltip`.
    pub tooltip: Option<String>,
    /// `link`.
    pub link: Option<String>,
    /// `linkTarget`.
    pub link_target: Option<String>,
    /// `tags`, as written.
    pub tags: Option<String>,
    /// `placeholders="1"`.
    pub placeholders: bool,
    /// Every other attribute, name and value, in the order written.
    pub props: Vec<(String, String)>,
}

/// One cell of a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The cell's id as the file has it. A repeated id is the reader's own (`renamed`), and a cell with no id has the empty text.
    pub id: String,
    /// The file's id was an earlier cell's, so this cell has a new one.
    pub renamed: bool,
    /// The parent after the repairs; `None` for the root and for a left-out cell.
    pub parent: Option<CellId>,
    /// What the cell is.
    pub role: Role,
    /// Levels below the root: the root 0, a layer 1, a cell in a layer 2. 0 for a left-out cell.
    pub depth: u32,
    /// `vertex="1"` as the file says.
    pub vertex: bool,
    /// `edge="1"` as the file says.
    pub edge: bool,
    /// `connectable`: `Some(false)` for `0`, `Some(true)` for `1`, else `None` (the default is for the shape's kind: shapes yes, connectors no).
    pub connectable: Option<bool>,
    /// `collapsed="1"`.
    pub collapsed: bool,
    /// `visible`: true unless it is `0`.
    pub visible: bool,
    /// The label: the wrapper's `label`, or else the cell's `value`. An empty label is the same as none, and is kept as the file has it.
    pub label: Option<String>,
    /// The `style` string, empty when the cell has none.
    pub style: String,
    /// A connector's `source`.
    pub source: Link,
    /// A connector's `target`.
    pub target: Link,
    /// A wrapper's user object.
    pub user: Option<Box<UserObject>>,
    /// The cell's geometry.
    pub geometry: Option<Box<Geometry>>,
    /// Where the origin its geometry is measured from lies on the page: its parent's origin, in px.
    pub frame: Pt,
    /// The cell's box on the page: a shape with an absolute geometry, a port, and a cell with no vertex or edge flag that has a geometry, as mxGraph gives it a state. `None` for
    /// the rest: a connector, a label (its place needs the connector's path), a layer, the root, a cell with no geometry.
    pub rect: Option<Rect>,
}

impl Cell {
    /// Where the origin of this cell's children lies on the page: its own top-left corner when it has a box, else the origin its own geometry is measured from.
    pub fn origin(&self) -> Pt {
        self.rect.map_or(self.frame, |r| r.origin())
    }

    /// A cell as the file states it, before the walk: nothing is repaired and nothing is placed.
    fn blank() -> Cell {
        Cell {
            id: String::new(),
            renamed: false,
            parent: None,
            role: Role::Dropped,
            depth: 0,
            vertex: false,
            edge: false,
            connectable: None,
            collapsed: false,
            visible: true,
            label: None,
            style: String::new(),
            source: Link::None,
            target: Link::None,
            user: None,
            geometry: None,
            frame: Pt::default(),
            rect: None,
        }
    }
}

/// The cells of one page, repaired into a tree.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Graph {
    /// Every cell, in document order.
    pub cells: Vec<Cell>,
    /// The page's root cell, if it has one.
    pub root: Option<CellId>,
    child_start: Vec<u32>,
    child_list: Vec<CellId>,
    ids: HashMap<String, CellId>,
}

impl Graph {
    /// The number of cells, the ones left out included.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the page has no cell.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The cell `id` names.
    pub fn cell(&self, id: CellId) -> Option<&Cell> {
        self.cells.get(id.index())
    }

    /// Every cell, in document order, as its id and the cell.
    pub fn iter(&self) -> impl Iterator<Item = (CellId, &Cell)> {
        self.cells.iter().enumerate().filter_map(|(i, c)| CellId::at(i).map(|id| (id, c)))
    }

    /// The children of a cell in drawing order: the file's order, then the cells moved into the first layer by and the depth cap, in document order. A later cell is drawn
    /// over the earlier ones.
    pub fn children(&self, id: CellId) -> &[CellId] {
        let i = id.index();
        let (Some(&from), Some(&to)) = (self.child_start.get(i), self.child_start.get(i + 1)) else { return &[] };
        self.child_list.get(from as usize..to as usize).unwrap_or(&[])
    }

    /// The layers, in order: the children of the root.
    pub fn layers(&self) -> &[CellId] {
        self.root.map_or(&[], |root| self.children(root))
    }

    /// The cell whose id is `id`: the first cell that has it as the file wrote it (references to a repeated id mean this one), or the cell that was given it.
    pub fn by_id(&self, id: &str) -> Option<CellId> {
        self.ids.get(id).copied()
    }

    /// `p`, a point in the coordinates of the cell's geometry (a waypoint, an end), as a point on the page: the cell's `frame` plus `p`.
    pub fn page_point(&self, id: CellId, p: Pt) -> Option<Pt> {
        let cell = self.cell(id)?;
        Some(Pt { x: cell.frame.x + p.x, y: cell.frame.y + p.y })
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ reading a page

/// A page being read: the read it belongs to, the page's name for the cases of its warnings, and the two warnings that count every cell of the page in one.
pub(crate) struct PageReading<'a, 'l> {
    /// The read.
    pub(crate) reading: &'a mut Reading<'l>,
    /// The page's name as the warnings name it, not escaped.
    pub(crate) label: String,
    unknown: Vec<String>,
    unknown_more: bool,
    unknown_any: bool,
    numbers: u64,
}

impl<'a, 'l> PageReading<'a, 'l> {
    pub(crate) fn new(reading: &'a mut Reading<'l>, label: String) -> Self {
        PageReading { reading, label, unknown: Vec::new(), unknown_more: false, unknown_any: false, numbers: 0 }
    }

    /// Counts an attribute that is not listed: its name goes in the case of the page's warning when there is room (`MAX_UNKNOWN_NAMES`), each once.
    pub(crate) fn unknown_attr(&mut self, name: &str) {
        self.unknown_any = true;
        if self.unknown.iter().any(|known| known == name) {
            return;
        }
        if self.unknown.len() < crate::limits::MAX_UNKNOWN_NAMES {
            self.unknown.push(name.chars().take(24).collect());
        } else {
            self.unknown_more = true;
        }
    }

    /// Counts an element that is not read, by its name: a case of its own, and no page named.
    pub(crate) fn unknown_element(&mut self, name: &str) {
        self.reading.note(file_warn::UNKNOWN_ELEMENT, name);
    }

    /// Counts a number that was repaired.
    pub(crate) fn bad_number(&mut self) {
        self.numbers = self.numbers.saturating_add(1);
    }

    /// Notes a warning of `class` with the case `<page>: <subject>`.
    pub(crate) fn note(&mut self, class: &'static str, subject: &str) {
        let label = &self.label;
        self.reading.note_lazy(class, || page_case(label, subject));
    }

    /// `text` as a number of the file: the value of a decimal number within the cap, or 0 for anything else, each repair counted.
    pub(crate) fn number(&mut self, text: &str) -> f64 {
        let limits = self.reading.limits;
        match parse_px(text, limits.max_number_bytes) {
            Some(v) => {
                let held = clamp_to(v, limits.max_coord_px);
                if held != v {
                    self.bad_number();
                }
                held
            }
            None => {
                self.bad_number();
                0.0
            }
        }
    }

    /// [`number`](PageReading::number) for a width or a height: below 0 is 0, counted.
    pub(crate) fn size(&mut self, text: &str) -> f64 {
        let v = self.number(text);
        if v < 0.0 {
            self.bad_number();
            0.0
        } else {
            v
        }
    }

    /// Ends the page: the two warnings that it counted in one each.
    pub(crate) fn finish(self) {
        let PageReading { reading, label, unknown, unknown_more, unknown_any, numbers } = self;
        if unknown_any {
            let mut names = unknown.join(", ");
            if unknown_more {
                names.push_str(", \u{2026}");
            }
            reading.note_lazy(warn::UNKNOWN_ATTRIBUTE, || page_case(&label, &names));
        }
        if numbers > 0 {
            let words = if numbers == 1 { "1 number".to_owned() } else { format!("{numbers} numbers") };
            reading.note_lazy(warn::BAD_NUMBER, || page_case(&label, &words));
        }
    }
}

/// What a cell names by id, held until the ids are known.
#[derive(Default)]
struct Refs {
    parent: Option<String>,
    source: Option<String>,
    target: Option<String>,
}

/// A wrapper's attributes: its id and label apart from the rest of the user object.
#[derive(Default)]
struct Wrapper {
    id: Option<String>,
    label: Option<String>,
    user: UserObject,
}

/// The attributes of a wrapper: `id` and `label` are the cell's, `tooltip`, `link`, `linkTarget`, `tags` and `placeholders` are the user object's own, every other one is a
/// property, in the order written.
fn wrapper_of(attrs: Vec<(String, String)>) -> Wrapper {
    let mut w = Wrapper::default();
    for (k, v) in attrs {
        match k.as_str() {
            "id" => w.id = Some(v),
            "label" => w.label = Some(v),
            "tooltip" => w.user.tooltip = Some(v),
            "link" => w.user.link = Some(v),
            "linkTarget" => w.user.link_target = Some(v),
            "tags" => w.user.tags = Some(v),
            "placeholders" => w.user.placeholders = v == "1",
            _ => w.user.props.push((k, v)),
        }
    }
    w
}

/// The cell of one element child of `root`: a plain `mxCell`, or a wrapper and the first `mxCell` in it.
fn read_cell(el: El, pr: &mut PageReading<'_, '_>) -> Result<(Cell, Refs), DrawioError> {
    pr.reading.charge(Unit::TreeNodes, (el.attrs.len() + el.children.len()) as u64)?;
    let El { name, attrs, children } = el;
    if name == "mxCell" {
        return inner_cell(attrs, children, None, pr);
    }
    let wrapper = wrapper_of(attrs);
    let mut inner: Option<El> = None;
    for node in children {
        let Node::El(child) = node else { continue };
        if child.name == "mxCell" && inner.is_none() {
            inner = Some(child);
        } else {
            pr.unknown_element(&child.name);
        }
    }
    match inner {
        Some(inner) => {
            pr.reading.charge(Unit::TreeNodes, (inner.attrs.len() + inner.children.len()) as u64)?;
            inner_cell(inner.attrs, inner.children, Some(wrapper), pr)
        }
        // A wrapper with no cell in it: a cell with no parent, no style and no geometry.
        None => inner_cell(Vec::new(), Vec::new(), Some(wrapper), pr),
    }
}

/// The cell that the attributes and the children of an `mxCell` make, in the wrapper it is in, if it is in one.
fn inner_cell(
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    wrapper: Option<Wrapper>,
    pr: &mut PageReading<'_, '_>,
) -> Result<(Cell, Refs), DrawioError> {
    let mut cell = Cell::blank();
    let mut refs = Refs::default();
    let (mut inner_id, mut value) = (None, None);
    for (k, v) in attrs {
        match k.as_str() {
            "id" => inner_id = Some(v),
            "parent" => refs.parent = Some(v),
            "value" => value = Some(v),
            "style" => cell.style = v,
            "vertex" => cell.vertex = v == "1",
            "edge" => cell.edge = v == "1",
            "source" => refs.source = Some(v),
            "target" => refs.target = Some(v),
            "connectable" => cell.connectable = flag(&v),
            "collapsed" => cell.collapsed = v == "1",
            "visible" => cell.visible = v != "0",
            other => pr.unknown_attr(other),
        }
    }
    let (wrapper_id, label, user) = match wrapper {
        Some(Wrapper { id, label, user }) => (id, label, Some(Box::new(user))),
        None => (None, None, None),
    };
    cell.id = match (wrapper_id, inner_id) {
        (Some(w), Some(_)) => {
            pr.note(warn::WRAPPER_INNER_ID, &w);
            w
        }
        (Some(id), None) | (None, Some(id)) => id,
        (None, None) => String::new(),
    };
    cell.label = label.or(value);
    cell.user = user;
    cell.geometry = read_children(children, &cell.id, pr)?;
    Ok((cell, refs))
}

/// `0` is no and `1` is yes; anything else says nothing.
fn flag(v: &str) -> Option<bool> {
    match v {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// The geometry among the children of a cell: the first `mxGeometry`; anything else is an element that is not read.
fn read_children(children: Vec<Node>, id: &str, pr: &mut PageReading<'_, '_>) -> Result<Option<Box<Geometry>>, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, children.len() as u64)?;
    let mut geometry = None;
    for node in children {
        let Node::El(child) = node else { continue };
        if child.name == "mxGeometry" && geometry.is_none() {
            geometry = Some(Box::new(read_geometry(child, id, pr)?));
        } else {
            pr.unknown_element(&child.name);
        }
    }
    Ok(geometry)
}

/// A point: `x` and `y`, a missing one 0.
fn read_point(el: El, pr: &mut PageReading<'_, '_>) -> Result<Pt, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, el.attrs.len() as u64)?;
    let mut p = Pt::default();
    for (k, v) in el.attrs {
        match k.as_str() {
            "x" => p.x = pr.number(&v),
            "y" => p.y = pr.number(&v),
            "as" => {}
            other => pr.unknown_attr(other),
        }
    }
    Ok(p)
}

/// The box `mxRectangle` gives (`alternateBounds`): the corner and the size, the size not below 0.
fn read_rectangle(el: El, pr: &mut PageReading<'_, '_>) -> Result<Rect, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, el.attrs.len() as u64)?;
    let mut r = Rect::default();
    for (k, v) in el.attrs {
        match k.as_str() {
            "x" => r.x = pr.number(&v),
            "y" => r.y = pr.number(&v),
            "width" => r.width = pr.size(&v),
            "height" => r.height = pr.size(&v),
            "as" => {}
            other => pr.unknown_attr(other),
        }
    }
    Ok(r)
}

/// The waypoints of an `Array as="points"`: at most `max_points`, in order; the rest are counted and not read.
fn read_points(el: El, id: &str, pr: &mut PageReading<'_, '_>) -> Result<Vec<Pt>, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, (el.attrs.len() + el.children.len()) as u64)?;
    let max = pr.reading.limits.max_points;
    let mut points = Vec::new();
    let mut dropped = false;
    for node in el.children {
        let Node::El(child) = node else { continue };
        if child.name != "mxPoint" {
            pr.unknown_element(&child.name);
        } else if points.len() < max {
            points.push(read_point(child, pr)?);
        } else {
            dropped = true;
        }
    }
    if dropped {
        pr.note(warn::TOO_MANY_POINTS, id);
    }
    Ok(points)
}

/// What an element inside an `mxGeometry` is, by its name and its `as`, told without copying either.
#[derive(Clone, Copy)]
enum Part {
    Source,
    Target,
    Offset,
    Points,
    Alternate,
    Unknown,
}

fn part_of(child: &El) -> Part {
    match (child.name.as_str(), child.attr("as")) {
        ("mxPoint", Some("sourcePoint")) => Part::Source,
        ("mxPoint", Some("targetPoint")) => Part::Target,
        ("mxPoint", Some("offset")) => Part::Offset,
        ("Array", Some("points")) => Part::Points,
        ("mxRectangle", Some("alternateBounds")) => Part::Alternate,
        _ => Part::Unknown,
    }
}

/// What an `mxGeometry` element holds, with the repairs.
fn read_geometry(el: El, id: &str, pr: &mut PageReading<'_, '_>) -> Result<Geometry, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, (el.attrs.len() + el.children.len()) as u64)?;
    let mut g = Geometry::default();
    for (k, v) in el.attrs {
        match k.as_str() {
            "x" => g.x = pr.number(&v),
            "y" => g.y = pr.number(&v),
            "width" => g.width = pr.size(&v),
            "height" => g.height = pr.size(&v),
            "relative" => g.relative = v == "1",
            "as" => {}
            other => pr.unknown_attr(other),
        }
    }
    let mut points_seen = false;
    for node in el.children {
        let Node::El(child) = node else { continue };
        match part_of(&child) {
            Part::Source if g.source_point.is_none() => g.source_point = Some(read_point(child, pr)?),
            Part::Target if g.target_point.is_none() => g.target_point = Some(read_point(child, pr)?),
            Part::Offset => g.offset = read_point(child, pr)?,
            Part::Points if !points_seen => {
                points_seen = true;
                g.points = read_points(child, id, pr)?;
            }
            Part::Alternate if g.alternate_bounds.is_none() => g.alternate_bounds = Some(read_rectangle(child, pr)?),
            _ => pr.unknown_element(&child.name),
        }
    }
    Ok(g)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the one walk

/// No cell: the end of a list, and a cell with no parent.
const NONE: u32 = u32::MAX;

/// How a cell came to be moved or left out: what the final pass counts.
const NORMAL: u8 = 0;
const MISSING: u8 = 1;
const CYCLE: u8 = 2;
const DEEP: u8 = 3;

/// `i` as a cell index that fits the lists, or the end of a list when it does not (the cap of cells keeps every index below it).
fn at(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(NONE)
}

/// The forest of a page: who is whose parent after the ids are known, and what the walk found.
struct Forest {
    /// The parent of each cell: its index, or [`NONE`] for the root, the orphans and the cells of cycles (cut).
    parent: Vec<u32>,
    /// How each cell came to be an orphan or moved ([`MISSING`], [`CYCLE`], [`DEEP`]).
    how: Vec<u8>,
    /// The first child of each cell and the next sibling of each cell, in document order, of the parents after the cuts.
    first: Vec<u32>,
    next: Vec<u32>,
    /// The level of each cell below its layer (a layer 0), or [`NONE`] for a cell the walk did not reach.
    level: Vec<u32>,
    /// The cells reached, each after its parent.
    order: Vec<u32>,
}

impl Forest {
    fn new(parent: Vec<u32>) -> Forest {
        let n = parent.len();
        Forest { parent, how: vec![NORMAL; n], first: vec![NONE; n], next: vec![NONE; n], level: vec![NONE; n], order: Vec::new() }
    }

    fn parent_of(&self, i: usize) -> Option<usize> {
        self.parent.get(i).copied().filter(|p| *p != NONE).map(|p| p as usize)
    }

    /// Cuts every cycle of parents, once: each cell of a cycle loses its parent and is marked [`CYCLE`]. Each cell is on the path of one walk at most, so the work is `n`.
    fn cut_cycles(&mut self) {
        let n = self.parent.len();
        let mut state = vec![0u8; n];
        let mut path: Vec<usize> = Vec::new();
        for start in 0..n {
            if state.get(start) != Some(&0) {
                continue;
            }
            path.clear();
            let mut cur = start;
            loop {
                match state.get(cur).copied() {
                    Some(0) => {
                        if let Some(s) = state.get_mut(cur) {
                            *s = 1;
                        }
                        path.push(cur);
                        match self.parent_of(cur) {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                    Some(1) => {
                        let from = path.iter().rposition(|m| *m == cur).unwrap_or(0);
                        for &m in path.get(from..).unwrap_or(&[]) {
                            if let Some(p) = self.parent.get_mut(m) {
                                *p = NONE;
                            }
                            if let Some(h) = self.how.get_mut(m) {
                                *h = CYCLE;
                            }
                        }
                        break;
                    }
                    _ => break,
                }
            }
            for &m in &path {
                if let Some(s) = state.get_mut(m) {
                    *s = 2;
                }
            }
        }
    }

    /// The root: of the cells with no parent, the one whose id is `0`, else the first that is neither a vertex nor an edge, else the first.
    fn pick_root(&self, cells: &[Cell]) -> Option<usize> {
        let (mut zero, mut plain, mut first) = (None, None, None);
        for (i, c) in cells.iter().enumerate() {
            if self.parent_of(i).is_some() {
                continue;
            }
            first.get_or_insert(i);
            if zero.is_none() && c.id == "0" && !c.renamed {
                zero = Some(i);
            }
            if plain.is_none() && !c.vertex && !c.edge {
                plain = Some(i);
            }
        }
        zero.or(plain).or(first)
    }

    /// The lists of first child and next sibling, in document order, of the parents as they are: one pass from the last cell to the first.
    fn link_children(&mut self) {
        for i in (0..self.parent.len()).rev() {
            let Some(p) = self.parent_of(i) else { continue };
            let head = self.first.get(p).copied().unwrap_or(NONE);
            if let Some(n) = self.next.get_mut(i) {
                *n = head;
            }
            if let Some(f) = self.first.get_mut(p) {
                *f = at(i);
            }
        }
    }

    /// The walk from the root down, each cell once, by an explicit stack: the level of each cell below its layer, and the cells whose level passes `cap`, which begin again at level 1
    /// (they are moved to the first layer). The orphans, when there is a layer to take them, are walked from level 1. A cell never reached is left out.
    fn walk(&mut self, root: usize, orphans: &[usize], cap: u32) {
        let mut stack: Vec<usize> = Vec::new();
        for &o in orphans {
            if let Some(l) = self.level.get_mut(o) {
                *l = 1;
            }
            stack.push(o);
        }
        if let Some(l) = self.level.get_mut(root) {
            *l = 0;
        }
        stack.push(root);
        while let Some(i) = stack.pop() {
            self.order.push(at(i));
            let level = self.level.get(i).copied().unwrap_or(NONE);
            let mut c = self.first.get(i).copied().unwrap_or(NONE);
            while c != NONE {
                let ci = c as usize;
                let mut l = if i == root { 0 } else { level.saturating_add(1) };
                if l > cap {
                    l = 1;
                    if let Some(h) = self.how.get_mut(ci) {
                        *h = DEEP;
                    }
                }
                if let Some(slot) = self.level.get_mut(ci) {
                    *slot = l;
                }
                stack.push(ci);
                c = self.next.get(ci).copied().unwrap_or(NONE);
            }
        }
    }

    /// Whether the walk reached the cell.
    fn reached(&self, i: usize) -> bool {
        self.level.get(i).is_some_and(|l| *l != NONE)
    }

    /// Whether the cell was moved to the first layer: an orphan, a cell of a cycle, or a cell below the cap.
    fn moved(&self, i: usize) -> bool {
        self.how.get(i).is_some_and(|h| *h != NORMAL)
    }

    /// The parent of a cell after the moves: the first layer for a moved cell, else the one it names.
    fn final_parent(&self, i: usize, layer: Option<usize>) -> Option<usize> {
        if self.moved(i) { layer } else { self.parent_of(i) }
    }
}

/// The ids of the cells: the first cell with an id owns it, and the later cells with it, in document order, are returned to be given new ones.
fn index_ids(cells: &[Cell]) -> (HashMap<String, CellId>, Vec<usize>) {
    let mut ids: HashMap<String, CellId> = HashMap::new();
    let mut repeated = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        if c.id.is_empty() {
            continue;
        }
        match (ids.contains_key(&c.id), CellId::at(i)) {
            (true, _) => repeated.push(i),
            (false, Some(id)) => {
                ids.insert(c.id.clone(), id);
            }
            (false, None) => {}
        }
    }
    (ids, repeated)
}

/// Gives each repeated id a new one of the reader's, `<id>~2`, `<id>~3` and so on, probing past the ids that are taken. The counter of an id goes on from where it was, and every
/// string `<id>~<n>` has one id and one `n`, so no id of the page is probed over twice: the work is the cells'.
fn rename_repeats(cells: &mut [Cell], repeated: &[usize], ids: &mut HashMap<String, CellId>, pr: &mut PageReading<'_, '_>) {
    let mut counters: HashMap<String, usize> = HashMap::new();
    for &i in repeated {
        let (Some(cell), Some(at)) = (cells.get_mut(i), CellId::at(i)) else { continue };
        let base = std::mem::take(&mut cell.id);
        let mut n = counters.get(&base).copied().unwrap_or(2);
        let renamed = loop {
            let candidate = format!("{base}~{n}");
            n += 1;
            if !ids.contains_key(&candidate) {
                break candidate;
            }
        };
        ids.insert(renamed.clone(), at);
        cell.id = renamed;
        cell.renamed = true;
        pr.note(warn::DUPLICATE_ID, &base);
        counters.insert(base, n);
    }
}

/// An end of a connector: the first cell with that id, or none named, or one that is not in the page.
fn link_of(named: Option<&str>, ids: &HashMap<String, CellId>) -> Link {
    match named {
        None | Some("") => Link::None,
        Some(id) => ids.get(id).map_or(Link::Missing, |c| Link::Cell(*c)),
    }
}

/// The role of a cell below a layer, from its flags, its parent and whether its geometry is relative.
fn role_of(cell: &Cell, parent: Role, relative: bool) -> Role {
    if cell.edge {
        Role::Connector
    } else if cell.vertex {
        match parent {
            Role::Connector if relative => Role::Label,
            Role::Shape | Role::Port | Role::Label if relative => Role::Port,
            _ => Role::Shape,
        }
    } else {
        Role::Neither
    }
}

/// Counts what happened to each cell, in document order, so that the cases of a warning are in the order of the file.
fn count_repairs(cells: &[Cell], forest: &Forest, pr: &mut PageReading<'_, '_>) {
    for (i, c) in cells.iter().enumerate() {
        if c.role == Role::Dropped {
            pr.note(warn::CELL_DROPPED, &c.id);
        }
        match forest.how.get(i).copied().unwrap_or(NORMAL) {
            CYCLE => pr.note(warn::PARENT_CYCLE, &c.id),
            MISSING if c.role != Role::Dropped => pr.note(warn::PARENT_MISSING, &c.id),
            DEEP => pr.note(warn::TOO_DEEP_TREE, &c.id),
            _ => {}
        }
        if c.vertex && c.edge && c.role == Role::Connector {
            pr.note(warn::VERTEX_AND_EDGE, &c.id);
        }
    }
}

/// Where a cell's parent puts it: the parent's role, the origin its children are measured from and the size of its box.
struct Parent {
    role: Role,
    origin: Pt,
    size: (f64, f64),
}

/// The parent of cell `i` as the places need it: a moved cell hangs from the first layer, whose origin is the page's and which has no box, wherever it was walked.
fn parent_of_place(cells: &[Cell], i: usize, forest: &Forest) -> Parent {
    let layer_like = Parent { role: Role::Layer, origin: Pt::default(), size: (0.0, 0.0) };
    if forest.moved(i) {
        return layer_like;
    }
    let Some(p) = forest.parent_of(i).and_then(|p| cells.get(p)) else { return layer_like };
    Parent { role: p.role, origin: p.origin(), size: p.rect.map_or((0.0, 0.0), |r| (r.width, r.height)) }
}

/// Gives every cell the walk reached its role, parent, depth, frame and box, parents first.
fn place(cells: &mut [Cell], forest: &Forest, root: usize, layer: Option<usize>, pr: &mut PageReading<'_, '_>) {
    let max = pr.reading.limits.max_coord_px;
    for &iu in &forest.order {
        let i = iu as usize;
        if i == root {
            if let Some(c) = cells.get_mut(i) {
                c.role = Role::Root;
            }
            continue;
        }
        let parent = parent_of_place(cells, i, forest);
        let final_parent = forest.final_parent(i, layer).and_then(CellId::at);
        let level = forest.level.get(i).copied().unwrap_or(0);
        let Some(c) = cells.get_mut(i) else { continue };
        let relative = c.geometry.as_deref().is_some_and(|g| g.relative);
        c.parent = final_parent;
        c.depth = level.saturating_add(1);
        c.frame = parent.origin;
        c.role = if final_parent.map(CellId::index) == Some(root) { Role::Layer } else { role_of(c, parent.role, relative) };
        if matches!(c.role, Role::Shape | Role::Port | Role::Neither)
            && let Some(g) = c.geometry.as_deref()
        {
            let (rect, held) = page_box(c.frame, parent.size, g, max);
            c.rect = Some(rect);
            if held {
                pr.bad_number();
            }
        }
    }
}

/// The lists of children after the moves, as one array and the start of each list: a cell's own children in document order, then the cells that were moved to it, in document
/// order. Two passes over the cells.
fn child_lists(cells: &[Cell], forest: &Forest, root: usize, layer: Option<usize>) -> (Vec<u32>, Vec<CellId>) {
    let n = cells.len();
    let mut start = vec![0u32; n + 1];
    let kids = |i: usize| i != root && forest.reached(i);
    for i in (0..n).filter(|i| kids(*i)) {
        if let Some(p) = forest.final_parent(i, layer)
            && let Some(s) = start.get_mut(p + 1)
        {
            *s += 1;
        }
    }
    for i in 0..n {
        let before = start.get(i).copied().unwrap_or(0);
        if let Some(s) = start.get_mut(i + 1) {
            *s += before;
        }
    }
    let mut fill: Vec<u32> = start.clone();
    let mut list = vec![CellId(0); start.get(n).copied().unwrap_or(0) as usize];
    for pass_moved in [false, true] {
        for i in (0..n).filter(|i| kids(*i) && forest.moved(*i) == pass_moved) {
            let (Some(p), Some(id)) = (forest.final_parent(i, layer), CellId::at(i)) else { continue };
            let Some(at) = fill.get_mut(p) else { continue };
            if let Some(slot) = list.get_mut(*at as usize) {
                *slot = id;
            }
            *at += 1;
        }
    }
    (start, list)
}

/// Makes the list of cells a [`Graph`]: the ids, the parents and ends, the cycles, the root and the orphans, the levels, the lists and the places.
fn link(mut cells: Vec<Cell>, refs: &[Refs], pr: &mut PageReading<'_, '_>) -> Result<Graph, DrawioError> {
    let n = cells.len();
    let limits = pr.reading.limits;
    // Pass 1: the ids, the parents and the ends.
    pr.reading.charge(Unit::Cells, n as u64)?;
    let (mut ids, repeated) = index_ids(&cells);
    let mut parent = Vec::with_capacity(n);
    for (cell, r) in cells.iter_mut().zip(refs) {
        parent.push(r.parent.as_deref().and_then(|p| ids.get(p)).map_or(NONE, |c| at(c.index())));
        cell.source = link_of(r.source.as_deref(), &ids);
        cell.target = link_of(r.target.as_deref(), &ids);
    }
    rename_repeats(&mut cells, &repeated, &mut ids, pr);
    // Pass 2: the cycles, the root and the orphans.
    pr.reading.charge(Unit::Cells, n as u64)?;
    let mut forest = Forest::new(parent);
    forest.cut_cycles();
    let Some(root) = forest.pick_root(&cells) else { return Ok(Graph { cells, ..Graph::default() }) };
    let orphans: Vec<usize> = (0..n).filter(|i| *i != root && forest.parent_of(*i).is_none()).collect();
    for &o in &orphans {
        if let Some(h) = forest.how.get_mut(o).filter(|h| **h != CYCLE) {
            *h = MISSING;
        }
    }
    forest.link_children();
    let layer = forest.first.get(root).copied().filter(|f| *f != NONE).map(|f| f as usize);
    // Pass 3: the levels, from the root down.
    pr.reading.charge(Unit::Cells, n as u64)?;
    let taken: &[usize] = if layer.is_some() { &orphans } else { &[] };
    let cap = at(limits.max_cell_depth.max(1));
    forest.walk(root, taken, cap);
    // Pass 4: the lists and the places.
    pr.reading.charge(Unit::Cells, (n as u64).saturating_mul(2))?;
    for c in &mut cells {
        c.source = reached_end(c.source, &forest);
        c.target = reached_end(c.target, &forest);
    }
    place(&mut cells, &forest, root, layer, pr);
    let (child_start, child_list) = child_lists(&cells, &forest, root, layer);
    count_repairs(&cells, &forest, pr);
    Ok(Graph { cells, root: CellId::at(root), child_start, child_list, ids })
}

/// An end of a connector that names a cell the walk left out is an end that names none that is there.
fn reached_end(end: Link, forest: &Forest) -> Link {
    match end {
        Link::Cell(to) if !forest.reached(to.index()) => Link::Missing,
        other => other,
    }
}

/// Reads the cells of a page: `root`, the element that holds them, into a [`Graph`], each cell once and every number repaired and counted. The cap of the page's cells is held before
/// any cell is read.
pub(crate) fn build(root: El, pr: &mut PageReading<'_, '_>) -> Result<Graph, DrawioError> {
    let limits = pr.reading.limits;
    pr.reading.charge(Unit::TreeNodes, root.children.len() as u64)?;
    let count = root.elements().count();
    let cap = limits.max_shapes_per_page.min(usize::try_from(NONE).unwrap_or(usize::MAX).saturating_sub(1));
    if count > cap {
        return Err(DrawioError::too_complex(format!(
            "The page \"{}\" has {count} cells, more than the {cap} that DiaCraft reads.",
            shown(&pr.label, crate::report::NAME_CHARS)
        )));
    }
    let (mut cells, mut refs) = (Vec::new(), Vec::new());
    if cells.try_reserve_exact(count).is_err() || refs.try_reserve_exact(count).is_err() {
        return Err(DrawioError::out_of_memory());
    }
    for node in root.children {
        if let Node::El(el) = node {
            let (cell, r) = read_cell(el, pr)?;
            cells.push(cell);
            refs.push(r);
        }
    }
    link(cells, &refs, pr)
}
