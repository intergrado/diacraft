//! Issue 2: a notice too long for the room the status bar has is cut with an ellipsis, shown whole in a tooltip, and whole in `ui.inspect` and for a screen
//! reader.
//!
//! The notice stands left of the zoom controls, in the room the page count and the marks leave it. A sentence longer than that room was cut hard at the room's edge, in
//! the middle of a word ("Connection points doubled on 4 ec"), with nothing to say that more was said.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::Rect;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::support::{Rest, Window, call, controlled_with, move_to, window};

/// The words of a notice a command of the connection points says: too long for a narrow window's status bar.
const LONG: &str = "Connection points doubled on 4 edges of the shape: the points that were there stay, and a new one lies between each two of them";

/// A short one, which any window has room for.
const SHORT: &str = "Connection points doubled on 4 edges";

/// Windows from the least the app takes to one wide enough for the sentence, in points.
const NARROW: [[f32; 2]; 4] = [[480.0, 320.0], [540.0, 400.0], [640.0, 480.0], [760.0, 500.0]];

/// A window of `size` on a new drawing with `text` in the status bar, at rest.
fn noticed(text: &str, size: [f32; 2]) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut w = window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_notice(text), size);
    w.rest();
    w
}

/// What the last frame painted of the notice: where its text begins, how wide it is, whether the layout cut it, the characters it shows, and the clip it was painted in.
struct Painted {
    left: f32,
    width: f32,
    elided: bool,
    shown: String,
    clip: Rect,
}

/// Every text shape of the last frame that holds all of `text` in its job: the notice, and a tooltip with its words.
fn painted(w: &Window, text: &str) -> Vec<Painted> {
    w.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(Painted {
                left: t.pos.x,
                width: t.galley.size().x,
                elided: t.galley.elided,
                shown: t.galley.rows.iter().map(|row| row.text()).collect(),
                clip: clipped.clip_rect,
            }),
            _ => None,
        })
        .collect()
}

/// Where the controls at the right of the status bar begin: the left edge of the leftmost the last frame drew.
fn right_controls_start(w: &Window) -> f32 {
    ["status.zoom_percent", "status.zoom_out", "status.zoom_slider", "status.view1", "status.presentation", "status.view2"]
        .iter()
        .filter_map(|name| w.state().element(name).map(|e| e.rect.left()))
        .fold(f32::INFINITY, f32::min)
}

/// The pointer on the notice for as long as a tooltip waits.
fn hover_notice(w: &mut Window) {
    let at = w.state().element("status.notice").expect("the notice is drawn and named for a client").rect.center();
    for _ in 0..60 {
        move_to(w, at);
    }
    w.rest();
}

/// **A notice that does not fit is cut with an ellipsis inside its room**, at every width: its text ends in the ellipsis, shows enough to read, and stops before the controls at the
/// right of the bar (before, it ran on to the room's edge and was cut by the clip in the middle of a letter, and none said so).
#[test]
fn a_notice_too_long_for_its_room_ends_in_an_ellipsis_inside_it() {
    for size in NARROW {
        let w = noticed(LONG, size);
        let shapes = painted(&w, LONG);
        assert_eq!(shapes.len(), 1, "{size:?}: the notice is painted once, and no tooltip is up");
        let notice = &shapes[0];
        let limit = right_controls_start(&w);
        assert!(notice.elided, "{size:?}: the layout cut the text ({} wide, room up to {limit}) and says so", notice.width);
        assert!(notice.shown.ends_with('\u{2026}'), "{size:?}: what shows ends in an ellipsis: {:?}", notice.shown);
        assert!(notice.shown.chars().count() >= 12, "{size:?}: and shows enough to read: {:?}", notice.shown);
        assert!(
            LONG.starts_with(notice.shown.trim_end_matches('\u{2026}').trim_end()),
            "{size:?}: and is the start of the words: {:?}",
            notice.shown
        );
        assert!(notice.left + notice.width <= limit, "{size:?}: the text ends at {} and the controls begin at {limit}", notice.left + notice.width);
        assert!(notice.clip.right() >= notice.left + notice.width - 0.5, "{size:?}: the clip does not cut the ellipsis: {:?}", notice.clip);
        let rect = w.state().element("status.notice").expect("the notice is named").rect;
        assert!(rect.right() <= limit + 0.5 && rect.width() > 20.0, "{size:?}: the control the client finds is the text's room: {rect:?}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// **The whole words are in a tooltip**, and in the label of the notice for a screen reader: the pointer on the notice shows them, and they are never the words that were cut.
#[test]
fn the_whole_notice_is_in_its_tooltip_and_in_its_label() {
    for size in NARROW {
        let mut w = noticed(LONG, size);
        assert_eq!(w.query_all_by_label(LONG).count(), 1, "{size:?}: the notice is labelled with all of its words");
        hover_notice(&mut w);
        let shapes = painted(&w, LONG);
        assert_eq!(shapes.len(), 2, "{size:?}: the notice and a tooltip with the same words are painted");
        let tip = shapes.iter().find(|p| !p.elided && p.width > 0.0 && p.shown == LONG).expect("a tooltip that shows all the words");
        assert!(tip.width > 100.0, "{size:?}: the tooltip is as wide as the words: {}", tip.width);
        assert_eq!(w.state().notice(), Some(LONG), "{size:?}: the notice is as it was said");
    }
}

/// A notice that fits is painted whole and has no tooltip to repeat it.
#[test]
fn a_notice_that_fits_is_whole_and_has_no_tooltip() {
    for size in [[900.0, 600.0], [1280.0, 800.0]] {
        let mut w = noticed(SHORT, size);
        let shapes = painted(&w, SHORT);
        assert_eq!(shapes.len(), 1, "{size:?}");
        assert!(!shapes[0].elided && shapes[0].shown == SHORT, "{size:?}: whole: {:?}", shapes[0].shown);
        hover_notice(&mut w);
        assert_eq!(painted(&w, SHORT).len(), 1, "{size:?}: nothing says it again");
    }
    // The long one, too, where the room is wide enough for it.
    let w = noticed(LONG, [1920.0, 1000.0]);
    let shapes = painted(&w, LONG);
    assert!(
        shapes.len() == 1 && !shapes[0].elided && shapes[0].shown == LONG,
        "the whole sentence fits a wide window: {:?}",
        shapes.first().map(|p| p.shown.clone())
    );
}

/// **`ui.inspect` has the whole words**, whatever the window shows of them.
#[test]
fn ui_inspect_keeps_the_whole_notice() {
    let (mut w, tx) = controlled_with(|app| app.with_notice(LONG));
    w.set_size(egui::vec2(480.0, 320.0));
    w.rest();
    let shown = painted(&w, LONG);
    assert!(shown.first().is_some_and(|p| p.elided), "the window shows part of it");
    let answer = call(&mut w, &tx, "ui.inspect", json!({})).expect("the window answers");
    assert_eq!(answer["notice"], LONG, "{answer}");
}
