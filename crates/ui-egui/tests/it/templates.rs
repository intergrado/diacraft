//! A drawing started from a template, as the window sees it (review round 1): the Shapes window lists the stencils the template opened and not the
//! ones the drawing before it had (M1), and a template that is a file is read on a thread, as a file to open is (L3). Every package is made by the
//! engine's own writer or built in code (`fixture.rs`); no template from another application is used.

use std::time::{Duration, Instant};

use diacraft_engine::template;
use egui::Key;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{edit_window_with, key};
use crate::fixture;
use crate::saving::ctrl;
use crate::stencil_files::{picker, rows};
use crate::support::{Rest, TempDir, Watchdog, controlled_with, send};

const WIDE: [f32; 2] = [1280.0, 900.0];

const BASIC: &str = "Basic Shapes";
const FLOWCHART: &str = "Basic Flowchart";

/// The bytes of a built-in template, as a `.vstx` file holds them.
fn template_file(dir: &TempDir, id: &str) -> std::path::PathBuf {
    let built = template::build("test", template::find(id).unwrap()).unwrap();
    let path = dir.file(&format!("{id}.vstx"));
    std::fs::write(&path, built.bytes).unwrap();
    path
}

/// The reviewer's first case: a new drawing from a template lists the stencils the template opened. The Shapes window kept the list it had read for
/// the drawing before, because it keyed that list on the stencil files alone.
#[test]
fn a_new_drawing_from_a_template_lists_the_stencils_the_template_opened() {
    let mut w = edit_window_with(WIDE, |app| app);
    assert_eq!(
        rows(&mut w),
        [BASIC, "Decorative Shapes", "Arrow Shapes", FLOWCHART, "Cross-Functional Flowchart Shapes", "Gane-Sarson", "Organization Chart Shapes"],
        "a blank drawing has all of them"
    );
    w.state_mut().execute("file.new", &json!({"template": "basic_diagram"})).unwrap();
    w.rest();
    assert_eq!(rows(&mut w), [BASIC, "Arrow Shapes"], "Basic Diagram opens the basic shapes and the arrows");
    w.state_mut().execute("file.new", &json!({"template": "basic_flowchart"})).unwrap();
    w.rest();
    assert_eq!(rows(&mut w), [FLOWCHART, BASIC], "Basic Flowchart opens the flowchart shapes first");
    w.state_mut().execute("file.new", &json!({})).unwrap();
    w.rest();
    assert_eq!(
        rows(&mut w),
        [BASIC, "Decorative Shapes", "Arrow Shapes", FLOWCHART, "Cross-Functional Flowchart Shapes", "Gane-Sarson", "Organization Chart Shapes"],
        "and a blank drawing has both again"
    );
    assert_eq!(w.state().ui_panics(), 0);
}

/// The reviewer's second case: a template file opened through the window narrows the list, and a new blank drawing (Ctrl+N) widens it again.
#[test]
fn a_new_blank_drawing_after_a_template_file_was_opened_lists_both_stencils_again() {
    let dir = TempDir::new("template-then-new");
    let file = template_file(&dir, "basic_diagram");
    let mut w = edit_window_with(WIDE, |app| app.with_file_picker(picker(file.clone())));
    assert_eq!(
        rows(&mut w),
        [BASIC, "Decorative Shapes", "Arrow Shapes", FLOWCHART, "Cross-Functional Flowchart Shapes", "Gane-Sarson", "Organization Chart Shapes"]
    );
    key(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(rows(&mut w), [BASIC, "Arrow Shapes"], "the template file opened its stencils");
    assert_eq!(w.state().session().stencils().open_builtins(), ["basic", "arrows"]);
    // Ctrl+N: the drawing from the template is clean, so nothing is asked.
    key(&mut w, Key::N, ctrl());
    assert!(!w.state().prompt_open());
    assert_eq!(
        w.state().session().stencils().open_builtins(),
        ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"],
        "the engine has all"
    );
    assert_eq!(
        rows(&mut w),
        [BASIC, "Decorative Shapes", "Arrow Shapes", FLOWCHART, "Cross-Functional Flowchart Shapes", "Gane-Sarson", "Organization Chart Shapes"],
        "and so does the window"
    );
    assert_eq!(w.state().ui_panics(), 0);
}

/// A template given by its id is small and made at once; one given as a file is read on a thread, so a large one never holds the window: frames are drawn
/// while it is read, the progress card says what is being opened, and the drawing is the template's when it ends.
#[test]
fn a_template_file_given_to_file_new_is_read_while_the_window_goes_on() {
    let _guard = Watchdog::new("the large template test", 120);
    let dir = TempDir::new("large-template");
    let path = dir.file("crowded.vstx");
    std::fs::write(&path, fixture::crowded_template(30_000)).unwrap();
    let (mut window, tx) = controlled_with(|app| app);
    window.rest();
    let before = window.state().window_title().to_string();
    let answer = send(&tx, "engine.execute", json!({"command": "file.new", "params": {"template": path.to_str().unwrap()}}));
    let (mut while_reading, mut card) = (0, false);
    let start = Instant::now();
    let reply = loop {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            break reply;
        }
        if window.state().opening_now().is_some() {
            while_reading += 1;
            card |= window.query_by_label_contains("Opening crowded.vstx").is_some();
        }
        assert!(start.elapsed() < Duration::from_secs(100), "the template was not read in time");
        std::thread::sleep(Duration::from_millis(2));
    };
    let answer = reply.unwrap();
    assert!(while_reading >= 3, "frames were drawn while the template was read: {while_reading}");
    assert!(card, "the progress card said what was being opened");
    window.step();
    window.step();
    assert!(window.state().opening_now().is_none());
    // The answer is `file.new`'s own, and the drawing is a new unsaved one made from the template.
    assert_eq!(answer["template"], json!({"id": null, "name": null, "builtin": false}), "{answer}");
    assert_eq!(answer["pages"][0]["name"], "Crowded");
    assert_eq!(window.state().session().document().unwrap().page(1).unwrap().shapes.len(), 30_000);
    assert!(window.state().session().path().is_none(), "no file of its own");
    assert_ne!(window.state().window_title(), before, "it is another drawing");
    // A template that is an id, in the same channel, is answered at once with no reading state.
    let again = send(&tx, "engine.execute", json!({"command": "file.new", "params": {"template": "basic_diagram"}}));
    window.step();
    assert!(window.state().opening_now().is_none());
    assert_eq!(again.try_recv().unwrap().unwrap()["template"]["id"], "basic_diagram");
    assert_eq!(window.state().ui_panics(), 0);
}

/// A template file that cannot be read is told in the engine's words, leaves the drawing as it was, and a stray key is refused before anything is read.
#[test]
fn a_template_file_that_is_refused_leaves_the_window_as_it_was() {
    let dir = TempDir::new("refused-template");
    let drawing = dir.file("plain.vsdx");
    std::fs::write(&drawing, fixture::crowded_vsdx(10)).unwrap();
    let (mut window, tx) = controlled_with(|app| app);
    window.rest();
    let run = |window: &mut crate::support::Window, params: serde_json::Value| {
        let answer = send(&tx, "engine.execute", json!({"command": "file.new", "params": params}));
        let start = Instant::now();
        loop {
            window.step();
            if let Ok(reply) = answer.try_recv() {
                return reply;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "not answered");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let revision = window.state().session().revision();
    let message = run(&mut window, json!({"template": drawing.to_str().unwrap()})).unwrap_err();
    assert!(message.contains("not a template") && message.contains("file.open"), "{message}");
    let message = run(&mut window, json!({"template": dir.file("missing.vstx").to_str().unwrap()})).unwrap_err();
    assert!(message.contains("no such file"), "{message}");
    let message = run(&mut window, json!({"template": dir.file("a.vstx").to_str().unwrap(), "bogus": 1})).unwrap_err();
    assert!(message.contains("unknown parameter `bogus`"), "{message}");
    let message = run(&mut window, json!({"template": 5})).unwrap_err();
    assert!(message.contains("must be text") || message.contains("template"), "{message}");
    assert_eq!(window.state().session().revision(), revision, "nothing changed");
    assert!(window.state().opening_now().is_none());
    assert_eq!(window.state().ui_panics(), 0);
}

/// N1 of the re-review: a template's answer carries the stencils it opened, and the Shapes window then shows the first of them and not the stencil the
/// drawing before it showed; a blank drawing and a file that is not a template leave the stencil shown alone.
#[test]
fn a_template_shows_its_first_stencil_and_a_plain_file_or_a_blank_drawing_leaves_the_one_shown() {
    let dir = TempDir::new("shown-stencil");
    let plain = dir.file("plain.vsdx");
    std::fs::write(&plain, fixture::crowded_vsdx(5)).unwrap();
    let flowchart_file = template_file(&dir, "basic_flowchart");
    let mut w = edit_window_with(WIDE, |app| app.with_file_picker(picker(plain.clone())));
    assert!(w.state_mut().show_stencil("basic"));
    assert_eq!(w.state_mut().current_stencil().as_deref(), Some("basic"));
    // Click Basic Shapes, start Basic Flowchart: the flowchart stencil is shown.
    w.state_mut().execute("file.new", &json!({"template": "basic_flowchart"})).unwrap();
    w.rest();
    assert_eq!(w.state_mut().current_stencil().as_deref(), Some("flowchart"), "the template's first stencil");
    // A blank drawing and a plain file leave what is shown alone.
    assert!(w.state_mut().show_stencil("basic"));
    w.state_mut().execute("file.new", &json!({})).unwrap();
    w.rest();
    assert_eq!(w.state_mut().current_stencil().as_deref(), Some("basic"), "a blank drawing does not change it");
    key(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "plain.vsdx - DiaCraft", "the plain file was read");
    assert_eq!(w.state_mut().current_stencil().as_deref(), Some("basic"), "nor does a file that is not a template");
    // A template file opened through the window shows its first stencil too.
    assert!(w.state_mut().show_stencil("basic"));
    let mut w = edit_window_with(WIDE, |app| app.with_file_picker(picker(flowchart_file.clone())));
    assert!(w.state_mut().show_stencil("basic"));
    key(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state_mut().current_stencil().as_deref(), Some("flowchart"));
    assert_eq!(w.state().ui_panics(), 0);
}

/// N2 of the re-review: a template opened as a file is a new unnamed drawing, counted as one as `file.new` counts it.
#[test]
fn a_template_file_opened_is_counted_as_another_drawing_and_a_plain_file_is_not() {
    let dir = TempDir::new("counted");
    let file = template_file(&dir, "basic_diagram");
    let plain = dir.file("plain.vsdx");
    std::fs::write(&plain, fixture::crowded_vsdx(5)).unwrap();
    let mut w = edit_window_with(WIDE, |app| app.with_file_picker(picker(file.clone())));
    assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft");
    key(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "Drawing2 - DiaCraft", "the template made a new drawing");
    key(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "Drawing3 - DiaCraft", "and again");
    // The same through the control channel, a template file for `file.new` included.
    let (mut window, tx) = controlled_with(|app| app);
    window.rest();
    let started = |window: &crate::support::Window| window.state().window_title();
    let answer = send(&tx, "engine.execute", json!({"command": "file.new", "params": {"template": file.to_str().unwrap()}}));
    let start = Instant::now();
    let reply = loop {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            break reply;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "not answered");
        std::thread::sleep(Duration::from_millis(2));
    };
    reply.unwrap();
    assert_eq!(started(&window), "Drawing1 - DiaCraft");
    let answer = send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": file.to_str().unwrap()}}));
    let start = Instant::now();
    let reply = loop {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            break reply;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "not answered");
        std::thread::sleep(Duration::from_millis(2));
    };
    reply.unwrap();
    assert_eq!(started(&window), "Drawing2 - DiaCraft", "and `file.open` of the same file is another");
    assert_eq!(window.state().ui_panics(), 0);
}

/// N3 of the re-review: a word that is neither a template nor written as a path is answered at once by the engine, and is not sent to a thread to be
/// refused there.
#[test]
fn a_typo_of_a_template_name_is_answered_at_once_and_not_read_as_a_file() {
    let (mut window, tx) = controlled_with(|app| app);
    window.rest();
    for typo in ["basic_flowchrt", "Basic Flowchat", "flowchart", ""] {
        let answer = send(&tx, "engine.execute", json!({"command": "file.new", "params": {"template": typo}}));
        let mut reading = false;
        let mut reply = None;
        for _ in 0..4 {
            window.step();
            reading |= window.state().opening_now().is_some();
            if reply.is_none() {
                reply = answer.try_recv().ok();
            }
        }
        assert!(!reading, "{typo:?}: no file was being read");
        let message = reply.unwrap_or_else(|| panic!("{typo:?}: not answered in four frames")).unwrap_err();
        assert!(message.contains("no template") && message.contains("template.list"), "{typo:?}: {message}");
    }
    assert_eq!(window.state().ui_panics(), 0);
}
