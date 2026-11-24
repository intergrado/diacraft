//! What the window asks of the renderer and when: a picture at rest that ran out of time is asked for again, a bounded number of
//! times and with more time; a new document drops what the renderer kept for the old one; a request for another view stops the picture in
//! progress. The renderer is a drawer the test scripts, so the answers are the ones a heavy page would give.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_render::{Limits, Rendered, SkipKind, Skipped};
use diacraft_ui_egui::render_worker::{Drawer, Job, MakeDrawer};
use serde_json::json;

use crate::fixture;
use crate::support::{Rest, TempDir, Window, call, controlled_with};

/// What a scripted drawer was asked, and how often its caches were dropped.
#[derive(Default)]
struct Log {
    asked: Mutex<Vec<(Limits, bool)>>,
    cleared: AtomicUsize,
}

/// A drawer that answers the nth call with `script[n]` shapes left out for want of time (none past the end of the script), lit all over.
struct Scripted {
    script: Vec<usize>,
    log: Arc<Log>,
}

impl Drawer for Scripted {
    fn draw(&mut self, job: &Job) -> Rendered {
        let call = {
            let mut asked = self.log.asked.lock().unwrap();
            asked.push((job.limits, job.supersedes));
            asked.len() - 1
        };
        let left = self.script.get(call).copied().unwrap_or(0);
        let mut pixels = vec![0u8; job.view.width as usize * job.view.height as usize * 4];
        pixels.chunks_mut(4).for_each(|p| p.copy_from_slice(&[40, 60, 90, 255]));
        let skipped = if left > 0 {
            vec![Skipped { kind: SkipKind::FrameBudget, count: left, first: "the shapes after the last one drawn".into() }]
        } else {
            Vec::new()
        };
        Rendered { rgba_premul: pixels, width: job.view.width, height: job.view.height, skipped }
    }

    fn clear_caches(&mut self) {
        self.log.cleared.fetch_add(1, Ordering::SeqCst);
    }
}

fn scripted(script: &[usize]) -> (MakeDrawer, Arc<Log>) {
    let log = Arc::new(Log::default());
    let (script, made) = (script.to_vec(), Arc::clone(&log));
    (Box::new(move || Box::new(Scripted { script, log: made })), log)
}

/// Opens the demo drawing in a window whose pictures come from `make`, drawn inline so that each answer is in the frame that asked.
fn open_demo(make: MakeDrawer) -> (Window, std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>, TempDir) {
    let dir = TempDir::new("pictures");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let (mut window, tx) = controlled_with(|app| app.with_inline_pictures(make));
    call_until_answered(&mut window, &tx, &path).unwrap();
    (window, tx, dir)
}

/// Steps the window with the view still until it has stopped asking: no new request for `quiet` of frames in a row.
fn until_quiet(window: &mut Window, log: &Log) {
    let mut quiet = 0;
    let mut seen = log.asked.lock().unwrap().len();
    for _ in 0..200 {
        // The window's own clock goes 40 ms between frames (the test's clock stands still until it is advanced, `clock.rs`).
        window.advance(Duration::from_millis(40));
        window.step();
        let now = log.asked.lock().unwrap().len();
        quiet = if now == seen { quiet + 1 } else { 0 };
        seen = now;
        if quiet >= 8 {
            return;
        }
    }
    panic!("the window kept asking for pictures");
}

/// The review's case: a picture at rest that hit the frame budget counted as full, so the shapes in front and the later labels stayed missing
/// until the view moved. It is asked for again, with twice the time and then four times, and no more than that.
#[test]
fn a_picture_at_rest_that_ran_out_of_time_is_asked_for_again_with_more_time_a_bounded_number_of_times() {
    // The first picture (the view is new, so it is rough) is whole; at rest, three pictures each leave fewer out than the last. The third is
    // never asked for: two askings again is the bound.
    let (make, log) = scripted(&[0, 90, 40, 10, 1]);
    let (mut window, _tx, _dir) = open_demo(make);
    until_quiet(&mut window, &log);
    let asked = log.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 4, "a rough picture, the full one, and two askings again: {asked:?}");
    let frame_times: Vec<Option<Duration>> = asked.iter().map(|(l, _)| l.frame_time).collect();
    assert_eq!(frame_times[0], Some(Duration::from_millis(120)), "the picture of a view in motion is rough");
    assert_eq!(frame_times[1], Some(Duration::from_millis(1_500)), "at rest: the defaults");
    assert_eq!(frame_times[2], Some(Duration::from_millis(3_000)), "asked again: twice the time");
    assert_eq!(frame_times[3], Some(Duration::from_millis(6_000)), "and again: four times");
    let text_times: Vec<Option<Duration>> = asked.iter().skip(1).map(|(l, _)| l.text_time).collect();
    assert_eq!(text_times, [Some(Duration::from_millis(400)), Some(Duration::from_millis(800)), Some(Duration::from_millis(1_600))]);
    // The list behind the status bar is of the last picture.
    let inspected = call(&mut window, &_tx, "ui.inspect", json!({})).unwrap();
    assert!(inspected["findings"]["lines"].to_string().contains("10 shapes left out because the page took too long"), "{inspected}");
}

/// A picture that is whole is not asked for again, and neither is one that left out as many as the picture before it (asking cannot help).
#[test]
fn a_whole_picture_is_not_asked_for_again_and_neither_is_one_that_gained_nothing() {
    let (make, log) = scripted(&[0, 0]);
    let (mut window, _tx, _dir) = open_demo(make);
    until_quiet(&mut window, &log);
    assert_eq!(log.asked.lock().unwrap().len(), 2, "the rough picture and the whole one at rest");

    let (make, log) = scripted(&[0, 30, 30, 30]);
    let (mut window, _tx, _dir) = open_demo(make);
    until_quiet(&mut window, &log);
    assert_eq!(log.asked.lock().unwrap().len(), 3, "the second asking left out as many as the first, so there was no third");
}

/// The review's case for the old file's media outliving it: opening another drawing tells the drawer to drop what it kept.
#[test]
fn opening_another_drawing_drops_what_the_renderer_kept_for_the_old_one() {
    let (make, log) = scripted(&[0, 0, 0, 0]);
    let (mut window, tx, dir) = open_demo(make);
    until_quiet(&mut window, &log);
    assert_eq!(log.cleared.load(Ordering::SeqCst), 0, "nothing is dropped while the one drawing is open");
    let second = fixture::write(dir.path(), "other.vsdx", &fixture::demo_vsdx());
    call_until_answered(&mut window, &tx, &second).unwrap();
    for _ in 0..40 {
        window.step();
        std::thread::sleep(Duration::from_millis(10));
        if log.cleared.load(Ordering::SeqCst) > 0 {
            break;
        }
    }
    assert_eq!(log.cleared.load(Ordering::SeqCst), 1, "the second file replaced the first, and the drawer was told once");
    // A new drawing does the same.
    window.state_mut().execute("file.new", &json!({})).unwrap();
    window.rest();
    assert_eq!(log.cleared.load(Ordering::SeqCst), 2);
}

/// The review's case for a hung mount: the progress card has a Cancel button (and Escape does the same), the drawing that was open stays,
/// and the next Open is not told to wait.
#[test]
fn the_progress_card_can_be_cancelled_with_its_button_or_escape() {
    let dir = TempDir::new("cancel-open");
    let path = fixture::write(dir.path(), "crowded.vsdx", &fixture::crowded_vsdx(40_000));
    for by_escape in [false, true] {
        let (mut window, tx) = controlled_with(|app| app.with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()));
        window.rest();
        let revision = window.state().session().revision();
        let answer = crate::support::send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
        let start = std::time::Instant::now();
        while window.state().element("opening.cancel").is_none() {
            window.step();
            assert!(start.elapsed() < Duration::from_secs(30), "the card with its Cancel button was not drawn");
        }
        // An area is placed by its size, which it learns in its first frame: the button is where it will stay a frame later.
        window.step();
        window.step();
        if by_escape {
            window.input_mut().events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            window.step();
        } else {
            let at = window.state().element("opening.cancel").unwrap().rect.center();
            crate::support::click(&mut window, at);
        }
        window.step();
        assert!(window.state().opening_now().is_none(), "the card is gone (escape: {by_escape})");
        assert_eq!(answer.try_recv().expect("the control channel is told").unwrap_err(), "Opening crowded.vsdx was cancelled.");
        assert_eq!(window.state().session().revision(), revision, "the drawing that was open is as it was");
        assert!(window.state().open_error().is_none(), "a cancel is not an error");
        assert!(window.state().notice().is_some_and(|n| n == "Cancelled opening crowded.vsdx."));
        // Another file can be opened now, and nothing the cancelled thread brings back replaces it.
        let quick = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
        let reply = call_until_answered(&mut window, &tx, &quick);
        assert!(reply.is_ok(), "{reply:?}");
        for _ in 0..40 {
            window.step();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(window.state().window_title(), "plans.vsdx - DiaCraft", "the cancelled file did not arrive late and take its place");
    }
}

/// Opens `path` through the control channel and steps the window until the answer comes: the file is read on a thread, and a loaded
/// machine may be slow to read it.
fn call_until_answered(
    window: &mut Window,
    tx: &std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>,
    path: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let answer = crate::support::send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            return reply;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened in 60 s");
}
