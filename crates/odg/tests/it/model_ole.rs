//! A VSDX OLE object is written as an ODF object frame: `draw:object-ole` with its class and its links, the replacement picture
//! as `draw:image`, and the two entries of R21 (`Object N`, `ObjectReplacements/Object N`) with their manifest rows. The model is built by hand as the VSDX reader fills one.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, ContentTypeRow, Document, ForeignData, Format, Media, Part, PartRole, RawAttrs, Rel, Shape, ShapeType, Value, VsdxHeld,
};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::model_write::blank;
use crate::write_core::PNG;

const PAGE: &str = "/visio/pages/page1.xml";
const OBJECT: &str = "/visio/embeddings/oleObject1.bin";
const OLE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// The compound file of R7: a header and a root entry of the class `D1A0C0DE-0001-4000-8000-00000000BEEF`.
pub(crate) fn compound(class: [u8; 16]) -> Vec<u8> {
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

pub(crate) const CLASS: [u8; 16] = [0xDE, 0xC0, 0xA0, 0xD1, 0x01, 0x00, 0x00, 0x40, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBE, 0xEF];

fn number(text: &str) -> Cell {
    Cell::constant(Value::from_text(text)).with_unit("IN")
}

/// A drawing as the VSDX reader fills one for a page holding an OLE object: the shape, its relationship held, its part with its own relationship to a picture, and the page's part name.
pub(crate) fn drawing(bytes: Vec<u8>, picture: Vec<u8>, with_image_rel: bool) -> Document {
    let mut doc = blank(0);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.part_name = Some(Arc::from(PAGE));
    let mut shape = Shape::new(1);
    shape.kind = ShapeType::Foreign;
    shape.name_u = Some(Arc::from("Equation"));
    shape.name = Some(Arc::from("Equation"));
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, value) in [("PinX", "3"), ("PinY", "5"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5")] {
        sheet.cells.insert(CellName::new(name), number(value));
    }
    shape.foreign = Some(Arc::new(ForeignData {
        attrs: RawAttrs::from_pairs([
            ("ForeignType", "Object"),
            ("ObjectType", "33280"),
            ("ShowAsIcon", "0"),
            ("ObjectWidth", "2"),
            ("ObjectHeight", "1"),
        ]),
        rel_id: Some(Arc::from("rId1")),
        picture: Some(Arc::new(Media {
            part: Arc::from("/visio/media/image1.png"),
            content_type: Arc::from("image/png"),
            bytes: Arc::from(picture),
        })),
        ..ForeignData::default()
    }));
    page.shapes = diacraft_doc::SharedVec::from_vec(vec![Arc::new(shape)]);
    let rel = |source: &str, id: &str, kind: &str, target: &str| Rel {
        source: Arc::from(source),
        id: Arc::from(id),
        kind: Arc::from(kind),
        target: Arc::from(target),
        external: false,
    };
    doc.vsdx = Some(Arc::new(VsdxHeld {
        rels: vec![rel(PAGE, "rId1", OLE_REL, "../embeddings/oleObject1.bin")],
        content_types: Vec::<ContentTypeRow>::new(),
        ..VsdxHeld::default()
    }));
    let part_rels = if with_image_rel { vec![rel(OBJECT, "rId1", IMAGE_REL, "../media/image1.png")] } else { Vec::new() };
    doc.parts.push(Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(OBJECT),
        media_type: Arc::from("application/vnd.openxmlformats-officedocument.oleObject"),
        role: PartRole::Embedding,
        manifest: RawAttrs::new(),
        rels: Arc::new(part_rels),
        bytes: Arc::from(bytes),
    }));
    doc
}

fn written(doc: &Document) -> Result<Vec<u8>, WriteError> {
    write_with(doc, None, &WriteOptions::default()).map(|w| w.bytes)
}

fn part(package: &Package, name: &str) -> String {
    String::from_utf8_lossy(package.part(name).unwrap_or_else(|| panic!("no entry {name}"))).into_owned()
}

#[test]
fn a_vsdx_ole_object_is_written_as_object_ole_with_its_replacement() {
    let object = compound(CLASS);
    let bytes = written(&drawing(object.clone(), PNG.to_vec(), true)).unwrap_or_else(|e| panic!("the object is refused: {e}"));
    let package = Package::read(&bytes, &Limits::default()).unwrap();
    let content = part(&package, "content.xml");
    // The frame holds the object and then the replacement, with the three link attributes; the class, bare.
    assert!(content.contains("<draw:frame "), "{content}");
    let object_at = content.find("<draw:object-ole").expect("the object element");
    let image_at = content.find("<draw:image").expect("the replacement");
    assert!(object_at < image_at, "the object comes first: {content}");
    let element = &content[object_at..content[object_at..].find("/>").map(|n| object_at + n).unwrap()];
    for want in [
        "xlink:href=\"./Object 1\"",
        "xlink:type=\"simple\"",
        "xlink:show=\"embed\"",
        "xlink:actuate=\"onLoad\"",
        "draw:class-id=\"D1A0C0DE-0001-4000-8000-00000000BEEF\"",
    ] {
        assert!(element.contains(want), "{want}: {element}");
    }
    assert!(content.contains("xlink:href=\"./ObjectReplacements/Object 1\""), "{content}");
    // The two entries hold the bytes unchanged, each with its row.
    assert_eq!(package.part("Object 1").unwrap(), &object[..], "the object's bytes are the part's, unchanged");
    assert_eq!(package.part("ObjectReplacements/Object 1").unwrap(), &PNG[..], "and the replacement's are the picture's");
    let row = |name: &str| package.manifest().entry(name).and_then(|e| e.media_type.clone());
    assert_eq!(row("Object 1").as_deref(), Some("application/vnd.sun.star.oleobject"));
    assert_eq!(row("ObjectReplacements/Object 1").as_deref(), Some("image/png"));
    // The VSDX part itself is no entry, and the file reads back with the frame holding the object and one picture.
    assert!(package.part("visio/embeddings/oleObject1.bin").is_none() && package.part("/visio/embeddings/oleObject1.bin").is_none());
    let (back, _) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 1, "the object is one shape");
    assert!(
        back.pages.get(0).unwrap().shapes.get(0).unwrap().foreign.as_deref().is_some_and(|f| f.picture.is_some()),
        "that shows its replacement picture"
    );
}

#[test]
fn a_null_class_is_left_out_and_the_number_is_the_lowest_above_every_object_in_use() {
    let mut doc = drawing(compound([0; 16]), PNG.to_vec(), true);
    // An object of the model's own is in use: `Object 4/` (a folder entry and a file in it).
    doc.parts.push(Arc::new(Part {
        format: Format::Odf,
        name: Arc::from("Object 4/content.xml"),
        media_type: Arc::from("text/xml"),
        role: PartRole::ObjectFile,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&b"<office:document-content/>"[..]),
    }));
    let bytes = written(&doc).unwrap_or_else(|e| panic!("refused: {e}"));
    let package = Package::read(&bytes, &Limits::default()).unwrap();
    let content = part(&package, "content.xml");
    assert!(content.contains("xlink:href=\"./Object 5\"") && package.part("Object 5").is_some(), "{content}");
    assert!(!content.contains("draw:class-id"), "a null class is left out: {content}");
}

/// An object that cannot be written natively rides in the carrier: its bytes are an entry of DiaCraft's own folder and come back byte for byte, and the shape shows its replacement
/// picture where it has one.
#[test]
fn an_object_that_is_not_carried_natively_rides_in_the_carrier_and_comes_back_byte_for_byte() {
    // The target must be a compound file, and its relationship part must name a picture.
    for (what, bytes, picture, with_image_rel) in [
        ("bytes that are no compound file", b"PK\x03\x04 not a compound file".to_vec(), PNG.to_vec(), true),
        ("a part with no picture of its own", compound(CLASS), PNG.to_vec(), false),
    ] {
        let saved = written(&drawing(bytes.clone(), picture, with_image_rel)).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (back, _) = read(&saved, &Limits::default()).unwrap();
        let part = back
            .parts
            .iter()
            .find(|p| p.format == Format::Vsdx && p.name.ends_with("oleObject1.bin"))
            .unwrap_or_else(|| panic!("{what}: the object does not come back"));
        assert_eq!(&part.bytes[..], &bytes[..], "{what}");
    }
    // A replacement DiaCraft cannot draw is a picture frame the writer does not write: refused in words, as a new picture in that format is.
    let refused =
        written(&drawing(compound(CLASS), b"<svg xmlns=\"x\"/>".to_vec(), true)).expect_err("a replacement that is no picture DiaCraft draws");
    assert!(matches!(&refused, WriteError::Unsupported(w) if w.contains("does not draw")), "{refused}");
}

/// **The drawing as the file the model writer wrote holds it of what rides.** The model writer writes a carried object as an object frame and takes the VSDX embedding (and the
/// relationship that named it) out of what rides; a drawing saved into that file with the embedding still riding was an edit with identical bytes, and an edit wrote the object a second time in
/// DiaCraft's own folder. `without_carried_objects` gives the drawing as the file has it: no embedding, no relationship, the same shapes and pages; `None` where nothing was carried.
#[test]
fn a_carried_objects_embedding_is_out_of_what_rides_in_the_drawing_a_save_into_the_file_is_given() {
    let doc = drawing(compound(CLASS), PNG.to_vec(), true);
    let stripped = diacraft_odg::without_carried_objects(&doc).unwrap().expect("the object is carried");
    assert!(!stripped.parts.iter().any(|p| p.role == PartRole::Embedding), "the embedding is the frame's own entry in the file");
    assert!(stripped.vsdx.as_ref().is_some_and(|held| held.rels.is_empty()), "and the relationship that named it is no held data");
    assert_eq!(stripped.pages.len(), doc.pages.len());
    assert!(stripped.pages.iter().zip(doc.pages.iter()).all(|(a, b)| Arc::ptr_eq(a, b)), "the pages are the drawing's own");
    assert_eq!(doc.parts.len(), 1, "the drawing a person has still holds it");
    // Nothing carried: an object that cannot go natively rides, and a drawing with no object has nothing to take out.
    assert!(diacraft_odg::without_carried_objects(&drawing(compound(CLASS), PNG.to_vec(), false)).unwrap().is_none());
    assert!(diacraft_odg::without_carried_objects(&drawing(b"PK\x03\x04 no compound file".to_vec(), PNG.to_vec(), true)).unwrap().is_none());
    assert!(diacraft_odg::without_carried_objects(&blank(0)).unwrap().is_none());
}

/// The chain on the writer: the file the model writer wrote, saved into with the drawing as the file holds it, is no edit and the same bytes; with the embedding riding it is an edit,
/// which is what an unedited Save of the session's drawing was.
#[test]
fn a_save_into_the_file_the_model_writer_wrote_is_no_edit_with_the_embedding_out_of_what_rides() {
    let doc = drawing(compound(CLASS), PNG.to_vec(), true);
    let first = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let original = diacraft_odg::OriginalPackage { bytes: &first.bytes, index: &first.index };
    let given = diacraft_odg::without_carried_objects(&doc).unwrap().unwrap();
    let again = write_with(&given, Some(&original), &WriteOptions::default()).unwrap();
    assert!(!again.edited, "the drawing as the file holds it is no edit");
    assert_eq!(again.bytes, first.bytes, "and the file is as it was");
    let whole = write_with(&doc, Some(&original), &WriteOptions::default()).unwrap();
    assert!(whole.edited, "the drawing whole is the edit that wrote the object again");
}

/// A black-box check against LibreOffice, run by hand (ignored): the object written as `draw:object-ole` goes to `$ODG_OUT_DIR/in/ole.odg` for `soffice --convert-to pdf`.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_OUT_DIR set to an empty folder"]
fn lo_model_ole() {
    let dir = std::path::PathBuf::from(std::env::var("ODG_OUT_DIR").expect("set ODG_OUT_DIR to an empty folder")).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ole.odg"), written(&drawing(compound(CLASS), PNG.to_vec(), true)).unwrap()).unwrap();
}
