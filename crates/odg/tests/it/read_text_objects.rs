//! A text that holds the frame of an object: the model keeps nothing of such a frame when it is in a paragraph (the reader reads the text of
//! the frame and no more) and keeps its element, as a child of the text box, when it is a direct child of it. A shape that holds one is **marked** with the `User` row
//! `ODFTextObject` (as an object with no picture is with `ODFObject`), so that a copy of the shape, which could not carry the object, is refused by the engine and not saved
//! without it.

use diacraft_doc::{CellAddr, RowKey, Shape};
use diacraft_odg::TEXT_OBJECT_ROW;

use crate::drawing::{Drawing, cell_at, only};

/// The text of the `User.ODFTextObject.Value` cell of a shape, when it has one.
fn marked(s: &Shape) -> Option<String> {
    cell_at(&s.sheet, &CellAddr::in_row("User", None, RowKey::name(TEXT_OBJECT_ROW), "Value"))
        .and_then(|c| c.value.as_ref())
        .map(|v| v.text().to_owned())
}

/// A text frame whose text box holds `inside`.
fn text_frame(inside: &str) -> String {
    format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="6cm" svg:height="4cm"><draw:text-box>{inside}</draw:text-box></draw:frame>"#)
}

/// A frame holding `object`, as a text holds one.
fn nested(object: &str) -> String {
    format!(r#"<draw:frame svg:width="2cm" svg:height="1cm">{object}<draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#)
}

/// The name of the row is the one the engine looks for, and the reader says what is in the text.
#[test]
fn odg_shapes_a_text_that_holds_the_frame_of_an_object_marks_its_shape() {
    assert_eq!(TEXT_OBJECT_ROW, "ODFTextObject");
    let object = nested(r#"<draw:object xlink:href="./Object 1"/>"#);
    // Each place the frame can be in: a child of the text box (beside the paragraph or before it), in a paragraph, in a span of a paragraph, in a list item, in a frame
    // that is itself in a paragraph.
    let text_box = format!(r#"<draw:frame svg:width="3cm" svg:height="2cm"><draw:text-box><text:p>b{object}</text:p></draw:text-box></draw:frame>"#);
    let places: [(&str, String); 6] = [
        ("a child of the text box after a paragraph", format!("<text:p>hello</text:p>{object}")),
        ("a child of the text box with no paragraph", object.clone()),
        ("a paragraph", format!("<text:p>hello{object}</text:p>")),
        ("a paragraph with nothing else in it", format!("<text:p>{object}</text:p>")),
        ("a span", format!(r#"<text:p>he<text:span>l{object}lo</text:span></text:p>"#)),
        ("a frame in a paragraph", format!("<text:p>a{text_box}</text:p>")),
    ];
    for (what, inside) in places {
        let (doc, report) = Drawing::a4().page(&text_frame(&inside)).read();
        let shape = only(&doc);
        assert_eq!(marked(shape).as_deref(), Some("Object in text"), "{what}");
        assert_eq!(report.warnings, Vec::<String>::new(), "{what}: no warning");
    }
}

/// Each element that makes a frame an object makes the text that holds it a marked one.
#[test]
fn odg_shapes_each_kind_of_object_in_the_frame_of_a_text_marks_the_shape() {
    for object in [
        r#"<draw:object xlink:href="./Object 1"/>"#,
        r#"<draw:object-ole xlink:href="./Object 1"/>"#,
        r#"<draw:applet draw:code="a.class"/>"#,
        r#"<draw:plugin xlink:href="m.mp4"/>"#,
        r#"<draw:floating-frame xlink:href="u"/>"#,
        r#"<table:table table:name="T"/>"#,
    ] {
        let (doc, _) = Drawing::a4().page(&text_frame(&format!("<text:p>hello{}</text:p>", nested(object)))).read();
        assert!(marked(only(&doc)).is_some(), "{object}");
    }
}

/// What holds no object is not marked: a text with no frame, one that holds the frame of a picture or a frame with nothing in it, an object that is not in a frame of its
/// own (the frame that is the object is not a text), and the frame of an object that is the shape itself (an object frame is no text).
#[test]
fn odg_shapes_a_text_that_holds_no_object_is_not_marked() {
    let picture = r#"<draw:frame svg:width="2cm" svg:height="1cm"><draw:image xlink:href="Pictures/p.png"/></draw:frame>"#;
    let empty = r#"<draw:frame svg:width="2cm" svg:height="1cm"/>"#;
    let text_box = r#"<draw:frame svg:width="2cm" svg:height="1cm"><draw:text-box><text:p>x</text:p></draw:text-box></draw:frame>"#;
    for (what, inside) in [
        ("no frame", "<text:p>hello</text:p>".to_owned()),
        ("a picture frame in a paragraph", format!("<text:p>hello{picture}</text:p>")),
        ("a picture frame beside a paragraph", format!("<text:p>hello</text:p>{picture}")),
        ("an empty frame", format!("<text:p>hello{empty}</text:p>")),
        ("a frame with a text box", format!("<text:p>hello{text_box}</text:p>")),
        ("a table that is not in a frame", r#"<text:p>hello<table:table table:name="T"/></text:p>"#.to_owned()),
        ("an object element that is not in a frame", r#"<text:p>hello<draw:object xlink:href="./Object 1"/></text:p>"#.to_owned()),
    ] {
        let (doc, _) = Drawing::a4().page(&text_frame(&inside)).read();
        assert_eq!(marked(only(&doc)), None, "{what}");
    }
    // An object frame is a picture frame (or a box) of its own, and is marked by `ODFObject` when it has no picture and by nothing when it has one: not by this row.
    let (doc, _) = Drawing::a4().page(&nested(r#"<draw:object xlink:href="./Object 1"/>"#)).read();
    assert_eq!(marked(only(&doc)), None);
    // The text of a shape that is no frame: a custom shape's text.
    let custom = r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"><text:p>hello</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let (doc, _) = Drawing::a4().page(custom).read();
    assert_eq!(marked(only(&doc)), None);
}

/// The text is read as it was, with the marker: the characters of a paragraph that holds the frame of an object are the same as of one that holds a plain frame (the reader
/// reads the text inside the frame, as it did), and the shape has one `User` section, with the one row.
#[test]
fn odg_shapes_the_marker_changes_neither_the_text_nor_the_sections_beside_its_own_row() {
    let with_object = nested(r#"<draw:object xlink:href="./Object 1"/>"#);
    let without = nested(r#"<draw:image xlink:href="Pictures/p.png"/>"#);
    let (marked_doc, _) = Drawing::a4().page(&text_frame(&format!("<text:p>hello{with_object}</text:p>"))).read();
    let (plain_doc, _) = Drawing::a4().page(&text_frame(&format!("<text:p>hello{without}</text:p>"))).read();
    let (a, b) = (only(&marked_doc), only(&plain_doc));
    // The same characters: the frame each paragraph holds is held whole where it stands, and the one holds an object, the other a picture.
    let characters = |shape: &diacraft_doc::Shape| {
        shape.text.as_ref().map(|t| t.parts.iter().filter(|p| !matches!(p, diacraft_doc::TextPart::OdfRaw(_))).cloned().collect::<Vec<_>>())
    };
    assert_eq!(characters(a), characters(b), "the same text");
    let frames = |shape: &diacraft_doc::Shape| {
        shape.text.as_ref().map_or(0, |t| t.parts.iter().filter(|p| matches!(p, diacraft_doc::TextPart::OdfRaw(_))).count())
    };
    assert_eq!((frames(a), frames(b)), (1, 1), "each holds its frame whole");
    let user: Vec<_> = a.sheet.sections.iter().filter(|s| s.name.as_str() == "User").collect();
    assert_eq!(user.len(), 1, "one `User` section");
    assert_eq!(user[0].rows.len(), 1, "with the one row");
    assert_eq!(a.sheet.sections.len(), b.sheet.sections.len() + 1, "and it is the one section more");
    assert!(b.sheet.sections.iter().all(|s| s.name.as_str() != "User"));
}
