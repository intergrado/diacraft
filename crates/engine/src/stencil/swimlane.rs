//! The shapes a cross-functional flowchart is made of: the frame, the two lists (of lanes and of phases), a lane and a phase. Each is a shape with no master, made in
//! code like a container (`container.rs`), with the cells a ShapeSheet says make it what it is, and no picture of another application.
//!
//! (ours: the specifications name the parts, `CFF Container`, `Swimlane List`, `Phase List`, lanes and phases, and what each one is made of, a container or a list
//! And no cell of a look):
//!
//! * **The frame** is a container drawn with a plain outline and no heading; it holds the two lists (code 1) and is their box.
//! * **A list** is a shape typed `List` (`User.msvStructureType`) with no outline and no fill: it holds its members under code 2 (`DEPENDSON(2, ...)`, in their order) and is the box
//!   of them. Its direction is a row of ours, `User.DiaCraftListDirection`: 0 for members one under the next, 1 for members side by side.
//! * **A lane** is a container of the gallery's `strip-centre` look, the heading on its left side (the label runs up it) or its top. It holds the shapes dropped in it under code 1, and names its
//!   list under code 5. **A phase** is a container of the `phase` look: a dashed line where it begins and a divider under its label; its box is the band and the lanes' length across.
//! * **What each is** is a row of ours, `User.DiaCraftSwimlane`: `frame`, `lanes`, `phases`, `lane` or `phase`. A shape that has none is read from the file's own cells instead
//!   (`draw/container/lists.rs`): a list of lanes another program wrote is laid out too, and no row of ours is written into its shapes.
//! * **What each holds of `Rounding`**: 0, as the shape's own cell. The parts stand against one another (a lane under a lane, a phase beside a phase), and a theme with
//!   rounded corners would round each by itself and leave a notch in every join.
//! * **What each holds of `ObjType`** (item 2): **the lane, the phase and both lists hold 4**, neither placeable nor routable, as the
//!   reference's swimlane and separator masters and the lists of a cross-functional flowchart hold, so no route goes round a lane and a glue to one writes nothing on it; **the frame holds 1**, as the
//!   reference's outer container does, so it is placeable. Each is the shape's own cell (these shapes have no master), written when DiaCraft makes the shape: a drawing saved before holds none.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Value};

use super::container::{HEADING_ROW, Made, STYLE_ROW, Side, container_shape, drop_user_rows, find_style, put_user_rows};
use super::{length, plain, put};

/// The row that says what a shape is in a cross-functional flowchart.
pub(crate) const ROLE_ROW: &str = "DiaCraftSwimlane";
/// The row of a list that says which way its members run: 0 down the page, 1 across it.
pub(crate) const DIRECTION_ROW: &str = "DiaCraftListDirection";

/// The roles, as the row holds them.
pub(crate) const FRAME: &str = "frame";
pub(crate) const LANES: &str = "lanes";
pub(crate) const PHASES: &str = "phases";
pub(crate) const LANE: &str = "lane";
pub(crate) const PHASE: &str = "phase";
/// A pool's frame: the frame of a structure of one list of lanes, with the pool's name in a strip on one side. Everything that treats a frame treats it as one.
pub(crate) const POOL: &str = "pool";

/// The label a new lane has until a person types another, and a new phase.
pub const LANE_LABEL: &str = "Function";
pub const PHASE_LABEL: &str = "Phase";

/// What a part is made with, in drawing units.
pub(crate) struct Part<'a> {
    pub id: u32,
    pub centre: (f64, f64),
    pub size: (f64, f64),
    /// The side of the box the label is on.
    pub side: Side,
    /// The label's strip, and the margin a Fit to Contents leaves round the members.
    pub band: f64,
    pub margin: f64,
    pub text: &'a str,
}

fn text_cell(t: &str) -> Cell {
    // A text a cell holds says `STR`.
    Cell::with_formula(&format!("\"{t}\""), Some(Value::from_text(t))).with_unit("STR")
}

/// `ObjType` 1: placeable, the frame's. The lane, the phase and the lists hold [`NEITHER`].
const PLACEABLE: f64 = 1.0;
/// `ObjType` 4: neither placeable nor routable (item 2).
const NEITHER: f64 = 4.0;

/// A container with no resizing of its own (the layout of the lists sizes it), the role `role` and the `ObjType` `obj_type`, as the shape's own cell.
fn made(part: &Part<'_>, style: &str, role: &'static str, obj_type: f64) -> Shape {
    let spec = find_style(style).unwrap_or(&super::container::STYLES[0]);
    let mut shape = container_shape(&Made {
        id: part.id,
        centre: part.centre,
        size: part.size,
        style: spec,
        side: part.side,
        band: part.band,
        margin: part.margin,
        resize: 0,
        heading: part.text,
    });
    put_user_rows(&mut shape, vec![(ROLE_ROW, text_cell(role))]);
    let cells = &mut Arc::make_mut(&mut shape.sheet).cells;
    cells.insert(CellName::new("ObjType"), plain(obj_type));
    // The parts of a structure stand against one another (a lane under a lane, a phase beside a phase), and a theme with rounded corners would round each by itself and leave a notch in every
    // join. A cell of the shape outranks its style, so each part holds its own `Rounding`, 0.
    put(cells, "Rounding", length(0.0));
    shape
}

/// The frame: a plain outline, no heading, holding the lists.
pub(crate) fn frame_shape(part: &Part<'_>) -> Shape {
    made(part, "frame", FRAME, PLACEABLE)
}

/// A lane: a strip look with its label on `part.side`.
pub(crate) fn lane_shape(part: &Part<'_>) -> Shape {
    made(part, "strip-centre", LANE, NEITHER)
}

/// A pool's frame: a container whose name is in a strip on `part.side`, **with a line between the strip and what the pool holds** (the `strip-centre` look). It is an outer
/// container of the structure, so it holds `ObjType` 1 ([`PLACEABLE`]) as the cross-functional frame does (the `made`, which every shape of this file is made by).
pub(crate) fn pool_frame_shape(part: &Part<'_>) -> Shape {
    made(part, "strip-centre", POOL, PLACEABLE)
}

/// A lane of a pool (DD11): **no line between its label and its contents** (the `frame-centre` look: a box and a strip for the text), and `ObjType` 4, as the reference's
/// swimlanes hold ([`NEITHER`]), so that no route goes round it.
pub(crate) fn pool_lane_shape(part: &Part<'_>) -> Shape {
    made(part, "frame-centre", LANE, NEITHER)
}

/// The list of a pool's lanes: as a cross-functional flowchart's, which holds `ObjType` 4 already (`list_shape`).
pub(crate) fn pool_list_shape(part: &Part<'_>, direction: f64) -> Shape {
    list_shape(part, LANES, direction)
}

/// A phase: a dashed edge and a divider under its label.
pub(crate) fn phase_shape(part: &Part<'_>) -> Shape {
    made(part, "phase", PHASE, NEITHER)
}

/// A list of lanes (`role` is `lanes`) or of phases (`phases`): typed `List`, drawn with nothing, with its direction (0 down the page, 1 across it).
pub(crate) fn list_shape(part: &Part<'_>, role: &'static str, direction: f64) -> Shape {
    let mut shape = made(part, "frame", role, NEITHER);
    drop_user_rows(&mut shape, &[STYLE_ROW, HEADING_ROW]);
    put_user_rows(&mut shape, vec![("msvStructureType", text_cell("List")), (DIRECTION_ROW, plain(direction))]);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("LinePattern"), plain(0.0));
    sheet.cells.insert(CellName::new("FillPattern"), plain(0.0));
    shape
}
