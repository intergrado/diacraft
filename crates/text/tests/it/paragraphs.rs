//! Paragraph settings: and the vectors of the `Paragraph` cells.

use diacraft_text::{BlockFormat, CharFormat, ParaFormat, lay_out};

use crate::{baselines, cf, close, em_db, glyph_count, lay_text, left, line_text, pt, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn top_block(width: f64) -> (BlockFormat, Option<f64>) {
    (BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }, Some(width))
}

fn step(laid: &diacraft_text::Laid) -> f64 {
    let b = baselines(laid);
    b[0] - b[1]
}

#[test]
fn para_a_negative_spline_is_a_multiple_of_the_size_and_a_positive_one_a_distance() {
    let db = em_db();
    let (block, w) = top_block(2.0);
    let p = ParaFormat { sp_line: -1.5, ..ParaFormat::default() };
    close(step(&lay_text(&db, "a\nb", &cf("Em", 20.0), &p, &block, w)), pt(30.0));
    // 0.5 inch is 36 points whatever the size.
    let p = ParaFormat { sp_line: 0.5, ..ParaFormat::default() };
    for size in [8.0, 20.0, 60.0] {
        close(step(&lay_text(&db, "a\nb", &cf("Em", size), &p, &block, w)), pt(36.0));
    }
}

#[test]
fn para_a_positive_spline_smaller_than_the_size_overlaps_the_lines_with_no_floor() {
    let (block, w) = top_block(2.0);
    let p = ParaFormat { sp_line: 0.25, ..ParaFormat::default() };
    close(step(&lay_text(&em_db(), "a\nb", &cf("Em", 72.0), &p, &block, w)), pt(18.0));
}

#[test]
fn spline_zero_is_one_size_not_lines_of_no_height() {
    // SpLine 0 uses one size's height.
    let (block, w) = top_block(2.0);
    let p = ParaFormat { sp_line: 0.0, ..ParaFormat::default() };
    close(step(&lay_text(&em_db(), "a\nb", &cf("Em", 10.0), &p, &block, w)), pt(10.0));
}

#[test]
fn para_space_before_and_after_add_to_each_other() {
    let db = em_db();
    let c = cf("Em", 10.0);
    let (block, w) = top_block(2.0);
    let a = ParaFormat::default();
    let b = ParaFormat { sp_before: 1.0 / 12.0, ..ParaFormat::default() };
    let laid = lay_out(&[run("a\n", &c, &a), run("b\n", &c, &b)], &block, w, &db);
    close(step(&laid), pt(18.0));
    let a2 = ParaFormat { sp_after: 1.0 / 36.0, ..ParaFormat::default() };
    let laid = lay_out(&[run("a\n", &c, &a2), run("b\n", &c, &b)], &block, w, &db);
    close(step(&laid), pt(20.0));
}

#[test]
fn spafter_a_negative_value_is_kept_as_stored() {
    let db = em_db();
    let c = cf("Em", 10.0);
    let (block, w) = top_block(2.0);
    let a = ParaFormat { sp_after: -1.0 / 36.0, ..ParaFormat::default() };
    let laid = lay_out(&[run("a\n", &c, &a), run("b\n", &c, &ParaFormat::default())], &block, w, &db);
    close(step(&laid), pt(10.0));
}

#[test]
fn para_the_step_between_lines_of_different_sizes_follows_both_boxes() {
    // Line 1 at 10 pt, line 2 at 20 pt, SpLine -1: 0.25 of the upper box plus 0.75 of the lower, 17.5 points. The lower line alone
    // would be 20 and the upper alone 10.
    let db = em_db();
    let p = ParaFormat { sp_line: -1.0, ..ParaFormat::default() };
    let (block, w) = top_block(2.0);
    let laid = lay_out(&[run("a\n", &cf("Em", 10.0), &p), run("b\n", &cf("Em", 20.0), &p)], &block, w, &db);
    close(step(&laid), pt(17.5));
}

#[test]
fn para_indents_move_lines_in_from_the_margins_and_are_kept_inside_the_block() {
    let db = em_db();
    let c = tenth();
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let hang = ParaFormat { halign: 0, ind_left: 0.25, ind_first: -0.25, ..ParaFormat::default() };
    let t = "aaaa bbbb cccc dddd";
    let laid = lay_text(&db, t, &c, &hang, &block, Some(1.0));
    assert!(laid.lines.len() >= 2);
    close(laid.lines[0].x, 0.0);
    for l in &laid.lines[1..] {
        close(l.x, 0.25);
    }
    // IndFirst -1 at IndLeft 0.25: the first line is at the block's edge, not 0.75 inch outside it.
    let out = ParaFormat { halign: 0, ind_left: 0.25, ind_first: -1.0, ..ParaFormat::default() };
    close(lay_text(&db, "aa", &c, &out, &block, Some(1.0)).lines[0].x, 0.0);
    // IndLeft -0.25 puts the line at the block's edge too.
    let neg = ParaFormat { halign: 0, ind_left: -0.25, ..ParaFormat::default() };
    close(lay_text(&db, "aa", &c, &neg, &block, Some(1.0)).lines[0].x, 0.0);
}

#[test]
fn para_an_indent_right_larger_than_the_room_leaves_it_to_indleft_and_still_draws() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let p = ParaFormat { halign: 0, ind_left: 0.25, ind_right: 5.0, ..ParaFormat::default() };
    let laid = lay_text(&db, "abc", &tenth(), &p, &block, Some(1.0));
    // No room at all, so one letter to a line, each starting at IndLeft, and every letter drawn.
    assert!(laid.lines.iter().all(|l| (l.x - 0.25).abs() < 1e-9));
    assert_eq!(laid.lines.iter().map(glyph_count).sum::<usize>(), 3);
}

#[test]
fn indright_right_aligned_lines_end_at_the_right_indent() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(1.0), ..BlockFormat::default() };
    let p = ParaFormat { halign: 2, ind_right: 0.5, ..ParaFormat::default() };
    let laid = lay_text(&db, "aaaaa", &tenth(), &p, &block, Some(2.0));
    close(laid.lines[0].x + laid.lines[0].width, 1.5);
}

#[test]
fn halign_lines_sit_left_centred_and_right_inside_the_indents() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(1.0), left_margin: 0.1, right_margin: 0.2, ..BlockFormat::default() };
    for (halign, x) in [(0u8, 0.1), (1, 0.1 + (1.7 - 0.5) / 2.0), (2, 1.8 - 0.5)] {
        let p = ParaFormat { halign, ..ParaFormat::default() };
        let laid = lay_text(&db, "aaaaa", &tenth(), &p, &block, Some(2.0));
        close(laid.lines[0].x, x);
    }
}

#[test]
fn halign_a_stray_value_reads_as_the_roots_centre_and_6_to_8_justify() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(1.0), ..BlockFormat::default() };
    let centred = lay_text(&db, "aaaaa", &tenth(), &ParaFormat { halign: 1, ..ParaFormat::default() }, &block, Some(2.0)).lines[0].x;
    for halign in [5u8, 9, 255] {
        let p = ParaFormat { halign, ..ParaFormat::default() };
        close(lay_text(&db, "aaaaa", &tenth(), &p, &block, Some(2.0)).lines[0].x, centred);
    }
}

#[test]
fn para_justifying_stretches_gaps_on_every_line_but_the_last_and_4_stretches_it_too() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let t = "aa bb cc dd ee ff";
    let line = |halign: u8, n: usize| {
        let p = ParaFormat { halign, ..ParaFormat::default() };
        let laid = lay_text(&db, t, &tenth(), &p, &block, Some(1.0));
        (laid.lines.len(), laid.lines[n].x, laid.lines[n].width)
    };
    // With 1.0 to fill: "aa bb cc" (0.8), "dd ee ff" (0.8). Justified, the first line spans the whole measure, the last does not.
    for halign in [3u8, 6, 7, 8] {
        let (n, x, w) = line(halign, 0);
        assert_eq!(n, 2);
        close(x, 0.0);
        close(w, 1.0);
        let (_, lx, lw) = line(halign, 1);
        close(lx, 0.0);
        close(lw, 0.8);
    }
    let (_, x, w) = line(4, 1);
    close(x, 0.0);
    close(w, 1.0);
}

#[test]
fn a_justified_line_with_one_word_cannot_be_stretched() {
    let db = em_db();
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let p = ParaFormat { halign: 3, ..ParaFormat::default() };
    let laid = lay_text(&db, "aaaa\nbbbb", &tenth(), &p, &block, Some(1.0));
    close(laid.lines[0].width, 0.4);
}

#[test]
fn the_paragraph_takes_its_format_from_the_run_holding_its_last_character() {
    // A pp inside a paragraph sets the whole paragraph.
    let db = em_db();
    let c = tenth();
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let row0 = ParaFormat { halign: 0, ..ParaFormat::default() };
    let row1 = ParaFormat { halign: 2, ..ParaFormat::default() };
    // `<pp 0/>a<pp 1/>b` LF: one paragraph, on row 1.
    let laid = lay_out(&[run("a", &c, &row0), run("b\n", &c, &row1)], &block, Some(1.0), &db);
    assert_eq!(laid.lines.len(), 1);
    close(laid.lines[0].x, 1.0 - 0.2);
    // `<pp 0/>a LF b LF <pp 1/>c LF`: a and b on row 0, c on row 1.
    let laid = lay_out(&[run("a\nb\n", &c, &row0), run("c\n", &c, &row1)], &block, Some(1.0), &db);
    assert_eq!(laid.lines.len(), 3);
    close(laid.lines[0].x, 0.0);
    close(laid.lines[1].x, 0.0);
    close(laid.lines[2].x, 0.9);
}

#[test]
fn flags_a_right_to_left_paragraph_orders_its_letters_right_to_left() {
    // Only 1 is right to left, so Hebrew letters, which are right to left anyway, make the order visible in `gid`s.
    let he = diacraft_fonts::synth::SynthFont::new("He").chars("אבג", 1000).ascii(1000);
    let db = crate::db(std::slice::from_ref(&he));
    let c = CharFormat { size: 0.1, ..cf("He", 0.0) };
    let block = BlockFormat { vertical_align: 0, height: Some(1.0), ..BlockFormat::default() };
    let p = ParaFormat { halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&db, "אבג", &c, &p, &block, Some(2.0));
    let order: Vec<u32> = crate::gids(&laid.lines[0]);
    // Right to left on the page: the first letter typed is rightmost, so drawn last from the left.
    assert_eq!(order, vec![u32::from(he.gid('ג')), u32::from(he.gid('ב')), u32::from(he.gid('א'))]);
    // Latin letters before it, base direction left to right: `a` first from the left.
    let laid = lay_text(&db, "aאב", &c, &p, &block, Some(2.0));
    let order = crate::gids(&laid.lines[0]);
    assert_eq!(order[0], u32::from(he.gid('a')));
    // 2 is left to right for drawing, as 0.
    let two = ParaFormat { flags: 2, halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&db, "aאב", &c, &two, &block, Some(2.0));
    assert_eq!(crate::gids(&laid.lines[0])[0], u32::from(he.gid('a')));
    // Flags 1: the base direction is right to left, so the Latin letter, first typed, ends up at the far right of the run, and
    // alignment 0 keeps its side: the line still starts at the left.
    let rtl = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&db, "aאב", &c, &rtl, &block, Some(2.0));
    assert_eq!(crate::gids(&laid.lines[0]), vec![u32::from(he.gid('ב')), u32::from(he.gid('א')), u32::from(he.gid('a'))]);
    close(laid.lines[0].x, 0.0);
    assert!(laid.lines[0].rtl);
}

#[test]
fn the_line_text_slicing_helper_agrees_with_the_range() {
    let db = em_db();
    let laid = lay_text(&db, "ab\ncd", &tenth(), &left(), &BlockFormat::default(), Some(1.0));
    assert_eq!(laid.lines.iter().map(|l| line_text("ab\ncd", l)).collect::<Vec<_>>(), vec!["ab".to_owned(), "cd".to_owned()]);
}
