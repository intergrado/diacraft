//! What reading and saving a flat drawing costs, counted: a document of many inline pictures and a deep one.
//! Nothing here asserts a time: each case counts the units the loops walk (`ReadWork`, `WriteWork`) and holds them to the size of the document, which crafted shapes
//! cannot make quadratic.

use diacraft_odg::{Limits, OriginalPackage, WriteOptions, WriteWork, read, read_partial, write_counted};

use crate::drawing::Drawing;
use crate::flat::{Flat, base64_lines, frame, picture};
use crate::flat_write::text_of;
use crate::write_core::{custom, full_meta, move_by, open_bytes};

/// A flat drawing of `n` frames, each holding a picture of `size` bytes of its own (no two alike), inline.
fn pictures(n: usize, size: usize) -> Flat {
    let frames: String = (0..n)
        .map(|k| {
            let mut bytes = picture(size);
            bytes.extend((0..4).map(|b| (k >> (8 * b)) as u8));
            let data = base64_lines(&bytes, 76, "\n");
            frame(&format!("<draw:image draw:mime-type=\"image/png\"><office:binary-data>{data}</office:binary-data></draw:image>"))
        })
        .collect();
    Flat::of(&Drawing::a4().page(&frames))
}

/// A flat drawing of a group nested `depth` deep, holding a custom shape `s1` at its centre.
fn deep(depth: usize) -> Flat {
    let mut inner = custom("s1", "1cm");
    for _ in 0..depth {
        inner = format!("<draw:g>{inner}</draw:g>");
    }
    Flat::of(&Drawing::a4().page(&inner))
}

/// Reading a document of many inline pictures costs what they are: the work grows with the pictures, each picture is charged to the media budget by its
/// characters before it is decoded and decoded once.
#[test]
fn odg_package_reading_many_inline_pictures_is_linear_and_counted() {
    let work = |n: usize| {
        let (doc, report) = read(&pictures(n, 600).bytes(), &Limits::default()).unwrap();
        assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(n));
        report.work
    };
    let (a, b, c) = (work(200), work(400), work(800));
    assert_eq!(a.media_bytes, 200 * 604, "each picture is decoded once, and counted in the bytes it decodes to");
    assert_eq!((b.media_bytes, c.media_bytes), (2 * a.media_bytes, 4 * a.media_bytes));
    // What the walk of the tree costs is the same for each frame: the differences over equal steps are equal.
    for (what, f) in [
        ("elements", (|w: &diacraft_odg::ReadWork| w.elements) as fn(&diacraft_odg::ReadWork) -> u64),
        ("attributes", |w| w.attributes),
        ("items", |w| w.items),
    ] {
        let (d1, d2) = (f(&b) - f(&a), f(&c) - f(&b));
        assert_eq!(d2, 2 * d1, "{what}: 200 more frames cost what 200 did, 400 more cost twice that");
    }
    let events = |w: &diacraft_odg::ReadWork| w.package.xml_events;
    assert_eq!(events(&c) - events(&b), 2 * (events(&b) - events(&a)), "the parser reads each frame the same");
    // No package work: nothing is inflated, no entry read.
    assert_eq!((a.package.inflated, a.package.rows), (0, 0));
}

/// A picture past the media budget is refused by its characters before it is decoded, and the cap is exact.
#[test]
fn odg_package_an_inline_picture_is_measured_before_it_is_decoded() {
    let limits = |n: u64| Limits { max_media_bytes: n, ..Limits::default() };
    let one = |size: usize| pictures(1, size).bytes();
    // 1,004 bytes (the picture and its four marking bytes) at a budget of 1,004: decoded; at 1,003: not.
    let (doc, report) = read(&one(1000), &limits(1004)).unwrap();
    assert!(only_shape_has_picture(&doc) && report.work.media_bytes == 1004);
    let (doc, report) = read(&one(1000), &limits(1003)).unwrap();
    assert!(!only_shape_has_picture(&doc), "past the budget it shows none");
    assert_eq!((report.kept("picture_over_budget"), report.work.media_bytes), (1, 0), "refused before a byte was decoded");
    // A large text of base64 is refused as it is counted, not decoded and then judged.
    let big = Flat::of(
        &Drawing::a4().page(&frame(&format!("<draw:image><office:binary-data>{}</office:binary-data></draw:image>", "QUJD".repeat(4_000_000)))),
    );
    let (result, report) = read_partial(&big.bytes(), &limits(1_000_000));
    assert!(result.is_ok());
    assert_eq!((report.kept("picture_over_budget"), report.work.media_bytes), (1, 0));
}

fn only_shape_has_picture(doc: &diacraft_doc::Document) -> bool {
    doc.pages.get(0).and_then(|p| p.shapes.get(0)).and_then(|s| s.foreign.as_ref()).is_some_and(|f| f.picture.is_some())
}

/// A deep document costs what its levels are, one step each; one past the container's depth is refused.
#[test]
fn odg_package_a_deep_flat_document_is_linear_and_refused_past_the_depth_cap() {
    let work = |d: usize| read(&deep(d).bytes(), &Limits::default()).unwrap().1.work;
    let (a, b, c) = (work(40), work(80), work(120));
    for (what, f) in [
        ("elements", (|w: &diacraft_odg::ReadWork| w.elements) as fn(&diacraft_odg::ReadWork) -> u64),
        ("items", |w| w.items),
        ("xml events", |w| w.package.xml_events),
    ] {
        assert_eq!(f(&c) - f(&b), f(&b) - f(&a), "{what}: each level costs the same");
    }
    // The depth cap is the container's 200: the root, `office:body`, `office:drawing` and the page are 4 levels, a group one each, the shape one, so 195 groups
    // is the last that reads.
    let limits = Limits::default();
    let refused = |d: usize| read(&deep(d).bytes(), &limits).err().map(|e| e.class());
    assert_eq!(refused(190), None);
    assert_eq!(refused(210), Some("xml_too_complex"));
    // Saving a deep document is as linear: the scan walks each level once.
    let save_work = |d: usize| {
        let flat = deep(d);
        let mut o = open_bytes(flat.bytes());
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).map(|s| s.id).expect("a group");
        // The group holds the shape: move the whole group.
        move_by(&mut o.doc, 0, id, 1.0, 0.0);
        let mut work = WriteWork::default();
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
        assert!(saved.is_ok(), "{d}: {:?}", saved.err());
        work
    };
    let (a, b, c) = (save_work(40), save_work(80), save_work(120));
    assert_eq!(c.elements_walked - b.elements_walked, b.elements_walked - a.elements_walked, "each level is walked the same number of times");
}

/// The length of the document the pass of a save made, in which the generator is still the producer's `TestWriter/9$Linux` (the last pass writes DiaCraft's),
/// from the document the save gave.
fn made_by_the_pass(saved: &[u8]) -> u64 {
    saved.len() as u64 + "TestWriter/9$Linux".len() as u64 - crate::flat_write::generator(&text_of(saved)).len() as u64
}

/// An unedited save scans nothing and copies nothing of the document; an edit of one shape scans and copies it a fixed number of times (the pass, the last
/// pass, the check of the bytes made), whatever the size, and the edit itself is the same few edits.
#[test]
fn odg_package_a_flat_save_scans_and_copies_the_document_a_fixed_number_of_times() {
    let doc_of = |n: usize| {
        let mut d = Drawing::a4().page(&(1..=n).map(|k| custom(&format!("s{k}"), &format!("{k}cm"))).collect::<String>());
        d.meta = Some(full_meta());
        Flat::of(&d)
    };
    // The work of a save, the document's length, the length of what the pass made, and the length of the result.
    let run = |n: usize, edit: bool| {
        let flat = doc_of(n);
        let mut o = open_bytes(flat.bytes());
        if edit {
            move_by(&mut o.doc, 0, 2, 1.0, 0.0);
        }
        let mut work = WriteWork::default();
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work).unwrap();
        let made = if edit { made_by_the_pass(&saved.bytes) } else { 0 };
        (work, o.bytes.len() as u64, made, saved.bytes.len() as u64)
    };
    let (unedited, ..) = run(300, false);
    assert_eq!(
        (unedited.scan_bytes, unedited.bytes_copied, unedited.edits, unedited.attributes_written),
        (0, 0, 0, 0),
        "an unedited save does nothing to the document"
    );
    let ((a, a_len, a_pass, a_out), (b, b_len, b_pass, b_out)) = (run(300, true), run(600, true));
    // The pass scans the document, the last pass scans what the pass made, and the check of the bytes made scans the result as each of the two parts of a package.
    assert_eq!(a.scan_bytes, a_len + a_pass + 2 * a_out);
    assert_eq!(b.scan_bytes, b_len + b_pass + 2 * b_out);
    // The pass copies the document and the last pass what the pass made, but for the bytes the edits replace (the value `2cm`, 3 bytes, and the producer's
    // generator, 18); the text the edits put in is copied too, and counted on its own as well.
    let replaced = 3 + 18;
    assert_eq!(a.bytes_copied, a_len + a_pass + a.bytes_written - replaced);
    assert_eq!(b.bytes_copied, b_len + b_pass + b.bytes_written - replaced);
    assert_eq!((a.edits, a.attributes_written), (b.edits, b.attributes_written), "the same few edits whatever the size of the document");
    assert!(a.edits <= 4 && a.attributes_written <= 4, "{} {}", a.edits, a.attributes_written);
    let ratio = b.scan_bytes as f64 / a.scan_bytes as f64;
    assert!((1.9..2.1).contains(&ratio), "twice the shapes, twice the scan: {ratio}");
}

/// The pictures of a document are scanned and copied with the rest of its bytes, and never decoded by the save beyond the one read of the baseline.
#[test]
fn odg_package_saving_a_document_of_many_pictures_decodes_them_once() {
    let run = |n: usize| {
        let mut d = Drawing::a4().page(&format!("{}{}", pictures(n, 2000).pages(), custom("mover", "1cm")));
        d.meta = Some(full_meta());
        let flat = Flat::of(&d);
        let mut o = open_bytes(flat.bytes());
        let mover = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("mover")).map(|s| s.id).unwrap();
        move_by(&mut o.doc, 0, mover, 1.0, 0.0);
        let mut work = WriteWork::default();
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work).unwrap();
        (work, o.bytes.len() as u64, made_by_the_pass(&saved.bytes), saved.bytes.len() as u64)
    };
    let ((a, a_len, a_pass, a_out), (b, b_len, b_pass, b_out)) = (run(100), run(200));
    assert_eq!(a.baseline_read.media_bytes, 100 * 2004, "the baseline read decodes each picture once");
    assert_eq!(b.baseline_read.media_bytes, 200 * 2004);
    assert_eq!((a.scan_bytes, b.scan_bytes), (a_len + a_pass + 2 * a_out, b_len + b_pass + 2 * b_out));
    assert!(a.attributes_written <= 4 && b.attributes_written == a.attributes_written);
}

impl Flat {
    /// The page element of the first page's pictures, as the text of what `office:drawing` holds.
    fn pages(&self) -> String {
        let text = self.body.clone();
        let from = text.find("<office:drawing>").map_or(0, |i| i + "<office:drawing>".len());
        let to = text.find("</office:drawing>").unwrap_or(text.len());
        let inside = &text[from..to];
        // The shapes of the one page: between its start tag and its end tag.
        let start = inside.find('>').map_or(0, |i| i + 1);
        let end = inside.rfind("</draw:page>").unwrap_or(inside.len());
        inside[start..end].to_owned()
    }
}
