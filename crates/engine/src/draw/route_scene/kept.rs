//! The scenes a session keeps: one for each page it has routed on, entered for the time of an edit (as `calc_index` keeps the readers of the page's cells).
//!
//! The commands make their routings deep in `draw`, where a document is held and no session, so the session **enters its scenes for the time an edit runs** ([`InUse`], taken by
//! `Session::edit_if` around the command's closure and the refit that follows), and [`with_scene`] is the one call the routings make: it gives the scene of the page of the session that is
//! editing, and with none (a call outside an edit, a document no session holds) it makes a scene for the call.
//!
//! **Nothing depends on a kept scene being right.** [`Scene::put_right`] makes it the document's by pointer whatever happened between: an undo, a redo, a command that failed and was put
//! back, a file opened over the drawing. The session also puts the scenes right where it puts another document back (`Session::step_history`, `restore` and an edit that came to nothing:
//! [`KeptScenes::sync`]), so that the edit after it is not told that a shape an undo moved back "came to cross" a connector. **What a scene holds is counted**: [`KeptScenes::held_bytes`] is in
//! the session's hold on the process's ledger, the scenes of the pages used longest ago are let go when they hold more than [`MAX_KEPT_BYTES`] (the scene in use never is), and all of them are let go
//! when the drawing is replaced. A scene that was being put right when a panic unwound is let go too: what it holds is not trusted.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use diacraft_doc::Document;
use diacraft_route::Work;

use super::Scene;

/// About the most bytes the scenes of a session hold in all, by [`Scene::held_bytes`]: checked after each use, past it the page used longest ago is let go. A scene at the caps is about
/// 40 MiB (a page of 10,000 shapes and 10,000 connectors, about 10), so two or three big pages are kept and a drawing of many pages keeps the ones in use.
pub(crate) const MAX_KEPT_BYTES: u64 = 64 * 1024 * 1024;

/// The steps of work a put right is given when the session puts the scenes right (an undo, a failed command): enough for every cap at once.
const SYNC_STEPS: u64 = 20_000_000;

#[derive(Debug)]
struct Entry {
    scene: Scene,
    /// When it was last used, by the tick of the scenes.
    used: u64,
}

#[derive(Debug)]
struct Kept {
    scenes: BTreeMap<u32, Entry>,
    tick: u64,
    cap: u64,
}

impl Kept {
    fn held(&self) -> u64 {
        self.scenes.values().fold(0u64, |sum, e| sum.saturating_add(e.scene.held_bytes()))
    }

    /// Lets go of the scene used longest ago, other than `keep`, while the scenes hold more than the cap.
    fn trim(&mut self, keep: u32) {
        while self.scenes.len() > 1 && self.held() > self.cap {
            let Some(oldest) = self.scenes.iter().filter(|(page, _)| **page != keep).min_by_key(|(_, e)| e.used).map(|(page, _)| *page) else {
                break;
            };
            self.scenes.remove(&oldest);
        }
    }
}

/// The scenes a session keeps: shared with the thread that is editing for the time of an edit.
#[derive(Debug)]
pub(crate) struct KeptScenes(Arc<Mutex<Kept>>);

impl Default for KeptScenes {
    fn default() -> KeptScenes {
        KeptScenes::with_cap(MAX_KEPT_BYTES)
    }
}

/// The scenes behind the lock; **a lock a panic poisoned lets go of every scene**: a put right that unwound may have left a scene half made, which is no scene to trust.
fn lock(kept: &Mutex<Kept>) -> MutexGuard<'_, Kept> {
    match kept.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            guard.scenes.clear();
            kept.clear_poison();
            guard
        }
    }
}

impl KeptScenes {
    /// Scenes that are let go past `cap` bytes.
    pub(crate) fn with_cap(cap: u64) -> KeptScenes {
        KeptScenes(Arc::new(Mutex::new(Kept { scenes: BTreeMap::new(), tick: 0, cap })))
    }

    /// What the scenes are charged, as accounted ([`Scene::held_bytes`]).
    pub(crate) fn held_bytes(&self) -> u64 {
        lock(&self.0).held()
    }

    /// How many scenes are kept.
    pub(crate) fn len(&self) -> usize {
        lock(&self.0).scenes.len()
    }

    /// The pages a scene is kept of, in order.
    pub(crate) fn pages(&self) -> Vec<u32> {
        lock(&self.0).scenes.keys().copied().collect()
    }

    /// Lets every scene go: the drawing they were made for is replaced.
    pub(crate) fn clear(&self) {
        lock(&self.0).scenes.clear();
    }

    /// Puts every scene right with `doc`, the document the session has now, and reports nothing of what that moved: the edit that used them did not become the drawing, or another
    /// document was put back (an undo, a redo, a command that failed). A scene of the document it has seen costs a compare of pointers.
    pub(crate) fn sync(&self, doc: &Document) {
        let mut kept = lock(&self.0);
        kept.scenes.retain(|page, _| doc.page(*page).is_some());
        for (page, entry) in &mut kept.scenes {
            // What the put right reports is the edit's own business and this is not an edit; work that runs out leaves a scene that is read whole next time.
            let _ = entry.scene.put_right(doc, *page, &mut Work::new(SYNC_STEPS));
        }
    }
}

thread_local! {
    /// The scenes of the session that is editing on this thread, while it is.
    static CURRENT: RefCell<Option<Arc<Mutex<Kept>>>> = const { RefCell::new(None) };
}

/// For as long as it lives, the routings made on this thread use the scenes of the session it was made for. One inside another (a command that runs a command) leaves the outer one's in place
/// when it ends.
pub(crate) struct InUse {
    previous: Option<Arc<Mutex<Kept>>>,
}

impl InUse {
    pub(crate) fn enter(kept: &KeptScenes) -> InUse {
        InUse { previous: CURRENT.with(|current| current.replace(Some(Arc::clone(&kept.0)))) }
    }
}

impl Drop for InUse {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

/// The scenes of the session that is editing on this thread, if one is.
fn current() -> Option<Arc<Mutex<Kept>>> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Runs `f` on the scene of `page`: the session's, kept for the next edit, or, with no session editing, one made for the call.
///
/// `f` holds the session's scenes for as long as it runs: a call to `with_scene` inside it (on the same thread) finds them held and is given a scene of its own, which it makes whole and
/// nobody keeps, rather than waiting for itself.
pub(crate) fn with_scene<R>(page: u32, f: impl FnOnce(&mut Scene) -> R) -> R {
    let Some(kept) = current() else {
        let mut scene = Scene::new(page);
        return f(&mut scene);
    };
    let mut guard = match kept.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => {
            drop(poisoned);
            lock(&kept)
        }
        Err(TryLockError::WouldBlock) => {
            let mut scene = Scene::new(page);
            return f(&mut scene);
        }
    };
    guard.tick = guard.tick.saturating_add(1);
    let tick = guard.tick;
    let out = {
        let entry = guard.scenes.entry(page).or_insert_with(|| Entry { scene: Scene::new(page), used: tick });
        entry.used = tick;
        f(&mut entry.scene)
    };
    guard.trim(page);
    out
}
