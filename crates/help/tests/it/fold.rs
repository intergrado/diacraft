//! Folding text for matching, and cutting a query (design section 5.2).

use diacraft_help::fold::{Casing, cut_query, fold, query_words};
use diacraft_help::{MAX_QUERY_CHARS, MAX_QUERY_WORDS};

#[test]
fn text_is_lowercased_decomposed_and_stripped_of_its_marks() {
    assert_eq!(fold("Ünï Çöde Écran", Casing::Standard), "uni code ecran");
    assert_eq!(fold("e\u{301}", Casing::Standard), "e", "a combining mark alone is dropped");
    assert_eq!(fold("Fett", Casing::Standard), "fett");
    assert_eq!(fold("ＡＢＣ １２", Casing::Standard), "abc 12", "full-width letters and digits fold to plain ones");
    assert_eq!(fold("ﬁle", Casing::Standard), "file");
}

#[test]
fn white_space_is_collapsed_and_invisible_marks_are_dropped() {
    assert_eq!(fold("  a \t b\n\nc  ", Casing::Standard), "a b c");
    assert_eq!(fold("a\u{200B}b", Casing::Standard), "ab");
    assert_eq!(fold("a\u{00AD}b\u{FEFF}c", Casing::Standard), "abc");
    assert_eq!(fold("   ", Casing::Standard), "");
    assert_eq!(fold("", Casing::Standard), "");
}

#[test]
fn turkish_has_two_pairs_of_i() {
    assert_eq!(fold("İtalik", Casing::Turkic), "italik");
    assert_eq!(fold("ISI", Casing::Turkic), "ısı");
    assert_eq!(fold("İtalik", Casing::Standard), "italik", "the standard rule gives i and a dot, and the dot is a mark");
    assert_eq!(fold("ISI", Casing::Standard), "isi");
}

#[test]
fn the_casing_follows_the_language_tag() {
    for tr in ["tr", "tr-TR", "TR", "tr_TR", "az", "az-Latn", "az-Latn-AZ"] {
        assert_eq!(Casing::for_language(tr), Casing::Turkic, "{tr}");
    }
    for other in ["en", "en-US", "de", "ja", "pt-BR", "", "tre", "trx-TR", "a"] {
        assert_eq!(Casing::for_language(other), Casing::Standard, "{other}");
    }
}

#[test]
fn a_query_is_cut_at_a_character() {
    assert_eq!(cut_query("héllo", 3), "hél");
    assert_eq!(cut_query("abc", 10), "abc");
    assert_eq!(cut_query("abc", 0), "");
    assert_eq!(cut_query("", 5), "");
    assert_eq!(cut_query("a🙂b", 2), "a🙂");
    let long = "é".repeat(300);
    assert_eq!(cut_query(&long, MAX_QUERY_CHARS).chars().count(), 255);
}

#[test]
fn a_query_is_words_of_letters_and_digits_cut_folded_and_capped() {
    assert_eq!(query_words("Add  Image", Casing::Standard), ["add", "image"]);
    assert_eq!(query_words("add-in", Casing::Standard), ["add", "in"]);
    assert_eq!(query_words("c++", Casing::Standard), ["c"]);
    assert_eq!(query_words("F5 Éa", Casing::Standard), ["f5", "ea"]);
    assert!(query_words("", Casing::Standard).is_empty());
    assert!(query_words("   \t ", Casing::Standard).is_empty(), "spaces only is empty");
    assert!(query_words("--- &&", Casing::Standard).is_empty(), "no letter or digit is empty");
    let many = (0..40).map(|n| format!("w{n}")).collect::<Vec<_>>().join(" ");
    let words = query_words(&many, Casing::Standard);
    assert_eq!(words.len(), MAX_QUERY_WORDS);
    assert_eq!(words[15], "w15");
}

#[test]
fn a_query_pasted_at_a_huge_length_keeps_255_characters() {
    let pasted = "a".repeat(100_000);
    let words = query_words(&pasted, Casing::Standard);
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].len(), MAX_QUERY_CHARS);
    // Words past the 255th character are not read at all.
    let late = format!("{} zzz", "a ".repeat(130));
    assert!(!query_words(&late, Casing::Standard).iter().any(|w| w == "zzz"));
}

#[test]
fn a_run_of_cjk_characters_is_one_query_word() {
    assert_eq!(query_words("結合 図形", Casing::Standard), ["結合", "図形"]);
}
