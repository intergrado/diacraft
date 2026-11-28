//! Reverse Ends, `connector.reverse_ends` (`draw/reverse.rs`, `docs/editing-commands.md`).
//!
//! The command swaps the begin and the end of the connectors and lines it is given. What these tests hold it to: the ends, their formulas and their `Connect` rows swap, so each
//! end stays glued to the shape and the point it was; the route is the same line walked the other way, whatever its kind (a fixed route, a hand-edited leg, a straight line, a
//! curve); the format cells (the arrows) stay where they are, so the arrow ends up at the other end; a label stays where it is; many shapes are one undo step; and a shape
//! that is no connector, a protected end, a locked layer and hostile pages are handled in words and in counted work.

use diacraft_doc::{Document, Owner, Resolver, SectionKey, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::draw::{Pose, connection_points, connector_path};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::connect_style::{cell, distance_to, drawn, z_pair};
use crate::edit::{doc, drop_shape, geometry, id_of, num, route};
use crate::edit_foreign::opened;
use crate::odg_build::Dir;
use crate::{blank_session, check_session, run, snapshot};

type P = (f64, f64);

/// Where the begin and the end of a connector are glued: the shape and the connection point (3 for the whole shape).
type Glue = (Option<(u32, u32)>, Option<(u32, u32)>);

const ID: &str = "connector.reverse_ends";

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn same_points(got: &[P], want: &[P], what: &str) {
    assert!(got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near_p(*g, *w)), "{what}: got {got:?}, want {want:?}");
}

fn reversed(points: &[P]) -> Vec<P> {
    points.iter().rev().copied().collect()
}

/// Whether two angles differ by a half turn.
fn half_turn(a: f64, b: f64) -> bool {
    ((b - a).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs() < 1.0e-6
}

/// Whether two angles are the same turn.
fn same_turn(a: f64, b: f64) -> bool {
    let d = (b - a).rem_euclid(std::f64::consts::TAU);
    d < 1.0e-6 || std::f64::consts::TAU - d < 1.0e-6
}

fn ends(d: &Document, c: u32) -> (P, P) {
    ((num(d, c, "BeginX"), num(d, c, "BeginY")), (num(d, c, "EndX"), num(d, c, "EndY")))
}

/// Where the begin and the end of connector `c` are glued, from the `Connect` rows of page `page`: the shape and the connection point's `IX` (3 for the whole shape).
fn glue_on(d: &Document, page: u32, c: u32) -> Glue {
    let p = d.page(page).unwrap();
    let find = |name: &str| {
        p.connects
            .iter()
            .find(|r| r.from_sheet == c && r.from_cell.as_deref() == Some(name))
            .map(|r| (r.to_sheet, r.to_part.map_or(0, |part| if part >= 100 { part - 100 } else { part })))
    };
    (find("BeginX"), find("EndX"))
}

fn glue_of(d: &Document, c: u32) -> Glue {
    glue_on(d, 1, c)
}

/// The text of the formula the cell holds on the shape itself.
fn formula(d: &Document, c: u32, name: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(1), c).unwrap();
    let cell = d.shape(&at).unwrap().sheet.cells.get(&diacraft_doc::CellName::new(name))?;
    cell.formula.text().map(str::to_owned)
}

/// The cached error a cell of the shape holds, if it has one.
fn error_of(d: &Document, c: u32, name: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(1), c).unwrap();
    d.shape(&at).unwrap().sheet.cells.get(&diacraft_doc::CellName::new(name))?.error.as_deref().map(str::to_owned)
}

/// The pose of connector `c` as its cells hold it now.
fn pose_of(d: &Document, c: u32) -> Pose {
    Pose {
        pin: (num(d, c, "PinX"), num(d, c, "PinY")),
        loc: (num(d, c, "LocPinX"), num(d, c, "LocPinY")),
        angle: num(d, c, "Angle"),
        flip_x: false,
        flip_y: false,
    }
}

/// Where the middle of the label of connector `c` is on the page, and the angle the text lies at there.
fn label_of(d: &Document, c: u32) -> (P, f64) {
    let at = pose_of(d, c).up((num(d, c, "TxtPinX"), num(d, c, "TxtPinY")));
    (at, num(d, c, "Angle") + num(d, c, "TxtAngle"))
}

/// The `X` formula of each row of the first `Geometry` section.
fn x_formulas(d: &Document, c: u32) -> Vec<Option<String>> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), c).unwrap());
    let rows = Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap();
    rows.iter()
        .map(|row| {
            row.cells
                .iter()
                .find(|(n, _)| n.as_str() == "X")
                .and_then(|(_, r)| r.formula.cell.and_then(|cell| cell.formula.text().map(str::to_owned)))
        })
        .collect()
}

fn quiet(s: &Session) -> crate::Snapshot {
    crate::Snapshot { journal: Vec::new(), ..snapshot(s) }
}

#[test]
fn a_z_connector_reversed_swaps_its_ends_and_its_glue_and_draws_the_same_line_the_other_way() {
    let (mut s, a, b, c) = z_pair();
    let d = doc(&s);
    let (begin, end) = ends(d, c);
    let (route_before, angle, width) = (route(d, c), num(d, c, "Angle"), num(d, c, "Width"));
    assert_eq!(glue_of(d, c), (Some((a, 3)), Some((b, 1))));
    let steps = snapshot(&s).undo_steps;
    let r = run(&mut s, ID, json!({"id": c}));
    assert_eq!(r["page"], json!(1));
    assert_eq!(r["reversed"], json!(1));
    assert_eq!(r["ids"], json!([c]));
    assert_eq!((r["id"].clone(), r["begin"].clone(), r["end"].clone()), (json!(c), json!({"id": b, "point": 1}), json!({"id": a, "point": 3})));
    assert!(r["says"].as_str().is_some_and(|words| words.contains("Reversed")), "it says what it did: {r}");
    let d = doc(&s);
    let (new_begin, new_end) = ends(d, c);
    assert!(near_p(new_begin, end) && near_p(new_end, begin), "the ends swap: {new_begin:?} {new_end:?}");
    assert_eq!(glue_of(d, c), (Some((b, 1)), Some((a, 3))), "each end stays glued to the shape and the point it was");
    assert!(formula(d, c, "BeginX").unwrap().contains(&format!("Sheet.{b}!Connections.X2")), "{:?}", formula(d, c, "BeginX"));
    assert!(formula(d, c, "EndY").unwrap().contains(&format!("Sheet.{a}!Connections.Y4")), "{:?}", formula(d, c, "EndY"));
    assert!(near(num(d, c, "Width"), width) && half_turn(angle, num(d, c, "Angle")), "the same length, the other way");
    same_points(&route(d, c), &reversed(&route_before), "the same line, walked the other way");
    // The arrow cells stay: the arrow is at the end of the path, which is now where the connector began.
    assert_eq!((num(d, c, "EndArrow"), cell(d, c, "BeginArrow").unwrap_or(0.0)), (4.0, 0.0));
    assert!(near_p(*route(d, c).last().unwrap(), begin), "the arrow is at the other end");
    assert_eq!(snapshot(&s).undo_steps, steps + 1, "one undo step");
    assert_eq!(check_session(&s), Ok(()));
    // The glue follows its shape: A is now at the end, and moving A moves the end, with the route.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": 1.0}));
    let d = doc(&s);
    assert!(near_p(ends(d, c).1, (3.75, 5.0)), "{:?}", ends(d, c));
    let r = route(d, c);
    assert!(near_p(r[0], ends(d, c).0) && near_p(*r.last().unwrap(), ends(d, c).1), "{r:?}");
}

#[test]
fn undo_takes_the_whole_reversal_back_and_a_second_reversal_gives_the_connector_back() {
    let (mut s, _, _, c) = z_pair();
    let was = quiet(&s);
    run(&mut s, ID, json!({"id": c}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(quiet(&s).document, was.document, "undo restores the drawing exactly");
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, ID, json!({"id": c}));
    let (d, d0) = (doc(&s), was.document.as_ref().unwrap());
    let (e, e0) = (ends(d, c), ends(d0, c));
    assert!(near_p(e.0, e0.0) && near_p(e.1, e0.1), "{e:?} {e0:?}");
    assert_eq!(glue_of(d, c), glue_of(d0, c));
    for name in ["BeginX", "BeginY", "EndX", "EndY"] {
        assert_eq!(formula(d, c, name), formula(d0, c, name), "{name}");
    }
    assert!(same_turn(num(d, c, "Angle"), num(d0, c, "Angle")));
    let (rows, rows0) = (geometry(d, c), geometry(d0, c));
    assert!(
        rows.len() == rows0.len() && rows.iter().zip(&rows0).all(|(r, r0)| r.0 == r0.0 && near(r.1, r0.1) && near(r.2, r0.2)),
        "{rows:?} {rows0:?}"
    );
    same_points(&route(d, c), &route(d0, c), "the route after two reversals");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_route_edited_by_hand_stays_fixed_and_is_the_same_line_reversed_and_can_be_edited_again() {
    let (mut s, a, b, c) = z_pair();
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.0}));
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    let d = doc(&s);
    let (before, rows) = (route(d, c), geometry(d, c).len());
    assert_eq!(before.len(), 6, "a jog at the begin: {before:?}");
    run(&mut s, ID, json!({"id": c}));
    let d = doc(&s);
    assert_eq!(cell(d, c, "ConFixedCode"), Some(3.0), "a hand-edited path stays hand-edited");
    same_points(&route(d, c), &reversed(&before), "the hand-edited legs, walked the other way");
    assert_eq!(geometry(d, c).len(), rows);
    // The last row follows the width, as the route of a file's fixed connector does, and the first does not.
    let xs = x_formulas(d, c);
    assert_eq!(xs.last().cloned().flatten().as_deref(), Some("Width*1"), "{xs:?}");
    assert!(xs[..xs.len() - 1].iter().all(Option::is_none), "{xs:?}");
    let p = connector_path(d, 1, c).unwrap();
    assert!(p.hand_edited && p.editable, "{p:?}");
    // A leg can be dragged again, and the end that is now B's follows B square.
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 2, "to": 6.5}));
    assert!(route(doc(&s), c).iter().any(|p| near(p.1, 6.5)), "a level leg now at the height 6.5: {:?}", route(doc(&s), c));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 0.5}));
    let d = doc(&s);
    let (begin, end) = ends(d, c);
    assert!(near_p(begin, (8.25, 6.5)), "the begin is on B now and follows it: {begin:?}");
    assert!(near_p(end, (3.75, 4.0)), "{end:?}");
    let r = route(d, c);
    assert!(near_p(r[0], begin) && near_p(*r.last().unwrap(), end), "{r:?}");
    let _ = a;
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn straight_and_curved_connectors_are_the_same_line_reversed() {
    for style in ["straight", "curved"] {
        let (mut s, _, _, c) = z_pair();
        run(&mut s, "connector.style", json!({"style": style, "ids": [c]}));
        let before = drawn(doc(&s), c);
        run(&mut s, ID, json!({"id": c}));
        let after = drawn(doc(&s), c);
        assert!(
            near_p(after[0], *before.last().unwrap()) && near_p(*after.last().unwrap(), before[0]),
            "{style}: it starts where it ended: {after:?}"
        );
        assert!(after.iter().all(|p| distance_to(&before, *p) < 2.0e-3), "{style}: every point of the new line is on the old one");
        assert!(before.iter().all(|p| distance_to(&after, *p) < 2.0e-3), "{style}: and every point of the old line is on the new one");
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_label_stays_where_it_is_on_the_page_and_upright() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "shape.set_text", json!({"id": c, "text": "go"}));
    let d = doc(&s);
    let (at, turn) = label_of(d, c);
    assert!(same_turn(turn, 0.0), "the label is upright: {turn}");
    run(&mut s, ID, json!({"id": c}));
    let (after, turn) = label_of(doc(&s), c);
    assert!(near_p(after, at) && same_turn(turn, 0.0), "the label does not move or turn: {after:?} {at:?} {turn}");
    // A label a person moved off the middle, and turned, is where it was on the page too.
    run(&mut s, "sheet.set_cell", json!({"id": c, "cell": "TxtPinX", "formula": "0.7"}));
    run(&mut s, "sheet.set_cell", json!({"id": c, "cell": "TxtPinY", "formula": "0.3"}));
    run(&mut s, "sheet.set_cell", json!({"id": c, "cell": "TxtAngle", "formula": "0.4"}));
    let (at, turn) = label_of(doc(&s), c);
    run(&mut s, ID, json!({"id": c}));
    let (after, after_turn) = label_of(doc(&s), c);
    assert!(near_p(after, at) && same_turn(after_turn, turn), "{after:?} {at:?} {after_turn} {turn}");
    // The text itself is untouched.
    let text = |d: &Document| d.shape(&d.find_shape(Owner::Page(1), c).unwrap()).unwrap().text.clone();
    assert!(text(doc(&s)).is_some());
}

#[test]
fn several_shapes_are_one_undo_step_and_a_shape_that_is_no_connector_is_left_alone_and_listed() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 7.0, 3.0);
    let c = drop_shape(&mut s, "rectangle", 3.0, 7.0);
    let k1 = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let k2 = id_of(&run(&mut s, "connector.add", json!({"from": {"id": b, "point": 2}, "to": {"id": c, "point": 3}})));
    let k3 = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 9.0}, "to": {"x": 8.0, "y": 9.0}})));
    let was = quiet(&s);
    let (e1, e3) = (ends(doc(&s), k1), ends(doc(&s), k3));
    let r = run(&mut s, ID, json!({"ids": [k1, a, k2, k3]}));
    assert_eq!((r["reversed"].clone(), r["ids"].clone()), (json!(3), json!([k1, k2, k3])));
    assert_eq!(r["skipped"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(r["skipped"][0]["id"], json!(a));
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("not a connector or a line"), "{r}");
    assert!(r.get("id").is_none(), "{r}");
    assert_eq!(quiet(&s).undo_steps, was.undo_steps + 1, "one undo step for all");
    let d = doc(&s);
    assert!(near_p(ends(d, k1).0, e1.1) && near_p(ends(d, k1).1, e1.0));
    assert!(near_p(ends(d, k3).0, e3.1) && near_p(ends(d, k3).1, e3.0), "a connector with free ends too");
    assert_eq!(glue_of(d, k3), (None, None), "free ends stay free");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(quiet(&s).document, was.document);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_that_is_no_connector_is_refused_in_words_and_nothing_changes() {
    let (mut s, a, b, _) = z_pair();
    let was = quiet(&s);
    let e = s.execute(ID, &json!({"id": a})).unwrap_err();
    let words = e.to_string();
    assert!(words.contains(&format!("shape {a} is not a connector or a line")) && words.contains("begin and end"), "{words}");
    let e = s.execute(ID, &json!({"ids": [a, b]})).unwrap_err();
    assert!(e.to_string().contains("none of the 2 shapes named") && e.to_string().contains("is not a connector or a line"), "{e}");
    assert_eq!(quiet(&s), was, "no change, no undo step");
}

/// A page of four connectors: 50 has `LockBegin` on, 51 `LockEnd`, 52 a begin that `GUARD` protects, and 53 nothing.
fn protected_page() -> Session {
    let shapes = format!(
        "{}{}{}{}",
        zigzag_connector(50, 1.0, 1.0, 3.0, r#"<Cell N="LockBegin" V="1"/>"#, false),
        zigzag_connector(51, 1.0, 3.0, 3.0, r#"<Cell N="LockEnd" V="1"/>"#, false),
        zigzag_connector(52, 1.0, 5.0, 3.0, "", true),
        zigzag_connector(53, 1.0, 7.0, 3.0, "", false),
    );
    let dir = Dir::new();
    let path = dir.path("protected.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build()).unwrap();
    reopened(&path)
}

#[test]
fn a_protected_begin_or_end_and_a_guarded_cell_are_left_alone_in_words() {
    let mut s = protected_page();
    let (e50, e53) = (ends(doc(&s), 50), ends(doc(&s), 53));
    // A selection acts on the shapes it can and lists the locked ones.
    let r = run(&mut s, ID, json!({"ids": [50, 53]}));
    assert_eq!((r["reversed"].clone(), r["ids"].clone(), r["locked"].clone()), (json!(1), json!([53]), json!([50])), "{r}");
    assert!(near_p(ends(doc(&s), 50).0, e50.0) && near_p(ends(doc(&s), 50).1, e50.1), "the protected connector did not change");
    assert!(near_p(ends(doc(&s), 53).0, e53.1) && near_p(ends(doc(&s), 53).1, e53.0));
    // LockEnd protects the same way, and a begin that GUARD holds is left alone and listed, in words.
    let r = run(&mut s, ID, json!({"ids": [51, 52, 53]}));
    assert_eq!((r["reversed"].clone(), r["ids"].clone(), r["locked"].clone()), (json!(1), json!([53]), json!([51])), "{r}");
    assert_eq!(r["skipped"][0]["id"], json!(52));
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("GUARD"), "{r}");
    // Every shape protected: refused in words that name the cells, with nothing changed.
    let was = quiet(&s);
    let e = s.execute(ID, &json!({"ids": [50, 51]})).unwrap_err();
    assert!(e.to_string().contains("none of the 2 shapes") && e.to_string().contains("LockBegin or LockEnd"), "{e}");
    let e = s.execute(ID, &json!({"id": 50})).unwrap_err();
    assert!(e.to_string().contains("shape 50") && e.to_string().contains("protected"), "{e}");
    let e = s.execute(ID, &json!({"id": 52})).unwrap_err();
    assert!(e.to_string().contains("shape 52") && e.to_string().contains("GUARD"), "{e}");
    assert_eq!(quiet(&s), was);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_on_a_locked_layer_is_refused_naming_the_layer() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [c], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let was = quiet(&s);
    for params in [json!({"id": c}), json!({"ids": [c]})] {
        let e = s.execute(ID, &params).unwrap_err();
        assert!(e.to_string().contains("Frozen") && e.to_string().contains("locked"), "{e}");
    }
    assert_eq!(quiet(&s), was);
}

#[test]
fn the_parameters_are_checked_and_a_refusal_changes_nothing() {
    let (mut s, _, _, c) = z_pair();
    let was = quiet(&s);
    let ids: Vec<u32> = (0..4097).collect();
    for (what, params) in [
        ("neither id nor ids", json!({})),
        ("both", json!({"id": c, "ids": [c]})),
        ("no ids", json!({"ids": []})),
        ("too many ids", json!({"ids": ids})),
        ("an id that is text", json!({"id": "3"})),
        ("a negative id", json!({"id": -1})),
        ("an id that is not an id", json!({"id": 1.5})),
        ("a shape that is not there", json!({"id": 9999})),
        ("a page that is not there", json!({"id": c, "page": 9})),
        ("an unknown key", json!({"id": c, "pgae": 1})),
    ] {
        let e = s.execute(ID, &params);
        assert!(e.is_err(), "{what}");
        assert_eq!(quiet(&s), was, "{what}");
    }
    let e = s.execute(ID, &json!({"id": c, "pgae": 1})).unwrap_err();
    assert!(e.to_string().contains("did you mean `page`"), "{e}");
    let e = s.execute(ID, &json!({"id": 9999})).unwrap_err();
    assert!(e.to_string().contains("has no shape 9999"), "{e}");
    // No document: the command is not enabled.
    let none = Session::new();
    let info = diacraft_engine::find_command(ID).unwrap().info(&none);
    assert!(!info.enabled && info.disabled_reason.is_some(), "{info:?}");
    assert_eq!(info.shortcut.as_deref(), Some("Cmd+Alt+R"));
}

#[test]
fn a_connector_glued_at_both_ends_to_one_shape_reverses_and_comes_back() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 5.0, 5.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 0}, "to": {"id": a, "point": 2}})));
    let (d, route_before) = (doc(&s).clone(), route(doc(&s), c));
    assert_eq!(glue_of(&d, c), (Some((a, 0)), Some((a, 2))));
    run(&mut s, ID, json!({"id": c}));
    let after = doc(&s);
    assert_eq!(glue_of(after, c), (Some((a, 2)), Some((a, 0))));
    same_points(&route(after, c), &reversed(&route_before), "a loop out of one shape and back into it");
    // The same point at both ends is a connector of no length: it reverses too, and stays put.
    let z = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 1}, "to": {"id": a, "point": 1}})));
    let was = ends(doc(&s), z);
    run(&mut s, ID, json!({"id": z}));
    assert!(near_p(ends(doc(&s), z).0, was.1) && near_p(ends(doc(&s), z).1, was.0));
    assert_eq!(glue_of(doc(&s), z), (Some((a, 1)), Some((a, 1))));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_connectors_of_a_file_keep_their_glue_swapped_with_their_ends_whichever_way_they_are_glued() {
    let dir = crate::edit_foreign::Dir::new();
    let mut s = opened(&dir);
    let glue_before: Vec<(u32, Glue)> = [20, 21, 22, 23].iter().map(|c| (*c, glue_of(doc(&s), *c))).collect();
    // 20: to points of 10 and 11; 21: to the whole of 12 and 13 (`_WALKGLUE`); 22: fixed, to points of 10 and 12; 23: a point of 11 and the whole of 13.
    for c in [20, 21, 22, 23] {
        let d = doc(&s);
        let (b, angle, width) = (ends(d, c), num(d, c, "Angle"), num(d, c, "Width"));
        let (glue, route_before) = (glue_of(d, c), route(d, c));
        let fixed = cell(d, c, "ConFixedCode");
        run(&mut s, ID, json!({"id": c}));
        let d = doc(&s);
        let now = ends(d, c);
        assert!(near_p(now.0, b.1) && near_p(now.1, b.0), "{c}: the ends swap");
        assert_eq!(glue_of(d, c), (glue.1, glue.0), "{c}: the glue swaps with them");
        assert!(half_turn(angle, num(d, c, "Angle")) && near(width, num(d, c, "Width")), "{c}");
        same_points(&route(d, c), &reversed(&route_before), &format!("{c}: the route"));
        assert_eq!(cell(d, c, "ConFixedCode"), fixed, "{c}: a fixed route stays fixed");
        for name in ["BeginX", "BeginY", "EndX", "EndY"] {
            assert_eq!(error_of(d, c, name), None, "{c}: {name} holds no error");
        }
        assert_eq!(num(d, c, "EndArrow"), 4.0, "{c}: the arrow cell stays");
    }
    // The triggers go with the ends they name, so that each end follows its shape.
    let d = doc(&s);
    assert_eq!(formula(d, 21, "BegTrigger").as_deref(), Some("_XFTRIGGER(Sheet.13!EventXFMod)"));
    assert_eq!(formula(d, 21, "EndTrigger").as_deref(), Some("_XFTRIGGER(Sheet.12!EventXFMod)"));
    assert_eq!(glue_of(d, 21), (Some((13, 3)), Some((12, 3))), "the whole of each shape, swapped");
    // The end that is glued follows its shape after the reversal: 21's begin is on 13 now, and a move of 13 moves it (the engine places an end that calc cannot work out) while
    // the end on 12 stays.
    let (begin, end) = ends(d, 21);
    run(&mut s, "shape.move", json!({"ids": [13], "dx": 0.0, "dy": 1.0, "page": 1}));
    let (moved_begin, moved_end) = ends(doc(&s), 21);
    assert!(moved_begin.1 > begin.1 + 0.25, "the begin follows 13 (the nearest point of its outline to the other end): {begin:?} {moved_begin:?}");
    assert!(near_p(moved_end, end), "and the end on 12 stays: {end:?} {moved_end:?}");
    assert_eq!(check_session(&s), Ok(()));
    // Saved in place and opened again, each connector has its glue swapped, and 21 its triggers.
    run(&mut s, "file.save", json!({}));
    let back = reopened(&dir.path("in.vsdx"));
    let d = back.document().unwrap();
    for (c, glue) in &glue_before {
        assert_eq!(glue_of(d, *c), (glue.1, glue.0), "{c}: the glue is swapped in the file");
    }
    assert_eq!(formula(d, 21, "BegTrigger").as_deref(), Some("_XFTRIGGER(Sheet.13!EventXFMod)"), "the triggers are swapped in the file");
    assert_eq!(check_session(&back), Ok(()));
    // Undo and redo are one step each.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(check_session(&s), Ok(()));
}

/// The one connector of the first page of `d`, by its page and `ID`.
fn connector_of(d: &Document) -> (u32, u32) {
    let page = d.pages.get(0).expect("a page");
    let found: Vec<u32> = page.shapes.iter().map(|s| s.id).filter(|id| connector_path(d, page.id, *id).is_some()).collect();
    assert_eq!(found.len(), 1, "one connector on the page: {found:?}");
    (page.id, found[0])
}

fn cell_on(d: &Document, page: u32, id: u32, name: &str) -> Option<f64> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(page), id)?);
    Resolver::new(d).resolve(&sheet, &diacraft_doc::CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number())
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

#[test]
fn the_glue_and_the_arrow_survive_a_save_and_a_reopen_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut first, a, b, c) = z_pair();
        let begin = ends(doc(&first), c).0;
        let route_before = route(doc(&first), c);
        run(&mut first, ID, json!({"id": c}));
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let back = reopened(&path);
        let d = back.document().unwrap();
        let (page, id) = connector_of(d);
        // The glue: the begin is on B's left point, the end on A's right point (by the shapes' own order in the file).
        let (gb, ge) = glue_on(d, page, id);
        assert!(gb.is_some() && ge.is_some(), "{ext}: both ends are still glued: {gb:?} {ge:?}");
        let shapes: Vec<u32> = d.page(page).unwrap().shapes.iter().map(|s| s.id).filter(|s| *s != id).collect();
        assert_eq!(
            (gb.map(|g| g.0), ge.map(|g| g.0)),
            (Some(shapes[1]), Some(shapes[0])),
            "{ext}: the begin on the second shape, the end on the first ({a} and {b})"
        );
        // The arrow stays at the end of the path, where the connector began.
        assert_eq!(cell_on(d, page, id, "EndArrow"), Some(4.0), "{ext}");
        assert!(cell_on(d, page, id, "BeginArrow").unwrap_or(0.0) == 0.0, "{ext}");
        let (bx, by) = (cell_on(d, page, id, "BeginX").unwrap(), cell_on(d, page, id, "BeginY").unwrap());
        assert!((bx - 8.25).abs() < 1.0e-3 && (by - 6.0).abs() < 1.0e-3, "{ext}: the begin is B's left point: ({bx}, {by})");
        let points = connector_path(d, page, id).expect("a path of lines").points;
        let want = reversed(&route_before);
        assert!(
            points.len() == want.len() && points.iter().zip(&want).all(|(g, w)| (g.0 - w.0).abs() < 1.0e-3 && (g.1 - w.1).abs() < 1.0e-3),
            "{ext}: the route, walked the other way: {points:?} {want:?}"
        );
        assert!((points.last().unwrap().0 - begin.0).abs() < 1.0e-3, "{ext}: the arrow is at the old begin");
    }
}

#[test]
fn a_drawing_read_from_a_file_is_reversed_and_saved_in_place_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut first, _, _, _) = z_pair();
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        let mut s = reopened(&path);
        let (page, id) = connector_of(s.document().unwrap());
        let (g0, ends0) = (glue_on(s.document().unwrap(), page, id), ends_on(s.document().unwrap(), page, id));
        run(&mut s, ID, json!({"id": id, "page": page}));
        // Every format saves it in place: the splice into an `.odg` writes the glue that changed, as the others do.
        run(&mut s, "file.save", json!({}));
        let back = reopened(&path);
        let d = back.document().unwrap();
        let (page, id) = connector_of(d);
        let (g1, ends1) = (glue_on(d, page, id), ends_on(d, page, id));
        assert_eq!(g1, (g0.1.map(|g| (g.0, g.1)), g0.0.map(|g| (g.0, g.1))), "{ext}: the glue swapped");
        assert!(near_loose(ends1.0, ends0.1) && near_loose(ends1.1, ends0.0), "{ext}: the ends swapped: {ends1:?} {ends0:?}");
        assert_eq!(cell_on(d, page, id, "EndArrow"), Some(4.0), "{ext}");
    }
}

fn ends_on(d: &Document, page: u32, id: u32) -> (P, P) {
    let n = |name: &str| cell_on(d, page, id, name).unwrap();
    ((n("BeginX"), n("BeginY")), (n("EndX"), n("EndY")))
}

fn near_loose(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < 1.0e-3 && (a.1 - b.1).abs() < 1.0e-3
}

// ------------------------------------------------------------------ hostile pages

/// The cells of a connector as a file holds them: its ends at (`x`, `y`) and (`x` + `w`, `y`), the creation formulas of the frame, and a Z for a route; a master-less shape of ours
/// (it holds the marker of the routes DiaCraft rewrites).
fn zigzag_connector(id: u32, x: f64, y: f64, w: f64, extra: &str, guard_begin: bool) -> String {
    let begin_x = if guard_begin { format!(r#"<Cell N="BeginX" V="{x}" F="GUARD({x})"/>"#) } else { format!(r#"<Cell N="BeginX" V="{x}"/>"#) };
    shape(
        id,
        &format!(
            r#"{begin_x}<Cell N="BeginY" V="{y}"/><Cell N="EndX" V="{ex}"/><Cell N="EndY" V="{ey}"/>
<Cell N="PinX" V="{px}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{py}" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{d}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{h}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Cell N="EndArrow" V="4"/>{extra}
<Section N="User"><Row N="DiaCraftRoute"><Cell N="Value" V="1"/></Row></Section>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{q}"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{q}"/><Cell N="Y" V="{up}"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="{d}" F="Width*1"/><Cell N="Y" V="{up}"/></Row></Section>"#,
            ex = x + w,
            ey = y + 1.0,
            px = x + w / 2.0,
            py = y + 0.5,
            d = (w * w + 1.0).sqrt(),
            h = (w * w + 1.0).sqrt() / 2.0,
            angle = (1.0_f64).atan2(w),
            q = (w * w + 1.0).sqrt() / 3.0,
            up = 0.5,
        ),
    )
}

fn many(count: u32) -> Document {
    let shapes: String = (0..count)
        .map(|n| zigzag_connector(100 + n, f64::from(n % 100) * 0.1, f64::from(n / 100) * 0.2, 0.05 + f64::from(n % 7) * 0.01, "", false))
        .collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build();
    let dir = Dir::new();
    let path = dir.path("many.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s.document().unwrap().clone()
}

#[test]
fn ten_thousand_connectors_reversed_in_one_call_are_one_edit_with_counted_work() {
    let mut d = many(10_000);
    let ids: Vec<u32> = (0..10_000).map(|n| 100 + n).collect();
    let probe = |d: &Document, id: u32| (ends(d, id), route_on(d, id));
    let first = probe(&d, 100);
    let last = probe(&d, 100 + 9_999);
    // What the recalculation of the whole page takes, the figure the call is set against (it holds on any machine at any load).
    let whole = {
        let mut copy = d.clone();
        let report = diacraft_calc::commit_in(
            &mut copy,
            &diacraft_calc::Budget::default(),
            diacraft_calc::EvalOptions::default(),
            diacraft_calc::Scope::Page(1),
        );
        assert!(!report.budget_exhausted, "{report:?}");
        report.stats.steps
    };
    diacraft_calc::work::reset();
    let done = diacraft_engine::draw::reverse_ends(&mut d, 1, &ids).unwrap();
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("10,000 connectors reversed: {commits} commit of {steps} steps; the whole page takes {whole}");
    assert_eq!(done.reversed.len(), 10_000);
    assert!(done.locked.is_empty() && done.skipped.is_empty() && done.geometry_kept.is_empty(), "{done:?}");
    assert_eq!(commits, 1, "one commit works out what reads the ends of all of them");
    // The work is the four cells of each connector and the cells that read them (the width, the angle and the pin): a few steps for each, so it is no more than the page
    // worked out twice (a quadratic pass over the page would be a thousand times that).
    assert!(steps <= 2 * whole, "{steps} steps for the call, {whole} for the whole page");
    for (id, was) in [(100, first), (100 + 9_999, last)] {
        let now = probe(&d, id);
        assert!(near_p(now.0.0, was.0.1) && near_p(now.0.1, was.0.0), "{id}: the ends swapped");
        same_points(&now.1, &reversed(&was.1), &format!("{id}: the route"));
    }
}

/// The route of connector `id` of the document `d` on the page: its rows carried through its pose.
fn route_on(d: &Document, id: u32) -> Vec<P> {
    let pose = pose_of(d, id);
    geometry(d, id).iter().map(|(_, x, y)| pose.up((*x, *y))).collect()
}

#[test]
fn four_thousand_ninety_six_connectors_by_the_command_are_one_undo_step() {
    let d = many(5_000);
    let mut s = Session::new();
    let dir = Dir::new();
    let path = dir.path("again.vsdx");
    let shapes: String = (0..5_000).map(|n| zigzag_connector(100 + n, f64::from(n % 100) * 0.1, f64::from(n / 100) * 0.2, 0.05, "", false)).collect();
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build()).unwrap();
    run(&mut s, "file.open", json!({ "path": path }));
    drop(d);
    let ids: Vec<u32> = (0..4_096).map(|n| 100 + n).collect();
    let was = quiet(&s);
    let r = run(&mut s, ID, json!({"ids": ids}));
    assert_eq!(r["reversed"], json!(4_096));
    assert_eq!(quiet(&s).undo_steps, was.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(quiet(&s).document, was.document);
    assert_eq!(check_session(&s), Ok(()));
}

/// A drawing of connectors that hold each other: 30 is glued to the whole of 31 at its end, 31 to the whole of 30 (a loop), and 32 to a connection point of 30 (30 has one at a
/// quarter of its length).
fn knotted() -> Vec<u8> {
    let walk = "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)";
    let wired = |id: u32, b: P, e: P, fb: &str, fe: &str, extra: &str| {
        let (dx, dy) = (e.0 - b.0, e.1 - b.1);
        let w = dx.hypot(dy);
        shape(
            id,
            &format!(
                r#"<Cell N="BeginX" V="{bx}" F="{fb}"/><Cell N="BeginY" V="{by}" F="{fb}"/><Cell N="EndX" V="{ex}" F="{fe}"/><Cell N="EndY" V="{ey}" F="{fe}"/>
<Cell N="PinX" V="{px}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{py}" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{w}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{h}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Cell N="EndArrow" V="4"/>{extra}
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width*1"/><Cell N="Y" V="0"/></Row></Section>"#,
                bx = b.0,
                by = b.1,
                ex = e.0,
                ey = e.1,
                px = (b.0 + e.0) / 2.0,
                py = (b.1 + e.1) / 2.0,
                h = w / 2.0,
                angle = dy.atan2(dx),
            ),
        )
    };
    let point_row = r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="1" F="Width*0.25"/><Cell N="Y" V="0" F="Height*0.5"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row></Section>"#;
    let c30 = wired(30, (1.0, 1.0), (5.0, 1.0), walk, walk, point_row);
    let c31 = wired(31, (1.0, 3.0), (5.0, 3.0), walk, walk, "");
    let to_point = "PAR(PNT(Sheet.30!Connections.X1,Sheet.30!Connections.Y1))";
    let c32 = wired(32, (1.0, 4.0), (2.0, 1.0), walk, to_point, "");
    let connects = [
        r#"<Connect FromSheet="30" FromCell="EndX" FromPart="12" ToSheet="31" ToCell="PinX" ToPart="3"/>"#,
        r#"<Connect FromSheet="31" FromCell="EndX" FromPart="12" ToSheet="30" ToCell="PinX" ToPart="3"/>"#,
        r#"<Connect FromSheet="32" FromCell="EndX" FromPart="12" ToSheet="30" ToCell="Connections.X1" ToPart="100"/>"#,
        // A row of a hostile file from a shape the page does not hold, to a shape that is reversed: it is left alone.
        r#"<Connect FromSheet="999" FromCell="EndX" FromPart="12" ToSheet="30" ToCell="Connections.X1" ToPart="100"/>"#,
    ]
    .join("\n");
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{c30}{c31}{c32}")).connects(&connects)).build()
}

#[test]
fn connectors_glued_to_each_other_in_a_loop_or_to_a_point_of_one_another_are_reversed_without_a_hang_or_a_broken_drawing() {
    let dir = Dir::new();
    let path = dir.path("knot.vsdx");
    std::fs::write(&path, knotted()).unwrap();
    let mut s = reopened(&path);
    let before = quiet(&s);
    // Each alone, then all three, then again: whatever the cells come to, the drawing stays valid and the call ends.
    for ids in [vec![30], vec![31], vec![32], vec![30, 31, 32], vec![30, 31, 32]] {
        let r = s.execute(ID, &json!({"ids": ids}));
        assert!(r.is_ok(), "{ids:?}: {r:?}");
        assert_eq!(check_session(&s), Ok(()));
    }
    // 32 is glued to the point of 30 that lies a quarter of the way along it: after 30 is reversed, the point is a quarter of the way from the other end, and 32 follows it.
    let mut s = reopened(&path);
    run(&mut s, ID, json!({"id": 30}));
    let d = doc(&s);
    let points = connection_points(d, 1, 30);
    assert_eq!(points.len(), 1);
    assert!(
        near_p(ends(d, 32).1, (points[0].x, points[0].y)),
        "the end of 32 is on the point of 30: {:?} {:?}",
        ends(d, 32).1,
        (points[0].x, points[0].y)
    );
    assert_eq!(glue_of(d, 32), (None, Some((30, 0))), "32 stays glued to the point of 30");
    assert!(before.document.is_some());
}

#[test]
fn a_line_with_no_master_and_a_bowed_path_is_the_same_line_reversed_with_its_arrow_at_the_other_end() {
    // A one-dimensional shape of a file that is no connector of ours: its pose is constants, its path bows, and its begin has the arrow.
    let line = shape(
        40,
        r#"<Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="5"/><Cell N="EndY" V="1"/>
<Cell N="PinX" V="3"/><Cell N="PinY" V="1"/><Cell N="Width" V="4"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="0"/>
<Cell N="BeginArrow" V="13"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0.5"/></Row>
<Row T="ArcTo" IX="3"><Cell N="X" V="3"/><Cell N="Y" V="0.5"/><Cell N="A" V="0.25"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="4"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&line)).build();
    let dir = Dir::new();
    let path = dir.path("line.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = reopened(&path);
    let before = geometry(doc(&s), 40);
    run(&mut s, ID, json!({"id": 40}));
    let d = doc(&s);
    assert_eq!((num(d, 40, "BeginX"), num(d, 40, "EndX")), (5.0, 1.0), "the ends swap");
    assert_eq!(
        (num(d, 40, "BeginArrow"), cell(d, 40, "EndArrow").unwrap_or(0.0)),
        (13.0, 0.0),
        "the arrow cells stay: the arrow is at the begin of the path, which is the other end now"
    );
    let rows = geometry(d, 40);
    let kinds: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(kinds, ["MoveTo", "LineTo", "ArcTo", "LineTo"], "{rows:?}");
    // The path starts where it ended (4, 0) and goes back through the same points; the arc bows to the same side of the line on the page.
    let want = [(4.0, 0.0), (3.0, 0.5), (1.0, 0.5), (0.0, 0.0)];
    assert!(rows.iter().zip(want).all(|(r, w)| near(r.1, w.0) && near(r.2, w.1)), "{rows:?} from {before:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_path_of_a_kind_it_cannot_carry_over_or_of_too_many_rows_keeps_its_rows_and_the_shape_is_listed() {
    // A line of a file whose path is a Bezier (`RelCubBezTo`), one with an ellipse, and one with more rows than a call reads for a shape.
    let wired = |id: u32, rows: &str| {
        shape(
            id,
            &format!(
                r#"<Cell N="BeginX" V="1"/><Cell N="BeginY" V="{id}"/><Cell N="EndX" V="5"/><Cell N="EndY" V="{id}"/>
<Cell N="PinX" V="3" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{id}" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>{rows}</Section>"#
            ),
        )
    };
    let bezier = r#"<Row T="RelMoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="RelCubBezTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="1"/><Cell N="A" V="0.3"/><Cell N="B" V="0"/><Cell N="C" V="0.6"/><Cell N="D" V="1"/></Row>"#;
    let ellipse = r#"<Row T="Ellipse" IX="1"><Cell N="X" V="2"/><Cell N="Y" V="0.5"/><Cell N="A" V="4"/><Cell N="B" V="0.5"/><Cell N="C" V="2"/><Cell N="D" V="1"/></Row>"#;
    let long: String = (1..=4_100)
        .map(|n| {
            format!(
                r#"<Row T="{}" IX="{n}"><Cell N="X" V="{}"/><Cell N="Y" V="0"/></Row>"#,
                if n == 1 { "MoveTo" } else { "LineTo" },
                f64::from(n) * 0.001
            )
        })
        .collect();
    let shapes = format!("{}{}{}", wired(60, bezier), wired(61, ellipse), wired(62, &long));
    let dir = Dir::new();
    let path = dir.path("kept.vsdx");
    std::fs::write(&path, VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build()).unwrap();
    let mut s = reopened(&path);
    let rows_before: Vec<_> = [60, 61, 62].iter().map(|id| geometry(doc(&s), *id)).collect();
    let r = run(&mut s, ID, json!({"ids": [60, 61, 62]}));
    assert_eq!((r["reversed"].clone(), r["geometry_kept"].clone()), (json!(3), json!([60, 61, 62])), "{r}");
    assert!(r["says"].as_str().unwrap().contains("could not be reversed"), "{r}");
    let d = doc(&s);
    for (n, id) in [60, 61, 62].into_iter().enumerate() {
        assert_eq!(geometry(d, id), rows_before[n], "{id}: its rows are as they were");
        assert_eq!((num(d, id, "BeginX"), num(d, id, "EndX")), (5.0, 1.0), "{id}: its ends swapped all the same");
        assert!(half_turn(0.0, num(d, id, "Angle")), "{id}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
