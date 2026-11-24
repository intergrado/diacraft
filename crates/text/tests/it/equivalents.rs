//! Tests written for the mutants the first mutation report called equivalent. Each is tried against the real behaviour: where a
//! mutant is still not caught, the test says why no input can tell it apart (an exhaustive check, or the way the code is built).

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, ParaFormat, lay_out};
use proptest::prelude::*;

use crate::{cf, db, em_db, lay_text, left, line_text, run};

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

#[test]
fn text_of_no_size_costs_no_shaping_even_when_every_letter_changes_face() {
    // 5,000 letters alternating between two faces would be 5,000 shaper calls, past the budget of 4,096 and its note, if size 0 text
    // were shaped. It draws nothing, so it is not.
    let d = db(&[SynthFont::new("Em").ascii(1000), SynthFont::new("Han").chars("中", 1000)]);
    let c = CharFormat { size: 0.0, font: "Em".into(), asian_font: "Han".into(), ..CharFormat::default() };
    let text = "a中".repeat(2500);
    let laid = lay_text(&d, &text, &c, &left(), &top_block(), Some(5.0));
    assert!(!laid.notes.contains(&Note::ShapingBudgetUsed), "{:?}", laid.notes);
    // The same text at a size is shaped, and does pass the budget: the test can tell.
    let sized = CharFormat { size: 0.1, ..c };
    assert!(lay_text(&d, &text, &sized, &left(), &top_block(), Some(5.0)).notes.contains(&Note::ShapingBudgetUsed));
}

#[test]
fn a_long_stretch_of_one_script_with_spaces_in_it_is_one_item_when_the_face_is_one() {
    // 3,000 Hebrew words with a space between each, all in one face: with a space starting a new item that is 6,000 shaper calls, past
    // the budget of 4,096. A cluster of no script takes the one before it, so it is one.
    let d = db(&[SynthFont::new("Both").ascii(1000).chars("אבג", 1000)]);
    let c = CharFormat { size: 0.1, font: "Both".into(), complex_font: "Both".into(), ..CharFormat::default() };
    let text = "אבג ".repeat(3000);
    let laid = lay_text(&d, &text, &c, &left(), &top_block(), Some(5.0));
    assert!(!laid.notes.contains(&Note::ShapingBudgetUsed), "{:?}", laid.notes);
}

#[test]
fn a_line_is_taken_whole_when_it_reaches_the_measure_to_the_last_bit_of_the_tolerance_and_not_one_bit_past_it() {
    // A face of letters with no advance, so a cluster's width is its letterspace exactly. `a` is 0.5 and `b` just what brings the line to
    // the measure plus the 1e-9 the layout allows for rounding.
    let d = db(&[SynthFont::new("Zero").ascii(0)]);
    let at_the_edge = (1.0f64 + 1e-9) - 0.5;
    let lines = |b_width: f64| {
        let a = CharFormat { size: 0.1, letterspace: 0.5, ..cf("Zero", 0.0) };
        let b = CharFormat { size: 0.1, letterspace: b_width, ..cf("Zero", 0.0) };
        lay_out(&[run("a", &a, &left()), run("b", &b, &left())], &top_block(), Some(1.0), &d).lines.len()
    };
    assert_eq!(lines(at_the_edge), 1, "equal to measure plus tolerance fits");
    assert_eq!(lines(at_the_edge + 1e-15), 2, "a bit more does not");
}

#[test]
fn every_character_that_has_a_capital_is_a_letter_or_a_digit() {
    // A word's first character gets its capital only if it is alphanumeric. If a character with a capital were not, the check would
    // matter; there is none, over all of Unicode, so it cannot.
    let odd: Vec<char> =
        (0..=0x10FFFFu32).filter_map(char::from_u32).filter(|c| c.to_uppercase().next() != Some(*c) && !c.is_alphanumeric()).collect();
    assert!(odd.is_empty(), "{odd:?}");
}

fn lines_of(laid: &Laid, text: &str) -> Vec<(String, f64)> {
    laid.lines.iter().map(|l| (line_text(text, l), l.width)).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn a_justified_line_wider_than_its_measure_has_no_gap_in_it_to_take_back(
        words in proptest::collection::vec(proptest::sample::select(vec!["a", "bb", "ccc", "(", ")", "dddd", "eeeee", "\u{00A0}"]), 1..14),
        spaces in proptest::collection::vec(0usize..3, 1..14),
        measure in 0.05f64..1.2,
    ) {
        // `(`, a space and a word are held together by the line-break rules (an opening bracket is not left at a line's end), so a
        // line can be too wide with a gap inside it; this looks for one.
        let mut text = String::new();
        for (i, w) in words.iter().enumerate() {
            text.push_str(w);
            text.push_str(&" ".repeat(spaces.get(i).copied().unwrap_or(1)));
        }
        let p = ParaFormat { halign: 3, ..ParaFormat::default() };
        let c = CharFormat { size: 0.1, ..cf("Em", 0.0) };
        let laid = lay_text(&em_db(), &text, &c, &p, &top_block(), Some(measure));
        for (shown, width) in lines_of(&laid, &text) {
            let trimmed = shown.trim_end_matches(' ');
            if width > measure + 1e-6 {
                prop_assert!(!trimmed.contains(' '), "{trimmed:?} is {width} wide in {measure}");
            }
        }
    }
}
