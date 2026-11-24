//! An edit of a shape's text keeps the ODF data the text holds for its paragraphs (`Text.ext`).
//!
//! The data is **the file's paragraphs, in the file's order**: what each `text:p` named, held beside the characters it had (`OdfParagraph`). An edit of one
//! paragraph, or a newline typed before another, leaves that list as the file made it, since the splice writer keeps an untouched paragraph's style and bytes
//! (the native design §3.3), and matches the file's paragraphs to the text's by content when it saves
//! (`crates/odg/src/write/runs.rs`, `plan`: a test of the `odg` crate holds that). Each of the three edits of a text carries the slot forward: before this fix each
//! built `Text { parts, ..Default::default() }` and every held style was gone at the first keystroke.

use std::sync::Arc;

use diacraft_doc::{Document, Ext, OdfParagraph, OdfText, OdfTextStyle, RowKey, SectionKey, Stamp, TextPart};
use diacraft_engine::draw::{set_text, set_text_range};
use diacraft_engine::formatting::{TextFormat, format_text};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

/// What the file held of a paragraph: the style it named and the stamp of its characters (the writer's plan compares the characters themselves).
fn paragraph(name: &str, stamp: u64) -> OdfParagraph {
    OdfParagraph {
        style: OdfTextStyle { name: Some(Arc::from(name)), ..OdfTextStyle::default() },
        characters: Some(Stamp(stamp)),
        ..OdfParagraph::default()
    }
}

/// The two paragraphs of `one\ntwo` as the file held them.
fn held() -> OdfText {
    OdfText { paragraphs: vec![paragraph("P1", 1), paragraph("P2", 2)], ..OdfText::default() }
}

/// Shape 1 of page 1, whose text is `chars` in one run, and holds `held()` for its paragraphs.
fn doc_with_held(chars: &str) -> Document {
    let parts = vec![TextPart::Cp(0), TextPart::Chars(Arc::from(chars))];
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts))).build();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(shape.text.as_mut().unwrap()).ext.make_mut().odf = held();
    doc
}

fn text_of(doc: &Document) -> Option<Arc<diacraft_doc::Text>> {
    doc.page(1).unwrap().shapes.get(0).unwrap().text.clone()
}

fn ext_of(doc: &Document) -> Ext<OdfText> {
    text_of(doc).map(|t| t.ext.clone()).unwrap_or_default()
}

/// The characters of shape 1's text.
fn chars_of(doc: &Document) -> String {
    text_of(doc).map_or_else(String::new, |t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect())
}

fn names(ext: &Ext<OdfText>) -> Vec<Option<String>> {
    ext.get().map(|d| d.odf.paragraphs.iter().map(|p| p.style.name.as_deref().map(str::to_owned)).collect()).unwrap_or_default()
}

/// The parts of shape 1's text as short words: `cp1`, and the characters in quotes.
fn words(doc: &Document) -> Vec<String> {
    text_of(doc)
        .map(|t| {
            t.parts
                .iter()
                .map(|part| match part {
                    TextPart::Chars(s) => format!("{s:?}"),
                    TextPart::Cp(ix) => format!("cp{ix}"),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `Style` cell of shape 1's `Character` row `ix`, as text.
fn style_of_run(doc: &Document, ix: u32) -> Option<String> {
    let shape = doc.page(1).unwrap().shapes.get(0).unwrap().clone();
    let section = shape.sheet.section(&SectionKey::new("Character", None))?.clone();
    let row = section.row(&RowKey::Ix(ix))?.clone();
    row.cells.get("Style").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

fn named(list: &[&str]) -> Vec<Option<String>> {
    list.iter().map(|n| Some((*n).to_owned())).collect()
}

#[test]
fn bold_on_the_second_paragraph_leaves_the_first_paragraphs_held_style() {
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    assert_eq!(names(&before), named(&["P1", "P2"]));
    let bold = TextFormat { bold: Some(true), ..TextFormat::default() };
    assert_eq!(format_text(&mut doc, 1, &[1], Some((4, 7)), &bold).unwrap(), 1);
    // The run was split by the format (`two` is a run of its own, row 1, and bold), so the text changed; the file's paragraphs are as the file made them.
    assert_eq!(words(&doc), ["cp0", "\"one\\n\"", "cp1", "\"two\""]);
    assert_eq!(style_of_run(&doc, 1).as_deref(), Some("1"), "the format took effect");
    assert_eq!(chars_of(&doc), "one\ntwo");
    assert_eq!(ext_of(&doc), before, "the slot goes with the text");
    assert_eq!(names(&ext_of(&doc)), named(&["P1", "P2"]));
}

#[test]
fn a_format_of_the_whole_text_keeps_the_slot_too() {
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    let bold = TextFormat { bold: Some(true), ..TextFormat::default() };
    assert_eq!(format_text(&mut doc, 1, &[1], None, &bold).unwrap(), 1);
    assert_eq!(style_of_run(&doc, 0).as_deref(), Some("1"), "the format took effect");
    assert_eq!(ext_of(&doc), before);
}

#[test]
fn a_range_of_one_paragraph_replaced_keeps_the_others_held_styles() {
    // `two` typed over to give `2`: the second paragraph is edited, and the first is as it was.
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    assert!(set_text_range(&mut doc, 1, 1, 4, 7, "2").unwrap());
    assert_eq!(chars_of(&doc), "one\n2");
    assert_eq!(ext_of(&doc), before);
    // And the first, `one` to `1`.
    let mut doc = doc_with_held("one\ntwo");
    assert!(set_text_range(&mut doc, 1, 1, 0, 3, "1").unwrap());
    assert_eq!(chars_of(&doc), "1\ntwo");
    assert_eq!(ext_of(&doc), before);
}

#[test]
fn a_newline_typed_before_a_held_paragraph_leaves_the_file_s_list_as_it_was() {
    // `one\n\ntwo`: the model has a paragraph the file lacks between the two. The held list is the file's, in the file's order, so it is not shifted onto the new
    // paragraph (`P2` is `two`'s, not the empty one's): the writer's plan matches by content.
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    assert!(set_text_range(&mut doc, 1, 1, 4, 4, "\n").unwrap());
    assert_eq!(chars_of(&doc), "one\n\ntwo");
    assert_eq!(ext_of(&doc), before);
    assert_eq!(names(&ext_of(&doc)), named(&["P1", "P2"]));
    // A newline at the very start, and at the end.
    for (at, expected) in [(0, "\none\ntwo"), (7, "one\ntwo\n")] {
        let mut doc = doc_with_held("one\ntwo");
        assert!(set_text_range(&mut doc, 1, 1, at, at, "\n").unwrap());
        assert_eq!(chars_of(&doc), expected);
        assert_eq!(ext_of(&doc), before, "newline at {at}");
    }
    // A newline taken out: the file's paragraphs are still the file's.
    let mut doc = doc_with_held("one\ntwo");
    assert!(set_text_range(&mut doc, 1, 1, 3, 4, "").unwrap());
    assert_eq!(chars_of(&doc), "onetwo");
    assert_eq!(ext_of(&doc), before);
}

#[test]
fn the_whole_text_set_again_keeps_the_slot_for_the_plan_to_match_by_content() {
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    assert!(set_text(&mut doc, 1, 1, "one\nTWO").unwrap());
    assert_eq!(chars_of(&doc), "one\nTWO");
    assert_eq!(ext_of(&doc), before);
}

#[test]
fn a_text_made_empty_holds_nothing_and_a_text_that_held_nothing_still_holds_nothing() {
    // No text is no `Text` and so no slot (the splice writer then takes the paragraphs out).
    let mut doc = doc_with_held("one\ntwo");
    assert!(set_text(&mut doc, 1, 1, "").unwrap());
    assert!(text_of(&doc).is_none());
    // All the characters taken out of a range leave the markers (`edit_text.rs`: the next characters typed are the first run's), so the text stays a text, and
    // with it the slot: the file's paragraphs as it held them, which the writer finds nothing of in the one empty paragraph left and takes out.
    let mut doc = doc_with_held("one\ntwo");
    let before = ext_of(&doc);
    assert!(set_text_range(&mut doc, 1, 1, 0, 7, "").unwrap());
    assert_eq!(chars_of(&doc), "");
    assert_eq!(ext_of(&doc), before);
    // A text with no slot gets none from an edit.
    let parts = vec![TextPart::Cp(0), TextPart::Chars(Arc::from("one\ntwo"))];
    let plain = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts))).build();
    let mut doc = plain.clone();
    assert!(set_text_range(&mut doc, 1, 1, 0, 0, "x").unwrap());
    assert!(ext_of(&doc).is_empty());
    let mut doc = plain.clone();
    let bold = TextFormat { bold: Some(true), ..TextFormat::default() };
    format_text(&mut doc, 1, &[1], Some((0, 3)), &bold).unwrap();
    assert!(ext_of(&doc).is_empty());
    // A shape with no text: typing makes a new text, which holds nothing.
    let bare = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    let mut doc = bare.clone();
    assert!(set_text(&mut doc, 1, 1, "new").unwrap());
    assert!(ext_of(&doc).is_empty());
}

#[test]
fn an_edit_that_changes_nothing_changes_nothing() {
    let mut doc = doc_with_held("one\ntwo");
    let before = doc.clone();
    assert!(!set_text(&mut doc, 1, 1, "one\ntwo").unwrap());
    assert!(!set_text_range(&mut doc, 1, 1, 2, 2, "").unwrap());
    assert_eq!(doc, before);
}
