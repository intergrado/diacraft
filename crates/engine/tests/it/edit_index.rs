//! What an edit asks of the formulas of the page. To say which cells an edit reaches, `calc` needs the cells that read a cell through another sheet,
//! which it found by asking every formula of the page whether it reads beyond its own sheet, for every edit: on a page of twelve thousand shapes that was most of what
//! an edit cost (with a profiler). The session now keeps the answer from one edit to the next (`calc_index.rs`, `diacraft_calc::ReaderIndex`) and an edit puts right
//! what it replaced. The cost is **counted** (`diacraft_calc::work::formulas_asked`: the formulas the dependency walk was made to ask), not timed: a count does not move with the load.

use diacraft_engine::Session;
use diacraft_testkit::package::typical_drawing;
use serde_json::{Value, json};

use crate::native_common::Dir;
use crate::run;

/// Shapes on the page: each holds fourteen formulas, so a walk of the page asks 21,000.
const SHAPES: u32 = 1_500;
const FORMULAS: u64 = 14 * SHAPES as u64;

/// A session holding a page of `SHAPES` typical shapes, opened from a file, and the `ID`s of the shapes.
fn page() -> (Session, Vec<u32>) {
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &typical_drawing(1, SHAPES, None));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let ids = s.document().unwrap().pages.iter().next().unwrap().shapes.iter().map(|sh| sh.id).collect();
    (s, ids)
}

/// The formulas the dependency walk was made to ask for one command.
fn asked(s: &mut Session, command: &str, params: Value) -> u64 {
    diacraft_calc::work::reset();
    run(s, command, params);
    diacraft_calc::work::formulas_asked()
}

/// The first edit of a drawing makes the index, which asks every formula once; **the edits after it ask the formulas of the shapes they replaced and no others**. A move writes
/// values, which no formula of the page reads beyond its sheet, so it asks none; a fill writes a formula in a cell of one shape, which asks the formulas of that shape.
#[test]
fn an_edit_after_the_first_asks_the_formulas_of_what_it_replaced_and_no_others() {
    let (mut s, ids) = page();
    let first = asked(&mut s, "shape.move", json!({"ids": [ids[3]], "dx": 0.25, "dy": 0.0}));
    assert!(first >= FORMULAS, "the first edit asks every formula of the page once: {first} of {FORMULAS}");
    for (k, id) in ids.iter().copied().enumerate().skip(10).step_by(97).take(8) {
        let moved = asked(&mut s, "shape.move", json!({"ids": [id], "dx": 0.5, "dy": 0.25}));
        assert!(moved <= 14, "a move of shape {id} (the {k}th) asked {moved} formulas of the {FORMULAS} on the page");
        let filled = asked(&mut s, "shape.format", json!({"ids": [id], "fill": "#3366CC"}));
        // A fill writes a formula in a cell of the shape: its fourteen and the new one, and no formula of another shape.
        assert!(filled <= 15, "a fill of shape {id} asked {filled} formulas of the {FORMULAS} on the page");
    }
}

/// An undo goes back to an earlier document, and the next edit is made from it: the index is put right by what is different, and is not asked for again.
#[test]
fn an_undo_and_the_edit_after_it_ask_no_formula_of_the_page_again() {
    let (mut s, ids) = page();
    asked(&mut s, "shape.move", json!({"ids": [ids[1]], "dx": 0.25, "dy": 0.0}));
    asked(&mut s, "shape.move", json!({"ids": [ids[2]], "dx": 0.25, "dy": 0.0}));
    let undone = asked(&mut s, "edit.undo", json!({}));
    let again = asked(&mut s, "shape.move", json!({"ids": [ids[5]], "dx": 0.25, "dy": 0.0}));
    assert!(undone <= 14 && again <= 28, "an undo asked {undone} formulas and the move after it {again} of the {FORMULAS} on the page");
}

/// What the index holds is counted, and goes with the drawing: a page of rectangles and the connectors glued between them (formulas that read the shape they are glued to) makes an
/// index that the session says it holds, which no edit grows past what it holds for the same page, and a new drawing lets go of.
#[test]
fn the_index_of_a_page_with_connectors_is_counted_and_goes_with_the_drawing() {
    let mut s = crate::edit_cost::big_page();
    let ids: Vec<u32> = s.document().unwrap().pages.iter().next().unwrap().shapes.iter().map(|sh| sh.id).collect();
    run(&mut s, "shape.move", json!({"ids": [ids[30]], "dx": 0.25, "dy": 0.0}));
    let held = s.calc_index_held();
    assert!(held > 0, "the connectors read the shapes they are glued to: the index holds those readers");
    for id in [ids[31], ids[40], ids[60]] {
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.25, "dy": 0.1}));
        assert_eq!(s.calc_index_held(), held, "a move writes values: the readers of the page are the same");
    }
    run(&mut s, "file.new", json!({}));
    assert_eq!(s.calc_index_held(), 0, "a drawing that is replaced lets the index of the one that is gone go");
}

/// A resize of several shapes as one and a turn of several about a point (`shape.resize` with `ids`, `shape.rotate` with `by` and `about`) are worked out
/// through the session's index as every other edit is (`draw::settle`, which calls `calc_index`): after the first edit they ask the formulas of the shapes they replaced and no
/// others, and so does a resize of a group, whose members follow.
#[test]
fn a_set_resize_a_set_turn_and_a_group_resize_after_the_first_edit_ask_the_formulas_of_their_shapes_only() {
    let (mut s, ids) = page();
    asked(&mut s, "shape.move", json!({"ids": [ids[3]], "dx": 0.25, "dy": 0.0}));
    // The joint box of some shapes of the page: (left, bottom, right, top).
    let joint = |s: &Session, of: &[u32]| {
        let d = s.document().unwrap();
        let points: Vec<(f64, f64)> = of.iter().flat_map(|id| diacraft_engine::draw::shape_corners(d, 1, *id).unwrap()).collect();
        let (xs, ys) = (points.iter().map(|p| p.0), points.iter().map(|p| p.1));
        (
            xs.clone().fold(f64::INFINITY, f64::min),
            ys.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
            ys.fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let set = [ids[10], ids[400], ids[901]];
    let (x0, y0, x1, y1) = joint(&s, &set);
    let resized = asked(&mut s, "shape.resize", json!({"ids": set, "x": x0, "y": y0, "width": 1.5 * (x1 - x0), "height": 1.25 * (y1 - y0)}));
    let (nx0, _, nx1, _) = joint(&s, &set);
    assert!(((nx1 - nx0) - 1.5 * (x1 - x0)).abs() < 1e-6, "the three were stretched as one");
    assert!(resized <= 3 * 15, "a resize of three shapes asked {resized} formulas of the {FORMULAS} on the page");
    let turned = asked(&mut s, "shape.rotate", json!({"ids": set, "by": 30, "about": {"x": x0, "y": y0}}));
    assert!(turned <= 3 * 15, "a turn of three shapes about a point asked {turned} formulas of the {FORMULAS} on the page");
    // A group is a shape added to the page: the edit after it makes the index again whole, and the resize of the group after that asks only what it replaced.
    let group = crate::edit::id_of(&run(&mut s, "shape.group", json!({"ids": [ids[20], ids[21]]})));
    asked(&mut s, "shape.move", json!({"ids": [ids[5]], "dx": 0.25, "dy": 0.0}));
    let (gx0, gy0, gx1, gy1) = joint(&s, &[group]);
    let grown = asked(&mut s, "shape.resize", json!({"id": group, "x": gx0, "y": gy0, "width": 2.0 * (gx1 - gx0), "height": gy1 - gy0}));
    assert!((joint(&s, &[group]).2 - (gx0 + 2.0 * (gx1 - gx0))).abs() < 1e-6, "the group was stretched");
    assert!(grown <= 3 * 15, "a resize of a group of two asked {grown} formulas of the {FORMULAS} on the page");
}
