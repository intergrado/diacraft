//! The seam between the drawing area and what a page draws: [`paint_page_paper`] and [`paint_page_picture`].
//!
//! The canvas asks for a page's drawing through these two functions and nowhere else: the paper first, then, over the grid, the
//! picture `render` drew for the page (`picture.rs` asks the render worker for it and keeps it). The grid and the rulers are
//! drawn by the canvas, never here and never in the picture, because neither is page content.
//!
//! **Order (a project rule, no spec gives one):** the paper, the grid over it, then the shapes of the page and of its background
//! pages over the grid, so that a shape with a fill hides the grid behind it.

use diacraft_engine::doc::{Cell, Document};
use diacraft_units::{Measure, Unit, parse_unit};
use egui::{CornerRadius, Painter, Rect, Shadow, Stroke, StrokeKind, pos2, vec2};

use super::picture::PageImage;
use crate::theme::{self, Tokens, size};

/// US Letter landscape, in inches: what a new document's page is (`crates/doc`, `default_page_sheet`) and what a page
/// whose sheet holds no readable size is shown as.
pub const DEFAULT_PAGE_INCHES: (f64, f64) = (11.0, 8.5);

/// The roots of the `Ruler & Grid` cells, which a page that holds none reads:
/// the root style's `XGridDensity` 8, `XGridSpacing` 0 and origin 0, `XRulerDensity` 32.
const ROOT_GRID_DENSITY: f64 = 8.0;
const ROOT_RULER_DENSITY: f64 = 32.0;

/// A page's size and how its grid and rulers look, as its sheet holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSettings {
    /// Width and height, in inches.
    pub size: (f64, f64),
    /// `XGridDensity` and `YGridDensity`.
    pub grid_density: (f64, f64),
    /// `XGridSpacing` and `YGridSpacing`, in inches.
    pub grid_spacing: (f64, f64),
    /// `XGridOrigin` and `YGridOrigin`, in inches.
    pub grid_origin: (f64, f64),
    /// `XRulerOrigin` and `YRulerOrigin`: where zero falls on each ruler, in inches from the page's lower left.
    pub ruler_origin: (f64, f64),
    /// `XRulerDensity` and `YRulerDensity`.
    pub ruler_density: (f64, f64),
    /// The unit the rulers count in: the unit `U` of the page's `DrawingScale`.
    /// The inch where the cell has none or names no length.
    pub ruler_unit: Unit,
    /// Paper inches over drawing inches (`PageScale` over `DrawingScale`, 1 where either is missing or not above 0): the rulers
    /// measure at the drawing's scale, so a point `p` paper inches along reads `p` over this, less the ruler origin.
    pub drawing_scale: f64,
}

impl Default for PageSettings {
    fn default() -> Self {
        PageSettings {
            size: DEFAULT_PAGE_INCHES,
            grid_density: (ROOT_GRID_DENSITY, ROOT_GRID_DENSITY),
            grid_spacing: (0.0, 0.0),
            grid_origin: (0.0, 0.0),
            ruler_origin: (0.0, 0.0),
            ruler_density: (ROOT_RULER_DENSITY, ROOT_RULER_DENSITY),
            ruler_unit: Unit::Inch,
            drawing_scale: 1.0,
        }
    }
}

/// The unit a cell's `U` names, if it names a length.
fn length_unit(cell: Option<&Cell>) -> Option<Unit> {
    let unit = parse_unit(cell?.unit.as_deref()?)?;
    (unit.measure() == Measure::Length).then_some(unit)
}

/// The number a cell of the page's own sheet holds, if it holds a finite one.
fn number(cell: Option<&Cell>) -> Option<f64> {
    cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).filter(|n| n.is_finite())
}

/// The settings of page `id` of `doc`. A cell the page does not hold reads the root's value; a size that is not a
/// positive number reads as the default page's.
pub fn page_settings(doc: &Document, id: u32) -> Option<PageSettings> {
    let page = doc.page(id)?;
    let get = |name: &str| number(page.sheet.cell(name));
    let (root, mut s) = (PageSettings::default(), PageSettings::default());
    // The page's size on paper, as the renderer reads it (`PageWidth` and `PageHeight` times the page's scale), so that Fit shows the
    // page the picture is drawn on. A size that is not a number, or is absurd, reads as the default page's.
    s.size = diacraft_render::page_paper_size(doc, id).filter(|(w, h)| *w < 1.0e5 && *h < 1.0e5).unwrap_or(root.size);
    let pair = |x: &str, y: &str, fallback: (f64, f64)| (get(x).unwrap_or(fallback.0), get(y).unwrap_or(fallback.1));
    s.grid_density = pair("XGridDensity", "YGridDensity", root.grid_density);
    s.grid_spacing = pair("XGridSpacing", "YGridSpacing", root.grid_spacing);
    s.grid_origin = pair("XGridOrigin", "YGridOrigin", root.grid_origin);
    s.ruler_origin = pair("XRulerOrigin", "YRulerOrigin", root.ruler_origin);
    s.ruler_density = pair("XRulerDensity", "YRulerDensity", root.ruler_density);
    s.ruler_unit = length_unit(page.sheet.cell("DrawingScale")).unwrap_or(root.ruler_unit);
    let positive = |name: &str| get(name).filter(|n| *n > 0.0).unwrap_or(1.0);
    let scale = positive("PageScale") / positive("DrawingScale");
    s.drawing_scale = if scale.is_finite() && scale > 0.0 { scale } else { root.drawing_scale };
    Some(s)
}

/// Where the page is on the screen.
#[derive(Clone, Copy, Debug)]
pub struct PageFrame {
    /// The page's id in the document.
    pub id: u32,
    /// The page's rectangle on the screen, in points.
    pub rect: Rect,
    /// Points to one inch.
    pub scale: f64,
}

/// Paints the paper of page `frame`: its soft edge, its fill and its border. The first of the two calls the canvas makes for a
/// page, because the grid goes over the paper and under the drawing.
pub fn paint_page_paper(painter: &Painter, t: &Tokens, frame: &PageFrame) {
    // The soft edge: a shadow that falls a little below the page.
    painter.add(Shadow { offset: [0, 2], blur: 12, spread: 1, color: t.page_shadow }.as_shape(frame.rect, CornerRadius::ZERO));
    painter.rect_filled(frame.rect, CornerRadius::ZERO, t.page_fill);
    painter.rect_stroke(frame.rect, CornerRadius::ZERO, Stroke::new(1.0, t.page_border), StrokeKind::Outside);
}

/// Paints the words of a page whose drawing failed, in a panel in the middle of the part of the page on view, over whatever the picture
/// shows. A small mark in the status bar is not enough for a page that came up empty. → the panel's rectangle.
pub fn paint_page_lost(painter: &Painter, t: &Tokens, frame: &PageFrame, words: &str) -> Rect {
    let clip = painter.clip_rect();
    let on_view = frame.rect.intersect(clip);
    let on_view = if on_view.is_finite() && on_view.width() > 0.0 && on_view.height() > 0.0 { on_view } else { clip };
    let wrap = (on_view.width() - 40.0).clamp(120.0, 440.0);
    let galley = painter.layout(words.to_owned(), theme::font(size::TAB), t.text, wrap);
    let panel = Rect::from_center_size(on_view.center(), galley.size() + vec2(28.0, 20.0));
    painter.rect_filled(panel, CornerRadius::same(6), t.panel_fill);
    painter.rect_stroke(panel, CornerRadius::same(6), Stroke::new(1.0, t.tile_border), StrokeKind::Inside);
    painter.galley(panel.min + vec2(14.0, 10.0), galley, t.text);
    panel
}

/// Paints the picture of the page: what `render` drew, clear where it drew nothing, laid where the page is now. The second call
/// the canvas makes. A picture drawn for another view is still laid by the inches it covers, so it stays on its shapes while a new
/// one is drawn. Nothing is painted for a page with no picture yet.
pub fn paint_page_picture(painter: &Painter, t: &Tokens, image: Option<&PageImage>, frame: &PageFrame) {
    let Some(image) = image else { return };
    let rect = image.screen_rect(frame.rect, frame.scale);
    if rect.width() > 0.0
        && rect.height() > 0.0
        && rect.min.x.is_finite()
        && rect.max.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.y.is_finite()
    {
        painter.image(image.texture_id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), t.picture_tint);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use diacraft_testkit::build::DocBuilder;
    use diacraft_testkit::build::PageBuilder;

    use super::*;

    fn settings_of(page: PageBuilder) -> PageSettings {
        page_settings(&DocBuilder::new().page(1, page).build(), 1).unwrap()
    }

    #[test]
    fn the_rulers_count_in_the_unit_of_the_pages_drawing_scale() {
        // A metric page: 1 mm of paper to 1 mm of drawing, stored in inches with the unit MM.
        let metric = PageBuilder::new()
            .cell("PageWidth", "11.693")
            .cell("PageHeight", "8.268")
            .cell_u("PageScale", "0.0393701", "MM")
            .cell_u("DrawingScale", "0.0393701", "MM")
            .cell("XRulerOrigin", "1.5");
        let s = settings_of(metric);
        assert_eq!(s.ruler_unit, Unit::Millimetre);
        assert_eq!(s.drawing_scale, 1.0);
        assert_eq!(s.ruler_origin.0, 1.5);
    }

    #[test]
    fn a_scaled_page_gives_paper_over_drawing_and_a_page_with_no_unit_counts_inches() {
        // 1 inch of paper to 12 inches (1 foot) of drawing.
        let scaled = PageBuilder::new().cell("PageWidth", "96").cell("PageHeight", "72").cell("PageScale", "1").cell_u("DrawingScale", "12", "FT");
        let s = settings_of(scaled);
        assert_eq!(s.ruler_unit, Unit::Foot);
        assert!((s.drawing_scale - 1.0 / 12.0).abs() < 1e-12);
        let bare = PageBuilder::new().cell("PageWidth", "4").cell("PageHeight", "4").cell("PageScale", "1").cell("DrawingScale", "1");
        let s = settings_of(bare);
        assert_eq!((s.ruler_unit, s.drawing_scale), (Unit::Inch, 1.0));
    }

    #[test]
    fn a_unit_that_is_no_length_or_no_unit_at_all_reads_inches_and_a_scale_that_is_no_number_reads_one() {
        let odd = PageBuilder::new().cell("PageWidth", "4").cell("PageHeight", "4").cell_u("PageScale", "0", "DEG").cell_u(
            "DrawingScale",
            "-3",
            "not a unit",
        );
        let s = settings_of(odd);
        assert_eq!((s.ruler_unit, s.drawing_scale), (Unit::Inch, 1.0));
    }
}
