//! Part B: **equal sheets and sections are one allocation** (the design's 2.3, "shared pointers are not part of the value"). A decode keeps one copy
//! of each sheet and each section it reads and gives every equal one that copy, charged once; a write makes its own copy of what it changes; and nothing a caller
//! can compare shows the difference.
use std::sync::Arc;

use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{Document, Page, Section, Sheet};
use diacraft_sheetxml::model::cost::element_bytes;
use diacraft_sheetxml::model::{
    Budget, CodecError, DecodeNote, MemBlobs, SHARE_ENTRY_BYTES, SHARED_REF_BYTES, URN, check_decodes, decode, decode_master, decode_page, encode,
    encode_for_save, open_limits,
};

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;
const D: &str = "diacraft";

fn page_text(root_attrs: &str, inner: &str) -> String {
    format!("<{D}:page xmlns:{D}=\"{URN}\"{root_attrs} id=\"1\">{inner}</{D}:page>")
}

fn parse(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

/// The page part that says every unit in full, as a file of another writer may.
fn parse_text_page(inner: &str) -> El {
    parse(&page_text("", inner))
}

fn page_of(inner: &str) -> Page {
    decode_page(&parse(&page_text("", inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap()
}

/// A shape with this ID whose sheet is `sheet` (the inside of the `sheet` element).
fn shape(id: u32, sheet: &str) -> String {
    format!("<{D}:shape id=\"{id}\"><{D}:sheet>{sheet}</{D}:sheet></{D}:shape>")
}

fn geometry() -> String {
    format!(
        "<{D}:Section N=\"Geometry\" IX=\"0\"><{D}:Row IX=\"1\" T=\"RelMoveTo\"><{D}:Cell N=\"X\" V=\"0\"/><{D}:Cell N=\"Y\" V=\"0\"/></{D}:Row><{D}:Row IX=\"2\" T=\"RelLineTo\"><{D}:Cell N=\"X\" V=\"1\"/><{D}:Cell N=\"Y\" V=\"0\"/></{D}:Row></{D}:Section>"
    )
}

fn pin(x: u32) -> String {
    format!("<{D}:Cell N=\"PinX\" V=\"{x}\"/>")
}

fn sheet_of(page: &Page, at: usize) -> &Arc<Sheet> {
    &page.shapes.get(at).unwrap().sheet
}

// ------------------------------------------------------------------------------------------------------------------------------- what is shared

#[test]
fn shapes_whose_sheets_are_equal_have_one_sheet() {
    let page = page_of(&format!("{}{}{}", shape(1, &pin(5)), shape(2, &pin(5)), shape(3, &pin(6))));
    assert!(Arc::ptr_eq(sheet_of(&page, 0), sheet_of(&page, 1)), "equal text, one allocation");
    assert!(!Arc::ptr_eq(sheet_of(&page, 0), sheet_of(&page, 2)), "a different cell, a sheet of its own");
    assert_ne!(sheet_of(&page, 0), sheet_of(&page, 2));
    assert_eq!(sheet_of(&page, 0), sheet_of(&page, 1));
}

#[test]
fn shapes_that_have_no_sheet_share_the_one_empty_sheet() {
    let page = page_of(&format!(
        "<{D}:shape id=\"1\"/><{D}:shape id=\"2\"/><{D}:shape id=\"3\"><{D}:sheet/></{D}:shape><{D}:shape id=\"4\"><{D}:sheet/></{D}:shape>"
    ));
    assert!(Arc::ptr_eq(sheet_of(&page, 0), sheet_of(&page, 1)));
    assert_eq!(**sheet_of(&page, 0), Sheet::default());
    // A sheet that is written and is empty is the same value, and one copy of its own.
    assert_eq!(sheet_of(&page, 0), sheet_of(&page, 2));
    assert!(Arc::ptr_eq(sheet_of(&page, 2), sheet_of(&page, 3)));
}

#[test]
fn sheets_that_differ_share_the_sections_that_are_equal() {
    let one = format!("{}{}", pin(1), geometry());
    let two = format!("{}{}", pin(2), geometry());
    let page = page_of(&format!("{}{}", shape(1, &one), shape(2, &two)));
    let (a, b) = (sheet_of(&page, 0), sheet_of(&page, 1));
    assert!(!Arc::ptr_eq(a, b));
    assert_eq!((a.sections.len(), b.sections.len()), (1, 1));
    assert!(Arc::ptr_eq(&a.sections[0], &b.sections[0]), "the geometry is one section in two sheets");
    assert_eq!(a.sections[0].name.as_str(), "Geometry");
}

#[test]
fn sections_that_differ_by_a_cell_are_two_sections() {
    let flat = |v: u32| format!("<{D}:Section N=\"User\"><{D}:Row N=\"r\"><{D}:Cell N=\"Value\" V=\"{v}\"/></{D}:Row></{D}:Section>");
    let page = page_of(&format!("{}{}", shape(1, &flat(1)), shape(2, &flat(2))));
    assert!(!Arc::ptr_eq(&sheet_of(&page, 0).sections[0], &sheet_of(&page, 1).sections[0]));
}

#[test]
fn what_is_shared_crosses_the_parts_of_one_decode() {
    // Two pages and a master of one document: the same section in each is one.
    let part = |kind: &str, id: u32, inner: &str| format!("<{D}:{kind} xmlns:{D}=\"{URN}\" id=\"{id}\">{inner}</{D}:{kind}>");
    let document = format!(
        "<{D}:document xmlns:{D}=\"{URN}\"><{D}:page-ref id=\"1\" part=\"pages/page1.xml\"/><{D}:page-ref id=\"2\" part=\"pages/page2.xml\"/><{D}:master-ref id=\"3\" part=\"masters/master3.xml\"/></{D}:document>"
    );
    let body = shape(1, &format!("{}{}", pin(1), geometry()));
    let (p1, p2, m3) = (parse(&part("page", 1, &body)), parse(&part("page", 2, &body)), parse(&part("master", 3, &body)));
    let doc = decode(&parse(&document), &[&p1, &p2], &[&m3], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    let sheet = |p: usize| Arc::clone(&doc.pages.get(p).unwrap().shapes.get(0).unwrap().sheet);
    assert!(Arc::ptr_eq(&sheet(0), &sheet(1)), "two pages");
    assert!(Arc::ptr_eq(&sheet(0), &doc.masters.get(0).unwrap().shapes.get(0).unwrap().sheet), "and a master");
}

#[test]
fn a_page_sheet_and_a_shape_sheet_that_are_equal_are_one() {
    let sheet = format!("{}{}", pin(9), geometry());
    let page = page_of(&format!("<{D}:sheet>{sheet}</{D}:sheet>{}", shape(1, &sheet)));
    assert!(Arc::ptr_eq(&page.sheet, sheet_of(&page, 0)), "a page's sheet and a shape's");
}

#[test]
fn a_sheet_that_is_unlike_another_only_in_what_a_name_means_is_not_shared() {
    // The same text under two bindings of the prefix `q`: what the kept element is, in the model, carries the namespace the prefix was bound to, so the two are
    // different sheets and each keeps its own.
    let section = format!("<{D}:Section N=\"a\"><q:x/></{D}:Section>");
    let inner = format!(
        "<{D}:shape id=\"1\" xmlns:q=\"urn:one\"><{D}:sheet>{section}</{D}:sheet></{D}:shape><{D}:shape id=\"2\" xmlns:q=\"urn:two\"><{D}:sheet>{section}</{D}:sheet></{D}:shape><{D}:shape id=\"3\" xmlns:q=\"urn:one\"><{D}:sheet>{section}</{D}:sheet></{D}:shape>"
    );
    let page = page_of(&inner);
    let section_at = |i: usize| Arc::clone(&sheet_of(&page, i).sections[0]);
    assert!(!Arc::ptr_eq(&section_at(0), &section_at(1)), "urn:one and urn:two");
    assert_ne!(section_at(0), section_at(1));
    let kept = |s: &Arc<Section>| format!("{:?}", s.ext.get().map(|d| d.native.clone()));
    assert!(kept(&section_at(0)).contains("urn:one"), "{}", kept(&section_at(0)));
    assert!(kept(&section_at(1)).contains("urn:two"), "{}", kept(&section_at(1)));
    assert_eq!(section_at(0), section_at(2), "the same namespace gives the same section, shared or not");
}

#[test]
fn the_text_of_a_unit_is_the_whole_of_its_key() {
    // Where one string ends and the next begins, the order of the attributes, a text, a comment and a nested element are each part of what a unit is.
    let cell = |attrs: &str| format!("<{D}:Cell {attrs}/>");
    let units = [
        cell("N=\"ab\" V=\"c\""),
        cell("N=\"a\" V=\"bc\""),
        cell("V=\"c\" N=\"ab\""),
        cell("N=\"ab\" V=\"c\" U=\"\""),
        format!("<{D}:Cell N=\"ab\" V=\"c\"><!-- x --></{D}:Cell>"),
        format!("<{D}:Cell N=\"ab\" V=\"c\"> </{D}:Cell>"),
        format!("<{D}:Cell N=\"ab\" V=\"c\"><{D}:extra/></{D}:Cell>"),
    ];
    let page = page_of(&units.iter().enumerate().map(|(i, u)| shape(i as u32, u)).collect::<String>());
    for i in 0..units.len() {
        for j in i + 1..units.len() {
            // Sheets that are the same value are one sheet, and ones that are not are two: the key never makes two units one that decode to different values.
            let (a, b) = (sheet_of(&page, i), sheet_of(&page, j));
            assert_eq!(Arc::ptr_eq(a, b), a == b && units[i] == units[j], "{} against {}", units[i], units[j]);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- what is charged

/// What a decode of a page of `n` shapes whose sheets are the same one spends, with and without sharing: the items, and the bytes.
fn spent(n: usize, sharing: bool) -> (u64, u64) {
    let inner: String = (0..n).map(|i| shape(i as u32, &format!("{}{}", pin(1), geometry()))).collect();
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    if !sharing {
        budget = budget.without_sharing();
    }
    decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
    (HUGE - budget.items, HUGE - budget.bytes)
}

#[test]
fn a_unit_paid_for_out_of_the_blobs_room_is_shared_as_one_paid_for_out_of_the_models_own_pool() {
    // The model may take the room of the blobs' pool that the blobs have not spent: a unit that costs it that room cost no blob, and the units equal to it are shared, as they are
    // when the model's own pool pays. Here the model's own pool is nothing, so the blobs' room pays for all of it.
    let inner: String = (0..40).map(|i| shape(i as u32, &format!("{}{}", pin(1), geometry()))).collect();
    let el = parse(&page_text("", &inner));
    let mut own = Budget::new(HUGE, HUGE, 0);
    decode_page(&el, &MemBlobs::new(), &mut own, &mut Vec::new()).unwrap();
    let mut lent = Budget::new(HUGE, 0, HUGE).with_lent_blobs();
    decode_page(&el, &MemBlobs::new(), &mut lent, &mut Vec::new()).unwrap();
    assert_eq!(HUGE - lent.blob_bytes, HUGE - own.bytes, "the same units are shared, whichever pool pays");
    assert_eq!(lent.items, own.items);
}

#[test]
fn a_unit_that_is_shared_costs_its_items_and_a_reference_and_not_itself() {
    let (items_off, bytes_off) = (|n| spent(n, false).0, |n| spent(n, false).1);
    let (items_on, bytes_on) = (|n| spent(n, true).0, |n| spent(n, true).1);
    // The items are what the parse took: they do not change with sharing.
    for n in 1..4 {
        assert_eq!(items_on(n), items_off(n), "{n} shapes");
    }
    // Each shape after the first costs its own element and a reference to the sheet, which is all of it: a reference, in a sheet, in a shape.
    let per_shape_on = bytes_on(3) - bytes_on(2);
    assert_eq!(bytes_on(2) - bytes_on(1), per_shape_on, "every later shape costs the same");
    // What a shape after the first costs, to the byte: its element, its one attribute (the figure for an attribute and the byte of its value), and the reference to
    // the sheet it holds.
    // The byte of its value is text, at nine eighths: two bytes (text is charged at nine eighths of its length, rounded up).
    let byte = diacraft_sheetxml::model::cost::text_bytes(1);
    assert_eq!(
        per_shape_on,
        element_bytes("shape") + diacraft_sheetxml::model::cost::ATTR_BYTES + byte + SHARED_REF_BYTES,
        "a shape and a reference"
    );
    // And what is not shared costs what it did: the first shape is charged its sheet in full, and the entries on top.
    let per_shape_off = bytes_off(3) - bytes_off(2);
    assert!(per_shape_off > 5 * per_shape_on, "{per_shape_off} unshared against {per_shape_on} shared");
    let first_on = bytes_on(1);
    assert!(first_on >= bytes_off(1), "the first copy is charged in full: {first_on} against {}", bytes_off(1));
    // The entries of its sheet and its section, and the streams they hold (the text of what they are, which a later unit is compared with): a cell's value of
    // `k` more characters is `k` more in each of the two streams that hold it, and nothing else about the entries changes.
    let entries_with = |value: &str| {
        let sheet = format!("{}<{D}:Section N=\"User\"><{D}:Cell N=\"X\" V=\"{value}\"/></{D}:Section>", pin(1));
        let inner = shape(1, &sheet);
        let mut on = Budget::new(HUGE, HUGE, HUGE);
        let mut off = Budget::new(HUGE, HUGE, HUGE).without_sharing();
        decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut on, &mut Vec::new()).unwrap();
        decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut off, &mut Vec::new()).unwrap();
        // The value is charged as a value in both; the entries are what the sharing decode spent more.
        off.bytes - on.bytes
    };
    let (short, long) = (entries_with("1"), entries_with(&"1".repeat(101)));
    assert_eq!(long - short, 2 * 100, "each of the sheet's and the section's streams is a hundred bytes longer");
    assert!(short > 2 * SHARE_ENTRY_BYTES, "an entry is its price and its stream: {short}");
}

#[test]
fn the_entry_of_a_unit_is_charged_before_it_is_made_and_a_unit_that_cannot_pay_for_it_is_refused() {
    let inner = shape(1, &format!("{}{}", pin(1), geometry()));
    let el = parse(&page_text("", &inner));
    let needed = {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let attempt = |bytes: u64| decode_page(&el, &MemBlobs::new(), &mut Budget::new(HUGE, bytes, HUGE), &mut Vec::new());
    assert!(attempt(needed).is_ok());
    for short in 1..=2 * SHARE_ENTRY_BYTES + 8 {
        assert!(matches!(attempt(needed - short), Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "{short} short");
    }
}

/// The sharing decode's price beside the one that shares nothing, for the shapes `inner` says: the entries, the streams and what the streams' chunks have beyond them.
fn spent_on_sharing(inner: &str) -> u64 {
    let mut on = Budget::new(HUGE, HUGE, HUGE);
    let mut off = Budget::new(HUGE, HUGE, HUGE).without_sharing();
    decode_page(&parse(&page_text("", inner)), &MemBlobs::new(), &mut on, &mut Vec::new()).unwrap();
    decode_page(&parse(&page_text("", inner)), &MemBlobs::new(), &mut off, &mut Vec::new()).unwrap();
    off.bytes - on.bytes
}

#[test]
fn the_room_of_a_chunk_of_streams_is_charged_by_the_stream_that_makes_the_chunk_before_it_is_made() {
    // The streams of the units a decode has met are kept in chunks, the first as long as the first stream and each next twice as long as the last. A table's first
    // stream makes a chunk of its own length and is charged its length; a stream that does not fit in the last chunk makes one twice as long, and is charged the room beyond
    // itself too, so that the memory of the chunk is spent before the chunk is made. Two shapes whose sheets and sections differ in a value of the same length have streams of the
    // same length: the second shape's make a chunk of twice the first's, so the room beyond them is the first shape's streams over again.
    let sheet = |value: &str| format!("{}<{D}:Section N=\"User\"><{D}:Cell N=\"X\" V=\"{value}\"/></{D}:Section>", pin(1));
    let one = spent_on_sharing(&shape(1, &sheet("1")));
    let streams = one - 2 * SHARE_ENTRY_BYTES;
    assert!(streams > 0, "an entry is its price and its stream: {one}");
    let two = spent_on_sharing(&format!("{}{}", shape(1, &sheet("1")), shape(2, &sheet("2"))));
    assert_eq!(
        two,
        2 * one + streams,
        "two entries each, their streams, and the room of the second chunk of each table, which is as long as the first"
    );
    // A third shape's streams fit in the room the second's chunk has left: they are charged as they are and nothing beyond.
    let three = spent_on_sharing(&format!("{}{}{}", shape(1, &sheet("1")), shape(2, &sheet("2")), shape(3, &sheet("3"))));
    assert_eq!(three, two + one, "the third's streams are in the room that was charged");
}

#[test]
fn the_unknown_content_of_a_shared_unit_is_counted_each_time_it_is_read() {
    // A second `ext` of a section is not read and is counted: three such sections are three, shared or not.
    let section = format!("<{D}:Section N=\"a\"><{D}:ext/><{D}:ext/></{D}:Section>");
    let counted = |sharing: bool| {
        let inner: String = (0..3).map(|i| shape(i, &section)).collect();
        let (mut notes, mut budget) = (Vec::new(), Budget::new(HUGE, HUGE, HUGE));
        if !sharing {
            budget = budget.without_sharing();
        }
        let page = decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut budget, &mut notes).unwrap();
        (notes, page)
    };
    let (shared_notes, shared) = counted(true);
    let (alone_notes, alone) = counted(false);
    assert_eq!(shared_notes, vec![DecodeNote::Unknown { count: 3 }]);
    assert_eq!(shared_notes, alone_notes);
    assert_eq!(shared, alone, "the model is the same value");
}

#[test]
fn the_unknown_count_of_a_shared_unit_takes_no_attribute_left_by_the_element_before_it() {
    // The review's case: an `ext` with two attributes nobody reads, before a section that three shapes hold. What the `ext` left is its own to count (two), and not
    // the section's: a section that counted it would give each shape that holds the same section two more.
    let section = format!("<{D}:Section N=\"User\"><{D}:Row N=\"r\"><{D}:Cell N=\"Value\" V=\"1\"/></{D}:Row></{D}:Section>");
    let inner = format!(
        "<{D}:shape id=\"1\"><{D}:sheet><{D}:ext zz=\"1\" yy=\"2\"/>{section}</{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:sheet><{D}:Cell N=\"PinX\" V=\"2\"/>{section}</{D}:sheet></{D}:shape><{D}:shape id=\"3\"><{D}:sheet><{D}:Cell N=\"PinX\" V=\"3\"/>{section}</{D}:sheet></{D}:shape>"
    );
    let notes_of = |sharing: bool| {
        let (mut notes, mut budget) = (Vec::new(), Budget::new(HUGE, HUGE, HUGE));
        if !sharing {
            budget = budget.without_sharing();
        }
        decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut budget, &mut notes).unwrap();
        notes
    };
    let (shared, alone) = (notes_of(true), notes_of(false));
    assert_eq!(alone, vec![DecodeNote::Unknown { count: 2 }], "the two attributes of the ext, once");
    assert_eq!(shared, alone, "sharing changes nothing of what the note says was lost");
}

// ------------------------------------------------------------------------------------------------------------------------------- what a caller sees

#[test]
fn a_write_to_one_shape_makes_its_own_sheet_and_leaves_the_others() {
    let mut page = page_of(&format!("{}{}{}", shape(1, &pin(5)), shape(2, &pin(5)), shape(3, &pin(5))));
    let before = sheet_of(&page, 2).clone();
    let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut first.sheet);
    sheet.links.line = Some(7);
    assert_eq!(sheet_of(&page, 0).links.line, Some(7));
    assert_eq!(sheet_of(&page, 1).links.line, None, "the second shape's sheet is as it was");
    assert!(Arc::ptr_eq(sheet_of(&page, 1), sheet_of(&page, 2)), "and the others still share theirs");
    assert_eq!(**sheet_of(&page, 2), *before);
}

#[test]
fn a_document_that_shares_is_equal_to_one_that_does_not() {
    let inner: String = (0..20).map(|i| shape(i, &format!("{}{}", pin(i % 3), geometry()))).collect();
    let with = decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    let without =
        decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE).without_sharing(), &mut Vec::new()).unwrap();
    assert_eq!(with, without);
    let sheets = |p: &Page| p.shapes.iter().map(|s| Arc::as_ptr(&s.sheet)).collect::<std::collections::HashSet<_>>().len();
    assert_eq!(sheets(&with), 3, "three sheets in twenty shapes");
    assert_eq!(sheets(&without), 20);
}

#[test]
fn what_a_model_that_shares_encodes_to_and_what_one_that_does_not_encodes_to_is_one_file_that_says_each_unit_once_and_both_are_the_same_drawing() {
    // The encoder writes what the model holds, **equal units by value**: a sheet that several shapes hold, as one allocation or as equal ones, once with
    // a name, and a `same` for each of the others. They are one file of one drawing.
    let inner: String = (0..10).map(|i| shape(i, &format!("{}{}", pin(i % 2), geometry()))).collect();
    let made = |sharing: bool| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        if !sharing {
            budget = budget.without_sharing();
        }
        let page = decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page));
        doc
    };
    let (shared, alone) = (made(true), made(false));
    assert_eq!(shared, alone, "the same drawing");
    let (a, b) = (encode(&shared, &mut MemBlobs::new()).unwrap(), encode(&alone, &mut MemBlobs::new()).unwrap());
    assert_eq!(a, b, "the model that holds its equal units apart is written as the one that shares them: each unit once");
    assert!(diacraft_sheetxml::model::written_len(&b.pages[0].1) * 2 < diacraft_sheetxml::model::written_len(&parse_text_page(&inner)));
    // And each reads back as the drawing, whether or not the units it is read as are shared.
    for encoded in [&a, &b] {
        let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
        for budget in [Budget::new(HUGE, HUGE, HUGE), Budget::new(HUGE, HUGE, HUGE).without_sharing()] {
            let mut budget = budget;
            let back = decode(&encoded.document, &pages, &[], &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
            assert_eq!(back, shared);
        }
    }
}

#[test]
fn the_content_stamp_of_a_drawing_is_the_same_whether_the_model_shares_what_is_equal_or_not_and_whether_the_file_says_a_unit_once_or_not() {
    // A stamp that was stored with a drawing is good for the file that says its units once. The stamp is of the content, walked as the part would be if it
    // said nothing once (each `same` the element it names, no `def`), so the model that shares and the one that does not have the stamp of the second's encoding.
    let inner: String = (0..12).map(|i| shape(i, &format!("{}{}", pin(i % 3), geometry()))).collect();
    let made = |sharing: bool| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        if !sharing {
            budget = budget.without_sharing();
        }
        let page = decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page));
        doc
    };
    let (shared, alone) = (made(true), made(false));
    // What each says: one with names and references, one in full.
    let says = |doc: &Document| {
        let encoded = encode(doc, &mut MemBlobs::new()).unwrap();
        let text = String::from_utf8(diacraft_container::xml::to_bytes(&encoded.pages[0].1, &[], &[], false).unwrap()).unwrap();
        (text.matches("<diacraft:same").count(), text.matches(" def=\"").count())
    };
    assert_eq!(says(&alone), says(&shared), "the model that holds equal units apart says them once too, by value");
    assert!(says(&shared).0 >= 9 && says(&shared).1 >= 3, "the model that shares says units once: {:?}", says(&shared));
    let stamp = |doc: &Document| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        let (stamp, cost) = diacraft_sheetxml::model::content_stamp_counted(doc, &mut budget).unwrap();
        (stamp, cost)
    };
    let ((a, a_cost), (b, b_cost)) = (stamp(&shared), stamp(&alone));
    assert_eq!(a, b, "one drawing, one stamp");
    assert_eq!(a_cost.items, b_cost.items, "what is walked is what the part would be if it said nothing once");
    // And a unit that a `same` names is walked where each stands: the items are those of the full parts.
    assert!(a_cost.items > a_cost.largest_part_items.min(1));
}

/// What a decode of the page whose shapes are `inner` is charged in items, and the notes it gives, with the budget of the check and of the open alike.
fn items_and_notes(inner: &str) -> (u64, Vec<DecodeNote>) {
    let mut notes = Vec::new();
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    decode_page(&parse(&page_text("", inner)), &MemBlobs::new(), &mut budget, &mut notes).unwrap();
    (HUGE - budget.items, notes)
}

#[test]
fn a_part_that_says_a_unit_once_is_charged_the_items_and_the_lost_content_of_the_part_that_says_it_in_full() {
    // The unit: a section of two cells and a second `ext`, which is not read and is counted as lost. Four shapes hold it.
    let section = |name: &str| {
        format!(
            "<{D}:Section N=\"User\"{name}><{D}:Cell N=\"a\" V=\"1\"/><{D}:Row N=\"r\"><{D}:Cell N=\"b\" V=\"2\"/></{D}:Row><{D}:ext/><{D}:ext/></{D}:Section>"
        )
    };
    let full: String = (0..4).map(|i| shape(i, &format!("{}{}", pin(i), section("")))).collect();
    let once: String = (0..4)
        .map(|i| {
            let unit = if i == 0 { section(" def=\"1\"") } else { format!("<{D}:same of=\"1\"/>") };
            shape(i, &format!("{}{}", pin(i), unit))
        })
        .collect();
    let ((full_items, full_notes), (once_items, once_notes)) = (items_and_notes(&full), items_and_notes(&once));
    assert_eq!(full_notes, vec![DecodeNote::Unknown { count: 4 }], "the second ext of each place");
    assert_eq!(once_notes, full_notes, "each place that stands for the unit counts what the unit lost");
    assert_eq!(once_items, full_items, "and is charged the items of the unit");
    // The same for a sheet, which a shape holds in the place of its own.
    let sheet = |def: &str| format!("<{D}:sheet{def}><{D}:Cell N=\"PinX\" V=\"1\"/>{}</{D}:sheet>", section(""));
    let full: String = (0..5).map(|i| format!("<{D}:shape id=\"{i}\">{}</{D}:shape>", sheet(""))).collect();
    let once: String = (0..5)
        .map(|i| {
            let unit = if i == 0 { sheet(" def=\"s\"") } else { format!("<{D}:same of=\"s\"/>") };
            format!("<{D}:shape id=\"{i}\">{unit}</{D}:shape>")
        })
        .collect();
    let ((full_items, full_notes), (once_items, once_notes)) = (items_and_notes(&full), items_and_notes(&once));
    assert_eq!(full_notes, vec![DecodeNote::Unknown { count: 5 }]);
    assert_eq!((once_items, once_notes), (full_items, full_notes));
}

#[test]
fn a_same_is_charged_what_the_unit_it_names_cost_so_that_references_cannot_take_more_than_the_items_the_budget_has() {
    // A budget of the items of the file that says the unit in full: the part that says it once takes exactly that, and one item fewer refuses it.
    let unit = format!("<{D}:Section N=\"User\">{}</{D}:Section>", (0..20).map(|i| format!("<{D}:Cell N=\"c{i:02}\"/>")).collect::<String>());
    let full: String = (0..6).map(|i| shape(i, &unit)).collect();
    let once: String = (0..6)
        .map(|i| shape(i, &if i == 0 { unit.replace("N=\"User\"", "N=\"User\" def=\"1\"") } else { format!("<{D}:same of=\"1\"/>") }))
        .collect();
    let (full_items, _) = items_and_notes(&full);
    let with =
        |text: &str, items: u64| decode_page(&parse(&page_text("", text)), &MemBlobs::new(), &mut Budget::new(items, HUGE, HUGE), &mut Vec::new());
    assert!(with(&once, full_items).is_ok(), "as many items as the part in full");
    let refused = with(&once, full_items - 1);
    assert!(matches!(refused, Err(CodecError::Limit { what: "items in the drawing", .. })), "{refused:?}");
}

/// What a decode of the page of `inner` says: its refusal in words, or the notes it gave and the page.
fn read_of(inner: &str) -> Result<(diacraft_doc::Page, Vec<DecodeNote>), String> {
    let mut notes = Vec::new();
    decode_page(&parse(&page_text("", inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes)
        .map(|page| (page, notes))
        .map_err(|e| e.to_string())
}

#[test]
fn a_same_that_names_no_unit_is_not_read_and_a_second_name_is_read_as_no_name_and_both_are_counted_and_the_rest_of_the_file_opens() {
    // What a program of 1.0 does to a file of 1.1 without knowing 3.19 (a change of the order of the shapes, the deletion of the shape that holds a
    // `def`, a duplicate that carries a kept `def`) is not damage: what is left unread is counted, as an unknown unit is (5.3), and the file opens.
    let sheet = |name: &str| format!("<{D}:sheet def=\"{name}\"><{D}:Cell N=\"A\" V=\"1\"/></{D}:sheet>");
    let same = |of: &str| format!("<{D}:same of=\"{of}\"/>");
    let counted = |inner: &str, count: u64, sheets: &[bool]| {
        let (page, notes) = read_of(inner).unwrap_or_else(|e| panic!("refused: {e}: {inner}"));
        assert_eq!(notes, vec![DecodeNote::Unknown { count }], "{inner}");
        let have: Vec<bool> = page.shapes.iter().map(|s| !s.sheet.cells.is_empty()).collect();
        assert_eq!(have, sheets, "which shapes have a sheet: {inner}");
    };
    let clean = |inner: &str| {
        let (_, notes) = read_of(inner).unwrap_or_else(|e| panic!("refused: {e}: {inner}"));
        assert_eq!(notes, vec![], "{inner}");
    };
    clean(&format!("{}{}", shape_in(&sheet("1")), shape_in(&same("1"))));
    // The `same` before the unit it names (the shapes were swapped), a name that no unit has, and a unit that was deleted: no unit is named, the shape has no sheet.
    counted(&format!("{}{}", shape_in(&same("1")), shape_in(&sheet("1"))), 1, &[false, true]);
    counted(&format!("{}{}", shape_in(&sheet("1")), shape_in(&same("2"))), 1, &[true, false]);
    counted(&shape_in(&same("1")), 1, &[false]);
    // A second unit of one name is **read as a unit of no name**: its shape keeps its sheet, the name it did not get is counted, and the first has the name, so that the
    // `same` after them is the first's. Whether the other is a sheet or a section.
    counted(&format!("{}{}{}", shape_in(&sheet("1")), shape_in(&sheet("1")), shape_in(&same("1"))), 1, &[true, true, true]);
    let section = format!("<{D}:sheet><{D}:Section N=\"U\" def=\"x\"/></{D}:sheet>");
    counted(&format!("{}{}", shape_in(&section), shape_in(&sheet("x"))), 1, &[false, true]);
    // And the `same` that follows names the first's, not the second's.
    let first_value = |inner: &str| {
        let (page, _) = read_of(inner).expect("opens");
        page.shapes.get(2).unwrap().sheet.cells.get("A").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    let one = |v: &str| format!("<{D}:sheet def=\"1\"><{D}:Cell N=\"A\" V=\"{v}\"/></{D}:sheet>");
    assert_eq!(first_value(&format!("{}{}{}", shape_in(&one("first")), shape_in(&one("second")), shape_in(&same("1")))), Some("first".to_owned()));
    // A name that is empty gives none: the unit is read and no `same` can take it.
    counted(&format!("{}{}", shape_in(&sheet("")), shape_in(&same(""))), 2, &[true, false]);
    // A `same` that names a unit of the other kind names no unit here, in a sheet and in a shape.
    counted(&format!("{}<{D}:shape id=\"9\"><{D}:sheet>{}</{D}:sheet></{D}:shape>", shape_in(&sheet("1")), same("1")), 1, &[true, false]);
    counted(&format!("{}{}", shape_in(&section), shape_in(&same("x"))), 1, &[false, false]);
    // What is still damage: a `same` with no `of`.
    let missing = read_of(&shape_in(&format!("<{D}:same/>"))).expect_err("a same that names nothing");
    assert!(missing.contains("names nothing"), "{missing}");
    // A section's `same` in the place of a section that no unit names is counted too, and the sheet has the sections that it does.
    let (page, notes) =
        read_of(&format!("<{D}:shape id=\"1\"><{D}:sheet><{D}:Section N=\"U\"/>{}</{D}:sheet></{D}:shape>", same("9"))).expect("opens");
    assert_eq!((notes, page.shapes.get(0).unwrap().sheet.sections.len()), (vec![DecodeNote::Unknown { count: 1 }], 1));
    // The name of another part names nothing here.
    let second = page_text("", &shape_in(&same("1")));
    let out = decode_page(&parse(&second), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new());
    assert!(out.is_ok(), "the name of another part names nothing here, and the part opens");
}

/// A drawing of one page of three shapes that hold one sheet (a cell and a section), and what the codec writes of it: the page part as a tree, and the document part.
fn shared_page() -> (diacraft_sheetxml::model::Encoded, diacraft_container::xml::El) {
    let mut sheet = Sheet::default();
    sheet.cells.insert(diacraft_doc::CellName::new("A"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
    let mut section = Section::new("U", None);
    section.cells.insert(diacraft_doc::CellName::new("B"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("2")));
    sheet.sections.push(Arc::new(section));
    let sheet = Arc::new(sheet);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let shapes: Vec<Arc<diacraft_doc::Shape>> = (1..=3)
        .map(|id| {
            let mut shape = diacraft_doc::Shape::new(id);
            shape.sheet = Arc::clone(&sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
    let page = encoded.pages[0].1.clone();
    (encoded, page)
}

/// The shapes of a page part as the children that are shapes, with the positions of them in the part.
fn shape_positions(page: &diacraft_container::xml::El) -> Vec<usize> {
    page.children
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n, diacraft_container::xml::Node::El(e) if e.local() == "shape"))
        .map(|(i, _)| i)
        .collect()
}

/// Reads the page part `page` of a drawing whose document part is `encoded`'s: its shapes (whether each has a sheet of cells) and the notes.
fn read_page_part(encoded: &diacraft_sheetxml::model::Encoded, page: &diacraft_container::xml::El) -> (Vec<bool>, Vec<DecodeNote>) {
    let mut notes = Vec::new();
    let doc = decode(&encoded.document, &[page], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).expect("the part opens");
    let shapes = &doc.pages.get(0).unwrap().shapes;
    (shapes.iter().map(|s| !s.sheet.cells.is_empty()).collect(), notes)
}

#[test]
fn what_a_program_of_1_0_does_to_the_page_the_codec_wrote_opens_and_what_is_left_unread_is_counted() {
    use diacraft_container::xml::{El, Node};
    let (encoded, page) = shared_page();
    // As it was written: the first shape names the sheet and the others say so, and the part opens with nothing lost.
    let text = String::from_utf8(diacraft_container::xml::to_bytes(&page, &[], &[], false).unwrap()).unwrap();
    assert!(text.contains(" def=\"1\" fp=\""), "a name and its fingerprint: {text}");
    assert_eq!(read_page_part(&encoded, &page), (vec![true, true, true], vec![]));
    let at = shape_positions(&page);
    assert_eq!(at.len(), 3);
    // The shapes swapped (a change of z-order): the `same` is before the unit it names.
    let mut swapped = page.clone();
    swapped.children.swap(at[0], at[1]);
    assert_eq!(read_page_part(&encoded, &swapped), (vec![false, true, true], vec![DecodeNote::Unknown { count: 1 }]));
    // The shape that holds the name deleted.
    let mut deleted = page.clone();
    deleted.children.remove(at[0]);
    assert_eq!(read_page_part(&encoded, &deleted), (vec![false, false], vec![DecodeNote::Unknown { count: 2 }]));
    // A duplicate of the shape that holds the name, carrying the name the program kept as it kept every attribute it did not know.
    let mut duplicated = page.clone();
    let copy = duplicated.children[at[0]].clone();
    duplicated.children.push(copy);
    // It is read as a unit of no name, so its shape keeps its sheet: the two names it gave again (the sheet's, and the section's inside it) are counted.
    assert_eq!(read_page_part(&encoded, &duplicated), (vec![true, true, true, true], vec![DecodeNote::Unknown { count: 2 }]));
    // The unit edited, with the `same` elements and the name and the fingerprint kept: the fingerprint is not the unit's now, so it names nothing and the unit reads as
    // it is edited; the `same` elements name nothing.
    let mut edited = page.clone();
    fn first_cell(e: &mut El) -> Option<&mut El> {
        for node in &mut e.children {
            if let Node::El(c) = node {
                if c.local() == "Cell" {
                    return Some(c);
                }
                if let Some(found) = first_cell(c) {
                    return Some(found);
                }
            }
        }
        None
    }
    let Node::El(first) = &mut edited.children[at[0]] else { panic!("a shape") };
    let cell = first_cell(first).expect("a cell");
    for (k, v) in &mut cell.attrs {
        if k == "V" {
            *v = "edited".to_owned();
        }
    }
    assert_eq!(read_page_part(&encoded, &edited), (vec![true, false, false], vec![DecodeNote::Unknown { count: 3 }]));
    // A file with no fingerprint reads as it did: the name is the unit's and the `same` elements stand for it as edited.
    let mut unstamped = edited.clone();
    fn strip(e: &mut El) {
        e.attrs.retain(|(k, _)| k != "fp");
        for node in &mut e.children {
            if let Node::El(c) = node {
                strip(c);
            }
        }
    }
    strip(&mut unstamped);
    assert_eq!(read_page_part(&encoded, &unstamped), (vec![true, true, true], vec![]));
}

#[test]
fn a_same_and_a_sheet_in_one_place_are_one_sheet_the_first_and_the_second_is_counted_in_either_order() {
    let sheet = |name: &str, v: &str| format!("<{D}:sheet{name}><{D}:Cell N=\"A\" V=\"{v}\"/></{D}:sheet>");
    let named = shape_in(&sheet(" def=\"1\"", "named"));
    let same = format!("<{D}:same of=\"1\"/>");
    let value_of = |page: &diacraft_doc::Page, at: usize| {
        page.shapes.get(at).unwrap().sheet.cells.get("A").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    // `same` and then `sheet`: the first holds, the second is counted.
    let (page, notes) = read_of(&format!("{named}<{D}:shape id=\"7\">{same}{}</{D}:shape>", sheet("", "second"))).expect("opens");
    assert_eq!((value_of(&page, 1), notes), (Some("named".to_owned()), vec![DecodeNote::Unknown { count: 1 }]));
    // `sheet` and then `same`.
    let (page, notes) = read_of(&format!("{named}<{D}:shape id=\"7\">{}{same}</{D}:shape>", sheet("", "first"))).expect("opens");
    assert_eq!((value_of(&page, 1), notes), (Some("first".to_owned()), vec![DecodeNote::Unknown { count: 1 }]));
    // And in a page, a master, a style and the document, where a sheet stands.
    for (open, close) in [("page", "page"), ("master", "master")] {
        let part = format!("<{D}:{open} xmlns:{D}=\"{URN}\" id=\"1\">{named}{same}{}</{D}:{close}>", sheet("", "later"));
        let el = parse(&part);
        let mut notes = Vec::new();
        let sheet_of_part = if open == "page" {
            decode_page(&el, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).unwrap().sheet
        } else {
            decode_master(&el, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).unwrap().sheet
        };
        assert_eq!(sheet_of_part.cells.get("A").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()), Some("named".to_owned()), "{open}");
        assert_eq!(notes, vec![DecodeNote::Unknown { count: 1 }], "{open}: the second sheet");
    }
    let doc_part = format!(
        "<{D}:document xmlns:{D}=\"{URN}\"><{D}:style id=\"1\">{}</{D}:style><{D}:style id=\"2\">{same}{}</{D}:style>{same}{}</{D}:document>",
        sheet(" def=\"1\"", "named"),
        sheet("", "in the style"),
        sheet("", "later")
    );
    let mut notes = Vec::new();
    let doc = decode(&parse(&doc_part), &[], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut notes).unwrap();
    let text_of = |sheet: &diacraft_doc::Sheet| sheet.cells.get("A").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert_eq!(text_of(&doc.sheet), Some("named".to_owned()), "the document");
    assert_eq!(text_of(&doc.styles.get(1).expect("the second style").sheet), Some("named".to_owned()), "a style");
    assert_eq!(notes, vec![DecodeNote::Unknown { count: 2 }], "the second sheet of the style and of the document");
}

#[test]
fn a_name_costs_the_bytes_of_its_entry_besides_the_attribute_that_gives_it() {
    use diacraft_sheetxml::model::DEF_ENTRY_BYTES;
    use diacraft_sheetxml::model::cost::ATTR_BYTES;
    // The model's bytes of a page of one sheet with a name of `n` bytes and without one: the attribute (its price and its bytes), and the entry (its price and the
    // name's bytes again).
    let spent = |name: &str| {
        let def = if name.is_empty() { String::new() } else { format!(" def=\"{name}\"") };
        let text = format!(
            "<{D}:page xmlns:{D}=\"{URN}\" id=\"1\"><{D}:shape id=\"1\"><{D}:sheet{def}><{D}:Cell N=\"A\"/></{D}:sheet></{D}:shape></{D}:page>"
        );
        let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
        decode_page(&parse(&text), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let (plain, one, six) = (spent(""), spent("1"), spent("123456"));
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`).
    let text = diacraft_sheetxml::model::cost::text_bytes;
    assert_eq!(one - plain, ATTR_BYTES + text(1) + DEF_ENTRY_BYTES + text(1), "the attribute and the entry of a name of one byte");
    assert_eq!(six - one, 2 * (text(6) - text(1)), "each byte of the name is charged for the attribute's value and the entry's name");
}

#[test]
fn a_group_that_holds_shapes_of_a_sheet_that_a_shape_outside_it_holds_says_the_sheet_once() {
    let sheet = {
        let mut sheet = Sheet::default();
        sheet.cells.insert(diacraft_doc::CellName::new("A"), diacraft_doc::Cell::new());
        Arc::new(sheet)
    };
    let shape = |id: u32| {
        let mut shape = diacraft_doc::Shape::new(id);
        shape.sheet = Arc::clone(&sheet);
        shape
    };
    let mut group = shape(1);
    group.sheet = Arc::new(Sheet::default());
    group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape(2))]);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape(3)), Arc::new(group)]);
    doc.pages.push(Arc::new(page));
    let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
    let text = String::from_utf8(diacraft_container::xml::to_bytes(&encoded.pages[0].1, &[], &[], false).unwrap()).unwrap();
    assert_eq!(text.matches(" def=").count(), 1, "{text}");
    assert_eq!(text.matches("<diacraft:same").count(), 1, "the shape inside the group says the sheet that the shape before it named: {text}");
}

/// A shape of the page that holds `unit` in the place of its sheet.
fn shape_in(unit: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    format!("<{D}:shape id=\"{}\">{unit}</{D}:shape>", NEXT.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn a_part_that_names_a_unit_is_never_the_same_as_the_next_part_that_names_one_in_the_same_words() {
    // Two parts that say the same text, with a name and a `same` that stands for it: the second is its own, and its `same` names its own unit and not the first's (the
    // table of units is of the whole decode, and a stream with a name in it is relative to its part).
    let page = |id: u32| {
        format!(
            "<{D}:page xmlns:{D}=\"{URN}\" id=\"{id}\"><{D}:shape id=\"1\"><{D}:sheet><{D}:Cell N=\"P\" V=\"{id}\"/><{D}:Section N=\"U\" def=\"s\"><{D}:Cell N=\"A\"/></{D}:Section></{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:sheet><{D}:Cell N=\"P\" V=\"{id}\"/><{D}:same of=\"s\"/></{D}:sheet></{D}:shape></{D}:page>"
        )
    };
    let document = parse(&format!(
        "<{D}:document xmlns:{D}=\"{URN}\"><{D}:page-ref id=\"1\" part=\"pages/page1.xml\"/><{D}:page-ref id=\"2\" part=\"pages/page2.xml\"/></{D}:document>"
    ));
    let (one, two) = (parse(&page(1)), parse(&page(2)));
    for budget in [Budget::new(HUGE, HUGE, HUGE), Budget::new(HUGE, HUGE, HUGE).without_sharing()] {
        let mut budget = budget;
        let doc = decode(&document, &[&one, &two], &[], &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("both parts name their own");
        for p in 0..2 {
            let page = doc.pages.get(p).unwrap();
            let (a, b) = (page.shapes.get(0).unwrap(), page.shapes.get(1).unwrap());
            assert_eq!(a.sheet.sections.len(), 1);
            assert_eq!(b.sheet.sections.len(), 1);
            assert!(Arc::ptr_eq(&a.sheet.sections[0], &b.sheet.sections[0]), "the `same` is the section that its part named");
        }
    }
    // And the check of a save reads them the same way.
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    check_decodes(&document, &[&one, &two], &[], &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("a check reads what an open reads");
}

/// Two pages of one document, each said as `page`, read as an open reads them and as a check does.
fn two_pages(page: impl Fn(u32) -> String) -> Result<Document, String> {
    let document = parse(&format!(
        "<{D}:document xmlns:{D}=\"{URN}\"><{D}:page-ref id=\"1\" part=\"pages/page1.xml\"/><{D}:page-ref id=\"2\" part=\"pages/page2.xml\"/></{D}:document>"
    ));
    let (one, two) = (parse(&page(1)), parse(&page(2)));
    let doc =
        decode(&document, &[&one, &two], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).map_err(|e| e.to_string())?;
    check_decodes(&document, &[&one, &two], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new())
        .map_err(|e| format!("a check: {e}"))?;
    Ok(doc)
}

#[test]
fn a_unit_that_gives_itself_a_name_in_any_form_is_read_in_each_part_that_has_it_and_is_never_the_unit_of_another_part() {
    // The same text in two parts: the name is given in each, and the `same` that follows names the unit of its own part, whichever way the name is written.
    for (form, name) in [("def", "def=\"1\"".to_owned()), ("def-b64", "def-b64=\"MQ==\"".to_owned())] {
        let page = |id: u32| {
            format!(
                "<{D}:page xmlns:{D}=\"{URN}\" id=\"{id}\"><{D}:shape id=\"1\"><{D}:sheet {name}><{D}:Cell N=\"A\" V=\"x\"/></{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:same of=\"1\"/></{D}:shape></{D}:page>"
            )
        };
        let doc = two_pages(page).unwrap_or_else(|e| panic!("{form}: {e}"));
        for p in 0..2 {
            let page = doc.pages.get(p).unwrap();
            assert!(Arc::ptr_eq(&page.shapes.get(0).unwrap().sheet, &page.shapes.get(1).unwrap().sheet), "{form}, page {p}");
        }
    }
    // The name as a field of text in an element of its own.
    let page = |id: u32| {
        format!(
            "<{D}:page xmlns:{D}=\"{URN}\" id=\"{id}\"><{D}:shape id=\"1\"><{D}:sheet><{D}:s n=\"def\">1</{D}:s><{D}:Cell N=\"A\" V=\"x\"/></{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:same of=\"1\"/></{D}:shape></{D}:page>"
        )
    };
    let doc = two_pages(page).expect("a name that is a field of text");
    let second = doc.pages.get(1).unwrap();
    assert!(Arc::ptr_eq(&second.shapes.get(0).unwrap().sheet, &second.shapes.get(1).unwrap().sheet), "the second part's name was given");
}

#[test]
fn a_unit_that_holds_a_same_is_not_the_unit_of_another_part_that_has_the_same_text() {
    // The sheets of the two parts say the same, and each names the section `s` of its own part, which are different sections: the sheet of the second part has the
    // second's, and not the first's that a hit on the text would give.
    let page = |id: u32| {
        format!(
            "<{D}:page xmlns:{D}=\"{URN}\" id=\"{id}\"><{D}:shape id=\"1\"><{D}:sheet><{D}:Section N=\"U\" def=\"s\"><{D}:Cell N=\"P\" V=\"{id}\"/></{D}:Section></{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:sheet><{D}:Cell N=\"Q\" V=\"1\"/><{D}:same of=\"s\"/></{D}:sheet></{D}:shape></{D}:page>"
        )
    };
    let doc = two_pages(page).expect("both parts");
    for (p, want) in [(0usize, "1"), (1, "2")] {
        let page = doc.pages.get(p).unwrap();
        let section = &page.shapes.get(1).unwrap().sheet.sections[0];
        assert_eq!(section.cells.get("P").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()), Some(want.to_owned()), "page {p}");
    }
}

#[test]
fn units_that_hold_a_field_of_text_in_an_element_of_its_own_are_shared_and_only_a_name_so_given_makes_a_unit_relative() {
    // A long value is written as `s n="V"`; it is a field and no name, and two sheets that hold one are the one sheet.
    let long = "x".repeat(40_000);
    let sheet = |extra: &str| format!("<{D}:sheet><{D}:Cell N=\"A\"><{D}:s n=\"V\">{long}</{D}:s></{D}:Cell>{extra}</{D}:sheet>");
    let page = page_of(&format!("<{D}:shape id=\"1\">{0}</{D}:shape><{D}:shape id=\"2\">{0}</{D}:shape>", sheet("")));
    assert!(Arc::ptr_eq(sheet_of(&page, 0), sheet_of(&page, 1)), "a field of text is no name");
    // An element of another name that has an attribute `n` or the value `def` is no name either.
    let other = |inner: &str| {
        page_of(&format!(
            "<{D}:shape id=\"1\"><{D}:sheet>{inner}</{D}:sheet></{D}:shape><{D}:shape id=\"2\"><{D}:sheet>{inner}</{D}:sheet></{D}:shape>"
        ))
    };
    let with_n = other(&format!("<{D}:Cell N=\"A\" V=\"def\"/><{D}:attr name=\"n\" value=\"def\"/>"));
    assert!(Arc::ptr_eq(sheet_of(&with_n, 0), sheet_of(&with_n, 1)), "a value `def` is no name");
}

#[test]
fn a_sheet_that_one_place_holds_has_no_name_and_a_sheet_that_three_hold_has_one() {
    let mut sheet = Sheet::default();
    sheet.cells.insert(diacraft_doc::CellName::new("A"), diacraft_doc::Cell::new());
    // The sheet one place holds is not equal to the other (a cell of its own): equal sheets are one class, whatever allocations they are.
    let mut single = sheet.clone();
    single.cells.insert(diacraft_doc::CellName::new("B"), diacraft_doc::Cell::new());
    let (single, many) = (Arc::new(single), Arc::new(sheet));
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let mut shapes = Vec::new();
    for (id, s) in [(1u32, &single), (2, &many), (3, &many), (4, &many)] {
        let mut shape = diacraft_doc::Shape::new(id);
        shape.sheet = Arc::clone(s);
        shapes.push(Arc::new(shape));
    }
    page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
    let text = String::from_utf8(diacraft_container::xml::to_bytes(&encoded.pages[0].1, &[], &[], false).unwrap()).unwrap();
    assert_eq!(text.matches(" def=").count(), 1, "only the sheet that several places hold is named: {text}");
    assert_eq!(text.matches("<diacraft:same").count(), 2, "{text}");
    // The two sheets are two units, and the first shape's has no name.
    assert!(text.find("<diacraft:shape id=\"1\"><diacraft:sheet>").is_some(), "{text}");
}

// ------------------------------------------------------------------------------------------------------------------------------- an open and a check spend the same

#[test]
fn a_check_that_keeps_nothing_spends_the_budget_by_the_steps_an_open_does() {
    // The pre-flight of a save is a decode that keeps no unit: it must pass exactly what an open passes, so it spends exactly what an open spends.
    let rows: String = (0..30).map(|i| format!("<{D}:Row N=\"r{i}\"><{D}:Cell N=\"c\" V=\"{i}\"/></{D}:Row>")).collect();
    let big = format!("<{D}:Section N=\"User\">{rows}</{D}:Section>");
    let inner: String = (0..60).map(|i| shape(i, &format!("{}{}{}", pin(i % 4), geometry(), if i % 3 == 0 { big.as_str() } else { "" }))).collect();
    let page = page_of(&inner);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let mut blobs = MemBlobs::new();
    let encoded = encode_for_save(&doc, &mut blobs, &open_limits()).unwrap();
    let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let (mut open, mut check) = (Budget::new(HUGE, HUGE, HUGE), Budget::new(HUGE, HUGE, HUGE));
    let opened = decode(&encoded.document, &pages, &[], &blobs, &mut open, &mut Vec::new()).unwrap();
    check_decodes(&encoded.document, &pages, &[], &blobs, &mut check, &mut Vec::new()).unwrap();
    assert_eq!(opened, doc);
    assert_eq!((open.items, open.bytes, open.blob_bytes), (check.items, check.bytes, check.blob_bytes));
    // And with a budget that is a byte short, both refuse.
    let spent = HUGE - open.bytes;
    let tight = |checking: bool| {
        let mut budget = Budget::new(HUGE, spent - 1, HUGE);
        if checking {
            check_decodes(&encoded.document, &pages, &[], &blobs, &mut budget, &mut Vec::new()).err()
        } else {
            decode(&encoded.document, &pages, &[], &blobs, &mut budget, &mut Vec::new()).err()
        }
    };
    assert!(matches!(tight(true), Some(CodecError::Limit { .. })));
    assert_eq!(tight(true), tight(false));
}

// ------------------------------------------------------------------------------------------------------------------------------- what it costs, measured

const KIND: &str = "SHEETXML_SHARE_KIND";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

/// A page of `n` shapes of a kind: `unique` (a sheet of two cells and a section of a row and a cell, each of its own), `shared` (all the same sheet), `geometry` (a
/// sheet of its own that holds one section shared by all) or `plain` (no sheet).
fn shapes_of(kind: &str, n: usize) -> String {
    let shape_text = |i: usize| match kind {
        "unique" => shape(
            i as u32,
            &format!(
                "<{D}:Cell N=\"a\" V=\"{i}\"/><{D}:Cell N=\"b\" V=\"x\"/><{D}:Section N=\"User\"><{D}:Row N=\"r\"><{D}:Cell N=\"Value\" V=\"{i}\"/></{D}:Row></{D}:Section>"
            ),
        ),
        "shared" => shape(i as u32, &format!("<{D}:Cell N=\"a\" V=\"1\"/><{D}:Cell N=\"b\" V=\"x\"/>")),
        "geometry" => shape(i as u32, &format!("<{D}:Cell N=\"a\" V=\"{i}\"/>{}", geometry())),
        _ => format!("<{D}:shape id=\"{i}\"/>"),
    };
    page_text("", &(0..n).map(shape_text).collect::<String>())
}

#[test]
#[ignore = "run by the test below in a process of its own; alone it needs the kind in the environment"]
fn share_child() {
    let (kind, n) = std::env::var(KIND).unwrap_or_default().split_once(':').map(|(k, n)| (k.to_owned(), n.parse::<usize>().unwrap_or(1000))).unwrap();
    // The code a decode runs is mapped the first time it runs and counts as resident memory: a small decode first, so that what the large one grows is the model.
    let warm = parse(&shapes_of(&kind, n.min(50)));
    decode_page(&warm, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    drop(warm);
    let tree = parse(&shapes_of(&kind, n));
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
    let start = status_kib("VmHWM");
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    let page = decode_page(&tree, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
    let grew = status_kib("VmHWM").saturating_sub(start).saturating_mul(1024);
    println!("SHARE kind={kind} n={n} grew={grew} charged={}", HUGE - budget.bytes);
    std::hint::black_box((&page, &tree));
}

/// The growth of the process by the decode of `n` shapes of `kind`, and what the budget was charged for it.
#[cfg(target_os = "linux")]
fn measured(kind: &str, n: usize) -> (u64, u64) {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "model_share::share_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(KIND, format!("{kind}:{n}"))
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line =
        text.find("SHARE kind=").map(|at| text[at..].lines().next().unwrap_or_default().to_owned()).unwrap_or_else(|| panic!("{kind}: {text}"));
    let field = |key: &str| -> u64 { line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number") };
    (field("grew="), field("charged="))
}

#[test]
#[cfg(target_os = "linux")]
fn what_a_decode_that_shares_grows_the_process_by_is_what_it_charged_for_each_kind_of_sharing() {
    // The charge is what bounds the model: whatever is shared and whatever is not, the process grows by no more than it was charged for (the table of keys of the
    // units that are each their own included), within the noise of the allocator.
    let n = 100_000;
    for kind in ["unique", "shared", "geometry", "plain"] {
        let (grew, charged) = measured(kind, n);
        eprintln!("{kind:<9} {n} shapes: grew {:>6.1} bytes each, charged {:>6.1} bytes each", grew as f64 / n as f64, charged as f64 / n as f64);
        assert!(grew <= charged + charged / 20 + (1 << 20), "{kind}: the process grew by {grew} bytes and the decode was charged {charged}");
    }
    // And what is shared is charged little: a shape of the same sheet costs a shape and a reference, a tenth of one that is its own.
    let (_, unique) = measured("unique", 20_000);
    let (_, shared) = measured("shared", 20_000);
    assert!(shared * 3 < unique, "{shared} bytes charged for shapes that share a sheet, {unique} for shapes that do not");
}

// ------------------------------------------------------------------------------------------------------------------------------- the floor of a model

/// What an open of the parts of `doc` is charged when it shares no more than the file says (a `same` is a pointer, and equal units are not found by their text):
/// the model as the encoder wrote it, each allocation of the drawing once.
fn charged_held(doc: &Document) -> u64 {
    let mut blobs = MemBlobs::new();
    let encoded = encode(doc, &mut blobs).unwrap();
    let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&El> = encoded.masters.iter().map(|(_, e)| e).collect();
    let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
    decode(&encoded.document, &pages, &masters, &blobs, &mut budget, &mut Vec::new()).unwrap();
    HUGE - budget.bytes
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 48, ..proptest::prelude::ProptestConfig::default() })]

    /// The floor is what a save refuses a drawing by before it makes a tree: it must never be more than the open charges of what the save writes (the model's
    /// allocations once each), or a drawing the save could write would be refused.
    #[test]
    fn the_floor_of_a_model_is_never_more_than_what_the_open_charges_of_what_is_written_for_it(doc in diacraft_testkit::model::any_document(diacraft_testkit::model::ModelSize::Small)) {
        let floor = diacraft_sheetxml::model::held_bytes(&doc);
        let charged = charged_held(&doc);
        proptest::prop_assert!(floor <= charged, "a floor of {} against {}", floor, charged);
    }
}

#[test]
fn the_floor_of_a_drawing_of_the_usual_shapes_is_most_of_what_the_open_charges() {
    let doc = super::model_capacity::drawing("typical", 300, 3);
    let (floor, charged) = (diacraft_sheetxml::model::held_bytes(&doc), charged_held(&doc));
    assert!(floor <= charged && floor * 10 >= charged * 5, "a floor of {floor} against {charged}");
    assert_eq!(diacraft_sheetxml::model::held_bytes(&Document::empty()), element_bytes("sheet"), "a document of nothing is its sheet");
}

#[test]
fn the_floor_counts_every_shape_of_a_group_and_every_page_and_master_and_style() {
    use diacraft_doc::{Master, Shape, ShapeList, Style};
    let shape = |id: u32, kids: Vec<Arc<Shape>>| Arc::new(Shape { children: ShapeList::from_vec(kids), ..Shape::new(id) });
    let mut doc = Document::empty();
    let empty = diacraft_sheetxml::model::held_bytes(&doc);
    let sheet = element_bytes("sheet");
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![shape(1, vec![shape(2, vec![shape(3, vec![])]), shape(4, vec![])])]);
    doc.pages.push(Arc::new(page));
    let with_page = diacraft_sheetxml::model::held_bytes(&doc);
    assert_eq!(
        with_page - empty,
        element_bytes("page") + sheet + 4 * (element_bytes("shape") + sheet),
        "a page and four shapes, two of them in groups"
    );
    doc.masters.push(Arc::new(Master { shapes: ShapeList::from_vec(vec![shape(1, vec![])]), ..Master::default() }));
    let with_master = diacraft_sheetxml::model::held_bytes(&doc);
    assert_eq!(with_master - with_page, element_bytes("master") + sheet + element_bytes("shape") + sheet);
    doc.styles.push(Arc::new(Style {
        id: 1,
        name: None,
        name_u: None,
        sheet: Arc::new(Sheet::default()),
        raw: Default::default(),
        passthrough: Default::default(),
        ext: Default::default(),
    }));
    assert_eq!(diacraft_sheetxml::model::held_bytes(&doc) - with_master, element_bytes("style") + sheet);
}

#[test]
fn a_check_under_a_budget_that_shares_nothing_shares_nothing_and_spends_as_a_decode_that_shares_nothing_does() {
    // A decode asked to keep no unit is a check; a budget that says nothing is shared overrides it: the units are built for each shape and charged in full.
    let inner: String = (0..12).map(|i| shape(i, &format!("{}{}", pin(1), geometry()))).collect();
    // A model of units that are each its own (a reader that shares nothing), so that the file says each in full and what the decode finds equal is for it to share.
    let page =
        decode_page(&parse(&page_text("", &inner)), &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE).without_sharing(), &mut Vec::new()).unwrap();
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let mut blobs = MemBlobs::new();
    let encoded = encode(&doc, &mut blobs).unwrap();
    // The page as a writer that says every unit in full wrote it (the encoder shares equal units by value and says them once): the document part is the encoder's.
    let tree = parse_text_page(&inner);
    let pages: Vec<&El> = vec![&tree];
    let spent = |sharing: bool, checking: bool| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        if !sharing {
            budget = budget.without_sharing();
        }
        if checking {
            check_decodes(&encoded.document, &pages, &[], &blobs, &mut budget, &mut Vec::new()).unwrap();
        } else {
            decode(&encoded.document, &pages, &[], &blobs, &mut budget, &mut Vec::new()).unwrap();
        }
        (HUGE - budget.items, HUGE - budget.bytes)
    };
    assert_eq!(spent(false, true), spent(false, false), "nothing shared, checked or opened");
    assert_eq!(spent(true, true), spent(true, false), "shared, checked or opened");
    assert!(
        spent(false, true).1 > 2 * spent(true, true).1,
        "and what is not shared costs more: {} against {}",
        spent(false, true).1,
        spent(true, true).1
    );
}

/// The spec's vectors of 3.19 (`docs/diacraft-format.md`, 8.3): a sheet of one cell in two shapes, written with a name and the fingerprint of the unit, and read as what it
/// says. The text of the page is what the encoder writes, and the spec holds the same fingerprint (a vector that the code and the text do not agree about is a failure).
#[test]
fn the_fingerprint_the_spec_gives_for_its_vector_is_the_one_the_encoder_writes_and_the_reader_checks() {
    use diacraft_doc::{Cell, CellName, Shape, ShapeList, Value};
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("Width"), Cell::constant(Value::from_text("1")));
    let sheet = Arc::new(sheet);
    let shapes: Vec<Arc<Shape>> = (1..=2)
        .map(|id| {
            let mut shape = Shape::new(id);
            shape.sheet = Arc::clone(&sheet);
            Arc::new(shape)
        })
        .collect();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(shapes);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let encoded = diacraft_sheetxml::model::encode(&doc, &mut diacraft_sheetxml::model::MemBlobs::new()).expect("encodes");
    let text = String::from_utf8(diacraft_container::xml::to_bytes(&encoded.pages[0].1, &[], &[], false).expect("writes")).expect("utf-8");
    let at = text.find("fp=\"").expect("a fingerprint is written") + 4;
    let fp = &text[at..at + 16];
    println!("VECTOR page: {text}");
    let spec = include_str!("../../../../docs/diacraft-format.md");
    assert!(spec.contains(&format!("fp=\"{fp}\"")), "the spec's vector gives another fingerprint than the encoder's {fp}: {text}");
    // A reader that finds the same text accepts the name, and one that finds the unit edited does not.
    let ok = read_of(&inner_of(&text)).expect("opens");
    assert_eq!((ok.1, ok.0.shapes.len()), (vec![], 2));
    let edited = inner_of(&text).replace("V=\"1\"", "V=\"2\"");
    let (page, notes) = read_of(&edited).expect("opens");
    assert_eq!(
        notes,
        vec![DecodeNote::Unknown { count: 2 }],
        "the edited unit no longer has its name (counted), and the `same` that stood for it is counted"
    );
    let widths: Vec<bool> = page.shapes.iter().map(|s| s.sheet.cells.contains_key(&diacraft_doc::CellName::new("Width"))).collect();
    assert_eq!(widths, vec![true, false], "the unit is read as it is, and the shape that stood for it by a `same` has no sheet");
}

/// What is inside the root element of a part that the encoder wrote: the shapes.
fn inner_of(part: &str) -> String {
    let start = part.find('>').expect("the root's tag") + 1;
    let end = part.rfind("</").expect("the root's end");
    part[start..end].to_owned()
}
