//! Fields in a shape's text: Insert, Text, Field.
//!
//! **What is kept.** A field is a `fld` of the shape's text (`TextPart::Fld`: the row's `IX` and the text it shows) and the row of that `IX` in the shape's `Field` section, with
//! `Value` (a formula, and the value it gave), `Format` (the picture that turns the value into text) and `Type` (0 text, 2 number, 5 date, 6 duration, 7 currency). **What it shows is the
//! value through the format** (`calc::field_text`), which is what the file stores in the `fld` as well. The `.odg` writer says `PAGENUMBER()`, `PAGECOUNT()`, `FILENAME()` and
//! `NOW()` as the fields of that format and any other field as the text it shows, with a save note.
//!
//! **The categories** are, each field the function maps it to: Custom Formula, Date/Time (`DOCCREATION()`, `NOW()`,
//! `DOCLASTEDIT()`, `DOCLASTPRINT()`), Document Info, Geometry (`Height`, `Width`, `Angle`), Object Info, Page Info, Shape Data (`Prop.<row>`) and User-defined Cells (`User.<row>`).
//!
//! **Project rules** (ours; the specs leave them open, and the check list lists them):
//!
//! * A new field is the row whose `IX` is one more than the highest of the shape's `Field` rows and its `fld`s, from 0. A shape holds at most [`MAX_FIELDS`] fields.
//! * A field is made with the format its kind has by default, which the Data Format list can change: a date or a time `{<211>}` (the date and the time, `M/d/yyyy h:mm am/pm`),
//!   a length `esc(7)` (two places and the unit), an angle `esc(12)` (degrees), a shape data field the format its row has (`Prop.<row>.Format`), and any other none (a text as it is, a number in
//!   plain digits). The format is the text of `FIELDPICTURE`'s answer, so a field made in the dialog and one made by a program say the same.
//! * The Object Info `Type` field is `TYPEDESC()`: the specs do not say which of `TYPE` and `TYPEDESC` the field uses, and the text is what a person reads.
//! * The clock is the session's (`Session::now`), **in UTC**: the build reads no time zone. A drawing that has no creation or last edit date yet (a new one) shows the time it
//!   is made at, and a print date it has not got shows nothing. `FILENAME()` and `DIRECTORY()` are the session's file, empty for a drawing never saved.
//! * A field **updates by itself** when what it reads changes through an edit that moves or sizes its shape, and when a page is added, renamed, taken away,
//!   copied or moved: the text shown is worked out again from the row. A field that reads the clock or the file's name is worked out again by `text.update_fields`, which is also the way to
//!   bring every field up to date after a file is opened (a file's own text is kept as it was saved until then). **A field on a background page that reads the page** (`PAGENUMBER`,
//!   `PAGENAME`: an OpenDocument master page's page number among them) shows the page that uses the background, so no refresh works it out to the background page's own
//!   answer: it keeps the text it has.
//!
//! **Hostile input.** A call counts at most [`MAX_FIELDS_PER_CALL`] fields ([`MAX_AUTO_FIELDS`] for an edit's own refresh) before it works any out, skips a shape of more than [`MAX_FIELDS`] and says how many it skipped, works within
//! one evaluation budget for all of them, and walks at most [`MAX_WALK`] shapes; the work follows the number of fields (see [`refresh`]).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, FileInfo, Value as Calc, field_text};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Document, FormulaSlot, Owner, Resolver, RowKey, SectionKey, ShapeList, ShapeRef, SheetRef, Text, TextPart,
    Value, WriteOrigin,
};

use super::{Problem, require_page, require_shape, shape_mut, write_cells};

/// The most fields one shape holds.
pub const MAX_FIELDS: usize = 1_024;
/// The most fields one call of an update counts: every field of every shape counts, those of a shape that is skipped too.
pub const MAX_FIELDS_PER_CALL: usize = 100_000;
/// The most fields an edit's own refresh counts.
pub const MAX_AUTO_FIELDS: usize = 10_000;
/// The most shapes one walk of the pages looks at.
pub const MAX_WALK: usize = 2_000_000;
/// The most shapes the walk that follows an edit looks at (past it the rest are left to `text.update_fields`).
pub const MAX_AUTO_WALK: usize = 100_000;
/// The most shapes an edit's own refresh looks at.
pub const MAX_AUTO_SHAPES: usize = 256;
/// The most steps of the evaluator one call may take for the fields it works out.
pub const MAX_STEPS: u64 = 5_000_000;
/// The longest a custom formula or a format is, in characters (the picture language's own cap).
pub const MAX_TEXT_CHARS: usize = 4_096;
/// The most rows of a section listed for the dialog.
const MAX_ROWS_LISTED: usize = 4_096;

/// A field the dialog lists: its id, its label, its formula, its kind and its default format.
pub type Listed = (String, String, String, Kind, String);

/// The new texts of the fields of one shape: the page, the shape and the (`IX`, text) pairs.
type TextChanges = (u32, ShapeRef, Vec<(u32, String)>);

/// What kind of value a field holds: the `Type` cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    Date,
    Duration,
    Currency,
}

impl Kind {
    /// The `Type` cell's number.
    pub fn type_cell(self) -> u8 {
        match self {
            Kind::Text => 0,
            Kind::Number => 2,
            Kind::Date => 5,
            Kind::Duration => 6,
            Kind::Currency => 7,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Number => "number",
            Kind::Date => "date",
            Kind::Duration => "duration",
            Kind::Currency => "currency",
        }
    }

    /// The kind a value is of.
    fn of_value(value: &Calc) -> Kind {
        match value {
            Calc::Number(q) if q.unit == diacraft_units::Unit::Date => Kind::Date,
            Calc::Number(_) => Kind::Number,
            Calc::Currency { .. } => Kind::Currency,
            _ => Kind::Text,
        }
    }
}

/// One field of a category.
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
    pub formula: &'static str,
    pub kind: Kind,
    /// The format it is made with.
    pub format: &'static str,
}

const fn choice(id: &'static str, label: &'static str, formula: &'static str, kind: Kind, format: &'static str) -> Choice {
    Choice { id, label, formula, kind, format }
}

/// The date and time format: `FIELDPICTURE(211)`.
const DATE_FORMAT: &str = "{<211>}";

/// A category of the dialog.
#[derive(Clone, Copy, Debug)]
pub struct Category {
    pub id: &'static str,
    pub label: &'static str,
    /// The fields that are always the same; those of Shape Data and User-defined Cells are the shape's rows.
    pub choices: &'static [Choice],
}

const DATE_TIME: &[Choice] = &[
    choice("creation", "Creation Date/Time", "DOCCREATION()", Kind::Date, DATE_FORMAT),
    choice("current", "Current Date/Time", "NOW()", Kind::Date, DATE_FORMAT),
    choice("last_edit", "Last Edit Date/Time", "DOCLASTEDIT()", Kind::Date, DATE_FORMAT),
    choice("print", "Print Date/Time", "DOCLASTPRINT()", Kind::Date, DATE_FORMAT),
];

const DOCUMENT: &[Choice] = &[
    choice("creator", "Creator", "CREATOR()", Kind::Text, ""),
    choice("description", "Description", "DESCRIPTION()", Kind::Text, ""),
    choice("directory", "Directory", "DIRECTORY()", Kind::Text, ""),
    choice("filename", "Filename", "FILENAME()", Kind::Text, ""),
    choice("keywords", "Keywords", "KEYWORDS()", Kind::Text, ""),
    choice("subject", "Subject", "SUBJECT()", Kind::Text, ""),
    choice("title", "Title", "TITLE()", Kind::Text, ""),
    choice("manager", "Manager", "MANAGER()", Kind::Text, ""),
    choice("company", "Company", "COMPANY()", Kind::Text, ""),
    choice("category", "Category", "CATEGORY()", Kind::Text, ""),
    choice("hyperlink_base", "Hyperlink Base", "HYPERLINKBASE()", Kind::Text, ""),
];

const GEOMETRY: &[Choice] = &[
    choice("height", "Height", "Height", Kind::Number, "esc(7)"),
    choice("width", "Width", "Width", Kind::Number, "esc(7)"),
    choice("angle", "Angle", "Angle", Kind::Number, "esc(12)"),
];

const OBJECT: &[Choice] = &[
    choice("data1", "Data 1", "DATA1()", Kind::Text, ""),
    choice("data2", "Data 2", "DATA2()", Kind::Text, ""),
    choice("data3", "Data 3", "DATA3()", Kind::Text, ""),
    choice("id", "ID", "ID()", Kind::Number, ""),
    choice("master", "Master", "MASTERNAME()", Kind::Text, ""),
    choice("name", "Name", "NAME()", Kind::Text, ""),
    choice("type", "Type", "TYPEDESC()", Kind::Text, ""),
];

const PAGE: &[Choice] = &[
    choice("background", "Background", "BKGPAGENAME()", Kind::Text, ""),
    choice("name", "Name", "PAGENAME()", Kind::Text, ""),
    choice("page_count", "Number of Pages", "PAGECOUNT()", Kind::Number, ""),
    choice("page_number", "Page Number", "PAGENUMBER()", Kind::Number, ""),
];

/// The categories, in the order the dialog lists them.
pub const CATEGORIES: [Category; 8] = [
    Category { id: "custom", label: "Custom Formula", choices: &[] },
    Category { id: "date_time", label: "Date/Time", choices: DATE_TIME },
    Category { id: "document", label: "Document Info", choices: DOCUMENT },
    Category { id: "geometry", label: "Geometry", choices: GEOMETRY },
    Category { id: "object", label: "Object Info", choices: OBJECT },
    Category { id: "page", label: "Page Info", choices: PAGE },
    Category { id: "shape_data", label: "Shape Data", choices: &[] },
    Category { id: "user_cell", label: "User-defined Cells", choices: &[] },
];

/// The formats the Data Format list offers, by kind, as `(code, label)`: `FIELDPICTURE(code)` is the text a `Format` cell holds.
pub const DATE_FORMATS: [(u32, &str); 14] = [
    (22, "M/d/y"),
    (24, "MMM d, yyyy"),
    (25, "MMMM d, yyyy"),
    (28, "d MMM, yyyy"),
    (29, "d MMMM, yyyy"),
    (31, "h:mm"),
    (34, "HH:mm"),
    (35, "h:mm tt"),
    (201, "dddd, MMMM dd, yyyy"),
    (204, "yyyy-MM-dd"),
    (211, "M/d/yyyy h:mm am/pm"),
    (212, "M/d/yyyy h:mm:ss am/pm"),
    (215, "HH:mm"),
    (216, "HH:mm:ss"),
];

/// The number formats, by code.
pub const NUMBER_FORMATS: [(u32, &str); 8] =
    [(2, "0"), (4, "0.0"), (6, "0.00"), (3, "0 u"), (5, "0.0 u"), (7, "0.00 u"), (12, "degrees"), (15, "0 #/#")];

/// The text formats, by code.
pub const TEXT_FORMATS: [(u32, &str); 3] = [(37, "as it is"), (38, "lower case"), (39, "UPPER CASE")];

/// What the environment of a call gives the evaluator: the clock and the file.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// Seconds since 1970 (UTC), or `None` where there is no clock.
    pub now: Option<i64>,
    /// The machine's offset from UTC in seconds (0: the build reads no zone).
    pub utc_offset: i32,
    /// The file the drawing is open from.
    pub file: Option<(String, String)>,
    /// Whether `file` is known: a call made with none (the hook of an edit) leaves alone a field that reads it.
    pub complete: bool,
}

impl Env {
    /// An environment that knows nothing of the clock or the file: what an edit's own refresh has.
    pub fn none() -> Env {
        Env::default()
    }

    /// The environment of a session: its clock and its file.
    pub fn new(now: Option<i64>, file: Option<(String, String)>) -> Env {
        Env { now, utc_offset: 0, file, complete: true }
    }

    fn options(&self) -> EvalOptions {
        let clock = self.now.map_or_else(Default::default, |n| diacraft_calc::clock_at(n, self.utc_offset));
        let (name, directory) = self.file.clone().unzip();
        EvalOptions { clock, file: FileInfo { name, directory }, ..crate::measure::eval_options() }
    }

    /// The document the evaluator reads: a drawing with no creation or last edit date shows the time it is made at.
    fn document<'d>(&self, doc: &'d Document) -> Cow<'d, Document> {
        let Some(iso) = self.now.and_then(diacraft_calc::utc_iso) else { return Cow::Borrowed(doc) };
        if doc.props.created.is_some() && doc.props.modified.is_some() {
            return Cow::Borrowed(doc);
        }
        let mut copy = doc.clone();
        let props = Arc::make_mut(&mut copy.props);
        props.created.get_or_insert_with(|| Arc::from(iso.as_str()));
        props.modified.get_or_insert_with(|| Arc::from(iso.as_str()));
        Cow::Owned(copy)
    }
}

/// What a field is to be made of, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub category: &'static str,
    /// The field's id (or the row's name).
    pub field: String,
    pub formula: String,
    /// `None` for a formula whose result decides the type (a custom one, a user cell).
    pub kind: Option<Kind>,
    pub format: String,
}

/// What an insertion made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inserted {
    pub ix: u32,
    pub at: usize,
    pub formula: String,
    pub format: String,
    pub shown: String,
    pub kind: Kind,
}

fn category_of(id: &str) -> Result<&'static Category, Problem> {
    CATEGORIES.iter().find(|c| c.id == id).ok_or_else(|| {
        let all: Vec<&str> = CATEGORIES.iter().map(|c| c.id).collect();
        Problem::bad(format!("`category` must be one of {}, and {} is none of them", all.join(", "), crate::error::shown(id)))
    })
}

fn check_picture(what: &str, text: &str) -> Result<(), Problem> {
    if text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        return Err(Problem::bad(format!("`{what}` must be at most {MAX_TEXT_CHARS} characters")));
    }
    if text.chars().any(char::is_control) {
        return Err(Problem::bad(format!("`{what}` must not hold control characters")));
    }
    Ok(())
}

/// A row of a section of the shape: its name, its `Type` cell and its `Format` cell's text.
type RowFacts = (String, Option<f64>, String);

/// The rows of a section of the shape (`Property` or `User`), its own and those it takes from its master, with the row's `Type` and `Format` cells.
fn row_names(doc: &Document, at: &ShapeRef, section: &str) -> Result<Vec<RowFacts>, Problem> {
    let res = Resolver::new(doc);
    let rows = match res.effective_rows_within(&SheetRef::Shape(at.clone()), &SectionKey::new(section, None), MAX_ROWS_LISTED) {
        None => return Ok(Vec::new()),
        Some(Err(n)) => return Err(Problem::failed(format!("the shape's {section} section has {n} rows, and at most {MAX_ROWS_LISTED} are listed"))),
        Some(Ok(rows)) => rows,
    };
    let mut out = Vec::new();
    for row in &rows {
        let RowKey::Name(name) = &row.key else { continue };
        let kind = row.cells.iter().find(|(n, _)| n.as_str() == "Type").and_then(|(_, r)| r.value.cell?.value.as_ref()?.number());
        let format = row.cells.iter().find(|(n, _)| n.as_str() == "Format").and_then(|(_, r)| Some(r.value.cell?.value.as_ref()?.text().to_owned()));
        out.push((name.to_string(), kind, format.unwrap_or_default()));
    }
    Ok(out)
}

/// Whether a row's name can be written in a formula as `Prop.<name>`.
fn writable(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// The fields of a category for shape `id`: the same ones for each category but Shape Data and User-defined Cells, which are the shape's rows (`Prop.<row>`, `User.<row>`).
pub fn choices_of(doc: &Document, page: u32, id: Option<u32>, category: &Category) -> Result<Vec<Listed>, Problem> {
    let section = match category.id {
        "shape_data" => "Property",
        "user_cell" => "User",
        _ => {
            return Ok(category
                .choices
                .iter()
                .map(|c| (c.id.to_owned(), c.label.to_owned(), c.formula.to_owned(), c.kind, c.format.to_owned()))
                .collect());
        }
    };
    let Some(id) = id else { return Ok(Vec::new()) };
    let at = require_shape(doc, page, id)?;
    let prefix = if section == "Property" { "Prop" } else { "User" };
    Ok(row_names(doc, &at, section)?
        .into_iter()
        .filter(|(name, _, _)| writable(name))
        .map(|(name, kind, _)| {
            let kind = if section == "Property" { prop_kind(kind) } else { Kind::Text };
            let formula = format!("{prefix}.{name}");
            (name.clone(), name, formula, kind, String::new())
        })
        .collect())
}

/// The kind a shape data row's `Type` says (0 string, 2 number, 5 date, 6 duration, 7 currency; the lists are strings).
fn prop_kind(cell: Option<f64>) -> Kind {
    match cell.map(|n| n.round() as i64) {
        Some(2) => Kind::Number,
        Some(5) => Kind::Date,
        Some(6) => Kind::Duration,
        Some(7) => Kind::Currency,
        _ => Kind::Text,
    }
}

/// Checks what a field is asked to be and says what it is made of.
pub fn plan(
    doc: &Document,
    at: &ShapeRef,
    category: &str,
    field: Option<&str>,
    formula: Option<&str>,
    format: Option<&str>,
) -> Result<Planned, Problem> {
    let cat = category_of(category)?;
    if let Some(format) = format {
        check_picture("format", format)?;
    }
    if cat.id == "custom" {
        if field.is_some() {
            return Err(Problem::bad("`field` is not used by the custom category: give `formula`"));
        }
        let formula = formula.map(str::trim).filter(|f| !f.is_empty()).ok_or_else(|| Problem::bad("the custom category needs a `formula`"))?;
        check_picture("formula", formula)?;
        let parsed = diacraft_formula::parse(formula);
        if parsed.ast.is_err() {
            return Err(Problem::bad(format!("the formula {} does not read as a formula", crate::error::shown(formula))));
        }
        return Ok(Planned {
            category: "custom",
            field: String::new(),
            formula: formula.to_owned(),
            kind: None,
            format: format.unwrap_or("").to_owned(),
        });
    }
    if formula.is_some() {
        return Err(Problem::bad("`formula` is for the custom category"));
    }
    let field = field.ok_or_else(|| Problem::bad(format!("the {} category needs a `field`", cat.id)))?;
    if cat.choices.is_empty() {
        // Shape data and user-defined cells: a row of the shape.
        let section = if cat.id == "shape_data" { "Property" } else { "User" };
        let rows = row_names(doc, at, section)?;
        let Some((name, kind, row_format)) = rows.iter().find(|(n, _, _)| n == field) else {
            let listed: Vec<&str> = rows.iter().map(|(n, _, _)| n.as_str()).take(20).collect();
            return Err(Problem::bad(format!(
                "the shape has no {} row named {} (it has {})",
                if section == "Property" { "shape data" } else { "user-defined cell" },
                crate::error::shown(field),
                if listed.is_empty() { "none".to_owned() } else { listed.join(", ") }
            )));
        };
        if !writable(name) {
            return Err(Problem::bad(format!("the row {} has a name no formula can write", crate::error::shown(name))));
        }
        let prefix = if section == "Property" { "Prop" } else { "User" };
        // A shape data field has the format its row has, written as it is now.
        let default_format = if section == "Property" { row_format.clone() } else { String::new() };
        return Ok(Planned {
            category: cat.id,
            field: name.clone(),
            formula: format!("{prefix}.{name}"),
            kind: (section == "Property").then(|| prop_kind(*kind)),
            format: format.map_or(default_format, str::to_owned),
        });
    }
    let choice = cat.choices.iter().find(|c| c.id == field).ok_or_else(|| {
        let all: Vec<&str> = cat.choices.iter().map(|c| c.id).collect();
        Problem::bad(format!("the {} category has no field {} (it has {})", cat.id, crate::error::shown(field), all.join(", ")))
    })?;
    Ok(Planned {
        category: cat.id,
        field: choice.id.to_owned(),
        formula: choice.formula.to_owned(),
        kind: Some(choice.kind),
        format: format.unwrap_or(choice.format).to_owned(),
    })
}

fn row_addr(ix: u32, cell: &str) -> CellAddr {
    CellAddr::in_row("Field", None, RowKey::Ix(ix), cell)
}

/// Whether a function the evaluator cannot work out without a clock or a file stands in for a value that is not there (a field then shows nothing).
fn lacks_input(name: &str) -> bool {
    matches!(name, "NOW" | "DOCCREATION" | "DOCLASTEDIT" | "DOCLASTSAVE" | "DOCLASTPRINT")
}

/// What a row works out to: its value, the format's text, and the text it shows (`None` when the value is no text a format can say).
struct Worked {
    value: Calc,
    format: String,
    shown: Option<String>,
}

/// The text of the row's `Format` cell: the value it holds (`V`), which is the picture whether the cell is a constant or a formula's result. A picture that looks like a number (`0.00`) is
/// text here, which a value read through the evaluator would not be.
fn format_of(res: &Resolver<'_>, at: &ShapeRef, ix: u32) -> String {
    res.resolve(&SheetRef::Shape(at.clone()), &row_addr(ix, "Format"))
        .and_then(|r| Some(r.value.cell?.value.as_ref()?.text().to_owned()))
        .unwrap_or_default()
}

/// Whether the row's `Value` (its own or its master's) reads the page that shows the field: `PAGENUMBER` or `PAGENAME`. On a background page such a field shows the
/// page that uses the background, which the background page's own answer is not.
fn reads_the_page(res: &Resolver<'_>, at: &ShapeRef, ix: u32) -> bool {
    res.resolve(&SheetRef::Shape(at.clone()), &row_addr(ix, "Value"))
        .and_then(|r| {
            let formula = r.value.cell?.formula.text()?;
            let upper = formula.to_ascii_uppercase();
            Some(upper.contains("PAGENUMBER") || upper.contains("PAGENAME"))
        })
        .unwrap_or(false)
}

fn work_row(ev: &mut Evaluator<'_, '_>, res: &Resolver<'_>, at: &ShapeRef, ix: u32, language: Option<&str>) -> Worked {
    let value = ev.evaluate(&CellAddress::new(SheetRef::Shape(at.clone()), row_addr(ix, "Value")));
    let format = format_of(res, at, ix);
    let shown = field_text(&value, &format, language);
    Worked { value, format, shown }
}

/// Whether the shape's master gives a `Field` row for `ix`, with the `IX` values it gives made once into `known` on the first call.
fn has_inherited_row(res: &Resolver<'_>, at: &ShapeRef, known: &mut Option<HashSet<u32>>, ix: u32) -> bool {
    known
        .get_or_insert_with(|| match res.effective_rows_within(&SheetRef::Shape(at.clone()), &SectionKey::new("Field", None), MAX_FIELDS * 4) {
            Some(Ok(rows)) => rows
                .into_iter()
                .filter_map(|row| match row.key {
                    RowKey::Ix(n) => Some(n),
                    _ => None,
                })
                .collect(),
            _ => HashSet::new(),
        })
        .contains(&ix)
}

/// The next `IX` for a field of the shape: one more than the highest of its rows and its `fld`s, and how many fields it has.
fn next_ix(doc: &Document, at: &ShapeRef) -> Result<(u32, usize), Problem> {
    let shape = doc.shape(at).ok_or_else(|| Problem::failed("the shape went away"))?;
    let mut highest: Option<u32> = None;
    let mut count = 0usize;
    for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
        if let TextPart::Fld { ix, .. } = part {
            highest = highest.max(Some(*ix));
            count += 1;
        }
    }
    let res = Resolver::new(doc);
    match res.effective_rows_within(&SheetRef::Shape(at.clone()), &SectionKey::new("Field", None), MAX_FIELDS * 4) {
        None => {}
        Some(Err(n)) => return Err(Problem::failed(format!("the shape's Field section has {n} rows, and at most {} are read", MAX_FIELDS * 4))),
        Some(Ok(rows)) => {
            for row in rows {
                if let RowKey::Ix(ix) = row.key {
                    highest = highest.max(Some(ix));
                }
            }
        }
    }
    let next = highest.map_or(Some(0), |h| h.checked_add(1)).ok_or_else(|| Problem::failed("no field numbers are left on the shape"))?;
    Ok((next, count))
}

/// The cells a worked value is kept in: `V` and `U`, written as a result of the evaluation.
fn cache_edit(value: &Calc) -> Option<CellEdit> {
    let encoded = diacraft_calc::encode(value)?;
    Some(CellEdit::Cached { value: Value::from_text(&encoded.v), error: None, unit: encoded.u.map(|u| Arc::from(u.as_str())) })
}

/// Writes results of an evaluation into cells (the origin that a guard does not stop).
fn write_results(doc: &mut Document, writes: Vec<(SheetRef, CellAddr, CellEdit)>) -> Result<(), Problem> {
    if writes.is_empty() {
        return Ok(());
    }
    let batch = writes.into_iter().map(|(sheet, addr, edit)| (sheet, CellWrite { addr, edit, origin: WriteOrigin::Evaluation })).collect();
    doc.write_batch(batch)?;
    Ok(())
}

/// A shape's parts with `part` put at character `at` of its editable text (the `Chars` put end to end): where the characters before the place end, and before any marker that follows them, so
/// that the field is in the run of the character before it; at the start, after the markers that start the text and ahead of the first characters.
fn with_part(parts: &[TextPart], at: usize, part: TextPart) -> Result<Vec<TextPart>, Problem> {
    let total: usize = parts.iter().map(|p| if let TextPart::Chars(s) = p { s.chars().count() } else { 0 }).fold(0, usize::saturating_add);
    if at > total {
        return Err(Problem::bad(format!("the place {at} is not within the text, which has {total} characters (fields are not counted)")));
    }
    // Where the part goes: before the part at `index`, or inside the characters of the part at `split` (and the character that many in).
    let mut index: Option<usize> = None;
    let mut split: Option<(usize, usize)> = None;
    let mut pos = 0usize;
    if at == total {
        // The end of the text is after what is already there, the fields at its end too, and before the markers that close it; with nothing there, after the markers that start it.
        let leading = parts.iter().position(|p| !matches!(p, TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_))).unwrap_or(parts.len());
        index = Some(parts.iter().rposition(|p| matches!(p, TextPart::Chars(_) | TextPart::Fld { .. })).map_or(leading, |last| last + 1));
    }
    for (i, p) in parts.iter().enumerate() {
        if index.is_some() {
            break;
        }
        let TextPart::Chars(s) = p else { continue };
        let n = s.chars().count();
        if n == 0 {
            continue;
        }
        if at == 0 {
            index = Some(i);
        } else if at < pos + n {
            split = Some((i, at - pos));
        } else if at == pos + n {
            index = Some(i + 1);
        }
        if index.is_some() || split.is_some() {
            break;
        }
        pos += n;
    }
    let mut out: Vec<TextPart> = Vec::with_capacity(parts.len() + 2);
    match (index, split) {
        (_, Some((i, k))) => {
            for (j, p) in parts.iter().enumerate() {
                match p {
                    TextPart::Chars(s) if j == i => {
                        let cut = s.char_indices().nth(k).map_or(s.len(), |(b, _)| b);
                        let (head, tail) = s.split_at_checked(cut).unwrap_or((s, ""));
                        out.push(TextPart::Chars(Arc::from(head)));
                        out.push(part.clone());
                        out.push(TextPart::Chars(Arc::from(tail)));
                    }
                    other => out.push(other.clone()),
                }
            }
        }
        (Some(i), None) => {
            out.extend(parts.iter().take(i).cloned());
            out.push(part);
            out.extend(parts.iter().skip(i).cloned());
        }
        (None, None) => {
            // No characters at all: the field follows the markers that start the text.
            let after = parts.iter().position(|p| !matches!(p, TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_))).unwrap_or(parts.len());
            out.extend(parts.iter().take(after).cloned());
            out.push(part);
            out.extend(parts.iter().skip(after).cloned());
        }
    }
    Ok(out)
}

/// Puts a field into shape `id` of a page at character `at` of its text (the end when `None`), as the plan says: its row is made, what it shows is worked out and the `fld` is put in the text,
/// as one edit. Refused, with the document as it was, for a shape that holds [`MAX_FIELDS`] fields, a place past the text, and a formula whose value is an error or a function that cannot be
/// worked out.
pub fn insert_field(doc: &mut Document, page: u32, id: u32, at_char: Option<usize>, plan: &Planned, env: &Env) -> Result<Inserted, Problem> {
    let at = require_shape(doc, page, id)?;
    let (ix, count) = next_ix(doc, &at)?;
    if count >= MAX_FIELDS {
        return Err(Problem::failed(format!("a shape holds at most {MAX_FIELDS} fields")));
    }
    let (old_parts, ext) = {
        let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
        (shape.text.as_ref().map(|t| t.parts.clone()).unwrap_or_default(), shape.text.as_ref().map(|t| t.ext.clone()).unwrap_or_default())
    };
    let total: usize = old_parts.iter().map(|p| if let TextPart::Chars(s) = p { s.chars().count() } else { 0 }).fold(0, usize::saturating_add);
    let at_char = at_char.unwrap_or(total);
    // The place is checked before anything is written.
    with_part(&old_parts, at_char, TextPart::Fld { ix, text: Arc::from("") })?;
    let sheet = SheetRef::Shape(at.clone());
    let type_now = plan.kind.map_or(0, Kind::type_cell);
    let format_cell = Cell::constant(Value::from_text(&plan.format));
    write_cells(
        doc,
        vec![
            (sheet.clone(), row_addr(ix, "Value"), CellEdit::Cell(Cell::with_formula(&plan.formula, None))),
            (sheet.clone(), row_addr(ix, "Format"), CellEdit::Cell(format_cell)),
            (sheet.clone(), row_addr(ix, "Type"), CellEdit::Value(Value::from_text(&type_now.to_string()))),
        ],
    )?;
    super::settle::settle_cells(doc, page, vec![(sheet.clone(), row_addr(ix, "Value")), (sheet.clone(), row_addr(ix, "Format"))])?;
    // Worked out with the session's clock and file, which the commit of the edit has not got.
    let (worked, language) = {
        let evaldoc = env.document(doc);
        let mut ctx = EvalCtx::new(env.options(), Budget::with_steps(MAX_STEPS));
        let res = Resolver::new(&evaldoc);
        let mut ev = Evaluator::new(&evaldoc, &mut ctx);
        let language = evaldoc.props.language.clone();
        (work_row(&mut ev, &res, &at, ix, language.as_deref()), language)
    };
    let mut results: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let mut format = worked.format.clone();
    let mut shown = worked.shown.clone();
    let kind = match &worked.value {
        Calc::Error(e) => {
            let words = format!("the field's formula {} gives {}", crate::error::shown(&plan.formula), e.text());
            return Err(if plan.category == "custom" { Problem::bad(words) } else { Problem::failed(words) });
        }
        Calc::Unsupported(name) if !lacks_input(name) => {
            return Err(Problem::bad(format!(
                "the field's formula {} uses {}, which DiaCraft does not work out",
                crate::error::shown(&plan.formula),
                crate::error::shown(name)
            )));
        }
        Calc::Unsupported(_) => plan.kind.unwrap_or(Kind::Text),
        value => plan.kind.unwrap_or_else(|| Kind::of_value(value)),
    };
    if let Some(edit) = cache_edit(&worked.value) {
        results.push((sheet.clone(), row_addr(ix, "Value"), edit));
    }
    // A custom formula's type is what it gave, and a date of one has the date's format.
    let mut writes = Vec::new();
    if plan.kind.is_none() {
        writes.push((sheet.clone(), row_addr(ix, "Type"), CellEdit::Value(Value::from_text(&kind.type_cell().to_string()))));
        if kind == Kind::Date && format.is_empty() {
            format = DATE_FORMAT.to_owned();
            writes.push((sheet.clone(), row_addr(ix, "Format"), CellEdit::Value(Value::from_text(&format))));
            shown = field_text(&worked.value, &format, language.as_deref());
        }
    }
    write_cells(doc, writes)?;
    write_results(doc, results)?;
    let shown = shown.unwrap_or_default();
    let parts = with_part(&old_parts, at_char, TextPart::Fld { ix, text: Arc::from(shown.as_str()) })?;
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    shape.text = Some(Arc::new(Text { parts, ext }));
    super::settle::settle_text(doc, page, &at)?;
    Ok(Inserted { ix, at: at_char, formula: plan.formula.clone(), format, shown, kind })
}

/// What `insert_field` would show, with nothing changed: on a copy of the document, which is let go of.
pub fn sample(doc: &Document, page: u32, id: u32, plan: &Planned, env: &Env) -> Result<Inserted, Problem> {
    let mut probe = doc.clone();
    insert_field(&mut probe, page, id, None, plan, env)
}

/// Which fields a refresh looks at.
#[derive(Clone, Copy, Debug)]
pub enum Scope<'a> {
    /// Every page.
    All,
    /// One page.
    Page(u32),
    /// These shapes of a page.
    Shapes(u32, &'a [u32]),
    /// These shapes of a page, where they are.
    Refs(u32, &'a [ShapeRef]),
}

/// What a refresh did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// The fields whose text changed.
    pub updated: usize,
    /// The fields looked at (each distinct field of a shape once).
    pub examined: usize,
    /// The fields of shapes that hold more than [`MAX_FIELDS`]: left as they were, and counted.
    pub skipped: usize,
}

/// A shape's fields, as a walk finds them.
struct Found {
    page: u32,
    at: ShapeRef,
    /// The distinct `IX` of its `fld`s, in the order they first appear.
    fields: Vec<u32>,
}

fn has_fields(shape: &diacraft_doc::Shape) -> bool {
    shape.text.as_ref().is_some_and(|t| t.parts.iter().any(|p| matches!(p, TextPart::Fld { .. })))
}

/// How many `fld`s the shape's text holds.
fn fld_count(shape: &diacraft_doc::Shape) -> usize {
    shape.text.iter().flat_map(|t| t.parts.iter()).filter(|p| matches!(p, TextPart::Fld { .. })).count()
}

/// What a walk of the shapes counted and kept: the shapes whose fields are worked out, and the budget of the call, **which every field counts against, the fields of a shape that is skipped too**.
pub(crate) struct Tally {
    found: Vec<Found>,
    /// The fields counted against `limit`: those of the shapes kept and of the shapes skipped.
    charged: usize,
    /// The fields of the shapes that hold more than [`MAX_FIELDS`].
    skipped: usize,
    /// The most fields a call counts.
    limit: usize,
    /// The budget is spent: no more shapes are taken.
    over: bool,
}

impl Tally {
    pub(crate) fn new(limit: usize) -> Tally {
        Tally { found: Vec::new(), charged: 0, skipped: 0, limit, over: false }
    }

    /// Counts a shape's `count` fields against the call's budget: whether the shape is kept, which it is when it has fields, holds at most [`MAX_FIELDS`] of them and the
    /// budget lasts. The fields of a shape of more are counted as skipped; past the budget no shape is kept, then or after.
    pub(crate) fn charge(&mut self, count: usize) -> bool {
        if self.over || count == 0 {
            return false;
        }
        self.charged = self.charged.saturating_add(count);
        if self.charged > self.limit {
            self.over = true;
            return false;
        }
        if count > MAX_FIELDS {
            self.skipped = self.skipped.saturating_add(count);
            return false;
        }
        true
    }

    /// The fields counted as skipped, and whether the budget is spent.
    #[cfg(kani)]
    pub(crate) fn skipped_and_over(&self) -> (usize, bool) {
        (self.skipped, self.over)
    }

    /// Counts the shape's fields, and keeps the shape when it holds at most [`MAX_FIELDS`] of them and the budget lasts.
    fn take(&mut self, page: u32, at: ShapeRef, shape: &diacraft_doc::Shape) {
        if self.over || !self.charge(fld_count(shape)) {
            return;
        }
        let mut seen: HashSet<u32> = HashSet::new();
        let fields: Vec<u32> = shape
            .text
            .iter()
            .flat_map(|t| t.parts.iter())
            .filter_map(|p| if let TextPart::Fld { ix, .. } = p { Some(*ix) } else { None })
            .filter(|ix| seen.insert(*ix))
            .collect();
        self.found.push(Found { page, at, fields });
    }
}

/// The shapes with fields on a page, in order, walking at most `max` shapes in all (`visited` counts over the pages): `Err(())` past it, and the walk stops when the tally's budget is spent.
fn walk_page(doc: &Document, page: u32, visited: &mut usize, max: usize, tally: &mut Tally) -> Result<(), ()> {
    let Some(top) = doc.shapes_of(Owner::Page(page)) else { return Ok(()) };
    let mut stack: Vec<(&ShapeList, Vec<u32>, usize)> = vec![(top, Vec::new(), 0)];
    while let Some((list, base, at)) = stack.pop() {
        if tally.over {
            break;
        }
        let Some(shape) = list.get(at) else { continue };
        stack.push((list, base.clone(), at + 1));
        *visited += 1;
        if *visited > max {
            return Err(());
        }
        let mut path = base.clone();
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        tally.take(page, ShapeRef { owner: Owner::Page(page), path: path.clone() }, shape);
        if !shape.children.is_empty() && path.len() < diacraft_doc::MAX_SHAPE_DEPTH {
            stack.push((&shape.children, path, 0));
        }
    }
    Ok(())
}

/// Works the text of fields out again and puts what changed in, as one edit. A field whose row reads the clock or the file's name is left alone when `env` knows neither
/// (an edit's own refresh), and a text a format cannot say (a date with no format, an error) is kept as it is.
///
/// **What it costs.** Every field is counted before any is worked out, **against one budget for the call** ([`MAX_FIELDS_PER_CALL`], and [`MAX_AUTO_FIELDS`] for an edit's own refresh): a
/// shape that holds more than [`MAX_FIELDS`] is skipped and its fields are counted as skipped, and past the budget `strict` refuses, in words, while the other stops where it is. A
/// shape's rows and the texts its fields show are indexed once for the shape, each distinct field is worked out once, and the apply is by the same index: the work follows the number of fields.
/// `strict` also refuses a document with more shapes than [`MAX_WALK`], and an evaluation past [`MAX_STEPS`]; without it the work stops where it ends and what was done stands.
pub fn refresh(doc: &mut Document, scope: Scope<'_>, env: &Env, strict: bool) -> Result<Refreshed, Problem> {
    let max_walk = if strict { MAX_WALK } else { MAX_AUTO_WALK };
    let mut tally = Tally::new(if strict { MAX_FIELDS_PER_CALL } else { MAX_AUTO_FIELDS });
    let mut visited = 0usize;
    match scope {
        Scope::All => {
            for p in doc.pages.iter() {
                if walk_page(doc, p.id, &mut visited, max_walk, &mut tally).is_err() {
                    if strict {
                        return Err(Problem::failed(format!(
                            "the drawing has more than {MAX_WALK} shapes, and the fields of that many are not looked for in one call: name a page"
                        )));
                    }
                    break;
                }
            }
        }
        Scope::Page(page) => {
            require_page(doc, page)?;
            if walk_page(doc, page, &mut visited, max_walk, &mut tally).is_err() && strict {
                return Err(Problem::failed(format!(
                    "the page has more than {MAX_WALK} shapes, and the fields of that many are not looked for in one call"
                )));
            }
        }
        Scope::Shapes(page, ids) => {
            // Found in one walk of the page, where a search for each walked the page from its top for each; the first one named that is missing is refused.
            let named = super::scan::Named::of(doc, page, ids.iter().copied())?;
            for id in ids {
                let at = named.require(*id)?;
                if let Some(shape) = doc.shape(&at) {
                    tally.take(page, at.clone(), shape);
                }
            }
        }
        Scope::Refs(page, refs) => {
            for at in refs {
                if let Some(shape) = doc.shape(at) {
                    tally.take(page, at.clone(), shape);
                }
            }
        }
    }
    if tally.over && strict {
        return Err(Problem::failed(format!(
            "the drawing has more than {MAX_FIELDS_PER_CALL} fields, and at most {MAX_FIELDS_PER_CALL} are counted in one call: name a page or a shape"
        )));
    }
    let (found, skipped) = (tally.found, tally.skipped);
    if found.is_empty() {
        return Ok(Refreshed { skipped, ..Refreshed::default() });
    }
    // The background pages: a field there that reads the page shows the page that uses the background, and keeps the text it has.
    let backgrounds: HashSet<u32> = doc.pages.iter().filter(|p| p.background).map(|p| p.id).collect();
    // Worked out on the document as it is; nothing is changed until all are.
    let mut changes: Vec<TextChanges> = Vec::new();
    let mut results: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let mut examined = 0usize;
    {
        let evaldoc = env.document(doc);
        let mut ctx = EvalCtx::new(env.options(), Budget::with_steps(MAX_STEPS));
        let language = evaldoc.props.language.clone();
        let res = Resolver::new(&evaldoc);
        let mut ev = Evaluator::new(&evaldoc, &mut ctx);
        'shapes: for f in &found {
            let Some(shape) = doc.shape(&f.at) else { continue };
            let sheet = SheetRef::Shape(f.at.clone());
            // The shape's own `Field` rows by `IX`, and what each field shows now: each made once for the shape.
            let mut rows: HashMap<u32, &Arc<diacraft_doc::Row>> = HashMap::new();
            if let Some(section) = doc.sheet_of(&sheet).and_then(|s| s.section(&SectionKey::new("Field", None))) {
                for row in section.rows.iter() {
                    if let Some(RowKey::Ix(ix)) = row.key("Field") {
                        rows.entry(ix).or_insert(row);
                    }
                }
            }
            let mut shown_now: HashMap<u32, &str> = HashMap::new();
            for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
                if let TextPart::Fld { ix, text } = part {
                    shown_now.entry(*ix).or_insert(text.as_ref());
                }
            }
            let mut new_texts: Vec<(u32, String)> = Vec::new();
            // The `IX` values the shape's master gives rows for, found once for the shape and only when a field has no row of its own.
            let mut inherited: Option<HashSet<u32>> = None;
            for ix in &f.fields {
                let own = rows.get(ix).copied();
                // A `fld` with no row at all (a file's own text, which no row explains) shows what it was saved with: nothing says how to work it out.
                if own.is_none() && !has_inherited_row(&res, &f.at, &mut inherited, *ix) {
                    continue;
                }
                let value_cell = own.and_then(|r| r.cells.get("Value"));
                // A field on a background page (a master page of an OpenDocument drawing) that reads the page shows the number or the name of the page that uses the
                // background, so its text is left as the file saved it: the background page's own answer (0 for a number) is not what it shows.
                if backgrounds.contains(&f.page) && reads_the_page(&res, &f.at, *ix) {
                    continue;
                }
                // A row that reads the file's name is left alone by a refresh that does not know it.
                if !env.complete
                    && value_cell.and_then(|c| c.formula.text()).is_some_and(|s| {
                        let upper = s.to_ascii_uppercase();
                        upper.contains("FILENAME") || upper.contains("DIRECTORY")
                    })
                {
                    continue;
                }
                examined += 1;
                let worked = work_row(&mut ev, &res, &f.at, *ix, language.as_deref());
                if let Some(text) = &worked.shown
                    && shown_now.get(ix).copied() != Some(text.as_str())
                {
                    new_texts.push((*ix, text.clone()));
                    // The value the row shows is kept with it, where the row works its value out by a formula of its own.
                    if value_cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_)))
                        && let Some(edit) = cache_edit(&worked.value)
                    {
                        results.push((sheet.clone(), row_addr(*ix, "Value"), edit));
                    }
                }
                if ev_exhausted(&ev) {
                    if strict {
                        return Err(Problem::failed("the fields take more work than one call does: name a shape or a page"));
                    }
                    break 'shapes;
                }
            }
            if !new_texts.is_empty() {
                changes.push((f.page, f.at.clone(), new_texts));
            }
        }
    }
    let mut updated = 0usize;
    for (page, at, texts) in &changes {
        let Some(shape) = shape_mut(doc, *page, &at.path) else { continue };
        let Some(text) = shape.text.clone() else { continue };
        let by_ix: HashMap<u32, &str> = texts.iter().map(|(i, t)| (*i, t.as_str())).collect();
        let mut parts = text.parts.clone();
        for part in parts.iter_mut() {
            if let TextPart::Fld { ix, text: shown } = part
                && let Some(new) = by_ix.get(ix)
                && shown.as_ref() != *new
            {
                *shown = Arc::from(*new);
                updated += 1;
            }
        }
        shape.text = Some(Arc::new(Text { parts, ext: text.ext.clone() }));
    }
    write_results(doc, results)?;
    for (page, at, _) in &changes {
        super::settle::settle_text(doc, *page, at)?;
    }
    Ok(Refreshed { updated, examined, skipped })
}

fn ev_exhausted(ev: &Evaluator<'_, '_>) -> bool {
    ev.exhausted()
}

/// The edit's own refresh for the shapes whose cells an edit changed: at most [`MAX_AUTO_SHAPES`] of them, each looked at where it is (no search), with no clock and no
/// file name. A failure leaves the texts as they were.
pub fn refresh_changed(doc: &mut Document, page: u32, cells: &[(SheetRef, CellAddr)]) {
    let mut refs: Vec<ShapeRef> = Vec::new();
    for (sheet, _) in cells {
        let SheetRef::Shape(at) = sheet else { continue };
        if at.owner != Owner::Page(page) || refs.contains(at) {
            continue;
        }
        if doc.shape(at).is_some_and(|s| has_fields(s)) {
            refs.push(at.clone());
            if refs.len() >= MAX_AUTO_SHAPES {
                break;
            }
        }
    }
    if refs.is_empty() {
        return;
    }
    // A refresh that cannot be made leaves the text as it was; the row, which is right, is what a save writes.
    let _ = refresh(doc, Scope::Refs(page, &refs), &Env::none(), false);
}

/// The edit's own refresh for every page (a page was added, renamed, taken away, copied or moved), with the clock and the file `env` gives: a failure leaves the texts as they were.
pub fn refresh_all_after_edit(doc: &mut Document, env: &Env) {
    // As above: a refresh that cannot be made leaves the texts as they were.
    let _ = refresh(doc, Scope::All, env, false);
}

/// How many `fld`s the pages' shapes hold in all, walking at most [`MAX_WALK`] shapes: `None` past it.
fn count_fields(doc: &Document) -> Option<usize> {
    let mut total = 0usize;
    let mut walked = 0usize;
    for page in doc.pages.iter() {
        let mut stack: Vec<(&ShapeList, usize)> = vec![(&page.shapes, 0)];
        while let Some(frame) = stack.last_mut() {
            let (list, at) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            walked += 1;
            if walked > MAX_WALK {
                return None;
            }
            total = total.saturating_add(fld_count(shape));
            if stack.len() < diacraft_doc::MAX_SHAPE_DEPTH {
                stack.push((&shape.children, 0));
            }
        }
    }
    Some(total)
}

/// What a save into an `.odg` file says of the fields made since the file was read (`baseline`): the splice writer puts a field only where the file had one, so a new one is written as the text it
/// shows. `None` when there is none, and when the drawings are too big to count.
pub fn new_fields_note(doc: &Document, baseline: Option<&Document>) -> Option<String> {
    let now = count_fields(doc)?;
    let before = baseline.map_or(Some(0), count_fields)?;
    let new = now.checked_sub(before).filter(|n| *n > 0)?;
    Some(format!(
        "{new} new field{} written as the text {} show{}: an OpenDocument drawing saved in place holds a field only where the file had one (a drawing saved as a new .odg holds page number, page count, file name and date fields), and a .diacraft drawing holds them all",
        if new == 1 { "" } else { "s" },
        if new == 1 { "it" } else { "they" },
        if new == 1 { "s" } else { "" },
    ))
}
