//! The View tab's visible Snap controls, the Zoom dialog, and the status bar's Snap mark. Nothing here is the engine's: every control is a press on what the
//! window drew, and what is checked is the document's own `snap.settings` (or the window's `zoomPercent`), the journal, and where the controls stand.

use diacraft_ui_egui::DiacraftApp;
use egui::accesskit::{Role, Toggled};
use egui::{Key, Modifiers, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{edit_window, key, type_text};
use crate::support::{Rest, Window, click, drawing_window, elements_named, press, run_command, window};

fn settings(w: &mut Window) -> Value {
    w.state_mut().execute("snap.settings", &json!({})).unwrap()
}

fn journal(w: &Window) -> Vec<(String, Value)> {
    w.state().session().journal().to_vec()
}

fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_label(label).accesskit_node().toggled()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn zoom_percent(w: &Window) -> f64 {
    w.state().window_state()["zoomPercent"].as_f64().expect("a zoom")
}

fn dialog(w: &Window) -> Value {
    w.state().window_state()["zoomDialog"].clone()
}

// ---------------------------------------------------------------------------------------------------------- the ribbon's Snap boxes

/// The Visual Aids group has a Snap box and a Snap to Grid box (DiaCraft's own, beside the dialog): each shows what `snap.settings` has, and a press is one
/// `snap.set` with the option the other way round, no undo step (a View setting is "unsaved, not undo").
#[test]
fn the_visual_aids_group_has_snap_and_snap_to_grid_boxes_that_follow_the_document() {
    let mut w = edit_window([1600.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    for name in ["ribbon.View.SnapToggle", "ribbon.View.SnapToGrid"] {
        let e = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).clone();
        assert!(e.enabled, "{name} works with a drawing open");
    }
    assert_eq!(toggled(&w, "Snap"), Some(Toggled::True), "a new drawing snaps");
    assert_eq!(toggled(&w, "Snap to Grid"), Some(Toggled::True), "and snaps to the grid");

    let entries = journal(&w).len();
    press(&mut w, "ribbon.View.SnapToggle");
    w.rest();
    assert_eq!(settings(&mut w)["snap"], false, "the press turned the switch off in the document");
    assert_eq!(settings(&mut w)["grid"], true, "and touched nothing else");
    assert_eq!(toggled(&w, "Snap"), Some(Toggled::False), "the box follows");
    assert_eq!(journal(&w).len(), entries + 1);
    assert_eq!(journal(&w).last().unwrap(), &("snap.set".to_owned(), json!({"snap": false})));
    press(&mut w, "ribbon.View.SnapToggle");
    w.rest();
    assert_eq!(settings(&mut w)["snap"], true);
    assert_eq!(toggled(&w, "Snap"), Some(Toggled::True));
    press(&mut w, "ribbon.View.SnapToggle");
    w.rest();
    assert_eq!(settings(&mut w)["snap"], false);
    assert_eq!(w.state().session().history().unwrap().undo_len(), 0, "a press is no undo step");
    assert!(w.state().is_dirty(), "the drawing is unsaved");
    assert!(w.state_mut().execute("edit.undo", &json!({})).is_err(), "nothing to undo");
    assert_eq!(settings(&mut w)["snap"], false, "and the box stays as it was turned");
    assert_eq!(toggled(&w, "Snap"), Some(Toggled::False));
    // A program's change shows too.
    run_command(&mut w, "snap.set", json!({"snap": true}));
    assert_eq!(toggled(&w, "Snap"), Some(Toggled::True));

    // Snap to Grid is the dialog's Grid box: its own option, `grid`.
    press(&mut w, "ribbon.View.SnapToGrid");
    w.rest();
    assert_eq!(settings(&mut w)["grid"], false);
    assert_eq!(settings(&mut w)["snap"], true, "the master switch is not the grid's");
    assert_eq!(toggled(&w, "Snap to Grid"), Some(Toggled::False));
    assert_eq!(journal(&w).last().unwrap(), &("snap.set".to_owned(), json!({"grid": false})));
    run_command(&mut w, "snap.set", json!({"grid": true}));
    assert_eq!(toggled(&w, "Snap to Grid"), Some(Toggled::True));
}

/// With no drawing open the boxes are drawn quiet: the command they send needs a document.
#[test]
fn the_snap_boxes_are_quiet_with_no_drawing_open() {
    let mut w = window(DiacraftApp::new(diacraft_engine::Session::new(), None), [1600.0, 900.0]);
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    for name in ["ribbon.View.SnapToggle", "ribbon.View.SnapToGrid", "ribbon.View.ZoomDialog"] {
        assert!(!w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled, "{name} needs a drawing");
    }
    assert!(w.get_by_label("Snap").accesskit_node().is_disabled());
    assert!(w.state_mut().execute("window.zoomDialog", &json!({})).is_err(), "the Zoom dialog needs a drawing");
}

// ---------------------------------------------------------------------------------------------------------- the status bar's mark

/// The status bar says whether snap is on, and a click on it is `snap.set` with the switch the other way.
#[test]
fn the_status_bar_marks_snap_and_a_click_toggles_it() {
    let mut w = edit_window([1280.0, 720.0]);
    let mark = w.state().element("status.snap").expect("the status bar has a Snap mark").clone();
    assert!(mark.enabled);
    assert_eq!(toggled(&w, "Snap on"), Some(Toggled::True), "a new drawing snaps and the mark says so");
    let entries = journal(&w).len();
    press(&mut w, "status.snap");
    w.rest();
    assert_eq!(settings(&mut w)["snap"], false, "a click on the mark turned the switch off in the document");
    assert_eq!(toggled(&w, "Snap off"), Some(Toggled::False), "and the mark says so");
    assert_eq!(journal(&w).len(), entries + 1);
    assert_eq!(journal(&w).last().unwrap(), &("snap.set".to_owned(), json!({"snap": false})));
    press(&mut w, "status.snap");
    w.rest();
    assert_eq!(settings(&mut w)["snap"], true);
    assert_eq!(toggled(&w, "Snap on"), Some(Toggled::True));
    // What another control or a program does shows in the mark, and the ribbon's box agrees with it.
    run_command(&mut w, "snap.set", json!({"snap": false}));
    assert_eq!(toggled(&w, "Snap off"), Some(Toggled::False));
    assert!(w.state_mut().execute("edit.undo", &json!({})).is_err(), "no undo step: nothing to undo");
    assert_eq!(toggled(&w, "Snap off"), Some(Toggled::False), "the mark stays as the switch was turned");
    run_command(&mut w, "snap.set", json!({"snap": true}));
    assert_eq!(toggled(&w, "Snap on"), Some(Toggled::True));
}

/// The mark is quiet with no drawing open.
#[test]
fn the_status_bar_snap_mark_is_quiet_with_no_drawing() {
    let mut w = window(DiacraftApp::new(diacraft_engine::Session::new(), None), [1280.0, 720.0]);
    w.rest();
    assert!(w.state().element("status.snap").is_none_or(|e| !e.enabled), "nothing to turn off");
}

/// The mark and the rest of the bar stand apart at every width, and the mark is there wherever the page count and the zoom controls have room.
#[test]
fn the_status_bar_keeps_its_parts_apart_at_every_width() {
    for width in [1920.0, 1280.0, 1000.0, 800.0, 640.0, 520.0, 420.0, 320.0] {
        let mut w = drawing_window([width, 600.0]);
        let parts = elements_named(&w, "status.");
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 600.0));
        for (name, rect, _) in &parts {
            assert!(screen.contains_rect(*rect), "{width}: {name} {rect:?} is outside the window");
        }
        for (i, (a, ra, _)) in parts.iter().enumerate() {
            for (b, rb, _) in &parts[i + 1..] {
                assert!(!ra.intersects(*rb), "{width}: {a} {ra:?} overlaps {b} {rb:?}");
            }
        }
        if width >= 640.0 {
            assert!(w.state().element("status.snap").is_some(), "{width}: the Snap mark is on the bar");
        }
        // A shape selected adds its size to the bar: the parts still stand apart.
        let page = w.state().session().document().unwrap().pages.get(0).unwrap().id;
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 4.0, "y": 5.0})).unwrap();
        w.state_mut().execute("edit.selectAll", &json!({})).unwrap();
        w.rest();
        let parts = elements_named(&w, "status.");
        for (i, (a, ra, _)) in parts.iter().enumerate() {
            for (b, rb, _) in &parts[i + 1..] {
                assert!(!ra.intersects(*rb), "{width} with a shape: {a} {ra:?} overlaps {b} {rb:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- the Zoom dialog

/// The choices of the dialog, in the order of the reference's list, with the words they are drawn with.
const CHOICES: [(&str, &str); 9] = [
    ("400", "400%"),
    ("200", "200%"),
    ("150", "150%"),
    ("100", "100%"),
    ("75", "75%"),
    ("50", "50%"),
    ("page_width", "Page width"),
    ("whole_page", "Whole page"),
    ("percent", "Percentage"),
];

fn open_from_ribbon(w: &mut Window) {
    run_command(w, "window.tab", json!({"tab": "View"}));
    assert!(dialog(w).is_null(), "closed to begin with");
    press(w, "ribbon.View.ZoomDialog");
    w.rest();
}

fn ok(w: &mut Window) {
    press(w, "zoom.ok");
    w.rest();
}

/// View > Zoom > Zoom opens the dialog: nine choices in the documented order, Whole page chosen, a Percentage box, OK and Cancel; it changes nothing until OK.
#[test]
fn the_zoom_control_opens_the_dialog_with_the_documented_choices_and_whole_page_chosen() {
    let mut w = edit_window([1280.0, 900.0]);
    let revision = w.state().session().revision();
    let before = zoom_percent(&w);
    open_from_ribbon(&mut w);
    assert_eq!(dialog(&w), "whole_page", "Whole page is the first choice");
    // Every choice is drawn, in the documented order from top to bottom, each with its words.
    let mut tops = Vec::new();
    for (name, words) in CHOICES {
        let e = w.state().element(&format!("zoom.choice.{name}")).unwrap_or_else(|| panic!("zoom.choice.{name} is not drawn")).clone();
        assert!(e.enabled, "{name}");
        tops.push(e.rect.min.y);
        w.get_by_role_and_label(Role::RadioButton, words);
    }
    assert!(tops.windows(2).all(|p| p[0] < p[1]), "the choices stand one under the next in the documented order: {tops:?}");
    assert!(w.state().element("zoom.percent").is_some(), "the Percentage box");
    for name in ["zoom.ok", "zoom.cancel"] {
        assert!(w.state().element(name).unwrap().enabled, "{name}");
    }
    assert_eq!(w.get_by_role_and_label(Role::RadioButton, "Whole page").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(w.get_by_role_and_label(Role::RadioButton, "Percentage").accesskit_node().toggled(), Some(Toggled::False));
    // Nothing moved, and nothing was sent to the engine.
    assert_eq!(zoom_percent(&w), before);
    assert_eq!(w.state().session().revision(), revision);
    // Cancel and Escape close it and change nothing.
    press(&mut w, "zoom.cancel");
    w.rest();
    assert!(dialog(&w).is_null());
    assert_eq!(zoom_percent(&w), before);
    run_command(&mut w, "window.zoomDialog", json!({}));
    assert_eq!(dialog(&w), "whole_page");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(dialog(&w).is_null(), "Escape is Cancel");
    assert_eq!(zoom_percent(&w), before);
    run_command(&mut w, "window.zoomDialog", json!({}));
    run_command(&mut w, "window.zoomDialog", json!({"close": true}));
    assert!(dialog(&w).is_null(), "the command closes it");
    assert_eq!(w.state().session().revision(), revision, "the drawing was never touched");
}

/// Each preset sets that zoom, Page width and Whole page fit as the two View buttons do, and the Percentage box takes a number.
#[test]
fn each_choice_of_the_zoom_dialog_sets_the_view_it_names() {
    let mut w = edit_window([1280.0, 900.0]);
    for (name, percent) in [("400", 400.0), ("200", 200.0), ("150", 150.0), ("100", 100.0), ("75", 75.0), ("50", 50.0)] {
        open_from_ribbon(&mut w);
        press(&mut w, &format!("zoom.choice.{name}"));
        assert_eq!(dialog(&w), name, "the press chose it");
        ok(&mut w);
        assert!(dialog(&w).is_null(), "OK closes the dialog");
        assert_eq!(zoom_percent(&w), percent, "{name}");
    }
    // Page width is the View tab's Page Width button.
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    run_command(&mut w, "view.fitWidth", json!({}));
    let by_button = (zoom_percent(&w), w.state().window_state()["pointsPerInch"].as_f64().unwrap());
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    open_from_ribbon(&mut w);
    press(&mut w, "zoom.choice.page_width");
    ok(&mut w);
    assert_eq!((zoom_percent(&w), w.state().window_state()["pointsPerInch"].as_f64().unwrap()), by_button, "Page width is Page Width");
    // Whole page is Fit to Window: the page is in the drawing area.
    run_command(&mut w, "view.zoom", json!({"percent": 300}));
    open_from_ribbon(&mut w);
    press(&mut w, "zoom.choice.whole_page");
    ok(&mut w);
    let area = rect_of(&w, "canvas");
    let page = w.state().canvas_report().page.unwrap();
    assert!(area.contains_rect(page), "{page:?} is not inside {area:?}: Whole page fits the page");
    // OK on the first choice, left alone, is Whole page too.
    run_command(&mut w, "view.zoom", json!({"percent": 300}));
    open_from_ribbon(&mut w);
    ok(&mut w);
    assert!(area.contains_rect(w.state().canvas_report().page.unwrap()), "the initial choice is Whole page");
}

/// A typed percentage sets the zoom; a number out of the window's range or a word is refused in words and the dialog stays open; typing in the box chooses it.
#[test]
fn the_percentage_box_takes_a_number_and_refuses_the_rest_in_words() {
    let mut w = edit_window([1280.0, 900.0]);
    open_from_ribbon(&mut w);
    let revision = w.state().session().revision();
    let field = rect_of(&w, "zoom.percent").center();
    click(&mut w, field);
    w.rest();
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "175");
    assert_eq!(dialog(&w), "percent", "typing in the box chooses Percentage");
    ok(&mut w);
    assert!(dialog(&w).is_null());
    assert_eq!(zoom_percent(&w), 175.0);
    // A typed number with its percent sign is a number.
    open_from_ribbon(&mut w);
    let field = rect_of(&w, "zoom.percent").center();
    click(&mut w, field);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "60 %");
    ok(&mut w);
    assert_eq!(zoom_percent(&w), 60.0);
    // Refusals: the dialog stays, the zoom stays, and the words say what range is allowed.
    for bad in ["abc", "5", "999", "", "-20", "NaN"] {
        open_from_ribbon(&mut w);
        let before = zoom_percent(&w);
        let field = rect_of(&w, "zoom.percent").center();
        click(&mut w, field);
        key(&mut w, Key::A, crate::saving::ctrl());
        key(&mut w, Key::Backspace, Modifiers::NONE);
        if !bad.is_empty() {
            type_text(&mut w, bad);
        }
        // An empty box does not choose Percentage by being typed in: choose it by the radio.
        press(&mut w, "zoom.choice.percent");
        ok(&mut w);
        assert!(!dialog(&w).is_null(), "{bad:?}: the dialog stays open");
        assert_eq!(zoom_percent(&w), before, "{bad:?}: the zoom is as it was");
        w.get_by_label_contains("from 10 to 400");
        assert_eq!(w.state().session().revision(), revision);
        press(&mut w, "zoom.cancel");
        w.rest();
    }
}

/// The percentage under the status bar's slider opens the dialog.
#[test]
fn the_status_bars_zoom_percentage_opens_the_zoom_dialog() {
    let mut w = edit_window([1280.0, 720.0]);
    let pct = w.state().element("status.zoom_percent").expect("the percentage").clone();
    assert!(pct.enabled, "it opens a dialog now");
    press(&mut w, "status.zoom_percent");
    w.rest();
    assert_eq!(dialog(&w), "whole_page");
    press(&mut w, "zoom.choice.200");
    ok(&mut w);
    assert_eq!(zoom_percent(&w), 200.0);
    w.get_by_label("200%");
}

/// The dialog stays inside the window and its parts stand apart, from a very small window to a very large one; in a short window the list scrolls, and the
/// last choice is reached by the wheel.
#[test]
fn the_zoom_dialog_fits_every_window_size() {
    for size in [[1920.0, 1080.0], [1280.0, 720.0], [800.0, 600.0], [520.0, 480.0], [400.0, 360.0]] {
        let mut w = drawing_window(size);
        run_command(&mut w, "window.zoomDialog", json!({}));
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(size[0], size[1]));
        let (ok, cancel, content) = (rect_of(&w, "zoom.ok"), rect_of(&w, "zoom.cancel"), rect_of(&w, "zoom.content"));
        for (name, rect) in [("zoom.ok", ok), ("zoom.cancel", cancel), ("zoom.content", content)] {
            assert!(screen.contains_rect(rect), "{size:?}: {name} {rect:?} is outside the window");
        }
        assert!(!ok.intersects(cancel), "{size:?}: OK and Cancel overlap");
        assert!(!content.intersects(ok) && !content.intersects(cancel), "{size:?}: the list {content:?} runs over the buttons {ok:?} {cancel:?}");
        let choices: Vec<(String, Rect, bool)> = elements_named(&w, "zoom.choice.");

        assert_eq!(choices.len(), 9, "{size:?}: {choices:?}");
        for (i, (name, rect, _)) in choices.iter().enumerate() {
            for (other, r, _) in choices.iter().skip(i + 1) {
                assert!(!rect.shrink(0.5).intersects(r.shrink(0.5)), "{size:?}: {name} and {other} overlap: {rect:?} {r:?}");
            }
        }
        // The last choice is reached: by the wheel over the list when the window is too short to show it.
        let mut tries = 0;
        while !content.contains_rect(rect_of(&w, "zoom.choice.percent")) {
            tries += 1;
            assert!(tries < 12, "{size:?}: the last choice is out of reach");
            w.input_mut().events.push(egui::Event::PointerMoved(content.center()));
            w.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, -3.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            });
            w.rest();
        }
        press(&mut w, "zoom.choice.percent");
        assert_eq!(dialog(&w), "percent", "{size:?}: the last choice can be pressed");
        assert_eq!(w.state().ui_panics(), 0);
    }
}
