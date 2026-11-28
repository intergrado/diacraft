//! **The VSDX writers write a language cell the way the files of the program that opens them hold it.** A `LangID` or `DocLangID` cell of a file holds the language as a plain
//! tag (`V="fr-FR"`), with no `F` and no `U`: 75,418 of the 75,418 `Character` cells of the sample files, and every `Shape Data` one that holds a value
//! A cell that holds a **string formula** (`F="&quot;fr-FR&quot;"`, `U="STR"`: what typing `"fr-FR"` in the formula bar makes)
//! is a formula that gives a text where the cell is a language, so the program reads it as an error and falls back to a language of its own. Every writer (a drawing never saved, the
//! model writer and a save into a file for a cell it writes) writes the tag alone; **a cell the file already holds is not rewritten**, so a save with no edit writes what it read.
//!
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::lang;
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, FormulaSlot, Owner, RowKey, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape_with};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{model_write, text_of};
use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const PAGE1: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines

/// What typing `"<tag>"` in the formula bar of a language cell makes: the formula, its value and the unit of a text.
fn typed(tag: &str) -> Cell {
    Cell::with_formula(&format!("\"{tag}\""), Some(Value::from_text(tag))).with_unit("STR")
}

fn row(section: &str, ix: u32, name: &str) -> CellAddr {
    CellAddr::in_row(section, None, RowKey::Ix(ix), name)
}

/// A drawing never saved that holds a language cell of the string form in each place a file holds one: a `Character` row of a page's shape (the rows 1, 2 and 4 of the first run's
/// languages), the shape's own `LangID`, a `Shape Data` row's, a master's shape's, a style's and the document's.
fn drawing() -> Document {
    let shape = ShapeBuilder::new(11)
        .cell("PinX", "1")
        .set(row("Character", 0, "LangID"), typed("en-US"))
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.2")
        .set(row("Character", 1, "LangID"), typed("fr-FR"))
        .row_cell("Character", None, RowKey::Ix(2), "LangID", "x-none")
        .set(row("Character", 3, "LangID"), typed("tr-TR"))
        .set(CellAddr::direct("LangID"), typed("de-CH"))
        .set(CellAddr::in_row("Property", None, RowKey::Name(Arc::from("Greeting")), "LangID"), typed("es-ES"));
    let mut doc = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Korean").set(row("Character", 0, "LangID"), typed("ko-KR")))
        .master(1, MasterBuilder::new().name("Master").shape(ShapeBuilder::new(5).set(row("Character", 0, "LangID"), typed("ja-JP"))))
        .page(1, PageBuilder::new().name("Page-1").shape(shape))
        .build();
    let document = SheetRef::Document;
    doc.write_cell(&document, &CellAddr::direct("DocLangID"), CellEdit::Cell(typed("nl-NL")), WriteOrigin::AutomationForce).unwrap();
    doc
}

/// The start tag of every `LangID` and `DocLangID` cell of every XML part of the package `bytes`, in the order of the parts and of the text.
fn language_tags(bytes: &[u8]) -> Vec<String> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let mut names: Vec<String> = package.parts().filter(|name| name.ends_with(".xml")).map(str::to_owned).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let text = String::from_utf8_lossy(package.part(&name).unwrap_or_default()).into_owned();
        for key in ["<Cell N=\"LangID\"", "<Cell N=\"DocLangID\""] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(key) {
                let end = rest.get(at..).and_then(|t| t.find('>')).map_or(rest.len(), |e| at + e + 1);
                out.push(rest.get(at..end).unwrap_or_default().to_owned());
                rest = rest.get(end..).unwrap_or_default();
            }
        }
    }
    out.sort();
    out
}

/// The tags in the order [`language_tags`] gives them.
fn sorted<const N: usize>(tags: [String; N]) -> Vec<String> {
    let mut tags = tags.to_vec();
    tags.sort();
    tags
}

/// `<Cell N="LangID" V="<tag>"/>`: the form the files hold.
fn plain(name: &str, tag: &str) -> String {
    format!("<Cell N=\"{name}\" V=\"{tag}\"/>")
}

fn expected_plain() -> Vec<String> {
    let mut want: Vec<String> =
        ["en-US", "fr-FR", "x-none", "tr-TR", "de-CH", "es-ES", "ko-KR", "ja-JP"].iter().map(|t| plain("LangID", t)).collect();
    want.push(plain("DocLangID", "nl-NL"));
    want.sort();
    want
}

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn save_into(doc: &Document, original: &[u8], baseline: Option<&Document>) -> Written {
    let original = OriginalBytes { bytes: original, baseline };
    write_with(doc, Some(&original), &options()).unwrap_or_else(|e| panic!("{e}"))
}

/// A file that holds one run of each language, as `lang_rows` says them (the `Character` rows' `LangID` cells as text of the file), and a shape with a plain `PinX`.
fn file_with(lang_rows: &[&str]) -> Vec<u8> {
    let rows: String = lang_rows.iter().enumerate().map(|(ix, c)| format!(r#"<Row IX="{ix}">{c}<Cell N="Size" V="0.2"/></Row>"#)).collect();
    let inner = format!("{}<Section N=\"Character\">{rows}</Section><Text><cp IX=\"0\"/>ab</Text>", cell("PinX", "1"));
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape_with(11, " Type=\"Shape\"", &inner))).build()
}

/// The text of a `LangID` cell, as the model holds it, in `Character` row `ix` of the first shape of the first page.
fn held(doc: &Document, ix: u32) -> Cell {
    let page = doc.pages.get(0).expect("a page");
    let shape = page.shapes.get(0).expect("a shape");
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").expect("a Character section");
    let row = section.rows.iter().find(|r| r.ix == Some(ix)).expect("the row");
    row.cells.get("LangID").expect("a LangID cell").clone()
}

fn write_cell(doc: &mut Document, ix: u32, cell: Cell) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &row("Character", ix, "LangID"), CellEdit::Cell(cell), WriteOrigin::AutomationForce).unwrap();
}

// ---- a drawing never saved ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_new_drawing_writes_every_language_cell_as_the_tag_alone() {
    let written = write_with(&drawing(), None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(language_tags(&written.bytes), expected_plain());
}

#[test]
fn what_a_new_drawing_is_written_as_reads_back_in_the_languages_it_held() {
    let written = write_with(&drawing(), None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read_ok(&written.bytes);
    for (ix, tag) in [(0, "en-US"), (1, "fr-FR"), (2, "x-none"), (3, "tr-TR")] {
        let cell = held(&back, ix);
        assert_eq!(cell.value.as_ref().map(Value::text), Some(tag), "row {ix}");
        assert!(matches!(cell.formula, FormulaSlot::Absent) && cell.unit.is_none(), "row {ix} is the tag alone: {cell:?}");
    }
    // What the save hands back is what the file reads as.
    assert_eq!(held(&written.document, 1).value.as_ref().map(Value::text), Some("fr-FR"));
}

// ---- the model writer --------------------------------------------------------------------------------------------------------------------

#[test]
fn the_model_writer_writes_a_drawing_that_came_from_a_file_with_the_language_cells_as_the_tag_alone() {
    // A drawing that holds a package (one read from a file) is written from the model alone; a model from another source (a `.diacraft` file of an earlier build) holds the string form.
    let (mut doc, _) = read_ok(&file_with(&[&cell("LangID", "en-US"), &cell("LangID", "fr-FR")]));
    write_cell(&mut doc, 1, typed("fr-CA"));
    let written = model_write(&doc, false);
    assert_eq!(language_tags(&written.bytes), sorted([plain("LangID", "en-US"), plain("LangID", "fr-CA")]));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(held(&back, 1).value.as_ref().map(Value::text), Some("fr-CA"));
}

// ---- a save into a file ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_save_into_a_file_writes_the_cell_a_person_typed_as_the_tag_alone_and_leaves_the_rest_as_it_was() {
    let bytes = file_with(&[&cell("LangID", "en-US"), &cell("LangID", "fr-FR")]);
    let (base, _) = read_ok(&bytes);
    let mut doc = base.clone();
    write_cell(&mut doc, 1, typed("fr-CA"));
    let written = save_into(&doc, &bytes, Some(&base));
    assert_eq!(language_tags(&written.bytes), sorted([plain("LangID", "en-US"), plain("LangID", "fr-CA")]));
    // Only that cell changed: the page part is the original's with the one value.
    let was = text_of(&bytes, PAGE1);
    assert_eq!(text_of(&written.bytes, PAGE1), was.replacen("V=\"fr-FR\"", "V=\"fr-CA\"", 1));
}

#[test]
fn a_save_into_a_file_that_holds_the_string_form_takes_it_out_of_a_cell_that_is_written_again() {
    // A file an earlier build wrote: the string form. A person sets the cell to another language, typing it as a formula again: the cell is written as the tag alone, with the formula and the unit
    // taken out and not left beside a value they no longer say.
    let old = r#"<Cell N="LangID" V="fr-FR" U="STR" F="&quot;fr-FR&quot;"/>"#;
    let bytes = file_with(&[&cell("LangID", "en-US"), old]);
    let (base, _) = read_ok(&bytes);
    let mut doc = base.clone();
    write_cell(&mut doc, 1, typed("de-DE"));
    let written = save_into(&doc, &bytes, Some(&base));
    assert_eq!(language_tags(&written.bytes), sorted([plain("LangID", "en-US"), plain("LangID", "de-DE")]));
}

#[test]
fn a_save_into_a_file_leaves_a_cell_of_the_string_form_the_file_holds_as_the_file_has_it() {
    // A save with no edit writes what it read: the file's own cell is kept verbatim, formula and unit and all, and so it is by an edit of another cell.
    let old = r#"<Cell N="LangID" V="fr-FR" U="STR" F="&quot;fr-FR&quot;"/>"#;
    let bytes = file_with(&[&cell("LangID", "en-US"), old]);
    let (base, _) = read_ok(&bytes);
    assert_eq!(language_tags(&bytes)[1], old, "the control: the file holds the string form");
    let unedited = save_into(&base, &bytes, Some(&base));
    assert_eq!(text_of(&unedited.bytes, PAGE1), text_of(&bytes, PAGE1), "no edit, no change");
    let mut edited = base.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    edited.write_cell(&sheet, &row("Character", 0, "Size"), CellEdit::Value(Value::from_text("0.3")), WriteOrigin::Drawing).unwrap();
    let written = save_into(&edited, &bytes, Some(&base));
    assert!(language_tags(&written.bytes).contains(&old.to_owned()), "{:?}", language_tags(&written.bytes));
    assert!(text_of(&written.bytes, PAGE1).contains("V=\"0.3\""), "the control: the page was written");
}

// ---- what is not a tag typed as a string --------------------------------------------------------------------------------------------------

#[test]
fn a_language_cell_that_is_a_formula_of_something_else_is_written_as_it_is() {
    // A formula that is no string of its own (it reads another cell) is the person's formula: it is kept, and so is its unit.
    let mut doc = drawing();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let other = Cell::with_formula("Prop.Greeting.Value", Some(Value::from_text("es-ES"))).with_unit("STR");
    doc.write_cell(&sheet, &row("Character", 0, "LangID"), CellEdit::Cell(other), WriteOrigin::AutomationForce).unwrap();
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let tags = language_tags(&written.bytes);
    assert!(tags.contains(&r#"<Cell N="LangID" V="es-ES" U="STR" F="Prop.Greeting.Value"/>"#.to_owned()), "{tags:?}");
    // The cells next to it are still written as the tag alone.
    assert!(tags.contains(&plain("LangID", "fr-FR")), "{tags:?}");
}

#[test]
fn a_language_cell_that_holds_a_text_as_a_text_loses_the_unit_and_keeps_the_value() {
    let mut doc = drawing();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let text = Cell::constant(Value::from_text("it-IT")).with_unit("STR");
    doc.write_cell(&sheet, &row("Character", 2, "LangID"), CellEdit::Cell(text), WriteOrigin::AutomationForce).unwrap();
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert!(language_tags(&written.bytes).contains(&plain("LangID", "it-IT")));
}

// ---- reading both forms ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_number_and_the_string_form_are_both_read_and_both_mean_the_language() {
    let old = r#"<Cell N="LangID" V="fr-FR" U="STR" F="&quot;fr-FR&quot;"/>"#;
    let bytes = file_with(&[&cell("LangID", "1036"), old, &cell("LangID", "fr-FR")]);
    let (doc, _) = read_ok(&bytes);
    let document = lang::fallback_language();
    for ix in 0..3 {
        let text = held(&doc, ix).value.as_ref().map(|v| v.text().to_owned());
        assert_eq!(lang::effective(text.as_deref(), &document).shaping(), "fr-FR", "row {ix}: {text:?}");
    }
    // A number is kept as the file has it: a save with no edit writes it back.
    let unedited = save_into(&doc, &bytes, Some(&doc));
    assert_eq!(language_tags(&unedited.bytes), language_tags(&bytes));
}

// ---- the function the writers ask --------------------------------------------------------------------------------------------------------

#[test]
fn plain_languages_changes_what_it_must_and_finds_nothing_more_in_what_it_gave() {
    let doc = drawing();
    let plain_doc = diacraft_vsdx::plain_languages(&doc, None).expect("the string form is in the drawing");
    assert!(diacraft_vsdx::plain_languages(&plain_doc, None).is_none(), "what it gave stands");
    // A drawing with no such cell is not copied.
    let none = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(1).cell("PinX", "1"))).build();
    assert!(diacraft_vsdx::plain_languages(&none, None).is_none());
    // The cells it gave are the tag alone, in the model.
    let cell = held(&plain_doc, 1);
    assert_eq!(cell.value.as_ref().map(Value::text), Some("fr-FR"));
    assert!(matches!(cell.formula, FormulaSlot::Absent) && cell.unit.is_none() && cell.error.is_none(), "{cell:?}");
    // And the drawing it was given is the caller's, as it was.
    assert!(matches!(held(&doc, 1).formula, FormulaSlot::Local(_)));
}

// ---- hostile and large drawings ----------------------------------------------------------------------------------------------------------

/// A drawing never saved of `shapes` shapes, each with one `Character` row whose `LangID` is the string form.
fn many_shapes(shapes: u32) -> Document {
    let mut page = PageBuilder::new().name("Page-1");
    for id in 1..=shapes {
        page = page.shape(ShapeBuilder::new(id).cell("PinX", "1").set(row("Character", 0, "LangID"), typed("fr-FR")));
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn the_walk_looks_at_each_sheet_once_and_writes_each_cell_once() {
    let doc = many_shapes(3_000);
    let (sheets, cells) = (diacraft_vsdx::work::language_sheets_walked(), diacraft_vsdx::work::language_cells_written());
    let plain_doc = diacraft_vsdx::plain_languages(&doc, None).expect("every shape holds the string form");
    let sheets = diacraft_vsdx::work::language_sheets_walked() - sheets;
    let cells = diacraft_vsdx::work::language_cells_written() - cells;
    assert_eq!(cells, 3_000, "one write for each cell");
    // The shapes, the page's sheet, the document's and the root style's.
    assert_eq!(sheets, 3_003, "one look at each sheet");
    assert!(diacraft_vsdx::plain_languages(&plain_doc, None).is_none());
}

#[test]
fn a_save_into_a_file_looks_only_at_the_sheets_an_edit_copied() {
    // The file holds every shape's cell in the string form (an earlier build wrote it), and the session edits one cell of one shape: the other 2,999 shapes are the file's own allocations and are
    // not looked at, and the one cell is the only one written.
    let base = many_shapes(3_000);
    let mut doc = base.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 1_500));
    doc.write_cell(&sheet, &row("Character", 0, "LangID"), CellEdit::Cell(typed("de-DE")), WriteOrigin::AutomationForce).unwrap();
    let (sheets, cells) = (diacraft_vsdx::work::language_sheets_walked(), diacraft_vsdx::work::language_cells_written());
    let plain_doc = diacraft_vsdx::plain_languages(&doc, Some(&base)).expect("the edited cell");
    let sheets = diacraft_vsdx::work::language_sheets_walked() - sheets;
    let cells = diacraft_vsdx::work::language_cells_written() - cells;
    assert_eq!(cells, 1);
    assert!(sheets <= 4, "the edited shape and the page that holds it, not the 3,000: {sheets}");
    let edited =
        plain_doc.pages.get(0).unwrap().shapes.get(1_500).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "Character").unwrap().rows[0]
            .cells
            .get("LangID")
            .unwrap()
            .clone();
    assert_eq!(edited.value.as_ref().map(Value::text), Some("de-DE"));
    // A drawing that is the baseline is not looked at at all.
    let before = diacraft_vsdx::work::language_sheets_walked();
    assert!(diacraft_vsdx::plain_languages(&base, Some(&base)).is_none());
    assert_eq!(diacraft_vsdx::work::language_sheets_walked(), before, "the very allocations the baseline holds");
}

/// One shape whose `Character` section holds `rows` rows, each `LangID` the string form.
fn many_rows(rows: u32) -> Document {
    let mut shape = ShapeBuilder::new(1).cell("PinX", "1");
    for ix in 0..rows {
        shape = shape.set(row("Character", ix, "LangID"), typed("tr-TR"));
    }
    DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build()
}

#[test]
fn a_section_of_many_rows_is_written_in_one_write_for_each_row() {
    let doc = many_rows(20_000);
    let cells = diacraft_vsdx::work::language_cells_written();
    let plain_doc = diacraft_vsdx::plain_languages(&doc, None).expect("every row holds the string form");
    assert_eq!(diacraft_vsdx::work::language_cells_written() - cells, 20_000);
    assert!(diacraft_vsdx::plain_languages(&plain_doc, None).is_none());
}

/// The most rows a section holds, written at a measured time (`--ignored --nocapture`): a hang guard of a minute, and no cost held to a time.
#[test]
#[ignore = "timed: the section of the most rows the model holds"]
fn a_section_of_the_most_rows_is_written_within_a_minute() {
    let doc = many_rows(u32::try_from(diacraft_doc::MAX_ROWS_PER_SECTION).unwrap());
    let start = std::time::Instant::now();
    let plain_doc = diacraft_vsdx::plain_languages(&doc, None).expect("every row holds the string form");
    let took = start.elapsed();
    eprintln!("plain_languages on {} rows: {took:?}", diacraft_doc::MAX_ROWS_PER_SECTION);
    assert!(took < std::time::Duration::from_secs(60), "{took:?}");
    assert!(diacraft_vsdx::plain_languages(&plain_doc, None).is_none());
}

#[test]
fn groups_nested_as_deep_as_the_model_allows_are_reached_and_no_deeper() {
    // A group inside a group, a few more levels than the model's depth: the cell of each level the model holds is written, and the walk is not as deep as the nesting is.
    let levels = diacraft_doc::MAX_SHAPE_DEPTH + 10;
    let mut shape = ShapeBuilder::new(u32::try_from(levels).unwrap()).group().set(row("Character", 0, "LangID"), typed("ja-JP"));
    for id in (1..levels).rev() {
        shape = ShapeBuilder::new(u32::try_from(id).unwrap()).group().set(row("Character", 0, "LangID"), typed("ja-JP")).child(shape);
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build();
    let cells = diacraft_vsdx::work::language_cells_written();
    let plain_doc = diacraft_vsdx::plain_languages(&doc, None).expect("the levels the model holds");
    assert_eq!(diacraft_vsdx::work::language_cells_written() - cells, u64::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap());
    // The levels the model does not hold are not looked at, so nothing more is found in what it gave.
    assert!(diacraft_vsdx::plain_languages(&plain_doc, None).is_none());
}

#[test]
fn a_cell_of_a_second_row_of_one_key_is_left_as_it_is_and_never_written_over_the_first_rows() {
    // A hostile sheet holds two rows of one `IX`: an address names the first, so a write for the second would change the first row's cell. Neither is written.
    let mut doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(1).cell("PinX", "1").row_cell(
                "Character",
                None,
                RowKey::Ix(0),
                "LangID",
                "en-US",
            )),
        )
        .build();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Character").unwrap();
    let mut second = diacraft_doc::Row { ix: Some(0), ..diacraft_doc::Row::default() };
    second.cells.insert(diacraft_doc::CellName::new("LangID"), typed("fr-FR"));
    Arc::make_mut(section).rows.push(Arc::new(second));
    assert!(diacraft_vsdx::plain_languages(&doc, None).is_none(), "the second row's cell is not a cell an address can name");
    assert_eq!(held(&doc, 0).value.as_ref().map(Value::text), Some("en-US"), "the first row's cell is as it was");
}
