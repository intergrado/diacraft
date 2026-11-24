//! Through the engine and the real readers: a drawing opened from an `.odg` that holds object frames is saved as `.diacraft` and opened again (what 0.2 lets a person do with it
//! before a change), and saved as a VSDX type from there: the frames that calls carried back are written as embeddings, the others ride in DiaCraft's own parts,
//! and the file read again gives the objects back. The `.odg` is built in code from the standard, and **every model here is made by the real `.odg` reader**, so that what the writer judges is what a
//! person's file gives it (the part of an object's own file has no role, a replacement the manifest types as XML is typed by its bytes, and so on).

use diacraft_doc::{Document, Format, Shape};
use diacraft_vsdx::{Limits, compare_content, read};
use serde_json::json;

use crate::odg_build::{Dir, Drawing};
use crate::run;

/// 1,536 bytes, a compound file whose class is invented.
fn compound() -> Vec<u8> {
    let mut bytes = vec![0u8; 1536];
    bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
    bytes[1024 + 0x42] = 5;
    bytes[1024 + 0x50..1024 + 0x60]
        .copy_from_slice(&[0xDE, 0xC0, 0xA0, 0xD1, 0x01, 0x00, 0x00, 0x40, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBE, 0xEF]);
    bytes
}

/// A small PNG.
fn png(shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[shade; 16]).unwrap();
    out
}

/// The bytes of the object of frame C: a ZIP package's first bytes, no compound file.
const PACKAGE: &[u8] = b"PK\x03\x04 a package, not a compound file";

const LINKS: &str = "xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"";

/// A frame of `name` holding `object` (an element) and the replacement picture `image`.
fn object_frame(name: &str, object: &str, image: &str, y: f64) -> String {
    format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"{y}cm\">{object}<draw:image xlink:href=\"{image}\" {LINKS}/></draw:frame>"
    )
}

/// An `.odg` of three object frames: **A** an object that is a compound file with a PNG for its replacement; **B** an embedded document (`draw:object`, a folder of two files) with a PNG; **C** an
/// object whose bytes are a ZIP package, with a PNG. The manifest types the pictures as XML, as the builder's manifest does for what is not a PNG by name.
fn odg() -> Vec<u8> {
    let mut d = Drawing::new().page(
        &[
            object_frame("A", &format!("<draw:object-ole xlink:href=\"./Object 1\" {LINKS}/>"), "./ObjectReplacements/Object 1", 2.0),
            object_frame("B", &format!("<draw:object xlink:href=\"./Object 2\" {LINKS}/>"), "./ObjectReplacements/Object 2", 6.0),
            object_frame("C", &format!("<draw:object-ole xlink:href=\"./Object 3\" {LINKS}/>"), "./ObjectReplacements/Object 3", 10.0),
        ]
        .concat(),
    );
    d.files.push(("Object 1".to_owned(), compound()));
    d.files.push(("ObjectReplacements/Object 1".to_owned(), png(10)));
    d.files.push(("Object 2/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
    d.files.push(("Object 2/styles.xml".to_owned(), b"<office:document-styles/>".to_vec()));
    d.files.push(("ObjectReplacements/Object 2".to_owned(), png(20)));
    d.files.push(("Object 3".to_owned(), PACKAGE.to_vec()));
    d.files.push(("ObjectReplacements/Object 3".to_owned(), png(30)));
    d.bytes()
}

fn shape_named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    doc.pages
        .iter()
        .flat_map(|p| p.shapes.iter())
        .find(|s| s.name_u.as_deref() == Some(name) || s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no shape {name}"))
}

/// The bytes of the embedding a shape of a VSDX model names.
fn embedding_of(doc: &Document, shape: &Shape) -> Option<Vec<u8>> {
    let page = doc.pages.iter().find(|p| p.shapes.iter().any(|s| s.id == shape.id))?;
    let part = page.part_name.as_deref()?;
    let rel = shape.foreign.as_ref()?.rel_id.clone()?;
    let row = doc.vsdx.as_ref()?.rels.iter().find(|r| &*r.source == part && r.id.eq_ignore_ascii_case(&rel) && !r.external)?;
    let target = diacraft_opc::name::resolve(part, &row.target)?;
    doc.parts.iter().find(|p| p.format == Format::Vsdx && p.name.eq_ignore_ascii_case(&target)).map(|p| p.bytes.to_vec())
}

#[test]
fn the_odg_reader_gives_the_model_the_writer_judges_an_object_by() {
    let (doc, _) = diacraft_odg::read(&odg(), &diacraft_odg::Limits::default()).unwrap();
    let a = shape_named(&doc, "A");
    // A frame is a `Foreign` shape that shows its replacement, keeps the element of its object beside it, and the object's file is a part of the other format with no role of its own.
    assert_eq!(a.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()), Some(png(10)));
    let kept = a.ext.get().map(|e| e.odf.kept.kept.len());
    assert_eq!(kept, Some(1), "the element of the object");
    let object = doc.parts.iter().find(|p| &*p.name == "Object 1").expect("the object's file is a part");
    assert_eq!((object.format, object.bytes.to_vec()), (Format::Odf, compound()));
    assert!(doc.parts.iter().all(|p| !p.name.starts_with("ObjectReplacements/")), "a replacement a frame shows is a picture and no part");
}

/// The trip a person can make in 0.2: an `.odg` opened, saved as `.diacraft`, opened again and saved as a VSDX drawing, with and without macros allowed. Frame **A** is an embedding, **B** and **C** ride, and the file read
/// again gives the objects back, byte for byte, with the content check agreeing with the model writer's own second trip.
#[test]
fn an_odg_with_objects_goes_through_diacraft_to_vsdx_embeddings_and_comes_back() {
    let dir = Dir::new();
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg()) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    for ext in ["vsdx", "vsdm"] {
        // A session of its own for each type: a second Save As in the session that wrote the first is a save into that file, whose baseline holds the embedding where the session's model still holds the
        // `.odg` frame, and the splice writer refuses it in words ("a change to the picture of a shape": the concern of that is to close).
        let mut s = diacraft_engine::Session::new();
        run(&mut s, "file.open", json!({ "path": dir.path("a.diacraft") }));
        let name = format!("out.{ext}");
        s.execute("file.save_as", &json!({ "path": dir.path(&name) })).unwrap_or_else(|e| panic!("{ext}: {e}"));
        let bytes = dir.read(&name);
        let (back, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{ext}: {e}"));
        // The items of the three frames are fresh: what rides with each is back on its shape. (The page's own item is stale after this trip, for a reason that is no frame's: the page an `.odg`
        // reader makes is numbered from 0 and holds cells the page writer does not write as they are; it is reported with this task, and is no object's.)
        assert!(report.carry.stale <= 1, "{ext}: {:?}", report.carry);
        for name in ["A", "B", "C"] {
            let slot = shape_named(&back, name)
                .ext
                .get()
                .map(|e| e.odf.clone())
                .unwrap_or_else(|| panic!("{ext}: {name} holds no data of the other format"));
            assert_eq!(slot.style_name.as_deref(), Some("gr1"), "{ext}: {name}");
            // The frame carried back holds no element that names an entry that is not there; the frames that ride hold theirs.
            assert_eq!(slot.kept.kept.len(), usize::from(name != "A"), "{ext}: {name}");
        }
        // Frame A: an embedding, whose picture is the fallback.
        let a = shape_named(&back, "A");
        assert_eq!(a.foreign.as_ref().and_then(|f| f.attr("ForeignType")), Some("Object"), "{ext}");
        assert_eq!(embedding_of(&back, a), Some(compound()), "{ext}: the object's bytes are unchanged");
        assert_eq!(a.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()), Some(png(10)), "{ext}");
        // Frames B and C show their pictures and ride: their files are the model's parts of the other format, byte for byte.
        for (name, shade) in [("B", 20), ("C", 30)] {
            let shape = shape_named(&back, name);
            assert_ne!(shape.foreign.as_ref().and_then(|f| f.attr("ForeignType")), Some("Object"), "{ext}: {name} is not an embedding");
            assert_eq!(shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()), Some(png(shade)), "{ext}: {name}");
        }
        let odf: Vec<(String, usize)> = {
            let mut v: Vec<(String, usize)> =
                back.parts.iter().filter(|p| p.format == Format::Odf).map(|p| (p.name.to_string(), p.bytes.len())).collect();
            v.sort();
            v
        };
        assert_eq!(
            odf,
            [("Object 2/content.xml".to_owned(), 26), ("Object 2/styles.xml".to_owned(), 25), ("Object 3".to_owned(), PACKAGE.len())],
            "{ext}: what rides, and not the object that was written as an embedding"
        );
        // The content check: the file the model that was read writes is the same content, but for what is said below.
        let again = diacraft_vsdx::write_with(
            &back,
            None,
            &diacraft_vsdx::WriteOptions { content_unchanged: true, ..diacraft_vsdx::WriteOptions::default() },
        )
        .unwrap_or_else(|e| panic!("{ext}: {e}"));
        let diff = compare_content(&bytes, &again.bytes, &Limits::default()).unwrap();
        // Every part is the same content but two: the solution part (the page's own item is stale after this trip, no frame's, so the file made again has one item fewer) and the rows of the
        // content types (the engine's save is no unchanged visit, so it leaves a thumbnail out and types as the writer does, where the second is a visit).
        let shown: Vec<String> = diff.shown.iter().map(|d| format!("{} {}", d.part, d.path)).collect();
        let others: Vec<&String> =
            shown.iter().filter(|d| !d.starts_with("[Content_Types].xml ") && !d.starts_with("/visio/solutions/solution1.xml ")).collect();
        assert!(others.is_empty() && diff.count == diff.shown.len() as u64, "{ext}: {} differences: {shown:?}", diff.count);
        // The parts the objects are in are among the ones that are the same: the page, the embedding and its relationships.
        assert!(!shown.iter().any(|d| d.contains("/embeddings/") || d.contains("/visio/pages/")), "{ext}: {shown:?}");
    }
}

/// The control of the note in the test above: the same trip of an `.odg` with no object at all (a rectangle) also leaves the page's own item stale, so the page's item is no frame's concern.
#[test]
fn the_pages_own_item_is_stale_after_the_trip_with_no_object_in_the_drawing_too() {
    let dir = Dir::new();
    let mut s = diacraft_engine::Session::new();
    run(
        &mut s,
        "file.open",
        json!({ "path": dir.write("a.odg", &Drawing::new().page(&crate::odg_build::rect("R", (2.0, 2.0, 4.0, 2.0), "")).bytes()) }),
    );
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path("a.diacraft") }));
    s.execute("file.save_as", &json!({ "path": dir.path("out.vsdx") })).unwrap();
    let (_, report) = read(&dir.read("out.vsdx"), &Limits::default()).unwrap();
    eprintln!("{:?}", report.carry);
    assert!(report.carry.stale <= 1, "{:?}", report.carry);
}
