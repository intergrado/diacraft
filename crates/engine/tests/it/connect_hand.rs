//! A connector's path edited by hand, and Reset Connector.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::draw::{ConnectorPath, PathStyle, connector_path};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::connect_style::{cell, z_pair};
use crate::edit::{doc, drop_shape, geometry, id_of, num, route};
use crate::edit_foreign::{Dir, opened};
use crate::{Snapshot, check_session, run, snapshot};

type P = (f64, f64);

fn quiet(s: &Session) -> Snapshot {
    Snapshot { journal: Vec::new(), ..snapshot(s) }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_point(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn assert_route(got: &[P], want: &[P]) {
    assert!(got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near_point(*g, *w)), "got {got:?}, want {want:?}");
}

fn path(d: &Document, id: u32) -> ConnectorPath {
    connector_path(d, 1, id).unwrap()
}

fn ends(d: &Document, id: u32) -> (P, P) {
    ((num(d, id, "BeginX"), num(d, id, "BeginY")), (num(d, id, "EndX"), num(d, id, "EndY")))
}

#[test]
fn a_window_reads_the_legs_and_what_it_may_do_with_them() {
    let (mut s, _, _, c) = z_pair();
    let p = path(doc(&s), c);
    assert_route(&p.points, &[(3.75, 4.0), (6.0, 4.0), (6.0, 6.0), (8.25, 6.0)]);
    assert_eq!((p.style, p.hand_edited, p.editable), (PathStyle::RightAngle, false, true));
    run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    let p = path(doc(&s), c);
    assert_eq!((p.style, p.points.len(), p.editable), (PathStyle::Straight, 2, false), "a straight connector has no legs to move");
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [c]}));
    assert_eq!(connector_path(doc(&s), 1, c), None, "a curve is not a path of lines");
    assert_eq!(connector_path(doc(&s), 1, 1), None, "a shape that is no connector");
    assert_eq!(connector_path(doc(&s), 1, 99), None);
    assert_eq!(connector_path(doc(&s), 9, 1), None);
}

#[test]
fn the_middle_leg_moved_by_hand_stretches_its_neighbours_and_the_ends_stay() {
    let (mut s, _, _, c) = z_pair();
    let ends_before = ends(doc(&s), c);
    let before = quiet(&s);
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    assert_eq!(r, json!({"page": 1, "id": c, "changed": true}));
    assert_eq!(quiet(&s).undo_steps, before.undo_steps + 1, "one undo step");
    let d = doc(&s);
    assert_route(&route(d, c), &[(3.75, 4.0), (7.0, 4.0), (7.0, 6.0), (8.25, 6.0)]);
    assert_eq!(ends(d, c), ends_before, "the glued ends do not move");
    assert_eq!(cell(d, c, "ConFixedCode"), Some(3.0), "A path edited by hand writes 3");
    let p = path(d, c);
    assert!(p.hand_edited && p.editable);
    // The label sits on the new middle leg.
    assert!(near(num(d, c, "EndArrow"), 4.0));
    // Undo takes the route and the code in one step.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(quiet(&s).document, before.document);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_leg_at_an_end_gets_a_jog_and_the_path_still_leaves_by_the_connection_point() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (6.0, 3.0), (6.0, 6.0), (8.25, 6.0)]);
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 4, "to": 7.0}));
    let r = route(doc(&s), c);
    assert!(near_point(r[0], (3.75, 4.0)) && near_point(r[r.len() - 1], (8.25, 6.0)), "{r:?}");
    assert!(near(r[r.len() - 1].1, 6.0));
    assert!(r.windows(2).all(|w| near(w[0].0, w[1].0) != near(w[0].1, w[1].1)), "only level and upright legs: {r:?}");
}

#[test]
fn the_edited_route_is_fixed_the_ends_follow_their_shapes_and_the_last_row_follows_the_width() {
    let (mut s, _, b, c) = z_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    let rows_before = geometry(doc(&s), c);
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 1.0, "dy": 0.5}));
    let d = doc(&s);
    let (begin, end) = ends(d, c);
    assert!(near_point(end, (9.25, 6.5)), "the end follows B: {end:?}");
    assert_eq!(geometry(d, c).len(), rows_before.len(), "The route is not made again, it is followed");
    let r = route(d, c);
    assert!(near_point(r[0], begin) && near_point(r[r.len() - 1], end), "and still runs from the begin to the end: {r:?}");
}

#[test]
fn a_leg_already_on_the_line_a_repeat_and_a_bad_leg_change_nothing() {
    let (mut s, a, _, c) = z_pair();
    let was = quiet(&s);
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 6.0}));
    assert_eq!(r["changed"], json!(false));
    assert_eq!(quiet(&s), was, "no edit, no undo step, and the path is not marked edited");
    for (what, params) in [
        ("a leg that is not there", json!({"id": c, "leg": 3, "to": 1.0})),
        ("a leg of the largest number", json!({"id": c, "leg": 4_294_967_295_u64, "to": 1.0})),
        ("a shape that is no connector", json!({"id": a, "leg": 0, "to": 1.0})),
        ("a place off the page", json!({"id": c, "leg": 1, "to": 1.0e9})),
        ("a shape that is not there", json!({"id": 99, "leg": 0, "to": 1.0})),
        ("a missing leg", json!({"id": c, "to": 1.0})),
    ] {
        let e = s.execute("connector.edit_path", &params);
        assert!(e.is_err(), "{what}");
        assert_eq!(snapshot(&s).document, was.document, "{what}");
    }
    let e = s.execute("connector.edit_path", &json!({"id": c, "leg": 3, "to": 1.0})).unwrap_err();
    assert!(e.to_string().contains("no leg 3"), "{e}");
    // A straight or curved connector has no legs.
    run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    let e = s.execute("connector.edit_path", &json!({"id": c, "leg": 0, "to": 1.0})).unwrap_err();
    assert!(e.to_string().contains("straight or curved"), "{e}");
}

#[test]
fn a_path_that_would_fold_back_on_itself_is_refused_in_words_and_nothing_changes() {
    let (mut s, a, b, _) = z_pair();
    // A hook: both points face right and B is level with A's point after the shapes are moved, so the route has five legs; moving a middle leg onto the
    // first leg's line would run the path over itself.
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": -2.0}));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 3}})));
    let was = quiet(&s);
    let r = route(doc(&s), c);
    assert!(r.len() >= 5, "{r:?}");
    let mut refused = 0;
    for leg in 0..r.len() - 1 {
        for to in [0.0, 1.0, 4.0, 4.25, 4.75, 6.0, 10.0] {
            match s.execute("connector.edit_path", &json!({"id": c, "leg": leg, "to": to})) {
                Err(e) => {
                    assert!(e.to_string().contains("fold back"), "{e}");
                    refused += 1;
                    assert_eq!(
                        (snapshot(&s).document, snapshot(&s).undo_steps),
                        (was.document.clone(), was.undo_steps),
                        "a refused edit changes nothing"
                    );
                }
                Ok(done) if done["changed"] == json!(true) => {
                    run(&mut s, "edit.undo", json!({}));
                    assert_eq!(snapshot(&s).document, was.document, "undo");
                }
                Ok(_) => assert_eq!(
                    (snapshot(&s).document, snapshot(&s).undo_steps),
                    (was.document.clone(), was.undo_steps),
                    "a leg already on the line is no edit"
                ),
            }
        }
    }
    assert!(refused > 0, "some position folds the path back");
}

#[test]
fn reset_connector_takes_the_route_back_to_the_routed_one_and_the_code_to_0() {
    let (mut s, _, _, c) = z_pair();
    let routed = route(doc(&s), c);
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    let was = quiet(&s);
    let r = run(&mut s, "connector.reset", json!({"ids": [c]}));
    assert_eq!(r, json!({"page": 1, "reset": 1}));
    assert_eq!(quiet(&s).undo_steps, was.undo_steps + 1);
    let d = doc(&s);
    assert_route(&route(d, c), &routed);
    assert_eq!(cell(d, c, "ConFixedCode"), Some(0.0), "Reset Connector writes 0");
    assert!(!path(d, c).hand_edited);
    // A connector that was not edited has nothing to reset, and a second reset is no step.
    let again = quiet(&s);
    let r = run(&mut s, "connector.reset", json!({"ids": [c]}));
    assert_eq!(r["reset"], json!(0));
    assert_eq!(quiet(&s), again);
    // After a reset the connector is routed again when its shape moves.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 1.0}));
    assert_eq!(geometry(doc(&s), c).len(), 4, "a Z of the new place");
    for bad in [json!({"ids": [1]}), json!({"ids": [99]}), json!({"ids": []}), json!({})] {
        let was = snapshot(&s).document;
        assert!(s.execute("connector.reset", &bad).is_err(), "{bad}");
        assert_eq!(snapshot(&s).document, was);
    }
}

#[test]
fn a_reconnected_connector_that_was_edited_by_hand_is_routed_by_its_new_ends_and_is_no_longer_edited() {
    let (mut s, a, b, c) = z_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    let third = drop_shape(&mut s, "rectangle", 6.0, 9.0);
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": third, "point": 1}}));
    let d = doc(&s);
    assert_eq!(cell(d, c, "ConFixedCode"), Some(0.0));
    let r = route(d, c);
    assert!(near_point(r[0], (3.75, 4.0)) && near_point(r[r.len() - 1], (5.25, 9.0)), "{r:?}");
    let _ = (a, b);
}

#[test]
fn a_file_connector_of_one_straight_leg_gets_a_jog_at_both_ends_and_keeps_its_label_on_the_middle() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    // Connector 20 runs from A's right point (3, 2) to B's left point (7, 2), a straight run of two rows, with the label "to B".
    let before = path(doc(&s), 20);
    assert_route(&before.points, &[(3.0, 2.0), (7.0, 2.0)]);
    run(&mut s, "connector.edit_path", json!({"id": 20, "leg": 0, "to": 3.0}));
    let d = doc(&s);
    assert_route(&route(d, 20), &[(3.0, 2.0), (3.25, 2.0), (3.25, 3.0), (6.75, 3.0), (6.75, 2.0), (7.0, 2.0)]);
    assert_eq!(cell(d, 20, "ConFixedCode"), Some(3.0));
    // The middle leg of the five is the one at height 3, and the label is centred on it and upright.
    let n = |name: &str| num(d, 20, name);
    let (tx, ty) = (n("TxtPinX"), n("TxtPinY"));
    let pose = diacraft_engine::draw::Pose {
        pin: (n("PinX"), n("PinY")),
        loc: (n("LocPinX"), n("LocPinY")),
        angle: n("Angle"),
        flip_x: false,
        flip_y: false,
    };
    assert!(near_point(pose.up((tx, ty)), (5.0, 3.0)), "{:?}", pose.up((tx, ty)));
    // The other connectors of the file are as the file had them.
    assert_eq!(check_session(&s), Ok(()));
}

/// A drawing whose connector inherits a long `Geometry` section from its master and holds a short route of its own.
fn inheriting_drawing() -> Vec<u8> {
    let master = shape(
        5,
        r#"<Cell N="Width" V="1"/><Cell N="Height" V="0"/><Cell N="PinX" V="0.5" F="Width*0.5"/><Cell N="PinY" V="0"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="2"/><Cell N="Y" V="2"/></Row><Row T="LineTo" IX="6"><Cell N="X" V="3"/><Cell N="Y" V="2"/></Row>
<Row T="LineTo" IX="7"><Cell N="X" V="3"/><Cell N="Y" V="3"/></Row><Row T="LineTo" IX="8"><Cell N="X" V="4"/><Cell N="Y" V="3"/></Row></Section>"#,
    );
    let rows_of_the_instance = (3..=8).map(|ix| format!(r#"<Row T="LineTo" IX="{ix}" Del="1"/>"#)).collect::<String>();
    let connector = shape_with(
        20,
        r#" Master="9""#,
        &format!(
            r#"<Cell N="BeginX" V="2"/><Cell N="BeginY" V="2"/><Cell N="EndX" V="6"/><Cell N="EndY" V="2"/>
<Cell N="PinX" V="4" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="4"/><Cell N="Y" V="0"/></Row>{rows_of_the_instance}</Section>"#
        ),
    );
    VsdxBuilder::new().master(MasterSpec::new(9, "Connector").shapes(&master)).page(PageSpec::new(1, "Page-1").shapes(&connector)).build()
}

#[test]
fn a_connector_that_inherits_rows_from_its_master_gets_a_route_with_no_row_of_the_masters_left_in_it() {
    let dir = Dir::new();
    let path_name = dir.path("inherit.vsdx");
    std::fs::write(&path_name, inheriting_drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path_name}));
    assert_eq!(geometry(doc(&s), 20).len(), 2, "the instance's own two rows and its deletions of the rest");
    // A route of three rows, then of five: the master's eight rows do not come back, and a row it has past the route is deleted.
    run(&mut s, "connector.edit_path", json!({"id": 20, "leg": 0, "to": 3.0}));
    assert_eq!(geometry(doc(&s), 20).len(), 6, "the six points of the route and nothing of the master's seventh and eighth rows");
    assert_route(&route(doc(&s), 20), &[(2.0, 2.0), (2.25, 2.0), (2.25, 3.0), (5.75, 3.0), (5.75, 2.0), (6.0, 2.0)]);
    run(&mut s, "connector.reset", json!({"ids": [20]}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_of_ours_with_free_ends_that_a_command_moves_is_routed_by_its_ends_again_and_is_no_longer_edited() {
    let mut s = crate::blank_session();
    // A free connector from (2, 2) to (8, 5): a Z. Moved by hand, then moved as a whole: its ends move and the route is made again.
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 2.0}, "to": {"x": 8.0, "y": 5.0}})));
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 6.0}));
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
    run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 1.0}));
    let d = doc(&s);
    assert_eq!(cell(d, c, "ConFixedCode"), Some(0.0), "a command about the connector made the route again");
    assert_route(&route(d, c), &[(3.0, 3.0), (6.0, 3.0), (6.0, 6.0), (9.0, 6.0)]);
    assert_eq!(check_session(&s), Ok(()));
}
