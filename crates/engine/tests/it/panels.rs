//! Size & Position: `shape.pin`, `shape.size`, `shape.set_end`, `shape.set_length` and the query `shape.placement`, through
//! `Session::execute` as the control channel calls it. Lengths are paper inches, angles degrees; the cells hold drawing units and radians.
//!
//! The file shapes are built in code from the specifications' XML, with the values real files hold: `LocPinX` as the formula `Width*0.5` (the form
//! it is created with) and an off-centre pin as `Width*0.25`, shapes turned (`Angle` 30 degrees as `0.5235987755983`) and flipped, a member of a
//! turned group, and a page with a drawing scale, where the cells are in drawing units and a paper inch is `units_per_inch` of them.

use diacraft_doc::{CellAddr, Document, Lookup, Owner};
use diacraft_engine::draw::{connection_points, shape_corners};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num, route};
use crate::edit_foreign::Dir;
use crate::{Snapshot, blank_session, check_session, run, snapshot};

pub(crate) type P = (f64, f64);

/// How near two numbers are the same: a cell holds nine places.
const TOL: f64 = 1.0e-8;

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

pub(crate) fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn corners(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}"))
}

fn assert_same_corners(what: &str, was: &[P; 4], now: &[P; 4]) {
    assert!(was.iter().zip(now).all(|(w, n)| near_p(*w, *n)), "{what}: the corners were {was:?} and are {now:?}");
}

pub(crate) fn pin(d: &Document, id: u32) -> P {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap()
}

/// What shape `id` holds locally in a cell: its formula and its value text, and its unit.
fn local(d: &Document, id: u32, cell: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => {
            Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref().map(str::to_owned)))
        }
        _ => None,
    }
}

/// The `Angle` a shape holds, 0 for one that holds none.
fn angle_of(d: &Document, id: u32) -> f64 {
    local(d, id, "Angle").and_then(|c| c.1).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn formula(d: &Document, id: u32, cell: &str) -> Option<String> {
    local(d, id, cell).and_then(|c| c.0)
}

/// A command that changed nothing left the document, the undo and redo steps and the revision as they were.
pub(crate) fn assert_unchanged(s: &Session, before: &Snapshot, what: &str) {
    let now = snapshot(s);
    assert_eq!(now.document, before.document, "{what}: the document");
    assert_eq!(
        (now.undo_steps, now.redo_steps, now.revision),
        (before.undo_steps, before.redo_steps, before.revision),
        "{what}: no step, no revision"
    );
}

pub(crate) fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

// ---- drawings from files -------------------------------------------------------------------------------------------------------

pub(crate) const LINE: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/>"##;

/// A box 2 by 1 with no master, as a file holds it: its pin at (`x`, `y`), its local pin a quarter of the way in by formulas (`Width*0.25`, so not at
/// the middle), a connection point on each side (0 right, 1 left, 2 top, 3 bottom) and `extra` cells.
pub(crate) fn file_box(id: u32, x: f64, y: f64, extra: &str) -> String {
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

/// A line as a file holds one that is not a connector of ours: its ends are cells, and its `Width`, `Angle` and pin are formulas of them (the
/// creation formulas).
pub(crate) fn file_line(id: u32, begin: P, end: P, extra: &str) -> String {
    let (dx, dy) = (end.0 - begin.0, end.1 - begin.1);
    let (width, angle) = (dx.hypot(dy), dy.atan2(dx));
    shape(
        id,
        &format!(
            r#"<Cell N="BeginX" V="{}"/><Cell N="BeginY" V="{}"/><Cell N="EndX" V="{}"/><Cell N="EndY" V="{}"/>
<Cell N="PinX" V="{}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{}" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{width}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>{LINE}{extra}
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{width}" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#,
            begin.0,
            begin.1,
            end.0,
            end.1,
            (begin.0 + end.0) / 2.0,
            (begin.1 + end.1) / 2.0,
            width / 2.0
        ),
    )
}

/// A session that has opened a page holding `shapes` and `connects`.
pub(crate) fn open_page(shapes: &str, connects: &str) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(shapes).connects(connects)).build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    (s, dir)
}

/// Group 10 at (5, 5), 4 by 2 with its local pin at (2, 1), turned by `angle` radians (and flipped when `flip` is `FlipX`), holding member 11: a
/// 1 by 1 box at (1, 1) of the group's frame whose pin is at its middle by formulas, and line 12, from (0.5, 1.5) to (3.5, 1.5) of the group's frame.
pub(crate) fn group_with_members(angle: &str, flip: &str) -> String {
    let member = shape(
        11,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
    );
    let line = file_line(12, (0.5, 1.5), (3.5, 1.5), "");
    shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/>
<Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Cell N="Angle" V="{angle}"/>{flip}<Shapes>{member}{line}</Shapes>"#
        ),
    )
}

/// 30 degrees as a file holds an angle: radians to thirteen places.
const THIRTY: &str = r#"<Cell N="Angle" V="0.5235987755983" U="DEG" F="30 deg"/>"#;

pub(crate) fn placement(s: &mut Session, ids: &[u32]) -> Json {
    run(s, "shape.placement", json!({"ids": ids}))
}

pub(crate) fn shape_entry(r: &Json, id: u32) -> &Json {
    r["shapes"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no entry for shape {id} in {r}"))
}

// ---- shape.pin -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_is_pinned_at_each_of_the_nine_places_and_stays_where_it_is() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let was = corners(doc(&s), a);
    // The box is 1.5 by 1 with its lower-left corner at (3.25, 2.5).
    assert!(near_p(was[0], (3.25, 2.5)) && near_p(was[2], (4.75, 3.5)), "{was:?}");
    let nine = [
        ("top-left", (0.0, 1.0)),
        ("top-center", (0.5, 1.0)),
        ("top-right", (1.0, 1.0)),
        ("middle-left", (0.0, 0.5)),
        ("center", (0.5, 0.5)),
        ("middle-right", (1.0, 0.5)),
        ("bottom-left", (0.0, 0.0)),
        ("bottom-center", (0.5, 0.0)),
        ("bottom-right", (1.0, 0.0)),
    ];
    // The centre is the pin the rectangle has: nothing to change, and no step.
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [a], "at": "Centre"})), json!({"page": 1, "pinned": 0}));
    assert_unchanged(&s, &before, "the pin it has");
    for (name, (fx, fy)) in nine {
        let r = run(&mut s, "shape.pin", json!({"ids": [a], "at": name}));
        let d = doc(&s);
        assert_eq!(r, json!({"page": 1, "pinned": 1}), "{name}");
        assert_same_corners(name, &was, &corners(d, a));
        // The pin is at the place on the page, and the cells say so: formulas of the size, as the cell is created.
        assert!(near_p(pin(d, a), (3.25 + fx * 1.5, 2.5 + fy)), "{name}: the pin is {:?}", pin(d, a));
        assert!(near(num(d, a, "LocPinX"), fx * 1.5) && near(num(d, a, "LocPinY"), fy), "{name}");
        assert_eq!(formula(d, a, "LocPinX"), Some(format!("Width*{fx}")), "{name}");
        assert_eq!(formula(d, a, "LocPinY"), Some(format!("Height*{fy}")), "{name}");
        assert_eq!(local(d, a, "LocPinX").unwrap().2.as_deref(), Some("IN"));
        let at = shape_entry(&placement(&mut s, &[a]), a).clone();
        assert_eq!(at["pin_at"], name, "the window reads back the name it sent");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_pin_is_a_formula_of_the_size_so_a_shape_pinned_top_left_keeps_its_top_left_corner_when_it_is_sized() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    run(&mut s, "shape.pin", json!({"ids": [a], "at": "top-left"}));
    let top_left = corners(doc(&s), a)[3];
    assert!(near_p(top_left, (3.25, 3.5)));
    run(&mut s, "shape.size", json!({"ids": [a], "width": 4.0, "height": 2.5}));
    let d = doc(&s);
    assert!(near_p(corners(d, a)[3], top_left), "the top-left corner is the pin, and the pin stays: {:?}", corners(d, a));
    assert!(near_p(corners(d, a)[0], (3.25, 1.0)) && near_p(corners(d, a)[2], (7.25, 3.5)), "{:?}", corners(d, a));
    // And a pin at the middle that a formula keeps there stays at the middle.
    let b = drop_shape(&mut s, "rectangle", 9.0, 3.0);
    run(&mut s, "shape.size", json!({"ids": [b], "width": 3.0}));
    let c = corners(doc(&s), b);
    assert!(near_p(c[0], (7.5, 2.5)) && near_p(c[1], (10.5, 2.5)), "grown about the pin, which is at the middle: {c:?}");
}

#[test]
fn a_turned_and_flipped_shape_with_a_pin_off_the_middle_is_pinned_without_moving_in_the_picture() {
    let flip = r#"<Cell N="FlipX" V="1"/>"#;
    for (extra, what) in [(THIRTY.to_owned(), "turned"), (format!("{THIRTY}{flip}"), "turned and flipped"), (flip.to_owned(), "flipped")] {
        let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, &extra), "");
        let (was, angle) = (corners(doc(&s), 10), angle_of(doc(&s), 10));
        for (name, corner) in [("top-left", Some(3)), ("top-right", Some(2)), ("bottom-right", Some(1)), ("bottom-left", Some(0)), ("center", None)] {
            run(&mut s, "shape.pin", json!({"ids": [10], "at": name}));
            let d = doc(&s);
            assert_same_corners(&format!("{what}, {name}"), &was, &corners(d, 10));
            assert!(near(angle_of(d, 10), angle), "{what}: the angle is not touched");
            // The pin is at the corner of the shape's own frame that the name says (corners are listed from (0, 0) of that frame), or at the middle.
            let want = match corner {
                Some(i) => was[i],
                None => ((was[0].0 + was[2].0) / 2.0, (was[0].1 + was[2].1) / 2.0),
            };
            assert!(near_p(pin(d, 10), want), "{what}, {name}: the pin is {:?}, want {want:?}", pin(d, 10));
        }
        let r = placement(&mut s, &[10]);
        assert_eq!(shape_entry(&r, 10)["flip_x"], what.contains("flipped"), "{what}");
    }
}

#[test]
fn a_pin_at_a_point_of_the_shapes_own_frame_is_a_value_that_stays_where_it_is_when_the_shape_is_sized() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, THIRTY), "");
    let was = corners(doc(&s), 10);
    // The pin the box has already, a quarter in: nothing to change, and the formulas stay.
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [10], "at": {"x": 0.5, "y": 0.25}}))["pinned"], 0);
    assert_unchanged(&s, &before, "the pin it has");
    assert_eq!(formula(doc(&s), 10, "LocPinX").as_deref(), Some("Width*0.25"));
    // Another point: values, with the unit of a length, and the box where it was.
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [10], "at": {"x": 1.5, "y": 0.75}}))["pinned"], 1);
    let d = doc(&s);
    assert_same_corners("custom", &was, &corners(d, 10));
    assert_eq!(local(d, 10, "LocPinX"), Some((None, Some("1.5".to_owned()), Some("IN".to_owned()))));
    assert_eq!(local(d, 10, "LocPinY"), Some((None, Some("0.75".to_owned()), Some("IN".to_owned()))));
    let pin_now = pin(d, 10);
    assert_eq!(shape_entry(&placement(&mut s, &[10]), 10)["pin_at"], "custom");
    // A resize leaves a pin that is a value where it is: the pin stays on the page and the box grows from it (so the pin is not at a place).
    run(&mut s, "shape.size", json!({"ids": [10], "width": 4.0}));
    let d = doc(&s);
    assert!(near_p(pin(d, 10), pin_now), "the pin stays");
    assert!(near(num(d, 10, "LocPinX"), 1.5) && near(num(d, 10, "Width"), 4.0));
    // Only the x of the point: the y stays what it is.
    run(&mut s, "shape.pin", json!({"ids": [10], "at": "bottom-left"}));
    assert_eq!(formula(doc(&s), 10, "LocPinY").as_deref(), Some("Height*0"));
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [10], "at": {"x": 2.0, "y": 0.0}}))["pinned"], 1, "only the x differs");
    assert_eq!(formula(doc(&s), 10, "LocPinY").as_deref(), Some("Height*0"), "the y was where it was and keeps its formula");
}

#[test]
fn several_shapes_are_pinned_in_one_step_and_a_shape_already_there_is_not_counted() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    let c = drop_shape(&mut s, "hexagon", 9.0, 2.0);
    let was: Vec<[P; 4]> = [a, b, c].iter().map(|id| corners(doc(&s), *id)).collect();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.pin", json!({"ids": [a, b, c, a], "at": "bottom-left"}));
    assert_eq!(r, json!({"page": 1, "pinned": 3}), "a shape named twice is counted once");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    for (id, w) in [a, b, c].iter().zip(&was) {
        assert_same_corners("bottom-left", w, &corners(doc(&s), *id));
        assert!(near_p(pin(doc(&s), *id), w[0]), "the pin is at the lower-left corner");
    }
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [a, b, c], "at": "bottom-left"}))["pinned"], 0);
    assert_unchanged(&s, &before, "all there already");
    // Mixed: one that is not there yet.
    let d = drop_shape(&mut s, "rectangle", 2.0, 8.0);
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [a, d], "at": "bottom-left"}))["pinned"], 1);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_pin_is_one_undo_step_that_takes_back_the_very_document() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let before = doc(&s).clone();
    run(&mut s, "shape.pin", json!({"ids": [a], "at": "top-right"}));
    assert_ne!(doc(&s), &before);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    run(&mut s, "edit.redo", json!({}));
    assert!(near_p(pin(doc(&s), a), (4.75, 3.5)));
}

#[test]
fn a_line_is_refused_a_pin_and_a_group_and_the_member_of_a_turned_group_take_one() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 5.0, "y": 5.0}, "to": {"x": 8.0, "y": 5.0}})));
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.pin", &json!({"ids": [a, c], "at": "top-left"}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("its pin is where its ends put it"), "{e}");
    assert_unchanged(&s, &before, "a line among the shapes refuses the whole call");
    // A group.
    let b = drop_shape(&mut s, "ellipse", 6.0, 2.0);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let members = [corners(doc(&s), a), corners(doc(&s), b)];
    let group_was = corners(doc(&s), group);
    run(&mut s, "shape.pin", json!({"ids": [group], "at": "top-left"}));
    let d = doc(&s);
    assert_same_corners("group", &group_was, &corners(d, group));
    assert_same_corners("member a", &members[0], &corners(d, a));
    assert_same_corners("member b", &members[1], &corners(d, b));
    // A member named with its group is pinned too, and nothing moves: the group's pin and the member's are two cells.
    assert_eq!(run(&mut s, "shape.pin", json!({"ids": [group, a], "at": "bottom-right"}))["pinned"], 2);
    let d = doc(&s);
    assert_same_corners("group again", &group_was, &corners(d, group));
    assert_same_corners("member a again", &members[0], &corners(d, a));
    assert_same_corners("member b again", &members[1], &corners(d, b));
    // The member of a group turned a quarter turn and flipped: the pin is in the group's frame, carried by the member's own pose, so no frame is crossed.
    let (mut t, _dir) = open_page(&group_with_members("1.5707963267949", r#"<Cell N="FlipY" V="1"/>"#), "");
    let was = corners(doc(&t), 11);
    run(&mut t, "shape.pin", json!({"ids": [11], "at": "top-right"}));
    assert_same_corners("member of a turned and flipped group", &was, &corners(doc(&t), 11));
    // Its pin in the group's frame is its own top-right corner: the member is 1 by 1 and unturned in that frame.
    assert!(near_p(pin(doc(&t), 11), (1.5, 1.5)), "{:?}", pin(doc(&t), 11));
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn a_pin_and_a_size_and_the_end_of_a_line_survive_a_save_and_an_open() {
    let dir = Dir::new();
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let b = drop_shape(&mut s, "ellipse", 8.0, 3.0);
    let line = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 6.0}, "to": {"x": 3.0, "y": 6.0}})));
    run(&mut s, "shape.pin", json!({"ids": [a], "at": "bottom-right"}));
    run(&mut s, "shape.pin", json!({"ids": [b], "at": {"x": 0.25, "y": 0.75}}));
    run(&mut s, "shape.size", json!({"ids": [a, b], "width": 2.0}));
    run(&mut s, "shape.set_end", json!({"id": line, "end": "end", "x": 5.0, "y": 7.0}));
    let (was_a, was_b) = (corners(doc(&s), a), corners(doc(&s), b));
    let path = dir.path("pinned.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let d = doc(&t);
    assert_same_corners("a", &was_a, &corners(d, a));
    assert_same_corners("b", &was_b, &corners(d, b));
    assert_eq!(formula(d, a, "LocPinX").as_deref(), Some("Width*1"), "the pin of a place stays a formula of the size");
    assert_eq!(formula(d, a, "LocPinY").as_deref(), Some("Height*0"));
    assert_eq!(local(d, b, "LocPinX").map(|c| (c.0, c.1)), Some((None, Some("0.25".to_owned()))), "and a point stays a value");
    assert_eq!(ends(d, line), ((1.0, 6.0), (5.0, 7.0)));
    // Read back, the window's fields say what they said.
    let e = shape_entry(&placement(&mut t, &[a]), a).clone();
    assert_eq!(e["pin_at"], "bottom-right");
    assert_eq!(shape_entry(&placement(&mut t, &[b]), b)["pin_at"], "custom");
    // And a size after the open keeps the pin at the bottom right.
    run(&mut t, "shape.size", json!({"ids": [a], "width": 3.0, "height": 2.0}));
    assert!(near_p(corners(doc(&t), a)[1], was_a[1]), "the bottom-right corner stays: {:?}", corners(doc(&t), a));
}

#[test]
fn a_pin_the_drawing_guards_is_an_error_and_nothing_changes() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, r#"<Cell N="Width" V="2"/>"#).replace(r#"F="Width*0.25""#, r#"F="GUARD(Width*0.25)""#), "");
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.pin", &json!({"ids": [10], "at": "top-left"}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("GUARD"), "{e}");
    assert_unchanged(&s, &before, "a guarded cell");
}

#[test]
fn a_pin_on_a_page_with_a_drawing_scale_is_in_paper_inches_and_held_in_drawing_units() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let k = placement(&mut s, &[a])["units_per_inch"].as_f64().unwrap();
    assert!(near(k, 48.0));
    let was = corners(doc(&s), a);
    run(&mut s, "shape.pin", json!({"ids": [a], "at": {"x": 0.25, "y": 0.5}}));
    let d = doc(&s);
    assert_same_corners("scaled", &was, &corners(d, a));
    assert!(near(num(d, a, "LocPinX"), 0.25 * 48.0) && near(num(d, a, "LocPinY"), 0.5 * 48.0), "{} {}", num(d, a, "LocPinX"), num(d, a, "LocPinY"));
    let e = shape_entry(&placement(&mut s, &[a]), a).clone();
    assert!(near(e["pin"]["x"].as_f64().unwrap(), 0.25) && near(e["pin"]["y"].as_f64().unwrap(), 0.5), "{e}");
    assert_eq!(e["pin_at"], "custom");
}

#[test]
fn a_pin_with_a_place_or_shapes_that_are_not_there_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"ids": [a], "at": "nowhere"}), "not a place of the box"),
        (json!({"ids": [a], "at": "\u{1b}[2J"}), "not a place of the box"),
        (json!({"ids": [a], "at": "top"}), "not a place of the box"),
        (json!({"ids": [a]}), "missing `at`"),
        (json!({"ids": [a], "at": 3}), "must be a place"),
        (json!({"ids": [a], "at": ["top-left"]}), "must be a place"),
        (json!({"ids": [a], "at": {"x": 1}}), "missing `y`"),
        (json!({"ids": [a], "at": {"x": 1, "y": 1, "z": 1}}), "unknown key"),
        (json!({"ids": [a], "at": {"x": 1e300, "y": 1}}), "within"),
        (json!({"ids": [a], "at": {"x": "1", "y": 1}}), "must be a number"),
        (json!({"ids": [99], "at": "center"}), "no shape 99"),
        (json!({"ids": [a], "at": "center", "page": 7}), "no page 7"),
        (json!({"ids": [], "at": "center"}), "between 1 and"),
    ] {
        let e = error_of(&mut s, "shape.pin", &params);
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(words), "{params}: {e}");
        assert!(!e.to_string().chars().any(char::is_control), "{params}: a control character came back raw in `{e}`");
        assert_unchanged(&s, &before, &params.to_string());
    }
}

// ---- shape.size ----------------------------------------------------------------------------------------------------------------

#[test]
fn several_shapes_are_given_one_size_each_keeping_its_pin_and_a_turned_shape_keeps_its_angle() {
    let (mut s, _dir) = open_page(&format!("{}{}", file_box(10, 4.0, 3.0, THIRTY), file_box(11, 9.0, 3.0, r#"<Cell N="FlipX" V="1"/>"#)), "");
    let a = drop_shape(&mut s, "rectangle", 2.0, 7.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 7.0);
    let pins = [10, 11, a, b].map(|id| pin(doc(&s), id));
    let angle = num(doc(&s), 10, "Angle");
    let heights = [10, 11, a, b].map(|id| num(doc(&s), id, "Height"));
    let r = run(&mut s, "shape.size", json!({"ids": [10, 11, a, b], "width": 3.0}));
    assert_eq!(r, json!({"page": 1, "sized": 4}));
    let d = doc(&s);
    for (i, id) in [10, 11, a, b].into_iter().enumerate() {
        assert!(near(num(d, id, "Width"), 3.0), "shape {id} is 3 wide");
        assert!(near(num(d, id, "Height"), heights[i]), "the height is not asked for");
        assert!(near_p(pin(d, id), pins[i]), "shape {id} keeps its pin");
    }
    // The pin of the box the formula keeps a quarter in is 0.75 in now, the middle's 1.5: the cells followed the size.
    assert!(near(num(d, 10, "LocPinX"), 0.75) && near(num(d, a, "LocPinX"), 1.5), "{} {}", num(d, 10, "LocPinX"), num(d, a, "LocPinX"));
    assert_eq!(formula(d, 10, "LocPinX").as_deref(), Some("Width*0.25"), "the formula is kept");
    assert!(near(num(d, 10, "Angle"), angle), "the turned box keeps its angle");
    assert_eq!(check_session(&s), Ok(()));
    // Both: the same size.
    let r = run(&mut s, "shape.size", json!({"ids": [10, 11, a, b], "width": 3.0, "height": 2.0}));
    assert_eq!(r["sized"], 4);
    let p = placement(&mut s, &[10, 11, a, b]);
    for e in p["shapes"].as_array().unwrap() {
        assert!(near(e["width"].as_f64().unwrap(), 3.0) && near(e["height"].as_f64().unwrap(), 2.0), "{e}");
    }
    // The same again: nothing to write, no step.
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.size", json!({"ids": [10, 11, a, b], "width": 3.0, "height": 2.0}))["sized"], 0);
    assert_unchanged(&s, &before, "the size they have");
}

#[test]
fn a_turned_shape_is_sized_along_its_own_axes_with_its_pin_where_it_is() {
    let (mut s, _dir) = open_page(&file_box(10, 4.0, 3.0, THIRTY), "");
    let pin_was = pin(doc(&s), 10);
    run(&mut s, "shape.size", json!({"ids": [10], "width": 4.0, "height": 2.0}));
    let d = doc(&s);
    // The corner at (0, 0) of its own frame is the pin less the local pin (a quarter in: 1, 0.5) turned by 30 degrees.
    let c = corners(d, 10);
    let (sin, cos) = 30.0_f64.to_radians().sin_cos();
    let turn = |v: P| (v.0 * cos - v.1 * sin, v.0 * sin + v.1 * cos);
    let corner = turn((-1.0, -0.5));
    assert!(near_p(c[0], (pin_was.0 + corner.0, pin_was.1 + corner.1)), "{:?}", c[0]);
    // Along the shape's own x axis the box is 4 long and along its y 2.
    let side = |a: P, b: P| (b.0 - a.0).hypot(b.1 - a.1);
    assert!(near(side(c[0], c[1]), 4.0) && near(side(c[1], c[2]), 2.0), "{c:?}");
    assert!(near_p(pin(d, 10), pin_was));
}

#[test]
fn a_line_or_a_connector_among_the_shapes_refuses_the_whole_call_and_a_group_with_its_member_does_too() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.size", &json!({"ids": [a, c], "width": 2.0}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("sized by its ends"), "{e}");
    assert_unchanged(&s, &before, "a connector");
    // A group is sized as one shape but not beside a member of its own: the group's size would carry the member a second time.
    let e = error_of(&mut s, "shape.size", &json!({"ids": [group, a], "height": 2.0}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains(&format!("shape {a} is in group {group}")), "{e}");
    assert_unchanged(&s, &before, "a group with its member");
    assert_eq!(run(&mut s, "shape.size", json!({"ids": [group], "height": 2.0}))["sized"], 1, "a group is sized");
    // The members of the group are not refused: they are shapes.
    assert_eq!(run(&mut s, "shape.size", json!({"ids": [a, b], "width": 2.5}))["sized"], 2);
}

#[test]
fn a_size_that_is_not_one_is_refused_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"ids": [a]}), "give `width`, `height` or both"),
        (json!({"ids": [a], "width": 0}), "more than 0"),
        (json!({"ids": [a], "height": -1}), "more than 0"),
        (json!({"ids": [a], "width": 1e300}), "within"),
        (json!({"ids": [a], "width": "3"}), "must be a number"),
        (json!({"ids": [99], "width": 1}), "no shape 99"),
        (json!({"width": 1}), "must be an array"),
    ] {
        let e = error_of(&mut s, "shape.size", &params);
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(words), "{params}: {e}");
        assert_unchanged(&s, &before, &params.to_string());
    }
}

#[test]
fn a_connector_glued_to_a_shape_that_is_sized_follows_its_connection_point() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 8.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let end_point = |d: &Document| {
        let p = connection_points(d, 1, a);
        let p = p.iter().find(|p| p.index == 3).unwrap();
        (p.x, p.y)
    };
    run(&mut s, "shape.size", json!({"ids": [a], "width": 3.0, "height": 2.0}));
    let d = doc(&s);
    let at = end_point(d);
    assert!(
        near_p((num(d, c, "BeginX"), num(d, c, "BeginY")), at),
        "the connector's begin {:?} is at the shape's point {at:?}",
        (num(d, c, "BeginX"), num(d, c, "BeginY"))
    );
    assert!(route(d, c).iter().all(|p| p.0.is_finite()), "and it is routed");
    assert!(d.page(1).unwrap().connects.iter().any(|k| k.from_sheet == c && k.to_sheet == a), "still glued");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_with_auto_size_grows_to_take_in_a_shape_that_is_sized_past_its_edge() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let width = |s: &mut Session| run(s, "page.setup", json!({}))["width"].as_f64().unwrap();
    run(&mut s, "page.auto_size", json!({"on": true}));
    assert!(near(width(&mut s), 11.0));
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.size", json!({"ids": [a], "width": 6.0}));
    assert!(width(&mut s) > 11.0, "the page grew by whole sheets");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "in the one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert!(near(width(&mut s), 11.0));
    // A pin does not move a shape and does not grow the page.
    run(&mut s, "shape.pin", json!({"ids": [a], "at": "bottom-right"}));
    assert!(near(width(&mut s), 11.0));
}

#[test]
fn an_end_or_a_length_that_takes_a_line_past_the_edge_grows_a_page_with_auto_size_in_the_one_undo_step() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 3.0}, "to": {"x": 4.0, "y": 3.0}})));
    let width = |s: &mut Session| run(s, "page.setup", json!({}))["width"].as_f64().unwrap();
    run(&mut s, "page.auto_size", json!({"on": true}));
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.set_end", json!({"id": c, "end": "end", "x": 14.0}));
    assert!(width(&mut s) > 11.0, "the page grew for the end");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(near(width(&mut s), 11.0));
    run(&mut s, "shape.set_length", json!({"id": c, "length": 20.0}));
    assert!(width(&mut s) > 11.0, "and for the length");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_size_on_a_page_with_a_drawing_scale_is_in_paper_inches_and_held_in_drawing_units() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let r = run(&mut s, "shape.size", json!({"ids": [a], "width": 1.0, "height": 0.5}));
    assert_eq!(r["sized"], 1);
    let d = doc(&s);
    assert!(near(num(d, a, "Width"), 48.0) && near(num(d, a, "Height"), 24.0), "{} {}", num(d, a, "Width"), num(d, a, "Height"));
    let e = shape_entry(&placement(&mut s, &[a]), a).clone();
    assert!(near(e["width"].as_f64().unwrap(), 1.0) && near(e["height"].as_f64().unwrap(), 0.5), "{e}");
}

#[test]
fn a_member_of_a_turned_group_is_sized_along_its_own_axes_and_keeps_its_pin() {
    // `shape.resize` refuses it, for the corner it takes is a point of the page; a size is a length and a turned group does not change one.
    let (mut s, _dir) = open_page(&group_with_members("0.5", ""), "");
    let pin_was = pin(doc(&s), 11);
    let r = run(&mut s, "shape.size", json!({"ids": [11], "width": 2.0}));
    assert_eq!(r["sized"], 1);
    let d = doc(&s);
    assert!(near(num(d, 11, "Width"), 2.0) && near(num(d, 11, "Height"), 1.0));
    assert!(near_p(pin(d, 11), pin_was), "the pin, in the group's frame, stays");
    assert!(near(num(d, 11, "LocPinX"), 1.0), "the pin stays at the middle");
    // The group itself is sized, as one shape, and its members follow.
    let r = run(&mut s, "shape.size", json!({"ids": [10], "width": 2.0}));
    assert_eq!(r["sized"], 1);
    assert!(near(num(doc(&s), 10, "Width"), 2.0));
    // A line in the group is refused as a line.
    let e = error_of(&mut s, "shape.size", &json!({"ids": [12], "width": 2.0}));
    assert!(e.to_string().contains("sized by its ends"), "{e}");
}

#[test]
fn a_size_is_one_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.size", json!({"ids": [a, b], "width": 3.0}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
}

// ---- shape.set_end and shape.set_length ----------------------------------------------------------------------------------

pub(crate) fn ends(d: &Document, id: u32) -> (P, P) {
    ((num(d, id, "BeginX"), num(d, id, "BeginY")), (num(d, id, "EndX"), num(d, id, "EndY")))
}

#[test]
fn an_end_of_a_connector_is_put_at_a_place_and_the_width_the_angle_and_the_route_follow() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 3.0}})));
    let r = run(&mut s, "shape.set_end", json!({"id": c, "end": "end", "x": 6.0, "y": 5.0}));
    assert_eq!(r, json!({"page": 1, "id": c, "changed": true}));
    let d = doc(&s);
    assert_eq!(ends(d, c), ((2.0, 3.0), (6.0, 5.0)));
    assert!(near(num(d, c, "Width"), 20.0_f64.sqrt()) && near(num(d, c, "Angle"), 0.5_f64.atan()), "{} {}", num(d, c, "Width"), num(d, c, "Angle"));
    assert!(near_p(pin(d, c), (4.0, 4.0)), "the pin is midway between the ends");
    assert!(route(d, c).last().is_some_and(|p| near_p(*p, (6.0, 5.0))), "{:?}", route(d, c));
    // One coordinate: the other keeps its place.
    run(&mut s, "shape.set_end", json!({"id": c, "end": "begin", "x": 1.0}));
    assert_eq!(ends(doc(&s), c), ((1.0, 3.0), (6.0, 5.0)));
    run(&mut s, "shape.set_end", json!({"id": c, "end": "begin", "y": 0.5}));
    assert_eq!(ends(doc(&s), c), ((1.0, 0.5), (6.0, 5.0)));
    // The place it has: nothing to do, no step.
    let before = snapshot(&s);
    let r = run(&mut s, "shape.set_end", json!({"id": c, "end": "end", "x": 6.0, "y": 5.0}));
    assert_eq!(r["changed"], false);
    assert_unchanged(&s, &before, "the place the end has");
    // One undo step for one end.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(ends(doc(&s), c), ((1.0, 3.0), (6.0, 5.0)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_end_that_is_glued_is_refused_in_words_and_the_other_end_of_the_connector_is_not() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 8.0, 2.0);
    let both = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let half = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": 5.0, "y": 8.0}})));
    let before = snapshot(&s);
    for (id, end) in [(both, "begin"), (both, "end"), (half, "begin")] {
        let e = error_of(&mut s, "shape.set_end", &json!({"id": id, "end": end, "x": 1.0, "y": 1.0}));
        assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("glued to a shape and follows it"), "{id} {end}: {e}");
        assert!(e.to_string().contains("connector.reconnect"), "it says what to do: {e}");
        assert_unchanged(&s, &before, &format!("{id} {end}"));
    }
    // The free end of the half-glued connector goes where it is told and the glue at the other stays.
    run(&mut s, "shape.set_end", json!({"id": half, "end": "end", "x": 5.0, "y": 6.0}));
    let d = doc(&s);
    assert!(near_p(ends(d, half).1, (5.0, 6.0)));
    assert!(d.page(1).unwrap().connects.iter().any(|k| k.from_sheet == half && k.to_sheet == a), "the begin is still glued");
    // `connector.reconnect` to a place takes the glue off; then the end is free for the command.
    run(&mut s, "connector.reconnect", json!({"id": half, "end": "begin", "target": {"x": 1.0, "y": 1.0}}));
    assert_eq!(run(&mut s, "shape.set_end", json!({"id": half, "end": "begin", "x": 2.0}))["changed"], true);
}

#[test]
fn an_end_of_a_line_a_file_holds_follows_by_the_formulas_and_its_geometry_is_not_rewritten_as_a_route() {
    let (mut s, _dir) = open_page(&file_line(30, (1.0, 1.0), (5.0, 1.0), ""), "");
    let geometry_before = shape_of(doc(&s), 30).sheet.sections.clone();
    run(&mut s, "shape.set_end", json!({"id": 30, "end": "end", "x": 4.0, "y": 5.0}));
    let d = doc(&s);
    assert_eq!(ends(d, 30), ((1.0, 1.0), (4.0, 5.0)));
    assert!(near(num(d, 30, "Width"), 5.0), "the width follows: {}", num(d, 30, "Width"));
    assert!(near(num(d, 30, "Angle"), (4.0_f64).atan2(3.0)), "{}", num(d, 30, "Angle"));
    assert!(near_p(pin(d, 30), (2.5, 3.0)));
    // The geometry's `LineTo` follows the width by its own formula, and its rows are the file's.
    assert_eq!(shape_of(d, 30).sheet.sections.len(), geometry_before.len());
    assert!(local(d, 30, "DiaCraftRoute").is_none());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_end_of_a_line_in_a_turned_group_is_put_at_a_place_of_the_page() {
    // Group 10 is turned a quarter turn: its frame's x runs up the page. Line 12 runs along x from (0.5, 1.5) to (3.5, 1.5) of the group's frame,
    // which is the page's (5 - 0.5, 5 + 1.5 - 2 ...): the page point of a frame point q is (5, 5) + R90 (q - (2, 1)).
    let (mut s, _dir) = open_page(&group_with_members("1.5707963267949", ""), "");
    let page_of = |q: P| (5.0 - (q.1 - 1.0), 5.0 + (q.0 - 2.0));
    let p = placement(&mut s, &[12]);
    let e = shape_entry(&p, 12);
    let (b, f) = (page_of((0.5, 1.5)), page_of((3.5, 1.5)));
    assert!(near(e["begin"]["x"].as_f64().unwrap(), b.0) && near(e["begin"]["y"].as_f64().unwrap(), b.1), "{e}");
    assert!(near(e["end"]["x"].as_f64().unwrap(), f.0) && near(e["end"]["y"].as_f64().unwrap(), f.1), "{e}");
    // Move the end to a place of the page: the cells get that place carried into the group's frame (the inverse of the turn).
    let target = (page_of((3.0, 1.0)).0, page_of((3.0, 1.0)).1);
    run(&mut s, "shape.set_end", json!({"id": 12, "end": "end", "x": target.0, "y": target.1}));
    let d = doc(&s);
    assert!(near_p(ends(d, 12).1, (3.0, 1.0)), "the end's cells, in the group's frame: {:?}", ends(d, 12));
    assert!(near_p(ends(d, 12).0, (0.5, 1.5)), "the begin was not touched");
    let after = placement(&mut s, &[12]);
    let e = shape_entry(&after, 12);
    assert!(
        near(e["end"]["x"].as_f64().unwrap(), target.0) && near(e["end"]["y"].as_f64().unwrap(), target.1),
        "and it reads back as the page place: {e}"
    );
}

#[test]
fn an_end_that_is_not_asked_for_properly_is_refused_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 3.0}})));
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"id": c, "end": "end"}), "give `x`, `y` or both"),
        (json!({"id": c, "end": "middle", "x": 1}), "`end` must be `begin` or `end`"),
        (json!({"id": c, "x": 1}), "missing `end`"),
        (json!({"id": a, "end": "end", "x": 1}), "not a line or a connector"),
        (json!({"id": 99, "end": "end", "x": 1}), "no shape 99"),
        (json!({"id": c, "end": "end", "x": 1e300}), "within"),
        (json!({"id": c, "end": "end", "x": "1"}), "must be a number"),
        (json!({"id": c, "end": "end", "x": 1, "page": 4}), "no page 4"),
    ] {
        let e = error_of(&mut s, "shape.set_end", &params);
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(words), "{params}: {e}");
        assert_unchanged(&s, &before, &params.to_string());
    }
}

#[test]
fn a_length_moves_the_end_along_the_line_and_keeps_the_begin_and_the_direction() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 4.0, "y": 5.0}})));
    let angle = num(doc(&s), c, "Angle");
    let r = run(&mut s, "shape.set_length", json!({"id": c, "length": 10.0}));
    assert_eq!(r, json!({"page": 1, "id": c, "changed": true}));
    let d = doc(&s);
    assert!(near_p(ends(d, c).0, (1.0, 1.0)), "the begin stays");
    assert!(near_p(ends(d, c).1, (7.0, 9.0)), "3-4-5 grown to 10: {:?}", ends(d, c));
    assert!(near(num(d, c, "Width"), 10.0) && near(num(d, c, "Angle"), angle), "the width follows and the direction is kept");
    // The length it has: nothing to do, no step.
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "shape.set_length", json!({"id": c, "length": 10.0}))["changed"], false);
    assert_unchanged(&s, &before, "the length it has");
    // Shorter, and undone in one step.
    run(&mut s, "shape.set_length", json!({"id": c, "length": 2.5}));
    assert!(near_p(ends(doc(&s), c).1, (2.5, 3.0)), "{:?}", ends(doc(&s), c));
    run(&mut s, "edit.undo", json!({}));
    assert!(near_p(ends(doc(&s), c).1, (7.0, 9.0)));
}

#[test]
fn a_length_moves_the_free_end_and_a_connector_with_both_ends_glued_is_refused() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 8.0, 2.0);
    let both = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.set_length", &json!({"id": both, "length": 3.0}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("both ends") && e.to_string().contains("glued"), "{e}");
    assert_unchanged(&s, &before, "both ends glued");
    // The begin glued: the end moves along the line.
    let begin_glued = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 0}, "to": {"x": 2.0, "y": 8.0}})));
    let (begin, finish) = ends(doc(&s), begin_glued);
    assert!(near(begin.0, finish.0), "the connector runs straight up");
    run(&mut s, "shape.set_length", json!({"id": begin_glued, "length": 4.0}));
    let d = doc(&s);
    assert!(near_p(ends(d, begin_glued).0, begin) && near_p(ends(d, begin_glued).1, (begin.0, begin.1 + 4.0)), "{:?}", ends(d, begin_glued));
    assert!(d.page(1).unwrap().connects.iter().any(|k| k.from_sheet == begin_glued && k.to_sheet == a), "still glued");
    // The end glued and the begin free: the begin moves and the glued end stays.
    let end_glued = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 8.0, "y": 8.0}, "to": {"id": b, "point": 2}})));
    let (begin, finish) = ends(doc(&s), end_glued);
    run(&mut s, "shape.set_length", json!({"id": end_glued, "length": 1.0}));
    let d = doc(&s);
    let unit = (
        (finish.0 - begin.0) / (finish.0 - begin.0).hypot(finish.1 - begin.1),
        (finish.1 - begin.1) / (finish.0 - begin.0).hypot(finish.1 - begin.1),
    );
    assert!(near_p(ends(d, end_glued).1, finish), "the glued end stays");
    assert!(
        near_p(ends(d, end_glued).0, (finish.0 - unit.0, finish.1 - unit.1)),
        "the begin is one inch from it, on the same line: {:?}",
        ends(d, end_glued)
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_line_of_no_length_grows_along_the_angle_it_holds() {
    // A line whose ends are one place has no direction by its ends; its `Angle` cell is what is left of one.
    let (mut s, _dir) =
        open_page(&file_line(30, (2.0, 2.0), (2.0, 2.0), "").replace(r#"V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)""#, r#"V="0.5" F="0.5""#), "");
    run(&mut s, "shape.set_length", json!({"id": 30, "length": 2.0}));
    let d = doc(&s);
    assert!(near_p(ends(d, 30).1, (2.0 + 2.0 * 0.5_f64.cos(), 2.0 + 2.0 * 0.5_f64.sin())), "{:?}", ends(d, 30));
    assert!(near_p(ends(d, 30).0, (2.0, 2.0)));
}

#[test]
fn a_length_that_is_not_one_is_refused_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 3.0}, "to": {"x": 6.0, "y": 3.0}})));
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"id": c, "length": 0}), "more than 0"),
        (json!({"id": c, "length": -3}), "more than 0"),
        (json!({"id": c, "length": 1e300}), "more than 0"),
        (json!({"id": c}), "missing `length`"),
        (json!({"id": a, "length": 3}), "not a line or a connector"),
        (json!({"id": 99, "length": 3}), "no shape 99"),
    ] {
        let e = error_of(&mut s, "shape.set_length", &params);
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(words), "{params}: {e}");
        assert_unchanged(&s, &before, &params.to_string());
    }
}

#[test]
fn a_length_and_an_end_on_a_page_with_a_drawing_scale_are_in_paper_inches() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 2.0, "y": 1.0}})));
    assert!(near_p(ends(doc(&s), c).1, (96.0, 48.0)), "{:?}", ends(doc(&s), c));
    run(&mut s, "shape.set_length", json!({"id": c, "length": 2.0}));
    assert!(near_p(ends(doc(&s), c).1, (48.0 + 96.0, 48.0)), "{:?}", ends(doc(&s), c));
    run(&mut s, "shape.set_end", json!({"id": c, "end": "begin", "y": 0.5}));
    assert!(near_p(ends(doc(&s), c).0, (48.0, 24.0)), "{:?}", ends(doc(&s), c));
    let e = shape_entry(&placement(&mut s, &[c]), c).clone();
    assert!(near(e["length"].as_f64().unwrap(), (2.0_f64).hypot(0.5)), "{e}");
}

// ---- shape.placement -----------------------------------------------------------------------------------------------------------

#[test]
fn the_placement_of_a_shape_is_what_the_fields_show_in_the_order_the_shapes_are_named() {
    let (mut s, _dir) =
        open_page(&format!("{}{}", file_box(20, 4.0, 3.0, &format!("{THIRTY}<Cell N=\"FlipX\" V=\"1\"/>")), group_with_members("0", "")), "");
    let a = drop_shape(&mut s, "rectangle", 2.0, 7.0);
    let before = snapshot(&s);
    let r = placement(&mut s, &[a, 20, 20, 10]);
    assert_unchanged(&s, &before, "a query changes nothing");
    assert_eq!(s.journal().len(), before.journal.len(), "and is not journaled");
    assert_eq!(r["page"], 1);
    assert!(near(r["units_per_inch"].as_f64().unwrap(), 1.0));
    let ids: Vec<u64> = r["shapes"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [u64::from(a), 20, 10], "the order named, each once");
    let e = shape_entry(&r, a);
    assert_eq!((e["one_d"].clone(), e["group"].clone(), e["pin_at"].clone()), (json!(false), json!(false), json!("center")));
    assert!(near(e["x"].as_f64().unwrap(), 2.0) && near(e["y"].as_f64().unwrap(), 7.0) && near(e["width"].as_f64().unwrap(), 1.5), "{e}");
    assert_eq!(e["angle"].as_f64(), Some(0.0));
    assert!(e.get("begin").is_none() && e.get("length").is_none(), "a shape that is not a line has no ends");
    // The file box: its pin a quarter in is no place, turned 30 degrees, flipped.
    let e = shape_entry(&r, 20);
    assert_eq!(e["pin_at"], "custom");
    assert!(near(e["angle"].as_f64().unwrap(), 30.0) && e["flip_x"] == true && e["flip_y"] == false, "{e}");
    assert!(near(e["pin"]["x"].as_f64().unwrap(), 0.5) && near(e["pin"]["y"].as_f64().unwrap(), 0.25), "{e}");
    assert!(near(e["x"].as_f64().unwrap(), 4.0) && near(e["y"].as_f64().unwrap(), 3.0));
    // The group holds members; its member does not.
    assert_eq!(shape_entry(&r, 10)["group"], true);
    assert_eq!(shape_entry(&placement(&mut s, &[11]), 11)["group"], false);
}

#[test]
fn the_placement_of_a_group_and_of_a_member_of_it_is_on_the_page() {
    let (mut s, _dir) = open_page(&group_with_members("1.5707963267949", ""), "");
    let r = placement(&mut s, &[10, 11]);
    let g = shape_entry(&r, 10);
    assert_eq!(g["group"], true);
    assert!(near(g["x"].as_f64().unwrap(), 5.0) && near(g["y"].as_f64().unwrap(), 5.0) && near(g["angle"].as_f64().unwrap(), 90.0), "{g}");
    // The member's pin is at (1, 1) of the group's frame: (5 - (1 - 1), 5 + (1 - 2)) on the page.
    let m = shape_entry(&r, 11);
    assert!(near(m["x"].as_f64().unwrap(), 5.0) && near(m["y"].as_f64().unwrap(), 4.0), "{m}");
    assert!(near(m["angle"].as_f64().unwrap(), 0.0), "its own angle, as `shape.rotate` sets it");
}

#[test]
fn the_placement_of_a_connector_is_its_ends_what_each_is_glued_to_and_its_length_and_direction() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": 6.0, "y": 8.0}})));
    let r = placement(&mut s, &[c]);
    let e = shape_entry(&r, c);
    assert_eq!(e["one_d"], true);
    assert_eq!(e["pin_at"], Json::Null, "a line has no pin position");
    assert_eq!(e["begin"]["glued"], json!({"id": a, "point": 3}));
    assert_eq!(e["end"]["glued"], Json::Null);
    let (begin, finish) = ends(doc(&s), c);
    assert!(near(e["begin"]["x"].as_f64().unwrap(), begin.0) && near(e["begin"]["y"].as_f64().unwrap(), begin.1), "{e}");
    assert!(near(e["end"]["x"].as_f64().unwrap(), 6.0) && near(e["end"]["y"].as_f64().unwrap(), 8.0), "{e}");
    assert!(near(e["length"].as_f64().unwrap(), (finish.0 - begin.0).hypot(finish.1 - begin.1)), "{e}");
    assert!(near(e["angle"].as_f64().unwrap(), (finish.1 - begin.1).atan2(finish.0 - begin.0).to_degrees()), "{e}");
    // The angle reads back what `shape.rotate` set, as 0 up to 360: a quarter turn the other way is 270.
    let free = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 5.0}, "to": {"x": 6.0, "y": 5.0}})));
    run(&mut s, "shape.rotate", json!({"ids": [free], "angle": 90}));
    assert!(near(shape_entry(&placement(&mut s, &[free]), free)["angle"].as_f64().unwrap(), 90.0));
    run(&mut s, "shape.rotate", json!({"ids": [free], "angle": -90}));
    assert!(near(shape_entry(&placement(&mut s, &[free]), free)["angle"].as_f64().unwrap(), 270.0));
    // A connector glued to the whole of a shape has no point.
    let (mut t, _dir) = open_page(
        &format!("{}{}", file_box(10, 4.0, 3.0, ""), file_line(20, (6.0, 3.0), (9.0, 3.0), "")),
        r#"<Connect FromSheet="20" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="PinX" ToPart="3"/>"#,
    );
    assert_eq!(shape_entry(&placement(&mut t, &[20]), 20)["begin"]["glued"], json!({"id": 10, "point": Json::Null}));
}

#[test]
fn the_placement_on_a_page_with_a_drawing_scale_is_in_paper_inches_with_the_factor() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    let r = placement(&mut s, &[a]);
    assert!(near(r["units_per_inch"].as_f64().unwrap(), 48.0));
    let e = shape_entry(&r, a);
    assert!(near(e["x"].as_f64().unwrap(), 4.0) && near(e["y"].as_f64().unwrap(), 3.0), "{e}");
    assert!(near(e["width"].as_f64().unwrap() * 48.0, num(doc(&s), a, "Width")), "{e}");
}

#[test]
fn the_placement_names_the_shape_for_the_title_and_cuts_a_name_a_file_made_very_long() {
    let long = "N".repeat(5_000);
    let named = shape_with(31, r#" Name="Process step" NameU="Process.step""#, r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#);
    let universal = shape_with(32, r#" NameU="OnlyUniversal""#, r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#);
    let nameless = shape(33, r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#);
    let huge = shape_with(34, &format!(r#" Name="{long}""#), r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#);
    let (mut s, _dir) = open_page(&format!("{named}{universal}{nameless}{huge}"), "");
    let dropped = drop_shape(&mut s, "ellipse", 6.0, 6.0);
    let r = placement(&mut s, &[31, 32, 33, 34, dropped]);
    let name = |id: u32| shape_entry(&r, id)["name"].as_str().unwrap().to_owned();
    assert_eq!(name(31), "Process step");
    assert_eq!(name(32), "OnlyUniversal", "NameU when there is no Name");
    assert_eq!(name(33), "Sheet.33", "NAME() of a shape with none");
    assert_eq!(name(34).chars().count(), 256, "cut");
    assert_eq!(name(dropped), format!("Ellipse.{dropped}"));
}

#[test]
fn the_placement_of_shapes_that_are_not_there_is_refused_and_the_page_may_be_named() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    run(&mut s, "page.add", json!({"name": "Second"}));
    let e = error_of(&mut s, "shape.placement", &json!({"ids": [a, 99]}));
    assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("no shape 99"), "{e}");
    let e = error_of(&mut s, "shape.placement", &json!({"ids": [a], "page": 2}));
    assert!(e.to_string().contains("no shape"), "shape {a} is not on page 2: {e}");
    assert_eq!(placement(&mut s, &[a])["page"], 1);
    assert_eq!(run(&mut s, "shape.placement", json!({"ids": [a], "page": 1}))["page"], 1);
}

// ---- random sequences ----------------------------------------------------------------------------------------------------------

/// A fixed-seed xorshift, so a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn coord(&mut self) -> f64 {
        (self.below(1_000) as f64) / 100.0
    }

    /// A shape id: one that is there, most of the time, and a number that may not be, some of it.
    fn id(&mut self, live: &[u32]) -> u64 {
        if live.is_empty() || self.below(7) == 0 { 1 + self.below(30) } else { u64::from(live[self.below(live.len() as u64) as usize]) }
    }
}

/// One of the Size & Position commands among the drawing's own, on shapes that may not be there and with parameters that may be wrong now and then.
fn random_command(r: &mut Rng, live: &[u32], lines: &[u32]) -> (&'static str, Json) {
    let ids = json!([r.id(live), r.id(live), r.id(live)]);
    let which = if r.below(2) == 0 { "begin" } else { "end" };
    match r.below(16) {
        0..=2 => {
            let master = ["rectangle", "ellipse", "hexagon", "circle"][r.below(4) as usize];
            ("shape.drop", json!({"master": master, "page": 1, "x": r.coord(), "y": r.coord()}))
        }
        3 => ("shape.move", json!({"ids": ids, "dx": r.coord() - 5.0, "dy": r.coord() - 5.0})),
        4 | 5 => {
            // Glued at both ends, glued at one, or free at both: the commands that type into an end need a free one.
            let from = if r.below(2) == 0 { json!({"x": r.coord(), "y": r.coord()}) } else { json!({"id": r.id(live), "point": r.below(5)}) };
            let to = if r.below(2) == 0 { json!({"x": r.coord(), "y": r.coord()}) } else { json!({"id": r.id(live)}) };
            ("connector.add", json!({"from": from, "to": to}))
        }
        6 => ("shape.rotate", json!({"ids": ids, "by": (r.below(721) as f64) - 360.0})),
        7 => ("shape.flip", json!({"ids": ids, "axis": if r.below(2) == 0 { "horizontal" } else { "vertical" }})),
        8 | 9 => {
            let at = ["top-left", "top-center", "top-right", "middle-left", "center", "middle-right", "bottom-left", "bottom-center", "bottom-right"]
                [r.below(9) as usize];
            (
                "shape.pin",
                if r.below(4) == 0 { json!({"ids": ids, "at": {"x": r.coord() / 5.0, "y": r.coord() / 5.0}}) } else { json!({"ids": ids, "at": at}) },
            )
        }
        10 | 11 => ("shape.size", json!({"ids": ids, "width": 0.2 + r.coord() / 3.0, "height": 0.2 + r.coord() / 3.0})),
        12 | 13 => ("shape.set_end", json!({"id": r.id(lines), "end": which, "x": r.coord(), "y": r.coord()})),
        14 => ("shape.set_length", json!({"id": r.id(lines), "length": 0.2 + r.coord() / 2.0})),
        _ => (if r.below(3) == 0 { "edit.undo" } else { "edit.redo" }, json!({})),
    }
}

/// Every glued end of the page is on its connection point.
fn glued_ends_are_on_their_points(d: &Document) -> Result<(), String> {
    for c in &d.page(1).ok_or("no page")?.connects {
        let index = c.to_part.and_then(|p| p.checked_sub(100)).ok_or("a Connect row with no ToPart")?;
        let point =
            connection_points(d, 1, c.to_sheet).into_iter().find(|p| p.index == index).ok_or("a Connect row names a point that is not there")?;
        let (x_cell, y_cell) = if c.from_cell.as_deref() == Some("BeginX") { ("BeginX", "BeginY") } else { ("EndX", "EndY") };
        let (x, y) = (num(d, c.from_sheet, x_cell), num(d, c.from_sheet, y_cell));
        if !((x - point.x).abs() < 1.0e-6 && (y - point.y).abs() < 1.0e-6) {
            return Err(format!("{x_cell} of {} is ({x}, {y}) and its point is ({}, {})", c.from_sheet, point.x, point.y));
        }
    }
    Ok(())
}

#[test]
fn a_few_hundred_random_pins_sizes_ends_and_lengths_leave_the_ends_on_their_points_and_undo_back_to_blank() {
    for seed in [0x9E37_79B9_7F4A_7C15_u64, 0xD1B5_4A32_D192_ED03, 0x2545_F491_4F6C_DD1D] {
        let mut r = Rng(seed);
        let mut s = blank_session();
        let blank = doc(&s).clone();
        let mut ran: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for step in 0..400 {
            let live: Vec<u32> = doc(&s).page(1).unwrap().shapes.iter().map(|shape| shape.id).collect();
            // The lines among them: the shapes that have all four ends.
            let lines: Vec<u32> =
                doc(&s).page(1).unwrap().shapes.iter().filter(|shape| shape.sheet.cell("BeginX").is_some()).map(|shape| shape.id).collect();
            let (id, params) = random_command(&mut r, &live, &lines);
            let before = s.document().cloned();
            let outcome = s.execute(id, &params);
            match &outcome {
                Err(EngineError::Internal { .. }) => panic!("seed {seed:#x} step {step}: {id} {params} hit a bug: {outcome:?}"),
                Err(_) => assert_eq!(s.document().cloned(), before, "seed {seed:#x} step {step}: {id} {params} failed but changed the document"),
                Ok(_) => *ran.entry(id).or_default() += 1,
            }
            assert_eq!(s.panics_caught(), 0, "seed {seed:#x} step {step}: {id} {params}");
            check_session(&s).unwrap_or_else(|e| panic!("seed {seed:#x} step {step}: {id} {params}: {e}"));
            glued_ends_are_on_their_points(doc(&s)).unwrap_or_else(|e| panic!("seed {seed:#x} step {step}: after {id} {params}: {e}"));
        }
        eprintln!("seed {seed:#x}: {ran:?}");
        for kind in ["shape.pin", "shape.size", "shape.set_end", "shape.set_length"] {
            assert!(ran.get(kind).copied().unwrap_or(0) >= 5, "seed {seed:#x}: {kind} ran {:?} times: {ran:?}", ran.get(kind));
        }
        while s.execute("edit.undo", &json!({})).is_ok() {}
        assert_eq!(doc(&s), &blank, "seed {seed:#x}: undoing every step does not give the blank page back");
    }
}
