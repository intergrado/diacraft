//! The PDF a print becomes: a page for each sheet of paper, as large as the paper, read back with the shared reader and interpreter.
//!
//! Paper inch `(x, y)` of a page (y up) is at the sheet's inch `(x + 0.25, 10.75 - ...)` on a Letter sheet with the root's quarter-inch margins,
//! which is point `72 *` that, y down from the sheet's top left corner. Every assertion is a value in points.

use diacraft_doc::Document;
use diacraft_pdf::{PrintOptions, export_page, export_sheets};
use diacraft_render::print::{JobPlan, PrintColor, plan_job};
use diacraft_render::{Renderer, SkipKind};
use diacraft_testkit::build::PageBuilder;

use crate::readback::{Seen, read_page};
use crate::support::*;

fn plain_print() -> PrintOptions {
    PrintOptions { compress: false, ..PrintOptions::default() }
}

fn all_sheets(plan: &JobPlan) -> Vec<diacraft_render::print::Sheet> {
    plan.sheets().map(|(_, s)| s.clone()).collect()
}

/// Prints `pages` of `doc` and reads every page of the file back.
fn print(doc: &Document, pages: &[u32]) -> (diacraft_pdf::Printed, Vec<Seen>) {
    let plan = plan_job(doc, pages).expect("plans");
    let printed = export_sheets(&mut Renderer::new(), doc, &all_sheets(&plan), &plain_print()).expect("prints");
    let seen = (0..printed.sheets).map(|n| read_page(&printed.bytes, n)).collect();
    (printed, seen)
}

fn red(id: u32, x: f64, y: f64, w: f64, h: f64) -> diacraft_testkit::build::ShapeBuilder {
    rect(id, x, y, w, h).cell("FillForegnd", "#FF0000").cell("LinePattern", "0")
}

fn with(p: PageBuilder, cells: &[(&str, &str)]) -> PageBuilder {
    cells.iter().fold(p, |p, (name, value)| p.cell(name, value))
}

#[test]
fn a_page_for_each_sheet_as_large_as_the_paper_and_clipped_to_the_margins() {
    // A 17 by 11 inch page on Letter upright (8 by 10.5 printable): 3 across and 2 down, six pages of 612 by 792 points.
    let doc = base().page(1, with(page(17.0, 11.0), &[("PrintPageOrientation", "1")]).shape(red(2, 6.0, 0.0, 4.0, 11.0))).build();
    let (printed, seen) = print(&doc, &[1]);
    assert!(printed.bytes.starts_with(b"%PDF-"));
    assert_eq!((printed.sheets, seen.len()), (6, 6));
    assert_eq!(seen[0].pages, 6, "the file has six pages");
    for s in &seen {
        assert_eq!(s.size, (612.0, 792.0));
        // The printable part: a quarter inch in on every side, as the clip every page pushes.
        let clip = s.clips.first().expect("each page is clipped to its printable part");
        assert!(close(clip.x0, 18.0) && close(clip.y0, 18.0) && close(clip.x1, 594.0) && close(clip.y1, 774.0), "{clip:?}");
        assert_eq!(s.open_clips, 0, "every clip is closed");
    }
    // The bar spans page x 6 to 10, the page's full height. The path is written whole and the clip cuts it to the printable part: on the first
    // column it starts at the sheet's 6.25 inches (450 points) and ends at 10.25 (738, past the clip's 594); on the second, page x 8 is the
    // sheet's 0.25 inches, so the bar runs from -1.75 inches (-126 points) to 2.25 (162); on the third the bar is off the sheet and not written.
    let bar = |s: &Seen| s.with_colour([255, 0, 0]).into_iter().map(|p| p.bbox).collect::<Vec<_>>();
    let first = bar(&seen[0]);
    assert_eq!(first.len(), 1);
    assert!(close(first[0].x0, 450.0) && close(first[0].x1, 738.0), "{first:?}");
    let second = bar(&seen[1]);
    assert_eq!(second.len(), 1);
    assert!(close(second[0].x0, -126.0) && close(second[0].x1, 162.0), "{second:?}");
    assert!(bar(&seen[2]).is_empty(), "nothing of the page's third column is in the bar");
}

#[test]
fn one_sheet_is_the_export_moved_by_the_margin() {
    // The print draws the page as the export does: a rectangle at paper inches [1, 3] x [1, 2] of a 4 by 4 page is at the export's points
    // (72..216, 144..216), and on a Letter sheet inside quarter-inch margins it is 18 points further in each way.
    let doc = base().page(1, page(4.0, 4.0).shape(red(2, 1.0, 1.0, 2.0, 1.0))).build();
    let exported = export_page(&mut Renderer::new(), &doc, 1, &plain()).unwrap();
    let exported = crate::readback::read(&exported.bytes);
    let e = exported.with_colour([255, 0, 0])[0].bbox;
    let (_, seen) = print(&doc, &[1]);
    let p = seen[0].with_colour([255, 0, 0])[0].bbox;
    assert!(close(p.x0, e.x0 + 18.0) && close(p.x1, e.x1 + 18.0) && close(p.y0, e.y0 + 18.0) && close(p.y1, e.y1 + 18.0), "{e:?} against {p:?}");
    // And the printed sheet has no white paper under it: the printer's is the paper.
    assert!(seen[0].with_colour([255, 255, 255]).is_empty(), "no paper is drawn");
}

#[test]
fn sheets_take_the_papers_size_and_a_job_may_mix_papers() {
    // Page 1 on Letter, page 2 on A4 sideways (842 by 595 points, to the nearest).
    let doc = base()
        .page(1, page(4.0, 4.0).shape(red(2, 0.0, 0.0, 1.0, 1.0)))
        .page(2, with(page(4.0, 4.0), &[("PaperKind", "9"), ("PrintPageOrientation", "2")]).shape(red(3, 0.0, 0.0, 1.0, 1.0)))
        .build();
    let (printed, seen) = print(&doc, &[1, 2]);
    assert_eq!(printed.sheets, 2);
    assert_eq!(seen[0].size, (612.0, 792.0));
    assert!((seen[1].size.0 - 842.0).abs() < 1.5 && (seen[1].size.1 - 595.0).abs() < 1.5, "{:?}", seen[1].size);
}

#[test]
fn a_non_printing_shape_is_not_in_the_file() {
    let off = red(2, 0.0, 0.0, 1.0, 1.0).cell("NonPrinting", "1");
    let on = red(3, 2.0, 0.0, 1.0, 1.0);
    let doc = base().page(1, page(4.0, 4.0).shape(off).shape(on)).build();
    let (_, seen) = print(&doc, &[1]);
    assert_eq!(seen[0].with_colour([255, 0, 0]).len(), 1, "only the shape that prints");
    // The export of the same page draws both.
    let exported = crate::readback::read(&export_page(&mut Renderer::new(), &doc, 1, &plain()).unwrap().bytes);
    assert_eq!(exported.with_colour([255, 0, 0]).len(), 2);
}

#[test]
fn printing_at_a_scale_scales_the_line_weights_with_the_page() {
    // A page printed at 0.5 is half the size, its lines too. A 0.1 inch outline on a 2 by 2 inch box is a 3.6 point stroke at
    // scale 0.5 (0.1 * 72 * 0.5) and the box 72 points wide (2 * 72 * 0.5).
    let doc = base()
        .page(
            1,
            with(page(4.0, 4.0), &[("ScaleX", "0.5"), ("ScaleY", "0.5")]).shape(
                rect(2, 1.0, 1.0, 2.0, 2.0).cell("FillPattern", "0").cell("LinePattern", "1").cell("LineColor", "#0000FF").cell("LineWeight", "0.1"),
            ),
        )
        .build();
    let (_, seen) = print(&doc, &[1]);
    let stroke = seen[0].paths.iter().find(|p| p.stroked && p.rgba.is_some_and(|c| c[..3] == [0, 0, 255])).expect("the outline");
    assert!((f64::from(stroke.width) - 3.6).abs() < 0.01, "{}", stroke.width);
    assert!(close(stroke.bbox.width(), 72.0), "{:?}", stroke.bbox);
}

#[test]
fn text_stays_text_in_the_printed_file() {
    let doc = base().page(1, page(4.0, 4.0).shape(red(2, 0.5, 0.5, 3.0, 1.0).cell("FillPattern", "0").text("Hello"))).build();
    let (_, seen) = print(&doc, &[1]);
    assert_eq!(seen[0].text(), "Hello");
    assert!(seen[0].glyphs.iter().all(|g| g.embedded));
}

#[test]
fn what_was_left_out_is_summed_over_the_sheets() {
    // A blurred shadow is drawn hard-edged in a PDF; a page of three sheets with one says so once, with the count of the sheets that drew it.
    let shape = red(2, 7.0, 1.0, 2.0, 2.0)
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.1")
        .cell("ShapeShdwOffsetY", "-0.1")
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", "0.15");
    let doc = base().page(1, with(page(17.0, 4.0), &[("PrintPageOrientation", "1")]).shape(shape)).build();
    let plan = plan_job(&doc, &[1]).unwrap();
    assert_eq!(plan.sheet_count(), 3);
    let printed = export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &plain_print()).unwrap();
    // The shape lies across the first two sheets (x 7 to 9 against a tile edge at 8), and the walk places a shape within a margin of the sheet
    // before it knows nothing of it shows, so a third sheet may have counted it too: one entry, summed.
    let blurs: Vec<_> = printed.skipped.iter().filter(|s| s.kind == SkipKind::ShadowApproximate).collect();
    assert_eq!(blurs.len(), 1, "one entry for the kind: {:?}", printed.skipped);
    assert!((2..=3).contains(&blurs[0].count), "{:?}", blurs[0]);
}

#[test]
fn no_sheets_is_an_error_and_a_missing_page_is_one_too() {
    let doc = base().page(1, page(4.0, 4.0)).build();
    let error = export_sheets(&mut Renderer::new(), &doc, &[], &plain_print()).unwrap_err();
    assert!(error.to_string().contains("no sheets"), "{error}");
    let plan = plan_job(&doc, &[1]).unwrap();
    let mut sheet = all_sheets(&plan).remove(0);
    sheet.page = 99;
    assert!(export_sheets(&mut Renderer::new(), &doc, &[sheet], &plain_print()).is_err());
}

#[test]
fn the_title_is_the_jobs_name() {
    let doc = base().page(1, page(4.0, 4.0)).build();
    let plan = plan_job(&doc, &[1]).unwrap();
    let options = PrintOptions { title: Some("Floor plan".to_owned()), ..plain_print() };
    let printed = export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &options).unwrap();
    assert!(printed.bytes.windows(10).any(|w| w == b"Floor plan"), "the title is in the file's information");
}

/// Another reader, where the machine has one: poppler's `pdfinfo` counts the pages and says the paper each is.
#[test]
fn another_reader_counts_the_sheets_and_names_the_paper() {
    use std::process::Command;
    if Command::new("pdfinfo").arg("-v").output().is_err() {
        eprintln!("SKIPPED: pdfinfo (poppler) is not on this machine");
        return;
    }
    let doc = base()
        .page(1, with(page(17.0, 11.0), &[("PrintPageOrientation", "1")]).shape(red(2, 6.0, 0.0, 4.0, 11.0)))
        .page(2, with(page(4.0, 4.0), &[("PaperKind", "9")]))
        .build();
    let plan = plan_job(&doc, &[1, 2]).unwrap();
    let printed = export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &PrintOptions::default()).unwrap();
    let dir = std::env::temp_dir().join(format!("diacraft-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("print.pdf");
    std::fs::write(&file, &printed.bytes).unwrap();
    let out = Command::new("pdfinfo").args(["-f", "1", "-l", "7"]).arg(&file).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // A scratch folder that cannot be removed is left to the system's own clean-up of its temporary folder.
    let _ = std::fs::remove_dir_all(&dir);
    // Spacing is pdfinfo's own; the words are compared with the runs of spaces folded.
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(text.contains("Pages: 7"), "{text}");
    assert!(text.contains("Page 1 size: 612 x 792 pts (letter)"), "{text}");
    assert!(text.contains("Page 7 size: 595.276 x 841.89 pts (A4)"), "{text}");
}

// ---- memory: a drawing that tiles across many sheets (counted in bytes, never in time) ----------------------------------------------------

/// A 64 by 64 inch page (8 sheets across and 7 down on Letter upright: 56 sheets) holding one polyline of 2,000 points that climbs the page's diagonal,
/// each point a hundredth of an inch off the line so that no two segments are one. `dashed` draws it in a dashed pattern.
fn diagonal(dashed: bool) -> Document {
    let mut rows: Vec<(&str, Vec<(&str, f64)>)> = Vec::new();
    for i in 0..2_000u32 {
        let t = f64::from(i) * 64.0 / 1_999.0;
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        rows.push((kind, vec![("X", t), ("Y", (t + if i % 2 == 0 { 0.0 } else { 0.01 }).min(64.0))]));
    }
    let line = geometry_of(shape_at(2, 0.0, 0.0, 64.0, 64.0), rows)
        .cell("FillPattern", "0")
        .cell("LinePattern", if dashed { "2" } else { "1" })
        .cell("LineColor", "#0000FF")
        .cell("LineWeight", "0.02");
    base().page(1, with(page(64.0, 64.0), &[("PrintPageOrientation", "1")]).shape(line)).build()
}

/// The sheet number a refusal says it stopped at.
fn stopped_at(words: &str) -> usize {
    let after = words.split("at sheet ").nth(1).unwrap_or_else(|| panic!("no sheet is named: {words}"));
    after.split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn a_shape_that_spans_many_tiles_is_cut_to_each_and_written_about_once() {
    let doc = diagonal(false);
    let plan = plan_job(&doc, &[1]).unwrap();
    assert_eq!(plan.sheet_count(), 56);
    // The export of the page writes the polyline once: that is what it costs. A print of 56 sheets that wrote it whole on each would be 56 times.
    let once = export_page(&mut Renderer::new(), &doc, 1, &plain()).unwrap().bytes.len();
    let printed = export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &plain_print()).unwrap();
    assert!(
        printed.bytes.len() < 4 * once,
        "the print is {} bytes against {once} for the page once: the polyline was written whole on the sheets it crosses",
        printed.bytes.len()
    );
    // Only the sheets the diagonal crosses hold any of it: eight tiles on the diagonal and the ones beside it, of 56.
    let with_ink = (0..printed.sheets).filter(|n| read_page(&printed.bytes, *n).paths.iter().any(|p| p.stroked)).count();
    assert!((7..=16).contains(&with_ink), "{with_ink} of 56 sheets hold the line");
}

#[test]
fn what_a_sheet_writes_is_counted_and_a_print_past_its_budget_stops_before_the_sheets_that_follow() {
    // The dashed polyline is not cut (a dash pattern would restart at the cut), so each of the sheets its box touches takes all 2,000 points:
    // about 64 KB of content on each. With 1 MB to spend the print is refused after about 16 sheets, and the words say how far it got.
    let doc = diagonal(true);
    let plan = plan_job(&doc, &[1]).unwrap();
    let options = PrintOptions { max_bytes: 1_000_000, ..plain_print() };
    let error = export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &options).unwrap_err();
    let words = error.to_string();
    assert!(matches!(error, diacraft_pdf::ExportError::TooLarge(_)), "{error:?}");
    assert!(words.contains("of 56") && words.contains("1 megabytes"), "{words}");
    let n = stopped_at(&words);
    assert!(
        (14..=18).contains(&n),
        "refused at sheet {n}: 1,000,000 bytes at 2,000 elements of 32 and a paint of 96 on a sheet is 16 sheets, not all 56"
    );
    // A budget that holds the lot lets it through: the count is the same one, not a guess.
    let ample = PrintOptions { max_bytes: 8_000_000, ..plain_print() };
    assert!(export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &ample).is_ok());
}

#[test]
fn the_same_budget_that_refuses_the_uncut_line_takes_the_cut_one() {
    // The solid line is cut to each sheet: about 64 KB of content in all, a sixteenth of the budget that stopped the dashed one.
    let doc = diagonal(false);
    let plan = plan_job(&doc, &[1]).unwrap();
    let options = PrintOptions { max_bytes: 1_000_000, ..plain_print() };
    assert!(export_sheets(&mut Renderer::new(), &doc, &all_sheets(&plan), &options).is_ok());
}

#[test]
fn a_shape_that_misses_a_sheets_printable_part_is_not_written_on_it() {
    // A red bar on the first column only: the sheets of the other columns hold no red path at all, though their box was walked.
    let doc = base().page(1, with(page(17.0, 4.0), &[("PrintPageOrientation", "1")]).shape(red(2, 1.0, 1.0, 2.0, 2.0))).build();
    let (printed, seen) = print(&doc, &[1]);
    assert_eq!(printed.sheets, 3);
    assert_eq!(seen[0].with_colour([255, 0, 0]).len(), 1);
    assert!(seen[1].with_colour([255, 0, 0]).is_empty() && seen[2].with_colour([255, 0, 0]).is_empty());
}

/// A sheet of colours: a red box with a blue line, a green ellipse-sized box with an orange line, and yellow words.
fn colours() -> Document {
    use diacraft_doc::RowKey;
    let words = rect(4, 0.5, 0.5, 3.0, 0.8)
        .cell("FillForegnd", "#2060F0")
        .cell("LinePattern", "0")
        .text("Yellow")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.3")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#FFFF00");
    base()
        .page(
            1,
            page(4.0, 4.0)
                .shape(rect(2, 0.5, 2.5, 1.2, 1.0).cell("FillForegnd", "#FF0000").cell("LineColor", "#1030C0").cell("LineWeight", "0.05"))
                .shape(rect(3, 2.0, 2.5, 1.5, 1.0).cell("FillForegnd", "#20B040").cell("LineColor", "#E0A000").cell("LineWeight", "0.04"))
                .shape(words),
        )
        .build()
}

fn grey_print() -> PrintOptions {
    PrintOptions { color: PrintColor::Grayscale, ..plain_print() }
}

#[test]
fn a_print_in_black_and_white_writes_every_colour_as_its_grey_and_leaves_nothing_out() {
    let doc = colours();
    let plan = plan_job(&doc, &[1]).expect("plans");
    let mut renderer = Renderer::new();
    let colour = export_sheets(&mut renderer, &doc, &all_sheets(&plan), &plain_print()).expect("prints");
    let colour = read_page(&colour.bytes, 0);
    assert_eq!(colour.with_colour([255, 0, 0]).len(), 1, "the red box in colour");
    assert!(colour.glyphs.iter().all(|g| g.rgba == [255, 255, 0, 255]), "yellow words");
    let printed = export_sheets(&mut renderer, &doc, &all_sheets(&plan), &grey_print()).expect("prints");
    assert_eq!(renderer.print_color(), PrintColor::Color, "the renderer's own setting is put back");
    let grey = read_page(&printed.bytes, 0);
    let painted: Vec<[u8; 4]> = grey.paths.iter().filter_map(|p| p.rgba).collect();
    assert!(!painted.is_empty());
    assert!(painted.iter().all(|c| c[0] == c[1] && c[1] == c[2]), "only greys are painted: {painted:?}");
    assert!(grey.glyphs.iter().all(|g| g.rgba[0] == g.rgba[1] && g.rgba[1] == g.rgba[2]), "the words are grey");
    // Yellow is 0.299 * 255 + 0.587 * 255 = 225.93; the red box 76.245; its line, #1030C0, 4.784 + 28.176 + 21.888 = 54.848.
    assert!(grey.glyphs.iter().all(|g| g.rgba == [226, 226, 226, 255]), "{:?}", grey.glyphs.first().map(|g| g.rgba));
    assert_eq!(grey.with_colour([76, 76, 76]).len(), 1, "the red box's grey");
    assert_eq!(grey.with_colour([55, 55, 55]).len(), 1, "its line's grey");
    // Nothing is left out for it: the same paths and glyphs, in the same places.
    assert_eq!((grey.paths.len(), grey.glyphs.len()), (colour.paths.len(), colour.glyphs.len()));
    for (g, c) in grey.paths.iter().zip(&colour.paths) {
        assert!(close(g.bbox.x0, c.bbox.x0) && close(g.bbox.y1, c.bbox.y1) && g.stroked == c.stroked, "{g:?} against {c:?}");
    }
    assert!(grey.text().contains("Yellow"), "the words are still text: {}", grey.text());
}
