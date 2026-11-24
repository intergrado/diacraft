//! Strokes thinner than a device pixel (a project rule).
//! A raster target draws one a pixel wide in the line's own colour, a writer of SVG or PDF keeps its true width, the dashes keep
//! the line's pattern as it is seen, and the line ends keep their size.

use std::ops::Range;
use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_render::export::{export_png, unpremultiply};
use diacraft_render::{Paint, Pixmap, Rendered, Renderer, Target, View};
use diacraft_testkit::build::ShapeBuilder;
use kurbo::{Affine, BezPath, Rect, Shape as _, Stroke};

use crate::support::*;

/// One point in paper inches.
const PT: f64 = 1.0 / 72.0;

/// The paper height at which a line is centred on the middle of pixel row 100 at 96 pixels to the inch on a 4 inch page.
const MIDDLE_OF_ROW_100: f64 = 4.0 - 100.5 / 96.0;

/// The paper height at which a line is centred on the edge between pixel rows 99 and 100 at 96 pixels to the inch.
fn edge_of_row_100() -> f64 {
    4.0 - 100.0 / 96.0
}

/// A horizontal black line 3 inches long from x = 0.5 at paper height `y`, no fill, of this weight (inches), pattern and cap.
fn hline(y: f64, weight: f64, pattern: u32, cap: u32) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, y, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", &pattern.to_string())
        .cell("LineCap", &cap.to_string())
        .cell("LineColor", "#000000")
}

fn render(doc: &Document, ppi: f64) -> Rendered {
    let view = View::fit_page(doc, 1, ppi).expect("the page has a size");
    Renderer::new().render(doc, 1, &view)
}

fn rgba(r: &Rendered, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * r.width + x) * 4) as usize;
    [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]]
}

/// How much of a pixel black ink covers, 0 for the white paper and 1 for black, read from the red channel.
fn ink(r: &Rendered, x: u32, y: u32) -> f64 {
    1.0 - f64::from(rgba(r, x, y)[0]) / 255.0
}

/// The ink of a column over these rows, in pixels: how wide a horizontal line is there.
fn column_ink(r: &Rendered, x: u32, rows: Range<u32>) -> f64 {
    rows.map(|y| ink(r, x, y)).sum()
}

/// The ink of a row over these columns, in pixels: how long the marks of a horizontal line are there.
fn row_ink(r: &Rendered, y: u32, columns: Range<u32>) -> f64 {
    columns.map(|x| ink(r, x, y)).sum()
}

#[test]
fn a_line_of_a_quarter_point_is_one_pixel_wide_and_black_at_96_dpi() {
    // 0.24 points is 0.32 pixel at 96 to the inch. Drawn at its true width it would be a grey of a third.
    let r = render(&one_page(vec![hline(MIDDLE_OF_ROW_100, 0.24 * PT, 1, 0)]), 96.0);
    for x in [100, 200, 300] {
        assert!(ink(&r, x, 100) >= 0.99, "the row the line is centred on is black: {:?}", rgba(&r, x, 100));
        assert!(ink(&r, x, 99) <= 0.01 && ink(&r, x, 101) <= 0.01, "and the rows beside it are paper");
    }
}

#[test]
fn a_thin_line_covers_one_pixel_in_all_wherever_it_falls() {
    // Not snapped to the pixel grid: a line centred on the edge of two rows is half on each, and the sum is still one pixel.
    for row in [100.0, 100.25, 100.5, 100.8] {
        let y = 4.0 - row / 96.0;
        let r = render(&one_page(vec![hline(y, 0.24 * PT, 1, 0)]), 96.0);
        let wide = column_ink(&r, 200, 94..108);
        assert!((wide - 1.0).abs() < 0.04, "centred on row {row}: {wide} pixels wide");
    }
    let r = render(&one_page(vec![hline(edge_of_row_100(), 0.24 * PT, 1, 0)]), 96.0);
    assert!(
        (ink(&r, 200, 99) - 0.5).abs() < 0.05 && (ink(&r, 200, 100) - 0.5).abs() < 0.05,
        "half and half: {} {}",
        ink(&r, 200, 99),
        ink(&r, 200, 100)
    );
}

#[test]
fn a_thin_line_and_a_line_of_exactly_one_pixel_draw_the_same_pixels() {
    // The rule is "a pixel wide", not "something near": the frame of a 0.24 point line is the frame of a line whose weight is 1/96 inch.
    for y in [MIDDLE_OF_ROW_100, 4.0 - 100.3 / 96.0] {
        let thin = render(&one_page(vec![hline(y, 0.24 * PT, 1, 0)]), 96.0);
        let one = render(&one_page(vec![hline(y, 1.0 / 96.0, 1, 0)]), 96.0);
        assert_eq!(thin.rgba_premul, one.rgba_premul);
    }
}

#[test]
fn a_line_a_pixel_wide_or_wider_is_not_changed() {
    // 2 points are 2.667 pixels and 1.125 points are 1.5: the width is the weight, as before.
    for (weight, pixels) in [(2.0 * PT, 2.0 * 96.0 / 72.0), (1.125 * PT, 1.5), (3.0 * PT, 4.0)] {
        for y in [MIDDLE_OF_ROW_100, edge_of_row_100(), 4.0 - 100.3 / 96.0] {
            let r = render(&one_page(vec![hline(y, weight, 1, 0)]), 96.0);
            let wide = column_ink(&r, 200, 90..112);
            assert!((wide - pixels).abs() < 0.05, "weight {weight} is {pixels} pixels, got {wide}");
        }
    }
}

#[test]
fn the_colour_and_the_see_through_share_of_the_line_stay_its_own_however_thin_it_is() {
    // Full opacity: the widening does not fade the colour, and a line that says 50% clear is that and no clearer.
    let red = hline(MIDDLE_OF_ROW_100, 0.24 * PT, 1, 0).cell("LineColor", "#FF0000");
    let r = render(&one_page(vec![red]), 96.0);
    assert_eq!(rgba(&r, 200, 100), [255, 0, 0, 255]);
    let clear = hline(MIDDLE_OF_ROW_100, 0.24 * PT, 1, 0).cell("LineColor", "#FF0000").cell("LineColorTrans", "0.5");
    let r = render(&one_page(vec![clear]), 96.0);
    let p = rgba(&r, 200, 100);
    assert!(p[0] >= 252 && (125..=131).contains(&p[1]) && (125..=131).contains(&p[2]), "half red over white paper: {p:?}");
}

#[test]
fn a_zero_weight_is_one_pixel_at_every_zoom() {
    // The hairline is one device pixel at any zoom on screen.
    for ppi in [24.0, 96.0, 300.0] {
        let y = 4.0 - (2.0 * ppi + 0.5) / ppi;
        let r = render(&one_page(vec![hline(y, 0.0, 1, 0)]), ppi);
        let centre = (2.0 * ppi) as u32;
        let wide = column_ink(&r, (2.0 * ppi) as u32, centre - 6..centre + 7);
        assert!((wide - 1.0).abs() < 0.05, "at {ppi} pixels to the inch: {wide}");
    }
}

#[test]
fn a_dashed_thin_line_keeps_the_marks_of_its_own_pattern_not_a_pixel_wide_ones() {
    // Pattern 16 is a mark of 15 weights and a gap of 9. At 0.24 points and 96 pixels to the inch a weight is 0.32 pixel, so a
    // period is 7.68 pixels and the three inches (288 pixels) hold about 37 marks. Dashes laid out in the 1 pixel the line is drawn at would
    // be 24 pixels to the period: 12 marks.
    for cap in [0, 1] {
        let r = render(&one_page(vec![hline(MIDDLE_OF_ROW_100, 0.24 * PT, 16, cap)]), 96.0);
        let mut marks = 0;
        let mut inside = false;
        for x in 50..334 {
            let dark = ink(&r, x, 100) > 0.5;
            if dark && !inside {
                marks += 1;
            }
            inside = dark;
        }
        assert!((36..=39).contains(&marks), "cap {cap}: {marks} marks");
    }
}

#[test]
fn a_dashed_thin_line_shows_marks_as_long_as_the_pattern_says() {
    // A flat cap adds nothing to a mark, so its ink is the pattern's: 16 weights of every 24, over 288 pixels.
    let r = render(&one_page(vec![hline(MIDDLE_OF_ROW_100, 0.24 * PT, 16, 1)]), 96.0);
    let inked = row_ink(&r, 100, 0..384);
    let expected = 288.0 * 16.0 / 24.0;
    assert!((inked - expected).abs() < 8.0, "the marks add up to {inked} pixels of ink, the pattern says {expected}");
}

#[test]
fn the_window_and_the_png_draw_the_same_thin_line() {
    // The window draws through `Renderer::render` and a view, as the PNG does: the same pixels, and a panned view puts the same line on the
    // shifted row.
    let doc = one_page(vec![hline(MIDDLE_OF_ROW_100, 0.24 * PT, 1, 0)]);
    let mut renderer = Renderer::new();
    let view = View::fit_page(&doc, 1, 96.0).expect("a size");
    let window = renderer.render(&doc, 1, &view);
    let out = export_png(&mut renderer, &doc, 1, 96.0).expect("exports");
    let mut reader = png::Decoder::new(std::io::Cursor::new(out.bytes.as_slice())).read_info().expect("a PNG");
    let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
    reader.next_frame(&mut pixels).expect("pixels");
    assert_eq!(pixels, unpremultiply(&window.rgba_premul), "the PNG is the window's picture");
    assert!(ink(&window, 200, 100) >= 0.99 && ink(&window, 200, 99) <= 0.01);
    // Panned by a whole number of pixels: the line is on row 50 now, as wide and as dark.
    let panned = View { to_px: Affine::translate((-40.0, -50.0)) * view.to_px, width: 300, height: 300 };
    let moved = renderer.render(&doc, 1, &panned);
    assert!(ink(&moved, 100, 50) >= 0.99 && ink(&moved, 100, 49) <= 0.01 && ink(&moved, 100, 51) <= 0.01, "{:?}", rgba(&moved, 100, 50));
}

/// A target that keeps the strokes and the fills it is told, on a 4 inch page at `ppi` pixels to the inch, with a minimum stroke it names.
struct Probe {
    to_px: Affine,
    size: (f64, f64),
    min_stroke: f64,
    strokes: Vec<Stroke>,
    fills: Vec<Rect>,
    skipped: Vec<diacraft_render::Skipped>,
}

impl Probe {
    /// What the walk tells a target of this resolution and minimum, with nothing left out.
    fn draw(doc: &Document, ppi: f64, min_stroke: f64) -> Probe {
        let probe = Probe::draw_loose(doc, ppi, min_stroke);
        assert!(probe.skipped.is_empty(), "{:?}", probe.skipped);
        probe
    }

    /// The same, for a page that reports something.
    fn draw_loose(doc: &Document, ppi: f64, min_stroke: f64) -> Probe {
        let mut probe = Probe {
            to_px: Affine::new([ppi, 0.0, 0.0, -ppi, 0.0, 4.0 * ppi]),
            size: (4.0 * ppi, 4.0 * ppi),
            min_stroke,
            strokes: Vec::new(),
            fills: Vec::new(),
            skipped: Vec::new(),
        };
        probe.skipped = Renderer::new().draw_onto(doc, 1, &mut probe);
        probe
    }

    /// The widths of the strokes, in the order they were told.
    fn widths(&self) -> Vec<f64> {
        self.strokes.iter().map(|s| s.width).collect()
    }
}

impl Target for Probe {
    fn to_px(&self) -> Affine {
        self.to_px
    }
    fn size(&self) -> (f64, f64) {
        self.size
    }
    fn min_stroke(&self) -> f64 {
        self.min_stroke
    }
    fn fill(&mut self, path: &BezPath, _: &Paint, _: bool) {
        self.fills.push(path.bounding_box());
    }
    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, _: Affine) {
        self.fill(path, paint, even_odd);
    }
    fn stroke(&mut self, _: &BezPath, stroke: &Stroke, _: &Paint) {
        self.strokes.push(stroke.clone());
    }
    fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
        draw(self);
    }
    fn push_clip(&mut self, _: &BezPath) {}
    fn pop_clip(&mut self) {}
    fn image(&mut self, _: &Arc<Pixmap>, _: Affine) {}
    fn mark(&self) -> usize {
        0
    }
    fn rewind(&mut self, _: usize) {}
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_target_that_writes_a_file_is_told_the_true_width_however_thin() {
    // A reader of an SVG or a PDF rasterises it at a resolution of its own: the file holds the width the line has.
    let doc = one_page(vec![hline(2.0, 0.24 * PT, 1, 0)]);
    assert!(near(Probe::draw(&doc, 96.0, 0.0).widths()[0], 0.24 * PT));
    assert!(near(Probe::draw(&doc, 72.0, 0.0).widths()[0], 0.24 * PT));
}

#[test]
fn a_target_that_rasterises_is_told_a_pixel_when_the_line_is_thinner_and_the_true_width_when_it_is_not() {
    let doc = one_page(vec![hline(2.0, 0.24 * PT, 1, 0)]);
    assert!(near(Probe::draw(&doc, 96.0, 1.0).widths()[0], 1.0 / 96.0), "0.32 pixel is drawn as 1");
    assert!(near(Probe::draw(&doc, 300.0, 1.0).widths()[0], 1.0 / 300.0), "1 pixel at 300 to the inch is 0.24 points less");
    assert!(near(Probe::draw(&doc, 600.0, 1.0).widths()[0], 0.24 * PT), "2 pixels at 600 to the inch: the weight stands");
    // A stroke of 2 pixels is not 1.
    let wide = one_page(vec![hline(2.0, 0.24 * PT * 6.0, 1, 0)]);
    assert!(near(Probe::draw(&wide, 96.0, 1.0).widths()[0], 1.44 * PT));
}

#[test]
fn a_zero_weight_is_a_pixel_on_a_raster_and_a_quarter_point_in_a_file() {
    // One device pixel at any zoom on screen, and 0.25 points in a vector export.
    let doc = one_page(vec![hline(2.0, 0.0, 1, 0)]);
    assert!(near(Probe::draw(&doc, 96.0, 0.0).widths()[0], 0.25 * PT), "a file");
    assert!(near(Probe::draw(&doc, 72.0, 0.0).widths()[0], 0.25 * PT), "a file in points");
    assert!(near(Probe::draw(&doc, 96.0, 1.0).widths()[0], 1.0 / 96.0), "a raster");
    assert!(near(Probe::draw(&doc, 300.0, 1.0).widths()[0], 1.0 / 300.0), "a raster, zoomed in");
}

#[test]
fn the_dashes_of_a_thin_line_are_laid_out_in_its_own_weight_and_the_caps_do_not_lengthen_them() {
    // Pattern 2 under a round cap is a list of 7 and 5 weights, seen as marks of 8 and gaps of 4. The line is drawn a pixel wide
    // (1/96 inch) and is 0.24 points (u) in weight: the cap adds a pixel to a mark, so the list gives that pixel back, and the period is
    // still 12 weights. Marks and gaps are as long as the pattern makes them.
    let u = 0.24 * PT;
    let d = 1.0 / 96.0;
    let doc = one_page(vec![hline(2.0, u, 2, 0)]);
    let file = Probe::draw(&doc, 96.0, 0.0);
    assert!(near(file.strokes[0].dash_pattern[0], 7.0 * u) && near(file.strokes[0].dash_pattern[1], 5.0 * u), "{:?}", file.strokes[0].dash_pattern);
    let pixels = Probe::draw(&doc, 96.0, 1.0);
    let list = &pixels.strokes[0].dash_pattern;
    assert_eq!(list.len(), 2);
    assert!(near(list[0] + d, 8.0 * u), "a mark is seen as 8 weights: {list:?}");
    assert!(near(list[1] - d, 4.0 * u), "a gap is seen as 4: {list:?}");
    assert!(near(list[0] + list[1], 12.0 * u), "the period is 12 weights");
    // A flat cap adds nothing: the list is the flat form whatever the width drawn.
    let flat = one_page(vec![hline(2.0, u, 2, 1)]);
    let list = &Probe::draw(&flat, 96.0, 1.0).strokes[0].dash_pattern;
    assert!(near(list[0], 8.0 * u) && near(list[1], 4.0 * u), "{list:?}");
}

#[test]
fn a_dot_of_a_thin_line_is_a_dot_of_the_pixel_it_is_drawn_at_and_the_period_is_kept() {
    // Pattern 3 is a dot and a gap of 5 weights. The dot is drawn as wide as the line is (a pixel), and the dots stay 5 weights apart.
    let u = 0.24 * PT;
    let doc = one_page(vec![hline(2.0, u, 3, 0)]);
    let list = &Probe::draw(&doc, 96.0, 1.0).strokes[0].dash_pattern;
    assert!(list[0] < u * 0.01, "no length of its own: {list:?}");
    assert!((list[0] + list[1] - 5.0 * u).abs() < u * 0.01, "{list:?}");
}

#[test]
fn a_line_end_keeps_its_size_whatever_the_line_is_drawn_at() {
    // S is the size class's base plus the weight, on paper. A raster draws the shaft a pixel wide; the head is the same head.
    let u = 0.24 * PT;
    let arrow = hline(2.0, u, 1, 0).cell("EndArrow", "4").cell("EndArrowSize", "2");
    let doc = one_page(vec![arrow]);
    let file = Probe::draw(&doc, 96.0, 0.0);
    let pixels = Probe::draw(&doc, 96.0, 1.0);
    assert_eq!(file.fills.len(), 2, "the paper and a head: {:?}", file.fills);
    assert_eq!(file.fills, pixels.fills, "the same head");
    let head = file.fills[1];
    let s = 0.035 + u;
    assert!((head.width() - 2.0 * s).abs() < 1e-6 && (head.height() - 2.0 * s).abs() < 1e-6, "2 units of {s}: {head:?}");
}

#[test]
fn the_unit_of_a_line_end_takes_the_weight_zero_at_every_zoom() {
    // "S taking weight 0". The head of a line of weight 0 is the size class's base, and does not grow as the view shrinks.
    let arrow = hline(2.0, 0.0, 1, 0).cell("EndArrow", "4").cell("EndArrowSize", "2");
    let doc = one_page(vec![arrow]);
    for (ppi, min_stroke) in [(24.0, 1.0), (100.0, 1.0), (1000.0, 1.0), (96.0, 0.0)] {
        let head = Probe::draw(&doc, ppi, min_stroke).fills[1];
        assert!((head.width() - 0.07).abs() < 1e-6 && (head.height() - 0.07).abs() < 1e-6, "at {ppi}: {head:?}");
    }
}

#[test]
fn the_hollow_mark_of_a_thin_line_is_stroked_a_pixel_wide_too() {
    // Number 16 is a hollow head: its outline is a stroke, at the width the shaft is drawn.
    let arrow = hline(2.0, 0.24 * PT, 1, 0).cell("EndArrow", "16");
    let doc = one_page(vec![arrow]);
    let widths = Probe::draw(&doc, 96.0, 1.0).widths();
    assert!(widths.len() >= 2 && widths.iter().all(|w| near(*w, 1.0 / 96.0)), "{widths:?}");
    let widths = Probe::draw(&doc, 96.0, 0.0).widths();
    assert!(widths.iter().all(|w| near(*w, 0.24 * PT)), "{widths:?}");
}

#[test]
fn the_shadow_of_a_thin_line_is_as_wide_as_the_line() {
    let boxed = rect(1, 1.0, 1.0, 2.0, 1.0)
        .cell("FillPattern", "0")
        .cell("LineWeight", &(0.24 * PT).to_string())
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.1")
        .cell("ShapeShdwOffsetY", "-0.1")
        .cell("ShdwForegnd", "#808080");
    let doc = one_page(vec![boxed]);
    let pixels = Probe::draw(&doc, 96.0, 1.0).widths();
    assert_eq!(pixels.len(), 2, "the shadow's outline and the outline");
    assert!(pixels.iter().all(|w| near(*w, 1.0 / 96.0)), "{pixels:?}");
    let file = Probe::draw(&doc, 96.0, 0.0).widths();
    assert!(file.iter().all(|w| near(*w, 0.24 * PT)), "{file:?}");
}

#[test]
fn the_placeholder_of_a_picture_that_is_not_drawn_is_stroked_a_pixel_wide_at_least() {
    // A hundredth of an inch is half a pixel at 50 to the inch.
    let noise: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
    let doc = one_page(vec![crate::pictures::picture(1.0, 1.0, 2.0, 2.0, crate::pictures::foreign("Bitmap", "image/png", noise))]);
    let pixels = Probe::draw_loose(&doc, 50.0, 1.0).widths();
    assert!(pixels.len() >= 2 && pixels.iter().all(|w| near(*w, 0.02)), "{pixels:?}");
    let file = Probe::draw_loose(&doc, 50.0, 0.0).widths();
    assert!(file.iter().all(|w| near(*w, 0.01)), "{file:?}");
}
