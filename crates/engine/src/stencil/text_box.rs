//! The shape the Text tool makes: a box of text on the page with no master (`docs/editing-commands.md`).
//!
//! Our rule (no specification gives the cells of a text box). A text box is a **shape with no master**, holding its own cells:
//!
//! * *Why no master.* The feature's grade says the Text tool "makes a box of text independent of any shape" (Text tool and
//!   text boxes), so the box is not an instance of a stencil's master. Files hold such shapes as a matter of course: 126,699 page shapes with no
//!   master at any depth are nameless, and every page shape with no master carries its style links
//!   (135,527 of them). A master would add a master to every drawing, with no stencil to belong to and a name in the
//!   document's list of masters that no person chose.
//! * *Its cells.* The pose cells a shape made from a master has (`PinX`, `PinY`, `Width`, `Height`, `LocPinX` and `LocPinY` as the formulas written on
//!   creation, `Angle`, `FlipX`, `FlipY`), the seven `Text Transform` cells with the formulas a public page prints as the ones written on creation
//!   And the links to the root style, as the shapes of real files have them. It is **linked to the root style**
//!   So its text is the root's: 12 point, black, in the middle of its block with the root's margin.
//! * *Taking fill, line and effects.* One `Geometry` section holds a rectangle that fills the box, with `NoFill` 0 and `NoLine` 0, and the shape holds
//!   `FillPattern` 0 and `LinePattern` 0 of its own: it paints nothing until a fill or a line is chosen, and then `shape.format`
//!   makes the pattern solid and the path takes the colour. A box with no path would have nothing to fill.
//! * *Connection points.* The four middles of the sides, as the Rectangle master has them, so that a connector can be glued to a text box.
//! * *Name.* None: a nameless shape answers `Sheet.<id>` to `NAME()`.
//! * *Size.* 1 inch by 0.25 inch unless the caller says (a drag), and it does not grow with its text: it holds no formula for its size. `calc` works
//!   `TEXTHEIGHT` and `TEXTWIDTH` out (the text laid out by `crates/text`, in the faces of `engine::measure`), so a box that is to size
//!   itself by them can, and the dependency graph follows what they read (the formats, tab stops and margins of the text, the page's scale); the
//!   box does not yet.

use std::sync::Arc;

use diacraft_doc::{Cell, Links, Section, Shape, ShapeType, Sheet, Text, TextPart};

use super::{MASTERS, MasterSpec, More, Outline, flagged_section, geometry, length, length_formula, plain, put};

/// The width of a text box made without a size, in inches.
pub const TEXT_BOX_WIDTH: f64 = 1.0;
/// The height of a text box made without a size, in inches.
pub const TEXT_BOX_HEIGHT: f64 = 0.25;

/// The seven cells of the text block for a block that is the whole shape, as prints their formulas: `TxtWidth` is `Width*1`, the pin the
/// middle of the shape, the local pin the middle of the block. `width` and `height` are the shape's, for the values the formulas give.
pub(crate) fn text_transform_cells(width: f64, height: f64) -> [(&'static str, Cell); 7] {
    [
        ("TxtPinX", length_formula("Width*0.5", width * 0.5)),
        ("TxtPinY", length_formula("Height*0.5", height * 0.5)),
        ("TxtLocPinX", length_formula("TxtWidth*0.5", width * 0.5)),
        ("TxtLocPinY", length_formula("TxtHeight*0.5", height * 0.5)),
        ("TxtWidth", length_formula("Width*1", width)),
        ("TxtHeight", length_formula("Height*1", height)),
        ("TxtAngle", plain(0.0)),
    ]
}

/// The `Geometry` section of a text box: the rectangle that fills the box, as the Rectangle master draws it.
pub(super) fn rectangle(width: f64, height: f64) -> Section {
    let spec = MasterSpec {
        identity: (0, 0),
        id: "text_box",
        name: "Text Box",
        width,
        height,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &[],
        more: &More::NONE,
    };
    // A polygon is one section; the fallback is an empty one that still fills, which `geometry` cannot give.
    geometry(&spec).into_iter().next().unwrap_or_else(|| flagged_section(0, true))
}

/// The `Connection` section of a text box: the Rectangle master's four, the middles of the sides.
pub(super) fn sides(width: f64, height: f64) -> Section {
    let points = MASTERS.iter().find(|m| m.id == "rectangle").map_or(&[][..], |m| m.points);
    super::connections(&MasterSpec {
        identity: (0, 0),
        id: "text_box",
        name: "Text Box",
        width,
        height,
        outline: Outline::Polygon(&[]),
        points,
        more: &More::NONE,
    })
}

/// The shape `id`: a text box centred at (`x`, `y`) of the size `width` by `height`, holding `text` (none when it is empty). Lengths are in the
/// page's drawing units; the caller has checked them.
pub(crate) fn text_box_shape(id: u32, (x, y): (f64, f64), (width, height): (f64, f64), text: &str) -> Shape {
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    put(cells, "PinX", length(x));
    put(cells, "PinY", length(y));
    put(cells, "Width", length(width));
    put(cells, "Height", length(height));
    put(cells, "LocPinX", length_formula("Width*0.5", width * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", height * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    for (name, cell) in text_transform_cells(width, height) {
        put(cells, name, cell);
    }
    // Nothing is painted until a fill or a line is chosen.
    put(cells, "FillPattern", plain(0.0));
    put(cells, "LinePattern", plain(0.0));
    sheet.sections.push(Arc::new(rectangle(width, height)));
    sheet.sections.push(Arc::new(sides(width, height)));
    let text = (!text.is_empty()).then(|| Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
    Shape { kind: ShapeType::Shape, sheet: Arc::new(sheet), text, ..Shape::new(id) }
}
