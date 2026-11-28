//! Issue 1: the window state a client reads right after `view.pan` or `view.zoom` describes the view after the command.
//!
//! A program on the control channel sends `view.pan` and then `ui.inspect` at once. The window answers every request it has in a frame before it draws, so
//! `ui.pageRect` (where the page was drawn) used to be the last frame's, while `ui.pointsPerInch` (the view itself) was the new one. A client that moved a page
//! by the difference between what it read and what it wanted moved it by the wrong amount, and after a few rounds the page was off the drawing area.
//! The page's rectangle is now worked out from the view, in the window state of every window command and of `ui.inspect` alike.

use serde_json::{Value, json};

use crate::support::{Rest, Window, call, controlled, send};

/// A controlled window on a new drawing, at rest.
fn drawing() -> (Window, std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>) {
    let (mut w, tx) = controlled();
    call(&mut w, &tx, "engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    w.rest();
    (w, tx)
}

fn rect(state: &Value) -> [f64; 4] {
    let r = state["pageRect"].as_array().unwrap_or_else(|| panic!("no pageRect in {state}"));
    [r[0].as_f64().unwrap(), r[1].as_f64().unwrap(), r[2].as_f64().unwrap(), r[3].as_f64().unwrap()]
}

/// The two requests, sent before the window draws a frame, and their answers.
fn batch(
    w: &mut Window,
    tx: &std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>,
    first: (&str, Value),
    second: (&str, Value),
) -> (Value, Value) {
    let (a, b) = (send(tx, first.0, first.1), send(tx, second.0, second.1));
    let (mut first_answer, mut second_answer) = (None, None);
    for _ in 0..20 {
        w.step();
        first_answer = first_answer.or_else(|| a.try_recv().ok());
        second_answer = second_answer.or_else(|| b.try_recv().ok());
        if let (Some(a), Some(b)) = (&first_answer, &second_answer) {
            return (a.clone().unwrap(), b.clone().unwrap());
        }
    }
    panic!("the two requests were not both answered in 20 frames")
}

/// A pan and the `ui.inspect` sent with it in the same frame agree on where the page is, and the page moved by what the pan said, round after round.
#[test]
fn a_pan_and_an_inspect_in_one_frame_agree_on_where_the_page_is() {
    let (mut w, tx) = drawing();
    let mut before = rect(&w.state().window_state());
    for round in 0..40 {
        let (dx, dy) = (17.0 + f64::from(round % 5), -9.0 + f64::from(round % 3));
        let (answer, inspected) =
            batch(&mut w, &tx, ("engine.execute", json!({"command": "view.pan", "params": {"dx": dx, "dy": dy}})), ("ui.inspect", json!({})));
        let (from_answer, from_inspect) = (rect(&answer), rect(&inspected["ui"]));
        for k in 0..2 {
            let want = before[k] + if k == 0 { dx } else { dy };
            assert!(
                (from_answer[k] - want).abs() < 0.01,
                "round {round}: the answer of the pan has the page at {from_answer:?}, not moved by ({dx}, {dy}) from {before:?}"
            );
            assert!(
                (from_inspect[k] - want).abs() < 0.01,
                "round {round}: ui.inspect has the page at {from_inspect:?}, not moved by ({dx}, {dy}) from {before:?}"
            );
        }
        // The frame that came after draws what was read.
        let drawn = w.state().canvas_report().page.expect("the page was drawn");
        assert!(
            (f64::from(drawn.min.x) - from_inspect[0]).abs() < 0.01 && (f64::from(drawn.min.y) - from_inspect[1]).abs() < 0.01,
            "round {round}: drawn at {drawn:?}"
        );
        before = from_inspect;
    }
}

/// A zoom and the `ui.inspect` sent with it: the page's rectangle is as wide as the page is long at the zoom's points to an inch.
#[test]
fn a_zoom_and_an_inspect_in_one_frame_agree_on_the_size_of_the_page() {
    let (mut w, tx) = drawing();
    for percent in [85.0, 105.0, 200.0, 33.0, 100.0] {
        let (answer, inspected) =
            batch(&mut w, &tx, ("engine.execute", json!({"command": "view.zoom", "params": {"percent": percent}})), ("ui.inspect", json!({})));
        for (what, state) in [("the answer of the zoom", &answer), ("ui.inspect", &inspected["ui"])] {
            let ppi = state["pointsPerInch"].as_f64().unwrap();
            let r = rect(state);
            assert!((ppi - 96.0 * percent / 100.0).abs() < 1e-6, "{what}: {ppi} points to an inch at {percent}%");
            // The new drawing's page is 11 by 8.5 inches.
            assert!(
                (r[2] - 11.0 * ppi).abs() < 0.05 && (r[3] - 8.5 * ppi).abs() < 0.05,
                "{what}: the page is {} by {} points at {percent}%, {ppi} points to an inch",
                r[2],
                r[3]
            );
        }
        let drawn = w.state().canvas_report().page.expect("the page was drawn");
        let r = rect(&inspected["ui"]);
        assert!((f64::from(drawn.width()) - r[2]).abs() < 0.05, "the frame after draws the page {r:?} wide, drew {drawn:?}");
    }
}

/// Fit, and a pan after it, read in the same frame, say where the page is as the frame after will draw it.
#[test]
fn fit_page_then_pan_in_one_frame_is_what_the_next_frame_draws() {
    let (mut w, tx) = drawing();
    call(&mut w, &tx, "engine.execute", json!({"command": "view.zoom", "params": {"percent": 150}})).unwrap();
    w.rest();
    let (a, b) = (
        send(&tx, "engine.execute", json!({"command": "view.fitPage", "params": {}})),
        send(&tx, "engine.execute", json!({"command": "view.pan", "params": {"dx": 40, "dy": 25}})),
    );
    let c = send(&tx, "ui.inspect", json!({}));
    for _ in 0..20 {
        w.step();
    }
    a.try_recv().unwrap().unwrap();
    b.try_recv().unwrap().unwrap();
    let read = rect(&c.try_recv().unwrap().unwrap()["ui"]);
    let drawn = w.state().canvas_report().page.expect("the page was drawn");
    for (k, v) in [drawn.min.x, drawn.min.y, drawn.width(), drawn.height()].into_iter().enumerate() {
        assert!((f64::from(v) - read[k]).abs() < 0.05, "{read:?} was read, {drawn:?} was drawn");
    }
}
