//! The media type of an EMF picture (owner ruling C2, 2026-11-17): a picture the writer writes is typed `image/emf` ([RFC 7903](https://www.rfc-editor.org/rfc/rfc7903.html),
//! read 2026-11-17), in a package's manifest row, as `draw:mime-type` of a flat document's inline picture and in the conversion of a
//! package to a flat document when its manifest does not say; **a row or an attribute of a file that says `image/x-emf` is kept as the file has it**, and the reader takes
//! both, since it judges a picture by its bytes.

use diacraft_odg::{Form, Limits, OriginalPackage, WriteOptions, read, write_with};

use crate::build::{DRAWING, files, manifest, page, shapes, write};
use crate::flat::{Flat, base64_lines};
use crate::flat_write::text_of;
use crate::write_core::{move_by, open_bytes, part_bytes};
use crate::write_picture::{manifest_rows, named, picture, pictures_of, save};

/// An enhanced metafile as far as the reader looks: 40 bytes, ` EMF`, and more.
fn emf(fill: u8) -> Vec<u8> {
    let mut bytes = vec![0u8; 40];
    bytes.extend_from_slice(b" EMF");
    bytes.resize(96, fill);
    bytes
}

/// A picture frame called `name` whose image names `href`.
fn frame_of(name: &str, href: &str, y: u32) -> String {
    format!(
        "<draw:frame draw:name=\"{name}\" svg:x=\"1cm\" svg:y=\"{y}cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"{href}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>"
    )
}

/// A package of one shape `s1` and a picture frame for each of `pictures` (an entry under `Pictures/` with its bytes and the media type its manifest row gives).
fn package_with(pictures: &[(&str, Vec<u8>, &str)]) -> Vec<u8> {
    let frames: String =
        pictures.iter().enumerate().map(|(at, (name, _, _))| frame_of(&format!("E{at}"), &format!("Pictures/{name}"), 4 + 3 * at as u32)).collect();
    let mut entries = files(&page("p1", &format!("{}{frames}", shapes(1))));
    let mut rows: Vec<(String, &str)> = entries.iter().map(|(name, _)| (name.clone(), "text/xml")).collect();
    for (name, bytes, media) in pictures {
        entries.push((format!("Pictures/{name}"), bytes.clone()));
        rows.push((format!("Pictures/{name}"), *media));
    }
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, m)| (n.as_str(), *m)).collect();
    entries.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
    write(Some(DRAWING), &entries)
}

/// The manifest row of the entry `name`, as written.
fn row_of(bytes: &[u8], name: &str) -> String {
    let rows: Vec<String> = manifest_rows(bytes).into_iter().filter(|r| r.contains(&format!("manifest:full-path=\"{name}\""))).collect();
    assert_eq!(rows.len(), 1, "{name}: {rows:?}");
    rows.into_iter().next().unwrap()
}

/// A new EMF picture is typed `image/emf` by the row the save writes, and the row of the EMF the file already has, which says `image/x-emf`, is kept byte for byte, in the
/// save that adds the new one, in an unedited save and in the save of an edit that has nothing to do with the picture.
#[test]
fn a_new_emf_row_says_image_emf_and_the_row_the_file_has_that_says_image_x_emf_is_kept() {
    let bytes = package_with(&[("old.emf", emf(1), "image/x-emf")]);
    let old = row_of(&bytes, "Pictures/old.emf");
    assert!(old.contains("manifest:media-type=\"image/x-emf\""), "{old}");
    // An unedited save: the input.
    let mut o = open_bytes(bytes.clone());
    let unedited = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!unedited.edited);
    assert_eq!(unedited.bytes, bytes, "an unedited save gives the file back");
    // An unrelated edit: the row and the entry are as they were.
    let id = named(&o.doc, "s1").id;
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let moved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(moved.edited);
    assert_eq!(row_of(&moved.bytes, "Pictures/old.emf"), old, "an unrelated edit keeps the row");
    assert_eq!(part_bytes(&moved.bytes, "Pictures/old.emf"), emf(1));
    // A new EMF picture in the same save as another edit: its row says `image/emf`, and the old row is still the file's.
    let mut o = open_bytes(bytes);
    let id = o.index.next_id(0).unwrap();
    crate::write_new::push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 12.0, 3.0, 2.0), &emf(2)));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let mut pictures = pictures_of(&saved.bytes);
    pictures.sort();
    assert_eq!(pictures, ["Pictures/image1.emf", "Pictures/old.emf"], "one new entry beside the old one");
    let added = row_of(&saved.bytes, "Pictures/image1.emf");
    assert!(added.contains("manifest:media-type=\"image/emf\""), "the new row: {added}");
    assert_eq!(row_of(&saved.bytes, "Pictures/old.emf"), old, "the old row is byte for byte what it was");
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.emf"), emf(2));
}

/// The reader takes an EMF by its bytes, whatever its row says: `image/x-emf`, `image/emf`, another type and none are all an enhanced metafile with the row's type as the
/// picture's (and the bytes' own type when the row says none).
#[test]
fn the_reader_takes_an_emf_typed_image_x_emf_or_image_emf_or_anything_else_as_an_emf() {
    let rows =
        [("a.emf", "image/x-emf", "image/x-emf"), ("b.emf", "image/emf", "image/emf"), ("c.emf", "text/xml", "text/xml"), ("d.emf", "", "image/emf")];
    let bytes = package_with(&rows.map(|(name, media, _)| (name, emf(3), media)));
    let (doc, _) = read(&bytes, &Limits::default()).unwrap();
    for (at, (name, _, expected)) in rows.iter().enumerate() {
        let shape = named(&doc, &format!("E{at}"));
        let foreign = shape.foreign.as_ref().expect("a picture");
        assert_eq!(foreign.attr("ForeignType"), Some("EnhMetaFile"), "{name}");
        let media = foreign.picture.as_ref().expect("its bytes");
        assert_eq!((media.bytes.to_vec(), &*media.content_type), (emf(3), *expected), "{name}");
    }
}

/// The conversion of a package to a flat document types an inline picture by its row when the row says a type (`image/x-emf` as the file has it, `image/emf`), and by the
/// bytes' own type, `image/emf`, when the row says none.
#[test]
fn a_package_written_flat_keeps_the_type_its_row_gives_an_emf_and_types_one_that_has_none_image_emf() {
    let bytes = package_with(&[("a.emf", emf(4), "image/x-emf"), ("b.emf", emf(5), "image/emf"), ("c.emf", emf(6), "")]);
    let o = open_bytes(bytes);
    let saved = write_with(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() },
    )
    .unwrap();
    let text = text_of(&saved.bytes);
    let typed = |fill: u8| {
        let data = base64_lines(&emf(fill), 76, "\n");
        // Characters 64 to 72 are bytes 48 to 54: the fill, which differs from one picture to the next (the first bytes of all of them are zeros).
        let at = text.find(&data[64..72]).unwrap_or_else(|| panic!("the picture {fill} is inline: {text}"));
        let head = &text[..at];
        let start = head.rfind("<draw:image").expect("its image");
        head[start..].split('>').next().unwrap().to_owned()
    };
    assert!(typed(4).contains("draw:mime-type=\"image/x-emf\""), "the file's own type: {}", typed(4));
    assert!(typed(5).contains("draw:mime-type=\"image/emf\""), "{}", typed(5));
    assert!(typed(6).contains("draw:mime-type=\"image/emf\""), "none given: the bytes' own: {}", typed(6));
}

/// A flat document that already has an inline EMF typed `image/x-emf` keeps the attribute as it is through the save of an edit that has nothing to do with it.
#[test]
fn a_flat_document_keeps_the_x_emf_type_of_an_inline_emf_through_an_unrelated_edit() {
    let inline = base64_lines(&emf(7), 76, "\n");
    let picture_frame = format!(
        "<draw:frame draw:name=\"E0\" svg:x=\"1cm\" svg:y=\"4cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image draw:mime-type=\"image/x-emf\"><office:binary-data>{inline}</office:binary-data></draw:image></draw:frame>"
    );
    let drawing = crate::drawing::Drawing::a4().page(&format!("{}{picture_frame}", shapes(1)));
    let mut o = open_bytes(Flat::of(&drawing).bytes());
    let id = named(&o.doc, "s1").id;
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let text = text_of(&saved.bytes);
    assert!(text.contains("<draw:image draw:mime-type=\"image/x-emf\">") && !text.contains("image/emf"), "{text}");
}

/// A flat drawing of one shape and an inline EMF the file types `media`.
fn flat_with_inline_emf(media: &str) -> Vec<u8> {
    let inline = base64_lines(&emf(7), 76, "\n");
    let picture_frame = format!(
        "<draw:frame draw:name=\"E0\" svg:x=\"1cm\" svg:y=\"4cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image draw:mime-type=\"{media}\"><office:binary-data>{inline}</office:binary-data></draw:image></draw:frame>"
    );
    Flat::of(&crate::drawing::Drawing::a4().page(&format!("{}{picture_frame}", shapes(1)))).bytes()
}

/// **An unedited flat file with an inline EMF typed `image/x-emf` saves as the same bytes**, said no edit, and so does the second save of it (the type
/// the file gives is no business of the save's, whichever type the writer would give a new one).
#[test]
fn an_unedited_flat_file_with_an_inline_x_emf_saves_as_the_same_bytes() {
    let bytes = flat_with_inline_emf("image/x-emf");
    let o = open_bytes(bytes.clone());
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!saved.edited, "no edit");
    assert_eq!(saved.bytes, bytes, "the input, byte for byte");
    let again = crate::flat_write::save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == bytes, "and so is the second save");
}

/// **A flat drawing with a new EMF, saved, reopened and saved again is no edit and the same bytes**: the new picture is typed `image/emf`, the
/// reader gives it back as a picture the writer would write as it is (by its bytes, whatever the type), and a save of what was read changes nothing.
#[test]
fn a_flat_drawing_with_a_new_emf_saved_reopened_and_saved_again_is_no_edit_and_the_same_bytes() {
    let drawing = crate::drawing::Drawing::a4().page(&shapes(1));
    let mut o = open_bytes(Flat::of(&drawing).bytes());
    let id = o.index.next_id(0).unwrap();
    crate::write_new::push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 12.0, 3.0, 2.0), &emf(2)));
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let text = text_of(&saved.bytes);
    assert!(text.contains("<draw:image draw:mime-type=\"image/emf\">") && !text.contains("x-emf"), "{text}");
    // Reopened (read from the bytes, with the index of that read) and saved again: no edit, the same bytes; and once more.
    let reopened = open_bytes(saved.bytes.clone());
    let (_, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new(), "it reads with no warning");
    let (kind, media) = {
        let shape = named(&reopened.doc, "Picture.1");
        let foreign = shape.foreign.as_ref().expect("a picture");
        (foreign.attr("ForeignType").map(str::to_owned), foreign.picture.as_ref().map(|m| (m.bytes.to_vec(), m.content_type.to_string())))
    };
    assert_eq!(
        (kind.as_deref(), media),
        (Some("EnhMetaFile"), Some((emf(2), "image/emf".to_owned()))),
        "read back as the EMF it is, typed as written"
    );
    let second = crate::flat_write::save(&reopened.doc, &reopened.bytes, &reopened.index).unwrap();
    assert!(!second.edited, "the second save is no edit");
    assert_eq!(second.bytes, saved.bytes, "and the same bytes");
    let third = crate::flat_write::save(&reopened.doc, &second.bytes, &second.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes, "and so is the third");
}
