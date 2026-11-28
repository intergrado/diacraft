//! Integration tests of `diacraft-text`: the vectors, then hostile input.
//!
//! No test depends on a font installed on the machine. Faces are built in code by `diacraft_fonts::synth` (a letter exactly 500
//! or 1,000 units wide, a face with no bold, a face that lacks a Han character), and the few tests that use the bundled faces
//! assert nothing a machine's fonts could change. Lengths are paper inches; `pt` turns points into them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod bidi;
mod bullets;
mod case;
mod chars;
mod contract;
mod edges;
mod equivalents;
mod faces;
mod hostile;
mod layout_block;
mod notes;
mod paragraphs;
mod real_fonts;
mod rtl;
mod rules;
mod runs;
mod tabs;
mod vectors;
mod vertical;
mod wrap;

use std::sync::Arc;

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, Line, ParaFormat, Run, lay_out};

/// Points to the paper inches the cells hold.
pub fn pt(v: f64) -> f64 {
    v / 72.0
}

/// Compare within 1E-6, the tolerance of the vectors.
#[track_caller]
pub fn close(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6, "got {got}, wanted {want}");
}

/// A database of only these synthetic faces.
pub fn db(fonts: &[SynthFont]) -> FontDb {
    let mut db = FontDb::empty();
    for f in fonts {
        assert_eq!(db.add_font_data(f.build()).added, 1, "{} should parse", f.family);
    }
    db
}

/// One face, `Em`, in which every printable ASCII character, and a no-break space, advances exactly one em.
pub fn em_db() -> FontDb {
    db(&[SynthFont::new("Em").ascii(1000).chars("\u{00A0}", 1000)])
}

/// A character format in face `font` at `size_pt` points, the root style's values otherwise.
pub fn cf(font: &str, size_pt: f64) -> CharFormat {
    CharFormat { font: font.to_owned(), size: pt(size_pt), ..CharFormat::default() }
}

/// Left-aligned paragraphs: most vectors that look at where a line starts want a known side.
pub fn left() -> ParaFormat {
    ParaFormat { halign: 0, ..ParaFormat::default() }
}

/// A run with these formats.
pub fn run(text: &str, c: &CharFormat, p: &ParaFormat) -> Run {
    Run::new(text, Arc::new(c.clone()), Arc::new(p.clone()))
}

/// The block of the stacking vectors: margins of 4 pt and a height given by the caller.
pub fn block4(height: f64) -> BlockFormat {
    BlockFormat {
        left_margin: pt(4.0),
        right_margin: pt(4.0),
        top_margin: pt(4.0),
        bottom_margin: pt(4.0),
        height: Some(height),
        ..BlockFormat::default()
    }
}

/// Lay out `text` as one run.
pub fn lay_text(db: &FontDb, text: &str, c: &CharFormat, p: &ParaFormat, block: &BlockFormat, width: Option<f64>) -> Laid {
    lay_out(&[run(text, c, p)], block, width, db)
}

/// The text of a line, cut from the source by the line's character range.
pub fn line_text(source: &str, line: &Line) -> String {
    source.chars().skip(line.chars.start).take(line.chars.end - line.chars.start).collect()
}

/// The baselines, top line first.
pub fn baselines(laid: &Laid) -> Vec<f64> {
    laid.lines.iter().map(|l| l.baseline).collect()
}

/// The glyph ids of a line's runs, in order.
pub fn gids(line: &Line) -> Vec<u32> {
    line.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.gid)).collect()
}

/// How many glyphs a line draws.
pub fn glyph_count(line: &Line) -> usize {
    line.runs.iter().map(|r| r.glyphs.len()).sum()
}
