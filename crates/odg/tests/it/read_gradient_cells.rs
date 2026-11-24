//! Another program's gradient, read from an `.odg` or a `.fodg`, stands in as **the cells the renderer and a VSDX file read** (`GradientStopColor` and `GradientStopPosition` in each row of `FillGradient`, where the reader wrote `Color` and `Position`, which nothing
//! reads, so every such gradient was drawn as the root's grey ramp),
//! with `FillGradientAngle` (the measure: `draw:angle` turns counterclockwise from a start at the top, so the cell
//! is 90 degrees less it), the layout from the centre, the border, the intensities and LibreOffice's `loext:gradient-stop` children. What LibreOffice 26.8.2.2 draws for each of these was
//! measured on our own files (the task's report); the engine's `odg_gradients.rs` draws them. Every drawing here is written by hand.

use diacraft_doc::{Document, Owner, Resolver, SectionKey, Shape, ShapeRef, SheetRef};

use crate::drawing::{Drawing, cell, num, row_cell, shape, text};
use crate::flat::Flat;

const LOEXT: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";

/// A `loext:gradient-stop` at `at` of `colour`.
fn stop(at: &str, colour: &str) -> String {
    format!(r#"<loext:gradient-stop svg:offset="{at}" loext:color-type="rgb" loext:color-value="{colour}"/>"#)
}

/// Red at 0, green at a quarter and blue at the end.
fn three() -> String {
    format!("{}{}{}", stop("0", "#ff0000"), stop("0.25", "#00ff00"), stop("1", "#0000ff"))
}

/// A definition `name` from red to blue with `attrs`, and the children `stops` (the extension declared on the element, as DiaCraft writes it).
fn gradient(name: &str, attrs: &str, stops: &str) -> String {
    let ends = r##"draw:start-color="#ff0000" draw:end-color="#0000ff""##;
    if stops.is_empty() {
        format!(r#"<draw:gradient draw:name="{name}" {attrs} {ends}/>"#)
    } else {
        format!(r#"<draw:gradient xmlns:loext="{LOEXT}" draw:name="{name}" {attrs} {ends}>{stops}</draw:gradient>"#)
    }
}

/// A drawing of one rectangle for each definition of `defs`, in order, each on an automatic style that fills with it and draws no line.
fn drawing(defs: &[String]) -> Drawing {
    let mut d = Drawing::a4();
    let mut shapes = String::new();
    for (i, def) in defs.iter().enumerate() {
        let name = def.split("draw:name=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or_default().to_owned();
        d = d.common(def).auto(&format!(
            r#"<style:style style:name="g{i}" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="gradient" draw:fill-gradient-name="{name}"/></style:style>"#
        ));
        shapes
            .push_str(&format!(r#"<draw:rect draw:style-name="g{i}" svg:x="2cm" svg:y="{}cm" svg:width="8cm" svg:height="5cm"/>"#, 1 + 6 * (i % 4)));
    }
    d.page(&shapes)
}

/// The rows of a shape's own `FillGradient`, in `IX` order: the colour and the place of each, and `None` for a deleted row.
fn rows(s: &Shape) -> Vec<Option<(String, f64)>> {
    let Some(section) = s.sheet.section(&SectionKey::new("FillGradient", None)) else { return Vec::new() };
    section
        .rows
        .iter()
        .map(|row| {
            (!row.del).then(|| {
                let at = |name: &str| row.cells.get(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
                (at("GradientStopColor").expect("a colour"), at("GradientStopPosition").and_then(|p| p.parse().ok()).expect("a place"))
            })
        })
        .collect()
}

/// What a shape's gradient stands in as: `FillGradientDir`, the angle in degrees (none when the shape has no cell) and the live rows.
type Look = (f64, Option<f64>, Vec<(String, f64)>);

/// The layout, the angle in degrees (none when the shape has no cell) and the live rows of the shape at `at`.
fn look(doc: &Document, at: usize) -> Look {
    let s = shape(doc, 0, at);
    assert_eq!(text(cell(s, "FillGradientEnabled")), "1");
    assert_eq!(text(cell(s, "FillPattern")), "1");
    // The cells the renderer reads, and none it does not.
    let names: Vec<String> = s
        .sheet
        .section(&SectionKey::new("FillGradient", None))
        .map(|sec| sec.rows.iter().flat_map(|r| r.cells.keys().map(|k| k.as_str().to_owned()).collect::<Vec<_>>()).collect())
        .unwrap_or_default();
    assert!(names.iter().all(|n| n == "GradientStopColor" || n == "GradientStopPosition"), "shape {at}: {names:?}");
    let angle = cell(s, "FillGradientAngle").map(|c| num(Some(c)).to_degrees());
    (num(cell(s, "FillGradientDir")), angle, rows(s).into_iter().flatten().collect())
}

fn row(c: &str, at: f64) -> (String, f64) {
    (c.to_owned(), at)
}

#[track_caller]
fn same_rows(got: &[(String, f64)], want: &[(String, f64)], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?}");
    for ((gc, ga), (wc, wa)) in got.iter().zip(want) {
        assert!(gc == wc && (ga - wa).abs() < 1e-9, "{what}: {got:?}, not {want:?}");
    }
}

/// The matrix both forms of the file are read from: each layout, with and without the stops.
fn matrix() -> Vec<String> {
    vec![
        gradient("Lin", r#"draw:style="linear" draw:angle="0""#, ""),
        gradient("Lin3", r#"draw:style="linear" draw:angle="30" draw:border="20%""#, &three()),
        gradient("Ax", r#"draw:style="axial" draw:angle="90deg""#, ""),
        gradient("Ax3", r#"draw:style="axial" draw:angle="0""#, &three()),
        gradient("Rad", r#"draw:style="radial" draw:cx="50%" draw:cy="50%""#, ""),
        gradient("Rad3", r#"draw:style="radial" draw:cx="0%" draw:cy="0%""#, &three()),
        gradient("Ell", r#"draw:style="ellipsoid" draw:cx="50%" draw:cy="100%""#, ""),
        gradient("Ell3", r#"draw:style="ellipsoid" draw:cx="100%" draw:cy="100%""#, &three()),
        gradient("Sq", r#"draw:style="square" draw:cx="50%" draw:cy="50%""#, ""),
        gradient("Sq3", r#"draw:style="square" draw:cx="100%" draw:cy="0%""#, &three()),
        gradient("Rect", r#"draw:style="rectangular" draw:cx="50%" draw:cy="50%" draw:border="30%""#, ""),
        gradient("Rect3", r#"draw:style="rectangular" draw:cx="0%" draw:cy="100%""#, &three()),
    ]
}

/// What each shape of [`matrix`] stands in with: `FillGradientDir`, the angle and the rows.
fn expected() -> Vec<Look> {
    let (r, g, b) = ("#FF0000", "#00FF00", "#0000FF");
    // From an origin, the stops run from the end colour at 0 out to the start colour; a corner's places are halved (LibreOffice ends there at half the way).
    let centred_three = |k: f64| vec![row(b, 0.0), row(g, 0.75 / k), row(r, 1.0 / k), row(r, 1.0)];
    vec![
        (0.0, Some(90.0), vec![row(r, 0.0), row(b, 1.0)]),
        (0.0, Some(60.0), vec![row(r, 0.0), row(r, 0.2), row(g, 0.4), row(b, 1.0)]),
        (0.0, Some(0.0), vec![row(b, 0.0), row(r, 0.5), row(b, 1.0)]),
        (0.0, Some(90.0), vec![row(b, 0.0), row(g, 0.375), row(r, 0.5), row(g, 0.625), row(b, 1.0)]),
        (3.0, None, vec![row(b, 0.0), row(r, 1.0)]),
        (7.0, None, centred_three(2.0)),
        (4.0, None, vec![row(b, 0.0), row(r, 1.0 / 1.581_138_83), row(r, 1.0)]),
        (1.0, None, centred_three(2.0)),
        (10.0, None, vec![row(b, 0.0), row(r, 1.0)]),
        (11.0, None, centred_three(2.0)),
        (10.0, None, vec![row(b, 0.0), row(r, 0.7), row(r, 1.0)]),
        (9.0, None, centred_three(2.0)),
    ]
}

#[track_caller]
fn every_shape_stands_in_as_expected(doc: &Document, what: &str) {
    for (at, (dir, angle, want)) in expected().into_iter().enumerate() {
        let (got_dir, got_angle, got) = look(doc, at);
        assert_eq!(got_dir, dir, "{what}: shape {at}: the layout");
        match (got_angle, angle) {
            (Some(g), Some(w)) => assert!((g - w).abs() < 1e-9, "{what}: shape {at}: the angle {g}, not {w}"),
            (g, w) => assert_eq!(g.is_some(), w.is_some(), "{what}: shape {at}: an angle {g:?}, not {w:?}"),
        }
        same_rows(&got, &want, &format!("{what}: shape {at}"));
    }
}

#[test]
fn every_layout_of_another_program_s_gradient_stands_in_as_the_cells_the_renderer_reads_from_a_package_and_a_flat_drawing() {
    let d = drawing(&matrix());
    let (package, report) = d.read();
    assert_eq!(report.kept("gradient_stop"), 0, "every stop is read");
    every_shape_stands_in_as_expected(&package, "package");
    let (flat, _) = Flat::of(&d).read();
    every_shape_stands_in_as_expected(&flat, "flat");
}

#[test]
fn the_stops_are_read_by_libreoffice_s_namespace_wherever_it_is_declared_and_no_other() {
    // Declared on the root, and the element under a prefix of another name: the same stops.
    let mut d = drawing(&[format!(
        r##"<draw:gradient draw:name="G" draw:style="linear" draw:start-color="#000000" draw:end-color="#ffffff">{}{}</draw:gradient>"##,
        stop("0", "#ff0000").replace("loext:", "lo:"),
        stop("1", "#0000ff").replace("loext:", "lo:")
    )]);
    d.ns = format!(r#"{} xmlns:lo="{LOEXT}""#, d.ns);
    let (doc, _) = d.read();
    same_rows(&look(&doc, 0).2, &[row("#FF0000", 0.0), row("#0000FF", 1.0)], "declared on the root");
    // A `gradient-stop` of another namespace, a stop of the type `theme`, one with no colour and one with no place are no stops of the ramp (measured: LibreOffice draws none of them).
    let other = r##"<x:gradient-stop xmlns:x="urn:example:other" svg:offset="0.5" x:color-type="rgb" x:color-value="#00ff00"/>"##;
    let theme = r##"<loext:gradient-stop svg:offset="0.5" loext:color-type="theme" loext:color-value="#00ff00"/>"##;
    let bare = r#"<loext:gradient-stop svg:offset="0.5" loext:color-type="rgb"/>"#;
    let nowhere = r##"<loext:gradient-stop loext:color-type="rgb" loext:color-value="#00ff00"/>"##;
    let d = drawing(&[gradient(
        "G",
        r#"draw:style="linear""#,
        &format!("{}{other}{theme}{bare}{nowhere}{}", stop("0", "#ff0000"), stop("1", "#0000ff")),
    )]);
    let (doc, report) = d.read();
    same_rows(&look(&doc, 0).2, &[row("#FF0000", 0.0), row("#0000FF", 1.0)], "the stops that are none");
    assert_eq!(report.kept("gradient_stop"), 3, "the three stops of LibreOffice's that are no stops are kept");
    // The stops alone draw the gradient, in the order of their places, and a place written with `%` is the number (measured).
    let d = drawing(&[format!(
        r#"<draw:gradient xmlns:loext="{LOEXT}" draw:name="G" draw:style="linear">{}{}{}</draw:gradient>"#,
        stop("1", "#0000ff"),
        stop("0.5", "#00ff00"),
        stop("0", "#ff0000")
    )]);
    same_rows(&look(&d.read().0, 0).2, &[row("#FF0000", 0.0), row("#00FF00", 0.5), row("#0000FF", 1.0)], "sorted");
    let d = drawing(&[gradient("G", r#"draw:style="linear""#, &format!("{}{}", stop("0%", "#ff0000"), stop("50%", "#0000ff")))]);
    same_rows(&look(&d.read().0, 0).2, &[row("#FF0000", 0.0), row("#FA0005", 1.0)], "a place of 50 reaches a fiftieth of the way at the end");
}

#[test]
fn a_gradient_with_no_centre_no_layout_and_no_angle_is_drawn_from_its_defaults() {
    // Measured: no `draw:cx` or `draw:cy` is the top-left corner, no `draw:style` is linear, and no `draw:angle` is 0 (from the top down). A word the standard has no layout for is linear and kept.
    let d = drawing(&[
        gradient("R", r#"draw:style="radial""#, ""),
        gradient("N", "", ""),
        gradient("W", r#"draw:style="conical""#, ""),
        gradient("I", r#"draw:start-intensity="50%" draw:end-intensity="50%""#, ""),
    ]);
    let (doc, report) = d.read();
    assert_eq!(look(&doc, 0).0, 7.0);
    for at in [1, 2] {
        let (dir, angle, got) = look(&doc, at);
        assert_eq!((dir, angle.map(f64::round)), (0.0, Some(90.0)), "shape {at}");
        same_rows(&got, &[row("#FF0000", 0.0), row("#0000FF", 1.0)], &format!("shape {at}"));
    }
    assert!(report.kept("property_value") >= 1, "the layout that is no layout is kept");
    same_rows(&look(&doc, 3).2, &[row("#800000", 0.0), row("#000080", 1.0)], "the intensities darken the colours");
}

#[test]
fn a_gradient_of_fewer_rows_than_the_style_under_it_deletes_the_rows_it_would_lend() {
    // The common style fills with five rows; the shape's own style with two. The rows past the shape's two are deleted, so its gradient is its own two, and the style's are its five.
    let five =
        format!("{}{}{}{}{}", stop("0", "#ff0000"), stop("0.25", "#00ff00"), stop("0.5", "#0000ff"), stop("0.75", "#ffff00"), stop("1", "#00ffff"));
    let d = Drawing::a4()
        .common(&gradient("Five", r#"draw:style="linear""#, &five))
        .common(&gradient("Two", r#"draw:style="radial" draw:cx="50%" draw:cy="50%""#, ""))
        .common(r#"<style:style style:name="P" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="Five"/></style:style>"#)
        .auto(r#"<style:style style:name="g0" style:family="graphic" style:parent-style-name="P"><style:graphic-properties draw:fill-gradient-name="Two"/></style:style>"#)
        .auto(r##"<style:style style:name="g1" style:family="graphic" style:parent-style-name="P"><style:graphic-properties draw:fill-color="#123456"/></style:style>"##)
        .page(r#"<draw:rect draw:style-name="g0" svg:x="2cm" svg:y="2cm" svg:width="8cm" svg:height="5cm"/><draw:rect draw:style-name="g1" svg:x="2cm" svg:y="9cm" svg:width="8cm" svg:height="5cm"/>"#);
    let (doc, _) = d.read();
    let merged = |at: u32| {
        let res = Resolver::new(&doc);
        let page = doc.pages.get(0).map(|p| p.id).expect("a page");
        let rows =
            res.effective_rows(&SheetRef::Shape(ShapeRef::top(Owner::Page(page), at)), &SectionKey::new("FillGradient", None)).expect("the shape");
        rows.iter()
            .map(|r| {
                let colour = r
                    .cells
                    .iter()
                    .find(|(n, _)| n.as_str() == "GradientStopColor")
                    .and_then(|(_, v)| v.value.cell)
                    .and_then(|c| c.value.as_ref())
                    .map(|v| v.text().to_owned());
                colour.expect("a colour")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(merged(0), ["#0000FF", "#FF0000"], "the shape's own two, and none of the style's");
    assert_eq!(merged(1), ["#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#00FFFF"], "a shape that fills with the style's has the style's five");
    let own = rows(shape(&doc, 0, 0));
    assert_eq!(own.len(), 5, "two rows and three deleted: {own:?}");
    assert!(own[2..].iter().all(Option::is_none));
    assert!(row_cell(shape(&doc, 0, 0), "FillGradient", None, 1, "Color").is_none(), "no cell of the old names");
}

/// **Every fill, line and shadow cell the reader writes is one the catalogue lists** (the names a VSDX file and the renderer read), on a shape and on the style it names, so
/// that no cell is a name nothing reads, as the gradient's `Color` and `Position` were.
#[test]
fn every_fill_line_and_shadow_cell_the_reader_writes_is_a_cell_of_the_catalogue() {
    let props = r##"draw:fill="gradient" draw:fill-gradient-name="G" draw:fill-color="#336699" draw:opacity="60%" draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="0.05cm" svg:stroke-color="#112233" svg:stroke-opacity="80%" svg:stroke-linecap="round" draw:marker-start="DiaCraft_20_end_20_4" draw:marker-start-width="0.3cm" draw:marker-end="Other" draw:marker-end-width="0.3cm" draw:shadow="visible" draw:shadow-color="#445566" draw:shadow-opacity="50%" draw:shadow-offset-x="0.1cm" draw:shadow-offset-y="0.1cm""##;
    let hatch = r##"draw:fill="hatch" draw:fill-hatch-name="H" draw:fill-hatch-solid="true" draw:fill-color="#abcdef""##;
    let d = Drawing::a4()
        .common(&gradient("G", r#"draw:style="axial" draw:angle="15" draw:border="10%""#, &three()))
        .common(r##"<draw:hatch draw:name="H" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##)
        .common(r#"<draw:stroke-dash draw:name="D" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/>"#)
        .common(r#"<draw:marker draw:name="DiaCraft_20_end_20_4" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/><draw:marker draw:name="Other" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#)
        .common(&format!(r#"<style:style style:name="P" style:family="graphic"><style:graphic-properties {props}/></style:style>"#))
        .auto(&format!(r#"<style:style style:name="g0" style:family="graphic"><style:graphic-properties {props}/></style:style>"#))
        .auto(&format!(r#"<style:style style:name="g1" style:family="graphic" style:parent-style-name="P"><style:graphic-properties {hatch}/></style:style>"#))
        .page(r#"<draw:rect draw:style-name="g0" svg:x="2cm" svg:y="2cm" svg:width="8cm" svg:height="5cm"/><draw:rect draw:style-name="g1" svg:x="2cm" svg:y="9cm" svg:width="8cm" svg:height="5cm"/>"#);
    let (doc, _) = d.read();
    let family = |name: &str| ["Fill", "Line", "Shdw", "ShapeShdw", "BeginArrow", "EndArrow"].iter().any(|p| name.starts_with(p));
    let mut seen = 0;
    let sheets = doc
        .pages
        .get(0)
        .expect("a page")
        .shapes
        .iter()
        .map(|s| s.sheet.clone())
        .chain(doc.styles.iter().map(|s| s.sheet.clone()))
        .collect::<Vec<_>>();
    for sheet in &sheets {
        for name in sheet.cells.keys().map(|k| k.as_str()).filter(|n| family(n)) {
            assert!(diacraft_doc::cell_info(None, name).listed, "{name} is no cell of the catalogue");
            seen += 1;
        }
        for section in sheet.sections.iter().filter(|s| s.name.as_str() == "FillGradient") {
            for row in section.rows.iter() {
                for name in row.cells.keys() {
                    assert!(diacraft_doc::cell_info(Some("FillGradient"), name.as_str()).listed, "{name} is no cell of a FillGradient row");
                    seen += 1;
                }
            }
        }
    }
    assert!(seen >= 40, "the cells were written: {seen}");
}

/// A definition of any form: a layout of the six or none or another word, any centre, border, angle and intensities (numbers past their ranges, words that are none), and up to twenty
/// stops of any place (outside 0 to 1, with `%`, none) and any colour (none, of another type).
fn any_definition() -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::prelude::*;
    let word = || prop_oneof![Just(String::new()), (-500i32..500).prop_map(|n| n.to_string()), Just("x".to_owned()), Just("1e400".to_owned())];
    let layout = prop_oneof![
        Just(""),
        Just(r#"draw:style="linear""#),
        Just(r#"draw:style="axial""#),
        Just(r#"draw:style="radial""#),
        Just(r#"draw:style="ellipsoid""#),
        Just(r#"draw:style="square""#),
        Just(r#"draw:style="rectangular""#),
        Just(r#"draw:style="conical""#)
    ];
    let stop = (
        prop_oneof![
            (-30i32..130).prop_map(|n| format!("{}", f64::from(n) / 100.0)),
            (-30i32..130).prop_map(|n| format!("{n}%")),
            Just("x".to_owned())
        ],
        0u32..0x100_0000,
        0u8..4,
    )
        .prop_map(|(at, rgb, form)| match form {
            0 => format!(r##"<loext:gradient-stop svg:offset="{at}" loext:color-type="rgb" loext:color-value="#{rgb:06x}"/>"##),
            1 => format!(r##"<loext:gradient-stop svg:offset="{at}" loext:color-value="#{rgb:06x}"/>"##),
            2 => format!(r##"<loext:gradient-stop svg:offset="{at}" loext:color-type="theme" loext:color-value="#{rgb:06x}"/>"##),
            _ => format!(r#"<loext:gradient-stop svg:offset="{at}"/>"#),
        });
    (layout, word(), word(), word(), word(), word(), word(), proptest::collection::vec(stop, 0..20)).prop_map(
        |(layout, cx, cy, border, angle, si, ei, stops)| {
            let attr = |name: &str, v: &str, unit: &str| if v.is_empty() { String::new() } else { format!(r#" draw:{name}="{v}{unit}""#) };
            let attrs = format!(
                "{layout}{}{}{}{}{}{}",
                attr("cx", &cx, "%"),
                attr("cy", &cy, "%"),
                attr("border", &border, "%"),
                attr("angle", &angle, ""),
                attr("start-intensity", &si, "%"),
                attr("end-intensity", &ei, "%")
            );
            gradient("F", &attrs, &stops.concat())
        },
    )
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 256, ..proptest::prelude::ProptestConfig::default() })]

    /// **Any definition stands in as the cells the renderer reads** (the reader's fuzz target's kind 8 holds the same): every live row holds a colour `#RRGGBB` and a place from 0 to 1, at most
    /// ten rows are live, the places rise, the layout is one of 0 to 13, an angle is a finite turn, and a shape under a style whose gradient has more rows deletes the rest.
    #[test]
    fn any_definition_stands_in_as_rows_the_renderer_reads(def in any_definition()) {
        let d = Drawing::a4()
            .common(&def)
            .common(&gradient("Many", r#"draw:style="linear""#, &(0..10).map(|i| stop(&format!("{}", f64::from(i) / 9.0), "#808080")).collect::<String>()))
            .common(r#"<style:style style:name="P" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="Many"/></style:style>"#)
            .auto(r#"<style:style style:name="g0" style:family="graphic" style:parent-style-name="P"><style:graphic-properties draw:fill-gradient-name="F"/></style:style>"#)
            .page(r#"<draw:rect draw:style-name="g0" svg:x="2cm" svg:y="2cm" svg:width="8cm" svg:height="5cm"/>"#);
        let (doc, _) = d.read();
        let s = shape(&doc, 0, 0);
        // A layout or an angle equal to the style's is the style's, and not the shape's own.
        if let Some(dir) = cell(s, "FillGradientDir") {
            let dir = num(Some(dir));
            proptest::prop_assert!((0.0..=13.0).contains(&dir) && dir.fract() == 0.0, "{dir}");
        }
        if let Some(angle) = cell(s, "FillGradientAngle") {
            let a = num(Some(angle));
            proptest::prop_assert!(a.is_finite() && (0.0..std::f64::consts::TAU).contains(&a), "{a}");
        }
        let all = rows(s);
        let live: Vec<(String, f64)> = all.iter().flatten().cloned().collect();
        proptest::prop_assert!((2..=10).contains(&live.len()), "{all:?}");
        proptest::prop_assert!(live.iter().all(|(c, at)| c.len() == 7 && c.starts_with('#') && (0.0..=1.0).contains(at)), "{live:?}");
        proptest::prop_assert!(live.windows(2).all(|w| w[0].1 <= w[1].1), "{live:?}");
        // The live rows first, then the deleted ones up to the style's ten.
        proptest::prop_assert!(all.len() == 10 || live.len() == all.len(), "{all:?}");
        proptest::prop_assert!(all.iter().skip(live.len()).all(Option::is_none), "{all:?}");
    }
}

/// A linear gradient of DiaCraft's own, red to blue, with **no `FillGradientAngle` cell** (a shape that takes its angle from nowhere: the 90 degrees), on the shape `id` of page 0.
fn linear_without_angle(doc: &mut Document, id: u32) {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Row, Section, Value};
    let s = crate::write_core::shape_mut(doc, 0, id);
    let sheet = Arc::make_mut(&mut s.sheet);
    for (name, v) in [("FillPattern", 1.0), ("FillGradientEnabled", 1.0), ("FillGradientDir", 0.0)] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).expect("a number")));
    }
    sheet.cells.remove("FillGradientAngle");
    let mut section = Section::new("FillGradient", None);
    for (i, (hex, at)) in [("#ff0000", 0.0), ("#0000ff", 1.0)].into_iter().enumerate() {
        let mut row = Row { ix: Some(i as u32), ..Row::default() };
        row.cells.insert(CellName::new("GradientStopColor"), Cell::constant(Value::from_text(hex)));
        row.cells.insert(CellName::new("GradientStopPosition"), Cell::constant(Value::from_number(at).expect("a number")));
        section.rows.push(Arc::new(row));
    }
    sheet.sections.retain(|x| x.name.as_str() != "FillGradient");
    sheet.sections.push(Arc::new(section));
}

/// **The angle the reader now gives a linear gradient is taken out again where the model has none**: a gradient of DiaCraft's own is written as a `draw:gradient` of its own name,
/// whose stand-in has `FillGradientAngle`; the writer says so to the rebuild it predicts (`fx::display_of`) and to the edit it carries (`own.rs`), so a shape with no angle cell reads back with
/// none, by the model writer and by the splice, and the save is not refused.
#[test]
fn a_linear_gradient_of_diacraft_s_own_with_no_angle_reads_back_with_none_from_both_writers() {
    use diacraft_odg::{Limits, WriteOptions, read, write_with};
    let mut doc = crate::model_write::blank(1);
    linear_without_angle(&mut doc, 1);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the model writer refused the save: {e}"));
    let styles = crate::model_safe::part(&written.bytes, "styles.xml").expect("styles.xml");
    assert!(styles.contains("gradient_20_0_20_0_20_ff0000p0_20_0000ffp1000"), "the gradient of DiaCraft's own, from the top down");
    let (back, _) = read(&written.bytes, &Limits::default()).expect("it reads");
    let s = shape(&back, 0, 0);
    assert!(cell(s, "FillGradientAngle").is_none(), "no angle: {:?}", cell(s, "FillGradientAngle"));
    assert_eq!(text(cell(s, "FillGradientEnabled")), "1");
    // The splice: a shape of a file given the same gradient.
    let file = Drawing::a4()
        .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="solid" draw:fill-color="#336699"/></style:style>"##)
        .page(&crate::model_safe::rect("gr1", "2cm"))
        .bytes();
    let mut o = crate::write_core::open_bytes(file);
    let id = shape(&o.doc, 0, 0).id;
    linear_without_angle(&mut o.doc, id);
    let saved = crate::write_core::real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the splice refused the save: {e}"));
    let (back, _) = read(&saved.bytes, &Limits::default()).expect("it reads");
    let s = shape(&back, 0, 0);
    assert!(cell(s, "FillGradientAngle").is_none(), "no angle after the splice: {:?}", cell(s, "FillGradientAngle"));
    assert_eq!(text(cell(s, "FillGradientEnabled")), "1");
}

/// **What the stops cost** (counted): each stop read is an item, and so is each row the definition stands in with; past 256 stops the rest are kept in the definition and not read.
#[test]
fn each_stop_read_is_charged_and_past_two_hundred_and_fifty_six_the_rest_are_kept_and_not_read() {
    // Stops of a colour type LibreOffice draws (`rgb`) or not (`xyz`, of the same length, so the definition held whole costs the same).
    let items = |n: usize, kind: &str| {
        let stops: String = (0..n)
            .map(|i| {
                format!(
                    r##"<loext:gradient-stop svg:offset="{:.6}" loext:color-type="{kind}" loext:color-value="#{:06x}"/>"##,
                    i as f64 / (n.max(2) - 1) as f64,
                    i * 4099
                )
            })
            .collect();
        let (_, report) = Drawing::a4().common(&gradient("G", r#"draw:style="linear""#, &stops)).page("").read();
        (report.work.items, report.kept("gradient_stop"))
    };
    let (five, none_five) = (items(5, "rgb"), items(5, "xyz"));
    assert_eq!(none_five.1, 5, "five stops that are none are kept");
    // Five stops read, and five rows where the two colours stand in with two.
    assert_eq!(five.0 - none_five.0, 5 + (5 - 2));
    let (many, none_many) = (items(300, "rgb"), items(300, "xyz"));
    assert_eq!(many.1, 300 - 256, "the stops past 256 are kept");
    // 256 stops read, and the ten rows of a gradient of more stops than ten.
    assert_eq!(many.0 - none_many.0, 256 + (10 - 2));
}
