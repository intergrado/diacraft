//! owner rulings B1 and C2, 2026-11-17.
//!
//! **B1.** An object the file holds in an element of its own (an embedded object, a table, a 3-D scene) and a copy of one, or one an undo brings back after a save that
//! deleted it, is **refused in words** when the drawing is saved as an `.odg`, whether or not the object has a replacement picture: its element and the files that go with
//! it are not copied yet, and a save could only write a plain picture that has lost them. (refused an object with **no**
//! picture, which the reader marks with a `User` row; one with a picture has no marker, and its copy was saved as a plain picture, silently.)
//!
//! **C2.** A new EMF picture is typed `image/emf` ([RFC 7903](https://www.rfc-editor.org/rfc/rfc7903.html), read 2026-11-17), and a row of a file that says `image/x-emf`
//! is kept as the file has it.

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Document, RawNode, Shape};
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing, id_of, open_bytes_in, part_bytes, part_of, rect};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, refused, save, shift};
use crate::run;

/// A small PNG.
fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200u8; 16]).unwrap();
    out
}

/// An enhanced metafile as far as the reader looks: 40 bytes and then the signature ` EMF`, and some more.
fn emf() -> Vec<u8> {
    let mut bytes = vec![0u8; 40];
    bytes.extend_from_slice(b" EMF");
    bytes.resize(88, 7);
    bytes
}

/// A part of a package, with the media type its manifest row gives.
type Part = (&'static str, Vec<u8>, &'static str);

const LINKS: &str = "xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"";

/// A frame holding the object `./Object 1` (the element `object`, as `draw:object` or `table:table`) and the replacement picture `./ObjectReplacements/Object 1`.
fn object_frame(name: &str, object: &str) -> String {
    format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"9cm\">{object}<draw:image xlink:href=\"./ObjectReplacements/Object 1\" {LINKS}/></draw:frame>"
    )
}

/// The element of an embedded object (a folder of the package).
fn embedded() -> String {
    format!("<draw:object xlink:href=\"./Object 1\" {LINKS}/>")
}

/// A picture frame whose `draw:image` names `href`.
fn picture_frame(name: &str, href: &str, x: f64) -> String {
    format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"3cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"15cm\"><draw:image xlink:href=\"{href}\" {LINKS}/></draw:frame>"
    )
}

/// A package of the drawing whose page holds `shapes`, with `parts` besides the drawing's own, each with the media type its manifest row gives. A part under `Object 1/`
/// puts the folder's row in too, as a package that holds an object has it.
fn package_of(shapes: &str, parts: &[Part]) -> Vec<u8> {
    let drawing = Drawing::new().page(shapes);
    // The namespace of a table, which `odg_build` does not declare.
    let content = drawing.content_xml().replacen(
        "<office:document-content ",
        "<office:document-content xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" ",
        1,
    );
    let mut files: Vec<(String, Vec<u8>, &str)> = vec![
        ("content.xml".to_owned(), content.into_bytes(), "text/xml"),
        ("styles.xml".to_owned(), drawing.styles_xml().into_bytes(), "text/xml"),
        ("meta.xml".to_owned(), drawing.meta_xml().into_bytes(), "text/xml"),
    ];
    files.extend(parts.iter().map(|(name, bytes, media)| ((*name).to_owned(), bytes.clone(), *media)));
    let mut manifest = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
    );
    manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{DRAWING}\"/>\n"));
    if parts.iter().any(|(name, _, _)| name.starts_with("Object 1/")) {
        manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"Object 1/\" manifest:media-type=\"{DRAWING}\"/>\n"));
    }
    for (name, _, media) in &files {
        manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media}\"/>\n"));
    }
    manifest.push_str("</manifest:manifest>\n");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    for (name, data, media) in &files {
        w.add(name, data, if media.starts_with("image/") { Method::Stored } else { Method::Deflated }).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// The parts that go with the object frame: its folder's document and its replacement picture.
fn object_parts(replacement: Vec<u8>, media: &'static str) -> Vec<Part> {
    vec![("Object 1/content.xml", b"<office:document-content/>".to_vec(), "text/xml"), ("ObjectReplacements/Object 1", replacement, media)]
}

/// A drawing of the rectangle `A`, the object `O` with a replacement picture, and the picture `P`.
fn fixture() -> Vec<u8> {
    let shapes = [rect("A", (2.0, 3.0, 4.0, 2.0), ""), object_frame("O", &embedded()), picture_frame("P", "Pictures/p.png", 12.0)].concat();
    let mut parts = object_parts(png(), "image/png");
    parts.push(("Pictures/p.png", png(), "image/png"));
    package_of(&shapes, &parts)
}

/// Whether the model's `shape` is what the reader gives an object with a replacement picture: a picture frame (foreign data with a picture), **no** `ODFObject` row (that
/// is for an object with no picture), and the element of the object among the children the shape keeps.
fn is_object_with_picture(shape: &Shape) -> bool {
    let has_row = shape.sheet.sections.iter().any(|s| s.name.as_str() == "User" && s.rows.iter().any(|r| r.name.as_deref() == Some("ODFObject")));
    // The children the reader keeps are in the shape's `odf` slot.
    let kept = shape.ext.get().map(|d| d.odf.kept.kept.clone()).unwrap_or_default();
    let keeps_object = kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if matches!(&*e.name, "draw:object" | "table:table")));
    shape.foreign.as_ref().is_some_and(|f| f.picture.is_some()) && !has_row && keeps_object
}

/// The shape called `name` on the first page.
fn named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}"))
}

/// The last shape of the first page: what `shape.duplicate` just made.
fn last(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().next_back().unwrap()
}

/// The names of the entries of a package.
fn entries(bytes: &[u8]) -> Vec<String> {
    diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap().entries().map(|e| e.name.clone()).collect()
}

/// The manifest rows of the package that name `needle`, as written.
fn rows_naming(bytes: &[u8], needle: &str) -> Vec<String> {
    part_of(bytes, "META-INF/manifest.xml").lines().filter(|l| l.contains(needle)).map(str::to_owned).collect()
}

/// What the refusal says of an object: it says what it is, that it is not copied, and names nothing the file holds.
fn assert_refused_as_an_object(said: &str) {
    assert!(said.contains("an object") && said.contains("not copied") && !said.contains("Object 1") && !said.contains("\"O\""), "{said}");
}

/// B1: **a copy of an object that has a replacement picture is refused in words**, and nothing is written. It has no marker row, and it was saved as a plain picture that has
/// lost the object's element and its files (the shape still showed its picture, and nothing said the object was gone). The way out is the usual one: undo the copy.
#[test]
fn a_copy_of_an_object_that_has_a_replacement_picture_is_refused_in_words_and_nothing_is_written() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &fixture());
    let o = id_of(s.document().unwrap(), 0, "O");
    // What the test stands on: the object is a picture with its element kept and no marker, and so is its copy (a duplicate carries what the shape keeps).
    assert!(is_object_with_picture(named(s.document().unwrap(), "O")), "the object as the reader gives it");
    let before = dir.read("a.odg");
    run(&mut s, "shape.duplicate", json!({ "ids": [o] }));
    assert!(is_object_with_picture(last(s.document().unwrap())), "the copy keeps the element of the object and has no marker");
    let said = refused(&mut s);
    assert_refused_as_an_object(&said);
    assert!(said.contains("edit.undo"), "the usual way out: {said}");
    assert_eq!(dir.read("a.odg"), before, "nothing was written");
    // Undone, the drawing is the file's again and saves; a move of the object itself saves by its frame, and its files are as they were.
    run(&mut s, "edit.undo", json!({}));
    shift(&mut s, "O", 1.0, 0.5);
    assert_eq!(save(&mut s)["edited"], true);
    let saved = dir.read("a.odg");
    for name in ["Object 1/content.xml", "ObjectReplacements/Object 1"] {
        assert_eq!(part_bytes(&saved, name), part_bytes(&before, name), "{name} is as it was");
    }
}

/// B1: the same for **an object that has a picture and that an undo brought back** after a save that deleted it: the save took its element and its files out of the package,
/// so what the undo brings back is as new as a copy is.
#[test]
fn an_object_with_a_replacement_picture_that_a_save_deleted_and_an_undo_brought_back_is_refused() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &fixture());
    let o = id_of(s.document().unwrap(), 0, "O");
    run(&mut s, "shape.delete", json!({ "ids": [o] }));
    assert_eq!(save(&mut s)["edited"], true);
    let saved = dir.read("a.odg");
    let names = entries(&saved);
    assert!(
        !names.iter().any(|n| n.starts_with("Object 1") || n.starts_with("ObjectReplacements")) && rows_naming(&saved, "Object 1").is_empty(),
        "the object's entries and rows went with it: {names:?}"
    );
    run(&mut s, "edit.undo", json!({}));
    assert!(is_object_with_picture(named(s.document().unwrap(), "O")), "the object is back in the model, with its picture");
    let said = refused(&mut s);
    assert_refused_as_an_object(&said);
    assert_eq!(dir.read("a.odg"), saved, "nothing was written");
    // Deleted again, the drawing is the file's.
    run(&mut s, "shape.delete", json!({ "ids": [o] }));
    save(&mut s);
}

/// B1: an object whose replacement picture is in a format DiaCraft does not draw (an SVG: the reader gives it the picture, no `ForeignType` and no marker) is refused as an
/// object too, and not by the words of a picture the writer cannot type. And an object that is a table, with a picture of its own.
#[test]
fn a_copy_of_an_object_whose_picture_is_not_one_that_is_drawn_or_that_is_a_table_is_refused_as_an_object() {
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>".to_vec();
    let table = "<table:table table:name=\"T\"/>";
    let cases: [(&str, String, Vec<Part>); 3] = [
        ("an SVG replacement", object_frame("O", &embedded()), object_parts(svg, "image/svg+xml")),
        ("a table with a picture", object_frame("O", table), object_parts(png(), "image/png")),
        ("an embedded object with an EMF picture", object_frame("O", &embedded()), object_parts(emf(), "image/x-emf")),
    ];
    for (what, frame, parts) in cases {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &package_of(&[rect("A", (2.0, 3.0, 4.0, 2.0), ""), frame].concat(), &parts));
        let o = id_of(s.document().unwrap(), 0, "O");
        let shape = named(s.document().unwrap(), "O");
        assert!(shape.foreign.as_ref().is_some_and(|f| f.picture.is_some()), "{what}: the object has a picture");
        let before = dir.read("a.odg");
        run(&mut s, "shape.duplicate", json!({ "ids": [o] }));
        let said = refused(&mut s);
        assert!(said.contains("an object") && said.contains("not copied") && !said.contains("reader does not draw"), "{what}: {said}");
        assert_eq!(dir.read("a.odg"), before, "{what}: nothing was written");
    }
}

/// Control: **a copy of a plain picture still saves**: a picture frame is no object, whatever its picture is. The copy is a new picture entry (a picture the file holds is
/// not reused), and the object beside it keeps its entries.
#[test]
fn a_copy_of_a_plain_picture_is_saved_as_before_and_the_object_beside_it_keeps_its_files() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &fixture());
    let p = id_of(s.document().unwrap(), 0, "P");
    let before = dir.read("a.odg");
    assert!(!is_object_with_picture(named(s.document().unwrap(), "P")), "the picture frame is no object");
    run(&mut s, "shape.duplicate", json!({ "ids": [p] }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let bytes = dir.read("a.odg");
    let pictures: Vec<String> = entries(&bytes).into_iter().filter(|n| n.starts_with("Pictures/")).collect();
    assert_eq!(pictures.len(), 2, "the picture of the file and the copy's own: {pictures:?}");
    for name in ["Object 1/content.xml", "ObjectReplacements/Object 1", "Pictures/p.png"] {
        assert_eq!(part_bytes(&bytes, name), part_bytes(&before, name), "{name} is as it was");
    }
}

/// Control: **an object of the file that is left alone saves as it was**: a save after an edit of another shape keeps its element, its files and its manifest rows
/// byte for byte, and so does a save after the object itself is moved.
#[test]
fn an_object_of_the_file_that_is_not_copied_keeps_its_element_its_files_and_its_rows() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &fixture());
    let before = dir.read("a.odg");
    let rows = rows_naming(&before, "Object 1");
    assert_eq!(rows.len(), 3, "the folder, its document and its replacement: {rows:?}");
    for edit in ["A", "O"] {
        shift(&mut s, edit, 1.0, 0.5);
        assert_eq!(save(&mut s)["edited"], true, "{edit}");
        let bytes = dir.read("a.odg");
        assert_eq!(rows_naming(&bytes, "Object 1"), rows, "{edit}: the rows");
        for name in ["Object 1/content.xml", "ObjectReplacements/Object 1"] {
            assert_eq!(part_bytes(&bytes, name), part_bytes(&before, name), "{edit}: {name}");
        }
        let content = part_of(&bytes, "content.xml");
        assert!(content.contains("<draw:object xlink:href=\"./Object 1\""), "{edit}: the element stays: {content}");
    }
}

/// C2: a new EMF picture is typed `image/emf` by the manifest row the save writes (RFC 7903), **and the row of the EMF the file already has says what the file says** (`image/x-emf`,
/// the type the files this was seen in carry), byte for byte: the copy of the file's EMF picture is a new entry, and the old row is no business of the save.
#[test]
fn a_copy_of_an_emf_picture_is_typed_image_emf_and_the_old_row_that_says_image_x_emf_is_kept() {
    let shapes = [rect("A", (2.0, 3.0, 4.0, 2.0), ""), picture_frame("E", "Pictures/e.emf", 12.0)].concat();
    let package = package_of(&shapes, &[("Pictures/e.emf", emf(), "image/x-emf")]);
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &package);
    let e = id_of(s.document().unwrap(), 0, "E");
    let old = rows_naming(&package, "Pictures/e.emf");
    assert_eq!(old.len(), 1, "{old:?}");
    assert!(old[0].contains("manifest:media-type=\"image/x-emf\""), "{old:?}");
    // An unrelated edit and a save: the row is as it was.
    shift(&mut s, "A", 1.0, 0.5);
    assert_eq!(save(&mut s)["edited"], true);
    assert_eq!(rows_naming(&dir.read("a.odg"), "Pictures/e.emf"), old, "an unrelated edit keeps the row");
    // The copy of the picture is a new EMF entry, typed as RFC 7903 types it, and the file's own row is still the file's.
    run(&mut s, "shape.duplicate", json!({ "ids": [e] }));
    assert_eq!(save(&mut s)["edited"], true);
    let bytes = dir.read("a.odg");
    assert_eq!(rows_naming(&bytes, "Pictures/e.emf"), old, "the old row is byte for byte what it was");
    let added = rows_naming(&bytes, "Pictures/image1.emf");
    assert_eq!(added.len(), 1, "one row for the new entry: {added:?}");
    assert!(added[0].contains("manifest:media-type=\"image/emf\""), "{added:?}");
    assert_eq!(part_bytes(&bytes, "Pictures/image1.emf"), Some(emf()));
    // Nothing compares the type the model holds for the copy (the file's `image/x-emf`) with the row written (`image/emf`): the file reopens as the session has it, and a
    // second save is no edit.
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// With B1: a **new group holds its members' copies**, so the refusal of an object that is not in the file reaches the members too, **whether or not the
/// object has a replacement picture** (the first has no marker row, the second has the `ODFObject` row). A group made of the file's shapes `A` and the object `O` and then
/// duplicated is a new group holding a copy of `O`: the save refuses it in the words of an object, and writes nothing. The group that holds the file's own `O` (grouped, not
/// copied) is no copy of an object, and is not refused for one.
#[test]
fn a_copy_of_a_group_that_holds_an_object_is_refused_as_an_object_and_a_group_of_the_files_own_object_is_not() {
    // An object with no picture: the frame holds the element and no `draw:image`, and the package no replacement.
    let bare = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"O\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"9cm\">{}</draw:frame>",
        embedded()
    );
    let no_picture = package_of(
        &[rect("A", (2.0, 3.0, 4.0, 2.0), ""), bare].concat(),
        &[("Object 1/content.xml", b"<office:document-content/>".to_vec(), "text/xml")],
    );
    for (what, bytes) in [("an object with a replacement picture", fixture()), ("an object with no picture", no_picture)] {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &bytes);
        let (a, o) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "O"));
        let before = dir.read("a.odg");
        let group = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a, o] }));
        // The file's own shapes put into a group: refused as that, and not as an object that is not in the file.
        let said = refused(&mut s);
        assert!(!said.contains("not copied"), "{what}: the file's own object in a new group is not a copy of an object: {said}");
        assert_eq!(dir.read("a.odg"), before, "{what}: nothing was written");
        // The group duplicated holds a copy of the object: refused as an object.
        run(&mut s, "shape.duplicate", json!({ "ids": [group["id"]] }));
        let said = refused(&mut s);
        assert_refused_as_an_object(&said);
        assert!(said.contains("edit.undo"), "{what}: the usual way out: {said}");
        assert_eq!(dir.read("a.odg"), before, "{what}: nothing was written");
    }
}

// ---- review: a text frame that holds an object frame ----------------------------------------------------------------------------------------

/// Where a text frame holds the frame of an object: as a direct child of its `draw:text-box`, or inside the paragraph the text is in (anchored to it, as a frame is).
#[derive(Clone, Copy, Debug)]
enum Place {
    DirectChild,
    InParagraph,
}

/// A text frame `T` whose `draw:text-box` holds the text "hello" and `nested`, in the place `place` says.
fn text_frame_holding(place: Place, nested: &str) -> String {
    let inside = match place {
        Place::DirectChild => format!("<text:p>hello</text:p>{nested}"),
        Place::InParagraph => format!("<text:p>hello{nested}</text:p>"),
    };
    format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"T\" svg:width=\"6cm\" svg:height=\"4cm\" svg:x=\"2cm\" svg:y=\"9cm\"><draw:text-box>{inside}</draw:text-box></draw:frame>"
    )
}

/// The frame of the object `./Object 1` and its replacement picture, as a text frame holds it.
fn nested_object_frame() -> String {
    format!(
        "<draw:frame draw:name=\"N\" svg:width=\"2cm\" svg:height=\"1cm\" svg:x=\"0cm\" svg:y=\"0cm\">{}<draw:image xlink:href=\"./ObjectReplacements/Object 1\" {LINKS}/></draw:frame>",
        embedded()
    )
}

/// A drawing of the rectangle `A` and the text frame `T`, in a package that holds the object's files.
fn text_fixture(place: Place, nested: &str) -> Vec<u8> {
    package_of(&[rect("A", (2.0, 3.0, 4.0, 2.0), ""), text_frame_holding(place, nested)].concat(), &object_parts(png(), "image/png"))
}

/// Review: **a copy of a text frame that holds the frame of an object is refused in words**, in either place the frame can be in, and nothing is written. The
/// copy kept the frame (when it was a direct child of the text box) with a note that one element was not written, or kept nothing of it and said nothing (when it was in the
/// paragraph, which the reader reads only the text of): either way the copy lost an object without a word. The text frame of the file, and a move of it, are as they were.
#[test]
fn a_copy_of_a_text_frame_that_holds_the_frame_of_an_object_as_a_direct_child_is_refused_in_words_and_nothing_is_written() {
    a_copy_of_a_text_frame_that_holds_the_frame_of_an_object_is_refused(Place::DirectChild);
}

/// The same, with the frame inside the paragraph the text is in.
#[test]
fn a_copy_of_a_text_frame_that_holds_the_frame_of_an_object_in_its_paragraph_is_refused_in_words_and_nothing_is_written() {
    a_copy_of_a_text_frame_that_holds_the_frame_of_an_object_is_refused(Place::InParagraph);
}

#[track_caller]
fn a_copy_of_a_text_frame_that_holds_the_frame_of_an_object_is_refused(place: Place) {
    {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &text_fixture(place, &nested_object_frame()));
        let t = id_of(s.document().unwrap(), 0, "T");
        let shape = named(s.document().unwrap(), "T");
        assert!(shape.foreign.is_none() && shape.text.is_some(), "{place:?}: the text frame is a text frame, with its text");
        let before = dir.read("a.odg");
        run(&mut s, "shape.duplicate", json!({ "ids": [t] }));
        let said = refused(&mut s);
        assert_refused_as_an_object(&said);
        assert!(said.contains("text box") && said.contains("edit.undo"), "{place:?}: {said}");
        assert_eq!(dir.read("a.odg"), before, "{place:?}: nothing was written");
        // Undone, the drawing saves, and a move of the text frame itself saves with the object's files as they were.
        run(&mut s, "edit.undo", json!({}));
        shift(&mut s, "T", 1.0, 0.5);
        assert_eq!(save(&mut s)["edited"], true, "{place:?}");
        let saved = dir.read("a.odg");
        for name in ["Object 1/content.xml", "ObjectReplacements/Object 1"] {
            assert_eq!(part_bytes(&saved, name), part_bytes(&before, name), "{place:?}: {name} is as it was");
        }
        assert!(part_of(&saved, "content.xml").contains("<draw:object xlink:href=\"./Object 1\""), "{place:?}: the object stays in the text frame");
    }
}

/// Review: the same for a text frame that holds the frame of an object and **an undo brought back** after a save deleted it: it is not in the file any more,
/// so it is as new as a copy.
#[test]
fn a_text_frame_that_holds_the_frame_of_an_object_as_a_direct_child_and_that_an_undo_brought_back_is_refused() {
    a_text_frame_that_holds_the_frame_of_an_object_and_that_an_undo_brought_back_is_refused(Place::DirectChild);
}

/// The same, with the frame inside the paragraph the text is in.
#[test]
fn a_text_frame_that_holds_the_frame_of_an_object_in_its_paragraph_and_that_an_undo_brought_back_is_refused() {
    a_text_frame_that_holds_the_frame_of_an_object_and_that_an_undo_brought_back_is_refused(Place::InParagraph);
}

#[track_caller]
fn a_text_frame_that_holds_the_frame_of_an_object_and_that_an_undo_brought_back_is_refused(place: Place) {
    {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &text_fixture(place, &nested_object_frame()));
        let t = id_of(s.document().unwrap(), 0, "T");
        run(&mut s, "shape.delete", json!({ "ids": [t] }));
        assert_eq!(save(&mut s)["edited"], true, "{place:?}");
        let saved = dir.read("a.odg");
        run(&mut s, "edit.undo", json!({}));
        let said = refused(&mut s);
        assert_refused_as_an_object(&said);
        assert_eq!(dir.read("a.odg"), saved, "{place:?}: nothing was written");
        // Deleted again, the drawing is the file's.
        run(&mut s, "shape.delete", json!({ "ids": [t] }));
        save(&mut s);
    }
}

/// Controls: what holds no object is copied as before. A text frame with nothing but its text, one that holds the frame of a **picture** (in either place), and one that
/// holds a frame that has no children at all are copies a save writes (the picture of the nested frame is not copied, as it never was: the note says an element was not
/// written, in the one place the reader keeps it).
#[test]
fn a_copy_of_a_text_frame_that_holds_no_object_is_saved_as_before() {
    let picture =
        format!("<draw:frame draw:name=\"N\" svg:width=\"2cm\" svg:height=\"1cm\"><draw:image xlink:href=\"Pictures/p.png\" {LINKS}/></draw:frame>");
    let empty = "<draw:frame draw:name=\"N\" svg:width=\"2cm\" svg:height=\"1cm\"/>".to_owned();
    for (what, place, nested) in [
        ("no frame", Place::InParagraph, String::new()),
        ("a picture frame, as a direct child", Place::DirectChild, picture.clone()),
        ("a picture frame, in the paragraph", Place::InParagraph, picture),
        ("a frame with no children, in the paragraph", Place::InParagraph, empty),
    ] {
        let dir = Dir::new();
        let shapes = [rect("A", (2.0, 3.0, 4.0, 2.0), ""), text_frame_holding(place, &nested)].concat();
        let mut s = open_bytes_in(&dir, "a.odg", &package_of(&shapes, &[("Pictures/p.png", png(), "image/png")]));
        let t = id_of(s.document().unwrap(), 0, "T");
        run(&mut s, "shape.duplicate", json!({ "ids": [t] }));
        assert_eq!(save(&mut s)["edited"], true, "{what}");
    }
}

/// **a copy of a group that holds a text frame that holds the frame of an object** is refused in the words of an object too, in either place the
/// frame can be in: the members of a new group are looked at by the same `is_object` as the shapes of the page are (a text frame has no marker but the `User` row the reader
/// gives it). The file's own `A` and `T` put into a group are refused as shapes put into a group, and not as an object that is not in the file.
#[test]
fn a_copy_of_a_group_that_holds_a_text_frame_that_holds_the_frame_of_an_object_is_refused_as_an_object() {
    for place in [Place::DirectChild, Place::InParagraph] {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &text_fixture(place, &nested_object_frame()));
        let (a, t) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "T"));
        let before = dir.read("a.odg");
        let group = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a, t] }));
        let said = refused(&mut s);
        assert!(!said.contains("not copied"), "{place:?}: the file's own text frame in a new group is not a copy of an object: {said}");
        run(&mut s, "shape.duplicate", json!({ "ids": [group["id"]] }));
        let said = refused(&mut s);
        assert_refused_as_an_object(&said);
        assert!(said.contains("text box") && said.contains("edit.undo"), "{place:?}: {said}");
        assert_eq!(dir.read("a.odg"), before, "{place:?}: nothing was written");
    }
}
