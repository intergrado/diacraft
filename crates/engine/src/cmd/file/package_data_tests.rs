//! A `.diacraft` made from a drawing opened from an `.odg` carries the package data of the file the drawing stands on **as it is now**, after a save too: the
//! generator, the thumbnail and the rows of the package that the save wrote.
//!
//! These tests call [`save_native`](super::save_native) itself, the function the save routes to for a drawing opened from an `.odg` lifted the refusal, on a session
//! whose file is an `.odg`; the engine's tests of the table are in `native_file.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_odg::{Limits, PackageData};
use serde_json::json;

use super::{NativeKind, save_native};
use crate::Session;

const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#
);

/// A drawing of one rectangle with a producer's generator, a title, a date and editing cycles, an old thumbnail and a font, and a manifest that names them.
fn drawing() -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles/><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\"><draw:rect draw:name=\"R\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"/></draw:page></office:drawing></office:body></office:document-content>"
    );
    let styles = format!(
        "<?xml version=\"1.0\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<?xml version=\"1.0\"?><office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>LibreOffice/7.3</meta:generator><dc:title>A title</dc:title><dc:date>2024-06-07T08:09:10</dc:date><meta:editing-cycles>7</meta:editing-cycles></office:meta></office:document-meta>"
    );
    let manifest = format!(
        "<?xml version=\"1.0\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"Thumbnails/thumbnail.png\" manifest:media-type=\"image/png\"/><manifest:file-entry manifest:full-path=\"Fonts/f.ttf\" manifest:media-type=\"application/x-font-ttf\" manifest:size=\"10\"/></manifest:manifest>"
    );
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, b'o', b'l', b'd'];
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("meta.xml", meta.as_bytes(), Method::Deflated).unwrap();
    w.add("Thumbnails/thumbnail.png", &png, Method::Stored).unwrap();
    w.add("Fonts/f.ttf", b"font bytes", Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

fn folder() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("diacraft-engine-package-data-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The package data a `.odg`'s bytes read as.
fn data_of_odg(bytes: &[u8]) -> PackageData {
    diacraft_odg::read_package_data(bytes, &Limits::default()).unwrap()
}

/// The package data of a `.diacraft` file: the model it opens as.
fn data_of_diacraft(path: &std::path::Path) -> PackageData {
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap();
    PackageData::of(s.document().unwrap())
}

/// **The plan's check, as far as it can be run before the lift:** a drawing is opened from an `.odg`, edited and saved (the save writes DiaCraft's generator and renews the
/// thumbnail), and then saved as `.diacraft`: the package rows of the `.diacraft` are those of the file on disk, and not those the drawing was read with. And a second Save As
/// after a further edit and an undo of it is the same.
#[test]
fn a_diacraft_made_after_an_odg_save_carries_the_package_data_of_the_file_written() {
    let dir = folder();
    let odg = dir.join("a.odg");
    std::fs::write(&odg, drawing()).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": odg.to_str().unwrap() })).unwrap();
    let as_read = PackageData::of(s.document().unwrap());
    s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 4.0 })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let on_disk = data_of_odg(&std::fs::read(&odg).unwrap());
    assert_ne!(on_disk, as_read, "the save wrote a generator and a thumbnail of its own");
    // The model the session holds is the drawing as it was read: it does not hold the file's package data now.
    assert_eq!(PackageData::of(s.document().unwrap()), as_read);
    // Save As `.diacraft`, through the function the lift routes to.
    let first = dir.join("one.diacraft");
    save_native(&mut s, "file.save_as", &first, first.clone(), NativeKind::Drawing).unwrap();
    assert_eq!(data_of_diacraft(&first), on_disk, "the `.diacraft` carries the package rows of the file on disk");
}

/// The same after an edit that was undone: the version of the drawing is an earlier one, whose package data is the one it was read with, and the file is what it is.
#[test]
fn an_undo_does_not_bring_the_old_package_data_back() {
    let dir = folder();
    let odg = dir.join("a.odg");
    std::fs::write(&odg, drawing()).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": odg.to_str().unwrap() })).unwrap();
    s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 4.0 })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let on_disk = data_of_odg(&std::fs::read(&odg).unwrap());
    // Undo back to the drawing as it was read: its model holds the package data it was read with.
    s.execute("edit.undo", &json!({})).unwrap();
    let second = dir.join("two.diacraft");
    save_native(&mut s, "file.save_as", &second, second.clone(), NativeKind::Drawing).unwrap();
    assert_eq!(data_of_diacraft(&second), on_disk, "the package rows are the file's, whatever version of the drawing is written");
}

/// A drawing nobody saved is written with the package data it was read with: nothing is read again to make it, and it is the same file the model gives.
#[test]
fn a_drawing_not_saved_since_it_was_opened_carries_the_package_data_it_was_read_with() {
    let dir = folder();
    let odg = dir.join("a.odg");
    std::fs::write(&odg, drawing()).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": odg.to_str().unwrap() })).unwrap();
    let as_read = PackageData::of(s.document().unwrap());
    let out = dir.join("one.diacraft");
    save_native(&mut s, "file.save_as", &out, out.clone(), NativeKind::Drawing).unwrap();
    assert_eq!(data_of_diacraft(&out), as_read);
}

/// A save that cannot read the file it stands on again says so in words and writes nothing: a file that does not read, and a room that is not there. A drawing that stands on no
/// `.odg` is not asked for any.
#[test]
fn a_file_that_cannot_be_read_again_refuses_the_save_in_words_and_a_drawing_of_no_odg_asks_for_nothing() {
    use crate::file::OpenFile;
    use crate::odgfile::{OdgState, file_package_data, no_room_words};
    let odg =
        |bytes: &[u8]| OpenFile::new(std::path::Path::new("a.odg"), bytes.to_vec()).with_odg(OdgState::new(diacraft_odg::ShapeIndex::default()));
    let words = file_package_data(&odg(b"not a package")).unwrap_err();
    assert!(words.starts_with("the drawing could not be saved: the file it was opened from cannot be read again: "), "{words}");
    assert!(words.len() < 400);
    assert!(matches!(file_package_data(&OpenFile::new(std::path::Path::new("a.vsdx"), b"x".to_vec())), Ok(None)), "a file that is no `.odg`");
    assert!(no_room_words(1_000, 1_000).is_none() && no_room_words(0, 0).is_none(), "what fits is read");
    let words = no_room_words(3 << 30, 1 << 30).expect("three gigabytes in one");
    assert!(words.contains("3072 MiB") && words.contains("1024 MiB") && words.ends_with("Nothing was written"), "{words}");
    assert!(no_room_words(u64::MAX, 0).is_some() && no_room_words(1, 0).is_some());
}

// ------------------------------------------------------------------------------------------------------------------------ the named data

/// What a model holds of the document's named data: the definitions, the layer set, the style elements that are no style sheet, the font table and what the containers hold that
/// no field reads.
type Named = (
    Vec<diacraft_doc::Definition>,
    Vec<std::sync::Arc<diacraft_doc::RawElement>>,
    Vec<diacraft_doc::KeptStyle>,
    Vec<diacraft_doc::FaceName>,
    diacraft_doc::Kept,
);

fn named(doc: &diacraft_doc::Document) -> Named {
    let odf = doc.ext.get().map(|d| d.odf.clone()).unwrap_or_default();
    (odf.definitions, odf.layers, odf.styles, (*doc.fonts).clone(), odf.kept)
}

fn named_of_odg(bytes: &[u8]) -> Named {
    named(&diacraft_odg::read(bytes, &Limits::default()).unwrap().0)
}

fn named_of_diacraft(path: &std::path::Path) -> Named {
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap();
    named(s.document().unwrap())
}

/// Opens `drawing()`, adds a connector between two rectangles and gives the page another size (the save writes a `draw:marker` for the line end and a page layout of the size) and
/// saves it.
fn odg_with_a_save_that_wrote_definitions() -> (Session, PathBuf, Named, Named) {
    let dir = folder();
    let odg = dir.join("a.odg");
    std::fs::write(&odg, drawing()).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": odg.to_str().unwrap() })).unwrap();
    let as_read = named(s.document().unwrap());
    let id = |v: serde_json::Value| u32::try_from(v["id"].as_u64().unwrap()).unwrap();
    let a = id(s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 4.0 })).unwrap());
    let b = id(s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 7.0, "y": 4.0 })).unwrap());
    s.execute("connector.add", &json!({ "from": { "id": a }, "to": { "id": b } })).unwrap();
    // A page of another size is written with a page layout of its own.
    s.execute("page.size", &json!({ "preset": "letter" })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let on_disk = named_of_odg(&std::fs::read(&odg).unwrap());
    (s, dir, as_read, on_disk)
}

/// **The named data follows the file where a `.diacraft` is written.** A save writes a marker definition and styles of its own into the file; the drawing the session holds is the
/// one it was read as, and the `.diacraft` made after the save carries what the file on disk holds (and not the definitions and styles as the file had them when it was read).
#[test]
fn a_diacraft_made_after_an_odg_save_carries_the_definitions_and_styles_of_the_file_written() {
    let (mut s, dir, as_read, on_disk) = odg_with_a_save_that_wrote_definitions();
    assert_ne!(on_disk.0, as_read.0, "the save wrote a marker definition the file had not");
    assert_ne!(on_disk.2, as_read.2, "and the page layout of the size it gave the page");
    assert_eq!(named(s.document().unwrap()), as_read, "the model the session holds is the drawing as it was read");
    let first = dir.join("one.diacraft");
    save_native(&mut s, "file.save_as", &first, first.clone(), NativeKind::Drawing).unwrap();
    assert_eq!(named_of_diacraft(&first), on_disk, "definitions, layers, styles, fonts and kept containers are the file's");
}

/// The same after an undo: the version of the drawing is the one it was read as, and the file is what it is.
#[test]
fn an_undo_does_not_bring_the_old_definitions_and_styles_back() {
    let (mut s, dir, _as_read, on_disk) = odg_with_a_save_that_wrote_definitions();
    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    let second = dir.join("two.diacraft");
    save_native(&mut s, "file.save_as", &second, second.clone(), NativeKind::Drawing).unwrap();
    assert_eq!(named_of_diacraft(&second), on_disk, "the named data is the file's, whatever version of the drawing is written");
}
