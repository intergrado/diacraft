//! The window's assets: the splash picture, the cover and the licence list, which the web build fetches instead of carrying (`asset_host.rs`). The tests give the window a host of their own, which
//! answers when the test says, and look at what the window shows while an asset is on its way, once it has come, and when it could not be had. Driven frame by frame with kittest, so nothing here needs a
//! display or a network. A window with no host (the desktop) takes the assets from the program; the other tests of the crate hold that.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_engine::Session;
use diacraft_ui_egui::splash::{SHOWN_FOR, SplashKind};
use diacraft_ui_egui::{Asset, AssetHost, Bytes, DiacraftApp, Fetch};
use egui::{Context, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::key;
use crate::support::{Rest, Window, run_command, window};

/// A host that answers what the test put in it, and records what the window asked for and what it let go of.
#[derive(Default)]
struct Fake {
    answers: Mutex<HashMap<Asset, Fetch>>,
    wanted: Mutex<Vec<Asset>>,
    taken: Mutex<Vec<Asset>>,
}

impl Fake {
    fn answer(&self, asset: Asset, fetch: Fetch) {
        self.answers.lock().unwrap().insert(asset, fetch);
    }

    fn wanted(&self, asset: Asset) -> usize {
        self.wanted.lock().unwrap().iter().filter(|a| **a == asset).count()
    }

    fn taken(&self, asset: Asset) -> usize {
        self.taken.lock().unwrap().iter().filter(|a| **a == asset).count()
    }
}

impl AssetHost for Fake {
    fn want(&self, _ctx: &Context, asset: Asset) {
        self.wanted.lock().unwrap().push(asset);
    }

    fn state(&self, asset: Asset) -> Fetch {
        self.answers.lock().unwrap().get(&asset).cloned().unwrap_or(Fetch::Waiting)
    }

    fn release(&self, asset: Asset) {
        self.taken.lock().unwrap().push(asset);
    }
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn file(name: &str) -> Fetch {
    Fetch::Ready(Bytes::Shared(Arc::from(std::fs::read(root().join(name)).expect("the file is in the repository"))))
}

fn splash_png() -> Fetch {
    file("assets/branding/diacraft-splash.png")
}

fn cover_png() -> Fetch {
    file("assets/branding/diacraft-cover.png")
}

fn licences() -> Fetch {
    file("THIRD-PARTY-LICENSES-WEB.txt")
}

/// A window with `host` and the start splash, on a clock that stands still until the test moves it.
fn web_window(host: &Arc<Fake>, size: [f32; 2]) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = DiacraftApp::new(session, None).with_asset_host(host.clone()).with_start_splash();
    window(app, size)
}

fn no_splash_window(host: &Arc<Fake>) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    window(DiacraftApp::new(session, None).with_asset_host(host.clone()), [1280.0, 800.0])
}

// ----------------------------------------------------------------------------------------------------------------------------------- the splash

/// The splash that waits for its picture is the panel at its size, in the middle, with the paper of the picture and the words: it is a modal (it is on the screen), its text is there, and its
/// four seconds have not begun.
#[test]
fn a_splash_whose_picture_is_on_its_way_is_a_panel_with_a_short_wait_and_its_time_has_not_begun() {
    let host = Arc::new(Fake::default());
    let mut w = web_window(&host, [1280.0, 800.0]);
    w.rest();
    assert_eq!(w.state().splash_kind(), Some(SplashKind::Start));
    assert!(w.state().element("splash.waiting").is_some(), "the panel says it is waiting");
    let panel = w.state().element("splash.panel").unwrap().rect;
    assert!(panel.width() > 500.0, "the panel has its size already: {panel:?}");
    w.get_by_label(&format!("DiaCraft {}", env!("CARGO_PKG_VERSION")));
    w.get_by_label("Third-party licences");
    assert!(host.wanted(Asset::Splash) >= 1, "the window asked for the picture");
    assert_eq!(host.wanted(Asset::Cover), 0, "and for nothing it does not need yet");
    assert_eq!(host.wanted(Asset::Licenses), 0);
    assert!(w.state().is_modal_open());
    // A clock that runs past the four seconds does not take the panel: the picture is not there to be seen.
    w.advance(SHOWN_FOR * 3);
    w.step();
    assert_eq!(w.state().splash_kind(), Some(SplashKind::Start), "the time does not run while the panel waits");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_picture_replaces_the_wait_where_it_was_and_the_four_seconds_begin_then() {
    let host = Arc::new(Fake::default());
    let mut w = web_window(&host, [1280.0, 800.0]);
    w.rest();
    let waiting = w.state().element("splash.panel").unwrap().rect;
    host.answer(Asset::Splash, splash_png());
    w.step();
    w.rest();
    assert!(w.state().element("splash.waiting").is_none(), "the wait is gone");
    let panel = w.state().element("splash.panel").unwrap().rect;
    assert_eq!(panel, waiting, "the panel does not move when the picture comes");
    assert_eq!(host.taken(Asset::Splash), 1, "the host is told to let the bytes go once the texture has the pixels");
    w.advance(SHOWN_FOR - Duration::from_secs(1));
    w.step();
    assert_eq!(w.state().splash_kind(), Some(SplashKind::Start), "its time began when the picture came");
    w.advance(Duration::from_secs(2));
    w.step();
    w.step();
    assert_eq!(w.state().splash_kind(), None, "and ends four seconds after");
    // The host is not asked again once the picture is made.
    let asked = host.wanted(Asset::Splash);
    w.advance(Duration::from_secs(1));
    w.step();
    assert_eq!(host.wanted(Asset::Splash), asked);
}

#[test]
fn a_click_or_escape_dismisses_a_splash_that_is_still_waiting() {
    let host = Arc::new(Fake::default());
    let mut w = web_window(&host, [1280.0, 800.0]);
    w.rest();
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().splash_kind(), None, "Escape");
    assert!(!w.state().is_modal_open(), "nothing is left blocking the window");
}

#[test]
fn a_picture_that_could_not_be_had_is_no_splash_and_says_why_in_the_status_bar() {
    let host = Arc::new(Fake::default());
    let mut w = web_window(&host, [1280.0, 800.0]);
    w.rest();
    host.answer(Asset::Splash, Fetch::Failed("the server answered 404 for the splash picture".to_owned()));
    w.step();
    w.rest();
    assert_eq!(w.state().splash_kind(), None);
    assert!(!w.state().is_modal_open(), "the window works without it");
    let notice = w.state().notice().expect("the person is told");
    assert!(notice.contains("artwork could not load") && notice.contains("404"), "{notice}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A picture that is not the picture (an error page that came with a 200) is not decoded into a texture: the same words as a picture that cannot be had.
#[test]
fn a_file_that_is_not_the_picture_is_told_in_words_and_not_decoded() {
    let host = Arc::new(Fake::default());
    host.answer(Asset::Splash, Fetch::Ready(Bytes::Shared(Arc::from(&b"<html>Not found</html>"[..]))));
    let mut w = web_window(&host, [1280.0, 800.0]);
    w.rest();
    assert_eq!(w.state().splash_kind(), None);
    let notice = w.state().notice().expect("the person is told");
    assert!(notice.contains("artwork could not load"), "{notice}");
}

/// About shows the same panel with no timer, and the picture is made once: the second time it is not asked for.
#[test]
fn about_over_a_picture_that_is_waiting_waits_too_and_is_not_timed() {
    let host = Arc::new(Fake::default());
    let mut w = no_splash_window(&host);
    run_command(&mut w, "app.about", json!({}));
    assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
    assert!(w.state().element("splash.waiting").is_some());
    w.advance(Duration::from_secs(60));
    w.step();
    assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
    host.answer(Asset::Splash, splash_png());
    w.step();
    w.rest();
    assert!(w.state().element("splash.waiting").is_none());
    assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
}

// ------------------------------------------------------------------------------------------------------------------------------------ the cover

/// The File view's Home page shows the cover: a placeholder with a short wait while the cover comes, then the picture. It is asked for when the File view first shows it, and not before.
#[test]
fn the_cover_is_asked_for_when_the_file_view_shows_it_and_waits_in_a_placeholder() {
    let host = Arc::new(Fake::default());
    let mut w = no_splash_window(&host);
    w.rest();
    assert_eq!(host.wanted(Asset::Cover), 0, "the window does not ask for what it does not show");
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    w.rest();
    let cover = w.state().element("backstage.cover").expect("the place of the cover is drawn").rect;
    assert!((cover.width() / cover.height() - 1672.0 / 941.0).abs() < 0.001, "the place keeps the cover's shape");
    assert!(host.wanted(Asset::Cover) >= 1);
    assert_eq!(host.taken(Asset::Cover), 0);
    host.answer(Asset::Cover, cover_png());
    w.step();
    w.rest();
    assert_eq!(host.taken(Asset::Cover), 1, "the cover was made into a texture and the host let go of the bytes");
    assert!(w.state().notice().is_none_or(|n| !n.contains("artwork")), "no complaint");
    assert_eq!(w.state().element("backstage.cover").unwrap().rect, cover);
}

#[test]
fn a_cover_that_could_not_be_had_says_why_once_and_the_file_view_works_without_it() {
    let host = Arc::new(Fake::default());
    host.answer(Asset::Cover, Fetch::Failed("the cover picture could not be fetched (Failed to fetch)".to_owned()));
    let mut w = no_splash_window(&host);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    w.rest();
    let notice = w.state().notice().expect("told");
    assert!(notice.contains("artwork could not load") && notice.contains("Failed to fetch"), "{notice}");
    assert!(w.state().element("backstage.tile.blank").is_some(), "the rest of the page is there");
    // Once, and not asked for again: later frames neither say it again nor ask the host for what has failed.
    let asked = host.wanted(Asset::Cover);
    for _ in 0..3 {
        w.step();
    }
    assert_eq!(host.wanted(Asset::Cover), asked);
    assert_eq!(w.state().ui_panics(), 0);
}

// ------------------------------------------------------------------------------------------------------------------------- the licence list

fn open_licences(w: &mut Window) {
    run_command(w, "app.licenses", json!({}));
}

#[test]
fn the_licence_list_is_asked_for_when_its_window_opens_and_the_window_waits_in_its_place() {
    let host = Arc::new(Fake::default());
    let mut w = no_splash_window(&host);
    w.rest();
    assert_eq!(host.wanted(Asset::Licenses), 0);
    open_licences(&mut w);
    w.rest();
    assert!(host.wanted(Asset::Licenses) >= 1);
    let state = w.state().window_state()["licenses"].clone();
    assert_eq!((state["text"].as_str(), state["rows"].as_u64()), (Some("waiting"), Some(0)), "{state}");
    assert!(w.state().element("licenses.notice").is_some(), "the words are where the list will be");
    assert!(w.state().element("licenses.list").is_none());
    w.get_by_label("Third-party licences");
    w.get_by_label("Close");
    // The card keeps its size when the list comes: its height does not depend on its list.
    let before = w.state().element("licenses.notice").unwrap().rect;
    host.answer(Asset::Licenses, licences());
    w.step();
    w.rest();
    let state = w.state().window_state()["licenses"].clone();
    assert_eq!(state["text"], "ready");
    assert!(state["rows"].as_u64().unwrap() > 1000, "{state}");
    assert_eq!(host.taken(Asset::Licenses), 1);
    assert!(w.state().element("licenses.notice").is_none());
    let list = w.state().element("licenses.list").expect("the list is drawn").rect;
    // Where the wait was, and as high; as wide less the bar the list has at its side.
    assert_eq!(list.height(), before.height());
    assert!(before.width() - list.width() >= 0.0 && before.width() - list.width() <= 16.0, "{list:?} in {before:?}");
    assert_eq!(list.min, before.min);
    w.get_by_label("DiaCraft: third-party licences (the web build)");
}

#[test]
fn a_licence_list_that_could_not_be_had_says_why_in_its_place_and_close_works() {
    let host = Arc::new(Fake::default());
    host.answer(
        Asset::Licenses,
        Fetch::Failed("the list of third-party licences came shorter than the manifest says (10 bytes, not 225292)".to_owned()),
    );
    let mut w = no_splash_window(&host);
    open_licences(&mut w);
    w.rest();
    let state = w.state().window_state()["licenses"].clone();
    assert_eq!(state["text"], "failed");
    assert!(state["error"].as_str().unwrap().contains("shorter than the manifest says"), "{state}");
    assert!(w.state().element("licenses.notice").is_some());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().licenses_open(), "Escape closes it as it closes the list");
    // Opened again, it is not asked for again: the failure is kept.
    let asked = host.wanted(Asset::Licenses);
    open_licences(&mut w);
    w.rest();
    assert_eq!(w.state().window_state()["licenses"]["text"], "failed");
    assert_eq!(host.wanted(Asset::Licenses), asked, "a failed list is kept: the window asks of the host only until it has an answer");
}

#[test]
fn a_licence_list_that_is_not_text_is_a_failure_and_not_a_panic() {
    let host = Arc::new(Fake::default());
    host.answer(Asset::Licenses, Fetch::Ready(Bytes::Shared(Arc::from(&[0xff_u8, 0xfe, 0x00, 0x81][..]))));
    let mut w = no_splash_window(&host);
    open_licences(&mut w);
    w.rest();
    let state = w.state().window_state()["licenses"].clone();
    assert_eq!(state["text"], "failed");
    assert!(state["error"].as_str().unwrap().contains("not text"), "{state}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A list that the host has the second time (the window was closed and opened again) is the same rows, and the host's bytes are let go once.
#[test]
fn the_list_is_made_once_and_kept_for_the_next_time_the_window_opens() {
    let host = Arc::new(Fake::default());
    host.answer(Asset::Licenses, licences());
    let mut w = no_splash_window(&host);
    open_licences(&mut w);
    w.rest();
    let rows = w.state().window_state()["licenses"]["rows"].as_u64().unwrap();
    run_command(&mut w, "app.licenses", json!({"close": true}));
    open_licences(&mut w);
    w.rest();
    assert_eq!(w.state().window_state()["licenses"]["rows"].as_u64(), Some(rows));
    assert_eq!(host.taken(Asset::Licenses), 1);
}
