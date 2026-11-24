//! The lines of the grid and the ruler's subdivisions that a drag can snap to.
//!
//! **Units.** The cells are lengths in the page's **drawing units** (`V` holds inches, and a grid line in these
//! cells shrinks on paper with the drawing). A line of the grid at `origin + n * pitch` drawing inches is at `(origin + n * pitch) / k` **paper
//! inches**, where `k` is the drawing inches to a paper inch (`DrawingScale` over `PageScale`, [`crate::draw::units_per_inch`]). Everything this
//! module hands out is in paper inches, the unit of the commands and of the window; the cells are read here, in drawing units, and divided once.
//! The window's grid divides nothing today (a known gap), so it draws a scaled drawing's grid at the wrong place; these are the right ones.
//!
//! **A fixed grid** (`XGridDensity` 0 with a spacing above 0) is at the origin plus whole multiples of the spacing at every zoom. A
//! spacing of 0 draws, and snaps to, no line. **A variable grid** is the project rule, taken in drawing inches (the unit the cells
//! hold): a pitch of 1/8 inch times a power of 2, the smallest that leaves the lines at least 16 screen points apart at density 8, 32 at density 4
//! and 64 at density 2 (any other density reads as 4) at the zoom, and never closer than a spacing above 0. The points are those the page has on
//! the screen, so the pitch depends on the zoom (`zoom` is screen points to a paper inch) and on the drawing's scale.
//!
//! **The ruler's subdivisions** follow the ruler the window draws: ticks at the ruler's zero (`XRulerOrigin`) plus whole
//! multiples of a pitch counted in the ruler's own unit (`DrawingScale`'s `U`), at least 8 screen points apart at density 32, 16 at 16 and 32 at
//! 8 (any other density reads as 16): halves from an eighth for an inch and what is made of inches, steps of 1, 2 and 5 times a power of ten for
//! every other unit.

use diacraft_doc::{CellAddr, SheetRef};
use diacraft_units::{Measure, Unit, parse_unit};

use crate::draw::Reader;

/// The smallest pitch of a variable grid, in drawing inches (1/8), which doubles until the lines are far enough apart.
const BASE_PITCH: f64 = 0.125;

/// The most doublings of the base pitch: a bound for the search, which a finite zoom ends far sooner.
const MAX_DOUBLINGS: u32 = 40;

/// The roots of the `Ruler & Grid` cells, which a page that holds none reads.
const ROOT_GRID_DENSITY: f64 = 8.0;
const ROOT_RULER_DENSITY: f64 = 32.0;

/// A relative slack for "at least this many points apart", so that a pitch that lands exactly on the limit (8 drawing inches at 1/4 inch to the
/// foot and 96 points to the inch) is not turned down by the last bit of a division.
const SLACK: f64 = 1.0e-9;

/// Which axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    /// The word a command gives it.
    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
        }
    }
}

/// The `Ruler & Grid` cells of a page, each axis as `(x, y)`, in drawing inches (`V`), as the page sheet holds them or the root does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridCells {
    pub density: (f64, f64),
    pub spacing: (f64, f64),
    pub origin: (f64, f64),
    pub ruler_density: (f64, f64),
    pub ruler_origin: (f64, f64),
    /// The unit the ruler counts in: `DrawingScale`'s `U` when it names a length, else the inch.
    pub ruler_unit: Unit,
}

impl Default for GridCells {
    fn default() -> Self {
        GridCells {
            density: (ROOT_GRID_DENSITY, ROOT_GRID_DENSITY),
            spacing: (0.0, 0.0),
            origin: (0.0, 0.0),
            ruler_density: (ROOT_RULER_DENSITY, ROOT_RULER_DENSITY),
            ruler_origin: (0.0, 0.0),
            ruler_unit: Unit::Inch,
        }
    }
}

impl GridCells {
    /// The cells of page `page` as the reader sees them: a cell the page does not hold reads the root's value.
    pub(crate) fn read(reader: &Reader<'_>, page: u32) -> GridCells {
        let sheet = SheetRef::Page(page);
        let root = GridCells::default();
        let get = |name: &str, fallback: f64| {
            if reader.is_set(&sheet, &CellAddr::direct(name)) {
                let n = reader.number(&sheet, name);
                if n.is_finite() { n } else { fallback }
            } else {
                fallback
            }
        };
        let ruler_unit = reader
            .resolver()
            .resolve(&sheet, &CellAddr::direct("DrawingScale"))
            .and_then(|found| found.value.cell.and_then(|c| c.unit.as_deref().map(str::to_owned)))
            .and_then(|u| parse_unit(&u))
            .filter(|u| u.measure() == Measure::Length)
            .unwrap_or(root.ruler_unit);
        GridCells {
            density: (get("XGridDensity", root.density.0), get("YGridDensity", root.density.1)),
            spacing: (get("XGridSpacing", root.spacing.0), get("YGridSpacing", root.spacing.1)),
            origin: (get("XGridOrigin", root.origin.0), get("YGridOrigin", root.origin.1)),
            ruler_density: (get("XRulerDensity", root.ruler_density.0), get("YRulerDensity", root.ruler_density.1)),
            ruler_origin: (get("XRulerOrigin", root.ruler_origin.0), get("YRulerOrigin", root.ruler_origin.1)),
            ruler_unit,
        }
    }

    fn pick<T: Copy>(pair: (T, T), axis: Axis) -> T {
        match axis {
            Axis::X => pair.0,
            Axis::Y => pair.1,
        }
    }

    /// The grid's lines on one axis at this zoom, in paper inches; `None` for a fixed grid with no spacing.
    ///
    /// `k` is the drawing inches to a paper inch and `zoom` the screen points to a paper inch.
    pub fn grid(&self, axis: Axis, k: f64, zoom: f64) -> Option<Marks> {
        let (density, spacing, origin) = (Self::pick(self.density, axis), Self::pick(self.spacing, axis), Self::pick(self.origin, axis));
        let spacing = if spacing.is_finite() && spacing > 0.0 { spacing } else { 0.0 };
        let pitch = if density.round() == 0.0 {
            // Density 0 is a fixed grid, which needs a spacing.
            (spacing > 0.0).then_some(spacing)?
        } else {
            let min_points = match density.round() as i64 {
                8 => 16.0,
                4 => 32.0,
                2 => 64.0,
                _ => 32.0,
            };
            let mut pitch = BASE_PITCH;
            for _ in 0..MAX_DOUBLINGS {
                if points_apart(pitch, k, zoom) >= min_points * (1.0 - SLACK) {
                    break;
                }
                pitch *= 2.0;
            }
            if spacing > pitch { spacing } else { pitch }
        };
        Marks::new(pitch, origin, k)
    }

    /// The ruler's subdivisions on one axis at this zoom, in paper inches.
    pub fn ruler(&self, axis: Axis, k: f64, zoom: f64) -> Option<Marks> {
        let (density, origin) = (Self::pick(self.ruler_density, axis), Self::pick(self.ruler_origin, axis));
        let min_points = match density.round() as i64 {
            32 => 8.0,
            16 => 16.0,
            8 => 32.0,
            _ => 16.0,
        };
        // The ruler's unit, in drawing inches; the pitch is counted in it.
        let per_unit = self.ruler_unit.base_per_unit();
        if !(per_unit.is_finite() && per_unit > 0.0) {
            return None;
        }
        let points_per_unit = points_apart(per_unit, k, zoom);
        if !(points_per_unit.is_finite() && points_per_unit > 0.0) {
            return None;
        }
        let least = min_points * (1.0 - SLACK) / points_per_unit;
        let pitch_units = if counts_in_halves(self.ruler_unit) {
            let mut pitch = BASE_PITCH;
            for _ in 0..MAX_DOUBLINGS {
                if pitch >= least {
                    break;
                }
                pitch *= 2.0;
            }
            pitch
        } else {
            ladder_at_least(least)
        };
        Marks::new(pitch_units * per_unit, origin, k)
    }
}

/// How many screen points a distance of `drawing_inches` is, at `k` drawing inches to a paper inch and `zoom` points to a paper inch.
fn points_apart(drawing_inches: f64, k: f64, zoom: f64) -> f64 {
    drawing_inches / k * zoom
}

/// The smallest of 1, 2 and 5 times a power of ten that is at least `x`; 1 for an `x` that is not above 0.
fn ladder_at_least(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let base = 10f64.powf(x.log10().floor().clamp(-9.0, 15.0));
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * base).find(|c| *c >= x * (1.0 - SLACK)).unwrap_or(10.0 * base)
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

/// Lines on one axis: at `origin + n * pitch` for whole `n`, in paper inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    /// The distance between two lines, in paper inches: more than 0 and finite.
    pub pitch: f64,
    /// Where a line falls, in paper inches.
    pub origin: f64,
}

impl Marks {
    /// Lines `pitch` drawing inches apart through `origin` drawing inches, in paper inches; `None` for numbers that make no line.
    fn new(pitch: f64, origin: f64, k: f64) -> Option<Marks> {
        let ok = |n: f64| n.is_finite();
        if !(ok(pitch) && ok(origin) && ok(k) && pitch > 0.0 && k > 0.0) {
            return None;
        }
        let marks = Marks { pitch: pitch / k, origin: origin / k };
        (marks.pitch.is_finite() && marks.pitch > 0.0 && marks.origin.is_finite()).then_some(marks)
    }

    /// The line nearest `v`, if there is a finite one.
    pub fn nearest(&self, v: f64) -> Option<f64> {
        if !v.is_finite() {
            return None;
        }
        let n = ((v - self.origin) / self.pitch).round();
        if !n.is_finite() || n.abs() > 1.0e12 {
            return None;
        }
        let line = self.origin + n * self.pitch;
        line.is_finite().then_some(line)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The drawing inches to a paper inch of the scales the task asks for, and a page's paper (Letter and A4) in inches.
    const SCALES: [(&str, f64); 3] = [("1:1", 1.0), ("1:100", 100.0), ("1/4 in = 1 ft", 48.0)];
    const LETTER: (f64, f64) = (11.0, 8.5);
    const A4: (f64, f64) = (11.693_307, 8.267_717);

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_variable_grid_at_one_to_one_is_what_the_window_draws() {
        // Density 8: 16 points apart at the least. At 96 points to an inch the eighth is 12, the quarter 24.
        let cells = GridCells::default();
        let x = cells.grid(Axis::X, 1.0, 96.0).unwrap();
        assert!(near(x.pitch, 0.25), "{x:?}");
        // Zoomed in to 8 times it is the eighth again.
        let x = cells.grid(Axis::X, 1.0, 768.0).unwrap();
        assert!(near(x.pitch, 0.125), "{x:?}");
        // Density 4: 32 points; density 2: 64; any other reads as 4.
        let at = |d: f64| GridCells { density: (d, d), ..GridCells::default() }.grid(Axis::X, 1.0, 96.0).unwrap().pitch;
        assert!(near(at(4.0), 0.5) && near(at(2.0), 1.0) && near(at(5.0), 0.5), "{} {} {}", at(4.0), at(2.0), at(5.0));
    }

    #[test]
    fn the_pitch_of_a_variable_grid_follows_the_drawings_scale_not_the_papers() {
        // The same 16 points on the screen is a bigger step in a drawing at 1:100: in drawing inches.
        let cells = GridCells::default();
        for (name, k) in SCALES {
            let marks = cells.grid(Axis::X, k, 96.0).unwrap();
            let drawing_pitch = marks.pitch * k;
            // The smallest eighth-times-a-power-of-two that is 16 points or more on the screen.
            assert!(marks.pitch * 96.0 >= 16.0 * (1.0 - 1e-9), "{name}: {} points", marks.pitch * 96.0);
            assert!(drawing_pitch / 2.0 / k * 96.0 < 16.0 * (1.0 - 1e-9) || drawing_pitch <= 0.125 + 1e-12, "{name}: the next smaller would do");
            let power = (drawing_pitch / 0.125).log2();
            assert!((power - power.round()).abs() < 1e-9, "{name}: {drawing_pitch} is an eighth times a power of two");
        }
        // The numbers: 16 points at 96 to the inch is 1/6 of a paper inch; 100 drawing inches to a paper inch, 16.7 drawing inches, so 32; at 48, 8.
        assert!(near(cells.grid(Axis::X, 100.0, 96.0).unwrap().pitch * 100.0, 32.0));
        assert!(
            near(cells.grid(Axis::X, 48.0, 96.0).unwrap().pitch * 48.0, 8.0),
            "8 drawing inches is exactly 16 points: the limit is not turned down"
        );
    }

    #[test]
    fn a_fixed_grid_is_at_the_origin_plus_multiples_of_the_spacing_whatever_the_zoom_and_the_scale() {
        // Origin 0.3, spacing 0.5, density 0: lines at 0.3, 0.8, 1.3 and -0.2 (drawing inches).
        let cells = GridCells { density: (0.0, 0.0), spacing: (0.5, 0.5), origin: (0.3, 0.3), ..GridCells::default() };
        for (name, k) in SCALES {
            for zoom in [10.0, 96.0, 4000.0] {
                let x = cells.grid(Axis::X, k, zoom).unwrap();
                assert!(near(x.pitch * k, 0.5), "{name} {zoom}");
                for line in [0.3, 0.8, 1.3, -0.2] {
                    let at = x.nearest(line / k).unwrap();
                    assert!(near(at * k, line), "{name} {zoom}: {line} -> {}", at * k);
                }
            }
        }
        // Per axis: the y lines use the y cells.
        let cells = GridCells { density: (0.0, 0.0), spacing: (0.5, 0.25), origin: (0.0, 0.1), ..GridCells::default() };
        assert!(near(cells.grid(Axis::Y, 1.0, 96.0).unwrap().pitch, 0.25));
        assert!(near(cells.grid(Axis::Y, 1.0, 96.0).unwrap().origin, 0.1));
    }

    #[test]
    fn a_fixed_grid_with_no_spacing_has_no_lines() {
        let cells = GridCells { density: (0.0, 0.0), spacing: (0.0, 0.0), ..GridCells::default() };
        assert!(cells.grid(Axis::X, 1.0, 96.0).is_none());
        let cells = GridCells { density: (0.0, 0.0), spacing: (f64::NAN, -1.0), ..GridCells::default() };
        assert!(cells.grid(Axis::X, 1.0, 96.0).is_none() && cells.grid(Axis::Y, 1.0, 96.0).is_none());
    }

    #[test]
    fn a_variable_grid_is_never_closer_than_its_spacing() {
        // Density 4 and 0.5: no closer than 0.5, however far the zoom goes in.
        let cells = GridCells { density: (4.0, 4.0), spacing: (3.0, 3.0), ..GridCells::default() };
        for zoom in [1.0, 96.0, 10_000.0] {
            let p = cells.grid(Axis::X, 1.0, zoom).unwrap().pitch;
            assert!(p >= 3.0 - 1e-12, "{zoom}: {p}");
        }
        // A spacing above the pitch the zoom asks for wins; below it, the zoom's pitch.
        assert!(near(cells.grid(Axis::X, 1.0, 96.0).unwrap().pitch, 3.0));
    }

    #[test]
    fn the_nearest_line_is_the_nearest_whatever_the_page_and_the_numbers() {
        let marks = Marks { pitch: 0.25, origin: 0.1 };
        assert!(near(marks.nearest(0.0).unwrap(), 0.1) && near(marks.nearest(0.2).unwrap(), 0.1) && near(marks.nearest(0.3).unwrap(), 0.35));
        assert!(near(marks.nearest(-0.2).unwrap(), -0.15));
        // Hostile numbers give nothing, not a panic or a nonsense line.
        assert_eq!(marks.nearest(f64::NAN), None);
        assert_eq!(marks.nearest(f64::INFINITY), None);
        assert_eq!(marks.nearest(1.0e300), None);
        assert!(Marks::new(0.0, 0.0, 1.0).is_none() && Marks::new(1.0, 0.0, 0.0).is_none() && Marks::new(f64::NAN, 0.0, 1.0).is_none());
        // Letter and A4 hold lines inside them whatever the scale.
        for (w, h) in [LETTER, A4] {
            for (_, k) in SCALES {
                let cells = GridCells::default();
                let x = cells.grid(Axis::X, k, 96.0).unwrap();
                let at = x.nearest(w / 2.0).unwrap();
                assert!((at - w / 2.0).abs() <= x.pitch / 2.0 + 1e-9);
                let y = cells.grid(Axis::Y, k, 96.0).unwrap();
                assert!((y.nearest(h / 2.0).unwrap() - h / 2.0).abs() <= y.pitch / 2.0 + 1e-9);
            }
        }
    }

    #[test]
    fn the_rulers_subdivisions_are_in_the_rulers_unit_from_its_zero() {
        // A metre ruler at 1:100: whole centimetres, whatever the origin; the zero is the ruler origin (drawing inches).
        let mm = Unit::Millimetre;
        let cells = GridCells { ruler_unit: mm, ruler_origin: (1.0, 0.0), ..GridCells::default() };
        let per_unit = mm.base_per_unit();
        let x = cells.ruler(Axis::X, 1.0, 96.0).unwrap();
        // 8 points apart at the least (density 32): one mm is 3.78 points, so 5 mm (18.9 points) is the first on the 1, 2, 5 ladder past 2.1.
        let units = x.pitch / per_unit;
        assert!(near(units, 2.0) || near(units, 5.0), "{units}");
        assert!(x.pitch * 96.0 >= 8.0 * (1.0 - 1e-9));
        // The zero falls on a tick: the origin is the ruler origin (1 drawing inch).
        assert!(near(x.nearest(1.0).unwrap(), 1.0));
        // In inches it halves from an eighth: at 96 points to the inch and 8 points, the eighth is 12.
        let cells = GridCells::default();
        assert!(near(cells.ruler(Axis::X, 1.0, 96.0).unwrap().pitch, 0.125));
        // At 1:100 in metres the ruler counts metres of drawing and the ticks are far apart on paper.
        let cells = GridCells { ruler_unit: Unit::Metre, ..GridCells::default() };
        let m = cells.ruler(Axis::X, 100.0 * 39.37007874, 96.0);
        assert!(m.is_some());
    }

    #[test]
    fn a_unit_that_makes_no_ruler_makes_none() {
        let cells = GridCells { ruler_unit: Unit::Unitless, ..GridCells::default() };
        // A unit with no length gives no usable pitch or a ladder one; never a panic.
        let _ = cells.ruler(Axis::X, 1.0, 96.0);
        let cells = GridCells::default();
        assert!(cells.ruler(Axis::X, 1.0, 0.0).is_none(), "no zoom, no points to count");
        assert!(cells.ruler(Axis::X, f64::NAN, 96.0).is_none());
        assert!(cells.grid(Axis::X, 0.0, 96.0).is_none());
    }
}
