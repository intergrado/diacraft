//! The window's tests and time (`clock.rs`): a test window's clock stands still until the test advances it, so what the window decides by how long ago something
//! happened is the same on a quick machine and a busy one, and nothing else in the window reads the time. (GitHub run 42398195820 failed a test of a window that
//! works, "Harness::run exceeded max_steps (4)", because of the machine's speed; so did a run at a load average of 125.)

use std::path::{Path, PathBuf};
use std::time::Duration;

use diacraft_ui_egui::canvas::picture::REST_AFTER;
use diacraft_ui_egui::render_worker::bundled_drawer;
use serde_json::json;

use crate::editing::edit_window;
use crate::fixture;
use crate::support::{Rest, TempDir, Window, controlled_with, send};

fn requested(w: &Window) -> u64 {
    w.state().picture_state()["requested"].as_u64().unwrap()
}

fn rough(w: &Window) -> bool {
    w.state().canvas_report().picture.is_some_and(|p| p.rough)
}

/// Opens `path` as the window does and steps until it is in place.
fn open_file(w: &mut Window, tx: &std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>, path: &Path) {
    let answer = send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    for _ in 0..4_000 {
        w.step();
        if let Ok(reply) = answer.try_recv() {
            reply.expect("the file opened");
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened");
}

/// Every window the tests build has a clock that stands still.
#[test]
fn every_test_window_has_a_clock_that_stands_still_until_it_is_advanced() {
    let w = edit_window([800.0, 600.0]);
    assert!(w.state().clock().is_manual(), "a test window's clock is the test's");
    let before = w.state().clock().now();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(w.state().clock().now(), before, "the machine's time does not move it");
    let (w, _tx) = controlled_with(|app| app.with_inline_pictures(bundled_drawer()));
    assert!(w.state().clock().is_manual());
}

/// A picture drawn while the view moved is drawn again in full once the view has stood still for `REST_AFTER`: **by the window's clock**. The machine's time
/// passing (here a sleep of three times `REST_AFTER` between the frames) changes nothing; the test's clock passing does, by exactly one picture.
#[test]
fn a_view_comes_to_rest_when_the_windows_clock_says_so_and_not_when_the_machine_is_slow() {
    let dir = TempDir::new("clock-rest");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut w, tx) = controlled_with(|app| app.with_inline_pictures(bundled_drawer()));
    open_file(&mut w, &tx, &path);
    w.rest();
    assert!(rough(&w), "the picture of a view that has just appeared is the rough one");
    let asked = requested(&w);
    // The machine is slow: three times as long as the view needs to rest passes between two frames.
    std::thread::sleep(REST_AFTER * 3);
    w.step();
    w.rest();
    assert!(rough(&w), "the machine's time passing did not bring the view to rest");
    assert_eq!(requested(&w), asked, "and no picture was asked for");
    // The window's time passes.
    w.advance(REST_AFTER + Duration::from_millis(1));
    w.rest();
    assert!(!rough(&w), "the view has stood still for the time it needs: the picture is drawn again in full");
    assert_eq!(requested(&w), asked + 1, "by one picture");
    // And it stays so.
    w.advance(Duration::from_secs(10));
    w.rest();
    assert_eq!(requested(&w), asked + 1, "nothing more is asked for while the view stands still");
}

/// The source files of the window's crate.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

fn source_files() -> Vec<(String, String)> {
    let mut paths = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut paths);
    assert!(
        paths.iter().any(|p| p.ends_with("clock.rs")) && paths.iter().any(|p| p.ends_with("app.rs")),
        "the scan sees the window's files: {paths:?}"
    );
    paths.into_iter().map(|p| (p.display().to_string(), std::fs::read_to_string(&p).unwrap())).collect()
}

/// Where each file of the window reads the machine's time, if it does: `Instant::now`, the system time and an `elapsed`. The window's own logic reads the
/// window's clock, so the files that may read the machine's are the clock itself and what is about real time on purpose: the render worker's limits on its
/// own thread, and a hang guard in a test of the scene. (The control channel's quit delay and screenshot timeout were on this list until a later change: they
/// read the window's clock now, and `control.rs` reads no time of its own.)
#[test]
fn nothing_but_the_clock_and_what_is_about_real_time_reads_the_time() {
    let allowed = ["clock.rs", "render_worker.rs", "scene.rs"];
    let spellings = ["Instant::now", "SystemTime", ".elapsed()", "duration_since("];
    let mut found = Vec::new();
    for (path, text) in source_files() {
        if allowed.iter().any(|a| path.ends_with(a)) {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if let Some(s) = spellings.iter().find(|s| code.contains(**s)) {
                found.push(format!("{path}:{}: `{s}`", n + 1));
            }
        }
    }
    assert!(found.is_empty(), "read the time from the window's clock (`self.clock.now()`), not the machine's: {found:?}");
}

/// A timed repaint (`request_repaint_after`) is a decision about time, and a test of the window waits for it by what it holds (`Rest::rest`), never by the
/// machine's time. The files that ask for one are listed with what each wakes up for; another file that does asks the person who adds it to look at this.
#[test]
fn a_timed_repaint_is_asked_for_only_where_it_is_known_what_it_wakes_up() {
    let known: [(&str, &str); 16] = [
        (
            "app.rs",
            "a frame after a panic, so that the notice shows, and while the shared scan of the faces goes, so that the Font box reads them when it ends",
        ),
        ("canvas/picture.rs", "the view's rest and a failed drawing's backoff, both by the window's clock"),
        (
            "control.rs",
            "what is left of the quit delay and a pending screenshot's poll, to look at the window's clock again (the deadlines are by the clock; the wake-up is the machine's time)",
        ),
        (
            "edit/autoconnect.rs",
            "the end of the rest before AutoConnect's arrows show, before its flyout opens and before the flyout's grace runs out, by the window's clock",
        ),
        ("edit/mod.rs", "the outline of a finished edit, until the picture shows it or the window's clock runs out its patience"),
        ("export.rs", "the thread of an export"),
        ("file_host.rs", "a file handed to the page that the page has not answered for yet (a download or a write in place)"),
        ("file_open.rs", "the thread of a file being read"),
        ("font_list.rs", "the scan of the machine's fonts"),
        ("insert_picture.rs", "the thread of the picture file chooser"),
        ("notice.rs", "the end of the status bar's notice, by the window's clock"),
        (
            "prefs_host.rs",
            "a change of the person's settings that waits out its quiet period before it is saved; a profile over a host has no wait but the one after a failed write",
        ),
        ("print.rs", "the threads of the printers and of a job"),
        ("save.rs", "the thread of a save"),
        ("splash.rs", "the end of the start splash's four seconds, by the window's clock; none while the pointer holds it, and none for About"),
        ("stencil_files.rs", "the thread of a stencil file being read"),
    ];
    let mut found = Vec::new();
    for (path, text) in source_files() {
        if text.lines().any(|l| l.split("//").next().unwrap_or_default().contains("request_repaint_after("))
            && !known.iter().any(|(f, _)| path.ends_with(f))
        {
            found.push(path);
        }
    }
    assert!(found.is_empty(), "a new timed repaint: say what it wakes up for, and keep a test from depending on the machine's time: {found:?}");
    assert!(known.iter().all(|(_, why)| !why.is_empty()));
}
