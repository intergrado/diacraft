//! The AutoConnect setting of a drawing (View, Visual Aids, AutoConnect).
//!
//! It is the file's own box: on for a drawing that holds nothing (the program-wide setting is on by default), turned off by
//! `autoconnect.set`, no undo step (a View setting is "unsaved, not undo"), and kept by every save in DiaCraft's own data (a namespaced extension: no specification gives the box a place in a VSDX or
//! OpenDocument file), as `.diacraft`, as VSDX and as `.odg`.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::native_common::{Dir, PINNED};
use crate::odg_build::{Drawing, rect};
use crate::{check_session, run, snapshot};

fn new_drawing() -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.new", json!({}));
    s
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn on(s: &mut Session) -> bool {
    let v: Value = run(s, "autoconnect.settings", json!({}));
    v["on"].as_bool().unwrap_or_else(|| panic!("no `on` in {v}"))
}

#[test]
fn a_drawing_that_holds_nothing_has_autoconnect_on() {
    let mut s = new_drawing();
    let v = run(&mut s, "autoconnect.settings", json!({}));
    assert_eq!((v["on"].as_bool(), v["held"].is_null()), (Some(true), true), "{v}");
}

#[test]
fn turning_it_off_is_no_undo_step_and_what_the_drawing_shows_already_is_no_edit() {
    let mut s = new_drawing();
    let revision = s.revision();
    let v = run(&mut s, "autoconnect.set", json!({ "on": true }));
    assert_eq!(v["on"], true);
    assert_eq!(s.revision(), revision, "on already: nothing is written");
    assert!(!s.is_dirty() && s.history().unwrap().undo_len() == 0);
    let v = run(&mut s, "autoconnect.set", json!({ "on": false }));
    assert_eq!((v["on"].as_bool(), v["held"].as_bool()), (Some(false), Some(false)), "{v}");
    assert!(s.is_dirty(), "a box turned is an edit, as the other View boxes are");
    assert_eq!(s.history().unwrap().undo_len(), 0, "and no undo step");
    assert_eq!(s.history().unwrap().next_undo_label(), None);
    assert_eq!(s.revision(), revision, "no revision moves");
    check_session(&s).unwrap();
    let again = s.revision();
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    assert_eq!(s.revision(), again, "off already");
    assert!(s.execute("edit.undo", &json!({})).is_err(), "nothing to undo");
    assert!(!on(&mut s), "Ctrl+Z leaves the box where it was turned");
    assert!(s.is_dirty());
    // On again takes the data out: a drawing whose box is on holds nothing of it, so a file that never had it is the file it would have been.
    run(&mut s, "autoconnect.set", json!({ "on": true }));
    assert!(on(&mut s));
    assert!(s.document().unwrap().diacraft.is_none(), "nothing of DiaCraft's is held once the box is on again");
}

#[test]
fn the_box_is_refused_in_words_when_it_is_not_true_or_false() {
    let mut s = new_drawing();
    let before = snapshot(&s);
    for (params, words) in
        [(json!({}), "`on`"), (json!({ "on": "off" }), "`on` must be true or false"), (json!({ "on": 1 }), "`on` must be true or false")]
    {
        let e = s.execute("autoconnect.set", &params).unwrap_err().to_string();
        assert!(e.contains(words), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn the_setting_and_the_snap_switches_keep_to_their_own_data() {
    let mut s = new_drawing();
    run(&mut s, "snap.set", json!({ "snap": false }));
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    assert!(!on(&mut s));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["snap"], false);
    // On again leaves the snap switch's data alone.
    run(&mut s, "autoconnect.set", json!({ "on": true }));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["snap"], false);
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    run(&mut s, "snap.set", json!({ "snap": true }));
    assert!(!on(&mut s), "a snap switch put back does not take AutoConnect's data with it");
}

#[test]
fn off_is_off_again_after_a_diacraft_save_and_a_vsdx_save() {
    let dir = Dir::new();
    let mut s = new_drawing();
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    let native = dir.path("off.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    assert!(!on(&mut opened(&native)), ".diacraft");
    let model = dir.path("off.vsdx");
    run(&mut s, "file.save_as", json!({ "path": model }));
    let mut back = opened(&model);
    assert!(!on(&mut back), "VSDX from the model");
    // Reopened and saved on to `.diacraft`, it stays off; turned on and saved again it is on.
    let again = dir.path("again.diacraft");
    run(&mut back, "file.save_as", json!({ "path": again }));
    assert!(!on(&mut opened(&again)), "VSDX to .diacraft");
    run(&mut back, "autoconnect.set", json!({ "on": true }));
    let third = dir.path("third.vsdx");
    run(&mut back, "file.save_as", json!({ "path": third }));
    assert!(on(&mut opened(&third)));
}

#[test]
fn a_vsdx_file_saved_into_itself_keeps_the_box_turned() {
    let dir = Dir::new();
    let path = dir.write("own.vsdx", &crate::native_common::vsdx_drawing());
    let mut s = opened(&path);
    assert!(on(&mut s), "a file that holds nothing shows it on");
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    run(&mut s, "file.save", json!({}));
    assert!(!on(&mut opened(&path)));
    // Turned on again and saved, the data goes from the file.
    let mut again = opened(&path);
    run(&mut again, "autoconnect.set", json!({ "on": true }));
    run(&mut again, "file.save", json!({}));
    assert!(on(&mut opened(&path)));
}

#[test]
fn an_odg_keeps_it_in_diacrafts_document_file() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).bytes());
    let mut s = opened(&path);
    assert!(on(&mut s));
    run(&mut s, "autoconnect.set", json!({ "on": false }));
    run(&mut s, "file.save", json!({}));
    assert!(!on(&mut opened(&path)), ".odg saved into itself");
    let native = dir.path("from-odg.diacraft");
    run(&mut opened(&path), "file.save_as", json!({ "path": native }));
    assert!(!on(&mut opened(&native)), ".odg to .diacraft");
}
