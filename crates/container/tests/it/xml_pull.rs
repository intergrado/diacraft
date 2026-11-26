//! The stream of an XML document: the children of the root one at a time, with what a tree would refuse refused, and the writer that makes a part
//! the same bytes a tree writes. A part of the codec is the root and a very long list of shapes, and a tree of it is what bounded a page; a stream holds one child.

use diacraft_container::XmlLimits;
use diacraft_container::xml::{El, Node, ParseOptions, PullLimits, RootWriter, XmlError, XmlText, parse_with, to_bytes};

fn limits() -> PullLimits {
    PullLimits { xml: XmlLimits::default(), max_document_nodes: usize::MAX }
}

/// Every child element of the root of `doc` as a stream gives them, and the root's head, or the error.
fn pulled(doc: &[u8], limits: &PullLimits) -> Result<(El, Vec<El>), XmlError> {
    let text = XmlText::new(doc)?;
    let (mut pull, head) = text.pull(limits, ParseOptions::default())?;
    let mut kids = Vec::new();
    while let Some(child) = pull.next_child()? {
        kids.push(child);
    }
    pull.finish()?;
    Ok((head, kids))
}

/// What the tree parse of `doc` makes of the same: the root without its children, and its child elements.
fn treed(doc: &[u8]) -> Result<(El, Vec<El>), XmlError> {
    let root = parse_with(doc, &XmlLimits::default())?;
    let kids = root.elements().cloned().collect();
    Ok((El { name: root.name, attrs: root.attrs, children: Vec::new() }, kids))
}

const DOCS: [&str; 12] = [
    "<a/>",
    "<a x=\"1\" y=\"two &amp; three\"/>",
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<!-- before --><a xmlns:p=\"urn:x\"><p:b q=\"1\"><c/>text<d>deep &lt; text</d></p:b><e/></a><!-- after -->\n",
    "<a>text<b/>more text<![CDATA[c<d]]><b/><!-- k --><?pi data?><b>x</b></a>",
    "<a><b/><b/><b/><b/><b/><b/><b/><b/><b/><b/></a>",
    "<a>  <b>1</b>\n  <b>2</b>\n</a>",
    "<a><b><c><d><e><f><g/></f></e></d></c></b></a>",
    "<a>&#65;&#x42;<b/></a>",
    "<a>x&amp;y<b/>z</a>",
    "<a b=\"it's\" c='say \"x\"'><b d=\"\r\n\t\"/></a>",
    "\u{FEFF}<a><b/></a>",
    "<a><b/><b>\u{4E00}\u{1F600}</b></a>",
];

#[test]
fn a_stream_gives_the_root_head_and_each_child_a_tree_would_hold_in_the_same_order() {
    for doc in DOCS {
        assert_eq!(pulled(doc.as_bytes(), &limits()), treed(doc.as_bytes()), "{doc:?}");
    }
}

#[test]
fn a_utf16_document_is_streamed_as_its_tree_is() {
    let doc = "<a x=\"\u{e9}\"><b/><c>\u{4E00}</c></a>";
    for little in [true, false] {
        let mut bytes: Vec<u8> = if little { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
        bytes.extend(doc.encode_utf16().flat_map(|u| if little { u.to_le_bytes() } else { u.to_be_bytes() }));
        assert_eq!(pulled(&bytes, &limits()), treed(&bytes), "little={little}");
    }
}

#[test]
fn what_a_tree_refuses_a_stream_refuses_and_the_children_before_the_damage_are_given() {
    // The same documents as a tree reads them, damaged at the end: the error is the tree's, found after the children that came before.
    let cases: [&str; 8] = [
        "<a><b/><c></a>",
        "<a><b/>",
        "<a><b/></a><a/>",
        "<a><b/></a>text",
        "<a><b/></a></a>",
        "<a><b>&x;</b></a>",
        "<a><b/><!-- a -- b --></a>",
        "<a><b/><c x=\"1\" x=\"2\"/></a>",
    ];
    for doc in cases {
        let tree = parse_with(doc.as_bytes(), &XmlLimits::default());
        let stream = pulled(doc.as_bytes(), &limits());
        assert_eq!(tree.map(drop).map_err(|e| e.class()), stream.map(drop).map_err(|e| e.class()), "{doc:?}");
    }
    // The first child is given before the document is found damaged.
    let text = XmlText::new(b"<a><b/><c></a>").unwrap();
    let (mut pull, _) = text.pull(&limits(), ParseOptions::default()).unwrap();
    assert_eq!(pull.next_child().unwrap().map(|e| e.name), Some("b".to_owned()));
    assert!(pull.next_child().is_err());
}

#[test]
fn a_dtd_is_refused_before_the_root_is_given_and_a_document_with_no_root_is_refused() {
    let text = XmlText::new(b"<!DOCTYPE a [<!ENTITY x \"y\">]><a/>").unwrap();
    assert_eq!(text.pull(&limits(), ParseOptions::default()).err().map(|e| e.class()), Some("xml_entity_refused"));
    let text = XmlText::new(b"<?xml version=\"1.0\"?>  ").unwrap();
    assert_eq!(text.pull(&limits(), ParseOptions::default()).err().map(|e| e.class()), Some("xml_malformed"));
    assert!(XmlText::new(&[0xFF, 0xFE, 0x00, 0x00, b'<', 0, 0, 0]).is_err(), "UTF-32 is no encoding of this reader");
}

#[test]
fn the_limit_of_items_is_one_childs_and_not_the_documents_and_the_documents_has_its_own() {
    // 100,000 children of three items each (the element and its attribute... two items, and one of text): far past a limit of 50 items a tree may have.
    let body: String = (0..100_000).map(|i| format!("<b n=\"{i}\"/>")).collect();
    let doc = format!("<a>{body}</a>");
    let small = PullLimits { xml: XmlLimits { max_nodes: 50, ..XmlLimits::default() }, max_document_nodes: usize::MAX };
    let tree = parse_with(doc.as_bytes(), &small.xml);
    assert!(matches!(tree, Err(XmlError::TooManyNodes { limit: 50 })), "{tree:?}");
    let (head, kids) = pulled(doc.as_bytes(), &small).unwrap();
    assert_eq!((head.name.as_str(), kids.len()), ("a", 100_000));
    // One child past the limit is refused as a tree is: the items of that child, counted from its own start.
    let big = format!("<a><b/><c>{}</c><b/></a>", "<d/>".repeat(60));
    assert!(matches!(pulled(big.as_bytes(), &small), Err(XmlError::TooManyNodes { limit: 50 })));
    let fits = format!("<a><b/><c>{}</c><b/></a>", "<d/>".repeat(48));
    assert_eq!(
        pulled(fits.as_bytes(), &small).unwrap().1.len(),
        3,
        "an element with 48 children is 49 items and an attribute-free root's start was not counted against it"
    );
    // The document has a limit of its own: every item, in the children and outside them. Each `<b n="..."/>` is two items, and the root is one.
    let capped = PullLimits { xml: XmlLimits::default(), max_document_nodes: 200_001 };
    assert_eq!(pulled(doc.as_bytes(), &capped).unwrap().1.len(), 100_000);
    let capped = PullLimits { max_document_nodes: 200_000, ..capped };
    assert!(matches!(pulled(doc.as_bytes(), &capped), Err(XmlError::TooManyNodes { limit: 200_000 })));
}

#[test]
fn the_items_a_stream_lets_go_of_count_toward_the_document_and_not_toward_the_next_child() {
    // A run of text, a comment and an instruction under the root are three items of the document; the child that follows has the limit to itself.
    let doc = "<a>t<!--c--><?p d?><b><c/></b></a>";
    let exact = PullLimits { xml: XmlLimits { max_nodes: 2, ..XmlLimits::default() }, max_document_nodes: 1 + 3 + 2 };
    assert_eq!(pulled(doc.as_bytes(), &exact).unwrap().1.len(), 1);
    let one_less = PullLimits { max_document_nodes: 5, ..exact };
    assert!(matches!(pulled(doc.as_bytes(), &one_less), Err(XmlError::TooManyNodes { limit: 5 })));
    // Text that is split by a reference is one run, as a tree counts it.
    let split = "<a>x&amp;y</a>";
    let two = PullLimits { xml: XmlLimits::default(), max_document_nodes: 2 };
    assert!(pulled(split.as_bytes(), &two).is_ok());
    let one = PullLimits { max_document_nodes: 1, ..two };
    assert!(pulled(split.as_bytes(), &one).is_err());
}

#[test]
fn the_depth_is_counted_from_the_root_as_a_tree_counts_it() {
    let doc = "<a><b><c/></b></a>";
    for max_depth in [1usize, 2, 3, 4] {
        let l = PullLimits { xml: XmlLimits { max_depth, ..XmlLimits::default() }, max_document_nodes: usize::MAX };
        assert_eq!(
            parse_with(doc.as_bytes(), &l.xml).map(drop).map_err(|e| e.class()),
            pulled(doc.as_bytes(), &l).map(drop).map_err(|e| e.class()),
            "{max_depth}"
        );
    }
}

#[test]
fn a_part_of_a_million_children_is_a_million_small_trees_and_the_root_never_holds_them() {
    let body: String = (0..1_000_000).map(|i| format!("<b n=\"{}\"/>", i % 7)).collect();
    let doc = format!("<a>{body}</a>");
    drop(body);
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let (mut pull, head) = text.pull(&limits(), ParseOptions::default()).unwrap();
    assert!(head.children.is_empty());
    let mut n = 0u64;
    while let Some(child) = pull.next_child().unwrap() {
        n += 1;
        assert!(child.children.is_empty());
    }
    assert_eq!(n, 1_000_000);
    pull.finish().unwrap();
    // The work is the document's events and no more: one for each tag.
    assert_eq!(pull.events(), 1_000_000 + 2 + 1);
}

#[test]
fn a_part_written_one_child_at_a_time_is_the_bytes_a_tree_writes() {
    for doc in DOCS {
        let root = parse_with(doc.as_bytes(), &XmlLimits::default()).unwrap();
        for declaration in [true, false] {
            let whole = to_bytes(&root, &[], &[], declaration).unwrap();
            let head = El { name: root.name.clone(), attrs: root.attrs.clone(), children: Vec::new() };
            let mut writer = RootWriter::begin(&head, declaration).unwrap();
            for child in &root.children {
                writer.child(child).unwrap();
            }
            assert!(!writer.is_empty());
            assert_eq!(String::from_utf8_lossy(&writer.finish()), String::from_utf8_lossy(&whole), "{doc:?} declaration={declaration}");
        }
    }
}

#[test]
fn a_writer_refuses_what_the_tree_writer_refuses() {
    assert!(RootWriter::begin(&El { name: "1a".to_owned(), ..El::default() }, false).is_err());
    let twice = El { name: "a".to_owned(), attrs: vec![("x".to_owned(), "1".to_owned()), ("x".to_owned(), "2".to_owned())], children: Vec::new() };
    assert!(RootWriter::begin(&twice, false).is_err());
    let mut writer = RootWriter::begin(&El { name: "a".to_owned(), ..El::default() }, false).unwrap();
    assert!(writer.child(&Node::Text("a\u{0}b".to_owned())).is_err());
    assert!(writer.child(&Node::Comment("a--b".to_owned())).is_err());
    assert!(writer.child(&Node::Pi { target: "xml".to_owned(), data: String::new() }).is_err());
    // `<a>`: the first child tried opens the root, and a child that is refused writes nothing.
    assert_eq!(writer.len(), 3);
}

#[test]
fn what_a_stream_costs_in_events_is_what_a_tree_costs_and_a_refused_stream_has_counted_what_it_read() {
    let doc = DOCS[3];
    let mut work = diacraft_container::Work::default();
    diacraft_container::xml::parse_counted(doc.as_bytes(), &XmlLimits::default(), &mut work).unwrap();
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let (mut pull, _) = text.pull(&limits(), ParseOptions::default()).unwrap();
    pull.finish().unwrap();
    assert_eq!(pull.events(), work.xml_events);
}

#[test]
fn the_limit_of_the_next_child_is_set_between_children_and_the_items_of_the_last_one_are_told() {
    // Children of 10, 60 and 200 items (an element and its elements), and a limit that is made larger and smaller as they are read.
    let doc = format!("<a><b>{}</b><b>{}</b><b>{}</b></a>", "<d/>".repeat(9), "<d/>".repeat(59), "<d/>".repeat(199));
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let begun = PullLimits { xml: XmlLimits { max_nodes: 50, ..XmlLimits::default() }, max_document_nodes: usize::MAX };
    let (mut pull, _) = text.pull(&begun, ParseOptions::default()).unwrap();
    assert_eq!(pull.last_child_items(), 0, "none given yet");
    assert!(pull.next_child().unwrap().is_some());
    assert_eq!(pull.last_child_items(), 10);
    // The second is 60 items: refused at 50, and read at 60 when the limit is raised before it is read.
    pull.set_child_items(60);
    assert!(pull.next_child().unwrap().is_some());
    assert_eq!(pull.last_child_items(), 60);
    // The third is 200: refused at 199 and read at 200.
    pull.set_child_items(199);
    assert!(matches!(pull.next_child(), Err(XmlError::TooManyNodes { limit: 199 })));
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let (mut pull, _) = text.pull(&begun, ParseOptions::default()).unwrap();
    pull.set_child_items(10);
    assert!(pull.next_child().unwrap().is_some());
    assert!(
        matches!(pull.next_child(), Err(XmlError::TooManyNodes { limit: 10 })),
        "a limit that is lowered between children refuses the child past it"
    );
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let (mut pull, _) = text.pull(&begun, ParseOptions::default()).unwrap();
    pull.set_child_items(200);
    let mut seen = Vec::new();
    while pull.next_child().unwrap().is_some() {
        seen.push(pull.last_child_items());
    }
    assert_eq!(seen, [10, 60, 200]);
}

#[test]
fn a_stream_says_how_far_into_the_text_it_has_read() {
    // A reader that caps the text of each child of the root (a page of a draw.io file) measures it by the position before and after the child.
    let doc = "<a x=\"1\"><b/><c>text</c>  <d><e/></d></a>";
    let text = XmlText::new(doc.as_bytes()).unwrap();
    let (mut pull, _) = text.pull(&limits(), ParseOptions::default()).unwrap();
    let head = pull.position();
    assert_eq!(head, "<a x=\"1\">".len(), "the root's start tag is read, and nothing of its children");
    let mut ends = Vec::new();
    while let Some(child) = pull.next_child().unwrap() {
        ends.push((child.name, pull.position()));
    }
    // Each child ends where its markup ends in the text, the text between children read with the next.
    let want = |s: &str| doc.find(s).unwrap() + s.len();
    assert_eq!(ends, [("b".to_owned(), want("<b/>")), ("c".to_owned(), want("</c>")), ("d".to_owned(), want("</d>"))]);
    pull.finish().unwrap();
    assert_eq!(pull.position(), doc.len(), "the whole document, once it is finished");
    // In UTF-16 the position is in the decoded text, the same for the same document.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(doc.encode_utf16().flat_map(u16::to_le_bytes));
    let text = XmlText::new(&bytes).unwrap();
    let (mut pull, _) = text.pull(&limits(), ParseOptions::default()).unwrap();
    while pull.next_child().unwrap().is_some() {}
    pull.finish().unwrap();
    assert_eq!(pull.position(), doc.len());
}
