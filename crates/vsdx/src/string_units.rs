//! The unit the text of a `User` or `Field` row is written with.
//!
//! **The `Value` of a `User` or a `Field` row that holds a text says `U="STR"`.** No `Type` cell in those rows says the value is a string, so the unit alone does: the program that opens the file takes
//! a text with none for a value of the wrong type, and says so once for each cell ("an element in this file contains data that is invalid or inappropriate for the expected type of data").
//! gave the unit to the cells a build makes (`diacraft_doc::string_unit`); the cells that **pass through** a writer as the drawing holds them did not have it:
//!
//! * the `Value` of the `User` rows of containers and swimlanes (`msvStructureType`, `DiaCraftContainerStyle`, `DiaCraftSwimlane`) in a `.diacraft` file of an earlier build, which a Save As to a
//!   VSDX type writes from the model;
//! * the cached text of the `Field` rows of a VSDX file an earlier build wrote (`<Cell N="Value" V="web-01" F="Inh"/>`), which a save into the file keeps as the file had it.
//!
//! **Which writers, and whose cells.** The writers that write from the model (a drawing never saved, and the model writer: a Save As from a `.diacraft` file or an `.odg`) give the unit to every such
//! cell **of a drawing whose units are DiaCraft's to state** ([`string_units`], `diacraft_doc::units_are_ours_to_state`): one that came from no VSDX package, or from one whose extended properties name
//! DiaCraft (a file an earlier build wrote, a template of DiaCraft's own). They do it in `write::write_new`, and so does the engine before it writes a drawing from the model (so that the check after the
//! save compares the file with the drawing it was written from, and the drawing becomes what the file reads as). **A drawing that came from the package of another program is that program's** (a
//! file whose extended properties name another program came out of the trip through `.diacraft` with `STR` on the text of a `User` row, a difference of the content in a file that was never ours): its cells are written as they were
//! read. **A save into a file does it only in a file DiaCraft wrote** ([`string_units_in_file`], asked when `write::names_diacraft` finds `docProps/app.xml` naming it, as every file a DiaCraft build writes
//! does): a cell another program wrote is kept as that program wrote it, an unedited save of its file writes the file's bytes, and a file that a save of ours has written is ours from then on.
//!
//! **What is given the unit** is what [`diacraft_doc::Cell::lacks_untyped_string_unit`] says: the `Value` of a `User` or `Field` row that reads as a text (a number, an empty value, a colour and a cell
//! that says a unit already are left as they are), **and not that of a `Field` row whose `Type` says it is no string**: a date field (`Type` 5) caches a date, and the files hold `U` `DATE` and a date
//! as text in it (gives it that where a date is read from the text or the drawing's times); a text no date is read from stays as it was read, and is no string. **No other cell is touched**: a `Property` row's `Value` is typed by its `Type` (104,655 string rows of the sample files say no unit),
//! the cells of a text kind carry no unit in the files of the program that opens them (`Prop.Format`, `User.Prompt`), and a language is the tag alone. A pass
//! over every text cell would have changed the content of the files of other programs that make the trip through the model (`.diacraft` and back) by the many thousands of cells that hold no unit
//! there. The drawing the caller keeps is not changed: this makes the drawing a writer is handed, and the file holds the unit.
//!
//! **The cost** is one read-only look at each sheet of the drawing (the document's, the styles', each master's and page's and each shape's), and a copy of the sheets that need it with what is on
//! the way to them: a drawing with nothing to give is not copied, and what the copy does not change it shares.

use diacraft_doc::{Document, Sheet};

use crate::page_follow::each_shape;
use crate::plain_languages::{Place, sheet_mut};

/// What [`string_units`] made: the drawing, and how many of its sheets were given the unit on a cell.
#[derive(Debug)]
pub struct Fitted {
    /// The drawing with the unit on every `Value` of a `User` or `Field` row that holds a text.
    pub doc: Document,
    /// The sheets that held a cell without the unit, and were given it.
    pub sheets: u64,
}

/// The drawing `doc` with the `Value` of every `User` and `Field` row that holds a text saying `STR` (see the [module](self)), **for a drawing whose units are DiaCraft's to state**
/// (`diacraft_doc::units_are_ours_to_state`: it came from no VSDX package, or from one that names DiaCraft), or `None` when every one does already (the common case, which copies nothing) and for
/// a drawing of the package of another program, whose cells are written as they were read. For the writers that write from the model.
#[must_use]
pub fn string_units(doc: &Document) -> Option<Fitted> {
    if !diacraft_doc::units_are_ours_to_state(doc) {
        return None;
    }
    string_units_in_file(doc)
}

/// [`string_units`] **for a save into a file DiaCraft wrote**, which judges whose the cells are by the file's own extended properties (`write::names_diacraft`) and not by the model, so the drawing is
/// fitted whatever it came from.
#[must_use]
pub fn string_units_in_file(doc: &Document) -> Option<Fitted> {
    let mut found: Vec<Place> = Vec::new();
    let mut look = |place: &dyn Fn() -> Place, sheet: &Sheet| {
        crate::work::string_sheet();
        if sheet.lacks_untyped_string_units() {
            found.push(place());
        }
    };
    look(&|| Place::Document, &doc.sheet);
    for (at, style) in doc.styles.iter().enumerate() {
        look(&|| Place::Style(at), &style.sheet);
    }
    for (at, master) in doc.masters.iter().enumerate() {
        look(&|| Place::Master(at), &master.sheet);
        each_shape(&master.shapes, |path, shape| look(&|| Place::MasterShape(at, path.to_vec()), &shape.sheet));
    }
    for (at, page) in doc.pages.iter().enumerate() {
        look(&|| Place::Page(at), &page.sheet);
        each_shape(&page.shapes, |path, shape| look(&|| Place::PageShape(at, path.to_vec()), &shape.sheet));
    }
    if found.is_empty() {
        return None;
    }
    let mut out = doc.clone();
    let mut sheets = 0u64;
    for place in &found {
        // A place the walk found is there in the copy (the copy is the drawing it walked); a sheet shared by two places is given the unit at each.
        if let Some(sheet) = sheet_mut(&mut out, place)
            && sheet.fit_untyped_string_units()
        {
            sheets = sheets.saturating_add(1);
            crate::work::string_sheet_fitted();
        }
    }
    Some(Fitted { doc: out, sheets })
}
