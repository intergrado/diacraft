//! Printing: where a page lands on paper, sheet by sheet, and the drawing of one sheet.
//!
//! A page is printed on **sheets** of paper. The paper is the page's own setting, apart from the page's size:
//! `PaperKind` names it, `PrintPageOrientation` turns it, and the four margins are blank strips on the sheet. A page larger than the
//! printable part of a sheet is cut into **tiles**, one to a sheet, left to right and then down; a smaller one takes a single sheet.
//! [`plan_page`] works all of that out from the page's `Print Properties` cells and nothing else, and [`plan_job`] does it for the pages of
//! a print. [`Renderer::render_sheet`] and [`draw_sheet`] then draw one sheet: the preview takes the pixels and a PDF writer takes the
//! vectors, from the same plan and through the same walk, so a preview shows what the printer is given.
//!
//! **The rules** (the cells are; each choice that no specification settles is named **ours**):
//!
//! * **Paper.** `PaperKind` is a Windows paper code; [`PAPERS`] has the sizes of the codes we know. A code that is not in
//!   it (0, 256 and 32767 are in files) prints on US Letter (**ours**, as a new drawing's page is Letter).
//! * **Orientation.** `PrintPageOrientation` 2 turns the sheet sideways and 1 holds it upright. 0 ("as the printer is set", and no printer is
//!   asked here), a missing cell and any other number hold the sheet the way the page's own size is, landscape when it is wider than tall
//!   (**ours**, and Auto Size's: `draw::page_setup`, so that the sheets a page grows by are the sheets it prints on). A new
//!   drawing, whose page holds no such cell, therefore prints on one landscape sheet. The page is never turned to fit the sheet once the cell
//!   says 1: a landscape page on upright paper takes two sheets across.
//! * **Margins** (`PageLeftMargin` and the three others, 0.25 inch in the root style) are inches on the sheet as it comes out of the printer, at
//!   whatever scale the page prints. Ink lands only inside them: what lies in a margin is cut off.
//! * **Scale.** `OnPage` off: the page prints at `ScaleX` (**ours**: `ScaleY` is not read, as Learn says the two are always equal and no file
//!   differs). `OnPage` on: `PagesX` by `PagesY` sheets are filled, the page enlarged or reduced to the largest size that fits them,
//!   and only the sheets its proportions need are used. A count of 0 is read as 1 (**ours**). A scale that is not above
//!   0 reads as 1.
//! * **Centring.** `CenterX` and `CenterY` centre the scaled page on the printable part of the sheets it covers, across and down (with the root's equal margins, the sheet's middle is the printable part's).
//!   Off, the page starts at the top left corner of the
//!   printable part.
//! * **Tile order** (**ours**: no specification gives it): left to right, then down, like the lines of a page of text.
//! * **Bounds.** A page takes at most [`MAX_SHEETS_PER_PAGE`] sheets and a print at most [`MAX_SHEETS_PER_JOB`] and [`MAX_PAGES_PER_JOB`]
//!   pages; past them the print is refused in words, not cut short, since a print with sheets missing is worse than none.
//!
//! **What a printed sheet leaves out or adds** is the page as the PDF export draws it (no grid, ruler or guide), with two
//! differences: the paper is the printer's, so none is drawn, and a **non-printing shape** (`NonPrinting` 1) and a shape whose listed layers
//! all have `Print` 0 are left off. On paper a
//! layer's `Visible` does not count: a hidden layer with `Print` 1 prints. A group with `NonPrinting` 1 is left off whole, with
//! its members (**ours**: the cell is the group's). `PrintGrid`, headers and footers are not printed yet.
//!
//! **Colour or black and white** ([`PrintColor`], [`Renderer::set_print_color`]): in black and white every colour a sheet draws is the grey of its
//! luma, by BT.601's weights (`gray.rs`; **ours**), the same in the preview's pixels and in the PDF.

use std::fmt;
use std::sync::Arc;

use diacraft_doc::{Document, SheetRef};
use kurbo::{Affine, BezPath, Rect, Shape as _};

use crate::export::{ExportError, export_limits};
use crate::gray::Grayed;
use crate::limits::{MAX_VIEW_PIXELS, MAX_VIEW_SIDE};
use crate::read::{Cx, eval_ctx};
use crate::skips::{SkipKind, Skipped, Skips};
use crate::target::Target;
use crate::{Limits, PageId, Rendered, Renderer, page_paper_size};

/// The most sheets one page takes. A page of 100 sheets is a wall; this is a drawing 32 sheets wide and 32 tall.
pub const MAX_SHEETS_PER_PAGE: u64 = 1_024;

/// The most sheets in one print, whatever the pages: each sheet is a walk of its page, so the cost is the sheets times the shapes.
pub const MAX_SHEETS_PER_JOB: usize = 4_096;

/// The most pages in one print.
pub const MAX_PAGES_PER_JOB: usize = 1_024;

/// The paper a code that is not in [`PAPERS`] prints on: US Letter (**ours**).
pub const DEFAULT_PAPER: u32 = 1;

/// The least printable width or height a sheet may be left with after its margins, in inches. Margins that leave less print nothing.
pub const MIN_PRINTABLE_INCHES: f64 = 0.1;

/// A margin is read as at most this many inches; a number past it is a hostile file's, and no sheet is that large.
const MAX_MARGIN_INCHES: f64 = 1_000.0;

/// How far past a whole number of sheets a page may reach and still take that many: float noise in a width, not a sliver of drawing.
const TILE_SLACK: f64 = 1.0e-6;

/// How a print's colours come out: as the drawing has them, or as greys (the Print view's **Black and white**). [`Renderer::set_print_color`]
/// sets it for the sheets a renderer draws ([`Renderer::render_sheet`] and [`draw_sheet`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PrintColor {
    /// The colours as they are.
    #[default]
    Color,
    /// Every colour as the grey of its luma, `0.299 R + 0.587 G + 0.114 B` (BT.601's weights, `diacraft_color::luma`), alpha kept: fills,
    /// lines, gradients, shadows, text, pictures and the colours of a theme alike (**ours**: a plain grayscale, not pure black
    /// on white).
    Grayscale,
}

impl PrintColor {
    /// The word `file.print` and `print.set` take and give for it: `color` or `grayscale`.
    pub const fn word(self) -> &'static str {
        match self {
            PrintColor::Color => "color",
            PrintColor::Grayscale => "grayscale",
        }
    }
}

/// A kind of paper: the code `PaperKind` holds, its name, and its size when held upright.
#[derive(Debug, PartialEq)]
pub struct Paper {
    pub kind: u32,
    pub name: &'static str,
    /// Inches across, upright: at most `height`.
    pub width: f64,
    pub height: f64,
}

const fn mm(n: f64) -> f64 {
    n / 25.4
}

/// The papers whose size we know, by the Windows paper code `PaperKind` holds. **Source:** the codes and their sizes are
/// those of Microsoft's public `XlPaperSize` page (Excel VBA reference, which uses the Windows codes; read 2026-11-07) and, for the
/// loose North American and ISO sizes, the standards the names stand for (ISO 216; the Wikipedia article "Paper size", read 2026-11-07). B5
/// is the JIS size, the page's own B5 row giving A5's by mistake. Ledger is held upright as Tabloid is: the page lists it as 17 by 11, a
/// sheet turned. The sheet's way round is the page's `PrintPageOrientation`'s, not the paper's.
pub const PAPERS: &[Paper] = &[
    Paper { kind: 1, name: "Letter", width: 8.5, height: 11.0 },
    Paper { kind: 2, name: "Letter Small", width: 8.5, height: 11.0 },
    Paper { kind: 3, name: "Tabloid", width: 11.0, height: 17.0 },
    Paper { kind: 4, name: "Ledger", width: 11.0, height: 17.0 },
    Paper { kind: 5, name: "Legal", width: 8.5, height: 14.0 },
    Paper { kind: 6, name: "Statement", width: 5.5, height: 8.5 },
    Paper { kind: 7, name: "Executive", width: 7.25, height: 10.5 },
    Paper { kind: 8, name: "A3", width: mm(297.0), height: mm(420.0) },
    Paper { kind: 9, name: "A4", width: mm(210.0), height: mm(297.0) },
    Paper { kind: 10, name: "A4 Small", width: mm(210.0), height: mm(297.0) },
    Paper { kind: 11, name: "A5", width: mm(148.0), height: mm(210.0) },
    Paper { kind: 12, name: "B4 (JIS)", width: mm(250.0), height: mm(354.0) },
    Paper { kind: 13, name: "B5 (JIS)", width: mm(182.0), height: mm(257.0) },
    Paper { kind: 14, name: "Folio", width: 8.5, height: 13.0 },
    Paper { kind: 15, name: "Quarto", width: mm(215.0), height: mm(275.0) },
    Paper { kind: 16, name: "10 x 14 in", width: 10.0, height: 14.0 },
    Paper { kind: 17, name: "11 x 17 in", width: 11.0, height: 17.0 },
    Paper { kind: 18, name: "Note", width: 8.5, height: 11.0 },
    Paper { kind: 24, name: "C sheet", width: 17.0, height: 22.0 },
    Paper { kind: 25, name: "D sheet", width: 22.0, height: 34.0 },
    Paper { kind: 26, name: "E sheet", width: 34.0, height: 44.0 },
];

/// The paper a code names, if it is in [`PAPERS`].
pub fn paper(kind: u32) -> Option<&'static Paper> {
    PAPERS.iter().find(|p| p.kind == kind)
}

/// US Letter, the paper of last resort. `PAPERS` always holds it, and the fallback is a literal so that no lookup can fail.
const LETTER: Paper = Paper { kind: 1, name: "Letter", width: 8.5, height: 11.0 };

/// Which way a sheet comes out of the printer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    /// The orientation of a rectangle: landscape when it is wider than tall (a page has no orientation cell). A
    /// square is portrait.
    pub fn of(width: f64, height: f64) -> Orientation {
        if width > height { Orientation::Landscape } else { Orientation::Portrait }
    }

    /// `portrait` or `landscape`.
    pub fn name(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }

    /// The orientation a word names, in any case.
    pub fn parse(text: &str) -> Option<Orientation> {
        match text.trim().to_ascii_lowercase().as_str() {
            "portrait" => Some(Orientation::Portrait),
            "landscape" => Some(Orientation::Landscape),
            _ => None,
        }
    }

    /// What `PrintPageOrientation` holds for it: 1 upright, 2 sideways.
    pub fn print_code(self) -> f64 {
        match self {
            Orientation::Portrait => 1.0,
            Orientation::Landscape => 2.0,
        }
    }
}

/// What a page's `Print Properties` cells say. Each field is read as the sheet holds it, with a
/// missing or hostile cell turned into the value the root style holds, so a setup is always one a plan can use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintSetup {
    /// `PaperKind`.
    pub paper_kind: u32,
    /// `PrintPageOrientation` as the cell holds it: 0 as the printer is set (and 0 when the page holds none), 1 upright, 2 sideways.
    pub orientation_code: i64,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub centre_x: bool,
    pub centre_y: bool,
    /// `OnPage`: fit the page to `PagesX` by `PagesY` sheets and ignore `ScaleX`.
    pub fit: bool,
    pub pages_x: u32,
    pub pages_y: u32,
    pub scale_x: f64,
    pub scale_y: f64,
    /// `PrintGrid`: read and kept, but the grid is not printed yet.
    pub print_grid: bool,
}

impl PrintSetup {
    /// The setup of `page`, or `None` for a page the document lacks.
    pub fn of(doc: &Document, page: PageId) -> Option<PrintSetup> {
        doc.page(page)?;
        let mut ctx = eval_ctx();
        let cx = Cx::new(doc, &mut ctx);
        let sheet = SheetRef::Page(page);
        let margin = |name: &str| {
            let n = cx.num_or(&sheet, name, 0.25);
            if n.is_finite() { n.clamp(0.0, MAX_MARGIN_INCHES) } else { 0.25 }
        };
        let count = |name: &str| {
            let n = cx.num_or(&sheet, name, 1.0);
            if n.is_finite() { n.round().clamp(1.0, MAX_SHEETS_PER_PAGE as f64) as u32 } else { 1 }
        };
        let scale = |name: &str| {
            let n = cx.num_or(&sheet, name, 1.0);
            if n.is_finite() && n > 0.0 { n } else { 1.0 }
        };
        let code = |name: &str, default: f64| {
            let n = cx.num_or(&sheet, name, default);
            if n.is_finite() { n.round() as i64 } else { default as i64 }
        };
        Some(PrintSetup {
            paper_kind: u32::try_from(code("PaperKind", f64::from(DEFAULT_PAPER))).unwrap_or(DEFAULT_PAPER),
            orientation_code: code("PrintPageOrientation", 0.0),
            left: margin("PageLeftMargin"),
            right: margin("PageRightMargin"),
            top: margin("PageTopMargin"),
            bottom: margin("PageBottomMargin"),
            centre_x: cx.flag(&sheet, "CenterX", false),
            centre_y: cx.flag(&sheet, "CenterY", false),
            fit: cx.flag(&sheet, "OnPage", false),
            pages_x: count("PagesX"),
            pages_y: count("PagesY"),
            scale_x: scale("ScaleX"),
            scale_y: scale("ScaleY"),
            print_grid: cx.flag(&sheet, "PrintGrid", false),
        })
    }

    /// The sheet's way round for a page of `page` inches across and down: sideways for 2, upright for 1, and for anything else (0, a missing cell,
    /// a number no list has) the way the page itself is held, landscape when it is wider than tall.
    pub fn orientation(&self, page: (f64, f64)) -> Orientation {
        match self.orientation_code {
            2 => Orientation::Landscape,
            1 => Orientation::Portrait,
            _ if page.0 > page.1 => Orientation::Landscape,
            _ => Orientation::Portrait,
        }
    }
}

/// Why a print cannot be planned. The words hold no path and no command id.
#[derive(Clone, Debug, PartialEq)]
pub enum PrintError {
    /// The document has no page with this `ID`.
    NoSuchPage(PageId),
    /// The page has no size, so there is nothing to put on paper.
    NoPageSize { name: String },
    /// The margins leave no room on the sheet.
    NoPrintableArea { name: String, width: f64, height: f64 },
    /// The page would take more sheets than a print may.
    TooManySheets { name: String, needed: u64 },
    /// The print has more sheets or pages than a print may.
    TooLarge(String),
    /// Nothing is left to print: the drawing has no page that prints.
    NothingToPrint,
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrintError::NoSuchPage(id) => write!(f, "the drawing has no page {id}"),
            PrintError::NoPageSize { name } => write!(f, "the page {name} has no size, so there is nothing to print"),
            PrintError::NoPrintableArea { name, width, height } => {
                write!(f, "the margins of the page {name} leave {width:.2} by {height:.2} inches of the sheet to print on; make the margins smaller")
            }
            PrintError::TooManySheets { name, needed } => write!(
                f,
                "the page {name} would take {needed} sheets of paper, and the most DiaCraft prints for one page is {MAX_SHEETS_PER_PAGE}; \
                 print it smaller or on larger paper"
            ),
            PrintError::TooLarge(what) => write!(f, "this print is too large: {what}"),
            PrintError::NothingToPrint => write!(f, "the drawing has no page to print"),
        }
    }
}

impl std::error::Error for PrintError {}

/// One sheet of paper, and where its page lands on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// The page this sheet shows a part of.
    pub page: PageId,
    /// Which sheet of the print this is, from 0. Set by [`plan_job`]; [`plan_page`] numbers from 0 within the page.
    pub number: usize,
    /// Which sheet of its page, from 0, and the tile: the column and the row.
    pub index: usize,
    pub col: u32,
    pub row: u32,
    /// The sheet in inches as it comes out, across and down.
    pub width: f64,
    pub height: f64,
    /// Where ink may land on the sheet, in sheet inches (origin at the sheet's top left, y down): the sheet inside its margins.
    pub printable: Rect,
    /// Page paper inches (the page's lower left corner is the origin, y up, [`crate::View::to_px`]'s space) to sheet inches (y down).
    pub to_sheet: Affine,
}

impl Sheet {
    /// The printable part of the sheet in page paper inches: the clip that cuts off what the margins and the neighbouring tiles own.
    pub fn clip_in_page(&self) -> BezPath {
        self.clip_rect_in_page().to_path(0.1)
    }

    /// The same as a rectangle: a sheet's transform only scales and moves, so the printable part is still a rectangle in page paper inches.
    pub fn clip_rect_in_page(&self) -> Rect {
        let back = if self.to_sheet.determinant().abs() > 1.0e-18 { self.to_sheet.inverse() } else { Affine::IDENTITY };
        back.transform_rect_bbox(self.printable)
    }
}

/// How one page prints.
#[derive(Clone, Debug, PartialEq)]
pub struct PagePlan {
    pub page: PageId,
    pub name: Arc<str>,
    pub setup: PrintSetup,
    /// The paper the sheets are (an unknown code is Letter) and whether the code was found.
    pub paper: &'static Paper,
    pub paper_known: bool,
    pub orientation: Orientation,
    /// The sheet in inches as it comes out, across and down.
    pub sheet_size: (f64, f64),
    /// The printable part of a sheet, across and down.
    pub printable_size: (f64, f64),
    /// The page's size on paper at scale 1 (its width and height times the drawing scale), and the scale it prints at.
    pub page_size: (f64, f64),
    pub scale: f64,
    /// The tiles across and down.
    pub across: u32,
    pub down: u32,
    pub sheets: Vec<Sheet>,
}

/// A page's tiles: how many printable widths of `size` it takes, never fewer than 1.
fn tiles(size: f64, printable: f64) -> u64 {
    let n = size / printable;
    if !n.is_finite() {
        return u64::MAX;
    }
    // `n` is finite and positive; the cast saturates.
    ((n - TILE_SLACK).ceil().max(1.0)) as u64
}

/// Works out how `page` prints. See the module documentation for the rules.
pub fn plan_page(doc: &Document, page: PageId) -> Result<PagePlan, PrintError> {
    let found = doc.page(page).ok_or(PrintError::NoSuchPage(page))?;
    let name = Arc::clone(&found.name);
    let (pw, ph) = page_paper_size(doc, page).ok_or_else(|| PrintError::NoPageSize { name: name.to_string() })?;
    let setup = PrintSetup::of(doc, page).ok_or(PrintError::NoSuchPage(page))?;
    let known = paper(setup.paper_kind);
    let paper = known.unwrap_or(&LETTER);
    let orientation = setup.orientation((pw, ph));
    let (sw, sh) = match orientation {
        Orientation::Portrait => (paper.width, paper.height),
        Orientation::Landscape => (paper.height, paper.width),
    };
    let (printable_w, printable_h) = (sw - setup.left - setup.right, sh - setup.top - setup.bottom);
    if !(printable_w >= MIN_PRINTABLE_INCHES && printable_h >= MIN_PRINTABLE_INCHES) {
        return Err(PrintError::NoPrintableArea { name: name.to_string(), width: printable_w.max(0.0), height: printable_h.max(0.0) });
    }
    let scale = if setup.fit {
        let across = f64::from(setup.pages_x) * printable_w / pw;
        let down = f64::from(setup.pages_y) * printable_h / ph;
        across.min(down)
    } else {
        setup.scale_x
    };
    let (dw, dh) = (pw * scale, ph * scale);
    if !(scale.is_finite() && scale > 0.0 && dw.is_finite() && dh.is_finite()) {
        return Err(PrintError::NoPageSize { name: name.to_string() });
    }
    let (across, down) = (tiles(dw, printable_w), tiles(dh, printable_h));
    let needed = across.saturating_mul(down);
    if needed > MAX_SHEETS_PER_PAGE {
        return Err(PrintError::TooManySheets { name: name.to_string(), needed });
    }
    // Both counts are at most the cap now: their product is, and neither is 0.
    let (across, down) = (u32::try_from(across).unwrap_or(1), u32::try_from(down).unwrap_or(1));
    let (assembled_w, assembled_h) = (f64::from(across) * printable_w, f64::from(down) * printable_h);
    let ox = if setup.centre_x { (assembled_w - dw) / 2.0 } else { 0.0 };
    let oy = if setup.centre_y { (assembled_h - dh) / 2.0 } else { 0.0 };
    let printable = Rect::new(setup.left, setup.top, sw - setup.right, sh - setup.bottom);
    let mut sheets = Vec::with_capacity(usize::try_from(needed).unwrap_or(1));
    for row in 0..down {
        for col in 0..across {
            // A point of the page, y up, lands on the sheet at its tile's offset from the assembled printable area, y down.
            let tx = setup.left + ox - f64::from(col) * printable_w;
            let ty = setup.top + oy + dh - f64::from(row) * printable_h;
            sheets.push(Sheet {
                page,
                number: sheets.len(),
                index: sheets.len(),
                col,
                row,
                width: sw,
                height: sh,
                printable,
                to_sheet: Affine::new([scale, 0.0, 0.0, -scale, tx, ty]),
            });
        }
    }
    Ok(PagePlan {
        page,
        name,
        setup,
        paper,
        paper_known: known.is_some(),
        orientation,
        sheet_size: (sw, sh),
        printable_size: (printable_w, printable_h),
        page_size: (pw, ph),
        scale,
        across,
        down,
        sheets,
    })
}

/// The pages a print of the whole drawing covers, in file order: the foreground pages, and not a page whose `UIVisibility` is 1 (**ours**: a
/// page that is hidden from the tabs is not one the person asked to print). A background page is printed behind the pages that use it and
/// not on its own.
pub fn printable_pages(doc: &Document) -> Vec<PageId> {
    let mut ctx = eval_ctx();
    let cx = Cx::new(doc, &mut ctx);
    doc.pages
        .iter()
        .filter(|p| !p.background)
        .filter(|p| !cx.flag(&SheetRef::Page(p.id), "UIVisibility", false))
        .map(|p| p.id)
        .take(MAX_PAGES_PER_JOB + 1)
        .collect()
}

/// The sheets of a print.
#[derive(Clone, Debug, PartialEq)]
pub struct JobPlan {
    pub pages: Vec<PagePlan>,
}

/// Plans a print of `pages`, in the order given. Sheets are numbered through the whole print.
pub fn plan_job(doc: &Document, pages: &[PageId]) -> Result<JobPlan, PrintError> {
    if pages.is_empty() {
        return Err(PrintError::NothingToPrint);
    }
    if pages.len() > MAX_PAGES_PER_JOB {
        return Err(PrintError::TooLarge(format!("it has more than {MAX_PAGES_PER_JOB} pages")));
    }
    let mut plans = Vec::with_capacity(pages.len());
    let mut total = 0usize;
    for page in pages {
        let mut plan = plan_page(doc, *page)?;
        total = total.saturating_add(plan.sheets.len());
        if total > MAX_SHEETS_PER_JOB {
            return Err(PrintError::TooLarge(format!("it would take more than {MAX_SHEETS_PER_JOB} sheets of paper")));
        }
        let first = total - plan.sheets.len();
        for sheet in &mut plan.sheets {
            sheet.number += first;
        }
        plans.push(plan);
    }
    Ok(JobPlan { pages: plans })
}

/// A run of sheets that take the same paper: what one print job to a printer holds, since a printer takes one paper and a driver one
/// orientation to a job.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The sheets' numbers: `first..first + count`.
    pub first: usize,
    pub count: usize,
    /// The sheet in inches, the shorter side first (a sheet turned is the same paper).
    pub paper: (f64, f64),
}

impl JobPlan {
    /// How many sheets the print takes.
    pub fn sheet_count(&self) -> usize {
        self.pages.iter().map(|p| p.sheets.len()).sum()
    }

    /// Every sheet, in print order.
    pub fn sheets(&self) -> impl Iterator<Item = (&PagePlan, &Sheet)> {
        self.pages.iter().flat_map(|p| p.sheets.iter().map(move |s| (p, s)))
    }

    /// The `n`th sheet of the print (from 0) and the plan of its page.
    pub fn sheet(&self, n: usize) -> Option<(&PagePlan, &Sheet)> {
        self.sheets().nth(n)
    }

    /// The runs of consecutive sheets that take the same paper, whichever way round. Papers are told apart to a hundredth of an inch.
    pub fn runs(&self) -> Vec<Run> {
        let key = |s: &Sheet| {
            let (a, b) = if s.width <= s.height { (s.width, s.height) } else { (s.height, s.width) };
            ((a * 100.0).round() as i64, (b * 100.0).round() as i64, (a, b))
        };
        let mut runs: Vec<(Run, (i64, i64))> = Vec::new();
        for (n, (_, sheet)) in self.sheets().enumerate() {
            let (ka, kb, paper) = key(sheet);
            match runs.last_mut() {
                Some((run, k)) if *k == (ka, kb) => run.count += 1,
                _ => runs.push((Run { first: n, count: 1, paper }, (ka, kb))),
            }
        }
        runs.into_iter().map(|(run, _)| run).collect()
    }
}

/// `path` (a straight-sided path, in paper inches) cut to `rect`, as a stroke when `stroke` and as a fill when not: what a writer of one sheet of a
/// tiled page keeps of a long path, so that a polyline of thousands of points that crosses thirty tiles is not written whole on each. `None` when
/// the path has a curve in it, which is not cut. A dash pattern restarts where a cut stroke enters the rectangle, so a caller does not cut a
/// dashed one; and the join at the first point of a closed outline becomes a plain end when the cut leaves that point inside.
pub fn cut_to(path: &BezPath, rect: Rect, stroke: bool) -> Option<BezPath> {
    if stroke { crate::clip::polyline(path, rect) } else { crate::clip::polygon(path, rect) }
}

/// Draws `sheet` onto `target`. The target's `to_px` takes page paper inches (as every target's does) to its own units, so a caller makes
/// it the units to the inch times [`Sheet::to_sheet`], and its size is the sheet's. The print's walk is the export's (limits of an export,
/// no grid and no guides), on no paper (the printer's is the paper), with non-printing shapes and unprinted layers left off, and **clipped
/// to the sheet's printable part** so that what lies in a margin or on a neighbouring tile is not drawn. In black and white
/// ([`Renderer::set_print_color`]) the target is told every colour as its grey, and draws no blur (a blurred shadow is drawn sharp and said to be,
/// as a PDF's always is).
///
/// Returns what was left out or drawn by a stand-in, as the export's list does. A bug in drawing is an error, since a sheet with a part
/// missing cannot say which.
pub fn draw_sheet<T: Target>(renderer: &mut Renderer, doc: &Document, sheet: &Sheet, target: &mut T) -> Result<Vec<Skipped>, ExportError> {
    draw_sheet_within(renderer, doc, sheet, target, export_limits())
}

/// As [`draw_sheet`], within `limits`.
pub fn draw_sheet_within<T: Target>(
    renderer: &mut Renderer,
    doc: &Document,
    sheet: &Sheet,
    target: &mut T,
    limits: Limits,
) -> Result<Vec<Skipped>, ExportError> {
    let (kept_limits, kept_paper, kept_print) = (renderer.limits(), renderer.paper(), renderer.print);
    renderer.set_limits(limits);
    renderer.set_paper(None);
    renderer.print = true;
    let mark = target.mark();
    target.push_clip(&sheet.clip_in_page());
    let skipped = match renderer.print_color {
        PrintColor::Color => renderer.draw_onto(doc, sheet.page, target),
        PrintColor::Grayscale => renderer.draw_onto(doc, sheet.page, &mut Grayed::new(target)),
    };
    target.rewind(mark);
    renderer.set_limits(kept_limits);
    renderer.set_paper(kept_paper);
    renderer.print = kept_print;
    if let Some(bug) = skipped.iter().find(|s| s.kind == SkipKind::Recovered) {
        return Err(ExportError::Bug(bug.first.clone()));
    }
    if skipped.iter().any(|s| s.kind == SkipKind::NoSuchPage) {
        return Err(ExportError::NoSuchPage(sheet.page));
    }
    Ok(skipped)
}

impl Renderer {
    /// How the sheets this renderer draws from now on show their colours: [`Renderer::render_sheet`] and [`draw_sheet`] follow it, and a page
    /// drawn for the screen or an export never does. [`PrintColor::Color`] until it is set.
    pub fn set_print_color(&mut self, color: PrintColor) {
        self.print_color = color;
    }

    /// How the sheets this renderer draws show their colours.
    pub fn print_color(&self) -> PrintColor {
        self.print_color
    }

    /// Draws `sheet` of a print as pixels at `px_per_inch`: the sheet's size, rounded up, with its page where the plan puts it and ink only
    /// inside the sheet's margins. The paper is not drawn (the sheet is transparent where nothing is), so a preview lays its own white
    /// under it. Drawn within the renderer's own limits and cancel, as [`Renderer::render`] is; the walk is the print's (see the module
    /// documentation). In black and white ([`Renderer::set_print_color`]) each pixel is the grey of the colour sheet's: the luma is a weighted
    /// mean, so that is what turning each colour before blending gives, and a shadow keeps its blur. Never panics: a view that is too large or
    /// a bug in drawing is in [`Rendered::skipped`].
    pub fn render_sheet(&mut self, doc: &Document, sheet: &Sheet, px_per_inch: f64) -> Rendered {
        if !(px_per_inch.is_finite() && px_per_inch > 0.0) {
            return Rendered::refused("a resolution that is not a positive number".to_owned());
        }
        let (w, h) = ((sheet.width * px_per_inch).ceil(), (sheet.height * px_per_inch).ceil());
        if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0 && w <= f64::from(MAX_VIEW_SIDE) && h <= f64::from(MAX_VIEW_SIDE)) {
            return Rendered::refused(format!("a sheet of {} by {} pixels", w, h));
        }
        if (w as u64).saturating_mul(h as u64) > MAX_VIEW_PIXELS {
            return Rendered::refused(format!("a sheet of {} by {} pixels", w as u64, h as u64));
        }
        let (Ok(pw), Ok(ph)) = (u16::try_from(w as u64), u16::try_from(h as u64)) else {
            return Rendered::refused("a sheet too wide for the rasteriser".to_owned());
        };
        let coeffs = sheet.to_sheet.as_coeffs();
        if !coeffs.iter().all(|c| c.is_finite()) {
            return Rendered::refused("a sheet placed by numbers that are not numbers".to_owned());
        }
        let (width, height) = (u32::from(pw), u32::from(ph));
        let blank = || vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
        if doc.page(sheet.page).is_none() {
            let mut skips = Skips::default();
            skips.note(SkipKind::NoSuchPage, || format!("page {}", sheet.page));
            return Rendered { rgba_premul: blank(), width, height, skipped: skips.into_list() };
        }
        let to_px = Affine::scale(px_per_inch) * sheet.to_sheet;
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (kept_paper, kept_print) = (self.paper, self.print);
            self.paper = None;
            self.print = true;
            let clip = sheet.clip_in_page();
            let drawn = self.rasterise(doc, sheet.page, pw, ph, to_px, Some(&clip));
            self.paper = kept_paper;
            self.print = kept_print;
            drawn.map(|(mut pixels, skips)| {
                if !skips.has(SkipKind::Cancelled) && self.print_color == PrintColor::Grayscale {
                    diacraft_color::gray_pixels(&mut pixels);
                }
                (pixels, skips)
            })
        }));
        let bug = match drawn {
            Ok(Ok((rgba_premul, skips))) => return Rendered { rgba_premul, width, height, skipped: skips.into_list() },
            Ok(Err(words)) => words,
            Err(_) => "a sheet that met a bug".to_owned(),
        };
        self.caches.clear();
        self.share_caches.clear();
        // The flags a panic left set are put back: the next sheet is drawn as the caller set the renderer.
        self.print = false;
        let mut skips = Skips::default();
        skips.note(SkipKind::Recovered, || bug);
        Rendered { rgba_premul: blank(), width, height, skipped: skips.into_list() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_papers_are_upright_unique_and_letter_is_the_fallback() {
        let mut kinds: Vec<u32> = PAPERS.iter().map(|p| p.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), PAPERS.len(), "a code is listed twice");
        for p in PAPERS {
            assert!(p.width > 0.0 && p.width <= p.height, "{} is not held upright", p.name);
        }
        assert_eq!(paper(DEFAULT_PAPER), Some(&LETTER), "the literal fallback is the table's Letter");
        assert!((paper(9).map(|p| p.width).unwrap_or(0.0) - 8.2677).abs() < 1.0e-3, "A4 is 210 mm");
    }

    #[test]
    fn a_page_takes_as_many_tiles_as_its_width_needs_and_float_noise_takes_none() {
        assert_eq!(tiles(8.0, 8.0), 1);
        assert_eq!(tiles(8.000_000_1, 8.0), 1, "a hundred-nanoinch sliver is float noise");
        assert_eq!(tiles(8.01, 8.0), 2);
        assert_eq!(tiles(0.3, 8.0), 1, "a small page takes a sheet");
        assert_eq!(tiles(f64::INFINITY, 8.0), u64::MAX);
    }
}
