//! The font families the Font box lists: the machine's, found on a thread of their own.
//!
//! Scanning the machine's font folders reads thousands of files on a crowded machine, so it never runs on the window's thread (gap U8 tells
//! the same story for the page's pictures): the first time the box is opened the scan starts, the box says "Loading fonts" until the list
//! arrives, and the window asks for a frame when it does. On the web there are no folders to read, so the list is the bundled faces and is
//! made on the spot. Tests give the scan a list of their own ([`FontList::with_scan`]).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What finds the families: runs on the scan's thread.
pub type Scan = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// The most families the box keeps: a machine with more than this has more than a person scrolls.
const MAX_FAMILIES: usize = 10_000;

/// The scan that reads the machine's fonts and the bundled ones.
fn system_scan() -> Scan {
    Arc::new(|| {
        if cfg!(target_arch = "wasm32") {
            diacraft_fonts::FontDb::bundled().families()
        } else {
            diacraft_fonts::FontDb::system_and_bundled().families()
        }
    })
}

/// The scan that lists the families of the faces the application found once and shares (`DiacraftApp::with_fonts`): no folder is read again.
/// It runs on the Font box's own thread, which waits there for the faces if they are still being read.
pub fn shared_scan(fonts: diacraft_engine::measure::SharedFonts) -> Scan {
    Arc::new(move || fonts.get().families())
}

/// The families of the machine, once they are known.
pub struct FontList {
    scan: Scan,
    families: Option<Arc<Vec<String>>>,
    waiting: Option<Receiver<Vec<String>>>,
    started: bool,
}

impl Default for FontList {
    fn default() -> Self {
        FontList { scan: system_scan(), families: None, waiting: None, started: false }
    }
}

impl FontList {
    /// A list that is found by `scan`.
    pub fn with_scan(scan: Scan) -> FontList {
        FontList { scan, ..FontList::default() }
    }

    /// Starts the scan if it has not been started: on a thread, and at once where there are none (the web).
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let scan = Arc::clone(&self.scan);
        if cfg!(target_arch = "wasm32") {
            self.families = Some(Arc::new(tidy(scan())));
            return;
        }
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-fonts".into()).spawn(move || {
            // A scan that panics is a list with nothing in it: the box then offers only the face the shape has.
            let found = diacraft_engine::guard::catch_panic(|| scan()).unwrap_or_default();
            // A send that fails means the window is gone (the list was dropped with it): there is nobody to tell.
            let _ = tx.send(found);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.waiting = Some(rx),
            Err(_) => self.families = Some(Arc::new(Vec::new())),
        }
    }

    /// Takes the answer of the scan if it has come. → whether the scan is still going.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(rx) = &self.waiting else { return false };
        match rx.try_recv() {
            Ok(found) => {
                self.families = Some(Arc::new(tidy(found)));
                self.waiting = None;
                ctx.request_repaint();
                false
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                true
            }
            Err(TryRecvError::Disconnected) => {
                self.families = Some(Arc::new(Vec::new()));
                self.waiting = None;
                false
            }
        }
    }

    /// The families, once the scan has answered.
    pub fn families(&self) -> Option<Arc<Vec<String>>> {
        self.families.clone()
    }

    /// Whether a scan was started and has not answered yet.
    pub fn scanning(&self) -> bool {
        self.waiting.is_some()
    }
}

/// The list as the box shows it: sorted without regard to case, each name once, no blank names and no more than [`MAX_FAMILIES`].
///
/// A name that begins with a full stop is not listed: macOS keeps those for its own interface (`.Aqua Kana`, `.SF NS`) and shows them in no font
/// menu, and on a Mac they came first in the box, above every font a person would pick. A drawing that names one still draws with it.
fn tidy(mut found: Vec<String>) -> Vec<String> {
    found.retain(|name| !name.trim().is_empty() && !name.starts_with('.'));
    found.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
    found.dedup();
    found.truncate(MAX_FAMILIES);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_sorted_without_regard_to_case_and_has_each_name_once() {
        let list = tidy(vec!["beta".into(), "Alpha".into(), "alpha".into(), "Alpha".into(), " ".into(), "Gamma".into()]);
        assert_eq!(list, ["Alpha", "alpha", "beta", "Gamma"]);
    }

    #[test]
    fn the_systems_own_hidden_families_are_not_listed() {
        // What a Mac's scan begins with: macOS keeps names that begin with a full stop for its own use and shows them in no font menu.
        let list = tidy(vec![".ADT Slab Numeric".into(), ".Aqua Kana".into(), "Apple SD Gothic Neo".into(), ".SF NS".into(), "Helvetica".into()]);
        assert_eq!(list, ["Apple SD Gothic Neo", "Helvetica"]);
    }

    #[test]
    fn the_scan_runs_on_a_thread_and_the_answer_is_taken_by_the_window() {
        let ctx = egui::Context::default();
        let (release, held) = channel::<()>();
        let held = std::sync::Mutex::new(held);
        let mut list = FontList::with_scan(Arc::new(move || {
            // Held until the test lets it go: the window's thread is not the one that waits.
            let _ = held.lock().map(|rx| rx.recv());
            vec!["Hack".to_owned(), "Ubuntu Light".to_owned()]
        }));
        assert!(list.families().is_none() && !list.scanning());
        list.start(&ctx);
        assert!(list.scanning() && list.families().is_none(), "the scan is going, and nothing waited for it");
        assert!(list.poll(&ctx));
        release.send(()).unwrap();
        for _ in 0..500 {
            if !list.poll(&ctx) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        assert_eq!(list.families().unwrap().as_slice(), ["Hack", "Ubuntu Light"]);
        assert!(!list.scanning());
    }
}
