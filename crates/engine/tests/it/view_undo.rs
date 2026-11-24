//! Which extends the owner's of 2026-11-22, "Unsaved, not undo": the settings of View > Visual Aids that are the document's follow the
//! rule the window settings follow (`window_settings.rs`).
//!
//! 1. `autoconnect.set` and `snap.set` (the switches, the options of the words of `DocumentSettings`, Dynamic Grid and the snap strengths) make **no undo step** and make the
//!    drawing **unsaved**. A value the drawing holds already changes nothing.
//! 2. Ctrl+Z after a toggle and a move undoes only the move: undo and redo **carry the settings across**, in the data of DiaCraft's own and in the words of `DocumentSettings`,
//!    for a drawing that held them and for one that held none.
//! 3. A toggle, a save and a reopen keeps the value, as `.diacraft`, as VSDX (from the model and spliced into a file) and as `.odg`; a second save is no edit.
//! 4. What is not a View setting stays an undo step: a guide, a connector's line jumps (a cell of its page) and a shape's own edit.
//!
//! Every file is built in code.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::native_common::{Dir, PINNED, vsdx_drawing};
use crate::odg_build::{Drawing, rect};
use crate::{check_session, run};

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

/// A drawing opened from a VSDX file that holds no `DocumentSettings`, no data of DiaCraft's and no windows part: a setting turned makes each of them.
fn from_a_file(dir: &Dir) -> Session {
    opened(&dir.write("from.vsdx", &vsdx_drawing()))
}

/// What a drawing holds of these settings: AutoConnect's box and everything `snap.settings` says (the options, the strengths and the words they are held in).
fn held(s: &mut Session) -> Value {
    json!({ "autoconnect": run(s, "autoconnect.settings", json!({})), "snap": run(s, "snap.settings", json!({})) })
}

fn undo_steps(s: &Session) -> usize {
    s.history().unwrap().undo_len()
}

fn status_dirty(s: &mut Session) -> bool {
    run(s, "file.status", json!({}))["dirty"].as_bool().unwrap()
}

/// Drops a rectangle on the first page and gives its id.
fn drop_shape(s: &mut Session) -> u32 {
    let r = run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}));
    u32::try_from(r["id"].as_u64().unwrap()).unwrap()
}

/// A shape's pin on the first page, `None` when the shape is not there.
fn pin(s: &Session, id: u32) -> Option<(f64, f64)> {
    diacraft_engine::draw::shape_frame(s.document().unwrap(), 1, id).map(|f| (f.pin_x, f.pin_y))
}

/// The settings under test: a command, what turns one thing, and what turns it back.
struct Case {
    name: &'static str,
    command: &'static str,
    turn: Value,
    back: Value,
}

fn cases() -> Vec<Case> {
    let c = |name, command, turn, back| Case { name, command, turn, back };
    vec![
        c("AutoConnect", "autoconnect.set", json!({"on": false}), json!({"on": true})),
        c("the Snap switch (DiaCraft's data)", "snap.set", json!({"snap": false}), json!({"snap": true})),
        c("the Glue switch (DiaCraft's data)", "snap.set", json!({"glue": false}), json!({"glue": true})),
        c("Snap to grid (a flag of SnapSettings)", "snap.set", json!({"grid": false}), json!({"grid": true})),
        c("Dynamic Grid (DynamicGridEnabled)", "snap.set", json!({"dynamic_grid": false}), json!({"dynamic_grid": true})),
        c("Glue to handles (a flag of GlueSettings)", "snap.set", json!({"glue_handles": true}), json!({"glue_handles": false})),
        c("a snap extension (a flag of SnapExtensions)", "snap.set", json!({"ext_tangents": true}), json!({"ext_tangents": false})),
        c("a snap strength (DiaCraft's data)", "snap.set", json!({"strength_grid": 20}), json!({"strength_grid": 5})),
        c(
            "several at once",
            "snap.set",
            json!({"snap": false, "dynamic_grid": false, "alignment_box": true, "strength_guides": 3}),
            json!({"snap": true, "dynamic_grid": true, "alignment_box": false, "strength_guides": 10}),
        ),
    ]
}

/// Item 1 of the brief: each toggle makes no undo step and marks the drawing unsaved; a value the drawing holds already is no edit. `autoconnect.set` moves no revision (the page and
/// its reads are not made of it), and `snap.set` moves the revision once (a Snap & Glue option decides which targets the snap reads, so the reads kept for the old one are not found).
#[test]
fn a_toggle_makes_no_undo_step_and_marks_the_drawing_unsaved() {
    for case in cases() {
        let mut s = new_drawing();
        let revision = s.revision();
        let kept = s.snap_page(1).unwrap();
        assert!(s.kept_snap_page(1).is_some(), "{}: the page is read for this revision", case.name);
        assert!(!s.is_dirty() && !status_dirty(&mut s), "{}: a new drawing", case.name);
        let before = held(&mut s);
        run(&mut s, case.command, case.turn.clone());
        assert_ne!(held(&mut s), before, "{}: the sample changed nothing", case.name);
        assert_eq!(undo_steps(&s), 0, "{}: no undo step", case.name);
        assert_eq!(s.history().unwrap().next_undo_label(), None, "{}", case.name);
        assert!(s.is_dirty() && status_dirty(&mut s), "{}: the drawing is unsaved", case.name);
        if case.command == "snap.set" {
            assert_eq!(s.revision(), revision + 1, "{}: the snap's reads are of another revision", case.name);
            assert!(s.kept_snap_page(1).is_none(), "{}: the read kept for the old options is not found", case.name);
        } else {
            assert_eq!(s.revision(), revision, "{}: no revision moves", case.name);
            assert!(std::sync::Arc::ptr_eq(&s.kept_snap_page(1).unwrap(), &kept), "{}: the page's read is still the kept one", case.name);
        }
        check_session(&s).unwrap();
        // The same value again changes nothing: no revision, and the same document (nothing copied).
        let (again, doc) = (s.revision(), s.document().cloned());
        run(&mut s, case.command, case.turn.clone());
        assert_eq!(s.revision(), again, "{}: as it is already", case.name);
        assert_eq!(s.document().cloned(), doc, "{}", case.name);
        // Ctrl+Z has nothing to take back, and the setting stays.
        let refused = s.execute("edit.undo", &json!({})).unwrap_err().to_string();
        assert!(refused.contains("nothing to undo"), "{}: {refused}", case.name);
        assert_ne!(held(&mut s), before, "{}: undo past the start leaves the setting", case.name);
        assert!(s.is_dirty(), "{}", case.name);
    }
}

/// Item 1: a value the drawing shows already is no edit at all, for each command: no document change, no step, not unsaved.
#[test]
fn a_value_held_already_is_no_edit() {
    let mut s = new_drawing();
    let revision = s.revision();
    run(&mut s, "autoconnect.set", json!({"on": true}));
    run(&mut s, "snap.set", json!({"snap": true, "grid": true, "dynamic_grid": true, "strength_grid": 5}));
    run(&mut s, "snap.set", json!({}));
    assert_eq!((s.revision(), undo_steps(&s), s.is_dirty()), (revision, 0, false));
}

/// Item 2: Ctrl+Z after a toggle and a move undoes only the move; at the start of the history the setting is as it was turned; redo brings the shape back and leaves the setting;
/// a setting turned between an undo and its redo keeps the redo and is not taken back by it. For each setting, in a new drawing and in one from a file that held none of them.
#[test]
fn undo_after_a_toggle_and_a_move_undoes_only_the_move() {
    let dir = Dir::new();
    for (kind, case) in ["new drawing", "file"].into_iter().flat_map(|kind| cases().into_iter().map(move |case| (kind, case))) {
        let (name, command, turn, back) = (case.name, case.command, case.turn, case.back);
        let mut s = if kind == "new drawing" { new_drawing() } else { from_a_file(&dir) };
        let id = drop_shape(&mut s);
        let dropped = pin(&s, id).unwrap();
        let base = undo_steps(&s);
        let original = held(&mut s);
        run(&mut s, command, turn.clone());
        let turned = held(&mut s);
        assert_ne!(turned, original, "{name} ({kind}): the sample changed nothing");
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        let moved = pin(&s, id).unwrap();
        assert!(moved.0 > dropped.0 + 0.5);
        assert_eq!(undo_steps(&s), base + 1, "{name} ({kind}): the move is a step and the setting is none");
        // Undo takes the move, and not the setting.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(pin(&s, id), Some(dropped), "{name} ({kind}): the move is undone");
        assert_eq!(held(&mut s), turned, "{name} ({kind}): and the setting stays");
        // Undo again takes the drop; the setting stays.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(pin(&s, id), None, "{name} ({kind}): the drop is undone");
        assert_eq!(held(&mut s), turned, "{name} ({kind})");
        assert!(s.execute("edit.undo", &json!({})).is_err());
        assert_eq!(held(&mut s), turned, "{name} ({kind}): at the start of the history");
        assert!(s.is_dirty(), "{name} ({kind}): the setting turned is a change of the drawing");
        // Redo brings the shape back, and leaves the setting.
        run(&mut s, "edit.redo", json!({}));
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(pin(&s, id), Some(moved), "{name} ({kind})");
        assert_eq!(held(&mut s), turned, "{name} ({kind}): redo leaves the setting");
        // A setting turned between an undo and its redo: the redo stays, and does not take the setting back to what it was when the move was undone.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.history().unwrap().redo_len(), 1);
        run(&mut s, command, back.clone());
        let turned_back = held(&mut s);
        assert_ne!(turned_back, turned, "{name} ({kind})");
        assert_eq!(s.history().unwrap().redo_len(), 1, "{name} ({kind}): a setting turned does not end the redo steps");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(pin(&s, id), Some(moved), "{name} ({kind})");
        assert_eq!(held(&mut s), turned_back, "{name} ({kind}): the redo leaves what was set last");
        check_session(&s).unwrap();
    }
}

/// The unsaved mark follows the settings as it does the edits: a setting turned is changed until it is saved; a save is clean; an edit undone back to the saved state is clean only
/// when the settings are the saved ones too.
#[test]
fn a_setting_turned_marks_the_drawing_unsaved_until_it_is_saved() {
    for case in cases() {
        let dir = Dir::new();
        let mut s = new_drawing();
        let id = drop_shape(&mut s);
        run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        assert!(!s.is_dirty(), "{}", case.name);
        // An edit and its undo are clean again: the settings are the saved ones.
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        assert!(s.is_dirty());
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.is_dirty(), "{}: the undo of an edit made since the save, with no setting turned, is the saved drawing", case.name);
        // A setting turned: changed, with no step.
        run(&mut s, case.command, case.turn.clone());
        assert!(s.is_dirty() && status_dirty(&mut s), "{}", case.name);
        assert_eq!(undo_steps(&s), 1, "{}: the drop only", case.name);
        // An edit, and the edit's undo: the drawing is not the saved one, for the setting.
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.is_dirty(), "{}: the setting is turned since the save", case.name);
        // A save is the drawing as it is, and clean; an edit and its undo after it are clean again, the saved setting kept.
        let turned = held(&mut s);
        run(&mut s, "file.save", json!({}));
        assert!(!s.is_dirty() && !status_dirty(&mut s), "{}", case.name);
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.is_dirty(), "{}: and an edit undone after it is clean again", case.name);
        assert_eq!(held(&mut s), turned, "{}", case.name);
        check_session(&s).unwrap();
    }
}

/// A setting turned inside a batch is no step of it: a batch of the setting alone records no step; a batch of the setting and an edit records the one step, whose undo takes the
/// edit and leaves the setting; a batch that fails takes the setting back with the rest.
#[test]
fn a_setting_turned_in_a_batch_is_no_step_of_the_batch() {
    let mut s = new_drawing();
    let only = json!({"commands": [
        {"command": "autoconnect.set", "params": {"on": false}},
        {"command": "snap.set", "params": {"dynamic_grid": false}},
    ]});
    run(&mut s, "command.batch", only);
    assert_eq!(undo_steps(&s), 0, "a batch that turned settings and made no edit has no step");
    assert!(s.is_dirty());
    let after_only = held(&mut s);
    // The settings and an edit: one step, which is the edit's.
    let both = json!({"commands": [
        {"command": "snap.set", "params": {"grid": false}},
        {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}},
    ], "label": "Both"});
    let made = run(&mut s, "command.batch", both);
    let id = u32::try_from(made["results"][1]["id"].as_u64().unwrap()).unwrap();
    assert_eq!(undo_steps(&s), 1);
    let after_both = held(&mut s);
    assert_ne!(after_both, after_only);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(pin(&s, id), None, "the batch's edit is undone");
    assert_eq!(held(&mut s), after_both, "and the settings stay");
    // A batch that fails changes nothing, the setting it turned included.
    let failing = json!({"commands": [
        {"command": "snap.set", "params": {"snap": false, "strength_grid": 40}},
        {"command": "autoconnect.set", "params": {"on": true}},
        {"command": "page.delete", "params": {"id": 99}},
    ]});
    assert!(s.execute("command.batch", &failing).is_err());
    assert_eq!(held(&mut s), after_both, "the settings were not turned");
    check_session(&s).unwrap();
}

/// What stays an undo step: a guide (a page shape), a connector's line jumps (a cell of its page) and a shape's own edit, each one step that takes back exactly itself and leaves a
/// setting turned between them alone.
#[test]
fn guides_page_cells_and_shape_edits_stay_undo_steps() {
    let mut s = new_drawing();
    let id = drop_shape(&mut s);
    run(&mut s, "snap.set", json!({"dynamic_grid": false}));
    run(&mut s, "autoconnect.set", json!({"on": false}));
    let turned = held(&mut s);
    assert_eq!(undo_steps(&s), 1, "the drop");
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.0}));
    assert_eq!(undo_steps(&s), 2, "a guide is one step");
    run(&mut s, "connector.line_jumps", json!({"page": 1, "code": 1}));
    assert_eq!(undo_steps(&s), 3, "the line jumps are one step");
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    assert_eq!(undo_steps(&s), 4, "a move is one step");
    let guides = |s: &mut Session| run(s, "guide.list", json!({"page": 1}))["guides"].as_array().unwrap().len();
    assert_eq!(guides(&mut s), 1);
    for expected in [3, 2, 1] {
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(undo_steps(&s), expected);
        assert_eq!(held(&mut s), turned, "the settings stay through the undo of a step");
    }
    assert_eq!(guides(&mut s), 0, "the guide went with its step");
    assert!(s.is_dirty());
}

/// Item 3: a toggle, a save and a reopen keeps the value: `.diacraft`, VSDX written from the model, and VSDX and `.odg` saved into their own files. Also with an edit and its undo
/// between the toggle and the save (the undo goes back to a document made before the toggle). A second save is no edit.
#[test]
fn a_toggle_then_a_save_and_a_reopen_keeps_the_value_in_every_format() {
    for case in cases() {
        let dir = Dir::new();
        let name = case.name;
        // A new drawing: an edit, the toggle, the edit's undo, then each save.
        let mut s = new_drawing();
        let id = drop_shape(&mut s);
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        run(&mut s, case.command, case.turn.clone());
        run(&mut s, "edit.undo", json!({}));
        let want = held(&mut s);
        let native = dir.path("a.diacraft");
        run(&mut s, "file.save_as", json!({ "path": native }));
        assert_eq!(held(&mut opened(&native)), want, "{name}: .diacraft");
        let first = std::fs::read(&native).unwrap();
        run(&mut s, "file.save", json!({}));
        assert_eq!(std::fs::read(&native).unwrap(), first, "{name}: a second .diacraft save is no edit");
        let model = dir.path("a.vsdx");
        run(&mut s, "file.save_as", json!({ "path": model }));
        assert_eq!(held(&mut opened(&model)), want, "{name}: VSDX from the model");
        // A VSDX file saved into itself: the toggle, an edit and its undo, then a save.
        let path = dir.write("own.vsdx", &vsdx_drawing());
        let mut v = opened(&path);
        let original = held(&mut v);
        run(&mut v, "page.add", json!({ "name": "Third" }));
        run(&mut v, case.command, case.turn.clone());
        run(&mut v, "edit.undo", json!({}));
        let turned = held(&mut v);
        assert_ne!(turned, original, "{name}: a file");
        run(&mut v, "file.save", json!({}));
        assert_eq!(held(&mut opened(&path)), turned, "{name}: VSDX into the file");
        let once = std::fs::read(&path).unwrap();
        let saved = run(&mut v, "file.save", json!({}));
        assert_eq!((saved["parts"]["rewritten"].as_u64(), saved["parts"]["added"].as_u64()), (Some(0), Some(0)), "{name}: {saved}");
        assert_eq!(std::fs::read(&path).unwrap(), once, "{name}: a second VSDX save is no edit");
        // An `.odg` saved into itself.
        let odg = dir.write("a.odg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).bytes());
        let mut o = opened(&odg);
        let original = held(&mut o);
        run(&mut o, case.command, case.turn.clone());
        let turned = held(&mut o);
        assert_ne!(turned, original, "{name}: an .odg");
        assert_eq!(run(&mut o, "file.save", json!({}))["edited"], true, "{name}");
        assert_eq!(held(&mut opened(&odg)), turned, "{name}: .odg saved into itself");
        let odg_once = std::fs::read(&odg).unwrap();
        assert_eq!(run(&mut o, "file.save", json!({}))["edited"], false, "{name}: a second .odg save is no edit");
        assert_eq!(std::fs::read(&odg).unwrap(), odg_once, "{name}");
        assert_eq!(run(&mut opened(&odg), "file.save", json!({}))["edited"], false, "{name}: an .odg reopened");
        // The three formats into one another: the VSDX file's setting goes on to `.diacraft`.
        let across = dir.path("across.diacraft");
        run(&mut opened(&path), "file.save_as", json!({ "path": across }));
        assert_eq!(held(&mut opened(&across)), held(&mut opened(&path)), "{name}: VSDX to .diacraft");
    }
}

/// Item 3: the data goes out of a file when the setting is back where a drawing that held none has it: a drawing whose AutoConnect box is on holds nothing of it, and a drawing
/// whose Snap & Glue options are all the defaults holds no `snap` child of DiaCraft's data.
#[test]
fn a_setting_turned_back_takes_its_data_out_and_stays_unsaved_until_a_save() {
    let mut s = new_drawing();
    run(&mut s, "autoconnect.set", json!({"on": false}));
    run(&mut s, "snap.set", json!({"snap": false, "strength_grid": 20}));
    assert!(s.document().unwrap().diacraft.is_some());
    run(&mut s, "autoconnect.set", json!({"on": true}));
    run(&mut s, "snap.set", json!({"snap": true, "strength_grid": 5}));
    assert!(s.document().unwrap().diacraft.is_none(), "nothing of DiaCraft's is held once every setting is back");
    assert!(s.is_dirty(), "turned and turned back is still a change until it is saved");
    assert_eq!(undo_steps(&s), 0);
}
