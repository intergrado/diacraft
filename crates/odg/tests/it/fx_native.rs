//! The shadows, hatches and gradients DiaCraft makes are written natively in an `.odg`, by the model writer and by the splice, and
//! read back as the same cells. What ODF has no attribute for rides in DiaCraft's data, and a program that draws the markup (LibreOffice, measured by hand: `lo_fx.rs`) draws them.
//!
//! Every read-back goes through the real reader. Every input is built in code.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::model_safe::rect;
use crate::model_safe::{part, schema_errors};
use crate::model_write::blank;
use crate::write_core::{num_of, real, shape_mut};

/// A new drawing with one rectangle (id 1), A4 portrait, as the engine makes one.
fn one_shape() -> Document {
    blank(1)
}

fn put(shape: &mut Shape, name: &str, cell: Cell) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), cell);
}

fn number(shape: &mut Shape, name: &str, value: f64) {
    put(shape, name, Cell::constant(Value::from_number(value).unwrap()));
}

fn length(shape: &mut Shape, name: &str, inches: f64) {
    put(shape, name, Cell::constant(Value::from_number(inches).unwrap()).with_unit("IN"));
}

fn colour(shape: &mut Shape, name: &str, hex: &str) {
    put(shape, name, Cell::constant(Value::from_text(hex)));
}

/// The model written from nothing, and read again: the bytes and the shape.
fn through_the_model_writer(doc: &Document) -> (Vec<u8>, Document) {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    (written.bytes, back)
}

/// The first non-background shape of the document's first page.
fn first(doc: &Document) -> &Shape {
    doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap()
}

fn cell_text(shape: &Shape, name: &str) -> Option<String> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The value of a cell as a number, or `None` for none.
fn cell_num(shape: &Shape, name: &str) -> Option<f64> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number())
}

/// The cells of the shape that are the shadow's.
const SHADOW: [&str; 9] = [
    "ShdwPattern",
    "ShapeShdwType",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwBlur",
    "ShapeShdwScaleFactor",
    "ShapeShdwObliqueAngle",
];

/// A shape's cells as `name=value` lines, for the names given: what must be the same after a save.
fn lines(shape: &Shape, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|n| match shape.sheet.cell(n) {
            // (A unit says only how a value shows: the file's own length unit is the page's.)
            // (The reader makes every colour in capitals, whatever case the file wrote it in: the case of a colour is no part of its value.)
            // (A number is compared to eight places: a share is written to nine, as the file's percentage holds it.)
            Some(c) => format!(
                "{n}={:?}",
                c.value.as_ref().map(|v| match v.number() {
                    // (`x + 0.0` is plus zero for minus zero: they are one number.)
                    Some(x) => (format!("{:.8}", x + 0.0), Some(format!("{:.8}", x + 0.0))),
                    None => (if v.text().starts_with('#') { v.text().to_ascii_lowercase() } else { v.text().to_owned() }, None),
                })
            ),
            // (The switch of a gradient that is off is 0 where it is and none where it is not: the same value, resolved, as the writer's own check takes it.)
            None if *n == "FillGradientEnabled" => format!("{n}=Some((\"0.00000000\", Some(\"0.00000000\")))"),
            None => format!("{n}=none"),
        })
        .collect()
}

/// The `style:graphic-properties` of `content.xml`'s automatic styles, as text.
fn graphic_props(bytes: &[u8]) -> String {
    let content = part(bytes, "content.xml").unwrap();
    let mut out = String::new();
    let mut rest = content.as_str();
    while let Some(at) = rest.find("<style:graphic-properties") {
        let tail = &rest[at..];
        // The tag ends at the first `/>` that follows a closing quote: an attribute's value may hold a `>` (DiaCraft's data is markup), and never a quote.
        let end = tail.find("\"/>").map_or_else(|| tail.find("/>").unwrap_or(tail.len()), |e| e + 1);
        out.push_str(&tail[..end]);
        out.push('\n');
        rest = &tail[end..];
    }
    out
}

fn valid(bytes: &[u8], what: &str) {
    if let Some(checked) = schema_errors(bytes, what) {
        checked.unwrap_or_else(|e| panic!("{what}: the ODF 1.4 schema refuses the file: {e}"));
    }
}

/// A shadow DiaCraft made: the cells the engine's `shape.shadow` writes.
fn with_shadow(doc: &mut Document, id: u32, (x, y): (f64, f64), blur: f64, trans: f64, hex: &str) {
    let s = shape_mut(doc, 0, id);
    number(s, "ShdwPattern", 1.0);
    number(s, "ShapeShdwType", 1.0);
    colour(s, "ShdwForegnd", hex);
    number(s, "ShdwForegndTrans", trans);
    number(s, "ShapeShdwScaleFactor", 1.0);
    length(s, "ShapeShdwBlur", blur);
    length(s, "ShapeShdwOffsetX", x);
    length(s, "ShapeShdwOffsetY", y);
}

#[test]
fn a_shadow_is_written_as_the_attributes_that_say_it_and_reads_back_as_the_same_cells() {
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.0625, -0.0625), 0.0556, 0.45, "#202020");
    let want = lines(first(&doc), &SHADOW);
    let (bytes, back) = through_the_model_writer(&doc);
    let props = graphic_props(&bytes);
    for attr in [
        "draw:shadow=\"visible\"",
        "draw:shadow-color=\"#202020\"",
        "draw:shadow-opacity=\"55%\"",
        "draw:shadow-offset-x=\"0.0625in\"",
        // ODF's y runs down the page and the cell's up: the offset down is positive in the file.
        "draw:shadow-offset-y=\"0.0625in\"",
    ] {
        assert!(props.contains(attr), "{attr} in {props}");
    }
    assert_eq!(lines(first(&back), &SHADOW), want, "the cells come back as they were made");
    valid(&bytes, "a shadow");
}

#[test]
fn the_blur_of_a_shadow_is_four_thirds_in_the_attribute_libreoffice_reads_and_the_cell_comes_back() {
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.0625, -0.0625), 0.06, 0.45, "#202020");
    let (bytes, back) = through_the_model_writer(&doc);
    let props = graphic_props(&bytes);
    // 0.06 in over 1.5 is the deviation DiaCraft draws; LibreOffice's blur is about twice its deviation, so the file says 0.08 in (the page's unit is the inch here).
    let at = props.find("loext:shadow-blur=\"").unwrap_or_else(|| panic!("no blur attribute: {props}"));
    let said: String = props[at + "loext:shadow-blur=\"".len()..].chars().take_while(|c| *c != '"').collect();
    assert_eq!(said, "0.08in", "{props}");
    assert!(props.contains("xmlns:loext=\"urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0\""), "{props}");
    assert!(!props.contains("draw:shadow-blur"), "LibreOffice reads no such attribute: {props}");
    assert_eq!(lines(first(&back), &SHADOW), lines(first(&doc), &SHADOW), "the cell of the blur is carried exactly");
    valid(&bytes, "a shadow with a blur");
    // No blur, no attribute; a shadow that is off says none.
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.0625, -0.0625), 0.0, 0.45, "#202020");
    assert!(!graphic_props(&through_the_model_writer(&doc).0).contains("shadow-blur"));
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.0625, -0.0625), 0.06, 0.45, "#202020");
    number(shape_mut(&mut doc, 0, 1), "ShdwPattern", 0.0);
    assert!(!graphic_props(&through_the_model_writer(&doc).0).contains("shadow-blur"));
}

#[test]
fn a_shadow_that_is_off_is_written_hidden_and_a_model_with_no_type_has_none_after_the_save() {
    let mut doc = one_shape();
    let s = shape_mut(&mut doc, 0, 1);
    number(s, "ShdwPattern", 0.0);
    let want = lines(first(&doc), &SHADOW);
    let (bytes, back) = through_the_model_writer(&doc);
    assert!(graphic_props(&bytes).contains("draw:shadow=\"hidden\""), "{}", graphic_props(&bytes));
    assert_eq!(lines(first(&back), &SHADOW), want);
    // A visible shadow of a model that holds no `ShapeShdwType` (a drawing from a VSDX file whose shadow follows its style): the reader gives the type 1 beside a visible shadow, and the
    // save takes it out again, so that the cells are the model's.
    let mut doc = one_shape();
    let s = shape_mut(&mut doc, 0, 1);
    number(s, "ShdwPattern", 1.0);
    colour(s, "ShdwForegnd", "#336699");
    let want = lines(first(&doc), &SHADOW);
    let (bytes, back) = through_the_model_writer(&doc);
    assert!(graphic_props(&bytes).contains("draw:shadow=\"visible\""));
    assert_eq!(lines(first(&back), &SHADOW), want, "no type came back");
    assert!(cell_num(first(&back), "ShapeShdwType").is_none());
    valid(&bytes, "a shadow with no type");
}

#[test]
fn a_shadow_of_another_type_or_size_is_carried_and_the_cells_come_back() {
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.1, -0.05), 0.0, 0.0, "#000000");
    let s = shape_mut(&mut doc, 0, 1);
    number(s, "ShapeShdwType", 2.0);
    number(s, "ShapeShdwScaleFactor", 0.8);
    let want = lines(first(&doc), &SHADOW);
    let (bytes, back) = through_the_model_writer(&doc);
    assert_eq!(lines(first(&back), &SHADOW), want);
    assert_eq!(cell_num(first(&back), "ShapeShdwType"), Some(2.0));
    valid(&bytes, "an oblique shadow");
}

#[test]
fn the_offset_of_a_shadow_that_is_zero_is_zero_and_never_minus_zero() {
    let mut doc = one_shape();
    with_shadow(&mut doc, 1, (0.0, 0.0), 0.0, 0.0, "#000000");
    let (bytes, back) = through_the_model_writer(&doc);
    assert!(!graphic_props(&bytes).contains("-0"), "{}", graphic_props(&bytes));
    assert_eq!(num_of(first(&back), "ShapeShdwOffsetY"), 0.0);
}

#[test]
fn the_colour_of_a_shadow_comes_back_whatever_case_it_was_made_in() {
    for hex in ["#AABBCC", "#aabbcc", "#FfEe00"] {
        let mut doc = one_shape();
        with_shadow(&mut doc, 1, (0.05, -0.05), 0.0, 0.2, hex);
        let (bytes, back) = through_the_model_writer(&doc);
        // (The reader makes every colour in capitals, whatever case the file wrote it in; the case of a colour is no part of its value.)
        assert_eq!(
            cell_text(first(&back), "ShdwForegnd").map(|c| c.to_ascii_lowercase()).as_deref(),
            Some(hex.to_ascii_lowercase().as_str()),
            "{hex}"
        );
        // A colour in lower case is the attribute's own text; one in capitals is carried, as the fill's and the line's are (`native::style_reproduced`).
        let said = graphic_props(&bytes).contains(&format!("draw:shadow-color=\"{hex}\""));
        assert_eq!(said, hex == hex.to_ascii_lowercase(), "{hex}: {}", graphic_props(&bytes));
        valid(&bytes, "a shadow colour");
    }
}

/// A drawing made by hand with one rectangle whose style is `gr1` (a solid fill), opened as a file: the splice writes into it.
fn a_file() -> (Vec<u8>, crate::write_core::Open) {
    let auto = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let bytes = Drawing::a4().auto(auto).page(&rect("gr1", "2cm")).bytes();
    let opened = crate::write_core::open_bytes(bytes.clone());
    (bytes, opened)
}

/// The shape of the file, id and all, made to be changed.
fn the_shape(o: &mut crate::write_core::Open) -> &mut Shape {
    let id = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap().id;
    shape_mut(&mut o.doc, 0, id)
}

#[test]
fn a_shadow_set_on_a_shape_of_a_file_is_written_into_its_style_and_reads_back() {
    let (_bytes, mut o) = a_file();
    let s = the_shape(&mut o);
    number(s, "ShdwPattern", 1.0);
    number(s, "ShapeShdwType", 1.0);
    colour(s, "ShdwForegnd", "#101010");
    number(s, "ShdwForegndTrans", 0.3);
    number(s, "ShapeShdwScaleFactor", 1.0);
    length(s, "ShapeShdwBlur", 0.05);
    length(s, "ShapeShdwOffsetX", 0.08);
    length(s, "ShapeShdwOffsetY", -0.08);
    let want = lines(&*s, &SHADOW);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let props = graphic_props(&saved.bytes);
    for attr in ["draw:shadow=\"visible\"", "draw:shadow-color=\"#101010\"", "draw:shadow-opacity=\"70%\""] {
        assert!(props.contains(attr), "{attr} in {props}");
    }
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(lines(first(&back), &SHADOW), want);
    // (The schema is asked of what the model writer makes; a file made by hand keeps its own version and parts, and the schema of ODF 1.4 refuses a version of 1.3.)
}

// ------------------------------------------------------------------------------------------------------------------ gradients

/// The `FillGradient` section of the stops: each a colour, a transparency and a place, in rows from `IX` 0 (the cells `shape.gradient` writes).
fn stop_section(stops: &[(&str, f64, f64)]) -> Section {
    let mut section = Section::new("FillGradient", None);
    for (i, (hex, trans, pos)) in stops.iter().enumerate() {
        let mut row = Row { ix: Some(i as u32), ..Row::default() };
        row.cells.insert(CellName::new("GradientStopColor"), Cell::constant(Value::from_text(hex)));
        row.cells.insert(CellName::new("GradientStopColorTrans"), Cell::constant(Value::from_number(*trans).unwrap()));
        row.cells.insert(CellName::new("GradientStopPosition"), Cell::constant(Value::from_number(*pos).unwrap()));
        section.rows.push(Arc::new(row));
    }
    section
}

/// A gradient DiaCraft made: the cells `shape.gradient` writes, with the angle in radians.
fn with_gradient(doc: &mut Document, id: u32, dir: f64, degrees: f64, stops: &[(&str, f64, f64)]) {
    let s = shape_mut(doc, 0, id);
    number(s, "FillPattern", 1.0);
    number(s, "FillGradientEnabled", 1.0);
    number(s, "FillGradientDir", dir);
    put(s, "FillGradientAngle", Cell::constant(Value::from_number(degrees.to_radians()).unwrap()).with_unit("DEG"));
    number(s, "RotateGradientWithShape", 1.0);
    let sheet = Arc::make_mut(&mut s.sheet);
    sheet.sections.retain(|x| x.name.as_str() != "FillGradient");
    sheet.sections.push(Arc::new(stop_section(stops)));
}

/// The cells that are a gradient's.
const GRADIENT: [&str; 5] = ["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillGradientAngle", "RotateGradientWithShape"];

/// The rows of the shape's `FillGradient` section, as text: `IX` and the cells of each.
fn rows_of(shape: &Shape) -> Vec<String> {
    let Some(section) = shape.sheet.sections.iter().find(|s| s.name.as_str() == "FillGradient") else { return vec!["no section".to_owned()] };
    section
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<String> =
                row.cells.iter().map(|(n, c)| format!("{}={:?}", n.as_str(), c.value.as_ref().map(|v| v.text().to_owned()))).collect();
            format!("{:?} {}", row.ix, cells.join(" "))
        })
        .collect()
}

/// The `draw:gradient` elements of `styles.xml`.
fn gradient_defs(bytes: &[u8]) -> Vec<String> {
    let styles = part(bytes, "styles.xml").unwrap();
    let mut out = Vec::new();
    let mut rest = styles.as_str();
    while let Some(at) = rest.find("<draw:gradient") {
        let tail = &rest[at..];
        let end =
            tail.find("</draw:gradient>").map(|e| e + "</draw:gradient>".len()).or_else(|| tail.find("/>").map(|e| e + 2)).unwrap_or(tail.len());
        // A self-closing element ends at its own `/>`, before any later gradient's close.
        let self_closing = tail.find("/>").is_some_and(|e| tail.find('>').is_some_and(|g| g + 1 == e + 2 && tail[..g].ends_with('/')));
        let end = if self_closing { tail.find("/>").map_or(end, |e| e + 2) } else { end };
        out.push(tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

#[test]
fn a_linear_gradient_is_written_as_a_gradient_of_its_own_and_reads_back_as_the_same_cells() {
    let mut doc = one_shape();
    // 90 degrees runs top to bottom with the first stop at the top: the file's angle is 0.
    with_gradient(&mut doc, 1, 0.0, 90.0, &[("#ff0000", 0.0, 0.0), ("#0000ff", 0.25, 1.0)]);
    let (want_cells, want_rows) = (lines(first(&doc), &GRADIENT), rows_of(first(&doc)));
    let (bytes, back) = through_the_model_writer(&doc);
    let props = graphic_props(&bytes);
    assert!(
        props.contains("draw:fill=\"gradient\"")
            && props.contains("draw:fill-gradient-name=\"DiaCraft_20_gradient_20_0_20_0_20_ff0000p0_20_0000ffp1000\""),
        "{props}"
    );
    let defs = gradient_defs(&bytes);
    assert_eq!(defs.len(), 1, "{defs:?}");
    assert!(
        defs[0].contains("draw:style=\"linear\"")
            && defs[0].contains("draw:angle=\"0deg\"")
            && defs[0].contains("draw:start-color=\"#ff0000\"")
            && defs[0].contains("draw:end-color=\"#0000ff\""),
        "{}",
        defs[0]
    );
    assert!(!defs[0].contains("loext"), "two stops at 0 and 1 are the two colours: {}", defs[0]);
    assert_eq!(lines(first(&back), &GRADIENT), want_cells);
    assert_eq!(rows_of(first(&back)), want_rows, "the stops, with their transparency, come back");
    valid(&bytes, "a linear gradient");
}

#[test]
fn the_angle_of_a_gradient_is_ninety_degrees_less_the_cells_and_a_stop_beyond_two_is_a_stop_of_the_file() {
    for (degrees, written) in
        [(0.0, "90deg"), (45.0, "45deg"), (90.0, "0deg"), (180.0, "270deg"), (270.0, "180deg"), (-45.0, "135deg"), (30.5, "59.5deg")]
    {
        let mut doc = one_shape();
        with_gradient(&mut doc, 1, 0.0, degrees, &[("#ff0000", 0.0, 0.0), ("#00ff00", 0.0, 0.4), ("#0000ff", 0.0, 1.0)]);
        let (bytes, back) = through_the_model_writer(&doc);
        let defs = gradient_defs(&bytes);
        assert_eq!(defs.len(), 1);
        assert!(defs[0].contains(&format!("draw:angle=\"{written}\"")), "{degrees}: {}", defs[0]);
        assert!(defs[0].contains("<loext:gradient-stop svg:offset=\"0.4\" loext:color-type=\"rgb\" loext:color-value=\"#00ff00\"/>"), "{}", defs[0]);
        let want = lines(first(&back), &["FillGradientAngle"]);
        assert!(want[0].contains("Some"), "{want:?}");
        // The exact angle comes back, to the precision a cell holds it.
        let got = cell_num(first(&back), "FillGradientAngle").unwrap();
        assert!((got - degrees.to_radians()).abs() < 1e-9, "{degrees}: {got}");
        valid(&bytes, "a gradient of three stops");
    }
}

#[test]
fn every_layout_of_a_gradient_comes_back_as_the_same_cells_and_the_path_one_is_written_as_the_rectangular() {
    for dir in 0..=13 {
        let mut doc = one_shape();
        with_gradient(&mut doc, 1, f64::from(dir), 60.0, &[("#102030", 0.0, 0.0), ("#a0b0c0", 0.5, 0.7), ("#ffffff", 0.0, 1.0)]);
        let (want_cells, want_rows) = (lines(first(&doc), &GRADIENT), rows_of(first(&doc)));
        let (bytes, back) = through_the_model_writer(&doc);
        assert_eq!(lines(first(&back), &GRADIENT), want_cells, "layout {dir}");
        assert_eq!(rows_of(first(&back)), want_rows, "layout {dir}");
        let defs = gradient_defs(&bytes);
        assert_eq!(defs.len(), 1, "layout {dir}");
        // The path gradient (13) follows the shape's outline inward, which ODF has no form for: it is written as the rectangular one from the centre, and the exact layout is carried.
        let style = match dir {
            0 => "linear",
            1..=7 => "radial",
            _ => "rectangular",
        };
        assert!(defs[0].contains(&format!("draw:style=\"{style}\"")), "layout {dir}: {}", defs[0]);
        valid(&bytes, &format!("a gradient of layout {dir}"));
    }
}

// ------------------------------------------------------------------------------------------------------------------ hatches

/// A pattern DiaCraft made: the cells `shape.format fill_pattern` writes (marks, ground and the pattern).
fn with_pattern(doc: &mut Document, id: u32, pattern: u8, marks: &str, ground: &str) {
    let s = shape_mut(doc, 0, id);
    number(s, "FillPattern", f64::from(pattern));
    colour(s, "FillForegnd", marks);
    colour(s, "FillBkgnd", ground);
    number(s, "FillGradientEnabled", 0.0);
}

const PATTERN: [&str; 4] = ["FillPattern", "FillForegnd", "FillBkgnd", "FillGradientEnabled"];

/// The `draw:hatch` elements of `styles.xml`.
fn hatch_defs(bytes: &[u8]) -> Vec<String> {
    let styles = part(bytes, "styles.xml").unwrap();
    styles.split("<draw:hatch").skip(1).map(|s| format!("<draw:hatch{}", s.split("/>").next().unwrap_or(""))).collect()
}

#[test]
fn a_pattern_of_lines_is_a_hatch_over_its_ground_and_every_pattern_comes_back_as_the_same_cells() {
    // Patterns 1 to 24, the lines and the stipples: a hatch for the lines (2 to 7, 13 to 16, 19 to 23), and for the stipples, which are one flat mixture of the two colours, that colour.
    let lines_of_a_hatch = [2u8, 3, 4, 5, 6, 7, 13, 14, 15, 16, 19, 20, 21, 22, 23];
    for pattern in 1..=24u8 {
        let mut doc = one_shape();
        with_pattern(&mut doc, 1, pattern, "#a02010", "#fff0c0");
        let want = lines(first(&doc), &PATTERN);
        let (bytes, back) = through_the_model_writer(&doc);
        assert_eq!(lines(first(&back), &PATTERN), want, "pattern {pattern}");
        let hatches = hatch_defs(&bytes);
        let props = graphic_props(&bytes);
        if lines_of_a_hatch.contains(&pattern) {
            assert_eq!(hatches.len(), 1, "pattern {pattern}: {hatches:?}");
            assert!(
                props.contains("draw:fill=\"hatch\"")
                    && props.contains("draw:fill-hatch-solid=\"true\"")
                    && props.contains("draw:fill-color=\"#fff0c0\""),
                "pattern {pattern}: {props}"
            );
            assert!(props.contains(&format!("draw:fill-hatch-name=\"DiaCraft_20_hatch_20_{pattern}_20_a02010\"")), "pattern {pattern}: {props}");
            assert!(hatches[0].contains("draw:color=\"#a02010\""), "{}", hatches[0]);
        } else if pattern == 1 {
            assert!(hatches.is_empty() && props.contains("draw:fill-color=\"#a02010\""), "pattern 1 is the solid fill: {props}");
        } else {
            // A stipple is the one flat colour it is drawn in: the marks and the ground blended by the pattern's share.
            assert!(hatches.is_empty(), "pattern {pattern}: no hatch");
            assert!(props.contains("draw:fill=\"solid\""), "pattern {pattern}: {props}");
            assert!(
                !props.contains("draw:fill-color=\"#a02010\"") && !props.contains("draw:fill-color=\"#fff0c0\""),
                "pattern {pattern}: a mixture, not a colour of the two: {props}"
            );
        }
        valid(&bytes, &format!("pattern {pattern}"));
    }
}

#[test]
fn a_hatch_that_is_turned_into_a_solid_fill_reads_back_with_the_marks_as_the_colour() {
    let mut doc = one_shape();
    with_pattern(&mut doc, 1, 1, "#a02010", "#fff0c0");
    let (bytes, back) = through_the_model_writer(&doc);
    assert!(graphic_props(&bytes).contains("draw:fill=\"solid\"") && graphic_props(&bytes).contains("draw:fill-color=\"#a02010\""));
    assert_eq!(lines(first(&back), &PATTERN), lines(first(&doc), &PATTERN));
    assert!(hatch_defs(&bytes).is_empty());
}

// ------------------------------------------------------------------------------------------------------------------ the splice

/// A rectangle of the model, as a new shape of the file (`id` the next one the file's index gives).
fn a_new_shape_of(o: &mut crate::write_core::Open, name: &str) -> u32 {
    // The index counts the shapes of the file; the ones the test has added since are the model's alone.
    let added = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| s.id).max().unwrap_or(0);
    let id = o.index.next_id(0).unwrap().max(added + 1);
    crate::write_new::push_shape(&mut o.doc, crate::write_new::rectangle(id, name, (8.0 + f64::from(id) * 3.0, 3.0, 2.0, 2.0)));
    id
}

#[test]
fn a_new_shape_with_a_gradient_a_hatch_and_a_shadow_is_written_into_a_file_and_the_definitions_go_in_its_office_styles() {
    let (_bytes, mut o) = a_file();
    let (g, h, s) = (a_new_shape_of(&mut o, "G"), a_new_shape_of(&mut o, "H"), a_new_shape_of(&mut o, "S"));
    with_gradient(&mut o.doc, g, 3.0, 0.0, &[("#ff0000", 0.0, 0.0), ("#00ff00", 0.0, 0.3), ("#0000ff", 0.0, 1.0)]);
    with_pattern(&mut o.doc, h, 4, "#202020", "#ffff00");
    with_shadow(&mut o.doc, s, (0.1, -0.1), 0.05, 0.4, "#303030");
    let want: Vec<Vec<String>> = [(g, &GRADIENT[..]), (h, &PATTERN[..]), (s, &SHADOW[..])]
        .iter()
        .map(|(id, names)| {
            let shape = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().find(|x| x.id == *id).unwrap();
            lines(shape, names)
        })
        .collect();
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    assert_eq!(gradient_defs(&saved.bytes).len(), 1, "{:?}", gradient_defs(&saved.bytes));
    assert_eq!(hatch_defs(&saved.bytes).len(), 1);
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let page = back.pages.iter().find(|p| !p.background).unwrap();
    for (k, (name, names)) in [("G", &GRADIENT[..]), ("H", &PATTERN[..]), ("S", &SHADOW[..])].iter().enumerate() {
        let shape = page.shapes.iter().find(|x| x.name_u.as_deref() == Some(*name)).unwrap();
        assert_eq!(lines(shape, names), want[k], "{name}");
    }
}

#[test]
fn a_gradient_set_on_a_shape_of_a_file_is_written_into_its_style_and_changed_later_it_leaves_one_definition() {
    let (_bytes, mut o) = a_file();
    let id = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap().id;
    with_gradient(&mut o.doc, id, 0.0, 30.0, &[("#ff0000", 0.0, 0.0), ("#0000ff", 0.1, 1.0)]);
    let (want_cells, want_rows) = (lines(first(&o.doc), &GRADIENT), rows_of(first(&o.doc)));
    let first_save = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let props = graphic_props(&first_save.bytes);
    assert!(
        props.contains("draw:fill=\"gradient\"")
            && props.contains("draw:fill-gradient-name=\"DiaCraft_20_gradient_20_0_20_600_20_ff0000p0_20_0000ffp1000\""),
        "{props}"
    );
    assert_eq!(gradient_defs(&first_save.bytes).len(), 1);
    let (back, _) = read(&first_save.bytes, &Limits::default()).unwrap();
    assert_eq!(lines(first(&back), &GRADIENT), want_cells);
    assert_eq!(rows_of(first(&back)), want_rows);
    // The file is opened again and the gradient changed: the new definition is there, and the old one, which no style names, is gone.
    let mut again = crate::write_core::open_bytes(first_save.bytes.clone());
    let id = again.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap().id;
    with_gradient(&mut again.doc, id, 3.0, 0.0, &[("#00ff00", 0.0, 0.0), ("#ffff00", 0.0, 0.5), ("#0000ff", 0.0, 1.0)]);
    let want = (lines(first(&again.doc), &GRADIENT), rows_of(first(&again.doc)));
    let second = real(&again.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("the second save was refused: {e}"));
    let defs = gradient_defs(&second.bytes);
    assert_eq!(defs.len(), 1, "one definition: {defs:?}");
    assert!(
        defs[0].contains("DiaCraft_20_gradient_20_3_20_0_20_00ff00p0_20_ffff00p500_20_0000ffp1000") && defs[0].contains("draw:style=\"radial\""),
        "{}",
        defs[0]
    );
    let (back, _) = read(&second.bytes, &Limits::default()).unwrap();
    assert_eq!((lines(first(&back), &GRADIENT), rows_of(first(&back))), want);
    // The gradient taken off: the fill is solid again and no definition is left.
    let mut third = crate::write_core::open_bytes(second.bytes.clone());
    let id = third.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap().id;
    number(shape_mut(&mut third.doc, 0, id), "FillGradientEnabled", 0.0);
    let off = real(&third.doc, &third.bytes, &third.index).unwrap_or_else(|e| panic!("the save with the gradient off was refused: {e}"));
    assert!(gradient_defs(&off.bytes).is_empty(), "{:?}", gradient_defs(&off.bytes));
    assert!(graphic_props(&off.bytes).contains("draw:fill=\"solid\""), "{}", graphic_props(&off.bytes));
}

// ------------------------------------------------------------------------------------------------------------------ files of other programs

/// A drawing as another program writes one: a gradient of its own name (two stops and a third as a `loext:gradient-stop`), a hatch, and a shape on each.
fn a_foreign_file() -> Vec<u8> {
    let common = r##"<draw:gradient draw:name="Gradient_20_1" draw:display-name="Gradient 1" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:start-intensity="100%" draw:end-intensity="100%" draw:angle="30deg" draw:border="0%"/><draw:hatch draw:name="Hatch_20_1" draw:display-name="Hatch 1" draw:style="single" draw:color="#000000" draw:distance="0.1in" draw:rotation="450"/>"##;
    let auto = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="gradient" draw:fill-gradient-name="Gradient_20_1"/></style:style><style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="hatch" draw:fill-hatch-name="Hatch_20_1" draw:fill-hatch-solid="true" draw:fill-color="#ffff00" draw:shadow="visible" draw:shadow-color="#000000" draw:shadow-opacity="50%" draw:shadow-offset-x="0.2cm" draw:shadow-offset-y="0.2cm"/></style:style>"##;
    Drawing::a4().common(common).auto(auto).page(&format!("{}{}", rect("gr1", "2cm"), rect("gr2", "8cm"))).bytes()
}

#[test]
fn the_gradients_and_hatches_of_other_programs_stay_as_they_are_through_a_save_of_the_model_and_of_the_splice() {
    let source = a_foreign_file();
    // The model writer: the drawing is read and written from the model alone.
    let written = crate::model_safe::visit(&source);
    let styles = part(&written.bytes, "styles.xml").unwrap();
    assert!(styles.contains("draw:name=\"Gradient_20_1\"") && styles.contains("draw:name=\"Hatch_20_1\""), "{styles}");
    assert!(!styles.contains("DiaCraft_20_gradient") && !styles.contains("DiaCraft_20_hatch"), "{styles}");
    let props = graphic_props(&written.bytes);
    assert!(props.contains("draw:fill-gradient-name=\"Gradient_20_1\"") && props.contains("draw:fill-hatch-name=\"Hatch_20_1\""), "{props}");
    let diff = diacraft_odg::compare_content(&source, &written.bytes, &Limits::default()).unwrap();
    let drawing: Vec<_> =
        diff.shown.iter().filter(|d| d.part == "content.xml" || d.part == "styles.xml").map(|d| format!("{} {}", d.part, d.path)).collect();
    assert!(drawing.is_empty(), "{drawing:?}");
    // The splice: the fill colour of the gradient's shape is edited, and the shadow of the other is turned off. Another program's gradient and hatch stay.
    let mut o = crate::write_core::open_bytes(source.clone());
    let ids: Vec<u32> = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| s.id).collect();
    colour(shape_mut(&mut o.doc, 0, ids[0]), "FillForegnd", "#123456");
    number(shape_mut(&mut o.doc, 0, ids[1]), "ShdwPattern", 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let styles = part(&saved.bytes, "styles.xml").unwrap();
    assert!(
        styles.contains("draw:name=\"Gradient_20_1\"") && styles.contains("draw:name=\"Hatch_20_1\"") && !styles.contains("DiaCraft_20_"),
        "{styles}"
    );
    let props = graphic_props(&saved.bytes);
    assert!(
        props.contains("draw:fill-gradient-name=\"Gradient_20_1\"")
            && props.contains("draw:fill-hatch-name=\"Hatch_20_1\"")
            && props.contains("draw:shadow=\"hidden\""),
        "{props}"
    );
}

// ------------------------------------------------------------------------------------------------------------------ any look at all

/// A deterministic stream of numbers (SplitMix64): the looks below are made from it, so a failure is a seed that can be run again.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A number from 0 to 1 that is a multiple of 1/256: exact in binary and in the nine places a share is written to, so that a read-back is compared as text.
    fn unit(&mut self) -> f64 {
        (self.next() % 257) as f64 / 256.0
    }
}

/// The rounds of a property test: `default`, or `ODG_FX_ROUNDS` times as many tenths of it when that is set (a longer run by hand).
fn rounds(default: u32) -> u32 {
    std::env::var("ODG_FX_ROUNDS").ok().and_then(|n| n.parse::<u32>().ok()).map_or(default, |n| default.saturating_mul(n.min(100)))
}

/// The seed of a property test: 0, or `ODG_FX_SEED`.
fn seed() -> u64 {
    std::env::var("ODG_FX_SEED").ok().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0)
}

/// A colour in the forms the cells hold one in: lower or upper case, or a palette index, or a text that is none.
fn any_colour(rng: &mut Rng) -> Cell {
    let (r, g, b) = (rng.below(256), rng.below(256), rng.below(256));
    match rng.below(8) {
        0 => Cell::constant(Value::from_text(&format!("#{r:02X}{g:02X}{b:02X}"))),
        1 => Cell::constant(Value::from_number(rng.below(24) as f64).unwrap()),
        2 => Cell::constant(Value::from_text("Themed")),
        3 => {
            // A colour with a formula of its own and the value it was worked out to, as `shape.gradient` writes one.
            let mut cell = Cell::constant(Value::from_text(&format!("#{r:02x}{g:02x}{b:02x}")));
            cell.formula = diacraft_doc::FormulaSlot::Local(Arc::new(diacraft_formula::parse(&format!("RGB({r},{g},{b})"))));
            cell
        }
        _ => Cell::constant(Value::from_text(&format!("#{r:02x}{g:02x}{b:02x}"))),
    }
}

/// A look made from the stream: a gradient, a pattern, a shadow or all of them, with the odd cells a drawing can hold.
fn any_look(shape: &mut Shape, rng: &mut Rng) {
    if rng.below(2) == 0 {
        number(shape, "FillPattern", if rng.below(3) == 0 { rng.below(60) as f64 } else { 1.0 });
        number(shape, "FillGradientEnabled", rng.below(2) as f64);
        number(shape, "FillGradientDir", rng.below(16) as f64);
        if rng.below(5) != 0 {
            put(shape, "FillGradientAngle", Cell::constant(Value::from_number(rng.unit() * 20.0 - 10.0).unwrap()).with_unit("DEG"));
        }
        let rows = 1 + rng.below(5) as u32;
        let mut section = Section::new("FillGradient", None);
        for i in 0..rows {
            let mut row = Row { ix: Some(i), del: rng.below(12) == 0, ..Row::default() };
            row.cells.insert(CellName::new("GradientStopColor"), any_colour(rng));
            row.cells
                .insert(CellName::new("GradientStopColorTrans"), Cell::constant(Value::from_number(f64::from(rng.below(3) as u32) * 0.25).unwrap()));
            let pos = if rng.below(8) == 0 { rng.unit() * 3.0 - 1.0 } else { rng.unit() };
            row.cells.insert(CellName::new("GradientStopPosition"), Cell::constant(Value::from_number(pos).unwrap()));
            section.rows.push(Arc::new(row));
        }
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|s| s.name.as_str() != "FillGradient");
        sheet.sections.push(Arc::new(section));
    } else {
        number(shape, "FillPattern", rng.below(30) as f64);
        put(shape, "FillForegnd", any_colour(rng));
        put(shape, "FillBkgnd", any_colour(rng));
        if rng.below(3) == 0 {
            number(shape, "FillForegndTrans", rng.below(3) as f64 * 0.25);
            number(shape, "FillBkgndTrans", rng.below(3) as f64 * 0.25);
        }
        if rng.below(2) == 0 {
            number(shape, "FillGradientEnabled", 0.0);
        }
    }
    if rng.below(2) == 0 {
        number(shape, "ShdwPattern", rng.below(3) as f64);
        if rng.below(3) != 0 {
            number(shape, "ShapeShdwType", rng.below(4) as f64);
        }
        put(shape, "ShdwForegnd", any_colour(rng));
        number(shape, "ShdwForegndTrans", rng.unit());
        number(shape, "ShapeShdwScaleFactor", 0.5 + rng.unit());
        length(shape, "ShapeShdwBlur", rng.unit() * 0.2);
        length(shape, "ShapeShdwOffsetX", rng.unit() - 0.5);
        length(shape, "ShapeShdwOffsetY", rng.unit() - 0.5);
    }
}

/// The cells that a look is made of, in a form to compare: the case of a colour and a switch that is off and absent are no difference.
const LOOK: [&str; 21] = [
    "FillPattern",
    "FillGradientEnabled",
    "FillGradientDir",
    "FillGradientAngle",
    "FillForegnd",
    "FillBkgnd",
    "FillForegndTrans",
    "FillBkgndTrans",
    "ShdwPattern",
    "ShapeShdwType",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwBlur",
    "ShapeShdwScaleFactor",
    "ShapeShdwObliqueAngle",
    "RotateGradientWithShape",
    "LinePattern",
    "LineColor",
    "FillStyle",
];

/// The lines of the shapes that differ, as `shape k: got / want`, so that a failure names the cells.
fn differences(got: &[Vec<String>], want: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        for (a, b) in g.iter().zip(w) {
            if a != b {
                out.push_str(&format!("shape {k}: got {a} / want {b}\n"));
            }
        }
    }
    out
}

/// What a cell holds as the formula it has and the value, for a comparison that ignores the case of a colour and the unit.
fn full_lines(shape: &Shape) -> Vec<String> {
    let mut out = lines(shape, &LOOK);
    out.push(rows_of(shape).join("|").to_ascii_lowercase());
    out
}

#[test]
fn any_look_a_drawing_holds_is_written_and_read_back_as_the_same_cells_or_refused_in_words() {
    let mut rng = Rng(0x000d_6f0f ^ seed());
    let (mut written, mut refused, mut gradients, mut hatches, mut shadows) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for round in 0..rounds(400) {
        // Four shapes of a new drawing, each with a look of its own, saved by the model writer and, as new shapes of a file, by the splice.
        let mut doc = blank(0);
        let mut ids = Vec::new();
        for k in 0..4u32 {
            let id = k + 1;
            let mut shape = crate::write_new::rectangle(id, &format!("S{id}"), (2.0 + f64::from(k) * 3.0, 3.0, 2.0, 2.0));
            any_look(&mut shape, &mut rng);
            crate::write_new::push_shape(&mut doc, shape);
            ids.push(id);
        }
        let before: Vec<Vec<String>> = doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
        match write_with(&doc, None, &WriteOptions::default()) {
            Ok(w) => {
                written += 1;
                gradients += gradient_defs(&w.bytes).len() as u32;
                hatches += hatch_defs(&w.bytes).len() as u32;
                shadows += graphic_props(&w.bytes).matches("draw:shadow=\"visible\"").count() as u32;
                let (back, _) = read(&w.bytes, &Limits::default()).unwrap_or_else(|e| panic!("round {round}: the file does not read: {e}"));
                let after: Vec<Vec<String>> = back.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
                assert!(after == before, "round {round}: the model writer\n{}", differences(&after, &before));
                if round % 40 == 0 {
                    valid(&w.bytes, &format!("random look {round}"));
                }
            }
            Err(e) => {
                refused += 1;
                let words = e.to_string();
                assert!(
                    !words.is_empty() && !matches!(e, diacraft_odg::WriteError::SelfCheck(_) | diacraft_odg::WriteError::Mismatch(_)),
                    "round {round}: {e}"
                );
            }
        }
        // The same shapes as new shapes of a file.
        let (_bytes, mut o) = a_file();
        let base = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| s.id).max().unwrap_or(0);
        let mut want = Vec::new();
        for (k, shape) in doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().enumerate() {
            let mut moved = (**shape).clone();
            moved.id = base + 1 + k as u32;
            moved.name_u = Some(Arc::from(format!("S{}", k + 1)));
            want.push(full_lines(&moved));
            crate::write_new::push_shape(&mut o.doc, moved);
        }
        match real(&o.doc, &o.bytes, &o.index) {
            Ok(w) => {
                let (back, _) = read(&w.bytes, &Limits::default()).unwrap_or_else(|e| panic!("round {round}: the spliced file does not read: {e}"));
                let page = back.pages.iter().find(|p| !p.background).unwrap();
                let after: Vec<Vec<String>> =
                    (1..=4).map(|k| full_lines(page.shapes.iter().find(|s| s.name_u.as_deref() == Some(&format!("S{k}"))).unwrap())).collect();
                assert_eq!(after, want, "round {round}: the splice");
            }
            Err(e) => assert!(
                !matches!(e, diacraft_odg::WriteError::SelfCheck(_) | diacraft_odg::WriteError::Mismatch(_)),
                "round {round}: the splice: {e}"
            ),
        }
    }
    // The drawings were not all refused, and the looks were of every kind.
    assert!(written > 300, "written {written}, refused {refused}");
    assert!(gradients > 40 && hatches > 100 && shadows > 100, "gradients {gradients}, hatches {hatches}, shadows {shadows}");
}

#[test]
fn any_look_set_on_the_shapes_of_a_file_twice_is_written_and_read_back_as_the_same_cells_or_refused_in_words() {
    let auto = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let shapes: String = (0..4).map(|k| rect("gr1", &format!("{}cm", 1 + 4 * k))).collect();
    let source = Drawing::a4().auto(auto).page(&shapes).bytes();
    let mut rng = Rng(0x0000_ed17 ^ seed());
    let (mut saved, mut refused) = (0u32, 0u32);
    for round in 0..rounds(120) {
        let mut o = crate::write_core::open_bytes(source.clone());
        let ids: Vec<u32> = o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| s.id).collect();
        for id in &ids {
            if rng.below(4) != 0 {
                any_look(shape_mut(&mut o.doc, 0, *id), &mut rng);
            }
        }
        let mut current = o;
        // Two saves: the looks as the first edit leaves them, then changed again in the file that save made.
        for save in 0..2 {
            let want: Vec<Vec<String>> = current.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
            match real(&current.doc, &current.bytes, &current.index) {
                Ok(w) => {
                    saved += 1;
                    let (back, _) =
                        read(&w.bytes, &Limits::default()).unwrap_or_else(|e| panic!("round {round} save {save}: the file does not read: {e}"));
                    let after: Vec<Vec<String>> = back.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
                    assert!(
                        after == want,
                        "round {round} save {save}: the splice\n{}\nBEFORE:\n{}\nAFTER:\n{}",
                        differences(&after, &want),
                        graphic_props(&current.bytes).replace("&lt;", "<").replace("&quot;", "\""),
                        graphic_props(&w.bytes).replace("&lt;", "<").replace("&quot;", "\"")
                    );
                    let mut next = crate::write_core::open_bytes(w.bytes.clone());
                    let ids: Vec<u32> = next.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| s.id).collect();
                    for id in &ids {
                        if rng.below(3) == 0 {
                            any_look(shape_mut(&mut next.doc, 0, *id), &mut rng);
                        }
                    }
                    current = next;
                }
                Err(e) => {
                    refused += 1;
                    assert!(
                        !matches!(e, diacraft_odg::WriteError::SelfCheck(_) | diacraft_odg::WriteError::Mismatch(_)),
                        "round {round} save {save}: {e}"
                    );
                    break;
                }
            }
        }
    }
    assert!(saved > 150, "saved {saved}, refused {refused}");
}

// ------------------------------------------------------------------------------------------------------------------ a flat drawing

#[test]
fn a_new_shape_with_a_gradient_a_hatch_and_a_shadow_is_written_into_a_flat_drawing_and_its_definitions_go_in_the_one_document() {
    let mut o = crate::write_core::open_bytes(crate::flat_write::three().bytes());
    let (g, h, s) = (a_new_shape_of(&mut o, "G"), a_new_shape_of(&mut o, "H"), a_new_shape_of(&mut o, "S"));
    with_gradient(&mut o.doc, g, 1.0, 0.0, &[("#ff0000", 0.0, 0.0), ("#0000ff", 0.0, 1.0)]);
    with_pattern(&mut o.doc, h, 22, "#202020", "#ffff00");
    with_shadow(&mut o.doc, s, (0.1, -0.1), 0.05, 0.4, "#303030");
    let want: Vec<Vec<String>> = [(g, &GRADIENT[..]), (h, &PATTERN[..]), (s, &SHADOW[..])]
        .iter()
        .map(|(id, names)| lines(o.doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().find(|x| x.id == *id).unwrap(), names))
        .collect();
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let text = crate::flat_write::text_of(&saved.bytes);
    // The definitions are in the one `office:styles` of the document, once each, and the styles name them.
    let styles =
        text.split("<office:styles>").nth(1).and_then(|r| r.split("</office:styles>").next()).unwrap_or_else(|| panic!("no office:styles in {text}"));
    assert_eq!(styles.matches("<draw:gradient ").count(), 1, "{styles}");
    assert_eq!(styles.matches("<draw:hatch ").count(), 1, "{styles}");
    assert!(
        text.contains("draw:fill-gradient-name=\"DiaCraft_20_gradient_20_1_20_0_20_ff0000p0_20_0000ffp1000\"")
            && text.contains("draw:shadow=\"visible\""),
        "{text}"
    );
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let page = back.pages.iter().find(|p| !p.background).unwrap();
    for (k, (name, names)) in [("G", &GRADIENT[..]), ("H", &PATTERN[..]), ("S", &SHADOW[..])].iter().enumerate() {
        let shape = page.shapes.iter().find(|x| x.name_u.as_deref() == Some(*name)).unwrap();
        assert_eq!(lines(shape, names), want[k], "{name}");
    }
}

// ------------------------------------------------------------------------------------------------------------------ cost

/// A drawing of `n` shapes, each with a gradient of its own (three stops, a colour from its number), a hatch of its own and a shadow, written by the model writer: the work counted.
fn counted(n: u32) -> (diacraft_odg::WriteWork, usize, usize) {
    let mut doc = blank(0);
    for k in 0..n {
        let id = k + 1;
        let mut shape = crate::write_new::rectangle(id, &format!("S{id}"), (2.0 + f64::from(k % 40) * 0.5, 3.0, 0.4, 0.4));
        let (r, g, b) = (k % 256, (k / 256) % 256, (k / 65_536) % 256);
        let hex = |x: u32, y: u32, z: u32| format!("#{x:02x}{y:02x}{z:02x}");
        if k % 2 == 0 {
            let (c1, c2, c3) = (hex(r, g, b), hex(255 - r, g, b), hex(r, 255 - g, b));
            let sheet = Arc::make_mut(&mut shape.sheet);
            sheet.cells.insert(CellName::new("FillPattern"), Cell::constant(Value::from_number(1.0).unwrap()));
            sheet.cells.insert(CellName::new("FillGradientEnabled"), Cell::constant(Value::from_number(1.0).unwrap()));
            sheet.cells.insert(CellName::new("FillGradientDir"), Cell::constant(Value::from_number(f64::from(k % 14)).unwrap()));
            sheet.sections.push(Arc::new(stop_section(&[(&c1, 0.0, 0.0), (&c2, 0.0, 0.4), (&c3, 0.0, 1.0)])));
        } else {
            number(&mut shape, "FillPattern", 2.0 + f64::from(k % 6));
            colour(&mut shape, "FillForegnd", &hex(r, g, b));
            colour(&mut shape, "FillBkgnd", &hex(255 - r, g, 255 - b));
        }
        number(&mut shape, "ShdwPattern", 1.0);
        number(&mut shape, "ShapeShdwType", 1.0);
        length(&mut shape, "ShapeShdwBlur", 0.05);
        crate::write_new::push_shape(&mut doc, shape);
    }
    let mut work = diacraft_odg::WriteWork::default();
    let written = diacraft_odg::write_counted(&doc, None, &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("the save of {n} shapes was refused: {e}"));
    (work, gradient_defs(&written.bytes).len(), hatch_defs(&written.bytes).len())
}

/// The work of a save with a definition for each shape is **linear in the shapes**: counted (never timed), twice the shapes cost twice the work and no more than a little over, whatever each
/// counter is, and the file holds a definition of each kind that was asked for (a gradient or a hatch of each shape, by a name made from its number).
#[test]
fn the_work_of_a_save_with_a_gradient_or_a_hatch_for_each_shape_is_linear_in_the_shapes() {
    let (small, small_gradients, small_hatches) = counted(300);
    let (big, big_gradients, big_hatches) = counted(600);
    assert_eq!((small_gradients, small_hatches), (150, hatch_kinds(300)), "300 shapes");
    assert_eq!((big_gradients, big_hatches), (300, hatch_kinds(600)), "600 shapes");
    for (what, a, b) in [
        ("attributes written", small.attributes_written, big.attributes_written),
        ("bytes written", small.bytes_written, big.bytes_written),
        ("elements walked", small.elements_walked, big.elements_walked),
        ("edits", small.edits, big.edits),
        ("order steps", small.order_steps, big.order_steps),
        ("scope steps", small.scope_steps, big.scope_steps),
        ("cells compared", small.cells_compared, big.cells_compared),
    ] {
        // (Twice the shapes, a little more than twice the bytes: the numbers in the names and the ids have more digits.)
        assert!(b <= 2 * a + a / 50 + 200, "{what}: {a} for 300 shapes and {b} for 600 is more than linear");
        assert!(b >= a, "{what}: {a} then {b}");
    }
}

/// How many hatches the shapes of [`counted`] have between them: the number of different (pattern, marks colour) pairs among the odd shapes of `n`.
fn hatch_kinds(n: u32) -> usize {
    (0..n).filter(|k| k % 2 == 1).map(|k| (2 + k % 6, k % 256, (k / 256) % 256, (k / 65_536) % 256)).collect::<std::collections::BTreeSet<_>>().len()
}

// ------------------------------------------------------------------------------------------------------------------ a save by another program

/// A package of `bytes` with `change` made to its `content.xml` and `styles.xml`: what a program that saves it again may do to the parts.
fn with_parts_changed(bytes: &[u8], change: impl Fn(&str, String) -> String) -> Vec<u8> {
    let pkg = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    let entries: Vec<(String, Vec<u8>)> = pkg
        .parts()
        .filter(|name| *name != "mimetype")
        .map(|name| {
            let data = pkg.part(name).unwrap().to_vec();
            let data = if matches!(name, "content.xml" | "styles.xml") { change(name, String::from_utf8(data).unwrap()).into_bytes() } else { data };
            (name.to_owned(), data)
        })
        .collect();
    crate::build::write(Some("application/vnd.oasis.opendocument.graphics"), &entries)
}

/// A save by LibreOffice (26.8.2.2, measured) gives every shape of a file the parent style `standard`, whose fill is solid: the solid stand-in of a hatch or a stipple is then what the parent
/// gives, and the reader leaves no local `FillPattern`. A fingerprint of that cell would call the carried pattern stale and drop it, so a display form's pattern is **not judged**: the
/// pattern of every shape of the matrix reads back after such a save.
#[test]
fn a_hatch_and_a_stipple_keep_their_pattern_when_a_program_gives_the_shape_a_parent_style_with_a_solid_fill() {
    let mut doc = one_shape();
    with_pattern(&mut doc, 1, 4, "#a02010", "#fff0c0");
    let second = crate::write_new::rectangle(2, "S2", (5.0, 3.0, 2.0, 2.0));
    crate::write_new::push_shape(&mut doc, second);
    with_pattern(&mut doc, 2, 9, "#a02010", "#fff0c0");
    let third = crate::write_new::rectangle(3, "S3", (8.0, 3.0, 2.0, 2.0));
    crate::write_new::push_shape(&mut doc, third);
    with_gradient(&mut doc, 3, 0.0, 30.0, &[("#ff0000", 0.0, 0.0), ("#0000ff", 0.0, 1.0)]);
    number(shape_mut(&mut doc, 0, 3), "FillPattern", 5.0);
    let want: Vec<Vec<String>> = doc.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
    let (bytes, _) = through_the_model_writer(&doc);
    let resaved = with_parts_changed(&bytes, |name, text| {
        match name {
        "content.xml" => text.replace("style:family=\"graphic\"", "style:family=\"graphic\" style:parent-style-name=\"standard\""),
        _ => text.replacen(
            "<office:styles>",
            "<office:styles><style:style style:name=\"standard\" style:family=\"graphic\"><style:graphic-properties draw:stroke=\"solid\" draw:fill=\"solid\" draw:fill-color=\"#18a303\"/></style:style>",
            1,
        ),
    }
    });
    assert!(part(&resaved, "styles.xml").unwrap().contains("style:name=\"standard\""), "the parent is there");
    let (back, _) = read(&resaved, &Limits::default()).unwrap();
    let after: Vec<Vec<String>> = back.pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|s| full_lines(s)).collect();
    // The patterns come back; a cell the parent now gives is no local cell, and the resolved values are what is compared (a switch that is off and none are one).
    for (k, (a, w)) in after.iter().zip(&want).enumerate() {
        assert_eq!(a.first(), w.first(), "shape {k}: the pattern");
        assert_eq!(a.get(4), w.get(4), "shape {k}: the marks' colour");
    }
}
