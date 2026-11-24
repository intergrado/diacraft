//! A window whose frame the app draws (a desktop that leaves it to the app, such as GNOME's Wayland session):
//! its title bar moves the window and its buttons work, and its edges resize it. With the desktop's own frame
//! none of this is drawn.

use diacraft_engine::Session;
use diacraft_ui_egui::{DiacraftApp, WindowButtons};
use egui::{CursorIcon, ResizeDirection, ViewportCommand, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::support::{Window, button, click, drag, fast_window, move_to, window};

const SIZE: [f32; 2] = [800.0, 500.0];

fn app_frame() -> DiacraftApp {
    DiacraftApp::new(Session::new(), None).with_window_buttons(WindowButtons::default())
}

fn desktop_frame() -> DiacraftApp {
    DiacraftApp::new(Session::new(), None)
}

fn starts_drag(sent: &[ViewportCommand]) -> usize {
    sent.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count()
}

fn resize_directions(sent: &[ViewportCommand]) -> Vec<ResizeDirection> {
    sent.iter().filter_map(|c| if let ViewportCommand::BeginResize(d) = c { Some(*d) } else { None }).collect()
}

/// The empty part of the title bar moves the window.
#[test]
fn dragging_the_empty_title_bar_moves_the_window() {
    let mut window = window(app_frame(), SIZE);
    let sent = drag(&mut window, pos2(400.0, 18.0), vec2(40.0, 0.0));
    assert_eq!(starts_drag(&sent), 1, "{sent:?}");
    // Also on the bar's margin, above and below its row, and next to the name.
    for y in [6.0, 30.0] {
        let sent = drag(&mut window, pos2(300.0, y), vec2(0.0, 40.0));
        assert_eq!(starts_drag(&sent), 1, "at y {y}: {sent:?}");
    }
}

/// A press on a window button that moves away is not a drag of the window, and not a click of the button: the
/// buttons take their own presses. (A background that senses drags under click-only buttons took them.)
#[test]
fn pressing_a_window_button_and_dragging_away_does_not_move_the_window() {
    for label in ["Close", "Minimize", "Maximize"] {
        let mut window = window(app_frame(), SIZE);
        let centre = window.get_by_label(label).rect().center();
        let sent = drag(&mut window, centre, vec2(-40.0, 0.0));
        assert_eq!(starts_drag(&sent), 0, "{label}: {sent:?}");
        for acted in [ViewportCommand::Close, ViewportCommand::Minimized(true), ViewportCommand::Maximized(true)] {
            assert!(!sent.contains(&acted), "{label}: dragging off it was taken as a click: {sent:?}");
        }
    }
}

#[test]
fn clicking_each_window_button_still_works() {
    for (label, expected) in
        [("Close", ViewportCommand::Close), ("Minimize", ViewportCommand::Minimized(true)), ("Maximize", ViewportCommand::Maximized(true))]
    {
        let mut window = window(app_frame(), SIZE);
        let centre = window.get_by_label(label).rect().center();
        let sent = click(&mut window, centre);
        assert!(sent.contains(&expected), "{label}: {sent:?}");
        assert_eq!(starts_drag(&sent), 0, "{label}: {sent:?}");
    }
}

#[test]
fn double_clicking_the_empty_title_bar_maximizes_the_window() {
    let mut window = fast_window(app_frame(), SIZE);
    let first = click(&mut window, pos2(400.0, 18.0));
    assert!(!first.contains(&ViewportCommand::Maximized(true)), "one click does not: {first:?}");
    let second = click(&mut window, pos2(400.0, 18.0));
    assert!(second.contains(&ViewportCommand::Maximized(true)), "{second:?}");
    // On a button, a double click is two clicks of the button and not a maximise by the bar.
    let mut window = fast_window(app_frame(), SIZE);
    let centre = window.get_by_label("Minimize").rect().center();
    click(&mut window, centre);
    let sent = click(&mut window, centre);
    assert!(!sent.contains(&ViewportCommand::Maximized(true)), "{sent:?}");
}

#[test]
fn the_edges_and_corners_resize_the_window() {
    let (w, h) = (SIZE[0], SIZE[1]);
    let cases = [
        (pos2(2.0, h / 2.0), ResizeDirection::West),
        (pos2(w - 2.0, h / 2.0), ResizeDirection::East),
        (pos2(w / 2.0, 1.0), ResizeDirection::North),
        (pos2(w / 2.0, h - 2.0), ResizeDirection::South),
        (pos2(2.0, 2.0), ResizeDirection::NorthWest),
        (pos2(w - 2.0, 2.0), ResizeDirection::NorthEast),
        (pos2(2.0, h - 2.0), ResizeDirection::SouthWest),
        (pos2(w - 2.0, h - 2.0), ResizeDirection::SouthEast),
    ];
    for (at, direction) in cases {
        let mut window = window(app_frame(), SIZE);
        for _ in 0..3 {
            move_to(&mut window, at);
        }
        let sent = button(&mut window, at, true);
        assert_eq!(resize_directions(&sent), [direction], "pressing at {at:?}: {sent:?}");
        assert_eq!(starts_drag(&sent), 0, "an edge is not the title bar: {sent:?}");
    }
}

#[test]
fn the_pointer_shows_a_resize_cursor_over_each_edge() {
    let mut window = window(app_frame(), SIZE);
    for (at, icon) in [
        (pos2(2.0, 250.0), CursorIcon::ResizeWest),
        (pos2(798.0, 250.0), CursorIcon::ResizeEast),
        (pos2(400.0, 498.0), CursorIcon::ResizeSouth),
        (pos2(2.0, 2.0), CursorIcon::ResizeNorthWest),
        (pos2(798.0, 498.0), CursorIcon::ResizeSouthEast),
    ] {
        for _ in 0..3 {
            move_to(&mut window, at);
        }
        assert_eq!(window.output().platform_output.cursor_icon, icon, "at {at:?}");
    }
    for _ in 0..3 {
        move_to(&mut window, pos2(400.0, 250.0));
    }
    assert_eq!(window.output().platform_output.cursor_icon, CursorIcon::Default, "in the middle");
}

/// Nothing resizes from the middle of the canvas, and nothing resizes at all when the desktop draws the frame
/// (it has its own edges) or the window is maximised.
#[test]
fn only_an_app_drawn_frame_has_resize_edges_and_not_when_maximized() {
    let mut window = window(app_frame(), SIZE);
    let sent = drag(&mut window, pos2(400.0, 250.0), vec2(30.0, 30.0));
    assert!(resize_directions(&sent).is_empty() && starts_drag(&sent) == 0, "{sent:?}");

    let mut window = window_for(desktop_frame());
    let sent = drag(&mut window, pos2(2.0, 250.0), vec2(30.0, 0.0));
    assert!(resize_directions(&sent).is_empty(), "the desktop's frame has its own edges: {sent:?}");
    let sent = drag(&mut window, pos2(400.0, 18.0), vec2(30.0, 0.0));
    assert_eq!(starts_drag(&sent), 0, "and its own title bar: {sent:?}");

    let mut window = window_for(app_frame());
    window.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().maximized = Some(true);
    window.step();
    for _ in 0..3 {
        move_to(&mut window, pos2(2.0, 250.0));
    }
    let sent = button(&mut window, pos2(2.0, 250.0), true);
    assert!(resize_directions(&sent).is_empty(), "a maximised window is not resized by its edges: {sent:?}");
}

fn window_for(app: DiacraftApp) -> Window {
    window(app, SIZE)
}

/// Presses at `from` and drags to `from + by`, and never releases: the window manager took the release when it began
/// the move or the resize, as it does on X11 and Wayland, so egui never sees it.
fn press_and_drag_without_release(window: &mut Window, from: egui::Pos2, by: egui::Vec2) -> Vec<ViewportCommand> {
    let mut sent = Vec::new();
    for _ in 0..3 {
        sent.extend(move_to(window, from));
    }
    sent.extend(button(window, from, true));
    for step in 1..=5 {
        sent.extend(move_to(window, from + by * (step as f32 / 5.0)));
    }
    sent
}

/// After the window manager has taken a drag (and its release), the next press on the title bar starts another. Found
/// by running the real window: the first drag moved it, the second did nothing, because egui still held the first.
#[test]
fn a_second_title_bar_drag_works_after_the_window_manager_took_the_first() {
    let mut window = window(app_frame(), SIZE);
    let first = press_and_drag_without_release(&mut window, pos2(400.0, 18.0), vec2(40.0, 20.0));
    assert_eq!(starts_drag(&first), 1, "{first:?}");
    // The hand-over itself: egui no longer holds the first press, though no release ever reached it. The second drag
    // below starts with or without the hand-over under kittest (measured: all nine tests here passed with it removed),
    // so only this line tells the two apart. On the real window, the held press made the second drag do nothing.
    assert!(!window.ctx.input(|i| i.pointer.any_down()), "egui still holds the first press after the window manager took it");
    // The window moved under the pointer; the next press is somewhere else on the bar, and no release came in between.
    let second = press_and_drag_without_release(&mut window, pos2(300.0, 18.0), vec2(-40.0, 20.0));
    assert_eq!(starts_drag(&second), 1, "the second drag did nothing: {second:?}");
}

#[test]
fn a_resize_works_again_after_the_window_manager_took_the_first() {
    let mut window = window(app_frame(), SIZE);
    let first = press_and_drag_without_release(&mut window, pos2(798.0, 250.0), vec2(40.0, 0.0));
    assert_eq!(resize_directions(&first), [ResizeDirection::East], "{first:?}");
    let second = press_and_drag_without_release(&mut window, pos2(2.0, 250.0), vec2(-40.0, 0.0));
    assert_eq!(resize_directions(&second), [ResizeDirection::West], "the second resize did nothing: {second:?}");
    // And a title-bar drag after a resize.
    let third = press_and_drag_without_release(&mut window, pos2(400.0, 18.0), vec2(40.0, 20.0));
    assert_eq!(starts_drag(&third), 1, "{third:?}");
}
