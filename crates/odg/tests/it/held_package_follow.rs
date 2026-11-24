//! The package data of a model follows the file a splice save writes.
//!
//! A model read from an `.odg` holds the file's package data **as it read it** (the roots, `mimetype`, the manifest's rows, `meta.xml`'s other fields, `settings.xml` and the
//! entries nothing reads). A splice save writes things of its own into the package: DiaCraft's `meta:generator`, the thumbnail and the rows that name what it adds. The model the
//! session keeps is not the file's reading of itself, so a `.diacraft` made from it after a save carried the old generator, the old thumbnail and the old rows. These tests save, and
//! ask that what a model writer is given (`PackageData`, the file's own package data put on the model) is what the file on disk holds, in the model's own words and in the codec's.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_odg::{Limits, PackageData, read, read_package_data};
use diacraft_sheetxml::model::{MemBlobs, encode};

use crate::drawing::{Drawing, NS};
use crate::write_core::{PNG, move_by, open_bytes, real};

/// The version a save writes as its generator: the crate's own.
const GENERATOR: &str = concat!("DiaCraft/", env!("CARGO_PKG_VERSION"));

/// A drawing with what LibreOffice puts in a package: a producer's generator, a title, dates, editing cycles, user-defined fields, a settings tree, a thumbnail of its own, a font
/// and a manifest that names them.
fn file() -> Vec<u8> {
    let mut d = Drawing::a4().page(r#"<draw:rect draw:name="R" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#);
    d.meta = Some(format!(
        "<?xml version=\"1.0\"?>\n<office:document-meta {NS} xmlns:x=\"urn:x\" office:version=\"1.3\"><office:meta>{}</office:meta></office:document-meta>",
        concat!(
            "<meta:generator>LibreOffice/7.3$Linux</meta:generator><dc:title>A title</dc:title><meta:creation-date>2024-05-06T07:08:09</meta:creation-date>",
            "<dc:date>2024-06-07T08:09:10</dc:date><meta:editing-cycles>7</meta:editing-cycles><meta:editing-duration>PT42S</meta:editing-duration>",
            "<meta:user-defined meta:name=\"Project\" meta:value-type=\"string\">Alpha</meta:user-defined>"
        )
    ));
    d.settings = Some(crate::drawing::settings(r#"<config:config-item config:name="ZoomValue" config:type="int">100</config:config-item>"#));
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), [&PNG[..8], b"OLD-THUMB"].concat()));
    d.files.push(("Fonts/f.ttf".to_owned(), b"font bytes".to_vec()));
    d.bytes()
}

/// The package data of a model, as words: what the model holds of the package, one line for each thing, so that two models are told apart by what they hold.
fn rows(doc: &Document) -> Vec<String> {
    let mut out = vec![format!("title {}", doc.title)];
    let p = &*doc.props;
    out.push(format!("props {:?} {:?} {:?} {:?}", p.creator, p.last_modified_by, p.created, p.modified));
    out.extend(p.custom.iter().map(|c| format!("custom {} {} {}", c.name, c.value_type, c.value)));
    for part in doc.parts.iter() {
        out.push(format!(
            "part {} {} {:?} {:?} {}",
            part.name,
            part.media_type,
            part.role,
            part.manifest.iter().collect::<Vec<_>>(),
            part.bytes.len()
        ));
    }
    if let Some(data) = doc.ext.get() {
        let odf = &data.odf;
        out.push(format!("mimetype {:?}", odf.mimetype));
        out.extend(odf.roots.iter().map(|(n, a)| format!("root {n} {:?}", a.iter().collect::<Vec<_>>())));
        out.extend(odf.manifest_rows.iter().map(|(n, a)| format!("row {n} {:?}", a.iter().collect::<Vec<_>>())));
        out.push(format!("meta {:?}", odf.meta));
        out.push(format!("settings {:?}", odf.settings));
    }
    out
}

/// The model of a drawing that was read, edited and saved: the model as the session has it, and what the file on disk reads as, and the file.
fn saved() -> (Document, Document, Vec<u8>) {
    let o = open_bytes(file());
    let mut doc = o.doc.clone();
    let id = doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).expect("a shape");
    move_by(&mut doc, 0, id, 0.5, 0.0);
    let written = real(&doc, &o.bytes, &o.index).expect("saved");
    assert!(written.edited);
    let on_disk = read(&written.bytes, &Limits::default()).expect("the file reads").0;
    (doc, on_disk, written.bytes)
}

/// **Red first** (`the_model_that_was_saved_holds_the_package_data_of_the_file_written` asserted that `rows(model) == rows(file)` and failed: the thumbnail of 17 bytes against 12,
/// `LibreOffice/7.3` against `DiaCraft/0.1.0`): the model that was saved holds the package as it read it, and the file holds what the save wrote.
#[test]
fn what_a_save_writes_of_its_own_is_in_the_file_and_not_in_the_model() {
    let (model, on_disk, _) = saved();
    let (stale, fresh) = (rows(&model), rows(&on_disk));
    assert_ne!(stale, fresh, "the model's package data is stale after a save");
    let only_in = |a: &[String], b: &[String]| a.iter().filter(|l| !b.contains(l)).cloned().collect::<Vec<_>>();
    // The generator is the save's own: the file says DiaCraft's, the model LibreOffice's.
    assert!(only_in(&fresh, &stale).iter().any(|l| l.contains(GENERATOR)), "{:?}", only_in(&fresh, &stale));
    assert!(only_in(&stale, &fresh).iter().any(|l| l.contains("LibreOffice/7.3")));
    // The thumbnail is replaced: the file's bytes are the thumbnail the save was given.
    assert!(only_in(&fresh, &stale).iter().any(|l| l.starts_with("part Thumbnails/thumbnail.png") && l.ends_with(&format!(" {}", PNG.len()))));
}

/// The package data of the file the save wrote, put on the model, is what the file holds: the same lines, and the same elements of the codec.
#[test]
fn the_package_data_of_the_file_put_on_the_model_is_the_files_in_words_and_in_the_codec() {
    let (model, on_disk, bytes) = saved();
    let data = read_package_data(&bytes, &Limits::default()).expect("the package data of the file");
    assert_eq!(data, PackageData::of(&on_disk), "what the file says of its package is what a read of it says");
    let mut followed = model.clone();
    data.put(&mut followed);
    assert_eq!(rows(&followed), rows(&on_disk), "the package rows are the file's");
    // The drawing is the model's: its pages are as they were, whatever the file's package data says.
    assert!(followed.pages == model.pages && followed.styles == model.styles);
    // The codec writes the same document part, the rows of the package among it.
    let (mut a, mut b) = (MemBlobs::new(), MemBlobs::new());
    let (a, b) = (encode(&followed, &mut a).expect("encoded"), encode(&on_disk, &mut b).expect("encoded"));
    assert!(a.document == b.document, "the document part of the `.diacraft` is the file's");
    // A model that held no package data (a drawing no file made) gets the file's.
    let mut empty = Document::new_blank();
    data.put(&mut empty);
    assert_eq!(rows(&empty).len(), rows(&on_disk).len());
    // And a model that is given the package data of a file that has none holds no slot for it.
    let mut none = followed.clone();
    PackageData::of(&Document::new_blank()).put(&mut none);
    assert!(
        none.parts.is_empty() && none.ext.get().is_none_or(|d| d.odf.roots.is_empty() && d.odf.manifest_rows.is_empty() && d.odf.settings.is_none())
    );
}

/// What a save writes of `meta.xml` is the generator and nothing else: **the generator is the save's own and not the drawing's** (it says DiaCraft wrote the file), and the dates, the
/// editing cycles and the duration are the file's, as they were: a save does not renew them, in this task or in the spec's V15.
#[test]
fn a_save_rewrites_the_generator_of_meta_and_leaves_the_rest_of_it() {
    let (model, on_disk, bytes) = saved();
    let data = read_package_data(&bytes, &Limits::default()).expect("the package data");
    let mut followed = model.clone();
    data.put(&mut followed);
    let fields = |d: &Document| -> Vec<(u32, String)> {
        d.ext
            .get()
            .map(|e| {
                e.odf
                    .meta
                    .iter()
                    .map(|p| match &p.node {
                        diacraft_doc::RawNode::Element(el) => (p.after, format!("{} {:?}", el.name, el.children)),
                        other => (p.after, format!("{other:?}")),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (before, after) = (fields(&model), fields(&followed));
    assert_eq!(before.len(), after.len(), "the same fields, in the same places");
    // The editing cycles and the duration are fields that no property reads: they are kept, and are the same.
    assert!(after.iter().any(|(_, f)| f.contains("meta:editing-cycles") && f.contains("\"7\"")), "{after:?}");
    assert!(after.iter().any(|(_, f)| f.contains("meta:editing-duration") && f.contains("PT42S")), "{after:?}");
    let changed: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).collect();
    assert_eq!(changed.len(), 1, "only the generator differs: {changed:?}");
    assert!(changed[0].0.1.contains("meta:generator") && changed[0].0.1.contains("LibreOffice") && changed[0].1.1.contains(GENERATOR), "{changed:?}");
    // The properties the file's fields give are the drawing's own and are as they were: the title, the creation and modification dates, the user-defined field.
    assert_eq!(followed.props, model.props);
    assert_eq!(
        (followed.title.as_ref(), followed.props.created.as_deref(), followed.props.modified.as_deref()),
        ("A title", Some("2024-05-06T07:08:09"), Some("2024-06-07T08:09:10"))
    );
    assert_eq!(rows(&followed), rows(&on_disk));
}

/// A package the save adds an entry to and takes none from reads as it did: the parts it copied are the model's own allocations, held once, and only what the save wrote is new.
#[test]
fn an_entry_the_save_copied_is_held_once() {
    let (model, _, bytes) = saved();
    let data = read_package_data(&bytes, &Limits::default()).expect("the package data");
    let mut followed = model.clone();
    data.put(&mut followed);
    let bytes_of = |d: &Document, name: &str| d.parts.iter().find(|p| &*p.name == name).map(|p| Arc::clone(&p.bytes)).expect("the entry");
    assert!(
        Arc::ptr_eq(&bytes_of(&followed, "Fonts/f.ttf"), &bytes_of(&model, "Fonts/f.ttf")),
        "an entry that is the same bytes keeps its allocation"
    );
    assert!(
        !Arc::ptr_eq(&bytes_of(&followed, "Thumbnails/thumbnail.png"), &bytes_of(&model, "Thumbnails/thumbnail.png")),
        "the thumbnail the save wrote is another"
    );
}

/// An unedited save writes nothing of its own: the file is the original, and the package data of it is the model's.
#[test]
fn an_unedited_save_leaves_the_package_data_as_it_was() {
    let o = open_bytes(file());
    let written = real(&o.doc, &o.bytes, &o.index).expect("saved");
    assert!(!written.edited && written.bytes == o.bytes);
    let data = read_package_data(&written.bytes, &Limits::default()).expect("the package data");
    assert_eq!(data, PackageData::of(&o.doc));
    let mut followed = o.doc.clone();
    data.put(&mut followed);
    assert!(followed == o.doc, "putting the file's package data on the model it was read as changes nothing");
}
