//! The status bar's notice has a lifetime: it goes when its time is up by the window's clock, or at the next command that changes
//! something, whichever is first, and the language it stood in for comes back. Before a change a failed command's words stayed through `file.new` and every
//! later action, and the language never showed again (section 8).

use std::time::Duration;

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::canvas::picture::REST_AFTER;
use diacraft_ui_egui::notice::LIFETIME;
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::ViewportId;
use serde_json::json;

use crate::support::{Rest, Window, window};

/// The audit's words: a refusal that stayed for dozens of actions.
const REFUSAL: &str = "`shape.intersect` failed: the shapes have no area in common";

/// A window of `size` on a new drawing with `text` in the status bar, at rest, and with its picture at rest too (so that the only timed frame left to
/// ask for is the notice's).
fn noticed(text: &str, size: [f32; 2]) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut w = window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_notice(text), size);
    w.rest();
    w.advance(REST_AFTER + Duration::from_millis(1));
    w.rest();
    w
}

/// How long the last frame asked the desktop to wait before the next one, if no input comes first.
fn repaint_delay(w: &Window) -> Duration {
    w.output().viewport_output.get(&ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay)
}

/// The notice is drawn in the room of the language, and when its time is up by the window's clock it goes and the language is back. The last frame before
/// that asks for a frame at the moment it ends (so it goes with no input), and none sooner: `rest`, which fails on a window that repaints for ever, passes
/// at every step.
#[test]
fn a_notice_goes_when_its_time_is_up_and_the_language_comes_back() {
    for size in [[1280.0, 800.0], [1920.0, 1000.0]] {
        let mut w = noticed(REFUSAL, size);
        assert_eq!(w.state().notice(), Some(REFUSAL));
        assert!(w.state().element("status.language").is_none(), "{size:?}: the notice has the language's room");
        let left = LIFETIME - REST_AFTER - Duration::from_millis(1);
        assert_eq!(repaint_delay(&w), left, "{size:?}: a frame is asked for when the notice ends, and not sooner");
        w.advance(left - Duration::from_millis(1));
        w.rest();
        assert_eq!(w.state().notice(), Some(REFUSAL), "{size:?}: a millisecond before its end it is still there");
        assert_eq!(repaint_delay(&w), Duration::from_millis(1), "{size:?}");
        w.advance(Duration::from_millis(1));
        w.rest();
        assert_eq!(w.state().notice(), None, "{size:?}: its time is up");
        assert!(w.state().element("status.language").is_some(), "{size:?}: the language is back");
        assert_eq!(repaint_delay(&w), Duration::MAX, "{size:?}: and nothing more is asked for");
    }
}

/// The next command that changes something takes the notice away (here `file.new`, through which the audit saw a refusal stay), and so does a window
/// command (a zoom); a query (`document.inspect`, a program polling) does not, and nor does a command that fails.
#[test]
fn the_next_command_that_changes_something_takes_the_notice_away() {
    let mut w = noticed(REFUSAL, [1280.0, 800.0]);
    w.state_mut().execute("document.inspect", &json!({})).unwrap();
    assert!(w.state_mut().execute("page.delete", &json!({"id": 999})).is_err());
    w.rest();
    assert_eq!(w.state().notice(), Some(REFUSAL), "a query and a refusal are not the next thing done");
    w.state_mut().execute("file.new", &json!({})).unwrap();
    w.rest();
    assert_eq!(w.state().notice(), None, "file.new took it away");
    assert!(w.state().element("status.language").is_some(), "and the language is back");

    let mut w = noticed(REFUSAL, [1280.0, 800.0]);
    w.state_mut().execute("view.zoomIn", &json!({})).unwrap();
    w.rest();
    assert_eq!(w.state().notice(), None, "a window command is something done too");
}
