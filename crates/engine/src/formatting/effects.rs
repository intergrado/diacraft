//! Gradients and shadows: `shape.gradient` and `shape.shadow` write the cells, and [`fx_look`] reads them back as the controls show them.
//!
//! **A gradient** is a flag (`FillGradientEnabled`, `LineGradientEnabled`), a layout
//! (`FillGradientDir`, 0 linear, 1 to 7 radial, 8 to 12 rectangular, 13 along the path), an angle (`FillGradientAngle`, radians, only for the linear
//! layout), `RotateGradientWithShape` for a fill, and up to ten stops, the rows of `FillGradient` or `LineGradient` keyed by `IX` from 0, each with
//! `GradientStopColor`, `GradientStopColorTrans` and `GradientStopPosition`. The stops are written **in position order, from `IX` 0** (a position
//! lower than the one before is not allowed), and a row of the shape's style or master that the new list does not reach is **deleted here** (`Del`:
//! "a two-stop gradient keeps two rows and marks the other eight `Del`"), so a shape that follows the `Theme` style, which holds ten rows, keeps no stop it
//! was not given. A gradient turned **on** makes sure the shape paints a fill at all: a `FillPattern` that is not 1 to 40 becomes 1 (ours:
//! pattern 0 paints nothing whatever else is set, so a gradient on it would be a command that does nothing), and for a line `LinePattern` 1 to 23. **Not for a member reached
//! through a group that draws none**, and a fill gradient leaves a member filled with its own line colour (`formatting/mod.rs`).
//! A gradient turned **on with no stops** keeps the ones the shape has, or, when it has none, is made of two: the colour the shape has now, and a lighter
//! one (ours: no specification says what Gradient fill starts as).
//!
//! **A shadow** is the cells `ShdwPattern` (1 on, 0 off), `ShapeShdwType` (1, the outline
//! moved: the pane's shadow is an outer one), `ShdwForegnd` and `ShdwForegndTrans`, `ShapeShdwScaleFactor` (size), `ShapeShdwBlur`, and the offset in `ShapeShdwOffsetX`
//! and `ShapeShdwOffsetY`, paper inches along the page's axes, y up. **The pane's distance and angle** are that offset in polar form: the distance in
//! points, and the angle in degrees **clockwise from the right as seen on the page** (ours: leaves the sense open,
//! and a theme's own `ShadowDirection` is DrawingML's bearing, clockwise from the right, which `ShapeShdwOffsetX` agrees with as the distance times its cosine and
//! `ShapeShdwOffsetY` as the negated distance times its sine; so 90 is straight down, and what a theme gives and what the pane shows agree).
//! Any shadow setting turns the shadow on unless `on` is false (ours). A shadow that is off is turned on with every cell written (the preset's
//! values where a setting is not given); one that is on has only the settings given written, so a shadow that follows its theme keeps following it for the rest.

use std::collections::BTreeMap;

use diacraft_doc::{Cell, CellAddr, CellName, Document, RowKey, SheetRef};

use super::look::{Reader, eval_ctx, pattern_shown};
use super::shape_cells::{MAX_STOP_ROWS, effective_stop_rows, row_addr, share, stop_rows};
use super::{Edits, New, Rgb, fill_is_line, holds_exactly, holds_row, keeps_none, targets, targets_reached};
use crate::draw::Problem;

/// The most stops a gradient has.
pub const MAX_GRADIENT_STOPS: usize = MAX_STOP_ROWS;

/// Which of a shape's two gradients a command works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientTarget {
    /// The inside: `FillGradientEnabled`, `FillGradientDir`, `FillGradientAngle`, `FillGradient`.
    Fill,
    /// The outline: `LineGradientEnabled`, `LineGradientDir`, `LineGradientAngle`, `LineGradient`.
    Line,
}

impl GradientTarget {
    fn section(self) -> &'static str {
        match self {
            GradientTarget::Fill => "FillGradient",
            GradientTarget::Line => "LineGradient",
        }
    }

    fn cell(self, what: &str) -> String {
        match self {
            GradientTarget::Fill => format!("Fill{what}"),
            GradientTarget::Line => format!("Line{what}"),
        }
    }

    /// The word a command takes and gives.
    pub fn word(self) -> &'static str {
        match self {
            GradientTarget::Fill => "fill",
            GradientTarget::Line => "line",
        }
    }

    pub fn from_word(word: &str) -> Option<GradientTarget> {
        [GradientTarget::Fill, GradientTarget::Line].into_iter().find(|t| t.word().eq_ignore_ascii_case(word))
    }
}

/// The layout of a gradient, the three families of `FillGradientDir`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientKind {
    Linear,
    Radial,
    Rectangular,
    Path,
}

impl GradientKind {
    pub fn word(self) -> &'static str {
        match self {
            GradientKind::Linear => "linear",
            GradientKind::Radial => "radial",
            GradientKind::Rectangular => "rectangular",
            GradientKind::Path => "path",
        }
    }

    pub fn from_word(word: &str) -> Option<GradientKind> {
        [GradientKind::Linear, GradientKind::Radial, GradientKind::Rectangular, GradientKind::Path]
            .into_iter()
            .find(|k| k.word().eq_ignore_ascii_case(word))
    }

    /// The kind a `FillGradientDir` number is (13 and any number past it read as the path).
    pub fn of_dir(dir: u8) -> GradientKind {
        match dir {
            0 => GradientKind::Linear,
            1..=7 => GradientKind::Radial,
            8..=12 => GradientKind::Rectangular,
            _ => GradientKind::Path,
        }
    }

    /// The `FillGradientDir` of this kind that starts at `origin` (`centre`, a corner `top_left`, `top_right`, `bottom_left`, `bottom_right`, or for a radial one
    /// also the middle of an edge `top` or `bottom`), by the table. `None` for an origin the kind has none of.
    pub fn dir_at(self, origin: &str) -> Option<u8> {
        let origin = origin.to_ascii_lowercase();
        let origin = origin.as_str();
        match self {
            GradientKind::Linear | GradientKind::Path => Some(if self == GradientKind::Linear { 0 } else { 13 }),
            GradientKind::Radial => match origin {
                "bottom_right" => Some(1),
                "bottom_left" => Some(2),
                "center" | "centre" => Some(3),
                "bottom" => Some(4),
                "top" => Some(5),
                "top_right" => Some(6),
                "top_left" => Some(7),
                _ => None,
            },
            GradientKind::Rectangular => match origin {
                "bottom_right" => Some(8),
                "bottom_left" => Some(9),
                "center" | "centre" => Some(10),
                "top_right" => Some(11),
                "top_left" => Some(12),
                _ => None,
            },
        }
    }

    /// The origin of a `FillGradientDir`, as [`GradientKind::dir_at`] takes it (`""` for the linear and the path layouts).
    pub fn origin_of(dir: u8) -> &'static str {
        match dir {
            1 | 8 => "bottom_right",
            2 | 9 => "bottom_left",
            3 | 10 => "center",
            4 => "bottom",
            5 => "top",
            6 | 11 => "top_right",
            7 | 12 => "top_left",
            _ => "",
        }
    }
}

/// A stop to write: where (0 to 1), the colour and how see-through (percent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopFormat {
    pub position: f64,
    pub colour: Rgb,
    pub transparency: f64,
}

/// What `shape.gradient` sets; a field left `None` is left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientFormat {
    /// `Some(false)` turns the gradient off (its rows stay); anything else given turns it on.
    pub on: Option<bool>,
    /// 0 to 13.
    pub dir: Option<u8>,
    /// Degrees, clockwise from the right as seen on the page.
    pub angle: Option<f64>,
    /// A fill's `RotateGradientWithShape`.
    pub rotate_with_shape: Option<bool>,
    /// The stops, two to ten.
    pub stops: Option<Vec<StopFormat>>,
}

impl GradientFormat {
    pub fn is_empty(&self) -> bool {
        *self == GradientFormat::default()
    }
}

/// The angle a cell holds for a number of degrees: radians.
fn radians(degrees: f64) -> f64 {
    // Whole turns are kept off the cell: -90 and 270 are one angle, and the cell holds the first the person typed within a turn either way.
    radians_within_turn(degrees % 360.0)
}

/// [`radians`] of an angle within a turn either way (what the remainder by 360 leaves of it), to the ninth place, and 0 (never -0) for one that rounds to none.
pub(crate) fn radians_within_turn(turned: f64) -> f64 {
    let r = (turned.to_radians() * 1.0e9).round() / 1.0e9;
    // An angle a hair below 0 rounds to -0, which a cell would hold as `-0`.
    if r == 0.0 { 0.0 } else { r }
}

fn rounded(n: f64, places: i32) -> f64 {
    let k = 10_f64.powi(places);
    let r = (n * k).round() / k;
    if r == 0.0 || !r.is_finite() { 0.0 } else { r }
}

/// The colour two stops start from when a shape has none: its own colour and a lighter one.
fn default_stops(colour: Rgb) -> Vec<StopFormat> {
    let luma = (0.299 * f64::from(colour.0) + 0.587 * f64::from(colour.1) + 0.114 * f64::from(colour.2)) / 255.0;
    let other = if luma > 0.85 { Rgb(166, 166, 166) } else { Rgb(255, 255, 255) };
    vec![StopFormat { position: 0.0, colour, transparency: 0.0 }, StopFormat { position: 1.0, colour: other, transparency: 0.0 }]
}

/// Sets the gradient cells of the shapes `ids` of a page, and the shapes inside them. → how many shapes were written to; the document is not touched when
/// that is none.
pub fn format_gradient(doc: &mut Document, page: u32, ids: &[u32], target: GradientTarget, f: &GradientFormat) -> Result<usize, Problem> {
    if let Some(stops) = &f.stops
        && !(2..=MAX_GRADIENT_STOPS).contains(&stops.len())
    {
        return Err(Problem::bad(format!("a gradient has from 2 to {MAX_GRADIENT_STOPS} stops, not {}", stops.len())));
    }
    let shapes = targets_reached(doc, page, ids)?;
    let mut edits = Edits::default();
    let mut formatted = 0_usize;
    {
        let mut ctx = eval_ctx();
        let reader = Reader::new(doc, &mut ctx);
        for shape in &shapes {
            edits.begin();
            let before = edits.planned();
            plan_gradient(doc, &reader, &SheetRef::Shape(shape.at.clone()), target, f, shape.through_group, &mut edits);
            formatted = formatted.saturating_add(usize::from(edits.planned() > before));
        }
    }
    if edits.is_empty() {
        return Ok(0);
    }
    edits.apply(doc, page)?;
    Ok(formatted)
}

/// What a gradient writes for one shape. **A shape reached through a group** (`through_group`) that is filled with its own line colour is left alone by a fill gradient, and one that draws no
/// line or no fill is not made to.
fn plan_gradient(
    doc: &Document,
    r: &Reader<'_, '_>,
    sheet: &SheetRef,
    target: GradientTarget,
    f: &GradientFormat,
    through_group: bool,
    edits: &mut Edits,
) {
    let direct = CellAddr::direct;
    if target == GradientTarget::Fill && fill_is_line(r, sheet, through_group) {
        return;
    }
    let enabled = target.cell("GradientEnabled");
    let on = f.on.unwrap_or(true);
    if !on {
        // Off is writing a 0 where the gradient is on: a shape that has none is left as it is.
        if r.num(sheet, &direct(&enabled), 0.0) != 0.0 {
            edits.put(doc, sheet, direct(&enabled), &New::whole(0));
        }
        return;
    }
    edits.put(doc, sheet, direct(&enabled), &New::whole(1));
    // A gradient on a pattern that paints nothing paints nothing, unless the shape is a member of a group that was made to draw none of it.
    let pattern_cell = if target == GradientTarget::Fill { "FillPattern" } else { "LinePattern" };
    if r.num(sheet, &direct(pattern_cell), 0.0) == 0.0 && !keeps_none(r, sheet, through_group, pattern_cell) {
        edits.put(doc, sheet, direct(pattern_cell), &New::whole(1));
    }
    if let Some(dir) = f.dir {
        edits.put(doc, sheet, direct(&target.cell("GradientDir")), &New::whole(u32::from(dir)));
    }
    if let Some(angle) = f.angle {
        edits.put(doc, sheet, direct(&target.cell("GradientAngle")), &New::number(radians(angle)));
    }
    if let Some(rotate) = f.rotate_with_shape.filter(|_| target == GradientTarget::Fill) {
        edits.put(doc, sheet, direct("RotateGradientWithShape"), &New::whole(u32::from(rotate)));
    }
    let section = target.section();
    let have = stop_rows(&r.res, sheet, section);
    let stops: Option<Vec<StopFormat>> = match &f.stops {
        Some(given) => Some(given.clone()),
        // Switched on with no stops: the shape's own, or two made from its colour.
        None if have.is_empty() => {
            let colour_cell = if target == GradientTarget::Fill { "FillForegnd" } else { "LineColor" };
            let colour =
                r.colour(sheet, &direct(colour_cell)).unwrap_or(if target == GradientTarget::Fill { Rgb(255, 255, 255) } else { Rgb(0, 0, 0) });
            Some(default_stops(colour))
        }
        None => None,
    };
    let Some(mut stops) = stops else { return };
    // Position order, equal positions in the order given.
    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
    for (ix, stop) in stops.iter().enumerate() {
        let ix = ix as u32;
        let mut cells: BTreeMap<CellName, Cell> = BTreeMap::new();
        cells.insert(CellName::new("GradientStopColor"), New::Colour(stop.colour).written_over(None));
        cells.insert(CellName::new("GradientStopColorTrans"), New::number(share(stop.transparency)).written_over(None));
        cells.insert(CellName::new("GradientStopPosition"), New::number(rounded(stop.position.clamp(0.0, 1.0), 4)).written_over(None));
        // A row the shape holds exactly so is left as it is: a call that changes nothing is no undo step.
        if !holds_exactly(doc, sheet, section, ix, &cells) {
            edits.set_revived_row(sheet, section, ix, cells);
        }
    }
    // The rows the new list does not reach: deleted here when the style or the master holds them, and taken out when only the shape does.
    for ix in stops.len() as u32..MAX_GRADIENT_STOPS as u32 {
        if have.contains(&ix) || holds_row(doc, sheet, section, ix) {
            edits.set_deleted_row(sheet, section, ix);
        }
    }
}

// ---- the shadow -----------------------------------------------------------------------------------------------------------------

/// What `shape.shadow` sets; a field left `None` is left alone (or, for a shadow that is being turned on, is the preset's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadowFormat {
    /// `Some(false)` takes the shadow away; anything else given turns it on.
    pub on: Option<bool>,
    pub colour: Option<Rgb>,
    /// Percent, 0 opaque to 100 clear.
    pub transparency: Option<f64>,
    /// Percent of the shape's size: 100 is the same size (`ShapeShdwScaleFactor` 1).
    pub size: Option<f64>,
    /// Points.
    pub blur: Option<f64>,
    /// Degrees clockwise from the right as seen on the page.
    pub angle: Option<f64>,
    /// Points.
    pub distance: Option<f64>,
}

impl ShadowFormat {
    pub fn is_empty(&self) -> bool {
        *self == ShadowFormat::default()
    }
}

/// The least a shadow is wide a size says, as a percent, and the most: a shadow of no size is none, and a thousand times a shape is past any page.
pub const SHADOW_SIZE_RANGE: (f64, f64) = (1.0, 1_000.0);
/// The most a blur or a distance is, in points.
pub const SHADOW_MAX_POINTS: f64 = 1_000.0;

/// What a shadow starts as when it is turned on and a setting is not given: black, 60 percent clear, the size of the shape, 4 points blurred, 4 points away
/// at 45 degrees (down and to the right). **Ours**: no specification gives a default shadow (lists the presets as not described).
const START: ShadowFormat = ShadowFormat {
    on: Some(true),
    colour: Some(Rgb(0, 0, 0)),
    transparency: Some(60.0),
    size: Some(100.0),
    blur: Some(4.0),
    angle: Some(45.0),
    distance: Some(4.0),
};

/// The presets of the Shadow gallery, by the words `shape.shadow` takes for `preset`: `none`, and an outer shadow in each direction (ours:
/// the galleries' contents are `named` only). A preset is the starting shadow in a direction, and `center` has no distance and a
/// wider blur.
pub fn shadow_preset(name: &str) -> Option<ShadowFormat> {
    let name = name.to_ascii_lowercase();
    if name == "none" {
        return Some(ShadowFormat { on: Some(false), ..ShadowFormat::default() });
    }
    let (angle, distance, blur) = match name.as_str() {
        "right" => (0.0, 4.0, 4.0),
        "bottom_right" => (45.0, 4.0, 4.0),
        "bottom" => (90.0, 4.0, 4.0),
        "bottom_left" => (135.0, 4.0, 4.0),
        "left" => (180.0, 4.0, 4.0),
        "top_left" => (225.0, 4.0, 4.0),
        "top" => (270.0, 4.0, 4.0),
        "top_right" => (315.0, 4.0, 4.0),
        "center" | "centre" => (0.0, 0.0, 8.0),
        _ => return None,
    };
    Some(ShadowFormat { angle: Some(angle), distance: Some(distance), blur: Some(blur), ..START })
}

/// The words of the presets, in the order of the gallery.
pub const SHADOW_PRESETS: [&str; 10] = ["none", "bottom_right", "bottom", "bottom_left", "right", "center", "left", "top_right", "top", "top_left"];

/// Sets the shadow cells of the shapes `ids` of a page, and the shapes inside them. → how many shapes were written to.
pub fn format_shadow(doc: &mut Document, page: u32, ids: &[u32], f: &ShadowFormat) -> Result<usize, Problem> {
    let shapes = targets(doc, page, ids)?;
    let mut edits = Edits::default();
    let mut formatted = 0_usize;
    {
        let mut ctx = eval_ctx();
        let reader = Reader::new(doc, &mut ctx);
        for at in &shapes {
            edits.begin();
            let before = edits.len();
            plan_shadow(doc, &reader, page, &SheetRef::Shape(at.clone()), f, &mut edits);
            formatted = formatted.saturating_add(usize::from(edits.len() > before));
        }
    }
    if edits.is_empty() {
        return Ok(0);
    }
    edits.apply(doc, page)?;
    Ok(formatted)
}

fn plan_shadow(doc: &Document, r: &Reader<'_, '_>, page: u32, sheet: &SheetRef, f: &ShadowFormat, edits: &mut Edits) {
    let direct = CellAddr::direct;
    let now = shadow_of(r, sheet, page);
    if f.on == Some(false) {
        // No pattern, no shadow. The other cells stay. A shape that shows none is left as it is.
        if now.is_some() {
            edits.put(doc, sheet, direct("ShdwPattern"), &New::whole(0));
        }
        return;
    }
    // A shadow that is off is made whole from the starting one; one that is on has only what is given written, except that a shadow whose type is
    // not 1 (the page's, `ShapeShdwType` 0) is given the shape's own cells with what it shows now, because the shape's own offsets and size are not read until
    // its type is 1.
    let whole = now.is_none();
    let own_type = r.num(sheet, &direct("ShapeShdwType"), 0.0).round() as i64;
    let carry = whole || own_type != 1;
    let start = |of: Option<f64>| of.unwrap_or(0.0);
    if whole {
        edits.put(doc, sheet, direct("ShdwPattern"), &New::whole(1));
    }
    if own_type != 1 {
        edits.put(doc, sheet, direct("ShapeShdwType"), &New::whole(1));
    }
    if let Some(colour) = f.colour.or_else(|| whole.then_some(START.colour).flatten()) {
        edits.put(doc, sheet, direct("ShdwForegnd"), &New::Colour(colour));
    }
    if let Some(t) = f.transparency.or_else(|| whole.then_some(START.transparency).flatten()) {
        edits.put(doc, sheet, direct("ShdwForegndTrans"), &New::number(share(t)));
    }
    if (f.size.is_some() || carry)
        && let Some(s) = f.size.or(now.as_ref().map(|n| n.size)).or(START.size)
    {
        edits.put(doc, sheet, direct("ShapeShdwScaleFactor"), &New::number(rounded(s / 100.0, 6)));
    }
    if let Some(b) = f.blur.or_else(|| whole.then_some(START.blur).flatten()) {
        edits.put(doc, sheet, direct("ShapeShdwBlur"), &New::number(rounded(b / 72.0, 9)));
    }
    if f.angle.is_some() || f.distance.is_some() || carry {
        let d = f.distance.or(now.as_ref().map(|n| n.distance)).or(START.distance);
        let a = f.angle.or(now.as_ref().map(|n| n.angle)).or(START.angle);
        let (x, y) = offset_of(start(d), start(a));
        edits.put(doc, sheet, direct("ShapeShdwOffsetX"), &New::number(x));
        edits.put(doc, sheet, direct("ShapeShdwOffsetY"), &New::number(y));
    }
}

/// The offset a distance (points) and an angle (degrees, clockwise from the right) are, in paper inches, y up.
pub(crate) fn offset_of(distance: f64, angle: f64) -> (f64, f64) {
    let a = angle.to_radians();
    (rounded(distance * a.cos() / 72.0, 9), rounded(-distance * a.sin() / 72.0, 9))
}

/// The distance (points) and the angle (degrees, `0` up to but not including `360`) of an offset in paper inches, y up.
pub(crate) fn polar_of(x: f64, y: f64) -> (f64, f64) {
    let distance = (x * x + y * y).sqrt() * 72.0;
    if distance < 1.0e-9 {
        return (0.0, 0.0);
    }
    let degrees = (-y).atan2(x).to_degrees();
    (rounded(distance, 4), rounded(if degrees < 0.0 { degrees + 360.0 } else { degrees }, 2))
}

// ---- what the controls read ------------------------------------------------------------------------------------------------------

/// A stop as the controls show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopLook {
    /// 0 to 1.
    pub position: f64,
    pub colour: Rgb,
    /// Percent.
    pub transparency: f64,
}

/// A gradient as the controls show it.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientLook {
    /// 0 to 13.
    pub dir: u8,
    /// Degrees, 0 up to but not including 360, clockwise from the right as seen on the page.
    pub angle: f64,
    pub rotate_with_shape: bool,
    /// In position order.
    pub stops: Vec<StopLook>,
}

/// A shadow as the controls show it, the page's defaults already in.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowLook {
    pub colour: Rgb,
    /// Percent.
    pub transparency: f64,
    /// Percent of the shape's size.
    pub size: f64,
    /// Points.
    pub blur: f64,
    /// Degrees, 0 up to but not including 360, clockwise from the right as seen on the page.
    pub angle: f64,
    /// Points.
    pub distance: f64,
    /// `ShapeShdwType` after the page's default is in: 1 outer, 2 oblique, 3 inner.
    pub kind: u8,
}

/// The shadow a shape shows, if it shows one: `ShdwPattern` is not 0. A shape whose own `ShapeShdwType` is 0 takes the type, offsets and scale of the page
/// (a scale of 0 reads 1).
pub(super) fn shadow_of(r: &Reader<'_, '_>, sheet: &SheetRef, page: u32) -> Option<ShadowLook> {
    let direct = CellAddr::direct;
    if r.num(sheet, &direct("ShdwPattern"), 0.0) == 0.0 {
        return None;
    }
    let own = r.num(sheet, &direct("ShapeShdwType"), 0.0).round() as i64;
    let (kind, x, y, scale) = if own == 0 {
        let page_sheet = SheetRef::Page(page);
        let scale = r.num(&page_sheet, &direct("ShdwScaleFactor"), 1.0);
        (
            r.num(&page_sheet, &direct("ShdwType"), 0.0).round() as i64,
            r.num(&page_sheet, &direct("ShdwOffsetX"), 0.0),
            r.num(&page_sheet, &direct("ShdwOffsetY"), 0.0),
            if scale == 0.0 { 1.0 } else { scale },
        )
    } else {
        (
            own,
            r.num(sheet, &direct("ShapeShdwOffsetX"), 0.0),
            r.num(sheet, &direct("ShapeShdwOffsetY"), 0.0),
            r.num(sheet, &direct("ShapeShdwScaleFactor"), 1.0),
        )
    };
    let (distance, angle) = polar_of(x, y);
    Some(ShadowLook {
        colour: r.colour(sheet, &direct("ShdwForegnd")).unwrap_or(Rgb(0, 0, 0)),
        transparency: rounded(r.num(sheet, &direct("ShdwForegndTrans"), 0.0) * 100.0, 2),
        size: rounded(scale * 100.0, 4),
        blur: rounded(r.num(sheet, &direct("ShapeShdwBlur"), 0.0).max(0.0) * 72.0, 4),
        angle,
        distance,
        kind: if (1..=3).contains(&kind) { kind as u8 } else { 1 },
    })
}

/// The stops of a gradient section as the renderer reads them: merged along the chain, at most ten, in position order, a row whose colour reads as nothing left out.
fn stops_of(r: &Reader<'_, '_>, sheet: &SheetRef, section: &str) -> Vec<StopLook> {
    let Some(rows) = effective_stop_rows(&r.res, sheet, section) else { return Vec::new() };
    let mut out = Vec::new();
    for row in rows.iter().take(MAX_STOP_ROWS) {
        let RowKey::Ix(ix) = row.key else { continue };
        let Some(colour) = r.colour(sheet, &row_addr(section, ix, "GradientStopColor")) else { continue };
        out.push(StopLook {
            position: rounded(r.num(sheet, &row_addr(section, ix, "GradientStopPosition"), 0.0).clamp(0.0, 1.0), 4),
            colour,
            transparency: rounded(r.num(sheet, &row_addr(section, ix, "GradientStopColorTrans"), 0.0) * 100.0, 2),
        });
    }
    out.sort_by(|a, b| a.position.total_cmp(&b.position));
    out
}

/// The gradient of a shape's fill or line, if it paints one: the flag is on (and, for a fill, the pattern is not 0).
fn gradient_of(r: &Reader<'_, '_>, sheet: &SheetRef, target: GradientTarget) -> Option<GradientLook> {
    let direct = CellAddr::direct;
    let pattern = r.num(sheet, &direct("FillPattern"), 0.0).round() as i64;
    if r.num(sheet, &direct(&target.cell("GradientEnabled")), 0.0) == 0.0 {
        // A fill the old way: the numbers 25 to 40 are gradients of the two fill colours, which the picture draws and the controls show as such.
        return (target == GradientTarget::Fill && (25..=40).contains(&pattern)).then(|| legacy_gradient(r, sheet, pattern));
    }
    if target == GradientTarget::Fill && pattern == 0 {
        return None;
    }
    let mut stops = stops_of(r, sheet, target.section());
    if stops.is_empty() {
        // As the renderer: no rows are the two colours of the fill in order.
        let (fore, back) = match target {
            GradientTarget::Fill => ("FillForegnd", "FillBkgnd"),
            GradientTarget::Line => ("LineColor", "LineColor"),
        };
        let a = r.colour(sheet, &direct(fore)).unwrap_or(Rgb(255, 255, 255));
        let b = r.colour(sheet, &direct(back)).unwrap_or(Rgb(0, 0, 0));
        stops = vec![StopLook { position: 0.0, colour: a, transparency: 0.0 }, StopLook { position: 1.0, colour: b, transparency: 0.0 }];
    }
    let degrees = r.num(sheet, &direct(&target.cell("GradientAngle")), std::f64::consts::FRAC_PI_2).to_degrees();
    let angle = rounded(degrees.rem_euclid(360.0), 2);
    Some(GradientLook {
        dir: r.num(sheet, &direct(&target.cell("GradientDir")), 0.0).round().clamp(0.0, 13.0) as u8,
        angle,
        rotate_with_shape: r.num(sheet, &direct("RotateGradientWithShape"), 1.0) != 0.0,
        stops,
    })
}

/// The gradient a `FillPattern` of 25 to 40 stands for: the layout and angle of its row of the table, and the two
/// fill colours, or ground, foreground, ground for 26 and 29.
fn legacy_gradient(r: &Reader<'_, '_>, sheet: &SheetRef, pattern: i64) -> GradientLook {
    let direct = CellAddr::direct;
    let (dir, angle) = match pattern {
        25 => (0, 0.0),
        26 | 27 => (0, 180.0),
        28 | 29 => (0, 90.0),
        30 => (0, 270.0),
        31 => (12, 0.0),
        32 => (11, 0.0),
        33 => (9, 0.0),
        34 => (8, 0.0),
        35 => (10, 0.0),
        36 => (7, 0.0),
        37 => (6, 0.0),
        38 => (2, 0.0),
        39 => (1, 0.0),
        _ => (3, 0.0),
    };
    let fore = StopLook {
        position: 0.0,
        colour: r.colour(sheet, &direct("FillForegnd")).unwrap_or(Rgb(255, 255, 255)),
        transparency: rounded(r.num(sheet, &direct("FillForegndTrans"), 0.0) * 100.0, 2),
    };
    let back = StopLook {
        position: 1.0,
        colour: r.colour(sheet, &direct("FillBkgnd")).unwrap_or(Rgb(0, 0, 0)),
        transparency: rounded(r.num(sheet, &direct("FillBkgndTrans"), 0.0) * 100.0, 2),
    };
    let stops = if matches!(pattern, 26 | 29) {
        vec![back, StopLook { position: 0.5, ..fore }, StopLook { position: 1.0, ..back }]
    } else {
        vec![fore, back]
    };
    GradientLook { dir, angle, rotate_with_shape: true, stops }
}

/// What the controls of fill and line transparency, patterns, gradients and shadows show of a shape.
///
/// **What is not painted is not shown**: a shape that paints no fill reads no transparency, ground or gradient of its fill, one that draws no line none of
/// its line's, and one that paints neither has no shadow (a shadow takes the shape's fill and strokes). So the look of a shape does not change with a theme variant
/// for what the picture does not draw: a text box has no fill and no line, and the same look in every variant.
pub(super) fn fx_look(r: &Reader<'_, '_>, sheet: &SheetRef, page: u32) -> super::look::FxLook {
    let direct = CellAddr::direct;
    let percent = |name: &str| rounded(r.num(sheet, &direct(name), 0.0) * 100.0, 2);
    let pattern = pattern_shown(r.num(sheet, &direct("FillPattern"), 0.0)).round().clamp(0.0, 254.0) as u32;
    let filled = pattern != 0;
    let stroked = pattern_shown(r.num(sheet, &direct("LinePattern"), 0.0)).round() as i64 != 0;
    super::look::FxLook {
        fill_pattern: pattern,
        fill_transparency: if filled { percent("FillForegndTrans") } else { 0.0 },
        fill_background: (2..=24).contains(&pattern).then(|| r.colour(sheet, &direct("FillBkgnd")).unwrap_or(Rgb(255, 255, 255))),
        line_transparency: if stroked { percent("LineColorTrans") } else { 0.0 },
        line_cap: if stroked { r.num(sheet, &direct("LineCap"), 0.0).round().clamp(0.0, 2.0) as u8 } else { 0 },
        compound: if stroked { r.num(sheet, &direct("CompoundType"), 0.0).round().clamp(0.0, 4.0) as u8 } else { 0 },
        gradient: gradient_of(r, sheet, GradientTarget::Fill),
        line_gradient: if stroked { gradient_of(r, sheet, GradientTarget::Line) } else { None },
        shadow: if filled || stroked { shadow_of(r, sheet, page) } else { None },
    }
}
