//! The messages the page shows when the app cannot start, and the window the shell makes.

use diacraft_browser::page::{CANVAS_ID, GuardStep, LeaveGuard, missing_canvas, new_app, start_failed};

#[test]
fn a_page_without_a_canvas_is_told_which_id_is_missing() {
    let lines = missing_canvas();
    assert!(lines.first().is_some_and(|l| l.contains(CANVAS_ID)), "{lines:?}");
    assert!(lines.len() > 1, "a message says what to do as well as what happened: {lines:?}");
}

#[test]
fn a_start_failure_shows_the_browsers_reason_and_what_the_browser_needs() {
    let lines = start_failed("no suitable graphics adapter");
    assert!(lines.first().is_some_and(|l| l.contains("no suitable graphics adapter")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("WebGPU or WebGL2")), "{lines:?}");
}

/// The reason is the browser's text: it is escaped (no control character reaches the page or a screen reader) and
/// cut short.
#[test]
fn a_start_failure_escapes_and_bounds_the_reason() {
    for line in start_failed("adapter\u{1b}[31m lost\u{0}\r\nnext\u{202e}line") {
        assert!(!line.chars().any(|c| c.is_control() || c == '\u{202e}'), "{line:?}");
    }
    let long = "x".repeat(100_000);
    let lines = start_failed(&long);
    let first = lines.first().unwrap();
    assert!(first.chars().count() < 400, "{} characters", first.chars().count());
    assert!(first.ends_with('…'), "{first:?}");
    // An empty reason is still a message.
    assert!(start_failed("").first().is_some_and(|l| l.starts_with("DiaCraft could not start")));
}

#[test]
fn the_shell_starts_the_window_with_no_document_open() {
    assert_eq!(new_app().status_text(), "No document open");
}

/// The web page starts with the splash popup, and says the same git release and year as the build it is made from: this crate's build script gave them, and the
/// engine answers them in `app.version`.
#[test]
fn the_shell_starts_the_window_with_the_splash_and_the_builds_git_release_and_year() {
    let mut app = new_app();
    assert_eq!(app.splash_kind(), Some(diacraft_ui_egui::splash::SplashKind::Start));
    assert_eq!(app.build_info().git, env!("DIACRAFT_BUILD_GIT"));
    assert_eq!(app.build_info().year, env!("DIACRAFT_BUILD_YEAR"));
    let answer = app.execute("app.version", &Default::default()).unwrap();
    assert_eq!((answer["git"].as_str(), answer["year"].as_str()), (Some(env!("DIACRAFT_BUILD_GIT")), Some(env!("DIACRAFT_BUILD_YEAR"))));
}

// ------------------------------------------------------------------------------------------------------------------------------ asking before the page is left

/// The handler for `beforeunload` is on the page exactly while the drawing has unsaved changes: put on by the first change, taken off by the save or the new drawing, and an idle page never has it.
#[test]
fn the_handler_is_on_the_page_only_while_the_drawing_has_unsaved_changes() {
    let mut guard = LeaveGuard::default();
    assert!(!guard.attached(), "a page starts with nothing to lose");
    assert_eq!(guard.want(false), GuardStep::Keep, "told there is nothing to lose: nothing to take off");
    assert_eq!(guard.want(true), GuardStep::Attach);
    assert!(guard.attached());
    assert_eq!(guard.want(true), GuardStep::Keep, "told again: not put on twice");
    assert_eq!(guard.want(false), GuardStep::Detach, "saved");
    assert!(!guard.attached());
    assert_eq!(guard.want(false), GuardStep::Keep);
}

/// The window's own close takes the handler off before it asks the browser to close, and it stays off until the page knows the browser kept the window: no second question after DiaCraft's.
#[test]
fn the_handler_goes_before_the_window_closes_and_stays_away_until_the_browser_keeps_the_window() {
    let mut guard = LeaveGuard::default();
    assert_eq!(guard.want(true), GuardStep::Attach);
    // Ctrl+W: the window says Don't Save or Save, and its close tells the host first (the drawing, unsaved, is still `wanted`).
    assert_eq!(guard.want(false), GuardStep::Detach, "the window clears it before the close");
    assert_eq!(guard.closing(), GuardStep::Keep);
    // The frame that follows tells the host again that the drawing has changes (Don't Save left it so): the handler must not come back while the close is under way.
    assert_eq!(guard.want(true), GuardStep::Keep, "closing: held off");
    assert!(!guard.attached());
    // The browser kept the window (a tab a script did not open): the drawing is still unsaved, so the handler is on again.
    assert_eq!(guard.kept(), GuardStep::Attach);
    assert!(guard.attached());
}

/// A close that finds nothing to lose, or one the browser keeps with a saved drawing, never puts the handler on; and a close takes the handler off even when the window did not say so first.
#[test]
fn a_close_with_nothing_to_lose_never_puts_the_handler_on() {
    let mut guard = LeaveGuard::default();
    assert_eq!(guard.closing(), GuardStep::Keep);
    assert_eq!(guard.kept(), GuardStep::Keep, "kept, and the drawing has no changes");
    assert!(!guard.attached());
    let mut guard = LeaveGuard::default();
    assert_eq!(guard.want(true), GuardStep::Attach);
    assert_eq!(guard.closing(), GuardStep::Detach);
    assert_eq!(guard.kept(), GuardStep::Attach);
}

/// The one place the page listens for the event names it: the event that fires is `beforeunload`. The web runner's own handler is registered as `onbeforeunload`, which no browser fires (that is
/// why a closed tab lost the drawing with no word); a handler of ours under that name would fail the same way.
#[test]
fn the_handler_listens_for_the_event_that_fires() {
    let page = crate::read("src/web_page.rs");
    assert!(page.contains("const LEAVE_EVENT: &str = \"beforeunload\";"), "the event's name is `beforeunload`");
    assert!(!page.contains("\"onbeforeunload\""), "no event has this name");
    // The browser asks when the event is cancelled (the standard way) and when `returnValue` is set (what the browsers before it need).
    assert!(page.contains("prevent_default()") && page.contains("set_return_value("));
}
