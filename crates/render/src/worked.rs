//! What the frames of one document worked out for the cells that hold a formula and no saved value, **kept from one frame to the next**.
//!
//! The answer to such a cell is its formula run on the sheet that asked (`read.rs`), so every shape that follows a style of `THEMEVAL()` cells has an answer of its own for
//! each of them, and a page of thousands of shapes is a quarter of a million answers. A frame that worked them out again at each pan and each zoom did the same work
//! every time, and one that the clock ended threw away what it had done. A [`WorkedCache`] holds them for as long as the document does not change.
//!
//! **The cache is for one document value, and holds it.** [`WorkedCache::new`] takes the `Arc<Document>` the frames are drawn from and keeps it: while the cache is alive
//! nobody can change that document in place (`Arc::make_mut` finds a second holder and copies), and a frame uses the cache only for **that very document**
//! ([`WorkedCache::is_for`] compares the address). An edit makes a new document, and the cache of the old one is not used for it: there is no revision to keep in step and no
//! answer that can be out of date. A caller that draws a changed document makes a cache for it; the one it had goes with the document it was for.
//!
//! **The steps for the cells it works out are the document's, in all** ([`MAX_STEPS_PER_DOCUMENT`]): each frame that uses the cache takes what it spends from them, so a crafted
//! page whose cells the steps do not reach is worked out as far as they go once, and the frames after it (a pan, a zoom) find them spent and work nothing more out,
//! where a budget for each frame would charge the whole of it again at each. A frame without a cache (an export draws once) has the steps of a frame.
//!
//! **It keeps answers and nothing else**, one per cell, in a table of at most [`MAX_KEPT_ANSWERS`] (past it a frame works the rest out as before and keeps them for the
//! frame). An answer is kept only when the formula was worked out: a frame whose steps ran out keeps none of what it did not reach.
//!
//! Nothing here is written to the document: a cell the file saved no value for keeps none, and a save writes none.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use diacraft_doc::{CellAddr, Document, SheetRef};

use crate::read::{UNSAVED_STEPS, Val};

/// The steps for the cells with no saved value that all the frames of one document may spend in all, when they keep their answers in a cache: two frames' worth
/// ([`UNSAVED_STEPS`]), so that a page that needs more than one frame has can be finished by the next, and no more.
pub const MAX_STEPS_PER_DOCUMENT: u64 = UNSAVED_STEPS * 2;

/// The most answers a cache keeps. An answer is a cell's address and a colour, a number or a short text, about 150 bytes: 400,000 of them are 60 MB, and they are the cells
/// of 57,000 shapes that each read seven. A page past it works the rest out in every frame, as it did before there was a cache.
pub const MAX_KEPT_ANSWERS: usize = 400_000;

/// The longest text of an answer that is kept. A colour or a number is no text, and the answer of a cell that is a text is a word or a name.
const MAX_KEPT_TEXT_BYTES: usize = 256;

/// The key of an answer: the sheet that asked and the cell.
pub(crate) type Key = (SheetRef, CellAddr);

/// The answers worked out for one document. See the [module documentation](self).
pub struct WorkedCache {
    doc: Arc<Document>,
    answers: RwLock<HashMap<Key, Val>>,
    /// The answers put in, each counted when it was new.
    evaluated: AtomicU64,
    /// The steps all the frames may spend in all, and what they have.
    steps: u64,
    spent: AtomicU64,
}

impl std::fmt::Debug for WorkedCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkedCache").field("answers", &self.answers()).field("evaluated", &self.evaluated()).finish()
    }
}

impl WorkedCache {
    /// An empty cache for `doc`, which it holds, with [`MAX_STEPS_PER_DOCUMENT`] steps.
    pub fn new(doc: Arc<Document>) -> Arc<WorkedCache> {
        WorkedCache::with_steps(doc, MAX_STEPS_PER_DOCUMENT)
    }

    /// An empty cache for `doc` with `steps` for all its frames to spend on cells with no saved value (a test gives few; none is a document whose cells are all left to
    /// their defaults).
    pub fn with_steps(doc: Arc<Document>, steps: u64) -> Arc<WorkedCache> {
        Arc::new(WorkedCache { doc, answers: RwLock::new(HashMap::new()), evaluated: AtomicU64::new(0), steps, spent: AtomicU64::new(0) })
    }

    /// The steps the frames have spent so far.
    pub fn steps_spent(&self) -> u64 {
        self.spent.load(Ordering::Relaxed).min(self.steps)
    }

    /// The steps the next frames have left.
    pub fn steps_left(&self) -> u64 {
        self.steps.saturating_sub(self.spent.load(Ordering::Relaxed))
    }

    /// Charges the steps a frame spent (a frame says so when it ends: `Cx`'s drop). The count stops at the most there are.
    pub(crate) fn spend(&self, steps: u64) {
        // A load and a store would lose a share's steps to another's; `try_update` retries until its own is in.
        let _ = self.spent.try_update(Ordering::Relaxed, Ordering::Relaxed, |now| Some(now.saturating_add(steps).min(self.steps)));
    }

    /// Whether `doc` is the very document the cache was made for (the same value in memory, not an equal one).
    pub fn is_for(&self, doc: &Document) -> bool {
        std::ptr::eq(Arc::as_ptr(&self.doc), doc)
    }

    /// The document the cache holds.
    pub fn document(&self) -> &Arc<Document> {
        &self.doc
    }

    /// How many answers it keeps now.
    pub fn answers(&self) -> usize {
        self.answers.read().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// How many answers were put in: the cells the frames using it worked out, each once. A frame that finds every cell it reads here adds none.
    pub fn evaluated(&self) -> u64 {
        self.evaluated.load(Ordering::Relaxed)
    }

    /// The answer kept for the cell, if there is one.
    pub(crate) fn get(&self, key: &Key) -> Option<Val> {
        self.answers.read().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    /// Keeps an answer, unless the table is full or the text of the answer is long. A second answer to a cell replaces the first, and is the same one: two shares of a
    /// walk that work the same cell out have the same document to do it on.
    pub(crate) fn put(&self, key: Key, val: Val) {
        if matches!(&val, Val::Text(t) if t.len() > MAX_KEPT_TEXT_BYTES) {
            return;
        }
        let mut answers = self.answers.write().unwrap_or_else(PoisonError::into_inner);
        if answers.len() >= MAX_KEPT_ANSWERS {
            return;
        }
        if answers.insert(key, val).is_none() {
            self.evaluated.fetch_add(1, Ordering::Relaxed);
        }
    }
}
