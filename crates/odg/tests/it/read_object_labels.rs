//! The label of an object with no picture, and the word for a shape of a kind DiaCraft does not know:
//! the reader says what an object is in a `User` row for the renderer's placeholder, and says once for the document how many
//! shapes it drew as boxes for want of a kind it knows.

use diacraft_doc::{CellAddr, RowKey, Shape, ShapeType};

use crate::drawing::{Drawing, cell_at, only, rows, shape};

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The text of the `User.ODFObject.Value` cell of a shape, when it has one.
fn label(s: &Shape) -> Option<String> {
    cell_at(&s.sheet, &CellAddr::in_row("User", None, RowKey::name("ODFObject"), "Value")).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// An object with no picture to show is a `Foreign` shape on its frame that says what it is, in the words, in the cell
/// `User.ODFObject.Value`, which the renderer draws as a labelled box; it has no foreign data and no outline.
#[test]
fn odg_shapes_an_object_with_no_picture_says_what_it_is_for_the_placeholder() {
    let frame = |inside: &str| format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm">{inside}</draw:frame>"#);
    let cases: [(&str, String, &str); 12] = [
        ("an object of ODF form", frame(r#"<draw:object xlink:href="./Object 1"/>"#), "Embedded object"),
        ("an object with no ODF form", frame(r#"<draw:object-ole xlink:href="./Object 2"/>"#), "Embedded object"),
        ("an applet", frame(r#"<draw:applet draw:code="a.class"/>"#), "Embedded object"),
        ("a plug-in", frame(r#"<draw:plugin xlink:href="m.mp4"/>"#), "Embedded object"),
        ("a floating frame", frame(r#"<draw:floating-frame xlink:href="u"/>"#), "Embedded object"),
        ("a table", frame(r#"<table:table table:name="T"/>"#), "Table"),
        ("a table before an object", frame(r#"<table:table table:name="T"/><draw:object xlink:href="./O"/>"#), "Table"),
        ("an object before a table", frame(r#"<draw:object xlink:href="./O"/><table:table table:name="T"/>"#), "Embedded object"),
        ("a dimension line", r#"<draw:measure svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="1cm"/>"#.to_owned(), "Dimension line"),
        ("a callout", r#"<draw:caption svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/>"#.to_owned(), "Callout"),
        (
            "a form control",
            r#"<draw:control svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:control="c1"/>"#.to_owned(),
            "Form control",
        ),
        ("a 3-D scene", r#"<dr3d:scene svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/>"#.to_owned(), "3-D scene"),
    ];
    for (what, xml, want) in cases {
        let (doc, report) = Drawing::a4().page(&xml).read();
        let s = only(&doc);
        assert_eq!((s.kind.clone(), label(s).as_deref()), (ShapeType::Foreign, Some(want)), "{what}");
        assert!(s.foreign.is_none() && rows(s, 0) == 0, "{what}: no picture and no outline");
        assert_eq!(report.object_placeholders, 1, "{what}");
    }
    // A custom shape with extrusion or fontwork: the words for them (an extruded shape is the nearest to a 3-D scene).
    for (what, attr, want) in [("extrusion", "draw:extrusion", "3-D scene"), ("fontwork", "draw:text-path", "Fontwork")] {
        let xml = format!(r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry {attr}="true"/></draw:custom-shape>"#);
        let (doc, report) = Drawing::a4().page(&xml).read();
        assert_eq!((label(only(&doc)).as_deref(), report.object_placeholders), (Some(want), 1), "{what}");
    }
}

/// An object that has a picture to show is the picture and has no label; one whose image is missing from the package has none to show, and is a
/// placeholder; a frame that is a text box, or a shape that is no frame, is no object and says nothing.
#[test]
fn odg_shapes_a_label_is_given_only_to_an_object_with_no_picture() {
    let mut d = Drawing::a4().page(
        r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="Pictures/r.png"/></draw:frame>"#,
    );
    d.files.push(("Pictures/r.png".to_owned(), PNG.to_vec()));
    let (doc, report) = d.read();
    assert_eq!((label(only(&doc)), report.object_placeholders, report.kept("frame_object")), (None, 0, 1), "the picture is the object's face");
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:object xlink:href="./O"/><draw:image xlink:href="Pictures/gone.png"/></draw:frame>"#)
        .read();
    assert_eq!((label(only(&doc)).as_deref(), report.object_placeholders), (Some("Embedded object"), 1), "the image is not in the package");
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:text-box><text:p>hi</text:p></draw:text-box></draw:frame><draw:rect svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    assert!(label(shape(&doc, 0, 0)).is_none() && label(shape(&doc, 0, 1)).is_none());
    assert_eq!(report.object_placeholders, 0);
}

/// The marker is charged like any row: a page of a thousand tables costs the section, the row and the cell for each, the same for each.
#[test]
fn odg_shapes_the_markers_of_many_objects_are_counted_and_charged() {
    let table = r#"<draw:frame svg:width="1cm" svg:height="1cm"><table:table table:name="T"/></draw:frame>"#;
    let one = Drawing::a4().page(table).read().1;
    let many = Drawing::a4().page(&table.repeat(1_000)).read().1;
    assert_eq!(many.object_placeholders, 1_000);
    let (items_one, items_many) = (one.work.items, many.work.items);
    let per_object = (items_many - items_one) / 999;
    // The shape and its frame cells, and the marker: a section, a row and one cell (read/mod.rs). Measured: 84 items for each table (83 until the model's
    // rows gained a slot for their extension data), which a charge of the marker by another rule (one that multiplied the section by the row, or
    // left it out) would change.
    // And the slot of the shape's `odf` data, for the attributes its element keeps: an allocation of its own, charged.
    let slot = (16 + std::mem::size_of::<diacraft_doc::ExtData<diacraft_doc::OdfShape>>()).div_ceil(64) as u64;
    // And the value that says the element the frame was read from: a record of its own.
    let element = (std::mem::size_of::<diacraft_doc::Held>() as u64).div_ceil(64);
    assert_eq!(per_object, 84 + slot + element, "items for each table");
    assert_eq!(items_many - items_one, per_object * 999, "linear: every table costs the same");
}

/// A shape of a kind DiaCraft does not know is drawn as its frame's box, and the read says so **once for the document**, in words and with the
/// count: a person is told the box stands in for something. A type it draws (an `ooxml-` preset, a shape with a path) is not one of these.
#[test]
fn odg_shapes_shapes_of_an_unknown_kind_are_said_once_with_their_number() {
    let unknown = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#;
    let known = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="ooxml-rect"/></draw:custom-shape>"#;
    let with_path = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="mso-spt100" draw:enhanced-path="M 0 0 L 21600 21600"/></draw:custom-shape>"#;
    let none = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"/>"#;
    let (_, nothing) = Drawing::a4().page(&format!("{known}{with_path}")).read();
    assert_eq!((nothing.warned("unknown_shape_type"), nothing.kept("shape_type"), nothing.boxed_shapes), (0, 0, 0));
    // A `rectangle` with no path is its frame's box, which is exactly what it is: counted as R31 says, and nothing to tell a person.
    let rectangle = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>"#;
    let (_, exact) = Drawing::a4().page(rectangle).read();
    assert_eq!((exact.warned("unknown_shape_type"), exact.kept("shape_type"), exact.boxed_shapes), (0, 1, 0));
    let (_, one) = Drawing::a4().page(unknown).read();
    assert_eq!((one.warned("unknown_shape_type"), one.kept("shape_type"), one.boxed_shapes), (1, 1, 1));
    let words: Vec<&String> = one.warnings.iter().filter(|w| w.contains("does not know")).collect();
    assert_eq!(words.len(), 1);
    assert!(words[0].starts_with("1 custom shape is of a kind DiaCraft does not know and is drawn as a box"), "{}", words[0]);
    let (_, three) = Drawing::a4().page(&format!("{unknown}{known}{unknown}{none}")).read();
    assert_eq!((three.warned("unknown_shape_type"), three.kept("shape_type"), three.boxed_shapes), (1, 3, 3), "one warning for the document");
    let words: Vec<&String> = three.warnings.iter().filter(|w| w.contains("does not know")).collect();
    assert!(words.len() == 1 && words[0].starts_with("3 custom shapes are of a kind DiaCraft does not know and are drawn as boxes"), "{words:?}");
    // Two pages: the count is the document's, and the warning is one.
    let (_, pages) = Drawing::a4().page(unknown).page(unknown).read();
    assert_eq!((pages.warned("unknown_shape_type"), pages.kept("shape_type"), pages.boxed_shapes), (1, 2, 2));
}

/// **What was kept is counted once** (`skipped_count` of `file.open` is the sum of [`ReadReport::kept`], which the census compares): the label of an object and the
/// word for a box are counted in fields of their own (`object_placeholders`, `boxed_shapes`) and add nothing to `kept`, so each element is one entry as before.
#[test]
fn odg_shapes_the_placeholders_and_boxes_add_nothing_to_what_was_kept() {
    let table = r#"<draw:frame svg:width="1cm" svg:height="1cm"><table:table table:name="T"/></draw:frame>"#;
    let measure = r#"<draw:measure svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="1cm"/>"#;
    let scene = r#"<dr3d:scene svg:width="1cm" svg:height="1cm"/>"#;
    let extruded = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:extrusion="true"/></draw:custom-shape>"#;
    let unknown = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#;
    let no_geometry = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"/>"#;
    let rectangle = r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>"#;
    let kinds = |report: &diacraft_odg::ReadReport| report.kept.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ");
    // Each of these is kept under its own kinds, exactly as before this task, and under none of the two names of the new counts.
    let cases = [
        ("a table", table, "attribute=0 frame_object=1 shape_child=1"),
        ("a dimension line", measure, "attribute=0 measure=1"),
        ("a 3-D scene", scene, "attribute=0 scene_3d=1"),
        ("an extruded shape", extruded, "attribute=0 enhanced_geometry=1 extrusion=1"),
        ("an unknown type", unknown, "attribute=0 enhanced_geometry=1 shape_type=1"),
        ("no geometry", no_geometry, "attribute=0 shape_type=1"),
        ("a rectangle", rectangle, "attribute=0 enhanced_geometry=1 shape_type=1"),
    ];
    for (what, xml, expected) in cases {
        let (_, report) = Drawing::a4().page(xml).read();
        assert_eq!(kinds(&report), expected, "{what}");
    }
    // Together: the sum of the parts (11 entries), none counted twice.
    let (_, all) = Drawing::a4().page(&[table, measure, scene, extruded, unknown, no_geometry, rectangle].concat()).read();
    assert_eq!(all.kept.values().sum::<u64>(), 11, "{}", kinds(&all));
    assert_eq!((all.object_placeholders, all.boxed_shapes), (4, 2), "the objects are four and the stand-in boxes two: the rectangle is no stand-in");
}

/// The file the screenshots of the placeholders are taken from (run by hand: `DIACRAFT_14C_OUT=/absolute/folder cargo test -- --ignored`): one A4 page with
/// every kind of object, an object with a replacement picture beside them, a shape of an unknown kind and an ordinary rectangle.
#[test]
#[ignore = "writes a file for a screenshot"]
fn write_the_drawing_the_placeholders_are_photographed_from() {
    let Some(dir) = std::env::var_os("DIACRAFT_14C_OUT") else { return };
    let frame = |x: u32, y: u32, w: u32, h: u32, inside: &str| {
        format!(r#"<draw:frame svg:x="{x}cm" svg:y="{y}cm" svg:width="{w}cm" svg:height="{h}cm">{inside}</draw:frame>"#)
    };
    let mut shapes = String::new();
    shapes.push_str(&frame(1, 1, 6, 4, r#"<draw:object xlink:href="./Object 1"/>"#));
    shapes.push_str(&frame(8, 1, 6, 4, r#"<table:table table:name="T"/>"#));
    shapes.push_str(&frame(15, 1, 4, 2, r#"<draw:object-ole xlink:href="./Object 2"/>"#));
    shapes.push_str(r#"<dr3d:scene svg:x="1cm" svg:y="6cm" svg:width="5cm" svg:height="4cm"/>"#);
    shapes.push_str(
        r#"<draw:caption svg:x="7cm" svg:y="6cm" svg:width="5cm" svg:height="3cm" draw:caption-point-x="6cm" draw:caption-point-y="10cm"/>"#,
    );
    shapes.push_str(r#"<draw:control svg:x="13cm" svg:y="6cm" svg:width="5cm" svg:height="2cm" draw:control="c1"/>"#);
    shapes.push_str(r#"<draw:measure svg:x1="1cm" svg:y1="12cm" svg:x2="9cm" svg:y2="12cm"/>"#);
    shapes.push_str(r#"<draw:custom-shape svg:x="1cm" svg:y="13cm" svg:width="5cm" svg:height="3cm"><draw:enhanced-geometry draw:extrusion="true"/></draw:custom-shape>"#);
    shapes.push_str(r#"<draw:custom-shape svg:x="7cm" svg:y="13cm" svg:width="6cm" svg:height="3cm"><draw:enhanced-geometry draw:text-path="true"/></draw:custom-shape>"#);
    shapes.push_str(&frame(14, 13, 0, 0, r#"<draw:object xlink:href="./Nothing"/>"#));
    shapes.push_str(r#"<draw:custom-shape svg:x="1cm" svg:y="18cm" svg:width="5cm" svg:height="3cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#);
    shapes.push_str(r#"<draw:rect svg:x="7cm" svg:y="18cm" svg:width="5cm" svg:height="3cm"/>"#);
    shapes.push_str(&frame(1, 23, 1, 1, r#"<draw:object xlink:href="./Small"/>"#));
    let drawing = Drawing::a4().page(&shapes);
    std::fs::write(std::path::Path::new(&dir).join("placeholders.odg"), drawing.bytes()).unwrap();
}
