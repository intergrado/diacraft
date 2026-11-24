//! The face a run is shown in (gap U21): what the window's Font box and `shape.formatting` name for a run's text.
//!
//! Our rule. The name is the one the renderer sets the run in:
//! `diacraft_render::FaceReader::named_font` reads it by the function the renderer's own layout uses (the `Font` cell of the run's `Character` row
//! through the shape, its master and the style chain; the theme's font where the cell holds the marker; the root style's `Font` where the cell is
//! 0 or empty), so the two cannot read different cells. The name is **the document's or the theme's, as written**: Calibri stays Calibri where
//! Carlito draws it (keeps the cell and the font table as they are). When no cell names a face, the renderer draws the font database's
//! default for an unnamed run (`FontDb::resolve("")`) and this names that face's family (`FontDb::default_family`), which depends on the faces
//! the machine has: it is the name of what is drawn, since nothing in the document names one.
//!
//! **Which database.** The default is the database's, and the renderer's database is the application's. The query asks the database the application
//! shared with the engine (`measure::set_measure_fonts`, `start_system_fonts`: the one its renderer draws in) **if it is in hand, and never waits for
//! it**: the window reads this in its frame, and the scan of the machine's faces can take seconds. While the scan is going, or when nothing is shared
//! (a library, a test, the control channel without a window), it asks the bundled faces, the same on every machine. A caller that has the renderer's
//! database asks [`look_of_in`](super::look_of_in) with it, and a view of the window that was read before the scan ended is read again when it has.

use std::cell::OnceCell;
use std::sync::{Arc, OnceLock};

use diacraft_doc::ShapeRef;
use diacraft_fonts::FontDb;

/// The faces `shape.formatting` names a default from: the application's shared database when it is in hand, else the bundled faces, built once.
pub(super) fn query_fonts() -> Arc<FontDb> {
    static BUNDLED: OnceLock<Arc<FontDb>> = OnceLock::new();
    crate::measure::measure_fonts_now().unwrap_or_else(|| Arc::clone(BUNDLED.get_or_init(|| Arc::new(FontDb::bundled()))))
}

/// The face names of the runs of one document, as the window shows them.
pub(super) struct Faces<'r> {
    named: &'r dyn Fn(&ShapeRef, u32) -> Option<String>,
    fonts: &'r FontDb,
    /// The name of the default, asked of the database once for all the runs a call reads.
    default: OnceCell<Option<String>>,
}

impl<'r> Faces<'r> {
    /// `named` reads the face the cells of a run name; `fonts` is the database the renderer would draw an unnamed run in.
    pub(super) fn new(named: &'r dyn Fn(&ShapeRef, u32) -> Option<String>, fonts: &'r FontDb) -> Faces<'r> {
        Faces { named, fonts, default: OnceCell::new() }
    }

    /// The face the run `run` of the shape `at` is set in: the name its cells give, else the name of the default.
    pub(super) fn font_of(&self, at: &ShapeRef, run: u32) -> Option<String> {
        (self.named)(at, run).or_else(|| self.default.get_or_init(|| self.default_name()).clone())
    }

    fn default_name(&self) -> Option<String> {
        #[cfg(test)]
        super::counts::default_looked_up();
        self.fonts.default_family()
    }
}
