//! The window settings (the grid, the rulers, the guides and the connection points) are the drawing's, held in the model as the VSDX windows
//! part holds them, set by `window.settings.set` and kept by every save.
//!
//! 1. A drawing of any format with no windows part shows a new drawing's settings: the grid, the rulers and the guides on, the connection points off.
//! 2. A save writes the settings the drawing holds: a drawing whose grid was turned off reopens with it off, as `.diacraft`, as VSDX (from the model, and spliced into a file
//!    that has a windows part or none) and as `.odg` (in DiaCraft's document file).
//! 3. A drawing whose settings did not change writes its windows part as it read it: a second save is no edit, and a splice save keeps the bytes.
//! 4. The owner's ruling, 2026-11-22, "Unsaved, not undo": turning a setting marks the drawing unsaved and is no undo step; undo and redo leave the settings
//!    alone, and no revision moves for one.
//!
//! Every file is built in code.

use std::collections::BTreeMap;

use diacraft_engine::Session;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use serde_json::{Value, json};

use crate::native_common::{Dir, PINNED};
use crate::odg_build::{Drawing, rect};
use crate::{check_session, run};

/// The relationship type of a windows part.
const WINDOWS_REL: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
/// The content type of a windows part.
const WINDOWS_TYPE: &str = "application/vnd.ms-visio.windows+xml";

/// A windows part as a file might hold one: a stencil window first, attributes the model has no field for, blanks, and a drawing window with the grid and the
/// rulers on and `ShowGuides` as an attribute.
const WINDOWS_PART: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Windows ClientWidth=\"1680\" ClientHeight=\"947\" xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\">\r\n  <Window ID=\"1\" WindowType=\"Stencil\" ParentWindow=\"0\" Document=\"C:\\x\\BASIC_M.vssx\"/>\r\n  <Window ID=\"0\" WindowType=\"Drawing\" WindowState=\"1073741824\" ContainerType=\"Page\" Page=\"1\" ViewScale=\"-1\" ShowGuides=\"1\">\r\n    <ShowRulers>1</ShowRulers>\r\n    <ShowGrid>1</ShowGrid>\r\n    <SnapSettings>295</SnapSettings>\r\n  </Window>\r\n</Windows>";

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

/// A VSDX drawing of two pages, with `windows` as its windows part (and the document's relationship to it), or none.
fn vsdx(windows: Option<&str>) -> Vec<u8> {
    let builder =
        VsdxBuilder::new().page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#))).page(PageSpec::new(2, "Second"));
    match windows {
        Some(xml) => {
            builder.with(Pkg::new().part("/visio/windows.xml", WINDOWS_TYPE, xml).rel("/visio/document.xml", "rId90", WINDOWS_REL, "windows.xml"))
        }
        None => builder,
    }
    .build()
}

/// `window.settings` of a session.
fn settings(s: &mut Session) -> Value {
    run(s, "window.settings", json!({}))
}

/// What a drawing shows, `(rulers, grid, guides, connection_points)`.
fn shown(s: &mut Session) -> (bool, bool, bool, bool) {
    let v = settings(s);
    let b = |k: &str| v[k].as_bool().unwrap_or_else(|| panic!("{k} in {v}"));
    (b("rulers"), b("grid"), b("guides"), b("connection_points"))
}

/// A new drawing's settings, as `shown` gives them.
const NEW: (bool, bool, bool, bool) = (true, true, true, false);

/// A session with the clock pinned that has opened `path`.
fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn new_drawing() -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.new", json!({}));
    s
}

/// Test 1 of the brief: a new drawing saved as `.diacraft` reopens with the grid and the rulers on (it showed them before the save, and holds no windows part).
#[test]
fn a_new_drawing_saved_as_diacraft_reopens_with_the_grid_and_the_rulers_on() {
    let dir = Dir::new();
    let mut s = new_drawing();
    assert_eq!(shown(&mut s), NEW);
    assert!(settings(&mut s)["held"].is_null(), "a new drawing holds no windows part");
    let path = dir.path("new.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut again = opened(&path);
    assert_eq!(shown(&mut again), NEW, "the grid, the rulers and the guides on, the connection points off");
}

/// No windows part, of any format, is a new drawing's settings. A VSDX file with none and an `.odg` (which has none) open as a new drawing shows.
#[test]
fn a_drawing_of_any_format_with_no_windows_part_shows_a_new_drawings_settings() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("none.vsdx", &vsdx(None)));
    assert_eq!(shown(&mut s), NEW, "a VSDX file with no windows part");
    let mut o = opened(&dir.write("a.odg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).bytes()));
    assert_eq!(shown(&mut o), NEW, "an .odg");
    // A part whose drawing window holds no value is no new drawing: the grid and the rulers off, the guides on, the points off.
    let mut empty = opened(&dir.write("empty.vsdx", &vsdx(Some(r#"<Windows><Window ID="0" WindowType="Drawing"/></Windows>"#))));
    assert_eq!(shown(&mut empty), (false, false, true, false));
    let mut file = opened(&dir.write("own.vsdx", &vsdx(Some(WINDOWS_PART))));
    assert_eq!(shown(&mut file), (true, true, true, false), "the file's own: the guides as an attribute");
}

/// How many undo steps a session holds.
fn undo_steps(s: &Session) -> usize {
    s.history().unwrap().undo_len()
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

/// What `file.status` says of the drawing's unsaved changes.
fn status_dirty(s: &mut Session) -> bool {
    run(s, "file.status", json!({}))["dirty"].as_bool().unwrap()
}

/// The owner's ruling, 2026-11-22: "Unsaved, not undo": turning a setting marks the drawing changed (`file.status`'s `dirty`, so a save keeps it and a close asks)
/// and is **no undo step**: no step is recorded, the redo steps stay, and no revision moves (the picture of the page, the snap reads and the rest are of the revision, and the
/// grid is no part of them). A value the drawing shows already writes nothing at all.
#[test]
fn turning_a_setting_is_no_undo_step_and_a_value_shown_already_is_no_edit() {
    let mut s = new_drawing();
    let revision = s.revision();
    let answer = run(&mut s, "window.settings.set", json!({ "grid": true, "rulers": true, "guides": true, "connection_points": false }));
    assert_eq!((answer["grid"].as_bool(), answer["held"].is_null()), (Some(true), true));
    assert_eq!(s.revision(), revision, "what a new drawing shows already: nothing is written");
    assert!(!s.is_dirty() && undo_steps(&s) == 0);
    let answer = run(&mut s, "window.settings.set", json!({ "grid": false }));
    assert_eq!((answer["grid"].as_bool(), answer["held"]["grid"].as_bool(), answer["rulers"].as_bool()), (Some(false), Some(false), Some(true)));
    assert_eq!(shown(&mut s), (true, false, true, false));
    // The drawing is changed: the title's mark, the close prompt and `file.status` all ask `is_dirty`.
    assert!(s.is_dirty() && status_dirty(&mut s), "a setting turned marks the drawing changed");
    // ... and no undo step was made, and the redo steps are not touched.
    assert_eq!(undo_steps(&s), 0, "no undo step");
    assert_eq!(s.history().unwrap().next_undo_label(), None);
    assert_eq!(s.revision(), revision, "no revision moves: the page's picture and the snap reads are still the ones of this revision");
    check_session(&s).unwrap();
    // Undo has nothing to take back, and the setting stays.
    let refused = s.execute("edit.undo", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("nothing to undo"), "{refused}");
    assert_eq!(shown(&mut s), (true, false, true, false), "Ctrl+Z past the start of the history leaves the grid as it is");
    assert!(s.is_dirty());
    // Every box is the same: no step for any of them.
    run(&mut s, "window.settings.set", json!({ "rulers": false }));
    run(&mut s, "window.settings.set", json!({ "guides": false }));
    run(&mut s, "window.settings.set", json!({ "connection_points": true }));
    run(&mut s, "window.settings.set", json!({ "grid": true, "rulers": true }));
    assert_eq!(undo_steps(&s), 0);
    assert_eq!(s.revision(), revision);
    assert_eq!(shown(&mut s), (true, true, false, true));
    // A value that is not a boolean is refused in words, and nothing changes.
    let e = s.execute("window.settings.set", &json!({ "rulers": "off" })).unwrap_err().to_string();
    assert!(e.contains("`rulers` must be true or false"), "{e}");
    assert_eq!(s.revision(), revision);
    check_session(&s).unwrap();
}

/// Undo and redo leave the settings alone. The grid is turned off, a shape is moved, and Ctrl+Z undoes the move only; and at the start of the history the grid is
/// still as it was turned. A setting turned between an undo and its redo keeps the redo, and the redo does not bring the old setting back.
#[test]
fn undo_after_a_toggle_and_a_move_undoes_only_the_move() {
    let mut s = new_drawing();
    let id = drop_shape(&mut s);
    let dropped = pin(&s, id).unwrap();
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    let moved = pin(&s, id).unwrap();
    assert!(moved.0 > dropped.0 + 0.5);
    assert_eq!(undo_steps(&s), 2, "the drop and the move: the grid is none");
    // Undo: the move, and not the grid.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(pin(&s, id), Some(dropped), "the move is undone");
    assert_eq!(shown(&mut s), (true, false, true, false), "and the grid stays off");
    // Undo again: the drop, and the grid is still off.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(pin(&s, id), None, "the drop is undone");
    assert_eq!(shown(&mut s), (true, false, true, false));
    // Past the start of the history: refused, the grid unchanged and the drawing still changed.
    assert!(s.execute("edit.undo", &json!({})).is_err());
    assert_eq!(shown(&mut s), (true, false, true, false));
    assert!(s.is_dirty(), "the drawing is back at its first edit, and the grid turned is a change of it");
    // Redo brings the shape back, not the grid.
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(pin(&s, id), Some(moved));
    assert_eq!(shown(&mut s), (true, false, true, false), "redo leaves the grid as it is");
    // A setting turned between an undo and its redo: the redo stays, and keeps the setting turned.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.history().unwrap().redo_len(), 1);
    run(&mut s, "window.settings.set", json!({ "grid": true, "rulers": false }));
    assert_eq!(s.history().unwrap().redo_len(), 1, "a setting turned does not end the redo steps");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(pin(&s, id), Some(moved));
    assert_eq!(shown(&mut s), (false, true, true, false), "the redo does not take the setting back to what it was when the move was undone");
    check_session(&s).unwrap();
}

/// The unsaved mark follows the settings as it does the edits. A drawing whose setting was turned is changed until it is saved; a save makes it clean; undoing an
/// edit back to the saved state is clean only when the settings are the saved ones too.
#[test]
fn a_setting_turned_marks_the_drawing_unsaved_until_it_is_saved() {
    let dir = Dir::new();
    let mut s = new_drawing();
    let id = drop_shape(&mut s);
    assert!(s.is_dirty() && status_dirty(&mut s));
    let path = dir.path("a.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert!(!s.is_dirty() && !status_dirty(&mut s));
    // An edit and its undo are clean again, as before this task: the settings are the saved ones.
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    assert!(s.is_dirty());
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.is_dirty(), "the undo of an edit made since the save, with no setting turned, is the saved drawing");
    // A setting turned: changed, with no step.
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    assert!(s.is_dirty() && status_dirty(&mut s));
    assert_eq!(undo_steps(&s), 1, "the drop only");
    // An edit, the setting, and the edit's undo: the drawing is not the saved one, for the grid.
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.is_dirty(), "the grid is turned since the save");
    assert_eq!(shown(&mut s), (true, false, true, false));
    // A save is the drawing as it is, and clean.
    run(&mut s, "file.save", json!({}));
    assert!(!s.is_dirty() && !status_dirty(&mut s));
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.is_dirty(), "and an edit undone after it is clean again, the saved grid kept");
    assert_eq!(shown(&mut s), (true, false, true, false));
    check_session(&s).unwrap();
}

/// A setting turned inside a batch is no step of it. A batch of the setting alone records no step; a batch of the setting and an edit records the one step, whose
/// undo takes the edit back and leaves the setting; a batch that fails takes the setting back with the rest.
#[test]
fn a_setting_turned_in_a_batch_is_no_step_of_the_batch() {
    let mut s = new_drawing();
    let only = json!({"commands": [{"command": "window.settings.set", "params": {"grid": false}}]});
    run(&mut s, "command.batch", only);
    assert_eq!(undo_steps(&s), 0, "a batch that turned a setting and made no edit has no step");
    assert_eq!(shown(&mut s), (true, false, true, false));
    assert!(s.is_dirty());
    // The setting and an edit: one step, which is the edit's.
    let both = json!({"commands": [
        {"command": "window.settings.set", "params": {"rulers": false}},
        {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}},
    ], "label": "Both"});
    let made = run(&mut s, "command.batch", both);
    let id = u32::try_from(made["results"][1]["id"].as_u64().unwrap()).unwrap();
    assert_eq!(undo_steps(&s), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(pin(&s, id), None, "the batch's edit is undone");
    assert_eq!(shown(&mut s), (false, false, true, false), "and both settings stay");
    // A batch that fails changes nothing, the setting it turned included.
    let failing = json!({"commands": [
        {"command": "window.settings.set", "params": {"guides": false}},
        {"command": "page.delete", "params": {"id": 99}},
    ]});
    assert!(s.execute("command.batch", &failing).is_err());
    assert_eq!(shown(&mut s), (false, false, true, false), "the guides were not turned");
    check_session(&s).unwrap();
}

/// An edit, then a setting turned, then the edit's undo (the undo goes back to a document made before the setting was turned), then a save: the file holds the
/// setting, and a reopen shows it. In `.diacraft`, in VSDX written from the model and in VSDX spliced into a file; and a second save of the same drawing is no edit.
#[test]
fn a_setting_turned_then_an_edit_undone_is_kept_by_a_save_and_a_reopen() {
    let dir = Dir::new();
    let want = (true, false, true, false);
    let mut s = new_drawing();
    let id = drop_shape(&mut s);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(shown(&mut s), want, "the undo took the move back and left the grid");
    let native = dir.path("a.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    assert_eq!(shown(&mut opened(&native)), want, ".diacraft");
    let first = std::fs::read(&native).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(&native).unwrap(), first, "a second save is no edit");
    let model = dir.path("a.vsdx");
    run(&mut s, "file.save_as", json!({ "path": model }));
    assert_eq!(shown(&mut opened(&model)), want, "VSDX from the model");
    // A file with its own windows part: an edit, the setting turned, the edit's undo, then a save into the file.
    let path = dir.write("own.vsdx", &vsdx(Some(WINDOWS_PART)));
    let mut v = opened(&path);
    run(&mut v, "page.add", json!({ "name": "Third" }));
    run(&mut v, "window.settings.set", json!({ "grid": false }));
    run(&mut v, "edit.undo", json!({}));
    run(&mut v, "file.save", json!({}));
    assert_eq!(shown(&mut opened(&path)), want, "VSDX into the file");
    let once = std::fs::read(&path).unwrap();
    let saved = run(&mut v, "file.save", json!({}));
    assert_eq!((saved["parts"]["rewritten"].as_u64(), saved["parts"]["added"].as_u64()), (Some(0), Some(0)), "{saved}");
    assert_eq!(std::fs::read(&path).unwrap(), once, "a second save is no edit");
}

/// Test 2 of the brief: the grid turned off is off again after a save and a reopen, as `.diacraft` and as VSDX written from the model, and the other settings with it.
#[test]
fn the_grid_turned_off_is_off_again_after_a_diacraft_save_and_a_vsdx_save() {
    let dir = Dir::new();
    let mut s = new_drawing();
    run(&mut s, "window.settings.set", json!({ "grid": false, "connection_points": true }));
    let want = (true, false, true, true);
    let native = dir.path("off.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    assert_eq!(shown(&mut opened(&native)), want, ".diacraft");
    let model = dir.path("off.vsdx");
    run(&mut s, "file.save_as", json!({ "path": model }));
    assert_eq!(shown(&mut opened(&model)), want, "VSDX from the model");
    // The file is a new drawing's: the skeleton's windows part, with the settings in its drawing window.
    let windows = String::from_utf8(parts(&std::fs::read(&model).unwrap())["/visio/windows.xml"].clone()).unwrap();
    assert!(windows.contains("<ShowGrid>0</ShowGrid>") && windows.contains("<ShowConnectionPoints>1</ShowConnectionPoints>"), "{windows}");
    // Reopened and saved again as `.diacraft`, the settings stay.
    let mut back = opened(&model);
    let again = dir.path("again.diacraft");
    run(&mut back, "file.save_as", json!({ "path": again }));
    assert_eq!(shown(&mut opened(&again)), want, "VSDX to .diacraft");
}

/// Test 2 of the brief, for a VSDX file saved into itself: the flag is set where the file holds it, a flag it lacks goes in, and nothing else of the part changes. A file
/// with no windows part is given one.
#[test]
fn a_vsdx_file_saved_in_place_keeps_the_settings_turned() {
    let dir = Dir::new();
    let original = vsdx(Some(WINDOWS_PART));
    let path = dir.write("own.vsdx", &original);
    let mut s = opened(&path);
    run(&mut s, "window.settings.set", json!({ "grid": false, "guides": false, "connection_points": true }));
    run(&mut s, "file.save", json!({}));
    // The windows part, and the bookkeeping of an edited save (the time it was saved and the application, in the two property parts): nothing else.
    let (before, after) = (parts(&original), parts(&std::fs::read(&path).unwrap()));
    let changed: Vec<&String> = after.keys().filter(|name| before.get(*name) != after.get(*name)).collect();
    assert_eq!(changed, ["/docProps/app.xml", "/docProps/core.xml", "/visio/windows.xml"], "the windows part and the bookkeeping");
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "no part added or taken out");
    assert_eq!(shown(&mut opened(&path)), (true, false, false, true));
    let windows = String::from_utf8(parts(&std::fs::read(&path).unwrap())["/visio/windows.xml"].clone()).unwrap();
    assert!(windows.contains("<ShowGrid>0</ShowGrid>") && windows.contains(r#"ShowGuides="0""#), "set where they were: {windows}");
    let at = |needle: &str| windows.find(needle).unwrap_or_else(|| panic!("{needle} in {windows}"));
    assert!(at("<ShowGrid>0</ShowGrid>") < at("<ShowConnectionPoints>1</ShowConnectionPoints>"), "put in at its place, after the grid: {windows}");
    assert!(at("<ShowConnectionPoints>1</ShowConnectionPoints>") < at("<SnapSettings>295</SnapSettings>"), "and before SnapSettings: {windows}");
    assert!(
        windows.contains("WindowType=\"Stencil\"") && windows.contains("ClientWidth=\"1680\"") && windows.contains("BASIC_M.vssx"),
        "the rest stays: {windows}"
    );
    // A file with no windows part: one is made, with the document's relationship to it.
    let bare = dir.write("bare.vsdx", &vsdx(None));
    let mut b = opened(&bare);
    run(&mut b, "window.settings.set", json!({ "rulers": false }));
    let saved = run(&mut b, "file.save", json!({}));
    assert_eq!(saved["parts"]["added"], 1, "{saved}");
    assert_eq!(shown(&mut opened(&bare)), (false, true, true, false));
}

/// Test 3 of the brief: an opened VSDX file with its own windows part keeps it byte for byte on a save that changes nothing, and on a save of another edit; and every other part of
/// the file stays as it was on the unedited save, but `docProps/app.xml`, which a save into this file built in code rewrote before this task too (measured on main `788df216`).
#[test]
fn an_opened_vsdx_keeps_its_windows_part_byte_identical_when_its_settings_did_not_change() {
    let dir = Dir::new();
    let original = vsdx(Some(WINDOWS_PART));
    let path = dir.write("own.vsdx", &original);
    let mut s = opened(&path);
    run(&mut s, "file.save", json!({}));
    let unedited = std::fs::read(&path).unwrap();
    assert_eq!(parts(&unedited)["/visio/windows.xml"], WINDOWS_PART.as_bytes(), "an unedited save keeps the part");
    let (before, after) = (parts(&original), parts(&unedited));
    let changed: Vec<&String> = after.keys().filter(|name| before.get(*name) != after.get(*name)).collect();
    assert!(changed.iter().all(|name| *name == "/docProps/app.xml"), "an unedited save changes no part of the drawing: {changed:?}");
    // A second save is the same file.
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(&path).unwrap(), unedited, "a second unedited save");
    // An edit of something else leaves the part as it was.
    run(&mut s, "page.add", json!({ "name": "Third" }));
    run(&mut s, "file.save", json!({}));
    assert_eq!(parts(&std::fs::read(&path).unwrap())["/visio/windows.xml"], WINDOWS_PART.as_bytes(), "another edit keeps it");
    // A setting turned and turned back writes nothing new either: what the drawing shows is what the file holds.
    let mut t = opened(&path);
    run(&mut t, "window.settings.set", json!({ "grid": false }));
    run(&mut t, "window.settings.set", json!({ "grid": true }));
    run(&mut t, "file.save", json!({}));
    assert_eq!(parts(&std::fs::read(&path).unwrap())["/visio/windows.xml"], WINDOWS_PART.as_bytes(), "turned back");
}

/// Test 4 of the brief: after a save of settings turned, a second save is no edit, in each format.
#[test]
fn a_second_save_is_no_edit() {
    let dir = Dir::new();
    // `.diacraft`: the same bytes.
    let mut s = new_drawing();
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    let native = dir.path("a.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    assert!(!s.is_dirty());
    let first = std::fs::read(&native).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(&native).unwrap(), first, ".diacraft");
    // VSDX spliced: no part rewritten.
    let path = dir.write("own.vsdx", &vsdx(Some(WINDOWS_PART)));
    let mut v = opened(&path);
    run(&mut v, "window.settings.set", json!({ "rulers": false }));
    run(&mut v, "file.save", json!({}));
    let once = std::fs::read(&path).unwrap();
    let saved = run(&mut v, "file.save", json!({}));
    assert_eq!((saved["parts"]["rewritten"].as_u64(), saved["parts"]["added"].as_u64()), (Some(0), Some(0)), "{saved}");
    assert_eq!(std::fs::read(&path).unwrap(), once, "VSDX");
    // A reopened file saved again is no edit either.
    let mut w = opened(&path);
    run(&mut w, "file.save", json!({}));
    assert_eq!(std::fs::read(&path).unwrap(), once, "VSDX reopened");
    // `.odg`.
    let odg = dir.write("a.odg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).bytes());
    let mut o = opened(&odg);
    run(&mut o, "window.settings.set", json!({ "grid": false }));
    assert_eq!(run(&mut o, "file.save", json!({}))["edited"], true);
    let odg_once = std::fs::read(&odg).unwrap();
    assert_eq!(run(&mut o, "file.save", json!({}))["edited"], false, "a second save of an .odg");
    assert_eq!(std::fs::read(&odg).unwrap(), odg_once);
    let mut o2 = opened(&odg);
    assert_eq!(run(&mut o2, "file.save", json!({}))["edited"], false, "an .odg reopened");
}

/// For `.odg`: no item of `settings.xml` holds them yet, so DiaCraft's own document file does; the grid turned off is off again after a save and a reopen,
/// and the drawing saved from it as `.diacraft` keeps them.
#[test]
fn an_odg_keeps_the_settings_in_diacrafts_document_file() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).bytes());
    let mut s = opened(&path);
    run(&mut s, "window.settings.set", json!({ "grid": false, "rulers": false, "guides": false }));
    run(&mut s, "file.save", json!({}));
    let file = crate::odg_build::part_of(&std::fs::read(&path).unwrap(), "DiaCraft/document.xml");
    assert!(file.contains("windows>") && file.contains("ShowGrid"), "{file}");
    let mut again = opened(&path);
    assert_eq!(shown(&mut again), (false, false, false, false));
    let native = dir.path("from-odg.diacraft");
    run(&mut again, "file.save_as", json!({ "path": native }));
    assert_eq!(shown(&mut opened(&native)), (false, false, false, false));
}

/// A flat drawing has no document file of DiaCraft's: a save of one whose settings were set says that they are not kept, and saves.
#[test]
fn a_flat_drawing_says_its_settings_are_not_kept() {
    let dir = Dir::new();
    let path = dir.write("a.fodg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).flat_bytes());
    let mut s = opened(&path);
    let saved = run(&mut s, "file.save", json!({}));
    assert!(saved["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("window's settings")), "{saved}");
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    let saved = run(&mut s, "file.save", json!({}));
    assert!(saved["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("not kept in a flat drawing")), "{saved}");
}

/// A window that names a page taken out goes from the file, and **the settings stay**: its flags are put in a drawing window of their own, in a save
/// into the file and in a save from the model.
#[test]
fn a_window_that_goes_with_its_page_leaves_the_settings_behind() {
    let dir = Dir::new();
    // Into the file: the drawing window names page 1.
    let path = dir.write("own.vsdx", &vsdx(Some(WINDOWS_PART)));
    let mut s = opened(&path);
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    run(&mut s, "page.delete", json!({ "id": 1 }));
    run(&mut s, "file.save", json!({}));
    let windows = String::from_utf8(parts(&std::fs::read(&path).unwrap())["/visio/windows.xml"].clone()).unwrap();
    assert!(!windows.contains(r#"Page="1""#), "the window of the page taken out goes: {windows}");
    assert_eq!(shown(&mut opened(&path)), (true, false, true, false), "its settings stay");
    // From the model: a new drawing's window names its first page.
    let mut n = new_drawing();
    run(&mut n, "page.add", json!({ "name": "Second" }));
    run(&mut n, "window.settings.set", json!({ "rulers": false }));
    run(&mut n, "page.delete", json!({ "id": 1 }));
    let model = dir.path("new.vsdx");
    run(&mut n, "file.save_as", json!({ "path": model }));
    assert_eq!(shown(&mut opened(&model)), (false, true, true, false));
}

/// A windows part with no `Window` at all, as 487 sample files hold one (no drawing window).
const NO_WINDOW: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Windows ClientWidth=\"1680\" ClientHeight=\"947\" xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"/>";

/// A windows part whose one window is a stencil's, which holds no flag, with the `ID` 0 that the drawing window of a new drawing's part has.
const STENCIL_ONLY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Windows ClientWidth=\"1680\" ClientHeight=\"947\" xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\">\r\n  <Window ID=\"0\" WindowType=\"Stencil\" WindowState=\"1025\" Document=\"C:\\x\\BASIC_M.vssx\" ReadOnly=\"1\"/>\r\n</Windows>";

/// The two windows parts of that hold no drawing window, by the name of the file each is put in.
const NO_DRAWING_WINDOW: [(&str, &str); 2] = [("empty.vsdx", NO_WINDOW), ("stencil.vsdx", STENCIL_ONLY)];

/// The windows part of a VSDX file, as text.
fn windows_of(bytes: &[u8]) -> String {
    String::from_utf8(parts(bytes)["/visio/windows.xml"].clone()).unwrap()
}

/// A windows part that holds no drawing `Window` counts as no settings, so the drawing shows what a new drawing shows (the grid, the rulers and the guides on, the connection
/// points off), as a drawing with no windows part does: an empty `Windows`, and one whose only window is a stencil's. (They opened with the grid and the rulers off.)
#[test]
fn a_windows_part_with_no_drawing_window_shows_a_new_drawings_settings() {
    let dir = Dir::new();
    for (name, part) in NO_DRAWING_WINDOW {
        let mut s = opened(&dir.write(name, &vsdx(Some(part))));
        assert_eq!(shown(&mut s), NEW, "{name}");
        assert!(settings(&mut s)["held"].is_null(), "{name}: the part holds no settings: {}", settings(&mut s));
        assert!(!s.is_dirty(), "{name}: opening it changes nothing");
        // A value it shows already writes nothing.
        run(&mut s, "window.settings.set", json!({ "grid": true, "rulers": true }));
        assert!(!s.is_dirty(), "{name}: the grid and the rulers are on already");
    }
}

/// The part's bytes stay as read on an unedited save; only the settings shown changed. A save after a value shown already was set is unedited, and a second save is the
/// same file. Every other part stays as the file had it too, but `docProps/app.xml`, which a save into a file built by the test kit rewrites with no setting turned (measured
/// it on main `788df216`, and the test `an_opened_vsdx_keeps_its_windows_part_byte_identical_when_its_settings_did_not_change` holds it so).
#[test]
fn a_windows_part_with_no_drawing_window_is_kept_byte_for_byte_by_an_unedited_save() {
    let dir = Dir::new();
    for (name, part) in NO_DRAWING_WINDOW {
        let original = vsdx(Some(part));
        let path = dir.write(name, &original);
        let mut s = opened(&path);
        run(&mut s, "window.settings.set", json!({ "grid": true }));
        let saved = run(&mut s, "file.save", json!({}));
        let unedited = std::fs::read(&path).unwrap();
        assert_eq!(windows_of(&unedited), part, "{name}: the windows part as read, byte for byte");
        let (before, after) = (parts(&original), parts(&unedited));
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{name}: no part added or taken out");
        let changed: Vec<&String> = after.keys().filter(|p| before.get(*p) != after.get(*p)).collect();
        assert!(changed.iter().all(|p| *p == "/docProps/app.xml"), "{name}: an unedited save changes no part of the drawing: {changed:?} ({saved})");
        run(&mut s, "file.save", json!({}));
        assert_eq!(std::fs::read(&path).unwrap(), unedited, "{name}: a second save");
        let mut again = opened(&path);
        assert_eq!(shown(&mut again), NEW, "{name}: reopened");
        run(&mut again, "file.save", json!({}));
        assert_eq!(std::fs::read(&path).unwrap(), unedited, "{name}: a save of the reopened drawing");
    }
}

/// A setting turned in a drawing whose windows part holds no drawing window, then saved, writes a drawing `Window` into that part as writes one for a new part
/// (the window of a new drawing's part, for the first page, with the rulers and the grid as the drawing shows them and the setting turned), keeps everything else of the part, and a
/// reopen shows the setting turned and the rest as before: the rulers stay on. A second save is no edit; `.diacraft` and VSDX saved from it keep the settings too.
#[test]
fn a_setting_turned_in_a_part_with_no_drawing_window_writes_one_and_a_reopen_keeps_it() {
    let dir = Dir::new();
    let want = (true, false, true, false);
    for (name, part) in NO_DRAWING_WINDOW {
        let path = dir.write(name, &vsdx(Some(part)));
        let mut s = opened(&path);
        run(&mut s, "window.settings.set", json!({ "grid": false }));
        assert_eq!(shown(&mut s), want, "{name}: only the grid changes");
        let answer = settings(&mut s);
        assert_eq!((answer["held"]["rulers"].as_bool(), answer["held"]["grid"].as_bool()), (Some(true), Some(false)), "{name}: {answer}");
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(saved["parts"]["added"].as_u64(), Some(0), "{name}: {saved}");
        let windows = windows_of(&std::fs::read(&path).unwrap());
        assert_eq!(windows.matches("WindowType=\"Drawing\"").count(), 1, "{name}: one drawing window: {windows}");
        for needle in [
            "ContainerType=\"Page\"",
            "Page=\"1\"",
            "<ShowRulers>1</ShowRulers>",
            "<ShowGrid>0</ShowGrid>",
            "ClientWidth=\"1680\"",
            "ClientHeight=\"947\"",
        ] {
            assert!(windows.contains(needle), "{name}: {needle} in {windows}");
        }
        if part == STENCIL_ONLY {
            assert!(
                windows.contains(r#"<Window ID="0" WindowType="Stencil""#) && windows.contains("BASIC_M.vssx"),
                "{name}: the stencil window stays: {windows}"
            );
            assert!(windows.contains(r#"<Window ID="1" WindowType="Drawing""#), "{name}: the new window takes an ID of its own: {windows}");
            assert!(windows.find("WindowType=\"Stencil\"") < windows.find("WindowType=\"Drawing\""), "{name}: after the stencil's: {windows}");
        }
        assert_eq!(shown(&mut opened(&path)), want, "{name}: reopened, the grid off and the rulers on");
        // A second save is no edit.
        let once = std::fs::read(&path).unwrap();
        let again = run(&mut s, "file.save", json!({}));
        assert_eq!((again["parts"]["rewritten"].as_u64(), again["parts"]["added"].as_u64()), (Some(0), Some(0)), "{name}: {again}");
        assert_eq!(std::fs::read(&path).unwrap(), once, "{name}: a second save");
        // `.diacraft`, and VSDX saved from it under another name, keep them.
        let native = dir.path(&format!("{name}.diacraft"));
        run(&mut s, "file.save_as", json!({ "path": native }));
        assert_eq!(shown(&mut opened(&native)), want, "{name}: .diacraft");
        let other = dir.path(&format!("other-{name}"));
        run(&mut opened(&native), "file.save_as", json!({ "path": other }));
        assert_eq!(shown(&mut opened(&other)), want, "{name}: .diacraft to VSDX");
    }
}

/// 2026-11-22; as amended: a `Window` whose `WindowType` is the number 1 is the drawing window, as the word `Drawing` is
/// (Learn's enumeration of window types gives `visDrawing` 1, `visStencil` 2 and `visSheet` 3). The 19 sample files whose only window is of type 1 then show that window's own settings
/// (off where it holds no value), where had them show a new drawing's. The numbers 2 and 3 and any other are no drawing window.
#[test]
fn a_window_whose_type_is_the_number_one_is_the_drawing_window_as_the_word_is() {
    let dir = Dir::new();
    // No flag in the window: the value for each (the rulers and the grid off, the guides on, the connection points off), the same for the number as for the word.
    for (label, ty) in [("word", "Drawing"), ("number", "1")] {
        let part = format!(r#"<Windows><Window ID="0" WindowType="{ty}"/></Windows>"#);
        let mut s = opened(&dir.write(&format!("bare-{label}.vsdx"), &vsdx(Some(&part))));
        assert_eq!(shown(&mut s), (false, false, true, false), "{label}: the window's own, which holds no value");
        assert!(!settings(&mut s)["held"].is_null(), "{label}: the part holds settings: {}", settings(&mut s));
    }
    // Flags in it: its own, as children and as an attribute, and a stencil's window before it does not stand in for it.
    let part = r#"<Windows><Window ID="1" WindowType="Stencil"><ShowGrid>1</ShowGrid></Window><Window ID="0" WindowType="1" ShowGuides="0"><ShowRulers>1</ShowRulers><ShowConnectionPoints>1</ShowConnectionPoints></Window></Windows>"#;
    let mut s = opened(&dir.write("own.vsdx", &vsdx(Some(part))));
    assert_eq!(shown(&mut s), (true, false, false, true), "the number's window holds the rulers, the guides off and the points on");
    // The numbers of the other window types, and any that is not 1, are no drawing window: a new drawing's settings.
    for ty in ["2", "3", "0", "11", "Sheet"] {
        let part = format!(r#"<Windows><Window ID="0" WindowType="{ty}"><ShowGrid>0</ShowGrid></Window></Windows>"#);
        let mut s = opened(&dir.write(&format!("not-{ty}.vsdx"), &vsdx(Some(&part))));
        assert_eq!(shown(&mut s), NEW, "type {ty} is no drawing window");
        assert!(settings(&mut s)["held"].is_null(), "type {ty}: {}", settings(&mut s));
    }
}

/// On saving: a setting turned is written into the window of type 1 (no second drawing window is made beside it), what the window shows already writes nothing, and a reopen
/// shows the setting turned.
#[test]
fn a_setting_turned_is_written_into_the_window_of_type_one_and_makes_no_second_drawing_window() {
    let dir = Dir::new();
    let part = r#"<Windows><Window ID="0" WindowType="1"><ShowRulers>1</ShowRulers><ShowGrid>0</ShowGrid></Window></Windows>"#;
    let original = vsdx(Some(part));
    let path = dir.write("one.vsdx", &original);
    let mut s = opened(&path);
    assert_eq!(shown(&mut s), (true, false, true, false));
    // A value shown already is no edit, and the part stays as read.
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    run(&mut s, "file.save", json!({}));
    assert_eq!(windows_of(&std::fs::read(&path).unwrap()), part, "an unedited save keeps the part byte for byte");
    // The grid turned on goes into that window.
    run(&mut s, "window.settings.set", json!({ "grid": true }));
    run(&mut s, "file.save", json!({}));
    let windows = windows_of(&std::fs::read(&path).unwrap());
    assert_eq!(windows.matches("<Window ").count(), 1, "one window: {windows}");
    assert!(windows.contains(r#"WindowType="1""#) && !windows.contains("WindowType=\"Drawing\""), "the window kept its type: {windows}");
    assert!(windows.contains("<ShowGrid>1</ShowGrid>") && windows.contains("<ShowRulers>1</ShowRulers>"), "{windows}");
    assert_eq!(shown(&mut opened(&path)), (true, true, true, false), "reopened");
}

/// Pages holding groups with no name (`draw:g` with no `draw:name`, which ODF 1.3 Part 3 §10.3.15 allows) whose markup gives them data the reader builds into the group's sheet: a
/// graphic style of its own (`draw:style-name`, Part 3 §19.219) and a glue point of its own (`draw:glue-point`, §10.3.16), each of which made the save of a setting refused; a
/// group with both, inside another group with no name; and two such groups on one page (the second is the second of its name). A plain group with no name is the guard.
fn unnamed_groups() -> Vec<(&'static str, Drawing)> {
    let [a, b, c] = [("A", 1.0), ("B", 4.0), ("C", 8.0)].map(|(name, x)| rect(name, (x, 1.0, 2.0, 1.0), ""));
    let glue = r#"<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/>"#;
    [
        ("style.odg", format!(r#"<draw:g draw:style-name="gr2">{a}{b}</draw:g>"#)),
        ("glue.odg", format!("<draw:g>{glue}{a}{b}</draw:g>")),
        ("nested.odg", format!(r#"<draw:g><draw:g draw:style-name="gr2">{glue}{a}{b}</draw:g>{c}</draw:g>"#)),
        ("two.odg", format!(r#"<draw:g draw:style-name="gr2">{a}{b}</draw:g><draw:g>{glue}{c}</draw:g>"#)),
        ("plain.odg", format!("<draw:g>{a}{b}</draw:g>")),
    ]
    .into_iter()
    .map(|(name, shapes)| (name, Drawing::new().page(&shapes)))
    .collect()
}

/// The shapes of the first page in pre-order: each one's name, its number of members and its sheet (the cells, sections and links that hold the group's style and glue points).
fn shapes_of(s: &Session) -> Vec<(String, usize, std::sync::Arc<diacraft_doc::Sheet>)> {
    let doc = s.document().unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<&std::sync::Arc<diacraft_doc::Shape>> = doc.pages.get(0).unwrap().shapes.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        out.push((shape.name_u.as_deref().unwrap_or("").to_owned(), shape.children.len(), std::sync::Arc::clone(&shape.sheet)));
        stack.extend(shape.children.iter().rev());
    }
    out
}

/// Problem 2 (cause: the document file counted what a group's own markup gives it, its graphic style and its glue points, as data of DiaCraft's, which a group with no name has
/// no key to carry, and refused the new entry): a setting turned in an `.odg` whose page holds such a group is saved, into DiaCraft's document file, which needs no entry for the
/// group; the reopened drawing has the grid off and every group and member as it was; a second save is no edit.
#[test]
fn an_odg_with_an_unnamed_group_saves_a_setting_turned() {
    let dir = Dir::new();
    for (name, drawing) in unnamed_groups() {
        let path = dir.write(name, &drawing.bytes());
        let mut s = opened(&path);
        let before = shapes_of(&s);
        assert!(before.iter().any(|(n, members, _)| n.is_empty() && *members > 0), "{name}: a group with no name");
        run(&mut s, "window.settings.set", json!({ "grid": false }));
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{name}: the save is refused: {e}"));
        assert_eq!(saved["edited"], true, "{name}: {saved}");
        let bytes = std::fs::read(&path).unwrap();
        let file = crate::odg_build::part_of(&bytes, "DiaCraft/document.xml");
        assert!(file.contains("windows>") && file.contains("<ShowGrid>0</ShowGrid>"), "{name}: {file}");
        assert!(!file.contains("group"), "{name}: no entry for a group: {file}");
        assert_eq!(crate::odg_build::content_of(&bytes), drawing.content_xml(), "{name}: the page's markup as it was");
        // A second save is no edit.
        assert_eq!(run(&mut s, "file.save", json!({}))["edited"], false, "{name}: a second save");
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}");
        let mut again = opened(&path);
        assert_eq!(shown(&mut again), (true, false, true, false), "{name}: reopened, the grid off");
        assert_eq!(shapes_of(&again), before, "{name}: the groups and their members as they were");
        assert_eq!(run(&mut again, "file.save", json!({}))["edited"], false, "{name}: a save of the reopened drawing");
    }
}

/// What problem 2's fix leaves as it was (the rule): a group with no name whose data was changed in DiaCraft still has no key to carry it, and a save says so in words, with a
/// setting turned or not; the same group with its data as the markup gave it saves.
#[test]
fn an_unnamed_group_given_data_in_diacraft_is_still_refused_in_words() {
    let dir = Dir::new();
    let (name, drawing) = unnamed_groups().into_iter().next().unwrap();
    let path = dir.write(name, &drawing.bytes());
    let mut s = opened(&path);
    s.edit("Test setup: data on a group", |doc: &mut diacraft_doc::Document| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let group = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let cell = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1"));
        std::sync::Arc::make_mut(&mut group.sheet).cells.insert(diacraft_doc::CellName::new("Cost"), cell);
        Ok(())
    })
    .unwrap();
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    let said = s.execute("file.save", &json!({})).unwrap_err().to_string();
    // The words name the group and what it needs, and do not tell a person to undo: the setting turned is what was done last, and undoing it would not help.
    assert!(said.contains("the group with no name numbered 1 on the page") && said.contains("has data of its own or a link to a master"), "{said}");
    assert!(!said.contains("Undo") && !said.contains("edit.undo"), "{said}");
    assert_eq!(std::fs::read(&path).unwrap(), drawing.bytes(), "nothing written");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true, "the data undone: the setting saves");
}

/// The groups of the first page in pre-order, each `(name, its members' names, its VSDX attribute UniqueID)`.
fn groups_with_data(s: &Session) -> Vec<(Option<String>, Vec<String>, Option<String>)> {
    let doc = s.document().unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<&std::sync::Arc<diacraft_doc::Shape>> = doc.pages.get(0).unwrap().shapes.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        if !shape.children.is_empty() {
            let members = shape.children.iter().map(|m| m.name_u.as_deref().unwrap_or("").to_owned()).collect();
            out.push((shape.name_u.as_deref().map(str::to_owned), members, shape.raw.get("UniqueID").map(str::to_owned)));
        }
        stack.extend(shape.children.iter().rev());
    }
    out
}

/// Gives every group of `list`, nested ones too, the VSDX data a VSDX file's group holds (an attribute of its own no `.odg` says, `UniqueID`, `{G1}`, `{G2}`, ... in pre-order).
fn give_groups_vsdx_data(list: &mut diacraft_doc::ShapeList, next: &mut u32) {
    for at in 0..list.len() {
        let shape = std::sync::Arc::make_mut(list.get_mut(at).unwrap());
        if !shape.children.is_empty() {
            *next += 1;
            shape.raw = diacraft_doc::RawAttrs::from_pairs([("UniqueID", format!("{{G{next}}}"))]);
            give_groups_vsdx_data(&mut shape.children, next);
        }
    }
}

/// The package `bytes` with its `content.xml` changed by `f`, which must change it, packed again entry by entry (the edit another program makes of the markup alone).
fn with_content(bytes: &[u8], f: impl FnOnce(&str) -> String) -> Vec<u8> {
    use diacraft_container::zip::{ArchiveWriter, Method};
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut w = ArchiveWriter::new();
    let mut f = Some(f);
    for entry in package.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        let mut data = package.part(&entry.name).unwrap().to_vec();
        if entry.name == "content.xml" {
            let before = String::from_utf8(data).unwrap();
            let after = f.take().map(|f| f(&before)).unwrap();
            assert_ne!(after, before, "the edit of content.xml changed nothing");
            data = after.into_bytes();
        }
        w.add(&entry.name, &data, if entry.name == "mimetype" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    assert!(f.is_none(), "the package has a content.xml");
    w.finish().unwrap()
}

/// `content` with a glue point of its own (`draw:glue-point`, ODF 1.3 Part 3 §10.3.16) given to the first group (`draw:g`), as another program gives one: data the reader builds into the
/// group's sheet, which no entry of DiaCraft's document file holds (the form `glue.odg`).
fn glue_on_the_first_group(content: &str) -> String {
    let at = ["<draw:g>", "<draw:g "].iter().filter_map(|tag| content.find(tag)).min().expect("a group");
    let end = at + content[at..].find('>').unwrap() + 1;
    format!(r#"{}<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/>{}"#, &content[..end], &content[end..])
}

/// Where meets: an `.odg` whose page holds groups with no name that **carry VSDX data** (the `.odg` a drawing whose groups hold a VSDX
/// file's data is written as by the model writer: the item of each group in DiaCraft's document file is keyed by its place among the page's groups with no name, `nth`, and stamped by
/// its members' names), a group alone, two on a page and one inside another; each as written, and with a glue point of its own that another program gave the first group, which the
/// markup gives it (its entry is left out of the file a save rewrites). A setting turned is saved, and every item stays in the file (an item is no entry the markup
/// gives); the reopened drawing has the setting, every group with no name, its members, its sheet and its VSDX data; a second save is no edit.
#[test]
fn an_odg_with_unnamed_groups_that_carry_vsdx_data_saves_a_setting_turned_and_keeps_the_data() {
    let dir = Dir::new();
    let items = |file: &str| file.matches(r#"of="group""#).count();
    let [a, b, c] = [("A", 1.0), ("B", 4.0), ("C", 8.0)].map(|(name, x)| rect(name, (x, 1.0, 2.0, 1.0), ""));
    let forms = [
        ("alone", format!("<draw:g>{a}{b}</draw:g>")),
        ("two", format!("<draw:g>{a}{b}</draw:g><draw:g>{c}</draw:g>")),
        ("nested", format!("<draw:g><draw:g>{a}{b}</draw:g>{c}</draw:g>")),
    ];
    for (form, shapes) in forms {
        // The drawing whose groups hold a VSDX file's data, written as an `.odg` by the model writer.
        let (mut model, _) = diacraft_odg::read(&Drawing::new().page(&shapes).bytes(), &diacraft_odg::Limits::default()).unwrap();
        give_groups_vsdx_data(&mut std::sync::Arc::make_mut(model.pages.get_mut(0).unwrap()).shapes, &mut 0);
        let carried = diacraft_odg::write_with(&model, None, &diacraft_odg::WriteOptions::default())
            .unwrap_or_else(|e| panic!("{form}: the model writer refused: {e}"))
            .bytes;
        let mut as_written: Option<Vec<(String, usize, std::sync::Arc<diacraft_doc::Sheet>)>> = None;
        for (name, bytes) in [(format!("{form}.odg"), carried.clone()), (format!("{form}-glue.odg"), with_content(&carried, glue_on_the_first_group))]
        {
            let path = dir.write(&name, &bytes);
            let file = crate::odg_build::part_of(&bytes, "DiaCraft/document.xml");
            let mut s = opened(&path);
            let want = groups_with_data(&s);
            assert!(
                !want.is_empty() && want.iter().all(|(n, _, data)| n.is_none() && data.is_some()),
                "{name}: groups with no name, each with its data: {want:?}"
            );
            assert_eq!(items(&file), want.len(), "{name}: an item for each group: {file}");
            assert!(file.contains(r#"name="" nth="1""#), "{name}: keyed by its place among the groups with no name: {file}");
            let sheets = shapes_of(&s);
            // The glue point another program gave is data the markup gives the first group: its sheet is not the one the file as written gives it.
            match &as_written {
                None => as_written = Some(sheets.clone()),
                Some(first) => assert_ne!(first.first(), sheets.first(), "{name}: the glue point is in the first group's sheet"),
            }
            run(&mut s, "window.settings.set", json!({ "grid": false }));
            let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{name}: the save of the setting is refused: {e}"));
            assert_eq!(saved["edited"], true, "{name}: {saved}");
            let now = std::fs::read(&path).unwrap();
            let file = crate::odg_build::part_of(&now, "DiaCraft/document.xml");
            assert!(file.contains("<ShowGrid>0</ShowGrid>"), "{name}: the setting is in the file: {file}");
            assert_eq!(items(&file), want.len(), "{name}: every item is still in the file: {file}");
            assert!(!file.contains("<diacraft:group"), "{name}: and no entry for a group: {file}");
            assert_eq!(crate::odg_build::content_of(&now), crate::odg_build::content_of(&bytes), "{name}: the page's markup as it was");
            assert_eq!(run(&mut s, "file.save", json!({}))["edited"], false, "{name}: a second save");
            assert_eq!(std::fs::read(&path).unwrap(), now, "{name}");
            let mut again = opened(&path);
            assert_eq!(shown(&mut again), (true, false, true, false), "{name}: reopened, the grid off");
            assert_eq!(groups_with_data(&again), want, "{name}: reopened, the groups, their members and their VSDX data");
            assert_eq!(shapes_of(&again), sheets, "{name}: reopened, every sheet as it was");
            assert_eq!(run(&mut again, "file.save", json!({}))["edited"], false, "{name}: a save of the reopened drawing");
        }
    }
}

/// The black-box check of problem 2 against LibreOffice 26.8.2.2, run by hand (ignored): each `.odg` of [`an_odg_with_an_unnamed_group_saves_a_setting_turned`], saved with the grid
/// turned off, is copied into `$WINDOWS_GATE_LO_DIR/in`; LibreOffice converts the copies with a profile of its own in that folder, to PDF (only a missing output is a failure: headless,
/// a package LibreOffice would offer to repair loads nothing) and to `.odg`, whose `content.xml` still holds as many groups and shapes as the file DiaCraft saved.
#[test]
#[ignore = "a black-box check against LibreOffice 26.8.2.2, run by hand with WINDOWS_GATE_LO_DIR set to an empty folder"]
fn lo_opens_the_odg_with_an_unnamed_group_saved_with_a_setting_turned() {
    let folder = std::path::PathBuf::from(std::env::var("WINDOWS_GATE_LO_DIR").expect("set WINDOWS_GATE_LO_DIR to an empty folder"));
    let (input, out) = (folder.join("in"), folder.join("out"));
    std::fs::create_dir_all(&input).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let dir = Dir::new();
    for (name, drawing) in unnamed_groups() {
        let path = dir.write(name, &drawing.bytes());
        let mut s = opened(&path);
        run(&mut s, "window.settings.set", json!({ "grid": false }));
        run(&mut s, "file.save", json!({}));
        let copy = input.join(name);
        std::fs::copy(&path, &copy).unwrap();
        let stem = name.trim_end_matches(".odg");
        for (to, made) in [("pdf", format!("{stem}.pdf")), ("odg", format!("{stem}.odg"))] {
            let status = std::process::Command::new("soffice")
                .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
                .args(["--headless", "--norestore", "--convert-to", to, "--outdir"])
                .arg(&out)
                .arg(&copy)
                .output()
                .expect("soffice is installed");
            assert!(out.join(&made).exists(), "{name}: LibreOffice wrote no {to} ({:?}): {}", status.status, String::from_utf8_lossy(&status.stderr));
        }
        let ours = crate::odg_build::content_of(&std::fs::read(&path).unwrap());
        let resaved = crate::odg_build::content_of(&std::fs::read(out.join(name)).unwrap());
        // A group's start tag is `<draw:g>` or `<draw:g ` (and `<draw:glue-point` is no group).
        let count = |xml: &str| {
            let groups = xml.matches("<draw:g>").count() + xml.matches("<draw:g ").count();
            (groups, xml.matches("<draw:custom-shape").count(), xml.matches("<draw:glue-point").count())
        };
        assert_eq!(count(&resaved), count(&ours), "{name}: the groups, shapes and glue points of LibreOffice's save: {resaved}");
        let (groups, shapes, glue) = count(&ours);
        println!("{name}: LibreOffice opened it, wrote a PDF, and saved an .odg holding its {groups} groups, {shapes} shapes and {glue} glue points");
    }
}
