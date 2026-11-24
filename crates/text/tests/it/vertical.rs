//! Vertical text (`TextDirection` 1).
//!
//! The block's frame is the same as for text across: paper inches, the origin at the lower left, `x` along the width and `y` up the height.
//! A column of vertical text runs down it, from the block's top, and the next column lies to the left of the one before. Every number below is
//! written out from the rule, and not read back from the layout. A face of 1 em to a letter makes a letter 0.1 inch long at the size of 0.1
//! inch, and a line box 0.12 inch wide (`SpLine` -1.2).

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, DecorationKind, Laid, Line, Note, ParaFormat, Rgba, lay_out, style};

use crate::{cf, close, db, em_db, lay_text, left, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

/// A vertical block `height` tall, margins 0, the text seated by `valign`; the width is given to the layout beside it.
fn block(height: f64, valign: u8) -> BlockFormat {
    BlockFormat { text_direction: 1, vertical_align: valign, height: Some(height), ..BlockFormat::default() }
}

fn lay(text: &str, p: &ParaFormat, c: &CharFormat, width: f64, height: f64, valign: u8) -> Laid {
    lay_text(&em_db(), text, c, p, &block(height, valign), Some(width))
}

/// The glyph origins of a line's runs, in order: `(x, y)`.
fn origins(line: &Line) -> Vec<(f64, f64)> {
    line.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| (g.x, g.y))).collect()
}

#[test]
fn one_column_is_centred_across_and_starts_at_the_top() {
    // A block 1 inch wide and 2 tall, margins 0, `VerticalAlign` 1, `HAlign` 0: one column whose centre line is at x 0.5, starting
    // at the block's top. Running across, or starting at the bottom, fails.
    let laid = lay("abc", &left(), &tenth(), 1.0, 2.0, 1);
    assert_eq!(laid.lines.len(), 1);
    let line = &laid.lines[0];
    assert!(line.vertical);
    close((line.top + line.bottom) / 2.0, 0.5);
    close(line.top - line.bottom, 0.12);
    // The baseline is three quarters of the column's width in from its right edge (turned).
    close(line.baseline, 0.56 - 0.09);
    let at = origins(line);
    assert_eq!(at.len(), 3);
    for (i, (x, y)) in at.iter().enumerate() {
        close(*x, 0.47);
        close(*y, 2.0 - 0.1 * i as f64);
    }
    // The text starts at the top and runs down by its length.
    close(line.x, 0.0);
    close(line.width, 0.3);
}

#[test]
fn a_letter_that_is_not_east_asian_lies_on_its_side() {
    // Others turn a quarter clockwise. The run says so, and a renderer turns the outline.
    let laid = lay("abc", &left(), &tenth(), 1.0, 2.0, 1);
    assert!(laid.lines[0].runs.iter().all(|r| r.turned));
    // Text across is not turned.
    let across = lay_text(&em_db(), "abc", &tenth(), &left(), &BlockFormat { height: Some(2.0), ..BlockFormat::default() }, Some(1.0));
    assert!(across.lines[0].runs.iter().all(|r| !r.turned));
    assert!(!across.lines[0].vertical);
}

#[test]
fn td_a_run_wraps_against_the_height_and_not_the_width() {
    // A block 2 inches wide and 1 inch tall, a run of 1.5 inches: it wraps against the 1 inch height. Here a run of 1.1 inch
    // in a block 0.65 inch tall breaks into two columns of 0.5 inch, "aa bb" and "cc dd", where across a width of 2 inches holds it whole.
    let text = "aa bb cc dd";
    let laid = lay(text, &left(), &tenth(), 2.0, 0.65, 0);
    assert_eq!(laid.lines.len(), 2, "two columns");
    close(laid.lines[0].width, 0.5);
    close(laid.lines[1].width, 0.5);
    // The second column lies to the left of the first by one line box (columns stack right to left).
    close(laid.lines[0].baseline - laid.lines[1].baseline, 0.12);
    // Seated from the right edge (`VerticalAlign` 0): the first column's right edge is the block's.
    close(laid.lines[0].top, 2.0);
    let across = lay_text(&em_db(), text, &tenth(), &left(), &BlockFormat { height: Some(0.65), ..BlockFormat::default() }, Some(2.0));
    assert_eq!(across.lines.len(), 1);
    // The stack is the columns' widths and the longest column.
    close(laid.size.width, 0.24);
    close(laid.size.height, 0.5);
}

#[test]
fn vertical_align_seats_the_text_from_the_right_the_middle_or_the_left() {
    // `VerticalAlign` 0, 1 and 2 seat the text from the right edge, the middle or the left edge.
    let right = lay("a", &left(), &tenth(), 1.0, 2.0, 0);
    close(right.lines[0].top, 1.0);
    let middle = lay("a", &left(), &tenth(), 1.0, 2.0, 1);
    close(middle.lines[0].top, 0.56);
    let at_left = lay("a", &left(), &tenth(), 1.0, 2.0, 2);
    close(at_left.lines[0].bottom, 0.0);
    // 3 and above are the top, which is the right edge here.
    let above = lay("a", &left(), &tenth(), 1.0, 2.0, 3);
    close(above.lines[0].top, 1.0);
}

#[test]
fn margins_stay_with_the_physical_edges() {
    // The top margin is where the column starts, the right margin keeps a right-seated column off the edge, the left one a left-seated
    // column, and the bottom margin is where a column wraps.
    let b = BlockFormat { top_margin: 0.3, bottom_margin: 0.1, right_margin: 0.2, left_margin: 0.15, ..block(2.0, 0) };
    let laid = lay_text(&em_db(), "ab", &tenth(), &left(), &b, Some(1.0));
    let line = &laid.lines[0];
    close(line.top, 1.0 - 0.2);
    let at = origins(line);
    close(at[0].1, 2.0 - 0.3);
    close(at[1].1, 2.0 - 0.3 - 0.1);
    let b = BlockFormat { vertical_align: 2, ..b };
    let laid = lay_text(&em_db(), "ab", &tenth(), &left(), &b, Some(1.0));
    close(laid.lines[0].bottom, 0.15);
    // The room down the block is its height less the top and bottom margins: 2 - 0.3 - 0.1 = 1.6, so 16 letters fit and the 17th wraps.
    let long = lay_text(&em_db(), &"a".repeat(17), &tenth(), &left(), &b, Some(1.0));
    assert_eq!(long.lines.len(), 2);
    close(long.lines[0].width, 1.6);
}

#[test]
fn alignment_runs_along_the_column() {
    // `HAlign` 0 starts at the area's top, 1 centres, 2 ends at the bottom.
    let first = |h: u8| {
        let p = ParaFormat { halign: h, ..ParaFormat::default() };
        origins(&lay("aa", &p, &tenth(), 1.0, 1.0, 1).lines[0])[0].1
    };
    close(first(0), 1.0);
    close(first(1), 1.0 - 0.4);
    close(first(2), 1.0 - 0.8);
}

#[test]
fn indents_run_along_the_column() {
    // The left indent is the distance from the top; the first line's indent is added to it, and the right indent shortens the room at the bottom.
    let p = ParaFormat { halign: 0, ind_left: 0.25, ..ParaFormat::default() };
    let laid = lay("a", &p, &tenth(), 1.0, 2.0, 0);
    close(origins(&laid.lines[0])[0].1, 2.0 - 0.25);
    let p = ParaFormat { halign: 0, ind_right: 1.8, ..ParaFormat::default() };
    let laid = lay("aaaa", &p, &tenth(), 1.0, 2.0, 0);
    // 2 - 1.8 = 0.2 of room: two letters to a column.
    assert_eq!(laid.lines.len(), 2);
}

#[test]
fn an_east_asian_letter_stands_upright_in_the_middle_of_its_column() {
    // East Asian characters stand upright, others turn a quarter clockwise. The letter, the Han character and the letter make three runs.
    let font = SynthFont::new("Em").ascii(1000).chars("\u{4E2D}", 1000);
    let db = db(&[font]);
    let laid = lay_text(&db, "a\u{4E2D}b", &tenth(), &left(), &block(2.0, 1), Some(1.0));
    let line = &laid.lines[0];
    assert_eq!(line.runs.iter().map(|r| r.turned).collect::<Vec<_>>(), [true, false, true]);
    let han = &line.runs[1].glyphs[0];
    // Centred across the column: the advance box is centred on the column's centre line (x 0.5), and down the column in its cell, whose centre
    // is where the em box (ascent 0.8, descent 0.2 of the em) centres: the origin is 0.3 em under it.
    close(han.x, 0.5 - 0.05);
    close(han.y, 2.0 - 0.1 - 0.05 - 0.03);
    close(han.advance, 0.1);
    // The letter after it carries on down the column.
    close(line.runs[2].glyphs[0].y, 2.0 - 0.2);
}

#[test]
fn a_bullet_turns_with_the_text_and_hangs_at_the_top() {
    // A bullet is drawn like the letters, at the start of the first column, and pushes the text down by `TextPosAfterBullet`.
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let db = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, halign: 0, text_pos_after_bullet: 0.25, ..ParaFormat::default() };
    let laid = lay_text(&db, "ab", &tenth(), &p, &block(2.0, 0), Some(1.0));
    let line = &laid.lines[0];
    let bullet = line.bullet.as_ref().expect("a bullet");
    assert!(bullet.turned);
    close(bullet.glyphs[0].y, 2.0);
    close(origins(line)[0].1, 2.0 - 0.25);
}

#[test]
fn an_underline_lies_down_the_left_of_the_column() {
    // The line under sideways text is on the baseline side of the letters, which is the left; it runs the length of the text. Its place and its
    // thickness are the face's, as across: taken from the same text laid out across.
    let c = CharFormat { style: style::UNDERLINE, ..tenth() };
    let across = lay_text(&em_db(), "abc", &c, &left(), &BlockFormat { height: Some(2.0), ..BlockFormat::default() }, Some(2.0));
    let flat = across.lines[0].decorations.iter().find(|d| d.kind == DecorationKind::Underline).expect("an underline");
    assert!(!flat.vertical);
    let below = flat.top - across.lines[0].baseline;
    assert!(below < 0.0, "an underline lies under the baseline: {below}");
    let laid = lay_text(&em_db(), "abc", &c, &left(), &block(2.0, 1), Some(1.0));
    let line = &laid.lines[0];
    let d = line.decorations.iter().find(|d| d.kind == DecorationKind::Underline).expect("an underline");
    assert!(d.vertical);
    let r = d.rect();
    close(r.x1, line.baseline + below);
    close(r.x1 - r.x0, flat.thickness);
    close(r.y1, 2.0);
    close(r.y0, 2.0 - 0.3);
}

#[test]
fn a_text_background_hugs_the_columns() {
    let b = BlockFormat { background: Some((Rgba::BLACK, 0.0)), ..block(2.0, 1) };
    let laid = lay_text(&em_db(), "abc", &tenth(), &left(), &b, Some(1.0));
    let r = laid.background.expect("a background").rect;
    close(r.x0, 0.44);
    close(r.x1, 0.56);
    close(r.y1, 2.0);
    close(r.y0, 1.7);
}

#[test]
fn the_height_left_out_lays_the_columns_to_their_own_length() {
    // With no height the block is as tall as its longest column, with nothing to wrap against, and the text is where it would be.
    let b = BlockFormat { height: None, ..block(2.0, 0) };
    let laid = lay_text(&em_db(), "abc", &tenth(), &left(), &b, Some(1.0));
    assert_eq!(laid.lines.len(), 1);
    let at = origins(&laid.lines[0]);
    // The top of the block is the column's length below its bottom: the first letter is at 0.3 and the others follow down.
    close(at[0].1, 0.3);
    close(at[2].1, 0.1);
    assert!(laid.notes.is_empty(), "{:?}", laid.notes);
}

#[test]
fn nothing_in_a_vertical_layout_says_it_was_not_laid_out() {
    let laid = lay("abc", &left(), &tenth(), 1.0, 2.0, 1);
    // A layout of text down the block says nothing about the direction: it was laid out as it was asked.
    assert!(laid.notes.is_empty(), "{:?}", laid.notes);
}

#[test]
fn hostile_sizes_are_laid_out_and_say_so() {
    // A height or a width that is not a number, an absurd one, or one of no room: laid out, never a panic, with the note.
    for (w, h) in [(f64::NAN, 2.0), (1.0, f64::NAN), (f64::INFINITY, 1.0), (-1.0, -1.0), (0.0, 0.0), (1.0e300, 1.0e300), (1.0, 1.0e-300)] {
        let b = BlockFormat { text_direction: 1, height: Some(h), ..BlockFormat::default() };
        let laid = lay_out(&[run("hello world", &tenth(), &left())], &b, Some(w), &em_db());
        assert!(!laid.lines.is_empty());
        assert_eq!(
            laid.notes.contains(&Note::BadNumber),
            w.is_nan() || h.is_nan() || w.abs() > 1.0e7 || h.abs() > 1.0e7 || w.is_infinite(),
            "{w} {h}"
        );
        for line in &laid.lines {
            for r in &line.runs {
                for g in &r.glyphs {
                    assert!(g.x.is_finite() && g.y.is_finite(), "{w} {h}");
                }
            }
        }
    }
}

#[test]
fn a_long_text_costs_its_length_and_no_more() {
    // 200,000 letters in columns of a hundred: every glyph is placed once. The counted bound is the glyphs and the lines, and nothing is lost.
    let text = "a".repeat(200_000);
    let b = BlockFormat { text_direction: 1, height: Some(10.0), vertical_align: 0, ..BlockFormat::default() };
    let laid = lay_out(&[run(&text, &tenth(), &left())], &b, Some(1.0), &em_db());
    let glyphs: usize = laid.lines.iter().map(|l| l.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()).sum();
    assert_eq!(glyphs, 200_000);
    assert_eq!(laid.lines.len(), 2_000);
    // Sideways text of one face and one size is one run to a column however long it is.
    assert!(laid.lines.iter().all(|l| l.runs.len() == 1));
}

/// `n` characters alternating a letter and a Han character, in a face that has both, laid out down a block with no height to wrap against: one
/// column of one run, every glyph of it turned or standing by its own character.
fn alternating(n: usize) -> (diacraft_fonts::FontDb, String, BlockFormat) {
    let font = SynthFont::new("Em").ascii(1000).chars("\u{4E2D}", 1000);
    let text: String = (0..n).map(|i| if i % 2 == 0 { 'a' } else { '\u{4E2D}' }).collect();
    (db(&[font]), text, BlockFormat { text_direction: 1, height: None, vertical_align: 0, ..BlockFormat::default() })
}

#[test]
fn text_that_alternates_the_way_up_copies_each_glyph_once() {
    // The turn takes the glyphs of a run into new runs whenever the way up changes. A run copied whole for each change copies all its glyphs again and
    // again: letters and Han characters alternating make a change at every character, and the copies grow with the square of the text. Counted:
    // one copy for each glyph placed, and none for the runs made to hold them.
    let n = 20_000;
    let (db, text, block) = alternating(n);
    diacraft_text::work::reset();
    let laid = lay_out(&[run(&text, &tenth(), &left())], &block, Some(1.0), &db);
    let copies = diacraft_text::work::glyph_copies();
    assert_eq!(laid.lines.len(), 1);
    let line = &laid.lines[0];
    assert_eq!(line.runs.len(), n, "a run for every change of the way up");
    assert_eq!(line.runs.iter().filter(|r| r.turned).count(), n / 2);
    assert_eq!(copies, n as u64, "{copies} glyphs copied for {n} glyphs");
}

#[test]
#[ignore = "a real-time check, to run by hand: cargo test -p diacraft-text --test it -- --ignored alternating"]
fn alternating_text_of_the_most_characters_costs_about_what_all_letters_do() {
    // The yardstick is the same text all of letters, which makes one run: it is laid out and turned in a time that grows with the text. The
    // alternating text must take about the same, and not the square of it.
    let n = diacraft_text::limits::MAX_CHARS;
    let (db, text, block) = alternating(n);
    let letters = "a".repeat(n);
    let time = |text: &str| {
        let started = std::time::Instant::now();
        let laid = lay_out(&[run(text, &tenth(), &left())], &block, Some(1.0), &db);
        assert_eq!(laid.lines.iter().map(|l| l.runs.iter().map(|r| r.glyphs.len()).sum::<usize>()).sum::<usize>(), n);
        started.elapsed()
    };
    let yardstick = time(&letters);
    let alternating = time(&text);
    assert!(alternating < yardstick * 5 + std::time::Duration::from_millis(200), "alternating {alternating:?} against {yardstick:?}");
}
