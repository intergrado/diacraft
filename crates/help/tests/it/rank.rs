//! Ranking commands against a query (design section 5.2).

use diacraft_help::MAX_SCORED;
use diacraft_help::fold::Casing;
use diacraft_help::rank::{Candidate, English, Hit, MatchKind, rank};

fn cand<'a>(label: &'a str, place: &'a [&'a str], keywords: &'a [&'a str], order: u32) -> Candidate<'a> {
    Candidate { label, place, keywords, english: None, order, recent: None }
}

fn ids(hits: &[Hit]) -> Vec<usize> {
    hits.iter().map(|h| h.index).collect()
}

#[test]
fn v5_add_image_finds_pictures_through_its_keywords_though_its_label_holds_neither_word() {
    let pictures = cand("Pictures", &["Insert", "Illustrations"], &["add", "image", "picture", "photo"], 10);
    let setup = cand("Page Setup", &["Design", "Page Setup"], &["paper", "margins"], 11);
    let hits = rank("add image", &[setup, pictures], Casing::Standard, 5);
    assert_eq!(hits, [Hit { index: 1, kind: MatchKind::Other }]);
}

#[test]
fn the_label_that_starts_with_the_query_comes_first_then_a_word_in_the_label_then_the_rest() {
    let quick = cand("Quick Print", &["File"], &[], 1);
    let preview = cand("Print Preview", &["File"], &[], 5);
    let print = cand("Print", &["File"], &[], 9);
    let by_keyword = cand("Output", &["File"], &["print"], 0);
    let by_place = cand("Settings", &["Print", "Setup"], &[], 0);
    let hits = rank("print", &[quick, preview, print, by_keyword, by_place], Casing::Standard, 10);
    assert_eq!(
        hits,
        [
            Hit { index: 1, kind: MatchKind::LabelStart },
            Hit { index: 2, kind: MatchKind::LabelStart },
            Hit { index: 0, kind: MatchKind::Label },
            Hit { index: 3, kind: MatchKind::Other },
            Hit { index: 4, kind: MatchKind::Other },
        ]
    );
}

#[test]
fn the_kind_is_the_weakest_the_words_needed() {
    let both = cand("Print Preview", &["File"], &["check"], 0);
    assert_eq!(rank("print", &[both], Casing::Standard, 5)[0].kind, MatchKind::LabelStart);
    assert_eq!(rank("print check", &[both], Casing::Standard, 5)[0].kind, MatchKind::Other);
    assert_eq!(rank("preview print", &[both], Casing::Standard, 5)[0].kind, MatchKind::Label);
    assert_eq!(rank("print pre", &[both], Casing::Standard, 5)[0].kind, MatchKind::LabelStart);
}

#[test]
fn the_command_run_from_search_most_recently_wins_a_tie_and_then_ribbon_order_and_then_the_registry() {
    let mut late = cand("Align Left", &["Home"], &[], 50);
    let early = cand("Align Right", &["Home"], &[], 3);
    let mut older = cand("Align Center", &["Home"], &[], 1);
    // none recent: order decides
    assert_eq!(ids(&rank("align", &[late, early, older], Casing::Standard, 5)), [2, 1, 0]);
    // recent beats order, the newest first
    late.recent = Some(0);
    older.recent = Some(1);
    assert_eq!(ids(&rank("align", &[late, early, older], Casing::Standard, 5)), [0, 2, 1]);
    // the same order: the lower index
    let a = cand("Align A", &[], &[], 7);
    let b = cand("Align B", &[], &[], 7);
    assert_eq!(ids(&rank("align", &[b, a], Casing::Standard, 5)), [0, 1]);
    // recency never lifts a weaker kind above a stronger one
    let mut weak = cand("Use Align", &[], &[], 0);
    weak.recent = Some(0);
    let strong = cand("Align", &[], &[], 9);
    assert_eq!(ids(&rank("align", &[weak, strong], Casing::Standard, 5)), [1, 0]);
}

#[test]
fn an_english_word_finds_a_command_whose_label_is_translated() {
    let english = English { label: "Bold", place: &["Home", "Font"], keywords: &["strong", "heavy"] };
    let fett = Candidate { label: "Fett", place: &["Start", "Schriftart"], keywords: &["stark"], english: Some(english), order: 1, recent: None };
    let hits = rank("bold", &[fett], Casing::Standard, 5);
    assert_eq!(hits, [Hit { index: 0, kind: MatchKind::LabelStart }]);
    assert_eq!(rank("fett", &[fett], Casing::Standard, 5)[0].kind, MatchKind::LabelStart);
    assert_eq!(rank("heavy", &[fett], Casing::Standard, 5)[0].kind, MatchKind::Other);
    assert_eq!(rank("font", &[fett], Casing::Standard, 5)[0].kind, MatchKind::Other);
    assert_eq!(rank("schriftart", &[fett], Casing::Standard, 5)[0].kind, MatchKind::Other);
    assert!(rank("italic", &[fett], Casing::Standard, 5).is_empty());
}

#[test]
fn every_word_must_match_and_each_is_the_start_of_a_word_not_a_piece_of_one() {
    let c = cand("Print Preview", &["File"], &[], 0);
    assert!(rank("print zzz", &[c], Casing::Standard, 5).is_empty());
    assert!(rank("rint", &[c], Casing::Standard, 5).is_empty());
    assert!(rank("review", &[c], Casing::Standard, 5).is_empty());
    assert_eq!(rank("pre pri", &[c], Casing::Standard, 5).len(), 1);
    // a word of the query may match the same word of the label as another does
    assert_eq!(rank("p p", &[c], Casing::Standard, 5).len(), 1);
}

#[test]
fn case_and_accents_are_ignored_and_a_label_is_read_by_its_words_whatever_separates_them() {
    let ecran = cand("Écran Partagé", &[], &[], 0);
    assert_eq!(rank("ECRAN", &[ecran], Casing::Standard, 5).len(), 1);
    assert_eq!(rank("partage", &[ecran], Casing::Standard, 5).len(), 1);
    let fwd = cand("Bring Forward/Back", &[], &[], 0);
    assert_eq!(rank("back", &[fwd], Casing::Standard, 5).len(), 1);
    let snap = cand("Snap & Glue", &[], &[], 0);
    assert_eq!(rank("glue", &[snap], Casing::Standard, 5).len(), 1);
    assert_eq!(rank("snap glue", &[snap], Casing::Standard, 5)[0].kind, MatchKind::LabelStart);
}

#[test]
fn turkish_text_is_found_by_its_own_rules() {
    let italik = cand("İtalik", &[], &[], 0);
    assert_eq!(rank("italik", &[italik], Casing::Turkic, 5).len(), 1);
    let isi = cand("ISI", &[], &[], 0);
    assert_eq!(rank("ısı", &[isi], Casing::Turkic, 5).len(), 1);
}

#[test]
fn characters_of_a_language_with_no_spaces_start_a_word_each() {
    let join = cand("図形を結合", &["ホーム", "配置"], &["ぐるーぷ"], 0);
    assert_eq!(rank("図形", &[join], Casing::Standard, 5)[0].kind, MatchKind::LabelStart);
    assert_eq!(rank("結合", &[join], Casing::Standard, 5)[0].kind, MatchKind::Label);
    assert_eq!(rank("配置", &[join], Casing::Standard, 5)[0].kind, MatchKind::Other);
    assert!(rank("合結", &[join], Casing::Standard, 5).is_empty());
}

#[test]
fn an_empty_query_and_one_with_no_word_give_nothing() {
    let c = cand("Print", &[], &[], 0);
    for q in ["", "   ", "\t", "---", "&&"] {
        assert!(rank(q, &[c], Casing::Standard, 5).is_empty(), "{q:?}");
    }
}

#[test]
fn at_most_the_limit_come_back_and_a_limit_of_zero_gives_none() {
    let all: Vec<Candidate<'_>> = (0..10).map(|n| cand("Align", &[], &[], n)).collect();
    assert_eq!(rank("align", &all, Casing::Standard, 5).len(), 5);
    assert_eq!(ids(&rank("align", &all, Casing::Standard, 5)), [0, 1, 2, 3, 4]);
    assert!(rank("align", &all, Casing::Standard, 0).is_empty());
    assert_eq!(rank("align", &all, Casing::Standard, 100).len(), 10);
}

#[test]
fn only_the_first_2000_candidates_are_scored() {
    let all: Vec<Candidate<'_>> = (0..5_000).map(|n| cand("Align", &[], &[], n)).collect();
    let hits = rank("align", &all, Casing::Standard, 10_000);
    assert_eq!(hits.len(), MAX_SCORED);
    assert!(hits.iter().all(|h| h.index < MAX_SCORED));
    // a match past the cap is not found
    let mut tail: Vec<Candidate<'_>> = (0..MAX_SCORED as u32).map(|n| cand("Other", &[], &[], n)).collect();
    tail.push(cand("Target", &[], &[], 9_999));
    assert!(rank("target", &tail, Casing::Standard, 5).is_empty());
}

#[test]
fn a_query_of_twenty_words_is_read_as_sixteen() {
    let c = cand("Align", &[], &[], 0);
    let query = format!("{} zzz", "a ".repeat(19));
    assert_eq!(rank(&query, &[c], Casing::Standard, 5).len(), 1, "the words past the sixteenth are not read");
}

#[test]
fn a_query_pasted_at_a_huge_length_is_cut_and_ranked_at_once() {
    let c = cand("Align", &[], &[], 0);
    let query = "a".repeat(100_000);
    assert!(rank(&query, &[c], Casing::Standard, 5).is_empty());
    let prefix = format!("align{}", " x".repeat(60_000));
    assert!(rank(&prefix, &[c], Casing::Standard, 5).is_empty());
}
