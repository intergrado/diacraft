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
use super::group_master::{Factor, drop_factor};
use super::look::Flag;
use super::{length, length_formula, number_text, plain, put};

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

    /// A master that holds `ObjType` 4 and nothing else ([`Dress::NEITHER`]): a part of a structure, which no route goes round and no layout places (the Swimlane and the Separator).
    pub(crate) const NEITHER: More = More { dress: Some(&Dress::NEITHER), hidden: false, brings: &[] };
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

/// What a cell that a dress sets in a row holds.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CellSet {
    /// A constant number.
    Number(f64),
    /// A formula, which has no value in the table: the master's build settles it (a class's name is italic when its `Abstract` is true).
    Formula(&'static str),
}

/// One cell of one row of a section of the master's shape that the dress sets, beside the rows the other fields make: a paragraph's alignment (`Paragraph`, row 0, `HorzAlign`), or a formula in a
/// character format (`Character`, row 0, `Style`). The section and the row are made when the dress has none of them yet, and a row the dress already made (a `Character` row of `chars`) keeps its
/// other cells. A dress holds at most [`MAX_ROW_CELLS`] of them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowCell {
    pub section: &'static str,
    pub ix: u32,
    pub cell: &'static str,
    pub set: CellSet,
}

/// The most row cells a dress sets: a table is data a person wrote, and the build never reads past it.
pub(crate) const MAX_ROW_CELLS: usize = 16;

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
    /// Cells of rows the dress sets besides: a paragraph's alignment, a formula in a character format ([`RowCell`]).
    pub rows: &'static [RowCell],
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
    /// `ObjType`: 1 for a shape people join, which a route goes round from the start, and 4 for a part of a structure (a swimlane, a separator), which
    /// is neither placeable nor routable. `None` writes no cell, so a drop reads none and becomes placeable when a connector is glued to it.
    pub obj_type: Option<u8>,
    /// A `Rounding` of 0 of the shape's own: a figure whose parts stand against one another (the package's tab on its body) is not rounded part by part by a theme with rounded corners.
    pub no_rounding: bool,
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
        rows: &[],
        data: &[],
        user: &[],
        locks: &[],
        select_mode: None,
        text_edit_target: None,
        obj_type: None,
        no_rounding: false,
        role: Role::Shape,
    };

    /// The dress of a shape people join: `ObjType` 1 and nothing else. The masters of Basic Shapes, the Basic Flowchart and the Gane-Sarson shapes wear it (`More::PLACEABLE`).
    pub(crate) const PLACEABLE: Dress = Dress { obj_type: Some(1), ..Dress::NONE };

    /// The dress of a part of a structure: `ObjType` 4 and nothing else, as the reference's swimlane and separator masters hold (item 2). The Swimlane and the Separator of the
    /// Cross-Functional Flowchart stencil wear it (`More::NEITHER`).
    pub(crate) const NEITHER: Dress = Dress { obj_type: Some(4), ..Dress::NONE };
}

/// The picture of a field that shows a text: the text as it is (`FIELDPICTURE(37)`). An empty picture shows nothing.
pub(crate) const TEXT_FORMAT: &str = "@";

/// A cell that holds a number as text (`0`, `0001`): no unit.
fn text_cell(text: &str) -> Cell {
    Cell::constant(Value::from_text(text))
}

/// A cell that holds a string (the label, the prompt, the format, a user cell's text): `STR`.
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
            ("Label", string_cell(field.label)),
            ("Prompt", string_cell(field.prompt)),
            ("Type", text_cell(code)),
            ("Format", string_cell(format)),
            ("SortKey", string_cell(&format!("{:04}", index + 1))),
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
        row.cells.insert(CellName::new("Format"), string_cell(TEXT_FORMAT));
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

/// The `Character` section of the dress: a size is in inches, 1/72 to the point. A row past the first is whole (`super::text_row`): a reader of the file takes none of its
/// cells from the styles, which hold row 0 alone.
fn character_section(rows: &[CharRow]) -> Section {
    let mut section = Section::new("Character", None);
    for (ix, row) in rows.iter().enumerate() {
        let mut cells = vec![("Size", length(row.size_pt / 72.0)), ("Style", plain(f64::from(row.style)))];
        if let Some(color) = row.color {
            cells.push(("Color", Cell::with_formula(color, None)));
        }
        section.rows.push(super::text_row("Character", u32::try_from(ix).unwrap_or(0), cells));
    }
    section
}

/// The `User` section of the dress: the rows it names, and what its role says. **A container's margin follows the page's drop scale**: the row holds
/// `<margin>*<factor>`, the factor being the shape's own `DropOnPageScale` (`of`) or 1 where it reads none, so that a package dropped beyond the range of eight keeps the room it leaves
/// round what it holds, on paper.
fn user_section(dress: &Dress, of: Factor) -> Option<Section> {
    let mut section = Section::new("User", None);
    if let Role::Container { heading, margin_in } = dress.role {
        // The rows, but not the style row: a container with a master and no style row keeps its master's outline.
        let kind = Cell::with_formula("\"Container\"", Some(Value::from_text("Container"))).with_unit("STR");
        section.rows.push(named_row("msvStructureType", vec![("Value", kind)]));
        section.rows.push(named_row(HEADING_ROW, vec![("Value", plain(f64::from(heading.number())))]));
        let margin = length_formula(&format!("{}*{}", number_text(margin_in), drop_factor(of)), margin_in);
        section.rows.push(named_row(MARGIN_ROW, vec![("Value", margin)]));
        section.rows.push(named_row(RESIZE_ROW, vec![("Value", plain(1.0))]));
        section.rows.push(named_row(LOCK_ROW, vec![("Value", plain(0.0))]));
    }
    for (name, value) in dress.user {
        section.rows.push(named_row(name, vec![("Value", string_cell(value))]));
    }
    (!section.rows.is_empty()).then_some(section)
}

/// What the dress adds to the shape the generic builder made: the look, the locks and the group properties as cells, the Shape Data, the fields, the character formats and the `User` rows as sections.
/// → the master's text. `of` says where a length it holds reads the factor of the page's drop scale: the shape's own or its group's.
pub(super) fn decorate(dress: &Dress, sheet: &mut Sheet, of: Factor) -> Option<Text> {
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
    if dress.no_rounding {
        put(cells, "Rounding", length(0.0));
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
    for set in dress.rows.iter().take(MAX_ROW_CELLS) {
        put_row_cell(sheet, set);
    }
    if let Some(user) = user_section(dress, of) {
        sheet.sections.push(Arc::new(user));
    }
    text
}

/// Sets one cell of one row of a section of the sheet: the section and the row are made when there is none, and a row that is there keeps its other cells. A section that is not `Geometry` has
/// no `IX` of its own, so the one with the name and no `IX` is the one. Rows stay in the order of their `IX`.
fn put_row_cell(sheet: &mut Sheet, set: &RowCell) {
    let cell = match set.set {
        CellSet::Number(n) => plain(n),
        CellSet::Formula(formula) => Cell::with_formula(formula, None),
    };
    let at = match sheet.sections.iter().position(|s| s.name.as_str() == set.section && s.ix.is_none()) {
        Some(at) => at,
        None => {
            sheet.sections.push(Arc::new(Section::new(set.section, None)));
            sheet.sections.len().saturating_sub(1)
        }
    };
    let Some(section) = sheet.sections.get_mut(at).map(Arc::make_mut) else { return };
    if !section.rows.iter().any(|r| r.ix == Some(set.ix)) {
        section.rows.push(Arc::new(Row { ix: Some(set.ix), ..Row::default() }));
        section.rows.sort_by_key(|r| r.ix);
    }
    if let Some(row) = section.rows.iter_mut().find(|r| r.ix == Some(set.ix)) {
        Arc::make_mut(row).cells.insert(CellName::new(set.cell), cell);
    }
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
