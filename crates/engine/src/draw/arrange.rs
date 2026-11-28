//! Arrange: rotate, flip, drawing order and align.
//!
//! Every function here edits a [`Document`] as the other `draw` functions do, so a command makes one undo step of it. The rules are named once,
//! in `docs/editing-commands.md`, and here:
//!
//! * Our rule (give the cell, the order of the steps). A turn writes `Angle` and, for
//!   a set turned by an angle, the pins. `by` adds an angle; one shape turns about its pin, **several turn as one about the middle of their joint
//!   box** (the box on the page that holds all their boxes): "several shapes turn as if the page turned". `angle`
//!   sets the cell, each shape about its own pin. A turn is kept from just over -180 up to 180 degrees (the files
//!   hold 34,207 negative `Angle` cells and 323 larger than pi in size, so they hold -pi to pi), and a turn that comes within a billionth of a radian
//!   of a quarter is that quarter. Positive is counterclockwise.
//! * Our rule (leaves open what a flip writes). A flip is a mirror of the page picture, across the vertical
//!   (horizontal flip) or horizontal (vertical flip) line through the middle of the joint box of the shapes, which for one shape is its own box.
//!   The shape's pin is mirrored with it, `FlipX` (or `FlipY`) is toggled and `Angle` changes sign: `M·R(θ)·F = R(-θ)·M·F` for the mirror `M`, so
//!   the mirrored shape is the shape of the picture. The box stays where it is, a pin that is not at the middle moves with the mirror, and text is
//!   never mirrored: the cells that turn it are not touched.
//! * **ROTATE-1D-1** (ours, after **MOVE-1D**): a one-dimensional shape (a connector) turns or flips its **ends that are not glued**, and writes no
//!   pose cell: its `Angle`, `Width` and pin follow the ends by their formulas, and a route DiaCraft wrote is made again. With no end glued a turn is
//!   about its pin (one shape) or the joint box (a set turned `by`). **With one end glued the free end turns about the glued end**, so that the line
//!   turns by the angle and `angle` sets the way it points from its begin to its end exactly ("a line takes the angle from its
//!   begin point"); a set turned `by` turns its free ends about the joint box like everything else. A glued end stays with its target. A connector
//!   with both ends glued has nothing to turn.
//! * Our rule (document order is drawing order): the order of a list is the order of the shapes in it, and
//!   the commands reorder that list (the page's, or a group's own for a member). Front and back go to the end of the shape's *band*
//!   The shape stays behind every shape of a higher `DisplayLevel` and in front of every shape of a
//!   lower one, so it is put just before the first shape after it with a higher band, or after the last before it with a lower band. Forward and
//!   backward move one place, past the next shape that is not named and not in the way of the band. Shapes named together keep the order they
//!   have among themselves. A shape of the reserved band -32768 has none to keep: it goes to the very front or back, and neither stops another.
//!   No command writes `DisplayLevel`, so the -32768 that writes for a command that carries a shape out of its band does not arise.
//! * (ours; "left, centre, right, top, middle and bottom move the others to the primary shape without
//!   changing spacing"): the first shape named is the primary one and stays; each other shape moves by the distance on the page that puts its edge
//!   (or its middle) where the primary's is. A shape's edge is that of its **box on the page** (the bounding box of its box carried through its pose
//!   and its groups', the box and not the drawn curve; for a connector the box of its ends), and a move is a move of
//!   [`super::move_shapes`]'s kind (**MOVE-1D**).

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, Shape, ShapeList, ShapeRef, SheetRef, Value, WriteOrigin, WriteOutcome};
use diacraft_units::{Quantity, Unit};

use super::read::Reader;
use super::route::P;
use super::scan::{Named, distinct};
use super::settle::settle;
use super::shapes::move_each;
use super::{Problem, checked, page_mut, shape_mut, write_length, write_quantity};

/// The most a turn may be, in radians: past any angle a person types, short of arithmetic that has lost its digits.
pub(super) const MAX_TURN: f64 = 1.0e5;

/// Lengths closer than this are the same place (the cells hold nine places).
pub(super) const SAME_PLACE: f64 = 1.0e-9;

/// Angles closer than this, in radians, are the same angle, and an angle closer than this to a quarter turn is that quarter turn. One number for
/// both, so that a turn of nothing on an angle that a turn would have snapped writes nothing.
const SAME_ANGLE: f64 = 1.0e-9;

/// `DisplayLevel` -32768: no band.
const RESERVED_BAND: f64 = -32_768.0;

/// How a shape is turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Turn {
    /// To this angle in radians, each shape about its pin.
    To(f64),
    /// By this angle in radians, counterclockwise: one shape about its pin, several as one about the middle of their joint box.
    By(f64),
}

/// The line a flip mirrors across.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipAxis {
    /// Left to right: a mirror across a vertical line (`FlipX`).
    Horizontal,
    /// Top to bottom: a mirror across a horizontal line (`FlipY`).
    Vertical,
}

/// A change of drawing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    Front,
    Back,
    Forward,
    Backward,
}

/// An edge or middle to align on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

// ---- the shapes a command is about ------------------------------------------------------------------------------------------------

/// A shape an arrange command works on.
pub(super) struct Target {
    pub(super) id: u32,
    pub(super) at: ShapeRef,
    pub(super) sheet: SheetRef,
    pub(super) one_d: bool,
}

/// The shapes named, each once, in the order they are named (the first of a repeated `ID` counts): every one must be on the page. They are found in one walk of the
/// page, where a search for each walked the page from its top for each, and the first one named that is missing is refused as it was.
pub(super) fn targets(doc: &Document, reader: &Reader<'_>, page: u32, ids: &[u32]) -> Result<Vec<Target>, Problem> {
    let ids = distinct(ids);
    let named = Named::of(doc, page, ids.iter().copied())?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let at = named.require(id)?;
        let sheet = SheetRef::Shape(at.clone());
        out.push(Target { id, one_d: reader.is_one_d(&sheet), sheet, at });
    }
    Ok(out)
}

/// Whether `inner` is inside `outer`: a member, or a member of a member, of a group.
fn is_inside(inner: &ShapeRef, outer: &ShapeRef) -> bool {
    inner.path.len() > outer.path.len() && inner.path.starts_with(&outer.path)
}

/// The targets that are not inside another target. A group that is turned, flipped or moved takes its members with it, so a member named with
/// its group would be moved twice: it is left to the group.
pub(super) fn outermost(targets: Vec<Target>) -> Vec<Target> {
    let paths: BTreeSet<Vec<u32>> = targets.iter().map(|t| t.at.path.clone()).collect();
    targets.into_iter().filter(|t| !inside_one_of(&t.at, &paths)).collect()
}

/// Whether a shape is inside a shape whose path is in `paths`.
fn inside_one_of(at: &ShapeRef, paths: &BTreeSet<Vec<u32>>) -> bool {
    (1..at.path.len()).any(|k| at.path.get(..k).is_some_and(|prefix| paths.contains(prefix)))
}

/// A turn or a flip is about the page, and a member of a turned or flipped group has its cells in the group's frame: refused for now, as
/// The group frame rule refuses its resize.
pub(super) fn refuse_turned_groups(reader: &Reader<'_>, targets: &[Target], verb: &str) -> Result<(), Problem> {
    if targets.iter().any(|t| reader.groups_turn(&t.at)) {
        return Err(Problem::failed(format!("a member of a turned or flipped group cannot be {verb} yet")));
    }
    Ok(())
}

/// Where a shape is on the page, as the points that make its box: its four corners, or for a one-dimensional shape its two ends.
pub(super) fn page_points(reader: &Reader<'_>, t: &Target) -> Vec<P> {
    if t.one_d {
        let end = |x: &str, y: &str| reader.parent_to_page(&t.at, (reader.number(&t.sheet, x), reader.number(&t.sheet, y)));
        vec![end("BeginX", "BeginY"), end("EndX", "EndY")]
    } else {
        reader.corners(&t.at).to_vec()
    }
}

/// The box on the page that holds some points: (left, bottom, right, top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub(super) x0: f64,
    pub(super) y0: f64,
    pub(super) x1: f64,
    pub(super) y1: f64,
}

impl Rect {
    pub(super) fn of(points: impl IntoIterator<Item = P>) -> Result<Rect, Problem> {
        let mut rect: Option<Rect> = None;
        for p in points {
            if !(p.0.is_finite() && p.1.is_finite()) {
                return Err(Problem::failed("a shape's box is not a number"));
            }
            rect = Some(match rect {
                None => Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 },
                Some(r) => Rect { x0: r.x0.min(p.0), y0: r.y0.min(p.1), x1: r.x1.max(p.0), y1: r.y1.max(p.1) },
            });
        }
        rect.ok_or_else(|| Problem::failed("there is nothing to take the box of"))
    }

    pub(super) fn middle(&self) -> P {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }
}

/// The joint box of the targets on the page.
pub(super) fn joint_box(reader: &Reader<'_>, targets: &[Target]) -> Result<Rect, Problem> {
    Rect::of(targets.iter().flat_map(|t| page_points(reader, t)))
}

// ---- cells ---------------------------------------------------------------------------------------------------------------------------

/// How a planned write is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Length,
    Angle,
    Flag,
}

/// One cell to write under a shape.
pub(super) struct Write {
    pub(super) sheet: SheetRef,
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    pub(super) value: f64,
}

/// An angle as a cell holds it: from just over -pi up to pi (the range the files hold), the noise of the arithmetic taken off, and a
/// quarter turn exact, with a whole turn no turn and a zero without a sign.
pub(super) fn tidy_angle(a: f64) -> f64 {
    let wrapped = a.rem_euclid(TAU);
    let quarters = (wrapped / FRAC_PI_2).round();
    if (wrapped - quarters * FRAC_PI_2).abs() < SAME_ANGLE {
        // A quarter turn, as the exact number of its place in the range (0 to 4 quarters: 4 is a whole turn).
        return match quarters as i64 % 4 {
            1 => FRAC_PI_2,
            2 => PI,
            3 => -FRAC_PI_2,
            _ => 0.0,
        };
    }
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// Whether two angles are the same turn, whatever whole turns they differ by.
pub(super) fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d < SAME_ANGLE || TAU - d < SAME_ANGLE
}

/// An angle as the text of a cell: thirteen places, as the files of the application that writes them hold an angle, where a length keeps nine.
pub(crate) fn angle_text(a: f64) -> String {
    let rounded = (a * 1.0e13).round() / 1.0e13;
    Value::from_number(if rounded.is_finite() { rounded + 0.0 } else { 0.0 }).map_or_else(|| "0".to_owned(), |v| v.text().to_owned())
}

/// Whether a formula is `BOUND(...)` or `SETATREF(...)`, which keep themselves and pass a written number on.
fn passes_the_number_on(formula: &str) -> bool {
    let head = formula.trim_start().to_ascii_uppercase();
    head.starts_with("BOUND(") || head.starts_with("SETATREF(")
}

/// Writes `Angle` as a drag does, with all the digits a file holds; a formula that passes the number on gets it by [`write_quantity`], and a
/// guarded cell is an error.
fn write_angle(doc: &mut Document, sheet: &SheetRef, radians: f64) -> Result<(), Problem> {
    write_angle_cell(doc, sheet, "Angle", radians)
}

/// [`write_angle`] for any cell that holds an angle (`TxtAngle`).
pub(super) fn write_angle_cell(doc: &mut Document, sheet: &SheetRef, name: &str, radians: f64) -> Result<(), Problem> {
    let addr = CellAddr::direct(name);
    let passes_on = {
        let reader = Reader::new(doc);
        reader
            .resolver()
            .resolve(sheet, &addr)
            .and_then(|found| found.formula.cell)
            .is_some_and(|cell| cell.formula.text().is_some_and(passes_the_number_on))
    };
    if passes_on {
        return write_quantity(doc, sheet, name, &Quantity::new(radians, Unit::Radian));
    }
    match doc.write_cell(sheet, &addr, CellEdit::Value(Value::from_text(&angle_text(radians))), WriteOrigin::Drawing)? {
        WriteOutcome::Written => Ok(()),
        WriteOutcome::KeptGuarded | WriteOutcome::Skipped => {
            Err(Problem::failed(format!("`{name}` is protected by GUARD and cannot be changed by a turn")))
        }
    }
}

pub(super) fn apply(doc: &mut Document, writes: Vec<Write>) -> Result<(), Problem> {
    for w in writes {
        match w.kind {
            Kind::Length => write_length(doc, &w.sheet, w.name, checked(w.name, w.value)?)?,
            Kind::Angle => write_angle(doc, &w.sheet, w.value)?,
            Kind::Flag => write_quantity(doc, &w.sheet, w.name, &Quantity::new(w.value, Unit::Unitless))?,
        }
    }
    Ok(())
}

/// Plans the writes that take a pin from `old` to `new` (both in the parent's frame): the coordinates that differ.
fn plan_pin(writes: &mut Vec<Write>, t: &Target, old: P, new: P) {
    for (name, was, now) in [("PinX", old.0, new.0), ("PinY", old.1, new.1)] {
        if (now - was).abs() > SAME_PLACE {
            writes.push(Write { sheet: t.sheet.clone(), name, kind: Kind::Length, value: now });
        }
    }
}

fn plan_angle(writes: &mut Vec<Write>, t: &Target, old: f64, new: f64) {
    let new = tidy_angle(new);
    if !same_angle(new, old) {
        writes.push(Write { sheet: t.sheet.clone(), name: "Angle", kind: Kind::Angle, value: new });
    }
}

/// Which ends of connector `id` are glued: (begin, end).
pub(super) fn glued_ends(doc: &Document, page: u32, id: u32) -> (bool, bool) {
    let Some(p) = doc.page(page) else { return (false, false) };
    let has = |cells: [&str; 2]| p.connects.iter().any(|c| c.from_sheet == id && c.from_cell.as_deref().is_some_and(|name| cells.contains(&name)));
    (has(["BeginX", "BeginY"]), has(["EndX", "EndY"]))
}

/// Plans the move of the ends of a one-dimensional shape that are not glued, each carried by `carry` (a page point to a page point).
pub(super) fn plan_line(doc: &Document, page: u32, reader: &Reader<'_>, t: &Target, carry: &dyn Fn(P) -> P, writes: &mut Vec<Write>) {
    let (begin_glued, end_glued) = glued_ends(doc, page, t.id);
    for (glued, x_cell, y_cell) in [(begin_glued, "BeginX", "BeginY"), (end_glued, "EndX", "EndY")] {
        if glued {
            continue;
        }
        let old = (reader.number(&t.sheet, x_cell), reader.number(&t.sheet, y_cell));
        let new = reader.point_from_page(&t.at, carry(reader.parent_to_page(&t.at, old)));
        for (name, was, now) in [(x_cell, old.0, new.0), (y_cell, old.1, new.1)] {
            if (now - was).abs() > SAME_PLACE {
                writes.push(Write { sheet: t.sheet.clone(), name, kind: Kind::Length, value: now });
            }
        }
    }
}

/// A point turned counterclockwise by `by` radians about `c`.
fn turn_about(c: P, by: f64, p: P) -> P {
    let (s, k) = by.sin_cos();
    let (x, y) = (p.0 - c.0, p.1 - c.1);
    (c.0 + x * k - y * s, c.1 + x * s + y * k)
}

/// A point mirrored across the vertical line `x = c` (a horizontal flip) or the horizontal line `y = c`.
fn mirror(axis: FlipAxis, c: f64, p: P) -> P {
    match axis {
        FlipAxis::Horizontal => (2.0 * c - p.0, p.1),
        FlipAxis::Vertical => (p.0, 2.0 * c - p.1),
    }
}

// ---- rotate ------------------------------------------------------------------------------------------------------------------------

/// What a turn did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turned {
    /// How many shapes were turned: a shape whose angle is the angle it is turned to, or whose ends are both glued, is not counted.
    pub count: usize,
    /// The shapes left alone because their `LockRotate` is set, in the order they were named.
    pub locked: Vec<u32>,
}

/// Turns shapes (**ROTATE-1D-1**). → how many were turned: a shape whose angle is the angle it is turned to, or whose ends are
/// both glued, is not counted, and when none is the document is not touched. A shape whose `LockRotate` is set is left alone
/// ([`turn_shapes`] says which).
pub fn rotate_shapes(doc: &mut Document, page: u32, ids: &[u32], turn: Turn) -> Result<usize, Problem> {
    turn_shapes(doc, page, ids, turn, None).map(|turned| turned.count)
}

/// Turns shapes (**ROTATE-1D-1**). A turn `by` an angle with `about` a page point turns each shape's pin about that point (a shape's own
/// angle goes up by the angle); without `about`, one shape turns about its pin and several as one about the middle of their joint box. **A shape whose `LockRotate` is set
/// is left alone** (it is neither turned nor moved, and its box still counts in the joint box the others turn about), and when every shape named is locked the turn is
/// refused in words. → how many were turned and which were left alone; when none turned, the document is not touched.
pub fn turn_shapes(doc: &mut Document, page: u32, ids: &[u32], turn: Turn, about: Option<P>) -> Result<Turned, Problem> {
    let before = doc.clone();
    let (Turn::To(value) | Turn::By(value)) = turn;
    if !value.is_finite() || value.abs() > MAX_TURN {
        return Err(Problem::bad("the angle must be a number of a few turns at most"));
    }
    if let Some(c) = about {
        if matches!(turn, Turn::To(_)) {
            return Err(Problem::bad("`about` goes with `by`: an `angle` turns each shape about its own pin"));
        }
        checked("about.x", c.0)?;
        checked("about.y", c.1)?;
    }
    // Whole turns are no turn: kept out of the arithmetic, which loses a digit for each decade of them.
    let turn = match turn {
        Turn::To(a) => Turn::To(a.rem_euclid(TAU)),
        Turn::By(d) => Turn::By(d.rem_euclid(TAU)),
    };
    let (writes, count, touched, locked) = {
        let reader = Reader::new(doc);
        let targets = outermost(targets(doc, &reader, page, ids)?);
        refuse_turned_groups(&reader, &targets, "rotated")?;
        let locked: Vec<u32> = targets.iter().filter(|t| reader.number(&t.sheet, "LockRotate") != 0.0).map(|t| t.id).collect();
        if !targets.is_empty() && locked.len() == targets.len() {
            let named: Vec<String> = locked.iter().map(u32::to_string).collect();
            return Err(Problem::failed(format!(
                "{} locked against turning (`LockRotate`): shape {}; clear the lock to turn {}",
                if locked.len() == 1 { "the shape is" } else { "the shapes are" },
                named.join(", "),
                if locked.len() == 1 { "it" } else { "them" }
            )));
        }
        // Where the set turns about: the point asked for, else the middle of the joint box of everything named (a locked shape's box counts, as it is drawn).
        let about_set = match turn {
            Turn::By(_) if about.is_some() => about,
            Turn::By(_) if targets.len() > 1 => Some(joint_box(&reader, &targets)?.middle()),
            _ => None,
        };
        let mut writes = Vec::new();
        let mut count = 0_usize;
        for t in targets.iter().filter(|t| !locked.contains(&t.id)) {
            let planned = writes.len();
            if t.one_d {
                plan_line_turn(doc, page, &reader, t, turn, about_set, &mut writes);
            } else {
                plan_shape_turn(&reader, t, turn, about_set, &mut writes);
            }
            count = count.saturating_add(usize::from(writes.len() > planned));
        }
        (writes, count, targets.iter().filter(|t| !locked.contains(&t.id)).map(|t| t.id).collect::<Vec<u32>>(), locked)
    };
    if writes.is_empty() {
        return Ok(Turned { count: 0, locked });
    }
    apply(doc, writes)?;
    settle(doc, &before, page, &touched, &[])?;
    Ok(Turned { count, locked })
}

/// The writes that turn a two-dimensional shape: its angle, and for a set turned by an angle its pin about the middle of the joint box.
fn plan_shape_turn(reader: &Reader<'_>, t: &Target, turn: Turn, about_set: Option<P>, writes: &mut Vec<Write>) {
    let pose = reader.pose(&t.sheet);
    let wanted = match turn {
        Turn::To(a) => a,
        Turn::By(d) => pose.angle + d,
    };
    plan_angle(writes, t, pose.angle, wanted);
    if let (Turn::By(d), Some(c)) = (turn, about_set) {
        let moved = turn_about(c, d, reader.parent_to_page(&t.at, pose.pin));
        plan_pin(writes, t, pose.pin, reader.point_from_page(&t.at, moved));
    }
}

/// The writes that turn a one-dimensional shape: its free ends. **ROTATE-1D-1**: about the joint box for a set turned `by`; else, with one end glued,
/// about that end, so the line turns by the angle and `angle` is reached exactly; else about the pin.
fn plan_line_turn(doc: &Document, page: u32, reader: &Reader<'_>, t: &Target, turn: Turn, about_set: Option<P>, writes: &mut Vec<Write>) {
    let ends = |x: &str, y: &str| (reader.number(&t.sheet, x), reader.number(&t.sheet, y));
    let (begin, end) = (ends("BeginX", "BeginY"), ends("EndX", "EndY"));
    let pin = reader.parent_to_page(&t.at, ends("PinX", "PinY"));
    let held = match glued_ends(doc, page, t.id) {
        (true, false) => Some(reader.parent_to_page(&t.at, begin)),
        (false, true) => Some(reader.parent_to_page(&t.at, end)),
        _ => None,
    };
    let (by, about) = match turn {
        Turn::By(d) => (d, about_set.or(held).unwrap_or(pin)),
        Turn::To(a) => (a - (end.1 - begin.1).atan2(end.0 - begin.0), held.unwrap_or(pin)),
    };
    plan_line(doc, page, reader, t, &|p| turn_about(about, by, p), writes);
}

// ---- flip --------------------------------------------------------------------------------------------------------------------------

/// Flips shapes (**ROTATE-1D-1**). → how many were flipped: a connector with both ends glued is not counted, and when none is the
/// document is not touched.
pub fn flip_shapes(doc: &mut Document, page: u32, ids: &[u32], axis: FlipAxis) -> Result<usize, Problem> {
    let before = doc.clone();
    let (writes, count, touched) = {
        let reader = Reader::new(doc);
        let targets = outermost(targets(doc, &reader, page, ids)?);
        refuse_turned_groups(&reader, &targets, "flipped")?;
        let rect = joint_box(&reader, &targets)?;
        let c = match axis {
            FlipAxis::Horizontal => (rect.x0 + rect.x1) / 2.0,
            FlipAxis::Vertical => (rect.y0 + rect.y1) / 2.0,
        };
        let mut writes = Vec::new();
        let mut count = 0_usize;
        for t in &targets {
            let planned = writes.len();
            if t.one_d {
                plan_line(doc, page, &reader, t, &|p| mirror(axis, c, p), &mut writes);
            } else {
                plan_shape_flip(&reader, t, axis, c, &mut writes);
            }
            count = count.saturating_add(usize::from(writes.len() > planned));
        }
        (writes, count, targets.iter().map(|t| t.id).collect::<Vec<u32>>())
    };
    if writes.is_empty() {
        return Ok(0);
    }
    apply(doc, writes)?;
    settle(doc, &before, page, &touched, &[])?;
    Ok(count)
}

/// The writes that flip a two-dimensional shape: its pin mirrored, its flip toggled, its angle negated.
fn plan_shape_flip(reader: &Reader<'_>, t: &Target, axis: FlipAxis, c: f64, writes: &mut Vec<Write>) {
    let pose = reader.pose(&t.sheet);
    let mirrored = mirror(axis, c, reader.parent_to_page(&t.at, pose.pin));
    plan_pin(writes, t, pose.pin, reader.point_from_page(&t.at, mirrored));
    let (name, flipped) = match axis {
        FlipAxis::Horizontal => ("FlipX", pose.flip_x),
        FlipAxis::Vertical => ("FlipY", pose.flip_y),
    };
    writes.push(Write { sheet: t.sheet.clone(), name, kind: Kind::Flag, value: if flipped { 0.0 } else { 1.0 } });
    plan_angle(writes, t, pose.angle, -pose.angle);
}

// ---- drawing order -----------------------------------------------------------------------------------------------------------------

/// Changes the drawing order of shapes. → how many of the shapes named changed place; when none did the document is not touched.
pub fn order_shapes(doc: &mut Document, page: u32, ids: &[u32], to: Stack) -> Result<usize, Problem> {
    // The positions picked in each list: the page's part (parent path empty) or a group's members.
    let mut lists: BTreeMap<Vec<u32>, BTreeSet<usize>> = BTreeMap::new();
    {
        let reader = Reader::new(doc);
        for t in targets(doc, &reader, page, ids)? {
            if let Some((last, parent)) = t.at.path.split_last() {
                lists.entry(parent.to_vec()).or_default().insert(usize::try_from(*last).unwrap_or(usize::MAX));
            }
        }
    }
    // The deepest lists first: reordering a list changes the places of its members, which are on the way to the lists inside them.
    let mut lists: Vec<(Vec<u32>, BTreeSet<usize>)> = lists.into_iter().collect();
    lists.sort_by_key(|(parent, _)| std::cmp::Reverse(parent.len()));
    let mut moved = 0_usize;
    for (parent, picked) in lists {
        let Some(list) = siblings(doc, page, &parent) else { continue };
        let bands = bands_of(doc, page, &parent, list.len());
        let selected: Vec<bool> = (0..list.len()).map(|i| picked.contains(&i)).collect();
        let new_order = restack(&selected, &bands, to);
        let changed = new_order.iter().enumerate().filter(|(place, was)| selected.get(**was) == Some(&true) && place != *was).count();
        if changed == 0 {
            continue;
        }
        let reordered: ShapeList = new_order.iter().filter_map(|was| list.get(*was).cloned()).collect();
        put_siblings(doc, page, &parent, reordered)?;
        moved = moved.saturating_add(changed);
    }
    Ok(moved)
}

/// The shapes of a list, bottom first: the page's part, or the members of the group at `parent`.
pub(super) fn siblings(doc: &Document, page: u32, parent: &[u32]) -> Option<Vec<Arc<Shape>>> {
    if parent.is_empty() {
        doc.page(page).map(|p| p.shapes.iter().cloned().collect())
    } else {
        doc.shape(&ShapeRef { owner: Owner::Page(page), path: parent.to_vec() }).map(|s| s.children.iter().cloned().collect())
    }
}

/// Puts a rebuilt list of shapes in place of the page's, or of a group's members: the one place the drawing order, group and ungroup put a list back. **The unknown nodes among the
/// shapes (a comment, a foreign element) go with the shapes they followed** (`Page::edit_shapes`, `Shape::edit_members`).
pub(super) fn put_siblings(doc: &mut Document, page: u32, parent: &[u32], list: ShapeList) -> Result<(), Problem> {
    if parent.is_empty() {
        page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.edit_shapes(|shapes| *shapes = list);
    } else {
        shape_mut(doc, page, parent).ok_or_else(|| Problem::failed("the group went away"))?.edit_members(|members| *members = list);
    }
    Ok(())
}

/// The band of each of the `len` shapes of a list (`None` for the reserved -32768), by inheritance.
fn bands_of(doc: &Document, page: u32, parent: &[u32], len: usize) -> Vec<Option<i64>> {
    let reader = Reader::new(doc);
    (0..len)
        .map(|i| {
            let mut path = parent.to_vec();
            path.push(u32::try_from(i).unwrap_or(u32::MAX));
            let level = reader.number(&SheetRef::Shape(ShapeRef { owner: Owner::Page(page), path }), "DisplayLevel").round();
            if level == RESERVED_BAND { None } else { Some(level.clamp(-1.0e9, 1.0e9) as i64) }
        })
        .collect()
}

/// The new order of a list of `selected.len()` shapes: for each place, bottom first, the place the shape there had. `bands` is each shape's band
/// (`None` for none).
pub(super) fn restack(selected: &[bool], bands: &[Option<i64>], to: Stack) -> Vec<usize> {
    let n = selected.len();
    match to {
        Stack::Front => to_front(selected, bands),
        // The back is the front of the list turned over, with the bands negated so that "lower" is "higher".
        Stack::Back => {
            let turned_selected: Vec<bool> = selected.iter().rev().copied().collect();
            let turned_bands: Vec<Option<i64>> = bands.iter().rev().map(|b| b.map(|b| -b)).collect();
            to_front(&turned_selected, &turned_bands).into_iter().rev().map(|i| n.saturating_sub(1).saturating_sub(i)).collect()
        }
        Stack::Forward => step(selected, bands, true),
        Stack::Backward => step(selected, bands, false),
    }
}

/// Each selected shape goes just before the first shape after it with a higher band (the end of the list when there is none), the selected ones
/// that go to one place in the order they had. One pass over the list and a sort of the selected.
fn to_front(selected: &[bool], bands: &[Option<i64>]) -> Vec<usize> {
    let n = selected.len();
    // The first later shape with a higher band, for each shape: a stack of the shapes to the right that nothing nearer is higher than.
    let mut limit = vec![n; n];
    let mut stack: Vec<usize> = Vec::new();
    for i in (0..n).rev() {
        let Some(Some(band)) = bands.get(i) else { continue };
        while let Some(top) = stack.last() {
            if bands.get(*top).copied().flatten().is_some_and(|b| b <= *band) {
                stack.pop();
            } else {
                break;
            }
        }
        if let (Some(top), Some(slot)) = (stack.last(), limit.get_mut(i)) {
            *slot = *top;
        }
        stack.push(i);
    }
    let mut going: Vec<(usize, usize)> =
        (0..n).filter(|i| selected.get(*i) == Some(&true)).map(|i| (limit.get(i).copied().unwrap_or(n), i)).collect();
    going.sort_unstable();
    let mut out = Vec::with_capacity(n);
    let mut next = 0;
    for place in 0..=n {
        while let Some((limit, i)) = going.get(next) {
            if *limit != place {
                break;
            }
            out.push(*i);
            next += 1;
        }
        if place < n && selected.get(place) != Some(&true) {
            out.push(place);
        }
    }
    out
}

/// Each selected shape moves one place up (or down) past a neighbour that is not selected and not in the way of its band; the ones nearest the end
/// go first, so a block moves as a block and a block at the end stays.
fn step(selected: &[bool], bands: &[Option<i64>], up: bool) -> Vec<usize> {
    let n = selected.len();
    let mut order: Vec<usize> = (0..n).collect();
    let band = |shape: usize| bands.get(shape).copied().flatten();
    let blocked = |mover: usize, other: usize| match (band(mover), band(other)) {
        (Some(m), Some(o)) => {
            if up {
                o > m
            } else {
                o < m
            }
        }
        _ => false,
    };
    let places: Vec<usize> = if up { (0..n.saturating_sub(1)).rev().collect() } else { (1..n).collect() };
    for place in places {
        let neighbour = if up { place.saturating_add(1) } else { place.saturating_sub(1) };
        let (Some(mover), Some(other)) = (order.get(place).copied(), order.get(neighbour).copied()) else { continue };
        if selected.get(mover) == Some(&true) && selected.get(other) != Some(&true) && !blocked(mover, other) {
            order.swap(place, neighbour);
        }
    }
    order
}

// ---- align -------------------------------------------------------------------------------------------------------------------------

impl Align {
    /// The distance on the page that takes a box with `r` to the primary's `p` on this edge or middle.
    fn shift(self, p: &Rect, r: &Rect) -> P {
        let (pm, rm) = (p.middle(), r.middle());
        match self {
            Align::Left => (p.x0 - r.x0, 0.0),
            Align::Center => (pm.0 - rm.0, 0.0),
            Align::Right => (p.x1 - r.x1, 0.0),
            Align::Top => (0.0, p.y1 - r.y1),
            Align::Middle => (0.0, pm.1 - rm.1),
            Align::Bottom => (0.0, p.y0 - r.y0),
        }
    }
}

/// Aligns shapes to the first one named. → how many moved: the first never does, a shape already there does not, and a connector
/// with both ends glued has nothing to move; when none moves the document is not touched.
pub fn align_shapes(doc: &mut Document, page: u32, ids: &[u32], how: Align) -> Result<usize, Problem> {
    let moves: Vec<(u32, P)> = {
        let reader = Reader::new(doc);
        let targets = targets(doc, &reader, page, ids)?;
        let Some((first, rest)) = targets.split_first() else { return Ok(0) };
        let primary = Rect::of(page_points(&reader, first))?;
        // A shape inside another that is named moves with it, and so does the primary one when it is inside a shape that is named: neither is
        // moved on its own.
        let paths: BTreeSet<Vec<u32>> = targets.iter().map(|t| t.at.path.clone()).collect();
        let mut moves = Vec::with_capacity(rest.len());
        for t in rest.iter().filter(|t| !inside_one_of(&t.at, &paths) && !is_inside(&first.at, &t.at)) {
            let shift = how.shift(&primary, &Rect::of(page_points(&reader, t))?);
            if shift.0.abs() > SAME_PLACE || shift.1.abs() > SAME_PLACE {
                moves.push((t.id, shift));
            }
        }
        moves
    };
    if moves.is_empty() {
        return Ok(0);
    }
    move_each(doc, page, &moves)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn flags(picked: &[usize], n: usize) -> Vec<bool> {
        (0..n).map(|i| picked.contains(&i)).collect()
    }

    fn level(bands: &[i64]) -> Vec<Option<i64>> {
        bands.iter().map(|b| Some(*b)).collect()
    }

    #[test]
    fn an_angle_is_kept_from_just_over_minus_pi_to_pi_with_a_quarter_exact() {
        assert_eq!(tidy_angle(0.0), 0.0);
        assert_eq!(tidy_angle(TAU), 0.0);
        assert_eq!(tidy_angle(-0.0), 0.0);
        assert!(tidy_angle(-0.0).is_sign_positive());
        assert_eq!(tidy_angle(FRAC_PI_2 + 1.0e-12), FRAC_PI_2);
        // The quarters, whatever they are given as: exact, and in the range.
        assert_eq!(tidy_angle(-FRAC_PI_2), -FRAC_PI_2);
        assert_eq!(tidy_angle(3.0 * FRAC_PI_2), -FRAC_PI_2);
        assert_eq!(tidy_angle(5.0 * FRAC_PI_2), FRAC_PI_2);
        assert_eq!(tidy_angle(PI), PI);
        assert_eq!(tidy_angle(-PI), PI, "half a turn is +pi, the top of the range");
        assert_eq!(tidy_angle(PI + 1.0e-12), PI);
        assert_eq!(tidy_angle(-PI - 1.0e-12), PI);
        // Not a quarter: wrapped to the range, a turn past half the way round is a negative one.
        assert!((tidy_angle(-0.3) + 0.3).abs() < 1.0e-15);
        assert!((tidy_angle(TAU - 0.3) + 0.3).abs() < 1.0e-12);
        assert!((tidy_angle(PI + 0.5) - (0.5 - PI)).abs() < 1.0e-12);
        assert!((tidy_angle(PI - 0.5) - (PI - 0.5)).abs() < 1.0e-12);
        assert_eq!(tidy_angle(0.5), 0.5);
        for a in [-1.0e5, -7.0, -3.2, -3.0, 0.1, 3.0, 3.2, 7.0, 1.0e5] {
            let t = tidy_angle(a);
            assert!(t > -PI && t <= PI, "{a} gives {t}");
            assert!(same_angle(t, a), "{a} gives {t}, another turn");
        }
        // Within a billionth of a radian of a quarter turn is that quarter turn, and the noise of the arithmetic is nothing.
        assert_eq!(tidy_angle(f64::MIN_POSITIVE), 0.0);
        assert_eq!(tidy_angle(6.123_233_995_736_766e-17), 0.0);
        assert_ne!(tidy_angle(2.0e-9), 0.0);
    }

    #[test]
    fn an_angle_is_written_with_the_digits_a_file_holds() {
        assert_eq!(angle_text(FRAC_PI_2), "1.5707963267949");
        assert_eq!(angle_text(0.0), "0");
        assert_eq!(angle_text(-0.0), "0");
        assert_eq!(angle_text(PI), "3.1415926535898");
        assert_eq!(angle_text(-FRAC_PI_2), "-1.5707963267949");
        assert_eq!(angle_text(f64::NAN), "0");
    }

    #[test]
    fn two_angles_a_whole_turn_apart_are_the_same_turn_and_the_sameness_is_the_snap() {
        assert!(same_angle(0.0, TAU));
        assert!(same_angle(-0.5, TAU - 0.5));
        assert!(same_angle(1.0, 1.0 + 1.0e-14));
        assert!(!same_angle(1.0, 1.001));
        // 89.9999999999 degrees is snapped to 90, and is the same angle as 90: a turn of nothing on it writes nothing.
        let nearly = 89.999_999_999_9_f64.to_radians();
        assert_eq!(tidy_angle(nearly), FRAC_PI_2);
        assert!(same_angle(nearly, tidy_angle(nearly)));
        // The edge is the one number: just inside it snaps and is the same, just outside neither.
        assert_eq!(tidy_angle(FRAC_PI_2 + 0.9 * SAME_ANGLE), FRAC_PI_2);
        assert!(same_angle(FRAC_PI_2 + 0.9 * SAME_ANGLE, FRAC_PI_2));
        assert_ne!(tidy_angle(FRAC_PI_2 + 1.1 * SAME_ANGLE), FRAC_PI_2);
        assert!(!same_angle(FRAC_PI_2 + 1.1 * SAME_ANGLE, FRAC_PI_2));
    }

    #[test]
    fn only_a_bound_or_setatref_formula_passes_the_number_on() {
        for yes in ["BOUND(0,0,6.28,-6.28,6.28)", " setatref(Sheet.2!Angle)", "SETATREF(ThePage!User.X)"] {
            assert!(passes_the_number_on(yes), "{yes}");
        }
        for no in ["30 deg", "GUARD(0)", "ATAN2(EndY-BeginY,EndX-BeginX)", "BOUNDS", ""] {
            assert!(!passes_the_number_on(no), "{no}");
        }
    }

    #[test]
    fn front_puts_the_chosen_just_before_the_first_higher_band_in_the_order_they_had() {
        // Bands 0, 0, 25000: the first to the front is behind the third.
        assert_eq!(restack(&flags(&[0], 3), &level(&[0, 0, 25_000]), Stack::Front), [1, 0, 2]);
        // All in one band: the end of the list, in their order.
        assert_eq!(restack(&flags(&[1, 0], 4), &level(&[0; 4]), Stack::Front), [2, 3, 0, 1]);
        // Two bands apiece: each to the end of its own.
        let bands = level(&[0, 0, 5, 5, 9]);
        assert_eq!(restack(&flags(&[0, 2], 5), &bands, Stack::Front), [1, 0, 3, 2, 4]);
        // Already at the front: the same order.
        assert_eq!(restack(&flags(&[3], 4), &level(&[0; 4]), Stack::Front), [0, 1, 2, 3]);
        assert_eq!(restack(&[], &[], Stack::Front), Vec::<usize>::new());
    }

    #[test]
    fn a_shape_with_no_band_goes_to_the_very_front_and_stops_nobody() {
        let bands = vec![Some(0), None, Some(25_000), Some(0)];
        assert_eq!(restack(&flags(&[1], 4), &bands, Stack::Front), [0, 2, 3, 1]);
        // A banded shape goes past the unbanded one, but not the higher band.
        assert_eq!(restack(&flags(&[0], 4), &bands, Stack::Front), [1, 0, 2, 3]);
        assert_eq!(restack(&flags(&[1], 4), &bands, Stack::Back), [1, 0, 2, 3]);
    }

    #[test]
    fn back_is_front_the_other_way_round() {
        assert_eq!(restack(&flags(&[2], 3), &level(&[0, 0, 0]), Stack::Back), [2, 0, 1]);
        assert_eq!(restack(&flags(&[1, 2], 4), &level(&[0; 4]), Stack::Back), [1, 2, 0, 3]);
        // The band below: the third of bands 0, 0, 25000 sent back stays in front of the others.
        assert_eq!(restack(&flags(&[2], 3), &level(&[0, 0, 25_000]), Stack::Back), [0, 1, 2]);
        assert_eq!(restack(&flags(&[1], 3), &level(&[-5, 0, 25_000]), Stack::Back), [0, 1, 2]);
        assert_eq!(restack(&flags(&[0, 1], 3), &level(&[0, 0, 25_000]), Stack::Back), [0, 1, 2]);
    }

    #[test]
    fn forward_and_backward_move_a_block_one_place_and_a_block_at_the_end_stays() {
        let flat = level(&[0; 5]);
        assert_eq!(restack(&flags(&[1, 2], 5), &flat, Stack::Forward), [0, 3, 1, 2, 4]);
        assert_eq!(restack(&flags(&[3, 4], 5), &flat, Stack::Forward), [0, 1, 2, 3, 4]);
        assert_eq!(restack(&flags(&[2, 4], 5), &flat, Stack::Forward), [0, 1, 3, 2, 4]);
        assert_eq!(restack(&flags(&[1, 2], 5), &flat, Stack::Backward), [1, 2, 0, 3, 4]);
        assert_eq!(restack(&flags(&[0, 1], 5), &flat, Stack::Backward), [0, 1, 2, 3, 4]);
        // The band is in the way.
        let bands = level(&[0, 0, 7]);
        assert_eq!(restack(&flags(&[1], 3), &bands, Stack::Forward), [0, 1, 2]);
        assert_eq!(restack(&flags(&[2], 3), &bands, Stack::Backward), [0, 1, 2]);
        assert_eq!(restack(&flags(&[0], 3), &bands, Stack::Forward), [1, 0, 2]);
    }

    #[test]
    fn a_reorder_is_always_a_permutation() {
        // A deterministic spread of lists, picks and bands, in every direction.
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..400 {
            let n = usize::try_from(next() % 12).unwrap();
            let selected: Vec<bool> = (0..n).map(|_| next() % 3 == 0).collect();
            let bands: Vec<Option<i64>> =
                (0..n).map(|_| if next() % 5 == 0 { None } else { Some(i64::try_from(next() % 3).unwrap() * 10) }).collect();
            for to in [Stack::Front, Stack::Back, Stack::Forward, Stack::Backward] {
                let order = restack(&selected, &bands, to);
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..n).collect::<Vec<_>>(), "{to:?} {selected:?} {bands:?}");
                // The ones not chosen keep the order they had among themselves.
                let rest: Vec<usize> = order.iter().copied().filter(|i| !selected[*i]).collect();
                assert!(rest.windows(2).all(|w| w[0] < w[1]), "{to:?} {selected:?} {bands:?} {order:?}");
            }
        }
    }

    #[test]
    fn a_turn_and_a_mirror_are_what_they_say() {
        let p = turn_about((1.0, 1.0), FRAC_PI_2, (3.0, 1.0));
        assert!((p.0 - 1.0).abs() < 1e-12 && (p.1 - 3.0).abs() < 1e-12, "{p:?}");
        assert_eq!(mirror(FlipAxis::Horizontal, 2.0, (3.0, 5.0)), (1.0, 5.0));
        assert_eq!(mirror(FlipAxis::Vertical, 2.0, (3.0, 5.0)), (3.0, -1.0));
    }
}
