//! The shape Insert, Pictures makes: a picture on the page with no master (`docs/editing-commands.md`).
//!
//! Our rule (the specs give the cells of a picture but no creation recipe beyond `ImgWidth` and `ImgHeight` as `Width*1` and `Height*1`). A picture is a
//! **shape of `Type` `Foreign` with no master**, holding its own cells: the pose cells (`PinX`, `PinY`, `Width`, `Height`, `LocPinX` and `LocPinY` as the formulas written on creation,
//! `Angle`, `FlipX`, `FlipY`), the four cells of the box (`ImgOffsetX` and `ImgOffsetY` 0, `ImgWidth` `Width*1` and `ImgHeight` `Height*1`), and the links to the root style. It holds **one
//! `Geometry` section, the rectangle of its frame** (the `.odg` reader gives a picture frame the same), with `FillPattern` and `LinePattern` 0 so that nothing is
//! painted round it until a fill or a line is chosen. **It has no connection points** ("a picture is a shape without connection points but a connector can still be attached
//! to it"): a connector glues to the shape as a whole.

use std::sync::Arc;

use diacraft_doc::{ForeignData, Links, RawAttrs, Shape, ShapeType, Sheet};

use super::text_box::rectangle;
use super::{length, length_formula, plain, put};

/// The shape `id`: a picture centred at (`x`, `y`) of the size `width` by `height`, in the page's drawing units (the caller has checked them), showing `media`, of `foreign_type`
/// (`Bitmap` or `EnhMetaFile`).
pub(crate) fn picture_shape(id: u32, (x, y): (f64, f64), (width, height): (f64, f64), foreign_type: &str, media: Arc<diacraft_doc::Media>) -> Shape {
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
    // The box the picture is stretched to: the whole frame.
    put(cells, "ImgOffsetX", length(0.0));
    put(cells, "ImgOffsetY", length(0.0));
    put(cells, "ImgWidth", length_formula("Width*1", width));
    put(cells, "ImgHeight", length_formula("Height*1", height));
    put(cells, "FillPattern", plain(0.0));
    put(cells, "LinePattern", plain(0.0));
    sheet.sections.push(Arc::new(rectangle(width, height)));
    let foreign = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", foreign_type)]), picture: Some(media), ..ForeignData::default() };
    Shape { kind: ShapeType::Foreign, sheet: Arc::new(sheet), foreign: Some(Arc::new(foreign)), ..Shape::new(id) }
}
