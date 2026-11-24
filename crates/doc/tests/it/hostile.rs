//! Imported documents hold anything. These tests build the documents a hostile file could, and check that every walk of a link ends,
//! every lookup that cannot answer says so, and nothing panics.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, Document, Master, Owner, Page, Resolver, Row, RowKey, Section, SectionKey, Shape, ShapeList, ShapeRef, SheetRef,
    Source, Value, WriteOrigin,
};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use proptest::prelude::*;

/// A chain of `depth` nested shapes, built from the inside out with no recursion (a recursive builder would be the stack overflow
/// this test is about).
fn nested(depth: usize) -> ShapeList {
    let mut inner = ShapeList::new();
    for level in (0..depth).rev() {
        let mut shape = Shape::new(u32::try_from(level).unwrap() + 4);
        shape.children = inner;
        inner = ShapeList::from_vec(vec![Arc::new(shape)]);
    }
    inner
}

#[test]
fn groups_nested_past_the_limit_are_walked_to_the_limit_and_no_further() {
    // 200 levels. A file nested a thousand deep is data: finding a shape stops at the limit, a path past it names
    // nothing, and a read or a write there is a refusal and not a stack overflow. (1,000 levels can still be dropped on a test thread; the
    // reader of a file refuses a part nested deeper than 200, which is what keeps `Drop` and `==` shallow in a real document.)
    let mut doc = DocBuilder::new().page(1, PageBuilder::new()).build();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.shapes = nested(1_000);
    let deepest_allowed = diacraft_doc::MAX_SHAPE_DEPTH;
    // A shape at the limit is found by ID; one beyond it is not.
    assert!(doc.find_shape(Owner::Page(1), u32::try_from(deepest_allowed).unwrap() + 4 - 1).is_some());
    assert!(doc.find_shape(Owner::Page(1), 900).is_none(), "level 896 is past the walk's depth limit");
    let past = ShapeRef { owner: Owner::Page(1), path: vec![0; deepest_allowed + 1] };
    let at_limit = ShapeRef { owner: Owner::Page(1), path: vec![0; deepest_allowed] };
    assert!(doc.shape(&past).is_none());
    assert!(doc.shape(&at_limit).is_some());
    assert!(doc.shape_chain(&past).is_none());
    assert!(Resolver::new(&doc).resolve(&SheetRef::Shape(past.clone()), &CellAddr::direct("Width")).is_none());
    let write = doc.write_cell(&SheetRef::Shape(past), &CellAddr::direct("Width"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation);
    assert_eq!(write, Err(diacraft_doc::DocError::NoSuchSheet));
    // At the limit a resolve goes through the whole chain of ancestors, and a write works.
    assert!(Resolver::new(&doc).resolve(&SheetRef::Shape(at_limit.clone()), &CellAddr::direct("Width")).is_some());
    assert!(
        doc.write_cell(&SheetRef::Shape(at_limit), &CellAddr::direct("Width"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation)
            .is_ok()
    );
}

#[test]
fn links_of_every_kind_that_come_back_are_data_not_a_crash() {
    // A style that links to itself, a three-cycle, a link in each family, a page that is its own background, a shape that is its own master
    // shape, and a master whose only shape is an instance of itself.
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().links(Some(1), Some(1), Some(1)))
        .style(2, StyleBuilder::new().links(Some(3), Some(3), Some(3)))
        .style(3, StyleBuilder::new().links(Some(4), Some(4), Some(4)))
        .style(4, StyleBuilder::new().links(Some(2), Some(2), Some(2)))
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).master(1).master_shape(5).links(Some(2), None, None)))
        .page(
            1,
            PageBuilder::new()
                .back_page(1)
                .shape(ShapeBuilder::new(10).master(1).master_shape(10).links(Some(1), Some(2), Some(3)))
                .shape(ShapeBuilder::new(11).master_shape(11).child(ShapeBuilder::new(12).master(1).master_shape(5))),
        )
        .build();
    let resolver = Resolver::new(&doc);
    for at in [vec![0], vec![1], vec![1, 0]] {
        let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: at });
        for cell in ["LineWeight", "FillPattern", "Size", "SelectMode", "PinX", "Width", "TxtWidth"] {
            assert!(resolver.resolve(&sheet, &CellAddr::direct(cell)).is_some());
        }
        for section in ["Geometry", "Character", "FillGradient", "User"] {
            assert!(resolver.effective_rows(&sheet, &SectionKey::new(section, None)).is_some());
        }
    }
    for id in [1, 2, 3, 4] {
        assert!(resolver.resolve(&SheetRef::Style(id), &CellAddr::direct("LineWeight")).is_some());
    }
    assert!(doc.background_chain(1).is_empty(), "a page that is its own background has no other");
}

#[test]
fn a_chain_of_pages_that_names_the_next_one_ends_at_the_first_repeat_and_the_first_gap() {
    let mut doc = Document::empty();
    for id in 0..2_000_u32 {
        let mut page = Page::new(id, "p");
        page.back_page = Some((id + 1) % 1_500);
        doc.pages.push(Arc::new(page));
    }
    let chain = doc.background_chain(0);
    assert_eq!(chain.len(), 1_499, "every other page of the cycle of 1,500, once");
    let mut gap = Page::new(7_000, "gap");
    gap.back_page = Some(404_404);
    doc.pages.push(Arc::new(gap));
    assert!(doc.background_chain(7_000).is_empty());
    assert!(doc.background_chain(u32::MAX).is_empty(), "no such page");
}

#[test]
fn a_long_style_chain_with_a_cycle_at_the_end_ends() {
    let mut b = DocBuilder::new();
    for id in 1..=1_000_u32 {
        b = b.style(id, StyleBuilder::new().links(Some(if id == 1_000 { 1 } else { id + 1 }), None, None));
    }
    let doc = b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build();
    let r = Resolver::new(&doc).resolve(&SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), &CellAddr::direct("LineWeight")).unwrap();
    assert_eq!(r.formula.source, Source::Default);
    assert!(!r.warnings.is_empty());
}

#[test]
fn shape_ids_used_twice_are_both_kept_and_the_first_is_found() {
    // The same ID twice in a page part, nested ones counted.
    let doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(5).cell("Width", "first"))
                .shape(ShapeBuilder::new(9).child(ShapeBuilder::new(5).cell("Width", "nested")))
                .shape(ShapeBuilder::new(5).cell("Width", "last")),
        )
        .build();
    assert_eq!(doc.find_shape(Owner::Page(1), 5).map(|r| r.path), Some(vec![0]));
    assert_eq!(doc.page(1).unwrap().shapes.len(), 3);
    // Two masters and two pages with one ID: the first is the one that is used.
    let mut twice = doc.clone();
    twice.masters.push(Arc::new(Master { id: 3, name: Some(Arc::from("first")), ..Master::default() }));
    twice.masters.push(Arc::new(Master { id: 3, name: Some(Arc::from("second")), ..Master::default() }));
    assert_eq!(twice.master(3).and_then(|m| m.name.as_deref()), Some("first"));
    assert!(Resolver::new(&twice).resolve(&SheetRef::Master(3), &CellAddr::direct("PageWidth")).is_some());
}

#[test]
fn a_section_of_a_hundred_thousand_rows_is_read_without_a_scan_per_row() {
    // The reader caps a section's rows; a document built without it may hold more. Reading every row's cells stays linear in them: a scan
    // per cell would be 100,000 times 100,000.
    let mut section = Section::new("Scratch", None);
    for i in 0..100_000_u32 {
        let mut row = Row { ix: Some(i), ..Row::default() };
        row.cells.insert(diacraft_doc::CellName::new("X"), Cell::constant(Value::from_text("1")));
        section.rows.push(Arc::new(row));
    }
    // A repeated key later in the section is ignored, as in a scan.
    let mut dup = Row { ix: Some(7), ..Row::default() };
    dup.cells.insert(diacraft_doc::CellName::new("X"), Cell::constant(Value::from_text("dup")));
    section.rows.push(Arc::new(dup));
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    let at = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let rows = Resolver::new(&doc).effective_rows(&at, &SectionKey::new("Scratch", None)).unwrap();
    assert_eq!(rows.len(), 100_000);
    let seven = rows.iter().find(|r| r.key == RowKey::Ix(7)).unwrap();
    assert_eq!(seven.cells[0].1.value.cell.and_then(|c| c.value.as_ref()).map(Value::text), Some("1"), "the first row with the key wins");
}

#[test]
fn enormous_names_and_values_are_held_and_a_write_of_a_name_that_big_is_refused() {
    let big = "n".repeat(2 << 20);
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    // The model's own writes refuse a name this size, so a document that holds one came from somewhere else: put it in directly.
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new(&big), Cell::constant(Value::from_text(&big)));
    let at = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    // Reading is by name and costs the name, not a scan or an allocation.
    let r = Resolver::new(&doc).resolve(&at, &CellAddr::direct(&big)).unwrap();
    assert_eq!(r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().len()), Some(2 << 20));
    let e = doc.write_cell(&at, &CellAddr::direct(&big), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation);
    assert!(matches!(e, Err(diacraft_doc::DocError::BadName(_))));
}

// ---- a fuzz of resolution --------------------------------------------------------------------------------------------------

/// A shape the fuzz makes: its `ID`, `Master`, `MasterShape`, line and fill links, and cells as a name and a kind.
type ShapeSpec = (u32, Option<u32>, Option<u32>, Option<u32>, Option<u32>, Vec<(&'static str, u8)>);

/// A random document: styles linked at random (to each other, to themselves, to nothing, to the root), masters of random shapes, and
/// shapes with random `Master` and `MasterShape` and random cells.
fn arbitrary_document() -> impl Strategy<Value = Document> {
    let id = 0u32..7;
    let link = prop::option::of(0u32..9);
    let cell = (prop::sample::select(vec!["LineWeight", "FillPattern", "Size", "PinX", "Width", "SelectMode", "TxtWidth", "Mystery"]), 0u8..4);
    let styles = prop::collection::vec((id.clone(), link.clone(), link.clone(), link.clone(), prop::collection::vec(cell.clone(), 0..4)), 0..7);
    let shapes = |depth_ids: std::ops::Range<u32>| {
        prop::collection::vec(
            (
                depth_ids.clone(),
                prop::option::of(0u32..5),
                prop::option::of(depth_ids),
                link.clone(),
                link.clone(),
                prop::collection::vec(cell.clone(), 0..4),
            ),
            0..5,
        )
    };
    (styles, prop::collection::vec((id, shapes(4..9)), 0..4), shapes(4..9), prop::collection::vec(shapes(4..9), 0..3)).prop_map(
        |(styles, masters, page, members)| {
            let mut b = DocBuilder::new();
            let apply = |mut sb: StyleBuilder, cells: &[(&str, u8)]| {
                for (name, kind) in cells {
                    sb = match kind {
                        0 => sb.cell(name, "1"),
                        1 => sb.formula(name, "Width*2", "2"),
                        2 => sb.inh(name, Some("3")),
                        _ => sb.no_formula(name, "4"),
                    };
                }
                sb
            };
            for (id, l, f, t, cells) in styles {
                b = b.style(id, apply(StyleBuilder::new().links(l, f, t), &cells));
            }
            let build_shape = |(id, master, master_shape, l, f, cells): ShapeSpec, children: Vec<ShapeBuilder>| {
                let mut sb = ShapeBuilder::new(id).links(l, f, None);
                if let Some(m) = master {
                    sb = sb.master(m);
                }
                if let Some(m) = master_shape {
                    sb = sb.master_shape(m);
                }
                for (name, kind) in cells {
                    sb = match kind {
                        0 => sb.cell(name, "1"),
                        1 => sb.formula(name, "Width*2", "2"),
                        2 => sb.inh(name, Some("3")),
                        _ => sb.no_formula(name, "4"),
                    };
                }
                for c in children {
                    sb = sb.child(c);
                }
                sb
            };
            for (id, shapes) in masters {
                let mut mb = MasterBuilder::new();
                for s in shapes {
                    mb = mb.shape(build_shape(s, Vec::new()));
                }
                b = b.master(id, mb);
            }
            let mut pb = PageBuilder::new();
            for (i, s) in page.into_iter().enumerate() {
                let kids = members.get(i).cloned().unwrap_or_default().into_iter().map(|m| build_shape(m, Vec::new())).collect();
                pb = pb.shape(build_shape(s, kids));
            }
            b.page(1, pb).build()
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn resolving_any_cell_of_any_document_ends_and_agrees_with_itself(doc in arbitrary_document()) {
        let resolver = Resolver::new(&doc);
        let mut sheets = vec![SheetRef::Document, SheetRef::Page(1), SheetRef::Master(1), SheetRef::Style(0), SheetRef::Style(3), SheetRef::Style(99)];
        for master in &doc.masters {
            for i in 0..master.shapes.len() {
                sheets.push(SheetRef::Shape(ShapeRef::top(Owner::Master(master.id), u32::try_from(i).unwrap())));
            }
        }
        for (i, shape) in doc.pages.get(0).unwrap().shapes.iter().enumerate() {
            let top = ShapeRef::top(Owner::Page(1), u32::try_from(i).unwrap());
            for j in 0..shape.children.len() {
                sheets.push(SheetRef::Shape(top.child(u32::try_from(j).unwrap())));
            }
            sheets.push(SheetRef::Shape(top));
        }
        for sheet in &sheets {
            for cell in ["LineWeight", "FillPattern", "Size", "PinX", "Width", "SelectMode", "TxtWidth", "Mystery", "PageWidth"] {
                let addr = CellAddr::direct(cell);
                let Some(r) = resolver.resolve(sheet, &addr) else {
                    prop_assert!(resolver.sheet(sheet).is_none(), "{sheet:?} {cell}: None for a sheet that exists");
                    continue;
                };
                prop_assert_eq!(&r.on, sheet);
                // A cell is found, or the formula is the default; a rebind goes with a formula found on the master and with nothing else.
                prop_assert_eq!(r.formula.cell.is_none(), r.formula.source == Source::Default);
                prop_assert_eq!(r.value.cell.is_none(), r.value.source == Source::Default);
                prop_assert_eq!(r.rebind.is_some(), matches!(r.formula.source, Source::Master(_)));
                // A formula that was found ends the search: it is not `Inh`.
                if let Some(c) = r.formula.cell {
                    prop_assert!(c.ends_search());
                }
                // A value that was found holds a `V`.
                if let Some(c) = r.value.cell {
                    prop_assert!(c.value.is_some());
                }
                if let Some(rebind) = &r.rebind {
                    for id in 0..10 {
                        let _ = resolver.rebind(rebind, id);
                    }
                }
            }
            if matches!(sheet, SheetRef::Shape(_)) && resolver.sheet(sheet).is_some() {
                for section in ["Geometry", "Character", "User"] {
                    prop_assert!(resolver.effective_rows(sheet, &SectionKey::new(section, Some(0))).is_some());
                }
            }
        }
    }
}
