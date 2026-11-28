//! An edit that leaves the drawing as it was is no change: no step of the history, no revision, no unsaved mark, whichever command made it, and a batch whose edits left the drawing
//! as it was is the same. The sweep of every command's sample is in `command_sweep.rs`; here are the cases it does not spell out.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, id_of};
use crate::{blank_session, check_session, run, snapshot};

/// The marks of a session's history and drawing: undo steps, redo steps, revision, whether the drawing is unsaved, and the label of the step undo takes back.
fn marks(s: &Session) -> (usize, usize, u64, bool, Option<String>) {
    let history = s.history();
    (
        history.map_or(0, |h| h.undo_len()),
        history.map_or(0, |h| h.redo_len()),
        s.revision(),
        s.is_dirty(),
        history.and_then(|h| h.next_undo_label()).map(str::to_owned),
    )
}

/// The formula text of a cell of shape `id`.
fn formula_of(s: &mut Session, id: u32, cell: &str) -> Value {
    run(s, "sheet.cell", json!({"id": id, "page": 1, "cell": cell}))["formula"].clone()
}

#[test]
fn a_move_by_nothing_counts_no_shape_makes_no_step_and_leaves_the_formula_of_the_pin() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    run(&mut s, "sheet.set_cell", json!({"id": a, "page": 1, "cell": "PinX", "formula": "1+2"}));
    let (before, held) = (snapshot(&s), marks(&s));
    assert_eq!(formula_of(&mut s, a, "PinX"), json!("1+2"));
    let answer = run(&mut s, "shape.move", json!({"ids": [a, b], "dx": 0, "dy": 0}));
    assert_eq!(answer["moved"], json!(0), "{answer}: no shape moved");
    assert_eq!(marks(&s), held, "no step, no revision, and not made unsaved by it");
    assert!(snapshot(&s).document == before.document, "the drawing is as it was");
    assert_eq!(formula_of(&mut s, a, "PinX"), json!("1+2"), "a move by nothing leaves the formula of the pin");
    // A move by a distance on one axis is a move of the shape: one step, and the count is 1 for it.
    let answer = run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.5, "dy": 0}));
    assert_eq!(answer["moved"], json!(1), "{answer}");
    assert_eq!((marks(&s).0, marks(&s).2), (held.0 + 1, held.2 + 1));
}

#[test]
fn a_no_op_after_an_edit_is_not_a_step_above_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let start = doc(&s).clone();
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.0, "dy": 1.0}));
    let moved = marks(&s);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0, "dy": 0}));
    assert_eq!(marks(&s), moved, "the step undo takes back is still the move's, and no redo step was ended");
    run(&mut s, "edit.undo", json!({}));
    assert!(*doc(&s) == start, "one undo comes back to the drawing as it was before the move");
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0, "dy": 0}));
    assert_eq!(marks(&s).1, 1, "a no-op does not end the redo steps");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_reconnect_to_the_place_an_end_is_at_is_no_step_for_a_free_end_and_a_glued_one() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"x": 6.0, "y": 1.0}})));
    let held = marks(&s);
    let before = doc(&s).clone();
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"x": 6.0, "y": 1.0}}));
    assert_eq!(marks(&s), held, "a free end put where it is");
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "begin", "target": {"id": a, "point": 3}}));
    assert_eq!(marks(&s), held, "a glued end glued to the point it is glued to");
    assert!(*doc(&s) == before);
    // To another place it is one step.
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": b, "point": 1}}));
    assert_eq!((marks(&s).0, marks(&s).2), (held.0 + 1, held.2 + 1));
}

#[test]
fn a_batch_that_leaves_the_drawing_as_it_was_is_no_step_no_revision_and_not_unsaved() {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // Saved: the drawing is not unsaved, so that a batch that left a mark would show.
    let held = marks(&s);
    let before = doc(&s) as *const diacraft_doc::Document;
    let name = doc(&s).page(1).unwrap().name.to_string();
    let rename = |to: &str| json!({"command": "page.rename", "params": {"id": 1, "name": to}});
    // A name given and given back: two edits that net nothing.
    let answer = run(&mut s, "command.batch", json!({"commands": [rename("Somewhere"), rename(&name)], "label": "There and back"}));
    assert_eq!(answer["results"].as_array().map(Vec::len), Some(2), "both ran: {answer}");
    assert_eq!(marks(&s), held, "the batch left no step, no revision and no mark");
    assert!(std::ptr::eq(doc(&s), before), "the drawing is the very one it was: the batch was taken back");
    assert_eq!(check_session(&s), Ok(()));
    // The same two edits with a third that changes something are one step.
    let third = json!({"command": "page.add", "params": {"name": "Extra"}});
    run(&mut s, "command.batch", json!({"commands": [rename("Somewhere"), rename(&name), third], "label": "And one more"}));
    assert_eq!(marks(&s).0, held.0 + 1, "one step for the batch");
    assert!(marks(&s).2 > held.2, "and the revision moved");
    assert_eq!(doc(&s).pages.len(), 2);
    // Undo takes the batch back whole.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).pages.len(), 1);
}

#[test]
fn a_batch_of_calls_that_each_change_nothing_is_no_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let held = marks(&s);
    let nudge = json!({"command": "shape.move", "params": {"ids": [a], "dx": 0, "dy": 0}});
    let format = json!({"command": "shape.format", "params": {"ids": [a], "fill": "#336699"}});
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#336699"}));
    let after_format = marks(&s);
    assert_eq!(after_format.0, held.0 + 1);
    run(&mut s, "command.batch", json!({"commands": [nudge, format]}));
    assert_eq!(marks(&s), after_format, "a move by nothing and a format to the colour the shape has");
}

/// **Timed, run by hand** (`--run-ignored all`): what the session's comparison of an edit's copy with the document costs on a big page. The copy of one edit of one shape of a page of
/// 20,000 shapes shares every chunk of the page's shapes but the one it wrote to, which is looked at (64 shapes). It prints the time of the comparison and of the edit's own command;
/// nothing is asserted of a clock.
#[test]
#[ignore = "timed: prints what the comparison of an edit's copy with the document costs on a page of 20,000 shapes"]
fn timed_the_comparison_of_an_edit_with_the_drawing_on_a_page_of_twenty_thousand_shapes() {
    use diacraft_doc::{Cell, CellName, Value as Held};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
    use std::sync::Arc;
    let mut page = PageBuilder::new().name("Page-1");
    for id in 1..=20_000_u32 {
        let (x, y) = (f64::from(id % 200) * 0.5, f64::from(id / 200) * 0.5);
        let shape = ShapeBuilder::new(id)
            .cell_u("PinX", &x.to_string(), "IN")
            .cell_u("PinY", &y.to_string(), "IN")
            .cell_u("Width", "0.4", "IN")
            .cell_u("Height", "0.4", "IN");
        page = page.shape(shape);
    }
    let mut s = blank_session();
    let built = DocBuilder::new().page(1, page).build();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    let before = doc(&s).clone();
    let mut next = before.clone();
    {
        let page = Arc::make_mut(next.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(10_000).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), Cell::constant(Held::from_text("9")));
    }
    let rounds = 1_000_u32;
    let started = std::time::Instant::now();
    let mut differs = 0_u32;
    for _ in 0..rounds {
        differs += u32::from(std::hint::black_box(next != before));
    }
    let compare = started.elapsed() / rounds;
    let started = std::time::Instant::now();
    run(&mut s, "shape.move", json!({"ids": [10_001], "dx": 0.25, "dy": 0.0}));
    let command = started.elapsed();
    eprintln!(
        "a page of 20,000 shapes: the comparison of a copy with the drawing {compare:?} ({differs} of {rounds} differ), the move of one shape {command:?}"
    );
    assert_eq!(differs, rounds);
}
