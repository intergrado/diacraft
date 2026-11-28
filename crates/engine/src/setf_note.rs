//! What an edit says of the `SETF` calls it dropped (the effects past the cap "are dropped with a note").
//!
//! `calc` makes the writes a `SETF` asks for within a loop budget (`diacraft_calc::rules::MAX_SETF_WRITES` writes, `MAX_SETF_ROUNDS` rounds, `MAX_SETF_QUEUED` calls to a
//! working-out) and **counts** the calls past it in the report of each commit (`CommitReport::setf_dropped`). The commits of an edit are made deep inside its command, where the
//! command's answer is not in reach, so the count is kept for the edit as the connectors an edit left are (`draw::EditRoutes`): [`EditSetf`] is held by `Session::edit_if` for
//! as long as the edit runs, every commit the commands make through `calc_index::commit_changed` adds its report's count ([`note`]), and the session takes the total when the edit
//! is made and puts it in the command's answer (`setf_dropped`, and [`dropped_words`] among its `notes`), from where the window shows the same words in its status bar.

use std::cell::Cell;

use diacraft_calc::CommitReport;
use diacraft_calc::rules::{MAX_SETF_ROUNDS, MAX_SETF_WRITES};

thread_local! {
    /// The calls the commits of the edit that is running on this thread have dropped, while an edit is running.
    static DROPPED: Cell<Option<u64>> = const { Cell::new(None) };
}

/// For as long as it lives, the commits made on this thread add the `SETF` calls they drop to one count, the edit's. An edit that runs inside another (a command that
/// runs a command) adds to the outer one's count, and only the outer one takes it.
pub(crate) struct EditSetf {
    entered: bool,
}

impl EditSetf {
    pub(crate) fn enter() -> EditSetf {
        let entered = DROPPED.with(|dropped| {
            if dropped.get().is_some() {
                return false;
            }
            dropped.set(Some(0));
            true
        });
        EditSetf { entered }
    }

    /// The calls dropped since the edit began or since the last take: taken, so that each is said once. An edit that is inside another takes none.
    pub(crate) fn take(&self) -> u64 {
        if !self.entered {
            return 0;
        }
        DROPPED.with(|dropped| dropped.replace(Some(0)).unwrap_or(0))
    }
}

impl Drop for EditSetf {
    fn drop(&mut self) {
        if self.entered {
            DROPPED.with(|dropped| dropped.set(None));
        }
    }
}

/// Adds the `SETF` calls a commit dropped to the count of the edit that is running on this thread, if one is: a commit made outside an edit has no answer to say them in.
pub(crate) fn note(report: &CommitReport) {
    if report.setf_dropped == 0 {
        return;
    }
    DROPPED.with(|dropped| {
        if let Some(so_far) = dropped.get() {
            dropped.set(Some(so_far.saturating_add(report.setf_dropped)));
        }
    });
}

/// The words an edit says of `dropped` `SETF` calls it did not make: how many, and the caps they were past (the window shows these words in its status bar).
#[must_use]
pub fn dropped_words(dropped: u64) -> String {
    let calls = if dropped == 1 { "1 SETF call was".to_owned() } else { format!("{dropped} SETF calls were") };
    format!(
        "{calls} dropped: one edit makes at most {MAX_SETF_WRITES} SETF writes and {MAX_SETF_ROUNDS} rounds of them, so a SETF that sets itself off again stops there"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(n: u64) -> CommitReport {
        CommitReport { setf_dropped: n, ..CommitReport::default() }
    }

    /// The count is the edit's: commits add to it while the guard lives, a take gives it once, and a commit outside any edit leaves nothing behind for the next edit.
    #[test]
    fn the_calls_commits_drop_are_counted_for_the_edit_and_taken_once() {
        note(&dropped(5));
        let edit = EditSetf::enter();
        assert_eq!(edit.take(), 0, "a commit made before the edit began is not the edit's");
        note(&dropped(2));
        note(&dropped(0));
        note(&dropped(3));
        assert_eq!(edit.take(), 5);
        assert_eq!(edit.take(), 0, "each is said once");
        note(&dropped(1));
        drop(edit);
        assert_eq!(EditSetf::enter().take(), 0, "an edit that ended leaves nothing behind");
    }

    /// An edit inside another adds to the outer one's count and takes none, and its end leaves the outer one's count in place.
    #[test]
    fn an_edit_inside_another_leaves_the_count_to_the_outer_one() {
        let outer = EditSetf::enter();
        note(&dropped(4));
        {
            let inner = EditSetf::enter();
            note(&dropped(1));
            assert_eq!(inner.take(), 0);
        }
        assert_eq!(outer.take(), 5);
    }

    /// The count saturates and never wraps.
    #[test]
    fn the_count_does_not_wrap() {
        let edit = EditSetf::enter();
        note(&dropped(u64::MAX));
        note(&dropped(7));
        assert_eq!(edit.take(), u64::MAX);
    }

    #[test]
    fn the_words_count_the_calls_and_name_the_caps() {
        assert_eq!(
            dropped_words(1),
            "1 SETF call was dropped: one edit makes at most 64 SETF writes and 16 rounds of them, so a SETF that sets itself off again stops there"
        );
        assert!(dropped_words(36).starts_with("36 SETF calls were dropped: "));
    }
}
