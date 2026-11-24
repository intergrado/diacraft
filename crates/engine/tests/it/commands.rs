//! The commands one by one: what each returns, changes and refuses.

use diacraft_engine::{CommandInfo, EngineError, Session, command_specs, find_command, param_keys};
use serde_json::{Value, json};

use crate::{bad_params_msg, blank_session, page_names, run, session_with_history, snapshot};

#[test]
fn a_session_starts_without_a_document() {
    let s = Session::new();
    assert_eq!(s.document(), None);
    assert_eq!(s.revision(), 0);
    assert!(s.journal().is_empty());
    assert_eq!(s.panics_caught(), 0);
    assert!(s.history().is_none());
}

#[test]
fn a_session_can_move_to_another_thread() {
    fn needs_send<T: Send>(_: &T) {}
    needs_send(&Session::new());
}

#[test]
fn app_version_names_the_app_the_version_the_git_release_and_the_year_without_a_document() {
    let mut s = Session::new();
    let v = run(&mut s, "app.version", json!({}));
    // No application has told the engine its build here: the git release is `unknown` and the year the last resort, 2026.
    assert_eq!(v, json!({"app": "diacraft", "version": env!("CARGO_PKG_VERSION"), "git": "unknown", "year": "2026"}));
    assert_eq!((s.revision(), s.journal().len()), (0, 0), "a query changes and records nothing");
}

#[test]
fn file_new_makes_a_blank_document() {
    let mut s = Session::new();
    let result = run(&mut s, "file.new", json!({}));
    // The blank document, with the four Snap & Glue words a new drawing is given.
    assert_eq!(s.document(), Some(&diacraft_engine::snap::blank_drawing()));
    assert_eq!(result["pages"], json!([{"id": 1, "name": "Page-1"}]));
    assert_eq!(s.revision(), 1);
    assert_eq!((s.history().unwrap().undo_len(), s.history().unwrap().redo_len()), (0, 0));
}

#[test]
fn file_new_replaces_the_document_and_starts_a_fresh_history() {
    let mut s = session_with_history();
    run(&mut s, "edit.undo", json!({}));
    let revision = s.revision();

    run(&mut s, "file.new", json!({}));

    assert_eq!(page_names(&s), ["Page-1"]);
    assert_eq!(s.revision(), revision + 1);
    assert_eq!((s.history().unwrap().undo_len(), s.history().unwrap().redo_len()), (0, 0));
    for id in ["edit.undo", "edit.redo"] {
        assert!(matches!(s.execute(id, &json!({})), Err(EngineError::Disabled { .. })), "{id}");
    }
    assert_eq!(s.journal().last().map(|e| e.0.as_str()), Some("file.new"));
}

#[test]
fn document_inspect_reports_the_title_pages_and_history() {
    let mut s = session_with_history();
    run(&mut s, "edit.undo", json!({}));
    let v = run(&mut s, "document.inspect", json!({}));
    assert_eq!(
        v,
        json!({
            "title": "Untitled",
            "version": 1,
            "pages": [{"id": 1, "name": "Baseline"}],
            "history": {"undo": 1, "redo": 1},
        })
    );
}

#[test]
fn document_inspect_needs_a_document_and_changes_nothing() {
    let mut s = Session::new();
    let e = s.execute("document.inspect", &json!({})).unwrap_err();
    assert_eq!(e, EngineError::Disabled { cmd: "document.inspect".into(), reason: "no document open".into() });
    let mut s = session_with_history();
    let before = snapshot(&s);
    run(&mut s, "document.inspect", json!({}));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn page_add_names_the_page_and_says_which() {
    let mut s = blank_session();
    let revision = s.revision();
    assert_eq!(run(&mut s, "page.add", json!({})), json!({"id": 2, "name": "Page-2"}));
    assert_eq!(run(&mut s, "page.add", json!({"name": "  Spaced  "})), json!({"id": 3, "name": "  Spaced  "}));
    assert_eq!(run(&mut s, "page.add", Value::Null), json!({"id": 4, "name": "Page-4"}));
    assert_eq!(run(&mut s, "page.add", json!({"name": null})), json!({"id": 5, "name": "Page-5"}));
    assert_eq!(page_names(&s), ["Page-1", "Page-2", "  Spaced  ", "Page-4", "Page-5"]);
    assert_eq!(s.revision(), revision + 4);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Add page"));
}

#[test]
fn page_add_with_a_name_that_is_not_text_is_refused() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for name in [json!(5), json!(true), json!([]), json!({}), json!(["a"])] {
        let e = s.execute("page.add", &json!({ "name": name })).unwrap_err();
        assert!(bad_params_msg(&e).contains("`name` must be text"), "{name}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn page_add_stops_at_the_page_limit() {
    let mut s = blank_session();
    for _ in 1..diacraft_doc::MAX_PAGES {
        run(&mut s, "page.add", json!({}));
    }
    assert_eq!(s.document().map(|d| d.pages.len()), Some(diacraft_doc::MAX_PAGES));
    let before = snapshot(&s);
    let e = s.execute("page.add", &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Failed { cmd, msg } if cmd == "page.add" && msg.contains("at most 1000 pages")), "{e:?}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn page_rename_renames_and_says_which() {
    let mut s = session_with_history();
    let result = run(&mut s, "page.rename", json!({"id": 2, "name": "Renamed"}));
    assert_eq!(result, json!({"id": 2, "name": "Renamed"}));
    assert_eq!(page_names(&s), ["Baseline", "Renamed"]);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Rename page"));
    // Renaming a page to the name it has is allowed.
    run(&mut s, "page.rename", json!({"id": 2, "name": "Renamed"}));
}

#[test]
fn page_rename_refuses_bad_ids_and_names() {
    let mut s = session_with_history();
    let before = snapshot(&s);
    for (params, words) in [
        (json!({}), "missing `id`"),
        (json!({"name": "x"}), "missing `id`"),
        (json!({"id": 1}), "missing `name`"),
        (json!({"id": null, "name": "x"}), "missing `id`"),
        (json!({"id": "1", "name": "x"}), "`id` must be a whole number"),
        (json!({"id": 1.5, "name": "x"}), "`id` must be a whole number"),
        (json!({"id": 1.0, "name": "x"}), "`id` must be a whole number"),
        (json!({"id": -1, "name": "x"}), "`id` must be a whole number"),
        (json!({"id": 4_294_967_296_u64, "name": "x"}), "`id` must be a whole number"),
        (json!({"id": u64::MAX, "name": "x"}), "`id` must be a whole number"),
        (json!({"id": [1], "name": "x"}), "`id` must be a whole number"),
        (json!({"id": 99, "name": "x"}), "no page has id 99"),
        (json!({"id": 0, "name": "x"}), "no page has id 0"),
        (json!({"id": 1, "name": 5}), "`name` must be text"),
        (json!({"id": 1, "name": null}), "missing `name`"),
        (json!({"id": 1, "name": "a\nb"}), "control characters"),
        (json!({"id": 1, "name": "x".repeat(256)}), "at most 255 characters"),
    ] {
        let e = s.execute("page.rename", &params).unwrap_err();
        assert!(matches!(&e, EngineError::BadParams { cmd, .. } if cmd == "page.rename"), "{params}: {e:?}");
        assert!(bad_params_msg(&e).contains(words), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn params_that_are_not_an_object_are_refused_and_null_means_none() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for junk in [json!(42), json!("name"), json!([]), json!([{"name": "x"}]), json!(true), json!(1.5)] {
        for id in ["page.add", "app.version", "file.new", "edit.undo"] {
            let e = s.execute(id, &junk).unwrap_err();
            assert!(bad_params_msg(&e).contains("must be a JSON object"), "{id} {junk}: {e}");
        }
    }
    assert_eq!(snapshot(&s), before);
    assert!(s.execute("app.version", &Value::Null).is_ok());
}

#[test]
fn command_info_lists_every_command_with_its_enablement() {
    let mut s = Session::new();
    let info = s.commands();
    assert_eq!(info.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), command_specs().iter().map(|c| c.id).collect::<Vec<_>>());

    let find = |info: &[CommandInfo], id: &str| info.iter().find(|c| c.id == id).cloned().unwrap();
    let new = find(&info, "file.new");
    assert_eq!(new.label, "New");
    assert_eq!(new.menu, ["File"]);
    assert_eq!(new.shortcut.as_deref(), Some("Cmd+N"));
    assert_eq!(new.params, find_command("file.new").unwrap().params);
    assert!(new.enabled && new.disabled_reason.is_none());
    let add = find(&info, "page.add");
    assert_eq!((add.enabled, add.disabled_reason.as_deref()), (false, Some("no document open")));

    run(&mut s, "file.new", json!({}));
    let add = find(&s.commands(), "page.add");
    assert_eq!((add.enabled, add.disabled_reason), (true, None));
}

#[test]
fn command_info_serializes_with_the_reason_only_when_disabled() {
    let s = Session::new();
    let info = s.commands();
    let find = |id: &str| serde_json::to_value(info.iter().find(|c| c.id == id).unwrap()).unwrap();
    let new = find("file.new");
    assert_eq!(new["id"], "file.new");
    assert_eq!(new["menu"], json!(["File"]));
    assert_eq!(new["enabled"], true);
    assert!(new.get("disabled_reason").is_none(), "{new}");
    let add = find("page.add");
    assert_eq!(add["enabled"], false);
    assert_eq!(add["disabled_reason"], "no document open");
}

#[test]
fn param_keys_reads_a_params_doc() {
    assert_eq!(param_keys("none"), Ok(vec![]));
    assert_eq!(param_keys("name: the name"), Ok(vec!["name"]));
    assert_eq!(param_keys("id: page id; name: new name, as: text (default: x)"), Ok(vec!["id", "name"]));
    assert_eq!(param_keys("a1_b: x;c: y"), Ok(vec!["a1_b", "c"]));
    for malformed in
        ["", " ", "nothing", "name", "name -x", "Name: x", "1a: x", "name: x;", "name: x; ; id: y", "name: x; name: y", ": x", "na me: x"]
    {
        assert!(param_keys(malformed).is_err(), "{malformed:?} should be refused");
    }
}

/// The params docs of `file.new` and `stencil.open` name every template and every built-in stencil there is (they named the first two of each after a change
/// had added theirs), so a program that reads the registry learns of all of them.
#[test]
fn the_params_of_file_new_and_stencil_open_name_every_template_and_every_built_in_stencil() {
    let new = find_command("file.new").unwrap().params;
    for t in diacraft_engine::template::templates() {
        assert!(new.contains(t.id), "file.new does not name the template {}: {new}", t.id);
    }
    let open = find_command("stencil.open").unwrap().params;
    for st in diacraft_engine::stencil::stencils() {
        assert!(
            open.contains(&format!(" {},", st.id)) || open.contains(&format!(" {} ", st.id)) || open.contains(&format!(" {})", st.id)),
            "stencil.open does not name {}: {open}",
            st.id
        );
    }
}
