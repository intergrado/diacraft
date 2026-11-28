//! The cap on the name a command gives a page: **a page name has at most 31 characters** (the Page Setup dialog's Page Properties tab, rule
//! and its vector V5). `page.rename` and every command that names a new page (`page.add`, with or without `background`, and the
//! copy `page.duplicate` makes) refuse a longer name in words and change nothing; **a name a file holds stays as it is, however long**, up to the model's own 255 characters.

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::edit::doc;
use crate::native_common::Dir;
use crate::{bad_params_msg, blank_session, check_session, page_names, run, snapshot};

const WORDS: &str = "a page name must be at most 31 characters";

fn refused(s: &mut Session, command: &str, params: Value) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    assert!(matches!(&e, EngineError::BadParams { cmd, .. } if cmd == command), "{command} {params}: {e:?}");
    bad_params_msg(&e).to_owned()
}

/// A drawing that holds a page named by a file: 40 characters, which no command could have given it (the model's own limit is 255).
fn with_a_long_name_from_a_file() -> Session {
    let mut s = blank_session();
    s.edit("a name a file held", |d| d.rename_page(1, &"n".repeat(40)).map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() }))
        .unwrap();
    s
}

#[test]
fn a_rename_to_31_characters_is_made_and_to_32_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let at_most = "x".repeat(31);
    run(&mut s, "page.rename", json!({"id": 1, "name": at_most}));
    assert_eq!(page_names(&s), [at_most.as_str()]);
    let before = snapshot(&s);
    let e = refused(&mut s, "page.rename", json!({"id": 1, "name": "x".repeat(32)}));
    assert!(e.contains(WORDS), "{e}");
    assert_eq!(snapshot(&s), before, "no name, no undo step, no journal entry");
    // A name far past it, which the model's own limit would have refused with other words, says the same.
    let e = refused(&mut s, "page.rename", json!({"id": 1, "name": "x".repeat(300)}));
    assert!(e.contains(WORDS), "{e}");
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn the_31_are_characters_and_not_bytes() {
    let mut s = blank_session();
    // 31 characters of 2 bytes each, and one of 4 bytes; 32 of them are refused.
    let wide = "é".repeat(31);
    assert_eq!(wide.len(), 62);
    run(&mut s, "page.rename", json!({"id": 1, "name": wide}));
    let emoji = "\u{1f600}".repeat(31);
    run(&mut s, "page.rename", json!({"id": 1, "name": emoji}));
    let e = refused(&mut s, "page.rename", json!({"id": 1, "name": "\u{1f600}".repeat(32)}));
    assert!(e.contains(WORDS), "{e}");
    assert_eq!(page_names(&s), ["\u{1f600}".repeat(31)]);
}

#[test]
fn a_new_page_named_by_page_add_is_held_to_it_and_so_is_a_background_page() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in
        [json!({"name": "y".repeat(32)}), json!({"name": "y".repeat(32), "background": true}), json!({"name": "y".repeat(1_000), "like": 1})]
    {
        let e = refused(&mut s, "page.add", params.clone());
        assert!(e.contains(WORDS), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before, "no page was added");
    run(&mut s, "page.add", json!({"name": "y".repeat(31)}));
    run(&mut s, "page.add", json!({"name": "b".repeat(31), "background": true}));
    assert_eq!(page_names(&s), ["b".repeat(31), "Page-1".to_owned(), "y".repeat(31)]);
    // A page with no name is named as it was.
    run(&mut s, "page.add", json!({}));
    run(&mut s, "page.add", json!({"background": true}));
    assert!(page_names(&s).iter().all(|n| n.chars().count() <= 31), "{:?}", page_names(&s));
    check_session(&s).unwrap();
}

#[test]
fn a_copy_of_a_page_is_named_within_the_31_and_cut_short_to_take_its_number() {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "n".repeat(31)}));
    let made = run(&mut s, "page.duplicate", json!({"id": 1}));
    let name = made["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 31, "{name}");
    assert!(name.ends_with("-2"), "{name}");
    assert!(name.starts_with(&"n".repeat(29)), "{name}");
    // The copy of a copy, and a name that is short, are as they were.
    let again = run(&mut s, "page.duplicate", json!({"id": 1}));
    assert!(again["name"].as_str().unwrap().ends_with("-3"), "{again}");
    let short = run(&mut s, "page.add", json!({"name": "Overview"}));
    let copy = run(&mut s, "page.duplicate", json!({"id": short["id"]}));
    assert_eq!(copy["name"], "Overview-2");
    check_session(&s).unwrap();
}

#[test]
fn a_page_whose_name_a_file_gave_is_copied_to_a_name_within_the_31() {
    let mut s = with_a_long_name_from_a_file();
    assert_eq!(page_names(&s), ["n".repeat(40)]);
    let made = run(&mut s, "page.duplicate", json!({"id": 1}));
    let name = made["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 31, "a command named the copy: {name}");
    assert!(name.ends_with("-2"), "{name}");
    assert_eq!(page_names(&s)[0], "n".repeat(40), "the page the file named is as it was");
}

#[test]
fn a_name_from_a_file_stays_as_it_is_and_is_not_asked_again_until_it_is_renamed() {
    let mut s = with_a_long_name_from_a_file();
    let long = "n".repeat(40);
    // Other edits leave it alone, and the audit (which no edit runs) takes it.
    run(&mut s, "page.add", json!({"name": "Second"}));
    run(&mut s, "page.move", json!({"id": 2, "to": 0}));
    assert!(page_names(&s).contains(&long));
    check_session(&s).unwrap();
    // A command that names the page again is held to the cap, the same name included.
    let e = refused(&mut s, "page.rename", json!({"id": 1, "name": long.clone()}));
    assert!(e.contains(WORDS), "{e}");
    run(&mut s, "page.rename", json!({"id": 1, "name": "Short"}));
    assert!(!page_names(&s).contains(&long));
}

#[test]
fn a_long_name_a_file_holds_is_saved_and_opened_again_as_it_is_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = with_a_long_name_from_a_file();
        let name = "n".repeat(40);
        let dir = Dir::new();
        let path = dir.path(&format!("long.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = Session::new();
        run(&mut back, "file.open", json!({ "path": path }));
        assert_eq!(page_names(&back), [name.as_str()], "{ext}: the name is as it was");
        // The drawing opened from the file takes other edits, and saves again with the name as it was.
        run(&mut back, "page.add", json!({"name": "Second"}));
        run(&mut back, "file.save", json!({}));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": path }));
        assert_eq!(page_names(&again), [name.as_str(), "Second"], "{ext}");
        assert!(doc(&again).validate().is_ok(), "{ext}");
    }
}

#[test]
fn the_commands_params_say_the_limit() {
    let spec = |id: &str| diacraft_engine::command_specs().iter().find(|c| c.id == id).map_or("", |c| c.params);
    for id in ["page.add", "page.rename"] {
        assert!(spec(id).contains("at most 31 characters"), "{id}: {}", spec(id));
    }
}
