//! The Operations menu of the Developer tab: Union, Combine, Fragment, Intersect and Subtract run from the menu on the shapes selected, in the
//! order the window keeps its selection in (a click selection in click order, a rubber band back to front, SHAPEOPS-PRIMARY), an entry is
//! enabled only when the engine says its operation can run, one that is refused says why in the status bar, each is one undo step, and the Developer
//! tab that holds the menu is turned on in File, Options. The pointer and the keyboard are given to egui as a person's would be, and what is
//! checked is the drawing the commands changed.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_engine::formatting::{Rgb, look_of};
use diacraft_testkit::package::{cell, shape};
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::work;
use diacraft_ui_egui::{ControlRequest, DiacraftApp};
use egui::{Key, Modifiers};
use serde_json::json;

use crate::editing::{click_at, click_with, drag_with, frame_of, key, shape_ids, type_text};
use crate::fixture::{self, placed, rectangle};
use crate::support::{Rest, TempDir, Window, click, controlled_with, fast_window, move_to, run_command, send};

const WIDE: [f32; 2] = [1280.0, 900.0];
const CTRL: Modifiers = Modifiers::COMMAND;

/// The page's size in the fixtures (inches): shapes sit in the middle of it, where a window shows them at 100%.
const RED: &str = "#FF0000";
const BLUE: &str = "#0000FF";

/// A rectangle `w` by `h` with its lower left at `(x, y)`, filled with `fill`; `extra` is more cells (a lock).
fn painted(id: u32, (x, y, w, h): (f64, f64, f64, f64), fill: &str, extra: &str) -> String {
    shape(id, &[placed(x, y, w, h), cell("FillPattern", "1"), cell("FillForegnd", fill), extra.to_owned(), rectangle(w, h)].concat())
}

/// The two squares of the specification's vectors, moved to the middle of the page: A, red, from (3, 3) to (5, 5), and B, blue, from
/// (4, 4) to (6, 6).
pub(crate) fn two_squares() -> String {
    [painted(1, (3.0, 3.0, 2.0, 2.0), RED, ""), painted(2, (4.0, 4.0, 2.0, 2.0), BLUE, "")].concat()
}

/// The shapes of the A-only, B-only and overlapping parts of [`two_squares`].
const A_ONLY: (f64, f64) = (3.4, 3.4);
const B_ONLY: (f64, f64) = (5.6, 5.6);

fn open_in(dir: &TempDir, name: &str, shapes: &str) -> Session {
    let path = fixture::write(dir.path(), name, &fixture::page_with(shapes));
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    session
}

/// A window of `size` at 100% on a drawing that holds `shapes`, with the Developer tab on and open.
pub(crate) fn developer_window(dir: &TempDir, shapes: &str, size: [f32; 2]) -> Window {
    let mut w = fast_window(DiacraftApp::new(open_in(dir, "ops.vsdx", shapes), None).with_inline_pictures(bundled_drawer()), size);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    run_command(&mut w, "window.developer", json!({"on": true}));
    run_command(&mut w, "window.tab", json!({"tab": "Developer"}));
    w
}

fn wide(dir: &TempDir, shapes: &str) -> Window {
    developer_window(dir, shapes, WIDE)
}

/// A click on the control called `name` in the last frame's list, then the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn menu_row(op: &str) -> String {
    format!("menu.ShapesObject{op}")
}

/// Opens the Operations menu and runs `op` from it.
fn run_from_menu(w: &mut Window, op: &str) {
    hit(w, "ribbon.Developer.OperationsMenu");
    hit(w, &menu_row(op));
}

/// Whether each of the menu's nine rows is enabled now, in the menu's order; the menu is opened and closed again.
fn menu_enabled(w: &mut Window) -> Vec<(&'static str, bool)> {
    hit(w, "ribbon.Developer.OperationsMenu");
    let rows = ["Union", "Combine", "Fragment", "Intersect", "Subtract", "Join", "Trim", "Offset", "UpdateAlignmentBox"]
        .map(|op| (op, w.state().element(&menu_row(op)).unwrap_or_else(|| panic!("{op} is not in the menu")).enabled));
    hit(w, "ribbon.Developer.OperationsMenu");
    assert!(w.state().element(&menu_row("Union")).is_none(), "the menu closed");
    rows.to_vec()
}

pub(crate) fn undo_steps(w: &Window) -> (usize, usize) {
    let h = w.state().session().history().unwrap();
    (h.undo_len(), h.redo_len())
}

fn first_page_shapes(w: &Window) -> Vec<(u32, usize)> {
    let d = w.state().session().document().unwrap();
    d.pages.get(0).unwrap().shapes.iter().map(|s| (s.id, geometry_sections(w, s.id))).collect()
}

/// How many `Geometry` sections shape `id` of the first page has of its own.
fn geometry_sections(w: &Window, id: u32) -> usize {
    let d = w.state().session().document().unwrap();
    let shape = d.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry" && !s.del).count()
}

/// How many rows the first `Geometry` section of shape `id` has.
fn rows_of(w: &Window, id: u32) -> usize {
    let d = w.state().session().document().unwrap();
    let shape = d.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && !s.del).map_or(0, |s| s.rows.len())
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn fill_of(w: &Window, id: u32) -> Option<Rgb> {
    look_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap().remove(0).fill
}

fn near(got: (f64, f64, f64, f64), want: (f64, f64, f64, f64)) -> bool {
    [got.0 - want.0, got.1 - want.1, got.2 - want.2, got.3 - want.3].iter().all(|d| d.abs() < 1e-6)
}

/// Selects A, then B with Shift: click order.
fn select_a_then_b(w: &mut Window) {
    click_at(w, A_ONLY.0, A_ONLY.1);
    click_with(w, B_ONLY, Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [1, 2]);
}

/// The `ids` of the last command of `command` in the engine's journal.
fn journal_ids(w: &Window, command: &str) -> Vec<u64> {
    let journal = w.state().session().journal();
    let (_, params) = journal.iter().rev().find(|(id, _)| id == command).unwrap_or_else(|| panic!("`{command}` was not sent: {journal:?}"));
    params["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

// ---- the five run from the menu ---------------------------------------------------------------------------------------------------------------

/// Each of the five runs from the menu on A and B and makes what its vector says (on the squares moved two inches along each axis); it is
/// one undo step, the originals go, and nothing is selected after.
#[test]
fn each_of_the_five_runs_from_the_menu_and_makes_what_its_vector_says() {
    for (op, command) in [
        ("Union", "shape.union"),
        ("Combine", "shape.combine"),
        ("Fragment", "shape.fragment"),
        ("Intersect", "shape.intersect"),
        ("Subtract", "shape.subtract"),
    ] {
        let dir = TempDir::new("ops-five");
        let mut w = wide(&dir, &two_squares());
        select_a_then_b(&mut w);
        let steps = undo_steps(&w);
        run_from_menu(&mut w, op);
        assert!(w.state().notice().is_none(), "{op}: {:?}", w.state().notice());
        assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "{op} is one undo step");
        assert!(w.state().selection().is_empty(), "{op}: nothing stays selected");
        assert_eq!(journal_ids(&w, command), [1, 2], "{op} sends the shapes in the order they were selected");
        let now = first_page_shapes(&w);
        let ids = shape_ids(&w);
        match op {
            // One shape with the outline of both, one section, a box of 3 by 3 from (3, 3).
            "Union" => {
                assert_eq!(now.len(), 1, "{now:?}");
                assert!(ids[0] > 2, "a new shape: {ids:?}");
                assert!(near(frame_of(&w, ids[0]), (3.0, 3.0, 3.0, 3.0)), "{:?}", frame_of(&w, ids[0]));
                assert_eq!(now[0].1, 1);
            }
            // The same box, and a section for each of the originals.
            "Combine" => {
                assert_eq!(now.len(), 1, "{now:?}");
                assert!(near(frame_of(&w, ids[0]), (3.0, 3.0, 3.0, 3.0)));
                assert_eq!(now[0].1, 2);
            }
            // Three shapes, the last frontmost: B less A, A less B, and the overlap.
            "Fragment" => {
                assert_eq!(now.len(), 3, "{now:?}");
                assert!(near(frame_of(&w, ids[0]), (4.0, 4.0, 2.0, 2.0)), "B less A: {:?}", frame_of(&w, ids[0]));
                assert!(near(frame_of(&w, ids[1]), (3.0, 3.0, 2.0, 2.0)), "A less B: {:?}", frame_of(&w, ids[1]));
                assert!(near(frame_of(&w, ids[2]), (4.0, 4.0, 1.0, 1.0)), "the overlap: {:?}", frame_of(&w, ids[2]));
                assert!(ids.iter().all(|id| *id > 2));
            }
            // The overlap alone.
            "Intersect" => {
                assert_eq!(now.len(), 1, "{now:?}");
                assert!(near(frame_of(&w, ids[0]), (4.0, 4.0, 1.0, 1.0)), "{:?}", frame_of(&w, ids[0]));
            }
            // A stays, with the same box and a new outline (an L: more rows than a rectangle's five), and B goes.
            _ => {
                assert_eq!(ids, [1], "A keeps its id and B is gone");
                assert!(near(frame_of(&w, 1), (3.0, 3.0, 2.0, 2.0)), "{:?}", frame_of(&w, 1));
                assert!(rows_of(&w, 1) > 5, "{} rows", rows_of(&w, 1));
                assert_eq!(now[0].1, 1);
            }
        }
        assert_eq!(w.state().ui_panics(), 0, "{op}");
    }
}

/// Union takes the formatting of the first shape: A was selected first, so the shape made is red.
#[test]
fn the_shape_made_has_the_formatting_of_the_shape_selected_first() {
    let dir = TempDir::new("ops-first");
    let mut w = wide(&dir, &two_squares());
    select_a_then_b(&mut w);
    run_from_menu(&mut w, "Union");
    let made = shape_ids(&w)[0];
    assert_eq!(fill_of(&w, made), Some(Rgb(255, 0, 0)));
}

// ---- the order of ids -------------------------------------------------------------------------------------------------------------------------

/// A click selection is in click order and a rubber band back to front: with A at the back and B in front, B then A (clicks) makes a blue shape, a rubber band a red
/// one (SHAPEOPS-PRIMARY), and `edit.select` keeps the order it is given.
#[test]
fn a_click_selection_goes_in_click_order_and_a_rubber_band_back_to_front() {
    // The first-clicked shape is first.
    let dir = TempDir::new("ops-click-order");
    let mut w = wide(&dir, &two_squares());
    click_at(&mut w, B_ONLY.0, B_ONLY.1);
    click_with(&mut w, A_ONLY, Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [2, 1], "B was clicked first");
    run_from_menu(&mut w, "Union");
    assert_eq!(journal_ids(&w, "shape.union"), [2, 1]);
    assert_eq!(fill_of(&w, shape_ids(&w)[0]), Some(Rgb(0, 0, 255)), "B's formatting");

    // A rubber band lists what it takes back to front, so A (the backmost) is first, whichever corner it started from.
    for (from, to) in [((2.0, 2.0), (7.0, 7.0)), ((7.0, 7.0), (2.0, 2.0)), ((2.0, 7.0), (7.0, 2.0))] {
        let dir = TempDir::new("ops-band-order");
        let mut w = wide(&dir, &two_squares());
        drag_with(&mut w, from, to, Modifiers::NONE);
        assert_eq!(w.state().selection(), [1, 2], "a band from {from:?} to {to:?} goes back to front");
        run_from_menu(&mut w, "Union");
        assert_eq!(journal_ids(&w, "shape.union"), [1, 2]);
        assert_eq!(fill_of(&w, shape_ids(&w)[0]), Some(Rgb(255, 0, 0)), "A's formatting");
    }

    // The same two shapes with B at the back: the band's first shape is B.
    let dir = TempDir::new("ops-band-reordered");
    let swapped = [painted(2, (4.0, 4.0, 2.0, 2.0), BLUE, ""), painted(1, (3.0, 3.0, 2.0, 2.0), RED, "")].concat();
    let mut w = wide(&dir, &swapped);
    drag_with(&mut w, (2.0, 2.0), (7.0, 7.0), Modifiers::NONE);
    assert_eq!(w.state().selection(), [2, 1], "the page lists B first: it is at the back");
    run_from_menu(&mut w, "Union");
    assert_eq!(fill_of(&w, shape_ids(&w)[0]), Some(Rgb(0, 0, 255)));

    // A Shift-click after a band adds the shape after the band's.
    let dir = TempDir::new("ops-band-shift");
    let three = [two_squares(), painted(3, (7.0, 3.0, 1.0, 1.0), "#00FF00", "")].concat();
    let mut w = wide(&dir, &three);
    drag_with(&mut w, (2.0, 2.0), (7.0, 7.0), Modifiers::NONE);
    assert_eq!(w.state().selection(), [1, 2]);
    click_with(&mut w, (7.5, 3.5), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [1, 2, 3], "the Shift-click comes last");

    // The control channel's `edit.select` keeps the order given.
    let dir = TempDir::new("ops-select-order");
    let mut w = wide(&dir, &two_squares());
    run_command(&mut w, "edit.select", json!({"ids": [2, 1]}));
    assert_eq!(w.state().selection(), [2, 1]);
    run_command(&mut w, "operations.union", json!({}));
    assert_eq!(journal_ids(&w, "shape.union"), [2, 1]);
}

// ---- greying ----------------------------------------------------------------------------------------------------------------------------------

/// With nothing selected every entry is quiet and the Operations button is too; with one shape Fragment alone can run; with two all five can. The four that are
/// not built stay drawn and quiet whatever is selected. The words of a quiet one are the engine's.
#[test]
fn an_entry_is_enabled_only_when_the_engine_says_its_operation_can_run() {
    let dir = TempDir::new("ops-greying");
    let mut w = wide(&dir, &two_squares());
    assert!(!w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled, "nothing is selected");
    for op in ["union", "combine", "fragment", "intersect", "subtract"] {
        assert_eq!(w.state().command_enabled(&format!("operations.{op}")), Err("nothing is selected".to_owned()), "{op}");
    }

    // One shape: the others need two; Fragment takes one.
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    assert!(w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled, "Fragment can run");
    assert_eq!(
        menu_enabled(&mut w),
        [
            ("Union", false),
            ("Combine", false),
            ("Fragment", true),
            ("Intersect", false),
            ("Subtract", false),
            ("Join", false),
            ("Trim", false),
            ("Offset", false),
            ("UpdateAlignmentBox", false)
        ]
    );
    let words = w.state().command_enabled("operations.union").unwrap_err();
    assert!(words.contains("Union needs two shapes at least"), "{words}");

    // Two: all five; the four that are not built stay quiet.
    click_with(&mut w, B_ONLY, Modifiers::SHIFT);
    assert_eq!(
        menu_enabled(&mut w),
        [
            ("Union", true),
            ("Combine", true),
            ("Fragment", true),
            ("Intersect", true),
            ("Subtract", true),
            ("Join", false),
            ("Trim", false),
            ("Offset", false),
            ("UpdateAlignmentBox", false)
        ]
    );

    // The selection going takes them away again.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    assert!(!w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled);
}

/// A shape that cannot be deleted refuses the commands that would delete it, and Subtract only when it is not the first shape: the quiet entries say
/// `LockDelete`.
#[test]
fn a_locked_shape_greys_what_would_delete_it_and_subtract_by_which_is_first() {
    let dir = TempDir::new("ops-locked");
    let shapes = [painted(1, (3.0, 3.0, 2.0, 2.0), RED, &cell("LockDelete", "1")), painted(2, (4.0, 4.0, 2.0, 2.0), BLUE, "")].concat();
    let mut w = wide(&dir, &shapes);
    select_a_then_b(&mut w);
    assert_eq!(
        menu_enabled(&mut w)[..5],
        [("Union", false), ("Combine", false), ("Fragment", false), ("Intersect", false), ("Subtract", true)],
        "A, the locked shape, is first: Subtract does not delete it"
    );
    for op in ["union", "combine", "fragment", "intersect"] {
        let words = w.state().command_enabled(&format!("operations.{op}")).unwrap_err();
        assert!(words.contains("LockDelete"), "{op}: {words}");
    }
    // The other way round A would be deleted by Subtract too.
    key(&mut w, Key::Escape, Modifiers::NONE);
    click_at(&mut w, B_ONLY.0, B_ONLY.1);
    click_with(&mut w, A_ONLY, Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [2, 1]);
    // With every entry quiet the Operations button is quiet too (as the other menus of the ribbon are), so the words are read from the commands.
    assert!(!w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled, "B first, and A, which is locked, would be deleted by every one");
    assert!(w.state().command_enabled("operations.subtract").unwrap_err().contains("LockDelete"));
}

/// With no drawing open there is nothing to select and no command can run, and the window says so.
#[test]
fn with_no_drawing_open_no_operation_can_run() {
    let mut w = crate::support::window(DiacraftApp::new(Session::new(), None).with_inline_pictures(bundled_drawer()), WIDE);
    w.rest();
    for op in ["union", "combine", "fragment", "intersect", "subtract"] {
        assert_eq!(w.state().command_enabled(&format!("operations.{op}")), Err("no document open".to_owned()), "{op}");
        assert!(w.state_mut().execute(&format!("operations.{op}"), &json!({})).is_err(), "{op}");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// A group in the selection is refused, in the engine's words.
#[test]
fn a_selection_with_a_group_greys_the_entries_that_need_areas() {
    let dir = TempDir::new("ops-group");
    let group = fixture::group(10, &placed(7.0, 3.0, 2.0, 2.0), "", &fixture::boxed(11, 0.0, 0.0, 2.0, 2.0));
    let mut w = wide(&dir, &[two_squares(), group].concat());
    run_command(&mut w, "edit.select", json!({"ids": [1, 10]}));
    assert_eq!(w.state().selection(), [1, 10]);
    assert!(!w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled, "no entry can run: the button is quiet");
    for op in ["union", "combine", "fragment", "intersect", "subtract"] {
        let words = w.state().command_enabled(&format!("operations.{op}")).unwrap_err();
        assert!(words.contains("is a group"), "{op}: {words}");
    }
    // The button's tooltip is the first of them: a person who cannot open the menu is told why.
}

// ---- the entries that are not built ----------------------------------------------------------------------------------------------------------

/// Join, Trim, Offset and Update Alignment Box are drawn in the menu and quiet, and a click on one does nothing; hovering one says it is not available yet, in
/// the words a quiet button of the ribbon uses (one wording for a control that is not built, `widgets::tip`).
#[test]
fn the_entries_that_are_not_built_stay_quiet_and_say_so_as_a_quiet_button_does() {
    let dir = TempDir::new("ops-later");
    let mut w = wide(&dir, &two_squares());
    select_a_then_b(&mut w);
    let steps = undo_steps(&w);
    for op in ["Join", "Trim", "Offset", "UpdateAlignmentBox"] {
        hit(&mut w, "ribbon.Developer.OperationsMenu");
        let row = w.state().element(&menu_row(op)).unwrap_or_else(|| panic!("{op} is drawn")).clone();
        assert!(!row.enabled, "{op}");
        click(&mut w, row.rect.center());
        w.rest();
        assert!(w.state().element(&menu_row(op)).is_none(), "{op}: a click closes the menu, as a click on any row does");
        assert!(w.state().notice().is_none(), "{op}: and says nothing");
    }
    assert_eq!((undo_steps(&w), shape_ids(&w)), (steps, vec![1, 2]), "nothing was done");
    for (op, words) in [
        ("Join", "Join (not available yet)"),
        ("Trim", "Trim (not available yet)"),
        ("Offset", "Offset (not available yet)"),
        ("UpdateAlignmentBox", "Update Alignment Box (not available yet)"),
    ] {
        hit(&mut w, "ribbon.Developer.OperationsMenu");
        assert_eq!(hovered_words(&mut w, &menu_row(op)), [words], "{op}");
        hit(&mut w, "ribbon.Developer.OperationsMenu");
    }
    // A quiet button of the ribbon with no command says it in the same words.
    assert_eq!(hovered_words(&mut w, "ribbon.Developer.MacroSecurity"), ["Macro Security (not available yet)"]);
}

/// A quiet entry of the five says why, in the engine's words, in its tooltip; one that works has none.
#[test]
fn a_quiet_entry_says_why_in_its_tooltip_and_one_that_works_says_nothing() {
    let dir = TempDir::new("ops-why");
    let mut w = wide(&dir, &two_squares());
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    let union = hovered_words(&mut w, &menu_row("Union"));
    assert!(union.len() == 1 && union[0].contains("Union needs two shapes at least"), "{union:?}");
    assert_eq!(hovered_words(&mut w, &menu_row("Fragment")), Vec::<String>::new(), "Fragment can run: no tooltip");
}

/// The words of the tooltips shown while the pointer rests on the control `name` (a frame is 1/60 s here, and a tooltip waits for the pointer): the ones this file's tests
/// look for, the quiet entries' and the operations'.
fn hovered_words(w: &mut Window, name: &str) -> Vec<String> {
    hovered_words_where(w, name, |v| v.contains("not available yet") || v.contains("needs two") || v.contains("LockDelete"))
}

/// The words of the tooltips shown while the pointer rests on the control `name`, those that `keep` says to keep, sorted, each once.
pub fn hovered_words_where(w: &mut Window, name: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
    use egui_kittest::kittest::{NodeT, Queryable};
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    move_to(w, at);
    for _ in 0..60 {
        w.step();
    }
    // (A label keeps its words in the node's value; the menu's own rows are buttons, with their labels.)
    let mut words: Vec<String> = w
        .query_all(egui_kittest::kittest::By::new().role(egui::accesskit::Role::Label))
        .filter_map(|n| n.accesskit_node().value().map(|v| v.to_string()))
        .filter(|v| keep(v))
        .collect();
    words.sort();
    words.dedup();
    words
}

// ---- a refusal --------------------------------------------------------------------------------------------------------------------------------

/// Intersect of squares that do not meet passes the check (both have an area) and is refused by its result: the engine's words are in the status bar, the drawing
/// is as it was, there is no undo step and the selection stays.
#[test]
fn a_refused_run_shows_the_engines_words_and_changes_nothing() {
    let dir = TempDir::new("ops-refused");
    let apart = [painted(1, (2.0, 3.0, 1.0, 1.0), RED, ""), painted(2, (7.0, 3.0, 1.0, 1.0), BLUE, "")].concat();
    let mut w = wide(&dir, &apart);
    click_at(&mut w, 2.5, 3.5);
    click_with(&mut w, (7.5, 3.5), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [1, 2]);
    let before = w.state().session().document().unwrap().clone();
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    assert_eq!(menu_enabled(&mut w)[3], ("Intersect", true), "the check cannot know the shapes miss each other");
    run_from_menu(&mut w, "Intersect");
    let notice = w.state().notice().expect("the refusal is said").to_owned();
    assert!(notice.contains("no area in common") && notice.contains("shape.intersect"), "{notice}");
    assert_eq!(w.state().session().document().unwrap(), &before, "the drawing is as it was");
    assert_eq!((w.state().session().revision(), undo_steps(&w)), (revision, steps), "no step was added");
    assert_eq!(w.state().selection(), [1, 2], "and the shapes are still selected");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A command run by name while its entry would be quiet is refused with the engine's words, as the control channel and a script meet it.
#[test]
fn a_command_run_while_its_entry_is_quiet_is_refused_in_the_engines_words() {
    let dir = TempDir::new("ops-quiet-run");
    let mut w = wide(&dir, &two_squares());
    let nothing = w.state_mut().execute("operations.union", &json!({})).unwrap_err().to_string();
    assert!(nothing.contains("nothing is selected"), "{nothing}");
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    let before = w.state().session().revision();
    let one = w.state_mut().execute("operations.union", &json!({})).unwrap_err().to_string();
    assert!(one.contains("Union needs two shapes at least"), "{one}");
    assert_eq!(w.state().session().revision(), before);
    let params = w.state_mut().execute("operations.union", &json!({"ids": [1, 2]})).unwrap_err().to_string();
    assert!(params.contains("unknown parameter"), "the window's command takes no ids: {params}");
}

// ---- undo and redo ----------------------------------------------------------------------------------------------------------------------------

/// Each operation is one undo step from the window (Ctrl+Z) and Redo (Ctrl+Y) makes it again.
#[test]
fn each_operation_is_one_undo_step_and_redo_makes_it_again() {
    for op in ["Union", "Combine", "Fragment", "Intersect", "Subtract"] {
        let dir = TempDir::new("ops-undo");
        let mut w = wide(&dir, &two_squares());
        let before = w.state().session().document().unwrap().clone();
        select_a_then_b(&mut w);
        run_from_menu(&mut w, op);
        let after = w.state().session().document().unwrap().clone();
        assert_ne!(after, before, "{op} changed the drawing");
        key(&mut w, Key::Z, CTRL);
        assert_eq!(w.state().session().document().unwrap(), &before, "{op}: one Ctrl+Z brings the originals back, with their ids and cells");
        assert_eq!(shape_ids(&w), [1, 2]);
        key(&mut w, Key::Y, CTRL);
        assert_eq!(w.state().session().document().unwrap(), &after, "{op}: Ctrl+Y makes it again");
        assert_eq!(undo_steps(&w).0, 1, "{op}");
    }
}

// ---- the cache --------------------------------------------------------------------------------------------------------------------------------

/// The engine is asked when the selection or the drawing changes, and not for each frame: thirty frames with the Developer tab shown ask nothing, the menu's
/// other entries are asked once when it opens and not when it opens again, and a change of selection or of the drawing asks again.
#[test]
fn the_engine_is_asked_when_the_selection_or_the_drawing_changes_and_never_for_a_frame() {
    let dir = TempDir::new("ops-cache");
    let mut w = wide(&dir, &two_squares());
    let frames = |w: &mut Window, n: usize| (0..n).for_each(|_| w.step());
    work::reset();
    frames(&mut w, 10);
    assert_eq!(work::work().shape_op_checks, 0, "nothing is selected: nothing to ask");
    work::reset();
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    let first = work::work().shape_op_checks;
    assert_eq!(first, 1, "the button asks for the first entry that can run, and Union can");
    frames(&mut w, 30);
    assert_eq!(work::work().shape_op_checks, first, "thirty frames ask nothing more");
    // Opening the menu asks about the four others once, and closing and opening it again asks nothing.
    menu_enabled(&mut w);
    let all = work::work().shape_op_checks;
    assert_eq!(all, 5, "one question for each operation, Union's kept from the button");
    menu_enabled(&mut w);
    frames(&mut w, 10);
    assert_eq!(work::work().shape_op_checks, 5, "the answers are kept");
    // Another selection is a new question; so is an edit of the drawing.
    key(&mut w, Key::Escape, Modifiers::NONE);
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    let one = work::work().shape_op_checks;
    assert!(one > 5, "a selection of one shape is asked about: {one}");
    // **And its answers are kept in turn**: thirty frames of the second selection ask nothing (an entry that held the new answers under the old selection would ask
    // in every frame, up to five times).
    frames(&mut w, 30);
    assert_eq!(work::work().shape_op_checks, one, "the second selection's answers are kept");
    menu_enabled(&mut w);
    let menu = work::work().shape_op_checks;
    frames(&mut w, 30);
    assert_eq!(work::work().shape_op_checks, menu, "and so are the menu's");
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0}));
    frames(&mut w, 10);
    let moved = work::work().shape_op_checks;
    assert!(moved > menu, "the drawing changed: asked again");
    frames(&mut w, 30);
    assert_eq!(work::work().shape_op_checks, moved, "and kept again");
}

/// The answers are those of the selection they were asked for: A, then A and B, then A again gives A's answers each time (Union needs two shapes; with two it
/// can run), and each change of selection asks once and no more.
#[test]
fn going_back_to_a_selection_gives_that_selections_answers() {
    let dir = TempDir::new("ops-back");
    let mut w = wide(&dir, &two_squares());
    let frames = |w: &mut Window, n: usize| (0..n).for_each(|_| w.step());
    let union = |w: &Window| w.state().command_enabled("operations.union");
    let fragment = |w: &Window| w.state().command_enabled("operations.fragment");
    for round in 0..2 {
        work::reset();
        run_command(&mut w, "edit.select", json!({"ids": [1]}));
        let words = union(&w).unwrap_err();
        assert!(words.contains("Union needs two shapes at least"), "round {round}, A alone: {words}");
        assert_eq!(fragment(&w), Ok(()), "round {round}: Fragment takes one");
        let asked = work::work().shape_op_checks;
        assert!((1..=4).contains(&asked), "round {round}: A is asked about once: {asked}");
        frames(&mut w, 30);
        assert_eq!(work::work().shape_op_checks, asked, "round {round}: A's answers are kept");
        run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
        assert_eq!(union(&w), Ok(()), "round {round}: A and B");
        let both = work::work().shape_op_checks;
        assert!(both > asked, "round {round}: A and B is a new question");
        frames(&mut w, 30);
        assert_eq!(work::work().shape_op_checks, both, "round {round}: A and B's answers are kept");
    }
    // Back to A: its answer, and not A and B's.
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert!(union(&w).is_err(), "A alone cannot be united");
    assert_eq!(fragment(&w), Ok(()));
}

/// Opens `path` as the window does, on a thread of its own and into a session of its own (the control channel's `file.open`), and waits for the answer.
fn open_in_the_window(w: &mut Window, tx: &Sender<ControlRequest>, path: &Path) {
    let answer = send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        w.step();
        if let Ok(reply) = answer.try_recv() {
            reply.unwrap();
            w.rest();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened in 30 s");
}

/// The answers are kept for the drawing's generation as well as its revision: **a file that is opened starts its revision at 1 again**, with the same page and
/// shape ids, and the new drawing's shapes are asked about, not the old one's answers read back.
#[test]
fn the_cache_does_not_carry_an_answer_across_a_file_open() {
    let dir = TempDir::new("ops-cache-open");
    let plain = fixture::write(dir.path(), "plain.vsdx", &fixture::page_with(&two_squares()));
    let locked = [painted(1, (3.0, 3.0, 2.0, 2.0), RED, &cell("LockDelete", "1")), painted(2, (4.0, 4.0, 2.0, 2.0), BLUE, "")].concat();
    let locked = fixture::write(dir.path(), "locked.vsdx", &fixture::page_with(&locked));
    let (mut w, tx) = controlled_with(|app| app.with_inline_pictures(bundled_drawer()));
    open_in_the_window(&mut w, &tx, &plain);
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    let revision = w.state().session().revision();
    assert_eq!(w.state().command_enabled("operations.union"), Ok(()));
    work::reset();
    assert_eq!(w.state().command_enabled("operations.union"), Ok(()));
    assert_eq!(work::work().shape_op_checks, 0, "the answer is kept");
    // The other file: the same page, the same shapes selected, the same revision (the session counts from 1 again), another answer.
    open_in_the_window(&mut w, &tx, &locked);
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    assert_eq!(w.state().session().revision(), revision, "the revision is the same: only the drawing's generation tells the drawings apart");
    assert_eq!(w.state().selection(), [1, 2]);
    let words = w.state().command_enabled("operations.union").unwrap_err();
    assert!(words.contains("LockDelete"), "the locked drawing's answer: {words}");
    assert_eq!(work::work().shape_op_checks, 1, "the new drawing was asked about");
    // And back.
    open_in_the_window(&mut w, &tx, &plain);
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    assert_eq!(w.state().session().revision(), revision);
    assert_eq!(w.state().command_enabled("operations.union"), Ok(()));
}

// ---- the Developer tab -------------------------------------------------------------------------------------------------------------------------

/// The Developer tab is off in a new window and is turned on in File, Options, Customize Ribbon: the box takes effect at OK, Cancel and Escape leave the tab
/// off, and only that box can be changed.
#[test]
fn the_developer_tab_is_turned_on_in_file_options_customize_ribbon() {
    let dir = TempDir::new("ops-options");
    let mut w = fast_window(DiacraftApp::new(open_in(&dir, "ops.vsdx", &two_squares()), None).with_inline_pictures(bundled_drawer()), WIDE);
    w.rest();
    assert!(w.state().element("tab.Developer").is_none(), "off until it is turned on");
    assert!(w.state().options_dialog_tab().is_none());

    // File, then Options: the File view closes behind the dialog.
    hit(&mut w, "tab.File");
    assert_eq!(w.state().window_state()["backstage"], "Home");
    hit(&mut w, "backstage.Options");
    assert_eq!(w.state().options_dialog_tab(), Some("Customize Ribbon"));
    assert!(w.state().window_state()["backstage"].is_null());
    assert_eq!(w.state().window_state()["options"], "Customize Ribbon");
    // Only Customize Ribbon and Advanced work in the list, and only the Developer box in the page.
    for section in ["General", "Proofing", "Save", "Language", "Accessibility", "Quick Access Toolbar", "Add-ins", "Trust Center"] {
        assert!(!w.state().element(&format!("options.section.{section}")).unwrap().enabled, "{section}");
    }
    assert!(w.state().element("options.section.Customize Ribbon").unwrap().enabled);
    // A quiet section says so in the ribbon's one wording for a control that is not built.
    assert_eq!(hovered_words(&mut w, "options.section.General"), ["General (not available yet)"]);
    for tab in ["Home", "Insert", "Draw", "Design", "Data", "Process", "Review", "View", "Help"] {
        assert!(!w.state().element(&format!("options.tab.{tab}")).unwrap().enabled, "{tab} always shows");
    }
    assert!(w.state().element("options.tab.Developer").unwrap().enabled);
    assert_eq!(w.state().options_dialog_shows_developer(), Some(false));

    // Ticking the box changes nothing until OK; Cancel leaves it as it was.
    hit(&mut w, "options.tab.Developer");
    assert_eq!(w.state().options_dialog_shows_developer(), Some(true));
    assert!(w.state().element("tab.Developer").is_none(), "not until OK");
    hit(&mut w, "options.cancel");
    assert!(w.state().options_dialog_tab().is_none() && w.state().element("tab.Developer").is_none());

    // Escape is Cancel too.
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    hit(&mut w, "options.tab.Developer");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().options_dialog_tab().is_none() && w.state().element("tab.Developer").is_none());

    // OK turns the tab on, and the tab shows with the Operations menu in it.
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    hit(&mut w, "options.tab.Developer");
    hit(&mut w, "options.ok");
    assert!(w.state().options_dialog_tab().is_none());
    hit(&mut w, "tab.Developer");
    assert!(w.state().element("ribbon.Developer.OperationsMenu").is_some());
    // The box starts from what the window has: ticked now, and unticking it and OK takes the tab away.
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    assert_eq!(w.state().options_dialog_shows_developer(), Some(true));
    hit(&mut w, "options.tab.Developer");
    hit(&mut w, "options.ok");
    assert!(w.state().element("tab.Developer").is_none(), "the tab is gone");
    assert!(!w.state().window_state()["tab"].as_str().unwrap().eq("Developer"), "and so is its body");
    // A dialog can always be closed by its command, and a tab it does not have is refused.
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    run_command(&mut w, "window.options", json!({"close": true}));
    assert!(w.state().options_dialog_tab().is_none());
    let refused = w.state_mut().execute("window.options", &json!({"tab": "General"})).unwrap_err().to_string();
    assert!(refused.contains("Customize Ribbon"), "{refused}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// With a shape selected and the Options dialog open, a letter typed and Delete pressed reach the dialog and not the drawing behind it: no label is started on
/// the shape and the shape is not deleted (the dialog is modal, `is_modal_open`).
#[test]
fn keys_typed_with_the_options_dialog_open_leave_the_drawing_alone() {
    let dir = TempDir::new("ops-options-keys");
    let mut w = wide(&dir, &two_squares());
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    assert_eq!(w.state().selection(), [1]);
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    let (before, steps) = (w.state().session().document().unwrap().clone(), undo_steps(&w));
    type_text(&mut w, "x");
    assert_eq!(w.state().window_state()["editingText"], false, "no label was started on the shape behind the dialog");
    key(&mut w, Key::Delete, Modifiers::NONE);
    key(&mut w, Key::Backspace, Modifiers::NONE);
    assert_eq!(w.state().session().document().unwrap(), &before, "the drawing is as it was");
    assert_eq!((undo_steps(&w), shape_ids(&w)), (steps, vec![1, 2]));
    assert_eq!(w.state().selection(), [1]);
    assert_eq!(w.state().options_dialog_tab(), Some("Customize Ribbon"), "and the dialog is still open");
    // Closed, the same keys act on the drawing again.
    run_command(&mut w, "window.options", json!({"close": true}));
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(shape_ids(&w), [2], "Delete deletes the shape selected once the dialog is gone");
}

/// Enter is OK while no control has the keyboard: it applies the Developer box.
#[test]
fn enter_applies_the_developer_box() {
    let dir = TempDir::new("ops-options-enter");
    let mut w = fast_window(DiacraftApp::new(open_in(&dir, "ops.vsdx", &two_squares()), None).with_inline_pictures(bundled_drawer()), WIDE);
    w.rest();
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    hit(&mut w, "options.tab.Developer");
    assert_eq!(w.state().options_dialog_shows_developer(), Some(true));
    assert!(w.state().element("tab.Developer").is_none(), "not yet");
    // The box was pressed with the pointer, which does not give it the keyboard: Enter is the dialog's.
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(w.state().options_dialog_tab().is_none(), "Enter closed the dialog");
    assert!(w.state().element("tab.Developer").is_some(), "and the Developer tab is on");
    // With the box unchanged Enter closes the dialog and sends nothing.
    let steps = w.state().session().revision();
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(w.state().options_dialog_tab().is_none() && w.state().element("tab.Developer").is_some());
    assert_eq!(w.state().session().revision(), steps);
}

/// The dialog's note says the choice is for this window only: it is not saved, and the next window starts with the tab off.
#[test]
fn the_options_dialog_says_the_developer_choice_is_for_this_window_only() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let dir = TempDir::new("ops-options-note");
    let mut w = fast_window(DiacraftApp::new(open_in(&dir, "ops.vsdx", &two_squares()), None).with_inline_pictures(bundled_drawer()), WIDE);
    w.rest();
    run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
    let words: Vec<String> = w
        .query_all(egui_kittest::kittest::By::new().role(egui::accesskit::Role::Label))
        .filter_map(|n| n.accesskit_node().value().map(|v| v.to_string()))
        .filter(|v| v.contains("Tick Developer"))
        .collect();
    assert_eq!(words.len(), 1, "{words:?}");
    assert!(words[0].contains("for this window only") && words[0].contains("not saved"), "{}", words[0]);
    assert!(!words[0].contains("off until you do"), "{}", words[0]);
}

// ---- narrow and wide --------------------------------------------------------------------------------------------------------------------------

/// The Developer tab with a selection and the menu open, and the Options dialog, fit at narrow and wide windows: no group lies over another, no control leaves the
/// window, and the dialog's parts are inside it and apart.
#[test]
fn the_developer_tab_and_the_options_dialog_fit_at_narrow_and_wide_windows() {
    for width in [480.0_f32, 560.0, 640.0, 800.0, 1024.0, 1280.0, 1700.0, 2400.0] {
        let dir = TempDir::new("ops-fit");
        let mut w = developer_window(&dir, &two_squares(), [width, 700.0]);
        run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
        let ribbon = crate::support::elements_named(&w, "ribbon.Developer.");
        let mut groups: Vec<_> = ribbon.iter().filter(|(n, ..)| n.starts_with("ribbon.Developer.group.")).cloned().collect();
        groups.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
        for pair in groups.windows(2) {
            assert!(pair[0].1.max.x <= pair[1].1.min.x + 0.5, "{} lies on {} at {width}", pair[0].0, pair[1].0);
        }
        for (name, rect, _) in &ribbon {
            assert!(rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        // The menu: its button is drawn in the group, or the group is folded to one button that opens a card with it; the menu opens from either (the
        // card's own tests are in `fold_card.rs`).
        let folded = w.state().element("ribbon.Developer.OperationsMenu").is_none();
        if folded {
            assert!(w.state().element("ribbon.Developer.fold.GroupShapeDesign").is_some(), "the Shape Design group is drawn, folded, at {width}");
            hit(&mut w, "ribbon.Developer.fold.GroupShapeDesign");
            assert!(
                w.state().element("ribbon.Developer.OperationsMenu").is_some_and(|e| e.enabled),
                "the card has the Operations button, working, at {width}"
            );
            let card = w.state().element("ribbon.Developer.OperationsMenu").unwrap().rect;
            assert!(card.min.x >= 0.0 && card.max.x <= width + 0.5, "the card is inside the window at {width}: {card:?}");
        }
        hit(&mut w, "ribbon.Developer.OperationsMenu");
        let rows: Vec<_> = crate::support::elements_named(&w, "menu.ShapesObject");
        assert_eq!(rows.len(), 9, "the nine rows are drawn at {width} (folded: {folded})");
        assert!(rows[..5].iter().all(|(_, _, on)| *on), "two shapes are selected: the five work at {width}");
        let mut rects: Vec<_> = rows.iter().map(|(n, r, _)| (n.clone(), *r)).collect();
        rects.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
        for pair in rects.windows(2) {
            assert!(pair[0].1.max.y <= pair[1].1.min.y + 0.5, "{} lies on {} at {width}", pair[0].0, pair[1].0);
        }
        for (name, rect, _) in &rows {
            assert!(rect.min.x >= 0.0 && rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        // A row runs from there: Fragment, which any selection of areas takes.
        hit(&mut w, &menu_row("Fragment"));
        assert_eq!(shape_ids(&w).len(), 3, "Fragment ran at {width} (folded: {folded})");
        key(&mut w, Key::Z, CTRL);
        assert_eq!(shape_ids(&w), [1, 2]);
        // The Options dialog.
        run_command(&mut w, "window.options", json!({"tab": "Customize Ribbon"}));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 700.0));
        let parts = crate::support::elements_named(&w, "options.");
        assert!(parts.len() >= 13, "the dialog is drawn at {width}: {} parts", parts.len());
        for (name, rect, _) in &parts {
            assert!(screen.contains_rect(*rect), "{name} is outside the window at {width}: {rect:?}");
        }
        let ok = w.state().element("options.ok").unwrap().rect;
        let cancel = w.state().element("options.cancel").unwrap().rect;
        assert!(ok.max.x <= cancel.min.x + 0.5, "OK and Cancel are side by side at {width}");
        let mut tabs: Vec<_> = parts.iter().filter(|(n, ..)| n.starts_with("options.tab.")).map(|(_, r, _)| *r).collect();
        tabs.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        for pair in tabs.windows(2) {
            assert!(pair[0].max.y <= pair[1].min.y + 0.5, "two tab rows overlap at {width}");
        }
        // The list of sections is beside the page where there is room for both, and left out where there is not.
        let listed = w.state().element("options.section.Customize Ribbon").is_some();
        assert_eq!(listed, width >= 560.0, "the list of sections at {width}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}
