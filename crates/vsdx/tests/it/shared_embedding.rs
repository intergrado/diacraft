//! Item M10: **an embedded object or a picture that a part taken out and a part kept both reach stays, and every row of the file leads to a part it has**
//! (a part goes with the row of a part taken out only when nothing else reaches it). Both writers: the model writer (`write_with(doc, None, ..)`) and the
//! save into the file the drawing was read from. The kept reach is a page, a master, the document part, a held part, and a row that spells its target another way.
//!
//! Every input is built in code.

use std::collections::BTreeSet;

use diacraft_doc::Document;
use diacraft_opc::TargetMode;
use diacraft_opc::name::{fold, is_rels_part, rels_source};
use diacraft_testkit::package::{CT_PAGE, CT_PAGES, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{CT_OLE, DOC, NOW, package};
use crate::read_ok;

const SHARED: &str = "/visio/embeddings/shared.bin";
const OLE_SHAPE: &str = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;

fn ole() -> String {
    format!("{OD}oleObject")
}

/// Which writer.
#[derive(Clone, Copy, Debug)]
enum Path {
    Model,
    Splice,
}

fn save(doc: &Document, original: &[u8], path: Path) -> Written {
    let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let result = match path {
        Path::Model => write_with(doc, None, &opts),
        Path::Splice => write_with(doc, Some(&OriginalBytes { bytes: original, baseline: None }), &opts),
    };
    result.unwrap_or_else(|e| panic!("{path:?}: the save refused: {e}"))
}

/// Every row of the file that leads inside it, as `(source, Id, the part it leads to)`, where the part is not in the file.
fn dangling(bytes: &[u8]) -> Vec<(String, String, String)> {
    let pkg = package(bytes);
    let names: BTreeSet<String> = pkg.parts().map(fold).collect();
    let mut out = Vec::new();
    for rels in pkg.parts().filter(|n| is_rels_part(n)).map(str::to_owned).collect::<Vec<_>>() {
        let Some(source) = rels_source(&rels) else { continue };
        for row in pkg.rels(&source).unwrap().items {
            if row.mode == TargetMode::External {
                continue;
            }
            if let Some(target) = row.target.clone()
                && !names.contains(&fold(&target))
            {
                out.push((source.clone(), row.id.clone(), target));
            }
        }
    }
    out
}

/// The parts of the file, folded.
fn names(bytes: &[u8]) -> BTreeSet<String> {
    package(bytes).parts().map(fold).collect()
}

fn assert_kept(written: &Written, path: Path, case: &str) {
    let have = names(&written.bytes);
    assert!(have.contains(&fold(SHARED)), "{case} ({path:?}): the object a kept part reaches was taken out; removed {:?}", written.removed);
    let bad = dangling(&written.bytes);
    assert!(bad.is_empty(), "{case} ({path:?}): rows lead to parts the file lacks: {bad:?}");
    // And the file reads, with the object held.
    let (back, _) = read_ok(&written.bytes);
    assert!(back.parts.iter().any(|p| fold(&p.name) == fold(SHARED)), "{case} ({path:?}): the object is not held after a read");
}

fn assert_gone(written: &Written, path: Path, case: &str) {
    assert!(!names(&written.bytes).contains(&fold(SHARED)), "{case} ({path:?}): the object only the part taken out reached stays");
    let bad = dangling(&written.bytes);
    assert!(bad.is_empty(), "{case} ({path:?}): rows lead to parts the file lacks: {bad:?}");
}

fn object() -> Pkg {
    Pkg::new().part(SHARED, CT_OLE, [1u8, 2, 3])
}

// ------------------------------------------------------------------------------------------------------------------------------ the cases

#[test]
fn an_object_two_pages_reach_stays_when_either_page_is_taken_out_and_goes_with_both() {
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
        .page(PageSpec::new(2, "B").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
        .with(object())
        .build();
    for path in [Path::Model, Path::Splice] {
        for at in [0usize, 1] {
            let (mut doc, _) = read_ok(&bytes);
            doc.pages.remove(at).unwrap();
            assert_kept(&save(&doc, &bytes, path), path, &format!("page {at} taken out"));
        }
        // (The page is added first, so that it has an `ID` of its own: the save into the file knows a page by its `ID`.)
        let (mut doc, _) = read_ok(&bytes);
        doc.add_page(Some("New")).unwrap();
        doc.pages.remove(1).unwrap();
        doc.pages.remove(0).unwrap();
        assert_gone(&save(&doc, &bytes, path), path, "both pages taken out");
    }
}

#[test]
fn an_object_a_page_and_a_master_reach_stays_when_either_is_taken_out() {
    let bytes = VsdxBuilder::new()
        .master(MasterSpec::new(1, "M").shapes(&shape(4, &cell("Width", "1"))).rel("rId3", &ole(), "../embeddings/shared.bin"))
        .page(PageSpec::new(1, "A").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
        .page(PageSpec::new(2, "B").shapes(&shape(1, &cell("PinX", "1"))))
        .with(object())
        .build();
    for path in [Path::Model, Path::Splice] {
        let (mut doc, _) = read_ok(&bytes);
        doc.pages.remove(0).unwrap();
        assert_kept(&save(&doc, &bytes, path), path, "the page taken out, the master kept");
        let (mut doc, _) = read_ok(&bytes);
        doc.masters.remove(0).unwrap();
        assert_kept(&save(&doc, &bytes, path), path, "the master taken out, the page kept");
    }
}

#[test]
fn an_object_the_document_part_or_a_held_part_reaches_stays_when_the_page_is_taken_out() {
    let by_document = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
        .page(PageSpec::new(2, "B").shapes(&shape(1, &cell("PinX", "1"))))
        .with(object().rel(DOC, "rId90", &ole(), "embeddings/shared.bin"))
        .build();
    let by_held = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
        .page(PageSpec::new(2, "B").shapes(&shape(1, &cell("PinX", "1"))))
        .with(object().part("/visio/extra/x.bin", "application/octet-stream", [7u8]).rel(
            "/visio/extra/x.bin",
            "rId1",
            &ole(),
            "../embeddings/shared.bin",
        ))
        .build();
    for path in [Path::Model, Path::Splice] {
        for (case, bytes) in [("the document part", &by_document), ("a held part", &by_held)] {
            let (mut doc, _) = read_ok(bytes);
            doc.pages.remove(0).unwrap();
            assert_kept(&save(&doc, bytes, path), path, case);
        }
    }
}

#[test]
fn an_object_a_kept_page_reaches_by_a_target_spelt_another_way_stays() {
    for (case, target) in [
        ("absolute", "/visio/embeddings/shared.bin"),
        ("in another case", "../Embeddings/SHARED.bin"),
        ("through a dot", "./../embeddings/shared.bin"),
    ] {
        let bytes = VsdxBuilder::new()
            .page(PageSpec::new(1, "A").shapes(OLE_SHAPE).rel("rId2", &ole(), "../embeddings/shared.bin"))
            .page(PageSpec::new(2, "B").shapes(OLE_SHAPE).rel("rId2", &ole(), target))
            .with(object())
            .build();
        for path in [Path::Model, Path::Splice] {
            let (mut doc, _) = read_ok(&bytes);
            doc.pages.remove(0).unwrap();
            assert_kept(&save(&doc, &bytes, path), path, case);
        }
    }
}

/// Three pages that name one part, which reaches the object.
fn one_part_three_pages() -> Vec<u8> {
    let content = format!("<PageContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes>{OLE_SHAPE}</Shapes></PageContents>");
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><PageSheet/><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><PageSheet/><Rel r:id=\"rId2\"/></Page><Page ID=\"3\" Name=\"C\"><PageSheet/><Rel r:id=\"rId3\"/></Page></Pages>"
    );
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/one.xml", CT_PAGE, content)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "one.xml")
        .rel("/visio/pages/pages.xml", "rId2", &format!("{VIS}page"), "one.xml")
        .rel("/visio/pages/pages.xml", "rId3", &format!("{VIS}page"), "one.xml")
        .rel("/visio/pages/one.xml", "rId2", &ole(), "../embeddings/shared.bin")
        .part(SHARED, CT_OLE, [3u8]);
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn an_object_of_a_part_three_pages_name_stays_when_one_page_is_taken_out_and_another_is_given_a_part_of_its_own() {
    let bytes = one_part_three_pages();
    for path in [Path::Model, Path::Splice] {
        for at in [0usize, 1, 2] {
            let (mut doc, _) = read_ok(&bytes);
            doc.pages.remove(at).unwrap();
            let written = save(&doc, &bytes, path);
            assert_kept(&written, path, &format!("page {at} of the three taken out"));
            // The two pages left have their content: the part they name is in the file.
            let (back, _) = read_ok(&written.bytes);
            assert_eq!(back.pages.len(), 2, "{path:?}");
            for page in back.pages.iter() {
                assert_eq!(page.shapes.len(), 1, "{path:?}, page {at} taken out: page {} lost its shapes", page.name);
            }
        }
    }
    // Every page taken out: the part goes, and the object with it.
    for path in [Path::Model, Path::Splice] {
        let (mut doc, _) = read_ok(&bytes);
        doc.add_page(Some("New")).unwrap();
        for _ in 0..3 {
            doc.pages.remove(0).unwrap();
        }
        let written = save(&doc, &bytes, path);
        assert_gone(&written, path, "the three pages taken out");
        assert!(!names(&written.bytes).contains("/visio/pages/one.xml"), "{path:?}: the part no page names stays");
    }
    // The model writer: the last page edited, so that it is a part of its own, and the first taken out.
    let (mut doc, _) = read_ok(&bytes);
    crate::model_writer::page_cell(&mut doc, 3, 0, "PinX", "5");
    doc.pages.remove(0).unwrap();
    assert_kept(&save(&doc, &bytes, Path::Model), Path::Model, "one page edited and one taken out");
}

#[test]
fn an_object_of_a_part_two_masters_name_stays_when_one_master_is_taken_out() {
    let content = format!("<MasterContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes>{OLE_SHAPE}</Shapes></MasterContents>");
    let masters = format!(
        "<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Master ID=\"1\" Name=\"A\" NameU=\"A\"><Rel r:id=\"rId1\"/></Master><Master ID=\"2\" Name=\"B\" NameU=\"B\"><Rel r:id=\"rId2\"/></Master></Masters>"
    );
    let pkg = Pkg::new()
        .part("/visio/masters/masters.xml", diacraft_testkit::package::CT_MASTERS, masters)
        .rel(DOC, "rId51", &format!("{VIS}masters"), "masters/masters.xml")
        .part("/visio/masters/one.xml", diacraft_testkit::package::CT_MASTER, content)
        .rel("/visio/masters/masters.xml", "rId1", &format!("{VIS}master"), "one.xml")
        .rel("/visio/masters/masters.xml", "rId2", &format!("{VIS}master"), "one.xml")
        .rel("/visio/masters/one.xml", "rId2", &ole(), "../embeddings/shared.bin")
        .part(SHARED, CT_OLE, [3u8]);
    let bytes = VsdxBuilder::new().without_masters_part().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).with(pkg).build();
    for path in [Path::Model, Path::Splice] {
        for at in [0usize, 1] {
            let (mut doc, _) = read_ok(&bytes);
            doc.masters.remove(at).unwrap();
            let written = save(&doc, &bytes, path);
            assert_kept(&written, path, &format!("master {at} of the two taken out"));
            let (back, _) = read_ok(&written.bytes);
            assert_eq!(back.masters.len(), 1, "{path:?}");
            assert_eq!(back.masters.iter().next().map(|m| m.shapes.len()), Some(1), "{path:?}: the master left lost its shapes");
        }
    }
}
