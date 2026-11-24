//! Integration tests of `diacraft-fonts`.
//!
//! No test here depends on a font installed on the machine: faces are the bundled ones, or built in code by `synth`, and the
//! one test that reads the machine's own font folders asserts nothing about what it finds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod add;
mod boundaries;
mod build_input;
mod composites;
mod coverage;
mod hostile;
mod limits;
mod real_faces;
mod resolve;
mod script;
mod search;
mod shape;
mod synth_font;
mod system;

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, FontStyle};

/// A database of the bundled faces and nothing else, so lookups don't depend on the machine.
pub fn bundled() -> FontDb {
    FontDb::bundled()
}

/// A database holding only the given synthetic faces (no bundled ones either).
pub fn only(fonts: &[SynthFont]) -> FontDb {
    let mut db = FontDb::empty();
    for f in fonts {
        let report = db.add_font_data(f.build());
        assert_eq!(report.added, 1, "{} should parse", f.family);
    }
    db
}

pub const REGULAR: FontStyle = FontStyle { bold: false, italic: false };
pub const BOLD: FontStyle = FontStyle { bold: true, italic: false };
pub const ITALIC: FontStyle = FontStyle { bold: false, italic: true };
