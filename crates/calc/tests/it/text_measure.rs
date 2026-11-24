//! `TEXTWIDTH` and `TEXTHEIGHT`:
//! the size of a shape's text laid out by the text crate.
//!
//! Most vectors use a synthetic face, `Em`, in which every printable ASCII character advances exactly one em, so a width is a count of em and
//! no installed font can change a number. The tests on real faces ask the database for a face that has the letters they need, and say
//! `SKIPPED: <why>` on standard error where the machine has none. Lengths in the cells are paper inches, as the cells hold them.

use std::sync::{Arc, OnceLock};

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, Fonts, Value};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, TextPart};
use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, FontStyle};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::probe_addr;

pub fn pt(v: f64) -> f64 {
    v / 72.0
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The constant a project rule: the small term, in paper inches, that a measure of a text holding a character carries; none for empty text.
pub const TERM: f64 = 5e-5;

/// A database of the synthetic faces.
pub fn db_of(fonts: &[SynthFont]) -> FontDb {
    let mut db = FontDb::empty();
    for f in fonts {
        assert_eq!(db.add_font_data(f.build()).added, 1, "{} should parse", f.family);
    }
    db
}

/// `Em`: every printable ASCII character, and a no-break space, advances one em.
pub fn em() -> Fonts {
    Fonts::new(db_of(&[SynthFont::new("Em").ascii(1000).chars("\u{00A0}", 1000)]))
}

pub fn parts(text: &str) -> Vec<TextPart> {
    vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Tp(0), TextPart::Chars(Arc::from(text))]
}

/// A shape with this text in face `Em` at `size_pt`, lines at 120% (`SpLine` -1.2), left aligned, with margins of 4 points.
pub fn shape(text: &str, size_pt: f64) -> ShapeBuilder {
    ShapeBuilder::new(1)
        .text_parts(parts(text))
        .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
        .row_cell("Character", None, RowKey::Ix(0), "Size", &pt(size_pt).to_string())
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
        .cell("LeftMargin", &pt(4.0).to_string())
        .cell("RightMargin", &pt(4.0).to_string())
        .cell("TopMargin", &pt(4.0).to_string())
        .cell("BottomMargin", &pt(4.0).to_string())
}

/// A page of this shape, with the probe formula added to it and the page's scale cells.
fn document(shape: ShapeBuilder, formula: &str, scale: (&str, &str)) -> (Document, CellAddress) {
    let shape = shape.row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
    let page = PageBuilder::new().cell("PageScale", scale.0).cell("DrawingScale", scale.1).shape(shape);
    let doc = DocBuilder::new().page(1, page).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    (doc, at)
}

fn options(fonts: Fonts) -> EvalOptions {
    EvalOptions { fonts: Some(fonts), ..EvalOptions::default() }
}

fn run(shape: ShapeBuilder, formula: &str, fonts: Fonts) -> Value {
    run_scaled(shape, formula, fonts, ("1", "1")).0
}

/// The value, and what the evaluation did.
fn run_scaled(shape: ShapeBuilder, formula: &str, fonts: Fonts, scale: (&str, &str)) -> (Value, diacraft_calc::EvalStats) {
    let (doc, at) = document(shape, formula, scale);
    let mut ctx = EvalCtx::new(options(fonts), Budget::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    (v, ctx.stats)
}

fn width(shape: ShapeBuilder, formula: &str) -> f64 {
    match run(shape, formula, em()) {
        Value::Number(q) => q.stored,
        other => panic!("`{formula}`: {other:?}"),
    }
}

// ---- the vectors ---------------------------------------------------------------------------------------------------------------------------

/// A text of three em at 12 pt (0.5 inch), a space of one em (1/6 inch), margins of 4 pt each and a final line feed,
/// on a 1:1 page: 0.5 + 1/6 + 8/72 = 0.777778. Leaving out the space gives 0.611111. At `DrawingScale` 10 over `PageScale` 1 the answer is in
/// drawing units, ten times the paper's: 7.77778.
#[test]
fn measure_the_final_line_feed_measures_as_a_space() {
    let w = width(shape("abc\n", 12.0), "TEXTWIDTH(TheText)");
    assert!(near(w, 0.5 + 1.0 / 6.0 + 8.0 / 72.0 + TERM), "{w}");
    let (v, _) = run_scaled(shape("abc\n", 12.0), "TEXTWIDTH(TheText)", em(), ("1", "10"));
    let Value::Number(q) = v else { panic!("{v:?}") };
    assert!(near(q.stored, 10.0 * (0.5 + 1.0 / 6.0 + 8.0 / 72.0 + TERM)), "{q:?}");
    assert_eq!(q.unit, diacraft_units::Unit::DrawingDefault);
    // No final line feed (project rule, cannot tell): nothing for the missing mark.
    let w = width(shape("abc", 12.0), "TEXTWIDTH(TheText)");
    assert!(near(w, 0.5 + 8.0 / 72.0 + TERM), "{w}");
}

/// Empty text, 12 pt, `SpLine` -1.2, margins 4 pt, on a 1:1 page: `TEXTHEIGHT` 0.311111, not 0.111111 for zero
/// lines. At `DrawingScale` 10 over `PageScale` 1: 3.11111; margins in drawing units would give 2.11111. Empty text with
/// margins of 9 pt each is 0.25.
#[test]
fn measure_empty_text_is_one_line_and_the_margins() {
    let empty = |s: ShapeBuilder| s.text_parts(parts(""));
    let h = width(empty(shape("", 12.0)), "TEXTHEIGHT(TheText,4 in)");
    assert!(near(h, 0.2 + 8.0 / 72.0), "{h}");
    let (v, _) = run_scaled(empty(shape("", 12.0)), "TEXTHEIGHT(TheText,4 in)", em(), ("1", "10"));
    let Value::Number(q) = v else { panic!("{v:?}") };
    assert!(near(q.stored, 10.0 * (0.2 + 8.0 / 72.0)), "{q:?}");
    // A shape with no text element at all is empty text too.
    let none = ShapeBuilder::new(1)
        .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
        .row_cell("Character", None, RowKey::Ix(0), "Size", &pt(12.0).to_string())
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
        .cell("TopMargin", &pt(4.0).to_string())
        .cell("BottomMargin", &pt(4.0).to_string());
    assert!(near(width(none, "TEXTHEIGHT(TheText,4 in)"), 0.2 + 8.0 / 72.0));
    let nine = |s: ShapeBuilder| s.cell("LeftMargin", &pt(9.0).to_string()).cell("RightMargin", &pt(9.0).to_string());
    assert!(near(width(nine(shape("", 12.0)), "TEXTWIDTH(TheText)"), 0.25));
}

/// A page of this scale (`PageScale`, `DrawingScale`) holding the shape.
fn scaled_page(shape: ShapeBuilder, scale: (&str, &str)) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().cell("PageScale", scale.0).cell("DrawingScale", scale.1).shape(shape)).build()
}

/// The stored value of the cell `name` of the first shape of page 1, in the faces `Em`.
fn cell_value(doc: &Document, name: &str) -> f64 {
    value_at(doc, CellAddr::direct(name))
}

/// The stored value of a cell of the first shape of page 1, in the faces `Em`.
fn value_at(doc: &Document, addr: CellAddr) -> f64 {
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), addr.clone());
    let mut ctx = EvalCtx::new(options(em()), Budget::default());
    match Evaluator::new(doc, &mut ctx).evaluate(&at) {
        Value::Number(q) => q.stored,
        other => panic!("{addr:?}: {other:?}"),
    }
}

/// `1e-9` apart: the cells below are compared to the rule, not to its tolerance.
fn exactly(got: f64, want: f64) -> bool {
    (got - want).abs() < 1e-9
}

/// For, the constant a project rule: one line of text at 12 pt, `SpLine` -1.2, margins of 4 pt, `DrawingScale` 10 over
/// `PageScale` 1, and a local `TxtHeight` formula of `TEXTHEIGHT` of it: (0.311111 + 5E-5) times 10, 3.1116111. Adding the term after the scale
/// gives 3.1111611 and dropping it 3.1111111. With empty text, 3.1111111. `TEXTWIDTH` carries the same term inside the scale.
#[test]
fn measure_the_small_term_sits_inside_the_scale_for_text_that_holds_a_character() {
    let m = 8.0 / 72.0;
    let local = |text: &str| {
        shape(text, 12.0).cell("TxtWidth", "40").formula("TxtHeight", "TEXTHEIGHT(TheText,TxtWidth)", "0").formula("Wide", "TEXTWIDTH(TheText)", "0")
    };
    let with_text = scaled_page(local("abc"), ("1", "10"));
    let height = cell_value(&with_text, "TxtHeight");
    assert!(exactly(height, (0.2 + m + TERM) * 10.0), "{height}");
    assert!(!exactly(height, (0.2 + m) * 10.0 + TERM), "the term after the scale gives 3.1111611: {height}");
    assert!(!exactly(height, (0.2 + m) * 10.0), "no term gives 3.1111111: {height}");
    let width = cell_value(&with_text, "Wide");
    assert!(exactly(width, (0.5 + m + TERM) * 10.0), "{width}");
    assert!(!exactly(width, (0.5 + m) * 10.0 + TERM), "{width}");
    // Empty text: the margins and one line, with no term.
    let empty = scaled_page(local(""), ("1", "10"));
    assert!(exactly(cell_value(&empty, "TxtHeight"), (0.2 + m) * 10.0), "{}", cell_value(&empty, "TxtHeight"));
    assert!(exactly(cell_value(&empty, "Wide"), 2.0 * pt(4.0) * 10.0), "empty text is the margins alone: {}", cell_value(&empty, "Wide"));
    // On a 1:1 page the term is 5E-5 and the oracle's tolerance hides it ("plus at most 1E-4").
    let unscaled = scaled_page(local("abc"), ("1", "1"));
    assert!(exactly(cell_value(&unscaled, "TxtHeight"), 0.2 + m + TERM));
}

/// A `User` cell that gives 1 when `TEXTHEIGHT` of the text at its width equals 0.25 inch and 2 otherwise, on a 1:1 page, with
/// margins 0, `SpLine` 0.25 inch and one line of text at 15 pt: 2, as the height is 0.25005. A measure with no term gives 1. With empty text: 1.
#[test]
fn measure_a_test_for_equality_with_a_fixed_height_shows_the_term() {
    let tested = |text: &str| {
        shape(text, 15.0)
            .cell("LeftMargin", "0")
            .cell("RightMargin", "0")
            .cell("TopMargin", "0")
            .cell("BottomMargin", "0")
            .cell("TxtWidth", "4")
            .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "0.25")
    };
    let test = "IF(TEXTHEIGHT(TheText,TxtWidth)=0.25,1,2)";
    assert_eq!(width(tested("abc"), test), 2.0, "one line of text: 0.25005, not 0.25");
    assert_eq!(width(tested(""), test), 1.0, "empty text: 0.25");
    let h = width(tested("abc"), "TEXTHEIGHT(TheText,TxtWidth)");
    assert!(exactly(h, 0.25 + TERM), "{h}");
}

/// At 12 pt, `SpLine` -1.2 and margins of 4 pt at `DrawingScale` 10 over `PageScale` 1, text that is only a line feed measures one
/// line plus the term, 3.1116111 inches, and text with no characters 3.1111111. Treating the line feed as no text gives 3.1111111 for both, and counting it as a
/// second line 5.1116111. A field that shows no character is no text either, and one that shows a character is.
#[test]
fn measure_a_line_feed_counts_as_a_character_and_a_field_that_shows_none_does_not() {
    let m = 8.0 / 72.0;
    let one_line = (0.2 + m + TERM) * 10.0;
    let height =
        |s: ShapeBuilder| cell_value(&scaled_page(s.cell("TxtWidth", "40").formula("H", "TEXTHEIGHT(TheText,TxtWidth)", "0"), ("1", "10")), "H");
    assert!(exactly(height(shape("\n", 12.0)), one_line), "{}", height(shape("\n", 12.0)));
    assert!(exactly(height(shape("", 12.0)), (0.2 + m) * 10.0));
    let field = |shown: &str, tail: &str| {
        let mut parts = vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Fld { ix: 0, text: Arc::from(shown) }];
        if !tail.is_empty() {
            parts.push(TextPart::Chars(Arc::from(tail)));
        }
        shape("", 12.0).text_parts(parts)
    };
    assert!(exactly(height(field("", "\n")), one_line), "a line feed beside an empty field: one line plus the term");
    assert!(exactly(height(field("", "")), (0.2 + m) * 10.0), "an empty field alone shows no character");
    assert!(exactly(height(field("12", "")), one_line), "a field that shows characters");
    // The width of a text that is only a line feed: the margins, the space the feed measures as (one em at 12 pt in `Em`) and the term.
    let wide = |s: ShapeBuilder| cell_value(&scaled_page(s.formula("W", "TEXTWIDTH(TheText)", "0"), ("1", "10")), "W");
    assert!(exactly(wide(shape("\n", 12.0)), (2.0 * pt(4.0) + pt(12.0) + TERM) * 10.0), "{}", wide(shape("\n", 12.0)));
}

/// The term is in the function every call reaches, so a formula an instance takes from its master gets it as one the shape holds ("local or
/// inherited": 1,400 local and 1,741 inherited cells fit it).
#[test]
fn the_small_term_is_in_the_formulas_an_instance_inherits_too() {
    let m = 8.0 / 72.0;
    let w_cell = || CellAddr::in_row("User", None, RowKey::name("W"), "Value");
    for (text, term) in [("abc", TERM), ("", 0.0)] {
        let master = ShapeBuilder::new(5).formula("TxtHeight", "TEXTHEIGHT(TheText,TxtWidth)", "0").row_formula(
            "User",
            None,
            RowKey::name("W"),
            "Value",
            "TEXTWIDTH(TheText)",
            "0",
        );
        let instance = shape(text, 12.0).master(1).cell("TxtWidth", "40").inh("TxtHeight", Some("0")).row_inh(
            "User",
            None,
            RowKey::name("W"),
            "Value",
            Some("0"),
        );
        let doc = DocBuilder::new()
            .master(1, MasterBuilder::new().shape(master))
            .page(1, PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "10").shape(instance))
            .build();
        let h = cell_value(&doc, "TxtHeight");
        assert!(exactly(h, (0.2 + m + term) * 10.0), "{text:?}: {h}");
        let w = value_at(&doc, w_cell());
        let advances = if text.is_empty() { 0.0 } else { 0.5 };
        assert!(exactly(w, (advances + 2.0 * pt(4.0) + term) * 10.0), "{text:?}: {w}");
    }
}

/// For the Errors facets: a reference that is not a `TheText` cell is `#VALUE!`.
#[test]
fn a_reference_that_is_not_the_text_is_value() {
    for f in ["TEXTHEIGHT(Width,4 in)", "TEXTWIDTH(Height)", "TEXTWIDTH(3)", "TEXTHEIGHT(User.Probe,1)"] {
        assert_eq!(run(shape("a", 12.0), f, em()), Value::Error(diacraft_calc::CellError::Value), "{f}");
    }
    // An error in the limit comes back.
    assert_eq!(run(shape("a", 12.0), "TEXTHEIGHT(TheText,1/0)", em()), Value::Error(diacraft_calc::CellError::DivZero));
    // Too many arguments are `#VALUE!` (of the text batch); `TEXTHEIGHT` may leave the width out.
    assert_eq!(run(shape("a", 12.0), "TEXTWIDTH(TheText,1,2)", em()), Value::Error(diacraft_calc::CellError::Value));
    assert!(matches!(run(shape("a", 12.0), "TEXTHEIGHT(TheText)", em()), Value::Number(_)));
}

/// Three words of three em at 12 pt, and a limit on the block's width (margins in it).
/// Width 2.0 holds the line (1.8333 inch of text), 1.2 + margins holds two words (1.1667), and 0.6 + margins one word to a line.
#[test]
fn a_limit_wraps_the_text_and_the_height_counts_the_lines() {
    let m = 8.0 / 72.0;
    let height = |limit: f64| width(shape("aaa bbb ccc", 12.0), &format!("TEXTHEIGHT(TheText,{limit})"));
    assert!(near(height(2.0), 0.2 + m + TERM), "{}", height(2.0));
    assert!(near(height(1.2 + m), 0.4 + m + TERM), "{}", height(1.2 + m));
    assert!(near(height(0.6 + m), 0.6 + m + TERM), "{}", height(0.6 + m));
    // The width is the longest line that fits: "aaa bbb" without its trailing space.
    let w = width(shape("aaa bbb ccc", 12.0), &format!("TEXTWIDTH(TheText,{})", 1.2 + m));
    assert!(near(w, 7.0 / 6.0 + m + TERM), "{w}");
    // With no limit the text is one line.
    assert!(near(width(shape("aaa bbb ccc", 12.0), "TEXTWIDTH(TheText)"), 11.0 / 6.0 + m + TERM), "unlimited");
    assert!(near(width(shape("aaa bbb ccc", 12.0), "TEXTHEIGHT(TheText)"), 0.2 + m + TERM), "unlimited");
    // The block's own width is a limit the same way: TEXTHEIGHT(TheText, TxtWidth) is the height the block draws.
    let own = |s: ShapeBuilder| s.cell("TxtWidth", &(1.2 + m).to_string());
    assert!(near(width(own(shape("aaa bbb ccc", 12.0)), "TEXTHEIGHT(TheText,TxtWidth)"), 0.4 + m + TERM));
}

/// Lines are `SpLine` times the largest size on them, a line feed ends a paragraph and an extra one adds a
/// line, and the space between paragraphs is `SpAfter` and `SpBefore`, left out above the first and below the last.
#[test]
fn lines_paragraphs_and_spacing_stack_as_the_block_does() {
    let m = 8.0 / 72.0;
    // Two paragraphs: a line each, and 3 pt after the first and 6 pt before the second.
    let two = |s: ShapeBuilder| {
        s.text_parts(vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Chars(Arc::from("a\n")), TextPart::Pp(1), TextPart::Chars(Arc::from("b\n"))])
            .row_cell("Paragraph", None, RowKey::Ix(0), "SpAfter", &pt(3.0).to_string())
            .row_cell("Paragraph", None, RowKey::Ix(0), "SpBefore", &pt(5.0).to_string())
            .row_cell("Paragraph", None, RowKey::Ix(1), "SpLine", "-1.2")
            .row_cell("Paragraph", None, RowKey::Ix(1), "SpBefore", &pt(6.0).to_string())
            .row_cell("Paragraph", None, RowKey::Ix(1), "SpAfter", &pt(7.0).to_string())
    };
    let h = width(two(shape("", 12.0)), "TEXTHEIGHT(TheText,4 in)");
    assert!(near(h, 0.4 + pt(3.0) + pt(6.0) + m + TERM), "{h}: SpBefore above the first and SpAfter below the last are not in it");
    // A blank paragraph between others is a line of its own.
    assert!(near(width(shape("a\n\nb\n", 12.0), "TEXTHEIGHT(TheText,4 in)"), 0.6 + m + TERM));
    // `SpLine` positive is a distance; 0 is one size's height.
    fn fixed(s: ShapeBuilder, v: &str) -> ShapeBuilder {
        s.row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", v)
    }
    assert!(near(width(fixed(shape("a\nb\n", 12.0), "0.5"), "TEXTHEIGHT(TheText,4 in)"), 1.0 + m + TERM));
    assert!(near(width(fixed(shape("a\nb\n", 12.0), "0"), "TEXTHEIGHT(TheText,4 in)"), 2.0 / 6.0 + m + TERM));
    // A line takes its height from the largest run on it: 12 pt and 8 pt sizes together.
    let mixed = |s: ShapeBuilder| {
        s.text_parts(vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Chars(Arc::from("ab")), TextPart::Cp(1), TextPart::Chars(Arc::from("cd\n"))])
            .row_cell("Character", None, RowKey::Ix(1), "Font", "Em")
            .row_cell("Character", None, RowKey::Ix(1), "Size", &pt(8.0).to_string())
    };
    assert!(near(width(mixed(shape("", 12.0)), "TEXTHEIGHT(TheText,4 in)"), 0.2 + m + TERM));
    // Several runs, several sizes: the width is the sum of the advances, the final space at the size of the run that holds the
    // line feed (8 pt).
    let w = width(mixed(shape("", 12.0)), "TEXTWIDTH(TheText)");
    assert!(near(w, 2.0 * pt(12.0) + 2.0 * pt(8.0) + pt(8.0) + m + TERM), "{w}");
}

/// `FontScale` widens every letter and `Letterspace` adds to each; `Case` capitals do not change a width in a face of one advance;
/// a text of fields counts the field's displayed characters.
#[test]
fn font_scale_letterspace_and_fields_are_measured_as_drawn() {
    let m = 8.0 / 72.0;
    let scaled = |s: ShapeBuilder| s.row_cell("Character", None, RowKey::Ix(0), "FontScale", "2");
    assert!(near(width(scaled(shape("abc", 12.0)), "TEXTWIDTH(TheText)"), 2.0 * 0.5 + m + TERM));
    let spaced = |s: ShapeBuilder| s.row_cell("Character", None, RowKey::Ix(0), "Letterspace", &pt(1.0).to_string());
    assert!(near(width(spaced(shape("abc", 12.0)), "TEXTWIDTH(TheText)"), 0.5 + 3.0 * pt(1.0) + m + TERM));
    let field = |s: ShapeBuilder| {
        s.text_parts(vec![
            TextPart::Cp(0),
            TextPart::Pp(0),
            TextPart::Chars(Arc::from("a")),
            TextPart::Fld { ix: 0, text: Arc::from("1234") },
            TextPart::Chars(Arc::from("\n")),
        ])
    };
    assert!(near(width(field(shape("", 12.0)), "TEXTWIDTH(TheText)"), 5.0 / 6.0 + 1.0 / 6.0 + m + TERM));
}

/// A shape with no text of its own shows its master's, measured with its own formats. And its field values are its own, which are
/// not worked out here: such a text is unsupported and the saved value stands.
#[test]
fn an_instance_measures_its_masters_text() {
    let master_shape = ShapeBuilder::new(5).text_parts(parts("abc\n"));
    let field_shape = ShapeBuilder::new(5).text_parts(vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Fld { ix: 0, text: Arc::from("12") }]);
    for (master, expect_unsupported) in [(master_shape, false), (field_shape, true)] {
        let instance = ShapeBuilder::new(1)
            .master(1)
            .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
            .row_cell("Character", None, RowKey::Ix(0), "Size", &pt(12.0).to_string())
            .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
            .row_formula("User", None, RowKey::name("Probe"), "Value", "TEXTWIDTH(TheText)", "0");
        let doc = DocBuilder::new().master(1, MasterBuilder::new().shape(master)).page(1, PageBuilder::new().shape(instance)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::new(options(em()), Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        if expect_unsupported {
            assert!(matches!(v, Value::Unsupported(_)), "{v:?}");
        } else {
            let Value::Number(q) = v else { panic!("{v:?}") };
            assert!(near(q.stored, 0.5 + 1.0 / 6.0 + TERM), "{q:?}");
        }
    }
}

/// And the renderer's reading: a shape with no `Font` of its own reads the root style's, and one with no margin cell reads the root
/// style's margin.
#[test]
fn the_root_style_stands_where_a_shape_holds_nothing() {
    let bare = ShapeBuilder::new(1).text_parts(parts("abc\n")).row_cell("Character", None, RowKey::Ix(0), "Size", &pt(12.0).to_string()).row_cell(
        "Paragraph",
        None,
        RowKey::Ix(0),
        "SpLine",
        "-1.2",
    );
    let rooted = DocBuilder::new().root(|r| {
        r.row_cell("Character", None, RowKey::Ix(0), "Font", "Em").cell("LeftMargin", &pt(4.0).to_string()).cell("RightMargin", &pt(4.0).to_string())
    });
    let shape = bare.row_formula("User", None, RowKey::name("Probe"), "Value", "TEXTWIDTH(TheText)", "0");
    let doc = rooted.page(1, PageBuilder::new().shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let mut ctx = EvalCtx::new(options(em()), Budget::default());
    let Value::Number(q) = Evaluator::new(&doc, &mut ctx).evaluate(&at) else { panic!("not a number") };
    assert!(near(q.stored, 0.5 + 1.0 / 6.0 + 8.0 / 72.0 + TERM), "{q:?}");
}

// ---- faces: the machine's, the twins and the stand-ins ------------------------------------------------------------------------------

/// Whether a cell that holds `formula` on `shape` carries that a stand-in measured it (`CellEval::stand_in`), in the faces `fonts`.
fn flagged(shape: ShapeBuilder, formula: &str, fonts: Fonts) -> bool {
    let (doc, at) = document(shape, formula, ("1", "1"));
    let mut ctx = EvalCtx::new(options(fonts), Budget::default());
    Evaluator::new(&doc, &mut ctx).evaluate_cell(&at).stand_in
}

fn faces(names: &[&str]) -> Fonts {
    Fonts::new(db_of(&names.iter().map(|n| SynthFont::new(n).ascii(1000)).collect::<Vec<_>>()))
}

fn in_font(font: &str, text: &str) -> ShapeBuilder {
    shape("", 12.0).row_cell("Character", None, RowKey::Ix(0), "Font", font).text_parts(parts(text))
}

/// A name the database holds is measured in it and the cell is not flagged; the name of a face whose open twin the database holds
/// (Calibri, Carlito) is measured in the twin and is not flagged; a name with no twin and no face is measured in a stand-in and flagged. The
/// flag is the cell's: the oracle leaves a flagged cell out of the score and says so.
#[test]
fn a_stand_in_that_is_not_the_twin_flags_the_cell() {
    let width = |font: &str, fonts: Fonts| {
        flagged(
            in_font(
                font, "abc
",
            ),
            "TEXTWIDTH(TheText)",
            fonts,
        )
    };
    assert!(!width("Em", faces(&["Em"])), "the named face");
    assert!(!width("Calibri", faces(&["Carlito"])), "its open twin");
    assert!(!width("Arial", faces(&["Liberation Sans"])), "its open twin");
    assert!(width("Calibri", faces(&["Mystery"])), "another face stands in");
    assert!(width("Calibri Light", faces(&["Carlito"])), "the nearest face, with no claim that the widths match");
    assert!(width("Nothing", faces(&["Em"])), "a name with no twin");
    // A repeat is the same cell: the flag is a fact of the cell and not a count of evaluations, so a second evaluation says the same.
    let (doc, at) = document(in_font("Calibri", "abc\n"), "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(options(faces(&["Mystery"])), Budget::default());
    for _ in 0..3 {
        ctx.invalidate();
        assert!(Evaluator::new(&doc, &mut ctx).evaluate_cell(&at).stand_in);
    }
}

/// A run that names no font draws in the database's default, and a bold the face lacks is made up by the renderer: neither is the original's width,
/// though the layout notes no stand-in for either (the text crate notes a name that is given, and not a made-up weight). Empty text has no letters
/// and is never flagged.
#[test]
fn a_run_with_no_font_and_a_made_up_bold_flag_the_cell() {
    let em = || faces(&["Em"]);
    let no_font = ShapeBuilder::new(1).text_parts(parts("abc\n")).row_cell("Character", None, RowKey::Ix(0), "Size", &pt(12.0).to_string()).row_cell(
        "Paragraph",
        None,
        RowKey::Ix(0),
        "SpLine",
        "-1.2",
    );
    assert!(flagged(no_font, "TEXTWIDTH(TheText)", em()), "no font name at all");
    let zero = in_font("0", "abc\n");
    assert!(flagged(zero, "TEXTWIDTH(TheText)", em()), "a font of 0 is none");
    // `Em` has a regular face only: a bold run is the regular emboldened by the renderer.
    let bold = in_font("Em", "abc\n").row_cell("Character", None, RowKey::Ix(0), "Style", "1");
    assert!(flagged(bold, "TEXTWIDTH(TheText)", em()), "a bold the face lacks");
    let italic = in_font("Em", "abc\n").row_cell("Character", None, RowKey::Ix(0), "Style", "2");
    assert!(flagged(italic, "TEXTWIDTH(TheText)", em()), "an italic the face lacks");
    let plain = in_font("Em", "abc\n");
    assert!(!flagged(plain, "TEXTWIDTH(TheText)", em()));
    // Empty text: the margins alone, whatever the faces.
    assert!(!flagged(in_font("Nothing", ""), "TEXTWIDTH(TheText)", em()));
    assert!(!flagged(shape("", 12.0).text_parts(parts("")), "TEXTHEIGHT(TheText,4 in)", faces(&["Mystery"])));
}

/// A `TEXTHEIGHT` is flagged only when the text wrapped: a text that fits on its lines in a wider face fits in the original's narrower one, so
/// the height of a one-line text does not depend on the face. A `TEXTWIDTH` always depends on it.
#[test]
fn a_height_is_flagged_only_when_the_text_wrapped() {
    let mystery = || faces(&["Mystery"]);
    // "aaa bbb ccc" is eleven em, 1.83 inch at 12 pt in a face of one em an advance.
    let text = || in_font("Calibri", "aaa bbb ccc");
    assert!(!flagged(text(), "TEXTHEIGHT(TheText,4 in)", mystery()), "one line");
    assert!(!flagged(text(), "TEXTHEIGHT(TheText)", mystery()), "no limit");
    assert!(flagged(text(), "TEXTHEIGHT(TheText,1 in)", mystery()), "it wraps");
    assert!(flagged(text(), "TEXTWIDTH(TheText)", mystery()), "a width is the faces'");
    assert!(!flagged(text(), "TEXTHEIGHT(TheText,1 in)", faces(&["Carlito"])), "wrapped in the twin: not flagged");
}

/// The flag goes to the cells that read the measure (as the clock's does): a cell that reads a flagged `TxtWidth`, in the live mode where it is
/// worked out, is flagged, and a cell that reads an unflagged one is not.
#[test]
fn a_cell_that_reads_a_flagged_measure_is_flagged() {
    use diacraft_doc::{Cell, Value as DocValue};
    let build = |font: &str| {
        shape("", 12.0)
            .row_cell("Character", None, RowKey::Ix(0), "Font", font)
            .text_parts(parts("abc\n"))
            .set(CellAddr::in_row("User", None, RowKey::name("W"), "Value"), Cell::with_formula("TEXTWIDTH(TheText)", Some(DocValue::from_text("1"))))
    };
    for (font, expected) in [("Em", false), ("Nothing", true)] {
        let (doc, at) = document(build(font), "User.W*2", ("1", "1"));
        let mut ctx = EvalCtx::new(options(faces(&["Em"])), Budget::default());
        let outcome = Evaluator::new(&doc, &mut ctx).evaluate_cell(&at);
        assert_eq!(outcome.stand_in, expected, "{font}: the reader of a measure");
    }
}

/// With no database given, the bundled faces measure: the same on every machine, and a name without a twin among them is a flagged stand-in.
#[test]
fn without_a_database_the_bundled_faces_measure() {
    let (doc, at) = document(shape("abc\n", 12.0).row_cell("Character", None, RowKey::Ix(0), "Font", "Calibri"), "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let outcome = Evaluator::new(&doc, &mut ctx).evaluate_cell(&at);
    let Value::Number(q) = outcome.raw else { panic!("not a number") };
    // Margins alone are 0.111; the text is on top of that and wider than the empty text.
    assert!(q.stored > 8.0 / 72.0 + 0.2, "{q:?}");
    assert!(outcome.stand_in, "Calibri has no twin among the bundled faces");
    // The same document twice gives the same number.
    let mut again = EvalCtx::new(EvalOptions::default(), Budget::default());
    let Value::Number(r) = Evaluator::new(&doc, &mut again).evaluate(&at) else { panic!("not a number") };
    assert_eq!(q.stored, r.stored);
}

/// The faces of the machine, as the text crate's own tests ask for them: a face that has the letters, as the regular face of its family.
fn machine() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::system_and_bundled)
}

fn skipped(why: &str) {
    eprintln!("SKIPPED: {why}");
}

/// On a real face: the width is the sum of the face's own nominal advances, with no kerning, plus the margins and the space.
#[test]
fn on_a_real_face_the_width_is_the_sum_of_its_advances() {
    let letters: Vec<char> = "Hello, world".chars().collect();
    let Some(found) = machine().covering(&letters) else { return skipped("no face has the letters of the test") };
    let face = machine().pick(found.family(), FontStyle::default());
    if !letters.iter().all(|c| face.covers(*c)) {
        return skipped(&format!("{} has the letters only in a style other than the regular", found.family()));
    }
    let size = pt(11.0);
    let sum: f64 = letters.iter().map(|c| face.advance(face.glyph_for(*c)) / face.units_per_em() * size).sum();
    let space = face.advance(face.glyph_for(' ')) / face.units_per_em() * size;
    let family = face.family().to_owned();
    let on_machine = Fonts::shared(Arc::new(FontDb::system_and_bundled()));
    let s = ShapeBuilder::new(1)
        .text_parts(parts("Hello, world\n"))
        .row_cell("Character", None, RowKey::Ix(0), "Font", &family)
        .row_cell("Character", None, RowKey::Ix(0), "Size", &size.to_string())
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
        .cell("LeftMargin", "0.1")
        .cell("RightMargin", "0.1");
    let Value::Number(q) = run(s, "TEXTWIDTH(TheText)", on_machine) else { panic!("not a number") };
    assert!((q.stored - (sum + space + 0.2 + TERM)).abs() < 1e-6, "{family}: {} against {}", q.stored, sum + space + 0.2 + TERM);
}

/// The twin of Calibri, where the machine has it: measured in Carlito, counting nothing. Where it has not, the bundled stand-in is counted and
/// the test says so.
#[test]
fn calibri_is_measured_in_carlito_where_the_machine_has_it() {
    if !machine().has_family("Carlito") {
        return skipped("no Carlito: Calibri is measured in the bundled stand-in, which is counted, and wider by 6 to 11 percent");
    }
    let carlito = machine().pick("Carlito", FontStyle::default());
    let letters: Vec<char> = "Hello".chars().collect();
    if !letters.iter().all(|c| carlito.covers(*c)) {
        return skipped("Carlito lacks Latin letters");
    }
    let size = pt(10.0);
    let sum: f64 = letters.iter().map(|c| carlito.advance(carlito.glyph_for(*c)) / carlito.units_per_em() * size).sum();
    let s = ShapeBuilder::new(1).text_parts(parts("Hello")).row_cell("Character", None, RowKey::Ix(0), "Font", "Calibri").row_cell(
        "Character",
        None,
        RowKey::Ix(0),
        "Size",
        &size.to_string(),
    );
    let on_machine = Fonts::shared(Arc::new(FontDb::system_and_bundled()));
    let (doc, at) = document(s, "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(options(on_machine), Budget::default());
    let outcome = Evaluator::new(&doc, &mut ctx).evaluate_cell(&at);
    let Value::Number(q) = outcome.raw else { panic!("not a number") };
    assert!((q.stored - (sum + TERM)).abs() < 1e-6, "{} against {}", q.stored, sum + TERM);
    assert!(!outcome.stand_in, "Carlito is Calibri's twin");
}

// ---- the oracle ---------------------------------------------------------------------------------------------------------------------------

/// A `TxtWidth` that calls `TEXTWIDTH`, loaded and not re-evaluated, is the width the file saved; evaluated, it is
/// this machine's measure. And the real-file salt still fails an echo of these cells.
#[test]
fn the_real_salt_passes_the_text_measures_and_fails_an_echo() {
    use diacraft_calc::{Calculator, CellKind, OperandMode, SaltPolicy, recalc_all_salted};
    use diacraft_doc::{Cell, Value as DocValue};

    let m = 8.0 / 72.0;
    let user = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    let cell = |f: &str, v: f64| Cell::with_formula(f, Some(DocValue::from_text(&v.to_string())));
    let s = shape("abc\n", 12.0)
        .set(user("W"), cell("TEXTWIDTH(TheText)", 0.5 + 1.0 / 6.0 + m))
        .set(user("H"), cell("TEXTHEIGHT(TheText,4 in)", 0.2 + m))
        .set(user("Limited"), cell("TEXTWIDTH(TheText,1 in)", 0.5 + 1.0 / 6.0 + m));
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
    let opts = || EvalOptions { operands: OperandMode::Cached, fonts: Some(em()), ..EvalOptions::default() };
    let policy = SaltPolicy { seed: 3, one_in: 1 };
    let r = recalc_all_salted(&doc, &Budget::default(), opts(), &mut diacraft_calc::Native, Some(policy));
    assert_eq!(r.agreed, 3, "{r:?}");
    assert!(r.real_salt.passed() && r.real_salt.salted == r.real_salt.caught && r.real_salt.salted >= 3, "{:?}", r.real_salt);

    struct Echo;
    impl Calculator for Echo {
        fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<diacraft_calc::CellEval> {
            let mut e = (*ev.evaluate_cell(at)).clone();
            if e.kind == CellKind::Formula {
                if let Some(saved) = e.saved.clone() {
                    e.raw = saved.clone();
                    e.view = saved;
                }
                e.error = e.saved_error;
                e.unsupported = None;
            }
            Arc::new(e)
        }
    }
    let echo = recalc_all_salted(&doc, &Budget::default(), opts(), &mut Echo, Some(policy));
    assert_eq!((echo.real_salt.salted, echo.real_salt.missed), (r.real_salt.salted, r.real_salt.salted), "{:?}", echo.real_salt);
    // Measured in a face that is not `Em`, the same cells differ: the calculation depends on the faces, not on what the file saved.
    let other =
        EvalOptions { operands: OperandMode::Cached, fonts: Some(Fonts::new(db_of(&[SynthFont::new("Em").ascii(500)]))), ..EvalOptions::default() };
    let r = recalc_all_salted(&doc, &Budget::default(), other, &mut diacraft_calc::Native, None);
    assert_eq!(r.differed.len(), 2, "TEXTWIDTH changes with the face and TEXTHEIGHT does not: {r:?}");
}

/// The oracle compares a text measure within 1E-4, as the file's own measure runs up to that much over the layout's
/// ("plus at most 1E-4"), and compares the other cells to 1E-9. A `TxtHeight` 5E-5 over agrees and 2E-4 over does not.
#[test]
fn the_oracle_compares_a_text_measure_within_a_ten_thousandth() {
    use diacraft_calc::{OperandMode, recalc_all_with};
    use diacraft_doc::{Cell, Value as DocValue};

    let measure = 0.2 + 8.0 / 72.0;
    let saved = |over: f64| {
        let s = shape("a", 12.0)
            .set(
                CellAddr::in_row("User", None, RowKey::name("H"), "Value"),
                Cell::with_formula("TEXTHEIGHT(TheText,4 in)", Some(DocValue::from_text(&(measure + over).to_string()))),
            )
            .set(
                CellAddr::in_row("User", None, RowKey::name("Plain"), "Value"),
                Cell::with_formula("1+1", Some(DocValue::from_text(&(2.0 + over).to_string()))),
            );
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
        let opts = EvalOptions { operands: OperandMode::Cached, fonts: Some(em()), ..EvalOptions::default() };
        recalc_all_with(&doc, &Budget::default(), opts)
    };
    let near_enough = saved(5e-5);
    assert_eq!(near_enough.differed.iter().map(|d| d.cell.as_str()).collect::<Vec<_>>(), ["User.Plain.Value"], "{near_enough:?}");
    let too_far = saved(2e-4);
    assert_eq!(too_far.differed.len(), 2, "{too_far:?}");
}

/// As the oracle reads it: a cell measured in a stand-in that is not the name's twin is left out of the score, under its own reason,
/// and counted once for the cell (a salt rerun, a repeat or a second reading of the cell does not count it again); one measured in a twin is
/// scored. The cell that reads a flagged measure is left out with it.
#[test]
fn the_oracle_leaves_out_a_cell_measured_in_a_stand_in_and_says_so() {
    use diacraft_calc::{OperandMode, SaltPolicy, recalc_all_salted, recalc_all_with};
    use diacraft_doc::{Cell, Value as DocValue};

    let build = |font: &str| {
        let s = in_font(font, "abc\n")
            .set(
                CellAddr::in_row("User", None, RowKey::name("W"), "Value"),
                Cell::with_formula("TEXTWIDTH(TheText)", Some(DocValue::from_text(&(4.0 / 6.0 + 8.0 / 72.0).to_string()))),
            )
            .set(CellAddr::in_row("User", None, RowKey::name("Twice"), "Value"), Cell::with_formula("User.W*2", Some(DocValue::from_text("1"))))
            .set(CellAddr::in_row("User", None, RowKey::name("Plain"), "Value"), Cell::with_formula("1+1", Some(DocValue::from_text("2"))));
        DocBuilder::new().page(1, PageBuilder::new().shape(s)).build()
    };
    let live = |fonts: Fonts| EvalOptions { operands: OperandMode::Live, fonts: Some(fonts), ..EvalOptions::default() };
    // In the named face: scored, all three, and `Twice` differs from its stale 1.
    let named = recalc_all_with(&build("Em"), &Budget::default(), live(faces(&["Em"])));
    assert_eq!((named.skipped.text_stand_in, named.agreed, named.differed.len()), (0, 2, 1), "{named:?}");
    // In a stand-in: the measure and its reader are left out, once each, and the plain cell is scored.
    let stand_in = recalc_all_with(&build("Calibri"), &Budget::default(), live(faces(&["Mystery"])));
    assert_eq!((stand_in.skipped.text_stand_in, stand_in.agreed, stand_in.differed.len()), (2, 1, 0), "{stand_in:?}");
    assert_eq!(stand_in.accounted(), stand_in.cells, "every cell is in one place: {stand_in:?}");
    // A salted run counts the same cells: its reruns put wrong values right out and work the cell again, and the count does not move.
    let policy = SaltPolicy { seed: 1, one_in: 1 };
    let salted = recalc_all_salted(&build("Calibri"), &Budget::default(), live(faces(&["Mystery"])), &mut diacraft_calc::Native, Some(policy));
    assert_eq!(salted.skipped.text_stand_in, 2, "{salted:?}");
    // In the twin: scored again.
    let twin = recalc_all_with(&build("Calibri"), &Budget::default(), live(faces(&["Carlito"])));
    assert_eq!(twin.skipped.text_stand_in, 0, "{twin:?}");
}

/// H1: a text that names a different `Tabs` row at every letter reads the section once, not once for each marker: the steps are linear in the
/// markers and the rows, where they were their product (65,535 markers over a 320,000-row section were two billion merges).
#[test]
fn a_flood_of_tab_markers_over_a_big_tabs_section_is_read_in_one_look() {
    let flood = |n: u32| {
        let mut p = vec![TextPart::Cp(0), TextPart::Pp(0)];
        let mut s = shape("", 12.0);
        for i in 0..n {
            p.push(TextPart::Tp(i));
            p.push(TextPart::Chars(Arc::from("a")));
            s = s.row_cell("Tabs", None, RowKey::Ix(i), "Position0", "0.5").row_cell("Tabs", None, RowKey::Ix(i), "Alignment0", "0");
        }
        s.text_parts(p)
    };
    // The document model counts the rows it merges: the cost the steps cannot show, as the merge is not charged. A control first, that the count
    // moves with the looks (asking for a ten-row section 50 times is 500 rows merged or more); then the flood's own.
    let (control, at) = document(flood(10), "TEXTWIDTH(TheText)", ("1", "1"));
    {
        let resolver = diacraft_doc::Resolver::new(&control);
        let sheet = at.sheet.clone();
        diacraft_doc::work::reset();
        for _ in 0..50 {
            let _ = resolver.effective_rows(&sheet, &diacraft_doc::SectionKey::new("Tabs", None));
        }
        assert!(diacraft_doc::work::rows_merged() >= 500, "the instrument sees rows merged: {}", diacraft_doc::work::rows_merged());
    }
    diacraft_doc::work::reset();
    let (large_doc, large_at) = document(flood(2000), "TEXTWIDTH(TheText)", ("1", "1"));
    diacraft_doc::work::reset();
    let mut ctx = EvalCtx::new(options(em()), Budget::default());
    let _ = Evaluator::new(&large_doc, &mut ctx).evaluate(&large_at);
    let merged = diacraft_doc::work::rows_merged();
    assert!(merged < 5 * 2000, "2,000 markers over 2,000 rows merged {merged} rows: the section once for the text, not once for each marker");
    let (small_value, small) = run_scaled(flood(1000), "TEXTWIDTH(TheText)", em(), ("1", "1"));
    let (large_value, large) = run_scaled(flood(2000), "TEXTWIDTH(TheText)", em(), ("1", "1"));
    assert!(matches!(small_value, Value::Number(_)) && matches!(large_value, Value::Number(_)), "{small_value:?} {large_value:?}");
    let ratio = large.steps as f64 / small.steps as f64;
    assert!((1.6..2.5).contains(&ratio), "steps {} for 1,000 markers and {} for 2,000", small.steps, large.steps);
    assert!(large.steps < 200 * 2000, "{} steps for 2,000 markers over 2,000 rows is a few for each", large.steps);
    // Each stop still reaches the layout: a row that names no tab section, and a marker past the rows, read nothing.
    let none = run_scaled(
        shape("", 12.0).text_parts(vec![TextPart::Cp(0), TextPart::Pp(0), TextPart::Tp(5), TextPart::Chars(Arc::from("a"))]),
        "TEXTWIDTH(TheText)",
        em(),
        ("1", "1"),
    );
    assert!(matches!(none.0, Value::Number(_)), "{:?}", none.0);
    // A budget the section is bigger than ends the run, bounded.
    let (doc, at) = document(flood(2000), "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(options(em()), Budget::with_steps(1000));
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}");
    assert!(ctx.stats.steps <= 1000, "{:?}", ctx.stats);
}

// ---- hostile text, and the cost of a layout to a count ---------------------------------------------------------------------------------

fn long_text(chars: usize) -> String {
    let word = "abcdefg ";
    word.repeat(chars / word.len() + 1).chars().take(chars).collect()
}

/// A layout costs steps in proportion to the characters, and the text crate's own counters say its work is linear in them too (the cluster steps
/// the line breaker takes, and the calls to the shaper).
#[test]
fn a_layout_costs_in_proportion_to_its_characters() {
    let cost = |chars: usize| -> (u64, u64) {
        diacraft_text::work::reset();
        let (v, stats) = run_scaled(shape(&long_text(chars), 10.0), "TEXTHEIGHT(TheText,3 in)", em(), ("1", "1"));
        assert!(matches!(v, Value::Number(_)), "{v:?}");
        (stats.steps, diacraft_text::work::cluster_steps())
    };
    let (small_steps, small_clusters) = cost(20_000);
    let (large_steps, large_clusters) = cost(40_000);
    let ratio = |a: u64, b: u64| b as f64 / a as f64;
    assert!((1.6..2.4).contains(&ratio(small_steps, large_steps)), "steps {small_steps} and {large_steps}");
    assert!((1.6..2.4).contains(&ratio(small_clusters, large_clusters)), "cluster steps {small_clusters} and {large_clusters}");
    assert!(large_clusters < 40 * 40_000, "{large_clusters} cluster steps for 40,000 characters");
}

/// A budget smaller than the text's charge ends the run before any of it is laid out: the text crate counts no work.
#[test]
fn a_budget_smaller_than_the_text_ends_the_run_before_the_layout() {
    diacraft_text::work::reset();
    let (doc, at) = document(shape(&long_text(200_000), 10.0), "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(options(em()), Budget::with_steps(10_000));
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR));
    assert!(ctx.stats.budget_exhausted && ctx.stats.steps <= 10_000, "{:?}", ctx.stats);
    assert_eq!(diacraft_text::work::cluster_steps(), 0, "nothing was laid out");
}

/// A text past what the layout takes is not measured (the layout would cut it, and the size would be the cut text's): the saved value stands.
#[test]
fn a_text_the_layout_would_cut_is_unsupported() {
    let v = run(shape(&long_text(diacraft_text::limits::MAX_CHARS + 10), 10.0), "TEXTWIDTH(TheText)", em());
    assert!(matches!(v, Value::Unsupported(_)), "{v:?}");
}

/// Many rows of formats: a text that names a different `Character` row at every letter reads each row once (a format is found once for a row),
/// and one that names more rows than a section may hold is refused.
#[test]
fn formats_are_read_once_for_a_row_and_a_flood_of_rows_is_refused() {
    let flood = |rows: u32| {
        let mut p = vec![TextPart::Pp(0)];
        for i in 0..rows {
            p.push(TextPart::Cp(i));
            p.push(TextPart::Chars(Arc::from("a")));
        }
        p
    };
    let few = run_scaled(shape("", 12.0).text_parts(flood(200)), "TEXTWIDTH(TheText)", em(), ("1", "1"));
    assert!(matches!(few.0, Value::Number(_)), "{:?}", few.0);
    // Rows that name nothing read the root: 200 distinct rows cost about 200 times the cells of one, and no more.
    let one = run_scaled(shape("", 12.0).text_parts(flood(1)), "TEXTWIDTH(TheText)", em(), ("1", "1"));
    assert!(few.1.steps < 250 * one.1.steps, "{} steps for 200 rows and {} for one", few.1.steps, one.1.steps);
    let (v, stats) = run_scaled(shape("", 12.0).text_parts(flood(70_000)), "TEXTWIDTH(TheText)", em(), ("1", "1"));
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{stats:?}");
}

/// Hostile formats do not panic: a size that is not a number, a negative or an enormous size, a width that is not a number or is huge, a
/// zero-width block, and cells that hold text where a number goes.
#[test]
fn hostile_formats_give_a_number_or_an_error_and_never_a_panic() {
    for size in ["-5", "1e300", "0", "nan", "goat", "1.#INF"] {
        let odd = shape("abc def\n", 12.0).row_cell("Character", None, RowKey::Ix(0), "Size", size);
        let v = run(odd, "TEXTWIDTH(TheText,2 in)", em());
        assert!(matches!(v, Value::Number(_) | Value::Error(_) | Value::Unsupported(_)), "{size}: {v:?}");
    }
    for limit in ["0", "-3", "1e300", "1/3"] {
        let v = run(shape("abc def\n", 12.0), &format!("TEXTHEIGHT(TheText,{limit})"), em());
        assert!(matches!(v, Value::Number(_) | Value::Error(_) | Value::Unsupported(_)), "{limit}: {v:?}");
    }
    let margins = |s: ShapeBuilder| s.cell("LeftMargin", "1e300").cell("RightMargin", "-1e300").cell("TopMargin", "goat");
    let v = run(margins(shape("abc\n", 12.0)), "TEXTHEIGHT(TheText,2 in)", em());
    assert!(matches!(v, Value::Number(_) | Value::Error(_) | Value::Unsupported(_)), "{v:?}");
    // Vertical text is laid out by the text crate: its measure is a number, as a hostile `TextDirection` is no reason to refuse one.
    let vertical = |s: ShapeBuilder| s.cell("TextDirection", "1");
    for formula in ["TEXTWIDTH(TheText)", "TEXTHEIGHT(TheText,2 in)"] {
        assert!(matches!(run(vertical(shape("abc\n", 12.0)), formula, em()), Value::Number(_)), "{formula}");
    }
}

/// `TextDirection` 1 (it): `TEXTWIDTH` and `TEXTHEIGHT` measure the text as it runs across, in the shape's own
/// axes, whichever way it is drawn: the width is the widest line and the left and right margins, and the height the stack of lines and the top and bottom
/// ones, as with `TextDirection` 0. The margins differ on each side, so a measure that turned them would show.
#[test]
fn vertical_text_measures_as_text_across_in_the_shapes_own_axes() {
    let margins = |s: ShapeBuilder| {
        s.cell("LeftMargin", &pt(1.0).to_string())
            .cell("RightMargin", &pt(2.0).to_string())
            .cell("TopMargin", &pt(3.0).to_string())
            .cell("BottomMargin", &pt(5.0).to_string())
    };
    let across = |text: &str| margins(shape(text, 12.0));
    let down = |text: &str| across(text).cell("TextDirection", "1");
    for text in ["", "abc\n", "abc\nde", "abc def ghi\n"] {
        for formula in ["TEXTWIDTH(TheText)", "TEXTWIDTH(TheText,1 in)", "TEXTHEIGHT(TheText,2 in)", "TEXTHEIGHT(TheText,0.6 in)"] {
            let (a, d) = (width(across(text), formula), width(down(text), formula));
            assert!(exactly(a, d), "{text:?} {formula}: across {a}, down {d}");
        }
    }
    // The numbers: three em along the line and a space for the final line feed, with the left and right margins; one line of 1.2 em, 0.2 inch at 12 pt,
    // with the top and bottom ones.
    let w = width(down("abc\n"), "TEXTWIDTH(TheText)");
    assert!(near(w, 0.5 + 1.0 / 6.0 + 3.0 / 72.0 + TERM), "{w}");
    let h = width(down("abc\n"), "TEXTHEIGHT(TheText,2 in)");
    assert!(near(h, 0.2 + 8.0 / 72.0 + TERM), "{h}");
    // Two paragraphs are two lines: the longer is the width, and the height is both.
    let w = width(down("abc\nde"), "TEXTWIDTH(TheText)");
    assert!(near(w, 0.5 + 3.0 / 72.0 + TERM), "{w}");
    let h = width(down("abc\nde"), "TEXTHEIGHT(TheText,2 in)");
    assert!(near(h, 0.4 + 8.0 / 72.0 + TERM), "{h}");
}

/// The form of the two sample files of (a master's shape with `TextDirection` 1, no text and margins 0, whose text block's width is the measure of
/// the text's height and whose height the measure of its width): the saved values were the width one line and the height 0, which the turned measure gave
/// the other way round. Empty text measures the margins alone across and one line down, whichever way it is drawn; a text with
/// characters measures as it does across.
#[test]
fn an_empty_vertical_text_block_sized_by_its_measures_is_one_line_wide_and_nothing_tall() {
    let block = |text: &str| {
        shape(text, 12.0)
            .cell("TextDirection", "1")
            .cell("LeftMargin", "0")
            .cell("RightMargin", "0")
            .cell("TopMargin", "0")
            .cell("BottomMargin", "0")
            .formula("TxtHeight", "TEXTWIDTH(TheText)", "0")
            .formula("TxtWidth", "TEXTHEIGHT(TheText,TxtHeight)", "0")
    };
    let empty = scaled_page(block(""), ("1", "1"));
    let (w, h) = (cell_value(&empty, "TxtWidth"), cell_value(&empty, "TxtHeight"));
    assert!(exactly(w, 0.2) && exactly(h, 0.0), "empty: TxtWidth {w} (one line), TxtHeight {h} (nothing)");
    // At a scale the answer is in drawing units, as across.
    let scaled = scaled_page(block(""), ("1", "4"));
    let (w, h) = (cell_value(&scaled, "TxtWidth"), cell_value(&scaled, "TxtHeight"));
    assert!(exactly(w, 0.8) && exactly(h, 0.0), "empty at 1:4: TxtWidth {w}, TxtHeight {h}");
    // A text of three em: its line is the height of the block, and its one line the width.
    let three = scaled_page(block("abc"), ("1", "1"));
    let (w, h) = (cell_value(&three, "TxtWidth"), cell_value(&three, "TxtHeight"));
    assert!(exactly(h, 0.5 + TERM), "abc: TxtHeight {h}");
    assert!(exactly(w, 0.2 + TERM), "abc: TxtWidth {w}");
}

// ----: what the formats copy ---------------------------------------------------------------------------------------------------

/// A text that names `rows` `Character` and `Paragraph` rows, each of which holds `len` bytes of text in its face names, its language, its bullet
/// font and its bullet string.
fn big_names(rows: u32, len: usize, name: &str) -> ShapeBuilder {
    let big: String = name.chars().cycle().take(len / name.len().max(1) * name.chars().count().max(1)).collect();
    let mut parts = Vec::new();
    let mut s = shape("", 12.0);
    for i in 0..rows {
        parts.push(TextPart::Pp(i));
        parts.push(TextPart::Cp(i));
        parts.push(TextPart::Chars(Arc::from("a")));
        for cell in ["Font", "AsianFont", "ComplexScriptFont", "LangID"] {
            s = s.row_cell("Character", None, RowKey::Ix(i), cell, &big);
        }
        for cell in ["BulletFont", "BulletStr"] {
            s = s.row_cell("Paragraph", None, RowKey::Ix(i), cell, &big);
        }
    }
    s.text_parts(parts)
}

/// A face name, a language, a bullet font and a bullet string are cut at 256 bytes (on a character) before they are copied into a
/// format, and what a format keeps is held until the measure is done: 100 more rows with 4 KB in each of these six cells hold about 1.3 KB more
/// each, 128 KB at least (five names at 256) and no more than 6 KB a row, where a copy of the whole cells would be 24 KB a row, and nothing was
/// held before. Operands as saved, so that the memo does not hold the cells' values and hide what the formats keep.
#[test]
fn the_names_a_format_copies_are_cut_and_held() {
    use diacraft_calc::OperandMode;

    let peak = |rows: u32, name: &str| {
        let (doc, at) = document(big_names(rows, 4096, name), "TEXTWIDTH(TheText)", ("1", "1"));
        let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..options(em()) }, Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        assert!(matches!(v, Value::Number(_)), "{rows} rows: {v:?}");
        assert_eq!(ctx.bytes_held(), 0, "everything held is let go when the measure is done");
        ctx.stats.bytes_peak
    };
    let (few, many) = (peak(100, "x"), peak(200, "x"));
    let growth = many - few;
    assert!(growth >= 100 * 5 * 256, "100 more rows held {growth} bytes: the copies are held ({few} and {many})");
    assert!(growth <= 100 * 6 * 1024, "100 more rows held {growth} bytes: the copies are cut at 256 bytes ({few} and {many})");
    // A name of characters that are three bytes each is cut on a character, not in the middle of one.
    let _ = peak(20, "\u{20AC}");
}

/// A measure that ends in an error lets go of what its formats held, so a run of many cells does not fill the memory a budget gives.
#[test]
fn the_formats_a_failed_measure_held_are_let_go() {
    use diacraft_calc::OperandMode;

    // 70,000 rows are refused part of the way; the formats made before that were held.
    let mut parts = vec![TextPart::Pp(0)];
    for i in 0..70_000u32 {
        parts.push(TextPart::Cp(i));
        parts.push(TextPart::Chars(Arc::from("a")));
    }
    let (doc, at) = document(shape("", 12.0).text_parts(parts), "TEXTWIDTH(TheText)", ("1", "1"));
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..options(em()) }, Budget::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}");
    assert!(ctx.stats.bytes_peak > 1_000_000, "the formats were held while they were made: {:?}", ctx.stats);
    assert_eq!(ctx.bytes_held(), 0, "and let go on the error: {:?}", ctx.stats);
}

// ----: faces found on demand ---------------------------------------------------------------------------------------------------

/// Faces a caller may not have yet (a scan of the machine's folders still going on a thread of its own) are asked for only by a measure: a
/// formula that measures nothing never asks, so a commit that sets no text does not wait for the scan, and a measure asks, and gets the faces it
/// was given.
#[test]
fn faces_given_on_demand_are_asked_for_only_when_a_text_is_measured() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let asked = Arc::new(AtomicUsize::new(0));
    let db = Arc::new(db_of(&[SynthFont::new("Em").ascii(1000).chars("\u{00A0}", 1000)]));
    let lazy = {
        let (asked, db) = (Arc::clone(&asked), Arc::clone(&db));
        Fonts::lazy(move || {
            asked.fetch_add(1, Ordering::SeqCst);
            Arc::clone(&db)
        })
    };
    let v = run(shape("abc\n", 12.0), "1+1", lazy.clone());
    assert!(matches!(v, Value::Number(_)), "{v:?}");
    assert_eq!(asked.load(Ordering::SeqCst), 0, "a formula that measures no text does not ask for the faces");
    let v = run(shape("abc\n", 12.0), "TEXTWIDTH(TheText)", lazy);
    let Value::Number(q) = v else { panic!("{v:?}") };
    assert!(near(q.stored, 0.5 + 1.0 / 6.0 + 8.0 / 72.0 + TERM), "the faces it was given measured: {q:?}");
    assert!(asked.load(Ordering::SeqCst) >= 1, "a measure asks for them");
}
