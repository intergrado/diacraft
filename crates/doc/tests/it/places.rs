//! **The writers of the model keep the place of every unknown node**: a node goes on following the child it followed.
//!
//! The model keeps an unknown node (a foreign element, a comment) as the count of typed children that stood before it (`Placed::after`), and a model writer puts it back after that many
//! (`crates/doc/src/places.rs`). The writers that put a cell, a row or a section in, or take one out, move the count: `Sheet::section_mut`, `Sheet::row_mut`, `Sheet::set_cell`,
//! `Sheet::remove_cell`, `Section::insert_row`, `Section::remove_rows`, `Section::retain_rows`, `Section::sort_rows_by_key`, `Sheet::insert_section` and `Sheet::remove_sections` for
//! the nodes of the sheet, the section and the row; and `Counted` (got from a document, a shape or a style) for the nodes of the shape or the style that holds the sheet as well.
//! The `Shapes` list of a page, a master or a group is kept the same way by `Page::push_shape` and its kin.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, Cells, Counted, DocError, Document, Kept, Owner, Passthrough, Placed, RawNode, Row, RowKey, Rows, Section,
    SectionKey, Shape, ShapeRef, Sheet, SheetRef, Value, WriteOrigin, cell_names, cells_changed, owner_places_after, owner_places_after_where,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

/// Unknown nodes that stood after these counts of typed children, each a comment named by its place in the list.
fn nodes(afters: &[u32]) -> Passthrough {
    Passthrough::from_nodes(
        afters.iter().enumerate().map(|(k, after)| Placed { after: *after, node: RawNode::Comment(Arc::from(format!("n{k}"))) }).collect(),
    )
}

fn places(nodes: &Passthrough) -> Vec<u32> {
    nodes.iter().map(|placed| placed.after).collect()
}

/// The names of the nodes in their order.
fn names(nodes: &Passthrough) -> Vec<String> {
    nodes
        .iter()
        .map(|placed| match &placed.node {
            RawNode::Comment(text) => text.to_string(),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn row(ix: u32) -> Arc<Row> {
    Arc::new(Row { ix: Some(ix), ..Row::default() })
}

/// A section of `rows` rows numbered from 0, with unknown nodes that stood after these counts of its children.
fn section_with(rows: u32, afters: &[u32]) -> Section {
    let mut section = Section::new("Scratch", None);
    for ix in 0..rows {
        section.rows.push(row(ix));
    }
    section.passthrough = nodes(afters);
    section
}

fn numbers(section: &Section) -> Vec<u32> {
    section.rows.iter().filter_map(|row| row.ix).collect()
}

fn names_of(list: &[&str]) -> Vec<CellName> {
    list.iter().map(|n| CellName::new(n)).collect()
}

fn cells_of(list: &[&str]) -> Cells {
    list.iter().map(|n| (CellName::new(n), Cell::default())).collect()
}

fn key(name: &str) -> SectionKey {
    SectionKey::new(name, None)
}

// ---------------------------------------------------------------------------------------------------------------------------------- rows

#[test]
fn a_row_put_in_goes_where_it_is_asked_or_at_the_end_and_the_nodes_after_the_row_before_it_follow_it() {
    let mut section = section_with(3, &[0, 1, 3]);
    section.insert_row(99, row(9));
    assert_eq!(numbers(&section), [0, 1, 2, 9], "past the end is the end");
    assert_eq!(places(&section.passthrough), [0, 1, 4], "the node that trailed the last row trails the new one: it stands after the row put in");
    let mut section = section_with(3, &[0, 1, 3]);
    section.insert_row(0, row(9));
    assert_eq!(numbers(&section), [9, 0, 1, 2]);
    assert_eq!(places(&section.passthrough), [0, 2, 4], "the node before the first row stays before it; the others follow the rows they followed");
}

#[test]
fn a_row_appended_goes_ahead_of_the_node_that_trailed_the_rows() {
    let mut section = section_with(2, &[0, 2]);
    section.append_row(row(7));
    assert_eq!(numbers(&section), [0, 1, 7]);
    assert_eq!(places(&section.passthrough), [0, 3]);
}

#[test]
fn rows_taken_out_by_their_places_leave_the_others_in_order_and_the_nodes_with_the_row_before() {
    let mut section = section_with(5, &[0, 1, 3, 5]);
    section.remove_rows(&[1, 3]);
    assert_eq!(numbers(&section), [0, 2, 4]);
    assert_eq!(places(&section.passthrough), [0, 1, 2, 3]);
    // The cells of a section come before its rows and are not moved.
    let mut with_cells = section_with(3, &[1, 2, 3, 4]);
    with_cells.cells.insert(CellName::new("NoFill"), Cell::default());
    with_cells.cells.insert(CellName::new("NoLine"), Cell::default());
    with_cells.remove_rows(&[0]);
    assert_eq!(numbers(&with_cells), [1, 2]);
    assert_eq!(places(&with_cells.passthrough), [1, 2, 2, 3]);
    // None to take out is no change.
    with_cells.remove_rows(&[]);
    assert_eq!(numbers(&with_cells), [1, 2]);
}

#[test]
fn rows_kept_by_a_test_are_the_rows_the_others_were_taken_out_of_in_one_pass() {
    let mut section = section_with(6, &[0, 2, 4, 6]);
    let mut asked = 0;
    section.retain_rows(|row| {
        asked += 1;
        row.ix.is_some_and(|ix| ix % 2 == 0)
    });
    assert_eq!((numbers(&section), asked), (vec![0, 2, 4], 6), "each row is asked once");
    assert_eq!(
        places(&section.passthrough),
        [0, 1, 2, 3],
        "the node after row 1 goes with row 0, the one after row 3 with row 2, the one after row 5 with row 4"
    );
    // Nothing to take out is no change, and no node.
    let mut kept = section_with(3, &[1]);
    kept.retain_rows(|_| true);
    assert_eq!((numbers(&kept), places(&kept.passthrough)), (vec![0, 1, 2], vec![1]));
    let mut plain = section_with(3, &[]);
    plain.retain_rows(|row| row.ix != Some(1));
    assert_eq!(numbers(&plain), [0, 2]);
}

#[test]
fn rows_put_in_order_by_a_sort_take_the_nodes_that_followed_them() {
    // Rows numbered 2, 0, 1 (a row a write made came after the rows the sheet held), with a node before the first, after each.
    let mut section = Section::new("Scratch", None);
    for ix in [2, 0, 1] {
        section.rows.push(row(ix));
    }
    section.passthrough = nodes(&[0, 1, 2, 3]);
    section.sort_rows_by_key(|row| row.ix);
    assert_eq!(numbers(&section), [0, 1, 2]);
    // The node after row 2 (n1) is after it still, at the end; the one after row 0 (n2) is after it, and the one after row 1 (n3); the node before the first stays there.
    assert_eq!(names(&section.passthrough), ["n0", "n2", "n3", "n1"]);
    assert_eq!(places(&section.passthrough), [0, 1, 2, 3]);
    // Rows in order are not touched, and the list of nodes is not copied.
    let before = section.passthrough.shared_list().map(Arc::as_ptr);
    section.sort_rows_by_key(|row| row.ix);
    assert_eq!(section.passthrough.shared_list().map(Arc::as_ptr), before);
}

#[test]
fn a_sort_of_rows_that_hold_a_section_s_cells_leaves_the_nodes_among_the_cells() {
    let mut section = Section::new("Geometry", Some(0));
    section.cells.insert(CellName::new("NoFill"), Cell::default());
    for ix in [1, 0] {
        section.rows.push(row(ix));
    }
    // Before the cell (0), after the cell (1), after the row 1 (2), after row 0 (3).
    section.passthrough = nodes(&[0, 1, 2, 3]);
    section.sort_rows_by_key(|row| row.ix);
    assert_eq!(numbers(&section), [0, 1]);
    assert_eq!(names(&section.passthrough), ["n0", "n1", "n3", "n2"]);
    assert_eq!(places(&section.passthrough), [0, 1, 2, 3]);
}

// ------------------------------------------------------------------------------------------------------- sections and the nodes of a sheet

#[test]
fn a_section_made_for_a_write_goes_after_the_others_and_ahead_of_the_node_that_trailed_them() {
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("PinX"), Cell::default());
    sheet.section_mut(&key("User")).unwrap();
    sheet.section_mut(&key("Scratch")).unwrap();
    // One cell and two sections: a node before the cell (0), one after it (1), one between the sections (2), one after the last section (3).
    sheet.passthrough = nodes(&[0, 1, 2, 3]);
    sheet.section_mut(&key("Hyperlink")).unwrap();
    assert_eq!(sheet.sections.len(), 3);
    assert_eq!(places(&sheet.passthrough), [0, 1, 2, 4], "the node that trailed the sections trails the new one");
    // A section the sheet holds is found and nothing moves.
    sheet.section_mut(&key("User")).unwrap();
    assert_eq!(places(&sheet.passthrough), [0, 1, 2, 4]);
}

#[test]
fn a_row_made_by_row_mut_makes_its_section_and_moves_the_nodes_of_the_sheet_and_of_the_section() {
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("PinX"), Cell::default());
    sheet.section_mut(&key("User")).unwrap();
    // The sheet: after the cell (1), after the section (2). The section User holds none yet.
    sheet.passthrough = nodes(&[1, 2]);
    let section = Arc::make_mut(sheet.sections.first_mut().unwrap());
    section.rows.push(row(0));
    section.passthrough = nodes(&[0, 1]);
    // A row of the section it has: the section's node that trailed the rows trails the new one.
    sheet.row_mut(&key("User"), &RowKey::Ix(5)).unwrap();
    assert_eq!(places(&sheet.sections.first().unwrap().passthrough), [0, 2]);
    assert_eq!(places(&sheet.passthrough), [1, 2], "no section was made");
    // A row of a section it lacks makes the section, ahead of the node that trailed the sections.
    sheet.row_mut(&key("Scratch"), &RowKey::Ix(0)).unwrap();
    assert_eq!(sheet.sections.len(), 2);
    assert_eq!(places(&sheet.passthrough), [1, 3]);
}

#[test]
fn sections_taken_out_leave_the_nodes_that_followed_them_with_the_section_before() {
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("PinX"), Cell::default());
    for name in ["User", "Scratch", "Prop"] {
        sheet.section_mut(&key(name)).unwrap();
    }
    // After the cell (1), after User (2), after Scratch (3), after Prop (4).
    sheet.passthrough = nodes(&[1, 2, 3, 4]);
    let gone = sheet.remove_sections(|section| section.name.as_str() == "Scratch");
    assert_eq!(gone, [1]);
    assert_eq!(sheet.sections.len(), 2);
    assert_eq!(places(&sheet.passthrough), [1, 2, 2, 3]);
    assert!(sheet.remove_sections(|section| section.name.as_str() == "Nothing of the kind").is_empty());
    assert_eq!(places(&sheet.passthrough), [1, 2, 2, 3], "nothing to take out is no change");
}

#[test]
fn a_section_put_in_at_a_place_goes_ahead_of_the_nodes_that_trailed_the_one_before() {
    let mut sheet = Sheet::default();
    for name in ["User", "Scratch"] {
        sheet.section_mut(&key(name)).unwrap();
    }
    sheet.passthrough = nodes(&[0, 1, 2]);
    let at = sheet.insert_section(1, Arc::new(Section::new("Prop", None)));
    assert_eq!((at, sheet.sections.len()), (1, 3));
    assert_eq!(places(&sheet.passthrough), [0, 2, 3]);
    let at = sheet.push_section(Arc::new(Section::new("Controls", None)));
    assert_eq!((at, places(&sheet.passthrough)), (3, vec![0, 2, 4]));
}

// ------------------------------------------------------------------------------------------------------------------------------- cells

#[test]
fn a_cell_put_in_among_the_cells_of_an_element_goes_ahead_of_the_nodes_that_stood_after_the_cell_before_it() {
    let mut sheet = Sheet::default();
    for name in ["B", "D"] {
        sheet.cells.insert(CellName::new(name), Cell::default());
    }
    // Before the first cell (0), between the cells (1), after the cells (2).
    sheet.passthrough = nodes(&[0, 1, 2]);
    for name in ["A", "C", "E"] {
        assert!(sheet.set_cell(&CellAddr::direct(name), Cell::default()).unwrap().is_none());
    }
    assert_eq!(places(&sheet.passthrough), [0, 3, 5], "the node before the cells stays, the others follow the cell they followed");
    // A cell that is replaced moves nothing.
    assert!(sheet.set_cell(&CellAddr::direct("C"), Cell::default()).unwrap().is_some());
    assert_eq!(places(&sheet.passthrough), [0, 3, 5]);
    // Cells taken out: the nodes that stood after them follow the cell before.
    assert!(sheet.remove_cell(&CellAddr::direct("A")).is_some());
    assert!(sheet.remove_cell(&CellAddr::direct("E")).is_some());
    assert_eq!(places(&sheet.passthrough), [0, 2, 3]);
    assert!(sheet.remove_cell(&CellAddr::direct("Nothing")).is_none());
    assert_eq!(places(&sheet.passthrough), [0, 2, 3]);
}

#[test]
fn a_cell_put_in_a_section_or_a_row_moves_the_nodes_of_that_element_and_no_other() {
    let mut sheet = Sheet { passthrough: nodes(&[0]), ..Sheet::default() };
    let mut section = Section { passthrough: nodes(&[0, 1]), ..Section::new("Geometry", Some(0)) };
    section.cells.insert(CellName::new("NoLine"), Cell::default());
    let mut held = Row { ix: Some(1), passthrough: nodes(&[0, 1]), ..Row::default() };
    held.cells.insert(CellName::new("X"), Cell::default());
    section.rows.push(Arc::new(held));
    sheet.sections.push(Arc::new(section));
    // A cell of the section: `NoFill` sorts before `NoLine`, so the node after `NoLine` follows it.
    sheet.set_cell(&CellAddr::in_section("Geometry", Some(0), "NoFill"), Cell::default()).unwrap();
    assert_eq!(places(&sheet.sections.first().unwrap().passthrough), [0, 2]);
    // A cell of the row: `A` goes first, `Y` last.
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "A"), Cell::default()).unwrap();
    assert_eq!(places(&sheet.sections.first().unwrap().rows.first().unwrap().passthrough), [0, 2]);
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "Y"), Cell::default()).unwrap();
    assert_eq!(
        places(&sheet.sections.first().unwrap().rows.first().unwrap().passthrough),
        [0, 3],
        "the node that trailed the cells trails the new one"
    );
    assert_eq!(places(&sheet.passthrough), [0], "the sheet's node is untouched");
    // The cells of a row and of a section can be taken out again.
    assert!(sheet.remove_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "A")).is_some());
    assert_eq!(places(&sheet.sections.first().unwrap().rows.first().unwrap().passthrough), [0, 2]);
    assert!(sheet.remove_cell(&CellAddr::in_section("Geometry", Some(0), "NoFill")).is_some());
    assert_eq!(places(&sheet.sections.first().unwrap().passthrough), [0, 1]);
}

#[test]
fn the_cells_a_section_and_a_row_put_in_by_name_move_the_nodes_among_their_cells() {
    let mut section = Section::new("Geometry", Some(0));
    section.cells.insert(CellName::new("NoLine"), Cell::default());
    section.passthrough = nodes(&[0, 1]);
    assert!(section.put_cell(&CellName::new("NoFill"), Cell::default()).unwrap().is_none());
    assert_eq!(places(&section.passthrough), [0, 2]);
    let mut held = Row::default();
    held.cells.insert(CellName::new("X"), Cell::default());
    held.passthrough = nodes(&[1]);
    held.put_cell(&CellName::new("Y"), Cell::default()).unwrap();
    assert_eq!(places(&held.passthrough), [2]);
    // A row whose cells are replaced: the nodes follow the cells that stay, and a new cell goes ahead of a node that followed the cell before it.
    held.replace_cells(cells_of(&["Y", "Z"]));
    assert_eq!(
        places(&held.passthrough),
        [2],
        "X is gone and Z is new: the node that followed Y stands after Y and after Z, which went in ahead of it"
    );
}

#[test]
fn the_cells_put_in_and_taken_out_among_the_cells_of_an_element_move_the_nodes_by_the_places_in_the_two_lists() {
    let moved = |afters: &[u32], before: &[&str], now: &[&str]| -> Vec<u32> {
        let mut held = nodes(afters);
        cells_changed(&mut held, &names_of(before), &cells_of(now));
        places(&held)
    };
    // `B` and `D` stay, `A` and `C` are put in: the nodes follow the cells they followed.
    assert_eq!(moved(&[0, 1, 2], &["B", "D"], &["A", "B", "C", "D"]), [0, 3, 4]);
    assert_eq!(moved(&[0, 1, 2], &["A", "B", "C", "D"], &["B", "D"]), [0, 0, 1]);
    // Nothing changes: no move.
    assert_eq!(moved(&[0, 1, 2], &["A", "B"], &["A", "B"]), [0, 1, 2]);
    // No node, no names asked for.
    let mut none = Passthrough::new();
    cells_changed(&mut none, &[], &cells_of(&["A"]));
    assert!(none.is_empty());
    assert!(cell_names(&none, &cells_of(&["A", "B"])).is_empty());
    assert_eq!(cell_names(&nodes(&[0]), &cells_of(&["A", "B"])).len(), 2);
}

// ----------------------------------------------------------------------------------------------------------------------- a shape and a style

/// A document with a style 3, a page 1 with a shape 10 and a group 11, a master 1 with a shape and the nodes of every sheet and of the shape and the style that hold them.
fn world() -> Document {
    let mut doc = DocBuilder::new()
        .style(3, StyleBuilder::new())
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("PinX", "1")).shape(ShapeBuilder::new(11).group().child(ShapeBuilder::new(12))))
        .build();
    // The shape's own nodes: one after the cell (1), one after the (not yet any) section (1).
    let shape = Arc::make_mut(Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.get_mut(0).unwrap());
    shape.passthrough = nodes(&[0, 1]);
    let style = Arc::make_mut(doc.styles.get_mut(doc.styles.iter().position(|s| s.id == 3).unwrap()).unwrap());
    style.passthrough = nodes(&[0, 1]);
    doc
}

fn shape_at() -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] })
}

#[test]
fn the_sheet_of_a_shape_and_of_a_style_is_got_with_the_nodes_of_the_element_that_holds_it_and_those_of_a_page_are_the_sheets() {
    let mut doc = world();
    assert!(doc.counted_mut(&shape_at()).is_some_and(|c| c.owner.is_some()));
    assert!(doc.counted_mut(&SheetRef::Style(3)).is_some_and(|c| c.owner.is_some()));
    assert!(doc.counted_mut(&SheetRef::Page(1)).is_some_and(|c| c.owner.is_none()));
    assert!(doc.counted_mut(&SheetRef::Document).is_some_and(|c| c.owner.is_none()));
    assert!(doc.counted_mut(&SheetRef::Page(77)).is_none() && doc.counted_mut(&SheetRef::Style(77)).is_none());
    let missing = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![9] });
    assert!(doc.counted_mut(&missing).is_none());
    let member = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![1, 0] });
    assert!(doc.counted_mut(&member).is_some_and(|c| c.owner.is_some()), "a group's member");
}

#[test]
fn a_section_made_for_a_shape_moves_the_nodes_of_the_sheet_and_of_the_shape_that_holds_it_and_a_row_a_cell_too() {
    let mut doc = world();
    let shape = Arc::make_mut(Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.get_mut(0).unwrap());
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("PinX"), Cell::default());
    sheet.section_mut(&key("User")).unwrap();
    // The shape's nodes count its cell, its section, then the text and the members: after the cell (1), after the section (2), after the text (3).
    sheet.passthrough = nodes(&[1, 2]);
    shape.sheet = Arc::new(sheet);
    shape.passthrough = nodes(&[0, 2, 3]);
    {
        let mut counted = shape.counted_sheet();
        counted.section_mut(&key("Hyperlink")).unwrap();
        assert_eq!(places(&counted.sheet.passthrough), [1, 3], "the sheet's own node that trailed the sections trails the new one");
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 3, 4], "the shape's: the same, and what stood after the sections moves by one");
        // A section the sheet holds is found and nothing moves.
        counted.section_mut(&key("User")).unwrap();
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 3, 4]);
        // A row of a section it lacks makes the section.
        counted.row_mut(&key("Scratch"), &RowKey::Ix(0)).unwrap();
        assert_eq!(places(&counted.sheet.passthrough), [1, 4]);
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 4, 5]);
        // A cell that sorts before `PinX`: the nodes that stood after the cell follow it, in both lists.
        counted.set_cell(&CellAddr::direct("Angle"), Cell::default()).unwrap();
        assert_eq!(places(&counted.sheet.passthrough), [2, 5]);
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 5, 6]);
        // A cell of a row, a section's: no node of the shape's is moved.
        counted.set_cell(&CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A"), Cell::default()).unwrap();
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 5, 6]);
        // The cell is taken out again.
        assert!(counted.remove_cell(&CellAddr::direct("Angle")).is_some());
        assert_eq!(places(&counted.sheet.passthrough), [1, 4]);
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 4, 5]);
        // Two sections taken out, one put in.
        counted.remove_sections(|s| matches!(s.name.as_str(), "Hyperlink" | "Scratch"));
        assert_eq!(places(counted.owner.as_deref().unwrap()), [0, 2, 3]);
        counted.insert_section(0, Arc::new(Section::new("Prop", None)));
        assert_eq!(
            places(counted.owner.as_deref().unwrap()),
            [0, 3, 4],
            "the node that followed the section stands after the one put in ahead of it"
        );
        counted.push_section(Arc::new(Section::new("Controls", None)));
        counted.retain_sections(|s| s.name.as_str() == "User");
        assert_eq!(counted.sheet.sections.len(), 1);
    }
}

#[test]
fn a_cell_a_command_writes_on_a_shape_that_holds_unknown_nodes_moves_them_and_the_other_shapes_are_not_touched() {
    let mut doc = world();
    let (shape, other) = (shape_at(), SheetRef::Style(3));
    // The shape holds its cell `PinX` and nodes after none (0) and after the cell (1).
    let before = doc.clone();
    doc.write_cell(&shape, &CellAddr::direct("Angle"), CellEdit::Value(Value::from_text("30")), WriteOrigin::Automation).unwrap();
    let shape_nodes = |doc: &Document| places(&doc.shape(&ShapeRef { owner: Owner::Page(1), path: vec![0] }).unwrap().passthrough);
    assert_eq!(shape_nodes(&before), [0, 1]);
    assert_eq!(shape_nodes(&doc), [0, 2], "`Angle` sorts before `PinX`, which the second node followed");
    assert_eq!(places(&doc.style(3).unwrap().passthrough), [0, 1], "the style is another sheet");
    // A write that makes a section on a style moves the style's nodes: the new section goes ahead of the nodes that stood right after the cells (the style holds none).
    doc.write_cell(
        &other,
        &CellAddr::in_row("User", None, RowKey::name("Rate"), "Value"),
        CellEdit::Value(Value::from_text("1")),
        WriteOrigin::Automation,
    )
    .unwrap();
    assert_eq!(places(&doc.style(3).unwrap().passthrough), [1, 2]);
    assert_eq!(doc.style(3).unwrap().sheet.sections.len(), 1);
    // The older copy keeps what it had.
    assert_eq!(places(&before.shape(&ShapeRef { owner: Owner::Page(1), path: vec![0] }).unwrap().passthrough), [0, 1]);
    // A refused write changes nothing.
    let refused = doc.write_cell(&shape, &CellAddr::direct(""), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation);
    assert!(matches!(refused, Err(DocError::BadName(_))));
    assert_eq!(shape_nodes(&doc), [0, 2]);
}

#[test]
fn a_list_no_node_counts_is_not_read_by_any_writer() {
    diacraft_doc::work::reset();
    let mut sheet = Sheet::default();
    for k in 0..200 {
        sheet.set_cell(&CellAddr::direct(&format!("C{k:03}")), Cell::default()).unwrap();
        sheet.row_mut(&key("Scratch"), &RowKey::Ix(k)).unwrap();
        sheet.set_cell(&CellAddr::in_row("User", None, RowKey::name(&format!("R{k}")), "Value"), Cell::default()).unwrap();
    }
    assert_eq!(diacraft_doc::work::places_read(), 0);
    let mut shape = Shape::new(1);
    shape.counted_sheet().set_cell(&CellAddr::direct("Angle"), Cell::default()).unwrap();
    shape.counted_sheet().section_mut(&key("User")).unwrap();
    assert_eq!(diacraft_doc::work::places_read(), 0);
}

#[test]
fn a_write_reads_the_nodes_of_each_list_it_moves_once() {
    let mut sheet = Sheet::default();
    for k in 0..50 {
        sheet.cells.insert(CellName::new(&format!("C{k:03}")), Cell::default());
    }
    sheet.passthrough = nodes(&[0, 10, 20, 30, 40, 50]);
    let mut owner = nodes(&[5, 25, 45, 60]);
    diacraft_doc::work::reset();
    let mut counted = Counted { sheet: &mut sheet, owner: Some(&mut owner) };
    counted.set_cell(&CellAddr::direct("C005b"), Cell::default()).unwrap();
    assert_eq!(diacraft_doc::work::places_read(), 6 + 4, "each list once, for the cell put in");
    assert_eq!(places(&sheet.passthrough), [0, 11, 21, 31, 41, 51]);
    assert_eq!(places(&owner), [5, 26, 46, 61]);
}

// --------------------------------------------------------------------------------------------------------------------- lists of shapes

/// A page whose list of shapes `ids` holds unknown nodes after these counts, and the `Shapes` element's own place for them (the slot of the page).
fn page_with(ids: &[u32], afters: &[u32]) -> diacraft_doc::Page {
    let mut page = diacraft_doc::Page::new(1, "Page-1");
    for id in ids {
        page.shapes.push(Arc::new(Shape::new(*id)));
    }
    page.ext.make_mut().vsdx.0.push((Arc::from("Shapes"), Kept { kept: nodes(afters), ..Kept::default() }));
    page
}

fn ids(page: &diacraft_doc::Page) -> Vec<u32> {
    page.shapes.iter().map(|s| s.id).collect()
}

fn kept(page: &diacraft_doc::Page) -> Vec<u32> {
    let held = page.ext.get().and_then(|data| data.vsdx.0.iter().find(|(key, _)| &**key == "Shapes")).map(|(_, kept)| places(&kept.kept));
    held.unwrap_or_default()
}

#[test]
fn a_shape_put_in_a_list_goes_ahead_of_the_node_that_trailed_the_shapes_and_one_taken_out_leaves_its_node_with_the_shape_before() {
    // Shapes 1, 2, 3 with a node before the first (0), after 1 (1), after 2 (2) and after 3 (3).
    let mut page = page_with(&[1, 2, 3], &[0, 1, 2, 3]);
    page.push_shape(Arc::new(Shape::new(9)));
    assert_eq!((ids(&page), kept(&page)), (vec![1, 2, 3, 9], vec![0, 1, 2, 4]));
    page.insert_shape(1, Arc::new(Shape::new(8))).unwrap();
    assert_eq!((ids(&page), kept(&page)), (vec![1, 8, 2, 3, 9], vec![0, 2, 3, 5]));
    assert!(page.remove_shape(2).is_some());
    assert_eq!((ids(&page), kept(&page)), (vec![1, 8, 3, 9], vec![0, 2, 2, 4]), "the node after 2 goes with 8, the shape before it");
    assert!(page.remove_shape(0).is_some());
    assert_eq!(
        (ids(&page), kept(&page)),
        (vec![8, 3, 9], vec![0, 1, 1, 3]),
        "the node before the first shape stays before it, and the one after 8 follows 8"
    );
    // A place past the end is refused and nothing moves.
    assert!(page.insert_shape(99, Arc::new(Shape::new(7))).is_err());
    assert!(page.remove_shape(99).is_none());
    assert_eq!(kept(&page), [0, 1, 1, 3]);
}

#[test]
fn a_list_that_is_rebuilt_moves_each_node_with_the_shape_it_followed() {
    // 1 2 3 4 with a node after each (1 to 4) and one before the first.
    let mut page = page_with(&[1, 2, 3, 4], &[0, 1, 2, 3, 4]);
    // Shape 1 to the end: the node that followed it goes with it, and the node before the first stays.
    page.edit_shapes(|list| {
        let first = list.remove(0).unwrap();
        list.push(first);
    });
    assert_eq!(ids(&page), [2, 3, 4, 1]);
    assert_eq!(kept(&page), [0, 1, 2, 3, 4]);
    let held = page.ext.get().unwrap().vsdx.0.first().unwrap().1.kept.clone();
    assert_eq!(names(&held), ["n0", "n2", "n3", "n4", "n1"], "the nodes are put in order of their places again");
    // Two shapes grouped: 3 and 4 are taken out and a group 9 put where they were.
    let mut page = page_with(&[1, 2, 3, 4], &[0, 1, 2, 3, 4]);
    page.edit_shapes(|list| {
        list.remove(3);
        list.remove(2);
        list.push(Arc::new(Shape::new(9)));
    });
    assert_eq!(ids(&page), [1, 2, 9]);
    assert_eq!(
        kept(&page),
        [0, 1, 3, 3, 3],
        "the nodes that followed 2, 3 and 4 follow shape 2 and the group that took the place of 3 and 4, which went in ahead of them"
    );
    // The same list is no change: nothing is copied.
    let before = page.ext.get().unwrap().vsdx.0.first().unwrap().1.kept.shared_list().map(Arc::as_ptr);
    page.edit_shapes(|_| {});
    assert_eq!(page.ext.get().unwrap().vsdx.0.first().unwrap().1.kept.shared_list().map(Arc::as_ptr), before);
}

#[test]
fn the_members_of_a_group_and_the_shapes_of_a_master_are_kept_the_same_way() {
    let mut group = Shape::new(1);
    for id in [2, 3] {
        group.children.push(Arc::new(Shape::new(id)));
    }
    group.ext.make_mut().vsdx.0.push((Arc::from("Shapes"), Kept { kept: nodes(&[1, 2]), ..Kept::default() }));
    group.push_member(Arc::new(Shape::new(4)));
    let held = |shape: &Shape| places(&shape.ext.get().unwrap().vsdx.0.first().unwrap().1.kept);
    assert_eq!(held(&group), [1, 3]);
    assert!(group.remove_member(0).is_some());
    assert_eq!(held(&group), [0, 2]);
    group.insert_member(0, Arc::new(Shape::new(5))).unwrap();
    assert_eq!(held(&group), [0, 3], "a new first shape goes behind a node that stood before the first");
    let mut master = diacraft_doc::Master { id: 1, ..diacraft_doc::Master::default() };
    master.shapes.push(Arc::new(Shape::new(1)));
    master.ext.make_mut().vsdx.0.push((Arc::from("Shapes"), Kept { kept: nodes(&[1]), ..Kept::default() }));
    master.push_shape(Arc::new(Shape::new(2)));
    assert_eq!(places(&master.ext.get().unwrap().vsdx.0.first().unwrap().1.kept), [2]);
    assert!(master.remove_shape(1).is_some());
    assert!(master.insert_shape(1, Arc::new(Shape::new(3))).is_ok());
}

#[test]
fn a_list_of_shapes_with_no_node_is_not_read_and_one_with_nodes_costs_the_nodes_and_never_the_shapes_but_for_a_rebuilt_list() {
    let mut plain = diacraft_doc::Page::new(1, "Plain");
    for id in 0..300 {
        plain.shapes.push(Arc::new(Shape::new(id)));
    }
    diacraft_doc::work::reset();
    plain.push_shape(Arc::new(Shape::new(1_000)));
    plain.remove_shape(5);
    plain.edit_shapes(|list| {
        list.remove(0);
    });
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (0, 0));
    // A shape put in, taken out or put in the middle reads the nodes once each, and no key.
    let mut page = page_with(&(0..300u32).collect::<Vec<_>>(), &[0, 100, 300]);
    diacraft_doc::work::reset();
    page.remove_shape(5);
    page.push_shape(Arc::new(Shape::new(1_000)));
    page.insert_shape(10, Arc::new(Shape::new(1_001))).unwrap();
    assert_eq!(
        (diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()),
        (0, 9),
        "the nodes once each for each of the three, and not a shape"
    );
    assert_eq!(
        kept(&page),
        [0, 100, 301],
        "after 5 out (99, 299), 1000 in at the end (ahead of the trailing node: 300) and 1001 in at 10 (100, 301)"
    );
    // A rebuilt list reads the keys of the old and the new list once each, and the nodes once.
    let mut page = page_with(&(0..300u32).collect::<Vec<_>>(), &[0, 100, 300]);
    diacraft_doc::work::reset();
    page.edit_shapes(|list| {
        list.remove(5);
    });
    assert_eq!(diacraft_doc::work::follow_keys(), 300 + 299, "the old and the new list once, and not a pass for each node");
    assert_eq!(diacraft_doc::work::places_read(), 3, "each node once");
    assert_eq!(kept(&page), [0, 99, 299]);
}

// ------------------------------------------------------------------------------------------------------------------------- hostile input

#[test]
fn places_at_the_edge_of_what_a_count_holds_move_without_a_panic_whatever_the_writer() {
    let mut section = Section::new("Scratch", None);
    section.passthrough = Passthrough::from_nodes(vec![
        Placed { after: u32::MAX, node: RawNode::Comment(Arc::from("max")) },
        Placed { after: 0, node: RawNode::Comment(Arc::from("zero")) },
    ]);
    section.insert_row(0, row(0));
    section.remove_rows(&[0, 5, 9]);
    section.retain_rows(|_| false);
    assert_eq!(section.rows.len(), 0);
    let mut sheet = Sheet { passthrough: nodes(&[u32::MAX, 7]), ..Sheet::default() };
    sheet.section_mut(&key("User")).unwrap();
    sheet.set_cell(&CellAddr::direct("A"), Cell::default()).unwrap();
    sheet.remove_cell(&CellAddr::direct("A"));
    assert_eq!(sheet.passthrough.len(), 2, "no node is lost");
    let mut page = page_with(&[1, 1, 1], &[u32::MAX, 0, 2]);
    page.remove_shape(1);
    page.push_shape(Arc::new(Shape::new(1)));
    page.edit_shapes(|list| while list.remove(0).is_some() {});
    assert_eq!((page.shapes.len(), kept(&page).len()), (0, 3), "the nodes are all there, wherever they stand");
    // Rows and sheets made again, with places at the edge: no panic and no node lost.
    let mut section = Section::new("Scratch", None);
    section.rows.push(row(1));
    section.passthrough = nodes(&[u32::MAX, 0, 1]);
    section.replace_rows(rows_numbered(&[1, 2]));
    let mut other = Section::new("Scratch", None);
    other.take_places_of(&section);
    assert_eq!((section.passthrough.len(), other.passthrough.len()), (3, 3));
    let old = Sheet { passthrough: nodes(&[u32::MAX, 0, 9]), ..Sheet::default() };
    let mut new = sheet_of(&["A"], &["User"], &[]);
    new.take_places_of(&old);
    assert_eq!(owner_places_after(&nodes(&[u32::MAX, 5]), &old, &new).len(), 2);
    assert_eq!(new.passthrough.len(), 3);
}

// ------------------------------------------------------------------------------------------------------------ rows and sheets made again

/// A section of the rows numbered `ixs` (in that order), its own cells `cells`, and unknown nodes that stood after these counts of its children.
fn section_of_rows(cells: &[&str], ixs: &[u32], afters: &[u32]) -> Section {
    let mut section = Section::new("Scratch", None);
    section.cells = cells_of(cells);
    for ix in ixs {
        section.rows.push(row(*ix));
    }
    section.passthrough = nodes(afters);
    section
}

fn rows_numbered(ixs: &[u32]) -> Rows {
    Rows::from(ixs.iter().map(|ix| row(*ix)).collect::<Vec<_>>())
}

/// A sheet of the cells `cells` and the sections `sections` (each a name), and unknown nodes that stood after these counts of its children.
fn sheet_of(cells: &[&str], sections: &[&str], afters: &[u32]) -> Sheet {
    let mut sheet = Sheet { cells: cells_of(cells), passthrough: nodes(afters), ..Sheet::default() };
    for name in sections {
        sheet.sections.push(Arc::new(Section::new(name, None)));
    }
    sheet
}

#[test]
fn rows_put_in_place_of_the_rows_keep_each_node_with_the_row_of_the_same_number_that_it_followed() {
    // A route of 2 rows with a node before the first, after each (0, 1, 2), made again as 4 rows: rows 1 and 2 stay, and 3 and 4 are new.
    let mut section = section_of_rows(&[], &[1, 2], &[0, 1, 2]);
    section.replace_rows(rows_numbered(&[1, 2, 3, 4]));
    assert_eq!(numbers(&section), [1, 2, 3, 4]);
    assert_eq!(
        places(&section.passthrough),
        [0, 1, 4],
        "the node after row 1 stays after it; the one that trailed row 2 trails the rows put in behind it"
    );
    // 4 rows made again as 2: the nodes that followed the rows taken out go with the row before.
    let mut section = section_of_rows(&[], &[1, 2, 3, 4], &[0, 1, 2, 3, 4]);
    section.replace_rows(rows_numbered(&[1, 2]));
    assert_eq!(numbers(&section), [1, 2]);
    assert_eq!(places(&section.passthrough), [0, 1, 2, 2, 2]);
    // The rows of another numbering altogether: no row stays, and every node stands before the first.
    let mut section = section_of_rows(&[], &[1, 2], &[0, 1, 2]);
    section.replace_rows(rows_numbered(&[7, 8]));
    assert_eq!(places(&section.passthrough), [0, 0, 0]);
    // The same rows in another order: each node goes with its row, and the nodes are put in order of their places.
    let mut section = section_of_rows(&[], &[1, 2], &[0, 1, 2]);
    section.replace_rows(rows_numbered(&[2, 1]));
    assert_eq!(numbers(&section), [2, 1]);
    assert_eq!(names(&section.passthrough), ["n0", "n2", "n1"]);
    assert_eq!(places(&section.passthrough), [0, 1, 2]);
}

#[test]
fn rows_put_in_place_of_the_rows_of_a_section_that_holds_cells_leave_the_nodes_among_the_cells_and_the_rows_are_found_after() {
    // Two cells, then the rows: a node after the first cell (1), before the first row (2), after row 1 (3), after row 2 (4).
    let mut section = section_of_rows(&["NoFill", "NoLine"], &[1, 2], &[1, 2, 3, 4]);
    section.replace_rows(rows_numbered(&[1, 2, 3]));
    assert_eq!(places(&section.passthrough), [1, 2, 3, 5]);
    assert_eq!(section.row(&RowKey::Ix(3)).and_then(|r| r.ix), Some(3), "the lookup follows the rows that were put in");
    // A row with no number or name is told from none: the rows it stood for are gone and the new ones are new.
    let mut section = Section::new("Scratch", None);
    section.rows.push(Arc::new(Row::default()));
    section.passthrough = nodes(&[0, 1]);
    section.replace_rows(Rows::from(vec![Arc::new(Row::default()), row(1)]));
    assert_eq!(places(&section.passthrough), [0, 0], "a row with no key is no row of the new list");
}

#[test]
fn rows_put_in_place_of_the_rows_read_the_keys_once_each_and_only_when_a_node_is_held() {
    let ixs: Vec<u32> = (1..=200).collect();
    let mut plain = section_of_rows(&[], &ixs, &[]);
    diacraft_doc::work::reset();
    plain.replace_rows(rows_numbered(&(1..=250).collect::<Vec<_>>()));
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (0, 0), "a section with no node reads no key");
    assert_eq!(plain.rows.len(), 250);
    let mut held = section_of_rows(&[], &ixs, &[0, 100, 200]);
    diacraft_doc::work::reset();
    held.replace_rows(rows_numbered(&(1..=250).collect::<Vec<_>>()));
    assert_eq!(diacraft_doc::work::follow_keys(), 200 + 250, "the keys of the old and the new rows once each");
    assert_eq!(diacraft_doc::work::places_read(), 3, "each node once");
    assert_eq!(places(&held.passthrough), [0, 100, 250]);
}

#[test]
fn a_sheet_made_again_takes_the_nodes_of_the_old_one_with_the_cells_and_the_sections_they_followed() {
    // Cells A and C, sections User and Geometry: a node before A (0), after A (1), after C (2), after User (3), after Geometry (4).
    let old = sheet_of(&["A", "C"], &["User", "Geometry"], &[0, 1, 2, 3, 4]);
    // Made again with the cells A to D and the sections Geometry and Layer: B and D are new, User is gone and Layer is new.
    let mut new = sheet_of(&["A", "B", "C", "D"], &["Geometry", "Layer"], &[]);
    new.take_places_of(&old);
    assert_eq!(
        places(&new.passthrough),
        [0, 2, 4, 4, 6],
        "the node after A goes ahead of B, the one after C ahead of D; the one after User (gone) stands before Geometry; the one that trailed the sections trails Layer"
    );
    assert_eq!(names(&new.passthrough), ["n0", "n1", "n2", "n3", "n4"], "no node is lost");
    // The nodes of the shape that holds the sheet count the same children, and what comes after the sections (the text, the members).
    let owner = nodes(&[1, 3, 4, 5]);
    assert_eq!(places(&owner_places_after(&owner, &old, &new)), [2, 4, 6, 7]);
    // A sheet made again with the same cells and sections is no change, and an old sheet with no node gives none.
    let same = sheet_of(&["A", "C"], &["User", "Geometry"], &[]);
    let mut again = same.clone();
    again.take_places_of(&old);
    assert_eq!(places(&again.passthrough), [0, 1, 2, 3, 4]);
    let mut none = sheet_of(&["A"], &[], &[]);
    none.take_places_of(&sheet_of(&["B"], &[], &[]));
    assert!(none.passthrough.is_empty());
    assert!(owner_places_after(&Passthrough::new(), &old, &new).is_empty());
}

#[test]
fn a_sheet_made_again_with_sections_replaced_by_the_new_master_s_takes_the_nodes_of_the_sections_that_stand_and_the_replaced_one_is_gone() {
    // User, Geometry and Layer, with a node before the first, after each.
    let old = sheet_of(&[], &["User", "Geometry", "Layer"], &[0, 1, 2, 3]);
    // Made again with User and Layer and, after them, the Geometry the new master gives (the same name and `IX` as the old one) and a Connection section.
    let new = sheet_of(&[], &["User", "Layer", "Geometry", "Connection"], &[]);
    // The old Geometry stays: the nodes follow it to its new place, the one that followed Layer goes to where Layer is.
    let mut same = new.clone();
    same.take_places_of(&old);
    assert_eq!(names(&same.passthrough), ["n0", "n1", "n3", "n2"]);
    assert_eq!(places(&same.passthrough), [0, 1, 2, 4]);
    // The old Geometry is replaced: the node that followed it goes with the section before it, and the sections that are new go ahead of the one that trailed the sheet's sections.
    let mut replaced = new.clone();
    replaced.take_places_of_where(&old, |section| section.name.as_str() != "Geometry");
    assert_eq!(names(&replaced.passthrough), ["n0", "n1", "n2", "n3"]);
    assert_eq!(places(&replaced.passthrough), [0, 1, 1, 4]);
    // The nodes of the shape that holds the sheet are moved the same way.
    let owner = nodes(&[0, 1, 2, 3, 4]);
    assert_eq!(places(&owner_places_after_where(&owner, &old, &new, |section| section.name.as_str() != "Geometry")), [0, 1, 1, 4, 5]);
}

#[test]
fn a_section_made_again_takes_the_nodes_of_the_old_one_with_its_cells_and_its_rows() {
    // Cells NoFill and NoLine, rows 1 to 3: a node after the first cell (1), before the first row (2), after row 1 (3), after row 3 (5).
    let old = section_of_rows(&["NoFill", "NoLine"], &[1, 2, 3], &[1, 2, 3, 5]);
    // Made again with the cells NoLine and NoShow (NoFill is gone, NoShow is new) and the rows 2, 3 and 4 (1 is gone, 4 is new).
    let mut new = section_of_rows(&["NoLine", "NoShow"], &[2, 3, 4], &[]);
    new.take_places_of(&old);
    assert_eq!(
        places(&new.passthrough),
        [0, 2, 2, 5],
        "the node after NoFill (gone) stands before NoLine; the node before the first row stands after the cells, NoShow having gone in ahead of it; the one after row 1 (gone) stands before row 2; the one that trailed the rows trails row 4, which went in ahead of it"
    );
    assert_eq!(names(&new.passthrough), ["n0", "n1", "n2", "n3"]);
}

#[test]
fn nodes_moved_for_a_sheet_made_again_cost_the_keys_once_each_and_nothing_for_a_sheet_that_holds_none() {
    let sections: Vec<String> = (0..120).map(|n| format!("S{n}")).collect();
    let names: Vec<&str> = sections.iter().map(String::as_str).collect();
    let old = sheet_of(&["A", "B"], &names, &[]);
    let mut new = sheet_of(&["A", "B", "C"], &names, &[]);
    diacraft_doc::work::reset();
    new.take_places_of(&old);
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (0, 0));
    let held = sheet_of(&["A", "B"], &names, &[0, 2, 122]);
    let mut new = sheet_of(&["A", "B", "C"], &names, &[]);
    diacraft_doc::work::reset();
    new.take_places_of(&held);
    assert!(diacraft_doc::work::follow_keys() <= 2 * 120 + 2, "the section keys once each: {}", diacraft_doc::work::follow_keys());
    assert!(
        diacraft_doc::work::places_read() <= 3 * 4,
        "the nodes a few times each, never a pass of the sheet for each: {}",
        diacraft_doc::work::places_read()
    );
    assert_eq!(places(&new.passthrough), [0, 3, 123]);
}
