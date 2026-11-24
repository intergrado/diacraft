//! Glue to guides (in `docs/editing-commands.md`): a shape pulled to a guide by a move, a resize, a drop or a
//! place is held to it by the cells give, with the `Connect` rows, and follows the guide when it
//! moves or turns by recalculation. The numbers are the vectors' where one fits.
//!
//! Lengths the commands take are paper inches; the cells hold drawing units. The grid, the ruler and the dynamic grid are turned off in most tests so
//! that a guide is the only line that pulls.

use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Owner, Resolver, SheetRef};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

const SIX: [&str; 6] = ["AlignLeft", "AlignCenter", "AlignRight", "AlignBottom", "AlignMiddle", "AlignTop"];

fn id_of(v: &Value) -> u32 {
    u32::try_from(v["id"].as_u64().unwrap()).unwrap()
}

/// What a shape holds itself in a cell: the formula's text, the value and the unit.
fn local(s: &Session, id: u32, name: &str) -> (Option<String>, Option<f64>, Option<String>) {
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    let shape = doc.shape(&at).unwrap();
    match shape.sheet.cell(name) {
        Some(c) => (c.formula.text().map(str::to_owned), c.value.as_ref().and_then(|v| v.number()), c.unit.as_deref().map(str::to_owned)),
        None => (None, None, None),
    }
}

fn formula(s: &Session, id: u32, name: &str) -> Option<String> {
    local(s, id, name).0
}

/// The value of a cell, found by inheritance.
fn value(s: &Session, id: u32, name: &str) -> f64 {
    let doc = s.document().unwrap();
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(doc).resolve(&sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The `Connect` rows of page 1 from shape `id`: `(FromCell, FromPart, ToSheet, ToCell, ToPart)`.
fn rows_from(s: &Session, id: u32) -> Vec<(String, u32, u32, String, u32)> {
    let doc = s.document().unwrap();
    doc.page(1)
        .unwrap()
        .connects
        .iter()
        .filter(|c| c.from_sheet == id)
        .map(|c| {
            (
                c.from_cell.as_deref().unwrap_or("").to_owned(),
                c.from_part.unwrap_or(0),
                c.to_sheet,
                c.to_cell.as_deref().unwrap_or("").to_owned(),
                c.to_part.unwrap_or(0),
            )
        })
        .collect()
}

/// A session where only guides pull (no grid, ruler or dynamic grid).
fn guides_only() -> Session {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false, "dynamic_grid": false}));
    s
}

/// A rectangle 2 by 1 (`LocPinX` 1, `LocPinY` 0.5) with its middle at (`x`, `y`).
fn box_at(s: &mut Session, x: f64, y: f64) -> u32 {
    sized_box(s, (x, y), (2.0, 1.0))
}

fn sized_box(s: &mut Session, (x, y): (f64, f64), (w, h): (f64, f64)) -> u32 {
    let id = id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": y})));
    run(s, "shape.resize", json!({"id": id, "x": x - w / 2.0, "y": y - h / 2.0, "width": w, "height": h}));
    id
}

fn guide(s: &mut Session, orientation: &str, at: f64) -> u32 {
    id_of(&run(s, "guide.add", json!({"page": 1, "orientation": orientation, "at": at})))
}

fn single(func: &str, g: u32, place: (&str, &str)) -> String {
    format!("{func}(Sheet.{g}!PinX,Sheet.{g}!PinY,Sheet.{g}!Angle,{} DL,{} DL,Sheet.{g}!Angle+90 deg)", place.0, place.1)
}

fn crossing(func: &str, a: u32, b: u32) -> String {
    format!("{func}(Sheet.{a}!PinX,Sheet.{a}!PinY,Sheet.{a}!Angle,Sheet.{b}!PinX,Sheet.{b}!PinY,Sheet.{b}!Angle)")
}

/// The four cells that hold no guide are `_MARKER(1)` with `V` 1 and no `U`.
fn assert_markers(s: &Session, id: u32, guide_cells: &[&str]) {
    for name in SIX.iter().filter(|n| !guide_cells.contains(n)) {
        assert_eq!(local(s, id, name), (Some("_MARKER(1)".to_owned()), Some(1.0), None), "{name}");
    }
}

/// A shape held to an upright guide `Sheet.g` at x 3 by its left edge: the box of moved from the right to 0.04 off the guide.
fn held_by_its_left_edge() -> (Session, u32, u32) {
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    let id = box_at(&mut s, 6.0, 1.5);
    // The left edge is at 5; a move of -1.96 puts it at 3.04, and the guide pulls it the rest of the way.
    let moved = run(&mut s, "shape.move", json!({"ids": [id], "dx": -1.96, "dy": 0.0, "snap": true}));
    assert_eq!(moved["snap"]["holds"], json!([{"guide": g, "axis": "x", "anchor": "low", "cell": "AlignLeft"}]), "{moved}");
    assert_eq!(moved["snap"]["held"], json!([id]), "{moved}");
    (s, g, id)
}

// ---------------------------------------------------------------------------------------------------------------------- what is written

#[test]
fn a_box_pulled_to_an_upright_guide_is_held_by_its_left_edge_and_follows_it() {
    // The glued cell and the other axis's middle cell name the guide, the other four are markers, both pins follow the two
    // cells, and one `Connect` row goes from the glued cell to the guide's `Angle`, part 4.
    let (mut s, g, id) = held_by_its_left_edge();
    assert!(near(value(&s, id, "PinX"), 4.0) && near(value(&s, id, "PinY"), 1.5));
    // The place is the shape's middle where it came to rest, in drawing units (V1 writes the pin before the pull, which changes no value).
    assert_eq!(local(&s, id, "AlignLeft"), (Some(single("INTERSECTX", g, ("4", "1.5"))), Some(3.0), None));
    assert_eq!(local(&s, id, "AlignMiddle"), (Some(single("INTERSECTY", g, ("4", "1.5"))), Some(1.5), None));
    assert_markers(&s, id, &["AlignLeft", "AlignMiddle"]);
    assert_eq!(local(&s, id, "PinX"), (Some("LocPinX+AlignLeft".to_owned()), Some(4.0), Some("IN".to_owned())));
    assert_eq!(local(&s, id, "PinY"), (Some("LocPinY+AlignMiddle-Height/2".to_owned()), Some(1.5), Some("IN".to_owned())));
    assert_eq!(rows_from(&s, id), vec![("AlignLeft".to_owned(), 1, g, "Angle".to_owned(), 4)]);
    // The guide moved to 5: the cell is 5, the pin 6, and the shape's height on the page is as it was.
    run(&mut s, "guide.move", json!({"id": g, "at": 5.0}));
    assert!(near(value(&s, id, "AlignLeft"), 5.0) && near(value(&s, id, "PinX"), 6.0) && near(value(&s, id, "PinY"), 1.5));
    assert_eq!(rows_from(&s, id).len(), 1, "the glue stays");
    // A write to the guide's pin by any command is a move of it: `shape.move` on the guide.
    run(&mut s, "shape.move", json!({"ids": [g], "dx": -0.5, "dy": 0.25}));
    assert!(near(value(&s, id, "PinX"), 5.5) && near(value(&s, id, "PinY"), 1.5), "an upright guide carries it along x only");
    assert_eq!(check_session(&s), Ok(()));
    // Undone, the move and the glue go in their steps.
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert!(rows_from(&s, id).is_empty());
    assert!(SIX.iter().all(|n| formula(&s, id, n).is_none()), "no alignment cell before the glue");
}

#[test]
fn a_box_held_by_its_middle_to_a_level_guide_has_part_5_and_follows_it_up() {
    // The second half: held by its middle to a level guide, the row is from `AlignMiddle`, part 5; `AlignCenter` names the guide and has no row.
    let mut s = guides_only();
    let g = guide(&mut s, "horizontal", 4.0);
    let id = box_at(&mut s, 3.0, 2.0);
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 2.03, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["cell"], json!("AlignMiddle"), "{out}");
    assert!(near(value(&s, id, "PinY"), 4.0));
    assert_eq!(local(&s, id, "AlignMiddle").0, Some(single("INTERSECTY", g, ("3", "4"))));
    assert_eq!(local(&s, id, "AlignCenter").0, Some(single("INTERSECTX", g, ("3", "4"))));
    assert_markers(&s, id, &["AlignMiddle", "AlignCenter"]);
    assert_eq!(formula(&s, id, "PinX").as_deref(), Some("LocPinX+AlignCenter-Width/2"));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignMiddle-Height/2"));
    assert_eq!(rows_from(&s, id), vec![("AlignMiddle".to_owned(), 5, g, "Angle".to_owned(), 4)]);
    run(&mut s, "guide.move", json!({"id": g, "at": 6.25}));
    assert!(near(value(&s, id, "PinY"), 6.25) && near(value(&s, id, "PinX"), 3.0));
}

#[test]
fn the_right_and_top_edges_take_the_whole_size_and_a_crossing_names_both_guides() {
    // `AlignRight` 6, `Width` 3, `LocPinX` 1.5 give `PinX` 4.5; `AlignTop` 9, `Height` 2, `LocPinY` 1 give `PinY` 8. Held at the crossing of
    // the two guides, both cells name both guides in one order (upright first) and each has its row.
    let mut s = guides_only();
    let a = guide(&mut s, "vertical", 6.0);
    let b = guide(&mut s, "horizontal", 9.0);
    let id = sized_box(&mut s, (3.0, 5.0), (3.0, 2.0));
    // Right edge 4.5 -> 5.97, top 6 -> 8.96.
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.47, "dy": 2.96, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    assert!(near(value(&s, id, "PinX"), 4.5) && near(value(&s, id, "PinY"), 8.0), "{} {}", value(&s, id, "PinX"), value(&s, id, "PinY"));
    assert_eq!(local(&s, id, "AlignRight"), (Some(crossing("INTERSECTX", a, b)), Some(6.0), None));
    assert_eq!(local(&s, id, "AlignTop"), (Some(crossing("INTERSECTY", a, b)), Some(9.0), None));
    assert_markers(&s, id, &["AlignRight", "AlignTop"]);
    assert_eq!(formula(&s, id, "PinX").as_deref(), Some("LocPinX+AlignRight-Width"));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignTop-Height"));
    let mut rows = rows_from(&s, id);
    rows.sort();
    assert_eq!(rows, vec![("AlignRight".to_owned(), 3, a, "Angle".to_owned(), 4), ("AlignTop".to_owned(), 6, b, "Angle".to_owned(), 4)]);
    // Either guide moved carries its own axis.
    run(&mut s, "guide.move", json!({"id": a, "at": 7.0}));
    assert!(near(value(&s, id, "PinX"), 5.5) && near(value(&s, id, "PinY"), 8.0));
    run(&mut s, "guide.move", json!({"id": b, "at": 10.0}));
    assert!(near(value(&s, id, "PinX"), 5.5) && near(value(&s, id, "PinY"), 9.0));
}

#[test]
fn a_box_held_at_a_crossing_by_its_centre_and_middle_follows_both_guides() {
    // An upright guide at 2.5 and a level one at 4; `AlignCenter` 2.5 and `AlignMiddle` 4, `PinX` 2.5 and `PinY` 4.
    let mut s = guides_only();
    let a = guide(&mut s, "vertical", 2.5);
    let b = guide(&mut s, "horizontal", 4.0);
    let id = box_at(&mut s, 2.46, 4.05);
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    assert_eq!(local(&s, id, "AlignCenter"), (Some(crossing("INTERSECTX", a, b)), Some(2.5), None));
    assert_eq!(local(&s, id, "AlignMiddle"), (Some(crossing("INTERSECTY", a, b)), Some(4.0), None));
    assert_eq!(formula(&s, id, "PinX").as_deref(), Some("LocPinX+AlignCenter-Width/2"));
    assert!(near(value(&s, id, "PinX"), 2.5) && near(value(&s, id, "PinY"), 4.0));
    // Each row's parts (`AlignCenter` 2, `AlignMiddle` 5), not their count alone.
    let mut rows = rows_from(&s, id);
    rows.sort();
    assert_eq!(rows, vec![("AlignCenter".to_owned(), 2, a, "Angle".to_owned(), 4), ("AlignMiddle".to_owned(), 5, b, "Angle".to_owned(), 4)]);
    run(&mut s, "guide.move", json!({"id": a, "at": 3.0}));
    run(&mut s, "guide.move", json!({"id": b, "at": 5.0}));
    assert!(near(value(&s, id, "PinX"), 3.0) && near(value(&s, id, "PinY"), 5.0));
}

#[test]
fn a_turned_guide_carries_its_shape_to_the_foot_of_the_square_line() {
    // Our reasoning: a level guide through (0, 0) holding a box by its middle at (2, 0), turned to 45 degrees about its pin: the
    // box's middle goes to (1, 1), the foot of the line square to the guide through the place the glue wrote.
    let mut s = guides_only();
    let g = guide(&mut s, "horizontal", 0.0);
    let id = box_at(&mut s, 2.0, 0.04);
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["cell"], json!("AlignMiddle"), "{out}");
    run(&mut s, "shape.rotate", json!({"ids": [g], "angle": 45.0}));
    assert!(near(value(&s, id, "AlignCenter"), 1.0) && near(value(&s, id, "AlignMiddle"), 1.0), "{}", value(&s, id, "AlignCenter"));
    assert!(near(value(&s, id, "PinX"), 1.0) && near(value(&s, id, "PinY"), 1.0));
    // The guide is now at an angle: it is not a snap target and not listed, and the glue stays.
    assert_eq!(run(&mut s, "guide.list", json!({}))["left_out"], json!(1));
    assert_eq!(rows_from(&s, id).len(), 1);
}

#[test]
fn a_drop_a_resize_and_a_place_pulled_to_a_guide_glue_as_a_move_does() {
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 4.0);
    // A drop whose right edge lands 0.03 short of the guide (the rectangle is 1.5 wide).
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.22, "y": 2.0, "snap": true}));
    let dropped = id_of(&r);
    assert_eq!(r["snap"]["held"], json!([dropped]), "{r}");
    assert_eq!(formula(&s, dropped, "PinX").as_deref(), Some("LocPinX+AlignRight-Width"));
    assert!(near(value(&s, dropped, "PinX"), 3.25));
    // A resize that drags the right edge of another box to the guide holds that edge.
    let resized = box_at(&mut s, 2.0, 5.0);
    let out = run(&mut s, "shape.resize", json!({"id": resized, "x": 1.0, "y": 4.5, "width": 2.96, "height": 1.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([resized]), "{out}");
    assert_eq!(rows_from(&s, resized), vec![("AlignRight".to_owned(), 3, g, "Angle".to_owned(), 4)]);
    assert!(near(value(&s, resized, "Width"), 3.0));
    // A place whose pin puts the left edge 0.02 off the guide.
    let placed = box_at(&mut s, 7.0, 8.0);
    let out = run(&mut s, "shape.place", json!({"ids": [placed], "x": 5.02, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([placed]), "{out}");
    assert_eq!(rows_from(&s, placed)[0].0, "AlignLeft");
    // All three follow the guide.
    run(&mut s, "guide.move", json!({"id": g, "at": 4.5}));
    // The dropped box's right edge, the resized box's right edge (x 1 to 4, its pin 2.5) and the placed box's left edge are on the guide.
    let pins = [value(&s, dropped, "PinX"), value(&s, resized, "PinX"), value(&s, placed, "PinX")];
    assert!(near(pins[0], 3.75) && near(pins[1], 3.0) && near(pins[2], 5.5), "{pins:?}");
    assert!(near(value(&s, resized, "Width"), 3.0), "a guide moves its shapes and does not resize them");
}

#[test]
fn a_shape_whose_pin_is_not_in_its_middle_stays_where_it_is_when_held() {
    // The place in the formulas is the middle of the box: with the pin at the bottom-left corner (`LocPinY` 0), `LocPinY+AlignMiddle-Height/2` keeps the
    // shape still only if `AlignMiddle` is its middle. The pin's own place (the words) would drop it by half its height.
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    let id = box_at(&mut s, 6.0, 4.0);
    run(&mut s, "shape.pin", json!({"ids": [id], "at": "bottom-left"}));
    assert!(near(value(&s, id, "PinX"), 5.0) && near(value(&s, id, "PinY"), 3.5));
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": -1.97, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    assert!(near(value(&s, id, "PinX"), 3.0) && near(value(&s, id, "PinY"), 3.5), "{} {}", value(&s, id, "PinX"), value(&s, id, "PinY"));
    assert_eq!(local(&s, id, "AlignMiddle").0, Some(single("INTERSECTY", g, ("4", "4"))));
    run(&mut s, "guide.move", json!({"id": g, "at": 2.0}));
    assert!(near(value(&s, id, "PinX"), 2.0) && near(value(&s, id, "PinY"), 3.5));
}

#[test]
fn several_shapes_moved_together_are_held_where_their_own_edge_is_on_the_guide() {
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    // Two boxes whose left edges are at 5, one above the other, and a third whose left edge is at 5.5.
    let (a, b) = (box_at(&mut s, 6.0, 2.0), box_at(&mut s, 6.0, 4.0));
    let c = box_at(&mut s, 6.5, 6.0);
    let out = run(&mut s, "shape.move", json!({"ids": [a, b, c], "dx": -1.97, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([a, b]), "{out}");
    assert!(rows_from(&s, c).is_empty() && formula(&s, c, "AlignLeft").is_none());
    run(&mut s, "guide.move", json!({"id": g, "at": 2.0}));
    assert!(near(value(&s, a, "PinX"), 3.0) && near(value(&s, b, "PinX"), 3.0) && near(value(&s, c, "PinX"), 4.5));
}

#[test]
fn what_is_not_held_turned_shapes_lines_guides_and_glue_turned_off() {
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    // A turned box is pulled by its box and not held (the forms have no turn). Pulled by its middle, which is the middle of its own frame too,
    // so that only the turn keeps it from being held.
    let turned = box_at(&mut s, 6.0, 2.0);
    run(&mut s, "shape.rotate", json!({"ids": [turned], "angle": 30.0}));
    let out = run(&mut s, "shape.move", json!({"ids": [turned], "dx": -2.96, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["anchor"], json!("middle"), "{out}");
    assert!(near(value(&s, turned, "PinX"), 3.0), "pulled onto the guide by its middle");
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    assert!(rows_from(&s, turned).is_empty());
    // A connector's end is not written to a guide (read and kept, never written).
    let c = run(&mut s, "connector.add", json!({"from": {"x": 3.03, "y": 7.0}, "to": {"x": 5.0, "y": 7.0}, "snap": true}));
    let line = id_of(&c);
    assert!(rows_from(&s, line).is_empty() && formula(&s, line, "BeginX").is_none(), "{c}");
    // Nor is a line moved with its box's edge on the guide: its pin follows its ends.
    let out = run(&mut s, "shape.move", json!({"ids": [line], "dx": 0.0, "dy": 0.5, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["guide"], json!(g), "pulled: {out}");
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    assert!(rows_from(&s, line).is_empty() && formula(&s, line, "AlignLeft").is_none());
    assert_eq!(formula(&s, line, "PinX").as_deref(), Some("(BeginX+EndX)/2"));
    // Glue off, or glue to guides off: pulled by the guide (the hit is a guide's), and not held. Each box is taken away after, so that the next is
    // pulled by the guide and not by the corners of the one before.
    for off in [json!({"glue": false}), json!({"glue": true, "glue_guides": false})] {
        run(&mut s, "snap.set", off.clone());
        let id = box_at(&mut s, 6.0, 9.0);
        let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": -1.96, "dy": 0.0, "snap": true}));
        assert_eq!(out["snap"]["hits"][0]["target"], json!("guide"), "{off}: {out}");
        assert_eq!(out["snap"]["holds"], json!([]), "{off}: {out}");
        assert!(near(value(&s, id, "PinX"), 4.0), "{off}: still pulled");
        assert!(rows_from(&s, id).is_empty() && formula(&s, id, "AlignLeft").is_none(), "{off}");
        run(&mut s, "shape.delete", json!({"ids": [id]}));
    }
    // A guide moved with `snap` to another guide is not held to it: an upright one (which is turned a quarter, and would not be held anyway) and a
    // level one, which is not turned.
    run(&mut s, "snap.set", json!({"glue": true, "glue_guides": true}));
    let h = guide(&mut s, "vertical", 5.0);
    let out = run(&mut s, "shape.move", json!({"ids": [h], "dx": -1.97, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    assert!(rows_from(&s, h).is_empty() && formula(&s, h, "AlignLeft").is_none() && g != h);
    let (a, b) = (guide(&mut s, "horizontal", 10.0), guide(&mut s, "horizontal", 10.5));
    let out = run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": -0.47, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["guide"], json!(a), "pulled: {out}");
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    assert!(rows_from(&s, b).is_empty() && SIX.iter().all(|n| formula(&s, b, n).is_none()));
}

#[test]
fn snap_at_tells_the_holds_and_changes_nothing() {
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    let before = snapshot(&s);
    let at = run(&mut s, "snap.at", json!({"page": 1, "left": 3.04, "bottom": 1.0, "right": 5.04, "top": 2.0}));
    assert_eq!(at["holds"], json!([{"guide": g, "axis": "x", "anchor": "low", "cell": "AlignLeft"}]), "{at}");
    // A point is no shape: nothing holds.
    let at = run(&mut s, "snap.at", json!({"page": 1, "x": 3.04, "y": 1.0}));
    assert_eq!(at["holds"], json!([]), "{at}");
    assert_eq!(snapshot(&s).document, before.document);
}

// ---------------------------------------------------------------------------------------------------------------------- what breaks a hold

/// The shape is held no more: no row, the two cells that named the guide hold their values and no formula (a write breaks the hold),
/// and the guide moves without it.
fn assert_released(s: &mut Session, g: u32, id: u32, cells: [(&str, f64); 2]) {
    assert!(rows_from(s, id).is_empty(), "{:?}", rows_from(s, id));
    for (name, v) in cells {
        assert_eq!(local(s, id, name), (None, Some(v), None), "{name} keeps its value");
    }
    let pin = (value(s, id, "PinX"), value(s, id, "PinY"));
    run(s, "guide.move", json!({"id": g, "at": 1.0}));
    assert_eq!((value(s, id, "PinX"), value(s, id, "PinY")), pin, "the guide moves without it");
    // No formula names the guide, so it can go.
    run(s, "guide.delete", json!({"id": g}));
    assert_eq!(check_session(s), Ok(()));
}

#[test]
fn a_move_without_snap_breaks_the_hold() {
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.5, "dy": 0.0}));
    assert!(near(value(&s, id, "PinX"), 4.5));
    assert_eq!(formula(&s, id, "PinX"), None, "the move wrote the pin");
    assert_markers(&s, id, &["AlignLeft", "AlignMiddle"]);
    assert_released(&mut s, g, id, [("AlignLeft", 3.0), ("AlignMiddle", 1.5)]);
}

#[test]
fn a_move_with_snap_that_is_not_pulled_to_the_guide_breaks_the_hold() {
    let (mut s, g, id) = held_by_its_left_edge();
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 2.0, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    assert!(near(value(&s, id, "PinX"), 6.0));
    assert_released(&mut s, g, id, [("AlignLeft", 3.0), ("AlignMiddle", 1.5)]);
}

#[test]
fn a_write_to_one_pin_breaks_the_hold_of_the_whole_shape() {
    // `shape.place` with `x` only writes `PinX`; `PinY` keeps its formula, which now reads a value, so the shape stays where it is.
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.place", json!({"ids": [id], "x": 4.5}));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignMiddle-Height/2"));
    assert_released(&mut s, g, id, [("AlignLeft", 3.0), ("AlignMiddle", 1.5)]);
}

#[test]
fn a_move_along_the_guide_with_snap_keeps_the_hold_at_the_new_place() {
    let (mut s, g, id) = held_by_its_left_edge();
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 1.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    assert_eq!(local(&s, id, "AlignLeft").0, Some(single("INTERSECTX", g, ("4", "2.5"))));
    assert_eq!(rows_from(&s, id).len(), 1, "one row, not two");
    assert!(near(value(&s, id, "PinY"), 2.5));
    run(&mut s, "guide.move", json!({"id": g, "at": 3.5}));
    assert!(near(value(&s, id, "PinX"), 4.5));
}

#[test]
fn a_hold_renewed_where_the_shape_is_writes_nothing_and_a_new_guide_under_it_makes_it_a_crossing() {
    let (mut s, g, id) = held_by_its_left_edge();
    // Placed with `snap` where it is: the snap holds it as it is held, nothing is written, `held` names no shape (it tells what the command wrote), and
    // the call is no undo step.
    let kept = snapshot(&s);
    let out = run(&mut s, "shape.place", json!({"ids": [id], "x": 4.0, "snap": true}));
    assert_eq!(out["snap"]["holds"][0]["guide"], json!(g), "{out}");
    assert_eq!(out["snap"]["held"], json!([]), "{out}");
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps, now.revision), (kept.document, kept.undo_steps, kept.revision));
    // A level guide laid under its bottom edge, and the shape placed where it is: its pins are not written, and it is held at the crossing now, its rows
    // replaced (the one from `AlignLeft` is not left beside the new one).
    let level = guide(&mut s, "horizontal", 1.0);
    let out = run(&mut s, "shape.place", json!({"ids": [id], "x": 4.0, "snap": true}));
    assert_eq!(out["snap"]["holds"].as_array().map(Vec::len), Some(2), "{out}");
    assert_eq!(out["snap"]["held"], json!([id]), "written: {out}");
    let mut rows = rows_from(&s, id);
    rows.sort();
    assert_eq!(rows, vec![("AlignBottom".to_owned(), 4, level, "Angle".to_owned(), 4), ("AlignLeft".to_owned(), 1, g, "Angle".to_owned(), 4)]);
    assert_eq!(local(&s, id, "AlignLeft").0, Some(crossing("INTERSECTX", g, level)));
    assert_eq!(formula(&s, id, "AlignMiddle").as_deref(), Some("_MARKER(1)"));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignBottom"));
    run(&mut s, "guide.move", json!({"id": level, "at": 2.0}));
    assert!(near(value(&s, id, "PinY"), 2.5) && near(value(&s, id, "PinX"), 4.0));
}

#[test]
fn a_resize_with_snap_keeps_the_hold_of_an_edge_it_does_not_move_and_one_without_breaks_it() {
    let (mut s, g, id) = held_by_its_left_edge();
    // The right edge dragged from 5 to 5.5, far from anything: the left edge stays on the guide, and stays held.
    let out = run(&mut s, "shape.resize", json!({"id": id, "x": 3.0, "y": 1.0, "width": 2.5, "height": 1.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    assert_eq!(rows_from(&s, id).len(), 1);
    assert!(near(value(&s, id, "PinX"), 4.25));
    run(&mut s, "guide.move", json!({"id": g, "at": 3.5}));
    assert!(near(value(&s, id, "PinX"), 4.75) && near(value(&s, id, "Width"), 2.5));
    // The left edge dragged off it breaks the hold.
    run(&mut s, "shape.resize", json!({"id": id, "x": 2.0, "y": 1.0, "width": 3.5, "height": 1.0, "snap": true}));
    assert!(rows_from(&s, id).is_empty());
    // Held again, and resized without `snap`: released.
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.47, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 1);
    run(&mut s, "shape.resize", json!({"id": id, "x": 3.5, "y": 1.0, "width": 3.0, "height": 1.0}));
    assert!(rows_from(&s, id).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------- deleting

#[test]
fn deleting_a_held_shape_takes_its_rows_and_frees_the_guide() {
    let (mut s, g, id) = held_by_its_left_edge();
    let kept = snapshot(&s);
    let err = s.execute("guide.delete", &json!({"id": g})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err}");
    assert_eq!(snapshot(&s).document, kept.document, "refused, nothing changed");
    run(&mut s, "shape.delete", json!({"ids": [id]}));
    assert!(s.document().unwrap().page(1).unwrap().connects.is_empty());
    run(&mut s, "guide.delete", json!({"id": g}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn every_copy_of_a_held_shape_is_released_one_left_in_place_too() {
    // No form of is two guide cells with no row, and a copy gets no row (only rows between copies are copied). So every copy is
    // released: put elsewhere or where its original is, its two guide cells hold their values, nothing of it names the guide, and it stays put.
    let (mut s, g, id) = held_by_its_left_edge();
    let mut copies = Vec::new();
    for dy in [3.0, 0.0] {
        let copy = run(&mut s, "shape.duplicate", json!({"ids": [id], "dx": 0.0, "dy": dy}));
        let c = u32::try_from(copy["ids"][0].as_u64().unwrap()).unwrap();
        assert!(rows_from(&s, c).is_empty(), "{dy}");
        assert_eq!(local(&s, c, "AlignLeft"), (None, Some(3.0), None), "{dy}");
        assert_eq!(local(&s, c, "AlignMiddle"), (None, Some(1.5), None), "{dy}");
        assert_markers(&s, c, &["AlignLeft", "AlignMiddle"]);
        assert!(near(value(&s, c, "PinX"), 4.0) && near(value(&s, c, "PinY"), 1.5 + dy), "{dy}");
        copies.push(c);
    }
    // A guide copied with its shape: the copy of the row between them is taken out with the copy's hold.
    let both = run(&mut s, "shape.duplicate", json!({"ids": [g, id], "dx": 0.0, "dy": 5.0}));
    let pair: Vec<u32> = both["ids"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect();
    assert!(pair.iter().all(|c| rows_from(&s, *c).is_empty() && SIX.iter().all(|n| formula(&s, *c, n).is_none_or(|f| f == "_MARKER(1)"))));
    // The original is still held and follows the guide; the copies do not.
    assert_eq!(rows_from(&s, id).len(), 1);
    run(&mut s, "guide.move", json!({"id": g, "at": 3.5}));
    assert!(near(value(&s, id, "PinX"), 4.5));
    assert!(copies.iter().all(|c| near(value(&s, *c, "PinX"), 4.0)));
    // Once the original is deleted nothing names the guide, and it can go.
    run(&mut s, "shape.delete", json!({"ids": [id]}));
    run(&mut s, "guide.delete", json!({"id": g}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn deleting_a_guide_as_a_shape_releases_what_it_held() {
    // `shape.delete` takes any shape, a guide too: the shapes it held stay where they are, released, and no formula is left naming it.
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.delete", json!({"ids": [g]}));
    assert!(rows_from(&s, id).is_empty());
    assert_eq!(local(&s, id, "AlignLeft"), (None, Some(3.0), None));
    assert_eq!(local(&s, id, "AlignMiddle"), (None, Some(1.5), None));
    assert!(near(value(&s, id, "PinX"), 4.0) && near(value(&s, id, "PinY"), 1.5));
    assert_eq!(check_session(&s), Ok(()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(rows_from(&s, id).len(), 1, "undone, the guide and the glue are back");
}

// ---------------------------------------------------------------------------------------------------------------------- real scales and files

#[test]
fn a_scaled_drawing_writes_the_place_in_drawing_units_and_follows_in_paper_inches() {
    // A metric drawing at 1:100: a paper inch is 100 drawing inches. The guide's place and the shape's are cells in drawing units, and the place in the
    // glued formula is a drawing length (`DL`).
    let mut s = guides_only();
    run(&mut s, "page.scale", json!({"preset": "metric_1_100"}));
    let k = s.document().map(|d| diacraft_engine::draw::units_per_inch(d, 1)).unwrap();
    assert!(near(k, 100.0), "{k}");
    let g = guide(&mut s, "vertical", 2.0);
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 3.0}));
    let id = id_of(&r);
    let w = r["width"].as_f64().unwrap();
    // The left edge, at 5 - w/2 paper inches, moved to 0.03 right of the guide.
    let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": 2.03 - (5.0 - w / 2.0), "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([id]), "{out}");
    let middle_x = (2.0 + w / 2.0) * k;
    let text = |n: f64| diacraft_engine::stencil::number_text(n);
    assert_eq!(local(&s, id, "AlignLeft").0, Some(single("INTERSECTX", g, (&text(middle_x), &text(300.0)))));
    assert!(near(value(&s, id, "AlignLeft"), 200.0), "drawing units: {}", value(&s, id, "AlignLeft"));
    run(&mut s, "guide.move", json!({"id": g, "at": 2.5}));
    assert!(near(value(&s, id, "PinX") / k, 2.5 + w / 2.0), "{}", value(&s, id, "PinX") / k);
}

/// A page part as a file holds glue to guides, written by hand: an upright
/// guide 7 at x 3 and a level guide 8 at y 6 (in millimetres, as a metric file writes its `U`), box 1 held by its left edge to 7, box 2 held at the
/// crossing by its centre and middle, and box 3 glued to nothing.
fn glued_file() -> Vec<u8> {
    let guide_cells = |pin: (&str, &str), angle: &str| {
        format!(
            r#"<Cell N="PinX" V="{}" U="MM"/><Cell N="PinY" V="{}" U="MM"/><Cell N="Width" V="0"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="{angle}"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="InfiniteLine" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="1"/><Cell N="B" V="0"/></Row></Section>"#,
            pin.0, pin.1
        )
    };
    let upright = shape_with(7, r#" Type="Guide""#, &guide_cells(("3", "0"), "1.5707963267949"));
    let level = shape_with(8, r#" Type="Guide""#, &guide_cells(("0", "6"), "0"));
    let marker = |n: &str| format!(r#"<Cell N="{n}" V="1" F="_MARKER(1)"/>"#);
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#;
    let one = shape(
        1,
        &format!(
            r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="1.5" F="LocPinY+AlignMiddle-Height/2"/>{size}<Cell N="AlignLeft" V="3" F="INTERSECTX(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg)"/>{}{}{}{}<Cell N="AlignMiddle" V="1.5" F="INTERSECTY(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg)"/>"#,
            marker("AlignCenter"),
            marker("AlignRight"),
            marker("AlignTop"),
            marker("AlignBottom"),
        ),
    );
    let two = shape(
        2,
        &format!(
            r#"<Cell N="PinX" V="3" F="LocPinX+AlignCenter-Width/2"/><Cell N="PinY" V="6" F="LocPinY+AlignMiddle-Height/2"/>{size}{}<Cell N="AlignCenter" V="3" F="INTERSECTX(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,Sheet.8!PinX,Sheet.8!PinY,Sheet.8!Angle)"/>{}{}<Cell N="AlignMiddle" V="6" F="INTERSECTY(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,Sheet.8!PinX,Sheet.8!PinY,Sheet.8!Angle)"/>{}"#,
            marker("AlignLeft"),
            marker("AlignRight"),
            marker("AlignTop"),
            marker("AlignBottom"),
        ),
    );
    let three = shape(3, &format!(r#"<Cell N="PinX" V="7"/><Cell N="PinY" V="9"/>{size}"#));
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/><Connect FromSheet="2" FromCell="AlignCenter" FromPart="2" ToSheet="7" ToCell="Angle" ToPart="4"/><Connect FromSheet="2" FromCell="AlignMiddle" FromPart="5" ToSheet="8" ToCell="Angle" ToPart="4"/>"#;
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{upright}{level}{one}{two}{three}")).connects(connects)).build()
}

fn scratch(name: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("diacraft-snap-glue-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_str().unwrap().to_owned()
}

/// The text of the page part of the drawing at `path`.
fn page_xml(path: &str) -> String {
    let bytes = std::fs::read(path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(pkg.part("/visio/pages/page1.xml").unwrap().to_vec()).unwrap()
}

/// The element `<Shape ID="id" ...>...</Shape>` of a page part (the shapes here hold no shapes), and the `Connects` element.
fn element(xml: &str, start: &str, end: &str) -> String {
    let from = xml.find(start).unwrap_or_else(|| panic!("no {start} in {xml}"));
    let to = xml[from..].find(end).unwrap() + from + end.len();
    xml[from..to].to_owned()
}

#[test]
fn a_file_s_glue_opens_follows_a_moved_guide_and_an_unrelated_edit_keeps_its_bytes() {
    let path = scratch("glued.vsdx");
    std::fs::write(&path, glued_file()).unwrap();
    let original = page_xml(&path);
    // The guides move their shapes, by the file's own formulas.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let k = s.document().map(|d| diacraft_engine::draw::units_per_inch(d, 1)).unwrap();
    run(&mut s, "guide.move", json!({"id": 7, "at": 4.0 / k}));
    assert!(near(value(&s, 1, "PinX"), 5.0) && near(value(&s, 2, "PinX"), 4.0) && near(value(&s, 2, "PinY"), 6.0));
    run(&mut s, "guide.move", json!({"id": 8, "at": 7.0 / k}));
    assert!(near(value(&s, 2, "PinY"), 7.0) && near(value(&s, 1, "PinY"), 1.5));
    assert_eq!(rows_from(&s, 2).len(), 2);
    // An unrelated edit (box 3 moved) leaves the glued shapes, the guides and the rows byte for byte.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    run(&mut t, "shape.move", json!({"ids": [3], "dx": 0.5, "dy": 0.0}));
    let copy = scratch("glued-copy.vsdx");
    run(&mut t, "file.save_as", json!({"path": copy}));
    let saved = page_xml(&copy);
    for start in [r#"<Shape ID="1""#, r#"<Shape ID="2""#, r#"<Shape ID="7""#, r#"<Shape ID="8""#] {
        assert_eq!(element(&saved, start, "</Shape>"), element(&original, start, "</Shape>"), "{start}");
    }
    assert_eq!(element(&saved, "<Connects>", "</Connects>"), element(&original, "<Connects>", "</Connects>"));
    assert_ne!(element(&saved, r#"<Shape ID="3""#, "</Shape>"), element(&original, r#"<Shape ID="3""#, "</Shape>"), "box 3 moved");
    // And the file's glue is broken by a move of its own as ours is: the rows of box 2 go and its cells keep their values.
    run(&mut t, "shape.move", json!({"ids": [2], "dx": 0.5, "dy": 0.5}));
    assert!(rows_from(&t, 2).is_empty() && rows_from(&t, 1).len() == 1);
    assert_eq!(local(&t, 2, "AlignCenter"), (None, Some(3.0), None));
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn hostile_glue_in_a_file_is_released_without_a_crash() {
    // Formulas that do not parse, a row from a cell that holds no guide, a row to a shape that is not there, and a pin that names a missing cell.
    let cells = r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="1.5" F="LocPinY+(("/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="AlignLeft" V="3" F="INTERSECTX(Sheet.7!PinX,"/><Cell N="AlignTop" V="x" F="INTERSECTY(Sheet.99!PinX,Sheet.99!PinY,Sheet.99!Angle,1,2,3)"/>"#;
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/><Connect FromSheet="1" FromCell="AlignBottom" FromPart="4" ToSheet="99" ToCell="Angle" ToPart="4"/><Connect FromSheet="1" FromCell="AlignRight" ToSheet="4294967295"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape(1, cells)).connects(connects)).build();
    let path = scratch("hostile.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false, "dynamic_grid": false}));
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.25, "dy": 0.0, "snap": true}));
    assert!(rows_from(&s, 1).is_empty(), "{:?}", rows_from(&s, 1));
    assert_eq!(check_session(&s), Ok(()));
}

/// The attribute `name` of the first element that starts with `start` in `xml`.
fn attr(xml: &str, start: &str, name: &str) -> Option<String> {
    let from = xml.find(start)?;
    let element = &xml[from..from + xml[from..].find("/>")?];
    let key = format!(" {name}=\"");
    let at = element.find(&key)? + key.len();
    Some(element[at..at + element[at..].find('"')?].to_owned())
}

#[test]
fn our_glue_is_saved_in_the_forms_of_the_spec_and_follows_its_guide_when_reopened() {
    let (mut s, g, id) = held_by_its_left_edge();
    let path = scratch("ours.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let xml = page_xml(&path);
    let shape_xml = element(&xml, &format!(r#"<Shape ID="{id}""#), "</Shape>");
    // The glued cell: its formula, a numeric `V` and no `U`; a marker cell likewise.
    let left = r#"<Cell N="AlignLeft""#;
    assert_eq!(attr(&shape_xml, left, "F"), Some(single("INTERSECTX", g, ("4", "1.5"))), "{shape_xml}");
    assert_eq!(attr(&shape_xml, left, "V").as_deref(), Some("3"));
    assert_eq!(attr(&shape_xml, left, "U"), None);
    let marker = r#"<Cell N="AlignTop""#;
    assert_eq!(
        (attr(&shape_xml, marker, "F").as_deref(), attr(&shape_xml, marker, "V").as_deref(), attr(&shape_xml, marker, "U")),
        (Some("_MARKER(1)"), Some("1"), None)
    );
    assert_eq!(attr(&shape_xml, r#"<Cell N="PinX""#, "F").as_deref(), Some("LocPinX+AlignLeft"));
    // The row.
    let row = format!(r#"<Connect FromSheet="{id}""#);
    let got: Vec<Option<String>> = ["FromCell", "FromPart", "ToSheet", "ToCell", "ToPart"].iter().map(|n| attr(&xml, &row, n)).collect();
    let want: Vec<Option<String>> = ["AlignLeft", "1", &g.to_string(), "Angle", "4"].iter().map(|v| Some((*v).to_owned())).collect();
    assert_eq!(got, want, "{xml}");
    // Reopened, the guide still carries the shape.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    run(&mut t, "guide.move", json!({"id": g, "at": 4.0}));
    assert!(near(value(&t, id, "PinX"), 5.0) && near(value(&t, id, "PinY"), 1.5));
    assert_eq!(rows_from(&t, id).len(), 1);
    assert_eq!(check_session(&t), Ok(()));
}

/// Whether every leg of a route is level or upright.
fn square_legs(points: &[(f64, f64)]) -> bool {
    points.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 1e-6 || (w[0].1 - w[1].1).abs() < 1e-6)
}

fn near_p(a: (f64, f64), b: (f64, f64)) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// Whether an alignment cell of a shape of page 1 names shape `id` with `Sheet.<id>!`.
fn named_anywhere(s: &Session, id: u32) -> bool {
    let needle = format!("Sheet.{id}!");
    let doc = s.document().unwrap();
    let mut stack: Vec<&diacraft_doc::Shape> = doc.page(1).unwrap().shapes.iter().map(|sh| &**sh).collect();
    while let Some(shape) = stack.pop() {
        if SIX.iter().any(|n| shape.sheet.cell(n).and_then(|c| c.formula.text()).is_some_and(|f| f.contains(&needle))) {
            return true;
        }
        stack.extend(shape.children.iter().map(|sh| &**sh));
    }
    false
}

#[test]
fn a_guide_moved_or_turned_routes_again_the_connectors_glued_to_the_shapes_it_carries() {
    // A guide at 3, a rectangle A held by its left edge, a rectangle B at (9, 6), a connector from A's point 3 (its right side) to B's
    // point 1 (its left side), and the guide moved to 1. The connector's begin follows A; its route is made again from its ends, with level and upright
    // legs, and its far end stays on B's point (8.25, 6). The same when the guide is moved as a shape and turned.
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.78, "y": 6.0, "snap": true}));
    let a = id_of(&r);
    assert_eq!(r["snap"]["held"], json!([a]), "{r}");
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 6.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let check = |s: &Session, what: &str| {
        let points = crate::edit::route(s.document().unwrap(), c);
        let a_right = (value(s, a, "PinX") + 0.75, value(s, a, "PinY"));
        assert!(square_legs(&points), "{what}: a slanted leg in {points:?}");
        assert!(near_p(points[0], a_right), "{what}: begins at {:?}, A's point is at {a_right:?}", points[0]);
        assert!(near_p(points[points.len() - 1], (8.25, 6.0)), "{what}: ends at {:?}", points[points.len() - 1]);
    };
    check(&s, "glued");
    run(&mut s, "guide.move", json!({"id": g, "at": 1.0}));
    assert!(near(value(&s, a, "PinX"), 1.75));
    check(&s, "guide.move");
    run(&mut s, "shape.move", json!({"ids": [g], "dx": 0.5, "dy": 0.0}));
    assert!(near(value(&s, a, "PinX"), 2.25));
    check(&s, "shape.move of the guide");
    // Turned by ten degrees about its pin, the guide takes A to the foot of the line square to it through A's place.
    let pin = (value(&s, a, "PinX"), value(&s, a, "PinY"));
    run(&mut s, "shape.rotate", json!({"ids": [g], "by": -10.0}));
    assert!(!near_p((value(&s, a, "PinX"), value(&s, a, "PinY")), pin), "A followed the turn");
    check(&s, "shape.rotate of the guide");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_guide_moved_places_again_an_end_glued_to_the_whole_of_a_shape_it_carries() {
    // The file of `edit_foreign.rs`: connector 21 joins the whole of box C (12, at (2, 7), 2 by 1, connection points on its sides) to the whole of the blob
    // D (13, at (8, 7)) with `_WALKGLUE`, which calc cannot work out, so the engine places its ends. C is held by its left edge to a guide at 1, and the
    // guide is moved to 0.5: C follows, the end glued to its whole is placed again at the point nearest D's end, and the route is made again.
    let dir = crate::edit_foreign::Dir::new();
    let mut s = crate::edit_foreign::opened(&dir);
    run(
        &mut s,
        "snap.set",
        json!({"grid": false, "ruler": false, "dynamic_grid": false, "snap_vertices": false, "snap_geometry": false, "intersections": false, "snap_connection_points": false}),
    );
    let g = guide(&mut s, "vertical", 1.0);
    let out = run(&mut s, "shape.move", json!({"ids": [12], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(out["snap"]["held"], json!([12]), "{out}");
    run(&mut s, "guide.move", json!({"id": g, "at": 0.5}));
    let d = s.document().unwrap();
    let (c, dd) = (crate::edit_foreign::pin(d, 12), crate::edit_foreign::pin(d, 13));
    assert!(near_p(c, (1.5, 7.0)), "{c:?}");
    let d_end = crate::edit_foreign::box_nearest(dd, 2.0, 1.0, c);
    let dist = |p: &(f64, f64)| (p.0 - d_end.0).hypot(p.1 - d_end.1);
    let c_end = (0..4).map(|ix| crate::edit_foreign::point_of(c.0, c.1, ix)).min_by(|p, q| dist(p).total_cmp(&dist(q))).unwrap();
    let (begin, end) = crate::edit_foreign::end_of(d, 21);
    assert!(near_p(begin, c_end) && near_p(end, d_end), "{begin:?} {end:?}, not {c_end:?} {d_end:?}");
    let points = crate::edit::route(d, 21);
    assert!(crate::edit_foreign::is_clean(&points) && near_p(points[0], begin) && near_p(points[points.len() - 1], end), "{points:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_to_the_free_axis_holds_the_shape_again_and_one_to_the_glued_axis_or_a_crossing_releases_it() {
    // Held by its left edge, `shape.place {y: 4}` writes only `PinY`, the free axis: the hold is written again at the new place, its row
    // kept, and the guide still carries the shape.
    let (mut s, g, id) = held_by_its_left_edge();
    let row = rows_from(&s, id);
    run(&mut s, "shape.place", json!({"ids": [id], "y": 4.0}));
    assert_eq!(rows_from(&s, id), row, "the row is kept");
    assert_eq!(local(&s, id, "AlignLeft").0, Some(single("INTERSECTX", g, ("4", "4"))));
    assert_eq!(local(&s, id, "AlignMiddle").0, Some(single("INTERSECTY", g, ("4", "4"))));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignMiddle-Height/2"));
    assert!(near(value(&s, id, "PinX"), 4.0) && near(value(&s, id, "PinY"), 4.0));
    run(&mut s, "guide.move", json!({"id": g, "at": 5.0}));
    assert!(near(value(&s, id, "PinX"), 6.0) && near(value(&s, id, "PinY"), 4.0), "the guide carries it");
    // Held to a level guide, the free axis is x.
    let level = guide(&mut s, "horizontal", 9.0);
    let other = box_at(&mut s, 2.0, 9.03);
    run(&mut s, "shape.move", json!({"ids": [other], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, other).len(), 1);
    run(&mut s, "shape.place", json!({"ids": [other], "x": 3.0}));
    assert_eq!(rows_from(&s, other).len(), 1, "a write of x keeps a hold to a level guide");
    run(&mut s, "guide.move", json!({"id": level, "at": 8.0}));
    assert!(near(value(&s, other, "PinY"), 8.0) && near(value(&s, other, "PinX"), 3.0));
    // At a crossing there is no free axis: a write of y alone releases both holds (ours).
    let floor = guide(&mut s, "horizontal", 3.5);
    run(&mut s, "shape.place", json!({"ids": [id], "x": 6.0, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 2, "held at the crossing of {g} and {floor}");
    run(&mut s, "shape.place", json!({"ids": [id], "y": 5.0}));
    assert!(rows_from(&s, id).is_empty());
    assert!(SIX.iter().all(|n| !formula(&s, id, n).is_some_and(|f| f.contains("INTERSECT"))));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn grouping_a_guide_releases_the_shapes_held_to_it_where_they_are() {
    // Grouped, a guide's pin is in the group's frame; the shapes held to it are released first, so they do not jump, and no formula is
    // left naming the guide. Grouped with its shape, and grouped with another shape while its shape stays outside.
    for with_its_shape in [true, false] {
        let (mut s, g, id) = held_by_its_left_edge();
        let other = box_at(&mut s, 8.0, 8.0);
        let before = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
        let ids = if with_its_shape { vec![g, id] } else { vec![g, other] };
        let group = id_of(&run(&mut s, "shape.group", json!({"ids": ids})));
        let after = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
        assert!(before.iter().zip(after.iter()).all(|(p, q)| near_p(*p, *q)), "{with_its_shape}: {before:?} became {after:?}");
        assert!(!named_anywhere(&s, g), "{with_its_shape}");
        assert!(rows_from(&s, id).is_empty(), "{with_its_shape}");
        // Deleted, the group leaves no formula naming a shape that is gone.
        run(&mut s, "shape.delete", json!({"ids": [group]}));
        assert!(!named_anywhere(&s, g), "{with_its_shape}");
        assert_eq!(check_session(&s), Ok(()), "{with_its_shape}");
    }
}

#[test]
fn deleting_any_shape_a_file_s_alignment_cells_name_releases_them() {
    // Every shape that goes releases its holders, a guide or not. A file's box 1 whose cells name box 3 (no guide) and have a row to it.
    let marker = |n: &str| format!(r#"<Cell N="{n}" V="1" F="_MARKER(1)"/>"#);
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    let one = shape(
        1,
        &format!(
            r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="1.5" F="LocPinY+AlignMiddle-Height/2"/>{size}<Cell N="AlignLeft" V="3" F="INTERSECTX(Sheet.3!PinX,Sheet.3!PinY,Sheet.3!Angle,4 DL,1.5 DL,Sheet.3!Angle+90 deg)"/><Cell N="AlignMiddle" V="1.5" F="INTERSECTY(Sheet.3!PinX,Sheet.3!PinY,Sheet.3!Angle,4 DL,1.5 DL,Sheet.3!Angle+90 deg)"/>{}{}{}{}"#,
            marker("AlignCenter"),
            marker("AlignRight"),
            marker("AlignTop"),
            marker("AlignBottom")
        ),
    );
    let three = shape(3, &format!(r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="0"/><Cell N="Angle" V="1.5707963267949"/>{size}"#));
    // Box 2 names box 3 in its cells with no row (the ten shapes with a guide cell and no row).
    let two = shape(
        2,
        &format!(
            r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="5"/>{size}<Cell N="AlignLeft" V="3" F="INTERSECTX(Sheet.3!PinX,Sheet.3!PinY,Sheet.3!Angle,4 DL,5 DL,Sheet.3!Angle+90 deg)"/>"#
        ),
    );
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="3" ToCell="Angle" ToPart="4"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{one}{two}{three}")).connects(connects)).build();
    let path = scratch("named.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "shape.delete", json!({"ids": [3]}));
    assert!(!named_anywhere(&s, 3));
    assert_eq!(local(&s, 1, "AlignLeft"), (None, Some(3.0), None));
    assert_eq!(local(&s, 2, "AlignLeft"), (None, Some(3.0), None), "named with no row");
    assert!(near(value(&s, 1, "PinX"), 4.0) && near(value(&s, 2, "PinX"), 4.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_hold_whose_glued_cell_is_guarded_is_released_by_a_write_to_its_free_axis_and_not_written_again() {
    // A file's box held by its left edge whose `AlignLeft` is `GUARD(INTERSECTX(...))`: a write of `PinY` (its free axis) cannot write the hold again, so the
    // shape is released and the command goes through.
    let marker = |n: &str| format!(r#"<Cell N="{n}" V="1" F="_MARKER(1)"/>"#);
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    let guide_cells = r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="0"/><Cell N="Width" V="0"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="1.5707963267949"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="InfiniteLine" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="1"/><Cell N="B" V="0"/></Row></Section>"#;
    let one = shape(
        1,
        &format!(
            r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="1.5" F="LocPinY+AlignMiddle-Height/2"/>{size}<Cell N="AlignLeft" V="3" F="GUARD(INTERSECTX(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg))"/><Cell N="AlignMiddle" V="1.5" F="INTERSECTY(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg)"/>{}{}{}{}"#,
            marker("AlignCenter"),
            marker("AlignRight"),
            marker("AlignTop"),
            marker("AlignBottom")
        ),
    );
    let seven = shape_with(7, r#" Type="Guide""#, guide_cells);
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{seven}{one}")).connects(connects)).build();
    let path = scratch("guarded.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "shape.place", json!({"ids": [1], "y": 4.0}));
    assert!(rows_from(&s, 1).is_empty());
    assert_eq!(local(&s, 1, "AlignMiddle"), (None, Some(1.5), None));
    assert!(formula(&s, 1, "AlignLeft").is_some_and(|f| f.starts_with("GUARD(")), "a guarded cell is not written");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_that_grows_on_the_left_keeps_every_hold_and_an_unrelated_edit_keeps_the_glue_s_bytes() {
    // Auto Size on, a box held by its left edge (A), one held at a crossing (B), and an unrelated box moved past the page's left edge: the
    // page grows on the left and carries its shapes, the guides carry the shapes held to them, and every hold is kept.
    let (mut s, g, a) = held_by_its_left_edge();
    let level = guide(&mut s, "horizontal", 7.0);
    let b = box_at(&mut s, 4.0, 7.5);
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, b).len(), 2, "B at the crossing");
    let free = box_at(&mut s, 6.0, 4.0);
    run(&mut s, "page.auto_size", json!({"on": true}));
    let width = run(&mut s, "page.setup", json!({}))["width"].as_f64();
    run(&mut s, "shape.move", json!({"ids": [free], "dx": -6.5, "dy": 0.0}));
    let grown = run(&mut s, "page.setup", json!({}))["width"].as_f64();
    assert!(grown > width, "the page grew: {width:?} to {grown:?}");
    let shift = value(&s, g, "PinX") - 3.0;
    assert!(shift > 1.0, "the guide was carried: {shift}");
    for (id, rows) in [(a, 1), (b, 2)] {
        assert_eq!(rows_from(&s, id).len(), rows, "{id} keeps its hold");
        assert!(formula(&s, id, "PinX").is_some_and(|f| f.contains("Align")), "{id}");
        assert!(near(value(&s, id, "PinX"), 4.0 + shift), "{id} was carried with its guide: {}", value(&s, id, "PinX"));
    }
    assert!(near(value(&s, a, "PinY"), 1.5) && near(value(&s, b, "PinY"), 7.5));
    // Grown at the bottom, the page carries them up: A's free axis is y, and its place is carried with it.
    run(&mut s, "shape.move", json!({"ids": [free], "dx": 0.0, "dy": -10.0}));
    let up = value(&s, level, "PinY") - 7.0;
    assert!(up > 1.0, "the page grew at the bottom: {up}");
    assert!(near(value(&s, a, "PinY"), 1.5 + up) && near(value(&s, b, "PinY"), 7.5 + up), "{} {}", value(&s, a, "PinY"), value(&s, b, "PinY"));
    assert!(near(value(&s, a, "PinX"), 4.0 + shift) && rows_from(&s, a).len() == 1 && rows_from(&s, b).len() == 2);
    // They still follow their guides.
    run(&mut s, "guide.move", json!({"id": level, "at": 8.0 + up}));
    assert!(near(value(&s, b, "PinY"), 8.5 + up));
    // Saved, then saved again after an unrelated edit: the glued shapes, the guides and the rows are byte for byte.
    let (first, second) = (scratch("grown.vsdx"), scratch("grown-again.vsdx"));
    run(&mut s, "file.save_as", json!({"path": first}));
    run(&mut s, "shape.move", json!({"ids": [free], "dx": 0.0, "dy": 0.5}));
    run(&mut s, "file.save_as", json!({"path": second}));
    let (x1, x2) = (page_xml(&first), page_xml(&second));
    for id in [a, b, g, level] {
        let start = format!(r#"<Shape ID="{id}""#);
        assert_eq!(element(&x2, &start, "</Shape>"), element(&x1, &start, "</Shape>"), "{id}");
    }
    assert_eq!(element(&x2, "<Connects>", "</Connects>"), element(&x1, "<Connects>", "</Connects>"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_held_shape_turned_or_flipped_is_released_and_one_turned_a_half_turn_is_not() {
    // The forms place an unturned box; a shape turned by a half turn keeps its box where the forms put it, and stays held.
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.rotate", json!({"ids": [id], "angle": 180.0}));
    assert_eq!(rows_from(&s, id).len(), 1, "a half turn");
    run(&mut s, "guide.move", json!({"id": g, "at": 3.5}));
    assert!(near(value(&s, id, "PinX"), 4.5));
    run(&mut s, "shape.rotate", json!({"ids": [id], "angle": 30.0}));
    assert!(rows_from(&s, id).is_empty(), "turned");
    assert_eq!(formula(&s, id, "AlignLeft"), None);
    let (mut s, _, id) = held_by_its_left_edge();
    run(&mut s, "shape.flip", json!({"ids": [id], "axis": "horizontal"}));
    assert!(rows_from(&s, id).is_empty(), "flipped");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn shape_size_moves_a_held_shape_s_pin_so_that_its_edge_stays_on_the_guide() {
    // `shape.size` keeps a shape's pin, but a held shape's pin is its formula's: the held edge stays on the guide and the pin moves.
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 5.0);
    let id = box_at(&mut s, 3.97, 2.0);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, id)[0].0, "AlignRight");
    run(&mut s, "shape.size", json!({"ids": [id], "width": 3.0}));
    assert!(near(value(&s, id, "PinX"), 3.5), "the pin moved: {}", value(&s, id, "PinX"));
    assert!(near(value(&s, id, "PinX") + 1.5, 5.0), "the right edge is on the guide");
    assert_eq!(rows_from(&s, id).len(), 1);
    run(&mut s, "guide.move", json!({"id": g, "at": 6.0}));
    assert!(near(value(&s, id, "PinX"), 4.5));
}

#[test]
fn a_resize_keeps_a_middle_hold_only_while_the_middle_stays_on_its_guide() {
    // Held by its centre to an upright guide, a resize of the top edge keeps the hold and one of the right edge alone (which moves the centre) releases it;
    // held by its middle to a level guide, the other way round.
    let mut s = guides_only();
    guide(&mut s, "vertical", 3.0);
    let id = box_at(&mut s, 3.04, 2.0);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, id)[0].0, "AlignCenter");
    run(&mut s, "shape.resize", json!({"id": id, "x": 2.0, "y": 1.5, "width": 2.0, "height": 1.5, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 1, "the top moved, the centre did not");
    run(&mut s, "shape.resize", json!({"id": id, "x": 2.0, "y": 1.5, "width": 2.5, "height": 1.5, "snap": true}));
    assert!(rows_from(&s, id).is_empty(), "the right edge moved the centre");
    guide(&mut s, "horizontal", 8.0);
    let id = box_at(&mut s, 6.0, 8.04);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, id)[0].0, "AlignMiddle");
    run(&mut s, "shape.resize", json!({"id": id, "x": 5.0, "y": 7.5, "width": 2.5, "height": 1.0, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 1, "the right edge moved, the middle did not");
    run(&mut s, "shape.resize", json!({"id": id, "x": 5.0, "y": 7.5, "width": 2.5, "height": 1.5, "snap": true}));
    assert!(rows_from(&s, id).is_empty(), "the top moved the middle");
}

#[test]
fn a_move_that_moves_nothing_with_snap_is_no_undo_step() {
    // `shape.move`'s change is what it moved or the holds it wrote: a connector glued at both ends moves nothing, and `snap` adds no step.
    let mut s = guides_only();
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 2.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let kept = snapshot(&s);
    let out = run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.0, "snap": true}));
    assert_eq!((out["moved"].clone(), out["snap"]["held"].clone()), (json!(0), json!([])), "{out}");
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps, now.revision), (kept.document, kept.undo_steps, kept.revision));
}

#[test]
fn grouping_a_held_shape_without_its_guide_releases_it_where_it_is() {
    // A held shape put in a group has its pin carried into the group's frame (`settle_kept`): its hold is broken and it is released, and no formula of
    // it names the guide; the guide, outside the group, carries nothing.
    let (mut s, g, id) = held_by_its_left_edge();
    let other = box_at(&mut s, 8.0, 8.0);
    let before = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
    run(&mut s, "shape.group", json!({"ids": [id, other]}));
    let after = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
    assert!(before.iter().zip(after.iter()).all(|(p, q)| near_p(*p, *q)), "{before:?} became {after:?}");
    assert!(rows_from(&s, id).is_empty() && !named_anywhere(&s, g));
    run(&mut s, "guide.move", json!({"id": g, "at": 1.0}));
    let moved = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
    assert!(before.iter().zip(moved.iter()).all(|(p, q)| near_p(*p, *q)), "the guide carries nothing now: {moved:?}");
    run(&mut s, "guide.delete", json!({"id": g}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_resize_by_both_edges_alike_keeps_a_middle_on_its_guide_held() {
    // Held by its centre to an upright guide at 3, a box made a half inch wider on each side keeps its centre on the guide, and stays held.
    let mut s = guides_only();
    let g = guide(&mut s, "vertical", 3.0);
    let id = box_at(&mut s, 3.04, 2.0);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, id)[0].0, "AlignCenter");
    run(&mut s, "shape.resize", json!({"id": id, "x": 1.5, "y": 1.5, "width": 3.0, "height": 1.0, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 1, "the centre did not move");
    run(&mut s, "guide.move", json!({"id": g, "at": 4.0}));
    assert!(near(value(&s, id, "PinX"), 4.0) && near(value(&s, id, "Width"), 3.0));
}

#[test]
fn a_nudge_along_the_guide_writes_the_glued_pin_back_on_its_line_and_keeps_the_hold() {
    // The arrow key's nudge is `shape.move {dx: 0, dy: step}` with no `snap`, and it writes `PinX` back where it was. The left edge is still on
    // its guide, so the hold is written again where the shape is (its row kept), and the guide still carries it.
    let (mut s, g, id) = held_by_its_left_edge();
    let row = rows_from(&s, id);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 1.0}));
    assert_eq!(rows_from(&s, id), row);
    assert_eq!(local(&s, id, "AlignLeft").0, Some(single("INTERSECTX", g, ("4", "2.5"))));
    assert_eq!(formula(&s, id, "PinX").as_deref(), Some("LocPinX+AlignLeft"));
    assert_eq!(formula(&s, id, "PinY").as_deref(), Some("LocPinY+AlignMiddle-Height/2"));
    run(&mut s, "guide.move", json!({"id": g, "at": 4.0}));
    assert!(near(value(&s, id, "PinX"), 5.0) && near(value(&s, id, "PinY"), 2.5), "the guide carries it");
    // A nudge across the guide takes the edge off its line: released.
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.1, "dy": 0.0}));
    assert!(rows_from(&s, id).is_empty());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_crossing_written_on_one_axis_is_released_whole_and_one_written_back_on_both_lines_is_kept() {
    // N3 of the re-review: held at the crossing of an upright guide at 3 and a level guide at 1 by its left and bottom edges, a write of x alone off the
    // upright guide releases both holds; it does not become a hold to the level guide alone. A nudge along neither guide (written back on both lines)
    // keeps the crossing.
    let (mut s, g, id) = held_by_its_left_edge();
    let level = guide(&mut s, "horizontal", 1.0);
    run(&mut s, "shape.place", json!({"ids": [id], "x": 4.0, "snap": true}));
    assert_eq!(rows_from(&s, id).len(), 2, "held at the crossing of {g} and {level}");
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0}));
    assert_eq!(rows_from(&s, id).len(), 2, "both pins written back on their lines");
    assert_eq!(local(&s, id, "AlignLeft").0, Some(crossing("INTERSECTX", g, level)));
    run(&mut s, "shape.place", json!({"ids": [id], "x": 6.0}));
    assert!(rows_from(&s, id).is_empty(), "{:?}", rows_from(&s, id));
    assert!(SIX.iter().all(|n| !formula(&s, id, n).is_some_and(|f| f.contains("Sheet."))), "no cell names a guide");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_half_turned_shape_whose_pin_is_off_its_middle_is_released() {
    // The re-review's probe: held by its left edge, its pin moved to its bottom-left corner (the left edge stays on the guide: the hold is
    // written again), then turned a half turn about that pin: its box is now x 1 to 3, its right edge on the guide, which `AlignLeft` and `FromPart` 1
    // would not say. It is released, and stays where the turn put it.
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.pin", json!({"ids": [id], "at": "bottom-left"}));
    assert_eq!(rows_from(&s, id).len(), 1, "re-pinned on its line, still held");
    assert!(near(value(&s, id, "PinX"), 3.0));
    run(&mut s, "shape.rotate", json!({"ids": [id], "angle": 180.0}));
    assert!(rows_from(&s, id).is_empty());
    assert_eq!(formula(&s, id, "AlignLeft"), None);
    let corners = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
    run(&mut s, "guide.move", json!({"id": g, "at": 2.0}));
    let after = diacraft_engine::draw::shape_corners(s.document().unwrap(), 1, id).unwrap();
    assert!(corners.iter().zip(after.iter()).all(|(p, q)| near_p(*p, *q)), "the guide carries it no more");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn deleting_a_guide_freezes_every_alignment_cell_that_names_it_whatever_its_formula() {
    // The re-review: a cell `GUARD(INTERSECTX(Sheet.7!...))` (box 1, with its row) and one `INTERSECTX(Sheet.7!...)+0` (box 2, no row) name guide 7; deleted
    // as a shape, the guide leaves no formula naming it, the guard going with the formula, and the boxes stay where they are.
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    let guide_cells = r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="0"/><Cell N="Width" V="0"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="1.5707963267949"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="InfiniteLine" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="1"/><Cell N="B" V="0"/></Row></Section>"#;
    let call = "INTERSECTX(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg)";
    let one = shape(
        1,
        &format!(r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="1.5"/>{size}<Cell N="AlignLeft" V="3" F="GUARD({call})"/>"#),
    );
    let two =
        shape(2, &format!(r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="5"/>{size}<Cell N="AlignLeft" V="3" F="{call}+0"/>"#));
    let seven = shape_with(7, r#" Type="Guide""#, guide_cells);
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{seven}{one}{two}")).connects(connects)).build();
    let path = scratch("forms.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert!(s.execute("guide.delete", &json!({"id": 7})).is_err(), "refused while a row and formulas name it");
    run(&mut s, "shape.delete", json!({"ids": [7]}));
    assert!(!named_anywhere(&s, 7));
    assert_eq!(local(&s, 1, "AlignLeft"), (None, Some(3.0), None), "the guard goes with the formula");
    assert_eq!(local(&s, 2, "AlignLeft"), (None, Some(3.0), None));
    assert!(near(value(&s, 1, "PinX"), 4.0) && near(value(&s, 2, "PinX"), 4.0));
    assert!(rows_from(&s, 1).is_empty());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_half_turned_shape_pinned_in_the_middle_of_its_width_only_is_released() {
    // The pin has to be in the middle both ways for a half turn to keep the hold. Pinned at its bottom centre, the box half turned keeps its left
    // edge at x 3 but turns about a point that is not its middle, so it is released all the same.
    let (mut s, g, id) = held_by_its_left_edge();
    run(&mut s, "shape.pin", json!({"ids": [id], "at": "bottom-center"}));
    assert_eq!(rows_from(&s, id).len(), 1, "re-pinned in the middle of its width, still held");
    run(&mut s, "shape.rotate", json!({"ids": [id], "angle": 180.0}));
    assert!(rows_from(&s, id).is_empty());
    assert_eq!(formula(&s, id, "AlignLeft"), None);
    let pin = (value(&s, id, "PinX"), value(&s, id, "PinY"));
    run(&mut s, "guide.move", json!({"id": g, "at": 2.0}));
    assert!(near_p(pin, (value(&s, id, "PinX"), value(&s, id, "PinY"))), "the guide carries it no more");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_box_whose_alignment_cell_names_its_guide_in_any_formula_is_frozen_when_released() {
    // F4 of the mutants: an alignment cell names guide 7 whatever call is round the guide's, also when its formula does not begin with it: box 3 `0+INTERSECTX(...)`
    // with its row, box 4 `1*INTERSECTX(...)` with none. A move without snap releases each, and the cell is left holding its value with no formula.
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    let guide_cells = r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="0"/><Cell N="Width" V="0"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="1.5707963267949"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="InfiniteLine" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="1"/><Cell N="B" V="0"/></Row></Section>"#;
    let call = "INTERSECTX(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,4 DL,1.5 DL,Sheet.7!Angle+90 deg)";
    let boxed = |id: u32, y: f64, formula: &str| {
        shape(id, &format!(r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="{y}"/>{size}<Cell N="AlignLeft" V="3" F="{formula}"/>"#))
    };
    let (three, four) = (boxed(3, 1.5, &format!("0+{call}")), boxed(4, 5.0, &format!("1*{call}")));
    let seven = shape_with(7, r#" Type="Guide""#, guide_cells);
    let connects = r#"<Connect FromSheet="3" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{seven}{three}{four}")).connects(connects)).build();
    let path = scratch("other-forms.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "shape.move", json!({"ids": [3, 4], "dx": 0.5, "dy": 0.0}));
    assert!(rows_from(&s, 3).is_empty());
    for id in [3, 4] {
        assert_eq!(formula(&s, id, "AlignLeft"), None, "{id} frozen: {:?}", local(&s, id, "AlignLeft"));
        assert!(near(value(&s, id, "PinX"), 4.5), "{id}");
    }
    run(&mut s, "guide.move", json!({"id": 7, "at": 1.0}));
    assert!(near(value(&s, 3, "PinX"), 4.5) && near(value(&s, 4, "PinX"), 4.5), "the guide carries them no more");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_that_grows_keeps_the_holds_to_turned_guides() {
    // For guides turned after the glue (of the mutants): the release pass would take a pin written by the carrying off its
    // guide's line, as a turned guide has no line along an axis, so `shift_holds` leaves the crossing out of the carrying and writes the one-guide hold
    // at its place moved by the shift. A: held by its middle to a level guide through (0, 0) turned to 45 degrees (at (1, 1)). B: held at the crossing of
    // an upright guide at 5 turned to 120 degrees and a level guide at 6.
    let mut s = guides_only();
    let slant = guide(&mut s, "horizontal", 0.0);
    let a = box_at(&mut s, 2.0, 0.04);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": 0.0, "snap": true}));
    run(&mut s, "shape.rotate", json!({"ids": [slant], "angle": 45.0}));
    assert!(near(value(&s, a, "PinX"), 1.0) && near(value(&s, a, "PinY"), 1.0));
    let (upright, level) = (guide(&mut s, "vertical", 5.0), guide(&mut s, "horizontal", 6.0));
    let b = box_at(&mut s, 6.0, 6.5);
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 0.0, "snap": true}));
    assert_eq!(rows_from(&s, b).len(), 2, "B at the crossing");
    run(&mut s, "shape.rotate", json!({"ids": [upright], "angle": 120.0}));
    let b_at = (value(&s, b, "PinX"), value(&s, b, "PinY"));
    let free = box_at(&mut s, 8.0, 3.0);
    run(&mut s, "page.auto_size", json!({"on": true}));
    run(&mut s, "shape.move", json!({"ids": [free], "dx": -9.5, "dy": 0.0}));
    let shift = value(&s, upright, "PinX") - 5.0;
    assert!(shift > 1.0, "the page grew on the left: {shift}");
    assert_eq!((rows_from(&s, a).len(), rows_from(&s, b).len()), (1, 2), "every hold kept");
    assert!(near(value(&s, a, "PinX"), 1.0 + shift) && near(value(&s, a, "PinY"), 1.0), "{} {}", value(&s, a, "PinX"), value(&s, a, "PinY"));
    assert!(near(value(&s, b, "PinX"), b_at.0 + shift) && near(value(&s, b, "PinY"), b_at.1));
    // They still follow their guides: A to the foot of the square line on the guide turned back level, B to the crossing moved up.
    run(&mut s, "shape.rotate", json!({"ids": [slant], "angle": 0.0}));
    assert!(near(value(&s, a, "PinX"), 1.0 + shift) && near(value(&s, a, "PinY"), 0.0), "{} {}", value(&s, a, "PinX"), value(&s, a, "PinY"));
    run(&mut s, "guide.move", json!({"id": level, "at": 7.0}));
    assert!(!near(value(&s, b, "PinY"), b_at.1), "B follows the level guide");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_nudge_releases_a_shape_held_to_a_turned_guide_even_with_its_edge_at_the_guide_s_pin() {
    // The line is a guide's that lies along the axis (U7 of the mutants): a file's box held by its left edge to a guide through (3, 0) at 60 degrees,
    // at a place whose square line passes through the guide's pin, so `AlignLeft` is 3, the guide's `PinX`. A nudge up writes `PinX` back where it was, but
    // the edge leaves the slanted line: released, where the nudge put it.
    let size = r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#;
    let guide_cells = r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="0"/><Cell N="Width" V="0"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="1.0471975511966"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="InfiniteLine" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="1"/><Cell N="B" V="0"/></Row></Section>"#;
    // The place: the guide's pin and one inch along the square line (at 150 degrees), backwards.
    let (px, py) = (3.0 + 0.866_025_403_784_438_6, -0.5);
    let call = |func: &str| format!("{func}(Sheet.7!PinX,Sheet.7!PinY,Sheet.7!Angle,{px} DL,{py} DL,Sheet.7!Angle+90 deg)");
    let one = shape(
        1,
        &format!(
            r#"<Cell N="PinX" V="4" F="LocPinX+AlignLeft"/><Cell N="PinY" V="0" F="LocPinY+AlignMiddle-Height/2"/>{size}<Cell N="AlignLeft" V="3" F="{}"/><Cell N="AlignMiddle" V="0" F="{}"/>"#,
            call("INTERSECTX"),
            call("INTERSECTY")
        ),
    );
    let seven = shape_with(7, r#" Type="Guide""#, guide_cells);
    let connects = r#"<Connect FromSheet="1" FromCell="AlignLeft" FromPart="1" ToSheet="7" ToCell="Angle" ToPart="4"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&format!("{seven}{one}")).connects(connects)).build();
    let path = scratch("slanted.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert!(near(value(&s, 1, "PinY"), 0.0), "the file's values agree with its formulas");
    let y = value(&s, 1, "PinY");
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 1.0}));
    assert!(rows_from(&s, 1).is_empty());
    assert_eq!(formula(&s, 1, "AlignLeft"), None);
    assert!(near(value(&s, 1, "PinX"), 4.0) && near(value(&s, 1, "PinY"), y + 1.0), "{} {}", value(&s, 1, "PinX"), value(&s, 1, "PinY"));
    assert_eq!(check_session(&s), Ok(()));
}
