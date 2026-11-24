//! The grid and the rulers: overlays on the drawing area, turned on and off from the View tab.
//!
//! The rules are: (a fixed grid falls at the origin plus
//! whole multiples of the spacing), (a variable grid's pitch is 1/8 inch times a power of 2, at least
//! so many pixels apart for its density), (zero is the ruler origin, and labels read the
//! distance from it in the page's unit at the drawing's scale) and (ticks at least so many pixels apart for the
//! ruler's density, a number at some of them). The variable grid's pitch and the ruler's ticks are project rules no source gives. **The grid's lines are the
//! engine's** (`diacraft_engine::snap::GridCells::grid`: taken in drawing inches, so that a 1:100 drawing's grid follows
//! its scale), the lines a drag snaps to; this file draws them. The ruler's ticks lie at whole multiples of a pitch **counted
//! from the ruler's zero** ([`ruler_ticks`]), so its numbers are whole round numbers of the page's unit, whatever the origin
//! and the scale. Neither overlay is page content, so `render` and exports draw neither.

use diacraft_units::Unit;
use egui::{Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

use super::page_drawing::{PageFrame, PageSettings};
use super::view::View;
use crate::theme::{self, Tokens};

/// The ruler's thickness, in points.
pub const RULER_THICKNESS: f32 = 22.0;

/// The smallest pitch of a ruler counted in halves, in its unit (1/8), which doubles until the ticks are far enough apart; and what a
/// grid line's pitch that is not a number reads as.
const BASE_PITCH: f64 = 0.125;

/// The farthest the pitch is allowed to grow to, in doublings of [`BASE_PITCH`] (2^24 eighths). A bound for the search, which a finite scale ends
/// far sooner.
const MAX_DOUBLINGS: i32 = 24;

/// The most lines or ticks one axis draws: a hostile spacing or zoom cannot make a frame draw millions.
const MAX_MARKS: usize = 4_000;

/// The least distance between two ticks, in points, at the ruler's density: 32 gives 8, 16 gives 16, 8 gives 32.
/// Any other density reads as 16.
fn ruler_min_points(density: f64) -> f64 {
    match density.round() as i64 {
        32 => 8.0,
        16 => 16.0,
        8 => 32.0,
        _ => 16.0,
    }
}

/// One axis of the grid: where its lines fall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridAxis {
    /// The distance between two lines, in paper inches.
    pub pitch: f64,
    /// Where a line falls, in paper inches: lines are at `origin + k * pitch` for whole `k`.
    pub origin: f64,
}

/// The values `origin + k * pitch` that lie in `lo..=hi`, at most [`MAX_MARKS`] of them, each with its `k`.
fn marks(axis: GridAxis, lo: f64, hi: f64) -> impl Iterator<Item = (i64, f64)> {
    let pitch = if axis.pitch > 0.0 && axis.pitch.is_finite() { axis.pitch } else { BASE_PITCH };
    let first = ((lo - axis.origin) / pitch).ceil();
    let first = if first.is_finite() { first.clamp(-1.0e9, 1.0e9) as i64 } else { 0 };
    (0..MAX_MARKS as i64).map_while(move |i| {
        let k = first.checked_add(i)?;
        let v = axis.origin + k as f64 * pitch;
        (v <= hi + 1e-9).then_some((k, v))
    })
}

/// The lines of the grid the window draws on one axis: the snap's (`diacraft_engine::snap::GridCells::grid`), which follow the drawing's
/// units and scale (in drawing inches), so that a drag snaps to the lines that are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLines {
    /// Where the lines fall, in paper inches; `None` on an axis with no line (a fixed grid of spacing 0).
    pub x: Option<GridAxis>,
    pub y: Option<GridAxis>,
    /// Drawing inches to a paper inch: a line at a whole drawing inch stands out when the lines are closer than an inch of the drawing.
    pub k: f64,
}

/// Paints the grid `lines` over the page `frame`, clipped to the page and to the drawing area.
pub fn paint_grid(painter: &Painter, t: &Tokens, view: &View, area: Rect, frame: &PageFrame, s: &PageSettings, lines: &GridLines) -> (usize, usize) {
    let clip = frame.rect.intersect(area);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return (0, 0);
    }
    let painter = painter.with_clip_rect(clip);
    let (w, h) = s.size;
    let k = if lines.k.is_finite() && lines.k > 0.0 { lines.k } else { 1.0 };
    // Whole inches of the drawing stand out when the lines are closer than an inch of it.
    let line_for = |v: f64, pitch: f64| {
        let (v, pitch) = (v * k, pitch * k);
        let whole = pitch < 1.0 - 1e-9 && (v - v.round()).abs() < 1e-6;
        Stroke::new(1.0, if whole { t.grid_major } else { t.grid_line })
    };
    let (mut across, mut down) = (0, 0);
    if let Some(ax) = lines.x {
        for (_, x) in marks(ax, 0.0, w) {
            let sx = view.to_screen(area, (x, 0.0)).x;
            painter.line_segment([pos2(sx, clip.top()), pos2(sx, clip.bottom())], line_for(x, ax.pitch));
            across += 1;
        }
    }
    if let Some(ay) = lines.y {
        for (_, y) in marks(ay, 0.0, h) {
            let sy = view.to_screen(area, (0.0, y)).y;
            painter.line_segment([pos2(clip.left(), sy), pos2(clip.right(), sy)], line_for(y, ay.pitch));
            down += 1;
        }
    }
    (across, down)
}

/// One tick of a ruler: where it falls on paper, and the number written at it (only every few ticks has one).
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// Paper inches from the page's left edge (or its bottom edge).
    pub paper: f64,
    pub label: Option<String>,
}

/// How one ruler counts: in which unit, at what scale, and from where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerAxis {
    /// The unit the numbers are in (`DrawingScale`'s `U`).
    pub unit: Unit,
    /// Paper inches over drawing inches.
    pub drawing_scale: f64,
    /// Where zero falls, in drawing inches from the page's edge (`XRulerOrigin`, `YRulerOrigin`).
    pub origin: f64,
}

/// The smallest of 1, 2 and 5 times a power of ten that is at least `x`. 1 for a `x` that is not above 0.
fn ladder_at_least(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let base = 10f64.powf(x.log10().floor().clamp(-9.0, 15.0));
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * base).find(|c| *c >= x * (1.0 - 1e-9)).unwrap_or(10.0 * base)
}

/// Whether a unit is counted in halves (an inch and what is made of inches), against tens.
fn counts_in_halves(unit: Unit) -> bool {
    matches!(
        unit,
        Unit::Inch
            | Unit::InchFine
            | Unit::Foot
            | Unit::Yard
            | Unit::Mile
            | Unit::MileFine
            | Unit::FeetInches
            | Unit::DrawingDefault
            | Unit::PageDefault
            | Unit::TypeDefault
    )
}

/// The ticks of a ruler between `lo` and `hi` paper inches, at `points_per_inch` screen points to a paper inch and at least `min_points`
/// apart.
///
/// **Whole numbers from zero.** Zero is the ruler origin, and the ticks lie at whole multiples of a pitch counted from it, in the page's
/// own unit, so the numbers written are the multiples and nothing else: a ruler whose zero is 0.37 inch along still reads 0, 1, 2. (The
/// first ruler laid its ticks from the page's edge and subtracted the origin from each, which printed 0.63, 1.63.) The unit is
/// `DrawingScale`'s, so a page in millimetres reads millimetres, and the numbers are at the drawing's scale. The pitch is a
/// project rule (gives none for units but the inch): the inches (and feet, yards, miles) halve from an eighth, and every other
/// unit steps by 1, 2 and 5 times a power of ten; a number is written at every fourth or fifth tick, or at every whole unit when the ticks
/// are closer than a quarter.
pub fn ruler_ticks(axis: RulerAxis, points_per_inch: f64, min_points: f64, lo: f64, hi: f64) -> Vec<Tick> {
    let scale = if axis.drawing_scale.is_finite() && axis.drawing_scale > 0.0 { axis.drawing_scale } else { 1.0 };
    let per_unit = axis.unit.base_per_unit();
    let per_unit = if per_unit.is_finite() && per_unit > 0.0 { per_unit } else { 1.0 };
    let origin = if axis.origin.is_finite() { axis.origin } else { 0.0 };
    let Some((pitch, per_label)) = ruler_pitch(axis, points_per_inch, min_points) else { return Vec::new() };
    // A value in units is `paper / (scale * per_unit) - origin / per_unit`.
    let to_value = |paper: f64| paper / (scale * per_unit) - origin / per_unit;
    let (from, to) = (to_value(lo) / pitch, to_value(hi) / pitch);
    if !(from.is_finite() && to.is_finite()) {
        return Vec::new();
    }
    let first = from.min(to).ceil().clamp(-1.0e9, 1.0e9) as i64;
    let mut ticks = Vec::new();
    for i in 0..MAX_MARKS as i64 {
        let Some(k) = first.checked_add(i) else { break };
        if k as f64 > from.max(to) + 1e-9 {
            break;
        }
        let value = k as f64 * pitch;
        let label = (k.rem_euclid(per_label) == 0).then(|| ruler_label(value));
        ticks.push(Tick { paper: (origin + value * per_unit) * scale, label });
    }
    ticks
}

/// How far apart a ruler's ticks are at `points_per_inch`, in the ruler's unit, and how many ticks there are between two numbers: the pitch [`ruler_ticks`] lays its ticks at.
/// `None` for a zoom, a scale or a density that gives none.
fn ruler_pitch(axis: RulerAxis, points_per_inch: f64, min_points: f64) -> Option<(f64, i64)> {
    let scale = if axis.drawing_scale.is_finite() && axis.drawing_scale > 0.0 { axis.drawing_scale } else { 1.0 };
    let per_unit = axis.unit.base_per_unit();
    let per_unit = if per_unit.is_finite() && per_unit > 0.0 { per_unit } else { 1.0 };
    // Points to one unit of the ruler, and the least pitch in units.
    let points_per_unit = points_per_inch * scale * per_unit;
    if !(points_per_unit.is_finite() && points_per_unit > 0.0 && min_points.is_finite() && min_points > 0.0) {
        return None;
    }
    let least = min_points / points_per_unit;
    if counts_in_halves(axis.unit) {
        let mut pitch = BASE_PITCH;
        let mut doublings = 0;
        while pitch < least && doublings < MAX_DOUBLINGS {
            pitch *= 2.0;
            doublings += 1;
        }
        Some((pitch, if pitch < 0.25 { (1.0 / pitch).round() as i64 } else { 4 }))
    } else {
        let pitch = ladder_at_least(least);
        Some((pitch, ((ladder_at_least(4.0 * pitch) / pitch).round() as i64).max(1)))
    }
}

/// How far a ruler's ticks are apart at `points_per_inch` screen points to a paper inch, in paper inches, **as the window's ruler draws them** (`s` the page's settings,
/// `horizontal` the ruler along the top): what an arrow key moves a shape by when no place is ahead of it (`snap: {nudge: {tick}}`). `None` for a ruler with
/// no ticks at this zoom.
pub fn ruler_tick(s: &PageSettings, points_per_inch: f64, horizontal: bool) -> Option<f64> {
    let (density, origin) = if horizontal { (s.ruler_density.0, s.ruler_origin.0) } else { (s.ruler_density.1, s.ruler_origin.1) };
    let axis = RulerAxis { unit: s.ruler_unit, drawing_scale: s.drawing_scale, origin };
    let (pitch, _) = ruler_pitch(axis, points_per_inch, ruler_min_points(density))?;
    let scale = if axis.drawing_scale.is_finite() && axis.drawing_scale > 0.0 { axis.drawing_scale } else { 1.0 };
    let per_unit = axis.unit.base_per_unit();
    let per_unit = if per_unit.is_finite() && per_unit > 0.0 { per_unit } else { 1.0 };
    let paper = pitch * per_unit * scale;
    (paper.is_finite() && paper > 0.0).then_some(paper)
}

/// A ruler's number: whole when it is, else up to three decimals without trailing zeros. The sign of a value that rounds to
/// zero is dropped.
pub fn ruler_label(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".to_owned();
    }
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Which side a ruler lies along.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Left,
}

/// Paints both rulers and the square where they meet. `bounds` is the whole drawing area including the rulers, `area` the
/// part inside them.
pub fn paint_rulers(painter: &Painter, t: &Tokens, view: &View, bounds: Rect, area: Rect, frame: &PageFrame, s: &PageSettings) {
    let corner = Rect::from_min_max(bounds.min, pos2(area.left(), area.top()));
    painter.rect_filled(corner, 0.0, t.ruler_fill);
    paint_ruler(painter, t, view, area, frame, s, Side::Top);
    paint_ruler(painter, t, view, area, frame, s, Side::Left);
    painter.line_segment([pos2(bounds.left(), area.top()), pos2(bounds.right(), area.top())], Stroke::new(1.0, t.separator));
    painter.line_segment([pos2(area.left(), bounds.top()), pos2(area.left(), bounds.bottom())], Stroke::new(1.0, t.separator));
}

fn paint_ruler(painter: &Painter, t: &Tokens, view: &View, area: Rect, frame: &PageFrame, s: &PageSettings, side: Side) {
    let scale = view.scale();
    let strip = match side {
        Side::Top => Rect::from_min_max(pos2(area.left(), area.top() - RULER_THICKNESS), pos2(area.right(), area.top())),
        Side::Left => Rect::from_min_max(pos2(area.left() - RULER_THICKNESS, area.top()), pos2(area.left(), area.bottom())),
    };
    let painter = painter.with_clip_rect(strip);
    painter.rect_filled(strip, 0.0, t.ruler_fill);
    // The part of the ruler over the page is lighter, as the page is lighter than the workspace.
    let over_page = match side {
        Side::Top => Rect::from_x_y_ranges(frame.rect.x_range(), strip.y_range()),
        Side::Left => Rect::from_x_y_ranges(strip.x_range(), frame.rect.y_range()),
    };
    painter.rect_filled(over_page.intersect(strip), 0.0, t.ruler_page_fill);
    let (density, origin) = match side {
        Side::Top => (s.ruler_density.0, s.ruler_origin.0),
        Side::Left => (s.ruler_density.1, s.ruler_origin.1),
    };
    let axis = RulerAxis { unit: s.ruler_unit, drawing_scale: s.drawing_scale, origin };
    // The page coordinates the strip shows, a little beyond its ends.
    let (lo, hi) = match side {
        Side::Top => (view.to_page(area, pos2(strip.left(), strip.top())).0, view.to_page(area, pos2(strip.right(), strip.top())).0),
        Side::Left => (view.to_page(area, pos2(strip.left(), strip.bottom())).1, view.to_page(area, pos2(strip.left(), strip.top())).1),
    };
    let font = theme::font(10.0);
    for Tick { paper, label } in ruler_ticks(axis, scale, ruler_min_points(density), lo, hi) {
        let ink = Stroke::new(1.0, if label.is_some() { t.ruler_tick } else { t.ruler_tick_minor });
        let len = if label.is_some() { RULER_THICKNESS * 0.62 } else { RULER_THICKNESS * 0.3 };
        match side {
            Side::Top => {
                let x = view.to_screen(area, (paper, 0.0)).x;
                painter.line_segment([pos2(x, strip.bottom() - len), pos2(x, strip.bottom())], ink);
                if let Some(label) = label {
                    painter.text(pos2(x + 2.0, strip.top() + 1.0), Align2::LEFT_TOP, label, font.clone(), t.ruler_text);
                }
            }
            Side::Left => {
                let y = view.to_screen(area, (0.0, paper)).y;
                painter.line_segment([pos2(strip.right() - len, y), pos2(strip.right(), y)], ink);
                if let Some(label) = label {
                    vertical_text(&painter, pos2(strip.left() + 1.0, y - 2.0), &label, &font, t.ruler_text);
                }
            }
        }
    }
}

/// `text` turned a quarter to the left, starting at `at` and running up.
fn vertical_text(painter: &Painter, at: Pos2, text: &str, font: &egui::FontId, colour: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), colour);
    // Turned a quarter about its top left corner, the text runs up from there: that corner is where it starts.
    let shape = egui::epaint::TextShape::new(at, galley, colour).with_angle(-std::f32::consts::FRAC_PI_2);
    painter.add(Shape::Text(shape));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_of_an_axis_fall_at_the_origin_plus_whole_pitches() {
        let lines: Vec<f64> = marks(GridAxis { pitch: 0.5, origin: 0.25 }, 0.0, 2.0).map(|m| m.1).collect();
        assert_eq!(lines, vec![0.25, 0.75, 1.25, 1.75]);
    }

    #[test]
    fn a_ruler_tick_is_the_pitch_the_ruler_draws_at_the_zoom_in_paper_inches() {
        let inches = PageSettings::default();
        // 32 to the inch (the root's density) wants the ticks 8 points apart: an eighth of an inch at 96 points to an inch, a quarter at half that, a half at a quarter.
        assert_eq!(ruler_tick(&inches, 96.0, true), Some(0.125));
        assert_eq!(ruler_tick(&inches, 48.0, true), Some(0.25));
        assert_eq!(ruler_tick(&inches, 24.0, false), Some(0.5));
        // The ticks the ruler draws are that far apart.
        let axis = RulerAxis { unit: Unit::Inch, drawing_scale: 1.0, origin: 0.0 };
        let ticks = ruler_ticks(axis, 48.0, ruler_min_points(inches.ruler_density.0), 0.0, 2.0);
        assert!(ticks.windows(2).all(|t| (t[1].paper - t[0].paper - 0.25).abs() < 1e-12), "{ticks:?}");
        // A ruler in millimetres steps by 1, 2 and 5: at 96 points to an inch (3.78 points a millimetre) the least pitch of 8 points is 2.1 mm, so 5 mm. A page drawn at 1:2
        // (a drawing inch is half a paper inch) has its ticks a quarter of a drawing inch apart, an eighth of a paper inch.
        let mm = PageSettings { ruler_unit: Unit::Millimetre, ..PageSettings::default() };
        assert!((ruler_tick(&mm, 96.0, true).unwrap() - 5.0 / 25.4).abs() < 1e-12, "{:?}", ruler_tick(&mm, 96.0, true));
        let scaled = PageSettings { ruler_unit: Unit::Inch, drawing_scale: 0.5, ..PageSettings::default() };
        assert_eq!(ruler_tick(&scaled, 96.0, true), Some(0.125));
        // No zoom, no ruler.
        assert_eq!(ruler_tick(&inches, 0.0, true), None);
        assert_eq!(ruler_tick(&inches, f64::NAN, true), None);
    }

    #[test]
    fn a_ruler_density_the_rules_do_not_name_reads_as_normal() {
        assert_eq!(ruler_min_points(20.0), ruler_min_points(16.0));
    }

    #[test]
    fn marks_are_capped_and_survive_hostile_numbers() {
        assert_eq!(marks(GridAxis { pitch: 1e-12, origin: 0.0 }, 0.0, 1e9).count(), MAX_MARKS);
        // A pitch that is not a number reads as the base pitch; an origin that is not a number leaves nothing to count from.
        assert_eq!(marks(GridAxis { pitch: f64::NAN, origin: 0.0 }, 0.0, 10.0).count(), 81);
        assert_eq!(marks(GridAxis { pitch: 1.0, origin: f64::NAN }, 0.0, 10.0).count(), 0);
        assert_eq!(marks(GridAxis { pitch: 1.0, origin: 0.0 }, f64::NAN, 3.0).count(), 4);
    }

    /// The labels of the ticks, with where each falls.
    fn labelled(ticks: &[Tick]) -> Vec<(String, f64)> {
        ticks.iter().filter_map(|t| t.label.clone().map(|l| (l, t.paper))).collect()
    }

    fn axis(unit: Unit, drawing_scale: f64, origin: f64) -> RulerAxis {
        RulerAxis { unit, drawing_scale, origin }
    }

    #[test]
    fn a_ruler_with_an_origin_reads_whole_numbers_counted_from_it() {
        // The first ruler laid its ticks from the page's edge and printed the origin away from each: with zero 0.37 inch along, 0.63, 1.63.
        // At 96 points to the inch, an eighth is 12 points, over the 8 of the finest ruler: a tick every eighth, a number every inch.
        let ticks = ruler_ticks(axis(Unit::Inch, 1.0, 0.37), 96.0, 8.0, 0.0, 4.0);
        let labels = labelled(&ticks);
        let words: Vec<&str> = labels.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(words, ["0", "1", "2", "3"], "from the ruler's zero, whole inches: {words:?}");
        for (n, (_, paper)) in labels.iter().enumerate() {
            assert!((paper - (0.37 + n as f64)).abs() < 1e-9, "the number {n} falls {paper} inches along");
        }
        // Behind zero the numbers are negative whole inches.
        let behind = ruler_ticks(axis(Unit::Inch, 1.0, 3.2), 96.0, 8.0, 0.0, 4.0);
        assert_eq!(labelled(&behind).iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["-3", "-2", "-1", "0"]);
    }

    #[test]
    fn a_page_in_millimetres_counts_millimetres_and_not_inches() {
        // 1 mm is 1/25.4 inch. Zero 1 inch along; at 96 points to the inch a millimetre is 3.78 points, so the 8 point tick is 2.12 mm: ticks every
        // 5 mm, a number every 20.
        let ticks = ruler_ticks(axis(Unit::Millimetre, 1.0, 1.0), 96.0, 8.0, 0.0, 6.0);
        let labels = labelled(&ticks);
        let words: Vec<&str> = labels.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(words, ["-20", "0", "20", "40", "60", "80", "100", "120"], "{words:?}");
        let at = |word: &str| labels.iter().find(|(l, _)| l == word).map(|(_, p)| *p).unwrap();
        assert!((at("0") - 1.0).abs() < 1e-9 && (at("20") - (1.0 + 20.0 / 25.4)).abs() < 1e-9, "20 mm along from zero, not 20 inches");
        // The ticks between are 5 mm apart.
        let paper: Vec<f64> = ticks.iter().map(|t| t.paper).collect();
        assert!((paper[1] - paper[0] - 5.0 / 25.4).abs() < 1e-9);
        // Every number is whole.
        assert!(words.iter().all(|w| w.parse::<i64>().is_ok()), "{words:?}");
    }

    #[test]
    fn the_ruler_measures_at_the_drawing_scale() {
        // In feet, with zero 12 drawing inches along, a point 36 drawing inches along reads 2 feet (the origin taken off, then the
        // unit): ignoring the origin gives 3 and ignoring the unit gives 24. With a page scaled 1 inch of paper to 12 of drawing it is
        // 3 inches along the paper.
        let scale = 1.0 / 12.0;
        let ticks = ruler_ticks(axis(Unit::Foot, scale, 12.0), 96.0, 8.0, 0.0, 1.0);
        let labels = labelled(&ticks);
        let at = |word: &str| labels.iter().find(|(l, _)| l == word).map(|(_, p)| *p).unwrap();
        // 2 feet past zero is 12 + 24 = 36 drawing inches, 36 / 12 = 3 paper inches.
        assert!((at("0") - 1.0).abs() < 1e-9, "zero is 12 drawing inches, 1 inch of paper: {}", at("0"));
        let two = ruler_ticks(axis(Unit::Foot, scale, 12.0), 96.0, 8.0, 0.0, 4.0);
        let found = labelled(&two).into_iter().find(|(l, _)| l == "2").map(|(_, p)| p).unwrap();
        assert!((found - 3.0).abs() < 1e-9, "2 ft along from zero is 3 inches of paper: {found}");
    }

    #[test]
    fn numbers_are_whole_at_every_zoom_in_every_unit_that_has_a_unit_step() {
        for unit in [Unit::Inch, Unit::Foot, Unit::Millimetre, Unit::Centimetre, Unit::Metre, Unit::Point] {
            for points_per_inch in [4.0, 24.0, 96.0, 384.0] {
                let ticks = ruler_ticks(axis(unit, 1.0, 0.37), points_per_inch, 8.0, 0.0, 30.0);
                for (label, _) in labelled(&ticks) {
                    let n: f64 = label.parse().unwrap();
                    assert!(label.len() <= 10, "{unit:?} at {points_per_inch}: {label}");
                    // Zoomed in far enough that a unit is wide, a ruler may number halves, quarters or tenths; never a long decimal.
                    assert!(label.split('.').nth(1).is_none_or(|d| d.len() <= 3), "{unit:?} at {points_per_inch}: {label} ({n})");
                }
            }
        }
        // At the usual zoom the numbers are all whole.
        let usual = labelled(&ruler_ticks(axis(Unit::Inch, 1.0, 0.37), 96.0, 8.0, 0.0, 30.0));
        assert!(usual.iter().all(|(l, _)| l.parse::<i64>().is_ok()));
    }

    #[test]
    fn a_ruler_survives_hostile_numbers() {
        let hostile = [f64::NAN, f64::INFINITY, -1.0, 0.0, 1.0e300];
        for a in hostile {
            for b in hostile {
                let _ = ruler_ticks(axis(Unit::Inch, a, b), a, b, a, b);
                let _ = ruler_ticks(axis(Unit::Millimetre, b, a), 96.0, 8.0, a, b);
            }
        }
        assert!(ruler_ticks(axis(Unit::Inch, 1.0, 0.0), 96.0, 8.0, 0.0, 1.0e12).len() <= MAX_MARKS);
        assert!(ruler_ticks(axis(Unit::Inch, 1.0, 0.0), 0.0, 8.0, 0.0, 4.0).is_empty());
    }

    #[test]
    fn ruler_numbers_are_short() {
        assert_eq!(ruler_label(0.0), "0");
        assert_eq!(ruler_label(-0.0004), "0");
        assert_eq!(ruler_label(2.0), "2");
        assert_eq!(ruler_label(0.5), "0.5");
        assert_eq!(ruler_label(0.125), "0.125");
        assert_eq!(ruler_label(-1.25), "-1.25");
    }
}
