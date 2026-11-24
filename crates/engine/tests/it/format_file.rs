//! The format commands on a drawing that came from a file: a shape that selects a theme and holds the `Themed` marker in the cells a
//! person formats, and a label of two runs and a field. The package is built in code from the specifications' XML; no file from another application
//! is used. A value written under a theme beats the theme's for that cell, the theme part and everything else the commands do not reach is the
//! file's own after a save, and the saved drawing opens again with what was written.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{Document, Owner, RowKey, SectionKey, TextPart};
use diacraft_engine::Session;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, cell_f, cell_u, shape_with};
use diacraft_testkit::theme::ThemeXml;
use serde_json::json;

use crate::format::{close, count, fv, local, number, px, render};
use crate::{check_session, run};

/// The root style as a file holds it, as far as formatting goes: solid white fill, a thin black line, 12 point text in a face, centred.
const ROOT: &str = r##"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#ffffff"/><Cell N="FillBkgnd" V="#000000"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01041666666666667"/><Cell N="BeginArrow" V="0"/><Cell N="EndArrow" V="0"/><Cell N="Rounding" V="0"/><Cell N="FillGradientEnabled" V="0"/><Cell N="LineGradientEnabled" V="0"/><Cell N="VerticalAlign" V="1"/><Cell N="LeftMargin" V="0"/><Cell N="RightMargin" V="0"/><Cell N="TopMargin" V="0"/><Cell N="BottomMargin" V="0"/><Section N="Character"><Row IX="0"><Cell N="Font" V="Calibri"/><Cell N="Color" V="#000000"/><Cell N="Style" V="0"/><Cell N="Size" V="0.1666666666666667" U="PT"/></Row></Section><Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row></Section></StyleSheet>"##;

const LINKS: &str = r#" LineStyle="0" FillStyle="0" TextStyle="0""#;

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

fn page_sheet() -> String {
    [cell("PageWidth", "11"), cell("PageHeight", "8.5"), cell("PageScale", "1"), cell("DrawingScale", "1")].concat()
}

/// A 2 by 1 inch box with its lower left corner at (1, 1).
fn geometry() -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    format!(
        r#"{}{}{}{}{}{}<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
        cell("PinX", "1"),
        cell("PinY", "1"),
        cell("Width", "2"),
        cell("Height", "1"),
        cell("LocPinX", "0"),
        cell("LocPinY", "0"),
        cell("NoFill", "0"),
        row("MoveTo", 1, 0.0, 0.0),
        row("LineTo", 2, 2.0, 0.0),
        row("LineTo", 3, 2.0, 1.0),
        row("LineTo", 4, 0.0, 1.0),
        row("LineTo", 5, 0.0, 0.0)
    )
}

/// Shape 1: a box that selects theme 40 in all five scheme cells and holds the marker in its fill, line and text cells.
fn themed_box() -> String {
    let themed = |name: &str| cell_f(name, "Themed", "THEMEVAL()");
    let inner = [
        geometry(),
        FIVE.map(|n| cell(n, "40")).concat(),
        // Fill: accent1 (QuickStyleFillColor 2), fill entry 1; line: accent2 (QuickStyleLineColor 3), line entry 1.
        [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "1"), ("QuickStyleLineColor", "3"), ("QuickStyleLineMatrix", "1")]
            .map(|(n, v)| cell(n, v))
            .concat(),
        ["FillPattern", "FillForegnd", "LinePattern", "LineColor", "LineWeight"].map(themed).concat(),
        format!(
            r#"<Section N="Character"><Row IX="0">{}{}{}{}</Row></Section><Text><cp IX="0"/>Hello</Text>"#,
            themed("Font"),
            themed("Style"),
            themed("Color"),
            cell_u("Size", "0.1666666666666667", "PT")
        ),
    ]
    .concat();
    shape_with(1, LINKS, &inner)
}

fn themed_bytes() -> Vec<u8> {
    VsdxBuilder::new()
        .styles(ROOT)
        .theme(ThemeXml::default().xml().as_bytes())
        .page(PageSpec::new(1, "Themed").sheet(&(page_sheet() + &FIVE.map(|n| cell(n, "40")).concat())).shapes(&themed_box()))
        .build()
}

/// A label of two runs and a field, as the form gives: `Hello ` in row 0, `world` in row 1 and the field after it.
const LABEL: &str = r#"<Text><cp IX="0"/>Hello <cp IX="1"/>world<fld IX="0">3</fld></Text>"#;

fn label_bytes() -> Vec<u8> {
    let inner = [
        geometry(),
        r#"<Section N="Character"><Row IX="0"><Cell N="Font" V="Calibri"/><Cell N="Style" V="0"/><Cell N="Size" V="0.1666666666666667" U="PT"/></Row><Row IX="1"><Cell N="Font" V="Calibri"/><Cell N="Style" V="2"/><Cell N="Size" V="0.1111111111111111" U="PT"/></Row></Section>"#.to_owned(),
        r#"<Section N="Field"><Row IX="0"><Cell N="Value" V="3"/></Row></Section>"#.to_owned(),
        LABEL.to_owned(),
    ]
    .concat();
    VsdxBuilder::new()
        .styles(ROOT)
        // The attributes are invented, but for `Panose`, which is the value; the face
        // name is one the specs list as held by files. None is read from a file or a font.
        .face_names(r#"<FaceName NameU="Calibri" UnicodeRanges="1 0 0 0" CharSets="1 0" Panose="2 15 5 2 2 2 4 3 2 4" Flags="1"/>"#)
        .page(PageSpec::new(1, "Label").sheet(&page_sheet()).shapes(&shape_with(1, LINKS, &inner)))
        .build()
}

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-format-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A session that has opened `bytes`.
fn opened(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let path = dir.path(name);
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn row_cells(d: &Document, section: &str, ix: u32) -> BTreeMap<String, (Option<String>, String)> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let Some(section) = shape.sheet.section(&SectionKey::new(section, None)) else { return BTreeMap::new() };
    let Some(row) = section.row(&RowKey::Ix(ix)) else { return BTreeMap::new() };
    row.cells.iter().map(|(n, c)| (n.as_str().to_owned(), fv(c))).collect()
}

fn words(d: &Document) -> Vec<String> {
    let text = d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap().text.clone().unwrap();
    text.parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(s) => format!("{s:?}"),
            TextPart::Cp(i) => format!("cp{i}"),
            TextPart::Pp(i) => format!("pp{i}"),
            TextPart::Tp(i) => format!("tp{i}"),
            TextPart::Fld { ix, text } => format!("fld{ix}={text}"),
            TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => "raw".to_owned(),
        })
        .collect()
}

// ---- a themed shape ----------------------------------------------------------------------------------------------------------------

#[test]
fn a_local_colour_beats_the_theme_on_a_page_that_selects_one_and_the_theme_is_not_changed() {
    let dir = Dir::new();
    let mut s = opened(&dir, "themed.vsdx", &themed_bytes());
    // The theme answers: accent1 `#204060` for the fill and accent2 `#506070` for the line, and the picture says so.
    let before = render(&s);
    assert!(close(px(&before, 1.3, 1.2), (0x20, 0x40, 0x60), 2), "{:?}", px(&before, 1.3, 1.2));
    let themes = s.document().unwrap().themes.clone();

    let r = run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FF9900", "line": "#7F6000", "weight": 2.25}));
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    let d = s.document().unwrap();
    assert_eq!(
        fv(&local(d, 1, "FillForegnd").unwrap()),
        (Some("RGB(255,153,0)".to_owned()), "#ff9900".to_owned()),
        "the marker's formula is replaced"
    );
    assert_eq!(fv(&local(d, 1, "LineColor").unwrap()), (Some("RGB(127,96,0)".to_owned()), "#7f6000".to_owned()));
    assert_eq!(fv(&local(d, 1, "FillPattern").unwrap()), (None, "1".to_owned()), "the pattern was the marker: now solid, so that the colour shows");
    assert_eq!(fv(&local(d, 1, "LinePattern").unwrap()), (None, "1".to_owned()));
    assert!((number(&local(d, 1, "LineWeight").unwrap()) - 2.25 / 72.0).abs() < 1e-12);
    assert!(local(d, 1, "LineWeight").unwrap().formula.text().is_none());
    // What a person did not format is the theme's still, and so is the theme.
    assert_eq!(local(d, 1, "QuickStyleFillColor").map(|c| fv(&c).1).as_deref(), Some("2"));
    assert_eq!(d.themes, themes);
    let after = render(&s);
    assert!(close(px(&after, 1.3, 1.2), (255, 153, 0), 2), "the local colour wins over the theme's: {:?}", px(&after, 1.3, 1.2));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn text_formatting_beats_a_themed_run_and_a_theme_marker_in_style_reads_as_no_bits() {
    let dir = Dir::new();
    let mut s = opened(&dir, "themed.vsdx", &themed_bytes());
    run(
        &mut s,
        "text.format",
        json!({"ids": [1], "font": "Liberation Sans", "size": 11, "bold": true, "color": "#202020", "align": "right", "valign": "top"}),
    );
    let d = s.document().unwrap();
    let r = row_cells(d, "Character", 0);
    assert_eq!(r["Font"], (None, "Liberation Sans".to_owned()), "the marker's formula is replaced by the face");
    assert!((r["Size"].1.parse::<f64>().unwrap() - 11.0 / 72.0).abs() < 1e-12);
    assert_eq!(r["Size"].0, None);
    assert_eq!(r["Style"], (None, "1".to_owned()), "bold on a Style the theme gives: bit 1 set, and no formula");
    assert_eq!(r["Color"], (Some("RGB(32,32,32)".to_owned()), "#202020".to_owned()));
    assert_eq!(row_cells(d, "Paragraph", 0)["HorzAlign"], (None, "2".to_owned()));
    assert_eq!(local(d, 1, "VerticalAlign").map(|c| fv(&c).1).as_deref(), Some("0"));
    assert!(d.fonts.iter().any(|f| &*f.name_u == "Liberation Sans"), "added to the font table");
    // The label is inked in the new colour.
    let after = render(&s);
    assert!(count(&after, (1.0, 1.0, 3.0, 2.0), (0x20, 0x20, 0x20), 40) > 20, "the text is drawn in #202020");
}

#[test]
fn a_formatted_drawing_saves_with_only_the_page_changed_and_opens_again_with_what_was_written() {
    let dir = Dir::new();
    let original = themed_bytes();
    let mut s = opened(&dir, "themed.vsdx", &original);
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FF9900", "line": "none", "rounding": 0.25}));
    run(&mut s, "text.format", json!({"ids": [1], "size": 8, "italic": true}));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let saved = std::fs::read(&path).unwrap();

    // Only the page part differs: the theme, the styles' document part, the content types and the rest are the file's own, byte for byte.
    let (a, b) = (parts(&original), parts(&saved));
    let changed: Vec<&String> = b.keys().filter(|k| a.get(*k) != b.get(*k)).collect();
    assert!(changed.iter().all(|k| k.contains("/pages/") || k.starts_with("/docProps/")), "{changed:?}");
    assert!(changed.iter().any(|k| k.contains("/pages/page1")), "{changed:?}");
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "no part added or lost");
    let theme_parts: Vec<&String> = a.keys().filter(|k| k.contains("/theme/")).collect();
    assert!(!theme_parts.is_empty() && theme_parts.iter().all(|k| a[*k] == b[*k]), "the theme part is the file's own");

    // Read back: the same cells, and the picture is the same.
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    let (d, e) = (s.document().unwrap(), again.document().unwrap());
    for name in ["FillForegnd", "FillPattern", "LinePattern", "Rounding"] {
        assert_eq!(local(d, 1, name).map(|c| fv(&c)), local(e, 1, name).map(|c| fv(&c)), "{name}");
    }
    assert_eq!(row_cells(d, "Character", 0), row_cells(e, "Character", 0));
    assert_eq!(fv(&local(e, 1, "Rounding").unwrap()).1, "0.25");
    assert_eq!(px(&render(&s), 1.3, 1.2), px(&render(&again), 1.3, 1.2));
    assert_eq!(check_session(&again), Ok(()));
}

// ---- a label of two runs and a field --------------------------------------------------------------------------------------------

#[test]
fn a_range_of_a_label_splits_its_runs_and_keeps_the_field_through_a_save_and_a_read() {
    let dir = Dir::new();
    let mut s = opened(&dir, "label.vsdx", &label_bytes());
    // `lo wo`: the end of run 0 and the start of run 1.
    let r = run(&mut s, "text.format", json!({"ids": [1], "start": 3, "end": 8, "bold": true, "color": "#C00000"}));
    assert_eq!(r, json!({"page": 1, "formatted": 1}));
    let d = s.document().unwrap().clone();
    let d = &d;
    // The markers are numbered in text order, whichever run the split is in: run 1 is `lo `, run 2 `wo` (which was row 1, italic) and run 3
    // `rld` (a copy of that row).
    assert_eq!(words(d), ["cp0", "\"Hel\"", "cp1", "\"lo \"", "cp2", "\"wo\"", "cp3", "\"rld\"", "fld0=3"]);
    // Run 0 is as it was (12 points, not bold); run 1 is its copy with the change; run 2 was italic and is bold and italic now; run 3 is a copy of
    // it before the change, italic, 8 points.
    assert_eq!(row_cells(d, "Character", 0)["Style"].1, "0");
    assert_eq!(row_cells(d, "Character", 1)["Style"].1, "1");
    assert_eq!(row_cells(d, "Character", 1)["Size"], (None, "0.1666666666666667".to_owned()), "the copy has the size and the unit of its run");
    assert_eq!(row_cells(d, "Character", 2)["Style"].1, "3");
    assert_eq!(row_cells(d, "Character", 2)["Size"].1, "0.1111111111111111");
    assert_eq!(row_cells(d, "Character", 3)["Style"].1, "2");
    assert_eq!(row_cells(d, "Character", 3)["Size"].1, "0.1111111111111111");
    assert_eq!(row_cells(d, "Character", 1)["Color"], (Some("RGB(192,0,0)".to_owned()), "#c00000".to_owned()));
    assert_eq!(row_cells(d, "Character", 2)["Color"], (Some("RGB(192,0,0)".to_owned()), "#c00000".to_owned()));

    // It saves and reads back with the same text parts and rows, and the field.
    let path = dir.path("label-out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    let e = again.document().unwrap();
    assert_eq!(words(e), words(d));
    for ix in 0..4 {
        assert_eq!(row_cells(e, "Character", ix), row_cells(d, "Character", ix), "row {ix}");
    }
    assert_eq!(check_session(&again), Ok(()));
}

#[test]
fn a_face_added_to_the_font_table_is_in_the_saved_table() {
    let dir = Dir::new();
    let mut s = opened(&dir, "label.vsdx", &label_bytes());
    run(&mut s, "text.format", json!({"ids": [1], "font": "Liberation Serif"}));
    assert_eq!(s.document().unwrap().fonts.iter().map(|f| f.name_u.to_string()).collect::<Vec<_>>(), ["Calibri", "Liberation Serif"]);
    let path = dir.path("fonts.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    let names: Vec<String> = again.document().unwrap().fonts.iter().map(|f| f.name_u.to_string()).collect();
    assert_eq!(names, ["Calibri", "Liberation Serif"]);
    let calibri = &again.document().unwrap().fonts[0];
    assert!(calibri.raw.get("Panose").is_some(), "the face the file had keeps its attributes");
}

// ---- shape.formatting ---------------------------------------------------------------------------------------------------------------

#[test]
fn the_formatting_of_a_themed_shape_is_what_the_picture_draws_and_follows_a_format() {
    let dir = Dir::new();
    let mut s = opened(&dir, "themed.vsdx", &themed_bytes());
    let r = run(&mut s, "shape.formatting", json!({"ids": [1]}));
    let shape = &r["shapes"][0];
    assert_eq!(r["page"], 1);
    assert_eq!((shape["id"].as_u64(), shape["fill"].as_str(), shape["line"].as_str()), (Some(1), Some("#204060"), Some("#506070")), "{shape}");
    assert_eq!(shape["size"].as_f64(), Some(12.0));
    assert_eq!(shape["align"], "center");
    assert_eq!(shape["valign"], "middle");
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FF9900", "line": "#7F6000", "weight": 2.25, "end_arrow": 13, "rounding": 0.25}));
    run(
        &mut s,
        "text.format",
        json!({"ids": [1], "font": "Liberation Sans", "size": 11, "bold": true, "italic": true, "strike": true, "color": "#202020", "align": "left", "valign": "bottom"}),
    );
    let r = run(&mut s, "shape.formatting", json!({"ids": [1]}));
    assert_eq!(
        r["shapes"][0],
        json!({
            "id": 1, "fill": "#ff9900", "line": "#7f6000", "weight": 2.25, "pattern": 1, "begin_arrow": 0, "end_arrow": 13, "begin_arrow_size": 2, "end_arrow_size": 2, "rounding": 0.25,
            "font": "Liberation Sans", "size": 11.0, "bold": true, "italic": true, "underline": false, "strike": true, "color": "#202020",
            "align": "left", "valign": "bottom", "bullet": 0, "ind_left": 0.0, "case": "normal", "direction": "ltr", "vertical": false,
            // A solid fill and a line that are drawn in the colours written, with no transparency, no gradient and no shadow.
            "fill_pattern": 1, "fill_transparency": 0.0, "fill_background": null, "line_transparency": 0.0, "line_cap": "round", "compound": "single",
            "gradient": null, "line_gradient": null, "shadow": null
        })
    );
    // A query: nothing changed, nothing recorded.
    let (steps, journal) = (s.history().unwrap().undo_len(), s.journal().len());
    run(&mut s, "shape.formatting", json!({"ids": [1]}));
    assert_eq!((s.history().unwrap().undo_len(), s.journal().len()), (steps, journal));
}

#[test]
fn the_formatting_of_a_blank_page_shape_and_of_a_missing_one() {
    let mut s = crate::blank_session();
    let id = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let r = run(&mut s, "shape.formatting", json!({"ids": [id]}));
    let shape = &r["shapes"][0];
    assert_eq!((shape["fill"].as_str(), shape["line"].as_str()), (Some("#ffffff"), Some("#000000")));
    assert_eq!(shape["weight"].as_f64(), Some(0.72), "the blank page's line is 0.01 inch, 0.72 point");
    assert!((shape["size"].as_f64().unwrap() - 12.0).abs() < 1e-4, "the blank root holds 0.1666667 inch: {shape}");
    // The blank page's root names no face, so the shape is shown in the face the renderer draws it in, and not a blank (it was null).
    assert_eq!(shape["font"], json!(crate::face_query::shown_default()));
    assert!(shape["font"].is_string(), "{shape}");
    assert!(s.execute("shape.formatting", &json!({"ids": [99]})).is_err());
}

/// Writes formatted drawings to the folder in `DIACRAFT_FORMAT_DIR`, for opening them in another reader (`soffice --headless --convert-to pdf`).
/// Not a test unless asked for.
#[test]
#[ignore = "writes formatted drawings to the folder in DIACRAFT_FORMAT_DIR, for another reader to convert"]
fn write_formatted_drawings() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_FORMAT_DIR").expect("DIACRAFT_FORMAT_DIR names the folder to write to"));
    std::fs::create_dir_all(&dir).unwrap();
    let scratch = Dir::new();
    // The themed box and the label, formatted and saved.
    let mut themed = opened(&scratch, "themed.vsdx", &themed_bytes());
    run(&mut themed, "shape.format", json!({"ids": [1], "fill": "#FF9900", "line": "#7F6000", "weight": 2.25, "rounding": 0.25}));
    run(&mut themed, "text.format", json!({"ids": [1], "font": "Liberation Sans", "size": 11, "bold": true, "color": "#202020", "align": "left"}));
    run(&mut themed, "file.save_as", json!({"path": dir.join("themed-formatted.vsdx").to_str().unwrap()}));
    let mut label = opened(&scratch, "label.vsdx", &label_bytes());
    run(&mut label, "text.format", json!({"ids": [1], "start": 3, "end": 8, "bold": true, "color": "#C00000", "size": 14}));
    run(&mut label, "text.format", json!({"ids": [1], "align": "right", "valign": "bottom"}));
    run(&mut label, "file.save_as", json!({"path": dir.join("label-formatted.vsdx").to_str().unwrap()}));
    // A new drawing: shapes of both stencils and a connector, formatted every way.
    let mut s = crate::blank_session();
    let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 5.0);
    let b = crate::edit::drop_shape(&mut s, "decision", 6.0, 5.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Hello world\nsecond"}));
    run(&mut s, "shape.set_text", json!({"id": b, "text": "Yes?"}));
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#3C78D8", "line": "#1C4587", "weight": 3, "pattern": 2, "rounding": 0.2}));
    run(&mut s, "shape.format", json!({"ids": [b], "fill": "none", "line": "#CC0000", "weight": 1.5}));
    run(&mut s, "shape.format", json!({"ids": [c], "line": "#38761D", "weight": 2, "begin_arrow": 10, "end_arrow": 13}));
    run(&mut s, "text.format", json!({"ids": [a], "start": 0, "end": 5, "bold": true, "color": "#FFFFFF", "font": "Liberation Serif"}));
    run(&mut s, "text.format", json!({"ids": [a], "start": 12, "end": 18, "italic": true, "align": "left"}));
    run(&mut s, "text.format", json!({"ids": [b], "size": 14, "underline": true, "strike": true, "valign": "top"}));
    run(&mut s, "file.save_as", json!({"path": dir.join("new-formatted.vsdx").to_str().unwrap()}));
    // The three drawings of the review of round 0: a rectangle of the stencil with `Hello world` in its row 0, which the root style gives, with `lo`
    // made bold and red (`lo-a`); the same after the text was given a row 0 of its own by a format of the whole text (`lo-b`); and `lo-a` with `H` then made
    // blue and italic, a split in a run that has later runs (`lo-c`).
    for (name, local_row_0, then_h) in [("lo-a", false, false), ("lo-b", true, false), ("lo-c", false, true)] {
        let mut s = crate::blank_session();
        let big = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 4.0);
        run(&mut s, "shape.set_text", json!({"id": big, "text": "Hello world"}));
        if local_row_0 {
            run(&mut s, "text.format", json!({"ids": [big], "size": 12}));
        }
        run(&mut s, "text.format", json!({"ids": [big], "start": 3, "end": 5, "bold": true, "color": "#CC0000"}));
        if then_h {
            run(&mut s, "text.format", json!({"ids": [big], "start": 0, "end": 1, "italic": true, "color": "#0000CC"}));
        }
        run(&mut s, "file.save_as", json!({"path": dir.join(format!("{name}.vsdx")).to_str().unwrap()}));
    }
    eprintln!("wrote the drawings to {}", dir.display());
}

// ---- the size of an arrowhead survives a save in each format --------------------------------------------------

/// A connector given an arrowhead of each end and a size for each, saved as `.diacraft`, `.vsdx` and `.odg` and opened again: the sizes are the ones written (the cells are the
/// file's own, `BeginArrowSize` and `EndArrowSize`).
#[test]
fn the_size_of_an_arrowhead_is_kept_by_a_save_and_a_read_in_each_format() {
    let dir = Dir::new();
    let mut s = crate::blank_session();
    let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 5.0);
    let b = crate::edit::drop_shape(&mut s, "rectangle", 6.0, 5.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    run(&mut s, "shape.format", json!({"ids": [c], "begin_arrow": 4, "end_arrow": 13, "begin_arrow_size": 4, "end_arrow_size": 3}));
    for name in ["sizes.diacraft", "sizes.vsdx", "sizes.odg"] {
        let path = dir.path(name);
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut again = Session::new();
        let opened = run(&mut again, "file.open", json!({"path": path}));
        assert_eq!(opened["file"]["warning_count"], 0, "{name}: {opened}");
        let shapes = run(&mut again, "shape.formatting", json!({"ids": [c]}));
        let look = &shapes["shapes"][0];
        assert_eq!((look["begin_arrow"].as_u64(), look["end_arrow"].as_u64()), (Some(4), Some(13)), "{name}: {look}");
        assert_eq!(
            (look["begin_arrow_size"].as_u64(), look["end_arrow_size"].as_u64()),
            (Some(4), Some(3)),
            "{name}: the sizes are as written: {look}"
        );
        assert_eq!(check_session(&again), Ok(()), "{name}");
    }
}
