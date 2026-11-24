//! The shape a drawing tool makes: Rectangle, Ellipse, Line, Freeform, Arc and Pencil (`draw/pen`).
//!
//! Our rule (no specification gives the cells of a shape a tool draws). A drawn shape is a **shape with no master**, as a text box is (`text_box.rs` says why), holding its own
//! cells:
//!
//! * *The frame.* The box round what it draws: `PinX` and `PinY` the middle of the box, `Width` and `Height` its size, `LocPinX` and `LocPinY` the formulas `Width*0.5` and
//!   `Height*0.5`, `Angle`, `FlipX`, `FlipY` and `ResizeMode` 0, and the seven cells of the text block with the formulas, so that it can hold
//!   text and be resized, turned and flipped as any shape is.
//! * *The path.* One `Geometry` section, whose rows are the tool's (`draw/pen`), each coordinate a share of the box (`Width*0.25`, the constant 0 for a share of 0, the bare `Width` for 1:
//!   the way a master's rows are written, so that a resize takes the path with the box). `NoFill` is 0 for a closed path and 1 for an open one: "an open path shows the page through it
//!   and cannot take a fill", the other four flags 0.
//! * *The look.* **Linked to the root style**, as the shapes of files are, so that a closed shape takes the style's fill and every shape its line: "a closed shape takes
//!   the default fill; an open one has line only". The shape holds no fill or line cell of its own.
//! * *Connection points.* The Rectangle and the Ellipse have the four middles of their sides, as the masters of those shapes have, so that a connector can be glued to them. An open path
//!   has none: a connector is glued to such a shape as a whole.
//! * *Name.* None: a nameless shape answers `Sheet.<id>` to `NAME()`.

use std::sync::Arc;

use diacraft_doc::{Links, Section, Shape, ShapeType, Sheet};

use super::{length, length_formula, plain, put, text_box};

/// The frame of a drawn shape and what it holds besides its path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawnFrame {
    /// The middle of the box on the page.
    pub pin: (f64, f64),
    pub width: f64,
    pub height: f64,
    /// Whether the shape has the four middles of its sides as connection points.
    pub sides: bool,
}

/// The shape `id` with the frame `frame` and the path `path`.
pub(crate) fn drawn_shape(id: u32, frame: &DrawnFrame, path: Section) -> Shape {
    let (width, height) = (frame.width, frame.height);
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    put(cells, "PinX", length(frame.pin.0));
    put(cells, "PinY", length(frame.pin.1));
    put(cells, "Width", length(width));
    put(cells, "Height", length(height));
    put(cells, "LocPinX", length_formula("Width*0.5", width * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", height * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    for (name, cell) in super::text_transform_cells(width, height) {
        put(cells, name, cell);
    }
    sheet.sections.push(Arc::new(path));
    if frame.sides {
        sheet.sections.push(Arc::new(text_box::sides(width, height)));
    }
    Shape { kind: ShapeType::Shape, sheet: Arc::new(sheet), ..Shape::new(id) }
}
