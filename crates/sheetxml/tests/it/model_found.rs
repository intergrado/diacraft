//! Regression vectors from the fuzzer: inputs that once decoded to a document that did not come back as the property asked.
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{
    Document, Held, HeldGroup, HeldStyleValue, Kept, MAX_LIST_LEVELS, OdfListLevel, OdfParagraph, OdfRun, OdfShape, OdfStyle, OdfText, Page,
    Passthrough, Placed, RawAttrs, RawElement, RawNode, Shape, ShapeList, Style, TextGroup,
};
use diacraft_sheetxml::model::{BlobId, BlobSource, Budget, decode, part_lists};

use super::model_util::*;

/// Gives blobs 0 to 3, small, and loses the rest (as the fuzz target does).
struct Blobs;

impl BlobSource for Blobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        (id.0 < 4).then(|| Arc::from(vec![id.0 as u8; (id.0 as usize + 1) * 3]))
    }
}

fn page_part(id: u32) -> El {
    El {
        name: "d:page".to_owned(),
        attrs: vec![("xmlns:d".to_owned(), diacraft_sheetxml::model::URN.to_owned()), ("id".to_owned(), id.to_string())],
        children: Vec::new(),
    }
}

/// A document part, as the fuzz target decodes it, and what it makes through the codec and back.
fn decoded_and_back(xml: &str) -> (Document, Document) {
    let el = parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let lists = part_lists(&el).unwrap();
    let pages: Vec<El> = lists.pages.iter().map(|(id, _)| page_part(*id)).collect();
    let page_refs: Vec<&El> = pages.iter().collect();
    let doc = decode(&el, &page_refs, &[], &Blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new()).unwrap();
    let back = round_trip(&doc);
    (doc, back)
}

fn places(doc: &Document) -> Vec<u32> {
    doc.ext.get().map(|d| d.native.kept.iter().map(|p| p.after).collect()).unwrap_or_default()
}

/// The document with the places of the unknown elements of its native slot set to 0.
fn without_places(doc: &Document) -> Document {
    let mut out = doc.clone();
    let nodes: Vec<Placed> = out.ext.make_mut().native.kept.iter().map(|p| Placed { after: 0, node: p.node.clone() }).collect();
    out.ext.make_mut().native.kept = Passthrough::from_nodes(nodes);
    out
}

#[test]
fn a_place_after_a_known_child_that_is_not_written_again_is_closed_up_and_stays() {
    // The fuzzer's: a `theme` whose bytes are not a theme is a known child that is read and not kept, so the unknown element after it follows two known
    // children when it is read and one when it is written. The first write closes the place up; from there on the document is as written and comes back as it is.
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<diacraft:document xmlns:diacraft=\"urn:app.diacraft:extension\" version=\"2\" title-b64=\"AQo+CsOp77++fw==\"><diacraft:props creator=\"_h__\" last-modified-by=\"drky_H\" keywords=\"  leading and trailing  \" description=\"\" canted=\"--\"/><diacraft:t hmeepart=\"--\" blob=\"0\"/><diacraft:theme part=\"--\" blob=\"1\"/><di stamp=\"7272e0ba0cfd05e0f2\"/></diacraft:document>";
    let (doc, back) = decoded_and_back(xml);
    assert_eq!(places(&doc), [1, 2]);
    assert_eq!(places(&back), [1, 1]);
    assert!(without_places(&back) == without_places(&doc), "nothing else changes");
    let again = round_trip(&back);
    assert!(again == back);
    assert_eq!(pack(&back).document, pack(&again).document);
}

/// A document part holding `records` after the declaration, decoded and written and read back.
fn document_of(records: &str) -> (Document, Document) {
    decoded_and_back(&format!("<diacraft:document xmlns:diacraft=\"{}\" version=\"4\">{records}</diacraft:document>", diacraft_sheetxml::model::URN))
}

/// The names of the entries the document keeps for its records' unknown content.
fn kept_names(doc: &Document) -> Vec<String> {
    let nodes = doc.ext.get().map(|d| d.native.kept.iter().map(|p| p.node.clone()).collect::<Vec<_>>()).unwrap_or_default();
    nodes.iter().filter_map(|n| if let RawNode::Element(e) = n { Some(e.name.to_string()) } else { None }).collect()
}

/// The attributes of the entry named `name` in the document's native slot.
fn entry_attrs(doc: &Document, name: &str) -> Vec<(String, String)> {
    let nodes: Vec<RawNode> = doc.ext.get().map(|d| d.native.kept.iter().map(|p| p.node.clone()).collect()).unwrap_or_default();
    nodes
        .iter()
        .find_map(|n| match n {
            RawNode::Element(e) if &*e.name == name => Some(e.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn the_unknown_content_of_a_second_record_of_a_key_is_counted_and_not_kept() {
    // The fuzzer's: records that have the same key (two colour entries of index 0, one of them with no `ix`) cannot each take back an entry named by it,
    // so the first of a key has the entry and the others' unknown content is counted as lost; what is kept comes back on the record that had it.
    let (doc, back) = document_of(
        "<diacraft:color rgb=\"#000001\" first=\"1\"/><diacraft:color ix=\"0\" rgb=\"#000002\" second=\"2\"/><diacraft:color ix=\"7\" rgb=\"#000003\" third=\"3\"/>",
    );
    assert_eq!(kept_names(&doc), ["color[0]", "color[7]"]);
    assert_eq!(entry_attrs(&doc, "color[0]"), [("first".to_owned(), "1".to_owned())]);
    // The colours themselves are all three.
    assert_eq!(doc.colors.len(), 3);
    assert!(back == doc);
    // The record with nothing unknown, first of its key, has the key: the later record's content is not kept.
    let (doc, back) = document_of("<diacraft:color rgb=\"#000001\"/><diacraft:color ix=\"0\" rgb=\"#000002\" second=\"2\"/>");
    assert!(kept_names(&doc).is_empty());
    assert!(back == doc);
}

#[test]
fn a_part_that_is_lost_leaves_no_unknown_content_behind() {
    // The bytes of the part are not in the package, so its record and its relationships' unknown content go, and a later part of the same name keeps its own.
    let stamp = diacraft_sheetxml::model::stamp_of_bytes(&[0u8; 3]).text();
    let (doc, back) = document_of(&format!(
        "<diacraft:blob path=\"blobs/0.bin\" media-type=\"application/octet-stream\" size=\"3\" stamp=\"{stamp}\"/>\
         <diacraft:part format=\"vsdx\" name=\"/a\" media-type=\"x\" role=\"other\" blob=\"9\" lost=\"1\"><diacraft:rel source=\"/a\" id=\"r\" kind=\"k\" target=\"t\" relx=\"1\"/></diacraft:part>\
         <diacraft:part format=\"vsdx\" name=\"/a\" media-type=\"x\" role=\"other\" blob=\"0\" kept=\"1\"><diacraft:rel source=\"/a\" id=\"r\" kind=\"k\" target=\"t\" relx=\"2\"/></diacraft:part>"
    ));
    assert_eq!(doc.parts.len(), 1);
    assert_eq!(kept_names(&doc), ["rel[/a|r]", "part[/a]"]);
    // The entries are the second part's, and not the lost one's.
    assert_eq!(entry_attrs(&doc, "rel[/a|r]"), [("relx".to_owned(), "2".to_owned())]);
    assert_eq!(entry_attrs(&doc, "part[/a]"), [("kept".to_owned(), "1".to_owned())]);
    assert!(back == doc);
}

#[test]
fn a_second_child_of_what_an_element_holds_one_of_is_not_read_and_the_first_stays() {
    // `props` holds custom properties, whose unknown content is kept in the document's slot under their keys: a second `props` that replaced the first would
    // leave those entries with no record to take them back, and the document would not come back as it was read.
    let custom = "<diacraft:custom format=\"odf\" name=\"a\" value-type=\"string\" value=\"v\" later=\"1\"/>";
    let (doc, back) = document_of(&format!("<diacraft:props>{custom}</diacraft:props><diacraft:props/><diacraft:solution/><diacraft:solution/>"));
    assert_eq!(doc.props.custom.len(), 1);
    assert_eq!(kept_names(&doc), ["custom[odf:a]"]);
    assert!(back == doc);
    // And so for a shape's `ext` and `sheet`.
    let held = "<diacraft:held group=\"fill\" stamp=\"0000000000000001\" later=\"1\"/>";
    let xml = format!(
        "<diacraft:page xmlns:diacraft=\"{}\" id=\"1\"><diacraft:shape id=\"1\"><diacraft:ext><diacraft:odf>{held}</diacraft:odf></diacraft:ext><diacraft:ext/>\
         <diacraft:sheet/><diacraft:sheet><diacraft:Cell N=\"A\" V=\"1\"/></diacraft:sheet></diacraft:shape></diacraft:page>",
        diacraft_sheetxml::model::URN
    );
    let el = parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let mut notes = Vec::new();
    let page = diacraft_sheetxml::model::decode_page(&el, &Blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert_eq!(notes, [diacraft_sheetxml::model::DecodeNote::Unknown { count: 2 }]);
    assert!(page.shapes.get(0).unwrap().sheet.cells.is_empty(), "the second sheet is not read");
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    assert!(round_trip(&doc) == doc);
}

#[test]
fn held_values_of_two_groups_with_unknown_content_each_come_back_in_order() {
    let entry =
        |name: &str| RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs([("future-k", "1")]), children: Vec::new() });
    let held = |group| Held { group, stamp: None, attrs: RawAttrs::new(), kept: Passthrough::new() };
    let mut shape = Shape::new(1);
    let data = shape.ext.make_mut();
    data.odf = OdfShape {
        kept: Kept::default(),
        style_name: None,
        held: vec![held(HeldGroup::Shadow), held(HeldGroup::Glue), held(HeldGroup::Shadow)],
        units: RawAttrs::new(),
    };
    // Each value is keyed by its group and its ordinal among the values of that group (Call 24): the second shadow has an entry of its own.
    data.native.kept = Passthrough::from_nodes(vec![
        Placed { after: 0, node: entry("held[shadow.0]") },
        Placed { after: 0, node: entry("held[glue.0]") },
        Placed { after: 0, node: entry("held[shadow.1]") },
    ]);
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let back = round_trip(&doc);
    assert!(back == doc, "{}", super::model_diff::first_difference(&back, &doc).unwrap_or_default());
}

#[test]
fn a_style_that_holds_nothing_but_an_unknown_element_in_its_native_slot_comes_back() {
    // Found by the exactness property (a persisted seed): the generator's style of ID 0 with no name and no cell, whose slot holds one unknown element.
    let mut style = Style::default();
    style.ext.make_mut().native.kept = Passthrough::from_nodes(vec![Placed {
        after: 0,
        node: RawNode::Element(RawElement {
            name: Arc::from("q1:a"),
            attrs: RawAttrs::from_pairs([("xmlns:q1", "urn:test:q1")]),
            children: Vec::new(),
        }),
    }]);
    let mut doc = Document::empty();
    doc.styles = vec![Arc::new(style)].into_iter().collect();
    let back = round_trip(&doc);
    assert!(back == doc, "{}", super::model_diff::first_difference(&back, &doc).unwrap_or_default());
}

/// A childless element, as the model holds the element of a paragraph, a list or a run.
fn shell(name: &str, attrs: &[(&str, &str)]) -> Option<Arc<RawElement>> {
    Some(Arc::new(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() }))
}

/// The held values of a style, and the unknown content a newer build wrote on each, come back: the values of a common style are held as a shape's are, each keyed by its group and
/// its ordinal among the values of that group, and the values of its text properties by `style`, their group and ordinal.
#[test]
fn the_held_values_of_a_style_and_what_a_newer_build_wrote_on_them_come_back() {
    let entry =
        |name: &str| RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs([("future-k", "1")]), children: Vec::new() });
    let held = |group, stamp| Held { group, stamp, attrs: RawAttrs::from_pairs([("draw:fill", "hatch")]), kept: Passthrough::new() };
    let text_held =
        |group| HeldStyleValue { group, stamp: None, attrs: RawAttrs::from_pairs([("fo:text-align", "start")]), kept: Passthrough::new() };
    let mut style = Style::default();
    let data = style.ext.make_mut();
    data.odf = OdfStyle {
        held: vec![held(HeldGroup::Fill, Some(diacraft_doc::stamp::Stamp(7))), held(HeldGroup::Line, None), held(HeldGroup::Fill, None)],
        text_held: vec![text_held(TextGroup::HorzAlign), text_held(TextGroup::SpLine), text_held(TextGroup::HorzAlign)],
        ..OdfStyle::default()
    };
    data.native.kept = Passthrough::from_nodes(vec![
        Placed { after: 0, node: entry("held[fill.0]") },
        Placed { after: 0, node: entry("held[fill.1]") },
        Placed { after: 0, node: entry("held-style[style.HorzAlign.1]") },
    ]);
    let mut doc = Document::empty();
    doc.styles = vec![Arc::new(style)].into_iter().collect();
    let back = round_trip(&doc);
    assert!(back == doc, "{}", super::model_diff::first_difference(&back, &doc).unwrap_or_default());
}

/// The element of a paragraph and of a run, and the lists round a paragraph (a level that holds a list, an item, both or neither), come back in order.
#[test]
fn the_element_of_a_paragraph_a_run_and_the_levels_round_it_come_back() {
    let mut shape = Shape::new(1);
    let level = |list, item| OdfListLevel { list, item };
    let paragraph = OdfParagraph {
        element: shell("text:h", &[("text:outline-level", "2")]),
        levels: vec![
            level(shell("text:list", &[("text:style-name", "L")]), shell("text:list-item", &[])),
            level(None, None),
            level(None, shell("text:list-header", &[])),
            level(shell("text:list", &[]), None),
        ],
        runs: vec![
            OdfRun { from: 0, to: 4, element: shell("text:a", &[("xlink:href", "http://example.org/")]), ..OdfRun::default() },
            OdfRun { from: 1, to: 3, ..OdfRun::default() },
        ],
        ..OdfParagraph::default()
    };
    let mut text = diacraft_doc::Text::default();
    text.ext.make_mut().odf = OdfText { paragraphs: vec![paragraph, OdfParagraph::default()], ..OdfText::default() };
    shape.text = Some(Arc::new(text));
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let back = round_trip(&doc);
    assert!(back == doc, "{}", super::model_diff::first_difference(&back, &doc).unwrap_or_default());
}

/// A paragraph in more lists than the model allows is refused, by what it is: a limit, in words that name it. One at the cap is read.
#[test]
fn a_paragraph_in_more_levels_of_lists_than_the_cap_is_refused() {
    let page_with = |levels: usize| {
        format!(
            "<diacraft:page xmlns:diacraft=\"{}\" id=\"1\"><diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf><diacraft:paragraph>{}</diacraft:paragraph></diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape></diacraft:page>",
            diacraft_sheetxml::model::URN,
            "<diacraft:level/>".repeat(levels)
        )
    };
    let read = |levels: usize| {
        let el = parse_with(page_with(levels).as_bytes(), &native_limits()).unwrap();
        diacraft_sheetxml::model::decode_page(&el, &Blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new())
    };
    let page = read(MAX_LIST_LEVELS).expect("at the cap");
    let text = page.shapes.get(0).unwrap().text.clone().unwrap();
    assert_eq!(text.ext.get().unwrap().odf.paragraphs[0].levels.len(), MAX_LIST_LEVELS);
    let words = read(MAX_LIST_LEVELS + 1).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(words.contains("levels of lists") && words.contains(&MAX_LIST_LEVELS.to_string()), "{words}");
}

/// `crash-pi-empty-target.bin` of the narrow re-review (kind 2, a master part), as the fuzz target decodes it: an unknown element of a `connect`
/// holds the instruction `<??>`, which the reader takes and no writer can write. The drawing opened and its save was refused by the writer, in words no person
/// could act on. The instruction is dropped at the decode (and counted), and what the decode gives writes and comes back.
const PI_EMPTY_TARGET: &str = r####"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<diacraft:master xmlns:diacraft="urn:app.diacraft:extension" namu-b64="77+/Le+/vvCfmIAnCSY=" unique-id="6-9-l 8N  j" pattern-flags="5" rel-id="_Zdc2E9y_" icon="-" part-name-b64="CT4NAX/ChQ0=" xmlns:q9="urn:test:q9" q9:x="_.F" q9:y="j_-_"><q2:kaxpbl xmlns:q2="urn:test:q2" q2:q0="a &amp; b &lt; c > d &quot;e&quot;"/><diacraft:connect from-sheet="2" from-cell="_fs_." to-sheet="2" to-cell="" to-part="7"><diacraft:attr name=" " value="u.S  __"/><diacraft:attr name="--" value="a&#9;b&#10;c&#13;d"/></diacraft:connect><diacraft:connect from-sheet="2" from-cell=".m_ _D 203" from-part="4" to-sheet="4" to-cell="ue" to-part="3" future-0="43.1I" xmlns:q9="urn:test:q9" q9:x="a &amp; b &lt; c > d &quot;e&quot;" q9:y="AK5t-"><q1:uxiixz xmlns:q1="urn:test:q1">  pa<??>	dded
&#13;	 a &amp; b &lt; c &gt; d "e"</q1:uxiixz><diacraft:attr name="J4EC." value-b64="XS0JXe+/vyc+"/><diacraft:attr name="  leading and trailing  " value-b65="w6k+77+/Jw=}}}}}="/><diacraft:attr name="R" value-b64="woUBPF1dHyA="/><diacraft:ext><diacraft:odf><diacraft:attr name="R 5 " value="]]>"/></diacraft:odf><diacraft:vsdx><diacraft:rest key="a&#9;b&#10;c&#13;d"/><diacraft:rest key=".7vxrfRNX "><diacraft:attr name="--" value="  leading and trailing  "/><diacraft:attr name-b64="AR/vv77vv78=" value="K-Xb"/><diacraft:attr name="  leading and trailing  " value="a&#9;b&#10;c&#13;d"/><diacraft:kept after="1"><diacraft:el><diacraft:attr name="]]>" value="--"/><diacraft:attr name="JYZ73 a- " value="  leading and trailing  "/></diacraft:el></diacraft:kept><diacraft:kept after="2"><diacraft:el><diacraft:comment b64="1">77+/77++Jw==</diacraft:comment></diacraft:el></diacraft:kept></diacraft:rest></diacraft:vsdx></diacraft:ext></diacraft:connect><diacraft:connect from-sheet="4294967295" from-cell="]]>" to-cell="_s8RM." to-part="7"><diacraft:attr name="S_" value="  leading and trailing  "/></diacraft:connect><diacraft:attr name="3-5 tI2_gDem" value="__iP_ .IF4"/><diacraft:ext><diacraft:odf><diacraft:attr name="  leading and trailing  " value-b64="AR/vv77vv78="/></diacraft:odf></diacraft:ext></diacraft:master>"####;

#[test]
fn the_master_part_with_an_instruction_that_has_no_target_decodes_writes_and_comes_back() {
    let el =
        parse_with(PI_EMPTY_TARGET.as_bytes(), &diacraft_container::xml::XmlLimits { max_depth: 512, max_nodes: 300_000, max_attr_bytes: 1 << 20 })
            .unwrap();
    let mut notes = Vec::new();
    let master = diacraft_sheetxml::model::decode_master(&el, &Blobs, &mut Budget::new(4_000_000, 256 << 20, 64 << 20), &mut notes).unwrap();
    assert!(
        notes.iter().any(|n| matches!(n, diacraft_sheetxml::model::DecodeNote::Unknown { count } if *count >= 1)),
        "the instruction is counted: {notes:?}"
    );
    let mut doc = Document::empty();
    doc.masters.push(Arc::new(master));
    // The writer takes every part (the first version of this test is the crash: a node no writer writes).
    let (back, _) = unpack(&pack(&doc)).expect("what the decode gave is written and read again");
    let (again, _) = unpack(&pack(&back)).expect("and once more");
    assert!(again == back, "from there on it comes back as it is");
}
