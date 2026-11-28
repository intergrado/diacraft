//! The field rows a VSDX file is given.
//!
//! A program that reads a VSDX file takes a field row with an empty `Format` for a field that shows nothing (an empty picture gives empty text), a text with no `U` for a number it cannot read,
//! and a date with no `U` for the same. The rows a drawing may hold that lack what the file needs are of three kinds, none of which is made by this build's `text.insert_field` any more:
//!
//! * **The rows an `.odg` gives** (`diacraft_odg`'s reader): a `Value` cell alone, whose formula is `PAGENUMBER()`, `PAGECOUNT()`, `FILENAME()` or `NOW()` and whose value is the text the field showed
//!   (`5`, `plan.odg`, `11/27/2026 3:28 PM`): no `Type`, no `Format` and no unit.
//! * **The rows of an older build**: a field made with an empty `Format`, a master of the Organization Chart Shapes with two of them, and the cached text of an instance's own `F="Inh"` row with no `U`.
//! * **The row a program of another kind wrote**, with a picture or without, whose text says no unit.
//!
//! **What a row is given** is what a row this build makes has, and nothing a row already says is changed: the `Type` of the field's kind (the cell's own, or the kind of the formula for a row with none:
//! a number for the page number and the page count, a date for `NOW()` and the document dates, a text for the rest), the `Format` of that kind (`default_format`: `@`, `esc(0)` or `{<211>}`, and
//! `h:mm tt` for a time that shows no date) in a cell of its own that says `STR`, and **`STR` on a text** in `Value`. A date that shows as text no `U` says (`11/27/2026 3:28 PM`) is given `U` `DATE` and
//! the date as `YYYY-MM-DDThh:mm:ss`, read from the text when it reads as a date of the forms DiaCraft reads (`draw::shapedata`) and, when it does not (a time, a date in another
//! language), **from the drawing's own times** (its modification time, then its creation time: a saved `NOW()` is the moment of the save), so that the rows are the same whenever the
//! save is made; a drawing that holds neither keeps the text it has. A number says no unit.
//!
//! **Only what is empty or missing.** A `Format` that holds a picture, one that holds a formula, a `Type` that is there and a unit that is there are left as they are: a file's own cells keep what they
//! had. A row that inherits (the shape has a master, or stands for a shape of one) takes its `Type` and its `Format` from the master's row, which the master's own shape is given in its turn, and is
//! given only the unit of a text its cell caches. It is the copy of the drawing a writer is given that is changed: the drawing the person has is not (a save that adopts the model the file reads as,
//! [`super::adopts_by_working_file`], gives the drawing the rows).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, FormulaSlot, Reading, Row, Shape, Value};

use super::hold::rewrite_shapes;
use crate::draw::fields::{Kind, default_format, prop_kind};
use crate::draw::shapedata::date_text;

/// The `FIELDPICTURE` code of the picture of a time that shows no date (`h:mm tt`).
const TIME_PICTURE: u32 = 35;

/// The times a drawing holds, as the dates a field of the date and time reads as when its own text does not say one: `YYYY-MM-DDThh:mm:ss`.
struct Times {
    own: Option<String>,
}

impl Times {
    /// The modification time of the drawing, else its creation time, in the form a date cell holds.
    fn of(doc: &Document) -> Times {
        let canon = |text: &Option<Arc<str>>| {
            let serial = diacraft_calc::iso_to_serial(text.as_deref()?)?;
            diacraft_calc::serial_to_iso(serial)
        };
        Times { own: canon(&doc.props.modified).or_else(|| canon(&doc.props.created)) }
    }
}

/// The kind a formula that gives a field's value says it is, for a row that holds no `Type` of its own: the page number and the page count (and an object's `ID`) are numbers, the dates a document
/// holds and `NOW()` are dates, and any other is a text.
fn kind_of_formula(source: &str) -> Kind {
    match source.trim().to_ascii_uppercase().as_str() {
        "PAGENUMBER()" | "PAGECOUNT()" | "ID()" => Kind::Number,
        "NOW()" | "DOCCREATION()" | "DOCLASTEDIT()" | "DOCLASTSAVE()" | "DOCLASTPRINT()" => Kind::Date,
        _ => Kind::Text,
    }
}

/// Whether the text a date field shows is a time and no date (`3:28 PM`, `15:28:03`): it has a clock and no mark of a day.
fn time_only(shown: &str) -> bool {
    let shown = shown.trim();
    shown.contains(':') && !shown.contains(['/', '-', '.', ',']) && date_text(shown).is_none()
}

/// The date a field's text says, as `YYYY-MM-DDThh:mm:ss`: the text read as a date, or the drawing's own time.
fn date_of(shown: &str, times: &Times) -> Option<String> {
    let read = date_text(shown).and_then(|iso| diacraft_calc::iso_to_serial(&iso)).and_then(diacraft_calc::serial_to_iso);
    read.or_else(|| times.own.clone())
}

/// Whether the cell is a constant that holds no text: no formula and no value or a value of white space (a picture of one's own that is empty).
fn is_blank_constant(cell: &Cell) -> bool {
    matches!(cell.formula, FormulaSlot::Absent) && cell.value.as_ref().is_none_or(|v| v.text().trim().is_empty())
}

/// The number the cell `name` of the row holds.
fn number_of(row: &Row, name: &str) -> Option<f64> {
    row.cells.get(name).and_then(|c| c.value.as_ref()).and_then(Value::number)
}

/// The row given what it lacks. → the row, when it changed. `inherits`: the shape takes rows from a master's shape.
fn fitted_row(row: &Row, inherits: bool, times: &Times) -> Option<Row> {
    if row.del {
        return None;
    }
    let mut out = row.clone();
    let mut changed = false;
    let value = row.cells.get("Value");
    let formula = value.filter(|c| c.owns_formula()).and_then(|c| c.formula.text());
    // The kind: the row's own `Type`, else the formula's.
    let type_given = number_of(row, "Type");
    let kind = match type_given {
        Some(_) => prop_kind(type_given),
        None => formula.map_or(Kind::Text, kind_of_formula),
    };
    let shown = value.and_then(|c| c.value.as_ref()).map_or("", Value::text);
    // The `Type`, where nothing else gives it.
    if type_given.is_none() && !inherits && !row.cells.contains_key("Type") {
        let type_cell = Cell::constant(Value::from_text(&kind.type_cell().to_string()));
        changed |= out.put_cell(&CellName::new("Type"), type_cell).is_ok();
    }
    // The picture of the field's kind where there is none: an empty one of the row's own, or none where no master gives one.
    let picture = || {
        if kind == Kind::Date && time_only(shown) { diacraft_calc::field_picture(TIME_PICTURE) } else { default_format(kind).to_owned() }
    };
    match row.cells.get("Format") {
        Some(cell) if is_blank_constant(cell) => {
            let made = Cell::constant(Value::from_text(&picture())).with_unit("STR");
            changed |= out.put_cell(&CellName::new("Format"), made).is_ok();
        }
        None if !inherits => {
            let made = Cell::constant(Value::from_text(&picture())).with_unit("STR");
            changed |= out.put_cell(&CellName::new("Format"), made).is_ok();
        }
        _ => {}
    }
    // The value: a date says it is one, a text says it is a string.
    if let Some(cell) = out.cells.get_mut("Value") {
        let has_unit = cell.unit.as_deref().is_some_and(|u| !u.trim().is_empty());
        if kind == Kind::Date && !has_unit && cell.value.as_ref().is_some_and(|v| v.reading() == Reading::Text) {
            if let Some(iso) = date_of(shown, times) {
                cell.value = Some(Value::from_text(&iso));
                cell.unit = Some(Arc::from("DATE"));
                changed = true;
            }
        } else if kind == Kind::Text && type_given.is_none() && !inherits && !has_unit && cell.value.is_some() {
            // A row of the `.odg` reader that shows a text (a file name): a string whatever it reads as, an empty one too.
            cell.unit = Some(Arc::from("STR"));
            changed = true;
        } else {
            changed |= cell.fit_string_unit(Some("Field"), "Value");
        }
    }
    changed.then_some(out)
}

/// The shape with the rows of its `Field` section given what they lack. → the copy, when a row changed.
fn fitted(shape: &Shape, times: &Times) -> Option<Shape> {
    let inherits = shape.master.is_some() || shape.master_shape.is_some();
    let at = shape.sheet.sections.iter().position(|s| s.name.as_str() == "Field")?;
    let section = shape.sheet.sections.get(at)?;
    let made: Vec<(usize, Row)> = section.rows.iter().enumerate().filter_map(|(i, row)| fitted_row(row, inherits, times).map(|r| (i, r))).collect();
    if made.is_empty() {
        return None;
    }
    let mut own = Shape::clone(shape);
    let sheet = Arc::make_mut(&mut own.sheet);
    let section = Arc::make_mut(sheet.sections.get_mut(at)?);
    for (i, row) in made {
        *section.rows.get_mut(i)? = Arc::new(row);
    }
    Some(own)
}

/// The drawing with every field row of a page or a master given what a VSDX file needs of it, and how many rows were; `None` when none is (and then the count is 0). The copy shares everything it does
/// not change.
pub(crate) fn fit(doc: &Document, steps: &mut u64) -> Result<(u64, Option<Document>), &'static str> {
    let times = Times::of(doc);
    let mut count = 0u64;
    let mut change = |shape: &Shape| {
        let own = fitted(shape, &times);
        // (The count is of the shapes whose rows changed: a shape of a few rows is one.)
        count = count.saturating_add(u64::from(own.is_some()));
        own
    };
    let with = rewrite_shapes(doc, true, &mut change, steps)?;
    Ok((count, with))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[(&str, Cell)]) -> Row {
        let mut row = Row { ix: Some(0), ..Row::default() };
        for (name, cell) in cells {
            row.cells.insert(CellName::new(name), cell.clone());
        }
        row
    }

    fn times() -> Times {
        Times { own: Some("2026-03-04T10:30:00".to_owned()) }
    }

    fn cell_of(row: &Row, name: &str) -> (Option<String>, Option<String>) {
        row.cells.get(name).map_or((None, None), |c| (c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref().map(str::to_owned)))
    }

    #[test]
    fn the_kind_of_a_formula_is_a_number_a_date_or_a_text() {
        assert_eq!(kind_of_formula("PAGENUMBER()"), Kind::Number);
        assert_eq!(kind_of_formula(" pagecount() "), Kind::Number);
        assert_eq!(kind_of_formula("NOW()"), Kind::Date);
        assert_eq!(kind_of_formula("DOCLASTEDIT()"), Kind::Date);
        assert_eq!(kind_of_formula("FILENAME()"), Kind::Text);
        assert_eq!(kind_of_formula("Prop.Name"), Kind::Text);
    }

    #[test]
    fn a_time_is_a_clock_with_no_day() {
        assert!(time_only("3:28 PM") && time_only("15:28:03"));
        assert!(!time_only("11/27/2026 3:28 PM") && !time_only("27.11.2026 15:28") && !time_only("Page 3") && !time_only("2026-11-27 15:28"));
    }

    #[test]
    fn a_date_is_read_from_its_text_and_else_from_the_drawings_times() {
        assert_eq!(date_of("11/27/2026 3:28 PM", &times()).as_deref(), Some("2026-11-27T15:28:00"));
        assert_eq!(date_of("3:28 PM", &times()).as_deref(), Some("2026-03-04T10:30:00"));
        assert_eq!(date_of("13/45/2026", &times()).as_deref(), Some("2026-03-04T10:30:00"), "a day the calendar does not have");
        assert_eq!(date_of("3:28 PM", &Times { own: None }), None);
    }

    #[test]
    fn a_row_with_a_value_alone_is_given_its_kind_its_picture_and_its_unit() {
        let page = row(&[("Value", Cell::with_formula("PAGENUMBER()", Some(Value::from_text("5"))))]);
        let got = fitted_row(&page, false, &times()).expect("given a type and a picture");
        assert_eq!((cell_of(&got, "Type"), cell_of(&got, "Format")), ((Some("2".into()), None), (Some("esc(0)".into()), Some("STR".into()))));
        assert_eq!(cell_of(&got, "Value"), (Some("5".into()), None), "a number says no unit");
        let name = row(&[("Value", Cell::with_formula("FILENAME()", Some(Value::from_text(""))))]);
        let got = fitted_row(&name, false, &times()).expect("given a text's picture");
        assert_eq!(
            (cell_of(&got, "Type"), cell_of(&got, "Format"), cell_of(&got, "Value")),
            ((Some("0".into()), None), (Some("@".into()), Some("STR".into())), (Some(String::new()), Some("STR".into())))
        );
        let date = row(&[("Value", Cell::with_formula("NOW()", Some(Value::from_text("11/27/2026 3:28 PM"))))]);
        let got = fitted_row(&date, false, &times()).expect("given a date");
        assert_eq!(cell_of(&got, "Value"), (Some("2026-11-27T15:28:00".into()), Some("DATE".into())));
        assert_eq!(cell_of(&got, "Format").0.as_deref(), Some("{<211>}"));
        let time = row(&[("Value", Cell::with_formula("NOW()", Some(Value::from_text("3:28 PM"))))]);
        assert_eq!(cell_of(&fitted_row(&time, false, &times()).expect("given a time"), "Format").0.as_deref(), Some("h:mm tt"));
    }

    #[test]
    fn what_a_row_says_is_kept_and_a_row_that_inherits_takes_its_type_and_picture_from_the_master() {
        // A picture, a type and a unit that are there stay.
        let whole = row(&[
            ("Value", Cell::with_formula("Prop.Name", Some(Value::from_text("Ann"))).with_unit("STR")),
            ("Format", Cell::constant(Value::from_text("@+"))),
            ("Type", Cell::constant(Value::from_text("0"))),
        ]);
        assert_eq!(fitted_row(&whole, false, &times()), None);
        // A format that holds a formula is not an empty constant.
        let formula = row(&[
            ("Value", Cell::constant(Value::from_text("x")).with_unit("STR")),
            ("Format", Cell::with_formula("FIELDPICTURE(37)", None)),
            ("Type", Cell::constant(Value::from_text("0"))),
        ]);
        assert_eq!(fitted_row(&formula, false, &times()), None);
        // An instance's row of a cached text: only the unit.
        let inherited = row(&[("Value", Cell::inherited(Some(Value::from_text("Ann Lee"))))]);
        let got = fitted_row(&inherited, true, &times()).expect("the text says STR");
        assert_eq!(cell_of(&got, "Value"), (Some("Ann Lee".into()), Some("STR".into())));
        assert!(!got.cells.contains_key("Type") && !got.cells.contains_key("Format"), "the master's row gives them");
        // A master's own empty picture is given the picture of its kind, with the type it has.
        let master = row(&[
            ("Value", Cell::with_formula("Prop.Name", Some(Value::from_text("Name")))),
            ("Format", Cell::constant(Value::from_text(""))),
            ("Type", Cell::constant(Value::from_text("0"))),
        ]);
        let got = fitted_row(&master, false, &times()).expect("an empty picture");
        assert_eq!((cell_of(&got, "Format"), cell_of(&got, "Value").1), ((Some("@".into()), Some("STR".into())), Some("STR".into())));
        // A deleted row is left.
        assert_eq!(fitted_row(&Row { del: true, ..master.clone() }, false, &times()), None);
    }
}
