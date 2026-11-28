//! The ShapeSheet's side of the engine: `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate`, the queries of a sheet. They read the model through
//! [`crate::sheetview`] and write nothing, so a call is no undo step and is not journaled. The design is the ShapeSheet window's (sections 7.1 to 7.3,
//! 7.7 and 8) and the rules are named in `docs/editing-commands.md`, "The ShapeSheet (SHEETWIN)": the sheet, the cell, which sections, in which order,
//! the lines, the texts of a value.
//!
//! **Adds the writes**: `sheet.set_cell` and `sheet.set_cells` set any cell of any sheet the drawing holds, one undo step each and all or nothing, with their rules in
//! [`crate::draw::sheet`] and the locked-layer rule in
//! `cmd/locked.rs`.

use std::collections::HashSet;

use diacraft_calc::{EvalOptions, FileInfo, cell_address, clock_at};
use diacraft_doc::{CellAddr, DISPLAY_ORDER, Document, Lookup, Resolver, SectionKind, SheetKind, SheetRef, Source, cell_info, section_spelling};
use serde_json::{Map, Value, json};

use super::{CommandSpec, has_document, required_text};
use crate::Session;
use crate::draw::sheet::{self as writing, Recalculated, Write, WriteResult};
use crate::error::{Result, bad, failed};
use crate::sheetview::{
    self, Counts, Ctx, MAX_LINES_PER_READ, MAX_SECTION_NAMES, Options, Outline, Place, SectionInfo, Target, ViewError, cut_text, default_of,
    kind_word, read_count, read_lines,
};

/// The most bytes of a formula a call may give (no limit is published, and a file's formula may be up to 1 MiB and is kept).
const MAX_TEXT_BYTES: usize = 65_536;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "sheet.read",
            "Read ShapeSheet",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet is read (with page: on that page, else on the first page that holds it, a group's member too, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Answers {sheet, name, sections, lines_total, from, lines, more}: sheet names the sheet back as {kind, page, master, style, id} with the keys that apply, name is its title (Process.4, Page-1), \
             and sections lists what the sheet shows in the window's order, each {section, label, ix, kind, first_line, lines, rows, columns, can_add_rows, can_delete}, where section is the spelling a reference uses \
             (Geometry2, Prop, User) for a section of rows and the label for a group of cells (Shape Transform), kind is cells or rows, and columns are the cell names of a section of rows. \
             A line is a header, a cell, the column titles, a row or a note, each {line, section, kind, row, cells}, a row {key, label, type, origin} and a cell {ref, name, col, formula, value, unit, shown, error, origin, from, guarded, parsed, worked_out, truncated}: \
             ref is the reference a formula names the cell by (send it back as cell), formula is the text the file holds or the value as a literal, value is the cached value in its unit, shown the text of the Values view (four decimals), \
             origin local, inherited or default and from the master or style an inherited cell comes from, and a text longer than 4096 characters is cut with truncated true (sheet.cell gives it whole). \
             Nothing is worked out and nothing is written; \
             page: whole number, the page, whose sheet is read when it is alone; \
             master: whole number, the master, whose page sheet is read when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is read; \
             document: true, the document's sheet is read; \
             sections: array of text, optional, only the sections with these names (the section or the label of an answer, in any letter case, spaces ignored, at most 256), the lines then count over these only; \
             from: whole number, optional (default 0), the first line to give, counted over the sections read; \
             count: whole number, optional (default 200), from 1 to 2000, the most lines to give (the answer is cut at 8 MiB of text, and more says there are lines after it); \
             all: true or false, optional (default false), also the groups of cells the rules of the window leave out for this kind of sheet (1-D Endpoints on a box, Group Properties on a plain shape)",
            has_document,
            read
        ),
        cmd!(
            query "sheet.cell",
            "Read a ShapeSheet cell",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet holds the cell (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Answers the cell whole, as sheet.read gives a cell but with no text cut: {sheet, ref, name, formula, value, unit, shown, error, origin, from, guarded, parsed, worked_out, truncated, kind, class, listed, default, chain}, \
             where kind is what the cell holds (scalar, length, angle, days, color, text, font or point) and class which sheets it is looked for on (shape-only, style-any, fill-family, line-family, text-family, page-sheet, style-flag, style-local or unlisted), \
             listed whether the catalogue lists it, default {value, shown, formula} what it reads when found nowhere, and chain the sheets it is looked for on, nearest first, each {source, id, name, holds}. \
             A cell that is not in the sheet reads its default; \
             page: whole number, the page, whose sheet is read when it is alone; \
             master: whole number, the master, whose page sheet is read when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is read; \
             document: true, the document's sheet is read; \
             cell: text, the reference a formula names the cell by with no sheet before it: Width, Geometry1.X2, Scratch.A1, Prop.Cost, Prop.Cost.Label, User.Total.Prompt (a cell directly under the sheet that the catalogue lists is read in any letter case, so WIDTH is Width. A name that is no reference, a leading = and a sheet such as Sheet.4! are refused in words)",
            has_document,
            cell
        ),
        cmd!(
            query "sheet.sections",
            "ShapeSheet sections",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sections are listed (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Answers {sheet, name, present, can_add, can_delete}: what Insert Section and Delete Section offer. present is every section the sheet shows, each {section, label, ix, kind}, can_add the sections that can be inserted on this kind of sheet and that the sheet \
             has not (a Geometry section can be added again, as Ellipse or InfiniteLine too, named in variant), each {section, label, variant}, and can_delete the sections that can be deleted, each {section, label, ix} (Shape Transform never); \
             page: whole number, the page, whose sheet is read when it is alone; \
             master: whole number, the master, whose page sheet is read when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is read; \
             document: true, the document's sheet is read",
            has_document,
            sections
        ),
        cmd!(
            query "sheet.evaluate",
            "Try a ShapeSheet formula",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet the formula is worked out on (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Answers {sheet, value, unit, shown, error, parse_error, budget_exhausted, worked_out, steps} and writes nothing: value the number, text or true or false the formula works out to (in unit), shown its text as the Values view shows it, error the error it works out to (#DIV/0!), \
             parse_error {kind, offset, words} when the text does not parse (offset is the byte in the text as given), budget_exhausted when the work passed 200000 steps of the evaluator, and worked_out false for a function the evaluator does not have; \
             page: whole number, the page, whose sheet is read when it is alone; \
             master: whole number, the master, whose page sheet is read when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is read; \
             document: true, the document's sheet is read; \
             formula: text, the formula to try, at most 65536 bytes, cut of white space at its ends and of one leading = as a write cuts it; \
             cell: text, optional, the reference of the cell the formula is for, so that a bare number takes the unit the cell holds (a text that names the cell is a loop, as its write would make it)",
            has_document,
            evaluate
        ),
        cmd!(
            "sheet.set_cell",
            "Set a ShapeSheet cell",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet holds the cell (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Writes the cell's formula as the window's formula bar does, and works out again what reads the cell and the connectors it moved, in one undo step (none when the cell already holds the text). \
             Answers {sheet, cell, changed, formula, value, unit, shown, error, origin, worked_out, recalculated}: the cell as sheet.cell gives it after the write, and recalculated {cells, listed, more} the cells that changed besides this one \
             (listed holds at most 256 of them, each {sheet, cell}, and more says there are others). \
             The text is cut of white space at its ends and of one leading =, at most 65536 bytes. An empty text clears the cell's own formula (the cell inherits again, or reads No Formula where nothing is inherited). \
             A text that does not parse is refused with the kind of fault, its byte and words. A number alone (3, 3 in, -0.25 mm, 25%) is a value with no formula, and a bare number is read in the unit the cell shows (3 in a cell showing 50 mm is 3 mm). \
             In a cell that expects text, what is typed without a leading = is a string. A cell must be there: one the sheet holds or inherits, or the catalogue lists for this kind of sheet (a cell of a row that is not there is refused: add the row first). \
             A guarded cell (GUARD) is refused unless force is true. A shape on a locked layer is refused; \
             page: whole number, the page, whose sheet is written when it is alone; \
             master: whole number, the master, whose page sheet is written when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is written; \
             document: true, the document's sheet is written; \
             cell: text, the reference a formula names the cell by with no sheet before it: Width, Geometry1.X2, Scratch.A1, Prop.Cost.Value, User.Total.Prompt (a row's name matches in any letter case, and so does a cell directly under the sheet that the catalogue lists: WIDTH is Width); \
             formula: text, the formula typed, or an empty text to clear the cell's own formula; \
             force: true or false, optional (default false), true replaces a formula that GUARD protects",
            has_document,
            set_cell
        ),
        cmd!(
            "sheet.set_cells",
            "Set ShapeSheet cells",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, optional, the shape whose sheet the writes that name no sheet of their own use (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Writes up to 4096 cells in one undo step, all or nothing, and works out again each page it touched once, at the end. A write that is refused refuses the call and says which write it was (write 2: ...). \
             At most 4194304 bytes of formula text and 262144 parts of formulas in all. \
             Answers {written, unchanged, results, recalculated}: written the writes that changed a cell, unchanged those that left it as it was, results the first 256 writes' answers, each {sheet, cell, changed, formula, value, unit, shown, error, origin, worked_out}, \
             and recalculated {cells, listed, more} as sheet.set_cell gives it. Each text is read as sheet.set_cell reads it; \
             page: whole number, optional, the page whose sheet the writes that name none use when it is alone; \
             master: whole number, optional, the master whose page sheet is used when it is alone, and with id the master's shape; \
             style: whole number, optional, the style whose sheet is used; \
             document: true, optional, the document's sheet is used; \
             writes: array of objects {cell, formula, force}, from 1 to 4096, each a write as sheet.set_cell takes it: cell is the reference and formula the text, force is optional, \
             and a write that gives any of id, page, master, style or document names its own sheet whole and does not take the call's",
            has_document,
            set_cells
        ),
    ]
}

/// The most writes of a batch the registry's lock rule reads (the command refuses a call with more).
pub(crate) const MAX_WRITE_ITEMS: usize = writing::MAX_WRITES + 1;

/// The five keys that name a sheet.
const SHEET_KEYS: [&str; 5] = ["id", "page", "master", "style", "document"];

/// The keys a write of `sheet.set_cells` takes.
const WRITE_KEYS: [&str; 8] = ["cell", "formula", "force", "id", "page", "master", "style", "document"];

/// The sheet keys the write `write` of a batch names: its own, when it gives any of the five (a `null` gives none), else the call's. An object of only those keys.
pub(crate) fn sheet_keys_of(call: &Value, write: &Map<String, Value>) -> Value {
    let own: Map<String, Value> =
        SHEET_KEYS.iter().filter_map(|key| write.get(*key).filter(|v| !v.is_null()).map(|v| ((*key).to_owned(), v.clone()))).collect();
    if !own.is_empty() {
        return Value::Object(own);
    }
    Value::Object(SHEET_KEYS.iter().filter_map(|key| call.get(*key).filter(|v| !v.is_null()).map(|v| ((*key).to_owned(), v.clone()))).collect())
}

/// The words of a refusal, naming the write of a batch (`at`, from 1) when there is one.
fn said(cmd: &str, at: Option<usize>, words: String) -> crate::error::EngineError {
    bad(cmd, at.map_or_else(|| words.clone(), |n| format!("write {n}: {words}")))
}

/// The cell a text names, as `sheet.set_cell` reads it.
fn cell_of(value: Option<&Value>, cmd: &str, at: Option<usize>) -> Result<CellAddr> {
    match value {
        None | Some(Value::Null) => Err(said(cmd, at, "missing `cell`".to_owned())),
        Some(Value::String(text)) => cell_address(text).map_err(|e| said(cmd, at, format!("`cell`: {e}"))),
        Some(_) => Err(said(cmd, at, "`cell` must be text".to_owned())),
    }
}

/// The text a write gives for `formula`, which it must give.
fn formula_of<'v>(value: Option<&'v Value>, cmd: &str, at: Option<usize>) -> Result<&'v str> {
    match value {
        None | Some(Value::Null) => Err(said(cmd, at, "missing `formula`".to_owned())),
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(said(cmd, at, "`formula` must be text".to_owned())),
    }
}

/// `force` of a write: true or false, false when left out.
fn force_of(value: Option<&Value>, cmd: &str, at: Option<usize>) -> Result<bool> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(on)) => Ok(*on),
        Some(_) => Err(said(cmd, at, "`force` must be true or false".to_owned())),
    }
}

/// A write as its answer gives it: the cell as the sheet shows it after the call.
fn write_json(r: &WriteResult) -> Value {
    json!({
        "sheet": r.place.json(),
        "cell": r.cell.reference,
        "changed": r.changed,
        "formula": r.cell.formula,
        "value": r.cell.value.json(),
        "unit": r.cell.unit,
        "shown": r.cell.shown,
        "error": r.cell.error,
        "origin": r.cell.origin.word(),
        "worked_out": r.cell.worked_out,
    })
}

fn recalculated_json(r: &Recalculated) -> Value {
    let listed: Vec<Value> = r.listed.iter().map(|t| json!({"sheet": t.place.json(), "cell": t.cell})).collect();
    json!({"cells": r.cells, "listed": listed, "more": r.more})
}

/// `sheet.set_cell`.
fn set_cell(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.set_cell";
    let target = Target::from_params(p).map_err(|words| bad(ID, words))?;
    let addr = cell_of(p.get("cell"), ID, None)?;
    let text = formula_of(p.get("formula"), ID, None)?;
    let force = force_of(p.get("force"), ID, None)?;
    if text.len() > writing::MAX_FORMULA_BYTES {
        return Err(bad(ID, format!("a formula is at most {} bytes long, and this one has {}", writing::MAX_FORMULA_BYTES, text.len())));
    }
    let label = if cut_text(text).0.trim().is_empty() { "Clear formula" } else { "Edit formula" };
    let write = Write { target, addr, text: text.to_owned(), force };
    let done = s.edit_if(label, |doc| {
        let done = writing::apply(doc, std::slice::from_ref(&write), false).map_err(|problem| problem.into_engine(ID))?;
        let changed = done.changed;
        Ok((done, changed))
    })?;
    let first = done.results.first().ok_or_else(|| failed(ID, "the cell is not on this sheet after the write"))?;
    let mut answer = write_json(first);
    if let Value::Object(map) = &mut answer {
        map.insert("changed".into(), json!(done.changed));
        map.insert("recalculated".into(), recalculated_json(&done.recalculated));
    }
    Ok(answer)
}

/// `sheet.set_cells`.
fn set_cells(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.set_cells";
    let items = match p.get("writes") {
        None | Some(Value::Null) => return Err(bad(ID, "missing `writes`")),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(bad(ID, "`writes` must be a list of writes")),
    };
    if items.len() > writing::MAX_WRITES {
        return Err(bad(ID, format!("`writes` holds at most {} writes", writing::MAX_WRITES)));
    }
    if items.is_empty() {
        return Err(bad(ID, "`writes` must hold at least one write"));
    }
    let mut writes: Vec<Write> = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let n = at + 1;
        let Value::Object(map) = item else {
            return Err(bad(ID, format!("write {n} must be an object")));
        };
        if let Some(unknown) = map.keys().find(|k| !WRITE_KEYS.contains(&k.as_str())) {
            let words = format!(
                "write {n} has an unknown key `{}`; a write takes cell, formula, force, id, page, master, style and document",
                crate::error::shown(unknown)
            );
            return Err(bad(ID, words));
        }
        let target = Target::from_params(&sheet_keys_of(p, map)).map_err(|words| bad(ID, format!("write {n}: {words}")))?;
        let addr = cell_of(map.get("cell"), ID, Some(n))?;
        let text = formula_of(map.get("formula"), ID, Some(n))?;
        let force = force_of(map.get("force"), ID, Some(n))?;
        writes.push(Write { target, addr, text: text.to_owned(), force });
    }
    let done = s.edit_if("Edit formulas", |doc| {
        let done = writing::apply(doc, &writes, true).map_err(|problem| problem.into_engine(ID))?;
        let changed = done.changed;
        Ok((done, changed))
    })?;
    Ok(json!({
        "written": done.written,
        "unchanged": done.unchanged,
        "results": done.results.iter().map(write_json).collect::<Vec<_>>(),
        "recalculated": recalculated_json(&done.recalculated),
    }))
}

/// The sheet a call names, with the document: the words of a refusal when it names none.
fn target<'d>(s: &'d Session, p: &Value, cmd: &str) -> Result<(&'d Document, Place, SheetRef)> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let place = Place::from_params(doc, p).map_err(|words| bad(cmd, words))?;
    let sheet = place.sheet(doc).ok_or_else(|| failed(cmd, "the sheet is not in the drawing"))?;
    Ok((doc, place, sheet))
}

/// A cell reference the call gives for `key`, read as a formula reads it, with a named row spelled as the sheet spells it (a row's name matches in any letter case).
fn reference(res: &Resolver<'_>, sheet: &SheetRef, p: &Value, key: &str, cmd: &str) -> Result<Option<CellAddr>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            cell_address(text).map(|addr| Some(sheetview::respell(res, sheet, addr))).map_err(|e| bad(cmd, format!("`{key}`: {e}")))
        }
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

/// A whole number `p` gives for `key` that is at most `u32::MAX`.
fn whole(p: &Value, key: &str, cmd: &str) -> Result<Option<u64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if n <= u64::from(u32::MAX) => Ok(Some(n)),
            _ => Err(bad(cmd, format!("`{key}` must be a whole number from 0 to {}", u32::MAX))),
        },
    }
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(on)) => Ok(*on),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The sections a call asks for by name.
fn names_asked(p: &Value, cmd: &str) -> Result<Option<Vec<String>>> {
    match p.get("sections") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            if items.len() > MAX_SECTION_NAMES {
                return Err(bad(cmd, format!("`sections` names at most {MAX_SECTION_NAMES} sections")));
            }
            let names: Option<Vec<String>> = items.iter().map(|v| v.as_str().map(str::to_owned)).collect();
            let names = names.ok_or_else(|| bad(cmd, "`sections` must be a list of section names"))?;
            Ok((!names.is_empty()).then_some(names))
        }
        Some(_) => Err(bad(cmd, "`sections` must be a list of section names")),
    }
}

/// A section of an outline as `sheet.read` lists it.
fn section_json(section: &SectionInfo) -> Value {
    let rows = section.key.is_some();
    let mut out = json!({
        "section": section.name,
        "label": section.label,
        "ix": section.ix,
        "kind": if rows { "rows" } else { "cells" },
        "first_line": section.first_line,
        "lines": section.lines,
        "rows": rows.then_some(section.rows_total),
        "columns": rows.then(|| section.columns.clone()),
        "can_add_rows": section.can_add_rows,
        "can_delete": section.can_delete,
    });
    if let (Some(map), true) = (out.as_object_mut(), section.unlisted > 0) {
        map.insert("unlisted".into(), json!(section.unlisted));
    }
    out
}

/// `sheet.read`.
fn read(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.read";
    let (doc, place, sheet) = target(s, p, ID)?;
    let from = whole(p, "from", ID)?.unwrap_or(0);
    let count = match p.get("count") {
        None | Some(Value::Null) => read_count(None),
        Some(v) => v.as_u64().and_then(|n| read_count(Some(n))),
    }
    .ok_or_else(|| bad(ID, format!("`count` must be a whole number from 1 to {MAX_LINES_PER_READ}")))?;
    let options = Options { all: flag(p, "all", ID)?, only: names_asked(p, ID)?, ..Options::default() };
    let res = Resolver::new(doc);
    let outline = Outline::build(&res, &sheet, &options).map_err(|e| match e {
        ViewError::NoSheet => failed(ID, e.to_string()),
        ViewError::NoSection { .. } => bad(ID, e.to_string()),
    })?;
    let (lines, _) = read_lines(&res, &sheet, &outline, from, count);
    let listed: Vec<Value> =
        lines.lines.iter().map(|line| line.json(outline.sections.get(line.section).map_or("", |sec| sec.name.as_str()))).collect();
    let more = from.saturating_add(listed.len() as u64) < outline.total_lines;
    Ok(json!({
        "sheet": place.json(),
        "name": place.title(doc),
        "sections": outline.sections.iter().map(section_json).collect::<Vec<_>>(),
        "lines_total": outline.total_lines,
        "from": from,
        "lines": listed,
        "more": more,
    }))
}

/// A layer of a cell's chain as an answer lists it.
fn layer_json(doc: &Document, source: &Source, holds: bool) -> Value {
    let named = |name: Option<&std::sync::Arc<str>>| name.map(|n| n.to_string());
    match source {
        Source::Local => json!({"source": "local", "holds": holds}),
        Source::Master(at) => {
            let id = match at.owner {
                diacraft_doc::Owner::Master(id) | diacraft_doc::Owner::Page(id) => id,
            };
            let name = named(doc.master(id).and_then(|m| m.name.as_ref().or(m.name_u.as_ref())));
            json!({"source": "master", "id": id, "name": name, "shape": doc.shape(at).map(|shape| shape.id), "holds": holds})
        }
        Source::Style { id, family } => {
            let name = named(doc.style(*id).and_then(|st| st.name.as_ref().or(st.name_u.as_ref())));
            let family = family.map(|f| match f {
                diacraft_doc::Family::Line => "line",
                diacraft_doc::Family::Fill => "fill",
                diacraft_doc::Family::Text => "text",
            });
            json!({"source": "style", "id": id, "name": name, "family": family, "holds": holds})
        }
        Source::Default => json!({"source": "default", "holds": false}),
    }
}

/// `sheet.cell`.
fn cell(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.cell";
    let (doc, place, sheet) = target(s, p, ID)?;
    let res = Resolver::new(doc);
    let addr = reference(&res, &sheet, p, "cell", ID)?.ok_or_else(|| bad(ID, "missing `cell`"))?;
    let mut eval = sheetview::read_eval_ctx();
    let ctx = Ctx::new(&res, &sheet, &mut eval);
    let mut counts = Counts::default();
    let name = addr.cell.as_str().to_owned();
    let view = sheetview::view(&ctx, &addr, &name, None, None, &mut counts)
        .ok_or_else(|| failed(ID, "the cell is not on this sheet: the row or the section it is in is deleted here"))?;
    let info = cell_info(addr.section.as_ref().map(|k| k.name.as_str()), &name);
    let default = default_of(info.kind);
    let chain: Vec<Value> = res
        .lookup_layers(&sheet, info.class)
        .unwrap_or_default()
        .iter()
        .map(|(source, layer)| layer_json(doc, source, matches!(res.lookup(layer, &addr), Lookup::Found(_))))
        .collect();
    let mut out = match view.json() {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("cell".into(), other);
            map
        }
    };
    out.insert("sheet".into(), place.json());
    out.insert("kind".into(), json!(kind_word(info.kind)));
    out.insert("class".into(), json!(sheetview::class_word(info.class)));
    out.insert("listed".into(), json!(info.listed));
    out.insert("default".into(), json!({"value": default.value.json(), "shown": default.shown, "formula": info.default_formula}));
    out.insert("chain".into(), Value::Array(chain));
    Ok(Value::Object(out))
}

/// The entries `can_add` lists: the label an Insert Section card shows, the name a command gives it, and the variant of a Geometry section.
struct Offer {
    section: String,
    label: String,
    variant: Option<&'static str>,
}

/// `sheet.sections`.
fn sections(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.sections";
    let (doc, place, sheet) = target(s, p, ID)?;
    let res = Resolver::new(doc);
    let outline = Outline::build(&res, &sheet, &Options::default()).map_err(|e| failed(ID, e.to_string()))?;
    let kind: SheetKind = place.kind();
    // The sections of rows the sheet has, shown or not: one that is there already is not offered again (a Geometry section may be added again).
    let has: HashSet<String> = res.effective_section_names(&sheet).unwrap_or_default().iter().map(|n| n.as_str().to_owned()).collect();
    let shown_here: HashSet<&str> = outline.sections.iter().map(|sec| sec.label.as_str()).collect();
    let mut can_add: Vec<Offer> = Vec::new();
    for d in DISPLAY_ORDER.iter().filter(|d| d.rules.can_insert_on(kind)) {
        let section = d.element.and_then(section_spelling).map_or_else(|| d.label.to_owned(), str::to_owned);
        match (d.kind, d.element) {
            (SectionKind::Rows, Some("Geometry")) => {
                can_add.push(Offer { section: "Geometry".to_owned(), label: "Geometry".to_owned(), variant: None });
                can_add.push(Offer { section: "Geometry".to_owned(), label: "Ellipse".to_owned(), variant: Some("Ellipse") });
                can_add.push(Offer { section: "Geometry".to_owned(), label: "Infinite Line".to_owned(), variant: Some("InfiniteLine") });
            }
            (SectionKind::Rows, element) => {
                if !element.is_some_and(|e| has.contains(e)) {
                    can_add.push(Offer { section, label: d.label.to_owned(), variant: None });
                }
            }
            // A group inserted whole (Text Transform, Layer Membership) is there when the sheet shows it.
            (SectionKind::Cells, _) => {
                if !shown_here.contains(d.label) {
                    can_add.push(Offer { section, label: d.label.to_owned(), variant: None });
                }
            }
        }
    }
    let present: Vec<Value> = outline
        .sections
        .iter()
        .map(|sec| json!({"section": sec.name, "label": sec.label, "ix": sec.ix, "kind": if sec.key.is_some() { "rows" } else { "cells" }}))
        .collect();
    let can_delete: Vec<Value> =
        outline.sections.iter().filter(|sec| sec.can_delete).map(|sec| json!({"section": sec.name, "label": sec.label, "ix": sec.ix})).collect();
    Ok(json!({
        "sheet": place.json(),
        "name": place.title(doc),
        "present": present,
        "can_add": can_add.iter().map(|o| json!({"section": o.section, "label": o.label, "variant": o.variant})).collect::<Vec<_>>(),
        "can_delete": can_delete,
    }))
}

/// The options of an evaluation the session makes: its clock and its file, and the faces text is measured in.
fn options_of(s: &Session) -> EvalOptions {
    let clock = s.now().map_or_else(Default::default, |n| clock_at(n, 0));
    let (name, directory) = s
        .source()
        .and_then(|f| {
            let path = f.path();
            Some((path.file_name()?.to_str()?.to_owned(), path.parent()?.to_str()?.to_owned()))
        })
        .unzip();
    EvalOptions { clock, file: FileInfo { name, directory }, ..crate::measure::eval_options() }
}

/// `sheet.evaluate`.
fn evaluate(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.evaluate";
    let (doc, place, sheet) = target(s, p, ID)?;
    let text = required_text(p, "formula", ID)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(bad(ID, format!("a formula is at most {MAX_TEXT_BYTES} bytes long, and this one has {}", text.len())));
    }
    let at = reference(&Resolver::new(doc), &sheet, p, "cell", ID)?;
    let dry = sheetview::evaluate(doc, &sheet, at.as_ref(), text, &options_of(s));
    Ok(json!({
        "sheet": place.json(),
        "value": dry.typed.value.json(),
        "unit": dry.typed.unit,
        "shown": dry.typed.shown,
        "error": dry.error,
        "parse_error": dry.parse_error.as_ref().map(sheetview::Fault::json),
        "budget_exhausted": dry.budget_exhausted,
        "worked_out": dry.worked_out,
        "steps": dry.steps,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Every key of a command's params doc is its own entry, and the sheet's five are on each (design 7.1).
    #[test]
    fn the_params_docs_list_the_five_sheet_keys_and_each_command_its_own() {
        for spec in specs() {
            let keys = super::super::param_keys(spec.params).unwrap();
            for key in ["id", "page", "master", "style", "document"] {
                assert!(keys.contains(&key), "{} takes {key}", spec.id);
            }
            assert_eq!(spec.journal, spec.id.starts_with("sheet.set_"), "{}: a read is no journal entry and a write is one", spec.id);
        }
        let read = super::super::param_keys(specs()[0].params).unwrap();
        assert_eq!(&read[5..], ["sections", "from", "count", "all"]);
    }

    #[test]
    fn a_class_has_a_word_and_a_kind_has_a_word() {
        assert_eq!(sheetview::class_word(diacraft_doc::CellClass::ShapeOnly), "shape-only");
        assert_eq!(kind_word(diacraft_doc::ValueKind::Length), "length");
    }
}
