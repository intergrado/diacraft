//! The writer: a package written is read back by this crate's own reader, whole and with no warning; the local and central
//! headers agree; only stored and deflated entries; legal names. And the tree writer (`xml_out.rs`):
//! a tree written and read again is the tree.

use diacraft_opc::xml::{El, Node, SpanKind, XmlWriteError, child_spans, parse, split_document, to_bytes};
use diacraft_opc::{Limits, Package, PackageWriter, WriteError};
use proptest::prelude::*;

use crate::content_types_xml;

const XML: &str = "application/xml";

fn types() -> Vec<u8> {
    content_types_xml(
        &[
            ("rels", "application/vnd.openxmlformats-package.relationships+xml"),
            ("xml", XML),
            ("png", "image/png"),
            ("bin", "application/octet-stream"),
        ],
        &[("/visio/document.xml", "application/vnd.ms-visio.drawing.main+xml")],
    )
    .into_bytes()
}

fn writer() -> PackageWriter {
    PackageWriter::new(&types()).unwrap()
}

/// One entry of an archive as its two headers state it.
#[derive(Debug, PartialEq)]
struct Header {
    version_needed: u16,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    name: Vec<u8>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every entry of `zip` as (local header, central header, offset of the data), read by hand from the APPNOTE's layout.
fn headers(zip: &[u8]) -> Vec<(Header, Header, usize)> {
    let end = zip.len() - 22;
    assert_eq!(u32_at(zip, end), 0x0605_4B50, "an end record with no comment is the last 22 bytes");
    let count = u16_at(zip, end + 10) as usize;
    assert_eq!(count, u16_at(zip, end + 8) as usize);
    let mut at = u32_at(zip, end + 16) as usize;
    assert_eq!(at + u32_at(zip, end + 12) as usize, end, "the directory ends where the end record starts");
    let mut out = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(zip, at), 0x0201_4B50);
        let (name_len, extra, comment) = (u16_at(zip, at + 28) as usize, u16_at(zip, at + 30) as usize, u16_at(zip, at + 32) as usize);
        assert_eq!((extra, comment), (0, 0), "no extra field and no comment");
        assert_eq!(u16_at(zip, at + 34), 0, "disk");
        let central = Header {
            version_needed: u16_at(zip, at + 6),
            flags: u16_at(zip, at + 8),
            method: u16_at(zip, at + 10),
            time: u16_at(zip, at + 12),
            date: u16_at(zip, at + 14),
            crc: u32_at(zip, at + 16),
            compressed: u32_at(zip, at + 20),
            size: u32_at(zip, at + 24),
            name: zip[at + 46..at + 46 + name_len].to_vec(),
        };
        let local_at = u32_at(zip, at + 42) as usize;
        assert_eq!(u32_at(zip, local_at), 0x0403_4B50);
        let (l_name_len, l_extra) = (u16_at(zip, local_at + 26) as usize, u16_at(zip, local_at + 28) as usize);
        assert_eq!(l_extra, 0);
        let local = Header {
            version_needed: u16_at(zip, local_at + 4),
            flags: u16_at(zip, local_at + 6),
            method: u16_at(zip, local_at + 8),
            time: u16_at(zip, local_at + 10),
            date: u16_at(zip, local_at + 12),
            crc: u32_at(zip, local_at + 14),
            compressed: u32_at(zip, local_at + 18),
            size: u32_at(zip, local_at + 22),
            name: zip[local_at + 30..local_at + 30 + l_name_len].to_vec(),
        };
        out.push((local, central, local_at + 30 + l_name_len));
        at += 46 + name_len;
    }
    out
}

/// Bytes deflate cannot make smaller: a xorshift sequence.
fn noise(n: usize) -> Vec<u8> {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn read(bytes: &[u8]) -> Package {
    Package::read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the writer's own output does not read: {e}"))
}

#[test]
fn a_package_written_reads_back_whole_and_with_no_warning() {
    let mut w = writer();
    let doc = "<VisioDocument/>".as_bytes();
    w.add("/visio/document.xml", doc).unwrap();
    w.add("/visio/media/image1.png", &[0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
    w.add("/visio/data.bin", &vec![7u8; 5000]).unwrap();
    w.add("/visio/empty.xml", b"").unwrap();
    let zip = w.finish().unwrap();
    let pkg = read(&zip);
    assert!(pkg.warnings().is_empty(), "{:?}", pkg.warnings());
    assert_eq!(pkg.part("/visio/document.xml"), Some(doc));
    assert_eq!(pkg.part("/visio/media/image1.png"), Some(&[0x89, b'P', b'N', b'G', 1, 2, 3][..]));
    assert_eq!(pkg.part("/visio/data.bin"), Some(&vec![7u8; 5000][..]));
    assert_eq!(pkg.part("/visio/empty.xml"), Some(&b""[..]));
    assert_eq!(pkg.parts().count(), 4);
    assert_eq!(pkg.content_type("/visio/document.xml"), Some("application/vnd.ms-visio.drawing.main+xml"));
    assert_eq!(pkg.content_type("/visio/media/image1.png"), Some("image/png"));
}

#[test]
fn local_and_central_headers_agree_and_every_entry_is_stored_or_deflated() {
    let mut w = writer();
    w.add("/visio/document.xml", &b"<a>hello hello hello hello hello hello hello hello</a>".repeat(40)).unwrap();
    w.add("/visio/media/image1.png", b"\x89PNG-not-really").unwrap();
    w.add("/visio/media/random.bin", &noise(4000)).unwrap();
    w.add("/visio/tiny.xml", b"<a/>").unwrap();
    let zip = w.finish().unwrap();
    let all = headers(&zip);
    assert_eq!(all.len(), 5, "the content types stream and four parts");
    assert_eq!(all[0].1.name, b"[Content_Types].xml", "the content types stream is first");
    for (local, central, data_at) in &all {
        assert_eq!(local, central, "Local and central headers agree on {:?}", String::from_utf8_lossy(&central.name));
        assert!(matches!(central.method, 0 | 8), "only stored and deflated");
        assert_eq!(central.flags, 0, "no encryption, no data descriptor");
        assert!(central.version_needed == if central.method == 8 { 20 } else { 10 });
        assert!(data_at + central.compressed as usize <= zip.len());
        if central.method == 0 {
            assert_eq!(central.compressed, central.size);
        }
    }
    let by_name = |n: &str| all.iter().find(|(_, c, _)| c.name == n.as_bytes()).unwrap().1.method;
    assert_eq!(by_name("visio/document.xml"), 8, "XML is deflated");
    assert_eq!(by_name("visio/media/image1.png"), 0, "a picture that is compressed already is stored");
    assert_eq!(by_name("visio/media/random.bin"), 0, "a part deflate does not make smaller is stored");
    assert_eq!(by_name("visio/tiny.xml"), 0, "so is one too small to gain");
}

#[test]
fn the_same_parts_give_the_same_bytes() {
    let make = || {
        let mut w = writer();
        w.add("/visio/document.xml", b"<VisioDocument/>").unwrap();
        w.add("/visio/pages/pages.xml", b"<Pages/>").unwrap();
        w.finish().unwrap()
    };
    assert_eq!(make(), make(), "no clock, no randomness: a save of the same drawing is the same file");
}

#[test]
fn names_are_item_names_with_escapes_and_come_back_as_they_went() {
    let names = ["/visio/media/café.png", "/visio/a b.xml", "/visio/100%.xml", "/visio/a%41.xml", "/visio/中文/图.bin", "/Visio/MIXED/Case.XML"];
    let mut w = writer();
    for n in names {
        w.add(n, n.as_bytes()).unwrap();
    }
    let zip = w.finish().unwrap();
    let ascii = headers(&zip).iter().all(|(_, c, _)| c.name.is_ascii());
    assert!(ascii, "The item names are ASCII");
    let pkg = read(&zip);
    assert!(pkg.warnings().is_empty());
    for n in names {
        assert_eq!(pkg.part(n), Some(n.as_bytes()), "{n}");
        assert!(pkg.parts().any(|p| p == n), "{n}: in the case it was given");
    }
    assert_eq!(opc_item("/visio/a b.xml"), "visio/a%20b.xml");
    assert_eq!(opc_item("/visio/100%.xml"), "visio/100%25.xml");
    assert_eq!(opc_item("/visio/café.png"), "visio/caf%C3%A9.png");
}

fn opc_item(part: &str) -> String {
    diacraft_opc::item_name(part).unwrap()
}

#[test]
fn names_the_reader_would_refuse_are_refused() {
    for bad in [
        "visio/document.xml",
        "/",
        "",
        "/visio//document.xml",
        "/visio/a./b.xml",
        "/visio/trailing.",
        "/visio/../x.xml",
        "/visio/./x.xml",
        "/visio\\x.xml",
        "/visio/a\u{1}b.xml",
        "/visio/a\nb.xml",
        "/[Content_Types].xml",
        "/visio/folder/",
        "C:/x.xml",
    ] {
        let mut w = writer();
        let error = w.add(bad, b"x").unwrap_err();
        assert_eq!(error.class(), "invalid_part_name", "{bad:?}: {error}");
    }
    let mut w = writer();
    w.add(&format!("/{}.xml", "a".repeat(2000)), b"x").unwrap_err();
}

#[test]
fn two_names_that_compare_equal_and_a_folder_that_is_a_part_are_refused() {
    let mut w = writer();
    w.add("/visio/document.xml", b"x").unwrap();
    assert_eq!(w.add("/VISIO/Document.XML", b"y").unwrap_err().class(), "duplicate_part");
    w.add("/a/b.xml", b"x").unwrap();
    // `/a` after `/a/b.xml`, and `/c/d.xml` after `/c`.
    assert_eq!(w.add("/a", b"x").unwrap_err().class(), "part_name_prefix");
    w.add("/c.bin", b"x").unwrap();
    assert_eq!(w.add("/c.bin/d.xml", b"x").unwrap_err().class(), "part_name_prefix");
    // The failed adds left nothing behind.
    let zip = w.finish().unwrap();
    let names: Vec<_> = read(&zip).parts().map(str::to_owned).collect();
    assert_eq!(names, ["/visio/document.xml", "/a/b.xml", "/c.bin"]);
}

#[test]
fn a_part_with_no_content_type_is_refused() {
    let mut w = writer();
    let error = w.add("/visio/mystery.zzz", b"x").unwrap_err();
    assert!(matches!(error, WriteError::NoContentType { .. }), "{error}");
    assert_eq!(error.class(), "missing_content_type");
    // A content types stream that is not one is refused at the start.
    assert!(PackageWriter::new(b"<NotTypes/>").is_err());
    assert!(PackageWriter::new(b"not xml").is_err());
}

#[test]
fn a_large_part_and_many_parts_write_and_read_back() {
    let mut w = writer();
    let big: Vec<u8> = (0..6_000_000u32).map(|i| b"<Shape ID=\"1\"/>"[(i % 15) as usize]).collect();
    w.add("/visio/pages/page1.xml", &big).unwrap();
    for i in 0..2000 {
        w.add(&format!("/visio/media/image{i}.png"), &[i as u8; 10]).unwrap();
    }
    let zip = w.finish().unwrap();
    let pkg = read(&zip);
    assert_eq!(pkg.part("/visio/pages/page1.xml").map(<[u8]>::len), Some(big.len()));
    assert_eq!(pkg.parts().count(), 2001);
    assert!(zip.len() < big.len() / 10, "the big part is deflated");
}

#[test]
fn more_than_65535_items_are_refused_and_not_wrapped() {
    let mut w = PackageWriter::new(&types()).unwrap();
    let mut refused = None;
    for i in 0..70_000u32 {
        if let Err(e) = w.add(&format!("/p/{i}.bin"), b"") {
            refused = Some((i, e));
            break;
        }
    }
    let (at, error) = refused.expect("past the limit an add is refused");
    assert_eq!(error, WriteError::TooMany);
    assert_eq!(at, 65_534, "the content types stream is one of the items");
}

// ------------------------------------------------------------------------------------------------------------- the tree writer

fn tree(text: &str) -> El {
    parse(text.as_bytes()).unwrap()
}

fn written(el: &El) -> String {
    String::from_utf8(to_bytes(el, b"", b"", false).unwrap()).unwrap()
}

#[test]
fn a_tree_written_and_read_again_is_the_tree() {
    let source = concat!(
        "<VisioDocument xmlns=\"urn:a\" xmlns:r=\"urn:r\" xml:space=\"preserve\" x:mine=\"1\" xmlns:x=\"urn:x\">\n  ",
        "<Shape ID='5'  V=\"1.50\" F=\"IF(a&lt;b,&quot;x&quot;,&apos;y&apos;)\"><x:note>hi &amp; bye</x:note><Cell N=\"PinX\" V=\"1\"><?NewValue V?></Cell></Shape>",
        "<!-- a comment --><Text>  two  spaces <cp IX=\"1\"/>x</Text><![CDATA[raw <b> & ]]><Empty/>\n</VisioDocument>"
    );
    let first = tree(source);
    let bytes = to_bytes(&first, b"", b"", true).unwrap();
    let again = parse(&bytes).unwrap();
    assert_eq!(first, again, "{}", String::from_utf8_lossy(&bytes));
    // And the text is stable: a second write of the second read is the same bytes.
    assert_eq!(to_bytes(&again, b"", b"", true).unwrap(), bytes);
}

#[test]
fn line_breaks_and_tabs_in_values_survive_as_references() {
    // Of the preservation spec: a literal line break in an attribute would read back as a space.
    let el = El {
        name: "Cell".into(),
        attrs: vec![("V".into(), "a\nb\r\nc\td".into()), ("F".into(), "x\"y&z<w".into())],
        children: vec![Node::Text("one\r\ntwo\rthree\n".into())],
    };
    let out = written(&el);
    assert!(out.contains("&#10;") && out.contains("&#13;") && out.contains("&#9;"), "{out}");
    assert!(!out.contains('\r') && !out.contains('\t'), "{out:?}");
    let back = parse(out.as_bytes()).unwrap();
    assert_eq!(back.attr("V"), Some("a\nb\r\nc\td"));
    assert_eq!(back.attr("F"), Some("x\"y&z<w"));
    assert_eq!(back.text(), "one\r\ntwo\rthree\n");
}

#[test]
fn what_xml_cannot_hold_is_an_error_and_not_a_damaged_part() {
    let cell = |attrs: Vec<(&str, &str)>, children: Vec<Node>| El {
        name: "Cell".into(),
        attrs: attrs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        children,
    };
    let err = |el: &El| to_bytes(el, b"", b"", false).unwrap_err();
    assert!(matches!(err(&cell(vec![("V", "a\u{1}b")], vec![])), XmlWriteError::NotXmlChar { ch: 1, .. }));
    assert!(matches!(err(&cell(vec![], vec![Node::Text("\u{0}".into())])), XmlWriteError::NotXmlChar { ch: 0, .. }));
    assert!(matches!(err(&cell(vec![("V", "\u{FFFE}")], vec![])), XmlWriteError::NotXmlChar { .. }));
    assert!(matches!(err(&cell(vec![("a b", "x")], vec![])), XmlWriteError::BadName(_)));
    assert!(matches!(err(&cell(vec![("N", "1"), ("N", "2")], vec![])), XmlWriteError::DuplicateAttribute(_)));
    assert!(matches!(err(&El { name: "1bad".into(), ..El::default() }), XmlWriteError::BadName(_)));
    assert!(matches!(err(&cell(vec![], vec![Node::Comment("a--b".into())])), XmlWriteError::BadNode(_)));
    assert!(matches!(err(&cell(vec![], vec![Node::Comment("ends-".into())])), XmlWriteError::BadNode(_)));
    assert!(matches!(err(&cell(vec![], vec![Node::Pi { target: "xml".into(), data: String::new() }])), XmlWriteError::BadNode(_)));
    assert!(matches!(err(&cell(vec![], vec![Node::Pi { target: "a".into(), data: "x?>y".into() }])), XmlWriteError::BadNode(_)));
    // Many attributes: the duplicate is found by the set.
    let many: Vec<(String, String)> = (0..40).map(|i| (format!("a{}", i % 30), "1".to_owned())).collect();
    assert!(matches!(err(&El { name: "x".into(), attrs: many, children: vec![] }), XmlWriteError::DuplicateAttribute(_)));
}

#[test]
fn cdata_with_the_end_marker_is_split_and_reads_back_whole() {
    let el = El { name: "a".into(), attrs: vec![], children: vec![Node::CData("x]]>y".into()), Node::CData("p\rq".into())] };
    let out = written(&el);
    let back = parse(out.as_bytes()).unwrap();
    assert_eq!(back.text(), "x]]>yp\nq".replace("p\nq", "p\rq"), "{out}");
}

#[test]
fn a_tree_as_deep_as_the_reader_allows_writes_without_recursion() {
    // 500 levels is under the reader's ceiling of 512 but over its default of 200: the writer's own stack is the heap.
    let mut el = El { name: "leaf".into(), ..El::default() };
    for _ in 0..500 {
        el = El { name: "n".into(), attrs: vec![], children: vec![Node::El(el)] };
    }
    let out = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || {
        let bytes = to_bytes(&el, b"", b"", false).unwrap();
        // Dropping a tree this deep is the tree's own business and is bounded by the ceiling.
        bytes.len()
    });
    assert!(out.unwrap().join().unwrap() > 1000);
}

#[test]
fn the_part_around_the_root_is_found_and_kept() {
    let source = "\u{FEFF}<?xml version='1.0' encoding='utf-8'?>\r\n<!-- before --><Root a='1'>\r\n<x/></Root>\r\n<!-- after -->\r\n";
    let (start, end) = split_document(source.as_bytes()).unwrap();
    assert_eq!(&source.as_bytes()[start..start + 5], b"<Root");
    assert!(source[..start].starts_with('\u{FEFF}') && source[..start].contains("<!-- before -->"));
    assert_eq!(&source[end..], "\r\n<!-- after -->\r\n");
    let el = parse(source.as_bytes()).unwrap();
    let out = to_bytes(&el, &source.as_bytes()[..start], &source.as_bytes()[end..], true).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.starts_with("\u{FEFF}<?xml version='1.0' encoding='utf-8'?>\r\n<!-- before --><Root a=\"1\">"), "{out:?}");
    assert!(out.ends_with("</Root>\r\n<!-- after -->\r\n"), "{out:?}");
    // An empty root, no declaration, and parts that are not UTF-8 text.
    assert_eq!(split_document(b"<a/>"), Some((0, 4)));
    assert_eq!(split_document(b"  <a/>  "), Some((2, 6)));
    assert_eq!(split_document(b"\xFF\xFE<\0a\0/\0>\0"), None);
    assert_eq!(split_document(b"<\0a\0/\0>\0"), None);
    assert_eq!(split_document(b"no root"), None);
    assert_eq!(split_document(b"<a>"), None);
    // quick-xml 0.42 checks that an event is UTF-8 as it builds it: a byte that is not, inside the root, gives no split.
    assert_eq!(split_document(b"<a>\xFF</a>"), None);
}

/// The kind of a node, as `child_spans` names it.
fn kind_of(node: &Node) -> SpanKind {
    match node {
        Node::El(_) => SpanKind::Element,
        Node::Text(_) => SpanKind::Text,
        Node::CData(_) => SpanKind::CData,
        Node::Comment(_) => SpanKind::Comment,
        Node::Pi { .. } => SpanKind::Pi,
    }
}

/// What `child_spans` promises of `bytes`: the spans follow one another and cover the content of the root exactly, and they are the children of the
/// tree, in number and in kind. → the spans' text.
fn spans_agree_with_the_tree(bytes: &[u8]) -> Vec<String> {
    let spans = child_spans(bytes, usize::MAX).unwrap_or_else(|| panic!("no spans for {:?}", String::from_utf8_lossy(bytes)));
    let (start, end) = spans.inner;
    let mut at = start;
    let mut texts = Vec::new();
    for (_, a, b) in &spans.children {
        assert_eq!(*a, at, "the spans follow one another in {:?}", String::from_utf8_lossy(bytes));
        assert!(b > a);
        texts.push(String::from_utf8(bytes[*a..*b].to_vec()).unwrap());
        at = *b;
    }
    assert_eq!(at, end, "and cover the content");
    let tree = parse(bytes).unwrap();
    let kinds: Vec<SpanKind> = spans.children.iter().map(|(k, _, _)| *k).collect();
    let want: Vec<SpanKind> = tree.children.iter().map(kind_of).collect();
    assert_eq!(kinds, want, "{:?}", String::from_utf8_lossy(bytes));
    texts
}

#[test]
fn the_children_of_the_root_are_found_where_they_lie_and_are_the_trees_children() {
    let source = "\u{FEFF}<?xml version='1.0'?>\r\n<Root a='1'>\r\n  <x b='2'></x>\r\n  <!-- c -->text &amp; more &#65;<![CDATA[raw]]><?pi data?><y/><z><w>deep</w></z>\r\n</Root>\r\n";
    let texts = spans_agree_with_the_tree(source.as_bytes());
    assert_eq!(
        texts,
        [
            "\r\n  ",
            "<x b='2'></x>",
            "\r\n  ",
            "<!-- c -->",
            "text &amp; more &#65;",
            "<![CDATA[raw]]>",
            "<?pi data?>",
            "<y/>",
            "<z><w>deep</w></z>",
            "\r\n"
        ]
    );
    // A root with nothing in it but text, and with nothing at all (written open and shut).
    assert_eq!(spans_agree_with_the_tree(b"<a>  </a>"), ["  "]);
    assert_eq!(spans_agree_with_the_tree(b"<a></a>"), Vec::<String>::new());
    assert_eq!(child_spans(b"<a/>", 10), None, "a root written empty has no content to find");
    // Parts that are not UTF-8 text with a root, and a limit on the children.
    assert_eq!(child_spans(b"\xFF\xFE<\0a\0>\0<\0/\0a\0>\0", 10), None);
    assert_eq!(child_spans(b"no root", 10), None);
    assert_eq!(child_spans(b"<a><b/><b/><b/></a>", 2), None);
    assert!(child_spans(b"<a><b/><b/></a>", 2).is_some());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 600, failure_persistence: None, ..ProptestConfig::default() })]

    /// Whatever a root holds, the spans are its children: a document with every kind of child, damaged at random, either does not parse, or its spans
    /// agree with its tree, or there are none; and nothing panics.
    #[test]
    fn spans_of_damaged_parts_never_panic_and_agree_with_the_tree_when_both_read(cut in 0usize..160, flip in 0usize..160, byte in any::<u8>()) {
        let source = "<?xml version='1.0'?><R a='1'>\r\n <x/>t&amp;u<!--c--><![CDATA[d]]><?p q?><y k=\"v\"><z/></y>\r\n</R>\r\n".as_bytes().to_vec();
        let mut bytes = source.clone();
        if let Some(b) = bytes.get_mut(flip % source.len()) {
            *b = byte;
        }
        bytes.truncate(source.len().saturating_sub(cut % 8));
        if let (Some(spans), Ok(tree)) = (child_spans(&bytes, usize::MAX), parse(&bytes)) {
            let kinds: Vec<SpanKind> = spans.children.iter().map(|(k, _, _)| *k).collect();
            let want: Vec<SpanKind> = tree.children.iter().map(kind_of).collect();
            prop_assert_eq!(kinds, want);
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------- generated

fn arb_name() -> impl Strategy<Value = String> {
    prop_oneof![Just("Cell".to_owned()), Just("x:note".to_owned()), "[A-Za-z_][A-Za-z0-9_.-]{0,6}"]
}

fn arb_text() -> impl Strategy<Value = String> {
    // Everything XML 1.0 allows that a person or a hostile file might put in a value, including the characters that need escapes.
    proptest::collection::vec(
        prop_oneof![
            Just('&'),
            Just('<'),
            Just('>'),
            Just('"'),
            Just('\''),
            Just('\n'),
            Just('\r'),
            Just('\t'),
            Just(' '),
            any::<char>().prop_filter("xml char", |c| { matches!(*c, '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}') })
        ],
        0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn arb_el(depth: u32) -> BoxedStrategy<El> {
    let leaf = (arb_name(), proptest::collection::vec(("[a-z]{1,4}", arb_text()), 0..3)).prop_map(|(name, attrs)| {
        let mut seen = std::collections::HashSet::new();
        El { name, attrs: attrs.into_iter().filter(|(k, _)| seen.insert(k.clone())).collect(), children: vec![] }
    });
    if depth == 0 {
        return leaf.boxed();
    }
    let child = prop_oneof![
        arb_el(depth - 1).prop_map(Node::El),
        arb_text().prop_filter("not blank, so it is kept as a node", |t| !t.is_empty()).prop_map(Node::Text),
        "[a-z ]{1,5}".prop_map(|t| Node::Comment(t.trim_end_matches('-').to_owned())),
        Just(Node::Pi { target: "NewValue".into(), data: "V".into() }),
    ];
    (leaf, proptest::collection::vec(child, 0..4))
        .prop_map(|(mut el, children)| {
            // Adjacent text runs are one run when read.
            for node in children {
                match (el.children.last_mut(), node) {
                    (Some(Node::Text(prev)), Node::Text(next)) => prev.push_str(&next),
                    (_, node) => el.children.push(node),
                }
            }
            el
        })
        .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn any_tree_written_is_read_back_as_itself(el in arb_el(3)) {
        // A tree the writer writes is a tree the reader reads, and reads the same.
        let bytes = to_bytes(&el, b"", b"", true).unwrap();
        let back = parse(&bytes).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&bytes)));
        prop_assert_eq!(&back, &el);
    }

    #[test]
    fn any_name_either_is_refused_or_comes_back(name in "(/[ -~\u{80}-\u{2FF}]{0,6}){1,4}") {
        let mut w = writer();
        match w.add(&name, b"data") {
            Err(_) => {}
            Ok(()) => {
                // It has to have a type to have been added: `.bin`/`.xml`/`.png` or an override.
                let zip = w.finish().unwrap();
                let pkg = Package::read(&zip, &Limits::default()).unwrap();
                prop_assert_eq!(pkg.part(&name), Some(&b"data"[..]));
            }
        }
    }
}
