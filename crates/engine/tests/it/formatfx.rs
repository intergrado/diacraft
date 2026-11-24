//! The fill's and line's transparency, the pattern fill, the gradients, the shadow and the Quick Styles, through `Session::execute` as the control
//! channel calls them. Each command writes the cells a file holds, and what is drawn is looked at too: a transparency is a blend with the
//! page, a gradient runs from its first stop to its last, a shadow is a darker patch beside the shape.

use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, SectionKey};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape};
use crate::format::{close, fv, local, number, px, render};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

fn session_with_rectangle() -> (Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    (s, id)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

/// What `shape.formatting` says of shape `id`.
fn look(s: &mut Session, id: u32) -> Value {
    run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0].clone()
}

/// A cell of a row of a section the shape holds itself.
fn row_cell(d: &Document, id: u32, section: &str, ix: u32, name: &str) -> Option<diacraft_doc::Cell> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id)?)?;
    let addr = CellAddr::in_row(section, None, RowKey::Ix(ix), name);
    match shape.sheet.lookup(&addr) {
        Lookup::Found(c) => Some(c.clone()),
        _ => None,
    }
}

/// Whether the shape holds the row `ix` of a section as deleted (`Del`).
fn row_deleted(d: &Document, id: u32, section: &str, ix: u32) -> bool {
    // Used by the theme test below.
    let Some(at) = d.find_shape(Owner::Page(1), id) else { return false };
    let Some(shape) = d.shape(&at) else { return false };
    shape.sheet.section(&SectionKey::new(section, None)).and_then(|s| s.row(&RowKey::Ix(ix))).is_some_and(|r| r.del)
}

fn refused(s: &mut Session, command: &str, params: Value) -> String {
    let before = snapshot(s);
    let msg = bad_params_msg(&s.execute(command, &params).expect_err("must be refused")).to_owned();
    assert_eq!(snapshot(s), before, "a refusal changes nothing");
    msg
}

fn failed(s: &mut Session, command: &str, params: Value) -> String {
    let before = snapshot(s);
    let e = s.execute(command, &params).expect_err("must fail");
    assert_eq!(snapshot(s), before, "a failure changes nothing");
    match e {
        EngineError::Failed { msg, .. } => msg,
        other => panic!("expected a failure, got {other:?}"),
    }
}

// ---- transparency ------------------------------------------------------------------------------------------------------------

#[test]
fn a_fill_transparency_is_a_share_on_half_percent_steps_and_the_shape_is_drawn_through() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#000000"}));
    assert!(close(px(&render(&s), 3.0, 4.0), (0, 0, 0), 2), "opaque black before");
    let r = run(&mut s, "shape.format", json!({"ids": [id], "fill_transparency": 50}));
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    let c = local(doc(&s), id, "FillForegndTrans").expect("a local FillForegndTrans");
    assert!(near(number(&c), 0.5), "{c:?}");
    // Half clear black over the white page is mid grey.
    let p = px(&render(&s), 3.0, 4.0);
    assert!(close(p, (128, 128, 128), 3), "{p:?}");
    // A writer rounds to the nearest 0.005, so 35.3 percent is 0.355 and 35.2 is 0.35.
    run(&mut s, "shape.format", json!({"ids": [id], "fill_transparency": 35.3}));
    assert!(near(number(&local(doc(&s), id, "FillForegndTrans").unwrap()), 0.355));
    run(&mut s, "shape.format", json!({"ids": [id], "fill_transparency": 35.2}));
    assert!(near(number(&local(doc(&s), id, "FillForegndTrans").unwrap()), 0.35));
    assert_eq!(look(&mut s, id)["fill_transparency"], json!(35.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_transparency_is_refused_in_words_outside_zero_to_one_hundred() {
    let (mut s, id) = session_with_rectangle();
    for (key, value) in [("fill_transparency", json!(101)), ("fill_transparency", json!(-1)), ("line_transparency", json!(100.5))] {
        let msg = refused(&mut s, "shape.format", json!({"ids": [id], key: value}));
        assert!(msg.contains(key) && msg.contains("from 0 to 100"), "{msg}");
    }
    let msg = refused(&mut s, "shape.format", json!({"ids": [id], "fill_transparency": "half"}));
    assert!(msg.contains("must be a number"), "{msg}");
}

#[test]
fn a_line_transparency_writes_the_line_colour_trans_and_is_one_undo_step() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "line": "#CC0000", "weight": 6}));
    let before = render(&s);
    run(&mut s, "shape.format", json!({"ids": [id], "line_transparency": 75}));
    assert!(near(number(&local(doc(&s), id, "LineColorTrans").unwrap()), 0.75));
    // The left edge of the box at x 2.25 is a quarter as red as it was.
    let (a, b) = (px(&before, 2.25, 4.0), px(&render(&s), 2.25, 4.0));
    assert!(a[1] < 20 && b[1] > 150, "{a:?} then {b:?}");
    run(&mut s, "edit.undo", json!({}));
    assert!(local(doc(&s), id, "LineColorTrans").is_none(), "one undo takes the transparency back");
    assert!(close(px(&render(&s), 2.25, 4.0), (204, 0, 0), 4));
}

// ---- patterns ----------------------------------------------------------------------------------------------------------------

#[test]
fn a_pattern_fill_writes_the_pattern_and_the_ground_and_turns_a_gradient_off() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.gradient", json!({"ids": [id], "stops": [{"position": 0, "color": "#FF0000"}, {"position": 1, "color": "#0000FF"}]}));
    assert_eq!(number(&local(doc(&s), id, "FillGradientEnabled").unwrap()), 1.0);
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#000000", "fill_pattern": 2, "fill_background": "#FFFF00"}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "FillPattern").unwrap()), 2.0);
    assert_eq!(fv(&local(d, id, "FillBkgnd").unwrap()), (Some("RGB(255,255,0)".to_owned()), "#ffff00".to_owned()));
    assert_eq!(number(&local(d, id, "FillGradientEnabled").unwrap()), 0.0, "The gradient is off, its rows stay");
    assert!(row_cell(d, id, "FillGradient", 0, "GradientStopColor").is_some());
    // A hatch over a yellow ground: both the ink and the ground are drawn in the box, and the middle is neither white nor red.
    let r = render(&s);
    let mut ink = 0;
    let mut ground = 0;
    for i in 0..60 {
        let p = px(&r, 2.6 + f64::from(i) * 0.01, 4.0);
        ink += usize::from(close(p, (0, 0, 0), 30));
        ground += usize::from(close(p, (255, 255, 0), 30));
    }
    assert!(ink > 0 && ground > 0, "{ink} ink pixels and {ground} ground pixels along a line through the box");
    let l = look(&mut s, id);
    assert_eq!(l["fill_pattern"], json!(2));
    assert_eq!(l["fill_background"], json!("#ffff00"));
    assert!(l["gradient"].is_null());
    let msg = refused(&mut s, "shape.format", json!({"ids": [id], "fill_pattern": 25}));
    assert!(msg.contains("from 1 (solid) to 24") && msg.contains("shape.gradient"), "{msg}");
}

#[test]
fn a_pattern_with_a_transparency_makes_the_ground_as_clear_as_the_marks() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill_pattern": 3, "fill_transparency": 40}));
    assert!(near(number(&local(doc(&s), id, "FillForegndTrans").unwrap()), 0.4));
    assert!(near(number(&local(doc(&s), id, "FillBkgndTrans").unwrap()), 0.4), "The ground has its own transparency cell");
    // A solid fill leaves the ground's alone.
    run(&mut s, "shape.format", json!({"ids": [id], "fill_pattern": 1, "fill_transparency": 10}));
    assert!(near(number(&local(doc(&s), id, "FillBkgndTrans").unwrap()), 0.4));
    assert!(near(number(&local(doc(&s), id, "FillForegndTrans").unwrap()), 0.1));
}

#[test]
fn the_cap_and_the_compound_line_are_words_and_numbers_of_their_cells() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "line_cap": "square", "compound": "thick_thin"}));
    assert_eq!(number(&local(doc(&s), id, "LineCap").unwrap()), 2.0, "2 draws the projecting square cap");
    assert_eq!(number(&local(doc(&s), id, "CompoundType").unwrap()), 2.0);
    let l = look(&mut s, id);
    assert_eq!((l["line_cap"].as_str(), l["compound"].as_str()), (Some("square"), Some("thick_thin")));
    run(&mut s, "shape.format", json!({"ids": [id], "line_cap": "flat", "compound": "single"}));
    assert_eq!(number(&local(doc(&s), id, "LineCap").unwrap()), 1.0);
    assert_eq!(number(&local(doc(&s), id, "CompoundType").unwrap()), 0.0);
    let msg = refused(&mut s, "shape.format", json!({"ids": [id], "line_cap": "pointy"}));
    assert!(msg.contains("round, flat or square") && msg.contains("pointy"), "{msg}");
    let msg = refused(&mut s, "shape.format", json!({"ids": [id], "compound": "quadruple"}));
    assert!(msg.contains("single, double, thick_thin, thin_thick or triple"), "{msg}");
}

// ---- gradients ---------------------------------------------------------------------------------------------------------------

fn red_to_blue() -> Value {
    json!([{"position": 0, "color": "#FF0000"}, {"position": 1, "color": "#0000FF"}])
}

#[test]
fn a_linear_gradient_runs_from_its_first_stop_to_its_last_at_its_angle() {
    let (mut s, id) = session_with_rectangle();
    let r = run(&mut s, "shape.gradient", json!({"ids": [id], "kind": "linear", "angle": 90, "stops": red_to_blue()}));
    assert_eq!(r, json!({"page": 1, "target": "fill", "formatted": 1}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "FillGradientEnabled").unwrap()), 1.0);
    assert_eq!(number(&local(d, id, "FillGradientDir").unwrap()), 0.0);
    assert!(near(number(&local(d, id, "FillGradientAngle").unwrap()), std::f64::consts::FRAC_PI_2), "radians, as the cell holds an angle");
    assert_eq!(fv(&row_cell(d, id, "FillGradient", 0, "GradientStopColor").unwrap()), (Some("RGB(255,0,0)".to_owned()), "#ff0000".to_owned()));
    assert_eq!(fv(&row_cell(d, id, "FillGradient", 1, "GradientStopColor").unwrap()), (Some("RGB(0,0,255)".to_owned()), "#0000ff".to_owned()));
    assert_eq!(number(&row_cell(d, id, "FillGradient", 1, "GradientStopPosition").unwrap()), 1.0);
    assert_eq!(number(&row_cell(d, id, "FillGradient", 0, "GradientStopColorTrans").unwrap()), 0.0);
    // 90 degrees runs from the top, the first stop, to the bottom, the last. The box is 2.25 to 3.75 across and 3.5 to 4.5 up.
    let r = render(&s);
    let (top, bottom) = (px(&r, 3.0, 4.45), px(&r, 3.0, 3.55));
    assert!(top[0] > 200 && top[2] < 60, "red at the top: {top:?}");
    assert!(bottom[2] > 200 && bottom[0] < 60, "blue at the bottom: {bottom:?}");
    // Angle 0 runs from the left to the right instead.
    run(&mut s, "shape.gradient", json!({"ids": [id], "angle": 0}));
    let r = render(&s);
    let (left, right) = (px(&r, 2.3, 4.0), px(&r, 3.7, 4.0));
    assert!(left[0] > 200 && left[2] < 60, "red at the left: {left:?}");
    assert!(right[2] > 200 && right[0] < 60, "blue at the right: {right:?}");
    let g = look(&mut s, id)["gradient"].clone();
    assert_eq!((g["kind"].as_str(), g["angle"].as_f64(), g["stops"].as_array().map(Vec::len)), (Some("linear"), Some(0.0), Some(2)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_gradient_angle_a_hair_below_zero_is_written_as_zero_and_not_as_minus_zero() {
    // Gaps F39: the angle in radians was rounded to nine places after the test for 0, so an angle below 0 by less than half a billionth of a radian
    // was written as -0.
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.gradient", json!({"ids": [id], "kind": "linear", "angle": -1.0e-10, "stops": red_to_blue()}));
    assert_eq!(fv(&local(doc(&s), id, "FillGradientAngle").unwrap()), (None, "0".to_owned()));
    run(&mut s, "shape.gradient", json!({"ids": [id], "angle": -720.0}));
    assert_eq!(fv(&local(doc(&s), id, "FillGradientAngle").unwrap()), (None, "0".to_owned()), "two turns below 0 are none");
}

#[test]
fn a_gradient_is_one_undo_step_and_the_same_call_again_is_none() {
    let (mut s, id) = session_with_rectangle();
    let steps = |s: &Session| s.history().map_or(0, |h| h.undo_len());
    let base = steps(&s);
    run(&mut s, "shape.gradient", json!({"ids": [id], "stops": red_to_blue(), "angle": 45}));
    assert_eq!(steps(&s), base + 1);
    let again = run(&mut s, "shape.gradient", json!({"ids": [id], "stops": red_to_blue(), "angle": 45}));
    assert_eq!(again["formatted"], 0);
    assert_eq!(steps(&s), base + 1, "the same gradient again is no step");
    run(&mut s, "edit.undo", json!({}));
    assert!(local(doc(&s), id, "FillGradientEnabled").is_none());
    assert!(row_cell(doc(&s), id, "FillGradient", 0, "GradientStopColor").is_none());
}

#[test]
fn stops_are_written_in_position_order_and_each_has_its_own_transparency() {
    let (mut s, id) = session_with_rectangle();
    run(
        &mut s,
        "shape.gradient",
        json!({"ids": [id], "stops": [
            {"position": 1, "color": "#0000FF", "transparency": 75},
            {"position": 0, "color": "#FF0000"},
            {"position": 0.5, "color": "#00FF00", "transparency": 20.2}]}),
    );
    let d = doc(&s);
    let col = |ix| row_cell(d, id, "FillGradient", ix, "GradientStopColor").unwrap().value.unwrap().text().to_owned();
    assert_eq!((col(0), col(1), col(2)), ("#ff0000".to_owned(), "#00ff00".to_owned(), "#0000ff".to_owned()), "Rising positions from IX 0");
    assert!(near(number(&row_cell(d, id, "FillGradient", 2, "GradientStopColorTrans").unwrap()), 0.75));
    assert!(near(number(&row_cell(d, id, "FillGradient", 1, "GradientStopColorTrans").unwrap()), 0.2), "Half-percent steps");
    // Transparency belongs to the stop, so the stops differ; the same number on every stop is one over the shape.
    let g = look(&mut s, id)["gradient"].clone();
    let t: Vec<f64> = g["stops"].as_array().unwrap().iter().map(|x| x["transparency"].as_f64().unwrap()).collect();
    assert_eq!(t, vec![0.0, 20.0, 75.0]);
    run(&mut s, "shape.format", json!({"ids": [id], "fill_transparency": 75}));
    let g = look(&mut s, id)["gradient"].clone();
    let t: Vec<f64> = g["stops"].as_array().unwrap().iter().map(|x| x["transparency"].as_f64().unwrap()).collect();
    assert_eq!(t, vec![75.0, 75.0, 75.0], "75 on every stop");
}

#[test]
fn a_gradient_refuses_the_wrong_number_of_stops_and_bad_values_in_words() {
    let (mut s, id) = session_with_rectangle();
    let one = refused(&mut s, "shape.gradient", json!({"ids": [id], "stops": [{"position": 0, "color": "#FF0000"}]}));
    assert!(one.contains("from 2 to 10 stops"), "{one}");
    let eleven: Vec<Value> = (0..11).map(|i| json!({"position": f64::from(i) / 10.0, "color": "#FF0000"})).collect();
    let many = refused(&mut s, "shape.gradient", json!({"ids": [id], "stops": eleven}));
    assert!(many.contains("from 2 to 10 stops") && many.contains("11"), "{many}");
    let bad_colour =
        refused(&mut s, "shape.gradient", json!({"ids": [id], "stops": [{"position": 0, "color": "red"}, {"position": 1, "color": "#000000"}]}));
    assert!(bad_colour.contains("stop 0") && bad_colour.contains("colour"), "{bad_colour}");
    let bad_at = refused(
        &mut s,
        "shape.gradient",
        json!({"ids": [id], "stops": [{"position": 0, "color": "#FF0000"}, {"position": 1.5, "color": "#000000"}]}),
    );
    assert!(bad_at.contains("stop 1") && bad_at.contains("0 to 1"), "{bad_at}");
    let nothing = refused(&mut s, "shape.gradient", json!({"ids": [id]}));
    assert!(nothing.contains("nothing to set"), "{nothing}");
    let both = refused(&mut s, "shape.gradient", json!({"ids": [id], "direction": 3, "kind": "radial"}));
    assert!(both.contains("not both"), "{both}");
    let origin = refused(&mut s, "shape.gradient", json!({"ids": [id], "kind": "rectangular", "origin": "top"}));
    assert!(origin.contains("rectangular") && origin.contains("origin"), "{origin}");
    let kind = refused(&mut s, "shape.gradient", json!({"ids": [id], "kind": "conic"}));
    assert!(kind.contains("linear, radial, rectangular or path") && kind.contains("conic"), "{kind}");
    let target = refused(&mut s, "shape.gradient", json!({"ids": [id], "target": "text", "angle": 1}));
    assert!(target.contains("fill or line"), "{target}");
}

#[test]
fn the_layouts_are_the_numbers_of_the_cell_by_its_table() {
    let (mut s, id) = session_with_rectangle();
    for (kind, origin, dir) in [
        ("linear", None, 0),
        ("radial", Some("center"), 3),
        ("radial", Some("top_left"), 7),
        ("radial", Some("bottom"), 4),
        ("rectangular", Some("top_left"), 12),
        ("rectangular", Some("bottom_right"), 8),
        ("rectangular", None, 10),
        ("path", None, 13),
    ] {
        let mut params = json!({"ids": [id], "kind": kind, "stops": red_to_blue()});
        if let Some(origin) = origin {
            params["origin"] = json!(origin);
        }
        run(&mut s, "shape.gradient", params);
        assert_eq!(number(&local(doc(&s), id, "FillGradientDir").unwrap()), f64::from(dir), "{kind} {origin:?}");
        let g = look(&mut s, id)["gradient"].clone();
        assert_eq!(g["direction"], json!(dir));
        assert_eq!(g["kind"], json!(kind));
    }
    // A radial gradient from the centre runs from the first stop in the middle to the last at the edge.
    run(&mut s, "shape.gradient", json!({"ids": [id], "kind": "radial", "origin": "center", "stops": red_to_blue()}));
    let r = render(&s);
    let (middle, edge) = (px(&r, 3.0, 4.0), px(&r, 2.3, 4.45));
    assert!(middle[0] > edge[0] && edge[2] > middle[2], "{middle:?} at the middle and {edge:?} at the edge");
}

#[test]
fn a_gradient_turned_on_with_no_stops_is_made_of_the_shapes_colour_and_a_lighter_one() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#3C78D8"}));
    run(&mut s, "shape.gradient", json!({"ids": [id], "on": true}));
    let g = look(&mut s, id)["gradient"].clone();
    let stops = g["stops"].as_array().unwrap();
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0]["color"], json!("#3c78d8"), "The first stop is the colour the shape had");
    assert_eq!(stops[1]["color"], json!("#ffffff"));
    // Off keeps the stops, and a colour picked after turns it off too.
    run(&mut s, "shape.gradient", json!({"ids": [id], "on": false}));
    assert!(look(&mut s, id)["gradient"].is_null());
    assert!(row_cell(doc(&s), id, "FillGradient", 1, "GradientStopColor").is_some(), "The rows stay");
    // On again with none given keeps the shape's own stops.
    run(&mut s, "shape.gradient", json!({"ids": [id], "stops": red_to_blue()}));
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#00FF00"}));
    assert!(look(&mut s, id)["gradient"].is_null(), "A colour turns the gradient off");
    run(&mut s, "shape.gradient", json!({"ids": [id], "on": true}));
    let g = look(&mut s, id)["gradient"].clone();
    assert_eq!(g["stops"][0]["color"], json!("#ff0000"));
}

#[test]
fn a_gradient_on_a_shape_with_no_fill_gives_it_one() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "none"}));
    assert_eq!(number(&local(doc(&s), id, "FillPattern").unwrap()), 0.0);
    run(&mut s, "shape.gradient", json!({"ids": [id], "stops": red_to_blue()}));
    assert_eq!(number(&local(doc(&s), id, "FillPattern").unwrap()), 1.0, "Pattern 0 paints nothing whatever else is set");
    assert!(look(&mut s, id)["gradient"].is_object());
}

#[test]
fn a_line_gradient_is_the_rows_of_line_gradient() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.format", json!({"ids": [id], "line": "#000000", "weight": 6}));
    let r = run(&mut s, "shape.gradient", json!({"ids": [id], "target": "line", "angle": 90, "stops": red_to_blue()}));
    assert_eq!(r["target"], "line");
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "LineGradientEnabled").unwrap()), 1.0);
    assert!(local(d, id, "FillGradientEnabled").is_none(), "the fill is left alone");
    assert_eq!(row_cell(d, id, "LineGradient", 0, "GradientStopColor").unwrap().value.unwrap().text(), "#ff0000");
    assert_eq!(look(&mut s, id)["line_gradient"]["stops"][1]["color"], json!("#0000ff"));
    run(&mut s, "shape.format", json!({"ids": [id], "line_transparency": 50}));
    let g = look(&mut s, id)["line_gradient"].clone();
    assert_eq!(g["stops"][0]["transparency"], json!(50.0), "the same on every stop of the line's gradient");
}

#[test]
fn a_gradient_on_a_group_is_given_to_its_members() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 4.0);
    let g = crate::edit::id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let r = run(&mut s, "shape.gradient", json!({"ids": [g], "stops": red_to_blue()}));
    assert_eq!(r["formatted"], 2);
    for id in [a, b] {
        assert!(look(&mut s, id)["gradient"].is_object());
    }
}

// ---- shadows -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_shadow_preset_writes_the_cells_and_the_shadow_falls_where_it_says() {
    let (mut s, id) = session_with_rectangle();
    let r = run(&mut s, "shape.shadow", json!({"ids": [id], "preset": "bottom_right", "color": "#000000", "transparency": 0}));
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    let d = doc(&s);
    assert_eq!(number(&local(d, id, "ShdwPattern").unwrap()), 1.0);
    assert_eq!(number(&local(d, id, "ShapeShdwType").unwrap()), 1.0, "an outer shadow of the shape's own cells, not the page's");
    // 4 points at 45 degrees clockwise from the right, as the page shows it: right and down, and down is negative in the cells' y.
    let k = 4.0 * std::f64::consts::FRAC_1_SQRT_2 / 72.0;
    assert!((number(&local(d, id, "ShapeShdwOffsetX").unwrap()) - k).abs() < 1.0e-8);
    assert!((number(&local(d, id, "ShapeShdwOffsetY").unwrap()) + k).abs() < 1.0e-8);
    assert!(near(number(&local(d, id, "ShapeShdwBlur").unwrap()), 4.0 / 72.0));
    assert!(near(number(&local(d, id, "ShapeShdwScaleFactor").unwrap()), 1.0));
    assert_eq!(fv(&local(d, id, "ShdwForegnd").unwrap()), (Some("RGB(0,0,0)".to_owned()), "#000000".to_owned()));
    let sh = look(&mut s, id)["shadow"].clone();
    assert_eq!(
        (sh["angle"].as_f64(), sh["distance"].as_f64(), sh["blur"].as_f64(), sh["size"].as_f64()),
        (Some(45.0), Some(4.0), Some(4.0), Some(100.0))
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_distance_and_an_angle_are_the_offset_in_polar_form_and_read_back() {
    let (mut s, id) = session_with_rectangle();
    // The pane's Angle 70 and Distance 9 pt.
    run(&mut s, "shape.shadow", json!({"ids": [id], "angle": 70, "distance": 9, "blur": 6, "transparency": 38, "size": 134}));
    let d = doc(&s);
    let (x, y) = (number(&local(d, id, "ShapeShdwOffsetX").unwrap()), number(&local(d, id, "ShapeShdwOffsetY").unwrap()));
    assert!((x - 9.0 * 70_f64.to_radians().cos() / 72.0).abs() < 1.0e-8 && (y + 9.0 * 70_f64.to_radians().sin() / 72.0).abs() < 1.0e-8, "{x} {y}");
    assert!(near(number(&local(d, id, "ShdwForegndTrans").unwrap()), 0.38));
    assert!(near(number(&local(d, id, "ShapeShdwScaleFactor").unwrap()), 1.34));
    let sh = look(&mut s, id)["shadow"].clone();
    assert_eq!(
        (sh["angle"].as_f64(), sh["distance"].as_f64(), sh["transparency"].as_f64(), sh["size"].as_f64()),
        (Some(70.0), Some(9.0), Some(38.0), Some(134.0))
    );
    // Straight down is 90, and straight right is 0.
    run(&mut s, "shape.shadow", json!({"ids": [id], "angle": 90, "distance": 9}));
    let d = doc(&s);
    assert!(number(&local(d, id, "ShapeShdwOffsetX").unwrap()).abs() < 1.0e-9);
    assert!(near(number(&local(d, id, "ShapeShdwOffsetY").unwrap()), -9.0 / 72.0));
}

#[test]
fn a_shadow_that_is_on_has_only_what_is_given_written_and_the_rest_keeps_its_place() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.shadow", json!({"ids": [id], "angle": 30, "distance": 12, "blur": 3}));
    let x = number(&local(doc(&s), id, "ShapeShdwOffsetX").unwrap());
    run(&mut s, "shape.shadow", json!({"ids": [id], "blur": 9}));
    assert!(near(number(&local(doc(&s), id, "ShapeShdwBlur").unwrap()), 9.0 / 72.0));
    assert_eq!(number(&local(doc(&s), id, "ShapeShdwOffsetX").unwrap()), x, "the offset is as it was");
    // Only the distance: the angle stays 30.
    run(&mut s, "shape.shadow", json!({"ids": [id], "distance": 24}));
    let sh = look(&mut s, id)["shadow"].clone();
    assert_eq!((sh["angle"].as_f64(), sh["distance"].as_f64()), (Some(30.0), Some(24.0)));
}

#[test]
fn a_shadow_taken_away_is_no_pattern_and_the_picture_shows_none() {
    let (mut s, id) = session_with_rectangle();
    run(&mut s, "shape.shadow", json!({"ids": [id], "preset": "bottom_right", "distance": 20, "blur": 0, "transparency": 0}));
    // 20 points is 0.28 inch right and down: the pixel just beyond the box's bottom right corner (3.75, 3.5) is the shadow's.
    let with = px(&render(&s), 3.85, 3.4);
    assert!(with[0] < 80 && with[3] == 255, "dark beside the corner: {with:?}");
    run(&mut s, "shape.shadow", json!({"ids": [id], "on": false}));
    assert_eq!(number(&local(doc(&s), id, "ShdwPattern").unwrap()), 0.0, "No pattern, no shadow");
    assert!(look(&mut s, id)["shadow"].is_null());
    assert_eq!(px(&render(&s), 3.85, 3.4), [255, 255, 255, 255]);
    run(&mut s, "shape.shadow", json!({"ids": [id], "preset": "none"}));
    // The preset `none` is the same as on false, and a call that changes nothing is no step.
    assert_eq!(run(&mut s, "shape.shadow", json!({"ids": [id], "preset": "none"}))["formatted"], 0);
}

#[test]
fn a_shadow_is_refused_in_words_outside_its_ranges() {
    let (mut s, id) = session_with_rectangle();
    for (key, value, words) in [
        ("blur", json!(1001), "from 0 to 1000 points"),
        ("distance", json!(-1), "from 0 to 1000 points"),
        ("size", json!(0), "from 1 to 1000 percent"),
        ("transparency", json!(120), "from 0 to 100 percent"),
        ("color", json!("black"), "colour"),
        ("preset", json!("everywhere"), "bottom_right"),
        ("on", json!("yes"), "true or false"),
    ] {
        let msg = refused(&mut s, "shape.shadow", json!({"ids": [id], key: value}));
        assert!(msg.contains(words), "{key}: {msg}");
    }
    let msg = refused(&mut s, "shape.shadow", json!({"ids": [id]}));
    assert!(msg.contains("nothing to set"), "{msg}");
}

#[test]
fn a_shadow_on_a_shape_whose_shadow_is_the_pages_takes_the_pages_offsets_along() {
    let (mut s, id) = session_with_rectangle();
    // The page's shadow: type 1, offset 0.2 right and 0.1 down, which a shape of type 0 draws.
    run(&mut s, "shape.shadow", json!({"ids": [id], "on": true}));
    run(&mut s, "shape.shadow", json!({"ids": [id], "blur": 2}));
    let sh = look(&mut s, id)["shadow"].clone();
    assert_eq!(sh["kind"], 1);
}

// ---- Quick Styles ------------------------------------------------------------------------------------------------------------

fn themed_session() -> (Session, u32) {
    let (mut s, _) = session_with_rectangle();
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "page": 1}));
    let id = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    (s, id)
}

#[test]
fn a_quick_style_is_refused_in_words_when_the_page_has_no_theme() {
    let (mut s, id) = session_with_rectangle();
    let msg = failed(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": 1}));
    assert!(msg.contains("no theme") && msg.contains("Design"), "{msg}");
    let msg = failed(&mut s, "shape.quick_styles", json!({"ids": [id]}));
    assert!(msg.contains("no theme"), "{msg}");
}

#[test]
fn a_quick_style_writes_its_two_numbers_and_the_theme_answers_the_cells() {
    let (mut s, id) = themed_session();
    let before = look(&mut s, id);
    let r = run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 4, "column": 3}));
    assert_eq!((r["matrix"].as_u64(), r["colour"].as_u64(), r["formatted"].as_u64()), (Some(4), Some(102), Some(1)));
    let d = doc(&s);
    for name in ["QuickStyleLineMatrix", "QuickStyleFillMatrix", "QuickStyleEffectsMatrix"] {
        assert_eq!(number(&local(d, id, name).unwrap()), 4.0, "{name}");
    }
    for name in ["QuickStyleLineColor", "QuickStyleFillColor", "QuickStyleShadowColor"] {
        assert_eq!(number(&local(d, id, name).unwrap()), 102.0, "{name}");
    }
    let after = look(&mut s, id);
    assert_ne!(after["fill"], before["fill"], "a different colour from the theme: {before} then {after}");
    // The same Quick Style again is no step.
    assert_eq!(run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 4, "column": 3}))["formatted"], 0);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_quick_style_takes_a_colour_a_person_set_back_to_the_theme() {
    let (mut s, id) = themed_session();
    run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": 1}));
    let themed = look(&mut s, id)["fill"].clone();
    run(&mut s, "shape.format", json!({"ids": [id], "fill": "#FF00FF"}));
    assert_eq!(look(&mut s, id)["fill"], json!("#ff00ff"));
    // Choosing a style replaces the colour with the theme's call.
    run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": 1}));
    assert_eq!(look(&mut s, id)["fill"], themed);
    let c = local(doc(&s), id, "FillForegnd").unwrap();
    assert_eq!(fv(&c), (Some("THEMEVAL()".to_owned()), "Themed".to_owned()));
}

#[test]
fn the_variant_styles_are_the_first_to_fourth_style_of_the_pages_variant() {
    let (mut s, id) = themed_session();
    run(&mut s, "shape.quick_style", json!({"ids": [id], "variant": 2}));
    assert_eq!(number(&local(doc(&s), id, "QuickStyleFillMatrix").unwrap()), 102.0);
    assert_eq!(number(&local(doc(&s), id, "QuickStyleFillColor").unwrap()), 100.0);
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "variant": 4}));
    assert!(msg.contains("0 to 3"), "{msg}");
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "row": 7, "column": 1}));
    assert!(msg.contains("from 1 to 6") && msg.contains("from 1 to 7"), "{msg}");
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1}));
    assert!(msg.contains("go together"), "{msg}");
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id]}));
    assert!(msg.contains("variant"), "{msg}");
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": 1, "variant": 1}));
    assert!(msg.contains("not both"), "{msg}");
}

#[test]
fn a_quick_style_of_a_place_at_the_end_of_the_whole_numbers_is_refused_in_words() {
    // Gaps F39: `QuickStyle::at` and `variant` worked the style's numbers out before they asked whether the place is one of the gallery's, so a column or a
    // variant near the largest whole number overflowed.
    let (mut s, id) = themed_session();
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": u32::MAX}));
    assert!(msg.contains("from 1 to 6") && msg.contains("from 1 to 7"), "{msg}");
    let msg = refused(&mut s, "shape.quick_style", json!({"ids": [id], "variant": u32::MAX}));
    assert!(msg.contains("0 to 3"), "{msg}");
}

#[test]
fn the_gallery_is_four_variant_styles_and_forty_two_theme_styles_that_differ() {
    let (mut s, id) = themed_session();
    let g = run(&mut s, "shape.quick_styles", json!({"ids": [id]}));
    assert_eq!(g["variants"].as_array().map(Vec::len), Some(4));
    let styles = g["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 42);
    assert_eq!((styles[0]["row"].as_u64(), styles[0]["column"].as_u64()), (Some(1), Some(1)));
    assert_eq!((styles[41]["row"].as_u64(), styles[41]["column"].as_u64()), (Some(6), Some(7)));
    // The seven columns of a row are seven colours; the six rows are not all one look.
    let fills: std::collections::BTreeSet<String> = styles.iter().take(7).map(|t| t["fill"].as_str().unwrap().to_owned()).collect();
    assert!(fills.len() >= 4, "the columns are colours: {fills:?}");
    let shapes: std::collections::BTreeSet<String> = styles.iter().map(|t| format!("{}{}{}", t["fill"], t["line"], t["weight"])).collect();
    assert!(shapes.len() > 14, "the rows are styles: {} distinct tiles", shapes.len());
    // A tile is what the style makes of the shape: apply tile (3, 5) and the shape looks like it.
    let tile = &styles[2 * 7 + 4];
    run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 3, "column": 5}));
    let l = look(&mut s, id);
    assert_eq!(l["fill"], tile["fill"], "the tile's fill is the shape's");
    assert_eq!(l["line"], tile["line"]);
    let now = run(&mut s, "shape.quick_styles", json!({"ids": [id]}));
    assert_eq!((now["current"]["matrix"].as_u64(), now["current"]["colour"].as_u64()), (Some(3), Some(104)));
}

#[test]
fn the_gallery_changes_nothing() {
    let (mut s, id) = themed_session();
    let before = snapshot(&s);
    run(&mut s, "shape.quick_styles", json!({"ids": [id]}));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_gradient_on_a_shape_that_follows_the_theme_deletes_the_style_rows_it_does_not_use() {
    let (mut s, id) = themed_session();
    // The Theme style holds ten rows of stops; a shape that follows it reads them, and a gradient of three keeps three.
    run(
        &mut s,
        "shape.gradient",
        json!({"ids": [id], "stops": [
        {"position": 0, "color": "#FF0000"}, {"position": 0.5, "color": "#00FF00"}, {"position": 1, "color": "#0000FF"}]}),
    );
    let d = doc(&s);
    for ix in 3..10 {
        assert!(row_deleted(d, id, "FillGradient", ix), "Row {ix} of the style is deleted here");
    }
    assert!(!row_deleted(d, id, "FillGradient", 2));
    let stops = look(&mut s, id)["gradient"]["stops"].as_array().unwrap().len();
    assert_eq!(stops, 3, "the three stops given and none of the style's");
    // Five stops bring two of the deleted rows back, and the rest stay deleted.
    run(
        &mut s,
        "shape.gradient",
        json!({"ids": [id], "stops": [
        {"position": 0, "color": "#FF0000"}, {"position": 0.25, "color": "#FFFF00"}, {"position": 0.5, "color": "#00FF00"},
        {"position": 0.75, "color": "#00FFFF"}, {"position": 1, "color": "#0000FF"}]}),
    );
    let d = doc(&s);
    assert!(!row_deleted(d, id, "FillGradient", 3) && !row_deleted(d, id, "FillGradient", 4));
    assert!(row_deleted(d, id, "FillGradient", 5));
    assert_eq!(look(&mut s, id)["gradient"]["stops"].as_array().unwrap().len(), 5);
    // The cells are the shape's, and a quick style takes the theme's gradient back.
    run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 1, "column": 1}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_quick_style_on_a_shape_with_a_style_of_its_own_gives_it_the_themes_cells_locally() {
    // A shape whose fill is linked to a style of the drawing's own (white, from the root) is not given the `Theme` style for its fill by a theme, and a Quick Style
    // still takes its fill from the theme: the cells are the call `THEMEVAL()` on the shape.
    let (mut s, id) = themed_session();
    s.edit("Test setup", |d| {
        let mut style = d.style(0).unwrap().as_ref().clone();
        style.id = 7;
        style.name = Some("Mine".into());
        style.name_u = Some("Mine".into());
        d.add_style(style).unwrap();
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|shape| shape.id == id).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        std::sync::Arc::make_mut(&mut shape.sheet).links.fill = Some(7);
        Ok(())
    })
    .unwrap();
    assert_eq!(look(&mut s, id)["fill"], json!("#ffffff"), "the style's white, whatever the theme");
    let r = run(&mut s, "shape.quick_style", json!({"ids": [id], "row": 2, "column": 2}));
    assert_eq!(r["formatted"], 1);
    let d = doc(&s);
    assert_eq!(fv(&local(d, id, "FillForegnd").unwrap()), (Some("THEMEVAL()".to_owned()), "Themed".to_owned()));
    assert!(local(d, id, "LineColor").is_none(), "the line follows the Theme style already: nothing written");
    assert_ne!(look(&mut s, id)["fill"], json!("#ffffff"), "the theme's colour now");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a hostile file ----------------------------------------------------------------------------------------------------------------

#[test]
fn a_gradient_section_of_thousands_of_rows_is_read_as_one_with_no_stops_and_the_commands_still_work() {
    // A file may hold thousands of stop rows (counts ten). The controls read a section of more than 256 rows as one with no stops, counted before any row is
    // merged, so that a selection of such shapes is not a hundred thousand merges; the gradient the shape then gets from a command is written whole.
    let (mut s, id) = session_with_rectangle();
    s.edit("Test setup", |d| {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|shape| shape.id == id).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        for ix in 0..5_000_u32 {
            let colour = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#102030"));
            sheet.set_cell(&CellAddr::in_row("FillGradient", None, RowKey::Ix(ix), "GradientStopColor"), colour).unwrap();
        }
        sheet.set_cell(&CellAddr::direct("FillGradientEnabled"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1"))).unwrap();
        Ok(())
    })
    .unwrap();
    let l = look(&mut s, id);
    let stops = l["gradient"]["stops"].as_array().unwrap().len();
    assert_eq!(stops, 2, "no stops read: the two colours of the fill, as the picture draws a gradient with none: {l}");
    // A command writes the stops it is given, and what it deletes is bounded by the ten rows.
    let r = run(&mut s, "shape.gradient", json!({"ids": [id], "stops": red_to_blue()}));
    assert_eq!(r["formatted"], 1);
    assert_eq!(check_session(&s), Ok(()));
}
