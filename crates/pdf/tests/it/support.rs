//! Building documents for the PDF tests, and exporting and reading back.

use diacraft_doc::{Document, RowKey};
use diacraft_pdf::{PdfOptions, export_page};
use diacraft_render::Renderer;
use diacraft_render::export::Exported;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::readback::{Seen, read};

/// A document builder whose root style holds what every real file's does: a solid white fill, a black round line of 0.01 inch, no shadow.
pub fn base() -> DocBuilder {
    DocBuilder::new().root(|r| {
        r.cell("FillPattern", "1")
            .cell("FillForegnd", "1")
            .cell("FillBkgnd", "0")
            .cell("FillForegndTrans", "0")
            .cell("FillBkgndTrans", "0")
            .cell("LinePattern", "1")
            .cell("LineColor", "0")
            .cell("LineColorTrans", "0")
            .cell("LineWeight", "0.01")
            .cell("LineCap", "0")
            .cell("Rounding", "0")
            .cell("BeginArrow", "0")
            .cell("EndArrow", "0")
            .cell("BeginArrowSize", "2")
            .cell("EndArrowSize", "2")
            .cell("ShdwPattern", "0")
            .cell("ShapeShdwType", "0")
            .cell("ShapeShdwScaleFactor", "1")
            .cell("ShapeShdwOffsetX", "0")
            .cell("ShapeShdwOffsetY", "0")
            .cell("ShapeShdwShow", "2")
            .cell("FillGradientEnabled", "0")
            .cell("LineGradientEnabled", "0")
            .cell("RotateGradientWithShape", "1")
            .cell("DisplayMode", "2")
            .cell("LayerMember", "")
            .cell("VerticalAlign", "1")
            .cell("LeftMargin", "0")
            .cell("RightMargin", "0")
            .cell("TopMargin", "0")
            .cell("BottomMargin", "0")
            .cell("DefaultTabStop", "0.5")
            .cell("TextBkgnd", "0")
            .cell("TextBkgndTrans", "0")
    })
}

pub fn page(w: f64, h: f64) -> PageBuilder {
    PageBuilder::new().cell("PageWidth", &w.to_string()).cell("PageHeight", &h.to_string()).cell("PageScale", "1").cell("DrawingScale", "1")
}

pub fn shape_at(id: u32, x: f64, y: f64, w: f64, h: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &x.to_string())
        .cell("PinY", &y.to_string())
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .cell("Angle", "0")
        .cell("FlipX", "0")
        .cell("FlipY", "0")
}

pub fn geometry_of(mut sb: ShapeBuilder, rows: Vec<(&str, Vec<(&str, f64)>)>) -> ShapeBuilder {
    for (i, (kind, cells)) in rows.iter().enumerate() {
        let key = RowKey::Ix(i as u32);
        sb = sb.row_type("Geometry", Some(0), key.clone(), kind);
        for (name, value) in cells {
            sb = sb.row_cell("Geometry", Some(0), key.clone(), name, &value.to_string());
        }
    }
    sb
}

pub fn rect_rows(w: f64, h: f64) -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
    vec![
        ("MoveTo", vec![("X", 0.0), ("Y", 0.0)]),
        ("LineTo", vec![("X", w), ("Y", 0.0)]),
        ("LineTo", vec![("X", w), ("Y", h)]),
        ("LineTo", vec![("X", 0.0), ("Y", h)]),
        ("LineTo", vec![("X", 0.0), ("Y", 0.0)]),
    ]
}

/// A rectangle shape with its lower-left corner at `(x, y)`.
pub fn rect(id: u32, x: f64, y: f64, w: f64, h: f64) -> ShapeBuilder {
    geometry_of(shape_at(id, x, y, w, h), rect_rows(w, h))
}

/// A one-page document with these shapes on a 4 by 4 inch page.
pub fn one_page(shapes: Vec<ShapeBuilder>) -> Document {
    let mut p = page(4.0, 4.0);
    for s in shapes {
        p = p.shape(s);
    }
    base().page(1, p).build()
}

/// The options the tests use: the page's content in plain operators, which a reader reads either way.
pub fn plain() -> PdfOptions {
    PdfOptions { compress: false, ..PdfOptions::default() }
}

/// Exports page 1 and reads the file back.
pub fn export(doc: &Document, options: &PdfOptions) -> (Exported, Seen) {
    export_with(&mut Renderer::new(), doc, options)
}

pub fn export_with(renderer: &mut Renderer, doc: &Document, options: &PdfOptions) -> (Exported, Seen) {
    let out = export_page(renderer, doc, 1, options).expect("exports");
    let seen = read(&out.bytes);
    (out, seen)
}

pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}
