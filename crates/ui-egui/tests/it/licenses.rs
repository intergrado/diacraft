//! The Third-party licences window: `app.licenses` shows `THIRD-PARTY-LICENSES.txt` (built into the window) in a list that scrolls, and the
//! About panel's Third-party licences link runs that command. Driven frame by frame with kittest, so nothing here needs a display.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::licenses::{self, TEXT};
use diacraft_ui_egui::splash::{LICENSES_HINT, SplashKind};
use egui::{Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{at, key, shape_ids};
use crate::support::{Rest, Window, click, run_command, window};

fn app() -> DiacraftApp {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("shape.drop", &json!({"master": "Rectangle", "page": 1, "x": 1.0, "y": 8.0})).unwrap();
    DiacraftApp::new(session, None)
}

fn licenses_window(size: [f32; 2]) -> Window {
    window(app(), size)
}

fn open(w: &Window) -> bool {
    w.state().licenses_open()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

/// The notices are the file of the repository, built into the window: the window needs no file beside the program.
#[test]
fn the_text_in_the_window_is_the_file_of_the_repository() {
    let file = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../THIRD-PARTY-LICENSES.txt")).unwrap();
    assert_eq!(TEXT, file);
    assert!(TEXT.contains("libmimalloc-sys") && TEXT.contains("diacraft-cli"));
}

/// The words over the list name the file it is: the desktop's comes in the download, the web build's is the one the website links (the web build is no
/// download, and its window must not send a person to a file that is not there).
#[test]
fn the_words_over_the_list_name_the_file_of_the_build() {
    assert_eq!(licenses::INTRO.contains("THIRD-PARTY-LICENSES-WEB.txt"), cfg!(target_arch = "wasm32"));
    assert_eq!(licenses::INTRO.contains("comes in the download"), !cfg!(target_arch = "wasm32"));
    assert!(licenses::INTRO.contains("THIRD-PARTY-LICENSES"), "{}", licenses::INTRO);
    // The text it shows begins with the title of the file it names.
    assert!(TEXT.starts_with("DiaCraft: third-party licences"));
}

/// The web build shows its own list, the file of the repository that holds the crates of `diacraft-web`: not the desktop programs' (it has no
/// C library and no window system), and the same file the website links beside the app. **It does not carry the file**: the page fetches it, and the
/// tests of that are in `assets.rs`; this holds the file itself.
#[test]
fn the_web_builds_list_is_the_web_file_of_the_repository_and_not_the_desktops() {
    let web = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../THIRD-PARTY-LICENSES-WEB.txt")).unwrap();
    assert!(web.starts_with("DiaCraft: third-party licences (the web build)\n"), "{}", &web[..60]);
    assert!(web.contains("diacraft-web") && web.contains("wasm-bindgen"), "the web build's crates are named");
    assert!(!web.contains("libmimalloc-sys 0") && !web.contains("diacraft-cli"), "no program of the downloads, no C allocator");
    assert_ne!(web, TEXT);
}

/// `app.licenses` opens the window with its title, the list and Close; the window state says so, and the window is a modal.
#[test]
fn the_command_opens_the_window_with_the_list_and_a_close_button() {
    let mut w = licenses_window([1280.0, 800.0]);
    assert!(!open(&w));
    assert!(w.state().window_state()["licenses"].is_null());
    assert!(w.state().element("licenses.list").is_none());
    run_command(&mut w, "app.licenses", json!({}));
    assert!(open(&w));
    let state = &w.state().window_state()["licenses"];
    assert_eq!(state["rows"], licenses::rows().len());
    assert_eq!(state["offset"], 0.0);
    for name in ["licenses.title", "licenses.list", "licenses.close"] {
        assert!(w.state().element(name).is_some(), "{name} is in ui.elements");
    }
    w.get_by_label("Third-party licences");
    w.get_by_label("Close");
    // The first rows of the file are in view, and not the end of it.
    w.get_by_label("DiaCraft: third-party licences");
    assert!(w.query_by_label("End of the third-party licences.").is_none(), "only the rows in view are laid out");
    assert!(w.state().is_modal_open(), "nothing under it takes the keys or the pointer");
    assert_eq!(w.state().ui_panics(), 0);
}

/// One that is open stays as it is, and a window with no drawing can open it too (About is in the File view of a window with none).
#[test]
fn it_needs_no_drawing_and_a_second_open_changes_nothing() {
    let mut w = window(DiacraftApp::new(Session::new(), None), [1280.0, 800.0]);
    run_command(&mut w, "app.licenses", json!({}));
    assert!(open(&w));
    w.state_mut().execute("app.licenses", &json!({})).unwrap();
    assert!(open(&w));
    assert_eq!(w.state().command_enabled("app.licenses"), Ok(()));
}

/// Close, Escape and `close: true` close it; closing what is not open is no error.
#[test]
fn close_escape_and_the_command_close_it() {
    let mut w = licenses_window([1280.0, 800.0]);
    run_command(&mut w, "app.licenses", json!({}));
    let at = rect_of(&w, "licenses.close").center();
    click(&mut w, at);
    w.rest();
    assert!(!open(&w), "Close");
    assert!(!w.state().is_modal_open());
    run_command(&mut w, "app.licenses", json!({}));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!open(&w), "Escape");
    run_command(&mut w, "app.licenses", json!({}));
    run_command(&mut w, "app.licenses", json!({"close": true}));
    assert!(!open(&w), "close: true");
    run_command(&mut w, "app.licenses", json!({"close": true}));
    assert!(!open(&w), "closing what is not open is not an error");
}

/// Escape that closes the window is used up: with a shape selected, the selection stays.
#[test]
fn escape_that_closes_it_does_not_reach_the_drawing() {
    let mut w = licenses_window([1280.0, 800.0]);
    let shape = shape_ids(&w)[0];
    run_command(&mut w, "edit.select", json!({"ids": [shape]}));
    run_command(&mut w, "app.licenses", json!({}));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!open(&w));
    assert_eq!(w.state().selection(), [shape], "the window took the key");
}

/// A click outside the card does not dismiss it (it is read like a licence, not like a splash) and does not reach the drawing.
#[test]
fn a_click_outside_the_card_neither_closes_it_nor_reaches_the_drawing() {
    let mut w = licenses_window([1600.0, 1000.0]);
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    run_command(&mut w, "app.licenses", json!({}));
    let list = rect_of(&w, "licenses.list");
    let over_shape = at(&w, 1.0, 8.0);
    assert!(!list.contains(over_shape), "the rectangle is not under the card: {over_shape:?} {list:?}");
    click(&mut w, over_shape);
    w.rest();
    assert!(w.state().selection().is_empty(), "the click did not select the rectangle under the shade");
    assert!(open(&w));
}

/// The keys scroll the list: End to the last rows, Home to the first, a page at a time between, and the offset in the window state follows.
#[test]
fn the_keys_scroll_the_list() {
    let mut w = licenses_window([1280.0, 800.0]);
    run_command(&mut w, "app.licenses", json!({}));
    let offset = |w: &Window| w.state().window_state()["licenses"]["offset"].as_f64().unwrap();
    assert_eq!(offset(&w), 0.0);
    key(&mut w, Key::PageDown, Modifiers::NONE);
    w.rest();
    let page = offset(&w);
    assert!(page > 100.0, "a page down moved {page}");
    key(&mut w, Key::PageDown, Modifiers::NONE);
    w.rest();
    assert!(offset(&w) > page * 1.5, "and another: {}", offset(&w));
    key(&mut w, Key::PageUp, Modifiers::NONE);
    w.rest();
    assert!((offset(&w) - page).abs() < 2.0, "a page up goes back: {}", offset(&w));
    key(&mut w, Key::ArrowDown, Modifiers::NONE);
    w.rest();
    assert!(offset(&w) > page, "a line down");
    key(&mut w, Key::End, Modifiers::NONE);
    w.rest();
    w.get_by_label("End of the third-party licences.");
    assert!(w.query_by_label("DiaCraft: third-party licences").is_none(), "the first row is out of view at the end");
    key(&mut w, Key::Home, Modifiers::NONE);
    w.rest();
    assert_eq!(offset(&w), 0.0);
    w.get_by_label("DiaCraft: third-party licences");
}

/// It fits every window size: the card stays inside the window, with the list and the Close button on it.
#[test]
fn the_window_fits_every_window_size() {
    for size in [[1280.0, 800.0], [900.0, 600.0], [640.0, 400.0], [480.0, 320.0], [360.0, 300.0]] {
        let mut w = licenses_window(size);
        run_command(&mut w, "app.licenses", json!({}));
        w.rest();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
        for name in ["licenses.title", "licenses.list", "licenses.close"] {
            let r = rect_of(&w, name);
            assert!(screen.contains_rect(r), "{size:?}: {name} {r:?} is not inside the window");
        }
        assert_eq!(w.state().ui_panics(), 0, "{size:?}");
    }
}

/// The window is a command like the others: listed with its label, taking `close` and nothing else.
#[test]
fn it_is_a_window_command_that_takes_close_and_nothing_else() {
    let w = licenses_window([900.0, 600.0]);
    let command = diacraft_ui_egui::commands::find_ui_command("app.licenses").expect("the window has an app.licenses");
    assert_eq!(command.label, "Third-Party Licences");
    assert!(command.params.contains("close: true") && command.params.contains("THIRD-PARTY-LICENSES.txt"), "{}", command.params);
    assert!(diacraft_ui_egui::commands::known("app.licenses"));
    assert_eq!(w.state().command_enabled("app.licenses"), Ok(()));
    let mut w = w;
    let refused = w.state_mut().execute("app.licenses", &json!({"wrong": 1})).unwrap_err().to_string();
    assert!(refused.contains("unknown parameter `wrong`") && refused.contains("close"), "{refused}");
    assert!(!open(&w), "a refused call opens nothing");
}

// ------------------------------------------------------------------------------------------------------------------------- the About panel

fn about_window(size: [f32; 2]) -> Window {
    let mut w = licenses_window(size);
    run_command(&mut w, "app.about", json!({}));
    w
}

/// The About panel has the link, under the licence line and inside the picture's text zone, at every size.
#[test]
fn the_about_panel_has_a_third_party_licences_link_inside_the_text_zone() {
    for (size, ppp) in [([1280.0, 800.0], 1.0), ([900.0, 600.0], 1.0), ([480.0, 320.0], 1.0), ([1280.0, 800.0], 2.0), ([800.0, 300.0], 1.0)] {
        let mut w = egui_kittest::Harness::builder().with_size(size).with_pixels_per_point(ppp).build_eframe(|_| app());
        run_command(&mut w, "app.about", json!({}));
        w.rest();
        assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
        w.get_by_label("Third-party licences");
        let (panel, zone) = (rect_of(&w, "splash.panel"), rect_of(&w, "splash.zone"));
        let link = rect_of(&w, "splash.link.licenses");
        assert!(zone.expand(0.6).contains_rect(link), "{size:?} at {ppp}: the link {link:?} is not inside the zone {zone:?}");
        assert!(panel.contains_rect(link));
        let licence = rect_of(&w, "splash.licence");
        assert!(licence.max.y <= link.min.y, "{size:?}: the link is under the licence line: {licence:?} {link:?}");
        // Nothing of the older lines moved out of the zone to make room.
        for name in ["splash.version", "splash.git", "splash.copyright", "splash.licence", "splash.link.mit", "splash.link.apache"] {
            let r = rect_of(&w, name);
            assert!(zone.expand(0.6).contains_rect(r), "{size:?} at {ppp}: {name} {r:?} is not inside the zone {zone:?}");
        }
    }
}

/// A click on the link hands the screen to the licences window: the panel is gone, the window is open, and no page is opened in a browser.
#[test]
fn a_click_on_the_link_closes_the_panel_and_opens_the_window() {
    let mut w = about_window([1280.0, 800.0]);
    let link = rect_of(&w, "splash.link.licenses").center();
    click(&mut w, link);
    w.rest();
    assert!(open(&w), "the window is open");
    assert_eq!(w.state().splash_kind(), None, "the panel is gone");
    assert!(w.state().element("splash.panel").is_none());
    assert!(
        w.output().platform_output.commands.iter().all(|c| !matches!(c, egui::OutputCommand::OpenUrl(_))),
        "the link opens no page in a browser: it runs a command"
    );
    // And Escape from there ends it, with the panel still gone.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!open(&w) && w.state().splash_kind().is_none());
}

/// The same hand-over through the command: with the panel up, `app.licenses` replaces it.
#[test]
fn the_command_run_with_the_panel_up_replaces_the_panel() {
    let mut w = about_window([1280.0, 800.0]);
    run_command(&mut w, "app.licenses", json!({}));
    assert!(open(&w));
    assert_eq!(w.state().splash_kind(), None);
}

/// What the link's tooltip says is the file, and the link is a link for a screen reader (a role of its own, named by its words).
#[test]
fn the_link_says_what_it_shows() {
    assert!(LICENSES_HINT.contains(if cfg!(target_arch = "wasm32") { "THIRD-PARTY-LICENSES-WEB.txt" } else { "THIRD-PARTY-LICENSES.txt" }));
    let w = about_window([1280.0, 800.0]);
    w.get_by_role_and_label(egui::accesskit::Role::Link, "Third-party licences");
}
