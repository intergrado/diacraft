//! Snap strength: the six pulls of the Snap & Glue dialog's Advanced tab, in screen pixels, are the document's settings. A kind of target
//! pulls a shape only within its strength at the zoom of the query, and the grid and the ruler no longer pull from any distance.
//!
//! Distances are chosen well away from the boundary of a pull, so that no test rests on the rounding of a pixel. The grid's lines at 96 screen points to an inch are a
//! quarter inch apart (`snap_commands.rs`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// The six strengths, in the order of the Advanced tab, with their defaults in screen pixels.
const STRENGTHS: [(&str, u64); 6] = [
    ("strength_ruler", 4),
    ("strength_grid", 5),
    ("strength_guides", 10),
    ("strength_points", 10),
    ("strength_geometry", 8),
    ("strength_extensions", 13),
];

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-strength-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn near(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-6)
}

/// A session whose Snap and Glue options are all off but the ones named (`snap` stays on), so that one kind of target is asked alone.
fn alone(on: &[&str]) -> Session {
    let mut s = blank_session();
    let mut off = serde_json::Map::new();
    for name in [
        "ruler",
        "grid",
        "guides",
        "snap_handles",
        "snap_vertices",
        "snap_connection_points",
        "snap_geometry",
        "alignment_box",
        "snap_extensions",
        "intersections",
        "glue_guides",
        "glue_handles",
        "glue_vertices",
        "glue_connection_points",
        "glue_geometry",
        "dynamic_grid",
    ] {
        off.insert(name.to_owned(), json!(on.contains(&name)));
    }
    run(&mut s, "snap.set", Value::Object(off));
    s
}

/// Where `snap.at` lands the place `(x, y)` at `zoom` screen points to an inch, and what it says it snapped to.
fn at(s: &mut Session, x: f64, y: f64, zoom: f64) -> (f64, f64, Value) {
    let r = run(s, "snap.at", json!({"page": 1, "x": x, "y": y, "zoom": zoom}));
    (r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap(), r["hits"].clone())
}

// ----------------------------------------------------------------------------------------------------------------------------- the setting

/// A new drawing has the six strengths at the defaults of R80, in the answer of `snap.settings`, and they are not in the file's words.
#[test]
fn a_new_drawing_has_the_six_strengths_at_their_defaults() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let r = run(&mut s, "snap.settings", json!({}));
    for (name, default) in STRENGTHS {
        assert_eq!(r[name], json!(default), "{name}: {r}");
    }
    assert!(!r["words"].to_string().contains("strength"), "the words of DocumentSettings hold no strength: {r}");
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], json!(false), "asking changes nothing");
}

/// `snap.set` takes each strength, no undo step for any of them (a View setting is "unsaved, not undo"), none for a value that is the same, and `snap.settings`
/// answers with them.
#[test]
fn snap_set_changes_the_strengths_and_is_no_undo_step() {
    let mut s = blank_session();
    let steps = s.history().unwrap().undo_len();
    let r = run(
        &mut s,
        "snap.set",
        json!({"strength_ruler": 0, "strength_grid": 999, "strength_guides": 11, "strength_points": 12, "strength_geometry": 13, "strength_extensions": 14, "grid": false}),
    );
    for (name, want) in [
        ("strength_ruler", 0),
        ("strength_grid", 999),
        ("strength_guides", 11),
        ("strength_points", 12),
        ("strength_geometry", 13),
        ("strength_extensions", 14),
    ] {
        assert_eq!(r[name], json!(want), "{name}");
    }
    assert_eq!(r["grid"], json!(false), "an option and the strengths in one command");
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert!(s.is_dirty(), "the drawing is changed");
    assert_eq!(run(&mut s, "snap.settings", json!({})), r, "and `snap.settings` says the same");
    // The same values again, and a strength named alone that is as it was: no step, nothing recorded.
    let kept = crate::Snapshot { journal: Vec::new(), ..snapshot(&s) };
    run(&mut s, "snap.set", json!({"strength_grid": 999, "strength_guides": 11}));
    run(&mut s, "snap.set", json!({}));
    assert_eq!(crate::Snapshot { journal: Vec::new(), ..snapshot(&s) }, kept, "nothing changed, nothing recorded");
    // Undo has nothing to take back, and all six stay.
    assert!(s.execute("edit.undo", &json!({})).is_err());
    let r = run(&mut s, "snap.settings", json!({}));
    assert_eq!(r["strength_grid"], json!(999));
    assert_eq!(r["grid"], json!(false));
    assert!(STRENGTHS.iter().all(|(name, default)| r[*name] != json!(default)), "none is back at its default");
    // A change of one strength leaves the options and the other five strengths as they were.
    run(&mut s, "snap.set", json!({"strength_points": 7}));
    let r = run(&mut s, "snap.settings", json!({}));
    assert_eq!(
        (&r["strength_points"], &r["strength_geometry"], &r["strength_grid"], &r["grid"]),
        (&json!(7), &json!(13), &json!(999), &json!(false))
    );
    assert_eq!(check_session(&s), Ok(()));
}

/// A value that is not a whole number of pixels from 0 to 999 is refused in words that name the setting and the range, and nothing changes.
#[test]
fn a_strength_outside_0_to_999_or_not_a_whole_number_is_refused_in_words() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for (name, _) in STRENGTHS {
        for bad in
            [json!(1000), json!(-1), json!(5.5), json!("7"), json!(true), json!(99_999_999_999u64), json!(1.0e30), json!([4]), json!({"px": 4})]
        {
            let params = json!({name: bad});
            let err = s.execute("snap.set", &params).unwrap_err();
            let words = err.to_string();
            assert!(matches!(err, EngineError::BadParams { .. }), "{params}: {err}");
            assert!(
                words.contains(name) && words.contains("whole number of screen pixels from 0 to 999"),
                "{params}: the words name the setting and the range: {words}"
            );
        }
    }
    // A good strength beside a bad one changes nothing at all (the command is refused whole).
    let err = s.execute("snap.set", &json!({"strength_grid": 20, "strength_ruler": 1000, "grid": false})).unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err}");
    assert_eq!(snapshot(&s), before, "nothing changed, nothing recorded");
    // The ends are good.
    for edge in [0, 999] {
        let r = run(&mut s, "snap.set", json!({"strength_geometry": edge}));
        assert_eq!(r["strength_geometry"], json!(edge));
    }
}

// ----------------------------------------------------------------------------------------------------------------------------- the pulls

/// **The grid stops pulling from any distance.** The lines at 96 points to an inch are a quarter inch apart: a place 0.04 inch (3.84 pixels) from a line is pulled onto it
/// by the 5 pixels of the default, and one 0.07 inch (6.72 pixels) from it, or midway between two lines (12 pixels), is left where it is. A strength of 8 reaches the 6.72,
/// 0 reaches nothing short of the line, and the reach is in pixels: at twice the zoom the same distance is twice as far.
#[test]
fn a_place_farther_than_the_grids_strength_lands_where_it_was_put_and_one_within_it_lands_on_the_line() {
    let mut s = alone(&["grid"]);
    let (x, y, hits) = at(&mut s, 1.04, 1.04, 96.0);
    assert!((x - 1.0).abs() < 1e-9 && (y - 1.0).abs() < 1e-9, "3.84 pixels from the lines: pulled on to them: {x} {y}");
    assert_eq!(hits.as_array().unwrap().len(), 2, "one hit on each axis: {hits}");
    assert_eq!(hits[0]["target"], json!("grid"));
    let (x, y, hits) = at(&mut s, 1.07, 1.07, 96.0);
    assert!((x - 1.07).abs() < 1e-9 && (y - 1.07).abs() < 1e-9, "6.72 pixels from the lines: left where it was put: {x} {y}");
    assert!(hits.as_array().unwrap().is_empty(), "and it says it snapped to nothing: {hits}");
    let (x, y, _) = at(&mut s, 1.125, 1.125, 96.0);
    assert!((x - 1.125).abs() < 1e-9 && (y - 1.125).abs() < 1e-9, "midway between two lines (12 pixels): where it was put");
    // Only the axis that is within reach is pulled.
    let (x, y, hits) = at(&mut s, 1.04, 1.125, 96.0);
    assert!((x - 1.0).abs() < 1e-9 && (y - 1.125).abs() < 1e-9, "{x} {y}");
    assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
    // 8 pixels reach 6.72; 0 pulls only what is on the line already.
    run(&mut s, "snap.set", json!({"strength_grid": 8}));
    let (x, _, _) = at(&mut s, 1.07, 1.07, 96.0);
    assert!((x - 1.0).abs() < 1e-9, "8 pixels reach 6.72: {x}");
    run(&mut s, "snap.set", json!({"strength_grid": 0}));
    let (x, y, hits) = at(&mut s, 1.01, 1.01, 96.0);
    assert!(
        (x - 1.01).abs() < 1e-9 && (y - 1.01).abs() < 1e-9 && hits.as_array().unwrap().is_empty(),
        "0 pixels: nothing pulls 0.96 pixel: {x} {y} {hits}"
    );
    // The reach is in pixels at the zoom of the query: 0.04 inch is 3.84 pixels at 96 and 7.68 at 192 (where the lines are an eighth apart and 1.0 is one of them).
    run(&mut s, "snap.set", json!({"strength_grid": 5}));
    let (x, _, _) = at(&mut s, 1.04, 1.04, 192.0);
    assert!((x - 1.04).abs() < 1e-9, "7.68 pixels is past 5: {x}");
    let (x, _, _) = at(&mut s, 1.02, 1.02, 192.0);
    assert!((x - 1.0).abs() < 1e-9, "3.84 pixels is within 5: {x}");
    // A shape dropped with `snap` goes by the same reach: its sides and middle are 0.04 inch from the lines and are pulled, and 0.09 inch from them (8.64 pixels) they are not.
    let mut s = alone(&["grid"]);
    let close = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.04, "y": 4.04, "snap": true}));
    assert!(near(&close["snap"]["dx"], -0.04) && near(&close["snap"]["dy"], -0.04), "{close}");
    let far = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.09, "y": 4.09, "snap": true}));
    assert!(near(&far["snap"]["dx"], 0.0) && near(&far["snap"]["dy"], 0.0) && far["snap"]["hits"].as_array().unwrap().is_empty(), "{far}");
}

/// The ruler pulls within its own strength, 4 pixels at first, and does not take the nearest tick from any distance either.
#[test]
fn the_ruler_pulls_within_its_own_strength() {
    let mut s = alone(&["ruler"]);
    // Probe the ticks: with a strength that reaches everything, the nearest tick to 1.0 + a little.
    run(&mut s, "snap.set", json!({"strength_ruler": 999}));
    let (far_x, _, _) = at(&mut s, 1.3, 1.3, 96.0);
    let tick = (far_x - 1.3).abs();
    assert!(tick < 0.5, "the ruler's tick is near: {far_x}");
    run(&mut s, "snap.set", json!({"strength_ruler": 4}));
    // 1.0 is a tick of any ruler that has ticks at a power of two of an inch; 0.03 inch is 2.88 pixels, 0.06 inch 5.76 pixels.
    let on_x = |hits: &Value| hits.as_array().unwrap().iter().filter(|h| h["axis"] == json!("x")).count();
    let near_it = at(&mut s, 1.03, 1.5, 96.0);
    assert!((near_it.0 - 1.0).abs() < 1e-9 && on_x(&near_it.2) == 1, "2.88 pixels from the tick: {near_it:?}");
    assert_eq!(near_it.2[0]["target"], json!("ruler"));
    let past = at(&mut s, 1.06, 1.5, 96.0);
    assert!((past.0 - 1.06).abs() < 1e-9 && on_x(&past.2) == 0, "5.76 pixels from the tick is past 4: {past:?}");
    run(&mut s, "snap.set", json!({"strength_ruler": 6}));
    let wider = at(&mut s, 1.06, 1.5, 96.0);
    assert!((wider.0 - 1.0).abs() < 1e-9, "6 pixels reach 5.76: {wider:?}");
    // The grid's strength does not move the ruler's reach.
    run(&mut s, "snap.set", json!({"strength_ruler": 4, "strength_grid": 999}));
    let again = at(&mut s, 1.06, 1.5, 96.0);
    assert!((again.0 - 1.06).abs() < 1e-9, "the grid's strength is the grid's: {again:?}");
}

/// A guide pulls within the Guides strength, 10 pixels at first (it was 8): a place 0.09 inch (8.64 pixels) from a vertical guide is pulled, 0.12 inch (11.52) is not, and
/// 12 pixels reach it.
#[test]
fn a_guide_pulls_within_its_strength() {
    let mut s = alone(&["guides"]);
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.0}));
    let (x, _, hits) = at(&mut s, 3.09, 5.0, 96.0);
    assert!((x - 3.0).abs() < 1e-9 && hits[0]["target"] == json!("guide"), "8.64 pixels: pulled: {x} {hits}");
    let (x, _, hits) = at(&mut s, 3.12, 5.0, 96.0);
    assert!((x - 3.12).abs() < 1e-9 && hits.as_array().unwrap().is_empty(), "11.52 pixels: not pulled: {x} {hits}");
    run(&mut s, "snap.set", json!({"strength_guides": 12}));
    let (x, _, _) = at(&mut s, 3.12, 5.0, 96.0);
    assert!((x - 3.0).abs() < 1e-9, "12 pixels reach 11.52: {x}");
    run(&mut s, "snap.set", json!({"strength_guides": 8}));
    let (x, _, _) = at(&mut s, 3.09, 5.0, 96.0);
    assert!((x - 3.09).abs() < 1e-9, "8 pixels do not reach 8.64: {x}");
}

/// A rectangle dropped at (2, 2) has its right-hand connection point at (2 + w/2, 2); the end of a connector is glued to it within the Points strength, 10 pixels at first
/// (it was 12).
#[test]
fn a_connection_point_pulls_within_the_points_strength() {
    let mut s = alone(&["glue_connection_points"]);
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}));
    let right = 2.0 + r["width"].as_f64().unwrap() / 2.0;
    let glued = |s: &mut Session, d: f64| {
        let r = run(s, "snap.at", json!({"page": 1, "x": right + d, "y": 2.0, "connect": true, "zoom": 96}));
        (r["x"].as_f64().unwrap(), r["glue"]["kind"] == json!("connection_point"))
    };
    let (x, glue) = glued(&mut s, 0.09);
    assert!(glue && (x - right).abs() < 1e-9, "8.64 pixels: glued: {x}");
    let (x, glue) = glued(&mut s, 0.115);
    assert!(!glue && (x - (right + 0.115)).abs() < 1e-9, "11.04 pixels: past 10: {x}");
    run(&mut s, "snap.set", json!({"strength_points": 12}));
    let (x, glue) = glued(&mut s, 0.115);
    assert!(glue && (x - right).abs() < 1e-9, "12 pixels reach 11.04: {x}");
    run(&mut s, "snap.set", json!({"strength_points": 8}));
    let (_, glue) = glued(&mut s, 0.09);
    assert!(!glue, "8 pixels do not reach 8.64");
    // The guides' and the geometry's strengths are not the points'.
    run(&mut s, "snap.set", json!({"strength_points": 10, "strength_guides": 999, "strength_geometry": 999, "strength_grid": 999}));
    let (_, glue) = glued(&mut s, 0.115);
    assert!(!glue, "the other strengths do not reach for the points");
}

/// The outline of a shape pulls within the Geometry strength, 8 pixels at first: 0.07 inch (6.72 pixels) off the middle of a side is pulled onto it, 0.1 inch (9.6) is not,
/// and 10 pixels reach it.
#[test]
fn an_outline_pulls_within_the_geometry_strength() {
    let mut s = alone(&["glue_geometry"]);
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}));
    let right = 2.0 + r["width"].as_f64().unwrap() / 2.0;
    // Off the right-hand side, at a height that is not a connection point or a corner.
    let on_outline = |s: &mut Session, d: f64| {
        let r = run(s, "snap.at", json!({"page": 1, "x": right + d, "y": 2.2, "connect": true, "zoom": 96}));
        (r["x"].as_f64().unwrap(), r["glue"]["kind"] == json!("outline"))
    };
    let (x, glue) = on_outline(&mut s, 0.07);
    assert!(glue && (x - right).abs() < 1e-9, "6.72 pixels: onto the outline: {x}");
    let (x, glue) = on_outline(&mut s, 0.1);
    assert!(!glue && (x - (right + 0.1)).abs() < 1e-9, "9.6 pixels: past 8: {x}");
    run(&mut s, "snap.set", json!({"strength_geometry": 10}));
    let (x, glue) = on_outline(&mut s, 0.1);
    assert!(glue && (x - right).abs() < 1e-9, "10 pixels reach 9.6: {x}");
    // Not the points' strength.
    run(&mut s, "snap.set", json!({"strength_geometry": 8, "strength_points": 999}));
    let (_, glue) = on_outline(&mut s, 0.1);
    assert!(!glue, "the points' strength is not the geometry's");
}

/// The dynamic grid's alignment with another shape is the Extensions strength's, 13 pixels at first (it was 6): a box whose left edge is 0.12 inch (11.52 pixels) from the
/// left edge of a shape lines up with it, one 0.15 inch (14.4) from it does not, and 15 pixels reach it.
#[test]
fn the_dynamic_grids_alignment_pulls_within_the_extensions_strength() {
    let mut s = alone(&["dynamic_grid"]);
    // A rectangle 1.5 by 1 centred at (2, 2): its left edge is at 1.25.
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}));
    let lined = |s: &mut Session, d: f64| {
        // A box 0.4 wide and 0.4 high far above it, with its left edge d from 1.25: its other sides and its middle are far from every line of the shape.
        let r = run(s, "snap.at", json!({"page": 1, "left": 1.25 + d, "bottom": 5.0, "right": 1.65 + d, "top": 5.4, "zoom": 96}));
        (r["dx"].as_f64().unwrap(), r["hits"].clone())
    };
    let (dx, hits) = lined(&mut s, 0.12);
    assert!((dx + 0.12).abs() < 1e-9 && hits[0]["target"] == json!("alignment"), "11.52 pixels: lined up: {dx} {hits}");
    let (dx, hits) = lined(&mut s, 0.15);
    assert!(dx == 0.0 && hits.as_array().unwrap().is_empty(), "14.4 pixels: not: {dx} {hits}");
    run(&mut s, "snap.set", json!({"strength_extensions": 15}));
    let (dx, _) = lined(&mut s, 0.15);
    assert!((dx + 0.15).abs() < 1e-9, "15 pixels reach 14.4: {dx}");
    run(&mut s, "snap.set", json!({"strength_extensions": 10}));
    let (dx, _) = lined(&mut s, 0.12);
    assert!(dx == 0.0, "10 pixels do not reach 11.52: {dx}");
}

/// Each kind has its own reach: with the grid, the ruler and a guide all there, a strength set high for one kind and 0 for the rest lets only that kind pull.
#[test]
fn each_kinds_strength_is_its_own() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"alignment_box": false, "dynamic_grid": false}));
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.05}));
    let all_but = |s: &mut Session, kind: &str| {
        let mut p = serde_json::Map::new();
        for (name, _) in STRENGTHS {
            p.insert(name.to_owned(), json!(if name == kind { 999 } else { 0 }));
        }
        run(s, "snap.set", Value::Object(p));
    };
    // The place is 0.05 inch (4.8 pixels) from the guide at 3.05, 0.05 from the grid's line at 3.0, and 0.05 from the ruler's tick.
    all_but(&mut s, "strength_guides");
    let (x, _, hits) = at(&mut s, 3.0 + 0.1, 5.0 + 0.01, 96.0);
    assert!(hits.as_array().unwrap().iter().any(|h| h["target"] == json!("guide")), "only the guide reaches: {hits}");
    assert!((x - 3.05).abs() < 1e-9, "{x}");
    all_but(&mut s, "strength_grid");
    let (_, _, hits) = at(&mut s, 3.1, 5.01, 96.0);
    assert!(hits.as_array().unwrap().iter().all(|h| h["target"] != json!("guide")), "the guide's strength is 0 now: {hits}");
    assert!(hits.as_array().unwrap().iter().any(|h| h["target"] == json!("grid")), "{hits}");
    all_but(&mut s, "strength_ruler");
    let (_, _, hits) = at(&mut s, 3.1, 5.01, 96.0);
    assert!(hits.as_array().unwrap().iter().all(|h| h["target"] == json!("ruler")), "only the ruler reaches: {hits}");
}

/// The joint box of the shapes `ids` on page 1: (left, bottom, right, top), from their corners.
fn joint_box(s: &Session, ids: &[u32]) -> (f64, f64, f64, f64) {
    let d = s.document().unwrap();
    let points: Vec<(f64, f64)> = ids.iter().flat_map(|id| diacraft_engine::draw::shape_corners(d, 1, *id).unwrap()).collect();
    let xs = points.iter().map(|p| p.0);
    let ys = points.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

/// Where the right side of the box of `ids` lands when `shape.resize` with `snap` at 100% asks it to go to `right` (a side handle's drag: the other three sides stay), by `id`
/// for one shape and by `ids` for several; the resize is undone after.
fn stretched_right(s: &mut Session, ids: &[u32], right: f64) -> f64 {
    let (x0, y0, x1, y1) = joint_box(s, ids);
    let mut params = json!({"page": 1, "x": x0, "y": y0, "width": right - x0, "height": y1 - y0, "snap": {"zoom": 96}});
    match ids {
        [id] => params["id"] = json!(id),
        _ => params["ids"] = json!(ids),
    }
    run(s, "shape.resize", params);
    let (nx0, ny0, nx1, ny1) = joint_box(s, ids);
    assert!(
        (nx0 - x0).abs() < 1e-9 && (ny0 - y0).abs() < 1e-9 && (ny1 - y1).abs() < 1e-9,
        "only the right side moves: {x0} {y0} {x1} {y1} to {nx0} {ny0} {nx1} {ny1}"
    );
    run(s, "edit.undo", json!({}));
    nx1
}

/// A resize of several shapes snaps its moved side by the strengths as a resize of one shape does (`snap_resize_set` and `snap_resize_ext` are
/// one function for a box square to the page): the grid pulls the side within its strength and leaves it where it was put past it, and the reach follows `strength_grid`.
/// It holds the merge of into: before a change the grid pulled from any distance.
#[test]
fn a_set_resize_pulls_its_moved_side_within_the_grids_strength_as_a_resize_of_one_shape_does() {
    let mut s = alone(&["grid"]);
    // Two rectangles of 1.5 by 1 put down with no snap: their joint box is 1.25 to 5.75 across and 1.5 to 3.5 up.
    let (a, b) = (crate::edit::drop_shape(&mut s, "rectangle", 2.0, 2.0), crate::edit::drop_shape(&mut s, "rectangle", 5.0, 3.0));
    let (set, one) = (vec![a, b], vec![b]);
    for (what, ids) in [("the set", &set), ("one shape", &one)] {
        let landed = stretched_right(&mut s, ids, 7.04);
        assert!((landed - 7.0).abs() < 1e-6, "{what}: 3.84 pixels from the line at 7: pulled on to it: {landed}");
        let landed = stretched_right(&mut s, ids, 7.07);
        assert!((landed - 7.07).abs() < 1e-6, "{what}: 6.72 pixels from the line, past the grid's 5: left where it was put: {landed}");
    }
    run(&mut s, "snap.set", json!({"strength_grid": 8}));
    for (what, ids) in [("the set", &set), ("one shape", &one)] {
        let landed = stretched_right(&mut s, ids, 7.07);
        assert!((landed - 7.0).abs() < 1e-6, "{what}: 8 pixels reach 6.72: {landed}");
    }
    run(&mut s, "snap.set", json!({"strength_grid": 0}));
    for (what, ids) in [("the set", &set), ("one shape", &one)] {
        let landed = stretched_right(&mut s, ids, 7.01);
        assert!((landed - 7.01).abs() < 1e-6, "{what}: 0 pixels pull nothing 0.96 pixel off: {landed}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

// ----------------------------------------------------------------------------------------------------------------------------- the file

/// The text of a part of the package at `path`.
fn part_text(path: &str, name: &str) -> String {
    let bytes = std::fs::read(path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// **A saved VSDX keeps the strengths and a reopened one reads them back.** `DocumentSettings` has no element for them, so they are DiaCraft's own, attributes of the
/// `snap` child of its solution part, written for the values that are not the defaults only; a second change, saved over the file, is patched, and the defaults again
/// take the attributes (and the part) out.
#[test]
fn a_saved_vsdx_keeps_the_strengths_and_reads_them_back() {
    let dir = Dir::new();
    let path = dir.path("strength.vsdx");
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"strength_grid": 9, "strength_extensions": 100, "strength_ruler": 0}));
    let want = run(&mut s, "snap.settings", json!({}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let solution = part_text(&path, "/visio/solutions/solution1.xml");
    for attr in [r#"strength_grid="9""#, r#"strength_extensions="100""#, r#"strength_ruler="0""#] {
        assert!(solution.contains(attr), "{attr}: {solution}");
    }
    assert!(!solution.contains("strength_guides") && !solution.contains("strength_points"), "only the values that are not the defaults: {solution}");
    // The words of DocumentSettings are the file's own and say nothing of it.
    assert!(!part_text(&path, "/visio/document.xml").contains("strength"));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(run(&mut t, "snap.settings", json!({})), want, "a new session reads the same strengths");
    // A switch changed beside them keeps them, and a strength changed keeps the switch.
    run(&mut t, "snap.set", json!({"snap": false}));
    let r = run(&mut t, "snap.settings", json!({}));
    assert_eq!(
        (&r["strength_grid"], &r["strength_extensions"], &r["strength_ruler"], &r["snap"]),
        (&json!(9), &json!(100), &json!(0), &json!(false))
    );
    run(&mut t, "snap.set", json!({"strength_guides": 11}));
    run(&mut t, "file.save", json!({}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": path}));
    let r = run(&mut u, "snap.settings", json!({}));
    assert_eq!(
        (&r["strength_grid"], &r["strength_extensions"], &r["strength_ruler"], &r["strength_guides"], &r["snap"]),
        (&json!(9), &json!(100), &json!(0), &json!(11), &json!(false))
    );
    // Every strength back at its default: the attributes go, and with the switches on the part does.
    let mut back = serde_json::Map::new();
    for (name, default) in STRENGTHS {
        back.insert(name.to_owned(), json!(default));
    }
    back.insert("snap".to_owned(), json!(true));
    run(&mut u, "snap.set", Value::Object(back));
    run(&mut u, "file.save", json!({}));
    let bytes = std::fs::read(&path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    assert!(pkg.parts().all(|p| !p.contains("solution")), "the defaults need no part: {:?}", pkg.parts().collect::<Vec<_>>());
    let mut v = Session::new();
    run(&mut v, "file.open", json!({"path": path}));
    for (name, default) in STRENGTHS {
        assert_eq!(run(&mut v, "snap.settings", json!({}))[name], json!(default), "{name}");
    }
}

/// The native format carries them too, as it carries the switches: a `.diacraft` saved from a drawing with strengths reopens with them. (An `.odg` keeps DiaCraft's part in
/// its own data, as a VSDX does: the `docfile` tests of `crates/odg`.)
#[test]
fn the_native_format_keeps_the_strengths_too() {
    let dir = Dir::new();
    let path = dir.path("strength.diacraft");
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"strength_grid": 17, "strength_points": 3, "grid": false}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let r = run(&mut t, "snap.settings", json!({}));
    assert_eq!(
        (&r["strength_grid"], &r["strength_points"], &r["strength_guides"], &r["grid"]),
        (&json!(17), &json!(3), &json!(10), &json!(false)),
        "{r}"
    );
}

/// Hostile data: an attribute of the part that is not a whole number from 0 to 999 is not read (the default stands), whatever else the child holds, and the next change
/// of a strength drops it; a file whose DiaCraft data is of another version reads the defaults and refuses a change of strength in words, as it does for a switch.
#[test]
fn a_file_with_odd_strength_attributes_reads_the_defaults_for_them() {
    let dir = Dir::new();
    let path = dir.path("odd.vsdx");
    let solution = concat!(
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap "#,
        r#"strength_grid="1000" strength_ruler="-4" strength_guides="ten" strength_points="7.5" strength_geometry="" strength_extensions="21" "#,
        r#"strength_unknown="5"/></dc:Data></SolutionXML>"#
    );
    let bytes =
        diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "A")).solution("DiaCraft", solution).build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let r = run(&mut s, "snap.settings", json!({}));
    assert_eq!(
        (&r["strength_grid"], &r["strength_ruler"], &r["strength_guides"], &r["strength_points"], &r["strength_geometry"], &r["strength_extensions"]),
        (&json!(5), &json!(4), &json!(10), &json!(10), &json!(8), &json!(21)),
        "{r}"
    );
    // A change of one strength writes the child again with the strengths that are not the defaults, and what was not read is gone.
    run(&mut s, "snap.set", json!({"strength_guides": 12}));
    let copy = dir.path("odd2.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    let solution = part_text(&copy, "/visio/solutions/solution1.xml");
    assert!(solution.contains(r#"strength_guides="12""#) && solution.contains(r#"strength_extensions="21""#), "{solution}");
    for gone in ["strength_grid=", "strength_ruler=", "=\"ten\"", "strength_points=", "strength_geometry=", "strength_unknown"] {
        assert!(!solution.contains(gone), "{gone} was not read, and is not kept: {solution}");
    }
}

#[test]
fn data_of_another_version_refuses_a_change_of_strength_in_words() {
    let dir = Dir::new();
    let path = dir.path("later.vsdx");
    let solution = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="9"><dc:snap strength_grid="40"/></dc:Data></SolutionXML>"#;
    let bytes =
        diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "A")).solution("DiaCraft", solution).build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["strength_grid"], json!(5), "not interpreted");
    let before = snapshot(&s);
    let err = s.execute("snap.set", &json!({"strength_grid": 20})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }) && err.to_string().contains("DiaCraft data is of a version"), "{err}");
    assert_eq!(snapshot(&s).document, before.document, "nothing changed");
}
