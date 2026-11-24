//! The vectors of the batch, on the spec's fixture.
//!
//! Page index, in this order: `Cellar` (`ID` 3, `NameU` `Keller`, background), `Ground` (`ID` 0, `NameU` `Erdgeschoss`, `BackPage` 3), `Roof`
//! (`ID` 8, `NameU` `Dach`). Master index: `Valve` (2, `Schieber`), `Pump.17` (9, `Pumpe.17`), `Fan` (6, `Fan`). Shapes on `Roof`: 41 (`Master` 2),
//! 42 (`Master` 9), 43 (`Door`, `Tuer`), group 44 (`Master` 6) with member 45 (`MasterShape` 12). Shape 51 sits on `Cellar`. Master `Valve` holds
//! shape 12, and master `Fan` holds group 5 with member 12.

use std::sync::Arc;

use diacraft_calc::CellError::{DivZero, Value as ValueError};
use diacraft_calc::{CellAddress, EvalCtx, EvalOptions, Evaluator, FileInfo};
use diacraft_doc::{CellEdit, DocProps, Document, Owner, RowKey, ShapeRef, ShapeType, SheetRef, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::{E, mismatch, probe_addr};

fn fixture() -> Document {
    let mut doc = DocBuilder::new()
        .master(2, MasterBuilder::new().name("Valve").shape(ShapeBuilder::new(12)))
        .master(9, MasterBuilder::new().name("Pump.17").shape(ShapeBuilder::new(13)))
        .master(6, MasterBuilder::new().name("Fan").shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(12))))
        .page(3, PageBuilder::new().name("Cellar").background().shape(ShapeBuilder::new(51)))
        .page(0, PageBuilder::new().name("Ground").back_page(3).shape(ShapeBuilder::new(61)))
        .page(
            8,
            PageBuilder::new()
                .name("Roof")
                .shape(ShapeBuilder::new(41).master(2))
                .shape(ShapeBuilder::new(42).master(9))
                .shape(ShapeBuilder::new(43).named("Door", "Tuer"))
                .shape(ShapeBuilder::new(44).group().master(6).child(ShapeBuilder::new(45).master_shape(12))),
        )
        .build();
    let universal = |names: &[(u32, &str)], id: u32| names.iter().find(|(i, _)| *i == id).map(|(_, n)| Arc::<str>::from(*n));
    let page_names = [(3, "Keller"), (0, "Erdgeschoss"), (8, "Dach")];
    let master_names = [(2, "Schieber"), (9, "Pumpe.17"), (6, "Fan")];
    doc.pages = doc
        .pages
        .iter()
        .map(|p| {
            let mut p = (**p).clone();
            p.name_u = universal(&page_names, p.id);
            Arc::new(p)
        })
        .collect();
    doc.masters = doc
        .masters
        .iter()
        .map(|m| {
            let mut m = (**m).clone();
            m.name_u = universal(&master_names, m.id);
            Arc::new(m)
        })
        .collect();
    doc.title = Arc::from("Plant A");
    doc.props = Arc::new(DocProps {
        subject: Some("Wiring".into()),
        creator: Some("J. Noor".into()),
        keywords: Some("pipes valves".into()),
        description: Some("First issue".into()),
        category: Some("Survey".into()),
        company: Some("Noor Ltd".into()),
        manager: Some("R. Okafor".into()),
        hyperlink_base: Some("https://example.org/docs/".into()),
        app_version: Some("16.0000".into()),
        ..DocProps::default()
    });
    doc
}

/// The probe formula worked out on `sheet` of the fixture.
fn on(sheet: SheetRef, formula: &str) -> diacraft_calc::Value {
    on_with(&fixture(), sheet, formula, EvalOptions::default())
}

fn on_with(doc: &Document, sheet: SheetRef, formula: &str, options: EvalOptions) -> diacraft_calc::Value {
    let mut doc = doc.clone();
    let edit = CellEdit::Formula { formula: Arc::new(diacraft_formula::parse(formula)), value: Some(diacraft_doc::Value::from_text("0")) };
    doc.write_cell(&sheet, &probe_addr(), edit, WriteOrigin::AutomationForce).unwrap();
    let mut ctx = EvalCtx::new(options, Default::default());
    Evaluator::new(&doc, &mut ctx).evaluate(&CellAddress::new(sheet, probe_addr()))
}

fn roof(i: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(8), i))
}

fn member() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(8), 3).child(0))
}

fn cellar() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(3), 0))
}

fn ground() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(0), 0))
}

fn valve_shape() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Master(2), 0))
}

fn fan_shape() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Master(6), 0))
}

fn check(rows: &[(&str, SheetRef, &str, E)]) {
    let mut failures = Vec::new();
    for (id, sheet, formula, expected) in rows {
        let v = on(sheet.clone(), formula);
        if let Some(why) = mismatch(&v, expected) {
            failures.push(format!("{id}: `{formula}`: {why}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

#[test]
fn the_page_functions() {
    check(&[
        ("PAGENAME", roof(0), "PAGENAME()&\"!\"", E::T("Roof!")),
        ("PAGENAME", roof(0), "PAGENAME(750)&\"!\"", E::T("Dach!")),
        ("ALL", roof(0), "PAGENAME(1)&\"!\"", E::T("Roof!")),
        ("ALL", roof(0), "PAGENAME(9999)&\"!\"", E::T("Roof!")),
        ("PAGENAME", cellar(), "PAGENAME()&\"!\"", E::T("Cellar!")),
        ("PAGENAME", ground(), "PAGENAME()&\"!\"", E::T("Ground!")),
        ("PAGENAME", ground(), "Pages[Dach]!ThePage!PAGENAME()", E::T("Roof")),
        ("PAGENAME", valve_shape(), "PAGENAME()&\"!\"", E::T("Valve!")),
        ("PAGENAME", valve_shape(), "PAGENAME(750)&\"!\"", E::T("Schieber!")),
        ("PAGENAME", ground(), "Pages[Nowhere]!ThePage!PAGENAME()", E::Err(diacraft_calc::CellError::Ref)),
        ("PAGENUMBER", ground(), "PAGENUMBER()+0", E::N(1.0)),
        ("PAGENUMBER", roof(0), "PAGENUMBER()+0", E::N(2.0)),
        ("PAGENUMBER", cellar(), "PAGENUMBER()+0", E::N(0.0)),
        ("PAGENUMBER", valve_shape(), "PAGENUMBER()+0", E::N(1.0)),
        ("PAGENUMBER", fan_shape(), "PAGENUMBER()+0", E::N(3.0)),
        ("PAGENUMBER", cellar(), "PAGENUMBER()=0", E::B(true)),
        ("PAGECOUNT", roof(0), "PAGECOUNT()+0", E::N(2.0)),
        ("PAGECOUNT", valve_shape(), "PAGECOUNT()+0", E::N(2.0)),
        ("BKGPAGENAME", ground(), "BKGPAGENAME()&\"!\"", E::T("Cellar!")),
        ("BKGPAGENAME", ground(), "BKGPAGENAME(750)&\"!\"", E::T("Keller!")),
        ("BKGPAGENAME", roof(0), "BKGPAGENAME()&\"!\"", E::T("<no background>!")),
        ("BKGPAGENAME", roof(0), "BKGPAGENAME(750)&\"!\"", E::T("<no background>!")),
        ("BKGPAGENAME", ground(), "BKGPAGENAME(7/0)", E::Err(DivZero)),
    ]);
}

#[test]
fn names_and_ids() {
    check(&[
        ("MASTERNAME", roof(0), "MASTERNAME()&\"!\"", E::T("Valve!")),
        ("MASTERNAME", roof(0), "MASTERNAME(750)&\"!\"", E::T("Schieber!")),
        ("MASTERNAME", roof(1), "MASTERNAME()&\"!\"", E::T("Pump.17!")),
        ("MASTERNAME", member(), "MASTERNAME()&\"!\"", E::T("Fan!")),
        ("MASTERNAME", valve_shape(), "MASTERNAME()&\"!\"", E::T("<no master>!")),
        ("MASTERNAME", roof(2), "MASTERNAME()&\"!\"", E::T("<no master>!")),
        ("MASTERNAME", roof(2), "MASTERNAME(750)&\"!\"", E::T("<no master>!")),
        ("NAME", roof(2), "NAME()&\"!\"", E::T("Door!")),
        ("NAME", roof(2), "NAME(750)&\"!\"", E::T("Tuer!")),
        ("NAME", roof(0), "NAME()&\"!\"", E::T("Valve.41!")),
        ("NAME", roof(1), "NAME()&\"!\"", E::T("Pump.42!")),
        ("NAME", member(), "NAME()&\"!\"", E::T("Sheet.45!")),
        ("ALL", roof(2), "NAME(\"x\")&\"!\"", E::T("Door!")),
        ("ID", roof(0), "ID()*1", E::N(41.0)),
        ("ID", member(), "ID()*1", E::N(45.0)),
        ("ID", roof(0), "Sheet.43!ID()", E::N(43.0)),
        ("ID", roof(0), "Sheet.777777!ID()", E::Err(diacraft_calc::CellError::Ref)),
    ]);
    // An error argument comes back.
    let v = on(roof(2), "NAME(7/0)");
    assert_eq!(mismatch(&v, &E::Err(DivZero)), None, "{v:?}");
}

#[test]
fn a_formula_found_on_a_master_answers_for_the_instance() {
    // `ID()` on master shape 12 with `V` 12, and instance 41 with `F` `Inh`, is 41; `PAGENUMBER()` on the instance copy on `Roof` is 2.
    let mut doc = fixture();
    let valve = valve_shape();
    let edit = CellEdit::Formula { formula: Arc::new(diacraft_formula::parse("ID()")), value: Some(diacraft_doc::Value::from_text("12")) };
    doc.write_cell(&valve, &diacraft_doc::CellAddr::direct("Comment"), edit, WriteOrigin::AutomationForce).unwrap();
    let inherit = CellEdit::Cell(diacraft_doc::Cell::inherited(Some(diacraft_doc::Value::from_text("12"))));
    doc.write_cell(&roof(0), &diacraft_doc::CellAddr::direct("Comment"), inherit, WriteOrigin::AutomationForce).unwrap();
    let mut ctx = EvalCtx::default();
    let at = CellAddress::new(roof(0), diacraft_doc::CellAddr::direct("Comment"));
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::N(41.0)), None, "the master's value would be 12: {v:?}");
}

#[test]
fn document_properties() {
    check(&[
        ("CATEGORY", roof(0), "CATEGORY()", E::T("Survey")),
        ("COMPANY", roof(0), "COMPANY()", E::T("Noor Ltd")),
        ("CREATOR", roof(0), "CREATOR()", E::T("J. Noor")),
        ("DESCRIPTION", roof(0), "DESCRIPTION()", E::T("First issue")),
        ("HYPERLINKBASE", roof(0), "HYPERLINKBASE()", E::T("https://example.org/docs/")),
        ("KEYWORDS", roof(0), "KEYWORDS()", E::T("pipes valves")),
        ("KEYWORDS", roof(0), "KEYWORD()", E::T("pipes valves")),
        ("MANAGER", roof(0), "MANAGER()", E::T("R. Okafor")),
        ("SUBJECT", roof(0), "SUBJECT()", E::T("Wiring")),
        ("TITLE", roof(0), "TITLE()", E::T("Plant A")),
        ("CATEGORY", cellar(), "CATEGORY()", E::T("Survey")),
        ("CATEGORY", fan_shape(), "CATEGORY()", E::T("Survey")),
        ("VERSION", roof(0), "VERSION()+0", E::N(16.0)),
        ("ALL", roof(0), "TITLE(1)", E::Err(ValueError)),
        ("ALL", roof(0), "HASCATEGORY()", E::Err(ValueError)),
        ("TYPE", roof(0), "TYPE()*1", E::N(72.0)),
        ("TYPE", member(), "TYPE()*1", E::N(72.0)),
        ("TYPEDESC", SheetRef::Shape(ShapeRef::top(Owner::Page(8), 3)), "TYPEDESC()&\"!\"", E::T("Group!")),
        ("TYPEDESC", roof(0), "TYPEDESC()&\"!\"", E::T("Shape!")),
        ("TYPEDESC", SheetRef::Page(8), "TYPEDESC()&\"!\"", E::T("Page!")),
        ("TYPEDESC", SheetRef::Document, "TYPEDESC()&\"!\"", E::T("Document!")),
        ("TYPEDESC", SheetRef::Style(0), "TYPEDESC()&\"!\"", E::T("Style!")),
        ("TYPE", SheetRef::Page(8), "TYPE()*1", E::N(70.0)),
        ("TYPE", SheetRef::Document, "TYPE()*1", E::N(79.0)),
        ("TYPE", SheetRef::Style(0), "TYPE()*1", E::N(74.0)),
        ("IS1D", SheetRef::Page(8), "IS1D()", E::Err(diacraft_calc::CellError::Ref)),
        ("IS1D", roof(0), "IS1D()", E::B(false)),
    ]);
    // An empty element, and a title with spaces kept.
    let mut doc = fixture();
    doc.title = Arc::from("  Plant A ");
    let v = on_with(&doc, roof(0), "TITLE()&\"!\"", EvalOptions::default());
    assert_eq!(mismatch(&v, &E::T("  Plant A !")), None, "{v:?}");
    let mut doc = fixture();
    doc.props = Arc::new(DocProps::default());
    let v = on_with(&doc, roof(0), "SUBJECT()&\"!\"", EvalOptions::default());
    assert_eq!(mismatch(&v, &E::T("!")), None, "{v:?}");
    let v = on_with(&doc, roof(0), "VERSION()+0", EvalOptions::default());
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
}

#[test]
fn file_name_and_folder_are_inputs() {
    let doc = fixture();
    let file = |name: Option<&str>, dir: Option<&str>| EvalOptions {
        file: FileInfo { name: name.map(str::to_owned), directory: dir.map(str::to_owned) },
        ..EvalOptions::default()
    };
    let v = on_with(&doc, roof(0), "FILENAME()", file(Some("site.vsdx"), Some("C:\\plans\\")));
    assert_eq!(mismatch(&v, &E::T("site.vsdx")), None, "{v:?}");
    let v = on_with(&doc, roof(0), "DIRECTORY()", file(Some("site.vsdx"), Some("C:\\plans\\")));
    assert_eq!(mismatch(&v, &E::T("C:\\plans\\")), None, "{v:?}");
    let v = on_with(&doc, roof(0), "DIRECTORY()&\"!\"", file(None, None));
    assert_eq!(mismatch(&v, &E::T("!")), None, "A file never saved, or in the web build: {v:?}");
}

#[test]
fn data_boxes_and_categories() {
    let doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", "Pipe")))
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(1).row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", "Pipe;Joint"))
                .shape(ShapeBuilder::new(2))
                .shape(ShapeBuilder::new(3).master(1)),
        )
        .build();
    let on_top = |i: u32, f: &str| on_with(&doc, SheetRef::Shape(ShapeRef::top(Owner::Page(1), i)), f, EvalOptions::default());
    for (i, f, want) in [
        (0u32, "HASCATEGORY(\"Joint\")", true),
        (0, "HASCATEGORY(\"Fan;Joint\")", true),
        (0, "HASCATEGORY(\"Fan\")", false),
        (0, "HASCATEGORY(\"Join\")", false),
        (0, "HASCATEGORY(\"joint\")", false),
        (0, "HASCATEGORY(\"\")", false),
        (1, "HASCATEGORY(\"Pipe\")", false),
        (2, "HASCATEGORY(\"Pipe\")", true),
    ] {
        let v = on_top(i, f);
        assert_eq!(mismatch(&v, &E::B(want)), None, "HASCATEGORY: {f} on {i}: {v:?}");
    }
    // The data boxes are the shape's own text, empty when there is none.
    let v = on_top(1, "DATA1()&\"!\"");
    assert_eq!(mismatch(&v, &E::T("!")), None, "{v:?}");
}

#[test]
fn language_numbers() {
    let v = |f: &str| on(roof(0), f);
    assert_eq!(mismatch(&v("LANGUAGE(\"en-us\")"), &E::N(1033.0)), None);
    assert_eq!(mismatch(&v("LANGUAGE(\"es-es\")"), &E::N(3082.0)), None);
    assert_eq!(mismatch(&v("LANGUAGE(\"EN-US\")=LANGUAGE(1033)"), &E::B(true)), None);
    assert_eq!(mismatch(&v("LANGUAGE(\"zz-qq\")"), &E::Err(ValueError)), None);
    assert_eq!(mismatch(&v("LANGUAGE(\"fr-fr\")"), &E::Err(ValueError)), None);
}

#[test]
fn a_foreign_object_and_a_guide_are_their_own_types() {
    // Project rule: the word is the type's name, and the code is the type's number.
    for (kind, code, word) in [(ShapeType::Foreign, 78.0, "Foreign"), (ShapeType::Guide, 77.0, "Guide"), (ShapeType::Group, 71.0, "Group")] {
        let v = crate::eval_on(|s| s.kind(kind.clone()), "TYPE()*1");
        assert_eq!(mismatch(&v, &E::N(code)), None, "{word}: {v:?}");
        let v = crate::eval_on(|s| s.kind(kind), "TYPEDESC()&\"!\"");
        assert_eq!(mismatch(&v, &E::T(Box::leak(format!("{word}!").into_boxed_str()))), None, "{word}: {v:?}");
    }
}
