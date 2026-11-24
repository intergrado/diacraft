//! The plan's tests of the carrier, in this format (CARRYVSDX's list, with the `.odg`'s own: the entries of DiaCraft's folder, the manifest, and the judge of the items). Each
//! file is the real writer's and each read-back the real reader's; a file another program changed is the written one changed by hand at the text of one part and packed again.

use std::sync::Arc;

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Document, Format, PartRole};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, SaveNote, WriteError, WriteOptions, read, write_with};

use crate::carry::{repack, written};
use crate::carry_model::{vsdx_data_of, vsdx_model, vsdx_part};

// ------------------------------------------------------------------------------------------------------------------------------ helpers

/// The package `bytes` made again with each entry given to `f` (name and bytes in, the new bytes out, or `None` to leave it out), packed again with right checksums.
fn rebuild(bytes: &[u8], mut f: impl FnMut(&str, Vec<u8>) -> Option<Vec<u8>>) -> Vec<u8> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    for entry in package.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        let data = package.part(&entry.name).unwrap().to_vec();
        let Some(data) = f(&entry.name, data) else { continue };
        let method = if entry.name == "mimetype" { Method::Stored } else { Method::Deflated };
        writer.add(&entry.name, &data, method).unwrap();
    }
    writer.finish().unwrap()
}

/// The text of the entry `name`.
fn text_of(bytes: &[u8], name: &str) -> String {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap_or_else(|| panic!("no entry {name}")).to_vec()).unwrap()
}

/// The names of the entries of DiaCraft's own folder, but the document file.
fn carried_entries(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &Limits::default())
        .unwrap()
        .entries()
        .map(|e| e.name.clone())
        .filter(|n| n.starts_with("DiaCraft/") && n != "DiaCraft/document.xml")
        .collect()
}

/// The package with every `diacraft:` attribute taken off `content.xml` and DiaCraft's folder and its manifest rows taken out: what a program that knows nothing of DiaCraft's data is left with.
fn without_the_data(bytes: &[u8]) -> Vec<u8> {
    rebuild(bytes, |name, data| {
        if name.starts_with("DiaCraft/") {
            return None;
        }
        let mut text = String::from_utf8_lossy(&data).into_owned();
        match name {
            "content.xml" => {
                while let Some(at) = text.find(" diacraft:") {
                    let open = text[at..].find('"').unwrap() + at;
                    let close = text[open + 1..].find('"').unwrap() + open + 1;
                    text.replace_range(at..=close, "");
                }
            }
            "META-INF/manifest.xml" => {
                text = text.lines().filter(|l| !l.contains("DiaCraft/")).collect::<Vec<_>>().join("\n");
            }
            _ => return Some(data),
        }
        Some(text.into_bytes())
    })
}

fn doc_with_parts(parts: Vec<Arc<diacraft_doc::Part>>) -> Document {
    let mut doc = crate::model_write::blank(1);
    for part in parts {
        doc.parts.push(part);
    }
    doc
}

/// The warnings of the carrier's own: a stale item, an item or a record left out, a part refused (a warning about the cells of a group's entry is the document file's older ones).
fn carrier_warnings(report: &diacraft_odg::ReadReport) -> u64 {
    report
        .warning_counts
        .iter()
        .filter(|(class, _)| class.starts_with("diacraft_item") || class.starts_with("diacraft_part") || **class == "diacraft_stale")
        .map(|(_, n)| *n)
        .sum()
}

// ------------------------------------------------------------------------------------------------------------------------------ the entries

#[test]
fn the_manifest_lists_every_entry_and_the_folder_with_a_media_type() {
    let bytes = written(&vsdx_model());
    let package = Package::read(&bytes, &Limits::default()).unwrap();
    let entries = carried_entries(&bytes);
    assert!(entries.len() >= 3, "the macro project, the embedding and the custom XML: {entries:?}");
    for name in entries.iter().chain(&["DiaCraft/document.xml".to_owned()]) {
        let row = package.manifest().entry(name).unwrap_or_else(|| panic!("{name} has no row in the manifest"));
        assert!(row.media_type.as_deref().is_some_and(|m| !m.is_empty()), "{name}: {row:?}");
    }
    let folder = package.manifest().entry("DiaCraft/").expect("a row for the folder DiaCraft/");
    assert_eq!(folder.media_type.as_deref(), Some("application/vnd.diacraft.parts"), "{folder:?}");
    assert!(package.entry("DiaCraft/").is_none(), "no ZIP entry is written for the folder");
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(report.carry.left_out, 0, "{:?}", report.warnings);
    assert!(
        back.parts.iter().all(|p| p.format != Format::Odf || !p.name.starts_with("DiaCraft/")),
        "DiaCraft's entries are no part of the OpenDocument package"
    );
}

#[test]
fn the_file_draws_the_same_without_the_entries() {
    // The file with all of DiaCraft's data taken out says the same markup as the file of the same drawing that carries none: nothing of the VSDX data is a cell, a style attribute or a place that
    // the file's own markup says.
    let with = written(&vsdx_model());
    let stripped = without_the_data(&with);
    let (plain, report) = read(&stripped, &Limits::default()).unwrap();
    assert_eq!(report.carry.items, 0);
    let (full, _) = read(&with, &Limits::default()).unwrap();
    assert_eq!(plain.pages.get(0).unwrap().shapes.len(), full.pages.get(0).unwrap().shapes.len(), "the same shapes are drawn");
    assert_eq!(plain.pages.len(), full.pages.len());
    let mut bare = vsdx_model();
    {
        // The same model with every place the VSDX data is in made empty again: written, its markup is the file's.
        let page = Arc::make_mut(bare.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            shape.raw = diacraft_doc::RawAttrs::new();
            shape.passthrough = diacraft_doc::Passthrough::new();
            shape.data1 = None;
            shape.data2 = None;
            shape.data3 = None;
            shape.ext = Default::default();
        }
    }
    let again = without_the_data(&written(&bare));
    for part in ["styles.xml", "meta.xml", "settings.xml"] {
        // `meta.xml` holds the VSDX properties as ODF ones, as it always did: only the markup of the drawing is the same.
        if part == "meta.xml" {
            continue;
        }
        assert_eq!(text_of(&stripped, part), text_of(&again, part), "{part}");
    }
}

#[test]
fn a_vba_project_goes_only_where_the_design_says() {
    // An `.odg` has no macro-free type, so the design's Owner 9 applies as it stands: the project rides as bytes, with its signature and data parts, whatever the language, and is never run.
    let model = vsdx_model();
    let bytes = written(&model);
    let (back, _) = read(&bytes, &Limits::default()).unwrap();
    let project = back.parts.iter().find(|p| p.role == PartRole::MacroProject).expect("the macro project comes back");
    assert_eq!((project.format, &*project.name, &project.bytes[..]), (Format::Vsdx, "/vbaProject.bin", &b"MACRO PROJECT BYTES"[..]));
    assert!(!project.rels.is_empty(), "its relationships come back");
    // The caller's choice to leave it out: the rest rides, and the save says what it left out.
    let dropped = write_with(&model, None, &WriteOptions { drop_macros: true, ..WriteOptions::default() }).unwrap();
    let (back, _) = read(&dropped.bytes, &Limits::default()).unwrap();
    assert!(back.parts.iter().all(|p| p.role != PartRole::MacroProject), "a project the caller left out is not in the file");
    assert!(back.parts.iter().any(|p| &*p.name == "/visio/embeddings/oleObject1.bin"), "the rest of the data rides");
    assert!(dropped.notes.iter().any(|n| matches!(n, SaveNote::MacroProjectLeftOut { parts: 1 })), "{:?}", dropped.notes);
    // What rides says so: the structured note (the design's 5.6) has the macro project among its classes.
    let said = written_notes(&model);
    assert!(said.iter().any(|n| matches!(n, SaveNote::Carried { macros: true, objects: true, document_level: true, .. })), "{said:?}");
}

fn written_notes(doc: &Document) -> Vec<SaveNote> {
    write_with(doc, None, &WriteOptions::default()).unwrap().notes
}

#[test]
fn a_macro_part_is_never_parsed() {
    // A part of a macro project holds bytes that look like XML: the read counts the XML events it makes, and a part of the same size that is not XML makes as many.
    let looks_like_xml = format!("<?xml version=\"1.0\"?><root>{}</root>", "<a b=\"c\">text</a>".repeat(60_000));
    let plain = "x".repeat(looks_like_xml.len());
    let events = |bytes: &[u8]| {
        let doc =
            doc_with_parts(vec![vsdx_part("/vbaProject.bin", "application/vnd.ms-office.vbaProject", PartRole::MacroProject, Vec::new(), bytes)]);
        let (_, report) = read(&written(&doc), &Limits::default()).unwrap();
        (report.work.package.xml_events, report.work.package.scan_events)
    };
    assert_eq!(events(looks_like_xml.as_bytes()), events(plain.as_bytes()), "the bytes of a macro project are never parsed");
}

#[test]
fn a_carried_entry_one_byte_over_the_cap_is_refused_in_words() {
    let cap = usize::try_from(diacraft_container::limits::MAX_PART_BYTES).unwrap();
    let doc = doc_with_parts(vec![vsdx_part(
        "/visio/embeddings/big.bin",
        "application/octet-stream",
        PartRole::Embedding,
        Vec::new(),
        &vec![0u8; cap + 1],
    )]);
    let refused = write_with(&doc, None, &WriteOptions::default()).unwrap_err();
    assert!(matches!(&refused, WriteError::Carried { part, .. } if part == "/visio/embeddings/big.bin"), "{refused:?}");
    let words = refused.to_string();
    assert!(words.contains("/visio/embeddings/big.bin") && words.contains("256 MiB"), "{words}");
}

#[test]
fn a_crafted_vsdx_part_name_never_reaches_outside_a_vsdx() {
    // The writer refuses a name no VSDX package could hold, in words that name it.
    for bad in [
        "/visio/../x.bin",
        "/a/./b.bin",
        "visio/x.bin",
        "C:/x.bin",
        "//x.bin",
        "/x.bin/",
        "/x\\y.bin",
        "/a/b.",
        "/",
        "",
        "/[Content_Types].xml",
        "/x\u{1}.bin",
    ] {
        let doc = doc_with_parts(vec![vsdx_part(bad, "application/octet-stream", PartRole::Other, Vec::new(), b"bytes")]);
        let refused = write_with(&doc, None, &WriteOptions::default()).unwrap_err();
        assert!(matches!(refused, WriteError::Carried { .. }), "{bad:?}: {refused:?}");
    }
    // A file another program (or a person) made whose item names such a part: the reader leaves it out with a note, and no part of the model has the name.
    let good = doc_with_parts(vec![
        vsdx_part("/visio/a.bin", "application/octet-stream", PartRole::Other, Vec::new(), b"first"),
        vsdx_part("/visio/b.bin", "application/octet-stream", PartRole::Other, Vec::new(), b"second"),
    ]);
    let bytes = written(&good);
    for bad in ["/visio/../outside.bin", "/..", "visio/relative.bin", "C:/drive.bin", "/visio//empty.bin", "/visio/x\\y.bin"] {
        let edited = repack(&bytes, "DiaCraft/document.xml", |t| t.replacen("name=\"/visio/a.bin\"", &format!("name=\"{bad}\""), 1));
        let (back, report) = read(&edited, &Limits::default()).unwrap();
        assert_eq!(report.warned("diacraft_part_name_refused"), 1, "{bad:?}: {:?}", report.warnings);
        assert!(back.parts.iter().all(|p| !p.name.contains("..") && &*p.name != bad), "{bad:?}");
        assert!(back.parts.iter().any(|p| &*p.name == "/visio/b.bin"), "the part that is fine is read");
    }
    // Names that differ only in case are one name: the second is left out.
    let twice = doc_with_parts(vec![
        vsdx_part("/visio/Same.bin", "application/octet-stream", PartRole::Other, Vec::new(), b"one"),
        vsdx_part("/visio/same.BIN", "application/octet-stream", PartRole::Other, Vec::new(), b"two"),
    ]);
    let (back, _) = read(&written(&twice), &Limits::default()).unwrap();
    assert_eq!(
        back.parts.iter().filter(|p| p.format == Format::Vsdx).count(),
        1,
        "{:?}",
        back.parts.iter().map(|p| p.name.to_string()).collect::<Vec<_>>()
    );
    let edited = repack(&written(&good), "DiaCraft/document.xml", |t| t.replacen("name=\"/visio/b.bin\"", "name=\"/VISIO/A.BIN\"", 1));
    let (back, report) = read(&edited, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_part_name_refused"), 1, "{:?}", report.warnings);
    assert_eq!(back.parts.iter().filter(|p| p.format == Format::Vsdx).count(), 1);
}

#[test]
fn an_item_naming_a_missing_entry_is_dropped_with_a_note() {
    let bytes = written(&vsdx_model());
    let entries = carried_entries(&bytes);
    let first = entries.first().expect("an entry").clone();
    let without = rebuild(&bytes, |name, data| (name != first).then_some(data));
    let (back, report) = read(&without, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_item_entry"), 1, "{:?}", report.warnings);
    assert!(report.carry.left_out >= 1);
    let total = vsdx_model().parts.iter().filter(|p| p.role != PartRole::Thumbnail && p.format == Format::Vsdx).count();
    assert!(back.parts.iter().filter(|p| p.format == Format::Vsdx).count() < total, "the part whose entry is gone is left out");
    assert!(back.parts.iter().any(|p| p.format == Format::Vsdx), "the others are read");
}

#[test]
fn two_items_naming_one_entry_are_dropped_with_a_note() {
    let bytes = written(&vsdx_model());
    let entries = carried_entries(&bytes);
    assert!(entries.len() >= 2);
    let edited = repack(&bytes, "DiaCraft/document.xml", |t| t.replacen(&format!("path=\"{}\"", entries[1]), &format!("path=\"{}\"", entries[0]), 1));
    let (_, report) = read(&edited, &Limits::default()).unwrap();
    assert!(report.warned("diacraft_item_entry") >= 1, "{:?}", report.warnings);
    assert!(report.carry.left_out >= 1);
}

#[test]
fn a_stale_item_is_dropped_when_its_native_output_changed() {
    let bytes = written(&vsdx_model());
    // Another program renames the first shape: the item of a shape sits beside its name and its outline, and the rest of the drawing's items are as they were.
    let renamed = repack(&bytes, "content.xml", |t| t.replacen("draw:name=\"Box\"", "draw:name=\"Renamed\"", 1));
    let (back, report) = read(&renamed, &Limits::default()).unwrap();
    let data = vsdx_data_of(&back);
    assert!(
        data.iter().all(|d| !d.starts_with("page 1 shape 1 raw") && !d.starts_with("page 1 shape 1 data1")),
        "the stale shape's data is dropped: {data:#?}"
    );
    assert!(data.iter().any(|d| d.starts_with("page 1 shape 2 raw")), "the shapes that were not changed keep theirs");
    assert!(data.iter().any(|d| d.starts_with("page 1 raw")), "and so does the page");
    assert_eq!(report.warned("diacraft_stale"), 1, "{:?}", report.warnings);
    // The document's item sits beside its title: another program retitles the drawing.
    let retitled = repack(&bytes, "meta.xml", |t| t.replacen("<dc:title>Untitled</dc:title>", "<dc:title>Another</dc:title>", 1));
    let (back, report) = read(&retitled, &Limits::default()).unwrap();
    assert_eq!(report.carry.stale, 1, "{:?}", report.carry);
    assert!(vsdx_data_of(&back).iter().all(|d| !d.starts_with("props category")), "the document's data is dropped");
    assert!(vsdx_data_of(&back).iter().any(|d| d.starts_with("part ")), "the parts, which sit beside nothing, are kept");
    // A group's item sits beside the names of its members.
    let regrouped = repack(&bytes, "content.xml", |t| t.replacen("draw:name=\"MemberA\"", "draw:name=\"MemberZ\"", 1));
    let (back, report) = read(&regrouped, &Limits::default()).unwrap();
    assert_eq!(report.carry.stale, 1, "{:?}", report.carry);
    assert!(vsdx_data_of(&back).iter().all(|d| !d.starts_with("page 1 shape 7 raw")), "the group's data is dropped");
}

#[test]
fn an_item_for_a_gone_shape_is_dropped_silently() {
    let bytes = written(&vsdx_model());
    // Another program took the first shape out: the data on its own style goes with it, and no word is said.
    let edited = repack(&bytes, "content.xml", |t| {
        let start = t.find("<draw:custom-shape").unwrap();
        let end = start + t[start..].find("</draw:custom-shape>").unwrap() + "</draw:custom-shape>".len();
        format!("{}{}", &t[..start], &t[end..])
    });
    let (back, report) = read(&edited, &Limits::default()).unwrap();
    assert!(back.pages.get(0).unwrap().shapes.iter().all(|s| s.name_u.as_deref() != Some("Box")), "the shape is gone");
    assert_eq!(carrier_warnings(&report), 0, "{:?}", report.warnings);
    // The group is gone: its item names nothing, and the file says nothing of it.
    let no_group = repack(&bytes, "content.xml", |t| {
        let start = t.find("<draw:g ").unwrap();
        let end = start + t[start..].find("</draw:g>").unwrap() + "</draw:g>".len();
        format!("{}{}", &t[..start], &t[end..])
    });
    let (back, report) = read(&no_group, &Limits::default()).unwrap();
    assert!(back.pages.get(0).unwrap().shapes.iter().all(|s| s.name_u.as_deref() != Some("Group")), "the group is gone");
    assert_eq!(carrier_warnings(&report), 0, "{:?}", report.warnings);
    assert!(report.carry.gone >= 1, "{:?}", report.carry);
}

/// The files the LibreOffice check loads (run by hand with `CARRYODG_LO_DIR` naming a folder of the run, never in a gate): the drawing with all its data, the same with the manifest's row of the
/// folder taken out (what the design assumed before the probe), and the same with one entry listed nowhere (a writer's bug LibreOffice asks to repair).
#[test]
#[ignore = "writes the files the LibreOffice check loads: set CARRYODG_LO_DIR"]
fn lo_files() {
    let Ok(dir) = std::env::var("CARRYODG_LO_DIR") else { return };
    let with = written(&vsdx_model());
    std::fs::write(format!("{dir}/with.odg"), &with).unwrap();
    let no_folder_row = rebuild(&with, |name, data| {
        if name == "META-INF/manifest.xml" {
            let text = String::from_utf8(data).unwrap();
            return Some(text.lines().filter(|l| !l.contains("full-path=\"DiaCraft/\"")).collect::<Vec<_>>().join("\n").into_bytes());
        }
        Some(data)
    });
    std::fs::write(format!("{dir}/no-folder-row.odg"), &no_folder_row).unwrap();
    let first = carried_entries(&with).first().cloned().unwrap();
    let unlisted = rebuild(&with, |name, data| {
        if name == "META-INF/manifest.xml" {
            let text = String::from_utf8(data).unwrap();
            return Some(text.lines().filter(|l| !l.contains(&format!("full-path=\"{first}\""))).collect::<Vec<_>>().join("\n").into_bytes());
        }
        Some(data)
    });
    std::fs::write(format!("{dir}/unlisted.odg"), &unlisted).unwrap();
    // The same drawing with no data at all: the plain file.
    std::fs::write(format!("{dir}/plain.odg"), written(&crate::model_write::blank(2))).unwrap();
}

// ------------------------------------------------------------------------------------------------------------------------------ a file that carries data

/// A save into the file the data was read from, with no edit, is the file: **a second save is no edit**. An edit leaves the data where it was.
#[test]
fn an_unedited_save_of_a_file_that_carries_data_is_no_edit_and_an_edit_keeps_the_data() {
    let bytes = written(&vsdx_model());
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    let before = vsdx_data_of(&doc);
    let save = |doc: &Document, bytes: &[u8], index: &diacraft_odg::ShapeIndex| {
        write_with(doc, Some(&diacraft_odg::OriginalPackage { bytes, index }), &WriteOptions::default())
            .unwrap_or_else(|e| panic!("the save was refused: {e} ({})", e.class()))
    };
    let same = save(&doc, &bytes, &report.index);
    assert!(!same.edited && same.bytes == bytes, "an unedited save of a file that carries data is the file");
    // An edit of one shape (a move) leaves the data where it was.
    let mut moved = doc.clone();
    {
        let page = Arc::make_mut(moved.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let cell = sheet.cells.get_mut("PinX").unwrap();
        cell.value = Some(diacraft_doc::Value::from_number(4.5).unwrap());
    }
    let edited = save(&moved, &bytes, &report.index);
    assert!(edited.edited);
    let (back, again) = read(&edited.bytes, &Limits::default()).unwrap();
    assert_eq!(carrier_warnings(&again), 0, "{:?}", again.warnings);
    assert_eq!(vsdx_data_of(&back), before, "the data of the drawing is where it was");
    // And that save, saved again, is no edit.
    let twice = save(&back, &edited.bytes, &again.index);
    assert!(!twice.edited && twice.bytes == edited.bytes);
}

/// The concern 5: the attributes of a VSDX object's foreign data that rode with the object frame come back in `ForeignData::attrs`, in the order the file had them, beyond its type, and the
/// relationship with them: the model a VSDX writer is given has what its writer reads (`ForeignType`, then the attributes that rode).
#[test]
fn the_attributes_of_a_vsdx_objects_foreign_data_come_back_in_the_foreign_data() {
    let mut doc = crate::model_ole::drawing(crate::model_ole::compound(crate::model_ole::CLASS), crate::write_core::PNG.to_vec(), true);
    let attrs =
        [("ForeignType", "Object"), ("ObjectType", "33280"), ("ShowAsIcon", "0"), ("ObjectWidth", "2"), ("ObjectHeight", "1"), ("MappingMode", "8")];
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let mut foreign = (**shape.foreign.as_ref().unwrap()).clone();
        foreign.attrs = diacraft_doc::RawAttrs::from_pairs(attrs);
        shape.foreign = Some(Arc::new(foreign));
    }
    let bytes = written(&doc);
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(carrier_warnings(&report), 0, "{:?}", report.warnings);
    let shape = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    let foreign = shape.foreign.as_ref().expect("the object's foreign data");
    let got: Vec<(&str, &str)> = foreign.attrs.iter().collect();
    assert_eq!(got, attrs, "every attribute, in order, `ForeignType` first");
    assert_eq!(foreign.rel_id.as_deref(), Some("rId1"));
    assert!(foreign.picture.is_some(), "the replacement picture is the markup's");
}

/// For a shape written into a file: its text is written as plain characters, the formatting is DiaCraft's data, and the stamp covers that formatting too (the plain text's): a formatting
/// edit made in another program changes the markup and not the characters, and the file wins.
#[test]
fn a_formatting_edit_of_a_text_put_into_a_file_wins_over_what_was_carried() {
    use crate::drawing::Drawing;
    use crate::model_text::{chars, row, section};
    use crate::write_core::{custom, open, real};
    use diacraft_doc::{Text, TextPart};
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = crate::write_new::rectangle(id, "Typed", (2.0, 8.0, 3.0, 1.0));
    shape.text =
        Some(Arc::new(Text { parts: vec![chars("plain "), TextPart::Cp(1), chars("it"), TextPart::Cp(0), chars(" end")], ..Text::default() }));
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section("Character", vec![row(1, &[("Style", 2.0)])])));
    crate::write_new::push_shape(&mut o.doc, shape);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let xml = text_of(&saved.bytes, "content.xml");
    assert!(xml.contains("diacraft:fp-format="), "the carried text is stamped with its formatting: {xml}");
    // Unedited, the carried text is the model's (italic).
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 0, "{:?}", report.warnings);
    let typed = |d: &Document| d.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("Typed")).cloned().unwrap();
    let style_of_it = |shape: &diacraft_doc::Shape| {
        let text = shape.text.as_ref().unwrap();
        let mut row_ix = None;
        for part in &text.parts {
            match part {
                TextPart::Cp(ix) => row_ix = Some(*ix),
                TextPart::Chars(c) if c.contains("it") && row_ix.is_some() => {
                    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character")?;
                    let row = section.rows.iter().find(|r| r.ix == row_ix)?;
                    return row.cells.get("Style")?.value.as_ref()?.number();
                }
                _ => {}
            }
        }
        None
    };
    assert_eq!(style_of_it(&typed(&back)), Some(2.0));
    // Another program makes the run bold in the markup: the whole text becomes a span of a new bold style, and the characters are the same.
    let edited = repack(&saved.bytes, "content.xml", |t| {
        let t = t.replacen(
            "<office:automatic-styles>",
            "<office:automatic-styles><style:style style:name=\"TB\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>",
            1,
        );
        t.replacen("<text:p>plain it end</text:p>", "<text:p><text:span text:style-name=\"TB\">plain it end</text:span></text:p>", 1)
    });
    let (back, report) = read(&edited, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 1, "{:?}", report.warnings);
    assert_eq!(style_of_it(&typed(&back)), Some(1.0), "the file's bold wins over the carried italic");
    // A file an older DiaCraft wrote stamps the characters only (`fp-text`, no `fp-format`): there the carried formatting is kept as it was kept before the ruling, and the characters still judge the item.
    let legacy = repack(&saved.bytes, "content.xml", |t| {
        let at = t.find(" diacraft:fp-format=\"").expect("the stamp");
        let end = at + 1 + t[at + 1..].find('"').unwrap() + 1;
        let end = end + t[end..].find('"').unwrap() + 1;
        format!("{}{}", &t[..at], &t[end..])
    });
    assert!(!text_of(&legacy, "content.xml").contains("fp-format"));
    let (back, report) = read(&legacy, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 0, "{:?}", report.warnings);
    assert_eq!(style_of_it(&typed(&back)), Some(2.0), "a legacy stamp covers the characters, and the item's formatting stands");
}

/// Two pages of carried text, each stamped with the formatting its markup says: a formatting edit on the second page drops that page's text item and no other (the second pass of the model writer
/// finds each shape by its page's place and its `ID`).
#[test]
fn the_formatting_of_each_pages_carried_text_is_stamped_by_its_page() {
    use crate::model_text::{chars, row, section};
    use diacraft_doc::{Text, TextPart};
    let mut doc = crate::model_write::blank(0);
    doc.add_page(Some("Second")).unwrap();
    for at in 0..2 {
        let mut shape = crate::write_new::rectangle(1, &format!("Typed{at}"), (2.0, 8.0, 3.0, 1.0));
        shape.text = Some(Arc::new(Text { parts: vec![TextPart::Cp(1), chars("italic"), TextPart::Cp(0), chars(" end")], ..Text::default() }));
        // The rows differ, so the texts name styles of their own (equal rows share one).
        let cells: &[(&str, f64)] = if at == 0 { &[("Style", 2.0)] } else { &[("Size", 14.0 / 72.0)] };
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section("Character", vec![row(1, cells)])));
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        page.shapes.push(Arc::new(shape));
    }
    let bytes = written(&doc);
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 0, "{:?}", report.warnings);
    assert_eq!(back.pages.len(), 2);
    // The second page's text is set in a larger size in the markup.
    let edited = repack(&bytes, "content.xml", |t| {
        assert!(t.contains("fo:font-size=\"14pt\""), "{t}");
        t.replacen("fo:font-size=\"14pt\"", "fo:font-size=\"16pt\"", 1)
    });
    let (_, report) = read(&edited, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 1, "only the page whose markup changed: {:?}", report.warnings);
}

/// The note of a save that carried data says what LibreOffice does with it, as measured (26.8.2.2): it keeps what DiaCraft put on shapes, and DiaCraft's folder with the document file
/// because the manifest lists the folder with a media type; **the plan expected it to drop the document data, and it does not** (a program that does not list the folder does). The words say this and no more.
#[test]
fn the_note_says_what_libreoffice_keeps_of_the_data() {
    let saved = write_with(&vsdx_model(), None, &WriteOptions::default()).unwrap();
    let note = saved.notes.iter().find(|n| matches!(n, SaveNote::Carried { .. })).expect("a note of what was carried").to_string();
    assert!(note.contains("keeps it inside the file and reads it back"), "{note}");
    assert!(note.contains("another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell"), "{note}");
    assert!(note.contains("DiaCraft reads each piece of its own folder of document data back only while it still matches the drawing"), "{note}");
    assert!(note.contains("never run or opened"), "{note}");
    // A drawing whose data is all on shapes does not speak of the folder, and says nothing of the macro project it does not hold.
    let only_shapes = SaveNote::Carried { classes: vec!["raw cells"], document_level: false, macros: false, objects: false }.to_string();
    assert!(only_shapes.contains("another program that saves the file may keep this data or leave it out"), "{only_shapes}");
    assert!(!only_shapes.contains("folder") && !only_shapes.contains("never run"), "{only_shapes}");
    assert_eq!(SaveNote::Carried { classes: Vec::new(), document_level: false, macros: false, objects: false }.class(), "carried");
}
