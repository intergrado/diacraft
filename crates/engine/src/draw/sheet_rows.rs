//! Rows and sections of a ShapeSheet: what `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section` do to a document, on the rules of the
//! design of the ShapeSheet window (section 7.6) as the window's spec amends them.
//!
//! * **Which sections allow it** (`diacraft_doc::DISPLAY_ORDER`, the rules of each entry): rows of Geometry, Connection Points, Controls, Scratch, Shape Data, User-defined Cells,
//!   Hyperlinks, Actions and Action Tag, and the stops of the two gradients; sections of those, on a shape or on any sheet as the entry says, and the groups Text Transform and
//!   Layer Membership on a shape. The text makes the rows of Character, Paragraph, Tabs and Text Fields, the layer commands those of Layers and the comments those of Reviewer and
//!   Annotation: a refusal names the command that does.
//! * **A new row** holds every cell the catalogue lists for its section, at its default as a value; a Geometry row's `X` and `Y` are the row before it's, so that the drawing does
//!   not change until it is edited, and its other cells are those its type uses, at 0. An unnamed row of a named section is `Row_1`, `Row_2` and on, the first free in any
//!   letter case. A new section holds one row, a Geometry section a `MoveTo` and a `LineTo`, an Ellipse or an Infinite Line section one row; Text Transform holds the seven
//!   formulas of and Layer Membership an empty `LayerMember`.
//! * **Indexed rows keep their `IX` as their key, and a reference names a row by its number**. An insert takes the
//!   number after the row it goes after (or the number of the row it goes before, which and what follows move up one): **a number nobody holds is taken, and nothing moves; a
//!   number a row holds is taken by that row and the run of rows after it, numbered one up**; at the end it is one past the largest. A delete closes the run after it up. **Every
//!   formula that names a row that moved names it still, and one that named a row that went reads `#REF!`** ([`super::sheet_follow`], R29, a project rule), the `Connect` rows
//!   of the connection points too. **A master's own rows, a style's and the root's keep their numbers** (the instances and the shapes that read them hold rows by them), and an insert
//!   that would move them is refused in words, so that the row goes at the end. **A sheet that inherits the section** (a dropped shape's Geometry and connection points, the stops a
//!   shape gets from its style) **takes its own copy of the rows first** ([`super::sheet_copy`], the owner's ruling of 2026-11-26) and the insert goes as it goes into a section the sheet
//!   holds: a later edit of the master's rows no longer reaches that instance, and the copy and the insert are one undo step, with no `Del` row left behind.
//! * A Geometry section that holds an Ellipse or an Infinite Line row holds that row alone: no row is added to it, and such a row is added only to an empty section or made
//!   with a new one (`sheet.add_section`'s `variant`). No command changes a row's type, so there is no change of type to refuse.
//! * **Rows and sections a master or a style gives** are dropped by a local `Del` row or section with its key and no cell, the numbers of the others
//!   staying as they are; a row or a section the sheet holds and no other sheet gives is taken out.
//! * **One step, all or nothing.** The caller edits a copy of the document, so a refusal leaves the drawing as it was. The pages that read what changed are worked out again once,
//!   at the end ([`settle`]).
//!
//! **Costs, as counts**: the sheet's section is read once for its rows and once for the keys of every sheet it is looked for on; the formulas are found by [`super::sheet_follow`],
//! which reads a formula only when its text holds the section's name; the rows of a section move in one pass over its rows.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::spell_row;
use diacraft_doc::{
    Cell, CellAddr, CellName, DISPLAY_ORDER, DisplaySection, Document, Keying, MAX_ROWS_PER_SECTION, Owner, Resolver, Row, RowKey, RowShape, Section,
    SectionKey, SectionKind, Sheet, SheetKind, SheetRef, Source, Value as Held, ValueKind, cell_info, display_section, ix_to_row_number, keying,
    next_row_number, row_number_to_ix, section_class, section_spelling,
};

use super::sheet::{default_text, pages_using_master, too_large};
use super::sheet_copy::{CopyPlan, MAX_COPIED_CELLS, apply_copy, mask_leaks, plan_copy};
pub use super::sheet_follow::Work;
use super::sheet_follow::{Fate, Followed, Remap, follow};
use super::{Problem, settle};
use crate::sheetview::{CellView, Counts, Ctx, Place, Target, label_of, name_of, view};

/// The most characters of a row's name (the model holds names to 1,024 bytes: four bytes a character).
pub const MAX_NAME_CHARS: usize = 256;

/// The fifteen row types of a Geometry section, in the order of the section's spec.
pub const GEOMETRY_ROW_TYPES: [&str; 15] = [
    "MoveTo",
    "RelMoveTo",
    "LineTo",
    "RelLineTo",
    "ArcTo",
    "EllipticalArcTo",
    "RelEllipticalArcTo",
    "PolylineTo",
    "NURBSTo",
    "SplineStart",
    "SplineKnot",
    "RelCubBezTo",
    "RelQuadBezTo",
    "Ellipse",
    "InfiniteLine",
];

/// A row a call names: by its number, as a reference spells it, or by its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowRef {
    Number(u32),
    Text(String),
}

/// Where a new row goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum At {
    /// After the last row.
    End,
    After(RowRef),
    Before(RowRef),
}

/// What `sheet.add_row` asks.
#[derive(Clone, Debug)]
pub struct AddRow {
    pub section: String,
    pub at: At,
    /// The name of a row of a named section; none for the first `Row_n` that is free.
    pub name: Option<String>,
    /// The type of a Geometry row; none for `LineTo`.
    pub row_type: Option<String>,
}

/// What a delete did to the row or the section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// It was taken out.
    Removed,
    /// It is given by a master or a style: the sheet holds a deleted row or section (`Del`) in its place.
    Marked,
}

impl How {
    pub fn word(self) -> &'static str {
        match self {
            How::Removed => "removed",
            How::Marked => "marked",
        }
    }
}

/// What a call numbered again, as counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Renumbered {
    /// The rows that took another number.
    pub rows: usize,
    /// The Geometry sections that took another number.
    pub sections: usize,
    /// The formulas written again with the new numbers.
    pub formulas: usize,
    /// The `Connect` rows that followed a connection point or went with it.
    pub connects: usize,
    /// The rows an instance took as its own before an insert in the middle of what it inherits, and the cells of them.
    pub copied_rows: usize,
    pub copied_cells: usize,
    pub work: Work,
}

impl Renumbered {
    fn of(followed: &Followed, rows: usize, sections: usize) -> Renumbered {
        Renumbered { rows, sections, formulas: followed.formulas, connects: followed.connects, work: followed.work, ..Renumbered::default() }
    }

    /// The same with the copy an insert took first.
    fn copying(self, copy: Option<&CopyPlan>) -> Renumbered {
        Renumbered { copied_rows: copy.map_or(0, |c| c.rows.len()), copied_cells: copy.map_or(0, |c| c.cells), ..self }
    }
}

/// A section as an answer names it.
#[derive(Clone, Debug)]
pub struct SectionId {
    /// The spelling a reference uses (`Geometry2`, `Prop`, `User`), or a group's label.
    pub name: String,
    /// What the window titles it (`Geometry 2`, `Shape Data`).
    pub label: String,
    pub ix: Option<u32>,
    /// The section of rows' key; `None` for a group of cells.
    pub key: Option<SectionKey>,
}

/// What a row command did.
#[derive(Clone, Debug)]
pub struct RowDone {
    pub place: Place,
    pub section: SectionId,
    pub row: RowKey,
    /// A Geometry row's type.
    pub row_type: Option<String>,
    /// The cells of a new row, as the sheet shows them after the call.
    pub cells: Vec<CellView>,
    /// For a delete: what was done to the row.
    pub how: Option<How>,
    pub renumbered: Renumbered,
    /// Something the answer says besides: the rows past a gradient's tenth stop are ignored.
    pub note: Option<String>,
}

/// What a section command did.
#[derive(Clone, Debug)]
pub struct SectionDone {
    pub place: Place,
    pub section: SectionId,
    /// The rows a new section holds.
    pub rows: Vec<RowKey>,
    /// The cells a group inserted holds.
    pub cells: Vec<CellView>,
    pub how: Option<How>,
    /// The cells a group deleted took with it.
    pub removed_cells: usize,
    pub renumbered: Renumbered,
}

// ---- the sheet to change -----------------------------------------------------------------------------------------------------------------

/// The sheet `at` names, to change: every `Arc` on the way is made unique first, so a copy of the document that shares them keeps what it had. `None` for a sheet that is not there.
pub(super) fn sheet_mut<'a>(doc: &'a mut Document, at: &SheetRef) -> Option<&'a mut Sheet> {
    doc.counted_mut(at).map(|counted| counted.sheet)
}

/// A sheet that others inherit from or read by number: a master's shape or page sheet, a style. Its rows keep their numbers.
fn is_source(sheet: &SheetRef) -> bool {
    match sheet {
        SheetRef::Shape(at) => matches!(at.owner, Owner::Master(_)),
        SheetRef::Master(_) | SheetRef::Style(_) => true,
        SheetRef::Page(_) | SheetRef::Document => false,
    }
}

/// Who numbers the rows of a sheet that is a source: the words of a refusal.
fn source_words(sheet: &SheetRef) -> &'static str {
    match sheet {
        SheetRef::Shape(_) => "the instances of this master",
        SheetRef::Master(_) => "the pages that use this master",
        _ => "the shapes that use this style",
    }
}

fn who(kind: SheetKind) -> &'static str {
    match kind {
        SheetKind::Shape => "a shape",
        SheetKind::Page => "a page",
        SheetKind::Document => "the document",
        SheetKind::Style => "a style",
    }
}

fn the(kind: SheetKind) -> &'static str {
    match kind {
        SheetKind::Shape => "the shape",
        SheetKind::Page => "the page",
        SheetKind::Document => "the document",
        SheetKind::Style => "the style",
    }
}

struct Opened {
    place: Place,
    sheet: SheetRef,
    kind: SheetKind,
}

fn open(doc: &Document, target: &Target) -> Result<Opened, Problem> {
    let place = target.resolve(doc).map_err(Problem::bad)?;
    let sheet = place.sheet(doc).ok_or_else(|| Problem::failed("the sheet is not in the drawing"))?;
    let kind = place.kind();
    Ok(Opened { place, sheet, kind })
}

fn document_error(error: diacraft_doc::DocError) -> Problem {
    Problem::failed(error.to_string())
}

// ---- naming a section --------------------------------------------------------------------------------------------------------------------

/// A section as a call names it.
enum Named {
    /// A section of rows: its entry, and for Geometry its `IX` (`None` for the bare name `Geometry`).
    Rows { entry: &'static DisplaySection, ix: Option<u32> },
    /// A group of cells directly under the sheet.
    Group(&'static DisplaySection),
}

/// A name without its white space and hyphens, in lower case, as names are compared.
fn norm(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace() && *c != '-').flat_map(char::to_lowercase).collect()
}

/// What the window titles the stops of the two gradients (the spec's R12).
fn window_label(element: &str) -> Option<&'static str> {
    match element {
        "LineGradient" => Some("Line Gradient Stops"),
        "FillGradient" => Some("Fill Gradient Stops"),
        _ => None,
    }
}

fn named_by(entry: &DisplaySection, text: &str) -> bool {
    norm(entry.label) == text
        || entry.element.is_some_and(|e| {
            norm(e) == text || section_spelling(e).is_some_and(|s| norm(s) == text) || window_label(e).is_some_and(|l| norm(l) == text)
        })
}

/// The section a call names: by what `sheet.read` calls it (`Geometry2`, `Prop`, `User`), by its label (`Shape Data`, `Connection Points`) or by its name in a file.
fn parse_named(res: &Resolver<'_>, sheet: &SheetRef, text: &str) -> Result<Named, Problem> {
    let t = norm(text);
    let geometry = display_section("Geometry").ok_or_else(|| Problem::failed("the catalogue has no Geometry section"))?;
    if let Some(number) = t.strip_prefix("geometry") {
        if number.is_empty() {
            return Ok(Named::Rows { entry: geometry, ix: None });
        }
        if number.bytes().all(|b| b.is_ascii_digit()) {
            let n: u32 = number.parse().map_err(|_| Problem::bad(format!("a Geometry section number is at most {}", u32::MAX)))?;
            let ix = n.checked_sub(1).ok_or_else(|| Problem::bad("a Geometry section is named with its number, from 1: Geometry1"))?;
            return Ok(Named::Rows { entry: geometry, ix: Some(ix) });
        }
    }
    if let Some(entry) = DISPLAY_ORDER.iter().filter(|d| d.element != Some("Geometry")).find(|d| named_by(d, &t)) {
        return Ok(match entry.kind {
            SectionKind::Rows => Named::Rows { entry, ix: None },
            SectionKind::Cells => Named::Group(entry),
        });
    }
    let held = res.effective_section_names(sheet).unwrap_or_default().iter().any(|n| norm(n.as_str()) == t);
    Err(Problem::bad(if held {
        format!("the rules of the window let no row or section of {text} be added or deleted")
    } else {
        format!("no section called {text}")
    }))
}

/// The key of a section of rows.
fn key_of(entry: &DisplaySection, ix: Option<u32>) -> SectionKey {
    SectionKey::new(entry.element.unwrap_or(entry.label), ix)
}

/// The section as an answer names it.
fn id_of(entry: &'static DisplaySection, key: &SectionKey) -> SectionId {
    SectionId { name: name_of(Some(entry), key), label: label_of(Some(entry), key), ix: key.ix, key: Some(key.clone()) }
}

/// Why rows of the section cannot be added or deleted (the words of a refusal), for an entry whose rules allow none. The window greys its Insert Row and Delete Row with these words.
pub fn rows_refusal(entry: &DisplaySection) -> String {
    let label = entry.label;
    match entry.element {
        Some("Character" | "Paragraph" | "Tabs") => format!("{label} rows are made by the text: format the text instead (text.format)"),
        Some("Field") => "Text Fields rows are made by the text: insert a field instead (text.insert_field)".to_owned(),
        Some("Layer") => "Layers rows are made by layer.add and layer.delete, which keep the members' numbers right".to_owned(),
        Some("Reviewer" | "Annotation") => format!("{label} rows are made by the comments (comment.add), not here"),
        _ => format!("{label} rows cannot be added or deleted here"),
    }
}

/// Why the section cannot be inserted or deleted on a sheet of this kind; `None` when it can. The window greys its Delete Section with these words.
pub fn section_refusal(entry: &DisplaySection, kind: SheetKind) -> Option<String> {
    if entry.rules.insertable && entry.rules.deletable && entry.rules.can_insert_on(kind) {
        return None;
    }
    let label = entry.label;
    Some(match entry.element {
        _ if label == "Shape Transform" => "Shape Transform cannot be inserted or deleted: a shape always has one".to_owned(),
        Some("Character" | "Paragraph" | "Tabs") => format!("{label} cannot be inserted or deleted here: the text makes them"),
        Some("Field") => "Text Fields cannot be inserted or deleted here: the text makes them".to_owned(),
        Some("Layer") => "Layers cannot be inserted or deleted here: layer.add and layer.delete make them".to_owned(),
        Some("Reviewer" | "Annotation") => format!("{label} cannot be inserted or deleted here: the comments make them"),
        Some("FillGradient") => "Fill Gradient Stops belongs to the fill: shape.gradient makes it".to_owned(),
        Some("LineGradient") => "Line Gradient Stops belongs to the line: format the line instead".to_owned(),
        _ if entry.rules.insertable => format!("{} has no {label} sections: only a shape has", who(kind)),
        _ => format!("{label} cannot be inserted or deleted"),
    })
}

// ---- the layers of a section -------------------------------------------------------------------------------------------------------------

/// A section as each sheet it is looked for on holds it.
struct Layers<'a> {
    /// The sheet's own, when it holds one that is not deleted.
    local: Option<&'a Arc<Section>>,
    /// The sheet holds the section as deleted (`Del`): it is gone here, and so is what a master or a style gives.
    local_deleted: bool,
    /// The master's, the styles' and the root's, nearest first, down to a layer that deletes it.
    others: Vec<&'a Arc<Section>>,
    /// Who gives it, in words.
    from: &'static str,
}

impl Layers<'_> {
    /// The section is there: the sheet holds it, or a master or a style gives it.
    fn there(&self) -> bool {
        self.local.is_some() || !self.others.is_empty()
    }

    /// Every layer's section, the sheet's first.
    fn all(&self) -> impl Iterator<Item = &Arc<Section>> {
        self.local.into_iter().chain(self.others.iter().copied())
    }
}

fn layers_of<'a>(res: &Resolver<'a>, sheet: &SheetRef, key: &SectionKey) -> Layers<'a> {
    let mut out = Layers { local: None, local_deleted: false, others: Vec::new(), from: "the master" };
    let Some(list) = res.lookup_layers(sheet, section_class(key.name.as_str())) else { return out };
    for (source, layer) in list {
        let Some(found) = layer.section(key) else { continue };
        let local = matches!(source, Source::Local);
        if found.del {
            out.local_deleted |= local;
            break;
        }
        if local {
            out.local = Some(found);
        } else {
            if out.others.is_empty() {
                out.from = if matches!(source, Source::Master(_)) { "the master" } else { "the style" };
            }
            out.others.push(found);
        }
    }
    out
}

/// The keys of every row of the section on every sheet it is looked for on, a deleted row too: a number a deleted row holds is taken, since a row written over it would
/// bring back what the master gives.
fn taken_keys(layers: &Layers<'_>, element: &str) -> HashSet<RowKey> {
    let mut taken = HashSet::new();
    for section in layers.all() {
        taken.extend(section.rows.iter().filter_map(|row| row.key(element)));
    }
    taken
}

/// Whether the rows of the section are numbered: by what the catalogue says of the section, and for one that takes either, by the rows it holds (named when any is).
fn is_numbered(element: &str, taken: &HashSet<RowKey>) -> bool {
    match keying(element) {
        Keying::Indexed => true,
        Keying::Named => false,
        Keying::Either => !taken.iter().any(|k| matches!(k, RowKey::Name(_))),
    }
}

/// The `IX`es among the keys.
fn indexes(taken: &HashSet<RowKey>) -> HashSet<u32> {
    taken.iter().filter_map(|k| if let RowKey::Ix(ix) = k { Some(*ix) } else { None }).collect()
}

/// The number a reference gives the row with this `IX`, as text.
fn number_text(geometry: bool, ix: u32) -> String {
    ix_to_row_number(geometry, ix).map_or_else(|| ix.to_string(), |n| n.to_string())
}

/// The largest `IX` a row can have: a reference spells a row's number as a `u32`, which is the `IX` plus one outside Geometry.
fn max_ix(geometry: bool) -> u32 {
    if geometry { u32::MAX } else { u32::MAX - 1 }
}

// ---- the row a call names -----------------------------------------------------------------------------------------------------------------

/// The row `given` names among the rows of a section, which are `effective` and which `layers` hold: by its number in a numbered section (a whole number, or text of digits), by its
/// name in a named one, **read as a formula reads a name**: the row that spells the name exactly first, and only when none does the row that spells it in
/// another letter case that the evaluator would read, so that a delete or an insert never takes the twin of the row it was told (`diacraft_calc::spell_row`). A row that spells the
/// name exactly and is deleted here is that row, and is not there: the name is not read as its twin. `what` is the key of the call that gave it, for the words.
fn resolve_row(
    effective: &[RowShape],
    layers: &Layers<'_>,
    numbered: bool,
    geometry: bool,
    given: &RowRef,
    what: &str,
    spelled: &str,
) -> Result<RowKey, Problem> {
    if numbered {
        let number = match given {
            RowRef::Number(n) => *n,
            RowRef::Text(text) if !text.trim().is_empty() && text.trim().bytes().all(|b| b.is_ascii_digit()) => {
                text.trim().parse::<u32>().map_err(|_| Problem::bad(format!("`{what}`: a row number is at most {}", u32::MAX)))?
            }
            RowRef::Text(_) => {
                return Err(Problem::bad(format!(
                    "`{what}`: an indexed section's rows are numbered: give a whole number, as in {spelled}.A3 (the number a reference spells)"
                )));
            }
        };
        let ix = row_number_to_ix(geometry, number).ok_or_else(|| Problem::bad("row numbers start at 1"))?;
        let key = RowKey::Ix(ix);
        return if effective.iter().any(|r| r.key == key) { Ok(key) } else { Err(Problem::bad(format!("the section has no row {number}"))) };
    }
    match given {
        RowRef::Number(n) => Err(Problem::bad(format!("`{what}`: the rows of {spelled} are named: give a row's name, not {n}"))),
        RowRef::Text(text) => {
            let held: Vec<&Arc<Section>> = layers.all().collect();
            spell_row(&held, text)
                .map(RowKey::Name)
                .filter(|key| effective.iter().any(|r| &r.key == key))
                .ok_or_else(|| Problem::bad(format!("the section has no row {text}")))
        }
    }
}

/// Whether a Geometry row of this type may begin a path.
fn begins_a_path(row_type: &str) -> bool {
    matches!(row_type, "MoveTo" | "RelMoveTo" | "Ellipse" | "InfiniteLine")
}

fn path_words() -> Problem {
    Problem::bad("the first row of a Geometry section must be a MoveTo, RelMoveTo, Ellipse or InfiniteLine")
}

// ---- the cells of a new row ---------------------------------------------------------------------------------------------------------------

/// The cells past `X` and `Y` that a Geometry row of the type uses (the table of the 15 row types).
fn type_cells(row_type: &str) -> &'static [&'static str] {
    match row_type {
        "ArcTo" | "PolylineTo" | "SplineKnot" => &["A"],
        "EllipticalArcTo" | "RelEllipticalArcTo" | "SplineStart" | "RelCubBezTo" | "Ellipse" => &["A", "B", "C", "D"],
        "NURBSTo" => &["A", "B", "C", "D", "E"],
        "RelQuadBezTo" | "InfiniteLine" => &["A", "B"],
        _ => &[],
    }
}

/// A row type whose `X` and `Y` are shares of the shape's size.
fn is_relative(row_type: &str) -> bool {
    row_type.starts_with("Rel")
}

pub(super) fn plain(text: &str) -> Cell {
    Cell::constant(Held::from_text(text))
}

/// The default of a cell of a Geometry row of this type: 0, and a constructor that draws a straight run for the two cells that hold one.
fn geometry_default(row_type: &str, name: &str) -> Cell {
    match (row_type, name) {
        ("PolylineTo", "A") => plain("POLYLINE(1,1)").with_unit("POLYLINE"),
        ("NURBSTo", "E") => plain("NURBS(1,1,1,1)").with_unit("NURBS"),
        ("NURBSTo", "B" | "D") => plain("1"),
        _ => plain("0"),
    }
}

/// The cells of a new Geometry row: `X` and `Y` as given (the row before it's, else 0), and the cells its type uses at their defaults.
fn geometry_cells(row_type: &str, x: Option<Cell>, y: Option<Cell>) -> Vec<(String, Cell)> {
    let zero = || if is_relative(row_type) { plain("0") } else { plain("0").with_unit("IN") };
    let mut cells = vec![("X".to_owned(), x.unwrap_or_else(zero)), ("Y".to_owned(), y.unwrap_or_else(zero))];
    cells.extend(type_cells(row_type).iter().map(|name| ((*name).to_owned(), geometry_default(row_type, name))));
    cells
}

/// The default of a cell of a row of a section, as a value: by the kind the catalogue gives the cell. A cell with no default is not made.
pub(super) fn default_cell(element: &str, name: &str) -> Option<Cell> {
    let kind = cell_info(Some(element), name).kind;
    let text = default_text(kind)?;
    let cell = plain(text);
    Some(match kind {
        ValueKind::Length => cell.with_unit("IN"),
        ValueKind::Angle => cell.with_unit("DEG"),
        // A cell of a text kind (a prompt, a label) holds the empty text, and a text says `STR`.
        ValueKind::Text => cell.with_unit("STR"),
        _ => cell,
    })
}

/// The cells a new row of a section other than Geometry holds: each the catalogue lists for the section.
fn section_cells(entry: &DisplaySection) -> Vec<(String, Cell)> {
    let element = entry.element.unwrap_or(entry.label);
    entry.cells.iter().filter_map(|name| default_cell(element, name).map(|cell| ((*name).to_owned(), cell))).collect()
}

/// The row a plan makes: its key, its type and its cells.
fn make_row(key: &RowKey, row_type: Option<&str>, cells: Vec<(String, Cell)>) -> Row {
    let mut row = Row { row_type: row_type.map(Arc::from), ..Row::default() };
    match key {
        RowKey::Ix(ix) => row.ix = Some(*ix),
        RowKey::Name(name) => row.name = Some(Arc::clone(name)),
    }
    row.cells = cells.into_iter().map(|(name, cell)| (CellName::new(&name), cell)).collect();
    row
}

// ---- a name ------------------------------------------------------------------------------------------------------------------------------

/// A name given to a new row: letters, digits and underscores, not starting with a digit (the parser's own names), at most [`MAX_NAME_CHARS`] characters, and not a row of
/// the section in any letter case.
fn check_name(name: &str, taken: &HashSet<RowKey>) -> Result<(), Problem> {
    let characters = name.chars().count();
    if characters > MAX_NAME_CHARS {
        return Err(Problem::bad(format!("a row's name is at most {MAX_NAME_CHARS} characters, and this one has {characters}")));
    }
    let starts_well = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
    if !starts_well || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(Problem::bad("a row's name is letters, digits and underscores, not starting with a digit, and not empty"));
    }
    let folded = name.to_lowercase();
    match taken.iter().find(|k| matches!(k, RowKey::Name(n) if n.to_lowercase() == folded)) {
        Some(RowKey::Name(existing)) => Err(Problem::bad(format!("the section already has a row {existing}"))),
        _ => Ok(()),
    }
}

/// The first `Row_n` no row of the section has, in any letter case (the numbering takes account of the rows there and of those deleted before).
fn next_name(taken: &HashSet<RowKey>) -> Result<String, Problem> {
    let used: Vec<u32> = taken
        .iter()
        .filter_map(|k| match k {
            RowKey::Name(n) => n.to_lowercase().strip_prefix("row_").and_then(|digits| digits.parse().ok()),
            RowKey::Ix(_) => None,
        })
        .collect();
    let n = next_row_number(&used).ok_or_else(|| Problem::failed("the section holds a row named Row_ with every number"))?;
    Ok(format!("Row_{n}"))
}

// ---- the number of a new row --------------------------------------------------------------------------------------------------------------

/// The numbers of an insert at `n` in a numbered section whose rows have the `IX`es in `held`: none to move when `n` is free, else the run of rows from `n` on that have
/// each its number held, each moving one up. `None` when the last of them has no number to move to (`max`).
fn run_up(held: &HashSet<u32>, n: u32, max: u32) -> Option<Vec<(u32, u32)>> {
    let mut moves = Vec::new();
    let mut at = n;
    while held.contains(&at) {
        let to = at.checked_add(1).filter(|to| *to <= max)?;
        moves.push((at, to));
        at = to;
    }
    Some(moves)
}

/// The numbers of a delete of the row `gone`: the run of rows after it, each moving one down.
fn run_down(held: &HashSet<u32>, gone: u32) -> Vec<(u32, u32)> {
    let mut moves = Vec::new();
    let mut at = gone;
    while let Some(next) = at.checked_add(1).filter(|next| held.contains(next)) {
        moves.push((next, at));
        at = next;
    }
    moves
}

// ---- adding a row ---------------------------------------------------------------------------------------------------------------------------

/// What an insert of a row is, decided against the document as found.
struct RowPlan {
    entry: &'static DisplaySection,
    key: SectionKey,
    row_key: RowKey,
    row: Row,
    cell_names: Vec<String>,
    /// The rows that take another number: old `IX` to new.
    shifts: Vec<(u32, u32)>,
    /// A named row's place among the sheet's own rows.
    at_local: Option<usize>,
    /// The copy of the rows an instance takes before an insert in the middle of what it inherits.
    copy: Option<CopyPlan>,
    geometry: bool,
    note: Option<String>,
}

/// The type a new Geometry row has, or the refusal.
fn row_type_of(request: &AddRow, geometry: bool) -> Result<Option<String>, Problem> {
    match (&request.row_type, geometry) {
        (None, true) => Ok(Some("LineTo".to_owned())),
        (None, false) => Ok(None),
        (Some(_), false) => Err(Problem::bad("only a Geometry row has a type")),
        (Some(text), true) => {
            let folded = text.to_lowercase();
            GEOMETRY_ROW_TYPES
                .iter()
                .find(|t| t.to_lowercase() == folded)
                .map(|t| Some((*t).to_owned()))
                .ok_or_else(|| Problem::bad(format!("{text} is no Geometry row type: {}", GEOMETRY_ROW_TYPES.join(", "))))
        }
    }
}

/// The row of the effective rows that stands before the place a new row `n` takes, in a numbered section.
fn row_before(effective: &[RowShape], n: u32) -> Option<&RowShape> {
    effective.iter().rev().find(|r| matches!(r.key, RowKey::Ix(ix) if ix < n))
}

/// The `X` and `Y` of the row before the new one, as values with their units, when both rows are lengths or both are shares of the size (the same unit).
fn previous_xy(res: &Resolver<'_>, sheet: &SheetRef, key: &SectionKey, before: Option<&RowShape>, row_type: &str) -> (Option<Cell>, Option<Cell>) {
    let Some(before) = before else { return (None, None) };
    if is_relative(row_type) != before.row_type.as_deref().is_some_and(is_relative) {
        return (None, None);
    }
    let value_of = |name: &str| {
        let addr = CellAddr { section: Some(key.clone()), row: Some(before.key.clone()), cell: CellName::new(name) };
        let found = res.resolve(sheet, &addr)?;
        let held = found.value.cell?;
        Some(Cell { value: held.value.clone(), unit: held.unit.clone(), ..Cell::default() })
    };
    (value_of("X"), value_of("Y"))
}

/// The name a call says for a new row of a named section, or the first free `Row_n`.
fn row_name(request: &AddRow, taken: &HashSet<RowKey>) -> Result<RowKey, Problem> {
    let name = match &request.name {
        Some(given) => {
            check_name(given, taken)?;
            given.clone()
        }
        None => next_name(taken)?,
    };
    Ok(RowKey::name(&name))
}

/// The place of a named row among the sheet's own rows: where `at` says, or the end. A row the master or the style gives cannot be gone after, since the sheet's own rows
/// follow what they give.
fn local_place(
    layers: &Layers<'_>,
    element: &str,
    at: &At,
    effective: &[RowShape],
    numbered: bool,
    geometry: bool,
    spelled: &str,
) -> Result<usize, Problem> {
    let local: &[Arc<Row>] = layers.local.map_or(&[], |s| &s.rows[..]);
    let (anchor, after) = match at {
        At::End => return Ok(local.len()),
        At::After(r) => (r, true),
        At::Before(r) => (r, false),
    };
    let what = if after { "after" } else { "before" };
    let key = resolve_row(effective, layers, numbered, geometry, anchor, what, spelled)?;
    match local.iter().position(|row| row.key(element).as_ref() == Some(&key)) {
        Some(at) => Ok(if after { at + 1 } else { at }),
        None => Err(Problem::bad(format!("a row of {spelled} that {} gives comes before the sheet's own rows: add the row at the end", layers.from))),
    }
}

/// The number a new row of a numbered section takes, and the rows that move for it.
struct Numbered {
    n: u32,
    shifts: Vec<(u32, u32)>,
    /// The rows are inherited and must be taken as the sheet's own first.
    copy: bool,
}

fn number_for(request: &AddRow, ctx: &Insert<'_>) -> Result<Numbered, Problem> {
    let Insert { layers, effective, taken, geometry, spelled, renumber_ok, sheet } = ctx;
    let held = indexes(taken);
    let max = max_ix(*geometry);
    let anchor_of = |given: &RowRef, what: &str| match resolve_row(effective, layers, true, *geometry, given, what, spelled)? {
        RowKey::Ix(ix) => Ok(ix),
        RowKey::Name(_) => Err(Problem::bad(format!("`{what}`: not a numbered row"))),
    };
    let (n, words) = match &request.at {
        At::End => (held.iter().max().map_or(Some(if *geometry { 1 } else { 0 }), |last| last.checked_add(1)), None),
        At::After(given) => {
            let a = anchor_of(given, "after")?;
            (a.checked_add(1), Some(format!("after row {}", number_text(*geometry, a))))
        }
        At::Before(given) => {
            let b = anchor_of(given, "before")?;
            (Some(b), Some(format!("before row {}", number_text(*geometry, b))))
        }
    };
    let Some(n) = n.filter(|n| *n <= max) else {
        let last = held.iter().max().map_or_else(|| "0".to_owned(), |last| number_text(*geometry, *last));
        return Err(Problem::failed(format!("no room for a row after row {last}: row numbers end at {}", u32::MAX)));
    };
    if !held.contains(&n) {
        return Ok(Numbered { n, shifts: Vec::new(), copy: false });
    }
    // What a master's own rows, a style's and the root's are numbered by is read by number from the sheets that inherit from them; a sheet that inherits takes a copy first.
    if is_source(sheet) {
        let words = words.unwrap_or_default();
        return Err(Problem::failed(format!("rows of {spelled} {words} are numbered by {}: add the row at the end", source_words(sheet))));
    }
    let copy = !renumber_ok;
    let shifts = run_up(&held, n, max).ok_or_else(|| {
        Problem::failed(format!(
            "the rows from {} on would be numbered past the largest number: row numbers end at {}",
            number_text(*geometry, n),
            u32::MAX
        ))
    })?;
    Ok(Numbered { n, shifts, copy })
}

/// What the plan of an insert reads of the sheet.
struct Insert<'a> {
    layers: Layers<'a>,
    effective: Vec<RowShape>,
    taken: HashSet<RowKey>,
    geometry: bool,
    spelled: String,
    /// The rows may be numbered again: the section is the sheet's own and nobody inherits from the sheet.
    renumber_ok: bool,
    sheet: SheetRef,
}

/// The rows' section, for a call that gives one of rows and names a section the sheet has.
fn rows_section<'a>(res: &Resolver<'a>, opened: &Opened, text: &str) -> Result<(&'static DisplaySection, SectionKey, Layers<'a>), Problem> {
    let (entry, ix) = match parse_named(res, &opened.sheet, text)? {
        Named::Rows { entry, ix } => (entry, ix),
        Named::Group(entry) => return Err(Problem::bad(format!("{} is a group of cells and has no rows", entry.label))),
    };
    if !entry.rules.rows_addable {
        return Err(Problem::bad(rows_refusal(entry)));
    }
    let geometry = entry.element == Some("Geometry");
    if geometry && ix.is_none() {
        return Err(Problem::bad("name the Geometry section with its number, from 1: Geometry1"));
    }
    let key = key_of(entry, ix);
    let layers = layers_of(res, &opened.sheet, &key);
    if !layers.there() {
        let spelled = name_of(Some(entry), &key);
        return Err(Problem::bad(format!("the sheet has no section {spelled}: add it first (sheet.add_section)")));
    }
    Ok((entry, key, layers))
}

fn plan_row(res: &Resolver<'_>, opened: &Opened, request: &AddRow) -> Result<RowPlan, Problem> {
    let (entry, key, layers) = rows_section(res, opened, &request.section)?;
    let element = key.name.as_str().to_owned();
    let geometry = element == "Geometry";
    let spelled = name_of(Some(entry), &key);
    let row_type = row_type_of(request, geometry)?;
    let effective = res.effective_row_keys(&opened.sheet, &key).unwrap_or_default();
    if let Some(ty) = row_type.as_deref().filter(|_| geometry) {
        alone_in_its_section(&spelled, &effective, ty)?;
    }
    let taken = taken_keys(&layers, &element);
    let numbered = is_numbered(&element, &taken);
    if numbered && request.name.is_some() {
        return Err(Problem::bad(format!("rows of {spelled} are numbered, not named")));
    }
    let renumber_ok = layers.others.is_empty() && !is_source(&opened.sheet);
    let ctx = Insert { layers, effective, taken, geometry, spelled, renumber_ok, sheet: opened.sheet.clone() };
    let (row_key, shifts, at_local, copy) = if numbered {
        let placed = number_for(request, &ctx)?;
        let copy = if placed.copy {
            Some(plan_copy(res, &opened.sheet, &key, &ctx.effective, &ctx.layers.others, &ctx.spelled, MAX_COPIED_CELLS)?)
        } else {
            None
        };
        (RowKey::Ix(placed.n), placed.shifts, None, copy)
    } else {
        let at = local_place(&ctx.layers, &element, &request.at, &ctx.effective, false, geometry, &ctx.spelled)?;
        (row_name(request, &ctx.taken)?, Vec::new(), Some(at), None)
    };
    let (cells, note) = if geometry {
        let ty = row_type.clone().unwrap_or_else(|| "LineTo".to_owned());
        if matches!(row_key, RowKey::Ix(n) if ctx.effective.first().is_none_or(|first| matches!(first.key, RowKey::Ix(f) if n <= f)))
            && !begins_a_path(&ty)
        {
            return Err(path_words());
        }
        let before = match row_key {
            RowKey::Ix(n) => row_before(&ctx.effective, n),
            RowKey::Name(_) => None,
        };
        let (x, y) = previous_xy(res, &opened.sheet, &key, before, &ty);
        (geometry_cells(&ty, x, y), None)
    } else {
        (section_cells(entry), stops_note(entry, &ctx.effective))
    };
    let cell_names = cells.iter().map(|(name, _)| name.clone()).collect();
    let row = make_row(&row_key, row_type.as_deref(), cells);
    Ok(RowPlan { entry, key, row_key, row, cell_names, shifts, at_local, copy, geometry, note })
}

/// Whether a Geometry row of this type is the only row of its section: an Ellipse and an Infinite Line (MS-VSDX forbids any other row in the section).
fn is_alone_type(row_type: &str) -> bool {
    matches!(row_type, "Ellipse" | "InfiniteLine")
}

/// A Geometry section that holds an Ellipse or an Infinite Line row holds that row alone, so no row is added to it; and such a row is added only to an empty section
/// (or made with a new one, `sheet.add_section`'s `variant`). The rows are those the sheet reads, its master's too.
fn alone_in_its_section(spelled: &str, effective: &[RowShape], new_type: &str) -> Result<(), Problem> {
    if let Some(held) = effective.iter().find_map(|r| r.row_type.as_deref().filter(|t| is_alone_type(t))) {
        return Err(Problem::bad(format!(
            "{spelled} holds an {held} row, and a Geometry section that holds an Ellipse or an Infinite Line row holds that row alone: add the row to another Geometry section (sheet.add_section)"
        )));
    }
    if is_alone_type(new_type) && !effective.is_empty() {
        return Err(Problem::bad(format!(
            "an Ellipse or an Infinite Line row can only be added to an empty section: {spelled} holds other rows, and such a row is the only one of its section: add a Geometry section with the variant {new_type} (sheet.add_section)"
        )));
    }
    Ok(())
}

/// A gradient's rows past the tenth are ignored by whatever reads them: the answer says so.
fn stops_note(entry: &DisplaySection, effective: &[RowShape]) -> Option<String> {
    let limit = entry.rules.row_limit?;
    (effective.len() >= usize::try_from(limit).unwrap_or(usize::MAX))
        .then(|| format!("rows past the {limit}th are ignored by whatever reads {}", entry.label))
}

/// Numbers the sheet's own rows of the section again: the rows in `moves` by their `IX`.
fn move_rows(section: &mut Section, element: &str, moves: &HashMap<u32, u32>) {
    for held in section.rows.iter_mut() {
        if let Some(RowKey::Ix(ix)) = held.key(element)
            && let Some(to) = moves.get(&ix)
        {
            Arc::make_mut(held).ix = Some(*to);
        }
    }
}

/// Where a numbered row goes among the sheet's own rows: before the first that has a larger `IX`.
fn numbered_place(section: &Section, element: &str, n: u32) -> usize {
    section.rows.iter().position(|row| matches!(row.key(element), Some(RowKey::Ix(ix)) if ix > n)).unwrap_or(section.rows.len())
}

fn put_row(doc: &mut Document, sheet: &SheetRef, plan: &RowPlan) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    let section = target.section_mut(&plan.key).map_err(document_error)?;
    if section.rows.len() >= MAX_ROWS_PER_SECTION {
        return Err(Problem::failed(format!("a section holds at most {MAX_ROWS_PER_SECTION} rows")));
    }
    let element = plan.key.name.as_str().to_owned();
    if !plan.shifts.is_empty() {
        move_rows(section, &element, &plan.shifts.iter().copied().collect());
    }
    let at = match (plan.at_local, &plan.row_key) {
        (Some(at), _) => at.min(section.rows.len()),
        (None, RowKey::Ix(n)) => numbered_place(section, &element, *n),
        (None, RowKey::Name(_)) => section.rows.len(),
    };
    section.insert_row(at, Arc::new(plan.row.clone()));
    Ok(())
}

/// A remap of the rows of a section: `moves` take their new `IX`, and `gone` is taken out.
fn rows_remap(sheet: &SheetRef, key: &SectionKey, moves: &[(u32, u32)], gone: Option<u32>) -> Remap {
    let mut remap = Remap::new(sheet.clone(), key.name.as_str());
    remap.rows_of = Some(key.ix);
    remap.rows = moves.iter().map(|(from, to)| (*from, Fate::To(*to))).collect();
    if let Some(ix) = gone {
        remap.rows.insert(ix, Fate::Goes);
    }
    remap
}

/// Works out again the pages that read what changed, once each: the sheet's page and the pages a formula was written on, the pages that use a master, or every page for the
/// document's sheet and a style's.
fn settle_after(doc: &mut Document, before: &Document, place: &Place, also: &BTreeSet<u32>) -> Result<(), Problem> {
    // The pages the call works out share one budget of the evaluator's steps.
    let _shared = crate::calc_index::SharedSteps::enter(diacraft_calc::Budget::DEFAULT_STEPS);
    // The sheets no page works out (the document's, a style's, a master's) first, with the formulas that read the rows that went or moved on them.
    super::unpaged::work_out(doc, before)?;
    let mut pages: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    match place {
        Place::Shape { owner: Owner::Page(page), id } => pages.entry(*page).or_default().push(*id),
        Place::Page(page) => {
            pages.entry(*page).or_default();
        }
        Place::Shape { owner: Owner::Master(master), .. } | Place::Master(master) => {
            for page in pages_using_master(doc, *master) {
                pages.entry(page).or_default();
            }
        }
        Place::Style(_) | Place::Document => {
            for page in doc.pages.iter() {
                pages.entry(page.id).or_default();
            }
        }
    }
    // Every page of the document is worked out whole, the cells of one that name another included.
    let _every = matches!(place, Place::Style(_) | Place::Document).then(crate::calc_index::EveryPage::enter);
    for page in also {
        pages.entry(*page).or_default();
    }
    for (page, ids) in &pages {
        settle(doc, before, *page, ids, &[]).map_err(|problem| too_large(problem, *page))?;
    }
    Ok(())
}

/// The cells of a row as the sheet shows them now.
fn views(doc: &Document, sheet: &SheetRef, key: &SectionKey, row: &RowKey, names: &[String]) -> Vec<CellView> {
    let res = Resolver::new(doc);
    let mut eval = crate::sheetview::read_eval_ctx();
    let ctx = Ctx::new(&res, sheet, &mut eval);
    let mut counts = Counts::default();
    names
        .iter()
        .filter_map(|name| {
            let addr = CellAddr { section: Some(key.clone()), row: Some(row.clone()), cell: CellName::new(name) };
            view(&ctx, &addr, name, None, None, &mut counts)
        })
        .collect()
}

/// Adds one row to a section of a sheet (`sheet.add_row`).
pub fn add_row(doc: &mut Document, target: &Target, request: &AddRow) -> Result<RowDone, Problem> {
    let before = doc.clone();
    let opened = open(doc, target)?;
    let plan = plan_row(&Resolver::new(&before), &opened, request)?;
    // An instance takes the rows it inherits as its own first; the formulas that name the rows that move are then found where the rows are written.
    if let Some(copy) = &plan.copy {
        apply_copy(doc, &opened.sheet, &plan.key, copy)?;
    }
    let followed = if plan.shifts.is_empty() { Followed::default() } else { follow(doc, &rows_remap(&opened.sheet, &plan.key, &plan.shifts, None))? };
    put_row(doc, &opened.sheet, &plan)?;
    if let Some(copy) = &plan.copy {
        mask_leaks(doc, &opened.sheet, &plan.key, copy)?;
    }
    settle_after(doc, &before, &opened.place, &followed.pages)?;
    let cells = views(doc, &opened.sheet, &plan.key, &plan.row_key, &plan.cell_names);
    Ok(RowDone {
        place: opened.place,
        section: id_of(plan.entry, &plan.key),
        row: plan.row_key,
        row_type: plan.row.row_type.as_deref().map(str::to_owned).filter(|_| plan.geometry),
        cells,
        how: None,
        renumbered: Renumbered::of(&followed, plan.shifts.len(), 0).copying(plan.copy.as_ref()),
        note: plan.note,
    })
}

// ---- deleting a row -------------------------------------------------------------------------------------------------------------------------

/// What a delete of a row is, decided against the document as found.
struct DeletePlan {
    entry: &'static DisplaySection,
    key: SectionKey,
    row_key: RowKey,
    how: How,
    /// The rows that take another number, and the row that goes, when the numbers close up.
    shifts: Vec<(u32, u32)>,
    follows: bool,
}

fn plan_delete(res: &Resolver<'_>, opened: &Opened, section: &str, row: &RowRef) -> Result<DeletePlan, Problem> {
    let (entry, key, layers) = rows_section(res, opened, section)?;
    let element = key.name.as_str().to_owned();
    let geometry = element == "Geometry";
    let spelled = name_of(Some(entry), &key);
    let effective = res.effective_row_keys(&opened.sheet, &key).unwrap_or_default();
    let taken = taken_keys(&layers, &element);
    let numbered = is_numbered(&element, &taken);
    let row_key = resolve_row(&effective, &layers, numbered, geometry, row, "row", &spelled)?;
    if geometry {
        // The row that would be first when this one is gone must still begin a path.
        let next_first = effective.iter().find(|r| r.key != row_key);
        if effective.first().is_some_and(|first| first.key == row_key)
            && next_first.is_some_and(|next| !next.row_type.as_deref().is_some_and(begins_a_path))
        {
            return Err(path_words());
        }
    }
    let given = layers.others.iter().any(|section| section.row(&row_key).is_some());
    let how = if given { How::Marked } else { How::Removed };
    // A row that goes from a numbered section is `#REF!` to the formulas that name it, on any sheet; the numbers close up only where the sheet's own rows are the only ones (a master's
    // and a style's keep theirs, and the formulas that name the row that went are written `REF()` all the same, or a row added after it would take its number and be read
    // by them).
    let removes = how == How::Removed && numbered;
    let closes = removes && layers.others.is_empty() && !is_source(&opened.sheet);
    let (shifts, follows) = match &row_key {
        RowKey::Ix(gone) if removes => (if closes { run_down(&indexes(&taken), *gone) } else { Vec::new() }, true),
        _ => (Vec::new(), false),
    };
    Ok(DeletePlan { entry, key, row_key, how, shifts, follows })
}

/// Takes the row out of the sheet's own section, or puts a deleted row (`Del`, its key and no cell) in its place; and closes the numbers up.
fn take_row(doc: &mut Document, sheet: &SheetRef, plan: &DeletePlan) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    let section = target.section_mut(&plan.key).map_err(document_error)?;
    let element = plan.key.name.as_str().to_owned();
    let is_it = |row: &Arc<Row>| row.key(&element).as_ref() == Some(&plan.row_key);
    match plan.how {
        How::Removed => {
            let gone: Vec<usize> = section.rows.iter().enumerate().filter(|(_, row)| is_it(row)).map(|(at, _)| at).collect();
            section.remove_rows(&gone);
        }
        How::Marked => {
            let mut tomb = Row { del: true, ..Row::default() };
            match &plan.row_key {
                RowKey::Ix(ix) => tomb.ix = Some(*ix),
                RowKey::Name(name) => tomb.name = Some(Arc::clone(name)),
            }
            let tomb = Arc::new(tomb);
            match section.rows.iter().position(is_it) {
                Some(at) => {
                    if let Some(slot) = section.rows.get_mut(at) {
                        *slot = tomb;
                    }
                    // A second row with the key is shadowed by the first and gone with it (a deleted one stays).
                    let gone: Vec<usize> = section
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(place, row)| *place > at && is_it(row) && !row.del)
                        .map(|(place, _)| place)
                        .collect();
                    section.remove_rows(&gone);
                }
                None => {
                    let end = section.rows.len();
                    section.insert_row(end, tomb);
                }
            }
        }
    }
    if !plan.shifts.is_empty() {
        move_rows(section, &element, &plan.shifts.iter().copied().collect());
    }
    Ok(())
}

/// Deletes one row of a section of a sheet (`sheet.delete_row`).
pub fn delete_row(doc: &mut Document, target: &Target, section: &str, row: &RowRef) -> Result<RowDone, Problem> {
    let before = doc.clone();
    let opened = open(doc, target)?;
    let plan = plan_delete(&Resolver::new(&before), &opened, section, row)?;
    let followed = if plan.follows {
        let gone = if let RowKey::Ix(ix) = plan.row_key { Some(ix) } else { None };
        follow(doc, &rows_remap(&opened.sheet, &plan.key, &plan.shifts, gone))?
    } else {
        Followed::default()
    };
    take_row(doc, &opened.sheet, &plan)?;
    settle_after(doc, &before, &opened.place, &followed.pages)?;
    Ok(RowDone {
        place: opened.place,
        section: id_of(plan.entry, &plan.key),
        row: plan.row_key,
        row_type: None,
        cells: Vec::new(),
        how: Some(plan.how),
        renumbered: Renumbered::of(&followed, plan.shifts.len(), 0),
        note: None,
    })
}

// ---- adding a section -----------------------------------------------------------------------------------------------------------------------

/// The seven cells of Text Transform with the formulas of `TxtAngle` is 0, as a new section holds them.
fn text_transform_cells(entry: &DisplaySection) -> Vec<(String, Cell)> {
    entry
        .cells
        .iter()
        .map(|name| {
            let info = cell_info(None, name);
            let cell = match info.default_formula {
                Some(text) => Cell::with_formula(text, None).with_unit("IN"),
                None if info.kind == ValueKind::Angle => plain("0").with_unit("DEG"),
                None => plain("0"),
            };
            ((*name).to_owned(), cell)
        })
        .collect()
}

/// The cells a group inserted holds: Text Transform's seven, and Layer Membership's one, empty.
fn group_cells(entry: &DisplaySection) -> Vec<(String, Cell)> {
    if entry.label == "Layer Membership" { vec![("LayerMember".to_owned(), plain(""))] } else { text_transform_cells(entry) }
}

/// Whether a cell of the sheet's, own or given by a master, is the cell `name`.
fn holds_cell(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> bool {
    res.resolve(sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some())
}

/// The next `IX` of a Geometry section: one past the largest any sheet it is looked for on holds, a deleted section too.
fn next_geometry_ix(res: &Resolver<'_>, sheet: &SheetRef) -> Result<u32, Problem> {
    let layers = res.lookup_layers(sheet, section_class("Geometry")).unwrap_or_default();
    let last = layers.iter().flat_map(|(_, layer)| layer.sections.iter()).filter(|s| s.name.as_str() == "Geometry").filter_map(|s| s.ix).max();
    last.map_or(Ok(0), |last| last.checked_add(1).ok_or_else(|| Problem::failed(format!("Geometry section numbers end at {}", u32::MAX))))
}

/// The rows a new section holds: a Geometry section a `MoveTo` and a `LineTo` (or one row of its variant), any other one row.
fn new_section_rows(
    entry: &'static DisplaySection,
    key: &SectionKey,
    variant: Option<&str>,
    taken_names: &HashSet<RowKey>,
) -> Result<Vec<Row>, Problem> {
    if key.name.as_str() == "Geometry" {
        let types: &[&str] = match variant {
            Some("Ellipse") => &["Ellipse"],
            Some("InfiniteLine") => &["InfiniteLine"],
            _ => &["MoveTo", "LineTo"],
        };
        return Ok(types
            .iter()
            .enumerate()
            .map(|(at, ty)| make_row(&RowKey::Ix(u32::try_from(at + 1).unwrap_or(u32::MAX)), Some(*ty), geometry_cells(ty, None, None)))
            .collect());
    }
    let row_key = if is_numbered(key.name.as_str(), taken_names) { RowKey::Ix(0) } else { RowKey::name(&next_name(taken_names)?) };
    Ok(vec![make_row(&row_key, None, section_cells(entry))])
}

/// The variant a call gives for a Geometry section: Ellipse or InfiniteLine, in any letter case and spacing.
fn variant_of(text: Option<&str>, geometry: bool) -> Result<Option<&'static str>, Problem> {
    let Some(text) = text else { return Ok(None) };
    if !geometry {
        return Err(Problem::bad("only a Geometry section has a variant"));
    }
    match norm(text).as_str() {
        "ellipse" => Ok(Some("Ellipse")),
        "infiniteline" => Ok(Some("InfiniteLine")),
        "geometry" | "" => Ok(None),
        _ => Err(Problem::bad(format!("{text} is no kind of Geometry section: Ellipse or InfiniteLine"))),
    }
}

/// What an insert of a section is.
enum SectionPlan {
    Rows { entry: &'static DisplaySection, key: SectionKey, rows: Vec<Row> },
    Group { entry: &'static DisplaySection, cells: Vec<(String, Cell)> },
}

fn plan_section(res: &Resolver<'_>, opened: &Opened, section: &str, variant: Option<&str>) -> Result<SectionPlan, Problem> {
    let named = parse_named(res, &opened.sheet, section)?;
    let entry = match &named {
        Named::Rows { entry, .. } | Named::Group(entry) => *entry,
    };
    if let Some(words) = section_refusal(entry, opened.kind) {
        return Err(Problem::bad(words));
    }
    let geometry = entry.element == Some("Geometry");
    let variant = variant_of(variant, geometry)?;
    match named {
        Named::Group(entry) => {
            if entry.cells.iter().any(|name| holds_cell(res, &opened.sheet, name)) {
                return Err(Problem::bad(format!("{} already has {}: a section other than Geometry is there once", the(opened.kind), entry.label)));
            }
            Ok(SectionPlan::Group { entry, cells: group_cells(entry) })
        }
        Named::Rows { entry, ix } => {
            if geometry && ix.is_some() {
                return Err(Problem::bad("a new Geometry section takes the next number: give section: Geometry"));
            }
            let ix = if geometry { Some(next_geometry_ix(res, &opened.sheet)?) } else { None };
            let key = key_of(entry, ix);
            let layers = layers_of(res, &opened.sheet, &key);
            if layers.local_deleted {
                return Err(Problem::bad(format!("the sheet deleted the {} it is given: undo that delete to have it again", entry.label)));
            }
            if !geometry && layers.there() {
                return Err(Problem::bad(format!("{} already has {}: a section other than Geometry is there once", the(opened.kind), entry.label)));
            }
            let taken = taken_keys(&layers, key.name.as_str());
            let rows = new_section_rows(entry, &key, variant, &taken)?;
            Ok(SectionPlan::Rows { entry, key, rows })
        }
    }
}

fn put_section(doc: &mut Document, sheet: &SheetRef, plan: &SectionPlan) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    match plan {
        SectionPlan::Rows { key, rows, .. } => {
            let section = target.section_mut(key).map_err(document_error)?;
            for row in rows {
                let end = section.rows.len();
                section.insert_row(end, Arc::new(row.clone()));
            }
        }
        SectionPlan::Group { cells, .. } => {
            for (name, cell) in cells {
                target.set_cell(&CellAddr::direct(name), cell.clone()).map_err(document_error)?;
            }
        }
    }
    Ok(())
}

/// Adds a section to a sheet (`sheet.add_section`).
pub fn add_section(doc: &mut Document, target: &Target, section: &str, variant: Option<&str>) -> Result<SectionDone, Problem> {
    let before = doc.clone();
    let opened = open(doc, target)?;
    let plan = plan_section(&Resolver::new(&before), &opened, section, variant)?;
    put_section(doc, &opened.sheet, &plan)?;
    settle_after(doc, &before, &opened.place, &BTreeSet::new())?;
    Ok(match plan {
        SectionPlan::Rows { entry, key, rows } => {
            let keys: Vec<RowKey> = rows.iter().filter_map(|row| row.key(key.name.as_str())).collect();
            SectionDone {
                place: opened.place,
                section: id_of(entry, &key),
                rows: keys,
                cells: Vec::new(),
                how: None,
                removed_cells: 0,
                renumbered: Renumbered::default(),
            }
        }
        SectionPlan::Group { entry, cells } => {
            let ctx_res = Resolver::new(doc);
            let mut eval = crate::sheetview::read_eval_ctx();
            let ctx = Ctx::new(&ctx_res, &opened.sheet, &mut eval);
            let mut counts = Counts::default();
            let views: Vec<CellView> =
                cells.iter().filter_map(|(name, _)| view(&ctx, &CellAddr::direct(name), name, None, None, &mut counts)).collect();
            SectionDone {
                place: opened.place,
                section: SectionId { name: entry.label.to_owned(), label: entry.label.to_owned(), ix: None, key: None },
                rows: Vec::new(),
                cells: views,
                how: None,
                removed_cells: 0,
                renumbered: Renumbered::default(),
            }
        }
    })
}

// ---- deleting a section --------------------------------------------------------------------------------------------------------------------

/// What a delete of a section is.
enum DeleteSectionPlan {
    Rows { entry: &'static DisplaySection, key: SectionKey, how: How, shifts: Vec<(u32, u32)>, follows: bool },
    Group { entry: &'static DisplaySection, own: Vec<CellName> },
}

/// The local Geometry sections' `IX`es after the one that goes, each moving one down while the numbers run on.
fn geometry_run(sheet: &Sheet, gone: u32) -> Vec<(u32, u32)> {
    let held: HashSet<u32> = sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").filter_map(|s| s.ix).collect();
    run_down(&held, gone)
}

fn plan_delete_section(res: &Resolver<'_>, opened: &Opened, section: &str) -> Result<DeleteSectionPlan, Problem> {
    let named = parse_named(res, &opened.sheet, section)?;
    let entry = match &named {
        Named::Rows { entry, .. } | Named::Group(entry) => *entry,
    };
    if !entry.rules.can_delete_on(opened.kind) {
        return Err(Problem::bad(section_refusal(entry, opened.kind).unwrap_or_else(|| format!("{} cannot be deleted", entry.label))));
    }
    match named {
        Named::Group(entry) => {
            let local = res.sheet(&opened.sheet).ok_or_else(|| Problem::failed("the sheet is not in the drawing"))?;
            let own: Vec<CellName> =
                entry.cells.iter().map(|name| CellName::new(name)).filter(|name| local.cells.contains_key(name.as_str())).collect();
            if own.is_empty() {
                let given = entry.cells.iter().any(|name| holds_cell(res, &opened.sheet, name));
                return Err(Problem::bad(if given {
                    format!("{} holds none of {} of its own: a master gives it", the(opened.kind), entry.label)
                } else {
                    format!("the sheet has no section {}", entry.label)
                }));
            }
            Ok(DeleteSectionPlan::Group { entry, own })
        }
        Named::Rows { entry, ix } => {
            if entry.element == Some("Geometry") && ix.is_none() {
                return Err(Problem::bad("name the Geometry section with its number, from 1: Geometry1"));
            }
            let key = key_of(entry, ix);
            let layers = layers_of(res, &opened.sheet, &key);
            if !layers.there() {
                return Err(Problem::bad(format!("the sheet has no section {}", name_of(Some(entry), &key))));
            }
            let how = if layers.others.is_empty() { How::Removed } else { How::Marked };
            // The formulas that name a section that goes read `#REF!` on any sheet (a master's and a style's too); the later Geometry sections are numbered again only on a
            // sheet that is no source.
            let follows = how == How::Removed;
            let shifts = match (follows && !is_source(&opened.sheet), key.ix, res.sheet(&opened.sheet)) {
                (true, Some(gone), Some(local)) if key.name.as_str() == "Geometry" => geometry_run(local, gone),
                _ => Vec::new(),
            };
            Ok(DeleteSectionPlan::Rows { entry, key, how, shifts, follows })
        }
    }
}

/// Takes a section out of the sheet, or puts a deleted one (`Del`, its name and `IX`, nothing else) in its place; and numbers the later Geometry sections again.
fn take_section(doc: &mut Document, sheet: &SheetRef, key: &SectionKey, how: How, shifts: &[(u32, u32)]) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    match how {
        How::Removed => target.remove_sections(|s| s.name == key.name && s.ix == key.ix),
        How::Marked => {
            let section = target.section_mut(key).map_err(document_error)?;
            *section = Section { del: true, ..Section::new(key.name.as_str(), key.ix) };
        }
    }
    let moves: HashMap<u32, u32> = shifts.iter().copied().collect();
    for held in target.sheet.sections.iter_mut() {
        if held.name == key.name
            && let Some(ix) = held.ix
            && let Some(to) = moves.get(&ix)
        {
            Arc::make_mut(held).ix = Some(*to);
        }
    }
    Ok(())
}

/// Deletes a section of a sheet (`sheet.delete_section`).
pub fn delete_section(doc: &mut Document, target: &Target, section: &str) -> Result<SectionDone, Problem> {
    let before = doc.clone();
    let opened = open(doc, target)?;
    match plan_delete_section(&Resolver::new(&before), &opened, section)? {
        DeleteSectionPlan::Group { entry, own } => {
            let mut counted = doc.counted_mut(&opened.sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
            for name in &own {
                counted.remove_cell(&CellAddr::direct(name.as_str()));
            }
            settle_after(doc, &before, &opened.place, &BTreeSet::new())?;
            Ok(SectionDone {
                place: opened.place,
                section: SectionId { name: entry.label.to_owned(), label: entry.label.to_owned(), ix: None, key: None },
                rows: Vec::new(),
                cells: Vec::new(),
                how: Some(How::Removed),
                removed_cells: own.len(),
                renumbered: Renumbered::default(),
            })
        }
        DeleteSectionPlan::Rows { entry, key, how, shifts, follows } => {
            let followed = if follows {
                let mut remap = Remap::new(opened.sheet.clone(), key.name.as_str());
                remap.sections.insert(key.ix, Fate::Goes);
                remap.sections.extend(shifts.iter().map(|(from, to)| (Some(*from), Fate::To(*to))));
                follow(doc, &remap)?
            } else {
                Followed::default()
            };
            take_section(doc, &opened.sheet, &key, how, &shifts)?;
            settle_after(doc, &before, &opened.place, &followed.pages)?;
            Ok(SectionDone {
                place: opened.place,
                section: id_of(entry, &key),
                rows: Vec::new(),
                cells: Vec::new(),
                how: Some(how),
                removed_cells: 0,
                renumbered: Renumbered::of(&followed, 0, shifts.len()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn held(ixs: &[u32]) -> HashSet<u32> {
        ixs.iter().copied().collect()
    }

    #[test]
    fn an_insert_at_a_free_number_moves_nothing_and_at_a_held_one_moves_the_run_after_it_up() {
        assert_eq!(run_up(&held(&[1, 2, 3, 5, 6]), 4, 10), Some(vec![]));
        assert_eq!(run_up(&held(&[1, 2, 3, 5, 6]), 3, 10), Some(vec![(3, 4)]), "4 is free: only row 3 moves, and the rows after the gap stay");
        assert_eq!(run_up(&held(&[1, 2, 3]), 2, 10), Some(vec![(2, 3), (3, 4)]));
        assert_eq!(run_up(&held(&[]), 1, 10), Some(vec![]));
    }

    #[test]
    fn an_insert_that_would_push_a_row_past_the_largest_number_has_no_run() {
        assert_eq!(run_up(&held(&[9, 10]), 9, 10), None);
        assert_eq!(run_up(&held(&[u32::MAX]), u32::MAX, u32::MAX), None);
        assert_eq!(run_up(&held(&[u32::MAX - 1, u32::MAX]), u32::MAX - 1, u32::MAX), None);
        assert_eq!(run_up(&held(&[u32::MAX - 1]), u32::MAX - 1, u32::MAX), Some(vec![(u32::MAX - 1, u32::MAX)]));
    }

    #[test]
    fn a_delete_closes_up_the_run_after_the_row_and_no_more() {
        assert_eq!(run_down(&held(&[1, 2, 3, 4, 5]), 3), vec![(4, 3), (5, 4)]);
        assert_eq!(run_down(&held(&[1, 2, 4, 5]), 2), Vec::<(u32, u32)>::new(), "a gap is not closed");
        assert_eq!(run_down(&held(&[u32::MAX]), u32::MAX - 1), vec![(u32::MAX, u32::MAX - 1)]);
        assert_eq!(run_down(&held(&[u32::MAX]), u32::MAX), Vec::<(u32, u32)>::new());
    }

    #[test]
    fn a_name_is_the_parsers_name_and_unique_in_any_letter_case() {
        let taken: HashSet<RowKey> = [RowKey::name("Cost"), RowKey::name("Row_1")].into_iter().collect();
        assert!(check_name("Price", &taken).is_ok());
        assert!(check_name("_x9", &taken).is_ok());
        for bad in ["", "1st", "a b", "a-b", "a.b"] {
            assert!(check_name(bad, &taken).is_err(), "{bad:?}");
        }
        assert!(check_name("COST", &taken).unwrap_err().message.contains("already has a row Cost"));
        assert!(check_name(&"n".repeat(MAX_NAME_CHARS), &taken).is_ok());
        assert!(check_name(&"n".repeat(MAX_NAME_CHARS + 1), &taken).unwrap_err().message.contains("at most 256 characters"));
    }

    #[test]
    fn the_next_name_is_the_first_row_number_free_in_any_letter_case() {
        let names = |list: &[&str]| -> HashSet<RowKey> { list.iter().map(|n| RowKey::name(n)).collect() };
        assert_eq!(next_name(&names(&[])).unwrap(), "Row_1");
        assert_eq!(next_name(&names(&["Cost", "Row_1", "ROW_3"])).unwrap(), "Row_2");
        assert_eq!(next_name(&names(&["row_1", "Row_2"])).unwrap(), "Row_3");
    }

    #[test]
    fn a_row_number_is_the_ix_in_geometry_and_the_ix_plus_one_elsewhere() {
        assert_eq!(number_text(true, 4), "4");
        assert_eq!(number_text(false, 4), "5");
        assert_eq!(max_ix(true), u32::MAX);
        assert_eq!(max_ix(false), u32::MAX - 1);
        assert_eq!(number_text(false, u32::MAX - 1), u32::MAX.to_string());
    }

    #[test]
    fn a_geometry_row_holds_the_cells_its_type_uses() {
        let names = |ty: &str| geometry_cells(ty, None, None).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names("LineTo"), ["X", "Y"]);
        assert_eq!(names("ArcTo"), ["X", "Y", "A"]);
        assert_eq!(names("NURBSTo"), ["X", "Y", "A", "B", "C", "D", "E"]);
        for ty in GEOMETRY_ROW_TYPES {
            let cells = geometry_cells(ty, None, None);
            assert!(cells.len() >= 2, "{ty}");
        }
        assert!(begins_a_path("Ellipse") && !begins_a_path("LineTo"));
    }

    #[test]
    fn a_section_is_named_by_its_reference_its_label_or_its_name_in_a_file() {
        let entry = |t: &str| DISPLAY_ORDER.iter().find(|d| d.element != Some("Geometry") && named_by(d, &norm(t))).map(|d| d.label);
        assert_eq!(entry("Prop"), Some("Shape Data"));
        assert_eq!(entry("Property"), Some("Shape Data"));
        assert_eq!(entry("shape data"), Some("Shape Data"));
        assert_eq!(entry("Connections"), Some("Connection Points"));
        assert_eq!(entry("Connection Points"), Some("Connection Points"));
        assert_eq!(entry("SmartTags"), Some("Action Tag"));
        assert_eq!(entry("User"), Some("User-defined Cells"));
        assert_eq!(entry("User-defined Cells"), Some("User-defined Cells"));
        assert_eq!(entry("FillGradientStops"), Some("Fill Gradient"));
        assert_eq!(entry("Char"), Some("Character"));
        assert_eq!(entry("Shape Transform"), Some("Shape Transform"));
        assert_eq!(entry("Nonsense"), None);
    }
}
