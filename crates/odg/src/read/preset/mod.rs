//! Preset shape geometry for custom shapes whose `draw:type` is `ooxml-<name>` and that have no path.
//!
//! **Provenance.** Ported from DeckCraft (`storytold/deckcraft`, `crates/geom/src/preset/` and `preset.rs`, MIT OR Apache-2.0, commit
//! c4674f24fb11eda1cdb440253c88fac686cbbafe), the generators of the ECMA-376 preset shapes by their names, with the adjust values in the file
//! convention (ratios in 1/100,000, angles in 1/60,000 degree). What changed: the serde derives are gone (the types are never serialised
//! here), `kurbo` is the workspace's own, the tests that read the gallery are left out, and the module is read through [`geometry_of`],
//! which holds the adjust values to a range in which no preset's work grows (see below). DeckCraft's note on them stands: each outline is a
//! construction of its own from the look of the shape and the meaning of its adjust values, not a copy of the annex's formula tables, so a
//! preset's outline may differ from the annex's by the shape's fine detail. `docs/provenance.md` has the row.
//!
//! **Bounded work.** A preset is a fixed construction: its path has a fixed number of segments, and its arcs are cut into cubic segments by
//! `kurbo` to a tolerance, whose number grows with the sixth root of the arc's radius over the tolerance. [`geometry_of`] builds a preset in a
//! box of at most 1,000 design units on a side whatever the frame's size (the reader takes shares of it), and holds each adjust value to
//! [`MOST_ADJUST`] (ECMA-376's own adjust values reach a few hundred thousand, and an angle 21,600,000), so a hostile value cannot make a radius
//! of a size that cuts an arc into millions of segments. A value that is not finite is 0 (the generator does that itself).
#![allow(dead_code, clippy::cognitive_complexity, clippy::too_many_arguments, clippy::too_many_lines, clippy::many_single_char_names)]
#![allow(clippy::needless_range_loop, clippy::type_complexity, clippy::excessive_precision, clippy::manual_range_contains)]

use kurbo::{Arc, BezPath, Point, Rect, Vec2};

mod build;
mod builder;
mod catalog;
mod extra;
mod shapes;

use build::build;
use builder::{Corner, Dir, Pb, corner_rect, fill, fill_eo, fill_only, mid_sites, orient, shade, simple, stroke_only, vis};
use catalog::Handle;
#[cfg(test)]
use catalog::{CATALOG, info};
pub(crate) use catalog::{FillMode, Geometry};

/// The largest an adjust value may be: ECMA-376's own values stay under a million.
pub(crate) const MOST_ADJUST: f64 = 1.0e8;

/// The size of the box a preset is built in, on its longer side.
const DESIGN: f64 = 1_000.0;

/// The geometry of the preset `name` for a frame of `w` by `h` (any unit) with adjust values `adjust`, in a box whose longer side is
/// [`DESIGN`], and the box's size: a coordinate of the geometry over that size is the share of the frame. `None` for a name the gallery does
/// not hold, or a frame with no area.
pub(crate) fn geometry_of(name: &str, w: f64, h: f64, adjust: &[f64]) -> Option<(Geometry, f64, f64)> {
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let scale = DESIGN / w.max(h);
    let (dw, dh) = (w * scale, h * scale);
    let held: Vec<f64> = adjust.iter().take(32).map(|v| if v.is_finite() { v.clamp(-MOST_ADJUST, MOST_ADJUST) } else { 0.0 }).collect();
    build(name, dw, dh, &held).map(|g| (g, dw, dh))
}
