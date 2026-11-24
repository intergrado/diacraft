//! The names a new picture takes (the review): a new picture never takes a name that any part of the
//! package already names, wherever the name is written and however it is spelt, because a frame that names an entry the package lacks shows the picture put there.
//!
//! The reader resolves an `xlink:href` by percent-decoding it and then stripping a leading `./`, else as written; the writer reserves each href of **both** parts, `content.xml`
//! and `styles.xml`, before it names any picture, whether or not the part is one the save changes, and in both spellings.

use std::sync::Arc;

use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::build;
use crate::drawing::Drawing;
use crate::flat_write::small_png;
use crate::write_core::{Open, custom, open_bytes, part};
use crate::write_picture::{PNG_HEAD, frame_of, manifest_rows, named, noise, picture, pictures_of, save, save_with, shown};

/// The master page `Default` holding a frame that names `href`, and a rectangle (a master page that holds a shape is a page of the model).
pub fn master_with(href: Option<&str>) -> String {
    let frame = href
        .map(|h| {
            format!(
                "<draw:frame draw:name=\"MasterFrame\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"{h}\"/></draw:frame>"
            )
        })
        .unwrap_or_default();
    format!(
        "<style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"><draw:rect draw:name=\"bg\" svg:x=\"1cm\" svg:y=\"6cm\" svg:width=\"2cm\" svg:height=\"1cm\"/>{frame}</style:master-page>"
    )
}

/// A frame of the page that names `href`, called `name`.
pub fn page_frame(name: &str, href: &str) -> String {
    format!(
        "\n    <draw:frame draw:name=\"{name}\" svg:x=\"1cm\" svg:y=\"{}cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"{href}\"/></draw:frame>",
        2 + name.len()
    )
}

/// A drawing of one page holding `page`, and the master page `master`.
pub fn drawing(page: &str, master: &str, common: &str) -> Open {
    let mut d = Drawing::a4().page(&format!("{}{page}", custom("s1", "1cm")));
    d.masters = d.masters.replace(r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#, master);
    d.common = common.to_owned();
    open_bytes(d.bytes())
}

/// Puts a new picture on the page (`on_master` false) or on the master page.
pub fn add(o: &mut Open, on_master: bool, name: &str, bytes: &[u8]) {
    let at = o.doc.pages.iter().position(|p| p.background == on_master).expect("the page");
    let id = o.index.next_id(at).unwrap();
    let page = Arc::make_mut(o.doc.pages.get_mut(at).unwrap());
    page.shapes.push(Arc::new(picture(id, name, (2.0, 8.0, 3.0, 2.0), bytes)));
}

/// The shape called `name` on any page of the model, as the file shows it.
pub fn shape_named<'a>(doc: &'a diacraft_doc::Document, name: &str) -> &'a diacraft_doc::Shape {
    doc.pages.iter().flat_map(|p| p.shapes.iter()).find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).as_ref()
}

/// A hyperlink at `href`, as the text of a custom shape called `name` on the page.
fn link_shape(name: &str, href: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" svg:x=\"1cm\" svg:y=\"{}cm\" svg:width=\"4cm\" svg:height=\"2cm\"><text:p><text:a xlink:href=\"{href}\" xlink:type=\"simple\">link</text:a></text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>",
        9 + name.len()
    )
}

/// The master page `Default` holding a shape whose text is a hyperlink at `href` (and a rectangle).
fn master_with_link(href: &str) -> String {
    format!(
        "<style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"><draw:rect draw:name=\"bg\" svg:x=\"1cm\" svg:y=\"6cm\" svg:width=\"2cm\" svg:height=\"1cm\"/>{}</style:master-page>",
        link_shape("MasterLink", href)
    )
}

/// (a), kept as a guard after a change: a name that a link in `styles.xml` holds (a hyperlink, which is no picture link and so no broken one) is not given to a new
/// picture on a page (a save that changes `content.xml` alone).
#[test]
fn a_new_picture_on_a_page_does_not_take_the_name_a_hyperlink_of_a_master_page_in_styles_xml_holds() {
    let png = small_png();
    let mut o = drawing("", &master_with_link("Pictures/image1.png"), "");
    add(&mut o, false, "Picture.1", &png);
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a picture is written");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image2.png"], "the name the master page's hyperlink holds is taken: the picture is the next");
    assert!(frame_of(&part(&saved.bytes, "content.xml"), "Picture.1").contains("Pictures/image2.png"));
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "styles.xml is the file's own");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(shown(shape_named(&back, "Picture.1")).map(|s| s.0), Some(png));
}

/// (c): a new picture on a master page (a save that changes `styles.xml` alone) beside a hyperlink of an untouched `content.xml`.
#[test]
fn a_new_picture_on_a_master_page_does_not_take_the_name_a_hyperlink_of_an_untouched_content_xml_holds() {
    let mut o = drawing(&link_shape("PageLink", "Pictures/image1.png"), &master_with(None), "");
    add(&mut o, true, "Picture.1", &small_png());
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a picture is written");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image2.png"]);
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml"), "content.xml is the file's own");
}

/// (d): the reader percent-decodes an href and then strips `./`, so `%2E/Pictures/image1.png` and `.%2FPictures/image1.png` are `Pictures/image1.png`; the writer reserved the
/// name only when the `./` was in the text as written. Both spellings, and the one with a letter encoded, are held.
#[test]
fn a_new_picture_does_not_take_a_name_a_hyperlink_spells_with_a_percent_encoded_dot_or_slash_or_letter() {
    for (spelling, names) in [
        ("%2E/Pictures/image1.png", ["Pictures/image2.png"]),
        (".%2FPictures/image1.png", ["Pictures/image2.png"]),
        ("Pictures/image%31.png", ["Pictures/image2.png"]),
        ("./Pictures/image1.png", ["Pictures/image2.png"]),
    ] {
        let mut o = drawing(&link_shape("Link", spelling), &master_with(None), "");
        add(&mut o, false, "Picture.1", &small_png());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{spelling}: {e}"));
        assert_eq!(pictures_of(&saved.bytes), names, "{spelling}");
    }
}

/// The two parts are both looked through whichever the save changes, and a name reserved by one is not given in the other: a hyperlink in each part, a picture on the page and one on
/// the master page, four names in all.
#[test]
fn a_name_a_hyperlink_holds_is_reserved_whichever_part_the_save_changes() {
    let (first, second) = (small_png(), noise(&PNG_HEAD, 64, 3));
    let mut o = drawing(&link_shape("PageLink", "Pictures/image2.png"), &master_with_link("Pictures/image1.png"), "");
    add(&mut o, false, "OnPage", &first);
    add(&mut o, true, "OnMaster", &second);
    let saved = save(&o.doc, &o.bytes, &o.index).expect("two pictures are written");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image3.png", "Pictures/image4.png"]);
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(shown(shape_named(&back, "OnPage")).map(|s| s.0), Some(first));
    assert_eq!(shown(shape_named(&back, "OnMaster")).map(|s| s.0), Some(second));
}

/// A link that is outside the package (an IRI with a scheme) names nothing of the package's: the first name is free.
#[test]
fn a_name_no_link_and_no_entry_holds_is_still_the_first_free_one() {
    let mut o = drawing(&link_shape("Link", "http://example.org/Pictures/image1.png"), &master_with(None), "");
    add(&mut o, false, "Picture.1", &small_png());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"], "a link outside the package names nothing of the package's");
}

/// A manifest row with no entry, in capitals, is a name that is held (the lower-casing of the rows is what makes the check case-blind).
#[test]
fn a_manifest_row_with_no_entry_holds_its_name_whatever_its_case() {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let mut files = d.files();
    let mut rows: Vec<(String, &str)> = files.iter().map(|(n, _)| (n.clone(), "text/xml")).collect();
    rows.push(("Pictures/IMAGE1.PNG".to_owned(), "image/png"));
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    files.push(("META-INF/manifest.xml".to_owned(), build::manifest(build::DRAWING, &rows).into_bytes()));
    let mut o = open_bytes(build::write(Some(build::DRAWING), &files));
    add(&mut o, false, "Picture.1", &small_png());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image2.png"], "{:?}", manifest_rows(&saved.bytes));
}

/// An entry the manifest omits, in capitals, is a name that is held too (the lower-casing of the entries is what makes the check case-blind for the files of a package).
#[test]
fn an_entry_the_manifest_omits_holds_its_name_whatever_its_case() {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let mut files = d.files();
    let rows: Vec<(String, &str)> = files.iter().map(|(n, _)| (n.clone(), "text/xml")).collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    files.push(("META-INF/manifest.xml".to_owned(), build::manifest(build::DRAWING, &rows).into_bytes()));
    // After the manifest is made, so that the manifest does not list it.
    files.insert(files.len() - 1, ("Pictures/IMAGE1.PNG".to_owned(), small_png()));
    let mut o = open_bytes(build::write(Some(build::DRAWING), &files));
    add(&mut o, false, "Picture.1", &small_png());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/IMAGE1.PNG", "Pictures/image2.png"]);
}

/// A part the save cannot look through for the names it holds (`styles.xml` in UTF-16, which the reader reads and a splice cannot scan) is no part a picture is put beside without a
/// word: refused in the words of that, and not as a fault of the check of the bytes made (every edited save of such a package is refused by that check, so nothing is lost by it).
#[test]
fn a_part_that_cannot_be_looked_through_for_its_names_refuses_a_new_picture_in_words() {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let styles = String::from_utf8(d.files().into_iter().find(|(n, _)| n == "styles.xml").unwrap().1).unwrap();
    let styles = styles.replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"");
    let files = build::replace(d.files(), "styles.xml", crate::flat::utf16le(&styles));
    let bytes = build::package(build::DRAWING, files);
    let (doc, report) = read(&bytes, &Limits::default()).expect("the reader reads a styles.xml in UTF-16");
    let mut o = crate::write_core::Open { bytes, doc, index: report.index };
    add(&mut o, false, "Picture.1", &small_png());
    let e = save(&o.doc, &o.bytes, &o.index).expect_err("a picture beside a part that cannot be looked through");
    assert_eq!(e.class(), "write_unsupported", "{e}");
    assert!(e.to_string().contains("look through"), "{e}");
}

/// The manifest's rows are a cap of the package the reader opens: past it, the save is a limit in words and not a fault of the save (LOW: `too_many_manifest_entries` was a self-check
/// fault). The picture fits the room the book counts; the thumbnail's row is the one more.
#[test]
fn a_row_the_save_makes_beside_the_pictures_that_takes_the_manifest_past_its_cap_is_a_limit() {
    let mut o = crate::write_picture::three();
    add(&mut o, false, "Picture.1", &small_png());
    let rows = manifest_rows(&o.bytes).len();
    let with_thumbnail = |max_manifest_entries: usize| {
        write_with(
            &o.doc,
            Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &WriteOptions {
                thumbnail: Some(crate::write_core::PNG.to_vec()),
                limits: Limits { max_manifest_entries, ..Limits::default() },
                ..WriteOptions::default()
            },
        )
    };
    match with_thumbnail(rows + 1) {
        Err(WriteError::Limit { what, max }) => assert!(what.contains("rows of a manifest") && max == (rows + 1) as u64, "{what} {max}"),
        other => panic!("a limit: {:?}", other.map(|w| w.bytes.len())),
    }
    assert!(with_thumbnail(rows + 2).is_ok(), "room for both rows");
}

/// LOW: the same picture repeated is not refused where different ones are not. A copy costs a hash and a comparison, and what it saves is a copy of the entry: so the comparison is
/// charged to the bytes copied, and the hash to the bytes scanned, and 100 copies of a 40,000-byte picture that a ceiling of 100 lengths and some room for the parts the save scans and
/// copies for each of the two admits are saved (both charged to the scan, as they were, they come to 200 lengths).
#[test]
fn copies_of_one_picture_are_charged_a_hash_each_and_the_comparison_against_the_copy_they_save() {
    let len = 40_000usize;
    let bytes = noise(&PNG_HEAD, len, 11);
    let one = |copies: usize, max_write_bytes: u64| {
        let mut o = crate::write_picture::three();
        let first = o.index.next_id(0).unwrap();
        for k in 0..copies {
            // Each copy has its own `Arc` of the same bytes, as a paste in the engine does.
            let mut shape = picture(first + k as u32, &format!("P{k}"), (1.0, 8.0, 0.5, 0.5), &bytes);
            if let Some(f) = shape.foreign.as_mut() {
                let f = Arc::make_mut(f);
                if let Some(m) = f.picture.as_mut() {
                    Arc::make_mut(m).bytes = Arc::from(bytes.to_vec());
                }
            }
            crate::write_new::push_shape(&mut o.doc, shape);
        }
        save_with(&o.doc, &o.bytes, &o.index, Limits { max_write_bytes, ..Limits::default() })
    };
    let room = (100 * len + 300_000) as u64;
    let saved = one(100, room).expect("100 copies of one picture are saved");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
    assert!(saved.work.scan_bytes >= (100 * len) as u64 && saved.work.scan_bytes < room, "the hashes are the scan's: {}", saved.work.scan_bytes);
    assert!(
        saved.work.bytes_copied >= (99 * len) as u64 && saved.work.bytes_copied < room,
        "and the comparisons are the copies they save: {}",
        saved.work.bytes_copied
    );
    assert!(matches!(one(100, (50 * len) as u64), Err(WriteError::Limit { .. })), "fewer lengths than the hashes need is a limit");
}

/// The review's input: 10,000 copies of one 64 KiB picture, each in an `Arc` of its own (655 MB of bytes in the model, as a paste of 10,000 frames holds), are saved as one entry. The
/// hash and the comparison were both charged to the scan budget (1 GiB) and the 10,000 copies took 1.3 GB of it: refused, where 10,000 different pictures saved.
#[test]
fn ten_thousand_copies_of_one_64_kib_picture_are_saved_as_one_entry() {
    let bytes = noise(&PNG_HEAD, 64 * 1024, 13);
    let mut o = crate::write_picture::three();
    let first = o.index.next_id(0).unwrap();
    let copies = 10_000u32;
    for k in 0..copies {
        let mut shape = picture(first + k, &format!("P{k}"), (1.0 + f64::from(k % 18), 8.0, 0.5, 0.5), &[0x89, b'P', b'N', b'G']);
        if let Some(f) = shape.foreign.as_mut() {
            let f = Arc::make_mut(f);
            if let Some(m) = f.picture.as_mut() {
                Arc::make_mut(m).bytes = Arc::from(bytes.as_slice());
            }
        }
        crate::write_new::push_shape(&mut o.doc, shape);
    }
    let saved = save(&o.doc, &o.bytes, &o.index).expect("10,000 copies of one picture are saved");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
    assert_eq!(manifest_rows(&saved.bytes).len(), manifest_rows(&o.bytes).len() + 1, "one row");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("picture_missing"), 0);
    assert_eq!(shown(named(&back, "P9999")).map(|s| s.0), Some(bytes));
}

/// `DIACRAFT_PICTURE_DIR`: where the files are written (run with `--run-ignored only`): one
/// of new rectangles and a connector with arrow sizes beside no end and beside an end. LibreOffice opens them as a black box.
#[test]
#[ignore = "writes the files the LibreOffice check opens"]
fn write_names_and_arrow_files_for_libreoffice() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_PICTURE_DIR").expect("DIACRAFT_PICTURE_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut o = open_bytes(Drawing::a4().page(&custom("s1", "1cm")).bytes());
    let first = o.index.next_id(0).unwrap();
    let cells: [&[(&str, &str)]; 4] = [
        &[("EndArrow", "4"), ("EndArrowSize", "5")],
        &[("BeginArrowSize", "4")],
        &[("BeginArrow", "1"), ("BeginArrowSize", "3"), ("EndArrow", "4"), ("EndArrowSize", "1")],
        &[("EndArrow", "0"), ("EndArrowSize", "5")],
    ];
    for (k, ends) in cells.iter().enumerate() {
        let mut shape = crate::write_new::rectangle(first + k as u32, &format!("R{k}"), (2.0, 4.0 + 5.0 * k as f64, 6.0, 2.0));
        for (name, value) in *ends {
            Arc::make_mut(&mut shape.sheet)
                .cells
                .insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::read(None, Some(value), None, None, &mut 1_000));
        }
        crate::write_new::push_shape(&mut o.doc, shape);
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    std::fs::write(dir.join("arrow_sizes.odg"), &saved.bytes).unwrap();
    // The same opened again, a shape moved, saved again.
    let mut again = open_bytes(saved.bytes.clone());
    let s1 = shape_named(&again.doc, "s1").id;
    crate::write_core::move_by(&mut again.doc, 0, s1, 1.0, 0.0);
    std::fs::write(dir.join("arrow_sizes_moved.odg"), save(&again.doc, &again.bytes, &again.index).unwrap().bytes).unwrap();
}
