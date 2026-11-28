//! The unit of a text a cell holds: `STR`.
//!
//! **A text in `V` carries `U="STR"`.** A cell whose value is a text but says no unit is read, by a program that takes the `U` to say what `V` is, as a number it cannot read: the
//! Shape Data of a position showed `0 0` for a name and a title, and a field with no unit shows nothing. The unit belongs to the cell that holds the value, so **a cell an instance
//! holds of its own (`F="Inh"` and a cached result) says it too**: the unit of the master's cell is no unit of the instance's.
//!
//! **Which cells.** The catalogue says what a cell is ([`cell_info`]):
//!
//! * a cell of a text kind ([`ValueKind::Text`]: the label, prompt, format and sort key of a Shape Data row, a field's format, a hyperlink's address, a layer's name, ...) holds a text
//!   whatever it reads as (`0001` is a sort key, not a number), and an empty one is the empty text: all carry `STR`;
//! * a cell of any value (`vAny`: the `Value` of a Shape Data, user-defined or field row, and the cells of `Scratch`) holds a text when its `V` reads as one, and then carries `STR`;
//!   a number, an empty value and a colour (`#RRGGBB`: a colour needs no unit) are left as they are.
//!
//! A cell that says a unit already is never changed, and a cell with no value is not a text.
//!
//! **Whose cells they are**. A writer that writes from the model gives the unit to such a cell only in a drawing whose units are DiaCraft's to state
//! ([`units_are_ours_to_state`]): one that came from no VSDX package, or from one whose extended properties name DiaCraft. The text of the `User` and `Field` rows of a file of another program is that
//! program's, and every route writes it as it was read: a file of the corpus that a program wrote with a text and no unit in a `User` row came out of the trip through `.diacraft` with `STR`, a
//! difference of the content in a file that was never DiaCraft's.
//!
//! **The cells a unit alone tells the application how to read**: a pass for the cells that **pass through** a writer as they are held, which gives the unit to the `Value` of a
//! `User` or a `Field` row that holds a text and nothing else ([`Cell::lacks_untyped_string_unit`], [`Sheet::fit_untyped_string_units`]). The application warns of such a cell once for each ("an
//! element in this file contains data that is invalid or inappropriate for the expected type of data"), since no `Type` cell in its row says it is a string; a `Property` row's `Value` is typed by
//! its row's `Type` (104,655 string rows of the sample files say no unit), and **the cells of a text kind carry none in the files of the program that
//! opens them** (`Prop.Format`, `User.Prompt`, `Hyperlink.SubAddress`), so a file that came from another program is not changed in them. A text the files hold
//! in the `Value` of a `User` or `Field` row says `STR` ("A top-level string result holds `U` `STR` in `Field.Value`, `Prop.Value` and `User.Value`"). **A `Field` row whose `Type` cell says
//! the value is no string** (a date, a number, a duration, a currency, a list item: `Type` other than 0) **is typed by it**, as a `Property` row is, and its cached value is its type's to say and not
//! a string's (the field rows of the files hold `U` `DATE` and a date as text): it is left as it is.

use std::sync::Arc;

use crate::alt_text::STRING_UNIT;
use crate::catalogue::cell_info;
use crate::cell::Cell;
use crate::document::Document;
use crate::root_text::came_from_a_vsdx_package;
use crate::shape::{MAX_SHAPE_DEPTH, Shape};
use crate::sheet::{Row, Sheet};
use crate::value::{Reading, ValueKind};

/// The sections whose `vAny` cells hold what a person typed: a text in them is a string.
const ANY_VALUE_SECTIONS: [&str; 4] = ["Property", "User", "Field", "Scratch"];

/// The cell that is `vAny` and is not a value: `EditMode` of a field row (kept as the file has it).
const NOT_A_VALUE: &str = "EditMode";

/// The sections whose `Value` cell has no `Type` beside it in its row to say it holds a text: the unit alone says so.
const UNTYPED_VALUE_SECTIONS: [&str; 2] = ["User", "Field"];

/// The cell of those rows that holds the value.
const VALUE_CELL: &str = "Value";

/// What says whether a cell lacks the unit, given its section (`None` under a sheet), its name and the row it is in (`None` when it is in none): [`Cell::lacks_string_unit`] for every cell of a
/// text, [`Cell::lacks_untyped_string_unit`] for the cells a unit alone tells the application how to read.
type Lacks = fn(&Cell, Option<&str>, &str, Option<&Row>) -> bool;

fn lacks_any(cell: &Cell, section: Option<&str>, name: &str, _row: Option<&Row>) -> bool {
    cell.lacks_string_unit(section, name)
}

fn lacks_untyped(cell: &Cell, section: Option<&str>, name: &str, row: Option<&Row>) -> bool {
    cell.lacks_untyped_string_unit(section, name, row)
}

/// Whether the row's own `Type` cell says its value is no string: a number other than 0 (`Type` 0 is a string; a row with none, or one whose `Type` is no number, says nothing).
fn row_is_typed_not_a_string(row: &Row) -> bool {
    row.cells.get("Type").and_then(|cell| cell.value.as_ref()).and_then(|value| value.number()).is_some_and(|n| n != 0.0)
}

/// Whether the units of the text a VSDX file holds are **DiaCraft's to state** when `doc` is written from the model: the drawing came from no VSDX package (a new one, one read
/// from an `.odg`, one read from a `.diacraft` file that holds no VSDX package: [`came_from_a_vsdx_package`]), or came from one whose extended properties name DiaCraft ([`crate::VsdxHeld::names_diacraft`]:
/// a file an earlier build wrote, or a template of DiaCraft's own). **A drawing that came from the package of another program holds that program's cells**, which a writer says as they were read on
/// every route, as a save into the file does: the text of its `User` and `Field` rows that says no unit is no cell to be given one. A package that names no program is another program's.
#[must_use]
pub fn units_are_ours_to_state(doc: &Document) -> bool {
    !came_from_a_vsdx_package(doc) || doc.vsdx.as_deref().is_some_and(|held| held.names_diacraft())
}

/// Whether `text` is a colour in the form a cell holds one: `#` and six hexadecimal digits.
fn is_colour(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Cell {
    /// Whether this cell, `name` directly under a sheet when `section` is `None` and inside a row of the section `section` otherwise, holds a text and says no unit, where a text
    /// carries `STR` (see the [module](self)).
    #[must_use]
    pub fn lacks_string_unit(&self, section: Option<&str>, name: &str) -> bool {
        if self.unit.as_deref().is_some_and(|u| !u.trim().is_empty()) {
            return false;
        }
        let Some(value) = &self.value else { return false };
        match cell_info(section, name).kind {
            ValueKind::Text => !matches!(value.reading(), Reading::Themed | Reading::Infinite { .. }),
            ValueKind::Days => {
                section.is_some_and(|s| ANY_VALUE_SECTIONS.contains(&s))
                    && name != NOT_A_VALUE
                    && value.reading() == Reading::Text
                    && !is_colour(value.text())
            }
            _ => false,
        }
    }

    /// Whether this cell is **the `Value` of a `User` or a `Field` row** that [lacks](Cell::lacks_string_unit) `STR`: the cells whose unit alone tells the application that the value is a text
    /// (see the [module](self)). A text of a text kind and the `Value` of a `Property` row are not these, and neither is the `Value` of a row whose own `Type` says it is no string (`row`, the row the
    /// cell is in, when the caller has it: a date field's cached value is a date's).
    #[must_use]
    pub fn lacks_untyped_string_unit(&self, section: Option<&str>, name: &str, row: Option<&Row>) -> bool {
        name == VALUE_CELL
            && section.is_some_and(|s| UNTYPED_VALUE_SECTIONS.contains(&s))
            && !row.is_some_and(row_is_typed_not_a_string)
            && self.lacks_string_unit(section, name)
    }

    /// Gives the cell `STR` when it [lacks](Cell::lacks_string_unit) it. → whether it did.
    pub fn fit_string_unit(&mut self, section: Option<&str>, name: &str) -> bool {
        let lacks = self.lacks_string_unit(section, name);
        if lacks {
            self.unit = Some(Arc::from(STRING_UNIT));
        }
        lacks
    }
}

impl Sheet {
    /// Whether any cell of the sheet [lacks](Cell::lacks_string_unit) `STR`: a read-only pass, so that a caller that asks it before [`Sheet::fit_string_units`] copies only the sheets that need it.
    #[must_use]
    pub fn lacks_string_units(&self) -> bool {
        sheet_lacks_with(self, lacks_any)
    }

    /// Whether any cell of the sheet is the `Value` of a `User` or `Field` row that [lacks](Cell::lacks_untyped_string_unit) `STR` (the read-only pass of [`Sheet::fit_untyped_string_units`]).
    #[must_use]
    pub fn lacks_untyped_string_units(&self) -> bool {
        sheet_lacks_with(self, lacks_untyped)
    }

    /// Gives every cell of the sheet that [lacks](Cell::lacks_string_unit) `STR`: the cells under the sheet and under its sections, and those of the rows. → whether any cell was given it.
    /// A section and a row that need nothing stay the very allocations they were (a model shares what an edit did not change).
    pub fn fit_string_units(&mut self) -> bool {
        self.fit_with(lacks_any)
    }

    /// Gives `STR` to the cells of the sheet that are the `Value` of a `User` or `Field` row and [lack](Cell::lacks_untyped_string_unit) it, and to no other. → whether any cell was given it.
    /// What needs nothing stays the very allocation it was.
    pub fn fit_untyped_string_units(&mut self) -> bool {
        self.fit_with(lacks_untyped)
    }

    fn fit_with(&mut self, lacks: Lacks) -> bool {
        let mut any = false;
        for (name, cell) in &mut self.cells {
            if lacks(cell, None, name.as_str(), None) {
                any |= cell.fit_string_unit(None, name.as_str());
            }
        }
        for section in &mut self.sections {
            let key = section.name.as_str();
            let needs = section.cells.iter().any(|(name, cell)| lacks(cell, Some(key), name.as_str(), None))
                || section.rows.iter().any(|row| row.cells.iter().any(|(name, cell)| lacks(cell, Some(key), name.as_str(), Some(row))));
            if !needs {
                continue;
            }
            let section = Arc::make_mut(section);
            let key = section.name.as_str().to_owned();
            for (name, cell) in &mut section.cells {
                if lacks(cell, Some(&key), name.as_str(), None) {
                    any |= cell.fit_string_unit(Some(&key), name.as_str());
                }
            }
            for row in section.rows.iter_mut() {
                // The cells that lack it are found before the row is copied, since the test reads the row (its `Type`) and the copy is what changes.
                let lacking: Vec<_> =
                    row.cells.iter().filter(|(name, cell)| lacks(cell, Some(&key), name.as_str(), Some(row))).map(|(name, _)| name.clone()).collect();
                if lacking.is_empty() {
                    continue;
                }
                let own = Arc::make_mut(row);
                for name in lacking {
                    if let Some(cell) = own.cells.get_mut(&name) {
                        any |= cell.fit_string_unit(Some(&key), name.as_str());
                    }
                }
            }
        }
        any
    }
}

impl Shape {
    /// [`Sheet::fit_string_units`] for the shape and every member of it, to [`MAX_SHAPE_DEPTH`] levels. → whether any cell was given the unit. A shape and a member that need nothing stay
    /// the very allocations they were.
    pub fn fit_string_units(&mut self) -> bool {
        self.fit_string_units_at(0)
    }

    fn fit_string_units_at(&mut self, depth: usize) -> bool {
        let mut any = false;
        if sheet_lacks_with(&self.sheet, lacks_any) {
            any |= Arc::make_mut(&mut self.sheet).fit_string_units();
        }
        if depth >= MAX_SHAPE_DEPTH {
            return any;
        }
        for at in 0..self.children.len() {
            if self.children.get(at).is_some_and(|member| shape_lacks(member, depth + 1))
                && let Some(member) = self.children.get_mut(at)
            {
                any |= Arc::make_mut(member).fit_string_units_at(depth + 1);
            }
        }
        any
    }
}

/// Whether the shape or a member of it holds a cell that lacks the unit (a read-only pass, so that a shape that needs nothing is not copied).
fn shape_lacks(shape: &Shape, depth: usize) -> bool {
    sheet_lacks_with(&shape.sheet, lacks_any) || (depth < MAX_SHAPE_DEPTH && shape.children.iter().any(|member| shape_lacks(member, depth + 1)))
}

/// Whether the sheet holds a cell that `lacks` says lacks the unit, under it, under a section or in a row.
fn sheet_lacks_with(sheet: &Sheet, lacks: Lacks) -> bool {
    sheet.cells.iter().any(|(name, cell)| lacks(cell, None, name.as_str(), None))
        || sheet.sections.iter().any(|section| {
            let key = section.name.as_str();
            section.cells.iter().any(|(name, cell)| lacks(cell, Some(key), name.as_str(), None))
                || section.rows.iter().any(|row| row.cells.iter().any(|(name, cell)| lacks(cell, Some(key), name.as_str(), Some(row))))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::FormulaSlot;
    use crate::name::CellName;
    use crate::sheet::{Row, Section};
    use crate::value::Value;

    fn text(t: &str) -> Cell {
        Cell::constant(Value::from_text(t))
    }

    #[test]
    fn a_text_cell_of_a_text_kind_holds_str_whatever_it_reads_as() {
        for t in ["Name", "", "0001", "1.5"] {
            assert!(text(t).lacks_string_unit(Some("Property"), "Label"), "{t:?}");
        }
        assert!(text("{<211>}").lacks_string_unit(Some("Field"), "Format"));
        assert!(!text("Name").with_unit("STR").lacks_string_unit(Some("Property"), "Label"), "it says it already");
        assert!(!text("Name").with_unit("IN").lacks_string_unit(Some("Property"), "Label"), "a unit that is there is not replaced");
        assert!(!Cell::new().lacks_string_unit(Some("Property"), "Label"), "no value, no text");
    }

    #[test]
    fn a_value_of_any_kind_holds_str_when_it_reads_as_a_text() {
        assert!(text("Ann Lee").lacks_string_unit(Some("Property"), "Value"));
        assert!(text("Container").lacks_string_unit(Some("User"), "Value"));
        assert!(
            Cell { formula: FormulaSlot::Inherit, ..text("Chief") }.lacks_string_unit(Some("Field"), "Value"),
            "an inherited formula with a cached text"
        );
        assert!(text("x").lacks_string_unit(Some("Scratch"), "A"));
        for left in ["12", "", "1.#INF", "Themed", "#FF8000"] {
            assert!(!text(left).lacks_string_unit(Some("Property"), "Value"), "{left:?} is no text");
        }
        assert!(!text("x").lacks_string_unit(Some("Field"), "EditMode"));
    }

    #[test]
    fn a_cell_of_another_kind_is_left_alone() {
        assert!(!text("abc").lacks_string_unit(None, "PinX"));
        assert!(!text("#FF0000").lacks_string_unit(None, "FillForegnd"));
        assert!(!text("abc").lacks_string_unit(Some("Property"), "Type"));
        assert!(!text("abc").lacks_string_unit(Some("Hyperlink"), "NoSuchCell"));
        assert!(!text("abc").lacks_string_unit(None, "TextColor"), "a cell under the sheet that is vAny is not a value");
    }

    #[test]
    fn a_sheet_is_fitted_where_it_needs_it_and_shares_the_rest() {
        let mut row = Row { name: Some(Arc::from("Name")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), text("Ann"));
        row.cells.insert(CellName::new("Label"), text("Name"));
        row.cells.insert(CellName::new("Type"), text("0"));
        let mut section = Section::new("Property", None);
        section.rows.push(Arc::new(row));
        let mut quiet = Section::new("Actions", None);
        quiet.rows.push(Arc::new(Row { name: Some(Arc::from("A")), ..Row::default() }));
        let quiet = Arc::new(quiet);
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(section));
        sheet.sections.push(Arc::clone(&quiet));
        assert!(sheet.fit_string_units());
        let held = sheet.sections.first().and_then(|s| s.rows.first()).expect("the row");
        let unit = |name: &str| held.cells.get(name).and_then(|c| c.unit.as_deref().map(str::to_owned));
        assert_eq!((unit("Value"), unit("Label"), unit("Type")), (Some("STR".to_owned()), Some("STR".to_owned()), None));
        assert!(sheet.sections.get(1).is_some_and(|s| Arc::ptr_eq(s, &quiet)), "a section that needs nothing is the same allocation");
        assert!(!sheet.fit_string_units(), "and a second pass changes nothing");
    }

    #[test]
    fn only_the_value_of_a_user_or_field_row_is_an_untyped_text() {
        for (section, name) in [("User", "Value"), ("Field", "Value")] {
            assert!(text("Container").lacks_untyped_string_unit(Some(section), name, None), "{section}/{name}");
            assert!(
                Cell { formula: FormulaSlot::Inherit, ..text("web-01") }.lacks_untyped_string_unit(Some(section), name, None),
                "an inherited formula with a cached text, {section}/{name}"
            );
        }
        // A `Property` row's `Value` is typed by its `Type`, and a cell of a text kind carries no unit in the files of the program that opens them: neither is one of these.
        assert!(!text("Ann").lacks_untyped_string_unit(Some("Property"), "Value", None));
        assert!(!text("Name").lacks_untyped_string_unit(Some("Property"), "Label", None));
        assert!(!text("a").lacks_untyped_string_unit(Some("User"), "Prompt", None));
        assert!(!text("a").lacks_untyped_string_unit(Some("Field"), "Format", None));
        assert!(!text("x").lacks_untyped_string_unit(Some("Scratch"), "A", None));
        assert!(!text("x").lacks_untyped_string_unit(None, "Value", None), "a cell under the sheet is no row's value");
        // What is no text, and what says a unit already, is left as it is.
        for left in ["12", "", "1.#INF", "Themed", "#FF8000"] {
            assert!(!text(left).lacks_untyped_string_unit(Some("User"), "Value", None), "{left:?} is no text");
        }
        assert!(!text("Container").with_unit("STR").lacks_untyped_string_unit(Some("User"), "Value", None));
        assert!(!text("Container").with_unit("IN").lacks_untyped_string_unit(Some("User"), "Value", None));
        assert!(!Cell::new().lacks_untyped_string_unit(Some("User"), "Value", None), "no value, no text");
    }

    #[test]
    fn a_sheet_is_given_the_unit_of_its_untyped_texts_alone_and_shares_the_rest() {
        let row_of = |name: &str, cells: &[(&str, &str)]| {
            let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
            for (cell, value) in cells {
                row.cells.insert(CellName::new(cell), text(value));
            }
            Arc::new(row)
        };
        let mut user = Section::new("User", None);
        user.rows.push(row_of("msvStructureType", &[("Value", "Container"), ("Prompt", "kind")]));
        let mut property = Section::new("Property", None);
        property.rows.push(row_of("Name", &[("Value", "Ann"), ("Label", "Name")]));
        let property = Arc::new(property);
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(user));
        sheet.sections.push(Arc::clone(&property));
        assert!(sheet.lacks_untyped_string_units());
        assert!(sheet.fit_untyped_string_units());
        let unit = |section: usize, name: &str| {
            sheet
                .sections
                .get(section)
                .and_then(|s| s.rows.first())
                .and_then(|r| r.cells.get(name))
                .and_then(|c| c.unit.as_deref().map(str::to_owned))
        };
        assert_eq!((unit(0, "Value"), unit(0, "Prompt")), (Some("STR".to_owned()), None), "the value of the user row, and not its prompt");
        assert_eq!((unit(1, "Value"), unit(1, "Label")), (None, None), "the property row is not touched");
        assert!(sheet.sections.get(1).is_some_and(|s| Arc::ptr_eq(s, &property)), "a section that needs nothing is the same allocation");
        assert!(!sheet.lacks_untyped_string_units() && !sheet.fit_untyped_string_units(), "and a second pass changes nothing");
        // The pass for every text is another pass: it still finds the property row and the prompt.
        assert!(sheet.lacks_string_units());
    }

    fn row_of(cells: &[(&str, &str)]) -> Row {
        let mut row = Row { name: Some(Arc::from("r")), ..Row::default() };
        for (name, value) in cells {
            row.cells.insert(CellName::new(name), text(value));
        }
        row
    }

    #[test]
    fn a_field_row_whose_type_says_it_is_no_string_is_typed_by_it() {
        // A date field caches a date: its `Value` is the date's to say, and is no string whatever its text reads as.
        for (ty, string) in [("0", true), ("5", false), ("2", false), ("6", false), ("7", false), ("3", false)] {
            let row = row_of(&[("Type", ty), ("Value", "27 novembre 2026")]);
            let value = row.cells.get("Value").expect("the value");
            assert_eq!(value.lacks_untyped_string_unit(Some("Field"), "Value", Some(&row)), string, "Type {ty}");
        }
        // A row with no `Type` (the cell an instance holds over its master's row), one whose `Type` is no number and the cell asked of with no row say nothing: a text is a string.
        let none = row_of(&[("Value", "web-01")]);
        assert!(none.cells.get("Value").is_some_and(|c| c.lacks_untyped_string_unit(Some("Field"), "Value", Some(&none))));
        let odd = row_of(&[("Type", "five"), ("Value", "web-01")]);
        assert!(odd.cells.get("Value").is_some_and(|c| c.lacks_untyped_string_unit(Some("Field"), "Value", Some(&odd))));
        assert!(text("web-01").lacks_untyped_string_unit(Some("Field"), "Value", None));
    }

    #[test]
    fn a_sheet_leaves_the_value_of_a_typed_field_row_and_gives_the_unit_to_the_string_rows_beside_it() {
        let mut field = Section::new("Field", None);
        field.rows.push(Arc::new(row_of(&[("Type", "5"), ("Value", "27 novembre 2026")])));
        field.rows.push(Arc::new(row_of(&[("Type", "0"), ("Value", "plan.odg")])));
        field.rows.push(Arc::new(row_of(&[("Value", "web-01")])));
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(field));
        assert!(sheet.fit_untyped_string_units());
        let unit = |at: usize| {
            sheet.sections.first().and_then(|s| s.rows.get(at)).and_then(|r| r.cells.get("Value")).and_then(|c| c.unit.as_deref().map(str::to_owned))
        };
        assert_eq!((unit(0), unit(1), unit(2)), (None, Some("STR".to_owned()), Some("STR".to_owned())), "the date row stays, the others are strings");
        assert!(!sheet.lacks_untyped_string_units() && !sheet.fit_untyped_string_units());
    }

    /// A drawing that came from a package of a VSDX file whose extended properties say `application` (or say none).
    fn from_a_package(application: Option<&str>) -> crate::document::Document {
        use crate::package::{Kept, VsdxHeld};
        use crate::raw::{Passthrough, Placed, RawAttrs, RawElement, RawNode};
        let mut held = VsdxHeld::default();
        let mut kept = Kept::default();
        if let Some(name) = application {
            let element = RawElement { name: Arc::from("Application"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(name))] };
            kept.kept = Passthrough::from_nodes(vec![Placed { after: 0, node: RawNode::Element(element) }]);
        }
        held.props_rest.push((Arc::from("Properties"), kept));
        let mut doc = crate::document::Document::empty();
        doc.vsdx = Some(Arc::new(held));
        doc
    }

    #[test]
    fn the_units_are_diacraft_s_to_state_in_a_drawing_of_no_package_or_of_a_package_that_names_diacraft() {
        assert!(units_are_ours_to_state(&crate::document::Document::empty()), "a drawing that came from no package");
        assert!(units_are_ours_to_state(&from_a_package(Some("DiaCraft"))));
        assert!(units_are_ours_to_state(&from_a_package(Some("  DiaCraft\n"))), "white space round the name is none");
        for other in ["Other Writer 9", "Quill Draw 3", "diacraft", "DiaCraft 2", ""] {
            assert!(!units_are_ours_to_state(&from_a_package(Some(other))), "{other:?}");
        }
        assert!(!units_are_ours_to_state(&from_a_package(None)), "a package whose properties name no program is another program's");
    }

    #[test]
    fn the_application_a_package_names_is_read_from_what_the_reader_held_of_it() {
        assert_eq!(
            from_a_package(Some("Other Writer 9")).vsdx.as_deref().and_then(crate::package::VsdxHeld::application).as_deref(),
            Some("Other Writer 9")
        );
        assert_eq!(from_a_package(None).vsdx.as_deref().and_then(crate::package::VsdxHeld::application), None);
        // A name is cut to 64 characters, as the report of a read is.
        let long = "x".repeat(200);
        assert_eq!(from_a_package(Some(&long)).vsdx.as_deref().and_then(crate::package::VsdxHeld::application).map(|n| n.chars().count()), Some(64));
    }
}
