//! **The `Value` of a `User` or `Field` row that holds a text says `U="STR"` in every file the VSDX writers write**.
//! No `Type` cell in those rows says the value is a string, so the unit alone does: the program that opens the file takes a text with none for a value of the wrong
//! type and says so for each cell ("an element in this file contains data that is invalid or inappropriate for the expected type of data"). Gave the unit to the cells a build makes; these
//! are the cells that pass through a writer as the drawing holds them: the `Value` of the `User` rows of containers and swimlanes that a `.diacraft` file of an earlier build holds, and the cached text of the
//! `Field` rows of a VSDX file an earlier build wrote.
//!
//! * **The writers that write from the model** (a drawing never saved, and the model writer: a Save As from a `.diacraft` file or an `.odg`) say the unit on every such cell, whatever made it.
//! * **A save into a file** says it only in a file DiaCraft wrote (`docProps/app.xml` names it): the cells another program wrote are kept as it wrote them.
//! * **No other cell is touched.** A `Property` row's `Value` is typed by its `Type` (104,655 string rows of the sample files say no unit), and the cells of a text kind carry none in the files of the program
//!   that opens them (`Prop.Format`, `User.Prompt`), so a file that went through the model is not changed there.
//!
//! Every input is built in code. `unitless_text_cells` is the check: every `Value` of a `User` or `Field` row of a page, a master or the document whose `V` is no number, colour, date or truth value and that
//! says no unit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_opc::xml::{El, Node, parse};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{DIR, MasterSpec, PageSpec, VsdxBuilder, cell, shape_with};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{model_write, text_of};
use crate::read_ok;

const NOW: i64 = 1_700_000_000;

/// The parts the tests read, by name (`DIR` is the folder of the drawing's parts).
fn page1() -> String {
    format!("{DIR}/pages/page1.xml")
}

fn pages() -> String {
    format!("{DIR}/pages/pages.xml")
}

fn master1() -> String {
    format!("{DIR}/masters/master1.xml")
}

/// `docProps/app.xml` of a file DiaCraft wrote.
const DIACRAFT_APP: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>DiaCraft</Application></Properties>";

/// `docProps/app.xml` of a file another program wrote.
const OTHER_APP: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Test Writer</Application></Properties>";

/// The `User` rows of a container and a swimlane as a 0.2.3 `.diacraft` file holds them: the text in `V`, a string formula for two of them, and no unit.
const USER_ROWS: &str = "<Section N=\"User\">\
<Row N=\"msvStructureType\"><Cell N=\"Value\" V=\"Container\" F=\"&quot;Container&quot;\"/></Row>\
<Row N=\"DiaCraftContainerStyle\"><Cell N=\"Value\" V=\"frame\" F=\"&quot;frame&quot;\"/></Row>\
<Row N=\"DiaCraftSwimlane\"><Cell N=\"Value\" V=\"phase\"/></Row>\
<Row N=\"DiaCraftContainerHeading\"><Cell N=\"Value\" V=\"0\"/></Row>\
<Row N=\"DiaCraftContainerMargin\"><Cell N=\"Value\" V=\"0.25\" U=\"IN\"/></Row>\
<Row N=\"DiaCraftTint\"><Cell N=\"Value\" V=\"#FFCC00\"/></Row>\
</Section>";

/// The `Field` row of a network shape of 0.2.3: the cached text of an instance's own cell beside `F="Inh"`, with no unit, over a `Property` row that says `STR`.
const FIELD_ROWS: &str = "<Section N=\"Property\"><Row N=\"NetworkName\"><Cell N=\"Value\" V=\"web-01\" U=\"STR\"/><Cell N=\"Label\" V=\"Network Name\"/></Row></Section>\
<Section N=\"Field\"><Row IX=\"0\"><Cell N=\"Value\" V=\"web-01\" F=\"Inh\"/><Cell N=\"Format\" V=\"@\" U=\"STR\"/><Cell N=\"Type\" V=\"0\"/></Row></Section>";

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn is_number(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E')) && v.parse::<f64>().is_ok()
}

/// Whether `v` is a value that needs no unit to be what it is: nothing, a number, a colour, a date, a truth value, the theme's marker or an infinity.
fn needs_no_unit(v: &str) -> bool {
    let colour = v.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    let date = v.len() >= 10 && v.get(4..5) == Some("-") && v.get(7..8) == Some("-") && v.bytes().take(4).all(|b| b.is_ascii_digit());
    v.is_empty() || is_number(v) || colour || date || ["TRUE", "FALSE", "Themed", "1.#INF", "-1.#INF"].iter().any(|word| v.eq_ignore_ascii_case(word))
}

fn walk(el: &El, part: &str, section: &str, row: &str, out: &mut Vec<String>) {
    let (mut section, mut row) = (section.to_owned(), row.to_owned());
    if el.is("Section") {
        section = el.attr("N").unwrap_or("").to_owned();
    }
    if el.is("Row") {
        row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
    }
    if el.is("Cell")
        && matches!(section.as_str(), "User" | "Field")
        && el.attr("N") == Some("Value")
        && let Some(v) = el.attr("V")
        && el.attr("U").is_none_or(|u| u.trim().is_empty())
        && !needs_no_unit(v)
    {
        out.push(format!("{part} {section}/{row}/{} V={v:?} F={:?}", el.attr("N").unwrap_or(""), el.attr("F")));
    }
    for child in &el.children {
        if let Node::El(child) = child {
            walk(child, part, &section, &row, out);
        }
    }
}

/// Every `Value` of a `User` or `Field` row of the package `bytes` (pages, masters and the document) that holds a text and says no unit.
fn unitless_text_cells(bytes: &[u8]) -> Vec<String> {
    let package = Package::read(bytes, &Limits::default()).expect("a package");
    let mut names: Vec<String> = package.parts().filter(|name| name.ends_with(".xml")).map(str::to_owned).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        if name.contains("/pages/page") || name.contains("/masters/master") || name.ends_with("/document.xml") {
            let root = parse(package.part(&name).expect("the part")).expect("xml");
            walk(&root, &name, "", "", &mut out);
        }
    }
    out
}

/// The `V`, `U` and `F` of the cell at `place` (`Section/Row/Cell`, the row by its `N` or its `IX`) in the part `name` of the package `bytes`: the first such cell.
fn cell_at(bytes: &[u8], name: &str, place: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    fn find(el: &El, section: &str, row: &str, place: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
        let (mut section, mut row) = (section.to_owned(), row.to_owned());
        if el.is("Section") {
            section = el.attr("N").unwrap_or("").to_owned();
        }
        if el.is("Row") {
            row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
        }
        if el.is("Cell") && format!("{section}/{row}/{}", el.attr("N").unwrap_or("")) == place {
            let get = |a: &str| el.attr(a).map(str::to_owned);
            return Some((get("V"), get("U"), get("F")));
        }
        el.children.iter().find_map(|child| if let Node::El(child) = child { find(child, &section, &row, place) } else { None })
    }
    let package = Package::read(bytes, &Limits::default()).expect("a package");
    let part =
        package.parts().find(|p| p.eq_ignore_ascii_case(name.trim_start_matches('/')) || p.eq_ignore_ascii_case(name)).expect("the part").to_owned();
    find(&parse(package.part(&part).expect("the part")).expect("xml"), "", "", place)
}

/// `(V, U, F)` as a test says it.
fn cell3(v: &str, u: Option<&str>, f: Option<&str>) -> Option<(Option<String>, Option<String>, Option<String>)> {
    Some((Some(v.to_owned()), u.map(str::to_owned), f.map(str::to_owned)))
}

#[track_caller]
fn assert_every_text_says_str(what: &str, bytes: &[u8]) {
    let found = unitless_text_cells(bytes);
    assert!(found.is_empty(), "{what}: {} user and field values hold a text and say no unit:\n{}", found.len(), found.join("\n"));
}

fn row(section: &str, name: &str, cell: &str) -> CellAddr {
    CellAddr::in_row(section, None, RowKey::Name(Arc::from(name)), cell)
}

fn text(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

fn string_formula(v: &str) -> Cell {
    Cell::with_formula(&format!("\"{v}\""), Some(Value::from_text(v)))
}

/// A drawing never saved that holds the cells of an earlier build in each place a file holds one: a shape's `User` rows and `Field` row, a master's shape, a page's and the
/// document's `User` row. No cell of it says `STR`.
fn drawing() -> Document {
    let container = ShapeBuilder::new(11)
        .cell("PinX", "1")
        .set(row("User", "msvStructureType", "Value"), string_formula("Container"))
        .set(row("User", "DiaCraftContainerStyle", "Value"), string_formula("frame"))
        .set(row("User", "DiaCraftSwimlane", "Value"), text("phase"))
        .set(row("User", "DiaCraftContainerHeading", "Value"), text("0"))
        .set(CellAddr::in_row("User", None, RowKey::Name(Arc::from("DiaCraftContainerMargin")), "Value"), text("0.25").with_unit("IN"))
        .set(row("User", "DiaCraftTint", "Value"), text("#FFCC00"))
        .set(CellAddr::in_row("Field", None, RowKey::Ix(0), "Value"), Cell::inherited(Some(Value::from_text("web-01"))))
        .set(row("Property", "Name", "Label"), text("Name"))
        .set(row("Property", "Name", "Value"), text("Ann"));
    DocBuilder::new()
        .doc_cell("DocLangID", "en-US")
        .master(
            1,
            MasterBuilder::new().name("Master").shape(ShapeBuilder::new(5).set(row("User", "msvStructureType", "Value"), string_formula("List"))),
        )
        .page(1, PageBuilder::new().name("Page-1").shape(container).set(row("User", "Mark", "Value"), text("draft")))
        .build()
}

/// A file with a shape that holds the cells of an earlier build, and a master that holds a `User` row, written by the program the `app.xml` names.
fn file_of(app: &str) -> Vec<u8> {
    let cells = format!("{}{}{USER_ROWS}{FIELD_ROWS}", cell("PinX", "1"), cell("PinY", "1"));
    let master = MasterSpec::new(1, "Container").shapes(&shape_with(5, " Type=\"Shape\"", &format!("{}{USER_ROWS}", cell("PinX", "1"))));
    VsdxBuilder::new().app(app).master(master).page(PageSpec::new(1, "Page-1").shapes(&shape_with(11, " Type=\"Shape\"", &cells))).build()
}

/// A save into `original` of `doc`, with `baseline` the model the file was read as.
fn save_into(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    let original = OriginalBytes { bytes: original, baseline: Some(baseline) };
    write_with(doc, Some(&original), &options()).unwrap_or_else(|e| panic!("{e}"))
}

/// The page text of `bytes` with every ` U="STR"` taken out, for a comparison of what is left.
fn without_str(text: &str) -> String {
    text.replace(" U=\"STR\"", "")
}

// ---- the writers that write from the model -----------------------------------------------------------------------------------------------

#[test]
fn a_drawing_never_saved_writes_every_text_cell_with_str() {
    let doc = drawing();
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert_every_text_says_str("a drawing never saved", &written.bytes);
    let b = &written.bytes;
    assert_eq!(cell_at(b, &page1(), "User/msvStructureType/Value"), cell3("Container", Some("STR"), Some("\"Container\"")));
    assert_eq!(cell_at(b, &page1(), "User/DiaCraftContainerStyle/Value"), cell3("frame", Some("STR"), Some("\"frame\"")));
    assert_eq!(cell_at(b, &page1(), "User/DiaCraftSwimlane/Value"), cell3("phase", Some("STR"), None));
    assert_eq!(cell_at(b, &page1(), "Field/0/Value"), cell3("web-01", Some("STR"), Some("Inh")));
    // No other cell is touched: a text kind's cell carries no unit in the files of the program that opens them, and a `Property` row's `Value` is typed by its `Type`.
    assert_eq!(cell_at(b, &page1(), "Property/Name/Label"), cell3("Name", None, None));
    assert_eq!(cell_at(b, &page1(), "Property/Name/Value"), cell3("Ann", None, None));
    assert_eq!(cell_at(b, &pages(), "User/Mark/Value"), cell3("draft", Some("STR"), None), "a page's own sheet");
    assert_eq!(cell_at(b, &master1(), "User/msvStructureType/Value"), cell3("List", Some("STR"), Some("\"List\"")), "a master's shape");
    // What is no text is left as it is: a number, a number with its unit and a colour.
    assert_eq!(cell_at(b, &page1(), "User/DiaCraftContainerHeading/Value"), cell3("0", None, None));
    assert_eq!(cell_at(b, &page1(), "User/DiaCraftContainerMargin/Value"), cell3("0.25", Some("IN"), None));
    assert_eq!(cell_at(b, &page1(), "User/DiaCraftTint/Value"), cell3("#FFCC00", None, None));
}

#[test]
fn the_model_writer_writes_every_text_cell_with_str_whatever_made_it() {
    // A drawing read from a file is written from the model alone; the cells are the earlier build's, in both the visit that keeps the bookkeeping and the edited one.
    let (doc, _) = read_ok(&file_of(DIACRAFT_APP));
    for unchanged in [true, false] {
        let written = model_write(&doc, unchanged);
        assert_every_text_says_str(&format!("the model writer, content unchanged: {unchanged}"), &written.bytes);
        assert_eq!(cell_at(&written.bytes, &page1(), "Field/0/Value"), cell3("web-01", Some("STR"), Some("Inh")));
        assert_eq!(cell_at(&written.bytes, &page1(), "User/msvStructureType/Value"), cell3("Container", Some("STR"), Some("\"Container\"")));
    }
}

#[test]
fn what_a_writer_wrote_reads_back_as_the_drawing_with_the_units() {
    let doc = drawing();
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read_ok(&written.bytes);
    let user = |d: &Document, name: &str| {
        let page = d.pages.iter().find(|p| p.id == 1).expect("a page");
        let shape = page.shapes.iter().find(|s| s.id == 11).expect("the shape");
        let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").expect("User");
        let row = section.rows.iter().find(|r| r.name.as_deref() == Some(name)).expect("the row");
        row.cells.get("Value").map(|c| (c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref().map(str::to_owned)))
    };
    assert_eq!(user(&back, "msvStructureType"), Some((Some("Container".to_owned()), Some("STR".to_owned()))));
    assert_eq!(user(&back, "DiaCraftContainerHeading"), Some((Some("0".to_owned()), None)));
    assert_eq!(user(&back, "DiaCraftContainerMargin"), Some((Some("0.25".to_owned()), Some("IN".to_owned()))));
    // The drawing the caller keeps is not changed by a write.
    assert_eq!(user(&doc, "msvStructureType"), Some((Some("Container".to_owned()), None)));
}

// ---- a save into a file ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_save_into_a_file_diacraft_wrote_gives_its_text_cells_the_unit_they_lack() {
    let bytes = file_of(DIACRAFT_APP);
    let (base, _) = read_ok(&bytes);
    assert!(!unitless_text_cells(&bytes).is_empty(), "the control: the file holds cells that say no unit");
    // No edit at all: the splice writes the cells the file has, with the unit a text carries.
    let written = save_into(&base, &bytes, &base);
    assert_every_text_says_str("an unedited save into a file DiaCraft wrote", &written.bytes);
    let page = text_of(&written.bytes, &page1());
    assert_eq!(cell_at(&written.bytes, &page1(), "Field/0/Value"), cell3("web-01", Some("STR"), Some("Inh")));
    assert_eq!(cell_at(&written.bytes, &page1(), "User/msvStructureType/Value"), cell3("Container", Some("STR"), Some("\"Container\"")));
    assert_eq!(cell_at(&written.bytes, &master1(), "User/DiaCraftSwimlane/Value"), cell3("phase", Some("STR"), None), "a master's shape too");
    // Nothing else of the page or the master changed: what is left when the units are taken out is the file's own text, in the order it had.
    assert_eq!(without_str(&page), without_str(&text_of(&bytes, &page1())));
    assert_eq!(without_str(&text_of(&written.bytes, &master1())), without_str(&text_of(&bytes, &master1())));
    // The drawing a save hands back is what the file reads as, and is the baseline of the next save, which then has nothing more to give.
    let again = save_into(&written.document, &written.bytes, &written.document);
    assert_eq!(text_of(&again.bytes, &page1()), page, "a second save writes the same page");
    assert_eq!(text_of(&again.bytes, &master1()), text_of(&written.bytes, &master1()), "and the same master");
}

#[test]
fn a_save_into_a_file_diacraft_wrote_gives_the_unit_to_an_edited_drawing_too() {
    let bytes = file_of(DIACRAFT_APP);
    let (base, _) = read_ok(&bytes);
    let mut doc = base.clone();
    let sheet = diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), diacraft_doc::CellEdit::Value(Value::from_text("2")), diacraft_doc::WriteOrigin::Drawing)
        .unwrap();
    let written = save_into(&doc, &bytes, &base);
    assert_every_text_says_str("an edited save into a file DiaCraft wrote", &written.bytes);
    assert!(text_of(&written.bytes, &page1()).contains("<Cell N=\"PinX\" V=\"2\"/>"), "the edit is written");
}

#[test]
fn a_save_into_a_file_another_program_wrote_keeps_its_cells_as_they_were() {
    // The same file, written by another program: nothing is added to a cell it wrote, with no edit and with an edit of another cell.
    let theirs = file_of(OTHER_APP);
    let (base, _) = read_ok(&theirs);
    let unedited = save_into(&base, &theirs, &base);
    assert_eq!(text_of(&unedited.bytes, &page1()), text_of(&theirs, &page1()), "no edit, no change of the page");
    assert_eq!(text_of(&unedited.bytes, &master1()), text_of(&theirs, &master1()), "or of the master");
    let mut doc = base.clone();
    let sheet = diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), diacraft_doc::CellEdit::Value(Value::from_text("2")), diacraft_doc::WriteOrigin::Drawing)
        .unwrap();
    let edited = save_into(&doc, &theirs, &base);
    assert_eq!(text_of(&edited.bytes, &page1()), text_of(&theirs, &page1()).replacen("<Cell N=\"PinX\" V=\"1\"/>", "<Cell N=\"PinX\" V=\"2\"/>", 1));
    assert_eq!(text_of(&edited.bytes, &master1()), text_of(&theirs, &master1()), "the master stays");
}

#[test]
fn an_unedited_save_into_a_file_diacraft_wrote_that_says_str_already_rewrites_no_part() {
    // The cells a build of today writes: a save with no edit writes the file as it is, and the walk copies nothing.
    let doc = drawing();
    let first = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let (base, _) = read_ok(&first.bytes);
    let again = save_into(&base, &first.bytes, &base);
    assert_eq!(text_of(&again.bytes, &page1()), text_of(&first.bytes, &page1()));
    assert_eq!(text_of(&again.bytes, &master1()), text_of(&first.bytes, &master1()));
    assert!(!again.rewritten.iter().any(|name| name.contains("/pages/page") || name.contains("/masters/master")), "{:?}", again.rewritten);
}

// ---- the function the writers ask --------------------------------------------------------------------------------------------------------

#[test]
fn string_units_changes_what_it_must_and_finds_nothing_more_in_what_it_gave() {
    let doc = drawing();
    let fitted = diacraft_vsdx::string_units(&doc).expect("the drawing holds text cells that say no unit");
    assert_eq!(fitted.sheets, 3, "the page's shape, the master's shape and the page's own sheet");
    assert!(diacraft_vsdx::string_units(&fitted.doc).is_none(), "what it gave stands");
    // A drawing with no such cell is not copied.
    let none = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(1).cell("PinX", "1"))).build();
    assert!(diacraft_vsdx::string_units(&none).is_none());
    // The drawing it was given is the caller's, as it was.
    let user = |d: &Document| {
        let shape = d.pages.get(0).and_then(|p| p.shapes.get(0)).expect("the shape");
        let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").expect("User");
        section.rows.first().and_then(|r| r.cells.get("Value")).map(|c| c.unit.as_deref().map(str::to_owned))
    };
    assert_eq!(user(&doc), Some(None));
    assert_eq!(user(&fitted.doc), Some(Some("STR".to_owned())));
}

#[test]
fn a_page_that_needs_nothing_stays_the_allocation_it_was() {
    let clean = PageBuilder::new()
        .name("Page-2")
        .shape(ShapeBuilder::new(1).cell("PinX", "1").set(row("User", "Mark", "Value"), text("draft").with_unit("STR")));
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(1).set(row("User", "Mark", "Value"), text("draft"))))
        .page(2, clean)
        .build();
    let fitted = diacraft_vsdx::string_units(&doc).expect("the first page needs it");
    let page = |d: &Document, at: usize| Arc::clone(d.pages.get(at).expect("a page"));
    assert!(!Arc::ptr_eq(&page(&doc, 0), &page(&fitted.doc, 0)), "the page that needed it is a copy");
    assert!(Arc::ptr_eq(&page(&doc, 1), &page(&fitted.doc, 1)), "the page that needed nothing is shared");
}

/// A drawing never saved of `shapes` shapes, a third of which hold a text with no unit.
fn many_shapes(shapes: u32) -> Document {
    let mut page = PageBuilder::new().name("Page-1");
    for id in 1..=shapes {
        let shape = ShapeBuilder::new(id).cell("PinX", "1");
        page = page.shape(if id % 3 == 0 { shape.set(row("User", "Mark", "Value"), text("draft")) } else { shape });
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn the_walk_looks_at_each_sheet_once_and_gives_the_unit_to_the_sheets_that_lack_it() {
    let doc = many_shapes(3_000);
    let (walked, fitted) = (diacraft_vsdx::work::string_sheets_walked(), diacraft_vsdx::work::string_sheets_fitted());
    let made = diacraft_vsdx::string_units(&doc).expect("a third of the shapes hold one");
    let walked = diacraft_vsdx::work::string_sheets_walked() - walked;
    let fitted = diacraft_vsdx::work::string_sheets_fitted() - fitted;
    assert_eq!(fitted, 1_000, "one write for each sheet that lacked the unit");
    assert_eq!(made.sheets, 1_000);
    // The shapes, the page's sheet, the document's and the root style's.
    assert_eq!(walked, 3_003, "one look at each sheet");
    // What it gave is looked at once more, and none is given a second time.
    let (walked, fitted) = (diacraft_vsdx::work::string_sheets_walked(), diacraft_vsdx::work::string_sheets_fitted());
    assert!(diacraft_vsdx::string_units(&made.doc).is_none());
    assert_eq!(diacraft_vsdx::work::string_sheets_walked() - walked, 3_003);
    assert_eq!(diacraft_vsdx::work::string_sheets_fitted() - fitted, 0);
}

#[test]
fn groups_nested_as_deep_as_the_model_allows_are_reached_and_no_deeper() {
    // A group inside a group, a few more levels than the model's depth: the cell of each level the model holds is given the unit, and the walk is not as deep as the nesting is.
    let levels = diacraft_doc::MAX_SHAPE_DEPTH + 10;
    let mut shape = ShapeBuilder::new(u32::try_from(levels).unwrap()).group().set(row("User", "Mark", "Value"), text("draft"));
    for id in (1..levels).rev() {
        shape = ShapeBuilder::new(u32::try_from(id).unwrap()).group().set(row("User", "Mark", "Value"), text("draft")).child(shape);
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build();
    let fitted = diacraft_vsdx::work::string_sheets_fitted();
    let made = diacraft_vsdx::string_units(&doc).expect("the levels the model holds");
    assert_eq!(diacraft_vsdx::work::string_sheets_fitted() - fitted, u64::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap());
    // The levels the model does not hold are not looked at, so nothing more is found in what it gave.
    assert!(diacraft_vsdx::string_units(&made.doc).is_none());
}

#[test]
fn a_hostile_section_of_two_rows_of_one_name_gives_the_unit_to_both() {
    // The section holds two `Mark` rows (a file may): an address names the first, but the walk is over the rows, so neither is left a text with no unit.
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(1).set(row("User", "Mark", "Value"), text("one"))))
        .build();
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    let shape = Arc::make_mut(page.shapes.get_mut(0).expect("the shape"));
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").expect("User"));
    let mut twin = section.rows.first().map(|r| diacraft_doc::Row::clone(r)).expect("the row");
    twin.cells.insert(diacraft_doc::CellName::new("Value"), text("two"));
    section.rows.push(Arc::new(twin));
    let made = diacraft_vsdx::string_units(&doc).expect("two texts with no unit");
    let shape = made.doc.pages.get(0).and_then(|p| p.shapes.get(0)).expect("the shape");
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").expect("User");
    let held: Vec<(String, Option<String>)> = section
        .rows
        .iter()
        .filter_map(|r| r.cells.get("Value"))
        .map(|c| (c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(), c.unit.as_deref().map(str::to_owned)))
        .collect();
    assert_eq!(held, vec![("one".to_owned(), Some("STR".to_owned())), ("two".to_owned(), Some("STR".to_owned()))]);
}

#[test]
fn only_a_file_whose_extended_properties_name_diacraft_is_a_file_of_ours() {
    let props = |inner: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">{inner}</Properties>"
        )
    };
    let cases: [(&str, String, bool); 7] = [
        ("DiaCraft", props("<Application>DiaCraft</Application>"), true),
        ("DiaCraft among other elements", props("<AppVersion>1</AppVersion><Application>DiaCraft</Application><Company>c</Company>"), true),
        ("DiaCraft with white space round it", props("<Application> DiaCraft </Application>"), true),
        ("another program", props("<Application>Test Writer</Application>"), false),
        ("no Application at all", props("<AppVersion>16</AppVersion>"), false),
        ("the name in another case", props("<Application>diacraft</Application>"), false),
        ("a part that is no XML", "<Properties><Application>DiaCraft".to_owned(), false),
    ];
    for (what, app, ours) in cases {
        let bytes = file_of(&app);
        let (base, _) = read_ok(&bytes);
        // The file may not read its properties (a part that is no XML is kept as it was, with a warning): the save is the same either way.
        let written = save_into(&base, &bytes, &base);
        let page = text_of(&written.bytes, &page1());
        assert_eq!(unitless_text_cells(&written.bytes).is_empty(), ours, "{what}: a file of ours is given the unit, any other is kept");
        if !ours {
            assert_eq!(page, text_of(&bytes, &page1()), "{what}: the page is the file's own");
        }
    }
}

#[test]
fn a_file_with_no_properties_part_is_not_a_file_of_ours_and_saves() {
    // A package that holds no extended properties at all: the writer makes none for a file it saves into, and the file is another program's.
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").shapes(&shape_with(11, " Type=\"Shape\"", &format!("{}{USER_ROWS}", cell("PinX", "1")))))
        .pkg()
        .without_rels_of_type("/", &format!("{}extended-properties", diacraft_testkit::package::OD))
        .build();
    let (base, _) = read_ok(&bytes);
    let written = save_into(&base, &bytes, &base);
    assert_eq!(text_of(&written.bytes, &page1()), text_of(&bytes, &page1()), "the page is the file's own");
}

// ---- whose cells they are -----------------------------------------------------------------------------------------------

#[test]
fn the_model_writer_writes_the_cells_of_another_programs_package_as_they_were_read() {
    // The reference recheck: a package whose extended properties name another program, taken through the model, gained `STR` on the text of its `User` rows.
    let (doc, report) = read_ok(&file_of(OTHER_APP));
    assert_eq!(report.application.as_deref(), Some("Test Writer"));
    for unchanged in [true, false] {
        let written = model_write(&doc, unchanged);
        let page = written.bytes.as_slice();
        assert_eq!(
            cell_at(page, &page1(), "User/msvStructureType/Value"),
            cell3("Container", None, Some("\"Container\"")),
            "content unchanged: {unchanged}"
        );
        assert_eq!(cell_at(page, &page1(), "Field/0/Value"), cell3("web-01", None, Some("Inh")), "content unchanged: {unchanged}");
        assert_eq!(cell_at(page, &master1(), "User/DiaCraftSwimlane/Value"), cell3("phase", None, None), "content unchanged: {unchanged}");
        assert!(!unitless_text_cells(page).is_empty(), "the control: the cells are still the other program's");
    }
    // A package that names no program is no file of DiaCraft's either.
    let (nameless, _) = read_ok(&file_of(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"/>",
    ));
    assert!(diacraft_vsdx::string_units(&nameless).is_none());
}

#[test]
fn a_package_that_names_diacraft_is_diacraft_s_to_state_and_so_is_a_drawing_of_no_package() {
    let (ours, _) = read_ok(&file_of(DIACRAFT_APP));
    let (theirs, _) = read_ok(&file_of(OTHER_APP));
    assert!(diacraft_doc::units_are_ours_to_state(&ours));
    assert!(!diacraft_doc::units_are_ours_to_state(&theirs));
    assert!(diacraft_doc::units_are_ours_to_state(&drawing()), "a drawing built in code came from no package");
    // The model route asks it; the save into a file of DiaCraft's is judged by the file's properties, whatever the model holds of them.
    assert!(diacraft_vsdx::string_units(&ours).is_some());
    assert!(diacraft_vsdx::string_units(&theirs).is_none());
    assert!(
        diacraft_vsdx::string_units_in_file(&theirs).is_some(),
        "a file of DiaCraft's that a program of another name once edited is judged by its own properties"
    );
}

#[test]
fn a_date_field_is_typed_by_its_type_and_is_not_given_str() {
    // A `Field` row whose `Type` is 5 (a date) holds a date's cached value: a text with no unit there is the date's to say, and the pass leaves it. A string row, and a row with no `Type` (the cell an
    // instance holds over its master's row), get the unit.
    let field = |ty: Option<&str>| {
        let mut shape = ShapeBuilder::new(1).cell("PinX", "1").set(CellAddr::in_row("Field", None, RowKey::Ix(0), "Value"), text("27 novembre 2026"));
        if let Some(ty) = ty {
            shape = shape.set(CellAddr::in_row("Field", None, RowKey::Ix(0), "Type"), text(ty));
        }
        DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build()
    };
    let unit_of = |doc: &Document| {
        let shape = doc.pages.get(0).and_then(|p| p.shapes.get(0)).expect("the shape");
        let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Field").expect("Field");
        section.rows.first().and_then(|r| r.cells.get("Value")).map(|c| c.unit.as_deref().map(str::to_owned))
    };
    for (ty, typed) in [(Some("5"), true), (Some("2"), true), (Some("6"), true), (Some("7"), true), (Some("0"), false), (None, false)] {
        let doc = field(ty);
        match diacraft_vsdx::string_units(&doc) {
            Some(fitted) => {
                assert!(!typed, "Type {ty:?} is typed and is left alone");
                assert_eq!(unit_of(&fitted.doc), Some(Some("STR".to_owned())), "Type {ty:?}");
            }
            None => assert!(typed, "Type {ty:?} is a string or says nothing, and is given the unit"),
        }
    }
    // And written: the cell says no unit in the file, whichever writer.
    let written = write_with(&field(Some("5")), None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(cell_at(&written.bytes, &page1(), "Field/0/Value"), cell3("27 novembre 2026", None, None));
}
