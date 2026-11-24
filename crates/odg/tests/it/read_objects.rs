//! The objects DiaCraft cannot draw, read as `Foreign` shapes
//! on their frames with the replacement image when there is one, and counted by kind; and the three tests the plan names for this task.

use diacraft_doc::ShapeType;

use crate::drawing::{Drawing, cell, close, cm, formula, num, odf, only, page, row_cell, rows, shape};

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[test]
fn ole_object_shows_replacement_image() {
    // A frame (2 cm, 2 cm, 5 cm by 3 cm) holding `draw:object` and `draw:image`: a shape on that frame drawn from the image.
    let mut d = Drawing::a4().page(
        r#"<draw:frame svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="Pictures/r.png"/></draw:frame>"#,
    );
    d.files.push(("Pictures/r.png".to_owned(), PNG.to_vec()));
    let (doc, report) = d.read();
    let s = only(&doc);
    assert_eq!(s.kind, ShapeType::Foreign);
    close(num(cell(s, "Width")), cm(5.0));
    close(num(cell(s, "Height")), cm(3.0));
    close(num(cell(s, "PinX")), cm(4.5));
    close(num(cell(s, "PinY")), cm(29.7 - 3.5));
    let picture = s.foreign.as_ref().and_then(|f| f.picture.as_ref()).expect("the replacement image");
    assert_eq!(picture.bytes.as_ref(), &PNG[..]);
    assert_eq!(picture.part.as_ref(), "Pictures/r.png");
    assert_eq!(report.kept("frame_object"), 1);
    // The object stays in the shape, as the file has it, and the image it shows does not.
    assert_eq!(odf(s).kept.len(), 1);
    // No geometry: the picture is what draws.
    assert_eq!(rows(s, 0), 0);
}

#[test]
fn type_only_named_shape_draws_as_box_and_is_kept() {
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#)
        .read();
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5, "The rectangle: the frame's box");
    assert_eq!(report.kept("shape_type"), 1, "counted");
    assert_eq!(odf(s).kept.len(), 1, "the element is kept as written");
    // A shape with no `draw:enhanced-geometry` at all is a box too.
    let (doc, report) = Drawing::a4().page(r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"/>"#).read();
    assert_eq!(rows(only(&doc), 0), 5);
    assert_eq!(report.kept("shape_type"), 1);
}

#[test]
fn connector_glued_to_glue_point_reads_the_glue() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:custom-shape xml:id="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="4cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-points="0 5400 21600 16200" draw:enhanced-path="M 0 0 L 21600 21600"/></draw:custom-shape>"#,
            r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:end-shape="a" draw:end-glue-point="5"/>"#
        ))
        .read();
    let c = shape(&doc, 0, 1);
    // The end holds the formula `connector.add` writes for point glue, with the place the file gives as its value (y flipped).
    assert_eq!(formula(cell(c, "EndX")), Some("PAR(PNT(Sheet.1!Connections.X6,Sheet.1!Connections.Y6))"));
    close(num(cell(c, "EndX")), cm(6.0));
    close(num(cell(c, "EndY")), cm(29.7 - 6.0));
    let connect = page(&doc, 0).connects.get(0).expect("a Connect row");
    assert_eq!((connect.from_cell.as_deref(), connect.to_part, connect.to_cell.as_deref()), (Some("EndX"), Some(105), Some("Connections.X6")));
    // The row it names exists on the shape: row 5 at shares (1, 0.25).
    close(num(row_cell(shape(&doc, 0, 0), "Connection", None, 5, "X")), cm(4.0));
}

#[test]
fn odg_shapes_extrusion_and_fontwork_are_objects_and_a_measure_is_a_1_d_object() {
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:extrusion="true" draw:enhanced-path="M 0 0 L 1 1"/><text:p>x</text:p></draw:custom-shape>"#,
            r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:text-path="true"/></draw:custom-shape>"#,
            r#"<draw:measure svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="1cm"><text:p>4</text:p></draw:measure>"#,
            r#"<draw:caption svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:caption-point-x="1cm" draw:caption-point-y="2cm"/>"#,
            r#"<draw:control svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:control="c1"/>"#
        ))
        .read();
    assert_eq!(page(&doc, 0).shapes.len(), 5);
    assert_eq!(
        (report.kept("extrusion"), report.kept("fontwork"), report.kept("measure"), report.kept("caption"), report.kept("control")),
        (1, 1, 1, 1, 1)
    );
    for at in 0..5 {
        let s = shape(&doc, 0, at);
        assert_eq!(s.kind, ShapeType::Foreign, "{at}");
        assert!(s.foreign.is_none(), "no picture: a placeholder, {at}");
        assert_eq!(rows(s, 0), 0, "no geometry, {at}");
    }
    // The measure is a 1-D shape: its ends, the 1-D formulas.
    let m = shape(&doc, 0, 2);
    close(num(cell(m, "BeginX")), cm(1.0));
    close(num(cell(m, "EndX")), cm(5.0));
    assert_eq!(formula(cell(m, "Width")), Some("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"));
    // The caption's point and the control's name stay as attributes the model does not map.
    assert_eq!(odf(shape(&doc, 0, 3)).raw.get("draw:caption-point-x"), Some("1cm"));
    assert_eq!(odf(shape(&doc, 0, 4)).raw.get("draw:control"), Some("c1"));
}

#[test]
fn odg_shapes_an_object_frame_whose_image_is_not_one_diacraft_draws_still_shows_it() {
    // An SVG replacement: the shape holds its bytes as the picture (the renderer judges the format), and the format is counted.
    let mut d = Drawing::a4().page(
        r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:object-ole xlink:href="./Object 2"/><draw:image xlink:href="Pictures/o.svg"/></draw:frame>"#,
    );
    d.files.push(("Pictures/o.svg".to_owned(), b"<svg/>".to_vec()));
    let (doc, report) = d.read();
    let s = only(&doc);
    assert_eq!(s.kind, ShapeType::Foreign);
    assert!(s.foreign.as_ref().is_some_and(|f| f.picture.is_some() && f.attr("ForeignType").is_none()));
    assert_eq!(report.kept("picture_format"), 1);
    // And one whose image is missing from the package is a placeholder.
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:object xlink:href="./O"/><draw:image xlink:href="Pictures/gone.png"/></draw:frame>"#)
        .read();
    assert!(only(&doc).foreign.as_ref().is_some_and(|f| f.picture.is_none()));
    assert_eq!(report.kept("picture_missing"), 1);
}
