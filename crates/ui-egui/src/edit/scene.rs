//! The page as the pointer sees it: each shape's box, outline and connector ends, read from the model.
//!
//! **Hit-testing never looks at the picture.** The render worker's picture is a stale image while a drag goes on; what a click
//! meets is the shape's frame and geometry rows, read here through the same resolver the engine's commands use (so a shape that
//! inherits its geometry from a master is found where its instance is). Everything is in paper inches with y up, the unit of the
//! window's view and of the editing commands: the page's drawing units are divided by `units_per_inch`.
//!
//! **A cell that holds a formula and no value saved anywhere on the way to it is read by what its formula works out to**, as the picture draws it (`diacraft_engine::draw::Worked`):
//! a shape pinned at `=1+1` with no `V` is drawn at 2 inches, and is found there. The steps for that are the scene's own, one budget for all it reads and for the
//! connection points it reads later ([`Scene::points`]), and the answers are kept in the reader the build makes, so the work is paid once for the revision the scene is built at and
//! never again by a click, a rubber band or a frame of a drag; a page whose formulas run the steps out is read as saved from there on (no value, which reads 0), as it was before.
//!
//! A scene is built when the pointer first needs it after the document changed, and kept until the next edit (`edit/mod.rs`).
//! The page's own top-level shapes are read, up to [`MAX_ITEMS`]; they are what a click and a rubber band meet, and a group is one thing to
//! select. **The members of the groups are read too**, up to [`MAX_MEMBERS`] in all and [`MAX_GROUP_DEPTH`] levels down, and kept apart in
//! [`Scene::members`]: a second click on a selected group selects the member under the pointer (`gesture.rs`). Their places are on the page,
//! carried up through the poses of the groups they are in. **A group whose `SelectMode` is 0 (group only) has no members here**, nor have the groups
//! inside it (a shape cannot be selected under an ancestor with 0).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use diacraft_engine::doc::{CellAddr, Document, Owner, Resolver, SectionKey, Shape, ShapeRef, ShapeType, SheetRef, Value};
use diacraft_engine::draw::layers::{Standing, page_standing};
use diacraft_engine::draw::worked::{WORKED_STEPS, Worked, WorkedBudget};
use diacraft_engine::draw::{Pose, connection_points_within, text_block_in_frame, units_per_inch};
use diacraft_engine::snap::MAX_CONNECTION_ROWS;
use diacraft_geom::SectionFlags;

use super::geom::{self, Basis, P, bounds_of, box_inside, distance, inside_polygon, near_polyline};

/// The most top-level shapes of a page the pointer reads. A page with more is left to the keyboard and the commands, and the window
/// says so: reading ten thousand shapes at every edit would stall a frame.
pub const MAX_ITEMS: usize = 5_000;

/// The most members of groups a page's scene reads, and how many levels of groups down it goes: a page of huge groups is left to the
/// commands, and a group inside a group inside a group is no deeper than a person goes with the mouse.
pub const MAX_MEMBERS: usize = 5_000;
pub const MAX_GROUP_DEPTH: usize = 8;

/// The most text blocks one item holds (a group holds those of its members), and the most members of groups the scene looks into **for their text blocks alone** (the members of a group set to
/// group only are not read as items, and its labels are still met): a hostile group is met at the labels read and no further.
pub const MAX_BLOCKS: usize = 256;
const MAX_BLOCK_VISITS: usize = 20_000;

/// The most `Geometry` sections, rows of a section and points of a shape read for its outline: [`MAX_SECTIONS`] that are shown, out of the first [`MAX_LOOKED_AT`] there are (a section that is
/// hidden costs three cells to find out, so a shape of thousands of them is not read to the end).
const MAX_SECTIONS: usize = 8;
const MAX_LOOKED_AT: usize = 64;
const MAX_ROWS: usize = 400;
const MAX_POINTS: usize = 2_000;

/// The most `Connect` rows looked at.
pub(crate) const MAX_CONNECTS: usize = 200_000;

/// How many straight pieces an ellipse is cut into for hit-testing and for the preview.
const ELLIPSE_SEGMENTS: usize = 48;

/// `FromPart` of the `Connect` row of a connector's begin and end and the `ToPart` of the first connection point.
const FROM_BEGIN: u32 = 9;
const FROM_END: u32 = 12;
const TO_PART_BASE: u32 = 100;

/// What a shape is to the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A shape with two dimensions: eight handles when it is not turned.
    Shape,
    /// A group: moved, resized, turned and deleted as one shape.
    Group,
    /// A one-dimensional shape with a begin and an end.
    Connector,
}

/// One top-level shape of the page.
#[derive(Clone, Debug)]
pub struct Item {
    pub id: u32,
    pub kind: Kind,
    /// The corners of the shape's box after its turn and flips, going round it from the lower left of its own frame.
    pub corners: [P; 4],
    /// The box before the turn: lower left and upper right. The size the status bar shows (a handle drag works in [`Item::basis`]).
    pub frame: (P, P),
    /// Whether the shape is turned or flipped.
    pub turned: bool,
    /// Whether a group the shape is in is turned or flipped: the engine refuses to resize, turn or flip the member of one.
    pub in_turned_group: bool,
    /// The pin, on the page: the point a turn is about.
    pub pin: P,
    /// The group the shape is a member of, if it is.
    pub parent: Option<u32>,
    /// What a click on a group picks, resolved for a group (`SelectMode`): 0 the group only (its members are not read at all, and
    /// not those of the groups inside it), 1 the group first and a second click the member, 2 the members first. A shape that is not a group
    /// reads 1. A cell that is absent reads 1 as well (the root style holds 1 in every file; MS-VSDX's own default of 0 would make
    /// every member unselectable).
    pub select_mode: u8,
    /// How far it is turned, in degrees counterclockwise.
    pub degrees: f64,
    /// The paths of the shape's geometry that its sections show: closed outlines and open lines, as points. A section that is hidden (`NoShow`) is no part of it. Empty for a shape with none.
    pub outline: Vec<Vec<P>>,
    /// For each path of [`Item::outline`], whether the page paints anything of it (the same length). **A path is painted** when its section has a line that is drawn, or a fill that
    /// is: `NoLine` and `NoFill` of the section, and the shape's `LinePattern` and `FillPattern`, as the renderer reads them (`diacraft_geom::SectionFlags::paint`), and the shape itself
    /// is not drawn when its `DisplayMode` is 0. A text box of a group (a rectangle with no line and no fill) has an outline for the pointer to meet and paints nothing.
    pub painted: Vec<bool>,
    /// Whether every one of the shape's `Geometry` sections is hidden (`NoShow`), and the shape is not a group and shows no text: nothing of it is seen, and a click passes over it
    /// (a hit test ignores hidden geometry). A hidden marker of a group's member is such a shape. A group is picked by its box whatever its own geometry says, and **a shape that
    /// shows text is picked in its frame, the text block, as a shape with no geometry is**: its text is there to be clicked.
    pub hidden: bool,
    /// The text blocks of the shape on the page, in inches, each as its four corners in the order of the block's own frame (`text_block_in_frame`): the shape's own when it shows text, and for
    /// a group those of its members at every depth that show text, whether the members are read as items or not (a group set to group only is met at its labels too). At most
    /// [`MAX_BLOCKS`]. **A click in a text block meets the shape wherever the block lies** ([`Item::hit_text`]): a label that sits under its shape is not part of the geometry.
    pub text_blocks: Vec<[P; 4]>,
    /// For a connector: where it begins and ends.
    pub ends: Option<(P, P)>,
    /// For a connector: the (shape, connection point) each end is glued to, begin first.
    pub glue: [Option<(u32, u32)>; 2],
    /// The smallest box that holds the corners and the outline.
    pub bounds: (P, P),
}

// How many shapes `Item::hit` was asked about, for the test that holds a hit test to a count. Not kept outside tests.
#[cfg(test)]
thread_local! {
    static HIT_ASKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Item {
    /// The paths of the outline that the page paints something of, as the renderer reads the sections ([`Item::painted`]): what a picture of the shape is drawn from. A shape that
    /// paints nothing (a text box with no line and no fill) has none, and a picture that wants something to show draws its box.
    pub fn painted_paths(&self) -> impl Iterator<Item = &Vec<P>> {
        self.outline.iter().zip(&self.painted).filter(|(_, painted)| **painted).map(|(path, _)| path)
    }

    /// The smallest box that holds the paths the page paints ([`Item::painted_paths`]), or `None` when it paints none.
    pub fn painted_bounds(&self) -> Option<(P, P)> {
        bounds_of(self.painted_paths().flatten())
    }

    /// Whether `p` is on the shape, with `tol` inches of reach round a line. A shape is hit inside its outline (or its box, when it has none
    /// that encloses anything); a connector only along its path. **A shape whose geometry is all hidden is never hit** ([`Item::hidden`]).
    pub fn hit(&self, p: P, tol: f64) -> bool {
        #[cfg(test)]
        HIT_ASKS.with(|c| c.set(c.get() + 1));
        if self.hidden {
            return false;
        }
        let (lo, hi) = self.bounds;
        if p.0 < lo.0 - tol || p.0 > hi.0 + tol || p.1 < lo.1 - tol || p.1 > hi.1 + tol {
            return false;
        }
        if self.kind == Kind::Connector {
            return self.outline.iter().any(|line| near_polyline(line, false, p, tol));
        }
        let areas: Vec<&Vec<P>> = self.outline.iter().filter(|path| path.len() >= 3).collect();
        if areas.is_empty() && self.outline.iter().any(|line| line.len() >= 2) {
            // Only lines: a line shape is hit along its lines.
            return self.outline.iter().any(|line| near_polyline(line, false, p, tol));
        }
        if areas.is_empty() {
            return inside_polygon(&self.corners, p) || near_polyline(&self.corners, true, p, tol);
        }
        areas.iter().any(|path| inside_polygon(path, p) || near_polyline(path, true, p, tol))
    }

    /// Whether `p` is in a text block of the shape (or, for a group, of a member), with `tol` inches of reach round its edge. **A text block is met wherever it lies**: the name under a BPMN
    /// event or a network device is outside the shape's frame, and a click on it is a click on the shape. [`Scene::hit`] asks this only where no shape's geometry is at the point.
    pub fn hit_text(&self, p: P, tol: f64) -> bool {
        self.text_blocks.iter().any(|block| inside_polygon(block, p) || near_polyline(block, true, p, tol))
    }

    /// Whether `p` meets the shape by its geometry ([`Item::hit`]) or by its text ([`Item::hit_text`]): what a press on one shape that is already known asks.
    pub fn meets(&self, p: P, tol: f64) -> bool {
        self.hit(p, tol) || self.hit_text(p, tol)
    }

    /// Whether the whole shape lies inside the box `lo` to `hi`.
    pub fn inside(&self, lo: P, hi: P) -> bool {
        box_inside(self.bounds, (lo, hi))
    }

    /// The middle of the shape's box.
    pub fn centre(&self) -> P {
        ((self.bounds.0.0 + self.bounds.1.0) / 2.0, (self.bounds.0.1 + self.bounds.1.1) / 2.0)
    }

    /// Whether a drag of the shape moves anything: a connector with both ends glued stays with its shapes.
    pub fn moves_by_itself(&self) -> bool {
        self.kind != Kind::Connector || self.glue.iter().any(Option::is_none)
    }

    /// The shape's own frame on the page, which the handles and a drag of one work in. `None` for a box with no area.
    pub fn basis(&self) -> Option<Basis> {
        Basis::of_corners(&self.corners)
    }

    /// Whether the eight handles work: a 2-D shape or a group that is not in a turned or flipped group. A turned or flipped shape has them
    /// And so has a group, which is sized as one shape and whose members follow it.
    pub fn resizable(&self) -> bool {
        matches!(self.kind, Kind::Shape | Kind::Group) && !self.in_turned_group
    }

    /// Whether the rotation handle is shown: a 2-D shape or a group that is not in a turned or flipped group.
    pub fn rotatable(&self) -> bool {
        matches!(self.kind, Kind::Shape | Kind::Group) && !self.in_turned_group
    }
}

/// A connection point of a shape on the page, in inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// The row's `IX`: what `connector.add` calls `point`.
    pub index: u32,
    pub at: P,
}

/// The shapes of one page at one revision.
pub struct Scene {
    pub page: u32,
    pub revision: u64,
    /// Drawing units to a paper inch.
    pub per_inch: f64,
    /// The shapes in the page's order: a later one is in front of an earlier one.
    pub items: Vec<Item>,
    /// How many top-level shapes were left unread because the page has more than [`MAX_ITEMS`].
    pub unread: usize,
    /// The members of the page's groups, to any depth read, the members of a group together and in the group's order.
    pub members: Vec<Item>,
    /// Where each `ID` is: in `items`, or (counted after them) in `members`.
    index: HashMap<u32, usize>,
    points: RefCell<HashMap<u32, Vec<Point>>>,
    /// What the page's layers make of the shapes that are on a layer that is hidden, locked or otherwise not as a plain shape's, by `ID` (`layers::page_standing`): a shape on a
    /// hidden or a locked layer is not selectable, so a click, a rubber band and Select All pass over it.
    standing: HashMap<u32, Standing>,
    /// The steps the cells that hold a formula and no saved value may be worked out in, for this scene and the connection points it reads later.
    worked: WorkedBudget,
}

/// The number a cell holds as the pointer reads it: saved, or for a cell with a formula and no saved value what the formula works out to; 0 for a cell that holds none and for one
/// that is not a finite number.
fn number(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, name: &str) -> f64 {
    worked.number_at(res, sheet, &CellAddr::direct(name)).filter(|n| n.is_finite()).unwrap_or(0.0)
}

fn is_set(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> bool {
    res.resolve(sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some())
}

/// A page-space point in inches, from drawing units.
fn inches(p: P, per_inch: f64) -> P {
    (p.0 / per_inch, p.1 / per_inch)
}

/// How the page paints the shape whose geometry is read (`diacraft_render`'s `Looks` and `Frame::enter`): what its `LinePattern`, `FillPattern` and `DisplayMode` say, the same cells read the
/// same way.
#[derive(Clone, Copy)]
struct Look {
    /// `LinePattern` is not 0: the shape has a line (a missing cell reads 0).
    line: bool,
    /// `FillPattern` is 0: no path of the shape is filled, whatever its section says (`SectionFlags::paint`).
    fill_none: bool,
    /// `DisplayMode` is not 0 (a missing cell reads 2): the shape's own geometry is drawn, behind its members at 1 and in front of them at 2.
    shown: bool,
}

/// One path of a shape's geometry, in the shape's own frame (drawing units), and whether the page paints anything of it.
struct Run {
    points: Vec<P>,
    painted: bool,
}

/// What [`paths_of`] found.
struct Geometry {
    runs: Vec<Run>,
    /// The shape has `Geometry` sections, and every one is hidden.
    all_hidden: bool,
}

/// The number the cell `name` of the shape `sheet` holds **as the renderer reads a cell of its look**: saved, or worked out; and for a shape that finds no value anywhere on the way **the root style's**
/// ("No Style" is the style every object has: `Resolver::root_standin`, the one rule the renderer and the formatting controls read by), and `default` for a cell the root style holds no
/// value of either. A shape of a stencil file that links to no style, or a document whose styles hold no line pattern, is read as the picture draws it.
fn look_number(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, name: &str, default: f64) -> f64 {
    let addr = CellAddr::direct(name);
    let Some(found) = res.resolve(sheet, &addr) else { return default };
    if found.holds_unsaved_formula() || found.value.cell.is_some() {
        return worked.number_of(sheet, &addr, &found).filter(|n| n.is_finite()).unwrap_or(default);
    }
    let root = res.root_standin(sheet, &addr, &found);
    root.and_then(|root| root.value.cell.and_then(|cell| cell.value.as_ref().and_then(Value::number))).filter(|n| n.is_finite()).unwrap_or(default)
}

/// Whether the flag cell `name` of the section `key` is set, as the renderer reads a flag: any number but 0.
fn section_flag(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, key: &SectionKey, name: &str) -> bool {
    worked.number_at(res, sheet, &CellAddr::in_section("Geometry", key.ix, name)).is_some_and(|n| n != 0.0)
}

/// The geometry of a shape as paths of points in its own frame (drawing units). Moves start a path, lines continue it, an ellipse is a
/// path of its own, an arc of an ellipse (angle 0) is walked, and every other kind of curve is followed by a straight piece to where it ends
/// (a hit test needs no more).
///
/// **A section that is hidden (`NoShow`) is left out**, as the renderer leaves it (a marker a group's member shows only for some Shape Data is there, in every variant, and hidden), and the points
/// of the others are all the budget is spent on. **A section's paths are painted** when [`SectionFlags::paint`] says the section has a stroke the shape draws (`LinePattern` not 0) or a fill (`FillPattern`
/// not 0): the page's rule, so a tile and a picture of the page agree on what is there.
fn paths_of(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, size: (f64, f64), look: Look) -> Geometry {
    let (width, height) = size;
    let Some(keys) = res.effective_sections(sheet, "Geometry") else { return Geometry { runs: Vec::new(), all_hidden: false } };
    let mut runs: Vec<Run> = Vec::new();
    let mut budget = MAX_POINTS;
    let mut shown = 0;
    for key in keys.iter().take(MAX_LOOKED_AT) {
        if shown >= MAX_SECTIONS {
            break;
        }
        if section_flag(res, worked, sheet, key, "NoShow") {
            continue;
        }
        shown += 1;
        let flags = SectionFlags {
            no_fill: section_flag(res, worked, sheet, key, "NoFill"),
            no_line: section_flag(res, worked, sheet, key, "NoLine"),
            no_show: false,
        };
        let paint = flags.paint(look.fill_none);
        let painted = look.shown && (paint.fill || paint.stroke && look.line);
        let Some(rows) = res.effective_rows(sheet, key) else { continue };
        let mut paths: Vec<Vec<P>> = Vec::new();
        let mut current: Vec<P> = Vec::new();
        for row in rows.iter().take(MAX_ROWS) {
            let cell = |name: &str| row_number(worked, sheet, key, row, name);
            let kind = row.row_type.as_deref().unwrap_or("");
            match kind {
                "MoveTo" | "RelMoveTo" => {
                    paths.push(std::mem::take(&mut current));
                    let (x, y) = (cell("X"), cell("Y"));
                    current.push(if kind == "RelMoveTo" { (x * width, y * height) } else { (x, y) });
                }
                // An arc of an ellipse whose axes lie along the shape's (angle 0), through a middle point `A`, `B`: walked, so that a round
                // end is round to the eye and to the pointer. At another angle it is followed by a straight piece like any other curve.
                "EllipticalArcTo" if cell("C").abs() < 1.0e-9 && !current.is_empty() => {
                    let from = current.last().copied().unwrap_or((0.0, 0.0));
                    current.extend(geom::elliptical_arc(from, (cell("A"), cell("B")), (cell("X"), cell("Y")), cell("D")));
                }
                "LineTo" | "ArcTo" | "EllipticalArcTo" | "PolylineTo" | "NURBSTo" | "SplineStart" | "SplineKnot" | "RelCubBezTo" | "RelLineTo" => {
                    let (x, y) = (cell("X"), cell("Y"));
                    current.push(if kind.starts_with("Rel") { (x * width, y * height) } else { (x, y) });
                }
                "Ellipse" => {
                    paths.push(std::mem::take(&mut current));
                    paths.push(ellipse(cell("X"), cell("Y"), cell("A"), cell("B"), cell("C"), cell("D")));
                }
                _ => {}
            }
            budget = budget.saturating_sub(1);
            if budget == 0 {
                break;
            }
        }
        paths.push(current);
        runs.extend(paths.into_iter().map(|points| Run { points, painted }));
    }
    runs.retain(|run| !run.points.is_empty() && run.points.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
    runs.iter_mut().for_each(|run| run.points.truncate(MAX_POINTS));
    // Hidden for certain only when every section was looked at.
    Geometry { runs, all_hidden: !keys.is_empty() && keys.len() <= MAX_LOOKED_AT && shown == 0 }
}

/// The number the cell `name` of a geometry row holds: saved, or worked out for a cell with a formula and no saved value (on the row of the section `key`); 0 for a cell the row
/// does not hold and for one that is not a finite number.
fn row_number(worked: &Worked<'_>, sheet: &SheetRef, key: &SectionKey, row: &diacraft_engine::doc::EffectiveRow<'_>, name: &str) -> f64 {
    row.cells
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .and_then(|(n, found)| {
            let addr = CellAddr { section: Some(key.clone()), row: Some(row.key.clone()), cell: n.clone() };
            worked.number_of(sheet, &addr, found)
        })
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

/// An ellipse as a closed path: its centre, a point on its right and a point on its top, taken as axis-aligned. **The first point is repeated at the end**, as the corners of a polygon
/// are (a tile draws a path that comes back to where it began as an outline, and one that does not as a line with a gap in it).
fn ellipse(x: f64, y: f64, a: f64, _b: f64, _c: f64, d: f64) -> Vec<P> {
    let (rx, ry) = ((a - x).abs(), (d - y).abs());
    let mut points: Vec<P> = (0..ELLIPSE_SEGMENTS)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / ELLIPSE_SEGMENTS as f64;
            (x + rx * angle.cos(), y + ry * angle.sin())
        })
        .collect();
    if let Some(first) = points.first().copied() {
        points.push(first);
    }
    points
}

/// Where a shape is among the shapes around it: the poses of the groups it is in (the innermost first), the group it is directly in, and the
/// path to it.
struct Place<'a> {
    path: Vec<u32>,
    groups: &'a [Pose],
    parent: Option<u32>,
}

/// What a click on the group at `sheet` picks: its `SelectMode`, found by inheritance, 1 when no cell gives one. A number that
/// is not 0 or 2 reads 1 (MS-VSDX treats 1 and 2 alike).
fn select_mode_of(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef) -> u8 {
    let held = worked.number_at(res, sheet, &CellAddr::direct("SelectMode")).filter(|n| n.is_finite());
    match held.map(|n| n.round() as i64) {
        Some(0) => 0,
        Some(2) => 2,
        _ => 1,
    }
}

/// Whether the shape shows any text: its own, or when it has none its master shape's (the renderer's rule: an own text with nothing in it shows nothing, and the master's plays no part).
fn shows_text(res: &Resolver<'_>, at: &ShapeRef, shape: &Shape) -> bool {
    let shows = |s: &Shape| !super::text::shape_text(s).is_empty();
    if shape.text.is_some() {
        return shows(shape);
    }
    match res.master_step(at) {
        Ok(Some(step)) => res.shape(&step.shape).is_some_and(|master| shows(master)),
        _ => false,
    }
}

/// The pose of the shape at `sheet`.
fn read_pose(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef) -> Pose {
    let n = |name: &str| number(res, worked, sheet, name);
    Pose { pin: (n("PinX"), n("PinY")), loc: (n("LocPinX"), n("LocPinY")), angle: n("Angle"), flip_x: n("FlipX") != 0.0, flip_y: n("FlipY") != 0.0 }
}

/// The text block of the shape `shape` at `at` on the page, in inches, when it shows text (the corners in the order of the block's own frame): the frame, with the flips of the groups
/// the shape is in (`groups` are their poses), by the engine's own rule ([`text_block_in_frame`]) and carried through the shape's pose and the groups' as its outline is. `None` for a shape that
/// shows no text, and for a block whose corners are not numbers.
fn text_block_of(reading: &Reading<'_>, at: &ShapeRef, shape: &Shape, pose: &Pose, groups: &[Pose]) -> Option<[P; 4]> {
    if !shows_text(reading.res, at, shape) {
        return None;
    }
    let flips = groups.iter().fold((false, false), |(x, y), group| (x != group.flip_x, y != group.flip_y));
    let corners = text_block_in_frame(reading.res, reading.worked, &SheetRef::Shape(at.clone()), pose, flips);
    let on_page = corners.map(|corner| inches(groups.iter().fold(pose.up(corner), |q, group| group.up(q)), reading.per_inch));
    on_page.iter().all(|p| p.0.is_finite() && p.1.is_finite()).then_some(on_page)
}

/// One shape as an item, and its own pose: `None` for a shape that is not drawn (deleted from its master, or a guide). The item holds the shape's own text block; a group's members' are added
/// by whoever reads the members ([`read_members`], [`blocks_below`]).
fn read_item(reading: &Reading<'_>, place: &Place<'_>, shape: &Shape) -> Option<(Item, Pose)> {
    let (res, worked, page, per_inch) = (reading.res, reading.worked, reading.page, reading.per_inch);
    if shape.del || shape.kind == ShapeType::Guide {
        return None;
    }
    let at = ShapeRef { owner: Owner::Page(page), path: place.path.clone() };
    let sheet = SheetRef::Shape(at.clone());
    let n = |name: &str| number(res, worked, &sheet, name);
    let pose = read_pose(res, worked, &sheet);
    // A point of the parent's frame on the page: through the poses of the groups the shape is in.
    let lift = |p: P| place.groups.iter().fold(p, |q, group| group.up(q));
    let (width, height) = (n("Width"), n("Height"));
    let up = |p: P| inches(lift(pose.up(p)), per_inch);
    let corners = [up((0.0, 0.0)), up((width, 0.0)), up((width, height)), up((0.0, height))];
    let low = inches(lift((pose.pin.0 - pose.loc.0, pose.pin.1 - pose.loc.1)), per_inch);
    let frame = (low, (low.0 + width / per_inch, low.1 + height / per_inch));
    let one_d = ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| is_set(res, &sheet, name));
    let kind = if one_d {
        Kind::Connector
    } else if shape.kind == ShapeType::Group {
        Kind::Group
    } else {
        Kind::Shape
    };
    let ends = one_d.then(|| (inches(lift((n("BeginX"), n("BeginY"))), per_inch), inches(lift((n("EndX"), n("EndY"))), per_inch)));
    let look = Look {
        line: look_number(res, worked, &sheet, "LinePattern", 0.0) != 0.0,
        fill_none: look_number(res, worked, &sheet, "FillPattern", 0.0) == 0.0,
        shown: look_number(res, worked, &sheet, "DisplayMode", 2.0) >= 0.5,
    };
    let geometry = paths_of(res, worked, &sheet, (width, height), look);
    let own_block = text_block_of(reading, &at, shape, &pose, place.groups);
    let hidden = kind == Kind::Shape && geometry.all_hidden && own_block.is_none();
    let (mut outline, mut painted): (Vec<Vec<P>>, Vec<bool>) =
        geometry.runs.into_iter().map(|run| (run.points.into_iter().map(up).collect(), run.painted)).unzip();
    // A connector that has no geometry shown is a straight line from its begin to its end.
    if let (Some((begin, end)), true) = (ends, outline.is_empty()) {
        outline.push(vec![begin, end]);
        painted.push(true);
    }
    let all: Vec<P> = corners.iter().copied().chain(outline.iter().flatten().copied()).chain(ends.into_iter().flat_map(|(a, b)| [a, b])).collect();
    let bounds = bounds_of(&all)?;
    let item = Item {
        id: shape.id,
        kind,
        corners,
        frame,
        turned: pose.is_turned_or_flipped(),
        in_turned_group: place.groups.iter().any(Pose::is_turned_or_flipped),
        select_mode: if kind == Kind::Group { select_mode_of(res, worked, &sheet) } else { 1 },
        pin: inches(lift(pose.pin), per_inch),
        parent: place.parent,
        degrees: pose.angle.to_degrees(),
        outline,
        painted,
        hidden,
        text_blocks: own_block.into_iter().collect(),
        ends,
        glue: [None, None],
        bounds,
    };
    Some((item, pose))
}

/// What reading the shapes of a page needs to know.
struct Reading<'a> {
    res: &'a Resolver<'a>,
    worked: &'a Worked<'a>,
    page: u32,
    per_inch: f64,
    /// Whether the members of a group are read whatever its `SelectMode` says (a tile draws them all).
    all_members: bool,
    /// How many more shapes may be looked into for their text blocks alone ([`MAX_BLOCK_VISITS`] to begin with, for all the page).
    visits: Cell<usize>,
}

/// Appends `more` to `blocks` up to [`MAX_BLOCKS`].
fn add_blocks(blocks: &mut Vec<[P; 4]>, more: impl IntoIterator<Item = [P; 4]>) {
    let room = MAX_BLOCKS.saturating_sub(blocks.len());
    blocks.extend(more.into_iter().take(room));
}

/// The text blocks of the shapes below `group` (at `path`, with its own pose first among `groups`), at every depth, **without reading them as items**: the labels of a group whose members are
/// not offered to the pointer (`SelectMode` 0, here or above) are met all the same. Stops at [`MAX_GROUP_DEPTH`] levels, at [`MAX_BLOCKS`] blocks and when the page's visits are spent.
fn blocks_below(reading: &Reading<'_>, group: &Shape, path: &[u32], groups: &[Pose], out: &mut Vec<[P; 4]>) {
    if groups.len() > MAX_GROUP_DEPTH {
        return;
    }
    for (index, child) in group.children.iter().enumerate() {
        if out.len() >= MAX_BLOCKS || reading.visits.get() == 0 {
            return;
        }
        reading.visits.set(reading.visits.get().saturating_sub(1));
        let Ok(at) = u32::try_from(index) else { continue };
        if child.del || child.kind == ShapeType::Guide {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(at);
        let child_ref = ShapeRef { owner: Owner::Page(reading.page), path: child_path.clone() };
        let pose = read_pose(reading.res, reading.worked, &SheetRef::Shape(child_ref.clone()));
        add_blocks(out, text_block_of(reading, &child_ref, child, &pose, groups));
        if child.kind == ShapeType::Group {
            let mut inner = vec![pose];
            inner.extend_from_slice(groups);
            blocks_below(reading, child, &child_path, &inner, out);
        }
    }
}

/// Reads the members of `group` (at `path`, with its own pose first among `groups`) and, below them, the members of the groups among them, into
/// `out`. Stops at [`MAX_GROUP_DEPTH`] levels and when [`MAX_MEMBERS`] have been read. **Gives the text blocks of the members at every depth** (those of the groups that are not read
/// are looked into by [`blocks_below`]); each member that is a group holds those below it.
fn read_members(reading: &Reading<'_>, group: &Shape, path: &[u32], groups: &[Pose], out: &mut Vec<Item>) -> Vec<[P; 4]> {
    let mut blocks = Vec::new();
    if groups.len() > MAX_GROUP_DEPTH {
        return blocks;
    }
    for (index, child) in group.children.iter().enumerate() {
        if out.len() >= MAX_MEMBERS {
            return blocks;
        }
        let Ok(at) = u32::try_from(index) else { continue };
        let mut child_path = path.to_vec();
        child_path.push(at);
        let place = Place { path: child_path.clone(), groups, parent: Some(group.id) };
        let Some((item, pose)) = read_item(reading, &place, child) else { continue };
        // A group set to group only has no members to offer, and none of the groups inside it either (every ancestor counts).
        let is_group = item.kind == Kind::Group;
        let has_members = is_group && (item.select_mode != 0 || reading.all_members);
        add_blocks(&mut blocks, item.text_blocks.iter().copied());
        let slot = out.len();
        out.push(item);
        if is_group {
            let mut inner = vec![pose];
            inner.extend_from_slice(groups);
            let mut below = Vec::new();
            if has_members {
                below = read_members(reading, child, &child_path, &inner, out);
            } else {
                blocks_below(reading, child, &child_path, &inner, &mut below);
            }
            add_blocks(&mut blocks, below.iter().copied());
            if let Some(member) = out.get_mut(slot) {
                add_blocks(&mut member.text_blocks, below);
            }
        }
    }
    blocks
}

/// The first of the items `items` makes (front to back) that `takes` and whose geometry is at `p`, else the first that `takes` and has a text block at `p`.
fn met_first<'a, I: Iterator<Item = &'a Item>>(items: impl Fn() -> I, p: P, tol: f64, takes: impl Fn(&Item) -> bool) -> Option<&'a Item> {
    items().find(|item| takes(item) && item.hit(p, tol)).or_else(|| items().find(|item| takes(item) && item.hit_text(p, tol)))
}

impl Scene {
    /// The shapes of page `page` of `doc`, read at `revision`. `None` for a page the document does not have.
    pub fn build(doc: &Document, page: u32, revision: u64) -> Option<Scene> {
        Scene::build_within(doc, page, revision, WORKED_STEPS)
    }

    /// [`Scene::build`] with `steps` for the cells that hold a formula and no saved value to be worked out in, in all (a test gives few): when they run out the cells left are read
    /// as saved, which is no value.
    pub fn build_within(doc: &Document, page: u32, revision: u64, steps: u64) -> Option<Scene> {
        Scene::build_with(doc, page, revision, false, steps)
    }

    /// [`Scene::build`] with **the members of every group read, whatever its `SelectMode` says**: a pointer is not offered a member of a group set to group only, but a
    /// tile that draws the group draws them (`stencils.rs`).
    pub fn build_all(doc: &Document, page: u32, revision: u64) -> Option<Scene> {
        Scene::build_with(doc, page, revision, true, WORKED_STEPS)
    }

    fn build_with(doc: &Document, page: u32, revision: u64, all_members: bool, steps: u64) -> Option<Scene> {
        let part = doc.page(page)?;
        let per_inch = units_per_inch(doc, page);
        let res = Resolver::new(doc);
        // The cells with a formula and no saved value are worked out with steps of this scene's own: its build, and the connection points it reads later.
        let budget = WorkedBudget::new(steps);
        let _scope = budget.enter();
        let worked = Worked::new(doc);
        let reading = Reading { res: &res, worked: &worked, page, per_inch, all_members, visits: Cell::new(MAX_BLOCK_VISITS) };
        let mut items = Vec::new();
        let mut members = Vec::new();
        for (index, shape) in part.shapes.iter().enumerate().take(MAX_ITEMS) {
            let Ok(at) = u32::try_from(index) else { continue };
            let place = Place { path: vec![at], groups: &[], parent: None };
            let Some((mut item, pose)) = read_item(&reading, &place, shape) else { continue };
            if item.kind == Kind::Group {
                // The labels of a group are its members' text blocks, at every depth, whether or not the members are offered to the pointer.
                let has_members = item.select_mode != 0 || all_members;
                let mut below = Vec::new();
                if has_members {
                    below = read_members(&reading, shape, &[at], &[pose], &mut members);
                } else {
                    blocks_below(&reading, shape, &[at], &[pose], &mut below);
                }
                add_blocks(&mut item.text_blocks, below);
            }
            items.push(item);
        }
        let unread = part.shapes.len().saturating_sub(MAX_ITEMS);
        let mut index: HashMap<u32, usize> = items.iter().enumerate().rev().map(|(at, item)| (item.id, at)).collect();
        for (at, member) in members.iter().enumerate().rev() {
            // A member whose `ID` a top-level shape has too is reached through the top-level one (a hostile file).
            index.entry(member.id).or_insert(items.len() + at);
        }
        for connect in part.connects.iter().take(MAX_CONNECTS) {
            let which = match connect.from_part {
                Some(FROM_BEGIN) => 0,
                Some(FROM_END) => 1,
                _ => continue,
            };
            let Some(point) = connect.to_part.and_then(|to| to.checked_sub(TO_PART_BASE)) else { continue };
            let slot = index.get(&connect.from_sheet).copied().and_then(|at| items.get_mut(at)).and_then(|item| item.glue.get_mut(which));
            if let Some(slot) = slot {
                *slot = Some((connect.to_sheet, point));
            }
        }
        // A page whose layers are all plain costs nothing; a table of too many rows is read as having no standing (the engine's commands say so in words).
        let standing = page_standing(doc, page).unwrap_or_default();
        Some(Scene { page, revision, per_inch, items, unread, members, index, points: RefCell::new(HashMap::new()), standing, worked: budget })
    }

    /// The shape with this `ID`: a top-level one, or a member of a group.
    pub fn get(&self, id: u32) -> Option<&Item> {
        let at = *self.index.get(&id)?;
        match at.checked_sub(self.items.len()) {
            Some(member) => self.members.get(member),
            None => self.items.get(at),
        }
    }

    /// The members of group `id`, in the group's order: a later one is in front of an earlier one.
    pub fn children_of(&self, id: u32) -> impl DoubleEndedIterator<Item = &Item> {
        self.members.iter().filter(move |member| member.parent == Some(id))
    }

    /// The top-level shape that `id` is, or is a member of at some depth.
    pub fn root_of(&self, id: u32) -> Option<u32> {
        let mut at = self.get(id)?;
        // A chain of parents is as long as the groups nest, which the scene read to at most MAX_GROUP_DEPTH levels.
        for _ in 0..=MAX_GROUP_DEPTH + 1 {
            match at.parent {
                Some(parent) => at = self.get(parent)?,
                None => return Some(at.id),
            }
        }
        None
    }

    /// What the page's layers make of the shape `id` (plain for a shape on no layer).
    pub fn standing(&self, id: u32) -> Standing {
        self.standing.get(&id).copied().unwrap_or_default()
    }

    /// Whether a click, a rubber band or Select All may pick the shape `id`: not when it is on a locked layer, or on hidden ones only.
    pub fn selectable(&self, id: u32) -> bool {
        let st = self.standing(id);
        !(st.locked || st.hidden)
    }

    /// The `ID`s of the top-level shapes that can be picked (see [`Scene::selectable`]), in the page's order.
    pub fn selectable_ids(&self) -> Vec<u32> {
        self.items.iter().filter(|item| self.selectable(item.id)).map(|item| item.id).collect()
    }

    /// The front-most member of group `group` at `p` (one level down), with `tol` inches of reach round a line: by its geometry, and where no member's geometry is there, by its text block.
    pub fn hit_child(&self, group: u32, p: P, tol: f64) -> Option<&Item> {
        met_first(|| self.children_of(group).rev(), p, tol, |member| self.selectable(member.id))
    }

    /// The front-most shape at `p`, with `tol` inches of reach round a line. **By its geometry first, and only where no shape's geometry is at `p` by its text block**: the topmost shape
    /// whose geometry is there is met exactly as it always was, and a click that is on no geometry meets the topmost shape that has a text block there (the name that sits under a shape,
    /// outside its frame). A shape's own text block, which the geometry of an ordinary shape covers, is never in the way of the shapes behind it.
    pub fn hit(&self, p: P, tol: f64) -> Option<&Item> {
        self.hit_where(p, tol, |_| true)
    }

    /// The front-most shape at `p` that `takes` and the layers allow a click to pick, by [`Scene::hit`]'s order: geometry first, then text blocks.
    pub fn hit_where(&self, p: P, tol: f64, takes: impl Fn(&Item) -> bool) -> Option<&Item> {
        met_first(|| self.items.iter().rev(), p, tol, |item| self.selectable(item.id) && takes(item))
    }

    /// The front-most shape that is not a connector at `p`: what a connector can be glued to (a layer whose Glue box is clear lets none glue to its shapes). Geometry first, then text
    /// blocks, as [`Scene::hit`].
    pub fn hit_target(&self, p: P, tol: f64) -> Option<&Item> {
        self.hit_where(p, tol, |item| item.kind != Kind::Connector && !self.standing(item.id).no_glue)
    }

    /// The `ID`s of the shapes wholly inside the box `lo` to `hi`, in the page's order.
    pub fn inside(&self, lo: P, hi: P) -> Vec<u32> {
        self.items.iter().filter(|item| self.selectable(item.id) && item.inside(lo, hi)).map(|item| item.id).collect()
    }

    /// The connection points of shape `id`, in inches, as the engine finds them. Read once per scene. **A `Connection` section of more than
    /// [`MAX_CONNECTION_ROWS`] rows (256), counted over the shape, its master and the styles before any is merged, is not read**, and the shape has
    /// none here, as the engine's snap reads them (a file can turn View > Connection Points on, and the reader allows 320,000
    /// rows a section). The Connector tool's marks and its pick of a point under the pointer read them here too.
    pub fn points(&self, doc: &Document, id: u32) -> Vec<Point> {
        if let Some(found) = self.points.borrow().get(&id) {
            return found.clone();
        }
        // The steps the points' cells may be worked out in are the scene's: all the shapes' points of a revision together are no more than the scene was given.
        let _scope = self.worked.enter();
        let read: Vec<Point> = connection_points_within(doc, self.page, id, MAX_CONNECTION_ROWS)
            .unwrap_or_default()
            .into_iter()
            .map(|p| Point { index: p.index, at: inches((p.x, p.y), self.per_inch) })
            .filter(|p| p.at.0.is_finite() && p.at.1.is_finite())
            .collect();
        self.points.borrow_mut().insert(id, read.clone());
        read
    }

    /// The connection point of `id` nearest `p` and within `reach` inches of it.
    pub fn nearest_point(&self, doc: &Document, id: u32, p: P, reach: f64) -> Option<Point> {
        self.points(doc, id)
            .into_iter()
            .map(|point| (distance(point.at, p), point))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, point)| point)
    }
}

#[cfg(test)]
mod hits;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    fn session_with(masters: &[(&str, f64, f64)]) -> (diacraft_engine::Session, Vec<u32>) {
        let mut s = diacraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let ids = masters
            .iter()
            .map(|(master, x, y)| {
                let r = s.execute("shape.drop", &json!({"master": master, "page": page, "x": x, "y": y})).unwrap();
                u32::try_from(r["id"].as_u64().unwrap()).unwrap()
            })
            .collect();
        (s, ids)
    }

    #[test]
    fn a_dropped_shape_is_found_where_it_is_and_the_box_agrees_with_the_engine() {
        let (s, ids) = session_with(&[("rectangle", 4.0, 3.0)]);
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap().id;
        let scene = Scene::build(doc, page, s.revision()).unwrap();
        let item = scene.get(ids[0]).unwrap();
        // A rectangle is 1.5 by 1 inch, centred at (4, 3).
        assert!((item.frame.0.0 - 3.25).abs() < 1e-9 && (item.frame.0.1 - 2.5).abs() < 1e-9, "{:?}", item.frame);
        assert!((item.frame.1.0 - 4.75).abs() < 1e-9 && (item.frame.1.1 - 3.5).abs() < 1e-9, "{:?}", item.frame);
        let frame = diacraft_engine::draw::shape_frame(doc, page, ids[0]).unwrap();
        assert!((frame.x - item.frame.0.0).abs() < 1e-9 && (frame.y - item.frame.0.1).abs() < 1e-9);
        assert!(!item.turned && item.kind == Kind::Shape);
        assert_eq!(scene.hit((4.0, 3.0), 0.05).map(|i| i.id), Some(ids[0]));
        assert!(scene.hit((6.0, 3.0), 0.05).is_none());
    }

    /// Window-free: the selection box of a group is the group's frame, which `Scene` reads from the cells (`read_item`), so a
    /// member moved out of the group gives the group the box of its members at once, in the session, and a click on the moved member is a click on the group.
    #[test]
    fn a_group_has_the_selection_box_of_its_members_at_once_when_one_of_them_is_moved_out_of_it() {
        let (mut s, ids) = session_with(&[("rectangle", 4.0, 3.0), ("ellipse", 7.0, 3.0)]);
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let made = s.execute("shape.group", &json!({"ids": ids, "page": page})).unwrap();
        let group = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.execute("shape.move", &json!({"ids": [ids[0]], "dx": -2.5, "dy": 3.0, "page": page})).unwrap();
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, page, s.revision()).unwrap();
        let item = scene.get(group).unwrap();
        // The box of the two members on the page, from the engine's own corners for each.
        let corners: Vec<(f64, f64)> = ids.iter().flat_map(|id| diacraft_engine::draw::shape_corners(doc, page, *id).unwrap()).collect();
        let low = corners.iter().fold((f64::MAX, f64::MAX), |m, c| (m.0.min(c.0), m.1.min(c.1)));
        let high = corners.iter().fold((f64::MIN, f64::MIN), |m, c| (m.0.max(c.0), m.1.max(c.1)));
        let per_inch = diacraft_engine::draw::units_per_inch(doc, page);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-7;
        assert!(near(item.frame.0.0, low.0 / per_inch) && near(item.frame.0.1, low.1 / per_inch), "{:?} against {low:?}", item.frame);
        assert!(near(item.frame.1.0, high.0 / per_inch) && near(item.frame.1.1, high.1 / per_inch), "{:?} against {high:?}", item.frame);
        assert!(near(item.bounds.0.0, item.frame.0.0) && near(item.bounds.1.1, item.frame.1.1), "the bounds a click is tested in are the frame");
        // The moved member is outside the frame the group had: a click on its middle is a click on the group.
        let moved = diacraft_engine::draw::shape_frame(doc, page, ids[0]).unwrap();
        let middle = ((moved.x + moved.width / 2.0) / per_inch, (moved.y + moved.height / 2.0) / per_inch);
        assert_eq!(scene.hit(middle, 0.02).map(|i| i.id), Some(group));
    }

    #[test]
    fn a_diamond_is_not_hit_at_the_corner_of_its_box_but_a_rectangle_is() {
        let (s, ids) = session_with(&[("diamond", 3.0, 3.0), ("rectangle", 7.0, 3.0)]);
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, doc.pages.get(0).unwrap().id, s.revision()).unwrap();
        // 1.5 by 1: the upper right corner of the box is 0.75 right of and 0.5 above the centre.
        assert!(scene.hit((3.0 + 0.7, 3.0 + 0.45), 0.02).is_none(), "outside the diamond's outline");
        assert_eq!(scene.hit((3.0, 3.0), 0.02).map(|i| i.id), Some(ids[0]));
        assert_eq!(scene.hit((7.0 + 0.7, 3.0 + 0.45), 0.02).map(|i| i.id), Some(ids[1]));
    }

    #[test]
    fn an_ellipse_is_hit_by_its_outline_not_its_box() {
        let (s, ids) = session_with(&[("ellipse", 3.0, 3.0)]);
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, doc.pages.get(0).unwrap().id, s.revision()).unwrap();
        assert_eq!(scene.hit((3.0, 3.0), 0.02).map(|i| i.id), Some(ids[0]));
        assert!(scene.hit((3.0 + 0.7, 3.0 + 0.45), 0.02).is_none());
    }

    #[test]
    fn the_front_shape_wins_and_a_connector_is_hit_along_its_path() {
        let (mut s, ids) = session_with(&[("rectangle", 2.0, 3.0), ("rectangle", 6.0, 3.0), ("square", 6.2, 3.1)]);
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        s.execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}, "page": page})).unwrap();
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, page, s.revision()).unwrap();
        assert_eq!(scene.hit((6.2, 3.1), 0.02).map(|i| i.id), Some(ids[2]), "the square is in front of the rectangle");
        let connector = scene.items.iter().find(|i| i.kind == Kind::Connector).unwrap();
        assert_eq!(connector.glue[0].map(|g| g.0), Some(ids[0]));
        assert_eq!(connector.glue[1].map(|g| g.0), Some(ids[1]));
        let (begin, end) = connector.ends.unwrap();
        let mid = ((begin.0 + end.0) / 2.0, (begin.1 + end.1) / 2.0);
        assert_eq!(scene.hit(mid, 0.05).map(|i| i.id), Some(connector.id), "the middle of the connector, between the shapes");
        assert!(!connector.moves_by_itself(), "both ends are glued");
    }

    #[test]
    fn connection_points_come_from_the_engine_in_inches() {
        let (s, ids) = session_with(&[("rectangle", 4.0, 3.0)]);
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, doc.pages.get(0).unwrap().id, s.revision()).unwrap();
        let points = scene.points(doc, ids[0]);
        assert_eq!(points.len(), 4);
        let top = points.iter().find(|p| p.index == 0).unwrap();
        assert!((top.at.0 - 4.0).abs() < 1e-9 && (top.at.1 - 3.5).abs() < 1e-9, "{top:?}");
        let near = scene.nearest_point(doc, ids[0], (4.05, 3.45), 0.2).unwrap();
        assert_eq!(near.index, 0);
        assert!(scene.nearest_point(doc, ids[0], (4.0, 3.0), 0.2).is_none());
    }

    #[test]
    fn marquee_takes_only_what_lies_wholly_inside() {
        let (s, ids) = session_with(&[("rectangle", 2.0, 3.0), ("rectangle", 6.0, 3.0)]);
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, doc.pages.get(0).unwrap().id, s.revision()).unwrap();
        assert_eq!(scene.inside((1.0, 2.0), (3.5, 4.0)), vec![ids[0]]);
        assert!(scene.inside((1.0, 2.0), (2.4, 4.0)).is_empty(), "partly inside is not inside");
        assert_eq!(scene.inside((0.0, 0.0), (10.0, 8.0)), ids);
    }

    /// Sets the cell `cell` of the shape `id` to `formula`, through the engine's command.
    fn set_cell(s: &mut diacraft_engine::Session, page: u32, id: u32, cell: &str, formula: &str) {
        s.execute("sheet.set_cell", &json!({"id": id, "page": page, "cell": cell, "formula": formula})).unwrap();
    }

    /// What the page paints of a shape's paths is what the renderer reads of its cells (`SectionFlags::paint`): the section's `NoShow`, `NoLine` and `NoFill`, the shape's `LinePattern` and
    /// `FillPattern`, and its `DisplayMode`. A path the page paints nothing of still has an outline for the pointer to meet; one that is hidden has none.
    #[test]
    fn what_the_page_paints_of_a_shape_is_what_its_sections_and_its_line_and_fill_say() {
        let (mut s, ids) = session_with(&[("diamond", 4.0, 3.0)]);
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let id = ids[0];
        // Each case sets cells on top of the last one: (the cells, whether the outline is painted, whether there is an outline at all).
        type Case<'a> = (&'a [(&'a str, &'a str)], bool, bool);
        let cases: [Case<'_>; 9] = [
            (&[], true, true),
            // No line, but a fill: the shape is filled, and that is painted.
            (&[("LinePattern", "0")], true, true),
            // A line and no fill.
            (&[("LinePattern", "1"), ("FillPattern", "0")], true, true),
            // No line and no fill: a text box. Nothing is painted, and the pointer still meets it.
            (&[("LinePattern", "0")], false, true),
            // The fill is back, and the section has no line: a fill is painted.
            (&[("FillPattern", "1"), ("Geometry1.NoLine", "1")], true, true),
            // The section has no line and no fill.
            (&[("Geometry1.NoFill", "1")], false, true),
            // The section is as it was, and the shape's own geometry is not drawn.
            (&[("Geometry1.NoLine", "0"), ("Geometry1.NoFill", "0"), ("DisplayMode", "0")], false, true),
            // The section is hidden: no outline, and a click passes over the shape.
            (&[("DisplayMode", "2"), ("Geometry1.NoShow", "1")], false, false),
            (&[("Geometry1.NoShow", "0")], true, true),
        ];
        for (cells, painted, has_outline) in cases {
            for (cell, formula) in cells {
                set_cell(&mut s, page, id, cell, formula);
            }
            let doc = s.document().unwrap();
            let scene = Scene::build(doc, page, s.revision()).unwrap();
            let item = scene.get(id).unwrap();
            assert_eq!(item.outline.len(), usize::from(has_outline), "{cells:?}: the outline {:?}", item.outline);
            assert_eq!(item.painted.len(), item.outline.len(), "{cells:?}");
            assert_eq!(item.painted_paths().count(), usize::from(painted), "{cells:?}");
            assert_eq!(item.painted_bounds().is_some(), painted, "{cells:?}");
            assert_eq!(item.hidden, !has_outline, "{cells:?}: a shape whose only section is hidden");
            // A hidden shape is passed over, and every other is met by its outline, painted or not.
            assert_eq!(scene.hit((4.0, 3.0), 0.02).map(|i| i.id), has_outline.then_some(id), "{cells:?}");
        }
    }

    /// A shape that holds no `LinePattern` or `FillPattern` finds the root style's, as the renderer reads it ("No Style" is the style every object has): a box of a stencil file that links to
    /// no style is drawn, or not, as the drawing's root style says.
    #[test]
    fn a_shape_that_holds_no_line_or_fill_pattern_reads_the_root_styles_as_the_page_does() {
        use diacraft_doc::RowKey;
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
        let doc_with = |line: &str, fill: &str| {
            let mut shape = ShapeBuilder::new(1)
                .cell("PinX", "1")
                .cell("PinY", "1")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5");
            for (at, (kind, x, y)) in
                [("MoveTo", 0.0, 0.0), ("LineTo", 1.0, 0.0), ("LineTo", 1.0, 1.0), ("LineTo", 0.0, 1.0), ("LineTo", 0.0, 0.0)].into_iter().enumerate()
            {
                let key = RowKey::Ix(u32::try_from(at).unwrap());
                shape = shape
                    .row_type("Geometry", Some(0), key.clone(), kind)
                    .row_cell("Geometry", Some(0), key.clone(), "X", &x.to_string())
                    .row_cell("Geometry", Some(0), key, "Y", &y.to_string());
            }
            DocBuilder::new().root(|r| r.cell("LinePattern", line).cell("FillPattern", fill)).page(1, PageBuilder::new().shape(shape)).build()
        };
        for (line, fill, painted) in [("1", "0", true), ("0", "1", true), ("0", "0", false)] {
            let doc = doc_with(line, fill);
            let scene = Scene::build(&doc, 1, 0).unwrap();
            let item = scene.get(1).unwrap();
            assert_eq!(item.outline.len(), 1, "line {line}, fill {fill}");
            assert_eq!(item.painted, [painted], "line {line}, fill {fill}: the root style's pattern is the shape's");
        }
    }

    /// A section that is hidden is no part of the outline, and the budget of points is not spent on it: the markers of a BPMN gateway are all there and hidden at the default Shape Data, and the
    /// pointer meets the diamond and not a marker that cannot be seen.
    #[test]
    fn the_hidden_markers_of_a_group_are_no_outline_and_the_pointer_passes_over_them() {
        let (s, ids) = session_with(&[("bpmn.gateway", 4.0, 3.0)]);
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap().id;
        let scene = Scene::build_all(doc, page, s.revision()).unwrap();
        let members: Vec<&Item> = scene.children_of(ids[0]).collect();
        let hidden: Vec<&&Item> = members.iter().filter(|m| m.hidden).collect();
        assert!(!hidden.is_empty(), "the gateway's markers are hidden at the default Shape Data: {members:?}");
        for marker in hidden {
            assert!(marker.outline.is_empty() && marker.painted_paths().next().is_none(), "{marker:?}");
            assert!(!marker.hit(marker.centre(), 0.05), "a marker nobody can see is not met");
        }
        let shown: Vec<&&Item> = members.iter().filter(|m| m.painted_paths().next().is_some()).collect();
        assert_eq!(shown.len(), 1, "the diamond is all the gateway paints: {shown:?}");
        assert_eq!(shown[0].outline[0].len(), 5, "its four corners and the first again");
    }

    /// An ellipse comes back to where it began, so that a tile draws it as an outline and not as a line with a gap in it.
    #[test]
    fn an_ellipse_is_a_closed_path() {
        let (s, ids) = session_with(&[("ellipse", 3.0, 3.0)]);
        let doc = s.document().unwrap();
        let scene = Scene::build(doc, doc.pages.get(0).unwrap().id, s.revision()).unwrap();
        let outline = &scene.get(ids[0]).unwrap().outline;
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].first(), outline[0].last());
        assert_eq!(outline[0].len(), ELLIPSE_SEGMENTS + 1);
    }

    /// What reading a crowded page costs, once per edit and not per frame: 500 rectangles and 300 connectors. Counted, not timed (a hit test held
    /// to 5 milliseconds failed under load): the lookups the model built for the read (`diacraft_doc::work`) and the shapes a hit test asks
    /// about. The only bound on time is a hang guard of a minute.
    #[test]
    fn reading_a_page_of_five_hundred_shapes_and_three_hundred_connectors_is_quick() {
        use diacraft_engine::draw::{End, attach_connector, drop_shape, settle};
        let mut s = diacraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let rectangle = diacraft_engine::stencil::find_master("rectangle").unwrap();
        s.edit("build", |doc| {
            let fail = |p: diacraft_engine::draw::Problem| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: p.message };
            let before = doc.clone();
            let mut ids = Vec::new();
            for r in 0..20 {
                for c in 0..25 {
                    ids.push(drop_shape(doc, page, rectangle, 1.5 + 3.0 * f64::from(c), 1.0 + 2.0 * f64::from(r)).map_err(fail)?.id);
                }
            }
            let mut connectors = Vec::new();
            'rows: for r in 0..20 {
                for c in 0..24 {
                    if connectors.len() == 300 {
                        break 'rows;
                    }
                    let (a, b) = (ids[r * 25 + c], ids[r * 25 + c + 1]);
                    connectors
                        .push(attach_connector(doc, page, &End::Shape { id: a, point: None }, &End::Shape { id: b, point: None }).map_err(fail)?.id);
                }
            }
            settle(doc, &before, page, &[], &connectors).map_err(fail)
        })
        .unwrap();
        let doc = s.document().unwrap();
        let started = std::time::Instant::now();
        diacraft_doc::work::reset();
        let scene = Scene::build(doc, page, s.revision()).unwrap();
        let lookups = diacraft_doc::work::resolver_builds();
        assert!(started.elapsed() < std::time::Duration::from_secs(60), "the read of 800 shapes took {:?}: a hang", started.elapsed());
        assert_eq!(scene.items.len(), 800);
        // The read builds the model's lookups a few times for the page, not once for each of the 800 shapes.
        assert!(lookups <= 64, "{lookups} lookups built to read 800 shapes");
        // A hit test asks about each shape once at the most (the front-most that is hit ends it): 800 asks for 100 tests is 80,000 at the most.
        HIT_ASKS.with(|c| c.set(0));
        for _ in 0..100 {
            assert!(scene.hit((10.5, 5.0), 0.05).is_some());
        }
        let asked = HIT_ASKS.with(std::cell::Cell::get);
        assert!((100..=100 * 800).contains(&asked), "{asked} shapes asked about by 100 hit tests of 800 shapes");
        let marks = scene.items.iter().filter(|i| i.kind != Kind::Connector).take(300).map(|i| scene.points(doc, i.id).len()).sum::<usize>();
        eprintln!("read 800 shapes with {lookups} lookups; 100 hit tests asked about {asked} shapes; the points of 300 shapes ({marks})");
    }
}
