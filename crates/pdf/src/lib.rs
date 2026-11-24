//! DiaCraft PDF export: a page of a document written as a vector PDF.
//!
//! [`export_page`] draws a page through the same walk the window uses (`diacraft_render`), onto a writer that implements the renderer's
//! `Target` over `craft-pdf`'s Krilla, so the file shows what the window shows and the list of what was left out or drawn by a stand-in is
//! the window's too ([`Exported::skipped`]). The page is the page's own size, at 72 points to the inch.
//!
//! **What the file holds.** Every fill and outline is a vector path in points; gradients are PDF shadings; clips are clipping paths; a
//! picture is an image XObject; the paper is a white rectangle ([`PdfOptions::paper`]).
//!
//! **Text is text where the font allows it** (`text.rs`): each run whose face may be embedded (its `OS/2` `fsType` allows it) is written with
//! a subset of the face embedded, one glyph at a time at the place the layout gave it, with the characters each glyph stands for, so the text can
//! be selected, searched and copied and looks as laid out on any machine. A run in a face that may not be embedded, a made-up bold or slant, or
//! raised marks are outlines, counted as `TextAsOutlines`. [`TextMode::AsOutlines`] writes every letter as outlines.
//!
//! **Not in the file:** the grid, rulers, guides and selection, and shapes of a layer that is hidden. **Approximated, and
//! said:** a blurred shadow is drawn hard-edged (PDF has no blur; `ShadowApproximate`).
//!
//! **Hostile input.** A drawing is as hostile as the file it came from. The walk bounds the shapes, rows, glyphs and time (the export's
//! limits, `diacraft_render::export::export_limits`); a path is checked against the coordinates a PDF may hold before it is written, a path
//! that reaches far past the page is cut to it (straight sides) or left out and counted; the whole export runs under `craft-pdf`'s guard, so a
//! panic in the writer is an error and not a crash; and a picture or a font the writer could not take fails the export instead of vanishing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod print;
mod target;
mod text;

use craft_pdf::write::{self, Metadata, Writer, backend as krilla};
use diacraft_color::Rgba;
use diacraft_doc::Document;
use diacraft_render::Renderer;
use diacraft_render::export::{PageLayout, draw_page};
use krilla::geom::Size;
use krilla::page::PageSettings;

use crate::target::PdfTarget;

pub use crate::print::{PrintOptions, Printed, export_sheets};
pub use diacraft_render::export::{ExportError, Exported, TextMode};

/// Points to a paper inch: a PDF's own unit.
pub const POINTS_PER_INCH: f64 = 72.0;

/// What [`export_page`] is asked.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfOptions {
    /// The colour of the page's paper, drawn first; `None` leaves the page without a ground.
    pub paper: Option<Rgba>,
    pub text: TextMode,
    /// The document's title.
    pub title: Option<String>,
    /// Compress the page's content (small files). Off, the content is plain operators: for looking at a file, and for the tests.
    pub compress: bool,
}

impl Default for PdfOptions {
    fn default() -> PdfOptions {
        PdfOptions { paper: Some(Rgba::WHITE), text: TextMode::AsText, title: None, compress: true }
    }
}

/// An error from the writer or from the page, in one type for `craft-pdf`'s guard.
struct Failure(ExportError);

impl From<write::WriteError> for Failure {
    fn from(e: write::WriteError) -> Failure {
        Failure(ExportError::Encode(e.to_string()))
    }
}

impl From<ExportError> for Failure {
    fn from(e: ExportError) -> Failure {
        Failure(e)
    }
}

/// Writes `page` of `doc` as a one-page PDF.
///
/// The renderer is the caller's: it holds the fonts the text is laid out in and what it keeps between pages. Its paper colour and limits
/// are put back as they were. Never panics: a bug in drawing is an [`ExportError::Bug`], a panic in the PDF writer is an
/// [`ExportError::Encode`], and nothing is written.
pub fn export_page(renderer: &mut Renderer, doc: &Document, page: u32, options: &PdfOptions) -> Result<Exported, ExportError> {
    let layout = PageLayout::of(doc, page, POINTS_PER_INCH)?;
    let (width, height) = layout.whole();
    let size = Size::from_wh(layout.width as f32, layout.height as f32)
        .ok_or_else(|| ExportError::TooLarge(format!("a page of {} by {} points", layout.width, layout.height)))?;
    let (bytes, skipped) = write::guard(|| -> Result<_, Failure> {
        let settings = krilla::SerializeSettings { compress_content_streams: options.compress, ..Default::default() };
        let metadata = Metadata {
            creator: "DiaCraft".to_owned(),
            producer: "DiaCraft".to_owned(),
            title: options.title.clone().filter(|t| !t.trim().is_empty()),
            ..Metadata::default()
        };
        let mut writer = Writer::new(settings, metadata);
        let mut pdf_page = writer.document_mut().start_page_with(PageSettings::new(size));
        let skipped = {
            let mut target = PdfTarget::new(pdf_page.surface(), layout.to_px, (layout.width, layout.height), options.text);
            let skipped = draw_page(renderer, doc, page, options.paper, &mut target)?;
            target.finish()?;
            skipped
        };
        pdf_page.finish();
        Ok((writer.finish()?, skipped))
    })
    .map_err(|Failure(e)| e)?;
    Ok(Exported { bytes, width, height, skipped })
}
