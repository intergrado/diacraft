//! A draw.io cell's `textDirection` (S-R40 with V23): `rtl` reads as `Flags` 1 on
//! each paragraph of the label, and `ltr`, `auto`, `default`, an absent key, `vertical-lr` and `vertical-rl` read as 0. This is the reader's half: the style says the direction, and a label
//! given it says it of each of its paragraphs. The model task that writes the `Paragraph` rows reads `Para::direction`.
//!
//! Every style and label is a string written by hand from the spec; no file made by another program is read.

use diacraft_mxfile::html::{Options, read_html, read_plain};
use diacraft_mxfile::style::{CellKind, Style, StyleCache, TextDirection};
use diacraft_mxfile::{Limits, Reading};

const IGNORED: &str = "drawio_style_keys_ignored";

/// The style of a cell whose style text is `text`, resolved as a shape's.
fn style_of(text: &str, reading: &mut Reading<'_>) -> std::sync::Arc<Style> {
    StyleCache::new().resolve(Some(text), CellKind::Shape, reading).unwrap()
}

/// The `Flags` each paragraph of the label `html` gets under the style text `style`.
fn flags_of(style: &str, html: &str, plain: bool) -> Vec<u8> {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let style = style_of(style, &mut reading);
    let mut label = if plain {
        read_plain(html, &Options::default(), &mut reading).unwrap()
    } else {
        read_html(html, &Options::default(), &mut reading).unwrap()
    };
    label.set_direction(style.text_direction());
    label.paras.iter().map(|p| p.direction.flags()).collect()
}

/// A two-paragraph label whose style holds `textDirection=rtl` has `Flags` 1 on both paragraphs.
#[test]
fn v78_rtl_gives_each_paragraph_of_the_label_flags_1() {
    assert_eq!(flags_of("rounded=0;whiteSpace=wrap;textDirection=rtl;", "<div>one</div><div>two</div>", false), [1, 1]);
    // A plain label is one paragraph (its line feeds are breaks of the line, S-R26).
    assert_eq!(flags_of("textDirection=rtl;", "one\ntwo", true), [1], "a plain label too");
    // A label with one paragraph, and one with an empty line.
    assert_eq!(flags_of("html=1;textDirection=rtl;", "<div><br></div><div>x</div>", false), [1, 1]);
}

/// Every other value, and no key, leaves `Flags` 0; a vertical value is not mapped.
#[test]
fn v78_ltr_auto_default_no_key_and_the_vertical_values_give_flags_0() {
    for style in [
        "textDirection=ltr;",
        "textDirection=auto;",
        "textDirection=default;",
        "textDirection=vertical-lr;",
        "textDirection=vertical-rl;",
        "textDirection=;",
        "textDirection=RTL;",
        "rounded=0;",
        "",
    ] {
        assert_eq!(flags_of(style, "<div>one</div><div>two</div>", false), [0, 0], "{style:?}");
    }
}

/// The style says the direction; a named style and a later piece are read as for any key.
#[test]
fn the_style_reads_text_direction_as_a_key() {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    assert_eq!(style_of("textDirection=rtl;", &mut reading).text_direction(), TextDirection::Rtl);
    assert_eq!(style_of("textDirection=rtl;textDirection=ltr;", &mut reading).text_direction(), TextDirection::Ltr, "the last piece wins (S-R4)");
    assert_eq!(
        style_of("textDirection=rtl;textDirection=none;", &mut reading).text_direction(),
        TextDirection::Ltr,
        "none takes the key away (S-R4)"
    );
    assert_eq!(TextDirection::Ltr.flags(), 0);
    assert_eq!(TextDirection::Rtl.flags(), 1);
}

/// `textDirection` is a key the importer reads: it is no more one of the keys it ignores.
#[test]
fn text_direction_is_no_ignored_key() {
    let limits = Limits::default();
    let mut cache = StyleCache::new();
    let mut reading = Reading::new(&limits);
    cache.resolve(Some("textDirection=rtl;sketchStyle=x;"), CellKind::Shape, &mut reading).unwrap();
    cache.finish(&mut reading).unwrap();
    let cases = reading.report.warnings.get(IGNORED).map(|n| n.cases.clone()).unwrap_or_default();
    assert!(cases.iter().any(|c| c.contains("sketchStyle")), "another key is still counted: {cases:?}");
    assert!(!cases.iter().any(|c| c.contains("textDirection")), "{cases:?}");
}
