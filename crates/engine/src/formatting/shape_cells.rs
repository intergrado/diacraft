//! `shape.format`: the Fill and Line cells of shapes.
//!
//! **What each parameter writes** (a cell of the shape itself, never of a style, a master or the theme):
//!
//! | Parameter | Cells |
//! |---|---|
//! | `fill` a colour | `FillForegnd` (`RGB(r,g,b)` and its hex); `FillPattern` 1 unless it is 1 to 40 already, `FillGradientEnabled` 0 unless it is 0 |
//! | `fill` a theme colour | `FillForegnd` as `THEMEGUARD(THEMEVAL("AccentColor2"))` with the colour it works out to, and (`theme/palette.rs`) |
//! | `fill` default | `FillForegnd`, `FillPattern` and `FillGradientEnabled` as the call `THEMEVAL()`, which hands them to the theme (ours) |
//! | `fill` none | `FillPattern` 0 ("paints nothing, whatever else is set"); the colours are not touched |
//! | `line` a colour, a theme colour, default | `LineColor`: `LinePattern` 1 unless it is 1 to 23 already, `LineGradientEnabled` 0 unless it is 0; a theme colour is a formula as for the fill; default writes `LineColor` and `LinePattern` as `THEMEVAL()` |
//! | `line` none | `LinePattern` 0 |
//! | `weight` | `LineWeight`, the points over 72 in inches on paper |
//! | `pattern` | `LinePattern` (0 to 23) |
//! | `begin_arrow`, `end_arrow` | `BeginArrow`, `EndArrow` (0 to 45; 254 is a master named by `USE`) |
//! | `begin_arrow_size`, `end_arrow_size` | `BeginArrowSize`, `EndArrowSize` (0 to 6: very small, small, medium, large, extra large, jumbo, colossal) |
//! | `rounding` | `Rounding`, in drawing units |
//! | `fill_transparency` | `FillForegndTrans`, a share on half-percent steps; beside a hatch or stipple also `FillBkgndTrans`; beside a gradient the `GradientStopColorTrans` of every stop |
//! | `line_transparency` | `LineColorTrans`, a share on half-percent steps; beside a line gradient the `GradientStopColorTrans` of every stop of `LineGradient` |
//! | `fill_pattern` | `FillPattern` (1 solid, 2 to 24 hatches and stipples): a gradient the shape had is turned off, `FillGradientEnabled` 0 (the pattern would not paint under it) |
//! | `fill_background` | `FillBkgnd`: what lies between the marks of a pattern |
//! | `line_cap` | `LineCap`: 0 round, 1 flat, 2 square (what files draw, whatever MS-VSDX names them) |
//! | `compound` | `CompoundType` (0 to 4) |
//!
//! Our rule (no specification says what picking a colour does to the pattern): a colour that cannot be seen would be a
//! command that does nothing, so a colour on a shape whose pattern is none, or a theme marker, or a custom pattern, makes the pattern solid, and a
//! colour on a shape that paints a gradient turns the gradient off (the gradient is painted in place of the colour). A hatch
//! pattern (2 to 40 on a fill) stays: the colour is the hatch's foreground.
//!
//! **A theme colour or default on a shape of a themed page** makes sure the shape reads the page's theme: it follows the page's `Theme` style for the families that have no style
//! of their own (as a shape put on the page does), and the scheme cells that send it to its page are written 65534 where it does not read them so (`quick.rs`'s `plan_scheme`), so
//! that what the formula answers is the page's theme and not the table of a drawing with none. A page with no theme needs none of this: the formula answers the table, and a theme
//! applied later links the shape and works the formula out again.

use diacraft_doc::{CellAddr, Document, Reading, Resolver, RowKey, SectionKey, SheetRef};

use super::quick::plan_scheme;
use super::{Edits, New, Rgb, has_value, resolved_number, targets};
use crate::draw::Problem;
use crate::theme::{PICK_PREFIX, ThemePick, follow_page_theme_refs, pick_colour, selection_of};

/// A paint: a colour, a colour of the page's theme, the theme's own colouring of the shape, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    None,
    Colour(Rgb),
    /// A colour picked from the page's theme, with a tint: a formula that follows the theme.
    Theme(ThemePick),
    /// Use Default: the cells the theme answers.
    Default,
}

impl Paint {
    /// Whether it needs the shape to read the page's theme.
    pub(super) fn is_themed(self) -> bool {
        matches!(self, Paint::Theme(_) | Paint::Default)
    }
}

/// The paint a command's parameter `key` names: `#RRGGBB`, `theme:accent2` or `theme:accent2:40` (a theme colour and its tint), `default`, and `none` where `allow_none`. A refusal is in
/// words, and says what the parameter takes.
pub fn parse_paint(text: &str, key: &str, allow_none: bool) -> Result<Paint, Problem> {
    let word = text.trim();
    if allow_none && word.eq_ignore_ascii_case("none") {
        return Ok(Paint::None);
    }
    if word.eq_ignore_ascii_case("default") {
        return Ok(Paint::Default);
    }
    let pick = word.get(..PICK_PREFIX.len()).filter(|p| p.eq_ignore_ascii_case(PICK_PREFIX)).and_then(|_| word.get(PICK_PREFIX.len()..));
    if let Some(pick) = pick {
        return ThemePick::parse(pick).map(Paint::Theme).map_err(|words| Problem::bad(format!("`{key}` {words}")));
    }
    Rgb::parse(word).map(Paint::Colour).ok_or_else(|| {
        let none = if allow_none { ", none" } else { "" };
        Problem::bad(format!(
            "`{key}` must be a colour like #3C78D8{none}, default or theme:accent2 (a colour of the theme, and theme:accent2:40 for a lighter shade of it); not `{}`",
            crate::error::shown(text)
        ))
    })
}

/// What `shape.format` sets; a field left `None` is left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeFormat {
    pub fill: Option<Paint>,
    pub line: Option<Paint>,
    /// Points on paper.
    pub weight: Option<f64>,
    /// 0 to 23.
    pub pattern: Option<u8>,
    /// 0 to 45.
    pub begin_arrow: Option<u8>,
    pub end_arrow: Option<u8>,
    /// 0 to 6: the size class of each line end.
    pub begin_arrow_size: Option<u8>,
    pub end_arrow_size: Option<u8>,
    /// Drawing units.
    pub rounding: Option<f64>,
    /// Percent, 0 (opaque) to 100 (clear).
    pub fill_transparency: Option<f64>,
    pub line_transparency: Option<f64>,
    /// 1 to 24.
    pub fill_pattern: Option<u8>,
    pub fill_background: Option<Rgb>,
    /// 0 round, 1 flat, 2 square.
    pub line_cap: Option<u8>,
    /// 0 to 4.
    pub compound: Option<u8>,
}

/// Points to a paper inch.
const POINTS_PER_INCH: f64 = 72.0;

/// Sets what `format` names on the shapes `ids` of a page, and the shapes inside them. → how many shapes were written to; the
/// document is not touched when that is none.
pub fn format_shapes(doc: &mut Document, page: u32, ids: &[u32], format: &ShapeFormat) -> Result<usize, Problem> {
    let shapes = targets(doc, page, ids)?;
    // A theme colour or default is answered by the page's theme: the shapes follow it (the module documentation).
    let themed = format.fill.is_some_and(Paint::is_themed) || format.line.is_some_and(Paint::is_themed);
    let page_is_themed = themed && selection_of(doc, page).is_some_and(|s| s.is_themed());
    if page_is_themed {
        // One pass over the places already resolved, charged and capped (the finding of the security scan: a pass for each shape walked the page once for each).
        follow_page_theme_refs(doc, page, &shapes)?;
    }
    let picked = Picked { fill: picked_value(doc, page, format.fill), line: picked_value(doc, page, format.line), scheme: page_is_themed };
    let mut edits = Edits::default();
    let mut formatted = 0_usize;
    {
        let res = Resolver::new(doc);
        for at in &shapes {
            edits.begin();
            let before = edits.len();
            plan(doc, &res, &SheetRef::Shape(at.clone()), format, &picked, &mut edits);
            formatted = formatted.saturating_add(usize::from(edits.len() > before));
        }
    }
    if edits.is_empty() {
        return Ok(0);
    }
    edits.apply(doc, page)?;
    Ok(formatted)
}

fn direct(name: &str) -> CellAddr {
    CellAddr::direct(name)
}

/// What the theme colours of a command come to, worked out once: the value their cells hold until the recalculation that follows the write.
struct Picked {
    fill: Option<String>,
    line: Option<String>,
    /// Whether the page selects a theme, so that a shape is made to read it.
    scheme: bool,
}

/// The colour `paint` comes to on `page` when it is a theme colour, as the text a colour cell holds (`#rrggbb`); black where the theme cannot answer it.
fn picked_value(doc: &Document, page: u32, paint: Option<Paint>) -> Option<String> {
    match paint? {
        Paint::Theme(pick) => Some(pick_colour(doc, page, pick).unwrap_or(Rgb(0, 0, 0)).hex()),
        Paint::None | Paint::Colour(_) | Paint::Default => None,
    }
}

/// What a colour cell is given for a colour or a theme colour; `None` for the paints that write no colour of their own.
pub(super) fn colour_new(paint: Paint, value: Option<&String>) -> Option<New> {
    match paint {
        Paint::Colour(colour) => Some(New::Colour(colour)),
        Paint::Theme(pick) => Some(New::Theme { formula: pick.formula(), value: value.cloned().unwrap_or_else(|| "#000000".to_owned()) }),
        Paint::Default => Some(New::Themed),
        Paint::None => None,
    }
}

/// The cells a fill and a line are written in: their names, and the highest pattern number that paints (a colour cannot be seen under a pattern that does not).
struct Side {
    colour: &'static str,
    pattern: &'static str,
    gradient: &'static str,
    most_pattern: f64,
    /// What Use Default hands to the theme.
    defaults: &'static [&'static str],
}

const FILL: Side = Side {
    colour: "FillForegnd",
    pattern: "FillPattern",
    gradient: "FillGradientEnabled",
    most_pattern: 40.0,
    defaults: &["FillForegnd", "FillPattern", "FillGradientEnabled"],
};
const LINE: Side = Side {
    colour: "LineColor",
    pattern: "LinePattern",
    gradient: "LineGradientEnabled",
    most_pattern: 23.0,
    defaults: &["LineColor", "LinePattern"],
};

/// What `paint` writes for the fill or the line of one shape (the module documentation's table): none turns the pattern off, default hands the cells to the theme, and a colour or a theme colour is
/// written with the pattern that shows it.
fn plan_side(doc: &Document, res: &Resolver<'_>, sheet: &SheetRef, side: &Side, paint: Paint, value: Option<&String>, edits: &mut Edits) {
    let mut put = |name: &str, new: New| {
        edits.put(doc, sheet, direct(name), &new);
    };
    match paint {
        Paint::None => put(side.pattern, New::whole(0)),
        Paint::Default => {
            for name in side.defaults {
                put(name, New::Themed);
            }
        }
        Paint::Colour(_) | Paint::Theme(_) => {
            if let Some(new) = colour_new(paint, value) {
                put(side.colour, new);
            }
            if !resolved_number(res, sheet, &direct(side.pattern)).is_some_and(|n| (1.0..=side.most_pattern).contains(&n)) {
                put(side.pattern, New::whole(1));
            }
            if gradient_on(res, sheet, side.gradient) {
                put(side.gradient, New::whole(0));
            }
        }
    }
}

/// Plans the writes for one shape.
fn plan(doc: &Document, res: &Resolver<'_>, sheet: &SheetRef, f: &ShapeFormat, picked: &Picked, edits: &mut Edits) {
    // The shape reads the page's theme for what a theme colour or default asks of it.
    if picked.scheme && (f.fill.is_some_and(Paint::is_themed) || f.line.is_some_and(Paint::is_themed)) {
        plan_scheme(doc, res, sheet, edits);
    }
    if let Some(paint) = f.fill {
        plan_side(doc, res, sheet, &FILL, paint, picked.fill.as_ref(), edits);
    }
    if let Some(paint) = f.line {
        plan_side(doc, res, sheet, &LINE, paint, picked.line.as_ref(), edits);
    }
    let mut put_at = |addr: CellAddr, new: New| {
        edits.put(doc, sheet, addr, &new);
    };
    let mut put = |name: &str, new: New| put_at(direct(name), new);
    if let Some(points) = f.weight {
        put("LineWeight", New::number(points / POINTS_PER_INCH));
    }
    // An explicit pattern after a colour wins: the later plan for a cell replaces the earlier in the batch.
    if let Some(pattern) = f.pattern {
        put("LinePattern", New::whole(u32::from(pattern)));
    }
    if let Some(arrow) = f.begin_arrow {
        put("BeginArrow", New::whole(u32::from(arrow)));
    }
    if let Some(arrow) = f.end_arrow {
        put("EndArrow", New::whole(u32::from(arrow)));
    }
    if let Some(size) = f.begin_arrow_size {
        put("BeginArrowSize", New::whole(u32::from(size)));
    }
    if let Some(size) = f.end_arrow_size {
        put("EndArrowSize", New::whole(u32::from(size)));
    }
    if let Some(rounding) = f.rounding {
        put("Rounding", New::number(rounding));
    }
    if let Some(pattern) = f.fill_pattern {
        put_at(direct("FillPattern"), New::whole(u32::from(pattern)));
        if gradient_on(res, sheet, "FillGradientEnabled") {
            put_at(direct("FillGradientEnabled"), New::whole(0));
        }
    }
    if let Some(colour) = f.fill_background {
        put_at(direct("FillBkgnd"), New::Colour(colour));
    }
    if let Some(percent) = f.fill_transparency {
        put_at(direct("FillForegndTrans"), New::number(share(percent)));
        let pattern = f.fill_pattern.map(f64::from).or_else(|| resolved_number(res, sheet, &direct("FillPattern"))).map_or(1, |n| n.round() as i64);
        if (2..=24).contains(&pattern) {
            put_at(direct("FillBkgndTrans"), New::number(share(percent)));
        }
        // A gradient is painted in place of the colours, and its transparency is the stops': the same on every stop is one over the whole shape.
        if gradient_on(res, sheet, "FillGradientEnabled") && f.fill_pattern.is_none() {
            for ix in stop_rows(res, sheet, "FillGradient") {
                put_at(row_addr("FillGradient", ix, "GradientStopColorTrans"), New::number(share(percent)));
            }
        }
    }
    if let Some(percent) = f.line_transparency {
        put_at(direct("LineColorTrans"), New::number(share(percent)));
        if gradient_on(res, sheet, "LineGradientEnabled") {
            for ix in stop_rows(res, sheet, "LineGradient") {
                put_at(row_addr("LineGradient", ix, "GradientStopColorTrans"), New::number(share(percent)));
            }
        }
    }
    if let Some(cap) = f.line_cap {
        put_at(direct("LineCap"), New::whole(u32::from(cap)));
    }
    if let Some(compound) = f.compound {
        put_at(direct("CompoundType"), New::whole(u32::from(compound)));
    }
}

/// The share (0 to 1) a percentage is, on the half-percent steps files hold ("a writer rounds these to the nearest 0.005").
pub(crate) fn share(percent: f64) -> f64 {
    if percent.is_finite() { ((percent * 2.0).round() / 200.0).clamp(0.0, 1.0) } else { 0.0 }
}

/// The `IX` of the stop rows of a gradient section the shape reads now (its own and its style's, merged: a row deleted here is not one).
pub(super) fn stop_rows(res: &Resolver<'_>, sheet: &SheetRef, section: &str) -> Vec<u32> {
    let Some(rows) = effective_stop_rows(res, sheet, section) else { return Vec::new() };
    rows.iter()
        .filter_map(|row| match row.key {
            RowKey::Ix(ix) => Some(ix),
            RowKey::Name(_) => None,
        })
        .take(MAX_STOP_ROWS)
        .collect()
}

/// The rows of a gradient section a shape reads, merged along its master and styles, **for a section of at most [`MAX_ROWS_COUNTED`] rows in all**: a hostile file may hold a hundred
/// thousand rows of stops (counts ten), and a section past the bound is read as one with no stops, counted before any row is merged.
pub(super) fn effective_stop_rows<'a>(res: &'a Resolver<'_>, sheet: &SheetRef, section: &str) -> Option<Vec<diacraft_doc::EffectiveRow<'a>>> {
    res.effective_rows_within(sheet, &SectionKey::new(section, None), MAX_ROWS_COUNTED)?.ok()
}

/// The most rows a gradient section holds (the shape's, its master's and its styles' together) for the controls to read it.
pub(super) const MAX_ROWS_COUNTED: usize = 256;

/// The most stop rows of a section read for one shape: counts ten, and a hostile file may hold thousands.
pub(super) const MAX_STOP_ROWS: usize = 10;

/// The cell `name` of a row `ix` of a section.
pub(super) fn row_addr(section: &str, ix: u32, name: &str) -> CellAddr {
    CellAddr::in_row(section, None, RowKey::Ix(ix), name)
}

/// Whether a gradient flag reads as set: any value but 0 and an empty one (the theme marker included, which the theme answers).
fn gradient_on(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> bool {
    if !has_value(res, sheet, &direct(name)) {
        return false;
    }
    let reading = res.resolve(sheet, &direct(name)).and_then(|found| found.value.cell?.value.as_ref().map(|v| v.reading()));
    match reading {
        None | Some(Reading::Empty) => false,
        Some(Reading::Number(n)) => n != 0.0,
        Some(_) => true,
    }
}
