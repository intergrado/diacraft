//! The arrange commands: `shape.rotate`, `shape.flip`, `shape.order` and `shape.align`, and `shape.resize` of a turned or flipped shape,
//! through `Session::execute` as the control channel calls it. Numbers are paper inches on pages of the default scale; angles are degrees in the
//! parameters and radians in the cells, as a file holds them.
//!
//! The file shapes are built in code from the specifications' XML: a box 2 by 1 whose local pin is not at its middle (`LocPinX` is `Width*0.25`),
//! with a connection point on each side, the way a file holds one. Where a stencil shape is used its pin is at its middle.

use diacraft_doc::{CellAddr, Document, Lookup, Owner};
use diacraft_engine::draw::{connection_points, shape_corners};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num, route};
use crate::edit_foreign::{Dir, opened};
use crate::{Snapshot, bad_params_msg, blank_session, check_session, run, snapshot};

type P = (f64, f64);

/// How near two numbers of a page, in inches, are the same: a cell holds nine places.
const TOL: f64 = 1.0e-8;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// The corners of shape `id`'s box on page 1, in the order of its own frame: (0, 0), (Width, 0), (Width, Height), (0, Height).
fn corners(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}"))
}

fn assert_corners(what: &str, d: &Document, id: u32, want: [P; 4]) {
    let got = corners(d, id);
    assert!(got.iter().zip(&want).all(|(g, w)| near_p(*g, *w)), "{what}: shape {id} has corners {got:?}, want {want:?}");
}

/// The box a set of corners spans: (left, bottom, right, top).
fn bounds(c: &[P; 4]) -> (f64, f64, f64, f64) {
    let xs = c.iter().map(|p| p.0);
    let ys = c.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

fn pin(d: &Document, id: u32) -> P {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

/// The shape with this `ID`, anywhere on page 1.
fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap()
}

/// What shape `id` holds locally in a cell: its formula (if any) and its value text.
fn local(d: &Document, id: u32, cell: &str) -> Option<(Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

fn unit_of(d: &Document, id: u32, cell: &str) -> Option<String> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => c.unit.as_deref().map(str::to_owned),
        _ => None,
    }
}

/// The `ID`s of the top-level shapes of page 1, bottom first.
fn order(d: &Document) -> Vec<u32> {
    d.page(1).unwrap().shapes.iter().map(|s| s.id).collect()
}

fn degrees(d: f64) -> f64 {
    d.to_radians()
}

fn mirror_x(c: f64, p: P) -> P {
    (2.0 * c - p.0, p.1)
}

fn mirror_y(c: f64, p: P) -> P {
    (p.0, 2.0 * c - p.1)
}

fn turn_about(c: P, by: f64, p: P) -> P {
    let (s, k) = by.sin_cos();
    let (x, y) = (p.0 - c.0, p.1 - c.1);
    (c.0 + x * k - y * s, c.1 + x * s + y * k)
}

/// A command that changed nothing left the document, the undo and redo steps and the revision as they were. (It is still in the journal: a call
/// that succeeded is.)
fn assert_unchanged(s: &Session, before: &Snapshot, what: &str) {
    let now = snapshot(s);
    assert_eq!(now.document, before.document, "{what}: the document");
    assert_eq!(
        (now.undo_steps, now.redo_steps, now.revision),
        (before.undo_steps, before.redo_steps, before.revision),
        "{what}: no step, no revision"
    );
}

fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

// ---- a drawing from a file ------------------------------------------------------------------------------------------------------

/// The line of a shape, which a file holds on the shape or on its style: black, a hundredth of an inch.
const LINE: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/>"##;

/// A box 2 by 1 with no master, as a file holds it: its pin at (`x`, `y`), its local pin a quarter of the way in by formulas (so not at the
/// middle), a connection point on each side (0 right, 1 left, 2 top, 3 bottom) and `extra` cells.
fn file_box(id: u32, x: f64, y: f64, extra: &str) -> String {
    shape(
        id,
        &format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.25"/><Cell N="LocPinY" V="0.25" F="Height*0.25"/>{LINE}{extra}
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row>
<Row IX="2"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="1" F="Height"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row>
<Row IX="3"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="0"/><Cell N="DirX" V="0"/><Cell N="DirY" V="-1"/></Row></Section>"#
        ),
    )
}

/// A session that has opened a page holding `shapes` and `connects`.
fn open_page(shapes: &str, connects: &str) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(shapes).connects(connects)).build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    (s, dir)
}

/// A box turned 30 degrees, as a file holds an angle: radians to thirteen places, `U` `DEG` and the formula `30 deg`.
const THIRTY: &str = r#"<Cell N="Angle" V="0.5235987755983" U="DEG" F="30 deg"/>"#;

// ---- rotate -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_file_shape_turns_about_its_pin_through_each_quadrant() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, THIRTY), "");
    // 30 and 60 more: a quarter turn, as the cell is written in a file (thirteen places), with the unit it had and no formula.
    let r = run(&mut s, "shape.rotate", json!({"ids": [10], "by": 60}));
    assert_eq!(r, json!({"page": 1, "rotated": 1}));
    let d = doc(&s);
    assert_eq!(local(d, 10, "Angle"), Some((None, Some("1.5707963267949".to_owned()))));
    assert_eq!(unit_of(d, 10, "Angle").as_deref(), Some("DEG"), "the unit the cell had");
    assert_eq!(pin(d, 10), (4.0, 3.0), "a turn about the pin leaves the pin");
    assert_corners("90", d, 10, [(4.25, 2.5), (4.25, 4.5), (3.25, 4.5), (3.25, 2.5)]);
    let formula = |d: &Document| local(d, 10, "LocPinX").unwrap().0;
    assert_eq!(formula(d).as_deref(), Some("Width*0.25"), "the local pin keeps its formula");
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    assert_corners("180", doc(&s), 10, [(4.5, 3.25), (2.5, 3.25), (2.5, 2.25), (4.5, 2.25)]);
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    assert_corners("270", doc(&s), 10, [(3.75, 3.5), (3.75, 1.5), (4.75, 1.5), (4.75, 3.5)]);
    // Three quarters is written as a quarter the other way: the files hold angles from -pi to pi (counts 34,207 negative cells).
    assert_eq!(local(doc(&s), 10, "Angle").unwrap().1.as_deref(), Some("-1.5707963267949"));
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    assert_corners("360", doc(&s), 10, [(3.5, 2.75), (5.5, 2.75), (5.5, 3.75), (3.5, 3.75)]);
    assert_eq!(local(doc(&s), 10, "Angle").unwrap().1.as_deref(), Some("0"), "a whole turn is 0, not the noise of 2 pi");
    // An absolute angle, and a turn the other way from it: kept from just over -180 to 180 degrees.
    run(&mut s, "shape.rotate", json!({"ids": [10], "angle": 45}));
    assert!(near(num(doc(&s), 10, "Angle"), degrees(45.0)));
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": -90}));
    assert!(near(num(doc(&s), 10, "Angle"), -degrees(45.0)), "{}", num(doc(&s), 10, "Angle"));
    run(&mut s, "shape.rotate", json!({"ids": [10], "angle": 270}));
    assert!(near(num(doc(&s), 10, "Angle"), -degrees(90.0)), "270 is minus 90");
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.rotate", json!({"ids": [10], "angle": -90}))["rotated"], 0, "the angle it has, however it is given");
    assert_unchanged(&s, &before, "-90 is 270");
    for (typed, held) in [(350.0, -10.0), (180.0, 180.0), (-180.0, 180.0), (190.0, -170.0), (-200.0, 160.0), (720.0, 0.0), (179.0, 179.0)] {
        run(&mut s, "shape.rotate", json!({"ids": [10], "angle": typed}));
        let angle = num(doc(&s), 10, "Angle");
        assert!(near(angle, degrees(held)), "{typed} degrees is held as {held}, and the cell says {angle}");
        // The range is from just over -pi to pi: the cell holds thirteen places, and pi to thirteen places is 3.1415926535898, a hair over.
        assert!(angle > -std::f64::consts::PI && angle <= std::f64::consts::PI + 1.0e-12, "{angle}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_angle_a_file_holds_past_pi_is_left_until_the_shape_is_turned() {
    // A file holds 330 degrees as 5.7595865315813 and a turn of nothing leaves it; the next real turn writes it in the range.
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, r#"<Cell N="Angle" V="5.7595865315813"/>"#), "");
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.rotate", json!({"ids": [10], "by": 0}))["rotated"], 0);
    assert_eq!(run(&mut s, "shape.rotate", json!({"ids": [10], "angle": -30}))["rotated"], 0, "330 is minus 30");
    assert_unchanged(&s, &before, "330 degrees");
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 10}));
    assert!(near(num(doc(&s), 10, "Angle"), -degrees(20.0)), "{}", num(doc(&s), 10, "Angle"));
}

#[test]
fn an_angle_that_a_turn_would_snap_to_a_quarter_is_the_quarter_and_a_turn_of_nothing_writes_nothing() {
    // 89.9999999999 degrees is within a billionth of a radian of a quarter: a turn of nothing is no change and no undo step, a real one starts
    // from the quarter.
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, r#"<Cell N="Angle" V="1.5707963267931"/>"#), "");
    let before = snapshot(&s);
    for params in [json!({"ids": [10], "by": 0}), json!({"ids": [10], "angle": 90}), json!({"ids": [10], "by": 360})] {
        assert_eq!(run(&mut s, "shape.rotate", params.clone())["rotated"], 0, "{params}");
        assert_unchanged(&s, &before, &params.to_string());
    }
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    assert_eq!(local(doc(&s), 10, "Angle").unwrap().1.as_deref(), Some("3.1415926535898"), "a half turn exactly");
}

#[test]
fn an_angle_a_file_holds_as_the_formula_the_cell_had_is_written_back_as_a_number_and_reads_the_same() {
    // The cell held `30 deg`; after a turn to the same angle nothing is written (no change, no undo step).
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, THIRTY), "");
    let before = snapshot(&s);
    let r = run(&mut s, "shape.rotate", json!({"ids": [10], "angle": 30}));
    assert_eq!(r["rotated"], 0);
    assert_unchanged(&s, &before, "the angle is the one it had");
    assert_eq!(local(doc(&s), 10, "Angle").unwrap().0.as_deref(), Some("30 deg"), "the formula stays when nothing is written");
}

#[test]
fn several_shapes_turn_by_an_angle_as_one_about_the_middle_of_their_joint_box() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let before = doc(&s).clone();
    // The joint box is 1.25 to 6.75 by 1.5 to 4.5, its middle (4, 3).
    run(&mut s, "shape.rotate", json!({"ids": [a, b], "by": 90}));
    let d = doc(&s);
    assert!(near_p(pin(d, a), (5.0, 1.0)) && near_p(pin(d, b), (3.0, 5.0)), "{:?} {:?}", pin(d, a), pin(d, b));
    for id in [a, b] {
        assert!(near(num(d, id, "Angle"), degrees(90.0)));
        let was = corners(&before, id);
        let now = corners(d, id);
        for (w, n) in was.iter().zip(&now) {
            assert!(near_p(turn_about((4.0, 3.0), degrees(90.0), *w), *n), "shape {id}: {w:?} to {n:?}");
        }
    }
    // Four quarter turns are the page back.
    for _ in 0..3 {
        run(&mut s, "shape.rotate", json!({"ids": [a, b], "by": 90}));
    }
    let d = doc(&s);
    assert!(near_p(pin(d, a), (2.0, 2.0)) && near_p(pin(d, b), (6.0, 4.0)));
    assert!(near(num(d, a, "Angle"), 0.0) || near(num(d, a, "Angle"), degrees(360.0)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_absolute_angle_turns_each_shape_about_its_own_pin() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "hexagon", 6.0, 4.0);
    run(&mut s, "shape.rotate", json!({"ids": [a, b], "angle": 90}));
    let d = doc(&s);
    assert_eq!((pin(d, a), pin(d, b)), ((2.0, 2.0), (6.0, 4.0)));
    assert!(near(num(d, a, "Angle"), degrees(90.0)) && near(num(d, b, "Angle"), degrees(90.0)));
}

#[test]
fn a_connector_turns_its_free_ends_about_its_pin_and_a_glued_end_stays() {
    let mut s = blank_session();
    let free = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 3.0}})));
    run(&mut s, "shape.rotate", json!({"ids": [free], "by": 90}));
    let d = doc(&s);
    let ends = |d: &Document| (num(d, free, "BeginX"), num(d, free, "BeginY"), num(d, free, "EndX"), num(d, free, "EndY"));
    let e = ends(d);
    assert!(near(e.0, 4.0) && near(e.1, 1.0) && near(e.2, 4.0) && near(e.3, 5.0), "{e:?}");
    assert!(near(num(d, free, "Angle"), degrees(90.0)), "the angle follows the ends by its formula");
    assert!(near(num(d, free, "Width"), 4.0));
    assert!(route(d, free).iter().all(|p| near(p.0, 4.0)), "{:?}", route(d, free));
    // An absolute angle sets the way the line points, from the begin to the end.
    run(&mut s, "shape.rotate", json!({"ids": [free], "angle": 0}));
    let e = ends(doc(&s));
    assert!(near(e.0, 2.0) && near(e.1, 3.0) && near(e.2, 6.0) && near(e.3, 3.0), "{e:?}");
    // Glued at the begin, free at the end: the begin stays at its shape, the end swings about the glued end (ROTATE-1D-1).
    let a = drop_shape(&mut s, "rectangle", 2.0, 8.0);
    let half = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": 6.0, "y": 8.0}})));
    let begin = (num(doc(&s), half, "BeginX"), num(doc(&s), half, "BeginY"));
    run(&mut s, "shape.rotate", json!({"ids": [half], "by": 90}));
    let d = doc(&s);
    assert!(near_p((num(d, half, "BeginX"), num(d, half, "BeginY")), begin), "the glued end stays");
    assert!(d.page(1).unwrap().connects.iter().any(|c| c.from_sheet == half && c.to_sheet == a), "and stays glued");
    assert!(!near_p((num(d, half, "EndX"), num(d, half, "EndY")), (6.0, 8.0)), "the free end moved");
}

/// The way connector `c` points on the page, in radians, from its begin to its end.
fn direction(d: &Document, c: u32) -> f64 {
    (num(d, c, "EndY") - num(d, c, "BeginY")).atan2(num(d, c, "EndX") - num(d, c, "BeginX"))
}

fn length_of(d: &Document, c: u32) -> f64 {
    (num(d, c, "EndY") - num(d, c, "BeginY")).hypot(num(d, c, "EndX") - num(d, c, "BeginX"))
}

/// Whether two directions are the same way, whatever whole turns they differ by.
fn same_way(a: f64, b: f64) -> bool {
    (a - b).sin().abs() < 1.0e-8 && (a - b).cos() > 0.0
}

fn glued_to(d: &Document, c: u32) -> Vec<(String, u32)> {
    d.page(1).unwrap().connects.iter().filter(|k| k.from_sheet == c).map(|k| (k.from_cell.clone().unwrap().to_string(), k.to_sheet)).collect()
}

#[test]
fn an_angle_is_reached_exactly_by_a_connector_glued_at_its_begin_and_a_repeat_changes_nothing() {
    // The review's case: the begin glued at (2, 2.5), the end free at (6, 5). The free end turns about the glued end, whatever the angle and
    // however long the line: the angle was 61 degrees and then 75.5 when the end swung about the pin.
    for (end, length) in [((6.0, 5.0), 4.717), ((60.0, 45.0), 71.904)] {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 1.25, 2.5);
        let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": end.0, "y": end.1}})));
        let (begin, was) = ((num(doc(&s), c, "BeginX"), num(doc(&s), c, "BeginY")), length_of(doc(&s), c));
        assert!(near_p(begin, (2.0, 2.5)), "{begin:?}");
        assert!((was - length).abs() < 0.01, "{was}");
        let glue = glued_to(doc(&s), c);
        for angle in [90.0, 0.0, -135.0, 61.0, 180.0, 45.0, 90.0] {
            run(&mut s, "shape.rotate", json!({"ids": [c], "angle": angle}));
            let d = doc(&s);
            assert!(same_way(direction(d, c), degrees(angle)), "{angle} degrees gives {} radians", direction(d, c));
            assert!(near(num(d, c, "Angle"), direction(d, c)), "the cell follows by its formula");
            assert!((length_of(d, c) - was).abs() < 1.0e-7, "the line keeps its length");
            assert!(near_p((num(d, c, "BeginX"), num(d, c, "BeginY")), begin), "the glued end stays");
            assert_eq!(glued_to(d, c), glue, "and stays glued");
            // The same again: nothing to turn, no step.
            let before = snapshot(&s);
            let r = run(&mut s, "shape.rotate", json!({"ids": [c], "angle": angle}));
            assert_eq!(r["rotated"], 0, "{angle} twice");
            assert_unchanged(&s, &before, &format!("{angle} twice"));
            assert!(same_way(direction(doc(&s), c), degrees(angle)));
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn an_angle_is_reached_exactly_by_a_connector_glued_at_its_end_and_a_turn_by_turns_the_line_by_it() {
    let mut s = blank_session();
    let b = drop_shape(&mut s, "rectangle", 8.0, 5.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 3.0, "y": 1.0}, "to": {"id": b, "point": 1}})));
    let (end, was) = ((num(doc(&s), c, "EndX"), num(doc(&s), c, "EndY")), length_of(doc(&s), c));
    assert!(near_p(end, (7.25, 5.0)), "{end:?}");
    for angle in [45.0, 200.0, -90.0, 0.0, 33.0] {
        run(&mut s, "shape.rotate", json!({"ids": [c], "angle": angle}));
        let d = doc(&s);
        assert!(same_way(direction(d, c), degrees(angle)), "{angle} degrees gives {} radians", direction(d, c));
        assert!((length_of(d, c) - was).abs() < 1.0e-7);
        assert!(near_p((num(d, c, "EndX"), num(d, c, "EndY")), end), "the glued end stays");
        assert!(glued_to(d, c).iter().any(|(cell, to)| cell == "EndX" && *to == b), "and stays glued");
        let before = snapshot(&s);
        assert_eq!(run(&mut s, "shape.rotate", json!({"ids": [c], "angle": angle}))["rotated"], 0, "{angle} twice");
        assert_unchanged(&s, &before, &format!("{angle} twice"));
    }
    // By an angle: the line turns by it about the glued end (the pin of a line with one end held is not where it turns).
    for by in [30.0, -75.0, 200.0] {
        let was = direction(doc(&s), c);
        run(&mut s, "shape.rotate", json!({"ids": [c], "by": by}));
        assert!(same_way(direction(doc(&s), c), was + degrees(by)), "by {by}");
        assert!(near_p((num(doc(&s), c, "EndX"), num(doc(&s), c, "EndY")), end));
    }
    // The route of ours is made again from the ends, whatever the turn.
    let r = route(doc(&s), c);
    assert!(near_p(r[0], (num(doc(&s), c, "BeginX"), num(doc(&s), c, "BeginY"))));
    assert!(near_p(r[r.len() - 1], end));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_in_a_set_turned_by_an_angle_still_turns_about_the_joint_box() {
    // Turned with the shape it is glued to, as the page turns: the glued end follows its shape, the free end is carried about the same middle.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": 6.0, "y": 2.0}})));
    let (box_a, end) = (bounds(&corners(doc(&s), a)), (num(doc(&s), c, "EndX"), num(doc(&s), c, "EndY")));
    // The joint box is the shape's box and the connector's ends: 1.25 to 6 across, 1.5 to 2.5 up.
    let middle = ((box_a.0 + 6.0) / 2.0, (box_a.1 + box_a.3) / 2.0);
    run(&mut s, "shape.rotate", json!({"ids": [a, c], "by": 90}));
    let d = doc(&s);
    let want = turn_about(middle, degrees(90.0), end);
    assert!(near_p((num(d, c, "EndX"), num(d, c, "EndY")), want), "{:?} for {want:?}", (num(d, c, "EndX"), num(d, c, "EndY")));
    assert_ends_at_their_points(d, "the glued end");
}

#[test]
fn a_connector_with_both_ends_glued_has_nothing_to_turn_and_is_no_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    for (command, params) in [
        ("shape.rotate", json!({"ids": [c], "by": 90})),
        ("shape.rotate", json!({"ids": [c], "angle": 12})),
        ("shape.flip", json!({"ids": [c], "axis": "horizontal"})),
    ] {
        let before = snapshot(&s);
        let r = run(&mut s, command, params.clone());
        assert!(r.as_object().unwrap().values().any(|v| v == &json!(0)), "{command} {params}: {r}");
        assert_unchanged(&s, &before, &format!("{command} {params}"));
    }
}

#[test]
fn a_turn_of_nothing_changes_nothing_and_is_no_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "circle", 6.0, 4.0);
    let before = snapshot(&s);
    for params in [json!({"ids": [a, b], "by": 0}), json!({"ids": [a, b], "by": 360}), json!({"ids": [a], "angle": 0})] {
        let r = run(&mut s, "shape.rotate", params.clone());
        assert_eq!(r["rotated"], 0, "{params}");
        assert_unchanged(&s, &before, &params.to_string());
    }
}

#[test]
fn whole_turns_are_left_out_of_the_arithmetic_so_a_huge_angle_is_as_exact_as_a_small_one() {
    // 1,000,000 degrees is 2,777 whole turns and 280 degrees.
    let make = || {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
        let b = drop_shape(&mut s, "hexagon", 6.0, 4.0);
        (s, a, b)
    };
    let (mut huge, a, b) = make();
    let (mut small, _, _) = make();
    run(&mut huge, "shape.rotate", json!({"ids": [a, b], "by": 1_000_000}));
    run(&mut small, "shape.rotate", json!({"ids": [a, b], "by": 280}));
    for id in [a, b] {
        assert_corners("a huge turn", doc(&huge), id, corners(doc(&small), id));
        assert!(near(num(doc(&huge), id, "Angle"), num(doc(&small), id, "Angle")));
    }
}

// ---- glue after a turn or a flip --------------------------------------------------------------------------------------------------

/// Every glued end of every connector on page 1 is at the connection point it is glued to, on the page.
fn assert_ends_at_their_points(d: &Document, what: &str) {
    for c in d.page(1).unwrap().connects.iter() {
        let (x, y) = match c.from_cell.as_deref() {
            Some("BeginX") => ("BeginX", "BeginY"),
            Some("EndX") => ("EndX", "EndY"),
            _ => continue,
        };
        // A glue to the whole of a shape (`ToPart` 3) has no point to be at.
        let Some(index) = c.to_part.and_then(|part| part.checked_sub(100)) else { continue };
        let point = connection_points(d, 1, c.to_sheet).into_iter().find(|p| p.index == index).unwrap();
        let end = (num(d, c.from_sheet, x), num(d, c.from_sheet, y));
        assert!(
            near_p(end, (point.x, point.y)),
            "{what}: connector {} {x} is at {end:?}, point {index} of {} is at ({}, {})",
            c.from_sheet,
            c.to_sheet,
            point.x,
            point.y
        );
    }
}

#[test]
fn a_glued_connector_follows_a_target_that_is_turned_or_flipped() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    assert_ends_at_their_points(doc(&s), "before");
    for (command, params) in [
        ("shape.rotate", json!({"ids": [b], "by": 90})),
        ("shape.rotate", json!({"ids": [b], "by": 30})),
        ("shape.flip", json!({"ids": [b], "axis": "horizontal"})),
        ("shape.flip", json!({"ids": [a], "axis": "vertical"})),
        ("shape.rotate", json!({"ids": [a], "angle": 200})),
        ("shape.rotate", json!({"ids": [a, b], "by": -45})),
        ("shape.flip", json!({"ids": [a, b], "axis": "vertical"})),
        ("shape.flip", json!({"ids": [a, b], "axis": "horizontal"})),
    ] {
        run(&mut s, command, params.clone());
        let d = doc(&s);
        assert_ends_at_their_points(d, &format!("{command} {params}"));
        // The route runs from one end to the other, and the glue is as it was.
        let r = route(d, c);
        assert!(near_p(r[0], (num(d, c, "BeginX"), num(d, c, "BeginY"))), "{command} {params}: {r:?}");
        assert!(near_p(r[r.len() - 1], (num(d, c, "EndX"), num(d, c, "EndY"))), "{command} {params}: {r:?}");
        assert_eq!(d.page(1).unwrap().connects.len(), 2);
        assert_eq!(check_session(&s), Ok(()));
    }
}

// ---- a file's connectors: the point of a turned target, and the whole of one -------------------------------------------------------

#[test]
fn the_connectors_of_a_file_follow_a_target_that_is_turned_or_flipped_whichever_way_they_are_glued() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    // Shapes 10 and 11 are boxes with points, 12 a box with points, 13 a blob without any. Connector 20 is glued to points (`PAR`), 21 to the
    // whole of 12 and 13 (`_WALKGLUE`), 23 to a point of 11 and the whole of 13.
    let near_edge = |d: &Document, id: u32, p: P| {
        let c = corners(d, id);
        (0..4).map(|i| distance_to_segment(c[i], c[(i + 1) % 4], p)).fold(f64::INFINITY, f64::min)
    };
    for (command, params) in [
        ("shape.rotate", json!({"ids": [13], "by": 90})),
        ("shape.rotate", json!({"ids": [13], "by": 20})),
        ("shape.flip", json!({"ids": [13], "axis": "horizontal"})),
        ("shape.rotate", json!({"ids": [12], "angle": 135})),
        ("shape.flip", json!({"ids": [12], "axis": "vertical"})),
        ("shape.rotate", json!({"ids": [11], "by": 90})),
        ("shape.flip", json!({"ids": [11, 13], "axis": "horizontal"})),
        ("shape.rotate", json!({"ids": [10, 11, 12, 13], "by": 33})),
    ] {
        run(&mut s, command, params.clone());
        let d = doc(&s);
        let what = format!("{command} {params}");
        // 20 and 22 are glued to points of their shapes: the end cells are those points on the page.
        assert_ends_at_their_points(d, &what);
        // 21 is glued to the whole of 12 (which has points: the end is at one) and of 13 (no points: the end is on its outline).
        let points: Vec<P> = connection_points(d, 1, 12).iter().map(|p| (p.x, p.y)).collect();
        let begin = (num(d, 21, "BeginX"), num(d, 21, "BeginY"));
        assert!(points.iter().any(|p| near_p(*p, begin)), "{what}: 21 begins at {begin:?}, not at a point of 12 {points:?}");
        let end = (num(d, 21, "EndX"), num(d, 21, "EndY"));
        assert!(near_edge(d, 13, end) < 1e-6, "{what}: 21 ends at {end:?}, {} from the outline of 13", near_edge(d, 13, end));
        let end = (num(d, 23, "EndX"), num(d, 23, "EndY"));
        assert!(near_edge(d, 13, end) < 1e-6, "{what}: 23 ends at {end:?}, off the outline of 13");
        // Each route runs from the begin to the end of its connector.
        for id in [20, 21, 22, 23] {
            let r = route(d, id);
            assert!(near_p(r[0], (num(d, id, "BeginX"), num(d, id, "BeginY"))), "{what}: {id} {r:?}");
            assert!(near_p(r[r.len() - 1], (num(d, id, "EndX"), num(d, id, "EndY"))), "{what}: {id} {r:?}");
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

fn distance_to_segment(a: P, b: P, p: P) -> f64 {
    let (ab, ap) = ((b.0 - a.0, b.1 - a.1), (p.0 - a.0, p.1 - a.1));
    let length = ab.0 * ab.0 + ab.1 * ab.1;
    let t = if length > 0.0 { ((ap.0 * ab.0 + ap.1 * ab.1) / length).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - (a.0 + ab.0 * t)).hypot(p.1 - (a.1 + ab.1 * t))
}

#[test]
fn turning_a_shape_changes_only_it_and_the_connectors_glued_to_it() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = doc(&s).clone();
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    let d = doc(&s);
    // Shape 10 is glued to by connectors 20 and 22; 11, 12, 13 and connectors 21 and 23 are the file's, byte for byte in the model.
    for id in [11, 12, 13, 21, 23] {
        assert_eq!(shape_of(d, id), shape_of(&before, id), "shape {id} was not touched");
    }
    assert_ne!(shape_of(d, 20), shape_of(&before, 20));
    assert_ne!(shape_of(d, 22), shape_of(&before, 22));
}

// ---- flip -------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_flip_mirrors_a_shape_in_place_and_flipping_again_gives_it_back() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, ""), "");
    let was = doc(&s).clone();
    // The box is 3.5 to 5.5 by 2.75 to 3.75, its middle (4.5, 3.25), and the pin is not at the middle: the pin is mirrored with the box.
    let r = run(&mut s, "shape.flip", json!({"ids": [10], "axis": "horizontal"}));
    assert_eq!(r, json!({"page": 1, "flipped": 1}));
    let d = doc(&s);
    assert_eq!(num(d, 10, "FlipX"), 1.0);
    assert!(near_p(pin(d, 10), (5.0, 3.0)), "{:?}", pin(d, 10));
    assert_corners("flipped", d, 10, [(5.5, 2.75), (3.5, 2.75), (3.5, 3.75), (5.5, 3.75)]);
    assert_eq!(local(d, 10, "LocPinX").unwrap().0.as_deref(), Some("Width*0.25"), "the local pin is the shape's own and stays");
    assert_eq!(bounds(&corners(d, 10)), bounds(&corners(&was, 10)), "the box is where it was");
    run(&mut s, "shape.flip", json!({"ids": [10], "axis": "horizontal"}));
    let d = doc(&s);
    assert_eq!(num(d, 10, "FlipX"), 0.0);
    assert_corners("flipped back", d, 10, corners(&was, 10));
    // The other way.
    run(&mut s, "shape.flip", json!({"ids": [10], "axis": "vertical"}));
    let d = doc(&s);
    assert_eq!((num(d, 10, "FlipX"), num(d, 10, "FlipY")), (0.0, 1.0));
    assert!(near_p(pin(d, 10), (4.0, 3.5)), "{:?}", pin(d, 10));
    assert_corners("vertical", d, 10, [(3.5, 3.75), (5.5, 3.75), (5.5, 2.75), (3.5, 2.75)]);
}

#[test]
fn a_flip_is_a_mirror_of_the_page_so_a_turned_shape_turns_the_other_way() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, THIRTY), "");
    let was = corners(doc(&s), 10);
    let c = bounds(&was);
    run(&mut s, "shape.flip", json!({"ids": [10], "axis": "horizontal"}));
    let d = doc(&s);
    assert_eq!(num(d, 10, "FlipX"), 1.0);
    assert!(near(num(d, 10, "Angle"), -degrees(30.0)), "the mirror turns it the other way: {}", num(d, 10, "Angle"));
    let now = corners(d, 10);
    for (w, n) in was.iter().zip(&now) {
        assert!(near_p(mirror_x((c.0 + c.2) / 2.0, *w), *n), "{w:?} to {n:?}");
    }
    let (b0, b1) = (bounds(&now), bounds(&was));
    assert!(near(b0.0, b1.0) && near(b0.1, b1.1) && near(b0.2, b1.2) && near(b0.3, b1.3), "the picture is mirrored in its own box: {b0:?} {b1:?}");
    // And vertically, with both flips set the shape is its half turn.
    run(&mut s, "shape.flip", json!({"ids": [10], "axis": "vertical"}));
    let d = doc(&s);
    assert_eq!((num(d, 10, "FlipX"), num(d, 10, "FlipY")), (1.0, 1.0));
    let both = corners(d, 10);
    let mid = ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0);
    for (w, n) in was.iter().zip(&both) {
        assert!(near_p(turn_about(mid, degrees(180.0), *w), *n), "{w:?} to {n:?}");
    }
}

#[test]
fn several_shapes_flip_as_one_about_their_joint_box() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "hexagon", 6.0, 4.0);
    let (ca, cb) = (corners(doc(&s), a), corners(doc(&s), b));
    // The joint box is 1.25 to 6.7 by 1.5 to 4.6: the middle of it is (3.975, 3.05).
    let (x0, x1) = (1.25_f64, 6.7_f64);
    run(&mut s, "shape.flip", json!({"ids": [a, b], "axis": "horizontal"}));
    let d = doc(&s);
    for (id, was) in [(a, ca), (b, cb)] {
        for (w, n) in was.iter().zip(&corners(d, id)) {
            assert!(near_p(mirror_x((x0 + x1) / 2.0, *w), *n), "shape {id}: {w:?} to {n:?}");
        }
        assert_eq!(num(d, id, "FlipX"), 1.0);
    }
    let (y0, y1) = (1.5_f64, 4.6_f64);
    run(&mut s, "shape.flip", json!({"ids": [a, b], "axis": "vertical"}));
    let d = doc(&s);
    for (id, was) in [(a, ca), (b, cb)] {
        for (w, n) in was.iter().zip(&corners(d, id)) {
            assert!(near_p(mirror_y((y0 + y1) / 2.0, mirror_x((x0 + x1) / 2.0, *w)), *n), "shape {id}: {w:?} to {n:?}");
        }
    }
}

#[test]
fn a_flipped_connector_mirrors_its_free_ends() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 5.0}})));
    run(&mut s, "shape.flip", json!({"ids": [c], "axis": "horizontal"}));
    let d = doc(&s);
    assert!(near(num(d, c, "BeginX"), 6.0) && near(num(d, c, "EndX"), 2.0), "{} {}", num(d, c, "BeginX"), num(d, c, "EndX"));
    assert!(near(num(d, c, "BeginY"), 3.0) && near(num(d, c, "EndY"), 5.0));
    run(&mut s, "shape.flip", json!({"ids": [c], "axis": "vertical"}));
    let d = doc(&s);
    assert!(near(num(d, c, "BeginY"), 5.0) && near(num(d, c, "EndY"), 3.0));
}

const PX_PER_INCH: f64 = 100.0;

fn render(s: &Session) -> Rendered {
    let d = s.document().unwrap();
    Renderer::new().render(d, 1, &View::fit_page(d, 1, PX_PER_INCH).unwrap())
}

/// The ink of each pixel column of a rectangle of the page (left, bottom to right, top, inches): how many dark pixels the column holds.
fn ink_columns(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> Vec<usize> {
    let (width, height) = (r.width as usize, r.height as usize);
    let page_height = height as f64 / PX_PER_INCH;
    let (c0, c1) = ((left * PX_PER_INCH) as usize, ((right * PX_PER_INCH) as usize).min(width));
    let (r0, r1) = (((page_height - top) * PX_PER_INCH) as usize, (((page_height - bottom) * PX_PER_INCH) as usize).min(height));
    (c0..c1)
        .map(|col| {
            (r0..r1)
                .filter(|row| {
                    let at = (row * width + col) * 4;
                    r.rgba_premul.get(at + 3).copied().unwrap_or(0) > 128
                        && r.rgba_premul[at..at + 3].iter().map(|v| u32::from(*v)).sum::<u32>() < 600
                })
                .count()
        })
        .collect()
}

#[test]
fn the_text_of_a_flipped_shape_is_where_it_was_and_not_mirrored() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Hello"}));
    let plain = render(&s);
    let columns = |r: &Rendered| ink_columns(r, 2.4, 3.65, 3.6, 4.35);
    let before = columns(&plain);
    assert!(before.iter().sum::<usize>() > 30, "the word is inked");
    for axis in ["horizontal", "vertical", "horizontal", "vertical"] {
        run(&mut s, "shape.flip", json!({"ids": [a], "axis": axis}));
        let after = columns(&render(&s));
        let off: usize = before.iter().zip(&after).map(|(b, a)| b.abs_diff(*a)).sum();
        assert!(
            off * 10 < before.iter().sum::<usize>(),
            "after a {axis} flip the word is drawn differently: {off} pixels off in {before:?} against {after:?}"
        );
        // Text is never mirrored and the cells that turn it are not touched.
        assert!(local(doc(&s), a, "TxtAngle").is_none());
    }
    // Turned a quarter, the word runs up the box: the ink is taller than wide (the window is inside the outline of the box, which is not text).
    let (wide, tall) = ink_extent(&plain, 2.4, 3.65, 3.6, 4.35);
    assert!(wide > 2 * tall, "the word runs along the box: {wide} pixels wide and {tall} tall");
    run(&mut s, "shape.rotate", json!({"ids": [a], "by": 90}));
    let (wide, tall) = ink_extent(&render(&s), 2.6, 3.35, 3.4, 4.65);
    assert!(tall > 2 * wide, "the word is on its side: {wide} pixels wide and {tall} tall");
}

/// How many pixel columns and rows of a rectangle of the page (inches) hold ink.
fn ink_extent(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> (usize, usize) {
    let columns = ink_columns(r, left, bottom, right, top).iter().filter(|n| **n > 0).count();
    // The rows are the columns of the page seen from the side: count them straight.
    let (width, height) = (r.width as usize, r.height as usize);
    let page_height = height as f64 / PX_PER_INCH;
    let (c0, c1) = ((left * PX_PER_INCH) as usize, ((right * PX_PER_INCH) as usize).min(width));
    let (r0, r1) = (((page_height - top) * PX_PER_INCH) as usize, (((page_height - bottom) * PX_PER_INCH) as usize).min(height));
    let rows = (r0..r1)
        .filter(|row| {
            (c0..c1).any(|col| {
                let at = (row * width + col) * 4;
                r.rgba_premul.get(at + 3).copied().unwrap_or(0) > 128 && r.rgba_premul[at..at + 3].iter().map(|v| u32::from(*v)).sum::<u32>() < 600
            })
        })
        .count();
    (columns, rows)
}

// ---- resize of a turned or flipped shape --------------------------------------------------------------------------------------------

#[test]
fn a_turned_shape_is_resized_along_its_own_axes_and_its_corner_stays_where_it_is_asked() {
    // (x, y) is where the corner at (0, 0) of the shape's own frame lands, width and height are along the shape's axes.
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, r#"<Cell N="Angle" V="1.5707963267949" U="DEG" F="90 deg"/>"#), "");
    run(&mut s, "shape.resize", json!({"id": 10, "x": 3.0, "y": 2.0, "width": 3.0, "height": 1.5}));
    let d = doc(&s);
    assert!(near(num(d, 10, "Width"), 3.0) && near(num(d, 10, "Height"), 1.5));
    assert!(near(num(d, 10, "LocPinX"), 0.75) && near(num(d, 10, "LocPinY"), 0.375), "the local pin follows its formulas");
    assert!(near_p(pin(d, 10), (2.625, 2.75)), "{:?}", pin(d, 10));
    assert_corners("resized", d, 10, [(3.0, 2.0), (3.0, 5.0), (1.5, 5.0), (1.5, 2.0)]);
    assert!(near(num(d, 10, "Angle"), degrees(90.0)), "the turn is the shape's");
    // The corner a window drags from is read with `shape_corners`: the same call with the corner it was gives the same shape.
    let c = corners(d, 10)[0];
    run(&mut s, "shape.resize", json!({"id": 10, "x": c.0, "y": c.1, "width": 3.0, "height": 1.5}));
    assert_corners("again", doc(&s), 10, [(3.0, 2.0), (3.0, 5.0), (1.5, 5.0), (1.5, 2.0)]);
}

#[test]
fn a_flipped_and_a_turned_and_flipped_shape_are_resized_the_same_way() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, r#"<Cell N="FlipX" V="1"/>"#), "");
    // Flipped in x: the local x axis points left. The corner at (0, 0) lands at (6, 2); the box 3 by 1.5 reaches left and up from it.
    run(&mut s, "shape.resize", json!({"id": 10, "x": 6.0, "y": 2.0, "width": 3.0, "height": 1.5}));
    assert_corners("flipped", doc(&s), 10, [(6.0, 2.0), (3.0, 2.0), (3.0, 3.5), (6.0, 3.5)]);
    assert!(near_p(pin(doc(&s), 10), (6.0 - 0.75, 2.0 + 0.375)), "{:?}", pin(doc(&s), 10));
    let (mut t, _dir2) = open_page(&file_box(10, 4.0, 3.0, &format!(r#"<Cell N="FlipY" V="1"/>{THIRTY}"#)), "");
    let (x, y) = (2.5, 3.5);
    run(&mut t, "shape.resize", json!({"id": 10, "x": x, "y": y, "width": 2.5, "height": 0.8}));
    let d = doc(&t);
    let a = degrees(30.0);
    // The local x axis is the direction of the turn, the local y axis the mirror of the turned y axis.
    let (ex, ey) = ((a.cos(), a.sin()), (a.sin(), -a.cos()));
    let at = |u: f64, v: f64| (x + ex.0 * u + ey.0 * v, y + ex.1 * u + ey.1 * v);
    assert_corners("turned and flipped", d, 10, [at(0.0, 0.0), at(2.5, 0.0), at(2.5, 0.8), at(0.0, 0.8)]);
    assert_eq!(num(d, 10, "FlipY"), 1.0);
}

#[test]
fn a_stencil_shape_that_is_turned_is_resized_and_its_connector_goes_with_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}}));
    run(&mut s, "shape.rotate", json!({"ids": [b], "by": 90}));
    let o = corners(doc(&s), b)[0];
    run(&mut s, "shape.resize", json!({"id": b, "x": o.0, "y": o.1, "width": 3.0, "height": 2.0}));
    let d = doc(&s);
    assert!(near(num(d, b, "Width"), 3.0) && near(num(d, b, "Height"), 2.0));
    assert!(near_p(corners(d, b)[0], o), "the corner stays");
    assert_ends_at_their_points(d, "after the resize");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_of_a_turned_group_is_still_not_resized_turned_or_flipped() {
    let member = shape(
        11,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/>"#,
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Cell N="Angle" V="0.5"/><Shapes>{member}</Shapes>"#
        ),
    );
    let (mut s, _dir) = open_page(&group, "");
    let before = snapshot(&s);
    for (command, params) in [
        ("shape.resize", json!({"id": 11, "x": 4.5, "y": 4.5, "width": 2.0, "height": 1.0})),
        ("shape.rotate", json!({"ids": [11], "by": 10})),
        ("shape.flip", json!({"ids": [11], "axis": "vertical"})),
    ] {
        let e = error_of(&mut s, command, &params);
        assert!(matches!(e, EngineError::Failed { .. }), "{command}: {e:?}");
        assert_eq!(snapshot(&s), before, "{command}");
    }
    // The group itself turns, and takes its members with it.
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 10}));
    assert!(near(num(doc(&s), 10, "Angle"), 0.5 + degrees(10.0)));
}

// ---- drawing order ----------------------------------------------------------------------------------------------------------------

/// Five stencil shapes, 1 to 5, bottom first.
fn five() -> Session {
    let mut s = blank_session();
    for (i, master) in ["rectangle", "circle", "hexagon", "triangle", "ellipse"].into_iter().enumerate() {
        drop_shape(&mut s, master, 1.5 + 2.0 * i as f64, 2.0);
    }
    assert_eq!(order(doc(&s)), [1, 2, 3, 4, 5]);
    s
}

fn put(s: &mut Session, ids: &[u32], to: &str) -> Json {
    run(s, "shape.order", json!({"ids": ids, "to": to}))
}

#[test]
fn bring_to_front_and_send_to_back_move_to_the_ends_of_the_page_part() {
    let mut s = five();
    assert_eq!(put(&mut s, &[2], "front"), json!({"page": 1, "moved": 1}), "the shapes named that changed place");
    assert_eq!(order(doc(&s)), [1, 3, 4, 5, 2]);
    put(&mut s, &[4], "back");
    assert_eq!(order(doc(&s)), [4, 1, 3, 5, 2]);
    // Several keep the order they have among themselves, whatever order they are named in.
    put(&mut s, &[5, 1], "front");
    assert_eq!(order(doc(&s)), [4, 3, 2, 1, 5]);
    put(&mut s, &[2, 3], "back");
    assert_eq!(order(doc(&s)), [3, 2, 4, 1, 5]);
    // Already there: nothing moves, no undo step.
    let before = snapshot(&s);
    assert_eq!(put(&mut s, &[5], "front"), json!({"page": 1, "moved": 0}));
    assert_eq!(put(&mut s, &[3, 2], "back"), json!({"page": 1, "moved": 0}));
    assert_unchanged(&s, &before, "");
}

#[test]
fn bring_forward_and_send_backward_move_one_place_and_a_block_at_the_end_stays() {
    let mut s = five();
    put(&mut s, &[2], "forward");
    assert_eq!(order(doc(&s)), [1, 3, 2, 4, 5]);
    put(&mut s, &[2], "backward");
    assert_eq!(order(doc(&s)), [1, 2, 3, 4, 5]);
    // Two together move past one shape each.
    put(&mut s, &[2, 3], "forward");
    assert_eq!(order(doc(&s)), [1, 4, 2, 3, 5]);
    put(&mut s, &[2, 3], "backward");
    assert_eq!(order(doc(&s)), [1, 2, 3, 4, 5]);
    // Apart: each past its neighbour. At the end: stays.
    put(&mut s, &[1, 3, 5], "forward");
    assert_eq!(order(doc(&s)), [2, 1, 4, 3, 5]);
    let before = snapshot(&s);
    assert_eq!(put(&mut s, &[3, 5], "forward"), json!({"page": 1, "moved": 0}));
    assert_eq!(put(&mut s, &[2], "backward"), json!({"page": 1, "moved": 0}));
    assert_unchanged(&s, &before, "no undo step for an order that does not change");
}

#[test]
fn an_order_is_one_undo_step_and_a_save_keeps_it() {
    let mut s = five();
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 5}}));
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    put(&mut s, &[1, 2], "front");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(order(doc(&s)), [3, 4, 5, 6, 1, 2]);
    let after = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    // The writer keeps the order of the page part, and a drawing read from a file keeps it too.
    let dir = Dir::new();
    let path = dir.path("order.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(order(doc(&t)), [3, 4, 5, 6, 1, 2]);
    // Reorder the file as read and save it over: the order is the new one.
    put(&mut t, &[6], "back");
    run(&mut t, "file.save", json!({}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": path}));
    assert_eq!(order(doc(&u)), [6, 3, 4, 5, 1, 2]);
    // The shapes themselves are where they were.
    assert_corners("read back", doc(&u), 3, corners(doc(&s), 3));
}

/// Shapes 1, 2 and 3 with these `DisplayLevel`s, in that order.
fn banded(levels: [&str; 3]) -> (Session, Dir) {
    let shapes: String = levels
        .iter()
        .enumerate()
        .map(|(i, level)| file_box(1 + i as u32, 2.0 + i as f64, 3.0, &format!(r#"<Cell N="DisplayLevel" V="{level}"/>"#)))
        .collect();
    open_page(&shapes, "")
}

#[test]
fn a_shape_is_moved_within_its_band_and_never_past_a_higher_one() {
    // Bands 0, 0 and 25000; Bring to Front on the first puts it just before the third.
    let (mut s, _dir) = banded(["0", "0", "25000"]);
    put(&mut s, &[1], "front");
    assert_eq!(order(doc(&s)), [2, 1, 3]);
    assert_eq!(put(&mut s, &[3], "front"), json!({"page": 1, "moved": 0}));
    assert_eq!(put(&mut s, &[3], "back"), json!({"page": 1, "moved": 0}), "the third is at the back of its own band");
    assert_eq!(put(&mut s, &[1], "forward"), json!({"page": 1, "moved": 0}), "one place forward is past the band");
    put(&mut s, &[1], "back");
    assert_eq!(order(doc(&s)), [1, 2, 3]);
    // Nothing is written for the band: the order commands never carry a shape out of it (does not arise).
    assert_eq!(local(doc(&s), 3, "DisplayLevel").unwrap().1.as_deref(), Some("25000"));
    assert!(local(doc(&s), 1, "DisplayLevel").unwrap().1.as_deref() == Some("0"));
    // A shape in the reserved band -32768 has none to keep: it goes to the very front.
    let (mut t, _dir2) = banded(["0", "-32768", "25000"]);
    put(&mut t, &[2], "front");
    assert_eq!(order(doc(&t)), [1, 3, 2]);
    put(&mut t, &[2], "back");
    assert_eq!(order(doc(&t)), [2, 1, 3]);
}

#[test]
fn the_members_of_a_group_are_ordered_among_themselves() {
    let member = |id: u32| shape(id, &format!(r#"<Cell N="PinX" V="{id}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#));
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Shapes>{}{}{}</Shapes>"#,
            member(11),
            member(12),
            member(13)
        ),
    );
    let (mut s, _dir) = open_page(&format!("{}{group}", file_box(1, 1.0, 1.0, "")), "");
    let ids = |d: &Document| shape_of(d, 10).children.iter().map(|m| m.id).collect::<Vec<_>>();
    // The page part and the group's own list are separate: 11 to the front of its group, 1 to the front of the page.
    assert_eq!(put(&mut s, &[11, 1], "front"), json!({"page": 1, "moved": 2}));
    assert_eq!(ids(doc(&s)), [12, 13, 11]);
    assert_eq!(order(doc(&s)), [10, 1]);
    put(&mut s, &[13], "back");
    assert_eq!(ids(doc(&s)), [13, 12, 11]);
}

// ---- align ------------------------------------------------------------------------------------------------------------------------

/// A rectangle (1.5 by 1) at (2, 2), a hexagon (1.4 by 1.2) at (6, 5), a circle (1 by 1) at (9, 3) and a rectangle made 3 by 0.6 at (5, 7.5).
fn mixed() -> Session {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 2.0, 2.0);
    drop_shape(&mut s, "hexagon", 6.0, 5.0);
    drop_shape(&mut s, "circle", 9.0, 3.0);
    let wide = drop_shape(&mut s, "rectangle", 5.0, 7.5);
    run(&mut s, "shape.resize", json!({"id": wide, "x": 3.5, "y": 7.2, "width": 3.0, "height": 0.6}));
    s
}

fn box_of(s: &Session, id: u32) -> (f64, f64, f64, f64) {
    bounds(&corners(doc(s), id))
}

#[test]
fn align_moves_the_other_shapes_to_the_first_one_named() {
    for (how, edge) in [("left", 0), ("right", 2), ("bottom", 1), ("top", 3), ("center", 4), ("middle", 5)] {
        let mut s = mixed();
        let want = |b: (f64, f64, f64, f64)| match edge {
            0 => b.0,
            1 => b.1,
            2 => b.2,
            3 => b.3,
            4 => (b.0 + b.2) / 2.0,
            _ => (b.1 + b.3) / 2.0,
        };
        let first = box_of(&s, 3);
        let others: Vec<_> = [1, 2, 4].iter().map(|id| (*id, box_of(&s, *id))).collect();
        let r = run(&mut s, "shape.align", json!({"ids": [3, 1, 2, 4], "how": how}));
        assert_eq!(r, json!({"page": 1, "moved": 3}), "{how}");
        assert_eq!(box_of(&s, 3), first, "{how}: the first shape stays");
        for (id, was) in others {
            let now = box_of(&s, id);
            assert!(near(want(now), want(first)), "{how}: shape {id} is at {} and not {}", want(now), want(first));
            // The other axis is not touched, nor the size.
            let across = matches!(edge, 0 | 2 | 4);
            assert!(if across { near(now.1, was.1) && near(now.3, was.3) } else { near(now.0, was.0) && near(now.2, was.2) }, "{how}: shape {id}");
            assert!(near(now.2 - now.0, was.2 - was.0) && near(now.3 - now.1, was.3 - was.1), "{how}: shape {id} keeps its size");
        }
        // One undo step, and undo puts every shape back.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(box_of(&s, 1), box_of(&mixed(), 1), "{how}");
    }
}

#[test]
fn a_turned_shape_aligns_by_its_box_on_the_page_and_a_glued_connector_follows() {
    let mut s = mixed();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
    run(&mut s, "shape.rotate", json!({"ids": [2], "angle": 30}));
    run(&mut s, "shape.align", json!({"ids": [1, 2], "how": "left"}));
    let (a, b) = (box_of(&s, 1), box_of(&s, 2));
    assert!(near(a.0, b.0), "{a:?} {b:?}");
    assert!(b.2 - b.0 > 1.4, "the box of a turned hexagon is not its width: {b:?}");
    assert_ends_at_their_points(doc(&s), "after the align");
    let r = route(doc(&s), c);
    assert!(near_p(r[0], (num(doc(&s), c, "BeginX"), num(doc(&s), c, "BeginY"))));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn align_with_nothing_to_move_is_no_undo_step() {
    let mut s = mixed();
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.align", json!({"ids": [1], "how": "left"})), json!({"page": 1, "moved": 0}));
    assert_eq!(run(&mut s, "shape.align", json!({"ids": [1, 1, 1], "how": "top"})), json!({"page": 1, "moved": 0}));
    assert_unchanged(&s, &before, "");
    run(&mut s, "shape.align", json!({"ids": [2, 1], "how": "middle"}));
    let again = snapshot(&s);
    assert_eq!(run(&mut s, "shape.align", json!({"ids": [2, 1], "how": "middle"})), json!({"page": 1, "moved": 0}), "already aligned");
    assert_unchanged(&s, &again, "");
}

// ---- parameters, errors, undo ------------------------------------------------------------------------------------------------------

#[test]
fn commands_with_bad_parameters_are_refused_and_change_nothing() {
    let mut s = mixed();
    let before = snapshot(&s);
    for (command, params) in [
        ("shape.rotate", json!({"ids": [1]})),
        ("shape.rotate", json!({"ids": [1], "by": 10, "angle": 10})),
        ("shape.rotate", json!({"ids": [1], "by": "ten"})),
        ("shape.rotate", json!({"ids": [1], "by": 1e300})),
        ("shape.rotate", json!({"ids": [1], "angle": -1e300})),
        ("shape.rotate", json!({"ids": [], "by": 10})),
        ("shape.rotate", json!({"ids": [99], "by": 10})),
        ("shape.rotate", json!({"ids": [1, 99], "by": 10})),
        ("shape.rotate", json!({"by": 10})),
        ("shape.rotate", json!({"ids": [1], "by": 10, "page": 7})),
        ("shape.flip", json!({"ids": [1]})),
        ("shape.flip", json!({"ids": [1], "axis": "sideways"})),
        ("shape.flip", json!({"ids": [1], "axis": 3})),
        ("shape.flip", json!({"ids": [99], "axis": "vertical"})),
        ("shape.order", json!({"ids": [1]})),
        ("shape.order", json!({"ids": [1], "to": "top"})),
        ("shape.order", json!({"ids": [99], "to": "front"})),
        ("shape.align", json!({"ids": [1, 2]})),
        ("shape.align", json!({"ids": [1, 2], "how": "centre"})),
        ("shape.align", json!({"ids": [1, 99], "how": "left"})),
    ] {
        let e = error_of(&mut s, command, &params);
        assert!(matches!(e, EngineError::BadParams { .. }), "{command} {params}: {e:?}");
        assert_eq!(snapshot(&s), before, "{command} {params}");
    }
    let too_many: Vec<u32> = (0..5_000).collect();
    let e = error_of(&mut s, "shape.rotate", &json!({"ids": too_many, "by": 10}));
    assert!(bad_params_msg(&e).contains("4096"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_guarded_pose_cell_stops_the_command_and_the_drawing_is_as_it_was() {
    let guarded = file_box(10, 4.0, 3.0, r#"<Cell N="Angle" V="0" F="GUARD(0)"/><Cell N="FlipX" V="0" F="GUARD(0)"/>"#);
    let (mut s, _dir) = open_page(&format!("{guarded}{}", file_box(11, 8.0, 3.0, "")), "");
    let before = snapshot(&s);
    for (command, params) in [
        ("shape.rotate", json!({"ids": [10], "by": 90})),
        ("shape.rotate", json!({"ids": [11, 10], "by": 90})),
        ("shape.flip", json!({"ids": [10], "axis": "horizontal"})),
        ("shape.flip", json!({"ids": [11, 10], "axis": "horizontal"})),
    ] {
        let e = error_of(&mut s, command, &params);
        assert!(matches!(e, EngineError::Failed { .. }), "{command}: {e:?}");
        assert!(e.to_string().contains("GUARD"), "{e}");
        assert_eq!(snapshot(&s), before, "{command}");
    }
    // The guarded shape does not stop one that is not.
    run(&mut s, "shape.rotate", json!({"ids": [11], "by": 90}));
}

#[test]
fn every_arrange_command_is_one_undo_step_that_undo_and_redo_give_back() {
    let mut s = mixed();
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    for (command, params) in [
        ("shape.rotate", json!({"ids": [1, 2], "by": 37})),
        ("shape.rotate", json!({"ids": [3], "angle": 120})),
        ("shape.flip", json!({"ids": [2, 3], "axis": "horizontal"})),
        ("shape.flip", json!({"ids": [2], "axis": "vertical"})),
        ("shape.order", json!({"ids": [1], "to": "front"})),
        ("shape.order", json!({"ids": [2], "to": "forward"})),
        ("shape.align", json!({"ids": [3, 1, 2], "how": "bottom"})),
        ("shape.resize", json!({"id": 2, "x": 5.0, "y": 4.0, "width": 2.0, "height": 1.5})),
    ] {
        let before = doc(&s).clone();
        let steps = s.history().unwrap().undo_len();
        run(&mut s, command, params.clone());
        let after = doc(&s).clone();
        assert_ne!(after, before, "{command} {params} changed something");
        assert_eq!(s.history().unwrap().undo_len(), steps + 1, "{command} {params}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(doc(&s), &before, "undo of {command} {params}");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(doc(&s), &after, "redo of {command} {params}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_drawing_with_turned_and_flipped_shapes_and_their_connectors_saves_and_reads_back() {
    let mut s = mixed();
    run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 3}, "to": {"id": 2, "point": 1}}));
    run(&mut s, "connector.add", json!({"from": {"id": 2}, "to": {"id": 3}}));
    run(&mut s, "shape.rotate", json!({"ids": [2], "by": 30}));
    run(&mut s, "shape.flip", json!({"ids": [3], "axis": "horizontal"}));
    run(&mut s, "shape.rotate", json!({"ids": [1, 4], "by": 90}));
    run(&mut s, "shape.order", json!({"ids": [2], "to": "back"}));
    let dir = Dir::new();
    let path = dir.path("turned.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(order(doc(&t)), order(doc(&s)));
    for id in 1..=6 {
        assert_corners("read back", doc(&t), id, corners(doc(&s), id));
    }
    assert_ends_at_their_points(doc(&t), "read back");
    assert_eq!(check_session(&t), Ok(()));
}

// ---- groups, formulas that read the angle, labels ------------------------------------------------------------------------------------

/// Group 10 at (5, 5), 4 by 2 with its local pin at (2, 1), not turned, holding member 11 (1 by 1, its pin at its middle by formula, at (1, 1) of the
/// group's frame, so at (4, 5) on the page); and a box 20 on the page at (8, 3).
fn group_and_box() -> (Session, Dir) {
    let member = shape(
        11,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Shapes>{member}</Shapes>"#
        ),
    );
    open_page(&format!("{group}{}", file_box(20, 8.0, 3.0, "")), "")
}

#[test]
fn a_member_of_a_group_that_is_not_turned_turns_flips_and_aligns_with_the_page() {
    let (mut s, _dir) = group_and_box();
    let (member, other) = (corners(doc(&s), 11), corners(doc(&s), 20));
    assert_corners("on the page", doc(&s), 11, [(3.5, 4.5), (4.5, 4.5), (4.5, 5.5), (3.5, 5.5)]);
    // Turned together about the middle of their joint box, as the page would turn.
    let joint = bounds(&[member[0], member[2], other[0], other[2]]);
    let c = ((joint.0 + joint.2) / 2.0, (joint.1 + joint.3) / 2.0);
    run(&mut s, "shape.rotate", json!({"ids": [11, 20], "by": 90}));
    let d = doc(&s);
    for (id, was) in [(11, member), (20, other)] {
        for (w, n) in was.iter().zip(&corners(d, id)) {
            assert!(near_p(turn_about(c, degrees(90.0), *w), *n), "shape {id}: {w:?} to {n:?}");
        }
    }
    // The member's own cells are in its group's frame: its pin there is a step away from where the page's turn put it.
    let page_pin = turn_about(c, degrees(90.0), (4.0, 5.0));
    assert!(near_p(pin(d, 11), (page_pin.0 - 3.0, page_pin.1 - 4.0)), "{:?} for {page_pin:?}", pin(d, 11));
    // Flipped together.
    let (member, other) = (corners(d, 11), corners(d, 20));
    let joint = bounds(&[member[0], member[2], other[0], other[2]]);
    run(&mut s, "shape.flip", json!({"ids": [11, 20], "axis": "horizontal"}));
    let d = doc(&s);
    for (id, was) in [(11, member), (20, other)] {
        for (w, n) in was.iter().zip(&corners(d, id)) {
            assert!(near_p(mirror_x((joint.0 + joint.2) / 2.0, *w), *n), "shape {id}: {w:?} to {n:?}");
        }
    }
    // Aligned to the box on the page.
    run(&mut s, "shape.align", json!({"ids": [20, 11], "how": "bottom"}));
    let (a, b) = (bounds(&corners(doc(&s), 20)), bounds(&corners(doc(&s), 11)));
    assert!(near(a.1, b.1), "{a:?} {b:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_named_with_its_group_is_left_to_the_group() {
    // The group takes its member with it, so the member is not turned, flipped or moved a second time.
    let (mut s, _dir) = group_and_box();
    let was = corners(doc(&s), 11);
    let r = run(&mut s, "shape.rotate", json!({"ids": [10, 11], "by": 45}));
    assert_eq!(r["rotated"], 1);
    assert!(near(num(doc(&s), 10, "Angle"), degrees(45.0)));
    assert!(local(doc(&s), 11, "Angle").is_none(), "the member's own angle is as it was");
    let turned = corners(doc(&s), 11);
    let pin_of_group = (5.0, 5.0);
    for (w, n) in was.iter().zip(&turned) {
        assert!(near_p(turn_about(pin_of_group, degrees(45.0), *w), *n), "{w:?} to {n:?}");
    }
    let r = run(&mut s, "shape.flip", json!({"ids": [11, 10], "axis": "vertical"}));
    assert_eq!(r["flipped"], 1);
    assert!(local(doc(&s), 11, "FlipY").is_none());
    // Aligned: the member moves with the group, and the box is the one that is aligned.
    let r = run(&mut s, "shape.align", json!({"ids": [20, 10, 11], "how": "left"}));
    assert_eq!(r["moved"], 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_formula_that_reads_the_angle_is_worked_out_again_with_the_turn() {
    // A file keeps text upright with a formula of the angle: it follows a turn and a flip.
    let upright = r#"<Cell N="TxtAngle" V="0" F="-Angle"/>"#;
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, upright), "");
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 90}));
    assert!(near(num(doc(&s), 10, "TxtAngle"), -degrees(90.0)), "{}", num(doc(&s), 10, "TxtAngle"));
    assert_eq!(local(doc(&s), 10, "TxtAngle").unwrap().0.as_deref(), Some("-Angle"), "the formula stays");
    run(&mut s, "shape.flip", json!({"ids": [10], "axis": "vertical"}));
    // The angle is now minus a quarter turn, and the text's turn is minus that.
    assert!(near(num(doc(&s), 10, "Angle"), -degrees(90.0)));
    assert!(near(num(doc(&s), 10, "TxtAngle"), degrees(90.0)), "{}", num(doc(&s), 10, "TxtAngle"));
}

#[test]
fn the_label_of_a_turned_connector_stays_upright() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 3.0}})));
    run(&mut s, "shape.set_text", json!({"id": c, "text": "yes"}));
    run(&mut s, "shape.rotate", json!({"ids": [c], "by": 90}));
    let d = doc(&s);
    // Minus the connector's angle, wrapped to the range from just over -pi to pi.
    assert!(near(num(d, c, "TxtAngle"), -degrees(90.0)), "{}", num(d, c, "TxtAngle"));
    // The label is at the middle of the route, in the connector's frame.
    assert!(near(num(d, c, "TxtPinX"), 2.0), "{}", num(d, c, "TxtPinX"));
}

// ---- what it costs --------------------------------------------------------------------------------------------------------------------

#[test]
fn what_each_arrange_command_costs_on_a_page_of_500_shapes_and_300_connectors() {
    use crate::edit_cost::{COLUMNS, CONNECTORS, big_page};
    let mut s = big_page();
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 500 + CONNECTORS);
    // The fifth shape of the sixth row has a connector on each side.
    let one = 5 * COLUMNS + 4 + 1;
    let hundred: Vec<u32> = (1..=100).collect();
    // Each step is a command and whether it is timed; an untimed one puts the shapes back where the next timed one has something to do.
    type Step = (bool, &'static str, Json);
    let every = |step: &dyn Fn(i32) -> Step| -> Vec<Step> { (0..7).map(step).collect() };
    let cases: Vec<(&str, Vec<Step>)> = vec![
        ("rotate one shape with a connector on each side", every(&|i| (true, "shape.rotate", json!({"ids": [one], "by": 15 + i})))),
        ("flip one shape with a connector on each side", every(&|_| (true, "shape.flip", json!({"ids": [one], "axis": "horizontal"})))),
        ("turn 100 shapes as one", every(&|_| (true, "shape.rotate", json!({"ids": hundred, "by": 10})))),
        ("flip 100 shapes as one", every(&|_| (true, "shape.flip", json!({"ids": hundred, "axis": "vertical"})))),
        ("bring one shape to the front", every(&|i| (true, "shape.order", json!({"ids": [one], "to": if i % 2 == 0 { "front" } else { "back" }})))),
        (
            "bring 100 shapes to the front",
            every(&|i| (true, "shape.order", json!({"ids": hundred, "to": if i % 2 == 0 { "front" } else { "back" }}))),
        ),
        (
            "align 100 shapes to the first (99 of them have to move)",
            (0..14)
                .map(|i| {
                    if i % 2 == 0 {
                        (false, "shape.move", json!({"ids": (2..=100).collect::<Vec<u32>>(), "dx": 0.3, "dy": 0.2}))
                    } else {
                        (true, "shape.align", json!({"ids": hundred, "how": if i % 4 == 1 { "left" } else { "top" }}))
                    }
                })
                .collect(),
        ),
        (
            "resize one shape that is turned",
            every(&|i| (true, "shape.resize", json!({"id": one, "x": 8.0, "y": 8.0, "width": 1.5 + 0.01 * f64::from(i), "height": 1.0}))),
        ),
    ];
    run(&mut s, "shape.rotate", json!({"ids": [one], "angle": 30}));
    // Counted, not timed (`diacraft_calc::work`): the commits each command makes and the steps they take, against the steps of the recalculation of
    // the whole page, which is what each would cost if it were not a recalculation of what changed.
    let mut copy = doc(&s).clone();
    let whole =
        diacraft_calc::commit_in(&mut copy, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default(), diacraft_calc::Scope::Page(1))
            .stats
            .steps;
    for (what, steps) in cases {
        let mut worst = (0, 0);
        for (counted, command, params) in steps {
            diacraft_calc::work::reset();
            s.execute(command, &params).unwrap();
            if counted {
                worst = (diacraft_calc::work::commits().max(worst.0), diacraft_calc::work::steps().max(worst.1));
            }
        }
        eprintln!(
            "{what} on 500 shapes and {CONNECTORS} connectors: {} commits of {} steps at the most; the whole page is {whole} steps",
            worst.0, worst.1
        );
        assert!(worst.0 <= 1, "{what}: {} commits, and one of what it changed is the most", worst.0);
        assert!(worst.1 < whole, "{what} took {} steps, the whole page {whole}", worst.1);
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_most_ids_one_arrange_command_takes_on_a_page_of_that_many_shapes_is_bounded() {
    use diacraft_engine::draw::drop_shape;
    use diacraft_engine::stencil::find_master;
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    s.edit("build", |d| {
        for i in 0..4_100_u32 {
            drop_shape(d, 1, rectangle, 1.0 + f64::from(i % 80) * 2.0, 1.0 + f64::from(i / 80) * 2.0)
                .map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
        }
        Ok(())
    })
    .unwrap();
    let mut copy = doc(&s).clone();
    let whole =
        diacraft_calc::commit_in(&mut copy, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default(), diacraft_calc::Scope::Page(1))
            .stats
            .steps;
    let ids: Vec<u32> = (1..=4_096).collect();
    // Counted, not timed: one commit for each command, of no more steps than the page has (a commit for each id would be 4,096 of them).
    for (what, command, params) in [
        ("turning 4,096 shapes as one", "shape.rotate", json!({"ids": ids, "by": 30})),
        ("flipping 4,096 shapes as one", "shape.flip", json!({"ids": ids, "axis": "horizontal"})),
        ("turning 4,096 shapes to an angle", "shape.rotate", json!({"ids": ids, "angle": 45})),
        ("bringing 4,096 shapes to the front", "shape.order", json!({"ids": ids, "to": "front"})),
        ("sending 4,096 shapes backward", "shape.order", json!({"ids": ids, "to": "backward"})),
        ("aligning 4,096 shapes", "shape.align", json!({"ids": ids, "how": "middle"})),
    ] {
        diacraft_calc::work::reset();
        s.execute(command, &params).unwrap();
        let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
        eprintln!("{what} on a page of 4,100: {commits} commits of {steps} steps; the whole page is {whole} steps");
        assert!(commits <= 1, "{what} made {commits} commits");
        assert!(steps < whole, "{what} took {steps} steps, the whole page {whole}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
