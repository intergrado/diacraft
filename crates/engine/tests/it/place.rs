//! `shape.place`: the Size & Position window's X and Y, an absolute place for the pins of many shapes at once,
//! through `Session::execute` as the control channel calls it. Lengths are paper inches. The place is where `shape.placement` shows the pin on the page.

use diacraft_engine::EngineError;
use diacraft_testkit::package::shape;
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, num, route};
use crate::panels::{P, assert_unchanged, ends, error_of, file_line, group_with_members, near, near_p, open_page, pin, placement, shape_entry};
use crate::{blank_session, check_session, run, snapshot};

/// Where `shape.placement` says the pin of each shape is, `(x, y)` in inches.
fn places(s: &mut diacraft_engine::Session, ids: &[u32]) -> Vec<P> {
    let r = placement(s, ids);
    ids.iter()
        .map(|id| {
            let e = shape_entry(&r, *id);
            (e["x"].as_f64().unwrap(), e["y"].as_f64().unwrap())
        })
        .collect()
}

/// Six rectangles at different places.
fn six(s: &mut diacraft_engine::Session) -> Vec<u32> {
    [(2.0, 1.0), (3.5, 2.5), (4.0, 4.0), (5.25, 5.5), (6.0, 7.0), (7.5, 2.0)].iter().map(|(x, y)| drop_shape(s, "rectangle", *x, *y)).collect()
}

// ---- many shapes, X and Y -----------------------------------------------------------------------------------------------------------

#[test]
fn shapes_at_different_places_are_set_to_one_x_in_one_undo_step_and_each_keeps_its_y() {
    let mut s = blank_session();
    let ids = six(&mut s);
    let was = places(&mut s, &ids);
    let before = snapshot(&s);
    let r = run(&mut s, "shape.place", json!({"ids": ids, "x": 9.25}));
    assert_eq!(r["moved"], 6);
    assert_eq!(r["unplaced"], 0, "every shape had a place to move from");
    assert_eq!(r["page"], 1);
    for (now, was) in places(&mut s, &ids).iter().zip(&was) {
        assert!(near(now.0, 9.25) && near(now.1, was.1), "{now:?} from {was:?}");
    }
    assert_eq!(s.history().unwrap().undo_len(), before.undo_steps + 1, "one undo step for the six shapes");
    // Each pin moved as `shape.move` moves it: a second session, a move of a different distance for each shape, ends in the same places.
    let mut moved = blank_session();
    let same = six(&mut moved);
    for (id, was) in same.iter().zip(&was) {
        run(&mut moved, "shape.move", json!({"ids": [id], "dx": 9.25 - was.0, "dy": 0.0}));
    }
    for id in &ids {
        assert!(near_p(pin(doc(&s), *id), pin(doc(&moved), *id)), "shape {id}");
    }
    // One undo takes all six back, and the redo puts them again.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    run(&mut s, "edit.redo", json!({}));
    assert!(places(&mut s, &ids).iter().all(|p| near(p.0, 9.25)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn y_alone_moves_y_alone_and_x_and_y_together_put_every_shape_at_one_point() {
    let mut s = blank_session();
    let ids = six(&mut s);
    let was = places(&mut s, &ids);
    let r = run(&mut s, "shape.place", json!({"ids": ids, "y": 3.0}));
    assert_eq!(r["moved"], 6);
    for (now, was) in places(&mut s, &ids).iter().zip(&was) {
        assert!(near(now.1, 3.0) && near(now.0, was.0), "{now:?} from {was:?}");
    }
    // A shape that is there already is not counted: after the first call all six are at y = 3, and one is given another x too.
    let again = run(&mut s, "shape.place", json!({"ids": ids, "y": 3.0}));
    assert_eq!(again["moved"], 0);
    let both = run(&mut s, "shape.place", json!({"ids": ids, "x": 4.5, "y": 3.0}));
    assert_eq!(both["moved"], 6, "x differs for all six, and y is the same for all");
    assert!(places(&mut s, &ids).iter().all(|p| near(p.0, 4.5) && near(p.1, 3.0)));
    // Only the cell asked for is written: a `PinX` that is a formula stays one when only y is placed.
    let (mut f, _dir) = open_page(
        &shape(
            1,
            r#"<Cell N="PinX" V="3" F="1+2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#,
        ),
        "",
    );
    run(&mut f, "shape.place", json!({"ids": [1], "y": 5.0}));
    let formula_of = |s: &diacraft_engine::Session| {
        let d = doc(s);
        let shape = d.shape(&d.find_shape(diacraft_doc::Owner::Page(1), 1).unwrap()).unwrap();
        match shape.sheet.lookup(&diacraft_doc::CellAddr::direct("PinX")) {
            diacraft_doc::Lookup::Found(c) => c.formula.text().map(str::to_owned),
            _ => None,
        }
    };
    assert_eq!(formula_of(&f), Some("1+2".to_owned()), "y alone leaves PinX and its formula alone");
    assert!(near(num(doc(&f), 1, "PinY"), 5.0) && near(num(doc(&f), 1, "PinX"), 3.0));
    run(&mut f, "shape.place", json!({"ids": [1], "x": 6.0}));
    assert!(near(num(doc(&f), 1, "PinX"), 6.0));
}

#[test]
fn a_shape_named_twice_is_placed_once_and_a_shape_already_there_leaves_no_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let r = run(&mut s, "shape.place", json!({"ids": [a, a, a], "x": 5.0}));
    assert_eq!(r["moved"], 1);
    let before = snapshot(&s);
    let r = run(&mut s, "shape.place", json!({"ids": [a], "x": 5.0, "y": 3.0}));
    assert_eq!(r["moved"], 0);
    assert_unchanged(&s, &before, "a place the shape is at");
}

// ---- a line -------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_line_moves_both_of_its_ends_and_its_pin_follows() {
    // A line of a file, from (1, 1) to (5, 3): its pin is (3, 2) by its formulas. Placed at (6, 4), it moves by (3, 2).
    let (mut s, _dir) = open_page(&file_line(1, (1.0, 1.0), (5.0, 3.0), ""), "");
    let r = run(&mut s, "shape.place", json!({"ids": [1], "x": 6.0, "y": 4.0}));
    assert_eq!(r["moved"], 1);
    assert_eq!(ends(doc(&s), 1), ((4.0, 3.0), (8.0, 5.0)));
    assert!(near_p(pin(doc(&s), 1), (6.0, 4.0)), "the pin follows by its formulas: {:?}", pin(doc(&s), 1));
    assert_eq!(places(&mut s, &[1]), [(6.0, 4.0)], "and it reads back as the place asked for");
    // A connector of ours with free ends, placed with a box in one call.
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 3.0}, "to": {"x": 4.0, "y": 3.0}})));
    let a = drop_shape(&mut s, "rectangle", 8.0, 6.0);
    let r = run(&mut s, "shape.place", json!({"ids": [c, a], "x": 5.0}));
    assert_eq!(r["moved"], 2);
    assert!(near(places(&mut s, &[c])[0].0, 5.0) && near(places(&mut s, &[a])[0].0, 5.0));
    let (begin, end) = ends(doc(&s), c);
    assert!(near(begin.0, 3.5) && near(end.0, 6.5), "both ends of the connector moved by 2.5: {:?}", (begin, end));
    assert!(route(doc(&s), c).iter().all(|p| p.0.is_finite()), "and the route is whole");
}

// ---- a connector glued --------------------------------------------------------------------------------------------------------------

#[test]
fn connectors_glued_to_a_placed_shape_follow_it_are_routed_again_and_stay_glued() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 8.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let begin_was = (num(doc(&s), c, "BeginX"), num(doc(&s), c, "BeginY"));
    let r = run(&mut s, "shape.place", json!({"ids": [a], "x": 3.0, "y": 5.0}));
    assert_eq!(r["moved"], 1);
    let d = doc(&s);
    let begin = (num(d, c, "BeginX"), num(d, c, "BeginY"));
    assert!(near(begin.0 - begin_was.0, 1.0) && near(begin.1 - begin_was.1, 3.0), "the begin followed the shape: {begin_was:?} to {begin:?}");
    let points = diacraft_engine::draw::connection_points(d, 1, a);
    let at = points.iter().find(|p| p.index == 3).unwrap();
    assert!(near_p(begin, (at.x, at.y)), "and is at the shape's connection point {:?}", (at.x, at.y));
    assert!(route(d, c).len() > 2 && route(d, c).iter().all(|p| p.0.is_finite() && p.1.is_finite()), "the route was made again: {:?}", route(d, c));
    let glued = |from: u32, to: u32| d.page(1).unwrap().connects.iter().any(|k| k.from_sheet == from && k.to_sheet == to);
    assert!(glued(c, a) && glued(c, b), "still glued at both ends");
    assert_eq!(places(&mut s, &[a]), [(3.0, 5.0)]);
    // The connector has both ends glued: it has nothing of its own to move, so it is not counted, and no step is made for it.
    let before = snapshot(&s);
    let r = run(&mut s, "shape.place", json!({"ids": [c], "x": 1.0, "y": 1.0}));
    assert_eq!(r["moved"], 0);
    assert_unchanged(&s, &before, "a connector with both ends glued");
    // Named with the shape it is glued to, it is the shape that is counted.
    let r = run(&mut s, "shape.place", json!({"ids": [a, c], "x": 4.0}));
    assert_eq!(r["moved"], 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_glued_to_a_member_of_a_group_follows_the_group_that_is_placed() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 2.0);
    let other = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, other]})));
    let point_of_a = |s: &diacraft_engine::Session| {
        let points = diacraft_engine::draw::connection_points(doc(s), 1, a);
        let p = points.iter().find(|p| p.index == 3).unwrap();
        (p.x, p.y)
    };
    let was = point_of_a(&s);
    let begin = |s: &diacraft_engine::Session| (num(doc(s), c, "BeginX"), num(doc(s), c, "BeginY"));
    assert!(near_p(begin(&s), was));
    let at = places(&mut s, &[g])[0];
    let r = run(&mut s, "shape.place", json!({"ids": [g], "x": at.0 + 1.5, "y": at.1 + 2.0}));
    assert_eq!(r["moved"], 1);
    let now = point_of_a(&s);
    assert!(near_p(now, (was.0 + 1.5, was.1 + 2.0)), "the member went with its group: {was:?} to {now:?}");
    assert!(near_p(begin(&s), now), "and the connector glued to it followed: {:?} against {now:?}", begin(&s));
    let (first, last) = (route(doc(&s), c).first().copied().unwrap(), route(doc(&s), c).last().copied().unwrap());
    assert!(near_p(first, now) && near_p(last, (num(doc(&s), c, "EndX"), num(doc(&s), c, "EndY"))), "and was routed again: {:?}", route(doc(&s), c));
}

// ---- a member of a group ------------------------------------------------------------------------------------------------------------

#[test]
fn a_member_of_a_turned_group_is_placed_on_the_page_and_its_cells_hold_the_place_in_the_groups_frame() {
    // Group 10, 4 by 2, pin (5, 5), local pin (2, 1), a quarter turn: a point q of its frame is on the page at (5 - (q.1 - 1), 5 + (q.0 - 2)). Member 11
    // has its pin at (1, 1) of the frame, so at (5, 4) on the page.
    let (mut s, _dir) = open_page(&group_with_members("1.5707963267949", ""), "");
    assert!(near_p(places(&mut s, &[11])[0], (5.0, 4.0)));
    let r = run(&mut s, "shape.place", json!({"ids": [11], "x": 8.0, "y": 6.0}));
    assert_eq!(r["moved"], 1);
    assert!(near_p(places(&mut s, &[11])[0], (8.0, 6.0)), "the place the window shows is the place asked for: {:?}", places(&mut s, &[11]));
    // Carried into the group's frame (the turn undone): q.1 = 1 - (8 - 5) = -2 and q.0 = 2 + (6 - 5) = 3.
    assert!(near_p(pin(doc(&s), 11), (3.0, -2.0)), "{:?}", pin(doc(&s), 11));
    // X alone: on the page y stays, which is the group's x, so only the member's PinY changes in the group's frame.
    let x_cell = num(doc(&s), 11, "PinX");
    run(&mut s, "shape.place", json!({"ids": [11], "x": 9.0}));
    assert!(near_p(places(&mut s, &[11])[0], (9.0, 6.0)), "{:?}", places(&mut s, &[11]));
    assert!(near(num(doc(&s), 11, "PinX"), x_cell) && near(num(doc(&s), 11, "PinY"), -3.0), "{:?}", pin(doc(&s), 11));
    // The group itself is a shape of the page: its pin goes to the place, and its members go with it.
    let member_was = places(&mut s, &[11])[0];
    run(&mut s, "shape.place", json!({"ids": [10], "x": 7.0}));
    assert!(near(places(&mut s, &[10])[0].0, 7.0) && near(places(&mut s, &[10])[0].1, 5.0));
    let member_now = places(&mut s, &[11])[0];
    assert!(near_p(member_now, (member_was.0 + 2.0, member_was.1)), "{member_was:?} to {member_now:?}");
    // A line in the group, and the member, in one call: each is placed on the page.
    run(&mut s, "shape.place", json!({"ids": [11, 12], "y": 2.0}));
    assert!(places(&mut s, &[11, 12]).iter().all(|p| near(p.1, 2.0)), "{:?}", places(&mut s, &[11, 12]));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_of_a_group_that_is_flipped_and_turned_reads_back_where_it_was_placed() {
    let (mut s, _dir) = open_page(&group_with_members("0.5235987755983", r#"<Cell N="FlipX" V="1"/>"#), "");
    for (x, y) in [(2.0, 3.0), (7.5, 1.25), (4.0, 4.0)] {
        run(&mut s, "shape.place", json!({"ids": [11], "x": x, "y": y}));
        assert!(near_p(places(&mut s, &[11])[0], (x, y)), "{:?} for {:?}", places(&mut s, &[11]), (x, y));
    }
}

#[test]
fn a_member_named_with_its_group_ends_at_the_place_as_the_group_does_and_is_not_moved_twice() {
    // Group 10 is turned a quarter turn, so a distance on the page is not the same in its frame. Named together, in either order, group and member both
    // have their pin at the place on the page: the group goes first and the member goes what is left.
    for ids in [json!([10, 11]), json!([11, 10])] {
        let (mut s, _dir) = open_page(&group_with_members("1.5707963267949", ""), "");
        let r = run(&mut s, "shape.place", json!({"ids": ids, "x": 8.0, "y": 6.0}));
        assert_eq!((r["moved"].clone(), r["unplaced"].clone()), (json!(2), json!(0)), "{ids}");
        assert!(places(&mut s, &[10, 11]).iter().all(|p| near_p(*p, (8.0, 6.0))), "{ids}: {:?}", places(&mut s, &[10, 11]));
    }
    // x alone: both at that x, each keeping the y it had.
    let (mut s, _dir) = open_page(&group_with_members("0.5235987755983", r#"<Cell N="FlipX" V="1"/>"#), "");
    let was = places(&mut s, &[10, 11, 12]);
    run(&mut s, "shape.place", json!({"ids": [10, 11, 12], "x": 9.5}));
    for (now, was) in places(&mut s, &[10, 11, 12]).iter().zip(&was) {
        assert!(near(now.0, 9.5) && near(now.1, was.1), "{now:?} from {was:?}");
    }
    // Two rectangles grouped by the command: the group and one of them named, the other not named and so carried by the group alone.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let (g_was, b_was) = (places(&mut s, &[g])[0], places(&mut s, &[b])[0]);
    run(&mut s, "shape.place", json!({"ids": [a, g], "y": 7.0}));
    let (a_now, b_now) = (places(&mut s, &[a])[0], places(&mut s, &[b])[0]);
    // The group went to the place first, and the member named after it: it is at the place, and the other member went with the group alone.
    assert!(near(a_now.1, 7.0), "the member named is at the place: {a_now:?}");
    assert!(
        near(b_now.1, b_was.1 + (7.0 - g_was.1)) && near(b_now.0, b_was.0),
        "the member not named went with the group alone: {b_was:?} to {b_now:?}"
    );
    // A member that has moved makes the group the box of its members again, so the group's pin is the middle of that box and no longer the
    // place it was put at before the member went there (a group and a member cannot both have a pin at one place and the group be its members' box).
    let corners = |id: u32| diacraft_engine::draw::shape_corners(doc(&s), 1, id).unwrap();
    let (g_box, members) = (corners(g), [corners(a), corners(b)].concat());
    let low = members.iter().fold((f64::MAX, f64::MAX), |m, c| (m.0.min(c.0), m.1.min(c.1)));
    let high = members.iter().fold((f64::MIN, f64::MIN), |m, c| (m.0.max(c.0), m.1.max(c.1)));
    assert!(
        g_box.iter().all(|c| (near(c.0, low.0) || near(c.0, high.0)) && (near(c.1, low.1) || near(c.1, high.1))),
        "the group is the box of its members: {g_box:?}"
    );
    let g_pin = places(&mut s, &[g])[0];
    assert!(near_p(g_pin, ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0)), "its pin is the middle of the box: {g_pin:?}");
}

#[test]
fn a_line_with_one_glued_end_goes_half_as_far_as_the_pin_was_asked_to() {
    // The begin is glued to a shape and stays; the end is free and moves by the distance the pin was to go; the pin is the middle of the two, so it goes half.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 0}, "to": {"x": 6.0, "y": 2.0}})));
    let (begin, end) = ends(doc(&s), c);
    let pin_was = places(&mut s, &[c])[0];
    assert!(near_p(pin_was, ((begin.0 + end.0) / 2.0, (begin.1 + end.1) / 2.0)));
    let r = run(&mut s, "shape.place", json!({"ids": [c], "x": pin_was.0 + 4.0}));
    assert_eq!(r["moved"], 1);
    let (begin_now, end_now) = ends(doc(&s), c);
    assert!(near_p(begin_now, begin), "the glued begin stays");
    assert!(near(end_now.0, end.0 + 4.0), "the free end went the whole distance: {end:?} to {end_now:?}");
    let pin_now = places(&mut s, &[c])[0];
    assert!(near(pin_now.0, pin_was.0 + 2.0), "so the pin went half of it: {pin_was:?} to {pin_now:?}");
}

#[test]
fn a_top_level_pin_is_exactly_the_number_typed() {
    // `1.0 + (0.3 - 1.0)` is `0.30000000000000004`: the cell holds the place, and not the old pin plus a distance.
    let (mut s, _dir) = open_page(
        &shape(
            1,
            r#"<Cell N="PinX" V="1.0"/><Cell N="PinY" V="1.0"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#,
        ),
        "",
    );
    run(&mut s, "shape.place", json!({"ids": [1], "x": 0.3, "y": 0.7}));
    assert_eq!((num(doc(&s), 1, "PinX"), num(doc(&s), 1, "PinY")), (0.3, 0.7));
    assert_eq!(places(&mut s, &[1]), [(0.3, 0.7)], "and `shape.placement` shows it as typed");
}

#[test]
fn a_shape_that_has_no_place_is_left_and_the_answer_says_so_and_the_others_are_placed() {
    // Group 20 and group 21 in it have pins and local pins of 1.7e308, each a number a cell holds, so the place of member 22 on the page overflows.
    let huge = r#"<Cell N="PinX" V="1.7e308"/><Cell N="PinY" V="0"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="-1.7e308"/><Cell N="LocPinY" V="0"/>"#;
    let member = shape(22, r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let inner = diacraft_testkit::package::shape_with(21, r#" Type="Group""#, &format!("{huge}<Shapes>{member}</Shapes>"));
    let outer = diacraft_testkit::package::shape_with(20, r#" Type="Group""#, &format!("{huge}<Shapes>{inner}</Shapes>"));
    let (mut s, _dir) = open_page(&format!("{}{outer}", crate::panels::file_box(1, 3.0, 2.0, "")), "");
    let before = snapshot(&s);
    let r = run(&mut s, "shape.place", json!({"ids": [1, 22], "x": 5.0}));
    assert_eq!((r["moved"].clone(), r["unplaced"].clone()), (json!(1), json!(1)), "{r}");
    assert!(near(places(&mut s, &[1])[0].0, 5.0));
    assert_eq!(num(doc(&s), 22, "PinX"), 1.0, "the member is as it was");
    // Nothing else to place: no step, and the answer says why.
    let before = snapshot(&s).document.clone().or(before.document);
    let r = run(&mut s, "shape.place", json!({"ids": [22], "x": 5.0}));
    assert_eq!((r["moved"].clone(), r["unplaced"].clone()), (json!(0), json!(1)), "{r}");
    assert_eq!(snapshot(&s).document, before);
}

// ---- refusals -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_that_is_not_on_the_page_is_refused_in_words_and_nothing_changes() {
    let mut s = blank_session();
    let ids = six(&mut s);
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.place", &json!({"ids": [ids[0], 4242], "x": 1.0}));
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
    assert!(e.to_string().contains("4242"), "{e}");
    assert_unchanged(&s, &before, "an id that is not on the page");
    // Nor does a page that is not there.
    let e = error_of(&mut s, "shape.place", &json!({"ids": [ids[0]], "x": 1.0, "page": 9}));
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
    assert_unchanged(&s, &before, "a page that is not there");
}

#[test]
fn a_place_that_is_not_a_number_or_is_huge_is_refused_as_a_move_by_it_is() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = snapshot(&s);
    // JSON has no NaN: a program that sends one sends `null`, and a text is not a number.
    let nothing = [json!({"ids": [a]}), json!({"ids": [a], "x": null}), json!({"ids": [a], "x": f64::NAN, "y": f64::NAN})];
    for params in nothing {
        let e = error_of(&mut s, "shape.place", &params);
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("x"), "{params}: {e}");
    }
    for params in [json!({"ids": [a], "x": "NaN"}), json!({"ids": [a], "y": "1"}), json!({"ids": [a], "x": true}), json!({"ids": [a], "x": [1]})] {
        let e = error_of(&mut s, "shape.place", &params);
        assert!(matches!(e, EngineError::BadParams { .. }), "{params}: {e}");
    }
    for huge in [1.0e300, -1.0e300, 1.0e7, -1.0e7, f64::MAX] {
        let e = error_of(&mut s, "shape.place", &json!({"ids": [a], "x": huge}));
        assert!(matches!(e, EngineError::BadParams { .. }), "{huge}: {e}");
        let e = error_of(&mut s, "shape.place", &json!({"ids": [a], "y": huge}));
        assert!(matches!(e, EngineError::BadParams { .. }), "{huge}: {e}");
        // `shape.move` refuses a distance that big the same way.
        let m = error_of(&mut s, "shape.move", &json!({"ids": [a], "dx": huge, "dy": 0.0}));
        assert!(matches!(m, EngineError::BadParams { .. }), "{huge}: {m}");
    }
    // An id that is not a whole number, none, and more than a call may name.
    for ids in [json!([]), json!([1.5]), json!([-1]), json!("1"), json!((0..5_000).collect::<Vec<u32>>())] {
        let e = error_of(&mut s, "shape.place", &json!({"ids": ids, "x": 1.0}));
        assert!(matches!(e, EngineError::BadParams { .. }), "{ids}: {e}");
    }
    assert_unchanged(&s, &before, "refused places");
    // A place at the very edge of what a move may reach is accepted.
    assert_eq!(run(&mut s, "shape.place", json!({"ids": [a], "x": 1.0e6}))["moved"], 1);
}

// ---- the page, the scale, the edge --------------------------------------------------------------------------------------------------

#[test]
fn a_place_on_a_page_with_a_drawing_scale_is_in_paper_inches_and_held_in_drawing_units() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let k = placement(&mut s, &[a])["units_per_inch"].as_f64().unwrap();
    assert!(near(k, 48.0));
    run(&mut s, "shape.place", json!({"ids": [a], "x": 2.5, "y": 1.5}));
    assert!(near(num(doc(&s), a, "PinX"), 2.5 * 48.0) && near(num(doc(&s), a, "PinY"), 1.5 * 48.0), "{:?}", pin(doc(&s), a));
    assert_eq!(places(&mut s, &[a]), [(2.5, 1.5)]);
}

#[test]
fn a_place_past_the_edge_grows_a_page_with_auto_size_in_the_one_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let width = |s: &mut diacraft_engine::Session| run(s, "page.setup", json!({}))["width"].as_f64().unwrap();
    run(&mut s, "page.auto_size", json!({"on": true}));
    let (steps, was) = (s.history().unwrap().undo_len(), width(&mut s));
    run(&mut s, "shape.place", json!({"ids": [a], "x": was + 6.0}));
    assert!(width(&mut s) > was, "the page grew to take the shape in");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "in the one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert!(near(width(&mut s), was));
}

#[test]
fn a_place_is_snapped_only_when_asked_and_the_pull_is_one_for_every_shape() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"ruler": false, "grid": false, "guides": true}));
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let w = placement(&mut s, &[a])["shapes"][0]["width"].as_f64().unwrap();
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 6.0}));
    // Both shapes come to x such that their right edges are 0.04 short of the guide; with `snap` they rest on it, without it they go where told.
    let x = 6.0 - 0.04 - w / 2.0;
    let plain = run(&mut s, "shape.place", json!({"ids": [a, b], "x": x}));
    assert!(plain.get("snap").is_none(), "no snap unless asked: {plain}");
    assert!(places(&mut s, &[a, b]).iter().all(|p| near(p.0, x)));
    run(&mut s, "edit.undo", json!({}));
    let snapped = run(&mut s, "shape.place", json!({"ids": [a, b], "x": x, "snap": true}));
    assert_eq!(snapped["moved"], 2);
    assert!(places(&mut s, &[a, b]).iter().all(|p| near(p.0 + w / 2.0, 6.0)), "{:?}", places(&mut s, &[a, b]));
    assert_eq!(snapped["snap"]["hits"][0]["target"], json!("guide"));
    // The pull is on the axes asked for: y was not, and does not move.
    assert!(near(places(&mut s, &[a])[0].1, 2.0) && near(places(&mut s, &[b])[0].1, 4.0));
    // More shapes than a snap looks into are refused, and nothing changes.
    let before = snapshot(&s);
    let ids: Vec<u32> = (0..600).collect();
    let e = error_of(&mut s, "shape.place", &json!({"ids": ids, "x": 1.0, "snap": true}));
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
    assert_eq!(snapshot(&s), before);
    // And a `snap` that is not one is refused.
    let e = error_of(&mut s, "shape.place", &json!({"ids": [a], "x": 1.0, "snap": "yes"}));
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
}

// ---- the cost, counted ---------------------------------------------------------------------------------------------------------------

/// A page of `n` plain boxes, the `ID`s 1 to `n`, at nine places across and seven down, as a file holds them.
fn boxes(n: u32) -> (diacraft_engine::Session, crate::edit_foreign::Dir) {
    let shapes: String = (1..=n)
        .map(|id| {
            shape(
                id,
                &format!(
                    r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/>"#,
                    1 + id % 9,
                    1 + id % 7
                ),
            )
        })
        .collect();
    open_page(&shapes, "")
}

#[test]
fn four_thousand_ninety_six_shapes_are_placed_by_one_command_in_one_recalculation_and_one_undo_step() {
    // Counted (`diacraft_calc::work`), not timed: the commits of the recalculation are those of the call, however many shapes, and its steps grow with the
    // shapes and not with their square: four times the shapes take about four times the steps.
    let steps_of = |n: u32| {
        let (mut s, _dir) = boxes(n);
        let ids: Vec<u32> = (1..=n).collect();
        let undo = s.history().unwrap().undo_len();
        // The shapes whose pin is at (5, 4) already are not moved: `id % 9` is 4 and `id % 7` is 3.
        let at_place = ids.iter().filter(|id| *id % 9 == 4 && *id % 7 == 3).count() as u64;
        diacraft_calc::work::reset();
        let r = run(&mut s, "shape.place", json!({"ids": ids, "x": 5.0, "y": 4.0}));
        assert_eq!(r["moved"].as_u64(), Some(u64::from(n) - at_place), "the shapes there already are not counted");
        assert_eq!(s.history().unwrap().undo_len(), undo + 1, "one undo step for {n} shapes");
        assert!(places(&mut s, &ids).iter().all(|p| near_p(*p, (5.0, 4.0))));
        (diacraft_calc::work::commits(), diacraft_calc::work::steps())
    };
    let (small, large) = (steps_of(1_024), steps_of(4_096));
    eprintln!("calculation of a place: 1,024 shapes {small:?}, 4,096 shapes {large:?} (commits, steps)");
    assert!(small.0 <= 4 && large.0 <= 4, "commits: {small:?} {large:?}");
    assert!(large.1 <= small.1 * 9 / 2, "steps grow with the shapes, not their square: {small:?} {large:?}");
}
