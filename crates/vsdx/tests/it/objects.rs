//! **ODF objects carried back become VSDX embeddings** (the native format's design 4.5 and
//! Owner 9; `crates/vsdx/src/write/objects.rs` says what the writers do).
//!
//! An object frame of a drawing that came from an `.odg` is a shape that shows its replacement picture and keeps the frame's `draw:object-ole` element and the object's file (an entry of the
//! package, held as a part of the model) beside it. Saved as a VSDX type, a frame that R25 calls *carried back* is written as a real embedding: `embeddings/oleObjectN.bin`, its relationship
//! part with the one `image` row to the picture, a page relationship of the type `oleObject`, and a shape whose `ForeignData` says `Object`. Any other frame rides in DiaCraft's
//! solution part. **Every file here is written by the real writers and read back by the real reader**; the models are built by hand the way the `.odg` reader builds them.
//!
//! **Three writers meet an object frame**, and each test that is about the file runs every one: the writer of a drawing that was never saved (`Skeleton`: the skeleton, patched), the model
//! writer (`Model`: a drawing that holds a package, written from the model alone) and the save into a file (`Splice`). Each makes the part of the page the frame is on, so each makes the rows
//! its shapes name (`write/page_rows.rs`).
//!
//! The crafted inputs the task names are here, each counted in the units the writer's loops walk (`diacraft_vsdx::work`) with a bound near the measured value, and timed beside a yardstick of
//! plain content in an `#[ignore]` test: an object whose replacement picture is missing, an object whose file is a directory of entries, two frames naming one object, an object one byte over the
//! part cap, and 10,000 small objects.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, Format, Kept, Media, Part, PartRole, Passthrough, RawAttrs, RawElement, RawNode, Shape, ShapeType, Value};
use diacraft_opc::name::fold;
use diacraft_testkit::package::{OD, PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{CT_OLE, NOW, package};
use crate::read_ok;

// ------------------------------------------------------------------------------------------------------------------------------- the inputs

/// The class V1 of the spec invents.
const CLASS: [u8; 16] = [0xDE, 0xC0, 0xA0, 0xD1, 0x01, 0x00, 0x00, 0x40, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBE, 0xEF];
const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
const PNG_B: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 5, 6, 7, 8];

/// V1 of the spec: 1,536 bytes, all zero but the header's signature, version, byte order mark, sector shift and first directory sector, and the root entry's type and class.
fn compound(class: [u8; 16]) -> Vec<u8> {
    let mut bytes = vec![0u8; 1536];
    bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
    bytes[1024 + 0x42] = 5;
    bytes[1024 + 0x50..1024 + 0x60].copy_from_slice(&class);
    bytes
}

fn element(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children })
}

/// An entry of an `.odg` package as the reader holds it: a part of the format `Odf`, typed by its manifest row.
fn entry(name: &str, media: &str, bytes: &[u8]) -> Arc<Part> {
    // The role the `.odg` reader gives an entry by its name: a file of a folder or the folder is an object's, an entry named `Object <n>` (an object's own file) is no role.
    let role = if name.starts_with("Object ") && name.contains('/') { PartRole::ObjectFile } else { PartRole::Other };
    Arc::new(Part {
        format: Format::Odf,
        name: Arc::from(name),
        media_type: Arc::from(media),
        role,
        manifest: RawAttrs::from_pairs([("manifest:version", "1.3")]),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

/// The replacement picture of a frame as the reader holds it: the entry's name for the part, and the manifest's type or the one its bytes say.
fn replacement(entry_name: &str, bytes: &[u8]) -> Arc<Media> {
    Arc::new(Media { part: Arc::from(entry_name), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })
}

/// What a frame says of its object: the `draw:object-ole` element the `.odg` reader keeps beside the shape, with the three link attributes LibreOffice writes.
fn object_element(href: &str, class: Option<&str>) -> RawNode {
    let mut attrs = vec![("xlink:href", href), ("xlink:type", "simple"), ("xlink:show", "embed"), ("xlink:actuate", "onLoad")];
    if let Some(class) = class {
        attrs.push(("draw:class-id", class));
    }
    element("draw:object-ole", &attrs, Vec::new())
}

/// An object frame of 4 cm by 2 cm: a `Foreign` shape that shows `picture` and keeps `object` as the frame's own element, with the automatic style name `gr1`.
fn frame(id: u32, object: RawNode, picture: Option<&Arc<Media>>) -> Shape {
    let foreign = diacraft_doc::ForeignData {
        attrs: if picture.is_some() { RawAttrs::from_pairs([("ForeignType", "Bitmap")]) } else { RawAttrs::new() },
        picture: picture.cloned(),
        ..diacraft_doc::ForeignData::default()
    };
    let mut shape = Shape { id, kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() };
    let sheet = Arc::make_mut(&mut shape.sheet);
    // The picture's box is the frame's (the four cells a program that draws a `Foreign` shape reads: the whole shape, from its own corner).
    for (name, value) in [
        ("PinX", "0.7874015748031497"),
        ("PinY", "0.3937007874015748"),
        ("Width", "1.5748031496062993"),
        ("Height", "0.7874015748031497"),
        ("ImgOffsetX", "0"),
        ("ImgOffsetY", "0"),
        ("ImgWidth", "1.5748031496062993"),
        ("ImgHeight", "0.7874015748031497"),
    ] {
        sheet.set_cell(&CellAddr::direct(name), diacraft_doc::Cell::constant(Value::from_text(value))).unwrap();
    }
    let slot = shape.ext.make_mut();
    slot.odf.style_name = Some(Arc::from("gr1"));
    slot.odf.kept = Kept { raw: RawAttrs::from_pairs([("draw:layer", "layout")]), kept: Passthrough::new() };
    slot.odf.kept.kept.push(0, object);
    shape
}

/// The frame V6 describes: the object `./Object 3` and the picture `./ObjectReplacements/Object 3`.
fn frame_6(id: u32) -> (Shape, Vec<Arc<Part>>) {
    let picture = replacement("ObjectReplacements/Object 3", &PNG);
    (frame(id, object_element("./Object 3", None), Some(&picture)), vec![entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS))])
}

// -------------------------------------------------------------------------------------------------------------------------------- the paths

/// Which writer makes the part of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    /// A drawing that was never saved: the skeleton, patched.
    Skeleton,
    /// A drawing that holds a package, written from the model alone.
    Model,
    /// A drawing saved into the file it was read from.
    Splice,
}

const PATHS: [Path; 3] = [Path::Skeleton, Path::Model, Path::Splice];
const PAGE_NAME: &str = "Objects";

/// The package a drawing read from a file starts as: one page with a box.
fn base() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "One").shapes(&shape(1, &cell("PinX", "1")))).build()
}

/// A drawing the writer `path` writes, holding `frames` on a page of its own (a page with no part yet, whose part the save makes) and `parts` of the other format.
fn world(path: Path, frames: Vec<Shape>, parts: Vec<Arc<Part>>) -> (Document, Vec<u8>) {
    let original = base();
    let mut doc = match path {
        Path::Skeleton => Document::new_blank(),
        Path::Model | Path::Splice => read_ok(&original).0,
    };
    let id = doc.add_page(Some(PAGE_NAME)).unwrap();
    let at = doc.pages.iter().position(|p| p.id == id).unwrap();
    let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
    for shape in frames {
        page.shapes.push(Arc::new(shape));
    }
    for part in parts {
        doc.parts.push(part);
    }
    (doc, original)
}

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn try_save(path: Path, doc: &Document, original: &[u8], opts: &WriteOptions) -> Result<Written, diacraft_vsdx::WriteError> {
    match path {
        Path::Skeleton | Path::Model => write_with(doc, None, opts),
        Path::Splice => write_with(doc, Some(&OriginalBytes { bytes: original, baseline: None }), opts),
    }
}

fn save(path: Path, doc: &Document, original: &[u8]) -> Written {
    try_save(path, doc, original, &options()).unwrap_or_else(|e| panic!("{path:?}: the save refused: {e} ({})", e.class()))
}

// ------------------------------------------------------------------------------------------------------------------------------ what a file holds

/// The parts of the package under `embeddings/`, and which hold `content`.
fn embeddings(bytes: &[u8]) -> Vec<String> {
    let pkg = package(bytes);
    let mut names: Vec<String> = pkg.parts().filter(|n| n.contains("/embeddings/")).map(str::to_owned).collect();
    names.sort();
    names
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    package(bytes).part(name).unwrap_or_else(|| panic!("the file has no part {name}")).to_vec()
}

fn ole_rel() -> String {
    format!("{OD}oleObject")
}

fn image_rel() -> String {
    format!("{OD}image")
}

/// The rows of the relationships of `source`: `(Id, type, the part it leads to)`.
fn rows(bytes: &[u8], source: &str) -> Vec<(String, String, Option<String>)> {
    package(bytes).rels(source).unwrap().items.iter().map(|r| (r.id.clone(), r.ty.clone(), r.target.clone())).collect()
}

/// The page the frames are on, read back.
fn page_of(doc: &Document) -> &diacraft_doc::Page {
    doc.pages.iter().find(|p| &*p.name == PAGE_NAME).unwrap_or_else(|| panic!("no page named {PAGE_NAME}"))
}

/// The `ForeignData` of shape `id` of the page, read back: its attributes, its `Rel`'s id and the bytes of the picture it shows.
type Shown = (Vec<(String, String)>, Option<String>, Option<Vec<u8>>);

fn foreign_of(doc: &Document, id: u32) -> Shown {
    let page = page_of(doc);
    let shape = page.shapes.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}"));
    assert_eq!(shape.kind, ShapeType::Foreign, "shape {id} is a Foreign shape");
    let f = shape.foreign.as_ref().unwrap_or_else(|| panic!("shape {id} has no ForeignData"));
    (
        f.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        f.rel_id.as_deref().map(str::to_owned),
        f.picture.as_ref().map(|m| m.bytes.to_vec()),
    )
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// The part of the file that the page's relationship `id` leads to, and its type.
fn page_row(bytes: &[u8], doc: &Document, id: &str) -> (String, Option<String>) {
    let part = page_of(doc).part_name.clone().unwrap();
    let rows = rows(bytes, &part);
    let row = rows.iter().find(|(i, _, _)| i == id).unwrap_or_else(|| panic!("{part} has no row {id}: {rows:?}"));
    (row.1.clone(), row.2.clone())
}

// ------------------------------------------------------------------------------------------------------------------------ V6: the embedding

/// A frame whose object is a compound file and whose replacement is a PNG is written as `embeddings/oleObject1.bin` with the object's bytes, a `Default` for `bin` of the object type, a
/// page relationship of the type `oleObject`, a PNG picture part, the object part's own relationship part with the one `image` relationship to the picture, and a `Foreign` shape whose
/// `ForeignData` says `Object` with the defaults: `ObjectType` 33280, `ShowAsIcon` 0, and `ObjectWidth` and `ObjectHeight` written as the shape's `Width` and `Height` are.
#[test]
fn an_object_frame_is_written_as_an_embedding_with_its_picture_on_the_object_part() {
    for path in PATHS {
        let (shape, parts) = frame_6(7);
        let (doc, original) = world(path, vec![shape], parts);
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        let bytes = &written.bytes;
        // The object part, with the object's bytes unchanged, and its relationship part.
        assert_eq!(embeddings(bytes), ["/visio/embeddings/_rels/oleObject1.bin.rels", "/visio/embeddings/oleObject1.bin"], "{case}");
        assert_eq!(part_bytes(bytes, "/visio/embeddings/oleObject1.bin"), compound(CLASS), "{case}: the bytes are the object's");
        // Its type is that of a `Default` for `bin` (added when there was none).
        let pkg = package(bytes);
        let types = pkg.content_types();
        assert!(types.defaults.iter().any(|(e, t)| e == "bin" && t == CT_OLE), "{case}: a Default for bin of the object type: {:?}", types.defaults);
        assert!(!types.overrides.iter().any(|(p, _)| p.eq_ignore_ascii_case("/visio/embeddings/oleObject1.bin")), "{case}: no Override of its own");
        assert_eq!(pkg.content_type("/visio/embeddings/oleObject1.bin"), Some(CT_OLE), "{case}");
        // The picture part, and the one relationship of the object part: to the picture.
        let object_rows = rows(bytes, "/visio/embeddings/oleObject1.bin");
        assert_eq!(object_rows.len(), 1, "{case}: {object_rows:?}");
        assert_eq!(object_rows[0].1, image_rel(), "{case}");
        let picture = object_rows[0].2.clone().unwrap();
        assert_eq!(part_bytes(bytes, &picture), PNG, "{case}: the picture's bytes");
        assert!(picture.starts_with("/visio/media/image") && picture.ends_with(".png"), "{case}: named as the picture writer names one: {picture}");
        assert_eq!(pkg.content_type(&picture), Some("image/png"), "{case}");
        // The shape names a relationship of the page of the type `oleObject`, and the page has no `image` row to the picture: the fallback hangs off the object part.
        let (back, report) = read_ok(bytes);
        assert_eq!(report.warning_count("picture_unresolved"), 0, "{case}: {:?}", report.warnings);
        let (attrs, rel, shown) = foreign_of(&back, 7);
        let rel = rel.unwrap_or_else(|| panic!("{case}: the shape names no relationship"));
        assert_eq!(page_row(bytes, &back, &rel), (ole_rel(), Some("/visio/embeddings/oleObject1.bin".to_owned())), "{case}");
        let page_rows = rows(bytes, page_of(&back).part_name.as_deref().unwrap());
        assert!(!page_rows.iter().any(|(_, ty, _)| *ty == image_rel()), "{case}: no image row on the page: {page_rows:?}");
        assert_eq!(shown, Some(PNG.to_vec()), "{case}: the shape shows the picture through the object's own row");
        assert_eq!(
            attrs,
            pairs(&[
                ("ForeignType", "Object"),
                ("ObjectType", "33280"),
                ("ShowAsIcon", "0"),
                ("ObjectWidth", "1.5748031496062993"),
                ("ObjectHeight", "0.7874015748031497")
            ]),
            "{case}: the attributes, in the order every object shape of the sample files hold them"
        );
        // The object comes back as the part the file holds, with its own relationship: no object file rides as a part of DiaCraft's own.
        assert!(
            back.parts.iter().any(|p| p.format == Format::Vsdx && &*p.name == "/visio/embeddings/oleObject1.bin" && p.role == PartRole::Embedding),
            "{case}"
        );
        assert!(
            !back.parts.iter().any(|p| p.format == Format::Odf),
            "{case}: the object is the embedding, and rides nowhere else: {:?}",
            back.parts.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}

/// A package that already has `Default Extension="bin" ContentType="application/octet-stream"` keeps that `Default`, and the object part is typed by an `Override`.
#[test]
fn a_default_for_bin_of_another_type_is_kept_and_the_object_part_gets_an_override() {
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "One").shapes(&shape(1, &cell("PinX", "1"))))
        .with(
            diacraft_testkit::package::Pkg::new()
                .default_type("bin", "application/octet-stream")
                .part_without_override("/visio/other.bin", [1u8, 2, 3]),
        )
        .build();
    for path in [Path::Model, Path::Splice] {
        let mut doc = read_ok(&original).0;
        let id = doc.add_page(Some(PAGE_NAME)).unwrap();
        let (shape, parts) = frame_6(7);
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        Arc::make_mut(doc.pages.get_mut(at).unwrap()).shapes.push(Arc::new(shape));
        for part in parts {
            doc.parts.push(part);
        }
        let written = save(path, &doc, &original);
        let pkg = package(&written.bytes);
        let types = pkg.content_types();
        let case = format!("{path:?}");
        assert!(
            types.defaults.iter().any(|(e, t)| e == "bin" && t == "application/octet-stream"),
            "{case}: the Default is kept: {:?}",
            types.defaults
        );
        assert!(
            types.overrides.iter().any(|(p, t)| p.eq_ignore_ascii_case("/visio/embeddings/oleObject1.bin") && t == CT_OLE),
            "{case}: the object part has an Override: {:?}",
            types.overrides
        );
        assert_eq!(pkg.content_type("/visio/embeddings/oleObject1.bin"), Some(CT_OLE), "{case}");
        assert_eq!(pkg.content_type("/visio/other.bin"), Some("application/octet-stream"), "{case}: other .bin parts are typed as they were");
    }
}

/// N is the lowest number above every `oleObject` part in use.
#[test]
fn the_number_is_the_lowest_above_every_object_part_in_use() {
    let pkg = diacraft_testkit::package::Pkg::new()
        .part("/visio/embeddings/oleObject1.bin", CT_OLE, [1u8])
        .part("/visio/embeddings/OLEOBJECT3.bin", CT_OLE, [3u8])
        .rel("/visio/pages/page1.xml", "rId1", &ole_rel(), "../embeddings/oleObject1.bin")
        .rel("/visio/pages/page1.xml", "rId2", &ole_rel(), "../embeddings/OLEOBJECT3.bin");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "One").shapes(&shape(1, &cell("PinX", "1")))).with(pkg).build();
    for path in [Path::Model, Path::Splice] {
        let mut doc = read_ok(&original).0;
        let id = doc.add_page(Some(PAGE_NAME)).unwrap();
        let (shape, parts) = frame_6(7);
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        Arc::make_mut(doc.pages.get_mut(at).unwrap()).shapes.push(Arc::new(shape));
        for part in parts {
            doc.parts.push(part);
        }
        let written = save(path, &doc, &original);
        let names: BTreeSet<String> = embeddings(&written.bytes).into_iter().filter(|n| !n.contains("_rels")).map(|n| fold(&n)).collect();
        assert_eq!(
            names,
            BTreeSet::from([
                "/visio/embeddings/oleobject1.bin".to_owned(),
                "/visio/embeddings/oleobject3.bin".to_owned(),
                "/visio/embeddings/oleobject4.bin".to_owned()
            ]),
            "{path:?}: the new object is oleObject4"
        );
    }
}

// ------------------------------------------------------------------------------------------------------------- V5: the attributes that rode

/// A frame whose `ForeignData` holds the attributes that rode (in 0.2, the frame's `foreign` item) is written with exactly those, in their order, after `ForeignType="Object"`. A writer that
/// ignores them writes the defaults and fails. The same frame without them (after a LibreOffice save drops them) gets the defaults, which is then right.
#[test]
fn the_attributes_that_rode_are_written_in_their_order() {
    for path in PATHS {
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        let mut shape = frame(7, object_element("./Object 3", None), Some(&picture));
        let mut foreign = (**shape.foreign.as_ref().unwrap()).clone();
        foreign.attrs = RawAttrs::from_pairs([
            ("ForeignType", "Bitmap"),
            ("ObjectType", "33280"),
            ("ShowAsIcon", "1"),
            ("ObjectWidth", "2.25"),
            ("ObjectHeight", "0.75"),
        ]);
        shape.foreign = Some(Arc::new(foreign));
        let (doc, original) = world(path, vec![shape], vec![entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS))]);
        let written = save(path, &doc, &original);
        let (back, _) = read_ok(&written.bytes);
        let (attrs, _, _) = foreign_of(&back, 7);
        assert_eq!(
            attrs,
            pairs(&[("ForeignType", "Object"), ("ObjectType", "33280"), ("ShowAsIcon", "1"), ("ObjectWidth", "2.25"), ("ObjectHeight", "0.75")]),
            "{path:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- V12: the class

/// A frame with `draw:class-id` in braces writes the compound file's bytes unchanged (its class is `D1A0C0DE-...`): the attribute is not read, and the class is never rewritten from it.
#[test]
fn the_class_attribute_is_not_read_and_the_object_is_written_as_it_is() {
    for path in PATHS {
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        let shape = frame(7, object_element("./Object 3", Some("{0BADC1A5-0002-4000-8000-000000000000}")), Some(&picture));
        let (doc, original) = world(path, vec![shape], vec![entry("Object 3", "application/octet-stream", &compound(CLASS))]);
        let written = save(path, &doc, &original);
        assert_eq!(part_bytes(&written.bytes, "/visio/embeddings/oleObject1.bin"), compound(CLASS), "{path:?}");
    }
}

// ------------------------------------------------------------------------------------------------------------------- more of what a file holds

/// The parts of the other format that a model holds, by name: `(name, media type, role, manifest attributes, bytes)`.
type OdfPart = (String, String, PartRole, Vec<(String, String)>, Vec<u8>);

fn odf_parts_of(doc: &Document) -> Vec<OdfPart> {
    let mut out: Vec<OdfPart> = doc
        .parts
        .iter()
        .filter(|p| p.format == Format::Odf)
        .map(|p| {
            (
                p.name.to_string(),
                p.media_type.to_string(),
                p.role,
                p.manifest.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<Vec<_>>(),
                p.bytes.to_vec(),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The parts of DiaCraft's own in a file: `/diacraft/...`.
fn blob_parts(bytes: &[u8]) -> Vec<String> {
    let pkg = package(bytes);
    let mut names: Vec<String> = pkg.parts().filter(|n| n.starts_with("/diacraft/")).map(str::to_owned).collect();
    names.sort();
    names
}

/// The bytes of the embedding the shape's `Rel` names, found as a reader's model holds it: the row of the page's part (`Document::vsdx`), the part it leads to (`Document::parts`).
fn embedding_of(doc: &Document, id: u32) -> Option<Vec<u8>> {
    let page = page_of(doc);
    let part = page.part_name.as_deref()?;
    let rel = page.shapes.iter().find(|s| s.id == id)?.foreign.as_ref()?.rel_id.clone()?;
    let held = doc.vsdx.as_ref()?;
    let row = held.rels.iter().find(|r| &*r.source == part && r.id.eq_ignore_ascii_case(&rel) && !r.external)?;
    let target = diacraft_opc::name::resolve(part, &row.target)?;
    doc.parts.iter().find(|p| p.format == Format::Vsdx && fold(&p.name) == fold(&target)).map(|p| p.bytes.to_vec())
}

/// The standard base64 alphabet with padding, in lines of 76 characters each indented by four spaces.
fn base64_lines(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                text.push('=');
            }
        }
    }
    let mut out = String::new();
    for line in text.as_bytes().chunks(76) {
        out.push_str("\n    ");
        out.push_str(std::str::from_utf8(line).unwrap());
    }
    out.push('\n');
    out
}

/// An `office:binary-data` element holding `bytes` as a flat file writes them.
fn inline_element(bytes: &[u8]) -> RawElement {
    RawElement {
        name: Arc::from("office:binary-data"),
        attrs: RawAttrs::new(),
        children: vec![RawNode::Text(Arc::from(base64_lines(bytes).as_str()))],
    }
}

fn with_children(node: RawNode, children: Vec<RawNode>) -> RawNode {
    match node {
        RawNode::Element(mut el) => {
            el.children = children;
            RawNode::Element(el)
        }
        other => other,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------: the rest ride

type Case = (&'static str, Shape, Vec<Arc<Part>>, usize);

/// Each of these turns V6 into a frame that is not carried back, so no `embeddings/` part is written for it. The object's file rides in a part of DiaCraft's own and comes back byte for byte.
#[test]
fn a_frame_that_is_not_carried_back_writes_no_embedding_and_its_object_rides() {
    let ole = compound(CLASS);
    let png = |entry: &str| replacement(entry, &PNG);
    let cases: Vec<Case> = vec![
        (
            "a replacement whose first bytes are NOTAPICT",
            frame(7, object_element("./Object 3", None), Some(&replacement("ObjectReplacements/Object 3", b"NOTAPICT bytes"))),
            vec![entry("Object 3", "application/vnd.sun.star.oleobject", &ole)],
            1,
        ),
        (
            "a replacement href naming an entry the package lacks (no picture)",
            frame(7, object_element("./Object 3", None), None),
            vec![entry("Object 3", "application/vnd.sun.star.oleobject", &ole)],
            1,
        ),
        (
            "an object whose bytes start PK",
            frame(7, object_element("./Object 3", None), Some(&png("ObjectReplacements/Object 3"))),
            vec![entry("Object 3", "application/vnd.sun.star.oleobject", b"PK\x03\x04 a package, not a compound file")],
            1,
        ),
        (
            "a draw:object naming a formula folder",
            frame(
                7,
                element("draw:object", &[("xlink:href", "./Object 2"), ("xlink:type", "simple")], Vec::new()),
                Some(&png("ObjectReplacements/Object 2")),
            ),
            vec![
                entry("Object 2/", "application/vnd.oasis.opendocument.formula", b""),
                entry("Object 2/content.xml", "text/xml", b"<math/>"),
                entry("Object 2/styles.xml", "text/xml", b"<styles/>"),
            ],
            3,
        ),
        (
            "an object href that names nothing in the package",
            frame(7, object_element("../outside.bin", None), Some(&png("ObjectReplacements/Object 3"))),
            Vec::new(),
            0,
        ),
        (
            "an object that is a folder of entries (a package file)",
            frame(7, object_element("./Object 3", None), Some(&png("ObjectReplacements/Object 3"))),
            vec![entry("Object 3/", "", b""), entry("Object 3/content.xml", "text/xml", b"<office:document-content/>")],
            2,
        ),
    ];
    for (what, shape, parts, riding) in cases {
        for path in PATHS {
            let before = {
                let (doc, _) = world(path, Vec::new(), parts.clone());
                odf_parts_of(&doc)
            };
            let (doc, original) = world(path, vec![shape.clone()], parts.clone());
            let written = save(path, &doc, &original);
            let case = format!("{what} ({path:?})");
            assert!(embeddings(&written.bytes).is_empty(), "{case}: no embedding is written: {:?}", embeddings(&written.bytes));
            let (back, report) = read_ok(&written.bytes);
            assert_eq!(report.warning_count("picture_unresolved"), 0, "{case}: {:?}", report.warnings);
            assert_eq!(odf_parts_of(&back), before, "{case}: the object's files come back as the drawing held them, byte for byte");
            assert_eq!(before.len(), riding, "{case}");
            // The frame's own element is back where it was (the item is fresh).
            assert_eq!(report.carry.stale, 0, "{case}: {:?}", report.carry);
            let kept = page_of(&back).shapes.iter().find(|s| s.id == 7).unwrap().ext.get().map(|e| e.odf.kept.kept.len());
            assert_eq!(kept, Some(1), "{case}: the frame's own element is back");
        }
    }
}

/// The plan's test: an object that cannot go natively rides in DiaCraft's own parts, its shape shows the replacement from the picture writer's part, and everything comes back byte for byte with its
/// entry name, media type and manifest attributes. No part is named as an `.odg` entry is (a space is no part name's), and the replacement is never held twice.
#[test]
fn an_object_that_cannot_go_natively_rides_and_comes_back_byte_for_byte() {
    let files = || {
        vec![
            entry("Object 2/", "application/vnd.oasis.opendocument.formula", b""),
            entry("Object 2/content.xml", "text/xml", b"<math xmlns=\"urn:x\"><mi>x</mi></math>"),
            entry("Object 2/styles.xml", "text/xml", b"<styles/>"),
        ]
    };
    for path in PATHS {
        let png = replacement("ObjectReplacements/Object 2", &PNG);
        let shape = frame(7, element("draw:object", &[("xlink:href", "./Object 2"), ("xlink:type", "simple")], Vec::new()), Some(&png));
        let (doc, original) = world(path, vec![shape], files());
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        // The files ride as parts of DiaCraft's own, named as the format names them.
        let blobs: Vec<String> = blob_parts(&written.bytes).into_iter().filter(|n| !n.contains("_rels")).collect();
        assert!(!blobs.is_empty(), "{case}");
        for name in &blobs {
            let ok = name.strip_prefix("/diacraft/blob").is_some_and(|rest| rest.contains('.') && !rest.contains(' ') && !rest.contains('/'));
            assert!(ok, "{case}: {name} is no part name of DiaCraft's");
        }
        assert!(!blobs.iter().any(|n| part_bytes(&written.bytes, n) == PNG), "{case}: the replacement is not held a second time");
        assert!(embeddings(&written.bytes).is_empty(), "{case}");
        // The shape shows the PNG from the picture writer's part.
        let (back, report) = read_ok(&written.bytes);
        let (attrs, rel, shown) = foreign_of(&back, 7);
        assert_eq!(shown, Some(PNG.to_vec()), "{case}");
        assert_eq!(attrs.first().map(|(k, v)| (k.as_str(), v.as_str())), Some(("ForeignType", "Bitmap")), "{case}");
        assert_eq!(page_row(&written.bytes, &back, &rel.unwrap()).0, image_rel(), "{case}");
        // Every file, with its entry name, media type, role and manifest attributes, as the drawing held it.
        let (before, _) = world(path, Vec::new(), files());
        assert_eq!(odf_parts_of(&back), odf_parts_of(&before), "{case}");
        assert_eq!(report.carry.stale, 0, "{case}: the shape's item is fresh: {:?}", report.carry);
    }
}

/// A frame carried back leaves nothing of its object in the carrier (no part of DiaCraft's own, no element that names the entry), and the rest of what rides with the frame (its style name, the
/// attributes it kept) comes back on the shape the file holds.
#[test]
fn a_frame_carried_back_leaves_nothing_of_its_object_in_the_carrier_and_the_rest_of_its_data_comes_back() {
    for path in PATHS {
        let (shape, parts) = frame_6(7);
        let (doc, original) = world(path, vec![shape], parts);
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        assert!(blob_parts(&written.bytes).is_empty(), "{case}: nothing rides as a part of DiaCraft's own: {:?}", blob_parts(&written.bytes));
        let solution = package(&written.bytes)
            .parts()
            .find(|n| n.contains("/solutions/solution") && !n.ends_with("/solutions.xml") && !n.contains("_rels"))
            .map(str::to_owned)
            .unwrap();
        let text = String::from_utf8(part_bytes(&written.bytes, &solution)).unwrap();
        assert!(!text.contains("object-ole") && !text.contains("Object 3"), "{case}: {text}");
        assert!(text.contains("gr1"), "{case}: the rest of the frame's data rides: {text}");
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(report.carry.stale, 0, "{case}: the item is fresh although the shape is written as an object: {:?}", report.carry);
        let shape = page_of(&back).shapes.iter().find(|s| s.id == 7).unwrap().clone();
        let odf = &shape.ext.get().expect("the shape's slot").odf;
        assert_eq!(odf.style_name.as_deref(), Some("gr1"), "{case}");
        assert_eq!(odf.kept.raw.get("draw:layer"), Some("layout"), "{case}");
        assert_eq!(odf.kept.kept.len(), 0, "{case}: no element that names an entry that is not there");
    }
}

/// A part that a frame which is carried back names, and a frame which is not names too, rides beside the embedding: the second frame comes back whole.
#[test]
fn a_part_that_a_frame_which_rides_names_too_rides_beside_the_embedding() {
    for path in PATHS {
        let good = replacement("ObjectReplacements/Object 3", &PNG);
        let bad = replacement("ObjectReplacements/Object 4", b"NOTAPICT");
        let a = frame(7, object_element("./Object 3", None), Some(&good));
        let b = frame(8, object_element("./Object 3", None), Some(&bad));
        let (doc, original) = world(path, vec![a, b], vec![entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS))]);
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        assert_eq!(embeddings(&written.bytes).len(), 2, "{case}: one embedding, for the frame carried back");
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(odf_parts_of(&back).len(), 1, "{case}: the entry rides for the frame that rides");
        assert_eq!(odf_parts_of(&back)[0].4, compound(CLASS), "{case}");
        assert_eq!(report.carry.stale, 0, "{case}: {:?}", report.carry);
        let rides = page_of(&back).shapes.iter().find(|s| s.id == 8).unwrap().ext.get().map(|e| e.odf.kept.kept.len());
        assert_eq!(rides, Some(1), "{case}: its element names the entry that rides");
        assert_eq!(embedding_of(&back, 7), Some(compound(CLASS)), "{case}: and the first is the embedding");
    }
}

/// The object written inline, as the bytes in base64 in lines of 76 characters each indented by four spaces, gives the parts and bytes. An href beside the data is ignored. The decode is counted.
#[test]
fn an_inline_object_is_decoded_once_and_gives_the_same_embedding() {
    for path in PATHS {
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        // The element has an href as well as the data: the href is ignored, and the entry it names is not even in the package.
        let object = with_children(object_element("./Object 3", None), vec![RawNode::Element(inline_element(&compound(CLASS)))]);
        let shape = frame(7, object, Some(&picture));
        let (doc, original) = world(path, vec![shape], Vec::new());
        diacraft_vsdx::work::reset();
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        assert_eq!(part_bytes(&written.bytes, "/visio/embeddings/oleObject1.bin"), compound(CLASS), "{case}");
        assert_eq!(diacraft_vsdx::work::object_bytes_decoded(), 1536, "{case}: decoded once");
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(embedding_of(&back, 7), Some(compound(CLASS)), "{case}");
        assert_eq!(foreign_of(&back, 7).0.first().map(|(_, v)| v.as_str()), Some("Object"), "{case}");
    }
}

/// Inline data that is no compound file, or no base64, is not carried back: it is judged from its characters and decoded no further, and it rides as the shape's own markup.
#[test]
fn inline_data_that_is_no_compound_file_or_no_base64_is_not_carried_back() {
    for path in PATHS {
        for (what, text) in
            [("not a compound file", base64_lines(b"PK\x03\x04 not a compound file at all")), ("not base64", "\n    ***not base64***\n".to_owned())]
        {
            let picture = replacement("ObjectReplacements/Object 3", &PNG);
            let data =
                RawElement { name: Arc::from("office:binary-data"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(text.as_str()))] };
            let shape = frame(7, with_children(object_element("./Object 3", None), vec![RawNode::Element(data)]), Some(&picture));
            let (doc, original) = world(path, vec![shape], Vec::new());
            let written = save(path, &doc, &original);
            assert!(embeddings(&written.bytes).is_empty(), "{path:?}: {what}");
        }
    }
}

/// Two frames that name one object and show one picture share one embedding: one object part, one picture part, one row of the page, and both shapes name it. Two frames that show two pictures
/// are two embeddings of the same bytes, each with its own picture.
#[test]
fn frames_of_one_object_share_an_embedding_when_they_show_one_picture_and_are_two_when_they_show_two() {
    for path in PATHS {
        let object = entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS));
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        let a = frame(7, object_element("./Object 3", None), Some(&picture));
        let b = frame(8, object_element("./Object 3", None), Some(&picture));
        let (doc, original) = world(path, vec![a, b], vec![Arc::clone(&object)]);
        let written = save(path, &doc, &original);
        let case = format!("{path:?}, one picture");
        assert_eq!(embeddings(&written.bytes), ["/visio/embeddings/_rels/oleObject1.bin.rels", "/visio/embeddings/oleObject1.bin"], "{case}");
        let (back, _) = read_ok(&written.bytes);
        let (rel_a, rel_b) = (foreign_of(&back, 7).1.unwrap(), foreign_of(&back, 8).1.unwrap());
        assert_eq!(rel_a, rel_b, "{case}: both shapes name one row");
        let part = page_of(&back).part_name.clone().unwrap();
        let object_rows: Vec<_> = rows(&written.bytes, &part).into_iter().filter(|(_, ty, _)| *ty == ole_rel()).collect();
        assert_eq!(object_rows.len(), 1, "{case}: {object_rows:?}");
        // Two pictures.
        let other = replacement("ObjectReplacements/Object 4", &PNG_B);
        let a = frame(7, object_element("./Object 3", None), Some(&picture));
        let b = frame(8, object_element("./Object 3", None), Some(&other));
        let (doc, original) = world(path, vec![a, b], vec![Arc::clone(&object)]);
        let written = save(path, &doc, &original);
        let case = format!("{path:?}, two pictures");
        let names: Vec<String> = embeddings(&written.bytes).into_iter().filter(|n| !n.contains("_rels")).collect();
        assert_eq!(names, ["/visio/embeddings/oleObject1.bin", "/visio/embeddings/oleObject2.bin"], "{case}");
        assert_eq!(part_bytes(&written.bytes, &names[0]), part_bytes(&written.bytes, &names[1]), "{case}: the same bytes");
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(foreign_of(&back, 7).2, Some(PNG.to_vec()), "{case}");
        assert_eq!(foreign_of(&back, 8).2, Some(PNG_B.to_vec()), "{case}: each shape shows its own picture");
        assert_ne!(foreign_of(&back, 7).1, foreign_of(&back, 8).1, "{case}");
    }
}

/// An object in a group is carried back (the group's members are shapes of the page), one on a master rides: an `.odg` has no master-level object, and the page relationship of R26 has no master's.
#[test]
fn an_object_in_a_group_is_carried_back_and_one_on_a_master_rides() {
    // The master's frame is put on a master the file already has, which a save into the file writes without a picture (a shape put on a part the file has is not given the rows a new part's shapes get,
    // as for any picture: gaps F11), so the master is the model writer's case; the group on the new page is every writer's.
    for path in [Path::Model, Path::Splice] {
        let (member, parts) = frame_6(9);
        let mut group = Shape { id: 7, kind: ShapeType::Group, ..Shape::default() };
        group.children.push(Arc::new(member));
        let original = VsdxBuilder::new()
            .master(diacraft_testkit::package::MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1"))))
            .page(PageSpec::new(1, "One").shapes(&shape(1, &cell("PinX", "1"))))
            .build();
        let mut doc = read_ok(&original).0;
        let id = doc.add_page(Some(PAGE_NAME)).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        Arc::make_mut(doc.pages.get_mut(at).unwrap()).shapes.push(Arc::new(group));
        // The master's frame names another object entry.
        let picture = replacement("ObjectReplacements/Object 8", &PNG_B);
        let on_master = frame(5, object_element("./Object 8", None), Some(&picture));
        if path == Path::Model {
            Arc::make_mut(doc.masters.get_mut(0).unwrap()).shapes.push(Arc::new(on_master));
        }
        for part in parts {
            doc.parts.push(part);
        }
        doc.parts.push(entry("Object 8", "application/vnd.sun.star.oleobject", &compound(CLASS)));
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        assert_eq!(
            embeddings(&written.bytes),
            ["/visio/embeddings/_rels/oleObject1.bin.rels", "/visio/embeddings/oleObject1.bin"],
            "{case}: one embedding, the group's member"
        );
        let (back, report) = read_ok(&written.bytes);
        let group = page_of(&back).shapes.iter().find(|s| s.id == 7).unwrap().clone();
        let member = group.children.iter().find(|s| s.id == 9).unwrap().clone();
        assert_eq!(member.foreign.as_ref().and_then(|f| f.attr("ForeignType")), Some("Object"), "{case}");
        assert!(member.foreign.as_ref().and_then(|f| f.rel_id.as_deref()).is_some(), "{case}");
        // The master's frame rides: its entry is a part of DiaCraft's own, and its element is on the shape. (An entry that no frame names rides too.)
        assert_eq!(odf_parts_of(&back).iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["Object 8"], "{case}");
        assert_eq!(report.carry.stale, 0, "{case}: {:?}", report.carry);
    }
}

/// A frame that names a macro project's entry is never written as an object, whatever its bytes are: a type that promises no macros refuses it as a macro project, and with the caller's choice to
/// leave macros out it is left out.
#[test]
fn a_frame_naming_a_macro_entry_is_never_written_natively() {
    let picture = replacement("ObjectReplacements/Object 3", &PNG);
    let shape = frame(7, object_element("./Basic/Standard/lib.bin", None), Some(&picture));
    let mut part = (*entry("Basic/Standard/lib.bin", "application/octet-stream", &compound(CLASS))).clone();
    part.role = PartRole::MacroProject;
    for path in PATHS {
        let (doc, original) = world(path, vec![shape.clone()], vec![Arc::new(part.clone())]);
        let refused = try_save(path, &doc, &original, &options()).unwrap_err();
        assert_eq!(refused, diacraft_vsdx::WriteError::MacroProject, "{path:?}");
        let dropped = try_save(path, &doc, &original, &WriteOptions { drop_macros: true, ..options() }).unwrap();
        assert!(embeddings(&dropped.bytes).is_empty(), "{path:?}: a macro project is not an embedding");
        assert_eq!(dropped.macros_dropped, ["Basic/Standard/lib.bin"], "{path:?}");
    }
}

/// The picture of a frame is typed by its bytes when the type the model claims for it is none a reader draws: the reader finds an object's fallback by its type, and a manifest row that says
/// `application/octet-stream` of a PNG would leave the object with no picture to show.
#[test]
fn the_picture_of_an_object_is_typed_by_its_bytes_when_the_claimed_type_is_none_a_reader_draws() {
    for path in PATHS {
        let picture = Arc::new(Media {
            part: Arc::from("ObjectReplacements/Object 3"),
            content_type: Arc::from("application/octet-stream"),
            bytes: Arc::from(&PNG[..]),
        });
        let shape = frame(7, object_element("./Object 3", None), Some(&picture));
        let (doc, original) = world(path, vec![shape], vec![entry("Object 3", "application/octet-stream", &compound(CLASS))]);
        let written = save(path, &doc, &original);
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(report.warning_count("picture_unresolved"), 0, "{path:?}: {:?}", report.warnings);
        assert_eq!(foreign_of(&back, 7).2, Some(PNG.to_vec()), "{path:?}");
    }
}

/// A frame's text is written natively beside the embedding (VSDX holds the text of an object shape): the reader of an `.odg` reads none of it (a text in a `draw:image` is not read), so the model of
/// an `.odg` holds none, and a model that was given one writes it.
#[test]
fn a_frame_with_text_has_its_text_written_beside_the_embedding() {
    for path in PATHS {
        let (mut shape, parts) = frame_6(7);
        shape.text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("Ab"))], ..diacraft_doc::Text::default() }));
        let (doc, original) = world(path, vec![shape], parts);
        let written = save(path, &doc, &original);
        let (back, _) = read_ok(&written.bytes);
        let shape = page_of(&back).shapes.iter().find(|s| s.id == 7).unwrap().clone();
        assert_eq!(shape.text.as_ref().map(|t| t.parts.clone()), Some(vec![diacraft_doc::TextPart::Chars(Arc::from("Ab"))]), "{path:?}");
        assert_eq!(embedding_of(&back, 7), Some(compound(CLASS)), "{path:?}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------- the trip back

/// Reading the written file gives a model whose objects equal the original's: the object's bytes are the embedding's, the replacement is the fallback, and the attributes are the spec's. The content check
/// agrees: the file written from the model that was read is the same content as the file read (the model writer, with the bookkeeping as it was read).
#[test]
fn the_trip_back_gives_the_objects_back_and_the_content_check_agrees() {
    for path in PATHS {
        let object = compound(CLASS);
        let a = frame(7, object_element("./Object 3", None), Some(&replacement("ObjectReplacements/Object 3", &PNG)));
        let mut b = frame(
            8,
            object_element("./Object 5", Some("{0BADC1A5-0002-4000-8000-000000000000}")),
            Some(&replacement("ObjectReplacements/Object 5", &PNG_B)),
        );
        let mut foreign = (**b.foreign.as_ref().unwrap()).clone();
        foreign.attrs = RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("ObjectType", "33280"), ("ShowAsIcon", "1")]);
        b.foreign = Some(Arc::new(foreign));
        let mut other = object.clone();
        other[100] = 0x5A;
        let (doc, original) = world(
            path,
            vec![a, b],
            vec![entry("Object 3", "application/vnd.sun.star.oleobject", &object), entry("Object 5", "application/octet-stream", &other)],
        );
        let written = save(path, &doc, &original);
        let case = format!("{path:?}");
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(report.carry.stale, 0, "{case}: {:?}", report.carry);
        // The objects: bytes, pictures, attributes.
        assert_eq!(embedding_of(&back, 7), Some(object.clone()), "{case}");
        assert_eq!(embedding_of(&back, 8), Some(other.clone()), "{case}");
        assert_eq!(foreign_of(&back, 7).2, Some(PNG.to_vec()), "{case}");
        assert_eq!(foreign_of(&back, 8).2, Some(PNG_B.to_vec()), "{case}");
        assert_eq!(
            foreign_of(&back, 8).0,
            pairs(&[("ForeignType", "Object"), ("ObjectType", "33280"), ("ShowAsIcon", "1")]),
            "{case}: what rode is what is written"
        );
        assert!(odf_parts_of(&back).is_empty(), "{case}: no entry of the package is left, the embeddings are the objects");
        // The content check: the file written from the model that was read is the same content as the file read. The writers type a picture's part by a `Default` or by an `Override` as they
        // choose (the model does not record which), so the one thing that may differ between two writers' files is a row of `[Content_Types].xml`; the model writer's own
        // file is the same content as itself after another trip, with nothing excepted.
        let limits = diacraft_vsdx::Limits::default();
        let again = write_with(&back, None, &WriteOptions { content_unchanged: true, ..options() })
            .unwrap_or_else(|e| panic!("{case}: the model read does not write: {e}"));
        let diff = diacraft_vsdx::compare_content(&written.bytes, &again.bytes, &limits).unwrap();
        let shown = |d: &diacraft_container::canon::ContentDiff| {
            d.shown.iter().take(10).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect::<Vec<_>>().join("\n")
        };
        let others: Vec<_> = diff.shown.iter().filter(|d| d.part != "[Content_Types].xml").collect();
        assert!(
            others.is_empty() && diff.count == diff.shown.len() as u64,
            "{case}: the trip changed the content ({} differences):\n{}",
            diff.count,
            shown(&diff)
        );
        if path == Path::Model {
            assert!(diff.same(), "{case}: the model writer's trip is the same content:\n{}", shown(&diff));
        }
        let (second, _) = read_ok(&again.bytes);
        let third = write_with(&second, None, &WriteOptions { content_unchanged: true, ..options() }).unwrap();
        let diff = diacraft_vsdx::compare_content(&again.bytes, &third.bytes, &limits).unwrap();
        assert!(diff.same(), "{case}: a second trip of the model writer's file changed the content ({} differences):\n{}", diff.count, shown(&diff));
    }
}

/// An object is never parsed: the reader counts the events its XML parser reads, and a file whose object is full of text that looks like XML costs the same as one whose object is zeros.
#[test]
fn an_object_is_never_parsed_whatever_its_bytes_look_like() {
    let events = |filler: &dyn Fn(usize) -> u8| -> u64 {
        let mut object = compound(CLASS);
        object.resize(1 << 20, 0);
        for (i, b) in object.iter_mut().enumerate().skip(1536) {
            *b = filler(i);
        }
        let (shape, _) = frame_6(7);
        let (doc, original) = world(Path::Model, vec![shape], vec![entry("Object 3", "application/vnd.sun.star.oleobject", &object)]);
        let written = save(Path::Model, &doc, &original);
        diacraft_vsdx::work::reset();
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(embedding_of(&back, 7).map(|b| b.len()), Some(1 << 20));
        diacraft_vsdx::work::xml_events_read()
    };
    let xml = b"<a x='1'><b/>&amp;</a>";
    let looks_like_xml = events(&|i| xml[i % xml.len()]);
    let zeros = events(&|_| 0);
    assert!(zeros > 0);
    assert_eq!(looks_like_xml, zeros, "the parser read no event of the object");
}

// ------------------------------------------------------------------------------------------------------------------------------ crafted inputs

/// A distinct picture of 12 bytes for frame `i`.
fn picture_n(i: usize) -> Arc<Media> {
    let mut bytes = PNG.to_vec();
    bytes.extend_from_slice(&u32::try_from(i).unwrap().to_le_bytes());
    Arc::new(Media {
        part: Arc::from(format!("ObjectReplacements/Object {i}").as_str()),
        content_type: Arc::from("image/png"),
        bytes: Arc::from(bytes),
    })
}

/// `n` frames of `n` objects of their own, each with its own picture: the shapes and the entries of an `.odg` of `n` small objects.
fn small_objects(n: usize) -> (Vec<Shape>, Vec<Arc<Part>>) {
    let mut object = compound(CLASS);
    let mut shapes = Vec::with_capacity(n);
    let mut parts = Vec::with_capacity(n);
    for i in 0..n {
        // The object differs by a byte, as every object does.
        let at = 2000 % object.len();
        object[at] = u8::try_from(i % 251).unwrap();
        let href = format!("./Object {}", i + 1);
        shapes.push(frame(u32::try_from(i + 1).unwrap(), object_element(&href, None), Some(&picture_n(i))));
        parts.push(entry(&format!("Object {}", i + 1), "application/vnd.sun.star.oleobject", &object));
    }
    (shapes, parts)
}

/// `n` picture frames with no object: the yardstick of plain content of the same size.
fn plain_pictures(n: usize) -> Vec<Shape> {
    (0..n)
        .map(|i| {
            let mut shape = frame(u32::try_from(i + 1).unwrap(), object_element("./nothing", None), Some(&picture_n(i)));
            // No object element: a picture frame.
            shape.ext = diacraft_doc::Ext::default();
            shape
        })
        .collect()
}

/// The counts of the writers' work for one save of `frames` and `parts`.
struct Counts {
    frames_walked: u64,
    parts_indexed: u64,
    bytes_checked: u64,
    bytes_decoded: u64,
    name_probes: u64,
    parts_planned: u64,
    written: Written,
}

fn counted(path: Path, frames: Vec<Shape>, parts: Vec<Arc<Part>>) -> Counts {
    let (doc, original) = world(path, frames, parts);
    diacraft_vsdx::work::reset();
    let written = save(path, &doc, &original);
    Counts {
        frames_walked: diacraft_vsdx::work::object_frames_walked(),
        parts_indexed: diacraft_vsdx::work::object_parts_indexed(),
        bytes_checked: diacraft_vsdx::work::object_bytes_checked(),
        bytes_decoded: diacraft_vsdx::work::object_bytes_decoded(),
        name_probes: diacraft_vsdx::work::name_probes(),
        parts_planned: diacraft_vsdx::work::model_parts_planned(),
        written,
    }
}

/// 10,000 small objects: each frame is looked at a bounded number of times, the parts of the model are put in the map once, the check of a compound file reads a fixed number of bytes of each,
/// no byte is copied, and the names are found by one walk up the numbers. The package holds three parts for each: the object, its relationships and its picture.
#[test]
fn ten_thousand_small_objects_are_counted() {
    let n = 10_000usize;
    for path in PATHS {
        let (shapes, parts) = small_objects(n);
        let c = counted(path, shapes, parts);
        let case = format!("{path:?}");
        eprintln!(
            "{case}: frames {} parts {} checked {} decoded {} probes {} planned {}",
            c.frames_walked, c.parts_indexed, c.bytes_checked, c.bytes_decoded, c.name_probes, c.parts_planned
        );
        let n64 = n as u64;
        assert!(c.frames_walked <= 2 * n64 + 100, "{case}: each shape once, and one node of its kept list: {}", c.frames_walked);
        assert_eq!(c.parts_indexed, n64, "{case}: the parts of the model are put in the map once");
        assert!(c.bytes_checked <= 640 * n64, "{case}: at most the header and one entry of each object: {}", c.bytes_checked);
        assert_eq!(c.bytes_decoded, 0, "{case}: no byte is copied");
        assert!(c.name_probes <= 2 * n64 + 100, "{case}: the names are found by one walk each (an object's and a picture's): {}", c.name_probes);
        if path == Path::Model {
            assert!(c.parts_planned <= 2 * n64 + 100, "{case}: the model writer plans each part once: {}", c.parts_planned);
        }
        let names = embeddings(&c.written.bytes);
        assert_eq!(names.len(), 2 * n, "{case}: an object part and its relationships part for each");
        let (back, _) = read_ok(&c.written.bytes);
        assert_eq!(embedding_of(&back, 1).map(|b| b.len()), Some(1536), "{case}");
        assert_eq!(embedding_of(&back, 10_000).map(|b| b.len()), Some(1536), "{case}");
        assert_eq!(page_of(&back).shapes.len(), n, "{case}");
    }
}

/// 10,000 frames that name one object and show one picture are one embedding: one object part, its relationships part, one picture part, and one row of the page.
#[test]
fn ten_thousand_frames_of_one_object_are_one_embedding() {
    let n = 10_000usize;
    for path in PATHS {
        let object = entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS));
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        let shapes: Vec<Shape> = (0..n).map(|i| frame(u32::try_from(i + 1).unwrap(), object_element("./Object 3", None), Some(&picture))).collect();
        let c = counted(path, shapes, vec![object]);
        let case = format!("{path:?}");
        assert!(c.frames_walked <= 2 * n as u64 + 100, "{case}: {}", c.frames_walked);
        assert!(c.bytes_checked <= 640, "{case}: the object is checked once: {}", c.bytes_checked);
        assert_eq!(embeddings(&c.written.bytes), ["/visio/embeddings/_rels/oleObject1.bin.rels", "/visio/embeddings/oleObject1.bin"], "{case}");
        let (back, _) = read_ok(&c.written.bytes);
        let part = page_of(&back).part_name.clone().unwrap();
        let object_rows = rows(&c.written.bytes, &part).into_iter().filter(|(_, ty, _)| *ty == ole_rel()).count();
        assert_eq!(object_rows, 1, "{case}");
        assert_eq!(embedding_of(&back, 5_000), Some(compound(CLASS)), "{case}");
    }
}

/// An object whose replacement picture is missing rides and the save is written; an object whose file is a directory of entries is not carried back (the last two cases, at the size of a crafted file).
#[test]
fn ten_thousand_objects_that_cannot_go_natively_cost_no_more_than_the_judge_of_each() {
    let n = 10_000usize;
    for path in PATHS {
        let (mut shapes, parts) = small_objects(n);
        // No picture for any of them.
        for shape in &mut shapes {
            let mut foreign = (**shape.foreign.as_ref().unwrap()).clone();
            foreign.picture = None;
            foreign.attrs = RawAttrs::new();
            shape.foreign = Some(Arc::new(foreign));
        }
        let c = counted(path, shapes, parts);
        let case = format!("{path:?}");
        assert!(c.frames_walked <= 2 * n as u64 + 100, "{case}: {}", c.frames_walked);
        assert_eq!(c.bytes_checked, 0, "{case}: an object whose picture is missing is not looked into");
        assert!(embeddings(&c.written.bytes).is_empty(), "{case}");
        let (back, _) = read_ok(&c.written.bytes);
        assert_eq!(odf_parts_of(&back).len(), n, "{case}: every object rides");
    }
}

// ---------------------------------------------------------------------------------------------------------------- the cap of a part, in a child

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            u8::try_from(x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56).unwrap()
        })
        .collect()
}

fn status_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.strip_prefix(field).and_then(|v| v.trim_start_matches(':').trim().strip_suffix("kB").and_then(|n| n.trim().parse().ok()))
            })
        })
        .unwrap_or(0)
}

const SCENARIO: &str = "DIACRAFT_OBJECTS_SCENARIO";

/// The child of the cap tests: an object of the cap of a part plus `extra` bytes, an `.odg`'s object that is a compound file at its head and noise after it.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("the scenario");
    let mut fields = scenario.split(':');
    let (path, extra): (&str, i64) = (fields.next().unwrap_or(""), fields.next().and_then(|v| v.parse().ok()).unwrap_or(0));
    let path = match path {
        "skeleton" => Path::Skeleton,
        "splice" => Path::Splice,
        _ => Path::Model,
    };
    let cap = i64::try_from(diacraft_container::limits::MAX_PART_BYTES).unwrap();
    let len = usize::try_from(cap + extra).unwrap();
    let mut object = noise(len, 7);
    object[..1536].copy_from_slice(&compound(CLASS));
    let (shape, _) = frame_6(7);
    let (doc, original) = world(path, vec![shape], vec![entry("Object 3", "application/vnd.sun.star.oleobject", &object)]);
    drop(object);
    match try_save(path, &doc, &original, &options()) {
        Err(e) => println!("OUTCOME refused:{} {e}", e.class()),
        Ok(written) => {
            let names = embeddings(&written.bytes);
            println!("OUTCOME written {} bytes, {} embedding parts", written.bytes.len(), names.len());
        }
    }
    println!("VMPEAK {}", status_kib("VmPeak"));
    println!("objects: no abort");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[cfg(target_os = "linux")]
fn under_the_limit(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact objects::child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("objects: no abort"), "{scenario}: the child did not end well: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("OUTCOME") || l.contains("VMPEAK")).collect::<Vec<_>>().join(" | "));
    text
}

/// An object one byte over the cap of a part is not carried back (its embedding would be a part no package may hold), so it rides, and the carrier refuses it in words that name the part and say
/// how to keep it: a refusal and not an abort, in a process held to 3 GB of address space.
#[test]
#[cfg(target_os = "linux")]
fn an_object_one_byte_over_the_part_cap_is_refused_in_words_and_not_an_abort() {
    for path in ["model", "skeleton"] {
        let text = under_the_limit(&format!("{path}:1"));
        assert!(text.contains("OUTCOME refused:carried_part_refused"), "{path}: {text}");
        assert!(text.contains("Object 3") && text.contains("256 MiB"), "{path}: {text}");
    }
}

/// An object of exactly the cap of a part is carried back: written as an embedding of that size, and read back by the check of the save, in the same address space.
#[test]
#[cfg(target_os = "linux")]
fn an_object_of_exactly_the_part_cap_is_written_as_an_embedding_under_the_address_limit() {
    for path in ["model", "skeleton"] {
        let text = under_the_limit(&format!("{path}:0"));
        assert!(text.contains("OUTCOME written") && text.contains("2 embedding parts"), "{path}: {text}");
        let peak: u64 = text.lines().find_map(|l| l.strip_prefix("VMPEAK ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        assert!(peak > 0 && peak < 3_000_000, "{path}: {peak}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- timed beside a yardstick

/// Each crafted input beside plain content of the same size, written: the case costs no more than a multiple of the plain write, and says what it measured.
#[test]
#[ignore = "timing: run with --ignored, on a machine that is not busy"]
fn timed_beside_a_yardstick() {
    use std::time::Instant;
    let time = |label: &str, work: &dyn Fn()| -> f64 {
        let start = Instant::now();
        work();
        let secs = start.elapsed().as_secs_f64();
        println!("{label}: {secs:.3} s");
        secs
    };
    let n = 10_000usize;
    for path in PATHS {
        // 10,000 small objects against 10,000 pictures: a picture is a part and a row, an object is three parts and a row.
        let objects = time(&format!("{path:?}: write 10,000 small objects"), &|| {
            let (shapes, parts) = small_objects(n);
            let (doc, original) = world(path, shapes, parts);
            drop(save(path, &doc, &original));
        });
        let plain = time(&format!("{path:?}: write 10,000 pictures (the yardstick)"), &|| {
            let (doc, original) = world(path, plain_pictures(n), Vec::new());
            drop(save(path, &doc, &original));
        });
        assert!(objects <= 4.0 * plain + 0.5, "{path:?}: {objects} against {plain}");
        // 10,000 frames of one object against 10,000 frames of one picture.
        let shared = time(&format!("{path:?}: write 10,000 frames of one object"), &|| {
            let picture = replacement("ObjectReplacements/Object 3", &PNG);
            let shapes: Vec<Shape> =
                (0..n).map(|i| frame(u32::try_from(i + 1).unwrap(), object_element("./Object 3", None), Some(&picture))).collect();
            let (doc, original) = world(path, shapes, vec![entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS))]);
            drop(save(path, &doc, &original));
        });
        let yardstick = time(&format!("{path:?}: write 10,000 frames of one picture (the yardstick)"), &|| {
            let picture = replacement("ObjectReplacements/Object 3", &PNG);
            let shapes: Vec<Shape> = (0..n)
                .map(|i| {
                    let mut shape = frame(u32::try_from(i + 1).unwrap(), object_element("./nothing", None), Some(&picture));
                    shape.ext = diacraft_doc::Ext::default();
                    shape
                })
                .collect();
            let (doc, original) = world(path, shapes, Vec::new());
            drop(save(path, &doc, &original));
        });
        assert!(shared <= 4.0 * yardstick + 0.5, "{path:?}: {shared} against {yardstick}");
        // 10,000 objects that ride (no replacement picture) against the same 10,000 parts held by a plain drawing as parts of DiaCraft's own.
        let rides = time(&format!("{path:?}: write 10,000 objects that ride"), &|| {
            let (mut shapes, parts) = small_objects(n);
            for shape in &mut shapes {
                let mut foreign = (**shape.foreign.as_ref().unwrap()).clone();
                foreign.picture = None;
                shape.foreign = Some(Arc::new(foreign));
            }
            let (doc, original) = world(path, shapes, parts);
            drop(save(path, &doc, &original));
        });
        let parts_only = time(&format!("{path:?}: write the same 10,000 parts with no frame (the yardstick)"), &|| {
            let (_, parts) = small_objects(n);
            let (doc, original) = world(path, Vec::new(), parts);
            drop(save(path, &doc, &original));
        });
        assert!(rides <= 4.0 * parts_only + 0.5, "{path:?}: {rides} against {parts_only}");
    }
}

// ------------------------------------------------------------------------------------------------------------------ files for LibreOffice

/// A real PNG of `w` by `h` pixels of one colour, made by hand (a stored zlib block): a file a program that draws it can read.
fn real_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32fast::hash(&body).to_be_bytes());
    }
    // One scanline: the filter byte, then the pixels; the picture is `h` of them.
    let mut line = vec![0u8];
    for _ in 0..w {
        line.extend_from_slice(&rgb);
    }
    let mut raw = Vec::new();
    for _ in 0..h {
        raw.extend_from_slice(&line);
    }
    let len = u16::try_from(raw.len()).unwrap();
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&(!len).to_le_bytes());
    z.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend_from_slice(&w.to_be_bytes());
    header.extend_from_slice(&h.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Writes the files the black-box check with LibreOffice opens, to the folder `DIACRAFT_LO_DIR` names (and nothing when it is not set): the cases of this file's tests that make a package, by every
/// writer. Run by `lo_check.py` of the run folder; not a test of this crate's own.
#[test]
#[ignore = "writes files for the LibreOffice check: set DIACRAFT_LO_DIR"]
fn lo_files() {
    let Ok(dir) = std::env::var("DIACRAFT_LO_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    let ole = compound(CLASS);
    let png_a = real_png(8, 8, [200, 30, 30]);
    let png_b = real_png(8, 8, [30, 30, 200]);
    let pic = |name: &str, bytes: &[u8]| Arc::new(Media { part: Arc::from(name), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) });
    for path in PATHS {
        let tag = format!("{path:?}").to_lowercase();
        let object = |n: u32| entry(&format!("Object {n}"), "application/vnd.sun.star.oleobject", &ole);
        // V6.
        let (doc, original) =
            world(path, vec![frame(7, object_element("./Object 3", None), Some(&pic("ObjectReplacements/Object 3", &png_a)))], vec![object(3)]);
        put(&format!("v6-{tag}.vsdx"), &save(path, &doc, &original).bytes);
        // Two frames of one object and one picture, and of one object and two pictures.
        let shared = pic("ObjectReplacements/Object 3", &png_a);
        let frames = vec![frame(7, object_element("./Object 3", None), Some(&shared)), frame(8, object_element("./Object 3", None), Some(&shared))];
        let (doc, original) = world(path, frames, vec![object(3)]);
        put(&format!("shared-{tag}.vsdx"), &save(path, &doc, &original).bytes);
        let frames = vec![
            frame(7, object_element("./Object 3", None), Some(&pic("ObjectReplacements/Object 3", &png_a))),
            frame(8, object_element("./Object 3", None), Some(&pic("ObjectReplacements/Object 4", &png_b))),
        ];
        let (doc, original) = world(path, frames, vec![object(3)]);
        put(&format!("two-pictures-{tag}.vsdx"), &save(path, &doc, &original).bytes);
        // The object written inline.
        let inline = with_children(object_element("./nothing", None), vec![RawNode::Element(inline_element(&ole))]);
        let (doc, original) = world(path, vec![frame(7, inline, Some(&pic("ObjectReplacements/Object 3", &png_a)))], Vec::new());
        put(&format!("inline-{tag}.vsdx"), &save(path, &doc, &original).bytes);
        // A formula folder rides, and an object that is no compound file rides.
        let folder = frame(7, element("draw:object", &[("xlink:href", "./Object 2")], Vec::new()), Some(&pic("ObjectReplacements/Object 2", &png_b)));
        let parts = vec![
            entry("Object 2/", "application/vnd.oasis.opendocument.formula", b""),
            entry("Object 2/content.xml", "text/xml", b"<math/>"),
            entry("Object 4", "application/octet-stream", b"PK\x03\x04 not a compound file"),
        ];
        let packed = frame(8, object_element("./Object 4", None), Some(&pic("ObjectReplacements/Object 4", &png_a)));
        let (doc, original) = world(path, vec![folder, packed], parts);
        put(&format!("rides-{tag}.vsdx"), &save(path, &doc, &original).bytes);
        // Ten objects, each its own.
        let mut shapes = Vec::new();
        let mut parts = Vec::new();
        for i in 0..10u32 {
            let mut o = ole.clone();
            o[1000] = u8::try_from(i).unwrap();
            let picture = pic(&format!("ObjectReplacements/Object {}", i + 1), if i % 2 == 0 { &png_a } else { &png_b });
            shapes.push(frame(i + 1, object_element(&format!("./Object {}", i + 1), None), Some(&picture)));
            parts.push(entry(&format!("Object {}", i + 1), "application/vnd.sun.star.oleobject", &o));
        }
        let (doc, original) = world(path, shapes, parts);
        put(&format!("ten-{tag}.vsdx"), &save(path, &doc, &original).bytes);
    }
}

// ---------------------------------------------------------------------------------------------------------------- a page the file has

/// A frame put on a page that has a part already (a page of the file the drawing was read from) is written by the model writer as an embedding too: the page part's own relationships gain the one
/// row to the object, none to the picture. (The save into a file writes a shape put on a part the file has without its picture, as it does any picture: gaps F11.)
#[test]
fn a_frame_on_a_page_that_has_a_part_is_written_as_an_embedding_by_the_model_writer() {
    let original = base();
    let mut doc = read_ok(&original).0;
    let (shape, parts) = frame_6(50);
    let at = doc.pages.iter().position(|p| p.id == 1).unwrap();
    Arc::make_mut(doc.pages.get_mut(at).unwrap()).shapes.push(Arc::new(shape));
    for part in parts {
        doc.parts.push(part);
    }
    let written = save(Path::Model, &doc, &original);
    assert_eq!(embeddings(&written.bytes), ["/visio/embeddings/_rels/oleObject1.bin.rels", "/visio/embeddings/oleObject1.bin"]);
    let (back, report) = read_ok(&written.bytes);
    let page = back.page(1).unwrap();
    let shape = page.shapes.iter().find(|s| s.id == 50).unwrap();
    let foreign = shape.foreign.as_ref().unwrap();
    assert_eq!(foreign.attr("ForeignType"), Some("Object"));
    assert_eq!(foreign.picture.as_ref().map(|m| m.bytes.to_vec()), Some(PNG.to_vec()));
    let rows = rows(&written.bytes, page.part_name.as_deref().unwrap());
    let rel = foreign.rel_id.as_deref().unwrap();
    assert_eq!(
        rows.iter().find(|(id, _, _)| id == rel).map(|r| (r.1.clone(), r.2.clone())),
        Some((ole_rel(), Some("/visio/embeddings/oleObject1.bin".to_owned())))
    );
    assert!(!rows.iter().any(|(_, ty, _)| *ty == image_rel()), "no image row on the page: {rows:?}");
    assert_eq!(report.carry.stale, 0, "{:?}", report.carry);
    assert!(blob_parts(&written.bytes).is_empty(), "the object rides nowhere else");
}

/// **A save says which parts of the other format it wrote as embeddings** (`Written::natively`), so that a caller which saves into the file again can give the writer the drawing without them: the
/// file holds each object once, and the drawing still holds the part the embedding was written from. Each writer names the part of a frame carried back, once for two frames of one object, and names
/// nothing for a frame that rides, for a part a frame which rides names too, and for a drawing with no object.
#[test]
fn a_save_names_the_parts_it_wrote_as_embeddings_and_no_other() {
    for path in PATHS {
        let case = format!("{path:?}");
        let object = entry("Object 3", "application/vnd.sun.star.oleobject", &compound(CLASS));
        let picture = replacement("ObjectReplacements/Object 3", &PNG);
        // One frame carried back.
        let (shape, parts) = frame_6(7);
        let (doc, original) = world(path, vec![shape], parts);
        let written = save(path, &doc, &original);
        assert_eq!(written.natively.iter().map(|n| &**n).collect::<Vec<_>>(), ["Object 3"], "{case}: the part the embedding was written from");
        // Two frames of one object and one picture share the embedding: the part is named once.
        let a = frame(7, object_element("./Object 3", None), Some(&picture));
        let b = frame(8, object_element("./Object 3", None), Some(&picture));
        let (doc, original) = world(path, vec![a, b], vec![Arc::clone(&object)]);
        let written = save(path, &doc, &original);
        assert_eq!(written.natively.iter().map(|n| &**n).collect::<Vec<_>>(), ["Object 3"], "{case}: one object, one name");
        // A part that a frame which rides names too rides: it is not named.
        let bad = replacement("ObjectReplacements/Object 4", b"NOTAPICT");
        let a = frame(7, object_element("./Object 3", None), Some(&picture));
        let b = frame(8, object_element("./Object 3", None), Some(&bad));
        let (doc, original) = world(path, vec![a, b], vec![Arc::clone(&object)]);
        assert!(save(path, &doc, &original).natively.is_empty(), "{case}: the entry rides for the frame that rides, and is no embedding's alone");
        // A frame that is not carried back (its object is not a compound file).
        let a = frame(7, object_element("./Object 3", None), Some(&picture));
        let (doc, original) = world(path, vec![a], vec![entry("Object 3", "application/vnd.sun.star.oleobject", b"not a compound file")]);
        assert!(save(path, &doc, &original).natively.is_empty(), "{case}: a frame that rides writes no embedding");
        // No object at all.
        let (doc, original) = world(path, Vec::new(), Vec::new());
        assert!(save(path, &doc, &original).natively.is_empty(), "{case}: a drawing with no object");
    }
}

/// **The baseline of a save that wrote an embedding is the parts the file holds, made from what the writer did** (`write::objects::baseline_parts`): the embedding part, holding **the very
/// allocation of the model's object file** (no second copy), with its one `image` row; and not the object's file, which the file holds as that embedding. What a read of the file gives of the
/// embedding is what the baseline says of it. **The save reads the file it made once** (the security scan: a second read at the default limits, its copy of the parts uncounted):
/// the XML events of the save are those of one read of the file, not two. And **no part is compared by its bytes**: 2,000 parts of the other format of one length ride beside the object, and each is
/// in the baseline as the model's own allocation.
#[test]
fn the_baseline_of_a_save_that_wrote_an_embedding_holds_it_as_the_models_bytes_and_the_file_is_read_once() {
    for path in [Path::Model, Path::Splice, Path::Skeleton] {
        let case = format!("{path:?}");
        let (shape, mut parts) = frame_6(7);
        let riding: Vec<Arc<Part>> = (0..2_000u32)
            .map(|k| {
                entry(&format!("Pictures/same-length-{k:05}.bin"), "application/octet-stream", &[(k % 251) as u8, (k / 251) as u8, 7, 7, 7, 7, 7, 7])
            })
            .collect();
        parts.extend(riding.iter().cloned());
        let object_bytes = Arc::clone(&parts.first().unwrap().bytes);
        let (doc, original) = world(path, vec![shape], parts);
        diacraft_vsdx::work::reset();
        let written = save(path, &doc, &original);
        let events_of_the_save = diacraft_vsdx::work::xml_events_read();
        diacraft_vsdx::work::reset();
        let (back, _) = read_ok(&written.bytes);
        let events_of_a_read = diacraft_vsdx::work::xml_events_read();
        assert!(
            events_of_the_save < events_of_a_read.saturating_mul(3) / 2,
            "{case}: the save read {events_of_the_save} XML events, one read of the file is {events_of_a_read}: the file is read once"
        );
        let names: Vec<String> = embeddings(&written.bytes).into_iter().filter(|n| !n.contains("/_rels/")).collect();
        assert_eq!(names.len(), 1, "{case}: {names:?}");
        let baseline = &written.document;
        let embedding =
            baseline.parts.iter().find(|p| fold(&p.name) == fold(&names[0])).unwrap_or_else(|| panic!("{case}: the baseline holds the embedding"));
        assert!(embedding.format == Format::Vsdx && embedding.role == PartRole::Embedding, "{case}: {embedding:?}");
        assert!(Arc::ptr_eq(&embedding.bytes, &object_bytes), "{case}: the embedding holds the model's allocation of the object's bytes, not a copy");
        assert!(
            !baseline.parts.iter().any(|p| p.format == Format::Odf && &*p.name == "Object 3"),
            "{case}: the object's file is the embedding in the file"
        );
        let read = back.parts.iter().find(|p| fold(&p.name) == fold(&names[0])).unwrap();
        let rels = |p: &Part| p.rels.iter().map(|r| (r.id.to_string(), r.kind.to_string(), r.target.to_string())).collect::<Vec<_>>();
        assert_eq!(rels(embedding), rels(read), "{case}: the baseline's embedding has the rows the file's has");
        assert_eq!((embedding.media_type.as_ref(), embedding.role), (read.media_type.as_ref(), read.role), "{case}");
        for part in &riding {
            assert!(baseline.parts.iter().any(|held| Arc::ptr_eq(held, part)), "{case}: {} is the model's own part in the baseline", part.name);
        }
    }
}
