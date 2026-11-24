//! Resizing shapes as one, and a group by its box.
//!
//! `shape.resize` with `ids` makes the joint box of the shapes named (the box on the page that holds their boxes, or for a connector its two ends: the box the window draws
//! round them) the box it is given, and each shape follows as the drawing does when the page is stretched: **every place is carried by the map that takes the old box to
//! the new one** (a stretch along each page axis about the corner of the old box), so the shapes keep their order and their gaps in proportion.
//!
//! * **A shape's box is the one the map makes of it, in its own axes.** Its centre goes where the map puts it; its `Width` and `Height` are scaled by the length the map
//!   gives its own x and y axes (for a shape turned by `a`, `sqrt((sx cos a)^2 + (sy sin a)^2)` along x and `sqrt((sx sin a)^2 + (sy cos a)^2)` along y: a shape not turned
//!   is stretched by `sx` and `sy`, one turned a quarter by `sy` and `sx`, and the same stretch on both axes scales every shape the same). **A shape turned by another angle
//!   keeps its angle** (a stretch that is not the same on both axes would shear it, which a box cannot say). The pin follows from the centre: the local pin is worked out
//!   again for the new size, as [`super::shapes::resize_shape`] does for one shape, so a pin in the middle stays there and the shape's turn and flips are kept.
//! * **A one-dimensional shape** (a connector) carries its ends that are not glued by the map; a glued end stays with its shape, and the connectors glued to a shape that
//!   moved or changed size are routed again by [`super::settle::settle`] as for any edit.
//! * **A group is one shape**: its `Width`, `Height` and pin change as a shape's do, and **its members follow by their own formulas**, which is how a
//!   group made by `shape.group` ties them to it. A member whose size or place is a number, and not a tie, is carried by the stretch of the group's own
//!   frame ([`scale_members`]): its `PinX`, `PinY`, `Width`, `Height` (and a local pin held as a number) by the factors of the group, its ends if it is a line, and a group among
//!   them again by its own factors. A cell that holds any other formula is data and is left as it is (then makes the group the box of what its members came to).
//!   **A line among the members is routed again** with the rest of the edit: its route is rows of its own frame, which the stretch changes.
//! * **Locks** (`LockWidth`, `LockHeight`: "keeps width (height) fixed during sizing", `LockAspect`: "allows sizing only in proportion"): **a shape with
//!   `LockWidth` or `LockHeight` set is left alone** (neither sized nor moved, and its box still counts in the joint box the others are stretched from), a shape with
//!   `LockAspect` set is scaled by the same factor on both axes (the geometric mean of the two), and when every shape named is locked the resize is refused in words.
//!
//! The members of a turned or flipped group are refused, as they are by every command that carries a place of the page into a group's frame.

use diacraft_doc::{CellAddr, Document, FormulaSlot, ShapeRef, SheetRef};

use super::arrange::{Rect, SAME_PLACE, Write, apply, joint_box, outermost, plan_line, refuse_turned_groups, targets};
use super::group::MAX_GROUP_DEPTH;
use super::read::{Pose, Reader};
use super::route::P;
use super::settle::settle;
use super::shapes::evaluate_number;
use super::{Problem, checked, write_length};

/// A joint box with less than this (drawing units) along an axis has no length to stretch along it.
const MIN_EXTENT: f64 = 1.0e-9;

/// The most members of groups one resize carries by their numbers: a group of more is refused in words and the document is as it was.
pub const MAX_SCALED_MEMBERS: usize = 20_000;

/// What a resize of several shapes did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scaled {
    /// How many shapes changed: those sized, and the connectors with an end moved.
    pub resized: usize,
    /// The shapes left alone because `LockWidth` or `LockHeight` is set, in the order they were named.
    pub locked: Vec<u32>,
}

/// The factor that takes a length of `old` to `new`: `None` when `old` is too short to stretch (under a billionth) or either is not a number.
pub fn factor(old: f64, new: f64) -> Option<f64> {
    (old.is_finite() && new.is_finite() && old.abs() >= MIN_EXTENT).then(|| new / old)
}

/// The factors by which a shape turned by `angle` radians has its width and its height scaled when the page is stretched by `sx` along x and `sy` along y: the lengths the
/// stretch gives the shape's own two axes. With `sx == sy` both are that factor, whatever the angle.
pub fn size_factors(angle: f64, sx: f64, sy: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (((sx * c).powi(2) + (sy * s).powi(2)).sqrt(), ((sx * s).powi(2) + (sy * c).powi(2)).sqrt())
}

/// The place a coordinate `v` goes to when the stretch takes the old low edge `old_low` to `new_low` and scales by `scale` (the map of one page axis).
pub fn carry(v: f64, old_low: f64, new_low: f64, scale: f64) -> f64 {
    new_low + (v - old_low) * scale
}

/// A shape of the set that is sized: where it is and what it comes to.
struct Sized {
    at: ShapeRef,
    sheet: SheetRef,
    pose: Pose,
    old: (f64, f64),
    new: (f64, f64),
    /// Where the middle of the box goes, on the page.
    centre: P,
    group: bool,
}

/// Makes the joint box of the shapes `ids` the box `to` (`x`, `y`, `width`, `height`, drawing units on the page). → what changed; when nothing
/// does, the document is not touched.
pub fn resize_shapes(doc: &mut Document, page: u32, ids: &[u32], to: (f64, f64, f64, f64)) -> Result<Scaled, Problem> {
    let before = doc.clone();
    let (x, y) = (checked("x", to.0)?, checked("y", to.1)?);
    let (width, height) = (checked("width", to.2)?, checked("height", to.3)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(Problem::bad("`width` and `height` must be more than 0"));
    }
    let to = Rect { x0: x, y0: y, x1: checked("x + width", x + width)?, y1: checked("y + height", y + height)? };
    let mut sized: Vec<Sized> = Vec::new();
    let mut line_writes: Vec<Write> = Vec::new();
    let mut locked: Vec<u32> = Vec::new();
    let mut lines = 0_usize;
    let mut touched: Vec<u32> = Vec::new();
    {
        let reader = Reader::new(doc);
        let targets = outermost(targets(doc, &reader, page, ids)?);
        refuse_turned_groups(&reader, &targets, "resized")?;
        // A group's members follow its size: one on a locked layer is not resized through it.
        let named: Vec<u32> = targets.iter().map(|t| t.id).collect();
        super::layers::refuse_locked_member(doc, page, &named, "resized")?;
        let old = joint_box(&reader, &targets)?;
        let (sx, sy) = (stretch(old.x1 - old.x0, width, "width")?, stretch(old.y1 - old.y0, height, "height")?);
        // A resize to the box the shapes have is no resize.
        let same =
            (sx - 1.0).abs() < 1.0e-12 && (sy - 1.0).abs() < 1.0e-12 && (to.x0 - old.x0).abs() < SAME_PLACE && (to.y0 - old.y0).abs() < SAME_PLACE;
        if same {
            return Ok(Scaled::default());
        }
        let map = |p: P| (carry(p.0, old.x0, to.x0, sx), carry(p.1, old.y0, to.y0, sy));
        for t in &targets {
            if ["LockWidth", "LockHeight"].iter().any(|name| reader.number(&t.sheet, name) != 0.0) {
                locked.push(t.id);
                continue;
            }
            touched.push(t.id);
            if t.one_d {
                let planned = line_writes.len();
                plan_line(doc, page, &reader, t, &map, &mut line_writes);
                lines = lines.saturating_add(usize::from(line_writes.len() > planned));
                continue;
            }
            let pose = reader.pose(&t.sheet);
            let corners = reader.corners(&t.at);
            let (w, h) = (reader.number(&t.sheet, "Width"), reader.number(&t.sheet, "Height"));
            let (mut fx, mut fy) = size_factors(pose.angle, sx, sy);
            if reader.number(&t.sheet, "LockAspect") != 0.0 {
                let same_factor = (fx * fy).sqrt();
                (fx, fy) = (same_factor, same_factor);
            }
            let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
            sized.push(Sized {
                at: t.at.clone(),
                sheet: t.sheet.clone(),
                pose,
                old: (w, h),
                new: (checked("width", w * fx)?, checked("height", h * fy)?),
                centre: map(middle),
                group: doc.shape(&t.at).is_some_and(|shape| !shape.children.is_empty()),
            });
        }
        if !targets.is_empty() && locked.len() == targets.len() {
            let named: Vec<String> = locked.iter().map(u32::to_string).collect();
            return Err(Problem::failed(format!(
                "{} locked against sizing (`LockWidth` or `LockHeight`): shape {}; clear the lock to size {}",
                if locked.len() == 1 { "the shape is" } else { "the shapes are" },
                named.join(", "),
                if locked.len() == 1 { "it" } else { "them" }
            )));
        }
    }
    // The sizes first: the local pin of each shape is worked out for its new size, and what a group's members hold as formulas of its size follows.
    for s in &sized {
        for (name, now, was) in [("Width", s.new.0, s.old.0), ("Height", s.new.1, s.old.1)] {
            if (now - was).abs() > SAME_PLACE {
                write_length(doc, &s.sheet, name, now)?;
            }
        }
    }
    // The pins, in one reading of the document: the middle of each box where the map put it, and the pin from the middle by the shape's own turn and flips.
    let mut pins: Vec<(SheetRef, &'static str, f64, f64)> = Vec::new();
    {
        let reader = Reader::new(doc);
        for s in &sized {
            let loc = (
                evaluate_number(doc, &s.sheet, "LocPinX").unwrap_or_else(|| reader.number(&s.sheet, "LocPinX")),
                evaluate_number(doc, &s.sheet, "LocPinY").unwrap_or_else(|| reader.number(&s.sheet, "LocPinY")),
            );
            let middle = reader.point_from_page(&s.at, s.centre);
            let by = s.pose.up_vector((loc.0 - s.new.0 / 2.0, loc.1 - s.new.1 / 2.0));
            pins.push((s.sheet.clone(), "PinX", middle.0 + by.0, s.pose.pin.0));
            pins.push((s.sheet.clone(), "PinY", middle.1 + by.1, s.pose.pin.1));
        }
    }
    for (sheet, name, value, held) in pins {
        if (value - held).abs() > SAME_PLACE {
            write_length(doc, &sheet, name, checked(name, value)?)?;
        }
    }
    // A group's members that hold their place and size as numbers go with the group's frame, and the lines among them are routed again.
    let mut left = MAX_SCALED_MEMBERS;
    let mut member_lines: Vec<u32> = Vec::new();
    for s in sized.iter().filter(|s| s.group) {
        scale_members_of(doc, &s.at, s.old, s.new, &mut left, &mut member_lines)?;
    }
    apply(doc, line_writes)?;
    let count = sized.len().saturating_add(lines);
    if count == 0 {
        return Ok(Scaled { resized: 0, locked });
    }
    settle(doc, &before, page, &touched, &member_lines)?;
    Ok(Scaled { resized: count, locked })
}

/// The factor of one page axis: the new length over the old, or the words that say the shapes have none along it.
fn stretch(old: f64, new: f64, what: &str) -> Result<f64, Problem> {
    factor(old, new).ok_or_else(|| Problem::failed(format!("the shapes' joint box has no {what} to stretch: it is a line along the other axis")))
}

/// Carries the members of the group at `group`, whose size goes from `old` to `new` (`Width`, `Height`), by the stretch of its own frame, within
/// `left` members for the whole command. A group with no size along an axis has nothing to stretch along it.
pub(super) fn scale_members_of(
    doc: &mut Document,
    group: &ShapeRef,
    old: (f64, f64),
    new: (f64, f64),
    left: &mut usize,
    lines: &mut Vec<u32>,
) -> Result<(), Problem> {
    match (factor(old.0, new.0), factor(old.1, new.1)) {
        (Some(sx), Some(sy)) => scale_members(doc, group, (sx, sy), left, lines, 0),
        _ => Ok(()),
    }
}

/// Writes the numbers the members of `group` hold, stretched by (`sx`, `sy`): see the module documentation. Members that are groups are carried by the factors their own size
/// comes to, once the numbers of this level are written (a tie to this group follows its new size).
fn scale_members(
    doc: &mut Document,
    group: &ShapeRef,
    (sx, sy): (f64, f64),
    left: &mut usize,
    lines: &mut Vec<u32>,
    depth: usize,
) -> Result<(), Problem> {
    if depth > MAX_GROUP_DEPTH {
        return Err(Problem::failed("a group inside this many groups is not resized"));
    }
    let mut writes: Vec<(SheetRef, &'static str, f64)> = Vec::new();
    let mut inner: Vec<(ShapeRef, (f64, f64))> = Vec::new();
    {
        let reader = Reader::new(doc);
        let Some(shape) = doc.shape(group) else { return Ok(()) };
        for (index, child) in shape.children.iter().enumerate() {
            if child.del {
                continue;
            }
            *left = left
                .checked_sub(1)
                .ok_or_else(|| Problem::failed(format!("a group of more than {MAX_SCALED_MEMBERS} members is not resized in one step")))?;
            let Ok(step) = u32::try_from(index) else { continue };
            let mut path = group.path.clone();
            path.push(step);
            let at = ShapeRef { owner: group.owner, path };
            let sheet = SheetRef::Shape(at.clone());
            // The number a cell holds when it holds no formula of its own: a formula is data, and a tie to the group follows the group.
            let constant = |name: &str| -> Option<f64> {
                let found = reader.resolver().resolve(&sheet, &CellAddr::direct(name))?;
                if found.formula.cell.is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_))) {
                    return None;
                }
                found.value.cell.and_then(|cell| cell.value.as_ref()).and_then(|value| value.number()).filter(|n| n.is_finite())
            };
            let factors: [(&'static str, f64); 6] = if reader.is_one_d(&sheet) {
                // A line carried by the stretch has its route made again with the rest of the edit.
                lines.push(child.id);
                [("BeginX", sx), ("BeginY", sy), ("EndX", sx), ("EndY", sy), ("PinX", sx), ("PinY", sy)]
            } else {
                let (lx, ly) = size_factors(reader.number(&sheet, "Angle"), sx, sy);
                [("PinX", sx), ("PinY", sy), ("Width", lx), ("Height", ly), ("LocPinX", lx), ("LocPinY", ly)]
            };
            for (name, by) in factors {
                if let Some(value) = constant(name) {
                    writes.push((sheet.clone(), name, value * by));
                }
            }
            if !child.children.is_empty() && !reader.is_one_d(&sheet) {
                inner.push((at, (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"))));
            }
        }
    }
    for (sheet, name, value) in writes {
        write_length(doc, &sheet, name, checked(name, value)?)?;
    }
    for (at, (w, h)) in inner {
        let sheet = SheetRef::Shape(at.clone());
        let (w2, h2) = (evaluate_number(doc, &sheet, "Width").unwrap_or(w), evaluate_number(doc, &sheet, "Height").unwrap_or(h));
        if let (Some(fx), Some(fy)) = (factor(w, w2), factor(h, h2)) {
            scale_members(doc, &at, (fx, fy), left, lines, depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_factor_is_the_new_length_over_the_old_and_none_for_a_length_that_is_not_there() {
        assert_eq!(factor(2.0, 3.0), Some(1.5));
        assert_eq!(factor(-2.0, 1.0), Some(-0.5));
        assert_eq!(factor(0.0, 3.0), None);
        assert_eq!(factor(1.0e-12, 3.0), None);
        assert_eq!(factor(f64::NAN, 3.0), None);
        assert_eq!(factor(2.0, f64::INFINITY), None);
    }

    #[test]
    fn a_stretch_gives_a_shape_the_lengths_it_gives_its_own_axes() {
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12;
        assert!(near(size_factors(0.0, 2.0, 3.0), (2.0, 3.0)), "not turned");
        assert!(near(size_factors(std::f64::consts::FRAC_PI_2, 2.0, 3.0), (3.0, 2.0)), "a quarter turn swaps the axes");
        assert!(near(size_factors(std::f64::consts::PI, 2.0, 3.0), (2.0, 3.0)), "a half turn does not");
        for angle in [0.3, 1.0, -2.0, 4.0] {
            assert!(near(size_factors(angle, 1.5, 1.5), (1.5, 1.5)), "the same stretch on both axes scales a shape by it at any angle: {angle}");
        }
        // At 45 degrees along both of the shape's axes the lengths are the mean square of the two.
        let (a, b) = size_factors(std::f64::consts::FRAC_PI_4, 2.0, 4.0);
        assert!((a - (10.0_f64).sqrt()).abs() < 1e-12 && (b - (10.0_f64).sqrt()).abs() < 1e-12, "{a} {b}");
    }

    #[test]
    fn a_coordinate_is_carried_from_the_old_low_edge_to_the_new_one_by_the_scale() {
        assert_eq!(carry(3.0, 1.0, 10.0, 2.0), 14.0);
        assert_eq!(carry(1.0, 1.0, 10.0, 2.0), 10.0, "the low edge goes to the new low edge");
        assert_eq!(carry(5.0, 1.0, 1.0, 0.5), 3.0);
    }
}
