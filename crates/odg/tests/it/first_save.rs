//! The first save of a new drawing writes its thumbnail (and the project rule of
//! `WriteOptions::first_save`): a package DiaCraft made for a drawing that has no file yet has no thumbnail, and a save that is no edit would copy it
//! without one. With `first_save` and a thumbnail passed, that save adds the picture and its manifest row and changes nothing else (the kind it is asked
//! to state, when it is asked). A package that has a thumbnail keeps it. A flat drawing has none to write.

use diacraft_odg::package::Package;
use diacraft_odg::{Form, Limits, OriginalPackage, SaveNote, Variant, WriteError, WriteOptions, Written, diff_packages, read, write_with};

use crate::build::{DRAWING, TEMPLATE, with_manifest, write};
use crate::drawing::{Drawing, meta, settings};
use crate::write_core::PNG;

/// The picture of a new drawing's one image: any bytes the reader takes as a picture by its signature.
const PICTURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// `n` rectangles in a row, which the reader numbers 1 to `n`.
fn rects(n: usize) -> String {
    (1..=n).map(|k| format!(r#"<draw:rect svg:x="{k}cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#)).collect()
}

/// The parts of a new drawing them: `content.xml`, `styles.xml`, `meta.xml` and `settings.xml`, of one page of two rectangles.
fn parts() -> Vec<(String, Vec<u8>)> {
    let mut d = Drawing::a4().page(&rects(2));
    d.meta = Some(meta("DiaCraft/test"));
    d.settings = Some(settings(""));
    d.files()
}

/// A new drawing's package as R25 lists it: `mimetype`, the four parts, a picture, then the manifest, with no thumbnail. `media` is the kind it states.
fn new_drawing(media: &str) -> Vec<u8> {
    let mut parts = parts();
    parts.push(("Pictures/a.png".to_owned(), PICTURE.to_vec()));
    write(Some(media), &with_manifest(media, parts))
}

/// The package of `bytes` saved with `options`: read, then written back unchanged.
fn save(bytes: &[u8], options: &WriteOptions) -> Result<Written, WriteError> {
    let (doc, report) = read(bytes, &Limits::default()).unwrap();
    write_with(&doc, Some(&OriginalPackage { bytes, index: &report.index }), options)
}

/// The options of a first save with `thumbnail`.
fn first(thumbnail: Option<&[u8]>) -> WriteOptions {
    WriteOptions { first_save: true, thumbnail: thumbnail.map(<[u8]>::to_vec), ..WriteOptions::default() }
}

fn package(bytes: &[u8]) -> Package {
    Package::read(bytes, &Limits::default()).unwrap()
}

/// The names of the entries of a package in the order they lie in the file.
fn names(bytes: &[u8]) -> Vec<String> {
    package(bytes).entries().map(|e| e.name.clone()).collect()
}

/// The paths of the manifest's rows in order.
fn rows(bytes: &[u8]) -> Vec<String> {
    package(bytes).manifest().entries.iter().map(|e| e.full_path.clone()).collect()
}

/// The first save of a new drawing of one page holding one PNG picture writes `mimetype` first, the parts in the order, the
/// picture, **the thumbnail before the manifest** (a new entry is appended before `META-INF/manifest.xml` when the manifest was the last entry),
/// and the manifest's rows in the order `/`, `content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, the picture, the thumbnail (`image/png`).
#[test]
fn odg_package_the_first_save_of_a_new_drawing_writes_its_thumbnail_and_its_manifest_row() {
    let new = new_drawing(DRAWING);
    assert!(!names(&new).iter().any(|n| n.starts_with("Thumbnails/")), "the new package has no thumbnail");
    let saved = save(&new, &first(Some(&PNG))).unwrap();
    assert_eq!(
        names(&saved.bytes),
        ["mimetype", "content.xml", "styles.xml", "meta.xml", "settings.xml", "Pictures/a.png", "Thumbnails/thumbnail.png", "META-INF/manifest.xml"]
    );
    assert_eq!(
        rows(&saved.bytes),
        ["/", "content.xml", "styles.xml", "meta.xml", "settings.xml", "Pictures/a.png", "Thumbnails/thumbnail.png"],
        "the row follows the last"
    );
    let p = package(&saved.bytes);
    assert_eq!(p.part("Thumbnails/thumbnail.png"), Some(&PNG[..]));
    assert_eq!(p.manifest().entry("Thumbnails/thumbnail.png").and_then(|e| e.media_type.as_deref()), Some("image/png"));
    assert_eq!(p.media_type(), Some(DRAWING));
    // The model is not edited, and the index is the one the save was given: the next save compares with it.
    assert!(!saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    // Read back: the same drawing, and nothing odd (the picture and its row are what the package already had).
    let (doc, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(2));
}

/// The first save changes **nothing else**: every entry keeps its bytes, method and header, the thumbnail is the one entry added, and the manifest
/// is the one entry changed, by one row.
#[test]
fn odg_package_the_first_save_adds_the_thumbnail_and_the_manifest_row_and_nothing_else() {
    let new = new_drawing(DRAWING);
    let saved = save(&new, &first(Some(&PNG))).unwrap();
    let changes = diff_packages(&new, &saved.bytes, &Limits::default()).unwrap();
    assert_eq!(changes.added, ["Thumbnails/thumbnail.png"]);
    assert!(
        changes.removed.is_empty() && changes.methods_changed.is_empty() && changes.headers_changed.is_empty() && !changes.order_changed,
        "{changes:?}"
    );
    assert!(changes.unparsed.is_empty() && changes.bytes_only.is_empty(), "{changes:?}");
    let touched: Vec<&str> = changes.parts.iter().map(|p| p.part.as_str()).collect();
    assert_eq!(touched, ["META-INF/manifest.xml"], "no part, no meta.xml, no generator");
    // Five entries, as they were: `mimetype`, four parts and the picture.
    assert_eq!(changes.identical, 6);
    // The manifest as it was, one row more, in the spelling the file has.
    let (before, after) = (part_text(&new, "META-INF/manifest.xml"), part_text(&saved.bytes, "META-INF/manifest.xml"));
    let head = before.trim_end_matches("</manifest:manifest>\n");
    assert!(after.starts_with(head), "{after}");
    assert_eq!(
        &after[head.len()..],
        " <manifest:file-entry manifest:full-path=\"Thumbnails/thumbnail.png\" manifest:media-type=\"image/png\"/>\n</manifest:manifest>\n"
    );
}

fn part_text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(package(bytes).part(name).unwrap().to_vec()).unwrap()
}

/// A package that has a thumbnail keeps its bytes on a save that is no edit, a first save included: the picture passed does not replace it.
#[test]
fn odg_package_a_first_save_keeps_the_thumbnail_a_package_has() {
    let mut parts = parts();
    parts.push(("Thumbnails/thumbnail.png".to_owned(), vec![0x89, b'P', b'N', b'G', 9, 9]));
    let theirs = write(Some(DRAWING), &with_manifest(DRAWING, parts));
    let saved = save(&theirs, &first(Some(&PNG))).unwrap();
    assert_eq!(saved.bytes, theirs, "the package as it was, every byte");
    assert!(!saved.edited);
    assert_eq!(package(&saved.bytes).part("Thumbnails/thumbnail.png"), Some(&[0x89, b'P', b'N', b'G', 9, 9][..]));
}

/// A first save that is passed no thumbnail asks for nothing: the package is copied as it is. And a thumbnail passed to a save that is not a first save, on
/// a drawing that is not edited, is not written either (an unedited save keeps the package's own bytes), whatever the package has.
#[test]
fn odg_package_only_a_first_save_adds_a_thumbnail_to_an_unedited_drawing() {
    let new = new_drawing(DRAWING);
    assert_eq!(save(&new, &first(None)).unwrap().bytes, new, "a first save with no picture is a copy");
    let not_first = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    assert_eq!(save(&new, &not_first).unwrap().bytes, new, "the flag is what asks for it");
    assert_eq!(save(&new, &WriteOptions::default()).unwrap().bytes, new);
}

/// A second save after the first is no edit and gives the same bytes: the package has a thumbnail now, with the index the first save gave, and the same
/// options (the engine passes them again for a drawing it has not saved yet only; passing them again does no harm).
#[test]
fn odg_package_a_second_save_is_no_edit_and_the_same_bytes() {
    let new = new_drawing(DRAWING);
    let (doc, report) = read(&new, &Limits::default()).unwrap();
    let options = first(Some(&PNG));
    let one = write_with(&doc, Some(&OriginalPackage { bytes: &new, index: &report.index }), &options).unwrap();
    let two = write_with(&doc, Some(&OriginalPackage { bytes: &one.bytes, index: &one.index }), &options).unwrap();
    assert_eq!((two.bytes.as_slice(), two.edited), (one.bytes.as_slice(), false));
    let three = write_with(&doc, Some(&OriginalPackage { bytes: &two.bytes, index: &two.index }), &WriteOptions::default()).unwrap();
    assert_eq!(three.bytes, one.bytes);
    assert!(two.notes.is_empty() && three.notes.is_empty());
}

/// With a first save: a drawing made from a template package, saved as a drawing, states the kind in `mimetype` and in the manifest's root row, and
/// gains its thumbnail; no other byte of the package moves. A drawing saved as a template is the same in the other direction.
#[test]
fn odg_package_a_first_save_that_states_another_kind_writes_the_kind_and_the_thumbnail_and_nothing_else() {
    for (own, asked) in [(TEMPLATE, Variant::Drawing), (DRAWING, Variant::Template)] {
        let new = new_drawing(own);
        let options = WriteOptions { variant: Some(asked), ..first(Some(&PNG)) };
        let saved = save(&new, &options).unwrap();
        assert_eq!(package(&saved.bytes).variant(), Some(asked));
        assert_eq!(package(&saved.bytes).manifest().root().and_then(|r| r.media_type.as_deref()), Some(asked.media_type()));
        assert_eq!(package(&saved.bytes).part("Thumbnails/thumbnail.png"), Some(&PNG[..]));
        let changes = diff_packages(&new, &saved.bytes, &Limits::default()).unwrap();
        assert_eq!(changes.added, ["Thumbnails/thumbnail.png"], "{own}");
        let touched: Vec<&str> = changes.parts.iter().map(|p| p.part.as_str()).collect();
        assert_eq!(touched, ["META-INF/manifest.xml"], "{own}");
        assert_eq!(changes.bytes_only, ["mimetype"], "{own}");
        assert!(changes.removed.is_empty() && !changes.order_changed, "{changes:?}");
        assert_eq!(rows(&saved.bytes).last().map(String::as_str), Some("Thumbnails/thumbnail.png"));
        // The kind it already is, asked for, is no change of kind: only the thumbnail is added.
        let same = save(&new, &WriteOptions { variant: Variant::from_media_type(own), ..first(Some(&PNG)) }).unwrap();
        let changes = diff_packages(&new, &same.bytes, &Limits::default()).unwrap();
        assert_eq!((changes.added.as_slice(), changes.bytes_only.len()), (&["Thumbnails/thumbnail.png".to_owned()][..], 0), "{own}");
    }
}

/// A signed package is changed by the save, and the save says so.
#[test]
fn odg_package_a_first_save_of_a_signed_package_says_the_signature_is_stale() {
    let mut parts = parts();
    parts.push(("META-INF/documentsignatures.xml".to_owned(), b"<sig/>".to_vec()));
    let signed = write(Some(DRAWING), &with_manifest(DRAWING, parts));
    let saved = save(&signed, &first(Some(&PNG))).unwrap();
    assert_eq!(saved.notes, [SaveNote::SignatureStale]);
    assert_eq!(part_text(&saved.bytes, "META-INF/documentsignatures.xml"), "<sig/>", "the signature file keeps its bytes");
}

/// A package whose manifest lists the thumbnail and whose archive holds no entry for it (a row for a file the package lacks stays as it is).
fn lists_a_thumbnail_it_lacks() -> Vec<u8> {
    let mut with = with_manifest(DRAWING, parts());
    // The manifest is last: its text is replaced by one that lists the thumbnail (which the archive does not hold).
    let rows_text = [
        ("content.xml", "text/xml"),
        ("styles.xml", "text/xml"),
        ("meta.xml", "text/xml"),
        ("settings.xml", "text/xml"),
        ("Thumbnails/thumbnail.png", "image/png"),
    ];
    let at = with.len() - 1;
    with[at] = ("META-INF/manifest.xml".to_owned(), crate::build::manifest(DRAWING, &rows_text).into_bytes());
    write(Some(DRAWING), &with)
}

/// A manifest that lists the thumbnail already, with no entry for it, gets no second row (a row for a file the package lacks stays as it is).
#[test]
fn odg_package_a_manifest_that_lists_the_thumbnail_already_gets_no_second_row() {
    let listed = lists_a_thumbnail_it_lacks();
    let saved = save(&listed, &first(Some(&PNG))).unwrap();
    assert_eq!(rows(&saved.bytes).iter().filter(|r| *r == "Thumbnails/thumbnail.png").count(), 1);
    assert_eq!(package(&saved.bytes).part("Thumbnails/thumbnail.png"), Some(&PNG[..]), "the entry is added all the same");
    assert_eq!(part_text(&saved.bytes, "META-INF/manifest.xml"), part_text(&listed, "META-INF/manifest.xml"), "the manifest is as it was");
}

/// **an edited save** does the same: the entry is added and the manifest, which lists it, gets no second row (the edited path wrote
/// one, which the first-save path never did). The manifest is the file's own, but for nothing; a second save is no edit.
#[test]
fn odg_package_an_edited_save_gives_a_manifest_that_lists_the_thumbnail_no_second_row() {
    let listed = lists_a_thumbnail_it_lacks();
    let (mut doc, report) = read(&listed, &Limits::default()).unwrap();
    crate::write_core::move_by(&mut doc, 0, 1, 1.0, 0.0);
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    let saved = write_with(&doc, Some(&OriginalPackage { bytes: &listed, index: &report.index }), &options).unwrap();
    assert!(saved.edited);
    assert_eq!(
        rows(&saved.bytes).iter().filter(|r| *r == "Thumbnails/thumbnail.png").count(),
        1,
        "one row for the thumbnail: {:?}",
        rows(&saved.bytes)
    );
    assert_eq!(package(&saved.bytes).part("Thumbnails/thumbnail.png"), Some(&PNG[..]), "the entry is added");
    assert_eq!(part_text(&saved.bytes, "META-INF/manifest.xml"), part_text(&listed, "META-INF/manifest.xml"), "the manifest is as it was");
    let again = write_with(&doc, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// A picture that is not a PNG, or is larger than the limit, is refused on a first save as on any save, and nothing is written; a picture of exactly the
/// limit is written, and a signature alone is a picture.
#[test]
fn odg_package_a_first_save_refuses_a_thumbnail_that_is_not_a_png_or_is_too_large() {
    let new = new_drawing(DRAWING);
    assert!(matches!(save(&new, &first(Some(b"GIF89a"))), Err(WriteError::Thumbnail(_))));
    let limited = |max: usize| save(&new, &WriteOptions { limits: Limits { max_thumbnail_bytes: max, ..Limits::default() }, ..first(Some(&PNG)) });
    assert!(matches!(limited(PNG.len() - 1), Err(WriteError::Limit { .. })));
    assert!(limited(PNG.len()).is_ok());
    let alone = save(&new, &first(Some(&PICTURE))).unwrap();
    assert_eq!(package(&alone.bytes).part("Thumbnails/thumbnail.png"), Some(&PICTURE[..]));
}

/// An edited save writes the thumbnail as it always did, and the first-save flag changes nothing of it (the picture is written once, the one row once, and
/// the save is an edit).
#[test]
fn odg_package_an_edited_first_save_writes_the_thumbnail_as_an_edited_save_does() {
    let new = new_drawing(DRAWING);
    let (mut doc, report) = read(&new, &Limits::default()).unwrap();
    crate::write_core::move_by(&mut doc, 0, 1, 1.0, 0.0);
    let original = OriginalPackage { bytes: &new, index: &report.index };
    let with_flag = write_with(&doc, Some(&original), &first(Some(&PNG))).unwrap();
    let without = write_with(&doc, Some(&original), &WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() }).unwrap();
    assert!(with_flag.edited && without.edited);
    assert_eq!(with_flag.bytes, without.bytes, "the flag changes nothing of an edited save");
    assert_eq!(rows(&with_flag.bytes).iter().filter(|r| *r == "Thumbnails/thumbnail.png").count(), 1);
    assert_eq!(package(&with_flag.bytes).part("Thumbnails/thumbnail.png"), Some(&PNG[..]));
}

/// A drawing of two shapes with no picture, as a package: what a flat document can be made of whole.
fn plain_drawing() -> Vec<u8> {
    write(Some(DRAWING), &with_manifest(DRAWING, parts()))
}

/// **A flat drawing has no thumbnail**, so a first save with one is the save with none: the input's bytes when nothing changed, and no
/// PNG signature, no `Thumbnails` and no manifest in what a package is written as when it is written as a flat document.
#[test]
fn odg_package_a_flat_drawing_is_given_no_thumbnail_on_a_first_save() {
    let package = plain_drawing();
    let as_flat = |options: WriteOptions| save(&package, &WriteOptions { form: Some(Form::Flat), ..options }).unwrap();
    // A package written as a flat document: the same document with or without the picture and the flag.
    let with = as_flat(first(Some(&PNG)));
    let without = as_flat(WriteOptions::default());
    assert_eq!(with.bytes, without.bytes);
    assert!(!with.bytes.windows(4).any(|w| w == b"\x89PNG"), "no PNG in a flat document");
    let text = String::from_utf8_lossy(&with.bytes).into_owned();
    assert!(!text.contains("humbnail") && !text.contains("manifest"), "no thumbnail and no manifest in a flat document");
    // A flat drawing saved with the flag and a picture: the input's bytes, no edit.
    let saved = save(&without.bytes, &first(Some(&PNG))).unwrap();
    assert_eq!(saved.bytes, without.bytes, "an unedited flat save is the input");
    assert!(!saved.edited);
    // Edited, it is the same save with and without the picture.
    let (mut doc, report) = read(&without.bytes, &Limits::default()).unwrap();
    crate::write_core::move_by(&mut doc, 0, 1, 1.0, 0.0);
    let original = OriginalPackage { bytes: &without.bytes, index: &report.index };
    let moved = write_with(&doc, Some(&original), &first(Some(&PNG))).unwrap();
    let moved_plain = write_with(&doc, Some(&original), &WriteOptions::default()).unwrap();
    assert!(moved.edited);
    assert_eq!(moved.bytes, moved_plain.bytes);
}

/// Counted cost (the plan's limits rule): a first save of a package of many entries copies each entry once, the thumbnail once and the manifest once, so the
/// bytes copied grow by the same amount for each entry added: four times the entries added, four times the growth.
#[test]
fn odg_package_the_cost_of_a_first_save_follows_the_package() {
    let sizes = [50usize, 200, 800];
    let mut copied = Vec::new();
    for n in sizes {
        let mut parts = parts();
        for k in 0..n {
            parts.push((format!("Pictures/p{k}.png"), PICTURE.to_vec()));
        }
        let package = write(Some(DRAWING), &with_manifest(DRAWING, parts));
        let saved = save(&package, &first(Some(&PNG))).unwrap();
        assert_eq!(rows(&saved.bytes).len(), n + 5 + 1, "{n}: a row for each entry, the root's and the thumbnail's");
        assert_eq!(saved.work.edits, 1, "{n}: the manifest's one new row is the one edit, and no part is edited");
        copied.push(saved.work.bytes_copied);
    }
    // 150 entries more, then 600 more: the growth is four times as much (the digits of a name give a few bytes more to a row).
    let (first_growth, second_growth) = (copied[1] - copied[0], copied[2] - copied[1]);
    assert!(second_growth * 10 >= first_growth * 35 && second_growth * 10 <= first_growth * 45, "{copied:?}");
    // And the whole is no more than the package and what one more entry costs: the thumbnail is copied once.
    assert!(copied[0] > 0 && copied[2] < copied[0] * 25, "{copied:?}");
}
