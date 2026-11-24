//! Hatches and stipples: the fill patterns 2 to 24.
//!
//! The artwork is ours, of the table's kind: the specification gives each number's kind and says nothing of size, so the lines are a fixed
//! size on paper, tiled from the page's origin. The marks are the foreground and what lies between them the ground.
//! A stipple is drawn as the flat mixture of the two colours at the share of foreground the table's words suggest: a speckle too fine to
//! see as dots at any zoom the window shows is the same colour from a distance.

use diacraft_color::Rgba;
use kurbo::{BezPath, Point, Rect, Stroke};

use crate::limits::MAX_MARKS;
use crate::target::{Paint, Target};

/// One family of parallel lines: the direction in degrees counterclockwise from the x-axis, the distance between lines and their width,
/// both in paper inches.
#[derive(Clone, Copy)]
struct Lines {
    degrees: f64,
    spacing: f64,
    width: f64,
}

/// What a number paints: lines, or a flat share of the foreground.
enum Art {
    /// One family of lines, or two crossing.
    Hatch(Lines, Option<Lines>),
    Stipple(f64),
}

const ANGLE_RISING: f64 = 45.0;
const ANGLE_FALLING: f64 = 135.0;

const fn lines(degrees: f64, spacing: f64, width: f64) -> Lines {
    Lines { degrees, spacing, width }
}

fn art(number: u8) -> Art {
    match number {
        2 => Art::Hatch(lines(ANGLE_RISING, 0.1, 0.012), None),
        3 => Art::Hatch(lines(0.0, 0.1, 0.012), Some(lines(90.0, 0.1, 0.012))),
        4 => Art::Hatch(lines(ANGLE_RISING, 0.1, 0.012), Some(lines(ANGLE_FALLING, 0.1, 0.012))),
        5 => Art::Hatch(lines(ANGLE_FALLING, 0.1, 0.012), None),
        6 => Art::Hatch(lines(0.0, 0.1, 0.012), None),
        7 => Art::Hatch(lines(90.0, 0.1, 0.012), None),
        8 => Art::Stipple(0.85),
        9 => Art::Stipple(0.5),
        10 => Art::Stipple(0.35),
        11 => Art::Stipple(0.2),
        12 => Art::Stipple(0.1),
        13 => Art::Hatch(lines(0.0, 0.06, 0.03), None),
        14 => Art::Hatch(lines(90.0, 0.06, 0.03), None),
        15 => Art::Hatch(lines(ANGLE_FALLING, 0.06, 0.03), None),
        16 => Art::Hatch(lines(ANGLE_RISING, 0.06, 0.03), None),
        17 => Art::Stipple(0.5),
        18 => Art::Stipple(0.6),
        19 => Art::Hatch(lines(0.0, 0.04, 0.006), None),
        20 => Art::Hatch(lines(90.0, 0.04, 0.006), None),
        21 => Art::Hatch(lines(ANGLE_FALLING, 0.04, 0.006), None),
        22 => Art::Hatch(lines(ANGLE_RISING, 0.04, 0.006), None),
        23 => Art::Hatch(lines(0.0, 0.04, 0.006), Some(lines(90.0, 0.04, 0.006))),
        _ => Art::Stipple(0.5),
    }
}

/// Fills `region` (paper inches) with pattern `number`.
pub(crate) fn draw<T: Target>(canvas: &mut T, number: u8, fore: Rgba, back: Rgba, region: &BezPath) {
    match art(number) {
        Art::Stipple(share) => canvas.fill(region, &Paint::Solid(back.lerp(fore, share)), true),
        Art::Hatch(first, second) => {
            canvas.fill(region, &Paint::Solid(back), true);
            let Some(wanted) = wanted(canvas, region) else {
                return;
            };
            canvas.push_clip(region);
            for family in std::iter::once(first).chain(second) {
                let marks = hatch_path(family, wanted);
                let stroke = Stroke::new(family.width);
                canvas.stroke(&marks, &stroke, &Paint::Solid(fore));
            }
            canvas.pop_clip();
        }
    }
}

/// How many lines a fill of pattern `number` over `region` draws on `canvas`, as [`draw`] makes them: the work the fill costs, counted before it is
/// done. None for a stipple, which is one flat fill.
pub(crate) fn marks<T: Target>(canvas: &T, number: u8, region: &BezPath) -> u64 {
    let Art::Hatch(first, second) = art(number) else {
        return 0;
    };
    let Some(wanted) = wanted(canvas, region) else {
        return 0;
    };
    std::iter::once(first).chain(second).filter_map(|family| hatch_lines(family, wanted)).map(|(_, count)| count as u64).sum()
}

/// The part of `region` (paper inches) the marks are drawn over: only what the view can see needs marks, and the region's own extent bounds them. `None` when
/// the two do not meet or the target says no view.
fn wanted<T: Target>(canvas: &T, region: &BezPath) -> Option<Rect> {
    let view = canvas.paper_view()?;
    let wanted = region.control_box().intersect(view);
    (wanted.width() >= 0.0 && wanted.height() >= 0.0).then_some(wanted)
}

/// The lines of one family that cross `rect`: the number of the first, counted in spacings from the page's origin along the lines' normal, and how many there
/// are. `None` when no line is drawn: a spacing that is no positive number, a rectangle that is not finite, or more lines than [`MAX_MARKS`].
fn hatch_lines(family: Lines, rect: Rect) -> Option<(f64, usize)> {
    let (sin, cos) = family.degrees.to_radians().sin_cos();
    let (nx, ny) = (-sin, cos);
    let offsets = [rect.x0 * nx + rect.y0 * ny, rect.x1 * nx + rect.y0 * ny, rect.x0 * nx + rect.y1 * ny, rect.x1 * nx + rect.y1 * ny];
    let (lo, hi) = offsets.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), o| (lo.min(*o), hi.max(*o)));
    if !(family.spacing > 0.0 && lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let first = (lo / family.spacing).floor();
    let count = ((hi - lo) / family.spacing).ceil() + 1.0;
    // At least 1, since `hi` is at least `lo`; a count that is no number is no line.
    if count.is_nan() || count > MAX_MARKS as f64 {
        return None;
    }
    Some((first, count as usize))
}

/// The lines of one family that cross `rect`, tiled from the origin of the page.
fn hatch_path(family: Lines, rect: Rect) -> BezPath {
    let mut path = BezPath::new();
    let (sin, cos) = family.degrees.to_radians().sin_cos();
    // The lines run along (cos, sin); `n` is their normal. A line is the set of points whose offset along `n` is a multiple of the spacing.
    let (nx, ny) = (-sin, cos);
    let Some((first, count)) = hatch_lines(family, rect) else {
        return path;
    };
    // Each line is long enough to cross the rectangle from corner to corner.
    let reach = rect.width().hypot(rect.height()) + family.spacing;
    let centre = rect.center();
    for k in 0..count {
        let offset = (first + k as f64) * family.spacing;
        // The point of the line nearest the rectangle's centre.
        let along = centre.x * cos + centre.y * sin;
        let base = Point::new(offset * nx + along * cos, offset * ny + along * sin);
        path.move_to(Point::new(base.x - reach * cos, base.y - reach * sin));
        path.line_to(Point::new(base.x + reach * cos, base.y + reach * sin));
    }
    path
}
