//! The text block on its own: `text.block` says where a shape's text area is, and `text.block_move`, `text.block_resize` and
//! `text.block_rotate` move, size and turn it apart from the shape.
//!
//! The file shapes are built in code from the specifications' XML: a box 2 by 1 at (4, 3) whose local pin is its middle, holding the seven cells of the
//! text block as a file writes them, and angles that are the radians a file holds (a quarter turn is `1.5707963267949`). The boxes the Text tool makes are
//! an inch by a quarter of an inch.

use diacraft_doc::{CellAddr, Document, Lookup, Owner};
use diacraft_engine::draw::shape_corners;
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value as Json, json};

use crate::edit::{doc, id_of, route};
use crate::edit_foreign::Dir;
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

type P = (f64, f64);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-8
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn degrees(d: f64) -> f64 {
    d.to_radians()
}

/// What shape `id` holds locally in a cell: its formula (if any) and its value text.
fn local(d: &Document, id: u32, cell: &str) -> Option<(Option<String>, Option<String>)> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    match shape.sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

fn value(d: &Document, id: u32, cell: &str) -> f64 {
    local(d, id, cell).and_then(|(_, v)| v).unwrap_or_else(|| panic!("shape {id} holds no {cell}")).parse().unwrap()
}

fn formula(d: &Document, id: u32, cell: &str) -> Option<String> {
    local(d, id, cell).and_then(|(f, _)| f)
}

/// What `text.block` says of a shape.
#[derive(Debug)]
struct Place {
    corners: [P; 4],
    pin: P,
    width: f64,
    height: f64,
    angle: f64,
    direction: f64,
}

fn pair(v: &Json) -> P {
    (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

fn place(s: &mut Session, id: u32) -> Place {
    let r = run(s, "text.block", json!({"id": id}));
    let c = &r["corners"];
    Place {
        corners: [pair(&c[0]), pair(&c[1]), pair(&c[2]), pair(&c[3])],
        pin: pair(&r["pin"]),
        width: r["width"].as_f64().unwrap(),
        height: r["height"].as_f64().unwrap(),
        angle: r["angle"].as_f64().unwrap(),
        direction: r["direction"].as_f64().unwrap(),
    }
}

fn assert_corners(what: &str, got: &[P; 4], want: [P; 4]) {
    assert!(got.iter().zip(&want).all(|(g, w)| near_p(*g, *w)), "{what}: the corners are {got:?}, want {want:?}");
}

/// Whether two angles in degrees are the same turn.
fn same_direction(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(360.0);
    d < 1.0e-6 || 360.0 - d < 1.0e-6
}

// ---- the shapes of files -----------------------------------------------------------------------------------------------------------

/// The pose of the box: 2 by 1, the pin at (4, 3) and the local pin at the middle.
const POSE: &str = r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#;

/// The seven cells of the text block as a file writes them for a block that is the whole shape.
const BLOCK: &str = r#"<Cell N="TxtPinX" V="1" F="Width*0.5"/><Cell N="TxtPinY" V="0.5" F="Height*0.5"/>
<Cell N="TxtLocPinX" V="1" F="TxtWidth*0.5"/><Cell N="TxtLocPinY" V="0.5" F="TxtHeight*0.5"/>
<Cell N="TxtWidth" V="2" F="Width*1"/><Cell N="TxtHeight" V="1" F="Height*1"/><Cell N="TxtAngle" V="0"/>"#;

/// Radians as a file holds them, thirteen places.
fn rad(degrees: f64) -> String {
    format!("{:.13}", degrees.to_radians())
}

/// A box with `extra` cells (`Angle`, flips) and the text block `block` (the seven cells, or `""` for a shape that has none, or some other).
fn file_box(id: u32, extra: &str, block: &str) -> String {
    shape(id, &format!("{POSE}{extra}{block}<Text>Hello</Text>"))
}

/// The sheet of an 11 by 8.5 inch page drawn one to one.
const PAGE: &str = r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;

/// A session that has opened a page holding `shapes`.
fn open_page(shapes: &str) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.path("in.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(shapes)).build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    (s, dir)
}

/// What a call that changed nothing leaves alone: the document, the history and the revision. (The journal records the call, so it is not compared.)
fn quiet(s: &Session) -> (Option<Document>, usize, usize, u64) {
    let history = s.history();
    (s.document().cloned(), history.map_or(0, |h| h.undo_len()), history.map_or(0, |h| h.redo_len()), s.revision())
}

fn text_box(s: &mut Session) -> u32 {
    id_of(&run(s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "Hello"})))
}

// ---- where the block is --------------------------------------------------------------------------------------------------------------

#[test]
fn the_block_of_a_text_box_is_the_box() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    let p = place(&mut s, id);
    assert_corners("the box", &p.corners, [(2.5, 3.875), (3.5, 3.875), (3.5, 4.125), (2.5, 4.125)]);
    assert!(near_p(p.pin, (3.0, 4.0)) && near(p.width, 1.0) && near(p.height, 0.25) && near(p.angle, 0.0) && near(p.direction, 0.0), "{p:?}");
    assert!(shape_corners(doc(&s), 1, id).unwrap().iter().zip(&p.corners).all(|(a, b)| near_p(*a, *b)));
}

#[test]
fn a_shape_with_no_text_cells_has_a_block_the_size_of_the_shape() {
    // 73,507 of the 105,102 shapes that show text have none of the seven cells, anywhere. The block is the shape.
    let (mut s, _dir) = open_page(&file_box(10, "", ""));
    let p = place(&mut s, 10);
    assert_corners("the shape", &p.corners, [(3.0, 2.5), (5.0, 2.5), (5.0, 3.5), (3.0, 3.5)]);
    assert!(near_p(p.pin, (4.0, 3.0)) && near(p.width, 2.0) && near(p.height, 1.0) && near(p.direction, 0.0), "{p:?}");
    // Turned a quarter turn: the block is the shape's box, turned.
    let quarter = format!(r#"<Cell N="Angle" V="{}"/>"#, rad(90.0));
    let (mut s, _dir) = open_page(&file_box(10, &quarter, ""));
    let p = place(&mut s, 10);
    assert_corners("the turned shape", &p.corners, [(4.5, 2.0), (4.5, 4.0), (3.5, 4.0), (3.5, 2.0)]);
    assert!(near(p.direction, 90.0), "{p:?}");
}

#[test]
fn a_file_with_part_of_the_set_keeps_what_it_has_and_the_rest_are_read_as_blocks_without_cells_are() {
    // Finds no partial set in the files, but a hand-made one is read: `TxtWidth` 1.5, and the rest as prints them (the pin at the
    // middle of the shape, the local pin at the middle of the block).
    let (mut s, _dir) = open_page(&file_box(10, "", r#"<Cell N="TxtWidth" V="1.5" F="Width*0.75"/>"#));
    let p = place(&mut s, 10);
    assert!(near(p.width, 1.5) && near(p.height, 1.0), "{p:?}");
    assert_corners("the block", &p.corners, [(3.25, 2.5), (4.75, 2.5), (4.75, 3.5), (3.25, 3.5)]);
    // A resize works on it and does not give it the rest of the set: it has some.
    run(&mut s, "text.block_resize", json!({"id": 10, "x": 6, "y": 2, "width": 2, "height": 0.5}));
    let p = place(&mut s, 10);
    assert_corners("resized", &p.corners, [(6.0, 2.0), (8.0, 2.0), (8.0, 2.5), (6.0, 2.5)]);
    let d = doc(&s);
    assert!(
        local(d, 10, "TxtPinX").is_some() && local(d, 10, "TxtAngle").is_none() && local(d, 10, "TxtLocPinX").is_none(),
        "only what was changed is written"
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_text_runs_by_the_angle_and_the_text_angle_added_or_taken_under_one_flip() {
    // A pure turn by Angle + TxtAngle with no flip or two, and by Angle - TxtAngle under one; the text is never mirrored.
    let cases: [(&str, f64, f64, f64); 8] = [
        ("", 30.0, 0.0, 30.0),
        ("", 30.0, 20.0, 50.0),
        ("", 30.0, -30.0, 0.0),
        (r#"<Cell N="FlipX" V="1"/>"#, 30.0, 0.0, 30.0),
        (r#"<Cell N="FlipX" V="1"/>"#, 30.0, 20.0, 10.0),
        (r#"<Cell N="FlipY" V="1"/>"#, 30.0, 20.0, 10.0),
        (r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#, 30.0, 20.0, 50.0),
        (r#"<Cell N="FlipX" V="1"/>"#, 30.0, 30.0, 0.0),
    ];
    for (flips, angle, text_angle, want) in cases {
        let extra = format!(r#"<Cell N="Angle" V="{}"/>{flips}"#, rad(angle));
        let block = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, &format!(r#"<Cell N="TxtAngle" V="{}"/>"#, rad(text_angle)));
        let (mut s, _dir) = open_page(&file_box(10, &extra, &block));
        let p = place(&mut s, 10);
        assert!(same_direction(p.direction, want), "{flips} Angle {angle} TxtAngle {text_angle}: the text runs {} degrees, want {want}", p.direction);
        assert!(same_direction(p.angle, text_angle), "{flips}: `angle` is the cell");
    }
}

#[test]
fn the_block_of_a_missing_shape_or_page_is_refused() {
    let mut s = blank_session();
    for params in [json!({"id": 9}), json!({"id": 9, "page": 1}), json!({"id": 1, "page": 7})] {
        let e = s.execute("text.block", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains("no shape") || bad_params_msg(&e).contains("page"), "{e}");
    }
    assert!(s.execute("text.block", &json!({})).is_err());
}

// ---- move --------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_block_of_a_text_box_moves_by_a_distance_on_the_page_and_the_box_stays() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    let box_before = shape_corners(doc(&s), 1, id).unwrap();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "text.block_move", json!({"id": id, "dx": 1, "dy": 0.5}));
    assert_eq!(r, json!({"page": 1, "id": id, "changed": true}));
    let p = place(&mut s, id);
    assert_corners("moved", &p.corners, [(3.5, 4.375), (4.5, 4.375), (4.5, 4.625), (3.5, 4.625)]);
    assert!(near_p(p.pin, (4.0, 4.5)), "{p:?}");
    assert_eq!(shape_corners(doc(&s), 1, id).unwrap(), box_before, "the shape is where it was");
    let d = doc(&s);
    // The pin is written in the shape's frame, as a value of its own; the rest of the set keeps its formulas.
    assert_eq!(local(d, id, "TxtPinX"), Some((None, Some("1.5".to_owned()))));
    assert_eq!(local(d, id, "TxtPinY"), Some((None, Some("0.625".to_owned()))));
    assert_eq!(formula(d, id, "TxtWidth").as_deref(), Some("Width*1"));
    assert_eq!(formula(d, id, "TxtLocPinX").as_deref(), Some("TxtWidth*0.5"));
    for cell in ["PinX", "PinY", "Width", "Height"] {
        assert!(local(d, id, cell).is_some());
    }
    assert_eq!(value(d, id, "PinX"), 3.0);
    // One undo step, and the block keeps its offset when the shape moves.
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 2, "dy": 0}));
    let moved = place(&mut s, id);
    assert_corners("with the shape", &moved.corners, [(5.5, 4.375), (6.5, 4.375), (6.5, 4.625), (5.5, 4.625)]);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(place(&mut s, id).corners, box_before, "undone, the block is the box again");
}

#[test]
fn a_distance_on_the_page_is_carried_into_a_turned_and_into_a_flipped_shape() {
    // A quarter turn: the shape's x axis points up the page, so a step right on the page is a step down the shape's y axis.
    let quarter = format!(r#"<Cell N="Angle" V="{}"/>"#, rad(90.0));
    let (mut s, _dir) = open_page(&file_box(10, &quarter, BLOCK));
    let before = place(&mut s, 10);
    run(&mut s, "text.block_move", json!({"id": 10, "dx": 1, "dy": 0}));
    let d = doc(&s);
    assert!(near(value(d, 10, "TxtPinY"), -0.5) && near(value(d, 10, "TxtPinX"), 1.0), "{} {}", value(d, 10, "TxtPinX"), value(d, 10, "TxtPinY"));
    assert_eq!(formula(d, 10, "TxtPinX").as_deref(), Some("Width*0.5"), "the coordinate that did not change keeps its formula");
    let after = place(&mut s, 10);
    for (a, b) in before.corners.iter().zip(&after.corners) {
        assert!(near_p((a.0 + 1.0, a.1), *b), "{a:?} to {b:?}");
    }
    // FlipX: the shape's x axis points left on the page.
    let (mut s, _dir) = open_page(&file_box(11, r#"<Cell N="FlipX" V="1"/>"#, BLOCK));
    let before = place(&mut s, 11);
    run(&mut s, "text.block_move", json!({"id": 11, "dx": 1, "dy": 0.25}));
    let d = doc(&s);
    assert!(near(value(d, 11, "TxtPinX"), 0.0) && near(value(d, 11, "TxtPinY"), 0.75), "{} {}", value(d, 11, "TxtPinX"), value(d, 11, "TxtPinY"));
    let after = place(&mut s, 11);
    for (a, b) in before.corners.iter().zip(&after.corners) {
        assert!(near_p((a.0 + 1.0, a.1 + 0.25), *b), "{a:?} to {b:?}");
    }
}

#[test]
fn the_block_of_a_member_of_a_turned_group_moves_by_a_distance_on_the_page() {
    let member = shape(
        11,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Text>in</Text>"#,
    );
    let angle = rad(90.0);
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/>
<Cell N="Angle" V="{angle}"/><Shapes>{member}</Shapes>"#
        ),
    );
    let (mut s, _dir) = open_page(&group);
    let before = place(&mut s, 11);
    run(&mut s, "text.block_move", json!({"id": 11, "dx": 0, "dy": 1}));
    let after = place(&mut s, 11);
    for (a, b) in before.corners.iter().zip(&after.corners) {
        assert!(near_p((a.0, a.1 + 1.0), *b), "{a:?} to {b:?}");
    }
    // In the group's frame, which is turned a quarter turn, a step up the page is a step along the group's x axis: the pin was at the middle of the
    // member (0.5, 0.5).
    let d = doc(&s);
    assert!(near(value(d, 11, "TxtPinX"), 1.5) && near(value(d, 11, "TxtPinY"), 0.5), "{} {}", value(d, 11, "TxtPinX"), value(d, 11, "TxtPinY"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_no_text_cells_is_given_all_seven_before_the_first_is_changed() {
    // The seven travel as a set.
    let (mut s, _dir) = open_page(&file_box(10, "", ""));
    for cell in ["TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtWidth", "TxtHeight", "TxtAngle"] {
        assert!(local(doc(&s), 10, cell).is_none(), "{cell}: none to begin with");
    }
    run(&mut s, "text.block_move", json!({"id": 10, "dx": 0.5, "dy": 0}));
    let d = doc(&s);
    let want: [(&str, Option<&str>, &str); 7] = [
        ("TxtPinX", None, "1.5"),
        ("TxtPinY", Some("Height*0.5"), "0.5"),
        ("TxtLocPinX", Some("TxtWidth*0.5"), "1"),
        ("TxtLocPinY", Some("TxtHeight*0.5"), "0.5"),
        ("TxtWidth", Some("Width*1"), "2"),
        ("TxtHeight", Some("Height*1"), "1"),
        ("TxtAngle", None, "0"),
    ];
    for (cell, f, v) in want {
        assert_eq!(local(d, 10, cell), Some((f.map(str::to_owned), Some(v.to_owned()))), "{cell}");
    }
    // A shape whose master holds the set is not given a local copy of it: the instance holds what it changed.
    let mut s = blank_session();
    let rect = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 3})));
    run(&mut s, "text.block_move", json!({"id": rect, "dx": 0, "dy": 0.5}));
    let d = doc(&s);
    assert!(local(d, rect, "TxtPinY").is_some());
    for cell in ["TxtWidth", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtAngle", "TxtPinX"] {
        assert!(local(d, rect, cell).is_none(), "{cell} stays the master's");
    }
}

#[test]
fn a_move_of_nothing_is_no_undo_step() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    let before = quiet(&s);
    let r = run(&mut s, "text.block_move", json!({"id": id, "dx": 0, "dy": 0}));
    assert_eq!(r, json!({"page": 1, "id": id, "changed": false}));
    assert_eq!(quiet(&s), before);
}

#[test]
fn a_guarded_cell_is_not_moved_and_nothing_changes() {
    let guarded = BLOCK.replace(r#"V="1" F="Width*0.5""#, r#"V="1" F="GUARD(Width*0.5)""#);
    let (mut s, _dir) = open_page(&file_box(10, "", &guarded));
    let before = snapshot(&s);
    let e = s.execute("text.block_move", &json!({"id": 10, "dx": 1, "dy": 0})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("GUARD"), "{e}");
    assert_eq!(snapshot(&s), before);
    // The other coordinate is not guarded.
    run(&mut s, "text.block_move", json!({"id": 10, "dx": 0, "dy": 1}));
    assert!(near(value(doc(&s), 10, "TxtPinY"), 1.5));
}

#[test]
fn the_page_scale_is_applied_to_a_block_move() {
    use diacraft_doc::{CellEdit, SheetRef, Value, WriteOrigin};
    let mut s = blank_session();
    s.edit("scale", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct("DrawingScale"), CellEdit::Value(Value::from_text("2")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let id = text_box(&mut s);
    run(&mut s, "text.block_move", json!({"id": id, "dx": 1, "dy": 0}));
    // The box is 2 by 0.5 drawing units, its pin at (1, 0.25); one paper inch is two units.
    assert!(near(value(doc(&s), id, "TxtPinX"), 3.0), "{}", value(doc(&s), id, "TxtPinX"));
    let p = place(&mut s, id);
    assert!(near_p(p.pin, (4.0, 4.0)) && near(p.width, 1.0), "{p:?}: the answers are paper inches");
}

// ---- resize ------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_block_of_a_text_box_is_given_a_size_and_a_corner_and_the_box_stays() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    let box_before = shape_corners(doc(&s), 1, id).unwrap();
    let r = run(&mut s, "text.block_resize", json!({"id": id, "x": 2, "y": 3, "width": 2, "height": 1}));
    assert_eq!(r, json!({"page": 1, "id": id, "changed": true}));
    let p = place(&mut s, id);
    assert_corners("resized", &p.corners, [(2.0, 3.0), (4.0, 3.0), (4.0, 4.0), (2.0, 4.0)]);
    assert!(near_p(p.pin, (3.0, 3.5)), "the pin is the middle of the block, as the local pin is: {p:?}");
    assert_eq!(shape_corners(doc(&s), 1, id).unwrap(), box_before);
    let d = doc(&s);
    assert_eq!(local(d, id, "TxtWidth"), Some((None, Some("2".to_owned()))));
    assert_eq!(local(d, id, "TxtHeight"), Some((None, Some("1".to_owned()))));
    // The local pin is worked out again for the new size, and the pin is the corner carried to it.
    assert_eq!(local(d, id, "TxtLocPinX"), Some((Some("TxtWidth*0.5".to_owned()), Some("1".to_owned()))));
    assert_eq!(local(d, id, "TxtLocPinY"), Some((Some("TxtHeight*0.5".to_owned()), Some("0.5".to_owned()))));
    assert!(near(value(d, id, "TxtPinX"), 0.5) && near(value(d, id, "TxtPinY"), -0.375));
    assert!(near(value(d, id, "Width"), 1.0) && near(value(d, id, "Height"), 0.25), "the shape's size is not written");
}

#[test]
fn a_block_that_is_turned_upright_in_a_turned_shape_is_sized_along_its_own_axes() {
    // The shape is turned a quarter turn and its text is kept upright by `-Angle`, as finds files do: the block's axes are the page's.
    let extra = format!(r#"<Cell N="Angle" V="{}"/>"#, rad(90.0));
    let block = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, &format!(r#"<Cell N="TxtAngle" V="{}" F="-Angle"/>"#, rad(-90.0)));
    let (mut s, _dir) = open_page(&file_box(10, &extra, &block));
    assert!(near(place(&mut s, 10).direction, 0.0));
    run(&mut s, "text.block_resize", json!({"id": 10, "x": 1, "y": 1, "width": 1.5, "height": 0.5}));
    let p = place(&mut s, 10);
    assert_corners("upright", &p.corners, [(1.0, 1.0), (2.5, 1.0), (2.5, 1.5), (1.0, 1.5)]);
    assert_eq!(formula(doc(&s), 10, "TxtAngle").as_deref(), Some("-Angle"), "the angle is not written by a resize");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_resize_puts_the_corner_where_it_is_asked_for_every_pose_and_every_text_angle() {
    // The corner at (0, 0) of the block lands at (6, 2), the sides are the size asked for along the block's own axes, and the pin is the middle.
    let mut done = 0;
    for angle in [0.0, 30.0, 90.0, -45.0, 180.0] {
        for flips in ["", r#"<Cell N="FlipX" V="1"/>"#, r#"<Cell N="FlipY" V="1"/>"#, r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#] {
            for text_angle in [0.0, 20.0, -angle] {
                let extra = format!(r#"<Cell N="Angle" V="{}"/>{flips}"#, rad(angle));
                let block = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, &format!(r#"<Cell N="TxtAngle" V="{}"/>"#, rad(text_angle)));
                let (mut s, _dir) = open_page(&file_box(10, &extra, &block));
                let r = run(&mut s, "text.block_resize", json!({"id": 10, "x": 6, "y": 2, "width": 1.25, "height": 0.5}));
                assert_eq!(r["changed"], true);
                let p = place(&mut s, 10);
                let what = format!("Angle {angle} {flips} TxtAngle {text_angle}: {p:?}");
                assert!(near_p(p.corners[0], (6.0, 2.0)), "{what}");
                let side = |a: P, b: P| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                assert!(near(side(p.corners[0], p.corners[1]), 1.25) && near(side(p.corners[0], p.corners[3]), 0.5), "{what}");
                assert!(near_p(p.pin, ((p.corners[0].0 + p.corners[2].0) / 2.0, (p.corners[0].1 + p.corners[2].1) / 2.0)), "{what}");
                let along = (p.corners[1].1 - p.corners[0].1).atan2(p.corners[1].0 - p.corners[0].0).to_degrees();
                assert!(same_direction(along, p.direction), "{what}: the text runs along the first side");
                assert!(near(p.width, 1.25) && near(p.height, 0.5), "{what}");
                done += 1;
            }
        }
    }
    assert_eq!(done, 60);
}

#[test]
fn a_local_pin_that_is_not_the_middle_stays_and_the_corner_still_lands() {
    // The local pin is a value (a quarter of an inch in, a tenth up) and not a formula of the size, so it does not move with the size.
    let block = BLOCK.replace(r#"V="1" F="TxtWidth*0.5""#, r#"V="0.25""#).replace(r#"V="0.5" F="TxtHeight*0.5""#, r#"V="0.1""#);
    let (mut s, _dir) = open_page(&file_box(10, &format!(r#"<Cell N="Angle" V="{}"/>"#, rad(30.0)), &block));
    run(&mut s, "text.block_resize", json!({"id": 10, "x": 6, "y": 2, "width": 3, "height": 1}));
    let p = place(&mut s, 10);
    assert!(near_p(p.corners[0], (6.0, 2.0)), "{p:?}");
    let ex = ((p.corners[1].0 - p.corners[0].0) / 3.0, (p.corners[1].1 - p.corners[0].1) / 3.0);
    let ey = ((p.corners[3].0 - p.corners[0].0) / 1.0, (p.corners[3].1 - p.corners[0].1) / 1.0);
    assert!(near_p(p.pin, (6.0 + 0.25 * ex.0 + 0.1 * ey.0, 2.0 + 0.25 * ex.1 + 0.1 * ey.1)), "{p:?}");
    assert_eq!(local(doc(&s), 10, "TxtLocPinX").unwrap().1.as_deref(), Some("0.25"));
}

#[test]
fn a_resize_to_the_size_and_place_the_block_has_is_no_undo_step() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    // A block that is not where the shape's own corner is, and turned: the corner is not (0, 0) of the shape.
    run(&mut s, "text.block_move", json!({"id": id, "dx": 1, "dy": 0.5}));
    run(&mut s, "text.block_rotate", json!({"id": id, "by": 30}));
    let p = place(&mut s, id);
    let before = quiet(&s);
    let r = run(&mut s, "text.block_resize", json!({"id": id, "x": p.corners[0].0, "y": p.corners[0].1, "width": p.width, "height": p.height}));
    assert_eq!(r["changed"], false);
    assert_eq!(quiet(&s), before);
}

#[test]
fn a_block_that_cannot_be_sized_is_refused_and_changes_nothing() {
    let cases: Vec<(Json, &str)> = vec![
        (json!({"id": 1, "x": 2, "y": 3, "width": 0, "height": 1}), "more than 0"),
        (json!({"id": 1, "x": 2, "y": 3, "width": 1, "height": -1}), "more than 0"),
        (json!({"id": 1, "x": 2, "y": 3, "width": 1.0e9, "height": 1}), "within"),
        (json!({"id": 1, "x": -1.0e9, "y": 3, "width": 1, "height": 1}), "within"),
        (json!({"id": 1, "x": 2, "y": 3, "width": 1}), "missing `height`"),
        (json!({"id": 9, "x": 2, "y": 3, "width": 1, "height": 1}), "no shape 9"),
        (json!({"id": 1, "page": 9, "x": 2, "y": 3, "width": 1, "height": 1}), "page"),
        (json!({"id": 1, "x": "left", "y": 3, "width": 1, "height": 1}), "must be a number"),
    ];
    for (params, needle) in cases {
        let mut s = blank_session();
        text_box(&mut s);
        let before = snapshot(&s);
        let e = s.execute("text.block_resize", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(needle), "{params}: {e}");
        assert_eq!(snapshot(&s), before);
    }
}

// ---- rotate -------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_block_of_a_text_box_turns_about_its_pin() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    let box_before = shape_corners(doc(&s), 1, id).unwrap();
    let r = run(&mut s, "text.block_rotate", json!({"id": id, "by": 90}));
    assert_eq!(r, json!({"page": 1, "id": id, "changed": true}));
    let d = doc(&s);
    assert_eq!(local(d, id, "TxtAngle"), Some((None, Some("1.5707963267949".to_owned()))), "a quarter turn, exact");
    assert_eq!(local(d, id, "Angle").unwrap().1.as_deref(), Some("0"), "the shape is not turned");
    let p = place(&mut s, id);
    assert_corners("turned", &p.corners, [(3.125, 3.5), (3.125, 4.5), (2.875, 4.5), (2.875, 3.5)]);
    assert!(near_p(p.pin, (3.0, 4.0)) && near(p.direction, 90.0) && near(p.angle, 90.0), "{p:?}: the pin is where it was");
    assert_eq!(shape_corners(doc(&s), 1, id).unwrap(), box_before);
    // The cells that were formulas are formulas still.
    assert_eq!(formula(doc(&s), id, "TxtPinX").as_deref(), Some("Width*0.5"));
}

#[test]
fn an_angle_is_set_added_wrapped_and_brought_back_to_nothing() {
    let mut s = blank_session();
    let id = text_box(&mut s);
    run(&mut s, "text.block_rotate", json!({"id": id, "angle": 45}));
    assert!(near(value(doc(&s), id, "TxtAngle"), degrees(45.0)) || (value(doc(&s), id, "TxtAngle") - degrees(45.0)).abs() < 1e-12);
    run(&mut s, "text.block_rotate", json!({"id": id, "by": -45}));
    assert_eq!(local(doc(&s), id, "TxtAngle"), Some((None, Some("0".to_owned()))), "back to exactly nothing");
    // Past a half turn: -170 degrees, as a file holds an angle.
    run(&mut s, "text.block_rotate", json!({"id": id, "angle": 170}));
    run(&mut s, "text.block_rotate", json!({"id": id, "by": 20}));
    assert!((value(doc(&s), id, "TxtAngle") - degrees(-170.0)).abs() < 1e-12, "{}", value(doc(&s), id, "TxtAngle"));
    run(&mut s, "text.block_rotate", json!({"id": id, "by": 3600 + 170}));
    assert!((value(doc(&s), id, "TxtAngle") - degrees(0.0)).abs() < 1e-9, "whole turns are no turn: {}", value(doc(&s), id, "TxtAngle"));
    // An angle the cell has is no change, and no undo step.
    let before = quiet(&s);
    assert_eq!(run(&mut s, "text.block_rotate", json!({"id": id, "angle": 0}))["changed"], false);
    assert_eq!(run(&mut s, "text.block_rotate", json!({"id": id, "by": 360}))["changed"], false);
    assert_eq!(quiet(&s), before);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn text_turned_by_a_quarter_stands_on_end_when_it_is_drawn() {
    let mut s = blank_session();
    run(&mut s, "text.box", json!({"page": 1, "x": 5, "y": 4, "width": 2, "height": 0.5, "text": "Hello"}));
    let flat = ink_extent(&render(&s), 3.0, 3.0, 7.0, 5.0).unwrap();
    assert!(flat.2 - flat.0 > 2.0 * (flat.3 - flat.1), "the word is wider than it is tall: {flat:?}");
    run(&mut s, "text.block_rotate", json!({"id": 1, "by": 90}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let up = ink_extent(&r, 3.0, 3.0, 7.0, 5.0).unwrap();
    assert!(up.3 - up.1 > 2.0 * (up.2 - up.0), "and taller than it is wide when turned a quarter: {up:?}");
    // The word is still about the pin of the block, the middle of the box.
    assert!(((up.0 + up.2) / 2.0 - 5.0).abs() < 0.3 && ((up.1 + up.3) / 2.0 - 4.0).abs() < 0.3, "{up:?}");
    // The box itself did not turn.
    assert!(near_p(shape_corners(doc(&s), 1, 1).unwrap()[1], (6.0, 3.75)));
}

#[test]
fn under_a_flip_the_cell_turns_the_other_way_so_that_the_text_turns_as_asked() {
    // The net angle is Angle - TxtAngle under one flip. A turn of 90 on the page is -90 in the cell, and the text is not mirrored.
    for flips in [r#"<Cell N="FlipX" V="1"/>"#, r#"<Cell N="FlipY" V="1"/>"#] {
        let (mut s, _dir) = open_page(&file_box(10, flips, BLOCK));
        run(&mut s, "text.block_rotate", json!({"id": 10, "by": 90}));
        assert_eq!(local(doc(&s), 10, "TxtAngle"), Some((None, Some("-1.5707963267949".to_owned()))), "{flips}");
        assert!(same_direction(place(&mut s, 10).direction, 90.0), "{flips}");
    }
    // Two flips are a half turn and no mirror: the cell turns as the text does.
    let (mut s, _dir) = open_page(&file_box(10, r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#, BLOCK));
    run(&mut s, "text.block_rotate", json!({"id": 10, "by": 90}));
    assert_eq!(local(doc(&s), 10, "TxtAngle").unwrap().1.as_deref(), Some("1.5707963267949"));
    assert!(same_direction(place(&mut s, 10).direction, 90.0));
    // A member of a group that is flipped once is mirrored on the page, though it is not flipped itself.
    let member = shape(
        11,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Text>in</Text>"#,
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/>
<Cell N="FlipX" V="1"/><Shapes>{member}</Shapes>"#
        ),
    );
    let (mut s, _dir) = open_page(&group);
    let was = place(&mut s, 11).direction;
    run(&mut s, "text.block_rotate", json!({"id": 11, "by": 90}));
    assert_eq!(local(doc(&s), 11, "TxtAngle").unwrap().1.as_deref(), Some("-1.5707963267949"));
    assert!(same_direction(place(&mut s, 11).direction, was + 90.0), "turned a quarter on the page, counterclockwise, from {was}");
    // `angle` sets the cell and nothing else.
    run(&mut s, "text.block_rotate", json!({"id": 11, "angle": 30}));
    assert!((value(doc(&s), 11, "TxtAngle") - degrees(30.0)).abs() < 1e-12);
}

#[test]
fn a_formula_that_keeps_the_text_upright_is_replaced_by_the_turn_a_person_asks_for() {
    // The shape is turned 30 degrees and TxtAngle is `-Angle`, so the text is upright. Turning the text 90 degrees is a value of its own: 60 degrees.
    let extra = format!(r#"<Cell N="Angle" V="{}"/>"#, rad(30.0));
    let block = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, &format!(r#"<Cell N="TxtAngle" V="{}" F="-Angle"/>"#, rad(-30.0)));
    let (mut s, _dir) = open_page(&file_box(10, &extra, &block));
    assert!(same_direction(place(&mut s, 10).direction, 0.0));
    run(&mut s, "text.block_rotate", json!({"id": 10, "by": 90}));
    assert!((value(doc(&s), 10, "TxtAngle") - degrees(60.0)).abs() < 1e-12, "{}", value(doc(&s), 10, "TxtAngle"));
    assert_eq!(formula(doc(&s), 10, "TxtAngle"), None, "a write replaces a formula");
    assert!(same_direction(place(&mut s, 10).direction, 90.0));
    // The shape turning afterwards no longer takes the text with it upright.
    run(&mut s, "shape.rotate", json!({"ids": [10], "by": 10}));
    assert!(same_direction(place(&mut s, 10).direction, 100.0));
}

#[test]
fn a_guarded_angle_is_not_turned_and_nothing_changes() {
    let guarded = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, r#"<Cell N="TxtAngle" V="0" F="GUARD(0)"/>"#);
    let (mut s, _dir) = open_page(&file_box(10, "", &guarded));
    let before = snapshot(&s);
    let e = s.execute("text.block_rotate", &json!({"id": 10, "by": 90})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("GUARD"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_turn_that_cannot_be_made_is_refused_and_changes_nothing() {
    let cases: Vec<(Json, &str)> = vec![
        (json!({"id": 1, "by": 10, "angle": 10}), "not both"),
        (json!({"id": 1}), "give `angle`"),
        (json!({"id": 1, "by": 1.0e7}), "degrees"),
        (json!({"id": 1, "by": "ninety"}), "degrees"),
        (json!({"id": 9, "by": 10}), "no shape 9"),
        (json!({"by": 10}), "missing `id`"),
    ];
    for (params, needle) in cases {
        let mut s = blank_session();
        text_box(&mut s);
        let before = snapshot(&s);
        let e = s.execute("text.block_rotate", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(needle), "{params}: {e}");
        assert_eq!(snapshot(&s), before);
    }
}

// ---- connectors and labels ----------------------------------------------------------------------------------------------------------------

/// Two rectangles joined by a connector that says "yes" (places the label on the middle leg).
fn joined() -> (Session, u32) {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6, "y": 4}));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
    run(&mut s, "shape.set_text", json!({"id": c, "text": "yes"}));
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0, "dy": 0.5}));
    (s, c)
}

#[test]
fn the_label_of_a_connector_is_moved_and_turned_and_the_route_is_left_alone() {
    let (mut s, c) = joined();
    let (route_before, connects_before) = (route(doc(&s), c), doc(&s).page(1).unwrap().connects.clone());
    let label = place(&mut s, c);
    run(&mut s, "text.block_move", json!({"id": c, "dx": 0, "dy": 0.5}));
    let moved = place(&mut s, c);
    for (a, b) in label.corners.iter().zip(&moved.corners) {
        assert!(near_p((a.0, a.1 + 0.5), *b), "{a:?} to {b:?}");
    }
    run(&mut s, "text.block_rotate", json!({"id": c, "by": 90}));
    assert!(same_direction(place(&mut s, c).direction, label.direction + 90.0), "{}", place(&mut s, c).direction);
    assert_eq!(route(doc(&s), c), route_before, "the route is as it was");
    assert_eq!(doc(&s).page(1).unwrap().connects, connects_before, "and the glue");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_that_is_routed_again_places_its_label_again() {
    // Keeps working: the label is placed on the middle leg whenever the connector is routed, whatever the Text Block tool did to it.
    let (mut moved, c) = joined();
    let (mut control, c2) = joined();
    assert_eq!(c, c2);
    run(&mut moved, "text.block_move", json!({"id": c, "dx": 0, "dy": 0.5}));
    run(&mut moved, "text.block_rotate", json!({"id": c, "by": 45}));
    for s in [&mut moved, &mut control] {
        run(s, "shape.move", json!({"ids": [2], "dx": 1, "dy": 1.5}));
    }
    let (a, b) = (place(&mut moved, c), place(&mut control, c));
    assert!(near_p(a.pin, b.pin) && same_direction(a.direction, b.direction), "{a:?} {b:?}");
    assert_eq!(check_session(&moved), Ok(()));
}

// ---- what it costs -----------------------------------------------------------------------------------------------------------------------

#[test]
fn what_each_text_command_costs_on_a_page_of_500_shapes_and_300_connectors() {
    use crate::edit_cost::{COLUMNS, CONNECTORS, big_page, recalculation_of, steps_of_the_whole_page};
    let mut s = big_page();
    // The fifth shape of the sixth row has a connector on each side; the first connector carries a label.
    let one = 5 * COLUMNS + 4 + 1;
    let connector = u32::try_from(500 + 1).unwrap();
    run(&mut s, "shape.set_text", json!({"id": one, "text": "Label"}));
    let cases: Vec<(&str, Vec<(&str, Json)>)> = vec![
        ("make a text box", (0..7).map(|i| ("text.box", json!({"page": 1, "x": 5.0 + f64::from(i), "y": 0.4, "text": "Note"}))).collect()),
        ("look at a block", (0..7).map(|_| ("text.block", json!({"id": one}))).collect()),
        ("move a block", (0..7).map(|i| ("text.block_move", json!({"id": one, "dx": 0.1, "dy": if i % 2 == 0 { 0.1 } else { -0.1 }}))).collect()),
        (
            "resize a block",
            (0..7)
                .map(|i| ("text.block_resize", json!({"id": one, "x": 8.0, "y": 8.0, "width": 1.5 + 0.01 * f64::from(i), "height": 0.5})))
                .collect(),
        ),
        ("turn a block", (0..7).map(|i| ("text.block_rotate", json!({"id": one, "by": 15 + i}))).collect()),
        ("turn the label of a connector", (0..7).map(|i| ("text.block_rotate", json!({"id": connector, "by": 15 + i}))).collect()),
    ];
    // Counted, not timed (`diacraft_calc::work`): the commits each command makes and the steps they take, against the steps of the recalculation of
    // the whole page, which is what each would cost if it were not a recalculation of what changed.
    let whole = steps_of_the_whole_page(&s);
    for (what, steps) in cases {
        let mut worst = (0, 0);
        for (command, params) in steps {
            let (commits, taken) = recalculation_of(&mut s, command, &params);
            worst = (commits.max(worst.0), taken.max(worst.1));
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

// ---- text laid out in the block ----------------------------------------------------------------------------------------------------------------

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    Renderer::new().render(doc, 1, &view)
}

/// The box (left, bottom, right, top), in inches, of the dark pixels in a rectangle of the page; `None` if there are none.
fn ink_extent(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> Option<(f64, f64, f64, f64)> {
    let (width, height) = (r.width as usize, r.height as usize);
    let mut found: Option<(f64, f64, f64, f64)> = None;
    let mut y = bottom;
    while y < top {
        let mut x = left;
        while x < right {
            let col = ((x * PX_PER_INCH) as usize).min(width - 1);
            let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
            let at = (row * width + col) * 4;
            let p = &r.rgba_premul[at..at + 4];
            if p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600 {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((l, b, rr, t)) => (l.min(x), b.min(y), rr.max(x), t.max(y)),
                });
            }
            x += 1.0 / PX_PER_INCH;
        }
        y += 1.0 / PX_PER_INCH;
    }
    found
}

#[test]
fn text_wraps_to_the_width_of_its_block_and_not_the_width_of_its_shape() {
    let mut s = blank_session();
    let words = "one two three four five six";
    run(&mut s, "text.box", json!({"page": 1, "x": 5, "y": 4, "width": 3, "height": 0.5, "text": words}));
    let line = ink_extent(&render(&s), 3.0, 3.0, 7.0, 5.0).unwrap();
    assert!(line.3 - line.1 < 0.25, "one line in a block 3 inches wide: {line:?}");
    // The block is narrowed to 0.8 inch and made taller, apart from the box, which is 3 inches wide still.
    run(&mut s, "text.block_resize", json!({"id": 1, "x": 4.6, "y": 3.2, "width": 0.8, "height": 1.6}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let wrapped = ink_extent(&r, 3.0, 2.0, 7.0, 6.0).unwrap();
    assert!(wrapped.3 - wrapped.1 > 2.0 * (line.3 - line.1), "the words wrap to several lines: {wrapped:?} against {line:?}");
    assert!(wrapped.0 >= 4.6 - 0.01 && wrapped.2 <= 5.4 + 0.01, "inside the block: {wrapped:?}");
    assert!(((wrapped.1 + wrapped.3) / 2.0 - 4.0).abs() < 0.3, "in the middle of the block, which spans y 3.2 to 4.8: {wrapped:?}");
    assert!(near(value(doc(&s), 1, "Width"), 3.0), "the box is as it was");
    // Put the block back to the box and the line is one again.
    run(&mut s, "text.block_resize", json!({"id": 1, "x": 3.5, "y": 3.75, "width": 3, "height": 0.5}));
    let back = ink_extent(&render(&s), 3.0, 2.0, 7.0, 6.0).unwrap();
    assert!(back.3 - back.1 < 0.25, "{back:?}");
}

// ---- the groups above a shape ---------------------------------------------------------------------------------------------

/// A group 4 by 2 pinned at (5, 5) on its middle, holding one member: a box 2 by 1 whose middle is at (1, 0.5) of the group, showing `Hello`, with the
/// text block cells `block`. `group_cells` are the group's `Angle` and flips.
fn group_with_member(group_cells: &str, block: &str) -> String {
    let member = shape(
        11,
        &format!(
            r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{block}<Text>Hello</Text>"#
        ),
    );
    shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/>
{group_cells}<Shapes>{member}</Shapes>"#
        ),
    )
}

#[test]
fn the_block_of_a_member_of_a_flipped_group_is_where_and_which_way_the_renderer_draws_the_text() {
    // No flip above a shape mirrors its text, so the block's own frame is right-handed on the page whatever the groups above are: its first
    // corner is at the lower left of the box, the second to the right of it and the fourth above it, and the text runs to the right (0 degrees). The
    // member's box is at (4 to 6, 4 to 5) on the page before the group is flipped, and a flip about the group's middle (5, 5) carries it across.
    let cases = [
        (r#"<Cell N="FlipX" V="1"/>"#, (5.0, 4.0)),
        (r#"<Cell N="FlipY" V="1"/>"#, (3.0, 5.0)),
        (r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#, (5.0, 5.0)),
        ("", (3.0, 4.0)),
    ];
    for (flips, lower_left) in cases {
        let (mut s, _dir) = open_page(&group_with_member(flips, ""));
        let p = place(&mut s, 11);
        let (x, y) = lower_left;
        let want = [(x, y), (x + 2.0, y), (x + 2.0, y + 1.0), (x, y + 1.0)];
        for (got, want) in p.corners.iter().zip(want) {
            assert!(near_p(*got, want), "{flips}: corners {:?}, wanted {want:?} among {:?}", p.corners, want);
        }
        assert!(same_direction(p.direction, 0.0), "{flips}: the text runs to the right, not {}", p.direction);
        // The drawn text lies in the block the command says, and is wider than tall.
        let r = render(&s);
        let ink = ink_extent(&r, x - 0.1, y - 0.1, x + 2.1, y + 1.1).unwrap();
        assert!(ink.2 - ink.0 > 2.0 * (ink.3 - ink.1), "{flips}: {ink:?}");
        assert!(ink.0 >= x - 0.05 && ink.2 <= x + 2.05 && ink.1 >= y - 0.05 && ink.3 <= y + 1.05, "{flips}: {ink:?} in the block at ({x}, {y})");
    }
}

#[test]
fn the_text_of_a_member_of_a_flipped_group_turns_as_a_flipped_shape_does() {
    // A block turned 90 degrees by its cell: gives Angle - TxtAngle under one flip, here the group's, and the same under the other. Two
    // flips are a half turn that the text does not take.
    let block = BLOCK.replace(r#"<Cell N="TxtAngle" V="0"/>"#, &format!(r#"<Cell N="TxtAngle" V="{}"/>"#, rad(90.0)));
    for (flips, want) in [
        (r#"<Cell N="FlipX" V="1"/>"#, -90.0),
        (r#"<Cell N="FlipY" V="1"/>"#, -90.0),
        (r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#, 90.0),
        ("", 90.0),
    ] {
        let (mut s, _dir) = open_page(&group_with_member(flips, &block));
        let p = place(&mut s, 11);
        assert!(same_direction(p.direction, want), "{flips}: {} for {want}", p.direction);
    }
    // A group turned a quarter and flipped: the turn is the group's, the flip is not taken by the text.
    let turned = format!(r#"<Cell N="Angle" V="{}"/><Cell N="FlipX" V="1"/>"#, rad(90.0));
    let (mut s, _dir) = open_page(&group_with_member(&turned, ""));
    assert!(same_direction(place(&mut s, 11).direction, 90.0));
}

#[test]
fn a_block_of_a_member_of_a_flipped_group_is_sized_and_turned_where_it_is_drawn() {
    // Resize: the corner asked for is the block's first, at the lower left of the block as the page shows it, and the block runs to the right and up
    // from it. Rotate: a turn of 90 on the page is 90 on the page (the cell takes the negative under the group's one flip).
    for flips in [r#"<Cell N="FlipX" V="1"/>"#, r#"<Cell N="FlipY" V="1"/>"#, r#"<Cell N="FlipX" V="1"/><Cell N="FlipY" V="1"/>"#] {
        let (mut s, _dir) = open_page(&group_with_member(flips, BLOCK));
        run(&mut s, "text.block_resize", json!({"id": 11, "x": 4, "y": 5.5, "width": 3, "height": 0.5}));
        let p = place(&mut s, 11);
        let want = [(4.0, 5.5), (7.0, 5.5), (7.0, 6.0), (4.0, 6.0)];
        for (got, want) in p.corners.iter().zip(want) {
            assert!(near_p(*got, want), "{flips}: {:?}", p.corners);
        }
        assert!(near(p.width, 3.0) && near(p.height, 0.5), "{flips}");
        let was = p.direction;
        run(&mut s, "text.block_rotate", json!({"id": 11, "by": 90}));
        let after = place(&mut s, 11);
        assert!(same_direction(after.direction, was + 90.0), "{flips}: {was} then {}", after.direction);
        // The pin did not move.
        assert!(near_p(after.pin, p.pin), "{flips}");
    }
}
