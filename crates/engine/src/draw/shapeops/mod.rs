//! Shape operations: Union, Combine, Fragment, Intersect and Subtract, Join and Trim, and Offset and Fit Curve.
//!
//! **What they do.** Each builds new shapes out of the geometry of the shapes named, or, for Subtract and Fit Curve, changes the shapes named in place (Subtract the first
//! one only), and Offset keeps the originals. The areas the shapes' paths fill
//! are worked out by `craft-pathops` (the path library the Craft apps share: curve-preserving booleans on `kurbo::BezPath`, with refusals and counted limits);
//! what comes out is written back as `Geometry` rows by [`rows`], with the frame of R9, or for Combine carried row by row ([`carry`]).
//!
//! **Join and Trim work on paths, not areas.** [`runs`] reads each operand's stroke as paths (one for each `MoveTo`); [`join`] chains the ones whose ends meet;
//! [`trim`] cuts them where they cross or touch, with [`snap`] moving the end of an open path within the closure tolerance onto the path it touches and
//! [`ops::trim_pieces`] asking the library's Outline for the stretches; [`rows::section_of_run`] writes a path as a section. They are bounded as the others are: the
//! reading is counted, the library's own limits hold, and what they add (the steps of the end search, the cap of 10,000 shapes) is counted and refused in words.
//!
//! **Offset and Fit Curve work on paths too** and share [`runs`] and its writer of rows (with `ArcTo` for the arcs of Fit Curve): [`offset`] is Offset, a
//! stroke outline cut at its caps for an open path and the library's offset for the closed ones of a section, each worked on near its own origin; [`fit`] is Fit Curve
//! The library's refit of every stretch of every shape in one call, arcs found in the input for the circles flag, the rows of a shape changed in place.
//!
//! **Where the conversion lives.** Here in `draw`, beside the commands, and not in `geom` (layer 0): reading a shape's geometry needs its sheet, its master
//! and its poses (the document model, layer 1, and the renderer's reader of a row's cells), and writing it needs the cells' own helpers. The paths and the
//! numbers are plain kurbo values at every step in between ([`ops`] takes and gives `BezPath`s and knows nothing of a document), so the pure part can move to
//! the layer-2 `pathops` crate of the architecture table when a second user needs it.
//!
//! **The steps of a command**, in order, so that a refusal changes nothing (`Session::edit` works on a copy and a refusal leaves the document as it was):
//!
//! 1. [`operand::validate`]: the shapes named exist, share one parent and each meets what the command needs. Nothing is built yet.
//! 2. [`operand::read`]: each operand's shown `Geometry` sections become paths in the **container's** coordinates, which are the page's for a shape on the page
//!    and the group's own frame for a member (a shape's own pose carries its rows there, the groups round it are not part of it). The work is counted
//!    against [`Budget`] before any section is merged or built, so a shape that inherits a huge master costs a count and not a merge.
//! 3. [`ops`]: the path library, in a frame near the origin (the container's coordinates less the middle of what is worked on, so that a small shape far out is
//!    no harder than a small shape near the origin), its refusals mapped to words.
//! 4. [`rows`] writes the loops as sections and works out the frame; [`look`] takes the first shape's text and formatting.
//! 5. [`apply`]: the originals go by the engine's own delete (so glue, guides and jumps are settled as a delete settles them), the produced shapes are put
//!    at the front of the container, what reads them is calculated, and the result is the one undo step the caller's edit makes.
//!
//! **Our choices** that the specification leaves open are in `docs/editing-commands.md` ("Shape operations") and listed in
//! the check list.

mod apply;
mod carry;
mod fit;
mod join;
mod look;
mod offset;
mod operand;
mod ops;
mod rows;
mod runs;
mod snap;
mod ties;
mod trim;

pub use apply::{Made, check, run, run_with};
pub use operand::MAX_OPERANDS;

/// What the drawing tools (`draw/pen.rs`) share with Combine's writer: a row of any kind in absolute coordinates, written as the shares of a box (`carry::row_of`), and read
/// back from a shape's rows (`carry::carry`).
pub(crate) use carry::{CRow, NurbsKnots, carry, row_of};
pub(crate) use rows::{Frame, flagged};

/// The nine operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// One shape with the outline of all.
    Union,
    /// One shape with a section for each section of each original.
    Combine,
    /// One shape for each region of the shapes laid together.
    Fragment,
    /// One shape for the area every operand covers.
    Intersect,
    /// The first shape less the area the others cover.
    Subtract,
    /// One shape holding the paths of all, those that abut chained into one section.
    Join,
    /// One shape for each stretch of the paths between the places they cross or touch.
    Trim,
    /// Parallel paths on each side of each path, the originals kept.
    Offset,
    /// Fewer segments within a tolerance, each shape changed in place.
    FitCurve,
}

/// What a command is given besides the shapes: the distance of Offset, and the tolerance and flags of Fit Curve. The others read none of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Args {
    /// Offset's distance, in the cells' own units (inches on a page of the default scale): positive.
    pub distance: f64,
    /// Fit Curve's tolerance, in the same units: how far the new paths may stray from the old ones. 0 asks for an exact match.
    pub tolerance: f64,
    /// Fit Curve's flags: a sum of [`Args::PERIODIC`], [`Args::DO_CIRCLES`] and [`Args::ABRUPT`].
    pub flags: u32,
}

impl Args {
    /// `visSplinePeriodic`: a closed path is fitted as a loop, so the seam where it closes is not forced to be a vertex.
    pub const PERIODIC: u32 = 1;
    /// `visSplineDoCircles`: circular arcs for what is found to be circular, and not splines.
    pub const DO_CIRCLES: u32 = 2;
    /// `visSplineAbrupt`: a spline is broken wherever the direction changes abruptly.
    pub const ABRUPT: u32 = 4;
    /// The three together: the flags are a sum of some of them, so they are at most this.
    pub const ALL: u32 = 7;

    /// Whether the arguments are ones `op` can run with, and if not why not in words: Offset's distance is positive, finite and no smaller than [`MIN_OFFSET`] and no larger
    /// than [`MAX_OFFSET`], Fit Curve's tolerance is not negative, finite and no larger than [`MAX_REACH`], and its flags a sum of the three. The other operations
    /// take none.
    pub(crate) fn check(&self, op: Op) -> Result<(), crate::draw::Problem> {
        use crate::draw::Problem;
        match op {
            Op::Offset => {
                if !self.distance.is_finite() || self.distance <= 0.0 {
                    return Err(Problem::bad("Offset needs a distance above 0 (a number that is not finite, 0 and less are refused)"));
                }
                if self.distance < MIN_OFFSET {
                    return Err(Problem::bad(format!(
                        "Offset needs a distance of at least {MIN_OFFSET}: a result is accurate to 1E-4, so a nearer copy cannot be told from the shape"
                    )));
                }
                if self.distance > MAX_OFFSET {
                    return Err(Problem::bad(format!(
                        "Offset needs a distance of at most {MAX_OFFSET} (a copy of a path is a stroke of twice the distance, and a stroke more than {MAX_WORK_EXTENT} across is too large for the path library)"
                    )));
                }
            }
            Op::FitCurve => {
                if !self.tolerance.is_finite() || self.tolerance < 0.0 {
                    return Err(Problem::bad("Fit Curve needs a tolerance of 0 or more (0 asks for an exact match)"));
                }
                if self.tolerance > MAX_REACH {
                    return Err(Problem::bad(format!("Fit Curve needs a tolerance of at most {MAX_REACH}")));
                }
                if self.flags > Args::ALL {
                    return Err(Problem::bad(
                        "Fit Curve's flags are 0 or a sum of 1 (periodic splines), 2 (circular arcs) and 4 (break at abrupt changes)",
                    ));
                }
            }
            Op::Union | Op::Combine | Op::Fragment | Op::Intersect | Op::Subtract | Op::Join | Op::Trim => {}
        }
        Ok(())
    }
}

impl Op {
    /// The command's id.
    pub const fn id(self) -> &'static str {
        match self {
            Op::Union => "shape.union",
            Op::Combine => "shape.combine",
            Op::Fragment => "shape.fragment",
            Op::Intersect => "shape.intersect",
            Op::Subtract => "shape.subtract",
            Op::Join => "shape.join",
            Op::Trim => "shape.trim",
            Op::Offset => "shape.offset",
            Op::FitCurve => "shape.fit_curve",
        }
    }

    /// The fewest shapes the command takes: Fragment, Trim, Offset and Fit Curve take one, the others two.
    pub const fn fewest(self) -> usize {
        match self {
            Op::Fragment | Op::Trim | Op::Offset | Op::FitCurve => 1,
            Op::Union | Op::Combine | Op::Intersect | Op::Subtract | Op::Join => 2,
        }
    }

    /// The name the command is called by in a message.
    pub const fn name(self) -> &'static str {
        match self {
            Op::Union => "Union",
            Op::Combine => "Combine",
            Op::Fragment => "Fragment",
            Op::Intersect => "Intersect",
            Op::Subtract => "Subtract",
            Op::Join => "Join",
            Op::Trim => "Trim",
            Op::Offset => "Offset",
            Op::FitCurve => "Fit Curve",
        }
    }

    /// Whether every operand must have a fill area: Union, Intersect and Subtract do; Combine takes any path, and Fragment needs one operand with a fill
    /// area and takes paths of the others.
    pub(crate) const fn needs_fill_area(self) -> bool {
        matches!(self, Op::Union | Op::Intersect | Op::Subtract)
    }

    /// Whether the command deletes the operand at place `k` of the shapes named: Union, Combine, Fragment, Intersect, Join and Trim delete them all, Subtract all but the
    /// first, and Offset and Fit Curve none (Offset keeps the originals, R38, and Fit Curve changes them in place).
    pub(crate) const fn deletes(self, k: usize) -> bool {
        match self {
            Op::Union | Op::Combine | Op::Fragment | Op::Intersect | Op::Join | Op::Trim => true,
            Op::Subtract => k != 0,
            Op::Offset | Op::FitCurve => false,
        }
    }

    /// Whether the command changes the vertices of the operand at place `k`: Subtract's first shape, and every shape of Fit Curve.
    pub(crate) const fn edits_vertices(self, k: usize) -> bool {
        match self {
            Op::Subtract => k == 0,
            Op::FitCurve => true,
            Op::Union | Op::Combine | Op::Fragment | Op::Intersect | Op::Join | Op::Trim | Op::Offset => false,
        }
    }

    /// Whether a shape the command makes holds the first shape's text: Union, Combine, Intersect and Join do; Fragment, Trim and Offset make shapes with none.
    pub(crate) const fn gives_text(self) -> bool {
        matches!(self, Op::Union | Op::Combine | Op::Intersect | Op::Join)
    }
}

// ---- the limits (SHAPEOPS-CAPS) ------------------------------------------------------------------------------------------

/// The most shapes one command produces: a Fragment past it is refused. Ours, not the reference app's.
pub const MAX_PRODUCED: usize = 10_000;

/// The most sections, rows, bytes of constructor text, path elements and spline work one command reads from all its operands together. Each is charged before
/// the thing is merged or built (the sheet's own counts: `Resolver::rows_held`), so none is a cost that has already been paid when it is found to be too big.
/// The path library has its own limits on top (`craft_pathops::Limits`, its defaults, never raised here); these keep the reading in step with them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    pub sections: usize,
    pub rows: usize,
    pub bytes: usize,
    pub elements: usize,
    pub work: u64,
}

/// The most `Geometry` sections one shape may have, counted over the shape, its master and its styles before any is merged, to be an operand at all.
pub(crate) const MAX_SECTIONS_PER_SHAPE: usize = 1_024;

impl Budget {
    /// The whole budget of one command.
    pub(crate) const fn new() -> Budget {
        Budget {
            sections: 8_192,
            rows: 200_000,
            bytes: 1 << 20,
            // The library's own limit on the elements of one operation, in all its inputs (`craft_pathops::MAX_ELEMENTS`).
            elements: craft_pathops::MAX_ELEMENTS,
            work: 50_000_000,
        }
    }
}

/// The most cells and rows that one command writes of the first shape's formatting into the shapes it makes, counted as what each shape is given times the shapes made,
/// before any shape is built (SHAPEOPS-CAPS): the cells the first shape holds itself (up to the 97 of its families) and its frame's, and the rows and cells of its
/// formatting sections (its gradients and, where a command gives text, its text's rows). The sections are shared, so they are held once in memory, but a file written from
/// the drawing writes them for each shape and the cells are held by each. A plain shape with a gradient of five stops (three cells each) under 10,000 pieces is about
/// 400,000, and the recalculation that follows holds about a kilobyte for each cell, which a million cells fill.
pub(crate) const MAX_LOOK_UNITS: usize = 500_000;

/// The nearest an Offset's copy may be (SHAPEOPS-PRECISION): curved edges of a result are within 1E-4 of the exact one, so a copy nearer than that cannot be told from
/// the shape it copies. In the cells' own units. Ours, not the reference app's.
pub const MIN_OFFSET: f64 = 1.0e-4;

/// The farthest a coordinate, a distance or a tolerance may be from nothing, in the cells' own units: the path library's sweep takes coordinates up to about 7E13, and the
/// work near the origin that a command does (the middle of what is worked on moved to it) leaves this much to the shapes' size and to the arguments.
pub(crate) const MAX_REACH: f64 = 5.0e13;

/// The most a path library call may be given to work on, in the cells' own units, for Offset: the sweep's tolerance is 64 units of the last place of a coordinate, which is
/// under 1E-5 for coordinates up to 7E8 after the paths are moved to their own middle, and Offset works at 1,000 times the size, where the tolerance it is given (0.01) is
/// the same 1E-5. It is half the extent of a section and twice the distance, so shapes any distance apart (up to what the cells hold) are each worked on near its own
/// origin, and a distance is refused past half of it (the farthest copy is a stroke of twice the distance from the path).
pub(crate) const MAX_WORK_EXTENT: f64 = 7.0e8;

/// The farthest an Offset's copy may be, the same limit seen from the distance: a stroke of twice the distance has to be inside [`MAX_WORK_EXTENT`].
pub const MAX_OFFSET: f64 = MAX_WORK_EXTENT / 2.0;

// ---- the precision (SHAPEOPS-PRECISION) --------------------------------------------------------------------------------

/// The refit tolerance and sliver width handed to the path library: curved edges of a result stay within 1E-4 of the exact one, and this is a tenth of it.
pub(crate) const PRECISION: f64 = 1.0e-5;

/// Two points this close are one: a path's ends, the ends of paths that abut, and the vertices of a result. Units
/// are the cells' own, which are inches on a page of the default scale.
pub(crate) const MERGE: f64 = 1.0e-5;

/// A region of less area than this is dropped, in square cell units.
pub(crate) const MIN_AREA: f64 = 1.0e-8;
