//! Building documents and reading pixels.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_render::{Rendered, Renderer, View};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

/// Pixels to the inch in every test: coarse enough to be quick, fine enough to see a 0.05 inch line.
pub const PPI: f64 = 100.0;

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

/// A page of `w` by `h` inches at scale 1.
pub fn page(w: f64, h: f64) -> PageBuilder {
    PageBuilder::new().cell("PageWidth", &w.to_string()).cell("PageHeight", &h.to_string()).cell("PageScale", "1").cell("DrawingScale", "1")
}

/// A shape with these cells, its pin at its lower-left corner `(x, y)`, and no geometry yet.
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

/// One geometry section 0 made of `rows`: `("LineTo", &[("X", 1.0), ("Y", 2.0)])`.
pub fn with_geometry(mut sb: ShapeBuilder, rows: &[(&str, &[(&str, f64)])]) -> ShapeBuilder {
    for (i, (kind, cells)) in rows.iter().enumerate() {
        let key = RowKey::Ix(i as u32);
        sb = sb.row_type("Geometry", Some(0), key.clone(), kind);
        for (name, value) in *cells {
            sb = sb.row_cell("Geometry", Some(0), key.clone(), name, &value.to_string());
        }
    }
    sb
}

/// A closed rectangle `w` by `h` in the shape's own frame.
pub fn rect_rows(w: f64, h: f64) -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
    vec![
        ("MoveTo", vec![("X", 0.0), ("Y", 0.0)]),
        ("LineTo", vec![("X", w), ("Y", 0.0)]),
        ("LineTo", vec![("X", w), ("Y", h)]),
        ("LineTo", vec![("X", 0.0), ("Y", h)]),
        ("LineTo", vec![("X", 0.0), ("Y", 0.0)]),
    ]
}

pub fn geometry_of(sb: ShapeBuilder, rows: Vec<(&str, Vec<(&str, f64)>)>) -> ShapeBuilder {
    let refs: Vec<(&str, &[(&str, f64)])> = rows.iter().map(|(k, c)| (*k, c.as_slice())).collect();
    with_geometry(sb, &refs)
}

/// A rectangle shape with its lower-left corner at `(x, y)`: red fill unless told, no explicit line cells.
pub fn rect(id: u32, x: f64, y: f64, w: f64, h: f64) -> ShapeBuilder {
    geometry_of(shape_at(id, x, y, w, h), rect_rows(w, h))
}

/// A cell on the `Geometry` section 0 itself (`NoFill`, `NoLine`, `NoShow`).
pub fn geom_flag(sb: ShapeBuilder, name: &str, value: &str) -> ShapeBuilder {
    sb.set(CellAddr::in_section("Geometry", Some(0), name), Cell::constant(Value::from_text(value)))
}

/// A frame, and where its pixels are.
pub struct Pic {
    pub r: Rendered,
    pub ppi: f64,
    pub page_h: f64,
}

#[allow(dead_code)] // Helpers the other test files use.
impl Pic {
    /// The premultiplied pixel at `(x, y)` paper inches, y up.
    pub fn at(&self, x: f64, y: f64) -> [u8; 4] {
        let px = (x * self.ppi).floor() as i64;
        let py = ((self.page_h - y) * self.ppi).floor() as i64;
        let (w, h) = (i64::from(self.r.width), i64::from(self.r.height));
        assert!(px >= 0 && py >= 0 && px < w && py < h, "({x}, {y}) is outside the {w} by {h} frame");
        let i = ((py * w + px) * 4) as usize;
        [self.r.rgba_premul[i], self.r.rgba_premul[i + 1], self.r.rgba_premul[i + 2], self.r.rgba_premul[i + 3]]
    }

    /// Whether the pixel at `(x, y)` is this opaque colour within `tol` per channel.
    pub fn is(&self, x: f64, y: f64, rgb: (u8, u8, u8), tol: u8) -> bool {
        let p = self.at(x, y);
        p[3] > 250 && near(p[0], rgb.0, tol) && near(p[1], rgb.1, tol) && near(p[2], rgb.2, tol)
    }

    pub fn is_white(&self, x: f64, y: f64) -> bool {
        self.is(x, y, (255, 255, 255), 6)
    }

    /// Whether the pixel is mostly dark ink on the paper.
    pub fn is_dark(&self, x: f64, y: f64) -> bool {
        let p = self.at(x, y);
        p[3] > 200 && p[0] < 100 && p[1] < 100 && p[2] < 100
    }

    pub fn is_red(&self, x: f64, y: f64) -> bool {
        self.is(x, y, (255, 0, 0), 12)
    }

    /// The pixels along a horizontal line at height `y`, from x0 to x1: how many are dark, in runs `(start, length)` in pixels.
    pub fn dark_runs_along_x(&self, y: f64, x0: f64, x1: f64) -> Vec<(i64, i64)> {
        let mut runs: Vec<(i64, i64)> = Vec::new();
        let (a, b) = ((x0 * self.ppi) as i64, (x1 * self.ppi) as i64);
        for px in a..b {
            if self.is_dark(px as f64 / self.ppi + 0.5 / self.ppi, y) {
                match runs.last_mut() {
                    Some((start, len)) if *start + *len == px => *len += 1,
                    _ => runs.push((px, 1)),
                }
            }
        }
        runs
    }

    /// The bounding box, in paper inches, of pixels that are not white paper (alpha above 0 and not white): `(x0, y0, x1, y1)`.
    pub fn ink_box(&self) -> Option<(f64, f64, f64, f64)> {
        let (w, h) = (self.r.width as usize, self.r.height as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let p = &self.r.rgba_premul[i..i + 4];
                if p[3] > 0 && (p[0] < 200 || p[1] < 200 || p[2] < 200) {
                    any = true;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        any.then(|| {
            (x0 as f64 / self.ppi, self.page_h - (y1 as f64 + 1.0) / self.ppi, (x1 as f64 + 1.0) / self.ppi, self.page_h - y0 as f64 / self.ppi)
        })
    }

    /// How many pixels are not white paper.
    pub fn ink_pixels(&self) -> usize {
        self.r.rgba_premul.chunks(4).filter(|p| p[3] > 0 && (p[0] < 200 || p[1] < 200 || p[2] < 200)).count()
    }
}

fn near(a: u8, b: u8, tol: u8) -> bool {
    a.abs_diff(b) <= tol
}

/// Draws page 1 of `doc` at [`PPI`], the whole page.
pub fn draw(doc: &Document) -> Pic {
    draw_with(&mut Renderer::new(), doc)
}

pub fn draw_with(renderer: &mut Renderer, doc: &Document) -> Pic {
    let view = View::fit_page(doc, 1, PPI).expect("the page has a size");
    let r = renderer.render(doc, 1, &view);
    let page_h = f64::from(view.height) / PPI;
    Pic { r, ppi: PPI, page_h }
}

/// A one-page document with these shapes on a 4 by 4 inch page.
pub fn one_page(shapes: Vec<ShapeBuilder>) -> Document {
    let mut p = page(4.0, 4.0);
    for s in shapes {
        p = p.shape(s);
    }
    base().page(1, p).build()
}
