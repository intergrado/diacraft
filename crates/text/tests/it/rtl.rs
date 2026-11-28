//! A right-to-left paragraph mirrors (design call the first half), and
//! Japanese and Korean vertical text is underlined on the right (design call for those two languages).
//!
//! A paragraph whose `Flags` is 1 is the mirror image of a left-to-right one: its start indent (`IndLeft`) is on the right and its stop indent
//! (`IndRight`) on the left, the first line's indent is counted from the start indent, alignment 0 puts a line against the right and 2 against the left,
//! a bullet stands at the right and pushes the text left, and tab stops are measured from the right edge of the text area with each alignment mirrored.
//! Every number below is written out from the rule, and the geometry read back is the glyphs' places, not the line's own `x`. Letters are 0.1 inch wide.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, DecorationKind, Laid, Line, ParaFormat, TabStop, lay_out, style};

use crate::{close, db, em_db, lay_text, left, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, font: "Em".into(), ..CharFormat::default() }
}

/// A right-to-left paragraph aligned by `halign`.
fn rtl(halign: u8) -> ParaFormat {
    ParaFormat { flags: 1, halign, ..ParaFormat::default() }
}

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn lay(db: &FontDb, text: &str, p: &ParaFormat, block: &BlockFormat, width: Option<f64>) -> Laid {
    lay_text(db, text, &tenth(), p, block, width)
}

/// The leftmost and rightmost edges of the glyphs a line draws: `(left, right)`.
fn extent(line: &Line) -> (f64, f64) {
    let glyphs = line.runs.iter().flat_map(|r| r.glyphs.iter());
    glyphs.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, r), g| (l.min(g.x), r.max(g.x + g.advance)))
}

/// The x of every glyph of a line, in drawing order.
fn xs(line: &Line) -> Vec<f64> {
    line.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect()
}

#[track_caller]
fn spans(line: &Line, left: f64, right: f64) {
    let (l, r) = extent(line);
    close(l, left);
    close(r, right);
    // The line says the same as the glyphs do.
    close(line.x, left);
    close(line.x + line.width, right);
}

/// The paragraph: a block 3 inches wide with margins 0, `Flags` `flags`, `IndLeft` 0.25 inch, and one line 1 inch long.
fn v22(flags: u8, halign: u8) -> Laid {
    let p = ParaFormat { flags, halign, ind_left: 0.25, ..ParaFormat::default() };
    lay(&em_db(), "aaaaaaaaaa", &p, &top_block(), Some(3.0))
}

#[test]
fn v22_alignment_0_puts_the_line_against_the_start_indent_on_the_right() {
    // From 1.75 to 2.75: the start indent 0.25 in from the right edge. The old rule gives 0.25 to 1.25.
    spans(&v22(1, 0).lines[0], 1.75, 2.75);
}

#[test]
fn v22_alignment_2_puts_the_line_against_the_left_edge() {
    // From 0 to 1: the stop indent is 0. The old rule gives 2 to 3.
    spans(&v22(1, 2).lines[0], 0.0, 1.0);
}

#[test]
fn v22_alignment_1_centres_the_line_between_the_stop_edge_and_the_start_indent() {
    // Between 0 and 2.75: from 0.875 to 1.875. The old rule gives 1.125 to 2.125.
    spans(&v22(1, 1).lines[0], 0.875, 1.875);
}

#[test]
fn v23_flags_0_is_unchanged() {
    // The same paragraph left to right: the line from 0.25 to 1.25, whatever else changes. Mirroring every paragraph gives 1.75 to 2.75.
    spans(&v22(0, 0).lines[0], 0.25, 1.25);
    spans(&v22(0, 2).lines[0], 2.0, 3.0);
    spans(&v22(0, 1).lines[0], 1.125, 2.125);
}

/// The paragraph: a block 3 inches wide with margins 0, `Flags` 1, `IndLeft` 0, `IndRight` 0.25, `IndFirst` 0.5 and two lines of 1 inch (a line break between them).
fn v74(halign: u8) -> Laid {
    let p = ParaFormat { ind_right: 0.25, ind_first: 0.5, ..rtl(halign) };
    lay(&em_db(), "aaaaaaaaaa\u{2028}aaaaaaaaaa", &p, &top_block(), Some(3.0))
}

#[test]
fn v74_the_first_line_indent_and_the_stop_indent_of_two_lines() {
    // `HorzAlign` 0: the first line from 1.5 to 2.5 (0.5 in from the right edge) and the second from 2 to 3. Naive: the first-line indent taken from the left.
    let laid = v74(0);
    assert_eq!(laid.lines.len(), 2);
    spans(&laid.lines[0], 1.5, 2.5);
    spans(&laid.lines[1], 2.0, 3.0);
    // `HorzAlign` 2: both from 0.25 to 1.25, the stop indent in from the left edge, and the first-line indent no part of it.
    let laid = v74(2);
    spans(&laid.lines[0], 0.25, 1.25);
    spans(&laid.lines[1], 0.25, 1.25);
}

#[test]
fn v74_a_left_tab_stop_of_1_inch_puts_the_text_after_the_tab_with_its_right_edge_on_the_stop() {
    // `A` TAB `B`, a stop at 1 inch of alignment 0, indents 0: `A`'s right edge at 3 and `B`'s at 2. Naive: tab stops measured from the left edge.
    let laid = tabbed(&[(1.0, 0)], 0.5, "A\tB", 0.0);
    let x = xs(&laid.lines[0]);
    assert_eq!(x.len(), 2);
    // Drawn from the left: `B`, then `A`.
    close(x[0] + 0.1, 2.0);
    close(x[1] + 0.1, 3.0);
}

#[test]
fn e11_flags_other_than_0_and_1_are_drawn_left_to_right() {
    for flags in [2u8, 3, 255] {
        spans(&v22(flags, 0).lines[0], 0.25, 1.25);
        assert!(!v22(flags, 0).lines[0].rtl, "Flags {flags}");
    }
    // The letters too: a Latin letter first in a paragraph of Flags 2 is on the left, and the Hebrew letters after it reverse among themselves.
    let f = SynthFont::new("Mixed").ascii(1000).chars("אב", 1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let p = ParaFormat { flags: 2, halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&d, "aאב", &c, &p, &top_block(), Some(3.0));
    assert_eq!(crate::gids(&laid.lines[0]), vec![u32::from(f.gid('a')), u32::from(f.gid('ב')), u32::from(f.gid('א'))]);
    close(extent(&laid.lines[0]).0, 0.0);
}

#[test]
fn the_blocks_margins_stay_on_their_own_sides() {
    // The margins are the block's, not the paragraph's: the right margin keeps the start side of a right-to-left paragraph in from the right edge and the left margin
    // keeps its stop side in from the left (design 10.1: from the left margin plus `IndRight` to the width less the right margin and `IndLeft`). Alignment 0 ends
    // at 3 - 0.2, alignment 2 starts at 0.1, and the left-to-right paragraph has the same margins at the same edges.
    let block = BlockFormat { left_margin: 0.1, right_margin: 0.2, ..top_block() };
    spans(&lay(&em_db(), "aaaaa", &rtl(0), &block, Some(3.0)).lines[0], 2.8 - 0.5, 2.8);
    spans(&lay(&em_db(), "aaaaa", &rtl(2), &block, Some(3.0)).lines[0], 0.1, 0.6);
    // Left to right, the same block puts them the other way round.
    spans(&lay(&em_db(), "aaaaa", &left(), &block, Some(3.0)).lines[0], 0.1, 0.6);
}

#[test]
fn the_stop_indent_is_on_the_left() {
    // `IndRight` 0.5 keeps a line aligned to the left 0.5 in from the left edge, and takes room from the wrapping: 3 - 0.5 - 0.25 = 2.25 inches.
    let p = ParaFormat { ind_left: 0.25, ind_right: 0.5, ..rtl(2) };
    spans(&lay(&em_db(), "aaaaa", &p, &top_block(), Some(3.0)).lines[0], 0.5, 1.0);
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk";
    let laid = lay(&em_db(), text, &p, &top_block(), Some(3.0));
    assert!(laid.lines.len() >= 2);
    for l in &laid.lines {
        assert!(l.width <= 2.25 + 1e-9, "{}", l.width);
    }
    // The same room as the left-to-right paragraph with the same indents: the lines break at the same places.
    let ltr = lay(&em_db(), text, &ParaFormat { flags: 0, ..p.clone() }, &top_block(), Some(3.0));
    assert_eq!(laid.lines.iter().map(|l| l.chars.clone()).collect::<Vec<_>>(), ltr.lines.iter().map(|l| l.chars.clone()).collect::<Vec<_>>());
}

#[test]
fn the_first_line_indent_is_counted_in_from_the_start_indent() {
    // Width 2, `IndLeft` 0.25, `IndFirst` 0.5: the first line starts 0.75 in from the right, and the room for it is 1.25, so "aaaa bbbb" (0.9) is its
    // line, and the later lines start 0.25 in from the right edge.
    let p = ParaFormat { ind_left: 0.25, ind_first: 0.5, ..rtl(0) };
    let laid = lay(&em_db(), "aaaa bbbb cccc dddd eeee ffff", &p, &top_block(), Some(2.0));
    assert_eq!(laid.lines.len(), 3);
    spans(&laid.lines[0], 2.0 - 0.75 - 0.9, 2.0 - 0.75);
    spans(&laid.lines[1], 2.0 - 0.25 - 1.4, 2.0 - 0.25);
    spans(&laid.lines[2], 2.0 - 0.25 - 0.4, 2.0 - 0.25);
}

#[test]
fn a_negative_first_line_indent_hangs_the_first_line_toward_the_right_edge_and_stays_in_the_block() {
    let p = ParaFormat { ind_left: 0.25, ind_first: -0.25, ..rtl(0) };
    let laid = lay(&em_db(), "aaaa bbbb cccc dddd", &p, &top_block(), Some(1.0));
    assert!(laid.lines.len() >= 2);
    close(extent(&laid.lines[0]).1, 1.0);
    for l in &laid.lines[1..] {
        close(extent(l).1, 0.75);
    }
    // Mirrored: `IndFirst` -1 at `IndLeft` 0.25 starts the first line at the block's right edge, not 0.75 inch outside it.
    let out = ParaFormat { ind_left: 0.25, ind_first: -1.0, ..rtl(0) };
    close(extent(&lay(&em_db(), "aa", &out, &top_block(), Some(1.0)).lines[0]).1, 1.0);
}

#[test]
fn a_stop_indent_too_large_for_the_room_gives_way_to_the_start_indent() {
    // Mirrored: no room at all, so a letter to a line, each starting at the start indent 0.25 in from the right edge, every letter drawn.
    let p = ParaFormat { ind_left: 0.25, ind_right: 5.0, ..rtl(0) };
    let laid = lay(&em_db(), "abc", &p, &top_block(), Some(1.0));
    assert_eq!(laid.lines.len(), 3);
    for l in &laid.lines {
        close(extent(l).1, 0.75);
    }
}

#[test]
fn a_justified_paragraph_stretches_its_lines_and_its_last_line_sits_against_the_right() {
    // An Arabic paragraph of `HorzAlign` 6 and two lines. The first line fills the block's width; the last keeps its natural width, as alignment 3
    // does, and sits against the start edge, the right: 0.9 wide, from 0.1 to 1.0. Left alignment on it is the naive answer.
    let f = SynthFont::new("Ar").ascii(1000).chars("ابتثجحخد", 1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Ar".into(), ..CharFormat::default() };
    for halign in [3u8, 6, 7, 8] {
        let laid = lay_text(&d, "ابتث جحخد ابتث جحخد", &c, &rtl(halign), &top_block(), Some(1.0));
        assert_eq!(laid.lines.len(), 2, "HorzAlign {halign}");
        spans(&laid.lines[0], 0.0, 1.0);
        spans(&laid.lines[1], 0.1, 1.0);
        // The letters of the last line are drawn between its edges, each word reversed.
        let (l, r) = extent(&laid.lines[1]);
        close(l, 0.1);
        close(r, 1.0);
    }
    // 4 stretches every line, the last too.
    let laid = lay_text(&d, "ابتث جحخد ابتث جحخد", &c, &rtl(4), &top_block(), Some(1.0));
    spans(&laid.lines[1], 0.0, 1.0);
}

#[test]
fn a_bullet_stands_at_the_start_on_the_right_and_pushes_the_text_left() {
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, text_pos_after_bullet: 0.25, ..rtl(0) };
    let laid = lay(&d, "aaaa bbbb cccc dddd", &p, &top_block(), Some(1.0));
    assert!(laid.lines.len() >= 2, "{} lines", laid.lines.len());
    let first = &laid.lines[0];
    let bullet = first.bullet.as_ref().expect("a bullet");
    let mark = &bullet.glyphs[0];
    // The bullet is 0.1 wide and its right edge is the start edge: 1.0. The text starts 0.25 from the bullet's start: its right edge at 0.75.
    close(mark.x + mark.advance, 1.0);
    close(extent(first).1, 0.75);
    // The later lines have no bullet and start at the edge.
    assert!(laid.lines[1].bullet.is_none());
    close(extent(&laid.lines[1]).1, 1.0);
    // The text never goes inside the bullet: a gap smaller than the bullet puts the text straight after it, at 1.0 - 0.1.
    let tight = ParaFormat { bullet: 1, text_pos_after_bullet: 0.01, ..rtl(0) };
    let laid = lay(&d, "aaaa", &tight, &top_block(), Some(1.0));
    close(extent(&laid.lines[0]).1, 0.9);
    // The same paragraph left to right has its bullet at the left, as before.
    let ltr = ParaFormat { flags: 0, ..p };
    let laid = lay(&d, "aaaa", &ltr, &top_block(), Some(1.0));
    close(laid.lines[0].bullet.as_ref().expect("a bullet").glyphs[0].x, 0.0);
    close(extent(&laid.lines[0]).0, 0.25);
}

#[test]
fn a_text_background_takes_in_the_bullet_on_the_right() {
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, text_pos_after_bullet: 0.25, ..rtl(0) };
    let block = BlockFormat { background: Some((diacraft_text::Rgba::BLACK, 0.0)), ..top_block() };
    let laid = lay(&d, "aaaa", &p, &block, Some(2.0));
    let r = laid.background.expect("a background").rect;
    // The text 0.4 wide ends at 2 - 0.25, and the bullet, 0.1 wide, is at 1.9 to 2: the rectangle hugs both.
    close(r.x0, 2.0 - 0.25 - 0.4);
    close(r.x1, 2.0);
}

/// `a`, a tab and `b` in a right-to-left paragraph with these stops, in a block 3 wide with the given margins and a default step of `step`.
fn tabbed(stops: &[(f64, u8)], step: f64, text: &str, right_margin: f64) -> Laid {
    let p = ParaFormat { tabs: stops.iter().map(|(position, alignment)| TabStop { position: *position, alignment: *alignment }).collect(), ..rtl(0) };
    let block = BlockFormat { default_tab_stop: step, right_margin, ..top_block() };
    lay(&em_db(), text, &p, &block, Some(3.0))
}

#[test]
fn a_tab_stop_is_measured_from_the_right_edge() {
    // `a`, a tab, `b` with a left stop at 1.0: `a` is first, so it stands at the right, and `b` starts at the stop, 1.0 in from the right edge, and runs
    // left: its right edge at 2.0, its glyph at 1.9. Drawn from the left: `b` then `a`.
    let laid = tabbed(&[(1.0, 0)], 0.5, "a\tb", 0.0);
    let x = xs(&laid.lines[0]);
    assert_eq!(x.len(), 2);
    close(x[1], 3.0 - 0.1);
    close(x[0], 3.0 - 1.0 - 0.1);
    // Measured from the text area's edge, inside the margin: the margin is 0.1.
    let x = xs(&tabbed(&[(1.0, 0)], 0.5, "a\tb", 0.1).lines[0]);
    close(x[1], 3.0 - 0.1 - 0.1);
    close(x[0], 3.0 - 0.1 - 1.0 - 0.1);
}

#[test]
fn the_default_steps_of_a_right_to_left_paragraph_count_from_the_right_edge() {
    // With no custom stop the block's step of 0.5 applies from the right edge: `b` at 0.5 in, and a second tab goes to the next step, 1.0.
    let x = xs(&tabbed(&[], 0.5, "a\tb\tc", 0.0).lines[0]);
    assert_eq!(x.len(), 3);
    close(x[2], 3.0 - 0.1);
    close(x[1], 3.0 - 0.5 - 0.1);
    close(x[0], 3.0 - 1.0 - 0.1);
}

#[test]
fn a_tab_stops_alignment_mirrors() {
    // Position 2.0, in from the right edge. Alignment 0 ("left") becomes right: the text starts at the stop and runs left, so its right edge is on it.
    let (left_stop, centre, right_stop) = (
        extent(&tabbed(&[(2.0, 0)], 0.5, "\tabcd", 0.0).lines[0]),
        extent(&tabbed(&[(2.0, 1)], 0.5, "\tabcd", 0.0).lines[0]),
        extent(&tabbed(&[(2.0, 2)], 0.5, "\tabcd", 0.0).lines[0]),
    );
    close(left_stop.1, 3.0 - 2.0);
    close(left_stop.0, 3.0 - 2.0 - 0.4);
    // Centred on the stop.
    close((centre.0 + centre.1) / 2.0, 3.0 - 2.0);
    // Alignment 2 ("right") becomes left: the text ends at the stop, running away from the start, so its left edge is on it.
    close(right_stop.0, 3.0 - 2.0);
    close(right_stop.1, 3.0 - 2.0 + 0.4);
}

#[test]
fn a_tab_never_moves_the_pen_backwards_in_a_right_to_left_paragraph() {
    // A right stop at 1.0 for four letters, with the pen already at 0.8: the text would start at 0.6, behind the pen, so it starts at the pen and the tab
    // is no width: the line is the twelve letters side by side, 1.2 wide, from the right edge.
    let laid = tabbed(&[(1.0, 2)], 0.5, "aaaaaaaa\tbbbb", 0.0);
    assert_eq!(xs(&laid.lines[0]).len(), 12);
    spans(&laid.lines[0], 3.0 - 1.2, 3.0);
}

#[test]
fn a_paragraph_of_each_direction_in_one_block_is_aligned_by_its_own() {
    let c = tenth();
    let (a, b) = (left(), rtl(0));
    let laid = lay_out(&[run("aaaa\n", &c, &a), run("aaaa\n", &c, &b)], &top_block(), Some(2.0), &em_db());
    assert_eq!(laid.lines.len(), 2);
    spans(&laid.lines[0], 0.0, 0.4);
    spans(&laid.lines[1], 1.6, 2.0);
}

#[test]
fn a_block_as_wide_as_its_widest_line_mirrors_within_it() {
    // No width: the block is as wide as its longest line, 0.4, and the short line, aligned to the start, sits against its right.
    let c = tenth();
    let laid = lay_out(&[run("aaaa\n", &c, &rtl(0)), run("aa\n", &c, &rtl(0))], &top_block(), None, &em_db());
    spans(&laid.lines[0], 0.0, 0.4);
    spans(&laid.lines[1], 0.2, 0.4);
    // The size of a text does not depend on its direction (TEXTWIDTH is the widest line).
    let ltr = lay_out(&[run("aaaa\n", &c, &left()), run("aa\n", &c, &left())], &top_block(), None, &em_db());
    close(laid.size.width, ltr.size.width);
    close(laid.size.height, ltr.size.height);
}

#[test]
fn a_right_to_left_paragraph_breaks_where_the_left_to_right_one_does() {
    let text = "aaaa bbbb cccc dddd eeee ffff";
    let (a, b) =
        (lay(&em_db(), text, &ParaFormat { halign: 3, ..left() }, &top_block(), Some(1.1)), lay(&em_db(), text, &rtl(3), &top_block(), Some(1.1)));
    assert_eq!(a.lines.iter().map(|l| l.chars.clone()).collect::<Vec<_>>(), b.lines.iter().map(|l| l.chars.clone()).collect::<Vec<_>>());
    close(a.size.width, b.size.width);
}

#[test]
fn hostile_widths_and_indents_lay_a_mirrored_paragraph_out_with_finite_places() {
    let numbers = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1.0e-300, 1.0e300, 1.0e7];
    for w in numbers.iter().map(|w| Some(*w)).chain([None]) {
        for n in numbers {
            let p = ParaFormat { ind_left: n, ind_right: n, ind_first: n, text_pos_after_bullet: n, bullet: 1, ..rtl(0) };
            let block = BlockFormat { left_margin: n, right_margin: n, ..top_block() };
            for halign in [0u8, 1, 2, 3, 4] {
                let p = ParaFormat { halign, ..p.clone() };
                let laid = lay(&em_db(), "aaaa bbbb cccc", &p, &block, w);
                assert!(!laid.lines.is_empty());
                for line in &laid.lines {
                    assert!(line.x.is_finite() && line.width.is_finite(), "{w:?} {n}");
                    for r in line.runs.iter().chain(line.bullet.iter()) {
                        assert!(r.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite()), "{w:?} {n}");
                    }
                }
            }
        }
    }
}

// ---- Japanese and Korean vertical text: the underline on the right

fn vertical_block() -> BlockFormat {
    BlockFormat { text_direction: 1, vertical_align: 1, height: Some(2.0), ..BlockFormat::default() }
}

/// A face with Latin letters, two Han characters and two Hangul syllables, each 1,000 units wide.
fn east_asian_db() -> FontDb {
    db(&[SynthFont::new("Em").ascii(1000).chars("\u{65E5}\u{672C}\u{D55C}\u{AE00}", 1000)])
}

fn underlined(lang: &str) -> CharFormat {
    CharFormat { style: style::UNDERLINE, lang: lang.to_owned(), ..tenth() }
}

/// The underlines of the first column, as rectangles, and the column's centre line.
fn underlines(laid: &Laid, kind: DecorationKind) -> (Vec<kurbo::Rect>, f64) {
    let line = &laid.lines[0];
    assert!(line.vertical);
    let rects = line.decorations.iter().filter(|d| d.kind == kind).map(diacraft_text::Decoration::rect).collect();
    (rects, (line.top + line.bottom) / 2.0)
}

#[test]
fn a_japanese_underline_lies_on_the_right_of_the_column() {
    // Two Han characters at 0.1 inch, in a block 1 wide and 2 tall, `VerticalAlign` 1: the column's centre line is x 0.5 and the characters' cells reach
    // 0.05 either side of it. The underline's inner edge is at the cells' right edge, 0.55, and it is 0.005 thick (a 0.05 em line at 0.1 inch).
    for lang in ["ja-JP", "ja", "JA-jp", "ja_JP", "jpn"] {
        let laid = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &underlined(lang), &left(), &vertical_block(), Some(1.0));
        let (rects, centre) = underlines(&laid, DecorationKind::Underline);
        assert_eq!(rects.len(), 1, "{lang}");
        close(centre, 0.5);
        close(rects[0].x0, 0.55);
        close(rects[0].x1, 0.555);
        // It runs the length of the text: from the top, 2.0, down 0.2.
        close(rects[0].y1, 2.0);
        close(rects[0].y0, 1.8);
    }
}

#[test]
fn a_korean_underline_lies_on_the_right_of_the_column_too() {
    for lang in ["ko-KR", "ko", "kor"] {
        let laid = lay_text(&east_asian_db(), "\u{D55C}\u{AE00}", &underlined(lang), &left(), &vertical_block(), Some(1.0));
        let (rects, _) = underlines(&laid, DecorationKind::Underline);
        assert_eq!(rects.len(), 1, "{lang}");
        close(rects[0].x0, 0.55);
    }
}

#[test]
fn an_english_run_keeps_the_underline_on_the_baseline_side() {
    // Stands for every other language: the same Han characters in an `en-US` run are underlined on the left, where the baseline is, and a run with
    // no language at all too.
    for lang in ["en-US", "zh-CN", "x-none", "", "de"] {
        let laid = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &underlined(lang), &left(), &vertical_block(), Some(1.0));
        let (rects, centre) = underlines(&laid, DecorationKind::Underline);
        assert_eq!(rects.len(), 1, "{lang:?}");
        assert!(rects[0].x1 < centre, "{lang:?}: {:?}", rects[0]);
        close(rects[0].x1, laid.lines[0].baseline - 0.01);
    }
}

#[test]
fn a_double_underline_goes_on_the_right_as_well_with_its_two_lines_apart() {
    let c = CharFormat { style: 0, double_underline: true, lang: "ja-JP".into(), ..tenth() };
    let laid = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &c, &left(), &vertical_block(), Some(1.0));
    let (mut rects, _) = underlines(&laid, DecorationKind::DoubleUnderline);
    assert_eq!(rects.len(), 2);
    rects.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    // The first line is on the cells' edge and the second a line's thickness beyond the gap of one thickness: 0.55 and 0.56.
    close(rects[0].x0, 0.55);
    close(rects[1].x0, 0.56);
    close(rects[0].x1 - rects[0].x0, 0.005);
    close(rects[1].x1 - rects[1].x0, 0.005);
}

#[test]
fn only_the_underline_moves_to_the_right() {
    let c = CharFormat { style: style::UNDERLINE, overline: true, strikethrough: true, lang: "ja-JP".into(), ..tenth() };
    let laid = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &c, &left(), &vertical_block(), Some(1.0));
    let en = CharFormat { lang: "en-US".into(), ..c };
    let other = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &en, &left(), &vertical_block(), Some(1.0));
    for kind in [DecorationKind::Overline, DecorationKind::Strikethrough] {
        assert_eq!(underlines(&laid, kind).0, underlines(&other, kind).0, "{kind:?}");
    }
}

#[test]
fn a_run_changing_language_in_one_underline_splits_it_where_the_language_changes() {
    // Two runs of underlined text: the Japanese one first, then an English one of Latin letters. The first is underlined on the right and the second on
    // the left, each along its own letters: the first 0.2 of the column and the next 0.2.
    let (ja, en) = (underlined("ja-JP"), underlined("en-US"));
    let laid = lay_out(&[run("\u{65E5}\u{672C}", &ja, &left()), run("ab", &en, &left())], &vertical_block(), Some(1.0), &east_asian_db());
    let line = &laid.lines[0];
    let mut rects: Vec<kurbo::Rect> = line.decorations.iter().filter(|d| d.kind == DecorationKind::Underline).map(|d| d.rect()).collect();
    rects.sort_by(|a, b| b.y1.total_cmp(&a.y1));
    assert_eq!(rects.len(), 2);
    close(rects[0].x0, 0.55);
    close(rects[0].y1, 2.0);
    close(rects[0].y0, 1.8);
    assert!(rects[1].x1 < 0.5);
    close(rects[1].y1, 1.8);
    close(rects[1].y0, 1.6);
}

#[test]
fn a_japanese_underline_across_the_block_is_where_it_always_was() {
    // Horizontal text is unchanged: below the baseline whatever the language, in one piece across a change of language, and a run is not split for it.
    let (ja, en) = (underlined("ja-JP"), underlined("en-US"));
    let block = BlockFormat { vertical_align: 0, height: Some(1.0), ..BlockFormat::default() };
    let japanese = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &ja, &left(), &block, Some(2.0));
    let english = lay_text(&east_asian_db(), "\u{65E5}\u{672C}", &en, &left(), &block, Some(2.0));
    let one = |laid: &Laid| laid.lines[0].decorations.iter().map(|d| (d.kind, d.x0, d.x1, d.top, d.thickness, d.vertical)).collect::<Vec<_>>();
    assert_eq!(one(&japanese), one(&english));
    assert_eq!(one(&japanese).len(), 1);
    let mixed = lay_out(&[run("\u{65E5}\u{672C}", &ja, &left()), run("ab", &en, &left())], &block, Some(2.0), &east_asian_db());
    assert_eq!(mixed.lines[0].decorations.len(), 1);
    assert_eq!(mixed.lines[0].runs.len(), 1);
}

#[test]
fn a_right_to_left_paragraph_in_vertical_text_mirrors_along_its_column() {
    // Not in the specification, and ours: vertical text is the layout across, turned, so a right-to-left paragraph mirrors along the column. Aligned
    // 0, it is against the start, which is the far end of the column, the bottom: four letters of 0.1 inch in a block 2 inches tall fill 0.4 up from its foot.
    // A left-to-right paragraph hangs from the top as before.
    let ys = |p: &ParaFormat| {
        let laid = lay_text(&em_db(), "aaaa", &tenth(), p, &vertical_block(), Some(1.0));
        laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.y)).collect::<Vec<f64>>()
    };
    let down = ys(&rtl(0));
    assert_eq!(down.len(), 4);
    close(down.iter().copied().fold(f64::NEG_INFINITY, f64::max), 0.4);
    close(down.iter().copied().fold(f64::INFINITY, f64::min), 0.1);
    let hanging = ys(&left());
    close(hanging.iter().copied().fold(f64::NEG_INFINITY, f64::max), 2.0);
}

#[test]
fn a_mirrored_paragraph_costs_what_the_same_one_left_to_right_does() {
    // Counted, not timed: the mirror is a change of where a line stands once it is broken, so it looks at every cluster as often as the unmirrored paragraph
    // does, tabs and bullets and wrapping included: 5,000 tabs, each looking along the text after it up to the next, and 100,000 words wrapped in a line of 1 inch.
    let work_of = |p: &ParaFormat, text: &str, width: f64| {
        diacraft_text::work::reset();
        let laid = lay(&em_db(), text, p, &top_block(), Some(width));
        (laid.lines.len(), diacraft_text::work::cluster_steps())
    };
    let tabs = vec![TabStop { position: 0.5, alignment: 1 }, TabStop { position: 1.0, alignment: 3 }, TabStop { position: 1.5, alignment: 2 }];
    let text = "a\t".repeat(5000);
    let ltr = work_of(&ParaFormat { tabs: tabs.clone(), bullet: 1, ..left() }, &text, 10.0);
    let mirrored = work_of(&ParaFormat { tabs, bullet: 1, ..rtl(0) }, &text, 10.0);
    assert_eq!(ltr, mirrored);
    assert!((10_000..=30_000).contains(&mirrored.1), "{} looks at 10,000 clusters", mirrored.1);
    let words = "ab ".repeat(100_000);
    let ltr = work_of(&ParaFormat { halign: 3, ..left() }, &words, 1.0);
    let mirrored = work_of(&rtl(3), &words, 1.0);
    assert_eq!(ltr, mirrored);
    assert!(mirrored.0 > 10_000, "{} lines", mirrored.0);
}
