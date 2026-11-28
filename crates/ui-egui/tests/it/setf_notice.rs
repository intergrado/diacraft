//! A `SETF` call dropped past the caps is said in the status bar (`docs/editing-commands.md`: the effects past the cap
//! "are dropped with a note").
//!
//! The engine counts the calls and says them in the edit's answer (`crates/engine/tests/it/setf_edit.rs`); what these tests hold is the window's part: every command it runs, the
//! ShapeSheet window's commit included, puts the words in the status bar, and the next thing done takes them away.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;
use serde_json::json;

use crate::sheet_window::{one_page, window_on};

/// A shape with a counter: `Tick` adds `Step` to `N`, which it reads, each time it is worked out, so each write works it out again; `Other` is read by nobody.
fn counter() -> ShapeBuilder {
    let shape = ShapeBuilder::new(10).cell("Width", "2").cell("Height", "1").cell("PinX", "4").cell("PinY", "4");
    let shape = shape.row_cell("User", None, RowKey::name("Step"), "Value", "1").row_cell("User", None, RowKey::name("Other"), "Value", "0");
    shape.row_cell("User", None, RowKey::name("N"), "Value", "0").row_formula(
        "User",
        None,
        RowKey::name("Tick"),
        "Value",
        "SETF(GETREF(User.N),User.N+User.Step)",
        "0",
    )
}

#[test]
fn an_edit_that_drops_setf_calls_says_so_in_the_status_bar_and_the_next_edit_takes_it_away() {
    let mut w = window_on(one_page(counter()), [1400.0, 900.0]);
    assert_eq!(w.state().notice(), None, "a drawing that is opened says nothing of calls");
    let answer = w.state_mut().execute("sheet.set_cell", &json!({"id": 10, "cell": "User.Step", "formula": "2", "force": true})).unwrap();
    let dropped = answer["setf_dropped"].as_u64().unwrap_or(0);
    assert!(dropped >= 1, "the engine counted them: {answer}");
    let notice = w.state().notice().unwrap_or_default().to_owned();
    assert!(
        notice.starts_with(&format!("{dropped} SETF call")) && notice.contains("dropped") && notice.contains("64") && notice.contains("16"),
        "{notice}"
    );
    // An edit that no call reads drops nothing, and being something done it takes the notice away.
    let answer = w.state_mut().execute("sheet.set_cell", &json!({"id": 10, "cell": "User.Other", "formula": "5", "force": true})).unwrap();
    assert!(answer.get("setf_dropped").is_none(), "{answer}");
    assert!(!w.state().notice().unwrap_or_default().contains("SETF"), "{:?}", w.state().notice());
}

/// An edit of a drawing with no `SETF` in it says nothing.
#[test]
fn an_edit_with_no_dropped_call_leaves_the_status_bar_alone() {
    let plain = ShapeBuilder::new(10).cell("Width", "2").cell("Height", "1").cell("PinX", "4").cell("PinY", "4");
    let mut w = window_on(one_page(plain), [1400.0, 900.0]);
    w.state_mut().execute("sheet.set_cell", &json!({"id": 10, "cell": "Width", "formula": "3", "force": true})).unwrap();
    assert_eq!(w.state().notice(), None);
}
