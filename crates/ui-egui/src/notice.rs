//! The status bar's notice: what the window tells a person in words, in the room the language takes (`status_bar.rs`).
//!
//! **A notice has a lifetime**. The status bar's specification says nothing of notices, so the rule is ours: a notice goes
//! [`LIFETIME`] after the window first shows it, by the window's clock (`clock.rs`), or when the next command that changes something succeeds, whichever is
//! first. A query (`document.inspect`, a program polling) is not something done and leaves it; so does a command that fails, whose caller says why in a
//! notice of its own. So a notice hides the language for [`LIFETIME`] at most. Before a change a refusal stayed through `file.new` and every later action.
//!
//! The time runs from the first frame whose status bar draws the notice, and not from when it was set: a notice set before the window is up (a busy
//! control port), or while the File view covers the status bar, is shown for its whole time. The frame that draws it asks for one more at the moment it
//! ends, and no frame before that: the window does not repaint while it waits, and `Harness::run`, which fails on a window that repaints for ever, settles.
//!
//! **A notice of work under way** ("Exporting plan.png...", "Saving Drawing1...", "Sending Drawing1 to Office laser...") has no lifetime: it stays while the
//! export or the print runs (the status bar shows the state of a print in the background), and goes when the work ends, unless the
//! work's own answer ("Exported plan.png.") has taken its place. A command does not take it away either: the work still goes.

use std::time::Duration;

use diacraft_engine::shown;

use crate::app::DiacraftApp;

/// How long a notice stays once the window has shown it: eight seconds, time to read a sentence of the status bar's width.
pub const LIFETIME: Duration = Duration::from_secs(8);

/// Longest notice, in characters of its escaped text.
const MAX_NOTICE_CHARS: usize = 399;

/// The notice in the status bar.
#[derive(Debug)]
pub(crate) struct Notice {
    /// Escaped and cut short.
    text: String,
    kind: Kind,
    /// When the window first drew it, by the window's clock: its time runs from there.
    shown_at: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// About the window, or about something done.
    Window,
    /// About the drawing open (it was saved): a new or opened drawing takes it away as well.
    Drawing,
    /// Work under way (an export, a print): it stays while the work runs.
    Work,
}

/// Whether a successful run of the command `id` is something done, which takes a notice away: every engine command that changes something, and every
/// window command. A query (an engine command that is not journaled, such as `document.inspect`) is not: a program that polls must not take away what a
/// person was told.
pub(crate) fn ends_a_notice(id: &str) -> bool {
    match diacraft_engine::find_command(id) {
        Some(spec) => spec.journal,
        None => crate::commands::find_ui_command(id).is_some(),
    }
}

/// When a notice shown at `shown_at` goes: [`LIFETIME`] later, held at the end of time rather than wrapping.
pub(crate) fn ends_at(shown_at: Duration) -> Duration {
    shown_at.saturating_add(LIFETIME)
}

impl DiacraftApp {
    /// The notice in the status bar, already escaped.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_ref().map(|n| n.text.as_str())
    }

    /// Makes `text` the notice, escaped and cut short: it can hold a panic message or a client's text, and neither may put control characters in front of a
    /// person or a screen reader. Its time starts again when it is next drawn.
    pub(crate) fn set_notice(&mut self, text: &str) {
        self.put_notice(text, Kind::Window);
    }

    /// As `set_notice`, for a notice that is about the drawing, and goes when another drawing replaces it.
    pub(crate) fn set_drawing_notice(&mut self, text: &str) {
        self.put_notice(text, Kind::Drawing);
    }

    /// As `set_notice`, for a notice of work under way (an export, a print): it stays while the work runs, and goes when it ends.
    pub(crate) fn set_work_notice(&mut self, text: &str) {
        self.put_notice(text, Kind::Work);
    }

    fn put_notice(&mut self, text: &str, kind: Kind) {
        self.notice = Some(Notice { text: shown(text, MAX_NOTICE_CHARS), kind, shown_at: None });
    }

    /// Takes the notice away if it was about the drawing that is gone (it said the drawing was saved), and leaves one about the window.
    pub(crate) fn clear_drawing_notice(&mut self) {
        if self.notice.as_ref().is_some_and(|n| n.kind == Kind::Drawing) {
            self.notice = None;
        }
    }

    /// The command `id` succeeded: if it is something done ([`ends_a_notice`]), the notice it follows goes, unless it is of work still under way.
    pub(crate) fn notice_after_command(&mut self, id: &str) {
        if self.notice.as_ref().is_some_and(|n| n.kind != Kind::Work) && ends_a_notice(id) {
            self.notice = None;
        }
    }

    /// Whether an export or a print is under way: what a notice of work stays for.
    fn work_under_way(&self) -> bool {
        self.exporting_now() || self.printing_now()
    }

    /// Before a frame is drawn: the notice goes if its time is up, or if it told of work that has ended.
    pub(crate) fn expire_notice(&mut self) {
        // The clock is read only where a time is to be compared (a ticking test clock moves at each reading).
        let gone = match &self.notice {
            None => false,
            Some(n) if n.kind == Kind::Work => !self.work_under_way(),
            Some(n) => n.shown_at.is_some_and(|at| self.clock.now() >= ends_at(at)),
        };
        if gone {
            self.notice = None;
        }
    }

    /// The status bar drew the notice: its time starts now, if it has not started yet.
    pub(crate) fn notice_shown(&mut self) {
        if self.notice.as_ref().is_some_and(|n| n.shown_at.is_none()) {
            let now = self.clock.now();
            if let Some(notice) = self.notice.as_mut() {
                notice.shown_at = Some(now);
            }
        }
    }

    /// After a frame is drawn: a frame is asked for at the moment the notice's time is up, so that it goes with no input, and none sooner. A notice set
    /// after the status bar was drawn in this frame is shown by the next frame, asked for at once (once: that frame draws it and starts its time). While
    /// the File view covers the status bar nothing is asked for, and the notice's time has not started.
    pub(crate) fn time_notice(&mut self, ctx: &egui::Context) {
        let Some(notice) = self.notice.as_ref().filter(|n| n.kind != Kind::Work) else { return };
        match notice.shown_at {
            Some(at) => ctx.request_repaint_after(ends_at(at).saturating_sub(self.clock.now())),
            None if self.ui.backstage.is_none() => ctx.request_repaint(),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_ends_its_lifetime_after_it_is_shown_and_never_wraps() {
        assert_eq!(ends_at(Duration::ZERO), LIFETIME);
        assert_eq!(ends_at(Duration::from_secs(3)), Duration::from_secs(3) + LIFETIME);
        assert_eq!(ends_at(Duration::MAX), Duration::MAX, "held at the end of time");
    }

    #[test]
    fn a_change_or_a_window_command_ends_a_notice_and_a_query_or_an_unknown_id_does_not() {
        for id in ["file.new", "shape.drop", "edit.undo", "snap.set", "view.zoomIn", "window.pane", "page.select"] {
            assert!(ends_a_notice(id), "{id}");
        }
        for id in ["document.inspect", "file.status", "app.version", "template.list", "no.such.command"] {
            assert!(!ends_a_notice(id), "{id}");
        }
    }
}
