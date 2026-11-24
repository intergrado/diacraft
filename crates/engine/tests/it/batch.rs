//! `command.batch`: several commands as one undo step, all or nothing.

use std::sync::Arc;

use diacraft_engine::{EngineError, MAX_BATCH_STEPS, Session};
use serde_json::{Value, json};

use crate::{bad_params_msg, check_session, page_names, run, session_with_history, snapshot};

fn step(command: &str, params: Value) -> Value {
    json!({"command": command, "params": params})
}

fn batch(steps: Vec<Value>) -> Value {
    json!({"commands": steps})
}

/// Runs a batch that must fail, and checks that it changed nothing.
fn failing(s: &mut Session, params: &Value) -> EngineError {
    let before = snapshot(s);
    let e = s.execute("command.batch", params).unwrap_err();
    assert_eq!(snapshot(s), before, "a failed batch changes nothing: {e}");
    assert_eq!(s.panics_caught(), 0, "{e}");
    assert_eq!(check_session(s), Ok(()));
    e
}

#[test]
fn batch_is_one_undo_step() {
    let mut s = session_with_history();
    let before = s.document().cloned().unwrap();
    let steps = s.history().unwrap().undo_len();

    let params = json!({"commands": [
        step("page.add", json!({"name": "Third"})),
        step("page.add", json!({})),
        step("page.rename", json!({"id": 1, "name": "First"})),
    ], "label": "Three edits"});
    let result = run(&mut s, "command.batch", params);

    // The results are the steps', in order.
    assert_eq!(result["results"], json!([{"id": 3, "name": "Third"}, {"id": 4, "name": "Page-4"}, {"id": 1, "name": "First"}]));
    assert_eq!(page_names(&s), ["First", "Second", "Third", "Page-4"]);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "three edits, one step");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Three edits"));
    let after = s.document().cloned().unwrap();

    run(&mut s, "edit.undo", json!({}));
    let undone = s.document().unwrap();
    assert_eq!(undone, &before, "one undo takes back all three edits");
    assert!(undone.pages.iter().zip(&before.pages).all(|(a, b)| Arc::ptr_eq(a, b)), "the very pages that were there");
    assert_eq!(s.history().unwrap().undo_len(), steps);

    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document(), Some(&after), "one redo puts all three back");
}

#[test]
fn a_batch_without_a_label_is_named_batch() {
    let mut s = session_with_history();
    run(&mut s, "command.batch", batch(vec![step("page.add", json!({}))]));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Batch"));
}

#[test]
fn batch_rolls_back_on_failed_step() {
    let mut s = session_with_history();
    let params = batch(vec![
        step("page.add", json!({"name": "Gone"})),
        step("page.rename", json!({"id": 1, "name": "Gone too"})),
        step("page.rename", json!({"id": 999, "name": "Nobody"})),
        step("page.add", json!({"name": "Never run"})),
    ]);

    let e = failing(&mut s, &params);

    let text = e.to_string();
    assert!(text.contains("batch step 2"), "{text}");
    assert!(text.contains("`page.rename`"), "names the command: {text}");
    assert!(text.contains("no page has id 999"), "and why: {text}");
    assert_eq!(page_names(&s), ["Baseline", "Second"]);
    // The session is a normal one afterwards: the next edit is a step of its own.
    run(&mut s, "page.add", json!({"name": "Later"}));
    assert_eq!(s.history().unwrap().undo_len(), 3);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(page_names(&s), ["Baseline", "Second"], "undo takes back only that page");
}

#[test]
fn the_first_step_is_step_zero() {
    let mut s = session_with_history();
    let e = failing(&mut s, &batch(vec![step("page.rename", json!({"id": 999, "name": "x"}))]));
    assert!(e.to_string().contains("batch step 0"), "{e}");
}

#[test]
fn a_failed_step_is_not_hidden_by_the_error_kind() {
    let mut s = session_with_history();
    let e = failing(&mut s, &batch(vec![step("page.add", json!({"nam": "typo"}))]));
    let text = e.to_string();
    assert!(text.contains("batch step 0") && text.contains("did you mean `name`?"), "each step's parameters are checked: {text}");
    let e = failing(&mut s, &batch(vec![step("no.such.command", json!({}))]));
    assert!(e.to_string().contains("batch step 0 (`no.such.command`)") && e.to_string().contains("unknown command"), "{e}");
    let e = failing(&mut s, &batch(vec![step("page.add", json!({})), step("edit.redo", json!({}))]));
    assert!(e.to_string().contains("batch step 1 (`edit.redo`)") && e.to_string().contains("nothing to redo"), "disabled steps too: {e}");
}

#[test]
fn batch_does_not_nest() {
    let mut s = session_with_history();
    let inner = step("command.batch", json!({"commands": []}));
    let e = failing(&mut s, &batch(vec![step("page.add", json!({})), inner]));
    let text = e.to_string();
    assert!(text.contains("batch step 1") && text.contains("cannot nest"), "{text}");
}

#[test]
fn batch_step_limit_is_enforced() {
    assert_eq!(MAX_BATCH_STEPS, 256);
    let rename = || step("page.rename", json!({"id": 1, "name": "Same"}));
    let mut s = session_with_history();

    let e = failing(&mut s, &batch(vec![rename(); 257]));
    assert!(bad_params_msg(&e).contains("at most 256"), "{e}");

    // Exactly the limit is fine, and is still one undo step.
    let steps = s.history().unwrap().undo_len();
    let result = run(&mut s, "command.batch", batch(vec![rename(); 256]));
    assert_eq!(result["results"].as_array().map(Vec::len), Some(256));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
}

#[test]
fn a_huge_batch_is_refused_before_any_step_is_looked_at() {
    let mut s = session_with_history();
    let e = failing(&mut s, &batch(vec![json!(null); 200_000]));
    assert!(bad_params_msg(&e).contains("at most 256"), "{e}");
}

#[test]
fn a_malformed_batch_is_refused_before_any_step_runs() {
    let mut s = session_with_history();
    let good = || step("page.add", json!({"name": "Not added"}));
    for (params, words) in [
        (json!({}), "missing `commands`"),
        (json!({"commands": null}), "`commands` must be an array"),
        (json!({"commands": "page.add"}), "`commands` must be an array"),
        (json!({"commands": {"command": "page.add"}}), "`commands` must be an array"),
        (batch(vec![good(), json!("page.add")]), "commands[1] must be an object"),
        (batch(vec![good(), json!([])]), "commands[1] must be an object"),
        (batch(vec![good(), json!({})]), "commands[1] needs a `command`"),
        (batch(vec![good(), json!({"command": 5})]), "commands[1] needs a `command`"),
        (batch(vec![good(), json!({"command": null})]), "commands[1] needs a `command`"),
        (batch(vec![good(), json!({"command": "page.add", "param": {}})]), "commands[1] has an unknown key `param`; did you mean `params`?"),
        (json!({"commands": [], "label": 5}), "`label` must be text"),
        (json!({"commands": [], "label": ""}), "`label` must not be empty"),
        (json!({"commands": [], "label": "x".repeat(256)}), "`label` must be at most 255 characters"),
        (json!({"commands": [], "label": " \t "}), "`label` must not be empty"),
        (json!({"commands": [], "lable": "x"}), "did you mean `label`?"),
    ] {
        let e = failing(&mut s, &params);
        assert!(bad_params_msg(&e).contains(words), "{params}: {e}");
    }
    assert_eq!(page_names(&s), ["Baseline", "Second"]);
}

#[test]
fn a_step_whose_params_are_not_an_object_fails_that_step() {
    let mut s = session_with_history();
    for params in [json!(5), json!("x"), json!([1])] {
        let e = failing(&mut s, &batch(vec![step("page.add", json!({})), step("page.add", params.clone())]));
        let text = e.to_string();
        assert!(text.contains("batch step 1") && text.contains("must be a JSON object"), "{params}: {text}");
    }
    // Missing or null params mean none.
    let steps = batch(vec![json!({"command": "page.add"}), json!({"command": "page.add", "params": null})]);
    run(&mut s, "command.batch", steps);
    assert_eq!(page_names(&s), ["Baseline", "Second", "Page-3", "Page-4"]);
}

#[test]
fn steps_that_would_leave_the_batch_s_one_step_are_refused() {
    // Undo and redo move along the history the batch is still writing; a new document replaces
    // what the batch edits. Each fails its step and rolls the batch back.
    for (command, params) in [("edit.undo", json!({})), ("file.new", json!({}))] {
        let mut s = session_with_history();
        let e = failing(&mut s, &batch(vec![step("page.add", json!({"name": "Gone"})), step(command, params)]));
        let text = e.to_string();
        assert!(text.contains("batch step 1") && text.contains(command) && text.contains("inside a batch"), "{text}");
    }
    let mut s = session_with_history();
    run(&mut s, "edit.undo", json!({}));
    let e = failing(&mut s, &batch(vec![step("edit.redo", json!({}))]));
    assert!(e.to_string().contains("inside a batch"), "{e}");
}

#[test]
fn a_batch_of_queries_changes_and_records_nothing_but_its_journal_entry() {
    let mut s = session_with_history();
    let before = snapshot(&s);
    let result = run(&mut s, "command.batch", batch(vec![step("app.version", json!({})), step("document.inspect", json!({}))]));
    assert_eq!(result["results"][0]["app"], "diacraft");
    assert_eq!(result["results"][1]["pages"].as_array().map(Vec::len), Some(2));
    let after = snapshot(&s);
    assert_eq!((after.document, after.undo_steps, after.revision), (before.document, before.undo_steps, before.revision));
    assert_eq!(after.journal.len(), before.journal.len() + 1);
}

#[test]
fn an_empty_batch_is_allowed_and_does_nothing() {
    let mut s = session_with_history();
    let steps = s.history().unwrap().undo_len();
    let result = run(&mut s, "command.batch", batch(vec![]));
    assert_eq!(result, json!({"results": []}));
    assert_eq!(s.history().unwrap().undo_len(), steps);
}

#[test]
fn a_batch_needs_a_document() {
    let mut s = Session::new();
    let e = s.execute("command.batch", &batch(vec![step("file.new", json!({}))])).unwrap_err();
    assert!(matches!(e, EngineError::Disabled { .. }), "{e:?}");
}

#[test]
fn a_batch_that_fills_the_document_fails_whole() {
    let mut s = session_with_history();
    // One page short of the limit, so that the batch's first page fits and its second does not.
    while s.document().map(|d| d.pages.len()) < Some(diacraft_doc::MAX_PAGES - 1) {
        run(&mut s, "page.add", json!({}));
    }
    let e = failing(&mut s, &batch(vec![step("page.add", json!({"name": "Fits"})), step("page.add", json!({"name": "Too many"}))]));
    assert!(e.to_string().contains("batch step 1") && e.to_string().contains("at most 1000 pages"), "{e}");
    assert!(page_names(&s).iter().all(|n| n != "Fits"), "the page that fitted is gone too");
}

#[test]
fn a_label_with_a_control_character_is_rejected_and_nothing_changes() {
    let mut s = session_with_history();
    // The steps are good ones: only the label stands in the way, and no step may run.
    for label in ["two\nlines", "tab\there", "nul\0byte", "esc\u{1b}[31m", "del\u{7f}", "next line\u{85}", "\rcarriage return", "end\n"] {
        let params = json!({"commands": [step("page.add", json!({"name": "Not added"}))], "label": label});
        let e = failing(&mut s, &params);
        assert!(bad_params_msg(&e).contains("`label` must not contain control characters"), "{label:?}: {e}");
    }
    assert_eq!(page_names(&s), ["Baseline", "Second"]);
    // The same label with the control character taken out is fine.
    run(&mut s, "command.batch", json!({"commands": [step("page.add", json!({}))], "label": "two lines"}));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("two lines"));
}
