//! The faces the engine's commits measure text in.
//!
//! A cell that calls `TEXTWIDTH` or `TEXTHEIGHT` (`TxtWidth`, `TxtHeight` of a master's text block) is worked out again when an edit changes
//! what it reads, and the number it gets depends on the faces the text is laid out in. The renderer lays the same text out in the machine's
//! faces, so a block sized in other faces would be drawn in letters of another width: a window shares the renderer's font database with the
//! commits it makes ([`set_measure_fonts`]), and every commit of the engine takes its options from [`eval_options`].
//!
//! **Reading the machine's faces never holds up a first frame.** Scanning the font folders can take seconds on a cold disk (the scan stops at
//! eight seconds), so an application starts it on a thread of its own at the top of `main` ([`start_system_fonts`]) and works on while it runs.
//! What is shared is a [`SharedFonts`], the database or the scan that is finding it. A commit that measures text waits for the scan the first
//! time it needs the faces, and so does an export; a commit that measures no text does not wait, and a window's workers take the database when
//! they are first asked to draw. So nothing ever measures in the bundled faces once the window can show the machine's. A scan that panics falls
//! back to the bundled faces, with a warning.
//!
//! With no database set (a library, a test) the commits measure in the bundled faces, which are the same on every machine, and an export or a
//! print builds the machine's faces for itself as it always did. The setting is a process-wide value because the commits are made deep in the
//! editing commands, which hold a document and not a window.

use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::JoinHandle;

use diacraft_calc::{EvalOptions, Fonts};
use diacraft_fonts::FontDb;

/// The faces text is set in: a database, or the scan on its thread that is finding it. A clone is another holder of the same faces, and every
/// holder that asks gets the same `Arc<FontDb>`.
#[derive(Clone)]
pub struct SharedFonts(Arc<Found>);

struct Found {
    /// The database, once there is one.
    ready: OnceLock<Arc<FontDb>>,
    /// The scan, until its answer is taken. It never ends in a panic: it catches its own.
    scan: Mutex<Option<JoinHandle<Arc<FontDb>>>>,
}

impl std::fmt::Debug for SharedFonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.ready.get() {
            Some(db) => write!(f, "SharedFonts({} faces)", db.face_count()),
            None => write!(f, "SharedFonts(being read)"),
        }
    }
}

/// The faces to use when none can be read: the bundled ones, the same on every machine.
fn fallback() -> Arc<FontDb> {
    Arc::new(FontDb::bundled())
}

/// What a scan found, or the bundled faces and a warning when it panicked.
fn found(scan: impl FnOnce() -> FontDb) -> Arc<FontDb> {
    match crate::guard::catch_panic(scan) {
        Ok(db) => Arc::new(db),
        Err(why) => {
            log::warn!("reading the fonts of this machine failed ({}): the bundled faces are used", crate::shown(&why, 200));
            fallback()
        }
    }
}

impl SharedFonts {
    /// A database in hand.
    pub fn ready(db: Arc<FontDb>) -> SharedFonts {
        let ready = OnceLock::new();
        let _ = ready.set(db);
        SharedFonts(Arc::new(Found { ready, scan: Mutex::new(None) }))
    }

    /// The faces. **Waits** for the scan if it is still going; call it on a thread that may wait (a worker, not the window's).
    pub fn get(&self) -> Arc<FontDb> {
        if let Some(db) = self.0.ready.get() {
            return Arc::clone(db);
        }
        // A holder that gets the lock after another has taken the answer finds it in `ready`.
        let mut scan = self.0.scan.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.take(&mut scan)
    }

    /// The faces if they are there, and `None` when the scan is still going. Never waits.
    pub fn try_get(&self) -> Option<Arc<FontDb>> {
        if let Some(db) = self.0.ready.get() {
            return Some(Arc::clone(db));
        }
        let mut scan = self.0.scan.try_lock().ok()?;
        if scan.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return None;
        }
        Some(self.take(&mut scan))
    }

    /// Whether the faces are in hand.
    pub fn is_ready(&self) -> bool {
        self.0.ready.get().is_some()
    }

    /// Whether `other` holds the same faces: a clone of this one.
    pub fn same_as(&self, other: &SharedFonts) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// How many holders there are (this one included).
    pub fn holders(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// The answer of the scan, joined, or what is already there; `scan` is the lock on it.
    fn take(&self, scan: &mut Option<JoinHandle<Arc<FontDb>>>) -> Arc<FontDb> {
        if let Some(db) = self.0.ready.get() {
            return Arc::clone(db);
        }
        let db = match scan.take() {
            Some(handle) => handle.join().unwrap_or_else(|_| fallback()),
            None => fallback(),
        };
        Arc::clone(self.0.ready.get_or_init(|| db))
    }
}

static MEASURE_FONTS: RwLock<Option<SharedFonts>> = RwLock::new(None);

/// What is shared now, if anything.
fn current() -> Option<SharedFonts> {
    // A lock a panic poisoned still holds a value that is a whole `Option`, so it is taken all the same.
    MEASURE_FONTS.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

fn share(fonts: Option<SharedFonts>) {
    *MEASURE_FONTS.write().unwrap_or_else(std::sync::PoisonError::into_inner) = fonts;
}

/// Shares `fonts` with every commit of the engine from now on (`None` takes it back): the database the renderer draws text in.
///
/// **An application sets it once, at its start, before it runs a command**, with the one database its renderers, its previews and its prints
/// are given as well ([`start_system_fonts`] and [`use_system_fonts`] do this): a commit made before it is set measures in the bundled faces,
/// and two databases would measure text in faces the other does not draw it in. **It is a setting of the process**, because the commits are
/// made deep in the editing commands, which hold a document and not a window: tests that set it do not see each other's value under
/// `cargo nextest`, which runs each in a process of its own, and may under a plain `cargo test`, which runs them on threads of one.
pub fn set_measure_fonts(fonts: Option<Arc<FontDb>>) {
    share(fonts.map(SharedFonts::ready));
}

/// Starts reading the machine's faces and the bundled ones on a thread of its own, **at once and without waiting**, and shares what it finds
/// with every commit and every export of the engine; the application gives the same [`SharedFonts`] to what it draws with. For a window's
/// start, the first thing in `main`: the first frame never waits for the scan. If the thread cannot be had, or the scan panics, the bundled
/// faces are used and a warning is logged.
pub fn start_system_fonts() -> SharedFonts {
    scan_fonts_with(FontDb::system_and_bundled)
}

/// As [`start_system_fonts`], with `scan` for the work on the thread: for a test that holds the scan back, or has it fail.
pub fn scan_fonts_with(scan: impl FnOnce() -> FontDb + Send + 'static) -> SharedFonts {
    let spawned = std::thread::Builder::new().name("diacraft-fonts".into()).spawn(move || found(scan));
    let fonts = match spawned {
        Ok(handle) => SharedFonts(Arc::new(Found { ready: OnceLock::new(), scan: Mutex::new(Some(handle)) })),
        Err(e) => {
            log::warn!("no thread to read the fonts of this machine on ({e}): the bundled faces are used");
            SharedFonts::ready(fallback())
        }
    };
    share(Some(fonts.clone()));
    fonts
}

/// Reads the machine's faces and the bundled ones, **on this thread**, and shares the database as [`start_system_fonts`] does. For a command
/// line that has nothing to show before it has them, before its first command.
pub fn use_system_fonts() -> Arc<FontDb> {
    let fonts = found(FontDb::system_and_bundled);
    set_measure_fonts(Some(Arc::clone(&fonts)));
    fonts
}

/// The bundled faces alone, shared as [`use_system_fonts`] shares the machine's: nothing is read from the machine, so a run measures and draws
/// the same on every machine (`diacraft-cli --bundled-fonts`).
pub fn use_bundled_fonts() -> Arc<FontDb> {
    let fonts = fallback();
    set_measure_fonts(Some(Arc::clone(&fonts)));
    fonts
}

/// The database set by [`set_measure_fonts`] or the scan [`start_system_fonts`] began, if any. **Waits** for a scan that is still going.
pub fn measure_fonts() -> Option<Arc<FontDb>> {
    current().map(|fonts| fonts.get())
}

/// As [`measure_fonts`], but **never waits**: `None` when nothing is shared, and also while the scan is still going. For a read that runs in the
/// window's frame and has a database of its own to fall back on (`shape.formatting` naming the default face, `formatting/face.rs`).
pub fn measure_fonts_now() -> Option<Arc<FontDb>> {
    current().and_then(|fonts| fonts.try_get())
}

/// The options of an evaluation the engine makes to commit an edit: the defaults, with the faces text is measured in. The faces are asked for
/// when a text is measured, and not here: a commit that measures no text never waits for a scan that is still going.
pub(crate) fn eval_options() -> EvalOptions {
    let fonts = current().map(|shared| match shared.try_get() {
        Some(db) => Fonts::shared(db),
        None => Fonts::lazy(move || shared.get()),
    });
    EvalOptions { fonts, ..EvalOptions::default() }
}

/// The faces an export or a print lays its text out in: the shared database when an application set one (so that a page is set in the faces the
/// person sees, and the machine's folders are not read again; it waits for a scan that is still going), else the machine's and the bundled
/// faces, read for this one.
pub fn renderer_fonts() -> Arc<FontDb> {
    measure_fonts().unwrap_or_else(|| Arc::new(FontDb::system_and_bundled()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::*;

    /// Runs `f` on a thread and waits at most ten seconds for it: a call that waits for a scan that is held back fails here, and does not hang
    /// the run.
    fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(10)).ok()
    }

    /// The options carry the faces that were set, and none after they are taken back.
    #[test]
    fn the_options_of_a_commit_carry_the_faces_that_were_set() {
        assert!(eval_options().fonts.is_none() || measure_fonts().is_some(), "no faces unless something set them");
        let shared = Arc::new(FontDb::bundled());
        set_measure_fonts(Some(Arc::clone(&shared)));
        assert!(eval_options().fonts.is_some());
        assert!(Arc::ptr_eq(&measure_fonts().unwrap(), &shared), "the renderer's database and the commits' are one");
        assert!(Arc::ptr_eq(&renderer_fonts(), &shared), "and an export draws in it");
        set_measure_fonts(None);
        assert!(eval_options().fonts.is_none());
    }

    /// An application's startup on the command line shares one database with everything that measures or draws: [`use_system_fonts`] reads the
    /// machine's faces, hands the database back and makes it the commits' and the exports'; [`use_bundled_fonts`] does the same with the
    /// bundled faces alone, for a run that must give the same numbers on every machine.
    #[test]
    fn the_startup_shares_one_database_with_the_commits_and_the_exports() {
        let bundled = use_bundled_fonts();
        assert_eq!(bundled.face_count(), FontDb::bundled().face_count(), "nothing is read from the machine");
        assert!(Arc::ptr_eq(&measure_fonts().unwrap(), &bundled));
        assert!(Arc::ptr_eq(&renderer_fonts(), &bundled), "an export draws in the faces the commits measure in");
        let system = use_system_fonts();
        assert!(system.face_count() >= bundled.face_count(), "the machine's faces and the bundled ones");
        assert!(Arc::ptr_eq(&measure_fonts().unwrap(), &system));
        assert!(Arc::ptr_eq(&renderer_fonts(), &system));
        assert!(eval_options().fonts.is_some());
        set_measure_fonts(None);
    }

    /// A scan that is still going holds up only what needs the faces: the options of a commit are made at once, a holder that asks for the
    /// faces waits and gets the scan's own database, and when the scan is done everything has the one database.
    #[test]
    fn a_scan_still_going_holds_up_only_what_asks_for_the_faces() {
        let (release, gate) = channel::<()>();
        let fonts = scan_fonts_with(move || {
            let _ = gate.recv();
            FontDb::empty()
        });
        assert!(!fonts.is_ready() && fonts.try_get().is_none(), "the scan is held back");
        assert!(within(|| eval_options().fonts.is_some()).expect("the options of a commit do not wait for the scan"));
        // Whoever asks for the faces waits for the scan.
        let (asked, answered) = channel();
        let waiter = std::thread::spawn(move || {
            let db = measure_fonts();
            let _ = asked.send(());
            db
        });
        assert!(answered.recv_timeout(Duration::from_millis(200)).is_err(), "asking for the faces waits for the scan");
        release.send(()).unwrap();
        let db = waiter.join().unwrap().unwrap();
        assert_eq!(db.face_count(), FontDb::empty().face_count(), "the scan's own database");
        assert!(fonts.is_ready() && Arc::ptr_eq(&fonts.get(), &db) && Arc::ptr_eq(&fonts.try_get().unwrap(), &db));
        assert!(Arc::ptr_eq(&renderer_fonts(), &db), "an export draws in it");
        set_measure_fonts(None);
    }

    /// A scan that panics (a font it cannot read) falls back to the bundled faces, and the commits, the exports and every holder have them.
    #[test]
    fn a_scan_that_panics_falls_back_to_the_bundled_faces() {
        let fonts = scan_fonts_with(|| panic!("a font the scan cannot read"));
        let db = fonts.get();
        assert_eq!(db.face_count(), FontDb::bundled().face_count());
        assert!(fonts.is_ready());
        assert!(Arc::ptr_eq(&measure_fonts().unwrap(), &db) && Arc::ptr_eq(&renderer_fonts(), &db));
        set_measure_fonts(None);
        // The same on the calling thread.
        let alone = found(|| panic!("again"));
        assert_eq!(alone.face_count(), FontDb::bundled().face_count());
    }

    /// Clones are holders of the same faces, and two scans are two sets of faces.
    #[test]
    fn a_clone_is_a_holder_of_the_same_faces() {
        let one = SharedFonts::ready(Arc::new(FontDb::bundled()));
        let two = one.clone();
        assert!(one.same_as(&two) && one.holders() == 2);
        let other = SharedFonts::ready(Arc::new(FontDb::bundled()));
        assert!(!one.same_as(&other));
        assert!(Arc::ptr_eq(&one.get(), &two.get()));
    }
}
