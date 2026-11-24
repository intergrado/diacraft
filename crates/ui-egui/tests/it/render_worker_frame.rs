//! The time of a page drawn by the window's render worker, by hand:
//! `DIACRAFT_THREADS=31 cargo test -p diacraft-ui-egui --release --test it render_worker_frame -- --ignored --nocapture`.
//!
//! The worker's own timing (`Done::took`) is printed for a page of 2,130 shapes and one of 11,925, whole at 1800 by 1200, a few frames each, so that
//! the least can be taken. The drawer is the bundled one, which takes its thread count from `DIACRAFT_THREADS` as the window's does
//! (`diacraft_render::default_threads`); the pictures of the two counts are compared by the render crate's tests, not here.

use std::sync::Arc;
use std::time::Duration;

use diacraft_doc::{Document, RowKey};
use diacraft_render::{Limits, View};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_ui_egui::render_worker::{Job, RenderWorker, bundled_drawer};
use kurbo::Affine;

/// `columns` by `rows` boxes of 1.2 by 0.6 inch, each with a label, and a connector from each to its right neighbour, on a page of `w` by `h` inches.
fn diagram(columns: u32, rows: u32, w: f64, h: f64) -> Document {
    let mut page =
        PageBuilder::new().cell("PageWidth", &w.to_string()).cell("PageHeight", &h.to_string()).cell("PageScale", "1").cell("DrawingScale", "1");
    let mut id = 1u32;
    for r in 0..rows {
        for c in 0..columns {
            let (x, y) = (1.0 + f64::from(c) * 1.6, 1.0 + f64::from(r) * 1.1);
            let mut sb = ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .cell("Width", "1.2")
                .cell("Height", "0.6")
                .cell("LocPinX", "0")
                .cell("LocPinY", "0")
                .cell("FillForegnd", "#C7D9F0")
                .cell("FillPattern", "1")
                .cell("LineColor", "#2F5597")
                .cell("LinePattern", "1")
                .cell("LineWeight", "0.0104")
                .text(&format!("Step {id}"))
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.1");
            for (i, (kind, px, py)) in
                [("MoveTo", 0.0, 0.0), ("LineTo", 1.2, 0.0), ("LineTo", 1.2, 0.6), ("LineTo", 0.0, 0.6), ("LineTo", 0.0, 0.0)].into_iter().enumerate()
            {
                let key = RowKey::Ix(i as u32);
                sb = sb.row_type("Geometry", Some(0), key.clone(), kind).row_cell("Geometry", Some(0), key.clone(), "X", &px.to_string());
                sb = sb.row_cell("Geometry", Some(0), key, "Y", &py.to_string());
            }
            page = page.shape(sb);
            id += 1;
            if c + 1 < columns {
                let mut line = ShapeBuilder::new(id)
                    .cell("PinX", &(x + 1.2).to_string())
                    .cell("PinY", &(y + 0.3).to_string())
                    .cell("Width", "0.4")
                    .cell("Height", "0")
                    .cell("LocPinX", "0")
                    .cell("LocPinY", "0")
                    .cell("LineColor", "#2F5597")
                    .cell("LinePattern", "1")
                    .cell("EndArrow", "4");
                for (i, (kind, px)) in [("MoveTo", 0.0), ("LineTo", 0.4)].into_iter().enumerate() {
                    let key = RowKey::Ix(i as u32);
                    line = line.row_type("Geometry", Some(0), key.clone(), kind).row_cell("Geometry", Some(0), key.clone(), "X", &px.to_string());
                    line = line.row_cell("Geometry", Some(0), key, "Y", "0");
                }
                page = page.shape(line);
                id += 1;
            }
        }
    }
    DocBuilder::new()
        .root(|r| {
            r.cell("FillPattern", "1")
                .cell("FillForegnd", "1")
                .cell("LinePattern", "1")
                .cell("LineColor", "0")
                .cell("LineWeight", "0.01")
                .cell("ShdwPattern", "0")
        })
        .page(1, page)
        .build()
}

/// The least time the worker took to draw `doc` whole in 1800 by 1200 pixels, of `frames` frames.
fn least_took(doc: Document, scale: f64, frames: u32) -> Duration {
    let doc = Arc::new(doc);
    let mut worker = RenderWorker::inline(bundled_drawer());
    let view = View { to_px: Affine::new([scale, 0.0, 0.0, -scale, 0.0, 1200.0]), width: 1800, height: 1200 };
    let mut least = Duration::MAX;
    for serial in 1..=u64::from(frames) {
        worker.submit(Job { serial, doc: Arc::clone(&doc), page: 1, view, limits: Limits::default(), supersedes: true, cancel: None, sheet: None });
        for done in worker.poll() {
            assert!(done.result.is_ok(), "the frame is drawn");
            least = least.min(done.took);
        }
    }
    least
}

#[test]
#[ignore = "a timing, run by hand"]
fn render_worker_frame_time_on_pages_of_thousands_of_shapes() {
    let threads = std::env::var("DIACRAFT_THREADS").unwrap_or_else(|_| "(unset)".to_owned());
    let small = least_took(diagram(36, 30, 60.0, 40.0), 30.0, 8);
    let big = least_took(diagram(80, 75, 129.0, 83.5), 14.0, 8);
    eprintln!("WORKER FRAME: DIACRAFT_THREADS={threads}: 2,130 shapes {small:?}, 11,925 shapes {big:?} (least of 8 frames each, `Done::took`)");
}
