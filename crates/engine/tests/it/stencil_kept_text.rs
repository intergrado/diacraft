//! Kept text made to cost, styles that differ only in what a sheet keeps, the pin of a line in a dropped group, a connector
//! glued to a member of a dropped group, and the connection points a master of several shapes lists. Crafted and timed where the cost is the point
//! (the times print with `--nocapture`; the bound is 60 s, a hang test and not a speed test).

use std::time::{Duration, Instant};

use diacraft_doc::FormulaSlot;
use diacraft_engine::Session;
use diacraft_engine::draw::connection_points;
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::stencil_open::{CT_STENCIL, Dir, LOOK, box_cells, ids, num, open, refusal};
use crate::{blank_session, check_session, run};

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    println!("{what}: {took:?}");
    assert!(took < Duration::from_secs(60), "{what} took {took:?}");
    out
}

fn part_with(shapes: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="utf-8"?><MasterContents xmlns="{NS_2012}" xmlns:r="{NS_R}"><Shapes>{shapes}</Shapes></MasterContents>"#)
}

// ------------------------------------------------------------------------------------------------------------ 1 and 2: kept text made to cost

#[test]
fn an_element_of_thirty_thousand_declarations_and_children_is_refused_at_once_and_so_is_a_shape_with_three_hundred_thousand_attributes() {
    let dir = Dir::new();
    // The review's fixtures, through the reader: an element of 30,000 `xmlns:` declarations and 30,000 children (a copy of the bindings for each child was
    // about 29 GB), and a shape of 300,000 declarations and 300,000 prefixed attributes (a scan of the bindings for each was 4.5e10 compares).
    let (k, n) = (30_000_usize, 30_000_usize);
    let declarations: String = (0..k).map(|i| format!(" xmlns:p{i}=\"urn:p{i}\"")).collect();
    let children: String = (0..n).map(|i| format!("<Child p{i}:a=\"1\"/>")).collect();
    let wide = shape(5, &format!("{}<Mystery{declarations}>{children}</Mystery>", box_cells(1.0, 1.0)));
    let big = 300_000_usize;
    let declared: String = (0..big).map(|i| format!(" xmlns:q{i}=\"urn:q{i}\"")).collect();
    let prefixed: String = (0..big).map(|i| format!(" q{i}:a=\"1\"")).collect();
    let attrs = shape_with(5, &format!("{declared}{prefixed}"), &box_cells(1.0, 1.0));
    let bytes = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "Wide").part(&part_with(&wide)))
        .master(MasterSpec::new(2, "Attrs").part(&part_with(&attrs)))
        .master(MasterSpec::new(3, "Plain").shapes(&shape(5, &box_cells(1.0, 1.0))))
        .build();
    println!("file: {} bytes", bytes.len());
    let mut s = blank_session();
    timed("open", || open(&mut s, &dir.write("kept.vssx", &bytes)));
    for name in ["Wide", "Attrs"] {
        let msg = timed(&format!("refuse {name}"), || {
            refusal(&mut s, "shape.drop", json!({"master": name, "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
        });
        assert!(msg.contains(name) && msg.contains("namespace prefixes"), "{msg}");
    }
    // The refusals left nothing behind, and a plain master of the same file drops.
    assert!(s.document().unwrap().masters.is_empty());
    run(&mut s, "shape.drop", json!({"master": "Plain", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!(check_session(&s), Ok(()));
}

// ----------------------------------------------------------------------------------------------------------------- 3: the style search

#[test]
fn twenty_thousand_alike_named_styles_against_forty_five_thousand_that_differ_only_in_a_kept_attribute_cost_a_hash_each() {
    let dir = Dir::new();
    // The re-review's fixture: a stencil of 20,000 styles called "Same" (each linked to the root, with one cell) reached by 20,000 master shapes, and a
    // drawing of 45,000 styles called "Same" whose cell is the same but for an attribute the model has no field for (`X="k"`, kept in the cell's `raw`).
    // A hash that left the kept attributes out put all 45,000 in one bucket with the chain's styles, 9e8 comparisons.
    let (n, m) = (20_000_u32, 45_000_u32);
    let chain: String = (1..=n)
        .map(|k| format!("<StyleSheet ID=\"{k}\" NameU=\"Same\" Name=\"Same\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"1\"/></StyleSheet>"))
        .collect();
    let others: String = (1..=m)
        .map(|k| {
            format!("<StyleSheet ID=\"{k}\" NameU=\"Same\" Name=\"Same\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Cell N=\"LineWeight\" V=\"1\" X=\"{k}\"/></StyleSheet>")
        })
        .collect();
    let members: String = (0..n).map(|k| shape_with(10 + k, &format!(" LineStyle=\"{}\"", k + 1), r#"<Cell N="Width" V="1"/>"#)).collect();
    let group = shape_with(5, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(2.0, 2.0)));
    let stencil = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .more_styles(&chain)
        .master(MasterSpec::new(1, "Deep").attrs(&ids(61, 62)).shapes(&group))
        .build();
    let target = VsdxBuilder::new().more_styles(&others).page(PageSpec::new(1, "Page-1")).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("same.vssx", &stencil));
    let r = timed("drop 20,000 styles into a drawing of 45,000 alike-named", || {
        run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 3, "y": 3}))
    });
    assert_eq!(r["copied"]["styles"], 20_000, "none of the drawing's is the same");
    assert_eq!(s.document().unwrap().styles.len(), 1 + 45_000 + 20_000);
    let again = timed("drop again", || run(&mut s, "shape.drop", json!({"master": "Deep", "stencil": "file-1", "page": 1, "x": 6, "y": 3})));
    assert_eq!(again["master_added"], false);
}

// ------------------------------------------------------------------------------------------------------- 5: a line in a dropped group

/// A group master of two top-level shapes far from the origin of the master's page: a box 2 by 1 at (10..12, 5..6), pin in the middle, and a line from (12, 6)
/// to (14, 6) whose pin is held as **numbers** (13, 6) and not as a formula of its ends. The joint box is from (10, 5) to (14, 6).
fn line_and_box_stencil() -> Vec<u8> {
    let a = shape(
        5,
        &format!(
            "{LOOK}<Cell N=\"PinX\" V=\"11\"/><Cell N=\"PinY\" V=\"5.5\"/><Cell N=\"Width\" V=\"2\"/><Cell N=\"Height\" V=\"1\"/><Cell N=\"LocPinX\" V=\"1\"/><Cell N=\"LocPinY\" V=\"0.5\"/><Section N=\"Geometry\" IX=\"0\"><Row T=\"MoveTo\" IX=\"1\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row><Row T=\"LineTo\" IX=\"2\"><Cell N=\"X\" V=\"2\"/><Cell N=\"Y\" V=\"0\"/></Row><Row T=\"LineTo\" IX=\"3\"><Cell N=\"X\" V=\"2\"/><Cell N=\"Y\" V=\"1\"/></Row><Row T=\"LineTo\" IX=\"4\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"1\"/></Row><Row T=\"LineTo\" IX=\"5\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row></Section>"
        ),
    );
    let line = shape(
        6,
        r#"<Cell N="BeginX" V="12"/><Cell N="BeginY" V="6"/><Cell N="EndX" V="14"/><Cell N="EndY" V="6"/>
<Cell N="PinX" V="13"/><Cell N="PinY" V="6"/><Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0"/>
<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.05"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Rail").attrs(&ids(41, 42)).shapes(&format!("{a}{line}"))).build()
}

#[test]
fn a_line_among_the_members_of_a_dropped_group_is_where_its_ends_are_when_its_master_holds_its_pin_as_numbers() {
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("rail.vssx", &line_and_box_stencil()));
    let r = run(&mut s, "shape.drop", json!({"master": "Rail", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(4.0), Some(1.0)));
    let d = s.document().unwrap();
    let group = d.page(1).unwrap().shapes.get(0).unwrap();
    let line_id = group.children.get(1).unwrap().id;
    // In the group's frame (its origin at the box's lower-left corner, (10, 5) on the master's page) the line runs from (2, 1) to (4, 1) with its pin
    // at (3, 1), not at the master's (13, 6).
    assert_eq!(
        (num(d, 1, line_id, "BeginX"), num(d, 1, line_id, "BeginY"), num(d, 1, line_id, "EndX"), num(d, 1, line_id, "EndY")),
        (2.0, 1.0, 4.0, 1.0)
    );
    assert_eq!((num(d, 1, line_id, "PinX"), num(d, 1, line_id, "PinY")), (3.0, 1.0), "the pin follows the ends");
    let line = group.children.get(1).unwrap();
    assert!(
        line.sheet.cells.get("PinX").is_some_and(|c| !matches!(c.formula, FormulaSlot::Inherit)),
        "the master holds the pin as a number, so the member holds its own"
    );
    // And it draws there: the group's box is centred on (6, 4), so its origin is (4, 3.5) and the line is at y 4.5 from x 6 to 8.
    let view = diacraft_render::View::fit_page(d, 1, 100.0).unwrap();
    let r = diacraft_render::Renderer::new().render(d, 1, &view);
    let ink = |x: f64, y: f64| {
        let (w, h) = (r.width as usize, r.height as usize);
        let at = ((((8.5 - y) * 100.0) as usize).min(h - 1) * w + ((x * 100.0) as usize).min(w - 1)) * 4;
        u32::from(r.rgba_premul[at]) + u32::from(r.rgba_premul[at + 1]) + u32::from(r.rgba_premul[at + 2]) < 200
    };
    assert!(ink(7.0, 4.5), "the line is on the page at (7, 4.5)");
    assert!(!ink(7.0, 4.0) && !ink(11.0, 10.5 - 6.0), "and not offset by the box's corner");
}

// ------------------------------------------------------------------------------------------- 6: glue to a member, and the connection points

/// A master of two boxes side by side, each with the four connection points of `box_cells`, the group 4 by 1.
fn twin_stencil() -> Vec<u8> {
    let left = shape(5, &format!("{LOOK}{}", box_cells(2.0, 1.0)));
    let right =
        shape(6, &format!("{LOOK}{}", box_cells(2.0, 1.0).replace("<Cell N=\"PinX\" V=\"1\" F=\"Width*0.5\"/>", "<Cell N=\"PinX\" V=\"3\"/>")));
    VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Twin").attrs(&ids(51, 52)).shapes(&format!("{left}{right}"))).build()
}

#[test]
fn a_master_of_several_shapes_lists_the_connection_points_of_all_of_them() {
    let dir = Dir::new();
    let mut s = blank_session();
    let opened = open(&mut s, &dir.write("twin.vssx", &twin_stencil()));
    assert_eq!(opened["masters"][0]["connection_points"], 8, "four on each of two members");
}

#[test]
fn a_connector_glued_to_a_member_of_a_dropped_group_follows_the_group_when_it_moves() {
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("twin.vssx", &twin_stencil()));
    let dropped = run(&mut s, "shape.drop", json!({"master": "Twin", "stencil": "file-1", "page": 1, "x": 4.0, "y": 4.0}));
    let (group, left, right) = (dropped["id"].as_u64().unwrap() as u32, 2_u32, 3_u32);
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.get(0).unwrap().children.iter().map(|m| m.id).collect::<Vec<_>>(), [left, right]);
    let rect = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 2.0}))["id"].as_u64().unwrap() as u32;
    // The right-hand member's right-hand point (index 3) to the rectangle's left-hand one (index 1).
    let added = run(&mut s, "connector.add", json!({"from": {"id": right, "point": 3}, "to": {"id": rect, "point": 1}}));
    assert_eq!(added["begin"], json!({"id": right, "point": 3}), "glued to the member");
    let connector = added["id"].as_u64().unwrap() as u32;
    let point = |s: &Session, id: u32, index: u32| {
        let d = s.document().unwrap();
        let p = connection_points(d, 1, id).into_iter().find(|p| p.index == index).unwrap();
        (p.x, p.y)
    };
    let (before, begin) =
        (point(&s, right, 3), (num(s.document().unwrap(), 1, connector, "BeginX"), num(s.document().unwrap(), 1, connector, "BeginY")));
    assert!((before.0 - begin.0).abs() < 1e-9 && (before.1 - begin.1).abs() < 1e-9, "{before:?} {begin:?}");
    // The group's frame: 4 by 1 centred on (4, 4), so the right box's right edge is at x 6 and its middle at y 4.
    assert!((before.0 - 6.0).abs() < 1e-9 && (before.1 - 4.0).abs() < 1e-9, "{before:?}");
    // Move the group, and the connector's end goes with the member it is glued to.
    run(&mut s, "shape.move", json!({"ids": [group], "dx": 2.0, "dy": 1.5}));
    let after = point(&s, right, 3);
    assert!((after.0 - 8.0).abs() < 1e-9 && (after.1 - 5.5).abs() < 1e-9, "{after:?}");
    let d = s.document().unwrap();
    let (bx, by) = (num(d, 1, connector, "BeginX"), num(d, 1, connector, "BeginY"));
    assert!((bx - after.0).abs() < 1e-9 && (by - after.1).abs() < 1e-9, "the connector's end ({bx}, {by}) is at the member's point {after:?}");
    // The far end stayed with the rectangle.
    assert!((num(d, 1, connector, "EndX") - 8.25).abs() < 1e-9);
    assert_eq!(check_session(&s), Ok(()));
}
