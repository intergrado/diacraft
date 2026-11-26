//! Properties of the style reader, and the law of the fuzz target `mxfile_style` held to a seed corpus and to random bytes by the reader's own tests, so that every
//! input the target ever crashed on is replayed here through the very same code (add it to `CRASHES`, with the commit that fixed it).
//!
//! The main property is the one the plan names: **a style resolved in one pass equals its pieces applied by hand** (S-R1). The hand is a model in this file, written from the
//! specification: a map of keys to values, started from the stylesheet's defaults (unless the string starts with `;`), each piece applied left to right, a named style's pairs
//! applied where it stands, a `none` taking its key away, and a value read as its key's type reads it.

use std::collections::BTreeMap;

use diacraft_mxfile::style::grammar::pieces;
use diacraft_mxfile::style::{CellKind, Spec, Style, StyleCache, Value, colour, keys};
use diacraft_mxfile::{Limits, Reading};
use proptest::prelude::*;

#[path = "../../fuzz/src/style.rs"]
mod law;

/// Inputs the target crashed on, each with what it was.
const CRASHES: &[(&str, &[u8])] = &[];

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the model

/// How a key reads its value, in the model: the few keys the generator uses.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Num,
    Flag,
    Colour,
    Word(&'static [&'static str]),
    Text,
}

const KEYS: &[(&str, Kind)] = &[
    ("strokeWidth", Kind::Num),
    ("fontSize", Kind::Num),
    ("spacing", Kind::Num),
    ("spacingTop", Kind::Num),
    ("rotation", Kind::Num),
    ("rounded", Kind::Flag),
    ("dashed", Kind::Flag),
    ("endFill", Kind::Flag),
    ("shadow", Kind::Flag),
    ("glass", Kind::Flag),
    ("fillColor", Kind::Colour),
    ("strokeColor", Kind::Colour),
    ("fontColor", Kind::Colour),
    ("gradientColor", Kind::Colour),
    ("labelBackgroundColor", Kind::Colour),
    ("align", Kind::Word(&["left", "center", "right"])),
    ("verticalAlign", Kind::Word(&["top", "middle", "bottom"])),
    ("shape", Kind::Text),
    ("image", Kind::Text),
    ("fontFamily", Kind::Text),
    ("perimeter", Kind::Text),
    ("endArrow", Kind::Text),
];

fn kind_of(key: &str) -> Option<Kind> {
    KEYS.iter().find(|(k, _)| *k == key).map(|(_, kind)| *kind)
}

/// The stylesheet's defaults (S-R7), as pairs.
fn defaults(kind: CellKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        CellKind::Shape => &[
            ("shape", "label"),
            ("perimeter", "rectanglePerimeter"),
            ("fontSize", "12"),
            ("fontFamily", "Helvetica"),
            ("align", "center"),
            ("verticalAlign", "middle"),
            ("fillColor", "default"),
            ("strokeColor", "default"),
            ("fontColor", "default"),
        ],
        CellKind::Connector => &[
            ("shape", "connector"),
            ("labelBackgroundColor", "default"),
            ("endArrow", "classic"),
            ("fontSize", "11"),
            ("fontFamily", "Helvetica"),
            ("align", "center"),
            ("verticalAlign", "middle"),
            ("rounded", "1"),
            ("strokeColor", "default"),
            ("fontColor", "default"),
        ],
    }
}

/// The named styles the generator uses, flattened, written from the table of S-R8.
fn named(name: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let blue = [("fillColor", "#DAE8FC"), ("gradientColor", "#7EA6E0"), ("strokeColor", "#6C8EBF")];
    Some(match name {
        "text" => vec![("fillColor", "none"), ("gradientColor", "none"), ("strokeColor", "none"), ("align", "left"), ("verticalAlign", "top")],
        "group" => vec![("verticalAlign", "top"), ("fillColor", "none"), ("strokeColor", "none"), ("gradientColor", "none")],
        "fancy" => vec![("shadow", "1"), ("glass", "1")],
        "blue" => [("shadow", "1"), ("glass", "1")].into_iter().chain(blue).collect(),
        "plain-blue" => blue.to_vec(),
        "line" => vec![("shape", "line"), ("strokeWidth", "4"), ("labelBackgroundColor", "default"), ("verticalAlign", "top"), ("spacingTop", "8")],
        _ => return None,
    })
}

/// What a value of a key says, as the line `render` writes; `None` takes the key away.
fn model_value(kind: Kind, value: &str) -> Option<String> {
    if value == "none" {
        return None;
    }
    match kind {
        // The values the generator uses are plain numbers: the standard library reads what the spec's number grammar reads.
        Kind::Num => value.parse::<f64>().ok().map(|n| format!("{n}")),
        Kind::Flag => Some((if matches!(value, "true") || value.parse::<f64>().is_ok_and(|n| n != 0.0) { "1" } else { "0" }).to_owned()),
        Kind::Colour => match value {
            "" => None,
            "default" => Some("default".to_owned()),
            "inherit" => Some("Inherit".to_owned()),
            "fillColor" => Some("Fill".to_owned()),
            "strokeColor" => Some("Stroke".to_owned()),
            "parentFillColor" => Some("ParentFill".to_owned()),
            "parentStrokeColor" => Some("ParentStroke".to_owned()),
            v if v.starts_with('#') && v.len() == 4 => Some(format!("#{}", v[1..].chars().flat_map(|c| [c, c]).collect::<String>().to_uppercase())),
            v if v.starts_with('#') => Some(v.to_uppercase()),
            _ => Some("default".to_owned()), // a bad colour is the key's default
        },
        Kind::Word(words) => (words.contains(&value)).then(|| value.to_owned()),
        Kind::Text => (!value.is_empty()).then(|| value.to_owned()),
    }
}

/// The style of `text` by hand: the defaults of `kind` unless `text` starts with `;`, then each piece in order.
fn by_hand(text: &str, kind: CellKind) -> String {
    let mut state: BTreeMap<String, String> = BTreeMap::new();
    let apply = |state: &mut BTreeMap<String, String>, key: &str, value: &str| {
        let Some(k) = kind_of(key) else { return };
        match model_value(k, value) {
            Some(v) => state.insert(key.to_owned(), v),
            None => state.remove(key),
        };
    };
    if !text.starts_with(';') {
        for (key, value) in defaults(kind) {
            apply(&mut state, key, value);
        }
    }
    for piece in text.split(';').filter(|p| !p.is_empty()) {
        match piece.split_once('=') {
            Some((key, value)) => apply(&mut state, key, value),
            None => {
                for (key, value) in named(piece).unwrap_or_default() {
                    apply(&mut state, key, value);
                }
            }
        }
    }
    let mut lines: Vec<String> = state.iter().map(|(k, v)| format!("{k}={v}")).collect();
    lines.sort();
    lines.join(";")
}

/// What the reader gave for `text`, as the same lines.
fn by_reader(style: &Style) -> String {
    let mut lines: Vec<String> = style
        .entries()
        .map(|(key, value)| {
            let shown = match value {
                Value::Num(n) => format!("{n}"),
                Value::Flag(f) => (if f { "1" } else { "0" }).to_owned(),
                Value::Text(t) => t.to_owned(),
                Value::Colour(Spec::Colour(c)) => c.to_hex(),
                Value::Colour(Spec::Default) => "default".to_owned(),
                Value::Colour(other) => format!("{other:?}"),
            };
            format!("{}={shown}", key.name())
        })
        .collect();
    lines.sort();
    lines.join(";")
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the generators

fn value_of(kind: Kind) -> BoxedStrategy<String> {
    let pick = |values: &[&str]| prop::sample::select(values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>()).boxed();
    match kind {
        Kind::Num => pick(&["0", "1", "12", "-3", "1e2", "2.5", "abc", "0x2", "none", ""]),
        Kind::Flag => pick(&["0", "1", "2", "true", "false", "yes", "none", ""]),
        Kind::Colour => pick(&[
            "#ff0000",
            "#00FF00",
            "#abc",
            "default",
            "inherit",
            "fillColor",
            "strokeColor",
            "parentFillColor",
            "parentStrokeColor",
            "banana",
            "none",
            "",
        ]),
        Kind::Word(_) => pick(&["left", "center", "right", "top", "middle", "bottom", "justify", "none", ""]),
        Kind::Text => pick(&["label", "ellipse", "data:a=b", "Arial, sans", "none", ""]),
    }
}

fn piece() -> BoxedStrategy<String> {
    let pairs = prop::sample::select(KEYS.to_vec()).prop_flat_map(|(key, kind)| value_of(kind).prop_map(move |v| format!("{key}={v}")));
    prop_oneof![
        6 => pairs,
        2 => prop::sample::select(vec!["text", "group", "fancy", "blue", "plain-blue", "line", "nosuch", "rounded", "html", "Text"]).prop_map(str::to_owned),
        1 => prop::sample::select(vec!["foo=1", "Rounded=1", "x=y=z", "=", "fillColor =#fff"]).prop_map(str::to_owned),
        1 => Just(String::new()),
    ]
    .boxed()
}

fn style_text() -> impl Strategy<Value = String> {
    (prop::collection::vec(piece(), 0..24), any::<bool>()).prop_map(|(pieces, lead)| format!("{}{}", if lead { ";" } else { "" }, pieces.join(";")))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]

    /// A style resolved in one pass is its pieces applied by hand (S-R1, S-R2, S-R4, S-R5, S-R17), for a shape and for a connector.
    #[test]
    fn a_style_resolved_in_one_pass_equals_the_pieces_applied_by_hand(text in style_text(), connector in any::<bool>()) {
        let kind = if connector { CellKind::Connector } else { CellKind::Shape };
        let limits = Limits::default();
        let mut reading = Reading::new(&limits);
        let mut cache = StyleCache::new();
        let style = cache.resolve(Some(&text), kind, &mut reading).unwrap();
        prop_assert_eq!(by_reader(&style), by_hand(&text, kind), "{:?}", text);
    }

    /// The pieces of a style, joined with `;`, are the style; each piece is where it says it is.
    #[test]
    fn the_pieces_of_a_string_are_the_string_cut_at_its_semicolons(text in "[a-z=; é]{0,200}") {
        let all: Vec<_> = pieces(&text).collect();
        prop_assert_eq!(all.len(), text.matches(';').count() + 1);
        prop_assert_eq!(all.iter().map(|p| p.text).collect::<Vec<_>>().join(";"), text.clone());
        for p in &all {
            prop_assert_eq!(text.get(p.offset..p.offset + p.text.len()), Some(p.text));
        }
    }

    /// The number grammar of S-R5: what it accepts the standard library reads as the same number, and a text made of a number's parts is accepted.
    #[test]
    fn a_decimal_is_read_as_the_standard_library_reads_it(sign in "[+-]?", whole in "[0-9]{0,6}", frac in prop::option::of("[0-9]{0,6}"), exp in prop::option::of(("[+-]?", "[0-9]{1,3}"))) {
        let mut text = format!("{sign}{whole}");
        if let Some(f) = &frac { text.push('.'); text.push_str(f); }
        if let Some((s, d)) = &exp { text.push('e'); text.push_str(s); text.push_str(d); }
        let has_digits = !whole.is_empty() || frac.as_deref().is_some_and(|f| !f.is_empty());
        let got = keys::parse_decimal(&text);
        if has_digits {
            let want: f64 = text.parse().unwrap();
            prop_assert_eq!(got, want.is_finite().then_some(want).map(|n| if n == 0.0 { 0.0 } else { n }), "{:?}", text);
        } else {
            prop_assert_eq!(got, None, "{:?}", text);
        }
    }

    /// Any text: a number it reads is the standard library's, a flag is a number but 0 or `true`, and nothing panics.
    #[test]
    fn any_text_as_a_number_or_a_flag(text in any::<String>()) {
        law::check(&[&[2u8][..], text.as_bytes()].concat());
    }

    /// Every colour writes itself out and reads back as itself, and its parts are the bytes.
    #[test]
    fn a_colour_reads_back_as_itself(r in any::<u8>(), g in any::<u8>(), b in any::<u8>(), a in any::<u8>()) {
        let c = colour::Colour { r, g, b, a };
        prop_assert_eq!(colour::parse(&c.to_hex()), Some(Spec::Colour(c)));
        prop_assert_eq!(colour::parse(&format!("rgb({r}, {g}, {b})")), Some(Spec::Colour(colour::Colour { r, g, b, a: 255 })));
        prop_assert_eq!(colour::parse(&format!("rgb({r} {g} {b} / {}%)", 0)), Some(Spec::Colour(colour::Colour { r, g, b, a: 0 })));
    }

    /// Hue, saturation and lightness of any size, NaN and infinity among them, are channels within 0 to 1.
    #[test]
    fn hsl_is_always_within_its_range(h in any::<f64>(), s in any::<f64>(), l in any::<f64>()) {
        for channel in colour::hsl_to_rgb(h, s, l) {
            prop_assert!((0.0..=1.0).contains(&channel), "{} {} {} -> {}", h, s, l, channel);
        }
        let _ = colour::to_u8(h);
    }

    /// Any bytes, in any mode, hold the law of the fuzz target.
    #[test]
    fn any_bytes_hold_the_law(data in prop::collection::vec(any::<u8>(), 0..1024)) {
        law::check(&data);
    }

    /// Text of the kind a style holds, in every mode.
    #[test]
    fn style_like_text_holds_the_law(mode in 0u8..8, text in "[a-zA-Z0-9=;#%(), .:/+\n\u{e9}-]{0,500}") {
        let mut data = vec![mode];
        data.extend(text.bytes());
        law::check(&data);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the law

/// The seeds: one of each mode, the spec's vectors among them, and what a hostile file does.
fn seeds() -> Vec<Vec<u8>> {
    let with = |mode: u8, text: &str| -> Vec<u8> {
        let mut v = vec![mode];
        v.extend(text.bytes());
        v
    };
    vec![
        with(0, "fillColor=#ff0000;text"),
        with(0, ";fillColor=#00ff00"),
        with(0, "image=data:image/png,iVBORw0KGgo=;rounded=1"),
        with(0, "strokeColor=none;dashed=1;strokeWidth=abc;strokeWidth=0x2"),
        with(0, "blue;plain-blue;icon;edgeLabel;swimlane;nosuch;;;;"),
        with(0, "strokeColor=parentFillColor;fillColor=inherit;fontColor=fillColor;labelBackgroundColor=strokeColor"),
        with(0, "fillColor=light-dark(#7EA6E0,#FF0000);strokeColor=rgb(255, 0, 0);gradientColor=hsl(120, 100%, 50%)"),
        with(0, "=;=;==;a=;=b;;"),
        with(0, "rounded=1;arcSize=-5;absoluteArcSize=1;dashPattern=3 3;fontStyle=15;fontSize=1e999"),
        with(4, "endArrow=none;rounded=0;endFill=0;startSize=10"),
        with(1, "#f00"),
        with(1, "light-dark(light-dark(red,blue),blue)"),
        with(1, "rgba(255 0 0 / 50%)"),
        with(1, "hsl(1e999, 100%, 50%)"),
        with(1, "\u{1b}[31m"),
        with(2, "1e3"),
        with(2, "-0"),
        with(2, "0x2"),
        with(2, "  2  "),
        with(2, "Infinity"),
        with(3, "text\nblue;fillColor=#fff\n\nrounded=1\nrounded=1\n;;;"),
        with(5, "a=1\nb=2"),
        vec![],
        vec![255; 64],
        vec![3; 1000],
        vec![0; 300],
    ]
}

#[test]
fn the_seeds_hold_the_law() {
    for seed in seeds() {
        law::check(&seed);
    }
}

#[test]
fn every_input_that_crashed_the_target_holds_the_law() {
    for (what, input) in CRASHES {
        eprintln!("{what}");
        law::check(input);
    }
}

#[test]
fn every_mode_and_a_sweep_of_short_inputs_hold_the_law() {
    for mode in 0..=255u8 {
        law::check(&[mode]);
    }
    for a in (0..=255u8).step_by(5) {
        for b in (0..=255u8).step_by(7) {
            for mode in 0..8u8 {
                law::check(&[mode, a, b, b';', a, b'=', b'#', b, b'(', b')']);
            }
        }
    }
}
