//! A cell: what `F`, `V`, `U` and `E` said, kept as the file said them, and which of them the cell owns.

use diacraft_doc::{Cell, CellName, FormulaSlot, Origin, Value};

fn read(f: Option<&str>, v: Option<&str>, u: Option<&str>, e: Option<&str>) -> Cell {
    Cell::read(f, v, u, e, &mut 1_000)
}

#[test]
fn the_two_markers_are_told_apart_from_formulas_before_parsing() {
    // `F="Inh"` is the inherited marker, not a formula named `Inh`.
    assert_eq!(read(Some("Inh"), Some("0"), None, None).formula, FormulaSlot::Inherit);
    assert_eq!(read(Some("No Formula"), Some("0"), None, None).formula, FormulaSlot::NoFormula);
    assert_eq!(read(None, Some("3"), None, None).formula, FormulaSlot::Absent);
    let local = read(Some("Width*0.5"), Some("1"), None, None);
    assert!(matches!(&local.formula, FormulaSlot::Local(f) if &*f.source == "Width*0.5"));
    // Exact words: another case is a formula (a reference to a cell called `inh`), kept as the text it was.
    assert!(matches!(read(Some("inh"), None, None, None).formula, FormulaSlot::Local(_)));
    assert!(matches!(read(Some("NO FORMULA"), None, None, None).formula, FormulaSlot::Local(_)));
}

#[test]
fn formula_text_and_the_other_attributes_survive_untouched() {
    // `<Cell N="Width" V="1" F="Height*2" U="IN"/>`.
    let c = read(Some("Height*2"), Some("1"), Some("IN"), None);
    assert!(matches!(&c.formula, FormulaSlot::Local(f) if &*f.source == "Height*2"));
    assert_eq!(c.value.as_ref().map(Value::text), Some("1"));
    assert_eq!(c.unit.as_deref(), Some("IN"));
    // A formula the parser cannot read is kept, verbatim, and is not a reason to refuse the cell.
    let broken = "((( not a formula";
    let c = read(Some(broken), Some("5"), None, Some("#VALUE!"));
    match &c.formula {
        FormulaSlot::Local(f) => {
            assert_eq!(&*f.source, broken);
            assert!(!f.is_parsed());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(c.error.as_deref(), Some("#VALUE!"));
    // Unknown units are opaque strings.
    assert_eq!(read(None, Some("1"), Some("NURBS"), None).unit.as_deref(), Some("NURBS"));
}

#[test]
fn an_empty_value_is_a_value_and_a_missing_one_is_not() {
    // `V=""` is common and is the empty text; no `V` at all is a malformed cell with its own default.
    assert_eq!(read(None, Some(""), None, None).value, Some(Value::from_text("")));
    assert_eq!(read(None, None, Some("MM"), None).value, None);
}

#[test]
fn origin_says_what_the_cell_owns() {
    // Only local formulas and values are owned; an inherited formula with its own cached value is not a local formula.
    let local_formula = read(Some("Width*0.5"), Some("1"), None, None);
    let constant = read(None, Some("3"), None, None);
    let blocked = read(Some("No Formula"), Some("0"), None, None);
    let cached = read(Some("Inh"), Some("3"), None, None);
    let bare = read(Some("Inh"), None, None, None);
    assert_eq!(local_formula.origin(), Origin::Local);
    assert_eq!(constant.origin(), Origin::Local);
    assert_eq!(blocked.origin(), Origin::Local);
    assert_eq!(cached.origin(), Origin::InheritedWithCachedValue);
    assert_eq!(bare.origin(), Origin::Inherited);
    assert!(local_formula.owns_formula());
    assert!(!constant.owns_formula() && !blocked.owns_formula() && !cached.owns_formula() && !bare.owns_formula());
    // The formula the inherited cell would run is nowhere in it: no copy of the master's text.
    assert!(!matches!(cached.formula, FormulaSlot::Local(_)));
}

#[test]
fn which_cells_end_the_search_for_a_formula() {
    // Step 1: an expression, `No Formula` or no `F` ends it; `Inh` does not.
    assert!(read(Some("Width*0.5"), Some("1"), None, None).ends_search());
    assert!(read(Some("No Formula"), Some("2"), None, None).ends_search());
    assert!(read(None, Some("2"), None, None).ends_search());
    assert!(!read(Some("Inh"), Some("2"), None, None).ends_search());
    assert!(!read(Some("Inh"), None, None, None).ends_search());
}

#[test]
fn guard_is_a_fact_about_the_formula() {
    // `GUARD(x)` is a call, and a guarded formula is one whose outermost call is GUARD.
    assert!(read(Some("GUARD(2 in)"), Some("2"), None, None).is_guarded());
    assert!(read(Some("guard(Width*0.47)"), Some("2"), None, None).is_guarded());
    assert!(!read(Some("Width*0.47"), Some("2"), None, None).is_guarded());
    assert!(!read(Some("Inh"), Some("2"), None, None).is_guarded());
    assert!(!read(Some("GUARD("), Some("2"), None, None).is_guarded(), "an unreadable formula guards nothing");
    assert!(!read(None, Some("GUARD(1)"), None, None).is_guarded(), "a value is not a formula");
}

#[test]
fn the_formula_budget_is_spent_by_reading_and_a_formula_past_it_is_kept_unparsed() {
    // formula's parse_with_budget: a document keeps a total of nodes; one past it is `TooManyNodes`, the text kept.
    let mut left = 5;
    let first = Cell::read(Some("1+2"), None, None, None, &mut left);
    assert!(matches!(&first.formula, FormulaSlot::Local(f) if f.is_parsed()));
    assert!(left < 5, "reading took nodes off the budget");
    let mut none = 0;
    let c = Cell::read(Some("Width*2"), Some("4"), None, None, &mut none);
    match &c.formula {
        FormulaSlot::Local(f) => {
            assert_eq!(&*f.source, "Width*2");
            assert!(!f.is_parsed());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(none, 0);
}

#[test]
fn names_keep_their_case_and_a_case_variant_is_another_cell() {
    // `pinx` is not `PinX`.
    let a = CellName::new("PinX");
    let b = CellName::new("pinx");
    assert_ne!(a, b);
    assert_eq!(a, CellName::new("PinX"));
    assert_eq!(a.as_str(), "PinX");
    assert_eq!(b.as_str(), "pinx");
    let mut map = std::collections::BTreeMap::new();
    map.insert(a, 1);
    map.insert(b, 2);
    assert_eq!(map.len(), 2);
    assert_eq!(map.get("PinX"), Some(&1));
    assert_eq!(map.get("pinx"), Some(&2));
    assert_eq!(map.get("PINX"), None);
}
