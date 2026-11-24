//! The session: the document, its history and everything a frontend drives through commands.
//!
//! [`Session::execute`] is the one entry point for every frontend (window, control channel, CLI,
//! MCP). A top-level command is all or nothing: if it fails or panics, the document, its history and
//! the revision are put back as they were and nothing is journaled.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use diacraft_doc::Document;
use diacraft_doc::stamp::Stamp;
use serde_json::Value;

use crate::cmd::{self, CommandInfo, CommandSpec, command_specs, find_command};
use crate::draw::Clipboard;
use crate::error::{EngineError, Result, failed, internal, shown, shown_panic};
use crate::file::OpenFile;
use crate::guard;
use crate::history::History;
use crate::journal::Journal;
use crate::nativefile::StampWork;
use crate::snap::{PageReads, SnapPage, SnapSettings};
use crate::stencil::OpenedStencils;

mod held;
use diacraft_sheetxml::model::{EDIT_MODEL_BYTES, held_bytes_of};
use held::{Edit, Held};

/// Most levels of commands running commands. The registry's own commands never run each other more
/// than a level or two deep; the cap turns a runaway recursion into an error instead of a stack overflow.
const MAX_NESTING: u32 = 8;

/// The open document and its undo history, and the file it was read from when it was opened from one.
#[derive(Clone)]
struct State {
    doc: Arc<Document>,
    history: History,
    /// The file's path and bytes, shared by every snapshot of this state: `None` for a document made by `file.new`, and until
    /// it is first saved. A save replaces it with the file it wrote.
    source: Option<OpenFile>,
    /// The document that the file (or, for a document never saved, the blank document `file.new` made) holds: the drawing is **unsaved** when
    /// the document is not this very allocation. Compared by pointer, so an edit and its undo are clean again, and no pass over the document
    /// is made to know (gaps F2).
    clean: Arc<Document>,
    /// The document as reading `source`'s bytes gives it: the model a save compares the document with, to write what changed and keep the
    /// rest. `None` with no `source`.
    baseline: Option<Arc<Document>>,
    /// The **high-water mark** of shape `ID`s, by page: the highest `ID` the session has given to a shape on the page (a drop, a connector). An
    /// `ID` that has been a shape's is not given again, though the shape was deleted or its drop undone: a formula that named it does
    /// not come to name another shape, and the writer does not meet a new shape where the file had a picture. Taken with the `ID`s in the
    /// page now and in the baseline by [`Session::id_floor`]; it goes with the document, so a command that fails puts it back.
    given_ids: BTreeMap<u32, u32>,
    /// What `edit.copy` and `edit.cut` put aside: the shapes of this document, with the masters they use. It goes with the document, so a
    /// new or opened drawing starts with none (a paste is within one document), and a command that fails puts it back as it was.
    clipboard: Option<Clipboard>,
    /// **What the session holds of the model** ([`held`]): the document, its undo and redo steps and the documents a save keeps, by the allocations they hold between them, each
    /// once, and the characters of the strings. It is kept by what each edit adds and what each step let go of frees, so that no edit counts the drawing whole; it moves with the
    /// document and the history (a failed command puts it back with them), and an undo or a redo leaves it as it is (the documents are the same).
    held: Held,
    /// What the count was when the file was opened: the drawing as the reader made it, which the file's own hold ([`OpenFile::held_bytes`]) holds already, by the reader's price. The
    /// session's hold is what the count has grown by since (edits, steps, the model a save keeps), so that the process counts the model once. `0` for a drawing that no file made.
    opened: u64,
    /// What the model a save keeps (the baseline) holds that the drawing and its steps do not: a second model that shares nothing with them, made by the save and held until the next one.
    /// It is in the count and in the process's ledger (a read of another file must leave room for it), and the figure of an edit is **that much more**: the figure is what the drawing
    /// and its steps may hold, and a drawing is not refused a shape because it was saved. `0` before a save (the baseline is then the document that was opened).
    baseline_extra: u64,
    /// Whether the count above includes the document a pending content stamp keeps (the drawing as it was read from a foreign file, [`crate::file::ContentStamp`]): it is a document the
    /// session holds beside the others, and the stamp lets go of it when it is made, which the count has not been told ([`State::note_read`]).
    read_counted: bool,
}

impl State {
    /// Notes whether the document the stamp keeps is still kept (`kept`): when it was counted and the stamp has been made since, it was let go of and the count has not taken from it what that
    /// freed, so it may be more than what is held, and is made again, whole, when it passes the figure.
    fn note_read(&mut self, kept: bool) {
        if self.read_counted && !kept {
            self.read_counted = false;
            self.held.drifted = true;
        }
    }
}

/// What a save needs of the session: the document, the file it was read from or last saved to, and the model that file reads as.
pub(crate) struct SaveInputs {
    pub doc: Arc<Document>,
    pub source: Option<OpenFile>,
    pub baseline: Option<Arc<Document>>,
}

/// An undo group in progress (a batch is one): its edits are not recorded one by one; the group records one step at the end.
struct Group {
    /// Whether any edit has been made since the group began.
    edited: bool,
    /// The document the group began from: the step the group records, held by the group, and never let go of by an edit of it.
    start: Arc<Document>,
    /// What the session held when the group began: put back with the document when the group is taken back.
    held: Held,
}

/// What a top-level command may change, taken before it runs and put back if it fails.
struct Snapshot {
    state: Option<State>,
    revision: u64,
    stencils: OpenedStencils,
}

/// An engine session: at most one document, with its history, a journal of the commands run, and the
/// revision that moves whenever the document does.
pub struct Session {
    state: Option<State>,
    revision: u64,
    journal: Journal,
    /// Atomic because listing the commands, which can catch a panic, only has `&self`.
    panics_caught: AtomicU64,
    /// How many commands are running, one inside the next: 0 between commands.
    depth: u32,
    group: Option<Group>,
    /// The stencil files opened in this session: not part of the document or its history, and kept when the document is replaced.
    stencils: OpenedStencils,
    /// The groups the refit of an edit left as they were in the command that is running, and how many besides these are not listed: the
    /// command's answer says them. Cleared when a command starts.
    refit_left: Vec<crate::draw::Left>,
    refit_unlisted: usize,
    /// The pages read for the snap, one for each page, by revision (`snap/reads.rs`): not part of the document. An edit moves the revision, and the read is then not found.
    reads: PageReads,
    /// What the model of the open document and its history may hold as an edit leaves it ([`MODEL_MOST`]; a test sets another, to see what is done at the figure).
    model_most: u64,
    /// What the running top-level command put aside to put back if it fails (`run_guarded`). It holds the documents of the history as they were when the command began, so that an edit
    /// that lets go of an undo step to make room lets it go **here too** (a document held by the snapshot is not freed, and the count would credit a free that did not happen).
    shadow: Option<Snapshot>,
    /// **What this session has told the process it holds** (the process-wide count): the count of [`held`], as it was after the last thing that moved it.
    /// Every place that sets [`State::held`] ends in [`Session::publish_held`], which sets the [`crate::memory::Hold`] of the session to it, **less what it was when the file was
    /// opened** ([`State::opened`]: the open file already holds the model it read, [`OpenFile::held_bytes`]).
    published: u64,
    /// The session's hold on the process's ledger: what [`Session::publish_held`] last set, and given back when the session is dropped.
    hold: crate::memory::Hold,
    /// The clock a save reads the time from while it is pinned ([`Session::pin_clock`]); `None` reads the system's. Not part of the document: a command that fails puts back nothing of it.
    clock: Option<i64>,
    /// What the passes that fingerprint the model have cost this session ([`Session::stamp_work`]), counted for the tests that hold the number of passes to one for each foreign open.
    stamp_work: std::sync::Mutex<StampWork>,
    /// The readers of the cells of the drawing's page that `calc` keeps from one edit to the next, so that an edit costs what it touched and not every formula of the page
    /// (`calc_index.rs`). Not part of the document or its history: the commits of an edit use it while the edit runs, and what it holds is in the session's hold
    /// on the process's ledger ([`Session::publish_held`]).
    calc_index: crate::calc_index::KeptIndex,
    /// The name and initials new comments are given (`comment.author`): the session's, not the drawing's.
    comment_author: (String, String),
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    /// A session with no document: `file.new` makes one.
    pub fn new() -> Session {
        Session {
            state: None,
            revision: 0,
            journal: Journal::new(),
            panics_caught: AtomicU64::new(0),
            depth: 0,
            group: None,
            stencils: OpenedStencils::new(),
            refit_left: Vec::new(),
            refit_unlisted: 0,
            reads: PageReads::default(),
            model_most: MODEL_MOST,
            shadow: None,
            published: 0,
            hold: crate::memory::Hold::new(0),
            clock: None,
            stamp_work: std::sync::Mutex::new(StampWork::default()),
            calc_index: crate::calc_index::KeptIndex::default(),
            comment_author: ("Author".to_owned(), "A".to_owned()),
        }
    }

    /// The name and initials a new comment is given (`comment.author`).
    pub fn comment_author(&self) -> (String, String) {
        self.comment_author.clone()
    }

    /// Sets the name and initials new comments are given. A setting of the session: no undo step, and the drawing is not changed.
    pub(crate) fn set_comment_author(&mut self, name: String, initials: String) {
        self.comment_author = (name, initials);
    }

    /// Pins the clock every save reads the time from to `now`, in seconds since 1970 (`None` leaves the system's clock to be read again). The VSDX writer's modified time comes from the
    /// caller, so a test that compares two saves byte for byte pins it (NP12).
    pub fn pin_clock(&mut self, now: Option<i64>) {
        self.clock = now;
    }

    /// The time a save writes into a file that has one: the pinned clock's ([`Session::pin_clock`]), else the system's, in seconds since 1970. `None` if the system's clock is before 1970 or too
    /// far on.
    pub fn now(&self) -> Option<i64> {
        self.clock.or_else(|| {
            let elapsed = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).ok()?;
            i64::try_from(elapsed.as_secs()).ok()
        })
    }

    /// The stamp of the drawing's content **as it was when it was read from a foreign file** (a VSDX one), or as the `.diacraft` file it was read from says it: the design's 4.4. It does not
    /// move with an edit, a save or an undo. `None` for a drawing that never came from a foreign file (a new one, one made from a template), and for one whose content could not be fingerprinted.
    ///
    /// **A VSDX file's stamp is made when it is first asked for** (the pass was a third of the open's time), **and by no save of a VSDX file** (a drawing that is only saved as
    /// VSDX would pay for a stamp nothing reads): by `Session::content_unchanged` once the drawing is not the document it was read as, and by a Save As to `.diacraft`. It is made from **the
    /// document the drawing was read as, which the pending stamp keeps** (a clone of the `Arc` taken at the open, with no pass), so it does not depend on the baseline a save replaces. The
    /// pass fingerprints that document, once, only when it fits beside what the process holds ([`crate::nativefile::stamp_fits`], asked at this moment), and gives `None` when it does not,
    /// which a model writer reads as edited; and the document is let go of when the stamp is made.
    pub fn content_stamp(&self) -> Option<Stamp> {
        let file = self.source()?;
        file.content_stamp_cell().resolve(|read| {
            let (stamp, cost) = crate::nativefile::stamp_of(read, crate::memory::held())?;
            self.note_stamp(cost);
            Some(stamp)
        })
    }

    /// The document the drawing was read as, which a pending content stamp keeps until it is made ([`crate::file::ContentStamp::keep`]); `None` once it is made, and for a drawing that was
    /// not read from a foreign file. A document the session holds beside the drawing and its history, counted with them ([`held`]).
    fn kept_read(&self) -> Option<Arc<Document>> {
        self.source().and_then(|file| file.content_stamp_cell().read_model())
    }

    /// Whether the stamp of the content as it was read is still waiting to be made ([`Session::content_stamp`]).
    pub fn content_stamp_pending(&self) -> bool {
        self.source().is_some_and(|file| file.content_stamp_cell().is_pending())
    }

    /// Whether the content is what it was when it was read from a foreign file: [`crate::nativefile::content_unchanged`] of the stamp the file has and the fingerprint of the content now (**one
    /// pass over the model's encoding**, counted in [`Session::stamp_work`]). `None` when there is no stamp to compare with, or the content cannot be fingerprinted beside what the process
    /// holds, which a model writer reads as edited. **It never asks the dirty flag**, which does not survive a reopen.
    ///
    /// **While the stamp is pending the drawing is the model it was read as when it is the very document the stamp keeps** (`Arc::ptr_eq`, which an edit and its undo keep or restore, and
    /// a save of the drawing as it was read does not move): the answer is `Some(true)` and no pass is made.
    pub fn content_unchanged(&self) -> Option<bool> {
        let state = self.state.as_ref()?;
        if self.kept_read().is_some_and(|read| Arc::ptr_eq(&read, &state.doc)) {
            return Some(true);
        }
        let stored = self.content_stamp()?;
        let now = crate::nativefile::stamp_of(&state.doc, crate::memory::held()).map(|(stamp, cost)| {
            self.note_stamp(cost);
            stamp
        });
        crate::nativefile::content_unchanged(Some(stored), now)
    }

    /// What the passes that fingerprint the model have cost this session: how many, the parts they made and the items of those parts' trees.
    pub fn stamp_work(&self) -> StampWork {
        self.stamp_work.lock().map(|work| *work).unwrap_or_default()
    }

    /// Counts a pass that fingerprinted the model and cost `cost`.
    pub(crate) fn note_stamp(&self, cost: diacraft_sheetxml::model::StampCost) {
        if let Ok(mut work) = self.stamp_work.lock() {
            *work = work.with(cost);
        }
    }

    /// Raises the marks of the identifier ledger to the file's own (the design's 4.6): a `.diacraft` file says, for each page, the highest shape `ID` ever given on it, and an `ID` that has been a
    /// shape's is not given again, though the shape was deleted before the file was saved. **A ledger raises a mark and never lowers one.**
    pub(crate) fn seed_ledger(&mut self, ledger: &diacraft_native::Ledger) {
        if let Some(state) = self.state.as_mut() {
            for (page, mark) in &ledger.0 {
                let slot = state.given_ids.entry(*page).or_insert(0);
                *slot = (*slot).max(*mark);
            }
        }
    }

    /// Runs the command `id`. This is the entry point for every frontend.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(shown(id)))?;
        self.run_spec(spec, params)
    }

    /// Runs `spec`, which need not be a registered command (tests run their own), with `params`.
    ///
    /// The params must be a JSON object (`null` counts as `{}`) whose keys are all named by the
    /// spec's `params` doc, and the spec must be enabled. A top-level call (a command that is not
    /// running inside another) runs under the panic guard and is all or nothing; if it succeeds
    /// and the spec journals, it is added to the journal. A call from inside a command is part of
    /// that command: it is not guarded or journaled of its own.
    pub fn run_spec(&mut self, spec: &CommandSpec, params: &Value) -> Result<Value> {
        if self.depth > 0 {
            let params = self.check(spec, params)?;
            return self.run_nested(spec, &params);
        }
        // The checks run under the guard too: an `enabled` that panics is a bug like any other. They only
        // read, so a panic in them leaves nothing to put back.
        let params = match guard::catch_panic(|| self.check(spec, params)) {
            Ok(checked) => checked?,
            Err(msg) => {
                self.note_panic();
                return Err(EngineError::Internal { cmd: spec.id.to_owned(), msg: shown_panic(&msg) });
            }
        };
        self.refit_left.clear();
        self.refit_unlisted = 0;
        let mut value = self.run_guarded(spec, &params)?;
        self.say_groups_left(&mut value);
        if spec.journal {
            self.record(spec.id, params);
        }
        Ok(value)
    }

    /// Adds to the answer of the command that ran the groups its edits left as they were: `groups_left`, each `{page, id, kind, why}` (`kind` is `group`, or `connector` for one that was not routed again), and
    /// `groups_left_unlisted`, how many more. An answer that is not an object, and a command that left none, say nothing.
    fn say_groups_left(&mut self, value: &mut Value) {
        let (left, unlisted) = (std::mem::take(&mut self.refit_left), std::mem::take(&mut self.refit_unlisted));
        if left.is_empty() && unlisted == 0 {
            return;
        }
        if let Value::Object(map) = value {
            let list: Vec<Value> = left.iter().map(|l| serde_json::json!({ "page": l.page, "id": l.id, "kind": l.kind, "why": l.why })).collect();
            map.insert("groups_left".to_owned(), Value::Array(list));
            if unlisted > 0 {
                map.insert("groups_left_unlisted".to_owned(), Value::from(unlisted));
            }
        }
    }

    /// The params `spec` is run with ([`cmd::checked_params`]), if it is enabled.
    fn check<'a>(&self, spec: &CommandSpec, params: &'a Value) -> Result<Cow<'a, Value>> {
        let params = cmd::checked_params(spec, params)?;
        (spec.enabled)(self).map_err(|reason| EngineError::Disabled { cmd: spec.id.to_owned(), reason })?;
        // A command that changes the drawing does not edit a shape on a locked layer (`cmd/locked.rs`).
        cmd::locked::refuse(self, spec, &params)?;
        Ok(params)
    }

    /// Runs `f` on a mutable copy of the document, as the undo step `label`. The name must follow the
    /// rule for names ([`diacraft_doc::check_text`]); a name that does not is refused before `f` runs.
    ///
    /// On `Ok` the edited document becomes the document, the revision moves, and the step is recorded:
    /// one step for this edit, or none inside an undo group (a batch), which records one for all its
    /// edits. On `Err`, or when `f` panics, nothing changes: the edit is made on a copy.
    ///
    /// The document is not checked as a whole afterwards. An imported document may hold a page name
    /// that would not pass, and an audit per edit would stop every other edit on it (gaps F2); a name
    /// is checked by the write that sets it (`Document::rename_page`, `Document::add_page`), and a
    /// size is capped by the write that grows it.
    ///
    /// Commands make their changes here. Called outside a command, it records the step but adds no
    /// journal entry and has no panic guard.
    pub fn edit<T>(&mut self, label: &str, f: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        self.edit_if(label, |doc| f(doc).map(|value| (value, true)))
    }

    /// [`Session::edit`] for an edit that may turn out to change nothing: `f` says with its value whether it changed the document. When it did
    /// not, the document, the history and the revision stay as they were: no empty undo step, and the drawing is not made unsaved (of
    /// the M3 review: moving a connector whose two ends are both glued moves nothing).
    pub fn edit_if<T>(&mut self, label: &str, f: impl FnOnce(&mut Document) -> Result<(T, bool)>) -> Result<T> {
        check_label(label)?;
        // What an edit may leave of this session's model: the figure less what the rest of the process holds.
        let figure = self.model_most.saturating_sub(self.held_by_the_rest());
        // The document a pending content stamp keeps is held beside the drawing and its history: a document of the count, never freed by what lets go of a step. (A stamp that has been made since
        // the count was made has let go of it: the count may be more than what is held, and says so.)
        let mut read = self.kept_read();
        let state = self.state.as_mut().ok_or_else(|| failed(label, "no document open"))?;
        state.note_read(read.is_some());
        let figure = figure.saturating_add(state.baseline_extra);
        let before = Arc::clone(&state.doc);
        // The edit is made on a copy off to the side, and only a finished one replaces the document. So
        // an error or a panic in `f` leaves the document, the history and the revision exactly as they
        // were, with nothing to put back. The copy shares everything with `before`, each list and each
        // text by one reference count: `f` copies the chunk and the item it writes to.
        let mut next = Document::clone(&before);
        // The commits the edit makes use the session's index of the readers of the page's cells, until the edit is over.
        let _index_in_use = crate::calc_index::InUse::enter(&self.calc_index);
        // The routings the edit makes share one budget of tries, and what they leave is said in the command's answer.
        let routes = crate::draw::EditRoutes::enter();
        // The reads of containers the edit makes (a move's followers and membership, a fit) share one budget of work (`draw/container/page.rs`).
        let _container_work = crate::draw::container::EditWork::enter();
        let (value, changed) = f(&mut next)?;
        if !changed {
            return Ok(value);
        }
        let (left, unlisted) = routes.take_left();
        let room = crate::draw::MAX_LEFT_LISTED.saturating_sub(self.refit_left.len());
        self.refit_unlisted = self.refit_unlisted.saturating_add(unlisted).saturating_add(left.len().saturating_sub(room));
        self.refit_left.extend(left.into_iter().take(room));
        // The groups a changed member is in are the box of their members again, in the step of the edit.
        // A group that cannot be fitted is left and said, whether or not the file holds it (the `.odg` writer writes a group whose frame is not the box of its
        // members as its members and says so in a note, so the edit is no longer refused for it).
        let refit = crate::draw::refit_groups(&before, &mut next).map_err(|problem| problem.into_engine(label))?;
        let room = crate::draw::MAX_LEFT_LISTED.saturating_sub(self.refit_left.len());
        self.refit_unlisted = self.refit_unlisted.saturating_add(refit.unlisted).saturating_add(refit.left.len().saturating_sub(room));
        self.refit_left.extend(refit.left.into_iter().take(room));
        // What the session holds, with the edit: the count is kept, and the edit adds what it copied. Past the figure the oldest undo steps make room, and an edit that cannot be
        // made to fit is refused with nothing changed ([`held::plan`]).
        let plan = match self.plan_edit(label, figure, &before, &next, read.as_ref()) {
            Ok(plan) => plan,
            Err(refused) if read.is_some() => {
                // **A refusal that the document a pending stamp keeps may be the cause of** (the drawing and its history fit, and the document that the drawing was read as, which nothing but
                // the stamp holds, is what passes the figure): the stamp is made now, which lets go of the document, and the edit is judged again without it. A drawing is not refused an edit
                // because it was read from a foreign file and saved. A stamp that does not fit beside what the process holds is none for good, which a model writer reads as edited.
                read = None;
                let _ = self.content_stamp();
                if let Some(state) = self.state.as_mut() {
                    state.note_read(false);
                }
                self.plan_edit(label, figure, &before, &next, None).map_err(|_| refused)?
            }
            Err(refused) => return Err(refused),
        };
        let state = self.state.as_mut().ok_or_else(|| failed(label, "no document open"))?;
        state.read_counted = state.read_counted && read.is_some();
        state.doc = Arc::new(next);
        state.held = plan.held;
        // What the history's own limit of bytes lets go of beyond what the plan did (the oldest undo steps): taken from the count once the command's snapshot has let go of them too.
        let mut cut: Vec<Arc<Document>> = Vec::new();
        match self.group.as_mut() {
            Some(group) => {
                group.edited = true;
                state.history.drop_oldest_undo(plan.drop_undo);
            }
            None => {
                state.history.drop_oldest_undo(plan.drop_undo);
                let bytes = crate::odgfile::hold::unshared_bytes(&before, &state.doc);
                state.history.record(Arc::from(label), before, bytes);
                cut = state.history.take_cut();
            }
        }
        if plan.drop_undo > plan.forced {
            // Steps were let go of to make room for the model: an undo that finds none says so.
            if let Some(state) = self.state.as_mut() {
                state.history.mark_drawing_cut();
            }
        }
        Session::let_go_in_shadow(&mut self.shadow, plan.drop_undo);
        if let Some(state) = self.state.as_mut() {
            Session::credit_history_cut(state, None, &mut self.shadow, cut, true);
        }
        self.publish_held();
        self.revision = self.revision.saturating_add(1);
        Ok(value)
    }

    /// Runs `f` on a copy of the document and, when it says it changed the document, puts the copy in **as a change that is no undo step**: the drawing's **View settings**, which
    /// are the window settings (`crate::view`, `window.settings.set`) and the AutoConnect box (`autoconnect.set`). The owner's ruling (2026-11-22, "Unsaved, not undo") and the
    /// Turning a View setting marks the drawing changed, so a save keeps it and closing asks, and it is not a step of the history, so
    /// Ctrl+Z goes on undoing the drawing's edits. So, against [`Session::edit_if`]:
    /// - **no step is recorded**, and the redo steps stay. Undo and redo carry what the drawing holds of these settings across ([`Session::step_history`],
    ///   [`crate::view::carry_settings`]), so they leave the settings alone;
    /// - **no revision moves**: the revision is what a frontend keeps the page's picture and the snap's reads for, and no setting of these is part of what they are made of (the grid
    ///   and the rulers are overlays of the window, and the AutoConnect box is read off the document when a frame is drawn), so the page is neither drawn again nor read again for a
    ///   box turned. The Snap & Glue options are not of these: they decide which targets the snap reads, so [`Session::edit_snap_setting`] moves the revision;
    /// - **the drawing is changed**: the document is another allocation than the clean one, so [`Session::is_dirty`] is true (the title's mark, the close question, `file.status`);
    /// - no calculation runs and no group is refitted (no cell is written), and no figure is asked of the model's count: the count weighs no part of a package's rows
    ///   ([`diacraft_sheetxml::model::held_bytes_of`]), the windows part among them, nor the data of DiaCraft's own and the words of `DocumentSettings`, so what the edit adds to it
    ///   is nothing.
    ///
    /// Inside an undo group the copy is put in and the group is **not** marked edited: a batch of nothing but settings records no step, and one that fails takes the setting back with the
    /// rest. On `Err`, or when `f` panics, nothing changes: the change is made on a copy. A value `f` says changed nothing leaves the document as it was.
    pub(crate) fn edit_view<T>(&mut self, cmd: &str, f: impl FnOnce(&mut Document) -> Result<(T, bool)>) -> Result<T> {
        self.put_setting(cmd, false, f)
    }

    /// [`Session::edit_view`] for **the Snap & Glue options** (`snap.set`: the switches, the options of the words of `DocumentSettings`, Dynamic Grid, and the snap strengths):
    /// no undo step and the drawing changed, as for a View setting, **but the revision moves**. A Snap & Glue option decides which targets a query reads,
    /// so what a frontend kept for the revision (the settings it read, the snap's reads of the page and a preview that was snapped by them) is no longer the document's: the
    /// revision is the one number it asks, and a toggle is rare. Undo and redo carry these options across like the rest ([`crate::view::carry_settings`]).
    pub(crate) fn edit_snap_setting<T>(&mut self, cmd: &str, f: impl FnOnce(&mut Document) -> Result<(T, bool)>) -> Result<T> {
        self.put_setting(cmd, true, f)
    }

    /// Puts in the change `f` made to a copy of the document, as no undo step ([`Session::edit_view`]), moving the revision when `moves_revision`.
    fn put_setting<T>(&mut self, cmd: &str, moves_revision: bool, f: impl FnOnce(&mut Document) -> Result<(T, bool)>) -> Result<T> {
        let state = self.state.as_mut().ok_or_else(|| failed(cmd, "no document open"))?;
        let mut next = Document::clone(&state.doc);
        let (value, changed) = f(&mut next)?;
        if changed {
            state.doc = Arc::new(next);
            if moves_revision {
                self.revision = self.revision.saturating_add(1);
            }
        }
        Ok(value)
    }

    /// What the figure says of the edit that made `next` from `before` (`before` is the open document), **judged with the document a pending stamp keeps** (`read`, which is part of what
    /// the session holds and is not freed by what lets go of a step): the count to keep and the steps to let go of, or the refusal ([`held::plan`]). Nothing is changed.
    fn plan_edit(&self, label: &str, figure: u64, before: &Arc<Document>, next: &Document, read: Option<&Arc<Document>>) -> Result<held::Plan> {
        let state = self.state.as_ref().ok_or_else(|| failed(label, "no document open"))?;
        let mut kept: Vec<&Arc<Document>> = vec![&state.clean];
        kept.extend(self.group.as_ref().map(|g| &g.start));
        kept.extend(read);
        // The redo steps that the command's snapshot holds are held until it ends, whatever the history does.
        let command: Vec<&Arc<Document>> =
            self.shadow.as_ref().and_then(|s| s.state.as_ref()).into_iter().flat_map(|s| s.history.redo_documents()).collect();
        let edit = Edit {
            label,
            before,
            next,
            history: &state.history,
            held: state.held,
            kept: &kept,
            baseline: state.baseline.as_ref(),
            command: &command,
            grouped: self.group.is_some(),
        };
        held::plan(figure, &edit)
    }

    /// **What the session holds of the model**, at the least: the open document, its undo and redo steps and the documents a save keeps, by the allocations they hold between
    /// them, each once, and the characters of the strings. It is the count that guards an edit ([`Session::edit_if`]), kept by what each edit adds and what
    /// each step let go of frees, so it costs nothing to ask; it may be more than [`Session::model_held_exact`] says when an edit has met an allocation twice, and is made
    /// again, whole, when it passes the figure an edit may leave. `None` with no document.
    pub fn model_held(&self) -> Option<u64> {
        self.state.as_ref().map(|s| s.held.bytes)
    }

    /// What the session keeps of the readers of the drawing's page for the next edit, as accounted (`calc_index.rs`): `0` for a page with no formula that reads beyond its
    /// sheet. It is in the session's hold on the process's ledger, beside [`Session::published_held`] and not in it.
    pub fn calc_index_held(&self) -> u64 {
        self.calc_index.held_bytes()
    }

    /// What the session has told the process that it holds: [`Session::model_held`] as it was after the last edit, undo, redo, group, failed command, open or save, `0` with no
    /// document. The tests hold it to the count after every one of them; the process-wide handle of reads the same figure.
    pub fn published_held(&self) -> u64 {
        self.published
    }

    /// **Tells the process what the session holds**: after anything that moved the count (an edit, an undo, a redo, a group, a failed command put back, an open, a save), the
    /// session's [`crate::memory::Hold`] is the count **less what it was when the file was opened and less the model a save keeps** (`State::opened` and `State::baseline_extra`: the open
    /// file holds the model it read, and after a save the file is the one the save wrote, whose model the read-back made: the process counts each once). A read of a file, in
    /// this session or another, is then given what the drawing and its history leave of its budget ([`crate::memory::read_limits`]), and is refused only where it was before this count: the
    /// model a save keeps is in **this** session's figure for an edit and not in the process's ledger for a read.
    fn publish_held(&mut self) {
        self.published = self.state.as_ref().map_or(0, |s| s.held.bytes);
        let (opened, kept_by_a_save) = self.state.as_ref().map_or((0, 0), |s| (s.opened, s.baseline_extra));
        // The readers of the page's cells that `calc` keeps for the next edit are held by the session as well, beside the model.
        let index = self.calc_index.held_bytes();
        self.hold.set(self.published.saturating_sub(opened).saturating_sub(kept_by_a_save).saturating_add(index));
    }

    /// What the rest of the process holds besides this session: the process's total ([`crate::memory::held`]) less this session's hold, its file's and its stencils' (a stencil is part of this session: it was never part of the figure). The edit guard keeps the
    /// drawing and its history within the figure **less what the others hold** (another session's drawing, an open stencil, a read that is being made).
    fn held_by_the_rest(&self) -> u64 {
        let own = self.hold.bytes().saturating_add(self.source().map_or(0, OpenFile::held_bytes)).saturating_add(self.stencils.memory_bytes());
        crate::memory::held().saturating_sub(own)
    }

    /// Whether [`Session::model_held`] may be more than what is held: an addition that may have been met twice, and not yet counted again. `None` with no document.
    pub fn model_held_drifted(&self) -> Option<bool> {
        let read_let_go = self.state.as_ref().is_some_and(|s| s.read_counted) && self.kept_read().is_none();
        self.state.as_ref().map(|s| s.held.may_be_more() || read_let_go)
    }

    /// The same count, made by walking every document the session holds, whole. A walk of the drawing and of the steps' changes: the tests hold the kept count to it, and a caller that
    /// needs the exact figure asks it. `None` with no document.
    pub fn model_held_exact(&self) -> Option<u64> {
        let state = self.state.as_ref()?;
        let read = self.kept_read();
        let mut docs: Vec<&Document> = vec![&*state.doc, &*state.clean];
        docs.extend(state.baseline.as_deref());
        docs.extend(read.as_deref());
        docs.extend(state.history.undo_documents().chain(state.history.redo_documents()).map(|d| &**d));
        docs.extend(self.group.as_ref().map(|g| &*g.start));
        Some(held_bytes_of(docs, u64::MAX))
    }

    /// The open document.
    pub fn document(&self) -> Option<&Document> {
        self.state.as_ref().map(|s| &*s.doc)
    }

    /// The file the open document was read from: its path and bytes as they were read. `None` when there is no document, or it was made
    /// by `file.new`. Undo and redo do not change it: they move the document, and the file is what it was opened from. **A new drawing keeps no
    /// file's bytes** (NP10): a drawing made from a template, a built-in one or a template file, has `None` too, and its model carries
    /// everything the template held. A caller that wants the drawing's file asks [`Session::path`].
    pub fn source(&self) -> Option<&OpenFile> {
        self.state.as_ref().and_then(|s| s.source.as_ref())
    }

    /// The path of the file the document was opened from or last saved to; `None` for a drawing that has never been saved, and for one made from a
    /// template (it has no file: `file.save` has nowhere to write, and `file.save_as` is asked for a path).
    pub fn path(&self) -> Option<&std::path::Path> {
        self.source().filter(|f| f.is_named()).map(OpenFile::path)
    }

    /// Whether `file.save_as` takes a name that ends in `extension` (in any case, without the dot) for this drawing: the save-path table's answer, asked without saving. A
    /// frontend that lists the types a Save dialog offers asks this for each type, so the engine alone says which saves across formats are written. `false` with no document.
    pub fn save_as_allows(&self, extension: &str) -> bool {
        self.document().is_some() && crate::cmd::save_as_allows(self.source(), extension)
    }

    /// Whether the drawing has changes that are not saved: the document is not the one the file holds. A frontend shows this in the title and
    /// asks before it discards the document. `false` with no document. A drawing from `file.new` is not unsaved until it is edited; undo back
    /// to the saved state makes it clean again. **A View setting turned makes it unsaved** though it is no undo step ([`Session::edit_view`], [`Session::edit_snap_setting`]: the window
    /// settings, the AutoConnect box and the Snap & Glue options), and an undo back to the saved state is clean only when the settings are the saved ones too.
    pub fn is_dirty(&self) -> bool {
        self.state.as_ref().is_some_and(|s| !Arc::ptr_eq(&s.doc, &s.clean))
    }

    /// The document that a save writes, the file it was read from or last saved to, and the model that file reads as. `None` with no document.
    pub(crate) fn save_inputs(&self) -> Option<SaveInputs> {
        self.state.as_ref().map(|s| SaveInputs { doc: Arc::clone(&s.doc), source: s.source.clone(), baseline: s.baseline.clone() })
    }

    /// **A save across to VSDX wrote a file whose model is `adopted`, and the drawing becomes it** (`across`: "the model after a save across"): the file is the drawing's own from
    /// now on, and the next save splices into it and compares the drawing with what the file reads as, so a drawing that is still the one that was saved (an `.odg`'s frame where the file holds an
    /// embedding) would be refused. The caller says so only when the file reads as everything the drawing held. It is **no undo step**: the steps the history holds are of the drawing before.
    pub(crate) fn adopt_saved(&mut self, adopted: Document, source: OpenFile) {
        let adopted = Arc::new(adopted);
        if let Some(state) = self.state.as_mut() {
            state.doc = Arc::clone(&adopted);
        }
        self.mark_saved(&adopted, source, Document::clone(&adopted));
    }

    /// **A save across to VSDX left something out of the file, and the drawing becomes what the file reads as with what was left out put back on it** (`across::dropped`): `adopted`
    /// is that drawing, and `baseline` what the next save compares it with (the drawing as the writer is given it, which is not what the file reads as). It is **no undo step**.
    pub(crate) fn adopt_saved_with(&mut self, adopted: Document, source: OpenFile, baseline: Document) {
        let adopted = Arc::new(adopted);
        if let Some(state) = self.state.as_mut() {
            state.doc = Arc::clone(&adopted);
        }
        self.mark_saved(&adopted, source, baseline);
    }

    /// A save wrote `doc` (the document as it was when the save began) to `source`, and that file reads as `baseline`: the drawing is clean if
    /// it is still `doc`, and the next save patches `source`'s bytes.
    ///
    /// The old baseline's highest `ID` on each page is **kept**: it goes into the session's high-water mark before the
    /// baseline is replaced. A shape of the file that was deleted before the save is not in the new baseline, so without this its `ID` would be given
    /// again, and a `Sheet.<id>!` that the delete left dangling would name the new shape.
    pub(crate) fn mark_saved(&mut self, doc: &Arc<Document>, source: OpenFile, baseline: Document) {
        // **No pass is made here** (the blocker of the re-review): a stamp that is still pending keeps the document the drawing was read as, so the baseline that is about to
        // be replaced is not the thing the stamp depends on, and a save of a VSDX file that nothing asks the stamp of pays for none.
        if let Some(state) = self.state.as_mut() {
            if let Some(old) = state.baseline.clone() {
                for page in old.pages.iter() {
                    if let Some((highest, _)) = crate::draw::page_stats(&old, page.id) {
                        let mark = state.given_ids.entry(page.id).or_insert(0);
                        *mark = (*mark).max(highest);
                    }
                }
            }
            // The model of the drawing is the one that was read, and what it took is what the next read leaves room for: the file that is written is its bytes only.
            let model = state.source.as_ref().map_or(0, OpenFile::model_bytes);
            // The stamp of the content as it was read from a foreign file is the drawing's, not the file's: **a save does not move it** (the design's 4.4), so the file written has it still.
            let carried = state.source.as_ref().map(|old| old.content_stamp_cell().clone());
            let source = match carried {
                Some(stamp) if source.content_stamp_cell().value().is_none() && !source.content_stamp_cell().is_pending() => {
                    source.with_content_stamp_cell(stamp)
                }
                _ => source,
            };
            state.source = Some(if source.model_bytes() == 0 { source.with_model_bytes(model) } else { source });
            state.baseline = Some(Arc::new(baseline));
            state.clean = Arc::clone(doc);
        }
        // The model a save reads its file as is held beside the drawing (a model of its own, which shares nothing with it), and the one that was held before it is let go of: the
        // count is made again, whole, at the cost of the save.
        self.recount();
    }

    /// Takes from the count what the steps the history let go of by its own limits free ([`held::credit_cut`]), and lets those documents go. `group_start` is the document an undo
    /// group began from. **The running command's snapshot holds the steps too** (it keeps the history to put back if the command fails): when the steps are the oldest undo
    /// steps (`oldest_undo`: an edit that recorded a step) the snapshot lets go of them first, as it does for what a plan lets go of, so that what they free is freed now; in any other
    /// case (an undo or a redo that cut a step of the other direction, a change of the limits) it holds them until the command ends, and the count says it may be more than what is held.
    fn credit_history_cut(
        state: &mut State,
        group_start: Option<&Arc<Document>>,
        shadow: &mut Option<Snapshot>,
        cut: Vec<Arc<Document>>,
        oldest_undo: bool,
    ) {
        if cut.is_empty() {
            return;
        }
        if oldest_undo {
            Session::let_go_in_shadow(shadow, cut.len());
        }
        let mut kept: Vec<&Arc<Document>> = vec![&state.clean, &state.doc];
        kept.extend(group_start);
        let in_command = shadow.as_ref().and_then(|s| s.state.as_ref());
        let command: Vec<&Arc<Document>> = if oldest_undo {
            in_command.into_iter().flat_map(|s| s.history.redo_documents()).collect()
        } else {
            in_command.into_iter().flat_map(|s| s.history.undo_documents().chain(s.history.redo_documents())).collect()
        };
        let held = held::credit_cut(state.held, &cut, &kept, &command);
        state.held = held;
    }

    /// Lets go of the oldest `n` undo steps in what the running command put aside, as the session did: a step that the snapshot still held would not be freed, and what the count
    /// credits for it would not be. **The steps are then gone if the command fails** (the drawing and its other steps are put back, and the count with them, which may be more than what
    /// is then held: it is marked so, and made again whole when it passes the figure).
    fn let_go_in_shadow(shadow: &mut Option<Snapshot>, n: usize) {
        if n == 0 {
            return;
        }
        if let Some(state) = shadow.as_mut().and_then(|s| s.state.as_mut()) {
            state.history.drop_oldest_undo(n);
            state.held.drifted = true;
        }
    }

    /// Makes the count of what the session holds again, whole.
    fn recount(&mut self) {
        let Some(bytes) = self.model_held_exact() else { return };
        let read = self.kept_read();
        // What the model a save keeps adds to the count: the count with it, less the count without it (it shares nothing with the rest). **The document a pending stamp keeps is of the
        // count without it**: it is not the model a save keeps, and the process counts what it holds beside the file's own model.
        let without = self.state.as_ref().map(|state| {
            let mut docs: Vec<&Document> = vec![&*state.doc, &*state.clean];
            docs.extend(read.as_deref());
            docs.extend(state.history.undo_documents().chain(state.history.redo_documents()).map(|d| &**d));
            held_bytes_of(docs, u64::MAX)
        });
        if let Some(state) = self.state.as_mut() {
            state.read_counted = read.is_some();
            state.held = Held { bytes, drifted: false, slack: 0 };
            state.baseline_extra = without.map_or(0, |without| bytes.saturating_sub(without));
        }
        self.publish_held();
    }

    /// The `ID` a new shape on `page` must be above (of the M3 review): the highest the session has given on the page, and the highest
    /// the page holds in the file the session saved from or last saved to (its baseline). The page's own highest `ID` is taken by the command
    /// that makes the shape. `0` when there is none.
    pub fn id_floor(&self, page: u32) -> u32 {
        let Some(state) = self.state.as_ref() else { return 0 };
        let given = state.given_ids.get(&page).copied().unwrap_or(0);
        let saved = state.baseline.as_deref().and_then(|baseline| crate::draw::page_stats(baseline, page)).map_or(0, |(highest, _)| highest);
        given.max(saved)
    }

    /// Notes that shape `id` was given on `page`: no later shape gets it ([`Session::id_floor`]).
    pub fn note_id(&mut self, page: u32, id: u32) {
        if let Some(state) = self.state.as_mut() {
            let mark = state.given_ids.entry(page).or_insert(0);
            *mark = (*mark).max(id);
        }
    }

    /// What the last `edit.copy` or `edit.cut` put aside, for a paste into this document. `None` before one, and after a new or opened
    /// drawing: the clipboard goes with the document it was taken from.
    pub fn clipboard(&self) -> Option<&Clipboard> {
        self.state.as_ref().and_then(|s| s.clipboard.as_ref())
    }

    /// Puts `clipboard` aside (or empties it). Not an undo step: the document is not changed.
    pub(crate) fn set_clipboard(&mut self, clipboard: Option<Clipboard>) {
        if let Some(state) = self.state.as_mut() {
            state.clipboard = clipboard;
        }
    }

    /// The stencil files opened in this session (`stencil.open`), in the order they were opened.
    pub fn stencils(&self) -> &OpenedStencils {
        &self.stencils
    }

    /// What is held of the memory that a read of a file must leave room for: **the process's total** ([`crate::memory::held`]: every open drawing
    /// and stencil of every session, and whatever else holds a [`crate::memory::Hold`]), and, beside it, what this session's own drawing (its file's bytes and the model it read as) and open
    /// stencils (their models) hold. **The memory a read may use is [`diacraft_vsdx::MAX_MEMORY`] for the whole process, and each read is given what is left of it**: the window reads a file in a
    /// session of its own while its own session holds the drawing, and the command line reads a file three times. The drawing has no figure of what it has grown by since it was read.
    pub fn held_memory(&self) -> HeldMemory {
        HeldMemory { drawing: self.source().map_or(0, OpenFile::held_bytes), stencils: self.stencils.memory_bytes(), process: crate::memory::held() }
    }

    /// The stencils, to change: `stencil.open` and `stencil.close`. A command that fails puts them back with the rest of the session.
    pub(crate) fn stencils_mut(&mut self) -> &mut OpenedStencils {
        &mut self.stencils
    }

    /// Page `page` of the open document as a snap reads it, **kept for the revision** it was read at: the window makes its index of the shapes and the
    /// targets on this very read, off its own thread, and the commands that snap find it with [`Session::kept_snap_page`]. A read already kept for the revision, the page and the
    /// document's snap settings is given back; an edit has moved the revision, and the old one is dropped. **A read made while a command runs is not kept** (a command that fails
    /// puts the revision back, and another document can come to the revision the failed one reached). `None` with no document, or no such page.
    pub fn snap_page(&self, page: u32) -> Option<Arc<SnapPage>> {
        let doc = self.document()?;
        let settings = SnapSettings::of(doc);
        if let Some(kept) = self.reads.get(self.revision, page, &settings) {
            return Some(kept);
        }
        let read = Arc::new(SnapPage::read(doc, page)?);
        if self.depth == 0 && self.group.is_none() {
            self.reads.keep(self.revision, page, settings, Arc::clone(&read));
        }
        Some(read)
    }

    /// The read of `page` kept for the document as it is, if there is one, and none otherwise (a cold page: the caller reads as it did before the cache). The commands that snap
    /// look here first, and count what they found ([`crate::snap::counts`]).
    pub fn kept_snap_page(&self, page: u32) -> Option<Arc<SnapPage>> {
        let doc = self.document()?;
        let found = self.reads.get(self.revision, page, &SnapSettings::of(doc));
        crate::snap::reads::count(|c| if found.is_some() { c.kept_found += 1 } else { c.kept_missing += 1 });
        found
    }

    /// The reads this session keeps for the snap, for the tests that look at what they hold.
    pub fn snap_reads(&self) -> &PageReads {
        &self.reads
    }

    /// The open document's undo history.
    pub fn history(&self) -> Option<&History> {
        self.state.as_ref().map(|s| &s.history)
    }

    /// Gives the open document's history other limits: `steps` and `bytes` in each direction ([`History::set_limits`]). A frontend that has less memory than
    /// the default is made for lowers them; a new or opened document starts with the defaults again.
    pub fn set_history_limits(&mut self, steps: usize, bytes: u64) {
        if let Some(state) = self.state.as_mut() {
            state.history.set_limits(steps, bytes);
            let cut = state.history.take_cut();
            Session::credit_history_cut(state, self.group.as_ref().map(|g| &g.start), &mut self.shadow, cut, false);
        }
    }

    /// A number that moves whenever the document does: a frontend redraws when it differs from the one
    /// it drew. A command that fails or only reads leaves it alone, and **so does one that turns a window setting or the AutoConnect box** (`window.settings.set`, `autoconnect.set`,
    /// [`Session::edit_view`]): a setting of these is no part of what the page is drawn or read from, and the drawing is changed all the same ([`Session::is_dirty`]). **A Snap & Glue
    /// option moves it** though it is no undo step (`snap.set`, [`Session::edit_snap_setting`]): it decides which targets the snap reads.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Every command with whether it can run now and, if not, why.
    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|spec| spec.info(self)).collect()
    }

    /// The top-level commands that succeeded, oldest first, with their params: the newest
    /// [`crate::JOURNAL_LIMIT`] of them. Queries are not journaled, nor are the commands a command runs.
    pub fn journal(&self) -> &[(String, Value)] {
        self.journal.entries()
    }

    /// The journal itself, with what it holds in memory.
    pub fn journal_log(&self) -> &Journal {
        &self.journal
    }

    /// How many panics the guard has caught. Each is a bug.
    pub fn panics_caught(&self) -> u64 {
        self.panics_caught.load(Ordering::Relaxed)
    }

    /// Counts a panic the guard caught.
    pub(crate) fn note_panic(&self) {
        self.panics_caught.fetch_add(1, Ordering::Relaxed);
    }

    /// A command that runs from inside another: part of it, so not guarded or journaled.
    fn run_nested(&mut self, spec: &CommandSpec, params: &Value) -> Result<Value> {
        if self.depth >= MAX_NESTING {
            return Err(failed(spec.id, format!("commands nest too deeply (at most {MAX_NESTING} levels)")));
        }
        self.depth += 1;
        let result = (spec.run)(self, params);
        self.depth = self.depth.saturating_sub(1);
        result
    }

    /// A top-level command: a panic is caught and an error from it undoes what it did, so the
    /// session is as it was before, and a panic is reported as [`EngineError::Internal`].
    fn run_guarded(&mut self, spec: &CommandSpec, params: &Value) -> Result<Value> {
        self.shadow = Some(Snapshot { state: self.state.clone(), revision: self.revision, stencils: self.stencils.clone() });
        self.depth = 1;
        let outcome = guard::catch_panic(|| (spec.run)(self, params));
        let snapshot = self.shadow.take();
        // Whether the command ended or unwound, no command is running now. (No undo group is open
        // either: only a running command opens one, and `grouped` closes it on every way out.)
        self.depth = 0;
        match outcome {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => {
                if let Some(snapshot) = snapshot {
                    self.restore(snapshot);
                }
                Err(e)
            }
            Err(msg) => {
                self.note_panic();
                if let Some(snapshot) = snapshot {
                    self.restore(snapshot);
                }
                Err(EngineError::Internal { cmd: spec.id.to_owned(), msg: shown_panic(&msg) })
            }
        }
    }

    /// Puts back what a failed command may have changed.
    fn restore(&mut self, snapshot: Snapshot) {
        self.state = snapshot.state;
        self.revision = snapshot.revision;
        self.stencils = snapshot.stencils;
        self.publish_held();
    }

    fn record(&mut self, id: &str, params: Cow<'_, Value>) {
        self.journal.push(id, params.into_owned());
    }

    /// Takes back (`undo`) or puts again the newest step of the history → its name.
    pub(crate) fn step_history(&mut self, cmd: &str, undo: bool) -> Result<Arc<str>> {
        if self.group.is_some() {
            return Err(failed(cmd, format!("`{cmd}` cannot run inside a batch or other undo group")));
        }
        let state = self.state.as_mut().ok_or_else(|| failed(cmd, "no document open"))?;
        let current = Arc::clone(&state.doc);
        // The View settings are no step of the history: the document gone back to or on to is made with the windows part, the AutoConnect box and the Snap &
        // Glue options the drawing holds now.
        let settings = crate::view::HeldSettings::of(&current);
        // What the step that takes the place of the one taken holds: the version being left, less what the one gone back to shares with it.
        let target = if undo { state.history.next_undo_document() } else { state.history.next_redo_document() };
        let bytes = target.map_or(0, |target| crate::odgfile::hold::unshared_bytes(&current, target));
        let stepped = if undo { state.history.undo(current, bytes) } else { state.history.redo(current, bytes) };
        let (label, doc) = stepped.ok_or_else(|| failed(cmd, if undo { state.history.undo_refusal() } else { state.history.redo_refusal() }))?;
        state.doc = crate::view::carry_settings(doc, &settings);
        // The history's limit of bytes may have let a step go as it took the other direction's: what that frees is taken from the count.
        let cut = state.history.take_cut();
        Session::credit_history_cut(state, None, &mut self.shadow, cut, false);
        self.revision = self.revision.saturating_add(1);
        // The documents the session holds are the same after an undo and a redo, and the count with them: told to the process all the same, at each of them.
        self.publish_held();
        Ok(label)
    }

    /// Makes `doc` the document, with a fresh history. The old document is gone.
    pub(crate) fn replace_document(&mut self, cmd: &str, doc: Document) -> Result<()> {
        if self.group.is_some() {
            return Err(failed(cmd, format!("`{cmd}` cannot run inside a batch or other undo group")));
        }
        let doc = Arc::new(doc);
        // What `calc` kept of the readers of the drawing that is gone is of no use to this one.
        self.calc_index.clear();
        // The drawing is counted whole once, as it is opened: no edit after it counts the drawing.
        let held = Held::of([&*doc]);
        self.state = Some(State {
            doc: Arc::clone(&doc),
            history: History::default(),
            source: None,
            clean: doc,
            baseline: None,
            given_ids: BTreeMap::new(),
            clipboard: None,
            held,
            opened: 0,
            baseline_extra: 0,
            read_counted: false,
        });
        self.publish_held();
        self.revision = self.revision.saturating_add(1);
        // The reads kept for the snap are of the drawing that is gone: the revision has moved, so none would be found, but each holds a page's index and targets (megabytes for a
        // page of thousands of shapes), and nothing would drop them until another read took their page (of the re-review).
        self.reads.clear();
        Ok(())
    }

    /// As [`Session::replace_document`], for a document read from `source`: the session keeps the file's path and bytes until the document is
    /// replaced, so that a writer can take any part it does not edit from them.
    pub(crate) fn replace_document_from(&mut self, cmd: &str, doc: Document, source: OpenFile) -> Result<()> {
        self.replace_document(cmd, doc)?;
        if let Some(state) = self.state.as_mut() {
            state.baseline = Some(Arc::clone(&state.doc));
            // A stamp that is pending keeps the document the drawing is read as: the same allocation, which the session counts already (it is the drawing, and the baseline, and the clean one).
            // It is a document the session holds from here on, until the stamp is made.
            if source.content_stamp_cell().is_pending() {
                source.content_stamp_cell().keep(Arc::clone(&state.doc));
                state.read_counted = true;
            }
            state.source = Some(source);
            state.opened = state.held.bytes;
        }
        self.publish_held();
        Ok(())
    }

    /// The baseline of a drawing just opened from its file is `read`, the document as the file holds it, where the open changed the one the drawing was given: it broke a loop of
    /// containers the file holds. A save into the file then writes what the open repaired, as it writes an edit (the splice writes the
    /// pages that differ from the baseline, and no other), and the drawing is clean all the same: the open is no edit and no step of the history. `read` shares every page the open left
    /// alone with the drawing.
    pub(crate) fn set_read_baseline(&mut self, read: Document) {
        if let Some(state) = self.state.as_mut() {
            state.baseline = Some(Arc::new(read));
        }
        self.publish_held();
    }

    /// Runs `f` as an undo group: the edits it makes are one undo step named `label` (which follows the
    /// rule for names, as for [`Session::edit`]), and if it fails the document and the revision are as
    /// they were. This is how a batch becomes one step. Groups do not nest, and undo, redo and a new
    /// document are refused inside one: they do not fit in a single step.
    ///
    /// Only a running command can open a group. The commands it runs are part of it and are not
    /// journaled, so a group that fails has no journal entry to take back, and a panic is put back by the
    /// command's guard. A panic that is caught inside the command, around the group, is put back here:
    /// the group is closed, the document and revision restored, and the panic goes on unwinding.
    pub(crate) fn grouped<T>(&mut self, label: &str, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        check_label(label)?;
        if self.depth == 0 {
            return Err(internal(&shown(label), "an undo group can only be opened by a running command"));
        }
        if self.group.is_some() {
            return Err(failed(label, "undo groups cannot nest"));
        }
        let (before, held) = self.state.as_ref().map(|s| (Arc::clone(&s.doc), s.held)).ok_or_else(|| failed(label, "no document open"))?;
        let (revision, depth) = (self.revision, self.depth);
        self.group = Some(Group { edited: false, start: Arc::clone(&before), held });
        let outcome = catch_unwind(AssertUnwindSafe(|| f(self)));
        let group = self.group.take();
        let result = match outcome {
            Ok(result) => result,
            Err(payload) => {
                // The commands the group ran were cut short, so the depth they raised is still up.
                self.depth = depth;
                self.undo_group(before, revision, group.map(|g| g.held));
                resume_unwind(payload)
            }
        };
        let read = self.kept_read();
        match (result, self.state.as_mut()) {
            (Ok(value), Some(state)) => {
                state.note_read(read.is_some());
                if group.is_some_and(|g| g.edited) {
                    // The one step the group records ends the redo steps, and lets go of the oldest past the limit: what they free is taken from the count. (The document a pending stamp
                    // keeps is held whatever the steps do.)
                    let mut kept: Vec<&Arc<Document>> = vec![&state.doc, &state.clean, &before];
                    kept.extend(read.as_ref());
                    let command: Vec<&Arc<Document>> =
                        self.shadow.as_ref().and_then(|s| s.state.as_ref()).into_iter().flat_map(|s| s.history.redo_documents()).collect();
                    let (held, drop_undo) = held::after_group(state.held, &state.history, &kept, &command);
                    state.held = held;
                    state.history.drop_oldest_undo(drop_undo);
                    let bytes = crate::odgfile::hold::unshared_bytes(&before, &state.doc);
                    state.history.record(Arc::from(label), before, bytes);
                    let cut = state.history.take_cut();
                    Session::let_go_in_shadow(&mut self.shadow, drop_undo);
                    Session::credit_history_cut(state, None, &mut self.shadow, cut, true);
                }
                self.publish_held();
                Ok(value)
            }
            (Ok(_), None) => Err(failed(label, "the document was closed while the group ran")),
            (Err(e), _) => {
                self.undo_group(before, revision, group.map(|g| g.held));
                Err(e)
            }
        }
    }

    /// Takes back a group's edits. They made no history steps, and undo, redo and a new document are
    /// refused inside a group, so putting the document and the revision back is all it takes.
    fn undo_group(&mut self, before: Arc<Document>, revision: u64, held: Option<Held>) {
        if let Some(state) = self.state.as_mut() {
            state.doc = before;
            // What the group's edits freed and added is put back with the document. An edit of it may have let go of undo steps to make room (the figure), and those are gone, so
            // the count is the one before and may be more than what is held now.
            if let Some(held) = held {
                state.held = Held { bytes: held.bytes, drifted: true, slack: held.slack };
            }
        }
        self.publish_held();
        self.revision = revision;
    }

    /// Is an undo group open, as when a batch is running?
    pub(crate) fn in_group(&self) -> bool {
        self.group.is_some()
    }
}

/// What the model of a drawing and its history may hold as an edit leaves it ([`EDIT_MODEL_BYTES`]): the count of [`held`].
const MODEL_MOST: u64 = EDIT_MODEL_BYTES;

/// Every name that enters the history comes through `edit` or `grouped`, and passes this: the rule page
/// names follow ([`diacraft_doc::check_text`]), so a control character in a name a command was given
/// cannot reach the undo menu, a log or a terminal. The error does not echo the name raw.
fn check_label(label: &str) -> Result<()> {
    diacraft_doc::check_text(label).map_err(|problem| failed(&shown(label), format!("the undo step name {problem}")))
}

/// What is held that a read of a file leaves room for ([`Session::held_memory`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldMemory {
    /// The session's open drawing: its file's bytes and its model.
    pub drawing: u64,
    /// The models of the session's open stencil files.
    pub stencils: u64,
    /// Everything the process holds ([`crate::memory::held`]), the session's own included.
    pub process: u64,
}

impl HeldMemory {
    /// The whole of it: what the process holds (never less than what this session says it holds).
    pub fn total(&self) -> u64 {
        self.process.max(self.drawing.saturating_add(self.stencils))
    }

    /// What the process holds beside this session's drawing and stencils: other sessions (the window's, a scratch session), a caller's model.
    pub fn elsewhere(&self) -> u64 {
        self.total().saturating_sub(self.drawing.saturating_add(self.stencils))
    }

    /// The limits of a read made now: the reader's own, with the memory it may use what is left of [`diacraft_vsdx::MAX_MEMORY`] beside what is held.
    pub fn read_limits(&self) -> diacraft_vsdx::Limits {
        crate::memory::limits_beside(self.total())
    }

    /// Words for a person, to add to a refusal for memory when something is held: what is held, and what to do. The budget they name is the VSDX reader's
    /// ([`diacraft_vsdx::MAX_MEMORY`]); a refusal of a `.diacraft` read names the one it was held to ([`HeldMemory::words_of`]).
    pub fn words(&self) -> String {
        self.words_of(diacraft_vsdx::MAX_MEMORY)
    }

    /// [`HeldMemory::words`] for a read whose budget is `budget` bytes in all: a `.diacraft` read's is the smaller of the process's budget and `diacraft_native::MAX_MEMORY` (the engine's
    /// `nativefile::open::READ_BUDGET`), a `.diacraft` save's is `diacraft_native::MAX_MEMORY`, and the number a person is told is the one that was held against.
    pub fn words_of(&self, budget: u64) -> String {
        format!(
            "the open drawing and its file hold {} bytes, the open stencils {} and the rest of the process {} of the {budget} bytes that reading a file may use at once: close a stencil, or the drawing, and open this file again",
            self.drawing,
            self.stencils,
            self.elsewhere(),
        )
    }
}

#[cfg(test)]
mod tests;
