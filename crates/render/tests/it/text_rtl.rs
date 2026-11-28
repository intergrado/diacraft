//! Right-to-left paragraphs mirror, and Japanese vertical text is underlined on the right (design calls
//!). Every assertion is about where ink is, in paper inches, with the bundled faces and one face built in code: the places are the pixels the
//! renderer paints, and not the numbers the layout gave it.

use diacraft_doc::RowKey;
use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_render::Renderer;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A 2 by 2 inch box at (1, 1) with no fill and no line, showing `text` at `size` inches in black, in a paragraph with these `Paragraph` cells.
fn label(text: &str, size: f64, paragraph: &[(&str, &str)]) -> ShapeBuilder {
    let mut sb = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", &size.to_string())
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000");
    for (name, value) in paragraph {
        sb = sb.row_cell("Paragraph", None, RowKey::Ix(0), name, value);
    }
    sb
}

/// The ink box of a drawn label: `(x0, x1)` in paper inches.
fn across(text: &str, paragraph: &[(&str, &str)]) -> (f64, f64) {
    let b = draw(&one_page(vec![label(text, 0.4, paragraph)])).ink_box().expect("the text is drawn");
    (b.0, b.2)
}

#[test]
fn an_alignment_of_0_puts_a_right_to_left_paragraph_against_the_right_edge() {
    // The block is x 1 to 3. Left to right, alignment 0 is the left edge; right to left it is the right one, and 2 is the left.
    let (l0, r0) = across("WWW", &[("HorzAlign", "0")]);
    assert!((1.0..1.1).contains(&l0) && r0 < 2.5, "left to right, alignment 0: {l0} to {r0}");
    let (l1, r1) = across("WWW", &[("HorzAlign", "0"), ("Flags", "1")]);
    assert!(r1 > 2.9 && r1 <= 3.0 && l1 > 1.5, "right to left, alignment 0: {l1} to {r1}");
    let (l2, r2) = across("WWW", &[("HorzAlign", "2"), ("Flags", "1")]);
    assert!((1.0..1.1).contains(&l2) && r2 < 2.5, "right to left, alignment 2: {l2} to {r2}");
    // The same width in each: only the place changes.
    assert!(((r1 - l1) - (r0 - l0)).abs() < 0.03 && ((r2 - l2) - (r0 - l0)).abs() < 0.03);
}

#[test]
fn a_centred_right_to_left_paragraph_stays_centred() {
    let (l, r) = across("WWW", &[("HorzAlign", "1"), ("Flags", "1")]);
    assert!(((l + r) / 2.0 - 2.0).abs() < 0.1, "{l} to {r}");
}

#[test]
fn the_start_indent_is_in_from_the_right_edge_and_the_stop_indent_in_from_the_left() {
    // V22 on paper: `IndLeft` 0.5 keeps a line aligned 0 half an inch in from the right edge (x 3 - 0.5), and `IndRight` 0.5 keeps a line aligned 2 half an inch in
    // from the left (x 1 + 0.5). Naive (the old rule): the start indent on the left.
    let (_, r) = across("WWW", &[("HorzAlign", "0"), ("Flags", "1"), ("IndLeft", "0.5")]);
    assert!(r > 2.4 && r < 2.53, "right edge {r}");
    let (l, _) = across("WWW", &[("HorzAlign", "2"), ("Flags", "1"), ("IndRight", "0.5")]);
    assert!((1.49..1.6).contains(&l), "left edge {l}");
    // Left to right, the same cells go the other way.
    let (l, _) = across("WWW", &[("HorzAlign", "0"), ("IndLeft", "0.5")]);
    assert!((1.49..1.6).contains(&l), "left to right: left edge {l}");
}

#[test]
fn the_blocks_margins_stay_on_their_own_sides() {
    let doc = |flags: &str, align: &str| {
        let sb = label("WWW", 0.4, &[("HorzAlign", align), ("Flags", flags)]).cell("LeftMargin", "0.2").cell("RightMargin", "0.4");
        let b = draw(&one_page(vec![sb])).ink_box().expect("drawn");
        (b.0, b.2)
    };
    // The margins stay at their own edges. Right to left, aligned 0: the start side is the right one, kept in by the right margin of 0.4, so the text ends at x 3 - 0.4.
    let (_, r) = doc("1", "0");
    assert!(r > 2.5 && r < 2.63, "{r}");
    // Aligned 2: it starts at the left margin, x 1 + 0.2.
    let (l, _) = doc("1", "2");
    assert!((1.19..1.3).contains(&l), "{l}");
}

#[test]
fn a_bullet_stands_on_the_right_of_a_right_to_left_paragraph_with_the_text_after_it() {
    // A bullet of the letter W, at 0.3 inch, with the text 0.5 inch from the bullet's start: right to left the bullet is at the right edge (x 2.7 to 3) and
    // the text ends at x 2.5, leaving the gap between clear; left to right the bullet is at the left edge and the text starts at x 1.5.
    let cells = [("HorzAlign", "0"), ("Bullet", "1"), ("BulletStr", "W"), ("TextPosAfterBullet", "0.5")];
    let ink_between = |pic: &Pic, x0: f64, x1: f64| -> usize {
        let mut n = 0;
        let (px0, px1) = ((x0 * pic.ppi) as i64, (x1 * pic.ppi) as i64);
        for px in px0..px1 {
            for py in 0..(pic.page_h * pic.ppi) as i64 {
                n += usize::from(pic.is_dark((px as f64 + 0.5) / pic.ppi, (py as f64 + 0.5) / pic.ppi));
            }
        }
        n
    };
    let mut rtl = cells.to_vec();
    rtl.push(("Flags", "1"));
    let pic = draw(&one_page(vec![label("WW", 0.3, &rtl)]));
    assert!(ink_between(&pic, 2.75, 2.97) > 20, "the bullet is at the right edge");
    assert_eq!(ink_between(&pic, 2.52, 2.67), 0, "the gap between the text and the bullet is clear");
    assert!(ink_between(&pic, 1.9, 2.5) > 50, "the text is left of the gap");
    let pic = draw(&one_page(vec![label("WW", 0.3, &cells)]));
    assert!(ink_between(&pic, 1.03, 1.25) > 20, "left to right, the bullet is at the left edge");
    assert_eq!(ink_between(&pic, 1.32, 1.48), 0, "and its gap is clear");
    assert_eq!(ink_between(&pic, 2.75, 2.97), 0, "nothing is on the right");
}

/// The bundled faces and one built in code, `Em`, whose Latin letters and two Han characters are solid boxes of three quarters of the em wide.
fn renderer() -> Renderer {
    let mut db = FontDb::bundled();
    db.add_font_data(SynthFont::new("Em").ascii(1000).chars("\u{65E5}\u{672C}", 1000).build());
    Renderer::with_fonts(db)
}

/// Two underlined Han characters at 0.5 inch down a vertical block 2 inches square, in the language `lang`; the first column's ink box `(x0, x1)`.
fn vertical(lang: &str) -> (Pic, f64, f64) {
    let sb = label("\u{65E5}\u{672C}", 0.5, &[("HorzAlign", "0")])
        .cell("TextDirection", "1")
        .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
        .row_cell("Character", None, RowKey::Ix(0), "Style", "4")
        .row_cell("Character", None, RowKey::Ix(0), "LangID", lang);
    let pic = draw_with(&mut renderer(), &one_page(vec![sb]));
    let b = pic.ink_box().expect("the characters are drawn");
    (pic, b.0, b.2)
}

#[test]
fn a_japanese_underline_is_drawn_on_the_right_of_vertical_characters() {
    // The column's centre line is x 2. A character is a box of three quarters of its 0.5 em wide, standing from the centre line less half the em: x 1.75 to
    // 2.125. The underline, 0.025 thick, has its inner edge at the cells' right edge, x 2.25, and runs the length of the two characters, y 3 down to 2.
    let (pic, x0, x1) = vertical("ja-JP");
    assert!((x0 - 1.75).abs() < 0.03, "the characters start at {x0}");
    assert!(x1 > 2.26 && x1 < 2.31, "the ink reaches {x1}, past the characters' own right edge, 2.125");
    for y in [2.1, 2.5, 2.9] {
        assert!(pic.is_dark(2.262, y), "underline at y {y}");
    }
    // Between the characters' ink and the underline the paper is clear.
    assert!(pic.is_white(2.2, 2.5));
}

#[test]
fn a_korean_underline_too_and_every_other_language_keeps_the_baseline_side() {
    let (_, _, ko) = vertical("ko-KR");
    assert!(ko > 2.26, "{ko}");
    // Another language: the underline lies down the baseline side, which is inside the characters' boxes here, so the ink reaches no further right than the boxes do.
    for lang in ["en-US", "zh-CN", "x-none", "de-DE"] {
        let (pic, x0, x1) = vertical(lang);
        assert!(x1 < 2.14, "{lang}: the ink reaches {x1}");
        assert!(x0 > 1.74, "{lang}: and starts at {x0}");
        assert!(!pic.is_dark(2.262, 2.5), "{lang}");
    }
}
