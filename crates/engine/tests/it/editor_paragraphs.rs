//! What the text box reads of a shape's paragraphs while its text is edited: the direction, the alignment and the two side indents of each paragraph of
//! the text as it will be once the edit is sent, each paragraph taking the format of the run that holds its last character and the characters typed
//! joining the run the commit gives them.

use std::sync::Arc;

use diacraft_doc::{Document, RowKey, TextPart};
use diacraft_engine::formatting::{Align, Direction, EditorParagraph, editor_paragraphs};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

/// A page with shape 1 holding `parts`, whose paragraph rows are: 0, right to left and aligned 0 with the indents of an Arabic paragraph; 1, left to right and
/// aligned 2.
fn doc_with(parts: Vec<TextPart>) -> Document {
    let shape = ShapeBuilder::new(1)
        .text_parts(parts)
        .row_cell("Paragraph", None, RowKey::Ix(0), "Flags", "1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
        .row_cell("Paragraph", None, RowKey::Ix(0), "IndLeft", "0.5")
        .row_cell("Paragraph", None, RowKey::Ix(0), "IndRight", "0.25")
        .row_cell("Paragraph", None, RowKey::Ix(1), "Flags", "0")
        .row_cell("Paragraph", None, RowKey::Ix(1), "HorzAlign", "2");
    DocBuilder::new().root(|r| r.row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1")).page(1, PageBuilder::new().shape(shape)).build()
}

fn two_paragraphs() -> Document {
    doc_with(vec![TextPart::Pp(0), chars("abc\n"), TextPart::Pp(1), chars("def")])
}

fn rtl(p: &EditorParagraph) -> bool {
    p.direction == Direction::RightToLeft
}

/// The paragraphs of the shape as they are: one for each line feed and one more, each with the cells of its row.
#[test]
fn each_paragraph_has_the_cells_of_its_row() {
    let doc = two_paragraphs();
    let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
    assert_eq!(paragraphs.len(), 2);
    let (first, second) = (paragraphs[0], paragraphs[1]);
    assert!(rtl(&first) && first.align == Align::Left, "{first:?}");
    assert_eq!((first.ind_left, first.ind_right), (0.5, 0.25), "the two indents, in inches");
    assert!(!rtl(&second) && second.align == Align::Right, "{second:?}");
    assert_eq!((second.ind_left, second.ind_right), (0.0, 0.0), "a cell that is not there is 0");
}

/// A paragraph that no cell says anything of is left to right, with the root style's alignment, and a missing alignment is centred.
#[test]
fn a_text_with_no_markers_has_the_defaults() {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(vec![chars("plain")]))).build();
    let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
    assert_eq!(paragraphs, vec![EditorParagraph { direction: Direction::LeftToRight, align: Align::Center, ind_left: 0.0, ind_right: 0.0 }]);
}

/// A shape with no text has one paragraph, the one typing there would make, and a trailing line feed starts an empty one.
#[test]
fn an_empty_text_and_a_trailing_line_feed_have_a_paragraph_each() {
    let doc = doc_with(vec![]);
    assert_eq!(editor_paragraphs(&doc, 1, 1, None).unwrap().len(), 1);
    let doc = doc_with(vec![TextPart::Pp(0), chars("abc\n")]);
    let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
    assert_eq!(paragraphs.len(), 2, "a line feed ends a paragraph and the caret after it is in the next");
    assert!(paragraphs.iter().all(rtl), "the empty one takes the row in force at the end");
}

/// A line feed typed in the middle of a paragraph splits it into two that have its format, as the commit makes them (the new characters join the run before
/// the point).
#[test]
fn a_line_feed_typed_in_a_paragraph_splits_it_into_two_of_one_format() {
    let doc = two_paragraphs();
    let paragraphs = editor_paragraphs(&doc, 1, 1, Some((2, 2, "\n"))).unwrap();
    assert_eq!(paragraphs.len(), 3);
    assert!(rtl(&paragraphs[0]) && rtl(&paragraphs[1]), "both halves of the first paragraph read right to left");
    assert!(!rtl(&paragraphs[2]), "the second paragraph is as it was");
}

/// A letter typed at the end of a paragraph is the paragraph's.
#[test]
fn a_letter_typed_at_the_end_of_a_paragraph_keeps_its_format() {
    let doc = two_paragraphs();
    let paragraphs = editor_paragraphs(&doc, 1, 1, Some((3, 3, "x"))).unwrap();
    assert_eq!(paragraphs.len(), 2);
    assert!(rtl(&paragraphs[0]) && !rtl(&paragraphs[1]));
}

/// Taking out a line feed joins two paragraphs, and the one that is left has the format of the run holding its last character, the second's.
#[test]
fn joining_two_paragraphs_leaves_the_format_of_the_second() {
    let doc = two_paragraphs();
    let paragraphs = editor_paragraphs(&doc, 1, 1, Some((3, 4, ""))).unwrap();
    assert_eq!(paragraphs.len(), 1);
    assert!(!rtl(&paragraphs[0]) && paragraphs[0].align == Align::Right, "{:?}", paragraphs[0]);
}

/// Typing into a shape with no text takes the format of the paragraph row its first characters would join.
#[test]
fn typing_into_an_empty_shape_takes_the_row_the_text_starts_with() {
    let doc = doc_with(vec![TextPart::Pp(1)]);
    let paragraphs = editor_paragraphs(&doc, 1, 1, Some((0, 0, "a\nb"))).unwrap();
    assert_eq!(paragraphs.len(), 2);
    assert!(paragraphs.iter().all(|p| !rtl(p) && p.align == Align::Right), "{paragraphs:?}");
}

/// A range the text does not have, and a shape that is not there, are refused in words, and nothing is read.
#[test]
fn a_bad_range_or_shape_is_refused() {
    let doc = two_paragraphs();
    let error = editor_paragraphs(&doc, 1, 1, Some((5, 99, ""))).unwrap_err();
    assert!(error.message.contains("not within the text"), "{}", error.message);
    assert!(editor_paragraphs(&doc, 1, 9, None).is_err());
    assert!(editor_paragraphs(&doc, 7, 1, None).is_err());
}

/// However the text is cut by line feeds, there is one paragraph for each of them and one more: the window splits its characters the same way.
#[test]
fn there_is_one_paragraph_for_each_line_feed_and_one_more() {
    for text in ["", "a", "\n", "a\n", "\na", "a\nb", "a\n\nb\n", "\n\n\n"] {
        let doc = doc_with(if text.is_empty() { vec![] } else { vec![TextPart::Pp(0), chars(text)] });
        let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
        assert_eq!(paragraphs.len(), text.matches('\n').count() + 1, "{text:?}");
    }
}

/// A text of many line feeds is read up to a bound, and the paragraphs past it are left out, as the window needs only the first of them and whether they agree.
#[test]
fn the_paragraphs_read_are_bounded() {
    let text = "x\n".repeat(diacraft_engine::formatting::MAX_EDITOR_PARAGRAPHS + 50);
    let doc = doc_with(vec![TextPart::Pp(0), chars(&text)]);
    let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
    assert_eq!(paragraphs.len(), diacraft_engine::formatting::MAX_EDITOR_PARAGRAPHS);
    assert!(paragraphs.iter().all(rtl));
}

/// `Flags` other than 1 read left to right, and a number that is not one is read as the default.
#[test]
fn only_flags_1_reads_right_to_left() {
    for (flags, expected) in
        [("0", Direction::LeftToRight), ("1", Direction::RightToLeft), ("2", Direction::LeftToRight), ("-1", Direction::LeftToRight)]
    {
        let shape = ShapeBuilder::new(1).text_parts(vec![TextPart::Pp(0), chars("a")]).row_cell("Paragraph", None, RowKey::Ix(0), "Flags", flags);
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        assert_eq!(editor_paragraphs(&doc, 1, 1, None).unwrap()[0].direction, expected, "Flags {flags}");
    }
}
