//! What a shape's fill, line and shadow cells say, read for drawing.
//!
//! Every value is the saved one, found by inheritance (`read.rs`). A cell the document does not hold anywhere reads the caller's default,
//! which for the cells of a root style is the root's value in every real file.

use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, SectionKey, SheetRef};

use crate::layers::LayerColour;
use crate::limits::MAX_GRADIENT_STOPS;
use crate::read::Cx;
use crate::skips::SkipKind;

/// The sense of a linear gradient's angle, as the sign of `sin a` in the direction of
/// travel `(cos a, SIGN * sin a)` in the frame's own x right, y up. `-1.0` is the project rule: the angle grows **clockwise** as seen on
/// the page, so 90 degrees runs from the top to the bottom with the first stop at the top. **This is the one sign to flip, if a render
/// disagrees.** `+1.0` would run the angle counterclockwise.
pub(crate) const GRADIENT_ANGLE_SIGN: f64 = -1.0;

/// How far a shadow's blur reaches: the cell `ShapeShdwBlur` is read as 1.5 standard deviations (a
/// project rule, thin: it pairs value lists and ties no shadow to a cell), so the Gaussian's deviation is the cell over this. **Change it here**
/// if a render disagrees.
pub(crate) const SHADOW_BLUR_DEVIATIONS: f64 = 1.5;

/// A colour stop of a gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stop {
    pub pos: f64,
    pub color: Rgba,
}

/// A gradient's cells (`FillGradientDir`, `FillGradientAngle`, the rows of `FillGradient`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GradientSpec {
    /// 0 linear, 1 to 7 radial, 8 to 12 rectangular, 13 along the path.
    pub dir: u8,
    /// Radians.
    pub angle: f64,
    pub stops: Vec<Stop>,
    /// `RotateGradientWithShape`: the gradient's frame is the shape's own.
    pub rotate_with_shape: bool,
}

/// What the inside of a shape paints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FillLook {
    None,
    Solid(Rgba),
    Gradient(GradientSpec),
    /// A hatch or stipple (2 to 24): the marks in `fore`, the ground in `back`.
    Pattern {
        number: u8,
        fore: Rgba,
        back: Rgba,
    },
}

/// What the outline paints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LineLook {
    /// Paper inches.
    pub weight: f64,
    pub color: Rgba,
    pub pattern: i64,
    pub cap: u8,
    pub rounding: f64,
    pub begin_arrow: i64,
    pub end_arrow: i64,
    pub begin_size: i64,
    pub end_size: i64,
    /// The name each end's `USE` holds when the end is a master (254 and `USE("Name")`): which master the end is drawn from (`custom_end.rs`).
    pub begin_use: Option<std::sync::Arc<str>>,
    pub end_use: Option<std::sync::Arc<str>>,
    pub compound: u8,
    pub gradient: Option<GradientSpec>,
}

/// The kind of shadow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowKind {
    /// Type 1 (and a page's 0): the outline moved.
    Simple,
    /// Type 2: the outline leaned.
    Oblique,
    /// Type 3: inside the outline.
    Inner,
}

/// A shadow, with the page defaults already in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShadowLook {
    pub kind: ShadowKind,
    /// Paper inches, along the page's axes, y up.
    pub offset: (f64, f64),
    pub scale: f64,
    /// Radians clockwise from straight up.
    pub oblique: f64,
    pub color: Rgba,
    /// Paper inches.
    pub blur: f64,
    /// `ShapeShdwShow`.
    pub show: u8,
}

/// The straight-alpha colour with `trans` (0 opaque, 1 clear) applied.
pub(crate) fn with_trans(color: Rgba, trans: f64) -> Rgba {
    let alpha = if trans.is_finite() { (1.0 - trans).clamp(0.0, 1.0) } else { 1.0 };
    Rgba { a: (f64::from(color.a) * alpha).round().clamp(0.0, 255.0) as u8, ..color }
}

/// A byte from a number of a cell that holds a whole number in 0 to 255: anything else reads `default`.
fn byte(n: f64, default: i64) -> i64 {
    if n.is_finite() && n.abs() < 1.0e9 { n.round() as i64 } else { default }
}

pub(crate) struct Looks;

impl Looks {
    /// The fill of the shape at `at`. `layer` is the colour override of a shape that is on exactly one layer with a colour.
    pub(crate) fn fill(cx: &Cx<'_, '_>, at: &SheetRef, layer: Option<&LayerColour>, who: &dyn Fn() -> String) -> FillLook {
        let pattern = byte(cx.num_or(at, "FillPattern", 0.0), 1);
        if pattern == 0 {
            return FillLook::None;
        }
        // A layer's colour paints a fill white, in place of any pattern or gradient.
        if let Some(l) = layer {
            return FillLook::Solid(with_trans(Rgba::WHITE, l.trans));
        }
        let fore = with_trans(cx.colour(at, "FillForegnd").unwrap_or(Rgba::WHITE), cx.num(at, "FillForegndTrans"));
        let back = with_trans(cx.colour(at, "FillBkgnd").unwrap_or(Rgba::BLACK), cx.num(at, "FillBkgndTrans"));
        if cx.flag(at, "FillGradientEnabled", false) {
            let dir = byte(cx.num(at, "FillGradientDir"), 0);
            let angle = cx.num_or(at, "FillGradientAngle", std::f64::consts::FRAC_PI_2);
            let rotate = cx.flag(at, "RotateGradientWithShape", true);
            let stops = gradient_stops(cx, at, "FillGradient");
            let stops = if stops.is_empty() { vec![Stop { pos: 0.0, color: fore }, Stop { pos: 1.0, color: back }] } else { stops };
            return FillLook::Gradient(GradientSpec { dir: dir.clamp(0, 13) as u8, angle, stops, rotate_with_shape: rotate });
        }
        match pattern {
            1 => FillLook::Solid(fore),
            2..=24 => {
                cx.note(SkipKind::PatternApproximate, || format!("{}: fill pattern {pattern}", who()));
                FillLook::Pattern { number: pattern as u8, fore, back }
            }
            25..=40 => FillLook::Gradient(legacy_gradient(pattern, fore, back)),
            254 => {
                cx.note(SkipKind::CustomPattern, || format!("{}: fill pattern 254, a master as the pattern", who()));
                FillLook::Solid(fore)
            }
            // A number outside MS-VSDX's list paints the foreground.
            _ => FillLook::Solid(fore),
        }
    }

    /// The line of the shape at `at`, or `None` for a line that is not drawn (`LinePattern` 0).
    pub(crate) fn line(cx: &Cx<'_, '_>, at: &SheetRef, layer: Option<&LayerColour>) -> Option<LineLook> {
        let pattern = byte(cx.num_or(at, "LinePattern", 0.0), 1);
        if pattern == 0 {
            return None;
        }
        let color = match layer {
            Some(l) => with_trans(l.color, l.trans),
            None => with_trans(cx.colour(at, "LineColor").unwrap_or(Rgba::BLACK), cx.num(at, "LineColorTrans")),
        };
        // `LineWeight` is a thickness on paper. The root's value is 0.75 points, or 0.01 inch in a few files.
        let weight = cx.num_or(at, "LineWeight", 0.01).max(0.0);
        let gradient = (layer.is_none() && cx.flag(at, "LineGradientEnabled", false)).then(|| GradientSpec {
            dir: byte(cx.num(at, "LineGradientDir"), 0).clamp(0, 13) as u8,
            angle: cx.num_or(at, "LineGradientAngle", std::f64::consts::FRAC_PI_2),
            stops: gradient_stops(cx, at, "LineGradient"),
            rotate_with_shape: true,
        });
        let (begin_arrow, end_arrow) = (byte(cx.num(at, "BeginArrow"), 0), byte(cx.num(at, "EndArrow"), 0));
        Some(LineLook {
            weight,
            color,
            pattern,
            cap: byte(cx.num(at, "LineCap"), 0).clamp(0, 2) as u8,
            rounding: cx.num(at, "Rounding").max(0.0),
            begin_arrow,
            end_arrow,
            begin_size: byte(cx.num_or(at, "BeginArrowSize", 2.0), 2),
            end_size: byte(cx.num_or(at, "EndArrowSize", 2.0), 2),
            begin_use: if begin_arrow == 254 { crate::custom_end::used_name(cx, at, "BeginArrow") } else { None },
            end_use: if end_arrow == 254 { crate::custom_end::used_name(cx, at, "EndArrow") } else { None },
            compound: byte(cx.num(at, "CompoundType"), 0).clamp(0, 4) as u8,
            gradient: gradient.filter(|g| !g.stops.is_empty()),
        })
    }

    /// The shadow of the shape at `at` on the page `page`, or `None` when none shows. The page's cells stand in for the shape's five where
    /// its `ShapeShdwType` is 0.
    pub(crate) fn shadow(cx: &Cx<'_, '_>, at: &SheetRef, page: &SheetRef, layer: Option<&LayerColour>) -> Option<ShadowLook> {
        if cx.num(at, "ShdwPattern") == 0.0 {
            return None;
        }
        let own_type = byte(cx.num(at, "ShapeShdwType"), 0);
        let (kind_number, offset, oblique, scale) = if own_type == 0 {
            // The page's default; a page type of 0 draws as a simple shadow. A scale of 0 or missing reads 1.
            let scale = cx.num(page, "ShdwScaleFactor");
            (
                byte(cx.num(page, "ShdwType"), 0),
                (cx.num(page, "ShdwOffsetX"), cx.num(page, "ShdwOffsetY")),
                cx.num(page, "ShdwObliqueAngle"),
                if scale == 0.0 { 1.0 } else { scale },
            )
        } else {
            (
                own_type,
                (cx.num(at, "ShapeShdwOffsetX"), cx.num(at, "ShapeShdwOffsetY")),
                cx.num(at, "ShapeShdwObliqueAngle"),
                cx.num_or(at, "ShapeShdwScaleFactor", 1.0),
            )
        };
        let kind = match kind_number {
            2 => ShadowKind::Oblique,
            3 => ShadowKind::Inner,
            _ => ShadowKind::Simple,
        };
        // Shadows stay as they are under a layer's colour.
        let _ = layer;
        let color = with_trans(cx.colour(at, "ShdwForegnd").unwrap_or(Rgba::BLACK), cx.num(at, "ShdwForegndTrans"));
        Some(ShadowLook {
            kind,
            offset,
            scale,
            oblique,
            color,
            blur: cx.num(at, "ShapeShdwBlur").max(0.0),
            show: byte(cx.num(at, "ShapeShdwShow"), 0).clamp(0, 2) as u8,
        })
    }
}

/// The stops of a `FillGradient` or `LineGradient` section, merged along the chain, at most ten, in position order.
/// A row whose colour reads as nothing (a themed row past its list) is dropped.
fn gradient_stops(cx: &Cx<'_, '_>, at: &SheetRef, section: &str) -> Vec<Stop> {
    let key = SectionKey::new(section, None);
    let Some(rows) = cx.res.effective_rows(at, &key) else {
        return Vec::new();
    };
    let mut stops = Vec::new();
    for row in rows.iter().take(MAX_GRADIENT_STOPS) {
        let cell = |name: &str| CellAddr { section: Some(key.clone()), row: Some(row.key.clone()), cell: diacraft_doc::CellName::new(name) };
        let Some(color) = cx.colour_at(at, &cell("GradientStopColor")) else {
            continue;
        };
        let trans = cx.num_at(at, &cell("GradientStopColorTrans"), 0.0);
        let pos = cx.num_at(at, &cell("GradientStopPosition"), 0.0).clamp(0.0, 1.0);
        stops.push(Stop { pos, color: with_trans(color, trans) });
    }
    // A stable sort keeps equal positions in row order, which gives a hard edge.
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    stops
}

/// The gradient a legacy pattern number 25 to 40 stands for: the two fill colours, or ground, foreground, ground for 26
/// and 29.
fn legacy_gradient(pattern: i64, fore: Rgba, back: Rgba) -> GradientSpec {
    use std::f64::consts::{FRAC_PI_2, PI};
    let (dir, angle) = match pattern {
        25 => (0, 0.0),
        26 | 27 => (0, PI),
        28 | 29 => (0, FRAC_PI_2),
        30 => (0, 3.0 * FRAC_PI_2),
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
    let stops = if matches!(pattern, 26 | 29) {
        vec![Stop { pos: 0.0, color: back }, Stop { pos: 0.5, color: fore }, Stop { pos: 1.0, color: back }]
    } else {
        vec![Stop { pos: 0.0, color: fore }, Stop { pos: 1.0, color: back }]
    };
    GradientSpec { dir, angle, stops, rotate_with_shape: true }
}
