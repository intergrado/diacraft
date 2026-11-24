//! How a test lets a window come to rest (`support::Rest`): it waits for the threads the window started, whenever they answer, and it still
//! fails for a window that never stops asking for frames. (the window's tests failed on GitHub's slower runner, where the Save dialog's
//! thread answered in the middle of `Harness::run`.)

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_ui_egui::font_list::Scan;
use diacraft_ui_egui::{SavePicker, Stage};
use egui::Key;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key};
use crate::saving::ctrl;
use crate::support::{Rest, TempDir, run_command};

/// **A thread that a frame starts is waited for too.** The Text dialog starts the scan for its font list as it is first drawn, which is inside the frames
/// of the first rest, so `working` had nothing to say when the rest began; a scan that answered in the middle of those frames (it does on a machine where
/// the scan is quick and the frames are not: GitHub run 42398195820) added the frames of its answer to the four the rest allows, and "exceeded max_steps (4). It
/// would have settled after 6 steps" failed a test of a window that works. The scan is held here and let go in each frame of the first rest in turn.
#[test]
fn a_font_scan_that_answers_in_any_frame_of_the_dialogs_first_rest_does_not_fail_the_wait() {
    for answers_in_frame in 0..=8 {
        let (let_go, wait) = channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let scan: Scan = Arc::new(move || {
            let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(30));
            vec!["Alpha Serif".to_owned(), "Beta Sans".to_owned()]
        });
        let countdown = Rc::new(Cell::new(-1_i32));
        let probe_countdown = Rc::clone(&countdown);
        let mut w = edit_window_with([1280.0, 900.0], |app| {
            app.with_font_scan(scan).with_probe(move |stage| {
                if !matches!(stage, Stage::Frame) || probe_countdown.get() < 0 {
                    return;
                }
                if probe_countdown.get() == 0 {
                    let _ = let_go.send(());
                    std::thread::sleep(Duration::from_millis(40));
                }
                probe_countdown.set(probe_countdown.get() - 1);
            })
        });
        drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
        countdown.set(answers_in_frame);
        run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
        assert!(!w.state().fonts_scanning(), "the scan answered (frame {answers_in_frame})");
        assert!(w.state().element("textdialog.font.item.Alpha Serif").is_some(), "the list is in the box (frame {answers_in_frame})");
    }
}

/// The Save dialog answers in whichever frame of the key's wait the machine happens to let its thread run in: before the key is pressed,
/// in the frames of the press and release, or in any of the frames after them. The window ends the same way each time.
#[test]
fn a_dialog_that_answers_in_any_frame_does_not_fail_the_wait_for_the_window_to_rest() {
    for answers_in_frame in 0..=8 {
        let dir = TempDir::new("rest-dialog");
        let target = dir.file("plan.vsdx");
        // The dialog's thread waits for the probe, which lets it go at the start of the chosen frame and then gives it time to answer, so
        // that the answer is taken in the frame after: the order of events a loaded machine decides by chance, fixed.
        let (let_go, wait) = channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let picker: SavePicker = {
            let target = target.clone();
            Arc::new(move |_| {
                let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(30));
                Some(target.clone())
            })
        };
        let countdown = Rc::new(Cell::new(-1_i32));
        let probe_countdown = Rc::clone(&countdown);
        let mut w = edit_window_with([1280.0, 900.0], |app| {
            app.with_save_picker(picker).with_probe(move |stage| {
                if !matches!(stage, Stage::Frame) || probe_countdown.get() < 0 {
                    return;
                }
                if probe_countdown.get() == 0 {
                    let _ = let_go.send(());
                    std::thread::sleep(Duration::from_millis(40));
                }
                probe_countdown.set(probe_countdown.get() - 1);
            })
        });
        drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
        countdown.set(answers_in_frame);
        key(&mut w, Key::S, ctrl());
        assert!(target.exists(), "the drawing was saved (the dialog answered in frame {answers_in_frame})");
        assert!(!w.state().is_dirty(), "frame {answers_in_frame}");
        assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved plan.vsdx")), "frame {answers_in_frame}: {:?}", w.state().notice());
        assert_eq!(w.state().window_title(), "plan.vsdx - DiaCraft", "frame {answers_in_frame}");
    }
}

/// Waiting for the window's threads does not make a window that repaints for ever rest: the frames that ask for another at once are still
/// counted, and more than four in a row fail the test.
#[test]
#[should_panic(expected = "exceeded max_steps")]
fn a_window_that_asks_for_a_frame_in_every_frame_still_fails_to_rest() {
    let context = Rc::new(RefCell::new(None::<egui::Context>));
    let probe_context = Rc::clone(&context);
    let mut w = edit_window_with([800.0, 600.0], |app| {
        app.with_probe(move |stage| {
            if let (Stage::Frame, Some(ctx)) = (stage, &*probe_context.borrow()) {
                ctx.request_repaint();
            }
        })
    });
    *context.borrow_mut() = Some(w.ctx.clone());
    w.rest();
}

/// No test calls `Harness::run` on a window itself: `Rest::rest` is the way to let one come to rest (`support.rs`), and the rule is kept
/// here because the next test written the old way would pass on a fast machine and fail on a slow one.
#[test]
fn no_test_lets_a_window_come_to_rest_with_run() {
    let needle = [".ru", "n()"].concat();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("it");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".rs") || name == "support.rs" {
            continue;
        }
        for (number, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
            if line.contains(&needle) {
                found.push(format!("{name}:{}", number + 1));
            }
        }
    }
    assert!(found.is_empty(), "use `.rest()` (support::Rest), not `run`: {found:?}");
}
