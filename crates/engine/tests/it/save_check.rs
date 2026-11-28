//! The check after a save that goes through a model writer (2026-11-22).
//!
//! After a save by the VSDX writer, the `.odg` writer or the `.diacraft` writer (from a drawing of another format), the engine reads the file back and compares it with the drawing, class by
//! class, and the save result's `checked` field says what differs. **Every file here is made by code** (the VSDX files by the testkit's builder, the `.odg` files by `odg_build`), and every
//! read-back goes through the real readers.
//!
//! * **Real losses**: what the writers lose today, through a real save: the formulas of a VSDX drawing saved as an `.odg`, and the line ends.
//! * **Planted losses**: a class the writers do not lose in a drawing built here is planted in the file read back (`check::with_tamper`, the hook a test uses), and the save must name it. Each
//!   class has a drawing in which it is there to be lost, and the drawing without the plant is clean.
//! * **Clean**: a trip that loses nothing names nothing, and the same content in the forms the formats hold is named once, as one group.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, DefFamily, Document, PartRole, RawAttrs, Shape, ShapeList, SharedVec, TextPart, Value};
use diacraft_engine::Session;
use diacraft_engine::check::{self, Class, Outcome, words};
use diacraft_testkit::package::{MasterSpec, OD, PageSpec, Pkg, VsdxBuilder};
use serde_json::{Value as Json, json};

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, rect_on};
use crate::run;

// ------------------------------------------------------------------------------------------------------------------------------- the drawings

/// The shapes of a drawing a person made: a rectangle that holds something of every class a cell can hold (an outline with formulas, a text with a field, hyperlinks, user cells, shape data, a connection
/// point, a gradient, a line end), a group of one shape, a picture, and an instance of a master.
const SHAPES: &str = concat!(
    r##"<Shape ID="1" Type="Shape" LineStyle="1" FillStyle="1" TextStyle="1"><Cell N="PinX" V="2" U="IN"/><Cell N="PinY" V="2" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"##,
    r##"<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="FillForegnd" V="#ff0000"/><Cell N="LineColor" V="#00ff00"/><Cell N="BeginArrow" V="3"/><Cell N="FillGradientEnabled" V="1"/><Cell N="Mystery" V="7"/>"##,
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#,
    r#"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.2"/></Row></Section>"#,
    r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row></Section>"#,
    r#"<Section N="Hyperlink"><Row N="Link1"><Cell N="Address" V="http://example.org/"/></Row></Section>"#,
    r#"<Section N="User"><Row N="Owner"><Cell N="Value" V="Ann"/></Row></Section>"#,
    r#"<Section N="Property"><Row N="Cost"><Cell N="Label" V="Cost"/><Cell N="Value" V="12"/></Row></Section>"#,
    r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="2"/><Cell N="Y" V="0.5"/></Row></Section>"#,
    r#"<Section N="Field"><Row IX="0"><Cell N="Value" V="12"/></Row></Section>"#,
    r##"<Section N="FillGradient"><Row IX="0"><Cell N="GradientStopColor" V="#ffffff"/></Row></Section>"##,
    r#"<Data1>d1</Data1><Text>Hello <fld IX="0">12</fld> world</Text></Shape>"#,
    r#"<Shape ID="2" Type="Group"><Cell N="PinX" V="6" U="IN"/><Cell N="PinY" V="2" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
    r#"<Shapes><Shape ID="3" Type="Shape"><Cell N="PinX" V="1" U="IN"/><Cell N="PinY" V="0.5" U="IN"/><Cell N="Width" V="1" U="IN"/><Cell N="Height" V="1" U="IN"/><Text>inner</Text></Shape></Shapes></Shape>"#,
    r#"<Shape ID="4" Type="Foreign"><Cell N="PinX" V="9" U="IN"/><Cell N="PinY" V="2" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#,
    r#"<Shape ID="6" Type="Shape" Master="1"><Cell N="PinX" V="4" U="IN"/><Cell N="PinY" V="5" U="IN"/></Shape>"#
);

/// A PNG of two by two pixels, for the picture.
fn png(shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[shade; 16]).unwrap();
    out
}

/// A VSDX file: two pages, two style sheets, a master, two font faces, the document's properties, a picture, a part nothing reads, and the shapes above.
fn vsdx_drawing() -> Vec<u8> {
    let styles = concat!(
        r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="LineWeight" V="0.01"/></StyleSheet>"#,
        r##"<StyleSheet ID="1" NameU="Box" Name="Box" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="EnableFillProps" V="1"/><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##
    );
    let core = concat!(
        r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:title>A title</dc:title><dc:creator>Ann</dc:creator></cp:coreProperties>"#
    );
    let layer = r#"<Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/><Cell N="Visible" V="1"/></Row></Section>"#;
    VsdxBuilder::new()
        .styles(styles)
        // The attributes a VSDX file's faces hold: each face says more than its name, so its loss is a missing font though no cell names it.
        .face_names(
            r#"<FaceName NameU="Arial" UnicodeRanges="1 2 3 4" CharSets="1 0"/><FaceName NameU="Noto Sans" UnicodeRanges="5 6 7 8" CharSets="1 0"/>"#,
        )
        .core(core)
        .master(
            MasterSpec::new(1, "Rect").shapes(r#"<Shape ID="5" Type="Shape"><Cell N="Width" V="1" U="IN"/><Cell N="Height" V="1" U="IN"/></Shape>"#),
        )
        .page(
            PageSpec::new(1, "Page-1")
                .sheet(&format!(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>{layer}"#))
                .shapes(SHAPES)
                .connects(r#"<Connect FromSheet="6" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/image1.png"),
        )
        .page(PageSpec::new(2, "Page-2").sheet(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>"#))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png(1)).part("/customXml/item1.xml", "application/xml", b"<x/>"))
        .build()
}

/// A session that opened the drawing and saved it as `.diacraft`: from here, a save as a VSDX type or an `.odg` is written from the model alone.
fn from_diacraft(dir: &Dir) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    s
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Json {
    run(s, "file.save_as", json!({ "path": dir.path(name) }))
}

/// What a save said was checked: the outcome, or a panic for a save that was not.
fn outcome(saved: &Json) -> Outcome {
    Outcome::from_json(&saved["checked"]).unwrap_or_else(|| panic!("this save has a checked field: {saved}"))
}

/// The classes and their counts a save named, and whether it said content is written as values.
fn named(saved: &Json) -> (Vec<(Class, u64)>, bool) {
    match outcome(saved) {
        Outcome::Checked(checked) => (checked.classes.iter().map(|d| (d.class, d.count)).collect(), checked.as_values),
        Outcome::Unchecked(why) => panic!("the check did not finish: {why}"),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- real losses

/// A VSDX drawing whose formulas an `.odg` cannot give back: the places of the members of a group, which the reader works out from the group's frame (the formulas of a shape's own
/// style and frame cells, `RGB()`, `Sheet.N!` and the outline's `Width*0`, come back since then; these two are written as their values, and the save says so).
fn vsdx_with_formulas() -> Vec<u8> {
    let member = |id: u32, x: f64, k: f64| {
        format!(
            r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}" F="Sheet.1!Width*{k}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/></Shape>"#
        )
    };
    let shapes = format!(
        r#"<Shape ID="1" Type="Group" NameU="Pair"><Cell N="PinX" V="4"/><Cell N="PinY" V="6"/><Cell N="Width" V="4"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Shapes>{}{}</Shapes></Shape>"#,
        member(2, 1.0, 0.25),
        member(3, 3.0, 0.75)
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shapes)).build()
}

/// **And EVID2's class "formulas lost".** A VSDX drawing saved as an `.odg` loses the formulas an `.odg` cannot give back (the places of the members of a group), and the save names how many.
#[test]
fn a_vsdx_drawing_saved_as_an_odg_names_the_formulas_the_odg_lost() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_with_formulas()) }));
    let saved = save_as(&mut s, &dir, "b.odg");
    // The `Sheet.1!Width*0.25` and `Sheet.1!Width*0.75` of the members' places: two formulas that are values in the file.
    let (classes, _) = named(&saved);
    assert!(classes.contains(&(Class::Formulas, 2)), "{saved}");
    let said = words(&outcome(&saved));
    assert!(said.iter().any(|w| w == "formulas: 2 formulas are lost"), "{said:?}");
    // The drawing in the session still holds them: the save changed nothing in it.
    let group = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    let held = group.children.get(0).unwrap().sheet.cells.get("PinX").unwrap().formula.text().map(str::to_owned);
    assert_eq!(held.as_deref(), Some("Sheet.1!Width*0.25"));
}

/// The arrows of a line are line ends, and an `.odg` the VSDX drawing is saved as holds none of them.
#[test]
fn the_line_ends_of_a_vsdx_drawing_are_lost_in_an_odg_and_the_save_says_so() {
    let shapes = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="BeginArrow" V="3"/><Cell N="EndArrow" V="4"/></Shape>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(shapes))
        .build();
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
    let saved = save_as(&mut s, &dir, "b.odg");
    let (classes, _) = named(&saved);
    assert_eq!(classes, vec![(Class::LineEnds, 1)], "{saved}");
    assert_eq!(saved["checked"]["classes"][0], json!({ "class": "line-ends", "words": "1 line end differs", "count": 1 }));
}

/// **A cell the `Theme` style gives through `THEMEVAL()` has no value of its own to differ from** (issue 10 of the demo video): a drawing the app made from the Basic Network Diagram template and gave a theme,
/// saved as an `.odg`, was told "6 line ends differ" and "13 items differ". The drawing's connectors and devices follow the `Theme` style, whose cells (a line end's size, a connector's rounding) hold
/// `THEMEVAL()` and the placeholder `Themed`, and the `.odg` writes the value the cell draws with, as a number: the same content, written as values, and no loss.
#[test]
fn a_drawing_the_app_made_from_the_network_template_saved_as_an_odg_is_told_no_line_ends_and_no_style_content_differ() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": "basic_network" }));
    // The demo's drawing had a theme (the shapes follow the `Theme` style, whose cells are `THEMEVAL()`).
    run(&mut s, "theme.apply", json!({ "theme": "fernwood" }));
    for (master, x) in [("network.router", 2.0), ("network.switch", 5.0), ("computers.desktop", 8.0)] {
        run(&mut s, "shape.drop", json!({ "master": master, "page": 1, "x": x, "y": 5.0 }));
    }
    run(&mut s, "connector.add", json!({ "from": { "id": 1 }, "to": { "id": 2 } }));
    run(&mut s, "connector.add", json!({ "from": { "id": 2 }, "to": { "id": 3 } }));
    let saved = save_as(&mut s, &dir, "network.odg");
    let (classes, as_values) = named(&saved);
    assert!(!classes.iter().any(|(class, _)| matches!(class, Class::LineEnds | Class::StyleContent)), "{saved}");
    assert!(as_values, "the values a style gave are written on the shapes, and the card says so once: {saved}");
}

// ------------------------------------------------------------------------------------------------------------------------------- clean

/// **A clean trip names nothing**: a drawing made here, a VSDX drawing through `.diacraft`, and an `.odg` through `.diacraft` and back lose nothing, and the saves say so by saying nothing.
#[test]
fn a_trip_that_loses_nothing_names_nothing() {
    let dir = Dir::new();
    let mut s = from_diacraft(&dir);
    let saved = save_as(&mut s, &dir, "b.vsdx");
    assert!(outcome(&saved).is_clean(), "{saved}");
    assert!(words(&outcome(&saved)).is_empty(), "nothing to say, so no card");
    // An `.odg` that came from another program, through `.diacraft` and out again.
    let odg = Drawing::new().page(&rect_on("gr1", "A", (2.0, 2.0, 4.0, 2.0), "hello")).bytes();
    let mut o = pinned();
    run(&mut o, "file.open", json!({ "path": dir.write("o.odg", &odg) }));
    let to_native = save_as(&mut o, &dir, "o.diacraft");
    assert!(outcome(&to_native).is_clean(), "{to_native}");
    let back = save_as(&mut o, &dir, "p.odg");
    assert!(outcome(&back).is_clean(), "{back}");
    let to_vsdx = save_as(&mut o, &dir, "p.vsdx");
    assert!(outcome(&to_vsdx).is_clean(), "{to_vsdx}");
}

/// **The allowances name only the group line**: the same content in the forms an `.odg` holds (a length in centimetres for one in inches, `Width` for `Width*1`, a colour in capitals, a shape that
/// follows a style sheet holding the values it gives) is no difference, and the card says it once.
#[test]
fn the_forms_an_odg_holds_are_named_once_as_values_and_never_counted() {
    let styles = concat!(
        r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/></StyleSheet>"#,
        r##"<StyleSheet ID="1" NameU="Box" Name="Box" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="EnableFillProps" V="1"/><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##
    );
    let shapes = concat!(
        r#"<Shape ID="1" Type="Shape" LineStyle="1" FillStyle="1" TextStyle="1"><Cell N="PinX" V="2" U="IN"/><Cell N="PinY" V="2" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
        r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0"/></Row></Section></Shape>"#
    );
    let bytes = VsdxBuilder::new()
        .styles(styles)
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Cell N="PageHeight" V="11" U="IN"/>"#).shapes(shapes))
        .build();
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
    let saved = save_as(&mut s, &dir, "b.odg");
    assert_eq!(named(&saved), (vec![], true), "{saved}");
    let said = words(&outcome(&saved));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Written as values the file format holds: units"), "{}", said[0]);
}

/// **A splice save is not checked in 0.2**: the answer is `null`, not an empty outcome, for a save into the file the drawing stands on, and for a `.diacraft` written from a `.diacraft` drawing or a new one.
#[test]
fn a_save_that_is_not_through_a_model_writer_is_not_checked() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert_eq!(save_as(&mut s, &dir, "b.vsdx")["checked"], Json::Null, "a VSDX file into a VSDX file");
    assert_eq!(run(&mut s, "file.save", json!({}))["checked"], Json::Null, "a save in place");
    let mut n = pinned();
    run(&mut n, "file.new", json!({}));
    assert_eq!(save_as(&mut n, &dir, "n.diacraft")["checked"], Json::Null, "a new drawing as .diacraft");
    assert_eq!(save_as(&mut n, &dir, "n2.diacraft")["checked"], Json::Null, "a .diacraft drawing as .diacraft");
    let odg = Drawing::new().page(&rect_on("gr1", "A", (2.0, 2.0, 4.0, 2.0), "")).bytes();
    let mut o = pinned();
    run(&mut o, "file.open", json!({ "path": dir.write("o.odg", &odg) }));
    assert_eq!(save_as(&mut o, &dir, "p.odg")["checked"], Json::Null, "an .odg into an .odg");
    // And a save that is through a model writer has the field, whether or not it found anything.
    let across = save_as(&mut s, &dir, "c.diacraft");
    assert_eq!(outcome(&across), Outcome::Checked(Default::default()), "a VSDX drawing as .diacraft: it holds the whole model");
}

// ------------------------------------------------------------------------------------------------------------------------------- planted losses

/// A loss planted in the file read back, as a function of the drawing.
type Plant = fn(&mut Document);

/// What a table of planted losses holds for each: what is planted, how, and the classes the save must name with their counts.
type Planted = (&'static str, Plant, Vec<(Class, u64)>);

/// Every shape of every page, to change.
fn each_shape(doc: &mut Document, f: &mut dyn FnMut(&mut Shape)) {
    fn walk(list: &mut ShapeList, f: &mut dyn FnMut(&mut Shape)) {
        for at in 0..list.len() {
            if let Some(slot) = list.get_mut(at) {
                let shape = Arc::make_mut(slot);
                f(shape);
                walk(&mut shape.children, f);
            }
        }
    }
    for at in 0..doc.pages.len() {
        if let Some(slot) = doc.pages.get_mut(at) {
            walk(&mut Arc::make_mut(slot).shapes, f);
        }
    }
}

fn drop_section(shape: &mut Shape, name: &str) {
    Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != name);
}

fn set_cell(shape: &mut Shape, name: &str, value: &str) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    if sheet.cells.contains_key(name) {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
}

fn drop_parts_named(doc: &mut Document, name: &str) {
    let kept: Vec<_> = doc.parts.iter().filter(|p| !p.name.contains(name)).cloned().collect();
    doc.parts = SharedVec::from_vec(kept);
}

fn drop_parts(doc: &mut Document, role: PartRole) {
    let kept: Vec<_> = doc.parts.iter().filter(|p| p.role != role).cloned().collect();
    doc.parts = SharedVec::from_vec(kept);
}

/// Saves the drawing as `name` with `plant` changing the file read back, and answers what the save named.
fn planted(plant: fn(&mut Document), name: &str) -> (Vec<(Class, u64)>, Json) {
    let dir = Dir::new();
    let mut s = from_diacraft(&dir);
    let saved = check::with_tamper(plant, || save_as(&mut s, &dir, name));
    let (classes, _) = named(&saved);
    (classes, saved)
}

/// **Each class a VSDX drawing holds is named when the VSDX writer loses it**: the drawing is saved from the model by the VSDX writer, a loss is planted in the file read back, and the save names that
/// class, with the count, and no other. The same drawing saved with nothing planted is clean.
#[test]
fn a_vsdx_save_names_each_class_planted_in_what_it_read_back() {
    // The drawing without a plant: nothing to say.
    let dir = Dir::new();
    let mut s = from_diacraft(&dir);
    let plain = save_as(&mut s, &dir, "clean.vsdx");
    assert!(outcome(&plain).is_clean(), "{plain}");
    let table: Vec<Planted> = vec![
        ("pages", |doc| drop(doc.pages.remove(1)), vec![(Class::Pages, 1)]),
        (
            "shapes",
            |doc| {
                let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                let at = page.shapes.iter().position(|s| s.id == 4).unwrap();
                drop(page.shapes.remove(at));
            },
            vec![(Class::Shapes, 1)],
        ),
        (
            "shape numbers",
            |doc| {
                each_shape(doc, &mut |s| {
                    if s.id == 3 {
                        s.id = 33
                    }
                })
            },
            vec![(Class::ShapeNumbers, 1)],
        ),
        ("geometry", |doc| each_shape(doc, &mut |s| set_cell(s, "Width", "3")), vec![(Class::Geometry, 4)]),
        (
            "text",
            |doc| {
                each_shape(doc, &mut |s| {
                    if s.id == 3 {
                        s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![TextPart::Chars(Arc::from("changed"))], ..Default::default() }));
                    }
                });
            },
            vec![(Class::Text, 1)],
        ),
        ("text formatting", |doc| each_shape(doc, &mut |s| drop_section(s, "Character")), vec![(Class::TextFormatting, 1)]),
        (
            "pictures",
            |doc| {
                each_shape(doc, &mut |s| {
                    if let Some(f) = s.foreign.as_mut() {
                        Arc::make_mut(f).picture = None
                    }
                })
            },
            vec![(Class::Pictures, 1)],
        ),
        ("fonts", |doc| Arc::make_mut(&mut doc.fonts).clear(), vec![(Class::Fonts, 2)]),
        ("line ends", |doc| each_shape(doc, &mut |s| set_cell(s, "BeginArrow", "0")), vec![(Class::LineEnds, 1)]),
        ("gradients", |doc| each_shape(doc, &mut |s| drop_section(s, "FillGradient")), vec![(Class::Gradients, 1)]),
        ("style content", |doc| each_shape(doc, &mut |s| set_cell(s, "FillForegnd", "#0000ff")), vec![(Class::StyleContent, 1)]),
        ("style sheets", |doc| drop(doc.styles.remove(1)), vec![(Class::StyleSheets, 1)]),
        ("masters", |doc| doc.masters = SharedVec::new(), vec![(Class::Shapes, 1), (Class::Masters, 1)]),
        (
            "layers",
            |doc| {
                let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                Arc::make_mut(&mut page.sheet).sections.retain(|s| s.name.as_str() != "Layer");
            },
            vec![(Class::Layers, 1)],
        ),
        ("connections", |doc| Arc::make_mut(doc.pages.get_mut(0).unwrap()).connects = Default::default(), vec![(Class::Connections, 1)]),
        (
            "formulas",
            |doc| {
                each_shape(doc, &mut |s| {
                    if s.id == 1 {
                        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("1")));
                    }
                })
            },
            vec![(Class::Formulas, 1)],
        ),
        ("hyperlinks", |doc| each_shape(doc, &mut |s| drop_section(s, "Hyperlink")), vec![(Class::Hyperlinks, 1)]),
        ("fields", |doc| each_shape(doc, &mut |s| drop_section(s, "Field")), vec![(Class::Fields, 1)]),
        ("user cells", |doc| each_shape(doc, &mut |s| drop_section(s, "User")), vec![(Class::UserCells, 1)]),
        ("other attributes", |doc| each_shape(doc, &mut |s| set_cell(s, "Mystery", "8")), vec![(Class::ShapeAttributes, 1)]),
        ("properties", |doc| doc.title = Arc::from("Another title"), vec![(Class::Properties, 1)]),
        ("kept data", |doc| drop_parts_named(doc, "customXml"), vec![(Class::KeptData, 1)]),
    ];
    for (what, plant, expected) in table {
        let (classes, saved) = planted(plant, "b.vsdx");
        assert_eq!(classes, expected, "{what}: {saved}");
        // The words the card says: a line for the class, with the count.
        let said = words(&outcome(&saved));
        let (class, count) = expected[expected.len() - 1];
        assert!(said.contains(&format!("{}: {}", class.label(), class.words(count))), "{what}: {said:?}");
    }
}

/// The macro project of a macro-enabled drawing is named when it does not come back, and is no loss when the caller chose to leave it out.
#[test]
fn a_macro_project_that_is_lost_is_named_and_one_left_out_by_choice_is_not() {
    let bytes = VsdxBuilder::new()
        .document_type(diacraft_vsdx::Variant::from_extension("vsdm").unwrap().content_type())
        .page(
            PageSpec::new(1, "Page-1")
                .sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#)
                .shapes(r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/></Shape>"#),
        )
        .with(
            Pkg::new()
                .default_type("bin", "application/vnd.ms-office.vbaProject")
                .part_without_override("/visio/vbaProject.bin", b"not code, never run")
                .rel("/visio/document.xml", "rId50", "http://schemas.microsoft.com/office/2006/relationships/vbaProject", "vbaProject.bin"),
        )
        .build();
    let dir = Dir::new();
    // A session that holds the drawing as a `.diacraft` file: each save from it is written from the model (a save as a VSDX type makes the VSDX file the drawing's own, and the next is a splice).
    let session = |dir: &Dir| {
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.vsdm", &bytes) }));
        run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        s
    };
    let kept = save_as(&mut session(&dir), &dir, "b.vsdm");
    assert!(outcome(&kept).is_clean(), "the project comes back: {kept}");
    let lost = check::with_tamper(|doc| drop_parts(doc, PartRole::MacroProject), || save_as(&mut session(&dir), &dir, "c.vsdm"));
    assert_eq!(named(&lost).0, vec![(Class::Macros, 1)], "{lost}");
    let left_out = check::with_tamper(
        |doc| drop_parts(doc, PartRole::MacroProject),
        || run(&mut session(&dir), "file.save_as", json!({ "path": dir.path("d.vsdx"), "drop_macros": true })),
    );
    assert!(outcome(&left_out).is_clean(), "the caller chose to leave it out: {left_out}");
}

/// The object frames of an `.odg` are named when the `.odg` writer does not write them as objects, and its definitions (a line end, a gradient, a hatch) and its layers when they do not come back.
#[test]
fn an_odg_save_names_each_class_planted_in_what_it_read_back() {
    let compound = {
        let mut bytes = vec![0u8; 1536];
        bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        bytes[1024 + 0x42] = 5;
        bytes
    };
    let links = "xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"";
    let object = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"O\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"8cm\"><draw:object-ole xlink:href=\"./Object 1\" {links}/><draw:image xlink:href=\"./ObjectReplacements/Object 1\" {links}/></draw:frame>"
    );
    let picture = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"P\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"8cm\" svg:y=\"8cm\"><draw:image xlink:href=\"Pictures/p1.png\" {links}/></draw:frame>"
    );
    let definitions = concat!(
        r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/>"##,
        r##"<draw:hatch draw:name="h1" draw:style="single" draw:color="#3465a4" draw:distance="0.1cm" draw:rotation="0"/>"##,
        r#"<draw:marker draw:name="m1" svg:viewBox="0 0 10 10" svg:d="M0 0h10v10z"/>"#
    );
    let mut d = Drawing::new().common(definitions);
    d.auto.push_str(r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="g1" draw:stroke="solid" draw:marker-start="m1" draw:marker-start-width="0.3cm"/></style:style>"##);
    let mut d = d.page(&format!("{}{object}{picture}", rect_on("gr3", "A", (2.0, 2.0, 4.0, 2.0), "hello")));
    d.files.push(("Object 1".to_owned(), compound));
    d.files.push(("ObjectReplacements/Object 1".to_owned(), png(10)));
    d.files.push(("Pictures/p1.png".to_owned(), png(20)));
    let bytes = d.bytes();
    let dir = Dir::new();
    let session = |dir: &Dir| {
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &bytes) }));
        run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        s
    };
    // Nothing planted: the `.odg` goes through `.diacraft` and out as it was.
    let mut s = session(&dir);
    let plain = save_as(&mut s, &dir, "clean.odg");
    assert!(outcome(&plain).is_clean(), "{plain}");
    let table: Vec<Planted> = vec![
        (
            "objects",
            |doc| {
                each_shape(doc, &mut |s| {
                    if !s.ext.is_empty() {
                        s.ext
                            .make_mut()
                            .odf
                            .kept
                            .kept
                            .nodes_mut()
                            .retain(|p| !matches!(&p.node, diacraft_doc::RawNode::Element(e) if &*e.name == "draw:object-ole"));
                    }
                })
            },
            vec![(Class::Objects, 1)],
        ),
        ("object entries", |doc| drop_parts_named(doc, "Object 1"), vec![(Class::Objects, 1)]),
        (
            "pictures",
            |doc| {
                each_shape(doc, &mut |s| {
                    if let Some(f) = s.foreign.as_mut()
                        && f.attr("ForeignType") != Some("Object")
                    {
                        Arc::make_mut(f).picture = None
                    }
                })
            },
            // (the frame of a picture, and the replacement picture an object's frame shows)
            vec![(Class::Pictures, 2)],
        ),
        ("line ends", |doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Marker), vec![(Class::LineEnds, 1)]),
        ("gradients", |doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Gradient), vec![(Class::Gradients, 1)]),
        ("style content", |doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Hatch), vec![(Class::StyleContent, 1)]),
        ("layers", |doc| doc.ext.make_mut().odf.layers.truncate(2), vec![(Class::Layers, 3)]),
        (
            "master pages",
            |doc| Arc::make_mut(doc.pages.get_mut(0).unwrap()).ext.make_mut().odf.master_page = Some(Arc::from("Another")),
            vec![(Class::Masters, 1)],
        ),
    ];
    for (what, plant, expected) in table {
        let mut s = session(&dir);
        let saved = check::with_tamper(plant, || save_as(&mut s, &dir, "b.odg"));
        assert_eq!(named(&saved).0, expected, "{what}: {saved}");
    }
}

/// A `.diacraft` file written from a drawing of another format is checked too: the file is read back as an Open reads it, and what differs is named.
#[test]
fn a_diacraft_save_of_a_drawing_of_another_format_names_what_is_planted_in_what_it_read_back() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    let saved = check::with_tamper(
        |doc| {
            each_shape(doc, &mut |s| {
                if s.id == 3 {
                    s.text = None
                }
            })
        },
        || save_as(&mut s, &dir, "b.diacraft"),
    );
    assert_eq!(named(&saved).0, vec![(Class::Text, 1)], "{saved}");
    let again = save_as(&mut s, &dir, "c.diacraft");
    assert_eq!(again["checked"], Json::Null, "a `.diacraft` drawing saved as `.diacraft` is copied, and is not checked");
}

/// **The drawing keeps what it holds when the check found a difference**: a save across to VSDX makes the drawing the model the file reads as only when that holds everything the drawing did.
#[test]
fn a_drawing_is_not_replaced_by_what_a_file_with_a_difference_reads_as() {
    let links = "xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"";
    let frame = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"O\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"2cm\"><draw:object-ole xlink:href=\"./Object 1\" {links}/><draw:image xlink:href=\"./ObjectReplacements/Object 1\" {links}/></draw:frame>"
    );
    let mut compound = vec![0u8; 1536];
    compound[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    compound[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    compound[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    compound[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    compound[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
    compound[1024 + 0x42] = 5;
    let mut d = Drawing::new().page(&frame);
    d.files.push(("Object 1".to_owned(), compound));
    d.files.push(("ObjectReplacements/Object 1".to_owned(), png(10)));
    let bytes = d.bytes();
    let form = |doc: &Document| {
        doc.pages.get(0).unwrap().shapes.iter().next().and_then(|s| s.foreign.as_ref().and_then(|f| f.attr("ForeignType").map(str::to_owned)))
    };
    let dir = Dir::new();
    // With nothing planted the drawing adopts the model of the file, and the frame is an embedding.
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &bytes) }));
    let was = form(s.document().unwrap());
    assert!(outcome(&save_as(&mut s, &dir, "b.vsdx")).is_clean());
    assert_eq!(form(s.document().unwrap()).as_deref(), Some("Object"), "adopted (the frame was {was:?})");
    // With a loss the drawing stays what it was.
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.write("a.odg", &bytes) }));
    // (The file holds the object as the embedding the writer made from the object's file, and what it reads back as says so: the loss planted is of that embedding.)
    let saved = check::with_tamper(|doc| drop_parts(doc, PartRole::Embedding), || save_as(&mut t, &dir, "c.vsdx"));
    let classes: Vec<Class> = named(&saved).0.iter().map(|(class, _)| *class).collect();
    assert_eq!(classes, vec![Class::Objects], "{saved}");
    assert_eq!(form(t.document().unwrap()), was, "the drawing is as it was");
}

// ------------------------------------------------------------------------------------------------------------------------------- the card's words

/// What a person reads is the engine's words for the field: the heading, a line for each class, and the values group.
#[test]
fn the_words_for_a_save_are_the_classes_with_their_counts_and_the_values_group() {
    let (classes, saved) = planted(|doc| each_shape(doc, &mut |s| drop_section(s, "Hyperlink")), "b.vsdx");
    assert_eq!(classes, vec![(Class::Hyperlinks, 1)]);
    let said = words(&outcome(&saved));
    assert_eq!(
        said,
        vec![
            "DiaCraft read the file back and compared it with the drawing, and the file differs in 1 way. The drawing in DiaCraft is as it was."
                .to_owned(),
            "hyperlinks: 1 hyperlink differs".to_owned(),
        ]
    );
    let _ = RawAttrs::new();
}
