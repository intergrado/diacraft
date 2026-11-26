//! What a built-in master wears besides its outline: a look, a text, Shape Data, character formats, `User` rows, locks and a role.
//!
//! `orgchart::decorate` did this by hand for six masters. A [`Dress`] is the same as data, so that a family module is mostly tables (`docs/editing-commands.md`, "DiaCraft's own stencils,
//! second batch"). **Every cell goes on the master's shape**, and an instance inherits it; the one thing a drop adds is the shown form of a text that holds fields (a
//! field's shown form lives in the text that holds it, [`own_text`]).
//!
//! A formula a dress gives has no value in the table: the master's build settles it by one recalculation of the master alone (`settle_values`), so a cached value is always what the formula
//! gives at the master's default Shape Data, and a table never repeats a number a formula computes.
#![allow(dead_code, reason = "the vocabulary a family's table is written in (STENCIL2): a variant is used when the family that needs it lands")]

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Sheet, Text, TextPart, Value};

use super::container::{HEADING_ROW, LOCK_ROW, MARGIN_ROW, RESIZE_ROW, Side};
use super::look::Flag;
use super::{length, plain, put};

/// The extras of a master: `More::NONE` for a plain one.
pub(crate) struct More {
    /// What the master wears.
    pub dress: Option<&'static Dress>,
    /// A master that the window and the listing leave out, as a stencil file's hidden master is: the custom line end is one.
    pub hidden: bool,
    /// The ids of other built-in masters that [`super::ensure_master`] copies in first (a composition connector brings its line end).
    pub brings: &'static [&'static str],
}

impl More {
    /// A master with no extras: a master of the first seven stencils that people do not join, and the stubs of the second batch.
    pub(crate) const NONE: More = More { dress: None, hidden: false, brings: &[] };

    /// A master that holds `ObjType` 1 and nothing else ([`Dress::PLACEABLE`]): a shape people join.
    pub(crate) const PLACEABLE: More = More { dress: Some(&Dress::PLACEABLE), hidden: false, brings: &[] };
}

/// One `Character` row: the size in points, the style bits (1 bold, 2 italic, 4 underline) and a colour formula (the style's when `None`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CharRow {
    pub size_pt: f64,
    pub style: u8,
    pub color: Option<&'static str>,
}

/// One field of a text: the formula of its `Field` row, the form it shows at the master's default Shape Data, the `Character` row its run uses and whether a line break follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldText {
    pub formula: &'static str,
    pub shown: &'static str,
    pub char_row: u32,
    pub line_after: bool,
}

/// The text of a master.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TextSpec {
    /// No text.
    None,
    /// Placeholder text, one run in the first character format.
    Plain(&'static str),
    /// Fields that show Shape Data, each in its own character format.
    Fields(&'static [FieldText]),
}

/// What a Shape Data field holds. A fixed list's text is its items, apart at semicolons.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FieldKind {
    String,
    FixedList(&'static str),
    Boolean,
    Number,
}

/// One Shape Data row: its name, its label, its kind, its default (text for a string and a list item, `1` or `0` for a boolean, a number's digits) and its prompt.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldDef {
    pub row: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub value: &'static str,
    pub prompt: &'static str,
}

/// What a part of a structure is: a drop of it goes into the structure and is no shape of its outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    Swimlane,
    Separator,
    Pool,
    Lane,
}

/// What a master is for the drawing.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Role {
    /// An ordinary shape.
    Shape,
    /// A container: typed `User.msvStructureType` `Container`, with its heading side and margin, resizing as needed and no style row, so that the container commands leave its outline alone.
    Container { heading: Side, margin_in: f64 },
    /// A part of a structure (a Swimlane, a Separator, BPMN's Pool and Lane).
    Part(PartKind),
    /// A shape no connector joins: a title, a key. It has no connection points.
    Pointless,
}

/// The dress of a master. Every field that is `None` or empty leaves the master's cell as the style gives it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dress {
    /// `FillForegnd`, a formula (a colour: `THEMEVAL("AccentColor",RGB(58,127,192))`).
    pub fill: Option<&'static str>,
    /// `FillPattern`: 0 for no fill, 1 solid. A dress with a fill formula and no pattern writes 1.
    pub fill_pattern: Option<u8>,
    pub line_weight_pt: Option<f64>,
    /// `LinePattern`: `Off` is 0 (no line), `On` is 1 (solid), a formula is a formula (`GUARD(2)`, `4`).
    pub line_pattern: Option<Flag>,
    pub text: TextSpec,
    /// The `Character` rows, in order.
    pub chars: &'static [CharRow],
    /// The Shape Data rows, in order.
    pub data: &'static [FieldDef],
    /// `User` rows of ours: a name and the text it holds.
    pub user: &'static [(&'static str, &'static str)],
    /// Protection cells: `LockHeight`, `LockRotate`, `LockTextEdit`, with their values.
    pub locks: &'static [(&'static str, f64)],
    /// `SelectMode`: 1 selects the group first.
    pub select_mode: Option<u8>,
    /// `IsTextEditTarget`.
    pub text_edit_target: Option<bool>,
    /// `ObjType`: 1 for a shape people join, which a route goes round from the start. `None` writes no cell, so a drop reads none and becomes placeable when a connector is
    /// glued to it.
    pub obj_type: Option<u8>,
    pub role: Role,
}

impl Dress {
    /// A dress that says nothing: a base for a table's `..Dress::NONE`.
    pub(crate) const NONE: Dress = Dress {
        fill: None,
        fill_pattern: None,
        line_weight_pt: None,
        line_pattern: None,
        text: TextSpec::None,
        chars: &[],
        data: &[],
        user: &[],
        locks: &[],
        select_mode: None,
        text_edit_target: None,
        obj_type: None,
        role: Role::Shape,
    };

    /// The dress of a shape people join: `ObjType` 1 and nothing else. The masters of Basic Shapes, the Basic Flowchart and the Gane-Sarson shapes wear it (`More::PLACEABLE`).
    pub(crate) const PLACEABLE: Dress = Dress { obj_type: Some(1), ..Dress::NONE };
}

fn text_cell(text: &str) -> Cell {
    Cell::constant(Value::from_text(text))
}

fn string_cell(text: &str) -> Cell {
    text_cell(text).with_unit("STR")
}

fn named_row(name: &str, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    for (cell, value) in cells {
        row.cells.insert(CellName::new(cell), value);
    }
    Arc::new(row)
}

/// The `Property` row of a field: the cells `shapedata.define` makes for a new row, the value typed as types it.
fn property_row(index: usize, field: &FieldDef) -> Arc<Row> {
    let (code, format, value) = match field.kind {
        FieldKind::String => ("0", "", string_cell(field.value)),
        FieldKind::FixedList(items) => ("1", items, string_cell(field.value)),
        FieldKind::Boolean => {
            let on = matches!(field.value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
            ("3", "", text_cell(if on { "1" } else { "0" }).with_unit("BOOL"))
        }
        FieldKind::Number => ("2", "", text_cell(field.value)),
    };
    named_row(
        field.row,
        vec![
            ("Value", value),
            ("Label", text_cell(field.label)),
            ("Prompt", text_cell(field.prompt)),
            ("Type", text_cell(code)),
            ("Format", text_cell(format)),
            ("SortKey", text_cell(&format!("{:04}", index + 1))),
            ("Invisible", text_cell("0")),
            ("Verify", text_cell("0")),
            ("Calendar", text_cell("0")),
        ],
    )
}

/// The `Field` section of a text of fields: a row for each, the value the formula and the shown form its cached result.
fn field_section(fields: &[FieldText]) -> Section {
    let mut section = Section::new("Field", None);
    for (ix, field) in fields.iter().enumerate() {
        let mut row = Row { ix: Some(u32::try_from(ix).unwrap_or(0)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::with_formula(field.formula, Some(Value::from_text(field.shown))).with_unit("STR"));
        row.cells.insert(CellName::new("Format"), text_cell(""));
        row.cells.insert(CellName::new("Type"), text_cell("0"));
        section.rows.push(Arc::new(row));
    }
    section
}

/// The text of a master that holds fields, or of a shape dropped from it: each field in its character format, with a line break after the ones that say so.
pub(crate) fn own_text(fields: &[FieldText]) -> Text {
    let mut parts = Vec::with_capacity(fields.len().saturating_mul(3));
    for (ix, field) in fields.iter().enumerate() {
        parts.push(TextPart::Cp(field.char_row));
        parts.push(TextPart::Fld { ix: u32::try_from(ix).unwrap_or(0), text: Arc::from(field.shown) });
        if field.line_after {
            parts.push(TextPart::Chars(Arc::from("\n")));
        }
    }
    Text { parts, ..Text::default() }
}

/// The `Character` section of the dress: a size is in inches, 1/72 to the point.
fn character_section(rows: &[CharRow]) -> Section {
    let mut section = Section::new("Character", None);
    for (ix, row) in rows.iter().enumerate() {
        let mut cells = vec![("Size", length(row.size_pt / 72.0)), ("Style", plain(f64::from(row.style)))];
        if let Some(color) = row.color {
            cells.push(("Color", Cell::with_formula(color, None)));
        }
        section.rows.push(super::row(u32::try_from(ix).unwrap_or(0), None, cells));
    }
    section
}

/// The `User` section of the dress: the rows it names, and what its role says.
fn user_section(dress: &Dress) -> Option<Section> {
    let mut section = Section::new("User", None);
    if let Role::Container { heading, margin_in } = dress.role {
        // The rows, but not the style row: a container with a master and no style row keeps its master's outline.
        let kind = Cell::with_formula("\"Container\"", Some(Value::from_text("Container")));
        section.rows.push(named_row("msvStructureType", vec![("Value", kind)]));
        section.rows.push(named_row(HEADING_ROW, vec![("Value", plain(f64::from(heading.number())))]));
        section.rows.push(named_row(MARGIN_ROW, vec![("Value", length(margin_in))]));
        section.rows.push(named_row(RESIZE_ROW, vec![("Value", plain(1.0))]));
        section.rows.push(named_row(LOCK_ROW, vec![("Value", plain(0.0))]));
    }
    for (name, value) in dress.user {
        section.rows.push(named_row(name, vec![("Value", string_cell(value))]));
    }
    (!section.rows.is_empty()).then_some(section)
}

/// What the dress adds to the shape the generic builder made: the look, the locks and the group properties as cells, the Shape Data, the fields, the character formats and the `User` rows as sections.
/// → the master's text.
pub(super) fn decorate(dress: &Dress, sheet: &mut Sheet) -> Option<Text> {
    let cells = &mut sheet.cells;
    if let Some(fill) = dress.fill {
        put(cells, "FillForegnd", Cell::with_formula(fill, None));
    }
    if let Some(pattern) = dress.fill_pattern.or(dress.fill.map(|_| 1)) {
        put(cells, "FillPattern", plain(f64::from(pattern)));
    }
    if let Some(points) = dress.line_weight_pt {
        put(cells, "LineWeight", length(points / 72.0));
    }
    if let Some(flag) = &dress.line_pattern {
        put(cells, "LinePattern", flag.cell());
    }
    for (name, value) in dress.locks {
        put(cells, name, plain(*value));
    }
    if let Some(mode) = dress.select_mode {
        put(cells, "SelectMode", plain(f64::from(mode)));
    }
    if let Some(target) = dress.text_edit_target {
        put(cells, "IsTextEditTarget", plain(f64::from(u8::from(target))));
    }
    if let Some(kind) = dress.obj_type {
        put(cells, "ObjType", plain(f64::from(kind)));
    }
    if !dress.data.is_empty() {
        let mut section = Section::new("Property", None);
        section.rows.extend(dress.data.iter().enumerate().map(|(i, f)| property_row(i, f)));
        sheet.sections.push(Arc::new(section));
    }
    let text = match dress.text {
        TextSpec::None => None,
        TextSpec::Plain(text) => Some(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Text::default() }),
        TextSpec::Fields(fields) => {
            sheet.sections.push(Arc::new(field_section(fields)));
            Some(own_text(fields))
        }
    };
    if !dress.chars.is_empty() {
        sheet.sections.push(Arc::new(character_section(dress.chars)));
    }
    if let Some(user) = user_section(dress) {
        sheet.sections.push(Arc::new(user));
    }
    text
}

/// The text a shape dropped from a master keeps of its own: for a master that holds fields, the text that shows them (the shown form of a field lives in the text that holds it), and for one that
/// holds plain placeholder text, that text. **A placeholder is the shape's own**, so that it is the same in every format: an `.odg` holds no master to take a text from, and a shape that left its
/// text to its master read back from one with the text as its own. `None` for a master with no text.
pub(crate) fn dropped_text(dress: &Dress) -> Option<Text> {
    match dress.text {
        TextSpec::Fields(fields) => Some(own_text(fields)),
        TextSpec::Plain(text) => Some(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Text::default() }),
        TextSpec::None => None,
    }
}
