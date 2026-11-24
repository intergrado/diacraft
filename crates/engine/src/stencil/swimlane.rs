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

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Value};

use super::container::{HEADING_ROW, Made, STYLE_ROW, Side, container_shape, drop_user_rows, find_style, put_user_rows};
use super::plain;

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
    Cell::with_formula(&format!("\"{t}\""), Some(Value::from_text(t)))
}

/// A container with no resizing of its own (the layout of the lists sizes it) and the role `role`.
fn made(part: &Part<'_>, style: &str, role: &'static str) -> Shape {
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
    shape
}

/// The frame: a plain outline, no heading, holding the lists.
pub(crate) fn frame_shape(part: &Part<'_>) -> Shape {
    made(part, "frame", FRAME)
}

/// A lane: a strip look with its label on `part.side`.
pub(crate) fn lane_shape(part: &Part<'_>) -> Shape {
    made(part, "strip-centre", LANE)
}

/// A phase: a dashed edge and a divider under its label.
pub(crate) fn phase_shape(part: &Part<'_>) -> Shape {
    made(part, "phase", PHASE)
}

/// A list of lanes (`role` is `lanes`) or of phases (`phases`): typed `List`, drawn with nothing, with its direction (0 down the page, 1 across it).
pub(crate) fn list_shape(part: &Part<'_>, role: &'static str, direction: f64) -> Shape {
    let mut shape = made(part, "frame", role);
    drop_user_rows(&mut shape, &[STYLE_ROW, HEADING_ROW]);
    put_user_rows(&mut shape, vec![("msvStructureType", text_cell("List")), (DIRECTION_ROW, plain(direction))]);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("LinePattern"), plain(0.0));
    sheet.cells.insert(CellName::new("FillPattern"), plain(0.0));
    shape
}
