//! The Operations menu of the Developer tab: Union, Combine, Fragment, Intersect, Subtract, Join, Trim and Offset run from the menu (Offset
//! through its dialog) on the shapes selected, and Fit Curve from the Draw tab's menu through its dialog, in the
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
use diacraft_ui_egui::ribbon::spec::{self, Kind};
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

/// A straight line from `(x0, y0)` to `(x1, y1)`: one open section with `NoFill` 1, in the box that has the two ends for corners.
fn stroke(id: u32, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> String {
    let (left, bottom) = (x0.min(x1), y0.min(y1));
    let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let geometry = format!(
        r#"<Section N="Geometry" IX="0">{}{}{}</Section>"#,
        cell("NoFill", "1"),
        row("MoveTo", 1, x0 - left, y0 - bottom),
        row("LineTo", 2, x1 - left, y1 - bottom)
    );
    shape(id, &[placed(left, bottom, w, h), geometry].concat())
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
    menu_enabled_at(w, "ribbon.Developer.OperationsMenu")
}

/// As [`menu_enabled`], for the Operations button called `button` (the Developer tab's, or the Draw tab's), which must be on the tab shown.
fn menu_enabled_at(w: &mut Window, button: &str) -> Vec<(&'static str, bool)> {
    hit(w, button);
    let rows = ["Union", "Combine", "Fragment", "Intersect", "Subtract", "Join", "Trim", "Offset", "UpdateAlignmentBox"]
        .map(|op| (op, w.state().element(&menu_row(op)).unwrap_or_else(|| panic!("{op} is not in the menu")).enabled));
    hit(w, button);
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

/// With nothing selected every entry is quiet and the Operations button is too; with one shape Fragment and Trim can run; with two all seven can. The two that are
/// not built (Update Alignment Box) stays drawn and quiet whatever is selected. Offset takes one shape, as Fragment and Trim do. The words of a quiet one are the engine's.
#[test]
fn an_entry_is_enabled_only_when_the_engine_says_its_operation_can_run() {
    let dir = TempDir::new("ops-greying");
    let mut w = wide(&dir, &two_squares());
    assert!(!w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled, "nothing is selected");
    for op in ["union", "combine", "fragment", "intersect", "subtract", "join", "trim", "offset", "offsetDialog", "fitCurve", "fitCurveDialog"] {
        assert_eq!(w.state().command_enabled(&format!("operations.{op}")), Err("nothing is selected".to_owned()), "{op}");
    }

    // One shape: the others need two; Fragment, Trim and Offset take one.
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
            ("Trim", true),
            ("Offset", true),
            ("UpdateAlignmentBox", false)
        ]
    );
    let words = w.state().command_enabled("operations.join").unwrap_err();
    assert!(words.contains("Join needs two shapes at least"), "{words}");
    let words = w.state().command_enabled("operations.union").unwrap_err();
    assert!(words.contains("Union needs two shapes at least"), "{words}");

    // Two: all eight; the one that is not built stays quiet.
    click_with(&mut w, B_ONLY, Modifiers::SHIFT);
    assert_eq!(
        menu_enabled(&mut w),
        [
            ("Union", true),
            ("Combine", true),
            ("Fragment", true),
            ("Intersect", true),
            ("Subtract", true),
            ("Join", true),
            ("Trim", true),
            ("Offset", true),
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
        menu_enabled(&mut w)[..7],
        [("Union", false), ("Combine", false), ("Fragment", false), ("Intersect", false), ("Subtract", true), ("Join", false), ("Trim", false)],
        "A, the locked shape, is first: Subtract does not delete it"
    );
    for op in ["union", "combine", "fragment", "intersect", "join", "trim"] {
        let words = w.state().command_enabled(&format!("operations.{op}")).unwrap_err();
        assert!(words.contains("LockDelete"), "{op}: {words}");
    }
    // The other way round A would be deleted by Subtract too.
    key(&mut w, Key::Escape, Modifiers::NONE);
    click_at(&mut w, B_ONLY.0, B_ONLY.1);
    click_with(&mut w, A_ONLY, Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [2, 1]);
    // Offset deletes nothing, so a lock on a shape's deletion does not refuse it: it is the one entry that can run, and the Operations button is enabled for it.
    assert!(
        w.state().element("ribbon.Developer.OperationsMenu").unwrap().enabled,
        "B first, and A, which is locked, would be deleted by every one but Offset"
    );
    assert!(w.state().command_enabled("operations.subtract").unwrap_err().contains("LockDelete"));
    assert_eq!(w.state().command_enabled("operations.offsetDialog"), Ok(()), "Offset keeps the originals");
    assert_eq!(w.state().command_enabled("operations.fitCurveDialog"), Ok(()), "and Fit Curve changes them in place");
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
    for op in ["union", "combine", "fragment", "intersect", "subtract", "offset", "offsetDialog", "fitCurve", "fitCurveDialog"] {
        let words = w.state().command_enabled(&format!("operations.{op}")).unwrap_err();
        assert!(words.contains("is a group"), "{op}: {words}");
    }
    // The button's tooltip is the first of them: a person who cannot open the menu is told why.
}

// ---- Join and Trim ----------------------------------------------------------------------------------------------------------------------------

/// Join and Trim run from the Operations menu of the Developer tab and from the Draw tab's, on the shapes selected in the order given, each as one undo
/// step, with the originals gone, nothing selected after, and what their vectors say: Join of three lines is one shape of one open section of four points; Trim of a
/// rectangle and a line across it is five shapes with the rectangle's formatting.
#[test]
fn join_and_trim_run_from_both_menus_on_the_shapes_selected_and_make_what_their_vectors_say() {
    for from_draw in [false, true] {
        for (op, command, shapes, ids, made) in [
            (
                "Join",
                "shape.join",
                [stroke(1, (3.0, 3.0), (5.0, 3.0)), stroke(2, (5.0, 3.0), (5.0, 5.0)), stroke(3, (3.0, 5.0), (5.0, 5.0))].concat(),
                vec![1, 2, 3],
                1,
            ),
            ("Trim", "shape.trim", [painted(1, (3.0, 3.0, 2.0, 2.0), RED, ""), stroke(2, (4.0, 2.0), (4.0, 6.0))].concat(), vec![1, 2], 5),
        ] {
            let dir = TempDir::new("ops-join-trim");
            let mut w = if from_draw { draw_window(&dir, &shapes, WIDE) } else { wide(&dir, &shapes) };
            run_command(&mut w, "edit.select", json!({"ids": ids}));
            let steps = undo_steps(&w);
            if from_draw {
                hit(&mut w, "ribbon.Draw.DrawShapeOperations");
                hit(&mut w, &menu_row(op));
            } else {
                run_from_menu(&mut w, op);
            }
            assert!(w.state().notice().is_none(), "{op}: {:?}", w.state().notice());
            assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "{op} is one undo step");
            assert!(w.state().selection().is_empty(), "{op}: nothing stays selected");
            assert_eq!(
                journal_ids(&w, command),
                ids.iter().map(|id| u64::from(*id)).collect::<Vec<_>>(),
                "{op} sends the shapes in the order they were selected"
            );
            let now = shape_ids(&w);
            assert_eq!(now.len(), made, "{op}: {now:?}");
            let highest = ids.iter().max().copied().unwrap_or(0);
            assert!(now.iter().all(|id| *id > highest), "{op}: the originals are gone and new shapes are made: {now:?}");
            if op == "Join" {
                assert_eq!(geometry_sections(&w, now[0]), 1, "one section, not one for each line");
                assert_eq!(rows_of(&w, now[0]), 4, "a move and three lines through four points");
                assert!(near(frame_of(&w, now[0]), (3.0, 3.0, 2.0, 2.0)), "{:?}", frame_of(&w, now[0]));
            } else {
                assert_eq!(fill_of(&w, now[0]), Some(Rgb(255, 0, 0)), "the rectangle's formatting");
            }
            // Ctrl+Z brings the originals back.
            key(&mut w, Key::Z, CTRL);
            assert_eq!(shape_ids(&w), ids, "{op}");
            assert_eq!(w.state().ui_panics(), 0, "{op}");
        }
    }
}

/// Trim of two lines that only meet end to end cuts nothing: it makes two shapes, which is a result and says nothing in the status bar, and is one undo step.
#[test]
fn trim_of_two_lines_that_meet_end_to_end_cuts_nothing_and_makes_two_shapes() {
    let dir = TempDir::new("ops-trim-ends");
    let mut w = wide(&dir, &[stroke(1, (3.0, 3.0), (5.0, 3.0)), stroke(2, (5.0, 3.0), (5.0, 5.0))].concat());
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    let steps = undo_steps(&w);
    run_from_menu(&mut w, "Trim");
    assert!(w.state().notice().is_none(), "{:?}", w.state().notice());
    assert_eq!(shape_ids(&w).len(), 2);
    assert_eq!(undo_steps(&w), (steps.0 + 1, 0));
}

// ---- the entries that are not built ----------------------------------------------------------------------------------------------------------

/// Update Alignment Box is drawn in the menu and quiet, and a click on it does nothing; hovering it says it is not available yet, in the words a quiet button of the ribbon
/// uses (one wording for a control that is not built, `widgets::tip`).
#[test]
fn the_entry_that_is_not_built_stays_quiet_and_says_so_as_a_quiet_button_does() {
    let dir = TempDir::new("ops-later");
    let mut w = wide(&dir, &two_squares());
    select_a_then_b(&mut w);
    let steps = undo_steps(&w);
    let op = "UpdateAlignmentBox";
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    let row = w.state().element(&menu_row(op)).unwrap_or_else(|| panic!("{op} is drawn")).clone();
    assert!(!row.enabled, "{op}");
    click(&mut w, row.rect.center());
    w.rest();
    assert!(w.state().element(&menu_row(op)).is_none(), "{op}: a click closes the menu, as a click on any row does");
    assert!(w.state().notice().is_none(), "{op}: and says nothing");
    assert_eq!((undo_steps(&w), shape_ids(&w)), (steps, vec![1, 2]), "nothing was done");
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    assert_eq!(hovered_words(&mut w, &menu_row(op)), ["Update Alignment Box (not available yet)"], "{op}");
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    // A quiet button of the ribbon with no command says it in the same words.
    assert_eq!(hovered_words(&mut w, "ribbon.Developer.MacroSecurity"), ["Macro Security (not available yet)"]);
}

/// A quiet entry of the seven says why, in the engine's words, in its tooltip; one that works has none.
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
    // Opening the menu asks about the seven others once (Fit Curve is not in this menu), and closing and opening it again asks nothing.
    menu_enabled(&mut w);
    let all = work::work().shape_op_checks;
    assert_eq!(all, 8, "one question for each operation of the menu, Union's kept from the button");
    menu_enabled(&mut w);
    frames(&mut w, 10);
    assert_eq!(work::work().shape_op_checks, 8, "the answers are kept");
    // Another selection is a new question; so is an edit of the drawing.
    key(&mut w, Key::Escape, Modifiers::NONE);
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    let one = work::work().shape_op_checks;
    assert!(one > 8, "a selection of one shape is asked about: {one}");
    // **And its answers are kept in turn**: thirty frames of the second selection ask nothing (an entry that held the new answers under the old selection would ask
    // in every frame, up to seven times).
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
        assert!(rows[..7].iter().all(|(_, _, on)| *on), "two shapes are selected: the seven work at {width}");
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

// ---- the Draw tab's copy of the menu ---------------------------------------------------------------------------------------------

/// The group of `tab` that holds the top-level control `id`, and the control.
fn top_control(tab: &str, id: &str) -> (&'static spec::Group, &'static spec::Ctl) {
    let tab = spec::tab_by_label(tab).unwrap_or_else(|| panic!("no tab {tab}"));
    tab.groups
        .iter()
        .find_map(|group| group.top_level().into_iter().find(|c| c.id == id).map(|c| (group, c)))
        .unwrap_or_else(|| panic!("{} has no control {id}", tab.label))
}

/// A window of `size` on the Draw tab, at 100%, with the Developer tab left off (a person who has not turned it on).
fn draw_window(dir: &TempDir, shapes: &str, size: [f32; 2]) -> Window {
    let mut w = fast_window(DiacraftApp::new(open_in(dir, "ops.vsdx", shapes), None).with_inline_pictures(bundled_drawer()), size);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    run_command(&mut w, "window.tab", json!({"tab": "Draw"}));
    w
}

/// The owner's extra (2026-11-25): the shape operations are also on the Draw tab, in a group of their own after Create and Convert, as a menu of the very items of
/// the Developer tab's Operations, which keeps its place and its id.
#[test]
fn the_draw_tab_has_an_operations_menu_with_the_items_of_the_developer_tabs() {
    let (developer_group, developer) = top_control("Developer", "OperationsMenu");
    let (group, draw) = top_control("Draw", "DrawShapeOperations");
    assert_eq!(developer_group.id, "GroupShapeDesign", "the Developer tab keeps its entry where it was");
    assert_eq!((developer.kind, developer.label, developer.confirmed), (Kind::Menu, "Operations", true));
    assert_eq!((draw.kind, draw.label), (Kind::Menu, "Operations"));
    assert_ne!(draw.id, developer.id, "the Draw tab's button has an id of its own");

    // The same items: the ids, the captions, the commands and the parameters they run with.
    // **Fit Curve is on the Draw tab's menu only**: the reference app's ribbon lists `FitCurveDialog` among the commands that are not on it, so the
    // Developer tab's menu keeps the nine items its map lists, and the tab that is ours has the tenth, after Offset.
    let shape = |c: &spec::Ctl| (c.id, c.label, c.kind, c.command, c.args);
    let drawn_without_fit: Vec<_> = draw.items.iter().filter(|c| c.id != "FitCurveDialog").map(shape).collect();
    let wanted: Vec<_> = developer.items.iter().map(shape).collect();
    assert_eq!(drawn_without_fit, wanted);
    assert_eq!(wanted.len(), 9);
    assert_eq!(draw.items.len(), 10);
    assert_eq!(draw.items.iter().position(|c| c.id == "FitCurveDialog"), Some(8), "after Offset, before Update Alignment Box");
    assert!(developer.items.iter().all(|c| c.id != "FitCurveDialog" && c.command != Some("operations.fitCurveDialog")));
    assert_eq!(
        draw.items.iter().filter_map(|c| c.command).collect::<Vec<_>>(),
        [
            "operations.union",
            "operations.combine",
            "operations.fragment",
            "operations.intersect",
            "operations.subtract",
            "operations.join",
            "operations.trim",
            "operations.offsetDialog",
            "operations.fitCurveDialog"
        ]
    );

    // After Create and Convert, in a group of its own.
    let ids: Vec<&str> = spec::tab_by_label("Draw").unwrap().groups.iter().map(|g| g.id).collect();
    assert_eq!(ids, ["GroupWrite", "GroupPens", "GroupDrawCreate", "GroupDrawConvert", "GroupDrawShapeOperations"]);
    assert_eq!((group.id, group.label), ("GroupDrawShapeOperations", "Shape Operations"));
    assert!(!group.confirmed, "the group is ours: the map names no such group");
    assert_eq!(group.top_level().len(), 1, "the Operations menu is the group's one control");
}

/// The Draw tab's button and its nine rows in common with the Developer tab's are enabled exactly when the Developer tab's are, for every selection: nothing, one shape, two, a locked shape first
/// (Subtract and Offset), a locked shape second (Offset alone) and a group.
#[test]
fn the_draw_tabs_operations_menu_is_enabled_as_the_developer_tabs_is() {
    let locked = [painted(1, (3.0, 3.0, 2.0, 2.0), RED, &cell("LockDelete", "1")), painted(2, (4.0, 4.0, 2.0, 2.0), BLUE, "")].concat();
    let group = fixture::group(10, &placed(7.0, 3.0, 2.0, 2.0), "", &fixture::boxed(11, 0.0, 0.0, 2.0, 2.0));
    let with_group = [two_squares(), group].concat();
    let squares = two_squares();
    let scenarios: [(&str, &str, &[u32]); 6] = [
        ("nothing", &squares, &[]),
        ("A", &squares, &[1]),
        ("A, B", &squares, &[1, 2]),
        ("a locked A, B", &locked, &[1, 2]),
        ("B, a locked A", &locked, &[2, 1]),
        ("A, a group", &with_group, &[1, 10]),
    ];
    let mut seen = Vec::new();
    for (name, shapes, ids) in scenarios {
        let dir = TempDir::new("ops-draw-states");
        let mut w = developer_window(&dir, shapes, WIDE);
        run_command(&mut w, "edit.select", json!({"ids": ids}));
        let mut state = |tab: &str, button: &str| {
            run_command(&mut w, "window.tab", json!({"tab": tab}));
            let button = format!("ribbon.{tab}.{button}");
            let on = w.state().element(&button).unwrap_or_else(|| panic!("{button} is not drawn at {WIDE:?}")).enabled;
            // A quiet button does not open its menu: there are no rows to read.
            (on, if on { menu_enabled_at(&mut w, &button) } else { Vec::new() })
        };
        let (developer, draw) = (state("Developer", "OperationsMenu"), state("Draw", "DrawShapeOperations"));
        assert_eq!(draw, developer, "{name}");
        seen.push((name, draw.0, draw.1.iter().filter(|row| row.1).count()));
        assert_eq!(w.state().ui_panics(), 0, "{name}");
    }
    // What those states are, so that two empty answers do not agree by chance.
    assert_eq!(
        seen,
        [("nothing", false, 0), ("A", true, 3), ("A, B", true, 8), ("a locked A, B", true, 2), ("B, a locked A", true, 1), ("A, a group", false, 0)]
    );
}

/// Each of the five runs from the Draw tab, with the Developer tab never turned on, on the shapes selected in click order, as one undo step, and makes what it
/// makes from the Developer tab: the same commands, the same results.
#[test]
fn each_of_the_five_runs_from_the_draw_tab() {
    for (op, command, made) in [
        ("Union", "shape.union", 1),
        ("Combine", "shape.combine", 1),
        ("Fragment", "shape.fragment", 3),
        ("Intersect", "shape.intersect", 1),
        ("Subtract", "shape.subtract", 1),
    ] {
        let dir = TempDir::new("ops-draw-five");
        let mut w = draw_window(&dir, &two_squares(), WIDE);
        assert!(w.state().element("tab.Developer").is_none(), "the Developer tab is off: {op} is reached without it");
        select_a_then_b(&mut w);
        let steps = undo_steps(&w);
        hit(&mut w, "ribbon.Draw.DrawShapeOperations");
        hit(&mut w, &menu_row(op));
        assert!(w.state().notice().is_none(), "{op}: {:?}", w.state().notice());
        assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "{op} is one undo step");
        assert!(w.state().selection().is_empty(), "{op}: nothing stays selected");
        assert_eq!(journal_ids(&w, command), [1, 2], "{op} sends the shapes in the order they were selected");
        let ids = shape_ids(&w);
        assert_eq!(ids.len(), made, "{op}");
        if op == "Union" {
            assert!(near(frame_of(&w, ids[0]), (3.0, 3.0, 3.0, 3.0)), "{:?}", frame_of(&w, ids[0]));
            assert_eq!(fill_of(&w, ids[0]), Some(Rgb(255, 0, 0)), "the first shape's formatting");
        }
        // Ctrl+Z brings the originals back.
        key(&mut w, Key::Z, CTRL);
        assert_eq!(shape_ids(&w), [1, 2], "{op}");
        assert_eq!(w.state().ui_panics(), 0, "{op}");
    }
}

/// The row that is not built is as quiet on the Draw tab as on the Developer tab, with the same words on hover, and a click on it runs nothing.
#[test]
fn the_row_that_is_not_built_is_quiet_on_the_draw_tab_too() {
    let dir = TempDir::new("ops-draw-later");
    let mut w = draw_window(&dir, &two_squares(), WIDE);
    select_a_then_b(&mut w);
    let steps = undo_steps(&w);
    let op = "UpdateAlignmentBox";
    hit(&mut w, "ribbon.Draw.DrawShapeOperations");
    assert_eq!(hovered_words(&mut w, &menu_row(op)), ["Update Alignment Box (not available yet)"], "{op}");
    let at = w.state().element(&menu_row(op)).unwrap().rect.center();
    click(&mut w, at);
    w.rest();
    assert!(w.state().element(&menu_row(op)).is_none(), "{op}: a click closes the menu");
    assert_eq!((undo_steps(&w), shape_ids(&w)), (steps, vec![1, 2]), "nothing was done");
}

/// The Draw tab with its new group and the menu open fits at narrow and wide windows, as the Developer tab does: the five groups do not lie on one another and
/// none leaves the window; the group is whole in every window the app allows (480 points and more) and folds to one button, first of the groups since it is the
/// last, in a narrower one (320, which the card's path needs); the menu opens from the button or from the card of the folded group, with nine rows inside the
/// window, and a row runs from there.
#[test]
fn the_draw_tab_and_its_operations_menu_fit_at_narrow_and_wide_windows() {
    for width in [320.0_f32, 360.0, 400.0, 440.0, 480.0, 560.0, 640.0, 800.0, 1024.0, 1280.0, 1700.0, 2400.0] {
        let dir = TempDir::new("ops-draw-fit");
        let mut w = draw_window(&dir, &two_squares(), [width, 700.0]);
        run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
        let ribbon = crate::support::elements_named(&w, "ribbon.Draw.");
        let mut groups: Vec<_> = ribbon.iter().filter(|(n, ..)| n.starts_with("ribbon.Draw.group.")).cloned().collect();
        assert_eq!(groups.len(), 5, "the five groups are drawn at {width}: {groups:?}");
        groups.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
        for pair in groups.windows(2) {
            assert!(pair[0].1.max.x <= pair[1].1.min.x + 0.5, "{} lies on {} at {width}", pair[0].0, pair[1].0);
        }
        assert_eq!(groups.last().map(|g| g.0.as_str()), Some("ribbon.Draw.group.Shape Operations"), "the new group is last at {width}");
        for (name, rect, _) in &ribbon {
            assert!(rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        // The menu: its button is in the group, or the group is folded to one button that opens a card with it.
        let folded = w.state().element("ribbon.Draw.DrawShapeOperations").is_none();
        if folded {
            assert!(w.state().element("ribbon.Draw.fold.GroupDrawShapeOperations").is_some(), "the group is drawn, folded, at {width}");
            hit(&mut w, "ribbon.Draw.fold.GroupDrawShapeOperations");
            let card =
                w.state().element("ribbon.Draw.DrawShapeOperations").unwrap_or_else(|| panic!("the card has no Operations at {width}")).clone();
            assert!(card.enabled, "the card has the Operations button, working, at {width}");
            assert!(card.rect.min.x >= 0.0 && card.rect.max.x <= width + 0.5, "the card is inside the window at {width}: {:?}", card.rect);
        }
        // The app's smallest window is 480 points wide (`MIN_WINDOW_SIZE`), where the whole tab fits with the group as it is; 320 is below it, where the group
        // folds (it is the last, so it folds first), so that the card is tried too.
        assert!(!folded || width < 480.0, "the group is folded at {width}, in a window the app allows");
        assert!(folded || width > 320.0, "the group is not folded at {width}, where the card is to be tried");
        hit(&mut w, "ribbon.Draw.DrawShapeOperations");
        let rows: Vec<_> = crate::support::elements_named(&w, "menu.ShapesObject");
        assert_eq!(rows.len(), 9, "the nine rows are drawn at {width} (folded: {folded})");
        assert!(rows[..7].iter().all(|row| row.2), "two shapes are selected: the seven work at {width}");
        let mut rects: Vec<_> = rows.iter().map(|(n, r, _)| (n.clone(), *r)).collect();
        rects.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
        for pair in rects.windows(2) {
            assert!(pair[0].1.max.y <= pair[1].1.min.y + 0.5, "{} lies on {} at {width}", pair[0].0, pair[1].0);
        }
        for (name, rect, _) in &rows {
            assert!(rect.min.x >= 0.0 && rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        hit(&mut w, &menu_row("Fragment"));
        assert_eq!(shape_ids(&w).len(), 3, "Fragment ran at {width} (folded: {folded})");
        key(&mut w, Key::Z, CTRL);
        assert_eq!(shape_ids(&w), [1, 2]);
        assert_eq!(w.state().ui_panics(), 0, "{width}");
    }
}

// ---- Offset and Fit Curve --------------------------------------------------------------------------------------------------------

/// A closed polygon of 36 sides on a circle of radius 1 about (`cx`, `cy`): one closed section of `LineTo` rows, in the box that holds the circle.
fn polygon_circle(id: u32, (cx, cy): (f64, f64)) -> String {
    let row =
        |ix: u32, t: &str, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let mut rows = String::new();
    for i in 0..=36_u32 {
        let a = f64::from(i % 36) * 10.0_f64.to_radians();
        rows.push_str(&row(i + 1, if i == 0 { "MoveTo" } else { "LineTo" }, 1.0 + a.cos(), 1.0 + a.sin()));
    }
    let geometry = format!(r#"<Section N="Geometry" IX="0">{}{}</Section>"#, cell("NoFill", "0"), rows);
    shape(id, &[placed(cx - 1.0, cy - 1.0, 2.0, 2.0), geometry].concat())
}

fn dialog_state(w: &Window, key: &str) -> serde_json::Value {
    w.state().window_state()[key].clone()
}

/// Types `text` into the box called `name` of a dialog, replacing what it holds.
fn type_into(w: &mut Window, name: &str, text: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
    key(w, Key::A, CTRL);
    type_text(w, text);
}

/// Offset's row opens its dialog with the distance of R40 (0.25 inch) in the box, nothing is sent until OK, and OK makes the two copies of the shape selected, keeps the
/// original, leaves nothing selected and is one undo step; Cancel and Escape change nothing.
#[test]
fn offset_opens_its_dialog_with_a_quarter_of_an_inch_and_ok_makes_the_two_copies_in_one_undo_step() {
    let dir = TempDir::new("ops-offset");
    let mut w = wide(&dir, &two_squares());
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    assert!(dialog_state(&w, "offsetDialog").is_null());
    run_from_menu(&mut w, "Offset");
    assert_eq!(dialog_state(&w, "offsetDialog"), "0.25 in", "0.25 inch");
    for name in ["offset.distance", "offset.ok", "offset.cancel"] {
        assert!(w.state().element(name).is_some_and(|e| e.enabled), "{name}");
    }
    assert_eq!((w.state().session().revision(), undo_steps(&w)), (revision, steps), "nothing is sent before OK");
    // Cancel and Escape change nothing.
    hit(&mut w, "offset.cancel");
    assert!(dialog_state(&w, "offsetDialog").is_null());
    run_from_menu(&mut w, "Offset");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(dialog_state(&w, "offsetDialog").is_null(), "Escape is Cancel");
    assert_eq!((w.state().session().revision(), undo_steps(&w), shape_ids(&w)), (revision, steps, vec![1, 2]));
    assert_eq!(w.state().selection(), [1], "and the shape is still selected");
    // OK: the original stays and its two copies are in front of the drawing.
    run_from_menu(&mut w, "Offset");
    hit(&mut w, "offset.ok");
    assert!(dialog_state(&w, "offsetDialog").is_null(), "OK closes it");
    assert_eq!(shape_ids(&w).len(), 4, "the two squares and the two copies of A: {:?}", shape_ids(&w));
    assert_eq!(&shape_ids(&w)[..2], [1, 2], "the originals stay in their places");
    assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "one undo step");
    assert!(w.state().selection().is_empty(), "nothing is selected after it");
    let d = w.state().session().document().unwrap();
    let copies: Vec<(u32, f64)> = shape_ids(&w)[2..].iter().map(|id| (*id, num_of(d, *id, "Width"))).collect();
    assert!(
        (copies[0].1 - 2.5).abs() < 1e-6 && (copies[1].1 - 1.5).abs() < 1e-6,
        "A is 2 across, grown and shrunk by a quarter on each side: {copies:?}"
    );
    // Ctrl+Z brings the drawing back.
    key(&mut w, Key::Z, CTRL);
    assert_eq!(shape_ids(&w), [1, 2]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// The number of the cell `name` of shape `id` of the first page.
fn num_of(d: &diacraft_doc::Document, id: u32, name: &str) -> f64 {
    let at = d.find_shape(diacraft_doc::Owner::Page(1), id).expect("the shape");
    d.shape(&at).and_then(|s| s.sheet.cell(name)).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(f64::NAN)
}

/// A typed distance is read in the page's unit, a refusal is the engine's or the measure's words in the dialog, which stays open and changes nothing, and typing again
/// clears them.
#[test]
fn offsets_box_takes_a_length_and_refuses_the_rest_in_words_and_the_dialog_stays_open() {
    let dir = TempDir::new("ops-offset-words");
    let mut w = wide(&dir, &two_squares());
    click_at(&mut w, A_ONLY.0, A_ONLY.1);
    run_from_menu(&mut w, "Offset");
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    for (typed, why) in [
        ("0", "distance above 0"),
        ("-1", "distance above 0"),
        ("0.00001", "at least 0.0001"),
        ("abc", "length"),
        ("", "length"),
        ("1,5", "comma"),
        ("1e999", "length"),
    ] {
        type_into(&mut w, "offset.distance", typed);
        hit(&mut w, "offset.ok");
        assert!(!dialog_state(&w, "offsetDialog").is_null(), "{typed:?}: the dialog stays open");
        let words = w.state().element("offset.error").unwrap_or_else(|| panic!("{typed:?}: the refusal is said")).clone();
        assert!(words.enabled);
        assert_eq!((w.state().session().revision(), undo_steps(&w)), (revision, steps), "{typed:?}: nothing changed");
        let _ = why;
    }
    // A length with a unit is read as it says: a foot is twelve inches, 0.5 in grows the square by half an inch.
    type_into(&mut w, "offset.distance", "0.5 in");
    hit(&mut w, "offset.ok");
    assert!(dialog_state(&w, "offsetDialog").is_null());
    let d = w.state().session().document().unwrap();
    let copies: Vec<f64> = shape_ids(&w)[2..].iter().map(|id| num_of(d, *id, "Width")).collect();
    assert!((copies[0] - 3.0).abs() < 1e-6 && (copies[1] - 1.0).abs() < 1e-6, "{copies:?}");
}

/// On a metric page the dialog starts with 6 mm, and a number with no unit is in millimetres.
#[test]
fn on_a_metric_page_the_distance_starts_at_six_millimetres_and_a_bare_number_is_in_millimetres() {
    let dir = TempDir::new("ops-offset-metric");
    let mut w = wide(&dir, &two_squares());
    run_command(&mut w, "page.scale", json!({"preset": "metric_1_100"}));
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    run_from_menu(&mut w, "Offset");
    assert_eq!(dialog_state(&w, "offsetDialog"), "6 mm", "6 mm on a metric page");
    // A number with no unit is in the page's unit, millimetres: 12 grows the square by 12 mm a side on the drawing, which at 1 to 100 is 0.12 mm of paper (a hundredth).
    type_into(&mut w, "offset.distance", "12");
    hit(&mut w, "offset.ok");
    assert!(dialog_state(&w, "offsetDialog").is_null());
    let d = w.state().session().document().unwrap();
    let copies: Vec<f64> = shape_ids(&w)[2..].iter().map(|id| num_of(d, *id, "Width")).collect();
    // 12 mm of drawing is 12/25.4 inches at the drawing's scale, over 100 on paper: 0.0047 inch each side of a square two inches across.
    let side = 12.0 / 25.4 / 100.0;
    assert!((copies[0] - (2.0 + 2.0 * side)).abs() < 1e-6, "{copies:?} {side}");
}

/// Fit Curve is in the Draw tab's Operations menu (and not in the Developer tab's), opens a dialog with a tolerance and three boxes, and OK fits the shape selected in place
/// with the flags the boxes say: a polygon of 36 sides becomes arcs with Circular arcs ticked and cubics without, as one undo step, the shape keeping its id.
#[test]
fn fit_curve_opens_its_dialog_from_the_draw_tabs_menu_and_ok_fits_the_shape_selected_in_place() {
    let dir = TempDir::new("ops-fit");
    let mut w = draw_window(&dir, &polygon_circle(1, (5.0, 5.0)), WIDE);
    click_at(&mut w, 5.0, 6.0);
    assert_eq!(w.state().selection(), [1]);
    let rows_of_shape = |w: &Window| -> Vec<String> {
        let d = w.state().session().document().unwrap();
        let at = d.find_shape(diacraft_doc::Owner::Page(1), 1).unwrap();
        d.shape(&at)
            .unwrap()
            .sheet
            .sections
            .iter()
            .filter(|s| s.name.as_str() == "Geometry" && !s.del)
            .flat_map(|s| s.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()).collect::<Vec<_>>())
            .collect()
    };
    assert_eq!(rows_of_shape(&w).len(), 37);
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    hit(&mut w, "ribbon.Draw.DrawShapeOperations");
    hit(&mut w, "menu.FitCurveDialog");
    let state = dialog_state(&w, "fitCurveDialog");
    assert_eq!(state["flags"], 0, "no box ticked");
    assert_eq!(state["tolerance"], "0.01 in", "a hundredth of an inch");
    for name in ["fit.tolerance", "fit.periodic", "fit.circles", "fit.abrupt", "fit.ok", "fit.cancel"] {
        assert!(w.state().element(name).is_some_and(|e| e.enabled), "{name}");
    }
    assert_eq!((w.state().session().revision(), undo_steps(&w)), (revision, steps), "nothing is sent before OK");
    // Cancel changes nothing.
    hit(&mut w, "fit.cancel");
    assert!(dialog_state(&w, "fitCurveDialog").is_null());
    assert_eq!(w.state().session().revision(), revision);
    // The boxes say the flags: Circular arcs is 2, and with Periodic splines 3.
    hit(&mut w, "ribbon.Draw.DrawShapeOperations");
    hit(&mut w, "menu.FitCurveDialog");
    hit(&mut w, "fit.circles");
    assert_eq!(dialog_state(&w, "fitCurveDialog")["flags"], 2);
    hit(&mut w, "fit.periodic");
    hit(&mut w, "fit.abrupt");
    assert_eq!(dialog_state(&w, "fitCurveDialog")["flags"], 7);
    hit(&mut w, "fit.periodic");
    hit(&mut w, "fit.abrupt");
    assert_eq!(dialog_state(&w, "fitCurveDialog")["flags"], 2);
    type_into(&mut w, "fit.tolerance", "0.02 in");
    hit(&mut w, "fit.ok");
    assert!(dialog_state(&w, "fitCurveDialog").is_null(), "OK closes it");
    let kinds = rows_of_shape(&w);
    assert!(kinds.len() < 37 && kinds.iter().any(|k| k == "ArcTo"), "arcs, and fewer rows: {kinds:?}");
    assert_eq!(shape_ids(&w), [1], "the same shape");
    assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "one undo step");
    assert!(w.state().selection().is_empty(), "nothing is selected after it");
    key(&mut w, Key::Z, CTRL);
    assert_eq!(rows_of_shape(&w).len(), 37, "undo brings the sides back");
    // Without the box the same fit is splines.
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    hit(&mut w, "ribbon.Draw.DrawShapeOperations");
    hit(&mut w, "menu.FitCurveDialog");
    type_into(&mut w, "fit.tolerance", "0.02 in");
    hit(&mut w, "fit.ok");
    let kinds = rows_of_shape(&w);
    assert!(kinds.iter().any(|k| k == "NURBSTo") && !kinds.iter().any(|k| k == "ArcTo"), "splines and no arcs: {kinds:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Fit Curve is not a row of the Developer tab's Operations menu (the reference app's ribbon lists it among the commands that are not on it), and its commands are in the
/// window's registry with the parameters the dialog sends: `operations.fitCurve {tolerance, flags}` and `operations.offset {distance}` run without a dialog, and the dialog
/// commands open and close one.
#[test]
fn the_commands_of_the_two_dialogs_run_without_a_dialog_and_open_and_close_one() {
    let dir = TempDir::new("ops-dialog-commands");
    let mut w = draw_window(&dir, &polygon_circle(1, (5.0, 5.0)), WIDE);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    let developer = spec::tab_by_label("Developer").unwrap();
    assert!(developer.groups.iter().flat_map(|g| g.top_level()).flat_map(|c| c.items.iter()).all(|c| c.id != "FitCurveDialog"));
    // The dialog commands open and close, and `close` needs no selection.
    run_command(&mut w, "operations.offsetDialog", json!({}));
    assert!(!dialog_state(&w, "offsetDialog").is_null());
    run_command(&mut w, "operations.offsetDialog", json!({"close": true}));
    assert!(dialog_state(&w, "offsetDialog").is_null());
    run_command(&mut w, "operations.fitCurveDialog", json!({}));
    assert!(!dialog_state(&w, "fitCurveDialog").is_null());
    run_command(&mut w, "operations.fitCurveDialog", json!({"close": true}));
    assert!(dialog_state(&w, "fitCurveDialog").is_null());
    // They are refused with no shape selected, in the engine's words.
    run_command(&mut w, "edit.select", json!({"ids": []}));
    let nothing = w.state_mut().execute("operations.offsetDialog", &json!({})).unwrap_err().to_string();
    assert!(nothing.contains("nothing is selected"), "{nothing}");
    // The commands that do the work take the numbers.
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    let missing = w.state_mut().execute("operations.offset", &json!({})).unwrap_err().to_string();
    assert!(missing.contains("distance"), "{missing}");
    let missing = w.state_mut().execute("operations.fitCurve", &json!({})).unwrap_err().to_string();
    assert!(missing.contains("tolerance"), "{missing}");
    let bad = w.state_mut().execute("operations.fitCurve", &json!({"tolerance": 0.02, "flags": 9})).unwrap_err().to_string();
    assert!(bad.contains("flags"), "{bad}");
    run_command(&mut w, "operations.fitCurve", json!({"tolerance": 0.02, "flags": 2}));
    assert!(shape_ids(&w) == [1] && w.state().selection().is_empty());
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    run_command(&mut w, "operations.offset", json!({"distance": 0.5}));
    assert_eq!(shape_ids(&w).len(), 3, "the shape and its two copies");
    assert_eq!(w.state().ui_panics(), 0);
}
