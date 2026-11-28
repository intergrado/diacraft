//! The window in a page of a browser: the keys it takes (Ctrl+N and Ctrl+W), a window of its own, full screen, and what its tooltips say of a key the browser keeps. The page's side is a host of the
//! test's own, so nothing here needs a browser: what the real browsers do with the keys was measured with real key presses and is in `docs/website.md`.

use std::sync::{Arc, Mutex};

use diacraft_engine::Session;
use diacraft_ui_egui::shortcuts::{BROWSER_KEPT, KeyTaker, kept_by_browser, keys_to_take, shortcut_of};
use diacraft_ui_egui::{Browser, DiacraftApp, Offer, PageHost};
use egui::Key;
use egui::os::OperatingSystem;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key};
use crate::saving::ctrl;
use crate::support::{Rest, TempDir, Window, click, fast_window, press, run_command};

/// What the host was asked, and what it answers.
#[derive(Default)]
struct Page {
    browser: Mutex<Option<Browser>>,
    own_window: Mutex<bool>,
    can_prompt: Mutex<bool>,
    /// `None` out of full screen; else whether the keyboard is locked.
    full: Mutex<Option<bool>>,
    /// Whether going to full screen locks the keyboard.
    grants_lock: Mutex<bool>,
    prompts: Mutex<u32>,
    entered: Mutex<u32>,
    left: Mutex<u32>,
    closed: Mutex<u32>,
    refusal: Mutex<Option<String>>,
    /// Whether the browser would ask before the page is left now: the last `guard_leaving` it was told.
    guard: Mutex<bool>,
    /// Every `guard_leaving` call, in order.
    guard_told: Mutex<Vec<bool>>,
    /// What `guard` was at each `close` call: a page must have stopped asking by then.
    guard_at_close: Mutex<Vec<bool>>,
    /// The browser keeps the window when asked to close it: `close` answers `false`.
    keeps_window: Mutex<bool>,
}

impl Page {
    fn new(browser: Browser) -> Arc<Page> {
        let page = Page::default();
        *page.browser.lock().unwrap() = Some(browser);
        *page.grants_lock.lock().unwrap() = true;
        Arc::new(page)
    }

    fn count(field: &Mutex<u32>) -> u32 {
        *field.lock().unwrap()
    }
}

impl PageHost for Page {
    fn browser(&self) -> Browser {
        self.browser.lock().unwrap().unwrap_or(Browser::Other)
    }

    fn own_window(&self) -> bool {
        *self.own_window.lock().unwrap()
    }

    fn can_prompt(&self) -> bool {
        *self.can_prompt.lock().unwrap()
    }

    fn prompt(&self) {
        *self.prompts.lock().unwrap() += 1;
        *self.can_prompt.lock().unwrap() = false;
    }

    fn full_screen(&self) -> Option<bool> {
        *self.full.lock().unwrap()
    }

    fn enter_full_screen(&self) {
        *self.entered.lock().unwrap() += 1;
        *self.full.lock().unwrap() = Some(*self.grants_lock.lock().unwrap());
    }

    fn leave_full_screen(&self) {
        if self.full.lock().unwrap().take().is_some() {
            *self.left.lock().unwrap() += 1;
        }
    }

    fn refusal(&self) -> Option<String> {
        self.refusal.lock().unwrap().take()
    }

    fn guard_leaving(&self, on: bool) {
        self.guard_told.lock().unwrap().push(on);
        *self.guard.lock().unwrap() = on;
    }

    fn close(&self) -> bool {
        *self.closed.lock().unwrap() += 1;
        self.guard_at_close.lock().unwrap().push(*self.guard.lock().unwrap());
        !*self.keeps_window.lock().unwrap()
    }
}

fn window_in(page: &Arc<Page>) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let host: Arc<dyn PageHost> = page.clone();
    let mut w = fast_window(DiacraftApp::new(session, None).with_page_host(host), [1280.0, 900.0]);
    // The keys are a PC's whatever the machine the test runs on is.
    w.ctx.set_os(OperatingSystem::Nix);
    w.rest();
    w
}

/// A window in a page that saves to `target` when the Save dialog is asked for a name.
fn window_in_with_save(page: &Arc<Page>, target: &std::path::Path) -> Window {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let picker = {
        let asked = Arc::clone(&asked);
        let target = target.to_path_buf();
        let picker: diacraft_ui_egui::SavePicker = Arc::new(move |request| {
            asked.lock().unwrap().push(request.name);
            Some(target.clone())
        });
        picker
    };
    let host: Arc<dyn PageHost> = page.clone();
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_save_picker(picker).with_page_host(host));
    w.ctx.set_os(OperatingSystem::Nix);
    w.rest();
    w
}

fn desktop_window() -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None), [1280.0, 900.0]);
    w.ctx.set_os(OperatingSystem::Nix);
    w.rest();
    w
}

fn drawing_count(w: &Window) -> usize {
    w.state().session().document().map_or(0, |doc| doc.pages.iter().map(|p| p.shapes.len()).sum())
}

// ------------------------------------------------------------------------------------------------------------------------------ the keys

/// Ctrl+N makes a new drawing in the window of a page, as it does on the desktop (the window runs the key: that the browser passes it is the page's business).
#[test]
fn ctrl_n_makes_a_new_drawing_in_the_window_of_a_page() {
    let page = Page::new(Browser::Chromium);
    *page.own_window.lock().unwrap() = true;
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert_eq!(drawing_count(&w), 1);
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open(), "a drawing with changes asks first");
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert_eq!(drawing_count(&w), 0, "the new drawing is empty");
    assert!(!w.state().is_dirty());
}

/// Ctrl+W closes the window of a page, after the question about unsaved changes. A clean drawing closes at once; a drawing with changes asks, and Cancel keeps the window.
#[test]
fn ctrl_w_closes_the_window_of_a_page_after_the_question_about_unsaved_changes() {
    let page = Page::new(Browser::Chromium);
    *page.own_window.lock().unwrap() = true;
    let mut w = window_in(&page);
    key(&mut w, Key::W, ctrl());
    assert_eq!(Page::count(&page.closed), 1, "a drawing with nothing to save closes at once");
    assert!(!w.state().prompt_open());

    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::W, ctrl());
    assert!(w.state().prompt_open(), "a drawing with changes asks first");
    assert_eq!(Page::count(&page.closed), 0, "and nothing is closed meanwhile");
    w.get_by_label_contains("Do you want to save changes to Drawing1?");
    press(&mut w, "prompt.cancel");
    w.rest();
    assert_eq!(Page::count(&page.closed), 0, "Cancel keeps the window");
    assert!(!w.state().prompt_open());
    key(&mut w, Key::W, ctrl());
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert_eq!(Page::count(&page.closed), 1, "Don't Save closes it");
}

/// Ctrl+W works from the File view too (where the window starts, with no drawing open), as Ctrl+N, Ctrl+O and Ctrl+S do.
#[test]
fn ctrl_w_closes_the_window_from_the_file_view() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    key(&mut w, Key::W, ctrl());
    assert_eq!(Page::count(&page.closed), 1, "from the File view");
    // With a drawing that has changes, the question is the File view's too.
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    key(&mut w, Key::W, ctrl());
    assert!(w.state().prompt_open());
    assert_eq!(Page::count(&page.closed), 0);
}

/// The desktop window has no Ctrl+W (its frame closes it, and the system's Alt+F4): the key is bound only where there is a page.
#[test]
fn the_desktop_window_has_no_ctrl_w_and_says_why_the_browser_commands_cannot_run() {
    let mut w = desktop_window();
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::W, ctrl());
    assert!(!w.state().prompt_open(), "no question: Ctrl+W is no key of the desktop window");
    for id in ["app.ownWindow", "window.fullScreen", "app.closeWindow"] {
        let why = w.state().command_enabled(id).unwrap_err();
        assert!(why.starts_with("only the browser version"), "{id}: {why}");
        assert!(w.state_mut().execute(id, &json!({})).is_err(), "{id} refuses on the desktop");
    }
}

/// The keys the page takes are the chords of the command key (and the function keys) of the window's commands, except the clipboard's own: the keys a browser keeps in a tab are among them, and so
/// are the ones it passes to the page and acts on all the same (Ctrl+R reloads the page, Ctrl+H opens its history).
#[test]
fn the_keys_the_page_takes_are_the_commands_chords_and_function_keys_but_not_the_clipboards() {
    for os in [OperatingSystem::Nix, OperatingSystem::Windows, OperatingSystem::Mac] {
        let taken = keys_to_take(os);
        assert!(!taken.is_empty(), "{os:?}");
        // `Cmd` in a registry key is the platform's command key: egui's `command`.
        let command = egui::Modifiers::COMMAND;
        let new = shortcut_of(os, "file.new", true).expect("New has a key");
        let close = shortcut_of(os, "app.closeWindow", true).expect("Close has a key on the web");
        assert!(taken.contains(&new) && taken.contains(&close), "{os:?}: Ctrl+N and Ctrl+W are taken");
        assert_eq!(new.logical_key, Key::N);
        assert_eq!(close.logical_key, Key::W);
        assert!(new.modifiers.matches_logically(command) && close.modifiers.matches_logically(command), "{os:?}");
        // Every key the browser keeps in a tab that a command has is taken.
        for key in &shortcuts_of_commands(os) {
            if kept_by_browser(os, key) {
                assert!(taken.contains(key), "{os:?}: a key the browser keeps and a command has is taken");
            }
        }
        // The clipboard's own are not: the browser's copy, cut and paste events are how the window gets them.
        for key in [Key::C, Key::X, Key::V] {
            assert!(
                !taken.iter().any(|t| t.logical_key == key && t.modifiers == command),
                "{os:?}: {key:?} with the command key alone is left to the browser"
            );
        }
        // Rotate Right, Flip, Underline, Find and Duplicate are chords that reach the page in a tab and that the browser acts on: taken.
        for key in [Key::R, Key::U, Key::F, Key::D] {
            assert!(taken.iter().any(|t| t.logical_key == key && t.modifiers == command), "{os:?}: {key:?}");
        }
        // Flip Horizontal is Ctrl+H, and on a Mac (where Command+H hides the app) Command+Shift+H.
        let flip = if os == OperatingSystem::Mac { command | egui::Modifiers::SHIFT } else { command };
        assert!(taken.iter().any(|t| t.logical_key == Key::H && t.modifiers == flip), "{os:?}: Flip Horizontal");
        // Plain keys are left alone: a letter, Delete, Escape and Enter are no chord of the command key and no function key.
        assert!(
            taken.iter().all(|t| t.modifiers.command || t.modifiers.ctrl || t.modifiers.mac_cmd || t.logical_key.name().starts_with('F')),
            "{os:?}"
        );
    }
    // On a Mac the Fonts window is Command+T, which a browser keeps for a new tab; elsewhere Ctrl+T has no command.
    let mac = keys_to_take(OperatingSystem::Mac);
    assert!(mac.iter().any(|t| t.logical_key == Key::T && t.modifiers == egui::Modifiers::COMMAND), "Command+T runs the Fonts window and is taken");
    let linux = keys_to_take(OperatingSystem::Nix);
    assert!(
        !linux.iter().any(|t| t.logical_key == Key::T && t.modifiers == egui::Modifiers::COMMAND),
        "Ctrl+T has no command here: the browser keeps it"
    );
    // Fit Page to Window is Ctrl+Shift+W off a Mac, and the browser closes its window with that key: it is taken there, and the Mac has Command+0.
    assert!(linux.iter().any(|t| t.logical_key == Key::W && t.modifiers == (egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)));
    assert!(!mac.iter().any(|t| t.logical_key == Key::W && t.modifiers.shift), "a Mac has no command on Command+Shift+W");
    // Ctrl+O, Ctrl+P and Ctrl+S, which the web runner prevents already, are taken too (twice is harmless), and keep working as they did.
    for key in [Key::O, Key::P, Key::S] {
        assert!(linux.iter().any(|t| t.logical_key == key && t.modifiers == egui::Modifiers::COMMAND), "{key:?}");
    }
}

/// Every key a command of the browser version has on `os`.
fn shortcuts_of_commands(os: OperatingSystem) -> Vec<egui::KeyboardShortcut> {
    diacraft_ui_egui::shortcuts::bound_keys(os, true)
}

/// What the page's handler decides, from the fields of a `keydown`: the key and its code, then Ctrl, Meta, Shift and Alt.
#[test]
fn the_handler_takes_a_keydown_that_is_a_chord_of_a_command_and_not_the_clipboards() {
    let linux = KeyTaker::for_user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/158.0.0.0 Safari/537.36");
    assert!(linux.count() > 0);
    assert!(linux.takes("n", "KeyN", true, false, false, false), "Ctrl+N");
    assert!(linux.takes("w", "KeyW", true, false, false, false), "Ctrl+W");
    assert!(linux.takes("W", "KeyW", true, false, true, false), "Ctrl+Shift+W (Fit Page): the browser closes its window with it");
    // The web runner prevents Ctrl+S, Ctrl+O and Ctrl+P itself: the page takes them too (the second call changes nothing), and stops no event, so they keep working as they did.
    for (key, code) in [("s", "KeyS"), ("o", "KeyO"), ("p", "KeyP")] {
        assert!(linux.takes(key, code, true, false, false, false), "Ctrl+{key}");
    }
    assert!(linux.takes("r", "KeyR", true, false, false, false), "Ctrl+R is Rotate Right, and the browser would reload the page");
    assert!(linux.takes("-", "Minus", true, false, false, false), "Ctrl+- is Zoom Out, and the browser would zoom the page");
    assert!(linux.takes("=", "Equal", true, false, false, false), "Ctrl+= is Zoom In");
    // On a Portuguese keyboard `=` is Shift and 0: the press arrives as Ctrl+Shift with the key `=`, and the window zooms in for it, so the page takes it (measured, Chrome 158, xdotool).
    assert!(linux.takes("=", "Digit0", true, false, true, false), "Ctrl+Shift with `=` is Zoom In too");
    // A letter's shortcut with no Shift in it is not run with Shift held: Ctrl+Shift+N is no key of the window, and the browser keeps it.
    assert!(!linux.takes("N", "KeyN", true, false, true, false), "Ctrl+Shift+N is no Ctrl+N");
    assert!(linux.takes("T", "KeyT", true, false, true, false), "Ctrl+Shift+T is Align Text Top, and the browser would reopen a closed tab");
    assert!(!linux.takes("c", "KeyC", true, false, false, false), "Ctrl+C is the browser's copy event");
    assert!(!linux.takes("v", "KeyV", true, false, false, false), "Ctrl+V is the browser's paste event");
    assert!(!linux.takes("e", "KeyE", true, false, false, false), "a key no command has is not taken");
    assert!(!linux.takes("n", "KeyN", false, false, false, false), "N alone is a letter");
    // An extra Alt does not stand (egui's `matches_logically` let it, and AltGr is Ctrl and Alt): the window runs New for Ctrl+N alone, and the page takes the press the window runs a command for.
    assert!(!linux.takes("n", "KeyN", true, false, false, true), "Ctrl+Alt+N: the window runs no New for it");
    assert!(!linux.takes("n", "KeyN", true, true, false, false), "the Windows key is not ours");
    assert!(!linux.takes("t", "KeyT", true, false, false, false), "Ctrl+T has no command");
    assert!(!linux.takes("Dead", "KeyN", false, false, false, false));
    // The key's code stands in where the key has no name: a layout that types something else for the key.
    assert!(linux.takes("\u{ff}", "KeyN", true, false, false, false), "the code names the key when the character does not");

    let mac = KeyTaker::for_user_agent(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
    );
    assert!(mac.takes("n", "KeyN", false, true, false, false), "Command+N");
    assert!(mac.takes("t", "KeyT", false, true, false, false), "Command+T is the Fonts window on a Mac");
    assert!(mac.takes("w", "KeyW", false, true, false, false), "Command+W");
    assert!(!mac.takes("n", "KeyN", true, false, false, false), "Control+N is not Command+N on a Mac");
    assert!(mac.takes("s", "KeyS", false, true, false, false), "Command+S: the web runner prevents it too");
    assert!(!mac.takes("c", "KeyC", false, true, false, false), "Command+C is the browser's copy event");
}

/// Every key a browser keeps in a tab ([`BROWSER_KEPT`]) says what the browser does with it, and each parses.
#[test]
fn every_kept_key_says_what_the_browser_does_with_it() {
    for (spec, does) in BROWSER_KEPT {
        let parsed = diacraft_ui_egui::shortcuts::parse(spec);
        assert!(parsed.is_some(), "{spec} is not a key");
        assert!(!does.is_empty() && !does.contains(&["Vis", "io"].concat()), "{spec}: {does}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ asking before the page is left

/// The host is told to ask before the page is left while the drawing has unsaved changes, and only then: an idle page, a saved drawing and a new one ask nothing, and the host is told when the answer
/// changes and not on every frame.
#[test]
fn the_browser_is_asked_to_ask_before_the_page_is_left_only_while_the_drawing_has_unsaved_changes() {
    let page = Page::new(Browser::Chromium);
    let dir = TempDir::new("unload-guard");
    let mut w = window_in_with_save(&page, &dir.file("plan.vsdx"));
    for _ in 0..5 {
        w.step();
    }
    assert!(page.guard_told.lock().unwrap().is_empty(), "an idle page asks the host for nothing");
    assert!(!*page.guard.lock().unwrap());

    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    assert!(*page.guard.lock().unwrap(), "an edit: the browser asks before the page is left");
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(*page.guard_told.lock().unwrap(), [true], "told once, not on every frame");

    // An edit taken back leaves the drawing clean: nothing to lose.
    key(&mut w, Key::Z, ctrl());
    assert!(!w.state().is_dirty());
    assert!(!*page.guard.lock().unwrap(), "undone to the saved state: the browser asks nothing");
    assert_eq!(*page.guard_told.lock().unwrap(), [true, false]);

    // A save clears it, in the frame that saved.
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(*page.guard.lock().unwrap());
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(!w.state().is_dirty(), "the drawing is saved");
    assert!(!*page.guard.lock().unwrap(), "saved: the browser asks nothing");
    assert_eq!(*page.guard_told.lock().unwrap(), [true, false, true, false]);

    // Another change, and it is on again; the same by a command, with no key and no frame between it and the answer.
    drag_tile_to(&mut w, "Diamond", 7.0, 4.0);
    assert!(*page.guard.lock().unwrap());
    run_command(&mut w, "file.save", json!({}));
    assert!(!*page.guard.lock().unwrap(), "saved by a command: the browser asks nothing");
    assert_eq!(page.guard_told.lock().unwrap().len(), 6);
}

/// A new drawing, and a drawing opened in place of the changed one, leave nothing to lose: the browser asks nothing after them.
#[test]
fn a_new_drawing_takes_the_question_away() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(*page.guard.lock().unwrap());
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open(), "the drawing has changes: the window asks first");
    assert!(*page.guard.lock().unwrap(), "and the browser still asks while the person decides");
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert!(!w.state().is_dirty());
    assert!(!*page.guard.lock().unwrap(), "the new drawing has nothing to lose");
    assert_eq!(*page.guard_told.lock().unwrap(), [true, false]);
}

/// Ctrl+W asks DiaCraft's question first, and the browser's box is not shown on top of it: the guard is on while the question is up, and off when the window asks the browser to close.
#[test]
fn ctrl_w_has_stopped_the_browser_asking_when_it_closes_the_window() {
    // Don't Save.
    let page = Page::new(Browser::Chromium);
    *page.own_window.lock().unwrap() = true;
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::W, ctrl());
    assert!(w.state().prompt_open());
    assert!(*page.guard.lock().unwrap(), "while the question is up, leaving the page would still lose the drawing");
    press(&mut w, "prompt.cancel");
    w.rest();
    assert_eq!(Page::count(&page.closed), 0);
    assert!(*page.guard.lock().unwrap(), "Cancel keeps the window, and the browser still asks");
    key(&mut w, Key::W, ctrl());
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert_eq!(Page::count(&page.closed), 1);
    assert_eq!(
        *page.guard_at_close.lock().unwrap(),
        [false],
        "the guard was off when the window asked the browser to close, so there is no second question"
    );

    // Save: the drawing is clean when the window closes.
    let dir = TempDir::new("unload-close");
    let page = Page::new(Browser::Chromium);
    let mut w = window_in_with_save(&page, &dir.file("plan.vsdx"));
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::W, ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    assert_eq!(Page::count(&page.closed), 1, "saved, then closed");
    assert_eq!(*page.guard_at_close.lock().unwrap(), [false]);

    // A drawing with nothing to save: the guard was never on.
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    key(&mut w, Key::W, ctrl());
    assert_eq!(Page::count(&page.closed), 1);
    assert_eq!(*page.guard_at_close.lock().unwrap(), [false]);
    assert!(page.guard_told.lock().unwrap().is_empty(), "no question at any time");
}

/// A close the browser keeps (a tab a script did not open) leaves the window, and the drawing, where they were: the host is told again that the drawing has changes, to ask before the page is left.
#[test]
fn a_window_the_browser_keeps_open_asks_again_before_the_page_is_left() {
    let page = Page::new(Browser::Chromium);
    *page.keeps_window.lock().unwrap() = true;
    let mut w = window_in(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::W, ctrl());
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert_eq!(*page.guard_at_close.lock().unwrap(), [false], "off when the close was asked for");
    assert!(w.state().notice().is_some_and(|n| n.contains("The browser keeps this window open")), "{:?}", w.state().notice());
    assert!(w.state().is_dirty(), "Don't Save left the drawing as it was");
    assert!(*page.guard.lock().unwrap(), "the window is still here with changes: the host is asked to ask again");
}

/// The desktop window has no page to ask: an edit changes nothing there.
#[test]
fn the_desktop_window_has_no_page_to_ask() {
    let mut w = desktop_window();
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    assert!(!w.state().in_browser());
}

// ------------------------------------------------------------------------------------------------------------------------------ a window of its own

fn open_home(w: &mut Window) {
    run_command(w, "window.backstage", json!({"page": "Home"}));
}

/// Where the browser gives an install prompt, the File view's Home page offers a button that asks for it, once.
#[test]
fn the_home_page_offers_a_window_of_its_own_with_a_button_where_the_browser_has_a_prompt() {
    let page = Page::new(Browser::Chromium);
    *page.can_prompt.lock().unwrap() = true;
    let mut w = window_in(&page);
    assert_eq!(w.state().own_window_offer(), Offer::Prompt);
    open_home(&mut w);
    w.get_by_label("In a window of its own");
    let button = w.state().element("backstage.ownWindow").expect("the button is drawn");
    assert!(button.enabled);
    assert!(button.rect.width() > 100.0 && button.rect.height() > 20.0, "{:?}", button.rect);
    let at = button.rect.center();
    click(&mut w, at);
    w.rest();
    assert_eq!(Page::count(&page.prompts), 1, "the prompt was asked for");
    assert!(w.state().element("backstage.ownWindow").is_none(), "the prompt is used once: the button goes");
    // The command says the same where there is no prompt: in words.
    let why = w.state().command_enabled("app.ownWindow").unwrap_err();
    assert!(why.contains("install"), "{why}");
}

/// Where the browser gives none the page says what to do instead, in words: Safari's File, Add to Dock; Firefox has no windows for web apps on Linux or macOS.
#[test]
fn where_the_browser_has_no_prompt_the_page_says_the_steps_in_words() {
    for (browser, words) in [
        (Browser::Safari, "File, then Add to Dock"),
        (Browser::Firefox, "Firefox has no windows of their own for web apps on Linux or macOS"),
        (Browser::Chromium, "install button"),
        (Browser::Other, "may not open web apps"),
    ] {
        let page = Page::new(browser);
        let mut w = window_in(&page);
        open_home(&mut w);
        w.get_by_label("In a window of its own");
        assert!(w.state().element("backstage.ownWindow").is_none(), "{browser:?}: no button without a prompt");
        let steps = w.state().element("backstage.ownWindowSteps").unwrap_or_else(|| panic!("{browser:?}: the steps are drawn"));
        assert!(steps.rect.width() > 100.0 && steps.rect.height() > 10.0);
        w.get_by_label_contains(words);
        let why = w.state().command_enabled("app.ownWindow").unwrap_err();
        assert!(why.contains(words), "{browser:?}: {why}");
    }
}

/// Inside a window of its own nothing is offered, and on the desktop nothing either: the Home page is as it was.
#[test]
fn a_window_of_its_own_and_the_desktop_are_offered_nothing() {
    let page = Page::new(Browser::Chromium);
    *page.own_window.lock().unwrap() = true;
    *page.can_prompt.lock().unwrap() = true;
    let mut w = window_in(&page);
    assert_eq!(w.state().own_window_offer(), Offer::None);
    open_home(&mut w);
    assert!(w.query_by_label("In a window of its own").is_none());
    assert!(w.state().element("backstage.ownWindow").is_none() && w.state().element("backstage.ownWindowSteps").is_none());
    assert!(w.state().command_enabled("app.ownWindow").unwrap_err().contains("already"));

    let mut w = desktop_window();
    assert_eq!(w.state().own_window_offer(), Offer::None);
    open_home(&mut w);
    assert!(w.query_by_label("In a window of its own").is_none());
}

/// The words on the Home page name the keys of the window it is: Ctrl on a PC, and the command key of a Mac.
#[test]
fn the_offer_names_the_keys_the_window_takes() {
    let page = Page::new(Browser::Safari);
    let mut w = window_in(&page);
    open_home(&mut w);
    w.get_by_label_contains("Ctrl+N");
    w.get_by_label_contains("Ctrl+W");
}

// ------------------------------------------------------------------------------------------------------------------------------ full screen

/// Full Screen is on the View tab; a press asks the host, the box is checked while the page is in full screen, and the status bar says how to leave it. A second press leaves.
#[test]
fn full_screen_is_a_command_of_the_view_tab_and_says_how_to_leave() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert!(w.state().element("ribbon.View.BrowserFullScreen").is_some(), "the control is on the View tab");
    assert!(w.state().element("ribbon.View.BrowserFullScreen").unwrap().enabled);
    assert!(!w.state().is_full_screen());
    press(&mut w, "ribbon.View.BrowserFullScreen");
    w.rest();
    assert_eq!(Page::count(&page.entered), 1);
    assert!(w.state().is_full_screen());
    assert_eq!(w.state().notice(), Some("Full screen. Hold Esc to leave it."));
    assert!(w.state().browser_keys_reach_window(), "the keyboard is locked: the browser's keys reach the window");
    press(&mut w, "ribbon.View.BrowserFullScreen");
    w.rest();
    assert_eq!(Page::count(&page.left), 1, "a second press leaves");
    assert!(!w.state().is_full_screen());
    assert!(!w.state().browser_keys_reach_window(), "back in a tab the browser keeps its keys");
}

/// The browser answers the lock a moment after full screen: the window says what it got when it has it, and does not say the keys were kept while the answer is on its way.
#[test]
fn a_lock_that_comes_after_the_full_screen_replaces_the_first_words() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    // The page is in full screen with no lock yet: the host says it is not in full screen until the lock is known (`entering`), so the window says nothing.
    w.state_mut().execute("window.fullScreen", &json!({"on": true})).unwrap();
    *page.full.lock().unwrap() = None;
    w.step();
    assert_eq!(w.state().notice(), None, "nothing is said while the lock is on its way");
    *page.full.lock().unwrap() = Some(true);
    w.step();
    assert_eq!(w.state().notice(), Some("Full screen. Hold Esc to leave it."));
    // A host that reports the full screen first and the lock after (not the web page, which waits) is corrected when the lock comes.
    *page.full.lock().unwrap() = Some(false);
    w.step();
    assert!(w.state().notice().unwrap().contains("kept its keys"));
    *page.full.lock().unwrap() = Some(true);
    w.step();
    assert_eq!(w.state().notice(), Some("Full screen. Hold Esc to leave it."), "the lock came: the words are corrected");
}

/// A browser that gives full screen and no keyboard lock says so, and its keys stay the browser's.
#[test]
fn full_screen_without_the_keyboard_lock_says_the_browser_kept_its_keys() {
    let page = Page::new(Browser::Firefox);
    *page.grants_lock.lock().unwrap() = false;
    let mut w = window_in(&page);
    run_command(&mut w, "window.fullScreen", json!({"on": true}));
    w.rest();
    assert!(w.state().is_full_screen());
    assert!(w.state().notice().unwrap().contains("this browser kept its keys"), "{:?}", w.state().notice());
    assert!(w.state().notice().unwrap().contains("Hold Esc"));
    assert!(!w.state().browser_keys_reach_window());
    run_command(&mut w, "window.fullScreen", json!({"on": false}));
    assert!(!w.state().is_full_screen());
    assert_eq!(Page::count(&page.left), 1);
}

/// Leaving by the browser's own Esc (held) is noticed: the box is clear again with nobody pressing anything.
#[test]
fn leaving_full_screen_by_the_browsers_own_key_clears_the_box() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    run_command(&mut w, "window.fullScreen", json!({"on": true}));
    w.rest();
    assert!(w.state().is_full_screen());
    *page.full.lock().unwrap() = None;
    w.step();
    w.rest();
    assert!(!w.state().is_full_screen());
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    w.rest();
}

/// A refusal the browser gives (no click, not allowed) is said in the status bar.
#[test]
fn a_refusal_of_the_browser_is_said_in_the_status_bar() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    *page.refusal.lock().unwrap() = Some("The browser did not go to full screen: not allowed.".to_owned());
    w.step();
    assert_eq!(w.state().notice(), Some("The browser did not go to full screen: not allowed."));
}

// ------------------------------------------------------------------------------------------------------------------------------ tooltips

/// A tooltip never shows a key that opens a browser tab: in a tab it says the browser keeps Ctrl+N, and in a window of its own, or in full screen with the keyboard locked, it shows the key.
#[test]
fn a_tooltip_shows_a_key_only_where_it_reaches_the_window() {
    let page = Page::new(Browser::Chromium);
    let w = window_in(&page);
    let tip = |w: &Window| w.state().tip_with_key(&w.ctx, "Blank drawing", true, "file.new");
    let in_a_tab = tip(&w);
    assert!(in_a_tab.contains("Ctrl+N belongs to the browser in a tab"), "{in_a_tab}");
    assert!(!in_a_tab.ends_with("(Ctrl+N)"), "{in_a_tab}");

    *page.own_window.lock().unwrap() = true;
    assert_eq!(tip(&w), "Blank drawing (Ctrl+N)");
    *page.own_window.lock().unwrap() = false;
    *page.full.lock().unwrap() = Some(true);
    assert_eq!(tip(&w), "Blank drawing (Ctrl+N)", "full screen with the keyboard locked");
    *page.full.lock().unwrap() = Some(false);
    assert!(tip(&w).contains("belongs to the browser"), "full screen without the lock keeps the browser's keys");

    // A key the browser leaves to the page is shown as it is, and the desktop's tooltips are what they were.
    assert_eq!(w.state().tip_with_key(&w.ctx, "Save", true, "file.save"), "Save (Ctrl+S)");
    let desktop = desktop_window();
    assert_eq!(desktop.state().tip_with_key(&desktop.ctx, "Blank drawing", true, "file.new"), "Blank drawing");
}

/// On the Home page the Blank drawing tile carries that tooltip (the tile is the one control of the page that makes a drawing).
#[test]
fn the_blank_tile_is_drawn_in_a_browser_window() {
    let page = Page::new(Browser::Chromium);
    let mut w = window_in(&page);
    open_home(&mut w);
    assert!(w.state().element("backstage.tile.blank").is_some_and(|tile| tile.enabled));
}
