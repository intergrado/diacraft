//! The time of a frame on a page like a real one: a few thousand instances of two masters, with labels, inherited cells, a style chain and
//! connectors with line ends. `cargo test -p diacraft-render --test it frame_time -- --ignored --nocapture` prints the times.

use std::time::Instant;

use diacraft_doc::RowKey;
use diacraft_render::{Renderer, View};
use diacraft_testkit::build::{MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use kurbo::Affine;

use crate::support::*;

/// The document: a 60 by 40 inch page with `columns` by `rows` boxes of master 1, each with a label, and a connector of master 2 from each
/// to its right neighbour.
pub(crate) fn diagram(columns: u32, rows: u32, shadows: bool) -> diacraft_doc::Document {
    diagram_on(columns, rows, shadows, 60.0, 40.0)
}

/// As [`diagram`] on a page of `page_w` by `page_h` inches.
pub(crate) fn diagram_on(columns: u32, rows: u32, shadows: bool, page_w: f64, page_h: f64) -> diacraft_doc::Document {
    // Master 1: a rounded box (a few geometry rows), filled by style 3, with its text style.
    let box_master = MasterBuilder::new().shape(
        geometry_of(
            shape_at(1, 0.0, 0.0, 1.2, 0.6).formula("LocPinX", "Width*0.5", "0.6").formula("LocPinY", "Height*0.5", "0.3"),
            vec![
                ("MoveTo", vec![("X", 0.0), ("Y", 0.0)]),
                ("LineTo", vec![("X", 1.2), ("Y", 0.0)]),
                ("LineTo", vec![("X", 1.2), ("Y", 0.6)]),
                ("LineTo", vec![("X", 0.0), ("Y", 0.6)]),
                ("LineTo", vec![("X", 0.0), ("Y", 0.0)]),
            ],
        )
        .links(Some(3), Some(3), Some(3))
        .cell("Rounding", "0.08"),
    );
    let line_master = MasterBuilder::new().shape(
        geometry_of(shape_at(1, 0.0, 0.0, 1.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0), ("Y", 0.0)])])
            .links(Some(3), Some(3), Some(3))
            .cell("EndArrow", "4"),
    );
    let mut page = PageBuilder::new()
        .cell("PageWidth", &page_w.to_string())
        .cell("PageHeight", &page_h.to_string())
        .cell("PageScale", "1")
        .cell("DrawingScale", "1");
    let mut id = 1u32;
    for r in 0..rows {
        for c in 0..columns {
            let (x, y) = (1.0 + f64::from(c) * 1.6, 1.0 + f64::from(r) * 1.1);
            let inst = ShapeBuilder::new(id)
                .master(1)
                .inh("Width", Some("1.2"))
                .inh("Height", Some("0.6"))
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .inh("LocPinX", Some("0.6"))
                .inh("LocPinY", Some("0.3"))
                .text(&format!("Step {id}"))
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.1");
            page = page.shape(inst);
            id += 1;
            if c + 1 < columns {
                let conn = ShapeBuilder::new(id)
                    .master(2)
                    .cell("PinX", &(x + 1.2).to_string())
                    .cell("PinY", &y.to_string())
                    .cell("Width", "0.4")
                    .cell("Height", "0")
                    .cell("LocPinX", "0")
                    .cell("LocPinY", "0");
                page = page.shape(conn);
                id += 1;
            }
        }
    }
    let shadow = |s: StyleBuilder| {
        if shadows {
            s.cell("ShdwPattern", "1")
                .cell("ShapeShdwType", "1")
                .cell("ShapeShdwOffsetX", "0.04")
                .cell("ShapeShdwOffsetY", "-0.04")
                .cell("ShapeShdwBlur", "0.04")
                .cell("ShdwForegnd", "#808080")
        } else {
            s
        }
    };
    base()
        .style(
            3,
            shadow(StyleBuilder::new())
                .cell("FillForegnd", "#C7D9F0")
                .cell("FillPattern", "1")
                .cell("LineColor", "#2F5597")
                .cell("LineWeight", "0.0104")
                .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.1666"),
        )
        .master(1, box_master)
        .master(2, line_master)
        .page(1, page)
        .build()
}

#[test]
#[ignore = "a timing, run by hand"]
fn frame_time_on_a_page_of_thousands_of_shapes() {
    let doc = diagram(36, 30, false);
    let shapes: usize = doc.page(1).map_or(0, |p| p.shapes.len());
    let view = View { to_px: Affine::new([30.0, 0.0, 0.0, -30.0, 0.0, 1200.0]), width: 1800, height: 1200 };
    let mut renderer = Renderer::new();
    let started = Instant::now();
    let first = renderer.render(&doc, 1, &view);
    let cold = started.elapsed();
    let started = Instant::now();
    let second = renderer.render(&doc, 1, &view);
    let warm = started.elapsed();
    eprintln!("FRAME TIME: {shapes} shapes, whole page at 1800x1200: first frame {cold:?}, second frame {warm:?}; skipped {:?}", first.skipped);
    let ink = first.rgba_premul.chunks(4).filter(|p| p[0] < 200 || p[1] < 200 || p[2] < 200).count();
    eprintln!("FRAME TIME: {ink} pixels are not paper");
    assert!(ink > 100_000, "the page is drawn");
    assert_eq!(first.rgba_premul.len(), 1800 * 1200 * 4);
    assert_eq!(second.rgba_premul.len(), first.rgba_premul.len());
    // Zoomed in on a corner: most shapes are off the view.
    let view = View { to_px: Affine::new([150.0, 0.0, 0.0, -150.0, -150.0, 1200.0 + 150.0 * 30.0]), width: 1800, height: 1200 };
    let started = Instant::now();
    let zoomed = renderer.render(&doc, 1, &view);
    eprintln!("FRAME TIME: zoomed to a corner: {:?}; skipped {:?}", started.elapsed(), zoomed.skipped);
}

#[test]
#[ignore = "a timing, run by hand"]
fn frame_time_with_a_blurred_shadow_on_every_shape() {
    let doc = diagram(36, 30, true);
    let view = View { to_px: Affine::new([30.0, 0.0, 0.0, -30.0, 0.0, 1200.0]), width: 1800, height: 1200 };
    let mut renderer = Renderer::new();
    let started = Instant::now();
    let first = renderer.render(&doc, 1, &view);
    eprintln!("FRAME TIME: blurred shadows, 2,130 shapes, first frame {:?}; skipped {:?}", started.elapsed(), first.skipped);
}
