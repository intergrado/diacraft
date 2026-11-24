//! Printing: how a page lands on paper
//! and what a sheet draws.
//! Every plan is checked at the values the cells give; every drawing at pixels.

use diacraft_doc::RowKey;
use diacraft_render::print::{
    MAX_SHEETS_PER_JOB, MAX_SHEETS_PER_PAGE, Orientation, PrintError, PrintSetup, Sheet, plan_job, plan_page, printable_pages,
};
use diacraft_render::{Rendered, Renderer};
use diacraft_testkit::build::PageBuilder;
use kurbo::Point;

use crate::support::*;

/// A page of `w` by `h` inches with these print cells.
fn printed(w: f64, h: f64, cells: &[(&str, &str)]) -> PageBuilder {
    cells.iter().fold(page(w, h), |p, (name, value)| p.cell(name, value))
}

fn plan(p: PageBuilder) -> diacraft_render::print::PagePlan {
    plan_page(&base().page(1, p).build(), 1).expect("the page plans")
}

/// Where the page's point `(x, y)` (inches, y up) lands on `sheet`, in sheet inches from its top left.
fn on_sheet(sheet: &Sheet, x: f64, y: f64) -> (f64, f64) {
    let p = sheet.to_sheet * Point::new(x, y);
    (p.x, p.y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

#[test]
fn a_page_with_no_print_cells_prints_on_one_letter_sheet_inside_quarter_inch_margins() {
    // And the others: no local cell reads 0.25; 1 upright.
    let plan = plan(page(8.0, 10.0));
    assert_eq!((plan.paper.name, plan.paper_known), ("Letter", true));
    assert_eq!(plan.orientation, Orientation::Portrait);
    assert_eq!(plan.sheet_size, (8.5, 11.0));
    assert_eq!(plan.printable_size, (8.0, 10.5));
    assert_eq!((plan.across, plan.down, plan.sheets.len()), (1, 1, 1));
    assert!(close(plan.scale, 1.0));
    let sheet = &plan.sheets[0];
    assert_eq!((sheet.printable.x0, sheet.printable.y0, sheet.printable.x1, sheet.printable.y1), (0.25, 0.25, 8.25, 10.75));
    // The page's top left corner (0, 10) is the printable part's top left corner.
    let (x, y) = on_sheet(sheet, 0.0, 10.0);
    assert!(close(x, 0.25) && close(y, 0.25), "({x}, {y})");
}

#[test]
fn the_setup_is_read_from_the_cells_and_a_hostile_cell_reads_as_the_root_style() {
    let doc = base()
        .page(
            1,
            printed(
                8.0,
                10.0,
                &[
                    ("PaperKind", "9"),
                    ("PrintPageOrientation", "2"),
                    ("PageLeftMargin", "0.5"),
                    ("PageRightMargin", "-3"),
                    ("PageTopMargin", "1e300"),
                    ("CenterX", "1"),
                    ("OnPage", "1"),
                    ("PagesX", "0"),
                    ("PagesY", "99999"),
                    ("ScaleX", "-2"),
                    ("PrintGrid", "1"),
                ],
            ),
        )
        .build();
    let s = PrintSetup::of(&doc, 1).unwrap();
    assert_eq!((s.paper_kind, s.orientation_code), (9, 2));
    assert_eq!((s.left, s.right, s.top, s.bottom), (0.5, 0.0, 1_000.0, 0.25), "a negative margin reads 0 and a huge one is bounded");
    assert!(s.centre_x && !s.centre_y && s.fit && s.print_grid);
    assert_eq!((s.pages_x, s.pages_y), (1, 1_024), "a count of 0 reads 1 and a huge one is bounded");
    assert_eq!(s.scale_x, 1.0, "a scale that is not above 0 reads 1");
    assert_eq!(s.orientation((8.0, 10.0)), Orientation::Landscape);
    assert!(PrintSetup::of(&doc, 7).is_none(), "no such page");
}

#[test]
fn paper_kind_names_the_sheet_and_orientation_turns_it() {
    // Keeps the code; the printing milestone gives it a size. 9 is A4, 5 Legal, 3 Tabloid.
    let a4 = plan(printed(8.0, 10.0, &[("PaperKind", "9")]));
    assert_eq!(a4.paper.name, "A4");
    assert!((a4.sheet_size.0 - 210.0 / 25.4).abs() < 1e-9 && (a4.sheet_size.1 - 297.0 / 25.4).abs() < 1e-9);
    let legal_sideways = plan(printed(8.0, 10.0, &[("PaperKind", "5"), ("PrintPageOrientation", "2")]));
    assert_eq!(legal_sideways.orientation, Orientation::Landscape);
    assert_eq!(legal_sideways.sheet_size, (14.0, 8.5), "the sheet is turned");
    assert_eq!(legal_sideways.printable_size, (13.5, 8.0));
    let tabloid = plan(printed(8.0, 10.0, &[("PaperKind", "3"), ("PrintPageOrientation", "1")]));
    assert_eq!(tabloid.sheet_size, (11.0, 17.0));
    // 0 is "as the printer is set", and any other number no list has: the sheet is held the way the page is, here upright for a tall page.
    for code in ["0", "7"] {
        assert_eq!(plan(printed(8.0, 10.0, &[("PrintPageOrientation", code)])).orientation, Orientation::Portrait, "{code}");
        assert_eq!(plan(printed(10.0, 8.0, &[("PrintPageOrientation", code)])).orientation, Orientation::Landscape, "{code}");
    }
    // A page that holds no such cell (a new drawing's) is held the way its size is: a landscape page prints on one landscape sheet.
    let new_page = &[("PageLeftMargin", "0"), ("PageRightMargin", "0"), ("PageTopMargin", "0"), ("PageBottomMargin", "0")];
    let wide = plan(printed(11.0, 8.5, new_page));
    assert_eq!((wide.orientation, wide.sheets.len()), (Orientation::Landscape, 1));
    assert_eq!(wide.sheet_size, (11.0, 8.5));
    assert_eq!(plan(page(8.0, 10.0)).orientation, Orientation::Portrait);
    assert_eq!(plan(page(8.0, 8.0)).orientation, Orientation::Portrait, "a square is upright");
}

#[test]
fn a_code_with_no_known_size_prints_on_letter_and_says_so() {
    // 0, 256 and 32767 are in files and in no list we have.
    for code in ["0", "256", "32767", "999"] {
        let plan = plan(printed(8.0, 10.0, &[("PaperKind", code)]));
        assert_eq!((plan.paper.name, plan.paper_known), ("Letter", false), "{code}");
    }
}

#[test]
fn a_landscape_page_on_upright_paper_takes_two_sheets_across_and_is_never_turned() {
    // 264 files hold a page wider than tall and read the root's upright (1). A page of 11 by 8.5 on Letter portrait (8 by
    // 10.5 printable): 11 / 8 is two sheets across, and 8.5 / 10.5 fits one down.
    let plan = plan(printed(11.0, 8.5, &[("PrintPageOrientation", "1")]));
    assert_eq!((plan.across, plan.down), (2, 1));
    assert_eq!(plan.sheets.len(), 2);
    let (first, second) = (&plan.sheets[0], &plan.sheets[1]);
    // The page's top left corner is at the first sheet's printable top left; the second sheet starts 8 inches further along the page.
    let (x, y) = on_sheet(first, 0.0, 8.5);
    assert!(close(x, 0.25) && close(y, 0.25));
    let (x, y) = on_sheet(second, 8.0, 8.5);
    assert!(close(x, 0.25) && close(y, 0.25), "({x}, {y}): the second sheet starts where the first one's printable part ends");
    // Both sheets show the same strip down the page.
    assert!(close(second.to_sheet.as_coeffs()[5], first.to_sheet.as_coeffs()[5]));
}

#[test]
fn tiles_run_left_to_right_and_then_down() {
    // A 17 by 22 inch page on Letter portrait: 17 / 8 is 3 across (2.125), 22 / 10.5 is 3 down (2.095).
    let plan = plan(page(17.0, 22.0));
    assert_eq!((plan.across, plan.down, plan.sheets.len()), (3, 3, 9));
    let order: Vec<(u32, u32)> = plan.sheets.iter().map(|s| (s.col, s.row)).collect();
    assert_eq!(order, [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1), (0, 2), (1, 2), (2, 2)]);
    for (i, s) in plan.sheets.iter().enumerate() {
        assert_eq!((s.index, s.number), (i, i));
    }
    // Row 1 starts a printable height (10.5) lower down the page: the page point 22 - 10.5 is at the top of its sheet.
    let (x, y) = on_sheet(&plan.sheets[3], 0.0, 22.0 - 10.5);
    assert!(close(x, 0.25) && close(y, 0.25), "({x}, {y})");
}

#[test]
fn a_page_that_just_fits_takes_one_sheet_and_one_that_just_does_not_takes_two() {
    assert_eq!(plan(page(8.0, 10.5)).sheets.len(), 1, "exactly the printable part");
    assert_eq!(plan(page(8.0 + 1.0e-9, 10.5)).sheets.len(), 1, "float noise");
    assert_eq!(plan(page(8.01, 10.5)).sheets.len(), 2);
}

#[test]
fn scale_x_prints_the_page_at_that_share_and_the_page_keeps_its_size() {
    // 0.7 and OnPage 0 prints a 10 inch page at 7; a 10 inch height at 7; it is apart from the drawing's scale.
    let plan = plan(printed(10.0, 10.0, &[("ScaleX", "0.7"), ("ScaleY", "0.7"), ("PagesX", "3")]));
    assert!(close(plan.scale, 0.7), "{}", plan.scale);
    assert_eq!(plan.sheets.len(), 1, "PagesX is unused while OnPage is off");
    assert_eq!(plan.page_size, (10.0, 10.0), "the page's own size does not change");
    let (x, y) = on_sheet(&plan.sheets[0], 10.0, 0.0);
    assert!(close(x, 0.25 + 7.0) && close(y, 0.25 + 7.0), "the page's lower right corner lands 7 inches from the corner: ({x}, {y})");
    // ScaleX 2 doubles a 6 inch page across 12 inches: two sheets.
    assert_eq!(plan_scaled(6.0, 2.0).sheets.len(), 2);
}

fn plan_scaled(w: f64, scale: f64) -> diacraft_render::print::PagePlan {
    plan(printed(w, 2.0, &[("ScaleX", &scale.to_string())]))
}

#[test]
fn fit_to_sheets_ignores_the_scale_and_uses_only_the_sheets_the_proportions_need() {
    // OnPage 1, PagesX 2 and ScaleX 0.5: two sheets across, with the 0.5 unused. A 10.5 by 1 page on Letter sideways
    // (10.5 by 8 printable) fills two sheets across at scale 2.
    let two = plan(printed(10.5, 1.0, &[("PrintPageOrientation", "2"), ("OnPage", "1"), ("PagesX", "2"), ("PagesY", "1"), ("ScaleX", "0.5")]));
    assert!(close(two.scale, 2.0), "{}", two.scale);
    assert_eq!((two.across, two.down), (2, 1));
    // PagesX 3 across for a 16 by 2 page: scale 31.5 / 16, the proportions need 3 across and 1 down although 3 by 3 were asked.
    let wide = plan(printed(16.0, 2.0, &[("PrintPageOrientation", "2"), ("OnPage", "1"), ("PagesX", "3"), ("PagesY", "3")]));
    assert!(close(wide.scale, 3.0 * 10.5 / 16.0), "{}", wide.scale);
    assert_eq!((wide.across, wide.down), (3, 1), "only the sheets the proportions need");
    // OnPage 0 with the same PagesX: half size on one sheet.
    let half = plan(printed(8.0, 2.0, &[("PrintPageOrientation", "2"), ("ScaleX", "0.5"), ("PagesX", "3")]));
    assert_eq!((half.across, half.down), (1, 1));
    assert!(close(half.scale, 0.5));
}

#[test]
fn centring_puts_the_scaled_page_in_the_middle_of_the_printable_part() {
    // A sheet 11 wide and a page 8 wide: the left edge at 1.5. A sheet 8.5 tall and a page 6.5 tall: the top edge
    // 1 below the sheet's top. Letter sideways, quarter inch margins on every side.
    let both = plan(printed(8.0, 6.5, &[("PrintPageOrientation", "2"), ("CenterX", "1"), ("CenterY", "1")]));
    let (x, y) = on_sheet(&both.sheets[0], 0.0, 6.5);
    assert!(close(x, 1.5) && close(y, 1.0), "({x}, {y})");
    // Off, the page is at the printable part's top left: 0.25 each way.
    let none = plan(printed(8.0, 6.5, &[("PrintPageOrientation", "2")]));
    let (x, y) = on_sheet(&none.sheets[0], 0.0, 6.5);
    assert!(close(x, 0.25) && close(y, 0.25), "({x}, {y})");
    // Only across.
    let across = plan(printed(8.0, 6.5, &[("PrintPageOrientation", "2"), ("CenterX", "1")]));
    let (x, y) = on_sheet(&across.sheets[0], 0.0, 6.5);
    assert!(close(x, 1.5) && close(y, 0.25));
}

#[test]
fn a_tiled_page_is_centred_across_the_sheets_it_covers() {
    // 11 by 8.5 on Letter upright is two sheets across (16 inches of printable width, 5 more than the page): centred, 2.5 each side.
    let plan = plan(printed(11.0, 8.5, &[("CenterX", "1"), ("PrintPageOrientation", "1")]));
    assert_eq!(plan.across, 2);
    let (x, _) = on_sheet(&plan.sheets[0], 0.0, 8.5);
    assert!(close(x, 0.25 + 2.5), "{x}");
    let (x, _) = on_sheet(&plan.sheets[1], 11.0, 8.5);
    assert!(close(x, 0.25 + 11.0 - 8.0 + 2.5), "the page's right edge on the second sheet: {x}");
}

#[test]
fn margins_are_inches_on_the_sheet_whatever_the_drawing_scale() {
    // V 0.25 and s 1/48 is 0.25 inch on the sheet, not 0.0052. A drawing at 1 inch to 4 feet: 408 by 528 units is 8.5 by 11
    // inches, which does not fit inside the quarter inch margins: 2 sheets across and 2 down.
    let p = page(408.0, 528.0).cell("DrawingScale", "48").cell("PageLeftMargin", "0.5");
    let plan = plan(p);
    assert_eq!(plan.page_size, (8.5, 11.0));
    assert_eq!((plan.sheets[0].printable.x0, plan.sheets[0].printable.x1), (0.5, 8.25));
    assert_eq!(plan.sheets.len(), 4);
}

#[test]
fn margins_that_leave_no_room_refuse_the_print_in_words() {
    let doc = base().page(1, printed(8.0, 10.0, &[("PageLeftMargin", "5"), ("PageRightMargin", "5")])).build();
    let error = plan_page(&doc, 1).unwrap_err();
    assert!(matches!(error, PrintError::NoPrintableArea { .. }), "{error:?}");
    let words = error.to_string();
    assert!(words.contains("margins") && !words.contains("PageLeftMargin"), "{words}");
}

#[test]
fn a_page_past_the_most_sheets_is_refused_and_a_hostile_count_cannot_make_it_take_more() {
    let doc = base().page(1, printed(8.0 * (MAX_SHEETS_PER_PAGE as f64 + 1.0), 1.0, &[("PrintPageOrientation", "1")])).build();
    let PrintError::TooManySheets { needed, .. } = plan_page(&doc, 1).unwrap_err() else { panic!("not refused as too many") };
    assert_eq!(needed, MAX_SHEETS_PER_PAGE + 1);
    // A tiny page fitted to 1,024 by 1,024 sheets would take about 800,000; and a page of 1e300 inches an unthinkable number.
    let tiny = base().page(1, printed(1.0, 1.0, &[("OnPage", "1"), ("PagesX", "1024"), ("PagesY", "1024")])).build();
    assert!(matches!(plan_page(&tiny, 1), Err(PrintError::TooManySheets { .. })));
    let huge = base().page(1, page(1.0e300, 1.0e300)).build();
    assert!(matches!(plan_page(&huge, 1), Err(PrintError::TooManySheets { .. })), "{:?}", plan_page(&huge, 1));
    let words = plan_page(&huge, 1).unwrap_err().to_string();
    assert!(words.contains("sheets of paper"), "{words}");
}

#[test]
fn a_job_numbers_its_sheets_through_the_pages_and_is_bounded() {
    let doc = base()
        .page(1, page(8.0, 10.0))
        .page(2, page(17.0, 10.0).cell("PrintPageOrientation", "1"))
        .page(3, page(7.0, 10.0).cell("PaperKind", "9"))
        .build();
    let job = plan_job(&doc, &[1, 2, 3]).unwrap();
    assert_eq!((job.pages.len(), job.sheet_count()), (3, 1 + 3 + 1));
    let numbers: Vec<usize> = job.sheets().map(|(_, s)| s.number).collect();
    assert_eq!(numbers, [0, 1, 2, 3, 4]);
    let (page_plan, sheet) = job.sheet(2).unwrap();
    assert_eq!((page_plan.page, sheet.page, sheet.index), (2, 2, 1), "the third sheet is the page 2's second");
    assert!(job.sheet(5).is_none());
    // Letter, Letter, Letter, Letter, A4: two runs of paper.
    let runs = job.runs();
    assert_eq!(runs.iter().map(|r| (r.first, r.count)).collect::<Vec<_>>(), [(0, 4), (4, 1)]);
    assert!(plan_job(&doc, &[]).is_err(), "nothing to print");
    assert_eq!(plan_job(&doc, &[1, 9]).unwrap_err(), PrintError::NoSuchPage(9));
    // A print of more sheets than a job takes is refused: five pages of 1,024 sheets each.
    let long = || printed(8.0 * 1_024.0, 1.0, &[("PrintPageOrientation", "1")]);
    let big = base().page(1, long()).page(2, long()).page(3, long()).page(4, long()).page(5, long()).build();
    const { assert!(MAX_SHEETS_PER_JOB < 5 * 1_024) };
    assert!(matches!(plan_job(&big, &[1, 2, 3, 4, 5]), Err(PrintError::TooLarge(_))));
}

#[test]
fn a_turned_sheet_is_the_same_paper_in_a_run() {
    let doc = base().page(1, page(8.0, 10.0)).page(2, printed(8.0, 10.0, &[("PrintPageOrientation", "2")])).build();
    let job = plan_job(&doc, &[1, 2]).unwrap();
    assert_eq!(job.runs().len(), 1, "Letter upright and Letter sideways are one paper");
    assert_eq!(job.runs()[0].paper, (8.5, 11.0));
}

#[test]
fn all_pages_are_the_foreground_pages_in_file_order_without_the_hidden_ones() {
    let doc = base()
        .page(10, page(8.0, 10.0))
        .page(11, page(8.0, 10.0).background())
        .page(12, page(8.0, 10.0).cell("UIVisibility", "1"))
        .page(13, page(8.0, 10.0).back_page(11))
        .build();
    assert_eq!(printable_pages(&doc), [10, 13]);
}

// ---- drawing a sheet --------------------------------------------------------------------------------------------------------------

const SHEET_PPI: f64 = 40.0;

/// The straight RGBA at sheet inches `(x, y)` from the sheet's top left.
fn pixel(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (px, py) = ((x * SHEET_PPI).floor() as usize, (y * SHEET_PPI).floor() as usize);
    let i = (py * r.width as usize + px) * 4;
    [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]]
}

fn is_red(r: &Rendered, x: f64, y: f64) -> bool {
    let p = pixel(r, x, y);
    p[3] > 250 && p[0] > 240 && p[1] < 30 && p[2] < 30
}

fn is_clear(r: &Rendered, x: f64, y: f64) -> bool {
    pixel(r, x, y)[3] == 0
}

fn sheet_of(doc: &diacraft_doc::Document, n: usize) -> Sheet {
    let plan = plan_job(doc, &[1]).unwrap();
    plan.sheet(n).map(|(_, s)| s.clone()).unwrap_or_else(|| panic!("no sheet {n}"))
}

#[test]
fn a_sheet_is_the_papers_size_and_ink_stops_at_the_margins() {
    // A page of 8.5 by 11 inches covered by a red rectangle: the sheet is 8.5 by 11 and the margins (0.25) stay clear.
    let doc = base().page(1, page(8.5, 11.0).shape(rect(2, 0.0, 0.0, 8.5, 11.0).cell("FillForegnd", "#FF0000"))).build();
    let sheet = sheet_of(&doc, 0);
    let r = Renderer::new().render_sheet(&doc, &sheet, SHEET_PPI);
    assert_eq!((r.width, r.height), (340, 440), "8.5 by 11 inches at 40 to the inch");
    // The page is 0.25 + 8.5 + ... wider than the printable part (8 by 10.5): the sheet shows the part that fits, edge to edge of it.
    assert!(is_red(&r, 4.0, 5.0), "the middle");
    assert!(is_clear(&r, 0.1, 5.0), "the left margin");
    assert!(is_clear(&r, 8.4, 5.0), "the right margin");
    assert!(is_clear(&r, 4.0, 0.1), "the top margin");
    assert!(is_clear(&r, 4.0, 10.9), "the bottom margin");
    assert!(is_red(&r, 0.3, 0.3) && is_red(&r, 8.2, 10.7), "up to the printable part's corners");
}

#[test]
fn each_tile_shows_its_part_of_the_page_and_no_ink_is_drawn_twice() {
    // 17 by 11 on Letter upright: 3 across, 2 down. A red bar 4 inches wide at x 6 to 10, the page's full height, is cut at the tile edge at
    // page x 8 (printable width 8): the first column holds 6 to 8, the second 8 to 10.
    let doc = base()
        .page(1, printed(17.0, 11.0, &[("PrintPageOrientation", "1")]).shape(rect(2, 6.0, 0.0, 4.0, 11.0).cell("FillForegnd", "#FF0000")))
        .build();
    let (first, second, third) = (sheet_of(&doc, 0), sheet_of(&doc, 1), sheet_of(&doc, 2));
    let mut renderer = Renderer::new();
    let a = renderer.render_sheet(&doc, &first, SHEET_PPI);
    let b = renderer.render_sheet(&doc, &second, SHEET_PPI);
    let c = renderer.render_sheet(&doc, &third, SHEET_PPI);
    // Sheet 0 shows page x 0 to 8 at sheet x 0.25 to 8.25: the bar from page x 6 is at sheet x 6.25 to 8.25.
    assert!(is_clear(&a, 5.9, 3.0) && is_red(&a, 6.4, 3.0) && is_red(&a, 8.1, 3.0), "the first column holds the start of the bar");
    // Sheet 1 shows page x 8 to 16 at sheet x 0.25 to 8.25: the bar to page x 10 is at sheet x 0.25 to 2.25.
    assert!(is_red(&b, 0.4, 3.0) && is_red(&b, 2.1, 3.0) && is_clear(&b, 2.5, 3.0), "the second column holds the rest of it");
    // Sheet 2 shows page x 16 and 17 only.
    assert!(is_clear(&c, 0.4, 3.0) && is_clear(&c, 0.9, 3.0));
    // The page is 11 high and a sheet takes 10.5: the second row (sheets 3 to 5) holds the last half inch, which the bar covers too.
    let below = renderer.render_sheet(&doc, &sheet_of(&doc, 4), SHEET_PPI);
    assert!(is_red(&below, 0.4, 0.3) && is_red(&below, 2.1, 0.5), "the bar continues on the next row");
    assert!(is_clear(&below, 0.4, 1.5), "and stops where the page does (11 inches is 0.5 past the first row)");
}

#[test]
fn printing_scale_enlarges_what_is_drawn() {
    // A 2 inch red square on a page printed at 2 times is 4 inches on the sheet, from the printable corner.
    let doc = base()
        .page(1, printed(4.0, 4.0, &[("ScaleX", "2"), ("ScaleY", "2")]).shape(rect(2, 0.0, 2.0, 2.0, 2.0).cell("FillForegnd", "#FF0000")))
        .build();
    let r = Renderer::new().render_sheet(&doc, &sheet_of(&doc, 0), SHEET_PPI);
    assert!(is_red(&r, 0.3, 0.3) && is_red(&r, 4.2, 4.2), "the square, doubled, from the corner of the printable part");
    assert!(is_clear(&r, 4.5, 4.5) && is_clear(&r, 0.3, 5.0));
}

#[test]
fn a_non_printing_shape_and_a_group_of_them_are_left_off_the_sheet_and_drawn_on_the_screen() {
    // Of the layers file: NonPrinting 1 shows on screen and is not on paper; NonPrinting 0 prints.
    let off = rect(2, 0.0, 0.0, 1.0, 1.0).cell("FillForegnd", "#FF0000").cell("NonPrinting", "1");
    let on = rect(3, 2.0, 0.0, 1.0, 1.0).cell("FillForegnd", "#FF0000").cell("NonPrinting", "0");
    let group = rect(4, 4.0, 0.0, 2.0, 1.0)
        .cell("FillForegnd", "#FF0000")
        .cell("NonPrinting", "1")
        .group()
        .child(rect(5, 0.0, 0.0, 1.0, 1.0).cell("FillForegnd", "#FF0000"))
        .child(rect(6, 1.0, 0.0, 1.0, 1.0).cell("FillForegnd", "#FF0000").cell("NonPrinting", "0"));
    let doc = base().page(1, page(8.0, 4.0).shape(off).shape(on).shape(group)).build();
    let mut renderer = Renderer::new();
    let r = renderer.render_sheet(&doc, &sheet_of(&doc, 0), SHEET_PPI);
    // The page's lower edge is at sheet y 4.25 (page 4 high at the top margin): a shape on the page's row 0 to 1 is at sheet y 3.25 to 4.25.
    assert!(is_clear(&r, 0.25 + 0.5, 3.75), "the non-printing shape");
    assert!(is_red(&r, 0.25 + 2.5, 3.75), "the printing shape");
    assert!(is_clear(&r, 0.25 + 4.5, 3.75) && is_clear(&r, 0.25 + 5.5, 3.75), "a group with NonPrinting 1, and its members, even one that says 0");
    // The same walk on the screen draws all of them, and the flag was put back.
    let screen = draw_with(&mut renderer, &doc);
    assert!(screen.is_red(0.5, 0.5) && screen.is_red(2.5, 0.5) && screen.is_red(4.5, 0.5) && screen.is_red(5.5, 0.5), "on screen all are drawn");
}

fn layer(p: PageBuilder, ix: u32, visible: &str, print: &str) -> PageBuilder {
    let key = RowKey::Ix(ix);
    p.row_cell("Layer", None, key.clone(), "Visible", visible)
        .row_cell("Layer", None, key.clone(), "Print", print)
        .row_cell("Layer", None, key.clone(), "Name", &format!("Layer{ix}"))
        .row_cell("Layer", None, key.clone(), "NameUniv", &format!("Layer{ix}"))
        .row_cell("Layer", None, key, "Status", "0")
}

fn on(layers: &str, x: f64) -> diacraft_testkit::build::ShapeBuilder {
    rect(10 + x as u32, x, 0.0, 0.8, 0.8).cell("FillForegnd", "#FF0000").cell("LayerMember", layers)
}

#[test]
fn paper_goes_by_a_layers_print_and_the_screen_by_its_visible() {
    // Print 0 and Visible 1 is on screen and off paper. Print 1 and Visible 0 is off screen and on paper.
    let p = layer(layer(layer(page(8.0, 4.0), 0, "1", "0"), 1, "0", "1"), 2, "1", "1");
    let p = p.shape(on("0", 0.0)).shape(on("1", 2.0)).shape(on("2", 4.0)).shape(on("0;1", 6.0)).shape(on("", 7.0));
    let doc = base().page(1, p).build();
    let mut renderer = Renderer::new();
    let r = renderer.render_sheet(&doc, &sheet_of(&doc, 0), SHEET_PPI);
    let at = |x: f64| (0.25 + x + 0.4, 3.25 + 0.4);
    let (x, y) = at(0.0);
    assert!(is_clear(&r, x, y), "Print 0: not on paper");
    let (x, y) = at(2.0);
    assert!(is_red(&r, x, y), "Print 1 and hidden: on paper");
    let (x, y) = at(4.0);
    assert!(is_red(&r, x, y), "both: on paper");
    let (x, y) = at(6.0);
    assert!(is_red(&r, x, y), "one printing layer among those it is on is enough");
    let (x, y) = at(7.0);
    assert!(is_red(&r, x, y), "on no layer: printed");
    let screen = draw_with(&mut renderer, &doc);
    assert!(screen.is_red(0.4, 0.4) && screen.is_white(2.4, 0.4), "on the screen the visible layer shows and the hidden one does not");
}

#[test]
fn a_page_that_does_not_exist_draws_a_blank_sheet_and_says_so() {
    let doc = base().page(1, page(8.0, 4.0)).build();
    let mut sheet = sheet_of(&doc, 0);
    sheet.page = 99;
    let r = Renderer::new().render_sheet(&doc, &sheet, SHEET_PPI);
    assert!(r.rgba_premul.iter().all(|b| *b == 0));
    assert!(r.skipped.iter().any(|s| s.kind == diacraft_render::SkipKind::NoSuchPage));
    // A resolution that is not a number, or a sheet too large for a picture, is refused and not drawn.
    let sheet = sheet_of(&doc, 0);
    assert_eq!(Renderer::new().render_sheet(&doc, &sheet, f64::NAN).width, 0);
    assert_eq!(Renderer::new().render_sheet(&doc, &sheet, 1.0e9).width, 0);
}
