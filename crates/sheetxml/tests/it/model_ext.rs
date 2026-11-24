//! The codec's hold on the types: what a newer build wrote (the `native` slots), the records that have no slot (NP7), the extension slots, the
//! parts and the held values. Every input is written by hand or built in code; the XML goes through the real reader at the native limits.
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, Node, parse_with};
use diacraft_doc::{
    Cell, CellName, ContentTypeRow, DefFamily, Definition, Document, Ext, Format, Held, HeldGroup, Kept, OdfDoc, OdfShape, Page, Part, PartRole,
    Passthrough, RawAttrs, RawElement, RawNode, Rel, Shape, ShapeList, Sheet, Stamp, Text, TextPart, Value, VsdxHeld, VsdxRest,
};
use diacraft_sheetxml::model::{BlobId, Budget, DecodeNote, MemBlobs, URN, decode, decode_page, encode, encode_page};

use super::model_diff::first_difference;
use super::model_util::*;

fn parsed(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

fn page_xml(inner: &str) -> String {
    format!("<diacraft:page xmlns:diacraft=\"{URN}\">{inner}</diacraft:page>")
}

fn decode_one(xml: &str) -> (Page, Vec<DecodeNote>) {
    let mut notes = Vec::new();
    let page = decode_page(&parsed(xml), &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    (page, notes)
}

/// The page as the codec writes it, parsed again.
fn written(page: &Page) -> El {
    parsed(&String::from_utf8(bytes_of_el(&encode_page(page, &mut MemBlobs::new()).unwrap())).unwrap())
}

fn names(e: &El) -> Vec<&str> {
    e.elements().map(|c| c.name.as_str()).collect()
}

fn attr<'a>(e: &'a El, name: &str) -> Option<&'a str> {
    e.attr(name)
}

fn native_of_shape(s: &Shape) -> Kept {
    s.ext.get().map(|d| d.native.clone()).unwrap_or_default()
}

#[test]
fn unknown_content_is_kept_in_native_and_written_back_in_place() {
    let xml = page_xml(
        "<diacraft:shape id=\"1\" future=\"1\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"1\" cfuture=\"x\"><diacraft:cellkid a=\"b\"/></diacraft:Cell></diacraft:sheet>\
         <diacraft:hatch a=\"b\"/><diacraft:text>t</diacraft:text></diacraft:shape>",
    );
    let (page, notes) = decode_one(&xml);
    assert!(notes.is_empty(), "{notes:?}");
    let shape = page.shapes.get(0).unwrap();
    // On the shape: the attribute as written, and the element after the one known child (the sheet) that came before it.
    let native = native_of_shape(shape);
    assert_eq!(native.raw.iter().collect::<Vec<_>>(), [("future", "1")]);
    let placed: Vec<(u32, &str)> = native.kept.iter().map(|p| (p.after, if let RawNode::Element(e) = &p.node { &*e.name } else { "" })).collect();
    assert_eq!(placed, [(1, "diacraft:hatch")]);
    // On the cell, which has no slot: in its extra, which the cell then has.
    let cell = shape.sheet.cells.get("PinX").unwrap();
    let extra = cell.extra.as_ref().expect("an extra is made to hold what a newer build wrote on the cell");
    let cn = &extra.ext.get().unwrap().native;
    assert_eq!(cn.raw.iter().collect::<Vec<_>>(), [("cfuture", "x")]);
    assert_eq!(cn.kept.len(), 1);
    // Written back: the same places, and no `extra` element, since the extra holds nothing of its own.
    let out = written(&page);
    let shape_el = out.elements().next().unwrap();
    assert_eq!(attr(shape_el, "future"), Some("1"));
    assert_eq!(names(shape_el), ["diacraft:sheet", "diacraft:hatch", "diacraft:text"]);
    let cell_el = shape_el.elements().next().unwrap().elements().next().unwrap();
    assert_eq!(attr(cell_el, "cfuture"), Some("x"));
    assert_eq!(names(cell_el), ["diacraft:cellkid"]);
    // And the second read gives the first's model: the unknown content survives any number of saves.
    let again = decode_page(&out, &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    assert!(again == page, "{}", first_difference(&doc_of(again.clone()), &doc_of(page.clone())).unwrap_or_default());
}

fn doc_of(page: Page) -> Document {
    let mut d = Document::empty();
    d.pages.push(Arc::new(page));
    d
}

#[test]
fn unknown_nodes_keep_their_places_among_the_known_children() {
    let xml = page_xml(
        "<diacraft:hatch n=\"0\"/><diacraft:shape id=\"1\"/><diacraft:hatch n=\"1\"/><diacraft:shape id=\"2\"/><diacraft:hatch n=\"2\"/><diacraft:hatch n=\"3\"/>",
    );
    let (page, _) = decode_one(&xml);
    let after: Vec<u32> = page.ext.get().unwrap().native.kept.iter().map(|p| p.after).collect();
    assert_eq!(after, [0, 1, 2, 2]);
    let out = written(&page);
    assert_eq!(names(&out), ["diacraft:hatch", "diacraft:shape", "diacraft:hatch", "diacraft:shape", "diacraft:hatch", "diacraft:hatch"]);
    let order: Vec<&str> = out.elements().filter_map(|e| e.attr("n")).collect();
    assert_eq!(order, ["0", "1", "2", "3"]);
}

#[test]
fn an_unknown_node_with_a_root_declared_prefix_writes_back_well_formed() {
    // The prefix is declared on the part's root only; the unknown element, and an unknown attribute, use it.
    let xml = format!(
        "<diacraft:page xmlns:diacraft=\"{URN}\" xmlns:x=\"urn:vendor\"><diacraft:shape id=\"1\" x:y=\"2\"><x:thing a=\"1\"><x:inner/></x:thing></diacraft:shape></diacraft:page>"
    );
    let (page, _) = decode_one(&xml);
    let native = native_of_shape(page.shapes.get(0).unwrap());
    // The attribute comes with the declaration of its prefix, before it.
    assert_eq!(native.raw.iter().collect::<Vec<_>>(), [("xmlns:x", "urn:vendor"), ("x:y", "2")]);
    let Some(RawNode::Element(thing)) = native.kept.iter().next().map(|p| &p.node) else { panic!("an unknown element") };
    assert_eq!(&*thing.name, "x:thing");
    assert_eq!(thing.attrs.iter().collect::<Vec<_>>(), [("xmlns:x", "urn:vendor"), ("a", "1")], "the declaration its names use is put on it");
    // Written back, the part is well-formed with the declarations where they are used, and no other part of it declares the prefix.
    let bytes = bytes_of_el(&encode_page(&page, &mut MemBlobs::new()).unwrap());
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("<x:thing xmlns:x=\"urn:vendor\""), "{text}");
    let out = parse_with(text.as_bytes(), &native_limits()).unwrap();
    assert!(out.attrs.iter().all(|(k, _)| k != "xmlns:x"), "the root declares only the codec's own prefix");
    let again = decode_page(&out, &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    assert!(again == page);
}

#[test]
fn an_unknown_element_in_the_default_namespace_takes_the_default_with_it() {
    // The codec's namespace is the default of this part, so an element no one knows is in it, and writes back declaring it.
    let xml = format!("<page xmlns=\"{URN}\"><shape id=\"1\"><hatch/></shape></page>");
    let (page, _) = decode_one(&xml);
    let native = native_of_shape(page.shapes.get(0).unwrap());
    let Some(RawNode::Element(hatch)) = native.kept.iter().next().map(|p| &p.node) else { panic!("an unknown element") };
    assert_eq!(hatch.attrs.iter().collect::<Vec<_>>(), [("xmlns", URN)]);
}

#[test]
fn an_unknown_element_inside_a_text_is_a_native_part_of_it() {
    let xml =
        page_xml("<diacraft:shape id=\"1\"><diacraft:text>a<diacraft:cp ix=\"1\"/><diacraft:future x=\"1\"/>b</diacraft:text></diacraft:shape>");
    let (page, _) = decode_one(&xml);
    let text = page.shapes.get(0).unwrap().text.as_ref().unwrap();
    assert_eq!(text.parts.len(), 4);
    assert!(matches!(&text.parts[0], TextPart::Chars(c) if &**c == "a"));
    assert!(matches!(&text.parts[1], TextPart::Cp(1)));
    assert!(matches!(&text.parts[2], TextPart::NativeRaw(n) if matches!(&**n, RawNode::Element(e) if &*e.name == "diacraft:future")));
    assert!(matches!(&text.parts[3], TextPart::Chars(c) if &**c == "b"));
    let out = written(&page);
    let t = out.elements().next().unwrap().elements().next().unwrap();
    assert_eq!(names(t), ["diacraft:cp", "diacraft:future"]);
    assert!(decode_page(&out, &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap() == page);
}

/// A document with a part and a colour, encoded, so that unknown content can be put on the records by hand.
fn document_with_records() -> (Document, MemBlobs, El, Vec<El>) {
    let mut d = Document::empty();
    d.parts.push(Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from("/a/b.bin"),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Other,
        manifest: RawAttrs::new(),
        rels: Arc::new(vec![Rel {
            source: Arc::from("/a/b.bin"),
            id: Arc::from("rId1"),
            kind: Arc::from("t"),
            target: Arc::from("u"),
            external: false,
        }]),
        bytes: Arc::from(&b"bytes"[..]),
    }));
    d.colors = Arc::new(vec![diacraft_doc::ColorEntry { ix: 30, rgb: Arc::from("#112233") }]);
    d.pages.push(Arc::new(Page::new(1, "P")));
    let mut blobs = MemBlobs::new();
    let e = encode(&d, &mut blobs).unwrap();
    let pages: Vec<El> = e.pages.iter().map(|(_, p)| p.clone()).collect();
    (d, blobs, e.document, pages)
}

fn element_mut<'a>(root: &'a mut El, local: &str) -> &'a mut El {
    root.children
        .iter_mut()
        .find_map(|n| if let Node::El(e) = n { if e.local() == local { Some(e) } else { None } } else { None })
        .unwrap_or_else(|| panic!("no {local}"))
}

fn decode_doc(document: &El, pages: &[El], blobs: &MemBlobs) -> Document {
    let page_refs: Vec<&El> = pages.iter().collect();
    decode(document, &page_refs, &[], blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap()
}

#[test]
fn unknown_content_on_a_part_record_comes_back_on_that_part() {
    let (original, blobs, mut document, pages) = document_with_records();
    // What a newer build wrote: an attribute and an element on the part, one on its relationship, and an attribute on the colour entry.
    {
        let part = element_mut(&mut document, "part");
        part.attrs.push(("future".to_owned(), "1".to_owned()));
        part.children.push(Node::El(El { name: "diacraft:newkid".to_owned(), attrs: vec![("k".to_owned(), "v".to_owned())], children: Vec::new() }));
        element_mut(part, "rel").attrs.push(("relfuture".to_owned(), "r".to_owned()));
    }
    element_mut(&mut document, "color").attrs.push(("colorfuture".to_owned(), "c".to_owned()));
    let doc = decode_doc(&document, &pages, &blobs);
    // The records themselves are as they were; what is unknown is in the document's native slot, under the kind and the key of each record.
    assert_eq!(doc.parts.get(0).unwrap().name, original.parts.get(0).unwrap().name);
    assert_eq!(doc.colors, original.colors);
    let native = doc.ext.get().unwrap().native.clone();
    let keys: Vec<&str> = native.kept.iter().filter_map(|p| if let RawNode::Element(e) = &p.node { Some(&*e.name) } else { None }).collect();
    assert_eq!(keys, ["color[30]", "rel[/a/b.bin|rId1]", "part[/a/b.bin]"]);
    // Written back: each record takes its own back.
    let mut blobs2 = MemBlobs::new();
    let again = encode(&doc, &mut blobs2).unwrap();
    let text = |e: &El| String::from_utf8(bytes_of_el(e)).unwrap();
    let out = text(&again.document);
    // The unknown element comes back with the declaration its name uses, which it carries itself.
    assert!(out.contains("future=\"1\"") && out.contains("<diacraft:newkid xmlns:diacraft=\"urn:app.diacraft:extension\" k=\"v\"/>"), "{out}");
    assert!(out.contains("relfuture=\"r\"") && out.contains("colorfuture=\"c\""), "{out}");
    assert!(!out.contains("part[") && !out.contains("color["), "a keyed entry is never written as an element: {out}");
    // And read again, the same document.
    let page_els: Vec<El> = again.pages.iter().map(|(_, p)| p.clone()).collect();
    let back = decode_doc(&again.document, &page_els, &blobs2);
    assert!(back == doc, "{}", first_difference(&back, &doc).unwrap_or_default());
    // A part that is gone takes its unknown content with it: the entry is dropped, and the rest still comes back.
    let mut gone = doc.clone();
    gone.parts = diacraft_doc::SharedVec::new();
    let written = encode(&gone, &mut MemBlobs::new()).unwrap();
    let out = text(&written.document);
    assert!(!out.contains("newkid") && !out.contains("relfuture"), "{out}");
    assert!(out.contains("colorfuture=\"c\""), "{out}");
}

#[test]
fn a_part_whose_bytes_are_lost_is_a_note_and_is_left_out() {
    let (_, mut blobs, document, pages) = document_with_records();
    blobs.lose(BlobId(0));
    let page_refs: Vec<&El> = pages.iter().collect();
    let mut notes = Vec::new();
    let doc = decode(&document, &page_refs, &[], &blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert!(doc.parts.is_empty());
    assert!(notes.contains(&DecodeNote::PartLost { name: "/a/b.bin".to_owned() }), "{notes:?}");
}

#[test]
fn an_empty_slot_writes_nothing_and_a_slot_of_defaults_equals_an_absent_one() {
    let mut shape = Shape::new(1);
    let mut p = Page::new(1, "P");
    // A slot taken for a write and left empty is the same as none.
    let _ = shape.ext.make_mut();
    let _ = p.ext.make_mut();
    p.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let d = doc_of(p);
    let packed = pack(&d);
    let text = String::from_utf8(packed.pages[0].clone()).unwrap();
    assert!(!text.contains("<diacraft:ext"), "{text}");
    assert!(round_trip(&d) == d);
}

#[test]
fn what_both_formats_hold_beyond_the_cells_comes_back() {
    let mut shape = Shape::new(1);
    let mut ext: Ext<OdfShape> = Ext::default();
    let data = ext.make_mut();
    data.odf.style_name = Some(Arc::from("gr1"));
    data.odf.held = vec![
        Held { group: HeldGroup::Fill, stamp: Some(Stamp(0x1234)), attrs: RawAttrs::from_pairs([("draw:fill", "hatch")]), kept: Passthrough::new() },
        // The frame's value has no stamp, and a group may hold two values.
        Held { group: HeldGroup::Frame, stamp: None, attrs: RawAttrs::from_pairs([("draw:transform", "skewX(1)")]), kept: Passthrough::new() },
        Held { group: HeldGroup::Fill, stamp: Some(Stamp(0)), attrs: RawAttrs::new(), kept: Passthrough::new() },
    ];
    data.vsdx = VsdxRest(vec![(Arc::from("Trigger"), Kept { raw: RawAttrs::from_pairs([("N", "RecalcColor")]), kept: Passthrough::new() })]);
    shape.ext = ext;
    let mut d = doc_of({
        let mut p = Page::new(1, "P");
        p.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        p
    });
    d.vsdx = Some(Arc::new(VsdxHeld {
        rels: vec![Rel { source: Arc::from("/"), id: Arc::from("rId9"), kind: Arc::from("k"), target: Arc::from("t"), external: true }],
        content_types: vec![
            ContentTypeRow::Default { extension: Arc::from("bin"), content_type: Arc::from("application/octet-stream") },
            ContentTypeRow::Override { part: Arc::from("/x.xml"), content_type: Arc::from("text/xml") },
        ],
        props_rest: vec![(Arc::from("Company"), Kept::default())],
        windows: Some(Arc::new(RawElement { name: Arc::from("Windows"), attrs: RawAttrs::new(), children: Vec::new() })),
    }));
    let odf = d.ext.make_mut();
    odf.odf = OdfDoc {
        mimetype: Some(Arc::from("application/vnd.oasis.opendocument.graphics")),
        definitions: vec![Definition {
            family: DefFamily::FillImage,
            name: Arc::from("img"),
            element: Arc::new(RawElement { name: Arc::from("draw:fill-image"), attrs: RawAttrs::new(), children: Vec::new() }),
            bytes: Some(Arc::from(&b"png"[..])),
        }],
        ..OdfDoc::default()
    };
    let mut props = (*d.props).clone();
    props.custom.push(diacraft_doc::CustomProp {
        format: Format::Odf,
        name: Arc::from("Reviewer"),
        value_type: Arc::from("string"),
        value: Arc::from("x"),
        raw: RawAttrs::new(),
    });
    d.props = Arc::new(props);
    let back = round_trip(&d);
    assert!(back == d, "{}", first_difference(&back, &d).unwrap_or_default());
    // The fill image's bytes are a blob, as a part's are.
    assert_eq!(pack(&d).blobs.len(), 1);
}

#[test]
fn a_cell_with_only_what_a_newer_build_wrote_has_no_extra_element_and_an_empty_extra_still_has_one() {
    let mut native_only = Cell::constant(Value::from_text("1"));
    let mut extra = diacraft_doc::CellExtra::default();
    extra.ext.make_mut().native = Kept { raw: RawAttrs::from_pairs([("future-0", "x")]), kept: Passthrough::new() };
    native_only.extra = Some(Arc::new(extra));
    let mut empty = Cell::constant(Value::from_text("2"));
    empty.extra = Some(Arc::new(diacraft_doc::CellExtra::default()));
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("A"), native_only);
    sheet.cells.insert(CellName::new("B"), empty);
    let mut d = Document::empty();
    d.sheet = Arc::new(sheet);
    assert!(round_trip(&d) == d);
    let packed = pack(&d);
    let text = String::from_utf8(packed.document).unwrap();
    assert_eq!(text.matches("<diacraft:extra").count(), 1, "{text}");
    assert!(text.contains("future-0=\"x\""), "{text}");
}

#[test]
fn text_parts_of_every_new_kind_come_back() {
    let mut s = Shape::new(1);
    s.text = Some(Arc::new(Text {
        parts: vec![
            TextPart::Chars(Arc::from("a")),
            TextPart::OdfRaw(Box::new(RawNode::Comment(Arc::from("odf")))),
            TextPart::OdfRaw(Box::new(RawNode::Element(RawElement {
                name: Arc::from("text:bookmark"),
                attrs: RawAttrs::from_pairs([("text:name", "b")]),
                children: Vec::new(),
            }))),
            TextPart::NativeRaw(Box::new(RawNode::Element(RawElement {
                name: Arc::from("q:n"),
                attrs: RawAttrs::from_pairs([("xmlns:q", "urn:q")]),
                children: Vec::new(),
            }))),
            TextPart::Chars(Arc::from("b")),
        ],
        ..Text::default()
    }));
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(vec![Arc::new(s)]);
    let d = doc_of(p);
    let back = round_trip(&d);
    assert!(back == d, "{}", first_difference(&back, &d).unwrap_or_default());
}

#[test]
fn the_generator_makes_every_kind_of_slot() {
    use diacraft_testkit::model::{ModelSize, any_document};
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[5u8; 32]));
    let strategy = any_document(ModelSize::Small);
    let mut seen = [0u32; 8];
    for _ in 0..300 {
        let d = strategy.new_tree(&mut runner).unwrap().current();
        let text = String::from_utf8_lossy(&pack(&d).document).into_owned()
            + &pack(&d).pages.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect::<String>();
        for (i, needle) in [
            "<diacraft:ext>",
            "<diacraft:part ",
            "<diacraft:vsdx-held",
            "<diacraft:custom ",
            "<diacraft:held ",
            "future-",
            "<diacraft:odf-raw>",
            "q1:note",
        ]
        .iter()
        .enumerate()
        {
            if text.contains(needle) {
                seen[i] += 1;
            }
        }
    }
    assert!(seen.iter().all(|n| *n > 0), "every kind of slot is generated: {seen:?}");
}

#[test]
fn the_generator_makes_every_kind_of_text_slot() {
    use diacraft_testkit::model::{ModelSize, any_document};
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    fn walk(shapes: &ShapeList, seen: &mut [u32; 9], groups: &mut std::collections::HashSet<&'static str>) {
        for s in shapes.iter() {
            if let Some(data) = s.text.as_ref().and_then(|t| t.ext.get()) {
                seen[0] += 1;
                let odf = &data.odf;
                seen[1] += u32::from(!odf.paragraphs.is_empty());
                seen[2] += u32::from(odf.paragraphs.iter().any(|p| !p.runs.is_empty()));
                seen[3] += u32::from(odf.paragraphs.iter().any(|p| p.style.name.is_some()));
                seen[4] += u32::from(odf.paragraphs.iter().any(|p| p.runs.iter().any(|r| !r.style.held.is_empty())));
                seen[5] += u32::from(odf.paragraphs.iter().any(|p| p.characters.is_some()));
                seen[6] += u32::from(!data.native.raw.is_empty());
                seen[7] += u32::from(data.native.kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if e.name.contains('['))));
                seen[8] += u32::from(data.native.kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if !e.name.contains('['))));
                for p in &odf.paragraphs {
                    for h in p.style.held.iter().chain(p.runs.iter().flat_map(|r| r.style.held.iter())) {
                        groups.insert(h.group.id());
                    }
                }
            }
            walk(&s.children, seen, groups);
        }
    }
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[7u8; 32]));
    let strategy = any_document(ModelSize::Small);
    let mut seen = [0u32; 9];
    let mut groups = std::collections::HashSet::new();
    for _ in 0..600 {
        let d = strategy.new_tree(&mut runner).unwrap().current();
        for page in d.pages.iter() {
            walk(&page.shapes, &mut seen, &mut groups);
        }
        for master in d.masters.iter() {
            walk(&master.shapes, &mut seen, &mut groups);
        }
    }
    assert!(seen.iter().all(|n| *n > 0), "every kind of text slot is generated: {seen:?}");
    assert_eq!(groups.len(), diacraft_doc::TextGroup::ALL.len(), "every group of a held style value is generated: {groups:?}");
}

/// A text with everything in its slot: paragraphs with runs, styles and held values of every group, a VSDX rest, and what a newer build wrote.
fn text_with_everything() -> Text {
    use diacraft_doc::{HeldStyleValue, OdfParagraph, OdfRun, OdfText, OdfTextStyle, TextGroup};
    let held = |group: TextGroup| HeldStyleValue {
        group,
        stamp: if group == TextGroup::Size { None } else { Some(Stamp(7)) },
        attrs: RawAttrs::from_pairs([("fo:font-size", "12pt")]),
        kept: Passthrough::new(),
    };
    let style = |name: Option<&str>| OdfTextStyle {
        name: name.map(Arc::from),
        stamp: Some(Stamp(99)),
        held: TextGroup::ALL.iter().copied().map(held).collect(),
    };
    let mut ext: Ext<OdfText> = Ext::default();
    let data = ext.make_mut();
    data.odf = OdfText {
        kept: Kept { raw: RawAttrs::from_pairs([("odf-attr", "1")]), kept: Passthrough::new() },
        paragraphs: vec![
            OdfParagraph {
                style: style(Some("P1")),
                characters: Some(Stamp(5)),
                runs: vec![
                    OdfRun { from: 0, to: 3, style: style(Some("T1")), element: None },
                    OdfRun { from: 3, to: 9, style: style(None), element: None },
                ],
                element: None,
                levels: Vec::new(),
            },
            // A paragraph that names none, holds nothing and has no runs, and one named the empty name.
            OdfParagraph::default(),
            OdfParagraph {
                style: OdfTextStyle { name: Some(Arc::from("")), ..OdfTextStyle::default() },
                characters: None,
                runs: Vec::new(),
                element: None,
                levels: Vec::new(),
            },
        ],
    };
    data.vsdx = VsdxRest(vec![(Arc::from("fld"), Kept { raw: RawAttrs::from_pairs([("N", "x")]), kept: Passthrough::new() })]);
    data.native = Kept {
        raw: RawAttrs::from_pairs([("future-0", "attribute of the text element")]),
        kept: Passthrough::from_nodes(vec![diacraft_doc::Placed {
            after: 0,
            node: RawNode::Element(RawElement { name: Arc::from("q:n"), attrs: RawAttrs::from_pairs([("xmlns:q", "urn:q")]), children: Vec::new() }),
        }]),
    };
    Text { parts: vec![TextPart::Chars(Arc::from("abcdefghi")), TextPart::Cp(1)], ext }
}

fn page_with_text(text: Text) -> Page {
    let mut s = Shape::new(1);
    s.text = Some(Arc::new(text));
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(vec![Arc::new(s)]);
    p
}

#[test]
fn a_texts_slot_comes_back_whole_and_is_written_inside_the_text() {
    let d = doc_of(page_with_text(text_with_everything()));
    let back = round_trip(&d);
    assert!(back == d, "{}", first_difference(&back, &d).unwrap_or_default());
    // The slot is the text's last child, its attributes are on the text element, and what a newer build wrote among the slot's children is inside the slot.
    let page = written(d.pages.get(0).unwrap());
    let text = page.elements().next().unwrap().elements().next().unwrap();
    assert_eq!(text.local(), "text");
    assert_eq!(attr(text, "future-0"), Some("attribute of the text element"));
    let children = names(text);
    assert_eq!(children.last().copied(), Some("diacraft:ext"));
    let ext = text.elements().last().unwrap();
    assert_eq!(names(ext), ["q:n", "diacraft:odf", "diacraft:vsdx"]);
    let odf = ext.elements().find(|e| e.local() == "odf").unwrap();
    let paragraphs: Vec<&El> = odf.elements().filter(|e| e.local() == "paragraph").collect();
    assert_eq!(paragraphs.len(), 3);
    // A default paragraph is an empty element, and one that names the empty name is told from it.
    assert!(paragraphs[1].children.is_empty() && paragraphs[1].attrs.is_empty());
    assert_eq!(attr(paragraphs[2].elements().next().unwrap(), "name"), Some(""));
    assert_eq!(attr(paragraphs[0], "characters"), Some(Stamp(5).text().as_str()));
    let styles: Vec<&El> = paragraphs[0].elements().filter(|e| e.local() == "text-style").collect();
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].elements().count(), diacraft_doc::TextGroup::ALL.len());
    // A held value without a stamp writes none.
    let first = styles[0].elements().next().unwrap();
    assert_eq!((attr(first, "group"), attr(first, "stamp")), (Some("Size"), None));
}

#[test]
fn an_unknown_attribute_on_a_text_and_an_unknown_element_in_its_slot_are_kept_where_they_were() {
    let xml = page_xml(
        "<diacraft:shape id=\"1\"><diacraft:text future=\"1\">a<diacraft:ext><diacraft:odf><diacraft:paragraph/></diacraft:odf><diacraft:later z=\"1\"/>\
         <diacraft:vsdx/><diacraft:last/></diacraft:ext>b<diacraft:unknown/></diacraft:text></diacraft:shape>",
    );
    let (page, notes) = decode_one(&xml);
    assert!(notes.is_empty(), "{notes:?}");
    let text = page.shapes.get(0).unwrap().text.as_ref().unwrap();
    // Parts: the bare texts, and the unknown element between and after them, as before.
    assert_eq!(text.parts.len(), 3);
    assert!(matches!(&text.parts[2], TextPart::NativeRaw(n) if matches!(&**n, RawNode::Element(e) if &*e.name == "diacraft:unknown")));
    let data = text.ext.get().unwrap();
    assert_eq!(data.odf.paragraphs.len(), 1);
    assert_eq!(data.native.raw.iter().collect::<Vec<_>>(), [("future", "1")]);
    // The two unknown elements of the slot follow one known child (the odf) and two (odf and vsdx).
    let placed: Vec<(u32, &str)> =
        data.native.kept.iter().map(|p| (p.after, if let RawNode::Element(e) = &p.node { &*e.name } else { "" })).collect();
    assert_eq!(placed, [(1, "diacraft:later"), (2, "diacraft:last")]);
    let out = written(&page);
    let text_el = out.elements().next().unwrap().elements().next().unwrap();
    assert_eq!(attr(text_el, "future"), Some("1"));
    let ext = text_el.elements().find(|e| e.local() == "ext").unwrap();
    assert_eq!(names(ext), ["diacraft:odf", "diacraft:later", "diacraft:last"], "the empty vsdx is not written again, and the place closes up");
    let again = decode_page(&out, &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    assert_eq!(
        again.shapes.get(0).unwrap().text.as_ref().unwrap().ext.get().unwrap().native.kept.iter().map(|p| p.after).collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn unknown_content_on_the_records_of_a_texts_data_is_kept_under_their_ordinals_and_comes_back_on_them() {
    let xml = page_xml(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf>\
           <diacraft:paragraph pfuture=\"1\">\
             <diacraft:text-style name=\"P\" sfuture=\"2\"><diacraft:held-style group=\"Size\" hfuture=\"3\"><diacraft:hkid/></diacraft:held-style></diacraft:text-style>\
             <diacraft:run from=\"1\" to=\"2\" rfuture=\"4\"><diacraft:text-style stamp=\"0000000000000001\" tfuture=\"5\"/></diacraft:run>\
           </diacraft:paragraph>\
           <diacraft:paragraph><diacraft:text-style hfuture=\"6\"/></diacraft:paragraph>\
         </diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>",
    );
    let (page, _) = decode_one(&xml);
    let text = page.shapes.get(0).unwrap().text.as_ref().unwrap();
    let entries: Vec<String> = text
        .ext
        .get()
        .unwrap()
        .native
        .kept
        .iter()
        .filter_map(|p| if let RawNode::Element(e) = &p.node { Some(e.name.to_string()) } else { None })
        .collect();
    // In the order the records are read: a style's values, the style, a run's style, the run, and the paragraph last.
    assert_eq!(entries, ["held-style[0.Size.0]", "text-style[0]", "text-style[0.0]", "run[0.0]", "paragraph[0]", "text-style[1]"]);
    // The records are as they were, and written back each takes its own.
    let odf = &text.ext.get().unwrap().odf;
    assert_eq!(odf.paragraphs.len(), 2);
    assert_eq!(odf.paragraphs[0].style.name.as_deref(), Some("P"));
    assert_eq!(odf.paragraphs[0].runs[0].style.stamp, Some(Stamp(1)));
    let out = written(&page);
    let p0 =
        out.elements().next().unwrap().elements().next().unwrap().elements().next().unwrap().elements().next().unwrap().elements().next().unwrap();
    assert_eq!(attr(p0, "pfuture"), Some("1"));
    let style = p0.elements().find(|e| e.local() == "text-style").unwrap();
    assert_eq!(attr(style, "sfuture"), Some("2"));
    assert_eq!(attr(style.elements().next().unwrap(), "hfuture"), Some("3"));
    assert_eq!(names(style.elements().next().unwrap()), ["diacraft:hkid"]);
    let run = p0.elements().find(|e| e.local() == "run").unwrap();
    assert_eq!(attr(run, "rfuture"), Some("4"));
    assert_eq!(attr(run.elements().next().unwrap(), "tfuture"), Some("5"));
    // A style that is only that is written for the sake of its unknown content, and comes back.
    let again = decode_page(&out, &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    assert!(again == page);
}

#[test]
fn a_second_slot_and_a_second_style_are_not_read() {
    // The unknown content of the records of what a second one would replace would be left with none, so the first of each is the one that is read.
    let xml = page_xml(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf><diacraft:paragraph>\
           <diacraft:text-style name=\"first\"/><diacraft:text-style name=\"second\"/>\
         </diacraft:paragraph></diacraft:odf><diacraft:odf><diacraft:paragraph/><diacraft:paragraph/></diacraft:odf></diacraft:ext>\
         <diacraft:ext><diacraft:odf><diacraft:paragraph/></diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>",
    );
    let (page, notes) = decode_one(&xml);
    let odf = &page.shapes.get(0).unwrap().text.as_ref().unwrap().ext.get().unwrap().odf;
    assert_eq!(odf.paragraphs.len(), 1);
    assert_eq!(odf.paragraphs[0].style.name.as_deref(), Some("first"));
    assert_eq!(notes, [DecodeNote::Unknown { count: 3 }]);
    // And what is read is a fixed point.
    let again = decode_page(&written(&page), &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    assert!(again == page);
}

#[test]
fn a_style_that_is_only_unknown_content_is_written_and_a_default_one_is_not() {
    let mut text = Text::default();
    text.ext.make_mut().odf.paragraphs.push(Default::default());
    let packed = pack(&doc_of(page_with_text(text)));
    let page = String::from_utf8(packed.pages[0].clone()).unwrap();
    assert!(page.contains("<diacraft:paragraph/>") && !page.contains("text-style"), "{page}");
}
