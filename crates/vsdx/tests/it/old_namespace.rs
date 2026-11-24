//! APPID: a VSDX file that a build made before the id moved to `diacraft.app` wrote holds DiaCraft's data in `urn:io.github.one_bit.diacraft:extension`, and its relationships to the parts
//! of DiaCraft's own are of the type `urn:io.github.one_bit.diacraft:extension:blob`. It opens as it did, a save that writes the data writes the current namespace and the current
//! type and adds no second `Data` beside the one that is there, and the content check reads the two spellings as one. The two files are the ones `main` at 3f696318 wrote
//! (`diacraft_testkit::old_namespace`), not files this code built by hand; the others are those files' parts with the namespace put in the current form, or hand-made.

use std::collections::BTreeMap;

use diacraft_doc::{DIACRAFT_URN, Document};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::old_namespace::{DRAWING_VSDX, ODG_AS_VSDX, OLD_NAMESPACE, file, parts as fixture_parts};
use diacraft_testkit::package::{DIR, PageSpec, VsdxBuilder, ZipItem, cell, shape, zip};
use diacraft_vsdx::{DiacraftPart, Limits, OriginalBytes, WriteOptions, Written, compare_content, write_with};

use crate::read_ok;

/// The solution part of the DiaCraft data in a package, and the part of its relationships.
fn solution() -> String {
    format!("{DIR}/solutions/solution1.xml")
}

fn solution_rels() -> String {
    format!("{DIR}/solutions/_rels/solution1.xml.rels")
}

fn options() -> WriteOptions {
    WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn text(p: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(p.get(name).unwrap_or_else(|| panic!("no part {name}: {:?}", p.keys())).clone()).unwrap()
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options()).unwrap_or_else(|e| panic!("{e}"))
}

/// The file `name` with the old namespace put in the current form in every part that holds it (the solution part, and the type of its relationships).
fn current(name: &str) -> Vec<u8> {
    let items: Vec<ZipItem> = fixture_parts(name)
        .into_iter()
        .map(|(entry, bytes)| match String::from_utf8(bytes) {
            Ok(text) => ZipItem::deflated(&entry, text.replace(OLD_NAMESPACE, DIACRAFT_URN).as_bytes()),
            Err(bytes) => ZipItem::deflated(&entry, bytes.as_bytes()),
        })
        .collect();
    zip(&items)
}

/// Every part of the package that names the old namespace.
fn parts_with_the_old_namespace(bytes: &[u8]) -> Vec<String> {
    parts(bytes).into_iter().filter(|(_, b)| String::from_utf8_lossy(b).contains("one_bit")).map(|(name, _)| name).collect()
}

#[test]
fn the_data_of_a_file_of_the_old_namespace_is_read_and_is_held_in_the_current_one() {
    let old = file(DRAWING_VSDX);
    assert_eq!(parts_with_the_old_namespace(&old), [solution()], "the file is one a build before the move wrote");
    let (doc, report) = read_ok(&old);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { ref version, .. }) if version == "1"), "{:?}", report.diacraft);
    let snap = doc.diacraft_child("snap").expect("the Snap and Glue options the file holds are read");
    assert_eq!(snap.attrs.get("strength_grid"), Some("40"));
    let data = doc.diacraft.as_ref().unwrap();
    assert_eq!(data.attrs.get("xmlns:dc"), Some(DIACRAFT_URN), "the model holds the data as this build writes it");
    // The same file with the namespace in the current form is the same model.
    let (new, new_report) = read_ok(&current(DRAWING_VSDX));
    assert!(doc == new, "the same drawing, whichever namespace the file says");
    assert_eq!(report.diacraft, new_report.diacraft);
    assert!(report.warnings.iter().all(|w| !w.contains("DiaCraft")), "{:?}", report.warnings);
}

#[test]
fn the_items_a_file_of_the_old_namespace_carries_and_the_relationships_of_the_old_type_are_read() {
    let old = file(ODG_AS_VSDX);
    let held = parts_with_the_old_namespace(&old);
    assert_eq!(held, [solution_rels(), solution()], "the solution part and the relationship of the old type: {held:?}");
    let (doc, report) = read_ok(&old);
    let (new, new_report) = read_ok(&current(ODG_AS_VSDX));
    assert!(report.carry.items > 0, "the file carries items of the other format: {:?}", report.carry);
    assert_eq!(report.carry, new_report.carry, "the items are judged alike in either namespace");
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    assert!(report.carry.parts_seen > 0, "the part the relationship leads to is read");
    assert!(doc == new, "the same drawing, whichever namespace the file says");
}

#[test]
fn a_save_that_writes_the_data_writes_the_current_namespace_the_current_type_and_one_data_element() {
    for name in [DRAWING_VSDX, ODG_AS_VSDX] {
        let original = file(name);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        assert!(doc.set_diacraft_child("snap", Some(&[("strength_grid", "41"), ("alignment_box", "0")])), "{name}");
        let written = save(&doc, &original, &baseline);
        let p = parts(&written.bytes);
        assert!(
            parts_with_the_old_namespace(&written.bytes).is_empty(),
            "{name}: the old namespace is written nowhere: {:?}",
            parts_with_the_old_namespace(&written.bytes)
        );
        let part = text(&p, &solution());
        assert!(part.contains(DIACRAFT_URN) && part.contains(r#"strength_grid="41""#), "{name}: {part}");
        assert_eq!(part.matches(":Data ").count(), 1, "{name}: one Data element: {part}");
        assert!(!p.contains_key(&format!("{DIR}/solutions/solution2.xml")), "{name}: no second part");
        let rels = p.get(&solution_rels()).map(|b| String::from_utf8(b.clone()).unwrap());
        if let Some(rels) = rels {
            assert!(
                rels.contains(&format!("{DIACRAFT_URN}:blob")),
                "{name}: the relationships to the parts of DiaCraft's own are of the current type: {rels}"
            );
        }
        // It reads back as the model that was saved: the items the file carried come back, once each.
        let (again, report) = read_ok(&written.bytes);
        assert_eq!(again.diacraft, doc.diacraft, "{name}");
        assert_eq!(again.parts.len(), doc.parts.len(), "{name}: no part of the other format twice");
        assert_eq!(report.carry.stale, 0, "{name}");
    }
}

#[test]
fn a_save_that_changes_nothing_of_the_data_leaves_the_part_as_the_file_had_it_and_the_file_still_reads() {
    let original = file(DRAWING_VSDX);
    let (doc, _) = read_ok(&original);
    let written = save(&doc, &original, &doc.clone());
    assert_eq!(parts(&written.bytes).get(&solution()), parts(&original).get(&solution()), "an unedited save writes the part it read");
    assert!(read_ok(&written.bytes).0 == doc);
}

#[test]
fn the_file_read_and_the_file_of_the_current_namespace_are_the_same_content_and_one_value_beside_it_is_still_found() {
    for name in [DRAWING_VSDX, ODG_AS_VSDX] {
        let old = file(name);
        let current = current(name);
        // The same either way round: the count does not depend on which is first.
        for (a, b) in [(&old, &current), (&current, &old)] {
            let diff = compare_content(a, b, &Limits::default()).unwrap();
            assert!(diff.same(), "{name}: {:?}", diff.shown);
        }
    }
    // Not vacuous: a value changed in the solution part, beside the namespace, is the one difference.
    let old = file(DRAWING_VSDX);
    let changed: Vec<ZipItem> = fixture_parts(DRAWING_VSDX)
        .into_iter()
        .map(|(entry, bytes)| {
            let text = String::from_utf8(bytes)
                .ok()
                .map(|t| if entry.ends_with("solution1.xml") { t.replacen("strength_grid=\"40\"", "strength_grid=\"41\"", 1) } else { t });
            ZipItem::deflated(&entry, text.unwrap_or_default().as_bytes())
        })
        .collect();
    let diff = compare_content(&old, &zip(&changed), &Limits::default()).unwrap();
    assert!(!diff.same() && diff.count == 1, "the one value that changed is the one difference: {:?}", diff.shown);
}

#[test]
fn a_file_written_from_the_model_of_a_file_of_the_old_namespace_names_the_old_one_nowhere() {
    for name in [DRAWING_VSDX, ODG_AS_VSDX] {
        let (doc, _) = read_ok(&file(name));
        let model = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
        assert!(parts_with_the_old_namespace(&model.bytes).is_empty(), "{name}: {:?}", parts_with_the_old_namespace(&model.bytes));
        let (again, report) = read_ok(&model.bytes);
        assert_eq!(again.diacraft, doc.diacraft, "{name}");
        assert_eq!(report.carry.stale, 0, "{name}");
    }
}

#[test]
fn a_part_that_holds_both_namespaces_reads_the_first_data_and_a_save_adds_none() {
    let both = format!(
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="{OLD_NAMESPACE}" dc:version="1"><dc:snap alignment_box="1"/></dc:Data><n:Data xmlns:n="{DIACRAFT_URN}" n:version="1"><n:snap alignment_box="0"/></n:Data></SolutionXML>"#
    );
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).solution("DiaCraft", &both).build();
    let (mut doc, report) = read_ok(&original);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { .. })), "{:?}", report.diacraft);
    assert_eq!(doc.diacraft_child("snap").unwrap().attrs.get("alignment_box"), Some("1"), "the first data element is the data");
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "0"), ("guides", "0")])));
    let written = save(&doc, &original, &baseline);
    let part = text(&parts(&written.bytes), &solution());
    assert_eq!(part.matches("Data xmlns").count(), 2, "the data that was there is replaced, one for each that was: none added, none taken: {part}");
    assert!(part.contains(r#"<dc:snap alignment_box="0" guides="0"/>"#) || part.contains(r#"snap alignment_box="0" guides="0""#), "{part}");
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
}

#[test]
fn a_file_that_has_no_data_of_either_namespace_gets_the_current_one_when_it_is_given_some() {
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("guides", "0")])));
    let written = save(&doc, &original, &baseline);
    assert!(text(&parts(&written.bytes), &solution()).contains(DIACRAFT_URN));
    assert!(parts_with_the_old_namespace(&written.bytes).is_empty());
}
