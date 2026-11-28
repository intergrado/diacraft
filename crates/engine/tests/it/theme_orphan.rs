//! A themed drawing that loses its colours: each of two things stops the theme on its own, and a file may hold both.
//!
//! 1. The theme part is in the package and in its content types, and no relationship of the document part names it: it is found by its content type.
//! 2. The style cells hold `THEMEVAL()` as a formula and no saved value: a cell with a formula and no value is worked out, as the same formula with the
//!    marker beside it is, and not drawn from a default (the root style's white fill and black line).
//!
//! The package is built in code from the specifications' XML: each of the four combinations is a drawing
//! of one rectangle that takes its fill and line from the `Theme` style, on a theme whose first accent is `#5b9bd5`. A drawing with no theme part is
//! the control: it keeps the root style's colours, as the specification says.

use diacraft_engine::Session;
use diacraft_testkit::package::{DIR, PageSpec, Pkg, VsdxBuilder, cell, shape_with};
use diacraft_testkit::theme::ThemeXml;
use serde_json::json;

use crate::native_common::Dir;
use crate::run;

const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const ACCENT1: &str = "#5b9bd5";

/// The root style as a file holds it, as far as a theme goes: a white fill, a black line, and the cells a themed marker falls back to.
const ROOT: &str = r##"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#ffffff"/><Cell N="FillBkgnd" V="#000000"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/><Section N="Character"><Row IX="0"><Cell N="Color" V="#000000"/><Cell N="Size" V="0.1666666666666667" U="PT"/></Row></Section></StyleSheet>"##;

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The cells of the `Theme` style that hold the call: with the marker beside it (`saved`), or as a formula alone.
const THEMED: [&str; 6] = ["FillPattern", "FillForegnd", "FillBkgnd", "LinePattern", "LineColor", "LineWeight"];

fn theme_style(saved: bool) -> String {
    let value = if saved { r#" V="Themed""# } else { "" };
    let cells: String = THEMED.iter().map(|n| format!(r#"<Cell N="{n}"{value} F="THEMEVAL()"/>"#)).collect();
    format!(
        r#"<StyleSheet ID="3" NameU="Theme" Name="Theme" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="EnableLineProps" V="1"/><Cell N="EnableFillProps" V="1"/><Cell N="EnableTextProps" V="1"/>{cells}</StyleSheet>"#
    )
}

fn theme_bytes() -> Vec<u8> {
    let colours = ["111111", "fefefe", "44546a", "e7e6e6", "5b9bd5", "ed7d31", "a5a5a5", "ffc000", "4472c4", "70ae47", "0563c1", "954f72"];
    ThemeXml { colours, ..ThemeXml::default() }.xml().into_bytes()
}

/// A box that takes its fill and line from the `Theme` style, and holds its scheme and Quick Style cells (accent1 for the fill and the line, entry 1).
fn themed_box(id: u32) -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let inner = [
        cell("PinX", "3"),
        cell("PinY", "3"),
        cell("Width", "2"),
        cell("Height", "1"),
        cell("LocPinX", "1"),
        cell("LocPinY", "0.5"),
        FIVE.map(|n| cell(n, "40")).concat(),
        [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "1"), ("QuickStyleLineColor", "2"), ("QuickStyleLineMatrix", "1")]
            .map(|(n, v)| cell(n, v))
            .concat(),
        format!(
            r#"<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
            cell("NoFill", "0"),
            row("MoveTo", 1, 0.0, 0.0),
            row("LineTo", 2, 2.0, 0.0),
            row("LineTo", 3, 2.0, 1.0),
            row("LineTo", 4, 0.0, 1.0),
            row("LineTo", 5, 0.0, 0.0)
        ),
    ]
    .concat();
    shape_with(id, r#" Type="Shape" LineStyle="3" FillStyle="3" TextStyle="3""#, &inner)
}

/// The drawing: `relationship` says whether the document part has the theme relationship, `saved` whether the style cells hold a value beside their
/// formula, and `theme` whether there is a theme part at all.
fn drawing(theme: bool, relationship: bool, saved: bool) -> Vec<u8> {
    let page_sheet = [cell("PageWidth", "6"), cell("PageHeight", "4"), cell("PageScale", "1"), cell("DrawingScale", "1")].concat();
    let mut builder =
        VsdxBuilder::new().styles(ROOT).more_styles(&theme_style(saved)).page(PageSpec::new(1, "File").sheet(&page_sheet).shapes(&themed_box(1)));
    if theme && relationship {
        builder = builder.theme(&theme_bytes());
    } else if theme {
        builder = builder.with(Pkg::new().part(&format!("{DIR}/theme/theme1.xml"), CT_THEME, theme_bytes()));
    }
    builder.build()
}

fn opened(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let path = dir.write(name, bytes);
    reopened(&path)
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// The page as SVG.
fn svg_of(s: &mut Session, dir: &Dir, name: &str) -> String {
    let path = dir.path(name);
    run(s, "file.export", json!({ "path": path }));
    String::from_utf8(dir.read(name)).unwrap()
}

/// Every `fill="#rrggbb"` and `stroke="#rrggbb"` of the SVG, in order.
fn paints(svg: &str, attribute: &str) -> Vec<String> {
    let key = format!("{attribute}=\"#");
    svg.match_indices(&key).filter_map(|(at, _)| svg.get(at + key.len() - 1..at + key.len() + 6).map(str::to_owned)).collect()
}

fn fills(s: &mut Session, dir: &Dir, name: &str) -> Vec<String> {
    paints(&svg_of(s, dir, name), "fill")
}

fn strokes(s: &mut Session, dir: &Dir, name: &str) -> Vec<String> {
    paints(&svg_of(s, dir, name), "stroke")
}

// ---- each condition alone, and both ----------------------------------------------------------------------------------------------------------------

#[test]
fn a_drawing_with_the_relationship_and_the_saved_values_is_drawn_themed() {
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, true, true));
    assert!(fills(&mut s, &dir, "a.svg").contains(&ACCENT1.to_owned()), "the baseline of the report: both patches in");
}

#[test]
fn a_theme_part_with_no_relationship_is_drawn_themed() {
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, false, true));
    assert!(fills(&mut s, &dir, "a.svg").contains(&ACCENT1.to_owned()), "only the relationship is missing");
    assert!(strokes(&mut s, &dir, "b.svg").contains(&ACCENT1.to_owned()), "the line follows the theme too");
}

#[test]
fn style_cells_with_a_formula_and_no_saved_value_are_drawn_themed() {
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, true, false));
    assert!(fills(&mut s, &dir, "a.svg").contains(&ACCENT1.to_owned()), "only the saved values are missing");
    assert!(strokes(&mut s, &dir, "b.svg").contains(&ACCENT1.to_owned()), "the line follows the theme too");
}

#[test]
fn both_conditions_together_are_drawn_themed() {
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, false, false));
    assert!(fills(&mut s, &dir, "a.svg").contains(&ACCENT1.to_owned()), "no relationship and no saved values");
    assert!(strokes(&mut s, &dir, "b.svg").contains(&ACCENT1.to_owned()), "the line follows the theme too");
}

#[test]
fn the_four_drawings_are_drawn_alike() {
    let dir = Dir::new();
    let mut all = Vec::new();
    for (relationship, saved) in [(true, true), (false, true), (true, false), (false, false)] {
        let mut s = opened(&dir, "in.vsdx", &drawing(true, relationship, saved));
        all.push((fills(&mut s, &dir, "a.svg"), strokes(&mut s, &dir, "b.svg")));
    }
    assert!(all.windows(2).all(|pair| pair[0] == pair[1]), "{all:?}");
}

#[test]
fn a_drawing_with_no_theme_part_keeps_the_root_style_s_colours() {
    // The control: with no theme to answer, a call that names no property gives the root style's cell, saved value or not.
    let dir = Dir::new();
    for saved in [true, false] {
        let mut s = opened(&dir, "in.vsdx", &drawing(false, false, saved));
        let fills = fills(&mut s, &dir, "a.svg");
        assert!(!fills.contains(&ACCENT1.to_owned()), "saved: {saved}: {fills:?}");
        assert!(fills.contains(&"#ffffff".to_owned()), "saved: {saved}: {fills:?}");
    }
}

// ---- a save ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_saved_file_reopens_themed_in_each_format() {
    for ext in ["vsdx", "diacraft"] {
        let dir = Dir::new();
        let mut s = opened(&dir, "in.vsdx", &drawing(true, false, false));
        let path = dir.path(&format!("out.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        assert!(fills(&mut back, &dir, "a.svg").contains(&ACCENT1.to_owned()), "{ext}");
        assert!(strokes(&mut back, &dir, "b.svg").contains(&ACCENT1.to_owned()), "{ext}");
    }
}

#[test]
fn a_file_saved_and_saved_again_through_the_native_format_keeps_its_theme_and_reads_without_a_word() {
    // The model writer makes the VSDX file from the model: the theme gets the relationship the original lacked, and the file reads themed.
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, false, false));
    let native = dir.path("mid.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut mid = reopened(&native);
    let out = dir.path("final.vsdx");
    run(&mut mid, "file.save_as", json!({ "path": out }));
    let mut back = reopened(&out);
    assert!(fills(&mut back, &dir, "a.svg").contains(&ACCENT1.to_owned()));
    let package = diacraft_opc::Package::read(&dir.read("final.vsdx"), &diacraft_opc::Limits::default()).unwrap();
    let rels = String::from_utf8(package.part(&format!("{DIR}/_rels/document.xml.rels")).unwrap().to_vec()).unwrap();
    assert!(rels.contains("relationships/theme"), "a relationship leads to the theme: {rels}");
}

/// The theme parts a VSDX package holds, by name.
fn theme_parts(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    package.parts().filter(|name| name.to_ascii_lowercase().contains("/theme/")).map(str::to_owned).collect()
}

#[test]
fn an_orphan_theme_keeps_its_part_name_through_a_plain_save_as_and_the_native_format() {
    // A theme part no relationship reaches keeps the name it had, both when a Save As writes the file from its own source and when the drawing went
    // through `.diacraft` and the model writer makes the VSDX file: the part is `theme1.xml` again, and the model the file reads back as is the model saved.
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, false, false));
    let before = s.document().unwrap().themes.clone();
    assert_eq!(&*before.get(0).unwrap().part, format!("{DIR}/theme/theme1.xml").as_str());
    let plain = dir.path("plain.vsdx");
    run(&mut s, "file.save_as", json!({ "path": plain }));
    assert_eq!(theme_parts(&dir.read("plain.vsdx")), [format!("{DIR}/theme/theme1.xml")], "a plain Save As keeps the part");
    let plain_back = reopened(&plain);
    assert_eq!(plain_back.document().unwrap().themes, before, "the plain Save As's model check agrees");

    let native = dir.path("mid.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut mid = reopened(&native);
    assert_eq!(mid.document().unwrap().themes, before, "the native format keeps the part's name");
    let out = dir.path("final.vsdx");
    run(&mut mid, "file.save_as", json!({ "path": out }));
    assert_eq!(theme_parts(&dir.read("final.vsdx")), [format!("{DIR}/theme/theme1.xml")], "the model writer reuses the part's name");
    let back = reopened(&out);
    assert_eq!(back.document().unwrap().themes, before, "the model check agrees");
}

#[test]
fn an_edit_after_opening_and_a_save_keep_the_theme() {
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, false, false));
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Edited"}));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = reopened(&path);
    assert!(fills(&mut back, &dir, "a.svg").contains(&ACCENT1.to_owned()));
}

// ---- the window's controls -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_formatting_controls_show_the_colours_the_picture_draws() {
    // `shape.formatting` reads cells as the renderer does, so a shape taking its fill and line from a style whose cells hold the call and no value shows the theme's
    // colour in the controls as it is drawn.
    let dir = Dir::new();
    for (relationship, saved) in [(true, true), (false, false), (true, false), (false, true)] {
        let mut s = opened(&dir, "in.vsdx", &drawing(true, relationship, saved));
        let look = run(&mut s, "shape.formatting", json!({"ids": [1], "page": 1}))["shapes"][0].clone();
        let text = look.to_string().to_lowercase();
        assert!(text.contains("5b9bd5"), "relationship {relationship}, saved {saved}: {look}");
    }
}

// ---- the cells a file saved no value for stay so ---------------------------------------------------------------------------------

/// The `Cell` elements of the style `Theme` of a saved VSDX file's document part that hold the call: each says whether a `V` stands in it.
fn theme_style_values(bytes: &[u8]) -> Vec<bool> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let xml = String::from_utf8(package.part(&format!("{DIR}/document.xml")).unwrap().to_vec()).unwrap();
    let start = xml.find("NameU=\"Theme\"").expect("the style Theme is in the document part");
    let style = &xml[start..xml[start..].find("</StyleSheet>").map_or(xml.len(), |end| start + end)];
    style.split("<Cell ").skip(1).filter(|cell| cell.contains("THEMEVAL")).map(|cell| cell.contains(" V=")).collect()
}

#[test]
fn a_save_writes_no_value_for_a_cell_the_file_had_none_for_after_the_drawing_was_drawn_and_formatted() {
    // The answers a frame works out are kept in memory, beside the model, and nowhere in it: the cells the file saved no `V` for hold none after the page was drawn
    // (twice, the second from the kept answers), after the formatting controls read it, after an edit of another shape's text and a save in each format.
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, true, false));
    let before = theme_style_values(&dir.read("in.vsdx"));
    assert_eq!(before.len(), THEMED.len());
    assert!(before.iter().all(|v| !v), "the file as built holds no value in them: {before:?}");
    fills(&mut s, &dir, "a.svg");
    fills(&mut s, &dir, "b.svg");
    run(&mut s, "shape.formatting", json!({"ids": [1], "page": 1}));
    for edited in [false, true] {
        if edited {
            run(&mut s, "shape.set_text", json!({"id": 1, "text": "Edited"}));
        }
        let name = format!("out{edited}.vsdx");
        run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
        let saved = theme_style_values(&dir.read(&name));
        assert_eq!(saved.len(), THEMED.len(), "edited {edited}: the cells are all there");
        assert!(saved.iter().all(|v| !v), "edited {edited}: a value was written for a cell the file had none for: {saved:?}");
    }
    // In the model of a file that is read again, from the native format too.
    let native = dir.path("out.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let back = reopened(&native);
    let doc = back.document().unwrap();
    let style = doc.styles.iter().find(|st| st.name_u.as_deref() == Some("Theme")).expect("the style Theme");
    for name in THEMED {
        let cell = style.sheet.cell(name).unwrap_or_else(|| panic!("the style holds {name}"));
        assert!(cell.value.is_none(), "{name} holds {:?} after a save in the native format", cell.value);
    }
}

#[test]
fn a_saved_odg_is_drawn_themed_too() {
    // The `.odg` writer flattens a shape the file does not hold to the values its cells resolve to, read as the picture reads them: the fill is the theme's.
    let dir = Dir::new();
    let mut s = opened(&dir, "in.vsdx", &drawing(true, true, false));
    let path = dir.path("out.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = reopened(&path);
    assert!(fills(&mut back, &dir, "a.svg").contains(&ACCENT1.to_owned()), "{:?}", fills(&mut back, &dir, "b.svg"));
}
