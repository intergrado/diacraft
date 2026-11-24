//! The panes and the ribbon after the window opens a second file. **A session that is opened starts its revision at 1 again**, so whatever is kept for a
//! revision of the drawing must be kept for the drawing as well: the tree of Drawing Explorer, the answers Size & Position shows and sends from, the picture of
//! Pan & Zoom and the formatting the ribbon reads. The files are opened the way the window opens them: on a thread, in a session of their own, and put in place
//! when the reading ends (`engine.execute file.open` through the control channel runs `open_path`'s own reading and `finish_open`).

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use diacraft_engine::formatting::Rgb;
use diacraft_ui_egui::ControlRequest;
use diacraft_ui_egui::format::Mix;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::work;
use egui::{Key, Modifiers};
use serde_json::json;

use crate::editing::{frame_of, key, type_text};
use crate::fixture;
use crate::saving::ctrl;
use crate::support::{Rest, TempDir, Window, click, controlled_with, run_command, send};

/// A window with a new drawing, as the app starts (revision 1).
fn viewer() -> (Window, Sender<ControlRequest>) {
    let (mut w, tx) = controlled_with(|app| app.with_inline_pictures(bundled_drawer()));
    run_command(&mut w, "file.new", json!({}));
    (w, tx)
}

/// Opens `path` as the window does and steps until it is in place.
fn open_file(w: &mut Window, tx: &Sender<ControlRequest>, path: &Path) {
    let answer = send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        w.step();
        if let Ok(reply) = answer.try_recv() {
            reply.expect("the file opened");
            w.rest();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened in 30 s");
}

fn pane(w: &mut Window, name: &str) {
    run_command(w, "window.pane", json!({"pane": name, "shown": true}));
}

fn shown_in(w: &Window, name: &str) -> String {
    w.state().pane_field_text(name).unwrap_or_else(|| panic!("{name} was not drawn")).to_owned()
}

fn click_on(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

/// Drawing Explorer lists the pages of the file that was opened, and of the next one, and not the tree it made before.
#[test]
fn the_explorer_lists_the_file_just_opened_and_not_the_drawing_it_replaced() {
    let dir = TempDir::new("explorer-swap");
    let (mut w, tx) = viewer();
    pane(&mut w, "drawing_explorer");
    w.rest();
    let four = fixture::write(dir.path(), "four.vsdx", &fixture::demo_vsdx());
    let one = fixture::write(dir.path(), "one.vsdx", &fixture::filled_vsdx(2.0, 2.0, 1.0, 1.0, "#FF0000", 0));
    open_file(&mut w, &tx, &four);
    for id in 1..=4 {
        assert!(w.state().element(&format!("explorer.page.{id}")).is_some(), "page {id} of the 4-page file is listed");
    }
    open_file(&mut w, &tx, &one);
    assert!(w.state().element("explorer.page.1").is_some());
    for id in 2..=4 {
        assert!(w.state().element(&format!("explorer.page.{id}")).is_none(), "page {id} belongs to the file that was replaced");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// Size & Position shows the values of the new file's shape (same page id, same shape id, other numbers) and a value typed is a distance from them.
#[test]
fn size_and_position_reads_and_sends_from_the_file_just_opened() {
    let dir = TempDir::new("size-swap");
    let (mut w, tx) = viewer();
    let first = fixture::write(dir.path(), "first.vsdx", &fixture::filled_vsdx(2.0, 2.0, 1.0, 1.0, "#FF0000", 0));
    let second = fixture::write(dir.path(), "second.vsdx", &fixture::filled_vsdx(6.0, 5.0, 2.0, 1.5, "#0000FF", 2));
    pane(&mut w, "size_position");
    open_file(&mut w, &tx, &first);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert_eq!(shown_in(&w, "pane.size.x"), "2 in");
    open_file(&mut w, &tx, &second);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert_eq!(shown_in(&w, "pane.size.x"), "6 in", "the second file's shape, not the first's");
    assert_eq!(shown_in(&w, "pane.size.y"), "5 in");
    assert_eq!(shown_in(&w, "pane.size.width"), "2 in");
    click_on(&mut w, "pane.size.x");
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "7");
    key(&mut w, Key::Enter, Modifiers::NONE);
    let frame = frame_of(&w, 1);
    assert!(
        (frame.0 - 7.0).abs() < 1e-6 && (frame.1 - 5.0).abs() < 1e-6 && (frame.2 - 2.0).abs() < 1e-6,
        "the shape is at 7, as the second file's shape: {frame:?}"
    );
}

/// Pan & Zoom draws the new file's shapes again, and the ribbon reads the new file's fill for the same shape id.
#[test]
fn pan_and_zoom_and_the_formatting_controls_read_the_file_just_opened() {
    let dir = TempDir::new("picture-swap");
    let (mut w, tx) = viewer();
    let first = fixture::write(dir.path(), "first.vsdx", &fixture::filled_vsdx(2.0, 2.0, 1.0, 1.0, "#FF0000", 0));
    let second = fixture::write(dir.path(), "second.vsdx", &fixture::filled_vsdx(6.0, 5.0, 2.0, 1.5, "#0000FF", 0));
    work::reset();
    pane(&mut w, "pan_zoom");
    open_file(&mut w, &tx, &first);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert_eq!(w.state().fill_mix(), Mix::One(Some(Rgb(255, 0, 0))));
    let before = work::work().thumb_builds;
    open_file(&mut w, &tx, &second);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert!(work::work().thumb_builds > before, "the picture of the second file was made: {:?}", work::work());
    assert_eq!(w.state().fill_mix(), Mix::One(Some(Rgb(0, 0, 255))), "the ribbon reads the second file's fill");
    assert_eq!(w.state().ui_panics(), 0);
}
