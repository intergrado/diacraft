//! `Case` and `LangID`.

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, lay_out};

use crate::{cf, db, left, run};

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn font() -> SynthFont {
    SynthFont::new("Cases").ascii(500).chars("İıßÀà", 500)
}

fn gid_text(laid: &Laid, f: &SynthFont) -> String {
    let all: Vec<char> = (0u32..0x250).filter_map(char::from_u32).filter(|c| f.gid(*c) != 0).collect();
    crate::gids(&laid.lines[0]).iter().filter_map(|g| all.iter().copied().find(|c| u32::from(f.gid(*c)) == *g)).collect()
}

fn lay(text: &str, case: u8, lang: &str) -> String {
    let f = font();
    let db = db(std::slice::from_ref(&f));
    let c = CharFormat { case, lang: lang.into(), ..cf("Cases", 12.0) };
    gid_text(&lay_out(&[run(text, &c, &left())], &block(), None, &db), &f)
}

#[test]
fn case_one_is_capitals_two_is_a_capital_for_each_word_and_zero_is_as_stored() {
    assert_eq!(lay("abc", 0, "en-US"), "abc");
    assert_eq!(lay("abc", 1, "en-US"), "ABC");
    assert_eq!(lay("ab cd", 2, "en-US"), "Ab Cd");
    // Text stored in capitals stays so under 2, and other letters are left as stored.
    assert_eq!(lay("AB cD", 2, "en-US"), "AB CD");
    assert_eq!(lay("ab-cd ef", 2, "en-US"), "Ab-Cd Ef");
}

#[test]
fn case_the_stored_text_is_not_changed_only_what_is_drawn() {
    // The line's character range still counts the stored characters.
    let f = font();
    let db = db(&[f]);
    let c = CharFormat { case: 1, ..cf("Cases", 12.0) };
    let laid = lay_out(&[run("abc", &c, &left())], &block(), None, &db);
    assert_eq!(laid.lines[0].chars, 0..3);
}

#[test]
fn langid_a_turkish_run_makes_a_dotted_capital_i() {
    assert_eq!(lay("i", 1, "tr-TR"), "İ");
    assert_eq!(lay("i", 1, "az-Latn-AZ"), "İ");
    assert_eq!(lay("ı", 1, "tr-TR"), "I");
    // A locale-free capital gives U+0049.
    assert_eq!(lay("i", 1, "en-US"), "I");
    assert_eq!(lay("i", 1, ""), "I");
}

#[test]
fn a_capital_that_is_longer_than_the_letter_draws_both_letters() {
    // The German sharp s capitalises to two letters.
    assert_eq!(lay("ß", 1, "de-DE"), "SS");
}

#[test]
fn case_the_roots_zero_draws_the_text_as_stored() {
    assert_eq!(lay("aBc", 0, "en-US"), "aBc");
}

#[test]
fn a_word_split_over_two_runs_is_one_word_and_gets_one_capital() {
    let f = font();
    let db = db(std::slice::from_ref(&f));
    let two = CharFormat { case: 2, ..cf("Cases", 12.0) };
    let none = CharFormat { case: 0, ..cf("Cases", 12.0) };
    let drawn = |runs: &[diacraft_text::Run]| gid_text(&lay_out(runs, &block(), None, &db), &f);
    // `he` and `llo` in two runs, both Case 2: the h is a capital and the l, which continues the word, is not.
    assert_eq!(drawn(&[run("he", &two, &left()), run("llo ab", &two, &left())]), "Hello Ab");
    // A word that starts in a run of Case 0 is not made a capital by the run after it.
    assert_eq!(drawn(&[run("he", &none, &left()), run("llo", &two, &left())]), "hello");
    // A new word in the second run is a capital.
    assert_eq!(drawn(&[run("he ", &two, &left()), run("llo", &two, &left())]), "He Llo");
}
