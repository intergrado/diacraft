//! **No two pages of a drawing have one name, and no page is named "page" and digits.** The program that opens a drawing file renames a page called `page1` (the name an `.odg`
//! gives its pages) and a name that another page has, so the commands that name a page (`page.add`, `page.rename`, the copy `page.duplicate` makes, the background page, and the renumbering
//! `page.move` does) refuse such a name in words and change nothing. Names are compared without regard to case.

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::{bad_params_msg, blank_session, check_session, page_names, run, snapshot};

const TAKEN: &str = "another page has this name";
const DIGITS: &str = "\"page\" and digits";

fn refused(s: &mut Session, command: &str, params: Value) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    assert!(matches!(&e, EngineError::BadParams { cmd, .. } if cmd == command), "{command} {params}: {e:?}");
    bad_params_msg(&e).to_owned()
}

#[test]
fn page_add_refuses_a_name_another_page_has_whatever_its_case_and_changes_nothing() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Overview"}));
    let before = snapshot(&s);
    for name in ["Overview", "overview", "OVERVIEW", "Page-1", "page-1"] {
        let e = refused(&mut s, "page.add", json!({"name": name}));
        assert!(e.contains(TAKEN), "{name}: {e}");
        let e = refused(&mut s, "page.add", json!({"name": name, "background": true}));
        assert!(e.contains(TAKEN), "{name} as a background page: {e}");
        let e = refused(&mut s, "page.add", json!({"name": name, "like": 1}));
        assert!(e.contains(TAKEN), "{name} with a setup: {e}");
    }
    assert_eq!(snapshot(&s), before, "no page, no undo step, no journal entry");
    check_session(&s).unwrap();
}

#[test]
fn page_add_refuses_page_and_digits_in_any_case_and_changes_nothing() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for name in ["page1", "Page2", "PAGE10", "page0"] {
        for params in [json!({"name": name}), json!({"name": name, "background": true})] {
            let e = refused(&mut s, "page.add", params.clone());
            assert!(e.contains(DIGITS), "{params}: {e}");
        }
    }
    assert_eq!(snapshot(&s), before);
    // The default form has a hyphen, and a name that holds more is another name: both are taken when no page has them.
    run(&mut s, "page.add", json!({"name": "Page 3"}));
    run(&mut s, "page.add", json!({"name": "page4a"}));
    run(&mut s, "page.add", json!({"name": "Page-9"}));
    assert_eq!(page_names(&s), ["Page-1", "Page 3", "page4a", "Page-9"]);
}

#[test]
fn page_rename_refuses_a_name_another_page_has_and_page_and_digits_and_may_give_a_page_its_own_name_back() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Overview"}));
    let before = snapshot(&s);
    for name in ["Overview", "OVERVIEW", "overview"] {
        let e = refused(&mut s, "page.rename", json!({"id": 1, "name": name}));
        assert!(e.contains(TAKEN), "{name}: {e}");
    }
    for name in ["page1", "Page2", "PAGE10"] {
        let e = refused(&mut s, "page.rename", json!({"id": 2, "name": name}));
        assert!(e.contains(DIGITS), "{name}: {e}");
    }
    assert_eq!(snapshot(&s), before, "no rename, no undo step, no journal entry");
    // A page is given the name it has, and the same name in another case.
    run(&mut s, "page.rename", json!({"id": 2, "name": "Overview"}));
    run(&mut s, "page.rename", json!({"id": 2, "name": "OVERVIEW"}));
    run(&mut s, "page.rename", json!({"id": 1, "name": "Page-1"}));
    assert_eq!(page_names(&s), ["Page-1", "OVERVIEW"]);
    check_session(&s).unwrap();
}

#[test]
fn a_page_with_no_name_is_called_by_a_name_no_page_has_whatever_its_case() {
    let mut s = blank_session();
    // The next id is 2 and a page already has that name in another case.
    run(&mut s, "page.add", json!({"name": "page-2"}));
    let made = run(&mut s, "page.add", json!({}));
    assert_eq!(made["name"], "Page-3", "{made}");
    // A background page: the first VBackground-N no page has, in any case.
    run(&mut s, "page.add", json!({"name": "vbackground-1", "background": true}));
    let back = run(&mut s, "page.add", json!({"background": true}));
    assert_eq!(back["name"], "VBackground-2", "{back}");
    let names = page_names(&s);
    let mut keys: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), names.len(), "no two pages have one name: {names:?}");
}

#[test]
fn a_copy_of_a_page_is_named_by_a_name_no_page_has_whatever_its_case() {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    run(&mut s, "page.add", json!({"name": "overview-2"}));
    let made = run(&mut s, "page.duplicate", json!({"id": 1}));
    assert_eq!(made["name"], "Overview-3", "{made}");
    let again = run(&mut s, "page.duplicate", json!({"id": 1}));
    assert_eq!(again["name"], "Overview-4", "{again}");
}

#[test]
fn a_renumbering_that_would_give_a_page_the_name_of_another_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    // A background page called Page-3, and then three pages that are not backgrounds: the second and third are called Page-4 and Page-5 (the ids 3 and 4 name them by the next free number).
    run(&mut s, "page.add", json!({"name": "Page-3", "background": true}));
    run(&mut s, "page.add", json!({}));
    run(&mut s, "page.add", json!({}));
    assert_eq!(page_names(&s), ["Page-3", "Page-1", "Page-4", "Page-5"]);
    let before = snapshot(&s);
    // Renumbered for their places, the third would be Page-3, which the background page has.
    let e = refused(&mut s, "page.move", json!({"id": 4, "to": 0, "renumber": true}));
    assert!(e.contains("\"Page-3\", which another page has"), "{e}");
    assert_eq!(snapshot(&s), before, "nothing moved and nothing renamed");
    // Without renumbering the move is made, and a renumbering with no clash is made.
    run(&mut s, "page.move", json!({"id": 4, "to": 0}));
    assert_eq!(page_names(&s), ["Page-3", "Page-5", "Page-1", "Page-4"]);
    run(&mut s, "page.rename", json!({"id": 2, "name": "Background"}));
    let moved = run(&mut s, "page.move", json!({"id": 4, "to": 2, "renumber": true}));
    assert_eq!(moved["renamed"], 2, "{moved}");
    assert_eq!(page_names(&s), ["Background", "Page-1", "Page-2", "Page-3"]);
    check_session(&s).unwrap();
}
