//! Hostile labels, counted: each cap of the reader of labels gets an input built in code that sits at it and one past it, and the work the read did is counted in
//! steps ([`ReadWork::label_steps`](diacraft_mxfile::ReadWork)), because a fuzzer finds crashes and misses hangs. Nothing here asserts a time: the
//! `#[ignore]`d twins at the end run the same inputs beside a yardstick, by hand, on a quiet machine (`cargo test -p diacraft-mxfile -- --ignored`).

use std::time::{Duration, Instant};

use diacraft_mxfile::html::{LINE_BREAK, Options, read_html, read_plain, warn};
use diacraft_mxfile::limits::{MAX_CASES, MAX_LABEL_BYTES};
use diacraft_mxfile::{Limits, Reading};

use crate::label_table::{BUILT, Got, cases, count, html, html_with, lines, plain};

const MIB: usize = 1 << 20;

/// The steps a read counted.
fn steps(got: &Got) -> u64 {
    got.report.work.label_steps
}

/// The length of a text as the counts have it.
fn n(len: usize) -> u64 {
    u64::try_from(len).unwrap()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the cap of a label

#[test]
fn a_label_of_the_cap_is_read_whole_and_one_byte_more_is_cut_and_counted() {
    assert_eq!(MAX_LABEL_BYTES, MIB);
    for read in [html, plain] {
        let at = read(&"a".repeat(MIB));
        assert_eq!(lines(&at)[0].len(), MIB);
        assert_eq!(count(&at.report, warn::LABEL_TOO_LONG), 0, "a label at the cap is not cut");
        let past = read(&"a".repeat(MIB + 1));
        assert_eq!(lines(&past)[0].len(), MIB, "cut at the cap");
        assert_eq!(count(&past.report, warn::LABEL_TOO_LONG), 1);
        assert_eq!(cases(&past.report, warn::LABEL_TOO_LONG), [format!("{} bytes", MIB + 1)]);
    }
}

#[test]
fn a_label_is_cut_back_to_a_character_and_not_in_the_middle_of_one() {
    // 1 + 2 * 524,288 bytes: the cap falls between the two bytes of the last character.
    let text = format!("a{}", "\u{e9}".repeat(MIB / 2));
    assert_eq!(text.len(), MIB + 1);
    for read in [html, plain] {
        let got = read(&text);
        assert_eq!(lines(&got)[0].len(), MIB - 1, "the character that would be cut in two is left out");
        assert_eq!(count(&got.report, warn::LABEL_TOO_LONG), 1);
    }
    // A smaller cap, the same.
    let limits = Limits { max_label_bytes: 100, ..Limits::default() };
    let got = html_with(&"x".repeat(150), &Options::default(), &limits);
    assert_eq!(lines(&got)[0].len(), 100);
    // A cut in the middle of a tag drops the tag, and in the middle of a reference leaves its text.
    let got = html_with(&format!("{}<b class=\"x\">y</b>", "x".repeat(95)), &Options { cell: "c3", ..Options::default() }, &limits);
    assert_eq!(lines(&got), ["x".repeat(95)]);
    assert_eq!(cases(&got.report, warn::LABEL_TOO_LONG), ["c3: 113 bytes"]);
    let got = html_with(&format!("{}&amp;", "x".repeat(97)), &Options::default(), &limits);
    assert_eq!(lines(&got), [format!("{}&am", "x".repeat(97))]);
}

#[test]
fn a_label_of_several_megabytes_costs_the_cap_and_not_its_length() {
    let big = "<b>a</b>".repeat(2_000_000);
    let got = html(&big);
    assert_eq!(count(&got.report, warn::LABEL_TOO_LONG), 1);
    assert!(steps(&got) < 4 * n(MIB), "{}", steps(&got));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ nesting

#[test]
fn a_million_b_tags_deep_are_64_levels_and_the_rest_is_text_with_no_recursion() {
    // 349,000 `<b>` and a word: under the cap, and 349,000 levels deep if they were followed.
    let text = format!("{}deep", "<b>".repeat(349_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["deep"]);
    assert_eq!(got.label.paras[0].runs[0].fmt.bold, Some(true));
    assert_eq!(
        steps(&got),
        n(text.len()) + 349_000 + 1 + 4 + BUILT,
        "the bytes, a token for each tag and the word, the bytes of the word, one paragraph and one run"
    );
    // The same with the end tags: each closes one that is open, or nothing.
    let text = format!("{}x{}", "<b>".repeat(140_000), "</b>".repeat(140_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["x"]);
    assert_eq!(steps(&got), n(text.len()) + 280_000 + 1 + 1 + BUILT);
    // Blocks and lists the same.
    let text = format!("{}x", "<div><ul><li>".repeat(30_000));
    let got = html(&text);
    assert!(got.label.paras.len() <= 64, "{}", got.label.paras.len());
    let text = format!("{}x", "<table><tr><td>".repeat(60_000));
    assert!(html(&text).label.paras.len() <= 64);
    let text = format!("{}x", "<a href=\"https://a.b\" style=\"color:red\">".repeat(25_000));
    assert_eq!(lines(&html(&text)), ["x"]);
}

#[test]
fn end_tags_that_close_nothing_cost_a_step_each_whatever_is_open() {
    // 64 open `i` and 100,000 `</b>`: no search of the stack for each.
    let text = format!("{}x{}", "<i>".repeat(64), "</b>".repeat(100_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["x"]);
    assert_eq!(steps(&got), n(text.len()) + 64 + 1 + 100_000 + 1 + BUILT);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ unclosed

#[test]
fn a_hundred_thousand_unclosed_scripts_are_one_walk() {
    let text = "<script".repeat(100_000);
    let got = html(&text);
    assert!(got.label.is_empty());
    assert_eq!(steps(&got), n(text.len()) + 1, "the first tag has no end and takes the rest: one token");
    let text = format!("a{}", "<script>".repeat(100_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["a"]);
    assert_eq!(steps(&got), n(text.len()) + 2 + 1 + BUILT, "a, the first script, a byte of the text, and what the a builds");
    let text = "<script>x</script>".repeat(50_000);
    let got = html(&text);
    assert!(got.label.is_empty());
    assert_eq!(steps(&got), n(text.len()) + 50_000, "a token for each script, and none for what is inside");
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 0);
}

#[test]
fn unclosed_tags_quotes_and_comments_cost_one_walk_to_the_end() {
    for unit in ["<b", "<b a=\"", "<!--", "</b", "<!", "<?x", "<b a='x", "<a href=\"x\" b"] {
        let text = unit.repeat(MIB / unit.len() - 1);
        let got = html(&text);
        assert!(got.label.is_empty(), "{unit}");
        assert_eq!(steps(&got), n(text.len()) + 1, "{unit}: the bytes, and one token that takes the rest");
    }
}

#[test]
fn a_hundred_thousand_unclosed_nested_elements_of_markup_are_one_element_dropped() {
    let text = "<svg>".repeat(100_000);
    let got = html(&text);
    assert!(got.label.is_empty());
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 1);
    assert_eq!(steps(&got), n(text.len()) + 100_000, "a token for each tag");
    // Pairs of them are each dropped and counted, and the cases kept are capped while the count goes on.
    let text = "<svg></svg>".repeat(90_000);
    let got = html(&text);
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 90_000);
    assert_eq!(cases(&got.report, warn::LABEL_CONTENT_DROPPED).len(), MAX_CASES);
    let text = "<img>".repeat(200_000);
    let got = html(&text);
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 200_000);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ references

#[test]
fn a_million_amp_references_are_cut_at_the_cap_and_each_is_charged_for_its_look_ahead() {
    let text = "&amp;".repeat(1_000_000);
    let got = html(&text);
    assert_eq!(count(&got.report, warn::LABEL_TOO_LONG), 1);
    let kept = text.get(..MIB).unwrap();
    let amps = n(kept.matches('&').count());
    // 209,715 whole references and the first byte of one more, which is a lone `&`.
    assert_eq!(lines(&got)[0], "&".repeat(209_716));
    assert_eq!(steps(&got), n(MIB) + 1 + n(MIB) + 33 * amps + BUILT);
}

#[test]
fn a_flood_of_stray_ampersands_costs_the_look_ahead_of_each_and_no_more() {
    let text = "&".repeat(MIB);
    let got = html(&text);
    assert_eq!(lines(&got)[0], text);
    assert_eq!(steps(&got), n(MIB) + 1 + n(MIB) + 33 * n(MIB) + BUILT);
    let text = "&;".repeat(MIB / 2);
    assert_eq!(lines(&html(&text))[0], text, "`&;` is no reference");
    let text = "&a".repeat(MIB / 2);
    assert_eq!(lines(&html(&text))[0], text);
}

#[test]
fn numeric_references_out_of_range_are_the_replacement_character_and_control_ones_are_gone() {
    let got = html(&"&#99999999999999999999;".repeat(40_000));
    assert_eq!(lines(&got)[0], "\u{FFFD}".repeat(40_000));
    let got = html(&"&#xD800;".repeat(100_000));
    assert_eq!(lines(&got)[0], "\u{FFFD}".repeat(100_000), "a surrogate is no character");
    let got = html(&format!("a{}b", "&#x110000;".repeat(50_000)));
    assert_eq!(lines(&got)[0].chars().count(), 50_002);
    let got = html(&"&#1;".repeat(200_000));
    assert!(got.label.is_empty(), "every one is a control character");
    let got = html(&format!("a{}b", "&#xFFFE;&#0;&#31;".repeat(50_000)));
    assert_eq!(lines(&got), ["ab"]);
    let padded = format!("&#x{}41;", "0".repeat(40));
    let got = html(&format!("a{}b", padded.repeat(20_000)));
    assert_eq!(lines(&got)[0].len(), 2 + 20_000 * padded.len(), "a number padded past the window is not looked for: it stays as written");
}

#[test]
fn text_made_of_what_a_decoder_leaves_behind_is_read() {
    // What a lossy decoding of bytes that are not UTF-8 gives, and what the clean takes out of a page.
    let lossy = String::from_utf8_lossy(&[0xFF; 3_000]).into_owned();
    assert_eq!(lines(&html(&lossy))[0], "\u{FFFD}".repeat(3_000));
    assert_eq!(lines(&plain(&lossy))[0], "\u{FFFD}".repeat(3_000));
    let noise: String = (0..100_000u32).map(|i| char::from_u32(i % 0x2400).unwrap_or('\u{FFFD}')).collect();
    for read in [html, plain] {
        let got = read(&noise);
        for run in got.label.paras.iter().flat_map(|p| p.runs.iter()) {
            assert!(run.text.chars().all(diacraft_mxfile::decode::keep_char));
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ attributes and style

#[test]
fn a_huge_attribute_is_charged_for_its_bytes_and_a_family_is_cut_to_a_name() {
    let face = "x".repeat(900_000);
    let text = format!("<font face=\"{face}\">t</font>");
    let got = html(&text);
    assert_eq!(got.label.paras[0].runs[0].fmt.family.as_deref().map(str::len), Some(128));
    assert_eq!(steps(&got), n(text.len()) + 3 + 900_000 + 1 + BUILT);
    // A style of 900,000 semicolons and one of 100,000 declarations are one walk each.
    let text = format!("<span style=\"{}\">t</span>", ";".repeat(900_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["t"]);
    assert_eq!(steps(&got), n(text.len()) + 3 + 2 * 900_000 + 1 + BUILT);
    let style = "color:red;".repeat(100_000);
    let got = html(&format!("<b style=\"{style}\">t</b>"));
    assert_eq!(got.label.paras[0].runs[0].fmt.color.map(|c| c.r), Some(255));
    // An attribute that is all references, and a link past its cap.
    let text = format!("<font face=\"{}\">t</font>", "&amp;".repeat(100_000));
    assert_eq!(lines(&html(&text)), ["t"]);
    let text = format!("<a href=\"https://x/{}\">go</a>", "a".repeat(40_000));
    let got = html(&text);
    assert_eq!((lines(&got), got.label.link), (vec!["go".to_owned()], None), "an address past 32 KiB is not kept");
}

#[test]
fn a_tag_with_a_hundred_thousand_attributes_keeps_the_first_sixteen() {
    let text = format!("<font {} style=\"color:red\" size=5>t</font>", "a ".repeat(100_000));
    let got = html(&text);
    assert_eq!(lines(&got), ["t"]);
    assert_eq!(got.label.paras[0].runs[0].fmt.color, None, "a style past the sixteenth is not read");
    let text = format!("<font size=5 {}>t</font>", "a=1 ".repeat(100_000));
    assert_eq!(html(&text).label.paras[0].runs[0].fmt.size_px, Some(24.0));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ breaks, lists, tables

#[test]
fn a_flood_of_line_breaks_is_one_paragraph_and_a_flood_of_items_is_a_paragraph_each() {
    let got = html(&"<br>".repeat(MIB / 4));
    assert_eq!(got.label.paras.len(), 1);
    assert_eq!(lines(&got)[0].chars().count(), MIB / 4 - 1, "the last break makes no line");
    assert!(lines(&got)[0].chars().all(|c| c == LINE_BREAK));
    assert_eq!(steps(&got), n(MIB) + n(MIB / 4) + BUILT);
    let got = html(&"a<br>".repeat(200_000));
    assert_eq!(got.label.paras.len(), 1);
    assert_eq!(got.label.paras[0].runs.len(), 1, "the runs are merged");
    let text = format!("<ol>{}", "<li>x".repeat(200_000));
    let got = html(&text);
    assert_eq!(got.label.paras.len(), 200_000, "each item ends the one before, with no end tag");
    assert_eq!(lines(&got)[0], "1. x");
    assert_eq!(lines(&got)[199_999], "200000. x");
    let text = format!("<table>{}", "<tr><td>a<td>b".repeat(60_000));
    let got = html(&text);
    assert_eq!(got.label.paras.len(), 60_000);
    assert_eq!(lines(&got)[59_999], "a\tb");
    let text = "<p>x".repeat(200_000);
    assert_eq!(html(&text).label.paras.len(), 200_000);
}

#[test]
fn a_flood_of_line_feeds_in_a_plain_label_is_one_paragraph_of_breaks() {
    let got = plain(&"\n".repeat(MIB));
    assert_eq!(got.label.paras.len(), 1);
    assert_eq!(lines(&got)[0].chars().count(), MIB - 1);
    let got = plain(&"\r\n".repeat(MIB / 2));
    assert_eq!(lines(&got)[0].chars().count(), MIB / 2 - 1);
    assert_eq!(steps(&got), n(MIB) + BUILT);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the work budget

#[test]
fn the_work_of_all_the_labels_of_a_file_is_one_budget() {
    // Each label of 1 MiB of text costs 2 MiB and 25 steps: its bytes, its token, the bytes of its text, and a paragraph and a run.
    let label = "a".repeat(MIB);
    let limits = Limits { max_read_work: 10 * n(MIB), ..Limits::default() };
    let mut reading = Reading::new(&limits);
    let mut read = 0;
    let err = loop {
        match read_html(&label, &Options::default(), &mut reading) {
            Ok(_) => read += 1,
            Err(e) => break e,
        }
        assert!(read < 100, "the budget never stops the reading");
    };
    assert_eq!(read, 4, "four labels fit in ten mebisteps");
    assert_eq!(err.class, "drawio_too_complex");
    assert!(reading.report.work.total() <= limits.max_read_work, "{}", reading.report.work.total());
    // A plain label is charged its bytes and what it builds: nine of them fit, and the tenth passes, by its 24 steps.
    let mut reading = Reading::new(&limits);
    let mut read = 0;
    while read_plain(&label, &Options::default(), &mut reading).is_ok() {
        read += 1;
        assert!(read < 100);
    }
    assert_eq!(read, 9);
}

#[test]
fn a_label_with_no_work_left_is_refused_whatever_it_is() {
    let limits = Limits { max_read_work: 0, ..Limits::default() };
    for text in ["a", "<b>", "&amp;", "x".repeat(1000).as_str()] {
        let mut reading = Reading::new(&limits);
        let err = read_html(text, &Options::default(), &mut reading).unwrap_err();
        assert_eq!(err.class, "drawio_too_complex", "{text}");
        assert_eq!(reading.report.work.total(), 0);
    }
    // And a label that is nothing costs nothing and is read.
    let mut reading = Reading::new(&limits);
    assert!(read_html("", &Options::default(), &mut reading).unwrap().paras.is_empty());
    assert!(read_plain("", &Options::default(), &mut reading).unwrap().paras.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ timed twins, by hand

/// The yardstick: labels as the editor writes them, a few thousand of them, read ten times. Gives the time per step the reader counts.
fn yardstick() -> f64 {
    let labels: Vec<String> = (0..3_000)
        .map(|i| {
            format!(
                "<div style=\"text-align: center\"><font color=\"#ff0000\" style=\"font-size: 14px\"><b>Item {i}</b></font><br>second &amp; <i>third</i> line</div><div><br></div><div>{}</div>",
                i * 7919 % 10_007
            )
        })
        .collect();
    let mut total_steps = 0u64;
    let start = Instant::now();
    for _ in 0..10 {
        let limits = Limits::default();
        let mut reading = Reading::new(&limits);
        for label in &labels {
            read_html(label, &Options::default(), &mut reading).unwrap();
        }
        total_steps += reading.report.work.label_steps;
    }
    start.elapsed().as_secs_f64() / total_steps as f64
}

/// A hostile label that makes `steps` steps is at most ten times as slow per step as the yardstick, which reads labels as they are.
fn within(case: &str, time: Duration, steps: u64, yard: f64) {
    let per = time.as_secs_f64() / steps as f64;
    eprintln!("{case}: {time:?} for {steps} steps, {:.2} times the yardstick's {:.1} ns", per / yard, yard * 1e9);
    assert!(per <= 10.0 * yard, "{case}: {:.2} times the yardstick per step", per / yard);
}

/// Reads `text` as an HTML label and holds it to the yardstick.
fn timed(case: &str, text: &str, yard: f64) {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let start = Instant::now();
    let _ = read_html(text, &Options::default(), &mut reading);
    within(case, start.elapsed(), reading.report.work.label_steps.max(1), yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_nesting_unclosed_tags_and_the_ends_that_close_nothing() {
    let yard = yardstick();
    timed("349,000 b tags deep", &format!("{}deep", "<b>".repeat(349_000)), yard);
    timed("140,000 b tags and their ends", &format!("{}x{}", "<b>".repeat(140_000), "</b>".repeat(140_000)), yard);
    timed("100,000 unclosed scripts", &"<script".repeat(100_000), yard);
    timed("100,000 scripts with a > and no end", &"<script>".repeat(100_000), yard);
    timed("50,000 scripts closed", &"<script>x</script>".repeat(50_000), yard);
    timed("100,000 nested svg", &"<svg>".repeat(100_000), yard);
    timed("90,000 svg pairs", &"<svg></svg>".repeat(90_000), yard);
    timed("64 open and 100,000 ends that close nothing", &format!("{}x{}", "<i>".repeat(64), "</b>".repeat(100_000)), yard);
    timed("unclosed comments and quotes", &"<b a=\"".repeat(MIB / 6 - 1), yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_references_attributes_breaks_lists_and_tables() {
    let yard = yardstick();
    timed("a million &amp;", &"&amp;".repeat(1_000_000), yard);
    timed("a mebibyte of stray &", &"&".repeat(MIB), yard);
    timed("40,000 numbers past the last character", &"&#99999999999999999999;".repeat(40_000), yard);
    timed("a font face of 900,000 bytes", &format!("<font face=\"{}\">t</font>", "x".repeat(900_000)), yard);
    timed("a style of 900,000 semicolons", &format!("<span style=\"{}\">t</span>", ";".repeat(900_000)), yard);
    timed("a style of 100,000 declarations", &format!("<b style=\"{}\">t</b>", "color:red;".repeat(100_000)), yard);
    timed("100,000 attributes", &format!("<font {} size=5>t</font>", "a ".repeat(100_000)), yard);
    timed("262,144 br", &"<br>".repeat(MIB / 4), yard);
    timed("200,000 items with no end", &format!("<ol>{}", "<li>x".repeat(200_000)), yard);
    timed("60,000 rows and cells with no end", &format!("<table>{}", "<tr><td>a<td>b".repeat(60_000)), yard);
    timed("200,000 p with no end", &"<p>x".repeat(200_000), yard);
}
