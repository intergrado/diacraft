//! The drawing area: the page at its default size on launch, the grid and rulers, panning and zooming about the pointer.

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, pos2, vec2};
use serde_json::json;

use crate::support::{Rest, drawing_window, move_to, run_command};

/// A drag with the middle button: the primary button draws now, and the middle one moves the page.
fn middle_drag(window: &mut crate::support::Window, from: egui::Pos2, by: egui::Vec2) {
    let button = |window: &mut crate::support::Window, pos: egui::Pos2, pressed: bool| {
        window.input_mut().events.push(Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: Modifiers::NONE });
        window.step();
    };
    for _ in 0..3 {
        move_to(window, from);
    }
    button(window, from, true);
    for step in 1..=5 {
        move_to(window, from + by * (step as f32 / 5.0));
    }
    button(window, from + by, false);
}

fn wheel(window: &mut crate::support::Window, at: egui::Pos2, dy: f32, modifiers: Modifiers) {
    for _ in 0..3 {
        move_to(window, at);
    }
    window.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, dy),
        phase: TouchPhase::Move,
        modifiers,
        source: MouseWheelSource::Unknown,
    });
    // The wheel is smoothed over a few frames.
    for _ in 0..12 {
        window.step();
    }
}

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// A new drawing is a white page of the default size (US Letter landscape), centred and fitted in the drawing area, with the
/// grid and the rulers over it.
#[test]
fn a_new_drawing_shows_a_default_size_page_fitted_and_centred() {
    let window = drawing_window([1280.0, 720.0]);
    let report = window.state().canvas_report();
    let page = report.page.expect("a page is drawn");
    let area = window.state().element("canvas").unwrap().rect;
    assert!((page.width() / page.height() - 11.0 / 8.5).abs() < 0.01, "the default page is 11 by 8.5: {page:?}");
    assert!(area.contains_rect(page), "{page:?} is not inside {area:?}");
    assert!((page.center().x - area.center().x).abs() < 1.0 && (page.center().y - area.center().y).abs() < 1.0, "centred");
    // Fitted: it fills the room in one direction, less a margin.
    assert!(page.height() > area.height() * 0.8 || page.width() > area.width() * 0.8, "{page:?} in {area:?}");
    assert!(report.rulers && report.grid_lines.0 > 3 && report.grid_lines.1 > 3, "{report:?}");
    // Reported at 100% = 96 points to an inch, a fit is under it for this window.
    let zoom = window.state().window_state()["zoomPercent"].as_f64().unwrap();
    assert!((30.0..100.0).contains(&zoom), "{zoom}");
}

/// A resize of the window fits the page again, while a view the person changed stays as it is.
#[test]
fn a_fitted_page_follows_the_window_and_a_zoomed_one_does_not() {
    let mut window = drawing_window([1280.0, 720.0]);
    let before = window.state().canvas_report().page.unwrap();
    window.set_size(vec2(900.0, 600.0));
    window.rest();
    let after = window.state().canvas_report().page.unwrap();
    assert!(after.width() < before.width(), "the page follows a smaller window: {before:?} -> {after:?}");
    let area = window.state().element("canvas").unwrap().rect;
    assert!(area.contains_rect(after));
    run_command(&mut window, "view.zoom", json!({"percent": 150}));
    let zoomed = window.state().canvas_report().page.unwrap();
    window.set_size(vec2(1280.0, 720.0));
    window.rest();
    let later = window.state().canvas_report().page.unwrap();
    assert!((later.width() - zoomed.width()).abs() < 0.5, "a zoomed page keeps its size when the window changes");
}

/// Zooming with the wheel (Ctrl) keeps the page point under the pointer where it is.
#[test]
fn zoom_about_the_pointer_keeps_the_point_under_it() {
    let mut window = drawing_window([1280.0, 720.0]);
    let area = window.state().element("canvas").unwrap().rect;
    for offset in [vec2(0.0, 0.0), vec2(180.0, -90.0), vec2(-250.0, 120.0)] {
        run_command(&mut window, "view.fitPage", json!({}));
        let at = area.center() + offset;
        for _ in 0..3 {
            move_to(&mut window, at);
        }
        let before = window.state().page_point_at(at).unwrap();
        let zoom_before = window.state().zoom();
        wheel(&mut window, at, 4.0, CTRL);
        let (zoom_after, after) = (window.state().zoom(), window.state().page_point_at(at).unwrap());
        assert!(zoom_after > zoom_before * 1.2, "the wheel zoomed in: {zoom_before} -> {zoom_after}");
        assert!((before.0 - after.0).abs() < 0.01 && (before.1 - after.1).abs() < 0.01, "{before:?} moved to {after:?} at offset {offset:?}");
        // And out again, about the same point.
        wheel(&mut window, at, -4.0, CTRL);
        let back = window.state().page_point_at(at).unwrap();
        assert!((before.0 - back.0).abs() < 0.01 && (before.1 - back.1).abs() < 0.01, "{before:?} moved to {back:?} zooming out");
    }
}

/// The zoom commands, about the middle of the drawing area unless a point is given, and within the range.
#[test]
fn zoom_commands_change_the_zoom_and_stay_in_range() {
    let mut window = drawing_window([1280.0, 720.0]);
    run_command(&mut window, "view.zoom", json!({"percent": 100}));
    assert_eq!(window.state().window_state()["zoomPercent"], 100.0);
    run_command(&mut window, "view.zoomIn", json!({}));
    assert_eq!(window.state().window_state()["zoomPercent"], 125.0);
    run_command(&mut window, "view.zoomOut", json!({}));
    run_command(&mut window, "view.zoomOut", json!({}));
    assert_eq!(window.state().window_state()["zoomPercent"], 80.0);
    run_command(&mut window, "view.zoom", json!({"percent": 100000}));
    assert_eq!(window.state().window_state()["zoomPercent"], 400.0, "clamped to the largest zoom");
    run_command(&mut window, "view.zoom", json!({"percent": 1}));
    assert_eq!(window.state().window_state()["zoomPercent"], 10.0, "and to the smallest");
    // A bad number is an error that changes nothing.
    assert!(window.state_mut().execute("view.zoom", &json!({"percent": "big"})).is_err());
    assert!(window.state_mut().execute("view.zoom", &json!({"percent": 150, "x": 10})).is_err(), "x without y");
    assert!(window.state_mut().execute("view.zoom", &json!({"percnt": 150})).unwrap_err().to_string().contains("percent"));
    assert_eq!(window.state().window_state()["zoomPercent"], 10.0);
}

/// A drag with the middle button moves the page with the pointer; the wheel scrolls it.
#[test]
fn a_drag_pans_the_page_and_the_wheel_scrolls_it() {
    let mut window = drawing_window([1280.0, 720.0]);
    run_command(&mut window, "view.zoom", json!({"percent": 100}));
    let area = window.state().element("canvas").unwrap().rect;
    let before = window.state().canvas_report().page.unwrap();
    middle_drag(&mut window, area.center(), vec2(-120.0, 60.0));
    window.rest();
    let after = window.state().canvas_report().page.unwrap();
    assert!((after.min.x - before.min.x + 120.0).abs() < 8.0 && (after.min.y - before.min.y - 60.0).abs() < 8.0, "{before:?} -> {after:?}");
    // The wheel scrolls up and down.
    let scrolled_from = window.state().canvas_report().page.unwrap();
    wheel(&mut window, area.center(), -3.0, Modifiers::NONE);
    let scrolled = window.state().canvas_report().page.unwrap();
    assert!((scrolled.min.y - scrolled_from.min.y).abs() > 10.0, "{scrolled_from:?} -> {scrolled:?}");
    assert!((scrolled.min.x - scrolled_from.min.x).abs() < 1.0, "a plain wheel does not move the page sideways");
    // The pan command moves it by an amount of points.
    let from = window.state().canvas_report().page.unwrap();
    run_command(&mut window, "view.pan", json!({"dx": 40, "dy": -25}));
    let to = window.state().canvas_report().page.unwrap();
    assert!((to.min.x - from.min.x - 40.0).abs() < 0.6 && (to.min.y - from.min.y + 25.0).abs() < 0.6, "{from:?} -> {to:?}");
}

/// Fit and Page Width.
#[test]
fn fit_shows_the_whole_page_and_page_width_fills_the_width() {
    let mut window = drawing_window([1280.0, 720.0]);
    let area = window.state().element("canvas").unwrap().rect;
    run_command(&mut window, "view.zoom", json!({"percent": 300}));
    run_command(&mut window, "view.fitPage", json!({}));
    assert!(area.contains_rect(window.state().canvas_report().page.unwrap()));
    run_command(&mut window, "view.fitWidth", json!({}));
    let page = window.state().canvas_report().page.unwrap();
    assert!(page.width() > area.width() * 0.9 && page.width() <= area.width(), "{page:?} in {area:?}");
}

/// The grid and the rulers are the drawing's window settings: the View commands turn them on and off. Each one changes the drawing (a save keeps it) and is
/// no undo step and no new revision.
#[test]
fn the_grid_and_rulers_are_overlays_the_view_commands_turn_on_and_off() {
    let mut window = drawing_window([1280.0, 720.0]);
    let revision = window.state().session().revision();
    run_command(&mut window, "view.grid", json!({"on": false}));
    assert_eq!(window.state().canvas_report().grid_lines, (0, 0));
    assert!(window.state().canvas_report().rulers);
    run_command(&mut window, "view.rulers", json!({"on": false}));
    assert!(!window.state().canvas_report().rulers);
    let with_no_rulers = window.state().element("canvas").unwrap().rect;
    run_command(&mut window, "view.rulers", json!({}));
    assert!(window.state().canvas_report().rulers, "without `on` the switch toggles");
    assert!(window.state().element("canvas").unwrap().rect.min.x > with_no_rulers.min.x, "the rulers take the room they need");
    run_command(&mut window, "view.grid", json!({"on": true}));
    assert!(window.state().canvas_report().grid_lines.0 > 3);
    assert_eq!(window.state().session().revision(), revision, "a box turned moves no revision: the page is not drawn again for it");
    assert_eq!(window.state().session().history().unwrap().undo_len(), 0, "and is no undo step");
    assert!(window.state().is_dirty(), "but it is a change of the drawing, which a save keeps");
    // The grid follows the zoom: fewer lines to the page when zoomed out than the doubling of the pitch gives at 400%.
    run_command(&mut window, "view.zoom", json!({"percent": 400}));
    let zoomed_in = window.state().canvas_report().grid_lines;
    run_command(&mut window, "view.zoom", json!({"percent": 10}));
    let zoomed_out = window.state().canvas_report().grid_lines;
    assert!(zoomed_out.0 < zoomed_in.0 || zoomed_in.0 >= 80, "{zoomed_out:?} {zoomed_in:?}");
}

/// A hostile command or window size never panics.
#[test]
fn hostile_view_commands_and_sizes_do_not_panic() {
    let mut window = drawing_window([480.0, 320.0]);
    for params in [json!({"percent": f64::MAX}), json!({"percent": -5}), json!({"percent": 0})] {
        let _ = window.state_mut().execute("view.zoom", &params);
        window.step();
    }
    let _ = window.state_mut().execute("view.pan", &json!({"dx": 1.0e300, "dy": -1.0e300}));
    window.step();
    window.set_size(vec2(200.0, 200.0));
    window.rest();
    window.set_size(vec2(4000.0, 4000.0));
    window.rest();
    assert_eq!(window.state().ui_panics(), 0);
    let _ = pos2(0.0, 0.0);
}
