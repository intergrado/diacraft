//! The salted control: a document built here, with values and errors that are known to be wrong, scored by the same path as any file.
//!
//! A score of 100% proves nothing by itself. A calculator that hands back the saved values, a comparer that compares nothing, and a walk that
//! skips a sheet each give it (the narrow check: an echoing calculator scored `agreed=104, differed=0`, and the score could not say so).
//! So **every run of the harness first scores this document**, as bytes, through [`super::file::examine`], the function a file goes through:
//! the package is written by `diacraft-vsdx`, read by it, and scored by the calculator the run uses. It passes only if
//!
//! * every **salted** cell lands in `differed`: a `V` that is not what the formula gives (a number, a text, a colour), an `E` the formula does
//!   not give, a formula that fails with no `E` saved, the wrong `E`, an `E` beside a `V` of `Themed` or an empty one, an inherited cell, a
//!   cell in a section's row, and one on each kind of sheet and at each depth the walk visits; and
//! * every **honest** cell agrees (a calculator that differs from everything would pass the first test alone); and
//! * nothing was left out, not calculated or unreached, so that each cell of the control was compared.
//!
//! When it fails the run has no score: each report says so (`score` is null, `score_withheld` is `control_failed`), and the command ends with a
//! nonzero status after printing them.

use std::sync::Arc;

use diacraft_calc::{Calculator, Difference, RecalcReport, SheetPlace};
use diacraft_doc::{Cell, CellAddr, Document, Master, Owner, Page, RowKey, Shape, ShapeList, ShapeType, Sheet, Style, Value};
use diacraft_vsdx::Limits;
use serde_json::{Value as Json, json};

use super::file::{Read, examine};

/// One cell of the control.
struct Probe {
    /// A fixed word that names the cell in a report.
    id: &'static str,
    place: SheetPlace,
    addr: CellAddr,
    formula: Formula,
    /// `V` as the file would have it: `None` for a cell with no `V` at all.
    v: Option<&'static str>,
    unit: Option<&'static str>,
    error: Option<&'static str>,
    /// A salted cell is wrong in the file and must differ; an honest one is right and must agree.
    salted: bool,
}

#[derive(Clone, Copy)]
enum Formula {
    Text(&'static str),
    Inherit,
}

const PAGE: u32 = 1;
const MASTER: u32 = 1;
const STYLE: u32 = 1;
/// The group on the page, its member (depth 2), the instance of the master, and the master's shape.
const GROUP: u32 = 2;
const MEMBER: u32 = 3;
const INSTANCE: u32 = 4;
const MASTER_SHAPE: u32 = 5;

fn shape_place(owner: Owner, id: u32, depth: u32) -> SheetPlace {
    SheetPlace::Shape { owner, id: Some(id), depth }
}

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// The control's cells. Each salted one is wrong in a way a different part of the comparer must catch.
fn probes() -> Vec<Probe> {
    let page_shape = |id, depth| shape_place(Owner::Page(PAGE), id, depth);
    let on = |id: &'static str, place: SheetPlace, addr: CellAddr, formula: &'static str, v: Option<&'static str>, salted: bool| Probe {
        id,
        place,
        addr,
        formula: Formula::Text(formula),
        v,
        unit: None,
        error: None,
        salted,
    };
    let with = |mut p: Probe, unit: Option<&'static str>, error: Option<&'static str>| {
        p.unit = unit;
        p.error = error;
        p
    };
    vec![
        // One on each kind of sheet, each saved as 3 where the formula gives 2.
        on("s01_document_sheet", SheetPlace::Document, CellAddr::direct("DocLangID"), "1+1", Some("3"), true),
        on("s02_style_sheet", SheetPlace::Style(STYLE), CellAddr::direct("LineWeight"), "1+1", Some("3"), true),
        on("s03_page_sheet", SheetPlace::Page(PAGE), CellAddr::direct("PageWidth"), "1+1", Some("3"), true),
        on("s04_master_sheet", SheetPlace::Master(MASTER), CellAddr::direct("PageWidth"), "1+1", Some("3"), true),
        on("s05_master_shape", shape_place(Owner::Master(MASTER), MASTER_SHAPE, 1), CellAddr::direct("Width"), "1+1", Some("3"), true),
        // Shapes at each depth, and the kinds of value.
        on("s06_page_shape_number", page_shape(GROUP, 1), user("s06"), "2*3", Some("7"), true),
        with(on("s07_nested_shape_text", page_shape(MEMBER, 2), user("s07"), "\"ab\"&\"cd\"", Some("xxxx"), true), Some("STR"), None),
        on("s14_colour", page_shape(GROUP, 1), user("s14"), "RGB(1,2,3)", Some("#010204"), true),
        // A colour where the file saved a text: values of unlike kinds are never the same.
        with(on("s17_unlike_kinds", page_shape(GROUP, 1), user("s17"), "RGB(1,2,3)", Some("abc"), true), Some("STR"), None),
        on("s15_section_ix_and_row", page_shape(GROUP, 1), CellAddr::in_row("Geometry", Some(2), RowKey::Ix(3), "X"), "1+1", Some("3"), true),
        // The error: a formula that fails and saved none; one that works and saved one; two that are not the same; an error beside a `V` that
        // is no value to compare.
        on("s08_error_unexpected", page_shape(MEMBER, 2), user("s08"), "1/0", Some("1"), true),
        with(on("s09_error_missing", page_shape(MEMBER, 2), user("s09"), "1+1", Some("2"), true), None, Some("#N/A")),
        with(on("s10_error_differs", page_shape(MEMBER, 2), user("s10"), "1/0", Some("1"), true), None, Some("#N/A")),
        with(on("s11_error_beside_themed", page_shape(MEMBER, 2), user("s11"), "1+1", Some("Themed"), true), None, Some("#REF!")),
        with(on("s12_error_beside_empty", page_shape(MEMBER, 2), user("s12"), "1+1", Some(""), true), None, Some("#N/A")),
        // An instance whose inherited cell saved 9 where the master's formula gives 2.
        Probe {
            id: "s13_inherited",
            place: page_shape(INSTANCE, 1),
            addr: CellAddr::direct("Width"),
            formula: Formula::Inherit,
            v: Some("9"),
            unit: None,
            error: None,
            salted: true,
        },
        // An inherited cell that saved an error and no value of its own: the master's formula gives no error.
        Probe {
            id: "s16_inherited_error_without_value",
            place: page_shape(INSTANCE, 1),
            addr: CellAddr::direct("PinY"),
            formula: Formula::Inherit,
            v: None,
            unit: None,
            error: Some("#N/A"),
            salted: true,
        },
        // The honest cells: right in the file, and they must agree.
        on("h07_master_shape_pin", shape_place(Owner::Master(MASTER), MASTER_SHAPE, 1), CellAddr::direct("PinY"), "1+1", Some("2"), false),
        on("h01_page_sheet", SheetPlace::Page(PAGE), CellAddr::direct("PageHeight"), "1+1", Some("2"), false),
        on("h02_shape", page_shape(GROUP, 1), user("h02"), "1+1", Some("2"), false),
        with(on("h03_error", page_shape(MEMBER, 2), user("h03"), "1/0", Some("1"), false), None, Some("#DIV/0!")),
        with(on("h04_text", page_shape(MEMBER, 2), user("h04"), "\"ab\"", Some("ab"), false), Some("STR"), None),
        on("h05_master_shape", shape_place(Owner::Master(MASTER), MASTER_SHAPE, 1), CellAddr::direct("Height"), "1+1", Some("2"), false),
        Probe {
            id: "h06_inherited",
            place: page_shape(INSTANCE, 1),
            addr: CellAddr::direct("Height"),
            formula: Formula::Inherit,
            v: Some("2"),
            unit: None,
            error: None,
            salted: false,
        },
    ]
}

fn cell_of(p: &Probe) -> Cell {
    let value = p.v.map(Value::from_text);
    let mut cell = match p.formula {
        Formula::Text(text) => Cell::with_formula(text, value),
        Formula::Inherit => Cell::inherited(value),
    };
    if let Some(u) = p.unit {
        cell = cell.with_unit(u);
    }
    cell.error = p.error.map(Arc::from);
    cell
}

/// A sheet with the cells of the probes that sit on `place`.
fn sheet_at(probes: &[Probe], place: SheetPlace) -> Result<Sheet, String> {
    let mut sheet = Sheet::default();
    for p in probes.iter().filter(|p| p.place == place) {
        sheet.set_cell(&p.addr, cell_of(p)).map_err(|e| format!("the control cell {} cannot be set: {e}", p.id))?;
    }
    Ok(sheet)
}

/// The control document. A page with a group, a member of it and an instance of the master; the master with one shape; a style; the document's
/// sheet.
fn document(probes: &[Probe]) -> Result<Document, String> {
    let page_part = Owner::Page(PAGE);
    let mut member = Shape::new(MEMBER);
    member.sheet = Arc::new(sheet_at(probes, shape_place(page_part, MEMBER, 2))?);
    let mut group = Shape::new(GROUP);
    group.kind = ShapeType::Group;
    group.sheet = Arc::new(sheet_at(probes, shape_place(page_part, GROUP, 1))?);
    group.children = std::iter::once(Arc::new(member)).collect::<ShapeList>();
    let mut instance = Shape::new(INSTANCE);
    instance.master = Some(MASTER);
    instance.sheet = Arc::new(sheet_at(probes, shape_place(page_part, INSTANCE, 1))?);
    let mut page = Page::new(PAGE, "Control");
    page.sheet = Arc::new(sheet_at(probes, SheetPlace::Page(PAGE))?);
    page.shapes = [Arc::new(group), Arc::new(instance)].into_iter().collect::<ShapeList>();

    let mut master_shape = Shape::new(MASTER_SHAPE);
    master_shape.sheet = Arc::new(sheet_at(probes, shape_place(Owner::Master(MASTER), MASTER_SHAPE, 1))?);
    let master = Master {
        id: MASTER,
        name: Some(Arc::from("Control")),
        name_u: Some(Arc::from("Control")),
        sheet: Arc::new(sheet_at(probes, SheetPlace::Master(MASTER))?),
        shapes: std::iter::once(Arc::new(master_shape)).collect::<ShapeList>(),
        ..Master::default()
    };
    let style = Style {
        id: STYLE,
        name: Some(Arc::from("Control")),
        name_u: Some(Arc::from("Control")),
        sheet: Arc::new(sheet_at(probes, SheetPlace::Style(STYLE))?),
        ..Style::default()
    };

    let mut doc = Document::new_blank();
    doc.styles.push(Arc::new(style));
    doc.masters.push(Arc::new(master));
    doc.pages = std::iter::once(Arc::new(page)).collect();
    doc.sheet = Arc::new(sheet_at(probes, SheetPlace::Document)?);
    Ok(doc)
}

/// The control's package, as bytes: the same kind of input a file is.
pub(super) fn package() -> Result<Vec<u8>, String> {
    let probes = probes();
    let doc = document(&probes)?;
    diacraft_vsdx::write(&doc, None).map_err(|e| format!("the control cannot be written: {e}"))
}

/// What the control found on one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub salted: usize,
    /// Salted cells that landed in `differed`.
    pub caught: usize,
    pub honest: usize,
    /// Honest cells that agreed.
    pub honest_agreed: usize,
    /// The ids of the salted cells that did not differ.
    pub missed: Vec<&'static str>,
    /// The ids of the honest cells that did not agree.
    pub wrong: Vec<&'static str>,
    /// Cells of the control that were not compared at all (left out, not calculated or unreached), which a control cannot have.
    pub not_compared: u64,
    /// Why the control could not be run, when it could not.
    pub unbuilt: Option<String>,
}

impl Control {
    /// Whether the run's scores can be trusted: every salted cell differed, every honest one agreed, and each was compared.
    pub fn passed(&self) -> bool {
        self.unbuilt.is_none()
            && self.salted > 0
            && self.honest > 0
            && self.caught == self.salted
            && self.honest_agreed == self.honest
            && self.not_compared == 0
    }

    /// The control as a report carries it.
    pub fn json(&self) -> Json {
        json!({
            "passed": self.passed(),
            "salted": self.salted,
            "caught": self.caught,
            "honest": self.honest,
            "honest_agreed": self.honest_agreed,
            "missed": self.missed,
            "wrong": self.wrong,
            "not_compared": self.not_compared,
            "unbuilt": self.unbuilt,
        })
    }

    /// One line of words for a person.
    pub fn words(&self) -> String {
        if let Some(why) = &self.unbuilt {
            return format!("control FAILED: {why}");
        }
        let mut line = format!(
            "control {}: {} of {} salted cells differed, {} of {} honest cells agreed",
            if self.passed() { "passed" } else { "FAILED" },
            self.caught,
            self.salted,
            self.honest_agreed,
            self.honest
        );
        if self.not_compared > 0 {
            line.push_str(&format!(", {} not compared", self.not_compared));
        }
        if !self.missed.is_empty() {
            line.push_str(&format!("; did not differ: {}", self.missed.join(", ")));
        }
        if !self.wrong.is_empty() {
            line.push_str(&format!("; did not agree: {}", self.wrong.join(", ")));
        }
        line
    }
}

/// Whether `d` is the difference of probe `p`.
fn is_difference_of(d: &Difference, p: &Probe) -> bool {
    d.place == p.place && d.address == p.addr
}

/// Scores the control with `calc` and checks it.
pub fn run(calc: &mut dyn Calculator) -> Control {
    let probes = probes();
    let salted = probes.iter().filter(|p| p.salted).count();
    let honest = probes.len() - salted;
    let unbuilt = |why: String| Control {
        salted,
        caught: 0,
        honest,
        honest_agreed: 0,
        missed: Vec::new(),
        wrong: Vec::new(),
        not_compared: 0,
        unbuilt: Some(why),
    };
    let bytes = match package() {
        Ok(bytes) => bytes,
        Err(why) => return unbuilt(why),
    };
    let examined = examine(&bytes, &Limits::default(), calc);
    let read = match examined.outcome {
        Ok(read) => read,
        Err(e) => return unbuilt(format!("the control does not read: {}", e.class())),
    };
    check(&probes, &read)
}

fn check(probes: &[Probe], read: &Read) -> Control {
    let r: &RecalcReport = &read.recalc;
    let differs = |p: &Probe| r.differed.iter().any(|d| is_difference_of(d, p));
    let missed: Vec<&'static str> = probes.iter().filter(|p| p.salted && !differs(p)).map(|p| p.id).collect();
    let wrong: Vec<&'static str> = probes.iter().filter(|p| !p.salted && differs(p)).map(|p| p.id).collect();
    let salted = probes.iter().filter(|p| p.salted).count();
    let honest = probes.len() - salted;
    // An honest cell agrees if it was compared and is not among the differences: no more can have agreed than the run counted as agreed.
    let honest_agreed = usize::try_from(r.agreed).unwrap_or(usize::MAX).min(honest.saturating_sub(wrong.len()));
    // The control holds as many formula cells as it has probes; any that is not scored was left out, not calculated, or not reached.
    let not_compared = (probes.len() as u64).saturating_sub(r.scored());
    Control { salted, caught: salted - missed.len(), honest, honest_agreed, missed, wrong, not_compared, unbuilt: None }
}
