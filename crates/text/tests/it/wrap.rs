//! Wrapping. Every character advances 0.1 inch, spaces included, blocks
//! are 1.2 inches wide with margins 0.1, so the usable width `A` is 1.0.

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, ParaFormat, lay_out};

use crate::{cf, close, db, em_db, glyph_count, lay_text, left, line_text, pt, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn block() -> BlockFormat {
    BlockFormat { left_margin: 0.1, right_margin: 0.1, vertical_align: 0, height: Some(1.0), ..BlockFormat::default() }
}

fn wrap(text: &str) -> Laid {
    lay_text(&em_db(), text, &tenth(), &left(), &block(), Some(1.2))
}

fn texts(text: &str, laid: &Laid) -> Vec<String> {
    laid.lines.iter().map(|l| line_text(text, l)).collect()
}

#[test]
fn wrap_text_wraps_to_the_usable_width_not_the_blocks() {
    let t = "aaaa bbbb cc";
    let laid = wrap(t);
    assert_eq!(laid.lines.len(), 2);
    close(laid.lines[0].width, 0.9);
    assert_eq!(line_text(t, &laid.lines[0]).trim_end(), "aaaa bbbb");
    assert_eq!(line_text(t, &laid.lines[1]), "cc");
    // The same text in the whole 1.2 would fit on one line.
    assert_eq!(lay_text(&em_db(), t, &tenth(), &left(), &BlockFormat::default(), Some(1.2)).lines.len(), 1);
}

#[test]
fn wrap_trailing_spaces_stay_on_the_line_and_do_not_count() {
    let t = "aaaaaaaaaa bbb";
    let laid = wrap(t);
    assert_eq!(laid.lines.len(), 2, "{:?}", texts(t, &laid));
    close(laid.lines[0].width, 1.0);
    assert_eq!(line_text(t, &laid.lines[0]), "aaaaaaaaaa ", "the space stays at the line's end");
    assert_eq!(line_text(t, &laid.lines[1]), "bbb");
    // Counting the space would overflow at 1.1 and break inside the word: no glyph of the space, nor a split word.
    assert_eq!(glyph_count(&laid.lines[0]), 10, "the trailing space is not laid out");
}

#[test]
fn wrap_a_word_wider_than_the_line_breaks_inside_itself() {
    let t = "a".repeat(15);
    let laid = wrap(&t);
    assert_eq!(texts(&t, &laid).iter().map(String::len).collect::<Vec<_>>(), vec![10, 5]);
    assert!(laid.lines.iter().all(|l| l.width <= 1.0 + 1e-9), "no line is allowed to overflow to 1.5");
}

#[test]
fn wrap_a_forced_break_is_not_a_paragraph_end() {
    let db = em_db();
    let c = tenth();
    let p = ParaFormat { halign: 0, sp_before: pt(6.0), ..ParaFormat::default() };
    let t = "aa\u{2028}bb\ncc";
    let laid = lay_text(&db, t, &c, &p, &block(), Some(1.2));
    assert_eq!(laid.lines.len(), 3, "{:?}", texts(t, &laid));
    let l = 0.12;
    // 6 pt only above `cc`, and not above the first paragraph's first line nor above `bb` (the same paragraph).
    close(laid.lines[0].baseline - laid.lines[1].baseline, l);
    close(laid.lines[1].baseline - laid.lines[2].baseline, l + pt(6.0));
    assert_eq!(laid.lines[0].paragraph, laid.lines[1].paragraph);
    assert_ne!(laid.lines[1].paragraph, laid.lines[2].paragraph);
    // The mark is never drawn: two letters on each of the first two lines.
    assert_eq!(glyph_count(&laid.lines[0]), 2);
}

#[test]
fn a_forced_break_takes_no_first_line_indent() {
    // Only the first line of the paragraph is indented.
    let db = em_db();
    let p = ParaFormat { halign: 0, ind_first: 0.25, ..ParaFormat::default() };
    let laid = lay_text(&db, "one\u{2028}two\n", &tenth(), &p, &block(), Some(2.0));
    assert_eq!(laid.lines.len(), 2);
    close(laid.lines[0].x - laid.lines[1].x, 0.25);
}

#[test]
fn a_width_that_is_zero_or_negative_puts_one_cluster_on_each_line_and_draws_all_of_it() {
    // Never fail, and never loop for ever.
    for width in [0.0, -1.0, 0.05, -1e9] {
        let t = "abcde";
        let laid = lay_text(&em_db(), t, &tenth(), &left(), &BlockFormat::default(), Some(width));
        assert_eq!(laid.lines.len(), 5, "width {width}");
        assert_eq!(laid.lines.iter().map(glyph_count).sum::<usize>(), 5);
    }
}

#[test]
fn a_width_that_is_not_a_number_is_no_wrapping() {
    for width in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let laid = lay_text(&em_db(), "aaaa bbbb cccc", &tenth(), &left(), &BlockFormat::default(), Some(width));
        assert_eq!(laid.lines.len(), 1, "width {width}");
    }
    assert_eq!(lay_text(&em_db(), "aaaa bbbb cccc", &tenth(), &left(), &BlockFormat::default(), None).lines.len(), 1);
}

#[test]
fn measure_a_width_is_the_sum_of_nominal_advances_and_never_kerned() {
    // A face whose A advances 0.6 em and whose pair AV kerns -0.1 em, V 0.6: at 10 pt `AV` is 12 pt, not 11.
    let mut font = SynthFont::new("Kerning").ascii(500).advance('A', 600).advance('V', 600);
    font.kern = vec![('A', 'V', -100)];
    let laid = lay_text(&db(&[font]), "AV", &cf("Kerning", 10.0), &left(), &BlockFormat::default(), None);
    close(laid.size.width, pt(12.0));
    close(laid.lines[0].width, pt(12.0));
}

#[test]
fn two_runs_in_one_word_do_not_break_between_them() {
    // A format change inside a word is not a break opportunity: Arabic joining and every word depend on it.
    let db = em_db();
    let c = tenth();
    let bold = CharFormat { style: 1, ..c.clone() };
    let runs = [run("aaaa", &c, &left()), run("bbbbbb", &bold, &left()), run(" cc", &c, &left())];
    let laid = lay_out(&runs, &block(), Some(1.2), &db);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[1].chars, 11..13, "only cc moved down; the word aaaabbbbbb stayed whole");
}

#[test]
fn a_no_break_space_does_not_break_and_a_zero_width_space_may() {
    // Eleven characters, 1.1 inches, with a no-break space in the middle: the space is no place to break, so the line is cut inside a
    // word at the tenth character, and not after the sixth.
    let t = "aaaaaa\u{00A0}bbbb";
    let laid = wrap(t);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].chars, 0..10);
    let t = "aaaaaa\u{200B}bbbbbb";
    let laid = wrap(t);
    assert_eq!(texts(t, &laid), vec!["aaaaaa\u{200B}".to_owned(), "bbbbbb".to_owned()]);
}

#[test]
fn a_soft_hyphen_shows_a_hyphen_only_when_the_line_breaks_there() {
    let t = "aaaaaa\u{00AD}bbbbbb";
    let laid = lay_text(&em_db(), t, &tenth(), &left(), &block(), Some(1.2));
    assert_eq!(laid.lines.len(), 2);
    // Six letters and the hyphen drawn after them.
    assert_eq!(glyph_count(&laid.lines[0]), 7);
    close(laid.lines[0].width, 0.7);
    // No break needed: nothing is drawn for it.
    let short = lay_text(&em_db(), "aa\u{00AD}bb", &tenth(), &left(), &block(), Some(1.2));
    assert_eq!(short.lines.len(), 1);
    assert_eq!(glyph_count(&short.lines[0]), 4);
    close(short.lines[0].width, 0.4);
}
