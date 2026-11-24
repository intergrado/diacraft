//! Relationships: what a `.rels` part gives, and that a cycle, a dangling target and a strange row are data.

use diacraft_opc::{Limits, Package, TargetMode, Warning};

use crate::zipbuild::{Item, build};
use crate::{base_items, rels_xml};

const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/";

fn package(parts: Vec<Item>) -> Package {
    let mut items = base_items();
    items.extend(parts);
    Package::read(&build(&items), &Limits::default()).unwrap()
}

#[test]
fn a_part_without_a_relationships_part_has_no_relationships() {
    let p = package(vec![Item::stored("visio/document.xml", b"<d/>")]);
    let rels = p.rels("/visio/document.xml").unwrap();
    assert!(rels.items.is_empty() && rels.warnings.is_empty());
    assert!(p.rels("/").unwrap().items.is_empty(), "nor does the package");
    assert!(p.rels("/visio/not-there.xml").unwrap().items.is_empty());
}

#[test]
fn the_package_relationships_come_from_the_root_rels_part() {
    let rels = rels_xml(&[
        ("rId1", &format!("{VIS}document"), "visio/document.xml", None),
        ("rId2", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml", None),
    ]);
    let p = package(vec![Item::stored("_rels/.rels", rels.as_bytes()), Item::stored("visio/document.xml", b"<d/>")]);
    let r = p.rels("/").unwrap();
    assert_eq!(r.items.len(), 2);
    assert_eq!(r.get("rId1").unwrap().target.as_deref(), Some("/visio/document.xml"));
    assert_eq!(r.get("rId2").unwrap().target.as_deref(), Some("/docProps/core.xml"));
    assert_eq!(p.rels("").unwrap(), r, "the root is named by `/` or by nothing");
}

#[test]
fn a_parts_relationships_resolve_against_that_part() {
    // V4 through a package: source `/visio/pages/page1.xml`, `../masters/master1.xml` and an absolute one.
    let rels = rels_xml(&[
        ("rId1", &format!("{VIS}master"), "../masters/master1.xml", None),
        ("rId2", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image", "/visio/media/image1.png", None),
    ]);
    let p = package(vec![
        Item::stored("visio/pages/page1.xml", b"<p/>"),
        Item::stored("visio/pages/_rels/page1.xml.rels", rels.as_bytes()),
        Item::stored("visio/masters/master1.xml", b"<m/>"),
    ]);
    let r = p.rels("/visio/pages/page1.xml").unwrap();
    assert_eq!(r.get("rId1").unwrap().target.as_deref(), Some("/visio/masters/master1.xml"));
    assert_eq!(r.get("rId2").unwrap().target.as_deref(), Some("/visio/media/image1.png"));
    assert!(p.part(r.get("rId1").unwrap().target.as_deref().unwrap()).is_some(), "the first target is a part");
    assert!(p.part(r.get("rId2").unwrap().target.as_deref().unwrap()).is_none(), "the second is dangling, and that is only data");
}

#[test]
fn a_target_that_spells_a_part_in_another_case_resolves_to_the_name_the_package_has() {
    // A lookup folds ASCII case and the case a part arrived with is the one it keeps. A target spelled `VISIO/Document.XML` is the part
    // `/visio/document.xml`, so the name a reader or a writer takes from it, and the folder of a part it makes beside it, are the package's; the target as written is
    // kept beside it. A target that names no part is the name it resolves to, as it is spelled.
    let root = rels_xml(&[
        ("rId1", &format!("{VIS}document"), "VISIO/Document.XML", None),
        ("rId2", "urn:example:dangling", "Other/Thing.xml", None),
        ("rId3", "urn:example:absolute", "/Visio/PAGES/page1.XML", None),
    ]);
    let document = rels_xml(&[("rId1", &format!("{VIS}pages"), "Pages/PAGES.xml", None)]);
    let p = package(vec![
        Item::stored("_rels/.rels", root.as_bytes()),
        Item::stored("visio/document.xml", b"<d/>"),
        Item::stored("visio/_rels/document.xml.rels", document.as_bytes()),
        Item::stored("visio/pages/pages.xml", b"<p/>"),
        Item::stored("visio/pages/page1.xml", b"<p/>"),
    ]);
    let r = p.rels("/").unwrap();
    assert_eq!(r.get("rId1").unwrap().target.as_deref(), Some("/visio/document.xml"));
    assert_eq!(r.get("rId1").unwrap().target_raw, "VISIO/Document.XML", "the target as written is kept");
    assert_eq!(r.get("rId2").unwrap().target.as_deref(), Some("/Other/Thing.xml"), "a dangling target is spelled as it resolves");
    assert_eq!(r.get("rId3").unwrap().target.as_deref(), Some("/visio/pages/page1.xml"));
    // The source in any case finds its relationships part, and what they reach is named as the package has it.
    for source in ["/visio/document.xml", "/VISIO/DOCUMENT.XML"] {
        let r = p.rels(source).unwrap();
        assert_eq!(r.get("rId1").unwrap().target.as_deref(), Some("/visio/pages/pages.xml"), "from {source}");
    }
}

#[test]
fn the_rows_keep_their_attributes_and_their_order() {
    // vsdx-preservation R5: Id, Type, Target and TargetMode, unchanged, rows in order; an Id is never renumbered (V3 there).
    let rels = rels_xml(&[
        ("rId7", "urn:example:a", "x/a.bin", None),
        ("rId3", "urn:example:b", "http://example.com/b?q=1", Some("External")),
        ("zz", "urn:example:c", "../c%20d.bin", Some("Internal")),
    ]);
    let p = package(vec![Item::stored("a/b.xml", b"<b/>"), Item::stored("a/_rels/b.xml.rels", rels.as_bytes())]);
    let r = p.rels("/a/b.xml").unwrap();
    let ids: Vec<&str> = r.items.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["rId7", "rId3", "zz"]);
    assert_eq!(r.items[0].ty, "urn:example:a");
    assert_eq!(r.items[0].target_raw, "x/a.bin");
    assert_eq!(r.items[0].mode, TargetMode::Internal);
    assert_eq!(r.items[0].target.as_deref(), Some("/a/x/a.bin"));
    assert_eq!(r.items[1].target_raw, "http://example.com/b?q=1");
    assert_eq!(r.items[1].mode, TargetMode::External);
    assert_eq!(r.items[1].target, None, "an external target names no part and is never resolved or fetched");
    assert_eq!(r.items[2].target_raw, "../c%20d.bin", "the target as written is kept beside the resolved one");
    assert_eq!(r.items[2].target.as_deref(), Some("/c d.bin"));
}

#[test]
fn a_relationship_type_is_compared_exactly() {
    // `.../relationships/Page` is not `.../relationships/page`.
    let rels = rels_xml(&[
        ("rId1", &format!("{VIS}Page"), "p1.xml", None),
        ("rId2", &format!("{VIS}page"), "p2.xml", None),
        ("rId3", &format!("{VIS}page "), "p3.xml", None),
    ]);
    let p = package(vec![Item::stored("v/doc.xml", b"<d/>"), Item::stored("v/_rels/doc.xml.rels", rels.as_bytes())]);
    let r = p.rels("/v/doc.xml").unwrap();
    let lower: Vec<&str> = r.of_type(&format!("{VIS}page")).map(|x| x.id.as_str()).collect();
    let upper: Vec<&str> = r.of_type(&format!("{VIS}Page")).map(|x| x.id.as_str()).collect();
    assert_eq!(lower, ["rId2"]);
    assert_eq!(upper, ["rId1"]);
}

#[test]
fn the_target_mode_is_internal_unless_it_says_external() {
    let rels = rels_xml(&[
        ("a", "t", "x.xml", None),
        ("b", "t", "x.xml", Some("Internal")),
        ("c", "t", "http://e/x", Some("External")),
        ("d", "t", "http://e/x", Some("external")),
        ("e", "t", "x.xml", Some("")),
        ("f", "t", "x.xml", Some("Nonsense")),
    ]);
    let p = package(vec![Item::stored("d.xml", b"<d/>"), Item::stored("_rels/d.xml.rels", rels.as_bytes())]);
    let r = p.rels("/d.xml").unwrap();
    let modes: Vec<TargetMode> = r.items.iter().map(|x| x.mode).collect();
    use TargetMode::{External, Internal};
    // Labelled choice: the check for External is case-insensitive, because an external URL read as a part name is harmless but
    // a part name read as an external URL would drop a part; and any other value is internal, the schema's default.
    assert_eq!(modes, [Internal, Internal, External, External, Internal, Internal]);
}

#[test]
fn cycles_are_data() {
    // A relationship to itself, A to B and B to A, and a chain that returns to its start: reading and asking each part for its
    // relationships ends, and the targets are plain names. (This crate walks no graph, so there is nothing to loop: a walker
    // keeps its own set of parts seen.)
    let ab = rels_xml(&[("r1", "t", "b.xml", None), ("r2", "t", "a.xml", None), ("r3", "t", "/a.xml", None)]);
    let ba = rels_xml(&[("r1", "t", "a.xml", None), ("r2", "t", "c.xml", None)]);
    let ca = rels_xml(&[("r1", "t", "a.xml", None)]);
    let p = package(vec![
        Item::stored("a.xml", b"<a/>"),
        Item::stored("_rels/a.xml.rels", ab.as_bytes()),
        Item::stored("b.xml", b"<b/>"),
        Item::stored("_rels/b.xml.rels", ba.as_bytes()),
        Item::stored("c.xml", b"<c/>"),
        Item::stored("_rels/c.xml.rels", ca.as_bytes()),
    ]);
    assert_eq!(p.rels("/a.xml").unwrap().items.len(), 3);
    assert_eq!(p.rels("/b.xml").unwrap().items.len(), 2);
    assert_eq!(p.rels("/c.xml").unwrap().items.len(), 1);
    assert_eq!(p.rels("/a.xml").unwrap().get("r2").unwrap().target.as_deref(), Some("/a.xml"));
}

#[test]
fn dangling_targets_are_data() {
    let rels = rels_xml(&[
        ("r1", "t", "no-such-part.xml", None),
        ("r2", "t", "../../../outside.xml", None),
        ("r3", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail", "../docProps/thumbnail.emf", None),
    ]);
    let p = package(vec![Item::stored("v/d.xml", b"<d/>"), Item::stored("v/_rels/d.xml.rels", rels.as_bytes())]);
    let r = p.rels("/v/d.xml").unwrap();
    assert_eq!(r.items.len(), 3);
    for rel in &r.items {
        let target = rel.target.as_deref().expect("a name was resolved");
        assert!(p.part(target).is_none(), "{target} is dangling");
    }
    assert_eq!(r.get("r2").unwrap().target.as_deref(), Some("/outside.xml"));
}

#[test]
fn a_relationships_part_with_no_source_part_is_only_a_part() {
    // The reader keeps it as a part and does not fail the open.
    let p = package(vec![Item::stored("visio/_rels/missing.xml.rels", rels_xml(&[("r1", "t", "x.xml", None)]).as_bytes())]);
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/_rels/missing.xml.rels"]);
    assert_eq!(p.rels("/visio/missing.xml").unwrap().items.len(), 1);
}

#[test]
fn a_row_without_id_type_or_target_is_skipped_and_reported() {
    let xml = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Type=\"t\" Target=\"a\"/><Relationship Id=\"1\" Target=\"a\"/><Relationship Id=\"2\" Type=\"t\"/><Relationship Id=\"3\" Type=\"t\" Target=\"a.xml\"/></Relationships>";
    let p = package(vec![Item::stored("d.xml", b"<d/>"), Item::stored("_rels/d.xml.rels", xml.as_bytes())]);
    let r = p.rels("/d.xml").unwrap();
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].id, "3");
    let skipped = r.warnings.iter().filter(|w| matches!(w, Warning::IncompleteRelationship { .. })).count();
    assert_eq!(skipped, 3, "{:?}", r.warnings);
}

#[test]
fn two_rows_with_one_id_are_both_kept_and_reported() {
    let rels = rels_xml(&[("rId1", "t", "a.xml", None), ("rId1", "t", "b.xml", None)]);
    let p = package(vec![Item::stored("d.xml", b"<d/>"), Item::stored("_rels/d.xml.rels", rels.as_bytes())]);
    let r = p.rels("/d.xml").unwrap();
    assert_eq!(r.items.len(), 2);
    assert_eq!(r.get("rId1").unwrap().target_raw, "a.xml", "a lookup finds the first");
    assert!(r.warnings.iter().any(|w| matches!(w, Warning::DuplicateRelationshipId { id, .. } if id == "rId1")), "{:?}", r.warnings);
}

#[test]
fn a_relationships_part_that_is_not_well_formed_is_an_error_for_that_part_only() {
    let p = package(vec![
        Item::stored("a.xml", b"<a/>"),
        Item::stored("_rels/a.xml.rels", b"<Relationships><Relationship Id=\"1\""),
        Item::stored("b.xml", b"<b/>"),
        Item::stored("_rels/b.xml.rels", b"<Wrong/>"),
        Item::stored("c.xml", b"<c/>"),
    ]);
    assert_eq!(p.rels("/a.xml").unwrap_err().class(), "xml_malformed");
    assert_eq!(p.rels("/b.xml").unwrap_err().class(), "xml_malformed", "the root must be Relationships");
    assert!(p.rels("/c.xml").unwrap().items.is_empty());
    assert!(p.rels("/a.xml").unwrap_err().to_string().contains("_rels/a.xml.rels"));
}

#[test]
fn a_dtd_in_a_relationships_part_is_refused() {
    // For a `.rels` part.
    let p = package(vec![
        Item::stored("a.xml", b"<a/>"),
        Item::stored("_rels/a.xml.rels", b"<!DOCTYPE Relationships [<!ENTITY a \"b\">]><Relationships/>"),
    ]);
    assert_eq!(p.rels("/a.xml").unwrap_err().class(), "xml_entity_refused");
}

#[test]
fn the_relationship_limits_hold_at_their_boundaries() {
    let limits = Limits { max_relationships: 3, max_package_xml_bytes: 1 << 20, ..Limits::default() };
    let rows = |n: usize| {
        let owned: Vec<(String, String)> = (0..n).map(|i| (format!("r{i}"), format!("p{i}.xml"))).collect();
        let r: Vec<(&str, &str, &str, Option<&str>)> = owned.iter().map(|(i, t)| (i.as_str(), "t", t.as_str(), None)).collect();
        rels_xml(&r)
    };
    let make = |n: usize| {
        let mut items = base_items();
        items.push(Item::stored("d.xml", b"<d/>"));
        items.push(Item::stored("_rels/d.xml.rels", rows(n).as_bytes()));
        Package::read(&build(&items), &limits).unwrap()
    };
    assert_eq!(make(3).rels("/d.xml").unwrap().items.len(), 3);
    assert_eq!(make(4).rels("/d.xml").unwrap_err().class(), "too_many_relationships");
    // The size limit of a relationships part is the package-XML one.
    let text = rows(8);
    let tight = Limits { max_package_xml_bytes: text.len() as u64, ..Limits::default() };
    let mut items = base_items();
    items.push(Item::stored("_rels/d.xml.rels", text.as_bytes()));
    let at = Package::read(&build(&items), &tight).unwrap();
    assert_eq!(at.rels("/d.xml").unwrap().items.len(), 8);
}

#[test]
fn an_over_size_relationships_part_is_refused_when_the_package_is_read_not_when_it_is_asked_for() {
    // The limit is on bytes, and applies to every `.rels` part, so a package cannot hold one the reader will not parse. Reading
    // the package refuses it, and with it any chance of a `rels` call that would build a huge tree. (The limit is above the content
    // types stream, which is held to it too, and below the part: it is the part that is named.)
    let owned: Vec<(String, String)> = (0..12).map(|i| (format!("r{i}"), format!("p{i}.xml"))).collect();
    let rows: Vec<(&str, &str, &str, Option<&str>)> = owned.iter().map(|(i, t)| (i.as_str(), "t", t.as_str(), None)).collect();
    let text = rels_xml(&rows);
    assert!(text.len() > 600, "{}", text.len());
    let tight = Limits { max_package_xml_bytes: 600, ..Limits::default() };
    let mut items = base_items();
    items.push(Item::stored("_rels/d.xml.rels", text.as_bytes()));
    let err = Package::read(&build(&items), &tight).unwrap_err();
    assert_eq!(err.class(), "part_too_large");
    assert!(err.to_string().contains("_rels/d.xml.rels"), "{err}");
    // Not a `.rels` part by name: a part like any other, and the limit of a part.
    let mut items = base_items();
    items.push(Item::stored("notes/d.xml.rels.txt", text.as_bytes()));
    items.push(Item::stored("other/d.rels", text.as_bytes()));
    assert!(Package::read(&build(&items), &tight).is_ok());
}

#[test]
fn the_item_limit_of_a_relationships_part_holds_at_its_boundary() {
    // The root and its namespace declaration, and three rows of an element and three attributes each: 2 + 3 x 4 = 14 items. (The content
    // types stream of the package is held to the same limit, and is 11.)
    let rels = rels_xml(&[("1", "t", "a.xml", None), ("2", "t", "b.xml", None), ("3", "t", "c.xml", None)]);
    let mk = |nodes: usize| {
        let mut items = base_items();
        items.push(Item::stored("d.xml", b"<d/>"));
        items.push(Item::stored("_rels/d.xml.rels", rels.as_bytes()));
        Package::read(&build(&items), &Limits { max_package_xml_nodes: nodes, ..Limits::default() }).unwrap()
    };
    assert_eq!(mk(14).rels("/d.xml").unwrap().items.len(), 3);
    assert_eq!(mk(13).rels("/d.xml").unwrap_err().class(), "xml_too_complex");
}

#[test]
fn a_relationships_part_may_be_read_within_a_number_of_items_and_a_part_with_more_is_refused_as_too_complex() {
    // `rels_within` is `rels` with a bound of the caller's on the items of the tree (the pool of a read says how many it can hold). Two rows are an
    // element each with three attributes: 8 items, and the root with its namespace, 10.
    let rels = rels_xml(&[
        ("rId1", &format!("{VIS}document"), "visio/document.xml", None),
        ("rId2", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml", None),
    ]);
    let p = package(vec![Item::stored("_rels/.rels", rels.as_bytes()), Item::stored("visio/document.xml", b"<d/>")]);
    let all = p.rels("/").unwrap();
    assert_eq!(p.rels_within("/", usize::MAX).unwrap(), all, "no bound is `rels`");
    assert_eq!(p.rels_within("/", 10).unwrap(), all, "the tree has 10 items and a bound of 10 holds it");
    for bound in [0, 1, 9] {
        assert_eq!(p.rels_within("/", bound).unwrap_err().class(), "xml_too_complex", "a bound of {bound}");
    }
    // The package's own limit still holds, whatever the caller allows: forty rows are 162 items, and the package allows 100.
    let rows: Vec<(String, String, String, Option<&str>)> =
        (0..40).map(|i| (format!("rId{i}"), format!("{VIS}other"), format!("visio/p{i}.xml"), None)).collect();
    let many = rels_xml(&rows.iter().map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), *d)).collect::<Vec<_>>());
    let mut items = base_items();
    items.extend([Item::stored("_rels/.rels", many.as_bytes()), Item::stored("visio/document.xml", b"<d/>")]);
    let p = Package::read(&build(&items), &Limits { max_package_xml_nodes: 100, ..Limits::default() }).unwrap();
    for bound in [usize::MAX, 1_000, 162] {
        assert_eq!(p.rels_within("/", bound).unwrap_err().class(), "xml_too_complex", "a bound of {bound}");
    }
    assert_eq!(p.rels("/").unwrap_err().class(), "xml_too_complex");
    let roomy = Package::read(&build(&items), &Limits { max_package_xml_nodes: 1_000, ..Limits::default() }).unwrap();
    assert_eq!(roomy.rels_within("/", 162).unwrap().items.len(), 40);
    assert_eq!(roomy.rels_within("/", 161).unwrap_err().class(), "xml_too_complex");
    // A part with no relationships has none whatever the bound.
    assert!(roomy.rels_within("/visio/not-there.xml", 0).unwrap().items.is_empty());
}
