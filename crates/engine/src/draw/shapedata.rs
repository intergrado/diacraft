//! The Shape Data of a shape: Data, Show/Hide, Shape Data Window and the Define Shape Data dialog (Shape Data
//! window, Enter shape data values, Add and delete shape data fields, Shape data field types, Fixed and variable list fields, Field display format, Prompt text on a field, Hidden shape
//! data fields, Shape data field order, Ask for data on drop, Field language and calendar).
//!
//! **What is kept in cells.** A field is a row of the shape's `Property` section, named by the author (`Prop.<name>`), with the cells `Label`, `Prompt`, `Type`, `Format`, `Value`, `SortKey`,
//! `Invisible`, `Verify` (the cell the Ask on drop box is: `Prop.<name>.Verify`), `LangID` and `Calendar`. A shape takes the rows of its master. A value is a cell write, so a formula
//! that reads `Prop.<name>` is worked out again (`settle_cells`), and a field of the shape's text that shows it follows.
//!
//! **Project rules** (ours; the specs leave them open, and the check list lists them):
//!
//! * The `Type` cell's numbers are 0 string, 1 fixed list, 2 number, 3 boolean, 4 variable list, 5 date, 6 duration and 7 currency (Learn
//!   and MS-VSDX give the table, and the files use no other number). No cell, or an empty one, is 0. **A row whose cell holds another number, a fraction or a text is of an unknown type**
//!   Its value is a string's, it is listed as `unknown` with the number it holds, and its `Type` cell is kept as written until a type is given to it.
//! * An entered value is read **as its field's type** and stored as that type's value with that type's `U`: a string and a list item `STR`, a truth value `1` or `0` with `BOOL`, a date
//!   `YYYY-MM-DDThh:mm:ss` with `DATE`, a currency `<amount>` at four places and its code with `CY`, a duration in days with the unit it was typed in (`ED` for none), a number with its own unit
//!   (a number typed with none takes the unit the cell has). A text that is no value of the type is refused in words and nothing changes. An empty text clears the field. **The Value cell page says an
//!   entered value replaces any formula there, even one under `GUARD`**: the write is the forced one. **A date never set is a plain 0 with no unit** (all 18,028
//!   date rows of the files that hold a number and no unit hold 0, and the 199 that hold a date carry `DATE`), so a new or cleared date field is written so, and it shows nothing (ours).
//! * A list's items are its `Format`, apart at the semicolons ("list items sit semicolon-separated in the field's Format cell"). A fixed list takes only its items
//!   and the empty value. On a variable list a new value joins the `Format` **at its end** (the page says only that it joins the list), in the same undo step.
//! * What a field shows is its value through its `Format` picture (`calc::field_text`), the text as it is for a string and a list item, `TRUE` or `FALSE` for a truth value, a date as
//!   `M/d/yyyy` (and the time when it has one), a currency as `$1,234.50` (the system's regional setting is not read: ours is the US one) and a number in plain digits and its unit.
//! * Fields are listed **by `SortKey`, then row order, the unkeyed last**: the keys are compared as text ignoring case (`str::to_lowercase`, not the
//!   locale's collation), and equal keys keep the order of the rows (a master's rows first, the shape's own after: the order `Resolver::effective_rows` gives).
//! * A new field's row name is `name`, else its label with each run of characters that are no letter, digit or `_` made one `_` and the ends trimmed (a name that starts with a digit gets a
//!   `_` first), and the least `_2`, `_3` and so on that no row of the shape has, ignoring case. A new field with neither is `Property<n>` for the least `n` from 1 whose name is free, and its label is
//!   its name. A row is **never renamed**: formulas read it by its name (a change of name is a change of label).
//! * `shapedata.define` is the shape's whole list, as `hyperlink.set` is: a row it names is kept (the cells it gives change and the others stay, and a cell is written only when
//!   it differs from what the row reads now), a field with no `row` is a new row, and **a row the list leaves out is taken away, hidden or not**. A change of **type** reads the value again as
//!   the new type: it is kept as that type's value when it reads as one, and is **kept as it was**, with a note in the answer, when it does not (a change loses nothing).
//! * The Ask on drop cell is `Verify` in a file and in a formula, kept and listed (`ask`); the window asks for the values when a master with such a field is dropped, and
//!   `shapedata.list` is what it asks the engine. A cell spelled `Ask` is no cell of the section and is kept as it is.
//! * `LangID` is a language tag such as `en-US` (every cell of the files that holds a value holds one), read as the text the cell holds and written as a tag
//!   ([`is_language_tag`]); a number a file holds is listed as the text it is and kept, and changes nothing. A new field is given none. **`Calendar`** is one of the fourteen numbers
//!   MS-VSDX lists ([`CALENDARS`]), named; a new field holds 0, as every file's does; only a date reads it, and the build shows every date in the Western one.
//!   A number past the table is kept as read and has no name.
//!
//! **Hostile input.** A shape that holds more than [`MAX_ROWS`] rows is refused before any is read, and one that holds more than [`MAX_FIELDS`] fields is refused as a whole (a list that left some
//! out would take them away). The text a call reads is charged to a budget of [`MAX_READ_BYTES`] and each text cut at its cap when it is read ([`MAX_TEXT_CHARS`], [`MAX_VALUE_CHARS`]); the
//! texts a call is given are checked before anything is built; a list has at most [`MAX_ITEMS`] items.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Value as Calc, decode, encode, field_text, format_number, iso_to_serial, parse_decimal, read_quantity};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, CellWrite, Document, EffectiveRow, Resolved, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Value,
    WriteOrigin, WriteOutcome,
};
use diacraft_units::{Measure, Quantity, Unit, parse_unit};

use super::{Problem, require_shape, shape_mut};

/// The most fields one shape holds, listed or defined.
pub const MAX_FIELDS: usize = 512;
/// The most rows of a `Property` section a call reads: past it the call is refused, in words, and nothing is read.
pub const MAX_ROWS: usize = 4_096;
/// The longest a label, a prompt, a name or a sort key is, in characters.
pub const MAX_TEXT_CHARS: usize = 4_096;
/// The longest a value or a format (a list's items) is, in characters.
pub const MAX_VALUE_CHARS: usize = 32_768;
/// The longest a row's name is, in characters.
pub const MAX_NAME_CHARS: usize = 128;
/// The most items a list holds.
pub const MAX_ITEMS: usize = 1_024;
/// The most bytes of text one call reads from a shape's fields.
pub const MAX_READ_BYTES: usize = 8 * 1024 * 1024;
/// The most values one `shapedata.set` takes.
pub const MAX_VALUES: usize = MAX_FIELDS;
/// The longest a language tag a field is given is, in characters (ours).
pub const MAX_LANG_CHARS: usize = 64;

/// The calendars, by the `Calendar` cell's number: MS-VSDX's table, which follows (Learn lists 0 to 9 and calls 3 the Taiwan Calendar and 7 the Saka Era).
pub const CALENDARS: [&str; 14] = [
    "Western",
    "Arabic Hijri",
    "Hebrew Lunar",
    "Chinese National",
    "Japanese Emperor",
    "Thai Buddhist",
    "Korean Danki",
    "Japanese Saka Era",
    "Transliterated English",
    "Transliterated French",
    "Gregorian US English",
    "Gregorian Middle East French",
    "Gregorian Arabic",
    "Um-al-Qura",
];

/// The name of calendar `code`, `None` for a number the table does not list.
pub fn calendar_name(code: u32) -> Option<&'static str> {
    CALENDARS.get(usize::try_from(code).ok()?).copied()
}

/// Whether `text` is a language tag as a `LangID` cell holds one (`en-US`, a culture name of RFC 4646): parts of ASCII letters and digits apart at hyphens, the first 2 to 8
/// letters and each other 1 to 8 letters or digits, at most [`MAX_LANG_CHARS`] in all (ours: the shape of a tag, not the registry of its parts).
pub fn is_language_tag(text: &str) -> bool {
    if text.len() > MAX_LANG_CHARS {
        return false;
    }
    let mut parts = text.split('-');
    let first = parts.next().unwrap_or_default();
    (2..=8).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The whole number a `Type` cell's number is, `None` for a fraction, an infinity or a number past those a double holds exactly (such a cell is of an unknown type, and not the type it
/// rounds to).
pub fn type_number(n: f64) -> Option<i64> {
    const EXACT: f64 = 9_007_199_254_740_992.0;
    // The cast is exact for the numbers kept, and saturates (never panics) for the others, which are dropped.
    (n.is_finite() && n.fract() == 0.0 && n.abs() <= EXACT).then_some(n as i64)
}

/// Whether a value is a date never set: a plain 0 with no unit. The serial 0 with `DATE` is a date that is set.
fn is_unset_date(kind: Kind, text: &str, unit: Option<&str>) -> bool {
    kind == Kind::Date && unit.is_none_or(|u| u.trim().is_empty()) && parse_decimal(text.trim()) == Some(0.0)
}

/// The kind of value a field holds: the `Type` cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    FixedList,
    Number,
    Boolean,
    VariableList,
    Date,
    Duration,
    Currency,
}

impl Kind {
    /// The eight, by the `Type` cell's number.
    pub const ALL: [Kind; 8] =
        [Kind::String, Kind::FixedList, Kind::Number, Kind::Boolean, Kind::VariableList, Kind::Date, Kind::Duration, Kind::Currency];

    /// The `Type` cell's number.
    pub const fn code(self) -> u8 {
        match self {
            Kind::String => 0,
            Kind::FixedList => 1,
            Kind::Number => 2,
            Kind::Boolean => 3,
            Kind::VariableList => 4,
            Kind::Date => 5,
            Kind::Duration => 6,
            Kind::Currency => 7,
        }
    }

    /// The kind of a `Type` cell's number.
    pub fn from_code(code: i64) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| i64::from(k.code()) == code)
    }

    /// The word the commands use.
    pub const fn word(self) -> &'static str {
        match self {
            Kind::String => "string",
            Kind::FixedList => "fixed_list",
            Kind::Number => "number",
            Kind::Boolean => "boolean",
            Kind::VariableList => "variable_list",
            Kind::Date => "date",
            Kind::Duration => "duration",
            Kind::Currency => "currency",
        }
    }

    /// The name the dialog's list shows.
    pub const fn label(self) -> &'static str {
        match self {
            Kind::String => "String",
            Kind::FixedList => "Fixed List",
            Kind::Number => "Number",
            Kind::Boolean => "Boolean",
            Kind::VariableList => "Variable List",
            Kind::Date => "Date",
            Kind::Duration => "Duration",
            Kind::Currency => "Currency",
        }
    }

    /// The kind a word or a label names, in any case, with a space or an underscore between words.
    pub fn from_word(text: &str) -> Option<Kind> {
        let t = text.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Kind::ALL.into_iter().find(|k| k.word() == t)
    }

    /// Whether the field's `Format` holds its items.
    pub const fn is_list(self) -> bool {
        matches!(self, Kind::FixedList | Kind::VariableList)
    }

    /// The pictures the dialog offers for the type: `(picture, label)`. A list's `Format` is its items and a truth value has no picture, so those offer none.
    pub fn formats(self) -> Vec<(String, String)> {
        let number = ["0", "0.0", "0.00", "0 u", "0.0 u", "0.00 u"];
        let plain = |list: &[&str]| list.iter().map(|p| ((*p).to_owned(), (*p).to_owned())).collect();
        match self {
            Kind::String => {
                vec![("@".to_owned(), "As it is".to_owned()), ("@-".to_owned(), "lower case".to_owned()), ("@+".to_owned(), "UPPER CASE".to_owned())]
            }
            Kind::Number => plain(&number),
            Kind::Duration => plain(&number[..3]),
            Kind::Currency => plain(&["0", "0.00"]),
            Kind::Date => {
                super::fields::DATE_FORMATS.iter().map(|(code, label)| (diacraft_calc::field_picture(*code), (*label).to_owned())).collect()
            }
            Kind::FixedList | Kind::VariableList | Kind::Boolean => Vec::new(),
        }
    }
}

/// One field of a shape, as its row holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// The row's name, which formulas read as `Prop.<name>`.
    pub row: String,
    pub label: String,
    /// What the value is read as: a row of an unknown type is a string's.
    pub kind: Kind,
    /// Whether the `Type` cell names one of the eight kinds; a row whose cell holds another number or a text is of an unknown type.
    pub known: bool,
    /// The whole number the `Type` cell holds (0 when it has none), `None` for a fraction or a text: kept as it is until a type is given.
    pub type_code: Option<i64>,
    pub format: String,
    /// The `V` of the `Value` cell, and its `U`.
    pub value: String,
    pub unit: Option<String>,
    /// The formula of the `Value` cell, when it has one.
    pub formula: Option<String>,
    /// What the field shows: the value through its format.
    pub shown: String,
    pub prompt: String,
    pub sort_key: String,
    pub invisible: bool,
    pub ask: bool,
    /// The `LangID` cell's text, as the row holds it: a tag such as `en-US`, or a number a file holds.
    pub lang_id: Option<String>,
    pub calendar: Option<u32>,
    /// A list's items, empty for any other kind.
    pub items: Vec<String>,
}

impl Field {
    /// The word the commands give the type by: the kind's, or `unknown`.
    pub fn type_word(&self) -> &'static str {
        if self.known { self.kind.word() } else { "unknown" }
    }
}

/// The fields of a shape, in the order they are listed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    /// Every field, hidden or not, in the order listed.
    pub fields: Vec<Field>,
}

impl Listed {
    /// How many of the fields are hidden.
    pub fn hidden(&self) -> usize {
        self.fields.iter().filter(|f| f.invisible).count()
    }
}

/// `text` quoted for a refusal.
fn q(text: &str) -> String {
    format!("\"{}\"", crate::error::shown(text))
}

fn key() -> SectionKey {
    SectionKey::new("Property", None)
}

/// The items of a list's `Format`: apart at the semicolons, the empty ones left out, at most [`MAX_ITEMS`].
pub fn items_of(format: &str) -> Vec<String> {
    format.split(';').filter(|i| !i.is_empty()).take(MAX_ITEMS).map(str::to_owned).collect()
}

fn too_many_rows(n: usize) -> Problem {
    Problem::failed(format!("the shape's Property section has {n} rows, and at most {MAX_ROWS} are read"))
}

/// Cuts `text` at `most` characters, charging the bytes kept to `spent` first; the whole call is refused past [`MAX_READ_BYTES`].
fn take(text: &str, most: usize, spent: &mut usize) -> Result<String, Problem> {
    let end = text.char_indices().nth(most).map_or(text.len(), |(at, _)| at);
    *spent = spent.saturating_add(end);
    if *spent > MAX_READ_BYTES {
        return Err(Problem::failed(format!("the shape's shape data holds more text than is read at once: at most {MAX_READ_BYTES} bytes")));
    }
    Ok(text.get(..end).unwrap_or_default().to_owned())
}

fn cell_of<'r, 'a>(row: &'r EffectiveRow<'a>, name: &str) -> Option<&'r Resolved<'a>> {
    row.cells.iter().find(|(n, _)| n.as_str() == name).map(|(_, r)| r)
}

fn text_of(row: &EffectiveRow<'_>, name: &str, most: usize, spent: &mut usize) -> Result<String, Problem> {
    match cell_of(row, name).and_then(|r| r.value.cell?.value.as_ref()) {
        Some(v) => take(v.text(), most, spent),
        None => Ok(String::new()),
    }
}

/// The text of cell `name` as [`text_of`] reads it, `None` when the row has no such cell.
fn held_text(row: &EffectiveRow<'_>, name: &str, most: usize, spent: &mut usize) -> Result<Option<String>, Problem> {
    match cell_of(row, name).and_then(|r| r.value.cell?.value.as_ref()) {
        Some(v) => take(v.text(), most, spent).map(Some),
        None => Ok(None),
    }
}

fn number_of(row: &EffectiveRow<'_>, name: &str) -> Option<f64> {
    let v = cell_of(row, name)?.value.cell?.value.as_ref()?;
    v.number().or_else(|| match v.text().trim().to_ascii_lowercase().as_str() {
        "true" => Some(1.0),
        "false" => Some(0.0),
        _ => None,
    })
}

fn whole_of(row: &EffectiveRow<'_>, name: &str) -> Option<u32> {
    number_of(row, name).and_then(whole)
}

/// A cell's number as the whole number of a `LangID` or a `Calendar` field: the number itself when it is a whole number of a `u32`, else none.
pub(crate) fn whole(n: f64) -> Option<u32> {
    (n >= 0.0 && n <= f64::from(u32::MAX) && n.fract() == 0.0).then_some(n as u32)
}

/// The order the fields are listed in, by their sort keys as [`read_at`] lowers them: the keys as text, and a field with no key after every field that has one.
pub(crate) fn listed_order(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => a.cmp(b),
    }
}

/// The fields of the shape at `at`, every one, in the order listed. Refused, in words, past [`MAX_ROWS`] rows, [`MAX_FIELDS`] fields or [`MAX_READ_BYTES`] of text.
fn read_at(doc: &Document, at: &ShapeRef) -> Result<Listed, Problem> {
    let res = Resolver::new(doc);
    let rows = match res.effective_rows_within(&SheetRef::Shape(at.clone()), &key(), MAX_ROWS) {
        None => return Ok(Listed::default()),
        Some(Err(n)) => return Err(too_many_rows(n)),
        Some(Ok(rows)) => rows,
    };
    let mut spent = 0usize;
    let mut keyed: Vec<(String, Field)> = Vec::with_capacity(rows.len().min(MAX_FIELDS));
    for row in &rows {
        let RowKey::Name(name) = &row.key else { continue };
        if keyed.len() >= MAX_FIELDS {
            return Err(Problem::failed(format!("the shape has more than {MAX_FIELDS} shape data fields, and at most {MAX_FIELDS} are read")));
        }
        // No `Type` cell, or an empty one, is 0; a number is kept whole or not at all.
        let type_code = match cell_of(row, "Type").and_then(|r| r.value.cell?.value.as_ref()) {
            None => Some(0),
            Some(v) if v.text().trim().is_empty() => Some(0),
            Some(v) => v.number().and_then(type_number),
        };
        let known_kind = type_code.and_then(Kind::from_code);
        let kind = known_kind.unwrap_or(Kind::String);
        let label = text_of(row, "Label", MAX_TEXT_CHARS, &mut spent)?;
        let format = text_of(row, "Format", MAX_VALUE_CHARS, &mut spent)?;
        let value = text_of(row, "Value", MAX_VALUE_CHARS, &mut spent)?;
        let prompt = text_of(row, "Prompt", MAX_TEXT_CHARS, &mut spent)?;
        let sort_key = text_of(row, "SortKey", MAX_TEXT_CHARS, &mut spent)?;
        let unit = cell_of(row, "Value").and_then(|r| r.value.cell?.unit.as_deref()).map(str::to_owned);
        let formula = cell_of(row, "Value").and_then(|r| r.formula.cell?.formula.text().map(str::to_owned));
        let shown = shown_of(kind, &value, unit.as_deref(), &format);
        let field = Field {
            row: name.to_string(),
            label,
            kind,
            known: known_kind.is_some(),
            type_code,
            items: if kind.is_list() { items_of(&format) } else { Vec::new() },
            format,
            value,
            unit,
            formula,
            shown,
            prompt,
            invisible: number_of(row, "Invisible").is_some_and(|n| n != 0.0),
            ask: number_of(row, "Verify").is_some_and(|n| n != 0.0),
            lang_id: held_text(row, "LangID", MAX_TEXT_CHARS, &mut spent)?,
            calendar: whole_of(row, "Calendar"),
            sort_key: String::new(),
        };
        keyed.push((sort_key.to_lowercase(), Field { sort_key, ..field }));
    }
    // By sort key, the unkeyed last; the sort is stable, so equal keys keep the order of the rows.
    keyed.sort_by(|(a, _), (b, _)| listed_order(a, b));
    Ok(Listed { fields: keyed.into_iter().map(|(_, f)| f).collect() })
}

/// The fields of shape `id` on a page, every one (hidden too), in the order listed.
pub fn read_fields(doc: &Document, page: u32, id: u32) -> Result<Listed, Problem> {
    let at = require_shape(doc, page, id)?;
    read_at(doc, &at)
}

/// The name a shape is shown by in the Shape Data window's title: its name, else its `NameU`, else `Sheet.<id>`, cut at 256 characters.
pub fn shape_title(doc: &Document, page: u32, id: u32) -> Result<String, Problem> {
    let at = require_shape(doc, page, id)?;
    let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
    let named = shape.name.as_deref().filter(|n| !n.is_empty()).or_else(|| shape.name_u.as_deref().filter(|n| !n.is_empty()));
    Ok(named.map_or_else(|| format!("Sheet.{id}"), |name| name.chars().take(256).collect()))
}

// ----------------------------------------------------------------------------------------------------------------------------------------------- what a value shows

/// Whether a value's text says true or false: a number (not zero is true) or the words, in any case.
fn truth(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        other => parse_decimal(other).map(|n| n != 0.0),
    }
}

/// A number with thousands separators and `places` decimals: `1234.5` at 2 is `1,234.50`.
fn grouped(amount: f64, places: usize) -> String {
    let text = format!("{:.places$}", amount.abs());
    let (whole, rest) = text.split_once('.').map_or((text.as_str(), ""), |(w, r)| (w, r));
    let mut out = String::new();
    for (n, c) in whole.chars().enumerate() {
        if n > 0 && (whole.len() - n) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !rest.is_empty() {
        out.push('.');
        out.push_str(rest);
    }
    if amount < 0.0 && out.chars().any(|c| c.is_ascii_digit() && c != '0') { format!("-{out}") } else { out }
}

/// What a value shows through the field's format.
fn shown_of(kind: Kind, text: &str, unit: Option<&str>, format: &str) -> String {
    if text.is_empty() || is_unset_date(kind, text, unit) {
        return String::new();
    }
    let picture = format.trim();
    match kind {
        Kind::String => field_text(&Calc::text(text), picture, None).unwrap_or_else(|| text.to_owned()),
        Kind::FixedList | Kind::VariableList => text.to_owned(),
        Kind::Boolean => match truth(text) {
            Some(true) => "TRUE".to_owned(),
            Some(false) => "FALSE".to_owned(),
            None => text.to_owned(),
        },
        Kind::Number | Kind::Duration => match decode(text, unit, None) {
            Calc::Number(q) => {
                if let Some(shown) = field_text(&Calc::Number(q), picture, None).filter(|_| !picture.is_empty()) {
                    return shown;
                }
                // A duration shows as the unit it was typed in, by its short word; a duration with no unit is days.
                let mut out = format_number(q.shown());
                let suffix = match q.unit {
                    Unit::Unitless => None,
                    Unit::ElapsedDay | Unit::DurationDefault => Some("d"),
                    Unit::ElapsedHour => Some("h"),
                    Unit::ElapsedMinute => Some("min"),
                    Unit::ElapsedSecond => Some("s"),
                    Unit::ElapsedWeek => Some("w"),
                    other => other.suffix(),
                };
                if let Some(suffix) = suffix {
                    out.push(' ');
                    out.push_str(&suffix.to_ascii_lowercase());
                }
                out
            }
            _ => text.to_owned(),
        },
        Kind::Date => match decode(text, unit.or(Some("DATE")), None) {
            Calc::Number(q) if q.unit == Unit::Date => {
                let default = if q.stored.fract() == 0.0 { "M/d/yyyy" } else { "M/d/yyyy h:mm am/pm" };
                let picture = if picture.is_empty() { default } else { picture };
                field_text(&Calc::Number(q), picture, None).unwrap_or_else(|| text.to_owned())
            }
            _ => text.to_owned(),
        },
        Kind::Currency => match decode(text, unit.or(Some("CY")), None) {
            Calc::Currency { amount, currency } => {
                if !picture.is_empty()
                    && let Some(shown) = field_text(&Calc::Currency { amount, currency }, picture, None)
                {
                    return shown;
                }
                match currency.map(|c| c.code).filter(|c| *c != "USD" && *c != "SYS") {
                    Some(code) => format!("{} {code}", grouped(amount, 2)),
                    None => format!("${}", grouped(amount, 2)),
                }
            }
            _ => text.to_owned(),
        },
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------------- reading what is typed

/// A value as a cell holds it: `V`, and `U` when the kind has one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stored {
    v: String,
    u: Option<String>,
}

impl Stored {
    fn of(v: impl Into<String>, u: Option<&str>) -> Stored {
        Stored { v: v.into(), u: u.map(str::to_owned) }
    }

    /// A field with no value: an empty text, a string's with `STR`, and a date's the plain 0 with no unit.
    fn clear(kind: Kind) -> Stored {
        match kind {
            Kind::Date => Stored::of("0", None),
            Kind::String | Kind::FixedList | Kind::VariableList => Stored::of("", Some("STR")),
            Kind::Number | Kind::Boolean | Kind::Duration | Kind::Currency => Stored::of("", None),
        }
    }

    fn encoded(value: &Calc, text: &str, what: &str) -> Result<Stored, String> {
        encode(value).map(|e| Stored { v: e.v, u: e.u }).ok_or_else(|| format!("{} is not {what}", q(text)))
    }
}

/// A date and a time as `YYYY-MM-DDThh:mm:ss`, the forms a person types: `2026-03-04`, `2026-03-04 14:30` and `3/4/2026 2:30 pm`. The calendar decides whether it is a day.
fn date_text(text: &str) -> Option<String> {
    let t = text.trim();
    let (date, time) = match t.split_once(['T', ' ']) {
        Some((d, rest)) => (d, rest.trim()),
        None => (t, ""),
    };
    let number = |s: &str, digits: usize| -> Option<u32> {
        (!s.is_empty() && s.len() <= digits && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
    };
    let (y, m, d) = if date.contains('-') {
        let mut parts = date.split('-');
        let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || y.len() != 4 {
            return None;
        }
        (number(y, 4)?, number(m, 2)?, number(d, 2)?)
    } else if date.contains('/') {
        let mut parts = date.split('/');
        let (m, d, y) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || y.len() != 4 {
            return None;
        }
        (number(y, 4)?, number(m, 2)?, number(d, 2)?)
    } else {
        return None;
    };
    let (mut hour, mut minute, mut second) = (0u32, 0u32, 0u32);
    if !time.is_empty() {
        let lower = time.to_ascii_lowercase();
        let (clock, meridiem) = match lower.strip_suffix("pm") {
            Some(c) => (c.trim(), Some(true)),
            None => match lower.strip_suffix("am") {
                Some(c) => (c.trim(), Some(false)),
                None => (lower.trim(), None),
            },
        };
        let mut parts = clock.split(':');
        hour = number(parts.next()?, 2)?;
        minute = number(parts.next()?, 2)?;
        second = match parts.next() {
            Some(s) => number(s, 2)?,
            None => 0,
        };
        if parts.next().is_some() || minute > 59 || second > 59 {
            return None;
        }
        match meridiem {
            Some(pm) => {
                if !(1..=12).contains(&hour) {
                    return None;
                }
                hour = hour % 12 + if pm { 12 } else { 0 };
            }
            None if hour > 23 => return None,
            None => {}
        }
    }
    Some(format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}"))
}

/// A duration: a number and a unit word (days with none).
fn duration_of(text: &str) -> Option<Quantity> {
    let t = text.trim();
    let split = t.find(|c: char| c.is_alphabetic()).unwrap_or(t.len());
    let (number, word) = t.split_at(split);
    let n = parse_decimal(number.trim())?;
    let unit = match word.trim().trim_end_matches('.').to_ascii_lowercase().as_str() {
        "" | "d" | "day" | "days" | "ed" => Unit::ElapsedDay,
        "h" | "hr" | "hrs" | "hour" | "hours" | "eh" => Unit::ElapsedHour,
        "m" | "min" | "mins" | "minute" | "minutes" | "em" => Unit::ElapsedMinute,
        "s" | "sec" | "secs" | "second" | "seconds" | "es" => Unit::ElapsedSecond,
        "w" | "wk" | "wks" | "week" | "weeks" | "ew" => Unit::ElapsedWeek,
        _ => return None,
    };
    Some(Quantity::from_value(n, unit))
}

/// An amount and a currency: a leading symbol, thousands commas and a trailing three-letter code are read.
fn currency_of(text: &str) -> Option<(f64, Option<&'static diacraft_formula::Currency>)> {
    let mut t = text.trim();
    let mut currency = None;
    if t.chars().count() > 3 {
        let tail: String = t.chars().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        if tail.chars().all(|c| c.is_ascii_alphabetic()) {
            currency = Some(diacraft_formula::currency_by_code(&tail.to_ascii_uppercase())?);
            t = t.get(..t.len() - tail.len())?.trim_end();
        }
    }
    let t = t.trim_start_matches(['$', '\u{20ac}', '\u{a3}', '\u{a5}']).trim();
    let cleaned: String = t.chars().filter(|c| *c != ',').collect();
    Some((parse_decimal(&cleaned)?, currency))
}

/// What `text` is as a value of `kind`, as the cell holds it; `now` is the cell as it is (its unit and its value, for a number that takes the unit the cell has and a currency that
/// keeps its code), and `items` the list's. The refusal is in words.
fn coerce(kind: Kind, text: &str, unit_now: Option<&str>, value_now: &str, items: &[String]) -> Result<Stored, String> {
    if text.trim().is_empty() && !matches!(kind, Kind::String | Kind::FixedList | Kind::VariableList) || text.is_empty() {
        return Ok(Stored::clear(kind));
    }
    match kind {
        Kind::String | Kind::VariableList => Ok(Stored::of(text, Some("STR"))),
        Kind::FixedList => {
            if items.iter().any(|i| i == text) {
                Ok(Stored::of(text, Some("STR")))
            } else if items.is_empty() {
                Err(format!("{} is not an item of this list: the list has no items", q(text)))
            } else {
                Err(format!("{} is not one of the items of this list: {}", q(text), items.join(", ")))
            }
        }
        Kind::Number => {
            let said = "a number (a number, or a number with a unit: 12.5, 3 in, 1/8 in)";
            let mut quantity = read_quantity(text)
                .filter(|_| !matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "false"))
                .ok_or_else(|| format!("{} is not {said}", q(text)))?;
            if quantity.unit == Unit::Unitless
                && let Some(unit) = unit_now.and_then(parse_unit).filter(|u| !matches!(u.measure(), Measure::None | Measure::Date))
            {
                quantity = Quantity::from_value(quantity.stored, unit);
            }
            Stored::encoded(&Calc::Number(quantity), text, said)
        }
        Kind::Boolean => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(Stored::of("1", Some("BOOL"))),
            "false" | "no" | "off" | "0" => Ok(Stored::of("0", Some("BOOL"))),
            _ => Err(format!("{} is not true or false (true, false, yes, no, on, off, 1 or 0)", q(text))),
        },
        Kind::Date => {
            let said = "a date (2026-03-04, 3/4/2026, and a time may follow: 3/4/2026 2:30 pm)";
            let serial = date_text(text).and_then(|iso| iso_to_serial(&iso)).ok_or_else(|| format!("{} is not {said}", q(text)))?;
            Stored::encoded(&Calc::date(serial), text, said)
        }
        Kind::Duration => {
            let said = "a duration (a number and a unit: 1.5, 36 hours, 2 d, 30 min, 1 week)";
            let quantity = duration_of(text).ok_or_else(|| format!("{} is not {said}", q(text)))?;
            Stored::encoded(&Calc::Number(quantity), text, said)
        }
        Kind::Currency => {
            let said = "a currency amount ($1,234.50, 12.5, 12.50 EUR)";
            let (amount, code) = currency_of(text).ok_or_else(|| format!("{} is not {said}", q(text)))?;
            let kept = match decode(value_now, Some("CY"), None) {
                Calc::Currency { currency, .. } => currency,
                _ => None,
            };
            Stored::encoded(&Calc::Currency { amount, currency: code.or(kept) }, text, said)
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------------- what a call is given

/// What a field is given in a list: the cells it names. A field that names a `row` is that row and the cells it leaves out stay; a field with no `row` is a new row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldSpec {
    pub row: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub kind: Option<Kind>,
    pub format: Option<String>,
    pub value: Option<String>,
    pub prompt: Option<String>,
    pub sort_key: Option<String>,
    pub invisible: Option<bool>,
    pub ask: Option<bool>,
    /// A language tag ([`is_language_tag`]).
    pub lang_id: Option<String>,
    /// A calendar's number, 0 to 13 ([`CALENDARS`]).
    pub calendar: Option<u32>,
}

/// What a call made of the shape's fields, for the command's answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Whether anything changed.
    pub changed: bool,
    /// How many values were set (for `shapedata.set`).
    pub values: usize,
    /// What a change of type left as it was, in words.
    pub notes: Vec<String>,
}

/// Whether `c` may not be in a label, a name, a prompt, a format or a sort key: the controls, and the invisible and direction marks that make a text read as another.
fn is_hidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

fn check_text(what: &str, text: &str, most: usize, lines: bool) -> Result<(), Problem> {
    if text.chars().take(most + 1).count() > most {
        return Err(Problem::bad(format!("`{what}` must be at most {most} characters")));
    }
    if text.chars().any(|c| is_hidden(c) && !(lines && matches!(c, '\n' | '\t'))) {
        return Err(Problem::bad(format!("`{what}` must not hold control characters or invisible marks")));
    }
    Ok(())
}

/// Whether `name` is a name a formula can write after `Prop.`: letters, digits and `_`, not starting with a digit.
fn writable(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// A row name made of a label.
fn name_of(label: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in label.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out.chars().take(MAX_NAME_CHARS).collect()
}

/// The first of `base`, `base_2`, `base_3`... that `taken` (lower case) does not hold.
fn free_name(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(&base.to_lowercase()) {
        return base.to_owned();
    }
    let room = MAX_NAME_CHARS.saturating_sub(12);
    let base: String = base.chars().take(room).collect();
    let mut n = 2u64;
    loop {
        let name = format!("{base}_{n}");
        if !taken.contains(&name.to_lowercase()) {
            return name;
        }
        n += 1;
    }
}

/// What a field is to be after the call: the cells it will hold.
struct Final {
    field: Field,
    is_new: bool,
    /// The value the call writes, `None` to leave the cell as it is.
    value: Option<Stored>,
}

/// The cells one write changes: what to write and where.
type Write = (SheetRef, CellWrite);

fn cell_write(sheet: &SheetRef, row: &str, cell: &str, edit: CellEdit, origin: WriteOrigin) -> Write {
    (sheet.clone(), CellWrite { addr: CellAddr::in_row("Property", None, RowKey::name(row), cell), edit, origin })
}

fn flag_text(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// What the fields of the shape are as the call begins, and what it has used of them.
struct Plan<'a> {
    current: &'a Listed,
    by_row: HashMap<&'a str, &'a Field>,
    /// The names of the rows (lower case) that are taken, the shape's own and those the call makes.
    taken: HashSet<String>,
    /// The rows the list names.
    seen: HashSet<String>,
    /// Whether the list is the shape's whole list.
    whole: bool,
    notes: Vec<String>,
}

impl Plan<'_> {
    /// The row a field names, which the shape must have, once, and which is not renamed.
    fn existing(&mut self, spec: &FieldSpec, row: &str, n: usize) -> Result<Field, Problem> {
        let found = self.by_row.get(row).copied().ok_or_else(|| {
            let have: Vec<&str> = self.current.fields.iter().take(20).map(|f| f.row.as_str()).collect();
            Problem::bad(format!(
                "field {n}: the shape has no shape data row named {} (it has {})",
                q(row),
                if have.is_empty() { "none".to_owned() } else { have.join(", ") }
            ))
        })?;
        if !self.seen.insert(row.to_owned()) {
            return Err(Problem::bad(format!("field {n}: the row {} is named twice", q(row))));
        }
        if let Some(name) = spec.name.as_deref().filter(|name| *name != row) {
            return Err(Problem::bad(format!(
                "field {n}: a field is not renamed (formulas read it as Prop.{row}): the name {} is not given to it; change its label instead",
                q(name)
            )));
        }
        Ok(found.clone())
    }

    /// The name of a new row.
    fn new_name(&mut self, spec: &FieldSpec, n: usize) -> Result<String, Problem> {
        let name = match &spec.name {
            Some(name) => {
                if !writable(name) {
                    return Err(Problem::bad(format!(
                        "field {n}: the name {} is not a name a formula can use (letters, digits and underscores, not starting with a digit)",
                        q(name)
                    )));
                }
                if self.taken.contains(&name.to_lowercase()) {
                    return Err(Problem::bad(format!("field {n}: the name {} is already used by another field", q(name))));
                }
                name.clone()
            }
            None => match spec.label.as_deref().map(name_of).filter(|b| !b.is_empty()) {
                Some(base) => free_name(&base, &self.taken),
                None => free_name_numbered("Property", &self.taken),
            },
        };
        self.taken.insert(name.to_lowercase());
        Ok(name)
    }

    /// A field the list makes: blank, a string.
    fn made(&mut self, spec: &FieldSpec, n: usize) -> Result<Field, Problem> {
        let name = self.new_name(spec, n)?;
        let label = spec.label.as_deref().map(str::trim).filter(|l| !l.is_empty()).map_or_else(|| name.clone(), str::to_owned);
        Ok(Field {
            row: name,
            label,
            kind: Kind::String,
            known: true,
            type_code: Some(0),
            format: String::new(),
            value: String::new(),
            unit: Some("STR".to_owned()),
            formula: None,
            shown: String::new(),
            prompt: String::new(),
            sort_key: String::new(),
            invisible: false,
            ask: false,
            lang_id: None,
            calendar: None,
            items: Vec::new(),
        })
    }

    /// The field as the list has it: the row it names, or a new one, and the cells it gives.
    fn field(&mut self, spec: &FieldSpec, n: usize) -> Result<Final, Problem> {
        let said = |p: Problem| Problem { message: format!("field {n}: {}", p.message), ..p };
        check_spec_texts(spec).map_err(said)?;
        let (mut field, is_new) = match &spec.row {
            Some(row) => (self.existing(spec, row, n)?, false),
            None => (self.made(spec, n)?, true),
        };
        let was_kind = field.kind;
        give_cells(&mut field, spec, is_new);
        if field.kind.is_list() && field.format.split(';').filter(|i| !i.is_empty()).count() > MAX_ITEMS {
            return Err(Problem::bad(format!("field {n}: a list holds at most {MAX_ITEMS} items")));
        }
        field.items = if field.kind.is_list() { items_of(&field.format) } else { Vec::new() };
        let value = self.value(&field, spec, is_new, was_kind, n)?;
        join_list(&mut field, value.as_ref(), n)?;
        Ok(Final { field, is_new, value })
    }

    /// The value a field is to be written: the one given, else the one it has read again as a new type.
    fn value(&mut self, field: &Field, spec: &FieldSpec, is_new: bool, was_kind: Kind, n: usize) -> Result<Option<Stored>, Problem> {
        if let Some(text) = &spec.value {
            let stored = coerce(field.kind, text, field.unit.as_deref(), &field.value, &field.items).map_err(|m| {
                if self.whole {
                    Problem::bad(format!("field {n} ({}): {m}", q(&field.row)))
                } else {
                    Problem::bad(format!("the value of {}: {m}", q(&field.row)))
                }
            })?;
            return Ok(Some(stored));
        }
        // A field with no value, or a date never set, has no value of the new type either.
        let none = field.value.is_empty() || is_unset_date(was_kind, &field.value, field.unit.as_deref());
        if is_new || (field.kind != was_kind && none) {
            return Ok(Some(Stored::clear(field.kind)));
        }
        if field.kind == was_kind {
            return Ok(None);
        }
        match coerce(field.kind, &field.value, field.unit.as_deref(), &field.value, &field.items) {
            Ok(stored) => Ok(Some(stored)),
            Err(_) => {
                self.notes.push(format!(
                    "the value {} of {} does not read as a {}: it is kept as it was",
                    q(&field.value),
                    q(&field.row),
                    field.kind.label().to_lowercase()
                ));
                Ok(None)
            }
        }
    }
}

/// The first of `Base1`, `Base2`... that `taken` (lower case) does not hold.
fn free_name_numbered(base: &str, taken: &HashSet<String>) -> String {
    let mut k = 1u64;
    loop {
        let name = format!("{base}{k}");
        if !taken.contains(&name.to_lowercase()) {
            return name;
        }
        k += 1;
    }
}

/// The texts a field is given are checked before anything is built.
fn check_spec_texts(spec: &FieldSpec) -> Result<(), Problem> {
    for (what, text, most, lines) in [
        ("label", &spec.label, MAX_TEXT_CHARS, false),
        ("name", &spec.name, MAX_NAME_CHARS, false),
        ("format", &spec.format, MAX_VALUE_CHARS, false),
        ("value", &spec.value, MAX_VALUE_CHARS, true),
        ("prompt", &spec.prompt, MAX_TEXT_CHARS, true),
        ("sort_key", &spec.sort_key, MAX_TEXT_CHARS, false),
    ] {
        if let Some(text) = text {
            check_text(what, text, most, lines)?;
        }
    }
    if let Some(tag) = spec.lang_id.as_deref().filter(|t| !is_language_tag(t)) {
        return Err(Problem::bad(format!("{} {LANG_WORDS}", q(tag))));
    }
    if let Some(n) = spec.calendar.filter(|n| calendar_name(*n).is_none()) {
        return Err(Problem::bad(format!("the calendar {n} is not one of the calendars: {CALENDAR_WORDS}")));
    }
    Ok(())
}

/// What a language must be, in the words of a refusal.
pub const LANG_WORDS: &str = "is not a language tag such as en-US: letters and digits apart at hyphens, the first part 2 to 8 letters and each other at most 8, at most 64 characters";
/// What a calendar must be, in the words of a refusal.
pub const CALENDAR_WORDS: &str = "a whole number from 0 to 13, as shapedata.types lists them (0 is Western)";

/// The cells a field is given in a list, over the field as it was.
fn give_cells(field: &mut Field, spec: &FieldSpec, is_new: bool) {
    if !is_new && let Some(label) = &spec.label {
        field.label = label.trim().to_owned();
    }
    if let Some(kind) = spec.kind {
        field.kind = kind;
        field.known = true;
        field.type_code = Some(i64::from(kind.code()));
    }
    if let Some(format) = &spec.format {
        field.format.clone_from(format);
    }
    if let Some(prompt) = &spec.prompt {
        field.prompt.clone_from(prompt);
    }
    if let Some(sort_key) = &spec.sort_key {
        field.sort_key.clone_from(sort_key);
    }
    field.invisible = spec.invisible.unwrap_or(field.invisible);
    field.ask = spec.ask.unwrap_or(field.ask);
    if let Some(tag) = &spec.lang_id {
        field.lang_id = Some(tag.clone());
    }
    field.calendar = spec.calendar.or(field.calendar);
}

/// A variable list takes a new value into its items, at the end.
fn join_list(field: &mut Field, value: Option<&Stored>, n: usize) -> Result<(), Problem> {
    let Some(stored) = value.filter(|s| field.kind == Kind::VariableList && !s.v.is_empty() && !field.items.contains(&s.v)) else {
        return Ok(());
    };
    if field.items.len() >= MAX_ITEMS {
        return Err(Problem::bad(format!("field {n}: a list holds at most {MAX_ITEMS} items")));
    }
    field.items.push(stored.v.clone());
    field.format = field.items.join(";");
    check_text("format", &field.format, MAX_VALUE_CHARS, false).map_err(|p| Problem { message: format!("field {n}: {}", p.message), ..p })
}

/// What a call writes: the cell writes, the units to put on the `Value` cells after, and how many values are written.
#[derive(Default)]
struct Queue {
    writes: Vec<Write>,
    unit_patches: Vec<(String, Option<String>)>,
    values: usize,
}

impl Queue {
    /// The cells of a new row.
    fn made(&mut self, sheet: &SheetRef, f: &Final) {
        let (field, row) = (&f.field, f.field.row.as_str());
        let text_cell = |s: &str| Cell::constant(Value::from_text(s));
        let value = f.value.clone().unwrap_or_else(|| Stored::clear(field.kind));
        let mut value_cell = text_cell(&value.v);
        if let Some(u) = &value.u {
            value_cell = value_cell.with_unit(u);
        }
        let mut put = |cell: &str, c: Cell, origin: WriteOrigin| self.writes.push(cell_write(sheet, row, cell, CellEdit::Cell(c), origin));
        put("Value", value_cell, WriteOrigin::AutomationForce);
        put("Label", text_cell(&field.label), WriteOrigin::Drawing);
        put("Prompt", text_cell(&field.prompt), WriteOrigin::Drawing);
        put("Type", text_cell(&field.kind.code().to_string()), WriteOrigin::Drawing);
        put("Format", text_cell(&field.format), WriteOrigin::Drawing);
        put("SortKey", text_cell(&field.sort_key), WriteOrigin::Drawing);
        put("Invisible", text_cell(flag_text(field.invisible)), WriteOrigin::Drawing);
        put("Verify", text_cell(flag_text(field.ask)), WriteOrigin::Drawing);
        if let Some(lang) = &field.lang_id {
            put("LangID", text_cell(lang), WriteOrigin::Drawing);
        }
        // Every new field holds a calendar, 0 unless one is given.
        put("Calendar", text_cell(&field.calendar.unwrap_or(0).to_string()), WriteOrigin::Drawing);
        self.values += usize::from(f.value.as_ref().is_some_and(|v| !v.v.is_empty()));
    }

    /// The cells of a row the shape has that differ from what it reads now.
    fn changed(&mut self, sheet: &SheetRef, f: &Final, now: &Field) {
        let (field, row) = (&f.field, f.field.row.as_str());
        let mut put = |cell: &str, text: &str| {
            self.writes.push(cell_write(sheet, row, cell, CellEdit::Value(Value::from_text(text)), WriteOrigin::Drawing));
        };
        if field.label != now.label {
            put("Label", &field.label);
        }
        if field.type_code != now.type_code
            && let Some(code) = field.type_code
        {
            put("Type", &code.to_string());
        }
        if field.format != now.format {
            put("Format", &field.format);
        }
        if field.prompt != now.prompt {
            put("Prompt", &field.prompt);
        }
        if field.sort_key != now.sort_key {
            put("SortKey", &field.sort_key);
        }
        if field.invisible != now.invisible {
            put("Invisible", flag_text(field.invisible));
        }
        if field.ask != now.ask {
            put("Verify", flag_text(field.ask));
        }
        if field.lang_id != now.lang_id
            && let Some(lang) = &field.lang_id
        {
            put("LangID", lang);
        }
        if field.calendar != now.calendar
            && let Some(calendar) = field.calendar
        {
            put("Calendar", &calendar.to_string());
        }
        let Some(value) = &f.value else { return };
        // The same text again is no write, unless it changes the unit or removes a formula (an entered value replaces any formula).
        if value.v == now.value && now.formula.is_none() && value.u == now.unit {
            return;
        }
        self.writes.push(cell_write(sheet, row, "Value", CellEdit::Value(Value::from_text(&value.v)), WriteOrigin::AutomationForce));
        // The write keeps the unit the row's own cell has, and a row that is the master's has none of its own yet: the unit is put on the cell as it is now.
        self.unit_patches.push((row.to_owned(), value.u.clone()));
        self.values += 1;
    }
}

/// Takes the rows `gone` away from the shape: a row of its own is removed, and one it takes from its master is marked deleted here.
fn take_away(doc: &mut Document, page: u32, at: &ShapeRef, gone: &[&Field]) -> Result<(), Problem> {
    let names: HashSet<&str> = gone.iter().map(|f| f.row.as_str()).collect();
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    if shape.sheet.section(&key()).is_some() {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = sheet.section_mut(&key()).map_err(|e| Problem::failed(e.to_string()))?;
        section.rows.retain(|row| !row.name.as_deref().is_some_and(|n| names.contains(n)));
    }
    let still: HashSet<String> = read_at(doc, at)?.fields.into_iter().map(|f| f.row).collect();
    for field in gone.iter().filter(|f| still.contains(&f.row)) {
        let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        let row = Arc::make_mut(&mut shape.sheet).row_mut(&key(), &RowKey::name(&field.row)).map_err(|e| Problem::failed(e.to_string()))?;
        row.del = true;
    }
    Ok(())
}

/// Puts the units on the `Value` cells the writes made, and takes a section that has no row any more away.
fn finish_cells(doc: &mut Document, page: u32, at: &ShapeRef, patches: Vec<(String, Option<String>)>) -> Result<(), Problem> {
    for (row, unit) in patches {
        let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        let found = Arc::make_mut(&mut shape.sheet).row_mut(&key(), &RowKey::name(&row)).map_err(|e| Problem::failed(e.to_string()))?;
        if let Some(cell) = found.cells.get_mut(&CellName::new("Value")) {
            cell.unit = unit.map(|u| Arc::from(u.as_str()));
        }
    }
    // A section left with no row is no section.
    if let Some(shape) = shape_mut(doc, page, &at.path)
        && shape.sheet.section(&key()).is_some_and(|s| s.rows.is_empty() && s.cells.is_empty())
    {
        Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "Property");
    }
    Ok(())
}

/// Applies a list of fields to shape `id` on a page. With `whole` the list is the shape's whole list and a row it leaves out is taken away; without, only the rows it names change.
/// Refused, with the document as it was, for a row the shape lacks, a row named twice, text a field may not hold, a value that is no value of its type, more than [`MAX_FIELDS`] fields and a cell a guard
/// protects (a `Value` cell is written as an entered value is, past a guard).
pub fn apply(doc: &mut Document, page: u32, id: u32, specs: &[FieldSpec], whole: bool) -> Result<Applied, Problem> {
    if specs.len() > MAX_FIELDS {
        return Err(Problem::bad(format!("a shape holds at most {MAX_FIELDS} shape data fields")));
    }
    let at = require_shape(doc, page, id)?;
    let current = read_at(doc, &at)?;
    let mut plan = Plan {
        current: &current,
        by_row: current.fields.iter().map(|f| (f.row.as_str(), f)).collect(),
        taken: current.fields.iter().map(|f| f.row.to_lowercase()).collect(),
        seen: HashSet::new(),
        whole,
        notes: Vec::new(),
    };
    let mut finals: Vec<Final> = Vec::with_capacity(specs.len());
    for (n, spec) in specs.iter().enumerate() {
        finals.push(plan.field(spec, n + 1)?);
    }
    let gone: Vec<&Field> = if whole { current.fields.iter().filter(|f| !plan.seen.contains(&f.row)).collect() } else { Vec::new() };
    // The writes, in the order of the list.
    let sheet = SheetRef::Shape(at.clone());
    let mut queue = Queue::default();
    for f in &finals {
        match (f.is_new, plan.by_row.get(f.field.row.as_str())) {
            (true, _) => queue.made(&sheet, f),
            (false, Some(now)) => queue.changed(&sheet, f, now),
            (false, None) => {}
        }
    }
    let notes = std::mem::take(&mut plan.notes);
    if queue.writes.is_empty() && gone.is_empty() {
        return Ok(Applied { changed: false, values: 0, notes });
    }
    // The rows left out go first.
    if !gone.is_empty() {
        take_away(doc, page, &at, &gone)?;
    }
    let touched: Vec<(SheetRef, CellAddr)> = queue
        .writes
        .iter()
        .map(|(s, w)| (s.clone(), w.addr.clone()))
        .chain(gone.iter().map(|f| (sheet.clone(), CellAddr::in_row("Property", None, RowKey::name(&f.row), "Value"))))
        .collect();
    let Queue { writes, unit_patches, values } = queue;
    let outcomes = doc.write_batch(writes).map_err(Problem::from)?;
    if outcomes.contains(&WriteOutcome::KeptGuarded) {
        return Err(Problem::failed("a cell of the field is protected by GUARD and cannot be changed here"));
    }
    finish_cells(doc, page, &at, unit_patches)?;
    super::settle::settle_cells(doc, page, touched)?;
    Ok(Applied { changed: true, values, notes })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_eight_types_have_their_numbers_and_words() {
        for (n, kind) in Kind::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(kind.code()), n);
            assert_eq!(Kind::from_code(n as i64), Some(kind));
            assert_eq!(Kind::from_word(kind.word()), Some(kind));
            assert_eq!(Kind::from_word(&kind.label().to_uppercase()), Some(kind), "the dialog's label, in any case");
        }
        assert_eq!(Kind::from_code(8), None);
        assert_eq!(Kind::from_code(-1), None);
    }

    #[test]
    fn a_type_cell_is_a_whole_number_or_of_an_unknown_type() {
        assert_eq!(type_number(5.0), Some(5));
        assert_eq!(type_number(-1.0), Some(-1));
        assert_eq!(type_number(-0.0), Some(0));
        for n in [2.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0e300, 9_007_199_254_740_994.0] {
            assert_eq!(type_number(n), None, "{n}");
        }
        assert_eq!(type_number(2.5).and_then(Kind::from_code), None, "not the 3 it rounds to");
    }

    #[test]
    fn a_language_is_a_tag_and_a_calendar_one_of_fourteen() {
        for tag in ["en-US", "de-DE", "zh-Hant-TW", "es-419", "en", "sgn-BE-FR", "abcdefgh"] {
            assert!(is_language_tag(tag), "{tag}");
        }
        for bad in ["", "e", "-en", "en-", "en--US", "en US", "en_US", "1033", "abcdefghi", "en-abcdefghi", "\u{e9}n-US", &"en-".repeat(30)] {
            assert!(!is_language_tag(bad), "{bad}");
        }
        assert_eq!(
            (calendar_name(0), calendar_name(4), calendar_name(12), calendar_name(13)),
            (Some("Western"), Some("Japanese Emperor"), Some("Gregorian Arabic"), Some("Um-al-Qura"))
        );
        assert_eq!(calendar_name(14), None);
        assert_eq!(calendar_name(u32::MAX), None);
    }

    #[test]
    fn a_date_never_set_is_a_plain_zero() {
        assert!(is_unset_date(Kind::Date, "0", None));
        assert!(is_unset_date(Kind::Date, " 0 ", Some("")));
        assert!(!is_unset_date(Kind::Date, "0", Some("DATE")), "the serial 0 with its unit is set");
        assert!(!is_unset_date(Kind::Date, "2026-03-04T00:00:00", Some("DATE")));
        assert!(!is_unset_date(Kind::Date, "1", None));
        assert!(!is_unset_date(Kind::Currency, "0", None), "the rule is a date's");
        assert_eq!(Stored::clear(Kind::Date), Stored::of("0", None));
        assert_eq!(shown_of(Kind::Date, "0", None, ""), "");
        assert_eq!(shown_of(Kind::Date, "0", Some("DATE"), ""), "12/30/1899");
    }

    #[test]
    fn a_name_is_made_of_a_label() {
        assert_eq!(name_of("Cost"), "Cost");
        assert_eq!(name_of("  Cost / unit (EUR) "), "Cost_unit_EUR");
        assert_eq!(name_of("7 days"), "_7_days");
        assert_eq!(name_of("???"), "");
        assert_eq!(name_of("a__b"), "a__b");
        assert_eq!(name_of(&"x".repeat(500)).chars().count(), MAX_NAME_CHARS);
        let taken: HashSet<String> = ["cost".to_owned(), "cost_2".to_owned()].into();
        assert_eq!(free_name("Cost", &taken), "Cost_3", "ignoring case");
        assert_eq!(free_name("Other", &taken), "Other");
    }

    #[test]
    fn dates_are_read_in_the_forms_a_person_types() {
        assert_eq!(date_text("2026-03-04").as_deref(), Some("2026-03-04T00:00:00"));
        assert_eq!(date_text("2026-03-04 14:30").as_deref(), Some("2026-03-04T14:30:00"));
        assert_eq!(date_text("2026-03-04T14:30:15").as_deref(), Some("2026-03-04T14:30:15"));
        assert_eq!(date_text("3/4/2026").as_deref(), Some("2026-03-04T00:00:00"));
        assert_eq!(date_text("3/4/2026 2:30 pm").as_deref(), Some("2026-03-04T14:30:00"));
        assert_eq!(date_text("3/4/2026 12:05 AM").as_deref(), Some("2026-03-04T00:05:00"));
        assert_eq!(date_text("3/4/2026 12:05 PM").as_deref(), Some("2026-03-04T12:05:00"));
        for bad in [
            "",
            "3/4/26",
            "2026-3",
            "2026-03-04-05",
            "3/4/2026 25:00",
            "3/4/2026 13:00 pm",
            "3/4/2026 1:61",
            "a/b/cccc",
            "2026/03/04 x",
            "99999-01-01",
        ] {
            assert_eq!(date_text(bad), None, "{bad}");
        }
        // The calendar says whether the day is one.
        assert!(iso_to_serial(&date_text("2026-02-30").unwrap()).is_none());
        assert!(iso_to_serial(&date_text("2/29/2028").unwrap()).is_some(), "2028 is a leap year");
        assert!(iso_to_serial(&date_text("2/29/2027").unwrap()).is_none());
    }

    #[test]
    fn durations_and_currencies_are_read_with_their_words() {
        let days = |t: &str| duration_of(t).map(|q| q.stored);
        assert_eq!(days("2"), Some(2.0));
        assert_eq!(days("1 week"), Some(7.0));
        assert_eq!(days("48 h"), Some(2.0));
        assert_eq!(duration_of("3 fortnights"), None);
        assert_eq!(duration_of("x"), None);
        assert_eq!(duration_of(""), None);
        assert_eq!(currency_of("$1,234.5").map(|c| c.0), Some(1234.5));
        assert_eq!(currency_of("12.50 EUR").and_then(|c| c.1).map(|c| c.code), Some("EUR"));
        assert_eq!(currency_of("12.50 ZZZ"), None, "a code nobody lists");
        assert_eq!(currency_of("lots"), None);
        assert_eq!(currency_of("-5").map(|c| c.0), Some(-5.0));
    }

    #[test]
    fn numbers_show_with_thousands_marks() {
        assert_eq!(grouped(1234.5, 2), "1,234.50");
        assert_eq!(grouped(0.0, 2), "0.00");
        assert_eq!(grouped(999.0, 0), "999");
        assert_eq!(grouped(1000.0, 0), "1,000");
        assert_eq!(grouped(1_234_567.891, 2), "1,234,567.89");
        assert_eq!(grouped(-1234.5, 2), "-1,234.50");
        assert_eq!(grouped(-0.001, 2), "0.00", "no minus sign on a zero");
    }

    #[test]
    fn a_text_is_cut_and_charged_before_it_is_kept() {
        let mut spent = 0;
        assert_eq!(take("hello", 3, &mut spent).unwrap(), "hel");
        assert_eq!(spent, 3);
        let mut spent = MAX_READ_BYTES - 2;
        assert!(take("hello", 10, &mut spent).is_err());
        let mut spent = 0;
        assert_eq!(take("\u{e9}\u{e9}\u{e9}", 2, &mut spent).unwrap(), "\u{e9}\u{e9}", "cut where a character ends");
        assert_eq!(spent, 4);
    }
}
