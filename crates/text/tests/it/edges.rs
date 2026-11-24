//! More edges, aimed at what an operator-swap pass over the layout code left standing: exact positions for lines through text, the
//! origin tabs are measured from, the width a block takes when it has none, and the cap on lines across paragraphs.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, DecorationKind, Laid, Note, ParaFormat, TabStop, style};

use crate::{cf, close, db, em_db, glyph_count, lay_text, left, pt};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn glyph_xs(laid: &Laid) -> Vec<f64> {
    laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect()
}

fn marks_db() -> FontDb {
    db(&[SynthFont::new("Em").ascii(1000), SynthFont::new("Marks").chars("\u{2022}", 1000)])
}

#[test]
fn an_overline_sits_one_line_thickness_above_the_faces_ascent() {
    // The synthetic face: ascent 800 of 1000, underline thickness 50. At 20 points: top at 17 points, 1 point thick.
    let c = CharFormat { overline: true, ..cf("Em", 20.0) };
    let laid = lay_text(&em_db(), "ab", &c, &left(), &top_block(), None);
    let d = laid.lines[0].decorations.iter().find(|d| d.kind == DecorationKind::Overline).unwrap();
    close(d.top - laid.lines[0].baseline, 0.85 * pt(20.0));
    close(d.thickness, 0.05 * pt(20.0));
}

#[test]
fn a_strikeout_is_where_the_face_puts_it_and_a_double_one_is_either_side_of_that() {
    // The synthetic face: strikeout 300 up and 50 thick. Single: top at 0.3 em. Double: 0.35 and 0.25 em, a thickness apart.
    let single = CharFormat { strikethrough: true, ..cf("Em", 20.0) };
    let laid = lay_text(&em_db(), "ab", &single, &left(), &top_block(), None);
    let d = laid.lines[0].decorations.iter().find(|d| d.kind == DecorationKind::Strikethrough).unwrap();
    close(d.top - laid.lines[0].baseline, 0.3 * pt(20.0));
    close(d.thickness, 0.05 * pt(20.0));
    let double = CharFormat { double_strikethrough: true, ..cf("Em", 20.0) };
    let laid = lay_text(&em_db(), "ab", &double, &left(), &top_block(), None);
    let mut tops: Vec<f64> =
        laid.lines[0].decorations.iter().filter(|d| d.kind == DecorationKind::DoubleStrikethrough).map(|d| d.top - laid.lines[0].baseline).collect();
    tops.sort_by(f64::total_cmp);
    assert_eq!(tops.len(), 2);
    close(tops[0], 0.25 * pt(20.0));
    close(tops[1], 0.35 * pt(20.0));
}

#[test]
fn a_soft_hyphen_is_drawn_at_the_end_of_the_text_before_it_and_underlined_with_it() {
    let c = CharFormat { style: style::UNDERLINE, ..tenth() };
    let laid = lay_text(&em_db(), "aaaaaa\u{00AD}bbbbbb", &c, &left(), &top_block(), Some(1.0));
    let line = &laid.lines[0];
    let hyphen = line.runs.iter().flat_map(|r| r.glyphs.iter()).last().unwrap();
    close(hyphen.x, 0.6);
    close(hyphen.y, line.baseline);
    close(hyphen.advance, 0.1);
    let under = line.decorations.iter().find(|d| d.kind == DecorationKind::Underline).unwrap();
    close(under.x0, 0.0);
    close(under.x1, 0.7);
}

#[test]
fn a_hyphen_is_as_wide_as_the_run_scales_it() {
    let c = CharFormat { scale: 2.0, ..tenth() };
    let laid = lay_text(&em_db(), "aaaaaa\u{00AD}bbbbbb", &c, &left(), &top_block(), Some(1.5));
    close(laid.lines[0].width, 6.0 * 0.2 + 0.2);
}

#[test]
fn a_mark_is_placed_by_the_shapers_offsets_in_both_directions() {
    // The bundled face puts the acute on a `q` back over it and a little up: left of where the pen stands after the `q`, above the
    // baseline.
    let c = CharFormat { font: "Segoe UI".into(), size: 1.0, ..CharFormat::default() };
    let laid = lay_text(&FontDb::bundled(), "q\u{0301}", &c, &left(), &top_block(), None);
    let g: Vec<_> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).collect();
    assert_eq!(g.len(), 2);
    assert!(g[1].x < g[0].x + g[0].advance - 1e-6, "back over the letter: {} against {}", g[1].x, g[0].x + g[0].advance);
    assert!(g[1].x > g[0].x, "and still on it");
    assert!(g[1].y > laid.lines[0].baseline + 1e-6, "above the baseline");
}

#[test]
fn a_custom_stop_at_a_default_step_wins_with_its_own_alignment() {
    // A right-aligned stop at 1.0 and default steps of 0.5: from the pen at 0.6 both say 1.0, and the custom one is the stop, so `bb`
    // ends there. The default step, left aligned, would start it there.
    let p = ParaFormat { tabs: vec![TabStop { position: 1.0, alignment: 2 }], ..left() };
    let block = BlockFormat { default_tab_stop: 0.5, ..top_block() };
    let laid = lay_text(&em_db(), "aaaaaa\tbb", &tenth(), &p, &block, Some(5.0));
    close(glyph_xs(&laid)[6], 0.8);
}

#[test]
fn stops_are_measured_from_the_area_with_a_margin_and_a_bullet_in_the_way() {
    // LeftMargin 0.1, default steps of 0.5, a run 0.45 wide and a tab: the next stop is 0.5 from the area, 0.6 from the block's edge.
    let block = BlockFormat { left_margin: 0.1, default_tab_stop: 0.5, ..top_block() };
    let laid = lay_text(&em_db(), "aaaaa\tb", &CharFormat { size: 0.09, ..cf("Em", 0.0) }, &left(), &block, Some(5.0));
    close(glyph_xs(&laid)[5], 0.6);
    // With a bullet 0.25 in, the first line's text starts at 0.25 and a margin of 0.1 puts the origin at 0.35 from the block's edge.
    let p = ParaFormat { bullet: 1, ..left() };
    let laid = lay_text(&marks_db(), "aaaaa\tb", &CharFormat { size: 0.09, ..cf("Em", 0.0) }, &p, &block, Some(5.0));
    // Text at 0.1 + 0.25, five letters of 0.09 reach 0.8 from the block's edge, which is 0.7 from the area: the next stop is 1.0.
    close(glyph_xs(&laid)[5], 0.1 + 1.0);
}

#[test]
fn the_first_line_of_a_bulleted_paragraph_has_the_bullets_gap_less_room() {
    // A block 1.0 wide with a bullet whose text starts 0.25 in: seven letters fit and the eighth wraps.
    let p = ParaFormat { bullet: 1, ..left() };
    let laid = lay_text(&marks_db(), "aaaaaaaa", &tenth(), &p, &top_block(), Some(1.0));
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].chars, 0..7);
}

#[test]
fn a_block_as_wide_as_its_text_counts_the_first_line_indent_and_the_bullet() {
    // Right-aligned, no width: the block is as wide as its widest line with its indent.
    let hang = ParaFormat { halign: 2, ind_first: 0.5, ..ParaFormat::default() };
    let laid = lay_text(&em_db(), "aaaaa\u{2028}bb", &tenth(), &hang, &top_block(), None);
    close(laid.lines[0].x, 0.5);
    close(laid.lines[1].x, 0.8);
    let bullet = ParaFormat { halign: 2, bullet: 1, ..ParaFormat::default() };
    let laid = lay_text(&marks_db(), "ab", &tenth(), &bullet, &top_block(), None);
    close(laid.lines[0].x, 0.25);
}

#[test]
fn the_overflow_flag_counts_the_indent_and_a_line_that_fits_exactly_does_not_overflow() {
    // One letter 0.5 wide, a measure of 0.6, an indent of 0.2: 0.4 of room, so it overflows. Without the indent it would not.
    let wide = CharFormat { size: 0.5, ..cf("Em", 0.0) };
    let indented = ParaFormat { ind_left: 0.2, ..left() };
    assert!(lay_text(&em_db(), "a", &wide, &indented, &top_block(), Some(0.6)).overflow);
    assert!(!lay_text(&em_db(), "a", &wide, &left(), &top_block(), Some(0.6)).overflow);
    // Ten letters of 0.1 in a measure of exactly 1.0 fit.
    assert!(!lay_text(&em_db(), "aaaaaaaaaa", &tenth(), &left(), &top_block(), Some(1.0)).overflow);
}

#[test]
fn the_line_cap_holds_across_paragraphs_and_not_for_each() {
    // Two paragraphs of 60,000 letters each in a block of no width: a line a letter, 120,000 lines wanted, 100,000 made.
    let text = format!("{}\n{}", "a".repeat(60_000), "b".repeat(60_000));
    let laid = lay_text(&em_db(), &text, &tenth(), &left(), &top_block(), Some(0.0));
    assert_eq!(laid.lines.len(), 100_000);
    assert!(laid.notes.contains(&Note::LinesCut { chars: 20_000 }), "{:?}", laid.notes);
}

#[test]
fn a_tab_after_a_bullet_counts_from_where_the_text_starts_and_not_from_behind_it() {
    // LeftMargin 0.1, steps of 0.4, a bullet whose text starts 0.25 in, five letters of 0.09: the pen is 0.7 from the area's edge and the
    // next step 0.8, so the letter after the tab is 0.1 further on. Counted from 0.25 behind the start it would be 0.2 further.
    let block = BlockFormat { left_margin: 0.1, default_tab_stop: 0.4, ..top_block() };
    let p = ParaFormat { bullet: 1, ..left() };
    let laid = lay_text(&marks_db(), "aaaaa\tb", &CharFormat { size: 0.09, ..cf("Em", 0.0) }, &p, &block, Some(5.0));
    close(glyph_xs(&laid)[5], 0.1 + 0.25 + 0.45 + 0.1);
}

#[test]
fn the_empty_line_after_a_closing_forced_break_sits_at_the_end_of_the_text() {
    let laid = lay_text(&em_db(), "ab\u{2028}", &tenth(), &left(), &top_block(), None);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].chars, 0..2);
    assert_eq!(laid.lines[1].chars, 3..3);
}

#[test]
fn a_stop_a_billionth_of_an_inch_ahead_of_the_pen_is_not_ahead_of_it() {
    // At the start of a line the pen is 0 and the tolerance 1e-9, so a stop at 1e-9 is within it: the tab goes to the step at 0.5.
    let p = ParaFormat { tabs: vec![TabStop { position: 1e-9, alignment: 0 }], ..left() };
    let block = BlockFormat { default_tab_stop: 0.5, ..top_block() };
    let laid = lay_text(&em_db(), "\ta", &tenth(), &p, &block, Some(5.0));
    close(glyph_xs(&laid)[0], 0.5);
}

#[test]
fn an_indent_past_the_right_limit_leaves_no_room_and_not_less_than_none() {
    // IndLeft 0.8 against a right limit of 0.5: no room at all. Letters that take no room still fit, so one line; with room counted
    // below zero each would be pushed to a line of its own.
    let nothing = SynthFont::new("Nothing").ascii(0);
    let d = db(std::slice::from_ref(&nothing));
    let p = ParaFormat { ind_left: 0.8, ind_right: 0.5, ..left() };
    let laid = lay_text(&d, "aaaa", &CharFormat { size: 0.1, ..cf("Nothing", 0.0) }, &p, &top_block(), Some(1.0));
    assert_eq!(laid.lines.len(), 1);
}

#[test]
fn a_centred_line_wider_than_its_room_with_a_bullet_has_a_background_from_its_own_left_edge() {
    // Text 2.0 wide in a room of 0.75 after a bullet, centred: the line starts left of the bullet's start, and the rectangle with it.
    let b = BlockFormat { background: Some((diacraft_text::Rgba::BLACK, 0.0)), ..top_block() };
    let p = ParaFormat { bullet: 1, bullet_size: -0.1, halign: 1, ..ParaFormat::default() };
    let c = CharFormat { size: 2.0, ..cf("Em", 0.0) };
    let laid = lay_text(&marks_db(), "a", &c, &p, &b, Some(1.0));
    let line = &laid.lines[0];
    let bullet_x = line.bullet.as_ref().unwrap().glyphs[0].x;
    assert!(line.x < bullet_x - 0.1, "the line {} starts left of the bullet {}", line.x, bullet_x);
    close(laid.background.unwrap().rect.x0, line.x);
}

#[test]
fn a_negative_size_reads_as_none_at_all_and_makes_no_negative_width() {
    // A tab with no stop to go to is one space wide, a space of this size: none, and never a negative number.
    let c = CharFormat { size: -pt(10.0), ..cf("Em", 0.0) };
    let block = BlockFormat { default_tab_stop: 0.0, ..top_block() };
    let laid = lay_text(&em_db(), "a\tb", &c, &left(), &block, Some(5.0));
    close(laid.lines[0].width, 0.0);
}

#[test]
fn text_of_no_size_takes_no_letterspace() {
    let c = CharFormat { size: 0.0, letterspace: pt(5.0), ..cf("Em", 12.0) };
    let laid = lay_text(&em_db(), "abc", &c, &left(), &top_block(), None);
    close(laid.lines[0].width, 0.0);
    assert_eq!(glyph_count(&laid.lines[0]), 0);
}

#[test]
fn a_turkish_run_changes_only_the_letter_i() {
    let f = SynthFont::new("Cases").ascii(500).chars("İı", 500);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { case: 1, lang: "tr-TR".into(), ..cf("Cases", 12.0) };
    let ids = |text: &str| {
        let laid = lay_text(&d, text, &c, &left(), &top_block(), None);
        crate::gids(&laid.lines[0])
    };
    assert_eq!(ids("abi"), vec![u32::from(f.gid('A')), u32::from(f.gid('B')), u32::from(f.gid('İ'))]);
    assert_eq!(ids("ıi"), vec![u32::from(f.gid('I')), u32::from(f.gid('İ'))]);
}
