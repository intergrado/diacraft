//! Structural sharing on a large document: what one cell write copies, and what an undo history of them costs.
//!
//! The case is the owner's: a drawing of 10,000 shapes (`design` section 6, performance targets), measured the review's way: with
//! connects on the page, masters with shapes and connects, and a list of styles at the cap, since a fixture with none of them hid a copy
//! of each. The tests count what is shared, with `Arc::ptr_eq`, and what the lookups of a write built, and so do not depend on the speed of
//! the machine: none of them is held to a time.

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Connect, Document, FormulaSlot, MAX_STYLES, Master, Owner, Page, Shape, ShapeList, ShapeRef, SheetRef,
    Style, Value, WriteOrigin,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder};
use std::sync::Arc;

const SHAPES: usize = 10_000;

fn connect(i: u32) -> Connect {
    Connect { from_sheet: i, to_sheet: i + 1, from_part: Some(9), to_part: Some(100), ..Connect::default() }
}

/// A document the review's way: one page of `SHAPES` shapes of 40 cells each (a formula among every five) **with 10,000 connects**, 99 other
/// small pages, **2,000 masters of 5 shapes and 10 connects each**, and **`MAX_STYLES` styles**.
fn big() -> Document {
    let mut doc = DocBuilder::new().build();
    let master_shape = Arc::new(Shape::new(5));
    for id in 1..=2_000 {
        let mut master = Master { id, ..Master::default() };
        master.shapes = (0..5).map(|_| Arc::clone(&master_shape)).collect();
        master.connects = (0..10).map(connect).collect();
        doc.masters.push(Arc::new(master));
    }
    doc.styles = (1..=u32::try_from(MAX_STYLES).unwrap()).map(|id| Arc::new(Style { id, ..Style::default() })).collect();
    let mut shapes = Vec::with_capacity(SHAPES);
    for i in 0..SHAPES {
        let mut shape = Shape::new(u32::try_from(i + 4).unwrap());
        let mut sheet = diacraft_doc::Sheet::default();
        for c in 0..40 {
            let cell = if c % 5 == 0 {
                Cell::with_formula(&format!("Width*{c}"), Some(Value::from_text("1")))
            } else {
                Cell::constant(Value::from_text("1"))
            };
            sheet.cells.insert(diacraft_doc::CellName::new(&format!("Cell{c}")), cell);
        }
        shape.sheet = Arc::new(sheet);
        shapes.push(Arc::new(shape));
    }
    let mut page = Page::new(1, "Big");
    page.shapes = shapes.into_iter().collect::<ShapeList>();
    page.connects = (0..10_000).map(connect).collect();
    doc.pages.push(Arc::new(page));
    for id in 2..=100 {
        doc.pages.push(Arc::new(Page::new(id, "small")));
    }
    doc
}

fn shape_at(i: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![i] })
}

fn write_one(doc: &mut Document, i: u32, value: &str) {
    let edit = CellEdit::Value(Value::from_text(value));
    doc.write_cell(&shape_at(i), &CellAddr::direct("Cell1"), edit, WriteOrigin::Automation).unwrap();
}

#[test]
fn one_cell_write_on_ten_thousand_shapes_copies_one_shape_one_chunk_and_the_lists_above() {
    let before = big();
    let mut after = before.clone();
    // The clone shares all of it.
    assert!(Arc::ptr_eq(after.pages.get(0).unwrap(), before.pages.get(0).unwrap()));

    let at = 7_321;
    write_one(&mut after, at, "2");

    // Documents: every page but the one written to, every master and every style is the very same allocation, and so are the lists that
    // hold them: a write copies not one pointer of the 65,536 styles or the 2,000 masters (the review's first round: it copied both lists).
    assert!(!Arc::ptr_eq(after.pages.get(0).unwrap(), before.pages.get(0).unwrap()));
    assert!(after.pages.iter().zip(&before.pages).skip(1).all(|(a, b)| Arc::ptr_eq(a, b)), "the other 99 pages are shared");
    assert!(after.masters.iter().zip(&before.masters).all(|(a, b)| Arc::ptr_eq(a, b)));
    assert!(after.styles.iter().zip(&before.styles).all(|(a, b)| Arc::ptr_eq(a, b)));
    assert!(after.styles.is_same_list_as(&before.styles), "the list of 65,536 styles is one pointer, shared: not a chunk of it was copied");
    assert!(after.masters.is_same_list_as(&before.masters), "and so is the list of 2,000 masters");
    assert_eq!(after.pages.shared_chunks_with(&before.pages), before.pages.chunk_count() - 1, "the chunk of the pages that holds the written page");
    assert!(before.styles.chunk_count() <= MAX_STYLES / diacraft_doc::CHUNK);
    // The page: its sheet, its connects and its list of chunks, all but the one chunk the shape is in.
    let (a, b) = (after.pages.get(0).unwrap(), before.pages.get(0).unwrap());
    assert!(Arc::ptr_eq(&a.sheet, &b.sheet));
    assert!(a.connects.shares_with(&b.connects), "the page's 10,000 connects are one list, shared and not copied");
    assert_eq!(a.connects.len(), 10_000);
    assert_eq!(a.shapes.chunk_count(), b.shapes.chunk_count());
    assert_eq!(a.shapes.shared_chunks_with(&b.shapes), b.shapes.chunk_count() - 1, "one chunk of {} was copied", diacraft_doc::CHUNK);
    // The shapes: exactly one of the 10,000 is new.
    let new_shapes = (0..SHAPES).filter(|i| !Arc::ptr_eq(a.shapes.get(*i).unwrap(), b.shapes.get(*i).unwrap())).count();
    assert_eq!(new_shapes, 1);
    // The other cells of that shape share their formulas: they are reference counts, not copies of the trees.
    let (shape_a, shape_b) = (a.shapes.get(at as usize).unwrap(), b.shapes.get(at as usize).unwrap());
    for c in [0, 5, 10, 35] {
        let name = format!("Cell{c}");
        let (FormulaSlot::Local(fa), FormulaSlot::Local(fb)) =
            (&shape_a.sheet.cell(&name).unwrap().formula, &shape_b.sheet.cell(&name).unwrap().formula)
        else {
            panic!("{name} holds a formula");
        };
        assert!(Arc::ptr_eq(fa, fb), "{name}");
    }
    // And the older document is unchanged.
    assert_eq!(shape_b.sheet.cell("Cell1").and_then(|c| c.value.as_ref()).map(Value::text), Some("1"));
    assert_eq!(shape_a.sheet.cell("Cell1").and_then(|c| c.value.as_ref()).map(Value::text), Some("2"));
}

#[test]
fn a_write_to_a_master_copies_that_master_and_shares_its_connects_and_the_lists() {
    let before = big();
    let mut after = before.clone();
    let edit = CellEdit::Value(Value::from_text("2"));
    after.write_cell(&SheetRef::Master(1_000), &CellAddr::direct("Mine"), edit, WriteOrigin::Automation).unwrap();
    let changed = (0..2_000).filter(|i| !Arc::ptr_eq(after.masters.get(*i).unwrap(), before.masters.get(*i).unwrap())).count();
    assert_eq!(changed, 1);
    assert!(after.masters.get(999).unwrap().connects.shares_with(&before.masters.get(999).unwrap().connects));
    assert!(after.masters.get(999).unwrap().shapes.shared_chunks_with(&before.masters.get(999).unwrap().shapes) >= 1);
    assert!(after.masters.get(999).unwrap().connects.shares_with(&before.masters.get(999).unwrap().connects));
    // The copied master shares its own connects and shapes with the older one.
    let (m_after, m_before) = (after.masters.get(999).unwrap(), before.masters.get(999).unwrap());
    assert!(m_after.connects.shares_with(&m_before.connects));
    assert_eq!(after.masters.shared_chunks_with(&before.masters), before.masters.chunk_count() - 1);
    assert!(after.styles.is_same_list_as(&before.styles));
    assert!(after.pages.iter().zip(&before.pages).all(|(a, b)| Arc::ptr_eq(a, b)));
}

#[test]
fn a_thousand_undo_steps_of_one_cell_each_cost_a_chunk_a_shape_and_a_page_a_step() {
    // The engine keeps the document before each edit. A thousand writes spread over the page leave a thousand versions that share
    // all but what each write copied.
    let mut doc = big();
    let mut versions = vec![doc.clone()];
    for step in 0..1_000_u32 {
        write_one(&mut doc, step * 7 % u32::try_from(SHAPES).unwrap(), &step.to_string());
        let now = doc.clone();
        let last = versions.last().unwrap();
        let shared = now.pages.get(0).unwrap().shapes.shared_chunks_with(&last.pages.get(0).unwrap().shapes);
        assert_eq!(shared, last.pages.get(0).unwrap().shapes.chunk_count() - 1, "step {step}: one chunk of the list is new");
        assert!(now.pages.get(0).unwrap().connects.shares_with(&last.pages.get(0).unwrap().connects), "step {step}: the connects are shared");
        assert!(now.styles.is_same_list_as(&last.styles), "step {step}: the styles are shared");
        assert!(now.masters.is_same_list_as(&last.masters), "step {step}: the masters are shared");
        versions.push(now);
    }
    // Every shape of the first version is still in the last, but the (at most 1,000) written ones: nothing else was copied.
    let (first, last) = (versions[0].pages.get(0).unwrap(), versions[1_000].pages.get(0).unwrap());
    let copied = (0..SHAPES).filter(|i| !Arc::ptr_eq(first.shapes.get(*i).unwrap(), last.shapes.get(*i).unwrap())).count();
    assert_eq!(copied, 1_000, "the thousand shapes that were written, and no others");
}

/// A document of one page of 40 small shapes, to compare with.
fn small() -> Document {
    let mut small = DocBuilder::new().page(1, PageBuilder::new()).build();
    let mut shapes = Vec::new();
    for i in 0..40_u32 {
        let mut s = Shape::new(i + 4);
        for c in 0..40 {
            Arc::make_mut(&mut s.sheet).cells.insert(diacraft_doc::CellName::new(&format!("Cell{c}")), Cell::constant(Value::from_text("1")));
        }
        shapes.push(Arc::new(s));
    }
    Arc::make_mut(small.pages.get_mut(0).unwrap()).shapes = shapes.into_iter().collect();
    small
}

/// What one write, with the version before it kept (as the engine's history does), left unshared with that version: the pages written to or
/// not shared, the chunks of the page's shapes that were copied, the shapes that were, and the lists (connects, masters, styles) that were
/// copied whole. Each one is an allocation the write made, and the count is the same on any machine at any load.
fn copies_made_by_a_write(doc: &mut Document, i: u32) -> usize {
    let kept = doc.clone();
    write_one(doc, i, "9");
    let (now, then) = (doc.pages.get(0).unwrap(), kept.pages.get(0).unwrap());
    let unshared = |a: &Document, b: &Document| {
        a.pages.iter().zip(&b.pages).filter(|(x, y)| !Arc::ptr_eq(x, y)).count()
            + a.masters.iter().zip(&b.masters).filter(|(x, y)| !Arc::ptr_eq(x, y)).count()
            + a.styles.iter().zip(&b.styles).filter(|(x, y)| !Arc::ptr_eq(x, y)).count()
    };
    let shapes_copied = (0..now.shapes.len()).filter(|k| !Arc::ptr_eq(now.shapes.get(*k).unwrap(), then.shapes.get(*k).unwrap())).count();
    unshared(doc, &kept)
        + (now.shapes.chunk_count() - now.shapes.shared_chunks_with(&then.shapes))
        + shapes_copied
        + usize::from(!now.connects.shares_with(&then.connects))
        + usize::from(!doc.masters.is_same_list_as(&kept.masters))
        + usize::from(!doc.styles.is_same_list_as(&kept.styles))
}

#[test]
fn what_one_write_copies_is_counted_and_independent_of_the_size_of_the_document() {
    // Counted, not timed (a time moves with the load on the machine, and this claim failed under it): the write copies the page, one chunk of
    // its shapes and the one shape, on a page of 40 shapes and on a page of 10,000 with 10,000 connects, 2,000 masters and 65,536 styles
    // alike. The same count for both is the claim that a write does not grow with the document; the count itself says what it is.
    let (mut small, mut large) = (small(), big());
    let small_copies = copies_made_by_a_write(&mut small, 3);
    let large_copies = copies_made_by_a_write(&mut large, 3);
    assert_eq!(small_copies, 3, "the page, one chunk of its shapes, the shape");
    assert_eq!(large_copies, small_copies, "a write must not grow with the document");
    // And the same on the first shape of the large page, one in the middle, and the last.
    for i in [0, 4_999, 9_999] {
        assert_eq!(copies_made_by_a_write(&mut large, i), 3, "a write to shape {i}");
    }
}

#[test]
fn writes_to_the_members_of_an_instance_of_a_big_master_are_batched_not_rebuilt() {
    // The review: a member write that reaches the master step rebuilt the whole map of the master's shapes, 476 µs at 10,000 master shapes
    // and 13 ms at 100,000, and the commit writes one cell per member. A batch builds it once.
    let n: u32 = 20_000;
    let mut group = Shape::new(1);
    group.children = (0..n).map(|c| Arc::new(Shape::new(c + 2))).collect();
    let mut instance = Shape::new(40);
    instance.master = Some(1);
    instance.children = (0..n)
        .map(|c| {
            let mut m = Shape::new(1000 + c);
            m.master_shape = Some(c + 2);
            Arc::new(m)
        })
        .collect();
    let mut doc = DocBuilder::new().build();
    doc.masters.push(Arc::new(Master { id: 1, shapes: std::iter::once(Arc::new(group)).collect(), ..Master::default() }));
    let mut page = Page::new(1, "p");
    page.shapes = std::iter::once(Arc::new(instance)).collect();
    doc.pages.push(Arc::new(page));

    let member = |c: u32| SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0, c] });
    let members = 300;
    let write = || CellWrite { addr: CellAddr::direct("PinX"), edit: CellEdit::Value(Value::from_text("1")), origin: WriteOrigin::Automation };
    // `PinX` is a shape-only cell, so each write is judged by a walk that reaches the master step: the master's map is needed.
    // Counted, not timed: the lookups the resolvers built (`diacraft_doc::work`), which is the work the claim is about.
    diacraft_doc::work::reset();
    let mut one_by_one = doc.clone();
    for c in 0..members {
        let w = write();
        one_by_one.write_cell(&member(c), &w.addr, w.edit, w.origin).unwrap();
    }
    let loop_builds = diacraft_doc::work::resolver_builds();
    diacraft_doc::work::reset();
    let mut batched = doc.clone();
    batched.write_batch((0..members).map(|c| (member(c), write())).collect()).unwrap();
    let batch_builds = diacraft_doc::work::resolver_builds();
    eprintln!("{members} member writes against a master of {n} shapes: {loop_builds} lookups built one by one, {batch_builds} as one batch");
    assert_eq!(one_by_one, batched);
    assert!(loop_builds >= u64::from(members), "the loop builds the master's map for each write: {loop_builds}");
    assert!(batch_builds <= 8, "the batch builds the lookups once: {batch_builds}");
}

#[test]
fn the_list_of_shapes_copies_chunks_not_shapes_when_a_shape_is_added_or_removed() {
    let before = big();
    let mut after = before.clone();
    let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
    page.shapes.push(Arc::new(Shape::new(99_999)));
    assert_eq!(
        page.shapes.shared_chunks_with(&before.pages.get(0).unwrap().shapes),
        before.pages.get(0).unwrap().shapes.chunk_count() - 1,
        "a push copies the last chunk"
    );
    let removed = page.shapes.remove(8_000).unwrap();
    assert_eq!(removed.id, 8_004);
    let shared = page.shapes.shared_chunks_with(&before.pages.get(0).unwrap().shapes);
    assert!(shared >= 8_000 / diacraft_doc::CHUNK, "the chunks before the removed shape are shared: {shared}");
    // Every shape that is still there is the same allocation (a removal clones the Arcs, not the shapes).
    let same = (0..8_000).all(|i| Arc::ptr_eq(page.shapes.get(i).unwrap(), before.pages.get(0).unwrap().shapes.get(i).unwrap()));
    assert!(same);
}

#[test]
fn a_huge_title_and_page_name_are_shared_by_the_copy_an_edit_makes_and_not_copied() {
    // The review: `title`, `Page.name` and `Page.name_u` were `String`s. A reader may hand over 16 MiB of each (the limit on a page name is
    // for the write that sets one), and every undo step of an edit to that page copied them: 16 MB a step, 16 GB over 1,000. They are now
    // `Arc<str>`, and the test counts what is shared: the same allocation in the document and in the step that kept it.
    let big_text = || "x".repeat(16 * 1024 * 1024);
    let mut doc = DocBuilder::new().build();
    doc.title = big_text().into();
    let mut page = Page::new(1, "p");
    page.name = big_text().into();
    page.name_u = Some(big_text().into());
    page.shapes = std::iter::once(Arc::new(Shape::new(4))).collect();
    doc.pages.push(Arc::new(page));
    let (before, after) = (doc.clone(), {
        let mut edited = doc.clone();
        write_one_at(&mut edited, "5");
        edited
    });
    for (name, kept, copy) in [
        ("title", before.title.as_ptr(), after.title.as_ptr()),
        ("page name", before.pages.get(0).unwrap().name.as_ptr(), after.pages.get(0).unwrap().name.as_ptr()),
        (
            "page name_u",
            before.pages.get(0).unwrap().name_u.as_deref().map(str::as_ptr).unwrap_or(std::ptr::null()),
            after.pages.get(0).unwrap().name_u.as_deref().map(str::as_ptr).unwrap_or(std::ptr::null()),
        ),
    ] {
        assert_eq!(kept, copy, "the {name} of the older version is the very text of the newer");
    }
    assert!(!Arc::ptr_eq(before.pages.get(0).unwrap(), after.pages.get(0).unwrap()), "the page itself was copied: the edit was to a shape of it");
}

/// Writes a value to the one shape of page 1.
fn write_one_at(doc: &mut Document, value: &str) {
    let at = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] });
    doc.write_cell(&at, &CellAddr::direct("Width"), CellEdit::Value(Value::from_text(value)), WriteOrigin::Automation).unwrap();
}
