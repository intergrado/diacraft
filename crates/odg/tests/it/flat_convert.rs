//! A package written as a flat document: `WriteOptions::form` asks for it, the package's parts
//! become the children of one `office:document` in the order, and its pictures go inline; what the specs give no rule for is refused in words.

use diacraft_doc::Document;
use diacraft_odg::{Form, Limits, OriginalPackage, SaveNote, Variant, WriteError, WriteOptions, Written, read, sniff_form, write_with};

use crate::build::{TEMPLATE, package};
use crate::drawing::{Drawing, meta, settings};
use crate::flat::{base64_lines, frame};
use crate::flat_write::{save, second_save_is_no_edit, text_of};
use crate::write_core::{PNG, custom, move_by, open_bytes};
use crate::write_new::{push_shape, rectangle};

/// The model with each picture said to come from one place: a picture read from a package part (`Pictures/a.png`) and the same read inline
/// (`office:binary-data`) are one picture, which the writer's comparison counts so too.
fn named_alike(mut doc: Document) -> Document {
    use std::sync::Arc;
    for i in 0..doc.pages.len() {
        let page = Arc::make_mut(doc.pages.get_mut(i).expect("a page"));
        for j in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(j).expect("a shape"));
            if let Some(foreign) = shape.foreign.as_mut() {
                let foreign = Arc::make_mut(foreign);
                if let Some(picture) = foreign.picture.as_mut() {
                    let mut media = (**picture).clone();
                    media.part = Arc::from("a picture");
                    *picture = Arc::new(media);
                }
                // The attributes of the `draw:image` element are written in the form of each file (a flat document names the MIME type).
                foreign.ext = Default::default();
            }
        }
    }
    crate::drawing::without_package(&doc)
}

/// The package of `d`, with `media` as its kind.
fn package_of(d: &Drawing, media: &str) -> Vec<u8> {
    package(media, d.files())
}

/// The writer asked for a flat document.
fn save_flat(doc: &Document, bytes: &[u8], index: &diacraft_odg::ShapeIndex) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() })
}

/// A drawing a producer wrote: a meta part, settings, a picture, a page of shapes, styles in both parts.
fn producer() -> Drawing {
    let picture = frame(r#"<draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#);
    let mut d = Drawing::a4().common(r#"<style:style style:name="Standard" style:family="graphic"/>"#);
    d.auto = r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc3333"/></style:style>"##.to_owned();
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="'DejaVu Sans Mono'"/>"#.to_owned();
    let d = d.page(&format!("{}{picture}", custom("s1", "1cm").replace("<draw:custom-shape ", "<draw:custom-shape draw:style-name=\"gr1\" ")));
    let mut d = d;
    d.meta = Some(meta("LibreOffice/7.3$Linux"));
    d.settings = Some(settings(""));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), b"\x89PNG\r\n\x1a\nthumb".to_vec()));
    d.files.push(("Configurations2/".to_owned(), Vec::new()));
    d
}

/// Every part of a package becomes a child of the flat document, in the order, and the model is the package's, to the picture.
#[test]
fn odg_package_a_package_is_written_as_a_flat_document_with_the_same_model() {
    let d = producer();
    let bytes = package_of(&d, crate::build::DRAWING);
    let o = open_bytes(bytes.clone());
    let saved = save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!saved.edited, "the drawing was not edited");
    assert_eq!(sniff_form(&saved.bytes), Some((Variant::Drawing, Form::Flat)));
    let text = text_of(&saved.bytes);
    // The order, each child once, and the document's own pieces carried as they were.
    let order = [
        "<office:meta>",
        "<office:settings>",
        "<office:font-face-decls>",
        "<office:styles>",
        "<office:automatic-styles>",
        "<office:master-styles>",
        "<office:body>",
    ];
    let places: Vec<usize> = order.iter().map(|p| text.find(p).unwrap_or_else(|| panic!("no {p} in {text}"))).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]), "The order: {places:?}");
    for child in ["office:meta", "office:automatic-styles", "office:font-face-decls", "office:body"] {
        assert_eq!(text.matches(&format!("<{child}>")).count(), 1, "{child} once");
    }
    assert!(text.contains("office:version=\"1.3\"") && text.contains(&format!("office:mimetype=\"{}\"", crate::build::DRAWING)));
    assert!(text.contains("LibreOffice/7.3$Linux"), "the producer's meta is carried: nothing is written until an edit");
    // The picture is inline and its href is gone; nothing of the package's other files is in the document.
    assert!(!text.contains("Pictures/a.png") && !text.contains("xlink:href"), "{text}");
    assert!(text.contains("<office:binary-data>") && text.contains("draw:mime-type=\"image/png\""), "{text}");
    assert!(!text.contains("thumb"), "no thumbnail in a flat document");
    // The model it reads as is the package's.
    let (flat_doc, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.form, Form::Flat);
    assert_eq!(
        named_alike(flat_doc.clone()),
        named_alike(o.doc.clone()),
        "a flat document reads as the package it was made from, but for where a picture was read from"
    );
    second_save_is_no_edit(&o.doc, &saved);
    // The picture's bytes are the package's.
    let picture = flat_doc.pages.get(0).unwrap().shapes.iter().find_map(|s| s.foreign.as_ref().and_then(|f| f.picture.as_ref())).expect("a picture");
    assert_eq!(&picture.bytes[..], &PNG[..]);
}

/// An edit and the form in one save: the model's change is spliced into the document made, as it is into any flat document, and nothing else changes.
#[test]
fn odg_package_an_edited_package_is_written_as_a_flat_document_in_one_save() {
    let d = producer();
    let mut o = open_bytes(package_of(&d, crate::build::DRAWING));
    let unedited = save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    let first = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.is_none() || s.foreign.is_none()).map(|s| s.id).unwrap();
    move_by(&mut o.doc, 0, first, 1.0, 0.0);
    let saved = save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let (before, after) = (text_of(&unedited.bytes), text_of(&saved.bytes));
    assert!(after.contains("svg:x=\"2cm\"") && before.contains("svg:x=\"1cm\""));
    assert_eq!(
        after.replacen("svg:x=\"2cm\"", "svg:x=\"1cm\"", 1).replacen(crate::flat_write::generator(&after), "LibreOffice/7.3$Linux", 1),
        before
    );
    second_save_is_no_edit(&o.doc, &saved);
    // The same, as two saves: the flat document the first made, edited by the second.
    let mut again = open_bytes(unedited.bytes.clone());
    move_by(&mut again.doc, 0, first, 1.0, 0.0);
    assert_eq!(save(&again.doc, &again.bytes, &again.index).unwrap().bytes, saved.bytes, "one save and two give one document");
}

/// A new drawing: a template package with a new shape saved as a flat document, as a new drawing saved as `.fodg` is.
#[test]
fn odg_package_a_template_with_a_new_shape_is_written_as_a_flat_drawing() {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let bytes = package(TEMPLATE, d.files());
    let mut o = open_bytes(bytes);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = write_with(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { form: Some(Form::Flat), variant: Some(Variant::Drawing), ..WriteOptions::default() },
    )
    .unwrap();
    assert_eq!(sniff_form(&saved.bytes), Some((Variant::Drawing, Form::Flat)));
    let text = text_of(&saved.bytes);
    assert!(text.contains("Rectangle.1") && text.contains("style:name=\"gr1\""), "{text}");
    assert_eq!(text.matches("<office:automatic-styles").count(), 1);
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(2));
    second_save_is_no_edit(&o.doc, &saved);
    // A template stays a template when no kind is asked.
    let o = open_bytes(package(TEMPLATE, Drawing::a4().page(&custom("s1", "1cm")).files()));
    assert_eq!(sniff_form(&save_flat(&o.doc, &o.bytes, &o.index).unwrap().bytes), Some((Variant::Template, Form::Flat)));
}

/// What the specs give no rule for is refused in words, with nothing written.
#[test]
fn odg_package_what_a_flat_document_has_no_place_for_is_refused_in_words() {
    let refused = |d: &Drawing, what: &str| {
        let o = open_bytes(package_of(d, crate::build::DRAWING));
        let err = save_flat(&o.doc, &o.bytes, &o.index).err().unwrap_or_else(|| panic!("{what}: saved"));
        assert_eq!(err.class(), "write_unsupported", "{what}: {err}");
        err.to_string()
    };
    // The two parts' automatic styles share a name.
    let mut clash = producer();
    clash.styles_auto.push_str(r#"<style:style style:name="gr1" style:family="graphic"/>"#);
    assert!(refused(&clash, "a shared name").contains("one name"));
    let mut layout = producer();
    layout.auto.push_str(r#"<style:page-layout style:name="PM1"/>"#);
    refused(&layout, "a shared page layout");
    // An embedded object, code, and a file nothing names.
    let mut object = producer();
    object.pages = object.pages.replace(
        "</draw:page>",
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"><draw:object xlink:href="./Object 1"/></draw:frame></draw:page>"#,
    );
    object.files.push(("Object 1/content.xml".to_owned(), b"<a/>".to_vec()));
    assert!(refused(&object, "an object").contains("object"));
    let mut code = producer();
    code.files.push(("Basic/Standard/Module1.xml".to_owned(), b"<a/>".to_vec()));
    refused(&code, "code");
    let mut stray = producer();
    stray.files.push(("Extra/foo.bin".to_owned(), vec![1, 2, 3]));
    refused(&stray, "a stray file");
    // A prefix bound to two namespaces by the two parts.
    let mut prefix = producer();
    prefix.ns = prefix.ns.clone();
    let o = {
        let mut files = prefix.files();
        for (name, bytes) in files.iter_mut() {
            if name == "styles.xml" {
                *bytes = String::from_utf8(bytes.clone())
                    .unwrap()
                    .replace("xmlns:dc=\"http://purl.org/dc/elements/1.1/\"", "xmlns:dc=\"urn:other\"")
                    .into_bytes();
            }
        }
        open_bytes(package(crate::build::DRAWING, files))
    };
    assert_eq!(save_flat(&o.doc, &o.bytes, &o.index).err().map(|e| e.class()), Some("write_unsupported"));
    // A flat drawing saved as a package: no rule says how the styles split and the pictures move.
    let flat = crate::flat::Flat::of(&producer());
    let o = open_bytes(flat.bytes());
    let err = write_with(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { form: Some(Form::Package), ..WriteOptions::default() },
    )
    .expect_err("refused");
    assert_eq!(err.class(), "write_unsupported");
    assert!(err.to_string().contains("package"), "{err}");
}

/// The package of `producer()` whose part `part` has `from` replaced by `to`, once (`from` must be there).
fn producer_edited(part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut files = producer().files();
    let (_, bytes) = files.iter_mut().find(|(n, _)| n == part).unwrap_or_else(|| panic!("no part {part}"));
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains(from), "{part} has no {from}");
    *bytes = text.replacen(from, to, 1).into_bytes();
    package(crate::build::DRAWING, files)
}

/// What a part's root holds that the flat document has no place for is refused in words, and not dropped without a word: a second child of one
/// name (the flat document holds one of each, and the first is used), a child that is not an office element of the part's own kind (a foreign element, or an office
/// element that belongs to another part), and an attribute of the root that is neither a namespace declaration nor `office:version`. Each class is tried in each of
/// the four parts, and the package of the same parts without the addition converts.
#[test]
fn odg_package_what_a_parts_root_holds_that_a_flat_document_has_no_place_for_is_refused_in_words() {
    const FOREIGN: &str = r#"<x:note xmlns:x="urn:x"/>"#;
    // (the part, where, what goes in, the words the refusal has)
    let cases: [(&str, &str, &str, &str); 18] = [
        ("content.xml", "</office:body>", "</office:body><office:body/>", "two children of one name"),
        ("content.xml", "<office:automatic-styles>", "<office:automatic-styles/><office:automatic-styles>", "two children of one name"),
        ("styles.xml", "</office:master-styles>", "</office:master-styles><office:master-styles/>", "two children of one name"),
        ("styles.xml", "<office:styles>", "<office:styles/><office:styles>", "two children of one name"),
        ("meta.xml", "</office:meta>", "</office:meta><office:meta/>", "two children of one name"),
        ("settings.xml", "</office:settings>", "</office:settings><office:settings/>", "two children of one name"),
        ("content.xml", "<office:body>", &format!("{FOREIGN}<office:body>"), "root has a child"),
        ("styles.xml", "<office:styles>", &format!("{FOREIGN}<office:styles>"), "root has a child"),
        ("meta.xml", "<office:meta>", &format!("{FOREIGN}<office:meta>"), "root has a child"),
        ("settings.xml", "<office:settings>", &format!("{FOREIGN}<office:settings>"), "root has a child"),
        ("content.xml", "<office:body>", "<office:styles/><office:body>", "root has a child"),
        ("content.xml", "<office:body>", "<office:master-styles/><office:body>", "root has a child"),
        ("styles.xml", "<office:styles>", "<office:scripts/><office:styles>", "root has a child"),
        ("meta.xml", "<office:meta>", "<office:settings/><office:meta>", "root has a child"),
        ("content.xml", "office:version=\"1.3\">", "office:version=\"1.3\" office:extra=\"y\">", "attribute"),
        ("styles.xml", "office:version=\"1.3\">", "office:version=\"1.3\" office:extra=\"y\">", "attribute"),
        ("meta.xml", "office:version=\"1.3\">", "office:version=\"1.3\" office:extra=\"y\">", "attribute"),
        ("settings.xml", "office:version=\"1.3\">", "office:version=\"1.3\" xml:space=\"preserve\">", "attribute"),
    ];
    for (part, from, to, words) in cases {
        let o = open_bytes(producer_edited(part, from, to));
        match save_flat(&o.doc, &o.bytes, &o.index) {
            Err(e) => assert!(e.class() == "write_unsupported" && e.to_string().contains(words), "{part} {to}: {e}"),
            Ok(_) => panic!("{part}: {to} was dropped without a word"),
        }
    }
    // The same parts with what a flat document does place (a child once, in its part, the namespace declarations and the version) convert, a foreign element
    // inside a child (not a child of the root) among them.
    let o = open_bytes(producer_edited("content.xml", "<office:body>", "<office:body><!-- a comment -->"));
    save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    let o = open_bytes(producer_edited("styles.xml", "<office:styles>", &format!("<office:styles>{FOREIGN}")));
    save_flat(&o.doc, &o.bytes, &o.index).unwrap();
}

/// Only a `draw:image` gets `draw:mime-type` when its picture goes inline (name that element): a `draw:fill-image` is judged by the
/// bytes it holds, and the attribute is not among the ones the standard gives it.
#[test]
fn odg_package_a_fill_image_goes_inline_with_no_media_type_and_an_image_beside_it_has_one() {
    let mut d = Drawing::a4()
        .common(r#"<draw:fill-image draw:name="Bg" draw:display-name="Bg" xlink:href="Pictures/a.png" xlink:type="simple"/>"#)
        .page(&crate::flat::frame(r#"<draw:image xlink:href="Pictures/a.png"/>"#));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let o = open_bytes(package_of(&d, crate::build::DRAWING));
    let text = text_of(&save_flat(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let data = crate::flat::base64_lines(&PNG, 72, "\n");
    let inline = format!("<office:binary-data>{data}</office:binary-data>");
    assert!(
        text.contains(&format!(r#"<draw:fill-image draw:name="Bg" draw:display-name="Bg" xlink:type="simple">{inline}</draw:fill-image>"#)),
        "the fill image holds its data and no media type: {text}"
    );
    assert!(text.contains(&format!(r#"<draw:image draw:mime-type="image/png">{inline}</draw:image>"#)), "the image has its media type: {text}");
    assert_eq!(text.matches("mime-type").count(), 1, "{text}");
    // A fill image that names its own `draw:mime-type` keeps it: the file's own attribute is no business of the conversion's.
    let mut own = Drawing::a4()
        .common(r#"<draw:fill-image draw:name="Bg" draw:mime-type="image/x-own" xlink:href="Pictures/a.png"/>"#)
        .page(&crate::flat::frame(r#"<draw:image xlink:href="Pictures/a.png"/>"#));
    own.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let o = open_bytes(package_of(&own, crate::build::DRAWING));
    let text = text_of(&save_flat(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    assert!(text.contains(&format!(r#"<draw:fill-image draw:name="Bg" draw:mime-type="image/x-own">{inline}</draw:fill-image>"#)), "{text}");
}

/// An image that holds data of its own and an href that names a picture of the package is refused in words (a package reader follows the
/// href and a flat reader takes the data, R33: the two can be different pictures, and the element would hold two `office:binary-data`).
#[test]
fn odg_package_an_image_with_inline_data_and_an_href_that_names_a_picture_is_refused_in_words() {
    let inline = crate::flat::base64_lines(&PNG, 72, "\n");
    let both = format!(r#"<draw:image xlink:href="Pictures/a.png"><office:binary-data>{inline}</office:binary-data></draw:image>"#);
    // Each image in a frame of its own (a frame with two images is refused for another reason).
    let package_with = |images: &[&str]| {
        let mut d = Drawing::a4().page(&images.iter().map(|i| crate::flat::frame(i)).collect::<String>());
        d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
        open_bytes(package_of(&d, crate::build::DRAWING))
    };
    let o = package_with(&[&both]);
    match save_flat(&o.doc, &o.bytes, &o.index) {
        Err(e) => assert!(e.class() == "write_unsupported" && e.to_string().contains("inline data beside an href"), "{e}"),
        Ok(w) => panic!("an image with two sources was written: {}", text_of(&w.bytes)),
    }
    // Beside a second image that names the picture (so the package holds no file nothing names), the first is refused all the same, and the second alone converts.
    let o = package_with(&[&both, r#"<draw:image xlink:href="Pictures/a.png"/>"#]);
    assert!(save_flat(&o.doc, &o.bytes, &o.index).is_err(), "one image of two with two sources refuses the conversion");
    let o = package_with(&[r#"<draw:image xlink:href="Pictures/a.png"/>"#]);
    save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    // Data further down, in a child of the image and not a child of it, is no data of the image's own: the picture goes in as the first child.
    let deeper =
        format!(r#"<draw:image xlink:href="Pictures/a.png"><text:p><office:binary-data>{inline}</office:binary-data></text:p></draw:image>"#);
    let text = converted(&deeper).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(text.matches("<office:binary-data>").count(), 2, "{text}");
}

/// The thumbnail, `Configurations2/` and the manifest are left out; a signature is left out and the save says it is gone.
#[test]
fn odg_package_what_a_flat_document_leaves_out_and_the_signature_it_cannot_keep() {
    let mut d = producer();
    d.files.push(("META-INF/documentsignatures.xml".to_owned(), b"<sig/>".to_vec()));
    let mut files = d.files();
    files.push(("manifest.rdf".to_owned(), b"<rdf/>".to_vec()));
    let o = open_bytes(package(crate::build::DRAWING, files));
    let saved = save_flat(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.notes, vec![SaveNote::SignatureStale]);
    let text = text_of(&saved.bytes);
    assert!(!text.contains("sig/") && !text.contains("rdf/") && !text.contains("manifest"));
    // Base64 of the picture: the lines are 72 characters but the last.
    let png = crate::flat::picture(300);
    let mut big = producer();
    big.files.retain(|(n, _)| n != "Pictures/a.png");
    big.files.push(("Pictures/a.png".to_owned(), png.clone()));
    let o = open_bytes(package_of(&big, crate::build::DRAWING));
    let text = text_of(&save_flat(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let data = text.split("<office:binary-data>").nth(1).and_then(|r| r.split("</office:binary-data>").next()).expect("inline data");
    let lines: Vec<&str> = data.split('\n').collect();
    assert!(lines.len() > 1 && lines.iter().rev().skip(1).all(|l| l.len() == 72) && lines.last().is_some_and(|l| l.len() <= 72), "{lines:?}");
    assert_eq!(data, base64_lines(&png, 72, "\n"));
}

/// What a conversion holds is estimated from the sizes the archive states, before a picture is inflated: past `max_write_memory` it is `write_limit`, and the
/// picture was not inflated; one reference to the same picture is a copy each, and the number of copies is capped.
#[test]
fn odg_package_a_conversion_is_refused_for_the_memory_it_would_hold_before_a_picture_is_inflated() {
    let mut d = producer();
    d.files.retain(|(n, _)| n != "Pictures/a.png");
    let mut big = crate::flat::picture(64);
    big.resize(400_000, 7);
    d.files.push(("Pictures/a.png".to_owned(), big));
    let o = open_bytes(package_of(&d, crate::build::DRAWING));
    let with = |memory: u64| {
        let mut work = diacraft_odg::WriteWork::default();
        let options =
            WriteOptions { form: Some(Form::Flat), limits: Limits { max_write_memory: memory, ..Limits::default() }, ..WriteOptions::default() };
        let result = diacraft_odg::write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
        (result.map(|w| w.bytes.len()).map_err(|e| e.class()), work.package.inflated)
    };
    // The picture is 400,000 bytes, four times that (the picture, its characters, the part they go into and what that is made into) is 1,600,000.
    let (refused, inflated) = with(1_500_000);
    assert_eq!(refused, Err("write_limit"));
    assert!(inflated < 400_000, "the picture was not inflated: {inflated}");
    // The same drawing under the default limit converts.
    assert!(with(Limits::default().max_write_memory).0.is_ok());
    // A picture named by two images is two copies: one more reference, and the estimate passes what it did.
    let mut twice = d.clone();
    twice.pages = twice.pages.replace("</draw:page>", &format!("{}</draw:page>", crate::flat::frame("<draw:image xlink:href=\"Pictures/a.png\"/>")));
    let o2 = open_bytes(package_of(&twice, crate::build::DRAWING));
    let mut work = diacraft_odg::WriteWork::default();
    let options =
        WriteOptions { form: Some(Form::Flat), limits: Limits { max_write_memory: 2_500_000, ..Limits::default() }, ..WriteOptions::default() };
    let result = diacraft_odg::write_counted(&o2.doc, Some(&OriginalPackage { bytes: &o2.bytes, index: &o2.index }), &options, &mut work);
    assert_eq!(result.err().map(|e| e.class()), Some("write_limit"), "two copies of 400,000 bytes, four times each, are past 2,500,000");
}

/// The number of pictures one conversion writes inline is capped (each is a copy and its edits are kept): 200,000, and one past it is `write_limit`.
#[test]
fn odg_package_the_pictures_of_one_conversion_are_capped() {
    let image = "<draw:image xlink:href=\"Pictures/a.png\"/>";
    let make = |n: usize| {
        let mut d = Drawing::a4().page(&crate::flat::frame(&image.repeat(n)));
        d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
        open_bytes(package_of(&d, crate::build::DRAWING))
    };
    let result = |o: &crate::write_core::Open| {
        let options = WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() };
        write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options).map(|w| w.bytes.len()).map_err(|e| e.class())
    };
    assert_eq!(result(&make(200_001)), Err("write_limit"));
    // A frame of one image converts.
    assert!(result(&make(1)).is_ok());
}

/// The package of `d` with one frame whose image is `image` (the picture `Pictures/a.png` is a PNG), saved as a flat document: the text, or the error class.
fn converted(image: &str) -> Result<String, &'static str> {
    let mut d = Drawing::a4().page(&crate::flat::frame(image));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let o = open_bytes(package_of(&d, crate::build::DRAWING));
    save_flat(&o.doc, &o.bytes, &o.index).map(|w| text_of(&w.bytes)).map_err(|e| e.class())
}

/// What an image becomes, to the byte: the href and the white space before it are gone, `draw:mime-type` is added once at the end of the tag, the data is the
/// element's first child, and every other attribute is where it was.
#[test]
fn odg_package_an_image_becomes_its_inline_form_to_the_byte() {
    let data = crate::flat::base64_lines(&PNG, 72, "\n");
    let inline = format!("<office:binary-data>{data}</office:binary-data>");
    let text = converted(r#"<draw:image xlink:href="Pictures/a.png" xlink:type="simple"/>"#).unwrap();
    assert!(text.contains(&format!(r#"<draw:image xlink:type="simple" draw:mime-type="image/png">{inline}</draw:image>"#)), "{text}");
    // The href in the middle, in lines of its own, and the tag open (the data goes in as its first child, before what was in it).
    let text =
        converted("<draw:image\n   xlink:type=\"simple\"\n   xlink:href=\"Pictures/a.png\"\n   draw:name=\"n\">\n<text:p/></draw:image>").unwrap();
    assert!(
        text.contains(&format!(
            "<draw:image\n   xlink:type=\"simple\"\n   draw:name=\"n\" draw:mime-type=\"image/png\">{inline}\n<text:p/></draw:image>"
        )),
        "{text}"
    );
    // A `draw:mime-type` the file has is kept, and not written twice.
    let text = converted(r#"<draw:image xlink:href="Pictures/a.png" draw:mime-type="image/x-own"/>"#).unwrap();
    assert!(text.contains(&format!(r#"<draw:image draw:mime-type="image/x-own">{inline}</draw:image>"#)), "{text}");
    assert_eq!(text.matches("mime-type").count(), 1);
    // The href as the first attribute, with a quote of the other kind: its own text only is taken out.
    let text = converted(r#"<draw:image xlink:href='Pictures/a.png' draw:name="n"/>"#).unwrap();
    assert!(text.contains(r#"<draw:image draw:name="n" draw:mime-type="image/png">"#), "{text}");
}

/// What names a picture is told by namespace: an attribute of another namespace called `href`, an element called `image` of another namespace, an attribute whose
/// value is a picture's name that is no href, and the declaration of a prefix called `href` are none of them an href of an image.
#[test]
fn odg_package_only_an_xlink_href_of_a_draw_image_names_a_picture() {
    // An attribute `href` in another namespace on a shape that is no image names nothing: no refusal, and the picture is left alone.
    let mut d =
        Drawing::a4().page(r#"<draw:rect xmlns:x="urn:x" x:href="Pictures/a.png" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#);
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let o = open_bytes(package_of(&d, crate::build::DRAWING));
    // The picture is named by nothing the conversion knows of, so it has no place: refused, and not mistaken for a picture an href named.
    let err = save_flat(&o.doc, &o.bytes, &o.index).expect_err("an unreferenced picture has no place");
    assert!(err.to_string().contains("nothing names"), "a file nothing names, not one an href names: {err}");
    // An `image` of another namespace whose xlink:href names the picture is an element the conversion does not read as an image: the href names a file it
    // does not place.
    assert_eq!(converted(r#"<x:image xmlns:x="urn:x" xlink:href="Pictures/a.png"/>"#), Err("write_unsupported"));
    // An attribute that holds the picture's name and is no href is not taken for the href, whatever it comes first.
    let text = converted(r#"<draw:image draw:name="Pictures/a.png" xlink:href="Pictures/a.png"/>"#).unwrap();
    assert!(text.contains(r#"<draw:image draw:name="Pictures/a.png" draw:mime-type="image/png">"#) && !text.contains("xlink:href"), "{text}");
    // `xmlns:href` is a declaration, not an href, even when what it declares is a picture's name.
    let text = converted(r#"<draw:image xmlns:href="Pictures/a.png" xlink:href="Pictures/a.png"/>"#).unwrap();
    assert!(text.contains(r#"<draw:image xmlns:href="Pictures/a.png" draw:mime-type="image/png">"#) && !text.contains("xlink:href"), "{text}");
    // The `office:version` of the root is the office namespace's, not another's of the same local name: an attribute `x:version` is no version, so it is an
    // attribute of the root with no place in a flat document, and refused (the version taken is tested on `look`).
    let mut v = Drawing::a4().page("");
    v.version = "1.3\" x:version=\"9".to_owned();
    v.ns = format!("xmlns:x=\"urn:x\" {}", v.ns);
    let o = open_bytes(package_of(&v, crate::build::DRAWING));
    let err = save_flat(&o.doc, &o.bytes, &o.index).expect_err("a root attribute of another namespace has no place");
    assert!(err.to_string().contains("neither a namespace declaration nor office:version"), "{err}");
}

/// The estimate of what a conversion holds is exact at its limit: four times the picture's stated size, the markup round it, and three times the parts.
#[test]
fn odg_package_the_estimate_of_a_conversion_is_exact_at_its_limit() {
    let mut d = Drawing::a4().page(&crate::flat::frame(r#"<draw:image xlink:href="Pictures/a.png"/>"#));
    let mut picture = crate::flat::picture(64);
    picture.resize(10_000, 7);
    d.files.push(("Pictures/a.png".to_owned(), picture));
    let o = open_bytes(package_of(&d, crate::build::DRAWING));
    let parts: u64 = d
        .files()
        .iter()
        .filter(|(n, _)| ["content.xml", "styles.xml", "meta.xml", "settings.xml"].contains(&n.as_str()))
        .map(|(_, b)| b.len() as u64)
        .sum();
    let estimate = 10_000 * 4 + 512 + parts * 3;
    let with = |memory: u64| {
        let limits = Limits { max_write_memory: memory, ..Limits::default() };
        write_with(
            &o.doc,
            Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &WriteOptions { form: Some(Form::Flat), limits, ..WriteOptions::default() },
        )
        .map(|w| w.bytes.len())
        .map_err(|e| (e.class(), e.to_string()))
    };
    // Past the estimate the conversion is made (the save after it is held to the same limit, so give it room).
    let at = with(estimate);
    assert!(matches!(&at, Err((class, why)) if *class == "write_limit" && !why.contains("conversion")) || at.is_ok(), "{at:?}");
    let under = with(estimate - 1);
    assert!(matches!(&under, Err((class, why)) if *class == "write_limit" && why.contains("conversion")), "{under:?}");
}

/// The comparison of the two parts' style names is counted in the units it walks: each automatic style and page layout of each part, once.
#[test]
fn odg_package_the_comparison_of_the_names_of_the_two_parts_is_counted() {
    let o = open_bytes(package_of(&producer(), crate::build::DRAWING));
    let mut work = diacraft_odg::WriteWork::default();
    let options = WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() };
    write_counted_ok(&o, &options, &mut work);
    // `content.xml` holds the automatic style `gr1`, and `styles.xml` the layout `PM1`: two names in all.
    assert_eq!(work.order_steps, 2);
    // Three styles in the content and two layouts in the styles part, one name each: five.
    let mut more = producer();
    more.auto.push_str(r#"<style:style style:name="gr2" style:family="graphic"/><style:style style:name="gr3" style:family="graphic"/>"#);
    more.styles_auto.push_str(r#"<style:page-layout style:name="PM2"/>"#);
    let o = open_bytes(package_of(&more, crate::build::DRAWING));
    let mut work = diacraft_odg::WriteWork::default();
    write_counted_ok(&o, &options, &mut work);
    assert_eq!(work.order_steps, 5);
}

fn write_counted_ok(o: &crate::write_core::Open, options: &WriteOptions, work: &mut diacraft_odg::WriteWork) {
    let saved = diacraft_odg::write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), options, work);
    assert!(saved.is_ok(), "{:?}", saved.err().map(|e| e.to_string()));
}

/// The media type of an inline picture is the manifest's when the row says one, and the picture's own bytes' when it says nothing (an empty `media-type`); an image
/// written in the drawing namespace as the default one still gets `draw:mime-type`, in the prefix the root binds.
#[test]
fn odg_package_the_media_type_of_an_inline_picture_and_an_image_in_the_default_namespace() {
    let mut d = Drawing::a4().page(&crate::flat::frame(r#"<draw:image xlink:href="Pictures/a.png"/>"#));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let files = |media: &str| {
        let mut files = d.files();
        let rows: Vec<(String, String)> =
            files.iter().map(|(n, _)| (n.clone(), if n.ends_with(".png") { media.to_owned() } else { "text/xml".to_owned() })).collect();
        let rows: Vec<(&str, &str)> = rows.iter().map(|(n, m)| (n.as_str(), m.as_str())).collect();
        files.push(("META-INF/manifest.xml".to_owned(), crate::build::manifest(crate::build::DRAWING, &rows).into_bytes()));
        files
    };
    let flat_of = |media: &str| {
        let o = open_bytes(crate::build::write(Some(crate::build::DRAWING), &files(media)));
        text_of(&save_flat(&o.doc, &o.bytes, &o.index).unwrap().bytes)
    };
    assert!(flat_of("image/x-own").contains("draw:mime-type=\"image/x-own\""), "the manifest's type");
    assert!(flat_of("").contains("draw:mime-type=\"image/png\""), "no type in the row: the picture's own");
    // The drawing namespace as the default one on the image element.
    let text = converted(r#"<image xmlns="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xlink:href="Pictures/a.png"/>"#).unwrap();
    assert!(
        text.contains(r#"<image xmlns="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" draw:mime-type="image/png"><office:binary-data>"#),
        "{text}"
    );
}
