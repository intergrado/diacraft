//! What a splicing writer builds on: where the tags of an XML part lie in its bytes (`xml_scan`), an entry replaced under its own
//! headers (`ArchiveWriter::replace`), and the extra fields a copy walks, counted.

use diacraft_container::xml::{self, Node};
use diacraft_container::xml_scan::{AttrSpan, ScanError, TagKind, TagParts, is_blank, is_utf16, scan_tags, tag_attributes};
use diacraft_container::zip::{Archive, ArchiveWriter, Copied, Method, copy_account_counted};
use diacraft_container::{Limits, Work, XmlError, XmlLimits};
use proptest::prelude::*;

use crate::read_all;
use crate::zipbuild::{Item, build, extra_record};

/// Every tag of `bytes`: its kind, name, text of the whole tag, depth and the text run before it.
fn tags(bytes: &[u8]) -> Vec<(TagKind, String, String, usize, Option<String>)> {
    let mut out = Vec::new();
    scan_tags::<()>(bytes, &XmlLimits::default(), &mut Work::default(), |tag| {
        let text = |a: usize, b: usize| String::from_utf8_lossy(&bytes[a..b]).into_owned();
        out.push((tag.kind, tag.name.to_owned(), text(tag.start, tag.end), tag.depth, tag.text_before.map(|(a, b)| text(a, b))));
        Ok(())
    })
    .unwrap();
    out
}

#[test]
fn every_tag_is_found_where_it_lies_with_its_depth_and_the_text_before_it() {
    let part = concat!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- before -->\n",
        "<a:root xmlns:a=\"urn:x\" k='v>w'>\n  <a:one n=\"1\"/>\n  <!-- c --><a:two>t &amp; <![CDATA[<c>]]><a:in/> &#65; </a:two>\n",
        "  <a:three\n   n = \"3\" ></a:three>\n</a:root>\n<!-- after -->\n"
    );
    let found = tags(part.as_bytes());
    let shown: Vec<(TagKind, &str, usize)> = found.iter().map(|(k, n, _, d, _)| (*k, n.as_str(), *d)).collect();
    assert_eq!(
        shown,
        [
            (TagKind::Open, "a:root", 0),
            (TagKind::Empty, "a:one", 1),
            (TagKind::Open, "a:two", 1),
            (TagKind::Empty, "a:in", 2),
            (TagKind::Close, "a:two", 1),
            (TagKind::Open, "a:three", 1),
            (TagKind::Close, "a:three", 1),
            (TagKind::Close, "a:root", 0),
        ]
    );
    // The bytes between `start` and `end` are the tag, a `>` inside a quoted value does not end it, and a tag with line breaks is one tag.
    assert_eq!(found[0].2, "<a:root xmlns:a=\"urn:x\" k='v>w'>");
    assert_eq!(found[1].2, "<a:one n=\"1\"/>");
    assert_eq!(found[5].2, "<a:three\n   n = \"3\" >");
    assert_eq!(found[7].2, "</a:root>");
    // The run of text before a tag: the white space before `a:one`; none before `a:two`, a comment is between; text and a reference are one
    // run; a CDATA section ends a run, so `a:in` follows it directly; and the run before an end tag.
    assert_eq!(found[1].4.as_deref(), Some("\n  "));
    assert_eq!(found[2].4, None);
    assert_eq!(found[3].4, None, "a CDATA section is between");
    assert_eq!(found[4].4.as_deref(), Some(" &#65; "));
    assert_eq!(found[5].4.as_deref(), Some("\n  "));
    assert_eq!(found[7].4.as_deref(), Some("\n"));
}

#[test]
fn a_scan_counts_each_event_and_refuses_what_the_tree_reader_refuses() {
    let mut work = Work::default();
    scan_tags::<()>(b"<a><b/>text<c></c></a>", &XmlLimits::default(), &mut work, |_| Ok(())).unwrap();
    assert_eq!(work.xml_events, 7, "a, b, text, c, c, a, and the end of the input");
    let class = |bytes: &[u8], limits: &XmlLimits| match scan_tags::<()>(bytes, limits, &mut Work::default(), |_| Ok(())) {
        Ok(()) => "scanned",
        Err(ScanError::Xml(e)) => e.class(),
        Err(ScanError::Visit(())) => "visit",
    };
    let limits = XmlLimits::default();
    assert_eq!(class(b"<a><b></a>", &limits), "xml_malformed");
    assert_eq!(class(b"<a>", &limits), "xml_malformed");
    assert_eq!(class(b"", &limits), "xml_malformed");
    assert_eq!(class(b"<a/><b/>", &limits), "xml_malformed");
    assert_eq!(class(b"</a>", &limits), "xml_malformed");
    assert_eq!(class(b"<!DOCTYPE a [<!ENTITY x \"y\">]><a/>", &limits), "xml_entity_refused");
    assert_eq!(class(&[b'<', b'a', b'>', 0xFF, b'<', b'/', b'a', b'>'], &limits), "xml_malformed", "bytes that are not UTF-8");
    assert_eq!(class(&[0xFF, 0xFE, b'<', 0, b'a', 0, b'/', 0, b'>', 0], &limits), "xml_malformed", "UTF-16 cannot be spliced");
    assert!(is_utf16(&[0xFE, 0xFF, 0, b'<']) && is_utf16(&[b'<', 0, b'a', 0]) && !is_utf16(b"<a/>"));
    assert_eq!(class(b"<a><b><c/></b></a>", &XmlLimits { max_depth: 2, ..limits.clone() }), "xml_too_complex");
    assert_eq!(class(b"<a><b><c/></b></a>", &XmlLimits { max_depth: 3, ..limits.clone() }), "scanned");
    assert_eq!(class(b"<a><b/><b/><b/></a>", &XmlLimits { max_nodes: 4, ..limits.clone() }), "xml_too_complex");
    // The events are the six counted above (a, three b, a, the end): a limit of exactly that many holds, one fewer does not.
    assert_eq!(class(b"<a><b/><b/><b/></a>", &XmlLimits { max_nodes: 6, ..limits.clone() }), "scanned");
    assert_eq!(class(b"<a><b/><b/><b/></a>", &XmlLimits { max_nodes: 5, ..limits.clone() }), "xml_too_complex");
    // No root and two roots are told apart.
    let said = |bytes: &[u8]| match scan_tags::<()>(bytes, &limits, &mut Work::default(), |_| Ok(())) {
        Err(ScanError::Xml(e)) => e.to_string(),
        other => format!("{other:?}"),
    };
    assert!(said(b"<!-- nothing -->").contains("no root element"), "{}", said(b"<!-- nothing -->"));
    assert!(said(b"<a/><b/>").contains("more than one root element"), "{}", said(b"<a/><b/>"));
    // The visit's own refusal stops the scan at the tag.
    let mut seen = 0;
    let stopped = scan_tags(b"<a><b/><b/></a>", &limits, &mut Work::default(), |tag| {
        seen += 1;
        if tag.name == "b" { Err(7) } else { Ok(()) }
    });
    assert_eq!((stopped, seen), (Err(ScanError::Visit(7)), 2));
    // Depth 200 is the default and the one tag past it is `xml_too_complex`, the tree reader's own answer.
    let nested = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
    assert_eq!(class(nested(200).as_bytes(), &limits), "scanned");
    assert_eq!(class(nested(201).as_bytes(), &limits), "xml_too_complex");
    assert_eq!(xml::parse(nested(201).as_bytes()).unwrap_err(), XmlError::TooDeep { limit: 200 });
}

/// The tag scan passed a part with a character XML 1.0 does not allow, which the tree reader refuses. Each character the two
/// disagreed on, in text, in an attribute value and in a comment, is now refused by the scan as the tree's parser refuses it: the scan and the parse
/// agree on every character from the control characters and the noncharacters to the ends of the planes.
#[test]
fn a_scan_refuses_every_character_the_tree_reader_refuses_and_passes_every_one_it_passes() {
    let limits = XmlLimits::default();
    let mut chars: Vec<char> = (0u32..0x100).filter_map(char::from_u32).collect();
    chars.extend(['\u{D7FF}', '\u{E000}', '\u{FFFD}', '\u{FFFE}', '\u{FFFF}', '\u{10000}', '\u{10FFFF}', '\u{2028}', '\u{FEFF}']);
    // The characters that are markup where they stand say something else than that they are allowed.
    for c in chars.into_iter().filter(|c| !matches!(c, '<' | '>' | '&' | '"' | '\'' | '-' | ']' | '!' | '?' | '/' | '=')) {
        for template in ["<a>x{}y</a>", "<a b=\"x{}y\"/>", "<a><!-- x{}y --></a>"] {
            let text = template.replace("{}", &c.to_string());
            let scanned = scan_tags::<()>(text.as_bytes(), &limits, &mut Work::default(), |_| Ok(())).is_ok();
            let parsed = xml::parse(text.as_bytes()).is_ok();
            assert_eq!(scanned, parsed, "U+{:04X} in {template}: scan {scanned}, parse {parsed}", u32::from(c));
            assert_eq!(parsed, xml::xml_char(c), "U+{:04X} in {template}", u32::from(c));
        }
    }
    let said = match scan_tags::<()>("<a>\u{1}</a>".as_bytes(), &limits, &mut Work::default(), |_| Ok(())) {
        Err(ScanError::Xml(e)) => e.to_string(),
        other => format!("{other:?}"),
    };
    assert!(said.contains("U+0001"), "{said}");
    assert_eq!(xml::first_bad_char("a\tb\r\nc"), None);
    assert_eq!(xml::first_bad_char("ab\u{B}c"), Some((2, '\u{B}')));
    assert_eq!(xml::first_bad_char("é\u{FFFE}"), Some((2, '\u{FFFE}')));
    assert_eq!(xml::first_bad_char("\u{FFFD}\u{FFFF}"), Some((3, '\u{FFFF}')));
}

/// The attributes of the first tag of `text`.
fn attrs_of(text: &str) -> Option<(Vec<AttrSpan>, TagParts)> {
    let end = text.find('>').map(|i| i + 1)?;
    // The first `>` outside quotes: the tests' tags keep none inside but the one the scan test above covers.
    let mut out = Vec::new();
    let parts = tag_attributes(text.as_bytes(), 0, end, &mut out)?;
    Some((out, parts))
}

#[test]
fn the_attributes_of_a_tag_are_found_with_their_places() {
    let text = "<d:rect  a=\"1\"\tb = '2 > 3'\n c=\"\" />";
    let (attrs, parts) = attrs_of(&text.replace("2 > 3", "2 3")).unwrap();
    let text = text.replace("2 > 3", "2 3");
    let span = |a: &AttrSpan| (&text[a.name.0..a.name.1], &text[a.value.0..a.value.1], &text[a.lead..a.end]);
    assert_eq!(attrs.iter().map(span).collect::<Vec<_>>(), [("a", "1", "  a=\"1\""), ("b", "2 3", "\tb = '2 3'"), ("c", "", "\n c=\"\"")]);
    assert_eq!(attrs.iter().map(|a| a.quote).collect::<Vec<_>>(), [b'"', b'\'', b'"']);
    assert_eq!(&text[..parts.name_end], "<d:rect");
    assert_eq!(&text[parts.attrs_end..], " />", "a new attribute goes after the last, before the white space and the slash");
    // No attribute: after the name.
    let (none, parts) = attrs_of("<a/>").unwrap();
    assert!(none.is_empty());
    assert_eq!((parts.name_end, parts.attrs_end), (2, 2));
    // What a well-formed tag is not: a value with no quote, a name with no value, an unclosed quote.
    for bad in ["<a b=1>", "<a b>", "<a b=\"1>", "<a =\"1\">", "not a tag>"] {
        assert!(attrs_of(bad).is_none(), "{bad}");
    }
    // A tag of many attributes is one pass.
    let many = format!("<a {}>", (0..5_000).map(|i| format!("n{i}=\"{i}\"")).collect::<Vec<_>>().join(" "));
    assert_eq!(attrs_of(&many).unwrap().0.len(), 5_000);
    assert!(is_blank(b" \t\r\n") && is_blank(b"") && !is_blank(b" x "));
}

// A generated document: nested elements with attributes in either quote, text, comments, CDATA and white space, the scan's tags checked
// against the tree the reader makes of the same bytes.
fn name() -> impl Strategy<Value = String> {
    "[a-z]{1,4}(:[a-z]{1,4})?".prop_map(String::from)
}

fn element(depth: u32) -> BoxedStrategy<String> {
    let attrs = proptest::collection::vec((name(), "[a-z0-9 ]{0,6}", any::<bool>()), 0..4).prop_map(|list| {
        let mut seen = std::collections::HashSet::new();
        list.into_iter()
            .filter(|(n, _, _)| !n.starts_with("xmlns") && seen.insert(n.clone()))
            .map(|(n, v, single)| if single { format!(" {n}='{v}'") } else { format!(" {n}=\"{v}\"") })
            .collect::<String>()
    });
    let gap = prop_oneof![Just(String::new()), Just("\n  ".to_owned()), Just(" ".to_owned())];
    if depth == 0 {
        (name(), attrs, any::<bool>()).prop_map(|(n, a, empty)| if empty { format!("<{n}{a}/>") } else { format!("<{n}{a}></{n}>") }).boxed()
    } else {
        let child = element(depth - 1);
        let piece = prop_oneof![
            3 => child,
            1 => Just("text &amp; more".to_owned()),
            1 => Just("<!-- note -->".to_owned()),
            1 => Just("<![CDATA[<raw>]]>".to_owned()),
            1 => Just("<?pi data?>".to_owned()),
        ];
        (name(), attrs, proptest::collection::vec((gap, piece), 0..4))
            .prop_map(|(n, a, pieces)| {
                let inner: String = pieces.into_iter().map(|(g, p)| format!("{g}{p}")).collect();
                if inner.is_empty() { format!("<{n}{a}/>") } else { format!("<{n}{a}>{inner}</{n}>") }
            })
            .boxed()
    }
}

fn names_of(el: &xml::El, out: &mut Vec<String>) {
    out.push(el.name.clone());
    for node in &el.children {
        if let Node::El(child) = node {
            names_of(child, out);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn the_scan_and_the_tree_reader_see_the_same_elements_and_attributes(doc in element(3), bom in any::<bool>()) {
        let text = format!("{}<?xml version=\"1.0\"?>\n{doc}\n", if bom { "\u{feff}" } else { "" });
        let tree = match xml::parse(text.as_bytes()) {
            Ok(tree) => tree,
            // A document the strategy made that is not well-formed (a name clash is not one; this is rare): nothing to compare.
            Err(_) => return Ok(()),
        };
        let mut expected = Vec::new();
        names_of(&tree, &mut expected);
        let (mut opened, mut attrs_seen) = (Vec::new(), Vec::new());
        let bytes = text.as_bytes();
        scan_tags::<()>(bytes, &XmlLimits::default(), &mut Work::default(), |tag| {
            prop_assert_eq_ok(&bytes[tag.start..tag.start + 1], b"<");
            prop_assert_eq_ok(&bytes[tag.end - 1..tag.end], b">");
            if tag.kind != TagKind::Close {
                opened.push(tag.name.to_owned());
                let mut spans = Vec::new();
                let parts = tag_attributes(bytes, tag.start, tag.end, &mut spans).unwrap();
                prop_assert_eq_ok(&bytes[tag.start + 1..parts.name_end], tag.name.as_bytes());
                for a in &spans {
                    attrs_seen.push((String::from_utf8_lossy(&bytes[a.name.0..a.name.1]).into_owned(), String::from_utf8_lossy(&bytes[a.value.0..a.value.1]).into_owned()));
                }
            }
            Ok(())
        }).unwrap();
        prop_assert_eq!(opened, expected);
        let mut want = Vec::new();
        fn collect(el: &xml::El, out: &mut Vec<(String, String)>) {
            out.extend(el.attrs.iter().cloned());
            for node in &el.children { if let Node::El(c) = node { collect(c, out); } }
        }
        collect(&tree, &mut want);
        prop_assert_eq!(attrs_seen, want);
    }
}

/// A check usable inside the visit's closure, which cannot use `prop_assert!`.
fn prop_assert_eq_ok(a: &[u8], b: &[u8]) {
    assert_eq!(a, b);
}

#[test]
fn a_replaced_entry_keeps_its_headers_and_changes_the_bytes_the_checksum_and_the_sizes() {
    // What another writer puts in its rows, on an entry a save then changes: its made-by, time, date, attributes, comment, extra fields and
    // method (deflated), beside a stored entry that is changed too.
    let stamp = |n: u8| extra_record(0x5455, &[1, n, 0, 0, 0]);
    let old = b"<office:document-content>old</office:document-content>".repeat(5);
    let new = b"<office:document-content>new and longer</office:document-content>".repeat(7);
    let items = vec![
        Item::stored("mimetype", b"application/vnd.oasis.opendocument.graphics"),
        Item {
            made_by: 0x031E,
            time: 0x6A21,
            date: 0x5B67,
            internal: 1,
            external: 0o100644 << 16,
            comment: b"a row's own comment".to_vec(),
            ..Item::deflated("content.xml", &old).with_local_extra(&stamp(1)).with_extra(&stamp(2))
        },
        Item { made_by: 0x0314, time: 3, date: 0x42, ..Item::stored("Thumbnails/thumbnail.png", b"\x89PNG old") },
        Item::stored("styles.xml", b"<s/>"),
    ];
    let original = build(&items);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    let mut work = Work::default();
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy_counted(&archive, 0, &mut work).unwrap(), Copied::Exact);
    writer.replace(&archive, 1, &new, &mut work).unwrap();
    writer.replace(&archive, 2, b"\x89PNG new and a good deal longer", &mut work).unwrap();
    assert_eq!(writer.copy_counted(&archive, 3, &mut work).unwrap(), Copied::Exact);
    let copy = writer.finish().unwrap();
    let all = read_all(&copy).unwrap();
    assert_eq!(all[1], ("content.xml".to_owned(), new.clone()));
    assert_eq!(all[2], ("Thumbnails/thumbnail.png".to_owned(), b"\x89PNG new and a good deal longer".to_vec()));
    assert_eq!((all[0].1.as_slice(), all[3].1.as_slice()), (&b"application/vnd.oasis.opendocument.graphics"[..], &b"<s/>"[..]));
    let back = Archive::read(&copy, &Limits::default()).unwrap();
    let (was, now) = (&archive.entries()[1], &back.entries()[1]);
    assert_eq!(
        (now.version_made_by, now.version_needed, now.time, now.date, now.internal_attributes, now.external_attributes, now.flags, now.method),
        (was.version_made_by, was.version_needed, was.time, was.date, was.internal_attributes, was.external_attributes, was.flags, was.method),
        "every field of the headers but the data"
    );
    assert_eq!(now.compression(), Some(Method::Deflated), "a deflated entry stays deflated");
    assert_eq!((now.size, now.crc), (new.len() as u64, crc32fast::hash(&new)));
    assert_eq!(back.entries()[2].compression(), Some(Method::Stored), "a stored entry stays stored");
    assert_eq!(now.local_extra(&copy), was.local_extra(&original), "the local header's extra field");
    assert_eq!(now.row_extra_and_comment(&copy), was.row_extra_and_comment(&original), "the row's extra field and comment");
    assert_eq!((back.entries()[2].time, back.entries()[2].date), (3, 0x42));
    // The entries nobody changed are the same bytes and the order is the file's.
    assert_eq!(
        back.entries().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["mimetype", "content.xml", "Thumbnails/thumbnail.png", "styles.xml"]
    );
    // A stored entry is stored even where deflating would be smaller, and a deflated one deflated where it is larger.
    let mut again = ArchiveWriter::new();
    again.replace(&archive, 1, b"x", &mut work).unwrap();
    again.replace(&archive, 2, &b"a".repeat(5_000), &mut work).unwrap();
    let small_bytes = again.finish().unwrap();
    let small = Archive::read(&small_bytes, &Limits::default()).unwrap();
    assert_eq!((small.entries()[0].compression(), small.entries()[1].compression()), (Some(Method::Deflated), Some(Method::Stored)));
}

#[test]
fn a_replace_refuses_what_a_copy_refuses_and_a_folder() {
    let original = build(&[Item::stored("mimetype", b"m"), Item::stored("Configurations2/", b""), Item::stored("a.xml", b"<a/>")]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    let mut work = Work::default();
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.replace(&archive, 1, b"data", &mut work).unwrap_err().class(), "zip_corrupt", "a folder holds no data");
    assert_eq!(writer.replace(&archive, 9, b"data", &mut work).unwrap_err().class(), "zip_corrupt");
    assert!(writer.is_empty(), "nothing is written by a refused replace");
    // A name written twice is refused at the end, as for a copy.
    writer.replace(&archive, 2, b"one", &mut work).unwrap();
    writer.replace(&archive, 2, b"two", &mut work).unwrap();
    assert_eq!(writer.finish().unwrap_err().class(), "duplicate_part");
}

#[test]
fn a_replaced_entry_keeps_its_flags_but_the_data_descriptor_bit() {
    // The flags of another writer's row: the UTF-8 name bit stays; the descriptor bit goes, because the replaced entry's sizes are in its
    // local header and no descriptor follows its data.
    let content = b"<office:document-content>described</office:document-content>".repeat(4);
    let base = Item::deflated("content.xml", &content);
    let described = Item {
        local_sizes: Some((0, 0, 0)),
        after_data: [&base.crc.to_le_bytes()[..], &(base.data.len() as u32).to_le_bytes(), &(base.size as u32).to_le_bytes()].concat(),
        ..base.clone().with_flags(0x0808)
    };
    let utf8 = Item::stored("u.xml", b"<u/>").with_flags(0x0800);
    let odd = Item::stored("o.xml", b"<o/>").with_flags(0x0004);
    let original = build(&[described, utf8, odd]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    let flags = |index: usize| archive.entries()[index].flags;
    assert_eq!((flags(0), flags(1), flags(2)), (0x0808, 0x0800, 0x0004));
    let mut writer = ArchiveWriter::new();
    for index in 0..3 {
        writer.replace(&archive, index, b"<new/>", &mut Work::default()).unwrap();
    }
    let copy = writer.finish().unwrap();
    let back = Archive::read(&copy, &Limits::default()).unwrap();
    assert_eq!(back.entries().iter().map(|e| e.flags).collect::<Vec<_>>(), [0x0800, 0x0800, 0x0004]);
    assert_eq!(read_all(&copy).unwrap().iter().map(|(_, bytes)| bytes.as_slice()).collect::<Vec<_>>(), [b"<new/>"; 3]);
}

#[test]
fn a_replaced_name_from_a_unicode_path_field_is_held_to_the_duplicate_rule_by_both_its_names() {
    // As for a copy: the name as written is `a.xml` and the field gives `b.xml`, so an entry added under either is the same name twice, and an
    // entry whose written name is its own name needs no second spelling.
    let original = build(&[Item::stored("a.xml", b"<a/>").with_unicode_path("b.xml"), Item::stored("plain.xml", b"<p/>")]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    assert_eq!(archive.entries()[0].name, "b.xml");
    for other in ["a.xml", "b.xml"] {
        let mut writer = ArchiveWriter::new();
        writer.replace(&archive, 0, b"<new/>", &mut Work::default()).unwrap();
        writer.add(other, b"<x/>", Method::Stored).unwrap();
        assert_eq!(writer.finish().unwrap_err().class(), "duplicate_part", "{other}");
    }
    let mut writer = ArchiveWriter::new();
    writer.replace(&archive, 1, b"<new/>", &mut Work::default()).unwrap();
    writer.add("a.xml", b"<x/>", Method::Stored).unwrap();
    assert!(writer.finish().is_ok(), "an entry named as it is written holds one name only");
}

/// An archive of `entries` folders (zero-length, each with a local header whose extra field is `records` empty records) and one file.
fn empty_records(entries: usize, records: usize) -> Vec<u8> {
    let area: Vec<u8> = (0..records).flat_map(|_| extra_record(0x9999, &[])).collect();
    let mut items: Vec<Item> = (0..entries).map(|i| Item::stored(&format!("d{i}/"), b"").with_local_extra(&area)).collect();
    items.push(Item::stored("content.xml", b"<a/>"));
    build(&items)
}

#[test]
fn the_extra_field_walk_of_a_copy_is_counted_in_the_unit_it_walks() {
    // The copy looks through each entry's local extra field for a ZIP64 record, and the account looks again; neither was
    // counted, so 131 MB of empty records (2,000 folders of 16,383) took 244 ms and 273 ms and said nothing. Each record each walk looks at
    // is `local_extra_records`; the ratio between 100 and 400 folders is the ratio of the records.
    let cost = |entries: usize| {
        let bytes = empty_records(entries, 1_000);
        let archive = Archive::read(&bytes, &Limits::default()).unwrap();
        let (mut copy, mut account) = (Work::default(), Work::default());
        let mut writer = ArchiveWriter::new();
        for i in 0..archive.entries().len() {
            writer.copy_counted(&archive, i, &mut copy).unwrap();
        }
        copy_account_counted(&archive, &mut account).unwrap();
        (copy.local_extra_records, account.local_extra_records)
    };
    let (small, big) = (cost(100), cost(400));
    assert_eq!(small, (100_000, 100_000), "each record of each folder once by the copy and once by the account");
    assert_eq!(big, (400_000, 400_000));
    // An entry written under headers made anew is walked again: its local extra field and its row's are cut of ZIP64 records.
    let area = extra_record(0x9999, &[]).repeat(50);
    let disagreeing = Item { local_name: Some(b"b.xml".to_vec()), ..Item::stored("a.xml", b"<a/>").with_local_extra(&area).with_extra(&area) };
    let bytes = build(&[Item::stored("mimetype", b"m"), disagreeing]);
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    let mut work = Work::default();
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy_counted(&archive, 1, &mut work).unwrap(), Copied::Rewritten);
    assert_eq!(work.local_extra_records, 50 + 50 + 50, "the ZIP64 look, then the local and the row's extra fields as they are cut");
}

/// The scan reads the manifests of OpenOffice.org as the tree reader does, when the caller asks: one DOCTYPE before the root with
/// a name and an external id and no internal subset, which declares nothing. Without the option every DTD is refused, and with it everything else the tree
/// reader refuses after one is: an internal subset, a second declaration, one after the root, and one not written `<!DOCTYPE` and a space.
#[test]
fn a_scan_that_is_asked_to_accepts_the_external_doctype_the_tree_reader_accepts_and_nothing_else() {
    use diacraft_container::xml::ParseOptions;
    use diacraft_container::xml_scan::scan_tags_with;
    let asked = ParseOptions { allow_external_doctype: true };
    let scan = |bytes: &[u8], options: ParseOptions| -> Result<usize, String> {
        let mut tags = 0usize;
        scan_tags_with::<()>(bytes, &XmlLimits::default(), options, &mut Work::default(), |_| {
            tags += 1;
            Ok(())
        })
        .map(|()| tags)
        .map_err(|e| match e {
            ScanError::Xml(e) => e.class().to_owned(),
            ScanError::Visit(()) => "visit".to_owned(),
        })
    };
    let parses = |bytes: &[u8], options: ParseOptions| {
        diacraft_container::xml::parse_with_options(bytes, &XmlLimits::default(), options, &mut Work::default()).is_ok()
    };
    let public = b"<?xml version=\"1.0\"?>\n<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">\n<manifest:manifest/>";
    let system = b"<!DOCTYPE m SYSTEM 'm.dtd'><m><n/></m>";
    assert_eq!(scan(public, asked), Ok(1));
    assert_eq!(scan(system, asked), Ok(3), "m, n and the end of m");
    assert_eq!(scan(public, ParseOptions::default()), Err("xml_entity_refused".to_owned()), "refused unless asked for");
    assert_eq!(
        scan_tags::<()>(public, &XmlLimits::default(), &mut Work::default(), |_| Ok(())),
        Err(ScanError::Xml(XmlError::EntityRefused { offset: 22, what: "a DTD".to_owned() }))
    );
    let refused: [(&str, &[u8]); 7] = [
        ("an internal subset", b"<!DOCTYPE m [<!ENTITY x \"y\">]><m/>"),
        ("an external id and an internal subset", b"<!DOCTYPE m SYSTEM \"m.dtd\" [<!ENTITY x \"y\">]><m/>"),
        ("a name alone", b"<!DOCTYPE m><m/>"),
        ("a second declaration", b"<!DOCTYPE m SYSTEM \"a\"><!DOCTYPE m SYSTEM \"b\"><m/>"),
        ("one after the root", b"<m/><!DOCTYPE m SYSTEM \"a\">"),
        ("one inside the root", b"<m><!DOCTYPE m SYSTEM \"a\"></m>"),
        ("a keyword with no space", b"<!DOCTYPEm SYSTEM \"a\"><m/>"),
    ];
    for (what, bytes) in refused {
        assert!(scan(bytes, asked).is_err(), "{what}: the scan accepted it");
        assert!(!parses(bytes, asked), "{what}: and the tree reader refuses it too");
    }
    // What the scan accepts the tree reader accepts, and the other way round, for each form the two were given.
    for bytes in [&public[..], &system[..]] {
        assert!(parses(bytes, asked) && scan(bytes, asked).is_ok());
    }
    // A reference to an entity that was not declared is still refused after an accepted declaration by the tree reader (which reads a part before any scan
    // of it is made: the scan builds no tree and checks no reference, with or without a declaration).
    let after = b"<!DOCTYPE m SYSTEM \"a\"><m>&x;</m>";
    assert!(!parses(after, asked));
    // A byte order mark in front moves nothing: the declaration is found where it is.
    let mut with_mark = vec![0xEF, 0xBB, 0xBF];
    with_mark.extend_from_slice(system);
    assert_eq!(scan(&with_mark, asked), Ok(3));
    assert!(scan(&[&[0xEF, 0xBB, 0xBF][..], b"<!DOCTYPEm SYSTEM \"a\"><m/>"].concat(), asked).is_err());
}
