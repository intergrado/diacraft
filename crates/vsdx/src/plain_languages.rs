//! The language cells a VSDX file is written with.
//!
//! **A language is held as a plain tag**: `<Cell N="LangID" V="fr-FR"/>`, with no `F` and no `U`, in every cell the sample files hold (75,418 `Character` cells, and every `Shape Data` one that
//! holds a value; none holds a number). A cell that holds a **string formula** (`F="&quot;fr-FR&quot;"` with `U="STR"`, which is what typing `"fr-FR"` in the formula bar makes) gives a text
//! where the cell is a language: the program that opens the file reads that as an error and falls back to a language of its own, so the language is lost (it read `#VALUE!` and 8192 for
//! `Char.LangID="en-US"`, `Char.LangID[2]="fr-FR"` and `Char.LangID[4]="tr-TR"`). Every writer therefore gives such a cell the form the files hold: the tag the formula says, **with no formula
//! and no unit**. The drawing the caller keeps is not changed (as `page_names.rs` and `root.rs` leave it): this makes the drawing a writer is handed, and it is the file that holds the tag.
//!
//! * **The cells.** The cells `LangID` and `DocLangID` directly under a sheet (a shape's, a master's, a style's, a page's and the document's) and the `LangID` of the rows of a `Character` and a
//!   `Property` section. An `Annotation` row's `LangID` is a number of the legacy files and is left as it is.
//! * **What is rewritten.** A cell whose formula is a string of its own (`"fr-FR"`, parsed as a string and nothing more) is the tag alone. A unit `STR` on a language cell is taken off whatever
//!   formula it has, except where the formula is something else: **a formula that is no string** (it reads another cell, it calls a function) is the person's and is written as it is, with its unit.
//! * **A cell the file holds is not rewritten.** A save into a file (`baseline` is the drawing the file was read as) leaves a cell that the file holds in this very form as it is, so a save with
//!   no edit writes what it read, and an edit of another cell does not touch it. A cell the edit wrote, or one the file does not hold there, is written as the tag alone, and the patch takes the
//!   formula and the unit that the file's cell had out of the file.
//! * **A number is a tag's twin and is kept.** A cell that holds `1036` is no string formula: it is read as the language `fr-FR` (`diacraft_doc::lang`) and written back as the file had it.
//!
//! **The cost is a walk of the sheets and a write for each cell that needs one**, never a recalculation or a lookup that grows with the drawing: a sheet that is the very allocation the baseline
//! holds at its place is not looked at (the model shares what an edit did not copy), and a cell is written in place, so a drawing with nothing to do is not copied.
//!
//! The three writers ask [`plain_languages`] for the drawing they write (`write::write_new`, whose drawing the model writer in `held.rs` is handed, and a save into a file in `write::run`), and so
//! does the engine before it writes a drawing from the model, so that the check after the save compares the file with the drawing it was written from.

use std::sync::Arc;

use diacraft_doc::alt_text::STRING_UNIT;
use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, Lookup, MAX_SHAPE_DEPTH, SectionKey, Shape, ShapeList, Sheet, Value};
use diacraft_formula::Expr;

use crate::page_follow::{each_shape, each_shape_under};

/// The cells directly under a sheet that hold a language.
const DIRECT: [&str; 2] = ["LangID", "DocLangID"];

/// The sections whose rows hold a language in a cell called `LangID`.
const SECTIONS: [&str; 2] = ["Character", "Property"];

/// The cell as the files hold a language: the tag its string formula says, with no formula and no unit. `None` when it is held so already, or when its formula is something else than a
/// string (the person's own).
fn plain_form(cell: &Cell) -> Option<Cell> {
    let says_a_string = cell.unit.as_deref().is_some_and(|unit| unit.trim().eq_ignore_ascii_case(STRING_UNIT));
    let literal = match &cell.formula {
        FormulaSlot::Local(formula) => match &formula.ast {
            Ok(Expr::Str(text)) => Some(Arc::clone(text)),
            // A formula that is no string, or does not parse, is kept whatever unit it has.
            _ => return None,
        },
        _ => None,
    };
    if literal.is_none() && !says_a_string {
        return None;
    }
    let mut plain = cell.clone();
    if let Some(text) = literal {
        plain.formula = FormulaSlot::Absent;
        plain.value = Some(Value::from_text(&text));
        plain.error = None;
    }
    if says_a_string {
        plain.unit = None;
    }
    Some(plain)
}

/// Where a sheet is in a model, by position in the lists that hold it (a position is found in constant time and says the same in the drawing and in a clone of it).
#[derive(Clone, Debug)]
pub(crate) enum Place {
    Document,
    Style(usize),
    Master(usize),
    Page(usize),
    /// A shape of a master: the master's position and the shape's path.
    MasterShape(usize, Vec<u32>),
    /// A shape of a page: the page's position and the shape's path.
    PageShape(usize, Vec<u32>),
}

/// The cells of one sheet that are to be written, and where the sheet is.
struct Work {
    place: Place,
    cells: Vec<(CellAddr, Cell)>,
}

/// The shape at the end of `path` in `list`: one step for each place of the path, no deeper than the model allows.
fn shape_at<'l>(list: &'l ShapeList, path: &[u32]) -> Option<&'l Arc<Shape>> {
    if path.is_empty() || path.len() > MAX_SHAPE_DEPTH {
        return None;
    }
    let mut members = list;
    let mut found = None;
    for step in path {
        let shape = members.get(usize::try_from(*step).ok()?)?;
        members = &shape.children;
        found = Some(shape);
    }
    found
}

/// The language cells of `sheet` that are to be written, with their addresses, **leaving out a cell that `held` (the sheet of the baseline at the same place) holds as it is**.
fn plain_sheet(sheet: &Sheet, held: Option<&Sheet>) -> Vec<(CellAddr, Cell)> {
    let mut out: Vec<(CellAddr, Cell)> = Vec::new();
    let mut take = |addr: CellAddr, cell: &Cell, next: Cell| {
        let as_filed = held.is_some_and(|file| matches!(file.lookup(&addr), Lookup::Found(filed) if filed == cell));
        if !as_filed {
            out.push((addr, next));
        }
    };
    for name in DIRECT {
        if let Some(cell) = sheet.cell(name)
            && let Some(next) = plain_form(cell)
        {
            take(CellAddr::direct(name), cell, next);
        }
    }
    for section in sheet.sections.iter().filter(|section| !section.del && SECTIONS.contains(&section.name.as_str())) {
        // A hostile sheet may hold two sections of one key, and two rows of one: an address names the first of each, so a cell of the second is no cell this can write (it is left as it is).
        if !sheet.section(&SectionKey::new(section.name.as_str(), section.ix)).is_some_and(|first| Arc::ptr_eq(first, section)) {
            continue;
        }
        for row in section.rows.iter().filter(|row| !row.del) {
            let Some(cell) = row.cells.get("LangID") else { continue };
            let Some(next) = plain_form(cell) else { continue };
            let Some(key) = row.key(section.name.as_str()) else { continue };
            if !section.row(&key).is_some_and(|first| Arc::ptr_eq(first, row)) {
                continue;
            }
            take(CellAddr::in_row(section.name.as_str(), section.ix, key, "LangID"), cell, next);
        }
    }
    out
}

/// What a walk of a drawing finds: the sheets that have a cell to write.
struct Walk {
    found: Vec<Work>,
}

impl Walk {
    /// Looks at one sheet. `held` is the baseline's sheet at the same place, if the baseline has one.
    fn sheet(&mut self, place: &dyn Fn() -> Place, sheet: &Sheet, held: Option<&Sheet>) {
        crate::work::language_sheet();
        let cells = plain_sheet(sheet, held);
        if !cells.is_empty() {
            self.found.push(Work { place: place(), cells });
        }
    }

    /// The shapes of one master or page that differ from the baseline's, and their members.
    fn shapes(&mut self, list: &ShapeList, held: Option<&ShapeList>, place: &dyn Fn(Vec<u32>) -> Place) {
        let mut each = |path: &[u32], shape: &Shape| {
            let filed = held.and_then(|file| shape_at(file, path));
            self.sheet(&|| place(path.to_vec()), &shape.sheet, filed.map(|s| &*s.sheet));
        };
        let Some(held) = held else {
            each_shape(list, each);
            return;
        };
        for (at, shape) in list.iter().enumerate() {
            // A shape that is the very allocation the baseline holds there has nothing to write, and neither have its members.
            if held.get(at).is_some_and(|filed| Arc::ptr_eq(filed, shape)) {
                continue;
            }
            let top = [u32::try_from(at).unwrap_or(u32::MAX)];
            each(&top, shape);
            each_shape_under(&shape.children, &top, &mut each);
        }
    }
}

/// The sheet at `place` of `doc`, to change: what is on the way to it is copied and nothing else.
pub(crate) fn sheet_mut<'d>(doc: &'d mut Document, place: &Place) -> Option<&'d mut Sheet> {
    match place {
        Place::Document => Some(Arc::make_mut(&mut doc.sheet)),
        Place::Style(at) => Some(Arc::make_mut(&mut Arc::make_mut(doc.styles.get_mut(*at)?).sheet)),
        Place::Master(at) => Some(Arc::make_mut(&mut Arc::make_mut(doc.masters.get_mut(*at)?).sheet)),
        Place::Page(at) => Some(Arc::make_mut(&mut Arc::make_mut(doc.pages.get_mut(*at)?).sheet)),
        Place::MasterShape(at, path) => {
            let master = Arc::make_mut(doc.masters.get_mut(*at)?);
            Some(Arc::make_mut(&mut shape_mut(&mut master.shapes, path)?.sheet))
        }
        Place::PageShape(at, path) => {
            let page = Arc::make_mut(doc.pages.get_mut(*at)?);
            Some(Arc::make_mut(&mut shape_mut(&mut page.shapes, path)?.sheet))
        }
    }
}

/// The shape at the end of `path` in `list`, to change: one step for each place of the path (at most [`MAX_SHAPE_DEPTH`], so the recursion is as deep as the model is), copying each shape on the way.
fn shape_mut<'l>(list: &'l mut ShapeList, path: &[u32]) -> Option<&'l mut Shape> {
    if path.len() > MAX_SHAPE_DEPTH {
        return None;
    }
    let (first, rest) = path.split_first()?;
    let shape = Arc::make_mut(list.get_mut(usize::try_from(*first).ok()?)?);
    if rest.is_empty() { Some(shape) } else { shape_mut(&mut shape.children, rest) }
}

/// The drawing `doc` with every language cell written as the files hold one (see the [module](self)), or `None` when every one is held so already or is the person's own formula (the common
/// case, which copies nothing).
///
/// `baseline` is the drawing a file was read as, for a save into that file: a cell of `doc` that the baseline holds as it is stands as it is, whatever its form. With `None` (a drawing never saved,
/// a model written from the model alone) every cell is looked at.
pub fn plain_languages(doc: &Document, baseline: Option<&Document>) -> Option<Document> {
    let mut walk = Walk { found: Vec::new() };
    let file = baseline;
    if !file.is_some_and(|held| Arc::ptr_eq(&held.sheet, &doc.sheet)) {
        walk.sheet(&|| Place::Document, &doc.sheet, file.map(|held| &*held.sheet));
    }
    for (at, style) in doc.styles.iter().enumerate() {
        let held = file.and_then(|f| f.styles.get(at)).filter(|held| held.id == style.id);
        if held.is_some_and(|held| Arc::ptr_eq(held, style)) {
            continue;
        }
        walk.sheet(&|| Place::Style(at), &style.sheet, held.map(|held| &*held.sheet));
    }
    for (at, master) in doc.masters.iter().enumerate() {
        let held = file.and_then(|f| f.masters.get(at)).filter(|held| held.id == master.id);
        if held.is_some_and(|held| Arc::ptr_eq(held, master)) {
            continue;
        }
        walk.sheet(&|| Place::Master(at), &master.sheet, held.map(|held| &*held.sheet));
        walk.shapes(&master.shapes, held.map(|held| &held.shapes), &|path| Place::MasterShape(at, path));
    }
    for (at, page) in doc.pages.iter().enumerate() {
        let held = file.and_then(|f| f.pages.get(at)).filter(|held| held.id == page.id);
        if held.is_some_and(|held| Arc::ptr_eq(held, page)) {
            continue;
        }
        walk.sheet(&|| Place::Page(at), &page.sheet, held.map(|held| &*held.sheet));
        walk.shapes(&page.shapes, held.map(|held| &held.shapes), &|path| Place::PageShape(at, path));
    }
    if walk.found.is_empty() {
        return None;
    }
    let mut out = doc.clone();
    for Work { place, cells } in walk.found {
        let Some(sheet) = sheet_mut(&mut out, &place) else { continue };
        for (addr, cell) in cells {
            // A cell the sheet holds is replaced in place (the walk looked only at cells of rows and sections that are not deleted), so nothing is added and no limit is reached.
            let _ = sheet.set_cell(&addr, cell);
            crate::work::language_cell();
        }
    }
    Some(out)
}
