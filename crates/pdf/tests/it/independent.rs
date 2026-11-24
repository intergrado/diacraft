//! An independent reader: poppler's `pdftotext`, `pdftoppm` and `pdffonts`, where the machine has them. A machine without them skips these
//! tests and says so; the in-process reader (`readback`) covers the same ground everywhere. Nothing here is a reference picture from the reference app: the pages are
//! built in code, and the questions are whether another reader shows the text, the colours and the fonts the file was written with.

use std::path::PathBuf;
use std::process::Command;

use diacraft_doc::RowKey;
use diacraft_pdf::{PdfOptions, export_page};
use diacraft_render::Renderer;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

fn have(tool: &str) -> bool {
    Command::new(tool).arg("-v").output().is_ok()
}

/// A scratch folder of this test's own, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("diacraft-m29-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A scratch folder that cannot be removed is left for the system's own clean-up of its temporary folder.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn label(id: u32, y: f64, text: &str) -> ShapeBuilder {
    rect(id, 0.5, y, 3.0, 0.6)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.3")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
}

fn stops(mut sb: ShapeBuilder, list: &[(&str, f64)]) -> ShapeBuilder {
    for (i, (color, pos)) in list.iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", &pos.to_string());
    }
    sb
}

#[test]
fn another_reader_finds_the_text_and_the_embedded_fonts() {
    if !(have("pdftotext") && have("pdffonts")) {
        eprintln!("SKIPPED: pdftotext and pdffonts (poppler) are not on this machine");
        return;
    }
    let dir = Scratch::new("text");
    let doc = one_page(vec![label(1, 2.5, "Hello World"), label(2, 1.5, "Second line 42")]);
    let out = export_page(&mut Renderer::new(), &doc, 1, &PdfOptions::default()).expect("exports");
    let file = dir.0.join("page.pdf");
    std::fs::write(&file, &out.bytes).expect("written");
    let text = Command::new("pdftotext").arg("-layout").arg(&file).arg("-").output().expect("pdftotext runs");
    assert!(text.status.success(), "{}", String::from_utf8_lossy(&text.stderr));
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    assert!(text.contains("Hello World") && text.contains("Second line 42"), "{text:?}");
    assert!(text.find("Hello World") < text.find("Second line 42"), "top to bottom: {text:?}");
    let fonts = Command::new("pdffonts").arg(&file).output().expect("pdffonts runs");
    let fonts = String::from_utf8_lossy(&fonts.stdout).into_owned();
    // One embedded subset: its name starts with the six-letter subset tag, and the `emb` column says yes.
    let rows: Vec<&str> = fonts.lines().skip(2).collect();
    assert!(!rows.is_empty() && rows.iter().all(|r| r.contains(" yes ")), "{fonts}");
}

#[test]
fn another_reader_draws_the_colours_the_gradients_and_the_page_where_the_file_says() {
    if !have("pdftoppm") {
        eprintln!("SKIPPED: pdftoppm (poppler) is not on this machine");
        return;
    }
    let dir = Scratch::new("raster");
    let gradient = stops(
        rect(2, 1.0, 2.5, 2.0, 1.0)
            .cell("LinePattern", "0")
            .cell("FillGradientEnabled", "1")
            .cell("FillGradientDir", "0")
            .cell("FillGradientAngle", "0"),
        &[("#FF0000", 0.0), ("#0000FF", 1.0)],
    );
    let doc = one_page(vec![rect(1, 1.0, 1.0, 2.0, 1.0).cell("FillForegnd", "#00AA00").cell("LinePattern", "0"), gradient]);
    let out = export_page(&mut Renderer::new(), &doc, 1, &PdfOptions::default()).expect("exports");
    let file = dir.0.join("page.pdf");
    std::fs::write(&file, &out.bytes).expect("written");
    let prefix = dir.0.join("page");
    let run = Command::new("pdftoppm").args(["-r", "72", "-png", "-singlefile"]).arg(&file).arg(&prefix).output().expect("pdftoppm runs");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let bytes = std::fs::read(dir.0.join("page.png")).expect("a picture");
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("a PNG");
    let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut pixels).expect("pixels");
    assert_eq!((info.width, info.height), (288, 288));
    let channels = info.color_type.samples();
    let at = |x_in: f64, y_in: f64| {
        let (px, py) = ((x_in * 72.0) as usize, ((4.0 - y_in) * 72.0) as usize);
        let i = (py * 288 + px) * channels;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 12);
    assert!(near(at(2.0, 1.5), [0, 170, 0]), "the green box: {:?}", at(2.0, 1.5));
    assert!(near(at(0.5, 0.5), [255, 255, 255]), "the paper: {:?}", at(0.5, 0.5));
    // The gradient runs from red at its left edge to blue at its right, through the mix at the middle.
    let (left, middle, right) = (at(1.05, 3.0), at(2.0, 3.0), at(2.95, 3.0));
    assert!(left[0] > 220 && left[2] < 40, "red at the left: {left:?}");
    assert!(right[2] > 220 && right[0] < 40, "blue at the right: {right:?}");
    assert!(middle[0] > 90 && middle[0] < 170 && middle[2] > 90 && middle[2] < 170, "a mix in the middle: {middle:?}");
}

/// A print in black and white, drawn by another reader, has no colour in it: not in its fills, lines, gradient, words or picture.
#[test]
fn another_reader_draws_a_print_in_black_and_white_with_no_colour_in_it() {
    use diacraft_render::print::{PrintColor, plan_job};
    if !have("pdftoppm") {
        eprintln!("SKIPPED: pdftoppm (poppler) is not on this machine");
        return;
    }
    let dir = Scratch::new("grey");
    let gradient = stops(
        rect(2, 2.0, 2.5, 1.5, 1.0)
            .cell("LinePattern", "0")
            .cell("FillGradientEnabled", "1")
            .cell("FillGradientDir", "0")
            .cell("FillGradientAngle", "0"),
        &[("#FF0000", 0.0), ("#0000FF", 1.0)],
    );
    let words = label(3, 1.5, "Coloured words").row_cell("Character", None, RowKey::Ix(0), "Color", "#00AA00");
    let pixels: Vec<u8> = (0..64u32).flat_map(|i| if i % 2 == 0 { [255, 0, 0, 255] } else { [0, 200, 255, 255] }).collect();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("a header").write_image_data(&pixels).expect("pixels");
    }
    let picture = geometry_of(shape_at(4, 0.3, 0.2, 1.0, 1.0), rect_rows(1.0, 1.0)).cell("FillPattern", "0").cell("LinePattern", "0").foreign(
        diacraft_doc::ForeignData {
            attrs: diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
            picture: Some(std::sync::Arc::new(diacraft_doc::Media {
                part: std::sync::Arc::from("/visio/media/image1"),
                content_type: std::sync::Arc::from("image/png"),
                bytes: std::sync::Arc::from(png),
            })),
            ..diacraft_doc::ForeignData::default()
        },
    );
    let boxed = rect(1, 0.5, 2.5, 1.2, 1.0).cell("FillForegnd", "#FF0000").cell("LineColor", "#0000FF").cell("LineWeight", "0.05");
    let doc = one_page(vec![boxed, gradient, words, picture]);
    let plan = plan_job(&doc, &[1]).expect("plans");
    let sheets: Vec<_> = plan.sheets().map(|(_, s)| s.clone()).collect();
    let draw = |color: PrintColor, name: &str| {
        let options = diacraft_pdf::PrintOptions { color, ..diacraft_pdf::PrintOptions::default() };
        let printed = diacraft_pdf::export_sheets(&mut Renderer::new(), &doc, &sheets, &options).expect("prints");
        let file = dir.0.join(format!("{name}.pdf"));
        std::fs::write(&file, &printed.bytes).expect("written");
        let prefix = dir.0.join(name);
        let run = Command::new("pdftoppm").args(["-r", "36", "-png", "-singlefile"]).arg(&file).arg(&prefix).output().expect("pdftoppm runs");
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let bytes = std::fs::read(dir.0.join(format!("{name}.png"))).expect("a picture");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("a PNG");
        let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
        let info = reader.next_frame(&mut pixels).expect("pixels");
        let channels = info.color_type.samples();
        pixels.chunks_exact(channels).map(|p| [p[0], p[1], p[2]]).collect::<Vec<[u8; 3]>>()
    };
    let spread = |p: &[u8; 3]| p.iter().max().unwrap_or(&0) - p.iter().min().unwrap_or(&0);
    let colour = draw(PrintColor::Color, "colour");
    assert!(colour.iter().filter(|p| spread(p) > 2).count() > 200, "the colour print has colour");
    let grey = draw(PrintColor::Grayscale, "grey");
    let worst = grey.iter().map(spread).max().unwrap_or(0);
    assert!(worst <= 2, "the print in black and white has no colour: a pixel's channels are {worst} apart");
    let ink = grey.iter().filter(|p| **p != [255, 255, 255]).count();
    assert!(ink > 200, "and the shapes are on it, in greys: {ink} pixels");
}
