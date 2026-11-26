//! **Random chains of saves in one session**, from every kind of drawing a person starts from, through every type a Save As writes, with and without the options that leave
//! things out, and the edits a person makes between saves.
//!
//! **The rule every chain keeps.** In one session, after any Save As:
//!
//! * an unedited Save is never refused, is not an edit, and writes the same bytes;
//! * an edit is written, and the save after it is no edit;
//! * each object, picture and macro project is held once in the file;
//! * what a Save As option left out stays out of that file, and the drawing keeps it.
//!
//! **The drawings a chain starts from**, each built in code: a new drawing, one made from an `.otg` and from a `.vstx` template, and `.vsdx`, `.vsdm`, `.odg`, `.fodg` and `.diacraft` files.
//! The files a person brings are written by hand here (a VSDX package by the testkit's builder, an OpenDocument drawing by `odg_build`): plain shapes, a group, a glued connector, text with a
//! link, a picture, an embedded object and a macro project (VBA in a `.vsdm`, Basic in an `.odg`). The files DiaCraft made are what a session saved from those, or from a new drawing. Before
//! the first step, the session may add a picture from a file, a group, a connector and text with a link and a field.
//!
//! **The steps**: a Save As to each type, with or without `drop_objects` and `drop_macros`; a Save; an edit (a move, a text, a picture inserted, a delete) and then a Save. After each step the file
//! written is read back and **every payload this test made is counted in it** (the object's compound file, each picture, each file of a macro project): each is held once, at most.
//!
//! [`chains_keep_the_rule`] runs in nextest; [`a_long_sweep_of_chains`] is the local sweep (`#[ignore]`), which runs many more and groups what fails by its words.

use std::collections::BTreeMap;

use diacraft_doc::{Document, Shape, TextPart};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, TEMPLATE, glued, rect};

// ---------------------------------------------------------------------------------------------------------------- payloads

/// The bytes of the VBA project of a person's `.vsdm`.
const VBA: &[u8] = b"A VBA project, never run";
/// The two files of the Basic project of a person's `.odg`.
const BASIC_MODULE: &[u8] = b"<script:module script:name=\"Sweep\">REM, never run</script:module>";
const BASIC_LIBRARIES: &[u8] = b"<library:libraries>libraries</library:libraries>";

/// What a payload is to the rule: what a Save As may be asked to leave out (an object, a macro project), or a picture (which an edit may add).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Object,
    Macro,
    Picture,
}

/// The shade of the PNG of the picture a person's file holds.
const PICTURE_SHADE: u8 = 33;
/// The shade of the picture an `.odg`'s object is shown as.
const ODG_REPLACEMENT_SHADE: u8 = 10;
/// The shade of the picture a VSDX file's object is shown as.
const VSDX_REPLACEMENT_SHADE: u8 = 44;
/// The shade of the picture the session inserts before the first step.
const LIVE_PICTURE_SHADE: u8 = 77;
/// The shade of the first picture an edit inserts; each edit adds one.
const EDIT_PICTURE_SHADE: u8 = 120;

/// Every payload this test makes, by name: the bytes and what they are.
fn payloads() -> Vec<(String, Vec<u8>, Kind)> {
    let mut all = vec![
        ("the object".to_owned(), crate::across::compound(), Kind::Object),
        ("the VBA project".to_owned(), VBA.to_vec(), Kind::Macro),
        ("the Basic module".to_owned(), BASIC_MODULE.to_vec(), Kind::Macro),
        ("the Basic libraries".to_owned(), BASIC_LIBRARIES.to_vec(), Kind::Macro),
        ("the file's picture".to_owned(), crate::across::png(PICTURE_SHADE), Kind::Picture),
        ("the .odg object's picture".to_owned(), crate::across::png(ODG_REPLACEMENT_SHADE), Kind::Picture),
        ("the VSDX object's picture".to_owned(), crate::across::png(VSDX_REPLACEMENT_SHADE), Kind::Picture),
        ("the picture inserted first".to_owned(), crate::across::png(LIVE_PICTURE_SHADE), Kind::Picture),
    ];
    for k in 0..8u8 {
        all.push((format!("the picture of edit {k}"), crate::across::png(EDIT_PICTURE_SHADE + k), Kind::Picture));
    }
    all
}

/// How many entries of a package hold each payload, by name (a payload held by none is not listed). `None` for a file that is no package (a flat `.fodg`, one XML document).
fn counts(bytes: &[u8], payloads: &[(String, Vec<u8>, Kind)]) -> Option<BTreeMap<String, usize>> {
    use diacraft_container::zip::Archive;
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).ok()?;
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for at in 0..archive.entries().len() {
        if archive.entries().get(at).is_some_and(|e| e.folder) {
            continue;
        }
        let Ok(data) = archive.inflate(at, &mut diacraft_container::Work::default()) else { continue };
        for (name, payload, _) in payloads {
            if data == *payload {
                *found.entry(name.clone()).or_insert(0) += 1;
            }
        }
    }
    Some(found)
}

/// The payloads of the given kinds that the drawing holds as parts.
fn held_by(doc: &Document, payloads: &[(String, Vec<u8>, Kind)], kinds: &[Kind]) -> Vec<String> {
    payloads
        .iter()
        .filter(|(_, bytes, kind)| kinds.contains(kind) && doc.parts.iter().any(|part| &*part.bytes == bytes.as_slice()))
        .map(|(name, _, _)| name.clone())
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------- the files a person brings

/// A rectangle of a VSDX file: 2 by 1 inches, its pin at (`x`, `y`), with `inner` after its cells.
fn vsdx_box(id: u32, x: f64, y: f64, inner: &str) -> String {
    shape(
        id,
        &format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>{GEOMETRY}{inner}"#
        ),
    )
}

/// The outline of a box of 2 by 1 inches.
const GEOMETRY: &str = concat!(
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row>"#,
    r#"<Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row>"#,
    r#"<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
);

/// The page of a person's VSDX file: two boxes with text (the first with a hyperlink), a group of two, a connector glued from the first box to the second, a picture and an embedded object.
fn vsdx_page() -> PageSpec {
    let link = r#"<Section N="Hyperlink"><Row N="Row_1"><Cell N="Description" V="site" U="STR"/><Cell N="Address" V="https://example.com/" U="STR"/></Row></Section><Text>Box one</Text>"#;
    let first = vsdx_box(1, 2.0, 2.0, link);
    let second = vsdx_box(2, 6.0, 2.0, "<Text>Box two</Text>");
    let group = format!(
        r#"<Shape ID="3" Type="Group"><Cell N="PinX" V="4"/><Cell N="PinY" V="8"/><Cell N="Width" V="4"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="0.5"/><Shapes>{}{}</Shapes></Shape>"#,
        vsdx_box(4, 1.0, 0.5, ""),
        vsdx_box(5, 3.0, 0.5, "")
    );
    let connector = shape(
        6,
        r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="BeginX" V="3"/><Cell N="BeginY" V="2"/><Cell N="EndX" V="5"/><Cell N="EndY" V="2"/><Cell N="ObjType" V="2"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let picture = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="2"/><Cell N="PinY" V="5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/><Cell N="ImgOffsetX" V="0"/><Cell N="ImgOffsetY" V="0"/><Cell N="ImgWidth" V="2"/><Cell N="ImgHeight" V="1"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let object = r#"<Shape ID="8" Type="Foreign"><Cell N="PinX" V="6"/><Cell N="PinY" V="5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/><Cell N="ImgOffsetX" V="0"/><Cell N="ImgOffsetY" V="0"/><Cell N="ImgWidth" V="2"/><Cell N="ImgHeight" V="1"/><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    PageSpec::new(1, "Page-1")
        .sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#)
        .shapes(&format!("{first}{second}{group}{connector}{picture}{object}"))
        .connects(r#"<Connect FromSheet="6" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/><Connect FromSheet="6" FromCell="EndX" FromPart="12" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
        .rel("rId1", &format!("{OD}image"), "../media/image1.png")
        .rel("rId2", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin")
}

/// A person's VSDX file of the type `extension`: the page of [`vsdx_page`], and a VBA project when the type holds one.
fn vsdx_person(extension: &str) -> Vec<u8> {
    let variant = diacraft_vsdx::Variant::from_extension(extension).unwrap();
    let mut parts = Pkg::new()
        .default_type("png", "image/png")
        .part_without_override("/visio/media/image1.png", crate::across::png(PICTURE_SHADE))
        .part("/visio/embeddings/oleObject1.bin", "application/vnd.openxmlformats-officedocument.oleObject", crate::across::compound())
        .part_without_override("/visio/media/image2.png", crate::across::png(VSDX_REPLACEMENT_SHADE))
        .rel("/visio/embeddings/oleObject1.bin", "rId1", &format!("{OD}image"), "../media/image2.png");
    if variant.macro_enabled {
        parts = parts.default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", VBA).rel(
            "/visio/document.xml",
            "rId50",
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
        );
    }
    VsdxBuilder::new().document_type(variant.content_type()).page(vsdx_page()).with(parts).with(custom_properties()).build()
}

/// A rectangle of an `.odg` that a connector may name: `draw:id` and `xml:id` are `id`.
fn odg_box(id: &str, frame: (f64, f64, f64, f64), text: &str) -> String {
    rect(id, frame, text).replacen("<draw:custom-shape ", &format!("<draw:custom-shape draw:id=\"{id}\" xml:id=\"{id}\" "), 1)
}

/// The page of a person's `.odg`: two boxes (the first with a link in its text), a connector glued from one to the other, a group of two, and, unless `flat`, a picture and an object frame.
fn odg_page(flat: bool) -> String {
    let link = r#"see <text:a xlink:type="simple" xlink:href="https://example.com/">site</text:a>"#;
    let mut page = format!(
        "{}{}{}<draw:g draw:name=\"G\">{}{}</draw:g>",
        odg_box("b1", (2.0, 2.0, 4.0, 2.0), link),
        odg_box("b2", (10.0, 2.0, 4.0, 2.0), "Box two"),
        glued("C", "b1", "b2", (6.0, 3.0, 10.0, 3.0)),
        rect("G1", (2.0, 12.0, 3.0, 2.0), ""),
        rect("G2", (7.0, 12.0, 3.0, 2.0), ""),
    );
    if !flat {
        page.push_str(concat!(
            r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="P" svg:width="4cm" svg:height="2cm" svg:x="2cm" svg:y="18cm">"#,
            r#"<draw:image xlink:href="Pictures/sweep.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#
        ));
        page.push_str(&crate::across::object_frame());
    }
    page
}

/// The user-defined fields of a person's `.odg` (`meta:user-defined`, ODF 1.4 Part 3 §4.3.2.1): a string and a number. Names and values are made up (the vector, rebuilt from its note).
const USER_FIELDS: &str = concat!(
    r#"<meta:user-defined meta:name="Ledger code">LC0042</meta:user-defined>"#,
    r#"<meta:user-defined meta:name="Review count" meta:value-type="float">3</meta:user-defined>"#
);

/// The custom properties of a person's VSDX file (`docProps/custom.xml`, ECMA-376 Part 1 §22.3): a string and a number, made up.
fn custom_properties() -> Pkg {
    Pkg::new()
        .part(
            "/docProps/custom.xml",
            "application/vnd.openxmlformats-officedocument.custom-properties+xml",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" "#,
                r#"xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Ledger code">"#,
                r#"<vt:lpwstr>LC0042</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Review count"><vt:i4>3</vt:i4></property></Properties>"#
            ),
        )
        .rel("/", "rId9", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties", "docProps/custom.xml")
}

/// A person's `.odg` drawing, or `.otg` template when `media` is a template's.
fn odg_person(media: &str) -> Vec<u8> {
    Drawing::new()
        .meta(USER_FIELDS)
        .page(&odg_page(false))
        .file("Pictures/sweep.png", &crate::across::png(PICTURE_SHADE))
        .file("Object 1", &crate::across::compound())
        .file("ObjectReplacements/Object 1", &crate::across::png(ODG_REPLACEMENT_SHADE))
        .file("Basic/Standard/Module1.xml", BASIC_MODULE)
        .file("Basic/script-lc.xml", BASIC_LIBRARIES)
        .bytes_of_type(media)
}

/// A person's flat `.fodg`: the shapes, with no picture, object or macro project.
fn fodg_person() -> Vec<u8> {
    Drawing::new().meta(USER_FIELDS).page(&odg_page(true)).flat_bytes()
}

/// The vector, rebuilt in code from its note: an `.otg` template of one rectangle whose `meta.xml` holds two user-defined fields.
fn otg_fields() -> Vec<u8> {
    Drawing::new().meta(USER_FIELDS).page(&rect("R", (2.0, 2.0, 4.0, 2.0), "")).bytes_of_type(TEMPLATE)
}

// ---------------------------------------------------------------------------------------------------------------- what the session adds

/// What the session adds to the drawing before the first step, each a command a person runs.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Live {
    pub(crate) picture: bool,
    pub(crate) group: bool,
    pub(crate) connector: bool,
    pub(crate) link: bool,
}

impl Live {
    pub(crate) fn any(self) -> bool {
        self.picture || self.group || self.connector || self.link
    }
}

/// The id of the drawing's first page.
pub(crate) fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

/// A rectangle dropped at (`x`, `y`) on the first page: its id.
fn dropped(s: &mut Session, x: f64, y: f64) -> Result<u32, String> {
    let page = first_page(s);
    let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": page, "x": x, "y": y })).map_err(|e| format!("shape.drop: {e}"))?;
    made["id"].as_u64().and_then(|id| u32::try_from(id).ok()).ok_or_else(|| format!("shape.drop gave no id: {made}"))
}

/// Runs what `live` asks on the drawing. `picture` is the path of the picture file.
pub(crate) fn add_live(s: &mut Session, live: Live, picture: &str) -> Result<(), String> {
    let page = first_page(s);
    let ex = |s: &mut Session, id: &str, params: Value| s.execute(id, &params).map(|_| ()).map_err(|e| format!("{id}: {e}"));
    if live.picture {
        ex(s, "insert.picture", json!({ "path": picture, "page": page, "x": 4.0, "y": 9.5 }))?;
    }
    if live.group {
        let (a, b) = (dropped(s, 1.5, 10.0)?, dropped(s, 3.0, 10.0)?);
        ex(s, "shape.group", json!({ "ids": [a, b], "page": page }))?;
    }
    if live.connector {
        let (a, b) = (dropped(s, 1.5, 6.5)?, dropped(s, 6.5, 6.5)?);
        ex(s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b }, "page": page }))?;
    }
    if live.link {
        let id = dropped(s, 6.5, 9.5)?;
        ex(s, "shape.set_text", json!({ "id": id, "text": "Linked ", "page": page }))?;
        ex(s, "hyperlink.set", json!({ "id": id, "page": page, "links": [{ "description": "site", "address": "https://example.com/" }] }))?;
        ex(s, "text.insert_field", json!({ "id": id, "page": page, "category": "page", "field": "page_number" }))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------- the starts

/// A drawing a chain starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Start {
    /// A new drawing.
    New,
    /// A drawing made from a person's `.otg` template.
    Otg,
    /// A drawing made from a person's `.vstx` template.
    Vstx,
    /// A drawing made from a `.vstx` DiaCraft wrote from a person's `.odg`.
    VstxMade,
    /// A person's `.vsdx`.
    Vsdx,
    /// A person's `.vsdm`, with a VBA project.
    Vsdm,
    /// A `.vsdm` DiaCraft wrote from a person's `.odg` (its object an embedding, its Basic project riding).
    VsdmMade,
    /// A `.vsdx` DiaCraft wrote from a new drawing that a picture, a group, a connector and a link were added to.
    VsdxNew,
    /// A person's `.odg`, with an object and a Basic project.
    Odg,
    /// An `.odg` DiaCraft wrote from a person's `.vsdm` (its VBA project riding).
    OdgMade,
    /// A person's flat `.fodg`.
    Fodg,
    /// A `.diacraft` DiaCraft wrote from a person's `.odg`.
    DiacraftOdg,
    /// A `.diacraft` DiaCraft wrote from a person's `.vsdm`.
    DiacraftVsdm,
    /// A `.diacraft` DiaCraft wrote from a new drawing with what [`Live`] adds.
    DiacraftNew,
    /// A drawing made from an `.otg` of one rectangle and two user-defined fields (the vector).
    OtgFields,
}

const STARTS: [Start; 15] = [
    Start::New,
    Start::Otg,
    Start::Vstx,
    Start::VstxMade,
    Start::Vsdx,
    Start::Vsdm,
    Start::VsdmMade,
    Start::VsdxNew,
    Start::Odg,
    Start::OdgMade,
    Start::Fodg,
    Start::DiacraftOdg,
    Start::DiacraftVsdm,
    Start::DiacraftNew,
    Start::OtgFields,
];

/// The files the starts open, made once for a run.
pub(crate) struct Files {
    pub(crate) dir: Dir,
    pub(crate) picture: String,
    made: BTreeMap<&'static str, Vec<u8>>,
}

/// Opens `bytes` as `name` in a pinned session.
fn open_in(dir: &Dir, name: &str, bytes: &[u8]) -> Result<Session, String> {
    let mut s = pinned();
    s.execute("file.open", &json!({ "path": dir.write(name, bytes) })).map_err(|e| format!("{name} does not open: {e}"))?;
    Ok(s)
}

/// The file a session holding `s` writes when it is saved as `name` with `params`.
fn saved_as(s: &mut Session, dir: &Dir, name: &str, mut params: Value) -> Vec<u8> {
    params["path"] = json!(dir.path(name));
    s.execute("file.save_as", &params).unwrap_or_else(|e| panic!("the start {name} is not written: {e}"));
    dir.read(name)
}

impl Files {
    pub(crate) fn new() -> Files {
        let dir = Dir::new();
        let picture = dir.write("live-picture.png", &crate::across::png(LIVE_PICTURE_SHADE));
        for k in 0..8u8 {
            dir.write(&format!("edit-picture-{k}.png"), &crate::across::png(EDIT_PICTURE_SHADE + k));
        }
        let mut made = BTreeMap::new();
        let odg = odg_person(crate::odg_build::DRAWING);
        let vsdm = vsdx_person("vsdm");
        let drop_macros = json!({ "drop_macros": true });
        made.insert("vstx", saved_as(&mut open_in(&dir, "src-a.odg", &odg).unwrap(), &dir, "made.vstx", drop_macros.clone()));
        made.insert("vsdm", saved_as(&mut open_in(&dir, "src-b.odg", &odg).unwrap(), &dir, "made.vsdm", json!({})));
        made.insert("odg", saved_as(&mut open_in(&dir, "src-c.vsdm", &vsdm).unwrap(), &dir, "made.odg", json!({})));
        made.insert("diacraft-odg", saved_as(&mut open_in(&dir, "src-d.odg", &odg).unwrap(), &dir, "made-odg.diacraft", json!({})));
        made.insert("diacraft-vsdm", saved_as(&mut open_in(&dir, "src-e.vsdm", &vsdm).unwrap(), &dir, "made-vsdm.diacraft", json!({})));
        let all = Live { picture: true, group: true, connector: true, link: true };
        let mut fresh = pinned();
        add_live(&mut fresh, all, &picture).unwrap();
        made.insert("vsdx-new", saved_as(&mut fresh, &dir, "made-new.vsdx", json!({})));
        let mut fresh = pinned();
        add_live(&mut fresh, all, &picture).unwrap();
        made.insert("diacraft-new", saved_as(&mut fresh, &dir, "made-new.diacraft", json!({})));
        Files { dir, picture, made }
    }

    /// A session holding `start`, opened from its file as `name` (with the extension added).
    pub(crate) fn open(&self, start: Start, name: &str) -> Result<Session, String> {
        let made = |key: &str| self.made.get(key).cloned().unwrap_or_default();
        let (extension, bytes) = match start {
            Start::New => return Ok(pinned()),
            Start::Otg => ("otg", odg_person(TEMPLATE)),
            Start::Vstx => ("vstx", vsdx_person("vstx")),
            Start::VstxMade => ("vstx", made("vstx")),
            Start::Vsdx => ("vsdx", vsdx_person("vsdx")),
            Start::Vsdm => ("vsdm", vsdx_person("vsdm")),
            Start::VsdmMade => ("vsdm", made("vsdm")),
            Start::VsdxNew => ("vsdx", made("vsdx-new")),
            Start::Odg => ("odg", odg_person(crate::odg_build::DRAWING)),
            Start::OdgMade => ("odg", made("odg")),
            Start::Fodg => ("fodg", fodg_person()),
            Start::DiacraftOdg => ("diacraft", made("diacraft-odg")),
            Start::DiacraftVsdm => ("diacraft", made("diacraft-vsdm")),
            Start::DiacraftNew => ("diacraft", made("diacraft-new")),
            Start::OtgFields => ("otg", otg_fields()),
        };
        open_in(&self.dir, &format!("{name}.{extension}"), &bytes)
    }
}

// ---------------------------------------------------------------------------------------------------------------- the steps

/// The types a Save As writes, as the extension of the name.
pub(crate) const TARGETS: [&str; 11] =
    ["vsdx", "vsdm", "vstx", "vstm", "vssx", "vssm", "odg", "otg", "diacraft", "diacraft-template", "diacraft-stencil"];

/// The edits a person makes between saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    /// A shape moved.
    Move,
    /// A shape's text set.
    Text,
    /// A picture inserted from a file.
    Picture,
    /// A shape deleted.
    Delete,
}

/// One step of a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A Save As to `TARGETS[target]`, with the options.
    SaveAs { target: usize, drop_objects: bool, drop_macros: bool },
    /// A Save.
    Save,
    /// An edit of the shape at `which` (of the first page's, counted round), then a Save.
    Edit { edit: Edit, which: usize },
}

/// A chain: where it starts, what the session adds first, and the steps.
#[derive(Clone, Debug)]
pub(crate) struct Chain {
    pub(crate) start: Start,
    pub(crate) live: Live,
    pub(crate) steps: Vec<Step>,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0..TARGETS.len(), any::<bool>(), any::<bool>()).prop_map(|(target, drop_objects, drop_macros)| Step::SaveAs { target, drop_objects, drop_macros }),
        3 => Just(Step::Save),
        3 => (prop_oneof![Just(Edit::Move), Just(Edit::Text), Just(Edit::Picture), Just(Edit::Delete)], 0..16usize)
            .prop_map(|(edit, which)| Step::Edit { edit, which }),
    ]
}

pub(crate) fn chain() -> impl Strategy<Value = Chain> {
    let live = (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>(), 0..3u8).prop_map(|(picture, group, connector, link, off)| {
        // A third of the chains add nothing before the first step.
        if off == 0 { Live::default() } else { Live { picture, group, connector, link } }
    });
    (0..STARTS.len(), live, prop::collection::vec(step(), 2..=6)).prop_map(|(start, live, steps)| Chain { start: STARTS[start], live, steps })
}

// ---------------------------------------------------------------------------------------------------------------- running a chain

/// What a chain found wrong: the step it failed at (from 0), and the words.
#[derive(Debug)]
struct Failure {
    at: usize,
    words: String,
    /// The chain had not written a file by a Save As when it failed (or the step is its first Save As): outside the rule, which is about what comes after one.
    outside: bool,
}

/// What the chains did, counted.
#[derive(Debug, Default)]
struct Tally {
    chains: usize,
    steps: usize,
    written: usize,
    refused_as_asked: usize,
    edits: usize,
}

/// The file the drawing has, as the last save wrote it: its name and bytes, and what the Save As that made it left out.
struct Current {
    name: String,
    bytes: Vec<u8>,
    /// The payloads of objects and macro projects the Save As that made the file did not write into it.
    absent: Vec<String>,
    /// Whether the type holds a macro project.
    macro_free: bool,
    /// Whether this session wrote the file: a file a person brings may be written with other bytes by its first unedited Save (the program's name in its properties), and never again.
    ours: bool,
}

/// The characters of a shape's text, its fields' shown text included.
pub(crate) fn plain(shape: &Shape) -> String {
    shape
        .text
        .as_ref()
        .map(|t| {
            t.parts
                .iter()
                .filter_map(|p| match p {
                    TextPart::Chars(c) => Some(c.to_string()),
                    TextPart::Fld { text, .. } => Some(text.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every shape of the first page, group members after their group.
fn all_shapes(doc: &Document) -> Vec<&Shape> {
    let mut out = Vec::new();
    let Some(page) = doc.pages.get(0) else { return out };
    let mut stack: Vec<&Shape> = page.shapes.iter().map(|s| &**s).rev().collect();
    while let Some(shape) = stack.pop() {
        out.push(shape);
        stack.extend(shape.children.iter().map(|c| &**c).rev());
    }
    out
}

/// The pin of a shape of the first page.
pub(crate) fn pin_of(doc: &Document, id: u32) -> Option<(f64, f64)> {
    let page = doc.pages.get(0)?.id;
    diacraft_engine::draw::shape_frame(doc, page, id).map(|f| (f.pin_x, f.pin_y))
}

/// The file at `name` read in a session of its own.
fn reread(dir: &Dir, name: &str) -> Result<Session, String> {
    let mut back = pinned();
    back.execute("file.open", &json!({ "path": dir.path(name) })).map_err(|e| format!("the file written does not open again: {e}"))?;
    Ok(back)
}

/// The checks of a file just written: each payload held once at most, and what the Save As that made the file left out still out of it.
fn check_written(current: &Current, payloads: &[(String, Vec<u8>, Kind)]) -> Result<(), String> {
    let Some(found) = counts(&current.bytes, payloads) else { return Ok(()) };
    for (name, n) in &found {
        if *n > 1 {
            return Err(format!("{name} is held {n} times by {}", current.name));
        }
    }
    for name in &current.absent {
        if found.contains_key(name) {
            return Err(format!("{name}, which the Save As left out, is in {} again", current.name));
        }
    }
    if current.macro_free
        && let Some((name, _)) = found.iter().find(|(name, _)| payloads.iter().any(|(n, _, kind)| n == *name && *kind == Kind::Macro))
    {
        return Err(format!("{name} is in {}, a type that holds no macro project", current.name));
    }
    Ok(())
}

/// The names of the custom properties of a drawing (the user-defined fields of an `.odg`, the custom properties of a VSDX file), sorted.
fn custom_names(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = doc.props.custom.iter().map(|c| c.name.to_string()).collect();
    names.sort();
    names
}

/// **The file written holds the drawing's custom properties** (a Save after a Save As took them out of a VSDX file): read again, it has every name the drawing has.
fn check_properties(s: &Session, dir: &Dir, name: &str) -> Result<(), String> {
    let back = reread(dir, name)?;
    let (want, got) = (custom_names(s.document().unwrap()), custom_names(back.document().unwrap()));
    let lost: Vec<&String> = want.iter().filter(|n| !got.contains(n)).collect();
    if lost.is_empty() { Ok(()) } else { Err(format!("{name} lost the custom properties {lost:?} the drawing holds")) }
}

/// The checks of an unedited Save: no refusal, no edit, not dirty, the same bytes.
fn check_unedited(s: &mut Session, files: &Files, current: &Current) -> Result<Vec<u8>, String> {
    let again = s.execute("file.save", &json!({})).map_err(|e| format!("an unedited Save was refused: {e}"))?;
    if again.get("edited").is_some_and(|edited| *edited != json!(false)) {
        return Err(format!("an unedited Save counts as an edit: {}", again["edited"]));
    }
    if again["dirty"] != json!(false) || s.is_dirty() {
        return Err("the drawing is dirty after an unedited Save".to_owned());
    }
    let now = files.dir.read(&current.name);
    if current.ours && now != current.bytes {
        return Err("an unedited Save wrote other bytes".to_owned());
    }
    Ok(now)
}

/// Whether a refusal of a Save As is one the drawing asks for: a macro project in a type that cannot hold one (and the Save As did not leave it out), or a stencil saved as an OpenDocument file.
fn refused_as_asked(e: &EngineError, target: &str, drop_macros: bool, holds_macros: bool, stencil: bool) -> bool {
    match e {
        EngineError::MacrosNotAllowed { .. } => !drop_macros && holds_macros && matches!(target, "vsdx" | "vstx" | "vssx"),
        _ => stencil && matches!(target, "odg" | "otg") && e.to_string().contains("a stencil is not saved as an .odg"),
    }
}

/// Runs `chain`: `Ok` with what it did when every step kept the rule, else the first step that did not.
fn run_chain(files: &Files, chain: &Chain, case: usize, tally: &mut Tally) -> Result<(), Failure> {
    let payloads = payloads();
    // Whether a Save As has written a file in this chain: the rule is about what comes after one, and a failure before it is said apart (`Failure::outside`).
    let mut after_save_as = false;
    let fail = |at: usize, words: String, after: bool| Failure { at, words, outside: !after };
    let mut s = files.open(chain.start, &format!("start-{case}")).map_err(|w| fail(0, w, false))?;
    if chain.live.any() {
        add_live(&mut s, chain.live, &files.picture).map_err(|w| fail(0, format!("before the first step: {w}"), false))?;
    }
    // What the drawing holds of what a Save As may leave out: it keeps it, whatever is saved.
    let kept_from_the_start = held_by(s.document().unwrap(), &payloads, &[Kind::Object, Kind::Macro]);
    let mut current: Option<Current> = s.path().map(|path| {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let bytes = files.dir.read(&name);
        Current { name, bytes, absent: Vec::new(), macro_free: false, ours: false }
    });
    let mut deleted = false;
    let mut pictures = 0u8;
    tally.chains += 1;
    for (at, step) in chain.steps.iter().enumerate() {
        tally.steps += 1;
        let dirty = s.is_dirty();
        let after = after_save_as;
        match *step {
            Step::SaveAs { target, drop_objects, drop_macros } => {
                let extension = TARGETS[target];
                let name = format!("case-{case}-step-{at}.{extension}");
                let holds_macros = !held_by(s.document().unwrap(), &payloads, &[Kind::Macro]).is_empty();
                let stencil = s.execute("file.status", &json!({})).is_ok_and(|status| status["kind"] == json!("stencil"));
                let params = json!({ "path": files.dir.path(&name), "drop_objects": drop_objects, "drop_macros": drop_macros });
                match s.execute("file.save_as", &params) {
                    Err(e) if refused_as_asked(&e, extension, drop_macros, holds_macros, stencil) => tally.refused_as_asked += 1,
                    Err(e) => return Err(fail(at, format!("a Save As to .{extension} was refused: {e}"), after)),
                    Ok(_) => {
                        tally.written += 1;
                        after_save_as = true;
                        if s.is_dirty() {
                            return Err(fail(at, format!("the drawing is dirty after a Save As to .{extension}"), true));
                        }
                        let bytes = files.dir.read(&name);
                        let found = counts(&bytes, &payloads).unwrap_or_default();
                        // (`drop_objects` and `drop_macros` leave out what is of the other format, or what the type cannot hold: a VSDX file's own embedding stays in a VSDX type, and an
                        // `.odg`'s own object in an `.odg`. What the Save As left out is `absent`, which every later save into the file keeps out.)
                        let absent: Vec<String> = kept_from_the_start.iter().filter(|p| !found.contains_key(*p)).cloned().collect();
                        let macro_free = matches!(extension, "vsdx" | "vstx" | "vssx");
                        let made = Current { name, bytes, absent, macro_free, ours: true };
                        check_written(&made, &payloads).map_err(|w| fail(at, format!("after a Save As to .{extension}: {w}"), true))?;
                        check_properties(&s, &files.dir, &made.name).map_err(|w| fail(at, format!("after a Save As to .{extension}: {w}"), true))?;
                        current = Some(made);
                    }
                }
            }
            Step::Save => match &mut current {
                None => match s.execute("file.save", &json!({})) {
                    Err(e) if e.to_string().contains("has no file yet") => {}
                    other => return Err(fail(at, format!("a Save of a drawing with no file: {other:?}"), after)),
                },
                Some(file) if !dirty => {
                    file.bytes = check_unedited(&mut s, files, file).map_err(|w| fail(at, w, after))?;
                    file.ours = true;
                    tally.written += 1;
                    check_properties(&s, &files.dir, &file.name).map_err(|w| fail(at, format!("after a Save: {w}"), after))?;
                }
                Some(file) => {
                    // The session's own additions, never saved yet: a Save writes them.
                    s.execute("file.save", &json!({})).map_err(|e| fail(at, format!("a Save of what the session added was refused: {e}"), after))?;
                    tally.written += 1;
                    file.bytes = files.dir.read(&file.name);
                    file.ours = true;
                    check_written(file, &payloads).map_err(|w| fail(at, format!("after a Save of what the session added: {w}"), after))?;
                    if s.is_dirty() {
                        return Err(fail(at, "the drawing is dirty after a Save".to_owned(), after));
                    }
                }
            },
            Step::Edit { edit, which } => {
                let doc = s.document().unwrap();
                let page = first_page(&s);
                // A text goes on a shape that holds one in every format: not a group, a picture or an object frame (an `.odg` holds no text of a graphic frame, and an `.odg`'s group's own
                // text is not written yet), and not the box whose paragraph holds a link (the word `site`: an `.odg` keeps no link in a paragraph it edits, the open question Q4). Each is
                // refused in words by the writer of an `.odg`, in any session, and is no save's to answer for.
                let top: Vec<u32> = doc
                    .pages
                    .get(0)
                    .unwrap()
                    .shapes
                    .iter()
                    .filter(|shape| edit != Edit::Text || (shape.children.is_empty() && shape.foreign.is_none() && !plain(shape).contains("site")))
                    // A move moves a shape of two dimensions: a connector stays on what it is glued to, and the `.odg` reader places one by its glue (a connector whose other end's shape was
                    // deleted is read back from an `.otg` elsewhere than the drawing has it: a question of connectors, said in the report, and no save's).
                    .filter(|shape| edit != Edit::Move || shape.sheet.cell("BeginX").is_none())
                    .map(|shape| shape.id)
                    .collect();
                let id = top.get(which % top.len().max(1)).copied();
                let text = format!("Edited {case} {at}");
                let before = id.and_then(|id| pin_of(doc, id));
                let applied = match (edit, id) {
                    (Edit::Move, Some(id)) => {
                        s.execute("shape.move", &json!({ "ids": [id], "dx": 0.25, "dy": 0.5, "page": page })).is_ok()
                            // (A connector glued at both ends stays where its ends are: a move that moves nothing is no edit.)
                            && pin_of(s.document().unwrap(), id) != before
                    }
                    (Edit::Text, Some(id)) => s.execute("shape.set_text", &json!({ "id": id, "text": text, "page": page })).is_ok(),
                    (Edit::Delete, Some(id)) => s.execute("shape.delete", &json!({ "ids": [id], "page": page })).is_ok(),
                    (Edit::Picture, _) if pictures < 8 => {
                        let path = files.dir.path(&format!("edit-picture-{pictures}.png"));
                        pictures += 1;
                        s.execute("insert.picture", &json!({ "path": path, "page": page, "x": 5.0, "y": 4.0 })).is_ok()
                    }
                    _ => false,
                };
                if applied {
                    tally.edits += 1;
                    deleted |= edit == Edit::Delete;
                }
                let Some(file) = &mut current else { continue };
                if !applied && !s.is_dirty() {
                    file.bytes = check_unedited(&mut s, files, file).map_err(|w| fail(at, format!("(no edit applied) {w}"), after))?;
                    file.ours = true;
                    continue;
                }
                let saved = s
                    .execute("file.save", &json!({}))
                    .map_err(|e| fail(at, format!("an edit ({edit:?}) was refused by the Save after it: {e}"), after))?;
                tally.written += 1;
                let written = files.dir.read(&file.name);
                if applied && (written == file.bytes || saved.get("edited").is_some_and(|edited| *edited == json!(false))) {
                    return Err(fail(at, format!("an edit ({edit:?}) was not written: edited {}", saved["edited"]), after));
                }
                if s.is_dirty() {
                    return Err(fail(at, format!("the drawing is dirty after an edit ({edit:?}) was saved"), after));
                }
                file.bytes = written;
                file.ours = true;
                check_written(file, &payloads).map_err(|w| fail(at, format!("after an edit ({edit:?}): {w}"), after))?;
                check_properties(&s, &files.dir, &file.name).map_err(|w| fail(at, format!("after an edit ({edit:?}): {w}"), after))?;
                if applied {
                    let back = reread(&files.dir, &file.name).map_err(|w| fail(at, w, after))?;
                    let (now, there) = (s.document().unwrap(), back.document().unwrap());
                    let ok = match edit {
                        Edit::Move => {
                            let want = id.and_then(|id| pin_of(now, id)).unwrap();
                            all_shapes(there).iter().any(|shape| {
                                pin_of(there, shape.id).is_some_and(|got| (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6)
                            })
                        }
                        Edit::Text => all_shapes(there).iter().any(|shape| plain(shape).contains(&text)),
                        Edit::Delete => there.pages.get(0).unwrap().shapes.len() == now.pages.get(0).unwrap().shapes.len(),
                        Edit::Picture => {
                            let bytes = crate::across::png(EDIT_PICTURE_SHADE + pictures - 1);
                            all_shapes(there)
                                .iter()
                                .any(|shape| shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some_and(|m| &*m.bytes == bytes.as_slice()))
                        }
                    };
                    if !ok {
                        return Err(fail(at, format!("the file read again does not have the edit ({edit:?})"), after));
                    }
                }
            }
        }
        // The drawing keeps what a Save As left out of a file.
        let now = held_by(s.document().unwrap(), &payloads, &[Kind::Object, Kind::Macro]);
        for name in &kept_from_the_start {
            if !now.contains(name) && !(deleted && name == "the object") {
                return Err(fail(at, format!("the drawing lost {name}"), after_save_as));
            }
        }
    }
    Ok(())
}

/// The words of a failure with its numbers taken out, which group the failures of one cause.
fn class_of(chain: &Chain, failure: &Failure) -> String {
    let step = chain.steps.get(failure.at).map(|step| match step {
        Step::SaveAs { .. } => "Save As".to_owned(),
        Step::Save => "Save".to_owned(),
        Step::Edit { edit, .. } => format!("{edit:?}"),
    });
    let words: String = failure.words.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
    let words = words.split("case-#").next().unwrap_or("").to_owned();
    let outside = if failure.outside { "[before any Save As] " } else { "" };
    format!("{outside}{} | {}", step.unwrap_or_else(|| "start".to_owned()), words.chars().take(160).collect::<String>())
}

/// Runs `cases` random chains from the seed, and gives every failure with its chain.
fn sweep(cases: u32, seed: u64) -> (Tally, Vec<(Chain, Failure)>) {
    let files = Files::new();
    let config = Config { cases, failure_persistence: None, ..Config::default() };
    let mut seed_bytes = [0u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes));
    let strategy = chain();
    let mut tally = Tally::default();
    let mut failures = Vec::new();
    for case in 0..cases as usize {
        let chain = strategy.new_tree(&mut runner).unwrap().current();
        if let Err(failure) = run_chain(&files, &chain, case, &mut tally) {
            failures.push((chain, failure));
        }
    }
    (tally, failures)
}

/// The failures grouped by class, each with how many and the shortest chain that shows it.
fn grouped(failures: &[(Chain, Failure)]) -> String {
    let mut classes: BTreeMap<String, (usize, &Chain, &Failure)> = BTreeMap::new();
    for (chain, failure) in failures {
        let class = class_of(chain, failure);
        let entry = classes.entry(class).or_insert((0, chain, failure));
        entry.0 += 1;
        if failure.at < entry.2.at {
            entry.1 = chain;
            entry.2 = failure;
        }
    }
    classes
        .iter()
        .map(|(class, (n, chain, failure))| {
            format!(
                "{n} x {class}\n    e.g. {:?} {:?} {:?}\n    failed at step {}: {}",
                chain.start, chain.live, chain.steps, failure.at, failure.words
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// **Every chain keeps the rule**: random chains of two to six steps from every start, a fixed seed and a case count set here (nextest runs this; `PROPTEST_CASES` is not read).
#[test]
fn chains_keep_the_rule() {
    let (tally, failures) = sweep(400, 0x5EED_0001);
    assert!(tally.chains == 400 && tally.written > 400, "{tally:?}");
    let broken: Vec<(Chain, Failure)> = failures.into_iter().filter(|(_, failure)| !failure.outside).collect();
    assert!(broken.is_empty(), "{} of {} chains failed ({tally:?}):\n{}", broken.len(), tally.chains, grouped(&broken));
}

/// **The local sweep**: many more chains, from the seed `DIACRAFT_RESAVE_SEED` (default 1) and `DIACRAFT_RESAVE_CASES` of them (default 2,000), with the failures grouped by their words.
/// `cargo test -p diacraft-engine --test it resave_sweep::a_long_sweep_of_chains -- --ignored --nocapture`.
#[test]
#[ignore = "a local sweep of many chains: minutes"]
fn a_long_sweep_of_chains() {
    let cases = std::env::var("DIACRAFT_RESAVE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let seed = std::env::var("DIACRAFT_RESAVE_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let (tally, failures) = sweep(cases, seed);
    let broken = failures.iter().filter(|(_, failure)| !failure.outside).count();
    println!("{tally:?}\n{} of {} chains failed, {broken} of them after a Save As\n{}", failures.len(), tally.chains, grouped(&failures));
    assert!(broken == 0, "{broken} of {} chains failed after a Save As", tally.chains);
}

/// A chain that `proptest` shrinks when it fails: the same property through the macro, for a failure's smallest chain (`#[ignore]`: the sweep above is the test).
#[test]
#[ignore = "a shrinking run, for a failure's smallest chain"]
fn a_shrinking_run() {
    let files = Files::new();
    let config = Config { cases: 64, failure_persistence: None, ..Config::default() };
    let mut runner = TestRunner::new(config);
    let case = std::cell::Cell::new(0usize);
    let result = runner.run(&chain(), |chain| {
        case.set(case.get() + 1);
        let mut tally = Tally::default();
        run_chain(&files, &chain, 100_000 + case.get(), &mut tally).map_err(|f| TestCaseError::fail(format!("step {}: {}", f.at, f.words)))
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

// ------------------------------------------------------------------------------------------------------------- the classes

// One test for each class of failure the sweep found, each the shortest chain that shows it (red on `f9676b22`). The report of names the cause and the fix of each.

/// The types of a VSDX file.
const VSDX_TYPES: [&str; 6] = ["vsdx", "vsdm", "vstx", "vstm", "vssx", "vssm"];

/// What a Save As of a drawing that holds a macro project asks of `target`: a macro-free type leaves the project out.
fn asked_of(target: &str) -> Value {
    if matches!(target, "vsdx" | "vstx" | "vssx") { json!({ "drop_macros": true }) } else { json!({}) }
}

/// The Save As of the session to `name` with `params`: the bytes written. Panics, saying `label`, when it is refused.
fn written_as(s: &mut Session, dir: &Dir, name: &str, mut params: Value, label: &str) -> Vec<u8> {
    params["path"] = json!(dir.path(name));
    s.execute("file.save_as", &params).unwrap_or_else(|e| panic!("{label}: the Save As to {name} was refused: {e}"));
    dir.read(name)
}

/// A Save with no edit: not refused, no edit, not dirty, and the bytes `first`. `Err` with what is wrong.
fn saved_again(s: &mut Session, dir: &Dir, name: &str, first: &[u8]) -> Result<(), String> {
    let again = s.execute("file.save", &json!({})).map_err(|e| format!("the unedited Save was refused: {e}"))?;
    if again.get("edited").is_some_and(|edited| *edited != json!(false)) {
        return Err(format!("the unedited Save is an edit: {again}"));
    }
    if s.is_dirty() {
        return Err("the drawing is dirty after the Save".to_owned());
    }
    if dir.read(name) != first {
        return Err("the unedited Save wrote other bytes".to_owned());
    }
    Ok(())
}

/// The `ID` of the first shape of the first page that shows `bytes` as its picture.
fn showing(doc: &Document, bytes: &[u8]) -> Option<u32> {
    all_shapes(doc).iter().find(|s| s.foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some_and(|m| &*m.bytes == bytes)).map(|s| s.id)
}

/// Whether the drawing's first embedded object shows a picture.
fn object_shows_a_picture(doc: &Document) -> bool {
    all_shapes(doc)
        .iter()
        .find(|sh| sh.foreign.as_ref().is_some_and(|f| f.attr("ForeignType") == Some("Object")))
        .is_some_and(|o| o.foreign.as_ref().is_some_and(|f| f.picture.is_some()))
}

/// **Class C (the known open case 1): a picture inserted from a file into a new drawing, saved as any VSDX type, was refused on its next Save** ("a change to the picture of a shape"). The drawing holds
/// the picture by its bytes and names no relationship; the file the model writer made names a row, a part of its own and the picture's `CompressionType`; the splice compared the two field by field.
/// Also a picture the drawing read from an earlier file, whose relationship the model writer numbered anew. Now the Save is no edit and the same bytes, a move of the picture is written, and the Save
/// after it is no edit.
#[test]
fn class_c_a_picture_held_by_its_bytes_is_the_picture_its_vsdx_file_names() {
    let files = Files::new();
    let dir = &files.dir;
    let live_picture = crate::across::png(LIVE_PICTURE_SHADE);
    let mut failures = Vec::new();
    for target in VSDX_TYPES {
        let label = format!("a new drawing with a picture inserted, saved as .{target}");
        let mut s = pinned();
        add_live(&mut s, Live { picture: true, ..Live::default() }, &files.picture).unwrap();
        let name = format!("c-{target}.{target}");
        let first = written_as(&mut s, dir, &name, json!({}), &label);
        if let Err(why) = saved_again(&mut s, dir, &name, &first) {
            failures.push(format!("{label}: {why}"));
            continue;
        }
        let id = showing(s.document().unwrap(), &live_picture).unwrap();
        let page = first_page(&s);
        s.execute("shape.move", &json!({ "ids": [id], "dx": 1.0, "dy": 0.5, "page": page })).unwrap();
        match s.execute("file.save", &json!({})) {
            Err(e) => failures.push(format!("{label}: the move of the picture was refused: {e}")),
            Ok(_) => {
                let written = dir.read(&name);
                let back = reread(dir, &name).unwrap();
                if pin_of(back.document().unwrap(), id) != pin_of(s.document().unwrap(), id) {
                    failures.push(format!("{label}: the move is not in the file"));
                }
                if let Err(why) = saved_again(&mut s, dir, &name, &written) {
                    failures.push(format!("{label}: after the move: {why}"));
                }
            }
        }
    }
    // A person's `.vsdx` with a picture of its own and one inserted, across an `.odg` and back to a VSDX type: the model writer numbers the file's picture's relationship anew.
    let label = "a .vsdx with a picture, one inserted, saved as .odg and then as .vstx";
    let mut s = files.open(Start::Vsdx, "c-person").unwrap();
    add_live(&mut s, Live { picture: true, ..Live::default() }, &files.picture).unwrap();
    written_as(&mut s, dir, "c-person.odg", json!({}), label);
    let first = written_as(&mut s, dir, "c-person.vstx", json!({ "drop_macros": true }), label);
    if let Err(why) = saved_again(&mut s, dir, "c-person.vstx", &first) {
        failures.push(format!("{label}: {why}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class C, an embedded object: a VSDX object carried by an `.odg`, whose fallback picture the `.odg` holds as the object's replacement, saved as a VSDX type from a `.diacraft`**: the file's picture
/// is a part of the package's, the drawing's the replacement, and the splice refused "a change to the picture of a shape" at the next Save. The object is the same by its relationship, its attributes
/// and its picture's bytes.
#[test]
fn class_c_an_object_is_the_same_by_its_relationship_and_its_pictures_bytes() {
    let files = Files::new();
    let dir = &files.dir;
    let mut s = files.open(Start::OdgMade, "co").unwrap();
    written_as(&mut s, dir, "co.diacraft", json!({ "drop_macros": true }), "co");
    let first = written_as(&mut s, dir, "co.vssm", json!({}), "co");
    saved_again(&mut s, dir, "co.vssm", &first).unwrap_or_else(|why| panic!("an .odg of a .vsdm saved as .diacraft and as .vssm: {why}"));
}

/// **Class A: a picture inserted into a drawing whose file is a VSDX type was refused by the Save** ("a picture new to a page the file already has"), and **class H: one inserted on a blank page was
/// saved with no `Rel`, lost with no word.** Now the Save writes it: the file read again shows it, and the Save after it is no edit and the same bytes. From a blank new drawing and one of a box,
/// each saved as a VSDX type first, and from a person's `.vsdx` saved in place.
#[test]
fn class_a_and_h_a_picture_inserted_after_a_save_as_to_a_vsdx_type_is_written_by_the_save() {
    let files = Files::new();
    let dir = &files.dir;
    let inserted = crate::across::png(EDIT_PICTURE_SHADE);
    let mut failures = Vec::new();
    for (what, start, boxed) in
        [("a blank new drawing", Start::New, false), ("a new drawing of a box", Start::New, true), ("a person's .vsdx", Start::Vsdx, false)]
    {
        let targets: Vec<&str> = if start == Start::Vsdx { vec!["vsdx"] } else { VSDX_TYPES.to_vec() };
        for target in targets {
            let label = format!("{what}, as .{target}");
            let mut s = files.open(start, &format!("a-{target}-{boxed}")).unwrap();
            if boxed {
                dropped(&mut s, 2.0, 2.0).unwrap();
            }
            let name = match start {
                Start::Vsdx => s.path().unwrap().file_name().unwrap().to_str().unwrap().to_owned(),
                _ => {
                    let name = format!("a-{target}-{boxed}.{target}");
                    written_as(&mut s, dir, &name, json!({}), &label);
                    name
                }
            };
            let page = first_page(&s);
            s.execute("insert.picture", &json!({ "path": dir.path("edit-picture-0.png"), "page": page, "x": 5.0, "y": 4.0 })).unwrap();
            if let Err(e) = s.execute("file.save", &json!({})) {
                failures.push(format!("{label}: the Save of the picture was refused: {e}"));
                continue;
            }
            let written = dir.read(&name);
            if showing(reread(dir, &name).unwrap().document().unwrap(), &inserted).is_none() {
                failures.push(format!("{label}: the file read again does not show the picture"));
                continue;
            }
            if let Err(why) = saved_again(&mut s, dir, &name, &written) {
                failures.push(format!("{label}: {why}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class I: an unedited Save after a Save As of a drawing as it was read wrote other bytes.** The model writer keeps the bookkeeping of a drawing whose content is as read (the program's name in
/// `Application`: the unedited visit), and the splice of the next Save named DiaCraft there. Now a Save with no edit into a file this session wrote leaves it: an `.odg`
/// DiaCraft wrote from a person's `.vsdm`, opened and saved as each macro-enabled VSDX type, is saved again as it was.
#[test]
fn class_i_an_unedited_save_after_a_save_as_of_a_drawing_as_read_writes_the_same_bytes() {
    let files = Files::new();
    let mut failures = Vec::new();
    for target in ["vsdm", "vstm", "vssm", "vsdx"] {
        let mut s = files.open(Start::OdgMade, &format!("i-{target}")).unwrap();
        let name = format!("i.{target}");
        let first = written_as(&mut s, &files.dir, &name, asked_of(target), target);
        if let Err(why) = saved_again(&mut s, &files.dir, &name, &first) {
            failures.push(format!(".{target}: {why}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class N: a Save As from a drawing that stands on the `.odg` it was read from wrote that file's package data as it is now, which the drawing did not hold** (a splice into the `.odg` writes its
/// generator, its thumbnail and rows of its own), and the Save after it, standing on the new file, wrote the drawing's own: other bytes with no edit. To `.diacraft`, and to another `.odg` asked to
/// leave something out (both written from the model). Now the drawing becomes the model written.
#[test]
fn class_n_a_save_as_from_a_drawing_saved_into_its_odg_is_saved_again_as_it_was() {
    let files = Files::new();
    let mut failures = Vec::new();
    for (k, (target, params)) in
        [("diacraft", json!({})), ("odg", json!({ "drop_objects": true })), ("odg", json!({ "drop_macros": true, "drop_objects": true }))]
            .into_iter()
            .enumerate()
    {
        let label = format!("a person's .odg edited and saved, then saved as .{target} with {params}");
        let mut s = files.open(Start::Odg, &format!("n-{k}")).unwrap();
        add_live(&mut s, Live { picture: true, group: true, ..Live::default() }, &files.picture).unwrap();
        s.execute("file.save", &json!({})).unwrap();
        let name = format!("n-{k}-out.{target}");
        let first = written_as(&mut s, &files.dir, &name, params, &label);
        if let Err(why) = saved_again(&mut s, &files.dir, &name, &first) {
            failures.push(format!("{label}: {why}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class E: a drawing that stands on an `.odg` this session wrote from its model was given that `.odg`'s package data by the next save from the model**: the file's parts are what the model writer
/// made of the drawing's (a VSDX object as the `.odg`'s own object file), and on the drawing's VSDX shapes they named nothing it shows. A person's `.vsdm` with an object, saved as `.odg` and then as
/// `.vsdm`, held the object only as an entry of DiaCraft's own, its shape naming an embedding that was not there (the file read again showed no picture for it); a second `.odg` written with
/// `drop_objects` was an edit at its next Save. Now each is saved again as it was, and the object shows its picture.
#[test]
fn class_e_a_drawing_takes_no_package_data_from_an_odg_written_from_its_own_model() {
    let files = Files::new();
    let dir = &files.dir;
    let mut failures = Vec::new();
    let mut s = files.open(Start::Vsdm, "e-vsdm").unwrap();
    written_as(&mut s, dir, "e.odg", json!({}), "e");
    let first = written_as(&mut s, dir, "e.vsdm", json!({}), "e");
    if !object_shows_a_picture(reread(dir, "e.vsdm").unwrap().document().unwrap()) {
        failures.push("a .vsdm saved as .odg and then as .vsdm: the object shows no picture in the file read again".to_owned());
    }
    if counts(&first, &payloads()).unwrap_or_default().get("the object") != Some(&1) {
        failures.push(format!("a .vsdm saved as .odg and then as .vsdm: the object is not held once: {:?}", counts(&first, &payloads())));
    }
    if let Err(why) = saved_again(&mut s, dir, "e.vsdm", &first) {
        failures.push(format!("a .vsdm saved as .odg and then as .vsdm: {why}"));
    }
    let mut s = files.open(Start::DiacraftVsdm, "e-native").unwrap();
    written_as(&mut s, dir, "e-a.odg", json!({ "drop_macros": true }), "e");
    let first = written_as(&mut s, dir, "e-b.odg", json!({ "drop_objects": true }), "e");
    if let Err(why) = saved_again(&mut s, dir, "e-b.odg", &first) {
        failures.push(format!("a .diacraft saved as .odg and as another .odg with drop_objects: {why}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class D: the entries of DiaCraft's folder in an `.odg` are named by their place among the parts that ride, and a save took an entry of the name for the one named** (it added only a name the
/// file lacked). A `.diacraft` of a person's `.vsdm` saved as `.otg`, and its object deleted and saved: the object's embedding rides from then on, `blob0` named the object in the document file and
/// still held the VBA project, which went into `blob1` too. Now the entries are what the document file names: the VBA project once, and read again it is the project's bytes.
#[test]
fn class_d_the_entries_of_diacrafts_folder_hold_what_the_document_file_names() {
    let files = Files::new();
    let dir = &files.dir;
    let mut s = files.open(Start::DiacraftVsdm, "d").unwrap();
    written_as(&mut s, dir, "d.otg", json!({}), "d");
    let page = first_page(&s);
    let object = all_shapes(s.document().unwrap())
        .iter()
        .find(|sh| sh.foreign.as_ref().is_some_and(|f| f.attr("ForeignType") == Some("Object")))
        .map(|sh| sh.id)
        .unwrap();
    s.execute("shape.delete", &json!({ "ids": [object], "page": page })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let written = dir.read("d.otg");
    let found = counts(&written, &payloads()).unwrap();
    assert_eq!(found.get("the VBA project"), Some(&1), "the VBA project is held once: {found:?}");
    let (back, _) = diacraft_odg::read(&written, &diacraft_odg::Limits::default()).unwrap();
    let vba = back.parts.iter().find(|p| &*p.name == "/visio/vbaProject.bin").expect("the drawing read again holds the VBA project");
    assert_eq!(&*vba.bytes, VBA, "and its bytes are the project's, not another part's");
    let embedding = back.parts.iter().find(|p| p.name.contains("oleObject")).expect("and the object's embedding, which rides now");
    assert_eq!(&*embedding.bytes, crate::across::compound().as_slice(), "whose bytes are the object's");
}

/// **Class B: the baseline a VSDX save returns held the model's parts, not the file's.** (1) A Save As to a macro-free type that left the macro project out said the file held it, so a Save As from
/// that file to a macro-enabled type wrote no project (the drawing and the baseline both had it) and the next edit did. (2) A save that wrote an `.odg`'s object as an embedding said the file held the
/// object's file and not the embedding, so the drawing that became the file's model named an embedding it did not have, and a later file from it showed no picture for the object. Now each is what
/// the file holds.
#[test]
fn class_b_the_baseline_of_a_vsdx_save_is_the_parts_the_file_holds() {
    let files = Files::new();
    let dir = &files.dir;
    let mut failures = Vec::new();
    // (1) The macro project in the macro-enabled type, at the Save As.
    let mut s = files.open(Start::VsdmMade, "b-macros").unwrap();
    written_as(&mut s, dir, "b.vssx", json!({ "drop_macros": true }), "b");
    let first = written_as(&mut s, dir, "b.vssm", json!({}), "b");
    if counts(&first, &payloads()).unwrap_or_default().get("the Basic module") != Some(&1) {
        failures.push("a .vssx with the project left out, saved as .vssm: the .vssm does not hold the project the drawing holds".to_owned());
    }
    // (2) The object across an `.odg` and back: it shows its picture, and the next Save is no edit.
    let mut s = files.open(Start::DiacraftOdg, "b-object").unwrap();
    written_as(&mut s, dir, "b-1.vstm", json!({}), "b");
    written_as(&mut s, dir, "b-2.otg", json!({ "drop_macros": true }), "b");
    let last = written_as(&mut s, dir, "b-3.vstm", json!({}), "b");
    if !object_shows_a_picture(reread(dir, "b-3.vstm").unwrap().document().unwrap()) {
        failures.push("a .diacraft of an .odg saved as .vstm, .otg and .vstm: the object shows no picture in the last file".to_owned());
    }
    if let Err(why) = saved_again(&mut s, dir, "b-3.vstm", &last) {
        failures.push(format!("a .diacraft of an .odg saved as .vstm, .otg and .vstm: {why}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **Class U (the vector, sample case `0747c38e644f0ebf`): a drawing made from an `.otg` with user-defined fields, saved as `.vsdx` or `.vsdm`, then saved with no edit, rewrote
/// `docProps/custom.xml` with none of its two properties.** The splice compared the whole list of custom properties, and the part holds the VSDX ones (the build number and the unit system the model
/// writer gives a new drawing's file); the `.odg`'s fields ride in DiaCraft's part. Now only the VSDX ones are compared: the Save is no edit and the same bytes, and the fields are in the file read again.
#[test]
fn class_u_user_defined_fields_of_an_odg_template_are_no_edit_in_a_vsdx_file() {
    let files = Files::new();
    let dir = &files.dir;
    let mut failures = Vec::new();
    for (k, (what, start)) in
        [("an .otg of one rectangle with two fields", Start::OtgFields), ("a person's .otg", Start::Otg), ("a person's .odg", Start::Odg)]
            .into_iter()
            .enumerate()
    {
        for target in ["vsdx", "vsdm"] {
            let label = format!("{what}, saved as .{target}");
            let mut s = files.open(start, &format!("u-{k}-{target}")).unwrap();
            let name = format!("u-{k}-out.{target}");
            let first = written_as(&mut s, dir, &name, asked_of(target), &label);
            if let Err(why) = saved_again(&mut s, dir, &name, &first) {
                failures.push(format!("{label}: {why}"));
            }
            if let Err(why) = check_properties(&s, dir, &name) {
                failures.push(format!("{label}: {why}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **The known open case 2 (the concern 2)**: in one session, an `.odg` saved as `.vsdm`, `.odg`, `.diacraft`, `.vsdx` with `drop_macros`, and then as `.vsdm`: each Save As is written and
/// the Save after each is no edit.
#[test]
fn the_known_case_of_an_odg_saved_through_five_types_is_written_and_saved_again_as_it_was() {
    let files = Files::new();
    let dir = &files.dir;
    let mut s = files.open(Start::Odg, "k").unwrap();
    for (name, params) in [
        ("k-1.vsdm", json!({})),
        ("k-2.odg", json!({})),
        ("k-3.diacraft", json!({})),
        ("k-4.vsdx", json!({ "drop_macros": true })),
        ("k-5.vsdm", json!({})),
    ] {
        let first = written_as(&mut s, dir, name, params, name);
        saved_again(&mut s, dir, name, &first).unwrap_or_else(|why| panic!("{name}: {why}"));
    }
}

/// **Class R: a Save As that left something out put back on the drawing an object's file the file it stood on no longer held**, and the next edit wrote it into the file the Save As wrote without
/// it. A person's `.odg` whose object frame is deleted and saved (the `.odg` drops the object's entry; the drawing keeps the part), saved as `.vssx` with `drop_macros`: the Save As was given the
/// `.odg`'s parts and wrote no object, `restore` put the part back from the session's drawing, and the baseline said the file held it. Now the drawing is restored from the one the writer was given, and
/// a move saved after it leaves the object out.
#[test]
fn class_r_what_a_save_as_did_not_write_is_not_put_into_its_file_by_an_edit() {
    let files = Files::new();
    let dir = &files.dir;
    let mut s = files.open(Start::Odg, "r").unwrap();
    let page = first_page(&s);
    let frame = all_shapes(s.document().unwrap())
        .iter()
        .find(|sh| {
            sh.foreign.as_ref().is_some_and(|f| f.picture.as_ref().is_some_and(|m| &*m.bytes == crate::across::png(ODG_REPLACEMENT_SHADE).as_slice()))
        })
        .map(|sh| sh.id)
        .unwrap();
    s.execute("shape.delete", &json!({ "ids": [frame], "page": page })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let first = written_as(&mut s, dir, "r.vssx", json!({ "drop_macros": true }), "r");
    assert!(!counts(&first, &payloads()).unwrap().contains_key("the object"), "the Save As writes no object");
    let id = s
        .document()
        .unwrap()
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.foreign.is_none() && sh.sheet.cell("BeginX").is_none())
        .map(|sh| sh.id)
        .unwrap();
    s.execute("shape.move", &json!({ "ids": [id], "dx": 0.5, "dy": 0.5, "page": page })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let written = dir.read("r.vssx");
    assert!(written != first, "the move is written");
    assert!(!counts(&written, &payloads()).unwrap().contains_key("the object"), "and the object is not put into the file");
    saved_again(&mut s, dir, "r.vssx", &written).unwrap_or_else(|why| panic!("after the move: {why}"));
}

/// **Class S: a patch that left a page part's bytes as they were counted as an edit**, and the save stamped a time and DiaCraft's name: other bytes with no edit. A page whose shapes differ from the
/// file's model only where the two say one thing (a picture the drawing holds by an earlier file's names, which the file a model writer made numbers anew) was patched, to the bytes it had. An
/// `.odg` DiaCraft wrote from a person's `.vsdm`, saved, saved as `.diacraft-template` and then as `.vstm`: the Save after it is no edit and the same bytes, with the clock moved on between.
#[test]
fn class_s_a_patch_that_changes_no_bytes_is_no_edit() {
    let files = Files::new();
    let dir = &files.dir;
    let mut s = files.open(Start::OdgMade, "s").unwrap();
    s.execute("file.save", &json!({})).unwrap();
    written_as(&mut s, dir, "s.diacraft-template", json!({ "drop_objects": true, "drop_macros": true }), "s");
    let first = written_as(&mut s, dir, "s.vstm", json!({}), "s");
    // (The time a save would stamp is not the time of the Save As: an edit would show.)
    s.pin_clock(Some(crate::native_common::PINNED + 86_400));
    saved_again(&mut s, dir, "s.vstm", &first).unwrap_or_else(|why| panic!("{why}"));
}
