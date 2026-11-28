//! The field rows of a drawing saved across to VSDX say what the VSDX specifications need.
//!
//! The `.odg` reader makes a field row of the `Value` cell alone (a formula and the text the field shows, with no unit), and an older build made a field with an empty `Format`, and a master
//! of the Organization Chart Shapes with one. A program that reads the file takes a text with no unit for a number it cannot read and an empty picture for no text at all
//! So **the model a VSDX writer is given has them** (`across::fields`): the `Type` of the field's kind, the `Format` of
//! `STR` on a text, and a date with the unit of a date. The tests read **the bytes of the file written**, saved as a VSDX file from an `.odg`, from the
//! `.diacraft` file written from it and from an old drawing; and the file read back and saved again is the file.

use diacraft_engine::Session;
use diacraft_opc::xml::{El, Node, parse};
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing, open_bytes_in, package, rect};
use crate::run;

/// A rectangle whose text is five fields: the page number, the page count, the file name, a date and a time, as a producer caches what each shows.
const FIELDS: &str = r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="fields" svg:width="9cm" svg:height="2cm" svg:x="8cm" svg:y="3cm"><text:p>Page <text:page-number>5</text:page-number> of <text:page-count>7</text:page-count> in <text:file-name>plan.odg</text:file-name> on <text:date>11/27/2026 3:28 PM</text:date> at <text:time>3:28 PM</text:time></text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#;

fn drawing() -> Drawing {
    Drawing::new()
        .meta("<meta:creation-date>2026-03-01T09:00:00</meta:creation-date><dc:date>2026-03-04T10:30:00</dc:date>")
        .page(&format!("{}{FIELDS}", rect("box", (2.0, 3.0, 4.0, 2.0), "")))
}

fn odg_bytes() -> Vec<u8> {
    bytes_of(&drawing())
}

fn bytes_of(d: &Drawing) -> Vec<u8> {
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), d.content_xml().into_bytes()),
            ("styles.xml".to_owned(), d.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), d.meta_xml().into_bytes()),
        ],
    )
}

/// The cells of each row of the `Field` section of the first shape that has one, as `(N, V, U, F)` by row, in the order of the rows.
type Cells = Vec<(String, Option<String>, Option<String>, Option<String>)>;

fn field_rows(page_xml: &str) -> Vec<Cells> {
    field_rows_of(page_xml, 0)
}

/// [`field_rows`] of the `nth` shape that has a `Field` section, in the order of the file.
fn field_rows_of(page_xml: &str, nth: usize) -> Vec<Cells> {
    fn find<'a>(el: &'a El, out: &mut Vec<&'a El>) {
        if el.is("Section") && el.attr("N") == Some("Field") {
            out.push(el);
        }
        for c in &el.children {
            if let Node::El(child) = c {
                find(child, out);
            }
        }
    }
    let root = parse(page_xml.as_bytes()).expect("xml");
    let mut sections = Vec::new();
    find(&root, &mut sections);
    let section = sections.get(nth).expect("a Field section");
    section
        .elements()
        .filter(|r| r.is("Row"))
        .map(|row| {
            row.elements()
                .filter(|c| c.is("Cell"))
                .map(|c| {
                    let get = |n: &str| c.attr(n).map(str::to_owned);
                    (get("N").unwrap_or_default(), get("V"), get("U"), get("F"))
                })
                .collect()
        })
        .collect()
}

/// The cell `name` of a row: `(V, U, F)`.
fn cell(row: &Cells, name: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    row.iter().find(|(n, ..)| n == name).map(|(_, v, u, f)| (v.clone(), u.clone(), f.clone()))
}

fn part(bytes: &[u8], suffix: &str) -> String {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|p| p.ends_with(suffix)).unwrap_or_else(|| panic!("no part ends with {suffix}")).to_owned();
    String::from_utf8(package.part(&name).expect("the part").to_vec()).expect("text")
}

fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

/// One of the five rows of [`FIELDS`] as the file says it: the `Type`, the `Format`, `Value`'s `V` and `U`, and its formula.
type Row5 = (&'static str, &'static str, &'static str, Option<&'static str>, Option<&'static str>);

/// What the five rows of [`FIELDS`] say in the file: `(Type, Format, Value's V, Value's U)`.
fn assert_five_rows(what: &str, bytes: &[u8]) {
    assert_five_rows_of(what, bytes, 0);
}

/// [`assert_five_rows`] for the `nth` shape of the page that has a `Field` section.
fn assert_five_rows_of(what: &str, bytes: &[u8], nth: usize) {
    let page = part(bytes, "/pages/page1.xml");
    let rows = field_rows_of(&page, nth);
    assert_eq!(rows.len(), 5, "{what}: five fields:\n{page}");
    let said = |row: usize, name: &str| cell(&rows[row], name).unwrap_or_else(|| panic!("{what}: row {row} has no {name}:\n{page}"));
    let want: [Row5; 5] = [
        // The page number and the page count are unit-free numbers, of the type of a number and the plain picture.
        ("2", "esc(0)", "5", None, Some("PAGENUMBER()")),
        ("2", "esc(0)", "7", None, Some("PAGECOUNT()")),
        // The file name is a text: `STR`, and the picture of a text.
        ("0", "@", "plan.odg", Some("STR"), Some("FILENAME()")),
        // A date is a date (`U` `DATE`, and the date as text of the form) shown through the date and the time; a time through the picture of a time.
        ("5", "{<211>}", "2026-11-27T15:28:00", Some("DATE"), Some("NOW()")),
        ("5", "h:mm tt", "2026-03-04T10:30:00", Some("DATE"), Some("NOW()")),
    ];
    for (i, (type_cell, format, value, unit, formula)) in want.into_iter().enumerate() {
        assert_eq!(said(i, "Type").0, some(type_cell), "{what}: row {i} Type:\n{page}");
        let (picture, picture_unit, _) = said(i, "Format");
        assert_eq!((picture, picture_unit), (some(format), some("STR")), "{what}: row {i} Format:\n{page}");
        let (v, u, f) = said(i, "Value");
        assert_eq!((v, u, f), (some(value), unit.map(str::to_owned), formula.map(str::to_owned)), "{what}: row {i} Value:\n{page}");
    }
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    run(s, "file.save_as", json!({ "path": dir.path(name) }));
    dir.read(name)
}

#[test]
fn an_odg_with_each_field_kind_saved_as_vsdx_writes_the_type_the_picture_and_the_unit_of_each() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes());
    let saved = save_as(&mut s, &dir, "across.vsdx");
    assert_five_rows("an .odg saved as VSDX", &saved);
}

#[test]
fn and_through_a_diacraft_file_the_drawing_is_given_the_same() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes());
    run(&mut s, "file.save_as", json!({ "path": dir.path("between.diacraft") }));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("between.diacraft") }));
    let saved = save_as(&mut again, &dir, "from-native.vsdx");
    assert_five_rows("a .diacraft file saved as VSDX", &saved);
}

/// The page the save renames (`page1` is `Page-1` in a VSDX file) is followed by the fields that show its name, **and by no other field**: the text a file holds in the fields of the page
/// number and the page count (`5` and `7`, as a producer cached them) is kept until `text.update_fields`, as it is when no page is renamed. The rename is the writer's, and no person
/// edited the drawing; the page number and the page count of the file are the number and the count its producer counted.
#[test]
fn a_page_the_save_renames_is_followed_by_the_fields_that_show_its_name_and_by_no_other_field() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes());
    assert_eq!(crate::page_names(&s), ["page1"]);
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    let id = s.document().unwrap().page(page).unwrap().shapes.iter().find(|sh| sh.name_u.as_deref() == Some("box")).unwrap().id;
    // A field of the page's name on another shape, which shows `page1` (and `box` takes the first `Field` section of the file).
    run(&mut s, "text.insert_field", json!({"id": id, "page": page, "category": "page", "field": "name"}));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("named.vsdx") }));
    assert_eq!(crate::page_names(&s), ["Page-1"], "the save gave the page the name a VSDX file keeps");
    assert_checked_clean(&saved);
    let bytes = dir.read("named.vsdx");
    let page_xml = part(&bytes, "/pages/page1.xml");
    let rows = field_rows_of(&page_xml, 0);
    assert_eq!(rows.len(), 1, "{page_xml}");
    assert_eq!(
        cell(&rows[0], "Value"),
        Some((some("Page-1"), some("STR"), some("PAGENAME()"))),
        "the field of the name follows the page:\n{page_xml}"
    );
    assert!(page_xml.contains("<fld IX=\"0\">Page-1</fld>"), "the text shows the new name:\n{page_xml}");
    // The five fields the file held keep their text and their cached values.
    assert_five_rows_of("the fields the file held, in a drawing a save renamed a page of", &bytes, 1);
    // A save into the file, where the page has the name a VSDX file keeps, writes the same five fields.
    run(&mut s, "file.save", json!({}));
    assert_five_rows_of("a save into the file", &dir.read("named.vsdx"), 1);
}

/// And in a drawing of three pages (`page1` to `page3`, each given the name a VSDX file keeps by the save), where the cells that read the pages are worked out for a change to more than one page: the five
/// fields keep their text and their cached values.
#[test]
fn the_fields_keep_their_text_in_a_drawing_of_three_pages_the_save_renames_every_page_of() {
    let dir = Dir::new();
    let three = Drawing::new()
        .meta("<meta:creation-date>2026-03-01T09:00:00</meta:creation-date><dc:date>2026-03-04T10:30:00</dc:date>")
        .page(&format!("{}{FIELDS}", rect("box", (2.0, 3.0, 4.0, 2.0), "")))
        .page(&rect("second", (2.0, 3.0, 4.0, 2.0), ""))
        .page(&rect("third", (2.0, 3.0, 4.0, 2.0), ""));
    let mut s = open_bytes_in(&dir, "fields.odg", &bytes_of(&three));
    assert_eq!(crate::page_names(&s), ["page1", "page2", "page3"]);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("three.vsdx") }));
    assert_eq!(crate::page_names(&s), ["Page-1", "Page-2", "Page-3"]);
    assert_checked_clean(&saved);
    assert_five_rows("a drawing of three pages saved as VSDX", &dir.read("three.vsdx"));
    run(&mut s, "file.save", json!({}));
    assert_five_rows("a save into it", &dir.read("three.vsdx"));
}

/// What a save across says of itself: no class of data the check compared differs.
#[track_caller]
fn assert_checked_clean(saved: &serde_json::Value) {
    assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{saved}");
}

#[test]
fn the_drawing_becomes_what_the_file_reads_as_and_a_save_into_the_file_keeps_each_field_row() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes());
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("across.vsdx") }));
    assert_checked_clean(&saved);
    let first = dir.read("across.vsdx");
    // A save with no edit is the file as it is.
    run(&mut s, "file.save", json!({}));
    assert_eq!(dir.read("across.vsdx"), first, "an unedited save writes what the first wrote");
    // An edit elsewhere, and a save into the file: each field row still says what the first save wrote (the splice writes the edit and not a change to the rows).
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    let id = s.document().unwrap().page(page).unwrap().shapes.iter().find(|sh| sh.name_u.as_deref() == Some("box")).unwrap().id;
    run(&mut s, "shape.move", json!({"page": page, "ids": [id], "dx": 0.5, "dy": 0.25}));
    run(&mut s, "file.save", json!({}));
    assert_five_rows("a save into the file after an edit", &dir.read("across.vsdx"));
    // And the file opened again and saved as a VSDX file again is the same rows.
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("across.vsdx")}));
    let again = save_as(&mut back, &dir, "again.vsdx");
    assert_five_rows("the file opened and saved as another VSDX file", &again);
}

/// Changes every row of every `Field` section of the model itself, past the commands' checks, as an older build left them: `f` is given each row and whether it is a master's.
fn old_rows(s: &mut Session, mut f: impl FnMut(&mut diacraft_doc::Row, bool)) {
    use std::sync::Arc;
    fn rows_of(shape: &mut diacraft_doc::Shape, f: &mut dyn FnMut(&mut diacraft_doc::Row)) {
        let sheet = Arc::make_mut(&mut shape.sheet);
        for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "Field") {
            for row in Arc::make_mut(section).rows.iter_mut() {
                f(Arc::make_mut(row));
            }
        }
    }
    s.edit("test: an older build's field rows", |d| {
        for at in 0..d.masters.len() {
            let master = Arc::make_mut(d.masters.get_mut(at).unwrap());
            for i in 0..master.shapes.len() {
                rows_of(Arc::make_mut(master.shapes.get_mut(i).unwrap()), &mut |row| f(row, true));
            }
        }
        for at in 0..d.pages.len() {
            let page = Arc::make_mut(d.pages.get_mut(at).unwrap());
            for i in 0..page.shapes.len() {
                rows_of(Arc::make_mut(page.shapes.get_mut(i).unwrap()), &mut |row| f(row, false));
            }
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_old_drawings_empty_formats_and_unit_less_texts_are_given_what_a_new_one_makes_when_it_is_saved_as_vsdx() {
    use crate::edit::id_of;
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"template": "organization_chart"}));
    let boss = id_of(&run(&mut s, "shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": 4.0, "y": 6.0})));
    run(&mut s, "shapedata.set", json!({"id": boss, "row": "Name", "value": "Ann Lee"}));
    run(&mut s, "shapedata.set", json!({"id": boss, "row": "Title", "value": "Chief"}));
    // A field of a text and a field of a number, as `text.insert_field` made them before: with no picture.
    run(&mut s, "text.insert_field", json!({"id": boss, "category": "document", "field": "creator"}));
    run(&mut s, "text.insert_field", json!({"id": boss, "category": "page", "field": "page_number"}));
    // The drawing as an older build left it: the master's field rows with an empty `Format` and no unit, the shape's own rows with no unit on a text, and the two fields with no picture.
    old_rows(&mut s, |row, master| {
        let blank = |row: &mut diacraft_doc::Row, name: &str| {
            if let Some(cell) = row.cells.get_mut(name) {
                cell.value = Some(diacraft_doc::Value::from_text(""));
                cell.unit = None;
            }
        };
        if master {
            blank(row, "Format");
            if let Some(cell) = row.cells.get_mut("Value") {
                cell.unit = None;
            }
        } else if let Some(cell) = row.cells.get_mut("Value") {
            cell.unit = None;
            if row.cells.contains_key("Format") {
                blank(row, "Format");
            }
        }
    });
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("old.vsdx") }));
    assert_checked_clean(&saved);
    let bytes = dir.read("old.vsdx");
    // The master: a picture for each of its two fields, which says it is a text.
    let master = part(&bytes, "/masters/master1.xml");
    let rows = field_rows(&master);
    assert_eq!(rows.len(), 2, "{master}");
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(cell(row, "Format"), Some((some("@"), some("STR"), None)), "master row {i}:\n{master}");
        assert_eq!(cell(row, "Type").map(|c| c.0), Some(some("0")), "master row {i}:\n{master}");
        assert_eq!(cell(row, "Value").map(|c| c.1), Some(some("STR")), "master row {i}:\n{master}");
    }
    // The shape: its inherited rows say `STR` on the text they cache, and the two fields it was given have a picture of their kind.
    let page = part(&bytes, "/pages/page1.xml");
    let rows = field_rows(&page);
    assert_eq!(rows.len(), 4, "{page}");
    assert_eq!(cell(&rows[0], "Value"), Some((some("Ann Lee"), some("STR"), some("Inh"))), "{page}");
    assert_eq!(cell(&rows[1], "Value"), Some((some("Chief"), some("STR"), some("Inh"))), "{page}");
    assert_eq!(cell(&rows[2], "Format"), Some((some("@"), some("STR"), None)), "{page}");
    assert_eq!(cell(&rows[3], "Format"), Some((some("esc(0)"), some("STR"), None)), "{page}");
    assert_eq!(cell(&rows[3], "Value").map(|c| c.1), Some(None), "a number says no unit: {page}");
    // A save with no edit after it is the file written.
    run(&mut s, "file.save", json!({}));
    assert_eq!(dir.read("old.vsdx"), bytes, "an unedited save writes what the first wrote");
}
