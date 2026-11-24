//! Shape operations: Union, Combine, Fragment, Intersect and Subtract.
//!
//! **What they do.** Each builds new shapes out of the geometry of the shapes named, or, for Subtract, changes the first one. The areas the shapes' paths fill
//! are worked out by `craft-pathops` (the path library the Craft apps share: curve-preserving booleans on `kurbo::BezPath`, with refusals and counted limits);
//! what comes out is written back as `Geometry` rows by [`rows`], with the frame of R9, or for Combine carried row by row ([`carry`]).
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
//! the check list. The reference app was not inspected, and our output has not been opened in it by the project.

mod apply;
mod carry;
mod look;
mod operand;
mod ops;
mod rows;
mod ties;

pub use apply::{Made, check, run};
pub use operand::MAX_OPERANDS;

/// What the drawing tools (`draw/pen.rs`) share with Combine's writer: a row of any kind in absolute coordinates, written as the shares of a box (`carry::row_of`), and read
/// back from a shape's rows (`carry::carry`).
pub(crate) use carry::{CRow, NurbsKnots, carry, row_of};
pub(crate) use rows::{Frame, flagged};

/// The five operations of this task.
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
        }
    }

    /// The fewest shapes the command takes: Fragment takes one, the others two.
    pub const fn fewest(self) -> usize {
        match self {
            Op::Fragment => 1,
            Op::Union | Op::Combine | Op::Intersect | Op::Subtract => 2,
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
        }
    }

    /// Whether every operand must have a fill area: Union, Intersect and Subtract do; Combine takes any path, and Fragment needs one operand with a fill
    /// area and takes paths of the others.
    pub(crate) const fn needs_fill_area(self) -> bool {
        matches!(self, Op::Union | Op::Intersect | Op::Subtract)
    }

    /// Whether the originals are deleted: every operation but Subtract deletes all of them; Subtract keeps the first shape and deletes the others.
    pub(crate) const fn keeps_first(self) -> bool {
        matches!(self, Op::Subtract)
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

// ---- the precision (SHAPEOPS-PRECISION) --------------------------------------------------------------------------------

/// The refit tolerance and sliver width handed to the path library: curved edges of a result stay within 1E-4 of the exact one, and this is a tenth of it.
pub(crate) const PRECISION: f64 = 1.0e-5;

/// Two points this close are one: a path's ends, the ends of paths that abut, and the vertices of a result. Units
/// are the cells' own, which are inches on a page of the default scale.
pub(crate) const MERGE: f64 = 1.0e-5;

/// A region of less area than this is dropped, in square cell units.
pub(crate) const MIN_AREA: f64 = 1.0e-8;
