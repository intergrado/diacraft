//! **The one count of what documents hold between them**, and what an edit adds and what dropping a document frees, which the session keeps its
//! count by. The three are held to each other: the documents of a history, counted whole, are the sum of what each edit added; and a document let go of frees exactly what the
//! count of the others is less than the count of all.
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Document, Page, RawElement, RawNode, Row, Section, Shape, ShapeList, Sheet, Text, TextPart, Value};
use diacraft_sheetxml::model::{Added, Released, allocations_walked, grown, held_bytes_of, whole_walks};
use proptest::prelude::*;

fn v(text: &str) -> Cell {
    Cell::constant(Value::from_text(text))
}

/// A shape with a sheet of `cells` cells, a section of `rows` rows, and a text.
fn shape(id: u32, cells: usize, rows: usize) -> Shape {
    let mut sheet = Sheet::default();
    for k in 0..cells {
        sheet.cells.insert(CellName::new(&format!("C{k}")), v(&format!("value {id} {k}")));
    }
    let mut section = Section::new("Geometry", Some(0));
    for r in 0..rows {
        let mut row = Row::default();
        row.cells.insert(CellName::new("X"), v(&format!("{id}.{r}")));
        section.rows.push(Arc::new(row));
    }
    sheet.sections.push(Arc::new(section));
    let mut shape = Shape::new(id);
    shape.sheet = Arc::new(sheet);
    shape.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(format!("label {id}").as_str()))], ..Text::default() }));
    shape
}

fn drawing(shapes: usize) -> Document {
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec((0..shapes).map(|i| Arc::new(shape(u32::try_from(i + 1).unwrap(), 4, 3))).collect());
    doc.pages.push(Arc::new(page));
    doc
}

fn held(docs: &[&Document]) -> u64 {
    held_bytes_of(docs.iter().copied(), u64::MAX)
}

fn edited(doc: &Document, change: impl FnOnce(&mut Page)) -> Document {
    let mut next = doc.clone();
    change(Arc::make_mut(next.pages.get_mut(0).expect("the page")));
    next
}

#[test]
fn documents_are_counted_between_them_each_allocation_once_and_the_same_document_twice_is_one() {
    let a = drawing(100);
    let alone = held(&[&a]);
    assert!(alone > 0);
    // A copy of the document holds nothing of its own: it is the same allocations.
    let copy = a.clone();
    assert_eq!(held(&[&a, &copy]), alone);
    assert_eq!(held(&[&a, &a]), alone, "the same document given twice is counted once");
    // An edit of one shape's cell: the page, a chunk of the shapes, the shape, its sheet and the new string are held beside the old ones, and nothing else.
    let b = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(7).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C0"), v("a new value of the cell"));
    });
    let both = held(&[&a, &b]);
    assert!(both > alone && both < alone + alone / 10, "{both} against {alone}");
    assert_eq!(both - alone, grown(&a, &b, u64::MAX).bytes, "what the edit added is what the two hold more than the one");
}

#[test]
fn the_same_document_given_twice_is_counted_once_even_when_nothing_else_holds_its_allocations() {
    // No copy of the document is held, so each allocation has one holder and only the address of the document tells the second pass from a new one.
    let solo = drawing(10);
    assert_eq!(held(&[&solo, &solo]), held(&[&solo]));
}

#[test]
fn the_rows_that_two_copies_of_a_section_share_are_counted_once() {
    // One shape with a section of 200 rows; an edit of the cell of one row copies the section and that row, and the other 199 rows are the very allocations in both.
    let mut a = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape(1, 2, 200))]);
    a.pages.push(Arc::new(page));
    let b = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = Arc::make_mut(sheet.sections.get_mut(0).expect("the section"));
        Arc::make_mut(section.rows.get_mut(7).expect("a row")).cells.insert(CellName::new("X"), v("changed"));
    });
    let (alone, both) = (held(&[&a]), held(&[&a, &b]));
    assert!(both - alone < alone / 10, "the 199 rows that are in both are counted once: {both} against {alone}");
    assert_eq!(both - alone, grown(&a, &b, u64::MAX).bytes);
}

#[test]
fn a_string_that_a_copy_of_a_cell_shares_is_counted_once_and_the_characters_of_a_new_one_are_charged() {
    let mut a = drawing(2);
    let big = "x".repeat(1 << 20);
    let page = Arc::make_mut(a.pages.get_mut(0).expect("the page"));
    let first = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
    Arc::make_mut(&mut first.sheet).cells.insert(CellName::new("Comment"), v(&big));
    let small = held(&[&drawing(2)]);
    let with = held(&[&a]);
    assert!(with >= small + (1 << 20), "the characters of the value are charged: {with} against {small}");
    // Another cell that shares the very string holds nothing more of it.
    let mut b = a.clone();
    let page = Arc::make_mut(b.pages.get_mut(0).expect("the page"));
    let second = Arc::make_mut(page.shapes.get_mut(1).expect("a shape"));
    let shared = a.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cells.get(&CellName::new("Comment")).unwrap().clone();
    Arc::make_mut(&mut second.sheet).cells.insert(CellName::new("Comment"), shared);
    assert!(held(&[&b]) < with + (1 << 19), "the same string in a second cell is one allocation: {} against {with}", held(&[&b]));
    // An edit that puts a new value in the cell adds its characters.
    let c = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), v(&"y".repeat(1 << 20)));
    });
    assert!(grown(&a, &c, u64::MAX).bytes >= 1 << 20, "a value the edit made is charged its characters");
    // An edit of another cell of the sheet leaves the value shared, and charges none of it.
    let d = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C0"), v("short"));
    });
    assert!(
        grown(&a, &d, u64::MAX).bytes < 1 << 16,
        "the value that was copied with its sheet is shared with the old one: {}",
        grown(&a, &d, u64::MAX).bytes
    );
}

#[test]
fn a_formula_and_a_text_that_an_edit_copies_with_their_sheet_and_shape_are_not_charged_again() {
    // A formula of a megabyte in one cell, and a text of two parts, the first a megabyte: an edit of another cell and of the second part copies the sheet and the text, and the
    // formula and the first part are the very allocations in both.
    let mut a = drawing(3);
    let big = format!("\"{}\"", "f".repeat(1 << 20));
    let page = Arc::make_mut(a.pages.get_mut(0).expect("the page"));
    let first = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
    Arc::make_mut(&mut first.sheet).cells.insert(CellName::new("F"), Cell::with_formula(&big, None));
    first.text = Some(Arc::new(Text {
        parts: vec![TextPart::Chars(Arc::from("p".repeat(1 << 20).as_str())), TextPart::Chars(Arc::from("tail"))],
        ..Text::default()
    }));
    assert!(held(&[&a]) >= 2 << 20, "the characters of both are charged once: {}", held(&[&a]));
    let b = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C0"), v("other"));
        let text = Arc::make_mut(shape.text.as_mut().expect("a text"));
        text.parts[1] = TextPart::Chars(Arc::from("changed"));
    });
    let added = grown(&a, &b, u64::MAX).bytes;
    assert!(added < 64 << 10, "neither the formula nor the first part is charged again: {added}");
    assert_eq!(held(&[&a, &b]) - held(&[&a]), added);
}

#[test]
fn a_shape_that_an_edit_put_in_another_place_is_not_new_and_none_of_what_it_holds_is_looked_at() {
    // A z-order change, a delete that shifts the others, and a group that takes two into a list of its own: the shapes are the allocations the document the edit started from holds, in
    // other places. None is counted, none is doubtful, and the sheets of the 2,000 are not looked at (the walk is of the chunks of the list that changed, one look for each shape).
    let a = drawing(2_000);
    let whole = {
        let before = allocations_walked();
        held(&[&a]);
        allocations_walked() - before
    };
    let to_front = edited(&a, |page| {
        let shape = page.shapes.remove(10).expect("a shape");
        page.shapes.push(shape);
    });
    let deleted = edited(&a, |page| {
        page.shapes.remove(20);
    });
    let grouped = edited(&a, |page| {
        let (x, y) = (page.shapes.remove(40).expect("a shape"), page.shapes.remove(40).expect("the next"));
        let mut group = Shape::new(9_999);
        group.children = ShapeList::from_vec(vec![x, y]);
        page.shapes.push(Arc::new(group));
    });
    for (name, next) in [("order", &to_front), ("delete", &deleted), ("group", &grouped)] {
        let before = allocations_walked();
        let added = grown(&a, next, u64::MAX);
        let walked = allocations_walked() - before;
        assert!(!added.doubtful && added.doubt_bytes == 0, "{name}: {added:?}");
        assert!(walked < whole / 3, "{name}: looked at {walked} of the {whole} allocations of the drawing");
        // What it added is what the two hold more than the one (the page, and a group's own shape and sheet).
        assert_eq!(held(&[&a, next]) - held(&[&a]), added.bytes, "{name}");
        assert!(added.bytes < 16 << 10, "{name}: {}", added.bytes);
    }
    // A shape that is a copy of one, with the sheet the first holds, is new, and the sheet it shares is the doubtful part of it: counted, as held once.
    let copied = edited(&a, |page| {
        let mut copy = Shape::clone(page.shapes.get(5).expect("a shape"));
        copy.id = 8_888;
        page.shapes.push(Arc::new(copy));
    });
    let added = grown(&a, &copied, u64::MAX);
    let more = held(&[&a, &copied]) - held(&[&a]);
    assert!(added.doubtful && added.doubt_bytes > 0 && added.doubt_bytes < added.bytes, "{added:?}");
    assert!(added.bytes > more, "the sheet it shares is counted again: it is the most the count is more by ({:?})", added.doubt_bytes);
    assert!(added.bytes - added.doubt_bytes <= more, "and nothing else of it is");
}

#[test]
fn a_string_that_a_cell_shares_with_a_cell_elsewhere_is_the_doubt_of_what_an_edit_adds() {
    // A value of a megabyte in one shape, and the very string put in the cell of another shape (a paste of a value): the string is held once, and the walk of the new sheet meets it as new.
    let mut a = drawing(3);
    let big = v(&"s".repeat(1 << 20));
    let page = Arc::make_mut(a.pages.get_mut(0).expect("the page"));
    Arc::make_mut(&mut Arc::make_mut(page.shapes.get_mut(0).expect("a shape")).sheet).cells.insert(CellName::new("Comment"), big.clone());
    let b = edited(&a, |page| {
        Arc::make_mut(&mut Arc::make_mut(page.shapes.get_mut(1).expect("a shape")).sheet).cells.insert(CellName::new("Comment"), big.clone());
    });
    let added = grown(&a, &b, u64::MAX);
    assert!(added.doubtful && added.doubt_bytes >= 1 << 20, "{added:?}");
    assert!(held(&[&a, &b]) - held(&[&a]) < 64 << 10, "the string is held once");
}

#[test]
fn what_an_edit_adds_stops_at_the_figure_it_is_asked_for_and_says_when_it_may_have_met_what_was_counted() {
    let a = drawing(200);
    // A shape that is a copy of one holds the sheet and the text of the first: new as a shape, and its sheet and its text are what the count may be more by.
    let copied = edited(&a, |page| {
        let mut copy = Shape::clone(page.shapes.get(0).expect("a shape"));
        copy.id = 7_777;
        page.shapes.push(Arc::new(copy));
    });
    let Added { bytes, doubtful, .. } = grown(&a, &copied, u64::MAX);
    assert!(doubtful, "what a new shape shares with one that is held is met as new");
    assert!(bytes > held(&[&copied]) - held(&[&a]), "an addition never counts a free: {bytes}");
    // A cell written in one shape is nothing of the kind.
    let written = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(3).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C1"), v("written"));
    });
    assert!(!grown(&a, &written, u64::MAX).doubtful, "every allocation of the edit is new");
    // A figure under the whole stops the walk past it.
    let many = edited(&a, |page| {
        for k in 0..20 {
            page.shapes.push(Arc::new(shape(5_000 + k, 4, 3)));
        }
    });
    let whole = grown(&a, &many, u64::MAX).bytes;
    let early = grown(&a, &many, whole / 4).bytes;
    assert!(early > whole / 4 && early < whole, "{early} of {whole}");
}

#[test]
fn a_document_that_is_let_go_of_frees_what_no_other_holds_and_the_documents_are_not_changed() {
    let a = drawing(100);
    let b = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(5).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C0"), v("only in b"));
    });
    let c = edited(&b, |page| {
        page.shapes.remove(9);
    });
    let (hall, hbc, hc) = (held(&[&a, &b, &c]), held(&[&b, &c]), held(&[&c]));
    let mut released = Released::new();
    let freed_a = released.release(&a).bytes;
    assert_eq!(freed_a, hall - hbc, "what only a holds");
    let freed_b = released.release(&b).bytes;
    assert_eq!(freed_a + freed_b, hall - hc);
    let freed_c = released.release(&c).bytes;
    assert_eq!(freed_a + freed_b + freed_c, hall, "all of them let go of: all of it freed");
    // The documents are as they were.
    assert_eq!(held(&[&a, &b, &c]), hall);
    // The order that they are let go of in does not change the total, and a release that has nothing to free frees nothing.
    let mut backwards = Released::new();
    let total: u64 = [&c, &b, &a].into_iter().map(|d| backwards.release(d).bytes).sum();
    assert_eq!(total, hall);
}

#[test]
fn a_document_that_something_else_holds_is_not_freed_by_letting_go_of_its_other_holder() {
    // `b` is a copy of `a`: letting go of one frees nothing, the other holds all of it; and it is when the last is let go of that the allocations are freed.
    let a = drawing(50);
    let b = a.clone();
    let mut released = Released::new();
    assert_eq!(released.release(&a).bytes, 0);
    assert_eq!(released.release(&b).bytes, held(&[&a]));
}

#[test]
fn counting_whole_is_counted_and_what_an_edit_does_to_the_count_is_not_a_walk_of_the_drawing() {
    let a = drawing(2_000);
    let before = whole_walks();
    let _ = held(&[&a]);
    assert_eq!(whole_walks(), before + 1);
    let b = edited(&a, |page| {
        let shape = Arc::make_mut(page.shapes.get_mut(1_000).expect("a shape"));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("C0"), v("moved"));
    });
    let mut released = Released::new();
    let (added, freed) = (grown(&a, &b, u64::MAX), released.release(&a).bytes);
    assert_eq!(whole_walks(), before + 1, "neither is a count of the documents whole");
    assert!(added.bytes < 64 << 10, "a chunk of the shapes, one shape and its sheet: {}", added.bytes);
    assert_eq!(freed, held(&[&a, &b]) - held(&[&b]), "the document b holds the rest, and what only a holds is the old page, shape, sheet and string");
    assert!(freed < 64 << 10, "{freed}");
}

// ------------------------------------------------------------------------------------------------------------------------------- what an object keeps without a field

fn connect(n: u32) -> Connect {
    Connect {
        from_sheet: n,
        from_cell: Some(Arc::from(format!("Connections.X{n}").as_str())),
        to_sheet: n + 1,
        to_cell: Some(Arc::from("PinX")),
        ..Connect::default()
    }
}

#[test]
fn what_an_object_keeps_without_a_field_is_in_the_count_with_its_characters_and_is_counted_once_however_many_copies_hold_it() {
    let base = drawing(3);
    let kept_text = "k".repeat(10_000);
    let attr_value = "a".repeat(10_000);
    let with = edited(&base, |page| {
        page.raw.push("future-attr", &attr_value);
        page.passthrough.push(0, RawNode::Text(Arc::from(kept_text.as_str())));
        page.passthrough.push(
            1,
            RawNode::Element(RawElement {
                name: Arc::from("x:element"),
                attrs: diacraft_doc::RawAttrs::from_pairs([("x:a", "1")]),
                children: vec![RawNode::Comment(Arc::from("c".repeat(5_000).as_str()))],
            }),
        );
        for n in 0..20 {
            page.connects.push(connect(n));
        }
    });
    let (a, b) = (held(&[&base]), held(&[&with]));
    // The characters of an attribute's value, a kept text and a kept comment: 25,000 at the least, and the connects (an element's price each) besides.
    assert!(b >= a + 25_000 + 20 * diacraft_sheetxml::model::cost::element_bytes("connect"), "{b} against {a}");
    // A copy of the page that an edit made holds the same lists: they are counted once between the two documents.
    let copy = edited(&with, |page| page.id = 7);
    let both = held(&[&with, &copy]);
    assert!(both < b + 4_096, "the lists of a page are counted once however many copies of the page hold them: {both} against {b}");
    // What an edit of one shape's attribute adds is the list it copied, and letting go of the first document frees the list that nothing else holds.
    let shaped = edited(&with, |page| {
        if let Some(s) = page.shapes.get_mut(0) {
            Arc::make_mut(s).raw.push("shape-future", &"s".repeat(7_000));
        }
    });
    assert!(held(&[&shaped]) >= b + 7_000);
    let added = grown(&with, &shaped, u64::MAX);
    assert!(added.bytes >= 7_000 && added.bytes < 7_000 + 4_096, "the edit added the attribute and not the drawing: {}", added.bytes);
    let mut released = Released::new();
    assert!(released.release(&with).bytes < 4_096 + 7_000, "the shape's list is held by the other document, and what the first held alone is freed");
}

/// What an edit of a generated sequence does to a document: a cell written, a value shared from another cell, a shape taken out, a shape put in twice, a row's cell written, a
/// page renamed, a shape moved to the end.
#[derive(Clone, Debug)]
enum Op {
    Write(usize, u8),
    Share(usize, usize),
    Remove(usize),
    Duplicate(usize),
    Row(usize, u8),
    Rename,
    ToEnd(usize),
    Attr(usize, u8),
    Kept(usize, u8),
    Connect(u8),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0usize..40, 0u8..5).prop_map(|(i, k)| Op::Write(i, k)),
        2 => (0usize..40, 0usize..40).prop_map(|(i, j)| Op::Share(i, j)),
        1 => (0usize..40).prop_map(Op::Remove),
        2 => (0usize..40).prop_map(Op::Duplicate),
        2 => (0usize..40, 0u8..5).prop_map(|(i, k)| Op::Row(i, k)),
        1 => Just(Op::Rename),
        1 => (0usize..40).prop_map(Op::ToEnd),
        1 => (0usize..40, 0u8..5).prop_map(|(i, k)| Op::Attr(i, k)),
        1 => (0usize..40, 0u8..5).prop_map(|(i, k)| Op::Kept(i, k)),
        1 => (0u8..5).prop_map(Op::Connect),
    ]
}

fn apply(doc: &Document, op: &Op) -> Document {
    edited(doc, |page| {
        let n = page.shapes.len().max(1);
        match op {
            Op::Write(i, k) => {
                if let Some(s) = page.shapes.get_mut(i % n) {
                    Arc::make_mut(&mut Arc::make_mut(s).sheet).cells.insert(CellName::new(&format!("C{k}")), v(&format!("written {i} {k}")));
                }
            }
            Op::Share(i, j) => {
                let taken = page.shapes.get(j % n).and_then(|s| s.sheet.cells.get(&CellName::new("C0")).cloned());
                if let (Some(taken), Some(s)) = (taken, page.shapes.get_mut(i % n)) {
                    Arc::make_mut(&mut Arc::make_mut(s).sheet).cells.insert(CellName::new("C0"), taken);
                }
            }
            Op::Remove(i) => {
                page.shapes.remove(i % n);
            }
            Op::Duplicate(i) => {
                if let Some(s) = page.shapes.get(i % n).cloned() {
                    page.shapes.push(s);
                }
            }
            Op::Row(i, k) => {
                if let Some(s) = page.shapes.get_mut(i % n) {
                    let sheet = Arc::make_mut(&mut Arc::make_mut(s).sheet);
                    if let Some(section) = sheet.sections.get_mut(0)
                        && let Some(row) = Arc::make_mut(section).rows.get_mut(usize::from(*k) % 3)
                    {
                        Arc::make_mut(row).cells.insert(CellName::new("X"), v(&format!("row {i} {k}")));
                    }
                }
            }
            Op::Rename => page.name = Arc::from("renamed"),
            Op::ToEnd(i) => {
                if let Some(s) = page.shapes.remove(i % n) {
                    page.shapes.push(s);
                }
            }
            Op::Attr(i, k) => {
                if let Some(s) = page.shapes.get_mut(i % n) {
                    Arc::make_mut(s).raw.push(&format!("future-{k}"), &format!("value {i} {k}"));
                }
            }
            Op::Kept(i, k) => {
                if let Some(s) = page.shapes.get_mut(i % n) {
                    Arc::make_mut(s).passthrough.push(u32::from(*k), RawNode::Text(Arc::from(format!("kept {i} {k}").as_str())));
                }
            }
            Op::Connect(k) => page.connects.push(connect(u32::from(*k))),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// A history of documents, each the one before with an edit: **the count kept by adding what each edit added is never less than the count of them whole**, and is it when no
    /// edit was doubtful; and **letting go of them in order frees exactly what the count of the rest is less than the count of all**.
    #[test]
    fn what_edits_add_and_what_letting_go_frees_are_the_count_of_the_documents_whole(
        size in 1usize..40,
        ops in prop::collection::vec(op(), 1..14),
    ) {
        let mut docs = vec![drawing(size)];
        for o in &ops {
            let next = apply(docs.last().unwrap(), o);
            docs.push(next);
        }
        let refs: Vec<&Document> = docs.iter().collect();
        // Added: from the first document by what each edit added.
        let mut kept = held(&refs[..1]);
        let mut doubtful = false;
        for pair in refs.windows(2) {
            let added = grown(pair[0], pair[1], u64::MAX);
            kept += added.bytes;
            doubtful |= added.doubtful;
        }
        let whole = held(&refs);
        prop_assert!(kept >= whole, "a count under what is held: {kept} against {whole} after {ops:?}");
        if !doubtful {
            prop_assert_eq!(kept, whole, "no edit was doubtful, so the sum is the count: {:?}", ops);
        }
        // Let go: in order, and in the other order.
        let mut released = Released::new();
        let mut freed = 0u64;
        for (k, d) in refs.iter().enumerate() {
            freed += released.release(d).bytes;
            prop_assert_eq!(freed, whole - held(&refs[k + 1..]), "after {} let go of, in {:?}", k + 1, ops);
        }
        let mut backwards = Released::new();
        let mut freed = 0u64;
        for (k, d) in refs.iter().enumerate().rev() {
            freed += backwards.release(d).bytes;
            prop_assert_eq!(freed, whole - held(&refs[..k]), "backwards, after {}, in {:?}", k, ops);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- a floor

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// **The count of what a model holds is a floor of what an open charges for it**: a drawing that the count refuses is one the open refuses. Over every generated drawing (which keeps
    /// attributes and nodes, and has connects), what the decode of its file is charged in bytes is at least the count of the model it makes.
    #[test]
    fn what_a_model_holds_counted_is_never_more_than_what_the_open_of_its_file_is_charged(m in diacraft_testkit::model::any_document(diacraft_testkit::model::ModelSize::Small)) {
        let packed = super::model_util::pack(&m);
        let budget = diacraft_sheetxml::model::Budget::new(1 << 40, 1 << 40, 1 << 40);
        let mut left = budget.clone();
        let (back, _) = super::model_util::unpack_with(&packed, &packed.blobs, &mut left).unwrap();
        let charged = (1u64 << 40) - left.bytes;
        let counted = diacraft_sheetxml::model::held_bytes(&back);
        prop_assert!(counted <= charged, "counted {counted} of a model the open charged {charged} for");
    }
}
