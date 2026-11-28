//! What a halving does to the connectors glued to the points it removes or renumbers, and the keepers of the marker (design §6).
//! The vectors are V21 (d). A doubling moves nothing, so it changes no connector
//! cell and no `Connect` row.

use diacraft_doc::{Cell, CellAddr, Connect, Value as CellValue};
use diacraft_engine::Session;
use serde_json::json;

use crate::conn_points::{
    act, assert_places, diamond, formula_of, marker, marker_text, near, params, places, put_cell, rect, refused, shape_at, user_cell, v21_b,
};
use crate::conn_points_edges::{page_of, rectangle, session_with};
use crate::edit::{doc, drop_shape, id_of, num};
use crate::{blank_session, check_session, run};

fn connects(s: &Session) -> Vec<Connect> {
    doc(s).page(1).unwrap().connects.iter().cloned().collect()
}

/// The `Connect` row of an end of `connector`: (`ToSheet`, `ToPart`, `ToCell`).
fn glue_of(s: &Session, connector: u32, begin: bool) -> Option<(u32, u32, String)> {
    let cell = if begin { "BeginX" } else { "EndX" };
    connects(s)
        .into_iter()
        .find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(cell))
        .map(|c| (c.to_sheet, c.to_part.unwrap(), c.to_cell.as_deref().unwrap_or("").to_owned()))
}

fn end_place(s: &Session, connector: u32, begin: bool) -> (f64, f64) {
    let (x, y) = if begin { ("BeginX", "BeginY") } else { ("EndX", "EndY") };
    (num(doc(s), connector, x), num(doc(s), connector, y))
}

/// A connector from point `point` of `shape` to the free place `to`.
fn connect_from(s: &mut Session, shape: u32, point: u32, to: (f64, f64)) -> u32 {
    id_of(&run(s, "connector.add", json!({"from": {"id": shape, "point": point}, "to": {"x": to.0, "y": to.1}})))
}

fn cells_of(s: &Session, id: u32) -> diacraft_doc::Shape {
    doc(s).shape(&shape_at(s, id)).unwrap().as_ref().clone()
}

// ---- a doubling moves no glued end -------------------------------------------------------------------------------------------------------

#[test]
fn a_doubling_changes_no_connector_cell_and_no_connect_row() {
    let mut s = blank_session();
    let a = rect(&mut s, 2.0, 1.0);
    let c1 = connect_from(&mut s, a, 3, (5.0, 3.0));
    let c2 = connect_from(&mut s, a, 0, (-2.0, 3.0));
    let (rows, c1_before, c2_before) = (connects(&s), cells_of(&s, c1), cells_of(&s, c2));
    act(&mut s, a, "double", json!({}));
    act(&mut s, a, "double", json!({"edge": 2}));
    assert_eq!(connects(&s), rows, "no Connect row changes");
    assert_eq!((cells_of(&s, c1), cells_of(&s, c2)), (c1_before, c2_before), "no cell of a connector changes");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a halving: rows that move up, and ends on removed points ---------------------------------------------------------------------------

#[test]
fn an_end_on_a_row_that_only_moved_up_keeps_its_place_and_its_formula_keeps_every_character_but_the_number() {
    // V21 (d): a connector end glued to row 14 (ToPart 114, `Connections.X15`) now has ToPart 110 and names row 11; its end does not move.
    let (mut s, a) = v21_b();
    let c = connect_from(&mut s, a, 14, (5.0, 3.0));
    let place = end_place(&s, c, true);
    assert!(near(place.0, 0.0) && near(place.1, 0.75), "{place:?}");
    let (x_formula, y_formula) = (formula_of(&s, c, &CellAddr::direct("BeginX")).unwrap(), formula_of(&s, c, &CellAddr::direct("BeginY")).unwrap());
    assert_eq!(glue_of(&s, c, true), Some((a, 114, "Connections.X15".to_owned())));
    let answer = act(&mut s, a, "halve", json!({"edge": 2}));
    assert_eq!(glue_of(&s, c, true), Some((a, 110, "Connections.X11".to_owned())));
    let renumbered = |text: &str| text.replace("X15", "X11").replace("Y15", "Y11");
    assert_eq!(formula_of(&s, c, &CellAddr::direct("BeginX")).unwrap(), renumbered(&x_formula));
    assert_eq!(formula_of(&s, c, &CellAddr::direct("BeginY")).unwrap(), renumbered(&y_formula));
    let moved = end_place(&s, c, true);
    assert!(near(moved.0, place.0) && near(moved.1, place.1), "the end did not move: {moved:?}");
    assert_eq!(answer["reglued"], json!([]), "an end whose row only moved up went to no other point");
}

#[test]
fn two_connectors_on_a_removed_middle_point_part_to_its_two_neighbours() {
    // After V21 (b), A's end on row 11 (1.25, 1) comes from the right and B's from the left; halving the top takes row 11 away: A goes to row 4, B to row 0.
    let (mut s, a) = v21_b();
    let ca = connect_from(&mut s, a, 11, (5.0, 1.5));
    let cb = connect_from(&mut s, a, 11, (-3.0, 1.5));
    let answer = act(&mut s, a, "halve", json!({"edge": 2}));
    assert_eq!(glue_of(&s, ca, true).map(|g| g.1), Some(104));
    assert_eq!(glue_of(&s, cb, true).map(|g| g.1), Some(100));
    let mut reglued = answer["reglued"].as_array().unwrap().clone();
    reglued.sort_by_key(|r| r["connector"].as_u64());
    assert_eq!(
        reglued,
        [
            json!({"connector": ca, "end": "begin", "shape": a, "from": 11, "to": 4}),
            json!({"connector": cb, "end": "begin", "shape": a, "from": 11, "to": 0}),
        ]
    );
    // The ends are where their new points are, and nothing reads #REF!.
    let (pa, pb) = (end_place(&s, ca, true), end_place(&s, cb, true));
    assert!(near(pa.0, 1.5) && near(pa.1, 1.0), "{pa:?}");
    assert!(near(pb.0, 1.0) && near(pb.1, 1.0), "{pb:?}");
    for c in [ca, cb] {
        assert!(!formula_of(&s, c, &CellAddr::direct("BeginX")).unwrap().contains("REF"));
    }
}

#[test]
fn an_end_on_row_10_goes_to_row_4_and_one_on_row_13_to_row_5() {
    let (mut s, a) = v21_b();
    let c10 = connect_from(&mut s, a, 10, (5.0, 1.5));
    let c13 = connect_from(&mut s, a, 13, (5.0, 1.5));
    act(&mut s, a, "halve", json!({"edge": 2}));
    assert_eq!(glue_of(&s, c10, true).map(|g| g.1), Some(104));
    assert_eq!(glue_of(&s, c13, true).map(|g| g.1), Some(105));
}

#[test]
fn an_end_on_a_removed_point_of_an_edge_left_empty_goes_to_another_edges_point() {
    // The diamond's edge 0 (from (1, 0) to (2, 1)) doubled: row 4 at (1.5, 0.5). A connector end on it, its other end at (4, 1.2): halving the edge leaves it no point, the
    // vertices (1, 0) and (2, 1) are as near, and the end goes to row 1 at (2, 1), the nearer its other end.
    let mut s = session_with(page_of(vec![diamond(1)]));
    act(&mut s, 1, "double", json!({"edge": 0}));
    assert_places("row 4", &places(&s, 1)[4..], &[(1.5, 0.5)]);
    let c = connect_from(&mut s, 1, 4, (4.0, 1.2));
    act(&mut s, 1, "halve", json!({"edge": 0}));
    assert_eq!(glue_of(&s, c, true).map(|g| g.1), Some(101));
    let p = end_place(&s, c, true);
    assert!(near(p.0, 2.0) && near(p.1, 1.0), "{p:?}");
}

#[test]
fn an_end_on_a_shape_left_with_no_point_is_glued_to_the_whole_shape_and_nothing_is_unglued() {
    // The rectangle with no points, doubled to one point; a connector end on it. Halving glues the end to the whole shape (ToPart 3, ToCell PinX).
    let mut s = session_with(page_of(vec![rectangle(1, 2.0, 1.0)]));
    act(&mut s, 1, "double", json!({"edge": 2}));
    let c = connect_from(&mut s, 1, 0, (5.0, 1.5));
    let answer = act(&mut s, 1, "halve", json!({"edge": 2}));
    assert_eq!(glue_of(&s, c, true), Some((1, 3, "PinX".to_owned())));
    assert_eq!(answer["reglued"], json!([{"connector": c, "end": "begin", "shape": 1, "from": 0, "to": null}]));
    // Placed on the outline of the box as any whole-shape end is, and reading no #REF!.
    let p = end_place(&s, c, true);
    let on_outline =
        (0.0..=2.0).contains(&p.0) && (0.0..=1.0).contains(&p.1) && (near(p.0, 0.0) || near(p.0, 2.0) || near(p.1, 0.0) || near(p.1, 1.0));
    assert!(on_outline, "{p:?}");
    let cell = formula_of(&s, c, &CellAddr::direct("BeginX"));
    assert!(cell.as_deref().is_none_or(|f| !f.contains("REF")), "{cell:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn halving_the_last_point_a_formula_names_is_refused_in_words_and_changes_nothing() {
    // Shape 5 doubled to one point, and shape 9's User.Anchor names `Sheet.5!Connections.X1`.
    let mut s = session_with(page_of(vec![rectangle(5, 2.0, 1.0), rectangle(9, 1.0, 1.0)]));
    act(&mut s, 5, "double", json!({"edge": 2}));
    put_cell(&mut s, 9, &user_cell("Anchor"), Cell::with_formula("Sheet.5!Connections.X1", Some(CellValue::from_text("1"))).with_unit("IN"));
    let words = refused(&mut s, &params(5, "halve", json!({"edge": 2})));
    assert!(words.ends_with("halving would remove the last connection point of shape 5, which the formula of Sheet.9!User.Anchor names"), "{words}");
}

#[test]
fn another_formula_that_names_a_removed_row_is_renumbered_to_its_successor_and_counted() {
    // With V21 (b) on shape 3 and `Sheet.<other>!User.Anchor` holding `Sheet.<a>!Connections.X12` (row 11), V21 (d) rewrites it to X1 (rows 0 and 4 as near, the lower).
    let (mut s, a) = v21_b();
    let other = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    put_cell(
        &mut s,
        other,
        &user_cell("Anchor"),
        Cell::with_formula(&format!("Sheet.{a}!Connections.X12"), Some(CellValue::from_text("1"))).with_unit("IN"),
    );
    let answer = act(&mut s, a, "halve", json!({"edge": 2}));
    assert_eq!(formula_of(&s, other, &user_cell("Anchor")).as_deref(), Some(format!("Sheet.{a}!Connections.X1").as_str()));
    assert_eq!(answer["renumbered"], 1);
}

#[test]
fn what_names_no_row_is_left_as_it_was() {
    // Strings, quoted names, another shape, another page's qualifier, another section and a trigger are none of the walk's concern.
    let (mut s, a) = v21_b();
    let c = connect_from(&mut s, a, 14, (5.0, 3.0));
    let untouched = [
        ("T1", "\"Connections.X15\"".to_owned()),
        ("T2", "'Connections'!X15".to_owned()),
        ("T3", "Sheet.77!Connections.X15".to_owned()),
        ("T4", format!("Pages[Page-1]!Sheet.{a}!Connections.X15")),
        ("T5", "Geometry1.X15".to_owned()),
    ];
    for (name, text) in &untouched {
        put_cell(&mut s, a, &user_cell(name), Cell::with_formula(text, Some(CellValue::from_text("0"))));
    }
    put_cell(&mut s, a, &user_cell("Own"), Cell::with_formula("Connections.X15", Some(CellValue::from_text("0"))));
    let trigger = format!("_XFTRIGGER(Sheet.{a}!EventXFMod)");
    put_cell(&mut s, c, &CellAddr::direct("BegTrigger"), Cell::with_formula(&trigger, Some(CellValue::from_text("0"))));
    act(&mut s, a, "halve", json!({"edge": 2}));
    for (name, text) in &untouched {
        assert_eq!(formula_of(&s, a, &user_cell(name)).as_deref(), Some(text.as_str()), "{name}");
    }
    assert_eq!(formula_of(&s, a, &user_cell("Own")).as_deref(), Some("Connections.X11"), "the shape's own unqualified reference follows its row");
    assert_eq!(formula_of(&s, c, &CellAddr::direct("BegTrigger")).as_deref(), Some(trigger.as_str()), "a trigger names the shape and no row");
}

#[test]
fn a_formula_of_65536_bytes_of_references_is_renumbered_within_a_counted_scan() {
    let (mut s, a) = v21_b();
    let other = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    let token = format!("Sheet.{a}!Connections.X15+");
    let text = format!("{}0", token.repeat(65_536 / token.len()));
    assert!(text.len() > 65_000 && text.len() <= 65_536);
    put_cell(&mut s, other, &user_cell("Long"), Cell::with_formula(&text, Some(CellValue::from_text("0"))));
    act(&mut s, a, "halve", json!({"edge": 2}));
    let now = formula_of(&s, other, &user_cell("Long")).unwrap();
    assert_eq!(now, text.replace("X15", "X11"));
}

#[test]
fn a_connector_on_a_locked_layer_is_renumbered_like_any_other() {
    // The lock stops a person editing the connector, and this keeps its glue.
    let (mut s, a) = v21_b();
    let c = connect_from(&mut s, a, 14, (5.0, 3.0));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [c], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    act(&mut s, a, "halve", json!({"edge": 2}));
    assert_eq!(glue_of(&s, c, true), Some((a, 110, "Connections.X11".to_owned())));
}

// ---- the keepers of the marker --------------------------------------------------------------------------------------------

#[test]
fn change_shape_drops_the_marker_and_a_duplicate_keeps_it_naming_its_own_rows() {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)]);
    // A duplicate has its own sheet, with the same marker naming its own rows 4 and 5; halving its top takes them from the copy and leaves the original alone.
    let copy = run(&mut s, "shape.duplicate", json!({"ids": [id]}))["ids"][0].as_u64().map(|n| u32::try_from(n).unwrap()).unwrap();
    assert_eq!(marker(&s, copy), [(4, 1), (5, 1)]);
    act(&mut s, copy, "halve", json!({"edge": 2}));
    assert_eq!((places(&s, copy).len(), places(&s, id).len()), (4, 6));
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)]);
    // Change Shape replaces the points with the new master's, which are originals: the marker goes.
    run(&mut s, "shape.change", json!({"ids": [id], "master": "diamond"}));
    assert_eq!(marker_text(&s, id), None, "the new master's points are originals");
    assert_eq!(places(&s, id).len(), 4);
}

#[test]
fn a_group_and_an_ungroup_leave_the_points_and_the_marker_of_a_member_as_they_were() {
    let mut s = blank_session();
    let a = rect(&mut s, 2.0, 1.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    act(&mut s, a, "double", json!({"edge": 2}));
    let (points, text) = (places(&s, a), marker_text(&s, a));
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    assert_eq!((places(&s, a), marker_text(&s, a)), (points.clone(), text.clone()), "a member of a group");
    run(&mut s, "shape.ungroup", json!({"ids": [group]}));
    assert_eq!((places(&s, a), marker_text(&s, a)), (points, text), "and out of it again");
    assert_eq!(marker(&s, a), [(4, 1), (5, 1)]);
}

#[test]
fn a_row_inserted_before_the_added_points_leaves_the_marker_naming_the_same_points() {
    // `sheet.add_row` numbers the rows after it again and every formula that names one follows it, the marker's terms among them.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    run(&mut s, "sheet.add_row", json!({"id": id, "section": "Connection", "before": 3}));
    assert_eq!(marker(&s, id), [(5, 1), (6, 1)], "rows 4 and 5 moved up to 5 and 6, and the terms went with them");
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(places(&s, id).len(), 5, "the two added points went, and the row put in by hand stayed");
}

#[test]
fn a_row_deleted_by_the_shapesheet_leaves_the_marker_right_or_void_and_never_names_an_original() {
    // An instance of a master keeps the numbers of its rows when one goes (a master's rows keep theirs), and the formulas that name the row that went read `REF()` there too
    // (which was made before): the term of the deleted point is void and so is the marker, every point left is an original, and a halving takes none (safe).
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    run(&mut s, "sheet.delete_row", json!({"id": id, "section": "Connection", "row": 6}));
    assert_eq!(marker_text(&s, id).as_deref(), Some("0*(1*Connections.X5+1*REF())"));
    assert_eq!(marker(&s, id), Vec::<(u32, u8)>::new());
    assert_eq!(places(&s, id).len(), 5, "row 6 went and the other five points stayed");
    let words = refused(&mut s, &params(id, "halve", json!({"edge": 2})));
    assert!(words.contains("nothing to halve"), "{words}");
    assert_eq!(places(&s, id).len(), 5, "a halving takes no original");
    // A shape whose rows are all its own has them numbered again, and the term of the row that went reads `REF()`: the marker is of another form, nothing is added,
    // and a halving takes no original (safe).
    let mut own = rectangle(1, 2.0, 1.0);
    for (ix, (x, y)) in [(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (2.0, 0.5), (1.5, 1.0), (0.5, 1.0)].into_iter().enumerate() {
        own = crate::conn_points_edges::point(own, u32::try_from(ix).unwrap(), x, y);
    }
    let mut t = session_with(page_of(vec![own]));
    crate::conn_points::put_marker(&mut t, 1, "0*(1*Connections.X5+1*Connections.X6)");
    assert_eq!(marker(&t, 1), [(4, 1), (5, 1)]);
    run(&mut t, "sheet.delete_row", json!({"id": 1, "section": "Connection", "row": 6}));
    assert_eq!(marker_text(&t, 1).as_deref(), Some("0*(1*Connections.X5+1*REF())"));
    assert_eq!(marker(&t, 1), Vec::<(u32, u8)>::new());
    let words = refused(&mut t, &params(1, "halve", json!({"edge": 2})));
    assert!(words.contains("nothing to halve"), "{words}");
    // A section deleted whole leaves the marker naming no row.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    run(&mut s, "sheet.delete_section", json!({"id": id, "section": "Connection"}));
    assert_eq!(marker(&s, id), Vec::<(u32, u8)>::new());
}

// ---- the doubled shape keeps its glue through a moved row (a stale reference never reads) -----------------------------------------------------

#[test]
fn every_end_stays_glued_to_a_point_that_exists_after_a_sequence_of_calls() {
    let (mut s, a) = v21_b();
    let ends: Vec<u32> = [4_u32, 7, 9, 12, 15].into_iter().map(|p| connect_from(&mut s, a, p, (5.0, 3.0))).collect();
    for call in [params(a, "halve", json!({"edge": 2})), params(a, "halve", json!({})), params(a, "double", json!({"edge": 1}))] {
        run(&mut s, "shape.connection_points", call);
        let rows = places(&s, a).len();
        for c in &ends {
            let glue = glue_of(&s, *c, true).expect("still glued");
            let ix = usize::try_from(glue.1 - 100).unwrap();
            assert!(ix < rows, "connector {c} is glued to row {ix} of {rows}");
            let p = end_place(&s, *c, true);
            let q = places(&s, a)[ix];
            assert!(near(p.0, q.0) && near(p.1, q.1), "connector {c}: end {p:?}, its point {q:?}");
        }
    }
}
