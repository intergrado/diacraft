//! Bullets, indents, text direction and Change Case: `text.format` writes the Paragraph section's `Bullet`, `IndLeft`, `IndFirst`
//! and `Flags`, the Character section's `Case` and `Style`, and the shape's `TextDirection`.
//!
//! **What each writes**:
//!
//! * a bullet is `Bullet` 1 to 7 in the paragraph's row, and a paragraph with no indent at all is given the hanging form a bulleted paragraph has in
//!   files (`IndLeft` 0.25 and `IndFirst` -0.25 inch), which a bullet taken off takes away again;
//! * an indent step moves `IndLeft` to the next multiple of a quarter inch, up or down, and never below 0;
//! * Change Case is a display attribute: `Case` 0, 1 or 2, and small capitals are bit 8 of `Style`; the stored text is never rewritten;
//! * text direction is the paragraph's `Flags` (0 left to right, 1 right to left) and the block's `TextDirection` (0 across, 1 down).

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Document, Owner, RowKey, SectionKey, TextPart};
use diacraft_engine::formatting::{Case, Direction, Indent, TextFormat, format_text};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::format::{fv, local};

fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

/// A document of one page and shape 1 with this text, and a root style as a file holds one.
fn doc_with(parts: Vec<TextPart>) -> Document {
    DocBuilder::new()
        .root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Color", "0").row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts).row_cell("Character", None, RowKey::Ix(0), "Style", "0")))
        .build()
}

fn words(doc: &Document) -> Vec<String> {
    let Some(text) = doc.page(1).unwrap().shapes.get(0).unwrap().text.clone() else { return vec!["(no text)".to_owned()] };
    text.parts
        .iter()
        .map(|part| match part {
            TextPart::Chars(s) => format!("{s:?}"),
            TextPart::Cp(ix) => format!("cp{ix}"),
            TextPart::Pp(ix) => format!("pp{ix}"),
            TextPart::Tp(ix) => format!("tp{ix}"),
            TextPart::Fld { ix, text } => format!("fld{ix}={text}"),
            TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => "raw".to_owned(),
        })
        .collect()
}

fn row(doc: &Document, section: &str, ix: u32) -> BTreeMap<String, (Option<String>, String)> {
    let shape = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let Some(section) = shape.sheet.section(&SectionKey::new(section, None)) else { return BTreeMap::new() };
    let Some(row) = section.row(&RowKey::Ix(ix)) else { return BTreeMap::new() };
    row.cells.iter().map(|(name, cell)| (name.as_str().to_owned(), fv(cell))).collect()
}

fn value(doc: &Document, section: &str, ix: u32, name: &str) -> Option<String> {
    row(doc, section, ix).get(name).map(|(_, v)| v.clone())
}

fn number(doc: &Document, section: &str, ix: u32, name: &str) -> Option<f64> {
    value(doc, section, ix, name).map(|v| v.parse().unwrap())
}

/// Sets a cell of a local row of shape 1 (a fixture edit).
fn set_cell(doc: &mut Document, section: &str, ix: u32, name: &str, value: &str) {
    let at = doc.find_shape(Owner::Page(1), 1).unwrap();
    let addr = diacraft_doc::CellAddr::in_row(section, None, RowKey::Ix(ix), name);
    doc.write_cell(
        &diacraft_doc::SheetRef::Shape(at),
        &addr,
        diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text(value)),
        diacraft_doc::WriteOrigin::Drawing,
    )
    .unwrap();
}

fn format(doc: &mut Document, range: Option<(usize, usize)>, f: &TextFormat) -> usize {
    format_text(doc, 1, &[1], range, f).unwrap_or_else(|p| panic!("{range:?}: {}", p.message))
}

fn bullet(n: u8) -> TextFormat {
    TextFormat { bullet: Some(n), ..TextFormat::default() }
}

fn indent(step: Indent) -> TextFormat {
    TextFormat { indent: Some(step), ..TextFormat::default() }
}

fn case(case: Case) -> TextFormat {
    TextFormat { case: Some(case), ..TextFormat::default() }
}

// ---- bullets ------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_bullet_is_the_paragraphs_bullet_cell_and_a_hanging_indent() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one\ntwo")]);
    assert_eq!(format(&mut doc, None, &bullet(1)), 1);
    assert_eq!(value(&doc, "Paragraph", 0, "Bullet").as_deref(), Some("1"));
    // A paragraph with no indent is given the form files hold for a bulleted one: the text of every line at a quarter
    // inch, the bullet at the margin.
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.25));
    assert_eq!(number(&doc, "Paragraph", 0, "IndFirst"), Some(-0.25));
    assert_eq!(words(&doc), ["cp0", "\"one\\ntwo\""], "the text is not touched, and no marker is added to a text that has none");
}

#[test]
fn every_style_of_the_table_is_a_bullet_and_none_takes_the_bullet_off() {
    for n in 1..=7u8 {
        let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
        format(&mut doc, None, &bullet(n));
        assert_eq!(value(&doc, "Paragraph", 0, "Bullet"), Some(n.to_string()), "style {n}");
    }
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    format(&mut doc, None, &bullet(3));
    assert_eq!(format(&mut doc, None, &bullet(0)), 1);
    assert_eq!(value(&doc, "Paragraph", 0, "Bullet").as_deref(), Some("0"));
    // The hanging form the bullet gave goes with it.
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.0));
    assert_eq!(number(&doc, "Paragraph", 0, "IndFirst"), Some(0.0));
}

#[test]
fn a_paragraph_that_has_an_indent_keeps_it_when_it_gets_a_bullet_and_when_it_loses_it() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    format(&mut doc, None, &indent(Indent::Increase));
    format(&mut doc, None, &indent(Indent::Increase));
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.5));
    format(&mut doc, None, &bullet(1));
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.5), "an indent of its own is the person's");
    assert_eq!(number(&doc, "Paragraph", 0, "IndFirst"), None, "no hanging form is written over it");
    format(&mut doc, None, &bullet(0));
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.5));
}

#[test]
fn a_bullet_on_a_range_is_on_the_paragraphs_it_touches_and_no_others() {
    // `w` is in the second paragraph: the paragraphs split at their line breaks as an alignment does.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one\ntwo\nthree")]);
    assert_eq!(format(&mut doc, Some((5, 6)), &bullet(1)), 1);
    assert_eq!(words(&doc), ["cp0", "pp0", "\"one\\n\"", "pp1", "\"two\\n\"", "pp2", "\"three\""]);
    assert_eq!(value(&doc, "Paragraph", 1, "Bullet").as_deref(), Some("1"));
    assert_ne!(value(&doc, "Paragraph", 0, "Bullet").as_deref(), Some("1"), "the first paragraph has no bullet");
    assert_ne!(value(&doc, "Paragraph", 2, "Bullet").as_deref(), Some("1"), "nor the last");
    assert_eq!(number(&doc, "Paragraph", 1, "IndLeft"), Some(0.25));
    assert_ne!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.25));
}

#[test]
fn a_bullet_on_every_paragraph_of_a_text_with_markers_changes_the_rows_they_name() {
    let mut doc = doc_with(vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    format(&mut doc, None, &bullet(2));
    assert_eq!(words(&doc), ["cp0", "pp0", "\"one\\n\"", "pp1", "\"two\""], "no marker is added");
    assert_eq!(value(&doc, "Paragraph", 0, "Bullet").as_deref(), Some("2"));
    assert_eq!(value(&doc, "Paragraph", 1, "Bullet").as_deref(), Some("2"));
}

#[test]
fn a_bullet_that_is_there_already_writes_nothing() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    format(&mut doc, None, &bullet(4));
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &bullet(4)), 0);
    assert_eq!(doc, before);
    let mut plain = doc_with(vec![TextPart::Cp(0), chars("one")]);
    let before = plain.clone();
    assert_eq!(format(&mut plain, None, &bullet(0)), 0, "none on a paragraph that has none");
    assert_eq!(plain, before);
}

#[test]
fn a_bullet_beyond_the_table_is_refused_in_words() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    let before = doc.clone();
    let err = format_text(&mut doc, 1, &[1], None, &bullet(8)).unwrap_err();
    assert!(err.message.contains("`bullet` must be from 0 to 7"), "{}", err.message);
    assert_eq!(doc, before);
}

// ---- indents ------------------------------------------------------------------------------------------------------------------------

#[test]
fn an_indent_step_is_a_quarter_inch_to_the_next_multiple_and_never_below_nothing() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    let at = |doc: &Document| number(doc, "Paragraph", 0, "IndLeft");
    format(&mut doc, None, &indent(Indent::Increase));
    assert_eq!(at(&doc), Some(0.25));
    format(&mut doc, None, &indent(Indent::Increase));
    assert_eq!(at(&doc), Some(0.5));
    format(&mut doc, None, &indent(Indent::Decrease));
    assert_eq!(at(&doc), Some(0.25));
    format(&mut doc, None, &indent(Indent::Decrease));
    assert_eq!(at(&doc), Some(0.0));
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &indent(Indent::Decrease)), 0, "no indent left to take off");
    assert_eq!(doc, before);
}

#[test]
fn an_indent_that_is_not_a_multiple_goes_to_the_next_one() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    for (start, up, down) in [("0.3", 0.5, 0.25), ("0.1", 0.25, 0.0), ("1.99", 2.0, 1.75)] {
        set_cell(&mut doc, "Paragraph", 0, "IndLeft", start);
        let mut up_doc = doc.clone();
        format(&mut up_doc, None, &indent(Indent::Increase));
        assert_eq!(number(&up_doc, "Paragraph", 0, "IndLeft"), Some(up), "{start} up");
        format(&mut doc, None, &indent(Indent::Decrease));
        assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(down), "{start} down");
    }
}

#[test]
fn an_indent_is_the_paragraphs_own_and_each_moves_from_where_it_is() {
    let mut doc = doc_with(vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    set_cell(&mut doc, "Paragraph", 1, "IndLeft", "0.5");
    format(&mut doc, None, &indent(Indent::Increase));
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.25));
    assert_eq!(number(&doc, "Paragraph", 1, "IndLeft"), Some(0.75));
}

#[test]
fn an_indent_stops_at_a_most() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    set_cell(&mut doc, "Paragraph", 0, "IndLeft", "100");
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &indent(Indent::Increase)), 0);
    assert_eq!(doc, before);
}

#[test]
fn the_indent_of_a_bulleted_paragraph_moves_the_bullet_with_it() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    format(&mut doc, None, &bullet(1));
    format(&mut doc, None, &indent(Indent::Increase));
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.5));
    assert_eq!(number(&doc, "Paragraph", 0, "IndFirst"), Some(-0.25), "the first line keeps its hang");
}

// ---- Change Case --------------------------------------------------------------------------------------------------------------------

#[test]
fn change_case_is_a_display_attribute_and_the_text_is_never_rewritten() {
    // Given `Case` 1 and stored `abc`, expect capitals drawn and `abc` kept in the saved text. Rewriting the text fails.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("Hello World")]);
    assert_eq!(format(&mut doc, None, &case(Case::AllCaps)), 1);
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("1"));
    assert_eq!(words(&doc), ["cp0", "\"Hello World\""]);
    format(&mut doc, None, &case(Case::InitialCaps));
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("2"));
    format(&mut doc, None, &case(Case::Normal));
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("0"));
    assert_eq!(words(&doc), ["cp0", "\"Hello World\""]);
}

#[test]
fn small_capitals_are_the_eight_bit_of_style_and_the_other_cases_clear_it() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("Hello")]);
    set_cell(&mut doc, "Character", 0, "Style", "17");
    format(&mut doc, None, &case(Case::SmallCaps));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("25"), "bold and complex bold kept, bit 8 set");
    assert_ne!(value(&doc, "Character", 0, "Case").as_deref(), Some("1"), "small capitals are no `Case`");
    format(&mut doc, None, &case(Case::AllCaps));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("17"), "one case at a time, as the dialog's Case box has");
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("1"));
    format(&mut doc, None, &case(Case::SmallCaps));
    format(&mut doc, None, &case(Case::Normal));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("17"));
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("0"));
}

#[test]
fn a_case_on_a_range_splits_the_runs_it_touches() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello world")]);
    format(&mut doc, Some((6, 11)), &case(Case::AllCaps));
    assert_eq!(words(&doc), ["cp0", "\"hello \"", "cp1", "\"world\""]);
    assert_eq!(value(&doc, "Character", 1, "Case").as_deref(), Some("1"));
    assert_ne!(value(&doc, "Character", 0, "Case").as_deref(), Some("1"));
}

#[test]
fn a_case_that_is_there_already_writes_nothing() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    format(&mut doc, None, &case(Case::InitialCaps));
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &case(Case::InitialCaps)), 0);
    assert_eq!(doc, before);
}

// ---- text direction -----------------------------------------------------------------------------------------------------------------

#[test]
fn vertical_text_is_the_blocks_text_direction() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    assert_eq!(format(&mut doc, None, &TextFormat { vertical: Some(true), ..TextFormat::default() }), 1);
    assert_eq!(local(&doc, 1, "TextDirection").map(|c| fv(&c).1).as_deref(), Some("1"));
    assert_eq!(format(&mut doc, None, &TextFormat { vertical: Some(false), ..TextFormat::default() }), 1);
    assert_eq!(local(&doc, 1, "TextDirection").map(|c| fv(&c).1).as_deref(), Some("0"));
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &TextFormat { vertical: Some(false), ..TextFormat::default() }), 0);
    assert_eq!(doc, before);
}

#[test]
fn a_reading_direction_is_the_paragraphs_flags() {
    let mut doc = doc_with(vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    format(&mut doc, None, &TextFormat { direction: Some(Direction::RightToLeft), ..TextFormat::default() });
    assert_eq!(value(&doc, "Paragraph", 0, "Flags").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Paragraph", 1, "Flags").as_deref(), Some("1"));
    format(&mut doc, None, &TextFormat { direction: Some(Direction::LeftToRight), ..TextFormat::default() });
    assert_eq!(value(&doc, "Paragraph", 0, "Flags").as_deref(), Some("0"));
    // A range takes the paragraphs it touches.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one\ntwo\nthree")]);
    format(&mut doc, Some((5, 6)), &TextFormat { direction: Some(Direction::RightToLeft), ..TextFormat::default() });
    assert_eq!(value(&doc, "Paragraph", 1, "Flags").as_deref(), Some("1"));
    assert_ne!(value(&doc, "Paragraph", 0, "Flags").as_deref(), Some("1"));
}

#[test]
fn a_shape_with_no_text_is_formatted_for_the_text_typed_next() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    assert_eq!(format(&mut doc, None, &bullet(1)), 1);
    assert_eq!(value(&doc, "Paragraph", 0, "Bullet").as_deref(), Some("1"));
    assert_eq!(format(&mut doc, None, &case(Case::AllCaps)), 1);
    assert_eq!(value(&doc, "Character", 0, "Case").as_deref(), Some("1"));
    assert_eq!(words(&doc), ["(no text)"]);
}

#[test]
fn a_bullet_and_an_indent_step_in_one_call_make_half_an_inch() {
    // The step is taken from the indent the bullet gave in the same call, not from what the row held before it.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one")]);
    format(&mut doc, None, &TextFormat { bullet: Some(1), indent: Some(Indent::Increase), ..TextFormat::default() });
    assert_eq!(number(&doc, "Paragraph", 0, "IndLeft"), Some(0.5));
    assert_eq!(number(&doc, "Paragraph", 0, "IndFirst"), Some(-0.25));
}
