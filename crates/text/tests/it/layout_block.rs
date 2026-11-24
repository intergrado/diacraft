//! Stacking lines in the block, the margin vectors, vertical alignment and the
//! background. Is not here: it turns drawing inches into paper inches before `lay_out` is called, which is the caller's
//! (Task 8's) work, as are V15 (vertical text, laid out across with a note) and V16 (the order things are drawn in); see
//! `crates/text/VECTORS.md`. Blocks are 2 inches by 1 inch with margins of 4 pt, size 12 pt and `SpLine` -1.2, so a line box is 0.2
//! inch, unless a vector says otherwise. `y` is height from the block's bottom.

use diacraft_text::{BlockFormat, ParaFormat, Rgba, lay_out};

use crate::{baselines, block4, cf, close, em_db, lay_text, left, pt, run};

fn one_line(block: &BlockFormat, para: &ParaFormat, text: &str) -> diacraft_text::Laid {
    lay_text(&em_db(), text, &cf("Em", 12.0), para, block, Some(2.0))
}

#[test]
fn layout_the_line_box_comes_from_spline_and_the_size_never_the_font() {
    // A face whose ascent and descent sum to 1.37 em must not change a thing.
    let tall = diacraft_fonts::synth::SynthFont::new("Tall").ascii(1000);
    let mut tall = tall;
    tall.ascent = 900;
    tall.descent = -470;
    let db = crate::db(&[tall]);
    let mut c = cf("Tall", 10.0);
    c.size = pt(10.0);
    for (spline, want) in [(-1.5, pt(15.0)), (0.25, 0.25), (0.0, pt(10.0)), (-1.2, pt(12.0))] {
        let p = ParaFormat { sp_line: spline, ..ParaFormat::default() };
        let laid = lay_text(&db, "a", &c, &p, &block4(1.0), Some(2.0));
        close(laid.lines[0].height, want);
        close(laid.size.height, want);
    }
}

#[test]
fn layout_one_middle_aligned_line_has_its_baseline_at_045() {
    let laid = one_line(&block4(1.0), &ParaFormat::default(), "ab");
    close(laid.lines[0].baseline, 0.45);
    // Not the middle of the box (0.5), and not an ascent of 0.9 of the size.
    close(laid.lines[0].top, 0.45 + 0.15);
    close(laid.lines[0].bottom, 0.45 - 0.05);
}

#[test]
fn layout_the_baseline_is_three_quarters_down_whatever_the_pitch() {
    let p = ParaFormat { sp_line: -1.0, ..ParaFormat::default() };
    close(one_line(&block4(1.0), &p, "ab").lines[0].baseline, 0.458333333);
}

#[test]
fn layout_top_and_bottom_alignment_keep_to_the_margins() {
    for (valign, want) in [(0, 0.794444444), (2, 0.105555556), (3, 0.794444444), (255, 0.794444444), (1, 0.45)] {
        let b = BlockFormat { vertical_align: valign, ..block4(1.0) };
        close(one_line(&b, &ParaFormat::default(), "ab").lines[0].baseline, want);
    }
}

#[test]
fn layout_three_lines_are_centred_as_a_stack() {
    let laid = one_line(&block4(1.0), &ParaFormat::default(), "a\nb\nc\n");
    let b = baselines(&laid);
    assert_eq!(b.len(), 3);
    for (got, want) in b.iter().zip([0.65, 0.45, 0.25]) {
        close(*got, want);
    }
    close(laid.size.height, 0.6);
}

#[test]
fn layout_asymmetric_margins_centre_on_the_area_not_the_block() {
    let b = BlockFormat { top_margin: 0.2, bottom_margin: 0.0, ..block4(1.0) };
    close(one_line(&b, &ParaFormat::default(), "ab").lines[0].baseline, 0.35);
}

#[test]
fn layout_spacing_goes_between_paragraphs_only() {
    let db = em_db();
    let c = cf("Em", 12.0);
    let first = ParaFormat { sp_after: pt(3.0), ..ParaFormat::default() };
    let second = ParaFormat { sp_before: pt(3.0), sp_after: pt(6.0), ..ParaFormat::default() };
    let runs = [run("a\n", &c, &first), run("b\n", &c, &second)];
    let top = lay_out(&runs, &BlockFormat { vertical_align: 0, ..block4(1.0) }, Some(2.0), &db);
    let b = baselines(&top);
    close(b[0], 0.794444444);
    close(b[1], 0.511111111);
    // The second paragraph's SpAfter is below the last line, so it is in neither T nor the placement: bottom-aligned, the last
    // baseline stays at 0.105556, and lifting it by the 6 pt gives 0.188889.
    let bottom = lay_out(&runs, &BlockFormat { vertical_align: 2, ..block4(1.0) }, Some(2.0), &db);
    close(bottom.lines[1].baseline, 0.105555556);
    close(bottom.size.height, 0.4 + pt(6.0));
}

#[test]
fn layout_space_before_the_first_paragraph_is_left_out() {
    let p = ParaFormat { sp_before: pt(6.0), ..ParaFormat::default() };
    let b = BlockFormat { vertical_align: 0, ..block4(1.0) };
    close(one_line(&b, &p, "ab").lines[0].baseline, 0.794444444);
}

#[test]
fn layout_a_line_takes_its_height_from_its_largest_size() {
    let db = em_db();
    let p = ParaFormat::default();
    let runs = [run("a\n", &cf("Em", 12.0), &p), run("a", &cf("Em", 12.0), &p), run("b\n", &cf("Em", 24.0), &p)];
    let laid = lay_out(&runs, &BlockFormat { vertical_align: 0, ..block4(1.0) }, Some(2.0), &db);
    let b = baselines(&laid);
    // 0.25 of 14.4 pt plus 0.75 of 28.8 pt; the first run's size would give 0.2.
    close(b[0] - b[1], 0.35);
    close(laid.lines[1].height, pt(28.8));
}

#[test]
fn layout_text_taller_than_the_block_spills_and_is_not_clamped() {
    let b = BlockFormat { height: Some(0.1), ..block4(0.1) };
    let laid = one_line(&b, &ParaFormat::default(), "a\nb\nc\nd\n");
    let bl = baselines(&laid);
    close(bl[0], 0.3);
    close(bl[3], -0.3);
    assert!(laid.overflow);
    assert!(!one_line(&block4(1.0), &ParaFormat::default(), "ab").overflow);
}

#[test]
fn layout_a_line_centres_and_right_aligns_by_its_trimmed_width() {
    // A line 1 inch wide without a trailing space that is 0.05 wide: Em at 0.05 inch, "aaaaaaaaaaaaaaaaaaaa" is 20 letters.
    let db = em_db();
    let c = crate::cf("Em", 0.0);
    let c = diacraft_text::CharFormat { size: 0.05, ..c };
    let b = BlockFormat { vertical_align: 0, ..BlockFormat::default() };
    for (halign, want) in [(1u8, 0.5), (2, 1.0), (0, 0.0)] {
        let p = ParaFormat { halign, ..ParaFormat::default() };
        let laid = lay_text(&db, "aaaaaaaaaaaaaaaaaaaa ", &c, &p, &b, Some(2.0));
        close(laid.lines[0].x, want);
        close(laid.lines[0].width, 1.0);
    }
}

#[test]
fn layout_indents_and_margins_set_where_lines_start() {
    let db = em_db();
    let c = diacraft_text::CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let p = ParaFormat { halign: 0, ind_left: 0.25, ind_first: -0.25, ..ParaFormat::default() };
    let b = BlockFormat { left_margin: 0.1, vertical_align: 0, ..BlockFormat::default() };
    // A wrapped paragraph: twelve letters do not fit the 1.1 the first line has, so six and six.
    let laid = lay_text(&db, "aaaaaa bbbbbb", &c, &p, &b, Some(1.2));
    assert!(laid.lines.len() >= 2, "{} lines", laid.lines.len());
    close(laid.lines[0].x, 0.1);
    close(laid.lines[1].x, 0.35);
}

#[test]
fn layout_a_last_line_feed_adds_no_line_and_an_extra_one_adds_one() {
    let b = block4(1.0);
    let one = one_line(&b, &ParaFormat::default(), "ab\n");
    assert_eq!(one.lines.len(), 1);
    close(one.lines[0].baseline, 0.45);
    let two = one_line(&b, &ParaFormat::default(), "ab\n\n");
    assert_eq!(two.lines.len(), 2);
    close(two.lines[0].baseline, 0.55);
    close(two.lines[1].baseline, 0.35);
    assert!(two.lines[1].chars.is_empty() || two.lines[1].width == 0.0, "the second line is empty");
    assert_eq!(one_line(&b, &ParaFormat::default(), "ab").lines.len(), 1, "no line feed at all is one line too");
    assert_eq!(one_line(&b, &ParaFormat::default(), "a\nb\nc").lines.len(), 3);
}

#[test]
fn margin_vectors_bottom_top_left_right() {
    let p = ParaFormat::default();
    let b = BlockFormat { vertical_align: 2, bottom_margin: 0.25, ..block4(1.0) };
    close(one_line(&b, &p, "ab").lines[0].baseline, 0.3);
    // Top margin 0.25, top aligned; bottom aligned with BottomMargin 0.05, the top margin not counting.
    let b = BlockFormat { vertical_align: 0, top_margin: 0.25, ..block4(1.0) };
    close(one_line(&b, &p, "ab").lines[0].baseline, 0.6);
    let b = BlockFormat { vertical_align: 2, top_margin: 0.25, bottom_margin: 0.05, ..block4(1.0) };
    close(one_line(&b, &p, "ab").lines[0].baseline, 0.1);
    // Left aligned, LeftMargin 0.25.
    let b = BlockFormat { left_margin: 0.25, ..block4(1.0) };
    close(one_line(&b, &left(), "ab").lines[0].x, 0.25);
    // A block 2 wide, a right-aligned line 1 wide, LeftMargin 0.1 and RightMargin 0.3: it starts at 0.7.
    let right = ParaFormat { halign: 2, ..ParaFormat::default() };
    let c = diacraft_text::CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let b = BlockFormat { left_margin: 0.1, right_margin: 0.3, ..block4(1.0) };
    let b = BlockFormat { left_margin: 0.1, right_margin: 0.3, top_margin: b.top_margin, bottom_margin: b.bottom_margin, ..b };
    let laid = lay_text(&em_db(), "aaaaaaaaaa", &c, &right, &b, Some(2.0));
    close(laid.lines[0].x, 0.7);
}

#[test]
fn verticalalign_the_default_block_format_is_the_middle() {
    // A shape with no cell of its own takes the root's 1: the middle, not the 0 a missing cell would be.
    assert_eq!(BlockFormat::default().vertical_align, 1);
    let laid = one_line(&block4(1.0), &ParaFormat::default(), "ab");
    close(laid.lines[0].baseline, 0.45);
}

#[test]
fn bkgnd_the_background_hugs_the_text_and_takes_no_margin() {
    // One centred line of width 1 at 12 pt in a 2 by 1 block with margins 4 pt: x 0.5 to 1.5, y 0.4 to 0.6. In the `Em` face six
    // letters at 12 pt are exactly one inch.
    let b = BlockFormat { background: Some((Rgba { r: 255, g: 255, b: 255, a: 255 }, 0.0)), ..block4(1.0) };
    let laid = lay_text(&em_db(), "aaaaaa", &cf("Em", 12.0), &ParaFormat::default(), &b, Some(2.0));
    let bg = laid.background.expect("a background");
    close(bg.rect.x0, 0.5);
    close(bg.rect.x1, 1.5);
    // The line box: three quarters of L above the baseline and a quarter below.
    close(bg.rect.y0, 0.4);
    close(bg.rect.y1, 0.6);
    assert_eq!(bg.opacity, 1.0);
}

#[test]
fn bkgnd_two_lines_get_one_rectangle_as_wide_as_the_wider() {
    let db = em_db();
    let c = diacraft_text::CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let b = BlockFormat { background: Some((Rgba { r: 1, g: 2, b: 3, a: 255 }, 0.25)), ..block4(1.0) };
    let laid = lay_text(&db, "aaaaaaaaaa\naaaaaa", &c, &ParaFormat::default(), &b, Some(2.0));
    let bg = laid.background.expect("one rectangle");
    close(bg.rect.width(), 1.0);
    // Tall as both line boxes: L is 0.12 here, two of them.
    close(bg.rect.height(), 0.24);
    close(bg.opacity, 0.75);
    assert_eq!((bg.color.r, bg.color.g, bg.color.b), (1, 2, 3));
}

#[test]
fn bkgnd_no_background_and_no_text_give_no_rectangle() {
    let b = block4(1.0);
    assert!(one_line(&b, &ParaFormat::default(), "ab").background.is_none());
    let with = BlockFormat { background: Some((Rgba::BLACK, 0.0)), ..block4(1.0) };
    // With no text there is nothing to hug.
    let laid = lay_out(&[], &with, Some(2.0), &em_db());
    assert!(laid.background.is_none());
}

#[test]
fn empty_text_takes_one_line_and_no_runs_take_none() {
    // TEXTHEIGHT of empty text is margins plus one line, not zero lines.
    let db = em_db();
    let laid = lay_text(&db, "", &cf("Em", 12.0), &ParaFormat::default(), &block4(1.0), Some(2.0));
    assert_eq!(laid.lines.len(), 1);
    close(laid.size.height, 0.2);
    close(laid.size.width, 0.0);
    let none = lay_out(&[], &block4(1.0), Some(2.0), &db);
    assert!(none.lines.is_empty());
    assert_eq!((none.size.width, none.size.height), (0.0, 0.0));
}

#[test]
fn a_block_with_no_height_is_as_tall_as_its_text_and_margins() {
    let block = BlockFormat { height: None, ..block4(0.0) };
    let laid = lay_text(&em_db(), "a\nb", &cf("Em", 12.0), &ParaFormat::default(), &block, Some(2.0));
    assert!(!laid.overflow);
    // The block is T plus both margins tall, so the stack fills the area: the last baseline is a quarter of the last box above the
    // area's bottom, which is the bottom margin.
    close(laid.lines[1].baseline, pt(4.0) + 0.05);
    close(laid.lines[0].baseline, pt(4.0) + 0.05 + 0.2);
}
