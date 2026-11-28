//! The language a measured run is in (`diacraft_doc::lang::effective`, design 5.1 of the language-proofing design): `TEXTWIDTH` and `TEXTHEIGHT` read a run's `LangID` as
//! the renderer draws it, so that a text box sized by its measure is as wide as the text it shows.
//!
//! The language shows in one place a test can measure without a font the machine has: `Case` 1 turns `i` into the capital of the run's language, and the capital of
//! Turkish and Azeri is the dotted `İ` (U+0130). A face built in code gives `İ` three times the width of `I`, so the width of the measure says which capital was laid
//! out: a wide one is Turkish, a narrow one is any other language or none. (`crates/render/tests/it/text_language.rs` draws the same table of values, and its last test sets
//! the measure beside the drawing.)

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, Fonts, OperandMode, Value};
use diacraft_doc::{Owner, RowKey, ShapeRef, SheetRef};
use diacraft_fonts::synth::SynthFont;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::probe_addr;
use crate::text_measure::{TERM, db_of, parts};

const FACE: &str = "Probe";

/// `i` at half an inch of type, in capitals, in the probe face, with the `LangID` cell `lang` on its first character row when there is one, and the measure as a formula.
fn label_i(lang: Option<&str>) -> ShapeBuilder {
    let shape = ShapeBuilder::new(1)
        .text_parts(parts("i"))
        .row_cell("Character", None, RowKey::Ix(0), "Font", FACE)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.5")
        .row_cell("Character", None, RowKey::Ix(0), "Case", "1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
        .row_formula("User", None, RowKey::name("Probe"), "Value", "TEXTWIDTH(TheText)", "0");
    match lang {
        Some(lang) => shape.row_cell("Character", None, RowKey::Ix(0), "LangID", lang),
        None => shape,
    }
}

/// The width `shape` measures on a page of `doc`, the operands read as `operands` say.
fn measure_in(doc: DocBuilder, shape: ShapeBuilder, operands: OperandMode) -> f64 {
    let doc = doc.page(1, PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "1").shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let fonts = Fonts::new(db_of(&[SynthFont::new(FACE).ascii(600).advance('\u{130}', 1800)]));
    let mut ctx = EvalCtx::new(EvalOptions { fonts: Some(fonts), operands, ..EvalOptions::default() }, Budget::default());
    match Evaluator::new(&doc, &mut ctx).evaluate(&at) {
        Value::Number(q) => q.stored,
        other => panic!("not a number: {other:?}"),
    }
}

/// The width of `i` in capitals in a shape whose `LangID` is `lang` (`None`: no such cell anywhere), in a drawing whose document language is `doc_lang` (`None`: no
/// `DocLangID`).
fn width(lang: Option<&str>, doc_lang: Option<&str>) -> f64 {
    let a = measure_in(with_doc_lang(doc_lang), label_i(lang), OperandMode::Live);
    // Whether a cell is read as saved or worked out, a constant is the same.
    let b = measure_in(with_doc_lang(doc_lang), label_i(lang), OperandMode::Cached);
    assert_eq!(a, b, "LangID {lang:?} in {doc_lang:?}: live and cached operands measure alike");
    a
}

fn with_doc_lang(doc_lang: Option<&str>) -> DocBuilder {
    match doc_lang {
        Some(doc_lang) => DocBuilder::new().doc_cell("DocLangID", doc_lang),
        None => DocBuilder::new(),
    }
}

/// `İ` at half an inch is a box of 0.9 inch and `I` one of 0.3, and the measure holds the small term.
fn is_turkish(width: f64) -> bool {
    assert!((width - (0.9 + TERM)).abs() < 1e-6 || (width - (0.3 + TERM)).abs() < 1e-6, "the measure is one capital or the other, and was {width}");
    width > 0.6
}

#[test]
fn the_probe_tells_the_dotted_capital_from_the_plain_one() {
    assert!(is_turkish(width(Some("tr-TR"), Some("en-US"))), "a Turkish run is laid out with the dotted capital");
    assert!(!is_turkish(width(Some("en-US"), Some("en-US"))), "an English run with the plain one");
}

#[test]
fn a_run_with_no_cell_anywhere_is_measured_in_the_documents_language() {
    // A drawing made before the language was a cell reads as the document's language. Naive: `en-US` whatever the document says.
    assert!(is_turkish(width(None, Some("tr-TR"))), "no LangID anywhere, DocLangID tr-TR");
    assert!(!is_turkish(width(None, Some("en-US"))), "no LangID anywhere, DocLangID en-US");
    // No DocLangID either: the order's last step is en-US.
    assert!(!is_turkish(width(None, None)), "no LangID and no DocLangID");
}

#[test]
fn a_run_with_a_language_of_its_own_keeps_it_whatever_the_document_says() {
    assert!(is_turkish(width(Some("tr-TR"), Some("en-US"))));
    assert!(!is_turkish(width(Some("en-US"), Some("tr-TR"))), "an English run in a Turkish drawing is English");
    // The tag's case is the file's: `tr-tr` is the same language.
    assert!(is_turkish(width(Some("tr-tr"), Some("en-US"))));
    assert!(is_turkish(width(Some("az-Latn-AZ"), None)), "Azeri has the dotted capital too, with a script and a region");
}

#[test]
fn a_run_of_no_language_is_measured_with_none_and_not_in_the_documents() {
    // `x-none`, `zxx`, empty and `none` mean no language. Naive: the document's language, or `en-US`, or the text `x-none` as a language.
    for none in ["x-none", "X-None", "zxx", "none", ""] {
        assert!(!is_turkish(width(Some(none), Some("tr-TR"))), "LangID `{none}` in a Turkish drawing is no language");
    }
    // 0 is no language in the legacy files' numbers.
    assert!(!is_turkish(width(Some("0"), Some("tr-TR"))), "LangID 0");
}

#[test]
fn a_run_with_a_number_or_an_unreadable_value_is_measured_in_the_documents_language() {
    // 65535 and a number the table lacks read as the document's language. So does text that is no well-formed tag.
    for odd in ["65535", "99999", "not a tag", "tr_TR"] {
        assert!(is_turkish(width(Some(odd), Some("tr-TR"))), "LangID `{odd}` in a Turkish drawing");
        assert!(!is_turkish(width(Some(odd), Some("en-US"))), "LangID `{odd}` in an English drawing");
    }
}

#[test]
fn an_inherited_language_is_found_through_the_root_style() {
    // A shape with no cell of its own reads the root style's `LangID` before the document's language: the chain of the cell, then the default.
    let doc = || DocBuilder::new().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", "tr-TR")).doc_cell("DocLangID", "en-US");
    assert!(is_turkish(measure_in(doc(), label_i(None), OperandMode::Live)), "the root style's Turkish, not the document's English");
    // An empty value on the root style is a value too: no language, and not the document's.
    let doc = || DocBuilder::new().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", "")).doc_cell("DocLangID", "tr-TR");
    assert!(!is_turkish(measure_in(doc(), label_i(None), OperandMode::Live)), "the root style says no language");
    // A cell of the shape's own wins over the root style's.
    let doc = || DocBuilder::new().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", "tr-TR"));
    assert!(!is_turkish(measure_in(doc(), label_i(Some("en-US")), OperandMode::Live)), "the shape's English, not the root style's Turkish");
}

#[test]
fn a_huge_language_value_is_charged_by_its_length_before_it_is_read() {
    // A cell may hold a megabyte. A number is digits and nothing else, so a megabyte of digits is read to its end: the length is charged first, as a cell's `V` is,
    // and what the format keeps is the document's tag, not the cell's text. Naive: the megabyte scanned for nothing, run after run, on a budget that has no step for it.
    let big = "7".repeat(1_000_000);
    let doc = |lang: &str| {
        let shape = label_i(Some(lang));
        DocBuilder::new()
            .doc_cell("DocLangID", "tr-TR")
            .page(1, PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "1").shape(shape))
            .build()
    };
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let fonts = || Fonts::new(db_of(&[SynthFont::new(FACE).ascii(600).advance('\u{130}', 1800)]));

    // With the steps of the default budget it is a number, in the document's language (a number of that size is no language of its own).
    let mut ctx = EvalCtx::new(EvalOptions { fonts: Some(fonts()), ..EvalOptions::default() }, Budget::default());
    let document = doc(&big);
    let v = Evaluator::new(&document, &mut ctx).evaluate(&at);
    let Value::Number(q) = v else { panic!("not a number: {v:?}") };
    assert!(is_turkish(q.stored), "the document's Turkish: {q:?}");
    assert!(ctx.stats.steps >= 1_000_000 / diacraft_calc::rules::UNITS_PER_STEP as u64, "the megabyte was charged: {:?}", ctx.stats);

    // With fewer steps than its length costs, the run ends before the value is read, bounded.
    let mut ctx = EvalCtx::new(EvalOptions { fonts: Some(fonts()), ..EvalOptions::default() }, Budget::with_steps(5_000));
    let v = Evaluator::new(&document, &mut ctx).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}");
    assert!(ctx.stats.budget_exhausted && ctx.stats.steps <= 5_000, "{:?}", ctx.stats);
}
