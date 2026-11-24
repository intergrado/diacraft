//! `state.xml`: the identifier ledger and the content stamp (the public spec's 2.7).
use std::sync::Arc;

use diacraft_doc::{Page, Shape, ShapeList};
use diacraft_native::{Ledger, State, WriteOptions, write};
use diacraft_sheetxml::model::Stamp;

use crate::util::*;

/// Vector 2 with `state` for its `state.xml` part, or none.
fn vector_2_with_state(state: Option<Vec<u8>>) -> Vec<u8> {
    let mut items = vector_2();
    match state {
        Some(state) => replace(&mut items, "state.xml", state),
        None => {
            remove(&mut items, "state.xml");
            replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", "")]));
        }
    }
    package(&items)
}

/// Vector 2's page with its rectangle of the shape ID `id`.
fn vector_with_shape(id: u32, state: Vec<u8>) -> Vec<u8> {
    let mut items = vector_2();
    replace(&mut items, "pages/page1.xml", page_1(&rectangle(&format!("<diacraft:shape id=\"{id}\" name=\"Rectangle\" type=\"Shape\">"), "", "")));
    replace(&mut items, "state.xml", state);
    package(&items)
}

fn marks(bytes: &[u8]) -> Vec<(u32, u32)> {
    open_ok(bytes).1.state.ledger.0.into_iter().collect()
}

#[test]
fn a_ledger_at_u32_max_opens() {
    let bytes = vector_2_with_state(Some(state_with(None, &[("1", "4294967295")])));
    assert_eq!(marks(&bytes), [(1, u32::MAX)]);
    // It is written back as it is: a mark is a mark, and the first ID above it does not exist (the engine says so; the file does not).
    let (doc, opened) = open_ok(&bytes);
    let written = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap().bytes;
    assert!(String::from_utf8(entry(&written, "state.xml")).unwrap().contains("<diacraft:ledger page=\"1\" mark=\"4294967295\"/>"));
    // One more is not a `u32`.
    let bytes = vector_2_with_state(Some(state_with(None, &[("1", "4294967296")])));
    assert_eq!(refused(&bytes).class(), "state_malformed");
}

#[test]
fn a_hostile_ledger_only_raises_marks() {
    // A mark of 3 on a page holding shape 9 reads as 9; a mark above the highest ID stays.
    assert_eq!(marks(&vector_with_shape(9, state_with(None, &[("1", "3")]))), [(1, 9)]);
    assert_eq!(marks(&vector_with_shape(9, state_with(None, &[("1", "12")]))), [(1, 12)]);
    assert_eq!(marks(&vector_with_shape(9, state_with(None, &[("1", "0")]))), [(1, 9)]);
    // More than one row for a page: the highest. A row for a page the drawing does not have is ignored.
    assert_eq!(marks(&vector_with_shape(2, state_with(None, &[("1", "5"), ("1", "20"), ("1", "7"), ("77", "100")]))), [(1, 20)]);
    // No row at all: the page's highest ID is its mark (a ledger raises and never lowers).
    assert_eq!(marks(&vector_with_shape(9, state_with(None, &[]))), [(1, 9)]);
    // A shape nested in a group counts: the walk goes down every group.
    let mut group = Shape::new(5);
    group.children = ShapeList::from_vec(vec![Arc::new(Shape::new(31))]);
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(group)]);
    let doc = drawing_of(vec![page]);
    let bytes = write(&doc, &State { ledger: Ledger([(1, 8)].into()), content_stamp: None }, None, &WriteOptions::default()).unwrap().bytes;
    assert_eq!(marks(&bytes), [(1, 31)]);
}

#[test]
fn a_writer_writes_one_row_for_each_page_with_a_mark_and_none_for_a_page_the_drawing_lacks() {
    let doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two"), Page::new(3, "Three")]);
    let state = State { ledger: Ledger([(1, 4), (3, 0), (99, 7), (2, 11)].into()), content_stamp: None };
    let bytes = write(&doc, &state, None, &WriteOptions::default()).unwrap().bytes;
    let text = String::from_utf8(entry(&bytes, "state.xml")).unwrap();
    assert_eq!(
        text,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<diacraft:state xmlns:diacraft=\"{NS}\"><diacraft:ledger page=\"1\" mark=\"4\"/><diacraft:ledger page=\"2\" mark=\"11\"/></diacraft:state>"
        )
    );
    // And what a drawing with nothing to say writes is the empty element of Vector 1.
    let blank = write_doc(&diacraft_doc::Document::new_blank());
    assert_eq!(entry(&blank, "state.xml"), state_empty());
}

#[test]
fn the_content_stamp_comes_back_and_a_stamp_that_is_not_one_is_damage() {
    let stamp = Stamp(0x00eb_c460_d282_d097);
    let doc = drawing_of(vec![Page::new(1, "One")]);
    let state = State { ledger: Ledger::default(), content_stamp: Some(stamp) };
    let bytes = write(&doc, &state, None, &WriteOptions::default()).unwrap().bytes;
    assert!(String::from_utf8(entry(&bytes, "state.xml")).unwrap().contains("content-stamp=\"00ebc460d282d097\""));
    assert_eq!(open_ok(&bytes).1.state, state);
    for bad in ["00EBC460D282D097", "00ebc460d282d09", "00ebc460d282d0970", "00ebc460d282d09g", ""] {
        let mut items = vector_2();
        replace(&mut items, "state.xml", state_with(Some(bad), &[]));
        assert_eq!(refused(&package(&items)).class(), "state_malformed", "{bad:?}");
    }
}

#[test]
fn a_state_that_is_not_what_the_format_says_is_damage() {
    for ledger in [("", "1"), ("1", ""), ("x", "1"), ("1", "-1"), ("1", "1.5"), ("1", "+2"), ("4294967296", "1"), (" 1", "1")] {
        let bytes = vector_2_with_state(Some(state_with(None, &[ledger])));
        assert_eq!(refused(&bytes).class(), "state_malformed", "{ledger:?}");
    }
    // A row with no `page` or no `mark`.
    for row in ["<diacraft:ledger mark=\"1\"/>", "<diacraft:ledger page=\"1\"/>", "<diacraft:ledger/>"] {
        let bytes = vector_2_with_state(Some(part(&format!("<diacraft:state xmlns:diacraft=\"{NS}\">{row}</diacraft:state>"))));
        assert_eq!(refused(&bytes).class(), "state_malformed", "{row}");
    }
    // A root that is not `state`, or not in the namespace.
    for root in [format!("<diacraft:manifest xmlns:diacraft=\"{NS}\"/>"), "<state/>".to_owned(), "<state xmlns=\"urn:other\"/>".to_owned()] {
        assert_eq!(refused(&vector_2_with_state(Some(part(&root)))).class(), "state_malformed", "{root}");
    }
}

#[test]
fn a_reader_ignores_what_it_does_not_know_of_the_state() {
    let text = part(&format!(
        "<diacraft:state xmlns:diacraft=\"{NS}\" future=\"1\"><diacraft:ledger page=\"1\" mark=\"3\" note=\"x\"/><diacraft:something/>text<other xmlns=\"urn:z\"><ledger page=\"x\" mark=\"y\"/></other></diacraft:state>"
    ));
    assert_eq!(marks(&vector_2_with_state(Some(text))), [(1, 3)]);
}

#[test]
fn no_state_part_is_an_empty_state() {
    // A reader that finds none uses an empty state: no content stamp, and no mark but the highest shape ID of each page.
    let (_, opened) = open_ok(&vector_2_with_state(None));
    assert_eq!(opened.state.content_stamp, None);
    assert_eq!(opened.state.ledger.0.into_iter().collect::<Vec<_>>(), [(1, 1)]);
    let mut items = vector_1();
    remove(&mut items, "state.xml");
    replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", "")]));
    assert_eq!(open_ok(&package(&items)).1.state, State::default());
}
