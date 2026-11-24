//! The reroute setting of a connector (`connector.reroute`) and the snap of a leg dragged by hand
//! (`connector.edit_path` with `snap`).

use diacraft_doc::Document;
use diacraft_engine::EngineError;
use diacraft_engine::draw::{Reroute, connector_path, connector_reroute};
use serde_json::json;

use crate::connect_style::{cell, z_pair};
use crate::edit::{doc, drop_shape, id_of, route};
use crate::{Snapshot, check_session, run, snapshot};

type P = (f64, f64);

fn quiet(s: &diacraft_engine::Session) -> Snapshot {
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

/// Box A at (3, 4) and box B at (9, 4), joined from A's right point to B's left point: one level leg.
fn level_pair() -> (diacraft_engine::Session, u32, u32, u32) {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    (s, a, b, c)
}

fn mode(d: &Document, id: u32) -> Option<Reroute> {
    connector_reroute(d, 1, id)
}

#[test]
fn the_reroute_setting_is_the_cell_of_the_connector_tab_and_is_one_undo_step() {
    let (mut s, _, _, c) = z_pair();
    assert_eq!(mode(doc(&s), c), Some(Reroute::Freely), "a connector with no code reroutes freely (0)");
    // 0 freely, 1 as needed, 2 never, 3 on crossover; each is written locally.
    for (word, code, want) in [
        ("never", 2.0, Reroute::Never),
        ("on_crossover", 3.0, Reroute::OnCrossover),
        ("as_needed", 1.0, Reroute::AsNeeded),
        ("freely", 0.0, Reroute::Freely),
    ] {
        let steps = quiet(&s).undo_steps;
        let r = run(&mut s, "connector.reroute", json!({"ids": [c], "mode": word}));
        assert_eq!(r, json!({"page": 1, "mode": word, "changed": 1}), "{word}");
        assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(code), "{word}");
        assert_eq!(mode(doc(&s), c), Some(want), "{word}");
        assert_eq!(quiet(&s).undo_steps, steps + 1, "{word}: one undo step");
    }
    // Setting what it already is changes nothing and is no undo step.
    let was = quiet(&s);
    let r = run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "freely"}));
    assert_eq!(r, json!({"page": 1, "mode": "freely", "changed": 0}));
    assert_eq!(quiet(&s), was);
    // One step takes it back.
    let before = quiet(&s);
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "never"}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(quiet(&s).document, before.document);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_that_is_no_connector_a_shape_that_is_not_there_and_a_page_that_is_not_there_have_no_setting() {
    let (s, _, _, c) = z_pair();
    assert_eq!(mode(doc(&s), 1), None);
    assert_eq!(mode(doc(&s), 99), None);
    assert_eq!(connector_reroute(doc(&s), 9, c), None);
}

#[test]
fn several_connectors_take_the_setting_together_and_a_refusal_changes_none() {
    let (mut s, a, _, c) = z_pair();
    let d = drop_shape(&mut s, "rectangle", 3.0, 8.0);
    let e = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 0}, "to": {"id": d, "point": 2}})));
    let r = run(&mut s, "connector.reroute", json!({"ids": [c, e], "mode": "on_crossover", "page": 1}));
    assert_eq!(r["changed"], json!(2));
    assert_eq!((mode(doc(&s), c), mode(doc(&s), e)), (Some(Reroute::OnCrossover), Some(Reroute::OnCrossover)));
    // A shape that is no connector among them: refused in words, and the others are not written.
    let was = quiet(&s);
    let err = s.execute("connector.reroute", &json!({"ids": [c, d], "mode": "never"})).unwrap_err();
    assert!(err.to_string().contains(&format!("shape {d} is not a connector")), "{err}");
    assert_eq!(quiet(&s), was);
    // The other refusals say what was wrong.
    for (what, params, words) in [
        ("an unknown mode", json!({"ids": [c], "mode": "sometimes"}), "freely, as_needed, never or on_crossover"),
        ("no mode", json!({"ids": [c]}), "missing `mode`"),
        ("no ids", json!({"mode": "never"}), "ids"),
        ("a shape that is not there", json!({"ids": [99], "mode": "never"}), "99"),
        ("a mode that is not text", json!({"ids": [c], "mode": 3}), "mode"),
    ] {
        let err = s.execute("connector.reroute", &params).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{what}: {err}");
        assert!(err.to_string().contains(words), "{what}: {err}");
        assert_eq!(quiet(&s), was, "{what}");
    }
}

#[test]
fn a_connector_on_crossover_is_routed_again_when_a_leg_would_pass_through_a_shape_it_is_glued_to() {
    // 3 builds a new path when a leg would cross a glued shape, and otherwise the glued ends follow and the path stays.
    let (mut s, _, b, c) = level_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 4.0), (8.25, 4.0)]);
    // B moves away along the connector: no leg meets a shape, the path stays and its end follows.
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 1.0, "dy": 0.0}));
    let r = route(doc(&s), c);
    assert_eq!(r.len(), 6, "kept: {r:?}");
    assert!(near_point(r[5], (9.25, 4.0)), "{r:?}");
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
    // B moves back and on, to a place where the path's last leg would run through it: the path is made again, by the style, and the setting stays 3.
    run(&mut s, "shape.move", json!({"ids": [b], "dx": -5.0, "dy": 0.0}));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (4.25, 4.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0), "it still reroutes on crossover");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_set_never_to_reroute_keeps_its_path_even_through_a_shape_it_is_glued_to() {
    let (mut s, _, b, c) = level_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "never"}));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": -4.0, "dy": 0.0}));
    let r = route(doc(&s), c);
    assert_eq!(r.len(), 6, "Never, even when a leg passes through a shape it is glued to: {r:?}");
    assert!(near_point(r[0], (3.75, 4.0)) && near_point(r[5], (4.25, 4.0)), "the ends follow: {r:?}");
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(2.0));
}

#[test]
fn a_connector_set_as_needed_is_routed_again_on_a_crossing_and_keeps_its_setting() {
    let (mut s, _, b, c) = level_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "as_needed"}));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": -4.0, "dy": 0.0}));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (4.25, 4.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(1.0));
}

/// Connector `c`'s route in `rows` rows, the same corners: points put along its first leg, which the crossing test drops as it reads them (as it drops the two
/// sides of a line jump).
fn padded(s: &mut diacraft_engine::Session, c: u32, rows: usize) {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Cells, Row, Rows, Section, Shape, Value};
    s.edit("pad the route", |d| {
        let page = d.pages.get_mut(0).map(Arc::make_mut).unwrap();
        let at = page.shapes.iter().position(|x| x.id == c).unwrap();
        let mut shape = Shape::clone(page.shapes.get(at).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let slot = sheet.sections.iter().position(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
        let mut section = Section::clone(&sheet.sections[slot]);
        let held: Vec<Row> = section.rows.iter().map(|r| Row::clone(r)).collect();
        let pads = rows - held.len();
        let mut out: Vec<Arc<Row>> = Vec::new();
        for (k, row) in held.into_iter().enumerate() {
            out.push(Arc::new(row));
            if k == 0 {
                for p in 0..pads {
                    let mut cells = Cells::new();
                    cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&format!("{}", 0.2 * (p + 1) as f64 / (pads + 1) as f64))));
                    cells.insert(CellName::new("Y"), Cell::constant(Value::from_text("0")));
                    out.push(Arc::new(Row { row_type: Some(Arc::from("LineTo")), cells, ..Row::default() }));
                }
            }
        }
        for (k, row) in out.iter_mut().enumerate() {
            Arc::make_mut(row).ix = Some(u32::try_from(k).unwrap() + 1);
        }
        section.rows = Rows::from(out);
        sheet.sections[slot] = Arc::new(section);
        page.shapes.set(at, Arc::new(shape)).unwrap();
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_route_of_more_rows_than_a_hand_path_has_room_for_is_not_read_for_the_crossing_and_keeps_its_path() {
    // The crossing test reads at most MAX_CROSSING_ROWS rows of a route (16 times the points of a hand path: room for line jumps on every leg), and a
    // route of more is held not to cross, unread, as one of no lines is. The same route, in as many rows as that and in one more, is moved to where it crosses B.
    use diacraft_engine::draw::{MAX_CROSSING_ROWS, MAX_HAND_POINTS};
    assert_eq!(MAX_CROSSING_ROWS, 16 * MAX_HAND_POINTS);
    for (rows, crosses) in [(MAX_CROSSING_ROWS, true), (MAX_CROSSING_ROWS + 1, false)] {
        let (mut s, _, b, c) = level_pair();
        run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
        padded(&mut s, c, rows);
        assert_eq!(route(doc(&s), c).len(), rows);
        assert_eq!(connector_path(doc(&s), 1, c).unwrap().points.len(), 6, "the same six corners");
        run(&mut s, "shape.move", json!({"ids": [b], "dx": -4.0, "dy": 0.0}));
        if crosses {
            assert_route(&route(doc(&s), c), &[(3.75, 4.0), (4.25, 4.0)]);
        } else {
            assert_eq!(route(doc(&s), c).len(), rows, "{rows} rows: the path is kept");
        }
        assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_connector_set_freely_is_routed_again_by_any_move_as_before() {
    let (mut s, _, b, c) = level_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "freely"}));
    // The path edited by hand stays until something moves; the next move of a glued shape makes it again.
    assert_eq!(route(doc(&s), c).len(), 6);
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 1.0, "dy": 0.0}));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (9.25, 4.0)]);
}

#[test]
fn a_reset_connector_goes_back_to_freely_whatever_it_was_set_to() {
    let (mut s, _, _, c) = level_pair();
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": "never"}));
    run(&mut s, "connector.reset", json!({"ids": [c]}));
    assert_eq!(mode(doc(&s), c), Some(Reroute::Freely), "Reset Connector writes 0");
}

#[test]
fn a_dragged_leg_snaps_to_the_grid_by_the_documents_settings_and_says_what_it_snapped_to() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "snap.set", json!({"ruler": false}));
    // The pointer is 0.04 inch past the grid line at 7: the grid pulls the leg onto it (and not off to a line it is not near).
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.04, "snap": {"zoom": 96.0}}));
    assert_eq!(r["changed"], json!(true), "{r}");
    assert!(near(r["to"].as_f64().unwrap(), 7.0), "the line the leg went to is told: {r}");
    assert_eq!(r["snap"]["hits"][0]["axis"], json!("x"), "{r}");
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (7.0, 4.0), (7.0, 6.0), (8.25, 6.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
    // A level leg snaps on y.
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "snap.set", json!({"ruler": false}));
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 2.97, "snap": {"zoom": 96.0}}));
    assert!(near(r["to"].as_f64().unwrap(), 3.0), "{r}");
    assert_eq!(r["snap"]["hits"][0]["axis"], json!("y"), "{r}");
}

#[test]
fn a_dragged_leg_is_left_where_it_is_with_the_snap_switch_off_and_without_the_parameter() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "snap.set", json!({"snap": false}));
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.04, "snap": {"zoom": 96.0}}));
    assert!(near(r["to"].as_f64().unwrap(), 7.04), "{r}");
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (7.04, 4.0), (7.04, 6.0), (8.25, 6.0)]);
    // No `snap`: the answer is the plain one, as before.
    let (mut s, _, _, c) = z_pair();
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.04}));
    assert_eq!(r, json!({"page": 1, "id": c, "changed": true}));
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (7.04, 4.0), (7.04, 6.0), (8.25, 6.0)]);
}

#[test]
fn a_leg_dragged_to_a_guide_is_pulled_onto_it() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "snap.set", json!({"ruler": false, "grid": false, "guides": true}));
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 6.5}));
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 6.53, "snap": {"zoom": 96.0}}));
    assert!(near(r["to"].as_f64().unwrap(), 6.5), "{r}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
    assert_route(&route(doc(&s), c), &[(3.75, 4.0), (6.5, 4.0), (6.5, 6.0), (8.25, 6.0)]);
}

#[test]
fn the_snap_words_of_an_edit_are_the_places_words() {
    let (mut s, _, _, c) = z_pair();
    let was = quiet(&s);
    // `lock` and `nudge` are a move's, and a leg's lock is its own (the axis it travels along), so they are refused in words.
    for snap in [json!({"zoom": 96, "lock": "x"}), json!({"nudge": {}}), json!({"keep": "proportions"}), json!({"zoom": 0}), json!("yes"), json!(3)] {
        let err = s.execute("connector.edit_path", &json!({"id": c, "leg": 1, "to": 7.0, "snap": snap})).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{snap}: {err}");
    }
    assert_eq!(quiet(&s), was);
    // `true` snaps by the document's own settings at the default zoom.
    run(&mut s, "snap.set", json!({"ruler": false}));
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.01, "snap": true}));
    assert!(near(r["to"].as_f64().unwrap(), 7.0), "{r}");
}

#[test]
fn a_snap_that_pulls_the_leg_back_onto_its_own_line_is_no_edit() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "snap.set", json!({"ruler": false}));
    let was = quiet(&s);
    // The leg is at x = 6, on a grid line: a pointer 0.02 inch off is pulled back onto it, which is where the leg is.
    let r = run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 6.02, "snap": {"zoom": 96.0}}));
    assert_eq!(r["changed"], json!(false), "{r}");
    assert_eq!(quiet(&s), was, "no edit, no undo step, and the path is not marked as edited");
}

#[test]
fn the_pure_route_a_window_previews_is_the_route_the_command_writes() {
    let (s, _, _, c) = z_pair();
    let points: Vec<P> = connector_path(doc(&s), 1, c).unwrap().points;
    for (leg, to) in [(0_usize, 3.0), (1, 7.0), (2, 7.5)] {
        let moved = diacraft_engine::draw::moved_route(&points, leg, to).unwrap();
        let (mut t, _, _, same) = z_pair();
        run(&mut t, "connector.edit_path", json!({"id": same, "leg": leg, "to": to}));
        assert_route(&route(doc(&t), same), &moved);
    }
    // A leg that is not there and a number that is not one come back as none.
    assert_eq!(diacraft_engine::draw::moved_route(&points, 9, 1.0), None);
    assert_eq!(diacraft_engine::draw::moved_route(&points, 1, f64::NAN), None);
}
