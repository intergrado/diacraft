//! DiaCraft SVG export: a page of a document written as an SVG file.
//!
//! [`export_page`] draws a page through the same walk the window uses (`diacraft_render`), onto a writer that implements the renderer's
//! `Target`, so the file shows what the window shows and the list of what was left out or drawn by a stand-in is the window's too
//! ([`Exported::skipped`]).
//!
//! **What the file holds.** Every fill and outline is a `<path>` in CSS pixels (96 to the inch by default) with the page's transform applied.
//! Gradients are `linearGradient` and `radialGradient`, clips are `clipPath`, a blurred shadow is a group with a Gaussian blur filter, a
//! picture is an `<image>` with a PNG in a data URI, and the paper is a white rectangle (`SvgOptions::paper`).
//!
//! **Text is text where the font allows it** (`text.rs`): a run whose letters are one glyph each is a `<text>` element naming the face, with
//! the x of every letter, so a viewer that has the face draws what we laid out and the letters can be selected and searched. Anything else
//! (ligatures, joined and right-to-left scripts, a made-up bold or slant, a letter the face lacks) is outlines, and counted as
//! `TextAsOutlines`. [`TextMode::AsOutlines`] writes every letter as outlines, for a file that must look the same on any machine.
//!
//! **Alt text.** The file says what the SVG standard says it with. A shape that is drawn is a `<g>` whose first child is a `<title>` holding the shape's alt text title
//! and next a `<desc>` holding its description, written as they are and escaped, and holding the shape's drawing and its members' (a group's `<g>` holds its members'). A shape with no alt text at all
//! has its name as the `<title>` and no `<desc>`, and one with no name writes no group. The root `<svg>` has the page's title as its `<title>` when the page has one (else the export's title option),
//! and the page's description as its `<desc>`. A title or description of white space only is never written. An export of the same page with no alt text and no names is the file it was.
//!
//! **Not in the file:** the grid, rulers, guides and selection, and shapes of a layer that is hidden.
//!
//! **Hostile input.** A drawing is as hostile as the file it came from. The walk bounds the shapes, rows, glyphs and time (the export's
//! limits, `diacraft_render::export::export_limits`), every path is checked against the coordinates a file may hold before it is written, a
//! path that reaches far past the page is cut to it (straight sides) or left out and counted, and the file is held to
//! [`diacraft_render::export::MAX_EXPORT_BYTES`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod num;
mod target;
mod text;

use diacraft_color::Rgba;
use diacraft_doc::{Document, SheetRef};
use diacraft_render::Renderer;
use diacraft_render::export::{PageLayout, draw_page};

use crate::target::SvgTarget;

pub use diacraft_render::export::{ExportError, Exported, TextMode};

/// What [`export_page`] is asked.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgOptions {
    /// CSS pixels to a paper inch: the file's size is the page's size in inches times this. 96 is CSS's own.
    pub px_per_inch: f64,
    /// The colour of the page's paper, drawn first; `None` leaves the page transparent.
    pub paper: Option<Rgba>,
    pub text: TextMode,
    /// The `<title>` of the file.
    pub title: Option<String>,
}

impl Default for SvgOptions {
    fn default() -> SvgOptions {
        SvgOptions { px_per_inch: 96.0, paper: Some(Rgba::WHITE), text: TextMode::AsText, title: None }
    }
}

/// Writes `page` of `doc` as SVG.
///
/// The renderer is the caller's: it holds the fonts the text is laid out in and what it keeps between pages. Its paper colour and limits
/// are put back as they were. Never panics: a bug in drawing is an [`ExportError::Bug`], and nothing is written.
pub fn export_page(renderer: &mut Renderer, doc: &Document, page: u32, options: &SvgOptions) -> Result<Exported, ExportError> {
    let layout = PageLayout::of(doc, page, options.px_per_inch)?;
    let mut target = SvgTarget::new(layout.to_px, (layout.width, layout.height), options.text);
    let (width, height) = layout.whole();
    let skipped = draw_page(renderer, doc, page, options.paper, &mut target)?;
    // The page's alt text is the file's own title and description; a page with no title leaves the `<title>` to the option.
    let alt = diacraft_doc::alt_text::read_in(doc, &SheetRef::Page(page));
    let title = if alt.title.has_text() { Some(alt.title.text) } else { options.title.as_deref() };
    let svg = target.finish(title, Some(alt.description.text))?;
    Ok(Exported { bytes: svg.into_bytes(), width, height, skipped })
}
