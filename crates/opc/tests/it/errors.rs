//! What a caller shows: the class word for a harness, and `kind()` for a person (review finding 13), and the warnings that say what was
//! odd about a package that read.

use diacraft_opc::name::rels_source;
use diacraft_opc::{ErrorKind, Limits, OpcError, Package, Warning, XmlError};

use crate::zipbuild::{Item, build, rows_only};
use crate::{base_items, package_with, rels_xml};

fn xml(error: XmlError) -> OpcError {
    OpcError::Xml { part: "p".into(), error }
}

#[test]
fn every_error_has_a_kind_and_a_class_and_the_two_agree_about_what_went_wrong() {
    use ErrorKind::{Damaged, NotAPackage, TooBig};
    let name = || "n".to_owned();
    // Each variant, with the class word it reports and the kind a person is told. Not a package, past a limit, or damaged.
    let table: Vec<(OpcError, &str, ErrorKind)> = vec![
        (OpcError::NotZip, "not_zip", NotAPackage),
        (OpcError::OleWithPackageExtension, "ole_with_package_extension", NotAPackage),
        (OpcError::MissingContentTypes, "missing_content_types", NotAPackage),
        (OpcError::EncryptedPackage { name: name() }, "encrypted_package", NotAPackage),
        (OpcError::UnsupportedCompression { name: name(), method: "12".into() }, "unsupported_compression", NotAPackage),
        (OpcError::ZipRatioExceeded { name: name(), limit: 1 }, "zip_ratio_exceeded", TooBig),
        (OpcError::PartTooLarge { name: name(), limit: 1 }, "part_too_large", TooBig),
        (OpcError::PackageTooLarge { limit: 1 }, "package_too_large", TooBig),
        (OpcError::TooManyParts { limit: 1 }, "too_many_parts", TooBig),
        (OpcError::TooManyRelationships { part: name(), limit: 1 }, "too_many_relationships", TooBig),
        (OpcError::TooManyContentTypeEntries { limit: 1 }, "too_many_content_type_entries", TooBig),
        (OpcError::OutOfMemory { what: name() }, "out_of_memory", TooBig),
        (xml(XmlError::TooDeep { limit: 1 }), "xml_too_complex", TooBig),
        (xml(XmlError::TooManyNodes { limit: 1 }), "xml_too_complex", TooBig),
        (xml(XmlError::AttributeTooLarge { limit: 1 }), "xml_too_complex", TooBig),
        (OpcError::ZipCorrupt { detail: name() }, "zip_corrupt", Damaged),
        (OpcError::PathTraversal { name: name() }, "path_traversal", Damaged),
        (OpcError::InvalidPartName { name: name(), reason: "r" }, "invalid_part_name", Damaged),
        (OpcError::DuplicatePart { first: name(), second: name() }, "duplicate_part", Damaged),
        (OpcError::PartNamePrefix { prefix: name(), part: name() }, "part_name_prefix", Damaged),
        (xml(XmlError::Malformed { offset: 0, message: name() }), "xml_malformed", Damaged),
        (xml(XmlError::EntityRefused { offset: 0, what: name() }), "xml_entity_refused", Damaged),
    ];
    for (error, class, kind) in &table {
        assert_eq!((error.class(), error.kind()), (*class, *kind), "{error}");
    }
    // Every class word the crate has is in the table: none is reachable without a kind.
    let classes: std::collections::BTreeSet<&str> = table.iter().map(|(e, _, _)| e.class()).collect();
    assert_eq!(classes.len(), 20, "{classes:?}");
}

#[test]
fn the_kind_of_what_a_real_read_gives_is_what_a_person_would_say() {
    let kind = |bytes: &[u8], limits: &Limits| Package::read(bytes, limits).unwrap_err().kind();
    let d = Limits::default();
    assert_eq!(kind(b"text, not a zip", &d), ErrorKind::NotAPackage);
    assert_eq!(kind(&package_with(vec![Item::stored("a", b"1").with_method(12)]), &d), ErrorKind::NotAPackage);
    assert_eq!(kind(&package_with(vec![Item::stored("a", b"1").with_bad_crc()]), &d), ErrorKind::Damaged);
    assert_eq!(kind(&package_with(vec![Item::stored("../a", b"1")]), &d), ErrorKind::Damaged);
    assert_eq!(kind(&package_with(vec![Item::stored("a", &[0; 200])]), &Limits { max_part_bytes: 100, ..Limits::default() }), ErrorKind::TooBig);
    assert_eq!(kind(&rows_only(1000, 0), &Limits { max_parts: 10, ..Limits::default() }), ErrorKind::TooBig);
    // A count with a bit flipped is damage, not "too big" (finding 13).
    let mut flipped = package_with(vec![Item::stored("a", b"1")]);
    let at = flipped.len() - 22;
    flipped[at + 11] |= 0x80;
    assert_eq!(kind(&flipped, &d), ErrorKind::Damaged);
}

#[test]
fn a_relationships_part_with_no_source_part_is_kept_and_reported() {
    let rels = rels_xml(&[("rId1", "urn:t", "x.xml", None)]);
    let mut items = base_items();
    items.extend([
        Item::stored("_rels/.rels", rels.as_bytes()), // the package's own: its source always exists
        Item::stored("visio/document.xml", b"<d/>"),
        Item::stored("visio/_rels/document.xml.rels", rels.as_bytes()), // its source is here
        Item::stored("visio/_rels/missing.xml.rels", rels.as_bytes()),  // its source is not
        Item::stored("visio/_rels/.rels", rels.as_bytes()),             // a folder's own: no part has an empty name
        Item::stored("other/_RELS/Gone.xml.RELS", rels.as_bytes()),     // any ASCII case
        Item::stored("notes/data.rels", rels.as_bytes()),               // not in a folder called _rels: a part like another
    ]);
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    let orphans: Vec<&str> =
        p.warnings().iter().filter_map(|w| if let Warning::OrphanRelationships { part } = w { Some(part.as_str()) } else { None }).collect();
    assert_eq!(orphans, ["/visio/_rels/missing.xml.rels", "/visio/_rels/.rels", "/other/_RELS/Gone.xml.RELS"]);
    // All of them are parts, kept, and their relationships can be asked for.
    assert!(p.part("/visio/_rels/missing.xml.rels").is_some());
    assert_eq!(p.rels("/visio/missing.xml").unwrap().len(), 1);
    // And the source of a relationships part is a name, which is what the report is made from.
    assert_eq!(rels_source("/_rels/.rels").as_deref(), Some("/"));
    assert_eq!(rels_source("/visio/_rels/document.xml.rels").as_deref(), Some("/visio/document.xml"));
    assert_eq!(rels_source("/a/b/_rels/c.d.rels").as_deref(), Some("/a/b/c.d"));
    assert_eq!(rels_source("/visio/_rels/.rels").as_deref(), Some("/visio/"));
    assert_eq!(rels_source("/visio/document.xml.rels"), None);
    assert_eq!(rels_source("/_rels/x.xml"), None);
    assert_eq!(rels_source("/x.rels"), None);
}
