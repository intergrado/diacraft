//! A stamp (`diacraft_doc::stamp_cells`) compares exactly what [`cell_same`] compares of a cell's meaning, so that the model writer
//! keeps and drops a held value on the edits on which this splice writer keeps and drops the attribute it stands beside. The pairs are those of
//! `cell_same`'s own rule: a formula by its text, an inherited formula as it is, a constant by its text or its number; what a calculation made of a
//! formula (its value, its error) and the unit a value shows in are no edit; and the attributes and content a cell keeps beside them (`raw`, `extra`)
//! are compared by both.

use diacraft_doc::{Cell, CellName, FormulaSlot, Sheet, Value, stamp_cells};

use super::cell_same;

fn stamp_of(cell: &Cell) -> diacraft_doc::Stamp {
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("A"), cell.clone());
    stamp_cells(&sheet, &["A"])
}

fn constant(value: Option<&str>, formula: FormulaSlot) -> Cell {
    Cell { formula, value: value.map(Value::from_text), ..Cell::new() }
}

#[test]
fn a_stamp_of_a_cell_says_what_cell_same_says() {
    let formula = |source: &str, value: Option<&str>| Cell::with_formula(source, value.map(Value::from_text));
    let mut pairs: Vec<(Cell, Cell)> = vec![
        // A formula: its text, and not what a calculation made of it.
        (formula("Width*0.5", Some("2")), formula("Width*0.5", Some("2"))),
        (formula("Width*0.5", Some("2")), formula("Width*0.5", Some("9"))),
        (formula("Width*0.5", Some("2")), formula("Width*0.5", None)),
        (formula("Width*0.5", None), Cell { error: Some("#N/A".into()), ..formula("Width*0.5", None) }),
        (formula("Width*0.5", None), formula("Width*0.5", None).with_unit("IN")),
        (formula("Width*0.5", Some("2")), formula("Width*0.6", Some("2"))),
        (formula("Width*0.5", Some("2")), formula("Height*0.5", Some("2"))),
        (formula("Inh", None), Cell::inherited(None)),
        // An inherited formula: the same whatever it calculated.
        (Cell::inherited(None), Cell::inherited(None)),
        (Cell::inherited(Some(Value::from_text("2"))), Cell::inherited(Some(Value::from_text("9")))),
        (Cell::inherited(None), Cell::inherited(Some(Value::from_text("9"))).with_unit("MM")),
        // Another slot is another cell.
        (Cell::inherited(None), constant(None, FormulaSlot::Absent)),
        (formula("1", Some("1")), constant(Some("1"), FormulaSlot::Absent)),
        (constant(Some("1"), FormulaSlot::Absent), constant(Some("1"), FormulaSlot::NoFormula)),
        (formula("No Formula", None), constant(None, FormulaSlot::NoFormula)),
    ];
    // A constant and a `No Formula` one: equal text, or numbers that are equal; the unit it shows in is no edit.
    for slot in [FormulaSlot::Absent, FormulaSlot::NoFormula] {
        for (a, b) in [
            (Some("0.01"), Some("0.01")),
            (Some("1"), Some("1.0")),
            (Some("1"), Some("1.000")),
            (Some("0"), Some("-0")),
            (Some("1e1"), Some("10")),
            (Some("2.54"), Some("2.540")),
            (Some("abc"), Some("abc")),
            (Some("abc"), Some("abd")),
            (Some("1"), Some("2")),
            (Some("1"), Some("one")),
            (Some(""), None),
            (Some("1"), None),
            (None, None),
            (Some("#FF0000"), Some("#ff0000")),
            (Some("1.#INF"), Some("1.#INF")),
            (Some("1.#INF"), Some("-1.#INF")),
            (Some("Themed"), Some("themed")),
        ] {
            pairs.push((constant(a, slot.clone()), constant(b, slot.clone())));
        }
        let with_unit = |unit: &str| constant(Some("2.54"), slot.clone()).with_unit(unit);
        pairs.push((with_unit("IN"), with_unit("CM")));
        pairs.push((with_unit("IN"), constant(Some("2.54"), slot.clone())));
    }
    // What a cell keeps beside its value and inside it (`raw`, `extra`), which `cell_same` compares too ("as `cell_same` is").
    let raw = |pairs: &[(&str, &str)], mut cell: Cell| {
        cell.raw = diacraft_doc::RawAttrs::from_pairs(pairs.iter().copied());
        cell
    };
    let inside = |text: Option<&str>, mut cell: Cell| {
        cell.extra = Some(std::sync::Arc::new(diacraft_doc::CellExtra { text: text.map(Into::into), ..diacraft_doc::CellExtra::default() }));
        cell
    };
    let with_node = |after: u32, text: &str, mut cell: Cell| {
        let mut kept = diacraft_doc::Passthrough::new();
        kept.push(after, diacraft_doc::RawNode::Comment(text.into()));
        cell.extra = Some(std::sync::Arc::new(diacraft_doc::CellExtra { passthrough: kept, ..diacraft_doc::CellExtra::default() }));
        cell
    };
    for base in [
        formula("Width*0.5", Some("2")),
        Cell::inherited(None),
        constant(Some("1"), FormulaSlot::Absent),
        constant(Some("1"), FormulaSlot::NoFormula),
    ] {
        pairs.push((base.clone(), base.clone()));
        pairs.push((raw(&[("k", "v")], base.clone()), base.clone()));
        pairs.push((raw(&[("k", "v")], base.clone()), raw(&[("k", "v")], base.clone())));
        pairs.push((raw(&[("k", "v")], base.clone()), raw(&[("k", "w")], base.clone())));
        pairs.push((raw(&[("a", "b"), ("c", "d")], base.clone()), raw(&[("c", "d"), ("a", "b")], base.clone())));
        pairs.push((inside(None, base.clone()), base.clone()));
        pairs.push((inside(Some("t"), base.clone()), inside(Some("t"), base.clone())));
        pairs.push((inside(Some("t"), base.clone()), inside(Some("u"), base.clone())));
        pairs.push((inside(Some("t"), base.clone()), inside(None, base.clone())));
        pairs.push((with_node(0, "c", base.clone()), with_node(0, "c", base.clone())));
        pairs.push((with_node(0, "c", base.clone()), with_node(1, "c", base.clone())));
        pairs.push((with_node(0, "c", base.clone()), with_node(0, "d", base.clone())));
        pairs.push((with_node(0, "c", base.clone()), base));
    }
    let mut same = 0;
    for (a, b) in &pairs {
        assert_eq!(stamp_of(a) == stamp_of(b), cell_same(a, b), "{a:?} and {b:?}");
        assert_eq!(stamp_of(b) == stamp_of(a), cell_same(b, a), "{b:?} and {a:?}");
        same += usize::from(cell_same(a, b));
    }
    // The pairs hold both answers, a good many of each.
    assert!(same > 20 && pairs.len() - same > 20, "{same} of {} are the same", pairs.len());
}
