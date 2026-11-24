//! Our own writers' output compares the same (the review's named risk 3): a drawing saved by `write_with` as a package and as a flat file,
//! as it stood, edited, and edited from the flat file, is the same content, whatever the drawing holds (faces in both parts, a linked picture, a thumbnail).
//! The check is what a person is told when a save in the other form "changed nothing".

use diacraft_odg::{Form, OriginalPackage, ShapeIndex, WriteOptions, compare_content, write_with};

use crate::build::DRAWING;
use crate::drawing::{Drawing, meta, settings};
use crate::flat::frame;
use crate::write_core::{PNG, custom, move_by, open_bytes};

/// A drawing as another producer makes one: styles in both parts, `meta.xml`, `settings.xml`, a thumbnail, and on request a face (declared in both parts) and a
/// picture named by an href.
fn producer(picture: bool, faces: bool) -> Drawing {
    let mut d = Drawing::a4().common(r#"<style:style style:name="Standard" style:family="graphic"/>"#);
    d.auto = r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc3333"/></style:style>"##.to_owned();
    if faces {
        d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="'DejaVu Sans Mono'"/>"#.to_owned();
    }
    let shapes = custom("s1", "1cm").replace("<draw:custom-shape ", "<draw:custom-shape draw:style-name=\"gr1\" ");
    let shapes = if picture {
        format!("{shapes}{}", frame(r#"<draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#))
    } else {
        shapes
    };
    let mut d = d.page(&shapes);
    d.meta = Some(meta("LibreOffice/7.3$Linux"));
    d.settings = Some(settings(""));
    if picture {
        d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    }
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), b"\x89PNG\r\n\x1a\nthumb".to_vec()));
    d
}

fn save(doc: &diacraft_doc::Document, bytes: &[u8], index: &ShapeIndex, form: Option<Form>) -> Vec<u8> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { form, ..WriteOptions::default() })
        .unwrap_or_else(|e| panic!("save: {e}"))
        .bytes
}

/// The number of differences between `a` and `b`, and what they are.
fn count(a: &[u8], b: &[u8]) -> (u64, Vec<String>) {
    let d = compare_content(a, b, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{} ({e})", e.class()));
    (d.count, d.shown.iter().map(|x| format!("{:?} {}", x.found, x.path)).collect())
}

fn first_shape_id(doc: &diacraft_doc::Document) -> u32 {
    doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).expect("a shape")
}

#[test]
fn a_drawing_saved_as_a_package_and_as_a_flat_file_by_our_writers_is_the_same_content() {
    let mut failures = Vec::new();
    for (picture, faces) in [(false, false), (false, true), (true, false), (true, true)] {
        let d = producer(picture, faces);
        let package = d.bytes();
        let opened = open_bytes(package.clone());
        // Unedited: the package as it stands against the flat file our writer makes of it.
        let flat = save(&opened.doc, &opened.bytes, &opened.index, Some(Form::Flat));
        let unedited = count(&package, &flat);
        if unedited.0 != 0 {
            failures.push(format!("unedited, picture={picture} faces={faces}: {unedited:?}"));
        }
        // Edited: the package and the flat file our writer makes of the same edit.
        let mut doc = opened.doc.clone();
        let id = first_shape_id(&doc);
        move_by(&mut doc, 0, id, 1.0, 0.5);
        let edited_package = save(&doc, &opened.bytes, &opened.index, None);
        let edited_flat = save(&doc, &opened.bytes, &opened.index, Some(Form::Flat));
        let edited = count(&edited_package, &edited_flat);
        if edited.0 != 0 {
            failures.push(format!("edited, picture={picture} faces={faces}: {edited:?}"));
        }
        // Edited from the flat file: the flat file opened, the same edit, saved as it was opened.
        let from_flat = open_bytes(flat.clone());
        let mut doc = from_flat.doc.clone();
        let id = first_shape_id(&doc);
        move_by(&mut doc, 0, id, 1.0, 0.5);
        let again = save(&doc, &from_flat.bytes, &from_flat.index, None);
        let edited_again = count(&edited_package, &again);
        if edited_again.0 != 0 {
            failures.push(format!("edited from the flat file, picture={picture} faces={faces}: {edited_again:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_drawing_the_converter_writes_with_a_picture_the_manifest_gives_no_type_to_is_the_same_content() {
    // The row of the picture has no media type: the converter writes the type the bytes say.
    let d = producer(true, true);
    let mut files = d.files();
    let rows: Vec<(String, String)> =
        files.iter().map(|(n, _)| (n.clone(), if n.ends_with(".png") { String::new() } else { "text/xml".to_owned() })).collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    files.push(("META-INF/manifest.xml".to_owned(), crate::build::manifest(DRAWING, &rows).into_bytes()));
    let package = crate::build::write(Some(DRAWING), &files);
    let opened = open_bytes(package.clone());
    let flat = save(&opened.doc, &opened.bytes, &opened.index, Some(Form::Flat));
    assert_eq!(count(&package, &flat).0, 0);
}
