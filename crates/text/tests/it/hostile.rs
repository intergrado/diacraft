//! A drawing is hostile: its text and format runs reach the layout. These are crafted inputs, not random ones (random mutation does
//! not build complexity inputs). Each is held to what it costs by a count of the work (`diacraft_text::work`: the clusters the line
//! breaker looked at, the shaper's calls, the searches of the font database), which does not move with the load on the machine, and
//! by a bound on the time that only a hang reaches.
//!
//! Every case must finish with no panic, give finite numbers, and do work that grows linearly with the text.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, ParaFormat, Run, lay_out, limits, work};
use proptest::prelude::*;

use crate::{cf, db, left, pt, run};

/// A hang guard and no measure of cost (a cost is counted): the slowest case takes about a second on a busy machine, so a minute is
/// generous for one at load 50, and a layout that hangs still fails.
const BOUND: Duration = Duration::from_secs(60);

fn font_db() -> FontDb {
    db(&[SynthFont::new("H").ascii(500).chars("אבג", 500).chars("e\u{0301}\u{0300}", 500), SynthFont::new("H2").chars("中文", 1000)])
}

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 1, height: Some(2.0), ..BlockFormat::default() }
}

fn timed(name: &str, f: impl FnOnce() -> Laid) -> (Laid, Duration) {
    let start = Instant::now();
    let laid = f();
    let took = start.elapsed();
    eprintln!("{name}: {took:?} ({} lines)", laid.lines.len());
    assert!(took < BOUND, "{name} took {took:?}");
    (laid, took)
}

/// As [`timed`], with the counts of the work started over first; gives the clusters the line breaker looked at as well.
fn counted(name: &str, f: impl FnOnce() -> Laid) -> (Laid, u64) {
    work::reset();
    let (laid, _) = timed(name, f);
    (laid, work::cluster_steps())
}

/// The line breaker looks at each cluster once, and again where a line is cut short of where its scan reached (`breaking`): under one and
/// a half looks for each cluster here. A step that backed up over a line, or over the paragraph, would be many times that.
#[track_caller]
fn linear(clusters: usize, steps: u64) {
    let clusters = clusters as u64;
    assert!(steps <= clusters + clusters / 2, "{steps} looks at {clusters} clusters");
    assert!(steps >= clusters, "{steps} looks at {clusters} clusters: it did not read them all");
}

/// Every number a renderer will use is finite.
#[track_caller]
fn finite(laid: &Laid) {
    assert!(laid.size.width.is_finite() && laid.size.height.is_finite());
    for l in &laid.lines {
        assert!([l.baseline, l.top, l.bottom, l.height, l.x, l.width].iter().all(|v| v.is_finite()), "{l:?}");
        // And every place a glyph is drawn at, and every line through the text.
        for r in l.runs.iter().chain(l.bullet.iter()) {
            assert!(r.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite() && g.advance.is_finite()), "{r:?}");
        }
        for d in &l.decorations {
            let r = d.rect();
            assert!([r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()), "{d:?}");
        }
        for r in &l.runs {
            assert!(r.size.is_finite() && r.x_scale.is_finite() && r.embolden.is_finite() && r.slant.is_finite() && r.opacity.is_finite());
            assert!(r.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite() && g.advance.is_finite()));
        }
        assert!(l.decorations.iter().all(|d| [d.x0, d.x1, d.top, d.thickness, d.opacity].iter().all(|v| v.is_finite())));
    }
}

fn lay(text: &str, c: &CharFormat, p: &ParaFormat, block: &BlockFormat, width: Option<f64>) -> Laid {
    lay_out(&[run(text, c, p)], block, width, &font_db())
}

#[test]
fn a_very_long_run_with_no_break_point_is_linear() {
    let c = cf("H", 12.0);
    for n in [16_384usize, 65_536, limits::MAX_CHARS] {
        let text = "a".repeat(n);
        let (laid, steps) = counted(&format!("one word of {n}"), || lay(&text, &c, &left(), &block(), Some(2.0)));
        finite(&laid);
        linear(n, steps);
        // 6 points a letter in a measure of 2 inches: twenty-four letters to a line, so the lines are the letters over that.
        assert!(laid.lines.len() >= n / 24 - 1 || laid.lines.len() == limits::MAX_LINES);
    }
}

#[test]
fn a_very_long_run_of_words_wraps_in_linear_time() {
    let c = cf("H", 12.0);
    for n in [16_384usize, 65_536, 200_000] {
        let text = "abc de ".repeat(n / 7);
        let (laid, steps) = counted(&format!("words of {n}"), || lay(&text, &c, &left(), &block(), Some(2.0)));
        finite(&laid);
        linear(text.chars().count(), steps);
    }
}

#[test]
fn thousands_of_format_runs_are_linear() {
    let (a, b) = (cf("H", 12.0), CharFormat { style: 1, size: pt(14.0), ..cf("H", 12.0) });
    let (pa, pb) = (Arc::new(left()), Arc::new(ParaFormat { halign: 2, ..left() }));
    for n in [4_000usize, 16_000, 64_000] {
        let runs: Vec<Run> = (0..n)
            .map(|i| if i % 2 == 0 { Run::new("ab", Arc::new(a.clone()), pa.clone()) } else { Run::new("c ", Arc::new(b.clone()), pb.clone()) })
            .collect();
        let (laid, steps) = counted(&format!("{n} runs"), || lay_out(&runs, &block(), Some(2.0), &font_db()));
        finite(&laid);
        // Two letters in each run.
        linear(2 * n, steps);
    }
}

#[test]
fn a_face_or_direction_that_changes_at_every_letter_does_not_make_the_shaper_the_bottleneck() {
    // Latin and Han letters in two faces: a shaper call for each letter, until the budget ends that and the rest is measured from the
    // cmap. The same for text that changes direction at every letter.
    let text = "a中".repeat(40_000);
    let c = CharFormat { asian_font: "H2".into(), ..cf("H", 12.0) };
    work::reset();
    let (laid, _) = timed("alternating faces", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
    assert!(laid.notes.contains(&Note::ShapingBudgetUsed), "{:?}", laid.notes);
    // 80,000 letters, and the shaper is called for no more than the budget (a call for each letter would be 80,000).
    assert!((1..=limits::MAX_SHAPE_ITEMS as u64).contains(&work::shaper_calls()), "{} shaper calls", work::shaper_calls());
    let text = "a\u{5d0}".repeat(40_000);
    work::reset();
    let (laid, _) = timed("alternating directions", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
    assert!(laid.notes.contains(&Note::ShapingBudgetUsed), "{:?}", laid.notes);
    assert!((1..=limits::MAX_SHAPE_ITEMS as u64).contains(&work::shaper_calls()), "{} shaper calls", work::shaper_calls());
    // Within the budget, no note.
    let small = lay(&"a中".repeat(100), &c, &left(), &block(), Some(2.0));
    assert!(!small.notes.contains(&Note::ShapingBudgetUsed));
}

#[test]
fn a_huge_or_tiny_or_hostile_size_is_arithmetic_that_stays_finite() {
    let sizes = [1e300, 1e-300, f64::MAX, f64::MIN_POSITIVE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1e7, 1e7 + 1.0, 5e-324];
    for size in sizes {
        for scale in [1.0, 1e300, f64::NAN, -2.0, f64::INFINITY] {
            let c = CharFormat { size, scale, letterspace: size, complex_size: size, ..cf("H", 12.0) };
            let p = ParaFormat {
                sp_line: size,
                sp_before: size,
                sp_after: size,
                ind_left: size,
                ind_right: size,
                ind_first: size,
                ..ParaFormat::default()
            };
            let b = BlockFormat {
                left_margin: size,
                right_margin: size,
                top_margin: size,
                bottom_margin: size,
                height: Some(size),
                default_tab_stop: size,
                ..block()
            };
            let laid = lay("ab אב\tc\n中 e\u{0301}", &c, &p, &b, Some(size));
            finite(&laid);
        }
    }
}

#[test]
fn a_block_of_no_width_with_a_huge_text_is_capped_by_lines_and_time() {
    let c = cf("H", 12.0);
    let text = "abcdefgh ".repeat(60_000);
    let (laid, _) = timed("zero width, 540k characters", || lay(&text, &c, &left(), &block(), Some(0.0)));
    finite(&laid);
    assert!(laid.lines.len() <= limits::MAX_LINES);
    assert!(laid.notes.iter().any(|n| matches!(n, Note::LinesCut { chars } if *chars > 0)), "{:?}", laid.notes);
}

#[test]
fn text_past_the_cap_is_cut_and_said_so_and_not_slowly() {
    let c = cf("H", 12.0);
    let text = "word ".repeat(limits::MAX_CHARS); // five times the cap
    let (laid, _) = timed("past the character cap", || lay(&text, &c, &left(), &block(), Some(2.0)));
    assert!(laid.notes.iter().any(|n| matches!(n, Note::Truncated { chars } if *chars > 0)), "{:?}", laid.notes);
    finite(&laid);
}

#[test]
fn deep_bidirectional_nesting_is_bounded() {
    let c = cf("H", 12.0);
    for opener in ["\u{202B}", "\u{202E}", "\u{2067}", "\u{2066}", "\u{2068}", "\u{202A}"] {
        let text = format!("{}abc אבג", opener.repeat(100_000));
        let (laid, _) =
            timed(&format!("100k of U+{:04X}", opener.chars().next().map_or(0, |c| c as u32)), || lay(&text, &c, &left(), &block(), Some(2.0)));
        finite(&laid);
    }
    // Closers with nothing to close, and alternating open and close.
    let text = format!("{}abc", "\u{202C}\u{2069}".repeat(100_000));
    finite(&timed("100k closers", || lay(&text, &c, &left(), &block(), Some(2.0))).0);
    let text = format!("{}אב", "\u{2067}\u{2069}".repeat(100_000));
    finite(&timed("100k isolate pairs", || lay(&text, &c, &left(), &block(), Some(2.0))).0);
    // Nested pairs 100,000 deep.
    let text = format!("{}אב{}", "\u{2067}".repeat(50_000), "\u{2069}".repeat(50_000));
    finite(&timed("50k nested isolates", || lay(&text, &c, &left(), &block(), Some(2.0))).0);
}

#[test]
fn stacks_of_combining_marks_are_cut_and_not_quadratic() {
    let c = cf("H", 12.0);
    // One letter with a hundred thousand marks.
    let text = format!("e{}x", "\u{0301}".repeat(100_000));
    let (laid, _) = timed("one letter, 100k marks", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
    assert!(laid.notes.contains(&Note::MarksCut), "{:?}", laid.notes);
    // Zalgo: many letters, each with fifty marks.
    let text = format!("e{}", "\u{0301}\u{0300}".repeat(25)).repeat(4_000);
    let (laid, _) = timed("4,000 letters with 50 marks", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
    // A hundred thousand marks with no letter before them, and zero-width joiners.
    let text = "\u{0301}".repeat(100_000) + &"\u{200D}".repeat(100_000);
    finite(&timed("marks and joiners alone", || lay(&text, &c, &left(), &block(), Some(2.0))).0);
}

#[test]
fn a_hundred_thousand_line_ends_and_forced_breaks_are_capped() {
    let c = cf("H", 12.0);
    for sep in ["\n", "\u{2028}", "\n\n", "\t", " \n "] {
        let text = format!("a{sep}").repeat(100_000);
        let (laid, _) = timed(&format!("100k of {sep:?}"), || lay(&text, &c, &left(), &block(), Some(2.0)));
        finite(&laid);
        assert!(laid.lines.len() <= limits::MAX_LINES);
    }
}

#[test]
fn a_hostile_name_and_a_hostile_text_together() {
    let c = CharFormat { font: "A".repeat(1 << 20), asian_font: "\0".repeat(1 << 20), complex_font: "../".repeat(1 << 18), ..cf("H", 12.0) };
    let text = "a中אb ".repeat(10_000);
    let (laid, _) = timed("hostile names", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
}

#[test]
fn bundled_faces_handle_a_page_of_mixed_scripts_quickly() {
    // The real bundled faces, not synthetic ones: Latin, Greek, Cyrillic, and letters they lack.
    let db = FontDb::bundled();
    let text = "The quick brown fox Ελληνικά Кириллица 中文 עברית العربية नमस्ते\n".repeat(2_000);
    let c = CharFormat { font: "Calibri".into(), ..CharFormat::default() };
    let (laid, _) = timed("mixed scripts, bundled faces", || lay_out(&[run(&text, &c, &left())], &block(), Some(5.0), &db));
    finite(&laid);
}

#[test]
fn empty_and_degenerate_inputs() {
    let db = font_db();
    for runs in
        [vec![], vec![run("", &cf("H", 12.0), &left())], vec![run("\n", &cf("H", 12.0), &left())], vec![run("\u{0}", &cf("H", 12.0), &left())]]
    {
        for width in [None, Some(0.0), Some(-1.0), Some(f64::NAN), Some(1.0)] {
            finite(&lay_out(&runs, &block(), width, &db));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Strings from an alphabet chosen to hurt: letters of three scripts, marks, joiners, bidi controls, tabs, both kinds of line end,
    /// spaces and odd controls, with sizes and widths from a list of nasty numbers.
    #[test]
    fn nasty_text_and_numbers_never_panic_and_stay_finite(
        text in proptest::collection::vec(proptest::sample::select(vec![
            "a", "b", " ", "  ", "\n", "\u{2028}", "\t", "中", "文", "א", "ב", "e", "\u{0301}", "\u{200D}", "\u{200B}", "\u{00AD}", "\u{00A0}",
            "\u{202B}", "\u{202C}", "\u{2067}", "\u{2069}", "\u{0}", "\r", "\u{1F600}", "-", "/", ".", "9",
        ]), 0..200),
        size in proptest::sample::select(vec![0.0, 1.0 / 6.0, 1e-9, 1e3, 1e9, f64::NAN, -3.0]),
        width in proptest::sample::select(vec![None, Some(0.0), Some(0.3), Some(2.0), Some(-1.0), Some(f64::INFINITY)]),
        halign in 0u8..6,
        flags in 0u8..3,
        bullet in 0u8..9,
        vertical in any::<bool>(),
    ) {
        let text: String = text.concat();
        let c = CharFormat { size, asian_font: "H".into(), ..cf("H", 12.0) };
        let p = ParaFormat { halign, flags, bullet, bullet_string: if bullet > 7 { "*".into() } else { String::new() }, tabs: vec![diacraft_text::TabStop { position: 0.4, alignment: (halign % 4) }], ..ParaFormat::default() };
        // Down the block as well as across it (`TextDirection` 1): the same text, the same numbers, the same bounds.
        let laid = lay(&text, &c, &p, &BlockFormat { text_direction: u8::from(vertical), ..block() }, width);
        finite(&laid);
        // The lines' character ranges rise and stay inside the source.
        let total = text.chars().count();
        let mut last = 0usize;
        for l in &laid.lines {
            prop_assert!(l.chars.start >= last.saturating_sub(0) || l.chars.start == l.chars.end);
            prop_assert!(l.chars.end <= total);
            last = l.chars.start;
        }
    }
}

#[test]
fn a_long_run_of_spaces_between_words_is_linear_even_justified() {
    let c = cf("H", 12.0);
    let p = ParaFormat { halign: 3, ..ParaFormat::default() };
    let text = format!("a{}b{}c", " ".repeat(100_000), " ".repeat(100_000));
    let (laid, _) = timed("200k spaces", || lay_out(&[run(&text, &c, &p)], &block(), Some(2.0), &font_db()));
    finite(&laid);
    let (laid, _) = timed("200k spaces, no wrap", || lay_out(&[run(&text, &c, &p)], &block(), None, &font_db()));
    finite(&laid);
}

#[test]
fn a_hundred_thousand_bulleted_paragraphs_do_not_make_a_shaper_call_each() {
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let db = db(&[SynthFont::new("H").ascii(500), marks]);
    let c = cf("H", 12.0);
    let p = ParaFormat { bullet: 1, halign: 0, ..ParaFormat::default() };
    let text = "a\n".repeat(100_000);
    work::reset();
    let (laid, _) = timed("100k bullets", || lay_out(&[run(&text, &c, &p)], &block(), Some(2.0), &db));
    finite(&laid);
    // A hundred thousand paragraphs, each with a bullet and a letter: the shaper is called within its budget, not for each of them.
    assert!((1..=limits::MAX_SHAPE_ITEMS as u64).contains(&work::shaper_calls()), "{} shaper calls", work::shaper_calls());
    assert!(laid.lines.iter().all(|l| l.bullet.is_some()), "every paragraph has its bullet even past the budget");
}

#[test]
fn a_hundred_thousand_clusters_no_face_has_are_not_a_scan_of_every_face_each() {
    // Two thousand faces, none with the mark: each cluster of a letter and the mark would scan them all without a memory of the answer.
    let mut db = FontDb::empty();
    for i in 0..2_000 {
        assert_eq!(db.add_font_data(SynthFont::new(&format!("F{i}")).ascii(500).build()).added, 1);
    }
    let c = cf("F0", 12.0);
    let text = "e\u{0301}".repeat(100_000);
    work::reset();
    let (laid, _) = timed("100k uncovered clusters, 2,000 faces", || lay_out(&[run(&text, &c, &left())], &block(), Some(2.0), &db));
    finite(&laid);
    // One cluster repeated 100,000 times is one search of the 2,000 faces, and its answer is remembered.
    assert_eq!(work::face_searches(), 1, "the answer of a search is remembered for the layout");
    assert!(!laid.notes.contains(&Note::FaceSearchCut), "{:?}", laid.notes);
    let text = (0..60_000u32).filter_map(|i| char::from_u32(0x4E00 + i)).collect::<String>();
    work::reset();
    let (laid, _) = timed("60k distinct uncovered letters, 2,000 faces", || lay_out(&[run(&text, &c, &left())], &block(), Some(2.0), &db));
    finite(&laid);
    assert!(laid.notes.contains(&Note::FaceSearchCut), "{:?}", laid.notes);
    // Sixty thousand different letters: the searches stop at the layout's budget, each of them a pass over 2,000 faces.
    assert_eq!(work::face_searches(), limits::MAX_FACE_SEARCHES as u64, "the searches are the budget's and no more");
}

#[test]
fn a_run_for_every_character_each_naming_its_own_missing_face_keeps_the_notes_bounded() {
    let db = FontDb::bundled();
    let runs: Vec<Run> = (0..50_000).map(|i| run("a", &cf(&format!("Missing Face Number {i}"), 12.0), &left())).collect();
    let (laid, _) = timed("50k distinct missing names", || lay_out(&runs, &block(), Some(2.0), &db));
    finite(&laid);
    assert!(laid.notes.len() <= 300, "{} notes", laid.notes.len());
}

#[test]
fn a_laid_block_and_the_font_database_may_cross_threads() {
    // A renderer may lay out or draw on a worker thread; the database is shared.
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Laid>();
    send_sync::<FontDb>();
    send_sync::<diacraft_fonts::Face>();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Any text at all: every scalar value the generator can make, controls and private use and the lot.
    #[test]
    fn arbitrary_unicode_never_panics(text in any::<String>(), wide in any::<bool>(), size in 0.0f64..2.0, bold in any::<bool>()) {
        let c = CharFormat { size, style: u8::from(bold) | 4, asian_font: "H2".into(), ..cf("H", 12.0) };
        let laid = lay(&text, &c, &left(), &block(), if wide { Some(3.0) } else { Some(0.2) });
        finite(&laid);
    }
}

#[test]
fn the_character_cap_is_exact() {
    let c = cf("H", 12.0);
    let at = lay(&"a".repeat(limits::MAX_CHARS), &c, &left(), &block(), Some(2.0));
    assert!(!at.notes.iter().any(|n| matches!(n, Note::Truncated { .. })), "{:?}", at.notes);
    let over = lay(&"a".repeat(limits::MAX_CHARS + 1), &c, &left(), &block(), Some(2.0));
    assert!(over.notes.contains(&Note::Truncated { chars: 1 }), "{:?}", over.notes);
    // Across runs the count is of all the characters of all of them.
    let runs = [run(&"a".repeat(limits::MAX_CHARS - 1), &c, &left()), run("bcd", &c, &left())];
    let laid = lay_out(&runs, &block(), Some(2.0), &font_db());
    assert!(laid.notes.contains(&Note::Truncated { chars: 2 }), "{:?}", laid.notes);
    let last_char = laid.lines.last().map(|l| l.chars.end);
    assert_eq!(last_char, Some(limits::MAX_CHARS));
}

#[test]
fn the_run_cap_is_exact() {
    // Runs with no characters cost a marker each and count against their own cap.
    let c = Arc::new(cf("H", 12.0));
    let p = Arc::new(left());
    let mk = |n: usize| (0..n).map(|_| Run::new("", c.clone(), p.clone())).collect::<Vec<Run>>();
    let at = lay_out(&mk(limits::MAX_RUNS), &block(), Some(2.0), &font_db());
    assert!(!at.notes.iter().any(|n| matches!(n, Note::RunsCut { .. })), "{:?}", at.notes);
    let over = lay_out(&mk(limits::MAX_RUNS + 1), &block(), Some(2.0), &font_db());
    assert!(over.notes.contains(&Note::RunsCut { runs: 1 }), "{:?}", over.notes);
}

#[test]
fn a_text_of_tens_of_megabytes_costs_a_count_and_not_a_pass_of_case_mapping() {
    let c = CharFormat { case: 2, ..cf("H", 12.0) };
    let text = "word ".repeat(6_000_000);
    let (laid, _) = timed("30 MB of text with Case 2", || lay(&text, &c, &left(), &block(), Some(2.0)));
    finite(&laid);
    assert!(laid.notes.iter().any(|n| matches!(n, Note::Truncated { .. })));
}
