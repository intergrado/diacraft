//! One or more tests for each finding of the full review (`task-NATIVE.CODEC-1-review.md`), each written to fail on the codec as it was
//! merged (7847e5a) and to pass on the fix. They are named by what the codec must do, with the finding in the comment.
use std::sync::Arc;

use diacraft_container::limits::MAX_XML_ATTR_BYTES;
use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_doc::{Page, RawAttrs, Shape, ShapeList};
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, URN, decode_page, decode_shape, encode_page, encode_shape};

use super::model_util::*;

const HUGE: u64 = 1 << 40;
const P: &str = "diacraft";

/// `QD` stands for the codec's prefix in the texts below, so that a text can also be written with another prefix on purpose.
fn dx(s: &str) -> String {
    s.replace("xmlns:QD=", &format!("xmlns:{P}=")).replace("QD:", &format!("{P}:"))
}

fn big() -> Budget {
    Budget::new(HUGE, HUGE, HUGE)
}

fn page_xml(inner: &str) -> String {
    dx(&format!("<QD:page xmlns:QD=\"{URN}\" id=\"1\">{inner}</QD:page>"))
}

fn parse(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

fn page_of(xml: &str) -> Result<Page, CodecError> {
    decode_page(&parse(xml), &MemBlobs::new(), &mut big(), &mut Vec::new())
}

/// The part as a package would hold it: what the codec writes of `page`, as bytes.
fn written(page: &Page) -> Result<Vec<u8>, CodecError> {
    let el = encode_page(page, &mut MemBlobs::new())?;
    Ok(to_bytes(&el, &[], &[], true).expect("the tree the codec made can be written"))
}

// ----------------------------------------------------------------------------------------------------------------------: our own prefix

#[test]
fn a_part_root_with_an_unknown_attribute_in_our_own_prefix_decodes_and_writes_again() {
    // The root declares the codec's namespace itself, and a newer build's attribute in it: the decode must not keep the declaration, which the encoder
    // puts on the root again (a duplicate attribute, which no writer can write).
    let page = page_of(&dx(&format!("<QD:page xmlns:QD=\"{URN}\" id=\"1\" QD:future=\"1\"/>"))).unwrap();
    let native = &page.ext.get().expect("a slot for what a newer build wrote").native;
    assert_eq!(native.raw.iter().collect::<Vec<_>>(), [("diacraft:future", "1")], "the attribute, and no declaration of the codec's own prefix");
    let bytes = written(&page).expect("a page decoded from a part can be written again");
    let again = decode_page(&parse_with(&bytes, &native_limits()).unwrap(), &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap();
    assert!(again == page, "and reads back as it was");
}

#[test]
fn a_shape_with_an_unknown_attribute_in_our_own_prefix_writes_as_a_fragment_and_in_its_page() {
    let page = page_of(&page_xml(&dx("<QD:shape id=\"2\" QD:future=\"1\"/>"))).unwrap();
    let shape = page.shapes.get(0).unwrap();
    assert!(written(&page).is_ok(), "the page writes");
    let fragment = encode_shape(shape, &mut MemBlobs::new()).expect("a shape that carries the attribute encodes");
    let bytes = to_bytes(&fragment, &[], &[], true).expect("a fragment that declares the namespace once");
    let back = decode_shape(&parse_with(&bytes, &native_limits()).unwrap(), &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap();
    assert_eq!(&back, shape.as_ref());
}

#[test]
fn the_codecs_prefix_bound_to_another_namespace_on_an_attribute_is_refused_in_words() {
    // The codec writes its own prefix for its namespace on every part: an attribute whose `diacraft:` is another namespace's cannot come back as it was.
    let xml = format!("<d:page xmlns:d=\"{URN}\" xmlns:diacraft=\"urn:other\" id=\"1\" diacraft:future=\"1\"/>");
    let out = page_of(&xml);
    assert!(
        matches!(&out, Err(CodecError::Malformed { why: "the codec's own prefix bound to another namespace on an attribute", .. })),
        "{:?}",
        out.err()
    );
    // And a model that holds the declaration is refused by the encoder in the same words, and not written with the prefix bound twice.
    let mut page = Page::new(1, "P");
    page.ext.make_mut().native.raw = RawAttrs::from_pairs([("xmlns:diacraft", "urn:other"), ("diacraft:future", "1")]);
    let out = encode_page(&page, &mut MemBlobs::new());
    assert!(
        matches!(&out, Err(CodecError::Malformed { why: "the codec's own prefix bound to another namespace on an attribute", .. })),
        "{:?}",
        out.err()
    );
    // The same namespace, declared in the model's attributes, is the codec's declaration and is not written twice.
    let mut page = Page::new(1, "P");
    page.ext.make_mut().native.raw = RawAttrs::from_pairs([("xmlns:diacraft", URN), ("diacraft:future", "1")]);
    assert!(written(&page).is_ok());
}

// ----------------------------------------------------------------------------------------------------------------------: what grows when written

/// What an attribute value is as written by the container's writer: `&` five bytes, `<` four, `"` six, line feed and carriage return five, tab four.
fn as_written(value: &str) -> usize {
    value
        .chars()
        .map(|c| match c {
            '&' => 5,
            '<' => 4,
            '"' => 6,
            '\n' | '\r' => 5,
            '\t' => 4,
            other => other.len_utf8(),
        })
        .sum()
}

#[test]
fn an_unknown_attribute_is_kept_up_to_the_bytes_the_reader_allows_as_written_and_no_more() {
    // Six bytes as written for each quote: a value of 174,762 is 1,048,572 bytes, under the reader's 1 MiB, and one more is over it. The file may hold it
    // in single quotes at one byte for a quote, and still be under the limit: it is what the codec would write that is measured.
    let most = MAX_XML_ATTR_BYTES / 6;
    assert!(as_written(&"\"".repeat(most)) <= MAX_XML_ATTR_BYTES && as_written(&"\"".repeat(most + 1)) > MAX_XML_ATTR_BYTES);
    for (n, fits) in [(most, true), (most + 1, false)] {
        let xml = page_xml(&dx(&format!("<QD:shape id=\"1\" future='{}'/>", "\"".repeat(n))));
        let out = page_of(&xml);
        if fits {
            let page = out.expect("a value that fits as written");
            let bytes = written(&page).expect("and writes");
            assert!(parse_with(&bytes, &native_limits()).is_ok(), "the part written reopens");
        } else {
            assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })), "{:?}", out.err());
        }
    }
}

#[test]
fn the_encoder_refuses_what_it_could_not_read_again_in_words_for_each_kind_of_unknown_attribute() {
    // A model that was not decoded from a part (another reader made it) can hold anything: the writer measures each unknown attribute against the reader's
    // limit before it writes it, whatever the character that makes it grow, and on every element that takes one.
    let cases: [(char, usize); 6] = [('"', 6), ('&', 5), ('<', 4), ('\n', 5), ('\r', 5), ('\t', 4)];
    for (c, width) in cases {
        let most = MAX_XML_ATTR_BYTES / width;
        for (n, fits) in [(most, true), (most + 1, false)] {
            let value: String = std::iter::repeat_n(c, n).collect();
            // On a page, which has a slot: the attribute is written after the typed ones.
            let mut page = Page::new(1, "P");
            page.ext.make_mut().native.raw = RawAttrs::from_pairs([("future", value.as_str())]);
            // On a shape.
            let mut shape = Shape::new(1);
            shape.ext.make_mut().native.raw = RawAttrs::from_pairs([("future", value.as_str())]);
            let mut with_shape = Page::new(1, "P");
            with_shape.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
            for page in [page, with_shape] {
                let out = written(&page);
                if fits {
                    let bytes = out.unwrap_or_else(|e| panic!("{c:?} x {n}: {e}"));
                    assert!(parse_with(&bytes, &native_limits()).is_ok(), "{c:?} x {n}: the part written reopens");
                } else {
                    assert!(
                        matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })),
                        "{c:?} x {}: {:?}",
                        n,
                        out.err()
                    );
                }
            }
        }
    }
}

#[test]
fn an_unknown_attribute_of_a_kept_element_that_grows_when_written_is_refused_where_it_is_read_and_where_it_is_written() {
    // The same on an unknown element and its descendants, which are written as the XML they were.
    let most = MAX_XML_ATTR_BYTES / 6;
    for (n, fits) in [(most, true), (most + 1, false)] {
        let xml = page_xml(&dx(&format!("<QD:shape id=\"1\"><x:deep xmlns:x=\"urn:x\"><x:inner a='{}'/></x:deep></QD:shape>", "\"".repeat(n))));
        let out = page_of(&xml);
        if fits {
            let page = out.expect("fits");
            let bytes = written(&page).expect("writes");
            assert!(parse_with(&bytes, &native_limits()).is_ok());
        } else {
            assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })), "{:?}", out.err());
        }
    }
}

#[test]
fn a_decode_that_succeeds_writes_and_the_part_written_decodes_again() {
    // The property the fuzz target holds, on a hand-made case of each thing that was found to break it: our own prefix on a root and on a shape, a native
    // attribute of the largest size, and several of them.
    let big_attr = "\"".repeat(MAX_XML_ATTR_BYTES / 6);
    let xml = dx(&format!(
        "<QD:page xmlns:QD=\"{URN}\" id=\"1\" QD:future=\"1\" other='{big_attr}'><QD:shape id=\"2\" QD:future=\"1\"/><QD:shape id=\"3\" a='{big_attr}'/></QD:page>"
    ));
    let page = page_of(&xml).unwrap();
    let bytes = written(&page).expect("writes");
    let again = decode_page(&parse_with(&bytes, &native_limits()).unwrap(), &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap();
    assert!(again == page);
}

// ---------------------------------------------------------------------------------------------------------------------- (Call 24): ordinal keys

/// The page the codec writes of the decode of `xml`, as text, with the notes of the decode.
fn through(xml: &str) -> (String, Vec<diacraft_sheetxml::model::DecodeNote>, Page) {
    let mut notes = Vec::new();
    let page = decode_page(&parse(xml), &MemBlobs::new(), &mut big(), &mut notes).unwrap();
    let text = String::from_utf8(written(&page).unwrap()).unwrap();
    (text, notes, page)
}

#[test]
fn two_held_values_of_one_group_each_keep_what_a_newer_build_wrote_on_them() {
    let xml = page_xml(&dx(
        "<QD:shape id=\"1\"><QD:ext><QD:odf><QD:held group=\"fill\" futa=\"A\"/><QD:held group=\"fill\" futb=\"B\"/><QD:held group=\"line\" futc=\"C\"/></QD:odf></QD:ext></QD:shape>",
    ));
    let (text, notes, page) = through(&xml);
    assert!(notes.is_empty(), "nothing was lost, so nothing is counted: {notes:?}");
    for kept in ["futa=\"A\"", "futb=\"B\"", "futc=\"C\""] {
        assert!(text.contains(kept), "{kept} was lost: {text}");
    }
    // The second write is the first's bytes and reads back to the same model.
    let again = page_of(&text).unwrap();
    assert!(again == page);
    assert_eq!(written(&again).unwrap(), text.into_bytes());
}

#[test]
fn two_held_style_values_of_one_group_in_a_style_each_keep_what_a_newer_build_wrote_on_them() {
    let group = diacraft_doc::TextGroup::ALL[0].id();
    let xml = page_xml(&dx(&format!(
        "<QD:shape id=\"1\"><QD:text><QD:ext><QD:odf><QD:paragraph><QD:text-style><QD:held-style group=\"{group}\" futa=\"A\"/><QD:held-style group=\"{group}\" futb=\"B\"/></QD:text-style></QD:paragraph></QD:odf></QD:ext></QD:text></QD:shape>"
    )));
    let (text, notes, page) = through(&xml);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(text.contains("futa=\"A\"") && text.contains("futb=\"B\""), "{text}");
    assert!(page_of(&text).unwrap() == page);
}

#[test]
fn the_generator_makes_two_values_of_one_group_that_each_keep_unknown_content() {
    use diacraft_doc::RawNode;
    use diacraft_testkit::model::{ModelSize, any_document};
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    fn entries(shapes: &ShapeList, held: &mut u32, style: &mut u32) {
        for s in shapes.iter() {
            if let Some(data) = s.ext.get() {
                let names: Vec<&str> =
                    data.native.kept.iter().filter_map(|p| if let RawNode::Element(e) = &p.node { Some(&*e.name) } else { None }).collect();
                // `held[group.0]` and `held[group.1]` of one group.
                *held += u32::from(names.iter().any(|n| n.starts_with("held[") && n.ends_with(".1]")));
            }
            if let Some(data) = s.text.as_ref().and_then(|t| t.ext.get()) {
                let names: Vec<&str> =
                    data.native.kept.iter().filter_map(|p| if let RawNode::Element(e) = &p.node { Some(&*e.name) } else { None }).collect();
                *style += u32::from(names.iter().any(|n| n.starts_with("held-style[") && n.ends_with(".1]")));
            }
            entries(&s.children, held, style);
        }
    }
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[9u8; 32]));
    let strategy = any_document(ModelSize::Small);
    let (mut held, mut style) = (0u32, 0u32);
    for _ in 0..500 {
        let d = strategy.new_tree(&mut runner).unwrap().current();
        for page in d.pages.iter() {
            entries(&page.shapes, &mut held, &mut style);
        }
        for master in d.masters.iter() {
            entries(&master.shapes, &mut held, &mut style);
        }
    }
    assert!(held > 0 && style > 0, "documents with a second value of a group that keeps unknown content: held {held}, held-style {style}");
}

// ---------------------------------------------------------------------------------------------------------------------- to 10

fn document_of(inner: &str) -> El {
    parse(&dx(&format!("<QD:document xmlns:QD=\"{URN}\">{inner}</QD:document>")))
}

fn notes_of(document: &El, pages: &[&El]) -> Vec<diacraft_sheetxml::model::DecodeNote> {
    let mut notes = Vec::new();
    diacraft_sheetxml::model::decode(document, pages, &[], &MemBlobs::new(), &mut big(), &mut notes).unwrap();
    notes
}

#[test]
fn unknown_attributes_and_children_on_a_blob_a_page_ref_and_a_master_ref_are_counted() {
    // The codec's own list records take the names the vocabulary gives them, and what else is on one is a newer build's, which a record with no slot
    // counts and cannot keep (the module doc says so).
    use diacraft_sheetxml::model::DecodeNote;
    let page = parse(&dx(&format!("<QD:page xmlns:QD=\"{URN}\"/>")));
    let known = "<QD:page-ref id=\"0\" part=\"pages/page0.xml\"/>";
    let none = notes_of(&document_of(known), &[&page]);
    assert!(none.is_empty(), "{none:?}");
    let attr = "<QD:page-ref id=\"0\" part=\"pages/page0.xml\" futp=\"1\"/>";
    assert_eq!(notes_of(&document_of(attr), &[&page]), [DecodeNote::Unknown { count: 1 }], "an attribute of a page-ref");
    let child = "<QD:page-ref id=\"0\" part=\"pages/page0.xml\"><x:y xmlns:x=\"urn:x\"/></QD:page-ref>";
    assert_eq!(notes_of(&document_of(child), &[&page]), [DecodeNote::Unknown { count: 1 }], "a child of a page-ref");
    let blob = "<QD:blob id=\"0\" path=\"p\" media-type=\"a/b\" size=\"1\" stamp=\"0123456789abcdef\" futb=\"1\"><QD:other/></QD:blob>";
    assert_eq!(notes_of(&document_of(blob), &[]), [DecodeNote::Unknown { count: 2 }], "an attribute and a child of a blob record");
    // A master-ref, which is given its part.
    let master = parse(&dx(&format!("<QD:master xmlns:QD=\"{URN}\"/>")));
    let mut notes = Vec::new();
    let document = document_of("<QD:master-ref futm=\"1\"/>");
    diacraft_sheetxml::model::decode(&document, &[], &[&master], &MemBlobs::new(), &mut big(), &mut notes).unwrap();
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }], "an attribute of a master-ref");
}

#[test]
fn a_second_of_a_wrapper_or_an_extra_is_counted_and_the_first_is_the_one_read() {
    // `extra` of a cell and the wrappers `settings`, `meta`, `drawing-page-style`, `master-page-attrs`, `layout-props`, `units` and `unmapped` hold one
    // of each: a second is not read (it would replace the first and lose it), and is counted.
    use diacraft_sheetxml::model::DecodeNote;
    let attr = |name: &str, value: &str| format!("<QD:attr name=\"{name}\" value=\"{value}\"/>");
    let el = |name: &str| format!("<QD:el name=\"{name}\"/>");
    // Document level: the first of each is read, and each second is one note.
    for (wrapper, first, second) in [("settings", el("one"), el("two")), ("meta", String::new(), String::new())] {
        let xml = format!("<QD:ext><QD:odf><QD:{wrapper}>{first}</QD:{wrapper}><QD:{wrapper}>{second}</QD:{wrapper}></QD:odf></QD:ext>");
        assert_eq!(notes_of(&document_of(&dx(&xml)), &[]), [DecodeNote::Unknown { count: 1 }], "a second {wrapper}");
    }
    // Page level.
    let page_notes = |inner: &str| {
        let page = parse(&dx(&format!("<QD:page xmlns:QD=\"{URN}\">{inner}</QD:page>")));
        let mut notes = Vec::new();
        let p = decode_page(&page, &MemBlobs::new(), &mut big(), &mut notes).unwrap();
        (p, notes)
    };
    for wrapper in ["master-page-attrs", "layout-props", "units"] {
        let xml = format!(
            "<QD:ext><QD:odf><QD:{wrapper}>{}</QD:{wrapper}><QD:{wrapper}>{}</QD:{wrapper}></QD:odf></QD:ext>",
            attr("a", "first"),
            attr("a", "second")
        );
        let (page, notes) = page_notes(&xml);
        assert_eq!(notes, [DecodeNote::Unknown { count: 1 }], "a second {wrapper}");
        let odf = &page.ext.get().unwrap().odf;
        let read = match wrapper {
            "master-page-attrs" => &odf.master_page_attrs,
            "layout-props" => &odf.layout_props,
            _ => &odf.units,
        };
        assert_eq!(read.iter().collect::<Vec<_>>(), [("a", "first")], "the first {wrapper} is the one read");
    }
    let xml = format!(
        "<QD:ext><QD:odf><QD:drawing-page-style>{}</QD:drawing-page-style><QD:drawing-page-style>{}</QD:drawing-page-style></QD:odf></QD:ext>",
        el("one"),
        el("two")
    );
    let (page, notes) = page_notes(&xml);
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    assert_eq!(&*page.ext.get().unwrap().odf.drawing_page_style.as_ref().unwrap().name, "one");
    // A cell's extra (the reviewer's case).
    let (page, notes) = page_notes("<QD:sheet><QD:Cell N=\"A\"><QD:extra text=\"first\"/><QD:extra text=\"second\"/></QD:Cell></QD:sheet>");
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }], "a second extra");
    let cell = page.sheet.cells.values().next().unwrap();
    assert_eq!(cell.extra.as_ref().and_then(|e| e.text.as_deref()), Some("first"));
}

#[test]
fn a_native_raw_that_is_a_text_a_comment_or_an_instruction_comes_back() {
    // A newer build's text node, comment or instruction among a text's parts is kept in data form and written back as itself, never merged into the
    // characters beside it nor dropped.
    use diacraft_doc::{RawNode, Text, TextPart};
    let nodes = [
        RawNode::Text(Arc::from("x")),
        RawNode::Text(Arc::from("  \r\n ")),
        RawNode::Comment(Arc::from("c")),
        RawNode::Pi { target: Arc::from("t"), data: Arc::from("d") },
        RawNode::Text(Arc::from("\u{1}")),
    ];
    for node in nodes {
        let mut shape = Shape::new(1);
        let parts = vec![
            TextPart::Chars(Arc::from("a")),
            TextPart::NativeRaw(Box::new(node.clone())),
            TextPart::Chars(Arc::from("b")),
            TextPart::NativeRaw(Box::new(node.clone())),
            TextPart::NativeRaw(Box::new(node.clone())),
        ];
        shape.text = Some(Arc::new(Text { parts, ..Text::default() }));
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        let bytes = written(&page).unwrap();
        let back = decode_page(&parse_with(&bytes, &native_limits()).unwrap(), &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap();
        assert!(back == page, "{node:?}: {:?}", back.shapes.get(0).unwrap().text.as_ref().unwrap().parts);
    }
}

#[test]
fn the_cap_on_the_definitions_of_one_family_is_met_where_they_are_read() {
    // The per-family cap is counted as the definitions are read, so that the 65,537th of a family is refused with the family's words and what follows
    // it is not read; the check at the end of a decode (which gives the same words) is not what stops a part of millions.
    use diacraft_doc::MAX_DEFINITIONS;
    let definitions = |n: usize| -> String {
        (0..n).map(|i| format!("<QD:definition family=\"marker\" name=\"n{i}\"><QD:el name=\"a\"/></QD:definition>")).collect()
    };
    let read = |n: usize| {
        let document = document_of(&dx(&format!("<QD:ext><QD:odf>{}</QD:odf></QD:ext>", definitions(n))));
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        let out = diacraft_sheetxml::model::decode(&document, &[], &[], &MemBlobs::new(), &mut budget, &mut Vec::new());
        (out.map(|_| ()), HUGE - budget.items)
    };
    let (at_the_cap, items_at_the_cap) = read(MAX_DEFINITIONS);
    assert!(at_the_cap.is_ok(), "{:?}", at_the_cap.err());
    let (past, items_past) = read(MAX_DEFINITIONS + 20_000);
    assert!(matches!(&past, Err(CodecError::Limit { what: "marker definitions", max }) if *max == MAX_DEFINITIONS as u64), "{:?}", past.err());
    // It stopped at the definition that passes the cap: about five items for each one, and the twenty thousand after it unread.
    assert!(items_past < items_at_the_cap + 50, "{items_past} items for the refusal, {items_at_the_cap} for the cap's worth");
}

// ----------------------------------------------------------------------------------------------------------------------: the prefix lookup

#[derive(Clone, Copy, Default)]
struct OursOnly;

impl diacraft_sheetxml::Vocabulary for OursOnly {
    fn reads(self, uri: Option<&str>) -> bool {
        uri == Some(URN)
    }
}

fn named(name: &str) -> El {
    El { name: name.to_owned(), attrs: Vec::new(), children: Vec::new() }
}

/// The scope at the bottom of a chain of `depth` elements that each declare a prefix of their own (`z0`, `z1`, and so on).
fn chain(depth: usize) -> diacraft_sheetxml::Scope<OursOnly> {
    let mut scope = diacraft_sheetxml::Scope::<OursOnly>::empty();
    for i in 0..depth {
        let level = El { name: "diacraft:shape".to_owned(), attrs: vec![(format!("xmlns:z{i}"), format!("urn:z{i}"))], children: Vec::new() };
        scope = scope.enter(&level);
    }
    scope
}

#[test]
fn a_prefix_that_no_open_element_declares_costs_the_lookup_no_walk_however_deep_the_scope_is() {
    // Each distinct unbound prefix walked the whole chain of declaring frames (and was remembered only in the frame that asked), so a part of 800,000
    // such names under a chain of 200 took 25 times what the same part with one prefix took. The work is counted in frames visited: it does not depend on the depth.
    let queries = 5_000u64;
    for depth in [1usize, 50, 400] {
        let scope = chain(depth);
        let before = scope.lookup_steps();
        for i in 0..queries {
            assert_eq!(scope.uri_of_element(&named(&format!("p{i}:x"))), None);
        }
        let steps = scope.lookup_steps() - before;
        assert!(steps <= queries * 2, "depth {depth}: {steps} steps for {queries} lookups");
    }
}

#[test]
fn a_declared_prefix_is_found_in_a_few_steps_whatever_the_distance_to_the_element_that_declares_it() {
    // The 400 prefixes are declared by 400 different elements, the nearest 1 frame up and the farthest 400. Each is found with a number of steps that grows with
    // the logarithm of the depth at the most, so 400 lookups cost far less than the 80,000 a walk of the chain costs.
    let depth = 400usize;
    let scope = chain(depth);
    let before = scope.lookup_steps();
    for i in 0..depth {
        assert_eq!(scope.uri_of_element(&named(&format!("z{i}:x"))).as_deref(), Some(format!("urn:z{i}").as_str()));
    }
    let steps = scope.lookup_steps() - before;
    assert!(steps >= depth as u64, "each lookup looks at one frame at least: {steps} steps");
    assert!(steps > 3 * depth as u64, "and a frame far above is reached by jumps, which are counted: {steps} steps");
    assert!(steps <= (depth as u64) * 12, "{steps} steps for {depth} lookups at depth {depth}");
    // Asked again, from the same place, the first eight are answered without a step: a frame keeps the answers of the first eight prefixes it was asked (what a
    // frame keeps is bounded, so that a part of millions of them is), and the others are found again in as few steps as the first time.
    let before = scope.lookup_steps();
    for i in 0..8 {
        assert!(scope.uri_of_element(&named(&format!("z{i}:x"))).is_some());
    }
    assert_eq!(scope.lookup_steps(), before, "the answers are kept in the frame that was asked");
    let ninth = scope.lookup_steps();
    assert!(scope.uri_of_element(&named("z8:x")).is_some());
    assert!(scope.lookup_steps() > ninth, "the ninth prefix asked is not kept: eight are");
    for i in 8..depth {
        assert!(scope.uri_of_element(&named(&format!("z{i}:x"))).is_some());
    }
    let again = scope.lookup_steps() - before;
    assert!(again >= (depth as u64 - 8), "the rest are looked for again: {again} steps");
    assert!(again <= (depth as u64) * 12, "and found in as few: {again} steps");
}

#[test]
fn a_prefix_declared_by_elements_that_are_closed_is_unbound_and_costs_no_walk() {
    // Declared by 5,000 sibling elements, each closed before the next, and then asked about from the bottom of a deep chain under the same root: nothing open
    // declares them.
    let root = El { name: "diacraft:page".to_owned(), attrs: vec![("xmlns:r".to_owned(), "urn:r".to_owned())], children: Vec::new() };
    let top = diacraft_sheetxml::Scope::<OursOnly>::empty().enter(&root);
    for i in 0..5_000 {
        let sibling = El { name: "diacraft:shape".to_owned(), attrs: vec![(format!("xmlns:s{i}"), format!("urn:s{i}"))], children: Vec::new() };
        let inside = top.enter(&sibling);
        assert_eq!(inside.uri_of_element(&named(&format!("s{i}:x"))).as_deref(), Some(format!("urn:s{i}").as_str()));
    }
    let mut deep = top.clone();
    for i in 0..300 {
        let level = El { name: "diacraft:shape".to_owned(), attrs: vec![(format!("xmlns:z{i}"), format!("urn:z{i}"))], children: Vec::new() };
        deep = deep.enter(&level);
    }
    let before = deep.lookup_steps();
    for i in 0..5_000 {
        assert_eq!(deep.uri_of_element(&named(&format!("s{i}:x"))), None);
    }
    assert!(deep.lookup_steps() - before <= 10_000, "{} steps", deep.lookup_steps() - before);
    // What the root declares is still found from there.
    assert_eq!(deep.uri_of_element(&named("r:x")).as_deref(), Some("urn:r"));
}

/// A page of `depth` nested shapes that each declare a prefix of their own (`z0`, `z1`, ...), holding `n` unknown elements of one prefix, or of a prefix
/// each: the same bytes in item and size to within the width of the prefix's digits, and so the same cost to anything that is linear in the file.
fn prefix_chain(depth: usize, n: usize, distinct: bool) -> String {
    let (mut open, mut close) = (String::new(), String::new());
    for i in 0..depth {
        open.push_str(&format!("<QD:shape xmlns:z{i}=\"urn:z{i}\">"));
        close.push_str("</QD:shape>");
    }
    let kids: String = (0..n).map(|i| if distinct { format!("<p{i:07}:x/>") } else { "<p0000000:x/>".to_owned() }).collect();
    page_xml(&format!("{open}{kids}{close}"))
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_distinct_prefixes_under_a_chain_of_declaring_elements_cost_what_one_prefix_does() {
    // The reviewer's inputs: 200,000 names under a chain of one and of 200, and 800,000 under 200. The ratio was 17 to 25 before the fix; it is the noise of a
    // second run now.
    use std::time::Instant;
    for (depth, n) in [(1usize, 200_000usize), (200, 200_000), (200, 800_000)] {
        let same = parse(&prefix_chain(depth, n, false));
        let distinct = parse(&prefix_chain(depth, n, true));
        let best = |tree: &El| {
            (0..3)
                .map(|_| {
                    let t = Instant::now();
                    decode_page(tree, &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap();
                    t.elapsed().as_secs_f64()
                })
                .fold(f64::MAX, f64::min)
        };
        let (one, many) = (best(&same), best(&distinct));
        eprintln!("depth {depth}, {n} names: one prefix {one:.3} s, distinct prefixes {many:.3} s, ratio {:.2}", many / one.max(1e-9));
        assert!(many < one * 3.0, "depth {depth}, {n} names: {many:.3} s against {one:.3} s");
    }
}

#[test]
fn the_generator_makes_our_own_prefix_on_an_unknown_attribute_and_a_native_part_that_is_no_element() {
    use diacraft_doc::{RawNode, TextPart};
    use diacraft_testkit::model::{ModelSize, any_document};
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    fn walk(shapes: &ShapeList, non_element: &mut u32) {
        for s in shapes.iter() {
            if let Some(text) = &s.text {
                *non_element += u32::from(text.parts.iter().any(|p| matches!(p, TextPart::NativeRaw(n) if !matches!(&**n, RawNode::Element(_)))));
            }
            walk(&s.children, non_element);
        }
    }
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[11u8; 32]));
    let strategy = any_document(ModelSize::Small);
    let (mut own, mut non_element) = (0u32, 0u32);
    for _ in 0..400 {
        let d = strategy.new_tree(&mut runner).unwrap().current();
        let packed = pack(&d);
        own += u32::from(packed.all_bytes().iter().any(|b| String::from_utf8_lossy(b).contains(" diacraft:future=")));
        for page in d.pages.iter() {
            walk(&page.shapes, &mut non_element);
        }
        for master in d.masters.iter() {
            walk(&master.shapes, &mut non_element);
        }
    }
    assert!(
        own > 0 && non_element > 0,
        "documents with our own prefix on an unknown attribute: {own}, with a native part that is no element: {non_element}"
    );
}

#[test]
fn a_prefix_the_inner_element_declares_again_is_the_outer_ones_when_the_inner_one_is_closed() {
    // The frames that declare a prefix are listed by it while they are open, nearest last: closing the inner takes the inner out of the list, and not the outer.
    let level =
        |prefix_uri: &str| El { name: "diacraft:shape".to_owned(), attrs: vec![("xmlns:a".to_owned(), prefix_uri.to_owned())], children: Vec::new() };
    let outer = diacraft_sheetxml::Scope::<OursOnly>::empty().enter(&level("urn:outer"));
    {
        let inner = outer.enter(&level("urn:inner"));
        assert_eq!(inner.uri_of_element(&named("a:x")).as_deref(), Some("urn:inner"), "the nearest declaration");
        let deeper = inner.enter(&named("diacraft:shape"));
        assert_eq!(deeper.uri_of_element(&named("a:x")).as_deref(), Some("urn:inner"), "and from an element below that declares nothing");
        // Asked from the outer one while the inner is open, it is still the outer's.
        assert_eq!(outer.uri_of_element(&named("a:x")).as_deref(), Some("urn:outer"));
    }
    // An element that declares something else is a frame of its own, with no answers kept yet (one that declares nothing is the frame above it, and its answers).
    let fresh = El { name: "diacraft:shape".to_owned(), attrs: vec![("xmlns:other".to_owned(), "urn:other".to_owned())], children: Vec::new() };
    let again = outer.enter(&fresh);
    assert_eq!(again.uri_of_element(&named("a:x")).as_deref(), Some("urn:outer"), "the inner is closed");
    // A sibling that declares it differently, closed, and a third: each is its own.
    for uri in ["urn:one", "urn:two", "urn:three"] {
        let sibling = outer.enter(&level(uri));
        assert_eq!(sibling.uri_of_element(&named("a:x")).as_deref(), Some(uri));
    }
    let last = outer.enter(&fresh);
    assert_eq!(last.uri_of_element(&named("a:x")).as_deref(), Some("urn:outer"));
}

#[test]
fn closed_elements_that_declared_one_prefix_leave_it_unbound_and_cost_no_walk() {
    // Five thousand siblings that each declare the same prefix, each closed before the next: asked about afterwards, from the bottom of a deep chain under the
    // same root, no open element declares it, and the lookup does not go through five thousand closed ones.
    let root = El { name: "diacraft:page".to_owned(), attrs: Vec::new(), children: Vec::new() };
    let top = diacraft_sheetxml::Scope::<OursOnly>::empty().enter(&root);
    for i in 0..5_000 {
        let sibling = El { name: "diacraft:shape".to_owned(), attrs: vec![("xmlns:shared".to_owned(), format!("urn:s{i}"))], children: Vec::new() };
        let inside = top.enter(&sibling);
        assert_eq!(inside.uri_of_element(&named("shared:x")).as_deref(), Some(format!("urn:s{i}").as_str()));
    }
    let mut deep = top.clone();
    for i in 0..50 {
        deep = deep.enter(&El { name: "diacraft:shape".to_owned(), attrs: vec![(format!("xmlns:z{i}"), format!("urn:z{i}"))], children: Vec::new() });
    }
    let before = deep.lookup_steps();
    assert_eq!(deep.uri_of_element(&named("shared:x")), None);
    assert!(deep.lookup_steps() - before <= 2, "{} steps", deep.lookup_steps() - before);
}

#[test]
fn the_declaration_copied_for_an_unknown_attribute_is_charged_by_its_bytes() {
    // An unknown attribute with a prefix is kept with the declaration of its prefix, which the model then holds: the bytes of the URI are charged for the copy,
    // and again for the frame of the element that declares it (a frame holds the namespace of each prefix), however long the URI is.
    let charged = |uri_len: usize| {
        let uri = "u".repeat(uri_len);
        let xml = page_xml(&dx(&format!("<QD:shape id=\"1\" xmlns:q=\"{uri}\" q:future=\"1\"/>")));
        let mut budget = big();
        decode_page(&parse(&xml), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    // The copy is text, at nine eighths, and the frame's bytes are the frame's price.
    let text = diacraft_sheetxml::model::cost::text_bytes;
    assert_eq!(charged(10_010) - charged(10), text(10_010) - text(10) + 10_000);
}

#[test]
fn an_unknown_attribute_of_exactly_the_most_bytes_as_written_is_kept_and_one_byte_more_is_not() {
    // The cap is met to the byte, in the three places that hold it: an unknown attribute of a shape and of a kept element when they are read, and any unknown
    // attribute when it is written. 174,762 quotes are 1,048,572 bytes as written, and four more characters make exactly the reader's 1 MiB.
    let quotes = "\"".repeat((MAX_XML_ATTR_BYTES - 4) / 6);
    assert_eq!(as_written(&quotes) + 4, MAX_XML_ATTR_BYTES);
    for (suffix, fits) in [("abcd", true), ("abcde", false)] {
        let value = format!("{quotes}{suffix}");
        let on_a_shape = page_of(&page_xml(&dx(&format!("<QD:shape id=\"1\" future='{value}'/>"))));
        let on_a_kept_element = page_of(&page_xml(&dx(&format!("<QD:shape id=\"1\"><x:u xmlns:x=\"urn:x\" a='{value}'/></QD:shape>"))));
        let mut model = Page::new(1, "P");
        model.ext.make_mut().native.raw = RawAttrs::from_pairs([("future", value.as_str())]);
        for out in [on_a_shape.map(|_| ()), on_a_kept_element.map(|_| ()), written(&model).map(|_| ())] {
            if fits {
                assert!(out.is_ok(), "{:?}", out.err());
            } else {
                assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })), "{:?}", out);
            }
        }
    }
}

#[test]
fn an_unknown_element_is_charged_for_each_of_its_attributes() {
    // Beside the item of each, a kept attribute costs the pair of strings the model holds for it, besides the bytes of its name and value.
    use diacraft_sheetxml::model::cost::{ATTR_BYTES, UNKNOWN_ATTR_BYTES};
    let charged = |attrs: &str| {
        let xml = page_xml(&dx(&format!("<QD:shape id=\"1\"><x:u xmlns:x=\"urn:x\" {attrs}/></QD:shape>")));
        let mut budget = big();
        decode_page(&parse(&xml), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let with = charged("a=\"1\" b=\"22\" c=\"333\"");
    let without = charged("");
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`).
    let text = diacraft_sheetxml::model::cost::text_bytes;
    assert_eq!(with - without, 3 * (UNKNOWN_ATTR_BYTES + ATTR_BYTES) + text(1 + 1) + text(1 + 2) + text(1 + 3));
}

#[test]
fn a_kept_unknown_text_comment_or_instruction_and_a_bare_text_of_a_text_are_each_charged_what_the_model_holds_for_them() {
    // What a node costs once kept is the price of the kind and the bytes of its text: the difference between a page that has one and a page that has none.
    use diacraft_sheetxml::model::cost::{TEXT_PART_BYTES, UNKNOWN_COMMENT_BYTES, UNKNOWN_PI_BYTES, UNKNOWN_TEXT_BYTES};
    let charged = |inner: &str| {
        let xml = page_xml(&dx(&format!("<QD:shape id=\"1\">{inner}</QD:shape>")));
        let mut budget = big();
        decode_page(&parse(&xml), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let empty = charged("<x:u xmlns:x=\"urn:x\"/>");
    // And the room the element makes for its one child: four, at the first.
    let room = diacraft_sheetxml::model::cost::vec_bytes(1, diacraft_sheetxml::model::cost::RAW_NODE_BYTES);
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`).
    let text = diacraft_sheetxml::model::cost::text_bytes;
    assert_eq!(charged("<x:u xmlns:x=\"urn:x\">abc</x:u>") - empty, UNKNOWN_TEXT_BYTES + text(3) + room, "a text inside a kept element");
    assert_eq!(charged("<x:u xmlns:x=\"urn:x\"><!--abcd--></x:u>") - empty, UNKNOWN_COMMENT_BYTES + text(4) + room, "a comment inside it");
    assert_eq!(charged("<x:u xmlns:x=\"urn:x\"><?ab cde?></x:u>") - empty, UNKNOWN_PI_BYTES + text(5) + room, "an instruction inside it");
    // A bare text among the parts of a text.
    let none = charged("<QD:text/>");
    assert_eq!(charged("<QD:text>abcdef</QD:text>") - none, TEXT_PART_BYTES + text(6), "a bare text of a text");
}

#[test]
fn an_unknown_attribute_too_long_as_written_is_refused_by_the_encoder_in_every_unknown_element_it_writes() {
    // A model another reader made can hold an unknown element with the attribute at any depth: the element at the top, and one inside it, are each measured.
    use diacraft_doc::{Passthrough, Placed, RawElement, RawNode};
    let big = "\"".repeat(MAX_XML_ATTR_BYTES / 6 + 1);
    let inner = |pairs: Vec<(&str, &str)>| {
        RawNode::Element(RawElement { name: Arc::from("q:inner"), attrs: RawAttrs::from_pairs(pairs), children: Vec::new() })
    };
    let element = |extra: Vec<(&str, &str)>, children: Vec<RawNode>| {
        let mut pairs = vec![("xmlns:q", "urn:q")];
        pairs.extend(extra);
        RawNode::Element(RawElement { name: Arc::from("q:outer"), attrs: RawAttrs::from_pairs(pairs), children })
    };
    for node in [element(vec![("a", big.as_str())], Vec::new()), element(Vec::new(), vec![inner(vec![("a", big.as_str())])])] {
        let mut shape = Shape::new(1);
        shape.ext.make_mut().native.kept = Passthrough::from_nodes(vec![Placed { after: 0, node }]);
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        let out = written(&page);
        assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })), "{:?}", out.err());
    }
}

#[test]
fn the_frames_of_two_siblings_that_declare_one_prefix_are_each_taken_out_when_each_is_closed() {
    // Two sibling elements are open at once and declare the same prefix; the first is closed, and the second still has its own declaration. (Each frame has an
    // identity of its own: a list that took out the last entry for either would take out the second's.)
    let root = El { name: "diacraft:page".to_owned(), attrs: Vec::new(), children: Vec::new() };
    let top = diacraft_sheetxml::Scope::<OursOnly>::empty().enter(&root);
    let declares = |uri: &str| El { name: "diacraft:shape".to_owned(), attrs: vec![("xmlns:a".to_owned(), uri.to_owned())], children: Vec::new() };
    let first = top.enter(&declares("urn:first"));
    let second = top.enter(&declares("urn:second"));
    assert_eq!(first.uri_of_element(&named("a:x")).as_deref(), Some("urn:first"));
    assert_eq!(second.uri_of_element(&named("a:x")).as_deref(), Some("urn:second"));
    drop(first);
    let fresh = El { name: "diacraft:shape".to_owned(), attrs: vec![("xmlns:other".to_owned(), "urn:other".to_owned())], children: Vec::new() };
    let below = second.enter(&fresh);
    assert_eq!(below.uri_of_element(&named("a:x")).as_deref(), Some("urn:second"), "the second is still open");
    drop(below);
    drop(second);
    assert_eq!(top.enter(&named("diacraft:shape")).uri_of_element(&named("a:x")), None, "and when it is closed too, nothing declares the prefix");
}

#[test]
fn an_unknown_attribute_of_an_unknown_element_is_written_up_to_exactly_the_most_bytes_and_not_one_more() {
    use diacraft_doc::{Passthrough, Placed, RawElement, RawNode};
    let quotes = "\"".repeat((MAX_XML_ATTR_BYTES - 4) / 6);
    for (suffix, fits) in [("abcd", true), ("abcde", false)] {
        let value = format!("{quotes}{suffix}");
        let node = RawNode::Element(RawElement {
            name: Arc::from("q:e"),
            attrs: RawAttrs::from_pairs([("xmlns:q", "urn:q"), ("a", value.as_str())]),
            children: Vec::new(),
        });
        let mut shape = Shape::new(1);
        shape.ext.make_mut().native.kept = Passthrough::from_nodes(vec![Placed { after: 0, node }]);
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        let out = written(&page);
        if fits {
            let bytes = out.expect("exactly the cap is written");
            assert!(parse_with(&bytes, &native_limits()).is_ok(), "and reopens");
        } else {
            assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of an unknown attribute as written", .. })), "{:?}", out.err());
        }
    }
}

#[test]
fn a_lookup_visits_the_one_frame_that_declares_the_prefix_and_reaches_it_by_jumps_of_a_power_of_two() {
    // The work is counted: one visit to the frame that declares the prefix, and a jump for each bit of the number of frames between it and the asker (50 is
    // three bits), and a prefix the frame itself declares needs no jump.
    let declares = |prefix: &str, uri: &str| El {
        name: "diacraft:shape".to_owned(),
        attrs: vec![(format!("xmlns:{prefix}"), uri.to_owned())],
        children: Vec::new(),
    };
    let top = diacraft_sheetxml::Scope::<OursOnly>::empty().enter(&declares("a", "urn:a"));
    let before = top.lookup_steps();
    assert_eq!(top.uri_of_element(&named("a:x")).as_deref(), Some("urn:a"));
    assert_eq!(top.lookup_steps() - before, 1, "the frame itself: a visit and no jump");
    let mut deep = top.clone();
    for i in 0..50 {
        deep = deep.enter(&declares(&format!("z{i}"), "urn:z"));
    }
    let before = deep.lookup_steps();
    assert_eq!(deep.uri_of_element(&named("a:x")).as_deref(), Some("urn:a"));
    assert_eq!(deep.lookup_steps() - before, 1 + 3, "a visit and three jumps for fifty frames");
    // Asked again from the same frame, the answer is kept and nothing is visited.
    let before = deep.lookup_steps();
    assert_eq!(deep.uri_of_element(&named("a:x")).as_deref(), Some("urn:a"));
    assert_eq!(deep.lookup_steps(), before);
    // A frame that declares the prefix and is not above the asker is visited and passed over: at the asker's own depth, or deeper, with no jump (the depth tells),
    // and above it with one jump to the frame at that depth, which is not the declarer.
    let sibling = top.enter(&declares("b", "urn:b"));
    let beside = top.enter(&declares("c", "urn:c"));
    let before = beside.lookup_steps();
    assert_eq!(beside.uri_of_element(&named("b:x")), None, "a sibling's declaration is not in force");
    assert_eq!(beside.lookup_steps() - before, 1, "a visit and no jump at the same depth");
    let under_beside = beside.enter(&declares("f", "urn:f"));
    let before = under_beside.lookup_steps();
    assert_eq!(under_beside.uri_of_element(&named("b:x")), None);
    assert_eq!(under_beside.lookup_steps() - before, 2, "a visit and one jump to the frame at the sibling's depth, which is not it");
    let under_sibling = sibling.enter(&declares("g", "urn:g"));
    let before = beside.lookup_steps();
    assert_eq!(beside.uri_of_element(&named("g:x")), None);
    assert_eq!(beside.lookup_steps() - before, 1, "a declarer deeper than the asker is visited and no jump is made");
    drop(under_sibling);
}

// ----------------------------------------------------------------------------------------------------------------------: namespaces as written

/// A decode that succeeds is a model that writes, and the bytes written read and decode again; or it is refused in words where it was read. Never both a decode
/// and a part that cannot be opened. `what` names where each of the namespaces the part carries goes when the model is written.
fn decodes_and_writes_or_is_refused(label: &str, xml: &str) -> &'static str {
    let tree = match parse_with(xml.as_bytes(), &native_limits()) {
        Ok(t) => t,
        Err(_) => return "the reader refused the input",
    };
    let page = match decode_page(&tree, &MemBlobs::new(), &mut big(), &mut Vec::new()) {
        Ok(p) => p,
        Err(e) => {
            assert!(
                matches!(&e, CodecError::Limit { what: "bytes of an unknown attribute as written", .. }),
                "{label}: refused, but not for the bytes as written: {e}"
            );
            return "refused in words";
        }
    };
    let el = encode_page(&page, &mut MemBlobs::new()).unwrap_or_else(|e| panic!("{label}: a decoded page the encoder refuses: {e}"));
    let bytes = to_bytes(&el, &[], &[], true).unwrap_or_else(|e| panic!("{label}: a tree the codec made that the writer refuses: {e}"));
    let again = parse_with(&bytes, &native_limits()).unwrap_or_else(|e| panic!("{label}: wrote {} bytes the reader refuses: {e}", bytes.len()));
    decode_page(&again, &MemBlobs::new(), &mut big(), &mut Vec::new()).unwrap_or_else(|e| panic!("{label}: wrote a part the decoder refuses: {e}"));
    "decoded and written"
}

#[test]
fn a_namespace_copied_onto_kept_content_is_held_to_the_bytes_as_written_wherever_it_is_copied() {
    // A quote is one byte in a value in single quotes and six as the writer spells it, so a namespace of quotes that the reader took can be too long to write.
    let mut refused = 0;
    let mut written_ok = 0;
    for quotes in [1_000usize, 100_000, 174_000, 175_000, 180_000, 200_000, 1_000_000] {
        let uri = "\"".repeat(quotes);
        let cases = [
            // An unknown attribute with a prefix: the declaration of its prefix is copied from the root.
            format!("<page xmlns=\"{URN}\" xmlns:k='{uri}'><shape id=\"1\" k:future=\"1\"/></page>"),
            // An unknown element with a prefix, bound above it.
            format!("<page xmlns=\"{URN}\" xmlns:k='{uri}'><shape id=\"1\"><k:x/></shape></page>"),
            // An unknown element two levels down whose attribute has a prefix.
            format!("<page xmlns=\"{URN}\" xmlns:k='{uri}'><shape id=\"1\"><y><z k:a=\"1\"/></y></shape></page>"),
            // An unknown element in the default namespace, which is the one of quotes, declared on the part's root: our elements are named by prefix.
            dx(&format!("<QD:page xmlns:QD=\"{URN}\" xmlns='{uri}'><QD:shape id=\"1\"><z/></QD:shape></QD:page>")),
            // The same, deeper than the first element that is unknown.
            dx(&format!("<QD:page xmlns:QD=\"{URN}\" xmlns='{uri}'><QD:shape id=\"1\"><z><w/></z></QD:shape></QD:page>")),
            // A prefix on an unknown element inside a kept one, declared by the shape.
            format!("<page xmlns=\"{URN}\"><shape id=\"1\" xmlns:k='{uri}'><y><k:z/></y></shape></page>"),
        ];
        for (i, xml) in cases.iter().enumerate() {
            match decodes_and_writes_or_is_refused(&format!("{quotes} quotes, place {i}"), xml) {
                "refused in words" => refused += 1,
                "decoded and written" => written_ok += 1,
                _ => {}
            }
        }
    }
    // Past the cap (a namespace of 174,763 quotes or more is 1 MiB as written) it is refused, and under it the part is written and read again.
    assert!(refused > 0 && written_ok > 0, "refused {refused}, written {written_ok}");
}

#[test]
fn a_namespace_of_exactly_the_most_bytes_as_written_is_copied_and_one_byte_more_is_not() {
    // 174,762 quotes are 1,048,572 bytes as written; four more characters make exactly the reader's 1 MiB.
    let quotes = "\"".repeat((MAX_XML_ATTR_BYTES - 4) / 6);
    for (suffix, fits) in [("abcd", true), ("abcde", false)] {
        let uri = format!("{quotes}{suffix}");
        let attribute = format!("<page xmlns=\"{URN}\" xmlns:k='{uri}'><shape id=\"1\" k:future=\"1\"/></page>");
        let element = format!("<page xmlns=\"{URN}\" xmlns:k='{uri}'><shape id=\"1\"><k:x/></shape></page>");
        let default = dx(&format!("<QD:page xmlns:QD=\"{URN}\" xmlns='{uri}'><QD:shape id=\"1\"><z/></QD:shape></QD:page>"));
        for (place, xml) in [("attribute", attribute), ("element", element), ("default", default)] {
            let out = decodes_and_writes_or_is_refused(place, &xml);
            assert_eq!(out, if fits { "decoded and written" } else { "refused in words" }, "{place} with {suffix}");
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------, finding 8

#[test]
fn a_second_held_element_in_a_row_is_kept_and_is_not_the_one_that_replaces_the_first() {
    // A row holds one value, and a second `held` in its `odf` is what a newer build wrote: it is kept (its unknown content with it), and the first stays the row's.
    let xml = page_xml(&dx(
        "<QD:shape id=\"1\"><QD:sheet><QD:Section N=\"User\"><QD:Row N=\"r\"><QD:ext><QD:odf><QD:held group=\"fill\" fa=\"A\"/><QD:held group=\"line\" fb=\"B\"/><QD:held group=\"fill\" fc=\"C\"/></QD:odf></QD:ext></QD:Row></QD:Section></QD:sheet></QD:shape>",
    ));
    let (text, notes, page) = through(&xml);
    assert!(notes.is_empty(), "nothing was lost, so nothing is counted: {notes:?}");
    // The first keeps what was on it as an attribute of its own; the others are kept as markup, with each attribute a kept `attr` of a kept `el`.
    for kept in ["fa=\"A\"", "name=\"fb\" value=\"B\"", "name=\"fc\" value=\"C\""] {
        assert_eq!(text.matches(kept).count(), 1, "{kept}: {text}");
    }
    // The first is the row's value: the group of the first is the one the model holds.
    let sheet = &page.shapes.get(0).unwrap().sheet;
    let row = sheet.sections[0].rows.first().unwrap().clone();
    let held = row.ext.get().unwrap().odf.held.as_ref().unwrap();
    assert_eq!(held.group, diacraft_doc::HeldGroup::Fill);
    // And the second write is the first's bytes, and reads back to the same model.
    let again = page_of(&text).unwrap();
    assert!(again == page);
    assert_eq!(written(&again).unwrap(), text.into_bytes());
}

#[test]
fn unknown_elements_and_attributes_directly_in_the_odf_of_a_text_are_kept() {
    let xml = page_xml(&dx(
        "<QD:shape id=\"1\"><QD:text><QD:ext><QD:odf zz=\"attr\"><x:first xmlns:x=\"urn:x\" k=\"1\"/><QD:paragraph up=\"P0\"/><next k=\"2\"/><QD:paragraph up=\"P1\"/></QD:odf></QD:ext></QD:text></QD:shape>",
    ));
    let (text, notes, page) = through(&xml);
    assert!(notes.is_empty(), "kept, so not counted: {notes:?}");
    for kept in [
        "name=\"zz\" value=\"attr\"",
        "name=\"x:first\"",
        "name=\"k\" value=\"1\"",
        "name=\"next\"",
        "name=\"k\" value=\"2\"",
        "up=\"P0\"",
        "up=\"P1\"",
    ] {
        assert!(text.contains(kept), "{kept} was lost: {text}");
    }
    let again = page_of(&text).unwrap();
    assert!(again == page);
    assert_eq!(written(&again).unwrap(), text.into_bytes(), "the second write is the first's");
}
