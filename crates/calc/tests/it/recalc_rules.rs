//! The rules settled from the first recalculation score, one test group for each, at the specs' vectors and at the values and
//! counts real files hold.
//!
//! The rules: the theme function's group step, a row named in another letter case,
//! the clipping path, a `SETATREF` before a colour, the theme
//! function's own rules and its table, and the three reasons
//! the oracle leaves a cell out under.

use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value, recalc_all};
use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::{E, mismatch, relationships};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];
/// The five and the three that go with them: what a group hands its members.
const EIGHT: [&str; 8] = [
    "ThemeIndex",
    "ColorSchemeIndex",
    "EffectSchemeIndex",
    "ConnectorSchemeIndex",
    "FontSchemeIndex",
    "VariationColorIndex",
    "VariationStyleIndex",
    "EmbellishmentIndex",
];

fn c(hex: &str) -> E {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    E::Col((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn expect(id: &str, got: Value, want: E) {
    assert_eq!(mismatch(&got, &want), None, "{id}: {got:?}");
}

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn cell(formula: &str, v: &str, unit: Option<&str>, e: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = unit {
        c = c.with_unit(u);
    }
    c.error = e.map(Arc::from);
    c
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn with_theme(mut doc: Document, themes: usize) -> Document {
    for _ in 0..themes {
        doc.themes.push(ThemeXml::default().part());
    }
    doc
}

fn evaluate_at(doc: &Document, owner: Owner, path: &[u32], addr: CellAddr) -> Value {
    let at = CellAddress::new(SheetRef::Shape(ShapeRef { owner, path: path.to_vec() }), addr);
    Evaluator::new(doc, &mut EvalCtx::default()).evaluate(&at)
}

// ---- item 1: the group step ----------------------------------------------------------------------------------------------------

/// A style holding each of `names` at `n`.
fn style_of(names: &[&str], n: &str) -> StyleBuilder {
    names.iter().fold(StyleBuilder::new(), |s, name| s.cell(name, n))
}

/// A shape holding each of `cells`.
fn holding(s: ShapeBuilder, cells: &[(&str, &str)]) -> ShapeBuilder {
    cells.iter().fold(s, |s, (name, v)| s.cell(name, v))
}

/// The cells of a group: the five at `n`, and `more`.
fn five_at<'a>(n: &'a str, more: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    FIVE.iter().map(|name| (*name, n)).chain(more.iter().copied()).collect()
}

/// The member (`ID` 2): it holds none of the five and its three style links reach style `style`; `held` are cells it holds.
fn member(style: u32, held: &[(&str, &str)], formula: &str) -> ShapeBuilder {
    holding(ShapeBuilder::new(2).links(Some(style), Some(style), Some(style)), held).row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        formula,
        "0",
    )
}

/// The styles of the vectors: style 2 gives 65534 in all eight cells (the `Theme` style), style 3 gives 0 in the five.
fn styles(doc: DocBuilder) -> DocBuilder {
    doc.style(2, style_of(&EIGHT, "65534")).style(3, style_of(&FIVE, "0"))
}

/// The world: a master whose page sheet holds no scheme cell, with `top` as its one shape (the group, and the member inside it, at
/// path `[0, 0]`), and the composed theme part (scheme 40) and a second (scheme 41, every slot black).
fn master_world(page_cells: &[(&str, &str)], top: ShapeBuilder) -> Document {
    let master = holding_master(MasterBuilder::new(), page_cells).shape(top);
    let mut doc = styles(DocBuilder::new()).master(1, master).build();
    doc.themes.push(ThemeXml::default().part());
    doc.themes.push(ThemeXml { scheme: 41, colours: ["000000"; 12], ..ThemeXml::default() }.part());
    doc
}

fn holding_master(m: MasterBuilder, cells: &[(&str, &str)]) -> MasterBuilder {
    cells.iter().fold(m, |m, (name, v)| m.cell(name, v))
}

fn on_member(doc: &Document) -> Value {
    evaluate_at(doc, Owner::Master(1), &[0, 0], crate::probe_addr())
}

fn probe_of(group_cells: &[(&str, &str)], formula: &str) -> Value {
    let group = holding(ShapeBuilder::new(1).group(), group_cells).child(member(2, &[], formula));
    on_member(&master_world(&[], group))
}

#[test]
fn themeval_a_member_reads_the_nearest_groups_scheme_cells_in_place_of_the_page_step() {
    // The group holds all five at 40 and `VariationColorIndex` 0, the member none, on a chain that gives 65534, and the page holds
    // none: the composed theme answers, with a default or without, where MS-VSDX's page step alone reads no theme (`#ab9ac0`, `#090807`).
    let group = five_at("40", &[("VariationColorIndex", "0")]);
    expect("name", probe_of(&group, r#"THEMEVAL("accentcolor3")"#), c("214365"));
    expect("default", probe_of(&group, r#"THEMEVAL("accentcolor3",RGB(9,8,7))"#), c("214365"));
    // A group that holds none either: the page step finds nothing, so no theme: the table, and the default.
    expect("no group cells, name", probe_of(&[], r#"THEMEVAL("accentcolor3")"#), c("ab9ac0"));
    expect("no group cells, default", probe_of(&[], r#"THEMEVAL("accentcolor3",RGB(9,8,7))"#), c("090807"));
    // The same group and a member whose own chain gives 0: the root style stands and the group plays no part. A group step that ignored the
    // member's chain gives `#214365` here.
    let zero = holding(ShapeBuilder::new(1).group(), &group).child(member(3, &[], r#"THEMEVAL("accentcolor3")"#));
    expect("chain 0, name", on_member(&master_world(&[], zero)), c("ab9ac0"));
    let zero = holding(ShapeBuilder::new(1).group(), &group).child(member(3, &[], r#"THEMEVAL("accentcolor3",RGB(9,8,7))"#));
    expect("chain 0, default", on_member(&master_world(&[], zero)), c("090807"));
}

#[test]
fn a_member_holding_some_of_the_five_cells_is_left_to_its_own_cells_and_chain() {
    // "a member holding some of the five cells but not all is also unknown, and our rule leaves it to its own cells and chain". The
    // member holds `ColorSchemeIndex` 40 itself; the other four come from its chain as 65534 and read the page, which holds none: 0, no theme. A
    // group step applied to the four it does not hold would give `#214365`.
    let group =
        holding(ShapeBuilder::new(1).group(), &five_at("40", &[])).child(member(2, &[("ColorSchemeIndex", "40")], r#"THEMEVAL("accentcolor3")"#));
    expect("some", on_member(&master_world(&[], group)), c("ab9ac0"));
    // One cell held, any one of the five, is enough.
    for held in FIVE {
        let group = holding(ShapeBuilder::new(1).group(), &five_at("40", &[])).child(member(2, &[(held, "65534")], r#"THEMEVAL("accentcolor3")"#));
        expect(held, on_member(&master_world(&[], group)), c("ab9ac0"));
    }
    // Holding all five, a member reads its own: 41 is the second part, black.
    let all = five_at("41", &[]);
    let group = holding(ShapeBuilder::new(1).group(), &five_at("40", &[])).child(member(2, &all, r#"THEMEVAL("accentcolor3")"#));
    expect("all five", on_member(&master_world(&[], group)), c("000000"));
}

#[test]
fn a_group_counts_only_when_it_holds_all_five_as_numbers_other_than_0_and_65534() {
    let q = r#"THEMEVAL("accentcolor3")"#;
    // Four of the five.
    let four: Vec<(&str, &str)> = five_at("40", &[]).into_iter().filter(|(n, _)| *n != "FontSchemeIndex").collect();
    expect("four of five", probe_of(&four, q), c("ab9ac0"));
    // One at 0, or one at 65534, is no scheme: not a group that selects a theme.
    for bad in ["0", "65534"] {
        let mut cells = five_at("40", &[]);
        cells[3].1 = bad;
        expect(bad, probe_of(&cells, q), c("ab9ac0"));
    }
    // A cell that is not a number does not count as held.
    let mut cells = five_at("40", &[]);
    cells[2].1 = "Themed";
    expect("not a number", probe_of(&cells, q), c("ab9ac0"));
}

#[test]
fn the_nearest_enclosing_group_that_qualifies_gives_its_values_and_a_group_that_does_not_is_passed_over() {
    let q = r#"THEMEVAL("accentcolor3")"#;
    let nest = |outer: &[(&str, &str)], inner: &[(&str, &str)]| {
        let inner = holding(ShapeBuilder::new(5).group(), inner).child(member(2, &[], q));
        let outer = holding(ShapeBuilder::new(1).group(), outer).child(inner);
        // The member is at [0, 0, 0] here.
        let master = MasterBuilder::new().shape(outer);
        let mut doc = styles(DocBuilder::new()).master(1, master).build();
        doc.themes.push(ThemeXml::default().part());
        doc.themes.push(ThemeXml { scheme: 41, colours: ["000000"; 12], ..ThemeXml::default() }.part());
        evaluate_at(&doc, Owner::Master(1), &[0, 0, 0], crate::probe_addr())
    };
    // The inner group qualifies: its 41 is read, the outer group's 40 is not.
    expect("nearest", nest(&five_at("40", &[]), &five_at("41", &[])), c("000000"));
    // The inner group holds one of the five at 0: passed over, and the outer group's 40 is read.
    let mut broken = five_at("41", &[]);
    broken[0].1 = "0";
    expect("passed over", nest(&five_at("40", &[]), &broken), c("214365"));
    // The inner group holds none: the outer.
    expect("none inside", nest(&five_at("40", &[]), &[]), c("214365"));
    // Neither: the page step, and the page holds none.
    expect("neither", nest(&[], &[]), c("ab9ac0"));
}

#[test]
fn the_enclosing_groups_are_tried_before_a_page_that_holds_the_cells() {
    // "our rule tries the enclosing groups before the page, nearest first". The page sheet holds 41 (black), the group 40.
    let q = r#"THEMEVAL("accentcolor3")"#;
    let page = five_at("41", &[]);
    let grouped = holding(ShapeBuilder::new(1).group(), &five_at("40", &[])).child(member(2, &[], q));
    expect("group first", on_member(&master_world(&page, grouped)), c("214365"));
    // A group that holds none: the page.
    let grouped = holding(ShapeBuilder::new(1).group(), &[]).child(member(2, &[], q));
    expect("page after", on_member(&master_world(&page, grouped)), c("000000"));
    // A shape that is in no group reads the page (path of one).
    let top = member(2, &[], q);
    let doc = master_world(&page, top);
    expect("top level", evaluate_at(&doc, Owner::Master(1), &[0], crate::probe_addr()), c("000000"));
}

#[test]
fn a_member_on_a_page_reads_its_group_as_a_member_of_a_master_does() {
    // The group step is the same on a page's shapes.
    let group = holding(ShapeBuilder::new(1).group(), &five_at("40", &[])).child(member(2, &[], r#"THEMEVAL("accentcolor3")"#));
    let mut doc = styles(DocBuilder::new()).page(1, PageBuilder::new().shape(group)).build();
    doc.themes.push(ThemeXml::default().part());
    expect("page", evaluate_at(&doc, Owner::Page(1), &[0, 0], crate::probe_addr()), c("214365"));
    // Three levels down.
    let deep = holding(ShapeBuilder::new(1).group(), &five_at("40", &[]))
        .child(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6).group().child(member(2, &[], r#"THEMEVAL("accentcolor3")"#))));
    let mut doc = styles(DocBuilder::new()).page(1, PageBuilder::new().shape(deep)).build();
    doc.themes.push(ThemeXml::default().part());
    expect("deep", evaluate_at(&doc, Owner::Page(1), &[0, 0, 0, 0], crate::probe_addr()), c("214365"));
}

#[test]
fn the_variation_and_embellishment_cells_come_from_the_group_where_it_holds_them() {
    // "for the five and for VariationColorIndex, VariationStyleIndex and EmbellishmentIndex, wherever the group holds them". The
    // composed part's variant 2 has `040506` second and variant 0 `223344`; embellishment is 3 for style variant 2 and 2 for variant 0.
    let second = "THEMEVAL(201)";
    expect("variant 2", probe_of(&five_at("40", &[("VariationColorIndex", "2")]), second), c("040506"));
    expect("none held: variant 0", probe_of(&five_at("40", &[]), second), c("223344"));
    let prop = r#"THEMEPROP("Embellishment")"#;
    expect("embellishment held", probe_of(&five_at("40", &[("EmbellishmentIndex", "7")]), prop), E::N(7.0));
    expect("style variant 2", probe_of(&five_at("40", &[("VariationStyleIndex", "2")]), prop), E::N(3.0));
    expect("style variant 0", probe_of(&five_at("40", &[]), prop), E::N(2.0));
    // The group gives the member's `ISTHEMED` its colour scheme too (reads the cell).
    expect("ISTHEMED", probe_of(&five_at("40", &[]), "ISTHEMED()"), E::B(true));
    expect("ISTHEMED, no group cells", probe_of(&[], "ISTHEMED()"), E::B(false));
}

#[test]
fn a_member_whose_chain_gives_65534_in_a_group_the_oracle_scores_by_the_groups_values() {
    // The oracle's walk: the member's cell holds the composed colour `#214365`, which the group step gives and the page step does not.
    let group = holding(ShapeBuilder::new(1).group(), &five_at("40", &[]))
        .child(member(2, &[], "0").set(user("C"), cell(r#"THEMEVAL("accentcolor3")"#, "#214365", None, None)));
    let mut doc = styles(DocBuilder::new()).page(1, PageBuilder::new().shape(group)).build();
    doc.themes.push(ThemeXml::default().part());
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.agreed, r.differed.len()), (2, 0), "{r:?}");
}

// ---- item 2: a row named in another letter case --------------------------------------------------------------------------------------

fn shape(id: u32) -> ShapeBuilder {
    ShapeBuilder::new(id)
}

fn rows(s: ShapeBuilder, rows: &[(&str, &str)]) -> ShapeBuilder {
    rows.iter().fold(s, |s, (name, v)| s.row_cell("User", None, RowKey::name(name), "Value", v))
}

fn list_member(rows_held: &[(&str, &str)]) -> Vec<ShapeBuilder> {
    vec![
        shape(1).formula("Relationships", &relationships(&[(5, &[2])]), "0"),
        rows(shape(2).formula("Relationships", &relationships(&[(2, &[1])]), "0"), rows_held),
    ]
}

#[test]
fn a_row_named_in_another_letter_case_is_found_through_a_list_call() {
    // A `User` row `Tally9` of 7, named `TALLY9` through `LISTSHEETREF()`. An exact match gives the `IFERROR` fallback.
    let f = "IFERROR(LISTSHEETREF()!User.TALLY9,-1)";
    expect("V56", crate::eval_page(list_member(&[("Tally9", "7")]), 0, f), E::Any(7.0));
    // Through `CONTAINERSHEETREF`, and through the plain `Sheet.<id>!` and the shape's own sheet (every reference).
    let container = vec![shape(1).formula("Relationships", &relationships(&[(4, &[2])]), "0"), rows(shape(2), &[("Tally9", "7")])];
    expect("container", crate::eval_page(container, 0, "IFERROR(CONTAINERSHEETREF(1)!User.tally9,-1)"), E::Any(7.0));
    expect("Sheet.2!", crate::eval_page(vec![shape(1), rows(shape(2), &[("Tally9", "7")])], 0, "Sheet.2!User.TALLY9"), E::Any(7.0));
    expect("own sheet", crate::eval_page(vec![rows(shape(1), &[("Tally9", "7")])], 0, "User.tally9"), E::Any(7.0));
    // The name alone as `Prop.Cost` reads the row's `Value`, in any case, and the cell after the row too.
    let prop = |s: ShapeBuilder| s.row_cell("Property", None, RowKey::name("Cost"), "Value", "3");
    expect("Prop", crate::eval_page(vec![prop(shape(1))], 0, "Prop.COST"), E::Any(3.0));
    expect("Prop cell", crate::eval_page(vec![prop(shape(1))], 0, "Prop.cost.Value"), E::Any(3.0));
    // A row that is not there in any case is still `#REF!`, and a cell's name still matches exactly.
    expect("lacking", crate::eval_page(list_member(&[("Tally9", "7")]), 0, "LISTSHEETREF()!User.Lacking"), E::Err(CellError::Ref));
    expect("cell name", crate::eval_page(vec![prop(shape(1))], 0, "Prop.Cost.VALUE"), E::Err(CellError::Ref));
}

#[test]
fn the_exact_spelling_wins_and_then_the_lower_row() {
    // Rows `Tally9` 7 and `TALLY9` 9 on the shape: 9 for `User.TALLY9`, and 7 for `User.tally9`, by the lower `IX` with `Tally9` first.
    let both = |first: (&'static str, &'static str), second: (&'static str, &'static str)| {
        move |f: &str| crate::eval_page(vec![rows(shape(1), &[first, second])], 0, f)
    };
    let tally_first = both(("Tally9", "7"), ("TALLY9", "9"));
    expect("exact TALLY9", tally_first("User.TALLY9"), E::Any(9.0));
    expect("exact Tally9", tally_first("User.Tally9"), E::Any(7.0));
    expect("no exact, lower IX", tally_first("User.tally9"), E::Any(7.0));
    // The other way round: the lower row is `TALLY9`, and it wins for a spelling that is neither.
    let upper_first = both(("TALLY9", "9"), ("Tally9", "7"));
    expect("no exact, lower IX, upper first", upper_first("User.tally9"), E::Any(9.0));
    expect("exact Tally9, upper first", upper_first("User.Tally9"), E::Any(7.0));
}

#[test]
fn a_row_of_the_masters_shape_is_found_in_another_case_and_the_exact_spelling_is_tried_over_the_whole_chain_first() {
    let build = |instance: ShapeBuilder| {
        let master = MasterBuilder::new().shape(rows(shape(5), &[("Tally9", "7")]));
        DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build()
    };
    let probe = |instance: ShapeBuilder, f: &str| {
        let instance = instance.row_formula("User", None, RowKey::name("Probe"), "Value", f, "0");
        evaluate_at(&build(instance), Owner::Page(1), &[0], crate::probe_addr())
    };
    expect("inherited, other case", probe(shape(10).master(1), "User.TALLY9"), E::Any(7.0));
    // The instance's own `TALLY9` is a row of another spelling than the master's `Tally9`: `User.Tally9` is the master's, exactly, and
    // `User.TALLY9` is the instance's own.
    let own = |s: ShapeBuilder| rows(s.master(1), &[("TALLY9", "9")]);
    expect("exact, in the master", probe(own(shape(10)), "User.Tally9"), E::Any(7.0));
    expect("exact, local", probe(own(shape(10)), "User.TALLY9"), E::Any(9.0));
    expect("neither exact: the nearer sheet", probe(own(shape(10)), "User.tally9"), E::Any(9.0));
}

#[test]
fn the_oracle_scores_a_row_in_another_case_as_the_file_saved_it() {
    // 12,486 cells (128 files, 44 sources) took the `IFERROR` fallback and differed from the saved 7.
    let mut shapes = list_member(&[("Tally9", "7")]);
    let first = shapes.remove(0).set(user("T"), cell("IFERROR(LISTSHEETREF()!User.TALLY9,-1)", "7", None, None));
    shapes.insert(0, first);
    let mut page = PageBuilder::new();
    for s in shapes {
        page = page.shape(s);
    }
    let r = recalc_all(&DocBuilder::new().page(1, page).build(), &Budget::default());
    assert_eq!(r.differed.len(), 0, "{r:?}");
    assert!(r.agreed >= 1, "{r:?}");
}

#[test]
fn a_case_blind_lookup_is_found_in_an_index_and_not_by_a_scan_of_the_rows() {
    // A section of 6,000 rows and 600 lookups by a name in another case: the steps of a lookup are a constant and the index is paid for once.
    let mut s = shape(1);
    for n in 0..6_000 {
        s = s.row_cell("User", None, RowKey::name(&format!("Row{n}")), "Value", &n.to_string());
    }
    let doc = doc_of(s.row_formula("User", None, RowKey::name("Probe"), "Value", "User.ROW5999", "0"));
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), crate::probe_addr());
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, Budget::default());
    let mut ev = Evaluator::new(&doc, &mut ctx);
    expect("found", ev.evaluate(&at), E::Any(5999.0));
    let first = ev.stats().steps;
    for _ in 0..600 {
        expect("found again", ev.evaluate(&at), E::Any(5999.0));
    }
    let each = (ev.stats().steps - first) / 600;
    assert!(each < 40, "{each} steps for a lookup after the first: the rows were scanned for each");
    assert!(first >= 6_000 / 64, "the index is charged by the rows it lists: {first} steps");
}

// ---- item 3: ---------------------------------------------------------------------------------------------------------------

fn clipping(formula: &str) -> Value {
    let doc = doc_of(ShapeBuilder::new(1).formula("ClippingPath", formula, ""));
    evaluate_at(&doc, Owner::Page(1), &[0], CellAddr::direct("ClippingPath"))
}

#[test]
fn a_clipping_path_that_gives_text_other_than_a_path_is_not_available() {
    // `LEFT("Geometry",0)` is the empty text, and the files save `#N/A` beside an empty `V` in 1,656 of 1,656 cells.
    expect("empty", clipping(r#"LEFT("Geometry",0)"#), E::Err(CellError::Na));
    expect("empty literal", clipping(r#""""#), E::Err(CellError::Na));
    // A path: the string and no error, in any digits from 1.
    expect("path", clipping(r#"IF(1>2,"","Geometry7.Path")"#), E::T("Geometry7.Path"));
    expect("path 1", clipping(r#""Geometry1.Path""#), E::T("Geometry1.Path"));
    expect("path 12", clipping(r#""Geometry12.Path""#), E::T("Geometry12.Path"));
    // Any other text is not a path: the case is exact, the number from 1, nothing before or after.
    for text in [
        "geometry1.path",
        "Geometry1.path",
        "Geometry0.Path",
        "Geometry.Path",
        "Geometry1",
        "Geometry1.Path ",
        " Geometry1.Path",
        "Geometry-1.Path",
        "Foo",
    ] {
        expect(text, clipping(&format!("\"{text}\"")), E::Err(CellError::Na));
    }
    // Only a string is judged: a number is left as the number it is, and a cell that is no `ClippingPath` keeps its empty text.
    expect("number", clipping("7"), E::Any(7.0));
    let doc = doc_of(ShapeBuilder::new(1).set(user("X"), cell(r#""""#, "", None, None)));
    expect("another cell", evaluate_at(&doc, Owner::Page(1), &[0], user("X")), E::T(""));
}

#[test]
fn the_oracle_agrees_with_the_error_the_files_save_beside_an_empty_value() {
    // 1,656 cells (179 files, 46 sources): the formula is the empty text, `V` is empty and `E` is `#N/A`.
    let shape = ShapeBuilder::new(1)
        .set(CellAddr::direct("ClippingPath"), cell(r#""""#, "", None, Some("#N/A")))
        .set(user("Other"), cell(r#""Geometry1.Path""#, "Geometry1.Path", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.agreed, r.differed.len(), r.skipped.total()), (2, 0, 0), "{r:?}");
}

// ---- item 4: SETATREF before a colour ----------------------------------------------------------------------------------

fn prop_width(formula: &str) -> Value {
    crate::eval_on(|s| s.row_cell("Property", None, RowKey::name("Width"), "Value", "3"), formula)
}

#[test]
fn setatref_an_empty_second_slot_is_a_missing_expression_and_zero_plus_a_colour_is_that_colour() {
    expect("0 + colour", prop_width("SETATREF(Prop.Width,,TRUE)+RGB(1,2,3)"), c("010203"));
    expect("FALSE reads the target", prop_width("SETATREF(Prop.Width,,FALSE)*2"), E::Any(6.0));
    expect("four slots", prop_width("SETATREF(Prop.Width,,TRUE,TRUE)"), E::Err(CellError::Value));
    // The sums the files hold: calls worth 0, then the colour, in one sum (1,950 cells).
    expect("two calls", prop_width("SETATREF(Prop.Width,,1)+SETATREF(Prop.Width,,1)+RGB(1,2,3)"), c("010203"));
    expect("number first", prop_width("0+RGB(1,2,3)"), c("010203"));
    expect("colour first, as before", prop_width("RGB(1,2,3)+SETATREF(Prop.Width,,1)"), c("010203"));
    expect("colour first, 1", prop_width("RGB(1,2,3)+1"), c("010203"));
    // Only 0 plus a colour is attested; a sum with a number other than 0 first is not changed by this.
    let other = prop_width("SETATREF(Prop.Width)+RGB(1,2,3)");
    assert!(!matches!(other, Value::Color(_)), "{other:?}");
}

#[test]
fn the_oracle_scores_a_colour_cell_that_is_a_sum_of_calls_and_a_colour() {
    // `FillBkgnd` 748, `FillForegnd` 619 and `GradientStopColor` 583 cells saved a colour and no `E`.
    let shape = ShapeBuilder::new(1)
        .row_cell("Property", None, RowKey::name("Width"), "Value", "3")
        .formula("FillForegnd", "SETATREF(Prop.Width,,1)+SETATREF(Prop.Width,,1)+RGB(9,8,7)", "#090807")
        .formula("FillBkgnd", "SETATREF(Prop.Width,,1)+RGB(1,2,3)", "#010203");
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.agreed, r.differed.len()), (2, 0), "{r:?}");
}

// ---- items 5 to 7: THEMEVAL with no theme -----------------------------------------------------------------------------------------------------

/// `formula` in the probe of a shape with the five scheme cells at 0, over a root style holding `root`.
fn no_theme(root: &[(&str, &str)], formula: &str) -> Value {
    let shape = holding(ShapeBuilder::new(1), &five_at("0", &[])).row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
    let doc = DocBuilder::new().root(|r| root.iter().fold(r, |r, (n, v)| r.cell(n, v))).page(1, PageBuilder::new().shape(shape)).build();
    evaluate_at(&doc, Owner::Page(1), &[0], crate::probe_addr())
}

#[test]
fn themeval_variant_colours_with_no_theme_are_the_tables_accents_and_the_seventh_is_black() {
    expect("3", no_theme(&[], r#"THEMEVAL("variantcolor3")"#), c("ab9ac0"));
    expect("6", no_theme(&[], r#"THEMEVAL("variantcolor6")"#), c("ffc000"));
    expect("7", no_theme(&[], r#"THEMEVAL("variantcolor7")"#), c("000000"));
    // The seventh is the table's `Dark`, whatever the root holds.
    expect("7, root", no_theme(&[("Color", "5")], r#"THEMEVAL("VariantColor7")"#), c("000000"));
    // A default replaces it, and an eighth is no name.
    expect("default", no_theme(&[], r#"THEMEVAL("VariantColor7",RGB(1,2,3))"#), c("010203"));
    expect("eighth", no_theme(&[], r#"THEMEVAL("VariantColor8")"#), E::Err(CellError::Value));
}

#[test]
fn themeval_a_bare_call_gives_the_root_styles_cell_of_the_hosts_name_with_or_without_a_theme() {
    let at = |doc: &Document, section: Option<&str>, name: &str| {
        let addr = match section {
            Some(s) => CellAddr::in_row(s, None, RowKey::Ix(0), name),
            None => CellAddr::direct(name),
        };
        evaluate_at(doc, Owner::Page(1), &[0], addr)
    };
    let world = |schemes: &str, themes: usize, root: &dyn Fn(StyleBuilder) -> StyleBuilder| {
        let shape = holding(ShapeBuilder::new(1), &five_at(schemes, &[]))
            .formula("TextBkgnd", "THEMEVAL()", "Themed")
            .row_formula("Character", None, RowKey::Ix(0), "ComplexScriptFont", "THEMEVAL()", "Themed")
            .row_formula("Character", None, RowKey::Ix(0), "AsianFont", "THEMEVAL()", "Themed")
            .formula("UseGroupGradient", "THEMEVAL()", "Themed");
        with_theme(DocBuilder::new().root(root).page(1, PageBuilder::new().shape(shape)).build(), themes)
    };
    let zero = |r: StyleBuilder| r.cell("TextBkgnd", "0").row_cell("Character", None, RowKey::Ix(0), "ComplexScriptFont", "0");
    // A root holding `TextBkgnd` 0 and `ComplexScriptFont` 0: both give 0 and no error.
    let doc = world("0", 0, &zero);
    expect("TextBkgnd", at(&doc, None, "TextBkgnd"), E::N(0.0));
    expect("ComplexScriptFont", at(&doc, Some("Character"), "ComplexScriptFont"), E::N(0.0));
    // The root's own cell, and not a constant: the fonts' names too.
    let rich = |r: StyleBuilder| {
        r.cell("TextBkgnd", "3")
            .cell("UseGroupGradient", "1")
            .row_cell("Character", None, RowKey::Ix(0), "ComplexScriptFont", "Complex Face")
            .row_cell("Character", None, RowKey::Ix(0), "AsianFont", "Asian Face")
    };
    let doc = world("0", 0, &rich);
    expect("TextBkgnd root", at(&doc, None, "TextBkgnd"), E::N(3.0));
    expect("UseGroupGradient root", at(&doc, None, "UseGroupGradient"), E::N(1.0));
    expect("ComplexScriptFont root", at(&doc, Some("Character"), "ComplexScriptFont"), E::T("Complex Face"));
    expect("AsianFont root", at(&doc, Some("Character"), "AsianFont"), E::T("Asian Face"));
    // Under an explicit theme a host in no table gives the root's cell too, and no call with no argument gives `#VALUE!`.
    let doc = world("40", 1, &rich);
    expect("TextBkgnd, theme", at(&doc, None, "TextBkgnd"), E::N(3.0));
    let doc = world("40", 1, &|r| r);
    expect("TextBkgnd, theme, no root cell", at(&doc, None, "TextBkgnd"), E::N(0.0));
    // The fonts and the gradient switch are in the table and the part does not answer them: the saved value stands.
    expect("ComplexScriptFont, theme", at(&doc, Some("Character"), "ComplexScriptFont"), E::Unsupported);
}

#[test]
fn a_stop_row_whose_entry_has_no_such_stop_takes_the_root_styles_cell_under_a_theme() {
    // `GradientStopColorTrans` 371 and `GradientStopPosition` 297 cells (9 files, 3 sources) saved 0 where the entry has no such stop.
    // The colour keeps its `#N/A` (the renderer drops the row).
    let row = |ix: u32, name: &str| CellAddr::in_row("FillGradient", None, RowKey::Ix(ix), name);
    let themed = |ix: u32| {
        ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"]
            .iter()
            .fold(holding(ShapeBuilder::new(1), &five_at("40", &[("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "3")])), |s, n| {
                s.set(row(ix, n), Cell::with_formula("THEMEVAL()", Some(DocValue::from_text("Themed"))))
            })
    };
    let doc_with = |ix: u32, root_trans: Option<&str>| {
        let mut doc = DocBuilder::new()
            .root(|r| match root_trans {
                Some(v) => r.row_cell("FillGradient", None, RowKey::Ix(ix), "GradientStopColorTrans", v).row_cell(
                    "FillGradient",
                    None,
                    RowKey::Ix(ix),
                    "GradientStopPosition",
                    "0.5",
                ),
                None => r,
            })
            .page(1, PageBuilder::new().shape(themed(ix)))
            .build();
        doc.themes.push(ThemeXml::default().part());
        doc
    };
    // Entry 3 holds two stops; row 2 is past them.
    let doc = doc_with(2, Some("0.25"));
    expect("trans, root", evaluate_at(&doc, Owner::Page(1), &[0], row(2, "GradientStopColorTrans")), E::Any(0.25));
    expect("position, root", evaluate_at(&doc, Owner::Page(1), &[0], row(2, "GradientStopPosition")), E::Any(0.5));
    expect("colour stays", evaluate_at(&doc, Owner::Page(1), &[0], row(2, "GradientStopColor")), E::Err(CellError::Na));
    // The root holds no such row: the cell's own default, 0.
    let doc = doc_with(2, None);
    expect("trans, default", evaluate_at(&doc, Owner::Page(1), &[0], row(2, "GradientStopColorTrans")), E::Any(0.0));
    // A stop the entry has is the entry's.
    expect("a stop of the entry", evaluate_at(&doc, Owner::Page(1), &[0], row(1, "GradientStopPosition")), E::Any(0.0));
}
