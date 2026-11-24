//! The window's start: the font scan on a thread of its own at the top of `main`, then the first drawing in the creator.
//!
//! **The first frame never waits for the faces.** Reading the machine's font folders takes about 6 ms warm and seconds on a cold disk (the scan
//! stops at eight), and the window must open at once. [`begin`] starts the scan on a thread of its own before `eframe` is started; [`start`], in
//! `eframe`'s creator, opens the session and the first drawing without waiting for it. What waits is whatever needs the faces: a commit that
//! measures text, the first time (the engine asks for them then, `diacraft_engine::measure`), and the page's picture, the Print view's sheets,
//! a print and the Font box, each on a thread of its own (`DiacraftApp::with_fonts`). So nothing measures in the bundled faces once the window
//! can show the machine's. A scan that panics falls back to the bundled faces, with a warning in the log.

use diacraft_engine::Session;
use diacraft_engine::measure::{self, SharedFonts};
use diacraft_fonts::FontDb;

/// What the window starts with.
pub struct Started {
    /// A session on an empty new drawing: the window opens on one and not on a screen with nothing in it.
    pub session: Session,
    /// The faces, which may still be being read: shared with the engine's commits and given to the window.
    pub fonts: SharedFonts,
}

/// Starts reading the machine's faces and the bundled ones on a thread of its own and shares them with the engine; returns at once. The first
/// thing in `main`, before `eframe` is started.
pub fn begin() -> SharedFonts {
    begin_with(FontDb::system_and_bundled)
}

/// As [`begin`], with `scan` for the work on the thread: the seam the tests hold a scan back or make it fail with.
pub fn begin_with(scan: impl FnOnce() -> FontDb + Send + 'static) -> SharedFonts {
    measure::scan_fonts_with(scan)
}

/// `eframe`'s creator, up to the window: the session with its first drawing. It does not wait for the faces: a new drawing measures no text.
pub fn start(fonts: SharedFonts) -> Started {
    let mut session = Session::new();
    if let Err(e) = session.execute("file.new", &serde_json::json!({})) {
        log::warn!("could not start a new drawing: {e}");
    }
    Started { session, fonts }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::*;

    /// The creator returns while a slow scan (held back here, as a cold disk would) is still going: the first frame does not wait for the
    /// faces. The scan is let go afterwards, and then everything has the one database it found.
    #[test]
    fn the_creator_returns_while_a_slow_scan_is_still_going() {
        let (release, gate) = channel::<()>();
        let fonts = begin_with(move || {
            let _ = gate.recv();
            FontDb::empty()
        });
        let (tx, rx) = channel();
        let creator = fonts.clone();
        std::thread::spawn(move || {
            let started = start(creator);
            let _ = tx.send(started.session.document().is_some());
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(true), "the creator waited for the scan, or did not open the first drawing");
        assert!(!fonts.is_ready(), "and the scan is still going");
        release.send(()).unwrap();
        let found = fonts.get();
        assert_eq!(found.face_count(), FontDb::empty().face_count(), "the scan's own database");
        assert!(Arc::ptr_eq(&measure::measure_fonts().unwrap(), &found), "the engine's commits have it too");
        measure::set_measure_fonts(None);
    }

    /// A scan that panics (a font file that breaks the reader) does not stop the app: the faces are the bundled ones, for the window and the
    /// engine alike.
    #[test]
    fn a_scan_that_panics_falls_back_to_the_bundled_faces() {
        let fonts = begin_with(|| panic!("a font the scan cannot read"));
        let started = start(fonts.clone());
        assert!(started.session.document().is_some(), "the window still opens on a drawing");
        let found = fonts.get();
        assert_eq!(found.face_count(), FontDb::bundled().face_count(), "the bundled faces");
        assert!(Arc::ptr_eq(&measure::measure_fonts().unwrap(), &found) && Arc::ptr_eq(&measure::renderer_fonts(), &found));
        measure::set_measure_fonts(None);
    }

    /// The real scan: `begin` shares what it finds with the engine, the machine's faces and the bundled ones.
    #[test]
    fn begin_shares_the_machines_faces_with_the_engine() {
        let fonts = begin();
        let found = fonts.get();
        assert!(found.face_count() >= FontDb::bundled().face_count());
        assert!(Arc::ptr_eq(&measure::measure_fonts().unwrap(), &found));
        measure::set_measure_fonts(None);
    }
}
