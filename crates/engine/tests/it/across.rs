//! A save across the VSDX and OpenDocument formats routes what the target cannot say, and says so (the native-format design's sections 5.1, 5.6 and 6).
//!
//! **Every file here is made by code**, from the standard: the VSDX files by the testkit's builder, the `.odg` files by `odg_build`, and every read-back goes through the real readers
//! (`diacraft_vsdx::read`, `diacraft_odg::read`, or a session's `file.open`).

use diacraft_doc::{Document, Format};
use diacraft_engine::across::{Carried, Target, words};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, rect};
use crate::run;

const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// A rectangle with its own cells, a geometry section and a text, in the style sheet `style`.
fn vsdx_rect(id: u32, x: f64, style: u32) -> String {
    format!(
        concat!(
            r#"<Shape ID="{id}" Type="Shape" LineStyle="{style}" FillStyle="{style}" TextStyle="{style}" UniqueID="{{ABC-{id}}}"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/>"#,
            r#"<Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section><Text>hello</Text></Shape>"#
        ),
        id = id,
        x = x,
        style = style
    )
}

/// The style sheets a VSDX file holds: a root with cells of its own, and two that follow it (`Box` follows `Normal` for its line, the root for its fill, and `Normal` for its text).
fn styles() -> String {
    concat!(
        r#"<StyleSheet ID="0" NameU="No Style" Name="No Style" IsCustomNameU="1"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/>"#,
        r##"<Cell N="LineWeight" V="0.01"/><Cell N="FillForegnd" V="#ffffff"/></StyleSheet>"##,
        r#"<StyleSheet ID="1" NameU="Normal" Name="Normal" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.02"/></StyleSheet>"#,
        r##"<StyleSheet ID="2" NameU="Box" Name="Box" LineStyle="1" FillStyle="0" TextStyle="1"><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##
    )
    .to_owned()
}

/// A VSDX drawing with style sheets, two rectangles, a part nothing models and a shape attribute nothing reads.
pub(crate) fn vsdx_with_styles() -> Vec<u8> {
    VsdxBuilder::new()
        .styles(&styles())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&format!(
            "{}{}",
            vsdx_rect(1, 2.0, 2),
            vsdx_rect(2, 5.0, 1)
        )))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build()
}

/// A `.vsdm` with a VBA project beside one rectangle.
pub(crate) fn vsdm_with_vba() -> Vec<u8> {
    VsdxBuilder::new()
        .document_type(diacraft_vsdx::Variant::from_extension("vsdm").unwrap().content_type())
        .styles(&styles())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&vsdx_rect(1, 2.0, 1)))
        .with(
            Pkg::new()
                .default_type("bin", "application/vnd.ms-office.vbaProject")
                .part_without_override("/visio/vbaProject.bin", b"not code, never run")
                .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin"),
        )
        .build()
}

fn open_file(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, bytes) }));
    s
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Value {
    s.execute("file.save_as", &json!({ "path": dir.path(name) })).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn carried(out: &Value) -> Carried {
    Carried::from_json(&out["carried"]).unwrap_or_else(|| panic!("a save result has a `carried` field: {out}"))
}

fn read_vsdx(bytes: &[u8]) -> (Document, diacraft_vsdx::ReadReport) {
    diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn read_odg(bytes: &[u8]) -> Document {
    diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{e}")).0
}

/// What identifies the style sheets of a drawing: the `ID`, the names and the three links, and the cells' values.
type Sheets = Vec<(u32, Option<String>, Option<String>, [Option<u32>; 3], Vec<(String, String)>)>;

fn style_sheets(doc: &Document) -> Sheets {
    doc.styles
        .iter()
        .map(|s| {
            let cells = s
                .sheet
                .cells
                .iter()
                .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()))
                .collect();
            (
                s.id,
                s.name.as_deref().map(str::to_owned),
                s.name_u.as_deref().map(str::to_owned),
                [s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text],
                cells,
            )
        })
        .collect()
}

/// The numbers the cells of the shapes of the first page hold, by shape, name and place: **values** (units and the form a formula is written in are the formats' to choose).
fn shape_values(doc: &Document) -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    for shape in doc.pages.iter().next().map(|p| p.shapes.iter()).into_iter().flatten() {
        for (name, cell) in shape.sheet.cells.iter() {
            if let Some(n) = cell.value.as_ref().and_then(|v| v.number()) {
                out.push((shape.id, name.as_str().to_owned(), format!("{n:.4}")));
            }
        }
        for section in shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry") {
            for row in section.rows.iter() {
                for (name, cell) in row.cells.iter() {
                    if let Some(n) = cell.value.as_ref().and_then(|v| v.number()) {
                        out.push((shape.id, format!("Geometry[{:?}].{}", row.ix, name.as_str()), format!("{n:.4}")));
                    }
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------- the two rows of the table

/// **The concern 1.** A drawing of a VSDX file with style sheets was refused as an `.odg` ("a change of a style sheet the file holds, or a style sheet taken from it"): the package the model
/// writer makes holds the root the reader makes of the file's defaults, and the splice path's rule for a style sheet of the file was applied to it. Its root is carried now, and so are style sheets whose
/// links an OpenDocument style cannot say.
#[test]
fn a_vsdx_drawing_with_style_sheets_is_saved_as_an_odg_and_reads_back_with_them() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_with_styles());
    let model = s.document().unwrap().clone();
    let out = save_as(&mut s, &dir, "b.odg");
    assert_eq!((out["variant"].as_str(), out["kind"].as_str()), (Some("odg"), Some("drawing")));
    let back = read_odg(&dir.read("b.odg"));
    // The style sheets come back whole: the root with its cells and the attribute its element held, and `Box`, whose three links differ.
    assert_eq!(style_sheets(&back), style_sheets(&model));
    assert_eq!(back.styles.get(0).unwrap().raw, model.styles.get(0).unwrap().raw, "what the root's element held rides");
    // The shapes: their text and what the format cannot say of them (the attribute nothing reads).
    for id in [1, 2] {
        let find = |d: &Document| d.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
        let (was, is) = (find(&model), find(&back));
        assert_eq!(
            format!("{:?}", was.text.as_ref().map(|t| &t.parts)),
            format!("{:?}", is.text.as_ref().map(|t| &t.parts)),
            "the text of shape {id}"
        );
        assert_eq!(was.raw, is.raw, "the attribute of shape {id} that no field reads");
    }
    // The file opens as a drawing of its own, and a save of it into itself is no refusal.
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("b.odg") }));
    run(&mut t, "file.save", json!({}));
}

/// A drawing of an `.odg` is saved as a VSDX type directly (the other row), and the file reads as a drawing with the same shapes.
#[test]
fn an_odg_drawing_is_saved_as_a_vsdx_type_directly() {
    let dir = Dir::new();
    let mut s = pinned();
    let odg = Drawing::new().page(&format!("{}{}", rect("R", (2.0, 2.0, 4.0, 2.0), "hello"), rect("S", (8.0, 2.0, 4.0, 2.0), ""))).bytes();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg) }));
    let was = s.document().unwrap().clone();
    for ext in ["vsdx", "vsdm", "vssx", "vstx"] {
        let name = format!("b.{ext}");
        let mut each = pinned();
        run(&mut each, "file.open", json!({ "path": dir.path("a.odg") }));
        let out = save_as(&mut each, &dir, &name);
        assert_eq!(out["variant"], ext);
        let (back, report) = read_vsdx(&dir.read(&name));
        let names = |d: &Document| d.pages.iter().flat_map(|p| p.shapes.iter().map(|x| (x.id, x.name_u.clone()))).collect::<Vec<_>>();
        assert_eq!(names(&back), names(&was), "{ext}");
        assert_eq!(
            (report.carry.stale, report.carry.gone, report.carry.left_out),
            (0, 0, 0),
            "{ext}: every item of the data an .odg held comes back: {:?}",
            report.carry
        );
    }
}

/// **A second Save As across formats in one session** (the concern 5): the first save made the VSDX file the drawing's own, whose baseline holds what the writer wrote where the
/// drawing still held the `.odg`'s own form, and the splice writer refused the second ("a change to the picture of a shape"). The drawing adopts the model the file reads as, and it is clean.
#[test]
fn a_second_save_as_across_formats_in_one_session_is_written_and_the_drawing_is_the_files_model() {
    let dir = Dir::new();
    let mut s = pinned();
    let odg = Drawing::new()
        .page(&format!("{}{}", object_frame(), rect("R", (2.0, 8.0, 4.0, 2.0), "")))
        .file("Object 1", &compound())
        .file("ObjectReplacements/Object 1", &png(10))
        .bytes();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg) }));
    save_as(&mut s, &dir, "b.vsdx");
    // The drawing is the model the file reads as (the one that was saved is no longer it), and it is clean.
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], false);
    let (back, report) = read_vsdx(&dir.read("b.vsdx"));
    assert_eq!(report.carry.stale, 0, "the page's own item is fresh (the concern 6): {:?}", report.carry);
    // (The model the writer reads the file as holds the parts as the model's bytes: it is the file's model, by shape and by form.)
    let form = |d: &Document| {
        d.pages
            .iter()
            .flat_map(|p| p.shapes.iter())
            .find(|x| x.name_u.as_deref() == Some("A"))
            .and_then(|x| x.foreign.as_ref().and_then(|f| f.attr("ForeignType").map(str::to_owned)))
    };
    assert_eq!(form(s.document().unwrap()), form(&back), "the drawing adopted the model the file reads as: the frame is an embedding");
    assert_eq!(form(&back).as_deref(), Some("Object"));
    // The second save, to another type, is written, and the object is an embedding in both.
    let second = save_as(&mut s, &dir, "c.vsdm");
    assert_eq!(second["variant"], "vsdm");
    let (again, _) = read_vsdx(&dir.read("c.vsdm"));
    let embeddings = |d: &Document| d.parts.iter().filter(|p| p.format == Format::Vsdx && p.role == diacraft_doc::PartRole::Embedding).count();
    assert_eq!((embeddings(&back), embeddings(&again)), (1, 1));
}

// ------------------------------------------------------------------------------------------------------------- the note

/// A frame of an object (a compound file) with a PNG for its replacement.
pub(crate) fn object_frame() -> String {
    concat!(
        r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="A" svg:width="4cm" svg:height="2cm" svg:x="2cm" svg:y="2cm">"#,
        r#"<draw:object-ole xlink:href="./Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#,
        r#"<draw:image xlink:href="./ObjectReplacements/Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#
    )
    .to_owned()
}

/// 1,536 bytes of a compound file (the five checks hold).
pub(crate) fn compound() -> Vec<u8> {
    let mut bytes = vec![0u8; 1536];
    bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
    bytes[1024 + 0x42] = 5;
    bytes
}

/// A small PNG.
pub(crate) fn png(shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[shade; 16]).unwrap();
    out
}

/// What a VSDX file carries of an `.odg`: the `carried` field names the classes in plain words, says that data is in the document-level data, and for a file with no such data says nothing.
#[test]
fn a_vsdx_file_written_from_an_odg_names_the_classes_it_carries() {
    let dir = Dir::new();
    let definitions = concat!(
        r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/>"##,
        r##"<draw:hatch draw:name="h1" draw:style="single" draw:color="#3465a4" draw:distance="0.1cm" draw:rotation="0"/>"##,
        r#"<draw:marker draw:name="m1" svg:viewBox="0 0 10 10" svg:d="M0 0h10v10z"/>"#
    );
    let odg = Drawing::new().common(definitions).page(&rect("R", (2.0, 2.0, 4.0, 2.0), "")).bytes();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg) }));
    let out = save_as(&mut s, &dir, "b.vsdx");
    let c = carried(&out);
    for class in ["line ends", "gradients", "hatches", "layer settings", "style names", "document properties"] {
        assert!(c.classes.iter().any(|x| x == class), "{class} is carried: {:?}", c.classes);
    }
    assert!(c.document_level && !c.macros && !c.objects && c.signatures_stale == 0, "{c:?}");
    assert!(
        out["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("has data a VSDX file has no place for")),
        "never a warning: {out}"
    );
    // The words of the note are the design's, with these classes.
    let said = words(Target::Vsdx, &c);
    assert_eq!(said.len(), 1);
    assert!(said[0].starts_with("This drawing has data a VSDX file has no place for (line ends, gradients, hatches, "), "{}", said[0]);
    // A VSDX drawing saved as VSDX again has nothing to say, and a new drawing saved as an `.odg` carries nothing of the other format's.
    let mut v = open_file(&dir, "c.vsdx", &vsdx_with_styles());
    assert!(carried(&save_as(&mut v, &dir, "d.vsdm")).is_empty());
    let mut n = pinned();
    run(&mut n, "file.new", json!({}));
    assert!(carried(&save_as(&mut n, &dir, "e.odg")).classes.is_empty());
}

/// §5.6, part by part: the words are the design's, exactly, and **with nothing to say there is no note**.
#[test]
fn the_note_words_are_exact() {
    let classes = |c: &[&str]| c.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    let carried = |c: &[&str], macros: bool, objects: bool, sigs: u32| Carried {
        classes: classes(c),
        document_level: c.iter().any(|x| *x != "shape data"),
        macros,
        objects,
        signatures_stale: sigs,
    };
    // Nothing to say, no note.
    assert!(words(Target::Vsdx, &Carried::none()).is_empty() && words(Target::Odg, &Carried::none()).is_empty());
    // Carried data, VSDX.
    assert_eq!(
        words(Target::Vsdx, &carried(&["line ends", "gradients"], false, false, 0)),
        [
            "This drawing has data a VSDX file has no place for (line ends, gradients). DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."
        ]
    );
    // Carried data, `.odg`: what LibreOffice keeps (shapes) and what it leaves out (the document classes).
    assert_eq!(
        words(Target::Odg, &carried(&["shape data", "page data", "window settings"], false, false, 0)),
        [
            "This drawing has data an OpenDocument drawing has no place for (shape data, page data, window settings). DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell."
        ]
    );
    // (The sentence is the same with no document class, what another program does is said as it was measured, and a LibreOffice save kept DiaCraft's own entries too.)
    assert_eq!(
        words(Target::Odg, &carried(&["shape data"], false, false, 0)),
        [
            "This drawing has data an OpenDocument drawing has no place for (shape data). DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell."
        ]
    );
    // A macro project, objects, or both, for each format's programs.
    for (target, programs) in [(Target::Vsdx, "VSDX"), (Target::Odg, "OpenDocument")] {
        for (macros, objects, what) in
            [(true, false, "a macro project"), (false, true, "embedded objects"), (true, true, "a macro project and embedded objects")]
        {
            let said = words(target, &carried(&[], macros, objects, 0));
            assert_eq!(
                said,
                [format!(
                    "This file carries {what} that {programs} programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
                )]
            );
        }
    }
    // The signatures.
    assert_eq!(words(Target::Vsdx, &carried(&[], false, false, 3)), ["3 digital signature(s) in this file no longer match it."]);
    // All three, in the design's order.
    let all = words(Target::Odg, &carried(&["macro project"], true, false, 1));
    assert_eq!(all.len(), 3);
    assert!(
        all[0].starts_with("This drawing has data an OpenDocument drawing")
            && all[1].starts_with("This file carries a macro project")
            && all[2].starts_with("1 digital signature(s)")
    );
    // The structured field round-trips.
    let c = carried(&["line ends", "settings"], true, true, 2);
    assert_eq!(Carried::from_json(&c.to_json()), Some(c));
    assert_eq!(Carried::from_json(&json!({ "classes": [1] })), None);
}

// ------------------------------------------------------------------------------------------------------------- macro projects and objects

/// An `.odg` with a macro project, in a folder of its own.
fn odg_with_macros() -> Vec<u8> {
    Drawing::new()
        .page(&rect("R", (2.0, 2.0, 4.0, 2.0), ""))
        .file("Basic/Standard/Module1.xml", b"<script/>")
        .file("Basic/script-lc.xml", b"<libraries/>")
        .bytes()
}

/// **Call 17**: a macro project of ODF goes into a VSDX file of a macro-enabled type only. A save to a macro-free type is refused in words that name the macro-enabled types, and writes nothing; with
/// `drop_macros` the project is left out; a macro-enabled type carries it, and the note says so.
#[test]
fn an_odf_macro_project_to_a_macro_free_vsdx_type_is_refused_without_drop_macros() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg_with_macros()) }));
    for ext in ["vsdx", "vssx", "vstx"] {
        let name = format!("b.{ext}");
        match s.execute("file.save_as", &json!({ "path": dir.path(&name) })) {
            Err(EngineError::MacrosNotAllowed { msg, .. }) => assert!(msg.contains(".vsdm") && msg.contains("macro project"), "{ext}: {msg}"),
            other => panic!("{ext}: {other:?}"),
        }
        assert!(!dir.files().contains(&name), "{ext}: nothing was written");
    }
    let left_out = run(&mut s, "file.save_as", json!({ "path": dir.path("c.vsdx"), "drop_macros": true }));
    assert!(left_out["macros_dropped"].as_u64().unwrap() > 0 && !carried(&left_out).macros, "{left_out}");
    let (back, _) = read_vsdx(&dir.read("c.vsdx"));
    assert!(back.parts.iter().all(|p| !p.name.starts_with("Basic/")), "the project is not in the file");
    let mut again = pinned();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    let kept = run(&mut again, "file.save_as", json!({ "path": dir.path("d.vsdm") }));
    let c = carried(&kept);
    assert!(c.macros && c.classes.iter().any(|x| x == "macro project"), "{c:?}");
    let (back, _) = read_vsdx(&dir.read("d.vsdm"));
    assert!(back.parts.iter().any(|p| &*p.name == "Basic/Standard/Module1.xml" && p.format == Format::Odf), "the project rides as bytes");
}

/// The same for VBA: a project of a `.vsdm` goes into another macro-free VSDX type only with `drop_macros`, and into an `.odg` it rides.
#[test]
fn a_vba_project_to_a_macro_free_vsdx_type_is_refused_without_drop_macros_and_rides_in_an_odg() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdm", &vsdm_with_vba());
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("a.diacraft") }));
    match t.execute("file.save_as", &json!({ "path": dir.path("b.vsdx") })) {
        Err(EngineError::MacrosNotAllowed { msg, .. }) => assert!(msg.contains(".vsdm"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(run(&mut t, "file.save_as", json!({ "path": dir.path("c.vsdx"), "drop_macros": true }))["macros_dropped"], 1);
    // A `.vsdm` drawing saved as an `.odg` directly: the project rides in the file as bytes, and the note says so.
    let mut v = open_file(&dir, "d.vsdm", &vsdm_with_vba());
    let out = save_as(&mut v, &dir, "d.odg");
    let c = carried(&out);
    assert!(c.macros && c.classes.iter().any(|x| x == "macro project"), "{c:?}");
    assert!(
        diacraft_odg::package::Package::read(&dir.read("d.odg"), &diacraft_odg::Limits::default())
            .unwrap()
            .entries()
            .any(|e| e.name.starts_with("DiaCraft/"))
    );
    let mut again = open_file(&dir, "e.vsdm", &vsdm_with_vba());
    let left_out = run(&mut again, "file.save_as", json!({ "path": dir.path("f.odg"), "drop_macros": true }));
    assert!(!carried(&left_out).macros && left_out["macros_dropped"].as_u64().unwrap() > 0, "{left_out}");
}

/// **`drop_macros` and `drop_objects` leave the `.diacraft` holding them**: the file written has neither, each shape that showed an object shows its picture, and a `.diacraft` saved afterwards from the
/// same drawing holds the macro project and the object, to the byte.
#[test]
fn drop_macros_and_drop_objects_leave_the_diacraft_holding_them() {
    let dir = Dir::new();
    let drawing = Drawing::new()
        .page(&format!("{}{}", object_frame(), rect("R", (2.0, 8.0, 4.0, 2.0), "")))
        .file("Object 1", &compound())
        .file("ObjectReplacements/Object 1", &png(10))
        .file("Basic/Standard/Module1.xml", b"<script/>");
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &drawing.bytes()) }));
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx"), "drop_macros": true, "drop_objects": true }));
    let c = carried(&out);
    assert!(!c.macros && !c.objects && c.signatures_stale == 0, "{out}");
    assert!(out["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("embedded object was left out")), "{out}");
    let (back, _) = read_vsdx(&dir.read("b.vsdx"));
    assert!(
        back.parts.iter().all(|p| p.role != diacraft_doc::PartRole::Embedding && !p.name.starts_with("Basic/") && !p.name.starts_with("Object ")),
        "neither is in the file"
    );
    let shape = back.pages.iter().flat_map(|p| p.shapes.iter()).find(|x| x.name_u.as_deref() == Some("A")).expect("the frame's shape");
    assert!(shape.foreign.as_ref().is_some_and(|f| f.picture.is_some() && f.attr("ForeignType") != Some("Object")), "the shape shows its picture");
    // The drawing, saved as `.diacraft` now, still holds the project and the object.
    run(&mut s, "file.save_as", json!({ "path": dir.path("c.diacraft") }));
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("c.diacraft") }));
    let kept: Vec<(String, usize)> =
        t.document().unwrap().parts.iter().filter(|p| p.format == Format::Odf).map(|p| (p.name.to_string(), p.bytes.len())).collect();
    assert!(kept.contains(&("Basic/Standard/Module1.xml".to_owned(), 9)) && kept.contains(&("Object 1".to_owned(), 1536)), "{kept:?}");
    // The VSDX direction: a drawing with no object has nothing to leave out of an `.odg`.
    let mut v = open_file(&dir, "d.vsdx", &vsdx_with_styles());
    let plain = run(&mut v, "file.save_as", json!({ "path": dir.path("e.odg"), "drop_objects": true }));
    assert!(
        plain["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("left out of the file")),
        "nothing to leave out: {plain}"
    );
}

// ------------------------------------------------------------------------------------------------------------- the trip

/// **§5.4's rule, by value** (`vsdx_to_odg_to_vsdx_is_the_same_model`): a VSDX drawing saved as an `.odg`, opened and saved as VSDX gives the same drawing but for what the format the trip went through
/// chooses. What must come back **equal**: the pages and the shapes (their `ID`s and names), every cell's number, the style sheets (names, links, cells), the data no field reads (an attribute of a
/// shape, a part of the package), and nothing may be lost. What the `.odg` says its own way, and the test lists by class: **the unit a cell shows in** (the `.odg` writes lengths in centimetres) and **the
/// form of a geometry formula** (`Width*1` is `Width`, a row that is 0 holds no formula), which the `.odg` reader gives every shape it reads, and the style links of the shapes (the engine's flattening, said
/// in the warnings of the save: see `no_flatten_loss_is_left_on_a_save_across`).
#[test]
fn vsdx_to_odg_to_vsdx_is_the_same_model() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_with_styles());
    let first = s.document().unwrap().clone();
    save_as(&mut s, &dir, "b.odg");
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("b.odg") }));
    save_as(&mut t, &dir, "c.vsdx");
    let (back, report) = read_vsdx(&dir.read("c.vsdx"));
    // Nothing is lost: every item of the data of the other format came back.
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    // Every number of the drawing comes back; the cells a shape took from its style sheet are its own in the file (the engine's flattening: named in the warnings of the save).
    let (before, after) = (shape_values(&first), shape_values(&back));
    assert!(before.iter().all(|v| after.contains(v)), "every cell's number comes back: {before:?} against {after:?}");
    let taken = [
        "LineWeight",
        "LineColor",
        "LinePattern",
        "FillPattern",
        "FillForegnd",
        "VerticalAlign",
        "TopMargin",
        "BottomMargin",
        "LeftMargin",
        "RightMargin",
    ];
    assert!(
        after.iter().filter(|v| !before.contains(v)).all(|(_, name, _)| taken.contains(&name.as_str())),
        "only values a style sheet gave: {after:?}"
    );
    assert_eq!(style_sheets(&back), style_sheets(&first), "the style sheets");
    let ids = |d: &Document| {
        d.pages.iter().flat_map(|p| p.shapes.iter().map(|x| (x.id, x.name_u.clone(), x.raw.get("UniqueID").map(str::to_owned)))).collect::<Vec<_>>()
    };
    assert_eq!(ids(&back), ids(&first), "the shapes, and the attribute no field reads");
    let part = |d: &Document| d.parts.iter().find(|p| &*p.name == "/foo/bar.dat").map(|p| p.bytes.to_vec());
    assert_eq!(part(&back), Some(b"0123456789".to_vec()), "the part nothing models, byte for byte");
    // What differs is the classes: units and the form of a geometry formula, the style links, and the file's own bookkeeping.
    let diff = diacraft_vsdx::compare_content(&dir.read("a.vsdx"), &dir.read("c.vsdx"), &diacraft_vsdx::Limits::default()).unwrap();
    for d in &diff.shown {
        let unit_or_formula = d.path.ends_with("/@U") || d.path.ends_with("/@F") || d.path.contains("/Section[0]/Row[");
        let own =
            d.part.starts_with("/visio/solutions/") || d.part == "/docProps/app.xml" || d.part.ends_with(".rels") || d.part == "[Content_Types].xml";
        let links = d.path.ends_with("@FillStyle") || d.path.ends_with("@LineStyle") || d.path.ends_with("@TextStyle");
        // A cell a shape took from its style sheet is its own in the file: one more `Cell` of the shape.
        let taken = d.path.rsplit_once("/Cell#").is_some_and(|(head, _)| head.contains("/Shape[") && !head.contains("/Section["))
            && format!("{:?}", d.found) == "Element";
        assert!(unit_or_formula || own || links || taken, "a difference of a class not listed: {} {} {:?}", d.part, d.path, d.found);
    }
}

/// What a VSDX shape that follows style sheets loses in an `.odg`, and the engine says so in words: **no flatten loss is silent on a save across**. The links to style sheets are named in the warnings of
/// the save, and what the model writer keeps that the splice path drops (`DrawingSizeType`) is in the file.
#[test]
fn no_flatten_loss_is_left_on_a_save_across() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_with_styles());
    let out = save_as(&mut s, &dir, "b.odg");
    let said: Vec<&str> = out["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert!(said.iter().any(|w| w.contains("style sheet") && w.contains("shape")), "the links to style sheets are named: {said:?}");
    // A drawing whose shapes follow no style sheet has nothing to say.
    let mut v = open_file(
        &dir,
        "c.vsdx",
        &VsdxBuilder::new()
            .page(PageSpec::new(1, "P").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&vsdx_rect(1, 2.0, 0)))
            .build(),
    );
    let plain = save_as(&mut v, &dir, "d.odg");
    assert!(plain["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("style sheet")), "{plain}");
    // The model writer keeps `DrawingSizeType` of a page, which a save into a file drops: it comes back.
    let mut n = pinned();
    run(&mut n, "file.new", json!({}));
    run(&mut n, "page.size", json!({ "width": 4.0, "height": 2.0 }));
    save_as(&mut n, &dir, "e.odg");
    let sized = n.document().unwrap().pages.get(0).unwrap().sheet.cell("DrawingSizeType").is_some();
    let mut m = pinned();
    run(&mut m, "file.open", json!({ "path": dir.path("e.odg") }));
    assert_eq!(m.document().unwrap().pages.get(0).unwrap().sheet.cell("DrawingSizeType").is_some(), sized);
}

// ------------------------------------------------------------------------------------------------------------- pictures

/// **The `Img` cells** (the concern 2): a picture an `.odg` held has no `ImgOffsetX`, `ImgOffsetY`, `ImgWidth` or `ImgHeight`, and LibreOffice draws a `Foreign` shape only with them. A save to VSDX
/// gives the shape the four cells, which the picture filling the shape needs.
#[test]
fn a_picture_an_odg_held_gets_the_cells_of_its_box_when_it_is_saved_as_vsdx() {
    let dir = Dir::new();
    let frame = r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="P" svg:width="4cm" svg:height="2cm" svg:x="2cm" svg:y="2cm"><draw:image xlink:href="Pictures/p.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#;
    let odg = Drawing::new().page(frame).file("Pictures/p.png", &png(7)).bytes();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg) }));
    let was = s.document().unwrap().pages.get(0).unwrap().shapes.iter().next().unwrap().clone();
    assert!(was.sheet.cell("ImgWidth").is_none(), "the reader gives the frame no box");
    save_as(&mut s, &dir, "b.vsdx");
    let (back, _) = read_vsdx(&dir.read("b.vsdx"));
    let shape = back.pages.iter().flat_map(|p| p.shapes.iter()).find(|x| x.name_u.as_deref() == Some("P")).unwrap();
    let number = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or_else(|| panic!("{name}"));
    assert_eq!((number("ImgOffsetX"), number("ImgOffsetY")), (0.0, 0.0));
    assert!(
        (number("ImgWidth") - number("Width")).abs() < 1e-9 && (number("ImgHeight") - number("Height")).abs() < 1e-9,
        "the picture fills the shape"
    );
}

// ------------------------------------------------------------------------------------------------------------- the files LibreOffice is shown

/// Writes the files that a LibreOffice run is given into the folder `ACROSS_LO_DIR` names (by hand: `ACROSS_LO_DIR=<folder> cargo nextest run -p diacraft-engine --run-ignored only -E 'test(lo_files)'`): the
/// saves across, by every writer, of a drawing with style sheets, an object, a picture and a macro project. Returns at once with no folder named.
#[test]
#[ignore = "writes files for a LibreOffice run, by hand"]
fn lo_files() {
    let Some(folder) = std::env::var_os("ACROSS_LO_DIR") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(folder.join(name), bytes).unwrap();
    let path = |name: &str| folder.join(name).to_str().unwrap().to_owned();
    // A VSDX drawing with style sheets, saved across as an `.odg` and an `.otg`.
    put("styles.vsdx", &vsdx_with_styles());
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": path("styles.vsdx") }));
    run(&mut s, "file.save_as", json!({ "path": path("styles-across.odg") }));
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": path("styles.vsdx") }));
    run(&mut s, "file.save_as", json!({ "path": path("styles-across.otg") }));
    // A `.vsdm` with a VBA project, saved across as an `.odg` (the project rides as bytes).
    put("vba.vsdm", &vsdm_with_vba());
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": path("vba.vsdm") }));
    run(&mut s, "file.save_as", json!({ "path": path("vba-across.odg") }));
    // An `.odg` with an object, a picture, definitions and a macro project, saved across to each VSDX type.
    let frame = r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="P" svg:width="4cm" svg:height="2cm" svg:x="8cm" svg:y="2cm"><draw:image xlink:href="Pictures/p.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#;
    let definitions = r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/>"##;
    let odg = Drawing::new()
        .common(definitions)
        .page(&format!("{}{}{}", object_frame(), frame, rect("R", (2.0, 8.0, 4.0, 2.0), "hello")))
        .file("Object 1", &compound())
        .file("ObjectReplacements/Object 1", &png(10))
        .file("Pictures/p.png", &png(7))
        .file("Basic/Standard/Module1.xml", b"<script/>")
        .bytes();
    put("objects.odg", &odg);
    for ext in ["vsdx", "vsdm", "vssx", "vstm"] {
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": path("objects.odg") }));
        let params = json!({ "path": path(&format!("objects-across.{ext}")), "drop_macros": !ext.ends_with('m') });
        run(&mut s, "file.save_as", params);
    }
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": path("objects.odg") }));
    run(&mut s, "file.save_as", json!({ "path": path("objects-dropped.vsdx"), "drop_macros": true, "drop_objects": true }));
}
