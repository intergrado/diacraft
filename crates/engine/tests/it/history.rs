//! `Session::edit`, undo and redo, the journal and the parameter checks.

use std::sync::Arc;

use diacraft_engine::cmd::always;
use diacraft_engine::{CommandSpec, EngineError, History, JOURNAL_LIMIT, Result, Run, Session};
use serde_json::{Value, json};

use crate::{bad_params_msg, blank_session, check_session, page_names, run, session_with_history, snapshot};

fn failed(msg: &str) -> EngineError {
    EngineError::Failed { cmd: "test".into(), msg: msg.into() }
}

#[test]
fn undo_redo_restore_pages() {
    let mut s = blank_session();
    let mut states = vec![s.document().cloned().unwrap()];
    for params in [json!({"name": "B"}), json!({"name": "C"}), json!({})] {
        run(&mut s, "page.add", params);
        states.push(s.document().cloned().unwrap());
    }
    run(&mut s, "page.rename", json!({"id": 2, "name": "B2"}));
    states.push(s.document().cloned().unwrap());
    assert_eq!(page_names(&s), ["Page-1", "B2", "C", "Page-4"]);

    // Back through every state, exactly, then forward again.
    for expected in states.iter().rev().skip(1) {
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.document(), Some(expected));
    }
    assert_eq!(page_names(&s), ["Page-1"]);
    for expected in states.iter().skip(1) {
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.document(), Some(expected));
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn undo_and_redo_report_the_step_and_move_the_revision() {
    let mut s = session_with_history();
    let revision = s.revision();
    assert_eq!(run(&mut s, "edit.undo", json!({})), json!({"label": "Add page"}));
    assert_eq!(s.revision(), revision + 1);
    assert_eq!(s.history().unwrap().next_redo_label(), Some("Add page"));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Rename page"));
    assert_eq!(run(&mut s, "edit.redo", json!({})), json!({"label": "Add page"}));
    assert_eq!(s.revision(), revision + 2);
}

#[test]
fn a_new_edit_ends_the_redo_steps() {
    let mut s = session_with_history();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.history().unwrap().redo_len(), 1);
    run(&mut s, "page.add", json!({"name": "Fork"}));
    assert_eq!(s.history().unwrap().redo_len(), 0);
    let e = s.execute("edit.redo", &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Disabled { reason, .. } if reason == "nothing to redo"), "{e:?}");
}

#[test]
fn undo_and_redo_are_disabled_when_there_is_nothing_to_do() {
    let mut s = blank_session();
    for (id, reason) in [("edit.undo", "nothing to undo"), ("edit.redo", "nothing to redo")] {
        let before = snapshot(&s);
        let e = s.execute(id, &json!({})).unwrap_err();
        assert_eq!(e, EngineError::Disabled { cmd: id.into(), reason: reason.into() });
        assert_eq!(snapshot(&s), before);
        let info = s.commands().into_iter().find(|c| c.id == id).unwrap();
        assert_eq!((info.enabled, info.disabled_reason.as_deref()), (false, Some(reason)));
    }
    let mut none = Session::new();
    let e = none.execute("edit.undo", &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Disabled { reason, .. } if reason == "no document open"), "{e:?}");
}

#[test]
fn edit_error_restores_document() {
    let mut s = session_with_history();
    let before = snapshot(&s);

    let e = s
        .edit("Doomed", |d| {
            d.title = "Changed".into();
            d.add_page(Some("Extra")).map_err(|e| failed(&e.to_string()))?;
            Err::<(), _>(failed("changed my mind"))
        })
        .unwrap_err();

    assert_eq!(e, failed("changed my mind"));
    assert_eq!(snapshot(&s), before, "no change, no undo step, no new revision");
}

#[test]
fn edit_keeps_the_label_and_the_value_and_counts_a_revision() {
    let mut s = session_with_history();
    let (revision, steps, journal) = (s.revision(), s.history().unwrap().undo_len(), s.journal().len());
    let id = s.edit("Make a page", |d| d.add_page(Some("Made")).map_err(|e| failed(&e.to_string()))).unwrap();
    assert_eq!(id, 3);
    assert_eq!(s.revision(), revision + 1);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Make a page"));
    assert_eq!(s.journal().len(), journal, "edit is not a command: it journals nothing");
}

/// Task 4 replaces `edit_refuses_a_document_that_breaks_its_rules`. That test had `Session::edit` run `Document::validate` over the
/// whole document after every edit, which gaps F2 forbids: an imported document may hold a page name that would not pass, and
/// that must not stop every other edit. The rules are now checked at the write that sets the thing they are about (the next two
/// tests), and this one says what `edit` no longer does.
#[test]
fn edit_does_not_audit_the_whole_document() {
    let mut s = session_with_history();
    type Break = fn(&mut diacraft_doc::Document);
    // What a model built in memory can hold, where a door would refuse it: a repeated page id, a blank page name, more pages than a write
    // may make. (A file never gets this far: the reader runs `Document::check_limits`, which refuses the last. A blank name and a repeated
    // `ID` are what `validate` reports, which a test or an import report may run and an edit may not.)
    let built_by_hand: [(&str, Break); 3] = [
        ("duplicate page id", |d| d.pages.push(Arc::new(diacraft_doc::Page::new(1, "Twin")))),
        ("blank page name", |d| d.pages.push(Arc::new(diacraft_doc::Page::new(9, "  ")))),
        ("too many pages", |d| d.pages = (1..=1001).map(|id| Arc::new(diacraft_doc::Page::new(id, "p"))).collect()),
    ];
    for (what, make_it) in built_by_hand {
        let mut s = session_with_history();
        s.edit("Bring it in", |d| {
            make_it(d);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{what}: an edit does not audit the document: {e:?}"));
        assert!(s.document().is_some_and(|d| d.validate().is_err()), "{what}: the audit still sees it");
        let past_the_cap = s.document().is_some_and(|d| d.check_limits().is_err());
        assert_eq!(past_the_cap, what == "too many pages", "{what}: the door's check is about the caps");
        // And an unrelated edit goes through on that document, which is the point of F2.
        s.edit("Edit something else", |d| {
            d.title = format!("Renamed after {what}").into();
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
    }
    // A write that sets a name still checks it, and a refused write changes nothing.
    let before = snapshot(&s);
    for params in [json!({"id": 1, "name": "  "}), json!({"id": 1, "name": "two\nlines"}), json!({"id": 1, "name": "x".repeat(300)})] {
        let e = s.execute("page.rename", &params).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }), "{e:?}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_page_cannot_be_added_past_the_limit_or_without_an_id() {
    // The limits that `validate` used to catch after the fact are now met by the write that would pass them.
    let mut s = blank_session();
    while s.document().map(|d| d.pages.len()) < Some(diacraft_doc::MAX_PAGES) {
        run(&mut s, "page.add", json!({}));
    }
    let before = snapshot(&s);
    let e = s.execute("page.add", &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Failed { msg, .. } if msg.contains("at most")), "{e:?}");
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn edit_without_a_document_is_an_error() {
    let mut s = Session::new();
    let e = s.edit("Nothing to edit", |_| Ok(())).unwrap_err();
    assert!(matches!(&e, EngineError::Failed { msg, .. } if msg == "no document open"), "{e:?}");
    assert_eq!(s.revision(), 0);
}

#[test]
fn edit_shares_the_pages_it_did_not_touch() {
    let mut s = session_with_history();
    let before = s.document().cloned().unwrap();
    run(&mut s, "page.rename", json!({"id": 2, "name": "Changed"}));
    let after = s.document().unwrap();
    assert!(Arc::ptr_eq(after.pages.get(0).unwrap(), before.pages.get(0).unwrap()), "untouched page shared with the undo step");
    assert!(!Arc::ptr_eq(after.pages.get(1).unwrap(), before.pages.get(1).unwrap()));
    assert_eq!(&*before.pages.get(1).unwrap().name, "Second", "and the undo step still has the old name");
}

fn nested_adds(s: &mut Session, _: &Value) -> Result<Value> {
    s.execute("page.add", &json!({"name": "Inner 1"}))?;
    s.execute("page.add", &json!({"name": "Inner 2"}))?;
    Ok(json!({}))
}

#[test]
fn only_top_level_commands_are_journaled() {
    let mut s = blank_session();
    assert_eq!(s.journal(), [("file.new".to_owned(), json!({}))]);

    // A batch is one entry, with its steps inside it, not an entry per step.
    let params = json!({"commands": [{"command": "page.add", "params": {"name": "A"}}, {"command": "page.add"}], "label": "Two"});
    run(&mut s, "command.batch", params.clone());
    assert_eq!(s.journal().len(), 2);
    assert_eq!(s.journal()[1], ("command.batch".to_owned(), params));

    // So is a command that runs others.
    let outer = CommandSpec {
        id: "test.outer",
        label: "Outer",
        menu: &[],
        shortcut: None,
        params: "none",
        enabled: always,
        run: nested_adds as Run,
        journal: true,
    };
    s.run_spec(&outer, &json!({})).unwrap();
    assert_eq!(page_names(&s), ["Page-1", "A", "Page-3", "Inner 1", "Inner 2"]);
    assert_eq!(s.journal().len(), 3);
    assert_eq!(s.journal()[2].0, "test.outer");
}

#[test]
fn queries_failures_and_null_params_are_journaled_correctly() {
    let mut s = blank_session();
    run(&mut s, "app.version", json!({}));
    run(&mut s, "document.inspect", json!({}));
    s.execute("page.rename", &json!({"id": 99, "name": "x"})).unwrap_err();
    s.execute("page.add", &json!({"nam": "x"})).unwrap_err();
    s.execute("no.such", &json!({})).unwrap_err();
    assert_eq!(s.journal().len(), 1, "queries and commands that failed leave no entry: {:?}", s.journal());
    // Null params are journaled as the empty object they mean.
    s.execute("page.add", &Value::Null).unwrap();
    assert_eq!(s.journal()[1], ("page.add".to_owned(), json!({})));
}

#[test]
fn journal_keeps_the_most_recent_entries() {
    let mut s = blank_session();
    let total = JOURNAL_LIMIT + 25;
    for i in 0..total {
        run(&mut s, "page.rename", json!({"id": 1, "name": format!("n{i}")}));
    }
    assert_eq!(s.journal().len(), JOURNAL_LIMIT);
    // `file.new` and the first 25 renames are gone (26 entries over the bound); the rest are the newest, in order.
    assert_eq!(s.journal().first().map(|e| e.1["name"].clone()), Some(json!("n25")));
    assert_eq!(s.journal().last().map(|e| e.1["name"].clone()), Some(json!(format!("n{}", total - 1))));
    assert!(s.journal().windows(2).all(|w| w[0].1["name"] != w[1].1["name"]));
}

#[test]
fn history_keeps_the_most_recent_steps() {
    let mut s = blank_session();
    let total = History::DEFAULT_LIMIT + 5;
    for i in 0..total {
        run(&mut s, "page.rename", json!({"id": 1, "name": format!("n{i}")}));
    }
    assert_eq!(s.history().unwrap().undo_len(), History::DEFAULT_LIMIT);
    assert_eq!(check_session(&s), Ok(()));

    // Every kept step can be undone, and the oldest that was let go cannot.
    for _ in 0..History::DEFAULT_LIMIT {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(page_names(&s), ["n4"], "the steps for n0 to n4 were let go: n4 is as far back as undo reaches");
    assert!(matches!(s.execute("edit.undo", &json!({})), Err(EngineError::Disabled { .. })));
    // And forward again over the redo steps (they are bounded by the same limit).
    for _ in 0..History::DEFAULT_LIMIT {
        run(&mut s, "edit.redo", json!({}));
    }
    assert_eq!(page_names(&s), [format!("n{}", total - 1)]);
}

#[test]
fn the_history_limit_is_a_thousand_and_never_below_the_minimum() {
    assert_eq!(History::DEFAULT_LIMIT, 1_000);
    assert_eq!(History::MIN_STEPS, 8);
    assert_eq!(History::default().limit(), 1_000);
    assert_eq!(History::with_limit(0).limit(), 8);
    assert_eq!(History::with_limit(7).limit(), 8);
    assert_eq!(History::with_limit(8).limit(), 8);
    assert_eq!(History::with_limit(5_000).limit(), 5_000);
}

#[test]
fn unknown_key_is_rejected_with_nearest_name() {
    let mut s = blank_session();
    let before = snapshot(&s);

    let e = s.execute("page.add", &json!({"nam": "x"})).unwrap_err();

    assert!(matches!(e, EngineError::BadParams { ref cmd, .. } if cmd == "page.add"), "{e:?}");
    assert!(bad_params_msg(&e).contains("unknown parameter `nam`; did you mean `name`?"), "{e}");
    assert_eq!(snapshot(&s), before);
    // Case and a swapped pair of letters are near too; a key that is nothing like any lists them all.
    for (key, near) in [("Name", "name"), ("nmae", "name"), ("names", "name")] {
        let e = s.execute("page.add", &json!({ key: "x" })).unwrap_err();
        assert!(bad_params_msg(&e).contains(&format!("did you mean `{near}`?")), "{key}: {e}");
    }
    let e = s.execute("page.rename", &json!({"id": 1, "name": "x", "zzzzzz": 1})).unwrap_err();
    assert!(bad_params_msg(&e).contains("the command takes: id, name"), "{e}");
    let e = s.execute("edit.undo", &json!({"steps": 2})).unwrap_err();
    assert!(bad_params_msg(&e).contains("takes no parameters"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_very_long_unknown_key_is_cut_short_in_the_message() {
    let mut s = blank_session();
    let key = "k".repeat(100_000);
    let e = s.execute("page.add", &json!({ key: 1 })).unwrap_err();
    assert!(e.to_string().len() < 500, "{} bytes", e.to_string().len());
}

#[test]
fn an_unknown_command_is_named_and_cut_short_when_long() {
    let mut s = blank_session();
    let e = s.execute("page.ad", &json!({})).unwrap_err();
    assert_eq!(e, EngineError::UnknownCommand("page.ad".into()));
    assert_eq!(e.to_string(), "unknown command `page.ad`");
    let e = s.execute(&"x".repeat(100_000), &json!({})).unwrap_err();
    assert!(e.to_string().len() < 500, "{} bytes", e.to_string().len());
}

#[test]
fn empty_page_name_is_rejected() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for name in ["", "   ", "\t\n"] {
        let e = s.execute("page.add", &json!({ "name": name })).unwrap_err();
        assert!(bad_params_msg(&e).contains("must not be empty"), "{name:?}: {e}");
        let e = s.execute("page.rename", &json!({"id": 1, "name": name})).unwrap_err();
        assert!(bad_params_msg(&e).contains("must not be empty"), "{name:?}: {e}");
    }
    assert_eq!(snapshot(&s), before, "no page, no rename, no undo step, no journal entry");
}

/// Undo step names are checked by the rule page names follow, where every name enters the history
/// (`Session::grouped`'s check is tested in the crate, with the rest of the group tests).
#[test]
fn edit_refuses_a_step_name_that_breaks_the_name_rule() {
    let too_long = "x".repeat(256);
    let mut s = session_with_history();
    let before = snapshot(&s);
    for name in ["two\nlines", "tab\there", "nul\0", "del\u{7f}", "", "   ", too_long.as_str()] {
        let mut ran = false;
        let e = s
            .edit(name, |d| {
                ran = true;
                d.add_page(Some("Not added")).map(drop).map_err(|e| failed(&e.to_string()))
            })
            .unwrap_err();
        assert!(!ran, "{name:?}: the edit must not run under a bad name");
        assert!(matches!(&e, EngineError::Failed { msg, .. } if msg.contains("undo step name")), "{name:?}: {e:?}");
        assert!(!e.to_string().contains(char::is_control) && e.to_string().len() < 500, "the error must not echo the raw name: {e:?}");
        assert_eq!(snapshot(&s), before, "edit {name:?}");
    }
    // The reasons are the page-name rule's.
    let e = s.edit("a\nb", |_| Ok(())).unwrap_err();
    assert!(e.to_string().contains("must not contain control characters"), "{e}");
    let e = s.edit("", |_| Ok(())).unwrap_err();
    assert!(e.to_string().contains("must not be empty"), "{e}");
    // Ordinary names, spaces and non-ASCII letters included, still work.
    s.edit("Ajouter une page \u{e9}", |d| d.add_page(Some("Fine")).map(drop).map_err(|e| failed(&e.to_string()))).unwrap();
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Ajouter une page \u{e9}"));
}

/// Ids and keys come from the control channel and MCP, which are hostile: whatever they hold, an error
/// shows it escaped and cut short, never as raw control characters.
#[test]
fn hostile_ids_and_keys_are_escaped_in_every_error_that_echoes_them() {
    let hostile = "\u{1b}[2Jevil\nline";
    let mut s = blank_session();
    let before = snapshot(&s);
    let errors = [
        s.execute(hostile, &json!({})).unwrap_err(),
        s.execute("page.add", &json!({ hostile: 1 })).unwrap_err(),
        s.execute("command.batch", &json!({"commands": [{"command": hostile}]})).unwrap_err(),
        s.execute("command.batch", &json!({"commands": [{"command": "page.add", "params": { hostile: 1 }}]})).unwrap_err(),
        s.execute("command.batch", &json!({"commands": [{"command": "page.add", hostile: 1}]})).unwrap_err(),
        s.execute("command.batch", &json!({ hostile: [] })).unwrap_err(),
        s.edit(hostile, |_| Ok(())).unwrap_err(),
    ];
    for e in &errors {
        let text = e.to_string();
        assert!(!text.chars().any(char::is_control), "raw control character in {text:?}");
        assert!(text.contains("\\u{1b}[2Jevil\\nline"), "the escaped text is shown: {text}");
    }
    assert_eq!(snapshot(&s), before);

    // Cut at 64 characters of escaped text, and never in the middle of an escape.
    let long = "\u{1b}".repeat(100);
    let e = s.execute("page.add", &json!({ long: 1 })).unwrap_err();
    let text = e.to_string();
    assert!(text.contains('…') && text.len() < 400, "{} bytes: {text}", text.len());
    assert!(!text.replace("\\u{1b}", "").contains('\\'), "no cut escape: {text}");

    // Characters that are not control characters but spoof the text (right to left override) are escaped too.
    let e = s.execute("rtl\u{202e}spoof", &json!({})).unwrap_err();
    assert_eq!(e, EngineError::UnknownCommand("rtl\\u{202e}spoof".into()));

    // So are the line and paragraph separators: not control characters, but each starts a new line in a log or a window.
    for (c, escaped) in [('\u{2028}', "one\\u{2028}two"), ('\u{2029}', "one\\u{2029}two")] {
        let e = s.execute(&format!("one{c}two"), &json!({})).unwrap_err();
        assert_eq!(e, EngineError::UnknownCommand(escaped.into()), "U+{:04X}", c as u32);
    }
}

/// A panic in the closure of a direct `edit` (outside any command, so no guard puts anything back) must not
/// leave a half-made edit behind: the edit is made on a copy and only a finished, valid one replaces the document.
#[test]
fn a_panic_inside_an_edit_leaves_the_session_untouched() {
    let mut s = session_with_history();
    let before = snapshot(&s);

    let caught = diacraft_engine::guard::catch_panic(|| {
        s.edit::<()>("Doomed", |d| {
            d.title = "half edited".into();
            panic!("boom in an edit")
        })
    });

    assert!(caught.unwrap_err().contains("boom in an edit"));
    assert_eq!(snapshot(&s), before, "title, undo steps and revision");
    // It goes on working, and the next edit is a step of its own.
    run(&mut s, "page.add", json!({}));
    assert_eq!(s.history().unwrap().undo_len(), before.undo_steps + 1);
}
