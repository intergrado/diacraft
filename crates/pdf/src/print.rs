//! A print as a PDF: one PDF page for each sheet of paper, as large as the paper, with the page's part for that sheet drawn on it.
//!
//! This is what a printer is given, and what a test or a person can open to see what would be printed: [`export_sheets`] takes sheets planned by
//! `diacraft_render::print` and draws each one through the same walk as [`crate::export_page`], onto a PDF page of the sheet's size (72 points to
//! the inch, the sheet's top left corner at the page's), clipped to the sheet's printable part, with non-printing shapes and unprinted layers left
//! off and no paper drawn (the printer's is the paper). A printer takes the PDF at its own resolution: every fill and outline is a vector.
//!
//! **Cost.** Each sheet is a walk of its page, which skips the shapes that lie outside the sheet but still places each one, so a print costs
//! the sheets times the shapes; a sheet has the export's time limits, and the print as a whole has [`MAX_PRINT_TIME`], past which it fails in
//! words and writes nothing (a print with sheets missing is worse than none).
//!
//! **Memory.** A document is built whole in memory before it is written, so a drawing that tiles across many sheets must not be allowed to
//! build a sheet's worth of a large shape for every sheet. Two things hold it. **What a sheet's clip hides is not written** (a path whose box
//! misses the printable part is left out, and a straight-sided path of many elements is cut to it, `PdfTarget::fitted`), so a shape that
//! spans thirty tiles costs about what it costs once. And **the content is counted as it is written** (`PdfTarget::charge`): a print may
//! hold [`MAX_PRINT_BYTES`] of it (or `PrintOptions::max_bytes`), the sheet being drawn is given what the sheets before it have left, and when it
//! passes that the sheet stops writing and the print fails before the next sheet is begun, in words that say how far it got.

use std::time::Duration;

use craft_pdf::write::{self, Metadata, Writer};
use diacraft_doc::Document;
use diacraft_render::print::{PrintColor, Sheet, draw_sheet};
use diacraft_render::{Renderer, SkipKind, Skipped};
use kurbo::Affine;
use web_time::Instant;

use crate::target::PdfTarget;
use crate::{ExportError, Failure, POINTS_PER_INCH, TextMode, krilla};

/// The longest a print may take in all, whatever its sheets: ten minutes. Each sheet is also held to the export's own limits.
pub const MAX_PRINT_TIME: Duration = Duration::from_secs(600);

/// The most bytes of drawing a print holds (the content of its pages as [`PdfTarget`] counts it, which is about what its streams take before
/// they are compressed) and so the most memory it builds: 256 MiB. A print past it is not one a printer takes.
pub const MAX_PRINT_BYTES: usize = 256 * 1024 * 1024;

/// What [`export_sheets`] is asked.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintOptions {
    pub text: TextMode,
    /// The document's title: the job's name in a print queue.
    pub title: Option<String>,
    /// Compress the pages' content. Off, the content is plain operators: for looking at a file, and for the tests.
    pub compress: bool,
    /// The most bytes of drawing the print may hold, counted as the module documentation says; at most [`MAX_PRINT_BYTES`].
    pub max_bytes: usize,
    /// Colour, or black and white: [`PrintColor::Grayscale`] writes every colour as its grey (the Print view's Black and white).
    pub color: PrintColor,
}

impl Default for PrintOptions {
    fn default() -> PrintOptions {
        PrintOptions { text: TextMode::AsText, title: None, compress: true, max_bytes: MAX_PRINT_BYTES, color: PrintColor::Color }
    }
}

/// A finished print.
#[derive(Clone, Debug, PartialEq)]
pub struct Printed {
    /// The PDF: one page for each sheet, in order.
    pub bytes: Vec<u8>,
    pub sheets: usize,
    /// What was left out or drawn by a stand-in, summed over the sheets, as an export's list is.
    pub skipped: Vec<Skipped>,
}

/// A size in bytes as a person reads it: megabytes from a million, else kilobytes.
fn size_in_words(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{} megabytes", bytes / 1_000_000) } else { format!("{} kilobytes", bytes / 1_000) }
}

/// Adds `more` to `all`, one entry for each kind with the counts added and the first of the first.
fn merge(all: &mut Vec<Skipped>, more: Vec<Skipped>) {
    for s in more {
        match all.iter_mut().find(|k| k.kind == s.kind) {
            Some(known) => known.count = known.count.saturating_add(s.count),
            None => all.push(s),
        }
    }
}

/// Writes `sheets` of `doc` as a PDF with a page for each, drawn with `renderer` (which holds the fonts the text is laid out in), in the
/// colours [`PrintOptions::color`] says. The renderer's limits, paper, mode and print colour are put back as they were. Never panics: a bug in
/// drawing is an [`ExportError::Bug`], a panic in the writer is an [`ExportError::Encode`], and nothing is written.
pub fn export_sheets(renderer: &mut Renderer, doc: &Document, sheets: &[Sheet], options: &PrintOptions) -> Result<Printed, ExportError> {
    if sheets.is_empty() {
        return Err(ExportError::Encode("there are no sheets to print".to_owned()));
    }
    let deadline = Instant::now() + MAX_PRINT_TIME;
    let kept_color = renderer.print_color();
    renderer.set_print_color(options.color);
    let written = write::guard(|| -> Result<_, Failure> {
        let settings = krilla::SerializeSettings { compress_content_streams: options.compress, ..Default::default() };
        let metadata = Metadata {
            creator: "DiaCraft".to_owned(),
            producer: "DiaCraft".to_owned(),
            title: options.title.clone().filter(|t| !t.trim().is_empty()),
            ..Metadata::default()
        };
        let mut writer = Writer::new(settings, metadata);
        let mut skipped: Vec<Skipped> = Vec::new();
        let limit = u64::try_from(options.max_bytes.min(MAX_PRINT_BYTES)).unwrap_or(u64::MAX);
        let mut spent = 0u64;
        for (n, sheet) in sheets.iter().enumerate() {
            if Instant::now() > deadline {
                return Err(ExportError::TooLarge(format!("the print took more than {} seconds", MAX_PRINT_TIME.as_secs())).into());
            }
            let (width, height) = (sheet.width * POINTS_PER_INCH, sheet.height * POINTS_PER_INCH);
            let size = krilla::geom::Size::from_wh(width as f32, height as f32)
                .ok_or_else(|| ExportError::TooLarge(format!("a sheet of {} by {} inches", sheet.width, sheet.height)))?;
            let mut pdf_page = writer.document_mut().start_page_with(krilla::page::PageSettings::new(size));
            let drawn = {
                let to_px = Affine::scale(POINTS_PER_INCH) * sheet.to_sheet;
                let mut target = PdfTarget::new(pdf_page.surface(), to_px, (width, height), options.text)
                    .with_print_limits(sheet.clip_rect_in_page(), limit.saturating_sub(spent));
                let drawn = draw_sheet(renderer, doc, sheet, &mut target);
                spent = spent.saturating_add(target.written());
                // The sheet that passed the budget stopped writing; the print stops here, before another sheet adds to what is held.
                let finished = target.finish();
                match (drawn, finished) {
                    (Err(e), _) | (Ok(_), Err(e @ ExportError::Bug(_) | e @ ExportError::Encode(_))) => return Err(e.into()),
                    (Ok(_), Err(ExportError::TooLarge(_))) => {
                        return Err(ExportError::TooLarge(format!(
                            "the drawing on the sheets passed the {} a print may hold, at sheet {} of {}; print fewer pages, or \
                             print a page at a smaller scale",
                            size_in_words(limit),
                            n + 1,
                            sheets.len()
                        ))
                        .into());
                    }
                    (Ok(_), Err(e)) => return Err(e.into()),
                    (Ok(drawn), Ok(())) => drawn,
                }
            };
            pdf_page.finish();
            if drawn.iter().any(|s| s.kind == SkipKind::Cancelled) {
                return Err(ExportError::Encode("the print was stopped".to_owned()).into());
            }
            merge(&mut skipped, drawn);
        }
        let bytes = writer.finish()?;
        // What the count could not see (fonts, the file's own structure) is judged by the file: compression only makes the content smaller.
        if bytes.len() > 2 * MAX_PRINT_BYTES {
            return Err(ExportError::TooLarge(format!("the print is {} bytes, past the {} a print may be", bytes.len(), 2 * MAX_PRINT_BYTES)).into());
        }
        Ok((bytes, skipped))
    })
    .map_err(|Failure(e)| e);
    renderer.set_print_color(kept_color);
    let (bytes, skipped) = written?;
    Ok(Printed { bytes, sheets: sheets.len(), skipped })
}
