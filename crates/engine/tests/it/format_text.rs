//! Formatting text: `text.format` sets Character rows, Paragraph rows and the text block's `VerticalAlign`. A range of characters splits
//! the runs it starts or ends in, and keeps every marker and field: the part of a run in the range gets a row of its
//! own, a copy of the run's row with the change, and the part left over keeps the row it had or a copy of it.
//!
//! The tests of this file call the core directly on documents built in code, and look at the parts of the text and the rows; the commands, the undo
//! step and the pictures are in `format.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Document, Owner, RowKey, SectionKey, TextPart};
use diacraft_engine::formatting::{Align, Rgb, TextFormat, VAlign, format_text};
use diacraft_testkit::build::{PageBuilder, ShapeBuilder};

use crate::format::{fv, local};

/// 12 points and 8 points in inches, as a file writes them.
const TWELVE: &str = "0.1666666666666667";
const EIGHT: &str = "0.1111111111111111";

fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

fn fld(ix: u32, text: &str) -> TextPart {
    TextPart::Fld { ix, text: Arc::from(text) }
}

/// A document of one page and one shape, 1, with this text and a first run of 12 points (row 0 of `Character`), black, and a root style that has
/// the same: the shape links to it.
fn doc_with(parts: Vec<TextPart>) -> Document {
    doc_of(ShapeBuilder::new(1).text_parts(parts).row_cell("Character", None, RowKey::Ix(0), "Size", TWELVE).row_cell(
        "Character",
        None,
        RowKey::Ix(0),
        "Style",
        "0",
    ))
}

fn doc_of(shape: ShapeBuilder) -> Document {
    diacraft_testkit::build::DocBuilder::new()
        .root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Color", "0").row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1"))
        .page(1, PageBuilder::new().shape(shape))
        .build()
}

/// The parts of shape 1's text as short words: `cp0`, `pp1`, `fld0=3`, and the characters in quotes.
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

/// The cells shape 1 holds in row `ix` of a section: name to (formula, value).
fn row(doc: &Document, section: &str, ix: u32) -> BTreeMap<String, (Option<String>, String)> {
    let shape = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let Some(section) = shape.sheet.section(&SectionKey::new(section, None)) else { return BTreeMap::new() };
    let Some(row) = section.row(&RowKey::Ix(ix)) else { return BTreeMap::new() };
    row.cells.iter().map(|(name, cell)| (name.as_str().to_owned(), fv(cell))).collect()
}

fn value(doc: &Document, section: &str, ix: u32, name: &str) -> Option<String> {
    row(doc, section, ix).get(name).map(|(_, v)| v.clone())
}

fn bold() -> TextFormat {
    TextFormat { bold: Some(true), ..TextFormat::default() }
}

fn format(doc: &mut Document, range: Option<(usize, usize)>, f: &TextFormat) -> usize {
    format_text(doc, 1, &[1], range, f).unwrap_or_else(|p| panic!("{range:?}: {}", p.message))
}

// ---- the runs a range splits -----------------------------------------------------------------------------------------------------

#[test]
fn a_range_at_the_end_of_a_run_splits_it_in_two_and_the_field_stays_with_the_last() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello world"), fld(0, "3")]);
    assert_eq!(format(&mut doc, Some((6, 11)), &bold()), 1);
    // `world` is run 1: a copy of run 0 (its 12 points), bold. The field has no characters after it and is the last run's.
    assert_eq!(words(&doc), ["cp0", "\"hello \"", "cp1", "\"world\"", "fld0=3"]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 1, "Size").as_deref(), Some(TWELVE), "the new run has the size of the one it came from");
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"), "the first run is as it was");
}

#[test]
fn a_range_in_the_middle_of_a_run_makes_three_runs_and_the_last_is_a_copy_of_the_first() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello world")]);
    format(&mut doc, Some((2, 8)), &TextFormat { italic: Some(true), size: Some(8.0), ..TextFormat::default() });
    assert_eq!(words(&doc), ["cp0", "\"he\"", "cp1", "\"llo wo\"", "cp2", "\"rld\""]);
    assert_eq!(row(&doc, "Character", 1).get("Style").map(|c| c.1.as_str()), Some("2"));
    assert_eq!(value(&doc, "Character", 1, "Size").as_deref(), Some(EIGHT), "8 points in inches");
    // The run after the range is what the first was.
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(TWELVE));
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Character", 0, "Size").as_deref(), Some(TWELVE));
}

#[test]
fn a_range_that_is_a_whole_run_changes_its_row_and_adds_no_marker() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd"), fld(0, "3")]);
    assert_eq!(format(&mut doc, Some((2, 4)), &bold()), 1);
    assert_eq!(words(&doc), ["cp0", "\"ab\"", "cp1", "\"cd\"", "fld0=3"]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"));
    // The whole text: row 0 only (there is one run, and it is in the range).
    let mut whole = doc_with(vec![TextPart::Cp(0), chars("ab")]);
    format(&mut whole, Some((0, 2)), &bold());
    assert_eq!(words(&whole), ["cp0", "\"ab\""]);
    assert_eq!(value(&whole, "Character", 0, "Style").as_deref(), Some("1"));
}

#[test]
fn a_range_across_two_runs_splits_both_and_the_tail_is_a_copy_of_the_second() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd")]);
    // Row 1 is italic; the range `b` to `c` is made bold.
    s_set(&mut doc, 1, "Style", "2");
    format(&mut doc, Some((1, 3)), &bold());
    // The rows are numbered in text order: the rows after the split moved down, with their cells.
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp1", "\"b\"", "cp2", "\"c\"", "cp3", "\"d\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"), "b: a copy of run 0, bold");
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("3"), "c: run 1 was italic, now bold and italic");
    assert_eq!(value(&doc, "Character", 3, "Style").as_deref(), Some("2"), "d: a copy of run 1, italic");
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"));
}

#[test]
fn a_range_format_that_changes_nothing_cuts_nothing() {
    // Bold on a run that is bold: no row would change, so no marker is written, no row is made and no row is moved.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello world")]);
    s_set(&mut doc, 0, "Style", "1");
    let before = doc.clone();
    assert_eq!(format(&mut doc, Some((2, 5)), &bold()), 0);
    assert_eq!(doc, before);
}

#[test]
fn rows_that_a_text_numbers_out_of_order_are_numbered_again_and_the_row_left_over_is_taken_out() {
    // A text from a file that names rows 0 and 3 (a reader follows the `IX`): after a split in its first run the runs are 0, 1 and 2, the cells
    // of row 3 are on row 2, and there is no row 3.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(3), chars("cd")]);
    s_set(&mut doc, 3, "Size", EIGHT);
    format(&mut doc, Some((1, 2)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp1", "\"b\"", "cp2", "\"cd\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(EIGHT), "the cells of the run that was row 3");
    assert!(row(&doc, "Character", 3).is_empty(), "row 3 is gone");
}

#[test]
fn a_text_whose_first_marker_is_not_row_zero_keeps_its_numbers() {
    // The first run names row 2, and row 0 may be a style's, so moving the cells of row 2 to row 0 would merge them with the style's: no row moves.
    let mut doc = doc_with(vec![TextPart::Cp(2), chars("ab"), TextPart::Cp(3), chars("cd")]);
    s_set(&mut doc, 2, "Size", EIGHT);
    s_set(&mut doc, 3, "Size", EIGHT);
    format(&mut doc, Some((1, 2)), &bold());
    // The new row takes the next free number, 4 (the range ends where `ab` does, so nothing is cut after it).
    assert_eq!(words(&doc), ["cp2", "\"a\"", "cp4", "\"b\"", "cp3", "\"cd\""]);
    assert_eq!(value(&doc, "Character", 4, "Style").as_deref(), Some("1"));
}

#[test]
fn rows_are_numbered_in_text_order_after_every_split_whichever_run_it_is_in() {
    // Format `lo`, then the `H` before it, then ` w` after it: each split is in a run that has later runs, and the markers stay 0, 1, 2 ... in text
    // order, each row holding the cells of the run it is on (the sizes tell the runs apart).
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("Hello world")]);
    format(&mut doc, Some((3, 5)), &TextFormat { size: Some(8.0), ..TextFormat::default() });
    format(&mut doc, Some((0, 1)), &TextFormat { size: Some(11.0), ..TextFormat::default() });
    format(&mut doc, Some((5, 7)), &TextFormat { size: Some(14.0), ..TextFormat::default() });
    assert_eq!(words(&doc), ["cp0", "\"H\"", "cp1", "\"el\"", "cp2", "\"lo\"", "cp3", "\" w\"", "cp4", "\"orld\""]);
    let sizes: Vec<f64> = (0..5).map(|ix| value(&doc, "Character", ix, "Size").unwrap().parse::<f64>().unwrap() * 72.0).collect();
    for (got, want) in sizes.iter().zip([11.0, 12.0, 8.0, 14.0, 12.0]) {
        assert!((got - want).abs() < 1e-6, "{sizes:?}");
    }
}

#[test]
fn rows_a_style_supplies_above_zero_are_not_moved_and_new_rows_take_the_next_free_number() {
    // The root style holds a row 1 as well as row 0: a text that names row 1 reads the style's cells there, and a row of the shape's own at 1 would
    // be merged with them. So no row is renumbered; a new row takes the next number above every row there is (2 here), and every row the text names
    // is a row of the shape's own afterwards.
    let mut doc = diacraft_testkit::build::DocBuilder::new()
        .root(|r| {
            r.row_cell("Character", None, RowKey::Ix(0), "Size", TWELVE).row_cell("Character", None, RowKey::Ix(1), "Size", EIGHT).row_cell(
                "Character",
                None,
                RowKey::Ix(1),
                "Style",
                "2",
            )
        })
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(1).links(None, None, Some(0)).text_parts(vec![
                TextPart::Cp(0),
                chars("ab"),
                TextPart::Cp(1),
                chars("cd"),
            ])),
        )
        .build();
    format(&mut doc, Some((0, 1)), &bold());
    // `a` is run 0 in place, `b` is a new run, then the style's row 1 for `cd`.
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp2", "\"b\"", "cp1", "\"cd\""]);
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("0"), "a copy of the first run's row, which the style gives no Style");
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(TWELVE));
    assert_eq!(value(&doc, "Character", 1, "Size").as_deref(), Some(EIGHT), "row 1, which the style gave, is the shape's own too now");
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("2"));
}

#[test]
fn rows_the_root_style_holds_above_zero_count_for_a_shape_with_no_style_link() {
    // A shape that links to no style has no chain, and the root style's row 0 and row 1 are what it reads for a cell it lacks (the renderer's rule, and
    // the rows a copy takes: `Rows`). Row 1 of the root is then a row a row of the shape's own at 1 would merge with, so no row is renumbered, and the
    // new row takes the next free number, 2, where it would take 1 if the root's rows were not counted.
    let mut doc = diacraft_testkit::build::DocBuilder::new()
        .root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Size", TWELVE).row_cell("Character", None, RowKey::Ix(1), "Size", EIGHT))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(vec![TextPart::Cp(0), chars("ab")])))
        .build();
    format(&mut doc, Some((1, 2)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp2", "\"b\""]);
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(TWELVE), "a copy of the run's row, which is the root's row 0");
}

#[test]
fn markers_that_name_one_row_go_on_naming_one_row_when_the_rows_are_numbered_again() {
    // Rows 0 and 1 are each named twice (a reader follows the `IX`). A split in the first run makes a new run; the rows are numbered in the order they
    // first appear in the text, 0, then the new run's, then 1, and each marker takes the number of its row: the runs that shared row 1 share row 2 now.
    let mut doc =
        doc_with(vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef"), TextPart::Cp(1), chars("gh")]);
    s_set(&mut doc, 1, "Size", EIGHT);
    format(&mut doc, Some((1, 2)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp1", "\"b\"", "cp2", "\"cd\"", "cp0", "\"ef\"", "cp2", "\"gh\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"), "b: the new run, a copy of row 0 and bold");
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(EIGHT), "cd and gh: the cells of what was row 1");
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"), "a and ef: row 0, as it was");
    assert!(row(&doc, "Character", 3).is_empty(), "no row more than the three there are");
}

/// Sets a cell of a local row of shape 1 (a fixture edit).
fn s_set(doc: &mut Document, ix: u32, name: &str, value: &str) {
    let at = doc.find_shape(Owner::Page(1), 1).unwrap();
    let addr = diacraft_doc::CellAddr::in_row("Character", None, RowKey::Ix(ix), name);
    doc.write_cell(
        &diacraft_doc::SheetRef::Shape(at),
        &addr,
        diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text(value)),
        diacraft_doc::WriteOrigin::Drawing,
    )
    .unwrap();
}

#[test]
fn a_run_that_two_markers_name_is_not_changed_for_the_one_outside_the_range() {
    // Runs 1 and 3 name row 0 (a reader follows the `IX` as written). The range is run 3 only.
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")]);
    s_set(&mut doc, 1, "Size", EIGHT);
    format(&mut doc, Some((4, 6)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"ab\"", "cp1", "\"cd\"", "cp2", "\"ef\""], "the last run has a row of its own");
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 2, "Size").as_deref(), Some(TWELVE));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"), "the first run did not turn bold");
}

#[test]
fn a_new_row_holds_the_cells_the_command_writes_even_where_its_source_has_none() {
    // The root gives a size and a colour and no face and no strike: after a range of the face, the last run holds `Font` 0 (no face named)
    // and `Strikethru` 0, where the first run's change would otherwise carry to it in a reader that carries a cell from one run to the next.
    let mut doc = doc_of(ShapeBuilder::new(1).text("hello world"));
    format(&mut doc, Some((2, 5)), &TextFormat { font: Some("Liberation Serif".to_owned()), strike: Some(true), ..TextFormat::default() });
    assert_eq!(words(&doc), ["cp0", "\"he\"", "cp1", "\"llo\"", "cp2", "\" world\""]);
    assert_eq!(value(&doc, "Character", 1, "Font").as_deref(), Some("Liberation Serif"));
    assert_eq!(value(&doc, "Character", 2, "Font").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Character", 2, "Strikethru").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Character", 1, "Strikethru").as_deref(), Some("1"));
}

#[test]
fn text_with_no_markers_gets_the_first_one_written_out() {
    let mut doc = doc_of(ShapeBuilder::new(1).text("hello"));
    format(&mut doc, Some((1, 3)), &bold());
    // Row 0 is the shape's own (here from the root style); `el` is the new run 1 and `lo` run 2.
    assert_eq!(words(&doc), ["cp0", "\"h\"", "cp1", "\"el\"", "cp2", "\"lo\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 1, "Color").as_deref(), Some("0"), "the new run keeps what its row inherited from the style: the colour");
    assert_eq!(value(&doc, "Character", 2, "Color").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Character", 2, "Style").as_deref(), Some("0"), "the last run holds the cell that was changed, as 0");
    // Run 0 is written as a row of the shape's own, with what the style gave it: a reader that takes the first row of a shape for run 0 (LibreOffice
    // does) reads a text whose row 0 a style supplies right, which it does not when the file holds rows 1 and 2 only.
    assert_eq!(value(&doc, "Character", 0, "Color").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"));
}

#[test]
fn a_field_inside_the_range_is_formatted_with_it_and_kept_in_place() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), fld(0, "7"), chars("cd")]);
    format(&mut doc, Some((1, 3)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"a\"", "cp1", "\"b\"", "fld0=7", "\"c\"", "cp2", "\"d\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
}

#[test]
fn a_field_at_the_edge_of_the_range_stays_with_the_run_it_is_in() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("ab"), fld(0, "7"), TextPart::Cp(1), chars("cd")]);
    format(&mut doc, Some((2, 4)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"ab\"", "fld0=7", "cp1", "\"cd\""]);
    assert_eq!(value(&doc, "Character", 1, "Style").as_deref(), Some("1"));
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("0"));
}

#[test]
fn offsets_count_characters_not_bytes() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("h\u{e9}llo \u{1f600}!")]);
    format(&mut doc, Some((1, 2)), &bold());
    assert_eq!(words(&doc), ["cp0", "\"h\"", "cp1", "\"\u{e9}\"", "cp2", "\"llo \u{1f600}!\""]);
    let mut emoji = doc_with(vec![TextPart::Cp(0), chars("h\u{e9}llo \u{1f600}!")]);
    format(&mut emoji, Some((6, 7)), &bold());
    assert_eq!(words(&emoji), ["cp0", "\"h\u{e9}llo \"", "cp1", "\"\u{1f600}\"", "cp2", "\"!\""]);
}

#[test]
fn a_range_the_text_does_not_have_is_refused_and_leaves_the_document() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("abcd")]);
    let before = doc.clone();
    for (start, end) in [(3, 2), (0, 5), (5, 5), (usize::MAX, usize::MAX), (0, usize::MAX)] {
        let problem = format_text(&mut doc, 1, &[1], Some((start, end)), &bold()).unwrap_err();
        assert!(problem.bad_params, "{start}..{end}: {}", problem.message);
        assert_eq!(doc, before);
    }
    assert!(format_text(&mut doc, 1, &[9], None, &bold()).is_err() && format_text(&mut doc, 7, &[1], None, &bold()).is_err());
    assert_eq!(doc, before);
}

#[test]
fn a_range_needs_one_shape() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("abcd")]);
    let problem = format_text(&mut doc, 1, &[1, 1], Some((0, 1)), &bold()).unwrap_err();
    assert!(problem.bad_params, "{}", problem.message);
}

// ---- the paragraph rows ----------------------------------------------------------------------------------------------------------

#[test]
fn alignment_of_a_range_splits_the_paragraphs_it_touches_at_their_line_breaks() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("one\ntwo\nthree")]);
    // `w` is in the second paragraph: from after the first line break to after the second.
    format(&mut doc, Some((5, 6)), &TextFormat { align: Some(Align::Right), ..TextFormat::default() });
    assert_eq!(words(&doc), ["cp0", "pp0", "\"one\\n\"", "pp1", "\"two\\n\"", "pp2", "\"three\""]);
    assert_eq!(value(&doc, "Paragraph", 1, "HorzAlign").as_deref(), Some("2"));
    assert_eq!(
        value(&doc, "Paragraph", 2, "HorzAlign").as_deref(),
        Some("1"),
        "the paragraph after is a copy of the first's row, with what the style gives it"
    );
    assert_eq!(
        value(&doc, "Paragraph", 0, "HorzAlign").as_deref(),
        Some("1"),
        "the first paragraph is as it was, written as a row of the shape's own with what the style gave it"
    );
}

#[test]
fn alignment_of_a_whole_text_changes_every_paragraph_row_it_uses() {
    let mut doc = doc_with(vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    format(&mut doc, None, &TextFormat { align: Some(Align::Left), ..TextFormat::default() });
    assert_eq!(words(&doc), ["cp0", "pp0", "\"one\\n\"", "pp1", "\"two\""], "no marker is added");
    assert_eq!(value(&doc, "Paragraph", 0, "HorzAlign").as_deref(), Some("0"));
    assert_eq!(value(&doc, "Paragraph", 1, "HorzAlign").as_deref(), Some("0"));
    // A shape with no paragraph marker uses row 0.
    let mut plain = doc_with(vec![chars("one\ntwo")]);
    format(&mut plain, None, &TextFormat { align: Some(Align::Justify), ..TextFormat::default() });
    assert_eq!(words(&plain), ["\"one\\ntwo\""]);
    assert_eq!(value(&plain, "Paragraph", 0, "HorzAlign").as_deref(), Some("3"));
}

/// Distributed is a fifth alignment, written as `HorzAlign` 4 and spelled `distributed`; justify stays 3.
#[test]
fn distributed_alignment_is_horzalign_4_and_justify_stays_3() {
    let mut doc = doc_with(vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]);
    format(&mut doc, None, &TextFormat { align: Some(Align::Distributed), ..TextFormat::default() });
    assert_eq!(value(&doc, "Paragraph", 0, "HorzAlign").as_deref(), Some("4"));
    assert_eq!(value(&doc, "Paragraph", 1, "HorzAlign").as_deref(), Some("4"));
    format(&mut doc, None, &TextFormat { align: Some(Align::Justify), ..TextFormat::default() });
    assert_eq!(value(&doc, "Paragraph", 0, "HorzAlign").as_deref(), Some("3"));
    assert_eq!(Align::Distributed.word(), "distributed");
    assert_eq!(Align::from_word("Distributed"), Some(Align::Distributed));
    assert_eq!((Align::from_cell(3.0), Align::from_cell(4.0), Align::from_cell(5.0)), (Align::Justify, Align::Distributed, Align::Left));
}

// ---- the cells, whole text -------------------------------------------------------------------------------------------------------

#[test]
fn the_character_cells_are_written_as_a_file_holds_them() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    let f = TextFormat {
        font: Some("Liberation Sans".to_owned()),
        size: Some(11.0),
        bold: Some(true),
        italic: Some(true),
        underline: Some(true),
        strike: Some(true),
        color: Some(diacraft_engine::formatting::TextPaint::Colour(Rgb(0xC0, 0x00, 0x00))),
        ..TextFormat::default()
    };
    assert_eq!(format(&mut doc, None, &f), 1);
    let r = row(&doc, "Character", 0);
    assert_eq!(r.get("Font"), Some(&(None, "Liberation Sans".to_owned())));
    assert!((r["Size"].1.parse::<f64>().unwrap() - 11.0 / 72.0).abs() < 1e-12, "{:?}", r["Size"]);
    assert_eq!(r["Style"], (None, "7".to_owned()), "bold, italic and underline are bits 1, 2 and 4 of Style");
    assert_eq!(r["Strikethru"], (None, "1".to_owned()));
    assert_eq!(r["Color"], (Some("RGB(192,0,0)".to_owned()), "#c00000".to_owned()));
    // The font is in the table, once.
    assert!(doc.fonts.iter().any(|f| &*f.name_u == "Liberation Sans"));
    format(&mut doc, None, &TextFormat { font: Some("Liberation Sans".to_owned()), size: Some(12.0), ..TextFormat::default() });
    assert_eq!(doc.fonts.iter().filter(|f| &*f.name_u == "Liberation Sans").count(), 1);
}

#[test]
fn style_bits_are_set_and_cleared_one_at_a_time_and_the_others_are_kept() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    s_set(&mut doc, 0, "Style", "17");
    // 17 is bold and the complex-script bold: setting italic keeps both.
    format(&mut doc, None, &TextFormat { italic: Some(true), ..TextFormat::default() });
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("19"));
    format(&mut doc, None, &TextFormat { bold: Some(false), ..TextFormat::default() });
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("18"), "bold off clears bit 1 only");
    format(&mut doc, None, &TextFormat { underline: Some(true), strike: Some(false), ..TextFormat::default() });
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("22"));
    assert_eq!(value(&doc, "Character", 0, "Strikethru"), None, "off where it is off already writes nothing");
}

#[test]
fn a_face_is_matched_to_the_table_ignoring_case_and_added_when_it_is_not_there() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    Arc::make_mut(&mut doc.fonts).push(diacraft_doc::FaceName { name_u: Arc::from("Noto Sans"), raw: Default::default(), ext: Default::default() });
    format(&mut doc, None, &TextFormat { font: Some("noto sans".to_owned()), ..TextFormat::default() });
    assert_eq!(value(&doc, "Character", 0, "Font").as_deref(), Some("Noto Sans"), "the table's spelling");
    assert_eq!(doc.fonts.len(), 1);
    format(&mut doc, None, &TextFormat { font: Some("DejaVu Serif".to_owned()), ..TextFormat::default() });
    assert_eq!(doc.fonts.len(), 2);
    assert_eq!(&*doc.fonts[1].name_u, "DejaVu Serif");
}

#[test]
fn a_block_alignment_is_one_cell_of_the_shape() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    for (v, want) in [(VAlign::Top, "0"), (VAlign::Middle, "1"), (VAlign::Bottom, "2")] {
        format(&mut doc, None, &TextFormat { valign: Some(v), ..TextFormat::default() });
        assert_eq!(local(&doc, 1, "VerticalAlign").map(|c| fv(&c).1).as_deref(), Some(want));
    }
}

#[test]
fn text_formatting_reaches_a_shape_with_no_text_for_the_text_typed_next() {
    let mut doc = doc_of(ShapeBuilder::new(1));
    assert_eq!(format(&mut doc, None, &bold()), 1);
    assert_eq!(value(&doc, "Character", 0, "Style").as_deref(), Some("1"));
    assert_eq!(words(&doc), ["(no text)"]);
}

#[test]
fn a_call_that_changes_nothing_writes_nothing() {
    let mut doc = doc_with(vec![TextPart::Cp(0), chars("hello")]);
    format(&mut doc, None, &TextFormat { size: Some(12.0), ..TextFormat::default() });
    let before = doc.clone();
    assert_eq!(format(&mut doc, None, &TextFormat { size: Some(12.0), bold: Some(false), ..TextFormat::default() }), 0);
    assert_eq!(doc, before);
}

#[test]
fn the_members_of_a_group_are_formatted_and_not_the_group() {
    let group = ShapeBuilder::new(1).group().child(ShapeBuilder::new(2).text("a")).child(ShapeBuilder::new(3).text("b"));
    let mut doc = doc_of(group);
    assert_eq!(format_text(&mut doc, 1, &[1], None, &bold()).unwrap(), 2);
    let page = doc.page(1).unwrap();
    let g = page.shapes.get(0).unwrap();
    assert!(g.sheet.section(&SectionKey::new("Character", None)).is_none(), "the group holds nothing");
    for member in g.children.iter() {
        let sec = member.sheet.section(&SectionKey::new("Character", None)).unwrap();
        assert_eq!(sec.row(&RowKey::Ix(0)).unwrap().cells.get("Style").map(|c| fv(c).1).as_deref(), Some("1"));
    }
}
