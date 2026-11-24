//! The DiaCraft solution part is read into the model and written from it: DiaCraft's own
//! data goes there, and a file with none gets none.

use std::collections::BTreeMap;

use diacraft_doc::{DIACRAFT_URN, Document};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{CT_SOLUTION, CT_SOLUTIONS, NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder, ZipItem, cell, shape};
use diacraft_vsdx::{DiacraftPart, OriginalBytes, WriteOptions, Written, read, write_with};

use crate::{read_ok, read_with};

fn options() -> WriteOptions {
    WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn text(p: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(p.get(name).unwrap_or_else(|| panic!("no part {name}: {:?}", p.keys())).clone()).unwrap()
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options()).unwrap_or_else(|e| panic!("{e}"))
}

const DIACRAFT_SOLUTION: &str = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap alignment_box="1"/><dc:later keep="me"/></dc:Data><Note>another reader's</Note></SolutionXML>"#;

fn drawing() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build()
}

#[test]
fn a_file_with_no_diacraft_data_gets_no_solutions_index_and_no_part() {
    let original = drawing();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    // An edit of something else.
    doc.title = "Renamed".into();
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    assert!(p.keys().all(|k| !k.contains("solution")), "{:?}", p.keys());
    let (again, report) = read_ok(&written.bytes);
    assert!(again.diacraft.is_none());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
}

#[test]
fn data_set_in_the_model_makes_the_index_the_part_the_relationships_and_the_types() {
    let original = drawing();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "1"), ("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    let index = text(&p, "/visio/solutions/solutions.xml");
    assert!(index.contains(r#"<Solution Name="DiaCraft">"#) && index.contains("r:id="), "{index}");
    let part = text(&p, "/visio/solutions/solution1.xml");
    assert!(part.contains(r#"<SolutionXML Name="DiaCraft">"#) && part.contains(DIACRAFT_URN) && part.contains("dc:version=\"1\""), "{part}");
    assert!(part.contains(r#"<dc:snap alignment_box="1" glue="0"/>"#), "{part}");
    // The types and the relationships.
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(pkg.content_type("/visio/solutions/solutions.xml"), Some("application/vnd.ms-visio.solutions+xml"));
    assert_eq!(pkg.content_type("/visio/solutions/solution1.xml"), Some("application/vnd.ms-visio.solution+xml"));
    assert!(text(&p, "/visio/_rels/document.xml.rels").contains("solutions/solutions.xml"));
    assert!(text(&p, "/visio/solutions/_rels/solutions.xml.rels").contains("solution1.xml"));
    // Every other part of the file is the original's, byte for byte, but the ones that name it.
    let before = parts(&original);
    for (name, bytes) in &before {
        if name == "/visio/_rels/document.xml.rels" || name.starts_with("/docProps/") {
            continue;
        }
        assert_eq!(p.get(name), Some(bytes), "{name}");
    }
    // It reads back, as the model, and the baseline the save returns is the model.
    let (again, report) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { ref version, children: 1 }) if version == "1"), "{:?}", report.diacraft);
    assert_eq!(written.document.diacraft, doc.diacraft);
    // A second save of the same model writes nothing of it again.
    let second = save(&doc, &written.bytes, &written.document);
    assert_eq!(parts(&second.bytes).get("/visio/solutions/solution1.xml"), p.get("/visio/solutions/solution1.xml"));
}

#[test]
fn a_new_drawing_carries_its_data() {
    let mut doc = Document::new_blank();
    assert!(doc.set_diacraft_child("snap", Some(&[("dynamic_grid", "1")])));
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let (again, report) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { .. })));
    // Without data, a new drawing has no solutions at all.
    let plain = write_with(&Document::new_blank(), None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert!(parts(&plain.bytes).keys().all(|k| !k.contains("solution")));
}

#[test]
fn the_part_a_file_has_is_patched_where_it_is_and_what_else_it_holds_stays() {
    let original =
        VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).solution("DiaCraft", DIACRAFT_SOLUTION).build();
    let (mut doc, report) = read_ok(&original);
    let baseline = doc.clone();
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { children: 2, .. })));
    assert_eq!(doc.diacraft_child("snap").unwrap().attrs.get("alignment_box"), Some("1"));
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "0"), ("guides", "0")])));
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    // The same part, no second one.
    assert!(!p.contains_key("/visio/solutions/solution2.xml"));
    let part = text(&p, "/visio/solutions/solution1.xml");
    assert!(part.contains(r#"<dc:snap alignment_box="0" guides="0"/>"#) && part.contains(r#"<dc:later keep="me"/>"#), "{part}");
    assert!(part.contains("<Note>another reader's</Note>"), "{part}");
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
}

#[test]
fn the_row_goes_into_an_index_another_writer_made_and_that_writers_part_is_not_touched() {
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("Other", r#"<SolutionXML Name="Other"><Mine/></SolutionXML>"#)
        .build();
    let (mut doc, report) = read_ok(&original);
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    let before = parts(&original);
    assert_eq!(p.get("/visio/solutions/solution1.xml"), before.get("/visio/solutions/solution1.xml"), "the other writer's part is byte for byte");
    let index = text(&p, "/visio/solutions/solutions.xml");
    assert!(index.contains(r#"Name="Other""#) && index.contains(r#"Name="DiaCraft""#), "{index}");
    let part = text(&p, "/visio/solutions/solution2.xml");
    assert!(part.contains("dc:snap"), "{part}");
    let (again, report) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { .. })));
}

#[test]
fn a_part_named_diacraft_with_no_data_gets_the_data_and_keeps_what_it_holds() {
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", r#"<SolutionXML Name="DiaCraft"><Foreign/></SolutionXML>"#)
        .build();
    let (mut doc, report) = read_ok(&original);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(..))));
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    assert!(written.warnings.iter().all(|w| !w.contains("DiaCraft")), "{:?}", written.warnings);
    let part = text(&parts(&written.bytes), "/visio/solutions/solution1.xml");
    assert!(part.contains("<Foreign/>") && part.contains(r#"<dc:snap glue="0"/>"#), "{part}");
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn a_solution_named_diacraft_that_is_not_ours_is_kept_and_the_save_says_the_data_was_not_saved() {
    // The row says DiaCraft and the part says it is another's.
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", r#"<SolutionXML Name="Elsewhere"><Foreign/></SolutionXML>"#)
        .build();
    let (mut doc, report) = read_ok(&original);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(..))));
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    assert!(written.warnings.iter().any(|w| w.contains("DiaCraft")), "{:?}", written.warnings);
    assert_eq!(parts(&written.bytes).get("/visio/solutions/solution1.xml"), parts(&original).get("/visio/solutions/solution1.xml"));
    // The baseline is the file's, so the next save tries again and says so again.
    assert!(written.document.diacraft.is_none());
}

#[test]
fn data_of_another_version_is_read_whole_and_a_save_of_something_else_leaves_it() {
    let solution =
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="9"><dc:snap grid="0"/></dc:Data></SolutionXML>"#;
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).solution("DiaCraft", solution).build();
    let (mut doc, _) = read_ok(&original);
    assert!(doc.diacraft.is_some());
    assert!(doc.diacraft_child("snap").is_none(), "not interpreted");
    assert!(!doc.set_diacraft_child("snap", Some(&[("grid", "1")])), "not changed");
    let baseline = doc.clone();
    doc.title = "Renamed".into();
    let written = save(&doc, &original, &baseline);
    assert_eq!(parts(&written.bytes).get("/visio/solutions/solution1.xml"), parts(&original).get("/visio/solutions/solution1.xml"));
}

#[test]
fn hostile_solution_parts_read_and_save_without_a_panic() {
    let big = "x".repeat(1 << 16);
    let cases = [
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension"/></SolutionXML>"#.to_owned(),
        format!(
            r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap a="{big}"/></dc:Data></SolutionXML>"#
        ),
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:other" dc:version="1"/></SolutionXML>"#.to_owned(),
        r#"<SolutionXML Name="diacraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"/></SolutionXML>"#.to_owned(),
        "not xml".to_owned(),
    ];
    for solution in cases {
        let original = VsdxBuilder::new().page(PageSpec::new(1, "A")).solution("DiaCraft", &solution).build();
        let Ok((mut doc, _)) = read(&original, &diacraft_vsdx::Limits::default()) else { continue };
        let baseline = doc.clone();
        doc.set_diacraft_child("snap", Some(&[("glue", "0")]));
        let _ = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &options());
    }
}

#[test]
fn data_taken_out_of_the_model_takes_the_data_element_out_of_the_part_and_nothing_else() {
    let solution = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap glue="0"/></dc:Data><Note>another reader's</Note></SolutionXML>"#;
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).solution("DiaCraft", solution).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_none(), "the last child out takes the data with it");
    let written = save(&doc, &original, &baseline);
    let part = text(&parts(&written.bytes), "/visio/solutions/solution1.xml");
    assert!(!part.contains("dc:snap") && !part.contains("Data") && part.contains("<Note>another reader's</Note>"), "{part}");
    let (again, report) = read_ok(&written.bytes);
    assert!(again.diacraft.is_none());
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(..))), "a part with no Data is not one DiaCraft reads: {:?}", report.diacraft);
    assert!(written.document.diacraft.is_none());
}

// ----------------------------------------------------------------------------------------------------: M1, M2, L1, L3

const OURS: &str =
    r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap glue="0"/></dc:Data></SolutionXML>"#;

fn base() -> VsdxBuilder {
    VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
}

fn index_of(rows: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="utf-8"?><Solutions xmlns="{NS_2012}" xmlns:r="{NS_R}">{rows}</Solutions>"#)
}

/// Solutions made by hand: the index `index`, the document's relationship to it, and a part for each `(relationship id, xml)`, with the index's
/// relationship to it.
fn hand_made(index: &str, solutions: &[(&str, &str)]) -> Pkg {
    let mut pkg = Pkg::new().part("/visio/solutions/solutions.xml", CT_SOLUTIONS, index).rel(
        "/visio/document.xml",
        "rId900",
        &format!("{VIS}solutions"),
        "solutions/solutions.xml",
    );
    for (i, (id, xml)) in solutions.iter().enumerate() {
        let n = i + 1;
        pkg = pkg.part(&format!("/visio/solutions/solution{n}.xml"), CT_SOLUTION, xml).rel(
            "/visio/solutions/solutions.xml",
            id,
            &format!("{VIS}solution"),
            &format!("solution{n}.xml"),
        );
    }
    pkg
}

/// The file `original` read, a title change and one DiaCraft choice made on the model, and saved: it must not fail. → the written file.
fn save_with_a_choice(original: &[u8]) -> (Written, Document) {
    let (mut doc, _) = read_ok(original);
    let baseline = doc.clone();
    doc.title = "Renamed".into();
    doc.set_diacraft_child("snap", Some(&[("glue", "0")]));
    let written = save(&doc, original, &baseline);
    (written, doc)
}

/// Every part with "solution" in its name is as it was, byte for byte, and none was added or taken out.
fn solutions_untouched(before: &[u8], after: &[u8]) {
    let (b, a) = (parts(before), parts(after));
    let names = |m: &BTreeMap<String, Vec<u8>>| m.keys().filter(|k| k.contains("solution")).cloned().collect::<Vec<_>>();
    assert_eq!(names(&b), names(&a));
    for name in names(&b) {
        assert_eq!(b.get(&name), a.get(&name), "{name}");
    }
}

#[test]
fn data_the_reader_did_not_read_is_left_as_it_is_and_the_save_says_so() {
    // Data with no version, and a part over the size the reader reads, are not in the model; a model that gained data of
    // its own must not replace them.
    let unversioned = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension"><dc:snap grid="0"/></dc:Data></SolutionXML>"#;
    // The cap of the part is 64 MiB (the part carries the data of the other format); the padding is text that does not compress to a ratio the container refuses.
    let pad: String = {
        let mut x = 99u32;
        (0..diacraft_vsdx::MAX_SOLUTION_PART_BYTES + 16)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                char::from(b'a' + u8::try_from((x >> 24) % 26).unwrap())
            })
            .collect()
    };
    let oversized = format!(
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap grid="0"/></dc:Data><Pad>{pad}</Pad></SolutionXML>"#
    );
    drop(pad);
    for (what, xml) in [("no version", unversioned.to_owned()), ("over the size read", oversized)] {
        let original = base().solution("DiaCraft", &xml).build();
        let (doc, report) = read_ok(&original);
        assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(..))), "{what}: {:?}", report.diacraft);
        assert!(doc.diacraft.is_none(), "{what}");
        let (written, model) = save_with_a_choice(&original);
        solutions_untouched(&original, &written.bytes);
        assert!(written.warnings.iter().any(|w| w.contains("DiaCraft")), "{what}: {:?}", written.warnings);
        assert_eq!(read_ok(&written.bytes).0.title.as_ref(), "Renamed", "{what}: the rest of the save is made");
        // The baseline is the file's, so that the next save refuses again, and says so again.
        assert!(written.document.diacraft.is_none(), "{what}");
        let again = save(&model, &written.bytes, &written.document);
        solutions_untouched(&original, &again.bytes);
        assert!(again.warnings.iter().any(|w| w.contains("DiaCraft")), "{what}: {:?}", again.warnings);
    }
}

#[test]
fn data_with_more_items_than_the_document_can_keep_is_reported_and_not_replaced() {
    // The reader's item budget: Data that does not fit is not in the model (M1), the read says so, and the writer leaves it.
    let children = "<dc:snap a=\"1\"/>".repeat(400);
    let xml =
        format!(r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1">{children}</dc:Data></SolutionXML>"#);
    let without = read_ok(&base().build()).1.stats.items;
    let limits = diacraft_vsdx::Limits { max_items: without + 100, ..diacraft_vsdx::Limits::default() };
    let original = base().solution("DiaCraft", &xml).build();
    let (mut doc, report) = read_with(&original, &limits);
    assert_eq!(report.diacraft, Some(DiacraftPart::Unreadable("too many items to keep")));
    assert_eq!(report.warning_count("diacraft_not_kept"), 1);
    assert!(doc.diacraft.is_none());
    // Saved with the default limits (the self-check reads with those), the file's data is there and is not the model's.
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    solutions_untouched(&original, &written.bytes);
    assert!(written.warnings.iter().any(|w| w.contains("DiaCraft")), "{:?}", written.warnings);
}

#[test]
fn a_solutions_package_the_writer_cannot_use_is_kept_and_never_blocks_a_save() {
    // A malformed relationships part, an index with another root, a row that leads nowhere.
    let row = r#"<Solution Name="DiaCraft"><Rel r:id="rId1"/></Solution>"#;
    let other = r#"<Solution Name="Other"><Rel r:id="rId1"/></Solution>"#;
    let cases: Vec<(&str, Pkg)> = vec![
        (
            "malformed relationships of the index, a row of ours",
            hand_made(&index_of(row), &[]).item(ZipItem::stored("visio/solutions/_rels/solutions.xml.rels", b"<<<not xml")),
        ),
        (
            "malformed relationships of the index, another's row",
            hand_made(&index_of(other), &[]).item(ZipItem::stored("visio/solutions/_rels/solutions.xml.rels", b"<<<not xml")),
        ),
        ("an index with another root", hand_made(&format!(r#"<Elsewhere xmlns="{NS_2012}"><Solution Name="DiaCraft"/></Elsewhere>"#), &[])),
        ("an index that is not XML", hand_made("not xml at all", &[])),
        ("a row whose relationship leads nowhere", hand_made(&index_of(row), &[])),
        ("a part that is not XML", hand_made(&index_of(row), &[("rId1", "not xml")])),
        ("a part with another root", hand_made(&index_of(row), &[("rId1", "<Elsewhere/>")])),
    ];
    for (what, extra) in cases {
        let original = base().with(extra).build();
        let (written, _) = save_with_a_choice(&original);
        solutions_untouched(&original, &written.bytes);
        assert!(written.warnings.iter().any(|w| w.contains("DiaCraft")), "{what}: {:?}", written.warnings);
        assert_eq!(read_ok(&written.bytes).0.title.as_ref(), "Renamed", "{what}");
    }
}

#[test]
fn the_last_data_taken_out_takes_the_part_its_relationship_its_type_and_its_row_with_it() {
    // A file with no DiaCraft data has none of it.
    let original = base().solution("DiaCraft", OURS).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_none());
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    assert!(p.keys().all(|k| !k.contains("solution")), "{:?}", p.keys());
    assert!(!text(&p, "/visio/_rels/document.xml.rels").contains("solution"));
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.content_types().overrides.iter().all(|(name, _)| !name.contains("solution")), "{:?}", pkg.content_types().overrides);
    let (again, report) = read_ok(&written.bytes);
    assert!(again.diacraft.is_none());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
    assert!(written.document.diacraft.is_none());
}

#[test]
fn the_last_data_taken_out_leaves_another_writers_solutions_and_their_rows_as_they_were() {
    let rows = r#"<Solution Name="Other" ID="7" Extra="x"><Rel r:id="rId1"/></Solution><Solution Name="DiaCraft"><Rel r:id="rId2"/></Solution><Solution Name="Last"><Rel r:id="rId3"/></Solution>"#;
    let original = base()
        .with(hand_made(
            &index_of(rows),
            &[
                ("rId1", r#"<SolutionXML Name="Other"><Mine/></SolutionXML>"#),
                ("rId2", OURS),
                ("rId3", r#"<SolutionXML Name="Last"><Mine/></SolutionXML>"#),
            ],
        ))
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    let written = save(&doc, &original, &baseline);
    let (b, p) = (parts(&original), parts(&written.bytes));
    // Ours is gone: its part and its relationship; the others' parts are as they were.
    assert!(!p.contains_key("/visio/solutions/solution2.xml"), "{:?}", p.keys());
    for kept in ["/visio/solutions/solution1.xml", "/visio/solutions/solution3.xml"] {
        assert_eq!(p.get(kept), b.get(kept), "{kept}");
    }
    let rels = text(&p, "/visio/solutions/_rels/solutions.xml.rels");
    assert!(rels.contains("solution1.xml") && rels.contains("solution3.xml") && !rels.contains("solution2.xml"), "{rels}");
    // The index keeps the others' rows, with their attributes, in their order, and has no row of ours.
    let index = text(&p, "/visio/solutions/solutions.xml");
    assert!(!index.contains("DiaCraft"), "{index}");
    let (first, last) = (index.find(r#"<Solution Name="Other" ID="7" Extra="x">"#), index.find(r#"<Solution Name="Last">"#));
    assert!(matches!((first, last), (Some(a), Some(z)) if a < z), "{index}");
    assert!(text(&p, "/visio/_rels/document.xml.rels").contains("solutions/solutions.xml"));
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.content_types().overrides.iter().all(|(name, _)| !name.ends_with("solution2.xml")), "{:?}", pkg.content_types().overrides);
    assert_eq!(pkg.content_type("/visio/solutions/solution1.xml"), Some("application/vnd.ms-visio.solution+xml"));
    let (again, report) = read_ok(&written.bytes);
    assert!(again.diacraft.is_none());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
}

#[test]
fn the_row_added_to_an_index_leaves_another_writers_rows_with_their_attributes_and_in_their_order() {
    // The index is written from its tree, and what the other writer put in it stays.
    let rows =
        r#"<Solution Name="B" ID="2" Extra="y"><Rel r:id="rId1"/></Solution><!-- kept --><Solution Name="A" ID="1"><Rel r:id="rId2"/></Solution>"#;
    let original = base()
        .with(hand_made(
            &index_of(rows),
            &[("rId1", r#"<SolutionXML Name="B"><Mine/></SolutionXML>"#), ("rId2", r#"<SolutionXML Name="A"><Mine/></SolutionXML>"#)],
        ))
        .build();
    let (written, doc) = save_with_a_choice(&original);
    let p = parts(&written.bytes);
    let index = text(&p, "/visio/solutions/solutions.xml");
    let at = |needle: &str| index.find(needle).unwrap_or_else(|| panic!("{needle} is not in {index}"));
    let (b, comment, a, ours) =
        (at(r#"<Solution Name="B" ID="2" Extra="y">"#), at("<!-- kept -->"), at(r#"<Solution Name="A" ID="1">"#), at(r#"Name="DiaCraft""#));
    assert!(b < comment && comment < a && a < ours, "{index}");
    for kept in ["/visio/solutions/solution1.xml", "/visio/solutions/solution2.xml"] {
        assert_eq!(p.get(kept), parts(&original).get(kept), "{kept}");
    }
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

// ---------------------------------------------------------------------------------------- the content types stream

fn types_text(bytes: &[u8]) -> String {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(pkg.content_types_bytes().to_vec()).unwrap()
}

#[test]
fn a_save_that_changes_only_a_snap_option_leaves_the_content_types_stream_as_the_file_wrote_it() {
    // The ruler and the grid are the file's own words (in the document part); every other option is in DiaCraft's part, which the file has or has not.
    // Neither adds, changes or takes out a row of `[Content_Types].xml`, and then writes the file's own bytes.
    for original in [base().build(), base().solution("DiaCraft", OURS).build()] {
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        let mut words = doc.snap_words();
        words.snap = Some(std::sync::Arc::from("3"));
        doc.set_snap_words(&words);
        let written = save(&doc, &original, &baseline);
        assert_eq!(types_text(&written.bytes), types_text(&original), "a ruler and grid change");
    }
    // A choice in the part of a file that has it: the part is rewritten, and its type row is untouched.
    let original = base().solution("DiaCraft", OURS).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "1"), ("guides", "0")])));
    let written = save(&doc, &original, &baseline);
    assert_eq!(types_text(&written.bytes), types_text(&original), "a change to the part");
    assert!(
        parts(&written.bytes).get("/visio/solutions/solution1.xml") != parts(&original).get("/visio/solutions/solution1.xml"),
        "the part did change"
    );
}

#[test]
fn the_rows_of_a_new_part_are_added_after_the_files_rows_and_the_rest_of_the_stream_is_its_own_bytes() {
    let original = base().build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    let written = save(&doc, &original, &baseline);
    let (before, after) = (types_text(&original), types_text(&written.bytes));
    // Everything the file wrote before its closing tag is the start of the new stream.
    let cut = before.rfind("</Types>").unwrap();
    assert!(after.starts_with(&before[..cut]), "{after}");
    assert!(after.ends_with(&before[cut..]), "{after}");
    let added = &after[cut..after.len() - (before.len() - cut)];
    assert_eq!(added.matches("<Override").count(), 2, "{added}");
    assert!(added.contains("/visio/solutions/solutions.xml") && added.contains("/visio/solutions/solution1.xml"), "{added}");
    // And through the model: the types are the package's.
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(pkg.content_type("/visio/solutions/solution1.xml"), Some("application/vnd.ms-visio.solution+xml"));
}

#[test]
fn the_rows_of_a_part_taken_out_leave_the_stream_as_a_file_without_them_wrote_it() {
    let original = base().solution("DiaCraft", OURS).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    let written = save(&doc, &original, &baseline);
    assert_eq!(types_text(&written.bytes), types_text(&base().build()));
    assert!(parts(&written.bytes).keys().all(|k| !k.contains("solution")));
}

#[test]
fn the_index_stays_when_it_holds_anything_but_our_row_and_goes_when_it_holds_nothing() {
    // L-d: the index went when no other `Solution` row was left, with the comment or the element another writer had put in it.
    let rows = r#"<Solution Name="DiaCraft"><Rel r:id="rId1"/></Solution><!-- another writer's note --><Notes Count="2"/>"#;
    let original = base().with(hand_made(&index_of(rows), &[("rId1", OURS)])).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    let written = save(&doc, &original, &baseline);
    let p = parts(&written.bytes);
    assert!(!p.contains_key("/visio/solutions/solution1.xml"), "our part is gone");
    let index = text(&p, "/visio/solutions/solutions.xml");
    assert!(index.contains("<!-- another writer's note -->") && index.contains(r#"<Notes Count="2"/>"#) && !index.contains("DiaCraft"), "{index}");
    assert!(text(&p, "/visio/_rels/document.xml.rels").contains("solutions/solutions.xml"), "the document still reaches the index");
    assert!(!text(&p, "/visio/solutions/_rels/solutions.xml.rels").contains("solution1.xml"));
    let (again, report) = read_ok(&written.bytes);
    assert!(again.diacraft.is_none());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
    // An index with blanks between our row and nothing else goes, as before (the other tests of this file).
    let blanks = "\n  <Solution Name=\"DiaCraft\"><Rel r:id=\"rId1\"/></Solution>\n";
    let original = base().with(hand_made(&index_of(blanks), &[("rId1", OURS)])).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert!(doc.set_diacraft_child("snap", None));
    let written = save(&doc, &original, &baseline);
    assert!(parts(&written.bytes).keys().all(|k| !k.contains("solution")));
}
