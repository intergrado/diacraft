//! The comments of a drawing (`comment.add`, `comment.reply`, `comment.edit`, `comment.delete`, `comment.list`, `comment.markers`, `comment.next`,
//! `comment.previous`, `comment.author`), through `Session::execute` as the control channel, the CLI and MCP call them. The rules are the specs';
//! the decisions the specs leave open are in `docs/editing-commands.md`, "Comments".

use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::{blank_session, check_session, run, snapshot};

/// A session whose clock is pinned, so a comment's date is known, with an author named.
fn session() -> Session {
    let mut s = blank_session();
    // 2031-03-07T20:05:09Z
    s.pin_clock(Some(1_930_000_000 + 12_345_678));
    run(&mut s, "comment.author", json!({"name": "Ada Lovelace", "initials": "AL"}));
    s
}

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    match &e {
        EngineError::BadParams { msg, .. } | EngineError::Failed { msg, .. } | EngineError::Disabled { reason: msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

fn list(s: &mut Session, params: Json) -> Vec<Json> {
    run(s, "comment.list", params)["comments"].as_array().unwrap().clone()
}

#[test]
fn a_new_drawing_has_no_comments_and_shows_their_markers() {
    let mut s = blank_session();
    let answer = run(&mut s, "comment.list", json!({}));
    assert_eq!(answer["comments"], json!([]));
    assert_eq!(answer["markers"], json!(true));
}

#[test]
fn a_comment_on_a_shape_names_the_shape_the_page_the_author_and_the_time() {
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let added = run(&mut s, "comment.add", json!({"id": shape, "text": "Check this size"}));
    let c = &added["comment"];
    assert_eq!(c["id"], 1);
    assert_eq!((c["page"].as_u64(), c["shape"].as_u64()), (Some(1), Some(u64::from(shape))));
    assert_eq!((c["author"].as_str(), c["initials"].as_str()), (Some("Ada Lovelace"), Some("AL")));
    assert_eq!(c["text"], "Check this size");
    assert_eq!(c["parent"], Json::Null);
    let date = c["date"].as_str().unwrap();
    assert!(date.ends_with('Z') && date.len() == 20 && date.contains('T'), "UTC, as the fields are: {date}");
    assert_eq!(c["done"], false);
    assert_eq!(doc(&s).comments.len(), 1);
    assert_eq!(list(&mut s, json!({"id": shape})).len(), 1);
}

#[test]
fn a_comment_with_no_shape_is_the_pages() {
    let mut s = session();
    let added = run(&mut s, "comment.add", json!({"text": "About the page"}));
    assert_eq!(added["comment"]["shape"], Json::Null);
    assert_eq!(added["comment"]["page"], 1);
    // Asked for by the page, it is listed; asked for by a shape, it is not.
    assert_eq!(list(&mut s, json!({"page": 1})).len(), 1);
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    assert!(list(&mut s, json!({"id": shape})).is_empty());
}

#[test]
fn adding_a_comment_is_one_undo_step_and_one_journal_entry() {
    let mut s = session();
    let before = snapshot(&s);
    run(&mut s, "comment.add", json!({"text": "one"}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    assert_eq!(snapshot(&s).journal.len(), before.journal.len() + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).comments.len(), 0);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s).comments.len(), 1);
    check_session(&s).unwrap();
}

#[test]
fn adding_a_comment_turns_the_markers_on() {
    let mut s = session();
    run(&mut s, "comment.markers", json!({"show": false}));
    assert_eq!(run(&mut s, "comment.list", json!({}))["markers"], false);
    run(&mut s, "comment.add", json!({"text": "now they show"}));
    assert_eq!(run(&mut s, "comment.list", json!({}))["markers"], true);
}

#[test]
fn a_bad_comment_is_refused_in_words_and_changes_nothing() {
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = snapshot(&s);
    assert!(refused(&mut s, "comment.add", json!({"text": "   "})).contains("some text"));
    assert!(refused(&mut s, "comment.add", json!({"text": "a\u{0}b"})).contains("control character"));
    assert!(refused(&mut s, "comment.add", json!({"text": "x".repeat(70_000)})).contains("65536 bytes"));
    assert!(refused(&mut s, "comment.add", json!({"text": "t", "id": shape + 99})).contains("no shape"));
    assert!(refused(&mut s, "comment.add", json!({"text": "t", "page": 99})).contains("no page"));
    assert!(refused(&mut s, "comment.add", json!({})).contains("text"));
    assert!(refused(&mut s, "comment.add", json!({"text": 5})).contains("text"));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_reply_sits_with_its_comment_and_a_reply_to_a_reply_joins_the_thread() {
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let first = run(&mut s, "comment.add", json!({"id": shape, "text": "question"}))["comment"]["id"].as_u64().unwrap();
    let r = run(&mut s, "comment.reply", json!({"comment": first, "text": "answer"}));
    let reply = r["comment"]["id"].as_u64().unwrap();
    assert_eq!(r["comment"]["parent"], first);
    assert_eq!(r["comment"]["shape"].as_u64(), Some(u64::from(shape)));
    let again = run(&mut s, "comment.reply", json!({"comment": reply, "text": "and another"}));
    assert_eq!(again["comment"]["parent"], first, "a thread is one level deep");
    let all = list(&mut s, json!({}));
    assert_eq!(all.len(), 3);
    assert_eq!(all.iter().filter(|c| c["parent"].is_null()).count(), 1);
    assert!(refused(&mut s, "comment.reply", json!({"comment": 99, "text": "x"})).contains("no comment"));
}

#[test]
fn editing_changes_the_text_and_says_when() {
    let mut s = session();
    let id = run(&mut s, "comment.add", json!({"text": "before"}))["comment"]["id"].as_u64().unwrap();
    s.pin_clock(Some(1_930_000_000 + 12_345_678 + 3600));
    let e = run(&mut s, "comment.edit", json!({"comment": id, "text": "after"}));
    assert_eq!(e["comment"]["text"], "after");
    assert_ne!(e["comment"]["edit_date"], Json::Null);
    assert_ne!(e["comment"]["edit_date"], e["comment"]["date"]);
    assert!(refused(&mut s, "comment.edit", json!({"comment": id, "text": ""})).contains("some text"));
    assert!(refused(&mut s, "comment.edit", json!({"comment": 77, "text": "x"})).contains("no comment"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).comments.get(id as u32).unwrap().text.as_ref(), "before");
}

#[test]
fn deleting_a_comment_takes_its_replies_and_is_one_undo_step() {
    let mut s = session();
    let a = run(&mut s, "comment.add", json!({"text": "a"}))["comment"]["id"].as_u64().unwrap();
    run(&mut s, "comment.add", json!({"text": "b"}));
    run(&mut s, "comment.reply", json!({"comment": a, "text": "r"}));
    let gone = run(&mut s, "comment.delete", json!({"comment": a}));
    assert_eq!(gone["deleted"], 2);
    assert_eq!(doc(&s).comments.len(), 1);
    assert!(refused(&mut s, "comment.delete", json!({"comment": a})).contains("no comment"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).comments.len(), 3);
}

#[test]
fn a_comment_stays_with_its_shape_when_it_moves() {
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "comment.add", json!({"id": shape, "text": "stay"}));
    let at = |s: &mut Session| {
        let c = &list_with_anchors(s)[0];
        (c["anchor"]["x"].as_f64().unwrap(), c["anchor"]["y"].as_f64().unwrap(), c["shape"].as_u64().unwrap())
    };
    let (x0, y0, id0) = at(&mut s);
    run(&mut s, "shape.move", json!({"ids": [shape], "dx": 2.0, "dy": 1.0}));
    let (x1, y1, id1) = at(&mut s);
    assert_eq!(id0, id1, "it names the same shape");
    assert!((x1 - x0 - 2.0).abs() < 1e-6 && (y1 - y0 - 1.0).abs() < 1e-6, "its marker moved with the shape: {x0},{y0} to {x1},{y1}");
}

fn list_with_anchors(s: &mut Session) -> Vec<Json> {
    list(s, json!({"details": true}))
}

#[test]
fn a_page_comment_has_an_anchor_in_the_pages_corner() {
    let mut s = session();
    run(&mut s, "comment.add", json!({"text": "page"}));
    let c = &list_with_anchors(&mut s)[0];
    let (x, y) = (c["anchor"]["x"].as_f64().unwrap(), c["anchor"]["y"].as_f64().unwrap());
    assert!(x > 0.0 && y > 0.0, "inside the page: {x},{y}");
}

#[test]
fn deleting_a_shape_takes_its_comments_and_undo_brings_them_back() {
    let mut s = session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    let on_a = run(&mut s, "comment.add", json!({"id": a, "text": "on a"}))["comment"]["id"].as_u64().unwrap();
    run(&mut s, "comment.reply", json!({"comment": on_a, "text": "reply on a"}));
    run(&mut s, "comment.add", json!({"id": b, "text": "on b"}));
    run(&mut s, "shape.delete", json!({"ids": [a]}));
    let left = list(&mut s, json!({}));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["text"], "on b");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(list(&mut s, json!({})).len(), 3);
}

#[test]
fn deleting_a_page_takes_its_comments() {
    let mut s = session();
    run(&mut s, "comment.add", json!({"text": "on page 1"}));
    let two = run(&mut s, "page.add", json!({"name": "Two"}));
    let page = two["page"].as_u64().or_else(|| two["id"].as_u64()).unwrap();
    run(&mut s, "comment.add", json!({"text": "on page 2", "page": page}));
    run(&mut s, "page.delete", json!({"id": page}));
    let left = list(&mut s, json!({}));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["text"], "on page 1");
}

#[test]
fn a_copy_of_a_page_does_not_copy_its_comments() {
    let mut s = session();
    run(&mut s, "comment.add", json!({"text": "only on the first"}));
    run(&mut s, "page.duplicate", json!({}));
    assert_eq!(list(&mut s, json!({})).len(), 1);
}

#[test]
fn show_markup_is_a_setting_that_is_no_undo_step_and_marks_the_drawing_changed() {
    let mut s = session();
    run(&mut s, "comment.add", json!({"text": "t"}));
    let steps = snapshot(&s).undo_steps;
    let off = run(&mut s, "comment.markers", json!({"show": false}));
    assert_eq!(off["markers"], false);
    assert_eq!(snapshot(&s).undo_steps, steps, "no undo step (the View settings' rule)");
    // With no value it toggles.
    assert_eq!(run(&mut s, "comment.markers", json!({}))["markers"], true);
    assert_eq!(run(&mut s, "comment.markers", json!({}))["markers"], false);
}

#[test]
fn next_and_previous_step_through_the_threads_by_page_and_number_and_wrap() {
    let mut s = session();
    let one = run(&mut s, "comment.add", json!({"text": "one"}))["comment"]["id"].as_u64().unwrap();
    let two = run(&mut s, "comment.add", json!({"text": "two"}))["comment"]["id"].as_u64().unwrap();
    run(&mut s, "comment.reply", json!({"comment": one, "text": "reply"}));
    let three = run(&mut s, "comment.add", json!({"text": "three"}))["comment"]["id"].as_u64().unwrap();
    let step = |s: &mut Session, command: &str, from: Option<u64>| -> Json {
        match from {
            Some(f) => run(s, command, json!({"from": f})),
            None => run(s, command, json!({})),
        }
    };
    assert_eq!(step(&mut s, "comment.next", None)["comment"]["id"], one, "with no place, next is the first");
    assert_eq!(step(&mut s, "comment.next", Some(one))["comment"]["id"], two, "a reply is not a stop of its own");
    assert_eq!(step(&mut s, "comment.next", Some(two))["comment"]["id"], three);
    let wrapped = step(&mut s, "comment.next", Some(three));
    assert_eq!((wrapped["comment"]["id"].as_u64(), wrapped["wrapped"].as_bool()), (Some(one), Some(true)));
    assert_eq!(step(&mut s, "comment.previous", Some(two))["comment"]["id"], one);
    assert_eq!(step(&mut s, "comment.previous", Some(one))["comment"]["id"], three);
    assert_eq!(step(&mut s, "comment.previous", None)["comment"]["id"], three, "with no place, previous is the last");
    // A reply as the place stands for its thread.
    let reply = list(&mut s, json!({})).iter().find(|c| !c["parent"].is_null()).unwrap()["id"].as_u64().unwrap();
    assert_eq!(step(&mut s, "comment.next", Some(reply))["comment"]["id"], two);
}

#[test]
fn next_with_no_comments_says_none() {
    let mut s = session();
    let a = run(&mut s, "comment.next", json!({}));
    assert_eq!(a["comment"], Json::Null);
}

#[test]
fn the_author_comes_from_a_setting_and_initials_are_made_from_the_name() {
    let mut s = blank_session();
    s.pin_clock(Some(2_000_000_000));
    let none_set = run(&mut s, "comment.author", json!({}));
    assert_eq!(none_set["name"], "Author");
    let set = run(&mut s, "comment.author", json!({"name": "Grace Brewster Murray Hopper"}));
    assert_eq!(set["initials"], "GBM");
    let c = run(&mut s, "comment.add", json!({"text": "hi"}));
    assert_eq!(c["comment"]["author"], "Grace Brewster Murray Hopper");
    assert!(refused(&mut s, "comment.author", json!({"name": "  "})).contains("name"));
    assert!(refused(&mut s, "comment.author", json!({"name": "x".repeat(300)})).contains("at most"));
    assert!(refused(&mut s, "comment.author", json!({"name": "a\u{7}b"})).contains("control"));
    // The same author twice is one author of the file.
    run(&mut s, "comment.add", json!({"text": "again"}));
    assert_eq!(doc(&s).comments.authors().count(), 1);
}

#[test]
fn the_list_is_paged_and_capped() {
    let mut s = session();
    for i in 0..5 {
        run(&mut s, "comment.add", json!({"text": format!("c{i}")}));
    }
    let first = run(&mut s, "comment.list", json!({"limit": 2}));
    assert_eq!(first["comments"].as_array().unwrap().len(), 2);
    assert_eq!(first["more"], true);
    let next = run(&mut s, "comment.list", json!({"limit": 2, "after": first["comments"][1]["id"]}));
    assert_eq!(next["comments"][0]["text"], "c2");
    assert!(refused(&mut s, "comment.list", json!({"limit": 0})).contains("limit"));
    assert!(refused(&mut s, "comment.list", json!({"limit": 1_000_000})).contains("limit"));
    let capped = run(&mut s, "comment.list", json!({}));
    assert_eq!(capped["comments"].as_array().unwrap().len(), 5);
}

#[test]
fn the_commands_need_a_document_and_the_queries_do_not_journal() {
    let mut s = Session::new();
    assert!(matches!(s.execute("comment.add", &json!({"text": "t"})), Err(EngineError::Disabled { .. })));
    assert!(matches!(s.execute("comment.list", &json!({})), Err(EngineError::Disabled { .. })));
    let mut s = session();
    let before = snapshot(&s).journal.len();
    run(&mut s, "comment.list", json!({}));
    run(&mut s, "comment.next", json!({}));
    assert_eq!(snapshot(&s).journal.len(), before);
}

#[test]
fn a_shape_gone_from_the_page_leaves_its_comment_shown_as_the_pages() {
    // A file may hold a comment on a shape that is not there; the comment is kept, and its anchor is the page's.
    let mut s = session();
    run(&mut s, "comment.add", json!({"text": "orphan"}));
    s.edit("test: a comment on a shape that is gone", |d| {
        let mut c = (**d.comments.get(1).unwrap()).clone();
        c.shape = Some(4242);
        let rebuilt = diacraft_doc::Comments::from_parts(
            d.comments.show_tags,
            diacraft_doc::RawAttrs::new(),
            d.comments.authors().map(|a| (**a).clone()).collect(),
            vec![c],
        )
        .unwrap();
        d.comments = rebuilt;
        Ok(())
    })
    .unwrap();
    let c = &list_with_anchors(&mut s)[0];
    assert_eq!(c["shape"], 4242);
    assert_eq!(c["shape_gone"], true);
    assert!(c["anchor"]["x"].as_f64().unwrap() > 0.0);
}

#[test]
fn a_comment_on_a_shape_names_the_shapes_text_for_the_pane() {
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.set_text", json!({"id": shape, "text": "Create in P2 Desktop"}));
    run(&mut s, "comment.add", json!({"id": shape, "text": "t"}));
    assert_eq!(list(&mut s, json!({"details": true}))[0]["shape_text"], "Create in P2 Desktop");
}

#[test]
fn comments_are_not_in_what_an_export_or_a_print_draws() {
    // "Comments are not printed": the markers are the window's, and the page's picture is the same with a comment on it.
    let dir = std::env::temp_dir().join(format!("comments-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    let shape = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = dir.join("before.png");
    run(&mut s, "file.export", json!({"path": before.to_str().unwrap()}));
    run(&mut s, "comment.add", json!({"id": shape, "text": "not printed"}));
    let after = dir.join("after.png");
    run(&mut s, "file.export", json!({"path": after.to_str().unwrap()}));
    assert_eq!(std::fs::read(&before).unwrap(), std::fs::read(&after).unwrap(), "the picture of the page is the same with a comment on it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drawing_whose_comments_part_could_not_be_read_takes_no_new_comment_in_words() {
    // A VSDX file whose comments part the reader keeps as bytes (it is too large, or holds what the model cannot): a comment added would be written as a second comments part.
    let mut s = session();
    s.edit("test: a held comments part", |d| {
        d.parts.push(std::sync::Arc::new(diacraft_doc::Part {
            format: diacraft_doc::Format::Vsdx,
            name: std::sync::Arc::from("/visio/comments.xml"),
            media_type: std::sync::Arc::from("application/vnd.ms-visio.comments+xml"),
            role: diacraft_doc::PartRole::Other,
            manifest: diacraft_doc::RawAttrs::new(),
            rels: std::sync::Arc::new(Vec::new()),
            bytes: std::sync::Arc::from(&b"<Comments/>"[..]),
        }));
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let words = refused(&mut s, "comment.add", json!({"text": "t"}));
    assert!(words.contains("comments part") && words.contains("could not be read"), "{words}");
    let words = refused(&mut s, "comment.reply", json!({"comment": 1, "text": "t"}));
    assert!(words.contains("comments part"), "{words}");
    assert_eq!(snapshot(&s), before);
}
