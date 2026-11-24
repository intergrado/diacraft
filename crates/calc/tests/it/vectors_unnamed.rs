//! Vectors of the batches that an evaluator has and that no test named (108 ids appeared in no test). What is left out
//! is listed, with the reason, in the report: the functions that are not worked, the vectors about an edit or a trigger, and the reader's.

use std::sync::Arc;

use diacraft_calc::CellError::{Ref, Value as Bad};
use diacraft_calc::{Budget, CellAddress, Clock, EvalCtx, EvalOptions, Evaluator, OperandMode, Value, commit, encode, iso_to_serial};
use diacraft_doc::{Cell, CellAddr, DocProps, Document, Lookup, Owner, RowKey, ShapeRef, ShapeType, SheetRef, TextPart, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::{E, check_table, eval_on, mismatch, probe, probe_addr, relationships};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// The probe formula worked out on a document whose properties `set` changes.
fn with_props(setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, formula: &str, set: impl FnOnce(&mut DocProps), options: EvalOptions) -> Value {
    let (mut doc, at) = probe(setup, formula);
    set(Arc::make_mut(&mut doc.props));
    let mut ctx = EvalCtx::new(options, Budget::default());
    Evaluator::new(&doc, &mut ctx).evaluate(&at)
}

fn text(s: &str) -> E {
    E::T(Box::leak(s.to_owned().into_boxed_str()))
}

#[test]
fn an_empty_or_escaped_property_reads_as_text() {
    // The element is read once, so `A &amp; B` is `A & B`, and an empty one is empty.
    let o = EvalOptions::default;
    let v = with_props(|s| s, "COMPANY()", |p| p.company = Some(Arc::from("A & B")), o());
    assert_eq!(mismatch(&v, &text("A & B")), None, "{v:?}");
    for (formula, set) in [
        ("COMPANY()", Box::new(|p: &mut DocProps| p.company = Some(Arc::from(""))) as Box<dyn Fn(&mut DocProps)>),
        ("DESCRIPTION()", Box::new(|p: &mut DocProps| p.description = Some(Arc::from("")))),
        ("HYPERLINKBASE()", Box::new(|p: &mut DocProps| p.hyperlink_base = Some(Arc::from("")))),
        ("MANAGER()", Box::new(|p: &mut DocProps| p.manager = Some(Arc::from("")))),
    ] {
        let v = with_props(|s| s, formula, |p| set(p), o());
        assert_eq!(mismatch(&v, &text("")), None, "{formula}: {v:?}");
        let v = with_props(|s| s, &format!("{formula}&\"!\""), |p| set(p), o());
        assert_eq!(mismatch(&v, &text("!")), None, "{formula}: {v:?}");
    }
}

#[test]
fn a_saved_property_value_stands_until_it_is_evaluated() {
    // A stored `Old Name` is read as that until the cell is worked out, and then `J. Noor`; no rewriting at load.
    let cached = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    let setup = |s: ShapeBuilder| {
        s.set(user("C"), Cell::with_formula("CREATOR()", Some(DocValue::from_text("Old Name"))))
            .set(user("F"), Cell::with_formula("FILENAME()", Some(DocValue::from_text("old.vsdx"))))
    };
    let set = |p: &mut DocProps| p.creator = Some(Arc::from("J. Noor"));
    for (read, want) in [("User.C", "Old Name"), ("User.F", "old.vsdx")] {
        let v = with_props(setup, read, set, cached.clone());
        assert_eq!(mismatch(&v, &text(want)), None, "{read} before evaluation: {v:?}");
    }
    let v = with_props(|s| s, "CREATOR()", set, cached.clone());
    assert_eq!(mismatch(&v, &text("J. Noor")), None);
    let v = with_props(|s| s, "CREATOR()", |p| p.creator = Some(Arc::from("")), cached);
    assert_eq!(mismatch(&v, &text("")), None);
}

#[test]
fn the_data_boxes_are_three_boxes() {
    let two = |s: ShapeBuilder| s.data(1, "north wing").data(2, "level 2").data(3, "rack 9");
    for (formula, want) in [("DATA1()", "north wing"), ("DATA2()", "level 2"), ("DATA3()", "rack 9")] {
        assert_eq!(mismatch(&eval_on(two, formula), &text(want)), None, "{formula}");
    }
    // No box is an empty string.
    assert_eq!(mismatch(&eval_on(|s| s, "DATA2()&\"!\""), &text("!")), None);
    assert_eq!(mismatch(&eval_on(|s| s, "DATA3()&\"!\""), &text("!")), None);
}

#[test]
fn a_category_is_found_whole_and_by_case() {
    let cats = |s: ShapeBuilder| s.row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", "Pipe;Joint");
    let rows: &[(&str, bool)] = &[
        ("HASCATEGORY(\"Joint\")", true),
        ("HASCATEGORY(\"Fan;Joint\")", true),
        ("HASCATEGORY(\"Fan\")", false),
        ("HASCATEGORY(\"Join\")", false),
        ("HASCATEGORY(\"joint\")", false),
        ("HASCATEGORY(\"\")", false),
    ];
    for (formula, want) in rows {
        assert_eq!(mismatch(&eval_on(cats, formula), &E::B(*want)), None, "{formula}");
    }
    // A shape with no such row.
    assert_eq!(mismatch(&eval_on(|s| s, "HASCATEGORY(\"Pipe\")"), &E::B(false)), None);
    // The row only on the master, read on the instance.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", "Pipe;Joint"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        "HASCATEGORY(\"Pipe\")",
        "0",
    ));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
    assert_eq!(mismatch(&v, &E::B(true)), None, "{v:?}");
}

#[test]
fn a_shape_is_one_dimensional_when_any_of_its_ends_is_there() {
    // Only `EndY` is enough; so is a `BeginX` the master holds.
    assert_eq!(mismatch(&eval_on(|s| s.cell("EndY", "1"), "IS1D()"), &E::B(true)), None);
    assert_eq!(mismatch(&eval_on(|s| s, "IS1D()"), &E::B(false)), None);
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).cell("BeginX", "1"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).row_formula("User", None, RowKey::name("Probe"), "Value", "IS1D()", "0"));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
    assert_eq!(mismatch(&v, &E::B(true)), None, "{v:?}");
}

#[test]
fn language_names_two_tags_and_nothing_else() {
    for (formula, want) in [
        ("LANGUAGE(\"en-us\")", E::N(1033.0)),
        ("LANGUAGE(\"es-es\")", E::N(3082.0)),
        ("LANGUAGE(\"EN-US\")=LANGUAGE(1033)", E::B(true)),
        ("LANGUAGE(\"zz-qq\")", E::Err(Bad)),
        ("LANGUAGE(\"fr-fr\")", E::Err(Bad)),
    ] {
        assert_eq!(mismatch(&eval_on(|s| s, formula), &want), None, "{formula}");
    }
}

#[test]
fn the_page_count_leaves_out_the_backgrounds() {
    let probe_shape = || ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Probe"), "Value", "PAGECOUNT()+0", "0");
    let count = |doc: Document| {
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at)
    };
    let two = DocBuilder::new()
        .page(1, PageBuilder::new().shape(probe_shape()))
        .page(2, PageBuilder::new())
        .page(3, PageBuilder::new().background())
        .build();
    assert_eq!(mismatch(&count(two), &E::N(2.0)), None);
    let none = DocBuilder::new().page(1, PageBuilder::new().background().shape(probe_shape())).page(2, PageBuilder::new().background()).build();
    assert_eq!(mismatch(&count(none), &E::N(0.0)), None, "No floor of 1");
}

#[test]
fn the_type_of_a_shape_is_its_code() {
    for (kind, code) in [(ShapeType::Shape, 72.0), (ShapeType::Group, 71.0), (ShapeType::Foreign, 78.0), (ShapeType::Guide, 77.0)] {
        let v = eval_on(|s| s.kind(kind.clone()), "TYPE()*1");
        assert_eq!(mismatch(&v, &E::N(code)), None, "{kind:?}: {v:?}");
    }
}

#[test]
fn the_document_dates_are_the_core_properties_in_the_zone_of_the_machine() {
    let hours = |h: f64| (h * 3600.0) as i32;
    let at = |offset: i32| EvalOptions { clock: Clock { now: None, utc_offset_seconds: offset }, ..EvalOptions::default() };
    let set = |p: &mut DocProps| {
        p.created = Some(Arc::from("2031-03-07T20:05:09Z"));
        p.modified = Some(Arc::from("2031-03-07T20:05:09Z"));
        p.last_printed = Some(Arc::from("2031-03-07T20:05:09Z"));
    };
    let rows: &[(&str, f64, &str, &str)] = &[
        ("DOCCREATION()", 0.0, "2031-03-07T20:05:09", "DOCCREATION"),
        ("DOCCREATION()", 5.0, "2031-03-08T01:05:09", "DOCCREATION"),
        ("DOCLASTEDIT()", 0.0, "2031-03-07T20:05:09", "DOCLASTEDIT"),
        ("DOCLASTPRINT()", 0.0, "2031-03-07T20:05:09", "DOCLASTPRINT"),
        ("DOCLASTSAVE()", 0.0, "2031-03-07T20:05:09", "DOCLASTSAVE"),
        ("DOCLASTSAVE()", 5.5, "2031-03-08T01:35:09", "DOCLASTSAVE"),
    ];
    for (formula, zone, want, id) in rows {
        let v = with_props(|s| s, formula, set, at(hours(*zone)));
        let e = encode(&v).expect("a date has a V");
        assert_eq!((e.v.as_str(), e.u.as_deref()), (*want, Some("DATE")), "{id}: {v:?}");
    }
    // A file never printed has `lastPrinted` equal to `created`, and that is the value.
    let v = with_props(|s| s, "DOCLASTPRINT()", |p| p.last_printed = Some(Arc::from("2031-03-07T20:05:09Z")), at(0));
    assert_eq!(encode(&v).map(|e| e.v), Some("2031-03-07T20:05:09".to_owned()), "{v:?}");
    assert!(iso_to_serial("2031-03-07T20:05:09").is_some());
}

#[test]
fn a_list_or_a_callout_that_is_not_there_counts_nothing() {
    assert_eq!(mismatch(&eval_on(|s| s, "CALLOUTCOUNT()"), &E::N(0.0)), None, "CALLOUTCOUNT");
    assert_eq!(mismatch(&eval_on(|s| s, "LISTMEMBERCOUNT()"), &E::N(0.0)), None, "LISTMEMBERCOUNT");
    assert_eq!(mismatch(&eval_on(|s| s, "CALLOUTTARGETREF()!Height"), &E::Err(Ref)), None, "CALLOUTTARGETREF");
    assert_eq!(mismatch(&eval_on(|s| s, "IFERROR(CALLOUTTARGETREF()!Height,0.5)"), &E::N(0.5)), None);
}

#[test]
fn a_container_counts_the_members_it_still_has() {
    // Code 1 with no targets is 0, and three targets of which one is lost are 2.
    let page = |targets: &[u32]| {
        let first = ShapeBuilder::new(1)
            .row_cell("User", None, RowKey::name("msvStructureType"), "Value", "Container")
            .formula("Relationships", &relationships(&[(1, targets)]), "0")
            .row_formula("User", None, RowKey::name("Probe"), "Value", "CONTAINERMEMBERCOUNT()", "0");
        DocBuilder::new().page(1, PageBuilder::new().shape(first).shape(ShapeBuilder::new(2)).shape(ShapeBuilder::new(3))).build()
    };
    for (targets, want) in [(&[][..], 0.0), (&[2, 3, 0][..], 2.0)] {
        let doc = page(targets);
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
        assert_eq!(mismatch(&v, &E::N(want)), None, "{targets:?}: {v:?}");
    }
}

#[test]
fn a_formula_inherited_exists_and_is_not_local() {
    // An instance's `User.Q` with `F` `Inh` over a master expression: `FORMULAEXISTS` is TRUE and `LOCALFORMULAEXISTS` FALSE.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Q"), "Value", "1+1", "2"));
    let page = PageBuilder::new().shape(
        ShapeBuilder::new(10)
            .master(1)
            .row_inh("User", None, RowKey::name("Q"), "Value", Some("2"))
            .row_formula("User", None, RowKey::name("P1"), "Value", "FORMULAEXISTS(User.Q)", "0")
            .row_formula("User", None, RowKey::name("P2"), "Value", "LOCALFORMULAEXISTS(User.Q)", "0"),
    );
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let read = |name: &str| {
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), user(name));
        Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at)
    };
    assert_eq!(mismatch(&read("P1"), &E::B(true)), None);
    assert_eq!(mismatch(&read("P2"), &E::B(false)), None);
}

#[test]
fn evalcell_reads_the_arguments_by_name() {
    // The page's median of three, `User.MiddleValue`, and a call that gives it 3, 9 and 5, then 12, 0 and 21.
    let middle = "MAX(MIN(ARG(\"A\"),ARG(\"B\")),MIN(MAX(ARG(\"A\"),ARG(\"B\")),ARG(\"C\")))";
    let setup = |s: ShapeBuilder| s.row_formula("User", None, RowKey::name("MiddleValue"), "Value", middle, "0");
    for (call, want) in [("EVALCELL(User.MiddleValue,\"A\",3,\"B\",9,\"C\",5)", 5.0), ("EVALCELL(User.MiddleValue,\"A\",12,\"B\",0,\"C\",21)", 12.0)]
    {
        assert_eq!(mismatch(&eval_on(setup, call), &E::N(want)), None, "{call}");
    }
}

#[test]
fn a_point_saved_with_units_is_read_in_inches() {
    // `PNT(25.4MM,2)` with `U` `PNT` is 1 inch and 2 inches; `PNT(5,3IN)` is 5 and 3.
    let cell = |v: &str| Cell::constant(DocValue::from_text(v)).with_unit("PNT");
    for (saved, x, y) in [("PNT(25.4MM,2)", 1.0, 2.0), ("PNT(5,3IN)", 5.0, 3.0)] {
        let setup = |s: ShapeBuilder| s.set(user("Q"), cell(saved));
        let vx = eval_on(setup, "PNTX(User.Q)");
        let vy = eval_on(setup, "PNTY(User.Q)");
        let stored = |v: &Value| v.quantity().map(|q| q.stored);
        assert!(stored(&vx).is_some_and(|s| (s - x).abs() < 1e-9) && stored(&vy).is_some_and(|s| (s - y).abs() < 1e-9), "{saved}: {vx:?} {vy:?}");
    }
}

#[test]
fn a_point_lands_in_the_cells_of_a_shape_by_name() {
    // `BeginX` holding `PAR(PNT(2,1))` on the shape is 4.5, and `BeginY` 4.
    let shape = ShapeBuilder::new(1)
        .cell("PinX", "5")
        .cell("PinY", "3")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string())
        .formula("BeginX", "PAR(PNT(2,1))", "0")
        .formula("BeginY", "PAR(PNT(2,1))", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    for (cell, want) in [("BeginX", 4.5), ("BeginY", 4.0)] {
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct(cell));
        let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
        assert_eq!(v.quantity().map(|q| (q.stored * 1e9).round() / 1e9), Some(want), "{cell}: {v:?}");
    }
}

fn committed(formula: &str) -> Cell {
    let shape = ShapeBuilder::new(1).set(user("X"), Cell::with_formula(formula, Some(DocValue::from_text("0"))));
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    commit(&mut doc, &Budget::default(), EvalOptions::default());
    match doc.shape(&ShapeRef::top(Owner::Page(1), 0)).expect("shape").sheet.lookup(&user("X")) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_constructor_and_a_free_length_are_written_as_files_write_them() {
    // The numbers in order, a comma and a space between, `U` the constructor's; a free cell holds `U` `DL`.
    let n = committed("NURBS(2,3,0,0,0.25,0.5,0,1)");
    assert_eq!((n.value.as_ref().map(|v| v.text()), n.unit.as_deref()), (Some("NURBS(2, 3, 0, 0, 0.25, 0.5, 0, 1)"), Some("NURBS")));
    let p = committed("POLYLINE(1,1,2.5,0,2.5,1.5,0,1.5)");
    assert_eq!((p.value.as_ref().map(|v| v.text()), p.unit.as_deref()), (Some("POLYLINE(1, 1, 2.5, 0, 2.5, 1.5, 0, 1.5)"), Some("POLYLINE")));
    let x = committed("PNTX(PNT(3,4))");
    assert_eq!((x.value.as_ref().map(|v| v.text()), x.unit.as_deref()), (Some("3"), Some("DL")));
}

#[test]
fn a_font_is_a_number_of_the_table_and_an_unknown_one_is_value() {
    let (mut doc, at) = probe(
        |s| s.row_formula("User", None, RowKey::name("Same"), "Value", "FONT(\"Some Face\")=FONTTOID(\"Some Face\")", "0"),
        "FONT(\"Some Face\")",
    );
    Arc::make_mut(&mut doc.fonts).push(diacraft_doc::FaceName { name_u: Arc::from("Some Face"), raw: Default::default(), ext: Default::default() });
    let ask = |doc: &Document, at: &CellAddress| Evaluator::new(doc, &mut EvalCtx::default()).evaluate(at);
    let first = ask(&doc, &at);
    let n = first.quantity().map(|q| q.stored);
    assert!(n.is_some_and(|n| n.fract() == 0.0 && n >= 0.0), "{first:?}");
    assert_eq!(ask(&doc, &at), first, "the same number each time");
    let same = CellAddress::new(at.sheet.clone(), user("Same"));
    assert_eq!(mismatch(&ask(&doc, &same), &E::B(true)), None);
    for formula in ["FONT(\"No Such Face\")", "FONTTOID(\"No Such Face\")"] {
        assert_eq!(mismatch(&eval_on(|s| s, formula), &E::Err(Bad)), None, "{formula}");
    }
    assert_eq!(mismatch(&eval_on(|s| s, "ISERRVALUE(FONT(\"No Such Face\"))"), &E::B(true)), None);
}

#[test]
fn a_step_is_the_smaller_of_the_amount_and_a_percent_of_the_room_left() {
    // Through the rule's callers: `TINT` moves the lightness of a grey by a step, and `LUM` reads it back. Plain adding gives 240, 172,
    // 24 and 240 where the rule gives 239, 170, 37 and 120.
    check_table(&[
        ("STEP", "LUM(TINT(RGB(254,254,254),48))", E::N(239.0)),
        ("STEP", "LUM(TINT(RGB(153,153,153),28))", E::N(170.0)),
        ("STEP", "LUM(TINT(RGB(82,82,82),-53))", E::N(37.0)),
        ("STEP", "LUM(TINT(RGB(0,0,0),120))", E::N(120.0)),
    ]);
}

#[test]
fn bound_charges_a_step_for_each_range_beside_its_three_nodes() {
    // Type 1 sorts its ranges, so a range costs its arguments and one step more for its place in the sort: 3,510 steps for 500
    // ranges, 7 each and 3,010 without the one.
    let ranges: Vec<String> = (0..500).map(|i| format!("FALSE,{},{}", i * 3, i * 3 + 2)).collect();
    let (doc, at) = probe(|s| s, &format!("BOUND(750.5,1,{})", ranges.join(",")));
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let _ = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert!(ctx.stats.steps >= 7 * 500, "{} steps", ctx.stats.steps);
}

#[test]
fn the_walk_by_names_holds_memory_for_each_cell_it_visits() {
    // A cell names 1,000 others in a branch it does not take, and is worked out only after a walk over them, which holds two
    // keys for each (512 bytes a cell). The run holds 1.18 MB without the walk and 1.70 MB with it, so 1.4 MB is room for the first only: the
    // memory is spent, and with room the cell is 1.
    let names: Vec<String> = (0..1000).map(|k| format!("User.C{k}")).collect();
    let setup = |mut s: ShapeBuilder| {
        for k in 0..1000 {
            s = s.row_cell("User", None, RowKey::name(&format!("C{k}")), "Value", "1");
        }
        s
    };
    let (doc, at) = probe(setup, &format!("IF(1=1,1,SUM({}))", names.join(",")));
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget { steps: Budget::DEFAULT_STEPS, bytes: 1_400_000 });
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert!(ctx.stats.memory_exhausted, "{v:?} {:?}", ctx.stats);
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::N(1.0)), None, "{v:?}");
    assert!(!ctx.stats.memory_exhausted, "{:?}", ctx.stats);
}

#[test]
fn a_source_text_past_the_cap_is_value_even_when_what_is_read_of_it_is_short() {
    // 1.8 MB of soft hyphens reads as empty text (SHAPETEXT's flag 1, absent, removes them), but the source is over the cap
    // first and is not built.
    let parts: Vec<TextPart> = (0..3).map(|_| TextPart::Chars(Arc::from("\u{ad}".repeat(300_000)))).collect();
    let v = eval_on(|s| s.text_parts(parts), "SHAPETEXT(TheText)");
    assert_eq!(mismatch(&v, &E::Err(Bad)), None, "{v:?}");
}
