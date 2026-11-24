//! How runs map onto text: and the rest that reach a layout. (Which row a
//! marker names is the caller's: a layout is given runs that already carry their rows' values.)

use diacraft_text::{BlockFormat, Laid, ParaFormat, lay_out};

use crate::{baselines, cf, close, em_db, glyph_count, left, line_text, pt, run};

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn tenth(font: &str) -> diacraft_text::CharFormat {
    diacraft_text::CharFormat { size: 0.1, ..cf(font, 0.0) }
}

fn lay(runs: &[diacraft_text::Run]) -> Laid {
    lay_out(runs, &block(), Some(2.0), &em_db())
}

fn all_text(runs: &[diacraft_text::Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

#[test]
fn run_markers_overlap_so_a_paragraph_can_hold_several_character_rows() {
    // `<cp 0/><pp 0/>ab<cp 1/>cd` LF `<pp 1/><cp 2/>ef` LF: two paragraphs, `abcd` on paragraph row 0 and `ef` on row 1, and `ab`,
    // `cd`, `ef` on character rows 0, 1 and 2.
    let (c0, c1, c2) = (tenth("Em"), diacraft_text::CharFormat { style: 1, ..tenth("Em") }, diacraft_text::CharFormat { style: 2, ..tenth("Em") });
    let (p0, p1) = (ParaFormat { halign: 0, ..ParaFormat::default() }, ParaFormat { halign: 2, ..ParaFormat::default() });
    let runs = [run("ab", &c0, &p0), run("cd\n", &c1, &p0), run("ef\n", &c2, &p1)];
    let laid = lay(&runs);
    assert_eq!(laid.lines.len(), 2);
    let src = all_text(&runs);
    assert_eq!(line_text(&src, &laid.lines[0]), "abcd");
    assert_eq!(line_text(&src, &laid.lines[1]), "ef");
    close(laid.lines[0].x, 0.0);
    close(laid.lines[1].x, 2.0 - 0.2);
    // Two character rows on the first line: two glyph runs, the second in the bold row.
    assert_eq!(laid.lines[0].runs.len(), 2);
}

#[test]
fn run_a_carriage_return_is_not_a_second_line_end_and_draws_nothing() {
    let c = tenth("Em");
    let laid = lay(&[run("one\r\n", &c, &left())]);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(glyph_count(&laid.lines[0]), 3);
    // A lone CR draws nothing and breaks nothing.
    let laid = lay(&[run("one\rtwo", &c, &left())]);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(glyph_count(&laid.lines[0]), 6);
    // Two paragraphs, as one LF each ends one.
    assert_eq!(lay(&[run("one\ntwo\n", &c, &left())]).lines.len(), 2);
}

#[test]
fn run_a_last_line_feed_closes_the_last_paragraph_and_adds_no_line() {
    let c = tenth("Em");
    assert_eq!(lay(&[run("one\n", &c, &left())]).lines.len(), 1);
    let two = lay(&[run("one\n\n", &c, &left())]);
    assert_eq!(two.lines.len(), 2);
    assert_eq!(glyph_count(&two.lines[1]), 0, "the second is an empty line");
    // An empty line inside the text takes a step.
    let mid = lay(&[run("a\n\nb", &c, &left())]);
    assert_eq!(mid.lines.len(), 3);
    close(baselines(&mid)[0] - baselines(&mid)[2], 2.0 * 0.12);
}

#[test]
fn run_a_forced_break_ends_a_line_not_a_paragraph_and_is_never_drawn() {
    let c = tenth("Em");
    let p = ParaFormat { halign: 0, ind_first: 0.25, ..ParaFormat::default() };
    let laid = lay(&[run("one\u{2028}two\n", &c, &p)]);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].paragraph, laid.lines[1].paragraph);
    close(laid.lines[0].x, 0.25);
    close(laid.lines[1].x, 0.0);
    assert_eq!(glyph_count(&laid.lines[0]), 3, "no box for the mark");
}

#[test]
fn run_an_empty_paragraph_takes_its_pitch_from_the_run_holding_its_line_feed() {
    let (small, big) = (cf("Em", 12.0), cf("Em", 36.0));
    // `<cp 0/>a` LF `<cp 1/>` LF, row 0 at 12 pt and row 1 at 36: the second, empty paragraph is a line of 36 points' pitch.
    let laid = lay(&[run("a\n", &small, &left()), run("\n", &big, &left())]);
    assert_eq!(laid.lines.len(), 2);
    close(laid.lines[0].height, pt(14.4));
    close(laid.lines[1].height, pt(43.2));
    // And with no line feed after it, the run holding no characters is kept and draws nothing, and sizes nothing.
    let laid = lay(&[run("a", &small, &left()), run("", &big, &left())]);
    assert_eq!(laid.lines.len(), 1);
    close(laid.lines[0].height, pt(14.4));
}

#[test]
fn run_indentation_left_by_formatted_xml_draws_no_empty_line() {
    // `<cp 0/>`, then a line feed and four spaces, then a field's text: the stretch is the file's own layout.
    let c = tenth("Em");
    let laid = lay(&[run("", &c, &left()), run("\n    ", &c, &left()), run("42", &c, &left())]);
    assert_eq!(laid.lines.len(), 1, "no empty line above the field's text");
    assert_eq!(glyph_count(&laid.lines[0]), 2);
    // A stretch with a line feed and no space, or a space and no line feed, is text.
    assert_eq!(lay(&[run("a", &c, &left()), run("\n", &c, &left()), run("b", &c, &left())]).lines.len(), 2);
    assert_eq!(lay(&[run("a", &c, &left()), run("  ", &c, &left()), run("b", &c, &left())]).lines.len(), 1);
}

#[test]
fn run_a_field_is_text_formatted_by_the_run_it_sits_in() {
    // A field's displayed characters are an ordinary run: here a bold 9 after plain text.
    let (plain, bold) = (tenth("Em"), diacraft_text::CharFormat { style: 1, ..tenth("Em") });
    let laid = lay(&[run("Page ", &plain, &left()), run("9", &bold, &left())]);
    assert_eq!(laid.lines[0].runs.len(), 2);
    assert_eq!(laid.lines[0].runs[1].glyphs.len(), 1);
    close(laid.lines[0].width, 0.6);
}

#[test]
fn source_character_ranges_count_every_character_of_every_run() {
    let c = tenth("Em");
    let laid = lay(&[run("ab\r\n", &c, &left()), run("cd", &c, &left())]);
    // The CR is a character of the source even though it draws nothing.
    assert_eq!(laid.lines[0].chars, 0..2);
    assert_eq!(laid.lines[1].chars, 4..6);
}

#[test]
fn formatting_only_runs_that_hold_no_text_make_one_empty_line() {
    let c = tenth("Em");
    let laid = lay(&[run("", &c, &left()), run("", &c, &left())]);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(glyph_count(&laid.lines[0]), 0);
}

#[test]
fn control_characters_other_than_tab_and_the_line_ends_draw_nothing() {
    let c = tenth("Em");
    let laid = lay(&[run("a\u{0}\u{7}\u{b}\u{c}\u{85}\u{2029}b", &c, &left())]);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(glyph_count(&laid.lines[0]), 2);
    close(laid.lines[0].width, 0.2);
}

#[test]
fn text_beyond_the_plane_is_counted_by_scalar_value() {
    // Characters past the Basic Multilingual Plane are one character each, not two.
    let c = tenth("Em");
    let laid = lay(&[run("a\u{1F600}b", &c, &left())]);
    assert_eq!(laid.lines[0].chars, 0..3);
    assert_eq!(glyph_count(&laid.lines[0]), 3);
}
