//! Black and white (the owner's report of 2026-11-21: the preview stayed in colour): a sheet drawn with
//! [`PrintColor::Grayscale`] has every colour as the grey of its luma (`diacraft_color::luma`, BT.601's weights), as pixels and as what a
//! target is told, and the colours come back with [`PrintColor::Color`].

use std::sync::Arc;

use diacraft_color::{Rgba, luma};
use diacraft_doc::{Document, RowKey};
use diacraft_render::print::{PrintColor, Sheet, draw_sheet, plan_job};
use diacraft_render::{Paint, Pixmap, Renderer, SkipKind, Target, TextRun};
use diacraft_testkit::build::ShapeBuilder;
use kurbo::{Affine, BezPath, Stroke};

use crate::pictures::{foreign, picture};
use crate::support::*;

/// A PNG of four quadrants: red, green, blue and yellow.
fn quadrants() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a header");
        let data: Vec<u8> = (0..8u32)
            .flat_map(|y| (0..8u32).map(move |x| (x, y)))
            .flat_map(|(x, y)| match (x < 4, y < 4) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 0, 255],
            })
            .collect();
        writer.write_image_data(&data).expect("pixels");
    }
    out
}

/// A page of every kind of colour: a red box with a blue line and a shadow, a gradient from red to blue, green words, and a picture.
fn colourful() -> Document {
    let boxed = rect(2, 0.5, 2.5, 1.2, 1.0)
        .cell("FillForegnd", "#FF0000")
        .cell("LineColor", "#0000FF")
        .cell("LineWeight", "0.05")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.1")
        .cell("ShapeShdwOffsetY", "-0.1")
        .cell("ShdwForegnd", "#FF00FF")
        .cell("ShapeShdwBlur", "0.05");
    let mut gradient = rect(3, 2.0, 2.5, 1.5, 1.0)
        .cell("LinePattern", "0")
        .cell("FillGradientEnabled", "1")
        .cell("FillGradientDir", "0")
        .cell("FillGradientAngle", "0");
    for (i, (color, pos)) in [("#FF0000", "0"), ("#0000FF", "1")].iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        gradient = gradient
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", pos);
    }
    let words: ShapeBuilder = rect(4, 0.5, 1.2, 3.0, 1.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text("Green words")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.3")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#00AA00");
    let pic = picture(0.5, 0.2, 0.8, 0.8, foreign("Bitmap", "image/png", quadrants()));
    base().page(1, page(4.0, 4.0).shape(boxed).shape(gradient).shape(words).shape(pic)).build()
}

fn first_sheet(doc: &Document) -> Sheet {
    plan_job(doc, &[1]).expect("plans").sheet(0).map(|(_, s)| s.clone()).expect("a sheet")
}

/// How far apart a premultiplied pixel's red, green and blue are.
fn spread(&[r, g, b, _]: &[u8; 4]) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

fn coloured(pixels: &[u8]) -> usize {
    pixels.as_chunks::<4>().0.iter().filter(|p| spread(p) > 0).count()
}

const PPI: f64 = 40.0;

#[test]
fn a_sheet_in_black_and_white_is_every_pixel_of_the_colour_sheet_as_its_grey() {
    let doc = colourful();
    let sheet = first_sheet(&doc);
    let mut renderer = Renderer::new();
    assert_eq!(renderer.print_color(), PrintColor::Color, "colour unless asked");
    let colour = renderer.render_sheet(&doc, &sheet, PPI);
    assert!(colour.skipped.iter().all(|s| s.kind != SkipKind::Recovered), "{:?}", colour.skipped);
    assert!(coloured(&colour.rgba_premul) > 1_000, "the colour sheet has colour: {}", coloured(&colour.rgba_premul));
    renderer.set_print_color(PrintColor::Grayscale);
    let grey = renderer.render_sheet(&doc, &sheet, PPI);
    assert_eq!((grey.width, grey.height), (colour.width, colour.height));
    assert_eq!(coloured(&grey.rgba_premul), 0, "no pixel of the sheet is coloured");
    // Pixel for pixel the colour sheet's luma, alpha kept: the shapes, the shadow's blur and the picture are all there.
    for (i, (c, g)) in colour.rgba_premul.as_chunks::<4>().0.iter().zip(grey.rgba_premul.as_chunks::<4>().0).enumerate() {
        let y = luma(c[0], c[1], c[2]);
        assert_eq!(*g, [y, y, y, c[3]], "pixel {i}: {c:?} became {g:?}");
    }
    // The red box (0.299 * 255 = 76.245) is the grey 76.
    let (px, py) = (((0.25 + 1.1) * PPI) as usize, ((0.25 + 4.0 - 3.0) * PPI) as usize);
    let at = (py * grey.width as usize + px) * 4;
    assert_eq!(&grey.rgba_premul[at..at + 4], &[76, 76, 76, 255], "the red box's grey");
    // Colour again: the sheet is as it was.
    renderer.set_print_color(PrintColor::Color);
    assert_eq!(renderer.render_sheet(&doc, &sheet, PPI).rgba_premul, colour.rgba_premul);
}

/// Every colour a target is told, and the pixels of every picture.
#[derive(Default)]
struct Colours {
    paints: Vec<Paint>,
    texts: Vec<Rgba>,
    pictures: Vec<Vec<u8>>,
    open: usize,
}

impl Target for Colours {
    fn to_px(&self) -> Affine {
        Affine::new([72.0, 0.0, 0.0, 72.0, 0.0, 0.0])
    }
    fn size(&self) -> (f64, f64) {
        (8.5 * 72.0, 11.0 * 72.0)
    }
    fn fill(&mut self, _: &BezPath, paint: &Paint, _: bool) {
        self.paints.push(paint.clone());
    }
    fn fill_with_paint_transform(&mut self, _: &BezPath, paint: &Paint, _: bool, _: Affine) {
        self.paints.push(paint.clone());
    }
    fn stroke(&mut self, _: &BezPath, _: &Stroke, paint: &Paint) {
        self.paints.push(paint.clone());
    }
    fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
        draw(self);
    }
    fn push_clip(&mut self, _: &BezPath) {
        self.open += 1;
    }
    fn pop_clip(&mut self) {
        self.open = self.open.saturating_sub(1);
    }
    fn image(&mut self, image: &Arc<Pixmap>, _: Affine) {
        self.pictures.push(image.data_as_u8_slice().to_vec());
    }
    fn mark(&self) -> usize {
        self.open
    }
    fn rewind(&mut self, mark: usize) {
        self.open = mark;
    }
    fn wants_text(&self) -> bool {
        true
    }
    fn text_run(&mut self, run: &TextRun<'_>) -> bool {
        self.texts.push(run.color);
        true
    }
}

impl Colours {
    /// Every colour told, the stops of gradients included.
    fn all(&self) -> Vec<Rgba> {
        let mut all = self.texts.clone();
        for p in &self.paints {
            match p {
                Paint::Solid(c) => all.push(*c),
                Paint::Gradient(g) => all.extend(g.stops.iter().map(|s| s.color)),
            }
        }
        all
    }
}

fn is_grey(c: &Rgba) -> bool {
    c.r == c.g && c.g == c.b
}

#[test]
fn a_target_is_told_only_greys_in_black_and_white_and_the_same_things_as_in_colour() {
    let doc = colourful();
    let sheet = first_sheet(&doc);
    let mut renderer = Renderer::new();
    let mut colour = Colours::default();
    draw_sheet(&mut renderer, &doc, &sheet, &mut colour).expect("draws");
    assert!(colour.all().iter().any(|c| *c == Rgba::rgb(255, 0, 0)), "red is told in colour");
    assert!(colour.texts.contains(&Rgba::rgb(0, 0xAA, 0)), "the words are green");
    assert!(colour.paints.iter().any(|p| matches!(p, Paint::Gradient(_))), "a gradient is told");
    assert_eq!(colour.pictures.len(), 1, "the picture");
    assert!(coloured(&colour.pictures[0]) > 0);
    renderer.set_print_color(PrintColor::Grayscale);
    let mut grey = Colours::default();
    let skipped = draw_sheet(&mut renderer, &doc, &sheet, &mut grey).expect("draws");
    let all = grey.all();
    assert!(all.iter().all(is_grey), "only greys: {:?}", all.iter().filter(|c| !is_grey(c)).collect::<Vec<_>>());
    assert_eq!(grey.texts, vec![Rgba::rgb(100, 100, 100)], "the words are green's grey: 0.587 * 170 = 99.79");
    assert!(all.contains(&Rgba::rgb(76, 76, 76)) && all.contains(&Rgba::rgb(29, 29, 29)), "red's and blue's greys");
    assert_eq!(grey.pictures.len(), 1);
    assert_eq!(coloured(&grey.pictures[0]), 0, "the picture's pixels are greys");
    let expected: Vec<u8> = colour.pictures[0]
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let y = luma(p[0], p[1], p[2]);
            [y, y, y, p[3]]
        })
        .collect();
    assert_eq!(grey.pictures[0], expected, "each pixel its luma, alpha kept");
    // Nothing is left out for it: as many things are told, in the same kinds.
    assert_eq!((grey.paints.len(), grey.texts.len()), (colour.paints.len(), colour.texts.len()));
    // A target that is told greys draws no blur: the shadow is drawn sharp, and said to be.
    assert!(skipped.iter().any(|s| s.kind == SkipKind::ShadowApproximate), "{skipped:?}");
    assert_eq!(grey.open, 0, "every clip is closed");
    assert_eq!(renderer.print_color(), PrintColor::Grayscale, "the renderer's setting is the caller's");
}
