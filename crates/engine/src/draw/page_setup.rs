//! Page setup: what a page says about its size, orientation, drawing scale, printer paper and Auto Size, and the writes that change them.
//!
//! **What is kept in cells**. `PageWidth` and `PageHeight` are in **drawing units** and `PageScale` over `DrawingScale` gives the
//! paper inches of one, so a page on paper is `PageWidth` times `PageScale` over `DrawingScale`. A page has no orientation
//! cell: it is landscape when it is wider than tall. `PaperKind` and `PrintPageOrientation` say what it prints on, apart from its
//! size (of the pages that print sideways, 86 are taller than wide). `DrawingResizeType` says whether the page grows by itself
//! Every length a command here takes or gives is in **paper inches**, as the other editing commands' are.
//!
//! **Project rules** (ours; no specification settles them, and the check list lists them):
//!
//! * A named size is portrait here (width at most height); choosing one gives the page that size **in the orientation it has**
//!   (a landscape page becomes the size turned). A custom size is taken as given. No shape moves when the size changes
//!   (the page grows from its lower-left corner).
//! * A named size that is a paper of the table (`paper::PAPERS`) also sets `PaperKind` and `PrintPageOrientation`, so that the
//!   page is one sheet. A custom size, and a named size with no paper (A0, the ARCH sizes), leave the paper alone.
//! * A page whose sheet names no paper, or a paper the table has no size for, prints on US Letter. A page whose
//!   `PrintPageOrientation` is absent, 0 (as the printer is set) or not 1 or 2 is held the way its own size is.
//! * Orientation turns the page, as `PageWidth` and `PageHeight` swapped, and turns the paper (`PrintPageOrientation`). A page that
//!   already has the orientation and the paper that has it is left alone, and the command is no undo step.
//! * A change of drawing scale keeps the page's size **on paper**: `PageWidth` and `PageHeight` are worked out again for the new
//!   scale. The shapes' cells are not touched (they print at a new size).
//! * `DrawingSizeType` and `DrawingScaleType` are written as the labels they are (they
//!   decide nothing that draws). Writing `DrawingSizeType` must not turn Auto Size on or off through `DrawingResizeType`'s 0, which defers to it
//!   So a page whose `DrawingResizeType` is 0 is first given the 1 or 2 its state comes to.
//! * Auto Size is on when `DrawingResizeType` is 1, or is 0 and `DrawingSizeType` is 0 or absent;
//!   off when it is 2 or any other number. **A page that holds no `DrawingResizeType` at all, as the pages of a new drawing, has
//!   it off**: `Page.AutoSize` says off is the default.

use std::sync::Arc;

use diacraft_calc::Changes;
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, SheetRef, Value};
use diacraft_units::Unit;

use super::paper::{self, DEFAULT_PAPER, Orientation, Paper, SCALES, ScalePreset, SizePreset};
use super::read::{Reader, units_per_inch};
use super::settle::{route_recalculated, settle_kept};
use super::{MAX_COORD, Problem, write_cells};
use crate::stencil::number_text;

/// The smallest page, on paper, a command makes: a tenth of an inch each way. The files hold pages from 0.775 inch.
pub const MIN_PAGE_INCHES: f64 = 0.1;

/// The largest page, on paper, a command makes or Auto Size grows to: 10,000 inches each way (the files hold pages up to 3,307).
pub const MAX_PAGE_INCHES: f64 = 10_000.0;

/// The longest length a drawing scale is made of, in inches, and the range of its ratio (drawing units to a paper inch).
const MAX_SCALE_INCHES: f64 = 1.0e9;
const MIN_UNITS_PER_INCH: f64 = 1.0e-3;
const MAX_UNITS_PER_INCH: f64 = 1.0e6;

/// Two numbers closer than this, in inches or in a count, are the same.
const SAME: f64 = 1.0e-9;

/// The printer paper of a page and the way round it is held.
#[derive(Clone, Debug, PartialEq)]
pub struct PaperSetup {
    /// `PaperKind`, when the page's sheet holds one.
    pub kind: Option<u32>,
    /// The table's paper for it, when it has one.
    pub known: Option<&'static Paper>,
    /// The size is a stand-in: the page names no paper, or one with no size in the table.
    pub assumed: bool,
    /// One sheet in the way it is held, in paper inches.
    pub width: f64,
    pub height: f64,
    pub orientation: Orientation,
    /// `PrintPageOrientation` says the way (1 or 2): when it does not, the way is the page's own.
    pub orientation_set: bool,
}

/// The drawing scale of a page.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleSetup {
    /// `PageScale` and `DrawingScale`, in inches (`V`), and the unit each shows in (`U`; empty when the cell holds none).
    pub paper: f64,
    pub drawing: f64,
    pub paper_unit: String,
    pub drawing_unit: String,
    /// `DrawingScaleType`.
    pub kind: u8,
    /// The scale of the list this ratio is, if it is one.
    pub preset: Option<&'static ScalePreset>,
}

/// What a page's cells say about its setup.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub page: u32,
    pub name: String,
    pub background: bool,
    /// The `ID` of the background page the page names (`BackPage`).
    pub back_page: Option<u32>,
    /// The page on paper, in inches.
    pub width: f64,
    pub height: f64,
    /// `PageWidth` and `PageHeight`.
    pub drawing_width: f64,
    pub drawing_height: f64,
    /// Drawing units to a paper inch: `DrawingScale` over `PageScale` (1 for a scale that cannot be read).
    pub units_per_inch: f64,
    pub orientation: Orientation,
    /// The named size the page is, in either orientation.
    pub size: Option<&'static SizePreset>,
    pub paper: PaperSetup,
    pub scale: ScaleSetup,
    pub auto_size: bool,
    /// How many sheets of the paper the page covers across and down.
    pub sheets: (u32, u32),
}

/// A cell directly under the page's sheet to write: a number, and the unit it shows in when the write sets one.
#[derive(Clone, Copy, Debug)]
struct Put {
    name: &'static str,
    value: f64,
    unit: Option<&'static str>,
}

impl Put {
    /// A number written to nine places, as the other editing commands write them.
    fn plain(name: &'static str, value: f64) -> Put {
        Put { name, value, unit: None }
    }

    /// A scale cell, with its unit and **all the digits of its number**: the two scale cells are divided by each other for every length on the page,
    /// so a millimetre written as 0.039370079 would make 1:100 a ratio of 99.99999997.
    fn scale(name: &'static str, value: f64, unit: &'static str) -> Put {
        Put { name, value, unit: Some(unit) }
    }

    fn text(&self) -> String {
        if self.unit.is_some() {
            Value::from_number(self.value).map_or_else(|| number_text(self.value), |v| v.text().to_owned())
        } else {
            number_text(self.value)
        }
    }
}

/// The number the cell `name` of `sheet` holds, if some sheet on the way holds it.
fn number_if_set(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> Option<f64> {
    let addr = CellAddr::direct(name);
    reader.is_set(sheet, &addr).then(|| reader.addr_number(sheet, &addr))
}

/// A whole number from 0 to `u32::MAX`, if `v` is one.
fn whole(v: f64) -> Option<u32> {
    (v.is_finite() && v >= 0.0 && v <= f64::from(u32::MAX) && (v - v.round()).abs() < SAME).then(|| v.round() as u32)
}

fn is(v: f64, n: f64) -> bool {
    (v - n).abs() < 0.5
}

/// Whether a page grows by itself.
pub(crate) fn auto_size_on(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
    match number_if_set(reader, sheet, "DrawingResizeType") {
        Some(v) if is(v, 1.0) => true,
        Some(v) if is(v, 0.0) => number_if_set(reader, sheet, "DrawingSizeType").is_none_or(|t| is(t, 0.0)),
        _ => false,
    }
}

/// A page's size and its paper, as the Auto Size hook and the setters need them.
pub(crate) struct Geometry {
    /// `PageWidth` and `PageHeight`, in drawing units: more than 0 and finite.
    pub width: f64,
    pub height: f64,
    /// Drawing units to a paper inch.
    pub k: f64,
    pub paper: PaperSetup,
}

impl Geometry {
    /// One sheet of the paper, in drawing units.
    pub fn tile(&self) -> (f64, f64) {
        (self.paper.width * self.k, self.paper.height * self.k)
    }
}

/// The paper a page prints on and the way round it is held.
fn paper_of(reader: &Reader<'_>, sheet: &SheetRef, page_way: Orientation) -> PaperSetup {
    let kind = number_if_set(reader, sheet, "PaperKind").and_then(whole);
    let known = kind.and_then(paper::paper);
    let (assumed, stand_in) = (known.is_none(), paper::paper(DEFAULT_PAPER));
    let (short, long) = known.or(stand_in).map_or((8.5, 11.0), |p| (p.width, p.height));
    let way = number_if_set(reader, sheet, "PrintPageOrientation");
    let (orientation, orientation_set) = match way {
        Some(v) if is(v, 1.0) => (Orientation::Portrait, true),
        Some(v) if is(v, 2.0) => (Orientation::Landscape, true),
        _ => (page_way, false),
    };
    let (width, height) = if orientation == Orientation::Landscape { (long, short) } else { (short, long) };
    PaperSetup { kind, known, assumed, width, height, orientation, orientation_set }
}

/// The geometry of a page, or why it has none.
pub(crate) fn geometry(doc: &Document, reader: &Reader<'_>, page: u32) -> Result<Geometry, Problem> {
    if doc.page(page).is_none() {
        return Err(Problem::bad(format!("there is no page {page}")));
    }
    let sheet = SheetRef::Page(page);
    let (width, height) = (reader.number(&sheet, "PageWidth"), reader.number(&sheet, "PageHeight"));
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(Problem::failed("the page has no size: PageWidth and PageHeight must be more than 0"));
    }
    let k = units_per_inch(doc, page);
    let paper = paper_of(reader, &sheet, Orientation::of(width, height));
    Ok(Geometry { width, height, k, paper })
}

/// The unit a cell shows in, or empty.
fn unit_of(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> String {
    reader
        .resolver()
        .resolve(sheet, &CellAddr::direct(name))
        .and_then(|found| found.value.cell.and_then(|c| c.unit.as_deref().map(str::to_owned)))
        .unwrap_or_default()
}

fn scale_of(reader: &Reader<'_>, sheet: &SheetRef, k: f64) -> ScaleSetup {
    let kind = number_if_set(reader, sheet, "DrawingScaleType").and_then(whole).and_then(|v| u8::try_from(v).ok()).unwrap_or(0);
    let ratio = |p: &ScalePreset| p.drawing.1.to_internal(p.drawing.0) / p.paper.1.to_internal(p.paper.0);
    let same = |p: &&ScalePreset| (ratio(p) - k).abs() <= 1.0e-6 * k;
    let preset = SCALES.iter().filter(same).find(|p| p.kind == kind).or_else(|| SCALES.iter().find(same));
    ScaleSetup {
        paper: reader.number(sheet, "PageScale"),
        drawing: reader.number(sheet, "DrawingScale"),
        paper_unit: unit_of(reader, sheet, "PageScale"),
        drawing_unit: unit_of(reader, sheet, "DrawingScale"),
        kind,
        preset,
    }
}

/// How many sheets of `tile` it takes to cover `length`, at least one. A page that is the paper's size to within a thousandth of an inch is one sheet
/// (a file's A4 page is 8.268 by 11.693 inches, and the paper is 8.2677 by 11.6929).
fn sheets_across(length: f64, tile: f64) -> u32 {
    let n = ((length - paper::SIZE_TOLERANCE) / tile).ceil();
    if n.is_finite() && n >= 1.0 { n.min(f64::from(u32::MAX)) as u32 } else { 1 }
}

/// What the cells of page `page` say about its setup.
pub fn read_setup(doc: &Document, page: u32) -> Result<PageSetup, Problem> {
    let reader = Reader::new(doc);
    let g = geometry(doc, &reader, page)?;
    let sheet = SheetRef::Page(page);
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let (width, height) = (g.width / g.k, g.height / g.k);
    Ok(PageSetup {
        page,
        name: p.name.to_string(),
        background: p.background,
        back_page: p.back_page,
        width,
        height,
        drawing_width: g.width,
        drawing_height: g.height,
        units_per_inch: g.k,
        orientation: Orientation::of(g.width, g.height),
        size: paper::size_named(width, height),
        sheets: (sheets_across(width, g.paper.width), sheets_across(height, g.paper.height)),
        scale: scale_of(&reader, &sheet, g.k),
        auto_size: auto_size_on(&reader, &sheet),
        paper: g.paper,
    })
}

/// Writes the cells that differ from what the page resolves them to, and works out what reads them. → whether anything was written.
fn apply(doc: &mut Document, page: u32, cells: &[Put]) -> Result<bool, Problem> {
    let sheet = SheetRef::Page(page);
    let writes: Vec<(SheetRef, CellAddr, CellEdit)> = {
        let reader = Reader::new(doc);
        cells
            .iter()
            .filter(|put| {
                let same_value = match number_if_set(&reader, &sheet, put.name) {
                    Some(v) => (v - put.value).abs() < SAME,
                    // A cell the page does not hold reads 0.
                    None => put.value == 0.0,
                };
                let same_unit = put.unit.is_none_or(|u| unit_of(&reader, &sheet, put.name) == u);
                !(same_value && same_unit)
            })
            .map(|put| {
                let value = Value::from_text(&put.text());
                let edit = match put.unit {
                    Some(unit) => CellEdit::Cell(Cell::constant(value).with_unit(unit)),
                    None => CellEdit::Value(value),
                };
                (sheet.clone(), CellAddr::direct(put.name), edit)
            })
            .collect()
    };
    if writes.is_empty() {
        return Ok(false);
    }
    let changed: Vec<(SheetRef, CellAddr)> = writes.iter().map(|(sheet, addr, _)| (sheet.clone(), addr.clone())).collect();
    let was = doc.clone();
    write_cells(doc, writes)?;
    settle_kept(doc, page, &Changes { cells: changed, ..Changes::default() })?;
    // The page's cells moved no shape themselves, but a shape whose formula reads one (the width of a title block, a size that follows the page) is resized by the recalculation, and
    // the connectors glued to it follow.
    route_recalculated(doc, &was, page)?;
    Ok(true)
}

/// The write that keeps Auto Size as it is when `DrawingSizeType` is about to change.
fn keep_auto_size(doc: &Document, page: u32) -> Option<Put> {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let deferring = number_if_set(&reader, &sheet, "DrawingResizeType").is_some_and(|v| is(v, 0.0));
    deferring.then(|| Put::plain("DrawingResizeType", if auto_size_on(&reader, &sheet) { 1.0 } else { 2.0 }))
}

fn check_inches(what: &str, inches: f64) -> Result<(), Problem> {
    if inches.is_finite() && (MIN_PAGE_INCHES..=MAX_PAGE_INCHES).contains(&inches) {
        Ok(())
    } else {
        Err(Problem::bad(format!("`{what}` must be a size from {MIN_PAGE_INCHES} to {MAX_PAGE_INCHES} inches")))
    }
}

/// The size a page is given.
#[derive(Clone, Copy, Debug)]
pub enum SizeChoice {
    /// A named size, in the page's orientation.
    Preset(&'static SizePreset),
    /// A width and a height in paper inches.
    Custom { width: f64, height: f64 },
}

/// Gives the page a size. → whether the document changed.
pub fn set_size(doc: &mut Document, page: u32, choice: SizeChoice) -> Result<bool, Problem> {
    let g = geometry(doc, &Reader::new(doc), page)?;
    let landscape = Orientation::of(g.width, g.height) == Orientation::Landscape;
    let (width, height) = match choice {
        SizeChoice::Preset(p) if landscape => (p.height, p.width),
        SizeChoice::Preset(p) => (p.width, p.height),
        SizeChoice::Custom { width, height } => (width, height),
    };
    check_inches("width", width)?;
    check_inches("height", height)?;
    let (drawing_width, drawing_height) = (width * g.k, height * g.k);
    if drawing_width > MAX_COORD || drawing_height > MAX_COORD {
        return Err(Problem::failed(format!("that size is more than {MAX_COORD} drawing units on this page's drawing scale")));
    }
    let mut cells = vec![Put::plain("PageWidth", drawing_width), Put::plain("PageHeight", drawing_height)];
    let label = match choice {
        SizeChoice::Preset(p) => {
            if let Some(kind) = p.paper_kind {
                cells.push(Put::plain("PaperKind", f64::from(kind)));
                cells.push(Put::plain("PrintPageOrientation", Orientation::of(width, height).print_code()));
            }
            f64::from(p.size_type)
        }
        SizeChoice::Custom { .. } if (g.k - 1.0).abs() < SAME => 3.0,
        SizeChoice::Custom { .. } => 4.0,
    };
    cells.extend(keep_auto_size(doc, page));
    cells.push(Put::plain("DrawingSizeType", label));
    apply(doc, page, &cells)
}

/// Turns the page and its paper. → whether the document changed.
pub fn set_orientation(doc: &mut Document, page: u32, target: Orientation) -> Result<bool, Problem> {
    let g = geometry(doc, &Reader::new(doc), page)?;
    let mut cells = Vec::new();
    if Orientation::of(g.width, g.height) != target && (g.width - g.height).abs() > SAME {
        cells.push(Put::plain("PageWidth", g.height));
        cells.push(Put::plain("PageHeight", g.width));
    }
    cells.push(Put::plain("PrintPageOrientation", target.print_code()));
    apply(doc, page, &cells)
}

/// The printer paper a page is given: the kind, the way round, or both.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaperChoice {
    pub kind: Option<u32>,
    pub orientation: Option<Orientation>,
}

/// Gives the page its printer paper and the way it is held; the page's own size is not touched. → whether the document changed.
pub fn set_paper(doc: &mut Document, page: u32, choice: PaperChoice) -> Result<bool, Problem> {
    geometry(doc, &Reader::new(doc), page)?;
    let mut cells = Vec::new();
    if let Some(kind) = choice.kind {
        if paper::paper(kind).is_none() {
            return Err(Problem::bad(format!("paper {kind} is not one whose size is known (see page.presets)")));
        }
        cells.push(Put::plain("PaperKind", f64::from(kind)));
    }
    if let Some(way) = choice.orientation {
        cells.push(Put::plain("PrintPageOrientation", way.print_code()));
    }
    apply(doc, page, &cells)
}

/// The drawing scale a page is given.
#[derive(Clone, Copy, Debug)]
pub enum ScaleChoice {
    Preset(&'static ScalePreset),
    /// A length on paper and the length in the drawing that it stands for, each a number and its unit.
    Custom {
        paper: (f64, Unit),
        drawing: (f64, Unit),
    },
}

/// The units a custom drawing scale may be given in.
pub fn length_unit(text: &str) -> Option<Unit> {
    let unit = diacraft_units::parse_unit(text)?;
    matches!(unit.code(), "IN" | "FT" | "YD" | "MI" | "MM" | "CM" | "M" | "KM").then_some(unit)
}

/// Gives the page a drawing scale and keeps its size on paper. → whether the document changed.
pub fn set_scale(doc: &mut Document, page: u32, choice: ScaleChoice) -> Result<bool, Problem> {
    let g = geometry(doc, &Reader::new(doc), page)?;
    let ((paper_value, paper_unit), (drawing_value, drawing_unit), family) = match choice {
        ScaleChoice::Preset(p) => (p.paper, p.drawing, p.kind),
        ScaleChoice::Custom { paper, drawing } => (paper, drawing, 3),
    };
    let (paper_in, drawing_in) = (paper_unit.to_internal(paper_value), drawing_unit.to_internal(drawing_value));
    for (what, inches) in [("paper", paper_in), ("drawing", drawing_in)] {
        if !(inches.is_finite() && inches > 0.0 && inches <= MAX_SCALE_INCHES) {
            return Err(Problem::bad(format!("`{what}` must be a length of more than 0 and at most {MAX_SCALE_INCHES} inches")));
        }
    }
    let k = drawing_in / paper_in;
    if !(MIN_UNITS_PER_INCH..=MAX_UNITS_PER_INCH).contains(&k) {
        return Err(Problem::bad(format!(
            "a drawing scale must stand for between {MIN_UNITS_PER_INCH} and {MAX_UNITS_PER_INCH} drawing units to a paper inch"
        )));
    }
    let family = if family == 3 && (k - 1.0).abs() < SAME { 0 } else { family };
    let (width, height) = (g.width / g.k * k, g.height / g.k * k);
    if width > MAX_COORD || height > MAX_COORD {
        return Err(Problem::failed(format!("the page would be more than {MAX_COORD} drawing units at that scale")));
    }
    let cells = [
        Put::scale("PageScale", paper_in, paper_unit.code()),
        Put::scale("DrawingScale", drawing_in, drawing_unit.code()),
        Put::plain("DrawingScaleType", f64::from(family)),
        Put::plain("PageWidth", width),
        Put::plain("PageHeight", height),
    ];
    apply(doc, page, &cells)
}

/// Turns Auto Size on or off for the page: `DrawingResizeType` 1 or 2. A page that is in the state asked for already is left alone, whatever
/// cells say so. → whether the document changed.
pub fn set_auto_size(doc: &mut Document, page: u32, on: bool) -> Result<bool, Problem> {
    if doc.page(page).is_none() {
        return Err(Problem::bad(format!("there is no page {page}")));
    }
    if auto_size_on(&Reader::new(doc), &SheetRef::Page(page)) == on {
        return Ok(false);
    }
    apply(doc, page, &[Put::plain("DrawingResizeType", if on { 1.0 } else { 2.0 })])
}

/// Gives the page `to` the page setup of the page `from`: its whole page sheet, so size, orientation, scale, units, shadow, ruler and grid.
pub fn copy_setup(doc: &mut Document, from: u32, to: u32) -> Result<(), Problem> {
    let sheet = doc.page(from).map(|p| Arc::clone(&p.sheet)).ok_or_else(|| Problem::bad(format!("there is no page {from}")))?;
    let target = super::page_mut(doc, to).ok_or_else(|| Problem::bad(format!("there is no page {to}")))?;
    target.sheet = sheet;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn page_with(cells: &[(&str, &str)]) -> Document {
        let mut doc = Document::new_blank();
        for (name, value) in cells {
            doc.write_cell(
                &SheetRef::Page(1),
                &CellAddr::direct(name),
                CellEdit::Value(Value::from_text(value)),
                diacraft_doc::WriteOrigin::Automation,
            )
            .unwrap();
        }
        doc
    }

    fn auto(doc: &Document) -> bool {
        auto_size_on(&Reader::new(doc), &SheetRef::Page(1))
    }

    #[test]
    fn auto_size_reads_the_cells_as_the_specification_says() {
        // 1 grows, 2 does not, 0 defers to DrawingSizeType, which grows only at 0.
        assert!(auto(&page_with(&[("DrawingResizeType", "1")])));
        assert!(!auto(&page_with(&[("DrawingResizeType", "2")])));
        assert!(auto(&page_with(&[("DrawingResizeType", "0"), ("DrawingSizeType", "0")])));
        assert!(auto(&page_with(&[("DrawingResizeType", "0")])), "a size type that is absent is 0");
        assert!(!auto(&page_with(&[("DrawingResizeType", "0"), ("DrawingSizeType", "3")])));
        assert!(!auto(&page_with(&[("DrawingResizeType", "7")])), "a number the specification does not list is off");
        assert!(!auto(&page_with(&[])), "A page with no DrawingResizeType has it off");
        assert!(!auto(&page_with(&[("DrawingSizeType", "0")])));
    }

    #[test]
    fn a_new_page_is_letter_landscape_on_letter_paper_held_the_way_the_page_is() {
        let doc = Document::new_blank();
        let s = read_setup(&doc, 1).unwrap();
        assert_eq!((s.width, s.height, s.orientation), (11.0, 8.5, Orientation::Landscape));
        assert_eq!(s.size.map(|p| p.id), Some("letter"));
        assert!(s.paper.assumed && s.paper.kind.is_none() && !s.paper.orientation_set, "{:?}", s.paper);
        assert_eq!((s.paper.width, s.paper.height, s.paper.orientation), (11.0, 8.5, Orientation::Landscape));
        assert_eq!(s.sheets, (1, 1));
        assert!(!s.auto_size);
        assert_eq!(s.scale.preset.map(|p| p.id), Some("none"));
    }

    #[test]
    fn a_page_of_a4_in_millimetres_on_a4_paper_is_one_sheet() {
        // A file's A4 page (8.268 by 11.693) with a root-style PaperKind 9 and no orientation of its own.
        let doc = page_with(&[("PageWidth", "8.268"), ("PageHeight", "11.693"), ("PaperKind", "9")]);
        let s = read_setup(&doc, 1).unwrap();
        assert_eq!((s.orientation, s.size.map(|p| p.id), s.paper.kind), (Orientation::Portrait, Some("a4"), Some(9)));
        assert!(!s.paper.assumed);
        assert_eq!(s.sheets, (1, 1), "a page that is the paper rounded to a thousandth is one sheet");
    }

    #[test]
    fn sheets_are_counted_whole() {
        assert_eq!(sheets_across(11.0, 11.0), 1);
        assert_eq!(sheets_across(11.000_5, 11.0), 1);
        assert_eq!(sheets_across(11.01, 11.0), 2);
        assert_eq!(sheets_across(33.0, 11.0), 3);
        assert_eq!(sheets_across(0.5, 11.0), 1);
    }

    #[test]
    fn an_unknown_paper_is_letter_and_says_so() {
        let doc = page_with(&[("PaperKind", "32767"), ("PrintPageOrientation", "1")]);
        let s = read_setup(&doc, 1).unwrap();
        assert!(s.paper.assumed);
        assert_eq!((s.paper.kind, s.paper.width, s.paper.height, s.paper.orientation), (Some(32767), 8.5, 11.0, Orientation::Portrait));
    }

    #[test]
    fn a_page_without_a_size_is_an_error_to_read_and_nothing_to_set() {
        let doc = page_with(&[("PageWidth", "0")]);
        assert!(read_setup(&doc, 1).is_err());
        let mut doc = doc;
        let before = doc.clone();
        assert!(set_size(&mut doc, 1, SizeChoice::Custom { width: 5.0, height: 5.0 }).is_err());
        assert_eq!(doc, before);
        assert!(read_setup(&doc, 99).unwrap_err().bad_params);
    }

    #[test]
    fn the_units_a_custom_scale_may_use() {
        for ok in ["in", "FT", "yd", "mi", "mm", "cm", "m", "km"] {
            assert!(length_unit(ok).is_some(), "{ok}");
        }
        for bad in ["", "pt", "deg", "IN_F", "px", "furlong", "ft in"] {
            assert!(length_unit(bad).is_none(), "{bad}");
        }
    }
}
