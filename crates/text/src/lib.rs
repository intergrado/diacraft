//! DiaCraft text layout: a shape's runs, with the formats the caller resolved from its ShapeSheet, laid out as the specs say
//! into lines of positioned glyphs.
//!
//! [`lay_out`] is the entry point. It takes the runs, the block's format, the block's width and a [`FontDb`](diacraft_fonts::FontDb),
//! and gives a [`Laid`] in the block's own frame. It draws nothing: the renderer draws what [`Laid`] holds.
//!
//! **What the layout does:** splits the text into paragraphs at line feeds and into lines at U+2028; reads each
//! paragraph's format from the run holding its last character; picks a face for every grapheme cluster by script, through
//! `Font`, `AsianFont` and `ComplexScriptFont` and then any face that has the glyph; sizes raised and lowered letters,
//! small capitals and complex script; shapes at nominal advances, with no kerning; breaks
//! lines at Unicode opportunities, greedily, trimming trailing spaces and splitting a word too wide for the line;
//! orders right-to-left text by the Unicode bidirectional algorithm; sets each line from its largest size, `SpLine` and
//! the baseline three quarters down its box; and aligns lines inside margins and indents. A right-to-left
//! paragraph (`Flags` 1) mirrors: its indents, alignment, bullet and tab stops are the mirror image of a left-to-right one's (see the `place` module).
//! It places tabs, bullets, underlines and strikeouts, and the one rectangle of a text background.
//!
//! Vertical text (`TextDirection` 1) is laid out down the block: columns from the right edge to the left, an East Asian letter upright and
//! the others on their sides ([`GlyphRun::turned`]), by the layout across in a block turned the other way up (see the `vertical` module),
//! with the underline of a Japanese or Korean run on the right of its column.
//!
//! **What it doesn't do:** a field's evaluation: a field's displayed characters are just a run.
//!
//! **Hostile input.** Text, formats and font names come from a drawing, so every size is capped (see [`limits`]), every number is
//! checked, and the work grows linearly with the text: a very long word, thousands of format runs, a block of no width, deep
//! bidirectional nesting and stacks of combining marks are all tested, the line breaker, the shaper and the search for a face by
//! counts of their work ([`work`]) and the whole layout by a bound that only a hang reaches. A cluster is never split across faces,
//! no layout recurses, and nothing here panics.
//!
//! **Without a real font.** The faces a drawing names are usually Microsoft's, which are never bundled, so on most machines text
//! is laid out in a stand-in: `fonts` picks a metric-compatible open face where one is installed and a bundled face otherwise. Line
//! heights and baselines don't depend on the face, but widths do, so where text wraps and how wide a block
//! measures can differ from the original. A stored `TxtWidth` stands at load.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bidi;
mod breaking;
mod bullet;
mod emit;
mod formats;
mod laid;
mod layout;
pub mod limits;
mod model;
mod place;
mod prepare;
mod shape;
mod vertical;
pub mod work;

pub use laid::{Background, Decoration, DecorationKind, GlyphRun, Laid, Line, Note, PlacedGlyph};
pub use layout::lay_out;
pub use model::{BlockFormat, CharFormat, ParaFormat, Rgba, Run, TabStop, style};
