//! The fields of an `.odg` through an edit and a save in place. A field on a master page, which the reader makes a background page, shows the number
//! of the page that uses it, so an edit's own refresh leaves its text as the file saved it, as for a background page: it is not worked out to the
//! background page's own number. A field the refresh does change keeps its element in the file: the splice writes it again around the new characters
//! (`crates/odg/src/write/zero.rs`). The drawings are written by hand (`odg_build`), a package and a flat file.

use diacraft_doc::{Owner, TextPart};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::odg_build::{DRAWING, Dir, Drawing, open_bytes_in, package, part_of, rect};
use crate::run;

/// A frame on the master page `Default` whose text is `Page ` and a page number field, with the placeholder a producer caches there.
const MASTER_FRAME: &str = r#"<draw:frame draw:layer="backgroundobjects" draw:name="footer" svg:width="4cm" svg:height="1cm" svg:x="1cm" svg:y="27cm"><draw:text-box><text:p>Page <text:page-number>&lt;number&gt;</text:page-number></text:p></draw:text-box></draw:frame>"#;

const EMPTY_MASTER: &str = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#;

fn full_master() -> String {
    format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{MASTER_FRAME}</style:master-page>"#)
}

/// One page holding a rectangle, `shapes` beside it, on a master page holding [`MASTER_FRAME`]: as a package, or as a flat file.
fn drawing(shapes: &str) -> Drawing {
    Drawing::new().page(&format!("{}{shapes}", rect("box", (2.0, 3.0, 4.0, 2.0), "")))
}

fn package_bytes(d: &Drawing) -> Vec<u8> {
    let styles = d.styles_xml().replace(EMPTY_MASTER, &full_master());
    assert!(styles.contains("text:page-number"), "the master page holds the field");
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), d.content_xml().into_bytes()),
            ("styles.xml".to_owned(), styles.into_bytes()),
            ("meta.xml".to_owned(), d.meta_xml().into_bytes()),
        ],
    )
}

fn flat_bytes(d: &Drawing) -> Vec<u8> {
    let flat = d.flat_xml(DRAWING).replace(EMPTY_MASTER, &full_master());
    assert!(flat.contains("text:page-number"), "the master page holds the field");
    flat.into_bytes()
}

/// The `ID` of the rectangle on the first page that is not a background, and that page's `ID`.
fn the_box(s: &Session) -> (u32, u32) {
    let doc = s.document().unwrap();
    let page = doc.pages.iter().find(|p| !p.background).unwrap();
    let shape = page.shapes.iter().find(|sh| sh.name_u.as_deref() == Some("box")).unwrap();
    (page.id, shape.id)
}

/// What the fields of the shapes of every page show, by page and shape.
fn fields_shown(s: &Session) -> Vec<(bool, String)> {
    let doc = s.document().unwrap();
    let mut out = Vec::new();
    for page in doc.pages.iter() {
        for shape in page.shapes.iter() {
            for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
                if let TextPart::Fld { text, .. } = part {
                    out.push((page.background, text.to_string()));
                }
            }
        }
    }
    out
}

fn warnings(saved: &Json) -> Vec<String> {
    saved["warnings"].as_array().map(|w| w.iter().filter_map(Json::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

/// Opens `bytes` as `name`, moves the rectangle and saves in place: what the save said and the file written.
fn move_and_save(dir: &Dir, name: &str, bytes: &[u8]) -> (Session, Json, Vec<u8>) {
    let mut s = open_bytes_in(dir, name, bytes);
    assert!(fields_shown(&s).contains(&(true, "<number>".to_owned())), "the reader holds the master's field: {:?}", fields_shown(&s));
    let (page, id) = the_box(&s);
    run(&mut s, "shape.move", json!({"page": page, "ids": [id], "dx": 0.5, "dy": 0.25}));
    // The master's field shows what the file saved: it is the number of the page that uses it, which no refresh of the background page knows.
    assert!(fields_shown(&s).contains(&(true, "<number>".to_owned())), "the move left the master's field alone: {:?}", fields_shown(&s));
    let saved = run(&mut s, "file.save", json!({}));
    let written = dir.read(name);
    (s, saved, written)
}

#[test]
fn a_field_on_a_master_page_keeps_its_text_and_its_element_through_a_move_and_a_save_of_a_package() {
    let dir = Dir::new();
    let (_, saved, written) = move_and_save(&dir, "a.odg", &package_bytes(&drawing("")));
    let said = warnings(&saved);
    assert!(said.iter().all(|w| !w.contains("field")), "no field was written as text: {said:?}");
    let styles = part_of(&written, "styles.xml");
    assert_eq!(styles.matches("<text:page-number").count(), 1, "{styles}");
    assert!(styles.contains("Page <text:page-number>&lt;number&gt;</text:page-number>"), "{styles}");
}

#[test]
fn a_field_on_a_master_page_keeps_its_text_and_its_element_through_a_move_and_a_save_of_a_flat_file() {
    let dir = Dir::new();
    let (_, saved, written) = move_and_save(&dir, "a.fodg", &flat_bytes(&drawing("")));
    let said = warnings(&saved);
    assert!(said.iter().all(|w| !w.contains("field")), "no field was written as text: {said:?}");
    let flat = String::from_utf8(written).unwrap();
    assert_eq!(flat.matches("<text:page-number").count(), 1, "{flat}");
    assert!(flat.contains("Page <text:page-number>&lt;number&gt;</text:page-number>"), "{flat}");
}

/// A field of the page itself whose cached text is stale (`5` on the first page): a move of its shape works it out again, and the save writes its element around
/// the new number instead of writing the number as text.
#[test]
fn a_stale_page_number_on_a_page_is_brought_up_to_date_by_a_move_and_keeps_its_element() {
    for (name, flat) in [("a.odg", false), ("a.fodg", true)] {
        let dir = Dir::new();
        let field = r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="numbered" svg:width="4cm" svg:height="1cm" svg:x="8cm" svg:y="3cm"><text:p>Page <text:page-number>5</text:page-number></text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#;
        let d = drawing(field);
        let bytes = if flat { flat_bytes(&d) } else { package_bytes(&d) };
        let mut s = open_bytes_in(&dir, name, &bytes);
        let doc = s.document().unwrap();
        let page = doc.pages.iter().find(|p| !p.background).unwrap().id;
        let numbered = doc.pages.iter().find(|p| p.id == page).unwrap().shapes.iter().find(|sh| sh.name_u.as_deref() == Some("numbered")).unwrap().id;
        run(&mut s, "shape.move", json!({"page": page, "ids": [numbered], "dx": 0.5, "dy": 0.0}));
        let at = s.document().unwrap().find_shape(Owner::Page(page), numbered).unwrap();
        let shown: Vec<String> = s
            .document()
            .unwrap()
            .shape(&at)
            .unwrap()
            .text
            .iter()
            .flat_map(|t| t.parts.iter())
            .filter_map(|p| if let TextPart::Fld { text, .. } = p { Some(text.to_string()) } else { None })
            .collect();
        assert_eq!(shown, ["1"], "{name}: the move worked the field out again");
        let saved = run(&mut s, "file.save", json!({}));
        let said = warnings(&saved);
        assert!(said.iter().all(|w| !w.contains("field")), "{name}: no field was written as text: {said:?}");
        let written = dir.read(name);
        let xml = if flat { String::from_utf8(written).unwrap() } else { part_of(&written, "content.xml") };
        assert!(xml.contains("Page <text:page-number>1</text:page-number>"), "{name}: {xml}");
        // The master's field is as the file saved it.
        assert!(xml.contains("<text:page-number>&lt;number&gt;</text:page-number>") || !flat, "{name}: {xml}");
    }
}

/// The gate's case: the first page that has a shape is the background page made of the master page, so the edit moves the master's frame itself. Its field still
/// shows what the file saved (the number of the page that uses the master page, which the background page has none of), and the file keeps its element.
#[test]
fn a_move_of_the_masters_own_frame_leaves_its_page_number_field_as_the_file_saved_it() {
    for (name, flat) in [("a.odg", false), ("a.fodg", true)] {
        let dir = Dir::new();
        let d = Drawing::new().page("");
        let bytes = if flat { flat_bytes(&d) } else { package_bytes(&d) };
        let mut s = open_bytes_in(&dir, name, &bytes);
        let doc = s.document().unwrap();
        let background = doc.pages.iter().find(|p| p.background).expect("the master page is a background page");
        let (page, frame) = (background.id, background.shapes.get(0).expect("the master's frame").id);
        run(&mut s, "shape.move", json!({"page": page, "ids": [frame], "dx": 0.5, "dy": 0.25}));
        assert_eq!(fields_shown(&s), [(true, "<number>".to_owned())], "{name}: the field shows what the file saved");
        let saved = run(&mut s, "file.save", json!({}));
        let said = warnings(&saved);
        assert!(said.iter().all(|w| !w.contains("field")), "{name}: no field was written as text: {said:?}");
        let written = dir.read(name);
        let xml = if flat { String::from_utf8(written).unwrap() } else { part_of(&written, "styles.xml") };
        assert_eq!(xml.matches("<text:page-number").count(), 1, "{name}: {xml}");
        assert!(xml.contains("Page <text:page-number>&lt;number&gt;</text:page-number>"), "{name}: {xml}");
        // `text.update_fields` leaves it too.
        run(&mut s, "text.update_fields", json!({}));
        assert_eq!(fields_shown(&s), [(true, "<number>".to_owned())], "{name}: an update leaves it");
    }
}
