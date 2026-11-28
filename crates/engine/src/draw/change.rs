//! Change Shape: shapes swapped for another master, keeping what they are.
//!
//! Change Shape: the selected shapes are replaced
//! by the one picked and keep their position, connections, formatting, text and shape data; a two-dimensional shape is swapped only for a two-dimensional
//! master and a one-dimensional one only for a one-dimensional master. The ShapeSheet index gives the cells that bear on it: `LockReplace` on a shape (and
//! on the master) keeps it out of a swap, `PageLockReplace` and `DocLockReplace` turn swapping off on a page and in a document, and `ReplaceLockFormat`,
//! `ReplaceLockShapeData` and `ReplaceLockText` of the new master say whether its formatting, data values and text beat the shape's own. What the sources
//! leave open is decided here, and each decision is a rule of ours that `docs/editing-commands.md` lists:
//!
//! * A shape that cannot be swapped is **skipped**, with its reason in words, and the others are changed: the shapes of a mixed selection
//!   that fit the master change, as they do when the pointer picks one. A guide, a picture, a member that stands for a shape of its group's master, a group
//!   that holds shapes of the person's own, a shape locked by `LockReplace`, a shape of the other dimension and a shape with more than [`MAX_DATA_ROWS`]
//!   data rows are skipped. A selection with nothing to change is refused with the reasons. A shape that is an instance of the master already is
//!   **unchanged**, and when nothing changed the command is no undo step.
//! * The shape keeps its `ID` (so every connector glued to it, every formula that names it and the layers and the order of the page keep
//!   it), its place in the drawing order, its `Name` when the person gave it one (an automatic name `Process.5` becomes `Decision.5`), its text and the
//!   sections of its text, its hyperlinks, actions and user cells, and **every cell it holds of its own** (a cell with `F="Inh"` is
//!   no cell of its own, so what the old master supplied does not follow). The `Geometry` and `Connection` sections of a two-dimensional shape are the new
//!   master's: its outline and its connection points. A connector keeps its own route.
//! * The frame stays: the shape's `Width`, `Height`, `Angle` and flips are written as cells of its own when the old master supplied them
//!   and the new one would give another result, and the centre of the box is put back where it was when the new master's pin is elsewhere in the box.
//!   A connector keeps its `BeginX`, `BeginY`, `EndX` and `EndY`, and gets the cells that put its frame between them (`connectors::frame_cells`) when it
//!   inherited them, so that they do not depend on the master.
//! * The shape data rows the shape had, local or inherited, are given to it as rows of its own, **cell by cell and only where the new
//!   master would not give the same**; a row the new master has and the shape does not is the master's. With `ReplaceLockShapeData` on the new master, a row
//!   that it has too is the master's and the shape's own values for it are not kept.
//! * With `ReplaceLockFormat` on the new master the shape's own line, fill and text cells, its gradients and its style
//!   links are not kept; with `ReplaceLockText` and a text of its own on the new master, the shape's own text is not.
//! * The cells the new master's `ReplaceCopyCells` names (a `DEPENDSON` call) are carried from the shape to
//!   the result, as a value or a formula, and beat the locks and the shape data above; where the new master has no such cell only the value is copied (`change_copy.rs`).
//! * A connector end glued to a connection point the new master has stays glued to it, and the recalculation moves the end. An end glued
//!   to a point the new master lacks, or to a member that is gone, is glued to the nearest point of the shape, and **let go where it is**
//!   when the shape has no point at all. Both are said in the answer. An end glued to the whole of the shape stays.
//!
//! **How a two-dimensional shape is made.** A new instance of the master is dropped as `shape.drop` drops it (so that the members of a group master, the
//! pin and the name come out as a drop's do, whatever the stencil), then taken off the page and its `master`, members and `Type` are put on the old shape,
//! which stays where it is in its list. A one-dimensional shape needs no such instance: it keeps its cells and takes the master.
//!
//! **What it costs.** At most [`MAX_CHANGED_SHAPES`] shapes, each one drop and one merge of its own sheet, [`MAX_DATA_ROWS`] data rows of one shape, and
//! [`MAX_RECONNECTED_ENDS`] connector ends moved; every list is bounded before it is made, and a refusal leaves the document as it was (the caller edits a copy).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::Changes;
use diacraft_doc::{
    Cell, CellAddr, CellName, Document, Links, MAX_CELLS_PER_CONTAINER, MAX_SECTIONS_PER_SHEET, MAX_SHAPE_DEPTH, Origin, Owner, Passthrough,
    Resolved, Resolver, Row, RowKey, Section, SectionKey, Shape, ShapeRef, ShapeType, Sheet, SheetRef, Source, Value, owner_places_after_where,
};

use super::connectors::{Which, frame_cells, reconnect_unsettled, release_end};
use super::local::{below, charge_all, localise_geometry};
use super::read::{Pose, Reader};
use super::route::P;
use super::scan::{Named, distinct};
use super::settle::settle;
use super::{End, Problem, page_mut, require_page, shape_mut, write_length};
use crate::stencil::{self, Brought, MasterEntry, MasterSpec, OpenedStencil};

/// The most shapes one change takes (each is a drop and a merge of its own).
pub const MAX_CHANGED_SHAPES: usize = 512;

/// The most shape data rows of one shape that are carried over; a shape with more is skipped.
pub const MAX_DATA_ROWS: usize = 1_024;

/// The most connector ends one change glues again.
pub const MAX_RECONNECTED_ENDS: usize = 4_096;

/// The most members the new instances of one change list, in all: as many as one copy holds (`MAX_UNGROUP_MEMBERS`). A master lists up to 5,000 members for an
/// instance (`MAX_LISTED_MEMBERS`) and a change takes up to [`MAX_CHANGED_SHAPES`] shapes, so the members are counted as they are made.
pub const MAX_NEW_MEMBERS: usize = 50_000;

/// `ToPart` of a `Connect` row at a connection point is this plus the point's `IX`.
const TO_PART_BASE: u32 = 100;

/// Two numbers closer than this are the same place or size.
const EPS: f64 = 1.0e-9;

/// The master the shapes are changed to.
pub enum Replacement<'a> {
    /// A master of a built-in stencil.
    Builtin(&'static MasterSpec),
    /// A master of a stencil opened from a file.
    Opened { stencil: &'a OpenedStencil, entry: &'a MasterEntry },
}

/// A shape that was left as it is, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub id: u32,
    pub why: String,
}

/// What became of one end of a connector that was glued to a shape that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glued {
    /// Glued to this connection point of the shape now.
    Point(u32),
    /// Let go: the shape has no connection point. The end stays where it was.
    Free,
    /// Left as it was: the glue could not be made again.
    Left,
}

/// A connector end that had to be glued again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reglued {
    pub connector: u32,
    pub begin: bool,
    pub shape: u32,
    pub now: Glued,
}

/// What a change did.
#[derive(Debug, Clone, PartialEq)]
pub struct Changed {
    /// The `ID` of the master in the document.
    pub master: u32,
    pub name: String,
    /// What bringing the master into the document did, for a master of an opened stencil.
    pub brought: Option<Brought>,
    pub changed: Vec<u32>,
    pub unchanged: Vec<u32>,
    pub skipped: Vec<Skipped>,
    pub reglued: Vec<Reglued>,
    /// The highest `ID` the change gave a shape (a member of a group master): the session's mark.
    pub highest_id: u32,
}

/// What the new master says about a swap.
#[derive(Clone, Copy, Default)]
struct Locks {
    format: bool,
    data: bool,
    text: bool,
}

/// What the change needs to know of the new master.
struct NewMaster {
    id: u32,
    name: String,
    one_d: bool,
    has_text: bool,
    locks: Locks,
    replace_locked: bool,
    /// The master's one shape, and the cells its `ReplaceCopyCells` names (`change_copy.rs`). A master of several shapes has neither.
    sheet: Option<SheetRef>,
    copy: Vec<diacraft_formula::CellRef>,
    /// The style links of the master's one shape, which an instance with none of its own follows ([`own_links`]). A master of several shapes has none to give.
    links: Option<Links>,
}

fn top_sheet(mid: u32, at: usize) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Master(mid), u32::try_from(at).unwrap_or(u32::MAX)))
}

/// Brings the master into the document, as a drop does.
fn bring(doc: &mut Document, to: &Replacement<'_>) -> Result<(u32, Option<Brought>), Problem> {
    match to {
        Replacement::Builtin(spec) => Ok((stencil::ensure_master(doc, spec)?, None)),
        Replacement::Opened { stencil, entry } => {
            let named = crate::text::shown(&entry.name, 64);
            if entry.top_shapes == 0 {
                return Err(Problem::failed(format!("master `{named}` holds no shapes, so no shape can be changed to it")));
            }
            let brought = stencil::bring_master(doc, stencil.document(), entry.position, stencil.identity(entry))
                .map_err(|r| Problem::failed(format!("master `{named}` cannot be used: {r}")))?;
            Ok((brought.id, Some(brought)))
        }
    }
}

/// What the document's own master says: its dimension, its locks, whether it has text. A master of several top-level shapes is a group, so two-dimensional, and
/// says nothing of the locks (there is no one shape to say them).
fn master_facts(doc: &Document, mid: u32) -> Result<NewMaster, Problem> {
    let master = doc.master(mid).ok_or_else(|| Problem::failed("the master went away"))?;
    let live: Vec<usize> = master.shapes.iter().enumerate().filter(|(_, s)| !s.del).map(|(at, _)| at).collect();
    let name = master.name.as_deref().or(master.name_u.as_deref()).unwrap_or("Shape").to_owned();
    let reader = Reader::new(doc);
    match live.as_slice() {
        [] => Err(Problem::failed(format!("master `{}` holds no shapes, so no shape can be changed to it", crate::text::shown(&name, 64)))),
        [only] => {
            let sheet = top_sheet(mid, *only);
            let shape = master.shapes.get(*only).ok_or_else(|| Problem::failed("the master went away"))?;
            let flag = |cell: &str| reader.number(&sheet, cell) != 0.0;
            Ok(NewMaster {
                id: mid,
                name,
                one_d: reader.is_one_d(&sheet),
                has_text: shape.text.as_ref().is_some_and(|t| !t.parts.is_empty()),
                locks: Locks { format: flag("ReplaceLockFormat"), data: flag("ReplaceLockShapeData"), text: flag("ReplaceLockText") },
                replace_locked: flag("LockReplace"),
                copy: super::change_copy::list_of(doc, &sheet),
                sheet: Some(sheet),
                links: Some(shape.sheet.links),
            })
        }
        _ => Ok(NewMaster {
            id: mid,
            name,
            one_d: false,
            has_text: false,
            locks: Locks::default(),
            replace_locked: false,
            sheet: None,
            copy: Vec::new(),
            links: None,
        }),
    }
}

/// The style links a changed shape keeps as its own: those of `links` that name another style than the new master's one shape links for the family, no link counting as
/// the root's, which is what a shape with none reads. A link that is the master's says nothing the master does not (DiaCraft follows the master shape's link when the instance has none,
/// step 3), and the VSDX application reads an instance's own link before the master's cells of that family, so it would hide the master's line ends, pattern and fill
/// there: the connector tool's links to the root, on a connector changed to a connector master, are dropped. `master` is `None` for a master of several shapes: every link is kept.
fn own_links(links: Links, master: Option<Links>) -> Links {
    let Some(master) = master else { return links };
    let own = |mine: Option<u32>, masters: Option<u32>| mine.filter(|id| *id != masters.unwrap_or(0));
    Links { line: own(links.line, master.line), fill: own(links.fill, master.fill), text: own(links.text, master.text) }
}

/// The locks that turn swapping off for the document and for the page (`DocLockReplace`, `PageLockReplace`).
fn check_locks(doc: &Document, page: u32) -> Result<(), Problem> {
    if let Some(why) = swapping_locked(doc) {
        return Err(Problem::failed(why));
    }
    let reader = Reader::new(doc);
    if reader.number(&SheetRef::Page(page), "PageLockReplace") != 0.0 {
        return Err(Problem::failed(format!("shapes cannot be swapped on page {page}: its PageLockReplace is on")));
    }
    Ok(())
}

/// Why swapping is turned off in the whole document, if it is (`DocLockReplace`): for the command's `enabled`.
pub fn swapping_locked(doc: &Document) -> Option<&'static str> {
    (Reader::new(doc).number(&SheetRef::Document, "DocLockReplace") != 0.0)
        .then_some("shapes cannot be swapped in this document: its DocLockReplace is on")
}

/// What a shape is to be done with.
enum Verdict {
    Change,
    Unchanged,
    Skip(String),
}

fn kind_word(one_d: bool) -> &'static str {
    if one_d { "one-dimensional" } else { "two-dimensional" }
}

fn classify(doc: &Document, at: &ShapeRef, id: u32, new: &NewMaster) -> Verdict {
    let Some(shape) = doc.shape(at) else { return Verdict::Skip(format!("shape {id} went away")) };
    match shape.kind {
        ShapeType::Guide => return Verdict::Skip(format!("shape {id} is a guide, which has no master to change")),
        ShapeType::Foreign => return Verdict::Skip(format!("shape {id} is a picture or an object, which cannot be changed to a master")),
        _ => {}
    }
    if shape.master_shape.is_some() {
        return Verdict::Skip(format!("shape {id} is a part of an instance of a group master: change the whole shape, or ungroup it first"));
    }
    if shape.master == Some(new.id) {
        return Verdict::Unchanged;
    }
    if shape.children.iter().any(|c| c.master_shape.is_none()) {
        return Verdict::Skip(format!(
            "shape {id} is a group of shapes of your own, which a master would replace: change its members, or ungroup it first"
        ));
    }
    let reader = Reader::new(doc);
    let sheet = SheetRef::Shape(at.clone());
    if reader.number(&sheet, "LockReplace") != 0.0 {
        return Verdict::Skip(format!("shape {id} is locked against swapping (LockReplace)"));
    }
    let one_d = reader.is_one_d(&sheet);
    if one_d != new.one_d {
        return Verdict::Skip(format!(
            "shape {id} is {} and master `{}` is {}: a shape is changed only to a master of its own dimension",
            kind_word(one_d),
            crate::text::shown(&new.name, 64),
            kind_word(new.one_d)
        ));
    }
    let rows = reader.resolver().rows_held(&sheet, &SectionKey::new("Property", None)).unwrap_or(0);
    if rows > MAX_DATA_ROWS {
        return Verdict::Skip(format!("shape {id} holds {rows} data rows, more than the {MAX_DATA_ROWS} a change carries over"));
    }
    Verdict::Change
}

// ---------------------------------------------------------------------------------------------------------------------------- what the old shape is

/// A shape data row as the shape has it, to be kept.
struct OwnedRow {
    key: RowKey,
    row_type: Option<Arc<str>>,
    cells: Vec<(CellName, Cell)>,
}

/// The cells of the old shape that the swap has to keep, read before anything is changed.
struct Old {
    shape: Arc<Shape>,
    one_d: bool,
    /// The centre of the box in the frame of the shape's parent (a two-dimensional shape).
    centre: P,
    /// Frame cells the shape does not hold itself, as the numbers they have now.
    frozen: Vec<(&'static str, Cell)>,
    data: Vec<OwnedRow>,
    auto_name: bool,
    /// The `ID`s of everything below the shape: gone once the shape has the new master's members.
    removed: Vec<u32>,
    nested: bool,
}

/// A cell as a number the shape has now, held as a constant with its unit.
fn constant_of(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> Option<Cell> {
    let found = res.resolve(sheet, &CellAddr::direct(name))?;
    let value = found.value.cell.and_then(|c| c.value.clone())?;
    let unit = found.value.cell.and_then(|c| c.unit.clone()).or_else(|| found.formula.cell.and_then(|c| c.unit.clone()));
    Some(Cell { value: Some(value), unit, ..Cell::default() })
}

// How many sections the merge of two sheets looked at, for the tests that hold it to a count: one for each section it takes or looks up. Not kept outside tests.
#[cfg(test)]
thread_local! {
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_step() {
    #[cfg(test)]
    STEPS.with(|c| c.set(c.get().saturating_add(1)));
}

fn is_local(sheet: &Sheet, name: &str) -> bool {
    sheet.cell(name).is_some_and(|c| c.origin() == Origin::Local)
}

fn number_of(cell: &Cell) -> Option<f64> {
    cell.value.as_ref().and_then(Value::number).filter(|n| n.is_finite())
}

/// A data row cell as the shape is to hold it: the shape's own cell as it is, any other as the constant it has now.
pub(super) fn kept_cell(found: &Resolved<'_>) -> Option<Cell> {
    if found.formula.source == Source::Local
        && let Some(cell) = found.formula.cell
    {
        return Some(cell.clone());
    }
    let value = found.value.cell.and_then(|c| c.value.clone())?;
    let unit = found.value.cell.and_then(|c| c.unit.clone()).or_else(|| found.formula.cell.and_then(|c| c.unit.clone()));
    Some(Cell { value: Some(value), unit, ..Cell::default() })
}

fn gather(doc: &Document, at: &ShapeRef, id: u32) -> Result<Old, Problem> {
    let shape = doc.shape(at).cloned().ok_or_else(|| Problem::failed("the shape went away"))?;
    let reader = Reader::new(doc);
    let res = reader.resolver();
    let sheet = SheetRef::Shape(at.clone());
    let one_d = reader.is_one_d(&sheet);
    let pose: Pose = reader.pose(&sheet);
    let (w, h) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let centre = pose.up((w / 2.0, h / 2.0));
    let mut frozen: Vec<(&'static str, Cell)> = Vec::new();
    let names: &[&'static str] =
        if one_d { &["BeginX", "BeginY", "EndX", "EndY"] } else { &["PinX", "PinY", "Width", "Height", "Angle", "FlipX", "FlipY"] };
    for name in names.iter().copied() {
        if !is_local(&shape.sheet, name)
            && let Some(cell) = constant_of(res, &sheet, name)
        {
            frozen.push((name, cell));
        }
    }
    if one_d {
        // The cells that put a connector's frame between its ends, for each one it inherited (the new master may hold other formulas for it).
        let end = |name: &str| reader.number(&sheet, name);
        for (name, cell) in frame_cells((end("BeginX"), end("BeginY")), (end("EndX"), end("EndY"))) {
            if !is_local(&shape.sheet, name) {
                frozen.push((name, cell));
            }
        }
    }
    let mut data = Vec::new();
    if let Some(Ok(rows)) = res.effective_rows_within(&sheet, &SectionKey::new("Property", None), MAX_DATA_ROWS) {
        for row in rows {
            let cells: Vec<(CellName, Cell)> = row.cells.iter().filter_map(|(name, found)| kept_cell(found).map(|c| (name.clone(), c))).collect();
            data.push(OwnedRow { key: row.key, row_type: row.row_type, cells });
        }
    }
    let auto_name = shape.name.as_deref().is_some_and(|n| n.ends_with(&format!(".{id}"))) && shape.raw.get("IsCustomName") != Some("1");
    let removed: Vec<u32> = below(doc, at).iter().filter_map(|r| doc.shape(r).map(|s| s.id)).collect();
    Ok(Old { shape, one_d, centre, frozen, data, auto_name, removed, nested: at.path.len() > 1 })
}

// ---------------------------------------------------------------------------------------------------------------------------- the merge

/// Whether a cell directly under a shape is formatting of the line, fill or text (the cells a style family reaches).
fn is_format(name: &str) -> bool {
    diacraft_doc::cell_info(None, name).class.family().is_some()
}

/// A name for an instance from its master's: no control characters, and no more than a name may be.
fn instance_name(master: &str, id: u32) -> Arc<str> {
    let clean: String = master.chars().filter(|c| !c.is_control()).take(200).collect();
    let clean = if clean.trim().is_empty() { "Shape".to_owned() } else { clean };
    Arc::from(format!("{clean}.{id}"))
}

/// A cell put in the merged sheet: refused in words when the sheet would hold more cells than a sheet may, which stops the copy at the cap and not after it.
fn put_cell(cells: &mut BTreeMap<CellName, Cell>, name: CellName, cell: Cell) -> Result<(), Problem> {
    cells.insert(name, cell);
    if cells.len() > MAX_CELLS_PER_CONTAINER {
        return Err(Problem::failed(format!(
            "the changed shape would hold more than {MAX_CELLS_PER_CONTAINER} cells of its own, the most a sheet holds: use a master with fewer, or take cells off the shape first"
        )));
    }
    Ok(())
}

/// The sheet the changed shape holds: the new instance's cells (a two-dimensional shape), the old shape's own cells over them, and the frame cells the
/// old master had supplied, with the sections that stay. **Bounded before it is copied**: a sheet that already holds more than a
/// sheet may is refused at once, the cells are copied up to the most a sheet holds and no further, the sections that stay and the new instance's are found by a set
/// of their keys (one look for each, not a search of the list for each), and the result is checked against every limit of a sheet (`Sheet::check_limits`).
fn merged_sheet(
    old: &Old,
    base: Option<&Sheet>,
    new_numbers: &HashMap<&'static str, f64>,
    locks: Locks,
    keeps: (bool, bool),
) -> Result<Sheet, Problem> {
    let held = &old.shape.sheet;
    let too_many = |what: &str, n: usize, most: usize| {
        Problem::failed(format!("a sheet of the shape or of the new master holds {n} {what}, more than the {most} a sheet may hold"))
    };
    for (what, n, most) in [
        ("cells", held.cells.len(), MAX_CELLS_PER_CONTAINER),
        ("cells", base.map_or(0, |b| b.cells.len()), MAX_CELLS_PER_CONTAINER),
        ("sections", held.sections.len(), MAX_SECTIONS_PER_SHEET),
        ("sections", base.map_or(0, |b| b.sections.len()), MAX_SECTIONS_PER_SHEET),
    ] {
        if n > most {
            return Err(too_many(what, n, most));
        }
    }
    let mut cells: BTreeMap<CellName, Cell> = BTreeMap::new();
    if let Some(b) = base {
        for (name, cell) in &b.cells {
            put_cell(&mut cells, name.clone(), cell.clone())?;
        }
    }
    for (name, cell) in &held.cells {
        if cell.origin() != Origin::Local || (locks.format && is_format(name.as_str())) {
            continue;
        }
        put_cell(&mut cells, name.clone(), cell.clone())?;
    }
    for (name, cell) in &old.frozen {
        // A size the new master keeps as a formula under its lock (a group of compartments is as tall as its text) is the master's, whatever the old shape's was.
        if (*name == "Width" && keeps.0) || (*name == "Height" && keeps.1) {
            continue;
        }
        // A frame cell the old master supplied is written when the new master would give another result; the pin and the ends of a line always.
        let always = old.one_d || matches!(*name, "PinX" | "PinY");
        let differs = number_of(cell).zip(new_numbers.get(name)).is_none_or(|(was, now)| (was - now).abs() > EPS);
        if always || differs {
            put_cell(&mut cells, CellName::new(name), cell.clone())?;
        }
    }
    // The pin of a group master's instance is a number at the middle of its box; it follows the size the shape was given.
    for (pin, size, of) in [("LocPinX", "Width", "Width*0.5"), ("LocPinY", "Height", "Height*0.5")] {
        if let Some(b) = base
            && !is_local(held, pin)
            && b.cell(pin).is_some_and(|c| !c.owns_formula())
            && let Some(length) = cells.get(size).and_then(number_of)
        {
            let mut half = Cell::with_formula(of, Some(Value::from_text(&stencil::number_text(length / 2.0))));
            half.unit = Some(Arc::from("IN"));
            put_cell(&mut cells, CellName::new(pin), half)?;
        }
    }
    let skip = |section: &Section| replaced(old, locks, section);
    let mut sections: Vec<Arc<Section>> = Vec::new();
    let mut seen: HashSet<(&str, Option<u32>)> = HashSet::new();
    for section in held.sections.iter().inspect(|_| note_step()).filter(|s| !skip(s)) {
        seen.insert((section.name.as_str(), section.ix));
        // The points a two-dimensional shape took from the new master are originals, so the marker of the points a doubling made (a row of its own `User`
        // section) goes with the points it named; the rest of the section stays.
        match marker_free(section, old.one_d) {
            Some(Some(without)) => sections.push(Arc::new(without)),
            Some(None) => {}
            None => sections.push(Arc::clone(section)),
        }
    }
    if let Some(b) = base {
        for section in b.sections.iter().inspect(|_| note_step()) {
            if seen.insert((section.name.as_str(), section.ix)) {
                if sections.len() >= MAX_SECTIONS_PER_SHEET {
                    return Err(Problem::failed(format!(
                        "the changed shape would hold more than {MAX_SECTIONS_PER_SHEET} sections of its own, the most a sheet holds: use a master with fewer, or take sections off the shape first"
                    )));
                }
                sections.push(Arc::clone(section));
            }
        }
    }
    let links = if locks.format {
        base.map(|b| b.links).unwrap_or_default()
    } else {
        let from = base.map(|b| b.links).unwrap_or_default();
        diacraft_doc::Links { line: held.links.line.or(from.line), fill: held.links.fill.or(from.fill), text: held.links.text.or(from.text) }
    };
    let mut sheet = Sheet { links, cells, sections, raw: held.raw.clone(), passthrough: Passthrough::new(), ext: held.ext.clone() };
    // The sheet is made again from the shape's: each unknown node of the sheet stays after the cell or the section it followed (the sections the new master gives are new, whatever the
    // old ones were called).
    sheet.take_places_of_where(held, |section| !replaced(old, locks, section));
    sheet.check_limits().map_err(|e| Problem::failed(format!("the changed shape would break a limit of a sheet: {e}")))?;
    Ok(sheet)
}

/// The `User` section of a two-dimensional shape without the marker of the points a doubling made: `None` when the section holds no such row (or is
/// not a `User` section, or the shape is one-dimensional and keeps its points), `Some(None)` when nothing else is in it and it goes, else the section without the row.
fn marker_free(section: &Section, one_d: bool) -> Option<Option<Section>> {
    let is_marker = |row: &diacraft_doc::Row| row.name.as_deref() == Some(super::conn_marker::MARKER_ROW);
    if one_d || section.name.as_str() != "User" || !section.rows.iter().any(|row| is_marker(row)) {
        return None;
    }
    let mut without = section.clone();
    without.retain_rows(|row| !is_marker(row));
    let bare = without.rows.is_empty()
        && without.cells.is_empty()
        && !without.del
        && without.raw.is_empty()
        && without.passthrough.is_empty()
        && without.ext.is_empty();
    Some(if bare { None } else { Some(without) })
}

/// Whether a section of the old shape goes with the change (the new master's takes its place): a two-dimensional shape's outline and points are the new master's, its shape data is carried
/// over row by row, and the gradients are formatting.
fn replaced(old: &Old, locks: Locks, section: &Section) -> bool {
    let name = section.name.as_str();
    (!old.one_d && matches!(name, "Geometry" | "Connection"))
        || name == "Property"
        || (locks.format && matches!(name, "FillGradient" | "LineGradient"))
}

/// The shape as it is once it is changed: the old shape, with the new master, members and `Type`, and the merged sheet.
fn merged_shape(
    old: &Old,
    id: u32,
    new: &NewMaster,
    dropped: Option<&Shape>,
    (new_numbers, keeps): (&HashMap<&'static str, f64>, (bool, bool)),
) -> Result<Shape, Problem> {
    let mut sheet = merged_sheet(old, dropped.map(|d| &*d.sheet), new_numbers, new.locks, keeps)?;
    sheet.links = own_links(sheet.links, new.links);
    let mut shape = (*old.shape).clone();
    shape.master = Some(new.id);
    shape.master_shape = None;
    shape.del = false;
    if let Some(d) = dropped {
        shape.kind = d.kind.clone();
        // The members are the new master's: the unknown nodes among the old ones stay after the member of the same `ID`, and go with the member before it when there is none.
        shape.edit_members(|members| *members = d.children.clone());
    }
    // The nodes of the shape count the cells and the sections of its sheet, which is made again.
    shape.passthrough = owner_places_after_where(&old.shape.passthrough, &old.shape.sheet, &sheet, |section| !replaced(old, new.locks, section));
    shape.sheet = Arc::new(sheet);
    if old.auto_name {
        let name = instance_name(&new.name, id);
        shape.name = Some(Arc::clone(&name));
        shape.name_u = Some(name);
    }
    if new.locks.text && new.has_text {
        shape.text = None;
    }
    Ok(shape)
}

/// The numbers a cell of the new instance has, by name, for the frame cells that are compared.
fn numbers_of(doc: &Document, at: &ShapeRef) -> HashMap<&'static str, f64> {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Shape(at.clone());
    ["Width", "Height", "Angle", "FlipX", "FlipY"].into_iter().map(|name| (name, reader.number(&sheet, name))).collect()
}

/// A new instance of the master, taken off the page again.
struct Dropped {
    shape: Arc<Shape>,
    /// What its frame cells are, by name.
    numbers: HashMap<&'static str, f64>,
    /// The highest `ID` the drop gave (the instance's, or its last member's).
    highest: u32,
    /// Which sizes the new master keeps as a formula under a lock (width, height: `shapes::keeps`).
    keeps: (bool, bool),
}

/// Drops an instance of the master on the page, as `shape.drop` does, and takes it off the page again.
fn drop_instance(doc: &mut Document, page: u32, to: &Replacement<'_>, above: u32, takes_the_id_of: u32) -> Result<Dropped, Problem> {
    let (id, highest) = match to {
        Replacement::Builtin(spec) => {
            // The trial instance is no drop on the page: the shape it stands in for keeps the size it has, so the page's drawing scale does not scale it (`drop_scale.rs`).
            let info = super::drop_shape_following(doc, page, spec, (0.0, 0.0), above, super::Follow::Never)?;
            // The members of a group master's instance have `ID`s above its own: the highest on the page is the last of them.
            let highest = super::page_stats(doc, page).map_or(info.id, |(highest, _)| highest.max(info.id));
            (info.id, highest)
        }
        Replacement::Opened { stencil, entry } => {
            let done = stencil::drop_opened_following(doc, page, stencil, entry, (0.0, 0.0), above, super::Follow::Never)?;
            (done.info.id, done.highest_id)
        }
    };
    // A drop puts the new shape at the end of the page's own list, and its `ID` is above every other on the page, so it is the one shape with that `ID`: it is looked for
    // from the end of that list, where a search of the page from its top walked the whole page for each shape changed.
    let list = &doc.page(page).ok_or_else(|| Problem::failed("the page went away"))?.shapes;
    let place = list.iter().rposition(|s| s.id == id).ok_or_else(|| Problem::failed("the new shape went away"))?;
    let at = ShapeRef::top(Owner::Page(page), u32::try_from(place).map_err(|_| Problem::failed("a page holds too many shapes"))?);
    let numbers = numbers_of(doc, &at);
    let keeps = super::shapes::kept_sizes(&Reader::new(doc), &SheetRef::Shape(at.clone()));
    let list = &mut page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes;
    let shape = list.remove(place).ok_or_else(|| Problem::failed("the new shape went away"))?;
    // The members' formulas name the new instance by its `ID` (a member's pin and size are formulas of the group's: `tie_to_group`); the shape that takes them keeps its own.
    let mut shape = Arc::unwrap_or_clone(shape);
    rehome(&mut shape, &HashMap::from([(id, takes_the_id_of)]), 0);
    Ok(Dropped { shape: Arc::new(shape), numbers, highest, keeps })
}

/// How many shapes are below `shape`, counted no further than `cap + 1`: iterative, and no deeper than the model allows.
fn members_below(shape: &Shape, cap: usize) -> usize {
    let mut count = 0_usize;
    let mut work: Vec<(&diacraft_doc::ShapeList, usize)> = vec![(&shape.children, 1)];
    while let Some((list, depth)) = work.pop() {
        count = count.saturating_add(list.len());
        if count > cap {
            return count;
        }
        if depth < MAX_SHAPE_DEPTH {
            work.extend(list.iter().filter(|c| !c.children.is_empty()).map(|c| (&c.children, depth + 1)));
        }
    }
    count
}

/// A shape and everything below it with the `Sheet.<id>!` references in their formulas rewritten by `map`, as deep as the model allows and no deeper.
fn rehome(shape: &mut Shape, map: &HashMap<u32, u32>, depth: usize) {
    shape.sheet = super::remap::remap_sheet(&shape.sheet, map, false);
    if depth >= MAX_SHAPE_DEPTH {
        return;
    }
    for at in 0..shape.children.len() {
        if let Some(child) = shape.children.get_mut(at) {
            rehome(Arc::make_mut(child), map, depth + 1);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- shape data

/// Gives the shape the data rows it had, where the new master does not give the same.
fn write_data(doc: &mut Document, page: u32, at: &ShapeRef, rows: &[OwnedRow], locked: bool) -> Result<(), Problem> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut section = Section::new("Property", None);
    {
        let res = Resolver::new(doc);
        let sheet = SheetRef::Shape(at.clone());
        let key = SectionKey::new("Property", None);
        let held = res.effective_rows_within(&sheet, &key, MAX_DATA_ROWS).and_then(Result::ok).unwrap_or_default();
        for row in rows {
            let present = held.iter().find(|h| h.key == row.key);
            if locked && present.is_some() {
                continue;
            }
            let mut cells: BTreeMap<CellName, Cell> = BTreeMap::new();
            for (name, cell) in &row.cells {
                let same = present.is_some_and(|p| {
                    cell.formula == diacraft_doc::FormulaSlot::Absent
                        && p.cells.iter().any(|(n, found)| {
                            n == name
                                && found.value.cell.is_some_and(|c| {
                                    c.value.as_ref().map(Value::text) == cell.value.as_ref().map(Value::text)
                                        && c.unit.as_deref() == cell.unit.as_deref()
                                })
                        })
                });
                if !same {
                    cells.insert(name.clone(), cell.clone());
                }
            }
            if cells.is_empty() && present.is_some() {
                continue;
            }
            let mut made = Row { cells, ..Row::default() };
            match &row.key {
                RowKey::Name(n) => made.name = Some(Arc::clone(n)),
                RowKey::Ix(ix) => made.ix = Some(*ix),
            }
            if present.is_none_or(|p| p.row_type != row.row_type) {
                made.row_type = row.row_type.clone();
            }
            section.rows.push(Arc::new(made));
        }
    }
    if section.rows.is_empty() {
        return Ok(());
    }
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let mut counted = shape.counted_sheet();
    counted.push_section(Arc::new(section));
    counted.sheet.check_limits()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------------- the change

/// The cells a change writes, which what reads them is worked out again from.
fn written(at: &ShapeRef, one_d: bool) -> Vec<(SheetRef, CellAddr)> {
    let names: &[&str] = if one_d {
        &["BeginX", "BeginY", "EndX", "EndY", "PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle"]
    } else {
        &["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"]
    };
    names.iter().map(|name| (SheetRef::Shape(at.clone()), CellAddr::direct(name))).collect()
}

fn recalculate(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    if cells.is_empty() {
        return Ok(());
    }
    super::settle::commit(doc, page, &Changes { cells, ..Changes::default() })
}

/// What a changed shape needs after the recalculation.
struct Done {
    id: u32,
    at: ShapeRef,
    old: Old,
    /// The cells the new master's `ReplaceCopyCells` names that the shape had, read before the change (`change_copy.rs`).
    carry: Vec<super::change_copy::Carried>,
}

/// Puts the centre of a two-dimensional shape's box back where it was, when the new master's pin is elsewhere in the box. A pin that is a
/// formula is left: the shape holds it of its own.
fn recentre(doc: &mut Document, done: &Done) -> Result<bool, Problem> {
    if done.old.one_d {
        return Ok(false);
    }
    let (now, sheet, own) = {
        let reader = Reader::new(doc);
        let sheet = SheetRef::Shape(done.at.clone());
        let pose = reader.pose(&sheet);
        let now = pose.up((reader.number(&sheet, "Width") / 2.0, reader.number(&sheet, "Height") / 2.0));
        let shape = doc.shape(&done.at).ok_or_else(|| Problem::failed("the shape went away"))?;
        let constant = |name: &str| shape.sheet.cell(name).is_some_and(|c| !c.owns_formula());
        (now, sheet, constant("PinX") && constant("PinY"))
    };
    let (dx, dy) = (done.old.centre.0 - now.0, done.old.centre.1 - now.1);
    if !own || (dx.abs() <= EPS && dy.abs() <= EPS) || !dx.is_finite() || !dy.is_finite() {
        return Ok(false);
    }
    let reader = Reader::new(doc);
    let pin = (reader.number(&sheet, "PinX"), reader.number(&sheet, "PinY"));
    write_length(doc, &sheet, "PinX", super::checked("PinX", pin.0 + dx)?)?;
    write_length(doc, &sheet, "PinY", super::checked("PinY", pin.1 + dy)?)?;
    Ok(true)
}

pub(crate) fn glue_index(c: &diacraft_doc::Connect) -> Option<u32> {
    c.to_part?.checked_sub(TO_PART_BASE)
}

/// The end of a connector that a `Connect` row is for, as a `Connect` row says it (the X cell).
fn which_of(c: &diacraft_doc::Connect) -> Option<Which> {
    match c.from_cell.as_deref() {
        Some("BeginX") => Some(Which::Begin),
        Some("EndX") => Some(Which::End),
        _ => None,
    }
}

/// Glues again the ends that were glued to a point the new master lacks or to a member that is gone.
fn repair_glue(doc: &mut Document, page: u32, done: &[Done]) -> Result<Vec<Reglued>, Problem> {
    // The shape each id belongs to: the shape that changed, and the members it had.
    let mut owner: HashMap<u32, usize> = HashMap::new();
    for (k, d) in done.iter().enumerate() {
        for gone in &d.old.removed {
            owner.insert(*gone, k);
        }
    }
    let mut points: Vec<HashSet<u32>> = Vec::with_capacity(done.len());
    {
        let reader = Reader::new(doc);
        for d in done {
            points.push(reader.connection_points(&d.at).iter().map(|p| p.index).collect());
        }
    }
    let by_id: HashMap<u32, usize> = done.iter().enumerate().map(|(k, d)| (d.id, k)).collect();
    let mut lost: Vec<(u32, Which, usize)> = Vec::new();
    for c in doc.page(page).map(|p| p.connects.iter()).into_iter().flatten() {
        let Some(which) = which_of(c) else { continue };
        let target = if let Some(k) = by_id.get(&c.to_sheet) {
            // The shape is the one that changed: its point is lost when the new master has no such point (a whole-shape glue stays).
            match glue_index(c) {
                Some(index) if !points.get(*k).is_some_and(|p| p.contains(&index)) => *k,
                _ => continue,
            }
        } else if let Some(k) = owner.get(&c.to_sheet) {
            *k
        } else {
            continue;
        };
        if lost.len() >= MAX_RECONNECTED_ENDS {
            return Err(Problem::failed(format!(
                "more than {MAX_RECONNECTED_ENDS} connector ends are glued to points the new master lacks: change fewer shapes at once"
            )));
        }
        lost.push((c.from_sheet, which, target));
    }
    let mut out = Vec::with_capacity(lost.len());
    for (connector, which, k) in lost {
        let Some(d) = done.get(k) else { continue };
        let any = points.get(k).is_some_and(|p| !p.is_empty());
        let now = if any {
            match reconnect_unsettled(doc, page, connector, which, &End::Shape { id: d.id, point: None }) {
                Ok(()) => doc
                    .page(page)
                    .and_then(|p| p.connects.iter().find(|c| c.from_sheet == connector && which_of(c) == Some(which)).and_then(glue_index))
                    .map_or(Glued::Left, Glued::Point),
                Err(_) => Glued::Left,
            }
        } else {
            match release_end(doc, page, connector, which) {
                Ok(()) => Glued::Free,
                Err(_) => Glued::Left,
            }
        };
        out.push(Reglued { connector, begin: which == Which::Begin, shape: d.id, now });
    }
    Ok(out)
}

/// Changes the shapes `ids` of a page to the master `to`, in one edit of the document the caller makes on a copy. `above` is the
/// highest `ID` the session has given on the page, as for a drop.
pub fn change_shapes(doc: &mut Document, page: u32, ids: &[u32], to: &Replacement<'_>, above: u32) -> Result<Changed, Problem> {
    require_page(doc, page)?;
    let ids = distinct(ids);
    if ids.len() > MAX_CHANGED_SHAPES {
        return Err(Problem::bad(format!("`ids` names {} shapes, and at most {MAX_CHANGED_SHAPES} are changed at once", ids.len())));
    }
    check_locks(doc, page)?;
    // Every shape named must be on the page, whatever becomes of it: found in one walk of the page, and the first one named that is missing refused.
    let named = Named::of(doc, page, ids.iter().copied())?;
    let mut places: Vec<(u32, ShapeRef)> = Vec::with_capacity(ids.len());
    for id in &ids {
        places.push((*id, named.require(*id)?));
    }
    let before = doc.clone();
    let (mid, brought) = bring(doc, to)?;
    let new = master_facts(doc, mid)?;
    if new.replace_locked {
        return Err(Problem::failed(format!("master `{}` cannot replace shapes: its LockReplace is on", crate::text::shown(&new.name, 64))));
    }
    let mut report = Changed {
        master: mid,
        name: new.name.clone(),
        brought,
        changed: Vec::new(),
        unchanged: Vec::new(),
        skipped: Vec::new(),
        reglued: Vec::new(),
        highest_id: 0,
    };
    let mut done: Vec<Done> = Vec::new();
    let mut made_members = 0_usize;
    for (id, at) in places {
        match classify(doc, &at, id, &new) {
            Verdict::Skip(why) => report.skipped.push(Skipped { id, why }),
            Verdict::Unchanged => report.unchanged.push(id),
            Verdict::Change => {
                let old = gather(doc, &at, id)?;
                let carry = if new.copy.is_empty() { Vec::new() } else { super::change_copy::carried(doc, &at, new.sheet.as_ref(), &new.copy) };
                let made = if old.one_d { None } else { Some(drop_instance(doc, page, to, above, id)?) };
                let (dropped, numbers, highest, keeps) = match made {
                    Some(d) => (Some(d.shape), d.numbers, d.highest, d.keeps),
                    None => (None, HashMap::new(), 0, (false, false)),
                };
                report.highest_id = report.highest_id.max(highest);
                // A file holds no shapes nested deeper than a group may make them (`MAX_GROUP_DEPTH`): the new members go below the shape, which is `at.path.len()` levels down.
                if let Some(shape) = dropped.as_deref() {
                    let deepest = at.path.len().saturating_sub(1).saturating_add(super::group::levels(shape));
                    if deepest > super::MAX_GROUP_DEPTH {
                        report.skipped.push(Skipped {
                            id,
                            why: format!(
                                "shape {id} would be nested too deeply by master `{}`: {deepest} levels, and at most {} are allowed",
                                crate::text::shown(&new.name, 64),
                                super::MAX_GROUP_DEPTH
                            ),
                        });
                        continue;
                    }
                }
                if let Some(shape) = dropped.as_deref() {
                    made_members = made_members.saturating_add(members_below(shape, MAX_NEW_MEMBERS));
                    if made_members > MAX_NEW_MEMBERS {
                        return Err(Problem::failed(format!(
                            "the new shapes would list more than {MAX_NEW_MEMBERS} members of their own, the most one change gives out: change fewer shapes at once"
                        )));
                    }
                }
                let merged = merged_shape(&old, id, &new, dropped.as_deref(), (&numbers, keeps))?;
                *shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))? = merged;
                report.changed.push(id);
                done.push(Done { id, at, old, carry });
            }
        }
    }
    if done.is_empty() {
        if report.unchanged.is_empty()
            && let Some(first) = report.skipped.first()
        {
            let more = report.skipped.len().saturating_sub(1);
            let tail = if more == 0 { String::new() } else { format!(" ({more} more shapes were left as they are)") };
            return Err(Problem::failed(format!("nothing was changed: {}{tail}", first.why)));
        }
        return Ok(report);
    }
    // What reads a cell the change wrote is worked out again, then the outline of a member is made local as a group's members have it.
    let cells: Vec<(SheetRef, CellAddr)> = done.iter().flat_map(|d| written(&d.at, d.old.one_d)).collect();
    recalculate(doc, page, cells)?;
    let mut moved: Vec<(SheetRef, CellAddr)> = Vec::new();
    for d in &done {
        if recentre(doc, d)? {
            moved.extend(["PinX", "PinY"].iter().map(|n| (SheetRef::Shape(d.at.clone()), CellAddr::direct(n))));
        }
    }
    recalculate(doc, page, moved)?;
    for d in &done {
        write_data(doc, page, &d.at, &d.old.data, new.locks.data)?;
        // The cells the new master lists have the last word: they beat the locks and the shape data above.
        super::change_copy::put(doc, page, &d.at, &d.carry)?;
        if d.old.nested && !d.old.one_d {
            let mut listed = vec![d.at.clone()];
            listed.extend(below(doc, &d.at));
            charge_all(doc, &listed)?;
            localise_geometry(doc, page, &listed)?;
        }
    }
    report.reglued = repair_glue(doc, page, &done)?;
    let touched: Vec<u32> = done.iter().map(|d| d.id).collect();
    let moved_ends: Vec<u32> = report.reglued.iter().map(|r| r.connector).collect();
    settle(doc, &before, page, &touched, &moved_ends)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_doc::{MAX_CELLS_PER_CONTAINER, MAX_SECTIONS_PER_SHEET};

    fn sections(prefix: &str, n: usize) -> Vec<Arc<Section>> {
        (0..n).map(|i| Arc::new(Section::new(&format!("{prefix}{i}"), None))).collect()
    }

    fn cells(prefix: &str, n: usize) -> BTreeMap<CellName, Cell> {
        (0..n).map(|i| (CellName::new(&format!("{prefix}{i}")), Cell::constant(Value::from_text("1")))).collect()
    }

    fn old_with(sheet: Sheet) -> Old {
        Old {
            shape: Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(1) }),
            one_d: false,
            centre: (0.0, 0.0),
            frozen: Vec::new(),
            data: Vec::new(),
            auto_name: false,
            removed: Vec::new(),
            nested: false,
        }
    }

    fn merge(old: &Old, base: &Sheet) -> Result<Sheet, Problem> {
        merged_sheet(old, Some(base), &HashMap::new(), Locks::default(), (false, false))
    }

    /// A shape and a new instance that hold more sections together than a sheet may are refused in words, and at the limit they are not.
    #[test]
    fn sections_of_both_sheets_past_the_limit_are_refused_in_words() {
        let old = old_with(Sheet { sections: sections("Old", MAX_SECTIONS_PER_SHEET / 2 + 1), ..Sheet::default() });
        let base = Sheet { sections: sections("New", MAX_SECTIONS_PER_SHEET / 2), ..Sheet::default() };
        let why = merge(&old, &base).expect_err("one section too many");
        assert!(why.message.contains("sections") && why.message.contains(&MAX_SECTIONS_PER_SHEET.to_string()), "{}", why.message);
        let old = old_with(Sheet { sections: sections("Old", MAX_SECTIONS_PER_SHEET / 2), ..Sheet::default() });
        let merged = merge(&old, &base).expect("exactly at the limit");
        assert_eq!(merged.sections.len(), MAX_SECTIONS_PER_SHEET);
    }

    /// The cells of a shape and of a new instance that are more together than a sheet may hold are refused in words; names both have are one cell.
    #[test]
    fn cells_of_both_sheets_past_the_limit_are_refused_in_words() {
        let old = old_with(Sheet { cells: cells("Old", MAX_CELLS_PER_CONTAINER / 2 + 1), ..Sheet::default() });
        let base = Sheet { cells: cells("New", MAX_CELLS_PER_CONTAINER / 2), ..Sheet::default() };
        let why = merge(&old, &base).expect_err("one cell too many");
        assert!(why.message.contains("cells") && why.message.contains(&MAX_CELLS_PER_CONTAINER.to_string()), "{}", why.message);
        let old = old_with(Sheet { cells: cells("Old", MAX_CELLS_PER_CONTAINER / 2), ..Sheet::default() });
        assert_eq!(merge(&old, &base).expect("exactly at the limit").cells.len(), MAX_CELLS_PER_CONTAINER);
        // The same names on both sides are one cell each: a shape that holds the cells of the new instance does not count them twice.
        let same = Sheet { cells: cells("Old", MAX_CELLS_PER_CONTAINER), ..Sheet::default() };
        assert_eq!(merge(&old_with(same.clone()), &same).expect("the same names").cells.len(), MAX_CELLS_PER_CONTAINER);
    }

    /// Merging the sections of two sheets looks at each section a few times at most: counted, for sheets of 1,000 and 4,000 sections each side, half of the new
    /// instance's sections being ones the shape holds too. A search of the whole list for each section of the other would count the square.
    #[test]
    fn merging_the_sections_of_two_sheets_costs_a_few_steps_for_each_section() {
        for n in [1_000_usize, 4_000] {
            let old = old_with(Sheet { sections: sections("S", n), ..Sheet::default() });
            let mut taken = sections("S", n / 2);
            taken.extend(sections("B", n / 2));
            let base = Sheet { sections: taken, ..Sheet::default() };
            STEPS.with(|c| c.set(0));
            let merged = merge(&old, &base).expect("within the limit");
            let steps = STEPS.with(std::cell::Cell::get);
            assert_eq!(merged.sections.len(), n + n / 2);
            assert!(steps <= 4 * (n + n), "{n} sections each side: {steps} steps");
        }
    }
}
