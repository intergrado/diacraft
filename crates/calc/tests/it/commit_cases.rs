//! `commit`, case by case (I1, I2, I8, I9): what it writes, what it leaves, and that a second commit writes nothing.

use std::sync::Arc;

use diacraft_calc::{Budget, EvalOptions, OperandMode, Value, commit, write_value};
use diacraft_doc::{Cell, CellAddr, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Quantity;
use diacraft_units::Unit::Inch;

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn cell(formula: &str, v: &str, u: Option<&str>, e: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = u {
        c = c.with_unit(u);
    }
    c.error = e.map(Arc::from);
    c
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn held(doc: &Document, addr: &CellAddr) -> Cell {
    let sheet = &doc.shape(&ShapeRef::top(Owner::Page(1), 0)).expect("shape").sheet;
    match sheet.lookup(addr) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{other:?}"),
    }
}

fn text_of(c: &Cell) -> Option<String> {
    c.value.as_ref().map(|v| v.text().to_owned())
}

fn options() -> EvalOptions {
    EvalOptions::default()
}

#[test]
fn a_second_commit_writes_nothing_and_a_kind_keeps_its_unit() {
    // I8: `U` was not written, so a date, a currency, a point, a polyline, a multidimensional value and a text that spells a colour were
    // read back as numbers or text and written again, 7, 6 and 6 cells on each repeat.
    let kinds: &[(&str, &str, &str, &str)] = &[
        ("D", "DATE(2031,3,7)", "DATE", "2031-03-07T00:00:00"),
        ("Cy", r#"CY(12.5,"EUR")"#, "CY", "12.5000EUR"),
        ("P", "PNT(1,2)", "PNT", "PNT(1,2)"),
        ("L", "POLYLINE(1,1,2,2)", "POLYLINE", "POLYLINE(1, 1, 2, 2)"),
        ("M", "3 IN^2", "MULTIDIM", "3IN^2"),
        ("S", r##""#ff0000""##, "STR", "#ff0000"),
        ("B", "1=1", "BOOL", "1"),
    ];
    let mut shape = ShapeBuilder::new(1).set(user("N"), cell("1.5", "1.50", None, None));
    for (name, formula, _, _) in kinds {
        shape = shape.set(user(name), cell(formula, "0", None, None));
    }
    let mut doc = doc_of(shape);
    let first = commit(&mut doc, &Budget::default(), options());
    assert_eq!((first.written, first.unchanged, first.skipped), (7, 1, 0), "{first:?}");
    for (name, _, unit, v) in kinds {
        let c = held(&doc, &user(name));
        assert_eq!((c.unit.as_deref(), text_of(&c).as_deref()), (Some(*unit), Some(*v)), "{name}");
    }
    // The cell that agreed keeps its text: `1.50` is not `1.5`.
    assert_eq!(text_of(&held(&doc, &user("N"))).as_deref(), Some("1.50"));
    let second = commit(&mut doc, &Budget::default(), options());
    assert_eq!((second.written, second.unchanged), (0, 8), "{second:?}");
    let third = commit(&mut doc, &Budget::default(), options());
    assert_eq!(third.written, 0, "{third:?}");
}

#[test]
fn a_unit_that_only_says_how_a_number_is_shown_is_not_added() {
    // `U` is written for a kind (`DATE`, `CY`...) and not for a measure: a width with no `U` stays with none.
    let mut doc = doc_of(ShapeBuilder::new(1).cell("Height", "2").formula("Width", "Height*2", "9"));
    let r = commit(&mut doc, &Budget::default(), options());
    assert_eq!(r.written, 1, "{r:?}");
    let c = held(&doc, &CellAddr::direct("Width"));
    assert_eq!((text_of(&c).as_deref(), c.unit.as_deref()), (Some("4"), None));
}

fn healthy(n: u32) -> Document {
    let mut s = ShapeBuilder::new(1);
    for k in 0..n {
        s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("{k}+{k}"), "0");
    }
    doc_of(s)
}

fn errors_in(doc: &Document, n: u32) -> Vec<u32> {
    (0..n).filter(|&k| held(doc, &CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A")).error.is_some()).collect()
}

#[test]
fn a_spent_budget_writes_no_error_on_a_healthy_cell() {
    // I1: a spent budget made `#VALUE!` the result of every cell after it, and the commit wrote it, at all 8 budgets tried.
    for steps in [1u64, 2, 3, 5, 8, 13, 21, 40, 80, 120] {
        let mut doc = healthy(40);
        let r = commit(&mut doc, &Budget::with_steps(steps), options());
        assert!(r.budget_exhausted, "{steps}: {r:?}");
        assert_eq!(errors_in(&doc, 40), Vec::<u32>::new(), "{steps} steps: {r:?}");
    }
    // The same for memory.
    for bytes in [0u64, 300, 2_000, 10_000] {
        let mut doc = healthy(40);
        let r = commit(&mut doc, &Budget { steps: Budget::DEFAULT_STEPS, bytes }, options());
        assert_eq!(errors_in(&doc, 40), Vec::<u32>::new(), "{bytes} bytes: {r:?}");
    }
    // With room, every cell is written as what it is.
    let mut doc = healthy(40);
    let r = commit(&mut doc, &Budget::default(), options());
    assert_eq!((r.written, r.budget_exhausted), (39, false), "{r:?}");
}

#[test]
fn a_spent_budget_counts_no_cell_it_did_not_finish() {
    // I1: the cell the budget ran out in has no result. It is not written, and it is not counted as left alone either: every cell that did
    // finish here has a result and nothing saved, so each is a write, and `skipped` and `unchanged` stay 0 at every budget.
    let shape = || {
        let mut s = ShapeBuilder::new(1);
        for k in 0..40u32 {
            s = s.set(CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A"), Cell::with_formula(&format!("{k}+{k}"), None));
        }
        s
    };
    for steps in [1u64, 2, 3, 5, 8, 13, 21, 40, 80, 120] {
        let mut doc = doc_of(shape());
        let r = commit(&mut doc, &Budget::with_steps(steps), options());
        assert!(r.budget_exhausted, "{steps}: {r:?}");
        assert_eq!((r.skipped, r.unchanged), (0, 0), "{steps} steps: {r:?}");
    }
}

#[test]
fn the_depth_limit_writes_no_error_and_a_real_cycle_writes_the_one_the_rule_gives() {
    // I1: a cell that met the native depth limit has the limit's result and not its own; a cell in a cycle has the rule's.
    let mut shape = ShapeBuilder::new(1).set(user("A"), cell("User.B+1", "5", None, None)).set(user("B"), cell("User.A+1", "5", None, None));
    for k in 0..60u32 {
        shape = shape.set(user(&format!("C{k}")), cell(&format!("SHEETREF()!User.C{}+1", k + 1), "7", None, None));
    }
    shape = shape.set(user("C60"), cell("0", "0", None, None));
    let mut doc = doc_of(shape);
    let r = commit(&mut doc, &Budget::default(), options());
    assert!(r.stats.depth_limit_hits > 0, "{r:?}");
    assert_eq!(held(&doc, &user("C0")).error, None, "the limit is not a result");
    assert_eq!(text_of(&held(&doc, &user("C0"))).as_deref(), Some("7"));
    // The cycle is a result of the cell that met itself (here `A`, the first worked out): `#VALUE!` by the rule, its value kept. The other
    // reads that cell at its last good value, and is a number.
    let a = held(&doc, &user("A"));
    assert_eq!((a.error.as_deref(), text_of(&a).as_deref()), (Some("#VALUE!"), Some("5")));
}

/// A hidden chain of 100 cells (each reads the next through `SHEETREF()!`, which the walk of names cannot see, and the last is 1) and, in
/// a section of their own, cells that read a cell of the chain. Every saved value is what the formula gives where the chain is not cut short,
/// so a commit has nothing to write.
fn chain_with_readers(readers_first: bool) -> Document {
    let mut shape = ShapeBuilder::new(1);
    let readers = |mut s: ShapeBuilder| {
        for k in 0..20u32 {
            s = s.set(user(&format!("R{k}")), cell("ISERROR(SHEETREF()!Scratch.A18)", "0", None, None));
            s = s.set(user(&format!("Q{k}")), cell("IFERROR(SHEETREF()!Scratch.A18,7)", "1", None, None));
        }
        s
    };
    if readers_first {
        shape = readers(shape);
    }
    for k in 0..100u32 {
        let formula = if k == 99 { "1".to_owned() } else { format!("SHEETREF()!Scratch.A{}", k + 2) };
        shape = shape.set(CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A"), cell(&formula, "1", None, None));
    }
    if !readers_first {
        shape = readers(shape);
    }
    doc_of(shape)
}

#[test]
fn a_cell_that_reads_one_the_depth_limit_cut_short_is_not_written_either() {
    // Round 2, item 1: a reader that finds the cut-short cell already worked out (a memo hit) took no hit of its own, so `ISERROR` of it
    // was written as 1 over a saved 0 and `IFERROR` of it as 7 over a saved 1: healthy cells overwritten. Here every saved value is right.
    for readers_first in [false, true] {
        let mut doc = chain_with_readers(readers_first);
        let r = commit(&mut doc, &Budget::default(), options());
        assert!(r.stats.depth_limit_hits > 0, "{r:?}");
        let mut wrong = Vec::new();
        for k in 0..20u32 {
            for (name, want) in [(format!("R{k}"), "0"), (format!("Q{k}"), "1")] {
                let c = held(&doc, &user(&name));
                if (text_of(&c).as_deref(), c.error.as_deref()) != (Some(want), None) {
                    wrong.push(format!("{name}: {:?} {:?}", text_of(&c), c.error));
                }
            }
        }
        for k in 0..100u32 {
            let c = held(&doc, &CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A"));
            if (text_of(&c).as_deref(), c.error.as_deref()) != (Some("1"), None) {
                wrong.push(format!("chain {k}: {:?} {:?}", text_of(&c), c.error));
            }
        }
        assert!(wrong.is_empty(), "readers first {readers_first}: {} cells damaged: {wrong:?} {r:?}", wrong.len());
        assert_eq!(r.written, 0, "readers first {readers_first}: {r:?}");
    }
}

#[test]
fn a_themed_value_a_far_date_and_a_colour_index_are_left_as_they_are() {
    // I2: `Themed` was overwritten with `#96afcf`, and a saved colour index 2 became `#ff0000`.
    let shape = ShapeBuilder::new(1)
        .set(user("T"), cell("RGB(1,2,3)", "Themed", None, None))
        .set(user("F"), cell("DATE(2031,3,7)", "30177-04-13T00:00:00", Some("DATE"), None))
        .set(user("I"), cell("RGB(255,0,0)", "2", None, None));
    let mut doc = doc_of(shape);
    let r = commit(&mut doc, &Budget::default(), options());
    assert_eq!((r.written, r.unchanged, r.skipped), (0, 1, 2), "{r:?}");
    assert_eq!(text_of(&held(&doc, &user("T"))).as_deref(), Some("Themed"));
    assert_eq!(text_of(&held(&doc, &user("F"))).as_deref(), Some("30177-04-13T00:00:00"));
    assert_eq!(text_of(&held(&doc, &user("I"))).as_deref(), Some("2"));
}

#[test]
fn a_stale_error_goes_and_the_text_of_the_value_stays() {
    // `1.50` with a stale `E` became `1.5`: the value agrees, so only the `E` is cleared.
    let shape = ShapeBuilder::new(1)
        .set(user("S"), cell("1.5", "1.50", None, Some("#VALUE!")))
        // An error the formula does give: the value stays as the file had it, byte for byte, and the error is recorded.
        .set(user("E"), cell("1/0", "1.50", None, Some("#N/A")));
    let mut doc = doc_of(shape);
    let r = commit(&mut doc, &Budget::default(), options());
    assert_eq!(r.written, 2, "{r:?}");
    let s = held(&doc, &user("S"));
    assert_eq!((text_of(&s).as_deref(), s.error.as_deref()), (Some("1.50"), None));
    let e = held(&doc, &user("E"));
    assert_eq!((text_of(&e).as_deref(), e.error.as_deref()), (Some("1.50"), Some("#DIV/0!")));
    assert_eq!(commit(&mut doc, &Budget::default(), options()).written, 0);
}

#[test]
fn a_commit_works_each_cell_out_from_the_ones_it_reads_whatever_the_options_say() {
    // `commit` is `Live`: `B` reads `A` as worked out (4), and not as saved (0).
    let shape = ShapeBuilder::new(1).set(user("A"), cell("2+2", "0", None, None)).set(user("B"), cell("User.A*2", "0", None, None));
    let mut doc = doc_of(shape);
    let cached = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    commit(&mut doc, &Budget::default(), cached);
    assert_eq!(text_of(&held(&doc, &user("B"))).as_deref(), Some("8"));
}

#[test]
fn a_write_through_bound_keeps_the_unit_the_cell_shows() {
    // 1 inch written to a cell in millimetres is 25.4 mm in the first argument, not 1 `DL` or 25.4 inches.
    let shape = ShapeBuilder::new(1).set(CellAddr::direct("Width"), cell("BOUND(5 mm,0,FALSE,0 mm,100 mm)", "5", Some("MM"), None));
    let mut doc = doc_of(shape);
    let at = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    write_value(&mut doc, &at, &CellAddr::direct("Width"), &Quantity::from_value(1.0, Inch), WriteOrigin::Drawing).expect("written");
    let c = held(&doc, &CellAddr::direct("Width"));
    let f = c.formula.text().expect("a formula").to_ascii_lowercase();
    assert!(f.starts_with("bound(25.4") && f.contains("mm") && !f.contains("dl"), "{f}");
}

#[test]
fn a_long_text_costs_steps_by_its_length_in_a_commit_too() {
    // A megabyte of text is charged at 64 bytes a step, before the work. 1,000 steps cannot buy it.
    let mut doc = doc_of(ShapeBuilder::new(1).set(user("U"), cell(r#"LEN(UPPER(REPT("x",1000000)))"#, "0", None, None)));
    let r = commit(&mut doc, &Budget::with_steps(1_000), options());
    assert!(r.budget_exhausted && r.written == 0, "{r:?}");
    let r = commit(&mut doc, &Budget::default(), options());
    assert!(r.stats.steps > 1_000_000 / 64, "charged by length: {} steps", r.stats.steps);
    assert_eq!(text_of(&held(&doc, &user("U"))).as_deref(), Some("1000000"));
    // `Value` is imported for the type of a cell value the tests above compare.
    let _: Option<Value> = None;
}

#[test]
fn a_saved_colour_index_of_the_documents_table_is_kept_when_it_names_the_colour() {
    // I2: an index past the fixed palette names an entry of the document's colour table; `agrees` reads it there.
    let mut doc = doc_of(ShapeBuilder::new(1).set(user("I"), cell("RGB(1,2,3)", "30", None, None)));
    doc.colors = Arc::new(vec![diacraft_doc::ColorEntry { ix: 30, rgb: "#010203".into() }]);
    let r = commit(&mut doc, &Budget::default(), options());
    assert_eq!((r.written, r.unchanged), (0, 1), "{r:?}");
    assert_eq!(text_of(&held(&doc, &user("I"))).as_deref(), Some("30"));
}

#[test]
fn the_writes_not_yet_made_are_held_memory() {
    // C4: a cell costs its memo entry and its pending write; at a budget of 200,000 bytes about 190 of 400 are made, and the commit says so.
    let mut s = ShapeBuilder::new(1);
    for k in 0..400u32 {
        s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("{k}+{k}"), "x");
    }
    let mut doc = doc_of(s);
    let r = commit(&mut doc, &Budget { steps: Budget::DEFAULT_STEPS, bytes: 200_000 }, options());
    assert!(r.memory_exhausted && r.budget_exhausted, "{r:?}");
    assert!(r.written > 100 && r.written < 215, "{} written; held at most {}", r.written, r.stats.bytes_peak);
}

#[test]
fn a_kind_unit_goes_with_the_result_in_a_typed_cell_too() {
    // I8: a width is a measure, and holds no unit of its own to say how it is shown; a date, a currency, a truth value or a text in it is
    // another kind, and the kind is what `U` says. Each is written once.
    let kinds: &[(&str, &str, &str)] = &[
        ("DATE(2031,3,7)", "DATE", "2031-03-07T00:00:00"),
        (r#"CY(12.5,"EUR")"#, "CY", "12.5000EUR"),
        ("1=1", "BOOL", "1"),
        (r##""#ff0000""##, "STR", "#ff0000"),
    ];
    for (formula, unit, text) in kinds {
        let mut doc = doc_of(ShapeBuilder::new(1).formula("Width", formula, "9"));
        let first = commit(&mut doc, &Budget::default(), options());
        assert_eq!(first.written, 1, "{formula}: {first:?}");
        let c = held(&doc, &CellAddr::direct("Width"));
        assert_eq!((c.unit.as_deref(), text_of(&c).as_deref()), (Some(*unit), Some(*text)), "{formula}");
        let second = commit(&mut doc, &Budget::default(), options());
        assert_eq!(second.written, 0, "{formula}: {second:?}");
    }
}

#[test]
fn a_page_scoped_commit_works_out_that_page_and_leaves_the_others() {
    // An editing command recalculates the page it edited. Two pages each hold a stale formula; the scope picks one.
    let stale = |id: u32| ShapeBuilder::new(id).set(user("N"), cell("2+3", "0", None, None));
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(stale(1))).page(2, PageBuilder::new().shape(stale(1))).build();
    let report = diacraft_calc::commit_in(&mut doc, &Budget::default(), options(), diacraft_calc::Scope::Page(2));
    assert_eq!(report.written, 1);
    let value_on = |doc: &Document, page: u32| {
        let sheet = &doc.shape(&ShapeRef::top(Owner::Page(page), 0)).expect("shape").sheet;
        match sheet.lookup(&user("N")) {
            Lookup::Found(c) => text_of(c),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(value_on(&doc, 1).as_deref(), Some("0"), "page 1 is not in the scope");
    assert_eq!(value_on(&doc, 2).as_deref(), Some("5"));
    // A page that is not there is an empty commit, and the whole-document form still reaches page 1.
    let none = diacraft_calc::commit_in(&mut doc, &Budget::default(), options(), diacraft_calc::Scope::Page(9));
    assert_eq!((none.written, none.unchanged, none.skipped), (0, 0, 0));
    assert_eq!(commit(&mut doc, &Budget::default(), options()).written, 1);
    assert_eq!(value_on(&doc, 1).as_deref(), Some("5"));
}
