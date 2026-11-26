//! How the reader recognises a file (the design's §1.9): by its first bytes and never by its name, looking at no more than
//! 64 KiB before the name of the root, and claiming only what it can read, so that a VSDX, an `.odg` or a `.diacraft` file is never read as a drawing.

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_mxfile::limits::MAX_PROLOG_BYTES;
use diacraft_mxfile::sniff::sniff_counted;
use diacraft_mxfile::{ErrorKind, Kind, Limits, read, sniff};

use crate::name0;

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn utf16(text: &str, little: bool, mark: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if mark {
        out.extend(if little { [0xFF, 0xFE] } else { [0xFE, 0xFF] });
    }
    out.extend(text.encode_utf16().flat_map(|u| if little { u.to_le_bytes() } else { u.to_be_bytes() }));
    out
}

/// The refusal reading `bytes` gives.
fn refusal(bytes: &[u8]) -> (ErrorKind, &'static str, String) {
    let e = read(bytes, &Limits::default()).expect_err("not read");
    (e.kind, e.class, e.words)
}

#[test]
fn the_png_signature_is_an_image_that_may_hold_a_drawing() {
    assert_eq!(sniff(&PNG), Some(Kind::Png));
    let mut more = PNG.to_vec();
    more.extend(b"\0\0\0\rIHDR");
    assert_eq!(sniff(&more), Some(Kind::Png));
    assert_eq!(sniff(&PNG[..7]), None, "seven bytes of the signature are not it");
    // Text that begins like it, and a signature that is not at the start.
    assert_eq!(sniff(b"\x89PNG\r\n\x1a\n<mxfile/>"), Some(Kind::Png), "the signature decides");
    assert_eq!(sniff(b" \x89PNG\r\n\x1a\n"), None);
}

#[test]
fn the_name_of_the_root_decides_after_a_mark_a_declaration_comments_and_white_space() {
    let roots: [(&str, Option<Kind>); 8] = [
        ("<mxfile/>", Some(Kind::Document)),
        ("<mxfile host=\"x\">", Some(Kind::Document)),
        ("<mxGraphModel>", Some(Kind::Page)),
        ("<mxGraphModel dx=\"1\">", Some(Kind::Page)),
        ("<diagram name=\"p\">", Some(Kind::Page)),
        ("<svg xmlns=\"http://www.w3.org/2000/svg\">", Some(Kind::Svg)),
        ("<mxlibrary>[]</mxlibrary>", Some(Kind::Library)),
        ("<html>", None),
    ];
    let before = [
        "",
        "\u{FEFF}",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "\u{FEFF}<?xml version=\"1.0\"?>\n",
        "<!-- a comment -->",
        "  \t\r\n",
        "<?xml version=\"1.0\"?>\n<!-- one -->\n<!--two--> \n\t<?pi data?>\n",
        "\u{FEFF}<!-- <mxfile> in a comment is no root -->",
    ];
    for (root, kind) in roots {
        for prefix in before {
            let doc = format!("{prefix}{root}");
            assert_eq!(sniff(doc.as_bytes()), kind, "{doc:?}");
        }
    }
}

#[test]
fn utf16_with_its_mark_is_claimed_as_utf8_is() {
    for little in [true, false] {
        for mark in [true, false] {
            let doc = utf16("<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<!-- c --><mxfile><diagram/></mxfile>", little, mark);
            assert_eq!(sniff(&doc), Some(Kind::Document), "little={little} mark={mark}");
        }
        let doc = utf16("<mxGraphModel/>", little, true);
        assert_eq!(sniff(&doc), Some(Kind::Page), "little={little}");
        assert_eq!(sniff(&utf16("<html/>", little, true)), None);
    }
    // UTF-32 is no encoding of this reader, and a half unit at the end of the part looked at is no harm.
    assert_eq!(sniff(&[0xFF, 0xFE, 0x00, 0x00, b'<', 0, 0, 0]), None);
    let mut odd = utf16("<mxfile/>", true, true);
    odd.push(0x3C);
    assert_eq!(sniff(&odd), Some(Kind::Document));
}

#[test]
fn a_root_that_is_not_one_of_the_five_is_not_claimed_and_neither_is_a_longer_name() {
    for doc in [
        &b"<html><body/></html>"[..],
        b"<!DOCTYPE html><html/>",
        b"<foo/>",
        b"<mxfiles/>",
        b"<mxFile/>",
        b"<mxfile",
        b"<mxfile",
        b"<mxGraphModels/>",
        b"<diagrams/>",
        b"<svgs/>",
        b"<mxlibraries/>",
        b"<x:svg/>",
        b"mxfile",
        b"",
        b"   ",
        b"<",
        b"<?xml version=\"1.0\"?>",
        b"<!-- never closed <mxfile/>",
        b"<?pi never closed <mxfile/>",
        b"{\"json\": true}",
        b"\0\0\0\0",
    ] {
        assert_eq!(sniff(doc), None, "{}", String::from_utf8_lossy(doc));
    }
    // An SVG with the standard DOCTYPE before it is an SVG, and one for HTML is not anything.
    assert_eq!(sniff(b"<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x.dtd\"><svg/>"), Some(Kind::Svg));
    assert_eq!(sniff(b"<!DOCTYPE mxfile [<!ENTITY a \"b\">]><mxfile/>"), Some(Kind::Document), "claimed, so that the read can refuse it in words");
}

#[test]
fn a_zip_is_not_mine_at_once_whatever_it_holds() {
    // A VSDX package, an `.odg` and a `.diacraft` file are ZIP archives: none begins with a root, so none is claimed, and the read says it is not a drawing.
    // The folder a VSDX package keeps its parts in, from the testkit's constant (a name no test of this crate spells).
    let document_part = format!("{}/document.xml", diacraft_testkit::package::DIR.trim_start_matches('/'));
    type Entries<'a> = Vec<(&'a str, &'a [u8])>;
    let packages: [(&str, Entries<'_>); 3] = [
        ("a VSDX", vec![("[Content_Types].xml", b"<Types/>"), (document_part.as_str(), b"<mxfile/>")]),
        ("an .odg", vec![("mimetype", b"application/vnd.oasis.opendocument.graphics"), ("content.xml", b"<mxfile/>")]),
        ("a .diacraft", vec![("mimetype", b"application/vnd.diacraft"), ("document.xml", b"<mxGraphModel/>")]),
    ];
    for (what, entries) in packages {
        let mut zip = ArchiveWriter::new();
        for (name, data) in entries {
            zip.add(name, data, Method::Stored).unwrap();
        }
        let bytes = zip.finish().unwrap();
        assert!(bytes.starts_with(b"PK"));
        assert_eq!(sniff(&bytes), None, "{what}");
        let (kind, class, _) = refusal(&bytes);
        assert_eq!((kind, class), (ErrorKind::NotMine, "drawio_not_a_drawing"), "{what}");
    }
    // The signature of an empty archive, and a stored entry that holds a drawing's text first.
    assert_eq!(sniff(b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"), None);
    assert_eq!(sniff(b"PK\x03\x04<mxfile/>"), None);
}

#[test]
fn no_more_than_64_kib_of_prolog_is_looked_at_and_a_root_that_comes_later_is_not_claimed() {
    assert_eq!(MAX_PROLOG_BYTES, 64 * 1024);
    let comment = |len: usize| format!("<!--{}-->", "c".repeat(len - 7));
    // A root whose start tag ends at the last byte of the window is claimed; one byte later, it is not.
    let root = "<mxfile>";
    let inside = format!("{}{root}", comment(MAX_PROLOG_BYTES - root.len()));
    assert_eq!(inside.len(), MAX_PROLOG_BYTES);
    assert_eq!(sniff(inside.as_bytes()), Some(Kind::Document));
    let outside = format!("{}{root}", comment(MAX_PROLOG_BYTES - root.len() + 1));
    assert_eq!(sniff(outside.as_bytes()), None);
    // 64 KiB of comment and then a root, and a hundred megabytes: the same, and the count says how much was looked at.
    let (kind, looked) = sniff_counted(inside.as_bytes());
    assert_eq!((kind, looked), (Some(Kind::Document), MAX_PROLOG_BYTES as u64));
    let huge = format!("{}<mxfile/>", comment(100 << 20));
    let (kind, looked) = sniff_counted(huge.as_bytes());
    assert_eq!(kind, None);
    assert!(looked <= MAX_PROLOG_BYTES as u64, "{looked}");
    let (kind, looked) = sniff_counted(b"  <mxfile/>");
    assert_eq!(kind, Some(Kind::Document));
    assert!(looked > 0 && looked <= 12, "{looked}");
    // A file made of nothing but declarations and comments, many of them, is looked at once and no further.
    let many = "<?a?><!--b-->".repeat(1_000_000);
    let (kind, looked) = sniff_counted(many.as_bytes());
    assert_eq!(kind, None);
    assert!(looked <= MAX_PROLOG_BYTES as u64, "{looked}");
}

#[test]
fn a_library_is_refused_in_words_and_other_roots_are_not_mine() {
    let (kind, class, words) = refusal(b"<mxlibrary>[{\"xml\":\"x\"}]</mxlibrary>");
    assert_eq!((kind, class), (ErrorKind::Refused, "drawio_library_not_drawing"));
    assert!(words.contains("library"), "{words}");
    for doc in [&b"<html><body/></html>"[..], b"<foo/>", b"plain text", b"", b"<svg/>"] {
        let (kind, class, _) = refusal(doc);
        assert_eq!((kind, class), (ErrorKind::NotMine, "drawio_not_a_drawing"), "{}", String::from_utf8_lossy(doc));
    }
}

#[test]
fn a_declared_encoding_other_than_utf8_utf16_and_us_ascii_is_refused_and_us_ascii_is_read() {
    let with = |enc: &str, body: &str| format!("<?xml version=\"1.0\" encoding=\"{enc}\"?><mxfile>{body}</mxfile>");
    for enc in ["ISO-8859-1", "windows-1252", "latin1", "Shift_JIS", "UTF-32", "utf8", "EBCDIC"] {
        let (kind, class, words) = refusal(with(enc, "<diagram/>").as_bytes());
        assert_eq!((kind, class), (ErrorKind::Refused, "drawio_encoding_unsupported"), "{enc}");
        assert!(words.contains(enc), "the encoding is named: {words}");
    }
    // Read: UTF-8 in any case, UTF-16 on UTF-16 bytes, and US-ASCII on a file that is all ASCII.
    for enc in ["UTF-8", "utf-8", "US-ASCII", "us-ascii"] {
        let file = read(with(enc, "<diagram name=\"A\"/>").as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{enc}: {e}"));
        assert_eq!(&*name0(&file.doc), "A", "{enc}");
    }
    let doc = utf16(&with("UTF-16", "<diagram name=\"B\"/>"), true, true);
    assert_eq!(&*name0(&read(&doc, &Limits::default()).unwrap().doc), "B");
    // US-ASCII declared on bytes that are not ASCII is the parser's refusal, as it always was; UTF-8 declared on UTF-16 bytes too.
    let (kind, class, _) = refusal(with("US-ASCII", "<diagram name=\"caf\u{e9}\"/>").as_bytes());
    assert_eq!((kind, class), (ErrorKind::Damaged, "xml_malformed"));
    let (_, class, _) = refusal(&utf16(&with("UTF-8", "<diagram/>"), true, true));
    assert_eq!(class, "xml_malformed");
    // A file that declares nothing is UTF-8.
    let file = read("<mxfile><diagram name=\"caf\u{e9}\"/></mxfile>".as_bytes(), &Limits::default()).unwrap();
    assert_eq!(&*name0(&file.doc), "caf\u{e9}");
    // The declaration is read at the start of the file only: a mention of an encoding in a comment or a value changes nothing.
    let file = read("<!-- encoding=\"ISO-8859-1\" --><mxfile><diagram name=\"encoding=&quot;x&quot;\"/></mxfile>".as_bytes(), &Limits::default());
    assert!(file.is_ok());
}

#[test]
fn a_file_that_is_not_well_formed_is_damaged_and_says_where_and_nothing_of_what_it_holds() {
    for doc in [&b"<mxfile><diagram></mxfile>"[..], b"<mxfile><diagram name=\"a></mxfile>", b"<mxfile></mxfile><mxfile/>", b"<mxfile/>text"] {
        let (kind, class, words) = refusal(doc);
        assert_eq!((kind, class), (ErrorKind::Damaged, "xml_malformed"), "{}", String::from_utf8_lossy(doc));
        assert!(words.contains("byte"), "{words}");
    }
    let secret = "SECRETNAME";
    let doc = format!("<mxfile><{secret}></mxfile>");
    let (_, _, words) = refusal(doc.as_bytes());
    assert!(!words.contains(secret), "{words}");
}
