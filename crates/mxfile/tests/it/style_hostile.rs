//! Hostile style strings, counted: each loop of the style reader gets an input built in code that makes it do the most it can, and the work the read did is counted in
//! the units each loop walks (`style_bytes`, `style_pieces`), because a fuzzer finds crashes and misses hangs. Nothing here asserts a time: the
//! `#[ignore]`d twins at the end run the same inputs beside a yardstick, by hand, on a quiet machine (`cargo test -p diacraft-mxfile -- --ignored`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_mxfile::style::colour::MAX_COLOUR_BYTES;
use diacraft_mxfile::style::{CellKind, Key, Parent, Spec, Style, StyleCache, Value};
use diacraft_mxfile::{Limits, ReadWork, Reading};

use crate::limits;

const MIB: usize = 1 << 20;
const BAD: &str = "drawio_style_bad_value";
const IGNORED: &str = "drawio_style_keys_ignored";

/// The report's warnings as the tests compare them: by class, the count and the cases.
type Warnings = std::collections::BTreeMap<&'static str, (u64, Vec<String>)>;

/// What reading `text` once gave: the style, the work of reading it (the finish, which visits the names of the keys it ignored, is not in it), and the report it made.
fn read_one(text: &str, kind: CellKind) -> (Arc<Style>, ReadWork, Warnings) {
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let style = cache.resolve(Some(text), kind, &mut reading).unwrap();
    let work = reading.report.work;
    cache.finish(&mut reading).unwrap();
    let warnings = reading.report.warnings.iter().map(|(class, n)| (*class, (n.count, n.cases.clone()))).collect();
    (style, work, warnings)
}

#[test]
fn two_hundred_thousand_named_styles_cost_the_keys_they_add_and_no_more() {
    let n = 200_000usize;
    let text = "blue;".repeat(n);
    let (style, work, warnings) = read_one(&text, CellKind::Shape);
    let (once, ..) = read_one("blue", CellKind::Shape);
    assert_eq!(style.entries().collect::<Vec<_>>(), once.entries().collect::<Vec<_>>(), "two hundred thousand `blue` are one `blue`");
    // Nine defaults, a piece for each `blue` and the empty one after the last `;`, and the five keys each `blue` adds (its own three and `fancy`'s two).
    assert_eq!(work.style_pieces, (9 + n + 1 + 5 * n) as u64);
    assert_eq!(work.style_bytes, 2 * text.len() as u64);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn two_hundred_thousand_of_the_longest_named_style() {
    // `icon` extends `label`: 8 and 11 keys.
    let n = 200_000usize;
    let text = ";icon".repeat(n);
    let (_, work, _) = read_one(&text, CellKind::Shape);
    assert_eq!(
        work.style_pieces,
        (n + 1 + 19 * n) as u64,
        "a leading `;` makes an empty first piece; `;icon` ends no piece, so the last `icon` is not followed by one"
    );
    // The same, one after the other with a trailing semicolon, on a connector.
    let text = "icon;".repeat(n);
    let (_, work, _) = read_one(&text, CellKind::Connector);
    assert_eq!(work.style_pieces, (10 + n + 1 + 19 * n) as u64);
}

#[test]
fn a_style_of_32_mib_of_semicolons_is_one_pass_of_empty_pieces() {
    let text = ";".repeat(32 * MIB);
    let (style, work, warnings) = read_one(&text, CellKind::Shape);
    assert_eq!(style.entries().count(), 0);
    assert_eq!(work.style_pieces, 32 * MIB as u64 + 1);
    assert_eq!(work.style_bytes, 64 * MIB as u64);
    assert!(warnings.is_empty());
}

#[test]
fn a_style_of_32_mib_of_one_pair_over_and_over_is_applied_to_one_slot() {
    let pair = "fillColor=#000000;";
    let text = pair.repeat(32 * MIB / pair.len());
    let pieces = (text.len() / pair.len() + 1) as u64;
    let (style, work, _) = read_one(&text, CellKind::Shape);
    assert_eq!(style.get(Key::FillColor), Some(Value::Colour(Spec::Colour(diacraft_mxfile::style::Colour::BLACK))));
    assert_eq!(work.style_pieces, 9 + pieces);
    assert_eq!(work.style_bytes, 2 * text.len() as u64);
}

#[test]
fn a_style_of_32_mib_of_one_unknown_key_is_counted_as_one_name() {
    let pair = "zzz=1;";
    let text = pair.repeat(32 * MIB / pair.len());
    let uses = (text.len() / pair.len()) as u64;
    let (_, work, warnings) = read_one(&text, CellKind::Shape);
    assert_eq!(warnings.get(IGNORED), Some(&(uses, vec![format!("zzz {uses}")])));
    assert_eq!(work.style_pieces, 9 + uses + 1);
}

#[test]
fn a_million_distinct_unknown_keys_are_counted_and_only_a_few_names_are_kept() {
    let text: String = (0..1_000_000).map(|i| format!("k{i}=1;")).collect();
    let (_, work, warnings) = read_one(&text, CellKind::Shape);
    let (count, cases) = warnings.get(IGNORED).unwrap();
    assert_eq!(*count, 1_000_000, "every use is counted, those whose names were not kept in one count");
    assert_eq!(cases.len(), 20);
    assert!(cases.iter().all(|c| c.ends_with(" 1")), "{cases:?}");
    assert_eq!(work.style_pieces, 9 + 1_000_000 + 1);
}

#[test]
fn a_million_cells_of_one_style_parse_it_once_and_count_its_unknown_keys_for_each() {
    let text = "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;aspect=fixed;";
    let cells = 1_000_000u64;
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let first = cache.resolve(Some(text), CellKind::Shape, &mut reading).unwrap();
    for _ in 1..cells {
        let again = cache.resolve(Some(text), CellKind::Shape, &mut reading).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    }
    let work = reading.report.work;
    // The pieces parsed are the style's, not a million times it: nine defaults, six pieces and the empty one after the last `;`.
    assert_eq!(work.style_pieces, 9 + 6 + 1);
    // The bytes hashed are the style's for each cell, and walked once.
    assert_eq!(work.style_bytes, (cells + 1) * text.len() as u64);
    cache.finish(&mut reading).unwrap();
    assert_eq!(reading.report.work.style_pieces, 9 + 6 + 1 + 1, "the finish visits the one name the style kept, once, and not once for each cell");
    let noted = reading.report.warnings.get(IGNORED).unwrap();
    assert_eq!((noted.count, noted.cases.clone()), (cells, vec![format!("aspect {cells}")]), "the unknown key, once for each cell");
}

#[test]
fn cells_that_each_have_a_style_of_their_own_are_each_a_parse() {
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let mut pieces = 0u64;
    for i in 0..200_000u32 {
        let text = format!("fillColor=#{i:06x};rounded=1");
        pieces += 9 + 2;
        cache.resolve(Some(&text), CellKind::Shape, &mut reading).unwrap();
    }
    assert_eq!(cache.len(), 200_000);
    assert_eq!(reading.report.work.style_pieces, pieces);
}

#[test]
fn numbers_that_are_nan_infinite_or_huge_are_bad_values_and_never_reach_the_style() {
    let nasty = [
        "NaN",
        "nan",
        "Infinity",
        "-Infinity",
        "inf",
        "1e999",
        "-1e999",
        "1e308",
        "-1e308",
        "1e400",
        "99999999999999999999",
        "10000001",
        "-10000001",
        "1e8",
        "0x7fffffff",
    ];
    let keys = [
        "strokeWidth",
        "fontSize",
        "rotation",
        "spacing",
        "spacingTop",
        "opacity",
        "arcSize",
        "size",
        "exitX",
        "entryDy",
        "imageWidth",
        "startSize",
        "fontStyle",
        "margin",
    ];
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    for text in nasty {
        let style_text: String = keys.iter().map(|k| format!("{k}={text};")).collect();
        let style = cache.resolve(Some(&style_text), CellKind::Shape, &mut reading).unwrap();
        for (key, value) in style.entries() {
            if let Value::Num(n) = value {
                assert!(n.is_finite() && n.abs() <= 10_000_000.0, "{key}={n} from {text}");
            }
        }
        for key in keys {
            let key = Key::from_name(key).unwrap();
            assert!(!style.is_set(key), "{key}={text} is a bad value");
        }
        assert_eq!(style.stroke_width(), 1.0);
        assert_eq!(style.font_size(), 11.0);
        assert!(style.margins().top.is_finite());
        assert!(style.corner(diacraft_mxfile::style::Corners::Box).is_none());
        assert!(!style.font_style().bold);
    }
    let noted = reading.report.warnings.get(BAD).unwrap();
    assert_eq!(noted.count, keys.len() as u64, "one warning for each key name, over fifteen bad values of each");
    // The numbers at the very edge of the range are numbers.
    let edge = cache.resolve(Some(";rotation=10000000;spacing=-10000000;opacity=1e7"), CellKind::Shape, &mut reading).unwrap();
    assert_eq!((edge.rotation(), edge.num(Key::Spacing)), (10_000_000.0, Some(-10_000_000.0)));
    assert_eq!(edge.opacity(), 1.0, "held to 100");
    assert!(edge.margins().top.is_finite());
}

#[test]
fn degenerate_colours_cost_their_bytes_and_are_bad_values() {
    let mib = MIB;
    for text in [
        "light-dark(".repeat(mib / 11),
        format!("light-dark({}", "(".repeat(mib)),
        format!("rgb({}", " ".repeat(mib)),
        format!("#{}", "f".repeat(mib)),
        format!("hsl(1e999, 100%, 50%){}", ")".repeat(mib)),
        "rgb(".repeat(mib / 4),
        format!("light-dark({},{})", "light-dark(red,blue),".repeat(mib / 20), "x"),
        ",".repeat(mib),
        "(".repeat(mib),
    ] {
        let style_text = format!(";fillColor={text};strokeColor={text}");
        let (style, work, warnings) = read_one(&style_text, CellKind::Shape);
        let p = style.paints(Parent::Layer);
        assert_eq!(
            (p.fill, p.stroke),
            (Some(diacraft_mxfile::style::Colour::WHITE), Some(diacraft_mxfile::style::Colour::BLACK)),
            "a bad colour is `default`"
        );
        assert_eq!(warnings.get(BAD).map(|w| w.0), Some(2));
        assert_eq!(work.style_pieces, 3);
        assert_eq!(work.style_bytes, 2 * style_text.len() as u64, "walked once, whatever the colour looks like");
    }
}

#[test]
fn a_colour_is_read_up_to_its_cap_and_not_past_it() {
    let pad = |n: usize| format!("rgb(255,0,0{})", " ".repeat(n));
    let fits = pad(MAX_COLOUR_BYTES - "rgb(255,0,0)".len());
    assert_eq!(fits.len(), MAX_COLOUR_BYTES);
    let one_more = pad(MAX_COLOUR_BYTES + 1 - "rgb(255,0,0)".len());
    for (text, bad) in [(fits, false), (one_more, true)] {
        let (style, _, warnings) = read_one(&format!(";fillColor={text}"), CellKind::Shape);
        let want = if bad { Spec::Default } else { Spec::Colour(diacraft_mxfile::style::Colour::rgb(255, 0, 0)) };
        assert_eq!(style.get(Key::FillColor), Some(Value::Colour(want)), "{}", text.len());
        assert_eq!(warnings.contains_key(BAD), bad);
    }
}

#[test]
fn long_runs_of_semicolons_and_equals_signs_between_pairs() {
    let text = format!("a=1;{}b=2;{}fillColor=#102030", ";".repeat(MIB), ";".repeat(MIB));
    let (style, work, _) = read_one(&text, CellKind::Shape);
    assert_eq!(style.get(Key::FillColor), Some(Value::Colour(Spec::Colour(diacraft_mxfile::style::Colour::rgb(0x10, 0x20, 0x30)))));
    assert_eq!(work.style_pieces, 9 + 2 * MIB as u64 + 3, "nine defaults, and a piece for each `;` (2 MiB and two) and the last");
    // A million pieces that are only `=`: a key with no name, ignored, counted as one name.
    let text = "=;".repeat(MIB);
    let (_, work, warnings) = read_one(&text, CellKind::Shape);
    assert_eq!(warnings.get(IGNORED).map(|w| w.0), Some(MIB as u64));
    assert_eq!(work.style_pieces, 9 + MIB as u64 + 1);
    // Equals signs inside a value are the value's.
    let text = format!("fontFamily={}", "=".repeat(MIB));
    let (style, ..) = read_one(&text, CellKind::Shape);
    assert_eq!(style.text(Key::FontFamily).map(str::len), Some(MIB));
}

#[test]
fn a_value_of_32_mib_is_a_place_in_the_style_and_is_not_copied_out_of_it() {
    let value = "A".repeat(32 * MIB - "image=".len());
    let text = format!("image={value}");
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let style = cache.resolve(Some(&text), CellKind::Shape, &mut reading).unwrap();
    assert_eq!(style.text(Key::Image).map(str::len), Some(value.len()));
    assert_eq!(reading.report.work.style_pieces, 9 + 1);
}

#[test]
fn a_style_that_runs_out_of_budget_part_way_is_refused_in_words_and_not_kept() {
    // 250,000 `blue`: 1.25 MB hashed and walked, which the budget holds; the pieces and the five keys of each, 1.5 million, are what it does not.
    let limits = Limits { max_read_work: 3_000_000, ..Limits::default() };
    let mut reading = Reading::new(&limits);
    let mut cache = StyleCache::new();
    let text = "blue;".repeat(250_000);
    let refused = cache.resolve(Some(&text), CellKind::Shape, &mut reading).unwrap_err();
    assert_eq!(refused.class, "drawio_too_complex");
    assert!(refused.words.contains("more than 3000000 steps"), "{}", refused.words);
    let work = reading.report.work;
    assert_eq!(work.style_bytes, 2 * text.len() as u64, "both walks of the text were charged first");
    assert!(work.style_pieces > 0 && work.total() <= 3_000_000, "the pieces were charged as they were reached, up to the budget: {work:?}");
    assert!(cache.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ timed, by hand

/// Seconds for one unit of work reading real-looking styles: 200,000 distinct ones, each of about 100 bytes.
fn yardstick() -> f64 {
    let texts: Vec<String> = (0..200_000)
        .map(|i| {
            format!(
                "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;fontSize={};align=left;spacingLeft={};shape=process;",
                8 + i % 20,
                i
            )
        })
        .collect();
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let start = Instant::now();
    for text in &texts {
        cache.resolve(Some(text), CellKind::Shape, &mut reading).unwrap();
    }
    let time = start.elapsed().as_secs_f64();
    time / reading.report.work.total() as f64
}

/// A hostile input that makes `units` of work is at most ten times as slow per unit as the yardstick.
fn within(case: &str, time: Duration, units: u64, yard: f64) {
    let per = time.as_secs_f64() / units as f64;
    eprintln!("{case}: {time:?} for {units} units, {:.2} times the yardstick's {:.1} ns", per / yard, yard * 1e9);
    assert!(per <= 10.0 * yard, "{case}: {:.2} times the yardstick per unit", per / yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_a_style_of_32_mib_of_semicolons_pairs_and_unknown_keys() {
    let yard = yardstick();
    for (case, text) in [
        ("32 MiB of semicolons", ";".repeat(32 * MIB)),
        ("32 MiB of one pair", "fillColor=#000000;".repeat(32 * MIB / 18)),
        ("32 MiB of one unknown key", "zzz=1;".repeat(32 * MIB / 6)),
        ("a million distinct unknown keys", (0..1_000_000).map(|i| format!("k{i}=1;")).collect()),
    ] {
        let start = Instant::now();
        let (_, work, _) = read_one(&text, CellKind::Shape);
        within(case, start.elapsed(), work.total(), yard);
    }
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_named_styles_a_million_cells_and_degenerate_colours() {
    let yard = yardstick();
    let text = "icon;".repeat(200_000);
    let start = Instant::now();
    let (_, work, _) = read_one(&text, CellKind::Shape);
    within("200,000 icons", start.elapsed(), work.total(), yard);
    let text = "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;aspect=fixed;";
    let mut reading = Reading::new(limits());
    let mut cache = StyleCache::new();
    let start = Instant::now();
    for _ in 0..1_000_000 {
        cache.resolve(Some(text), CellKind::Shape, &mut reading).unwrap();
    }
    cache.finish(&mut reading).unwrap();
    within("a million cells of one style", start.elapsed(), reading.report.work.total(), yard);
    for (case, colour) in
        [("a megabyte of light-dark(", "light-dark(".repeat(MIB / 11)), ("a megabyte of spaces", format!("rgb({}", " ".repeat(MIB)))]
    {
        let text = format!(";fillColor={colour};strokeColor={colour}");
        let start = Instant::now();
        let (_, work, _) = read_one(&text, CellKind::Shape);
        within(case, start.elapsed(), work.total(), yard);
    }
}
