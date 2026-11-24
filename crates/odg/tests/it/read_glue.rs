//! Handles, glue points, connectors, groups.
//! Each test carries the id of the vector it is from.

use diacraft_doc::{CellAddr, Document, RowKey, Shape, ShapeType};

use crate::drawing::{Drawing, cell, cell_at, close, cm, formula, num, odf, only, page, row_cell, row_type, rows, shape as nth};

fn read(shapes: &str) -> (Document, diacraft_odg::ReadReport) {
    Drawing::a4().page(shapes).read()
}

fn named<'a>(shape: &'a Shape, section: &str, row: &str, cell_name: &str) -> Option<&'a diacraft_doc::Cell> {
    cell_at(&shape.sheet, &CellAddr::in_row(section, None, RowKey::name(row), cell_name))
}

#[test]
fn odg_reader_defaults_a_handle_is_a_controls_row_and_drives_its_modifier() {
    // A handle at `$0` and `top` with modifier 5400 on a 21600 view box and a frame 4 cm wide.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    close(num(named(s, "Control", "Row_1", "X")), cm(1.0));
    close(num(named(s, "Control", "Row_1", "Y")), cm(2.0));
    assert_eq!(formula(named(s, "Control", "Row_1", "XDyn")), Some("Controls.Row_1.X"));
    assert_eq!(formula(named(s, "Control", "Row_1", "YDyn")), Some("Controls.Row_1.Y"));
    assert_eq!(formula(named(s, "User", "ODFMod0", "Value")), Some("Controls.Row_1.X/Width*21600"));
    close(num(named(s, "User", "ODFMod0", "Value")), 5400.0);
}

/// A `$n` is a value of the view box, which the frame maps: a handle's `X` and `Y` are shares of the frame, the way the
/// glue points' cells are, so that the modifier a handle drives (`Controls.Row_1.X/Width*21600`) works out to what the file says whatever the frame. They were constants,
/// and a resize turned `5400` into `2700`.
#[test]
fn secname_a_handles_cells_are_shares_of_the_frame_and_the_modifiers_formulas_are_as_they_were() {
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400 16200 7200" draw:enhanced-path="M 0 0 L $0 $1"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/><draw:handle draw:handle-position-x="right" draw:handle-position-y="$1"/><draw:handle draw:handle-position-x="$2" draw:handle-position-y="10800"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    // An x handle: a quarter of the width, at the top (the whole height).
    assert_eq!(formula(named(s, "Control", "Row_1", "X")), Some("Width*0.25"));
    assert_eq!(formula(named(s, "Control", "Row_1", "Y")), Some("Height"));
    // A y handle at the right: the width, and a quarter of the height (the view box's y runs down, the frame's up).
    assert_eq!(formula(named(s, "Control", "Row_2", "X")), Some("Width"));
    assert_eq!(formula(named(s, "Control", "Row_2", "Y")), Some("Height*0.25"));
    // A third of the width needs every digit of the share, or the modifier does not work out to 7200; a constant is a share too.
    assert_eq!(formula(named(s, "Control", "Row_3", "X")), Some("Width*0.3333333333333333"));
    assert_eq!(formula(named(s, "Control", "Row_3", "Y")), Some("Height*0.5"));
    // The values are the ones the cells held, and the modifiers' formulas read the handles as before.
    close(num(named(s, "Control", "Row_1", "X")), cm(1.0));
    close(num(named(s, "Control", "Row_2", "Y")), cm(2.0) * 0.25);
    assert_eq!(formula(named(s, "User", "ODFMod0", "Value")), Some("Controls.Row_1.X/Width*21600"));
    assert_eq!(formula(named(s, "User", "ODFMod1", "Value")), Some("(1-Controls.Row_2.Y/Height)*21600"));
    assert_eq!(formula(named(s, "Control", "Row_1", "XDyn")), Some("Controls.Row_1.X"));
    close(num(named(s, "User", "ODFMod2", "Value")), 7200.0);
    // A view box with an origin: the share is of the span, and the formula of the modifier adds the origin.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="100 200 1000 1000" draw:modifiers="300 400" draw:enhanced-path="M 0 0 L $0 $1"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="$1"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    assert_eq!(formula(named(s, "Control", "Row_1", "X")), Some("Width*0.2"));
    assert_eq!(formula(named(s, "Control", "Row_1", "Y")), Some("Height*0.8"));
    assert_eq!(formula(named(s, "User", "ODFMod1", "Value")), Some("(1-Controls.Row_1.Y/Height)*1000+200"));
}

/// A handle's share that is not a finite number (a view box of a width no number divides by), or whose cell is not one (the frame is larger than the share leaves
/// room for), leaves the cell a value and the handle a row: nothing is a formula that no number comes out of.
#[test]
fn secname_a_share_that_is_not_a_finite_number_leaves_the_handles_cell_a_value() {
    // 1e10 over 1e-300 is past every number; 1e4 over 1e-304 is 1e308, and 40 cm of it is not a number.
    for (view_box, modifier, width) in [("0 0 1e-300 21600", "1e10", "4cm"), ("0 0 1e-304 21600", "1e4", "40cm")] {
        let (doc, _) = read(&format!(
            r#"<draw:custom-shape svg:width="{width}" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="{view_box}" draw:modifiers="{modifier}" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#
        ));
        let s = only(&doc);
        let x = named(s, "Control", "Row_1", "X").unwrap_or_else(|| panic!("{view_box}: the handle has a row"));
        assert!(formula(Some(x)).is_none(), "{view_box}: no formula comes out of a share that is not a number");
        assert_eq!(num(Some(x)), 0.0, "{view_box}: the cell holds the 0 that stands for a number that is not one");
        assert_eq!(formula(named(s, "Control", "Row_1", "Y")), Some("Height"), "{view_box}: the other axis is as it is");
    }
}

#[test]
fn odg_shapes_a_polar_handle_or_one_at_a_formula_is_kept_and_makes_no_row() {
    let (doc, report) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400 100" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="?f0" draw:handle-range-x-minimum="0" draw:handle-range-x-maximum="21600"/><draw:handle draw:handle-position="$1 $1" draw:handle-polar="10800 10800"/><draw:equation draw:name="f0" draw:formula="1"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Control", None)).is_none());
    assert_eq!(report.kept("handle"), 2);
    close(num(named(s, "User", "ODFMod0", "Value")), 5400.0);
    assert!(formula(named(s, "User", "ODFMod0", "Value")).is_none(), "no handle drives it");
}

#[test]
fn odg_shapes_the_glue_points_of_a_custom_shape_are_rows_4_on() {
    // Glue points "0 5400 21600 16200" on a 21600 square view box, a connector ending on glue point 5.
    let shapes = concat!(
        r#"<draw:custom-shape xml:id="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="4cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-points="0 5400 21600 16200" draw:enhanced-path="M 0 0 L 21600 21600"/></draw:custom-shape>"#,
        r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:end-shape="a" draw:end-glue-point="5"/>"#
    );
    let (doc, report) = read(shapes);
    let s = nth(&doc, 0, 0);
    // Rows 0 to 3 at the side middles (top, left, bottom, right), row 4 at shares (0, 0.75) and row 5 at (1, 0.25).
    let w = cm(4.0);
    close(num(row_cell(s, "Connection", None, 0, "Y")), w);
    close(num(row_cell(s, "Connection", None, 1, "X")), 0.0);
    close(num(row_cell(s, "Connection", None, 3, "X")), w);
    close(num(row_cell(s, "Connection", None, 4, "X")), 0.0);
    close(num(row_cell(s, "Connection", None, 4, "Y")) / w, 0.75);
    close(num(row_cell(s, "Connection", None, 5, "X")) / w, 1.0);
    close(num(row_cell(s, "Connection", None, 5, "Y")) / w, 0.25);
    // The connector is glued to row 5 with ToPart 105.
    let connects: Vec<_> = page(&doc, 0).connects.iter().collect();
    assert_eq!(connects.len(), 1);
    let c = connects[0];
    assert_eq!((c.from_sheet, c.to_sheet, c.to_part, c.from_part), (2, 1, Some(105), Some(12)));
    assert_eq!(c.from_cell.as_deref(), Some("EndX"));
    assert_eq!(c.to_cell.as_deref(), Some("Connections.X6"));
    assert_eq!(report.warned("glue_target_missing"), 0);
    let conn = nth(&doc, 0, 1);
    assert_eq!(formula(cell(conn, "EndX")), Some("PAR(PNT(Sheet.1!Connections.X6,Sheet.1!Connections.Y6))"));
    assert_eq!(formula(cell(conn, "EndY")), Some("PAR(PNT(Sheet.1!Connections.X6,Sheet.1!Connections.Y6))"));
    assert_eq!(formula(cell(conn, "BeginX")), None, "a free begin holds plain values");
}

#[test]
fn odg_shapes_a_glue_point_element_is_a_length_from_the_centre_in_libreoffice_s_form() {
    // A rect 3 cm by 2 cm with a glue point of id 4 at x 2.5 cm, y 0 cm, no align: 2.25 cm across and 1 cm up.
    let (doc, _) = read(
        r#"<draw:rect svg:x="5cm" svg:y="5cm" svg:width="3cm" svg:height="2cm"><draw:glue-point draw:id="4" svg:x="2.5cm" svg:y="0cm"/></draw:rect>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Connection", None, 4, "X")), cm(2.25));
    close(num(row_cell(s, "Connection", None, 4, "Y")), cm(1.0));
    // Percentages are shares of the frame from its centre: 50% is the edge.
    let (doc, _) = read(
        r#"<draw:rect svg:width="4cm" svg:height="2cm"><draw:glue-point draw:id="4" svg:x="50%" svg:y="0%"/><draw:glue-point draw:id="5" svg:x="25%" svg:y="-50%"/></draw:rect>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Connection", None, 4, "X")), cm(4.0));
    close(num(row_cell(s, "Connection", None, 4, "Y")), cm(1.0));
    // 25% and -50% on a 4 cm by 2 cm frame: 3 cm and the top edge.
    close(num(row_cell(s, "Connection", None, 5, "X")), cm(3.0));
    close(num(row_cell(s, "Connection", None, 5, "Y")), cm(2.0));
    assert_eq!(formula(row_cell(s, "Connection", None, 5, "X")), Some("Width*(0.5+25/100)"));
}

#[test]
fn odg_shapes_escape_directions_read_as_dirx_and_diry() {
    let (doc, _) = read(
        r#"<draw:rect svg:width="4cm" svg:height="2cm"><draw:glue-point draw:id="4" svg:x="0%" svg:y="0%" draw:escape-direction="left"/><draw:glue-point draw:id="5" svg:x="0%" svg:y="0%" draw:escape-direction="auto"/></draw:rect>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Connection", None, 4, "DirX")), -1.0);
    close(num(row_cell(s, "Connection", None, 4, "DirY")), 0.0);
    assert!(row_cell(s, "Connection", None, 5, "DirX").is_none());
    // A listed point's leaving direction is an angle clockwise as seen from the x axis: 270 is straight up.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-points="10800 0" draw:glue-point-leaving-directions="270" draw:enhanced-path="M 0 0 L 1 1"/></draw:custom-shape>"#,
    );
    let s = only(&doc);
    close(num(row_cell(s, "Connection", None, 4, "DirX")), 0.0);
    close(num(row_cell(s, "Connection", None, 4, "DirY")), 1.0);
}

#[test]
fn odg_shapes_a_custom_shape_of_glue_point_type_none_that_lists_none_has_no_standard_rows() {
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-point-type="none" draw:enhanced-path="M 0 0 L 1 1"/></draw:custom-shape>"#,
    );
    assert!(only(&doc).sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_none());
}

#[test]
fn odg_shapes_a_connector_end_with_no_glue_point_is_whole_shape_glue() {
    let shapes = concat!(
        r#"<draw:rect xml:id="id1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:start-shape="id1" draw:end-shape="id1" draw:end-glue-point="2"/>"#,
        r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:end-shape="id1" draw:end-glue-point="1"/>"#,
        r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:end-shape="id1" draw:end-glue-point="3"/>"#
    );
    let (doc, _) = read(shapes);
    let connects: Vec<_> = page(&doc, 0).connects.iter().collect();
    // Connector 2: the start whole-shape (ToPart 3, `PinX`), the end on glue id 2, the bottom middle, row 2.
    let find = |from: u32, cell: &str| connects.iter().find(|c| c.from_sheet == from && c.from_cell.as_deref() == Some(cell)).copied().unwrap();
    let start = find(2, "BeginX");
    assert_eq!((start.to_part, start.to_cell.as_deref(), start.from_part), (Some(3), Some("PinX"), Some(9)));
    assert_eq!(find(2, "EndX").to_part, Some(102));
    // Glue id 1 is the right side middle, row 3 (ToPart 103); id 3 the left, row 1 (ToPart 101).
    assert_eq!(find(3, "EndX").to_part, Some(103));
    assert_eq!(find(4, "EndX").to_part, Some(101));
    // The whole-shape end holds plain values, no formula.
    let c2 = nth(&doc, 0, 1);
    assert_eq!(formula(cell(c2, "BeginX")), None);
    close(num(cell(c2, "BeginX")), cm(1.0));
    close(num(cell(c2, "BeginY")), cm(29.7 - 1.0));
}

#[test]
fn odg_reader_defaults_the_glue_id_is_not_the_row() {
    // An end on glue id 1 of a rectangle is the right side's row 3, and id 3 the left side's row 1: the formulas name those rows.
    let shapes = concat!(
        r#"<draw:rect xml:id="r" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:connector svg:x1="6cm" svg:y1="4cm" svg:x2="2cm" svg:y2="4cm" draw:start-shape="r" draw:start-glue-point="1" draw:end-shape="r" draw:end-glue-point="3"/>"#
    );
    let (doc, _) = read(shapes);
    let c = nth(&doc, 0, 1);
    assert_eq!(formula(cell(c, "BeginX")), Some("PAR(PNT(Sheet.1!Connections.X4,Sheet.1!Connections.Y4))"));
    assert_eq!(formula(cell(c, "EndX")), Some("PAR(PNT(Sheet.1!Connections.X2,Sheet.1!Connections.Y2))"));
}

#[test]
fn odg_shapes_a_missing_shape_is_a_free_end_with_a_warning() {
    let (doc, report) =
        read(r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="6cm" draw:start-shape="nope" draw:start-glue-point="2"/>"#);
    assert!(page(&doc, 0).connects.is_empty());
    assert_eq!(report.warned("glue_target_missing"), 1);
    assert_eq!(formula(cell(only(&doc), "BeginX")), None);
}

#[test]
fn odg_reader_defaults_a_connector_s_route_cells_and_geometry() {
    let (doc, _) = read(
        r#"<draw:connector svg:x1="2cm" svg:y1="5cm" svg:x2="8cm" svg:y2="2cm"/><draw:connector draw:type="curve" svg:x1="2cm" svg:y1="5cm" svg:x2="8cm" svg:y2="2cm"/>"#,
    );
    close(num(cell(nth(&doc, 0, 0), "ShapeRouteStyle")), 1.0);
    close(num(cell(nth(&doc, 0, 0), "ConLineRouteExt")), 1.0);
    close(num(cell(nth(&doc, 0, 1), "ShapeRouteStyle")), 1.0);
    close(num(cell(nth(&doc, 0, 1), "ConLineRouteExt")), 2.0);
    // The 1-D cells, as for a line.
    let c = nth(&doc, 0, 0);
    close(num(cell(c, "Width")), cm(6.708203932499369));
    assert_eq!(formula(cell(c, "Width")), Some("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"));
    // The marker of a route DiaCraft may route again.
    close(num(named(c, "User", "DiaCraftRoute", "Value")), 1.0);
    // With a path: `svg:d` in page coordinates, hundredths of a millimetre, and its view box ignored.
    let (doc, _) = read(
        r#"<draw:connector svg:x1="5cm" svg:y1="3cm" svg:x2="12cm" svg:y2="9cm" svg:viewBox="0 0 7001 6001" svg:d="M 5000 3000 h 3500 v 6000 h 3500"/>"#,
    );
    let c = only(&doc);
    assert_eq!(rows(c, 0), 4);
    // The route in the connector's own frame: x along the line to the end, the begin at the origin.
    let (bx, by, ex, ey) = (cm(5.0), cm(29.7 - 3.0), cm(12.0), cm(29.7 - 9.0));
    let width = (ex - bx).hypot(ey - by);
    let angle = (ey - by).atan2(ex - bx);
    let frame = |px: f64, py: f64| {
        let (dx, dy) = (px - bx, py - by);
        (dx * angle.cos() + dy * angle.sin(), -dx * angle.sin() + dy * angle.cos())
    };
    let want = [(5.0, 3.0), (8.5, 3.0), (8.5, 9.0), (12.0, 9.0)];
    for (i, (px, py)) in want.into_iter().enumerate() {
        let ix = u32::try_from(i + 1).unwrap();
        let (u, v) = frame(cm(px), cm(29.7 - py));
        assert_eq!(row_type(c, 0, ix).as_deref(), Some(if i == 0 { "MoveTo" } else { "LineTo" }));
        close(num(row_cell(c, "Geometry", Some(0), ix, "X")), u);
        close(num(row_cell(c, "Geometry", Some(0), ix, "Y")), v);
    }
    close(num(row_cell(c, "Geometry", Some(0), 4, "X")), width);
    // A cubic segment is a NURBSTo of degree 3 whose control points are the segment's.
    let (doc, _) =
        read(r#"<draw:connector svg:x1="2cm" svg:y1="5cm" svg:x2="8cm" svg:y2="2cm" svg:d="M 2000 5000 c 1000 0 2000 -3000 6000 -3000"/>"#);
    let c = only(&doc);
    assert_eq!(row_type(c, 0, 1).as_deref(), Some("MoveTo"));
    assert_eq!(row_type(c, 0, 2).as_deref(), Some("NURBSTo"));
    let e = row_cell(c, "Geometry", Some(0), 2, "E").expect("E");
    assert!(formula(Some(e)).is_some_and(|f| f.starts_with("NURBS(1,3,1,1,")), "{:?}", formula(Some(e)));
    close(num(row_cell(c, "Geometry", Some(0), 2, "B")), 1.0);
    close(num(row_cell(c, "Geometry", Some(0), 2, "D")), 1.0);
}

#[test]
fn odg_shapes_a_group_is_the_box_of_its_members_and_they_are_measured_in_it() {
    // r1 at (2, 3) cm, 4 by 2; r2 at (7, 6), 3 by 3: the group is 8 cm by 6 cm, PinX 6 cm and PinY 23.7 cm; r1's pin is at (2, 5) cm in the
    // group's frame and r2's at (6.5, 1.5) cm.
    let (doc, report) = read(
        r#"<draw:g draw:name="G"><draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/><draw:rect svg:x="7cm" svg:y="6cm" svg:width="3cm" svg:height="3cm"/></draw:g>"#,
    );
    let g = only(&doc);
    assert_eq!(g.kind, ShapeType::Group);
    close(num(cell(g, "Width")), cm(8.0));
    close(num(cell(g, "Height")), cm(6.0));
    close(num(cell(g, "PinX")), cm(6.0));
    close(num(cell(g, "PinY")), cm(23.7));
    assert_eq!(formula(cell(g, "LocPinX")), Some("Width*0.5"));
    assert!(cell(g, "Angle").is_none() && cell(g, "FlipX").is_none() && cell(g, "FlipY").is_none());
    assert_eq!(g.children.len(), 2);
    let r1 = g.children.get(0).unwrap();
    close(num(cell(r1, "PinX")), cm(2.0));
    close(num(cell(r1, "PinY")), cm(5.0));
    let r2 = g.children.get(1).unwrap();
    close(num(cell(r2, "PinX")), cm(6.5));
    close(num(cell(r2, "PinY")), cm(1.5));
    // No geometry, no text; the four glue rows.
    assert!(g.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).is_none());
    assert!(g.text.is_none());
    assert!(g.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_some());
    assert_eq!(g.name.as_deref(), Some("G"));
    assert_eq!(report.stats.shapes, 3);
}

#[test]
fn odg_shapes_a_turned_member_counts_by_its_turned_frame_and_a_nested_group_by_its_box() {
    // A bar 4 cm by 0.4 cm turned 90 degrees about its frame's origin then moved to (5, 5): its box is 0.4 wide and 4 tall, left of 5.
    let (doc, _) = read(
        r#"<draw:g><draw:rect svg:width="4cm" svg:height="0.4cm" draw:transform="rotate (1.5707963267948966) translate (5cm 5cm)"/><draw:g><draw:rect svg:x="8cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></draw:g></draw:g>"#,
    );
    let g = only(&doc);
    // A positive angle turns counterclockwise as seen: the bar's far end rises. It covers x from 5 to 5.4 and y from 1 to 5 (page, y
    // down); the inner group x 8 to 10, y 1 to 2: the box is x 5 to 10, y 1 to 5.
    close(num(cell(g, "Width")), cm(5.0));
    close(num(cell(g, "Height")), cm(4.0));
    let inner = g.children.get(1).unwrap();
    assert_eq!(inner.kind, ShapeType::Group);
    // The inner group's own frame is the rectangle's, and the rectangle is in it at its centre.
    close(num(cell(inner, "Width")), cm(2.0));
    close(num(cell(inner.children.get(0).unwrap(), "PinX")), cm(1.0));
    // The inner group in the outer one's frame: its pin is its centre less the outer box's bottom-left corner (5, 29.7 - 5, y up).
    close(num(cell(inner, "PinX")), cm(9.0 - 5.0));
    close(num(cell(inner, "PinY")), cm((29.7 - 1.5) - (29.7 - 5.0)));
}

#[test]
fn odg_shapes_a_connector_in_a_group_keeps_its_glue_and_a_member_a_page_holds_is_found_by_id() {
    let shapes = concat!(
        r#"<draw:rect xml:id="top" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:g><draw:rect xml:id="in" svg:x="4cm" svg:y="4cm" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:connector svg:x1="2cm" svg:y1="1.5cm" svg:x2="4cm" svg:y2="4.5cm" draw:start-shape="top" draw:start-glue-point="1" draw:end-shape="in" draw:end-glue-point="3"/></draw:g>"#
    );
    let (doc, _) = read(shapes);
    let connects: Vec<_> = page(&doc, 0).connects.iter().collect();
    assert_eq!(connects.len(), 2);
    // Shapes are numbered in pre-order: rect 1, group 2, rect 3, connector 4.
    assert!(connects.iter().any(|c| c.from_sheet == 4 && c.to_sheet == 1 && c.to_part == Some(103)));
    assert!(connects.iter().any(|c| c.from_sheet == 4 && c.to_sheet == 3 && c.to_part == Some(101)));
}

#[test]
fn odg_reader_defaults_a_glue_point_with_align_is_a_length_from_the_named_corner_or_edge() {
    // 4 cm by 2 cm, each point 0.5 cm in from (or out along) the side its `draw:align` names.
    let point = |id: u32, align: &str| format!(r#"<draw:glue-point draw:id="{id}" svg:x="0.5cm" svg:y="0.25cm" draw:align="{align}"/>"#);
    let aligns = ["top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right"];
    let inner: String = aligns.iter().enumerate().map(|(i, a)| point(4 + i as u32, a)).collect();
    let (doc, _) = read(&format!(r#"<draw:rect svg:width="4cm" svg:height="2cm">{inner}</draw:rect>"#));
    let s = only(&doc);
    let at = |i: u32| (num(row_cell(s, "Connection", None, 4 + i, "X")), num(row_cell(s, "Connection", None, 4 + i, "Y")));
    let (w, h, dx, dy) = (cm(4.0), cm(2.0), cm(0.5), cm(0.25));
    let want = [
        (dx, h - dy),
        (w / 2.0 + dx, h - dy),
        (w + dx, h - dy),
        (dx, h / 2.0 - dy),
        (w / 2.0 + dx, h / 2.0 - dy),
        (w + dx, h / 2.0 - dy),
        (dx, -dy),
        (w / 2.0 + dx, -dy),
        (w + dx, -dy),
    ];
    for (i, (x, y)) in want.into_iter().enumerate() {
        let got = at(i as u32);
        close(got.0, x);
        close(got.1, y);
    }
    // A left-aligned x is a length (no formula); the others read the width.
    assert!(formula(row_cell(s, "Connection", None, 4, "X")).is_none());
    assert_eq!(formula(row_cell(s, "Connection", None, 5, "X")).map(|f| f.starts_with("Width*0.5+")), Some(true));
    assert_eq!(formula(row_cell(s, "Connection", None, 6, "X")).map(|f| f.starts_with("Width+")), Some(true));
    assert_eq!(formula(row_cell(s, "Connection", None, 4, "Y")).map(|f| f.starts_with("Height-")), Some(true));
    assert!(formula(row_cell(s, "Connection", None, 10, "Y")).is_none(), "a bottom-aligned y is a length");
}

#[test]
fn odg_shapes_the_standard_ids_a_repeated_id_and_an_unreadable_position_make_no_row_or_the_centre() {
    let (doc, report) = read(concat!(
        r#"<draw:rect svg:width="4cm" svg:height="2cm">"#,
        r#"<draw:glue-point draw:id="2" svg:x="0cm" svg:y="0cm"/>"#,
        r#"<draw:glue-point svg:x="0cm" svg:y="0cm"/>"#,
        r#"<draw:glue-point draw:id="9" svg:x="50%" svg:y="0%"/>"#,
        r#"<draw:glue-point draw:id="9" svg:x="0%" svg:y="0%"/>"#,
        r#"<draw:glue-point draw:id="6" svg:x="nonsense" svg:y="0%"/>"#,
        r#"<draw:glue-point draw:id="5" svg:x="25%" svg:y="0cm"/>"#,
        r#"</draw:rect>"#
    ));
    let s = only(&doc);
    // Rows 0 to 3 stay the standard ones; 5, 6 and 9 are made, in order of id, the first 9 winning.
    let section = s.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).expect("connection rows");
    let ixs: Vec<u32> = section.rows.iter().filter_map(|r| r.ix).collect();
    assert_eq!(ixs, [0, 1, 2, 3, 5, 6, 9]);
    close(num(row_cell(s, "Connection", None, 9, "X")), cm(4.0));
    // The unreadable position is the centre, with a warning; the mixed forms (a percentage and a length) are unreadable too.
    close(num(row_cell(s, "Connection", None, 6, "X")), cm(2.0));
    close(num(row_cell(s, "Connection", None, 6, "Y")), cm(1.0));
    close(num(row_cell(s, "Connection", None, 5, "X")), cm(2.0));
    close(num(row_cell(s, "Connection", None, 5, "Y")), cm(1.0));
    assert_eq!(report.warned("glue_point_id"), 2);
    assert_eq!(report.warned("glue_point_position"), 2);
    assert_eq!(report.kept("glue_point"), 6, "every element is kept as written");
}

#[test]
fn odg_shapes_every_segment_kind_of_a_route() {
    // H, V, relative and absolute lines, a smooth cubic, a quadratic raised to a cubic, a closed subpath, and an arc as its chord.
    let (doc, report) = read(
        r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm" svg:d="M 0 0 H 1000 V 500 l 500 0 L 2000 500 C 2100 500 2200 600 2300 600 S 2500 700 2600 700 Q 2700 800 2800 700 A 100 100 0 0 1 3000 700 Z"/>"#,
    );
    let c = only(&doc);
    let kinds: Vec<String> = (1..=rows(c, 0) as u32).map(|ix| row_type(c, 0, ix).unwrap_or_default()).collect();
    assert_eq!(kinds[..5], ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
    assert_eq!(kinds[5..8], ["NURBSTo", "NURBSTo", "NURBSTo"], "C, S and Q are cubics");
    assert_eq!(kinds[8], "LineTo", "an arc is its chord");
    assert_eq!(kinds.last().map(String::as_str), Some("LineTo"), "Z closes");
    assert_eq!(report.warned("geometry_unreadable"), 0);
    // A quadratic raised to a cubic has the controls a third and two thirds of the way: its first control is 2/3 of the way to q from the pen.
    let e = formula(row_cell(c, "Geometry", Some(0), 7, "E")).expect("E");
    assert!(e.starts_with("NURBS(1,3,1,1,"), "{e}");
    // Unreadable data is a straight segment and a warning.
    let (doc, report) = read(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm" svg:d="M 0 0 L"/>"#);
    assert_eq!((rows(only(&doc), 0), report.warned("geometry_unreadable")), (2, 1));
    // No path: a straight segment, and the marker of a route DiaCraft may route again.
    let (doc, _) = read(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm"/>"#);
    assert_eq!(rows(only(&doc), 0), 2);
    close(num(row_cell(only(&doc), "Geometry", Some(0), 2, "X")), cm(3.0));
    assert_eq!(formula(row_cell(only(&doc), "Geometry", Some(0), 2, "X")), Some("Width*1"));
}

#[test]
fn odg_reader_defaults_the_four_connector_types() {
    let types = [("standard", 1.0, 1.0), ("line", 2.0, 1.0), ("lines", 2.0, 1.0), ("curve", 1.0, 2.0), ("nonsense", 1.0, 1.0)];
    for (ty, style, ext) in types {
        let (doc, _) = read(&format!(r#"<draw:connector draw:type="{ty}" svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm"/>"#));
        close(num(cell(only(&doc), "ShapeRouteStyle")), style);
        close(num(cell(only(&doc), "ConLineRouteExt")), ext);
    }
    // The type is taken; another attribute is kept as written.
    let (doc, _) = read(r#"<draw:connector draw:type="curve" draw:line-skew="1cm" svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm"/>"#);
    assert!(odf(only(&doc)).raw.get("draw:type").is_none());
    assert_eq!(odf(only(&doc)).raw.get("draw:line-skew"), Some("1cm"));
}

#[test]
fn odg_shapes_a_group_with_no_members_a_title_and_members_on_layers() {
    let (doc, report) = read(r#"<draw:g draw:name="E"/>"#);
    let g = only(&doc);
    assert_eq!(g.kind, ShapeType::Group);
    close(num(cell(g, "Width")), 0.0);
    close(num(cell(g, "PinX")), 0.0);
    assert_eq!(report.stats.shapes, 1);
    // A title between members is kept in the group's passthrough, in place; a member keeps its layer.
    let mut d = Drawing::a4();
    d.masters = d.masters.replace("</draw:layer-set>", r#"<draw:layer draw:name="Notes"/></draw:layer-set>"#);
    let (doc, report) = d
        .page(
            r#"<draw:g draw:z-index="3"><draw:rect draw:layer="Notes" svg:width="1cm" svg:height="1cm"/><svg:title>T</svg:title><draw:rect svg:width="1cm" svg:height="1cm"/></draw:g><draw:rect draw:z-index="1" svg:width="1cm" svg:height="1cm"/>"#,
        )
        .read();
    // The group's z-index orders it on the page: the rectangle with 1 first.
    assert_eq!(nth(&doc, 0, 0).kind, ShapeType::Shape);
    let g = nth(&doc, 0, 1);
    assert_eq!(g.kind, ShapeType::Group);
    assert_eq!(odf(g).kept.len(), 1);
    assert_eq!(report.kept("title"), 1);
    assert_eq!(crate::drawing::text(cell(g.children.get(0).unwrap(), "LayerMember")), "0");
    assert!(cell(g.children.get(1).unwrap(), "LayerMember").is_none());
}

#[test]
fn odg_shapes_a_connector_in_a_group_has_its_ends_measured_in_the_group_s_frame() {
    // The connector from (2, 3) to (6, 3) cm and a rectangle at (6, 5) 1 cm square: the box is x 2 to 7, y 3 to 6 (page, y down).
    let (doc, _) = read(
        r#"<draw:g><draw:connector svg:x1="2cm" svg:y1="3cm" svg:x2="6cm" svg:y2="3cm"/><draw:rect svg:x="6cm" svg:y="5cm" svg:width="1cm" svg:height="1cm"/></draw:g>"#,
    );
    let g = only(&doc);
    close(num(cell(g, "Width")), cm(5.0));
    close(num(cell(g, "Height")), cm(3.0));
    let c = g.children.get(0).unwrap();
    // Bottom of the box is y = 29.7 - 6 (y up): the connector's y in the frame is (29.7 - 3) - (29.7 - 6) = 3 cm.
    close(num(cell(c, "BeginX")), 0.0);
    close(num(cell(c, "BeginY")), cm(3.0));
    close(num(cell(c, "EndX")), cm(4.0));
    close(num(cell(c, "PinX")), cm(2.0));
    close(num(cell(c, "PinY")), cm(3.0));
    close(num(cell(c, "Width")), cm(4.0));
}

/// The numbers of a `NURBS(...)` constructor's text.
fn nurbs_numbers(text: &str) -> Vec<f64> {
    text.trim_start_matches("NURBS(").trim_end_matches(')').split(',').map(|n| n.parse().expect("a number")).collect()
}

#[test]
fn odg_reader_defaults_a_quadratic_segment_is_raised_to_the_cubic_with_the_same_curve() {
    // From (0, 0) through the control (15 mm, 10 mm) to (30 mm, 0): hundredths of a millimetre in `svg:d`, down being y.
    let (doc, _) = read(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="3cm" svg:y2="0cm" svg:d="M 0 0 Q 1500 1000 3000 0"/>"#);
    let c = only(&doc);
    assert_eq!(row_type(c, 0, 2).as_deref(), Some("NURBSTo"));
    let e = nurbs_numbers(formula(row_cell(c, "Geometry", Some(0), 2, "E")).expect("E"));
    // knot-last 1, degree 3, both types 1, then (x1, y1, 0, 1) and (x2, y2, 0, 1): the controls a third and two thirds of the way, in inches,
    // in the frame whose x runs along the line and whose y runs up.
    let inch = |hundredths: f64| hundredths / 2540.0;
    assert_eq!(e.len(), 12);
    assert_eq!(&e[..4], &[1.0, 3.0, 1.0, 1.0]);
    close(e[4], inch(1000.0));
    close(e[5], -inch(2000.0 / 3.0));
    assert_eq!(&e[6..8], &[0.0, 1.0]);
    close(e[8], inch(2000.0));
    close(e[9], -inch(2000.0 / 3.0));
    assert_eq!(&e[10..], &[0.0, 1.0]);
    // The end, as lengths in the frame.
    close(num(row_cell(c, "Geometry", Some(0), 2, "X")), inch(3000.0));
    close(num(row_cell(c, "Geometry", Some(0), 2, "Y")), 0.0);
    // A cubic is the same constructor with its own controls: C 100 200 300 400 500 0 from (0, 0).
    let (doc, _) = read(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="5cm" svg:y2="0cm" svg:d="M 0 0 C 1000 2000 3000 4000 5000 0"/>"#);
    let e = nurbs_numbers(formula(row_cell(only(&doc), "Geometry", Some(0), 2, "E")).expect("E"));
    close(e[4], inch(1000.0));
    close(e[5], -inch(2000.0));
    close(e[8], inch(3000.0));
    close(e[9], -inch(4000.0));
    // A smooth cubic reflects the last control about the pen, and a smooth quadratic its control.
    let (doc, _) =
        read(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="5cm" svg:y2="0cm" svg:d="M 0 0 C 0 100 100 100 100 0 S 200 -100 200 0"/>"#);
    let e = nurbs_numbers(formula(row_cell(only(&doc), "Geometry", Some(0), 3, "E")).expect("E"));
    close(e[4], inch(100.0));
    close(e[5], -inch(-100.0));
}

#[test]
fn odg_reader_defaults_a_controls_row_is_named_by_its_handle_s_place_among_the_handles() {
    // The first handle is polar and makes no row; the second is the shape's second handle, `Row_2`.
    let (doc, _) = read(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L $0 0"><draw:handle draw:handle-position="$0 top" draw:handle-polar="0 0"/><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#,
    );
    let s = only(&doc);
    assert!(named(s, "Control", "Row_1", "X").is_none());
    close(num(named(s, "Control", "Row_2", "X")), cm(1.0));
    assert_eq!(formula(named(s, "User", "ODFMod0", "Value")), Some("Controls.Row_2.X/Width*21600"));
}

#[test]
fn odg_reader_defaults_the_arithmetic_of_a_glue_point_s_position_and_its_four_directions() {
    let (doc, _) = read(concat!(
        r#"<draw:rect svg:width="4cm" svg:height="2cm">"#,
        r#"<draw:glue-point draw:id="4" svg:x="-25%" svg:y="25%" draw:escape-direction="right"/>"#,
        r#"<draw:glue-point draw:id="5" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="up"/>"#,
        r#"<draw:glue-point draw:id="6" svg:x="-1cm" svg:y="-0.5cm" draw:escape-direction="down"/>"#,
        r#"<draw:glue-point draw:id="7" svg:x="0.5in" svg:y="0.25in" draw:align="top-left" draw:escape-direction="horizontal"/>"#,
        r#"</draw:rect>"#
    ));
    let s = only(&doc);
    let at = |ix: u32| (num(row_cell(s, "Connection", None, ix, "X")), num(row_cell(s, "Connection", None, ix, "Y")));
    // A percentage is a share of the frame from its centre, y down as ODF's runs: 25% to the left of the middle is 1 cm of 4, and 25% below
    // it is 0.5 cm up from the bottom of 2.
    close(at(4).0, cm(1.0));
    close(at(4).1, cm(0.5));
    // A length in LibreOffice's own form: hundredths of a millimetre are hundredths of a percent: 1 cm is 1,000, a tenth of the width.
    close(at(5).0, cm(4.0) * 0.6);
    close(at(5).1, cm(2.0) * 0.45);
    close(at(6).0, cm(4.0) * 0.4);
    close(at(6).1, cm(2.0) * 0.55);
    // From the top-left corner: 0.5 in across and 0.25 in down.
    close(at(7).0, 0.5);
    close(at(7).1, cm(2.0) - 0.25);
    let dir = |ix: u32| {
        (row_cell(s, "Connection", None, ix, "DirX").map(|c| num(Some(c))), row_cell(s, "Connection", None, ix, "DirY").map(|c| num(Some(c))))
    };
    assert_eq!(dir(4), (Some(1.0), Some(0.0)));
    assert_eq!(dir(5), (Some(0.0), Some(1.0)));
    assert_eq!(dir(6), (Some(0.0), Some(-1.0)));
    assert_eq!(dir(7), (None, None), "horizontal is no direction");
    // A glue point's row holds the cells its form needs and no more.
    let section = s.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).expect("rows");
    let last = section.rows.iter().find(|r| r.ix == Some(7)).expect("row 7");
    assert_eq!(last.cells.len(), 2);
}

#[test]
fn odg_shapes_a_shape_inside_a_kept_wrapper_is_not_one_a_connector_can_name() {
    // The wrapper is kept whole, so neither it nor the shape it holds is a shape of the model: an end that names either is free.
    let (doc, report) = read(concat!(
        r#"<draw:a xml:id="w" xlink:href="u"><draw:rect xml:id="inside" svg:width="1cm" svg:height="1cm"/><draw:measure/></draw:a>"#,
        r#"<draw:rect xml:id="out" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="inside" draw:end-shape="w"/>"#,
        r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="out"/>"#
    ));
    assert_eq!(report.warned("glue_target_missing"), 2);
    assert_eq!(page(&doc, 0).connects.len(), 1, "only the end on the shape the page models");
    // What the wrapper holds is counted by its kind, once each.
    assert_eq!((report.kept("hyperlink_wrapper"), report.kept("nested_shape"), report.kept("measure")), (1, 1, 1));
}

#[test]
fn odg_shapes_a_group_has_no_transform_and_groups_in_a_row_are_not_groups_in_a_group() {
    // The attribute is kept as written, and the members are where they say they are.
    let (doc, report) =
        read(r#"<draw:g draw:transform="translate (1cm 1cm)"><draw:rect svg:x="2cm" svg:y="3cm" svg:width="1cm" svg:height="1cm"/></draw:g>"#);
    let g = only(&doc);
    assert_eq!(odf(g).raw.get("draw:transform"), Some("translate (1cm 1cm)"));
    assert_eq!(report.warned("transform_unreadable"), 0);
    close(num(cell(g, "Width")), cm(1.0));
    close(num(cell(g, "PinX")), cm(2.5));
    close(num(cell(g.children.get(0).unwrap(), "PinX")), cm(0.5));
    // 250 groups side by side, one after another: more than the nesting depth allows inside one another, and no nesting at all.
    let (doc, _) = read(&"<draw:g><draw:rect svg:width=\"1cm\" svg:height=\"1cm\"/></draw:g>".repeat(250));
    assert_eq!(page(&doc, 0).shapes.len(), 250);
    // And one inside another to the depth cap is read, a group past it is the nesting cap's refusal.
    let nest = |depth: usize| format!("{}{}", "<draw:g>".repeat(depth), "</draw:g>".repeat(depth));
    assert!(Drawing::a4().page(&nest(190)).read().1.stats.shapes == 190);
    assert!(diacraft_odg::read(&Drawing::a4().page(&nest(191)).bytes(), &diacraft_odg::Limits::default()).is_err());
}

#[test]
fn odg_shapes_what_an_object_holds_is_kept_whole_and_it_reads_no_text() {
    let (doc, report) = read(concat!(
        r#"<draw:custom-shape svg:width="3cm" svg:height="2cm"><draw:enhanced-geometry draw:extrusion="true"/><text:p>x</text:p></draw:custom-shape>"#,
        r#"<draw:g><text:p>y</text:p><draw:rect svg:width="1cm" svg:height="1cm"/></draw:g>"#,
        r#"<draw:caption svg:width="3cm" svg:height="2cm"><text:p>z</text:p><svg:title>T</svg:title></draw:caption>"#
    ));
    let extruded = nth(&doc, 0, 0);
    assert!(extruded.text.is_none(), "an object reads no text");
    assert_eq!(odf(extruded).kept.len(), 2, "the geometry and the paragraph are kept as written");
    let group = nth(&doc, 0, 1);
    assert!(group.text.is_none());
    let caption = nth(&doc, 0, 2);
    assert!(caption.text.is_none());
    assert_eq!(odf(caption).kept.len(), 2);
    assert_eq!(report.kept("title"), 1);
    // A group's own text node is kept as written; its member is a member.
    assert_eq!(group.children.len(), 1);
}

#[test]
fn odg_shapes_a_group_s_pin_is_the_middle_of_its_frame_and_its_loc_pins_are_half_its_size() {
    let (doc, _) = read(r#"<draw:g><draw:rect svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/></draw:g>"#);
    let g = only(&doc);
    close(num(cell(g, "LocPinX")), cm(2.0));
    close(num(cell(g, "LocPinY")), cm(1.0));
    assert_eq!(formula(cell(g, "LocPinY")), Some("Height*0.5"));
}

#[test]
fn odg_reader_defaults_a_quadratic_from_a_pen_that_is_not_the_origin() {
    // From (5 mm, 3 mm) through the control (15 mm, 10 mm) to (30 mm, 0): the controls are a third and two thirds of the way from each end.
    let (doc, _) = read(r#"<draw:connector svg:x1="0.5cm" svg:y1="0.3cm" svg:x2="3cm" svg:y2="0cm" svg:d="M 500 300 Q 1500 1000 3000 0"/>"#);
    let c = only(&doc);
    let e = nurbs_numbers(formula(row_cell(c, "Geometry", Some(0), 2, "E")).expect("E"));
    let inch = |hundredths: f64| hundredths / 2540.0;
    // The frame's origin is the begin (500, 300) and its x axis runs to (3000, 0): up and to the right, so the frame is turned.
    let (bx, by, ex, ey): (f64, f64, f64, f64) = (500.0, -300.0, 3000.0, 0.0);
    let (dx, dy) = (ex - bx, ey - by);
    let len = dx.hypot(dy);
    let frame = |x: f64, y: f64| {
        let (px, py) = (x - bx, -y - by);
        ((px * dx + py * dy) / len, (-px * dy + py * dx) / len)
    };
    let (c1, c2) =
        (frame(500.0 + 2.0 / 3.0 * 1000.0, 300.0 + 2.0 / 3.0 * 700.0), frame(3000.0 + 2.0 / 3.0 * (1500.0 - 3000.0), 0.0 + 2.0 / 3.0 * 1000.0));
    close(e[4], inch(c1.0));
    close(e[5], inch(c1.1));
    close(e[8], inch(c2.0));
    close(e[9], inch(c2.1));
}
