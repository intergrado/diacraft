//! The edges of the layout's rules, with the numbers written out as the specs give them (and not read back from the code), and the
//! caps proved just inside and just outside. Each test names the wrong result it catches.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, DecorationKind, Laid, Note, ParaFormat, Rgba, TabStop, lay_out, limits};

use crate::{baselines, cf, close, db, em_db, gids, glyph_count, lay_text, left, pt, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn lay(text: &str, p: &ParaFormat, width: f64) -> Laid {
    lay_text(&em_db(), text, &tenth(), p, &top_block(), Some(width))
}

// ---- the caps, written out

#[test]
fn the_caps_are_the_documented_numbers() {
    assert_eq!(limits::MAX_CHARS, 262_144);
    assert_eq!(limits::MAX_RUNS, 262_144);
    assert_eq!(limits::MAX_LINES, 100_000);
    assert_eq!(limits::MAX_CLUSTER_CHARS, 32);
    assert_eq!(limits::MAX_SHAPE_ITEMS, 4096);
    assert_eq!(limits::MAX_FACE_SEARCHES, 2048);
    assert_eq!(limits::MAX_NOTES, 256);
    assert_eq!(limits::MAX_NAME_CHARS, 256);
    assert_eq!(limits::MAX_LENGTH, 1.0e7);
}

#[test]
fn text_of_exactly_262144_characters_is_laid_out_whole_and_one_more_is_not() {
    let db = em_db();
    let at = lay_out(&[run(&"a".repeat(262_144), &tenth(), &left())], &top_block(), Some(2.0), &db);
    assert!(!at.notes.iter().any(|n| matches!(n, Note::Truncated { .. })));
    let over = lay_out(&[run(&"a".repeat(262_145), &tenth(), &left())], &top_block(), Some(2.0), &db);
    assert!(over.notes.contains(&Note::Truncated { chars: 1 }));
}

#[test]
fn exactly_262144_runs_are_read_and_one_more_is_not() {
    let db = em_db();
    let c = std::sync::Arc::new(tenth());
    let p = std::sync::Arc::new(left());
    let runs = |n: usize| (0..n).map(|_| diacraft_text::Run::new("", c.clone(), p.clone())).collect::<Vec<_>>();
    assert!(!lay_out(&runs(262_144), &top_block(), Some(2.0), &db).notes.iter().any(|n| matches!(n, Note::RunsCut { .. })));
    assert!(lay_out(&runs(262_145), &top_block(), Some(2.0), &db).notes.contains(&Note::RunsCut { runs: 1 }));
}

#[test]
fn exactly_100000_lines_are_laid_out_and_one_more_paragraph_is_not() {
    let db = em_db();
    let lines = |n: usize| lay_out(&[run(&"a\n".repeat(n), &tenth(), &left())], &top_block(), Some(2.0), &db);
    let at = lines(100_000);
    assert_eq!(at.lines.len(), 100_000);
    assert!(!at.notes.iter().any(|n| matches!(n, Note::LinesCut { .. })));
    let over = lines(100_001);
    assert_eq!(over.lines.len(), 100_000);
    // The paragraph left out is the letter and its line end.
    assert!(over.notes.contains(&Note::LinesCut { chars: 2 }), "{:?}", over.notes);
    assert!(!over.notes.iter().any(|n| matches!(n, Note::Truncated { .. })), "characters were not cut, lines were");
}

#[test]
fn a_letter_keeps_32_characters_of_marks_and_loses_the_rest() {
    let db = db(&[SynthFont::new("Marks").ascii(500).chars("\u{0301}", 0)]);
    let c = cf("Marks", 12.0);
    let with = |marks: usize| lay_out(&[run(&format!("e{}", "\u{0301}".repeat(marks)), &c, &left())], &top_block(), Some(2.0), &db);
    assert!(!with(31).notes.contains(&Note::MarksCut), "a letter and 31 marks are 32 characters");
    assert!(with(32).notes.contains(&Note::MarksCut), "33 are one too many");
}

#[test]
fn a_size_of_ten_million_inches_is_taken_and_one_more_is_not() {
    let db = em_db();
    let size = |v: f64| lay_out(&[run("a", &CharFormat { size: v, ..cf("Em", 12.0) }, &left())], &top_block(), Some(2.0), &db);
    let at = size(1.0e7);
    assert!(!at.notes.contains(&Note::BadNumber));
    close(at.lines[0].runs[0].size, 1.0e7);
    assert!(size(1.0e7 + 1.0).notes.contains(&Note::BadNumber));
    // A scale is a share, and past a million it is no scale.
    let scale = |v: f64| lay_out(&[run("a", &CharFormat { scale: v, ..cf("Em", 12.0) }, &left())], &top_block(), Some(2.0), &db);
    assert_eq!(scale(1.0e6).lines[0].runs[0].x_scale, 1.0e6);
    assert_eq!(scale(1.0e6 + 1.0).lines[0].runs[0].x_scale, 1.0);
}

#[test]
fn three_hundred_missing_names_make_exactly_256_notes() {
    let db = FontDb::bundled();
    let runs: Vec<_> = (0..300).map(|i| run("a", &cf(&format!("Missing {i}"), 12.0), &left())).collect();
    let laid = lay_out(&runs, &top_block(), Some(2.0), &db);
    assert_eq!(laid.notes.iter().filter(|n| matches!(n, Note::StandIn { .. })).count(), 256);
}

// ---- breaking

#[test]
fn a_tab_stop_exactly_at_the_pen_is_not_the_next_stop() {
    // After one letter the pen is at 0.1; a stop at 0.1 is not past it, so the tab goes on to the default step.
    let p = ParaFormat { tabs: vec![TabStop { position: 0.1, alignment: 0 }], ..left() };
    let block = BlockFormat { default_tab_stop: 1.0, ..top_block() };
    let laid = lay_text(&em_db(), "a\tb", &tenth(), &p, &block, Some(5.0));
    let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    close(x[1], 1.0);
}

#[test]
fn a_custom_stop_ahead_beats_a_nearer_default_step_and_the_steps_apply_past_the_last_one() {
    // A custom stop at 2 and default steps of 0.5: from the pen at 0.1 the tab goes to 2, not to the step at 0.5; past the
    // last custom stop the steps apply.
    let p = ParaFormat { tabs: vec![TabStop { position: 2.0, alignment: 0 }], ..left() };
    let block = BlockFormat { default_tab_stop: 0.5, ..top_block() };
    let laid = lay_text(&em_db(), "a\tb\tc\td", &tenth(), &p, &block, Some(5.0));
    let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    close(x[1], 2.0);
    close(x[2], 2.5);
    close(x[3], 3.0);
}

#[test]
fn with_no_stop_at_all_a_tab_is_one_space_and_the_line_is_that_wide() {
    for stop in [0.0, -1.0, f64::NAN] {
        let block = BlockFormat { default_tab_stop: stop, ..top_block() };
        let laid = lay_text(&em_db(), "a\tb\tc", &tenth(), &left(), &block, Some(5.0));
        let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
        close(x[1], 0.2);
        close(x[2], 0.4);
        close(laid.lines[0].width, 0.5);
    }
}

#[test]
fn a_tab_is_part_of_the_width_of_its_line() {
    let p = ParaFormat { tabs: vec![TabStop { position: 0.5, alignment: 0 }], ..left() };
    let laid = lay_text(&em_db(), "a\tb", &tenth(), &p, &top_block(), Some(5.0));
    close(laid.lines[0].width, 0.6);
    close(laid.size.width, 0.6);
}

#[test]
fn nothing_breaks_straight_after_a_tab_so_a_tabbed_word_is_cut_in_the_word() {
    // `aa`, a tab to 0.5, then eight letters: 1.3 in a measure of 1.0. A break after the tab would give `aa<tab>` and `bbbbbbbb`;
    // breaking only at spaces gives a line cut inside the word.
    let block = BlockFormat { default_tab_stop: 0.5, ..top_block() };
    let laid = lay_text(&em_db(), "aa\tbbbbbbbb", &tenth(), &left(), &block, Some(1.0));
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].chars, 0..8, "five letters after the tab stay on the first line");
}

#[test]
fn a_forced_break_at_the_end_of_a_paragraph_opens_an_empty_line() {
    let laid = lay("ab\u{2028}", &left(), 2.0);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(glyph_count(&laid.lines[1]), 0);
    assert_eq!(lay("ab\u{2028}\n", &left(), 2.0).lines.len(), 2, "and before a line feed too");
    assert_eq!(lay("ab\u{2028}cd", &left(), 2.0).lines.len(), 2);
}

#[test]
fn the_line_after_a_forced_break_wraps_by_the_measure_of_later_lines() {
    // IndLeft 0.3 and IndFirst -0.3: the first line has 1.2, later lines 0.9. After the forced break, eleven letters take two lines.
    let p = ParaFormat { ind_left: 0.3, ind_first: -0.3, ..left() };
    let laid = lay("aa\u{2028}bbbbbbbbbbb", &p, 1.2);
    assert_eq!(laid.lines.len(), 3, "{:?}", laid.lines.iter().map(|l| l.chars.clone()).collect::<Vec<_>>());
    close(laid.lines[1].x, 0.3);
}

#[test]
fn a_soft_hyphen_break_that_would_push_the_hyphen_past_the_measure_is_not_taken() {
    // `xx `, six letters, a soft hyphen and six more, in a measure of 0.95. A break at the hyphen would end the line at 0.9 and the
    // hyphen would reach 1.0, so the line breaks after `xx ` instead.
    let t = "xx aaaaaa\u{00AD}bbbbbb";
    let laid = lay_text(&em_db(), t, &tenth(), &left(), &top_block(), Some(0.95));
    assert_eq!(laid.lines[0].chars, 0..3);
    close(laid.lines[0].width, 0.2);
    // In a measure of 1.0 the hyphen fits exactly and is the break.
    let fits = lay_text(&em_db(), t, &tenth(), &left(), &top_block(), Some(1.0));
    assert_eq!(fits.lines[0].chars, 0..10);
    close(fits.lines[0].width, 1.0);
}

#[test]
fn an_ideographic_space_ends_a_line_without_counting_and_a_no_break_space_does_not() {
    let font = SynthFont::new("Em").ascii(1000).chars("\u{00A0}\u{3000}", 1000);
    let db = db(&[font]);
    let wrap = |t: &str| lay_text(&db, t, &tenth(), &left(), &top_block(), Some(1.0));
    // Ten letters and a space that would be the eleventh: the space is trimmed, so the line holds ten and keeps the space.
    let ideographic = wrap("aaaaaaaaaa\u{3000}b");
    assert_eq!(ideographic.lines[0].chars, 0..11);
    close(ideographic.lines[0].width, 1.0);
    // A no-break space is a letter for this: it overflows, and the line is cut before it.
    let no_break = wrap("aaaaaaaaaa\u{00A0}b");
    assert_eq!(no_break.lines[0].chars, 0..10);
}

#[test]
fn a_bidi_mirrored_bracket_in_a_right_to_left_run_takes_the_other_glyph() {
    // In a right-to-left paragraph `(`, a Hebrew letter and `)` are all at level 1, and the shaper mirrors the brackets (rule L4):
    // from the left, the glyph of the logical `)` is drawn as `(`, then the letter, then the logical `(` as `)`.
    let f = SynthFont::new("Mixed").ascii(1000).chars("אב", 1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&d, "(א)", &c, &p, &top_block(), Some(2.0));
    assert_eq!(gids(&laid.lines[0]), vec![u32::from(f.gid('(')), u32::from(f.gid('א')), u32::from(f.gid(')'))]);
}

#[test]
fn trailing_punctuation_of_a_right_to_left_paragraph_moves_to_its_left() {
    // `abc!` in a right-to-left paragraph: the bang is a neutral at the end, so it takes the paragraph's direction and stands to the
    // left of the Latin run.
    let f = SynthFont::new("Mixed").ascii(1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let p = ParaFormat { flags: 1, halign: 0, ..ParaFormat::default() };
    let laid = lay_text(&d, "abc!", &c, &p, &top_block(), Some(2.0));
    let g = gids(&laid.lines[0]);
    assert_eq!(g, vec![u32::from(f.gid('!')), u32::from(f.gid('a')), u32::from(f.gid('b')), u32::from(f.gid('c'))]);
}

// ---- placing

#[test]
fn an_indent_past_the_blocks_right_edge_is_brought_back_to_it() {
    let p = ParaFormat { ind_left: 5.0, ..left() };
    let laid = lay("ab", &p, 1.0);
    close(laid.lines[0].x, 1.0);
}

#[test]
fn a_justified_lines_gaps_are_stretched_so_the_words_after_a_gap_move() {
    // `aa bb cc` in 1.0 is 0.8 wide with two gaps: each grows by 0.1, so the second word starts at 0.4 and the third at 0.8.
    let p = ParaFormat { halign: 3, ..ParaFormat::default() };
    let laid = lay("aa bb cc dd", &p, 1.0);
    let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    assert_eq!(x.len(), 8);
    close(x[3], 0.4);
    close(x[6], 0.8);
    close(x[7] + 0.1, 1.0);
}

#[test]
fn a_line_that_is_too_wide_is_not_shrunk_by_justifying_it() {
    // One word wider than the measure, justified: it is not squeezed, and there is no gap to stretch.
    let p = ParaFormat { halign: 3, ..ParaFormat::default() };
    let laid = lay("aaaaaaaaaaaa", &p, 1.0);
    assert!(laid.lines.iter().all(|l| l.width <= 1.0 + 1e-9));
}

#[test]
fn a_line_wider_than_its_measure_sets_the_overflow_flag_and_one_that_fits_does_not() {
    assert!(lay("abcde", &left(), 0.05).overflow);
    assert!(!lay("abcde", &left(), 2.0).overflow);
}

#[test]
fn the_stack_overflows_when_it_is_taller_than_the_area_inside_the_margins_not_the_block() {
    // A block 1 inch tall with 0.1 margins top and bottom has 0.8 of room; five lines of 0.2 are 1.0 tall, which would fit the block.
    let block = BlockFormat { top_margin: 0.1, bottom_margin: 0.1, height: Some(1.0), ..BlockFormat::default() };
    let five = lay_text(&em_db(), "a\nb\nc\nd\ne\n", &cf("Em", 12.0), &left(), &block, Some(2.0));
    close(five.size.height, 1.0);
    assert!(five.overflow);
    let four = lay_text(&em_db(), "a\nb\nc\nd\n", &cf("Em", 12.0), &left(), &block, Some(2.0));
    assert!(!four.overflow);
}

#[test]
fn a_block_as_wide_as_its_text_aligns_the_text_within_its_own_width_with_the_indent_counted() {
    // No width: the block is as wide as the widest line, indent included, so centred text starts at the indent.
    let p = ParaFormat { halign: 1, ind_left: 0.5, ..ParaFormat::default() };
    let laid = lay_text(&em_db(), "aaaaaaaaaa", &tenth(), &p, &top_block(), None);
    close(laid.lines[0].x, 0.5);
}

#[test]
fn a_space_in_a_bigger_run_at_the_end_of_a_line_does_not_make_the_line_taller() {
    // Nine letters at 0.1, a space of 0.2 that ends the first line, then more letters: the line box is the letters' (0.12).
    let db = em_db();
    let (small, big) = (tenth(), CharFormat { size: 0.2, ..tenth() });
    let runs = [run("aaaaaaaaa", &small, &left()), run(" ", &big, &left()), run("bbbb", &small, &left())];
    let laid = lay_out(&runs, &top_block(), Some(1.0), &db);
    assert_eq!(laid.lines.len(), 2);
    close(laid.lines[0].height, 0.12);
}

#[test]
fn a_line_of_only_spaces_takes_its_height_from_the_spaces_and_not_from_the_line_feed() {
    // `   ` at 24 points and a line feed at 12: the paragraph is empty after trimming, and it is the spaces that size it.
    let db = em_db();
    let (small, big) = (cf("Em", 12.0), cf("Em", 24.0));
    let laid = lay_out(&[run("   ", &big, &left()), run("\n", &small, &left())], &top_block(), Some(2.0), &db);
    assert_eq!(laid.lines.len(), 1);
    close(laid.lines[0].height, 0.4);
}

#[test]
fn a_negative_size_adds_no_height() {
    // Not a positive height from -1.2 times a negative size.
    let c = CharFormat { size: -pt(10.0), ..cf("Em", 12.0) };
    let laid = lay_text(&em_db(), "abc", &c, &left(), &top_block(), Some(2.0));
    close(laid.lines[0].height, 0.0);
    close(laid.size.height, 0.0);
}

#[test]
fn a_transparency_outside_zero_to_one_is_held_to_it() {
    let opacity = |t: f64| {
        lay_text(&em_db(), "a", &CharFormat { transparency: t, ..cf("Em", 12.0) }, &left(), &top_block(), Some(2.0)).lines[0].runs[0].opacity
    };
    assert_eq!(opacity(2.0), 0.0);
    assert_eq!(opacity(-1.0), 1.0);
    assert_eq!(opacity(0.5), 0.5);
    let bg = |t: f64| {
        let b = BlockFormat { background: Some((Rgba::BLACK, t)), ..top_block() };
        lay_text(&em_db(), "a", &cf("Em", 12.0), &left(), &b, Some(2.0)).background.map(|b| b.opacity)
    };
    assert_eq!(bg(3.0), Some(0.0));
    assert_eq!(bg(-3.0), Some(1.0));
}

#[test]
fn only_the_first_64_tab_stops_are_read() {
    // The near stop at 0.5 comes after far ones, and counts only if it is within the first 64.
    let tab_goes_to = |far: usize| {
        let mut stops: Vec<TabStop> = (0..far).map(|i| TabStop { position: 3.0 + 0.01 * i as f64, alignment: 0 }).collect();
        stops.push(TabStop { position: 0.5, alignment: 0 });
        let p = ParaFormat { tabs: stops, ..left() };
        let block = BlockFormat { default_tab_stop: 1.0, ..top_block() };
        let laid = lay_text(&em_db(), "a\tb", &tenth(), &p, &block, Some(5.0));
        let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
        x[1]
    };
    // The 64th stop is read, so it is the nearest; the 65th is not, so the first of the far ones is.
    close(tab_goes_to(63), 0.5);
    close(tab_goes_to(64), 3.0);
}

#[test]
fn indentation_between_markers_is_the_files_own_only_with_a_line_feed_and_a_space_and_tabs_count() {
    let db = em_db();
    let c = tenth();
    let width = |stretch: &str| {
        let laid = lay_out(&[run("a", &c, &left()), run(stretch, &c, &left()), run("b", &c, &left())], &top_block(), Some(5.0), &db);
        (laid.lines.len(), laid.lines.iter().map(|l| l.width).fold(0.0, f64::max))
    };
    // Spaces alone are text: two spaces between the letters.
    assert_eq!(width("  "), (1, 0.4));
    // A line feed with a tab and a space is the file's indentation, and draws nothing.
    assert_eq!(width("\n\t  "), (1, 0.2));
    assert_eq!(width("\n "), (1, 0.2));
    // A line feed alone is a paragraph end.
    assert_eq!(width("\n").0, 2);
}

#[test]
fn a_language_in_capitals_is_still_turkish() {
    let f = SynthFont::new("Cases").ascii(500).chars("İ", 500);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { case: 1, lang: "TR-tr".into(), ..cf("Cases", 12.0) };
    let laid = lay_text(&d, "i", &c, &left(), &top_block(), Some(2.0));
    assert_eq!(gids(&laid.lines[0]), vec![u32::from(f.gid('İ'))]);
}

// ---- faces

#[test]
fn a_joiner_in_a_cluster_does_not_send_it_to_another_face() {
    // `a` and a zero-width joiner in `First`, which has the letter and not the joiner. `Other`, loaded earlier by another run, has
    // both. The joiner never decides, so `First` stays.
    let first = SynthFont::new("First").ascii(500);
    let other = SynthFont::new("Other").ascii(500).chars("\u{200D}", 0);
    let d = db(&[other, first]);
    let runs = [run("x", &cf("Other", 12.0), &left()), run("a\u{200D}", &cf("First", 12.0), &left())];
    let laid = lay_out(&runs, &top_block(), Some(2.0), &d);
    let families: Vec<&str> = laid.lines[0].runs.iter().map(|r| r.face.family()).collect();
    assert_eq!(families, vec!["Other", "First"]);
}

#[test]
fn no_name_is_not_a_missing_name_and_a_missing_name_in_two_weights_is_noted_once() {
    let d = FontDb::bundled();
    for name in ["", "0", "  "] {
        let laid = lay_text(&d, "a", &cf(name, 12.0), &left(), &top_block(), Some(2.0));
        assert!(laid.notes.is_empty(), "{name:?}: {:?}", laid.notes);
    }
    let (plain, bold) = (cf("Gone Face", 12.0), CharFormat { style: 1, ..cf("Gone Face", 12.0) });
    let laid = lay_out(&[run("a", &plain, &left()), run("b", &bold, &left())], &top_block(), Some(2.0), &d);
    assert_eq!(laid.notes.iter().filter(|n| matches!(n, Note::StandIn { .. })).count(), 1);
}

#[test]
fn a_cluster_the_faces_asked_for_lack_is_searched_for_through_the_database() {
    // Searching is budgeted per layout, and a run of the same letter costs one search.
    let d = db(&[SynthFont::new("Plain").ascii(500)]);
    let laid = lay_text(&d, &"中".repeat(5000), &cf("Plain", 12.0), &left(), &top_block(), Some(2.0));
    assert!(!laid.notes.contains(&Note::FaceSearchCut));
    assert_eq!(d.cover_searches(), 1);
}

#[test]
fn underlines_follow_a_raised_run_and_a_tab_that_starts_the_line_is_underlined() {
    // A raised run of the same size: the box of this face is a full em, so only the offset differs, and the run is its own run with its
    // own underline.
    let mut f = SynthFont::new("Full").ascii(500);
    f.superscript.y_size = 1000;
    f.superscript.y_offset = 450;
    let d = db(&[f]);
    let plain = CharFormat { style: 4, ..cf("Full", 20.0) };
    let raised = CharFormat { style: 4, pos: 1, ..cf("Full", 20.0) };
    let laid = lay_out(&[run("a", &plain, &left()), run("b", &raised, &left())], &top_block(), None, &d);
    let tops: Vec<f64> = laid.lines[0].decorations.iter().filter(|x| x.kind == DecorationKind::Underline).map(|x| x.top).collect();
    assert_eq!(tops.len(), 2);
    close(tops[1] - tops[0], 0.45 * pt(20.0));
    // A tab at the start of an underlined line: the underline begins at the line's start.
    let u = CharFormat { style: 4, ..tenth() };
    let laid = lay_text(&em_db(), "\tab", &u, &left(), &top_block(), Some(5.0));
    let d = laid.lines[0].decorations.first().unwrap();
    close(d.x0, laid.lines[0].x);
}

#[test]
fn runs_that_differ_only_in_colour_or_in_their_lines_are_separate_runs() {
    let db = em_db();
    let (red, blue) =
        (CharFormat { color: Rgba { r: 255, g: 0, b: 0, a: 255 }, ..tenth() }, CharFormat { color: Rgba { r: 0, g: 0, b: 255, a: 255 }, ..tenth() });
    let laid = lay_out(&[run("ab", &red, &left()), run("cd", &blue, &left())], &top_block(), None, &db);
    assert_eq!(laid.lines[0].runs.len(), 2);
    let under = CharFormat { style: 4, ..tenth() };
    let laid = lay_out(&[run("ab", &tenth(), &left()), run("cd", &under, &left())], &top_block(), None, &db);
    assert_eq!(laid.lines[0].runs.len(), 2);
    let underlines: Vec<_> = laid.lines[0].decorations.iter().filter(|d| d.kind == DecorationKind::Underline).collect();
    assert_eq!(underlines.len(), 1);
    close(underlines[0].x0, 0.2);
    close(underlines[0].x1, 0.4);
}

#[test]
fn a_double_strikeout_is_two_lines_a_thickness_apart() {
    let c = CharFormat { double_strikethrough: true, ..cf("Em", 20.0) };
    let laid = lay_text(&em_db(), "ab", &c, &left(), &top_block(), None);
    let two: Vec<_> = laid.lines[0].decorations.iter().filter(|d| d.kind == DecorationKind::DoubleStrikethrough).collect();
    assert_eq!(two.len(), 2);
    close((two[0].top - two[1].top).abs(), 2.0 * two[0].thickness);
}

#[test]
fn a_cluster_with_marks_is_shaped_in_one_piece_and_its_glyphs_keep_the_shapers_offsets() {
    // With the real bundled face: `q` (which has no composed form with an acute) and a combining acute. The mark's glyph sits where the shaper put it, not at the pen.
    let db = FontDb::bundled();
    let c = CharFormat { font: "Segoe UI".into(), size: 1.0, ..CharFormat::default() };
    let laid = lay_text(&db, "q\u{0301}", &c, &left(), &top_block(), None);
    let glyphs: Vec<_> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).collect();
    assert_eq!(glyphs.len(), 2);
    let pen_after_e = glyphs[0].x + glyphs[0].advance;
    assert!((glyphs[1].x - pen_after_e).abs() > 1e-9, "the mark is placed by the shaper: x {} against the pen at {}", glyphs[1].x, pen_after_e);
}

#[test]
fn bullets_are_drawn_from_the_first_face_when_it_has_the_mark() {
    // The mark is in the text's own face and in another, loaded earlier by a second run of the paragraph: the paragraph's first face is
    // the one used, and not whichever a search of the database finds first.
    let other = SynthFont::new("Other").ascii(1000).chars("\u{2022}", 1000);
    let text = SynthFont::new("Em").ascii(1000).chars("\u{2022}", 1000);
    let d = db(&[other, text]);
    let p = ParaFormat { bullet: 1, ..left() };
    let runs = [run("a", &tenth(), &p), run("b", &CharFormat { size: 0.1, ..cf("Other", 0.0) }, &p)];
    let laid = lay_out(&runs, &top_block(), Some(2.0), &d);
    assert_eq!(laid.lines[0].bullet.as_ref().unwrap().face.family(), "Em");
}

#[test]
fn a_symbol_face_keeps_its_private_use_mark_before_the_plain_code() {
    let symbol = SynthFont::new("Symbol").chars("\u{00B7}\u{F0B7}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), symbol.clone()]);
    let p = ParaFormat { bullet: 1, ..left() };
    let laid = lay_text(&d, "a", &tenth(), &p, &top_block(), Some(2.0));
    assert_eq!(laid.lines[0].bullet.as_ref().unwrap().glyphs[0].gid, u32::from(symbol.gid('\u{F0B7}')));
}

#[test]
fn a_bullet_string_is_cut_at_256_characters_and_a_note_says_so() {
    // Allows any string, so a long one is drawn as far as a bullet can use, and the cut is not silent.
    let draw = |n: usize| {
        let p = ParaFormat { bullet: 1, bullet_string: "*".repeat(n), ..left() };
        lay_text(&em_db(), "a", &tenth(), &p, &top_block(), Some(2.0))
    };
    let at = draw(256);
    assert_eq!(at.lines[0].bullet.as_ref().unwrap().glyphs.len(), 256);
    assert!(!at.notes.contains(&Note::BulletStringCut));
    let over = draw(257);
    assert_eq!(over.lines[0].bullet.as_ref().unwrap().glyphs.len(), 256);
    assert!(over.notes.contains(&Note::BulletStringCut));
}

#[test]
fn centred_and_right_aligned_lines_are_measured_by_trimmed_width_even_with_two_baselines() {
    // A sanity check across the stack: three paragraphs with different alignments keep their own sides.
    let db = em_db();
    let c = tenth();
    let runs =
        [run("aa\n", &c, &left()), run("aa\n", &c, &ParaFormat { halign: 2, ..ParaFormat::default() }), run("aa\n", &c, &ParaFormat::default())];
    let laid = lay_out(&runs, &top_block(), Some(1.0), &db);
    let xs: Vec<f64> = laid.lines.iter().map(|l| l.x).collect();
    close(xs[0], 0.0);
    close(xs[1], 0.8);
    close(xs[2], 0.4);
    assert_eq!(baselines(&laid).len(), 3);
}

#[test]
fn an_embedding_that_holds_only_left_to_right_letters_still_orders_the_neutral_at_its_end() {
    // `RLE a b !` with the embedding never closed: it is right to left, so the bang at its end stands to the left of the letters.
    let f = SynthFont::new("Mixed").ascii(1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), ..CharFormat::default() };
    let laid = lay_text(&d, "\u{202B}ab!", &c, &left(), &top_block(), Some(2.0));
    // The two controls shape to zero-width glyphs that draw nothing; the letters and the bang are the glyphs with an advance.
    let g: Vec<u32> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).filter(|g| g.advance > 0.0).map(|g| g.gid).collect();
    assert_eq!(g, vec![u32::from(f.gid('!')), u32::from(f.gid('a')), u32::from(f.gid('b'))]);
}

#[test]
fn a_tab_on_a_bulleted_first_line_starts_from_where_its_text_starts() {
    // Bullet 1 and a gap of 0.25: the text starts at 0.25, `a` ends at 0.35, and the stop at 0.5 is measured from the area's edge, so
    // `b` is at 0.5. Starting the pen at the bullet's place would send it to 0.75.
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, tabs: vec![TabStop { position: 0.5, alignment: 0 }], ..left() };
    let laid = lay_text(&d, "a\tb", &tenth(), &p, &top_block(), Some(5.0));
    let x: Vec<f64> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    close(x[0], 0.25);
    close(x[1], 0.5);
}

#[test]
fn a_soft_hyphen_draws_a_hyphen_only_where_the_line_was_cut_at_it() {
    // At the end of the text, or before a forced break, nothing is cut, so nothing is drawn for it.
    for t in ["aa\u{00AD}", "aa\u{00AD}\u{2028}bb"] {
        let laid = lay(t, &left(), 2.0);
        assert_eq!(glyph_count(&laid.lines[0]), 2, "{t:?}");
        close(laid.lines[0].width, 0.2);
    }
}

#[test]
fn a_background_needs_text_to_hug_and_takes_in_the_bullet() {
    let b = BlockFormat { background: Some((Rgba::BLACK, 0.0)), ..top_block() };
    let empty = lay_text(&em_db(), "", &tenth(), &left(), &b, Some(2.0));
    assert_eq!(empty.lines.len(), 1);
    assert!(empty.background.is_none());
    // The rectangle starts at the bullet, which is left of the text.
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, ..left() };
    let laid = lay_text(&d, "ab", &tenth(), &p, &b, Some(2.0));
    let bg = laid.background.unwrap();
    close(bg.rect.x0, 0.0);
    close(bg.rect.x1, 0.25 + 0.2);
}

#[test]
fn space_between_paragraphs_is_not_added_between_the_lines_of_one() {
    // A paragraph, then one wrapped to three lines with SpBefore and SpAfter of an inch: the gap is above the second paragraph's first
    // line and nowhere else, so the steps between its own lines are the line boxes'.
    let p = ParaFormat { sp_before: 1.0, sp_after: 1.0, ..left() };
    let runs = [run("x\n", &tenth(), &left()), run("aaaaa bbbbb ccccc", &tenth(), &p)];
    let laid = lay_out(&runs, &top_block(), Some(0.6), &em_db());
    assert_eq!(laid.lines.len(), 4);
    close(laid.lines[0].baseline - laid.lines[1].baseline, 0.12 + 1.0);
    close(laid.lines[1].baseline - laid.lines[2].baseline, 0.12);
    close(laid.lines[2].baseline - laid.lines[3].baseline, 0.12);
}

#[test]
fn a_justified_line_with_no_gap_in_it_is_not_stretched() {
    // `aaaaaaaa bbbb` in 0.9: the first line is the word and the space that ends it, with no gap inside to stretch.
    let p = ParaFormat { halign: 3, ..ParaFormat::default() };
    let laid = lay("aaaaaaaa bbbb", &p, 0.9);
    assert_eq!(laid.lines.len(), 2);
    close(laid.lines[0].width, 0.8);
    close(laid.lines[0].x, 0.0);
}
