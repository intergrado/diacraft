//! The model a model writer is given for a drawing of the other format (harvested from the shelved the `across/onto.rs`, which re-based the drawing on a blank package and emptied its
//! `raw` and `passthrough`; the model writers now take the whole model and route what the target cannot say, so this only does what the target needs of the drawing that the model cannot hold).
//!
//! * **`drop_objects`**: the objects of the other format are left out, and what is left of their shapes is the picture they show ([`super::hold::without_objects`]).
//! * **The picture box of a picture an `.odg` held** (the concern 2: "the `Img` cells are written as for a new picture"). The `.odg` reader
//!   gives a picture frame no `ImgOffsetX`, `ImgOffsetY`, `ImgWidth` or `ImgHeight`, since the frame is the picture; a `Foreign` shape of a VSDX file draws its picture in the box those four
//!   cells say, and **LibreOffice draws a `Foreign` shape only with them** (measured: the first run of its files converted to empty pages). So a save to VSDX **gives the model those
//!   cells** for each shape that shows a picture and came from an `.odg`: the picture fills the shape (`ImgWidth` is `Width*1`, `ImgHeight` is `Height*1`, the offsets are 0). The drawing the person
//!   has is not changed; the copy the writer is given has them, and a save that adopts the model the file reads as (`Session::adopt_saved`) gives the drawing them.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Format, OdfShape, Shape, ShapeType, Value};

use super::Options;
use super::hold::{Left, hold, rewrite_shapes, without_objects};

/// What the copy handed to a model writer is, and what was done to make it.
#[derive(Debug, Default)]
pub(crate) struct Given {
    /// The copy of the drawing, or `None` when it is given as it is.
    pub doc: Option<Document>,
    /// The objects of the other format that were left out (`drop_objects`).
    pub left: Left,
    /// The shapes that were given the cells of a picture's box.
    pub boxed: u64,
}

/// The number a shape's own cell holds.
fn number(shape: &Shape, name: &str) -> Option<f64> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()).and_then(Value::number)
}

/// A shape that shows a picture it held as an `.odg` frame and has no picture box: it gets the four cells that make the picture fill the shape. `None` for any other shape.
fn boxed(shape: &Shape) -> Option<Shape> {
    let odf = shape.ext.get().is_some_and(|data| data.odf != OdfShape::default());
    let picture = shape.foreign.as_ref().is_some_and(|f| f.picture.is_some());
    if shape.kind != ShapeType::Foreign || !odf || !picture || shape.sheet.cell("ImgWidth").is_some() || shape.sheet.cell("ImgHeight").is_some() {
        return None;
    }
    let (width, height) = (number(shape, "Width")?, number(shape, "Height")?);
    let mut own = Shape::clone(shape);
    let sheet = Arc::make_mut(&mut own.sheet);
    for (name, cell) in [
        ("ImgOffsetX", Value::from_number(0.0).map(|v| Cell::constant(v).with_unit("IN"))),
        ("ImgOffsetY", Value::from_number(0.0).map(|v| Cell::constant(v).with_unit("IN"))),
        ("ImgWidth", Value::from_number(width).map(|v| Cell::with_formula("Width*1", Some(v)).with_unit("IN"))),
        ("ImgHeight", Value::from_number(height).map(|v| Cell::with_formula("Height*1", Some(v)).with_unit("IN"))),
    ] {
        sheet.cells.insert(CellName::new(name), cell?);
    }
    Some(own)
}

/// The copy of `doc` that the model writer of `target` is given, or none when `doc` is given as it is (see the module documentation).
pub(crate) fn given(doc: &Document, target: Format, options: Options) -> Result<Given, &'static str> {
    let mut out = Given::default();
    let mut steps = 0u64;
    // (A drawing that holds no object has nothing to leave out, which one pass over its parts and shapes says before any copy is made.)
    if options.drop_objects {
        let held = hold(doc)?;
        if (held.object_parts > 0 || held.object_shapes > 0)
            && let Some((without, left)) = without_objects(doc, target)?
        {
            out.doc = Some(without);
            out.left = left;
        }
    }
    if target == Format::Vsdx {
        let mut count = 0u64;
        let mut change = |shape: &Shape| {
            let own = boxed(shape);
            count = count.saturating_add(u64::from(own.is_some()));
            own
        };
        let base = out.doc.as_ref().unwrap_or(doc);
        if let Some(with) = rewrite_shapes(base, true, &mut change, &mut steps)? {
            out.doc = Some(with);
        }
        out.boxed = count;
    }
    Ok(out)
}

/// The words that say what `drop_objects` left out, for the warnings of the save (none when it left nothing out): the drawing keeps the objects, and each shape that showed one shows its picture.
pub(crate) fn left_out_words(given: &Given) -> Option<String> {
    let Left { shapes, parts } = given.left;
    let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    if shapes > 0 {
        Some(format!(
            "{} left out of the file, as asked: each shape stays as the picture it showed, and the drawing still holds them (save it as .diacraft to keep them)",
            plural(shapes, "embedded object was", "embedded objects were")
        ))
    } else if parts > 0 {
        Some(format!(
            "{} of embedded objects left out of the file, as asked: the drawing still holds them (save it as .diacraft to keep them)",
            plural(u64::try_from(parts).unwrap_or(u64::MAX), "part was", "parts were")
        ))
    } else {
        None
    }
}
