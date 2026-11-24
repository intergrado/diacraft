//! Size & Position: the pin of a shape, giving several shapes one size, and reading where a shape is for the window's fields.
//!
//! The rules are named once, in `docs/editing-commands.md`, and here:
//!
//! * Our rule. The pin is the point of the
//!   shape that `PinX` and `PinY` name, and the point it turns about. [`pin_shapes`] puts it at one of the nine places of the shape's box, or at a point
//!   of the shape's own frame, **and the shape stays where it is on the page**: `LocPinX` and `LocPinY` change, and `PinX` and `PinY` follow by the
//!   shape's turn and flips (the pin moves by the change of the local pin carried through the pose, `Pose::up_vector`, so that every point of the
//!   shape lands where it did). A place is a formula of the size (`Width*0.5`, `Height*1`), the form the cell is created with, so the
//!   pin keeps its place on the box when the shape is resized; a point of the shape's own frame is a value, which stays where it is. The places are
//!   those of the shape's **own frame**, before its turn and its flips (a shape turned a quarter turn has its "top-left" where its own top-left
//!   went), so the name a shape reads back is the one it was given. A pin already at the place is left as it is, formula and all. A line or a
//!   connector has no pin of its own (its pin is where its ends put it) and is refused. A group and a member of a turned or flipped group
//!   take it: the shape does not move in its parent's frame, so no frame is crossed. The shape draws where it did, so **no route is made again and
//!   Auto Size is not asked** (the cells written and what reads them are worked out, `settle_kept`).
//! * (ours; "typing into a selection of several makes them the same size").
//!   [`size_shapes`] writes `Width` and `Height` of each shape named and nothing else, so **each shape keeps its pin**: `PinX`
//!   and `PinY` stay, `LocPinX` and `LocPinY` follow when they are formulas of the size (the pin stays where it is on the box) and stay when they
//!   are values (the pin stays where it is on the page and the box grows from it). The sizes are along the shape's own axes, so a turned or flipped
//!   shape keeps its `Angle`, its flips and its pin (the width and height, with the pin where it is and no corner given). The cells are
//!   lengths, which a turn or a flip of a group does not change, so a member of a turned or flipped group is sized (`shape.resize` refuses it, for
//!   the corner it takes is a point of the page). A line and a connector are refused, in words, as `shape.resize` refuses them, and when
//!   one of the shapes is refused nothing changes. **A group is sized as one shape and its members follow** (ours: the same stretch
//!   of its frame as `shape.resize` gives it): a member on a locked layer refuses it, and a shape named beside a group it is in is
//!   refused too, since the group's size would carry it a second time. A shape that has the size already is not written. Connectors glued to a shape sized follow it and
//!   are routed again, as for a resize; only the shapes a connector is glued to are handed to the routing (a shape with no `Connect` row can move no
//!   connector, and the routing looks each shape it is handed up from the top of the page).
//! * Our rule. [`placements`] reads what the window's fields show: a shape's pin **on the page** (a member's through its groups), its size,
//!   `Angle` as degrees from 0 up to 360, its local pin and which of the nine places that is, its flips and whether it holds members; for a line the
//!   place of each end on the page, what each is glued to, its length and the direction from the begin to the end. Lengths are in the page's drawing
//!   units, as every `draw` function has them; the command turns them into paper inches.

use std::collections::{HashMap, HashSet};

use diacraft_calc::Changes;
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, ShapeRef, SheetRef, Value};

use super::arrange::{SAME_PLACE, tidy_angle};
use super::layers::refuse_locked_member;
use super::read::Reader;
use super::route::P;
use super::scale::{MAX_SCALED_MEMBERS, scale_members_of};
use super::scan;
use super::settle::{settle, settle_kept};
use super::{Problem, checked, write_cells, write_length};
use crate::stencil::number_text;

/// One of the nine places of a shape's box, in the shape's own frame (y up, so the top is at `Height`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Place {
    /// The nine places, top row first and each row left to right.
    pub const ALL: [Place; 9] = [
        Place::TopLeft,
        Place::TopCenter,
        Place::TopRight,
        Place::MiddleLeft,
        Place::Center,
        Place::MiddleRight,
        Place::BottomLeft,
        Place::BottomCenter,
        Place::BottomRight,
    ];

    /// The name a command takes and answers with.
    pub fn name(self) -> &'static str {
        match self {
            Place::TopLeft => "top-left",
            Place::TopCenter => "top-center",
            Place::TopRight => "top-right",
            Place::MiddleLeft => "middle-left",
            Place::Center => "center",
            Place::MiddleRight => "middle-right",
            Place::BottomLeft => "bottom-left",
            Place::BottomCenter => "bottom-center",
            Place::BottomRight => "bottom-right",
        }
    }

    /// The place a name stands for: [`Place::name`] in any case, with `_` or a space for the hyphen, `centre` for `center`, and `middle-center` for
    /// the middle.
    pub fn parse(text: &str) -> Option<Place> {
        // No place is named in more than this many bytes: a hostile name is not copied.
        if text.len() > 32 {
            return None;
        }
        let name: String = text.trim().chars().map(|c| if matches!(c, '_' | ' ') { '-' } else { c.to_ascii_lowercase() }).collect();
        let name = name.replace("centre", "center");
        if name == "middle-center" {
            return Some(Place::Center);
        }
        Place::ALL.into_iter().find(|p| p.name() == name)
    }

    /// How far along the box the place is, as fractions of the width and of the height: 0, a half or 1.
    fn fractions(self) -> (f64, f64) {
        match self {
            Place::TopLeft => (0.0, 1.0),
            Place::TopCenter => (0.5, 1.0),
            Place::TopRight => (1.0, 1.0),
            Place::MiddleLeft => (0.0, 0.5),
            Place::Center => (0.5, 0.5),
            Place::MiddleRight => (1.0, 0.5),
            Place::BottomLeft => (0.0, 0.0),
            Place::BottomCenter => (0.5, 0.0),
            Place::BottomRight => (1.0, 0.0),
        }
    }
}

/// Where the pin goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinAt {
    /// One of the nine places, held as a formula of the size.
    Place(Place),
    /// A point of the shape's own frame, in the page's drawing units from the corner at (0, 0), held as a value.
    Local(P),
}

/// A length cell with a value, and the formula that gives it when there is one.
fn length_cell(formula: Option<&str>, value: f64) -> Cell {
    let text = Value::from_text(&number_text(value + 0.0));
    match formula {
        Some(formula) => Cell::with_formula(formula, Some(text)).with_unit("IN"),
        None => Cell::constant(text).with_unit("IN"),
    }
}

/// Puts the pin of each shape named at `at`, the shapes staying where they are on the page. → how many shapes changed: a shape whose
/// pin is at the place already is not counted and not written, and when none changes the document is not touched.
///
/// The shapes are found with one walk of the page, whatever their number.
pub fn pin_shapes(doc: &mut Document, page: u32, ids: &[u32], at: PinAt) -> Result<usize, Problem> {
    if let PinAt::Local((x, y)) = at {
        checked("x", x)?;
        checked("y", y)?;
    }
    let (_, found) = scan::named(doc, page, ids)?;
    let mut cells: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let mut pins: Vec<(SheetRef, &'static str, f64)> = Vec::new();
    let mut changed = 0_usize;
    {
        let reader = Reader::new(doc);
        for (id, shape) in &found {
            let sheet = SheetRef::Shape(shape.clone());
            if reader.is_one_d(&sheet) {
                return Err(Problem::failed(format!("shape {id} is a line or a connector: its pin is where its ends put it")));
            }
            let pose = reader.pose(&sheet);
            let (width, height) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
            let (want, formulas) = match at {
                PinAt::Place(place) => {
                    let (fx, fy) = place.fractions();
                    ((width * fx, height * fy), Some((format!("Width*{}", number_text(fx)), format!("Height*{}", number_text(fy)))))
                }
                PinAt::Local(point) => (point, None),
            };
            let (moves_x, moves_y) = ((want.0 - pose.loc.0).abs() > SAME_PLACE, (want.1 - pose.loc.1).abs() > SAME_PLACE);
            if !moves_x && !moves_y {
                continue;
            }
            changed = changed.saturating_add(1);
            // The change of the local pin, and the same change as it is in the shape's parent (turned and flipped): the pin moves by that.
            let by = (if moves_x { want.0 - pose.loc.0 } else { 0.0 }, if moves_y { want.1 - pose.loc.1 } else { 0.0 });
            let carried = pose.up_vector(by);
            if moves_x {
                let formula = formulas.as_ref().map(|f| f.0.as_str());
                cells.push((sheet.clone(), CellAddr::direct("LocPinX"), CellEdit::Cell(length_cell(formula, want.0))));
            }
            if moves_y {
                let formula = formulas.as_ref().map(|f| f.1.as_str());
                cells.push((sheet.clone(), CellAddr::direct("LocPinY"), CellEdit::Cell(length_cell(formula, want.1))));
            }
            for (name, was, by) in [("PinX", pose.pin.0, carried.0), ("PinY", pose.pin.1, carried.1)] {
                if by.abs() > SAME_PLACE {
                    pins.push((sheet.clone(), name, was + by));
                }
            }
        }
    }
    if changed == 0 {
        return Ok(0);
    }
    let mut changes = Changes::default();
    for (sheet, addr, _) in &cells {
        changes.cells.push((sheet.clone(), addr.clone()));
    }
    for (sheet, name, _) in &pins {
        changes.cells.push((sheet.clone(), CellAddr::direct(name)));
    }
    write_cells(doc, cells)?;
    for (sheet, name, value) in pins {
        write_length(doc, &sheet, name, checked(name, value)?)?;
    }
    settle_kept(doc, page, &changes)?;
    Ok(changed)
}

/// The `ID`s that a `Connect` row of the page names as the shape glued to: the only shapes that can move a connector.
fn glued_to(doc: &Document, page: u32) -> HashSet<u32> {
    doc.page(page).map(|p| p.connects.iter().map(|c| c.to_sheet).collect()).unwrap_or_default()
}

/// Gives each shape named the width `width` and the height `height`, whichever of them is given, along the shape's own axes, each shape keeping its
/// pin, in the page's drawing units. → how many shapes changed: a shape that has the size already is not counted and not written.
/// A group that [`size_shapes`] changes the size of: where it is, and the size its members are carried from and to.
struct SizedGroup {
    id: u32,
    at: ShapeRef,
    old: (f64, f64),
    new: (f64, f64),
}

/// A line or a connector among the shapes is refused and nothing changes. **A group is sized as one shape and its members follow**: their numbers go through the
/// stretch of the group's frame (`draw/scale.rs`), within the members one command carries (`MAX_SCALED_MEMBERS`).
///
/// The shapes are found with one walk of the page, whatever their number.
pub fn size_shapes(doc: &mut Document, page: u32, ids: &[u32], width: Option<f64>, height: Option<f64>) -> Result<usize, Problem> {
    if width.is_none() && height.is_none() {
        return Err(Problem::bad("give `width`, `height` or both"));
    }
    for (name, value) in [("width", width), ("height", height)] {
        if let Some(value) = value
            && checked(name, value)? <= 0.0
        {
            return Err(Problem::bad("`width` and `height` must be more than 0"));
        }
    }
    let before = doc.clone();
    let (_, found) = scan::named(doc, page, ids)?;
    let mut plan: Vec<(SheetRef, &'static str, f64)> = Vec::new();
    let mut sized: Vec<u32> = Vec::new();
    // The groups that change size: where each is, and the size it goes from and to (its members follow).
    let mut groups: Vec<SizedGroup> = Vec::new();
    {
        let reader = Reader::new(doc);
        for (id, shape) in &found {
            let sheet = SheetRef::Shape(shape.clone());
            if reader.is_one_d(&sheet) {
                return Err(Problem::failed(format!("shape {id} is a line or a connector: it is sized by its ends")));
            }
            // In document order the members of a group follow it, so the last group that changes is the only one that can hold this shape: the group's size carries it, and a
            // size written here too would be carried a second time.
            if let Some(outer) = groups.last()
                && shape.path.len() > outer.at.path.len()
                && shape.path.starts_with(&outer.at.path)
            {
                return Err(Problem::failed(format!(
                    "shape {id} is in group {}, which is sized in the same command: its members follow the group, so name one of them or the group",
                    outer.id
                )));
            }
            let (old_width, old_height) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
            let planned = plan.len();
            for (name, wanted) in [("Width", width), ("Height", height)] {
                if let Some(wanted) = wanted
                    && (reader.number(&sheet, name) - wanted).abs() > SAME_PLACE
                {
                    plan.push((sheet.clone(), name, wanted));
                }
            }
            if plan.len() > planned {
                sized.push(*id);
                if doc.shape(shape).is_some_and(|s| !s.children.is_empty()) {
                    groups.push(SizedGroup {
                        id: *id,
                        at: shape.clone(),
                        old: (old_width, old_height),
                        new: (width.unwrap_or(old_width), height.unwrap_or(old_height)),
                    });
                }
            }
        }
    }
    if sized.is_empty() {
        return Ok(0);
    }
    // A group's members follow its size: one on a locked layer is not sized through it.
    if !groups.is_empty() {
        let named: Vec<u32> = groups.iter().map(|g| g.id).collect();
        refuse_locked_member(doc, page, &named, "resized")?;
    }
    for (sheet, name, value) in plan {
        write_length(doc, &sheet, name, value)?;
    }
    // The members of each group that changed go with its frame: the numbers they hold are stretched, and the lines among them are routed again.
    let mut left = MAX_SCALED_MEMBERS;
    let mut member_lines: Vec<u32> = Vec::new();
    for g in &groups {
        scale_members_of(doc, &g.at, g.old, g.new, &mut left, &mut member_lines)?;
    }
    // Only a shape that a connector is glued to can move one: the others are not handed to the routing, which looks each up from the top of the page.
    let glued = glued_to(doc, page);
    let touched: Vec<u32> = sized.iter().copied().filter(|id| glued.contains(id)).collect();
    settle(doc, &before, page, &touched, &member_lines)?;
    Ok(sized.len())
}

// ---- reading -------------------------------------------------------------------------------------------------------------------------

/// What an end of a line is glued to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glue {
    /// The shape.
    pub id: u32,
    /// Its connection point, counted from 0, or `None` for an end glued to the whole of the shape.
    pub point: Option<u32>,
}

/// An end of a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineEnd {
    /// Where it is on the page.
    pub at: P,
    pub glued: Option<Glue>,
}

/// A line or a connector: the shape has all four of `BeginX`, `BeginY`, `EndX` and `EndY`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFacts {
    pub begin: LineEnd,
    pub end: LineEnd,
    /// The distance between the ends.
    pub length: f64,
}

/// Where a shape is, for the Size & Position window, in the page's drawing units.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapePlacement {
    pub id: u32,
    /// The shape's name (`Name`, else `NameU`), and `Sheet.<id>` for a shape with none, as `NAME()` answers; at most
    /// [`MAX_NAME_CHARS`] characters.
    pub name: String,
    /// Whether the shape is a line or a connector.
    pub one_d: bool,
    /// Whether the shape holds members.
    pub group: bool,
    /// The pin on the page.
    pub pin: P,
    /// The pin in the shape's own frame (`LocPinX`, `LocPinY`).
    pub local_pin: P,
    /// Which of the nine places the local pin is, `None` for any other point (and for a line, which has no pin of its own).
    pub place: Option<Place>,
    pub width: f64,
    pub height: f64,
    /// The angle in degrees from 0 up to 360, counterclockwise: the cell `Angle` for a shape, and for a line the way it points from its begin to its
    /// end (the cell `Angle` when the ends are one point).
    pub angle: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub line: Option<LineFacts>,
}

/// The most characters of a shape's name an answer holds: a file's name is as long as the file allows, and 4,096 of them are one answer.
pub const MAX_NAME_CHARS: usize = 256;

/// The name a shape is shown by.
fn name_of(shape: &diacraft_doc::Shape) -> String {
    let named = shape.name.as_deref().filter(|n| !n.is_empty()).or_else(|| shape.name_u.as_deref().filter(|n| !n.is_empty()));
    named.map_or_else(|| format!("Sheet.{}", shape.id), |name| name.chars().take(MAX_NAME_CHARS).collect())
}

/// An angle in radians as degrees from 0 up to 360, with the noise of the arithmetic taken off.
fn degrees_to_360(radians: f64) -> f64 {
    let d = tidy_angle(radians).to_degrees();
    let d = if d < 0.0 { d + 360.0 } else { d };
    let d = (d * 1.0e9).round() / 1.0e9;
    if d.is_finite() && (0.0..360.0).contains(&d) { d + 0.0 } else { 0.0 }
}

/// Which of the nine places a local pin is, for a box of this size.
fn place_of(local_pin: P, width: f64, height: f64) -> Option<Place> {
    // The cells hold nine places; a pin within a millionth of the place is at it.
    const NEAR: f64 = 1.0e-6;
    Place::ALL.into_iter().find(|p| {
        let (fx, fy) = p.fractions();
        (local_pin.0 - width * fx).abs() <= NEAR && (local_pin.1 - height * fy).abs() <= NEAR
    })
}

/// What the `Connect` rows of a page say each end of the lines in `wanted` is glued to: keyed by the line and whether the end is its end (and not its
/// begin). One pass over the rows, whatever the number of lines.
fn glue_of(doc: &Document, page: u32, wanted: &HashSet<u32>) -> HashMap<(u32, bool), Glue> {
    let mut out = HashMap::new();
    let Some(p) = doc.page(page) else { return out };
    for c in p.connects.iter().filter(|c| wanted.contains(&c.from_sheet)) {
        let is_end = match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => false,
            Some("EndX" | "EndY") => true,
            _ => continue,
        };
        let point = c.to_part.and_then(|part| part.checked_sub(super::connectors::TO_PART_BASE));
        out.entry((c.from_sheet, is_end)).or_insert(Glue { id: c.to_sheet, point });
    }
    out
}

/// Where the shapes named are, in the order they are named, each once. A shape that is not on the page is an error. The shapes and
/// their glue are found with one walk of the page and one pass over its `Connect` rows, whatever their number.
pub fn placements(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<ShapePlacement>, Problem> {
    let ids = scan::distinct(ids);
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let found = scan::scan(doc, page, &wanted)?;
    let reader = Reader::new(doc);
    let glue = glue_of(doc, page, &wanted);
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let at = found.found.get(&id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
        let sheet = SheetRef::Shape(at.clone());
        let pose = reader.pose(&sheet);
        let (width, height) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
        let one_d = reader.is_one_d(&sheet);
        // A line's ends in its parent's frame, where the cells hold them.
        let ends = one_d.then(|| {
            let end = |x: &str, y: &str| (reader.number(&sheet, x), reader.number(&sheet, y));
            (end("BeginX", "BeginY"), end("EndX", "EndY"))
        });
        let line = ends.map(|(begin, finish)| LineFacts {
            begin: LineEnd { at: reader.parent_to_page(at, begin), glued: glue.get(&(id, false)).copied() },
            end: LineEnd { at: reader.parent_to_page(at, finish), glued: glue.get(&(id, true)).copied() },
            length: (finish.0 - begin.0).hypot(finish.1 - begin.1),
        });
        // A line's angle is the way it points, as `shape.rotate` reads it; a line with both ends at one point has none and keeps its cell.
        let angle = match ends {
            Some((begin, finish)) if (finish.0 - begin.0).hypot(finish.1 - begin.1) > SAME_PLACE => (finish.1 - begin.1).atan2(finish.0 - begin.0),
            _ => pose.angle,
        };
        let shape = doc.shape(at);
        out.push(ShapePlacement {
            id,
            name: shape.map_or_else(|| format!("Sheet.{id}"), |s| name_of(s)),
            one_d,
            group: shape.is_some_and(|s| !s.children.is_empty()),
            pin: reader.parent_to_page(at, pose.pin),
            local_pin: pose.loc,
            place: if one_d { None } else { place_of(pose.loc, width, height) },
            width,
            height,
            angle: degrees_to_360(angle),
            flip_x: pose.flip_x,
            flip_y: pose.flip_y,
            line,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Connect, Connects, Shape, ShapeList};

    use super::*;
    use crate::draw::scan::NODES_WALKED;

    #[test]
    fn a_place_is_read_from_its_name_in_any_case_and_with_either_hyphen_and_the_nine_are_named_once() {
        for place in Place::ALL {
            assert_eq!(Place::parse(place.name()), Some(place));
            assert_eq!(Place::parse(&place.name().to_uppercase()), Some(place));
            assert_eq!(Place::parse(&place.name().replace('-', "_")), Some(place));
            assert_eq!(Place::parse(&place.name().replace('-', " ")), Some(place));
        }
        assert_eq!(Place::parse("centre"), Some(Place::Center));
        assert_eq!(Place::parse("Top-Centre"), Some(Place::TopCenter));
        assert_eq!(Place::parse("middle-center"), Some(Place::Center));
        for no in ["", "top", "left", "top-left-ish", "centre-centre", &"x".repeat(1_000)] {
            assert_eq!(Place::parse(no), None, "{no:?}");
        }
        let names: HashSet<&str> = Place::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(names.len(), 9);
    }

    #[test]
    fn the_nine_places_are_the_corners_the_middles_of_the_sides_and_the_middle_of_a_box_with_the_top_at_the_height() {
        let (w, h) = (4.0, 2.0);
        let at = |p: Place| {
            let (fx, fy) = p.fractions();
            (w * fx, h * fy)
        };
        assert_eq!(at(Place::TopLeft), (0.0, 2.0));
        assert_eq!(at(Place::BottomRight), (4.0, 0.0));
        assert_eq!(at(Place::Center), (2.0, 1.0));
        assert_eq!(at(Place::MiddleRight), (4.0, 1.0));
        assert_eq!(at(Place::BottomCenter), (2.0, 0.0));
        for p in Place::ALL {
            assert_eq!(place_of(at(p), w, h), Some(p));
        }
        assert_eq!(place_of((1.0, 1.0), w, h), None);
    }

    #[test]
    fn an_angle_is_shown_from_0_up_to_360_and_a_turn_back_to_zero_is_zero() {
        assert_eq!(degrees_to_360(0.0), 0.0);
        assert_eq!(degrees_to_360(-0.0), 0.0);
        assert_eq!(degrees_to_360(std::f64::consts::FRAC_PI_2), 90.0);
        assert_eq!(degrees_to_360(-std::f64::consts::FRAC_PI_2), 270.0);
        assert_eq!(degrees_to_360(std::f64::consts::PI), 180.0);
        assert_eq!(degrees_to_360(std::f64::consts::TAU), 0.0);
        assert_eq!(degrees_to_360(-1.0e-12), 0.0, "a hair under a turn is no turn");
        // A file holds thirty degrees to thirteen places of radians.
        let thirty_in_a_file = (30.0_f64.to_radians() * 1.0e13).round() / 1.0e13;
        assert_eq!(degrees_to_360(thirty_in_a_file), 30.0, "thirteen places of radians are thirty degrees");
        assert_eq!(degrees_to_360(f64::NAN), 0.0);
        assert_eq!(degrees_to_360(f64::INFINITY), 0.0);
        for radians in [-3.0, -0.001, 0.001, 3.0, 6.2] {
            let d = degrees_to_360(radians);
            assert!((0.0..360.0).contains(&d), "{radians} -> {d}");
        }
    }

    /// A page of `n` rectangles with the `ID`s 1 to `n`, none glued to anything, on a blank document.
    fn page_of(n: u32) -> Document {
        let mut doc = Document::new_blank();
        if let Some(page) = crate::draw::page_mut(&mut doc, 1) {
            let mut shapes = Vec::with_capacity(n as usize);
            for id in 1..=n {
                let mut sheet = diacraft_doc::Sheet::default();
                for (name, value) in [("PinX", "5"), ("PinY", "5"), ("Width", "2"), ("Height", "1")] {
                    sheet.cells.insert(diacraft_doc::CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
                }
                sheet
                    .cells
                    .insert(diacraft_doc::CellName::new("LocPinX"), Cell::with_formula("Width*0.5", Some(Value::from_text("1"))).with_unit("IN"));
                sheet
                    .cells
                    .insert(diacraft_doc::CellName::new("LocPinY"), Cell::with_formula("Height*0.5", Some(Value::from_text("0.5"))).with_unit("IN"));
                let mut shape = Shape::new(id);
                shape.sheet = Arc::new(sheet);
                shapes.push(Arc::new(shape));
            }
            page.shapes = ShapeList::from_vec(shapes);
        }
        doc
    }

    #[test]
    fn the_pin_of_four_thousand_ninety_six_shapes_on_a_page_of_twenty_thousand_is_found_with_one_walk_and_no_search_for_a_shape() {
        // Counted, not timed: the shapes are found by one walk of the page (a visit to each shape of it), and no shape is looked for from the top of
        // the page by its `ID` (`Reader::find`), whatever the number named.
        let n = 20_000_u32;
        let mut doc = page_of(n);
        let ids: Vec<u32> = (1..=4_096).collect();
        NODES_WALKED.with(|c| c.set(0));
        crate::draw::read::FINDS.with(|c| c.set(0));
        let pinned = pin_shapes(&mut doc, 1, &ids, PinAt::Place(Place::TopLeft)).unwrap();
        assert_eq!(pinned, 4_096);
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), n as usize, "one visit to each shape of the page");
        assert_eq!(crate::draw::read::FINDS.with(std::cell::Cell::get), 0, "no search from the top of the page for a shape");
        // And the same pin again is nothing to write: the document is as it was.
        let again = doc.clone();
        assert_eq!(pin_shapes(&mut doc, 1, &ids, PinAt::Place(Place::TopLeft)).unwrap(), 0);
        assert_eq!(doc, again);
    }

    #[test]
    fn the_size_of_four_thousand_ninety_six_shapes_on_a_page_of_twenty_thousand_is_one_walk_and_hands_the_routing_no_shape_that_has_no_connector() {
        let n = 20_000_u32;
        let mut doc = page_of(n);
        let ids: Vec<u32> = (1..=4_096).collect();
        NODES_WALKED.with(|c| c.set(0));
        crate::draw::read::FINDS.with(|c| c.set(0));
        let sized = size_shapes(&mut doc, 1, &ids, Some(3.0), None).unwrap();
        assert_eq!(sized, 4_096);
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), n as usize, "one visit to each shape of the page");
        assert_eq!(
            crate::draw::read::FINDS.with(std::cell::Cell::get),
            0,
            "no shape of the 4,096 is looked up from the top of the page: none has a connector"
        );
    }

    #[test]
    fn the_routing_is_handed_the_shapes_a_connector_is_glued_to_and_no_others() {
        // 3 rectangles; a connector row glues connector 4 (not on the page: a row alone) to shape 2.
        let mut doc = page_of(3);
        if let Some(page) = crate::draw::page_mut(&mut doc, 1) {
            page.connects =
                Connects::from_vec(vec![Connect { from_sheet: 4, from_cell: Some(Arc::from("BeginX")), to_sheet: 2, ..Connect::default() }]);
        }
        assert_eq!(glued_to(&doc, 1), HashSet::from([2]));
        assert!(glued_to(&doc, 9).is_empty(), "a page that is not there");
    }

    #[test]
    fn the_placements_of_four_thousand_ninety_six_shapes_are_one_walk() {
        let n = 20_000_u32;
        let doc = page_of(n);
        let ids: Vec<u32> = (1..=4_096).collect();
        NODES_WALKED.with(|c| c.set(0));
        crate::draw::read::FINDS.with(|c| c.set(0));
        let all = placements(&doc, 1, &ids).unwrap();
        assert_eq!(all.len(), 4_096);
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), n as usize);
        assert_eq!(crate::draw::read::FINDS.with(std::cell::Cell::get), 0);
        assert!(all.iter().all(|p| p.place == Some(Place::Center) && p.pin == (5.0, 5.0) && p.angle == 0.0));
        let err = placements(&doc, 1, &[1, n + 1]).unwrap_err();
        assert!(err.bad_params && err.message.contains("no shape"), "{err:?}");
    }
}
