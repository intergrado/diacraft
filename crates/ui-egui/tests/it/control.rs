//! The window's side of the control channel: what each method does, what a hostile request gets, the
//! per-frame budget, and the screenshot's two ends (a frame that arrives and one that never does).

use std::sync::Arc;
use std::time::Duration;

use diacraft_ui_egui::control::{MAX_REQUESTS_PER_FRAME, METHODS, QUIT_DELAY};
use diacraft_ui_egui::{ControlRequest, Stage};
use egui::{Color32, ColorImage, ViewportCommand};
use serde_json::{Value, json};

use crate::support::{Rest, TempDir, call, commands, controlled, controlled_with, send};

#[test]
fn execute_runs_a_command_and_bumps_revision() {
    let (mut window, tx) = controlled();
    let before = window.state().session().revision();
    let made = call(&mut window, &tx, "engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    assert_eq!(made["pages"], json!([{"id": 1, "name": "Page-1"}]));
    let after_new = window.state().session().revision();
    assert!(after_new > before, "file.new moves the revision: {before} -> {after_new}");

    let added = call(&mut window, &tx, "engine.execute", json!({"command": "page.add", "params": {"name": "Second"}})).unwrap();
    assert_eq!(added, json!({"id": 2, "name": "Second"}));
    assert!(window.state().session().revision() > after_new, "page.add moves it again");
    // The change is the document's, so the window shows it.
    window.step();
    egui_kittest::kittest::Queryable::get_by_label(&window, "Page 1 of 2");
    assert_eq!(window.state().session().document().unwrap().pages.len(), 2);

    // `params` may be left out, as the protocol says.
    call(&mut window, &tx, "engine.execute", json!({"command": "edit.undo"})).unwrap();
    assert_eq!(window.state().session().document().unwrap().pages.len(), 1);
}

#[test]
fn shape_formatting_answers_for_a_range_through_engine_execute() {
    // The window asks for the state of the characters selected in a text box with `start` and `end`, as any program on the channel can.
    let (mut window, tx) = controlled();
    let run = |window: &mut crate::support::Window, command: &str, params: Value| {
        call(window, &tx, "engine.execute", json!({"command": command, "params": params})).unwrap()
    };
    run(&mut window, "file.new", json!({}));
    run(&mut window, "shape.drop", json!({"master": "Rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut window, "shape.set_text", json!({"id": 1, "text": "hello world"}));
    run(&mut window, "text.format", json!({"ids": [1], "start": 6, "end": 11, "italic": true}));
    let revision = window.state().session().revision();

    let world = run(&mut window, "shape.formatting", json!({"ids": [1], "start": 6, "end": 11}));
    assert_eq!((&world["shapes"][0]["italic"], &world["shapes"][0]["mixed"]), (&json!(true), &json!([])), "{world}");
    let whole = run(&mut window, "shape.formatting", json!({"ids": [1], "start": 0, "end": 11}));
    assert_eq!((&whole["shapes"][0]["italic"], &whole["shapes"][0]["mixed"]), (&json!("mixed"), &json!(["italic"])), "{whole}");
    // A point at the end of `hello` is the run before it, which is not italic.
    let point = run(&mut window, "shape.formatting", json!({"ids": [1], "start": 5, "end": 5}));
    assert_eq!(point["shapes"][0]["italic"], false, "{point}");
    assert_eq!(window.state().session().revision(), revision, "a query moves nothing");

    // Bad offsets are an error with the engine's words, and change nothing.
    let error =
        call(&mut window, &tx, "engine.execute", json!({"command": "shape.formatting", "params": {"ids": [1], "start": 4, "end": 99}})).unwrap_err();
    assert!(error.contains("not within the text, which has 11 characters"), "{error}");
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "shape.formatting", "params": {"ids": [1], "end": 3}})).unwrap_err();
    assert!(error.contains("go together"), "{error}");
    assert_eq!(window.state().session().revision(), revision);
}

#[test]
fn a_command_that_fails_is_an_error_and_changes_nothing() {
    let (mut window, tx) = controlled();
    let before = window.state().session().revision();
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "page.add", "params": {}})).unwrap_err();
    assert!(error.contains("no document open"), "{error}");
    assert_eq!(window.state().session().revision(), before);
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "no.such.command"})).unwrap_err();
    assert!(error.contains("unknown command"), "{error}");
    // An unknown parameter of the command is the engine's error, and it names the nearest key.
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    let error = call(&mut window, &tx, "engine.execute", json!({"command": "page.add", "params": {"nmae": "x"}})).unwrap_err();
    assert!(error.contains("nmae") && error.contains("name"), "{error}");
}

#[test]
fn unknown_method_is_an_error() {
    let (mut window, tx) = controlled();
    let error = call(&mut window, &tx, "engine.explode", json!({})).unwrap_err();
    assert!(error.contains("unknown method") && error.contains("engine.explode"), "{error}");
    // It says what the methods are, so a client can correct itself.
    for method in METHODS {
        assert!(error.contains(method), "{method} is not listed in: {error}");
    }
    assert_eq!(
        METHODS,
        [
            "engine.execute",
            "engine.commands",
            "document.inspect",
            "ui.inspect",
            "ui.screenshot",
            "ui.elements",
            "ui.click",
            "ui.drag",
            "ui.release",
            "ui.move",
            "ui.wheel",
            "ui.key",
            "ui.text",
            "ui.resize",
            "app.quit"
        ]
    );
}

/// The method name is the client's, so it is escaped and cut short before it is echoed.
#[test]
fn an_unknown_method_name_is_echoed_escaped_and_short() {
    let (mut window, tx) = controlled();
    let method = format!("bad\u{1b}[2J\n{}", "m".repeat(1_000_000));
    let error = call(&mut window, &tx, &method, json!({})).unwrap_err();
    assert!(!error.chars().any(char::is_control), "{error:?}");
    assert!(error.chars().count() < 600, "{} characters", error.chars().count());
    assert!(error.contains("bad\\u{1b}[2J\\n"), "{error}");
}

#[test]
fn inspect_names_the_app() {
    let (mut window, tx) = controlled();
    let info = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(info["app"], "diacraft");
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    let frames = info["frames"].as_u64().unwrap();
    assert!(frames >= 1, "{info}");
    assert_eq!(info["status"], "No document open");
    assert_eq!(info["document"], Value::Null);
    assert_eq!(info["window"], json!([900.0, 600.0]));
    assert_eq!(info["revision"], 0);
    assert_eq!(info["panics"], json!({"engine": 0, "ui": 0}));

    window.step();
    window.step();
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    let later = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert!(later["frames"].as_u64().unwrap() > frames, "frames moves with the window: {frames} -> {}", later["frames"]);
    assert_eq!(later["status"], "Drawing1 · 1 page");
    assert_eq!(later["title"], "Drawing1 - DiaCraft");
    assert_eq!(later["document"], json!({"title": "Untitled", "pages": 1}));
    assert!(later["revision"].as_u64().unwrap() > 0);
}

#[test]
fn commands_lists_the_registry_with_what_can_run_now() {
    let (mut window, tx) = controlled();
    let list = call(&mut window, &tx, "engine.commands", json!({})).unwrap();
    let find = |list: &Value, id: &str| list.as_array().unwrap().iter().find(|c| c["id"] == id).cloned().unwrap_or_else(|| panic!("{id} not listed"));
    assert_eq!(find(&list, "file.new")["enabled"], true);
    let inspect = find(&list, "document.inspect");
    assert_eq!(inspect["enabled"], false);
    assert_eq!(inspect["disabled_reason"], "no document open");
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    let list = call(&mut window, &tx, "engine.commands", json!({})).unwrap();
    assert_eq!(find(&list, "document.inspect")["enabled"], true);
    // The engine's commands and then the window's own.
    assert_eq!(list.as_array().unwrap().len(), diacraft_engine::command_specs().len() + diacraft_ui_egui::commands::UI_COMMANDS.len());
    assert_eq!(find(&list, "view.zoomIn")["enabled"], true);
}

#[test]
fn document_inspect_reads_the_document() {
    let (mut window, tx) = controlled();
    let error = call(&mut window, &tx, "document.inspect", json!({})).unwrap_err();
    assert!(error.contains("no document open"), "{error}");
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    let doc = call(&mut window, &tx, "document.inspect", json!({})).unwrap();
    assert_eq!(doc["title"], "Untitled");
    assert_eq!(doc["pages"].as_array().unwrap().len(), 1);
}

/// Every method takes a fixed set of keys. One it does not know is an error, not a default: a client
/// that misspells `params` must hear about it.
#[test]
fn unknown_keys_and_wrong_shapes_are_errors() {
    let (mut window, tx) = controlled();
    let bad: [(&str, Value, &str); 12] = [
        ("engine.execute", json!({"command": "file.new", "parms": {}}), "parms"),
        ("engine.execute", json!({"command": "file.new", "params": {}, "extra": 1}), "extra"),
        ("engine.execute", json!({}), "command"),
        ("engine.execute", json!({"command": 5}), "command"),
        ("engine.execute", json!({"command": "file.new", "params": [1]}), "params"),
        ("engine.execute", json!({"command": "file.new", "params": "x"}), "params"),
        ("engine.execute", json!([1, 2]), "object"),
        ("engine.commands", json!({"x": 1}), "x"),
        ("document.inspect", json!({"x": 1}), "x"),
        ("ui.inspect", json!({"x": 1}), "x"),
        ("ui.screenshot", json!({"path": "/x.png", "x": 1}), "x"),
        ("app.quit", json!({"x": 1}), "x"),
    ];
    for (method, params, mention) in bad {
        let error = call(&mut window, &tx, method, params.clone()).unwrap_err();
        assert!(error.contains(mention), "{method} {params}: {error}");
    }
    // None of it did anything.
    assert!(window.state().session().document().is_none());
    assert!(!window.state().quit_requested());
    // `null` and an absent `params` are the same as `{}`.
    call(&mut window, &tx, "ui.inspect", Value::Null).unwrap();
}

#[test]
fn at_most_64_requests_run_per_frame() {
    assert_eq!(MAX_REQUESTS_PER_FRAME, 64);
    let (mut window, tx) = controlled();
    let answers: Vec<_> = (0..100).map(|_| send(&tx, "ui.inspect", json!({}))).collect();
    window.step();
    let answered = answers.iter().filter(|a| a.try_recv().is_ok()).count();
    assert_eq!(answered, 64, "one frame answers 64");
    // The rest wait on the channel, and the window asks for another frame to serve them.
    window.step();
    let rest = answers.iter().filter(|a| a.try_recv().is_ok()).count();
    assert_eq!(rest, 36, "the next frame answers the other 36");
}

/// A request whose caller gave up before the window took it is dropped unrun, so an edit does not land
/// later, after the caller was told it did not happen.
#[test]
fn a_withdrawn_request_never_runs() {
    let (mut window, tx) = controlled();
    let (request, answer) = ControlRequest::new("engine.execute", json!({"command": "file.new"}));
    let ticket = request.ticket();
    tx.send(request).unwrap();
    assert!(ticket.give_up(), "not started yet, so it can be withdrawn");
    window.step();
    window.step();
    assert!(window.state().session().document().is_none(), "the withdrawn request ran");
    assert!(answer.try_recv().is_err());

    // One the window has taken cannot be withdrawn.
    let (request, answer) = ControlRequest::new("engine.execute", json!({"command": "file.new"}));
    let ticket = request.ticket();
    tx.send(request).unwrap();
    window.step();
    assert!(!ticket.give_up(), "already taken");
    assert!(answer.try_recv().unwrap().is_ok());
    // A withdrawn request does not use up the frame's budget of 64.
    let mut withdrawn = Vec::new();
    for _ in 0..100 {
        let (request, answer) = ControlRequest::new("ui.inspect", json!({}));
        assert!(request.ticket().give_up());
        tx.send(request).unwrap();
        withdrawn.push(answer);
    }
    let answers: Vec<_> = (0..64).map(|_| send(&tx, "ui.inspect", json!({}))).collect();
    window.step();
    assert_eq!(answers.iter().filter(|a| a.try_recv().is_ok()).count(), 64, "the 100 withdrawn ones cost the frame nothing");
}

#[test]
fn a_request_that_panics_is_answered_and_the_window_goes_on() {
    let (mut window, tx) = controlled_with(|app| {
        app.with_probe(|stage| {
            if matches!(stage, Stage::Request("ui.inspect")) {
                panic!("boom in a request\u{1b}[31m");
            }
        })
    });
    let error = call(&mut window, &tx, "ui.inspect", json!({})).unwrap_err();
    assert!(error.contains("internal error") && error.contains("boom in a request"), "{error}");
    // Escaped on the way, once: the reply is JSON, but a client prints it, and the window shows it too.
    assert!(!error.chars().any(char::is_control), "{error:?}");
    assert!(error.contains("boom in a request\\u{1b}[31m") && !error.contains("\\\\u{1b}"), "escaped once: {error}");
    let notice = window.state().notice().unwrap();
    assert!(!notice.chars().any(char::is_control), "{notice:?}");
    assert!(notice.contains("boom in a request\\u{1b}[31m") && !notice.contains("\\\\u{1b}"), "escaped once: {notice}");
    assert_eq!(window.state().ui_panics(), 1);
    // The inbox survived: the next request is served.
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    assert!(window.state().session().document().is_some());
}

/// Whether the last frame asked for the window to close.
fn closes(window: &crate::support::Window) -> bool {
    commands(window).contains(&ViewportCommand::Close)
}

/// The quit delay is the window's clock's (`clock.rs`), counted from the frame that took the request: the test lets it pass by advancing the clock, and sees the
/// close by that time and by frames, never by the machine's time. (A test that measured it in real time, from a moment after that frame, failed on a busy
/// machine: 26 runs of 200 on one CPU with three busy loops beside it, "closed after 6 frames and 146.79ms, before its 150ms".)
#[test]
fn quit_answers_first_and_closes_the_window_a_little_later() {
    let (mut window, tx) = controlled();
    let answer = send(&tx, "app.quit", json!({}));
    window.step();
    assert_eq!(answer.try_recv().unwrap(), Ok(Value::Null), "the reply goes out before the window closes");
    assert!(window.state().quit_requested());
    assert!(!closes(&window), "not in the frame that answered: the reply needs time to leave the process");
    // The window's clock stands still until the test advances it, so frames alone close nothing, however many there are and however long the machine takes.
    for frame in 1..=20 {
        window.step();
        assert!(!closes(&window), "frame {frame}: no time has passed by the window's clock");
    }
    // A millisecond short of the delay, the window is open; at the delay, the next frame asks to close.
    window.advance(QUIT_DELAY - Duration::from_millis(1));
    window.step();
    assert!(!closes(&window), "a millisecond before the delay is up");
    assert!(window.state().quit_requested());
    window.advance(Duration::from_millis(1));
    window.step();
    assert!(closes(&window), "the delay is up");
    assert!(window.state().quit_requested(), "still reported as quitting");
    // Once is enough: it does not ask again, whatever time goes by or is asked again.
    window.step();
    assert!(!closes(&window));
    let again = send(&tx, "app.quit", json!({}));
    window.advance(QUIT_DELAY * 4);
    window.step();
    assert_eq!(again.try_recv().unwrap(), Ok(Value::Null));
    window.advance(QUIT_DELAY * 4);
    window.step();
    assert!(!closes(&window), "a second `app.quit` after the close does not start the delay again");
}

/// A second `app.quit` while the window waits does not push the close later: the delay runs from the first.
#[test]
fn a_second_quit_does_not_move_the_close_later() {
    let (mut window, tx) = controlled();
    send(&tx, "app.quit", json!({}));
    window.step();
    window.advance(QUIT_DELAY / 2);
    let second = send(&tx, "app.quit", json!({}));
    window.step();
    assert_eq!(second.try_recv().unwrap(), Ok(Value::Null), "it is answered");
    assert!(!closes(&window), "half the delay has passed");
    window.advance(QUIT_DELAY / 2);
    window.step();
    assert!(closes(&window), "the delay of the first request is up, and the second did not start a new one");
}

// ---- screenshots ----

/// Steps until the window has asked for a capture, and returns its callback.
fn capture_request(window: &mut crate::support::Window) -> egui::ScreenshotCallback {
    for _ in 0..20 {
        window.step();
        if let Some(callback) = commands(window).into_iter().find_map(|c| if let ViewportCommand::Screenshot(cb) = c { Some(cb) } else { None }) {
            return callback;
        }
    }
    panic!("the window never asked for a screenshot")
}

fn read_png(path: &std::path::Path) -> (png::OutputInfo, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info, pixels)
}

/// A frame the integration presents is written as a PNG with the pixels it holds, and the reply says where.
#[test]
fn a_presented_frame_is_written_as_a_png() {
    let dir = TempDir::new("shot");
    let path = dir.file("shot.png");
    let (mut window, tx) = controlled();
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    let callback = capture_request(&mut window);
    assert!(answer.try_recv().is_err(), "not answered before the frame arrives");
    assert!(!path.exists());

    // The integration hands over a frame: 3 × 2 pixels, each different, one see-through.
    let pixels = vec![
        Color32::from_rgb(255, 0, 0),
        Color32::from_rgb(0, 255, 0),
        Color32::from_rgb(0, 0, 255),
        Color32::from_rgb(10, 20, 30),
        Color32::from_rgb(200, 210, 220),
        Color32::from_rgb(1, 2, 3),
    ];
    callback.complete(Arc::new(ColorImage::new([3, 2], pixels)));
    for _ in 0..5 {
        window.step();
        if answer.try_recv().is_ok_and(|reply| {
            let reply = reply.unwrap();
            assert_eq!(reply, json!({"path": path.to_str().unwrap(), "width": 3, "height": 2}));
            true
        }) {
            let (info, bytes) = read_png(&path);
            assert_eq!((info.width, info.height, info.color_type, info.bit_depth), (3, 2, png::ColorType::Rgba, png::BitDepth::Eight));
            assert_eq!(
                bytes,
                [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255, 200, 210, 220, 255, 1, 2, 3, 255],
                "the pixels in the file are the pixels of the frame, row by row"
            );
            return;
        }
    }
    panic!("the screenshot was never answered after the frame arrived");
}

/// With no frame presented (a locked screen, a window nobody can see) the capture never comes. The
/// request is answered with an error once its time is up, and no file is left.
#[test]
fn screenshot_without_a_frame_is_an_error_within_timeout() {
    let dir = TempDir::new("noshot");
    let path = dir.file("never.png");
    let timeout = Duration::from_millis(600);
    let (mut window, tx) = controlled_with(|app| app.with_screenshot_timeout(timeout));
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    // The frame that takes the request starts the time limit, by the window's clock; frames alone do not run it out (the clock stands still).
    window.step();
    for frame in 1..=20 {
        window.step();
        assert!(answer.try_recv().is_err(), "frame {frame}: no time has passed by the window's clock, so the request still waits for its frame");
    }
    // Not earlier than asked (a refusal at once would pass a looser test): a millisecond short, it waits, and at its time it is answered.
    window.advance(timeout - Duration::from_millis(1));
    window.step();
    assert!(answer.try_recv().is_err(), "a millisecond before its {timeout:?}");
    window.advance(Duration::from_millis(1));
    window.step();
    let reply = answer.try_recv().expect("answered when its time is up");
    let error = reply.expect_err("a screenshot cannot succeed when no frame is ever presented (is the test harness rendering?)");
    assert!(error.contains("no frame") && error.contains("screen"), "{error}");
    assert!(!path.exists(), "an error left a file");
    // The window is free for the next request, and that one waits for its own full time, counted from its own request.
    call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    let again = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    window.step();
    assert!(again.try_recv().is_err(), "a new request is not answered with the old one's timeout");
    window.advance(timeout - Duration::from_millis(1));
    window.step();
    assert!(again.try_recv().is_err(), "a millisecond before its own {timeout:?}");
    window.advance(Duration::from_millis(1));
    window.step();
    assert!(again.try_recv().expect("answered when its own time is up").is_err());
}

/// A frame that arrives after the request's time is up is not used.
#[test]
fn a_frame_that_arrives_late_writes_nothing() {
    let dir = TempDir::new("late");
    let path = dir.file("late.png");
    let (mut window, tx) = controlled_with(|app| app.with_screenshot_timeout(Duration::from_millis(300)));
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    let callback = capture_request(&mut window);
    // The request's time is up by the window's clock.
    window.advance(Duration::from_millis(300));
    window.step();
    let reply = answer.try_recv().expect("answered when its time is up");
    assert!(reply.unwrap_err().contains("no frame"));
    // The integration hands over the frame after all.
    callback.complete(Arc::new(ColorImage::new([1, 1], vec![Color32::from_rgb(1, 2, 3)])));
    window.step();
    window.step();
    assert!(!path.exists(), "a late frame was written");
    assert!(answer.try_recv().is_err(), "a request is answered once");
}

/// A window nobody can see (minimised, covered, on another desktop) runs no frame, so none would be painted
/// to capture. A pending screenshot asks for the frame anyway, on every pass until it is answered.
#[test]
fn a_pending_screenshot_asks_for_a_frame_even_while_the_window_is_hidden() {
    let dir = TempDir::new("hidden");
    let (mut window, tx) = controlled();
    let hidden_paint = |window: &crate::support::Window| commands(window).contains(&ViewportCommand::RequestPaintWhileHidden);
    window.step();
    assert!(!hidden_paint(&window), "nothing asks for a frame while no screenshot is pending");
    let answer = send(&tx, "ui.screenshot", json!({"path": dir.file("h.png").to_str().unwrap()}));
    window.step();
    assert!(hidden_paint(&window), "the pass that took the request asks");
    let callback = commands(&window)
        .into_iter()
        .find_map(|c| if let ViewportCommand::Screenshot(cb) = c { Some(cb) } else { None })
        .expect("the capture is asked for in the pass that took the request, not a frame later");
    assert!(answer.try_recv().is_err());
    window.step();
    assert!(hidden_paint(&window), "and the passes after it, until the frame comes");
    assert!(!commands(&window).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))), "the capture is asked for once");
    // The frame comes: the answer is sent, and the asking stops.
    callback.complete(Arc::new(ColorImage::new([1, 1], vec![Color32::from_rgb(1, 2, 3)])));
    window.step();
    answer.try_recv().expect("answered in the pass after the frame came").expect("and the answer is a success");
    window.step();
    assert!(!hidden_paint(&window), "nothing asks once it is answered");
}

/// A timeout so long that the time it ends at does not exist must not panic the window (adding it to the clock
/// would): the request just waits for its frame.
#[test]
fn a_timeout_too_long_to_add_to_the_clock_does_not_panic() {
    let dir = TempDir::new("forever");
    let (mut window, tx) = controlled_with(|app| app.with_screenshot_timeout(Duration::MAX));
    // The window's clock is past zero, as the machine's always is: only then does the time the request ends at not exist.
    window.advance(Duration::from_secs(1));
    let answer = send(&tx, "ui.screenshot", json!({"path": dir.file("f.png").to_str().unwrap()}));
    window.step();
    window.step();
    assert!(answer.try_recv().is_err(), "waiting for its frame");
    assert_eq!(window.state().ui_panics(), 0, "nothing panicked");
    let info = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(info["screenshotPending"], true);
}

#[test]
fn only_one_screenshot_is_in_flight() {
    let dir = TempDir::new("two");
    let (mut window, tx) = controlled();
    let first = send(&tx, "ui.screenshot", json!({"path": dir.file("a.png").to_str().unwrap()}));
    let second = send(&tx, "ui.screenshot", json!({"path": dir.file("b.png").to_str().unwrap()}));
    window.step();
    let error = second.try_recv().expect("the second is refused at once").unwrap_err();
    assert!(error.contains("already") && error.contains("screenshot"), "{error}");
    assert!(first.try_recv().is_err(), "the first is still waiting for its frame");
}

/// The path is the client's: it must be a PNG file in a folder that exists, given in full.
#[test]
fn a_screenshot_path_must_be_an_absolute_png_in_an_existing_folder() {
    let dir = TempDir::new("paths");
    let (mut window, tx) = controlled();
    let missing_folder = dir.file("nowhere/shot.png");
    let cases: Vec<(Value, &str)> = vec![
        (json!({}), "path"),
        (json!({"path": 5}), "path"),
        (json!({"path": ""}), "path"),
        (json!({"path": "shot.png"}), "absolute"),
        (json!({"path": "../shot.png"}), "absolute"),
        (json!({"path": dir.file("shot.jpg").to_str().unwrap()}), ".png"),
        (json!({"path": dir.file("shot").to_str().unwrap()}), ".png"),
        (json!({"path": missing_folder.to_str().unwrap()}), "folder"),
        (json!({"path": format!("{}/bad\u{0}name.png", dir.path().display())}), "control"),
        (json!({"path": dir.path().to_str().unwrap()}), ".png"),
    ];
    for (params, mention) in cases {
        let error = call(&mut window, &tx, "ui.screenshot", params.clone()).unwrap_err();
        assert!(error.contains(mention), "{params}: {error}");
        assert!(!error.chars().any(char::is_control), "{error:?}");
    }
    // None of them started a capture.
    window.step();
    assert!(!commands(&window).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))));
}

// ---- what the screenshot is written to ----

#[cfg(unix)]
fn mkfifo(path: &std::path::Path) {
    let made = std::process::Command::new("mkfifo").arg(path).status().expect("mkfifo is needed to make a named pipe");
    assert!(made.success(), "mkfifo failed");
}

// The frame the tests of a pipe and of a file replaced whole complete a capture with: they are Unix's.
#[cfg(unix)]
fn frame_of_one_pixel() -> Arc<ColorImage> {
    Arc::new(ColorImage::new([1, 1], vec![Color32::from_rgb(9, 8, 7)]))
}

/// A path the client names that is already something other than a regular file (a folder with a `.png` name, a
/// named pipe, a link) is refused at once, before any frame is captured. Writing to a named pipe waits for a reader,
/// and the window writes on the thread that draws it.
#[test]
fn a_path_that_exists_and_is_not_a_regular_file_is_refused_before_the_capture() {
    let dir = TempDir::new("irregular");
    let (mut window, tx) = controlled();
    std::fs::create_dir(dir.file("folder.png")).unwrap();
    // A pipe and a link are added on Unix only.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut paths = vec![dir.file("folder.png")];
    #[cfg(unix)]
    {
        mkfifo(&dir.file("pipe.png"));
        std::fs::write(dir.file("real.png"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.file("real.png"), dir.file("link.png")).unwrap();
        paths.extend([dir.file("pipe.png"), dir.file("link.png")]);
    }
    for path in paths {
        let error = call(&mut window, &tx, "ui.screenshot", json!({"path": path.to_str().unwrap()})).unwrap_err();
        assert!(error.contains("regular file"), "{}: {error}", path.display());
        assert!(!error.chars().any(char::is_control), "{error:?}");
    }
    window.step();
    assert!(!commands(&window).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))), "a capture was asked for");
}

/// The path was fine when asked, and is a named pipe when the frame arrives: the file is replaced, never opened,
/// so the window does not block. (The watchdog turns a blocked window into a failure instead of a hang.)
#[cfg(unix)]
#[test]
fn a_file_that_becomes_a_pipe_before_the_frame_arrives_does_not_block_the_window() {
    let dir = TempDir::new("becomes-pipe");
    let path = dir.file("later.png");
    let (mut window, tx) = controlled();
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    let callback = capture_request(&mut window);
    mkfifo(&path);
    let _watchdog = crate::support::Watchdog::new("writing a screenshot over a named pipe", 20);
    callback.complete(frame_of_one_pixel());
    for _ in 0..3 {
        window.step();
    }
    let reply = answer.try_recv().expect("answered, not blocked").expect("the pipe was replaced by the picture");
    assert_eq!(reply["width"], 1);
    assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_file(), "a regular file now");
    assert_eq!(read_png(&path).1, [9, 8, 7, 255]);
}

/// The file is written whole beside its destination and renamed over it: the old file is never opened and cut in
/// place (a hard link to it keeps the old bytes), and no staging file is left.
#[cfg(unix)]
#[test]
fn a_screenshot_replaces_its_file_whole_and_leaves_nothing_else_behind() {
    let dir = TempDir::new("replace");
    let path = dir.file("shot.png");
    std::fs::write(&path, b"the old picture").unwrap();
    std::fs::hard_link(&path, dir.file("alias.png")).unwrap();
    let (mut window, tx) = controlled();
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    capture_request(&mut window).complete(frame_of_one_pixel());
    for _ in 0..3 {
        window.step();
    }
    answer.try_recv().unwrap().unwrap();
    assert_eq!(read_png(&path).1, [9, 8, 7, 255]);
    assert_eq!(std::fs::read(dir.file("alias.png")).unwrap(), b"the old picture", "the old file was cut in place");
    let mut names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["alias.png", "shot.png"], "a staging file was left");
}

/// When the file cannot be written the answer is an error and the old file is as it was. (A folder that takes no
/// new file stands in for a full disk: writing in place would still have succeeded.)
#[cfg(unix)]
#[test]
fn a_screenshot_that_cannot_be_written_leaves_the_old_file_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("readonly");
    let path = dir.file("keep.png");
    std::fs::write(&path, b"old bytes").unwrap();
    let (mut window, tx) = controlled();
    let answer = send(&tx, "ui.screenshot", json!({"path": path.to_str().unwrap()}));
    let callback = capture_request(&mut window);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let restore = |mode| std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    if std::fs::write(dir.file("probe"), b"").is_ok() {
        // A user who can write anywhere (root) cannot make this failure.
        restore(0o755);
        eprintln!("skipped: this user can write into a folder with no write permission");
        return;
    }
    callback.complete(frame_of_one_pixel());
    for _ in 0..3 {
        window.step();
    }
    let reply = answer.try_recv();
    restore(0o755);
    let error = reply.expect("answered").expect_err("the folder takes no new file, so there is nowhere to stage one");
    assert!(error.contains("cannot write"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), b"old bytes");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "a staging file was left");
}
