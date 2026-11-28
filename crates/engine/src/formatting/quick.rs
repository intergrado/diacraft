//! Quick Styles ("applies a ready-made combination of line and fill to the selected shapes; the styles are drawn from
//! the drawing's theme"): `shape.quick_style` and the gallery's tiles, `shape.quick_styles`.
//!
//! **A Quick Style is two numbers**: an entry of the theme's lists, and a colour. The cells that hold them are
//! `QuickStyleLineMatrix`, `QuickStyleFillMatrix` and `QuickStyleEffectsMatrix` (the entry: 1 to 6, or 100 to 103 for the first to fourth style of the page's
//! style variant) and `QuickStyleLineColor`, `QuickStyleFillColor` and `QuickStyleShadowColor` (the colour: 100 to 106 for the first to seventh colour of the page's
//! colour variant). The theme answers the cells of line, fill and shadow that call `THEMEVAL()` with them. **The gallery is the two grids the reference
//! application shows** ("Variant Styles" with four tiles, and "Theme Styles" in seven columns and six rows): a Variant Style is entry 100 to 103 in colour 100,
//! and a Theme Style at row `r` and column `c` is entry `r` in colour `99 + c` (ours: the page says only the seven columns and six rows, and
//! the seven variant colours and six list entries are what a theme has to fill them).
//!
//! **What applying writes** to each shape without members inside the shapes named: the six Quick Style cells, and so that the theme answers them
//! on this shape and no other: the shape follows the page's `Theme` style for the families that have no style of their own or the root style (as
//! `theme.apply` and a shape put on a themed page do), and every cell of the line, fill and shadow families (and the stops of the fill's
//! gradient, and the scheme cells that send a shape to its page) that still does not read the theme's marker is written as the call `THEMEVAL()` with the marker
//! as its value, as the shapes of files the reference application saved hold them (the formulas replace the colours and weights a person set, which is
//! what choosing a style does). Cells a shape that follows the `Theme` style already reads from it are not written. **A page with no theme** has none to answer: the
//! command fails in words, and so does a drawing with no `Theme` style (a page that selects a theme and a drawing that holds none).
//!
//! **A member reached through a group** keeps the line or the fill it does not draw (its `LinePattern` or `FillPattern` is not written as `THEMEVAL()`), and one filled with
//! its own line colour keeps its fill cells (`formatting/mod.rs`).
//!
//! **Bounded.** At most [`MAX_QUICK_SHAPES`] shapes in one command, each with at most about forty cells; the gallery makes one copy of the document and one reading
//! for each of its 46 tiles (a copy of a document is a handful of pointers) and reads one shape.

use std::collections::BTreeMap;

use diacraft_doc::{Cell, CellAddr, CellName, Document, ShapeRef, SheetRef};

use super::effects::fx_look;
use super::look::{FxLook, Reader, UnsavedBudget, eval_ctx};
use super::shape_cells::{MAX_STOP_ROWS, row_addr};
use super::{Edits, New, Rgb, fill_is_line, first_leaf, keeps_none, targets_reached};
use crate::draw::Problem;
use crate::theme::{follow_page_theme_refs, selection_of, theme_style_id};

/// The most shapes one `shape.quick_style` works on: a style is thirty or forty cells for a shape that does not follow the `Theme` style.
pub const MAX_QUICK_SHAPES: usize = 4_096;

/// The cells of the line, fill and shadow families the theme answers for a Quick Style.
const LINE_CELLS: [&str; 5] = ["LineColor", "LineWeight", "LinePattern", "LineCap", "LineColorTrans"];
const FILL_CELLS: [&str; 8] = [
    "FillForegnd",
    "FillBkgnd",
    "FillPattern",
    "FillForegndTrans",
    "FillGradientEnabled",
    "FillGradientDir",
    "FillGradientAngle",
    "RotateGradientWithShape",
];
const SHADOW_CELLS: [&str; 8] = [
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShdwPattern",
    "ShapeShdwType",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwScaleFactor",
    "ShapeShdwBlur",
];
/// The cells that send a shape to its page's theme: 65534.
const SCHEME_CELLS: [&str; 8] = [
    "ThemeIndex",
    "ColorSchemeIndex",
    "EffectSchemeIndex",
    "ConnectorSchemeIndex",
    "FontSchemeIndex",
    "EmbellishmentIndex",
    "VariationColorIndex",
    "VariationStyleIndex",
];
/// What the scheme cells hold to read the page's.
const PAGE_NUMBER: u32 = 65_534;

/// The entry and the colour of a Quick Style: the numbers its cells hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuickStyle {
    /// 1 to 6, or 100 to 103.
    pub matrix: u32,
    /// 100 to 106 (or 0 to 8: the theme's own colours, which the cells allow and the gallery does not offer).
    pub colour: u32,
}

impl QuickStyle {
    /// The Theme Style at this row (1 to 6) and column (1 to 7) of the gallery.
    pub fn at(row: u32, column: u32) -> Option<QuickStyle> {
        // The style is made only for a place of the gallery: `99 + column` of any other column could pass the largest whole number.
        ((1..=6).contains(&row) && (1..=7).contains(&column)).then(|| QuickStyle { matrix: row, colour: 99 + column })
    }

    /// The Variant Style at this place (0 to 3): the first to fourth style of the page's style variant, in the first colour.
    pub fn variant(place: u32) -> Option<QuickStyle> {
        (place <= 3).then(|| QuickStyle { matrix: 100 + place, colour: 100 })
    }
}

/// Puts the Quick Style `style` on the shapes `ids` of a page, and the shapes inside them (see the module documentation). → how many shapes were written to. The
/// document is not touched when that is none.
pub fn format_quick_style(doc: &mut Document, page: u32, ids: &[u32], style: QuickStyle) -> Result<usize, Problem> {
    require_theme(doc, page)?;
    let targets = targets_reached(doc, page, ids)?;
    if targets.len() > MAX_QUICK_SHAPES {
        return Err(Problem::failed(format!("a Quick Style is put on at most {MAX_QUICK_SHAPES} shapes at a time; {} were named", targets.len())));
    }
    let shapes: Vec<ShapeRef> = targets.iter().map(|target| target.at.clone()).collect();
    // A shape with no style of its own for a family follows the page's `Theme` style for it. **One pass over the places `targets` found**, one planner and one resolver for
    // all of them and no walk of the page to find a shape by its `ID` (a walk for each shape was 4,096 walks of the page), as a colour of the theme does (`formatting::shape_cells`).
    let linked: Vec<Option<diacraft_doc::Links>> = shapes.iter().map(|at| doc.shape(at).map(|s| s.sheet.links)).collect();
    follow_page_theme_refs(doc, page, &shapes)?;
    let mut touched: Vec<bool> = shapes.iter().zip(&linked).map(|(at, was)| doc.shape(at).map(|s| s.sheet.links) != *was).collect();
    let mut edits = Edits::default();
    {
        let mut ctx = eval_ctx();
        let reader = Reader::new(doc, &mut ctx);
        for (target, was) in targets.iter().zip(touched.iter_mut()) {
            edits.begin();
            let before = edits.planned();
            plan(doc, &reader, &SheetRef::Shape(target.at.clone()), style, target.through_group, &mut edits);
            *was |= edits.planned() > before;
        }
    }
    let formatted = touched.iter().filter(|t| **t).count();
    if !edits.is_empty() {
        edits.apply(doc, page)?;
    }
    Ok(formatted)
}

/// The refusal for a page that has no theme to answer a style, or a drawing that has no `Theme` style.
fn require_theme(doc: &Document, page: u32) -> Result<(), Problem> {
    if doc.page(page).is_none() {
        return Err(Problem::bad(format!("there is no page {page}")));
    }
    if !selection_of(doc, page).is_some_and(|s| s.is_themed()) {
        return Err(Problem::failed("the page has no theme to take a style from: apply a theme first (Design, Themes)"));
    }
    if theme_style_id(doc).is_none() {
        return Err(Problem::failed("the drawing has no Theme style to take a style from: apply a theme first (Design, Themes)"));
    }
    Ok(())
}

fn direct(name: &str) -> CellAddr {
    CellAddr::direct(name)
}

/// Whether the cell at `addr` of the shape follows the theme: it holds the theme's marker (`Themed`), **or a call of `THEMEVAL` and no saved value**, which the picture draws
/// as the same call with the marker beside it: a cell the theme answers, which is not written over.
fn reads_theme(reader: &Reader<'_, '_>, sheet: &SheetRef, addr: &CellAddr) -> bool {
    reader.reads_theme(sheet, addr)
}

/// Whether the cell at `addr` reads a whole number, and this one: its saved value, or for a cell that holds a formula and no saved value what the formula works out to.
fn reads_number(reader: &Reader<'_, '_>, sheet: &SheetRef, addr: &CellAddr, number: u32) -> bool {
    reader.held(sheet, addr).and_then(|value| value.number()).is_some_and(|n| n == f64::from(number))
}

/// The scheme cells that send a shape to its page (65534), for the ones the shape does not read as that already: a shape whose style is not `Theme` reads the root's 0, which is
/// no theme. A Quick Style and a theme pick both ask for it, so that what they write is answered by the page's theme.
pub(super) fn plan_scheme(doc: &Document, reader: &Reader<'_, '_>, sheet: &SheetRef, edits: &mut Edits) {
    for name in SCHEME_CELLS {
        let addr = direct(name);
        if !reads_number(reader, sheet, &addr, PAGE_NUMBER) {
            edits.put(doc, sheet, addr, &New::whole(PAGE_NUMBER));
        }
    }
}

/// What a Quick Style writes for one shape. **A shape reached through a group** (`through_group`) that draws no line or no fill keeps its pattern, and one that is filled with its own line
/// colour keeps its fill to the line: the style is the figure's, and the box of a name does not become a rectangle of it.
fn plan(doc: &Document, reader: &Reader<'_, '_>, sheet: &SheetRef, style: QuickStyle, through_group: bool, edits: &mut Edits) {
    for name in ["QuickStyleLineMatrix", "QuickStyleFillMatrix", "QuickStyleEffectsMatrix"] {
        edits.put(doc, sheet, direct(name), &New::whole(style.matrix));
    }
    for name in ["QuickStyleLineColor", "QuickStyleFillColor", "QuickStyleShadowColor"] {
        edits.put(doc, sheet, direct(name), &New::whole(style.colour));
    }
    plan_scheme(doc, reader, sheet, edits);
    let no_line = keeps_none(reader, sheet, through_group, "LinePattern");
    let no_fill = keeps_none(reader, sheet, through_group, "FillPattern");
    let fill_follows_line = fill_is_line(reader, sheet, through_group);
    for name in LINE_CELLS.iter().chain(FILL_CELLS.iter()).chain(SHADOW_CELLS.iter()) {
        let kept = (no_line && *name == "LinePattern") || (no_fill && *name == "FillPattern") || (fill_follows_line && FILL_CELLS.contains(name));
        let addr = direct(name);
        if !kept && !reads_theme(reader, sheet, &addr) {
            edits.put(doc, sheet, addr, &New::Themed);
        }
    }
    if fill_follows_line {
        return;
    }
    // The stops of the fill's gradient: a shape that follows the style has the ten rows already.
    for ix in 0..MAX_STOP_ROWS as u32 {
        if ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"]
            .iter()
            .all(|n| reads_theme(reader, sheet, &row_addr("FillGradient", ix, n)))
        {
            continue;
        }
        let mut cells: BTreeMap<CellName, Cell> = BTreeMap::new();
        for name in ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"] {
            cells.insert(CellName::new(name), New::Themed.written_over(None));
        }
        edits.set_revived_row(sheet, "FillGradient", ix, cells);
    }
}

// ---- the gallery -------------------------------------------------------------------------------------------------------------------

/// A tile of the gallery: the style and what a shape looks like in it.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickTile {
    pub style: QuickStyle,
    /// The fill colour, or `None` for a style that paints no fill.
    pub fill: Option<Rgb>,
    /// The line colour, or `None` for a style that draws no line.
    pub line: Option<Rgb>,
    /// The line's weight in points.
    pub weight: f64,
    /// Transparency, gradient and shadow of the style.
    pub fx: FxLook,
}

/// What the gallery shows of the shapes selected.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickLook {
    /// The style the first shape has, as its cells hold it.
    pub current: QuickStyle,
    /// The four Variant Styles.
    pub variants: Vec<QuickTile>,
    /// The Theme Styles, row by row: six rows of seven.
    pub styles: Vec<QuickTile>,
}

/// The gallery for the shape `ids` name (the first leaf): each tile is that shape **with the style put on it in a copy of the document**, so that a tile shows
/// what the theme gives it and a program that reads the tiles sees what `shape.quick_style` would make. The document is not changed.
pub fn quick_tiles(doc: &Document, page: u32, ids: &[u32]) -> Result<QuickLook, Problem> {
    quick_tiles_within(doc, page, ids, &UnsavedBudget::new(diacraft_render::UNSAVED_STEPS))
}

/// [`quick_tiles`] with `budget` for the steps of the cells that hold a formula and no saved value, **one for all the readers the gallery makes** (a reader for the shape and
/// two for each tile): the first exhaustion ends it for the rest, and a tile that is read after it reads those cells as they were before. The steps of
/// the themed cells are one budget for the gallery too: one context, lent to each reader in turn and cleared of what it worked out between them.
pub(super) fn quick_tiles_within(doc: &Document, page: u32, ids: &[u32], budget: &UnsavedBudget) -> Result<QuickLook, Problem> {
    require_theme(doc, page)?;
    let id = ids.first().copied().ok_or_else(|| Problem::bad("name a shape to show the styles of"))?;
    let at = first_leaf(doc, page, id)?;
    // The first shape of a group is reached through it, as `shape.quick_style` reaches it.
    let through_group = doc.shape(&at).is_some_and(|shape| shape.id != id);
    let sheet = SheetRef::Shape(at.clone());
    let ctx = std::cell::RefCell::new(eval_ctx());
    let current = {
        let mut ctx = ctx.borrow_mut();
        ctx.invalidate();
        let reader = Reader::lending(doc, &mut ctx, budget);
        let number = |name: &str| reader.num(&sheet, &direct(name), 100.0).round().max(0.0) as u32;
        QuickStyle { matrix: number("QuickStyleFillMatrix"), colour: number("QuickStyleFillColor") }
    };
    let tile = |style: QuickStyle| -> Result<QuickTile, Problem> {
        let mut copy = doc.clone();
        follow_page_theme_refs(&mut copy, page, std::slice::from_ref(&at))?;
        let mut edits = Edits::default();
        {
            let mut ctx = ctx.borrow_mut();
            ctx.invalidate();
            let reader = Reader::lending(&copy, &mut ctx, budget);
            plan(&copy, &reader, &sheet, style, through_group, &mut edits);
        }
        edits.apply_to_preview(&mut copy, page)?;
        let mut ctx = ctx.borrow_mut();
        ctx.invalidate();
        let reader = Reader::lending(&copy, &mut ctx, budget);
        let fx = fx_look(&reader, &sheet, page);
        let number = |name: &str, default: f64| reader.num(&sheet, &direct(name), default);
        Ok(QuickTile {
            style,
            fill: (number("FillPattern", 0.0) != 0.0).then(|| reader.colour(&sheet, &direct("FillForegnd")).unwrap_or(Rgb(255, 255, 255))),
            line: (number("LinePattern", 0.0) != 0.0).then(|| reader.colour(&sheet, &direct("LineColor")).unwrap_or(Rgb(0, 0, 0))),
            weight: number("LineWeight", 0.01) * 72.0,
            fx,
        })
    };
    let variants = (0..4).filter_map(QuickStyle::variant).map(tile).collect::<Result<Vec<_>, _>>()?;
    let mut styles = Vec::with_capacity(42);
    for row in 1..=6 {
        for column in 1..=7 {
            if let Some(style) = QuickStyle::at(row, column) {
                styles.push(tile(style)?);
            }
        }
    }
    Ok(QuickLook { current, variants, styles })
}
