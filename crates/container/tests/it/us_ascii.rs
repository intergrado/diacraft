//! `ParseOptions::allow_us_ascii`: a document that declares `US-ASCII` is read as UTF-8 when it asks for it and
//! every byte of it is below 0x80, and is refused, as every encoding but UTF-8 and UTF-16 always was, in every other case. Off unless asked for.

use diacraft_container::xml::{ParseOptions, PullLimits, XmlError, XmlText, parse_with_options};
use diacraft_container::{Work, XmlLimits};

fn asked() -> ParseOptions {
    ParseOptions { allow_us_ascii: true, ..ParseOptions::default() }
}

fn parsed(bytes: &[u8], options: ParseOptions) -> Result<String, XmlError> {
    parse_with_options(bytes, &XmlLimits::default(), options, &mut Work::default()).map(|root| root.name)
}

fn message(e: &XmlError) -> String {
    e.to_string()
}

#[test]
fn us_ascii_is_read_only_when_asked_and_only_when_every_byte_is_ascii() {
    let ascii = b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a b=\"c\">text &amp; more</a>";
    // Off by default: the declaration is refused as it always was.
    let refused = parsed(ascii, ParseOptions::default()).unwrap_err();
    assert_eq!(refused.class(), "xml_malformed");
    assert!(message(&refused).contains("declared encoding"), "{}", message(&refused));
    assert_eq!(parsed(ascii, ParseOptions { allow_external_doctype: true, ..ParseOptions::default() }).unwrap_err().class(), "xml_malformed");
    // Asked for, and every byte below 0x80: read.
    assert_eq!(parsed(ascii, asked()).unwrap(), "a");
    // The name of the encoding is matched as every other is: in any case, with the space the declaration may hold.
    for declared in ["us-ascii", "Us-AsCii", "US-ASCII"] {
        let doc = format!("<?xml version=\"1.0\" encoding=\"{declared}\"?><a/>");
        assert_eq!(parsed(doc.as_bytes(), asked()).unwrap(), "a", "{declared}");
        assert!(parsed(doc.as_bytes(), ParseOptions::default()).is_err(), "{declared}: off by default");
    }
    // A byte at or above 0x80 anywhere in the document, in text, in a value, in a comment or a name: refused as before, though it was asked for.
    let outside = [
        "<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a>caf\u{e9}</a>",
        "<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a b=\"\u{e9}\"/>",
        "<?xml version=\"1.0\" encoding=\"US-ASCII\"?><!-- \u{4E00} --><a/>",
        "<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a/><!-- \u{1F600} -->",
    ];
    for doc in outside {
        let e = parsed(doc.as_bytes(), asked()).unwrap_err();
        assert_eq!(e.class(), "xml_malformed", "{doc:?}");
        assert!(message(&e).contains("declared encoding"), "{doc:?}: {}", message(&e));
    }
    // The same text with that byte left out is read.
    assert_eq!(parsed(b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a>caf</a>", asked()).unwrap(), "a");
}

#[test]
fn asking_for_us_ascii_changes_no_other_refusal() {
    // Other encodings are still refused, UTF-8 and UTF-16 are still read, and a document that declares none reads as before. `US-ASCII-X` is a name that only starts with
    // the accepted one; its tail is a letter, as a tail of digits in a string of a test is taken for an id by the tool that makes the public tree, which cuts it off.
    for declared in ["ISO-8859-1", "windows-1252", "ASCII", "US-ASCII-X", "UTF-32"] {
        let doc = format!("<?xml version=\"1.0\" encoding=\"{declared}\"?><a/>");
        assert_eq!(parsed(doc.as_bytes(), asked()).unwrap_err().class(), "xml_malformed", "{declared}");
    }
    assert_eq!(parsed(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><a>\xc3\xa9</a>", asked()).unwrap(), "a");
    assert_eq!(parsed(b"<a>\xc3\xa9</a>", asked()).unwrap(), "a");
    // UTF-16 bytes that declare US-ASCII are not US-ASCII.
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a/>".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(parsed(&utf16, asked()).unwrap_err().class(), "xml_malformed");
    // A DTD is refused whatever else was asked for.
    let dtd = b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?><!DOCTYPE a [<!ENTITY x \"y\">]><a/>";
    assert_eq!(parsed(dtd, asked()).unwrap_err().class(), "xml_entity_refused");
}

#[test]
fn a_stream_reads_us_ascii_as_a_tree_does() {
    let doc = b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a><b/></a>";
    let limits = PullLimits { xml: XmlLimits::default(), max_document_nodes: usize::MAX };
    let text = XmlText::new(doc).unwrap();
    assert!(text.pull(&limits, ParseOptions::default()).is_err());
    let (mut pull, head) = text.pull(&limits, asked()).unwrap();
    assert_eq!(head.name, "a");
    assert_eq!(pull.next_child().unwrap().map(|e| e.name), Some("b".to_owned()));
    let non_ascii = XmlText::new("<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a>\u{e9}</a>".as_bytes()).unwrap();
    assert!(non_ascii.pull(&limits, asked()).is_err());
}
