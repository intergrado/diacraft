//! Dropping a master on a page of another drawing scale: the range of eight.
//!
//! A master carries a drawing scale of its own on its page sheet, and the page it is dropped on carries another. A scale is `PageScale` over `DrawingScale` (both
//! in inches), the master's *s*<sub>m</sub> and the page's *s*<sub>p</sub>; the ratio is *r* = *s*<sub>m</sub> / *s*<sub>p</sub>, and *k* is the greater of *r* and 1/*r*.
//!
//! * **Within the range** (*k* up to 8: a relative 1E-9 above 8 still counts, so that 1:12 against 1:96 is within after a division): the
//!   instance keeps the master's size in drawing units and **nothing is written**.
//! * **Beyond it**: the instance keeps the master's size on paper. The drop writes `DropOnPageScale` = *r* (a plain value with all its digits, no unit) on the
//!   top-level shape, and `Width` and `Height` as the master's times *r*.
//! * **A size formula**: one that a `GUARD` protects stays inherited and is recalculated, and so does one that the shape's lock keeps (`LockWidth` or
//!   `LockHeight` set and the cell a formula: the height of a group of compartments, which a resize keeps too); a plain value or an unguarded formula is replaced by the scaled
//!   value, as a resize replaces it. A one-dimensional shape's `Width` is the distance between its ends, so a formula of the ends is left to follow them and only a plain `Width`
//!   is scaled.
//! * **A one-dimensional master** (**DROPSCALE-1D-1**): its ends lie along the master's direction, the master's length times *r* apart, centred on the drop point.
//! * **Members**: only the top-level shape is written; the members of a group follow by their own formulas. **A master of several top-level
//!   shapes** is the one exception: the members that stand for its top-level shapes are written as the top-level shape is, and their offsets from
//!   the instance's local pin are the master's times *r*.
//! * **What is never written**: text size, line weight, geometry, connection points, the local pin of a shape, constants in formulas. A recalculation follows
//!   the cells written, so a formula that reads `Width` or `DropOnPageScale` has its result.
//! * **Unusable scales**: a `PageScale` or `DrawingScale` that is absent, 0, negative or not a number reads as one inch, each by itself.
//! * **Bounds**: a ratio that is not a finite number above 0, or a size, end or factor that would not be finite or would pass [`MAX_COORD`],
//!   drops the master as within the range, with words in the answer.
//! * **What is not a drop**: a paste, a duplicate, a move, Change Shape ([`Follow::Never`]) and a later change of the page's
//!   scale write none of this.
//!
//! The function that tells within from beyond ([`class_of`]) and the ones that read a scale ([`usable_length`], [`scale_of`]) are small, pure and total.

use diacraft_calc::Changes;
use diacraft_doc::{Cell, CellAddr, CellName, Document, FormulaSlot, Owner, Shape, ShapeRef, Sheet, SheetRef, Value};

use super::read::{Pose, Reader};
use super::worked::Worked;
use super::{MAX_COORD, Problem, checked, shape_mut};
use crate::stencil::number_text;

/// The factor of the range of eight: scales that differ by more than this are beyond it.
pub const RANGE: f64 = 8.0;

/// The relative amount above [`RANGE`] that still counts as within it.
pub const EDGE_TOLERANCE: f64 = 1.0e-9;

/// The greatest ratio, either way, that is within the range.
const LIMIT: f64 = RANGE * (1.0 + EDGE_TOLERANCE);

/// Two places or sizes closer than this are the same.
const EPS: f64 = 1.0e-9;

/// Which side of the range of eight a ratio of scales is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// *k* is 8 or less (the edge counts a little over): the drawing size is kept and nothing is written.
    Within,
    /// *k* is more than 8: the paper size is kept.
    Beyond,
}

/// Whether the ratio `ratio` of the master's drawing scale to the page's is beyond the range of eight, either way (`ratio` above 8, or below an eighth). Total: a ratio that is
/// not a finite number above 0 is [`Class::Within`], and so is one that is 8 or a hair above. It divides nothing, so a ratio too small to invert is beyond.
pub fn class_of(ratio: f64) -> Class {
    if !ratio.is_finite() || ratio <= 0.0 {
        return Class::Within;
    }
    if ratio > LIMIT || ratio * LIMIT < 1.0 { Class::Beyond } else { Class::Within }
}

/// A length of a page sheet as a scale reads it: the number when it is finite and more than 0, and one inch for a cell that is absent, 0, negative or not
/// a number.
pub fn usable_length(value: Option<f64>) -> f64 {
    match value {
        Some(n) if n.is_finite() && n > 0.0 => n,
        _ => 1.0,
    }
}

/// A sheet's drawing scale: its `PageScale` over its `DrawingScale`, each read by [`usable_length`]. It can overflow or underflow to a number that is not a usable ratio; the caller
/// of [`Scaling::of`] says so in words.
pub fn scale_of(paper: Option<f64>, drawing: Option<f64>) -> f64 {
    usable_length(paper) / usable_length(drawing)
}

/// Whether a drop follows the page's drawing scale. Every drop of a master does ([`Follow::Page`]); the trial instance Change Shape makes of the master it swaps to
/// ([`Follow::Never`]) is not a drop on a page: the shape it changes is already there and keeps the size it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    Page,
    Never,
}

/// What a drop makes of the two scales.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scaling {
    /// *r*, when the scales make a usable ratio and the drop is beyond the range; 1 otherwise.
    ratio: f64,
    beyond: bool,
    /// Why a ratio that could not be used was left unused: said in the answer.
    note: Option<String>,
}

impl Scaling {
    /// A drop that follows no scale.
    pub(crate) fn none() -> Scaling {
        Scaling { ratio: 1.0, beyond: false, note: None }
    }

    /// What dropping master `master` on page `page` of `doc` makes of the two scales.
    pub(crate) fn read(doc: &Document, page: u32, master: u32) -> Scaling {
        let worked = Worked::new(doc);
        Scaling::of(scale_read(&worked, &SheetRef::Master(master)), scale_read(&worked, &SheetRef::Page(page)))
    }

    /// The scaling of a master at scale `of_master` dropped on a page at scale `of_page`.
    pub(crate) fn of(of_master: f64, of_page: f64) -> Scaling {
        let ratio = of_master / of_page;
        if !(ratio.is_finite() && ratio > 0.0) {
            return Scaling {
                ratio: 1.0,
                beyond: false,
                note: Some(
                    "the master was dropped at its own size, not scaled to the page's drawing scale: the ratio of the master's drawing scale to the page's is not a number a size can be made from"
                        .to_owned(),
                ),
            };
        }
        match class_of(ratio) {
            Class::Within => Scaling { ratio: 1.0, beyond: false, note: None },
            Class::Beyond => Scaling { ratio, beyond: true, note: None },
        }
    }

    /// Whether the drop is beyond the range: the paper size is kept.
    pub(crate) fn beyond(&self) -> bool {
        self.beyond
    }

    /// *r*, the factor of a drop beyond the range.
    pub(crate) fn factor(&self) -> Option<f64> {
        self.beyond.then_some(self.ratio)
    }

    /// The words of a ratio that was left unused, if it was.
    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// `value` as a drop beyond the range makes it: the value times *r*; unchanged for a drop that is not. `None` when the result is not a finite number within [`MAX_COORD`].
    pub(crate) fn scale(&self, value: f64) -> Option<f64> {
        if !self.beyond {
            return Some(value);
        }
        fits(value * self.ratio)
    }

    /// The drop as within the range, with words that say why: a size, end or factor of the scaled drop would not be a usable number.
    pub(crate) fn too_big(&self) -> Scaling {
        Scaling {
            ratio: 1.0,
            beyond: false,
            note: Some(format!(
                "the master was dropped at its own size, not scaled to the page's drawing scale: the ratio of the master's drawing scale to the page's is {:e}, which would make a size that is not a number within {MAX_COORD:e}",
                self.ratio
            )),
        }
    }
}

/// The size, in drawing units, of a built-in master (drawn at 1:1) of `width` by `height` dropped on a page of `k` drawing units to a paper inch, **for a drop that is made by code
/// and not by an instance of the master** (BPMN's Pool, `cmd/swimlane.rs`): the master's own size within the range of eight, and beyond it its size on paper, which is its times the ratio,
/// `k`; a size that would not be a usable number is the master's own, as the drop does. The command and the preview of the window take it from here.
pub(crate) fn built_in_size(width: f64, height: f64, k: f64) -> (f64, f64) {
    built_in_fit(width, height, k).0
}

/// [`built_in_size`] and **the drawing units that one inch of the master's measure comes to in it**: 1 for the master's own size within the range (a master is drawn at
/// 1:1, so its inch is a drawing unit there), and the ratio, `k`, for its size on paper beyond the range. A least length the master's own figure holds (0.4 inch for a lane of BPMN's
/// Pool) is held to the size in that unit and not in the page's: at 1:8 the pool is 2 drawing units tall, and 0.4 times the 8 drawing units to a paper inch is more than that, which refused
/// the drop.
pub(crate) fn built_in_fit(width: f64, height: f64, k: f64) -> ((f64, f64), f64) {
    let scaling = Scaling::of(1.0, 1.0 / k);
    match (scaling.beyond(), scaling.scale(width), scaling.scale(height)) {
        (true, Some(width), Some(height)) => ((width, height), scaling.ratio),
        _ => ((width, height), 1.0),
    }
}

/// `value` when it is a finite number within [`MAX_COORD`] of 0.
fn fits(value: f64) -> Option<f64> {
    (value.is_finite() && value.abs() <= MAX_COORD).then_some(value)
}

/// The scale of the sheet `sheet` (a page's, or a master's page sheet): `PageScale` over `DrawingScale`, each read by [`usable_length`].
fn scale_read(worked: &Worked<'_>, sheet: &SheetRef) -> f64 {
    // A cell that holds a formula and no saved value is what the formula works out to, as the picture reads it.
    let read = |name: &str| worked.number_named(sheet, name);
    scale_of(read("PageScale"), read("DrawingScale"))
}

/// The size cells a drop beyond the range writes on the master's shape at `sheet`, each the master's value times *r*: `Width` and `Height`, none
/// where a `GUARD` protects the cell (it stays inherited), none where the shape's lock keeps a size that holds a formula (`LockWidth`, `LockHeight`, as a resize keeps it), and for a
/// one-dimensional shape (`one_d`) none where the cell holds a formula (a `Width` that is a formula of the ends follows them). `None` when a value would not be a usable number
/// ([`Scaling::scale`]). Empty for a drop that is not beyond the range.
pub(crate) fn size_cells(worked: &Worked<'_>, sheet: &SheetRef, scaling: &Scaling, one_d: bool) -> Option<Vec<(&'static str, f64)>> {
    let mut out = Vec::new();
    if !scaling.beyond() {
        return Some(out);
    }
    let locked = |lock: &str| worked.number_named(sheet, lock).is_some_and(|n| n != 0.0);
    for (name, lock) in [("Width", "LockWidth"), ("Height", "LockHeight")] {
        let Some(found) = worked.resolver().resolve(sheet, &CellAddr::direct(name)) else { continue };
        let formula = found.formula.cell;
        if formula.is_some_and(Cell::is_guarded) {
            continue;
        }
        if one_d && formula.is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_))) {
            continue;
        }
        // A size the shape's lock keeps and that holds a formula (the height of a group of compartments is the sum of theirs) is kept, as a resize keeps it.
        if locked(lock) && formula.is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_) | FormulaSlot::Inherit)) {
            continue;
        }
        // The master's size, as the picture reads it: a size that is a formula and no saved value is what the formula works out to.
        let Some(value) = worked.number_of(sheet, &CellAddr::direct(name), &found).filter(|n| n.is_finite()) else { continue };
        out.push((name, scaling.scale(value)?));
    }
    Some(out)
}

/// A length written with all its digits, in inches: the scaled `Width` over the master's is then the factor, to the last place.
pub(crate) fn exact_length(value: f64) -> Cell {
    Cell::constant(Value::from_number(value).unwrap_or_else(|| Value::from_text("0"))).with_unit("IN")
}

/// The `DropOnPageScale` cell: a plain value with all its digits and no unit.
pub(crate) fn factor_cell(ratio: f64) -> Cell {
    Cell::constant(Value::from_number(ratio).unwrap_or_else(|| Value::from_text("1")))
}

/// What a drop writes on the instance's top-level shape, besides its pin.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Writes {
    /// *r*, written as `DropOnPageScale`, when the drop is beyond the range.
    pub factor: Option<f64>,
    /// `Width` and `Height` as scaled, each with all its digits.
    pub sizes: Vec<(&'static str, f64)>,
    /// Words for a ratio that was left unused.
    pub note: Option<String>,
}

impl Writes {
    /// What dropping master `master` on page `page`, whose one top-level shape is the first that is not deleted, writes. Nothing for [`Follow::Never`].
    pub(crate) fn of_shape(doc: &Document, page: u32, master: u32, follow: Follow) -> Writes {
        if follow == Follow::Never {
            return Writes::default();
        }
        let scaling = Scaling::read(doc, page, master);
        if !scaling.beyond() {
            return Writes { note: scaling.note().map(str::to_owned), ..Writes::default() };
        }
        let Some(m) = doc.master(master) else { return Writes::default() };
        let Some(top) = m.shapes.iter().position(|s| !s.del).and_then(|at| u32::try_from(at).ok()) else { return Writes::default() };
        let sheet = SheetRef::Shape(ShapeRef::top(Owner::Master(master), top));
        let worked = Worked::new(doc);
        let one_d = ["BeginX", "BeginY", "EndX", "EndY"]
            .iter()
            .all(|n| worked.resolver().resolve(&sheet, &CellAddr::direct(n)).is_some_and(|f| f.formula.cell.is_some() || f.value.cell.is_some()));
        match size_cells(&worked, &sheet, &scaling, one_d) {
            Some(sizes) => Writes { factor: scaling.factor(), sizes, note: None },
            None => Writes { note: scaling.too_big().note().map(str::to_owned), ..Writes::default() },
        }
    }

    /// Puts the cells in `sheet`: `DropOnPageScale` and the sizes. Nothing for a drop that is not beyond the range.
    pub(crate) fn put(&self, sheet: &mut Sheet) {
        if let Some(factor) = self.factor {
            sheet.cells.insert(CellName::new("DropOnPageScale"), factor_cell(factor));
        }
        for (name, value) in &self.sizes {
            sheet.cells.insert(CellName::new(name), exact_length(*value));
        }
    }

    /// The names of the cells written.
    pub(crate) fn names(&self) -> Vec<&'static str> {
        self.factor.map(|_| "DropOnPageScale").into_iter().chain(self.sizes.iter().map(|(name, _)| *name)).collect()
    }
}

/// Works out again what reads `cells`, or says why that cannot be done.
pub(crate) fn recalculate(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    if cells.is_empty() {
        return Ok(());
    }
    super::settle::commit(doc, page, &Changes { cells, ..Changes::default() })
}

/// After a drop beyond the range wrote `names` on the top-level shape `id`: works out again what reads them, and, for a two-dimensional shape, puts the pin where the new box has
/// its middle at `centre` (the local pin is the master's formula, which follows the size by whatever it is: the drop never writes it).
pub(crate) fn settle(doc: &mut Document, page: u32, id: u32, centre: Option<(f64, f64)>, names: &[&'static str]) -> Result<(), Problem> {
    let at = doc.find_shape(Owner::Page(page), id).ok_or_else(|| Problem::failed("the new shape went away"))?;
    recalculate(doc, page, names.iter().map(|name| (SheetRef::Shape(at.clone()), CellAddr::direct(name))).collect())?;
    match centre {
        Some(centre) => recentre(doc, page, &at, centre),
        None => Ok(()),
    }
}

/// Puts the pin of the top-level shape at `at` where the middle of its box is at (`x`, `y`): the offset from the pin to the middle, carried through the shape's turn and flips, is
/// what the pin is moved by. Nothing is written when the pin is there.
pub(crate) fn recentre(doc: &mut Document, page: u32, at: &ShapeRef, (x, y): (f64, f64)) -> Result<(), Problem> {
    let ((want_x, want_y), (pin_x, pin_y)) = {
        let reader = Reader::new(doc);
        let sheet = SheetRef::Shape(at.clone());
        let (width, height) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
        let pose = reader.pose(&sheet);
        let offset = Pose { pin: (0.0, 0.0), ..pose }.up((width / 2.0, height / 2.0));
        ((x - offset.0, y - offset.1), pose.pin)
    };
    if (want_x - pin_x).abs() <= EPS && (want_y - pin_y).abs() <= EPS {
        return Ok(());
    }
    let (want_x, want_y) = (checked("x", want_x)?, checked("y", want_y)?);
    let shape: &mut Shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the new shape went away"))?;
    let mut counted = shape.counted_sheet();
    for (name, value) in [("PinX", want_x), ("PinY", want_y)] {
        counted
            .set_cell(&CellAddr::direct(name), Cell::constant(Value::from_text(&number_text(value))).with_unit("IN"))
            .map_err(|e| Problem::failed(e.to_string()))?;
    }
    recalculate(doc, page, vec![(SheetRef::Shape(at.clone()), CellAddr::direct("PinX")), (SheetRef::Shape(at.clone()), CellAddr::direct("PinY"))])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The edge: 8 is within and a hair above it is, but the tolerance ends there.
    #[test]
    fn the_range_is_eight_either_way_with_a_hair_of_tolerance_at_the_edge() {
        for within in [1.0, 2.0, 5.0, 8.0, 8.0 * (1.0 + 1.0e-12), 1.0 / 8.0, 0.5, 0.125 * (1.0 - 1.0e-12), 8.000_000_000_000_002] {
            assert_eq!(class_of(within), Class::Within, "{within}");
        }
        for beyond in [8.34, 8.333_333, 9.0, 10.0, 100.0, 1.0e300, 1.0 / 8.34, 0.1, 0.02, 1.0e-300, f64::MIN_POSITIVE, 5.0e-324] {
            assert_eq!(class_of(beyond), Class::Beyond, "{beyond}");
        }
        // The tolerance is a relative 1E-9 and no more.
        assert_eq!(class_of(8.0 * (1.0 + 1.0e-8)), Class::Beyond);
        assert_eq!(class_of(1.0 / (8.0 * (1.0 + 1.0e-8))), Class::Beyond);
    }

    /// A ratio that is not a finite number above 0 is within, for the planner to say so in words; nothing divides by it.
    #[test]
    fn a_ratio_that_is_not_a_usable_number_is_within() {
        for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, -1.0, -100.0, f64::MIN] {
            assert_eq!(class_of(ratio), Class::Within, "{ratio}");
        }
    }

    /// A scale cell that is absent, 0, negative or not a number reads as one inch, each by itself.
    #[test]
    fn a_scale_cell_that_cannot_be_used_reads_as_one_inch_each_by_itself() {
        for bad in [None, Some(0.0), Some(-3.0), Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY), Some(-0.0)] {
            assert_eq!(usable_length(bad), 1.0, "{bad:?}");
        }
        assert_eq!(usable_length(Some(0.5)), 0.5);
        assert_eq!(scale_of(Some(0.0), Some(1.0)), 1.0, "a master page of PageScale 0 and DrawingScale 1 is at 1:1");
        assert_eq!(scale_of(None, None), 1.0);
        assert_eq!(scale_of(Some(2.0), Some(f64::NAN)), 2.0, "the cell that is usable counts");
        assert_eq!(scale_of(Some(1.0), Some(100.0)), 0.01);
    }

    /// The ratios of the vectors: V1 (5), V2 (10), V5 (8), V6 (8.33), V7 (1/9 and 1/8) and the unusable ones.
    #[test]
    fn the_ratios_of_the_vectors_are_worked_out_as_they_say() {
        let at = |paper: f64, drawing: f64| paper / drawing;
        let beyond = |master: f64, page: f64| Scaling::of(master, page).beyond();
        assert!(!beyond(at(0.5, 12.0), at(1.0, 120.0)), "r = 5");
        assert!(beyond(at(0.5, 12.0), at(1.0, 240.0)), "r = 10");
        assert!(!beyond(at(1.0, 12.0), at(0.125, 12.0)), "r = 8");
        assert!(beyond(at(1.0, 12.0), at(0.393_701, 39.3701)), "r = 8.33");
        assert!(beyond(1.0, 9.0), "a page at 9:1");
        assert!(!beyond(1.0, 8.0), "a page at 8:1");
        assert_eq!(Scaling::of(at(0.5, 12.0), at(1.0, 240.0)).factor().map(|r| (r * 1.0e9).round() / 1.0e9), Some(10.0));
    }

    /// A ratio that overflows, underflows or is not a number makes no scaling and says why; a size past the cap does too.
    #[test]
    fn a_ratio_or_a_size_that_is_not_usable_is_said_in_words() {
        for (master, page) in
            [(1.0e300, 1.0e-300), (1.0e-300, 1.0e300), (f64::NAN, 1.0), (1.0, f64::NAN), (f64::INFINITY, 1.0), (1.0, 0.0), (0.0, 1.0)]
        {
            let s = Scaling::of(master, page);
            assert!(!s.beyond() && s.factor().is_none(), "{master}/{page}");
            assert!(s.note().is_some_and(|n| n.contains("not scaled") && n.contains("not a number")), "{master}/{page}: {:?}", s.note());
        }
        let big = Scaling::of(1.0e300, 1.0);
        assert!(big.beyond() && big.note().is_none());
        assert_eq!(big.scale(1.0e10), None, "an infinite width is not written");
        assert_eq!(big.scale(1.0e-300).map(|n| (n - 1.0).abs() < 1.0e-12), Some(true), "a width that fits is");
        let down = big.too_big();
        assert!(!down.beyond() && down.note().is_some_and(|n| n.contains("1e300")), "{:?}", down.note());
        assert_eq!(down.scale(7.0), Some(7.0), "dropped as within: nothing is scaled");
        assert_eq!(Scaling::of(100.0, 1.0).scale(2.0e5), None, "past the cap for a length");
    }

    /// The factor and a length are written with all their digits, the factor as a plain value with no unit and no formula.
    #[test]
    fn the_factor_is_a_plain_value_with_all_its_digits_and_a_length_has_the_unit() {
        let r = 1.0 / 96.0;
        let cell = factor_cell(r);
        assert!(cell.unit.is_none() && matches!(cell.formula, FormulaSlot::Absent));
        assert_eq!(cell.value.as_ref().and_then(Value::number), Some(r));
        let width = exact_length(r * 3.0);
        assert_eq!(width.unit.as_deref(), Some("IN"));
        assert_eq!(width.value.as_ref().and_then(Value::number), Some(r * 3.0));
    }

    /// No drop follows the page when asked not to (Change Shape's trial instance), and an unscaled scaling changes no value.
    #[test]
    fn no_scale_is_followed_when_none_is_asked() {
        let none = Scaling::none();
        assert!(!none.beyond() && none.factor().is_none() && none.note().is_none());
        assert_eq!(none.scale(123.456), Some(123.456));
        assert_eq!(
            none.scale(f64::INFINITY),
            Some(f64::INFINITY),
            "a value that is not scaled is passed on as it is: the caller's own checks refuse it"
        );
    }
}
