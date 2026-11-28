//! A field row that holds a `Format` and a `Type` is still the ODF field of its kind in an `.odg` the model writer makes.
//!
//! A VSDX file DiaCraft writes gives each field row a `Type` and a `Format`, and a unit on a date. The model writer of the `.odg` wrote a field as its
//! element only when the row held its `Value` and nothing else, so **an `.odg` saved from that VSDX file had its fields as plain text**, and so had an `.odg` saved from a drawing whose fields
//! were made in DiaCraft (whose rows always held a `Format`). The element is written whatever the row holds: the file cannot say a `Type` or a `Format`, so DiaCraft's extension carries them,
//! as it carries a cell no attribute holds. The tests read **the bytes of the files written**: an `.odg` saved as a VSDX file and that file saved as an `.odg`, the same through a `.diacraft`
//! file, and the fields a person makes.

use diacraft_doc::{Document, TextPart};
use diacraft_engine::Session;
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::odg_build::{DRAWING, Dir, Drawing, open_bytes_in, package, part_of, rect};
use crate::run;

/// A rectangle whose text is five fields: the page number, the page count, a date, a time and the file name, as a producer caches what each shows. The date has its value.
fn fields_shape(name: &str) -> String {
    format!(
        r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" svg:width="9cm" svg:height="2cm" svg:x="8cm" svg:y="3cm"><text:p>Page <text:page-number>5</text:page-number> of <text:page-count>7</text:page-count> on <text:date text:date-value="2026-03-04">11/27/2026 3:28 PM</text:date> at <text:time>3:28 PM</text:time> in <text:file-name>plan.odg</text:file-name></text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    )
}

/// The drawing of the vector: the fields on one page, and on a second.
fn two_pages() -> Drawing {
    Drawing::new()
        .meta("<meta:creation-date>2026-03-01T09:00:00</meta:creation-date><dc:date>2026-03-04T10:30:00</dc:date>")
        .page(&format!("{}{}", rect("box", (2.0, 3.0, 4.0, 2.0), ""), fields_shape("fields")))
        .page(&fields_shape("again"))
}

fn odg_bytes(d: &Drawing) -> Vec<u8> {
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), d.content_xml().into_bytes()),
            ("styles.xml".to_owned(), d.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), d.meta_xml().into_bytes()),
        ],
    )
}

/// How many elements of each of the five kinds the part holds: `(page-number, page-count, date, time, file-name)`.
fn fields_in(xml: &str) -> [usize; 5] {
    let count = |kind: &str| xml.matches(&format!("<text:{kind}>")).count() + xml.matches(&format!("<text:{kind} ")).count();
    [count("page-number"), count("page-count"), count("date"), count("time"), count("file-name")]
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    run(s, "file.save_as", json!({ "path": dir.path(name) }));
    dir.read(name)
}

/// What a save says of itself: the check of the file made compared the file with the drawing and found no class of data that differs. (Not asked of a save from a VSDX file's rows, whose
/// text DiaCraft's data carries: the check names the held markup of its text, which the file read back does not hold, as it did when those fields were written as text.)
#[track_caller]
fn assert_checked_clean(saved: &serde_json::Value) {
    assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{saved}");
}

/// The file `name` opened in a session of its own.
fn open_file(dir: &Dir, name: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s
}

/// The `Field` rows of every shape of every page, as `(formula of Value, Value's text, Type, Format)`.
type Row4 = (Option<String>, Option<String>, Option<String>, Option<String>);

fn rows_of(d: &Document) -> Vec<Row4> {
    let mut out = Vec::new();
    for page in d.pages.iter().filter(|p| !p.background) {
        for shape in page.shapes.iter() {
            for section in shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Field") {
                for row in section.rows.iter() {
                    let of = |name: &str| row.cells.get(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
                    let formula = row.cells.get("Value").and_then(|c| c.formula.text()).map(str::to_owned);
                    out.push((formula, of("Value"), of("Type"), of("Format")));
                }
            }
        }
    }
    out
}

/// The characters each shape of the pages shows, fields among them.
fn shown_of(d: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for page in d.pages.iter().filter(|p| !p.background) {
        for shape in page.shapes.iter() {
            let Some(text) = shape.text.as_ref() else { continue };
            out.push(
                text.parts
                    .iter()
                    .map(|p| match p {
                        TextPart::Chars(c) => c.to_string(),
                        TextPart::Fld { text, .. } => text.to_string(),
                        _ => String::new(),
                    })
                    .collect(),
            );
        }
    }
    out
}

/// The fields of the vector in `xml`: five kinds in each of the two shapes.
#[track_caller]
fn assert_ten_fields(what: &str, bytes: &[u8]) {
    let xml = part_of(bytes, "content.xml");
    assert_eq!(fields_in(&xml), [2, 2, 2, 2, 2], "{what}:\n{xml}");
    assert!(
        xml.contains(">5</text:page-number>") && xml.contains(">7</text:page-count>") && xml.contains(">plan.odg</text:file-name>"),
        "{what}: the text each shows:\n{xml}"
    );
}

/// What the five fields of a shape come back as when the `.odg` is read: the formulas, in order.
const FORMULAS: [&str; 5] = ["PAGENUMBER()", "PAGECOUNT()", "NOW()", "NOW()", "FILENAME()"];

#[track_caller]
fn assert_rows_read_back(what: &str, bytes: &[u8]) {
    let dir = Dir::new();
    dir.write("read.odg", bytes);
    let back = open_file(&dir, "read.odg");
    let formulas: Vec<String> = rows_of(doc(&back)).into_iter().filter_map(|r| r.0).collect();
    let want: Vec<String> = FORMULAS.iter().chain(FORMULAS.iter()).map(|f| (*f).to_owned()).collect();
    assert_eq!(formulas, want, "{what}: the fields are fields when the file is read");
}

#[test]
fn the_vector_an_odg_with_five_fields_on_two_pages_through_a_vsdx_file_and_back_keeps_the_ten_fields() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    // The first file, as the .odg is: the five fields of each shape.
    assert_ten_fields("the file", &dir.read("fields.odg"));
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    let back = save_as(&mut vsdx, &dir, "back.odg");
    assert_ten_fields("an .odg saved from the VSDX file", &back);
    assert_rows_read_back("an .odg saved from the VSDX file", &back);
}

#[test]
fn the_time_is_the_element_of_a_time_and_the_date_the_element_of_a_date_after_a_vsdx_file() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    let xml = part_of(&save_as(&mut vsdx, &dir, "back.odg"), "content.xml");
    // The element of each, as a producer's file says it: no element of the other.
    assert!(xml.contains(">11/27/2026 3:28 PM</text:date>"), "{xml}");
    assert!(xml.contains(">3:28 PM</text:time>"), "{xml}");
}

#[test]
fn through_a_diacraft_file_the_odg_keeps_the_fields_too() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    // The drawing of the .odg, kept in the file of its own and saved as an .odg again.
    save_as(&mut s, &dir, "plain.diacraft");
    let mut native = open_file(&dir, "plain.diacraft");
    assert_ten_fields("an .odg through a .diacraft file", &save_as(&mut native, &dir, "from-native.odg"));
    // And the drawing of the VSDX file, kept in a .diacraft file: its rows hold a `Type` and a `Format`.
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    save_as(&mut vsdx, &dir, "from-vsdx.diacraft");
    let mut native = open_file(&dir, "from-vsdx.diacraft");
    let back = save_as(&mut native, &dir, "from-vsdx.odg");
    assert_ten_fields("an .odg through a VSDX file and a .diacraft file", &back);
    assert_rows_read_back("an .odg through a VSDX file and a .diacraft file", &back);
}

/// The model of the drawing is what it was: the `.odg` the fields went through reads as the rows, the texts and the formats the VSDX file read as (DiaCraft's extension carries what the
/// element cannot say, so the `Type`, the `Format` and the date's unit come back).
#[test]
fn the_rows_the_element_cannot_say_ride_in_the_file_and_come_back() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    let before = (rows_of(doc(&vsdx)), shown_of(doc(&vsdx)));
    assert!(before.0.iter().all(|r| r.2.is_some() && r.3.is_some()), "the VSDX file's rows hold a Type and a Format: {:?}", before.0);
    save_as(&mut vsdx, &dir, "back.odg");
    let back = open_file(&dir, "back.odg");
    assert_eq!((rows_of(doc(&back)), shown_of(doc(&back))), before);
}

/// A drawing a person makes: a shape for each field, saved to a `.diacraft` file (whose name `FILENAME()` shows) and then as an `.odg`.
fn made_drawing(dir: &Dir, fields: &[(&str, &str)]) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("made.diacraft") }));
    for (i, (category, field)) in fields.iter().enumerate() {
        let id = drop_shape(&mut s, "rectangle", 1.0 + i as f64, 3.0);
        run(&mut s, "shape.set_text", json!({"id": id, "text": "x "}));
        run(&mut s, "text.insert_field", json!({"id": id, "category": category, "field": field}));
    }
    s
}

#[test]
fn the_fields_a_person_makes_are_the_fields_of_the_odg() {
    let dir = Dir::new();
    let mut s = made_drawing(&dir, &[("page", "page_number"), ("page", "page_count"), ("document", "filename"), ("date_time", "current")]);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("made.odg") }));
    assert_checked_clean(&saved);
    let xml = part_of(&dir.read("made.odg"), "content.xml");
    assert_eq!(fields_in(&xml), [1, 1, 1, 0, 1], "{xml}");
    assert!(xml.contains(">made.diacraft</text:file-name>"), "{xml}");
    // The drawing says nothing of a field written as text.
    let said = saved["warnings"].to_string();
    assert!(!said.contains("field"), "{said}");
    // Read again: four fields, each with the row the person made.
    let back = open_file(&dir, "made.odg");
    let formulas: Vec<_> = rows_of(doc(&back)).into_iter().map(|r| (r.0.unwrap_or_default(), r.2, r.3)).collect();
    assert_eq!(
        formulas,
        [
            ("PAGENUMBER()".to_owned(), Some("2".to_owned()), Some("esc(0)".to_owned())),
            ("PAGECOUNT()".to_owned(), Some("2".to_owned()), Some("esc(0)".to_owned())),
            ("FILENAME()".to_owned(), Some("0".to_owned()), Some("@".to_owned())),
            ("NOW()".to_owned(), Some("5".to_owned()), Some("{<211>}".to_owned())),
        ]
    );
}

/// The fields DiaCraft knows that the `.odg` has no element for in its mapping (`PAGENAME()`, the other dates, the document's properties, the shape's own cells, a shape data row and a custom
/// formula) are the text they show, and the save says so, as it did.
#[test]
fn the_fields_the_odg_mapping_has_no_element_for_are_their_text_and_the_save_says_so() {
    let dir = Dir::new();
    let others = [
        ("page", "name"),
        ("date_time", "creation"),
        ("date_time", "last_edit"),
        ("document", "title"),
        ("document", "creator"),
        ("geometry", "width"),
        ("object", "id"),
    ];
    let mut s = made_drawing(&dir, &others);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("others.odg") }));
    assert_checked_clean(&saved);
    let xml = part_of(&dir.read("others.odg"), "content.xml");
    assert_eq!(fields_in(&xml), [0, 0, 0, 0, 0], "{xml}");
    // One warning for each shape, in the words the save has always had for a field that has no element.
    let said: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert_eq!(said.len(), others.len(), "{said:?}");
    assert!(said.iter().all(|w| w.contains("has no element in an .odg") && w.contains("plain text")), "{said:?}");
    // They come back as the rows they were.
    let back = open_file(&dir, "others.odg");
    assert_eq!(rows_of(doc(&back)).len(), others.len(), "{:?}", rows_of(doc(&back)));
}

/// Writes the files the black-box check of another program's reader opens into the folder `DIACRAFT_LO_DIR` names: an `#[ignore]`d helper of the report's LibreOffice run, and no test.
#[test]
#[ignore = "writes files for a black-box check by hand: DIACRAFT_LO_DIR names the folder"]
fn write_the_files_for_the_black_box_check() {
    let Ok(folder) = std::env::var("DIACRAFT_LO_DIR") else { return };
    let at = |name: &str| format!("{folder}/{name}");
    let dir = Dir::new();
    std::fs::write(at("a_original.odg"), odg_bytes(&two_pages())).unwrap();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    run(&mut s, "file.save_as", json!({ "path": at("b_across.vsdx") }));
    let mut vsdx = Session::new();
    run(&mut vsdx, "file.open", json!({ "path": at("b_across.vsdx") }));
    run(&mut vsdx, "file.save_as", json!({ "path": at("c_back.odg") }));
    run(&mut vsdx, "file.save_as", json!({ "path": at("d_from_vsdx.diacraft") }));
    let mut native = Session::new();
    run(&mut native, "file.open", json!({ "path": at("d_from_vsdx.diacraft") }));
    run(&mut native, "file.save_as", json!({ "path": at("e_through_diacraft.odg") }));
    let mut made = made_drawing(&dir, &[("page", "page_number"), ("page", "page_count"), ("document", "filename"), ("date_time", "current")]);
    run(&mut made, "file.save_as", json!({ "path": at("f_made.odg") }));
    let mut others = made_drawing(&dir, &[("page", "name"), ("date_time", "creation"), ("document", "title"), ("object", "id")]);
    run(&mut others, "file.save_as", json!({ "path": at("g_others.odg") }));
}

/// The `ID` of the shape named `name` and its page, on the pages that are no background.
fn named(s: &Session, name: &str) -> (u32, u32) {
    for page in doc(s).pages.iter().filter(|p| !p.background) {
        if let Some(shape) = page.shapes.iter().find(|sh| sh.name_u.as_deref() == Some(name)) {
            return (page.id, shape.id);
        }
    }
    panic!("no shape {name}");
}

/// A save into the file the model writer made (the splice) keeps the fields through an edit of the shape that holds them: the text put in before them, and a move.
#[test]
fn a_save_into_the_odg_the_fields_were_written_to_keeps_them_through_an_edit_of_their_shape() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    save_as(&mut vsdx, &dir, "back.odg");
    let (page, id) = named(&vsdx, "fields");
    run(&mut vsdx, "text.insert", json!({"id": id, "page": page, "text": "! ", "at": 0}));
    run(&mut vsdx, "shape.move", json!({"page": page, "ids": [id], "dx": 0.5, "dy": 0.25}));
    let saved = run(&mut vsdx, "file.save", json!({}));
    assert_eq!(saved["warnings"], json!([]), "{saved}");
    let xml = part_of(&dir.read("back.odg"), "content.xml");
    assert_eq!(fields_in(&xml), [2, 2, 2, 2, 2], "{xml}");
    // The edit worked the fields of the shape out again: page 1 of 2. The element is the file's own, with the text put in before it.
    assert!(xml.contains("! Page <text:page-number>1</text:page-number>"), "{xml}");
    assert!(xml.contains(r#"<text:date text:date-value="2026-03-04">"#), "{xml}");
}

/// The template is written by the same model writer: its text holds the elements too. (Save As does not write a flat file: a drawing opened from one is saved by Save.)
#[test]
fn a_template_saved_from_the_vsdx_file_holds_the_fields_too() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "fields.odg", &odg_bytes(&two_pages()));
    save_as(&mut s, &dir, "across.vsdx");
    let mut vsdx = open_file(&dir, "across.vsdx");
    let template = save_as(&mut vsdx, &dir, "back.otg");
    assert_eq!(fields_in(&part_of(&template, "content.xml")), [2, 2, 2, 2, 2]);
}
