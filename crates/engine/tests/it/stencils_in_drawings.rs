//! The built-in stencils a drawing had open come back when it is opened (issue 7 of the demo video): a drawing started from the Basic Network Diagram
//! template and saved reopened with the default stencils, and `window.stencil` found no `network`. The list rides in DiaCraft's data (`template::STENCILS_CHILD`), which `.diacraft`, `.vsdx` and
//! `.odg` all carry; a file with no list opens the stencils its masters came from.
//!
//! Every package is made by the engine's own writer; no file from another application is used (AGENTS.md).

use std::sync::Arc;

use diacraft_doc::{DIACRAFT_URN, Document, RawAttrs, RawElement, RawNode};
use diacraft_engine::Session;
use diacraft_engine::template::{self, STENCILS_CHILD};
use diacraft_vsdx::{Kind, Variant, WriteOptions, write_with};
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::native_common::Dir;
use crate::{blank_session, check_session, run};

const DEFAULTS: [&str; 7] = ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"];

/// The ids of the built-in stencils that are open, in the order the Shapes window lists them (the stencil files, `file-1` and the like, are left out).
fn open_ids(s: &mut Session) -> Vec<String> {
    let listed = run(s, "stencil.list", json!({}));
    listed["stencils"].as_array().unwrap().iter().filter(|st| st["builtin"] == true).map(|st| st["id"].as_str().unwrap().to_owned()).collect()
}

fn started_from_network() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "basic_network" }));
    run(&mut s, "shape.drop", json!({ "master": "network.router", "page": 1, "x": 3.0, "y": 4.0 }));
    s
}

/// The bytes of a drawing (a `.vsdx`) whose DiaCraft data holds `list` as the `open` attribute of the `stencils` child, put there as it is: a file somebody sent could say anything.
fn vsdx_with_list(list: &str) -> Vec<u8> {
    let mut document = Document::new_blank();
    assert!(document.set_diacraft_child(STENCILS_CHILD, Some(&[("open", list)])));
    let options = WriteOptions { variant: Some(Variant { kind: Kind::Drawing, macro_enabled: false }), ..WriteOptions::default() };
    write_with(&document, None, &options).unwrap().bytes
}

#[test]
fn a_drawing_started_from_the_network_template_reopens_with_the_stencils_it_had_open_in_every_format() {
    // The demo's steps: `file.new {template: "basic_network"}`, drop shapes, `file.save_as`, `file.new`, `file.open`, then `window.stencil {stencil: "network"}`.
    for extension in ["diacraft", "vsdx", "odg"] {
        let dir = Dir::new();
        let mut s = started_from_network();
        assert_eq!(open_ids(&mut s), ["network", "computers", "basic"], "{extension}: the template's stencils");
        let path = dir.path(&format!("office-network.{extension}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        run(&mut s, "file.new", json!({}));
        assert_eq!(open_ids(&mut s), DEFAULTS, "{extension}: a new drawing has the default stencils");
        let opened = run(&mut s, "file.open", json!({ "path": path }));
        assert_eq!(open_ids(&mut s), ["network", "computers", "basic"], "{extension}: the stencils the drawing had open are open again: {opened}");
        assert_eq!(
            opened["stencils"],
            json!({ "open": ["network", "computers", "basic"], "ignored": 0 }),
            "{extension}: the answer says what was opened"
        );
        assert_eq!(opened["file"]["warning_count"], 0, "{extension}: {opened}");
        check_session(&s).unwrap();
    }
}

#[test]
fn the_stencils_opened_by_hand_are_saved_with_the_drawing_and_open_again_in_a_diacraft_file_and_a_vsdx_file() {
    for extension in ["diacraft", "vsdx"] {
        let dir = Dir::new();
        let mut s = blank_session();
        run(&mut s, "stencil.open", json!({ "id": "computers" }));
        run(&mut s, "stencil.open", json!({ "id": "network" }));
        drop_shape(&mut s, "rectangle", 3.0, 3.0);
        let mut want: Vec<&str> = DEFAULTS.to_vec();
        want.extend(["computers", "network"]);
        assert_eq!(open_ids(&mut s), want);
        let path = dir.path(&format!("drawing.{extension}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        assert!(!s.is_dirty(), "{extension}: a save of the stencil list is no edit");
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": path }));
        assert_eq!(open_ids(&mut t), want, "{extension}: the stencils open when it was saved, in their order");
        // A save into the file keeps the list up to date: another stencil opened, saved, opened again.
        run(&mut t, "stencil.open", json!({ "id": "bpmn" }));
        run(&mut t, "file.save", json!({}));
        let mut u = Session::new();
        run(&mut u, "file.open", json!({ "path": path }));
        want.push("bpmn");
        assert_eq!(open_ids(&mut u), want, "{extension}: a Save into the file records the stencils open then");
        check_session(&u).unwrap();
    }
}

#[test]
fn a_drawing_that_is_saved_with_the_stencils_it_opened_with_has_no_list_and_an_unedited_save_changes_no_byte() {
    for extension in ["diacraft", "vsdx"] {
        let dir = Dir::new();
        let mut s = blank_session();
        drop_shape(&mut s, "rectangle", 3.0, 3.0);
        let path = dir.path(&format!("plain.{extension}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        assert!(template::stencil_names(doc(&s)).is_none(), "{extension}: the default stencils say nothing");
        let first = dir.read(&format!("plain.{extension}"));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": path }));
        assert_eq!(open_ids(&mut t), DEFAULTS);
        run(&mut t, "file.save", json!({}));
        assert_eq!(
            dir.read(&format!("plain.{extension}")),
            first,
            "{extension}: an unedited save of a drawing whose stencils did not change writes the same file"
        );
        assert!(template::stencil_names(doc(&t)).is_none());
    }
}

#[test]
fn a_file_that_names_no_stencil_opens_the_stencils_its_masters_came_from() {
    // A shape dropped by its master's id needs no stencil open: the drawing holds the master, whose fixed identity says which stencil it came from.
    for extension in ["diacraft", "vsdx", "odg"] {
        let dir = Dir::new();
        let mut s = blank_session();
        run(&mut s, "shape.drop", json!({ "master": "network.router", "page": 1, "x": 3.0, "y": 4.0 }));
        assert_eq!(open_ids(&mut s), DEFAULTS, "{extension}: the stencil was not opened, only a master of it dropped");
        let path = dir.path(&format!("by-masters.{extension}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        assert!(template::stencil_names(doc(&s)).is_none(), "{extension}: nothing was recorded, the masters say it");
        let mut t = Session::new();
        let opened = run(&mut t, "file.open", json!({ "path": path }));
        let mut want: Vec<&str> = DEFAULTS.to_vec();
        want.push("network");
        assert_eq!(open_ids(&mut t), want, "{extension}: the stencil the master came from is open: {opened}");
        // Saved again as it opened, the file is the file it was (no list is written for what the masters say).
        if extension != "odg" {
            let before = dir.read(&format!("by-masters.{extension}"));
            run(&mut t, "file.save", json!({}));
            assert_eq!(dir.read(&format!("by-masters.{extension}")), before, "{extension}: an unedited save writes the same file");
        }
    }
}

#[test]
fn a_file_that_names_ten_thousand_stencils_or_names_that_are_not_built_in_opens_what_it_can_and_says_what_it_left() {
    let dir = Dir::new();
    // Ten thousand names: only the first 64 are looked at, and the words say so.
    let many = "network;".repeat(10_000);
    let path = dir.write("many.vsdx", &vsdx_with_list(&many));
    let mut s = Session::new();
    let opened = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(open_ids(&mut s), ["network"]);
    let words: Vec<&str> = opened["file"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("only the first 64")), "the cap is told in words: {words:?}");
    assert_eq!(opened["file"]["warning_count"].as_u64(), Some(words.len() as u64), "{opened}");
    // Names that are not the id of a built-in stencil: a path, a stencil file's id, nothing. None is followed, and the file is told it was not.
    let path = dir.write("unknown.vsdx", &vsdx_with_list("/etc/passwd;C:\\Stencils\\Mine.vssx;file-1;nope"));
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({ "path": path }));
    assert_eq!(open_ids(&mut t), DEFAULTS, "names it cannot open: the stencils a new drawing has");
    assert_eq!(opened["stencils"], json!({ "open": DEFAULTS, "ignored": 4 }), "{opened}");
    let words: Vec<&str> = opened["file"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("never followed") && w.contains("in the file")), "{words:?}");
    assert!(t.document().is_some(), "the drawing is open all the same");
    // The same through a `.diacraft` file: saved from the drawing that opened, and opened again.
    let native = dir.path("many.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({ "path": native }));
    assert_eq!(open_ids(&mut u), ["network"]);
    check_session(&u).unwrap();
}

#[test]
fn data_of_a_later_version_is_not_read_for_stencils_and_a_save_says_it_did_not_record_them() {
    // The file's DiaCraft data is of a version this build does not read: it is kept as it is, no list is read from it, and the stencils opened since are told not to be recorded.
    let child = RawElement { name: "dc:stencils".into(), attrs: RawAttrs::from_pairs([("open", "network")]), children: Vec::new() };
    let data = RawElement {
        name: "dc:Data".into(),
        attrs: RawAttrs::from_pairs([("xmlns:dc".to_owned(), DIACRAFT_URN.to_owned()), ("dc:version".to_owned(), "99".to_owned())]),
        children: vec![RawNode::Element(child)],
    };
    let mut document = Document::new_blank();
    document.diacraft = Some(Arc::new(data));
    let options = WriteOptions { variant: Some(Variant { kind: Kind::Drawing, macro_enabled: false }), ..WriteOptions::default() };
    let bytes = write_with(&document, None, &options).unwrap().bytes;
    let dir = Dir::new();
    let path = dir.write("later.vsdx", &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(open_ids(&mut s), DEFAULTS, "a later build's list is not read");
    run(&mut s, "stencil.open", json!({ "id": "computers" }));
    let saved = run(&mut s, "file.save", json!({}));
    let words: Vec<&str> = saved["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("not recorded") && w.contains("version this build does not read")), "{words:?}");
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": path }));
    assert_eq!(open_ids(&mut t), DEFAULTS, "the file keeps the data it had, and no list was put in it");
}
