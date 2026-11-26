//! The Organization Chart Shapes stencil: six position masters (ours).
//!
//! The specifications give the stencil in a sentence: it "opens a stencil of position shapes, such as executive, manager, position, assistant, consultant and vacancy", and
//! "each shape has name and title text" (Organization Chart template), with the default Department, Telephone, Name, Title and E-mail as the
//! fields of a block (Org chart wizard field choices). The drawings are ours: plain rectangles, told apart by size, tint and line.
//!
//! * **The text** is two lines, the name over the title, each a **field** that shows a Shape Data field of the shape (`Prop.Name`, `Prop.Title`): a person types them in the Shape
//!   Data window or by `orgchart.set`, and the text follows. The master holds the text and the field rows; a shape dropped from it has a text of its own to
//!   keep the shown form in (`own_text`), as a field's shown form lives in the text that holds it.
//! * **The Shape Data** is the five default fields: Name, Title, Department, Telephone and E-mail (the rows `Name`, `Title`, `Department`, `Telephone` and `E_mail`, whose label is
//!   `E-mail`), in that order, each a string; Name and Title start as placeholders (`Name`, and the position's kind as the title) so that a new shape shows what it is.
//! * **The marker** is the user cell `User.DiaCraftOrgPosition` (ours: no cell is specified for it), whose value is the kind of position (`Executive`, `Manager`, `Position`,
//!   `Assistant`, `Consultant`, `Vacancy`). It is what makes a shape a position of an org chart (`draw::orgchart`), on the master and on every shape dropped from it (a copy of the row
//!   on the shape, so that a format that keeps no masters still knows the shape).
//! * **The connection points** are the four middles of the sides, as the Rectangle master has them, so the chart's connectors glue at the bottom and the top, or at the side.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Sheet, Text, TextPart, Value};

use super::{MasterSpec, More, Outline, SIDES, length, plain, put};
use crate::formatting::Rgb;

/// The series number of the stencil's masters in their GUIDs (`MasterSpec::guid`): Basic Shapes 1, Basic Flowchart 2, 3 the GUID a copied master is given when its own is taken
/// (`bring.rs`), the other built-in libraries 4 to 6, and the Cross-Functional Flowchart Shapes 7 (`xfunc.rs`). A position is told by its series
/// ([`is_position`]), so no other library may share it.
pub(super) const SERIES: u8 = 8;

/// The id of the stencil.
pub const ID: &str = "orgchart";

/// The name of the stencil, as the Shapes window shows it.
pub const NAME: &str = "Organization Chart Shapes";

/// The user row that makes a shape a position of an org chart, and the value it holds: the kind of position.
pub const MARKER_ROW: &str = "DiaCraftOrgPosition";

/// The Shape Data fields of a position, in order: (row name, label, prompt).
pub const FIELDS: [(&str, &str, &str); 5] = [
    ("Name", "Name", "The person's name"),
    ("Title", "Title", "The position's title"),
    ("Department", "Department", "The department"),
    ("Telephone", "Telephone", "The telephone number"),
    ("E_mail", "E-mail", "The e-mail address"),
];

/// How a master looks: its line weight and pattern, and its fill, apart from the size.
struct Look {
    kind: &'static str,
    fill: Rgb,
    /// The line weight, in points.
    weight: f64,
    /// `LinePattern`: 1 solid.
    pattern: f64,
    name: &'static str,
}

const fn master(at: u16, id: &'static str, name: &'static str, width: f64, height: f64) -> MasterSpec {
    MasterSpec {
        identity: (SERIES, at),
        more: &More::NONE,
        id,
        name,
        width,
        height,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &SIDES,
    }
}

pub(super) static MASTERS: [MasterSpec; 6] = [
    master(1, "orgchart.executive", "Executive", 2.2, 1.0),
    master(2, "orgchart.manager", "Manager", 2.0, 0.9),
    master(3, "orgchart.position", "Position", 1.8, 0.8),
    master(4, "orgchart.assistant", "Assistant", 1.6, 0.7),
    master(5, "orgchart.consultant", "Consultant", 1.8, 0.8),
    master(6, "orgchart.vacancy", "Vacancy", 1.8, 0.8),
];

/// The look of the master `at` (its number in the series). **Every line is solid**: a dashed outline can leave a gap at the middle of a side, where a connector meets the shape, and the
/// kinds are told apart by size, tint and the weight of the line.
fn look_of(at: u16) -> Look {
    match at {
        1 => Look { kind: "Executive", fill: Rgb(214, 228, 244), weight: 2.25, pattern: 1.0, name: "Name" },
        2 => Look { kind: "Manager", fill: Rgb(232, 238, 247), weight: 1.25, pattern: 1.0, name: "Name" },
        3 => Look { kind: "Position", fill: Rgb(255, 255, 255), weight: 1.0, pattern: 1.0, name: "Name" },
        4 => Look { kind: "Assistant", fill: Rgb(236, 245, 236), weight: 1.0, pattern: 1.0, name: "Name" },
        5 => Look { kind: "Consultant", fill: Rgb(252, 244, 226), weight: 1.0, pattern: 1.0, name: "Name" },
        _ => Look { kind: "Vacancy", fill: Rgb(243, 243, 243), weight: 0.75, pattern: 1.0, name: "Vacant" },
    }
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

/// The user section holding the marker `kind`.
pub(crate) fn marker_section(kind: &str) -> Section {
    let mut user = Section::new("User", None);
    user.rows.push(named_row(MARKER_ROW, vec![("Value", string_cell(kind))]));
    user
}

/// The Shape Data section of a position: the five fields, with `name` and `title` as the values of the first two.
fn property_section(name: &str, title: &str) -> Section {
    let mut section = Section::new("Property", None);
    for (i, (row, label, prompt)) in FIELDS.iter().enumerate() {
        let value = match *row {
            "Name" => name,
            "Title" => title,
            _ => "",
        };
        section.rows.push(named_row(
            row,
            vec![
                ("Value", string_cell(value)),
                ("Label", text_cell(label)),
                ("Prompt", text_cell(prompt)),
                ("Type", text_cell("0")),
                ("Format", text_cell("")),
                ("SortKey", text_cell(&format!("{:04}", i + 1))),
                ("Invisible", text_cell("0")),
                ("Verify", text_cell("0")),
                ("Calendar", text_cell("0")),
            ],
        ));
    }
    section
}

/// The `Field` section: two fields, the name and the title, each the value of its Shape Data field.
fn field_section(name: &str, title: &str) -> Section {
    let mut section = Section::new("Field", None);
    for (ix, (formula, shown)) in [("Prop.Name", name), ("Prop.Title", title)].into_iter().enumerate() {
        let mut row = Row { ix: Some(u32::try_from(ix).unwrap_or(0)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::with_formula(formula, Some(Value::from_text(shown))).with_unit("STR"));
        row.cells.insert(CellName::new("Format"), text_cell(""));
        row.cells.insert(CellName::new("Type"), text_cell("0"));
        section.rows.push(Arc::new(row));
    }
    section
}

/// The `Character` section: the name bold at 10 points, the title at 8 (a size is in inches, 1/72 to the point).
fn character_section() -> Section {
    let mut section = Section::new("Character", None);
    section.rows.push(super::row(0, None, vec![("Size", length(10.0 / 72.0)), ("Style", plain(1.0))]));
    section.rows.push(super::row(1, None, vec![("Size", length(8.0 / 72.0)), ("Style", plain(0.0))]));
    section
}

/// The text of a position: the name field, a line break and the title field, each run in its own character format.
pub(crate) fn own_text(name: &str, title: &str) -> Text {
    Text {
        parts: vec![
            TextPart::Cp(0),
            TextPart::Fld { ix: 0, text: Arc::from(name) },
            TextPart::Chars(Arc::from("\n")),
            TextPart::Cp(1),
            TextPart::Fld { ix: 1, text: Arc::from(title) },
        ],
        ..Text::default()
    }
}

/// What a master of this stencil adds to the shape the generic builder made: the look (fill, line), the Shape Data, the fields, the character formats and the marker; and its text.
pub(super) fn decorate(spec: &MasterSpec, sheet: &mut Sheet) -> Text {
    let look = look_of(spec.identity.1);
    let (name, title) = (look.name, look.kind);
    put(&mut sheet.cells, "FillPattern", plain(1.0));
    put(&mut sheet.cells, "FillForegnd", Cell::with_formula(&look.fill.formula(), Some(Value::from_text(&look.fill.hex()))));
    put(&mut sheet.cells, "LinePattern", plain(look.pattern));
    put(&mut sheet.cells, "LineWeight", length(look.weight / 72.0));
    // Owner 2026-11-25: a position is a shape people join, so a route goes round one dropped on it; the same cell as a `Dress` with `obj_type` writes.
    put(&mut sheet.cells, "ObjType", plain(1.0));
    sheet.sections.push(Arc::new(property_section(name, title)));
    sheet.sections.push(Arc::new(field_section(name, title)));
    sheet.sections.push(Arc::new(character_section()));
    sheet.sections.push(Arc::new(marker_section(look.kind)));
    own_text(name, title)
}

/// The text a shape dropped from `spec` holds, and the marker row it keeps: `None` for a master of another stencil.
pub(crate) fn dressing(spec: &MasterSpec) -> Option<(Text, Section)> {
    if spec.identity.0 != SERIES {
        return None;
    }
    let look = look_of(spec.identity.1);
    Some((own_text(look.name, look.kind), marker_section(look.kind)))
}

/// Whether `spec` is a master of this stencil.
pub fn is_position(spec: &MasterSpec) -> bool {
    spec.identity.0 == SERIES
}
