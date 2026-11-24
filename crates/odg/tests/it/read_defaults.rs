//! The vectors for the plain shapes, pages and text: the cells, formulas and
//! units the reader writes, cell by cell. Are custom shapes, glue and connectors.

use crate::drawing::{Drawing, cell, close, cm, formula, num, only, page, row_cell, row_type, rows, settings, text, unit};

#[test]
fn odg_reader_defaults_rectangle_frame_cells() {
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "Width")), 1.574803149606299);
    assert_eq!(unit(cell(s, "Width")), Some("CM"));
    assert_eq!(formula(cell(s, "Width")), None, "Width is a value");
    close(num(cell(s, "Height")), 0.984251968503937);
    assert_eq!(unit(cell(s, "Height")), Some("CM"));
    assert_eq!(formula(cell(s, "LocPinX")), Some("Width*0.5"));
    close(num(cell(s, "LocPinX")), 1.574803149606299 / 2.0);
    assert_eq!(formula(cell(s, "LocPinY")), Some("Height*0.5"));
    close(num(cell(s, "PinX")), 1.574803149606299);
    assert_eq!(unit(cell(s, "PinX")), Some("CM"));
    close(num(cell(s, "PinY")), 10.019685039370078);
    assert_eq!(unit(cell(s, "PinY")), Some("CM"));
    for absent in ["Angle", "FlipX", "FlipY"] {
        assert!(cell(s, absent).is_none(), "{absent}");
    }
}

#[test]
fn odg_reader_defaults_line_ends_flipped_and_one_d_formulas() {
    let (doc, _) = Drawing::a4().page(r#"<draw:line svg:x1="2cm" svg:y1="5cm" svg:x2="8cm" svg:y2="2cm"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "BeginX")), cm(2.0));
    close(num(cell(s, "BeginY")), cm(24.7));
    close(num(cell(s, "EndX")), cm(8.0));
    close(num(cell(s, "EndY")), cm(27.7));
    for (name, f) in [
        ("Width", "SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"),
        ("Angle", "ATAN2(EndY-BeginY,EndX-BeginX)"),
        ("PinX", "(BeginX+EndX)/2"),
        ("PinY", "(BeginY+EndY)/2"),
        ("LocPinX", "Width*0.5"),
        ("LocPinY", "Height*0.5"),
    ] {
        assert_eq!(formula(cell(s, name)), Some(f), "{name}");
    }
    assert!((num(cell(s, "Width")) * 2.54 - 6.708203932499369).abs() < 1e-9);
    assert!((num(cell(s, "Angle")).to_degrees() - 26.56505117707799).abs() < 1e-9);
    assert_eq!(unit(cell(s, "Angle")), Some("DEG"));
    close(num(cell(s, "PinX")), cm(5.0));
    close(num(cell(s, "PinY")), cm(26.2));
    assert_eq!(text(cell(s, "Height")), "0");
    assert_eq!(formula(cell(s, "Height")), None);
    // The line's own geometry.
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("MoveTo"));
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("LineTo"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "X")), Some("Width*1"));
    assert_eq!(text(row_cell(s, "Geometry", Some(0), 2, "Y")), "0");
}

#[test]
fn odg_reader_defaults_rectangle_rows_from_one_and_the_four_connection_rows() {
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"/>"#).read();
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5);
    let expect = [("RelMoveTo", 0.0, 0.0), ("RelLineTo", 1.0, 0.0), ("RelLineTo", 1.0, 1.0), ("RelLineTo", 0.0, 1.0), ("RelLineTo", 0.0, 0.0)];
    for (i, (t, x, y)) in expect.iter().enumerate() {
        let ix = u32::try_from(i + 1).unwrap();
        assert_eq!(row_type(s, 0, ix).as_deref(), Some(*t), "row {ix}");
        close(num(row_cell(s, "Geometry", Some(0), ix, "X")), *x);
        close(num(row_cell(s, "Geometry", Some(0), ix, "Y")), *y);
        assert_eq!(formula(row_cell(s, "Geometry", Some(0), ix, "X")), None, "Rel rows hold values");
    }
    assert!(row_type(s, 0, 0).is_none(), "Geometry rows are numbered from 1");
    // Top, left, bottom, right.
    let conn = [
        (Some("Width*0.5"), Some("Height"), 0.0, 1.0),
        (None, Some("Height*0.5"), -1.0, 0.0),
        (Some("Width*0.5"), None, 0.0, -1.0),
        (Some("Width"), Some("Height*0.5"), 1.0, 0.0),
    ];
    for (ix, (x, y, dx, dy)) in conn.iter().enumerate() {
        let ix = u32::try_from(ix).unwrap();
        assert_eq!(formula(row_cell(s, "Connection", None, ix, "X")), *x, "row {ix} X");
        assert_eq!(formula(row_cell(s, "Connection", None, ix, "Y")), *y, "row {ix} Y");
        close(num(row_cell(s, "Connection", None, ix, "DirX")), *dx);
        close(num(row_cell(s, "Connection", None, ix, "DirY")), *dy);
        close(num(row_cell(s, "Connection", None, ix, "Type")), 0.0);
    }
    close(num(row_cell(s, "Connection", None, 1, "X")), 0.0);
    close(num(row_cell(s, "Connection", None, 2, "Y")), 0.0);
    // The rows' lengths are shown in the frame's units, each axis its own.
    assert_eq!(unit(row_cell(s, "Connection", None, 0, "X")), Some("CM"));
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:width="40mm" svg:height="1in"/>"#).read();
    let s = only(&doc);
    assert_eq!(unit(row_cell(s, "Connection", None, 0, "X")), Some("MM"));
    assert_eq!(unit(row_cell(s, "Connection", None, 0, "Y")), Some("IN"));
}

#[test]
fn odg_reader_defaults_circle_on_a_frame_that_is_not_square() {
    let (doc, _) = Drawing::a4().page(r#"<draw:circle svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#).read();
    let s = only(&doc);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("Ellipse"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "A")), Some("Width*0.5+MIN(Width,Height)*0.5"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "D")), Some("Height*0.5+MIN(Width,Height)*0.5"));
    close(num(row_cell(s, "Geometry", Some(0), 1, "A")), cm(3.0));
    close(num(row_cell(s, "Geometry", Some(0), 1, "D")), cm(2.0));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "X")), Some("Width*0.5"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "C")), Some("Width*0.5"));
}

#[test]
fn odg_reader_defaults_svg_arc_as_one_relative_elliptical_arc() {
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:path svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 2000 1000" svg:d="M 0 500 A 1000 500 0 0 1 2000 500"/>"#)
        .read();
    let s = only(&doc);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("RelMoveTo"));
    close(num(row_cell(s, "Geometry", Some(0), 1, "X")), 0.0);
    close(num(row_cell(s, "Geometry", Some(0), 1, "Y")), 0.5);
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("RelEllipticalArcTo"));
    assert_eq!(rows(s, 0), 2, "one piece: the arc is half the ellipse");
    let c = |name: &str| row_cell(s, "Geometry", Some(0), 2, name);
    close(num(c("X")), 1.0);
    close(num(c("Y")), 0.5);
    close(num(c("A")), 0.5);
    close(num(c("B")), 1.0);
    close(num(c("C")), 0.0);
    assert_eq!(formula(c("C")), None);
    assert_eq!(formula(c("D")), Some("1*Width/Height"));
    close(num(c("D")), 2.0);
}

#[test]
fn odg_reader_defaults_page_layout_and_scale() {
    let mut d = Drawing::a4();
    d.styles_auto = r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="8.5in" fo:page-height="11in" style:print-orientation="portrait"/></style:page-layout>"#.to_owned();
    let (doc, _) = d.clone().page("").read();
    let sheet = &page(&doc, 0).sheet;
    close(num(sheet.cell("PageWidth")), 8.5);
    assert_eq!(unit(sheet.cell("PageWidth")), Some("IN"));
    close(num(sheet.cell("PageHeight")), 11.0);
    close(num(sheet.cell("PrintPageOrientation")), 1.0);
    close(num(sheet.cell("PageScale")), 1.0);
    close(num(sheet.cell("DrawingScale")), 1.0);
    assert_eq!(unit(sheet.cell("DrawingScale")), Some("IN"));
    d.settings = Some(settings(
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">4</config:config-item>"#,
    ));
    let (doc, _) = d.page("").read();
    let sheet = &page(&doc, 0).sheet;
    close(num(sheet.cell("PageScale")), 1.0);
    close(num(sheet.cell("DrawingScale")), 4.0);
}

#[test]
fn odg_reader_defaults_one_empty_paragraph_is_no_text_and_no_rows() {
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/></draw:rect>"#).read();
    let s = only(&doc);
    assert!(s.text.is_none());
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Paragraph", None)).is_none());
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Character", None)).is_none());
}

#[test]
fn odg_reader_defaults_writes_only_the_cells_it_lists() {
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"/>"#).read();
    let s = only(&doc);
    let names: Vec<&str> = s.sheet.cells.keys().map(|n| n.as_str()).collect();
    assert_eq!(names, ["Height", "LocPinX", "LocPinY", "PinX", "PinY", "Width"]);
    // The geometry section's flags are written only when 1.
    let section = s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap();
    assert!(section.cells.is_empty());
    // A shape with no style links the root on all three.
    assert_eq!((s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text), (Some(0), Some(0), Some(0)));
}
