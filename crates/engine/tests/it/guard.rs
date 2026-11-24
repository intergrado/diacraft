//! The panic guard and the all-or-nothing rule: a command that panics or fails leaves the session as
//! it was, and the session keeps working.

use diacraft_engine::cmd::always;
use diacraft_engine::guard::catch_panic;
use diacraft_engine::{CommandSpec, EngineError, Result, Run, Session};
use serde_json::{Value, json};

use crate::{check_session, run, session_with_history, snapshot};

fn spec(id: &'static str, run: Run) -> CommandSpec {
    CommandSpec { id, label: "Test", menu: &[], shortcut: None, params: "none", enabled: always, run, journal: true }
}

fn failed(cmd: &str, msg: &str) -> EngineError {
    EngineError::Failed { cmd: cmd.into(), msg: msg.into() }
}

/// Two edits, each its own undo step when run alone, then a panic.
fn edit_edit_panic(s: &mut Session, _: &Value) -> Result<Value> {
    s.edit("First", |d| d.add_page(Some("Doomed")).map(drop).map_err(|e| failed("test.panic", &e.to_string())))?;
    s.edit("Second", |d| {
        d.title = "Doomed too".into();
        Ok(())
    })?;
    panic!("boom {}", 42)
}

/// One edit, then a plain error.
fn edit_then_fail(s: &mut Session, _: &Value) -> Result<Value> {
    s.edit("Doomed", |d| d.add_page(Some("Doomed")).map(drop).map_err(|e| failed("test.fail", &e.to_string())))?;
    Err(failed("test.fail", "gave up"))
}

#[test]
fn catch_panic_returns_the_value_or_the_message() {
    assert_eq!(catch_panic(|| 7), Ok(7));
    assert_eq!(catch_panic(|| -> i32 { panic!("static text") }), Err("static text".to_owned()));
    assert_eq!(catch_panic(|| -> i32 { panic!("formatted {}", 1) }), Err("formatted 1".to_owned()));
    assert_eq!(catch_panic(|| -> i32 { std::panic::panic_any(5_u8) }), Err("unknown panic".to_owned()));
}

#[test]
fn panic_in_command_restores_document_and_counts() {
    let mut s = session_with_history();
    let before = snapshot(&s);
    assert_eq!(s.panics_caught(), 0);

    let e = s.run_spec(&spec("test.panic", edit_edit_panic), &json!({})).unwrap_err();

    match &e {
        EngineError::Internal { cmd, msg } => {
            assert_eq!(cmd, "test.panic");
            assert!(msg.contains("boom 42"), "{msg}");
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    assert_eq!(s.panics_caught(), 1);
    // The document, the history (neither edit left a step behind), the revision and the journal.
    assert_eq!(snapshot(&s), before);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_session_works_after_a_panic_and_counts_every_one() {
    let mut s = session_with_history();
    s.run_spec(&spec("test.panic", edit_edit_panic), &json!({})).unwrap_err();

    // A later command is top level again: guarded and journaled, not taken for a nested call.
    let journal = s.journal().len();
    run(&mut s, "page.add", json!({"name": "After"}));
    assert_eq!(s.journal().len(), journal + 1);
    assert_eq!(s.history().map(|h| h.undo_len()), Some(3));

    s.run_spec(&spec("test.panic", edit_edit_panic), &json!({})).unwrap_err();
    assert_eq!(s.panics_caught(), 2);
    assert_eq!(s.document().map(|d| d.pages.len()), Some(3), "the second panic undid only its own edits");
}

#[test]
fn a_panic_leaves_no_journal_entry_and_no_revision_change() {
    let mut s = session_with_history();
    let (revision, journal) = (s.revision(), s.journal().to_vec());
    s.run_spec(&spec("test.panic", edit_edit_panic), &json!({})).unwrap_err();
    assert_eq!(s.revision(), revision);
    assert_eq!(s.journal(), journal);
}

#[test]
fn a_failing_command_undoes_the_edits_it_made() {
    let mut s = session_with_history();
    let before = snapshot(&s);

    let e = s.run_spec(&spec("test.fail", edit_then_fail), &json!({})).unwrap_err();

    assert_eq!(e, failed("test.fail", "gave up"));
    assert_eq!(snapshot(&s), before, "the edit made before the error is gone, with its undo step");
    assert_eq!(s.panics_caught(), 0, "an error is not a panic");
}

/// A command that runs itself again.
fn recurse(s: &mut Session, _: &Value) -> Result<Value> {
    s.run_spec(&RECURSE, &json!({}))
}

static RECURSE: CommandSpec =
    CommandSpec { id: "test.recurse", label: "Test", menu: &[], shortcut: None, params: "none", enabled: always, run: recurse, journal: true };

#[test]
fn runaway_recursion_is_refused_instead_of_overflowing_the_stack() {
    let mut s = session_with_history();
    let before = snapshot(&s);
    let e = s.run_spec(&RECURSE, &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Failed { msg, .. } if msg.contains("nest")), "{e:?}");
    assert_eq!(s.panics_caught(), 0);
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_spec_with_a_malformed_params_doc_is_an_internal_error_not_a_panic() {
    let mut s = session_with_history();
    let mut bad = spec("test.bad_doc", edit_then_fail);
    bad.params = "not a list of keys";
    let e = s.run_spec(&bad, &json!({})).unwrap_err();
    assert!(matches!(e, EngineError::Internal { .. }), "{e:?}");
    assert_eq!(s.panics_caught(), 0, "found by checking the doc, not by a panic");
}

#[test]
fn a_disabled_spec_is_not_run() {
    fn never(_: &Session) -> std::result::Result<(), String> {
        Err("not today".into())
    }
    let mut s = session_with_history();
    let before = snapshot(&s);
    let mut off = spec("test.off", edit_then_fail);
    off.enabled = never;
    let e = s.run_spec(&off, &json!({})).unwrap_err();
    assert_eq!(e, EngineError::Disabled { cmd: "test.off".into(), reason: "not today".into() });
    assert_eq!(snapshot(&s), before);
}

/// Runs a batch whose second step fails, ignores the error and carries on.
fn carry_on_after_a_failed_batch(s: &mut Session, _: &Value) -> Result<Value> {
    let steps = json!({"commands": [
        {"command": "page.add", "params": {"name": "Gone"}},
        {"command": "page.rename", "params": {"id": 999, "name": "Nobody"}},
    ]});
    let error = s.execute("command.batch", &steps).unwrap_err();
    Ok(json!({ "batch_said": error.to_string() }))
}

/// Runs a batch that works, then fails.
fn run_a_batch_then_fail(s: &mut Session, _: &Value) -> Result<Value> {
    s.execute("command.batch", &json!({"commands": [{"command": "page.add", "params": {"name": "Gone"}}]}))?;
    Err(failed("test.fail", "gave up after the batch"))
}

#[test]
fn a_failed_batch_inside_a_command_that_carries_on_leaves_nothing_behind() {
    let mut s = session_with_history();
    let before = snapshot(&s);

    let result = s.run_spec(&spec("test.carry_on", carry_on_after_a_failed_batch), &json!({})).unwrap();

    assert!(result["batch_said"].as_str().is_some_and(|m| m.contains("batch step 1")), "{result}");
    let after = snapshot(&s);
    // The command itself succeeded, so it is journaled; but the batch it ran changed nothing.
    assert_eq!((after.document, after.undo_steps, after.revision), (before.document, before.undo_steps, before.revision));
    // And a batch afterwards works: the failed one left no batch open.
    run(&mut s, "command.batch", json!({"commands": [{"command": "page.add"}]}));
}

#[test]
fn a_batch_that_worked_is_undone_when_the_command_around_it_fails() {
    let mut s = session_with_history();
    let before = snapshot(&s);
    let e = s.run_spec(&spec("test.fail", run_a_batch_then_fail), &json!({})).unwrap_err();
    assert_eq!(e, failed("test.fail", "gave up after the batch"));
    assert_eq!(snapshot(&s), before, "the batch's page and its undo step are gone");
}

#[test]
fn a_panic_in_an_enabled_check_is_caught_too() {
    fn explode(_: &Session) -> std::result::Result<(), String> {
        panic!("enabled blew up")
    }
    let mut s = session_with_history();
    let before = snapshot(&s);
    let mut bad = spec("test.enabled_panic", edit_then_fail);
    bad.enabled = explode;

    let e = s.run_spec(&bad, &json!({})).unwrap_err();

    assert!(matches!(&e, EngineError::Internal { cmd, msg } if cmd == "test.enabled_panic" && msg.contains("enabled blew up")), "{e:?}");
    assert_eq!(s.panics_caught(), 1);
    assert_eq!(snapshot(&s), before);
    run(&mut s, "page.add", json!({}));
}

/// `Session::commands` asks every command whether it can run, for every menu refresh and `engine.commands`
/// call. A command whose `enabled` panics must not take the caller down: it is listed as disabled.
#[test]
fn a_panic_in_enabled_is_listed_as_a_disabled_command_and_counted() {
    fn explode(_: &Session) -> std::result::Result<(), String> {
        panic!("enabled blew up in the list")
    }
    let s = session_with_history();
    let before = snapshot(&s);
    let mut bad = spec("test.list_panic", edit_then_fail);
    bad.enabled = explode;

    let info = bad.info(&s);

    assert_eq!((info.id.as_str(), info.enabled), ("test.list_panic", false));
    let reason = info.disabled_reason.expect("a reason");
    assert!(reason.contains("internal error") && reason.contains("enabled blew up in the list"), "{reason}");
    assert_eq!(s.panics_caught(), 1);
    assert_eq!(snapshot(&s), before);
    // Every call is caught and counted; a command that behaves is listed as before.
    bad.info(&s);
    assert_eq!(s.panics_caught(), 2);
    let ok = spec("test.fine", edit_then_fail).info(&s);
    assert_eq!((ok.enabled, ok.disabled_reason), (true, None));
    assert_eq!(s.panics_caught(), 2);
    assert!(s.commands().iter().all(|c| c.disabled_reason.as_deref().is_none_or(|r| !r.contains("internal error"))));
}

/// The text of a panic, which can hold what a client sent (a command id, a name, a path), reaches a reply, a status bar or a
/// listing as `shown` makes it: no control, line-separator or direction character, and cut short.
fn hostile() -> String {
    format!("bad\u{1b}[2J\nline2\u{202e}\u{2028}{}", "x".repeat(10_000))
}

/// What a message that went through `shown` is: nothing a terminal or a log would act on, and not huge.
fn assert_safe_to_show(text: &str) {
    assert!(!text.chars().any(|c| c.is_control() || matches!(c, '\u{202e}' | '\u{2028}')), "{:?}", text.chars().take(200).collect::<String>());
    assert!(text.contains("bad\\u{1b}[2J\\nline2\\u{202e}\\u{2028}"), "escaped, not dropped: {}", text.chars().take(200).collect::<String>());
    assert!(text.chars().count() < 1_000, "{} characters: not cut short", text.chars().count());
}

fn run_that_panics_hostilely(_: &mut Session, _: &Value) -> Result<Value> {
    panic!("{}", hostile())
}

fn enabled_that_panics_hostilely(_: &Session) -> std::result::Result<(), String> {
    panic!("{}", hostile())
}

#[test]
fn the_text_of_a_panic_in_a_command_is_escaped_and_cut_in_the_error() {
    let mut s = session_with_history();
    let e = s.run_spec(&spec("test.hostile_run", run_that_panics_hostilely), &json!({})).unwrap_err();
    let EngineError::Internal { cmd, msg } = &e else { panic!("expected Internal, got {e:?}") };
    assert_eq!(cmd, "test.hostile_run");
    assert_safe_to_show(msg);
    assert_safe_to_show(&e.to_string());
    assert_eq!(s.panics_caught(), 1);
}

#[test]
fn the_text_of_a_panic_in_an_enabled_check_is_escaped_and_cut_in_the_error() {
    let mut s = session_with_history();
    let mut bad = spec("test.hostile_enabled", edit_then_fail);
    bad.enabled = enabled_that_panics_hostilely;
    let e = s.run_spec(&bad, &json!({})).unwrap_err();
    let EngineError::Internal { msg, .. } = &e else { panic!("expected Internal, got {e:?}") };
    assert_safe_to_show(msg);
    assert_safe_to_show(&e.to_string());
}

#[test]
fn the_text_of_a_panic_in_a_listed_commands_enabled_is_escaped_and_cut_in_the_reason() {
    let s = session_with_history();
    let mut bad = spec("test.hostile_list", edit_then_fail);
    bad.enabled = enabled_that_panics_hostilely;
    let info = bad.info(&s);
    assert_safe_to_show(&info.disabled_reason.expect("a reason"));
    assert_eq!(s.panics_caught(), 1);
}

/// A plain panic message keeps its words: the escape is for what is dangerous, and the cut is for what is huge.
#[test]
fn an_ordinary_panic_message_is_shown_whole() {
    fn run_boom(_: &mut Session, _: &Value) -> Result<Value> {
        panic!("index out of bounds: the len is 3 but the index is 7")
    }
    let mut s = session_with_history();
    let e = s.run_spec(&spec("test.boom", run_boom), &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Internal { msg, .. } if msg == "index out of bounds: the len is 3 but the index is 7"), "{e:?}");
}

#[test]
fn a_session_can_be_shared_between_threads_for_listing() {
    fn needs_send_and_sync<T: Send + Sync>(_: &T) {}
    needs_send_and_sync(&Session::new());
}
