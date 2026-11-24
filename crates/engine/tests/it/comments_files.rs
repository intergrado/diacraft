//! The comments of a drawing through the files a person saves them in: an `.odg` (written from the model and spliced into a file), a `.vsdx` (replies ride in
//! DiaCraft's own part) and a `.diacraft`. One leg a session (`file.save_as`, then `file.open`); every file is built by the engine itself.

use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::drop_shape;
use crate::native_common::Dir;
use crate::{blank_session, run};

/// A drawing of two shapes with a comment on the first, a reply to it, one on the page, and one edited: what a reviewer leaves.
fn reviewed() -> Session {
    let mut s = blank_session();
    s.pin_clock(Some(1_930_000_000 + 12_345_678));
    run(&mut s, "comment.author", json!({"name": "Ada Lovelace", "initials": "AL"}));
    let first = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let _second = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    let root = run(&mut s, "comment.add", json!({"id": first, "text": "Check this size\nand the colour"}))["comment"]["id"].as_u64().unwrap();
    run(&mut s, "comment.author", json!({"name": "Bob", "initials": "B"}));
    run(&mut s, "comment.reply", json!({"comment": root, "text": "Done & dusted <ok>"}));
    run(&mut s, "comment.add", json!({"text": "About the page"}));
    run(&mut s, "comment.edit", json!({"comment": root, "text": "Check this size\nand the colour, again"}));
    s
}

/// What the comments of a session say: number, parent, whether on a shape, author and text, in order.
fn said(s: &mut Session) -> Vec<(u64, Json, bool, String, String)> {
    run(s, "comment.list", json!({}))["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_u64().unwrap(),
                c["parent"].clone(),
                c["shape"].is_u64(),
                c["author"].as_str().unwrap().to_owned(),
                c["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn reopened(dir: &Dir, name: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": dir.path(name) })).unwrap_or_else(|e| panic!("{name} did not open: {e}"));
    s
}

#[test]
fn an_odg_written_from_a_drawing_holds_its_comments_and_says_nothing_of_leaving_them_out() {
    let dir = Dir::new();
    let mut s = reviewed();
    let before = said(&mut s);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("reviewed.odg") }));
    let warnings = saved["warnings"].as_array().unwrap();
    assert!(warnings.iter().all(|w| !w.as_str().unwrap_or("").contains("comment")), "{warnings:?}");
    // The check after the save finds no class `comments` that differs.
    let classes = saved["checked"]["classes"].as_array().cloned().unwrap_or_default();
    assert!(classes.iter().all(|c| c["class"] != "comments"), "{classes:?}");
    let mut back = reopened(&dir, "reviewed.odg");
    assert_eq!(said(&mut back), before, "the same comments, replies and shapes");
    // A comment on a shape is still on it.
    let ties: Vec<u64> =
        run(&mut back, "comment.list", json!({}))["comments"].as_array().unwrap().iter().filter_map(|c| c["shape"].as_u64()).collect();
    assert_eq!(ties.len(), 2, "the comment and its reply are on the shape");
    assert!(run(&mut back, "comment.list", json!({"id": ties[0], "details": true}))["comments"][0]["shape_gone"] != true);
}

#[test]
fn comments_added_to_a_file_are_written_in_place_and_come_back_with_the_file() {
    let dir = Dir::new();
    let mut s = reviewed();
    run(&mut s, "file.save_as", json!({ "path": dir.path("file.odg") }));
    let mut again = reopened(&dir, "file.odg");
    run(&mut again, "comment.author", json!({"name": "Cy", "initials": "C"}));
    run(&mut again, "comment.add", json!({"text": "added in place"}));
    run(&mut again, "file.save", json!({}));
    let mut last = reopened(&dir, "file.odg");
    let texts: Vec<String> = said(&mut last).into_iter().map(|c| c.4).collect();
    assert_eq!(texts.len(), 4);
    assert!(texts.contains(&"added in place".to_owned()));
}

#[test]
fn replies_survive_a_vsdx_trip_and_a_diacraft_one() {
    let dir = Dir::new();
    for name in ["trip.vsdx", "trip.diacraft"] {
        let mut s = reviewed();
        let before = said(&mut s);
        run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
        let mut back = reopened(&dir, name);
        assert_eq!(said(&mut back), before, "{name}");
    }
}

#[test]
fn the_trip_across_the_three_formats_keeps_every_comment() {
    let dir = Dir::new();
    let mut s = reviewed();
    let before = said(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
    let mut a = reopened(&dir, "a.vsdx");
    run(&mut a, "file.save_as", json!({ "path": dir.path("b.odg") }));
    let mut b = reopened(&dir, "b.odg");
    run(&mut b, "file.save_as", json!({ "path": dir.path("c.diacraft") }));
    let mut c = reopened(&dir, "c.diacraft");
    assert_eq!(said(&mut c), before);
}

/// A VSDX file whose comments part is as a program that writes few attributes writes it: authors with no `Initials` (and one with an empty one), and a root with no `xml:space`.
fn bare_comments_vsdx() -> Vec<u8> {
    use diacraft_testkit::package::{NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder};
    let xml = format!(
        r#"<Comments xmlns="{NS_2012}" xmlns:r="{NS_R}"><AuthorList><AuthorEntry ID="1" Name="Ada"/><AuthorEntry ID="2" Name="Bob" Initials=""/><AuthorEntry ID="3" Name="Cy" Initials="C"/></AuthorList><CommentList><CommentEntry AuthorID="1" PageID="1" Date="2031-03-07T20:05:09Z" CommentID="1">plain</CommentEntry><CommentEntry AuthorID="2" PageID="1" Date="2031-03-08T10:00:00Z" CommentID="2">also plain</CommentEntry></CommentList></Comments>"#
    );
    let extra = Pkg::new().part("/visio/comments.xml", "application/vnd.ms-visio.comments+xml", &xml).rel(
        "/visio/document.xml",
        "rId91",
        &format!("{VIS}comments"),
        "comments.xml",
    );
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build()
}

#[test]
fn a_vsdx_comments_part_goes_through_diacraft_and_back_as_the_content_it_was() {
    // VSDX to `.diacraft` to VSDX: the content check of the files says the same (GATE.ea42 found two attributes added to a part that had none: `Initials` on each author without
    // one and `xml:space` on the root).
    let dir = Dir::new();
    let original = bare_comments_vsdx();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &original) }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("a.diacraft") }));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let same = diacraft_vsdx::compare_content(&original, &dir.read("b.vsdx"), &diacraft_vsdx::Limits::default()).unwrap();
    assert!(same.same(), "the comments part is as it was: {:?}", same.shown);
}

#[test]
fn what_an_odg_cannot_hold_of_a_comment_is_said_in_the_save() {
    let dir = Dir::new();
    let mut s = reviewed();
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.vsdx") }));
    let mut v = reopened(&dir, "v.vsdx");
    run(&mut v, "comment.markers", json!({"show": false}));
    let saved = run(&mut v, "file.save_as", json!({ "path": dir.path("v.odg") }));
    let words: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Json::as_str).collect();
    assert!(words.iter().any(|w| w.contains("the setting that hides the markers of the comments")), "{words:?}");
}
