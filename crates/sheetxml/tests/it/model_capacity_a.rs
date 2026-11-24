//! Part A: the findings of the narrow re-review (26776f5), each with a test written to fail on that commit.
//!
//! * **Depth.** `encode_for_save` passed parts that an open at `open_limits()` refused for depth (the open kept the container's default of 200 levels, and a
//!   part of 200 nested shapes is 205 levels deep).
//! * **A processing instruction no writer can write.** The reader takes `<??>` and `<?1a d?>`; the decoder kept it, and the save could not write it.
use std::sync::Arc;

use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{El, Node, XmlLimits, parse_with, to_bytes};
use diacraft_container::{Limits, xml};
use diacraft_doc::{Document, Page};
use diacraft_sheetxml::model::{
    Budget, CodecError, DecodeNote, MemBlobs, URN, check_decodes, decode, decode_page, encode_for_save, open_limits, part_lists, preflight,
};

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

fn chain(open: &str, close: &str, depth: usize, inner: &str) -> String {
    format!("{}{}{}", open.repeat(depth), inner, close.repeat(depth))
}

fn page_text(inner: &str) -> String {
    format!("<page xmlns=\"{URN}\" id=\"1\">{inner}</page>")
}

fn parse(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

fn page_of(inner: &str) -> Result<(Page, Vec<DecodeNote>), CodecError> {
    let mut notes = Vec::new();
    let page = decode_page(&parse(&page_text(inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes)?;
    Ok((page, notes))
}

fn document_of(page: Page) -> Document {
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    doc
}

/// The depth of a tree, the root being level 1.
fn depth_of(root: &El) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root, 1usize)];
    while let Some((e, d)) = stack.pop() {
        deepest = deepest.max(d);
        for c in e.elements() {
            stack.push((c, d + 1));
        }
    }
    deepest
}

/// A save at `limits`, and then the open: every part written is read back at the same limits and decoded within the budget of those limits.
fn save_then_open(doc: &Document, limits: &Limits) -> Result<(), String> {
    let mut blobs = MemBlobs::new();
    let encoded = encode_for_save(doc, &mut blobs, limits).map_err(|e| format!("the save refused: {e}"))?;
    let write = |e: &El| to_bytes(e, &[], &[], true).map_err(|e| format!("the writer refused: {e:?}"));
    let reopen = |bytes: &[u8]| parse_with(bytes, &limits.xml).map_err(|e| format!("the open cannot read a part the save passed: {e}"));
    let document = reopen(&write(&encoded.document)?)?;
    let pages: Vec<El> = encoded.pages.iter().map(|(_, e)| write(e).and_then(|b| reopen(&b))).collect::<Result<_, _>>()?;
    let masters: Vec<El> = encoded.masters.iter().map(|(_, e)| write(e).and_then(|b| reopen(&b))).collect::<Result<_, _>>()?;
    let (page_refs, master_refs): (Vec<&El>, Vec<&El>) = (pages.iter().collect(), masters.iter().collect());
    let back = decode(&document, &page_refs, &master_refs, &blobs, &mut Budget::for_open(limits), &mut Vec::new())
        .map_err(|e| format!("the open refused what the save passed: {e}"))?;
    if back != *doc {
        return Err("the open read another drawing than the save wrote".to_owned());
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------------------------ the depth

const CELL: &str = "<sheet><Section N=\"U\"><Row N=\"r\"><Cell N=\"c\"/></Row></Section></sheet>";

#[test]
fn the_open_limits_read_the_depth_the_codec_writes() {
    // A part of the codec is as deep as 200 nested shapes and a kept element at its own cap of 200 levels, and the container's ceiling is what holds it.
    assert_eq!(open_limits().xml.max_depth, XML_DEPTH_CEILING);
    const { assert!(XML_DEPTH_CEILING >= 2 * diacraft_doc::MAX_SHAPE_DEPTH + 8) };
}

#[test]
fn a_part_of_200_nested_shapes_with_a_cell_in_the_deepest_is_saved_and_opens_at_the_open_limits() {
    let (page, notes) = page_of(&chain("<shape>", "</shape>", 200, CELL)).expect("the model's own depth cap decodes");
    assert!(notes.is_empty(), "{notes:?}");
    let doc = document_of(page);
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("a drawing the model allows is saved");
    assert!(depth_of(&encoded.pages[0].1) > 200, "the part is deeper than the container's default");
    save_then_open(&doc, &open_limits()).unwrap();
}

#[test]
fn a_kept_chain_and_an_unknown_chain_at_their_caps_are_saved_and_open_at_the_open_limits() {
    for d in [190usize, 197, 198, 199, 200] {
        for (what, inner) in [
            ("kept", format!("<shape><kept>{}</kept></shape>", chain("<el name=\"a\">", "</el>", d, ""))),
            ("unknown", format!("<shape>{}</shape>", chain("<zz>", "</zz>", d, ""))),
        ] {
            let Ok((page, _)) = page_of(&inner) else { continue };
            let doc = document_of(page);
            save_then_open(&doc, &open_limits()).unwrap_or_else(|e| panic!("a {what} chain of {d}: {e}"));
        }
    }
}

#[test]
fn the_deepest_part_the_codec_makes_is_saved_and_opens_at_the_open_limits() {
    // The cap of 200 shapes, with an unknown chain at its cap in the deepest of them and a kept chain in the one above: the deepest the model and the codec allow.
    let inner = chain(
        "<shape>",
        "</shape>",
        199,
        &format!("<kept>{}</kept><shape>{}</shape>", chain("<el name=\"a\">", "</el>", 200, ""), chain("<zz>", "</zz>", 199, "")),
    );
    let (page, _) = page_of(&inner).expect("decodes");
    let doc = document_of(page);
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("saved");
    assert!(depth_of(&encoded.pages[0].1) <= XML_DEPTH_CEILING);
    save_then_open(&doc, &open_limits()).unwrap();
}

/// A page of `n` nested shapes and nothing else: `n + 1` levels, the page being the first.
fn nested(n: usize) -> Document {
    document_of(page_of(&chain("<shape>", "</shape>", n, "")).unwrap().0)
}

fn with_depth(max_depth: usize) -> Limits {
    Limits { xml: XmlLimits { max_depth, ..open_limits().xml }, ..open_limits() }
}

#[test]
fn a_part_as_deep_as_the_limit_is_saved_and_one_level_deeper_is_refused_in_words() {
    // The root is level 1: a page of 9 nested shapes is 10 levels, and the reader refuses an element at level `max_depth + 1`.
    let doc = nested(9);
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).unwrap();
    assert_eq!(depth_of(&encoded.pages[0].1), 10);
    assert_eq!(save_then_open(&doc, &with_depth(10)), Ok(()));
    let refused = encode_for_save(&doc, &mut MemBlobs::new(), &with_depth(9)).expect_err("one level too deep");
    assert_eq!(refused, CodecError::Limit { what: "levels of nested elements in one part", max: 9 });
    assert_eq!(refused.to_string(), "the drawing holds more than 9 levels of nested elements in one part");
    assert_eq!(refused.class(), "model_too_complex");
    // And it is what the reader says: a part that the save refused is one the open would not have read.
    let bytes = to_bytes(&encoded.pages[0].1, &[], &[], true).unwrap();
    assert!(matches!(parse_with(&bytes, &with_depth(9).xml), Err(xml::XmlError::TooDeep { limit: 9 })));
    assert!(parse_with(&bytes, &with_depth(10).xml).is_ok());
}

#[test]
fn a_limit_deeper_than_the_containers_ceiling_is_the_ceiling() {
    // The reader never goes past the ceiling whatever the limit says, so the check does not either: a part of 513 levels is refused although the limit says
    // 1,000. (The codec cannot make one, its caps being 200 shapes and 200 kept levels; the part is built by hand.)
    let deep = |levels: usize| {
        let mut e = El { name: "diacraft:page".to_owned(), attrs: vec![("xmlns:diacraft".to_owned(), URN.to_owned())], children: Vec::new() };
        for _ in 1..levels {
            e = El { name: "diacraft:zz".to_owned(), attrs: Vec::new(), children: vec![Node::El(e)] };
        }
        e
    };
    let document = El { name: "diacraft:document".to_owned(), attrs: vec![("xmlns:diacraft".to_owned(), URN.to_owned())], children: Vec::new() };
    let encoded =
        |levels: usize| diacraft_sheetxml::model::Encoded { document: document.clone(), pages: vec![(1, deep(levels))], masters: Vec::new() };
    let ceiling = XML_DEPTH_CEILING;
    let max = u64::try_from(ceiling).unwrap();
    let blobs = MemBlobs::new();
    let refused = preflight(&encoded(ceiling + 1), &blobs, &with_depth(1_000));
    assert_eq!(refused, Err(CodecError::Limit { what: "levels of nested elements in one part", max }));
    // A part exactly at the ceiling passes the depth check (and meets the next one, which says the lists of the document part name no such page).
    let at = preflight(&encoded(ceiling), &blobs, &with_depth(1_000));
    assert!(!matches!(at, Err(CodecError::Limit { what: "levels of nested elements in one part", .. })), "{at:?}");
}

#[test]
fn the_depth_of_a_master_and_of_the_document_part_is_held_to_the_limit_too() {
    let mut doc = Document::empty();
    let (page, _) = page_of(&chain("<shape>", "</shape>", 9, "")).unwrap();
    doc.masters.push(Arc::new(diacraft_doc::Master { id: 5, shapes: page.shapes.clone(), ..diacraft_doc::Master::default() }));
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).unwrap();
    assert_eq!(depth_of(&encoded.masters[0].1), 10);
    assert!(encode_for_save(&doc, &mut MemBlobs::new(), &with_depth(10)).is_ok());
    assert!(encode_for_save(&doc, &mut MemBlobs::new(), &with_depth(9)).is_err());
}

// ------------------------------------------------------------------------------------------------------ a processing instruction no writer writes

/// What the save makes of a page and what the open makes of that, for the page that holds `inner`.
fn through_save(inner: &str) -> (Vec<DecodeNote>, Result<(), String>) {
    let (page, notes) = page_of(inner).expect("decodes");
    (notes, save_then_open(&document_of(page), &open_limits()))
}

#[test]
fn a_processing_instruction_with_no_target_is_dropped_with_a_note_and_the_drawing_saves() {
    for (what, inner) in [
        ("an empty target in an unknown element of a shape", "<shape><zz><??></zz></shape>"),
        ("a digit first", "<shape><zz><?1a d?></zz></shape>"),
        ("an unknown element of the page", "<zz><??></zz>"),
        ("an unknown element of a sheet", "<shape><sheet><zz><??></zz></sheet></shape>"),
        ("an unknown element of a connect", "<connect><zz><??></zz></connect>"),
        ("a namespace on the element", "<shape><q:zz xmlns:q=\"urn:q\"><??></q:zz></shape>"),
    ] {
        let (notes, saved) = through_save(inner);
        assert_eq!(saved, Ok(()), "{what}");
        assert_eq!(notes, vec![DecodeNote::Unknown { count: 1 }], "{what}: the instruction is counted as unknown");
    }
}

#[test]
fn a_processing_instruction_with_a_target_is_kept_and_written_back() {
    for inner in [
        "<shape><zz><?t d?></zz></shape>",
        "<shape><zz><?x:y d?></zz></shape>",
        "<shape><zz><?a-b.c d?></zz></shape>",
        "<shape><zz><?\u{e9}t d?></zz></shape>",
        "<shape><zz><?t?></zz></shape>",
    ] {
        let (notes, saved) = through_save(inner);
        assert_eq!(saved, Ok(()), "{inner}");
        assert!(notes.is_empty(), "{inner}: {notes:?}");
    }
}

#[test]
fn an_unknown_instruction_in_an_element_is_charged_its_item_whether_it_is_kept_or_dropped() {
    let items = |inner: &str| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        decode_page(&parse(&page_text(inner)), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.items
    };
    let kept = items("<shape><zz><?t d?></zz></shape>");
    let dropped = items("<shape><zz><??></zz></shape>");
    // Dropped, the node still cost the parse an item; the note costs one more.
    assert_eq!(dropped, kept + 1);
}

#[test]
fn a_comment_the_writer_cannot_write_is_not_one_the_reader_takes() {
    // The comments the writer refuses hold `--` or end in `-`. The reader refuses the same, so no decode keeps one.
    for comment in ["<!-- a --->", "<!--a-- b-->", "<!--->"] {
        let text = format!("<page xmlns=\"{URN}\" id=\"1\"><shape><zz>{comment}</zz></shape></page>");
        assert!(parse_with(text.as_bytes(), &native_limits()).is_err(), "{comment}");
    }
}

/// A page whose unknown element holds, in its model, the node `make` gives (built by other means than this decoder), and what a save makes of it.
fn save_of_unknown_node(make: impl Fn() -> diacraft_doc::RawNode) -> Result<(), CodecError> {
    let (mut page, _) = page_of("<shape><zz><?t d?></zz></shape>").unwrap();
    let shape = Arc::make_mut(page.shapes.get_mut(0).expect("the shape"));
    let mut changed = 0;
    for placed in shape.ext.make_mut().native.kept.nodes_mut() {
        if let diacraft_doc::RawNode::Element(e) = &mut placed.node {
            for child in &mut e.children {
                *child = make();
                changed += 1;
            }
        }
    }
    assert_eq!(changed, 1);
    encode_for_save(&document_of(page), &mut MemBlobs::new(), &open_limits()).map(drop)
}

#[test]
fn the_encoder_refuses_in_words_an_instruction_it_cannot_write_and_takes_one_it_can() {
    use diacraft_doc::RawNode;
    let pi = |target: &'static str, data: &'static str| move || RawNode::Pi { target: Arc::from(target), data: Arc::from(data) };
    // A model that holds one is refused where it is written, in words, and not as a node the writer refuses: a target that is no name, `xml` in any case, data that
    // ends the instruction, and a character XML forbids.
    for (what, target, data) in [
        ("a target that is no name", "1bad", ""),
        ("an empty target", "", "d"),
        ("the target xml", "xml", "d"),
        ("the target XmL", "XmL", "d"),
        ("data that ends the instruction", "t", "a?>b"),
        ("a character XML forbids", "t", "a\u{1}b"),
    ] {
        let refused = save_of_unknown_node(pi(target, data)).expect_err(what);
        assert!(matches!(refused, CodecError::Malformed { .. }), "{what}: {refused:?}");
        assert!(refused.to_string().contains("processing instruction"), "{what}: {refused}");
    }
    for (target, data) in [("t", ""), ("t", "d"), ("a-b.c", "x ? y"), ("x:y", "z")] {
        assert!(save_of_unknown_node(pi(target, data)).is_ok(), "{target} {data}");
    }
    // The same for a comment.
    let comment = |text: &'static str| move || RawNode::Comment(Arc::from(text));
    for (what, text) in [("two dashes", "a--b"), ("a dash at the end", "a-"), ("a character XML forbids", "a\u{1}b")] {
        let refused = save_of_unknown_node(comment(text)).expect_err(what);
        assert!(matches!(refused, CodecError::Malformed { .. }) && refused.to_string().contains("comment"), "{what}: {refused:?}");
    }
    for text in ["", " a ", "a-b", "x - y"] {
        assert!(save_of_unknown_node(comment(text)).is_ok(), "{text:?}");
    }
}

#[test]
fn the_instruction_the_fuzzer_found_decodes_and_saves() {
    // `crash-pi-empty-target.bin` of the re-review: a master part with an empty-target instruction in an unknown element.
    let part = format!("<master xmlns=\"{URN}\" id=\"7\"><shape><zz><??></zz></shape></master>");
    let document = format!("<document xmlns=\"{URN}\"><master-ref id=\"7\" part=\"masters/master7.xml\"/></document>");
    let (document, master) = (parse(&document), parse(&part));
    let lists = part_lists(&document).unwrap();
    assert_eq!(lists.masters.len(), 1);
    let blobs = MemBlobs::new();
    let mut notes = Vec::new();
    let doc = decode(&document, &[], &[&master], &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).unwrap();
    assert_eq!(notes, vec![DecodeNote::Unknown { count: 1 }]);
    assert_eq!(save_then_open(&doc, &open_limits()), Ok(()));
    // A check that keeps nothing says the same.
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).unwrap();
    let masters: Vec<&El> = encoded.masters.iter().map(|(_, e)| e).collect();
    check_decodes(&encoded.document, &[], &masters, &blobs, &mut Budget::for_open(&open_limits()), &mut Vec::new()).unwrap();
}

// ------------------------------------------------------------------------------------------------------ the charge of a kept attribute's push

/// The bytes a decode of a page charged for a kept element that holds `k` attributes in its data form (`kept`, `el`, `attr`).
fn kept_attr_charge(k: usize) -> u64 {
    let attrs: String = (0..k).map(|i| format!("<attr name=\"n{i:03}\" value=\"v{i:03}\"/>")).collect();
    let inner = format!("<shape><kept><el name=\"a\">{attrs}</el></kept></shape>");
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    decode_page(&parse(&page_text(&inner)), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
    HUGE - budget.bytes
}

#[test]
fn each_attribute_of_a_kept_element_in_its_data_form_is_charged_the_growth_of_the_list_it_is_pushed_onto() {
    use diacraft_sheetxml::model::cost::{ATTR_PAIR_BYTES, RAW_NODE_BYTES, push_bytes, vec_bytes};
    // The room of the element's children is charged where the element opens, for each child element it has, the `attr` elements included.
    let room = |have: usize| vec_bytes(have + 1, RAW_NODE_BYTES) - vec_bytes(have, RAW_NODE_BYTES);
    // The cost of one attribute besides its push: the second is the first's equal, the list having room for four from the first push.
    let fixed = kept_attr_charge(2) - kept_attr_charge(1);
    assert!(fixed > 0);
    assert_eq!(kept_attr_charge(3) - kept_attr_charge(2), fixed, "the third needs no room");
    assert_eq!(kept_attr_charge(4) - kept_attr_charge(3), fixed, "nor the fourth");
    for have in [0usize, 4, 8, 16] {
        let grown = kept_attr_charge(have + 1) - kept_attr_charge(have);
        assert_eq!(grown, fixed + room(have) + push_bytes(have, ATTR_PAIR_BYTES), "the push onto a list of {have}");
    }
    assert!(push_bytes(4, ATTR_PAIR_BYTES) > 0, "the fifth doubles the room");
}

#[test]
fn an_unknown_comment_that_xml_cannot_write_is_dropped_and_counted_as_lost_and_a_writable_one_is_kept() {
    // `--` inside a comment, and a comment that ends in a dash, are what a tree can hold (the reader refuses them in a file, so this tree is made by hand) and the
    // writer cannot write: the decode drops them and counts each, and a plain comment is kept and not counted.
    let inner = "<shape><zz><!-- fine --><!-- placeholder --><!-- placeholder --><!-- placeholder --></zz></shape>";
    let mut tree = parse(&page_text(inner));
    fn first_zz(tree: &mut El) -> &mut El {
        let Some(Node::El(shape)) = tree.children.iter_mut().find(|n| matches!(n, Node::El(_))) else { panic!("a shape") };
        let Some(Node::El(zz)) = shape.children.iter_mut().find(|n| matches!(n, Node::El(_))) else { panic!("an unknown element") };
        zz
    }
    {
        let zz = first_zz(&mut tree);
        zz.children[1] = Node::Comment(" a--b ".to_owned());
        zz.children[2] = Node::Comment(" ends in a dash -".to_owned());
        zz.children[3] = Node::Comment(String::new());
    }
    let mut notes = Vec::new();
    let page = decode_page(&tree, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).unwrap();
    // Two are dropped (the one that holds `--`, the one that ends in a dash), the empty comment and the plain one are kept.
    assert_eq!(notes, vec![DecodeNote::Unknown { count: 2 }]);
    let shape = page.shapes.get(0).unwrap();
    let kept: Vec<&diacraft_doc::Placed> = shape.ext.get().unwrap().native.kept.iter().collect();
    let diacraft_doc::RawNode::Element(zz) = &kept[0].node else { panic!("an element") };
    assert_eq!(zz.children, vec![diacraft_doc::RawNode::Comment(Arc::from(" fine ")), diacraft_doc::RawNode::Comment(Arc::from(""))]);
}

#[test]
fn the_characters_of_an_unknown_element_that_a_dropped_node_leaves_side_by_side_are_one_run_and_come_back() {
    // The instruction `<??>` is dropped and the two texts it stood between are one text, as a read of what is written gives them back; text and CDATA are one run,
    // an empty CDATA is no node, and a comment that is kept ends the run.
    let inner = "<shape><zz>pa<??>dded<![CDATA[x]]><![CDATA[]]><!-- c -->y<![CDATA[z]]></zz></shape>";
    let (page, notes) = page_of(inner).unwrap();
    assert_eq!(notes, vec![DecodeNote::Unknown { count: 1 }]);
    let shape = page.shapes.get(0).unwrap();
    let kept: Vec<&diacraft_doc::Placed> = shape.ext.get().unwrap().native.kept.iter().collect();
    assert_eq!(kept.len(), 1);
    let diacraft_doc::RawNode::Element(zz) = &kept[0].node else { panic!("an element") };
    assert_eq!(
        zz.children,
        vec![
            diacraft_doc::RawNode::Text(Arc::from("paddedx")),
            diacraft_doc::RawNode::Comment(Arc::from(" c ")),
            diacraft_doc::RawNode::Text(Arc::from("yz")),
        ]
    );
    // And it is the document that the package gives back, and every later one.
    assert_eq!(save_then_open(&document_of(page), &open_limits()), Ok(()));
}
