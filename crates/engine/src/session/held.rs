//! **What a session holds of the model, counted once**: the open document, every undo and redo step, and the documents a save keeps
//! (the one the file holds and the one it reads as), by the allocations they hold **between them**, each counted once, and the characters of the strings the cells and
//! the texts hold. A drawing is held with its history, and the count that guards an edit is of both.
//!
//! **The count is kept, not made.** Counting the documents whole is a walk of the drawing, and an edit of one shape of a hundred thousand must not cost one. So the session keeps a
//! [`Held`]: the count at the least, from which each edit gives and takes only what it changed: it **adds** what the edit made ([`grown`](diacraft_sheetxml::model::grown):
//! the allocations of the new document that are not the ones the old holds in the same places, found by pointer, so that the walk is of what the edit copied) and **takes
//! away** what the documents let go of ([`Released`]: a document that is dropped frees the allocations that no other document holds, and nothing else). An undo or a redo moves a document from
//! the history to the drawing and back, and the documents the session holds are the same, so it leaves the count as it is. Every allocation is charged the same price on
//! the way in and on the way out, so that the kept count is the count of the documents whole whenever each allocation that was added was met once ([`Held::drifted`] says when
//! one may have been met twice: a shape that moved to another place of its list is the same allocation in two places, and the walk meets it as new), and then the count is **at
//! least** what is held. A count past the figure that may be more than what is held is made again, whole, before an edit is refused for it.
//!
//! **Past the figure, the history makes room.** The oldest undo steps are let go of, one after another, until the drawing and what is left of its history are within the
//! figure; an edit is refused in words only when the drawing alone, as the edit leaves it, is past the figure and larger than it was. Nothing is changed by a refusal.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_sheetxml::model::{Added, Released, grown, held_bytes_of};

use crate::error::{Result, failed, shown};
use crate::history::History;

/// The count a session keeps of the model it holds ([module](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    /// The bytes held, at the least, as far as the count is exact: the allocations the session's documents hold between them, once each, by [`held_bytes_of`]'s prices.
    pub bytes: u64,
    /// Whether the count may be more than what is held: an addition that may have been of what was counted, or a document that was let go of without taking away what it freed. A
    /// count that passes the figure while this is true is made again, whole, and is exact.
    pub drifted: bool,
    /// **By how much the count may be more than what is held, at the most**, for what edits added that another place may have held already (a sheet that a new shape shares with the
    /// shape it was made from is walked whole as new, and is held once). It is the sum of what those edits counted of it since the count was last made whole. The count is made again,
    /// whole, only when the figure is passed **and the slack could be the whole of what passes it** (the count less the slack is within the figure): an edit of a drawing is not a walk of
    /// it, a drawing far past the figure is not walked for the doubt of a copy, and the count is never less than what is held.
    pub slack: u64,
}

impl Held {
    /// The count of `docs` whole.
    pub fn of<'a>(docs: impl IntoIterator<Item = &'a Document>) -> Held {
        Held { bytes: held_bytes_of(docs, u64::MAX), drifted: false, slack: 0 }
    }

    /// Whether the count may be more than what is held, by anything.
    pub fn may_be_more(&self) -> bool {
        self.drifted || self.slack > 0
    }
}

/// What an edit that is made is to do to the session beside putting its document in: the count to keep, and how many of the oldest undo steps to let go of.
pub(crate) struct Plan {
    pub held: Held,
    pub drop_undo: usize,
    /// How many of those the limit of steps forces (the rest make room for the model, and the history is then **cut**: it says so, [`crate::history::History::cut_for_memory`]).
    pub forced: usize,
}

/// What is known of an edit that was made on a copy off to the side, before it is put in.
pub(crate) struct Edit<'a> {
    pub label: &'a str,
    /// The open document, which the edit started from.
    pub before: &'a Arc<Document>,
    /// The document the edit made.
    pub next: &'a Document,
    pub history: &'a History,
    pub held: Held,
    /// The documents the session holds besides these (the saved one and the one it reads as, and the one an undo group began from): never freed by what lets go of a step.
    pub kept: &'a [&'a Arc<Document>],
    /// The model a save reads its file as, which the session keeps beside the drawing (before a save of the drawing it is the document that was opened, which is `clean` as well, and then
    /// held in the list of that). **A model that a save made shares nothing with the drawing**, so it is not let go of to see what an edit takes away (that was a walk of all of it for each
    /// edit after a save): it is counted only when the count is made whole.
    pub baseline: Option<&'a Arc<Document>>,
    /// The redo steps that the running command's snapshot holds (`Session::shadow`): held until the command ends, whatever the history does, so that what letting go of them frees is not
    /// taken from the count now, and the count is marked as one that may be more than what is held.
    pub command: &'a [&'a Arc<Document>],
    /// Whether the edit is one of an undo group: it records no step (the group does), and the document it started from is let go of unless a document of `kept` is it.
    pub grouped: bool,
}

fn elsewhere(kept: &[&Arc<Document>], doc: &Arc<Document>) -> bool {
    kept.iter().any(|k| Arc::ptr_eq(k, doc))
}

/// Takes from `bytes` what letting go of each of `docs` frees (those that no `kept` document is, and that the snapshot of the running command, `command`, does not hold), on top of what
/// `released` has let go of already. Gives how many it did not let go of because the command holds them: they are freed when the command ends and not now.
fn let_go<'a>(
    released: &mut Released,
    bytes: &mut u64,
    kept: &[&Arc<Document>],
    command: &[&Arc<Document>],
    docs: impl Iterator<Item = &'a Arc<Document>>,
) -> usize {
    let mut held = 0usize;
    for doc in docs {
        if elsewhere(kept, doc) {
            continue;
        }
        if elsewhere(command, doc) {
            held = held.saturating_add(1);
        } else {
            *bytes = bytes.saturating_sub(released.release(doc).bytes);
        }
    }
    held
}

/// What recording a step does to the count: the redo steps it ends are let go of, and so are the oldest undo steps past the limit of steps. Gives how many undo steps that is, and whether a
/// step was held by another (the count then has not taken from it what it frees when it is let go of, and may be more than what is held).
fn recording(released: &mut Released, bytes: &mut u64, history: &History, kept: &[&Arc<Document>], command: &[&Arc<Document>]) -> (usize, bool) {
    let mut held = let_go(released, bytes, kept, command, history.redo_documents());
    let forced = history.undo_len().saturating_add(1).saturating_sub(history.limit());
    held = held.saturating_add(let_go(released, bytes, kept, command, history.undo_documents().take(forced)));
    (forced, held > 0)
}

/// The count after the history let go of steps **by its own limits** (the bytes of a direction: [`crate::history::History::take_cut`]): what dropping those documents frees is taken from it,
/// as it is for the steps a plan lets go of, so that a step the history cuts is not a count that is made again whole. `kept` holds the drawing and what a save holds, and `command` what
/// the running command's snapshot holds: a step that one of them is is not freed.
pub(crate) fn credit_cut(held: Held, cut: &[Arc<Document>], kept: &[&Arc<Document>], command: &[&Arc<Document>]) -> Held {
    if cut.is_empty() {
        return held;
    }
    let mut released = Released::new();
    let mut bytes = held.bytes;
    let held_by_another = let_go(&mut released, &mut bytes, kept, command, cut.iter());
    Held { bytes, drifted: held.drifted || held_by_another > 0, slack: held.slack }
}

/// The count after a group has recorded its one step (`before` is the document it began from, `kept` holds the drawing now and what a save holds): the redo steps it ends and
/// the steps past the limit are let go of. Gives the count and how many of the oldest undo steps to let go of.
pub(crate) fn after_group(held: Held, history: &History, kept: &[&Arc<Document>], command: &[&Arc<Document>]) -> (Held, usize) {
    let mut released = Released::new();
    let mut bytes = held.bytes;
    let (drop, held_by_another) = recording(&mut released, &mut bytes, history, kept, command);
    (Held { bytes, drifted: held.drifted || held_by_another, slack: held.slack }, drop)
}

/// What the figure `most` says of the edit `e`: the count to keep and the steps to let go of, or the refusal. Nothing is changed.
pub(crate) fn plan(most: u64, e: &Edit<'_>) -> Result<Plan> {
    // What the edit added: only the allocations it copied.
    let added = grown(e.before, e.next, u64::MAX);
    let mut released = Released::new();
    let mut bytes = e.held.bytes.saturating_add(added.bytes);
    let mut drifted = e.held.drifted;
    let mut slack = e.held.slack.saturating_add(added.doubt_bytes);
    let mut drop_undo = 0usize;
    let mut forced = 0usize;
    if e.grouped {
        // The edit of a group lets go of the document it started from, unless it is the one the group began from or one a save holds.
        let_go(&mut released, &mut bytes, e.kept, e.command, std::iter::once(e.before));
    } else {
        let (n, held_by_another) = recording(&mut released, &mut bytes, e.history, e.kept, e.command);
        drop_undo = n;
        forced = n;
        drifted |= held_by_another;
    }
    if bytes <= most {
        return Ok(Plan { held: Held { bytes, drifted, slack }, drop_undo, forced });
    }
    // Past the figure. A count that may be more than what is held, **and may be so by what passes the figure**, is made again, whole: the documents that are held once the edit is put in.
    if drifted || (slack > 0 && bytes.saturating_sub(slack) <= most) {
        let mut live: Vec<&Document> = vec![e.next];
        if !e.grouped || elsewhere(e.kept, e.before) {
            live.push(e.before);
        }
        live.extend(e.history.undo_documents().skip(drop_undo).map(|d| &**d));
        if e.grouped {
            live.extend(e.history.redo_documents().map(|d| &**d));
        }
        live.extend(e.kept.iter().chain(e.command).chain(e.baseline.iter()).map(|d| &***d));
        bytes = held_bytes_of(live, u64::MAX);
        // What the running command's snapshot holds is counted, and is freed when the command ends: the count is then more than what is held.
        drifted = !e.command.is_empty();
        slack = 0;
    }
    // The oldest undo steps make room, one after another.
    let steps = e.history.undo_len();
    while bytes > most && drop_undo < steps {
        let doc = e.history.undo_documents().nth(drop_undo);
        if let Some(doc) = doc {
            let_go(&mut released, &mut bytes, e.kept, e.command, std::iter::once(doc));
        }
        drop_undo += 1;
    }
    if bytes > most
        && let Some(why) = enlarges(e, added)
    {
        // The drawing alone is past the figure, and the edit makes it larger. (An edit that does not is made: a drawing another program's file made is edited.)
        let words = match why {
            Larger::Structure => {
                "of shapes, sheets and sections, with the undo steps that make room for it let go of: the shapes that share a sheet or a section each get a copy of what the edit changes"
            }
            Larger::Characters => {
                "with the text of the cells, the formulas and the texts of the shapes that it adds (a value is a copy of its text in each cell that has it), the undo steps that make room for it let go of: \
                 a shorter text, or fewer cells that hold it, fits"
            }
        };
        return Err(failed(
            &shown(e.label),
            format!("the edit would make the drawing hold more than {} MiB {words}. Nothing was changed", most >> 20),
        ));
    }
    Ok(Plan { held: Held { bytes, drifted, slack }, drop_undo, forced })
}

/// How many bytes of characters an edit may add to a drawing that is past the figure beyond what it takes away, and still not make it larger (a hundredth of the figure): the strings of
/// the cells change length (a number written with more digits, a unit that was one shared string and is now a string for each cell), and the drawing is not made larger by them. An edit
/// that adds more than this is refused, as one that adds an element is, and a drawing that is edited by edits that each add this much has edits that add it for each: a thousand of
/// them are a thousand times as many characters, which the history that is let go of does not hold.
const CHAR_SLACK: u64 = 16 << 20;

/// What an edit adds that the drawing alone is the larger for.
enum Larger {
    /// Elements and the nodes of maps that it does not take away.
    Structure,
    /// Characters, past what it takes away and [`CHAR_SLACK`].
    Characters,
}

/// How much of what an edit counted as new may be doubtful before the edit is judged by the two drawings counted whole: 4 MiB (a copy of a shape that holds the sheet of a shape of the
/// drawing counts that sheet, and the sheets of a drawing are some kilobytes).
const DOUBT_MOST: u64 = 4 << 20;

/// Whether the edit makes the drawing **alone** larger than it was, and how: it adds structure (elements, nodes) that it does not take away, or characters beyond what it takes away and
/// [`CHAR_SLACK`], the drawing being what `before` and `next` hold and no other document of the session. What it took away is what `before` holds that nothing else does once every
/// other document that can share with it is let go of (a [`Released`] of the steps and the kept ones, then of `before`; **not the model a save made, which shares nothing with it**), so
/// that an allocation that the older steps and the saved drawing also hold is still counted as taken. What an edit counted that may have been counted already (a sheet that a new shape
/// shares with one that is held) is taken from what it added; an edit that counted more than [`DOUBT_MOST`] of it is judged by the two drawings counted whole, which is a walk of the
/// drawing only for a drawing past the figure and an edit that adds a great deal that is shared.
fn enlarges(e: &Edit<'_>, added: Added) -> Option<Larger> {
    if added.doubt_bytes > DOUBT_MOST {
        let (next, before) = (held_bytes_of([e.next], u64::MAX), held_bytes_of([&**e.before], u64::MAX));
        return (next > before.saturating_add(CHAR_SLACK)).then_some(Larger::Structure);
    }
    let mut gone = Released::new();
    let mut seen: Vec<&Arc<Document>> = Vec::new();
    for doc in e.history.undo_documents().chain(e.history.redo_documents()).chain(e.kept.iter().chain(e.command).copied()) {
        if Arc::ptr_eq(doc, e.before) || seen.iter().any(|d| Arc::ptr_eq(d, doc)) {
            continue;
        }
        seen.push(doc);
        gone.release(doc);
    }
    let freed = gone.release(e.before);
    let structure = added.bytes.saturating_sub(added.chars).saturating_sub(added.doubt_bytes);
    if structure > freed.bytes.saturating_sub(freed.chars) {
        Some(Larger::Structure)
    } else if added.chars > freed.chars.saturating_add(CHAR_SLACK) {
        Some(Larger::Characters)
    } else {
        None
    }
}
