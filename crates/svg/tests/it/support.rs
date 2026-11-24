//! Building documents, and reading an SVG back with a parser that knows nothing of the writer.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_opc::xml::{El, parse};
use diacraft_render::Renderer;
use diacraft_render::export::Exported;
use diacraft_svg::{SvgOptions, export_page};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

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

/// A cell on the `Geometry` section 0 itself (`NoFill`, `NoLine`, `NoShow`).
#[allow(dead_code)]
pub fn geom_flag(sb: ShapeBuilder, name: &str, value: &str) -> ShapeBuilder {
    sb.set(CellAddr::in_section("Geometry", Some(0), name), Cell::constant(Value::from_text(value)))
}

/// A one-page document with these shapes on a 4 by 4 inch page.
pub fn one_page(shapes: Vec<ShapeBuilder>) -> Document {
    let mut p = page(4.0, 4.0);
    for s in shapes {
        p = p.shape(s);
    }
    base().page(1, p).build()
}

/// Exports page 1 with the options, and parses the file: the root `<svg>` element.
pub fn export(doc: &Document, options: &SvgOptions) -> (Exported, El) {
    let out = export_page(&mut Renderer::new(), doc, 1, options).expect("exports");
    let root = parse(&out.bytes).expect("a well-formed SVG file");
    (out, root)
}

/// Every element under `el` in document order, `el` first.
pub fn all(el: &El) -> Vec<&El> {
    let mut out = vec![el];
    for child in el.elements() {
        out.extend(all(child));
    }
    out
}

/// The elements named `local` under `root`.
pub fn named<'a>(root: &'a El, local: &str) -> Vec<&'a El> {
    all(root).into_iter().filter(|e| e.is(local)).collect()
}

/// The numbers of a string, in order, whatever separates them.
pub fn numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<f64>() {
                out.push(n);
            }
            cur.clear();
        }
    }
    out
}

/// The corners of a path's `d`, as `(x, y)` pairs in order (the letters are ignored: tests use paths of straight lines).
pub fn points(d: &str) -> Vec<(f64, f64)> {
    numbers(d).chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect()
}

/// The box `(x0, y0, x1, y1)` of a path's points.
pub fn bbox(d: &str) -> (f64, f64, f64, f64) {
    let pts = points(d);
    let xs = pts.iter().map(|p| p.0);
    let ys = pts.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

pub fn attr<'a>(el: &'a El, name: &str) -> &'a str {
    el.attr(name).unwrap_or_else(|| panic!("<{}> has no `{name}`: {:?}", el.name, el.attrs))
}
