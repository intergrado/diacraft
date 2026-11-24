//! AutoConnect ("Drop a master on an AutoConnect arrow"): `connector.autoconnect`.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, Value, WriteOrigin};
use diacraft_engine::EngineError;
use diacraft_engine::draw::route::Dir;
use diacraft_engine::draw::{autoconnect_centre, connection_points, shape_corners};
use serde_json::json;

use crate::connect_style::cell;
use crate::edit::{doc, drop_shape, route};
use crate::stencil_open::with_stencil;
use crate::{blank_session, check_session, run, snapshot};

type P = (f64, f64);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_point(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// The box of a shape on the page: left, bottom, right, top.
fn bounds(d: &Document, id: u32) -> (f64, f64, f64, f64) {
    let c = shape_corners(d, 1, id).unwrap();
    let (xs, ys): (Vec<f64>, Vec<f64>) = c.iter().copied().unzip();
    (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

fn centre(d: &Document, id: u32) -> P {
    let b = bounds(d, id);
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

/// The place and the page direction of connection point `ix` of shape `id`.
fn point(d: &Document, id: u32, ix: u32) -> (P, P) {
    let p = connection_points(d, 1, id).into_iter().find(|p| p.index == ix).unwrap();
    ((p.x, p.y), (p.dir_x, p.dir_y))
}

#[test]
fn an_arrow_makes_a_copy_beside_the_shape_and_a_connector_from_the_point_that_faces_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
    let (shape, connector) = (r["id"].as_u64().unwrap() as u32, r["connector"].as_u64().unwrap() as u32);
    assert_eq!((shape, connector), (2, 3));
    // The rectangle is 1.5 by 1 at (3, 4): its box ends at x 3.75, the gap is 0.75, the copy is as wide and centred on the same height.
    assert_eq!(r["begin"], json!({"id": 1, "point": 3}));
    assert_eq!(r["end"], json!({"id": 2, "point": 1}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1.5), Some(1.0)));
    assert!(near(r["x"].as_f64().unwrap(), 5.25) && near(r["y"].as_f64().unwrap(), 4.0), "{r}");
    let d = doc(&s);
    assert!(near_point(centre(d, shape), (5.25, 4.0)));
    assert_eq!(
        d.page(1).unwrap().shapes.iter().find(|s| s.id == shape).unwrap().master,
        d.page(1).unwrap().shapes.iter().find(|s| s.id == a).unwrap().master,
        "a copy is of the same master"
    );
    // The connector runs from the right point of the first to the left point of the copy, one leg of three quarters of an inch.
    let r = route(d, connector);
    assert!(near_point(r[0], (3.75, 4.0)) && near_point(r[r.len() - 1], (4.5, 4.0)), "{r:?}");
    assert_eq!(r.len(), 2);
    assert_eq!(cell(d, connector, "EndArrow"), Some(4.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn each_of_the_four_arrows_goes_its_own_way_and_glues_the_points_that_face_each_other() {
    for (direction, want) in [("right", (5.25, 4.0)), ("left", (0.75, 4.0)), ("up", (3.0, 5.75)), ("down", (3.0, 2.25))] {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
        let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": direction}));
        let d = doc(&s);
        let shape = r["id"].as_u64().unwrap() as u32;
        // 1.5 wide and 1 high: a gap of 0.75 and half the width (or the height) to the centre of the copy.
        assert!(near_point(centre(d, shape), want), "{direction}: {:?} for {want:?}", centre(d, shape));
        let (begin, end) = (r["begin"]["point"].as_u64().unwrap() as u32, r["end"]["point"].as_u64().unwrap() as u32);
        let (out, back) = (point(d, a, begin).1, point(d, shape, end).1);
        let dir = match direction {
            "right" => (1.0, 0.0),
            "left" => (-1.0, 0.0),
            "up" => (0.0, 1.0),
            _ => (0.0, -1.0),
        };
        assert!(near_point(out, dir) && near_point(back, (-dir.0, -dir.1)), "{direction}: leaves {out:?}, arrives against {back:?}");
        // The connector is straight, and the same length as the gap.
        let line = route(d, r["connector"].as_u64().unwrap() as u32);
        assert_eq!(line.len(), 2, "{direction}: {line:?}");
        assert!(near(((line[1].0 - line[0].0).powi(2) + (line[1].1 - line[0].1).powi(2)).sqrt(), 0.75), "{direction}: {line:?}");
    }
}

#[test]
fn a_chosen_master_is_dropped_beside_the_shape_as_shape_drop_would_and_the_page_gap_is_used() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // The page's AvenueSizeX is 2: the gap there.
    s.edit("avenue", |d| {
        d.write_cell(&diacraft_doc::SheetRef::Page(1), &CellAddr::direct("AvenueSizeX"), CellEdit::Value(Value::from_text("2")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "master": "ellipse"}));
    assert_eq!(r["name"], "Ellipse.2");
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1.5), Some(1.0)));
    let shape = r["id"].as_u64().unwrap() as u32;
    let d = doc(&s);
    assert!(near_point(centre(d, shape), (3.75 + 2.0 + 0.75, 4.0)), "{:?}", centre(d, shape));
    // The master is the document's now, and the shape an instance of it, as a drop leaves it.
    assert!(d.masters.iter().any(|m| m.name.as_deref() == Some("Ellipse")));
    // A master of the flowchart stencil, named with its stencil.
    let r = run(&mut s, "connector.autoconnect", json!({"from": shape, "direction": "down", "master": "Decision", "stencil": "flowchart"}));
    assert_eq!(r["name"], "Decision.4");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_master_of_an_opened_stencil_is_dropped_the_way_shape_drop_drops_it() {
    let (mut s, _dir) = with_stencil();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "master": "Process", "stencil": "file-1"}));
    assert!(near(r["width"].as_f64().unwrap(), 1.5) && near(r["height"].as_f64().unwrap(), 0.8), "{r}");
    let shape = r["id"].as_u64().unwrap() as u32;
    let d = doc(&s);
    assert!(near_point(centre(d, shape), (5.25, 4.0)), "{:?}", centre(d, shape));
    assert_eq!(r["begin"]["point"], json!(3));
    assert_eq!(r["end"]["point"], json!(1));
    let e = s.execute("connector.autoconnect", &json!({"from": a, "direction": "right", "master": "Nothing", "stencil": "file-1"})).unwrap_err();
    assert!(e.to_string().contains("no master"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_second_arrow_on_the_same_side_makes_the_next_shape_beside_the_first_and_not_on_top_of_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let first = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}))["id"].as_u64().unwrap() as u32;
    let second = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}))["id"].as_u64().unwrap() as u32;
    let third = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}))["id"].as_u64().unwrap() as u32;
    let d = doc(&s);
    let boxes = [bounds(d, first), bounds(d, second), bounds(d, third)];
    for (i, p) in boxes.iter().enumerate() {
        for q in boxes.iter().skip(i + 1) {
            assert!(p.2 <= q.0 + 1e-9 || q.2 <= p.0 + 1e-9 || p.3 <= q.1 + 1e-9 || q.3 <= p.1 + 1e-9, "{p:?} over {q:?}");
        }
    }
    // They are in one column, a box and a gap apart: 1 + 0.75 high.
    let ys: Vec<f64> = [first, second, third].iter().map(|id| centre(d, *id).1).collect();
    assert!(near(ys[0], 4.0) && near(ys[1], 5.75) && near(ys[2], 2.25), "{ys:?}");
    assert!([first, second, third].iter().all(|id| near(centre(d, *id).0, 5.25)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_arrow_of_a_turned_or_flipped_shape_uses_the_point_that_faces_that_way_on_the_page() {
    for (what, command) in [
        ("turned a quarter", json!({"ids": [1], "by": 90})),
        ("turned half", json!({"ids": [1], "by": 180})),
        ("flipped", json!({"ids": [1], "axis": "horizontal"})),
    ] {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
        if what == "flipped" {
            run(&mut s, "shape.flip", command);
        } else {
            run(&mut s, "shape.rotate", command);
        }
        let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
        let d = doc(&s);
        let begin = r["begin"]["point"].as_u64().unwrap() as u32;
        let (at, dir) = point(d, a, begin);
        assert!(near_point(dir, (1.0, 0.0)), "{what}: the point faces {dir:?}");
        let box_a = bounds(d, a);
        assert!(near(at.0, box_a.2), "{what}: it is on the right side of the box");
        let shape = r["id"].as_u64().unwrap() as u32;
        assert!(near(bounds(d, shape).0, box_a.2 + 0.75), "{what}: the gap is from the box on the page");
    }
}

#[test]
fn a_copy_has_the_look_and_the_size_of_the_shape_and_not_its_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "ellipse", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Start"}));
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#336699", "weight": 2}));
    run(&mut s, "shape.resize", json!({"id": a, "x": 2.0, "y": 3.5, "width": 2.0, "height": 1.0}));
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "down"}));
    let shape = r["id"].as_u64().unwrap() as u32;
    let d = doc(&s);
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(2.0), Some(1.0)), "the size of the shape as it is");
    let text_of = |id: u32| d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().text.is_some();
    assert!(text_of(a) && !text_of(shape), "the copy is made to be written in");
    let s_after = run(&mut s, "shape.formatting", json!({"ids": [a, shape]}));
    assert_eq!(s_after["shapes"][0]["fill"], s_after["shapes"][1]["fill"], "the same fill");
    assert_eq!(s_after["shapes"][0]["weight"], s_after["shapes"][1]["weight"], "the same line");
}

#[test]
fn a_shape_with_no_connection_point_is_refused_like_connector_add_and_leaves_no_copy_behind() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "ellipse", 3.5, 4.0);
    // A group has no connection point of its own, and a connector cannot be glued to it by point.
    let group = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap() as u32;
    let was = snapshot(&s);
    let e = s.execute("connector.autoconnect", &json!({"from": group, "direction": "right"})).unwrap_err();
    assert!(e.to_string().contains("no connection points"), "{e}");
    assert_eq!(snapshot(&s), was, "the copy that was made is not left on the page");
}

#[test]
fn a_refused_arrow_changes_nothing_and_says_why() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let was = snapshot(&s);
    for (what, params, words) in [
        ("a connector", json!({"from": c, "direction": "right"}), "connector"),
        ("a shape that is not there", json!({"from": 99, "direction": "right"}), "no shape 99"),
        ("a direction that is not one", json!({"from": a, "direction": "northwest"}), "direction"),
        ("no direction", json!({"from": a}), "direction"),
        ("a master that is not there", json!({"from": a, "direction": "up", "master": "nothing"}), "no master"),
        ("a stencil without a master", json!({"from": a, "direction": "up", "stencil": "basic"}), "stencil"),
        ("a stencil that is not open", json!({"from": a, "direction": "up", "master": "x", "stencil": "file-9"}), "no stencil"),
        ("a page that is not there", json!({"from": a, "direction": "up", "page": 9}), "page"),
    ] {
        let e = s.execute("connector.autoconnect", &params).unwrap_err();
        assert!(e.to_string().contains(words), "{what}: {e}");
        assert_eq!(snapshot(&s), was, "{what}");
    }
}

#[test]
fn it_is_one_undo_step_and_the_ids_are_never_given_twice() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let before = snapshot(&s);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document, "the shape, its move and the connector come back together");
    let again = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
    assert!(again["id"].as_u64() > r["id"].as_u64(), "an id the session gave is not given again: {r} then {again}");
    assert!(again["connector"].as_u64() > r["connector"].as_u64());
}

#[test]
fn a_window_asks_where_a_shape_of_a_size_would_go_for_its_preview() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let c = autoconnect_centre(doc(&s), 1, a, Dir::Up, (1.0, 0.5)).unwrap();
    assert!(near_point(c, (3.0, 4.5 + 0.75 + 0.25)), "{c:?}");
    let r = run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"x": 9, "y": 9}}));
    assert_eq!(autoconnect_centre(doc(&s), 1, crate::edit::id_of(&r), Dir::Up, (1.0, 1.0)), None, "a connector has no arrows");
    assert_eq!(autoconnect_centre(doc(&s), 1, 99, Dir::Up, (1.0, 1.0)), None);
}

#[test]
fn a_shape_made_past_the_edge_of_a_page_with_auto_size_grows_the_page_in_the_same_undo_step() {
    use crate::connect_style::page_cell;
    // To the right: the new shape's box reaches 12.5 past the edge at 11, and the page takes a sheet (11 inches) more; nothing is moved.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 9.5, 4.0);
    run(&mut s, "page.auto_size", json!({"on": true}));
    let before = snapshot(&s);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
    let d = doc(&s);
    assert_eq!(page_cell(d, "PageWidth"), Some(22.0));
    assert!(near_point(centre(d, a), (9.5, 4.0)) && near_point(centre(d, r["id"].as_u64().unwrap() as u32), (11.75, 4.0)), "nothing moved");
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1, "the shape, the connector and the growth are one step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    assert_eq!(check_session(&s), Ok(()));

    // To the left: the box reaches -2 and the page grows on its left, which moves everything on it a sheet to the right, the new shape and
    // its connector with it; the answer says where the shape is now.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 1.0, 4.0);
    run(&mut s, "page.auto_size", json!({"on": true}));
    let before = snapshot(&s);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "left"}));
    let (shape, connector) = (r["id"].as_u64().unwrap() as u32, r["connector"].as_u64().unwrap() as u32);
    let d = doc(&s);
    assert_eq!(page_cell(d, "PageWidth"), Some(22.0));
    assert!(near_point(centre(d, a), (12.0, 4.0)), "the old shape was carried by the growth: {:?}", centre(d, a));
    assert!(near_point(centre(d, shape), (9.75, 4.0)), "{:?}", centre(d, shape));
    assert!(near(r["x"].as_f64().unwrap(), 9.75) && near(r["y"].as_f64().unwrap(), 4.0), "the answer is read after the growth: {r}");
    let line = route(d, connector);
    assert!(near_point(line[0], (11.25, 4.0)) && near_point(line[line.len() - 1], (10.5, 4.0)), "the connector runs across the gap: {line:?}");
    assert_eq!(line.len(), 2);
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    // With Auto Size off the page stays and the shape is off it.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 1.0, 4.0);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "left"}));
    assert_eq!(page_cell(doc(&s), "PageWidth"), Some(11.0));
    assert!(near(r["x"].as_f64().unwrap(), -1.25), "{r}");
}
