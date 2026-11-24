//! The control channel's way to find a control and press it: `ui.elements`, and the synthetic input methods. Where a test can
//! see their result, it does: the queue of events is handed to egui as a person's input would be.

use egui::Event;
use serde_json::{Value, json};

use crate::support::{Rest, call, controlled_with, send};

/// Hands the queued input to the window a frame at a time, as the desktop app does, until none is left.
fn pump(window: &mut crate::support::Window) {
    for _ in 0..40 {
        let events = window.state_mut().next_input_frame();
        let idle = events.is_empty();
        window.input_mut().events.extend(events);
        window.step();
        if idle && window.state().pending_input() == 0 {
            break;
        }
    }
    window.rest();
}

fn drawing_controlled() -> (crate::support::Window, std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>) {
    controlled_with(|mut app| {
        app.execute("file.new", &json!({})).unwrap();
        app
    })
}

/// `ui.elements` names what the last frame drew, where, and whether it can be used.
#[test]
fn elements_lists_the_controls_with_their_places_and_state() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    let all = call(&mut window, &tx, "ui.elements", json!({})).unwrap();
    let find = |list: &Value, name: &str| {
        list.as_array().unwrap().iter().find(|e| e["name"] == name).cloned().unwrap_or_else(|| panic!("{name} not listed"))
    };
    let view = find(&all, "tab.View");
    assert_eq!(view["enabled"], true);
    assert!(view["w"].as_f64().unwrap() > 20.0 && view["h"].as_f64().unwrap() > 10.0, "{view}");
    assert_eq!(find(&all, "qat.Undo")["enabled"], false, "there is nothing to undo");
    assert_eq!(find(&all, "status.zoom_in")["enabled"], true);
    // A prefix narrows it, and a bad one is an error.
    let tabs = call(&mut window, &tx, "ui.elements", json!({"prefix": "tab."})).unwrap();
    assert!(tabs.as_array().unwrap().iter().all(|e| e["name"].as_str().unwrap().starts_with("tab.")));
    assert_eq!(tabs.as_array().unwrap().len(), 10, "File and nine tabs");
    assert!(call(&mut window, &tx, "ui.elements", json!({"prefix": 5})).unwrap_err().contains("prefix"));
    assert!(call(&mut window, &tx, "ui.elements", json!({"prefx": "x"})).unwrap_err().contains("prefx"));
}

/// A control is pressed by name through the channel, as by a person: the ribbon's View tab, then its Grid check box.
#[test]
fn clicking_a_control_by_name_presses_it() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    call(&mut window, &tx, "ui.click", json!({"element": "tab.View"})).unwrap();
    pump(&mut window);
    assert_eq!(window.state().window_state()["tab"], "View");
    assert!(window.state().canvas_report().grid_lines.0 > 0);
    call(&mut window, &tx, "ui.click", json!({"element": "ribbon.View.ShowGrid"})).unwrap();
    pump(&mut window);
    assert_eq!(window.state().canvas_report().grid_lines, (0, 0), "the Grid check box was pressed");
    // A disabled control is pressed too, and does nothing.
    let before = window.state().session().revision();
    call(&mut window, &tx, "ui.click", json!({"element": "qat.Save"})).unwrap();
    pump(&mut window);
    assert_eq!(window.state().session().revision(), before);
    // A name that was not drawn is an error that says where to look.
    let error = call(&mut window, &tx, "ui.click", json!({"element": "tab.Nope"})).unwrap_err();
    assert!(error.contains("ui.elements"), "{error}");
}

/// `ui.drag {hold: true}` leaves the button down where the drag ends, so that a client can look at the drag in progress (a guide out of a ruler),
/// and `ui.release` lets it go there.
#[test]
fn a_held_drag_stays_in_progress_until_it_is_released() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    let state = window.state().window_state();
    let (rect, ppi) = (state["pageRect"].as_array().unwrap().clone(), state["pointsPerInch"].as_f64().unwrap());
    let (left, top, height) = (rect[0].as_f64().unwrap(), rect[1].as_f64().unwrap(), rect[3].as_f64().unwrap());
    // Two inches up from the page's bottom edge, on the screen.
    let (x, y) = (left + 3.0 * ppi, top + height - 2.0 * ppi);
    call(&mut window, &tx, "ui.drag", json!({"element": "canvas.ruler.top", "toX": x, "toY": y, "hold": true})).unwrap();
    pump(&mut window);
    assert!(window.state().edit_dragging(), "the button is still down");
    let at = window.state().window_state()["drag"]["guideAt"].as_f64().expect("the guide's place is shown while it is dragged");
    assert!((at - 2.0).abs() < 0.02, "{at}");
    assert!(window.state().session().document().unwrap().pages.get(0).unwrap().shapes.is_empty(), "nothing is made yet");
    call(&mut window, &tx, "ui.release", json!({})).unwrap();
    pump(&mut window);
    assert!(!window.state().edit_dragging());
    assert_eq!(window.state().session().document().unwrap().pages.get(0).unwrap().shapes.len(), 1, "the guide is made at the release");
    // A bad `hold` is refused whole, and so is a key `ui.release` does not take.
    assert!(call(&mut window, &tx, "ui.drag", json!({"x": 1, "y": 1, "toX": 2, "toY": 2, "hold": "yes"})).unwrap_err().contains("hold"));
    assert!(call(&mut window, &tx, "ui.release", json!({"steps": 2})).unwrap_err().contains("steps"));
}

/// A press and its release reach egui in separate frames, with the move before the press.
#[test]
fn a_click_is_a_move_then_a_press_then_a_release_in_separate_frames() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    call(&mut window, &tx, "ui.click", json!({"x": 100, "y": 100})).unwrap();
    assert_eq!(window.state().pending_input(), 3);
    let first = window.state_mut().next_input_frame();
    assert!(matches!(first.as_slice(), [Event::PointerMoved(_), Event::PointerButton { pressed: true, .. }]), "{first:?}");
    let second = window.state_mut().next_input_frame();
    assert!(matches!(second.as_slice(), [Event::PointerButton { pressed: false, .. }]), "{second:?}");
    assert_eq!(window.state().pending_input(), 0);
    // A wheel turn is a move and the turn together; a key is a press and then a release.
    call(&mut window, &tx, "ui.wheel", json!({"x": 100, "y": 100, "dy": 2, "ctrl": true})).unwrap();
    let wheel = window.state_mut().next_input_frame();
    assert!(matches!(wheel.as_slice(), [Event::PointerMoved(_), Event::MouseWheel { modifiers, .. }] if modifiers.ctrl), "{wheel:?}");
    call(&mut window, &tx, "ui.key", json!({"key": "Escape"})).unwrap();
    assert!(matches!(window.state_mut().next_input_frame().as_slice(), [Event::Key { pressed: true, .. }]));
    assert!(matches!(window.state_mut().next_input_frame().as_slice(), [Event::Key { pressed: false, .. }]));
}

/// The input methods check everything before they queue anything.
#[test]
fn bad_input_requests_are_errors_that_queue_nothing() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    for (method, params, needle) in [
        ("ui.click", json!({"x": 1}), "`x` and `y`"),
        ("ui.click", json!({"x": "a", "y": 2}), "finite number"),
        ("ui.click", json!({"x": 1, "y": 2, "count": 99}), "count"),
        ("ui.click", json!({"x": 1, "y": 2, "cuont": 2}), "cuont"),
        ("ui.drag", json!({"x": 1, "y": 2}), "toX"),
        ("ui.drag", json!({"x": 1, "y": 2, "toX": 5, "toY": 5, "steps": 10000}), "steps"),
        ("ui.wheel", json!({"dy": 1}), "`x` and `y`"),
        ("ui.key", json!({"key": "nonsense"}), "key"),
        ("ui.text", json!({}), "text"),
        ("ui.move", json!({"element": 5}), "element"),
        ("ui.resize", json!({"width": 10, "height": 500}), "width"),
        ("ui.resize", json!({"width": 500}), "height"),
    ] {
        let error = call(&mut window, &tx, method, params).unwrap_err();
        assert!(error.contains(needle), "{method}: {error}");
    }
    assert_eq!(window.state().pending_input(), 0, "a refused request leaves nothing behind");
}

/// The queue has a limit, and a request that would go over it is refused whole.
#[test]
fn the_synthetic_queue_is_bounded() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    // Sent in one go: they are all answered in the same frame, before any of the queue is handed to egui.
    let answers: Vec<_> = (0..600).map(|_| send(&tx, "ui.click", json!({"x": 5, "y": 5, "count": 1}))).collect();
    // The window answers 64 requests a frame, so ten frames answer them all.
    for _ in 0..12 {
        window.step();
    }
    let (ok, refused): (Vec<_>, Vec<_>) = answers.into_iter().filter_map(|a| a.try_recv().ok()).partition(|r| r.is_ok());
    assert!(!refused.is_empty(), "600 clicks of 4 events each do not fit in 1,024");
    for r in refused {
        assert!(r.unwrap_err().contains("queue limit"));
    }
    assert!(ok.len() < 600);
    // The window carries on.
    for _ in 0..4 {
        window.step();
    }
    assert_eq!(window.state().ui_panics(), 0);
}

/// The window can be asked to take another size; here the harness has no desktop to ask, so only the request is checked.
#[test]
fn resize_sends_the_desktop_a_size() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    call(&mut window, &tx, "ui.resize", json!({"width": 500, "height": 400})).unwrap();
    let sent = crate::support::commands(&window);
    assert!(sent.iter().any(|c| matches!(c, egui::ViewportCommand::InnerSize(s) if s.x == 500.0 && s.y == 400.0)), "{sent:?}");
}

/// The window commands are listed with the engine's, and run through `engine.execute`.
#[test]
fn window_commands_run_through_engine_execute() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    let state = call(&mut window, &tx, "engine.execute", json!({"command": "window.tab", "params": {"tab": "insert"}})).unwrap();
    assert_eq!(state["tab"], "Insert");
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "window.tab", "params": {"tab": "Nope"}})).unwrap_err();
    assert!(error.contains("Home") && error.contains("Help"), "the error names the tabs: {error}");
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "window.theme", "params": {"theme": "pink"}})).unwrap_err();
    assert!(error.contains("light or dark"), "{error}");
    let state = call(&mut window, &tx, "engine.execute", json!({"command": "window.theme", "params": {"theme": "dark"}})).unwrap();
    assert_eq!(state["theme"], "dark");
    window.rest();
    let ui = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(ui["ui"]["theme"], "dark");
    assert_eq!(ui["ui"]["tab"], "Insert");
    // With no document the view commands say so, and the others still work.
    let (mut empty, tx) = controlled_with(|app| app);
    let error = call(&mut empty, &tx, "engine.execute", json!({"command": "view.zoomIn"})).unwrap_err();
    assert!(error.contains("no document open"), "{error}");
    call(&mut empty, &tx, "engine.execute", json!({"command": "window.ribbon", "params": {"collapsed": true}})).unwrap();
}

/// A drag is a press, a frame of moves, and the release in a frame of its own, so that a widget that waits for a drag sees one.
#[test]
fn a_drag_is_a_press_then_the_moves_then_the_release() {
    let (mut window, tx) = drawing_controlled();
    window.rest();
    call(&mut window, &tx, "ui.drag", json!({"x": 100, "y": 100, "toX": 300, "toY": 200, "steps": 4})).unwrap();
    let first = window.state_mut().next_input_frame();
    assert!(matches!(first.as_slice(), [Event::PointerMoved(_), Event::PointerButton { pressed: true, .. }]), "{first:?}");
    let moves = window.state_mut().next_input_frame();
    assert!(moves.len() == 4 && moves.iter().all(|e| matches!(e, Event::PointerMoved(_))), "{moves:?}");
    let release = window.state_mut().next_input_frame();
    assert!(matches!(release.as_slice(), [Event::PointerButton { pressed: false, .. }]), "{release:?}");
    assert_eq!(window.state().pending_input(), 0);
}
