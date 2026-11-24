//! The drawing core: what the editing commands do to a document, in the page's drawing units.
//!
//! Every function here edits a [`Document`] and nothing else, so a command makes one undo step of it (`Session::edit`), and a test or a
//! tool can build a page without the session. The commands (`cmd/shape.rs`, `cmd/connector.rs`) check their parameters, convert paper
//! inches to drawing units, and call these.
//!
//! * [`shapes`]: drop an instance of a master, move, resize, delete.
//! * [`arrange`]: rotate, flip, drawing order and align; [`scale`]: several shapes, or a group, resized as one.
//! * [`placement`] and [`line_ends`]: what the Size & Position window sends and reads: the pin, one size for several shapes, a line's ends and
//!   length, and where a shape is; [`place`]: an absolute place for the pins of many shapes.
//! * [`clipboard`]: copy, cut, paste and duplicate; [`group`]: group and ungroup; [`lift`] carries a shape's cells from one frame to another
//!   so that it draws where it did; [`remap`] gives a copy the `ID`s of its own in the formulas that name its shapes; [`scan`] walks a page
//!   once for the commands that take many `ID`s.
//! * [`local`]: a member that is an instance carries its master's `Geometry` itself, so that a reader that does not know the master draws it (
//!   kept); [`nested`]: it holds its `Width`, `Height`, `PinX` and `PinY` as formulas of its group, as a file's does.
//! * [`change`]: Change Shape: shapes swapped for another master, keeping their place, size, text, formatting, data and glue.
//! * [`shapeops`]: Union, Combine, Fragment, Intersect and Subtract, on the path library the Craft apps share.
//! * [`text_edit`]: the text of a shape: replace it, or a range of its characters with its runs and fields kept.
//! * [`text_box`]: the shape the Text tool makes, a box of text with no master; [`text_block`]: a shape's text block on its own, where it is, and
//!   moving, resizing and turning it.
//! * [`connectors`]: add a connector glued at its ends, move an end. **Glue** is a `Connect` row and a formula on the end cell that names
//!   the target's connection point (`PAR(PNT(Sheet.<id>!Connections.X<n>,Sheet.<id>!Connections.Y<n>))`), so a recalculation moves the end.
//! * [`route`]: the right-angle route, written as `Geometry` rows with values after each recalculation.
//! * [`settle`]: after an edit, the recalculation of what it changed and what reads it (through `calc`, not of the whole page), and the routes
//!   of the connectors it moved.
//! * [`page_setup`], [`page_fit`], [`pages`] and [`paper`]: a page's size, orientation, drawing scale, printer paper and Auto Size, the growth of
//!   a page that has it on, and deleting and reordering pages.
//!
//! **Why a command writes the route and no hook does.** A route is not a formula's result: it depends on the exit directions of the two
//! connection points and on a rule that later work replaces. If recalculation wrote it, every `calc::commit` (a file open, a cell edit
//! through some future command) would rewrite geometry the user may have changed by hand; and an undo step would hold a route that a
//! hook then recomputes. Here only the commands that move an end write rows, on exactly the connectors they moved, and an undo step
//! simply holds the rows that were there.

mod arrange;
mod autoconnect;
pub mod callout_preview;
pub mod callouts;
mod change;
mod clipboard;
pub mod comments;
mod connectors;
pub mod container;
pub mod fields;
pub mod find;
mod glue;
mod group;
mod hand;
mod jump_settings;
mod jumps;
mod label;
pub mod layers;
mod lift;
mod line_ends;
pub mod links;
mod local;
#[cfg(test)]
mod loops_cost;
mod nested;
mod odg_glue;
mod page_fit;
mod page_setup;
mod pages;
pub mod paper;
pub mod pen;
pub mod pictures;
mod place;
mod placement;
mod read;
mod refit;
pub mod relayout;
mod remap;
mod reroute;
pub mod route;
mod route_clear;
#[cfg(test)]
mod route_cost;
mod rows;
mod scale;
mod scan;
mod select;
mod settle;
pub mod shapedata;
mod shapeops;
mod shapes;
mod style;
mod text_block;
mod text_box;
mod text_edit;
pub(crate) mod ties;

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, CellWrite, DocError, Document, Owner, Shape, ShapeRef, SheetRef, WriteOrigin, WriteOutcome};

use crate::error::EngineError;

pub(crate) use arrange::angle_text;
pub use arrange::{Align, FlipAxis, Stack, Turn, Turned, align_shapes, flip_shapes, order_shapes, rotate_shapes, turn_shapes};
pub use autoconnect::{AutoConnected, DEFAULT_GAP, autoconnect, autoconnect_centre, autoconnect_join, neighbour};
pub use change::{
    Changed, Glued, MAX_CHANGED_SHAPES, MAX_DATA_ROWS, MAX_NEW_MEMBERS, MAX_RECONNECTED_ENDS, Reglued, Replacement, Skipped, change_shapes,
    swapping_locked,
};
pub use clipboard::{Clipboard, Pasted, Placement, copy_shapes, cut_shapes, duplicate_shapes, paste_shapes};
pub use connectors::{ConnectorInfo, End, Which, add_connector, add_connector_above, attach_connector, attach_connector_above, reconnect_connector};
pub use group::{Grouped, MAX_GROUP_DEPTH, MAX_UNGROUP_MEMBERS, Ungrouped, group_shapes, ungroup_shapes};
pub use hand::{ConnectorPath, HAND_EDITED, LegSubject, MAX_HAND_POINTS, connector_path, edit_leg, leg_subject, moved_route, reset_connectors};
pub use jump_settings::{JumpChange, LineJumpSettings, line_jump_settings, set_line_jumps};
pub use jumps::{MAX_JUMP_ROWS, MAX_PARTICIPANTS as MAX_JUMP_PARTICIPANTS};
pub use line_ends::{set_end, set_length};
pub use local::MAX_LOCALISED_WORK;
pub(crate) use local::{below, charge_all, localise_geometry};
pub(crate) use nested::tie_to_group;
pub use page_fit::{MAX_SHIFT_SHAPES, enlarge_page, fit_page};
pub use page_setup::{
    MAX_PAGE_INCHES, MIN_PAGE_INCHES, PageSetup, PaperChoice, PaperSetup, ScaleChoice, ScaleSetup, SizeChoice, copy_setup, length_unit, read_setup,
    set_auto_size, set_orientation, set_paper, set_scale, set_size,
};
pub use pages::{Deleted, Duplicated, Moved, Retyped, add_background_page, delete_page, duplicate_page, move_page, set_background, set_page_type};
pub use place::{Placed, place_shapes};
pub use placement::{Glue, LineEnd, LineFacts, PinAt, Place, ShapePlacement, pin_shapes, placements, size_shapes};
pub(crate) use read::Reader;
pub use read::{ConnectionPoint, Frame, Pose, connection_points, connection_points_within, shape_corners, shape_frame, units_per_inch};
pub use refit::{Left, MAX_LEFT_LISTED, MAX_OWN_POINT_ROWS, MAX_REFIT_MEMBERS, MAX_REROUTED, Refit, refit_groups};
pub(crate) use remap::{freeze_refs_of, sheet_refs};
pub use reroute::{MAX_CROSSING_POINT_ROWS, MAX_CROSSING_ROWS, Reroute, connector_reroute, set_reroute};
pub use scale::{MAX_SCALED_MEMBERS, Scaled, resize_shapes};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use scale::{carry as scale_carry, factor as scale_factor};
#[cfg(test)]
pub(crate) use scan::NODES_WALKED;
pub use scan::page_holding;
pub(crate) use scan::{Named, find_named, scan};
pub use select::{
    Choices, Kind as SelectKind, LayerInfo, LayerPick, MAX_SELECT_POINTS, MAX_SELECT_REGION_SHAPES, MAX_SELECT_TYPE_SHAPES, MAX_SELECT_WORK,
    MAX_SHAPE_POINTS, Picked, Role as SelectRole, TypeQuery, by_region, by_type, choices as select_choices,
};
pub(crate) use settle::EditRoutes;
pub use settle::{MAX_ROUTED_PER_EDIT, settle, settle_forced};
pub use shapeops::{MAX_OPERANDS as MAX_SHAPE_OPS_OPERANDS, MAX_PRODUCED as MAX_SHAPE_OPS_PRODUCED};
pub use shapeops::{Made, Op, check as check_shape_ops, run as shape_ops};
pub use shapes::{DropInfo, ShapesMoved, delete_shapes, drop_shape, drop_shape_above, move_shapes, move_shapes_leaving, resize_shape};
pub(crate) use shapes::{left_words, page_stats};
pub use style::{PathStyle, StyleChoice, page_style, set_connector_style, set_page_style};
pub(crate) use style::{path_style, style_of};
pub use text_block::{BlockPlace, move_text_block, resize_text_block, rotate_text_block, text_block_place};
pub use text_box::{TextBoxInfo, add_text_box_above};
pub use text_edit::{TextField, TextView, set_text, set_text_range, text_view};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use {
    change::glue_index,
    odg_glue::{axes_of, better_of, id_of},
    select::box_inside,
};

/// The farthest a coordinate or a size may be from the page's origin, in drawing units: past any page, short of overflowing arithmetic.
pub const MAX_COORD: f64 = 1.0e6;

/// The most characters of text one shape holds through `shape.set_text`.
pub const MAX_TEXT_CHARS: usize = 100_000;

/// Why an edit could not be made. Whatever the reason, the document is as it was (the caller edits a copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Whether the fault is in what the caller asked (bad parameters), and not in the document's state.
    pub bad_params: bool,
    pub message: String,
}

impl Problem {
    pub(crate) fn bad(message: impl Into<String>) -> Problem {
        Problem { bad_params: true, message: message.into() }
    }

    pub(crate) fn failed(message: impl Into<String>) -> Problem {
        Problem { bad_params: false, message: message.into() }
    }

    /// The error a command `cmd` reports for it.
    pub fn into_engine(self, cmd: &str) -> EngineError {
        if self.bad_params { crate::error::bad(cmd, self.message) } else { crate::error::failed(cmd, self.message) }
    }
}

impl From<DocError> for Problem {
    fn from(error: DocError) -> Problem {
        Problem::failed(error.to_string())
    }
}

/// A coordinate or size that is a number and within [`MAX_COORD`].
pub(crate) fn checked(name: &str, value: f64) -> Result<f64, Problem> {
    if value.is_finite() && value.abs() <= MAX_COORD {
        Ok(value)
    } else {
        Err(Problem::bad(format!("`{name}` must be a number within {MAX_COORD} of the page's origin")))
    }
}

/// The page, which must be there.
pub(crate) fn require_page(doc: &Document, page: u32) -> Result<(), Problem> {
    doc.page(page).map(|_| ()).ok_or_else(|| Problem::bad(format!("there is no page {page}")))
}

/// The path of the shape `id` on a page, which must be there.
pub(crate) fn require_shape(doc: &Document, page: u32, id: u32) -> Result<ShapeRef, Problem> {
    require_page(doc, page)?;
    doc.find_shape(Owner::Page(page), id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))
}

/// The shape with this `ID` on a page, or why there is none: for the commands that ask the engine about a shape before they edit it.
pub fn require_shape_ref(doc: &Document, page: u32, id: u32) -> Result<ShapeRef, Problem> {
    require_shape(doc, page, id)
}

/// The page, to change.
pub(crate) fn page_mut(doc: &mut Document, page: u32) -> Option<&mut diacraft_doc::Page> {
    let at = doc.pages.iter().position(|p| p.id == page)?;
    doc.pages.get_mut(at).map(Arc::make_mut)
}

/// The shape at `path` on a page, to change: every `Arc` on the way is made unique first, so older copies of the document keep what they had.
pub(crate) fn shape_mut<'a>(doc: &'a mut Document, page: u32, path: &[u32]) -> Option<&'a mut Shape> {
    let at = doc.pages.iter().position(|p| p.id == page)?;
    let page = Arc::make_mut(doc.pages.get_mut(at)?);
    let (first, rest) = path.split_first()?;
    let mut shape = Arc::make_mut(page.shapes.get_mut(usize::try_from(*first).ok()?)?);
    for step in rest {
        shape = Arc::make_mut(shape.children.get_mut(usize::try_from(*step).ok()?)?);
    }
    Some(shape)
}

/// Writes cells as a drag in the window does (`WriteOrigin::Drawing`): all or nothing, and a cell a `GUARD` protects is an error and not a
/// silent skip (reports it to the caller).
pub(crate) fn write_cells(doc: &mut Document, writes: Vec<(SheetRef, CellAddr, CellEdit)>) -> Result<(), Problem> {
    let batch = writes.into_iter().map(|(sheet, addr, edit)| (sheet, CellWrite { addr, edit, origin: WriteOrigin::Drawing })).collect();
    let outcomes = doc.write_batch(batch)?;
    if outcomes.contains(&WriteOutcome::KeptGuarded) {
        return Err(Problem::failed("a cell of the shape is protected by GUARD and cannot be changed by a drag"));
    }
    Ok(())
}

/// Sets a length cell directly under `sheet`, the way a drag does: a cell whose formula is `BOUND(...)` or `SETATREF(...)` keeps it and
/// passes the number on, any other becomes a value of its own. A guarded cell is an error.
pub(crate) fn write_length(doc: &mut Document, sheet: &SheetRef, name: &str, value: f64) -> Result<(), Problem> {
    write_quantity(doc, sheet, name, &diacraft_units::Quantity::new(value, diacraft_units::Unit::Inch))
}

/// [`write_length`] for any quantity (an angle in radians, a flip's 0 or 1).
pub(crate) fn write_quantity(doc: &mut Document, sheet: &SheetRef, name: &str, quantity: &diacraft_units::Quantity) -> Result<(), Problem> {
    match diacraft_calc::write_value(doc, sheet, &CellAddr::direct(name), quantity, WriteOrigin::Drawing)? {
        WriteOutcome::Written => Ok(()),
        WriteOutcome::KeptGuarded | WriteOutcome::Skipped => {
            Err(Problem::failed(format!("`{name}` is protected by GUARD and cannot be changed by a drag")))
        }
    }
}
