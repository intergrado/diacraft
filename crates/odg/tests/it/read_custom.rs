//! Custom shapes read from their
//! `draw:enhanced-geometry`, cell by cell. Each test carries the id of the vector it is from.

use diacraft_doc::{CellAddr, Document, RowKey, Shape};

use crate::drawing::{Drawing, cell, close, cm, formula, num, odf, only, page, row_cell, row_type, rows};

/// A custom shape of `w` by `h` cm at (2, 3) cm holding `geometry` (an `enhanced-geometry` element's attributes and children).
fn shape(w: f64, h: f64, geometry: &str) -> String {
    format!(
        r#"<draw:custom-shape svg:x="2cm" svg:y="3cm" svg:width="{w}cm" svg:height="{h}cm"><draw:enhanced-geometry {geometry}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

fn read(shapes: &str) -> (Document, diacraft_odg::ReadReport) {
    Drawing::a4().page(shapes).read()
}

/// A cell of a named row of a section.
fn named<'a>(shape: &'a Shape, section: &str, row: &str, cell_name: &str) -> Option<&'a diacraft_doc::Cell> {
    crate::drawing::cell_at(&shape.sheet, &CellAddr::in_row(section, None, RowKey::name(row), cell_name))
}

#[test]
fn odg_reader_defaults_a_rectangle_path_is_the_rows_of_diacraft_s_own_rectangle() {
    let (doc, _) = read(&shape(3.81, 2.54, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5, "no row for Z: the pen is at the start");
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("MoveTo"));
    // MoveTo with X and Y the constant 0; LineTo (Width, 0), (Width, Height), (0, Height), (0, 0).
    let at = |ix: u32, name: &str| row_cell(s, "Geometry", Some(0), ix, name);
    assert_eq!(formula(at(1, "X")), None);
    close(num(at(1, "X")), 0.0);
    assert_eq!(formula(at(1, "Y")), None);
    assert_eq!((formula(at(2, "X")), formula(at(2, "Y"))), (Some("Width"), None));
    assert_eq!((formula(at(3, "X")), formula(at(3, "Y"))), (Some("Width"), Some("Height")));
    assert_eq!((formula(at(4, "X")), formula(at(4, "Y"))), (None, Some("Height")));
    assert_eq!((formula(at(5, "X")), formula(at(5, "Y"))), (None, None));
    for ix in 2..=5 {
        assert_eq!(row_type(s, 0, ix).as_deref(), Some("LineTo"));
    }
    close(num(at(3, "X")), cm(3.81));
    close(num(at(3, "Y")), cm(2.54));
    // The four connection rows of R15.
    assert_eq!(formula(row_cell(s, "Connection", None, 0, "X")), Some("Width*0.5"));
    assert_eq!(formula(row_cell(s, "Connection", None, 3, "X")), Some("Width"));
}

#[test]
fn odg_reader_defaults_an_ellipse_path_is_one_ellipse_row() {
    let ell = |path: &str| {
        let (doc, _) = read(&shape(4.0, 2.0, &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="{path}">"#)));
        doc
    };
    let doc = ell("U 10800 10800 10800 10800 0 360 N");
    let s = only(&doc);
    assert_eq!(rows(s, 0), 1);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("Ellipse"));
    let at = |name: &str| row_cell(s, "Geometry", Some(0), 1, name);
    assert_eq!(formula(at("X")), Some("Width*0.5"));
    assert_eq!(formula(at("Y")), Some("Height*0.5"));
    assert_eq!(formula(at("A")), Some("Width"));
    assert_eq!(formula(at("B")), Some("Height*0.5"));
    assert_eq!(formula(at("C")), Some("Width*0.5"));
    assert_eq!(formula(at("D")), Some("Height"));
    let doc = ell("U 10800 10800 5400 5400 0 360 N");
    let s = only(&doc);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "A")), Some("Width*0.75"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "D")), Some("Height*0.75"));
    // Half of it is a move and one arc, and no Ellipse row.
    let doc = ell("U 10800 10800 10800 10800 0 180 N");
    let s = only(&doc);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("MoveTo"));
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("EllipticalArcTo"));
    assert_eq!(rows(s, 0), 2);
}

#[test]
fn odg_shapes_a_path_reading_an_equation_and_a_modifier() {
    // `draw:modifiers` 5400, an equation `f0` of `$0*2`, the path M 0 0 L ?f0 0 21600 21600 Z N on a frame 4 cm by 2 cm.
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L ?f0 0 21600 21600 Z N"><draw:equation draw:name="f0" draw:formula="$0*2"/>"#,
    ));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 4);
    let at = |ix: u32, name: &str| row_cell(s, "Geometry", Some(0), ix, name);
    assert_eq!(formula(at(1, "X")), None);
    assert_eq!(formula(at(1, "Y")), Some("Height"));
    assert_eq!(formula(at(2, "X")), Some("Width*User.ODFEq0/21600"));
    assert_eq!(formula(at(2, "Y")), Some("Height"));
    close(num(at(2, "X")), cm(2.0));
    assert_eq!((formula(at(3, "X")), formula(at(3, "Y"))), (Some("Width"), None));
    assert_eq!((formula(at(4, "X")), formula(at(4, "Y"))), (None, Some("Height")));
    close(num(named(s, "User", "ODFMod0", "Value")), 5400.0);
    assert_eq!(formula(named(s, "User", "ODFEq0", "Value")), Some("User.ODFMod0*2"));
    close(num(named(s, "User", "ODFEq0", "Value")), 10800.0);
}

#[test]
fn odg_reader_defaults_modifiers_and_equations_translate_by_the_table() {
    let build = |modifier: &str| {
        let (doc, _) = read(&shape(
            4.0,
            2.0,
            &format!(
                r#"svg:viewBox="0 0 21600 21600" draw:modifiers="{modifier}" draw:enhanced-path="M 0 0 L ?f1 0">
                <draw:equation draw:name="f0" draw:formula="logwidth*0.5"/><draw:equation draw:name="f1" draw:formula="if($0-5000,width,?f0)"/>"#
            ),
        ));
        doc
    };
    let doc = build("3000");
    let s = only(&doc);
    assert_eq!(formula(named(s, "User", "ODFEq0", "Value")), Some("Width/(1 in)*2540*0.5"));
    close(num(named(s, "User", "ODFEq0", "Value")), 2000.0);
    assert_eq!(formula(named(s, "User", "ODFEq1", "Value")), Some("IF((User.ODFMod0-5000)>0,21600,User.ODFEq0)"));
    close(num(named(s, "User", "ODFEq1", "Value")), 2000.0);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "X")), Some("Width*User.ODFEq1/21600"));
    // With the modifier at 6000 instead the test is positive.
    let doc = build("6000");
    close(num(named(only(&doc), "User", "ODFEq1", "Value")), 21600.0);
}

#[test]
fn odg_shapes_w_is_clockwise_as_seen_and_runs_over_the_top() {
    // M 0 10800 W -2700 0 24300 54000 0 10800 21600 10800 N on a frame 2 cm by 1 cm: one arc from (0, 0.5) through (0.5, 1) to (1, 0.5).
    let (doc, _) =
        read(&shape(2.0, 1.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 10800 W -2700 0 24300 54000 0 10800 21600 10800 N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 2, "the move to (0, 0.5) and one arc");
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("EllipticalArcTo"));
    let at = |name: &str| row_cell(s, "Geometry", Some(0), 2, name);
    let (w, h) = (cm(2.0), cm(1.0));
    close(num(at("X")) / w, 1.0);
    close(num(at("Y")) / h, 0.5);
    close(num(at("A")) / w, 0.5);
    close(num(at("B")) / h, 1.0);
    close(num(at("C")), 0.0);
    // D is the half-axes' ratio in the frame: (Width*wR/VW)/(Height*hR/VH) with wR 13500 and hR 27000.
    close(num(at("D")), (w * 13500.0 / 21600.0) / (h * 27000.0 / 21600.0));
    assert_eq!(formula(at("D")), Some("(Width*0.625)/(Height*1.25)"));
}

#[test]
fn odg_shapes_g_swings_clockwise_as_seen_for_a_positive_angle() {
    // M 0 10800 G 5400 5400 180 90 N: a quarter circle centred (5400, 10800) from the pen up to (5400, 5400).
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 10800 G 5400 5400 180 90 N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 2);
    let at = |name: &str| row_cell(s, "Geometry", Some(0), 2, name);
    let w = cm(2.0);
    close(num(at("X")) / w, 0.25);
    close(num(at("Y")) / w, 0.75);
    close(num(at("D")), 1.0);
}

#[test]
fn odg_shapes_the_other_commands() {
    // T: an arc of the ellipse about (10800, 10800), half-axes 10800, from 0 to 90 degrees clockwise as seen (right to bottom); a line from
    // the pen to the start; X and Y quarter ellipses; C and Q rows hold shares.
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 21600 10800 T 10800 10800 10800 10800 0 90 L 0 21600 C 0 0 21600 0 21600 21600 Q 10800 0 0 0 N">"#,
    ));
    let s = only(&doc);
    // The pen is at the arc's start already: no line to it.
    assert_eq!(row_type(s, 0, 2).as_deref(), Some("EllipticalArcTo"));
    let w = cm(2.0);
    close(num(row_cell(s, "Geometry", Some(0), 2, "X")) / w, 0.5);
    close(num(row_cell(s, "Geometry", Some(0), 2, "Y")) / w, 0.0);
    assert_eq!(row_type(s, 0, 3).as_deref(), Some("LineTo"));
    assert_eq!(row_type(s, 0, 4).as_deref(), Some("RelCubBezTo"));
    close(num(row_cell(s, "Geometry", Some(0), 4, "A")), 0.0);
    close(num(row_cell(s, "Geometry", Some(0), 4, "B")), 1.0);
    close(num(row_cell(s, "Geometry", Some(0), 4, "C")), 1.0);
    assert_eq!(row_type(s, 0, 5).as_deref(), Some("RelQuadBezTo"));
    close(num(row_cell(s, "Geometry", Some(0), 5, "A")), 0.5);
}

#[test]
fn odg_shapes_sets_and_their_flags() {
    // F: the set is not filled; S: not stroked; both: not shown. A set with neither has no flag cell; an open subpath of a set that fills has
    // NoFill 0.
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 N M 0 0 L 100 100 F N M 0 0 L 200 200 S N M 0 0 L 300 300 F S N">"#,
    ));
    let s = only(&doc);
    let flag = |geo: u32, name: &str| {
        s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(geo))).and_then(|sec| sec.cells.get(name)).map(|c| num(Some(c)))
    };
    assert_eq!(rows(s, 0), 3);
    assert_eq!((flag(0, "NoFill"), flag(0, "NoLine"), flag(0, "NoShow")), (None, None, None), "an open subpath of a filled set: NoFill 0");
    assert_eq!(flag(1, "NoFill"), Some(1.0));
    assert_eq!(flag(2, "NoLine"), Some(1.0));
    assert_eq!((flag(3, "NoShow"), flag(3, "NoFill"), flag(3, "NoLine")), (Some(1.0), None, None));
}

#[test]
fn odg_reader_defaults_a_modifier_past_the_list_reads_as_zero_with_a_warning() {
    let (doc, report) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:modifiers="100" draw:enhanced-path="M 0 0 L $3 0">"#));
    let s = only(&doc);
    close(num(named(s, "User", "ODFMod3", "Value")), 0.0);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "X")), Some("Width*User.ODFMod3/21600"));
    assert_eq!(report.warned("modifier_missing"), 1);
}

#[test]
fn odg_shapes_an_equation_cycle_is_ref_on_its_members_and_the_shape_still_reads() {
    let (doc, report) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?a 0 ?c 100"><draw:equation draw:name="a" draw:formula="?b+1"/><draw:equation draw:name="b" draw:formula="?a+1"/><draw:equation draw:name="c" draw:formula="7"/>"#,
    ));
    let s = only(&doc);
    let err = |row: &str| named(s, "User", row, "Value").and_then(|c| c.error.as_deref());
    assert_eq!((err("ODFEq0"), err("ODFEq1"), err("ODFEq2")), (Some("#REF!"), Some("#REF!"), None));
    close(num(named(s, "User", "ODFEq2", "Value")), 7.0);
    assert_eq!(report.work.equations, 3);
    assert_eq!(rows(s, 0), 3);
    // The path's cell that reads the cycle is an error cell too; the other is not.
    assert_eq!(row_cell(s, "Geometry", Some(0), 2, "X").and_then(|c| c.error.as_deref()), Some("#REF!"));
    assert!(row_cell(s, "Geometry", Some(0), 3, "X").is_some_and(|c| c.error.is_none()));
}

#[test]
fn odg_shapes_a_syntax_error_in_a_path_draws_the_frame_with_a_warning() {
    let (doc, report) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 L 1 2 3">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5, "The rectangle");
    assert_eq!(report.warned("geometry_unreadable"), 1);
    // The element is kept.
    assert_eq!(odf(s).kept.len(), 1);
    assert_eq!(report.kept("enhanced_geometry"), 1);
    let _ = (page(&doc, 0), cell(s, "Width"));
}

#[test]
fn odg_shapes_a_type_with_no_path_is_a_box_and_an_unknown_type_is_counted_and_kept() {
    let (doc, report) =
        read(r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:type="mso-spt100"/></draw:custom-shape>"#);
    let s = only(&doc);
    assert_eq!(rows(s, 0), 5, "a box");
    assert_eq!(report.kept("shape_type"), 1);
    assert_eq!(odf(s).kept.len(), 1, "the element is kept");
}

#[test]
fn odg_shapes_mirror_attributes_read_as_the_flips() {
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:mirror-horizontal="true" draw:enhanced-path="M 0 0 L 1 1">"#));
    let s = only(&doc);
    close(num(cell(s, "FlipX")), 1.0);
    assert!(cell(s, "FlipY").is_none());
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:mirror-vertical="true" draw:enhanced-path="M 0 0 L 1 1">"#));
    assert!(cell(only(&doc), "FlipX").is_none());
    close(num(cell(only(&doc), "FlipY")), 1.0);
}

#[test]
fn odg_shapes_the_frame_of_a_custom_shape() {
    let (doc, _) = read(&shape(4.0, 2.5, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L 1 1">"#));
    let s = only(&doc);
    close(num(cell(s, "Width")), cm(4.0));
    close(num(cell(s, "PinX")), cm(4.0));
    close(num(cell(s, "PinY")), cm(29.7 - 4.25));
}

#[test]
fn odg_reader_defaults_text_areas_give_the_text_block() {
    let (doc, _) = read(&shape(4.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:text-areas="0 0 21600 10800" draw:enhanced-path="M 0 0 L 1 1">"#));
    let s = only(&doc);
    close(num(cell(s, "TxtHeight")), cm(1.0));
    close(num(cell(s, "TxtWidth")), cm(4.0));
    close(num(cell(s, "TxtPinY")), cm(1.5));
    close(num(cell(s, "TxtPinX")), cm(2.0));
}

/// A custom shape of `ty` with no path, `w` by `h` cm, with modifiers.
fn typed(ty: &str, w: f64, h: f64, modifiers: &str) -> String {
    format!(
        r#"<draw:custom-shape svg:width="{w}cm" svg:height="{h}cm"><draw:enhanced-geometry draw:type="{ty}" draw:modifiers="{modifiers}"/></draw:custom-shape>"#
    )
}

#[test]
fn odg_shapes_an_ooxml_type_with_no_path_is_drawn_from_its_preset() {
    // `ooxml-rect` on a 3 cm by 2 cm frame is a rectangle filling the frame.
    let (doc, report) = read(&typed("ooxml-rect", 3.0, 2.0, ""));
    let s = only(&doc);
    assert_eq!(report.kept("shape_type_preset"), 1);
    assert_eq!(report.kept("shape_type"), 0);
    assert_eq!(rows(s, 0), 5);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("RelMoveTo"));
    let corners: Vec<(f64, f64)> =
        (1..=4).map(|ix| (num(row_cell(s, "Geometry", Some(0), ix, "X")), num(row_cell(s, "Geometry", Some(0), ix, "Y")))).collect();
    for want in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        assert!(corners.iter().any(|c| (c.0 - want.0).abs() < 1e-9 && (c.1 - want.1).abs() < 1e-9), "{want:?} in {corners:?}");
    }
    assert_eq!(odf(s).kept.len(), 1, "the element is kept as written");
}

#[test]
fn odg_shapes_the_modifiers_are_the_presets_adjust_values_and_read_as_values() {
    // A rounded rectangle: the corner radius is the adjust value (1/100,000 of the shorter side), so a larger value rounds more.
    let (round_small, _) = read(&typed("ooxml-roundRect", 4.0, 2.0, "5000"));
    let (round_large, _) = read(&typed("ooxml-roundRect", 4.0, 2.0, "50000"));
    let first_x = |doc: &Document| num(row_cell(only(doc), "Geometry", Some(0), 1, "X"));
    assert!(first_x(&round_large) > first_x(&round_small), "the path starts further along the top edge");
    // Values, not formulas.
    assert_eq!(formula(row_cell(only(&round_small), "Geometry", Some(0), 1, "X")), None);
    // A frame 4 cm by 2 cm: the share is of the frame, so the corner of a circle is 0.5 of the height along the width's 0.25.
    close(first_x(&round_large), 0.25);
}

#[test]
fn odg_shapes_an_ooxml_name_the_gallery_lacks_and_other_types_are_a_box_and_counted() {
    for ty in ["ooxml-noSuchShape", "ooxml-", "mso-spt1", "round-rectangle", "non-primitive", ""] {
        let (doc, report) = read(&typed(ty, 3.0, 2.0, ""));
        assert_eq!(rows(only(&doc), 0), 5, "{ty:?} is the frame's box");
        assert_eq!(report.kept("shape_type"), 1, "{ty:?}");
    }
}

#[test]
fn odg_shapes_a_hostile_adjust_value_cannot_make_a_preset_cost_more() {
    let started = std::time::Instant::now();
    for name in ["roundRect", "star24", "cloud", "wedgeRoundRectCallout", "gear9", "mathMultiply", "circularArrow", "blockArc", "pie", "chord"] {
        for modifiers in ["1e300 1e300 1e300 1e300", "-1e300 -1e300 -1e300 -1e300", "99999999999 1 1 1", "nan inf 0 0"] {
            let (doc, _) = read(&typed(&format!("ooxml-{name}"), 4.0, 2.0, modifiers));
            let s = only(&doc);
            // Every row is made, with finite numbers, and the work is that of the preset.
            let finite = s
                .sheet
                .sections
                .iter()
                .flat_map(|sec| sec.rows.iter())
                .flat_map(|row| row.cells.values())
                .all(|c| c.value.as_ref().and_then(diacraft_doc::Value::number).is_some_and(f64::is_finite));
            assert!(finite, "{name} {modifiers}");
            assert!(rows(s, 0) < 2_000, "{name} {modifiers}: {}", rows(s, 0));
        }
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
}

#[test]
fn odg_shapes_every_gallery_name_reads_to_finite_rows_inside_a_few_frames() {
    let names = [
        "rect",
        "roundRect",
        "ellipse",
        "triangle",
        "rtTriangle",
        "diamond",
        "parallelogram",
        "trapezoid",
        "pentagon",
        "hexagon",
        "octagon",
        "star4",
        "star5",
        "star8",
        "rightArrow",
        "leftArrow",
        "upArrow",
        "downArrow",
        "leftRightArrow",
        "cloud",
        "heart",
        "lightningBolt",
        "sun",
        "moon",
        "smileyFace",
        "flowChartProcess",
        "flowChartDecision",
        "wedgeRectCallout",
        "wedgeEllipseCallout",
        "can",
        "cube",
        "bevel",
        "donut",
        "noSmoking",
        "blockArc",
        "foldedCorner",
        "frame",
        "halfFrame",
        "corner",
        "plus",
        "mathPlus",
        "mathEqual",
        "chevron",
        "homePlate",
        "ribbon",
        "wave",
        "doubleWave",
        "line",
        "straightConnector1",
    ];
    let mut present = 0;
    for name in names {
        let (doc, report) = read(&typed(&format!("ooxml-{name}"), 5.0, 3.0, ""));
        let s = only(&doc);
        if report.kept("shape_type_preset") == 1 {
            present += 1;
            let reaches = s
                .sheet
                .sections
                .iter()
                .filter(|sec| sec.name.as_str() == "Geometry")
                .flat_map(|sec| sec.rows.iter())
                .flat_map(|row| row.cells.values())
                .filter_map(|c| c.value.as_ref().and_then(diacraft_doc::Value::number))
                .all(|v| v.is_finite() && (-1.0..=2.0).contains(&v));
            assert!(reaches, "{name} has a point far outside its frame");
        } else {
            assert_eq!(rows(s, 0), 5, "{name} not in the gallery is a box");
        }
    }
    assert!(present >= 40, "only {present} of the names read from the gallery");
}

#[test]
fn odg_reader_defaults_hasfill_and_hasstroke_read_the_shape_s_patterns() {
    let eq = r#"<draw:equation draw:name="f0" draw:formula="hasfill*10+hasstroke"/>"#;
    let geometry = format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?f0 0">{eq}"#);
    let shape = |style: &str| {
        format!(
            r#"<draw:custom-shape draw:style-name="{style}" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry {geometry}</draw:enhanced-geometry></draw:custom-shape>"#
        )
    };
    let styles = || {
        Drawing::a4()
        .common(r#"<style:style style:name="NoFill" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style><style:style style:name="Child" style:family="graphic" style:parent-style-name="NoFill"><style:graphic-properties draw:stroke="none"/></style:style>"#)
        .auto(r#"<style:style style:name="own" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:stroke="none"/></style:style><style:style style:name="own2" style:family="graphic" style:parent-style-name="NoFill"><style:graphic-properties draw:stroke="solid"/></style:style>"#)
    };
    let value = |style: &str| {
        let (doc, _) = styles().page(&shape(style)).read();
        num(named(only(&doc), "User", "ODFEq0", "Value"))
    };
    // The root draws a fill and a line: both are 1.
    close(value(""), 11.0);
    // An automatic style's own cells, a common style's, and a common parent's through its child's links.
    close(value("own"), 10.0);
    close(value("NoFill"), 1.0);
    close(value("Child"), 0.0);
    close(value("own2"), 1.0);
}

/// The rows of section 0 as (type, X, Y) in shares of a square frame of `side` cm.
fn shares(s: &Shape, side: f64) -> Vec<(String, f64, f64)> {
    (1..=rows(s, 0) as u32)
        .map(|ix| {
            (
                row_type(s, 0, ix).unwrap_or_default(),
                num(row_cell(s, "Geometry", Some(0), ix, "X")) / cm(side),
                num(row_cell(s, "Geometry", Some(0), ix, "Y")) / cm(side),
            )
        })
        .collect()
}

#[test]
fn odg_shapes_quarter_ellipses_alternate_and_the_arc_commands_choose_their_direction() {
    // X from the pen (0, 10800) to (10800, 0): tangent to the x axis first, so the centre is (0, 0); then Y alternates to tangent to the y axis.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 10800 X 10800 0 21600 10800 N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 3);
    let r = shares(s, 2.0);
    assert!((r[1].1 - 0.5).abs() < 1e-9 && (r[1].2 - 1.0).abs() < 1e-9, "{r:?}");
    assert!((r[2].1 - 1.0).abs() < 1e-9 && (r[2].2 - 0.5).abs() < 1e-9, "{r:?}");
    // The second quarter is the other way up: its middle point, on the ray at 45 degrees between the ends from the centre (10800, 10800).
    let mid = (num(row_cell(s, "Geometry", Some(0), 3, "A")) / cm(2.0), num(row_cell(s, "Geometry", Some(0), 3, "B")) / cm(2.0));
    assert!(mid.0 > 0.5 && mid.1 > 0.5, "{mid:?}");
    // A runs counterclockwise as seen and W clockwise, between the same rays: the two arcs are the two ways round a circle.
    let arc = |command: &str| {
        let (doc, _) = read(&shape(
            2.0,
            2.0,
            &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 21600 10800 {command} 0 0 21600 21600 21600 10800 10800 0 N">"#),
        ));
        let s = only(&doc);
        (num(row_cell(s, "Geometry", Some(0), 2, "A")) / cm(2.0), num(row_cell(s, "Geometry", Some(0), 2, "B")) / cm(2.0))
    };
    let (w, a) = (arc("W"), arc("A"));
    // From the right-middle to the top-middle: clockwise as seen is down and round the long way (two pieces, the first's middle point at the
    // lower right), counterclockwise the short way up (one piece, its middle at the upper right). A share's y is the frame's, y up.
    assert!(w.0 > 0.5 && w.1 < 0.5, "W {w:?}");
    assert!(a.0 > 0.5 && a.1 > 0.5, "A {a:?}");
}

#[test]
fn odg_shapes_the_moving_commands_start_a_section_and_g_swings_either_way() {
    // B and V move to the start and begin a section of their own (a section each, and the move first); a negative swing of G is counterclockwise as seen.
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 B 0 0 21600 21600 21600 10800 10800 0 V 0 0 21600 21600 21600 10800 10800 0 M 5400 10800 G 5400 5400 180 -90 N">"#,
    ));
    let s = only(&doc);
    let first = |geo: u32| row_type(s, geo, 1).unwrap_or_default();
    assert_eq!(first(0), "MoveTo");
    assert!(
        s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(3))).is_some(),
        "the lone M, B, V and M make four sections: {}",
        rows(s, 0)
    );
    // G from the pen (5400, 10800) at angle 180 sweeping -90: the centre is (10800, 10800) and the arc ends at (10800, 16200), below it.
    let end = (num(row_cell(s, "Geometry", Some(3), 2, "X")) / cm(2.0), num(row_cell(s, "Geometry", Some(3), 2, "Y")) / cm(2.0));
    assert!((end.0 - 0.5).abs() < 1e-9 && (end.1 - 0.25).abs() < 1e-9, "{end:?}");
}

#[test]
fn odg_shapes_an_arc_of_a_full_turn_is_two_pieces_and_a_longer_one_is_a_full_turn_too() {
    let count = |t1: &str| {
        let (doc, _) = read(&shape(
            2.0,
            2.0,
            &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 21600 10800 T 10800 10800 10800 10800 0 {t1} N">"#),
        ));
        let s = only(&doc);
        (1..=rows(s, 0) as u32).filter(|ix| row_type(s, 0, *ix).as_deref() == Some("EllipticalArcTo")).count()
    };
    assert_eq!(count("360"), 2, "equal rays 360 apart draw the whole ellipse in two halves");
    assert_eq!(count("90"), 1);
    assert_eq!(count("180"), 1);
    assert_eq!(count("270"), 2);
    assert_eq!(count("0"), 0, "equal rays draw nothing");
    assert_eq!(count("720"), 0, "rays 720 apart are equal modulo 360 and only 360 apart draws all");
}

#[test]
fn odg_shapes_an_arc_whose_parameters_are_formulas_reads_the_same_numbers_through_its_cells() {
    // The same quarter arc from numbers and from equations that hold them: the same values in every cell, and formulas that read the `User` rows.
    let numbers = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 21600 10800 T 10800 10800 10800 10800 0 90 N">"#)).0;
    let formulas = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 21600 10800 T ?a ?a ?r ?r 0 ?t N"><draw:equation draw:name="a" draw:formula="10800"/><draw:equation draw:name="r" draw:formula="10800"/><draw:equation draw:name="t" draw:formula="90"/>"#,
    ))
    .0;
    let (n, f) = (only(&numbers), only(&formulas));
    for name in ["X", "Y", "A", "B", "C", "D"] {
        close(num(row_cell(f, "Geometry", Some(0), 2, name)), num(row_cell(n, "Geometry", Some(0), 2, name)));
    }
    let x = formula(row_cell(f, "Geometry", Some(0), 2, "X")).expect("a formula");
    assert!(x.contains("COS(") && x.contains("User.ODFEq"), "{x}");
    assert!(
        formula(row_cell(n, "Geometry", Some(0), 2, "X")).is_none()
            || !formula(row_cell(n, "Geometry", Some(0), 2, "X")).unwrap_or("").contains("COS(")
    );
    // The ratio's formula reads the half-axes through the equations.
    assert!(formula(row_cell(f, "Geometry", Some(0), 2, "D")).is_some_and(|d| d.contains("User.ODFEq")));
    // A full `U` through equations is one Ellipse row all the same.
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U ?c ?c ?c ?c 0 360 N"><draw:equation draw:name="c" draw:formula="10800"/>"#,
    ));
    let s = only(&doc);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("Ellipse"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "X")), Some("Width*User.ODFEq0/21600"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "D")), Some("Height*(1-(User.ODFEq0-User.ODFEq0)/21600)"));
}

#[test]
fn odg_reader_defaults_a_handle_on_the_y_axis_and_one_at_a_constant() {
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="5400 16200" draw:enhanced-path="M 0 0 L $0 $1"><draw:handle draw:handle-position-x="right" draw:handle-position-y="$1"/><draw:handle draw:handle-position-x="10800" draw:handle-position-y="$0"/>"#,
    ));
    let s = only(&doc);
    close(num(named(s, "Control", "Row_1", "X")), cm(4.0));
    close(num(named(s, "Control", "Row_1", "Y")), cm(2.0) * (1.0 - 16200.0 / 21600.0));
    assert_eq!(formula(named(s, "User", "ODFMod1", "Value")), Some("(1-Controls.Row_1.Y/Height)*21600"));
    close(num(named(s, "User", "ODFMod1", "Value")), 16200.0);
    // The second handle's x is a constant, its y drives modifier 0.
    close(num(named(s, "Control", "Row_2", "X")), cm(2.0));
    assert_eq!(formula(named(s, "User", "ODFMod0", "Value")), Some("(1-Controls.Row_2.Y/Height)*21600"));
    // The 1.2 and 1.3 spelling: one attribute with both.
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position="$0 top"/>"#,
    ));
    close(num(named(only(&doc), "Control", "Row_1", "X")), cm(1.0));
    // A view box with an origin: the formula adds it.
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="100 200 1000 1000" draw:modifiers="300" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/>"#,
    ));
    let s = only(&doc);
    assert_eq!(formula(named(s, "User", "ODFMod0", "Value")), Some("Controls.Row_1.X/Width*1000+100"));
    close(num(named(s, "Control", "Row_1", "X")), cm(4.0) * 0.2);
}

#[test]
fn odg_shapes_the_text_angle_and_a_text_area_that_is_an_error() {
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:text-rotate-angle="30" draw:text-areas="0 0 ?nope 100" draw:enhanced-path="M 0 0 L 1 1">"#,
    ));
    let s = only(&doc);
    close(num(cell(s, "TxtAngle")), 30f64.to_radians());
    assert_eq!(crate::drawing::unit(cell(s, "TxtAngle")), Some("DEG"));
    assert!(cell(s, "TxtWidth").is_none(), "a text area that cannot be read leaves the text block to the frame");
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:text-rotate-angle="0" draw:enhanced-path="M 0 0 L 1 1">"#));
    assert!(cell(only(&doc), "TxtAngle").is_none());
}

#[test]
fn odg_shapes_a_view_box_that_is_missing_or_empty() {
    let (doc, report) = read(
        r#"<draw:custom-shape svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry draw:enhanced-path="M 0 0 L 21600 21600"/></draw:custom-shape>"#,
    );
    assert_eq!(report.warned("view_box_unreadable"), 1);
    close(num(row_cell(only(&doc), "Geometry", Some(0), 2, "X")), cm(2.0));
    let (doc, report) = read(
        r#"<draw:custom-shape svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 0 100" draw:enhanced-path="M 0 0 L 1 1"/></draw:custom-shape>"#,
    );
    assert_eq!((report.warned("view_box_empty"), rows(only(&doc), 0)), (1, 5), "the frame's box");
}

#[test]
fn odg_shapes_every_function_and_the_names_of_the_view_box_in_a_path() {
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 1000 1000" draw:enhanced-path="M left top L right bottom L width height L ?e0 ?e1 N"><draw:equation draw:name="e0" draw:formula="min(max(abs(0-300), 200), sqrt(250000))"/><draw:equation draw:name="e1" draw:formula="if(1, atan(1)*4*100+sin(0)+cos(0)+tan(0), 0)"/>"#,
    ));
    let s = only(&doc);
    let w = cm(2.0);
    close(num(row_cell(s, "Geometry", Some(0), 2, "X")) / w, 1.0);
    close(num(row_cell(s, "Geometry", Some(0), 2, "Y")) / w, 0.0);
    close(num(row_cell(s, "Geometry", Some(0), 4, "X")) / w, 0.3);
    close(num(row_cell(s, "Geometry", Some(0), 4, "Y")) / w, 1.0 - (std::f64::consts::PI * 100.0 + 1.0) / 1000.0);
}

#[test]
fn odg_shapes_a_rounded_rectangle_built_of_quarter_ellipses_and_equations() {
    // The kind of path a producer writes for a rounded rectangle: quarter ellipses (`X` then `Y` alternating) between lines, the radius a
    // modifier through three equations, 3600 of 21600 on a square frame of 4 cm.
    let (doc, report) = read(&shape(
        4.0,
        4.0,
        concat!(
            r#"svg:viewBox="0 0 21600 21600" draw:modifiers="3600" draw:enhanced-path="M ?f0 0 X 0 ?f0 L 0 ?f1 Y ?f0 bottom L ?f2 bottom X right ?f1 L right ?f0 Y ?f2 0 Z N">"#,
            r#"<draw:equation draw:name="f0" draw:formula="$0"/><draw:equation draw:name="f1" draw:formula="bottom-?f0"/><draw:equation draw:name="f2" draw:formula="right-?f0"/>"#,
        ),
    ));
    let s = only(&doc);
    assert_eq!(report.warnings.len(), 0, "{:?}", report.warnings);
    let kinds: Vec<String> = (1..=rows(s, 0) as u32).map(|ix| row_type(s, 0, ix).unwrap_or_default()).collect();
    assert_eq!(kinds, ["MoveTo", "EllipticalArcTo", "LineTo", "EllipticalArcTo", "LineTo", "EllipticalArcTo", "LineTo", "EllipticalArcTo", "LineTo"]);
    let side = cm(4.0);
    let at = |ix: u32, name: &str| num(row_cell(s, "Geometry", Some(0), ix, name)) / side;
    let r = 3600.0 / 21600.0;
    // The start (r, top), the first arc to (left, r) from a centre (r, r): its middle point is on the circle between them.
    close(at(1, "X"), r);
    close(at(1, "Y"), 1.0);
    close(at(2, "X"), 0.0);
    close(at(2, "Y"), 1.0 - r);
    let (mx, my) = (at(2, "A") - r, at(2, "B") - (1.0 - r));
    close(mx.hypot(my), r);
    assert!(mx < 0.0 && my > 0.0, "the corner's middle point is up and to the left of the centre: {mx} {my}");
    close(at(4, "X"), r);
    close(at(4, "Y"), 0.0);
    close(at(6, "X"), 1.0);
    close(at(6, "Y"), r);
    close(at(8, "X"), 1.0 - r);
    close(at(8, "Y"), 1.0);
    // Z closes with a line to the start.
    close(at(9, "X"), r);
    close(at(9, "Y"), 1.0);
    // Every arc is a circle's: C 0 and D 1 (equal half-axes).
    for ix in [2, 4, 6, 8] {
        close(num(row_cell(s, "Geometry", Some(0), ix, "C")), 0.0);
        close(num(row_cell(s, "Geometry", Some(0), ix, "D")), 1.0);
    }
    // The radius reads the modifier through the equations: a handle-less change of `$0` would move every coordinate that names an equation.
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 1, "X")), Some("Width*User.ODFEq0/21600"));
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 3, "Y")), Some("Height*(1-User.ODFEq1/21600)"));
}

#[test]
fn odg_shapes_only_a_whole_turn_of_u_is_an_ellipse_row() {
    let types = |sweep: &str| {
        let (doc, _) =
            read(&shape(2.0, 2.0, &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 10800 10800 0 {sweep} N">"#)));
        let s = only(&doc);
        (1..=rows(s, 0) as u32).map(|ix| row_type(s, 0, ix).unwrap_or_default()).collect::<Vec<_>>()
    };
    assert_eq!(types("360"), ["Ellipse"]);
    assert_eq!(types("270"), ["MoveTo", "EllipticalArcTo", "EllipticalArcTo"], "three quarters is two arcs, not an ellipse");
    assert_eq!(types("180"), ["MoveTo", "EllipticalArcTo"]);
    // A turn past the whole is the whole (U, as T's "360 or more" is cut), and a whole turn with something after it is not alone.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 10800 10800 0 360 L 0 0 N">"#));
    let s = only(&doc);
    assert_eq!(row_type(s, 0, 1).as_deref(), Some("MoveTo"));
    assert_eq!(rows(s, 0), 4, "a move, two halves and the line");
    // `Z` after a whole turn adds no row: the pen is at the start.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 10800 10800 0 360 Z N">"#));
    assert_eq!(row_type(only(&doc), 0, 1).as_deref(), Some("Ellipse"));
}

#[test]
fn odg_shapes_a_parameter_that_is_no_number_name_or_expression_breaks_the_path() {
    for path in ["M 0 0 L ?nope 1", "M 0 0 L 1x 1", "M 0 0 L foo 1", "M 0 0 L 1+ 1", "M 0 0 L (1 1"] {
        let (doc, report) = read(&shape(2.0, 2.0, &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="{path}">"#)));
        assert_eq!((rows(only(&doc), 0), report.warned("geometry_unreadable")), (5, 1), "{path}: the frame's box");
    }
    // An expression of the grammar is a parameter, as an equation's value is; pairs after an M are lines.
    let (doc, report) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 21600 0 width+0 height N">"#));
    let s = only(&doc);
    assert_eq!(report.warned("geometry_unreadable"), 0);
    assert_eq!(
        (row_type(s, 0, 1).as_deref(), row_type(s, 0, 2).as_deref(), row_type(s, 0, 3).as_deref()),
        (Some("MoveTo"), Some("LineTo"), Some("LineTo"))
    );
    assert_eq!(
        formula(row_cell(s, "Geometry", Some(0), 3, "X")),
        Some("Width*(21600+0)/21600"),
        "an expression is a formula even when it is a number's sum"
    );
}

#[test]
fn odg_reader_defaults_every_length_cell_of_a_custom_shape_is_shown_in_the_frame_s_unit() {
    // A frame in mm: the absolute cells, the arcs, the ellipse, the glue rows, the controls and the text block are shown in mm; the shares
    // of the `Rel` rows and the ratio `D` have no unit.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="40mm" svg:height="20mm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:glue-points="100 200" draw:text-areas="0 0 21600 10800" draw:enhanced-path="M 0 0 L 100 100 C 1 2 3 4 5 6 T 10800 10800 100 100 0 90 N U 10800 10800 5400 5400 0 360 N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    let unit = |section: &str, geo: Option<u32>, ix: u32, name: &str| crate::drawing::unit(row_cell(s, section, geo, ix, name)).map(str::to_owned);
    for (ix, name) in [(1, "X"), (1, "Y"), (2, "X"), (2, "Y")] {
        assert_eq!(unit("Geometry", Some(0), ix, name).as_deref(), Some("MM"), "{ix} {name}");
    }
    // The cubic's cells are shares.
    assert_eq!(unit("Geometry", Some(0), 3, "X"), None);
    // The arc: its end and middle in mm, its ratio none.
    assert_eq!(unit("Geometry", Some(0), 5, "X").as_deref(), Some("MM"));
    assert_eq!(unit("Geometry", Some(0), 5, "B").as_deref(), Some("MM"));
    assert_eq!(unit("Geometry", Some(0), 5, "D"), None);
    for name in ["X", "Y", "A", "B", "C", "D"] {
        assert_eq!(unit("Geometry", Some(1), 1, name).as_deref(), Some("MM"), "the ellipse's {name}");
    }
    assert_eq!(unit("Connection", None, 4, "X").as_deref(), Some("MM"));
    assert_eq!(unit("Connection", None, 4, "Y").as_deref(), Some("MM"));
    let named_unit = |section: &str, row: &str, name: &str| crate::drawing::unit(named(s, section, row, name)).map(str::to_owned);
    assert_eq!(named_unit("Control", "Row_1", "X").as_deref(), Some("MM"));
    assert_eq!(named_unit("Control", "Row_1", "YDyn").as_deref(), Some("MM"));
    assert_eq!(named_unit("User", "ODFMod0", "Value"), None);
    assert_eq!(crate::drawing::unit(cell(s, "TxtWidth")).map(str::to_owned).as_deref(), Some("MM"));
    assert_eq!(crate::drawing::unit(cell(s, "TxtPinY")).map(str::to_owned).as_deref(), Some("MM"));
    // The same shape in inches shows inches.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="2in" svg:height="1in"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L 100 100"/></draw:custom-shape>"#,
    );
    assert_eq!(crate::drawing::unit(row_cell(only(&doc), "Geometry", Some(0), 2, "X")), Some("IN"));
}

#[test]
fn odg_shapes_equation_names_may_hold_underscores_and_dots() {
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?my_eq.1 ?_x"><draw:equation draw:name="my_eq.1" draw:formula="10800"/><draw:equation draw:name="_x" draw:formula="?my_eq.1+?my_eq.1"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    assert_eq!(formula(named(s, "User", "ODFEq1", "Value")), Some("User.ODFEq0+User.ODFEq0"));
    close(num(row_cell(s, "Geometry", Some(0), 2, "X")), cm(1.0));
    close(num(row_cell(s, "Geometry", Some(0), 2, "Y")), cm(0.0));
}

/// The row types of section 0 of a custom shape of `path` on a 2 cm square frame.
fn types(path: &str) -> Vec<String> {
    let (doc, _) = read(&shape(2.0, 2.0, &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="{path}">"#)));
    let s = only(&doc);
    (1..=rows(s, 0) as u32).map(|ix| row_type(s, 0, ix).unwrap_or_default()).collect()
}

#[test]
fn odg_shapes_an_arc_of_no_radius_or_no_sweep_draws_nothing_and_a_longer_one_is_cut_at_half_a_turn() {
    // A half-axis of 0, a box of no size and a sweep of 0 make no arc and no line to its start.
    assert_eq!(types("M 0 0 T 10800 10800 0 5400 0 90 L 10 10 N"), ["MoveTo", "LineTo"]);
    assert_eq!(types("M 0 0 T 10800 10800 5400 0 0 90 L 10 10 N"), ["MoveTo", "LineTo"]);
    assert_eq!(types("M 0 0 W 5 5 5 5 5 5 9 9 L 10 10 N"), ["MoveTo", "LineTo"]);
    assert_eq!(types("M 0 0 T 10800 10800 5400 5400 20 20 L 10 10 N"), ["MoveTo", "LineTo"]);
    // `U` of no sweep still moves to the start and starts a section of its own.
    assert_eq!(types("M 0 0 L 5 5 U 10800 10800 5400 5400 0 0 N"), ["MoveTo", "LineTo"]);
    // Pieces of at most half a turn: 180 is one, 181 is two, 359 is two.
    let arcs = |sweep: &str| types(&format!("M 16200 10800 T 10800 10800 5400 5400 0 {sweep} N")).iter().filter(|t| *t == "EllipticalArcTo").count();
    assert_eq!((arcs("179"), arcs("180"), arcs("181"), arcs("359")), (1, 1, 2, 2));
}

#[test]
fn odg_shapes_an_arc_starts_with_a_line_from_the_pen_when_the_pen_is_not_at_its_start() {
    // The pen at (0, 0); the arc starts at the right-middle of the circle about (10800, 10800): a line to it first.
    assert_eq!(types("M 0 0 T 10800 10800 5400 5400 0 90 N"), ["MoveTo", "LineTo", "EllipticalArcTo"]);
    // The pen already there: none. `U` moves instead of drawing the line.
    assert_eq!(types("M 16200 10800 T 10800 10800 5400 5400 0 90 N"), ["MoveTo", "EllipticalArcTo"]);
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 U 10800 10800 5400 5400 0 90 N">"#));
    let s = only(&doc);
    assert_eq!((rows(s, 0), rows(s, 1)), (1, 2), "the move alone, then a section of its own with its move and the arc");
    assert_eq!(row_type(s, 1, 2).as_deref(), Some("EllipticalArcTo"));
}

#[test]
fn odg_shapes_an_arc_between_rays_that_are_not_at_zero_and_a_ratio_of_unequal_half_axes() {
    // T from 30 to 120 degrees is a sweep of 90, and from 270 to 90 one of 180: a half turn, one piece.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 5400 5400 30 120 N">"#));
    let s = only(&doc);
    let w = cm(2.0);
    // The end is on the ray at 120 degrees (clockwise as seen, so below and to the left of the centre), the middle on the ray at 75.
    let end = (num(row_cell(s, "Geometry", Some(0), 2, "X")) / w, num(row_cell(s, "Geometry", Some(0), 2, "Y")) / w);
    close(end.0, 0.5 + 0.25 * 120f64.to_radians().cos());
    close(end.1, 0.5 - 0.25 * 120f64.to_radians().sin());
    let mid = (num(row_cell(s, "Geometry", Some(0), 2, "A")) / w, num(row_cell(s, "Geometry", Some(0), 2, "B")) / w);
    close(mid.0, 0.5 + 0.25 * 75f64.to_radians().cos());
    close(mid.1, 0.5 - 0.25 * 75f64.to_radians().sin());
    assert_eq!(rows(s, 0), 2);
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 5400 5400 270 90 N">"#));
    assert_eq!(rows(only(&doc), 0), 2, "270 to 90 is a half turn, one arc");
    // Unequal half-axes on a 4 cm by 2 cm frame: the ratio of the frame's half-axes, (4 * 10800 / 21600) / (2 * 5400 / 21600) = 4.
    let (doc, _) = read(&shape(4.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 10800 5400 0 90 N">"#));
    let s = only(&doc);
    close(num(row_cell(s, "Geometry", Some(0), 2, "D")), 4.0);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "D")), Some("(Width*0.5)/(Height*0.25)"));
    // A half-axis that is not a whole share of the view box is rounded to nine places in the text and not in the value.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 7200 5400 0 90 N">"#));
    let s = only(&doc);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "D")), Some("(Width*0.333333333)/(Height*0.25)"));
    close(num(row_cell(s, "Geometry", Some(0), 2, "D")), (1.0 / 3.0) / 0.25);
}

#[test]
fn odg_shapes_quarter_ellipses_turn_the_short_way_either_way_round() {
    // X from (10800, 0) to (21600, 10800): about the centre (10800, 10800), from the top clockwise as seen to the right: the middle is up and right.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 10800 0 X 21600 10800 N">"#));
    let s = only(&doc);
    let (mx, my) = (num(row_cell(s, "Geometry", Some(0), 2, "A")) / cm(2.0), num(row_cell(s, "Geometry", Some(0), 2, "B")) / cm(2.0));
    close(mx, 0.5 + 0.5 * 45f64.to_radians().cos());
    close(my, 0.5 + 0.5 * 45f64.to_radians().sin());
    // Y from (0, 10800) to (10800, 0): about the centre (10800, 10800), counterclockwise as seen: the middle is up and left.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 10800 Y 10800 0 N">"#));
    let s = only(&doc);
    let (mx, my) = (num(row_cell(s, "Geometry", Some(0), 2, "A")) / cm(2.0), num(row_cell(s, "Geometry", Some(0), 2, "B")) / cm(2.0));
    close(mx, 0.5 - 0.5 * 45f64.to_radians().cos());
    close(my, 0.5 + 0.5 * 45f64.to_radians().sin());
    // The ratio of an unequal pair of half-axes: from (0, 0) to (21600, 10800), 4 cm by 2 cm: half-axes 21600 and 10800: 1 * 4/2 / 1 ... (4 * 1) / (2 * 0.5).
    let (doc, _) = read(&shape(4.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 X 21600 10800 N">"#));
    close(num(row_cell(only(&doc), "Geometry", Some(0), 2, "D")), (4.0 * 1.0) / (2.0 * 0.5));
}

#[test]
fn odg_shapes_a_box_arc_with_equal_rays_draws_nothing_and_g_needs_a_pen() {
    assert_eq!(types("M 0 0 A 0 0 21600 21600 21600 10800 21600 10800 L 10 10 N"), ["MoveTo", "LineTo"]);
    // G with no `M` before it starts at the origin: a move there, then the arc.
    let t = types("G 5400 5400 180 90 N");
    assert_eq!(t, ["MoveTo", "EllipticalArcTo"]);
}

#[test]
fn odg_shapes_a_drawing_command_before_any_move_starts_a_section_at_the_origin() {
    assert_eq!(types("L 10800 10800 N"), ["MoveTo", "LineTo"]);
    assert_eq!(types("C 1 2 3 4 5 6 N"), ["MoveTo", "RelCubBezTo"]);
    assert_eq!(types("Q 1 2 3 4 N"), ["MoveTo", "RelQuadBezTo"]);
    assert_eq!(types("Z N"), Vec::<String>::new(), "a close with nothing to close");
    assert_eq!(types("N"), Vec::<String>::new());
    // Pairs after an `M` are lines, and a command repeats while parameters remain: three `C` groups, two `Q` groups.
    assert_eq!(types("M 0 0 10 10 20 20 N"), ["MoveTo", "LineTo", "LineTo"]);
    assert_eq!(
        types("M 0 0 C 1 2 3 4 5 6 1 2 3 4 5 6 1 2 3 4 5 6 Q 1 2 3 4 5 6 7 8 N"),
        ["MoveTo", "RelCubBezTo", "RelCubBezTo", "RelCubBezTo", "RelQuadBezTo", "RelQuadBezTo"]
    );
    // `X` and `Y` alternate across repeated groups.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 10800 X 10800 0 21600 10800 10800 21600 N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 4);
    close(num(row_cell(s, "Geometry", Some(0), 4, "X")) / cm(2.0), 0.5);
    close(num(row_cell(s, "Geometry", Some(0), 4, "Y")) / cm(2.0), 0.0);
}

#[test]
fn odg_shapes_a_text_area_in_a_view_box_with_an_origin() {
    // View box (100, 200, 1000, 1000) on a 4 cm by 2 cm frame; the area (100, 200, 600, 700) is the top-left quarter of it.
    let (doc, _) = read(&shape(4.0, 2.0, r#"svg:viewBox="100 200 1000 1000" draw:text-areas="100 200 600 700" draw:enhanced-path="M 0 0 L 1 1">"#));
    let s = only(&doc);
    close(num(cell(s, "TxtWidth")), cm(2.0));
    close(num(cell(s, "TxtHeight")), cm(1.0));
    close(num(cell(s, "TxtPinX")), cm(1.0));
    close(num(cell(s, "TxtPinY")), cm(1.5));
    assert_eq!(formula(cell(s, "TxtWidth")), Some("Width*((600)-(100))/1000"));
    assert_eq!(formula(cell(s, "TxtPinX")), Some("Width*(((100)+(600))/2-100)/1000"));
    assert_eq!(formula(cell(s, "TxtPinY")), Some("Height*(1-(((200)+(700))/2-200)/1000)"));
    // The rectangle's corners may be expressions.
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="10800" draw:text-areas="0 0 $0 ?f0" draw:enhanced-path="M 0 0 L 1 1"><draw:equation draw:name="f0" draw:formula="$0/2"/>"#,
    ));
    close(num(cell(only(&doc), "TxtWidth")), cm(1.0));
    close(num(cell(only(&doc), "TxtHeight")), cm(0.5));
}

#[test]
fn odg_shapes_logheight_and_the_stretch_points_read_the_frame_and_the_attributes() {
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:path-stretchpoint-x="300" draw:path-stretchpoint-y="700" draw:enhanced-path="M 0 0 L ?a ?b"><draw:equation draw:name="a" draw:formula="logheight+xstretch"/><draw:equation draw:name="b" draw:formula="ystretch*2"/>"#,
    ));
    let s = only(&doc);
    close(num(named(s, "User", "ODFEq0", "Value")), 2.0 / 2.54 * 2540.0 + 300.0);
    assert_eq!(formula(named(s, "User", "ODFEq0", "Value")), Some("Height/(1 in)*2540+300"));
    close(num(named(s, "User", "ODFEq1", "Value")), 1400.0);
    assert_eq!(formula(named(s, "User", "ODFEq1", "Value")), Some("700*2"));
}

#[test]
fn odg_shapes_listed_glue_points_that_are_formulas_and_the_separators_of_their_directions() {
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:glue-points="?f0 $0 21600 0 0 21600 100 100" draw:glue-point-leaving-directions="0;90,180 270" draw:enhanced-path="M 0 0 L 1 1"><draw:equation draw:name="f0" draw:formula="$0*2"/>"#,
    ));
    let s = only(&doc);
    assert_eq!(formula(row_cell(s, "Connection", None, 4, "X")), Some("Width*User.ODFEq0/21600"));
    assert_eq!(formula(row_cell(s, "Connection", None, 4, "Y")), Some("Height*(1-User.ODFMod0/21600)"));
    close(num(row_cell(s, "Connection", None, 4, "X")), cm(2.0));
    close(num(row_cell(s, "Connection", None, 4, "Y")), cm(2.0) * 0.75);
    // Four points, four directions from three kinds of separator: 0, 90, 180 and 270 degrees.
    let dir = |ix: u32| (num(row_cell(s, "Connection", None, ix, "DirX")), num(row_cell(s, "Connection", None, ix, "DirY")));
    let want = [(1.0, 0.0), (0.0, -1.0), (-1.0, 0.0), (0.0, 1.0)];
    for (i, (x, y)) in want.into_iter().enumerate() {
        let got = dir(4 + i as u32);
        close(got.0, x);
        close(got.1, y);
    }
    // A second point and no second direction: no `DirX`.
    let (doc, _) = read(&shape(
        4.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:glue-points="1 1 2 2" draw:glue-point-leaving-directions="0" draw:enhanced-path="M 0 0 L 1 1">"#,
    ));
    assert!(row_cell(only(&doc), "Connection", None, 5, "DirX").is_none());
    // An odd count of coordinates drops the last.
    let (doc, _) = read(&shape(4.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:glue-points="1 1 2" draw:enhanced-path="M 0 0 L 1 1">"#));
    assert!(row_cell(only(&doc), "Connection", None, 4, "X").is_some() && row_cell(only(&doc), "Connection", None, 5, "X").is_none());
}

#[test]
fn odg_shapes_a_handle_that_is_switched_or_mirrored_is_kept_and_makes_no_row() {
    for attr in ["draw:handle-switched", "draw:handle-mirror-horizontal", "draw:handle-mirror-vertical"] {
        let (doc, report) = read(&shape(
            2.0,
            2.0,
            &format!(
                r#"svg:viewBox="0 0 21600 21600" draw:modifiers="1" draw:enhanced-path="M 0 0 L 1 1"><draw:handle draw:handle-position="$0 top" {attr}="true"/>"#
            ),
        ));
        assert!(only(&doc).sheet.section(&diacraft_doc::SectionKey::new("Control", None)).is_none(), "{attr}");
        assert_eq!(report.kept("handle"), 1, "{attr}");
    }
    // The same attributes false are a row (a range is kept and not editable, R34: `read_formula_costs`).
    let (doc, _) = read(&shape(
        2.0,
        2.0,
        r#"svg:viewBox="0 0 21600 21600" draw:modifiers="1" draw:enhanced-path="M 0 0 L 1 1"><draw:handle draw:handle-position="$0 top" draw:handle-switched="false" draw:handle-mirror-horizontal="false" draw:handle-mirror-vertical="false"/>"#,
    ));
    assert!(named(only(&doc), "Control", "Row_1", "X").is_some());
}

/// How many rows the named section holds.
fn rows_of(s: &Shape, section: &str) -> usize {
    s.sheet.section(&diacraft_doc::SectionKey::new(section, None)).map_or(0, |sec| sec.rows.len())
}

#[test]
fn odg_shapes_modifiers_and_equations_share_a_user_section_of_the_rows_a_section_holds() {
    let custom = |modifiers: usize| {
        Drawing::a4()
            .page(&format!(
                r#"<draw:custom-shape svg:width="1cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="{}" draw:enhanced-path="M 0 0 L 1 1"><draw:equation draw:name="f0" draw:formula="1"/></draw:enhanced-geometry></draw:custom-shape>"#,
                "1 ".repeat(modifiers)
            ))
            .bytes()
    };
    let (doc, _) = diacraft_odg::read(&custom(319_999), &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(rows_of(only(&doc), "User"), 320_000);
    assert_eq!(diacraft_odg::read(&custom(320_000), &diacraft_odg::Limits::default()).err().map(|e| e.class()), Some("xml_too_complex"));
}

#[test]
fn odg_shapes_the_gallery_s_arc_has_a_fill_only_path_and_a_stroke_only_one() {
    let (doc, _) = read(&typed("ooxml-arc", 4.0, 4.0, ""));
    let s = only(&doc);
    let flag = |geo: u32, name: &str| {
        s.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(geo))).and_then(|sec| sec.cells.get(name)).map(|c| num(Some(c)))
    };
    assert_eq!((flag(0, "NoLine"), flag(0, "NoFill")), (Some(1.0), None), "the area is not stroked");
    assert_eq!((flag(1, "NoFill"), flag(1, "NoLine")), (Some(1.0), None), "the outline is not filled");
}

#[test]
fn odg_styles_a_shape_whose_links_differ_reads_hasfill_from_its_fill_link_and_hasstroke_from_its_line_link() {
    // `Ln` draws a line and no fill; `Fl` a fill and no line. The shape's automatic style links its line to `Ln` and its fill to `Fl`.
    let d = Drawing::a4()
        .common(concat!(
            r#"<style:style style:name="Ln" style:family="graphic"><style:graphic-properties draw:fill="none" draw:stroke="solid"/></style:style>"#,
            r#"<style:style style:name="Fl" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:stroke="none"/></style:style>"#
        ))
        .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Ln"><style:graphic-properties diacraft:version="1" diacraft:links="Ln Fl Ln"/></style:style>"#)
        .page(
            r#"<draw:custom-shape draw:style-name="gr1" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?f0 0"><draw:equation draw:name="f0" draw:formula="hasfill*10+hasstroke"/></draw:enhanced-geometry></draw:custom-shape>"#,
        );
    let (doc, _) = d.read();
    // The fill comes from `Fl` (solid: 1) and the line from `Ln` (solid: 1): 11; with the links read the other way round each is 0.
    close(num(named(only(&doc), "User", "ODFEq0", "Value")), 11.0);
}

#[test]
fn odg_reader_defaults_the_width_and_the_height_each_keep_their_own_unit() {
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="40mm" svg:height="1in"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-points="100 200" draw:enhanced-path="M 0 0 L 100 100 U 10800 10800 5400 5400 0 360 N"/></draw:custom-shape>"#,
    );
    let s = only(&doc);
    let unit = |section: &str, geo: Option<u32>, ix: u32, name: &str| crate::drawing::unit(row_cell(s, section, geo, ix, name)).map(str::to_owned);
    assert_eq!((unit("Geometry", Some(0), 2, "X").as_deref(), unit("Geometry", Some(0), 2, "Y").as_deref()), (Some("MM"), Some("IN")));
    // The ellipse: x and y of the centre, A and B the point along x, C and D the point along y: each coordinate in its own axis's unit.
    for (name, want) in [("X", "MM"), ("Y", "IN"), ("A", "MM"), ("B", "IN"), ("C", "MM"), ("D", "IN")] {
        assert_eq!(unit("Geometry", Some(1), 1, name).as_deref(), Some(want), "{name}");
    }
    assert_eq!((unit("Connection", None, 4, "X").as_deref(), unit("Connection", None, 4, "Y").as_deref()), (Some("MM"), Some("IN")));
}

#[test]
fn odg_shapes_the_ratio_of_half_axes_reads_each_axis_of_the_view_box() {
    // A view box 10,800 wide and 21,600 tall: half-axes 2,700 across and 5,400 down are a quarter of the frame each way, so on 4 cm by
    // 2 cm the ratio is (4 * 0.25) / (2 * 0.25) = 2.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 10800 21600" draw:enhanced-path="U 5400 10800 2700 5400 0 90 N"/></draw:custom-shape>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Geometry", Some(0), 2, "D")), 2.0);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "D")), Some("(Width*0.25)/(Height*0.25)"));
    // The same through equations: the half-axes are expressions, each over its own extent.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 10800 21600" draw:enhanced-path="U 5400 10800 ?w ?h 0 90 N"><draw:equation draw:name="w" draw:formula="2700"/><draw:equation draw:name="h" draw:formula="5400"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Geometry", Some(0), 2, "D")), 2.0);
    assert_eq!(formula(row_cell(s, "Geometry", Some(0), 2, "D")), Some("(Width*User.ODFEq0/10800)/(Height*User.ODFEq1/21600)"));
}

#[test]
fn odg_shapes_an_arc_whose_angles_or_radii_are_errors_draws_nothing() {
    // The start angle, the sweep or a half-axis an error: no arc, and no line to its start.
    for params in
        ["T 10800 10800 5400 5400 ?bad 90", "T 10800 10800 5400 5400 0 ?bad", "T 10800 10800 ?bad 5400 0 90", "T 10800 10800 5400 ?bad 0 90"]
    {
        let (doc, _) = read(&shape(
            2.0,
            2.0,
            &format!(
                r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 {params} L 10 10 N"><draw:equation draw:name="bad" draw:formula="1/0"/>"#
            ),
        ));
        let s = only(&doc);
        let kinds: Vec<String> = (1..=rows(s, 0) as u32).map(|ix| row_type(s, 0, ix).unwrap_or_default()).collect();
        assert_eq!(kinds, ["MoveTo", "LineTo"], "{params}");
    }
}

#[test]
fn odg_shapes_each_piece_of_a_long_arc_ends_where_its_share_of_the_sweep_does() {
    // 270 degrees in two pieces of 135: the first ends on the ray at 135, the second on the ray at 270, and the middles at 67.5 and 202.5.
    let (doc, _) = read(&shape(2.0, 2.0, r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="U 10800 10800 10800 10800 0 270 N">"#));
    let s = only(&doc);
    assert_eq!(rows(s, 0), 3);
    let at =
        |ix: u32, x: &str, y: &str| (num(row_cell(s, "Geometry", Some(0), ix, x)) / cm(2.0), num(row_cell(s, "Geometry", Some(0), ix, y)) / cm(2.0));
    let on = |degrees: f64| (0.5 + 0.5 * degrees.to_radians().cos(), 0.5 - 0.5 * degrees.to_radians().sin());
    for (ix, end, mid) in [(2, 135.0, 67.5), (3, 270.0, 202.5)] {
        let (e, m) = (at(ix, "X", "Y"), at(ix, "A", "B"));
        close(e.0, on(end).0);
        close(e.1, on(end).1);
        close(m.0, on(mid).0);
        close(m.1, on(mid).1);
    }
}

#[test]
fn odg_shapes_a_box_arc_the_long_way_round_is_cut_in_halves_and_the_short_way_is_one_piece() {
    // The rays toward (21600, 10800) (0 degrees) and toward the point 10 degrees above it on the circle (350, clockwise as seen): `W` goes
    // the long way, 350 degrees in two pieces; `A` the short way, 10 degrees in one.
    let end = (10800.0 + 10800.0 * 10f64.to_radians().cos(), 10800.0 - 10800.0 * 10f64.to_radians().sin());
    let path = |command: &str| format!("M 21600 10800 {command} 0 0 21600 21600 21600 10800 {} {} N", end.0, end.1);
    // The end of each arc piece as shares of the frame.
    let arcs = |command: &str| -> Vec<(f64, f64)> {
        let (doc, _) = read(&shape(2.0, 2.0, &format!(r#"svg:viewBox="0 0 21600 21600" draw:enhanced-path="{}">"#, path(command))));
        let s = only(&doc);
        (1..=rows(s, 0) as u32)
            .filter(|ix| row_type(s, 0, *ix).as_deref() == Some("EllipticalArcTo"))
            .map(|ix| (num(row_cell(s, "Geometry", Some(0), ix, "X")) / cm(2.0), num(row_cell(s, "Geometry", Some(0), ix, "Y")) / cm(2.0)))
            .collect()
    };
    let (long, short) = (arcs("W"), arcs("A"));
    assert_eq!((long.len(), short.len()), (2, 1));
    // The last piece of either ends on the ray at 350 degrees (10 degrees above the right-middle, so up and to the right of it).
    let want = (0.5 + 0.5 * 10f64.to_radians().cos(), 0.5 + 0.5 * 10f64.to_radians().sin());
    for pieces in [&long, &short] {
        let last = pieces.last().copied().unwrap();
        close(last.0, want.0);
        close(last.1, want.1);
    }
    // The long way's first piece ends half way round: 175 degrees clockwise as seen, below the centre and just left of it.
    close(long[0].0, 0.5 + 0.5 * 175f64.to_radians().cos());
    close(long[0].1, 0.5 - 0.5 * 175f64.to_radians().sin());
}
