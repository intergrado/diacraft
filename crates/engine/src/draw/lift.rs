//! Taking shapes from one frame to another while they keep where they draw: group, ungroup, and a copy of a member of a group.
//!
//! A shape's cells are in the frame of its parent (the page for a top-level shape, the group for a member). To
//! move a shape from one parent to another so that it draws where it did, its pose is carried by the map from the old parent's frame to the new:
//! an [`Affine`], a turn, perhaps a mirror, and a move. Our rule, after a change: a two-dimensional shape's pin goes through the
//! map, its `Angle` becomes the map's angle plus its own (minus its own under a mirror, since a mirror turns the other way round), and its `FlipX`
//! and `FlipY` are its own flips and the map's, taken as one flip each. A one-dimensional shape (a connector) has its ends that are not glued
//! carried through the map; the cached value of a glued end is carried through it too, under the `Evaluation` origin so that its formula stays
//! as it is, and is worked out again where `calc` can: an end glued by a function `calc` cannot work out (`_WALKGLUE`) is then where the file
//! had it, in the new frame, and the engine's own placement rule is not asked. A map that mirrors is refused for a
//! one-dimensional shape: its `Begin` and `End` ends are fixed by its frame, which a mirror would turn about, and no source says what a
//! viewer would then draw.

use std::collections::HashSet;

use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, FormulaSlot, ShapeRef, Sheet, SheetRef, Value, WriteOrigin};

use super::Problem;
use super::arrange::{Kind, SAME_PLACE, Write, apply, same_angle, tidy_angle};
use super::read::{Pose, Reader};
use super::route::P;
use crate::stencil::number_text;

/// A map of the plane that a turn, a mirror and a move make: `p` goes to `R(angle) F p + t`, where `F` negates x for `flip_x` and y for `flip_y`
/// first (the order). A map that negates both is a half turn, and is held as one: at most one flip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine {
    angle: f64,
    flip_x: bool,
    flip_y: bool,
    t: P,
}

impl Affine {
    /// A move by (`dx`, `dy`).
    pub fn translation(dx: f64, dy: f64) -> Affine {
        Affine { angle: 0.0, flip_x: false, flip_y: false, t: (dx, dy) }
    }

    /// The map from a shape's frame to its parent's.
    pub fn of_pose(pose: &Pose) -> Affine {
        Affine { angle: pose.angle, flip_x: pose.flip_x, flip_y: pose.flip_y, t: pose.up((0.0, 0.0)) }.normal()
    }

    fn normal(mut self) -> Affine {
        if self.flip_x && self.flip_y {
            self.angle += std::f64::consts::PI;
            self.flip_x = false;
            self.flip_y = false;
        }
        self
    }

    /// Where a point goes.
    pub fn apply(&self, p: P) -> P {
        Pose { pin: self.t, loc: (0.0, 0.0), angle: self.angle, flip_x: self.flip_x, flip_y: self.flip_y }.up(p)
    }

    /// Whether the map mirrors: a turn and a move do not.
    pub fn reflects(&self) -> bool {
        self.flip_x != self.flip_y
    }

    /// The map that does `self` and then `outer`.
    pub fn then(&self, outer: &Affine) -> Affine {
        let sign = if outer.reflects() { -1.0 } else { 1.0 };
        Affine {
            angle: outer.angle + sign * self.angle,
            flip_x: outer.flip_x != self.flip_x,
            flip_y: outer.flip_y != self.flip_y,
            t: outer.apply(self.t),
        }
        .normal()
    }

    /// The pose a shape with pose `own` has once its parent's frame is carried through this map: the pin, the angle and the two flips
    /// The local pin does not change.
    pub fn lift(&self, own: &Pose) -> (P, f64, bool, bool) {
        let sign = if self.reflects() { -1.0 } else { 1.0 };
        (self.apply(own.pin), self.angle + sign * own.angle, own.flip_x != self.flip_x, own.flip_y != self.flip_y)
    }
}

/// A shape to carry to a new parent.
pub(crate) struct Lift {
    pub id: u32,
    pub at: ShapeRef,
    /// The map from the frame the shape's cells are in to the one they are to be in.
    pub by: Affine,
}

/// Whether the cell `name` of `sheet` has a formula, its own or one it inherits: a recalculation gives it its value.
fn has_formula(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> bool {
    reader
        .resolver()
        .resolve(sheet, &CellAddr::direct(name))
        .and_then(|found| found.formula.cell)
        .is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_)))
}

fn push_length(writes: &mut Vec<Write>, sheet: &SheetRef, name: &'static str, was: f64, now: f64) {
    if (now - was).abs() > SAME_PLACE {
        writes.push(Write { sheet: sheet.clone(), name, kind: Kind::Length, value: now });
    }
}

/// What carrying shapes writes: cells as a drag writes them, and the cached values of glued ends.
#[derive(Default)]
struct Plan {
    writes: Vec<Write>,
    /// The cached value of a glued end cell, carried: the cell's shape, its name and the number.
    cached: Vec<(SheetRef, &'static str, f64)>,
}

/// The writes that carry one shape.
fn plan_one(reader: &Reader<'_>, item: &Lift, glued: &HashSet<(u32, bool)>, plan: &mut Plan) -> Result<(), Problem> {
    let writes = &mut plan.writes;
    let sheet = SheetRef::Shape(item.at.clone());
    let pose = reader.pose(&sheet);
    if reader.is_one_d(&sheet) {
        if item.by.reflects() {
            return Err(Problem::failed("a connector cannot be carried out of a flipped group yet"));
        }
        for (end, x_cell, y_cell) in [(false, "BeginX", "BeginY"), (true, "EndX", "EndY")] {
            let was = (reader.number(&sheet, x_cell), reader.number(&sheet, y_cell));
            let now = item.by.apply(was);
            if glued.contains(&(item.id, end)) {
                for (name, was, now) in [(x_cell, was.0, now.0), (y_cell, was.1, now.1)] {
                    if (now - was).abs() > SAME_PLACE {
                        plan.cached.push((sheet.clone(), name, now));
                    }
                }
                continue;
            }
            push_length(writes, &sheet, x_cell, was.0, now.0);
            push_length(writes, &sheet, y_cell, was.1, now.1);
        }
        // The cells a recalculation gives from the ends are left to it; the others are carried.
        if !has_formula(reader, &sheet, "PinX") && !has_formula(reader, &sheet, "PinY") {
            let now = item.by.apply(pose.pin);
            push_length(writes, &sheet, "PinX", pose.pin.0, now.0);
            push_length(writes, &sheet, "PinY", pose.pin.1, now.1);
        }
        if !has_formula(reader, &sheet, "Angle") {
            let now = tidy_angle(pose.angle + item.by.angle);
            if !same_angle(now, pose.angle) {
                writes.push(Write { sheet, name: "Angle", kind: Kind::Angle, value: now });
            }
        }
        return Ok(());
    }
    let (pin, angle, flip_x, flip_y) = item.by.lift(&pose);
    push_length(writes, &sheet, "PinX", pose.pin.0, pin.0);
    push_length(writes, &sheet, "PinY", pose.pin.1, pin.1);
    let angle = tidy_angle(angle);
    if !same_angle(angle, pose.angle) {
        writes.push(Write { sheet: sheet.clone(), name: "Angle", kind: Kind::Angle, value: angle });
    }
    for (name, was, now) in [("FlipX", pose.flip_x, flip_x), ("FlipY", pose.flip_y, flip_y)] {
        if was != now {
            writes.push(Write { sheet: sheet.clone(), name, kind: Kind::Flag, value: if now { 1.0 } else { 0.0 } });
        }
    }
    Ok(())
}

/// Carries shapes to the frames they are to be in: writes the cells that change, as a drag writes them. `glued` says which
/// ends of the connectors among them (`(ID, true)` for the end, `(ID, false)` for the begin) are glued: those are not written, their cached values
/// are carried. → for each shape of `items`, in their order, the names of the cells written under it: one pass, so that a caller finds the cells of a
/// shape by its place and does not search the whole list for each shape. The shapes are where `items` says in the document as it is now.
pub(crate) fn lift(doc: &mut Document, items: &[Lift], glued: &HashSet<(u32, bool)>) -> Result<Vec<Vec<&'static str>>, Problem> {
    let mut plan = Plan::default();
    let mut written: Vec<Vec<&'static str>> = Vec::with_capacity(items.len());
    {
        let reader = Reader::new(doc);
        for item in items {
            let (planned, cached) = (plan.writes.len(), plan.cached.len());
            plan_one(&reader, item, glued, &mut plan)?;
            let names = plan.writes.iter().skip(planned).map(|w| w.name).chain(plan.cached.iter().skip(cached).map(|(_, name, _)| *name));
            written.push(names.collect());
        }
    }
    apply(doc, plan.writes)?;
    let cached: Vec<(SheetRef, CellWrite)> = plan
        .cached
        .into_iter()
        .map(|(sheet, name, value)| {
            let edit = CellEdit::Cached { value: Value::from_text(&number_text(value)), error: None, unit: None };
            (sheet, CellWrite { addr: CellAddr::direct(name), edit, origin: WriteOrigin::Evaluation })
        })
        .collect();
    if !cached.is_empty() {
        doc.write_batch(cached)?;
    }
    Ok(written)
}

/// The cells of a sheet whose formula calls a function that carries a point between frames (`PAR`, `LOC`, `LOCTOLOC`, `LOCTOPAR`, `ANGLETOLOC`,
/// `ANGLETOPAR`): the cells whose value depends on the frame the shape is in. A word of the formula that ends in `PAR(` or `LOC(` is enough; a
/// cell that only seems to call one is worked out once more and comes out as it was.
pub(crate) fn frame_cells(sheet: &Sheet) -> Vec<CellAddr> {
    let calls_one = |formula: &FormulaSlot| match formula {
        FormulaSlot::Local(f) => {
            let upper = f.source.to_ascii_uppercase();
            upper.contains("PAR(") || upper.contains("LOC(")
        }
        _ => false,
    };
    super::remap::cells(sheet).into_iter().filter(|(_, cell)| calls_one(&cell.formula)).map(|(addr, _)| addr).collect()
}

/// The cells that say where a shape is in its frame: what a formula that carries a point between frames reads of it.
pub(crate) const POSE_CELLS: [&str; 7] = ["PinX", "PinY", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn near(a: P, b: P) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    fn poses() -> Vec<Pose> {
        let mut out = Vec::new();
        for (fx, fy) in [(false, false), (true, false), (false, true), (true, true)] {
            for angle in [0.0, 0.4, std::f64::consts::FRAC_PI_2, 3.0, -2.2] {
                out.push(Pose { pin: (5.0, -3.0), loc: (1.0, 0.5), angle, flip_x: fx, flip_y: fy });
            }
        }
        out
    }

    #[test]
    fn the_map_of_a_pose_is_what_the_pose_does_to_a_point_of_its_frame() {
        for pose in poses() {
            let map = Affine::of_pose(&pose);
            for p in [(0.0, 0.0), (2.0, 1.0), (-3.5, 7.25)] {
                assert!(near(map.apply(p), pose.up(p)), "{pose:?} {p:?}");
            }
        }
    }

    #[test]
    fn two_maps_one_after_the_other_are_the_map_of_the_two_steps() {
        for inner in poses() {
            for outer in poses() {
                let (a, b) = (Affine::of_pose(&inner), Affine::of_pose(&outer));
                let both = a.then(&b);
                for p in [(0.0, 0.0), (2.0, 1.0), (-3.5, 7.25)] {
                    assert!(near(both.apply(p), b.apply(a.apply(p))), "{inner:?} then {outer:?} at {p:?}");
                }
            }
        }
    }

    #[test]
    fn a_lifted_pose_draws_every_point_where_it_drew_it() {
        // A shape in a frame, the frame carried by a map: the shape's pose after is the map's and its own together, whatever each flips and turns.
        for own in poses() {
            for outer in poses() {
                let map = Affine::of_pose(&outer);
                let (pin, angle, flip_x, flip_y) = map.lift(&own);
                let lifted = Pose { pin, loc: own.loc, angle, flip_x, flip_y };
                for p in [(0.0, 0.0), (2.0, 1.0), (-3.5, 7.25), (0.3, 0.9)] {
                    assert!(near(lifted.up(p), map.apply(own.up(p))), "{own:?} lifted by {outer:?} at {p:?}");
                }
            }
        }
    }

    #[test]
    fn a_translation_moves_and_a_half_turn_is_not_a_mirror() {
        assert!(near(Affine::translation(2.0, -1.0).apply((1.0, 1.0)), (3.0, 0.0)));
        let half = Affine::of_pose(&Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: 0.0, flip_x: true, flip_y: true });
        assert!(!half.reflects());
        assert!(near(half.apply((1.0, 2.0)), (-1.0, -2.0)));
        assert!(Affine::of_pose(&Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: 0.0, flip_x: true, flip_y: false }).reflects());
    }

    #[test]
    fn the_cells_that_carry_a_point_between_frames_are_found_by_their_formulas() {
        use diacraft_doc::Cell;
        let mut sheet = Sheet::default();
        sheet
            .cells
            .insert(diacraft_doc::CellName::new("BeginX"), Cell::with_formula("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))", None));
        sheet.cells.insert(diacraft_doc::CellName::new("EndX"), Cell::with_formula("loctoloc(PNT(1,1),Sheet.2!Width,Width)", None));
        sheet.cells.insert(diacraft_doc::CellName::new("Width"), Cell::with_formula("Height*2", None));
        sheet.cells.insert(diacraft_doc::CellName::new("PinX"), Cell::with_formula("Sheet.4!PinX+1", None));
        let found: Vec<String> = frame_cells(&sheet).iter().map(|a| a.cell.as_str().to_owned()).collect();
        assert_eq!(found, ["BeginX", "EndX"]);
    }
}
