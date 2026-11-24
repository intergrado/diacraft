//! Item 3: a new shape whose three links name one style sheet ("a VSDX shape whose three links name one style sheet is written
//! with an automatic style whose `style:parent-style-name` is that style's name and which sets only the shape's local cells"). The shape a person dropped after a theme was
//! written on the root with the theme's look as constants, which no variant moves; now it names the `Theme` style as its parent and reads back with the links, so that it follows
//! the theme after the file is opened again. Each case saves a model, reads the bytes back with the reader and compares, and saves again, which must be no edit.

use std::sync::Arc;

use diacraft_doc::{Links, Shape};
use diacraft_odg::{Limits, WriteError, read};

use crate::drawing::Drawing;
use crate::write_core::{Open, custom, open, open_bytes, part, real};
use crate::write_new::{a_line, connector, push_shape, rectangle};
use crate::write_styles_new::style_sheet;

fn all(id: u32) -> Links {
    Links { line: Some(id), fill: Some(id), text: Some(id) }
}

fn on(mut shape: Shape, links: Links) -> Shape {
    Arc::make_mut(&mut shape.sheet).links = links;
    shape
}

/// A drawing of one shape and a style sheet added last, `Theme`, which the new shapes may name: its `ID`.
fn with_theme() -> (Open, u32) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    (o, n)
}

fn links_of(shape: &Shape) -> (Option<u32>, Option<u32>, Option<u32>) {
    (shape.sheet.links.line, shape.sheet.links.fill, shape.sheet.links.text)
}

/// The writer's words of a refusal, or `None` for a save that was made.
fn refusal(o: &Open) -> Option<String> {
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => Some(words.to_owned()),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(_) => None,
    }
}

/// A new shape, a new line and a new connector whose links name `Theme`: each has its own automatic style that names `Theme` as its parent, the other shape's style has none, the
/// file reads back with the links the model has (and no warning), and a second save, and a save of the model the file reads as, are no edit.
#[test]
fn a_new_shape_a_line_and_a_connector_on_a_style_sheet_name_it_as_the_parent_of_their_own_style() {
    let (mut o, n) = with_theme();
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, on(rectangle(first, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)), all(n)));
    push_shape(&mut o.doc, on(a_line(first + 1), all(n)));
    push_shape(&mut o.doc, on(connector(first + 2, (2.0, 20.0, 9.0, 21.0)), all(n)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("style:parent-style-name=\"Theme\"").count(), 3, "one automatic style for each new shape names it: {content}");
    for name in ["gr1", "gr2", "gr3"] {
        assert!(
            content.contains(&format!("<style:style style:name=\"{name}\" style:family=\"graphic\" style:parent-style-name=\"Theme\">")),
            "{name}: {content}"
        );
    }
    assert!(
        part(&saved.bytes, "styles.xml").contains(r#"<style:style style:name="Theme" style:family="graphic"/>"#),
        "and the style is in `styles.xml`"
    );
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    let shapes: Vec<&Arc<Shape>> = back.pages.get(0).unwrap().shapes.iter().collect();
    assert_eq!(shapes.len(), 4);
    assert_eq!(links_of(shapes[0]), (Some(0), Some(0), Some(0)), "the shape of the file is as it was");
    for new in &shapes[1..] {
        assert_eq!(links_of(new), (Some(n), Some(n), Some(n)), "a new shape follows the style after the file is read again");
    }
    // What the shape holds of its own is written as it was: the fill the rectangle has is the shape's own.
    let fill = |s: &Shape| s.sheet.cell("FillForegnd").map(|c| c.value.clone());
    assert_eq!(fill(shapes[1]), fill(o.doc.pages.get(0).unwrap().shapes.iter().nth(1).unwrap()));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
    let reread = open_bytes(saved.bytes.clone());
    let third = real(&reread.doc, &reread.bytes, &reread.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes, "nor is a save of the model the file reads as");
}

/// A new shape on a style sheet the file already holds names it by the name the file has it under (as written), and a shape on the root names none.
#[test]
fn a_new_shape_on_a_style_the_file_holds_names_it_by_the_name_the_file_gives_it() {
    let drawing = Drawing::a4()
        .common(r#"<style:style style:name="Box_20_Plain" style:display-name="Box Plain" style:family="graphic"/>"#)
        .page(&custom("s1", "1cm"));
    let mut o = open(&drawing);
    let held = o.doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Box_20_Plain")).unwrap().id;
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, on(rectangle(first, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)), all(held)));
    push_shape(&mut o.doc, rectangle(first + 1, "Rectangle.2", (8.0, 3.0, 4.0, 2.0)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("style:parent-style-name=\"Box_20_Plain\"").count(), 1, "{content}");
    assert_eq!(content.matches("style:parent-style-name").count(), 1, "the shape on the root has no parent: {content}");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let shapes: Vec<&Arc<Shape>> = back.pages.get(0).unwrap().shapes.iter().collect();
    assert_eq!((links_of(shapes[1]), links_of(shapes[2])), ((Some(held), Some(held), Some(held)), (Some(0), Some(0), Some(0))));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// Makes a new shape of one kind on the given links.
type MakeShape = Box<dyn Fn(Links) -> Shape>;

/// What a new shape cannot say stays refused in words, each by itself and for each of the three kinds: links that name different style sheets (the file says one parent, so
/// two names are not one), and a style sheet the document does not hold. A link to the root in a family and a style sheet in another is one of the first.
#[test]
fn a_new_shape_whose_links_are_not_one_style_sheet_the_file_can_name_is_refused_in_words() {
    let (o, n) = with_theme();
    let first = o.index.next_id(0).unwrap();
    let kinds: [(&str, MakeShape); 3] = [
        ("a shape", Box::new(move |l| on(rectangle(first, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)), l))),
        ("a line", Box::new(move |l| on(a_line(first), l))),
        ("a connector", Box::new(move |l| on(connector(first, (2.0, 20.0, 9.0, 21.0)), l))),
    ];
    for (what, make) in &kinds {
        let try_links = |links: Links| {
            let mut c = Open { doc: o.doc.clone(), bytes: o.bytes.clone(), index: o.index.clone() };
            push_shape(&mut c.doc, make(links));
            refusal(&c)
        };
        for (why, links) in [
            ("the line family alone", Links { line: Some(n), fill: Some(0), text: Some(0) }),
            ("the fill family alone", Links { line: None, fill: Some(n), text: None }),
            ("the text family alone", Links { line: Some(0), fill: Some(0), text: Some(n) }),
            ("two different sheets", Links { line: Some(n), fill: Some(n), text: Some(n + 1) }),
            ("the fill alone is the root", Links { line: Some(n), fill: Some(0), text: Some(n) }),
            ("the text alone is the root", Links { line: Some(n), fill: Some(n), text: Some(0) }),
        ] {
            let words = try_links(links).unwrap_or_else(|| panic!("{what}, {why}: saved"));
            assert!(words.contains("a new shape whose links name different style sheets"), "{what}, {why}: {words}");
        }
        let words = try_links(all(n + 9)).unwrap_or_else(|| panic!("{what}, a style the document does not hold: saved"));
        assert!(words.contains("a style sheet whose name the file cannot say"), "{what}, a style the document does not hold: {words}");
    }
}

/// A package of `drawing` whose `office:styles` is an empty element (`<office:styles/>`: a hand-made file's, and the engine's tests') or holds one style (`<office:styles>` and
/// `</office:styles>`, the kind a program that writes the drawing's own styles makes).
fn drawing_whose_styles_is(empty: bool) -> Open {
    let drawing = if empty {
        Drawing::a4().page(&custom("s1", "1cm"))
    } else {
        Drawing::a4().common(r#"<style:style style:name="Kept" style:family="graphic"/>"#).page(&custom("s1", "1cm"))
    };
    let mut files = drawing.files();
    for (name, bytes) in &mut files {
        if name == "styles.xml" && empty {
            *bytes = drawing.styles_xml().replace("<office:styles></office:styles>", "<office:styles/>").into_bytes();
        }
    }
    let o = open_bytes(crate::build::package(crate::build::DRAWING, files));
    assert_eq!(part(&o.bytes, "styles.xml").contains("<office:styles/>"), empty);
    o
}

/// What a save adds to `office:styles` (a line end's marker, a style sheet, both) is put in after what is there, in the order the save adds them (the markers, then the style
/// sheets), **as one edit**, whether the element is empty (written whole, with what is added in it) or not; and what was there stays. A drawing the save adds nothing to is
/// as it was (not an element written whole for nothing). The line is a new connector with an end, which gives the marker.
#[test]
fn a_marker_and_a_style_sheet_added_to_office_styles_are_one_edit_in_the_order_the_save_adds_them_whether_it_is_empty_or_not() {
    for empty in [true, false] {
        for (marker, sheet) in [(true, true), (true, false), (false, true), (false, false)] {
            let mut o = drawing_whose_styles_is(empty);
            let n = u32::try_from(o.doc.styles.len()).unwrap();
            if sheet {
                o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
            }
            if marker {
                let first = o.index.next_id(0).unwrap();
                // The connector is on the root: the additions to `office:styles` are what this case is about, with no help of a new shape on a style sheet.
                let mut line = connector(first, (2.0, 20.0, 9.0, 21.0));
                let cells = Arc::make_mut(&mut line.sheet);
                let cell = |v: &str| diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(v));
                cells.cells.insert(diacraft_doc::CellName::new("EndArrow"), cell("4"));
                cells.cells.insert(diacraft_doc::CellName::new("EndArrowSize"), cell("2"));
                cells
                    .cells
                    .insert(diacraft_doc::CellName::new("LineWeight"), diacraft_doc::Cell::read(None, Some("0.0104167"), Some("IN"), None, &mut 8));
                push_shape(&mut o.doc, line);
            }
            let why = format!("empty {empty}, marker {marker}, style sheet {sheet}");
            let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{why}: not saved: {e}"));
            let styles = part(&saved.bytes, "styles.xml");
            if !marker && !sheet {
                assert!(!saved.edited && saved.bytes == o.bytes, "{why}: a save that adds nothing is the file as it was");
                continue;
            }
            assert!(!styles.contains("<office:styles/>"), "{why}: {styles}");
            let at = |needle: &str| styles.find(needle);
            assert_eq!(at("<draw:marker ").is_some(), marker, "{why}: {styles}");
            assert_eq!(at("<style:style style:name=\"Theme\"").is_some(), sheet, "{why}: {styles}");
            if let (Some(m), Some(t)) = (at("<draw:marker "), at("<style:style style:name=\"Theme\"")) {
                assert!(m < t, "{why}: the marker, then the style sheet: {styles}");
            }
            if !empty {
                assert!(at("<style:style style:name=\"Kept\"").unwrap() < at("</office:styles>").unwrap(), "{why}: what was there stays: {styles}");
                let added = at("<draw:marker ").or_else(|| at("<style:style style:name=\"Theme\""));
                if let Some(added) = added {
                    assert!(at("<style:style style:name=\"Kept\"").unwrap() < added, "{why}: after what was there: {styles}");
                }
            }
            let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
            assert_eq!(report.warnings, Vec::<String>::new(), "{why}");
            assert_eq!(back.styles.len(), o.doc.styles.len(), "{why}: the style sheets are read back");
            let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
            assert!(!again.edited && again.bytes == saved.bytes, "{why}: a second save is no edit");
        }
    }
}
