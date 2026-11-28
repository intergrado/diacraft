//! What the format commands do in a count of steps, for the sizes a hostile file can reach. These are crate-internal tests
//! because the counts (`counts`) are of the module's own work: a count is a bound that does not move with the speed or the load of the machine, which
//! the timed tests of `tests/it/format_scale.rs` do.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, SectionKey, Shape, ShapeList, Sheet, Text, TextPart, Value};

use super::counts;
use super::quick::{QuickStyle, format_quick_style};
use super::{Mixed, RangeLook, TextFormat, format_text, look_of_range, targets, targets_reached};
use crate::draw::{NODES_WALKED, page_mut};
use crate::theme::{Base, Choice, follow_work, reset_follow_work};

const TWELVE: &str = "0.1666666666666667";

/// A document with one shape, 1, on page 1, that holds this text and a row of `Character` with a size for each `IX` in `rows`; the size of the row
/// `guarded` is `GUARD(...)`.
fn doc_with(parts: Vec<TextPart>, rows: impl IntoIterator<Item = u32>, guarded: Option<u32>) -> Document {
    let mut sheet = Sheet::default();
    for ix in rows {
        let size = if guarded == Some(ix) {
            Cell::with_formula("GUARD(0.1666666666666667)", Some(Value::from_text(TWELVE)))
        } else {
            Cell::constant(Value::from_text(TWELVE))
        };
        sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(ix), "Size"), size).unwrap();
    }
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    shape.text = Some(Arc::new(Text { parts, ..Default::default() }));
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    doc
}

fn bold() -> TextFormat {
    TextFormat { bold: Some(true), ..TextFormat::default() }
}

/// `cp` 0 and two letters, then a run of one letter for each of the rows `first..first + n` (far above 0, as a text from a file may number them).
fn text_with_far_rows(first: u32, n: u32) -> Vec<TextPart> {
    let mut parts = vec![TextPart::Cp(0), TextPart::Chars("ab".into())];
    for ix in first..first + n {
        parts.push(TextPart::Cp(ix));
        parts.push(TextPart::Chars("c".into()));
    }
    parts
}

fn rows_of(doc: &Document) -> Vec<u32> {
    let shape = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let Some(section) = shape.sheet.section(&SectionKey::new("Character", None)) else { return Vec::new() };
    section.rows.iter().filter_map(|row| row.ix).collect()
}

#[test]
fn the_rows_a_split_leaves_behind_are_taken_out_in_one_pass_over_the_section() {
    // 3,000 rows named far above the rest: a split in the first run numbers the rows again from 0, so all of the old rows are left behind, and each
    // pass over the section would copy its rows. One pass takes them all.
    let n = 3_000;
    let mut doc = doc_with(text_with_far_rows(40_000, n), std::iter::once(0).chain(40_000..40_000 + n), None);
    counts::reset();
    format_text(&mut doc, 1, &[1], Some((1, 2)), &bold()).unwrap();
    assert_eq!(counts::section_passes(), 1, "one pass for the {n} rows left behind");
    let rows = rows_of(&doc);
    assert!(rows.iter().all(|ix| *ix <= n + 1), "the rows are 0 to {}, none above: {:?}", n + 1, rows.iter().max());
    assert_eq!(rows.len() as u32, n + 2, "a row for each of the runs: the three of the split run and the others");
}

#[test]
fn a_guarded_cell_in_a_row_to_go_is_refused_before_any_row_is_taken_out() {
    let n = 50;
    // The last of the rows to go holds a guarded cell.
    let mut doc = doc_with(text_with_far_rows(40_000, n), std::iter::once(0).chain(40_000..40_000 + n), Some(40_000 + n - 1));
    counts::reset();
    let problem = format_text(&mut doc, 1, &[1], Some((1, 2)), &bold()).unwrap_err();
    assert!(problem.message.contains("GUARD"), "{}", problem.message);
    assert_eq!(counts::section_passes(), 0, "no row was taken out before the refusal");
    assert!(rows_of(&doc).contains(&(40_000 + n - 1)), "the guarded row is still there");
}

#[test]
fn markers_that_name_one_row_write_one_row_however_many_there_are() {
    // 320,000 runs that all name row 0 (a reader follows the `IX`), and a format of a range of them: the runs of the range share row 0 with the
    // runs outside it, so they are given one copy, and the rows are renumbered, not the markers: two rows are written, and not a row for each marker.
    let mut parts = Vec::with_capacity(640_000);
    for _ in 0..320_000 {
        parts.push(TextPart::Cp(0));
        parts.push(TextPart::Chars("a".into()));
    }
    let mut doc = doc_with(parts, [0], None);
    counts::reset();
    format_text(&mut doc, 1, &[1], Some((1_000, 200_000)), &bold()).unwrap();
    assert!(counts::rows_set() <= 2, "rows planned whole: {}", counts::rows_set());
    assert_eq!(rows_of(&doc), [0, 1]);
    let shape = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let markers: Vec<u32> =
        shape.text.as_ref().unwrap().parts.iter().filter_map(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect();
    assert_eq!(markers.len(), 320_000, "no marker added or taken away");
    assert_eq!(markers.iter().filter(|ix| **ix == 1).count(), 199_000, "the runs of the range name the copy");
    assert_eq!(markers.iter().filter(|ix| **ix == 0).count(), 121_000);
}

#[test]
fn a_text_that_names_more_rows_than_a_section_numbers_is_refused_where_it_would_be_written() {
    // 70,000 rows, the numbers 0 to 69,999: more than the 65,536 a reader takes. A split needs a new number and there is none above the
    // highest; a format of the whole text would write a row for each. Both are refused with the words of a full section, and nothing is planned.
    // The first run holds two letters, so that the range 1 to 2 cuts it.
    let parts: Vec<TextPart> = (0..70_000).flat_map(|ix| [TextPart::Cp(ix), TextPart::Chars(if ix == 0 { "ab" } else { "a" }.into())]).collect();
    let mut doc = doc_with(parts, [0], None);
    let before = doc.clone();
    counts::reset();
    for range in [Some((1, 2)), None] {
        let problem = format_text(&mut doc, 1, &[1], range, &bold()).unwrap_err();
        assert!(!problem.bad_params && problem.message.contains("too many runs or paragraphs"), "{range:?}: {}", problem.message);
        assert_eq!(doc, before, "{range:?}");
    }
    assert_eq!(counts::rows_set(), 0, "no row was planned");
}

// ---- reading a range -----------------------------------------------------------------------------------------------------

/// A document with one shape, 1, whose text is `parts` and whose rows of `Character` and `Paragraph` hold the cells given: `(section, IX, cell, value)`.
fn doc_of_cells(parts: Vec<TextPart>, cells: &[(&str, u32, &str, &str)]) -> Document {
    let mut sheet = Sheet::default();
    for (section, ix, name, value) in cells {
        sheet.set_cell(&CellAddr::in_row(section, None, RowKey::Ix(*ix), name), Cell::constant(Value::from_text(value))).unwrap();
    }
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    shape.text = Some(Arc::new(Text { parts, ..Default::default() }));
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    doc
}

fn look_range(doc: &Document, start: usize, end: usize) -> RangeLook {
    look_of_range(doc, 1, &[1], start, end).unwrap_or_else(|p| panic!("{start}..{end}: {}", p.message))
}

/// `n` runs of one letter each, run `i` naming the row `row(i)`.
fn letters(n: u32, row: impl Fn(u32) -> u32) -> Vec<TextPart> {
    (0..n).flat_map(|i| [TextPart::Cp(row(i)), TextPart::Chars("a".into())]).collect()
}

#[test]
fn markers_that_name_two_rows_read_two_rows_in_one_pass() {
    // 200,000 runs that name row 0 and row 1 in turn, the rows holding the same cells: a range over all of them reads each row once, not once for each
    // marker, and walks the parts once.
    let n = 200_000;
    let cells = [("Character", 0, "Size", TWELVE), ("Character", 1, "Size", TWELVE)];
    let doc = doc_of_cells(letters(n, |i| i % 2), &cells);
    counts::reset();
    let seen = look_range(&doc, 0, n as usize);
    assert_eq!(seen.mixed, Mixed::default());
    assert_eq!(counts::run_rows_read(), 2, "a row read once however many markers name it");
    assert_eq!(counts::run_parts_scanned(), 2 * n as usize, "every part once");
}

#[test]
fn a_range_reads_the_runs_it_touches_and_stops_where_it_ends() {
    // 100,000 runs each in a row of its own, and a range over the first three characters: three rows are read and the walk stops at the marker of the
    // fourth run, a handful of parts in, however long the text is.
    let n = 100_000;
    let doc = doc_of_cells(letters(n, |i| i), &[]);
    counts::reset();
    look_range(&doc, 0, 3);
    assert_eq!(counts::run_rows_read(), 3);
    assert!(counts::run_parts_scanned() <= 8, "parts walked: {}", counts::run_parts_scanned());
    // A point is the character before it, one run.
    counts::reset();
    look_range(&doc, 2, 2);
    assert_eq!(counts::run_rows_read(), 1);
    // A range in the middle reads only the runs in it.
    counts::reset();
    look_range(&doc, 50_000, 50_010);
    assert_eq!(counts::run_rows_read(), 10);
}

#[test]
fn rows_that_agree_are_each_read_once_and_the_cost_grows_with_the_rows() {
    for n in [2_000_u32, 16_000] {
        // Rows 1 and up: nobody holds them, so each reads the defaults. (Row 0 reads the root style's, which is another size.)
        let doc = doc_of_cells(letters(n, |i| i + 1), &[]);
        counts::reset();
        let seen = look_range(&doc, 0, n as usize);
        assert_eq!(seen.mixed, Mixed::default());
        assert_eq!(counts::run_rows_read(), n as usize, "one read of each of the {n} rows");
        assert_eq!(counts::run_parts_scanned(), 2 * n as usize);
    }
}

#[test]
fn once_every_property_is_mixed_the_other_rows_are_not_read() {
    // Rows 0 and 1 differ in the face, size, bits, strike, colour and case: the eight character properties are all mixed after two rows, and 20,000
    // more are left unread.
    let cells = [
        ("Character", 0, "Font", "Calibri"),
        ("Character", 0, "Size", TWELVE),
        ("Character", 0, "Style", "0"),
        ("Character", 0, "Strikethru", "0"),
        ("Character", 0, "Color", "#000000"),
        ("Character", 1, "Font", "Arial"),
        ("Character", 1, "Size", "0.1111111111111111"),
        ("Character", 1, "Style", "7"),
        ("Character", 1, "Strikethru", "1"),
        ("Character", 1, "Color", "#336699"),
        ("Character", 1, "Case", "1"),
    ];
    let n = 20_000;
    let doc = doc_of_cells(letters(n, |i| i), &cells);
    counts::reset();
    let seen = look_range(&doc, 0, n as usize);
    assert_eq!(seen.mixed.names(), ["font", "size", "bold", "italic", "underline", "strike", "color", "case"]);
    assert_eq!(counts::run_rows_read(), 2, "the third row and the rest were not read");
}

#[test]
fn paragraph_rows_are_read_once_each_and_stop_when_every_paragraph_property_is_mixed() {
    // 20,000 paragraphs of one letter and a line break each, all in rows that hold the same alignment, bullet and direction: each row is read once. Then
    // rows 0 and 1 differ in all three: two rows are read. Rows that differ in the alignment alone leave the bullet and the direction open, so every
    // row is read once, and no row twice.
    let n = 20_000;
    let parts: Vec<TextPart> = (0..n).flat_map(|i| [TextPart::Pp(i), TextPart::Chars("a\n".into())]).collect();
    let same: Vec<(&str, u32, &str, &str)> = (0..n).map(|i| ("Paragraph", i, "HorzAlign", "0")).collect();
    let doc = doc_of_cells(parts.clone(), &same);
    counts::reset();
    let seen = look_range(&doc, 0, 2 * n as usize);
    assert_eq!(seen.mixed, Mixed::default());
    assert_eq!(counts::para_rows_read(), n as usize);
    let mut differ = same.clone();
    differ[1] = ("Paragraph", 1, "HorzAlign", "2");
    differ.extend([("Paragraph", 1, "Bullet", "1"), ("Paragraph", 1, "Flags", "1")]);
    let doc = doc_of_cells(parts.clone(), &differ);
    counts::reset();
    let seen = look_range(&doc, 0, 2 * n as usize);
    assert_eq!(seen.mixed.names(), ["align", "bullet", "direction"]);
    assert_eq!(counts::para_rows_read(), 2);
    let mut align_only = same;
    align_only[1] = ("Paragraph", 1, "HorzAlign", "2");
    let doc = doc_of_cells(parts, &align_only);
    counts::reset();
    let seen = look_range(&doc, 0, 2 * n as usize);
    assert_eq!(seen.mixed.names(), ["align"]);
    assert_eq!(counts::para_rows_read(), n as usize);
}

#[test]
fn a_point_in_a_long_text_reads_one_run_and_one_paragraph() {
    let n = 50_000;
    let parts: Vec<TextPart> = (0..n).flat_map(|i| [TextPart::Cp(i), TextPart::Pp(i), TextPart::Chars("a\n".into())]).collect();
    let doc = doc_of_cells(parts, &[]);
    counts::reset();
    look_range(&doc, 31_000, 31_000);
    assert_eq!((counts::run_rows_read(), counts::para_rows_read()), (1, 1));
}

#[test]
fn a_split_before_many_runs_plans_a_row_for_each_run_and_no_more() {
    // `b` of the first run: the run is cut, and every row after it moves one down (text order), so a row for each run is written, and not a
    // row for each run for each run. Counted for n and eight times n: the rows planned whole are the runs, exactly.
    for n in [1_500_u32, 12_000] {
        let parts: Vec<TextPart> = (0..n).flat_map(|ix| [TextPart::Cp(ix), TextPart::Chars("ab".into())]).collect();
        let mut doc = doc_with(parts, 0..n, None);
        counts::reset();
        format_text(&mut doc, 1, &[1], Some((1, 2)), &bold()).unwrap();
        assert_eq!(counts::rows_set(), n as usize, "one row planned for each of the {n} runs");
        assert_eq!(counts::section_passes(), 0, "no row was left behind to take out");
        assert_eq!(rows_of(&doc).len(), n as usize + 1, "a row for each run, and one for the piece the split cut off");
    }
}

// ---- the face that is shown ----------------------------------------------------------------------------------------------

#[test]
fn the_default_face_is_asked_of_the_font_database_once_for_all_the_runs_and_shapes_of_a_call() {
    // 16,000 runs in rows that name no face: every run is shown as the default, which is asked of the database once, not once for each run.
    let n = 16_000_u32;
    let doc = doc_of_cells(letters(n, |i| i + 1), &[]);
    counts::reset();
    let seen = look_range(&doc, 0, n as usize);
    assert_eq!(counts::run_rows_read(), n as usize, "every row was read");
    assert_eq!(counts::defaults_looked_up(), 1, "one question for the {n} runs");
    assert!(seen.look.font.is_some() && !seen.mixed.font);
    // 4,096 shapes, none naming a face: one question again. (`look_of` reads the first run of each.)
    let mut doc = Document::new_blank();
    for id in 1..=4_096_u32 {
        let mut shape = Shape::new(id);
        shape.text = Some(Arc::new(Text { parts: vec![TextPart::Cp(0), TextPart::Chars("a".into())], ..Default::default() }));
        page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    }
    let ids: Vec<u32> = (1..=4_096).collect();
    counts::reset();
    let looks = super::look_of(&doc, 1, &ids).unwrap();
    assert_eq!(looks.len(), 4_096);
    assert_eq!(counts::run_rows_read(), 4_096);
    assert_eq!(counts::defaults_looked_up(), 1);
    assert!(looks.iter().all(|l| l.font == looks[0].font && l.font.is_some()));
}

#[test]
fn a_call_in_which_every_run_names_its_face_does_not_ask_for_the_default() {
    let cells = [("Character", 0, "Font", "Quill Sans"), ("Character", 1, "Font", "Ferro Text")];
    let doc = doc_of_cells(letters(1_000, |i| i % 2), &cells);
    counts::reset();
    let seen = look_range(&doc, 0, 1_000);
    assert!(seen.mixed.font);
    assert_eq!(counts::defaults_looked_up(), 0);
}

// ---- one walk of the page for the shapes a command names --------------------------------------------------------------------------

/// A page of `n` shapes at the top level, with the `ID`s 1 to `n` in order.
fn page_of(n: u32) -> Document {
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes = ShapeList::from_vec((1..=n).map(|id| Arc::new(Shape::new(id))).collect());
    doc
}

/// The counts of the walks of this thread: the shapes the walks of one pass visited, and the shapes `Document::find_shape` visited.
fn walks() -> (usize, u64) {
    (NODES_WALKED.with(std::cell::Cell::get), diacraft_doc::work::finds_visited())
}

fn reset_walks() {
    NODES_WALKED.with(|c| c.set(0));
    diacraft_doc::work::reset();
}

#[test]
fn the_shapes_named_are_found_in_one_walk_of_the_page_however_many_are_named() {
    let n = 30_000;
    let doc = page_of(n);
    // 4,096 ids spread over the page: found one by one each is about half the page away, which is 4,096 walks.
    let ids: Vec<u32> = (0..4_096).map(|k| 1 + k * 7).collect();
    reset_walks();
    let found = targets(&doc, 1, &ids).unwrap();
    let (nodes, finds) = walks();
    assert_eq!(
        found.iter().map(|at| at.path.clone()).collect::<Vec<_>>(),
        ids.iter().map(|id| vec![id - 1]).collect::<Vec<_>>(),
        "in the order named"
    );
    assert_eq!(finds, 0, "no shape is looked for from the top of the page");
    assert!(nodes <= n as usize, "one walk of the page, {nodes} visits for a page of {n}");
    // One shape named costs no more than the walk up to it, as a search for it did.
    reset_walks();
    targets(&doc, 1, &[3]).unwrap();
    let (nodes, finds) = walks();
    assert_eq!((finds, nodes <= 3), (0, true), "the walk stops at the last shape named: {nodes} visits");
}

#[test]
fn a_shape_named_that_is_not_on_the_page_is_refused_as_before_and_the_first_of_two_with_one_id_is_the_one_found() {
    let mut doc = page_of(10);
    let problem = targets(&doc, 1, &[5, 999, 7]).unwrap_err();
    assert!(problem.bad_params && problem.message.contains("page 1 has no shape 999"), "{problem:?}");
    let problem = targets(&doc, 9, &[1]).unwrap_err();
    assert!(problem.bad_params && problem.message.contains("there is no page 9"), "{problem:?}");
    assert!(targets(&doc, 9, &[]).unwrap().is_empty(), "no shape named, nothing to look for: the page is not looked at");
    // A part may hold an `ID` twice: the first in document order is the one named, as `find_shape` gives it, and a name given twice is one shape.
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(Shape::new(4)));
    let found = targets(&doc, 1, &[4, 4, 2]).unwrap();
    assert_eq!(found.iter().map(|at| at.path.clone()).collect::<Vec<_>>(), [vec![3], vec![1]]);
}

/// A page with a group, 1, of two members, 2 and 3, and a shape, 4, beside it.
fn group_page() -> Document {
    let mut group = Shape::new(1);
    group.children = ShapeList::from_vec(vec![Arc::new(Shape::new(2)), Arc::new(Shape::new(3))]);
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes = ShapeList::from_vec(vec![Arc::new(group), Arc::new(Shape::new(4))]);
    doc
}

#[test]
fn a_shape_is_reached_through_a_group_unless_it_is_named_itself() {
    let doc = group_page();
    let reached = |ids: &[u32]| -> Vec<(u32, bool)> {
        targets_reached(&doc, 1, ids).unwrap().iter().map(|t| (doc.shape(&t.at).unwrap().id, t.through_group)).collect()
    };
    assert_eq!(reached(&[1]), [(2, true), (3, true)], "the members of a group named are reached through it");
    assert_eq!(reached(&[4]), [(4, false)], "a shape of no members that is named is not");
    assert_eq!(reached(&[3]), [(3, false)], "nor is a member that is named by itself");
    // A member named as well as its group is a shape named, whichever comes first (and is formatted once).
    assert_eq!(reached(&[1, 3]), [(2, true), (3, false)]);
    assert_eq!(reached(&[3, 1]), [(3, false), (2, true)]);
    // `targets` is the same shapes in the same order.
    let plain: Vec<u32> = targets(&doc, 1, &[1, 3]).unwrap().iter().map(|at| doc.shape(at).unwrap().id).collect();
    assert_eq!(plain, [2, 3]);
}

/// A page of `n` shapes that follow the `Theme` style of Tidepool, `n - 1` of them copies of the first.
fn themed_page_of(n: u32) -> Document {
    let mut doc = page_of(1);
    let theme = crate::theme::find("tidepool").unwrap();
    crate::theme::apply(&mut doc, &[1], Choice::Theme(theme, 0), Base::Root).unwrap();
    let page = page_mut(&mut doc, 1).unwrap();
    let model = page.shapes.get(0).map(Arc::clone).unwrap();
    let mut shapes: Vec<Arc<Shape>> = vec![Arc::clone(&model)];
    shapes.extend((2..=n).map(|id| {
        let mut copy = (*model).clone();
        copy.id = id;
        Arc::new(copy)
    }));
    page.shapes = ShapeList::from_vec(shapes);
    doc
}

#[test]
fn a_quick_style_makes_the_shapes_follow_the_theme_in_one_pass_and_walks_the_page_once() {
    let n = 20_000;
    let mut doc = themed_page_of(n);
    // Take the style links off every shape, so that the style has something to make them follow.
    for index in 0..n as usize {
        let shape = Arc::make_mut(page_mut(&mut doc, 1).unwrap().shapes.get_mut(index).unwrap());
        Arc::make_mut(&mut shape.sheet).links = diacraft_doc::Links::default();
    }
    let ids: Vec<u32> = (0..2_000).map(|k| 1 + k * 10).collect();
    reset_walks();
    reset_follow_work();
    let formatted = format_quick_style(&mut doc, 1, &ids, QuickStyle::at(1, 2).unwrap()).unwrap();
    let (nodes, finds) = walks();
    assert_eq!(formatted, ids.len(), "every shape named was written to");
    assert_eq!(finds, 0, "no shape is looked for from the top of the page");
    assert!(nodes <= n as usize, "one walk of the page to find the shapes named: {nodes} visits");
    let work = follow_work();
    assert_eq!((work.page_walks, work.shapes_planned), (0, ids.len() as u64), "{work:?}");
    // A shape named has the links; one not named has not.
    let links = |id: u32| doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert!(links(1).fill.is_some() && links(1).line.is_some() && links(1).text.is_some());
    assert!(links(2).fill.is_none(), "a shape the style was not put on is as it was");
}
