//! The splash popup and About DiaCraft: the octopus cover as the popup's whole background with the version, the git release, the year and the licence
//! painted over its lower left corner; gone after four seconds (the time of the window's clock, held while the pointer is over it), on a click outside the
//! links and on Escape, each of which the popup uses up; and shown with no timer by `app.about`.
//!
//! The window of the tests has no splash unless a test asks for it (`DiacraftApp::new` makes none: the desktop app and the web page ask for it), so the other tests keep
//! their clicks.

use std::time::Duration;

use diacraft_engine::Session;
use diacraft_engine::build_info::BuildInfo;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::clock::Clock;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::snap_reader::SnapReader;
use diacraft_ui_egui::splash::{APACHE_URL, ART_SIZE, LEGAL_ZONE, MIT_URL, SHOWN_FOR, SplashKind, TEXT_ZONE, TRADEMARK};
use diacraft_ui_egui::theme::Tokens;
use egui::{Key, Modifiers, OutputCommand, Pos2, Rect, ViewportId, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{at, key, shape_ids};
use crate::fixture;
use crate::support::{Rest, TempDir, Window, click, move_to, run_command, window};

fn build() -> BuildInfo {
    BuildInfo::new("v9.8.7-12-gabc1234", "2031")
}

/// A new drawing with one rectangle on it (so that a click on it selects it), and a window on it that has the splash a start gives, over a build of its own.
fn app() -> DiacraftApp {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("shape.drop", &json!({"master": "Rectangle", "page": 1, "x": 1.0, "y": 8.0})).unwrap();
    DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_snap_reader(SnapReader::inline()).with_build_info(build())
}

/// A window of `size` points at `ppp` pixels to the point, on a manual clock that stands still until the test advances it.
fn splash_window_at(size: [f32; 2], ppp: f32, app: DiacraftApp) -> Window {
    let mut w = Harness::builder().with_size(size).with_pixels_per_point(ppp).build_eframe(|_| app.with_clock(Clock::manual()));
    w.rest();
    w
}

fn splash_window(size: [f32; 2]) -> Window {
    splash_window_at(size, 1.0, app().with_start_splash())
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn open(w: &Window) -> bool {
    w.state().splash_kind().is_some()
}

/// The `OpenUrl` commands the last frame sent.
fn opened(w: &Window) -> Vec<(String, bool)> {
    w.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            OutputCommand::OpenUrl(o) => Some((o.url.clone(), o.new_tab)),
            _ => None,
        })
        .collect()
}

/// A window with no splash made by the start has none: the other tests of the crate click as they always have.
#[test]
fn a_window_has_no_splash_unless_it_is_asked_for_one() {
    let w = window(DiacraftApp::new(Session::new(), None), [900.0, 600.0]);
    assert_eq!(w.state().splash_kind(), None);
    assert!(w.state().element("splash.panel").is_none());
    assert!(w.state().window_state()["splash"].is_null());
    assert!(!w.state().is_modal_open());
}

/// The panel at the start shows the version, the git release, the year and the licence with both links.
#[test]
fn the_start_splash_shows_the_version_the_git_release_the_year_and_both_licence_links() {
    let w = splash_window([1280.0, 800.0]);
    assert_eq!(w.state().splash_kind(), Some(SplashKind::Start));
    assert_eq!(w.state().window_state()["splash"], "start");
    w.get_by_label(&format!("DiaCraft {}", env!("CARGO_PKG_VERSION")));
    w.get_by_label_contains("v9.8.7-12-gabc1234");
    w.get_by_label("© 2031 the DiaCraft contributors");
    w.get_by_label_contains("Licensed under the");
    w.get_by_label("MIT License");
    w.get_by_label("Apache License 2.0");
    // The owner, 2026-11-23: no "at your option" row; the sentence ends with a full stop after the second licence.
    assert!(w.query_by_label_contains("at your option").is_none());
    w.get_by_label(".");
    for name in [
        "splash.panel",
        "splash.zone",
        "splash.version",
        "splash.git",
        "splash.copyright",
        "splash.licence",
        "splash.link.mit",
        "splash.link.apache",
        "splash.link.licenses",
    ] {
        assert!(w.state().element(name).is_some(), "{name} is in ui.elements");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// A build with nothing to say shows `unknown`.
#[test]
fn a_build_with_no_git_release_shows_unknown_and_the_last_resort_year() {
    let w = splash_window_at([1280.0, 800.0], 1.0, app().with_build_info(BuildInfo::default()).with_start_splash());
    w.get_by_label_contains("unknown");
    w.get_by_label("© 2026 the DiaCraft contributors");
}

/// The image is the popup, edge to edge: the panel is the picture's own size divided by the pixels to the point, in the middle of the window, with no strip or margin
/// round it; in a window too small for it, it is scaled to fit with a margin.
#[test]
fn the_picture_is_the_whole_popup_at_its_size_over_the_pixels_to_the_point() {
    for (size, ppp) in [([1280.0, 800.0], 1.0), ([1280.0, 800.0], 2.0), ([1600.0, 1000.0], 1.5)] {
        let w = splash_window_at(size, ppp, app().with_start_splash());
        let panel = rect_of(&w, "splash.panel");
        let expected = ART_SIZE / ppp;
        assert!((panel.size() - expected).abs().max_elem() < 0.6, "{size:?} at {ppp}: {:?} is not {expected:?}", panel.size());
        let middle = vec2(size[0], size[1]) / 2.0;
        assert!((panel.center() - middle.to_pos2()).abs().max_elem() < 1.5, "{size:?} at {ppp}: {panel:?} is not in the middle");
    }
    for size in [[480.0, 320.0], [640.0, 400.0], [800.0, 300.0]] {
        let w = splash_window_at(size, 1.0, app().with_start_splash());
        let panel = rect_of(&w, "splash.panel");
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
        assert!(screen.shrink(8.0).contains_rect(panel), "{size:?}: {panel:?} does not fit with a margin");
        assert!(((panel.width() / panel.height()) - (ART_SIZE.x / ART_SIZE.y)).abs() < 0.01, "{size:?}: the picture keeps its shape");
    }
}

/// The text lies in the lower left corner of the picture, under the flowchart and clear of the diamond and the tentacles, whatever the size of the window and the
/// pixels to the point: scaled with the picture, it never reaches the art.
#[test]
fn the_text_stays_in_the_zone_of_the_picture_that_is_clear_at_every_size() {
    for (size, ppp) in
        [([1280.0, 800.0], 1.0), ([900.0, 600.0], 1.0), ([480.0, 320.0], 1.0), ([1280.0, 800.0], 2.0), ([640.0, 400.0], 1.5), ([800.0, 300.0], 1.0)]
    {
        let w = splash_window_at(size, ppp, app().with_start_splash());
        let (panel, zone) = (rect_of(&w, "splash.panel"), rect_of(&w, "splash.zone"));
        let k = panel.width() / ART_SIZE.x;
        let expected = Rect::from_min_max(panel.min + vec2(TEXT_ZONE[0], TEXT_ZONE[1]) * k, panel.min + vec2(TEXT_ZONE[2], TEXT_ZONE[3]) * k);
        assert!(
            (zone.min - expected.min).abs().max_elem() < 0.6 && (zone.max - expected.max).abs().max_elem() < 0.6,
            "{size:?}: {zone:?} is not {expected:?}"
        );
        for name in
            ["splash.version", "splash.git", "splash.copyright", "splash.licence", "splash.link.mit", "splash.link.apache", "splash.link.licenses"]
        {
            let r = rect_of(&w, name);
            assert!(zone.expand(0.6).contains_rect(r), "{size:?} at {ppp}: {name} {r:?} is not inside the zone {zone:?}");
        }
        // In the order of the brief, top to bottom: the version, the git release, the © line, the licence, and the link to the third-party licences.
        let tops: Vec<f32> = ["splash.version", "splash.git", "splash.copyright", "splash.licence", "splash.link.licenses"]
            .iter()
            .map(|n| rect_of(&w, n).min.y)
            .collect();
        assert!(tops.windows(2).all(|p| p[0] < p[1]), "{size:?}: {tops:?}");
        // Left-aligned with the picture's own title.
        for name in ["splash.version", "splash.git", "splash.copyright"] {
            assert!((rect_of(&w, name).min.x - zone.min.x).abs() < 0.6, "{name} starts at the left of the zone");
        }
    }
}

/// The owner, 2026-11-23: the last row (the link to the third-party licences) is not at the picture's bottom edge. Below it there is clear space as wide as the margin at
/// the text's left (the gap between the picture's left edge and the text), at every size of the window and the pixels to the point, in the start splash and in About; and
/// the rows are laid out from that margin up, so the space under the last row is that margin.
#[test]
fn the_last_row_ends_a_left_margin_above_the_pictures_bottom_edge() {
    for (size, ppp) in
        [([1280.0, 800.0], 1.0), ([900.0, 600.0], 1.0), ([480.0, 320.0], 1.0), ([1280.0, 800.0], 2.0), ([640.0, 400.0], 1.5), ([800.0, 300.0], 1.0)]
    {
        for about in [false, true] {
            let mut w = splash_window_at(size, ppp, if about { app() } else { app().with_start_splash() });
            if about {
                run_command(&mut w, "app.about", json!({}));
            }
            let panel = rect_of(&w, "splash.panel");
            let k = panel.width() / ART_SIZE.x;
            let left = rect_of(&w, "splash.version").min.x - panel.min.x;
            assert!((left - TEXT_ZONE[0] * k).abs() < 0.6, "{size:?} at {ppp}: the left margin is {left}");
            let last = rect_of(&w, "splash.link.licenses");
            let below = panel.max.y - last.max.y;
            assert!(below >= left - 0.6, "{size:?} at {ppp}, about {about}: {below} under the last row, less than the left margin {left}");
            assert!(below <= left + 1.0, "{size:?} at {ppp}, about {about}: laid out from the margin up, {below} under the last row");
            let top = rect_of(&w, "splash.version").min.y;
            assert!(top >= rect_of(&w, "splash.zone").min.y - 0.6, "{size:?} at {ppp}: the first row is under the flowchart");
        }
    }
}

/// The six sizes of window and pixels to the point the layout tests of the About panel run at.
const SIX_SIZES: [([f32; 2], f32); 6] =
    [([1280.0, 800.0], 1.0), ([900.0, 600.0], 1.0), ([480.0, 320.0], 1.0), ([1280.0, 800.0], 2.0), ([640.0, 400.0], 1.5), ([800.0, 300.0], 1.0)];

/// The notice (owner, 2026-11-23): the first sentence is the one the notice has always had; the others say that OpenDocument is an OASIS Standard, whose
/// marks LibreOffice and OpenOffice are, and that DiaCraft is not affiliated with or endorsed by any of the four owners.
#[test]
fn the_notice_keeps_its_first_sentence_and_names_the_owners_of_the_other_marks() {
    assert!(TRADEMARK.starts_with("Visio is a trademark of the Microsoft group of companies. "), "{TRADEMARK}");
    assert_eq!(
        TRADEMARK,
        "Visio is a trademark of the Microsoft group of companies. OpenDocument Format (ODF) is an OASIS Standard. LibreOffice is a registered trademark of The \
         Document Foundation. OpenOffice and Apache OpenOffice are registered trademarks of The Apache Software Foundation. DiaCraft is not affiliated with or \
         endorsed by Microsoft, OASIS, The Document Foundation or The Apache Software Foundation."
    );
}

/// About DiaCraft says the notice (owner, 2026-11-23: in NOTICE, the About box and the site), in the clear corner of the picture over its title,
/// whole and inside that zone at every size of the window and the pixels to the point; the start splash, which is the same panel on a timer, does not.
#[test]
fn about_shows_the_trademark_line_in_the_clear_corner_and_the_start_splash_does_not() {
    for (size, ppp) in SIX_SIZES {
        let mut w = splash_window_at(size, ppp, app());
        run_command(&mut w, "app.about", json!({}));
        w.get_by_label(TRADEMARK);
        let (panel, line) = (rect_of(&w, "splash.panel"), rect_of(&w, "splash.trademark"));
        let k = panel.width() / ART_SIZE.x;
        let zone = Rect::from_min_max(panel.min + vec2(LEGAL_ZONE[0], LEGAL_ZONE[1]) * k, panel.min + vec2(LEGAL_ZONE[2], LEGAL_ZONE[3]) * k);
        assert!(zone.expand(0.6).contains_rect(line), "{size:?} at {ppp}: the trademark line {line:?} is not inside its zone {zone:?}");
        assert!((line.min.x - zone.min.x).abs() < 0.6, "{size:?} at {ppp}: left-aligned with the picture's own title");
        assert!(!line.intersects(rect_of(&w, "splash.zone")), "{size:?} at {ppp}: apart from the version and the licence");
        assert_eq!(w.state().ui_panics(), 0);
    }
    let w = splash_window([1280.0, 800.0]);
    assert_eq!(w.state().splash_kind(), Some(SplashKind::Start));
    assert!(w.state().element("splash.trademark").is_none(), "the start splash has no trademark line");
    assert!(w.query_by_label(TRADEMARK).is_none());
}

/// The licence line says what the licence file says, each licence name is a link, and a click opens the right page through egui's `OpenUrl`, in a new tab (the web
/// build opens a new tab and does not leave the page): the popup stays, because the link must not dismiss it before it opens.
#[test]
fn a_click_on_a_licence_link_opens_its_page_and_does_not_dismiss_the_popup() {
    for (name, url) in [("splash.link.mit", MIT_URL), ("splash.link.apache", APACHE_URL)] {
        let mut w = splash_window([1280.0, 800.0]);
        let at = rect_of(&w, name).center();
        click(&mut w, at);
        assert_eq!(opened(&w), [(url.to_owned(), true)], "{name}");
        assert!(open(&w), "{name}: the link did not dismiss the popup");
        w.rest();
        assert!(open(&w));
    }
    assert_eq!(MIT_URL, "https://opensource.org/license/mit");
    assert_eq!(APACHE_URL, "https://www.apache.org/licenses/LICENSE-2.0");
}

/// Four seconds of the window's clock, and no more: the timer is the window's (`clock.rs`), so a slow machine does not shorten it.
#[test]
fn the_splash_goes_after_four_seconds_of_the_windows_clock() {
    assert_eq!(SHOWN_FOR, Duration::from_secs(4));
    let mut w = splash_window([1280.0, 800.0]);
    assert!(open(&w));
    w.advance(Duration::from_millis(3900));
    w.step();
    assert!(open(&w), "3.9 s");
    w.advance(Duration::from_millis(200));
    w.step();
    assert!(!open(&w), "4.1 s");
    assert!(w.state().element("splash.panel").is_none(), "nothing of it is drawn");
    assert_eq!(w.state().window_state()["splash"], serde_json::Value::Null);
    assert!(!w.state().is_modal_open(), "the window takes the keys and the pointer again");
}

/// The time is paused while the pointer is over the panel (the links can be reached) and runs again when the pointer leaves.
#[test]
fn the_timer_is_held_while_the_pointer_is_over_the_panel_and_goes_on_when_it_leaves() {
    let mut w = splash_window([1280.0, 800.0]);
    let panel = rect_of(&w, "splash.panel");
    w.advance(Duration::from_secs(3));
    w.step();
    move_to(&mut w, panel.center());
    w.advance(Duration::from_secs(60));
    w.step();
    w.step();
    assert!(open(&w), "a minute over the panel: still there");
    // Away, in the corner of the window (the backdrop): one second was left.
    move_to(&mut w, pos2(5.0, 5.0));
    w.advance(Duration::from_millis(900));
    w.step();
    assert!(open(&w), "0.9 s of the one that was left");
    w.advance(Duration::from_millis(200));
    w.step();
    assert!(!open(&w), "and it goes when the second is up");
}

/// A click outside the panel dismisses it, and the click is used up: it does not select what it was over.
#[test]
fn a_click_outside_the_panel_dismisses_it_and_does_not_reach_the_drawing() {
    let mut w = splash_window([1600.0, 1000.0]);
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    let panel = rect_of(&w, "splash.panel");
    let shape = shape_ids(&w)[0];
    let over_shape = at(&w, 1.0, 8.0);
    assert!(!panel.contains(over_shape), "the rectangle is outside the panel: {over_shape:?} {panel:?}");
    assert!(w.state().selection().is_empty());
    click(&mut w, over_shape);
    w.rest();
    assert!(!open(&w), "the click dismissed it");
    assert!(w.state().selection().is_empty(), "and was used up: the rectangle under it was not selected");
    // Now the same click selects.
    click(&mut w, over_shape);
    w.rest();
    assert_eq!(w.state().selection(), [shape], "with the popup gone the click reaches the drawing");
}

/// A click on the panel, outside the links, dismisses it too (on the picture, on the text that is no link).
#[test]
fn a_click_on_the_panel_outside_the_links_dismisses_it() {
    let w0 = splash_window([1280.0, 800.0]);
    let panel = rect_of(&w0, "splash.panel");
    let targets = [
        ("the octopus", panel.min + vec2(panel.width() * 0.7, panel.height() * 0.5)),
        ("the title", panel.min + vec2(panel.width() * 0.2, panel.height() * 0.4)),
        ("the version line", rect_of(&w0, "splash.version").center()),
        ("the © line", rect_of(&w0, "splash.copyright").center()),
    ];
    for (what, at) in targets {
        let mut w = splash_window([1280.0, 800.0]);
        click(&mut w, at);
        w.rest();
        assert!(!open(&w), "a click on {what} dismisses it");
    }
}

/// Escape dismisses it, and the key is used up: with a shape selected, the Escape that closes the popup leaves the selection, and the next one clears it.
#[test]
fn escape_dismisses_it_and_is_used_up() {
    let mut w = splash_window([1280.0, 800.0]);
    let shape = shape_ids(&w)[0];
    run_command(&mut w, "edit.select", json!({"ids": [shape]}));
    assert_eq!(w.state().selection(), [shape]);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!open(&w), "Escape dismissed it");
    assert_eq!(w.state().selection(), [shape], "and did not reach the drawing");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty(), "the next Escape is the drawing's");
}

/// `app.about` shows the same panel with no timer: it stays however long the window waits, and goes on a click or on Escape.
#[test]
fn app_about_shows_the_panel_with_no_timer_until_a_click_or_escape() {
    let mut w = window(app(), [1280.0, 800.0]);
    assert!(!open(&w));
    run_command(&mut w, "app.about", json!({}));
    assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
    assert_eq!(w.state().window_state()["splash"], "about");
    w.get_by_label("MIT License");
    w.get_by_label("© 2031 the DiaCraft contributors");
    w.advance(Duration::from_secs(600));
    w.rest();
    assert!(open(&w), "ten minutes on, with no timer");
    // The repaint: with no timer the window asks for none (a timer would ask for one at its end).
    w.advance(Duration::from_secs(1));
    w.step();
    assert_eq!(w.output().viewport_output.get(&ViewportId::ROOT).map(|v| v.repaint_delay), Some(Duration::MAX));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!open(&w), "Escape");
    run_command(&mut w, "app.about", json!({}));
    assert!(open(&w));
    let panel = rect_of(&w, "splash.panel");
    click(&mut w, panel.center());
    w.rest();
    assert!(!open(&w), "a click on the panel");
    run_command(&mut w, "app.about", json!({}));
    click(&mut w, pos2(4.0, 4.0));
    w.rest();
    assert!(!open(&w), "a click outside it");
    // A program closes it without a key or a click, and closing what is not open is not an error.
    run_command(&mut w, "app.about", json!({}));
    run_command(&mut w, "app.about", json!({"close": true}));
    assert!(!open(&w));
    run_command(&mut w, "app.about", json!({"close": true}));
}

/// `app.about` is a command of the window's registry like the others: listed with its label, it takes `close` and nothing else.
#[test]
fn app_about_is_a_listed_command_that_takes_close_and_refuses_the_rest() {
    let mut w = window(app(), [900.0, 600.0]);
    let command = diacraft_ui_egui::commands::find_ui_command("app.about").expect("the window has an app.about");
    assert_eq!(command.label, "About DiaCraft");
    assert!(diacraft_ui_egui::commands::known("app.about"));
    assert_eq!(w.state().command_enabled("app.about"), Ok(()));
    let refused = w.state_mut().execute("app.about", &json!({"wrong": 1})).unwrap_err().to_string();
    assert!(refused.contains("wrong") && refused.contains("close"), "{refused}");
    assert!(!open(&w), "a refused call shows nothing");
}

/// A start `app.about` replaces the start splash by the About panel: no timer from then on.
#[test]
fn about_over_the_start_splash_takes_its_timer_away() {
    let mut w = splash_window([1280.0, 800.0]);
    run_command(&mut w, "app.about", json!({}));
    assert_eq!(w.state().splash_kind(), Some(SplashKind::About));
    w.advance(Duration::from_secs(30));
    w.rest();
    assert!(open(&w));
}

/// While it waits the splash asks for one frame, at the moment its time ends, and none before: `Harness::run` fails on a window that repaints for ever.
#[test]
fn the_splash_asks_for_a_frame_only_when_its_time_ends() {
    // A window with no drawing: a drawing's picture has timers of its own, and the question here is the splash's.
    let mut w = splash_window_at([1280.0, 800.0], 1.0, DiacraftApp::new(Session::new(), None).with_build_info(build()).with_start_splash());
    let steps = w.rest();
    assert!(steps <= 3, "the window settles at once: {steps} frames");
    let delay = |w: &Window| w.output().viewport_output.get(&ViewportId::ROOT).map(|v| v.repaint_delay).unwrap();
    let first = delay(&w);
    assert!(first > Duration::from_millis(3500) && first <= SHOWN_FOR, "the first request is for the end, not for the next frame: {first:?}");
    w.advance(Duration::from_millis(1500));
    w.step();
    let later = delay(&w);
    assert!(later > Duration::from_millis(2000) && later <= Duration::from_millis(2500), "2.5 s were left: {later:?}");
    // Held while the pointer is over it: nothing is asked for, there is no end to wait for.
    let panel = rect_of(&w, "splash.panel");
    move_to(&mut w, panel.center());
    w.step();
    w.step();
    assert_eq!(delay(&w), Duration::MAX, "a held timer asks for no frame");
}

/// The window, a file given on the command line, and what loads behind it are not held back: the drawing opens under the popup.
#[test]
fn a_file_given_at_the_start_opens_under_the_popup() {
    let dir = TempDir::new("splash");
    let path = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let mut w = splash_window_at(
        [1280.0, 800.0],
        1.0,
        DiacraftApp::new(Session::new(), None)
            .with_inline_pictures(bundled_drawer())
            .with_build_info(build())
            .with_start_splash()
            .opening_at_start(&path),
    );
    w.rest();
    assert!(w.state().session().document().is_some_and(|d| d.pages.len() >= 2), "the drawing was read while the popup was up");
    assert!(open(&w), "and the popup is over the opened drawing");
    w.advance(SHOWN_FOR + Duration::from_millis(100));
    w.step();
    assert!(!open(&w));
}

/// A click anywhere dismisses it and a key that is not Escape does not (the popup is a popup: it takes the keys while it is up, as a card does, and gives them back).
#[test]
fn the_keys_other_than_escape_neither_dismiss_it_nor_reach_the_drawing() {
    let mut w = splash_window([1280.0, 800.0]);
    let shape = shape_ids(&w)[0];
    run_command(&mut w, "edit.select", json!({"ids": [shape]}));
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert!(open(&w), "Delete does not dismiss it");
    assert_eq!(shape_ids(&w), [shape], "and does not delete the shape under it");
}

/// The splash is drawn with the theme's tokens in both themes: the edge and shadow follow the theme and the text is the art's own ink in both.
#[test]
fn the_splash_shows_in_the_light_and_the_dark_theme_with_the_same_ink() {
    for theme in ["light", "dark"] {
        let mut w = splash_window([1280.0, 800.0]);
        run_command(&mut w, "window.theme", json!({"theme": theme}));
        w.rest();
        assert!(open(&w), "{theme}");
        assert_eq!(w.state().ui_panics(), 0);
    }
    let (light, dark) = (Tokens::light(), Tokens::dark());
    assert_eq!((light.splash_ink, light.splash_link), (dark.splash_ink, dark.splash_link), "the art does not change with the theme");
}

// ---- the picture itself: what the text is painted over

/// The splash copy as the app embeds it, decoded to RGB: its width, its height and its pixels.
fn picture() -> (usize, usize, Vec<[u8; 3]>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/branding/diacraft-splash.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(std::fs::read(path).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    let step = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("the splash copy decodes to {other:?}"),
    };
    let pixels = bytes[..info.buffer_size()].chunks(step).map(|p| [p[0], p[1], p[2]]).collect();
    (info.width as usize, info.height as usize, pixels)
}

fn luminance(c: [u8; 3]) -> f64 {
    let channel = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The pixels of `picture` in a rectangle of the picture, in its pixels (left, top, right, bottom).
fn region(picture: &(usize, usize, Vec<[u8; 3]>), [l, t, r, b]: [f32; 4]) -> Vec<[u8; 3]> {
    let (w, h, pixels) = picture;
    let (l, t, r, b) = (l as usize, t as usize, (r as usize).min(*w), (b as usize).min(*h));
    (t..b).flat_map(|y| (l..r).map(move |x| (x, y))).map(|(x, y)| pixels[y * w + x]).collect()
}

/// The splash copy is a size the app can embed: 836 × 470, a quarter of the cover's pixels, and under 400 KB (the cover is 2.4 MB).
#[test]
fn the_splash_copy_is_836_by_470_and_under_400_kilobytes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/branding/diacraft-splash.png");
    let bytes = std::fs::metadata(&path).unwrap().len();
    assert!(bytes < 400 * 1024, "{bytes} bytes");
    let picture = picture();
    assert_eq!((picture.0 as f32, picture.1 as f32), (ART_SIZE.x, ART_SIZE.y));
}

/// The zone the text is painted in holds nothing but the cover's cream: it is under the flowchart, left of the lone diamond and clear of every tentacle, so the text
/// never lies over the art.
#[test]
fn the_text_zone_of_the_picture_is_clear_cream() {
    let picture = picture();
    let zone = region(&picture, TEXT_ZONE);
    assert!(zone.len() > 20_000, "the zone has room for the text: {} pixels", zone.len());
    let cream = |c: &[u8; 3]| c[0] >= 228 && c[1] >= 218 && c[2] >= 203;
    let marks: Vec<&[u8; 3]> = zone.iter().filter(|c| !cream(c)).collect();
    assert!(marks.is_empty(), "{} pixels of the zone are art, for example {:?}", marks.len(), marks.first());
    // The flowchart's three shapes end above the zone: the rows just over it do hold art.
    let above = region(&picture, [TEXT_ZONE[0], TEXT_ZONE[1] - 24.0, TEXT_ZONE[2], TEXT_ZONE[1] - 8.0]);
    assert!(above.iter().any(|c| !cream(c)), "the flowchart is just above the zone: the zone is below it, not in empty space far from it");
}

/// The zone of the trademark line holds nothing but the cover's cream: the corner over the title, left of the flowchart's line and box; the title is just under it, and the
/// ink reads on it at 4.5 to 1 at least.
#[test]
fn the_legal_zone_of_the_picture_is_clear_cream_over_the_title() {
    let picture = picture();
    const { assert!(LEGAL_ZONE[0] >= 0.0 && LEGAL_ZONE[1] >= 0.0 && LEGAL_ZONE[2] <= ART_SIZE.x && LEGAL_ZONE[3] <= ART_SIZE.y) };
    let zone = region(&picture, LEGAL_ZONE);
    assert!(zone.len() > 30_000, "the zone has room for two rows of the line: {} pixels", zone.len());
    let cream = |c: &[u8; 3]| c[0] >= 228 && c[1] >= 218 && c[2] >= 203;
    let marks: Vec<&[u8; 3]> = zone.iter().filter(|c| !cream(c)).collect();
    assert!(marks.is_empty(), "{} pixels of the zone are art, for example {:?}", marks.len(), marks.first());
    let under = region(&picture, [LEGAL_ZONE[0], LEGAL_ZONE[3] + 16.0, LEGAL_ZONE[2], LEGAL_ZONE[3] + 40.0]);
    assert!(under.iter().any(|c| !cream(c)), "the title is just under the zone");
    // The zone is above the version and the licence.
    const { assert!(LEGAL_ZONE[3] <= TEXT_ZONE[1]) };
    let ink = Tokens::light().splash_ink;
    let worst = zone.into_iter().map(|bg| contrast([ink.r(), ink.g(), ink.b()], bg)).fold(f64::INFINITY, f64::min);
    assert!(worst >= 4.5, "the ink reads at {worst:.2} to 1 on the cream");
}

/// The first row of the picture, from the corner down, that holds a stroke of the title (in the columns the corner has).
fn title_top(picture: &(usize, usize, Vec<[u8; 3]>)) -> f32 {
    let cream = |c: &[u8; 3]| c[0] >= 228 && c[1] >= 218 && c[2] >= 203;
    (LEGAL_ZONE[3] as usize..ART_SIZE.y as usize)
        .find(|&y| region(picture, [LEGAL_ZONE[0], y as f32, LEGAL_ZONE[2], y as f32 + 1.0]).iter().any(|c| !cream(c)))
        .map_or(ART_SIZE.y, |y| y as f32)
}

/// The notice's corner is kept (owner, 2026-11-23) in the window `w` of `size` at `ppp`: the rectangle the whole of the notice takes, taken back into the picture's pixels,
/// lies over cream and nothing else (no tentacle, no line, no box of the drawing on the right, not the title), ends above the title with room to spare, keeps off the rows of the
/// version and the licence, and starts at the corner's left and top edges or after them. **The text is the whole notice**, not cut or shortened to fit.
#[track_caller]
fn assert_the_notice_keeps_its_corner(w: &Window, (size, ppp): ([f32; 2], f32), picture: &(usize, usize, Vec<[u8; 3]>)) {
    let cream = |c: &[u8; 3]| c[0] >= 228 && c[1] >= 218 && c[2] >= 203;
    let (panel, line) = (rect_of(w, "splash.panel"), rect_of(w, "splash.trademark"));
    let k = panel.width() / ART_SIZE.x;
    // The rectangle in the picture's pixels, a pixel more on every side for the rounding of the text to whole pixels.
    let at = |p: Pos2| ((p - panel.min) / k).to_pos2();
    let (min, max) = (at(line.min), at(line.max));
    assert!(min.x >= LEGAL_ZONE[0] - 1.0 && min.y >= LEGAL_ZONE[1] - 1.0, "{size:?} at {ppp}: the notice starts at {min:?}, before its corner");
    assert!(max.x <= LEGAL_ZONE[2] + 1.0 && max.y <= LEGAL_ZONE[3] + 1.0, "{size:?} at {ppp}: the notice ends at {max:?}, past its corner");
    let covered = region(picture, [(min.x - 1.0).max(0.0), (min.y - 1.0).max(0.0), max.x + 1.0, max.y + 1.0]);
    let marks = covered.iter().filter(|c| !cream(c)).count();
    assert_eq!(marks, 0, "{size:?} at {ppp}: {marks} pixels of art under the notice");
    let title_top = title_top(picture);
    assert!(max.y + 8.0 <= title_top, "{size:?} at {ppp}: the notice ends at {}, too near the title, which starts at {title_top}", max.y);
    assert!(line.max.y < rect_of(w, "splash.version").min.y, "{size:?} at {ppp}: apart from the version row");
    // Whole: the one label holds every sentence.
    w.get_by_label(TRADEMARK);
    assert_eq!(w.state().ui_panics(), 0);
}

/// The notice stays in the clear corner at all six sizes (owner, 2026-11-23), with the faces the window has.
#[test]
fn the_notice_stays_in_the_clear_corner_at_all_six_sizes() {
    let picture = picture();
    for (size, ppp) in SIX_SIZES {
        let mut w = splash_window_at(size, ppp, app());
        run_command(&mut w, "app.about", json!({}));
        assert_the_notice_keeps_its_corner(&w, (size, ppp), &picture);
    }
}

/// A face wider than any the desktop picks (every character three quarters of an em, where Inter's text is about half an em) does not push the notice out of its corner and over
/// the title: it is laid out smaller. The test makes sure the notice would not have fitted at its usual size in that face, so that it is the fit that keeps it in.
#[test]
fn a_wider_face_makes_the_notice_smaller_and_not_longer_than_its_corner() {
    use diacraft_fonts::synth::SynthFont;
    use egui::{FontData, FontFamily, FontId};
    let picture = picture();
    // The window's own faces, with the wide one before them in the two text families: the characters it lacks (the © of the version panel) come from the bundled face.
    let mut wide = diacraft_ui_egui::fonts::font_definitions();
    wide.font_data.insert("Wide".to_owned(), std::sync::Arc::new(FontData::from_owned(SynthFont::new("Wide").ascii(750).build())));
    for family in [FontFamily::Proportional, diacraft_ui_egui::fonts::strong_family()] {
        wide.families.entry(family).or_default().insert(0, "Wide".to_owned());
    }
    for (size, ppp) in SIX_SIZES {
        let mut w = splash_window_at(size, ppp, app());
        w.ctx.set_fonts(wide.clone());
        run_command(&mut w, "app.about", json!({}));
        let panel = rect_of(&w, "splash.panel");
        let k = panel.width() / ART_SIZE.x;
        let (width, height) = ((LEGAL_ZONE[2] - LEGAL_ZONE[0]) * k, (LEGAL_ZONE[3] - LEGAL_ZONE[1]) * k);
        let usual = w.ctx.fonts_mut(|f| f.layout(TRADEMARK.to_owned(), FontId::proportional(11.0 * k), egui::Color32::BLACK, width).size().y);
        assert!(
            usual > height + 1.0,
            "{size:?} at {ppp}: the wide face is not wide enough to need the fit ({usual} tall at the usual size, a corner of {height})"
        );
        assert_the_notice_keeps_its_corner(&w, (size, ppp), &picture);
    }
}

/// The two inks are the art's own: the near-black of "DiaCraft" and the teal of "Diagramming software", the same in both themes, and 4.5 to 1 against the cream at least
/// (the worst pixel of the zone is the test, and the picture's noise is in it).
#[test]
fn the_two_inks_come_from_the_art_and_read_on_its_cream() {
    let picture = picture();
    let near = |a: [u8; 3], b: [u8; 3], d: i32| a.iter().zip(b).all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= d);
    let mid = |mut v: Vec<u8>| {
        v.sort_unstable();
        v[v.len() / 2]
    };
    let to_array = |c: egui::Color32| [c.r(), c.g(), c.b()];
    for tokens in [Tokens::light(), Tokens::dark()] {
        let (ink, link) = (to_array(tokens.splash_ink), to_array(tokens.splash_link));
        assert_eq!(tokens.splash_ink.a(), 255);
        assert_eq!(tokens.splash_link.a(), 255);
        // The title "DiaCraft": the darkest solid pixels of its rows, at the picture's size.
        let title = region(&picture, [20.0, 150.0, 350.0, 205.0]);
        let dark: Vec<[u8; 3]> = title.into_iter().filter(|c| c.iter().all(|v| *v < 40)).collect();
        assert!(dark.len() > 1000, "the title's strokes: {}", dark.len());
        let title_ink = [0, 1, 2].map(|i| mid(dark.iter().map(|c| c[i]).collect()));
        assert!(near(ink, title_ink, 4), "splash_ink {ink:?} is not the title's {title_ink:?}");
        // The subtitle "Diagramming software": its teal strokes.
        let subtitle = region(&picture, [20.0, 225.0, 330.0, 262.0]);
        let mut teal: Vec<[u8; 3]> = subtitle.into_iter().filter(|c| c[0] < 40 && (90..150).contains(&c[1]) && (105..170).contains(&c[2])).collect();
        assert!(teal.len() > 200, "the subtitle's strokes: {}", teal.len());
        // The strokes are two pixels wide at this size and their edges are lighter: the colour is the core of them, the darkest tenth.
        teal.sort_by(|a, b| luminance(*a).total_cmp(&luminance(*b)));
        teal.truncate(teal.len() / 10);
        let subtitle_teal = [0, 1, 2].map(|i| mid(teal.iter().map(|c| c[i]).collect()));
        assert!(near(link, subtitle_teal, 8), "splash_link {link:?} is not the subtitle's {subtitle_teal:?}");
        // Against the worst pixel of the zone the text is painted on.
        for (what, colour) in [("ink", ink), ("link", link)] {
            let worst = region(&picture, TEXT_ZONE).into_iter().map(|bg| contrast(colour, bg)).fold(f64::INFINITY, f64::min);
            assert!(worst >= 4.5, "the {what} reads at {worst:.2} to 1 on the cream");
        }
    }
}
