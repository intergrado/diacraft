//! Parts parsed ahead of the read's turn for them, on threads of their own.
//!
//! **What is done ahead is the parse of a page or a master part into its tree, the figures of the tree, and the drop of the tree when the read has done with it**: a pure function of the
//! part's bytes, which on a drawing of many pages is about half of what a read costs (a profile of 250 pages, 76,000 shapes: parse 31 percent, the tree's figures 6, the drop 15). The model
//! is built from the trees by the read, **one part after another on the thread that calls it, charging the budgets in the order of the file, as the single-threaded read does**; that part
//! cannot be done on threads without changing what a budget charges (the table of what is shared charges a unit by what was read before it, and enters it only when the budget has the bytes of its
//! entry), so it stays the read's. What a read does with a tree it has been given is what it does with one it parsed itself.
//!
//! **What makes the answer the same at any count of threads.**
//!
//! * A tree is taken only if it is *the tree the read would have parsed*. The read asks the budget for the limits of the parse as it always did (`Reader::xml_limits_settled`: a part that the memory
//!   left does not hold is refused there, with the same words) and takes the tree parsed ahead when it made no more items than those limits allow (`parse_measured` counts them as the limit does; a
//!   limit only ever ends a parse, so a tree within it is the tree a parse within it makes). A part whose parse ahead was refused for any reason, or made more items, is parsed by the read itself,
//!   within its limits, which is the single-threaded read: **the error is the one that read gives**.
//! * A part that was not parsed yet when the read came for it is parsed by the read (`Taken::Not`): no thread is waited for to start.
//! * A thread that fails (a panic in a parse) is an error in words (`VsdxError::Worker`) and never a panic of the read.
//!
//! **What makes the memory never more than the budget.** The budget is paid out for no memory that is still held:
//!
//! * The memory of a tree held ahead is **set aside from the budget** (`SheetReader::set_aside`: not charged, so no figure of the report moves, and not paid out) at the dearest the parse of its
//!   part can make it, **before the part is asked for**, and the parse is bounded to it (`max_nodes`, at the dearest price of an item). The work ahead is held to a quarter of what the budget has
//!   left, so that at the edge of the budget it is none.
//! * **What is set aside goes back to the budget only when the memory it stands for is gone.** A tree the read takes is charged by its real size in the place of its set-aside, in one step. A tree
//!   the read has done with and gives to a thread to drop is *given back as a charge* only as the report counts it, and **its bytes stay set aside until the thread has dropped it**
//!   (`SheetReader::give_bytes_later`). A charge that the rest of the budget cannot pay, a parse of the read's own that the memory in flight could make too small, and the end of the parts of an index
//!   **end the work first**: [`Ahead::drain`] waits for every thread to finish its part and drops every tree that was made and not taken, and only then are the bytes set aside paid out
//!   (`SheetReader::settle_lookahead`). So the memory a read holds, the trees in flight included, is never more than its budget, whatever it abandons (a refusal, a cancel, rows that name parts it
//!   does not read), and a read that does no work ahead and one that does refuse the same charge, in the same words.
//!
//! Nothing here is made on the web or when one thread is asked for, and a thread that cannot be made is a thread that is not there: the parts wait and the read parses them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use diacraft_container::Work;
use diacraft_opc::xml::{El, Node, XmlLimits};
use diacraft_sheetxml::LookaheadHook;

use crate::ns::{Scope, ScopeExt};
use crate::reader::{Reader, RelMap};
use crate::unheld::TreeFigures;

/// A part parsed ahead: its tree, the items the parse counted against its limit, and the figures of the tree.
pub(crate) struct Fetched {
    pub tree: El,
    pub items: usize,
    pub figures: TreeFigures,
}

/// What the read gets when it asks for a part.
pub(crate) enum Taken {
    /// The tree parsed ahead, whole.
    Ready(Fetched),
    /// None: the part was not asked for, it was not begun (the read parses it), or its parse was refused (the read parses it within its own limits, and gives the error that gives).
    Not,
    /// The thread that was parsing it failed.
    Failed,
}

struct Job {
    key: String,
    max_nodes: usize,
}

enum Outcome {
    Tree(Fetched),
    /// The parse was refused (a limit or damage) or the part is not there: the read parses it itself.
    Refused,
    /// The parse panicked.
    Panicked,
}

enum Slot {
    Queued,
    Running,
    Done(Outcome),
}

struct State {
    jobs: VecDeque<Job>,
    slots: HashMap<String, Slot>,
    /// Trees the read has done with, to be dropped by a thread, with the bytes each held.
    drops: VecDeque<(El, u64)>,
    /// The bytes of the trees handed over to be dropped and not dropped yet (waiting, or being dropped).
    dropping: u64,
    /// The bytes of the trees whose drop is done since the read last asked: the read gives them back to the budget.
    dropped: u64,
    /// The bytes of the trees that are held and are the read's no longer or not yet: those parsed and not taken, and those handed over to be dropped and not dropped. For a test.
    live: u64,
    closing: bool,
}

/// The parts being parsed ahead, shared by the read and the threads. See the module documentation.
pub(crate) struct Ahead {
    state: Mutex<State>,
    /// Threads wait here for something to do.
    work: Condvar,
    /// The read waits here for a part that is being parsed, or for a tree that is being dropped.
    done: Condvar,
    xml: XmlLimits,
}

/// The most the trees waiting to be dropped may hold between them: they are the memory of parts that are read already.
const DROP_WINDOW_BYTES: u64 = 64 * 1024 * 1024;

/// A tree that holds less than this is dropped by the read: handing it over costs more than dropping it.
const DROP_MIN_BYTES: u64 = 256 * 1024;

impl Ahead {
    pub(crate) fn new(xml: XmlLimits) -> Ahead {
        Ahead {
            state: Mutex::new(State {
                jobs: VecDeque::new(),
                slots: HashMap::new(),
                drops: VecDeque::new(),
                dropping: 0,
                dropped: 0,
                live: 0,
                closing: false,
            }),
            work: Condvar::new(),
            done: Condvar::new(),
            xml,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A thread never holds the lock while it parses or drops, so a poisoned lock is one whose holder stopped between two plain assignments: the state is whole.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks for the part `key` (the name the read will ask for it by, which the threads find the bytes of) to be parsed to at most `max_nodes` items.
    pub(crate) fn request(&self, key: &str, max_nodes: usize) {
        let mut state = self.lock();
        if state.closing || state.slots.contains_key(key) {
            return;
        }
        state.slots.insert(key.to_owned(), Slot::Queued);
        state.jobs.push_back(Job { key: key.to_owned(), max_nodes });
        drop(state);
        self.work.notify_one();
    }

    /// The part `key`: waited for when a thread is parsing it, and not when no thread began (the read parses it then), unless a test asked the read to wait for the threads
    /// (`work::wait_for_the_work_ahead`; shipped code never does).
    pub(crate) fn take(&self, key: &str) -> Taken {
        let mut state = self.lock();
        let waits = crate::work::waits_for_the_work_ahead();
        loop {
            match state.slots.get(key) {
                None => return Taken::Not,
                Some(Slot::Queued) if !waits => {
                    state.jobs.retain(|job| job.key != key);
                    state.slots.remove(key);
                    return Taken::Not;
                }
                // A part being parsed, or one a thread will begin (a job waiting is always begun: the threads take the jobs in order, and only the read ends them).
                Some(Slot::Queued | Slot::Running) => state = self.done.wait(state).unwrap_or_else(PoisonError::into_inner),
                Some(Slot::Done(_)) => {
                    return match state.slots.remove(key) {
                        Some(Slot::Done(Outcome::Tree(fetched))) => {
                            state.live = state.live.saturating_sub(fetched.figures.bytes);
                            Taken::Ready(fetched)
                        }
                        Some(Slot::Done(Outcome::Panicked)) => Taken::Failed,
                        _ => Taken::Not,
                    };
                }
            }
        }
    }

    /// Gives a tree the read has done with, which held `bytes`, to be dropped by a thread; the tree comes back when it is not worth it (it is small) or the trees waiting hold the window already, and
    /// the caller drops it. **The bytes are the read's to give back to the budget when [`Ahead::take_dropped`] says the tree is dropped.**
    pub(crate) fn drop_later(&self, tree: El, bytes: u64) -> Option<El> {
        if bytes < DROP_MIN_BYTES {
            return Some(tree);
        }
        let mut state = self.lock();
        if state.closing || state.dropping.saturating_add(bytes) > DROP_WINDOW_BYTES {
            return Some(tree);
        }
        state.dropping += bytes;
        state.live += bytes;
        state.drops.push_back((tree, bytes));
        drop(state);
        self.work.notify_one();
        None
    }

    /// The bytes of the trees whose drop is done since this was last asked.
    pub(crate) fn take_dropped(&self) -> u64 {
        std::mem::take(&mut self.lock().dropped)
    }

    /// Stops the work that is not begun: the parts waiting for a thread are not parsed (the read parses what it comes to).
    pub(crate) fn cancel_queued(&self) {
        let mut state = self.lock();
        state.jobs.clear();
        state.slots.retain(|_, slot| !matches!(slot, Slot::Queued));
    }

    /// **Ends the work and lets go of everything it holds, and returns when that is done**: the parts waiting are not begun, the threads finish the parts they are parsing, every tree that was
    /// made and not taken is dropped, and every tree handed over to be dropped is dropped. The bytes of the budget that stand for them can be paid out then.
    pub(crate) fn drain(&self) {
        let mut state = self.lock();
        state.jobs.clear();
        state.slots.retain(|_, slot| !matches!(slot, Slot::Queued));
        // The trees handed over and not begun are dropped here (the threads are not waited for to begin them); the ones a thread is dropping are waited for.
        let mut trees: Vec<El> = Vec::new();
        while let Some((tree, bytes)) = state.drops.pop_front() {
            state.dropping = state.dropping.saturating_sub(bytes);
            state.live = state.live.saturating_sub(bytes);
            trees.push(tree);
        }
        while state.slots.values().any(|slot| matches!(slot, Slot::Running)) {
            state = self.done.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        for (_, slot) in std::mem::take(&mut state.slots) {
            if let Slot::Done(Outcome::Tree(fetched)) = slot {
                state.live = state.live.saturating_sub(fetched.figures.bytes);
                crate::work::ahead_abandoned(1);
                trees.push(fetched.tree);
            }
        }
        // What a thread is dropping now: its bytes are in `dropping` until it has.
        while state.dropping > 0 {
            state = self.done.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.dropped = 0;
        drop(state);
        drop(trees);
    }

    /// Ends the work for good: nothing waiting is begun, the threads go when what they are doing is done, and the parts parsed and not taken are let go.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closing = true;
        state.jobs.clear();
        let slots = std::mem::take(&mut state.slots);
        drop(state);
        self.work.notify_all();
        self.done.notify_all();
        drop(slots);
    }

    /// The bytes of the trees held that are not the read's (for a test).
    #[cfg(feature = "test-support")]
    pub(crate) fn live(&self) -> u64 {
        self.lock().live
    }

    /// What a thread does until the work is closed: parse the parts asked for in the order they were asked for (`bytes_of` finds the bytes of a part by its name), and drop the trees given.
    pub(crate) fn serve<'p>(&self, bytes_of: &(dyn Fn(&str) -> Option<&'p [u8]> + Sync)) {
        loop {
            let mut state = self.lock();
            if let Some(job) = state.jobs.pop_front() {
                state.slots.insert(job.key.clone(), Slot::Running);
                drop(state);
                // A parse that panics leaves nothing half made behind it: the slot says it failed and the thread goes on.
                let outcome = match catch_unwind(AssertUnwindSafe(|| self.parse(&job, bytes_of))) {
                    Ok(Some(fetched)) => Outcome::Tree(fetched),
                    Ok(None) => Outcome::Refused,
                    Err(_) => Outcome::Panicked,
                };
                let mut state = self.lock();
                if state.closing {
                    drop(state);
                    drop(outcome);
                } else {
                    if let Outcome::Tree(fetched) = &outcome {
                        state.live = state.live.saturating_add(fetched.figures.bytes);
                    }
                    state.slots.insert(job.key, Slot::Done(outcome));
                    drop(state);
                }
                self.done.notify_all();
                continue;
            }
            if let Some((tree, bytes)) = state.drops.pop_front() {
                drop(state);
                drop(tree);
                let mut state = self.lock();
                state.dropping = state.dropping.saturating_sub(bytes);
                state.live = state.live.saturating_sub(bytes);
                state.dropped = state.dropped.saturating_add(bytes);
                drop(state);
                self.done.notify_all();
                continue;
            }
            if state.closing {
                return;
            }
            drop(self.work.wait(state).unwrap_or_else(PoisonError::into_inner));
        }
    }

    fn parse<'p>(&self, job: &Job, bytes_of: &(dyn Fn(&str) -> Option<&'p [u8]> + Sync)) -> Option<Fetched> {
        let bytes = bytes_of(&job.key)?;
        #[cfg(feature = "test-support")]
        injected_panic(bytes);
        let limits = XmlLimits { max_nodes: job.max_nodes.min(self.xml.max_nodes), ..self.xml.clone() };
        let (tree, items) = diacraft_container::xml::parse_measured(bytes, &limits, &mut Work::default()).ok()?;
        let figures = TreeFigures::of(&tree);
        Some(Fetched { tree, items, figures })
    }
}

/// For the tests: a part that holds the comment `<!--panic-in-ahead-->` makes the thread that parses it panic, so that a failure of a thread is seen to be an error and not a panic of the read.
/// Compiled only with the `test-support` feature: shipped code has no way to make a thread panic.
#[cfg(feature = "test-support")]
#[allow(clippy::panic)]
fn injected_panic(bytes: &[u8]) {
    const MARK: &[u8] = b"<!--panic-in-ahead-->";
    if bytes.windows(MARK.len()).any(|window| window == MARK) {
        panic!("a part that holds the mark that makes the thread that parses it panic");
    }
}

/// Closes the work when it is dropped, whatever ends the read (it returns, or it unwinds), so that the threads of the scope can be joined.
pub(crate) struct Closer<'a>(pub &'a Ahead);

impl Drop for Closer<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// The budget's door to the work ahead: it ends the work before the bytes set aside go back, and, for a test, holds the memory in flight to what is set aside.
pub(crate) struct Hook(pub Arc<Ahead>);

impl LookaheadHook for Hook {
    fn drain(&self) {
        self.0.drain();
    }

    #[cfg(feature = "test-support")]
    fn changed(&self, set_aside: u64) {
        if self.0.live() > set_aside {
            crate::work::lookahead_violation();
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the read's side

/// The most parts that are in flight at once, for each thread: the read is the one thread that builds the model, and the threads are to keep ahead of it, not to hold trees.
const IN_FLIGHT_PER_THREAD: usize = 2;

/// The smallest part worth asking a thread for (the bytes of its XML): a smaller one is parsed in less time than it takes to hand over.
const AHEAD_MIN_BYTES: usize = 16 * 1024;

/// The most parts a plan holds: the rows of a document past this are parsed by the read when it comes to them.
const PLAN_MOST: usize = 4096;

/// The most that is set aside from the budget for the trees in flight, whatever the budget has: 512 MiB.
const SET_ASIDE_MOST: u64 = 512 * 1024 * 1024;

/// What the read keeps of the work ahead: the parts it plans to ask for, in order, and what is set aside for each that is asked for.
pub(crate) struct Lookahead<'p> {
    ahead: Arc<Ahead>,
    plan: VecDeque<(String, &'p [u8])>,
    reserved: HashMap<String, u64>,
    /// The most parts in flight at once.
    window: usize,
    /// The work ahead is over: the budget took back what it set aside, so the memory left is not to be spent on trees that are not wanted yet.
    stopped: bool,
}

impl<'p> Lookahead<'p> {
    pub(crate) fn new(ahead: Arc<Ahead>, workers: usize) -> Lookahead<'p> {
        Lookahead {
            ahead,
            plan: VecDeque::new(),
            reserved: HashMap::new(),
            window: workers.saturating_mul(IN_FLIGHT_PER_THREAD).max(1),
            stopped: false,
        }
    }

    /// The work is over (the budget has paid out what was set aside, after the work let go of everything): nothing more is asked for.
    fn stop(&mut self) {
        self.stopped = true;
        self.plan.clear();
        self.reserved.clear();
        self.ahead.cancel_queued();
    }
}

/// What the parse of a part of `bytes` may hold, at the dearest it can be made, and the items it is held to: the memory the budget sets aside for it, and its limit. **A part that is denser than
/// one item to six bytes** (a real page is one to nine) **makes more items than that and is not parsed ahead**: the read parses it when it comes to it.
fn reservation(bytes: &[u8]) -> (u64, usize) {
    let items = bytes.len() / 6 + 64;
    let fixed = crate::cost::parse_fixed_bytes(diacraft_opc::xml::parse_size(bytes));
    (fixed.saturating_add((items as u64).saturating_mul(crate::cost::TREE_ITEM_BYTES)), items)
}

impl<'p> Reader<'p> {
    /// Plans the parts the rows of an index lead to (`row` is `Page` or `Master`, `ty` the type of relationship the row's `Rel` must have): each part that is in the package and worth a thread, in
    /// the order of the rows, once. **A plan is a guess and costs nothing when it is wrong**: what the read does not ask for is not used, and what it asks for that is not planned it parses itself.
    pub(crate) fn plan_ahead(&mut self, root: &El, sc: &Scope, rels: &RelMap, row: &str, ty: &str) {
        let pkg = self.pkg;
        let Some(look) = self.look.as_mut() else { return };
        if look.stopped {
            return;
        }
        let mut seen: HashSet<String> = HashSet::new();
        for node in &root.children {
            let Node::El(row_el) = node else { continue };
            let rsc = sc.enter(row_el);
            if !rsc.is(row_el, row) {
                continue;
            }
            let mut rel_id = None;
            for child in &row_el.children {
                let Node::El(child) = child else { continue };
                let csc = rsc.enter(child);
                if csc.ns_of(child).is_ours() && child.is("Rel") && csc.rel_id(child).is_some() {
                    rel_id = csc.rel_id(child);
                    break;
                }
            }
            let Some(rel) = rel_id.and_then(|id| rels.get(id)).filter(|rel| rel.ty == ty) else { continue };
            let Some(target) = rel.target.as_deref() else { continue };
            if look.plan.len() >= PLAN_MOST || !seen.insert(target.to_owned()) {
                continue;
            }
            if let Some(bytes) = pkg.part(target).filter(|bytes| bytes.len() >= AHEAD_MIN_BYTES) {
                look.plan.push_back((target.to_owned(), bytes));
            }
        }
        self.top_up_ahead();
    }

    /// Notes that the budget took back what it set aside (a charge or a parse ended the work ahead, which let go of everything it held first): **what was set aside for each part asked for is
    /// not set aside any more**, so the work is over and what the read remembers of it is forgotten. It is asked before anything of that is given back to the budget.
    fn sync_ahead(&mut self) {
        if self.sheets.lookahead_taken_back()
            && let Some(look) = self.look.as_mut()
            && !look.stopped
        {
            crate::work::ahead_stopped();
            look.stop();
        }
    }

    /// Asks for the next parts of the plan while the threads have room and **the budget can set the memory of their trees aside, a quarter of what it has left at the most**; and gives back to the budget
    /// the bytes of the trees the threads have dropped.
    pub(crate) fn top_up_ahead(&mut self) {
        self.sync_ahead();
        let dropped = self.look.as_ref().map_or(0, |look| look.ahead.take_dropped());
        self.sheets.set_aside_back(dropped);
        let Some(look) = self.look.as_mut() else { return };
        while !look.stopped && look.reserved.len() < look.window {
            let Some((name, bytes)) = look.plan.front() else { break };
            let (reserve, max_nodes) = reservation(bytes);
            let set_aside = self.sheets.lookahead().saturating_add(reserve);
            if set_aside > SET_ASIDE_MOST || set_aside.saturating_mul(4) > self.sheets.bytes_left_settled() {
                if look.reserved.is_empty() {
                    // It would not be held beside the rest even alone: the read parses this one, and the next may be smaller.
                    look.plan.pop_front();
                    continue;
                }
                break;
            }
            if !self.sheets.set_aside(reserve) {
                break;
            }
            look.ahead.request(name, max_nodes);
            look.reserved.insert(name.clone(), reserve);
            look.plan.pop_front();
        }
    }

    /// The part `key` as the work ahead has it (`Taken`), and the bytes that were set aside for it, which **the caller gives back to the budget when it has charged what the tree is**
    /// (`SheetReader::set_aside_back`, at once before the charge, with nothing between).
    pub(crate) fn take_ahead(&mut self, key: &str) -> (Taken, u64) {
        self.sync_ahead();
        let Some(look) = self.look.as_mut() else { return (Taken::Not, 0) };
        if look.plan.front().is_some_and(|(name, _)| name == key) {
            look.plan.pop_front();
        } else {
            look.plan.retain(|(name, _)| name != key);
        }
        let taken = look.ahead.take(key);
        (taken, look.reserved.remove(key).unwrap_or(0))
    }

    /// Ends the work ahead for the parts of an index: **the work lets go of everything it holds** (the parts it parsed that the read did not ask for, the trees it is dropping), and then what was
    /// set aside goes back to the budget.
    pub(crate) fn end_ahead(&mut self) {
        let Some(look) = self.look.as_mut() else { return };
        look.plan.clear();
        look.reserved.clear();
        look.ahead.drain();
        self.sheets.settle_lookahead();
        // The work is over for this index, not for the read: the next index plans its own.
        let _ = self.sheets.lookahead_taken_back();
    }

    /// Lets go of a tree the read has done with and gives its bytes back to the budget: on a thread of the work ahead when it has one to spare (the bytes are given back when the thread has dropped
    /// it), and here otherwise.
    pub(crate) fn release_tree(&mut self, tree: El, held: u64) {
        self.sync_ahead();
        let tree = match self.look.as_ref().filter(|look| !look.stopped) {
            Some(look) => match look.ahead.drop_later(tree, held) {
                None => {
                    self.sheets.give_bytes_later(held);
                    return;
                }
                Some(tree) => tree,
            },
            None => tree,
        };
        drop(tree);
        self.give_bytes(held);
    }
}
