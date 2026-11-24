//! The bounded XML tree as the container gives it to any package: the names later tasks use (`Tree`, `Node`, `parse`, `to_bytes`), an
//! ODF part read and written back with its prefixes, declarations and white space, and the refusals with their classes. The `opc` crate's
//! `xml_tree.rs` and `xml_hostile.rs` test the same code in depth, through `opc`.

use diacraft_container::xml::{
    Node, ParseOptions, ParseSize, Tree, XmlError, parse, parse_size, parse_with, parse_with_options, split_document, to_bytes,
};
use diacraft_container::{ContainerError, Work, XmlLimits};

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" office:version=\"1.4\">\n <office:styles>\n  <draw:marker draw:name=\"Arrow\"/>\n  <!-- kept -->\n </office:styles>\n</office:document-styles>\n";

#[test]
fn an_odf_part_reads_as_a_tree_and_writes_back_as_it_was_read() {
    let root: Tree = parse(STYLES.as_bytes()).unwrap();
    assert_eq!(root.name, "office:document-styles");
    assert_eq!((root.prefix(), root.local()), ("office", "document-styles"));
    assert_eq!(root.attr("office:version"), Some("1.4"));
    assert_eq!(root.attrs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["xmlns:office", "xmlns:draw", "office:version"]);
    let styles = root.child("styles").unwrap();
    assert_eq!(styles.child("marker").and_then(|m| m.attr_local("name")), Some("Arrow"));
    assert!(styles.children.iter().any(|n| matches!(n, Node::Comment(c) if c == " kept ")));
    // The declaration and the line after the root are outside the tree: the bytes keep them, and the writer puts them back.
    let (start, end) = split_document(STYLES.as_bytes()).unwrap();
    let written = to_bytes(&root, &STYLES.as_bytes()[..start], &STYLES.as_bytes()[end..], true).unwrap();
    assert_eq!(String::from_utf8(written).unwrap(), STYLES);
}

#[test]
fn what_is_refused_has_its_class() {
    let class = |text: &str| parse(text.as_bytes()).unwrap_err().class();
    assert_eq!(class("<a><b></a>"), "xml_malformed");
    assert_eq!(class("<!DOCTYPE a [<!ENTITY x \"y\">]><a/>"), "xml_entity_refused");
    assert_eq!(class("<a>&x;</a>"), "xml_entity_refused");
    let shallow = XmlLimits { max_depth: 2, ..XmlLimits::default() };
    assert_eq!(parse_with(b"<a><b><c/></b></a>", &shallow).unwrap_err(), XmlError::TooDeep { limit: 2 });
    // An XML error of a part the caller parses, as the container's error: the class is the XML one.
    let error = ContainerError::Xml { part: "content.xml".into(), error: parse(b"<a>").unwrap_err() };
    assert_eq!(error.class(), "xml_malformed");
    assert!(error.to_string().starts_with("\"content.xml\": XML is not well-formed"), "{error}");
    // The XML error is the cause the error chain gives, and an error of the archive has none.
    let source = std::error::Error::source(&error).map(ToString::to_string);
    assert_eq!(source, Some(parse(b"<a>").unwrap_err().to_string()));
    assert!(std::error::Error::source(&ContainerError::NotZip).is_none());
}

/// The class of what parsing `text` with the external DOCTYPE allowed gives, or `"read"`.
fn with_doctype(text: &str) -> &'static str {
    let options = ParseOptions { allow_external_doctype: true };
    match parse_with_options(text.as_bytes(), &XmlLimits::default(), options, &mut Work::default()) {
        Ok(_) => "read",
        Err(e) => e.class(),
    }
}

#[test]
fn a_doctype_of_an_external_id_alone_is_read_only_when_asked_for() {
    // The form an old OpenDocument writer put in its manifests, and the SYSTEM form, read and are ignored; by default they are
    // refused as every DTD is.
    let office = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\"/>";
    assert_eq!(with_doctype(office), "read");
    let tree =
        parse_with_options(office.as_bytes(), &XmlLimits::default(), ParseOptions { allow_external_doctype: true }, &mut Work::default()).unwrap();
    assert_eq!(tree.name, "manifest:manifest");
    assert_eq!(parse(office.as_bytes()).unwrap_err().class(), "xml_entity_refused", "off by default");
    assert_eq!(with_doctype("<!DOCTYPE m SYSTEM \"m.dtd\"><m/>"), "read");
    assert_eq!(with_doctype("<!DOCTYPE m SYSTEM 'm.dtd' ><!-- a note --><m/>"), "read");
    assert_eq!(with_doctype("<!-- first -->\n<!DOCTYPE m PUBLIC '-//A//B' \"m.dtd\">\n<m/>"), "read");
    // An internal subset, alone or after an external id, is still refused; so is a DOCTYPE with no external id, a second one, one after
    // the root, one before the declaration, and one whose public id holds a character a public id may not.
    for refused in [
        "<!DOCTYPE m SYSTEM \"m.dtd\" [ <!ENTITY x \"y\"> ]><m/>",
        "<!DOCTYPE m [ <!ENTITY x \"y\"> ]><m/>",
        "<!DOCTYPE m><m/>",
        "<!DOCTYPE m SYSTEM \"a\"><!DOCTYPE m SYSTEM \"b\"><m/>",
        "<!DOCTYPE m PUBLIC \"-//A//B\"><m/>",
        "<!DOCTYPE m PUBLIC \"a{b\" \"m.dtd\"><m/>",
        "<!DOCTYPE m SYSTEM m.dtd><m/>",
        "<!DOCTYPE m SYSTEMx \"m.dtd\"><m/>",
        "<!DOCTYPE m SYSTEM \"m.dtd\" junk><m/>",
        "<!DOCTYPE 1m SYSTEM \"m.dtd\"><m/>",
    ] {
        assert_eq!(with_doctype(refused), "xml_entity_refused", "{refused}");
    }
    assert_ne!(with_doctype("<m/><!DOCTYPE m SYSTEM \"m.dtd\">"), "read", "after the root");
    assert_ne!(with_doctype("<!DOCTYPE m SYSTEM \"m.dtd\"><?xml version=\"1.0\"?><m/>"), "read", "before the declaration");
    // Nothing is declared by it: a reference to an entity is refused after it as before.
    assert_eq!(with_doctype("<!DOCTYPE m SYSTEM \"m.dtd\"><m>&x;</m>"), "xml_entity_refused");
    assert_eq!(with_doctype("<!DOCTYPE m SYSTEM \"m.dtd\"><m>&amp;</m>"), "read");
    // The keyword in another case, or with no space after it, is not XML (production 28): not well-formed when a DOCTYPE is asked for,
    // and a DTD refused as before when it is not.
    for misspelled in ["<!doctype m SYSTEM \"x\"><m/>", "<!DocType m SYSTEM \"x\"><m/>", "<!DOCTYPEm SYSTEM \"x\"><m/>"] {
        assert_eq!(with_doctype(misspelled), "xml_malformed", "{misspelled}");
        assert_eq!(parse(misspelled.as_bytes()).unwrap_err().class(), "xml_entity_refused", "{misspelled}");
    }
    assert_eq!(with_doctype("<!DOCTYPE\tm SYSTEM \"x\"><m/>"), "read", "a tab is a space");
    // An accepted DOCTYPE is held to XML's characters like any other text.
    assert_eq!(with_doctype("<!DOCTYPE m SYSTEM \"m\u{1}.dtd\"><m/>"), "xml_malformed");
}

/// UTF-16 with a byte order mark, little endian (`be`: big endian) or none (the document then begins `<?`).
fn utf16(text: &str, mark: bool, be: bool) -> Vec<u8> {
    let mut out = match (mark, be) {
        (false, _) => Vec::new(),
        (true, true) => vec![0xFE, 0xFF],
        (true, false) => vec![0xFF, 0xFE],
    };
    for unit in text.encode_utf16() {
        out.extend_from_slice(&if be { unit.to_be_bytes() } else { unit.to_le_bytes() });
    }
    out
}

#[test]
fn a_line_break_written_as_cr_lf_or_cr_reads_as_lf_and_every_string_it_makes_holds_no_more_than_it_says() {
    // A text, a comment, a CDATA section and an instruction, each with a CR LF, a lone CR and a LF written: one line feed for each break, and a string of the length it
    // ends with (the text was made twice by `replace`, each time grown by doubling, and copied again to be kept).
    let doc = "<a>x\r\ny\rz\nw\r\n</a>";
    let tree = parse(doc.as_bytes()).unwrap();
    assert!(matches!(tree.children.as_slice(), [Node::Text(t)] if t == "x\ny\nz\nw\n" && t.capacity() == t.len()), "{:?}", tree.children);
    let doc = "<a><!--c\r\nd\re--><![CDATA[f\r\ng\rh]]><?p  i\r\nj\rk?></a>";
    let tree = parse(doc.as_bytes()).unwrap();
    match tree.children.as_slice() {
        [Node::Comment(c), Node::CData(d), Node::Pi { target, data }] => {
            assert_eq!((c.as_str(), d.as_str(), target.as_str(), data.as_str()), ("c\nd\ne", "f\ng\nh", "p", "i\nj\nk"));
            assert!([c, d, data].iter().all(|s| s.capacity() == s.len()), "{c:?} {d:?} {data:?}");
        }
        other => panic!("{other:?}"),
    }
    // A text that has no CR is the text as it was, and a reference to CR stays one CR (it is no literal break).
    let tree = parse("<a>caf\u{e9}</a>".as_bytes()).unwrap();
    assert!(matches!(tree.children.as_slice(), [Node::Text(t)] if t == "caf\u{e9}" && t.capacity() == t.len()), "{:?}", tree.children);
    let tree = parse(b"<a>p&#13;q\r\nr</a>").unwrap();
    assert!(matches!(tree.children.as_slice(), [Node::Text(t)] if t == "p\rq\nr"), "{:?}", tree.children);
}

#[test]
fn what_a_parse_holds_besides_its_tree_is_told_from_the_first_bytes_and_the_length() {
    let size = |text_bytes: u64, copy_bytes: u64| ParseSize { text_bytes, copy_bytes };
    // UTF-8 is read where it is: the bytes are the text, and nothing is copied. A byte order mark and bytes that are no text change nothing of it.
    assert_eq!(parse_size(b"<a/>"), size(4, 0));
    assert_eq!(parse_size(&[0xEF, 0xBB, 0xBF, b'<', b'a', b'/', b'>']), size(7, 0));
    assert_eq!(parse_size(b""), size(0, 0));
    assert_eq!(parse_size(&[0xFF, 0xFE, 0, 0, 1, 2]), size(6, 0), "UTF-32 is refused by the parse, before it makes anything");
    // UTF-16 is decoded into a copy, which is up to three bytes of UTF-8 for each two bytes: the copy and the text are the same bytes.
    let cjk = "<a>\u{4E00}\u{4E01}\u{4E02}\u{4E03}</a>";
    let short = "<?xml version=\"1.0\"?><a>\u{4E00}\u{4E01}</a>";
    for (bytes, name) in [
        (utf16(cjk, true, false), "little endian with a mark"),
        (utf16(cjk, true, true), "big endian with a mark"),
        (utf16(short, false, false), "little endian with no mark"),
        (utf16(short, false, true), "big endian with no mark"),
    ] {
        let body = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) { bytes.len() - 2 } else { bytes.len() } as u64;
        assert_eq!(parse_size(&bytes), size(body * 3 / 2, body * 3 / 2), "{name}");
    }
    // The bound holds: the text of a document of characters that are three bytes in UTF-8 and two in UTF-16 is exactly what it says, and none of another kind is more.
    for text in [cjk, "<a>caf\u{e9} \u{1F600}</a>", "<a>plain</a>"] {
        let bytes = utf16(text, true, false);
        assert!(parse_size(&bytes).text_bytes >= text.len() as u64, "{text}");
    }
    // A unit is counted as three bytes whatever it is: eleven units, so 33 bytes, for a document that is 19 bytes of UTF-8.
    assert_eq!(parse_size(&utf16(cjk, true, false)).text_bytes, 33);
}

#[test]
fn a_document_in_utf16_is_read_and_its_strings_are_the_size_they_hold() {
    let doc = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a k=\"\u{4E00}\u{1F600}v\">\u{4E01}\r\n\u{4E02}</a>";
    for (bytes, name) in [
        (utf16(doc, true, false), "le"),
        (utf16(doc, true, true), "be"),
        (utf16(doc, false, false), "le without a mark"),
        (utf16(doc, false, true), "be without a mark"),
    ] {
        let tree = parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(tree.attr("k"), Some("\u{4E00}\u{1F600}v"), "{name}");
        assert!(
            matches!(tree.children.as_slice(), [Node::Text(t)] if t == "\u{4E01}\n\u{4E02}" && t.capacity() == t.len()),
            "{name}: {:?}",
            tree.children
        );
    }
    // An unpaired surrogate and half a unit at the end are refused, as before.
    let mut lone = utf16("<a>x</a>", true, false);
    lone.splice(8..8, [0x00, 0xD8]);
    assert_eq!(parse(&lone).unwrap_err().class(), "xml_malformed");
    let mut half = utf16("<a/>", true, false);
    half.push(0);
    assert_eq!(parse(&half).unwrap_err().class(), "xml_malformed");
}

#[test]
fn text_outside_the_root_element_is_refused_and_blanks_are_not() {
    // Text before the root or after it is not well-formed, a reference to a character outside it is not either, and white space is (the line breaks of a document).
    for refused in ["x<a/>", "<a/>x", "<a/>&amp;", "&amp;<a/>", "<a/>\u{e9}", "<?xml version=\"1.0\"?>x<a/>"] {
        assert_eq!(parse(refused.as_bytes()).unwrap_err().class(), "xml_malformed", "{refused:?}");
    }
    for read in [" <a/> ", "\r\n<a/>\r\n", "\t<a/>\n", "<?xml version=\"1.0\"?>\n<a/>\n"] {
        assert!(parse(read.as_bytes()).is_ok(), "{read:?}");
    }
}
