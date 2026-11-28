//! The language a run is drawn in (design 5.1: the renderer takes `diacraft_doc::lang::effective`).
//!
//! The language shows in one place a test can measure without a font the machine has: `Case` 1 turns `i` into the capital of the run's language, and the capital of
//! Turkish and Azeri is the dotted `İ` (U+0130). A face built in code gives `İ` three times the width of `I`, and its glyphs are solid boxes, so the width of the
//! ink says which capital was drawn: a wide one is Turkish, a narrow one is any other language or none.

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, Fonts, Value};
use diacraft_doc::{Owner, RowKey, ShapeRef, SheetRef};
use diacraft_fonts::{FontDb, synth::SynthFont};
use diacraft_render::Renderer;
use diacraft_testkit::build::{DocBuilder, ShapeBuilder};

use crate::support::*;

const FACE: &str = "Probe";

/// The bundled faces and the probe face, in which `İ` is three times as wide as `I`.
fn probe_db() -> FontDb {
    let mut db = FontDb::bundled();
    db.add_font_data(SynthFont::new(FACE).ascii(600).advance('\u{130}', 1800).build());
    db
}

/// The widths of the ink of `i` set in capitals in a shape whose `LangID` cell is `lang` (`None`: the shape has no such cell anywhere), in a drawing whose document
/// language is `doc_lang` (`None`: no `DocLangID`).
fn ink_width(lang: Option<&str>, doc_lang: Option<&str>) -> f64 {
    let mut doc = base();
    if let Some(doc_lang) = doc_lang {
        doc = doc.doc_cell("DocLangID", doc_lang);
    }
    ink_width_in(doc, label_i(lang))
}

/// A 2 by 2 inch box showing `i` at half an inch in capitals in the probe face, with `LangID` `lang` on its first character row when there is one.
fn label_i(lang: Option<&str>) -> ShapeBuilder {
    let shape = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text("i")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.5")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
        .row_cell("Character", None, RowKey::Ix(0), "Font", FACE)
        .row_cell("Character", None, RowKey::Ix(0), "Case", "1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0");
    match lang {
        Some(lang) => shape.row_cell("Character", None, RowKey::Ix(0), "LangID", lang),
        None => shape,
    }
}

/// The width of the ink of `shape` on a page of `doc`, drawn with the probe face beside the bundled ones.
fn ink_width_in(doc: DocBuilder, shape: ShapeBuilder) -> f64 {
    let doc = doc.page(1, page(4.0, 4.0).shape(shape)).build();
    let pic = draw_with(&mut Renderer::with_fonts(probe_db()), &doc);
    let ink = pic.ink_box().expect("the capital is drawn");
    ink.2 - ink.0
}

fn is_turkish(width: f64) -> bool {
    // 0.5 inch of type: `I` is a box of 0.3 inch and `İ` one of 0.9.
    width > 0.6
}

#[test]
fn the_probe_tells_the_dotted_capital_from_the_plain_one() {
    assert!(is_turkish(ink_width(Some("tr-TR"), Some("en-US"))), "a Turkish run draws the dotted capital");
    assert!(!is_turkish(ink_width(Some("en-US"), Some("en-US"))), "an English run draws the plain one");
}

#[test]
fn a_run_with_no_cell_anywhere_takes_the_documents_language() {
    // A drawing made before the language was a cell reads as the document's language. Naive: `en-US` whatever the document says.
    assert!(is_turkish(ink_width(None, Some("tr-TR"))), "no LangID anywhere, DocLangID tr-TR");
    assert!(!is_turkish(ink_width(None, Some("en-US"))), "no LangID anywhere, DocLangID en-US");
    // No DocLangID either: the order's last step is en-US.
    assert!(!is_turkish(ink_width(None, None)), "no LangID and no DocLangID");
}

#[test]
fn a_run_with_a_language_of_its_own_keeps_it_whatever_the_document_says() {
    assert!(is_turkish(ink_width(Some("tr-TR"), Some("en-US"))));
    assert!(!is_turkish(ink_width(Some("en-US"), Some("tr-TR"))), "an English run in a Turkish drawing is English");
    // The tag's case is the file's: `tr-tr` is the same language.
    assert!(is_turkish(ink_width(Some("tr-tr"), Some("en-US"))));
    assert!(is_turkish(ink_width(Some("az-Latn-AZ"), None)), "Azeri has the dotted capital too, with a script and a region");
}

#[test]
fn a_run_of_no_language_is_drawn_with_none_and_not_in_the_documents() {
    // `x-none`, `zxx`, empty and `none` mean no language. Naive: the document's language (a Turkish capital in a Turkish drawing), or the
    // text `x-none` handed to the shaper as a language.
    for none in ["x-none", "X-None", "zxx", "none", ""] {
        assert!(!is_turkish(ink_width(Some(none), Some("tr-TR"))), "LangID `{none}` in a Turkish drawing is no language");
    }
    // 0 is no language in the legacy files' numbers.
    assert!(!is_turkish(ink_width(Some("0"), Some("tr-TR"))), "LangID 0");
}

#[test]
fn a_run_with_a_number_or_an_unreadable_value_is_drawn_in_the_documents_language() {
    // 65535 and a number the table lacks read as the document's language. So does text that is no well-formed tag.
    for odd in ["65535", "99999", "not a tag", "tr_TR"] {
        assert!(is_turkish(ink_width(Some(odd), Some("tr-TR"))), "LangID `{odd}` in a Turkish drawing");
        assert!(!is_turkish(ink_width(Some(odd), Some("en-US"))), "LangID `{odd}` in an English drawing");
    }
}

#[test]
fn an_inherited_language_is_found_through_the_root_style() {
    // A shape with no cell of its own reads the root style's `LangID` before the document's language: the chain of the cell, then the default.
    let doc = base().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", "tr-TR")).doc_cell("DocLangID", "en-US");
    assert!(is_turkish(ink_width_in(doc, label_i(None))), "the root style's Turkish, not the document's English");
    // An empty value on the root style is a value too: no language, and not the document's.
    let doc = base().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", "")).doc_cell("DocLangID", "tr-TR");
    assert!(!is_turkish(ink_width_in(doc, label_i(None))), "the root style says no language");
}

// ---- the measure and the drawing -------------------------------------------------------------------------------------------------

/// `label_i` with no margins, and the formula that measures its text as a cell of the shape.
fn measured_label(lang: Option<&str>) -> ShapeBuilder {
    label_i(lang).cell("LeftMargin", "0").cell("RightMargin", "0").cell("TopMargin", "0").cell("BottomMargin", "0").row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        "TEXTWIDTH(TheText)",
        "0",
    )
}

/// What `TEXTWIDTH(TheText)` gives for `measured_label(lang)` on a page of `doc`, in the faces the renderer draws with: the advance of the capital, 0.3 inch for `I` and
/// 0.9 for `İ` at half an inch of type, and the small term of the measure (5E-5 inch). The ink of the drawing is narrower than the advance, as the probe face's boxes sit
/// inside it, so what the two are compared by is which capital they hold.
fn measured_width(doc: DocBuilder, lang: Option<&str>) -> f64 {
    let doc = doc.page(1, page(4.0, 4.0).shape(measured_label(lang))).build();
    let at = CellAddress::new(
        SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)),
        diacraft_doc::CellAddr::in_row("User", None, RowKey::name("Probe"), "Value"),
    );
    let mut ctx = EvalCtx::new(EvalOptions { fonts: Some(Fonts::new(probe_db())), ..EvalOptions::default() }, Budget::default());
    match Evaluator::new(&doc, &mut ctx).evaluate(&at) {
        Value::Number(q) => q.stored,
        other => panic!("the measure is not a number: {other:?}"),
    }
}

fn doc_in(doc_lang: Option<&str>) -> DocBuilder {
    match doc_lang {
        Some(doc_lang) => base().doc_cell("DocLangID", doc_lang),
        None => base(),
    }
}

#[test]
fn the_measure_and_the_drawing_take_the_same_capital() {
    // The text measure (`TEXTWIDTH`, `TEXTHEIGHT`) and the renderer read a run's language by one function (`lang::saved_value` and `lang::shaping_tag`). A text box that
    // grows with its text is as wide as the text it shows only where they agree, and they differ in the capital of a Turkish `i` and in a face's language forms. Naive: the
    // measure reads a run with no `LangID` as `en-US` and an empty one as `en-US`, where the renderer takes the document's language and none.
    let cases: [(Option<&str>, Option<&str>); 17] = [
        (None, Some("tr-TR")),
        (None, Some("en-US")),
        (None, None),
        (Some("tr-TR"), Some("en-US")),
        (Some("en-US"), Some("tr-TR")),
        (Some("tr-tr"), None),
        (Some("az-Latn-AZ"), None),
        (Some("x-none"), Some("tr-TR")),
        (Some("zxx"), Some("tr-TR")),
        (Some("none"), Some("tr-TR")),
        (Some(""), Some("tr-TR")),
        (Some("0"), Some("tr-TR")),
        (Some("65535"), Some("tr-TR")),
        (Some("not a tag"), Some("tr-TR")),
        (Some("not a tag"), Some("en-US")),
        (Some("tr_TR"), Some("tr-TR")),
        (Some("en-FI"), Some("tr-TR")),
    ];
    let (mut wide, mut narrow) = (0, 0);
    for (lang, doc_lang) in cases {
        let drawn = ink_width_in(doc_in(doc_lang), measured_label(lang));
        let measured = measured_width(doc_in(doc_lang), lang);
        assert!(
            (measured - 0.9).abs() < 0.001 || (measured - 0.3).abs() < 0.001,
            "LangID {lang:?} in a drawing in {doc_lang:?}: the measure is the advance of one capital or the other, and was {measured}"
        );
        assert_eq!(
            is_turkish(drawn),
            is_turkish(measured),
            "LangID {lang:?} in a drawing in {doc_lang:?}: drawn with ink {drawn} inch wide, measured {measured} inch (the dotted capital is the wide one)"
        );
        if is_turkish(drawn) {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    assert!(wide >= 5 && narrow >= 5, "the table holds both capitals ({wide} dotted, {narrow} plain), so that agreeing means something");
}
