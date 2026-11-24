//! A new picture in a file that holds a broken picture link: an image whose `xlink:href` names no file of
//! the package, in `content.xml` or `styles.xml`, in any spelling the reader resolves, beside a picture the package holds is a file LibreOffice does not open (measured on 26.8.1.1,
//! with a file made by hand as with one DiaCraft saves). A new picture is the save that would make that file out of one that opens, so it is refused in words that say so and what
//! to do; a file with such a link and no new picture still saves, and a file with none takes a new picture as before.
//!
//! The reservation of the names every href uses stays: it guards the names that a link which is no picture link (a hyperlink, an object) holds, and the entries and rows.

use std::sync::Arc;

use diacraft_odg::{Limits, WriteError, read};

use crate::drawing::Drawing;
use crate::flat_write::small_png;
use crate::write_core::{custom, move_by, open_bytes, part};
use crate::write_picture::{entry_names, frame_of, pictures_of, save, shown};
use crate::write_picture_names::{add, drawing, master_with, page_frame, shape_named};

/// The refusal a save with a new picture is given, or a failure of the test when it is saved.
fn refused(o: &crate::write_core::Open) -> String {
    match save(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("a refusal in words, not {other}"),
        Ok(saved) => panic!("a file with a broken picture link takes a new picture: {:?}", entry_names(&saved.bytes)),
    }
}

/// The words say what is wrong and what to do.
#[track_caller]
fn assert_words(words: &str) {
    for want in ["broken picture link", "LibreOffice", "remove or repair"] {
        assert!(words.contains(want), "{want}: {words}");
    }
}

/// The review's two files, as wrote them: a frame of the master page that dangles at `Pictures/image1.png`, and a frame of the page; each beside a new picture.
#[test]
fn a_new_picture_beside_a_dangling_frame_of_the_master_page_or_of_a_page_is_refused_and_nothing_is_written() {
    let mut master = drawing("", &master_with(Some("Pictures/image1.png")), "");
    add(&mut master, false, "Picture.1", &small_png());
    assert_words(&refused(&master));
    let mut page = drawing(&page_frame("PageFrame", "Pictures/image1.png"), &master_with(None), "");
    add(&mut page, false, "Picture.1", &small_png());
    assert_words(&refused(&page));
    // On the master page itself, beside a dangling frame of a page.
    let mut on_master = drawing(&page_frame("PageFrame", "Pictures/image1.png"), &master_with(None), "");
    add(&mut on_master, true, "Picture.1", &small_png());
    assert_words(&refused(&on_master));
}

/// Every element of the standard that loads a picture by an `xlink:href` is a picture link: a fill image, a stroke image, a background image and a bullet picture, in `styles.xml`.
#[test]
fn a_dangling_fill_image_stroke_image_background_image_or_bullet_picture_refuses_a_new_picture_too() {
    let styles = [
        (
            "a fill image",
            r#"<draw:fill-image draw:name="Bg" xlink:href="Pictures/gone.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#,
        ),
        (
            "a stroke image",
            r#"<draw:stroke-image draw:name="St" xlink:href="Pictures/gone.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#,
        ),
        (
            "a background image",
            r#"<style:style style:name="Sty" style:family="graphic"><style:graphic-properties><style:background-image xlink:href="Pictures/gone.png" xlink:type="simple"/></style:graphic-properties></style:style>"#,
        ),
        (
            "a bullet picture",
            r#"<text:list-style style:name="L1"><text:list-level-style-image text:level="1" xlink:href="Pictures/gone.png" xlink:type="simple"/></text:list-style>"#,
        ),
    ];
    for (what, common) in styles {
        let mut o = drawing("", &master_with(None), common);
        add(&mut o, false, "Picture.1", &small_png());
        let words = match save(&o.doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(words)) => words,
            other => panic!("{what}: a refusal in words, not {:?}", other.map(|w| entry_names(&w.bytes))),
        };
        assert_words(words);
    }
}

/// Every spelling the reader resolves to a name no entry holds is a broken link: as written, with `./`, with the dot or the slash percent-encoded, with a letter encoded. And the same
/// spellings of a name an entry holds are no broken link, and the picture is saved.
#[test]
fn a_broken_link_is_told_in_every_spelling_the_reader_resolves_and_so_is_one_that_is_not() {
    for href in
        ["Pictures/gone.png", "./Pictures/gone.png", "%2E/Pictures/gone.png", ".%2FPictures/gone.png", "Pictures/g%6Fne.png", "Pictures/gone%2Epng"]
    {
        let mut o = drawing(&page_frame("PageFrame", href), &master_with(None), "");
        add(&mut o, false, "Picture.1", &small_png());
        assert_words(&refused(&o));
    }
    for href in ["Pictures/a.png", "./Pictures/a.png", "%2E/Pictures/a.png", ".%2FPictures/a.png", "Pictures/%61.png", "Pictures/a%2Epng"] {
        let mut d = Drawing::a4().page(&format!("{}{}", custom("s1", "1cm"), page_frame("Held", href)));
        d.files.push(("Pictures/a.png".to_owned(), small_png()));
        let mut o = open_bytes(d.bytes());
        add(&mut o, false, "Picture.1", &small_png());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{href}: {e}"));
        assert_eq!(
            pictures_of(&saved.bytes),
            ["Pictures/a.png", "Pictures/image1.png"],
            "{href}: the name the link holds is held, and the picture is the first free"
        );
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        assert_eq!(shown(shape_named(&back, "Held")).map(|s| s.0), Some(small_png()), "{href}: the frame still shows its own picture");
    }
}

/// What is not a path in the package is no broken link of the package's, whatever it names (measured on LibreOffice 26.8.1.1: it opens each of these, with a picture the package holds
/// beside them, and does not open a path in the package that names no entry): an IRI with a scheme, a path from the root or out of the package, an empty href, and a replacement
/// picture of an object. A path of the package that names no entry (a plain name, an object's folder as an image) is broken.
#[test]
fn only_a_path_in_the_package_that_names_no_entry_is_a_broken_link_and_the_forms_libreoffice_opens_are_not() {
    for (href, broken) in [
        ("http://example.org/x.png", false),
        ("file:///tmp/x.png", false),
        ("/tmp/x.png", false),
        ("../x.png", false),
        ("", false),
        ("ObjectReplacements/Object 1", false),
        ("gone.png", true),
        ("./Object 1", true),
        ("Pictures/sub/gone.png", true),
    ] {
        let mut d = Drawing::a4().page(&format!("{}{}{}", custom("s1", "1cm"), page_frame("Held", "Pictures/a.png"), page_frame("Link", href)));
        d.files.push(("Pictures/a.png".to_owned(), small_png()));
        let mut o = open_bytes(d.bytes());
        add(&mut o, false, "Picture.1", &small_png());
        let result = save(&o.doc, &o.bytes, &o.index);
        if broken {
            assert!(matches!(result, Err(WriteError::Unsupported(w)) if w.contains("broken picture link")), "{href:?} is a broken link");
        } else {
            let saved = result.unwrap_or_else(|e| panic!("{href:?} is no broken link: {e}"));
            assert_eq!(pictures_of(&saved.bytes), ["Pictures/a.png", "Pictures/image1.png"], "{href:?}");
        }
    }
}

/// A file that holds a broken picture link and takes no new picture is saved as it always was: an edit of another shape, a second save that is no edit, and the broken frame as the
/// file has it.
#[test]
fn a_file_with_a_broken_picture_link_and_no_new_picture_is_saved_and_the_frame_is_as_the_file_has_it() {
    let mut o = drawing(
        &page_frame("PageFrame", "Pictures/gone.png"),
        &master_with(Some("Pictures/gone2.png")),
        r#"<draw:fill-image draw:name="Bg" xlink:href="Pictures/gone3.png"/>"#,
    );
    let s1 = shape_named(&o.doc, "s1").id;
    move_by(&mut o.doc, 0, s1, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).expect("an edit of another shape is saved");
    assert!(saved.edited);
    assert_eq!(frame_of(&part(&saved.bytes, "content.xml"), "PageFrame"), frame_of(&part(&o.bytes, "content.xml"), "PageFrame"));
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "styles.xml is the file's own");
    assert!(pictures_of(&saved.bytes).is_empty());
    assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "a second save is no edit");
}

/// A link that is no picture link is no broken one: a hyperlink to a file or to a place, an object's folder, an image that is a link outside the package (an IRI with a scheme, an
/// absolute path, a path out of the package). The picture is saved, and a name a hyperlink holds is held (the guard of the reservation).
#[test]
fn a_link_that_is_no_picture_link_in_the_package_is_no_broken_one_and_a_name_it_holds_is_held() {
    let links: [(&str, &str); 5] = [
        (r#"<text:p><text:a xlink:href="notes.txt" xlink:type="simple">a file</text:a></text:p>"#, "Pictures/image1.png"),
        (r##"<text:p><text:a xlink:href="#Page 2" xlink:type="simple">a place</text:a></text:p>"##, "Pictures/image1.png"),
        (r#"<text:p><text:a xlink:href="mailto:someone@example.org" xlink:type="simple">a mail</text:a></text:p>"#, "Pictures/image1.png"),
        (r#"<text:p><text:a xlink:href="Pictures/image1.png" xlink:type="simple">a hyperlink at the name</text:a></text:p>"#, "Pictures/image2.png"),
        (
            r#"<text:p><text:a xlink:href="%2E/Pictures/image1.png" xlink:type="simple">a hyperlink at the encoded name</text:a></text:p>"#,
            "Pictures/image2.png",
        ),
    ];
    for (link, expected) in links {
        let shape = format!(
            "\n    <draw:custom-shape draw:name=\"L\" svg:x=\"1cm\" svg:y=\"9cm\" svg:width=\"4cm\" svg:height=\"2cm\">{link}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
        );
        let mut o = drawing(&shape, &master_with(None), "");
        add(&mut o, false, "Picture.1", &small_png());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{link}: {e}"));
        assert_eq!(pictures_of(&saved.bytes), [expected], "{link}");
    }
}

/// A refusal writes nothing and changes nothing: the same model saved with the picture taken off is saved, and the broken link is still the file's.
#[test]
fn the_refusal_leaves_the_model_as_it_was_and_the_save_without_the_picture_is_saved() {
    let mut o = drawing(&page_frame("PageFrame", "Pictures/gone.png"), &master_with(None), "");
    add(&mut o, false, "Picture.1", &small_png());
    let before = o.doc.clone();
    assert_words(&refused(&o));
    assert_eq!(o.doc, before, "the model is as it was");
    let kept: Vec<_> =
        Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.iter().filter(|s| s.name_u.as_deref() != Some("Picture.1")).cloned().collect();
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes = diacraft_doc::SharedVec::from_vec(kept);
    let saved = save(&o.doc, &o.bytes, &o.index).expect("the same drawing with no new picture is saved");
    assert!(!saved.edited, "and it is no edit");
}

/// A file with a broken link whose frame the same save deletes is refused as well: the file holds the link, and the words say to save that first.
#[test]
fn a_broken_frame_the_same_save_deletes_is_still_the_files_and_the_save_with_a_picture_is_refused() {
    let mut o = drawing(&page_frame("PageFrame", "Pictures/gone.png"), &master_with(None), "");
    let kept: Vec<_> = o.doc.pages.get(0).unwrap().shapes.iter().filter(|s| s.name_u.as_deref() != Some("PageFrame")).cloned().collect();
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes = diacraft_doc::SharedVec::from_vec(kept);
    add(&mut o, false, "Picture.1", &small_png());
    assert_words(&refused(&o));
    // The way out the words give: save without the frame, and then add the picture.
    let mut first = drawing(&page_frame("PageFrame", "Pictures/gone.png"), &master_with(None), "");
    let kept: Vec<_> = first.doc.pages.get(0).unwrap().shapes.iter().filter(|s| s.name_u.as_deref() != Some("PageFrame")).cloned().collect();
    Arc::make_mut(first.doc.pages.get_mut(0).unwrap()).shapes = diacraft_doc::SharedVec::from_vec(kept);
    let saved = save(&first.doc, &first.bytes, &first.index).expect("the frame is taken out");
    let mut again = open_bytes(saved.bytes);
    add(&mut again, false, "Picture.1", &small_png());
    let saved = save(&again.doc, &again.bytes, &again.index).expect("with the broken frame gone the picture is saved");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
}

/// A package with no `Pictures/` at all and no link takes the picture, whatever `styles.xml` holds: the control of every case above.
#[test]
fn a_file_with_no_broken_link_takes_a_new_picture() {
    let mut o = drawing("", &master_with(None), "");
    add(&mut o, false, "Picture.1", &small_png());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
}

/// `DIACRAFT_PICTURE_DIR`: where the files are written (run with `--run-ignored only`): a drawing made by hand with a link of each form beside a picture the package holds, which
/// LibreOffice opens or does not (the black-box check that decides what a broken link is), and the files the save writes or refuses.
#[test]
#[ignore = "writes the files the LibreOffice check opens"]
fn write_broken_link_files_for_libreoffice() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_PICTURE_DIR").expect("DIACRAFT_PICTURE_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, href) in [
        ("link_missing", "Pictures/gone.png"),
        ("link_http", "http://example.org/x.png"),
        ("link_file", "file:///tmp/x.png"),
        ("link_absolute", "/tmp/x.png"),
        ("link_parent", "../x.png"),
        ("link_held", "Pictures/a.png"),
        ("link_plain", "gone.png"),
        ("link_empty", ""),
        ("link_object_like", "./Object 1"),
        ("link_replacements", "ObjectReplacements/Object 1"),
    ] {
        let mut d = Drawing::a4().page(&format!("{}{}{}", custom("s1", "1cm"), page_frame("Held", "Pictures/a.png"), page_frame("Link", href)));
        d.files.push(("Pictures/a.png".to_owned(), small_png()));
        std::fs::write(dir.join(format!("{name}.odg")), d.bytes()).unwrap();
    }
    // A file with a broken link and no picture (what keeps saving), and one with a picture and no broken link (what it keeps saving).
    let mut o = drawing(&page_frame("PageFrame", "Pictures/gone.png"), &master_with(None), "");
    let s1 = shape_named(&o.doc, "s1").id;
    move_by(&mut o.doc, 0, s1, 1.0, 0.0);
    std::fs::write(dir.join("broken_edited.odg"), save(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    let mut fine = drawing("", &master_with(None), "");
    add(&mut fine, false, "Picture.1", &small_png());
    std::fs::write(dir.join("picture_saved.odg"), save(&fine.doc, &fine.bytes, &fine.index).unwrap().bytes).unwrap();
}
