//! Opening a file in the window: its pages, its picture, a file that will not open, the Open dialog, and the work done off the
//! window's own thread. Every file is built in code (`fixture.rs`).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_ui_egui::render_worker::{Drawer, Job, MakeDrawer, bundled_drawer};
use diacraft_ui_egui::{ControlRequest, DiacraftApp, FilePicker};
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::fixture;
use crate::support::{Rest, TempDir, Watchdog, Window, call, click, controlled_with, elements_named, send, window};

/// A window with the renderer on its own thread's job done inline, so a picture is there in the frame that asked for it.
fn viewer() -> (Window, Sender<ControlRequest>) {
    controlled_with(|app| app.with_inline_pictures(bundled_drawer()))
}

/// Runs `file.open` for `path` through the control channel and steps the window until the answer comes (the file is read on a thread).
fn open(window: &mut Window, tx: &Sender<ControlRequest>, path: &Path) -> Result<Value, String> {
    let answer = send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            window.rest();
            return reply;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened in 30 s");
}

fn page_names(window: &Window) -> Vec<String> {
    window.state().session().document().unwrap().pages.iter().map(|p| p.name.to_string()).collect()
}

fn tab_order(window: &Window) -> Vec<String> {
    let mut tabs = elements_named(window, "page-tab.");
    tabs.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    tabs.into_iter().map(|(n, ..)| n).collect()
}

/// Steps until `done` holds, or fails after `secs`.
fn step_until(window: &mut Window, secs: u64, what: &str, done: impl Fn(&Window) -> bool) {
    let start = Instant::now();
    while !done(window) {
        assert!(start.elapsed() < Duration::from_secs(secs), "{what} did not happen in {secs} s");
        window.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The tab row shows the file's own pages with their own names, foreground pages first and background pages after them; the status bar
/// counts in that order; the title is the file's name.
#[test]
fn a_file_opens_with_its_pages_named_as_it_names_them() {
    let dir = TempDir::new("pages");
    let path = fixture::write(dir.path(), "floor plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    let reply = open(&mut window, &tx, &path).unwrap();
    // The file lists its background page second; the model keeps the file's order.
    assert_eq!(page_names(&window), ["Overview", "Frame", "Details", "Notes"]);
    assert_eq!(reply["pages"].as_array().unwrap().len(), 4);
    for name in ["Overview", "Details", "Notes", "Frame"] {
        window.get_by_label(name);
    }
    // The tabs: foreground pages in the file's order, then the background page.
    assert_eq!(tab_order(&window), ["page-tab.1", "page-tab.2", "page-tab.3", "page-tab.4"]);
    window.get_by_label("Page 1 of 4");
    window.get_by_label("floor plans.vsdx - DiaCraft");
    assert_eq!(window.state().window_title(), "floor plans.vsdx - DiaCraft");
    assert!(!window.state().window_title().contains("Untitled"));
    // Switching works from a tab, and the count follows the order of the tabs.
    window.get_by_label("Details").click();
    window.rest();
    window.get_by_label("Page 2 of 4");
    window.get_by_label("Frame").click();
    window.rest();
    window.get_by_label("Page 4 of 4");
}

/// A page's real size is what Fit and Page Width show: the 6 by 4 inch page is not drawn as a letter page.
#[test]
fn fit_and_page_width_use_the_size_of_the_page_in_the_file() {
    let dir = TempDir::new("size");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &path).unwrap();
    let letter = window.state().canvas_report().page.unwrap();
    assert!((letter.width() / letter.height() - 11.0 / 8.5).abs() < 0.01, "{letter:?}");
    window.get_by_label("Notes").click();
    window.rest();
    let notes = window.state().canvas_report().page.unwrap();
    assert!((notes.width() / notes.height() - 6.0 / 4.0).abs() < 0.01, "the Notes page is 6 by 4 inches: {notes:?}");
    let area = window.state().element("canvas").unwrap().rect;
    assert!(area.contains_rect(notes), "Fit shows the whole page: {notes:?} in {area:?}");
    // Page Width makes the page as wide as the area, less the margin.
    call(&mut window, &tx, "engine.execute", json!({"command": "view.fitWidth"})).unwrap();
    window.rest();
    let wide = window.state().canvas_report().page.unwrap();
    assert!(wide.width() > notes.width() * 1.1 && (wide.width() / wide.height() - 1.5).abs() < 0.01, "{wide:?}");
}

/// What `render` drew is on the screen: the picture of the page has ink in it, it lies over the grid and under the rulers, and a page
/// with nothing on it has none.
#[test]
fn the_picture_of_the_page_is_laid_over_the_grid_and_has_ink() {
    let dir = TempDir::new("picture");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &path).unwrap();
    window.rest();
    let report = window.state().canvas_report();
    let picture = report.picture.expect("the page has a picture");
    assert!(picture.lit > 5_000, "the shapes of the page are ink in it: {picture:?}");
    assert!(report.grid_lines.0 > 3 && report.rulers, "the file's window had the grid and the rulers on: {report:?}");
    // The shapes are drawn over the grid: in the frame's shapes the grid's lines come before the picture, and the rulers after it.
    let shapes = &window.output().shapes;
    let textured = |s: &egui::epaint::ClippedShape| matches!(&s.shape, egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default());
    let picture_at = shapes.iter().position(textured).expect("a textured mesh: the picture");
    let t = diacraft_ui_egui::theme::Tokens::light();
    let is_grid = |s: &egui::epaint::ClippedShape| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == t.grid_line || stroke.color == t.grid_major);
    let grid_at = shapes.iter().position(is_grid).expect("grid lines");
    assert!(grid_at < picture_at, "the grid ({grid_at}) is under the picture ({picture_at})");
    // A page with nothing on it has no picture.
    window.get_by_label("Notes").click();
    window.rest();
    assert!(window.state().canvas_report().picture.is_none(), "an empty page draws no picture");
    window.get_by_label("Overview").click();
    window.rest();
    assert!(window.state().canvas_report().picture.is_some(), "and the picture is back on the page that has shapes");
}

/// While the view moves the picture is drawn within the short limits, and at rest it is drawn again within the full ones.
#[test]
fn a_picture_drawn_while_the_view_moved_is_drawn_again_at_rest() {
    let dir = TempDir::new("rest");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &path).unwrap();
    let rest = |window: &mut Window| {
        window.advance(Duration::from_millis(250));
        window.step();
        window.step();
    };
    rest(&mut window);
    assert!(window.state().canvas_report().picture.is_some_and(|p| !p.rough), "at rest the picture is the full one");
    // A view that stays put asks for nothing more.
    let settled = call_inspect(&window);
    for _ in 0..5 {
        window.step();
    }
    assert_eq!(call_inspect(&window), settled);
    // A pan past the margin moves the view: the picture asked for in that frame is drawn within the short limits.
    call(&mut window, &tx, "engine.execute", json!({"command": "view.pan", "params": {"dx": -700.0, "dy": 0.0}})).unwrap();
    window.step();
    assert!(call_inspect(&window) > settled, "a pan past the margin asks for a picture");
    assert!(window.state().canvas_report().picture.is_some_and(|p| p.rough), "drawn while the view moved: within the short limits");
    let moved = call_inspect(&window);
    rest(&mut window);
    assert!(window.state().canvas_report().picture.is_some_and(|p| !p.rough), "and at rest it is drawn again in full");
    assert!(call_inspect(&window) > moved);
}

fn call_inspect(window: &Window) -> u64 {
    window.state().picture_state()["requested"].as_u64().unwrap()
}

/// A pan inside the margin of the picture asks for nothing; the picture follows the page by its inches.
#[test]
fn a_small_pan_asks_for_no_new_picture() {
    let dir = TempDir::new("pan");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &path).unwrap();
    window.advance(Duration::from_millis(250));
    window.step();
    window.step();
    let asked = call_inspect(&window);
    call(&mut window, &tx, "engine.execute", json!({"command": "view.pan", "params": {"dx": 40.0, "dy": -25.0}})).unwrap();
    window.step();
    window.step();
    assert_eq!(call_inspect(&window), asked, "a pan of 40 points is inside the margin of the picture");
    assert!(window.state().canvas_report().picture.is_some(), "the picture is still shown");
    // A zoom changes the scale: a picture is asked for.
    call(&mut window, &tx, "engine.execute", json!({"command": "view.zoom", "params": {"percent": 200.0}})).unwrap();
    window.step();
    window.step();
    assert!(call_inspect(&window) > asked, "a new scale asks for a new picture");
}

/// The grid and the rulers start from what the file's window says, and off where it says nothing; a file with no windows part shows a new drawing's (Ruling
#[test]
fn the_grid_and_rulers_start_from_the_files_window() {
    let dir = TempDir::new("flags");
    let flags = |grid: &str, rulers: &str| {
        let xml = format!(
            r#"<Windows><Window ID="0" WindowType="Drawing"><ShowRulers>{rulers}</ShowRulers><ShowGrid>{grid}</ShowGrid></Window></Windows>"#
        );
        fixture::demo_builder(Some(&xml)).build()
    };
    for (name, bytes, grid, rulers) in [
        ("on.vsdx", flags("1", "1"), true, true),
        ("mixed.vsdx", flags("0", "1"), false, true),
        ("other.vsdx", flags("1", "0"), true, false),
        ("none.vsdx", fixture::demo_builder(None).build(), true, true),
        ("silent.vsdx", fixture::demo_builder(Some(r#"<Windows><Window ID="0" WindowType="Drawing"/></Windows>"#)).build(), false, false),
    ] {
        let path = fixture::write(dir.path(), name, &bytes);
        let (mut window, tx) = viewer();
        open(&mut window, &tx, &path).unwrap();
        let state = window.state().window_state();
        assert_eq!((state["grid"].as_bool(), state["rulers"].as_bool()), (Some(grid), Some(rulers)), "{name}");
        assert_eq!(window.state().canvas_report().rulers, rulers, "{name}");
    }
}

/// The guides start from the file's window's `ShowGuides`, and on where it says nothing (a drawing DiaCraft saved holds none).
#[test]
fn the_guides_start_from_the_files_window_and_on_where_it_says_nothing() {
    let dir = TempDir::new("guide-flags");
    let shown = |guides: &str| {
        let xml =
            format!(r#"<Windows><Window ID="0" WindowType="Drawing"><ShowGrid>1</ShowGrid><ShowGuides>{guides}</ShowGuides></Window></Windows>"#);
        fixture::demo_builder(Some(&xml)).build()
    };
    for (name, bytes, guides) in
        [("off.vsdx", shown("0"), false), ("on.vsdx", shown("1"), true), ("none.vsdx", fixture::demo_builder(None).build(), true)]
    {
        let path = fixture::write(dir.path(), name, &bytes);
        let (mut window, tx) = viewer();
        // The window's box was turned the other way before: the file decides.
        call(&mut window, &tx, "engine.execute", json!({"command": "view.guides", "params": {"on": !guides}})).unwrap();
        open(&mut window, &tx, &path).unwrap();
        assert_eq!(window.state().window_state()["guides"].as_bool(), Some(guides), "{name}");
    }
}

/// A file that cannot be opened leaves the open drawing as it was, and says why in words, by the kind of trouble.
#[test]
fn a_file_that_will_not_open_keeps_the_document_and_says_why() {
    let dir = TempDir::new("bad");
    let good = fixture::write(dir.path(), "good.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &good).unwrap();
    let (revision, names) = (window.state().session().revision(), page_names(&window));
    let truncated = fixture::write(dir.path(), "cut.vsdx", &fixture::demo_vsdx()[..400]);
    let not_a_zip = fixture::write(dir.path(), "notes.vsdx", b"These are not drawing bytes at all.");
    let attempts: [(&std::path::Path, &str); 4] = [
        (&not_a_zip, "is not a drawing file DiaCraft can open"),
        (&truncated, "notes.vsdx"),
        (&dir.path().join("missing.vsdx"), "was not found"),
        (dir.path(), "is not a file"),
    ];
    for (path, words) in attempts {
        let error = open(&mut window, &tx, path).unwrap_err();
        let shown = window.state().open_error().expect("the window says why").to_owned();
        // Words for a person: the file, what is wrong, and that nothing was lost. No class name, no path.
        assert!(shown.contains("The drawing that was open is still open."), "{shown}");
        for raw in ["not_a_package", "NotAPackage", "damaged_package", "CannotOpen", "file.open", "::"] {
            assert!(!shown.contains(raw), "{raw} in {shown}");
        }
        assert!(!shown.contains(dir.path().to_str().unwrap()), "the folder is not in the message: {shown}");
        if words != "notes.vsdx" {
            assert!(shown.contains(words), "{words} in {shown}");
        }
        assert!(!error.is_empty());
        // The document is the one that was open before, untouched.
        assert_eq!(page_names(&window), names, "{shown}");
        assert_eq!(window.state().session().revision(), revision, "{shown}");
        assert_eq!(window.state().window_title(), "good.vsdx - DiaCraft");
        // The message is on the screen.
        window.get_by_label_contains("still open");
    }
    // Dismissed, it goes.
    let dismiss = window.state().element("open-error.dismiss").unwrap().rect.center();
    click(&mut window, dismiss);
    window.rest();
    assert!(window.state().open_error().is_none());
}

/// With no drawing open but the empty one, a bad file leaves that one, and the message does not claim a drawing was lost.
#[test]
fn a_bad_file_leaves_the_empty_drawing_and_a_good_one_clears_the_message() {
    let dir = TempDir::new("empty");
    let (mut window, tx) = viewer();
    call(&mut window, &tx, "engine.execute", json!({"command": "file.new"})).unwrap();
    let bad = fixture::write(dir.path(), "x.vsdx", b"nope");
    assert!(open(&mut window, &tx, &bad).is_err());
    window.get_by_label("Drawing1 - DiaCraft");
    assert!(window.state().open_error().is_some());
    let good = fixture::write(dir.path(), "good.vsdx", &fixture::demo_vsdx());
    open(&mut window, &tx, &good).unwrap();
    assert!(window.state().open_error().is_none(), "a file that opens clears the message");
    window.get_by_label("good.vsdx - DiaCraft");
}

/// What the renderer could not draw, and what the reader said, are one mark in the status bar with a list behind it; no dialog.
#[test]
fn what_could_not_be_drawn_is_one_mark_in_the_status_bar_with_a_list() {
    let dir = TempDir::new("findings");
    let (mut window, tx) = viewer();
    window.rest();
    assert!(window.state().element("status.findings").is_none(), "nothing to say, nothing drawn");
    // A fill with a custom pattern is drawn solid, and the page says so; the extension of the file is not its kind, which the reader says.
    let path = fixture::write(dir.path(), "pattern.vssx", &fixture::custom_pattern_vsdx());
    open(&mut window, &tx, &path).unwrap();
    // The picture of full quality is the one that counts.
    window.advance(Duration::from_millis(250));
    window.step();
    window.step();
    let inspect = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    let lines: Vec<String> = inspect["findings"]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_owned()).collect();
    assert!(lines.iter().any(|l| l == "1 fill or line drawn solid where the file uses a custom pattern"), "{lines:?}");
    assert!(lines.len() >= 2, "the reader's note about the extension is in the list too: {lines:?}");
    let mark = window.state().element("status.findings").expect("one mark in the status bar");
    assert!(mark.rect.height() <= 26.0 && mark.rect.width() < 80.0, "small: {:?}", mark.rect);
    // No dialog opened by itself; the list opens on a click.
    assert!(window.query_by_label_contains("drawn solid where").is_none());
    let at = mark.rect.center();
    click(&mut window, at);
    window.rest();
    window.get_by_label_contains("drawn solid where the file uses a custom pattern");
    window.get_by_label("Drawn approximately");
    window.get_by_label("When the file was read");
}

/// The Open entry of the File view is disabled where the app has no dialog, and enabled where it has one.
#[test]
fn open_is_enabled_only_with_a_file_dialog() {
    let mut without = window(DiacraftApp::new(Session::new(), None), [1000.0, 700.0]);
    without.state_mut().execute("window.backstage", &json!({"page": "Home"})).unwrap();
    without.rest();
    assert!(!without.state().element("backstage.Open").unwrap().enabled);
    let picker: FilePicker = Arc::new(|| None);
    let mut with = window(DiacraftApp::new(Session::new(), None).with_file_picker(picker), [1000.0, 700.0]);
    with.state_mut().execute("window.backstage", &json!({"page": "Home"})).unwrap();
    with.rest();
    assert!(with.state().element("backstage.Open").unwrap().enabled);
    assert!(diacraft_ui_egui::commands::known("file.open"), "Open stands for a registered command");
}

/// Open in the File view, and Ctrl+O, ask the dialog for a file and open what it gives. The dialog runs on a thread of its own, so
/// the window goes on drawing frames while it is open.
#[test]
fn open_and_ctrl_o_show_the_dialog_without_stopping_the_window_and_open_what_it_chooses() {
    let _guard = Watchdog::new("the Open dialog test", 60);
    let dir = TempDir::new("dialog");
    let path = fixture::write(dir.path(), "chosen.vsdx", &fixture::demo_vsdx());
    let shown = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Mutex::new(None::<std::sync::mpsc::Receiver<()>>));
    let (hold, held) = channel::<()>();
    *release.lock().unwrap() = Some(held);
    let picker: FilePicker = {
        let (shown, release, path) = (Arc::clone(&shown), Arc::clone(&release), path.clone());
        Arc::new(move || {
            shown.fetch_add(1, Ordering::SeqCst);
            // The dialog stays open until the test lets it go.
            if let Some(gate) = release.lock().unwrap().take() {
                let _ = gate.recv_timeout(Duration::from_secs(30));
            }
            Some(path.clone())
        })
    };
    let app = DiacraftApp::new(Session::new(), None).with_file_picker(picker).with_inline_pictures(bundled_drawer());
    let mut window = window(app, [1000.0, 700.0]);
    window.rest();
    // Ctrl+O.
    let ctrl = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    for pressed in [true, false] {
        window.input_mut().events.push(egui::Event::Key { key: egui::Key::O, physical_key: None, pressed, repeat: false, modifiers: ctrl });
        window.step();
    }
    step_until(&mut window, 10, "the dialog to be shown", |_| shown.load(Ordering::SeqCst) == 1);
    // While it is open, frames go on and are quick; no second dialog opens.
    let start = Instant::now();
    let frames = window.state().frames();
    for _ in 0..20 {
        window.step();
    }
    // Counted: twenty frames were drawn. The time is a hang guard and no measure of how quick they are (a minute is generous at load 50).
    assert!(window.state().frames() >= frames + 20 && start.elapsed() < Duration::from_secs(60), "the window runs while the dialog is open");
    assert!(!window.state().can_pick_file(), "a second Open waits for the first");
    assert_eq!(shown.load(Ordering::SeqCst), 1);
    drop(hold);
    step_until(&mut window, 20, "the chosen file to open", |w| w.state().session().document().is_some_and(|d| d.pages.len() == 4));
    window.rest();
    assert_eq!(window.state().window_title(), "chosen.vsdx - DiaCraft");
    // The file was opened by the command: the session that read it holds `file.open` in its journal, with the path.
    let journal = window.state().session().journal();
    assert!(journal.iter().any(|(id, p)| id == "file.open" && p["path"].as_str() == path.to_str()), "{journal:?}");
    // Open in the File view does the same.
    window.state_mut().execute("window.backstage", &json!({"page": "Home"})).unwrap();
    window.rest();
    let open_entry = window.state().element("backstage.Open").unwrap();
    assert!(open_entry.enabled);
    let at = open_entry.rect.center();
    click(&mut window, at);
    step_until(&mut window, 20, "the dialog to be shown again", |_| shown.load(Ordering::SeqCst) == 2);
    step_until(&mut window, 20, "the file to open again", |w| w.state().can_pick_file() && w.state().opening_now().is_none());
    window.rest();
    assert!(window.state().canvas_report().page.is_some(), "the File view closed on the file");
}

/// A dialog that is cancelled opens nothing and changes nothing.
#[test]
fn a_cancelled_dialog_changes_nothing() {
    let picker: FilePicker = Arc::new(|| None);
    let app = DiacraftApp::new(Session::new(), None).with_file_picker(picker);
    let mut window = window(app, [900.0, 600.0]);
    window.rest();
    window.state_mut().pick_and_open(&egui::Context::default());
    assert!(window.state().open_dialog_open(), "the dialog was started, and has not been answered yet");
    // Wait for its thread to answer (a loaded machine may be slow to), not for a fixed number of frames: the answer is taken in the frame
    // after it is sent, and a window that shows nothing yet has not necessarily been answered.
    step_until(&mut window, 30, "the dialog to answer", |w| !w.state().open_dialog_open());
    window.step();
    assert!(window.state().can_pick_file() && window.state().opening_now().is_none() && window.state().open_error().is_none());
    assert!(window.state().session().document().is_none());
}

/// Reading a large file does not stop the window: frames are drawn while it is read, a plain progress state is on the screen, and
/// the drawing is there when it ends.
#[test]
fn a_large_file_is_read_while_the_window_goes_on() {
    let _guard = Watchdog::new("the large file test", 120);
    let dir = TempDir::new("large");
    let path = fixture::write(dir.path(), "crowded.vsdx", &fixture::crowded_vsdx(30_000));
    let (mut window, tx) = viewer();
    window.rest();
    let answer = send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let (mut while_opening, mut card) = (0, false);
    let start = Instant::now();
    let reply = loop {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            break reply;
        }
        if window.state().opening_now().is_some() {
            while_opening += 1;
            card |= window.query_by_label_contains("Opening crowded.vsdx").is_some();
        }
        assert!(start.elapsed() < Duration::from_secs(100), "the file was not read in time");
        std::thread::sleep(Duration::from_millis(2));
    };
    reply.unwrap();
    assert!(while_opening >= 3, "frames were drawn while the file was read: {while_opening}");
    assert!(card, "the progress card said what was being opened");
    // Two frames, not `rest`: the page has 30,000 shapes and is drawn inline, so how many frames its pictures take (a rough one, the full one,
    // and two more asked for when the full one ran out of time) depends on how fast the machine is. None of them is this test's business.
    window.step();
    window.step();
    assert!(window.state().opening_now().is_none());
    assert_eq!(page_names(&window), ["Crowded"]);
}

/// A second open while one is being read is told to wait, and does not start another.
#[test]
fn a_second_open_waits_for_the_first() {
    let dir = TempDir::new("twice");
    let path = fixture::write(dir.path(), "crowded.vsdx", &fixture::crowded_vsdx(20_000));
    let (mut window, tx) = viewer();
    window.rest();
    let first = send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    window.step();
    let second = send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let mut answers = Vec::new();
    let start = Instant::now();
    while answers.len() < 2 && start.elapsed() < Duration::from_secs(60) {
        window.step();
        for (n, a) in [&first, &second].into_iter().enumerate() {
            if let Ok(reply) = a.try_recv() {
                answers.push((n, reply));
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(answers.len(), 2, "both were answered");
    let by = |n: usize| &answers.iter().find(|(i, _)| *i == n).unwrap().1;
    assert!(by(0).is_ok(), "the first opened");
    assert!(by(1).as_ref().unwrap_err().contains("Still opening crowded.vsdx"), "{:?}", by(1));
}

/// A drawer that panics: the window goes on, says so once, and loses nothing.
struct Panicking;

impl Drawer for Panicking {
    fn draw(&mut self, _: &Job) -> diacraft_render::Rendered {
        panic!("the drawer broke")
    }
}

#[test]
fn a_panic_in_drawing_is_a_notice_and_the_window_goes_on() {
    let dir = TempDir::new("drawpanic");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    for threaded in [false, true] {
        let make: MakeDrawer = Box::new(|| Box::new(Panicking));
        let (mut window, tx) = controlled_with(|app| if threaded { app.with_threaded_pictures(make) } else { app.with_inline_pictures(make) });
        open(&mut window, &tx, &path).unwrap();
        step_until(&mut window, 20, "the notice", |w| w.state().notice().is_some_and(|n| n.contains("internal error while drawing the page")));
        // The panic was caught where it happened, and is counted as the window's own: a check of `ui.inspect` must not pass over a blank page
        // (`panics` stayed 0 while a page was blank).
        assert!(window.state().ui_panics() >= 1, "a caught drawing panic is counted: {}", window.state().ui_panics());
        assert!(window.state().canvas_report().picture.is_none());
        // The window still answers and the document is open.
        let inspected = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
        assert_eq!(inspected["document"]["pages"], 4);
        assert!(inspected["panics"]["ui"].as_u64().unwrap() >= 1, "{inspected}");
        assert!(inspected["picture"]["caughtPanics"]["framesLost"].as_u64().unwrap() >= 1, "{inspected}");
        // And the page says, in words on the page, that it could not be drawn.
        let words = inspected["picture"]["lost"].as_str().expect("the page says it was lost");
        assert!(words.starts_with("This page could not be drawn"), "{words}");
        assert_eq!(elements_named(&window, "canvas.lost").len(), 1, "the words are drawn on the page");
        window.get_by_label("Details").click();
        window.rest();
        window.get_by_label("Page 2 of 4");
    }
}

/// A drawer that answers with a picture the test made: blank or lit, and with what the renderer says it lost.
struct Canned {
    skipped: Vec<diacraft_render::Skipped>,
    lit: bool,
}

impl Drawer for Canned {
    fn draw(&mut self, job: &Job) -> diacraft_render::Rendered {
        let bytes = job.view.width as usize * job.view.height as usize * 4;
        let mut pixels = vec![0u8; bytes];
        if self.lit {
            pixels.chunks_mut(4).for_each(|p| p.copy_from_slice(&[40, 60, 90, 255]));
        }
        diacraft_render::Rendered { rgba_premul: pixels, width: job.view.width, height: job.view.height, skipped: self.skipped.clone() }
    }
}

fn lost(kind: diacraft_render::SkipKind, count: usize) -> diacraft_render::Skipped {
    diacraft_render::Skipped { kind, count, first: "shape 7: a test bug".to_owned() }
}

/// Lets the picture of full quality, drawn once the view has been still, arrive: it is the one the list of findings reads.
fn let_the_full_picture_arrive(window: &mut Window) {
    window.advance(Duration::from_millis(250));
    window.step();
    window.step();
}

/// What a window shows of a page for which the renderer answered `skipped`, drawn blank or lit.
fn inspected_after(skipped: Vec<diacraft_render::Skipped>, lit: bool) -> (Window, Value) {
    let dir = TempDir::new("canned");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let make: MakeDrawer = Box::new(move || Box::new(Canned { skipped, lit }));
    let (mut window, tx) = controlled_with(|app| app.with_inline_pictures(make));
    open(&mut window, &tx, &path).unwrap();
    let_the_full_picture_arrive(&mut window);
    let inspected = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    (window, inspected)
}

/// Shapes the renderer lost to a bug are panics of the window, and a page with none of its shapes drawn says so on the page.
#[test]
fn shapes_that_failed_to_draw_count_as_panics_and_a_page_with_nothing_drawn_says_so() {
    let (window, inspected) = inspected_after(vec![lost(diacraft_render::SkipKind::ShapeFailed, 3)], false);
    assert!(inspected["panics"]["ui"].as_u64().unwrap() >= 3, "{inspected}");
    assert!(inspected["picture"]["caughtPanics"]["shapesLost"].as_u64().unwrap() >= 3, "{inspected}");
    let words = inspected["picture"]["lost"].as_str().expect("a page with nothing drawn says so");
    assert!(words.contains("could not draw any of its shapes") && words.contains("Please report"), "{words}");
    assert_eq!(elements_named(&window, "canvas.lost").len(), 1);
    // The list behind the status bar's mark says it too.
    let lines = inspected["findings"]["lines"].to_string();
    assert!(lines.contains("3 shapes left out because DiaCraft met an internal problem"), "{lines}");
}

/// A page that drew some shapes keeps its picture, and the shapes it lost are counted all the same.
#[test]
fn a_page_that_drew_most_of_its_shapes_counts_the_lost_ones_and_keeps_its_picture() {
    let (window, inspected) = inspected_after(vec![lost(diacraft_render::SkipKind::ShapeFailed, 1)], true);
    assert!(inspected["panics"]["ui"].as_u64().unwrap() >= 1, "{inspected}");
    assert!(inspected["picture"]["lost"].is_null(), "{inspected}");
    assert!(elements_named(&window, "canvas.lost").is_empty());
    assert!(inspected["picture"]["shown"]["litPixels"].as_u64().unwrap() > 0);
    assert!(inspected["findings"]["lines"].to_string().contains("1 shape left out because DiaCraft met an internal problem"), "{inspected}");
}

/// A frame the renderer's own net recovered is a lost frame.
#[test]
fn a_frame_the_renderer_recovered_counts_as_a_panic_and_says_so_on_the_page() {
    let (window, inspected) = inspected_after(vec![lost(diacraft_render::SkipKind::Recovered, 1)], false);
    assert!(inspected["panics"]["ui"].as_u64().unwrap() >= 1, "{inspected}");
    assert!(inspected["picture"]["lost"].as_str().unwrap().starts_with("This page could not be drawn"), "{inspected}");
    assert_eq!(elements_named(&window, "canvas.lost").len(), 1);
}

/// A page that drew whole has no panics and no words over it.
#[test]
fn a_page_that_drew_whole_has_no_panics_and_no_words_over_it() {
    let (window, inspected) = inspected_after(Vec::new(), true);
    assert_eq!(inspected["panics"]["ui"], 0, "{inspected}");
    assert!(inspected["picture"]["lost"].is_null());
    assert!(elements_named(&window, "canvas.lost").is_empty());
}

/// The first window's blank page, end to end with the real renderer: a drawing whose shape holds the `Themed` marker in its fill,
/// line and shadow cells, with a theme selected that cannot answer. It draws, and nothing in `ui.inspect` says a panic was caught.
#[test]
fn a_themed_drawing_draws_with_the_real_renderer_and_no_panic_is_caught() {
    let dir = TempDir::new("themed");
    let path = fixture::write(dir.path(), "themed.vsdx", &fixture::themed_vsdx());
    let (mut window, tx) = viewer();
    open(&mut window, &tx, &path).unwrap();
    let_the_full_picture_arrive(&mut window);
    let inspected = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(inspected["panics"]["ui"], 0, "{inspected}");
    assert!(inspected["picture"]["lost"].is_null(), "{inspected}");
    assert!(inspected["picture"]["shown"]["litPixels"].as_u64().unwrap() > 1000, "the page is not blank: {inspected}");
    let lines = inspected["findings"]["lines"].to_string();
    assert!(lines.contains("drawn with plain colours where the file uses a theme"), "{lines}");
}

/// Opening a file by the command, not the control channel, on the calling thread: the window follows (a test's way, as `file.new` is).
#[test]
fn the_command_run_in_place_leaves_the_window_in_step_with_the_file() {
    let dir = TempDir::new("inplace");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let mut window = window(DiacraftApp::new(Session::new(), None).with_inline_pictures(bundled_drawer()), [1100.0, 760.0]);
    window.rest();
    window.state_mut().execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    window.rest();
    window.get_by_label("plans.vsdx - DiaCraft");
    assert_eq!(window.state().window_state()["grid"], true);
    assert!(window.state().canvas_report().picture.is_some());
    // A new drawing after it is a new drawing: no picture of the file is left on it.
    window.state_mut().execute("file.new", &json!({})).unwrap();
    window.rest();
    assert!(window.state().canvas_report().picture.is_none());
    window.get_by_label("Drawing1 - DiaCraft");
}

/// An `.odg` of one page whose shapes are `shapes`, written by hand from the OASIS ODF standard.
fn odg_of(shapes: &str) -> Vec<u8> {
    use diacraft_container::zip::{ArchiveWriter, Method};
    const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
    );
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3"><office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default">{shapes}</draw:page></office:drawing></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3"><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles></office:document-styles>"#
    );
    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="{MEDIA}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// **What the drawing does not hold of the file is in the window's list, whole**: the reader's head line and a line with the count of each class, among what
/// the reader said, none of them cut; a drawing that holds all of its file says nothing of the kind.
#[test]
fn what_the_drawing_does_not_hold_of_the_file_is_in_the_list_whole() {
    let dir = TempDir::new("notkept");
    let (mut window, tx) = viewer();
    window.rest();
    let measures =
        r#"<draw:measure svg:x1="1cm" svg:y1="5cm" svg:x2="6cm" svg:y2="5cm"/><draw:measure svg:x1="1cm" svg:y1="7cm" svg:x2="6cm" svg:y2="7cm"/>"#;
    let path = fixture::write(dir.path(), "measures.odg", &odg_of(measures));
    open(&mut window, &tx, &path).unwrap();
    let inspect = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    let lines: Vec<String> = inspect["findings"]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_owned()).collect();
    let head = "What DiaCraft does not hold of this file: a save into the file keeps it, and a copy saved as another file (.diacraft or another format), or a new drawing made from it, is without it";
    assert!(lines.iter().any(|l| l == head), "{lines:?}");
    let kinds = "2 shapes of kinds DiaCraft keeps only as placeholders (dimension lines, callouts, form controls or 3-D scenes), which a copy writes as plain shapes";
    assert!(lines.iter().any(|l| l == kinds), "{lines:?}");
    let plain = fixture::write(dir.path(), "plain.odg", &odg_of(r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#));
    open(&mut window, &tx, &plain).unwrap();
    let inspect = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    let lines: Vec<String> =
        inspect["findings"]["lines"].as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    assert!(!lines.iter().any(|l| l == head), "{lines:?}");
}
