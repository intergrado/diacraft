//! The window: its name, its status bar, its layout at narrow and wide sizes, and the buttons it draws
//! when the app owns the frame.

use diacraft_engine::Session;
use diacraft_ui_egui::{DiacraftApp, WindowButton, WindowButtons};
use egui::{Rect, Vec2, ViewportCommand};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::support::{Rest, commands, window};

fn blank_app() -> DiacraftApp {
    DiacraftApp::new(Session::new(), None)
}

#[test]
fn window_shows_app_name_and_says_when_no_drawing_is_open() {
    let window = window(blank_app(), [800.0, 500.0]);
    window.get_by_label("DiaCraft");
    window.get_by_label("No drawing open. Choose File, then New.");
    assert_eq!(window.state().status_text(), "No document open");
    assert!(window.query_by_label_contains("Page 1 of").is_none(), "no page count without a page");
}

#[test]
fn file_new_changes_status_to_page_count() {
    let mut window = window(blank_app(), [800.0, 500.0]);
    window.state_mut().execute("file.new", &json!({})).unwrap();
    window.step();
    assert!(window.query_by_label_contains("No drawing open").is_none(), "the old message is gone");
    // The drawing has a name of the window's making, and the status bar counts its pages.
    window.get_by_label("Drawing1 - DiaCraft");
    window.get_by_label("Page 1 of 1");
    assert_eq!(window.state().status_text(), "Drawing1 · 1 page");

    window.state_mut().execute("page.add", &json!({})).unwrap();
    window.step();
    window.get_by_label("Page 1 of 2");
    assert_eq!(window.state().status_text(), "Drawing1 · 2 pages");

    window.state_mut().execute("edit.undo", &json!({})).unwrap();
    window.step();
    window.get_by_label("Page 1 of 1");
}

#[test]
fn a_failed_command_leaves_the_status_alone() {
    let mut window = window(blank_app(), [800.0, 500.0]);
    assert!(window.state_mut().execute("page.add", &json!({})).is_err(), "no document yet");
    window.step();
    window.get_by_label("No drawing open. Choose File, then New.");
}

/// A panic message can carry control characters (a terminal escape, a newline). It reaches the window
/// escaped, never raw.
#[test]
fn a_notice_is_escaped_before_it_is_shown() {
    let window = window(blank_app().with_notice("port busy\u{1b}[2J\nsecond line\u{202e}"), [800.0, 500.0]);
    let shown = window.state().notice().unwrap().to_owned();
    assert!(!shown.chars().any(char::is_control), "{shown:?}");
    assert!(!shown.contains('\u{202e}'), "{shown:?}");
    assert!(shown.contains("port busy"), "{shown:?}");
    window.get_by_label_contains("port busy");
    assert_eq!(window.query_all_by_label_contains("\u{1b}").count(), 0, "no raw escape reaches the accessibility tree");
}

/// Only what is dangerous is escaped: text people write (an apostrophe, a quote, a Windows path, an accent made of a
/// combining mark, the joiner in an emoji) is shown as it is, once.
#[test]
fn a_notice_shows_what_people_write_as_it_is() {
    let text = "John's plan \"v2\" in C:\\Users\\me, cafe\u{301} \u{1f469}\u{200d}\u{1f4bb}";
    let app = blank_app().with_notice(text);
    assert_eq!(app.notice(), Some(text));
}

#[test]
fn a_long_notice_is_cut_short_not_allowed_to_fill_the_window() {
    let app = blank_app().with_notice("x".repeat(100_000));
    assert!(app.notice().unwrap().chars().count() <= 400, "{}", app.notice().unwrap().chars().count());
}

/// The text of the window stays inside it, and the top bar and the status bar never overlap, whether the
/// window is narrow or wide, with a long notice or none.
#[test]
fn layout_holds_at_narrow_and_wide_sizes() {
    let notice = "The control channel is off: port 7997 is already in use. Another craft app may be listening there; give --control another port.";
    for size in [[320.0, 240.0], [480.0, 320.0], [800.0, 500.0], [1920.0, 1080.0], [3840.0, 600.0]] {
        for with_notice in [false, true] {
            // A new drawing before the window is built: `file.new` run in the window would take the notice away, as the next command does.
            let mut session = Session::new();
            session.execute("file.new", &json!({})).unwrap();
            let app = DiacraftApp::new(session, None);
            let app = if with_notice { app.with_notice(notice) } else { app };
            let mut window = window(app, size);
            window.rest();
            let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::from(size));
            let name = window.get_by_label("Drawing1 - DiaCraft").rect();
            let status = window.get_by_label("Page 1 of 1").rect();
            let mut texts = vec![("name", name), ("status", status)];
            if with_notice {
                texts.push(("notice", window.get_by_label_contains("The control channel is off").rect()));
            }
            for (what, rect) in &texts {
                assert!(rect.width() > 0.0 && rect.height() > 0.0, "{what} has no area at {size:?}: {rect:?}");
                assert!(screen.contains_rect(*rect), "{what} leaves the {size:?} window: {rect:?}");
            }
            for (i, (a, ra)) in texts.iter().enumerate() {
                for (b, rb) in texts.iter().skip(i + 1) {
                    assert!(!ra.intersects(*rb), "{a} {ra:?} overlaps {b} {rb:?} at {size:?}");
                }
            }
            assert!(name.center().y < status.center().y, "the name is above the status at {size:?}");
        }
    }
}

#[test]
fn a_window_the_desktop_frames_draws_no_window_buttons() {
    let window = window(blank_app(), [800.0, 500.0]);
    for label in ["Close", "Minimize", "Maximize"] {
        assert!(window.query_by_label(label).is_none(), "{label} button drawn although the desktop draws the frame");
    }
}

#[test]
fn a_window_the_app_frames_draws_working_window_buttons() {
    let buttons = WindowButtons { left: vec![WindowButton::Close], right: vec![WindowButton::Minimize, WindowButton::Maximize] };
    let mut window = window(blank_app().with_window_buttons(buttons), [800.0, 500.0]);
    let (close, minimize, maximize) = (window.get_by_label("Close"), window.get_by_label("Minimize"), window.get_by_label("Maximize"));
    // Where the layout puts them: Close on the left, the other two on the right.
    assert!(close.rect().center().x < 400.0 && minimize.rect().center().x > 400.0 && maximize.rect().center().x > 400.0);
    assert!(minimize.rect().center().x < maximize.rect().center().x, "in the order given");
    for node in [close, minimize, maximize] {
        assert!(node.rect().width() >= 16.0 && node.rect().height() >= 16.0, "a button big enough to hit: {:?}", node.rect());
    }

    window.get_by_label("Minimize").click();
    window.step();
    assert!(commands(&window).contains(&ViewportCommand::Minimized(true)), "{:?}", commands(&window));
    window.get_by_label("Maximize").click();
    window.step();
    assert!(commands(&window).contains(&ViewportCommand::Maximized(true)), "{:?}", commands(&window));
    window.get_by_label("Close").click();
    window.step();
    assert!(commands(&window).contains(&ViewportCommand::Close), "{:?}", commands(&window));
}

/// Without a button for one of them the window could not be closed with the mouse, so a layout with none
/// still gets a Close button.
#[test]
fn an_app_frame_always_has_a_close_button() {
    let window = window(blank_app().with_window_buttons(WindowButtons { left: vec![], right: vec![] }), [800.0, 500.0]);
    window.get_by_label("Close");
}

/// A panic while the frame is drawn must not end the app: the next frame draws, and the window says what happened.
#[test]
fn a_panic_in_the_frame_is_caught_and_the_window_goes_on() {
    use diacraft_ui_egui::Stage;
    use std::cell::Cell;
    use std::rc::Rc;
    let armed = Rc::new(Cell::new(false));
    let probe_armed = Rc::clone(&armed);
    let app = blank_app().with_probe(move |stage| {
        if matches!(stage, Stage::Frame) && probe_armed.replace(false) {
            panic!("boom in the frame\u{1b}[31m");
        }
    });
    let mut window = window(app, [800.0, 500.0]);
    assert_eq!(window.state().ui_panics(), 0);
    armed.set(true);
    window.step();
    assert_eq!(window.state().ui_panics(), 1, "the probe ran inside the guard and the panic was counted");
    // The frame that panicked asks for another one soon, so the error shows without waiting for input.
    assert_repaint_soon(&window);
    // And that frame, with no help from the test, draws everything again and says what happened.
    window.rest();
    window.get_by_label_contains("internal error");
    window.get_by_label("DiaCraft");
    assert!(!window.state().notice().unwrap().chars().any(char::is_control));
    // And it keeps working: a later command shows in the status bar.
    window.state_mut().execute("file.new", &json!({})).unwrap();
    window.step();
    window.get_by_label("Page 1 of 1");
    assert_eq!(window.state().ui_panics(), 1, "no second panic");
}

/// What the last frame asked for: another frame within 200 ms (not never, which is `Duration::MAX`).
fn assert_repaint_soon(window: &crate::support::Window) {
    let delay = window.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
    assert!(delay.is_some_and(|d| d <= std::time::Duration::from_millis(200)), "the next frame was not asked for soon: {delay:?}");
}

/// A panic in the middle of drawing, inside a panel's contents (the probe at the start of a frame cannot make that),
/// leaves a frame cut short on screen. The window asks for the next frame itself, and it draws whole.
#[test]
fn a_panic_in_the_middle_of_a_frame_is_caught_and_the_next_frame_draws_whole() {
    use diacraft_ui_egui::Stage;
    use std::cell::Cell;
    use std::rc::Rc;
    for panel in ["top_bar", "status_bar", "canvas"] {
        let armed = Rc::new(Cell::new(false));
        let probe_armed = Rc::clone(&armed);
        let app = blank_app().with_probe(move |stage| {
            if matches!(stage, Stage::Panel(name) if name == panel) && probe_armed.replace(false) {
                panic!("boom in the {panel}\u{1b}[31m");
            }
        });
        let mut window = window(app, [800.0, 500.0]);
        window.state_mut().execute("file.new", &json!({})).unwrap();
        window.rest();
        window.get_by_label("Page 1 of 1");
        armed.set(true);
        window.step();
        assert_eq!(window.state().ui_panics(), 1, "{panel}: caught");
        assert_repaint_soon(&window);
        // With no more input, the frames that follow settle on a whole window and the error.
        window.rest();
        assert_eq!(window.state().ui_panics(), 1, "{panel}: it does not panic again");
        window.get_by_label("Drawing1 - DiaCraft");
        window.get_by_label("Page 1 of 1");
        window.get_by_label_contains(&format!("boom in the {panel}"));
        assert!(!window.state().notice().unwrap().chars().any(char::is_control), "{panel}");
    }
}

/// A bug that panics on every frame costs a frame every 100 ms, not a frame as fast as the screen goes.
#[test]
fn a_panic_on_every_frame_does_not_spin() {
    use diacraft_ui_egui::Stage;
    let app = blank_app().with_probe(|stage| {
        if matches!(stage, Stage::Frame) {
            panic!("always");
        }
    });
    let mut window = window(app, [800.0, 500.0]);
    window.step();
    let delay = window.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap();
    assert!(delay >= std::time::Duration::from_millis(50), "the next frame was asked for at once: {delay:?}");
    assert!(delay <= std::time::Duration::from_millis(200), "{delay:?}");
}

/// The window starts in the light theme whatever the desktop prefers, and the dark theme is kept as a token set that a command
/// switches to.
#[test]
fn the_window_is_light_by_default_and_keeps_a_dark_theme() {
    let mut window = crate::support::drawing_window([1000.0, 600.0]);
    assert_eq!(window.ctx.theme(), egui::Theme::Light);
    assert_eq!(window.state().window_state()["theme"], "light");
    crate::support::run_command(&mut window, "window.theme", json!({"theme": "dark"}));
    assert_eq!(window.ctx.theme(), egui::Theme::Dark);
    crate::support::run_command(&mut window, "window.theme", json!({"theme": "light"}));
    assert_eq!(window.ctx.theme(), egui::Theme::Light);
}

/// Undo and Redo on the Quick Access Toolbar are enabled when the history has a step for them, and a press runs the command.
#[test]
fn undo_and_redo_on_the_toolbar_follow_the_history() {
    let mut window = crate::support::drawing_window([1000.0, 600.0]);
    for name in ["qat.Undo", "qat.Redo"] {
        assert!(!window.state().element(name).unwrap().enabled, "{name}: nothing to do yet");
    }
    assert!(window.state().element("qat.Save").unwrap().enabled, "Save works on any open drawing");
    crate::support::run_command(&mut window, "page.add", json!({}));
    assert!(window.state().element("qat.Undo").unwrap().enabled && !window.state().element("qat.Redo").unwrap().enabled);
    let undo = window.state().element("qat.Undo").unwrap().rect.center();
    crate::support::click(&mut window, undo);
    window.rest();
    assert_eq!(window.state().session().document().unwrap().pages.len(), 1, "Undo took the page away");
    assert!(window.state().element("qat.Redo").unwrap().enabled);
    let redo = window.state().element("qat.Redo").unwrap().rect.center();
    crate::support::click(&mut window, redo);
    window.rest();
    assert_eq!(window.state().session().document().unwrap().pages.len(), 2, "and Redo brought it back");
}

/// A command's shortcut runs it: Ctrl+Z undoes, Ctrl+Shift+Z redoes, Ctrl+N makes a new drawing, Ctrl with = zooms in.
#[test]
fn shortcuts_run_their_commands_when_the_commands_can_run() {
    let key = |window: &mut crate::support::Window, key: egui::Key, modifiers: egui::Modifiers| {
        window.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        window.step();
        window.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        window.rest();
    };
    let ctrl = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    let ctrl_shift = egui::Modifiers { shift: true, ..ctrl };
    let mut window = crate::support::drawing_window([1000.0, 600.0]);
    let pages = |window: &crate::support::Window| window.state().session().document().unwrap().pages.len();
    // Nothing to undo: the shortcut does nothing, and does not fail.
    key(&mut window, egui::Key::Z, ctrl);
    assert_eq!(window.state().ui_panics(), 0);
    crate::support::run_command(&mut window, "page.add", json!({}));
    assert_eq!(pages(&window), 2);
    key(&mut window, egui::Key::Z, ctrl);
    assert_eq!(pages(&window), 1, "Ctrl+Z undid the page");
    key(&mut window, egui::Key::Z, ctrl_shift);
    assert_eq!(pages(&window), 2, "Ctrl+Shift+Z redid it");
    let zoom = window.state().zoom();
    key(&mut window, egui::Key::Equals, ctrl);
    assert!(window.state().zoom() > zoom * 1.2, "Ctrl+= zoomed in");
    // The drawing has an edit that is not saved (the page it added, then took back and brought again): Ctrl+N asks before it replaces it.
    key(&mut window, egui::Key::N, ctrl);
    assert!(window.state().prompt_open());
    assert_eq!(pages(&window), 2, "and has not replaced it yet");
    window.get_by_label("Don't Save").click();
    window.rest();
    window.get_by_label("Drawing2 - DiaCraft");
    assert_eq!(pages(&window), 1, "Ctrl+N made a new drawing");
}
