//! Issue 8: one file dialog at a time.
//!
//! File, Open, then Save As, then Export started three system dialogs at once: each had a guard against its own second start and none against the others. While one of
//! the app's file dialogs is open the others are refused (their buttons are greyed, their keys do nothing and a program that asks for one is told why), and
//! `ui.inspect` says which one is open (`fileDialog` in the window state: `open`, `save`, `export`, `picture`, `stencil`, `print`, or null). The browser's host has the same
//! rule: a file chooser the page has not answered, or the window's own card that names a file, holds the others off.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use diacraft_ui_egui::{Delivered, FileHost, Want};
use egui::{Context, Key};
use serde_json::json;

use crate::editing::{edit_window_with, key_without_waiting};
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, Window, press, run_command};

/// A door a dialog's thread waits at, and a count of how many times a dialog was asked for.
#[derive(Clone, Default)]
struct Gate {
    open: Arc<(Mutex<bool>, Condvar)>,
    asked: Arc<AtomicUsize>,
}

impl Gate {
    /// What a dialog does when it is shown: counts itself, and waits until the test lets it answer (at most 20 s, so a failing test ends).
    fn wait(&self) {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let (lock, door) = &*self.open;
        let mut released = lock.lock().unwrap();
        let mut waited = Duration::ZERO;
        while !*released && waited < Duration::from_secs(20) {
            let (guard, _) = door.wait_timeout(released, Duration::from_millis(50)).unwrap();
            released = guard;
            waited += Duration::from_millis(50);
        }
    }

    fn let_through(&self) {
        *self.open.0.lock().unwrap() = true;
        self.open.1.notify_all();
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// Waits until the dialog's thread has shown it (it is started in a frame and runs on its own).
    fn shown(&self, w: &mut Window) {
        for _ in 0..500 {
            if self.asked() > 0 {
                return;
            }
            w.step();
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the dialog was not shown");
    }
}

/// A window whose five dialogs wait at a gate each and answer nothing (the person cancels).
fn window_with_dialogs() -> (Window, [Gate; 5]) {
    let gates: [Gate; 5] = Default::default();
    let [open, save, export, picture, stencil] = gates.clone();
    let w = edit_window_with([1280.0, 900.0], move |app| {
        app.with_file_picker(Arc::new({
            let g = open.clone();
            move || {
                g.wait();
                None
            }
        }))
        .with_save_picker(Arc::new({
            let g = save.clone();
            move |_| {
                g.wait();
                None
            }
        }))
        .with_export_picker(Arc::new({
            let g = export.clone();
            move |_| {
                g.wait();
                None
            }
        }))
        .with_picture_picker(Arc::new({
            let g = picture.clone();
            move || {
                g.wait();
                None
            }
        }))
        .with_stencil_picker(Arc::new({
            let g = stencil.clone();
            move || {
                g.wait();
                None
            }
        }))
    });
    (w, gates)
}

fn file_dialog(w: &Window) -> serde_json::Value {
    w.state().window_state()["fileDialog"].clone()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// Presses a row of the File view and gives the window its frames, without waiting for the dialog it starts.
fn press_row(w: &mut Window, name: &str) {
    press(w, name);
    w.rest_holding();
}

#[test]
fn nothing_is_open_before_a_dialog_is_asked_for() {
    let (mut w, _) = window_with_dialogs();
    w.rest();
    assert!(file_dialog(&w).is_null(), "{}", file_dialog(&w));
}

/// Open, with its dialog not answered, then Save As and Export: the File view greys both rows, a click on one does nothing, and the keys of Save As and Open do nothing.
#[test]
fn while_the_open_dialog_is_up_save_as_and_export_are_refused() {
    let (mut w, [open, save, export, picture, stencil]) = window_with_dialogs();
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    assert!(
        enabled(&w, "backstage.Open") && enabled(&w, "backstage.Save As") && enabled(&w, "backstage.Export"),
        "all three can be used when none is open"
    );
    press_row(&mut w, "backstage.Open");
    open.shown(&mut w);
    assert_eq!(open.asked(), 1, "the Open dialog was shown");
    assert_eq!(file_dialog(&w), json!("open"));
    for row in ["backstage.Open", "backstage.Save As", "backstage.Export"] {
        assert!(!enabled(&w, row), "{row} is greyed while the Open dialog is up");
        press_row(&mut w, row);
    }
    assert_eq!((open.asked(), save.asked(), export.asked()), (1, 0, 0), "no second dialog was shown");
    // The keys: Save As (Ctrl+Shift+S), Save of a drawing with no file (Ctrl+S) and Open (Ctrl+O).
    for (k, mods) in [(Key::S, ctrl_shift()), (Key::S, ctrl()), (Key::O, ctrl())] {
        key_without_waiting(&mut w, k, mods);
        w.rest_holding();
    }
    assert_eq!((open.asked(), save.asked(), export.asked()), (1, 0, 0), "the keys started no dialog");
    let said = w.state().notice().unwrap_or_default().to_owned();
    assert!(said.contains("The Open dialog is open"), "the key says why it did nothing: {said:?}");
    // A program that asks for a chooser through the window's commands is told why not.
    let error = w.state_mut().execute("window.insertPicture", &json!({})).unwrap_err().to_string();
    assert!(error.contains("Open dialog"), "{error}");
    let error = w.state_mut().execute("shapes.openStencil", &json!({})).unwrap_err().to_string();
    assert!(error.contains("Open dialog"), "{error}");
    let error = w.state().command_enabled("file.export").unwrap_err();
    assert!(error.contains("Open dialog"), "{error}");
    assert_eq!((picture.asked(), stencil.asked()), (0, 0));
    // The person closes the dialog: the state clears, and the others work again.
    open.let_through();
    w.rest();
    assert!(file_dialog(&w).is_null(), "{}", file_dialog(&w));
    assert!(enabled(&w, "backstage.Open") && enabled(&w, "backstage.Save As") && enabled(&w, "backstage.Export"));
    press_row(&mut w, "backstage.Save As");
    save.shown(&mut w);
    assert_eq!(save.asked(), 1, "Save As shows its dialog now");
    assert_eq!(file_dialog(&w), json!("save"));
    assert!(!enabled(&w, "backstage.Open") && !enabled(&w, "backstage.Export"), "Open and Export wait for it");
    save.let_through();
    w.rest();
    assert!(file_dialog(&w).is_null());
    assert_eq!(w.state().window_state()["pictureDialog"], false);
}

/// Every dialog says its own name, and holds the other four off.
#[test]
fn each_dialog_names_itself_and_holds_the_others_off() {
    let (mut w, gates) = window_with_dialogs();
    let [open, save, export, picture, stencil] = &gates;
    // Export, from the File view.
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press_row(&mut w, "backstage.Export");
    export.shown(&mut w);
    assert_eq!((file_dialog(&w), export.asked()), (json!("export"), 1));
    assert!(!enabled(&w, "backstage.Open") && !enabled(&w, "backstage.Save As"));
    export.let_through();
    w.rest();
    // Insert a picture, asked for by the window's own command, as the Pictures button asks.
    w.state_mut().execute("window.insertPicture", &json!({})).unwrap();
    w.rest_holding();
    picture.shown(&mut w);
    assert_eq!((file_dialog(&w), picture.asked()), (json!("picture"), 1));
    assert!(w.state_mut().execute("shapes.openStencil", &json!({})).is_err(), "no stencil chooser over the picture chooser");
    w.state_mut().execute("window.backstage", &json!({"page": "Home"})).unwrap();
    w.rest_holding();
    assert!(!enabled(&w, "backstage.Open") && !enabled(&w, "backstage.Save As") && !enabled(&w, "backstage.Export"));
    picture.let_through();
    w.rest();
    // A stencil chooser.
    w.state_mut().execute("shapes.openStencil", &json!({})).unwrap();
    w.rest_holding();
    stencil.shown(&mut w);
    assert_eq!((file_dialog(&w), stencil.asked()), (json!("stencil"), 1));
    assert!(w.state_mut().execute("window.insertPicture", &json!({})).is_err(), "no picture chooser over the stencil chooser");
    stencil.let_through();
    w.rest();
    assert!(file_dialog(&w).is_null());
    assert_eq!((open.asked(), save.asked()), (0, 0));
}

// ---- the browser's host --------------------------------------------------------------------------------------------------------------------

/// The page, as a host that does not answer a chooser until the test says.
#[derive(Default)]
struct HoldingPage {
    waiting: Mutex<VecDeque<Sender<Option<PathBuf>>>>,
    choices: AtomicUsize,
}

impl HoldingPage {
    /// The person closes the chooser: nothing was chosen.
    fn close_chooser(&self) {
        if let Some(tx) = self.waiting.lock().unwrap().pop_front() {
            tx.send(None).unwrap();
        }
    }
}

impl FileHost for HoldingPage {
    fn choose(&self, _ctx: &Context, _want: Want) -> Receiver<Option<PathBuf>> {
        self.choices.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.waiting.lock().unwrap().push_back(tx);
        rx
    }

    fn deliver(&self, _ctx: &Context, _path: &Path) -> Receiver<Result<Delivered, String>> {
        let (tx, rx) = channel();
        tx.send(Err("nothing is handed over in this test".to_owned())).unwrap();
        rx
    }

    fn dropped(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn refusals(&self) -> Vec<String> {
        Vec::new()
    }

    fn release(&self, _path: &Path) {}
}

/// In the browser, a chooser the page has not answered holds off the card that names a file, and the card holds off the chooser.
#[test]
fn in_the_browser_a_chooser_and_the_name_card_hold_each_other_off() {
    let page = Arc::new(HoldingPage::default());
    let host = Arc::clone(&page);
    let mut w = edit_window_with([1280.0, 900.0], move |app| app.with_file_host(host));
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press_row(&mut w, "backstage.Open");
    assert_eq!(page.choices.load(Ordering::SeqCst), 1);
    assert_eq!(file_dialog(&w), json!("open"));
    assert!(!enabled(&w, "backstage.Save As") && !enabled(&w, "backstage.Export"), "the card is not offered over a chooser");
    key_without_waiting(&mut w, Key::S, ctrl_shift());
    w.rest_holding();
    assert!(!w.state().name_card_open(), "Ctrl+Shift+S raised no card while the chooser is up");
    page.close_chooser();
    w.rest();
    assert!(file_dialog(&w).is_null());
    // Save As: the page has no dialog of its own for a name, so the window's card is the dialog.
    press_row(&mut w, "backstage.Save As");
    assert!(w.state().name_card_open(), "the card is up");
    assert_eq!(file_dialog(&w), json!("save"));
    assert!(!w.state().can_pick_file(), "no chooser over the card");
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest_holding();
    assert_eq!(page.choices.load(Ordering::SeqCst), 1, "Ctrl+O asked the page for no second chooser");
}
