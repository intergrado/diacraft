//! Printing: what a print of the drawing is, planned and written as a PDF.
//!
//! The engine never reaches a printer. It works out which sheets of which paper a print takes ([`plan`]), describes them for a program
//! ([`describe`], `file.print_preview`), and writes what would be sent as a PDF with a page for each sheet ([`pdf`], `file.print`). A window
//! hands those bytes to the system's spooler, a program to a file; the plan and the bytes are the same either way, so a test of the file is a
//! test of the print. The rules are in `diacraft_render::print`.

use std::ops::Range;
use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_render::export::ExportError;
use diacraft_render::print::{plan_job, printable_pages};
use diacraft_render::{Renderer, WorkedCache};
use serde_json::{Value, json};

pub use diacraft_pdf::{PrintOptions, Printed, TextMode};
pub use diacraft_render::print::{
    JobPlan, MAX_PAGES_PER_JOB, MAX_SHEETS_PER_JOB, MAX_SHEETS_PER_PAGE, Orientation, PagePlan, Paper, PrintColor, PrintError, Run, Sheet, paper,
};

/// Which pages a print covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pages {
    /// Every page that prints: the foreground pages, in file order, but not a page that is hidden.
    All,
    /// This one page, whichever kind it is.
    One(u32),
}

/// Plans a print of `pages`.
pub fn plan(doc: &Document, pages: Pages) -> Result<JobPlan, PrintError> {
    match pages {
        Pages::All => plan_job(doc, &printable_pages(doc)),
        Pages::One(id) => plan_job(doc, &[id]),
    }
}

/// Writes the sheets `range` of `plan` (numbers from 0) as a PDF, drawn with `renderer`, which holds the fonts the text is laid out in. A
/// range past the plan's sheets is cut to them. **The sheets are drawn from one value of the document, with one cache** of what they work out for the cells that hold a formula and
/// no saved value ([`with_worked_cache`]): a page that takes twenty sheets works each of its cells out once and spends the document's steps in all, where a frame for
/// each sheet worked a cell that two sheets draw out twice and spent the steps of a frame twenty times. (A one-page export draws one frame, so its frame's steps are its whole
/// budget, and it keeps no cache.)
pub fn pdf(renderer: &mut Renderer, doc: &Document, plan: &JobPlan, range: Range<usize>, options: &PrintOptions) -> Result<Printed, ExportError> {
    let sheets: Vec<Sheet> = plan.sheets().skip(range.start).take(range.end.saturating_sub(range.start)).map(|(_, s)| s.clone()).collect();
    with_worked_cache(renderer, doc, |renderer, shared| diacraft_pdf::export_sheets(renderer, shared, &sheets, options))
}

/// Runs `draw` with the renderer keeping the answers of the cells that hold a formula and no saved value for the document it is given, which is **the document as one value held
/// by an `Arc`** (a [`WorkedCache`] is for the very document it holds, and a frame uses it only for that one): the copy of `doc` is the document's chunks shared by reference, which is
/// cheap, and the renderer's own cache is put back when `draw` ends.
fn with_worked_cache<R>(renderer: &mut Renderer, doc: &Document, draw: impl FnOnce(&mut Renderer, &Document) -> R) -> R {
    let shared = Arc::new(doc.clone());
    let kept = renderer.worked_cache().cloned();
    renderer.set_worked_cache(Some(WorkedCache::new(Arc::clone(&shared))));
    let out = draw(renderer, &shared);
    renderer.set_worked_cache(kept);
    out
}

/// What to tell a person of a print that failed: the words of the failure, without the export's "too large to export:" in front of a print's.
pub fn failure_words(error: &ExportError) -> String {
    match error {
        ExportError::TooLarge(what) => format!("this print is too large: {what}"),
        other => other.to_string(),
    }
}

/// The plan as JSON, for `file.print_preview` and for the reply of `file.print`: for each page its paper, orientation, scale and tiles, and
/// the runs of sheets that take the same paper (what a print to one printer sends as one job).
pub fn describe(plan: &JobPlan) -> Value {
    let pages: Vec<Value> = plan
        .pages
        .iter()
        .map(|p| {
            let sheets: Vec<Value> = p.sheets.iter().map(|s| json!({ "number": s.number, "column": s.col, "row": s.row })).collect();
            json!({
                "page": { "id": p.page, "name": p.name },
                "paper": { "kind": p.paper.kind, "name": p.paper.name, "known": p.paper_known, "width": p.paper.width, "height": p.paper.height },
                "orientation": p.orientation.name(),
                "sheet": { "width": p.sheet_size.0, "height": p.sheet_size.1 },
                "printable": {
                    "left": p.setup.left, "top": p.setup.top, "width": p.printable_size.0, "height": p.printable_size.1,
                },
                "page_size": { "width": p.page_size.0, "height": p.page_size.1 },
                "scale": p.scale,
                "across": p.across,
                "down": p.down,
                "sheets": sheets,
            })
        })
        .collect();
    let jobs: Vec<Value> =
        plan.runs().iter().map(|r| json!({ "first": r.first, "sheets": r.count, "paper": { "width": r.paper.0, "height": r.paper.1 } })).collect();
    json!({ "sheet_count": plan.sheet_count(), "pages": pages, "jobs": jobs })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_sheets_of_a_print_are_drawn_with_a_cache_for_the_document_drawn_and_the_renderers_own_is_put_back() {
        let doc = Document::new_blank();
        let mut renderer = Renderer::new();
        assert!(renderer.worked_cache().is_none());
        let own = WorkedCache::new(Arc::new(doc.clone()));
        renderer.set_worked_cache(Some(Arc::clone(&own)));
        let seen = with_worked_cache(&mut renderer, &doc, |renderer, shared| {
            let cache = renderer.worked_cache().cloned().expect("a cache while the sheets are drawn");
            (cache.is_for(shared), cache.is_for(&doc), Arc::ptr_eq(&cache, &own))
        });
        assert_eq!(seen, (true, false, false), "a cache for the value the sheets are drawn from, and no other");
        assert!(renderer.worked_cache().is_some_and(|c| Arc::ptr_eq(c, &own)), "the renderer's own cache is back");
        let mut bare = Renderer::new();
        with_worked_cache(&mut bare, &doc, |_, _| ());
        assert!(bare.worked_cache().is_none(), "and a renderer that had none has none");
    }
}
