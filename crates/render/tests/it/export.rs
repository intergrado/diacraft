//! The drawing target seam and the PNG export: the walk of a page is the same for every output, so a target that only records what it
//! is told sees what the window would draw, and a PNG is that drawing in pixels.

use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_render::export::{ExportError, PageLayout, export_png};
use diacraft_render::{Paint, Pixmap, Renderer, SkipKind, Target, TextRun};
use diacraft_testkit::build::ShapeBuilder;
use kurbo::{Affine, BezPath, Shape as _, Stroke};

use crate::support::*;

/// What a target was told, in order.
#[derive(Clone, Debug)]
enum Op {
    Fill { bbox: kurbo::Rect, paint: Paint, even_odd: bool },
    Stroke { width: f64, paint: Paint },
    Clip,
    Unclip,
    Image,
    Text { text: String, glyphs: usize, size: f64, color: Rgba, face: String },
}

/// A target that keeps what it is told: paper inches to hundredths of a unit, so the numbers read as the page does.
struct Recorder {
    to_px: Affine,
    size: (f64, f64),
    ops: Vec<Op>,
    open: usize,
    text: Option<bool>,
}

impl Recorder {
    fn new(w: f64, h: f64, text: Option<bool>) -> Recorder {
        Recorder { to_px: Affine::new([100.0, 0.0, 0.0, -100.0, 0.0, h * 100.0]), size: (w * 100.0, h * 100.0), ops: Vec::new(), open: 0, text }
    }
}

impl Target for Recorder {
    fn to_px(&self) -> Affine {
        self.to_px
    }
    fn size(&self) -> (f64, f64) {
        self.size
    }
    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool) {
        self.ops.push(Op::Fill { bbox: path.bounding_box(), paint: paint.clone(), even_odd });
    }
    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, _: Affine) {
        self.fill(path, paint, even_odd);
    }
    fn stroke(&mut self, _: &BezPath, stroke: &Stroke, paint: &Paint) {
        self.ops.push(Op::Stroke { width: stroke.width, paint: paint.clone() });
    }
    fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
        draw(self);
    }
    fn push_clip(&mut self, _: &BezPath) {
        self.open += 1;
        self.ops.push(Op::Clip);
    }
    fn pop_clip(&mut self) {
        self.open = self.open.saturating_sub(1);
        self.ops.push(Op::Unclip);
    }
    fn image(&mut self, _: &Arc<Pixmap>, _: Affine) {
        self.ops.push(Op::Image);
    }
    fn mark(&self) -> usize {
        self.open
    }
    fn rewind(&mut self, mark: usize) {
        self.open = mark;
    }
    fn wants_text(&self) -> bool {
        self.text.is_some()
    }
    fn text_run(&mut self, run: &TextRun<'_>) -> bool {
        if self.text != Some(true) {
            return false;
        }
        let first = run.glyphs.first().map_or(0, |g| g.chars.start);
        let last = run.glyphs.last().map_or(0, |g| g.chars.end);
        let text = run.source.slice(&(first..last)).unwrap_or("?").to_owned();
        self.ops.push(Op::Text { text, glyphs: run.glyphs.len(), size: run.size, color: run.color, face: run.face.family().to_owned() });
        true
    }
}

fn red_box() -> ShapeBuilder {
    rect(1, 1.0, 1.0, 2.0, 1.0).cell("FillForegnd", "#FF0000").cell("LineColor", "#0000FF").cell("LineWeight", "0.05")
}

fn label(text: &str) -> ShapeBuilder {
    use diacraft_doc::RowKey;
    rect(2, 0.5, 2.5, 3.0, 1.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.3")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#00AA00")
}

#[test]
fn a_target_that_only_records_is_told_the_fill_and_the_outline_of_a_shape_in_paper_inches() {
    let doc = one_page(vec![red_box()]);
    let mut target = Recorder::new(4.0, 4.0, None);
    let skipped = Renderer::new().draw_onto(&doc, 1, &mut target);
    assert!(skipped.is_empty(), "{skipped:?}");
    let fills: Vec<_> =
        target.ops.iter().filter_map(|o| if let Op::Fill { bbox, paint, even_odd } = o { Some((bbox, paint, even_odd)) } else { None }).collect();
    // The paper first (white, the whole page), then the shape's red fill over [1, 3] x [1, 2].
    assert_eq!(fills.len(), 2, "{:?}", target.ops);
    assert_eq!(fills[0].1, &Paint::Solid(Rgba::WHITE));
    assert_eq!((fills[0].0.x0, fills[0].0.y0, fills[0].0.x1, fills[0].0.y1), (0.0, 0.0, 4.0, 4.0));
    assert_eq!(fills[1].1, &Paint::Solid(Rgba::rgb(255, 0, 0)));
    let b = fills[1].0;
    assert!((b.x0 - 1.0).abs() < 1e-9 && (b.x1 - 3.0).abs() < 1e-9 && (b.y0 - 1.0).abs() < 1e-9 && (b.y1 - 2.0).abs() < 1e-9, "{b:?}");
    assert!(*fills[1].2, "a fill by the odd-crossings rule, as the window draws it");
    let strokes: Vec<_> =
        target.ops.iter().filter_map(|o| if let Op::Stroke { width, paint, .. } = o { Some((*width, paint.clone())) } else { None }).collect();
    assert_eq!(strokes, vec![(0.05, Paint::Solid(Rgba::rgb(0, 0, 255)))]);
}

#[test]
fn a_target_without_a_page_gets_the_same_report_the_window_does() {
    let doc = one_page(vec![red_box()]);
    let mut target = Recorder::new(4.0, 4.0, None);
    let skipped = Renderer::new().draw_onto(&doc, 77, &mut target);
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].kind, SkipKind::NoSuchPage);
    assert!(target.ops.is_empty());
}

#[test]
fn letters_go_to_a_target_that_wants_text_with_the_characters_they_stand_for_and_are_not_also_drawn_as_outlines() {
    let doc = one_page(vec![label("Hello")]);
    let mut target = Recorder::new(4.0, 4.0, Some(true));
    let skipped = Renderer::new().draw_onto(&doc, 1, &mut target);
    assert_eq!(skipped.iter().filter(|s| s.kind == SkipKind::TextAsOutlines).count(), 0, "{skipped:?}");
    let texts: Vec<_> = target.ops.iter().filter(|o| matches!(o, Op::Text { .. })).collect();
    assert_eq!(texts.len(), 1, "{:?}", target.ops);
    let Op::Text { text, glyphs, size, color, face } = texts[0] else { panic!("a text op") };
    assert_eq!(text, "Hello");
    assert_eq!(*glyphs, 5);
    assert!((size - 0.3).abs() < 1e-9, "the em is the cell's size in inches: {size}");
    assert_eq!(*color, Rgba::rgb(0, 0xAA, 0));
    assert!(!face.is_empty());
    // Only the paper was filled: the letters left no outline behind.
    assert_eq!(target.ops.iter().filter(|o| matches!(o, Op::Fill { .. })).count(), 1, "{:?}", target.ops);
}

#[test]
fn letters_a_target_declines_are_drawn_as_outlines_and_counted() {
    let doc = one_page(vec![label("Hello")]);
    let mut target = Recorder::new(4.0, 4.0, Some(false));
    let skipped = Renderer::new().draw_onto(&doc, 1, &mut target);
    let counted = skipped.iter().find(|s| s.kind == SkipKind::TextAsOutlines).map(|s| s.count);
    assert_eq!(counted, Some(1), "{skipped:?}");
    assert!(target.ops.iter().filter(|o| matches!(o, Op::Fill { .. })).count() > 1, "the glyphs are filled: {:?}", target.ops.len());
    assert!(!target.ops.iter().any(|o| matches!(o, Op::Text { .. })));
}

#[test]
fn a_target_that_does_not_want_text_is_never_offered_any_and_nothing_is_counted() {
    let doc = one_page(vec![label("Hello")]);
    let mut target = Recorder::new(4.0, 4.0, None);
    let skipped = Renderer::new().draw_onto(&doc, 1, &mut target);
    assert!(skipped.iter().all(|s| s.kind != SkipKind::TextAsOutlines), "{skipped:?}");
    assert!(target.ops.iter().filter(|o| matches!(o, Op::Fill { .. })).count() > 1);
}

#[test]
fn the_png_is_the_drawing_in_pixels_at_the_resolution_asked_and_says_its_physical_size() {
    let doc = one_page(vec![red_box()]);
    let out = export_png(&mut Renderer::new(), &doc, 1, 50.0).expect("exports");
    assert_eq!((out.width, out.height), (200, 200));
    let mut decoder = png::Decoder::new(std::io::Cursor::new(out.bytes.as_slice()));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("a PNG");
    let info = reader.info().clone();
    assert_eq!((info.width, info.height), (200, 200));
    // 50 pixels to the inch is 1,969 to the metre.
    let dims = info.pixel_dims.expect("the physical size is written");
    assert_eq!((dims.xppu, dims.yppu, dims.unit), (1969, 1969, png::Unit::Meter));
    let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
    let frame = reader.next_frame(&mut pixels).expect("pixels");
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    let at = |x: f64, y_up: f64| {
        let (px, py) = ((x * 50.0) as usize, ((4.0 - y_up) * 50.0) as usize);
        let i = (py * 200 + px) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };
    assert_eq!(at(2.0, 1.5), [255, 0, 0, 255], "the box is red");
    assert_eq!(at(0.5, 0.5), [255, 255, 255, 255], "the paper is white");
    assert_eq!(at(1.0, 1.5)[2], 255, "the outline is blue, centred on the edge");
}

#[test]
fn the_png_of_a_page_with_no_paper_is_transparent_where_nothing_is_drawn_and_not_darkened_at_soft_edges() {
    let doc = one_page(vec![red_box().cell("FillForegndTrans", "0.5").cell("LinePattern", "0")]);
    let mut renderer = Renderer::new();
    renderer.set_paper(None);
    let out = export_png(&mut renderer, &doc, 1, 50.0).expect("exports");
    let mut reader = png::Decoder::new(std::io::Cursor::new(out.bytes.as_slice())).read_info().expect("a PNG");
    let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
    reader.next_frame(&mut pixels).expect("pixels");
    let at = |x: f64, y_up: f64| {
        let i = (((4.0 - y_up) * 50.0) as usize * 200 + (x * 50.0) as usize) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };
    assert_eq!(at(0.5, 0.5)[3], 0, "no paper, no ink: clear");
    let inside = at(2.0, 1.5);
    // Half transparent red, written as straight colour: the red stays full, and only the alpha is half.
    assert!(inside[0] >= 253 && inside[1] <= 2 && inside[2] <= 2 && (126..=129).contains(&inside[3]), "{inside:?}");
}

#[test]
fn an_export_that_cannot_be_made_says_why() {
    let doc = one_page(vec![red_box()]);
    let mut renderer = Renderer::new();
    assert_eq!(export_png(&mut renderer, &doc, 9, 96.0).unwrap_err(), ExportError::NoSuchPage(9));
    assert_eq!(export_png(&mut renderer, &doc, 1, 0.0).unwrap_err(), ExportError::BadResolution);
    assert_eq!(export_png(&mut renderer, &doc, 1, f64::NAN).unwrap_err(), ExportError::BadResolution);
    assert!(matches!(export_png(&mut renderer, &doc, 1, 1.0e9).unwrap_err(), ExportError::TooLarge(_)));
    // A page of no size has nothing to export.
    let sizeless = base().page(1, page(0.0, 4.0)).build();
    assert_eq!(export_png(&mut renderer, &sizeless, 1, 96.0).unwrap_err(), ExportError::NoPageSize);
}

#[test]
fn a_page_layout_puts_the_top_left_corner_of_the_page_at_the_origin_and_refuses_a_page_past_the_cap() {
    let doc = one_page(vec![]);
    let layout = PageLayout::of(&doc, 1, 72.0).expect("a layout");
    assert_eq!((layout.width, layout.height), (288.0, 288.0));
    let p = layout.to_px * kurbo::Point::new(0.0, 4.0);
    assert_eq!((p.x, p.y), (0.0, 0.0), "the page's upper left corner");
    let p = layout.to_px * kurbo::Point::new(4.0, 0.0);
    assert_eq!((p.x, p.y), (288.0, 288.0), "and its lower right");
    let huge = base().page(1, page(1.0e9, 1.0)).build();
    assert!(matches!(PageLayout::of(&huge, 1, 72.0), Err(ExportError::TooLarge(_))));
}

/// What an export may spend (no test pinned it, and a mutation run found none to fail on a changed or missing field): a minute for the page, half of it
/// for laying text out and half of it for the labels of placeholders, which have a budget of their own; the counts are a frame's.
#[test]
fn an_export_may_spend_a_minute_and_half_a_minute_each_on_text_and_on_labels() {
    use std::time::Duration;
    let limits = diacraft_render::export::export_limits();
    let window = diacraft_render::Limits::default();
    assert_eq!(
        (limits.frame_time, limits.text_time, limits.label_time),
        (Some(Duration::from_secs(60)), Some(Duration::from_secs(30)), Some(Duration::from_secs(30)))
    );
    // The window's: a frame that pans, with a text time and a label time of their own, and no export's minute.
    assert_eq!(
        (window.frame_time, window.text_time, window.label_time),
        (Some(Duration::from_millis(1_500)), Some(Duration::from_millis(400)), Some(Duration::from_millis(150)))
    );
    // The counts are the same for both.
    assert_eq!(
        (limits.max_shapes, limits.max_rows, limits.max_text_chars, limits.max_label_chars),
        (window.max_shapes, window.max_rows, window.max_text_chars, window.max_label_chars)
    );
    assert_eq!(window.max_label_chars, 100_000);
    // And no time at all for an export that is to draw everything.
    let free = diacraft_render::Limits::unlimited_time();
    assert_eq!((free.frame_time, free.text_time, free.label_time), (None, None, None));
}
