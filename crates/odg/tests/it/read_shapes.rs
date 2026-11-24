//! The vectors for the plain shapes, pages, master pages and layers, the reader's side (for frames).
//! Custom shapes, glue, connectors, groups and objects are; here they are
//! numbered and kept (V7 numbers a group without modelling it).

use std::sync::Arc;

use diacraft_doc::{ShapeType, TextPart};
use diacraft_odg::{Limits, read};

use crate::drawing::{Drawing, as_text, cell, close, cm, formula, num, odf, only, page, row_cell, row_type, rows, settings, shape, text, unit};

#[test]
fn odg_shapes_frame_and_the_y_flip_against_page_height() {
    // V1 names a custom shape; its frame rule is every 2-D shape's, so a rectangle carries it here and repeats it for custom
    // shapes.
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "Width")), cm(4.0));
    close(num(cell(s, "Height")), cm(2.5));
    close(num(cell(s, "PinX")), cm(4.0));
    close(num(cell(s, "PinY")), 10.019685039370078);
    close(num(cell(s, "LocPinX")), cm(2.0));
    close(num(cell(s, "LocPinY")), cm(1.25));
}

#[test]
fn odg_shapes_translate_places_the_frame() {
    let (doc, _) = Drawing::a4().page(r#"<draw:rect svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "PinX")), cm(7.0));
    close(num(cell(s, "PinY")), cm(18.7));
    assert!(cell(s, "Angle").is_none());
    assert_eq!(unit(cell(s, "PinX")), Some("CM"), "the translation's unit, with no svg:x");
}

#[test]
fn odg_shapes_a_shear_is_left_out_with_one_warning() {
    let (doc, report) =
        Drawing::a4().page(r#"<draw:rect svg:width="4cm" svg:height="2cm" draw:transform="skewX (0.2) translate (1cm 1cm)"/>"#).read();
    assert_eq!(report.warned("shear_left_out"), 1);
    let s = only(&doc);
    close(num(cell(s, "Width")), cm(4.0));
    close(num(cell(s, "Height")), cm(2.0));
    close(num(cell(s, "PinX")), cm(3.0));
    assert!(cell(s, "Angle").is_none());
}

#[test]
fn odg_shapes_rotate_is_radians_counterclockwise_about_the_frame_origin_in_every_version() {
    for version in ["1.4", "1.3", "1.2"] {
        let mut d = Drawing::a4();
        d.version = version.to_owned();
        let (doc, _) =
            d.page(r#"<draw:rect svg:width="4cm" svg:height="2cm" draw:transform="rotate (0.5235987755982988) translate (5cm 10cm)"/>"#).read();
        let s = only(&doc);
        assert!((num(cell(s, "Angle")).to_degrees() - 30.0).abs() < 1e-9, "{version}");
        assert_eq!(unit(cell(s, "Angle")), Some("DEG"));
        assert!((num(cell(s, "PinX")) * 2.54 - 7.232050807568877).abs() < 1e-9, "{version}");
        assert!((num(cell(s, "PinY")) * 2.54 - 19.833974596215562).abs() < 1e-9, "{version}");
        close(num(cell(s, "Width")), cm(4.0));
    }
    // The bar of probe V20: its own corner (6 cm, 0) lands at (15.696 cm, 4.5 cm) on the page.
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:rect svg:width="6cm" svg:height="0.4cm" draw:transform="rotate (0.5235987755982988) translate (10.5cm 7.5cm)"/>"#)
        .read();
    let s = only(&doc);
    let (px, py, a) = (num(cell(s, "PinX")), num(cell(s, "PinY")), num(cell(s, "Angle")));
    let (lx, ly) = (cm(3.0), cm(0.2));
    // The corner at (6 cm, 0) of the frame is (Width, Height) in the shape's y-up frame: from the pin, (+3 cm, +0.2 cm), turned.
    let (dx, dy) = (cm(6.0) - lx, cm(0.4) - ly);
    let (x, y) = (px + dx * a.cos() - dy * a.sin(), py + dx * a.sin() + dy * a.cos());
    assert!((x * 2.54 - 15.696).abs() < 1e-3, "{}", x * 2.54);
    assert!(((29.7 - y * 2.54) - 4.5).abs() < 1e-3, "{}", 29.7 - y * 2.54);
}

#[test]
fn odg_shapes_a_translation_written_first_is_scaled_and_turned_by_what_follows() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="translate (1cm 2cm) scale (2 3)"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="translate (1cm 0cm) rotate (1.5707963267948966)"/>"#,
            r#"<draw:rect svg:width="2cm" svg:height="1cm" draw:transform="scale (2 1) rotate (0.5235987755982988) translate (3cm 4cm)"/>"#
        ))
        .read();
    // Translate then scale: the centre (0.5 cm, 0.5 cm) goes to (1.5, 2.5), then to (3, 7.5).
    let a = shape(&doc, 0, 0);
    close(num(cell(a, "Width")), cm(2.0));
    close(num(cell(a, "Height")), cm(3.0));
    close(num(cell(a, "PinX")), cm(3.0));
    close(num(cell(a, "PinY")), cm(29.7 - 7.5));
    // Translate then a quarter turn, (u, v) to (v, -u): (1.5, 0.5) goes to (0.5, -1.5), above the page's top edge.
    let b = shape(&doc, 0, 1);
    close(num(cell(b, "PinX")), cm(0.5));
    close(num(cell(b, "PinY")), cm(29.7 + 1.5));
    assert!((num(cell(b, "Angle")).to_degrees() - 90.0).abs() < 1e-9);
    // Scale, turn 30 degrees, translate: the centre (1, 0.5) goes to (2, 0.5), then to (2 cos + 0.5 sin, -2 sin + 0.5 cos), then 3 and 4 on.
    let c = shape(&doc, 0, 2);
    let (s, co) = 30f64.to_radians().sin_cos();
    close(num(cell(c, "PinX")), cm(2.0 * co + 0.5 * s + 3.0));
    close(num(cell(c, "PinY")), cm(29.7 - (-2.0 * s + 0.5 * co + 4.0)));
    close(num(cell(c, "Width")), cm(4.0));
    close(num(cell(c, "Height")), cm(1.0));
    assert!((num(cell(c, "Angle")).to_degrees() - 30.0).abs() < 1e-9);
}

/// One term of a transform as the test applies it to a point, in centimetres on the y-down page: the rotate, translate and scale.
#[derive(Clone, Copy)]
enum Term {
    Rotate(f64),
    Translate(f64, f64),
    Scale(f64),
}

fn apply_terms(terms: &[Term], point: (f64, f64), with_translation: bool) -> (f64, f64) {
    terms.iter().fold(point, |(u, v), term| match *term {
        Term::Rotate(a) => (u * a.cos() + v * a.sin(), -u * a.sin() + v * a.cos()),
        Term::Translate(x, y) if with_translation => (u + x, v + y),
        Term::Translate(..) => (u, v),
        Term::Scale(k) => (u * k, v * k),
    })
}

#[test]
fn odg_shapes_terms_compose_in_the_order_written_whatever_they_are() {
    use Term::{Rotate, Scale, Translate};
    let cases: [(&str, Vec<Term>); 4] = [
        ("scale (2) rotate (0.5) translate (1cm 2cm)", vec![Scale(2.0), Rotate(0.5), Translate(1.0, 2.0)]),
        ("rotate (0.3) translate (1cm 2cm) rotate (0.4)", vec![Rotate(0.3), Translate(1.0, 2.0), Rotate(0.4)]),
        ("translate (1cm 2cm) rotate (0.5) scale (2) translate (0.5cm 0cm)", vec![Translate(1.0, 2.0), Rotate(0.5), Scale(2.0), Translate(0.5, 0.0)]),
        ("translate (1cm 2cm) scale (3) rotate (1.2) rotate (-0.2)", vec![Translate(1.0, 2.0), Scale(3.0), Rotate(1.2), Rotate(-0.2)]),
    ];
    let page_xml: String = cases.iter().map(|(t, _)| format!(r#"<draw:rect svg:width="2cm" svg:height="1cm" draw:transform="{t}"/>"#)).collect();
    let (doc, report) = Drawing::a4().page(&page_xml).read();
    assert_eq!(report.warned("shear_left_out"), 0);
    for (at, (written, terms)) in cases.iter().enumerate() {
        let s = shape(&doc, 0, at);
        // The frame's centre (1 cm, 0.5 cm) taken through each term in turn; its width's direction, (1, 0), through the turns and scales.
        let pin = apply_terms(terms, (1.0, 0.5), true);
        let along = apply_terms(terms, (1.0, 0.0), false);
        close(num(cell(s, "PinX")), cm(pin.0));
        close(num(cell(s, "PinY")), cm(29.7 - pin.1));
        close(num(cell(s, "Width")), cm(2.0 * along.0.hypot(along.1)));
        let across = apply_terms(terms, (0.0, 1.0), false);
        close(num(cell(s, "Height")), cm(across.0.hypot(across.1)));
        assert!((num(cell(s, "Angle")) - (-along.1).atan2(along.0)).abs() < 1e-9, "{written}");
    }
}

#[test]
fn odg_shapes_a_sheared_matrix_keeps_its_first_column_and_the_height_square_to_it() {
    // Spec question 12: columns (2, 1) and (-0.4, 3) are not square to each other, so the turn and width are the first column's (its
    // length the square root of 5) and the height is the area, the determinant 6.4, over that length.
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="matrix (2 1 -0.4 3 0cm 0cm)"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="scale (2 0)"/>"#
        ))
        .read();
    let s = shape(&doc, 0, 0);
    let first = 5f64.sqrt();
    close(num(cell(s, "Width")), cm(first));
    close(num(cell(s, "Height")), cm(6.4 / first));
    close(num(cell(s, "Angle")), (-1f64).atan2(2.0));
    close(num(cell(s, "PinX")), cm(2.0 * 0.5 - 0.4 * 0.5));
    close(num(cell(s, "PinY")), cm(29.7 - (0.5 + 3.0 * 0.5)));
    assert!(cell(s, "FlipY").is_none_or(|c| text(Some(c)) != "1"));
    assert_eq!(report.warned("shear_left_out"), 1);
    // A map that flattens the frame turns nothing over.
    let flat = shape(&doc, 0, 1);
    close(num(cell(flat, "Height")), 0.0);
    assert!(cell(flat, "FlipY").is_none_or(|c| text(Some(c)) != "1"));
}

#[test]
fn a_path_of_more_subpaths_than_a_sheet_holds_sections_is_too_complex() {
    // The model holds 8,192 sections a sheet; a shape keeps 4 for its other sections, so 8,188 subpaths are read and 8,189 are refused.
    let path = |subpaths: usize| {
        let d = vec!["M0 0L1 1"; subpaths].join(" ");
        Drawing::a4().page(&format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 1 1" svg:d="{d}"/>"#)).bytes()
    };
    let class = |bytes: &[u8]| read(bytes, &Limits::default()).err().map(|e| e.class());
    assert_eq!(class(&path(diacraft_doc::MAX_SECTIONS_PER_SHEET - 4)), None);
    assert_eq!(class(&path(diacraft_doc::MAX_SECTIONS_PER_SHEET - 3)), Some("xml_too_complex"));
    assert_eq!(class(&path(diacraft_doc::MAX_SECTIONS_PER_SHEET + 100)), Some("xml_too_complex"));
}

#[test]
fn a_shape_of_more_fields_than_a_section_holds_rows_is_too_complex() {
    // Each field is a `Field` row: 320,000 are read, more are refused.
    let fields = |count: usize| {
        let body = format!("<text:p>{}</text:p>", "<text:page-number/>".repeat(count));
        Drawing::a4().page(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{body}</draw:rect>"#)).bytes()
    };
    let class = |bytes: &[u8]| read(bytes, &Limits::default()).err().map(|e| e.class());
    assert_eq!(class(&fields(diacraft_doc::MAX_ROWS_PER_SECTION)), None);
    assert_eq!(class(&fields(diacraft_doc::MAX_ROWS_PER_SECTION + 10_000)), Some("xml_too_complex"));
}

#[test]
fn a_subpath_of_more_rows_than_a_section_holds_is_too_complex() {
    // A move and then `rows - 1` horizontal lines, two bytes each, so the data stays under the container's 1 MiB for one attribute.
    let path = |rows: usize| {
        let d = format!("M0 0H{}", "1 ".repeat(rows - 1));
        Drawing::a4().page(&format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 1 1" svg:d="{d}"/>"#)).bytes()
    };
    let class = |bytes: &[u8]| read(bytes, &Limits::default()).err().map(|e| e.class());
    assert_eq!(class(&path(diacraft_doc::MAX_ROWS_PER_SECTION)), None);
    assert_eq!(class(&path(diacraft_doc::MAX_ROWS_PER_SECTION + 1)), Some("xml_too_complex"));
    assert_eq!(class(&path(diacraft_doc::MAX_ROWS_PER_SECTION + 10_000)), Some("xml_too_complex"));
}

#[test]
fn odg_shapes_a_transform_of_more_terms_than_are_read_is_unreadable() {
    // `read/shapes.rs` MOST_TERMS: 64 terms are read; real transforms hold two.
    let terms = |n: usize| vec!["translate (0.5cm 0cm)"; n].join(" ");
    let (doc, report) = Drawing::a4()
        .page(&format!(
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="{}"/><draw:rect svg:width="1cm" svg:height="1cm" draw:transform="{}"/>"#,
            terms(64),
            terms(65)
        ))
        .read();
    close(num(cell(shape(&doc, 0, 0), "PinX")), cm(0.5 + 32.0));
    assert_eq!(report.warned("transform_unreadable"), 1, "the 65 terms only");
    close(num(cell(shape(&doc, 0, 1), "PinX")), cm(0.5));
}

#[test]
fn odg_shapes_drawing_order_by_z_index() {
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect draw:name="a" draw:z-index="2" svg:width="1cm" svg:height="1cm"/>"#,
            r#"<draw:rect draw:name="b" draw:z-index="0" svg:width="1cm" svg:height="1cm"/>"#,
            r#"<draw:rect draw:name="c" draw:z-index="1" svg:width="1cm" svg:height="1cm"/>"#
        ))
        .read();
    let order: Vec<&str> = page(&doc, 0).shapes.iter().map(|s| s.name.as_deref().unwrap_or("")).collect();
    assert_eq!(order, ["b", "c", "a"]);
    // The IDs follow document order, whatever the drawing order.
    let ids: Vec<u32> = page(&doc, 0).shapes.iter().map(|s| s.id).collect();
    assert_eq!(ids, [2, 3, 1]);
    assert_eq!(report.index.ordinal(0, 1), Some(1));
}

fn style_with_id(name: &str, id: u32) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties diacraft:version="1" diacraft:id="{id}"/></style:style>"#
    )
}

fn group_then_3(r1: &str, r3: &str) -> String {
    format!(
        r#"<draw:g><draw:rect draw:name="r1" {r1} svg:width="1cm" svg:height="1cm"/><draw:rect draw:name="r2" svg:width="1cm" svg:height="1cm"/></draw:g><draw:rect draw:name="r3" {r3} svg:width="1cm" svg:height="1cm"/>"#
    )
}

#[test]
fn odg_shapes_shapes_are_numbered_in_pre_order_and_diacraft_ids_are_kept() {
    // The ids of the page's top-level shapes, and of the members of the first (a group).
    let ids = |doc: &diacraft_doc::Document| -> (Vec<u32>, Vec<u32>) {
        let top = page(doc, 0).shapes.iter().map(|s| s.id).collect();
        let members = shape(doc, 0, 0).children.iter().map(|s| s.id).collect();
        (top, members)
    };
    let (doc, report) = Drawing::a4().page(&group_then_3("", "")).read();
    assert_eq!(ids(&doc), (vec![1, 4], vec![2, 3]), "group 1, r1 2, r2 3, r3 4");
    let index = report.index.page(0).unwrap();
    assert!(index.kept().is_empty(), "the group and its members are modelled");
    assert_eq!(index.modelled(), 4);
    assert_eq!(index.high_water(), 4);
    assert_eq!(report.kept("group"), 0);
    // r3's style carries DiaCraft id 2: the numbering skips it: r3 2 and the group, r1 and r2 1, 3 and 4.
    let (doc, report) = Drawing::a4().auto(&style_with_id("gr1", 2)).page(&group_then_3("", r#"draw:style-name="gr1""#)).read();
    assert_eq!(ids(&doc), (vec![1, 2], vec![3, 4]));
    assert_eq!(report.index.ordinal(0, 2), Some(4));
    // r1 and r3 both carry 2: r1 keeps it, and r3 is numbered by order, read as a copy.
    let (doc, report) =
        Drawing::a4().auto(&style_with_id("gr1", 2)).page(&group_then_3(r#"draw:style-name="gr1""#, r#"draw:style-name="gr1""#)).read();
    assert_eq!(ids(&doc), (vec![1, 4], vec![2, 3]));
    assert_eq!(report.warned("diacraft_id_repeated"), 1);
}

#[test]
fn odg_shapes_pages_on_master_pages_of_different_sizes() {
    let mut d = Drawing::a4();
    d.styles_auto.push_str(
        r#"<style:page-layout style:name="PM2"><style:page-layout-properties fo:page-width="30cm" fo:page-height="15cm"/></style:page-layout>"#,
    );
    d.masters.push_str(r#"<style:master-page style:name="Wide" style:page-layout-name="PM2"/>"#);
    d.pages = concat!(
        r#"<draw:page draw:name="one" draw:master-page-name="Default"><draw:rect svg:x="0cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></draw:page>"#,
        r#"<draw:page draw:name="two" draw:master-page-name="Wide"><draw:rect svg:x="0cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></draw:page>"#
    )
    .to_owned();
    let (doc, _) = d.read();
    close(num(page(&doc, 0).sheet.cell("PageWidth")), cm(21.0));
    close(num(page(&doc, 1).sheet.cell("PageWidth")), cm(30.0));
    close(num(page(&doc, 0).sheet.cell("PageHeight")), cm(29.7));
    close(num(page(&doc, 1).sheet.cell("PageHeight")), cm(15.0));
    close(num(cell(shape(&doc, 0, 0), "PinY")), cm(29.7 - 2.0));
    close(num(cell(shape(&doc, 1, 0), "PinY")), cm(15.0 - 2.0));
    assert_eq!((page(&doc, 0).id, page(&doc, 1).id), (0, 1));
    assert_eq!(page(&doc, 1).name.as_ref(), "two");
}

#[test]
fn odg_shapes_a_master_page_with_shapes_is_a_background_page() {
    let mut d = Drawing::a4();
    d.masters = r#"<draw:layer-set><draw:layer draw:name="layout"/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></style:master-page>"#.to_owned();
    let (doc, report) = d.page("").page("").read();
    assert_eq!(doc.pages.len(), 3);
    let back = page(&doc, 2);
    assert!(back.background);
    assert_eq!(back.name.as_ref(), "Default");
    assert_eq!(back.shapes.len(), 1);
    assert_eq!((page(&doc, 0).back_page, page(&doc, 1).back_page), (Some(back.id), Some(back.id)));
    assert_eq!(report.stats.background_pages, 1);
    assert_eq!(report.index.page(2).map(|p| p.source), Some(diacraft_odg::PageSource::MasterPage(0)));
    // A master page with no shapes is no background page.
    let (doc, _) = Drawing::a4().page("").read();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(page(&doc, 0).back_page, None);
    // Nor is one holding other things only, such as forms.
    let mut d = Drawing::a4();
    d.masters = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><office:forms/></style:master-page>"#.to_owned();
    let (doc, _) = d.page("").read();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(page(&doc, 0).back_page, None);
}

#[test]
fn odg_shapes_layers_but_not_libreoffice_s_five() {
    let mut d = Drawing::a4();
    d.masters = r#"<draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/><draw:layer draw:name="Notes" draw:display="printer"/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#.to_owned();
    let (doc, _) = d
        .page(r#"<draw:rect draw:layer="layout" svg:width="1cm" svg:height="1cm"/><draw:rect draw:layer="Notes" svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    let p = page(&doc, 0);
    let layers = p.sheet.section(&diacraft_doc::SectionKey::new("Layer", None)).expect("a layer section");
    assert_eq!(layers.rows.len(), 1);
    let at = |name: &str| crate::drawing::cell_at(&p.sheet, &diacraft_doc::CellAddr::in_row("Layer", None, diacraft_doc::RowKey::Ix(0), name));
    assert_eq!(text(at("Name")), "Notes");
    assert_eq!(text(at("NameUniv")), "Notes");
    assert_eq!(text(at("Visible")), "0");
    assert_eq!(text(at("Print")), "1");
    assert_eq!(text(at("Lock")), "0");
    assert!(cell(shape(&doc, 0, 0), "LayerMember").is_none());
    assert_eq!(odf(shape(&doc, 0, 0)).raw.get("draw:layer"), Some("layout"), "the producer's layer name is kept on the shape");
    assert_eq!(text(cell(shape(&doc, 0, 1), "LayerMember")), "0");
}

#[test]
fn odg_shapes_ellipse_fills_its_frame_and_a_circle_takes_the_smaller_side() {
    let (doc, _) = Drawing::a4().page(r#"<draw:ellipse svg:width="4cm" svg:height="2cm"/><draw:circle svg:width="4cm" svg:height="2cm"/>"#).read();
    let (e, c) = (shape(&doc, 0, 0), shape(&doc, 0, 1));
    for (name, f) in [("X", "Width*0.5"), ("Y", "Height*0.5"), ("A", "Width"), ("B", "Height*0.5"), ("C", "Width*0.5"), ("D", "Height")] {
        assert_eq!(formula(row_cell(e, "Geometry", Some(0), 1, name)), Some(f), "{name}");
    }
    close(num(row_cell(e, "Geometry", Some(0), 1, "A")), cm(4.0));
    close(num(row_cell(c, "Geometry", Some(0), 1, "A")), cm(3.0));
    close(num(row_cell(c, "Geometry", Some(0), 1, "D")), cm(2.0));
}

#[test]
fn odg_shapes_polygon_points_as_shares_with_y_flipped() {
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:polygon svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"/>"#)
        .read();
    let s = only(&doc);
    let expect = [("RelMoveTo", 0.0, 1.0), ("RelLineTo", 1.0, 1.0), ("RelLineTo", 0.5, 0.0), ("RelLineTo", 0.0, 1.0)];
    assert_eq!(rows(s, 0), 4);
    for (i, (t, x, y)) in expect.iter().enumerate() {
        let ix = u32::try_from(i + 1).unwrap();
        assert_eq!(row_type(s, 0, ix).as_deref(), Some(*t));
        close(num(row_cell(s, "Geometry", Some(0), ix, "X")), *x);
        close(num(row_cell(s, "Geometry", Some(0), ix, "Y")), *y);
    }
    assert!(row_cell(s, "Geometry", Some(0), 0, "X").is_none());
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap().cells.is_empty(), "closed: filled");
}

#[test]
fn odg_shapes_an_open_polyline_and_path_are_not_filled() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:polyline svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 10 10" draw:points="0,0 10,0 10,10"/>"#,
            r#"<draw:path svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 10 10" svg:d="M0 0 L10 0 L10 10 Z M2 2 C2 4 4 4 4 2 Q6 0 8 2"/>"#
        ))
        .read();
    let no_fill = |s: &diacraft_doc::Shape, ix: u32| {
        s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(ix))).and_then(|g| g.cells.get("NoFill").cloned()).is_some()
    };
    assert!(no_fill(shape(&doc, 0, 0), 0));
    let path = shape(&doc, 0, 1);
    assert!(!no_fill(path, 0), "closed by Z");
    assert!(no_fill(path, 1), "open");
    assert_eq!(row_type(path, 1, 2).as_deref(), Some("RelCubBezTo"));
    assert_eq!(row_type(path, 1, 3).as_deref(), Some("RelQuadBezTo"));
    close(num(row_cell(path, "Geometry", Some(1), 2, "A")), 0.2);
    close(num(row_cell(path, "Geometry", Some(1), 2, "B")), 0.6);
}

#[test]
fn odg_shapes_regular_polygons_scaled_to_their_frame() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:regular-polygon svg:width="3cm" svg:height="3cm" draw:corners="3" draw:concave="false"/>"#,
            r#"<draw:regular-polygon svg:width="3cm" svg:height="3cm" draw:corners="4" draw:concave="true" draw:sharpness="50%"/>"#
        ))
        .read();
    let tri = shape(&doc, 0, 0);
    for (ix, (x, y)) in [(1, (0.5, 1.0)), (2, (0.0, 0.0)), (3, (1.0, 0.0)), (4, (0.5, 1.0))] {
        close(num(row_cell(tri, "Geometry", Some(0), ix, "X")), x);
        close(num(row_cell(tri, "Geometry", Some(0), ix, "Y")), y);
    }
    let star = shape(&doc, 0, 1);
    assert_eq!(rows(star, 0), 9, "eight points and the closing row");
    // The first point is the top of the inner circle, so below the top edge.
    close(num(row_cell(star, "Geometry", Some(0), 1, "X")), 0.5);
    assert!(num(row_cell(star, "Geometry", Some(0), 1, "Y")) < 1.0);
}

#[test]
fn odg_shapes_an_odd_concave_regular_polygon_starts_on_the_outer_circle() {
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:regular-polygon svg:width="3cm" svg:height="3cm" draw:corners="5" draw:concave="true" draw:sharpness="50%"/>"#)
        .read();
    let star = only(&doc);
    assert_eq!(rows(star, 0), 11, "ten points and the closing row");
    assert_eq!(row_type(star, 0, 1).as_deref(), Some("RelMoveTo"));
    assert_eq!(row_type(star, 0, 2).as_deref(), Some("RelLineTo"));
    assert_eq!(row_type(star, 0, 11).as_deref(), Some("RelLineTo"));
    // The first point is the top of the outer circle, the top edge's middle.
    close(num(row_cell(star, "Geometry", Some(0), 1, "X")), 0.5);
    close(num(row_cell(star, "Geometry", Some(0), 1, "Y")), 1.0);
    // The second is on the inner circle, half the radius, 36 degrees on; the box of the points runs from -cos 18 to cos 18 across and from
    // sin 234 to 1 up.
    let (half_width, bottom) = (18f64.to_radians().cos(), 234f64.to_radians().sin());
    let at = 126f64.to_radians();
    let (x, y) = (0.5 * at.cos(), 0.5 * at.sin());
    close(num(row_cell(star, "Geometry", Some(0), 2, "X")), (x + half_width) / (2.0 * half_width));
    close(num(row_cell(star, "Geometry", Some(0), 2, "Y")), (y - bottom) / (1.0 - bottom));
}

#[test]
fn odg_shapes_slices_of_circles_and_ellipses_by_parameter_angles() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:circle svg:width="4cm" svg:height="4cm" draw:kind="section" draw:start-angle="0" draw:end-angle="60"/>"#,
            r#"<draw:ellipse svg:width="6cm" svg:height="4cm" draw:kind="section" draw:start-angle="90" draw:end-angle="120"/>"#
        ))
        .read();
    let c = shape(&doc, 0, 0);
    let g = |s, ix, n| num(row_cell(s, "Geometry", Some(0), ix, n));
    assert_eq!(row_type(c, 0, 1).as_deref(), Some("RelMoveTo"));
    close(g(c, 1, "X"), 0.5);
    close(g(c, 1, "Y"), 0.5);
    close(g(c, 2, "X"), 1.0);
    close(g(c, 2, "Y"), 0.5);
    assert_eq!(row_type(c, 0, 3).as_deref(), Some("RelEllipticalArcTo"));
    assert!((g(c, 3, "X") - 0.75).abs() < 1e-7);
    assert!((g(c, 3, "Y") - 0.9330127).abs() < 1e-7);
    assert!((g(c, 3, "A") - 0.9330127).abs() < 1e-7);
    assert!((g(c, 3, "B") - 0.75).abs() < 1e-7);
    close(g(c, 4, "X"), 0.5);
    close(g(c, 4, "Y"), 0.5);
    let e = shape(&doc, 0, 1);
    assert!((g(e, 3, "X") - 0.25).abs() < 1e-7, "{}", g(e, 3, "X"));
    assert!((g(e, 3, "Y") - 0.9330127).abs() < 1e-7);
}

#[test]
fn odg_shapes_a_circle_s_slice_on_a_frame_that_is_not_square_keeps_to_the_centred_square() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:circle svg:width="4cm" svg:height="2cm" draw:kind="section" draw:start-angle="0" draw:end-angle="90"/>"#,
            r#"<draw:circle svg:width="2cm" svg:height="4cm" draw:kind="section" draw:start-angle="0" draw:end-angle="90"/>"#,
            r#"<draw:circle svg:width="0cm" svg:height="2cm" draw:kind="section" draw:start-angle="0" draw:end-angle="90"/>"#,
            r#"<draw:circle svg:width="2cm" svg:height="0cm" draw:kind="section" draw:start-angle="0" draw:end-angle="90"/>"#
        ))
        .read();
    let g = |s, ix, n| num(row_cell(shape(&doc, 0, s), "Geometry", Some(0), ix, n));
    // Wide: the circle is the middle half across, (0.25, 0) to (0.75, 1) in shares. The slice runs from its right-middle to its top.
    for (ix, x, y) in [(1, 0.5, 0.5), (2, 0.75, 0.5), (3, 0.5, 1.0), (4, 0.5, 0.5)] {
        close(g(0, ix, "X"), x);
        close(g(0, ix, "Y"), y);
    }
    // Tall: the middle half up, (0, 0.25) to (1, 0.75).
    for (ix, x, y) in [(1, 0.5, 0.5), (2, 1.0, 0.5), (3, 0.5, 0.75), (4, 0.5, 0.5)] {
        close(g(1, ix, "X"), x);
        close(g(1, ix, "Y"), y);
    }
    // A frame of no width or no height has no square: the slice is drawn on the whole frame.
    for at in [2, 3] {
        for (ix, x, y) in [(1, 0.5, 0.5), (2, 1.0, 0.5), (3, 0.5, 1.0), (4, 0.5, 0.5)] {
            close(g(at, ix, "X"), x);
            close(g(at, ix, "Y"), y);
        }
    }
    // A circle's arc is round whatever its frame: the ratio of its axes is 1.
    close(g(0, 3, "D"), 1.0);
    close(g(1, 3, "D"), 1.0);
}

#[test]
fn odg_shapes_only_a_kind_the_standard_does_not_define_is_kept() {
    let kept = |ellipse: &str| Drawing::a4().page(ellipse).read().1.kept("property_value");
    assert_eq!(kept(r#"<draw:ellipse svg:width="4cm" svg:height="2cm"/>"#), 0, "full, by default");
    assert_eq!(
        kept(r#"<draw:ellipse svg:width="4cm" svg:height="2cm" draw:kind="section" draw:start-angle="30" draw:end-angle="30"/>"#),
        0,
        "no slice"
    );
    assert_eq!(kept(r#"<draw:ellipse svg:width="4cm" svg:height="2cm" draw:kind="wedge"/>"#), 1);
}

#[test]
fn shapes_share_their_rectangle_and_those_of_one_size_their_connection_rows() {
    let (doc, _) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect svg:x="1cm" svg:width="2cm" svg:height="1cm"/><draw:rect svg:x="5cm" svg:width="2cm" svg:height="1cm"/>"#,
            r#"<draw:rect svg:width="3cm" svg:height="1cm"/>"#
        ))
        .read();
    let section = |at: usize, name: &str| {
        shape(&doc, 0, at).sheet.sections.iter().find(|s| s.name.as_str() == name).map(std::sync::Arc::clone).expect("section")
    };
    // The rectangle's rows are shares of the frame, one section for every size; the connection rows hold lengths, one for each size.
    assert!(std::sync::Arc::ptr_eq(&section(0, "Geometry"), &section(2, "Geometry")));
    assert!(std::sync::Arc::ptr_eq(&section(0, "Connection"), &section(1, "Connection")), "one size, one section");
    assert!(!std::sync::Arc::ptr_eq(&section(0, "Connection"), &section(2, "Connection")), "another size, another");
}

#[test]
fn odg_shapes_scale_items_in_the_page_s_unit() {
    let mut d = Drawing::a4();
    d.settings = Some(settings(
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">50</config:config-item>"#,
    ));
    let (doc, _) = d.page("").read();
    let s = &page(&doc, 0).sheet;
    close(num(s.cell("PageScale")), cm(1.0));
    close(num(s.cell("DrawingScale")), cm(50.0));
    assert_eq!(unit(s.cell("DrawingScale")), Some("CM"));
    let (doc, _) = Drawing::a4().page("").read();
    close(num(page(&doc, 0).sheet.cell("DrawingScale")), cm(1.0));
}

#[test]
fn odg_shapes_corner_radius_and_unequal_radii() {
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect svg:width="4cm" svg:height="2cm" draw:corner-radius="0.5cm"/>"#,
            r#"<draw:rect svg:width="4cm" svg:height="2cm" svg:rx="0.4cm" svg:ry="0.2cm"/>"#
        ))
        .read();
    let r = cell(shape(&doc, 0, 0), "Rounding");
    close(num(r), cm(0.5));
    assert_eq!(unit(r), Some("CM"));
    close(num(cell(shape(&doc, 0, 1), "Rounding")), cm(0.2));
    assert_eq!(odf(shape(&doc, 0, 1)).raw.get("svg:rx"), Some("0.4cm"), "both kept");
    assert_eq!(report.kept("corner_radii"), 1);
}

#[test]
fn odg_shapes_names() {
    let (doc, _) =
        Drawing::a4().page(r#"<draw:rect draw:name="Box A" svg:width="1cm" svg:height="1cm"/><draw:rect svg:width="1cm" svg:height="1cm"/>"#).read();
    assert_eq!(shape(&doc, 0, 0).name.as_deref(), Some("Box A"));
    assert_eq!(shape(&doc, 0, 0).name_u.as_deref(), Some("Box A"));
    assert_eq!((shape(&doc, 0, 1).name.as_deref(), shape(&doc, 0, 1).name_u.as_deref()), (None, None));
}

#[test]
fn odg_shapes_a_circle_given_by_its_centre() {
    let (doc, _) = Drawing::a4().page(r#"<draw:circle svg:cx="5cm" svg:cy="4cm" svg:r="1cm"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "Width")), cm(2.0));
    close(num(cell(s, "PinX")), cm(5.0));
    close(num(cell(s, "PinY")), cm(25.7));
    assert!(odf(s).raw.get("svg:cx").is_none(), "read, not kept unread");
}

#[test]
fn odg_shapes_a_line_s_transform_moves_both_ends() {
    let (doc, _) =
        Drawing::a4().page(r#"<draw:line svg:x1="0cm" svg:y1="0cm" svg:x2="4cm" svg:y2="0cm" draw:transform="translate (2cm 3cm)"/>"#).read();
    let s = only(&doc);
    close(num(cell(s, "BeginX")), cm(2.0));
    close(num(cell(s, "BeginY")), cm(26.7));
    close(num(cell(s, "EndX")), cm(6.0));
    close(num(cell(s, "EndY")), cm(26.7));
}

#[test]
fn odg_shapes_a_text_frame_holds_its_text_box_s_text() {
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box><text:p>Hello</text:p></draw:text-box></draw:frame>"#)
        .read();
    let s = only(&doc);
    assert_eq!(s.kind, ShapeType::Shape);
    let parts = &s.text.as_ref().expect("text").parts;
    assert!(parts.iter().any(|p| matches!(p, TextPart::Chars(c) if c.as_ref() == "Hello")));
    close(num(cell(s, "Width")), cm(5.0));
}

#[test]
fn odg_shapes_a_picture_frame_is_a_foreign_shape_with_the_picture_s_bytes() {
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
    let mut d = Drawing::a4()
        .page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"><draw:image xlink:href="Pictures/r.png"/></draw:frame>"#);
    d.files.push(("Pictures/r.png".to_owned(), png.to_vec()));
    let (doc, _) = d.read();
    let s = only(&doc);
    assert_eq!(s.kind, ShapeType::Foreign);
    let foreign = s.foreign.as_ref().expect("foreign data");
    assert_eq!(foreign.attr("ForeignType"), Some("Bitmap"));
    let media = foreign.picture.as_ref().expect("a picture");
    assert_eq!(media.bytes.as_ref(), png);
    assert_eq!(media.content_type.as_ref(), "image/png");
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).is_none(), "no geometry");
    assert!(cell(s, "ImgWidth").is_none());
}

#[test]
fn odg_shapes_a_frame_holding_an_object_is_a_foreign_shape_on_its_frame_drawn_from_its_replacement_image() {
    // A frame holding `draw:object` and `draw:image` is a shape on that frame drawn from the image; a frame holding only a table is a
    // shape with no picture, counted as an object.
    let mut d = Drawing::a4().page(concat!(
        r#"<draw:frame svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#,
        r#"<draw:frame svg:width="5cm" svg:height="3cm"><table:table/></draw:frame>"#
    ));
    d.files.push(("ObjectReplacements/Object 1".to_owned(), vec![0x89, b'P', b'N', b'G']));
    let (doc, report) = d.read();
    assert_eq!(page(&doc, 0).shapes.len(), 2);
    assert_eq!(report.kept("frame_object"), 2);
    let first = shape(&doc, 0, 0);
    assert_eq!(first.kind, diacraft_doc::ShapeType::Foreign);
    close(num(cell(first, "Width")), cm(5.0));
    close(num(cell(first, "PinX")), cm(4.5));
    let foreign = first.foreign.as_ref().expect("the replacement image");
    assert_eq!(foreign.picture.as_ref().map(|p| p.bytes.as_ref()), Some(&[0x89, b'P', b'N', b'G'][..]));
    assert_eq!(foreign.attr("ForeignType"), Some("Bitmap"));
    assert_eq!(odf(first).kept.len(), 1, "the object is kept as the file has it");
    let second = shape(&doc, 0, 1);
    assert_eq!(second.kind, diacraft_doc::ShapeType::Foreign);
    assert!(
        second.foreign.is_none() && second.sheet.sections.iter().all(|s| s.name.as_str() != "Geometry"),
        "a placeholder: no picture, no geometry"
    );
    assert!(report.index.page(0).unwrap().kept().is_empty());
}

#[test]
fn odg_ownership_a_later_shape_holding_an_earlier_one_s_id_is_read_without_diacraft_s_data() {
    let style = |name: &str| {
        format!(
            r##"<style:style style:name="{name}" style:family="graphic" style:parent-style-name="Ln"><style:graphic-properties draw:fill-color="#00aa00" diacraft:version="1" diacraft:id="7" diacraft:links="Ln Fl Ln"/></style:style>"##
        )
    };
    let (doc, report) = Drawing::a4()
        .common(r##"<style:style style:name="Ln" style:family="graphic"><style:graphic-properties draw:fill-color="#ffffff"/></style:style><style:style style:name="Fl" style:family="graphic"><style:graphic-properties draw:fill-color="#00aa00"/></style:style>"##)
        .auto(&style("gr1"))
        .auto(&style("gr2"))
        .page(r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/><draw:rect draw:style-name="gr2" svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    let (first, second) = (shape(&doc, 0, 0), shape(&doc, 0, 1));
    assert_eq!(first.id, 7);
    let fl = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Fl")).unwrap().id;
    assert_eq!(first.sheet.links.fill, Some(fl), "the first keeps its links");
    assert!(cell(first, "FillForegnd").is_none(), "equal to what its fill link gives");
    assert_ne!(second.id, 7);
    assert_ne!(second.sheet.links.fill, Some(fl), "the copy has its native links only");
    assert_eq!(text(cell(second, "FillForegnd")), "#00AA00", "and its native cells");
    assert_eq!(report.warned("diacraft_id_repeated"), 1);
}

#[test]
fn a_prefix_bound_on_one_element_is_not_bound_on_its_siblings() {
    // The first rect binds `q` to the drawing namespace on itself; its sibling's `q:rect` is in no namespace, and not a shape.
    let (doc, report) = Drawing::a4()
        .page(r#"<q:rect xmlns:q="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" svg:width="1cm" svg:height="1cm"/><q:rect svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    assert_eq!(page(&doc, 0).shapes.len(), 1);
    assert_eq!(report.kept("page_child"), 1);
    assert_eq!(report.index.page(0).unwrap().elements(), 1, "one drawing shape numbered");
}

#[test]
fn a_kept_element_is_charged_for_every_element_attribute_and_text_node_it_holds() {
    // A hyperlink wrapper kept whole: three items for each element, two for each attribute and two for each text node.
    let children: String = (0..1_000).map(|k| format!(r#"<draw:equation draw:name="f{k}"/><draw:handle>t</draw:handle>"#)).collect();
    let page_of = |inner: &str| format!(r#"<draw:a svg:width="1cm" svg:height="1cm">{inner}</draw:a>"#);
    let items = |inner: &str, most: usize| {
        let d = Drawing::a4().page(&page_of(inner));
        read(&d.bytes(), &Limits { max_items: most, ..Limits::default() }).map(|(_, report)| report.work.items).map_err(|e| e.class())
    };
    let empty = items("", Limits::default().max_items).expect("reads");
    let full = items(&children, Limits::default().max_items).expect("reads");
    // 2,000 child elements, 1,000 attributes and 1,000 text nodes; and the element's bytes, its names, values and text, one item for each
    // 64 of them: the custom shape's own, then with its children's.
    let own = "draw:a".len() + "svg:width".len() + "1cm".len() + "svg:height".len() + "1cm".len();
    let held: usize = (0..1_000).map(|k| "draw:equation".len() + "draw:name".len() + format!("f{k}").len() + "draw:handle".len() + 1).sum();
    // As text, at nine eighths.
    let bytes = as_text((own + held) as u64).div_ceil(64) - as_text(own as u64).div_ceil(64);
    assert_eq!(full - empty, 2_000 * 3 + 1_000 * 2 + 1_000 * 2 + bytes);
    // So a cap between the two refuses the second drawing.
    assert_eq!(items(&children, usize::try_from(full).unwrap() - 1).err(), Some("too_many_items"));
    assert!(items(&children, usize::try_from(full).unwrap()).is_ok());
}

#[test]
fn a_frame_holding_an_object_is_an_object_whatever_else_it_holds_and_a_scene_is_numbered() {
    let mut d = Drawing::a4().page(concat!(
        r#"<draw:frame svg:width="5cm" svg:height="3cm"><table:table/><draw:image xlink:href="Pictures/r.png"/></draw:frame>"#,
        r#"<dr3d:scene svg:width="1cm" svg:height="1cm"/><draw:rect svg:width="1cm" svg:height="1cm"/>"#
    ));
    d.files.push(("Pictures/r.png".to_owned(), vec![0x89, b'P', b'N', b'G']));
    let (doc, report) = d.read();
    assert_eq!(report.kept("frame_object"), 1);
    assert_eq!(report.kept("scene_3d"), 1);
    let kinds: Vec<_> = page(&doc, 0).shapes.iter().map(|s| (s.id, s.kind.clone())).collect();
    use diacraft_doc::ShapeType::{Foreign, Shape};
    assert_eq!(kinds, [(1, Foreign), (2, Foreign), (3, Shape)], "the frame 1, the scene 2, the rectangle 3");
    assert!(shape(&doc, 0, 0).foreign.as_ref().is_some_and(|f| f.picture.is_some()), "the frame shows its image, not a table");
    assert!(shape(&doc, 0, 1).foreign.is_none());
}

#[test]
fn a_diacraft_line_written_by_the_spec_s_rules_reads_back_to_its_rows() {
    // A line from (3 cm, 5 cm) to (9 cm, 8 cm) up the page, with the 1-D cells and rows `connector.add` writes
    // Written as R24 says (its ends into `svg:x1` to `svg:y2`, y flipped against the page's height),
    // reads back to the same formulas and values; only the `U`s differ, which the rebuild test ignores.
    let (bx, by, ex, ey) = (cm(3.0), cm(5.0), cm(9.0), cm(8.0));
    let page_height = cm(29.7);
    let written = format!(
        r#"<draw:line svg:x1="{}cm" svg:y1="{}cm" svg:x2="{}cm" svg:y2="{}cm"/>"#,
        bx * 2.54,
        (page_height - by) * 2.54,
        ex * 2.54,
        (page_height - ey) * 2.54
    );
    let (doc, _) = Drawing::a4().page(&written).read();
    let s = only(&doc);
    for (name, value) in [("BeginX", bx), ("BeginY", by), ("EndX", ex), ("EndY", ey)] {
        assert!((num(cell(s, name)) - value).abs() < 1e-12, "{name}");
        assert_eq!(formula(cell(s, name)), None);
    }
    let width = (ex - bx).hypot(ey - by);
    for (name, f, value) in [
        ("Width", "SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)", width),
        ("PinX", "(BeginX+EndX)/2", (bx + ex) / 2.0),
        ("PinY", "(BeginY+EndY)/2", (by + ey) / 2.0),
        ("LocPinX", "Width*0.5", width / 2.0),
        ("LocPinY", "Height*0.5", 0.0),
        ("Angle", "ATAN2(EndY-BeginY,EndX-BeginX)", (ey - by).atan2(ex - bx)),
    ] {
        assert_eq!(formula(cell(s, name)), Some(f), "{name}");
        assert!((num(cell(s, name)) - value).abs() < 1e-12, "{name}");
    }
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("MoveTo"));
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("LineTo"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "X")), Some("Width*1"));
    assert!((num(row_cell(s, "Geometry", Some(0), 2, "X")) - width).abs() < 1e-12);
}

#[test]
fn a_frame_s_picture_is_read_within_its_own_namespace_declarations() {
    let png = [0x89, b'P', b'N', b'G', 1];
    let mut d = Drawing::a4().page(
        r#"<draw:frame svg:width="3cm" svg:height="2cm"><draw:image xmlns:l="http://www.w3.org/1999/xlink" l:href="Pictures/r.png"/></draw:frame>"#,
    );
    d.files.push(("Pictures/r.png".to_owned(), png.to_vec()));
    let (doc, _) = d.read();
    let picture = only(&doc).foreign.as_ref().and_then(|f| f.picture.clone()).expect("a picture");
    assert_eq!(picture.bytes.as_ref(), png);
}

#[test]
fn odg_shapes_pictures_by_their_signature_href_spelling_inline_data_and_budget() {
    let emf = {
        let mut b = vec![1u8; 48];
        b[40..44].copy_from_slice(b" EMF");
        b
    };
    let cases: Vec<(&str, Vec<u8>, Option<&str>, &str)> = vec![
        ("a.jpg", vec![0xFF, 0xD8, 0xFF, 1], Some("Bitmap"), "image/jpeg"),
        ("a.gif", b"GIF89a".to_vec(), Some("Bitmap"), "image/gif"),
        ("a.bmp", b"BMxx".to_vec(), Some("Bitmap"), "image/bmp"),
        ("a.emf", emf, Some("EnhMetaFile"), "image/emf"),
        ("a.svg", b"<svg/>".to_vec(), None, "application/octet-stream"),
    ];
    for (name, bytes, kind, content_type) in cases {
        let mut d =
            Drawing::a4().page(&format!(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="./Pictures/{name}"/></draw:frame>"#));
        d.files.push((format!("Pictures/{name}"), bytes.clone()));
        let (doc, _) = d.read();
        let f = only(&doc).foreign.as_ref().expect("foreign");
        assert_eq!(f.attr("ForeignType"), kind, "{name}");
        let media = f.picture.as_ref().expect("picture");
        assert_eq!(media.bytes.as_ref(), bytes.as_slice());
        // The manifest's row names text/xml for every file the tests' writer adds that is not a .png: its type wins over the signature's.
        let expected = if name.ends_with(".png") { content_type } else { "text/xml" };
        assert_eq!(media.content_type.as_ref(), expected, "{name}");
        assert_eq!(media.part.as_ref(), format!("Pictures/{name}"));
    }
    // A percent-encoded href, and one with a raw space.
    for href in ["Pictures/my%20pic.png", "Pictures/my pic.png"] {
        let mut d = Drawing::a4().page(&format!(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="{href}"/></draw:frame>"#));
        d.files.push(("Pictures/my pic.png".to_owned(), vec![0x89, b'P', b'N', b'G']));
        let (doc, _) = d.read();
        assert!(only(&doc).foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some(), "{href}");
    }
    // An entry whose name holds a literal %: the href as written.
    let mut d = Drawing::a4().page(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/100%25.png"/></draw:frame>"#);
    d.files.push(("Pictures/100%25.png".to_owned(), vec![0x89, b'P', b'N', b'G']));
    assert!(only(&d.read().0).foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some());
    // Inline data, base64.
    let (doc, _) = Drawing::a4()
        .page(
            r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw==</office:binary-data></draw:image></draw:frame>"#,
        )
        .read();
    let media = only(&doc).foreign.as_ref().and_then(|f| f.picture.clone()).expect("inline picture");
    assert_eq!(media.bytes.as_ref(), [0x89, b'P', b'N', b'G']);
    assert_eq!(media.content_type.as_ref(), "image/png");
    // Missing, and past the budget.
    let (doc, report) =
        Drawing::a4().page(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/none.png"/></draw:frame>"#).read();
    assert!(only(&doc).foreign.as_ref().is_some_and(|f| f.picture.is_none()));
    assert_eq!((report.warned("picture_missing"), report.kept("picture_missing")), (1, 1));
    // The media budget is charged once for each part, whatever the number of frames that name it, and they share its bytes: two frames of
    // one 6-byte part and a 3-byte part fit a budget of 9; a third part of 6 bytes does not, and no picture is left out of a model that is to hold the
    // package's entries: the read is refused, in the class of the budget of pictures, where it left the picture out and went on.
    let frame = |name: &str| format!(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/{name}"/></draw:frame>"#);
    let mut d = Drawing::a4().page(&format!("{}{}{}{}", frame("r.png"), frame("r.png"), frame("s.png"), frame("t.png")));
    d.files.push(("Pictures/r.png".to_owned(), vec![0x89, b'P', b'N', b'G', 0, 0]));
    d.files.push(("Pictures/s.png".to_owned(), vec![0xFF, 0xD8, 0]));
    d.files.push(("Pictures/t.png".to_owned(), vec![0x89, b'P', b'N', b'G', 1, 1]));
    match read(&d.bytes(), &Limits { max_media_bytes: 9, ..Limits::default() }) {
        Err(e) => assert_eq!(e.class(), "xml_too_complex", "{e}"),
        Ok(_) => panic!("a picture the budget has no room for was left out of a model that keeps the entries of the package"),
    }
    let (doc, report) = read(&d.bytes(), &Limits { max_media_bytes: 6 + 3 + 6, ..Limits::default() }).unwrap();
    let media = |at: usize| shape(&doc, 0, at).foreign.as_ref().and_then(|f| f.picture.clone());
    let (first, second) = (media(0).expect("fits"), media(1).expect("the same part"));
    assert!(Arc::ptr_eq(&first, &second), "one picture for both frames");
    assert!(media(2).is_some(), "3 more bytes fit");
    assert!(media(3).is_some(), "and 6 more");
    assert_eq!(report.kept("picture_over_budget"), 0);
    assert_eq!(report.work.media_bytes, 6 + 3 + 6);
    assert!(doc.parts.is_empty(), "the pictures a frame shows are pictures and no parts: nothing is held twice");
    // Inline data past the budget is refused by its length before it is decoded.
    let inline =
        r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw==</office:binary-data></draw:image></draw:frame>"#;
    let (doc, report) = read(&Drawing::a4().page(inline).bytes(), &Limits { max_media_bytes: 3, ..Limits::default() }).unwrap();
    assert!(only(&doc).foreign.as_ref().is_some_and(|f| f.picture.is_none()));
    assert_eq!((report.kept("picture_over_budget"), report.work.media_bytes), (1, 0));
    let (_, report) = read(&Drawing::a4().page(inline).bytes(), &Limits { max_media_bytes: 4, ..Limits::default() }).unwrap();
    assert_eq!((report.kept("picture_over_budget"), report.work.media_bytes), (0, 4));
}

#[test]
fn odg_shapes_more_kinds_and_forms() {
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:ellipse svg:width="4cm" svg:height="2cm" draw:kind="cut" draw:start-angle="0" draw:end-angle="180"/>"#,
            r#"<draw:circle svg:width="2cm" svg:height="2cm" draw:kind="arc" draw:start-angle="90deg" draw:end-angle="0"/>"#,
            r#"<draw:ellipse svg:width="4cm" svg:height="2cm" draw:kind="wedge"/>"#,
            r#"<draw:ellipse svg:cx="5cm" svg:cy="4cm" svg:rx="2cm" svg:ry="1cm"/>"#,
            r#"<draw:rect svg:width="4cm" svg:height="2cm" svg:ry="0.3cm"/>"#,
            r#"<draw:regular-polygon svg:width="1cm" svg:height="1cm" draw:corners="2"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="scale (2 3) translate (1cm 1cm)"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="matrix (0 -1 1 0 1cm 2cm)"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="scale (1 -1)"/>"#,
            r#"<draw:rect svg:width="1cm" svg:height="1cm" draw:transform="rotate (x)"/>"#
        ))
        .read();
    let g = |s, ix, n| num(row_cell(shape(&doc, 0, s), "Geometry", Some(0), ix, n));
    // A cut: move to the start, the arc, back to the start.
    assert_eq!(row_type(shape(&doc, 0, 0), 0, 1).as_deref(), Some("RelMoveTo"));
    close(g(0, 1, "X"), 1.0);
    close(g(0, 1, "Y"), 0.5);
    assert_eq!(row_type(shape(&doc, 0, 0), 0, 2).as_deref(), Some("RelEllipticalArcTo"));
    close(g(0, 2, "X"), 0.0);
    close(g(0, 2, "B"), 1.0);
    close(g(0, 2, "D"), 2.0);
    assert_eq!(row_type(shape(&doc, 0, 0), 0, 3).as_deref(), Some("RelLineTo"));
    close(g(0, 3, "X"), 1.0);
    // An arc from 90 to 360 degrees: open and not filled.
    let arc = shape(&doc, 0, 1);
    assert_eq!(rows(arc, 0), 2);
    assert!(arc.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).is_some_and(|s| s.cells.contains_key("NoFill")));
    close(g(1, 1, "X"), 0.5);
    close(g(1, 1, "Y"), 1.0);
    // A kind the standard does not define: the full ellipse, kept.
    assert_eq!(row_type(shape(&doc, 0, 2), 0, 1).as_deref(), Some("Ellipse"));
    assert_eq!(report.kept("property_value"), 1);
    // An ellipse by its centre and half-axes.
    close(num(cell(shape(&doc, 0, 3), "Width")), cm(4.0));
    close(num(cell(shape(&doc, 0, 3), "PinY")), cm(29.7 - 4.0));
    // One radius stands for both.
    close(num(cell(shape(&doc, 0, 4), "Rounding")), cm(0.3));
    // Two corners are no polygon: the frame, with a warning.
    assert_eq!(rows(shape(&doc, 0, 5), 0), 5);
    assert!(report.warned("geometry_unreadable") >= 1);
    // `scale` then `translate`: the frame is stretched about the origin and then moved.
    let scaled = shape(&doc, 0, 6);
    close(num(cell(scaled, "Width")), cm(2.0));
    close(num(cell(scaled, "Height")), cm(3.0));
    close(num(cell(scaled, "PinX")), cm(2.0));
    // A matrix turning a quarter: the first column (0, -1) points up the page, so the turn is 90 degrees.
    assert!((num(cell(shape(&doc, 0, 7), "Angle")).to_degrees() - 90.0).abs() < 1e-9);
    // A negative determinant flips.
    assert_eq!(text(cell(shape(&doc, 0, 8), "FlipY")), "1");
    // A transform that cannot be read: read as if absent, with a warning, and kept.
    assert_eq!(report.warned("transform_unreadable"), 1);
    assert_eq!(odf(shape(&doc, 0, 9)).raw.get("draw:transform"), Some("rotate (x)"));
}

#[test]
fn odg_shapes_kept_children_of_a_modelled_shape_by_kind() {
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:rect svg:width="1cm" svg:height="1cm"><draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/><office:event-listeners/><draw:image-map/>loose text<text:p>t</text:p></draw:rect><draw:frame svg:width="1cm" svg:height="1cm"><draw:text-box><draw:frame/><text:p>u</text:p></draw:text-box></draw:frame>"#)
        .read();
    assert_eq!((report.kept("glue_point"), report.kept("event_listeners"), report.kept("image_map"), report.kept("shape_text_node")), (1, 1, 1, 1));
    assert_eq!(odf(shape(&doc, 0, 0)).kept.len(), 4);
    assert_eq!(report.kept("text_box_child"), 1);
    assert_eq!(odf(shape(&doc, 0, 1)).kept.len(), 1);
    // White space between children is no text node to keep; a text box inside a shape that is no frame is a child like any other; and
    // a text box's paragraphs are its text, whatever else it holds.
    let (doc, report) = Drawing::a4()
        .page(concat!(
            "<draw:rect svg:width=\"1cm\" svg:height=\"1cm\">\n  <draw:text-box/>\n  <text:p>t</text:p>\n</draw:rect>",
            r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:text-box><text:p>a</text:p><draw:frame/><text:p>b</text:p><text:list/></draw:text-box></draw:frame>"#
        ))
        .read();
    assert_eq!(report.kept("shape_text_node"), 0);
    assert_eq!(report.kept("shape_child"), 1, "the text box in the rectangle");
    assert_eq!(odf(shape(&doc, 0, 0)).kept.len(), 1);
    assert_eq!(report.kept("text_box_child"), 1, "the frame in the text box, not its paragraphs or list");
    // A picture or text box in a shape that is no frame is a shape's child; a frame's second text box is a frame's.
    let (_, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:rect svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/none.png"/></draw:rect>"#,
            r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:text-box><text:p>a</text:p></draw:text-box><draw:text-box/></draw:frame>"#
        ))
        .read();
    assert_eq!((report.kept("shape_child"), report.kept("frame_image"), report.kept("frame_text_box")), (1, 0, 1));
}

#[test]
fn odg_shapes_a_frame_shows_the_first_child_diacraft_supports_and_keeps_the_rest() {
    // An SVG then a PNG drew a placeholder, and the PNG was neither shown nor kept.
    let frame = |children: &str| format!(r#"<draw:frame svg:width="1cm" svg:height="1cm">{children}</draw:frame>"#);
    let image = |name: &str| format!(r#"<draw:image xlink:href="Pictures/{name}"/>"#);
    let read_frame = |children: &str| {
        let mut d = Drawing::a4().page(&frame(children));
        d.files.push(("Pictures/a.svg".to_owned(), br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec()));
        d.files.push(("Pictures/b.png".to_owned(), vec![0x89, b'P', b'N', b'G', 0]));
        d.files.push(("Pictures/c.png".to_owned(), vec![0x89, b'P', b'N', b'G', 1]));
        d.read()
    };
    let shown = |doc: &diacraft_doc::Document| only(doc).foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|p| p.part.to_string());
    // An SVG, then a PNG: the PNG; the SVG kept and counted.
    let (doc, report) = read_frame(&format!("{}{}", image("a.svg"), image("b.png")));
    assert_eq!(shown(&doc).as_deref(), Some("Pictures/b.png"));
    assert_eq!(report.kept("frame_image"), 1);
    assert_eq!(odf(only(&doc)).kept.len(), 1);
    // Two PNGs: the first, and the second kept.
    let (doc, report) = read_frame(&format!("{}{}", image("b.png"), image("c.png")));
    assert_eq!(shown(&doc).as_deref(), Some("Pictures/b.png"));
    assert_eq!(report.kept("frame_image"), 1);
    // An SVG and then a text box: the text box, and the SVG kept.
    let (doc, report) = read_frame(&format!(r#"{}<draw:text-box><text:p>t</text:p></draw:text-box>"#, image("a.svg")));
    assert!(only(&doc).foreign.is_none());
    assert!(only(&doc).text.is_some());
    assert_eq!(report.kept("frame_image"), 1);
    // Only an SVG: it is what the frame shows, as a placeholder, and its format is counted.
    let (doc, report) = read_frame(&image("a.svg"));
    assert_eq!(shown(&doc).as_deref(), Some("Pictures/a.svg"));
    assert_eq!((report.kept("picture_format"), report.kept("frame_image")), (1, 0));
    // Inline data is judged by its own first bytes.
    let inline = r#"<draw:image><office:binary-data>PHN2Zy8+</office:binary-data></draw:image><draw:image><office:binary-data>iVBORw==</office:binary-data></draw:image>"#;
    let (doc, report) = read_frame(inline);
    assert_eq!(shown(&doc).as_deref(), Some("office:binary-data"));
    assert_eq!(only(&doc).foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|p| p.bytes.len()), Some(4), "the PNG's four bytes");
    assert_eq!(report.kept("frame_image"), 1);
    // An EMF's signature is at bytes 40 to 44, so the inline data's first sixty characters are read to find it.
    let mut emf = vec![0u8; 48];
    emf[40..44].copy_from_slice(b" EMF");
    let inline = format!(
        r#"<draw:image><office:binary-data>{}</office:binary-data></draw:image><draw:image><office:binary-data>iVBORw==</office:binary-data></draw:image>"#,
        base64_of(&emf)
    );
    let (doc, _) = read_frame(&inline);
    assert_eq!(only(&doc).foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|p| p.bytes.len()), Some(48), "the EMF");
}

/// Base64 of `bytes` (RFC 4648), for inline pictures built here.
fn base64_of(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn odg_shapes_a_text_frame_has_the_rectangle_so_its_style_draws() {
    // The rectangle, so that the frame's fill and line draw.
    let (doc, _) =
        Drawing::a4().page(r#"<draw:frame svg:width="2cm" svg:height="1cm"><draw:text-box><text:p>t</text:p></draw:text-box></draw:frame>"#).read();
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("RelMoveTo"));
    // A picture frame has none.
    let mut d = Drawing::a4().page(r#"<draw:frame svg:width="2cm" svg:height="1cm"><draw:image xlink:href="Pictures/b.png"/></draw:frame>"#);
    d.files.push(("Pictures/b.png".to_owned(), vec![0x89, b'P', b'N', b'G', 0]));
    let (doc, _) = d.read();
    assert_eq!(rows(only(&doc), 0), 0);
}

#[test]
fn shapes_kept_inside_kept_shapes_are_counted_too() {
    // With the one kind still kept whole (a hyperlink wrapper) in place of a group: a wrapper holding a rectangle and a
    // group of a custom shape and a connector, then a custom shape. The five shapes of the wrapper are kept shapes, counted by their kind;
    // the custom shape after it is modelled.
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:a xlink:href="u"><draw:rect/><draw:g><draw:custom-shape/><draw:connector/></draw:g></draw:a><draw:custom-shape/>"#)
        .read();
    assert_eq!(report.index.page(0).map(|p| p.kept().len()), Some(5));
    assert_eq!(page(&doc, 0).shapes.len(), 1);
    assert_eq!(report.stats.kept_shapes, 5);
    assert_eq!(
        (report.kept("hyperlink_wrapper"), report.kept("group"), report.kept("custom_shape"), report.kept("connector"), report.kept("nested_shape")),
        (1, 1, 1, 1, 1)
    );
    // A page's child that is no shape is no kept shape.
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:unknown""#);
    let (_, report) = d.page("<x:other/>").read();
    assert_eq!((report.stats.kept_shapes, report.kept("page_child")), (0, 1));
}
