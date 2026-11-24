//! Item 2: an `.odg` drawing whose `meta.xml` and `settings.xml` the model holds as bytes (a `meta.xml` that is not of the shape the fields hold,
//! a `settings.xml` past the cap of a small part) goes to VSDX through `.diacraft` and back. The VSDX carrier refused both names, as entries the `.odg` package makes itself, so the
//! save failed; they are the names the `.odg` model writer writes such bytes under, in place of the part it would make from fields, and they ride.

use std::sync::Arc;

use diacraft_doc::{Format, Part};
use diacraft_engine::Session;
use serde_json::json;

use crate::native_common::{Dir, opened};
use crate::odg_build::{DRAWING, Drawing, package, rect};
use crate::run;

/// A `meta.xml` the reader keeps as bytes: its `office:meta` holds an attribute of its own, which no field of the model could keep.
fn odd_meta() -> Vec<u8> {
    concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" ",
        "xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" office:version=\"1.3\">",
        "<office:meta meta:own=\"kept\"><meta:generator>TestWriter/1</meta:generator></office:meta></office:document-meta>"
    )
    .as_bytes()
    .to_vec()
}

/// A `settings.xml` past the cap of a small part (`diacraft_odg::limits::MAX_SMALL_PART_BYTES`), which the reader keeps as bytes: a comment fills it.
fn big_settings() -> Vec<u8> {
    let filler = "x".repeat(usize::try_from(diacraft_odg::limits::MAX_SMALL_PART_BYTES).unwrap() + 16);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         office:version=\"1.3\"><!--{filler}--><office:settings/></office:document-settings>"
    )
    .into_bytes()
}

/// The parts named `meta.xml` and `settings.xml` a session's model holds, in that order.
fn held(s: &Session) -> Vec<Arc<Part>> {
    let doc = s.document().unwrap();
    ["meta.xml", "settings.xml"]
        .iter()
        .map(|name| doc.parts.iter().find(|p| p.format == Format::Odf && &*p.name == *name).unwrap_or_else(|| panic!("no part {name}")).clone())
        .collect()
}

#[test]
fn gatef1_a_meta_and_settings_part_held_as_bytes_go_odg_to_diacraft_to_vsdx_to_diacraft_and_come_back_equal() {
    let dir = Dir::new();
    let drawing = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello"));
    let files = vec![
        ("content.xml".to_owned(), drawing.content_xml().into_bytes()),
        ("styles.xml".to_owned(), drawing.styles_xml().into_bytes()),
        ("meta.xml".to_owned(), odd_meta()),
        ("settings.xml".to_owned(), big_settings()),
    ];
    let source = dir.write("a.odg", &package(DRAWING, files));
    let mut s = opened(&source);
    let first = held(&s);
    assert_eq!(&*first[0].bytes, odd_meta().as_slice(), "the reader holds the meta.xml it cannot read into fields as bytes");
    assert_eq!(&*first[1].bytes, big_settings().as_slice(), "and the settings.xml past the cap");
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    let mut native = opened(&dir.path("b.diacraft"));
    assert_eq!(held(&native), first, "the .diacraft holds them as the .odg did");
    // The save the gate found refused: a carried part named `meta.xml`.
    run(&mut native, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    let mut vsdx = opened(&dir.path("c.vsdx"));
    assert_eq!(held(&vsdx), first, "the VSDX file carries them and its read gives them back");
    run(&mut vsdx, "file.save_as", json!({ "path": dir.path("d.diacraft") }));
    let back = opened(&dir.path("d.diacraft"));
    assert_eq!(held(&back), first, "both come back equal");
    // The `.odg` the `.diacraft` is saved as holds them under their own names, the bytes as they were: the names the carrier now lets ride are the ones the model writer writes
    // such parts under. (The drawing that visited VSDX holds that format's package data too, which the `.odg` carrier is not yet written to hold.)
    let mut again = opened(&dir.path("b.diacraft"));
    run(&mut again, "file.save_as", json!({ "path": dir.path("e.odg") }));
    let written = dir.read("e.odg");
    assert_eq!(crate::odg_build::part_bytes(&written, "meta.xml").as_deref(), Some(odd_meta().as_slice()));
    assert_eq!(crate::odg_build::part_bytes(&written, "settings.xml").as_deref(), Some(big_settings().as_slice()));
}
