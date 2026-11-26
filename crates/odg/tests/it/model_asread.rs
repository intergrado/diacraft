//! The `.odg` model writer writes a shape back **as it was read** (the native-format design's §3.3, §3.4 and §4.4): while a shape's held values'
//! stamps hold it is its own element (`draw:rect`, `draw:ellipse`, ...), on the style name the file gave it, with its lengths in the units the file wrote them in, its frame, its layer and its
//! text style name only where the file had them. When a stamp no longer holds (the shape was edited) the shape is written from its cells, as a new shape is.
//!
//! Every read-back goes through the real reader, and "the same" is `compare_content` (the CANON lists) over `content.xml` and `styles.xml`.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, RawAttrs, RawElement, Shape, Value, VsdxHeld};
use diacraft_odg::{Limits, WriteOptions, compare_content, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699" draw:stroke="solid" svg:stroke-color="#000000"/></style:style>"##;

/// The model writer's options for a drawing that was not edited since it was read.
fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The package of a drawing (as ODF 1.4, the version the model writer writes), and the model the reader makes of it.
fn read_back(drawing: &Drawing) -> (Vec<u8>, Document) {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    let bytes = package(DRAWING, drawing.files());
    let (doc, _) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the vector does not read: {e}"));
    (bytes, doc)
}

fn save(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &unedited()).unwrap_or_else(|e| panic!("the save is refused: {e}")).bytes
}

fn part(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap().to_vec()).unwrap()
}

/// The differences between the file and the model written from what the reader made of it, in the parts that say what the drawing is (`content.xml` and `styles.xml`).
fn differences(original: &[u8], written: &[u8]) -> Vec<String> {
    let diff = compare_content(original, written, &Limits::default()).unwrap();
    diff.shown
        .iter()
        .filter(|d| matches!(d.part.as_str(), "content.xml" | "styles.xml"))
        .map(|d| format!("{} {} {:?}", d.part, d.path, d.found))
        .collect()
}

/// The drawing is written back with no difference in its content, and its shape is the element `tag` (`<draw:rect `), not a custom shape.
fn assert_written_as_read(drawing: &Drawing, tag: &str) {
    let (original, doc) = read_back(drawing);
    let written = save(&doc);
    let content = part(&written, "content.xml");
    assert!(content.contains(tag), "the shape is written as {tag}: {content}");
    if tag != "<draw:custom-shape " {
        assert!(!content.contains("<draw:custom-shape "), "no shape is made a custom shape: {content}");
    }
    let found = differences(&original, &written);
    assert!(found.is_empty(), "the content is the file's: {found:?}\n{content}");
}

const RECT: &str = r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;

fn with(shapes: &str) -> Drawing {
    Drawing::a4().auto(GR1).page(shapes)
}

#[test]
fn a_rect_read_from_an_odg_is_written_back_as_a_rect() {
    assert_written_as_read(&with(RECT), "<draw:rect ");
}

#[test]
fn a_rect_with_no_style_of_its_own_is_written_with_no_style() {
    // No `draw:style-name`, no `draw:layer`, no `draw:text-style-name`: none is made up, and no automatic style is made for it.
    let (_, doc) = read_back(&Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#));
    let content = part(&save(&doc), "content.xml");
    for made_up in ["draw:style-name", "draw:layer", "draw:text-style-name", "<style:style "] {
        assert!(!content.contains(made_up), "{made_up} is made up: {content}");
    }
    assert!(content.contains("<draw:rect "), "{content}");
}

#[test]
fn an_ellipse_read_from_an_odg_is_written_back_as_an_ellipse_with_its_kind_and_angles() {
    let section = r#"<draw:ellipse draw:style-name="gr1" draw:kind="section" draw:start-angle="30" draw:end-angle="120" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    assert_written_as_read(&with(section), "<draw:ellipse ");
    let plain = r#"<draw:ellipse draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    assert_written_as_read(&with(plain), "<draw:ellipse ");
}

#[test]
fn a_circle_given_by_its_centre_is_written_back_as_a_circle_by_its_centre() {
    let circle = r#"<draw:circle draw:style-name="gr1" svg:cx="4cm" svg:cy="4cm" svg:r="1cm"/>"#;
    assert_written_as_read(&with(circle), "<draw:circle ");
    let ellipse = r#"<draw:ellipse draw:style-name="gr1" svg:cx="4cm" svg:cy="4cm" svg:rx="1cm" svg:ry="2cm"/>"#;
    assert_written_as_read(&with(ellipse), "<draw:ellipse ");
}

#[test]
fn a_regular_polygon_is_written_back_as_a_regular_polygon() {
    let polygon = r#"<draw:regular-polygon draw:style-name="gr1" draw:corners="5" draw:concave="false" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="4cm"/>"#;
    assert_written_as_read(&with(polygon), "<draw:regular-polygon ");
}

#[test]
fn a_polygon_read_from_an_odg_is_written_back_as_a_polygon() {
    let polygon = r#"<draw:polygon draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" draw:points="0,0 4000,0 2000,3000"/>"#;
    assert_written_as_read(&with(polygon), "<draw:polygon ");
}

#[test]
fn a_polyline_read_from_an_odg_is_written_back_as_a_polyline() {
    let polyline = r#"<draw:polyline draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" draw:points="0,0 4000,0 2000,3000"/>"#;
    assert_written_as_read(&with(polyline), "<draw:polyline ");
}

#[test]
fn a_path_read_from_an_odg_is_written_back_as_a_path() {
    let path = r#"<draw:path draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" svg:d="M0 0L4000 0L2000 3000Z"/>"#;
    assert_written_as_read(&with(path), "<draw:path ");
}

#[test]
fn a_line_read_from_an_odg_is_written_back_as_a_line() {
    let line = r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#;
    assert_written_as_read(&with(line), "<draw:line ");
}

#[test]
fn a_connector_read_from_an_odg_is_written_back_as_a_connector_with_its_route() {
    let connector = r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="3cm" svg:viewBox="0 0 5000 2000" svg:d="M0 0L5000 2000"/>"#;
    assert_written_as_read(&with(connector), "<draw:connector ");
    let plain = r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="3cm"/>"#;
    assert_written_as_read(&with(plain), "<draw:connector ");
}

#[test]
fn a_custom_shape_read_from_an_odg_is_written_back_as_a_custom_shape() {
    let custom = crate::build::shape("s1", "2cm");
    assert_written_as_read(&Drawing::a4().page(&custom), "<draw:custom-shape ");
}

#[test]
fn a_text_frame_read_from_an_odg_is_written_back_as_a_frame_with_its_text_box() {
    let frame =
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box><text:p>Hello</text:p></draw:text-box></draw:frame>"#;
    assert_written_as_read(&Drawing::a4().page(frame), "<draw:frame ");
}

#[test]
fn a_picture_frame_read_from_an_odg_is_written_back_as_a_frame_with_its_image() {
    let mut drawing = Drawing::a4()
        .page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>"#);
    drawing.files.push(("Pictures/a.png".to_owned(), vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4]));
    assert_written_as_read(&drawing, "<draw:frame ");
}

#[test]
fn a_group_read_from_an_odg_is_written_back_as_a_group_of_its_members() {
    let group = format!("<draw:g>{RECT}{RECT}</draw:g>");
    assert_written_as_read(&with(&group), "<draw:g>");
}

#[test]
fn a_turned_rect_is_written_back_with_its_transform_as_it_was_written() {
    let turned = r#"<draw:rect draw:style-name="gr1" svg:width="4cm" svg:height="2cm" draw:transform="rotate(0.5) translate(2cm 3cm)"/>"#;
    assert_written_as_read(&with(turned), "<draw:rect ");
}

/// A connector with a path of its own, glued to nothing.
const ROUTED: &str = r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="3cm" svg:viewBox="0 0 5000 2000" svg:d="M0 0L2500 0L2500 2000L5000 2000"/>"#;

#[test]
fn a_connector_keeps_the_path_the_file_gave_it_while_its_ends_and_rows_are_as_read_and_is_routed_again_when_an_end_moved() {
    let (_, doc) = read_back(&with(ROUTED));
    let content = part(&save(&doc), "content.xml");
    let tag = tags(&content, "draw:connector").first().copied().unwrap_or_default().to_owned();
    assert!(
        tag.contains("svg:d=\"M0 0L2500 0L2500 2000L5000 2000\"") && tag.contains("svg:viewBox=\"0 0 5000 2000\""),
        "the file's path as it was: {tag}"
    );
    // An end moves one inch to the right: the path of the file is no longer the route.
    let (_, mut moved) = read_back(&with(ROUTED));
    edit_shape(&mut moved, 0, |s| {
        let x = s.sheet.cell("EndX").and_then(|c| c.value.as_ref()?.number()).unwrap();
        set_cell(s, "EndX", Value::from_number(x + 1.0).unwrap());
    });
    let content = part(&save(&moved), "content.xml");
    let tag = tags(&content, "draw:connector").first().copied().unwrap_or_default().to_owned();
    assert!(tag.contains("svg:x2=\"8.54cm\""), "the end is where it is now, in the file's unit: {tag}");
    assert!(!tag.contains("M0 0L2500 0L2500 2000L5000 2000"), "the path is not the file's: {tag}");
    assert!(tag.contains("svg:d="), "a route is written: {tag}");
}

/// A shape made in DiaCraft, added to a drawing read from a file, is a new shape: its style takes a name no shape of the file holds.
#[test]
fn a_new_shape_beside_shapes_of_a_file_takes_a_style_name_no_shape_of_the_file_holds() {
    let drawing = Drawing::a4()
        .auto(GR1)
        .auto(r##"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#aa0000"/></style:style>"##)
        .page(r#"<draw:rect draw:style-name="gr2" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/><draw:rect draw:style-name="gr1" svg:x="7cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#);
    let (original, mut doc) = read_back(&drawing);
    // One new shape of its own, with its own fill, before the shapes of the file.
    let mut new = crate::write_new::rectangle(100, "Rectangle.100", (1.0, 8.0, 2.0, 2.0));
    set_cell(&mut new, "FillForegnd", Value::from_text("#00FF00"));
    crate::write_new::push_shape(&mut doc, new);
    let written = save(&doc);
    let content = part(&written, "content.xml");
    assert_eq!(content.matches("style:name=\"gr1\"").count(), 1, "{content}");
    assert_eq!(content.matches("style:name=\"gr2\"").count(), 1, "{content}");
    let rects: Vec<&str> = tags(&content, "draw:rect");
    assert_eq!(rects.iter().filter(|t| t.contains("draw:style-name=\"gr2\"")).count(), 1, "{content}");
    assert_eq!(rects.iter().filter(|t| t.contains("draw:style-name=\"gr1\"")).count(), 1, "{content}");
    let customs = tags(&content, "draw:custom-shape");
    assert_eq!(customs.len(), 1, "the new shape is a new shape: {content}");
    assert!(customs.iter().all(|t| !t.contains("draw:style-name=\"gr1\"") && !t.contains("draw:style-name=\"gr2\"")), "{customs:?}");
    // The shapes of the file are as they were.
    let (back, _) = read(&written, &Limits::default()).unwrap();
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 3);
    let _ = original;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- names and units

/// Two rectangles on one automatic style `gr7` (so they share it), a line on `gr3`, lengths in millimetres, inches, points and pixels.
pub(crate) fn names_and_units_for_dump() -> Drawing {
    names_and_units()
}

fn names_and_units() -> Drawing {
    let gr7 = r##"<style:style style:name="gr7" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#aa0000" draw:stroke="none"/></style:style>"##;
    let gr3 = r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-color="#00aa00"/></style:style>"##;
    let shapes = concat!(
        r#"<draw:rect draw:style-name="gr7" svg:x="25.4mm" svg:y="1in" svg:width="72pt" svg:height="48px"/>"#,
        r#"<draw:rect draw:style-name="gr7" svg:x="5cm" svg:y="6cm" svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:line draw:style-name="gr3" svg:x1="10mm" svg:y1="10mm" svg:x2="50mm" svg:y2="30mm"/>"#
    );
    Drawing::a4().auto(gr7).auto(gr3).page(shapes)
}

fn edit_shape(doc: &mut Document, at: usize, change: impl FnOnce(&mut Shape)) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    change(Arc::make_mut(page.shapes.get_mut(at).unwrap()));
}

/// Sets a cell's value, the unit it had kept (an edit of a value does not change how it is shown).
fn set_cell(shape: &mut Shape, name: &str, value: Value) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    let unit = sheet.cells.get(name).and_then(|c| c.unit.clone());
    let mut cell = Cell::constant(value);
    if let Some(unit) = unit {
        cell = cell.with_unit(&unit);
    }
    sheet.cells.insert(CellName::new(name), cell);
}

/// The text of the start tags of the elements named `element` in `content`.
fn tags<'a>(content: &'a str, element: &str) -> Vec<&'a str> {
    let open = format!("<{element} ");
    content.match_indices(&open).filter_map(|(at, _)| content.get(at..content[at..].find('>').map(|n| at + n)?)).collect()
}

#[test]
fn held_style_names_and_units_are_written_back_while_their_stamps_hold() {
    let (original, doc) = read_back(&names_and_units());
    let written = save(&doc);
    let content = part(&written, "content.xml");
    // The names are the file's, not `gr1`, `gr2`, `gr3`, and a style two shapes name is one style.
    let rects = tags(&content, "draw:rect");
    assert_eq!(rects.len(), 2, "{content}");
    assert!(rects.iter().all(|t| t.contains("draw:style-name=\"gr7\"")), "{rects:?}");
    assert_eq!(tags(&content, "draw:line").iter().filter(|t| t.contains("draw:style-name=\"gr3\"")).count(), 1, "{content}");
    assert_eq!(content.matches("style:name=\"gr7\"").count(), 1, "one style for the two shapes: {content}");
    assert_eq!(content.matches("style:name=\"gr3\"").count(), 1, "{content}");
    // The lengths are in the units the file wrote them in.
    let first = rects.first().copied().unwrap_or_default();
    for want in ["svg:x=\"25.4mm\"", "svg:y=\"1in\"", "svg:width=\"72pt\"", "svg:height=\"48px\""] {
        assert!(first.contains(want), "{want}: {first}");
    }
    let line = tags(&content, "draw:line").first().copied().unwrap_or_default();
    for want in ["svg:x1=\"10mm\"", "svg:y1=\"10mm\"", "svg:x2=\"50mm\"", "svg:y2=\"30mm\""] {
        assert!(line.contains(want), "{want}: {line}");
    }
    assert_eq!(differences(&original, &written), Vec::<String>::new());
}

#[test]
fn a_shape_whose_style_cells_were_edited_gets_a_style_of_its_own_and_the_name_is_back_when_they_are() {
    let (_, mut doc) = read_back(&names_and_units());
    let was = doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cell("FillForegnd").cloned().expect("the fill colour");
    edit_shape(&mut doc, 1, |s| set_cell(s, "FillForegnd", Value::from_text("#0000FF")));
    let content = part(&save(&doc), "content.xml");
    let rects = tags(&content, "draw:rect");
    assert!(rects.first().is_some_and(|t| t.contains("draw:style-name=\"gr7\"")), "the one that was not edited keeps its name: {rects:?}");
    let second = rects.get(1).copied().unwrap_or_default();
    assert!(!second.contains("draw:style-name=\"gr7\""), "the edited one is on a style of its own: {second}");
    assert!(!second.contains("draw:style-name=\"gr3\""), "and not on the other's: {second}");
    assert_eq!(content.matches("style:name=\"gr7\"").count(), 1, "{content}");
    assert!(content.contains("draw:fill-color=\"#0000ff\""), "the edit is written: {content}");
    // An undo puts the cell back, and the shape names the file's style again.
    edit_shape(&mut doc, 1, |s| {
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("FillForegnd"), was.clone());
    });
    let back = part(&save(&doc), "content.xml");
    assert!(tags(&back, "draw:rect").iter().all(|t| t.contains("draw:style-name=\"gr7\"")), "{back}");
    assert_eq!(back.matches("style:name=\"gr7\"").count(), 1, "{back}");
}

#[test]
fn a_moved_rect_stays_a_rect_with_its_style_name() {
    let (_, mut doc) = read_back(&with(RECT));
    let pin = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    // One inch to the right.
    edit_shape(&mut doc, 0, |s| set_cell(s, "PinX", Value::from_number(pin + 1.0).unwrap()));
    let content = part(&save(&doc), "content.xml");
    let rects = tags(&content, "draw:rect");
    assert_eq!(rects.len(), 1, "{content}");
    let tag = rects.first().copied().unwrap_or_default();
    assert!(tag.contains("draw:style-name=\"gr1\""), "it keeps its style name: {tag}");
    assert!(tag.contains("svg:x=\"4.54cm\""), "the new x, in the file's unit: {tag}");
    for same in ["svg:y=\"3cm\"", "svg:width=\"4cm\"", "svg:height=\"2cm\""] {
        assert!(tag.contains(same), "{same}: {tag}");
    }
    assert!(!content.contains("<draw:custom-shape "), "{content}");
    assert_eq!(content.matches("style:name=\"gr1\"").count(), 1, "{content}");
}

#[test]
fn a_resized_rect_is_a_rect_with_the_new_size_in_the_files_unit() {
    let (_, mut doc) = read_back(&with(RECT));
    edit_shape(&mut doc, 0, |s| {
        let sheet = Arc::make_mut(&mut s.sheet);
        // The pin is the centre: a width of 2 inches with the place of the pin as it is, and the pin's x moves with half the growth.
        let w = sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()).unwrap();
        let pin = sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
        let unit = |c: Cell| c.with_unit("CM");
        sheet.cells.insert(CellName::new("Width"), unit(Cell::constant(Value::from_number(w + 1.0).unwrap())));
        sheet.cells.insert(CellName::new("PinX"), unit(Cell::constant(Value::from_number(pin + 0.5).unwrap())));
        sheet.cells.insert(CellName::new("LocPinX"), unit(Cell::with_formula("Width*0.5", Value::from_number((w + 1.0) / 2.0))));
    });
    let content = part(&save(&doc), "content.xml");
    let tag = tags(&content, "draw:rect").first().copied().unwrap_or_default().to_owned();
    assert!(tag.contains("svg:x=\"2cm\"") && tag.contains("svg:width=\"6.54cm\""), "{tag}");
}

#[test]
fn layer_and_text_style_names_are_written_only_where_the_file_had_them() {
    // A shape that had neither.
    let (_, doc) = read_back(&with(RECT));
    let content = part(&save(&doc), "content.xml");
    assert!(!content.contains("draw:layer") && !content.contains("draw:text-style-name"), "{content}");
    // A shape that had both, on a layer of the drawing and with a paragraph style: the names are the file's, and the style is written once for the shapes that name it.
    let p1 = r#"<style:style style:name="P1" style:family="paragraph"/>"#;
    let rect = r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    let drawing = Drawing::a4().auto(GR1).auto(p1).page(&format!("{rect}{rect}"));
    let (original, doc) = read_back(&drawing);
    let written = save(&doc);
    let content = part(&written, "content.xml");
    assert!(content.contains("draw:layer=\"layout\"") && content.contains("draw:text-style-name=\"P1\""), "{content}");
    assert_eq!(content.matches("style:name=\"P1\"").count(), 1, "one paragraph style for the two shapes: {content}");
    assert_eq!(differences(&original, &written), Vec::<String>::new());
}

/// A paragraph style that a shape names for its text (`draw:text-style-name`) and that a paragraph of another shape names with a style of its own is **one style**: the text writer writes it
/// for the paragraph, and the shape's own name for it is left to that style (with the text writer).
#[test]
fn a_paragraph_style_named_by_a_shape_and_by_a_paragraph_is_written_once() {
    let p1 = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
    let named = r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p text:style-name="P1">Hi</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let only_shape = r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" svg:x="7cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    let (_, doc) = read_back(&Drawing::a4().auto(GR1).auto(p1).page(&format!("{only_shape}{named}")));
    let written = write_with(&doc, None, &unedited()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    assert_eq!(content.matches("style:name=\"P1\"").count(), 1, "one style for the shape's name and the paragraph's: {content}");
    assert_eq!(content.matches("draw:text-style-name=\"P1\"").count(), 2, "both shapes name it: {content}");
    // And the other way round: the shape that only names it comes after the paragraph that has the style.
    let (_, doc) = read_back(&Drawing::a4().auto(GR1).auto(p1).page(&format!("{named}{only_shape}")));
    let content = part(&save(&doc), "content.xml");
    assert_eq!(content.matches("style:name=\"P1\"").count(), 1, "{content}");
}

/// **The attributes of an automatic style that no cell reads are written with the style**: the file's style `gr1` says `draw:textarea-horizontal-align` and
/// `draw:auto-grow-height`, which the model holds as what its kept style says, and the style written under the file's name says them again; the save does not say they were left out.
#[test]
fn the_attributes_a_style_kept_that_no_cell_reads_are_written_with_the_style() {
    let gr1 = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699" draw:textarea-horizontal-align="center" draw:auto-grow-height="false"/></style:style>"##;
    let (original, doc) = read_back(&Drawing::a4().auto(gr1).page(RECT));
    let written = write_with(&doc, None, &unedited()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains("draw:textarea-horizontal-align=\"center\"") && content.contains("draw:auto-grow-height=\"false\""), "{content}");
    assert_eq!(differences(&original, &written.bytes), Vec::<String>::new());
    assert!(
        !written.notes.iter().any(|n| matches!(n, diacraft_odg::SaveNote::DocumentDataUnwritten { styles, .. } if *styles > 0)),
        "{:?}",
        written.notes
    );
    // A kept attribute a cell writes is the cell's: the style says the colour the cells say.
    let (_, mut edited) = read_back(&Drawing::a4().auto(gr1).page(RECT));
    edit_shape(&mut edited, 0, |s| set_cell(s, "FillForegnd", Value::from_text("#00FF00")));
    let content = part(&save(&edited), "content.xml");
    assert!(content.contains("draw:fill-color=\"#00ff00\"") && content.contains("draw:auto-grow-height=\"false\""), "{content}");
    assert_eq!(content.matches("style:name=\"gr1\"").count() + content.matches("style:name=\"gr2\"").count(), 1, "{content}");
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- the 24 vectors

/// The vectors a visit through the model writer can make the same, as the `.odg` reader holds them: the 24 vectors of `model_measure`.
fn vectors() -> Vec<(&'static str, Drawing)> {
    crate::model_measure::cases()
}

/// The vectors whose differences are **not** the shapes' elements, and why (said plainly, with the reason, in the report). None stays: the one that did, a bold span, whose run style the text
/// writer wrote with every attribute its cells give (the font size, the style, the variant) where the file said only the weight, is written as the file had it.
const NOT_YET: [(&str, &str); 0] = [];

#[test]
fn an_odg_visit_through_diacraft_has_the_same_content() {
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    for (name, drawing) in vectors() {
        let (original, doc) = read_back(&drawing);
        let written = write_with(&doc, None, &unedited()).unwrap_or_else(|e| panic!("{name}: refused: {e}")).bytes;
        let diff = differences(&original, &written);
        if !diff.is_empty() {
            found.push((name.to_owned(), diff));
        }
    }
    let unexpected: Vec<&(String, Vec<String>)> = found.iter().filter(|(n, _)| !NOT_YET.iter().any(|(m, _)| m == n)).collect();
    assert!(unexpected.is_empty(), "vectors that differ in content: {unexpected:#?}");
    // The vectors that stay are told by name, so that one that starts to pass is noticed.
    let stay: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    let want: Vec<&str> = NOT_YET.iter().map(|(n, _)| *n).collect();
    assert_eq!(stay, want, "the vectors that stay different");
}

#[test]
fn a_second_model_save_is_byte_identical() {
    for (name, drawing) in vectors() {
        let (_, doc) = read_back(&drawing);
        let first = write_with(&doc, None, &unedited()).unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let (again, _) = read(&first.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{name}: the file does not read: {e}"));
        let second = write_with(&again, None, &unedited()).unwrap_or_else(|e| panic!("{name}: the second save is refused: {e}"));
        if first.bytes != second.bytes {
            let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
            let shown: Vec<String> = diff.shown.iter().take(6).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
            panic!("{name}: the second save is not the first's bytes ({} differences of content): {shown:?}", diff.count);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- window settings

fn windows_part() -> RawElement {
    let window = RawElement {
        name: Arc::from("Window"),
        attrs: RawAttrs::from_pairs([("WindowType", "Drawing"), ("ID", "0")]),
        children: vec![diacraft_doc::RawNode::Element(RawElement {
            name: Arc::from("ShowGrid"),
            attrs: RawAttrs::new(),
            children: vec![diacraft_doc::RawNode::Text(Arc::from("0"))],
        })],
    };
    RawElement { name: Arc::from("Windows"), attrs: RawAttrs::new(), children: vec![diacraft_doc::RawNode::Element(window)] }
}

#[test]
fn a_model_that_holds_window_settings_is_written_with_the_windows_child_and_not_refused() {
    let (_, mut doc) = read_back(&with(RECT));
    let part = Arc::new(windows_part());
    doc.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::clone(&part)), ..VsdxHeld::default() }));
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("a model that holds window settings is refused: {e}"));
    let document_file = part_of(&written.bytes, "DiaCraft/document.xml");
    assert!(
        document_file.contains(":windows>") && document_file.contains("ShowGrid"),
        "the settings ride in DiaCraft's document file: {document_file}"
    );
    // Read again, the model holds the settings it held, and nothing else of a package.
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    let held = back.vsdx.as_ref().expect("the settings come back");
    assert_eq!(held.windows.as_deref(), Some(&*part));
    assert!(held.holds_windows_alone(), "{held:?}");
    // And a save of a model that does not hold them writes no such child.
    let (_, plain) = read_back(&with(RECT));
    let plain = write_with(&plain, None, &WriteOptions::default()).unwrap();
    assert!(!part_of(&plain.bytes, "DiaCraft/document.xml").contains(":windows>"));
}

/// The entry `name` of a package as text, empty when it has none.
fn part_of(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    package.part(name).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- hostile names

/// The written model is a part that reads again (so it is well-formed), holds no attribute the hostile name would have written, and the shape is a new shape whose unwritten attributes
/// the save counts.
fn assert_no_injection(doc: &Document, injected: &[&str], what: &str) {
    let written = write_with(doc, None, &unedited()).unwrap_or_else(|e| panic!("{what}: the save is refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{what}: the part written does not read again: {e}: {content}"));
    for text in injected {
        assert!(!content.contains(text), "{what}: {text} is in the markup: {content}");
    }
    let counted = written.notes.iter().any(|note| matches!(note, diacraft_odg::SaveNote::NewShapeUnwritten { attributes, .. } if *attributes > 0));
    assert!(counted, "{what}: the note says what was not written: {:?}", written.notes);
}

/// A drawing whose turned rectangle holds a frame value (so that its attributes are written back).
fn turned_rect() -> Drawing {
    let turned = r#"<draw:rect draw:style-name="gr1" svg:width="4cm" svg:height="2cm" draw:transform="rotate(0.5) translate(2cm 3cm)"/>"#;
    with(turned)
}

fn first_held(doc: &mut Document, change: impl FnOnce(&mut diacraft_doc::OdfShape)) {
    edit_shape(doc, 0, |s| change(&mut s.ext.make_mut().odf));
}

#[test]
fn a_held_attribute_name_that_holds_markup_is_never_written_into_a_start_tag() {
    // A name with a quote and a space in it: written as it is, it would be an attribute of its own.
    let (_, mut doc) = read_back(&turned_rect());
    first_held(&mut doc, |odf| {
        for h in odf.held.iter_mut().filter(|h| h.group == diacraft_doc::HeldGroup::Frame) {
            h.attrs.push("svg:a=\"1\" b", "x");
        }
    });
    assert_no_injection(&doc, &[" b=", "a=\"1\""], "a name with a quote and a space");
    // A name with `>` in it would end the tag.
    let (_, mut doc) = read_back(&turned_rect());
    first_held(&mut doc, |odf| {
        for h in odf.held.iter_mut().filter(|h| h.group == diacraft_doc::HeldGroup::Frame) {
            h.attrs.push("svg:x><draw:evil", "x");
        }
    });
    assert_no_injection(&doc, &["<draw:evil"], "a name with a `>`");
    // A unit's attribute name is a name too.
    let (_, mut doc) = read_back(&with(RECT));
    first_held(&mut doc, |odf| odf.units.push("svg:x\" evil=\"1", "cm"));
    assert_no_injection(&doc, &["evil="], "a unit's name");
}

#[test]
fn a_held_value_with_a_character_xml_does_not_allow_is_not_written() {
    let (_, mut doc) = read_back(&turned_rect());
    first_held(&mut doc, |odf| {
        for h in odf.held.iter_mut().filter(|h| h.group == diacraft_doc::HeldGroup::Frame) {
            h.attrs.push("svg:y", "1cm\u{1}");
        }
    });
    assert_no_injection(&doc, &["\u{1}"], "a control character in a held value");
}

#[test]
fn an_image_attribute_that_holds_markup_is_left_out_and_counted() {
    let mut drawing = Drawing::a4().page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/a.png" xlink:type="simple"/></draw:frame>"#);
    drawing.files.push(("Pictures/a.png".to_owned(), vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4]));
    let (_, mut doc) = read_back(&drawing);
    edit_shape(&mut doc, 0, |s| {
        if let Some(foreign) = s.foreign.as_mut() {
            Arc::make_mut(foreign).ext.make_mut().odf.raw.push("xlink:x y=\"z\"", "v");
        }
    });
    let written = write_with(&doc, None, &unedited()).unwrap();
    let content = part(&written.bytes, "content.xml");
    read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the part written does not read again: {e}: {content}"));
    assert!(!content.contains("y=\"z\""), "{content}");
    assert!(content.contains("xlink:type=\"simple\""), "the attribute that is a name stays: {content}");
    let counted = written.notes.iter().any(|note| matches!(note, diacraft_odg::SaveNote::NewShapeUnwritten { attributes, .. } if *attributes > 0));
    assert!(counted, "{:?}", written.notes);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- the second save

/// The rows of the first `Geometry` section of the first shape of the first page, as (row type, X, Y) texts.
fn outline_of(doc: &Document) -> Vec<(String, String, String)> {
    let shape = doc.pages.get(0).and_then(|p| p.shapes.get(0)).expect("a shape");
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry").expect("a geometry section");
    let text =
        |row: &diacraft_doc::Row, cell: &str| row.cells.get(cell).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default();
    section.rows.iter().map(|r| (r.row_type.as_deref().unwrap_or_default().to_owned(), text(r, "X"), text(r, "Y"))).collect()
}

/// **A second save keeps the outline the first carried** (the first save of a shape whose outline an edit changed writes a placeholder outline and carries the rows in DiaCraft's
/// data; the second, which read the placeholder as the shape's own geometry, wrote none of it and lost the outline). The first and the second save are the same bytes.
#[test]
fn a_second_save_keeps_an_outline_the_first_save_carried() {
    let polygon = r#"<draw:polygon draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" draw:points="0,0 4000,0 2000,3000"/>"#;
    let (_, mut doc) = read_back(&with(polygon));
    // The outline is edited: the last point moves, so the file's element no longer says it.
    edit_shape(&mut doc, 0, |s| {
        let sheet = Arc::make_mut(&mut s.sheet);
        let at = sheet.sections.iter().position(|g| g.name.as_str() == "Geometry").expect("a geometry section");
        let section = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
        let last = section.rows.len() - 2;
        let row = Arc::make_mut(section.rows.get_mut(last).unwrap());
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_number(0.6).unwrap()));
    });
    let want = outline_of(&doc);
    let first = write_with(&doc, None, &unedited()).unwrap();
    let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
    assert_eq!(outline_of(&again), want, "the first save keeps the outline");
    let second = write_with(&again, None, &unedited()).unwrap();
    let (third, _) = read(&second.bytes, &Limits::default()).unwrap();
    assert_eq!(outline_of(&third), want, "the second save keeps the outline too");
    assert_eq!(first.bytes, second.bytes, "and writes the first save's bytes");
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- LibreOffice

/// A black-box check against LibreOffice, run by hand (ignored): writes the drawings of the visit (the 24 vectors as the model writer writes them after a read, in the form a save writes, and
/// the drawings of this task's tests, edited ones among them) into `$ODG_OUT_DIR/in`. Then `soffice -env:UserInstallation=file://$ODG_OUT_DIR/profile --headless --convert-to pdf --outdir
/// $ODG_OUT_DIR/out $ODG_OUT_DIR/in/<file>` for each: only a missing output file is a failure (LibreOffice offers a repair for a package it finds damaged, and headless it then loads nothing).
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_OUT_DIR set to an empty folder"]
fn lo_asread() {
    let dir = std::path::PathBuf::from(std::env::var("ODG_OUT_DIR").expect("set ODG_OUT_DIR to an empty folder")).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, doc: &Document| {
        let bytes = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}")).bytes;
        std::fs::write(dir.join(name), bytes).unwrap();
    };
    for (n, (name, drawing)) in vectors().into_iter().enumerate() {
        let _ = name;
        let (_, doc) = read_back(&drawing);
        write(&format!("vector{n:02}.odg"), &doc);
    }
    let (_, doc) = read_back(&names_and_units());
    write("units.odg", &doc);
    let (_, mut moved) = read_back(&with(RECT));
    let pin = moved.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
    edit_shape(&mut moved, 0, |s| set_cell(s, "PinX", Value::from_number(pin + 1.0).unwrap()));
    write("moved.odg", &moved);
    let (_, mut edited) = read_back(&names_and_units());
    edit_shape(&mut edited, 1, |s| set_cell(s, "FillForegnd", Value::from_text("#0000FF")));
    write("edited_style.odg", &edited);
    let (_, turned) = read_back(&turned_rect());
    write("turned.odg", &turned);
    let (_, mut windows) = read_back(&with(RECT));
    windows.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::new(windows_part())), ..VsdxHeld::default() }));
    write("windows.odg", &windows);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- counted work

/// The steps a model save counts (`work.cells_compared`) for the drawing of one turned rectangle, after `change` is made to its slot.
fn steps_after(change: impl FnOnce(&mut diacraft_doc::OdfShape), limits: Limits) -> Result<u64, String> {
    let (_, mut doc) = read_back(&turned_rect());
    first_held(&mut doc, change);
    let options = WriteOptions { limits, ..unedited() };
    write_with(&doc, None, &options).map(|w| w.work.cells_compared).map_err(|e| e.to_string())
}

/// **What reading a shape's held values walks is counted by what is walked** (the security scan's round 2): a shape with one held value of 100,000 attributes, or whose element holds 100,000
/// children, costs 100,000 steps and is refused past the steps a save may take, and the plain shape is a small constant.
#[test]
fn a_held_value_of_a_hundred_thousand_attributes_and_an_element_of_a_hundred_thousand_children_are_counted_by_what_they_hold() {
    let plain = steps_after(|_| {}, Limits::default()).expect("the plain shape saves");
    assert!(plain < 2_000, "the yardstick is a small constant: {plain}");
    // One held value with 100,000 attributes: past a cap of 50,000 steps the save is refused in words, and under the default cap it costs what it holds.
    let many_attrs = |odf: &mut diacraft_doc::OdfShape| {
        for h in odf.held.iter_mut().filter(|h| h.group == diacraft_doc::HeldGroup::Frame) {
            for k in 0..100_000 {
                h.attrs.push(&format!("svg:a{k}"), "x");
            }
        }
    };
    let tight = Limits { max_write_steps: 50_000, ..Limits::default() };
    let refused = steps_after(many_attrs, tight.clone()).expect_err("100,000 attributes are refused under 50,000 steps");
    assert!(refused.contains("steps"), "refused in words: {refused}");
    let counted = steps_after(many_attrs, Limits::default());
    // (The save may refuse the held value in words for its content; what is asked is that the work is counted.)
    if let Ok(steps) = counted {
        assert!(steps >= 100_000, "{steps} steps for 100,000 attributes");
    }
    // One element with 100,000 children.
    let many_children = |odf: &mut diacraft_doc::OdfShape| {
        for h in odf.held.iter_mut().filter(|h| h.group == diacraft_doc::HeldGroup::Geometry && h.attrs.is_empty()) {
            let mut kept = diacraft_doc::Passthrough::new();
            let children = (0..100_000).map(|_| diacraft_doc::RawNode::Text(Arc::from("x"))).collect();
            kept.push(0, diacraft_doc::RawNode::Element(RawElement { name: Arc::from("draw:rect"), attrs: RawAttrs::new(), children }));
            h.kept = kept;
        }
    };
    let refused = steps_after(many_children, tight).expect_err("100,000 children are refused under 50,000 steps");
    assert!(refused.contains("steps"), "refused in words: {refused}");
    let counted = steps_after(many_children, Limits::default()).expect("the element's children are no refusal under the default cap");
    assert!(counted >= 100_000 && counted < 100_000 + plain + 2_000, "{counted} steps for 100,000 children, against {plain} for the plain shape");
}

/// What a model save counts for a drawing of `shapes` rectangles that name one style whose kept attribute is `len` bytes (`None`: the style keeps none): the bytes copied and the attributes
/// written.
fn counted_for(shapes: usize, kept: Option<usize>) -> (u64, u64) {
    let attr = kept.map(|len| format!(r#" draw:textarea-horizontal-align="{}""#, "a".repeat(len))).unwrap_or_default();
    let gr1 = format!(
        r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"{attr}/></style:style>"##
    );
    let rects: String = (0..shapes)
        .map(|k| format!(r#"<draw:rect draw:style-name="gr1" svg:x="{}cm" svg:y="3cm" svg:width="1cm" svg:height="1cm"/>"#, k % 50))
        .collect();
    let (_, doc) = read_back(&Drawing::a4().auto(&gr1).page(&rects));
    let work = write_with(&doc, None, &unedited()).unwrap_or_else(|e| panic!("the save is refused: {e}")).work;
    (work.bytes_copied, work.attributes_written)
}

/// **The attributes a style kept are made once for the style, not once for each shape that names it** (the security scan's round 3): 2,000 shapes naming one style whose kept attribute is 1 MB
/// copy a few styles' worth of bytes, counted (and not 2 GB), and write no attribute for the kept one beyond the style's own, against a style that keeps none.
#[test]
fn the_attributes_a_style_kept_are_copied_once_for_the_style_and_not_for_each_shape_that_names_it() {
    let (plain_bytes, plain_attrs) = counted_for(2_000, None);
    let (big_bytes, big_attrs) = counted_for(2_000, Some(1_000_000));
    let more = big_bytes.saturating_sub(plain_bytes);
    // A few copies of the one style (the kept values made once, the style's text, the splice's own), and none for each of the 2,000 shapes.
    assert!((900_000..=6_000_000).contains(&more), "a few styles' worth of bytes, counted: {more} more for a kept attribute of 1 MB");
    let extra_attrs = big_attrs.saturating_sub(plain_attrs);
    assert!(
        extra_attrs <= 8,
        "the kept attribute is written for the style once, and not for each of 2,000 shapes: {extra_attrs} attributes more than the style that keeps none"
    );
}
