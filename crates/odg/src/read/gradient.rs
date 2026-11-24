//! **The stand-in of a `draw:gradient`**: the cells DiaCraft draws another program's gradient with, made once, when the
//! definition is read, and shared by every style that names it. The definition itself is held as written (`odfdoc.rs`), and the fill that names it (`held.rs`), so nothing here is
//! written back: this is what DiaCraft **draws**, as near as its own layouts let it to what LibreOffice draws.
//!
//! **What LibreOffice 26.8.2.2 draws** (a black box, measured on our own hand-made files: the report, which add the border, the intensities, the
//! sizes and the defaults). The colours run along a **ramp** of places 0 to 1: the `loext:gradient-stop` children (`svg:offset`, `loext:color-value`), sorted by place, or, when there are
//! none, the start colour at 0 and the end colour at 1. Each colour is scaled toward black by its intensity (`draw:start-intensity` at 0, `draw:end-intensity` at 1, and between them in
//! proportion); a place outside 0 to 1 is not cut, so the ramp is the line between its stops where it crosses 0 and 1; before the first stop the first colour holds, after the last the
//! last. `draw:border` (*b*) is a band of the start colour:
//!
//! | `draw:style` | Where place *o* of the ramp is drawn | `FillGradientDir` | Places of the rows |
//! |---|---|---|---|
//! | `linear` | at *b* + *o* (1 − *b*) of the way along `draw:angle`, which turns counterclockwise from a start at the top; `0` is top to bottom | 0, with `FillGradientAngle` 90° − `draw:angle` (runs clockwise from the right) | *b* + *o* (1 − *b*) |
//! | `axial` | at *o* (1 − *b*) of the way from the axis to either edge: the start colour on the axis, a border of the end colour at the edges | 0, the same angle | ½ ∓ ½ *o* (1 − *b*), each stop twice |
//! | `radial` | on the circle of radius *r* = (1 − *b*) − *o* (1 − *b*)² of half the box's diagonal, round (`draw:cx`, `draw:cy`): the start colour outside, the end colour at the centre, and a core of the end colour inside *r* = *b* (1 − *b*) | by the centre (below) | *r* / *k* |
//! | `ellipsoid`, `square`, `rectangular` | on the contour *r* = (1 − *b*) (1 − *o*) of an ellipse through the corners, a square of the box's longer side, or the box, each round its centre (`draw:cx`, `draw:cy`) | by the centre (below) | *r* / *k* |
//!
//! A centre with no `draw:cx` or `draw:cy` is at 0 % (the top-left corner, measured), and every size is the centred one, whichever centre. **The centre** is taken to the nearest of DiaCraft's
//! origins, each of `draw:cx` and `draw:cy` read as 0, 50 or 100 % by thirds: a corner or the centre, and the middle of the top or bottom edge as a radial layout (DiaCraft has no
//! rectangular one there); the middle of the left or right edge, which DiaCraft has no layout from, as the centre. *k* is the writer's own ratio of the two programs' sizes from that origin
//! (`write::fx::lo_scale_of`: 2 from a corner, 1.58 from an edge's middle, 1 from the centre), so that a gradient DiaCraft wrote reads back as near as the writer put it.
//!
//! **What DiaCraft cannot draw** (the difference measured stays): an ellipse, a square or a rectangle whose bands keep the gap between their sides (LibreOffice's inner contours are
//! the outer one moved in, DiaCraft's are it scaled), a circle on a box that is not square, the turn of a layout that has a centre, and a centre between DiaCraft's origins. A gradient of more
//! than ten rows is drawn from ten places evenly along it.

use diacraft_container::xml::El;

use super::Reader;
use super::names::{LOEXT_URI, Names, Ns};
use super::units;
use crate::error::OdgError;
use crate::write::fx::lo_scale_of;

/// The most rows of `FillGradient` DiaCraft draws (only the first ten count).
pub(crate) const MOST_ROWS: usize = 10;

/// The most `loext:gradient-stop` children of one definition the reader reads; those after them are left in the definition, which is held whole, and counted as kept.
pub(crate) const MOST_STOPS_READ: usize = 256;

/// The six layouts of `draw:style` (ODF 1.4 Part 3 §19.218.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Linear,
    Axial,
    Radial,
    Ellipsoid,
    Square,
    Rectangular,
}

impl Layout {
    /// The layout `draw:style` names: `linear` when it is absent (measured: LibreOffice draws a definition with no `draw:style` as a linear one), `None` for a word the standard does not have.
    pub(crate) fn of(text: Option<&str>) -> Option<Layout> {
        Some(match text {
            None | Some("linear") => Layout::Linear,
            Some("axial") => Layout::Axial,
            Some("radial") => Layout::Radial,
            Some("ellipsoid") => Layout::Ellipsoid,
            Some("square") => Layout::Square,
            Some("rectangular") => Layout::Rectangular,
            Some(_) => return None,
        })
    }
}

/// A `draw:gradient` as the file says it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Definition {
    pub layout: Layout,
    pub start: Option<[u8; 3]>,
    pub end: Option<[u8; 3]>,
    /// Percentages, 100 when absent.
    pub start_intensity: f64,
    pub end_intensity: f64,
    /// `draw:border`, a percentage, 0 when absent.
    pub border: f64,
    /// `draw:angle` in degrees, 0 when absent.
    pub angle: f64,
    /// `draw:cx` and `draw:cy`, percentages from the top left, 0 when absent.
    pub cx: f64,
    pub cy: f64,
    /// The `loext:gradient-stop` children read: place and colour, sorted by place (stable).
    pub stops: Vec<(f64, [u8; 3])>,
}

/// The cells the reader gives a fill of the gradient: `FillGradientDir`, `FillGradientAngle` (radians, for the layouts that have one) and the rows of `FillGradient`, each a colour and a
/// place from 0 to 1, rising, at most [`MOST_ROWS`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StandIn {
    pub dir: u8,
    pub angle: Option<f64>,
    pub stops: Vec<([u8; 3], f64)>,
}

/// The place of a stop, `svg:offset`: a number, as LibreOffice writes it, **not cut to 0 to 1** (measured: a stop at -0.5 or 1.5 is drawn where it is, the ramp crossing 0 and 1 between
/// stops), and a number followed by `%` as the number itself (measured: LibreOffice 26.8.2.2 draws `25%` at the place 25, not 0.25).
fn offset(text: &str) -> Option<f64> {
    units::number_any(text.strip_suffix('%').unwrap_or(text))
}

/// **The `loext:gradient-stop` children of a `draw:gradient`** (LibreOffice's form of a gradient of more than two colours), in the order of their places (a stable sort, so
/// that two stops at one place are a hard edge in the order written). A stop is read when it is in LibreOffice's namespace, has a place and a colour `loext:color-value` of the type `rgb` (or
/// of no type); any other is left in the definition, which is held whole, and counted as kept (measured: LibreOffice draws neither a stop of the type `theme` nor one with no colour). Past
/// [`MOST_STOPS_READ`] stops the rest are kept the same way. Each stop read is charged as an item before it is stored.
pub(crate) fn read_stops<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<Vec<(f64, [u8; 3])>, OdgError> {
    let mut stops: Vec<(f64, [u8; 3])> = Vec::new();
    for child in el.elements() {
        r.saw_element();
        let mark = names.enter(child);
        if names.element(child) == (Ns::Other, "gradient-stop") && names.uri_of(&child.name) == Some(LOEXT_URI) {
            let attrs = names.attrs(child);
            r.saw_attrs(attrs.len());
            let loext = |local: &str| {
                attrs.iter().find(|a| a.ns == Ns::Other && a.local == local && names.uri_of(a.name) == Some(LOEXT_URI)).map(|a| a.value)
            };
            let place = attrs.peek(Ns::Svg, "offset").and_then(offset);
            let colour = loext("color-type").is_none_or(|t| t == "rgb").then(|| loext("color-value").and_then(units::rgb)).flatten();
            match (place, colour) {
                (Some(at), Some(c)) if stops.len() < MOST_STOPS_READ => {
                    r.take_items(1)?;
                    stops.push((at, c));
                }
                _ => r.keep("gradient_stop"),
            }
        }
        names.leave(mark);
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(stops)
}

/// A colour of the ramp, its channels as numbers so that a blend is rounded once.
type Rgb = [f64; 3];

fn rgb_of(c: [u8; 3]) -> Rgb {
    [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])]
}

/// A channel rounded to a byte.
fn byte(v: f64) -> u8 {
    if v.is_finite() { v.round().clamp(0.0, 255.0) as u8 } else { 0 }
}

/// The colour a list of (place, colour), sorted by place, has at `x`: the first colour before the first place, the last after the last, and the straight blend between two (the later of two
/// stops at one place starts the next span, so they are a hard edge).
fn colour_at(list: &[(f64, Rgb)], x: f64) -> Rgb {
    let (Some(first), Some(last)) = (list.first(), list.last()) else { return [0.0; 3] };
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    for pair in list.windows(2) {
        if let [(a, ca), (b, cb)] = pair
            && *a <= x
            && x < *b
        {
            let share = (x - a) / (b - a);
            return [ca[0] + (cb[0] - ca[0]) * share, ca[1] + (cb[1] - ca[1]) * share, ca[2] + (cb[2] - ca[2]) * share];
        }
    }
    last.1
}

/// Where a centre percentage is, by thirds: 0 for the near edge, 1 for the middle, 2 for the far edge.
pub(crate) fn third(percent: f64) -> u8 {
    if percent < 25.0 {
        0
    } else if percent > 75.0 {
        2
    } else {
        // The middle third, and a number that is no number (the reader gives none: a percentage is a decimal).
        1
    }
}

/// `FillGradientDir` for a layout with a centre at (`cx`, `cy`) percent of the box from its top left (the origins; see the module's documentation).
pub(crate) fn dir_of(layout: Layout, cx: f64, cy: f64) -> u8 {
    let rectangular = matches!(layout, Layout::Square | Layout::Rectangular);
    match (third(cx), third(cy), rectangular) {
        (1, 1, false) | (0 | 2, 1, false) => 3,
        (1, 1, true) | (0 | 2, 1, true) => 10,
        (1, 2, _) => 4,
        (1, 0, _) => 5,
        (0, 0, false) => 7,
        (2, 0, false) => 6,
        (0, 2, false) => 2,
        (2, 2, false) => 1,
        (0, 0, true) => 12,
        (2, 0, true) => 11,
        (0, 2, true) => 9,
        _ => 8,
    }
}

/// `FillGradientAngle` for a `draw:angle` of `degrees`: 90 degrees less it (against), from 0 up to a whole turn, in radians.
pub(crate) fn angle_of(degrees: f64) -> f64 {
    let turned = (90.0 - degrees).rem_euclid(360.0);
    // `rem_euclid` of a number just under 0 may round up to the whole turn itself.
    let turned = if turned.is_finite() && turned < 360.0 { turned } else { 0.0 };
    turned.to_radians()
}

/// The ramp of the definition: its stops (or the two colours), each scaled by its intensity, cut to the places 0 to 1 with a stop at each (the colour there).
fn ramp_of(d: &Definition) -> Vec<(f64, Rgb)> {
    let black = [0u8; 3];
    let raw: Vec<(f64, [u8; 3])> =
        if d.stops.is_empty() { vec![(0.0, d.start.unwrap_or(black)), (1.0, d.end.unwrap_or(black))] } else { d.stops.clone() };
    let (si, ei) = ((d.start_intensity / 100.0).clamp(0.0, 1.0), (d.end_intensity / 100.0).clamp(0.0, 1.0));
    let scaled: Vec<(f64, Rgb)> = raw
        .iter()
        .map(|(o, c)| {
            let k = si + (ei - si) * o.clamp(0.0, 1.0);
            let c = rgb_of(*c);
            (*o, [c[0] * k, c[1] * k, c[2] * k])
        })
        .collect();
    let mut ramp: Vec<(f64, Rgb)> = Vec::with_capacity(scaled.len() + 2);
    if scaled.first().is_none_or(|(o, _)| *o != 0.0) {
        ramp.push((0.0, colour_at(&scaled, 0.0)));
    }
    ramp.extend(scaled.iter().filter(|(o, _)| (0.0..=1.0).contains(o)).copied());
    if scaled.last().is_none_or(|(o, _)| *o != 1.0) {
        ramp.push((1.0, colour_at(&scaled, 1.0)));
    }
    ramp
}

/// The stand-in of a definition. See the module's documentation.
pub(crate) fn stand_in(d: &Definition) -> StandIn {
    let b = (d.border / 100.0).clamp(0.0, 1.0);
    let ramp = ramp_of(d);
    let (dir, angle, mut rows): (u8, Option<f64>, Vec<(f64, Rgb)>) = match d.layout {
        Layout::Linear => (0, Some(angle_of(d.angle)), ramp.iter().map(|(o, c)| (b + o * (1.0 - b), *c)).collect()),
        Layout::Axial => {
            // From one edge to the axis, then from the axis to the other edge: the stop on the axis once.
            let half = |o: f64| 0.5 * o * (1.0 - b);
            let mut rows: Vec<(f64, Rgb)> = ramp.iter().rev().map(|(o, c)| (0.5 - half(*o), *c)).collect();
            rows.extend(ramp.iter().skip(1).map(|(o, c)| (0.5 + half(*o), *c)));
            (0, Some(angle_of(d.angle)), rows)
        }
        Layout::Radial | Layout::Ellipsoid | Layout::Square | Layout::Rectangular => {
            let dir = dir_of(d.layout, d.cx, d.cy);
            let k = lo_scale_of(dir);
            let r = |o: f64| if d.layout == Layout::Radial { (1.0 - b) - o * (1.0 - b) * (1.0 - b) } else { (1.0 - b) * (1.0 - o) };
            (dir, None, ramp.iter().rev().map(|(o, c)| (r(*o) / k, *c)).collect())
        }
    };
    // The rows run from DiaCraft's place 0 to 1, the colour of each end held to it (the start colour's border, the radial core).
    if let Some(&(at, c)) = rows.first()
        && at > 0.0
    {
        rows.insert(0, (0.0, c));
    }
    if let Some(&(at, c)) = rows.last()
        && at < 1.0
    {
        rows.push((1.0, c));
    }
    StandIn { dir, angle, stops: thin(rows) }
}

/// At most [`MOST_ROWS`] rows: the rows without the two that only hold an end, when that is enough, or else the colours at ten places evenly from 0 to 1.
fn thin(rows: Vec<(f64, Rgb)>) -> Vec<([u8; 3], f64)> {
    let out =
        |list: &[(f64, Rgb)]| list.iter().map(|(at, c)| ([byte(c[0]), byte(c[1]), byte(c[2])], super::cells::round9(at.clamp(0.0, 1.0)))).collect();
    if rows.len() <= MOST_ROWS {
        return out(&rows);
    }
    // An end held by a row of its own is held by the renderer as well: the first row at 0 with the next one's colour after it, and the last at 1 with the one before's.
    let held_first = matches!(rows.as_slice(), [(at, c), (next, nc), ..] if *at == 0.0 && *next > 0.0 && c == nc);
    let held_last = matches!(rows.as_slice(), [.., (before, bc), (at, c)] if *at == 1.0 && *before < 1.0 && c == bc);
    let (from, to) = (usize::from(held_first), rows.len().saturating_sub(usize::from(held_last)));
    let inner: Vec<(f64, Rgb)> = rows.get(from..to).map(<[(f64, Rgb)]>::to_vec).unwrap_or_default();
    if inner.len() <= MOST_ROWS {
        return out(&inner);
    }
    let last = (MOST_ROWS - 1) as f64;
    let sampled: Vec<(f64, Rgb)> = (0..MOST_ROWS).map(|i| i as f64 / last).map(|at| (at, colour_at(&rows, at))).collect();
    out(&sampled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(layout: Layout) -> Definition {
        Definition {
            layout,
            start: Some([255, 0, 0]),
            end: Some([0, 0, 255]),
            start_intensity: 100.0,
            end_intensity: 100.0,
            border: 0.0,
            angle: 0.0,
            cx: 50.0,
            cy: 50.0,
            stops: Vec::new(),
        }
    }

    fn rows(s: &StandIn) -> Vec<(String, f64)> {
        s.stops.iter().map(|(c, at)| (crate::read::units::hex(*c), *at)).collect()
    }

    fn row(c: &str, at: f64) -> (String, f64) {
        (c.to_owned(), at)
    }

    #[test]
    fn a_linear_gradient_runs_from_its_start_along_ninety_degrees_less_its_angle() {
        let s = stand_in(&def(Layout::Linear));
        assert_eq!((s.dir, s.angle), (0, Some(std::f64::consts::FRAC_PI_2)), "0 runs from the top down: the 90 degrees");
        assert_eq!(rows(&s), [row("#FF0000", 0.0), row("#0000FF", 1.0)]);
        let turned = stand_in(&Definition { angle: 90.0, ..def(Layout::Linear) });
        assert_eq!(turned.angle, Some(0.0), "90 starts at the left: DiaCraft's 0");
        let back = stand_in(&Definition { angle: -45.0, ..def(Layout::Linear) });
        assert!((back.angle.unwrap() - 135f64.to_radians()).abs() < 1e-12, "-45 starts at the top right: 135 degrees clockwise from the right");
        // A border of the start colour, as the spec's V25 has it.
        let border = stand_in(&Definition { border: 20.0, ..def(Layout::Linear) });
        assert_eq!(rows(&border), [row("#FF0000", 0.0), row("#FF0000", 0.2), row("#0000FF", 1.0)]);
    }

    #[test]
    fn the_angle_is_a_turn_from_zero_up_to_a_whole_turn() {
        for (degrees, want) in [(0.0, 90.0), (90.0, 0.0), (180.0, 270.0), (450.0, 0.0), (-270.0, 0.0), (89.5, 0.5), (1e300, 0.0)] {
            let got = angle_of(degrees).to_degrees();
            assert!((0.0..360.0).contains(&got), "{degrees}: {got}");
            if degrees < 1e10 {
                assert!((got - want).abs() < 1e-9, "{degrees}: {got}, not {want}");
            }
        }
    }

    #[test]
    fn an_axial_gradient_has_the_start_colour_on_its_axis_and_a_border_of_the_end_colour() {
        let s = stand_in(&def(Layout::Axial));
        assert_eq!(rows(&s), [row("#0000FF", 0.0), row("#FF0000", 0.5), row("#0000FF", 1.0)]);
        let border = stand_in(&Definition { border: 30.0, ..def(Layout::Axial) });
        assert_eq!(rows(&border), [row("#0000FF", 0.0), row("#0000FF", 0.15), row("#FF0000", 0.5), row("#0000FF", 0.85), row("#0000FF", 1.0)]);
        // The stops from the axis out, each twice.
        let three = stand_in(&Definition { stops: vec![(0.0, [255, 0, 0]), (0.25, [0, 255, 0]), (1.0, [0, 0, 255])], ..def(Layout::Axial) });
        assert_eq!(rows(&three), [row("#0000FF", 0.0), row("#00FF00", 0.375), row("#FF0000", 0.5), row("#00FF00", 0.625), row("#0000FF", 1.0)]);
    }

    #[test]
    fn a_centred_gradient_has_the_end_colour_at_its_origin() {
        for (layout, dir) in [(Layout::Radial, 3), (Layout::Ellipsoid, 3), (Layout::Square, 10), (Layout::Rectangular, 10)] {
            let s = stand_in(&def(layout));
            assert_eq!((s.dir, s.angle), (dir, None), "{layout:?}");
            assert_eq!(rows(&s), [row("#0000FF", 0.0), row("#FF0000", 1.0)], "{layout:?}");
        }
        // A radial border: the start colour outside 0.7 and a core of the end colour inside 0.21 (measured).
        let border = stand_in(&Definition { border: 30.0, ..def(Layout::Radial) });
        assert_eq!(rows(&border), [row("#0000FF", 0.0), row("#0000FF", 0.21), row("#FF0000", 0.7), row("#FF0000", 1.0)]);
        let rect = stand_in(&Definition { border: 30.0, ..def(Layout::Rectangular) });
        assert_eq!(rows(&rect), [row("#0000FF", 0.0), row("#FF0000", 0.7), row("#FF0000", 1.0)]);
    }

    #[test]
    fn a_centre_is_taken_to_the_nearest_origin_and_its_places_to_the_writers_scale() {
        let at = |layout: Layout, cx: f64, cy: f64| dir_of(layout, cx, cy);
        assert_eq!(
            [at(Layout::Radial, 0.0, 0.0), at(Layout::Radial, 100.0, 0.0), at(Layout::Radial, 0.0, 100.0), at(Layout::Radial, 100.0, 100.0)],
            [7, 6, 2, 1]
        );
        assert_eq!(
            [
                at(Layout::Rectangular, 0.0, 0.0),
                at(Layout::Square, 100.0, 0.0),
                at(Layout::Rectangular, 0.0, 100.0),
                at(Layout::Square, 100.0, 100.0)
            ],
            [12, 11, 9, 8]
        );
        assert_eq!(
            [
                at(Layout::Ellipsoid, 50.0, 100.0),
                at(Layout::Ellipsoid, 50.0, 0.0),
                at(Layout::Rectangular, 50.0, 100.0),
                at(Layout::Square, 50.0, 0.0)
            ],
            [4, 5, 4, 5]
        );
        assert_eq!(
            [at(Layout::Radial, 0.0, 50.0), at(Layout::Radial, 100.0, 50.0), at(Layout::Rectangular, 0.0, 50.0), at(Layout::Radial, 40.0, 60.0)],
            [3, 3, 10, 3]
        );
        assert_eq!([at(Layout::Radial, 24.9, 75.1), at(Layout::Radial, 25.0, 75.0), at(Layout::Radial, -5.0, 300.0)], [2, 3, 2]);
        // From a corner the file's gradient ends at half the way: the places are halved.
        let corner = stand_in(&Definition { cx: 0.0, cy: 0.0, ..def(Layout::Radial) });
        assert_eq!(corner.dir, 7);
        assert_eq!(rows(&corner), [row("#0000FF", 0.0), row("#FF0000", 0.5), row("#FF0000", 1.0)]);
    }

    #[test]
    fn the_stops_are_the_ramp_and_their_places_outside_zero_to_one_are_cut_where_the_ramp_crosses_them() {
        let stops = vec![(-0.5, [255, 0, 0]), (0.5, [0, 255, 0]), (1.5, [0, 0, 255])];
        let s = stand_in(&Definition { stops, ..def(Layout::Linear) });
        assert_eq!(rows(&s), [row("#808000", 0.0), row("#00FF00", 0.5), row("#008080", 1.0)]);
        // A first stop after 0 holds its colour before it; two stops at one place are a hard edge.
        let late = stand_in(&Definition {
            stops: vec![(0.25, [255, 0, 0]), (0.5, [255, 0, 0]), (0.5, [0, 0, 255]), (0.75, [0, 0, 255])],
            ..def(Layout::Linear)
        });
        assert_eq!(
            rows(&late),
            [row("#FF0000", 0.0), row("#FF0000", 0.25), row("#FF0000", 0.5), row("#0000FF", 0.5), row("#0000FF", 0.75), row("#0000FF", 1.0)]
        );
    }

    #[test]
    fn the_intensities_darken_the_colours_from_the_start_to_the_end() {
        let s = stand_in(&Definition { start_intensity: 50.0, end_intensity: 50.0, ..def(Layout::Linear) });
        assert_eq!(rows(&s), [row("#800000", 0.0), row("#000080", 1.0)]);
        let three = stand_in(&Definition {
            start_intensity: 100.0,
            end_intensity: 0.0,
            stops: vec![(0.0, [255, 0, 0]), (0.5, [0, 255, 0]), (1.0, [0, 0, 255])],
            ..def(Layout::Linear)
        });
        assert_eq!(rows(&three), [row("#FF0000", 0.0), row("#008000", 0.5), row("#000000", 1.0)]);
    }

    #[test]
    fn more_than_ten_rows_are_ten_places_along_the_gradient() {
        let stops: Vec<(f64, [u8; 3])> = (0..=20).map(|i| (f64::from(i) / 20.0, [(i * 12) as u8, 0, 0])).collect();
        let s = stand_in(&Definition { stops, ..def(Layout::Linear) });
        assert_eq!(s.stops.len(), MOST_ROWS);
        assert_eq!(s.stops.first().map(|r| r.1), Some(0.0));
        assert_eq!(s.stops.last().map(|r| r.1), Some(1.0));
        assert!(s.stops.windows(2).all(|w| w[0].1 < w[1].1), "rising places: {:?}", s.stops);
        // Nine stops with a border's two held ends are nine rows, not ten resampled.
        let nine: Vec<(f64, [u8; 3])> = (0..9).map(|i| (f64::from(i) / 8.0, [0, (i * 30) as u8, 0])).collect();
        let s = stand_in(&Definition { stops: nine.clone(), border: 10.0, ..def(Layout::Axial) });
        assert!(s.stops.len() <= MOST_ROWS, "{:?}", s.stops);
        let s = stand_in(&Definition { stops: nine, border: 50.0, ..def(Layout::Linear) });
        assert_eq!(s.stops.len(), 10, "nine stops and the border's held start: {:?}", s.stops);
    }

    #[test]
    fn a_definition_with_no_colours_is_black_and_the_places_stay_in_zero_to_one() {
        let s = stand_in(&Definition { start: None, end: None, border: 100.0, ..def(Layout::Radial) });
        assert!(s.stops.iter().all(|(c, at)| *c == [0, 0, 0] && (0.0..=1.0).contains(at)), "{:?}", s.stops);
        let s = stand_in(&Definition { border: 1e300, angle: -1e300, ..def(Layout::Axial) });
        assert!(s.stops.iter().all(|(_, at)| (0.0..=1.0).contains(at)) && s.angle.is_some_and(f64::is_finite), "{s:?}");
    }
}
