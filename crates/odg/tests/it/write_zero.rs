//! Zero-width elements in a paragraph the writer writes whole: a
//! bookmark, a reference mark, a soft page break or a field that the model holds as plain text stays in the file when its paragraph is written again, at the
//! offset it has in the model's text, with the spaces round it written so that they read back as the model's.
//!
//! Every vector of the specification is one case, a function of a *saver*, and the echo control runs each with a writer that gives the original back, which must
//! fail them all. Each case also reads what the writer saved with the crate's reader, and saves the same model again, which is no edit.

use diacraft_doc::TextPart;
use diacraft_odg::{SaveNote, WriteError, Written};

use crate::drawing::Drawing;
use crate::write_core::{Case, Open, Saver, open, open_bytes, part, real};
use crate::write_runs::{chars, chars_of, set_parts, set_row, shape, the_shape};

/// The text style `T1` of the files that already have one: bold.
const T1: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;

fn drawing(auto: &str, inner: &str) -> Drawing {
    Drawing::a4().auto(auto).page(&shape("x", "1cm", inner))
}

/// The paragraphs of the saved part: what is between `<draw:custom-shape ...>` and the geometry that follows.
fn text_of(saved: &Written) -> String {
    let after = part(&saved.bytes, "content.xml");
    let from = after.find("<draw:custom-shape").and_then(|k| after[k..].find('>').map(|e| k + e + 1)).unwrap_or_else(|| panic!("no shape: {after}"));
    let to = after.find("<draw:enhanced-geometry").unwrap_or_else(|| panic!("no geometry: {after}"));
    after[from..to].to_owned()
}

/// Saves the model, expecting a save.
fn saved(save: &Saver, o: &Open) -> Written {
    save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"))
}

/// What every case holds of a save of text with elements: the file reads, with the characters the model has, the same model saved again through the new index is no
/// edit and gives the same bytes, and so is a third.
fn holds(save: &Saver, o: &Open, w: &Written) {
    let back = open_bytes(w.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), chars_of(the_shape(&o.doc)), "the file reads as the model's characters");
    let again = save(&o.doc, &w.bytes, &w.index).unwrap_or_else(|e| panic!("a second save: {e}"));
    assert!(!again.edited && again.bytes == w.bytes, "a second save is no edit");
    let third = save(&o.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("a third save: {e}"));
    assert!(!third.edited && third.bytes == w.bytes, "a third save is no edit");
}

/// The words of a refusal.
fn refusal(result: Result<Written, WriteError>) -> String {
    match result {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another error: {other}"),
        Ok(_) => panic!("the save was not refused"),
    }
}

/// Makes `Character` row `ix` bold (`Style` 1).
fn bold(o: &mut Open, ix: u32) {
    set_row(o, "Character", ix, &[("Style", 1.0)]);
}

/// A mark between runs stands at the paragraph's top level, before the bold run: `Alpha `, the bookmark, then `beta` in a span.
fn case_14(save: &Saver) {
    let mut o = open(&drawing("", r#"<text:p>Alpha <text:bookmark text:name="B1"/>beta gamma</text:p>"#));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("Alpha "), TextPart::Cp(1), chars("beta"), TextPart::Cp(0), chars(" gamma")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p>Alpha <text:bookmark text:name="B1"/><text:span text:style-name="T1">beta</text:span> gamma</text:p>"#,
        "B1's offset, 6, falls between the plain run and the bold one: at the top level, not after `beta`"
    );
    // The paragraph written is read again for what it holds, and so is counted.
    assert_eq!(w.work.checked_zero, 1);
    holds(save, &o, &w);
}

/// A mark inside a run stays inside the run's span, which is not split: `gam`, the bookmark, `ma`, all in the bold span.
fn case_15(save: &Saver) {
    let inner = r#"<text:p><text:span text:style-name="T1">gam<text:bookmark text:name="B2"/>ma</text:span> delta</text:p>"#;
    let mut o = open(&drawing(T1, inner));
    // The reader gives the bold run row 0 (its own cells) and ` delta` row 1 (every cell); the italic row is one more.
    set_row(&mut o, "Character", 2, &[("Size", 0.1666667), ("Style", 2.0)]);
    set_parts(&mut o, vec![TextPart::Cp(0), chars("gamma"), TextPart::Cp(2), chars(" delta")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p><text:span text:style-name="T1">gam<text:bookmark text:name="B2"/>ma</text:span><text:span text:style-name="T2"> delta</text:span></text:p>"#,
        "offset 3 lies inside the bold run: the span is not split in two around B2"
    );
    holds(save, &o, &w);
}

/// A mark in removed text goes to p. `alpha beta gamma` becomes `alpha gamma`: the reference mark in the removed `beta ` maps to 6.
fn case_16(save: &Saver) {
    let mut o = open(&drawing("", r#"<text:p>alpha beta<text:reference-mark text:name="M1"/> gamma</text:p>"#));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("alpha "), TextPart::Cp(1), chars("gamma")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p>alpha <text:reference-mark text:name="M1"/><text:span text:style-name="T1">gamma</text:span></text:p>"#,
        "the old text has 16 characters and the new 11, p = 6 and s = 5, and offset 10 maps to 6"
    );
    holds(save, &o, &w);
}

/// A range whose start's paragraph goes. The start anchor goes to the start of the next paragraph kept, which is otherwise unchanged.
fn case_17(save: &Saver) {
    let inner = r#"<text:p>start <text:bookmark-start text:name="R2"/>here</text:p><text:p>end<text:bookmark-end text:name="R2"/> there</text:p>"#;
    let mut o = open(&drawing("", inner));
    set_parts(&mut o, vec![chars("end there")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p><text:bookmark-start text:name="R2"/>end<text:bookmark-end text:name="R2"/> there</text:p>"#,
        "one paragraph, with the start put at its start"
    );
    assert_eq!(w.work.checked_zero, 1, "the paragraph that kept its bytes is read back for the element put in it");
    holds(save, &o, &w);
}

/// Spaces. A mark after a space that a mark before it separates from the next space; and a plain space after a mark is one the reader would drop.
fn case_18(save: &Saver) {
    // `a b` with the mark after the space: `b` bold, the plain space stays plain before the mark.
    let mut o = open(&drawing("", r#"<text:p>a <text:bookmark text:name="B3"/> b</text:p>"#));
    assert_eq!(chars_of(the_shape(&o.doc)), "a b", "the model text is `a b`");
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("a "), TextPart::Cp(1), chars("b")]);
    let w = saved(save, &o);
    assert_eq!(text_of(&w), r#"<text:p>a <text:bookmark text:name="B3"/><text:span text:style-name="T1">b</text:span></text:p>"#);
    holds(save, &o, &w);
    // `a`, two spaces, `b`, the mark between the two spaces, `b` bold: the second space follows a space with only a mark between, so it is `text:s`.
    let mut o = open(&drawing("", r#"<text:p>a <text:bookmark text:name="B3"/><text:s/>b</text:p>"#));
    assert_eq!(chars_of(the_shape(&o.doc)), "a  b");
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("a  "), TextPart::Cp(1), chars("b")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p>a <text:bookmark text:name="B3"/><text:s/><text:span text:style-name="T1">b</text:span></text:p>"#,
        "a plain space after the bookmark reads back as one space"
    );
    holds(save, &o, &w);
}

/// A bulleted list style, for the random test's list items.
const LIST_L1: &str =
    r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"#;

const AUTHOR: &str = r#"<text:author-name text:fixed="true">Ann</text:author-name>"#;

/// A kept field keeps its bytes and the space after it is `text:s`; a field whose characters changed is written as the new characters, with a note.
fn case_19(save: &Saver) {
    let mut o = open(&drawing("", &format!("<text:p>by {AUTHOR} today</text:p>")));
    assert_eq!(chars_of(the_shape(&o.doc)), "by Ann today");
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("by Ann "), TextPart::Cp(1), chars("today")]);
    let w = saved(save, &o);
    assert_eq!(text_of(&w), format!(r#"<text:p>by {AUTHOR}<text:s/><text:span text:style-name="T1">today</text:span></text:p>"#));
    assert!(w.notes.is_empty(), "{:?}", w.notes);
    holds(save, &o, &w);
    // `Ann` retyped as `Bob`: the characters changed, so the paragraph is written without the element, and the save says so once, naming it.
    let mut o = open(&drawing("", &format!("<text:p>by {AUTHOR} today</text:p>")));
    set_parts(&mut o, vec![chars("by Bob today")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        "<text:p>by Bob today</text:p>",
        "writing `Bob` inside the old field would show `Ann` again where the field is refreshed"
    );
    assert_eq!(w.notes.len(), 1, "{:?}", w.notes);
    assert!(matches!(w.notes[0], SaveNote::FieldWritten { element: "author-name", .. }), "{:?}", w.notes);
    assert!(w.notes[0].to_string().contains("text:author-name"), "{}", w.notes[0]);
    holds(save, &o, &w);
}

/// A range that collapses. Both anchors map to 4 and keep their order: the start, then the end, before the bold run.
fn case_20(save: &Saver) {
    let inner = r#"<text:p>one <text:bookmark-start text:name="R1"/>two<text:bookmark-end text:name="R1"/> four</text:p>"#;
    let mut o = open(&drawing("", inner));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("one "), TextPart::Cp(1), chars("four")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p>one <text:bookmark-start text:name="R1"/><text:bookmark-end text:name="R1"/><text:span text:style-name="T1">four</text:span></text:p>"#,
        "both map to 4 and keep their order: the end is not written before the start"
    );
    holds(save, &o, &w);
}

/// The prefix first. `one two<mark> three` becomes `one three`: p = 5 (`one t`), s = 4, the mark at 7 goes to 5, inside the bold span.
fn case_21(save: &Saver) {
    let mut o = open(&drawing("", r#"<text:p>one two<text:bookmark text:name="B4"/> three</text:p>"#));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("one "), TextPart::Cp(1), chars("three")]);
    let w = saved(save, &o);
    assert_eq!(
        text_of(&w),
        r#"<text:p>one <text:span text:style-name="T1">t<text:bookmark text:name="B4"/>hree</text:span></text:p>"#,
        "the suffix first (s = 6, then p = 3) would put it at offset 3"
    );
    holds(save, &o, &w);
}

const VECTORS: [Case; 8] = [
    ("case 14", case_14),
    ("case 15", case_15),
    ("case 16", case_16),
    ("case 17", case_17),
    ("case 18", case_18),
    ("case 19", case_19),
    ("case 20", case_20),
    ("case 21", case_21),
];

#[test]
fn a_mark_between_two_runs_of_different_rows_stands_at_the_top_level() {
    case_14(&real);
}

#[test]
fn a_mark_inside_a_run_stays_inside_its_span_which_is_not_split() {
    case_15(&real);
}

#[test]
fn a_mark_in_text_the_edit_removed_goes_to_the_end_of_the_common_prefix() {
    case_16(&real);
}

#[test]
fn a_range_whose_start_s_paragraph_goes_has_its_start_put_at_the_next_paragraph_kept() {
    case_17(&real);
}

#[test]
fn a_space_that_a_plain_one_would_lose_after_a_mark_is_written_as_text_s() {
    case_18(&real);
}

#[test]
fn a_kept_field_keeps_its_bytes_and_a_changed_one_is_plain_text_with_one_note() {
    case_19(&real);
}

#[test]
fn a_range_whose_text_went_collapses_with_both_ends_at_one_place_in_their_order() {
    case_20(&real);
}

#[test]
fn the_common_prefix_is_taken_before_the_common_suffix() {
    case_21(&real);
}

#[test]
fn the_echo_control_fails_every_vector() {
    crate::write_core::echo_control(&VECTORS);
}

/// Saves `inner` as the content of the shape's text after the edit `f`, and gives the text of the saved shape.
fn result(auto: &str, inner: &str, f: impl FnOnce(&mut Open)) -> (Open, Written, String) {
    let mut o = open(&drawing(auto, inner));
    f(&mut o);
    let w = saved(&real, &o);
    let t = text_of(&w);
    (o, w, t)
}

/// Text made of `parts`, whatever the file's rows are.
fn text(parts: Vec<TextPart>) -> impl FnOnce(&mut Open) {
    move |o| set_parts(o, parts)
}

#[test]
fn a_mark_at_the_start_or_the_end_of_the_paragraph_stands_at_the_top_level_outside_the_span() {
    let inner = r#"<text:p><text:bookmark-start text:name="s"/>one two<text:bookmark-end text:name="s"/></text:p>"#;
    let (o, w, t) = result("", inner, |o| {
        bold(o, 1);
        set_parts(o, vec![TextPart::Cp(1), chars("one two")]);
    });
    assert_eq!(
        t,
        r#"<text:p><text:bookmark-start text:name="s"/><text:span text:style-name="T1">one two</text:span><text:bookmark-end text:name="s"/></text:p>"#
    );
    holds(&real, &o, &w);
}

#[test]
fn elements_that_land_on_one_offset_are_written_in_the_order_they_stood_in_and_a_reference_mark_and_a_soft_page_break_are_kept() {
    let marks =
        r#"<text:bookmark text:name="1"/><text:reference-mark-start text:name="2"/><text:soft-page-break/><text:reference-mark-end text:name="2"/>"#;
    let (o, w, t) = result("", &format!("<text:p>ab{marks}cd</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("ab"), TextPart::Cp(1), chars("cd")]);
    });
    assert_eq!(t, format!(r#"<text:p>ab{marks}<text:span text:style-name="T1">cd</text:span></text:p>"#));
    holds(&real, &o, &w);
    // Inside a run they stay together in their order, and the span is not split.
    let (o, w, t) = result(T1, &format!(r#"<text:p><text:span text:style-name="T1">ab{marks}cd</text:span>ef</text:p>"#), |o| {
        set_row(o, "Character", 2, &[("Size", 0.1666667), ("Style", 2.0)]);
        set_parts(o, vec![TextPart::Cp(0), chars("abcd"), TextPart::Cp(2), chars("ef")]);
    });
    assert_eq!(
        t,
        format!(r#"<text:p><text:span text:style-name="T1">ab{marks}cd</text:span><text:span text:style-name="T2">ef</text:span></text:p>"#)
    );
    holds(&real, &o, &w);
}

#[test]
fn a_paragraph_written_whole_beside_ones_the_plan_keeps_leaves_theirs_as_they_are_and_puts_its_own_back() {
    let inner = r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>two<text:bookmark text:name="B"/></text:p>"#;
    let (o, w, t) = result("", inner, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(
        t,
        r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p><text:span text:style-name="T1">two</text:span><text:bookmark text:name="B"/></text:p>"#
    );
    holds(&real, &o, &w);
}

#[test]
fn two_paragraphs_made_one_carry_their_marks_by_the_block_s_prefix_and_suffix() {
    let inner = r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>two<text:bookmark text:name="B"/></text:p>"#;
    // `one\ntwo` (7) to `onetwo` (6), the second half bold, so the first paragraph is written whole: p = 3 and s = 3; A at 3 stays, between the two runs; B at 7
    // moves to 6, the end.
    let (o, w, t) = result("", inner, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(
        t,
        r#"<text:p>one<text:bookmark text:name="A"/><text:span text:style-name="T1">two</text:span><text:bookmark text:name="B"/></text:p>"#
    );
    holds(&real, &o, &w);
}

#[test]
fn an_edit_in_place_keeps_every_element_where_it_stands_and_the_paragraph_taken_out_gives_its_own_to_the_end_of_it() {
    // `one`, a bookmark; the line feed taken out and `two` joined: the characters are put in the raw text before the bookmark (R10 item 2), and the second
    // paragraph's mark goes to the end of the last paragraph kept, which is that one.
    let inner = r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>two<text:bookmark text:name="B"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("onetwo")]));
    assert_eq!(t, r#"<text:p>onetwo<text:bookmark text:name="A"/><text:bookmark text:name="B"/></text:p>"#);
    holds(&real, &o, &w);
}

#[test]
fn a_paragraph_taken_out_with_a_mark_and_no_paragraph_kept_after_it_gives_it_to_the_end_of_the_last_one_kept() {
    let inner = r#"<text:p>one</text:p><text:p>two<text:bookmark text:name="A"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("one")]));
    assert_eq!(t, r#"<text:p>one<text:bookmark text:name="A"/></text:p>"#);
    holds(&real, &o, &w);
}

#[test]
fn a_paragraph_put_in_takes_no_element_and_the_paragraphs_kept_keep_theirs() {
    let inner = r#"<text:p>one<text:bookmark text:name="A"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("one\nnew")]));
    assert_eq!(
        (w.work.checked_inserted, w.work.checked_zero),
        (1, 0),
        "the paragraph put in is read back, and no paragraph is read for an element when the edit moved none"
    );
    assert_eq!(t, r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>new</text:p>"#);
    holds(&real, &o, &w);
}

#[test]
fn an_element_goes_to_a_paragraph_the_plan_keeps_and_an_empty_one_is_opened_for_it() {
    // The kept paragraph is written empty: it is opened.
    let inner = r#"<text:p>gone<text:bookmark text:name="A"/></text:p><text:p/>"#;
    let (o, w, t) = result("", inner, text(vec![chars("")]));
    assert_eq!(t, r#"<text:p><text:bookmark text:name="A"/></text:p>"#);
    holds(&real, &o, &w);
    // At the start of a kept paragraph that has text and a mark of its own: the one put in is first.
    let inner = r#"<text:p>gone<text:bookmark text:name="A"/></text:p><text:p>kept<text:bookmark text:name="B"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("kept")]));
    assert_eq!(t, r#"<text:p><text:bookmark text:name="A"/>kept<text:bookmark text:name="B"/></text:p>"#);
    holds(&real, &o, &w);
}

/// What lands at the end of a paragraph the plan keeps that is written empty: it is opened for it, and the paragraph is read back for it.
#[test]
fn an_element_that_lands_at_the_end_of_an_empty_paragraph_opens_it() {
    let inner = r#"<text:p/><text:p>gone<text:bookmark text:name="A"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("")]));
    assert_eq!(t, r#"<text:p><text:bookmark text:name="A"/></text:p>"#);
    assert_eq!(w.work.checked_zero, 1);
    holds(&real, &o, &w);
}

#[test]
fn a_paragraph_in_a_list_item_is_written_whole_with_its_marks() {
    let list =
        r#"<text:list text:style-name="L1"><text:list-item><text:p>one<text:bookmark text:name="A"/> two</text:p></text:list-item></text:list>"#;
    let styles = r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"#;
    let (o, w, t) = result(styles, list, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one "), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(
        t,
        r#"<text:list text:style-name="L1"><text:list-item><text:p>one<text:bookmark text:name="A"/> <text:span text:style-name="T1">two</text:span></text:p></text:list-item></text:list>"#
    );
    holds(&real, &o, &w);
}

#[test]
fn a_field_keeps_its_bytes_wherever_its_run_is_and_the_text_around_it_is_written_as_the_model_has_it() {
    // A field at the start, a field inside a span, a field at the end.
    let title = "<text:title>A &amp; B</text:title>";
    let (o, w, t) = result("", &format!("<text:p>{title} and {AUTHOR}</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("A & B and "), TextPart::Cp(1), chars("Ann")]);
    });
    assert_eq!(chars_of(the_shape(&o.doc)), "A & B and Ann");
    assert_eq!(t, format!(r#"<text:p>{title}<text:s/>and <text:span text:style-name="T1">{AUTHOR}</text:span></text:p>"#));
    holds(&real, &o, &w);
}

/// A character typed after a field, in the text that follows it: an edit in place, so the text after the field keeps its plain space (the paragraph written whole
/// would write the first space after a field as `text:s`).
#[test]
fn an_edit_in_the_text_after_a_field_is_written_in_place_and_keeps_its_bytes() {
    let (o, w, t) = result("", &format!("<text:p>by {AUTHOR} today</text:p>"), text(vec![chars("by Ann todayX")]));
    assert_eq!(t, format!("<text:p>by {AUTHOR} todayX</text:p>"));
    assert!(w.notes.is_empty());
    holds(&real, &o, &w);
}

#[test]
fn text_put_before_a_field_moves_it_with_the_suffix() {
    let (o, w, t) = result("", &format!("<text:p>by {AUTHOR} today</text:p>"), text(vec![chars("by new Ann today")]));
    assert_eq!(t, format!("<text:p>by new {AUTHOR}<text:s/>today</text:p>"));
    assert!(w.notes.is_empty(), "{:?}", w.notes);
    holds(&real, &o, &w);
}

#[test]
fn a_field_with_no_characters_is_kept_as_an_element_and_a_space_after_it_is_text_s() {
    let (o, w, t) = result("", "<text:p>a<text:title/> b</text:p>", |o| {
        bold(o, 1);
        set_parts(o, vec![chars("a "), TextPart::Cp(1), chars("b")]);
    });
    assert_eq!(t, r#"<text:p>a<text:title/><text:s/><text:span text:style-name="T1">b</text:span></text:p>"#);
    holds(&real, &o, &w);
}

/// A soft page break between paragraphs (R10 item 1 and 5) stays with the paragraph after it: a paragraph put in before that paragraph goes before the break, one
/// written whole or taken out leaves the break where it stands (which is where R10 puts it: at the start of the paragraph that is now there).
#[test]
fn a_soft_page_break_between_paragraphs_stays_with_the_paragraph_after_it() {
    let soft = "<text:soft-page-break/>";
    // A paragraph put in at the start, before one that has a break before it: it goes before the break.
    let (o, w, t) = result("", &format!("{soft}<text:p>one</text:p>"), text(vec![chars("new\none")]));
    assert_eq!(t, format!("<text:p>new</text:p>{soft}<text:p>one</text:p>"));
    holds(&real, &o, &w);
    // The paragraph after the break written whole: the break is where it was.
    let (o, w, t) = result("", &format!("<text:p>one</text:p>{soft}<text:p>two</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(t, format!(r#"<text:p>one</text:p>{soft}<text:p><text:span text:style-name="T1">two</text:span></text:p>"#));
    holds(&real, &o, &w);
    // The paragraph after the break taken out: the break stands before the next paragraph.
    let (o, w, t) = result("", &format!("<text:p>one</text:p>{soft}<text:p>two</text:p><text:p>three</text:p>"), text(vec![chars("one\nthree")]));
    assert_eq!(t, format!("<text:p>one</text:p>{soft}<text:p>three</text:p>"));
    holds(&real, &o, &w);
    // In a list item, before its paragraph: the paragraph is written whole inside the item.
    let styles = r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"#;
    let list = format!(r#"<text:list text:style-name="L1"><text:list-item>{soft}<text:p>one two</text:p></text:list-item></text:list>"#);
    let (o, w, t) = result(styles, &list, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one "), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(
        t,
        format!(
            r#"<text:list text:style-name="L1"><text:list-item>{soft}<text:p>one<text:s/><text:span text:style-name="T1">two</text:span></text:p></text:list-item></text:list>"#
        )
    );
    holds(&real, &o, &w);
}

/// Two faults of the retext that the random test of R10 found in the code the elements share (the first read back as a refusal of the bytes made, the second too;
/// neither dropped a character unsaid): an edit spliced into one run of raw text left the white space after it read another way when the edit took the text that
/// stood before it (the plain space after a bookmark became the paragraph's leading space and was dropped), and a `text:span` kept as its bytes began with a plain space
/// that the text before it no longer made one space of.
#[test]
fn an_edit_that_takes_the_text_before_a_plain_space_is_written_whole_and_a_span_that_starts_with_a_space_is_not_kept() {
    // `a`, a bookmark, a plain space and a counted one: the model is `a` and three spaces. `a` taken out: the spaces are the paragraph's leading ones, so each is
    // counted, and the bookmark stays first.
    let inner = r#"<text:p>a<text:bookmark text:name="B"/> <text:s text:c="2"/></text:p>"#;
    let (o, w, t) = result("", inner, text(vec![chars("   ")]));
    assert_eq!(t, r#"<text:p><text:bookmark text:name="B"/><text:s text:c="3"/></text:p>"#);
    holds(&real, &o, &w);
    // The same with no element: a span that starts with a space, after a character that goes.
    let inner = r#"<text:p>a<text:span text:style-name="T1"> <text:s text:c="1"/></text:span></text:p>"#;
    let (o, w, t) = result(T1, inner, text(vec![chars("   ")]));
    assert!(!t.contains("<text:span text:style-name=\"T1\"> <text:s"), "the span is not copied: {t}");
    assert_eq!(chars_of(the_shape(&open_bytes(w.bytes.clone()).doc)), "   ");
    holds(&real, &o, &w);
}

/// How an element is read from the file (the forms a file may hold it in): a mark after the white space the paragraph's end takes off is at the end; a `>` in an
/// attribute's value does not end the tag; an element written as a start tag and its end tag at once is kept as those bytes; an element that holds something is no element
/// the writer maps; and a heading is a paragraph.
#[test]
fn an_element_is_read_in_every_form_a_file_holds_it_in() {
    // `a` and a collapsed space and a bookmark: the model is `a`, the mark is at its end (1, not 2), so it stands after the bold span.
    let (o, w, t) = result("", r#"<text:p>a <text:bookmark text:name="B"/></text:p>"#, |o| {
        bold(o, 1);
        set_parts(o, vec![TextPart::Cp(1), chars("a")]);
    });
    assert_eq!(t, r#"<text:p><text:span text:style-name="T1">a</text:span><text:bookmark text:name="B"/></text:p>"#);
    holds(&real, &o, &w);
    // A value with a `>`, and the long form of an empty element.
    let marks = r#"<text:bookmark text:name="a&gt;b" text:note='x>y'/><text:reference-mark text:name="r"></text:reference-mark>"#;
    let (o, w, t) = result("", &format!("<text:p>ab{marks}cd</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("ab"), TextPart::Cp(1), chars("cd")]);
    });
    assert_eq!(t, format!(r#"<text:p>ab{marks}<text:span text:style-name="T1">cd</text:span></text:p>"#));
    holds(&real, &o, &w);
    // A heading is a paragraph, and takes its elements back too.
    let (o, w, t) = result("", r#"<text:h text:outline-level="1">one<text:bookmark text:name="H"/> two</text:h>"#, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one "), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(t, r#"<text:h text:outline-level="1">one<text:bookmark text:name="H"/> <text:span text:style-name="T1">two</text:span></text:h>"#);
    holds(&real, &o, &w);
    // A mark that holds something, and a field that holds an element, are no elements the writer maps: refused where the paragraph is written whole.
    for body in [
        r#"<text:p>a<text:bookmark text:name="B">x</text:bookmark>b</text:p>"#,
        "<text:p>a<text:title><text:span>x</text:span></text:title>b</text:p>",
    ] {
        let mut o = open(&drawing("", body));
        bold(&mut o, 1);
        set_parts(&mut o, vec![chars("a"), TextPart::Cp(1), chars("xb")]);
        let words = refusal(real(&o.doc, &o.bytes, &o.index));
        assert!(words.contains("does not map"), "{body}: {words}");
    }
}

/// A zero-width element is not kept in a part where it is not read: the shape's paragraphs only. And an edit that leaves the paragraph's text alone keeps the file
/// as it is: a second paragraph's own mark is not moved by an edit of the first.
#[test]
fn an_edit_of_one_paragraph_leaves_the_marks_of_the_others_in_their_paragraphs() {
    let inner = r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>two<text:bookmark text:name="B"/></text:p><text:p>three<text:bookmark text:name="C"/></text:p>"#;
    let (o, w, t) = result("", inner, |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\ntwo\n"), TextPart::Cp(1), chars("three")]);
    });
    assert_eq!(
        t,
        r#"<text:p>one<text:bookmark text:name="A"/></text:p><text:p>two<text:bookmark text:name="B"/></text:p><text:p><text:span text:style-name="T1">three</text:span><text:bookmark text:name="C"/></text:p>"#
    );
    holds(&real, &o, &w);
}

/// The same in a text frame: a soft page break that is a child of the `draw:text-box` stays with the paragraph after it.
#[test]
fn a_soft_page_break_in_a_text_box_stays_with_the_paragraph_after_it() {
    let soft = "<text:soft-page-break/>";
    let d = Drawing::a4().auto(T1).page(&lo_frame(&format!("{soft}<text:p>one</text:p>")));
    let mut o = open(&d);
    set_parts(&mut o, vec![chars("new\none")]);
    let w = saved(&real, &o);
    let content = part(&w.bytes, "content.xml");
    assert!(content.contains(&format!("<draw:text-box><text:p>new</text:p>{soft}<text:p>one</text:p></draw:text-box>")), "{content}");
    holds(&real, &o, &w);
}

/// A paragraph taken out that holds an element the writer cannot read whole is refused when it holds an element that would have to go elsewhere, whichever kind
/// (a mark or a field), and goes with its content when it holds none.
#[test]
fn a_paragraph_taken_out_with_an_unmapped_element_is_refused_only_when_an_element_would_be_lost() {
    let link = r#"<text:a xlink:href="http://example.com">l</text:a>"#;
    for (what, content, refused) in [
        ("a link and a bookmark", format!(r#"x{link}<text:bookmark text:name="B"/>"#), true),
        ("a link and a field", format!("x{link}<text:title>T</text:title>"), true),
        ("a link and a soft page break", format!("x{link}<text:soft-page-break/>"), true),
        ("a link alone", format!("x{link}"), false),
        ("a link and a span", format!(r#"x{link}<text:span text:style-name="T1">y</text:span>"#), false),
    ] {
        let mut o = open(&drawing(T1, &format!("<text:p>keep</text:p><text:p>{content}</text:p>")));
        set_parts(&mut o, vec![chars("keep")]);
        let result = real(&o.doc, &o.bytes, &o.index);
        if refused {
            assert!(refusal(result).contains("does not map"), "{what}");
        } else {
            let w = result.unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(text_of(&w), "<text:p>keep</text:p>", "{what}");
        }
    }
}

/// A soft page break before a list stays with the list's first paragraph: a paragraph put in before it, outside the list, goes before the break.
#[test]
fn a_paragraph_put_in_before_a_list_goes_before_the_soft_page_break_that_stands_before_it() {
    let soft = "<text:soft-page-break/>";
    let list = format!(r#"{soft}<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item></text:list>"#);
    let mut o = open(&drawing(LIST_L1, &list));
    // The model: `one` has a list's paragraph row (row 0, which the reader opens the text with); the new paragraph before it is given a row that holds nothing.
    let held = the_shape(&o.doc).text.as_ref().unwrap().parts.iter().cloned();
    let parts: Vec<TextPart> = [TextPart::Pp(7), chars("new\n")].into_iter().chain(held).collect();
    set_parts(&mut o, parts);
    let w = saved(&real, &o);
    assert_eq!(
        text_of(&w),
        format!(r#"<text:p>new</text:p>{soft}<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item></text:list>"#)
    );
    holds(&real, &o, &w);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// A soft page break between paragraphs follows the block mapping (items 1, 3 and 5), as a bookmark does:
// it is anchored at the start of the paragraph after it, carried over with the prefix and suffix of its block, and written where it lands: between the paragraphs
// when it lands on a paragraph's start (at the start of the paragraph's list item when that is a list item's), else inside the paragraph, at the offset.

const SOFT: &str = "<text:soft-page-break/>";

/// R10 item 3: a block whose new side holds no paragraph, and that no paragraph the plan keeps follows, sends its anchors to the end of the last paragraph kept. The
/// break stood at the start of the last paragraph, which is taken out: it goes inside the end of the paragraph before it, in the order the breaks stood in.
#[test]
fn item_3_a_soft_page_break_before_the_last_paragraph_taken_out_goes_to_the_end_of_the_paragraph_before() {
    let (o, w, t) = result("", &format!("<text:p>one</text:p>{SOFT}<text:p>two</text:p>"), text(vec![chars("one")]));
    assert_eq!(t, format!("<text:p>one{SOFT}</text:p>"));
    holds(&real, &o, &w);
    // Two breaks before it, and a paragraph with a bookmark before them: the breaks follow what the kept paragraph already holds at its end.
    let inner = format!(r#"<text:p>one<text:bookmark text:name="B"/></text:p>{SOFT}{SOFT}<text:p>two</text:p><text:p>three</text:p>"#);
    let (o, w, t) = result("", &inner, text(vec![chars("one")]));
    assert_eq!(t, format!(r#"<text:p>one<text:bookmark text:name="B"/>{SOFT}{SOFT}</text:p>"#));
    holds(&real, &o, &w);
    // A break before a paragraph that goes with the paragraph after it, and a break before that one: all of them go to the end of the paragraph kept.
    let inner = format!("<text:p>one</text:p>{SOFT}<text:p>two</text:p>{SOFT}<text:p>three</text:p>");
    let (o, w, t) = result("", &inner, text(vec![chars("one")]));
    assert_eq!(t, format!("<text:p>one{SOFT}{SOFT}</text:p>"));
    holds(&real, &o, &w);
    // The same in a text frame, where the break is a child of the `draw:text-box`.
    let d = Drawing::a4().auto(T1).page(&lo_frame(&format!("<text:p>one</text:p>{SOFT}<text:p>two</text:p>")));
    let mut o = open(&d);
    set_parts(&mut o, vec![chars("one")]);
    let w = saved(&real, &o);
    let content = part(&w.bytes, "content.xml");
    assert!(content.contains(&format!("<draw:text-box><text:p>one{SOFT}</text:p></draw:text-box>")), "{content}");
    holds(&real, &o, &w);
}

/// R10 item 3: the break stood at the start of the paragraph after it, an offset the block's text moves like any other: two paragraphs made one, the break at the
/// start of the second (offset 4 of `abc` and `def` joined by a line feed) is past the common prefix and at the start of the common suffix, so it moves by the change of
/// length, to offset 3 of `abXdef`: inside the paragraph, between `abX` and `def`.
#[test]
fn item_3_a_soft_page_break_at_an_offset_the_blocks_text_moves_lands_inside_the_paragraph() {
    let (o, w, t) = result("", &format!("<text:p>one</text:p><text:p>abc</text:p>{SOFT}<text:p>def</text:p>"), text(vec![chars("one\nabXdef")]));
    assert_eq!(t, format!("<text:p>one</text:p><text:p>abX{SOFT}def</text:p>"));
    holds(&real, &o, &w);
    // The break inside a run's span: at an offset inside a bold run it is written inside the span (item 5), the span not split but for it.
    let (o, w, t) = result("", &format!("<text:p>one</text:p><text:p>abc</text:p>{SOFT}<text:p>def</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("abXdef")]);
    });
    assert_eq!(t, format!(r#"<text:p>one</text:p><text:p><text:span text:style-name="T1">abX{SOFT}def</text:span></text:p>"#));
    holds(&real, &o, &w);
}

/// R10 items 3 and 5: the break stood at the start of a paragraph written whole, which has a paragraph put in before it. The prefix of the block's two texts is empty
/// (offset 0 stays 0): the break lands on the start of the first paragraph of the new side, the one put in, and is written between the paragraphs, before it.
#[test]
fn items_3_and_5_a_soft_page_break_at_the_start_of_a_block_stays_at_the_start_of_its_new_side() {
    let (o, w, t) = result("", &format!("<text:p>one</text:p>{SOFT}<text:p>two</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\nzzzzzzzz\n"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(t, format!(r#"<text:p>one</text:p>{SOFT}<text:p>zzzzzzzz</text:p><text:p><text:span text:style-name="T1">two</text:span></text:p>"#));
    holds(&real, &o, &w);
}

/// R10 item 5: a break that lands on the start of a paragraph written whole stays between the paragraphs, as the file had it, **white space and all** (the bytes of the
/// break are copied from where it stood), and one that lands on the start of a list item's paragraph is written at the start of the list item, since a list holds no
/// soft page break between its items.
#[test]
fn item_5_a_soft_page_break_that_lands_on_a_paragraph_s_start_is_written_between_the_paragraphs_or_at_the_start_of_its_list_item() {
    let (o, w, t) = result("", &format!("<text:p>one</text:p>\n  {SOFT}\n  <text:p>two</text:p>"), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(t, format!("<text:p>one</text:p>\n  {SOFT}\n  <text:p><text:span text:style-name=\"T1\">two</text:span></text:p>"));
    holds(&real, &o, &w);
    // A bookmark at the start of the paragraph the break lands on stays inside the paragraph: only the break goes between the paragraphs.
    let (o, w, t) = result("", &format!(r#"<text:p>one</text:p>{SOFT}<text:p><text:bookmark text:name="B"/>two</text:p>"#), |o| {
        bold(o, 1);
        set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("two")]);
    });
    assert_eq!(
        t,
        format!(r#"<text:p>one</text:p>{SOFT}<text:p><text:bookmark text:name="B"/><text:span text:style-name="T1">two</text:span></text:p>"#)
    );
    holds(&real, &o, &w);
    // A break that lands inside the paragraph and one that lands on its start: the one is a mark of the paragraph, the other stands between the paragraphs.
    let inner = format!("<text:p>one</text:p>{SOFT}<text:p>abc</text:p>{SOFT}<text:p>def</text:p>");
    let (o, w, t) = result("", &inner, text(vec![chars("one\nabXdef")]));
    assert_eq!(t, format!("<text:p>one</text:p>{SOFT}<text:p>abX{SOFT}def</text:p>"));
    holds(&real, &o, &w);
    // Before a list, and the list's paragraph written whole: the break goes into the item.
    let list = format!(r#"{SOFT}<text:list text:style-name="L1"><text:list-item><text:p>one two</text:p></text:list-item></text:list>"#);
    let mut o = open(&drawing(LIST_L1, &list));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("one "), TextPart::Cp(1), chars("two")]);
    let w = saved(&real, &o);
    assert_eq!(
        text_of(&w),
        format!(
            r#"<text:list text:style-name="L1"><text:list-item>{SOFT}<text:p>one<text:s/><text:span text:style-name="T1">two</text:span></text:p></text:list-item></text:list>"#
        )
    );
    holds(&real, &o, &w);
}

/// R10 item 3: a block whose new side holds no paragraph sends its anchors to the start of the next paragraph the plan keeps: a break before a paragraph taken out
/// stands before the paragraph that follows it, between the paragraphs, however many breaks stood there; and a paragraph that is a list item's gets it at the start of
/// its item.
#[test]
fn item_3_the_breaks_of_paragraphs_taken_out_go_to_the_start_of_the_next_paragraph_kept() {
    let inner = format!("<text:p>one</text:p>{SOFT}{SOFT}<text:p>two</text:p><text:p>three</text:p>");
    let (o, w, t) = result("", &inner, text(vec![chars("one\nthree")]));
    assert_eq!(t, format!("<text:p>one</text:p>{SOFT}{SOFT}<text:p>three</text:p>"));
    holds(&real, &o, &w);
    // Two breaks before a paragraph that is kept: a paragraph put in before it goes before both (R10 item 1: the breaks are at the paragraph's start).
    let (o, w, t) = result("", &format!("{SOFT}{SOFT}<text:p>one</text:p>"), text(vec![chars("new\none")]));
    assert_eq!(t, format!("<text:p>new</text:p>{SOFT}{SOFT}<text:p>one</text:p>"));
    holds(&real, &o, &w);
    // The break of each paragraph taken out, and the paragraph kept has one of its own: the older ones first.
    let inner = format!("<text:p>one</text:p>{SOFT}<text:p>two</text:p>{SOFT}<text:p>three</text:p>{SOFT}<text:p>four</text:p>");
    let (o, w, t) = result("", &inner, text(vec![chars("one\nfour")]));
    assert_eq!(t, format!("<text:p>one</text:p>{SOFT}{SOFT}{SOFT}<text:p>four</text:p>"));
    holds(&real, &o, &w);
}

#[test]
fn what_does_not_place_is_refused_in_words() {
    // A change mark and an index mark are not placed.
    for element in [r#"<text:change-start text:change-id="c"/>"#, r#"<text:alphabetical-index-mark text:string-value="x"/>"#] {
        let (_, r) = {
            let mut o = open(&drawing("", &format!("<text:p>a{element}b</text:p>")));
            bold(&mut o, 1);
            set_parts(&mut o, vec![chars("a"), TextPart::Cp(1), chars("b")]);
            let r = real(&o.doc, &o.bytes, &o.index);
            (o, r)
        };
        let words = refusal(r);
        assert!(words.contains("does not map"), "{element}: {words}");
    }
    // A link.
    let mut o = open(&drawing("", r#"<text:p>a <text:a xlink:href="http://example.com">b</text:a> c</text:p>"#));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("a b "), TextPart::Cp(1), chars("c")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("link") && words.contains("Q4"), "{words}");
    // A format that cuts through a field's characters.
    let mut o = open(&drawing("", &format!("<text:p>by {AUTHOR} today</text:p>")));
    bold(&mut o, 1);
    set_parts(&mut o, vec![chars("by A"), TextPart::Cp(1), chars("nn today")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("cuts through the characters of a field"), "{words}");
}

#[test]
fn a_paragraph_that_holds_no_element_is_written_as_it_was_and_no_check_looks_for_one() {
    let (o, w, t) = result("", "<text:p>plain text</text:p>", |o| {
        bold(o, 1);
        set_parts(o, vec![chars("plain "), TextPart::Cp(1), chars("text")]);
    });
    assert_eq!(t, r#"<text:p>plain<text:s/><text:span text:style-name="T1">text</text:span></text:p>"#);
    holds(&real, &o, &w);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Random paragraphs with elements, random edits: what the writer gives holds the properties, or it is refused in words.

use proptest::prelude::*;

/// One thing in a paragraph of the file.
#[derive(Clone, Debug)]
enum Item {
    Word(String),
    /// A run of this many spaces: a plain one and a `text:s` for the rest.
    Spaces(usize),
    Tab,
    /// A zero-width element of this kind (0 to 5: bookmark, its start, its end, reference mark, its start, soft page break).
    Mark(u8),
    /// A field of this text.
    Field(String),
    /// A bold span round the items.
    Span(Vec<Item>),
}

fn item() -> impl Strategy<Value = Item> {
    let leaf = prop_oneof![
        3 => "[a-e]{1,4}".prop_map(Item::Word),
        2 => (1usize..4).prop_map(Item::Spaces),
        1 => Just(Item::Tab),
        4 => (0u8..6).prop_map(Item::Mark),
        1 => "[a-e]{1,3}".prop_map(Item::Field),
    ];
    prop_oneof![
        8 => leaf.clone(),
        2 => proptest::collection::vec(leaf, 1..4).prop_map(Item::Span),
    ]
}

/// The markup of the items, the marks named from `next`, and the names in the order written.
fn render(items: &[Item], next: &mut usize, marks: &mut Vec<String>, fields: &mut Vec<String>) -> String {
    let mut out = String::new();
    for it in items {
        match it {
            Item::Word(w) => out.push_str(w),
            Item::Spaces(n) => {
                out.push(' ');
                if *n > 1 {
                    out.push_str(&format!("<text:s text:c=\"{}\"/>", n - 1));
                }
            }
            Item::Tab => out.push_str("<text:tab/>"),
            Item::Mark(kind) => {
                *next += 1;
                let name = format!("M{next}");
                let element = match kind {
                    0 => format!("<text:bookmark text:name=\"{name}\"/>"),
                    1 => format!("<text:bookmark-start text:name=\"{name}\"/>"),
                    2 => format!("<text:bookmark-end text:name=\"{name}\"/>"),
                    3 => format!("<text:reference-mark text:name=\"{name}\"/>"),
                    4 => format!("<text:reference-mark-start text:name=\"{name}\"/>"),
                    _ => "<text:soft-page-break/>".to_owned(),
                };
                out.push_str(&element);
                marks.push(element);
            }
            Item::Field(t) => {
                *next += 1;
                let element = format!("<text:title text:fixed=\"true\">{t}</text:title>");
                out.push_str(&element);
                fields.push(element);
            }
            Item::Span(inner) => out.push_str(&format!("<text:span text:style-name=\"T1\">{}</text:span>", render(inner, next, marks, fields))),
        }
    }
    out
}

/// An edit of the model's text: a replacement of the characters from `from` to `to` (shares of the text's length, in 0 to 100) by `with`, and an italic stretch.
#[derive(Clone, Debug)]
struct Edit {
    from: usize,
    to: usize,
    with: String,
    italic: Option<(usize, usize)>,
}

fn edit() -> impl Strategy<Value = Edit> {
    (0usize..=100, 0usize..=100, "[a-e \n]{0,4}", proptest::option::of((0usize..=100, 0usize..=100))).prop_map(|(from, to, with, italic)| Edit {
        from,
        to,
        with,
        italic,
    })
}

fn share(text: &[char], at: usize) -> usize {
    text.len() * at / 100
}

proptest! {
    // A longer run: `ODG_ZW_CASES=50000` (the default is the 1,500 the gate runs; `PROPTEST_RNG_SEED` makes another run).
    #![proptest_config(ProptestConfig { cases: std::env::var("ODG_ZW_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1500), ..ProptestConfig::default() })]

    /// R10 over random paragraphs: a save of a random edit is a refusal in words, or a file that reads with the model's characters, holds every mark and every
    /// field it held (a field only when its characters stand), once each, and is no edit when saved again. **Never** an error of the check of the bytes made.
    #[test]
    fn a_random_edit_of_paragraphs_with_elements_keeps_every_element_or_is_refused_in_words(
        paragraphs in proptest::collection::vec((0u8..3, any::<bool>(), proptest::collection::vec(item(), 1..7)), 1..4),
        e in edit(),
    ) {
        let (mut next, mut marks, mut fields) = (0usize, Vec::new(), Vec::new());
        // Each paragraph is a plain paragraph or the only paragraph of an item of a bulleted list, with none, one or two soft page breaks between it and the paragraph
        // before (R10 items 1, 3 and 5: they are anchors at its start, and a block's mapping carries them over like any mark).
        let inner: String = paragraphs
            .iter()
            .map(|(breaks, listed, p)| {
                let one = format!("<text:p>{}</text:p>", render(p, &mut next, &mut marks, &mut fields));
                let lead = SOFT.repeat(usize::from(*breaks));
                marks.extend(std::iter::repeat_n(SOFT.to_owned(), usize::from(*breaks)));
                if *listed { format!("{lead}<text:list text:style-name=\"L1\"><text:list-item>{one}</text:list-item></text:list>") } else { format!("{lead}{one}") }
            })
            .collect();
        let mut o = open(&drawing(&format!("{T1}{LIST_L1}"), &inner));
        let old: Vec<char> = chars_of(the_shape(&o.doc)).chars().collect();
        let (from, to) = (share(&old, e.from), share(&old, e.to));
        let (from, to) = (from.min(to), from.max(to));
        let mut text: Vec<char> = old[..from].to_vec();
        text.extend(e.with.chars());
        text.extend(old[to..].iter().copied());
        let text_s: String = text.iter().collect();
        set_row(&mut o, "Character", 9, &[("Style", 2.0)]);
        let parts = match e.italic {
            Some((a, b)) => {
                let (a, b) = (share(&text, a), share(&text, b));
                let (a, b) = (a.min(b), a.max(b));
                vec![chars(&text[..a].iter().collect::<String>()), TextPart::Cp(9), chars(&text[a..b].iter().collect::<String>()), TextPart::Cp(0), chars(&text[b..].iter().collect::<String>())]
            }
            None => vec![chars(&text_s)],
        };
        set_parts(&mut o, parts);
        match real(&o.doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(_)) => {}
            Err(other) => prop_assert!(false, "an error that is no refusal in words: {other:?}"),
            Ok(w) => {
                let content = part(&w.bytes, "content.xml");
                // Each mark as often as the file held it (a soft page break has no name, so two are the same text).
                for m in &marks {
                    prop_assert_eq!(content.matches(m.as_str()).count(), marks.iter().filter(|n| *n == m).count(), "the mark {} in {}", m, content);
                }
                // Fields kept: each distinct field as often as the file now holds it (two of one text are one text).
                let mut distinct: Vec<&String> = fields.iter().collect();
                distinct.sort();
                distinct.dedup();
                let kept: usize = distinct.iter().map(|f| content.matches(f.as_str()).count()).sum();
                let dropped = w.notes.iter().filter(|n| matches!(n, SaveNote::FieldWritten { .. })).count();
                prop_assert!(kept + dropped <= fields.len(), "{} {} {}", kept, dropped, fields.len());
                if !w.edited {
                    prop_assert_eq!(&w.bytes, &o.bytes);
                }
                let back = open_bytes(w.bytes.clone());
                let expected: String = if text_s.is_empty() { String::new() } else { text_s.clone() };
                prop_assert_eq!(chars_of(the_shape(&back.doc)), expected);
                let again = real(&o.doc, &w.bytes, &w.index);
                match again {
                    Ok(again) => prop_assert!(!again.edited && again.bytes == w.bytes, "a second save is an edit"),
                    Err(e) => prop_assert!(false, "a second save failed: {e}"),
                }
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// LibreOffice as a black box: files this writer saved with each kept element, for a program that is not this crate to open. Run by hand:
// `ODG_ZW_OUT=<dir> cargo test -p diacraft-odg --test it -- --ignored lo_fixtures`, convert them with `soffice --headless --convert-to odg`, and read what it
// made with `ODG_ZW_IN=<dir> cargo test ... lo_readback`. Nothing here is a VSDX file, and nothing of LibreOffice is read: only whether an output file exists.

/// The page bodies that carry one kind of element each (and `holds` text to make bold, `bold` of it): `(name, paragraphs, the text before, the text after)`.
fn lo_cases() -> Vec<(&'static str, String, Vec<TextPart>)> {
    let bold_tail = |head: &str, tail: &str| vec![chars(head), TextPart::Cp(1), chars(tail)];
    vec![
        ("bookmark_point", r#"<text:p>Alpha <text:bookmark text:name="B1"/>beta gamma</text:p>"#.to_owned(), bold_tail("Alpha beta ", "gamma")),
        (
            "bookmark_range",
            r#"<text:p>one <text:bookmark-start text:name="R1"/>two<text:bookmark-end text:name="R1"/> four</text:p>"#.to_owned(),
            bold_tail("one two ", "four"),
        ),
        (
            "reference_mark_point",
            r#"<text:p>alpha beta<text:reference-mark text:name="M1"/> gamma</text:p>"#.to_owned(),
            bold_tail("alpha beta ", "gamma"),
        ),
        (
            "reference_mark_range",
            r#"<text:p>one <text:reference-mark-start text:name="r"/>two<text:reference-mark-end text:name="r"/> four</text:p>"#.to_owned(),
            bold_tail("one two ", "four"),
        ),
        ("soft_page_break", "<text:p>Alpha <text:soft-page-break/>beta gamma</text:p>".to_owned(), bold_tail("Alpha beta ", "gamma")),
        ("field_author", format!("<text:p>by {AUTHOR} today</text:p>"), bold_tail("by Ann ", "today")),
        ("field_title", "<text:p>by <text:title>Ann</text:title> today</text:p>".to_owned(), bold_tail("by Ann ", "today")),
        (
            "field_date_fixed",
            r#"<text:p>on <text:date text:fixed="true" text:date-value="2026-01-02">2 Jan</text:date> today</text:p>"#.to_owned(),
            bold_tail("on 2 Jan ", "today"),
        ),
        (
            "field_time_fixed",
            r#"<text:p>at <text:time text:fixed="true" text:time-value="10:11:12">10:11</text:time> today</text:p>"#.to_owned(),
            bold_tail("at 10:11 ", "today"),
        ),
        (
            "all_in_two_paragraphs",
            format!(
                "<text:p>a<text:bookmark text:name=\"x\"/>b<text:soft-page-break/> {AUTHOR}<text:reference-mark text:name=\"y\"/> c</text:p><text:p>second</text:p>"
            ),
            vec![chars("ab Ann "), TextPart::Cp(1), chars("c\nsecond")],
        ),
    ]
}

/// A page body that is a text frame, with the case's paragraphs in its `draw:text-box`.
fn lo_frame(inner: &str) -> String {
    format!(
        "\n    <draw:frame draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><draw:text-box>{inner}</draw:text-box></draw:frame>"
    )
}

#[test]
#[ignore = "writes the files a black-box check of LibreOffice converts; run by hand with ODG_ZW_OUT"]
fn lo_fixtures() {
    let Ok(dir) = std::env::var("ODG_ZW_OUT") else { return };
    let mut written = Vec::new();
    for (name, inner, parts) in lo_cases() {
        for (frame, page) in [("shape", shape("x", "1cm", &inner)), ("frame", lo_frame(&inner))] {
            let d = Drawing::a4().auto(T1).page(&page);
            // What the file is before our save.
            std::fs::write(format!("{dir}/in_{frame}_{name}.odg"), d.bytes()).unwrap();
            let mut o = open(&d);
            let id = crate::write_runs::id_of(&o.doc);
            bold(&mut o, 1);
            crate::write_core::shape_mut(&mut o.doc, 0, id).text =
                Some(std::sync::Arc::new(diacraft_doc::Text { parts: parts.clone(), ..Default::default() }));
            match real(&o.doc, &o.bytes, &o.index) {
                Ok(w) => {
                    std::fs::write(format!("{dir}/out_{frame}_{name}.odg"), &w.bytes).unwrap();
                    written.push(format!("out_{frame}_{name}"));
                }
                Err(e) => println!("REFUSED {frame} {name}: {e}"),
            }
        }
    }
    println!("FIXTURES {}: {}", written.len(), written.join(" "));
}

#[test]
#[ignore = "reads the files LibreOffice converted; run by hand with ODG_ZW_IN"]
fn lo_readback() {
    let Ok(dir) = std::env::var("ODG_ZW_IN") else { return };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".odg"))
        .collect();
    names.sort();
    for name in names {
        let bytes = std::fs::read(format!("{dir}/{name}")).unwrap();
        match diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()) {
            Ok((doc, report)) => {
                let text: Vec<String> = doc.pages.iter().flat_map(|p| p.shapes.iter()).map(|s| chars_of(s).replace('\n', "|")).collect();
                let content = part(&bytes, "content.xml");
                let count = |needle: &str| content.matches(needle).count();
                println!(
                    "READ {name}: ok text={text:?} tags: bookmark={} refmark={} softbreak={} author={} title={} date={} time={} kept={}",
                    count("<text:bookmark"),
                    count("<text:reference-mark"),
                    count("<text:soft-page-break"),
                    count("<text:author-name"),
                    count("<text:title"),
                    count("<text:date"),
                    count("<text:time"),
                    report.kept("text_mark") + report.kept("text_element") + report.kept("field"),
                );
            }
            Err(e) => println!("READ {name}: refused {}", e.class()),
        }
    }
}
