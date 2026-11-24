//! Printing: `file.print_preview` says what a print of the drawing would put on paper, and `file.print` writes it as a PDF.
//!
//! The engine has no printer to send a job to: a window does that, with the PDF that `file.print` writes (`crate::print::pdf`). A program that
//! runs `file.print` gets the file the printer would be given, a page for each sheet of paper at the paper's size, with the page's part for that
//! sheet drawn on it. Nothing in the drawing changes.

use std::path::Path;

use diacraft_pdf::PrintOptions;
use diacraft_render::Renderer;
use serde_json::{Value, json};

use super::file::is_the_open_drawing;
use super::params::optional_whole;
use super::{CommandSpec, has_document, optional_text, required_text};
use crate::error::{bad, failed, shown};
use crate::file::{save_target, write_target};
use crate::print::{Pages, PrintColor, TextMode, describe, failure_words, pdf, plan};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "file.print_preview",
            "Print Preview",
            ["File"],
            None,
            "page: the ID of one page to look at (every page that prints, when left out)",
            has_document,
            preview
        ),
        cmd!(
            "file.print",
            "Print",
            ["File"],
            Some("Cmd+P"),
            "path: where to write what would be printed, a PDF with a page for each sheet of paper (.pdf, in any case); \
             page: the ID of one page to print (every page that prints, when left out); \
             text: `text` (the default: letters are text where the font allows it) or `outlines` (every letter as paths); \
             color: `color` (the default: the drawing's colours) or `grayscale` (black and white: every colour as its grey, \
             0.299 red + 0.587 green + 0.114 blue)",
            has_document,
            print
        ),
    ]
}

/// The pages the params choose.
fn pages_of(p: &Value, cmd: &str) -> Result<Pages> {
    Ok(match optional_whole(p, "page", cmd)? {
        Some(id) => Pages::One(id),
        None => Pages::All,
    })
}

/// What a print of the drawing would put on paper → `{sheet_count, pages: [{page: {id, name}, paper: {kind, name, known, width, height},
/// orientation, sheet, printable, page_size, scale, across, down, sheets: [{number, column, row}]}], jobs: [{first, sheets, paper}]}`. Sizes are
/// inches. A print the drawing cannot have (margins that leave no room, a page that takes more sheets than a print may) is an error in words.
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.print_preview";
    let which = pages_of(p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let planned = plan(doc, which).map_err(|e| failed(ID, e.to_string()))?;
    Ok(describe(&planned))
}

/// Writes what Print would send to a printer as a PDF at `path`, a page for each sheet of paper. **It changes nothing in the drawing** (it is not an
/// undo step, does not mark the drawing saved and gives it no file), and its path has the rules of an export's: a link is refused unless it leads to
/// the open drawing, which is itself refused, the extension is judged on the file that would be written, and the file is made whole in memory, then
/// put in place so that a failure leaves a file that was there as it was. `color: grayscale` writes every colour as its grey (the Print view's
/// Black and white). → `file.print_preview`'s answer, with `bytes` and `skipped` (what was left out or drawn by a stand-in,
/// `{kind, count, first}`, as `file.export` gives it) added.
fn print(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.print";
    if s.in_group() {
        return Err(failed(ID, format!("`{ID}` cannot run inside a batch or other undo group")));
    }
    let path = Path::new(required_text(p, "path", ID)?);
    let which = pages_of(p, ID)?;
    let text = match optional_text(p, "text", ID)? {
        None | Some("text") => TextMode::AsText,
        Some("outlines") => TextMode::AsOutlines,
        Some(_) => return Err(bad(ID, "`text` must be `text` or `outlines`")),
    };
    let color = match optional_text(p, "color", ID)? {
        None | Some("color") => PrintColor::Color,
        Some("grayscale") => PrintColor::Grayscale,
        Some(_) => return Err(bad(ID, "`color` must be `color` or `grayscale`")),
    };
    let target = save_target(path, s.path()).map_err(|e| failed(ID, format!("the print could not be saved: {e}")))?;
    if target.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("pdf")) {
        return Err(bad(ID, "the file name must end in .pdf"));
    }
    if is_the_open_drawing(&target, s.path()) {
        return Err(failed(ID, "the print could not be saved: that is the drawing's own file, which a print never writes"));
    }
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let planned = plan(doc, which).map_err(|e| failed(ID, e.to_string()))?;
    // The machine's faces first, as the window lays text out, so that a print is set in the faces the person sees.
    let mut renderer = Renderer::with_shared_fonts(crate::measure::renderer_fonts());
    let title = (!doc.title.trim().is_empty()).then(|| doc.title.to_string());
    let options = PrintOptions { text, title, color, ..PrintOptions::default() };
    let printed = pdf(&mut renderer, doc, &planned, 0..planned.sheet_count(), &options).map_err(|e| failed(ID, failure_words(&e)))?;
    write_target(&target, &printed.bytes).map_err(|e| failed(ID, format!("the print could not be saved: {e}")))?;
    let skipped: Vec<Value> =
        printed.skipped.iter().map(|s| json!({ "kind": format!("{:?}", s.kind), "count": s.count, "first": shown(&s.first) })).collect();
    let mut answer = describe(&planned);
    if let Some(map) = answer.as_object_mut() {
        map.insert("bytes".to_owned(), json!(printed.bytes.len()));
        map.insert("skipped".to_owned(), Value::Array(skipped));
    }
    Ok(answer)
}
