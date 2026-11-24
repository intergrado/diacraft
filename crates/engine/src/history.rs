//! Undo and redo by whole-document snapshots.
//!
//! A step is the document as it was before an edit, and the edit's name. A document is a handful of
//! shared lists (pages, masters, styles, themes) of `Arc`s, so a step costs one pointer for each list
//! the edit did not write to, however long it is, and for the one it did, the chunk pointers of that
//! list, the chunk and the nodes on the way to what changed (a page, a shape, a sheet): not a copy of
//! the document, and not of a page's shapes or connects.
//!
//! **The history is bounded by the bytes it holds as well as by steps.** A count of steps says nothing of memory: an edit
//! that rewrites 5,000 members of a group (a member moved past the group's edge, the group refitted) leaves a step of
//! about 20 MB of address space (the review's measure: a process of 3 GB aborted after about 137 such moves), which the price below puts at about 41 MB (measured
//! 2026-11-20: four steps priced 164,608,634 bytes), and a move of 4,096 shapes about 16 MB, so 1,000 such steps are 16 to 41 GB. Each step is
//! **charged what it keeps that the next version does not** (`odgfile::hold::unshared_bytes`, priced as the save gate prices what the
//! history holds), and the oldest steps are let go while the steps of a direction hold more than [`History::DEFAULT_BYTES`]
//! (the newest step is always kept, whatever it holds: it is one edit's own memory). When steps were let go for their bytes, **an
//! undo past the ones that are left is refused in words** ([`History::undo_refusal`]): nothing is lost silently.
//!
//! **The figure, and the 3 GB it comes from.** DiaCraft 0.2 holds a drawing to about 3 GB of address space (`docs/diacraft-format.md`, the reader's
//! caps; the child tests run under `ulimit -v 3000000`, 2.86 GiB). Beside the history the process holds: the **model**, up to 960 MiB at the open of a `.diacraft`
//! (1,344 MiB of a drawing with no pictures; an edit grows it, to the figure of an edit); a **save**, which holds the model a second time and what a read holds beside it (the writer's
//! gate counts them against 2.9 GB, with the history's measured bytes among them); the **snap reads** and indexes of a page (a few tens of MiB at
//! the most a page holds); and the **render** (a frame of the window, 64 MiB at 4k, and the paths and glyphs it keeps, a few hundred MiB). The history is
//! two directions, an undo and a redo, each bounded alone: 2 x 192 MiB = 384 MiB. 960 (model) + 384 (history) + 960 (a save's second model) + 300 (render and
//! snap) is 2.54 GiB, under the 2.86 GiB of the child tests (a drawing of more than that is held by the figure of an edit and the save's own peak check, which refuse in words), with
//! about 0.3 GiB to spare for the allocator, the stacks and a save's package. A step of the
//! worst edit the commands allow (4,096 shapes of 4 KB: 16 MB) leaves room for 12 steps in a direction.

use std::sync::Arc;

use diacraft_doc::Document;

/// A size in words: whole MiB, or bytes below one.
fn size_words(bytes: u64) -> String {
    let mib = bytes / (1024 * 1024);
    if mib > 0 { format!("{mib} MiB") } else { format!("{bytes} bytes") }
}

/// One step: the document before an edit, what the edit was called, and what the step keeps that the next version does not.
#[derive(Clone, Debug)]
struct Step {
    label: Arc<str>,
    doc: Arc<Document>,
    bytes: u64,
}

/// The undo and redo steps of a document.
///
/// Cloning is cheap enough to take before every command (a few thousand reference counts at the
/// limit), which is how a failed or panicking command is rolled back.
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    limit: usize,
    byte_limit: u64,
    undo_bytes: u64,
    redo_bytes: u64,
    /// Whether steps were let go for the bytes they hold: the steps before the ones kept are gone for good.
    undo_cut: bool,
    redo_cut: bool,
    /// Whether undo steps were let go to keep **the drawing** within the memory a session may hold (the session's count, `session/held.rs`): the steps before the
    /// ones kept are gone for good. A drawing past what the session may hold keeps one step, or none.
    drawing_cut: bool,
    /// The documents of the steps that the history's **own** limits let go of (the steps and the bytes of a direction), kept until the session takes them ([`History::take_cut`]) so
    /// that it can take from its count of the memory it holds what they free, before they are dropped: a step that the history lets go of is not a recount of the drawing.
    cut: Vec<Arc<Document>>,
}

impl Default for History {
    fn default() -> Self {
        History::with_limits(History::DEFAULT_LIMIT, History::DEFAULT_BYTES)
    }
}

impl History {
    /// Most undo steps (and most redo steps) kept: the oldest go first.
    pub const DEFAULT_LIMIT: usize = 1_000;

    /// Most bytes the undo steps hold (and the redo steps, each alone): 192 MiB. The module's words give the arithmetic.
    pub const DEFAULT_BYTES: u64 = 192 * 1024 * 1024;

    /// The fewest steps any history keeps, whatever limit it is given.
    pub const MIN_STEPS: usize = 8;

    /// A history that keeps `limit` steps, or [`History::MIN_STEPS`] when that is more, and [`History::DEFAULT_BYTES`] of them.
    pub fn with_limit(limit: usize) -> Self {
        History::with_limits(limit, History::DEFAULT_BYTES)
    }

    /// A history that keeps `limit` steps and `bytes` bytes in each direction: the oldest steps go first, and the newest is kept whatever it holds.
    pub fn with_limits(limit: usize, bytes: u64) -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(History::MIN_STEPS),
            byte_limit: bytes,
            undo_bytes: 0,
            redo_bytes: 0,
            undo_cut: false,
            redo_cut: false,
            drawing_cut: false,
            cut: Vec::new(),
        }
    }

    /// Most steps kept in each direction.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Most bytes the steps of a direction may hold.
    pub fn byte_limit(&self) -> u64 {
        self.byte_limit
    }

    /// How many steps undo can take back.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// How many steps redo can put again.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// The bytes the undo steps hold, as they were charged.
    pub fn undo_bytes(&self) -> u64 {
        self.undo_bytes
    }

    /// The bytes the redo steps hold, as they were charged.
    pub fn redo_bytes(&self) -> u64 {
        self.redo_bytes
    }

    /// Whether older undo steps were let go for the bytes they hold.
    pub fn undo_was_cut(&self) -> bool {
        self.undo_cut
    }

    /// Whether older redo steps were let go for the bytes they hold.
    pub fn redo_was_cut(&self) -> bool {
        self.redo_cut
    }

    /// Gives the history other limits (a frontend with less memory lowers them): the steps held are kept, and the oldest let go if they are past the new ones.
    pub fn set_limits(&mut self, steps: usize, bytes: u64) {
        self.limit = steps.max(History::MIN_STEPS);
        self.byte_limit = bytes;
        self.trim();
    }

    /// The name of the step undo would take back.
    pub fn next_undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| &*s.label)
    }

    /// The name of the step redo would put again.
    pub fn next_redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| &*s.label)
    }

    /// Why there is nothing to undo, in words: no step was ever made, or the older steps were let go to keep the history, or the drawing, within its memory.
    pub fn undo_refusal(&self) -> String {
        if self.drawing_cut {
            "the older steps were let go to keep the drawing within its memory, and nothing before them can be undone".to_owned()
        } else if self.undo_cut {
            format!("the older steps were let go to keep the history within {}, and nothing before them can be undone", size_words(self.byte_limit))
        } else {
            "nothing to undo".to_owned()
        }
    }

    /// Why there is nothing to redo, in words (see [`History::undo_refusal`]).
    pub fn redo_refusal(&self) -> String {
        if self.redo_cut {
            format!("the farther steps were let go to keep the history within {}, and nothing beyond them can be redone", size_words(self.byte_limit))
        } else {
            "nothing to redo".to_owned()
        }
    }

    /// The document undo would go back to.
    pub(crate) fn next_undo_document(&self) -> Option<&Arc<Document>> {
        self.undo.last().map(|s| &s.doc)
    }

    /// The document redo would go on to.
    pub(crate) fn next_redo_document(&self) -> Option<&Arc<Document>> {
        self.redo.last().map(|s| &s.doc)
    }

    /// Records an edit named `label` that started from `before`, and left a step that holds `bytes`. A new edit ends the redo steps.
    pub(crate) fn record(&mut self, label: Arc<str>, before: Arc<Document>, bytes: u64) {
        self.undo_bytes = self.undo_bytes.saturating_add(bytes);
        self.undo.push(Step { label, doc: before, bytes });
        self.redo.clear();
        self.redo_bytes = 0;
        self.redo_cut = false;
        self.trim();
    }

    /// Takes back the newest undo step: `current` becomes a redo step that holds `redo_bytes`, and the name and the document
    /// to go back to are returned. `None`, with nothing changed, when there is no step.
    pub(crate) fn undo(&mut self, current: Arc<Document>, redo_bytes: u64) -> Option<(Arc<str>, Arc<Document>)> {
        let step = self.undo.pop()?;
        self.undo_bytes = self.undo_bytes.saturating_sub(step.bytes);
        self.redo_bytes = self.redo_bytes.saturating_add(redo_bytes);
        self.redo.push(Step { label: Arc::clone(&step.label), doc: current, bytes: redo_bytes });
        self.trim();
        Some((step.label, step.doc))
    }

    /// Puts again the newest redo step: the mirror of [`History::undo`].
    pub(crate) fn redo(&mut self, current: Arc<Document>, undo_bytes: u64) -> Option<(Arc<str>, Arc<Document>)> {
        let step = self.redo.pop()?;
        self.redo_bytes = self.redo_bytes.saturating_sub(step.bytes);
        self.undo_bytes = self.undo_bytes.saturating_add(undo_bytes);
        self.undo.push(Step { label: Arc::clone(&step.label), doc: current, bytes: undo_bytes });
        self.trim();
        Some((step.label, step.doc))
    }

    /// Lets go of the oldest `n` undo steps (fewer if there are not so many), whatever they hold: what the session's count of the memory it holds decided before an edit was made
    /// (`session/held.rs`: the steps the limit of steps forces, and those that make room for the edit). Says nothing of it; [`History::mark_drawing_cut`] does when it was for memory.
    pub(crate) fn drop_oldest_undo(&mut self, n: usize) {
        let n = n.min(self.undo.len());
        let gone: u64 = self.undo.drain(..n).fold(0, |sum, step| sum.saturating_add(step.bytes));
        self.undo_bytes = self.undo_bytes.saturating_sub(gone);
    }

    /// Notes that undo steps were let go of to keep the drawing within its memory: an undo that finds none says so ([`History::undo_refusal`]).
    pub(crate) fn mark_drawing_cut(&mut self) {
        self.drawing_cut = true;
    }

    /// The documents of the steps the history's own limits let go of since the last call, oldest first: the session takes from its count what they free, and then drops them. (A
    /// history that nobody asks holds them; the session asks after every call that can cut.)
    pub(crate) fn take_cut(&mut self) -> Vec<Arc<Document>> {
        std::mem::take(&mut self.cut)
    }

    /// Whether undo steps were let go of to keep the drawing within its memory.
    pub fn drawing_was_cut(&self) -> bool {
        self.drawing_cut
    }

    /// The documents the undo steps hold, oldest first: each is the document as it was before the edit that came next, and the newest is the one before the
    /// edit that made the document now. What an `.odg` save counts of the memory the history holds beside the drawing (`odgfile/hold.rs`).
    pub(crate) fn undo_documents(&self) -> impl Iterator<Item = &Arc<Document>> {
        self.undo.iter().map(|step| &step.doc)
    }

    /// The documents the redo steps hold, in the order they are kept: the last is the one an undo went back from, nearest to the document now, and the
    /// first the farthest from it.
    pub(crate) fn redo_documents(&self) -> impl Iterator<Item = &Arc<Document>> {
        self.redo.iter().map(|step| &step.doc)
    }

    /// The bytes the steps of both directions hold, as they were charged: what a count of the memory the session holds adds for its history.
    pub fn total_bytes(&self) -> u64 {
        self.undo_bytes.saturating_add(self.redo_bytes)
    }

    /// Lets the oldest steps go while there are more than the limit, and while the steps of a direction hold more than the bytes allowed (the newest stays). The documents of the
    /// steps that go are kept in `cut` until [`History::take_cut`].
    fn trim(&mut self) {
        let (limit, byte_limit) = (self.limit, self.byte_limit);
        for (steps, held, cut) in
            [(&mut self.undo, &mut self.undo_bytes, &mut self.undo_cut), (&mut self.redo, &mut self.redo_bytes, &mut self.redo_cut)]
        {
            let over = steps.len().saturating_sub(limit);
            if over > 0 {
                let mut gone = 0u64;
                for step in steps.drain(..over) {
                    gone = gone.saturating_add(step.bytes);
                    self.cut.push(step.doc);
                }
                *held = held.saturating_sub(gone);
            }
            shed(steps, held, cut, byte_limit, &mut self.cut);
        }
    }
}

/// Lets the oldest of `steps` go while what they hold (`held`) is past `allowed`; the newest stays. Says in `cut` that steps went for their bytes, and gives their documents to `out`.
fn shed(steps: &mut Vec<Step>, held: &mut u64, cut: &mut bool, allowed: u64, out: &mut Vec<Arc<Document>>) {
    let mut dropped = 0;
    let mut left = *held;
    while left > allowed && dropped + 1 < steps.len() {
        left = left.saturating_sub(steps.get(dropped).map_or(0, |s| s.bytes));
        dropped += 1;
    }
    if dropped > 0 {
        out.extend(steps.drain(..dropped).map(|step| step.doc));
        *held = left;
        *cut = true;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn doc() -> Arc<Document> {
        Arc::new(Document::new_blank())
    }

    fn label() -> Arc<str> {
        Arc::from("edit")
    }

    #[test]
    fn a_step_is_charged_what_it_holds_and_the_oldest_go_past_the_budget() {
        let mut h = History::with_limits(1_000, 100);
        for _ in 0..3 {
            h.record(label(), doc(), 30);
        }
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut()), (3, 90, false), "90 of 100 held");
        h.record(label(), doc(), 30);
        // 120 would be held: the oldest step goes, and the others are what is held.
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut()), (3, 90, true));
        h.record(label(), doc(), 75);
        // 165: the two oldest go, 105 is still past the budget, and a third goes too, leaving the newest alone at 75.
        assert_eq!((h.undo_len(), h.undo_bytes()), (1, 75));
    }

    #[test]
    fn the_steps_a_limit_of_steps_lets_go_of_are_given_to_the_session_oldest_first_and_their_bytes_are_no_longer_held() {
        // The cut by the number of steps (not by bytes): the oldest steps go, their documents are handed over once, the bytes they held are let go of, and no words say it
        // was for memory.
        let mut h = History::with_limits(History::MIN_STEPS, 1_000_000);
        let docs: Vec<Arc<Document>> = (0..12).map(|_| doc()).collect();
        for (i, d) in docs.iter().enumerate() {
            h.record(label(), Arc::clone(d), 10 * (i as u64 + 1));
        }
        // Twelve steps of 10, 20, ... 120 bytes under a limit of eight: the eight newest are held, 50 + 60 + ... + 120 = 680 bytes.
        assert_eq!((h.undo_len(), h.undo_bytes()), (History::MIN_STEPS, 680));
        assert!(!h.undo_was_cut(), "a cut by the number of steps is no cut for bytes");
        let cut = h.take_cut();
        assert_eq!(cut.len(), 4);
        assert!(cut.iter().zip(&docs).all(|(gone, first)| Arc::ptr_eq(gone, first)), "the oldest first");
        assert!(h.take_cut().is_empty(), "handed over once");
        // One more step past the limit lets the oldest of the eight go, and its bytes with it.
        h.record(label(), doc(), 5);
        assert_eq!((h.undo_len(), h.undo_bytes()), (History::MIN_STEPS, 680 - 50 + 5));
        let cut = h.take_cut();
        assert_eq!(cut.len(), 1);
        assert!(Arc::ptr_eq(&cut[0], &docs[4]));
        // The redo steps are held to the same limit and given the same way: undo them all, then record more than the limit and undo again.
        let mut h = History::with_limits(History::MIN_STEPS, 1_000_000);
        for _ in 0..History::MIN_STEPS {
            h.record(label(), doc(), 10);
        }
        for _ in 0..History::MIN_STEPS {
            h.undo(doc(), 7).unwrap();
        }
        assert_eq!((h.undo_len(), h.redo_len(), h.redo_bytes()), (0, History::MIN_STEPS, 7 * History::MIN_STEPS as u64));
        assert!(h.take_cut().is_empty(), "nothing is past the limit");
    }

    #[test]
    fn the_refusal_says_the_budget_in_whole_mebibytes_from_one_up_and_in_bytes_below() {
        assert_eq!(size_words(0), "0 bytes");
        assert_eq!(size_words(1_048_575), "1048575 bytes");
        assert_eq!(size_words(1_048_576), "1 MiB");
        assert_eq!(size_words(History::DEFAULT_BYTES), "192 MiB");
        let mut h = History::default();
        h.record(label(), doc(), History::DEFAULT_BYTES);
        h.record(label(), doc(), History::DEFAULT_BYTES);
        assert!(h.undo_refusal().contains("within 192 MiB,"), "{}", h.undo_refusal());
    }

    #[test]
    fn the_newest_step_is_kept_whatever_it_holds() {
        let mut h = History::with_limits(1_000, 10);
        h.record(label(), doc(), 500);
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut()), (1, 500, false), "one step is all there is to keep");
        h.record(label(), doc(), 500);
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut()), (1, 500, true), "the older one went");
    }

    #[test]
    fn a_limit_of_steps_lets_go_of_the_bytes_too_and_says_nothing_of_it() {
        let mut h = History::with_limits(8, u64::MAX);
        for _ in 0..20 {
            h.record(label(), doc(), 10);
        }
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut()), (8, 80, false), "dropped for their number, not for their bytes");
        assert_eq!(h.undo_refusal(), "nothing to undo");
    }

    #[test]
    fn an_undo_past_the_steps_let_go_for_bytes_is_refused_in_words() {
        let mut h = History::with_limits(1_000, 50);
        for _ in 0..4 {
            h.record(label(), doc(), 30);
        }
        assert_eq!(h.undo_len(), 1);
        assert!(h.undo(doc(), 30).is_some());
        assert_eq!(h.undo_len(), 0);
        assert!(h.undo(doc(), 30).is_none());
        let words = h.undo_refusal();
        assert!(
            words.contains("older steps were let go") && words.contains("within 50 bytes") && words.contains("nothing before them can be undone"),
            "{words}"
        );
        let mut fresh = History::default();
        assert_eq!((fresh.undo_refusal().as_str(), fresh.redo_refusal().as_str()), ("nothing to undo", "nothing to redo"));
        assert!(fresh.undo(doc(), 0).is_none());
    }

    #[test]
    fn the_bytes_go_with_a_step_from_one_direction_to_the_other() {
        let mut h = History::default();
        h.record(label(), doc(), 40);
        h.record(label(), doc(), 60);
        assert_eq!((h.undo_bytes(), h.redo_bytes()), (100, 0));
        // The step that takes the place of the one taken is charged what the version left holds beside the one gone back to.
        h.undo(doc(), 25).unwrap();
        assert_eq!((h.undo_len(), h.undo_bytes(), h.redo_len(), h.redo_bytes()), (1, 40, 1, 25));
        h.redo(doc(), 70).unwrap();
        assert_eq!((h.undo_len(), h.undo_bytes(), h.redo_len(), h.redo_bytes()), (2, 110, 0, 0));
        // A new edit ends the redo steps, and what they held.
        h.undo(doc(), 25).unwrap();
        h.record(label(), doc(), 5);
        assert_eq!((h.redo_len(), h.redo_bytes(), h.redo_was_cut()), (0, 0, false));
    }

    #[test]
    fn the_redo_steps_are_held_to_the_budget_too_and_say_so() {
        let mut h = History::with_limits(1_000, 50);
        for _ in 0..6 {
            h.record(label(), doc(), 10);
        }
        // Six steps of 10 are 60, past 50: the oldest went, and five are left to take back.
        for _ in 0..5 {
            h.undo(doc(), 30).unwrap();
        }
        // Five redo steps of 30 are 150, past 50: the farthest go, and redo past the ones left is refused in words.
        assert_eq!((h.redo_len(), h.redo_bytes(), h.redo_was_cut()), (1, 30, true));
        h.redo(doc(), 10).unwrap();
        assert!(h.redo(doc(), 10).is_none());
        assert!(h.redo_refusal().contains("farther steps were let go"), "{}", h.redo_refusal());
    }

    #[test]
    fn new_limits_apply_to_the_steps_held() {
        let mut h = History::default();
        for _ in 0..10 {
            h.record(label(), doc(), 10);
        }
        h.set_limits(1_000, 35);
        assert_eq!((h.undo_len(), h.undo_bytes(), h.undo_was_cut(), h.byte_limit()), (3, 30, true, 35));
        h.set_limits(8, 1_000);
        assert_eq!((h.undo_len(), h.limit()), (3, 8));
    }
}
