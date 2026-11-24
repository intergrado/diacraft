//! Right-to-left text and the Unicode bidirectional algorithm.

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, ParaFormat, lay_out};

use crate::{close, db, left, run};

fn font() -> SynthFont {
    SynthFont::new("Mixed").ascii(1000).chars("אבגדה", 1000)
}

fn lay(text: &str, p: &ParaFormat) -> (Laid, SynthFont) {
    let f = font();
    let db = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    (lay_out(&[run(text, &c, p)], &block, Some(10.0), &db), f)
}

fn visual(laid: &Laid, f: &SynthFont) -> String {
    let all: Vec<char> = "abcdefghijklmnopqrstuvwxyz 0123456789אבגדה".chars().collect();
    crate::gids(&laid.lines[0]).iter().filter_map(|g| all.iter().copied().find(|c| u32::from(f.gid(*c)) == *g)).collect()
}

#[test]
fn a_hebrew_word_inside_latin_text_reads_right_to_left_on_its_own() {
    let (laid, f) = lay("abc אבג def", &left());
    assert_eq!(visual(&laid, &f), "abc גבא def");
    assert!(!laid.lines[0].rtl);
}

#[test]
fn a_right_to_left_paragraph_puts_its_first_typed_text_on_the_right() {
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let (laid, f) = lay("אבג abc", &p);
    // Logical: Hebrew, space, Latin. Visual from the left: the Latin run, the space, then the Hebrew reversed.
    assert_eq!(visual(&laid, &f), "abc גבא");
    assert!(laid.lines[0].rtl);
}

#[test]
fn digits_in_right_to_left_text_keep_their_order() {
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let (laid, f) = lay("אב 123", &p);
    assert_eq!(visual(&laid, &f), "123 בא");
}

#[test]
fn a_wrapped_right_to_left_paragraph_is_reordered_a_line_at_a_time() {
    let f = font();
    let db = db(std::slice::from_ref(&f));
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    // Three five-letter words in a measure of 1.2: two to a line.
    let laid = lay_out(&[run("אבגדה אבגדה אבגדה", &c, &p)], &block, Some(1.2), &db);
    assert_eq!(laid.lines.len(), 2);
    for line in &laid.lines {
        let gids = crate::gids(line);
        // Within every word the letters come out reversed.
        assert_eq!(gids.first().copied(), Some(u32::from(f.gid('ה'))), "{gids:?}");
    }
}

#[test]
fn the_trailing_spaces_of_a_right_to_left_line_do_not_move_its_text() {
    let p = ParaFormat { flags: 1, halign: 2, ..ParaFormat::default() };
    let (laid, _) = lay("אבג   ", &p);
    close(laid.lines[0].x + laid.lines[0].width, 10.0);
}

#[test]
fn explicit_embeddings_are_honoured_and_never_unbalanced_into_a_crash() {
    // A right-to-left override, an unclosed embedding and stray closers.
    for text in ["a\u{202E}bc\u{202C}d", "a\u{202B}bc", "\u{202C}\u{202C}abc", "\u{2067}אב\u{2069}c", "\u{2068}a\u{2069}", "\u{2069}\u{2069}"] {
        let (laid, _) = lay(text, &left());
        assert!(!laid.lines.is_empty(), "{text:?}");
    }
}

// ---- rule L1: a tab, and the white space before it, belong to the paragraph and not to the text beside them

#[test]
fn a_tab_between_hebrew_words_in_a_left_to_right_paragraph_keeps_the_words_in_their_order() {
    // Logical: alef bet, tab, gimel dalet. Each word reads right to left on its own; the first stays on the left of the tab.
    let (laid, f) = lay("אב\tגד", &left());
    assert_eq!(visual(&laid, &f), "באדג");
    // The tab is between them: the second word starts after the tab's width, not right after the first.
    let xs: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    assert!(xs[2] > xs[1] + 0.1 + 1e-9, "a tab's gap lies between the words: {xs:?}");
}

#[test]
fn white_space_before_a_tab_takes_the_paragraph_direction_too() {
    let (laid, f) = lay("אב \tגד", &left());
    assert_eq!(visual(&laid, &f), "בא דג");
    // A letter before the tab keeps its own direction, and the space inside the Hebrew run stays in it: the first run is reversed whole.
    let (laid, f) = lay("אב ג\tד", &left());
    assert_eq!(visual(&laid, &f), "ג באד");
}

#[test]
fn a_tab_in_a_right_to_left_paragraph_has_the_text_before_it_on_its_right() {
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let (laid, f) = lay("abc\tdef", &p);
    // `abc` came first, so it is on the right of the tab and `def` on the left.
    assert_eq!(visual(&laid, &f), "defabc");
}

#[test]
fn a_tab_between_numbers_in_a_right_to_left_paragraph_keeps_them_apart() {
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let (laid, f) = lay("12\t34", &p);
    assert_eq!(visual(&laid, &f), "3412");
}

#[test]
fn white_space_that_ends_a_line_takes_the_paragraph_direction_too() {
    // A figure space (U+2007) is white space that is not a breaking space, so it stays at the end of the line: rule L1 puts it at the
    // paragraph's level, after the Hebrew word and not before it.
    let f = font().chars("\u{2007}", 1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() };
    let laid = lay_out(&[run("אב\u{2007}", &c, &left())], &block, Some(10.0), &d);
    let g = |ch: char| u32::from(f.gid(ch));
    assert_eq!(crate::gids(&laid.lines[0]), vec![g('ב'), g('א'), g('\u{2007}')]);
}

#[test]
fn an_isolate_control_before_a_tab_takes_the_paragraph_direction_like_the_white_space_it_is_counted_with() {
    // An unmatched PDI (U+2069, zero width here) at the end of a Hebrew word and before a tab: rule L1 puts it at the paragraph's level,
    // so it is drawn after the word and not before it. Left in the word's level it would come out first.
    let f = font().chars("\u{2069}", 0);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let laid = lay_out(
        &[run("אב\u{2069}\tגד", &c, &left())],
        &BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() },
        Some(10.0),
        &d,
    );
    let g = |ch: char| u32::from(f.gid(ch));
    // The shaper draws an invisible control as the face's space.
    assert_eq!(crate::gids(&laid.lines[0]), vec![g('ב'), g('א'), g(' '), g('ד'), g('ג')]);
}
