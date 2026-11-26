//! The input and the properties of the fuzz target `mxfile_style`: bytes become a style string, a colour value, a number or a list of styles, and the style reader is
//! held to its contract on them. The target and the reader's own tests include this file by its path (`fuzz_targets/mxfile_style.rs`, `crates/mxfile/tests/it/style_props.rs`), so
//! that a seed corpus and random inputs are held to the same law the fuzzer holds the bytes to, and the inputs the target crashed on are replayed through the same code.
//!
//! **The input.** The first byte, modulo 8, picks what the rest is: 0, a style string read for a shape; 4, the same read for a connector; 1, a colour value (which is then also the
//! colour of a fill and a line); 2, a number (which is then also a width, a flag and a size); any other, the styles of many cells, one for each line, read through one cache, each
//! as a shape or a connector in turn.
//!
//! **The properties.** The reader never panics and never hangs; the work it counts never passes the budget it was given, whether it reads or refuses; what it gives is the same
//! the second time; a number it holds is finite and within the read's range; every value is what its key's type says; a colour that is read writes itself out and reads back as
//! itself; a style it has parsed once is not parsed again; and what the report says of the keys it ignored and of the bad values stays within its caps.

use diacraft_mxfile::style::colour::{self, MAX_COLOUR_BYTES};
use diacraft_mxfile::style::keys::{self, MAX_NUMBER_BYTES};
use diacraft_mxfile::style::{CellKind, Key, Paints, Parent, Spec, Style, StyleCache, TOP_IGNORED, Ty, Value};
use diacraft_mxfile::{Limits, Reading};

/// The text a byte stream is read as: UTF-8 with what is not made into replacement characters.
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// What a style gives, as a line, so that two reads of one text can be compared.
fn line_of(style: &Style) -> String {
    style.entries().map(|(key, value)| format!("{key}={value:?}")).collect::<Vec<_>>().join(";")
}

/// The contract of one style: every value is what its key's type says, and every typed read is within its range.
fn hold(style: &Style, limits: &Limits) {
    let mut last: Option<Key> = None;
    for (key, value) in style.entries() {
        assert!(last.is_none_or(|l| l < key), "the keys are not in order");
        last = Some(key);
        match (key.ty(), value) {
            (Ty::Num(_), Value::Num(n)) => assert!(n.is_finite() && n.abs() <= limits.max_coord_px, "{key}={n}"),
            (Ty::Flag(_), Value::Flag(_)) => {}
            (Ty::Colour, Value::Colour(_)) => {}
            (Ty::Text, Value::Text(t)) => assert!(!t.is_empty(), "{key} is empty"),
            (Ty::OneOf(words), Value::Text(t)) => assert!(words.contains(&t), "{key}={t}"),
            (ty, value) => panic!("{key} is {ty:?} and holds {value:?}"),
        }
    }
    let size = style.font_size();
    assert!((1.0..=500.0).contains(&size), "font size {size}");
    for share in [style.opacity(), style.fill_opacity(), style.stroke_opacity(), style.text_opacity()] {
        assert!((0.0..=1.0).contains(&share), "opacity {share}");
    }
    let m = style.margins();
    assert!([m.top, m.right, m.bottom, m.left].iter().all(|v| v.is_finite()), "margins {m:?}");
    for corners in [diacraft_mxfile::style::Corners::Box, diacraft_mxfile::style::Corners::Polygon] {
        match style.corner(corners) {
            Some(diacraft_mxfile::style::Radius::Percent(p)) => assert!((0.0..=50.0).contains(&p), "{p}"),
            Some(diacraft_mxfile::style::Radius::Px(p)) => assert!(p.is_finite() && p >= 0.0, "{p}"),
            None => {}
        }
    }
    assert!(style.bend_radius().is_none_or(|r| r.is_finite() && r >= 0.0));
    assert!(style.dash_pattern().is_none_or(|d| !d.is_empty() && d.len() <= 16 && d.iter().all(|n| n.is_finite() && *n >= 0.0)));
    assert!(style.rotation().is_finite() && style.stroke_width().is_finite() && style.header_size().is_finite());
    let layer = style.paints(Parent::Layer);
    let again = style.paints(Parent::Layer);
    assert_eq!(layer, again, "the same style, two sets of colours");
    let on_cell = style.paints(Parent::Cell(&layer));
    let on_nothing = style.paints(Parent::Cell(&Paints::default()));
    // A cell whose own colours are plain colours, `default` or nothing names no parent's colour, so no parent changes them.
    let plain = |key: Key| matches!(style.get(key), None | Some(Value::Colour(Spec::Colour(_) | Spec::Default)));
    if [Key::FillColor, Key::StrokeColor, Key::FontColor, Key::GradientColor, Key::LabelBackgroundColor, Key::SwimlaneFillColor]
        .into_iter()
        .all(plain)
    {
        assert!(layer == on_cell && on_cell == on_nothing, "the parent changed the colours of a style that names none");
    }
}

/// A style string, read as a shape or a connector: the contract of one read and of reading it again.
fn style_text(text: &str, kind: CellKind) {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let mut cache = StyleCache::new();
    let first = cache.resolve(Some(text), kind, &mut reading);
    let work = reading.report.work;
    assert!(work.total() <= limits.max_read_work, "the work {} is past the budget", work.total());
    let Ok(first) = first else { return };
    hold(&first, &limits);
    assert_eq!(work.style_bytes, 2 * text.len() as u64, "the text is hashed once and walked once");
    // Once for the cell and once again: the same style, and no piece walked twice.
    let second = cache.resolve(Some(text), kind, &mut reading).expect("the second read of a style is a lookup");
    assert!(std::sync::Arc::ptr_eq(&first, &second), "a style parsed twice");
    assert_eq!(reading.report.work.style_pieces, work.style_pieces, "a lookup walked pieces");
    assert_eq!(reading.report.work.style_bytes, work.style_bytes + text.len() as u64, "a lookup costs the hash of the text");
    cache.finish(&mut reading).expect("finishing a read that has not spent its budget");
    // A fresh read of the same text is the same style, the same work and the same words.
    let mut other = Reading::new(&limits);
    let mut fresh = StyleCache::new();
    let again = fresh.resolve(Some(text), kind, &mut other).expect("the same read");
    assert_eq!(line_of(&first), line_of(&again), "the same text, two styles");
    assert_eq!(*first, *again);
    assert_eq!(work, other.report.work, "the same text, two counts");
    // The cell the first cache gave the style to twice is the second cell here too, so that what the finish counts is the same.
    fresh.resolve(Some(text), kind, &mut other).expect("the same lookup");
    fresh.finish(&mut other).expect("the same finish");
    for (class, noted) in &reading.report.warnings {
        assert!(noted.cases.len() <= limits.max_cases, "{class}");
        match *class {
            "drawio_style_bad_value" => assert!(noted.cases.iter().all(|c| Key::from_name(c).is_some()), "{noted:?}"),
            "drawio_style_keys_ignored" => {
                assert!(noted.cases.len() <= TOP_IGNORED && noted.count > 0, "{noted:?}");
                assert!(noted.cases.iter().all(|c| c.chars().count() <= 130 && !c.chars().any(char::is_control)), "{noted:?}");
            }
            other => panic!("a warning of the style reader's that is not one of its two: {other}"),
        }
    }
    assert_eq!(reading.report.warnings, other.report.warnings, "the same text, two reports");
}

/// A colour value: no panic, a colour of its own writes itself out and reads back as itself, and nothing past the cap is a colour.
fn colour_text(text: &str) {
    let spec = colour::parse(text);
    if text.len() > MAX_COLOUR_BYTES {
        assert_eq!(spec, None, "a colour past the cap");
    }
    if let Some(Spec::Colour(c)) = spec {
        assert_eq!(colour::parse(&c.to_hex()), Some(Spec::Colour(c)), "{c:?} did not come back");
    }
    assert_eq!(spec, colour::parse(text), "the same colour, two answers");
}

/// A number: no panic, and it is the number the standard library reads from the same text, when it reads one.
fn number_text(text: &str) {
    let n = keys::parse_decimal(text);
    if let Some(n) = n {
        assert!(n.is_finite(), "{text:?} is {n}");
        let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        assert!(trimmed.len() <= MAX_NUMBER_BYTES);
        let std: f64 = trimmed.parse().expect("a decimal that the standard library does not read");
        assert!(n == std, "{text:?}: {n} against {std}");
        assert!(keys::parse_number(text, n.abs()).is_some() && keys::parse_number(text, n.abs() / 2.0).is_none() == (n != 0.0));
        assert_eq!(keys::parse_flag(text), n != 0.0);
    } else {
        assert!(keys::parse_number(text, f64::MAX).is_none());
    }
}

/// Many styles, one for each line, through one cache: the counts of work add up and no style is parsed twice.
fn many(text: &str) {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let mut cache = StyleCache::new();
    let mut seen = std::collections::HashSet::new();
    for (i, line) in text.lines().take(2_000).enumerate() {
        let kind = if i % 2 == 0 { CellKind::Shape } else { CellKind::Connector };
        let Ok(style) = cache.resolve(Some(line), kind, &mut reading) else { return };
        hold(&style, &limits);
        seen.insert((i % 2, line.to_owned()));
        assert_eq!(cache.len(), seen.len(), "a text parsed twice, or a text found that was never parsed");
    }
    assert!(reading.report.work.total() <= limits.max_read_work);
    let _ = cache.finish(&mut reading);
}

/// The contract of the style reader on `data`; panics when it breaks it.
pub fn check(data: &[u8]) {
    let (mode, body) = data.split_first().map_or((0, &[][..]), |(m, b)| (*m, b));
    let text = text_of(body);
    match mode % 8 {
        0 | 4 => style_text(&text, if mode % 8 == 0 { CellKind::Shape } else { CellKind::Connector }),
        1 => {
            colour_text(&text);
            // The same text as a style's colour, so that the keywords and the bad values go through the cache too.
            style_text(&format!(";fillColor={};strokeColor={}", text.replace(';', ","), text.replace(';', ",")), CellKind::Shape);
        }
        2 => {
            number_text(&text);
            style_text(
                &format!("strokeWidth={};rounded={};fontSize={}", text.replace(';', ","), text.replace(';', ","), text.replace(';', ",")),
                CellKind::Connector,
            );
        }
        _ => many(&text),
    }
}
