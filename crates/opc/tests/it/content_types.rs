//! `[Content_Types].xml`: how a part's type is found, and what the reader keeps of the rows.

use diacraft_opc::{Limits, Package, Warning};

use crate::zipbuild::{Item, build};
use crate::{class_of, content_types_xml};

fn package(ct: &str, more: Vec<Item>) -> Package {
    let mut items = vec![Item::deflated("[Content_Types].xml", ct.as_bytes())];
    items.extend(more);
    Package::read(&build(&items), &Limits::default()).unwrap()
}

fn class(ct: &str) -> &'static str {
    class_of(&build(&[Item::deflated("[Content_Types].xml", ct.as_bytes())]), &Limits::default())
}

#[test]
fn an_override_beats_a_default_and_both_ignore_ascii_case() {
    // `Default Extension="XML"` and `Override PartName="/Visio/Document.xml"`.
    let ct = content_types_xml(&[("XML", "application/xml")], &[("/Visio/Document.xml", "application/vnd.ms-visio.drawing.main+xml")]);
    let p = package(&ct, vec![Item::stored("visio/document.xml", b"<d/>"), Item::stored("visio/other.xml", b"<o/>")]);
    assert_eq!(p.content_type("/visio/document.xml"), Some("application/vnd.ms-visio.drawing.main+xml"));
    assert_eq!(p.content_type("/visio/other.xml"), Some("application/xml"));
    // The same answers whatever the case of the question, and with or without the slash.
    assert_eq!(p.content_type("VISIO/DOCUMENT.XML"), Some("application/vnd.ms-visio.drawing.main+xml"));
    assert_eq!(p.content_type("/VISIO/OTHER.Xml"), Some("application/xml"));
}

#[test]
fn a_type_is_given_for_a_name_that_is_not_a_part() {
    // V3 asks for the type of `/visio/other.xml` as a question about a name; the part need not exist.
    let ct = content_types_xml(&[("xml", "application/xml")], &[("/visio/ghost.xml", "application/x-ghost")]);
    let p = package(&ct, vec![]);
    assert_eq!(p.content_type("/visio/other.xml"), Some("application/xml"));
    assert_eq!(p.content_type("/visio/ghost.xml"), Some("application/x-ghost"));
}

#[test]
fn a_part_with_no_row_that_matches_has_no_type() {
    // No Override, and a Default for no extension of its last segment: nothing. No guess.
    let ct = content_types_xml(&[("xml", "application/xml")], &[]);
    let p = package(&ct, vec![]);
    assert_eq!(p.content_type("/visio/image1.png"), None);
    assert_eq!(p.content_type("/visio/noextension"), None);
    assert_eq!(p.content_type("/visio.xml/noextension"), None, "the extension is of the last segment: a dot in a folder is not one");
    assert_eq!(p.content_type("/"), None);
    assert_eq!(p.content_type(""), None);
}

#[test]
fn the_extension_is_the_text_after_the_last_dot_of_the_last_segment() {
    let ct = content_types_xml(
        &[("rels", "application/vnd.openxmlformats-package.relationships+xml"), ("gz", "application/gzip"), ("bin", "application/x-bin")],
        &[],
    );
    let p = package(&ct, vec![]);
    assert_eq!(p.content_type("/_rels/.rels"), Some("application/vnd.openxmlformats-package.relationships+xml"), "`.rels` has the extension rels");
    assert_eq!(p.content_type("/visio/_rels/document.xml.rels"), Some("application/vnd.openxmlformats-package.relationships+xml"));
    assert_eq!(p.content_type("/a.tar.gz"), Some("application/gzip"));
    assert_eq!(p.content_type("/a.gz.bin"), Some("application/x-bin"));
}

#[test]
fn a_row_for_a_part_that_is_not_there_is_kept() {
    // vsdx-preservation V2: `<Default Extension="zzz" .../>` with no `.zzz` part is kept; so is an Override for no part.
    let ct = content_types_xml(&[("zzz", "application/x-z")], &[("/nowhere.xml", "application/x-nowhere")]);
    let p = package(&ct, vec![Item::stored("a.xml", b"<a/>")]);
    let types = p.content_types();
    assert_eq!(types.defaults, [("zzz".to_owned(), "application/x-z".to_owned())]);
    assert_eq!(types.overrides, [("/nowhere.xml".to_owned(), "application/x-nowhere".to_owned())]);
}

#[test]
fn rows_are_kept_in_file_order_with_the_case_they_were_written_in() {
    let ct = content_types_xml(&[("XML", "application/xml"), ("Rels", "x/rels"), ("png", "image/png")], &[("/B.xml", "t/b"), ("/a.xml", "t/a")]);
    let p = package(&ct, vec![]);
    let defaults: Vec<&str> = p.content_types().defaults.iter().map(|(e, _)| e.as_str()).collect();
    let overrides: Vec<&str> = p.content_types().overrides.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(defaults, ["XML", "Rels", "png"]);
    assert_eq!(overrides, ["/B.xml", "/a.xml"]);
}

#[test]
fn defaults_and_overrides_may_come_in_any_order() {
    // "in any order".
    let ct = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Override PartName=\"/a.xml\" ContentType=\"t/a\"/><Default Extension=\"xml\" ContentType=\"t/x\"/><Override PartName=\"/b.xml\" ContentType=\"t/b\"/></Types>";
    let p = package(ct, vec![]);
    assert_eq!(p.content_type("/a.xml"), Some("t/a"));
    assert_eq!(p.content_type("/b.xml"), Some("t/b"));
    assert_eq!(p.content_type("/c.xml"), Some("t/x"));
}

#[test]
fn a_second_row_for_the_same_extension_or_part_is_reported_and_the_first_wins() {
    // R6 allows one of each. A reader that took the last would let a trailing row change what an earlier part is.
    let ct = content_types_xml(&[("xml", "t/first"), ("XML", "t/second")], &[("/a.xml", "t/o-first"), ("/A.XML", "t/o-second")]);
    let p = package(&ct, vec![]);
    assert_eq!(p.content_type("/other.xml"), Some("t/first"));
    assert_eq!(p.content_type("/a.xml"), Some("t/o-first"));
    assert!(p.warnings().contains(&Warning::DuplicateDefault { extension: "XML".to_owned() }), "{:?}", p.warnings());
    assert!(p.warnings().contains(&Warning::DuplicateOverride { part_name: "/A.XML".to_owned() }), "{:?}", p.warnings());
    // The rows themselves are all kept, as written.
    assert_eq!(p.content_types().defaults.len(), 2);
    assert_eq!(p.content_types().overrides.len(), 2);
}

#[test]
fn a_row_without_its_attributes_is_skipped_and_reported() {
    let ct = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\"/><Default ContentType=\"t/x\"/><Override PartName=\"/a.xml\"/><Override ContentType=\"t/x\"/><Default Extension=\"ok\" ContentType=\"t/ok\"/></Types>";
    let p = package(ct, vec![]);
    assert_eq!(p.content_types().defaults, [("ok".to_owned(), "t/ok".to_owned())]);
    assert!(p.content_types().overrides.is_empty());
    let skipped = p.warnings().iter().filter(|w| matches!(w, Warning::IncompleteContentTypeEntry { .. })).count();
    assert_eq!(skipped, 4, "{:?}", p.warnings());
}

#[test]
fn an_override_part_name_is_decoded_like_an_item_name() {
    let ct = content_types_xml(&[], &[("/visio/media/image%201.png", "image/png"), ("visio/no-slash.xml", "t/ns")]);
    let p = package(&ct, vec![]);
    assert_eq!(p.content_type("/visio/media/image 1.png"), Some("image/png"));
    assert_eq!(p.content_type("/visio/no-slash.xml"), Some("t/ns"), "a name without its slash is read as one with it");
}

#[test]
fn rows_of_other_kinds_and_text_inside_types_are_ignored_not_fatal() {
    let ct = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n  <!-- c -->\n  <Unknown X=\"1\"/>text<Default Extension=\"xml\" ContentType=\"t/x\"/></Types>";
    let p = package(ct, vec![]);
    assert_eq!(p.content_type("/a.xml"), Some("t/x"));
}

#[test]
fn the_namespace_of_the_root_is_not_checked_only_its_name() {
    // Labelled choice: the reader matches by local name, as the rest of this crate does.
    let ct = "<t:Types xmlns:t=\"urn:something-else\"><t:Default Extension=\"xml\" ContentType=\"t/x\"/></t:Types>";
    let p = package(ct, vec![]);
    assert_eq!(p.content_type("/a.xml"), Some("t/x"));
}

#[test]
fn a_content_types_stream_that_is_not_well_formed_refuses_the_package() {
    assert_eq!(class("<Types><Default Extension=\"xml\" ContentType=\"t/x\"></Types>"), "xml_malformed");
    assert_eq!(class(""), "xml_malformed");
    assert_eq!(class("not xml at all"), "xml_malformed");
    assert_eq!(class("<Overrides/>"), "xml_malformed", "the root must be Types");
    assert_eq!(class("<Types/><Types/>"), "xml_malformed");
}

#[test]
fn a_dtd_or_entity_in_the_content_types_stream_is_refused() {
    // For the stream the reader parses itself.
    assert_eq!(class("<!DOCTYPE Types [<!ENTITY a \"b\">]><Types/>"), "xml_entity_refused");
    assert_eq!(class("<!DOCTYPE Types><Types/>"), "xml_entity_refused");
    assert_eq!(class("<Types a=\"&a;\"/>"), "xml_entity_refused");
}

#[test]
fn the_error_for_a_bad_stream_names_the_stream() {
    let err = Package::read(&build(&[Item::stored("[Content_Types].xml", b"<Types>")]), &Limits::default()).unwrap_err();
    assert!(err.to_string().contains("[Content_Types].xml"), "{err}");
}

#[test]
fn an_empty_types_element_is_a_stream_with_no_rows() {
    let p = package("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>", vec![Item::stored("a.xml", b"<a/>")]);
    assert_eq!(p.content_type("/a.xml"), None);
    assert_eq!(p.parts().count(), 1);
}

#[test]
fn the_rows_limit_holds_at_its_boundary() {
    let limits = Limits { max_content_type_entries: 5, ..Limits::default() };
    let rows = |n: usize| {
        let defaults: Vec<(String, String)> = (0..n).map(|i| (format!("e{i}"), "t/x".to_owned())).collect();
        let d: Vec<(&str, &str)> = defaults.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        content_types_xml(&d, &[])
    };
    let at = build(&[Item::stored("[Content_Types].xml", rows(5).as_bytes())]);
    assert_eq!(Package::read(&at, &limits).unwrap().content_types().defaults.len(), 5);
    let over = build(&[Item::stored("[Content_Types].xml", rows(6).as_bytes())]);
    assert_eq!(class_of(&over, &limits), "too_many_content_type_entries");
    // Skipped rows count too: a stream of a million empty `Default` elements is as large as one of full rows.
    let junk = format!("<Types>{}</Types>", "<Default/>".repeat(6));
    assert_eq!(class_of(&build(&[Item::stored("[Content_Types].xml", junk.as_bytes())]), &limits), "too_many_content_type_entries");
}

#[test]
fn the_stream_size_limit_holds_at_its_boundary() {
    let xml = content_types_xml(&[("xml", "t/x")], &[]);
    let n = xml.len() as u64;
    let at = Limits { max_package_xml_bytes: n, ..Limits::default() };
    let over = Limits { max_package_xml_bytes: n - 1, ..Limits::default() };
    let bytes = build(&[Item::stored("[Content_Types].xml", xml.as_bytes())]);
    assert!(Package::read(&bytes, &at).is_ok());
    assert_eq!(class_of(&bytes, &over), "part_too_large");
}

#[test]
fn the_item_limit_of_the_stream_holds_at_its_boundary_and_is_the_smaller_of_two() {
    // `<Types><Default .../></Types>` is the root, the row and its two attributes: 4 items.
    let ct = "<Types><Default Extension=\"a\" ContentType=\"b\"/></Types>";
    let bytes = build(&[Item::stored("[Content_Types].xml", ct.as_bytes())]);
    assert!(Package::read(&bytes, &Limits { max_package_xml_nodes: 4, ..Limits::default() }).is_ok());
    assert_eq!(class_of(&bytes, &Limits { max_package_xml_nodes: 3, ..Limits::default() }), "xml_too_complex");
    // The item limit of every XML tree holds too, when it is the smaller.
    let both = Limits { xml: diacraft_opc::XmlLimits { max_nodes: 3, ..Default::default() }, ..Limits::default() };
    assert_eq!(class_of(&bytes, &both), "xml_too_complex");
}

#[test]
fn the_stream_comes_back_as_inflated_whatever_its_layout() {
    // A writer that changes no row wants the stream back as it was. Every layout a writer may have used: the
    // declaration with and without a line end and in either quote, white space between rows, a comment, single-quoted attributes, a byte order
    // mark, no declaration, an element written open and shut, and a row the reader does not count.
    let layouts: [&[u8]; 6] = [
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>",
        b"<?xml version='1.0' encoding='UTF-8'?>\r\n<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'>\r\n  <!-- a comment -->\r\n  <Default Extension='xml' ContentType='application/xml'/>\r\n</Types>\r\n",
        b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\t<Default Extension=\"xml\" ContentType=\"application/xml\"></Default>\n</Types>",
        b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>\n\n",
        b"<ct:Types xmlns:ct=\"http://schemas.openxmlformats.org/package/2006/content-types\"><ct:Default Extension=\"xml\" ContentType=\"application/xml\"/></ct:Types>",
        b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\"/><Other/><Override PartName=\"/a.xml\" ContentType=\"x/y\"/></Types>",
    ];
    for (n, stream) in layouts.iter().enumerate() {
        let items = [Item::deflated("[Content_Types].xml", stream), Item::stored("a.xml", b"<a/>")];
        let p = Package::read(&build(&items), &Limits::default()).unwrap();
        assert_eq!(p.content_types_bytes(), *stream, "layout {n}");
    }
    // The rows are read from them all the same.
    let p = package(std::str::from_utf8(layouts[1]).unwrap(), vec![]);
    assert_eq!(p.content_types().defaults, [("xml".to_owned(), "application/xml".to_owned())]);
}

#[test]
fn an_external_doctype_that_odf_manifests_may_carry_is_still_refused_here() {
    // The ODF package reader accepts, in its manifest alone, a DOCTYPE of an external id and no internal subset (of the ODF
    // plan). An OPC package must not contain a DTD at all: the same form in the content types stream, or in a relationships part, is
    // refused.
    let doctype = "<!DOCTYPE Types PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">";
    let ct = content_types_xml(&[("xml", "application/xml"), ("rels", "application/vnd.openxmlformats-package.relationships+xml")], &[]);
    let with_doctype = ct.replacen("?>", &format!("?>{doctype}"), 1);
    assert_ne!(with_doctype, ct, "the DOCTYPE is in the stream");
    assert_eq!(class(&with_doctype), "xml_entity_refused");
    let rels = format!(
        "<?xml version=\"1.0\"?>{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
        doctype.replace("Types", "Relationships")
    );
    let items = vec![Item::deflated("[Content_Types].xml", ct.as_bytes()), Item::deflated("_rels/.rels", rels.as_bytes())];
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.rels("/").unwrap_err().class(), "xml_entity_refused");
}
