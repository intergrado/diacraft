//! **Another program's gradient, read from an `.odg` or a `.fodg`, is drawn by DiaCraft as LibreOffice draws it**, and its cells come
//! back from a `.diacraft` file and a VSDX file as they were read.
//!
//! The bug: the reader's stand-in rows named their cells `Color` and `Position`, where the renderer, the engine and a VSDX file read `GradientStopColor` and `GradientStopPosition`
//! And set no angle, so every such gradient was drawn as the root's grey ramp from the top (measured a red-to-blue `.fodg` exported grey).
//!
//! The matrix is one shape for each layout (linear, axial, radial, ellipsoid, square, rectangular), with and without LibreOffice's `loext:gradient-stop` children, a border, intensities,
//! centres at a corner and an edge, and no centre. What LibreOffice 26.8.2.2 draws at 25 places inside each shape was measured once on this very drawing (a black box: its own profile, a copy
//! of the file, exported to PNG; `lo_draws_the_matrix`, ignored, prints the table) and is the oracle of [`another_program_s_gradients_are_drawn_as_libreoffice_draws_them`]. The tolerances
//! are the (the report's table): a mean of 5 of 255 for a linear or axial gradient, and up to 28 for a layout with a centre, where DiaCraft's ellipse or rectangle bands are the box's
//! scaled and LibreOffice's are a circle or keep the gap between their sides.

use diacraft_doc::{Document, SectionKey, Shape};
use diacraft_engine::Session;
use diacraft_render::{Renderer, View};
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing, open_bytes_in};
use crate::run;

const LOEXT: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";

/// A `loext:gradient-stop`.
fn stop(at: &str, colour: &str) -> String {
    format!(r#"<loext:gradient-stop svg:offset="{at}" loext:color-type="rgb" loext:color-value="{colour}"/>"#)
}

/// Red, green at a quarter, blue.
fn three() -> String {
    format!("{}{}{}", stop("0", "#ff0000"), stop("0.25", "#00ff00"), stop("1", "#0000ff"))
}

/// A definition `name` with `attrs`, red to blue unless `attrs` says otherwise, and `stops` (the extension declared on the element).
fn gradient(name: &str, attrs: &str, stops: &str) -> String {
    let ends = if attrs.contains("start-color") { "" } else { r##"draw:start-color="#ff0000" draw:end-color="#0000ff""## };
    if stops.is_empty() {
        format!(r#"<draw:gradient draw:name="{name}" {attrs} {ends}/>"#)
    } else {
        format!(r#"<draw:gradient xmlns:loext="{LOEXT}" draw:name="{name}" {attrs} {ends}>{stops}</draw:gradient>"#)
    }
}

/// Each case of the matrix: its definition, and the largest mean difference (of 255) from LibreOffice's picture it may have.
fn cases() -> Vec<(String, f64)> {
    let (linear, centred) = (5.0, 28.0);
    vec![
        (gradient("G0", r#"draw:style="linear" draw:angle="0""#, ""), linear),
        (gradient("G1", r#"draw:style="linear" draw:angle="30" draw:border="20%""#, ""), linear),
        (gradient("G2", r#"draw:style="linear" draw:angle="-45""#, &three()), linear),
        (
            gradient(
                "G3",
                r##"draw:style="linear" draw:angle="90deg" draw:start-color="#000000" draw:end-color="#000000""##,
                &format!("{}{}{}{}", stop("0.2", "#c00000"), stop("0.5", "#c00000"), stop("0.5", "#2030e0"), stop("0.8", "#2030e0")),
            ),
            linear,
        ),
        (gradient("G4", r#"draw:style="axial" draw:angle="0""#, ""), linear),
        (gradient("G5", r#"draw:style="axial" draw:angle="90" draw:border="30%""#, &three()), linear),
        (gradient("G6", r#"draw:style="radial" draw:cx="50%" draw:cy="50%""#, ""), centred),
        (gradient("G7", r#"draw:style="radial" draw:cx="50%" draw:cy="50%" draw:border="30%""#, ""), centred),
        (gradient("G8", r#"draw:style="radial" draw:cx="0%" draw:cy="0%""#, &three()), centred),
        (gradient("G9", r#"draw:style="radial" draw:cx="50%" draw:cy="100%""#, ""), centred),
        (gradient("G10", r#"draw:style="ellipsoid" draw:cx="50%" draw:cy="50%""#, ""), centred),
        (gradient("G11", r#"draw:style="ellipsoid" draw:cx="100%" draw:cy="100%""#, &three()), centred),
        (gradient("G12", r#"draw:style="square" draw:cx="50%" draw:cy="50%""#, ""), centred),
        (gradient("G13", r#"draw:style="square" draw:cx="100%" draw:cy="0%""#, ""), centred),
        (gradient("G14", r#"draw:style="rectangular" draw:cx="50%" draw:cy="50%""#, ""), centred),
        (gradient("G15", r#"draw:style="rectangular" draw:cx="0%" draw:cy="100%""#, &three()), centred),
        (gradient("G16", r#"draw:style="linear" draw:angle="45" draw:start-intensity="60%" draw:end-intensity="80%""#, ""), linear),
        (gradient("G17", r#"draw:style="radial""#, ""), centred),
    ]
}

/// The frame of the shape at `at`, in centimetres from the page's top left: four columns of 4 by 2.5 cm.
fn frame(at: usize) -> (f64, f64, f64, f64) {
    (1.5 + 4.6 * (at % 4) as f64, 1.5 + 3.4 * (at / 4) as f64, 4.0, 2.5)
}

/// The drawing of the matrix: one custom shape for each case, on an automatic style that fills with its gradient and draws no line.
fn matrix() -> Drawing {
    let mut d = Drawing::new();
    let mut shapes = String::new();
    for (at, (def, _)) in cases().iter().enumerate() {
        d = d.common(def);
        d.auto.push_str(&format!(
            r#"<style:style style:name="f{at}" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="gradient" draw:fill-gradient-name="G{at}"/></style:style>"#
        ));
        shapes.push_str(&crate::odg_build::rect_on(&format!("f{at}"), &format!("s{at}"), frame(at), ""));
    }
    d.page(&shapes)
}

/// The places sampled inside the shape at `at`: five by five, from a tenth to nine tenths of the way across and down, in centimetres from the page's top left.
fn places(at: usize) -> Vec<(f64, f64)> {
    let (x, y, w, h) = frame(at);
    let mut out = Vec::new();
    for j in 0..5 {
        for i in 0..5 {
            let (u, v) = (0.1 + 0.2 * f64::from(i), 0.1 + 0.2 * f64::from(j));
            out.push((x + w * u, y + h * v));
        }
    }
    out
}

/// A picture: its width, height and the colour at a pixel.
struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Picture {
    /// The colour at (`x`, `y`) centimetres from the top left of an A4 page.
    fn at(&self, (x, y): (f64, f64)) -> [u8; 3] {
        let px = ((x / 21.0 * self.width as f64) as usize).min(self.width - 1);
        let py = ((y / 29.7 * self.height as f64) as usize).min(self.height - 1);
        self.rgb[py * self.width + px]
    }
}

/// The first page of `doc` as DiaCraft draws it, at 96 pixels to the inch on white paper.
fn drawn(doc: &Document) -> Picture {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 96.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    // A rectangular layout is drawn as its nearest form, and says so (`render`'s gradient notes); nothing else is left out.
    assert!(frame.skipped.iter().all(|s| s.kind == diacraft_render::SkipKind::GradientApproximate), "everything was drawn: {:?}", frame.skipped);
    // The paper is opaque, so the premultiplied colour is the colour.
    let rgb = frame.rgba_premul.as_chunks::<4>().0.iter().map(|p| [p[0], p[1], p[2]]).collect();
    Picture { width: frame.width as usize, height: frame.height as usize, rgb }
}

/// A PNG's pixels.
fn decoded(png_bytes: &[u8]) -> Picture {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().expect("a PNG");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut buf).expect("its pixels");
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("a colour type of the check's pictures: {other:?}"),
    };
    let (width, height) = (info.width as usize, info.height as usize);
    let rgb = (0..width * height).map(|i| [buf[i * channels], buf[i * channels + 1], buf[i * channels + 2]]).collect();
    Picture { width, height, rgb }
}

/// The mean of the channels' differences between two colours.
fn difference(a: [u8; 3], b: [u8; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).abs()).sum::<f64>() / 3.0
}

fn hex_of(text: &str) -> [u8; 3] {
    let n = u32::from_str_radix(text, 16).expect("a colour of the table");
    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
}

/// What LibreOffice 26.8.2.2 drew at [`places`] of each shape of [`matrix`] (`lo_draws_the_matrix`, by hand, on 2026-11-22): a black box, our own drawing, exported to PNG at 96 pixels
/// to the inch.
const LIBREOFFICE: [[&str; 25]; 18] = [
    [
        "e1001e", "e1001e", "e1001e", "e1001e", "e1001e", "b0004f", "b0004f", "b0004f", "b0004f", "b0004f", "82007d", "82007d", "82007d", "82007d",
        "82007d", "5100ae", "5100ae", "5100ae", "5100ae", "5100ae", "2000df", "2000df", "2000df", "2000df", "2000df",
    ],
    [
        "ff0000", "fe0001", "e0001f", "c2003d", "a5005a", "fc0003", "dd0022", "bf0040", "a2005d", "84007b", "dd0022", "be0041", "a0005f", "83007c",
        "65009a", "bc0043", "9d0062", "80007f", "62009d", "4400bb", "9b0064", "7d0082", "5f00a0", "4100be", "2300dc",
    ],
    [
        "008a75", "00b54a", "00e01f", "1ae500", "986700", "006f90", "009a65", "00c53a", "00ef10", "48b700", "0056a9", "00817e", "00ac53", "00d52a",
        "00ff00", "003cc3", "00659a", "00916e", "00bb44", "00e51a", "0022dd", "004bb4", "007689", "00a15e", "00ca35",
    ],
    [
        "c00000", "c00000", "2030e0", "2030e0", "2030e0", "c00000", "c00000", "2030e0", "2030e0", "2030e0", "c00000", "c00000", "2030e0", "2030e0",
        "2030e0", "c00000", "c00000", "2030e0", "2030e0", "2030e0", "c00000", "c00000", "2030e0", "2030e0", "2030e0",
    ],
    [
        "3700c7", "3700c7", "3700c7", "3700c7", "3700c7", "9b0063", "9b0063", "9b0063", "9b0063", "9b0063", "ff0000", "ff0000", "ff0000", "ff0000",
        "ff0000", "9b0063", "9b0063", "9b0063", "9b0063", "9b0063", "2c00d2", "2c00d2", "2c00d2", "2c00d2", "2c00d2",
    ],
    [
        "0000ff", "009768", "fb0400", "00926d", "0000ff", "0000ff", "009768", "fb0400", "00926d", "0000ff", "0000ff", "009768", "fb0400", "00926d",
        "0000ff", "0000ff", "009768", "fb0400", "00926d", "0000ff", "0000ff", "009768", "fb0400", "00926d", "0000ff",
    ],
    [
        "c90036", "890076", "6a0095", "880077", "c70038", "b3004c", "660099", "3500ca", "64009b", "b0004f", "ab0054", "5700a8", "0000ff", "5400ab",
        "a80057", "b3004c", "660099", "3500ca", "64009b", "b0004f", "c90036", "890076", "6a0095", "880077", "c70038",
    ],
    [
        "ff0000", "aa0055", "6b0094", "a60059", "ff0000", "fe0001", "62009d", "0000ff", "5d00a2", "f90006", "ee0011", "4300bc", "0000ff", "3d00c2",
        "e80017", "fe0001", "62009d", "0000ff", "5d00a2", "f90006", "ff0000", "aa0055", "6b0094", "a60059", "ff0000",
    ],
    [
        "0043bc", "00b44b", "6c9300", "ff0000", "ff0000", "007b84", "00cf30", "a35c00", "ff0000", "ff0000", "00bf40", "00fe01", "ff0000", "ff0000",
        "ff0000", "0ff000", "a35c00", "ff0000", "ff0000", "ff0000", "dc2300", "ff0000", "ff0000", "ff0000", "ff0000",
    ],
    [
        "ff0000", "ff0000", "f2000d", "ff0000", "ff0000", "ff0000", "cf0030", "be0041", "cf0030", "fd0002", "da0025", "a0005f", "890076", "a0005f",
        "d80027", "bd0042", "760089", "5400ab", "760089", "bb0044", "ad0052", "5a00a5", "2100de", "5a00a5", "ab0054",
    ],
    [
        "c1003e", "9a0065", "90006f", "9a0065", "c1003e", "7e0081", "5100ae", "4600b9", "5100ae", "7d0082", "4d00b2", "0200fd", "0100fe", "0200fd",
        "4d00b2", "81007e", "5400ab", "4900b6", "5400ab", "82007d", "c1003e", "9b0064", "90006f", "9b0064", "c2003d",
    ],
    [
        "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "f70800", "ff0000", "ff0000", "f20d00", "2fd000",
        "00f20d", "ff0000", "ff0000", "00ff00", "00b04f", "00916e", "ff0000", "ff0000", "00bf40", "0051ae", "0033cc",
    ],
    [
        "ce0031", "81007e", "81007e", "81007e", "cc0033", "ce0031", "64009b", "3f00c0", "65009a", "cc0033", "ce0031", "64009b", "0000ff", "65009a",
        "cc0033", "ce0031", "64009b", "3f00c0", "65009a", "cc0033", "ce0031", "80007f", "80007f", "80007f", "cc0033",
    ],
    [
        "ff0000", "ff0000", "fe0001", "980066", "3300cc", "ff0000", "ff0000", "fe0001", "980066", "5f00a0", "ff0000", "ff0000", "fe0001", "a0005f",
        "a0005f", "ff0000", "ff0000", "fe0001", "e0001f", "e0001f", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000",
    ],
    [
        "ce0031", "ce0031", "ce0031", "ce0031", "ce0031", "ae0051", "65009a", "65009a", "65009a", "ae0051", "ae0051", "0900f6", "0100fe", "0900f6",
        "ae0051", "ae0051", "65009a", "65009a", "65009a", "ae0051", "cd0032", "cd0032", "cd0032", "cd0032", "cd0032",
    ],
    [
        "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "ff0000", "fa0500", "fa0500", "ff0000", "ff0000",
        "ff0000", "00cb34", "00cb34", "ff0000", "ff0000", "ff0000", "0040bf", "007986", "ff0000", "ff0000", "ff0000",
    ],
    [
        "890015", "76002f", "640047", "51005f", "3f0078", "7d0025", "6a003e", "580056", "46006f", "330087", "720034", "5f004e", "4d0066", "3a007e",
        "280097", "660044", "53005d", "410075", "2f008e", "1c00a6", "5b0053", "48006d", "350085", "23009d", "1100b6",
    ],
    [
        "3600c9", "870078", "d90026", "ff0000", "ff0000", "5f00a0", "9b0064", "e60019", "ff0000", "ff0000", "90006f", "bd0042", "fe0001", "ff0000",
        "ff0000", "c3003c", "e60019", "ff0000", "ff0000", "ff0000", "f70008", "ff0000", "ff0000", "ff0000", "ff0000",
    ],
];

/// The mean and the largest difference of each shape of `picture` from LibreOffice's.
fn scores(picture: &Picture) -> Vec<(f64, f64)> {
    (0..cases().len())
        .map(|at| {
            let diffs: Vec<f64> = places(at).into_iter().zip(LIBREOFFICE[at]).map(|(p, lo)| difference(picture.at(p), hex_of(lo))).collect();
            (diffs.iter().sum::<f64>() / diffs.len() as f64, diffs.iter().copied().fold(0.0, f64::max))
        })
        .collect()
}

#[test]
fn another_program_s_gradients_are_drawn_as_libreoffice_draws_them() {
    let dir = Dir::new();
    let d = matrix();
    for (name, bytes) in [("matrix.odg", d.bytes()), ("matrix.fodg", d.flat_bytes())] {
        let s = open_bytes_in(&dir, name, &bytes);
        let picture = drawn(s.document().unwrap());
        for (at, ((mean, worst), (_, tolerance))) in scores(&picture).into_iter().zip(cases()).enumerate() {
            assert!(
                mean <= tolerance,
                "{name}: shape {at}: a mean difference of {mean:.1} from LibreOffice's picture (the largest {worst:.0}), past {tolerance}"
            );
        }
    }
}

/// The probe that found the bug, made again by hand: a page of 10 cm, a rectangle of 8 by 5 cm at (1, 2.5) filling with a linear gradient from red to blue, from the top down. DiaCraft
/// exported it as a grey ramp.
#[test]
fn the_red_to_blue_gradient_measured_grey_is_red_at_the_top_and_blue_at_the_bottom() {
    let fodg = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document {ns} office:version="1.4" office:mimetype="{DRAWING}"><office:styles><draw:gradient draw:name="G" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:angle="0" draw:border="0%"/></office:styles><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="0cm" fo:margin-bottom="0cm" fo:margin-left="0cm" fo:margin-right="0cm" fo:page-width="10.0cm" fo:page-height="10.0cm" style:print-orientation="portrait"/></style:page-layout><style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="gradient" draw:fill-gradient-name="G"/></style:style></office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles><office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default"><draw:rect draw:style-name="gr1" draw:layer="layout" svg:width="8.0cm" svg:height="5.0cm" svg:x="1.0cm" svg:y="2.5cm"/></draw:page></office:drawing></office:body></office:document>"##,
        ns = crate::odg_build::NS
    );
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a0.fodg", fodg.as_bytes());
    // Exported as the CLI exports it, at 96 pixels to the inch: the page is 378 pixels square, the shape from 38 to 340 across and 94 to 283 down.
    let png_path = dir.path("a0.png");
    run(&mut s, "file.export", json!({ "path": png_path, "dpi": 96 }));
    let picture = decoded(&std::fs::read(&png_path).unwrap());
    let at = |x: usize, y: usize| picture.rgb[y * picture.width + x];
    for x in [60, 189, 320] {
        let (top, middle, bottom) = (at(x, 100), at(x, 189), at(x, 277));
        assert!(top[0] > 230 && top[2] < 25, "red at the top, not {top:?}");
        assert!(bottom[2] > 230 && bottom[0] < 25, "blue at the bottom, not {bottom:?}");
        assert!(
            middle[1] == 0 && (i32::from(middle[0]) - 128).abs() < 12 && (i32::from(middle[2]) - 128).abs() < 12,
            "purple in the middle, not {middle:?}"
        );
    }
}

/// A shape's gradient cells, as [`gradient_cells`] gives them: each direct cell's name, value and unit, and each row (deleted, or its colour and its place).
type GradientCells = (Vec<(String, String, Option<String>)>, Vec<Option<(String, String)>>);

/// A shape's gradient cells: `FillPattern`, `FillGradientEnabled`, `FillGradientDir` and `FillGradientAngle` (a value, and its unit), and each row of `FillGradient` (deleted, or its
/// colour and its place).
fn gradient_cells(shape: &Shape) -> GradientCells {
    let shown = |c: &diacraft_doc::Cell| c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default();
    let direct = ["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillGradientAngle"]
        .iter()
        .filter_map(|name| shape.sheet.cell(name).map(|c| ((*name).to_owned(), shown(c), c.unit.as_deref().map(str::to_owned))))
        .collect();
    let rows = shape
        .sheet
        .section(&SectionKey::new("FillGradient", None))
        .map(|sec| {
            sec.rows
                .iter()
                .map(|r| {
                    (!r.del).then(|| {
                        let cell = |n: &str| r.cells.get(n).map(shown).unwrap_or_default();
                        (cell("GradientStopColor"), cell("GradientStopPosition"))
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (direct, rows)
}

fn all_cells(s: &Session) -> Vec<GradientCells> {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| gradient_cells(shape)).collect()
}

/// What `shape.formatting` says of each shape's gradient.
fn looks(s: &mut Session) -> Vec<serde_json::Value> {
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
    let out = run(s, "shape.formatting", json!({"ids": ids, "page": page}));
    out["shapes"].as_array().unwrap().iter().map(|look| look["gradient"].clone()).collect()
}

#[test]
fn the_gradients_cells_come_back_from_a_diacraft_file_and_a_vsdx_file_as_they_were_read() {
    let dir = Dir::new();
    let d = matrix();
    for (name, bytes) in [("matrix.odg", d.bytes()), ("matrix.fodg", d.flat_bytes())] {
        let mut s = open_bytes_in(&dir, name, &bytes);
        let (want, want_looks) = (all_cells(&s), looks(&mut s));
        assert_eq!(want.len(), cases().len());
        for (at, (direct, rows)) in want.iter().enumerate() {
            assert!(direct.iter().any(|(n, v, _)| n == "FillGradientEnabled" && v == "1"), "{name}: shape {at}: {direct:?}");
            assert!(rows.iter().flatten().count() >= 2, "{name}: shape {at}: the stops {rows:?}");
            assert_eq!(
                want_looks[at]["stops"].as_array().map(Vec::len),
                Some(rows.iter().flatten().count()),
                "{name}: shape {at}: the engine reads every stop"
            );
        }
        for target in ["matrix.diacraft", "matrix.vsdx"] {
            let path = dir.path(&format!("{name}-{target}"));
            run(&mut s, "file.save_as", json!({ "path": path }));
            let mut back = Session::new();
            run(&mut back, "file.open", json!({ "path": path }));
            assert_eq!(all_cells(&back), want, "{name} saved as {target}: the cells");
            assert_eq!(looks(&mut back), want_looks, "{name} saved as {target}: the gradients the engine reads");
        }
    }
}

/// **LibreOffice 26.8.2.2 draws the matrix**, by hand: `ODG_FX_F1_DIR=<an empty folder> cargo test -p diacraft-engine --test it -- --ignored lo_draws_the_matrix --nocapture`. The
/// drawing is written to the folder, LibreOffice converts a copy of it to PNG with a profile of its own in the folder (only a missing picture is a failure), and the colours at
/// [`places`] are printed as the table [`LIBREOFFICE`] holds, with each shape's difference from DiaCraft's picture of the same file.
#[test]
#[ignore = "a black-box check against LibreOffice 26.8.2.2, run by hand with ODG_FX_F1_DIR set to an empty folder"]
fn lo_draws_the_matrix() {
    let folder = std::path::PathBuf::from(std::env::var("ODG_FX_F1_DIR").expect("set ODG_FX_F1_DIR to an empty folder"));
    std::fs::create_dir_all(&folder).unwrap();
    let d = matrix();
    std::fs::write(folder.join("matrix.odg"), d.bytes()).unwrap();
    std::fs::write(folder.join("matrix.fodg"), d.flat_bytes()).unwrap();
    let work = folder.join("lo");
    std::fs::create_dir_all(&work).unwrap();
    let copy = work.join("matrix_copy.odg");
    std::fs::write(&copy, d.bytes()).unwrap();
    let status = std::process::Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--norestore", "--convert-to", "png", "--outdir"])
        .arg(&work)
        .arg(&copy)
        .output()
        .expect("soffice is installed");
    let png_path = work.join("matrix_copy.png");
    assert!(png_path.exists(), "LibreOffice wrote no picture ({:?}): {}", status.status, String::from_utf8_lossy(&status.stderr));
    let lo = decoded(&std::fs::read(&png_path).unwrap());
    println!("LibreOffice's picture: {} by {}", lo.width, lo.height);
    println!("const LIBREOFFICE: [[&str; 25]; {}] = [", cases().len());
    for at in 0..cases().len() {
        let row: Vec<String> = places(at).into_iter().map(|p| lo.at(p)).map(|c| format!("\"{:02x}{:02x}{:02x}\"", c[0], c[1], c[2])).collect();
        println!("    [{}],", row.join(", "));
    }
    println!("];");
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "matrix.odg", &d.bytes());
    // DiaCraft's own picture beside LibreOffice's, to look at.
    run(&mut s, "file.export", json!({ "path": folder.join("diacraft.png").to_str().unwrap(), "dpi": 96 }));
    let ours = drawn(s.document().unwrap());
    for at in 0..cases().len() {
        let diffs: Vec<f64> = places(at).into_iter().map(|p| difference(ours.at(p), lo.at(p))).collect();
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        println!("shape {at:2}: mean {mean:5.1}, largest {:5.1}", diffs.iter().copied().fold(0.0, f64::max));
    }
}
