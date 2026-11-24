//! Hostile sheets: every cap is checked before what it counts is stored, a refusal names its cap by a class word, and what costs a file much
//! costs the budgets much. Trees are built in code: a file that is past a cap of the model would be past the XML limits first.

use diacraft_container::xml::{El, Node};
use diacraft_doc::{MAX_CELLS_PER_CONTAINER, MAX_NAME_BYTES, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET};
use diacraft_sheetxml::{Caps, LimitError, LimitKind};

use super::{BIG, Main, read, read_el, read_with};

fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
    El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
}

fn cell(name: &str) -> Node {
    Node::El(el("Cell", &[("N", name)], vec![]))
}

fn refused(tree: &El, caps: Caps) -> LimitError {
    match read_el::<Main>(tree, caps) {
        Ok(_) => panic!("the sheet should have been refused"),
        Err(e) => e,
    }
}

#[test]
fn the_most_cells_one_place_may_hold_is_read_and_one_more_is_refused_by_its_class_word() {
    let at_most = el("PageSheet", &[], (0..MAX_CELLS_PER_CONTAINER).map(|i| cell(&format!("c{i}"))).collect());
    assert_eq!(read_el::<Main>(&at_most, BIG).unwrap().sheet.cells.len(), MAX_CELLS_PER_CONTAINER);
    let one_more = el("PageSheet", &[], (0..=MAX_CELLS_PER_CONTAINER).map(|i| cell(&format!("c{i}"))).collect());
    let e = refused(&one_more, BIG);
    assert_eq!((e.kind, e.max), (LimitKind::CellsInOnePlace, MAX_CELLS_PER_CONTAINER as u64));
    assert_eq!(e.kind.class(), "too_many_cells_in_one_place");
    // The same count under a section and in a row.
    let section = |n| el("PageSheet", &[], vec![Node::El(el("Section", &[("N", "User")], (0..n).map(|i| cell(&format!("c{i}"))).collect()))]);
    assert!(read_el::<Main>(&section(MAX_CELLS_PER_CONTAINER), BIG).is_ok());
    assert_eq!(refused(&section(MAX_CELLS_PER_CONTAINER + 1), BIG).kind, LimitKind::CellsInOnePlace);
    let row = |n| {
        let row = el("Row", &[("N", "r")], (0..n).map(|i| cell(&format!("c{i}"))).collect());
        el("PageSheet", &[], vec![Node::El(el("Section", &[("N", "User")], vec![Node::El(row)]))])
    };
    assert!(read_el::<Main>(&row(MAX_CELLS_PER_CONTAINER), BIG).is_ok());
    assert_eq!(refused(&row(MAX_CELLS_PER_CONTAINER + 1), BIG).kind, LimitKind::CellsInOnePlace);
}

#[test]
fn the_most_rows_and_the_most_sections_are_read_and_one_more_is_refused() {
    let rows = |n: usize| {
        let rows = (0..n).map(|_| Node::El(el("Row", &[], vec![]))).collect();
        el("PageSheet", &[], vec![Node::El(el("Section", &[("N", "Geometry")], rows))])
    };
    let r = read_el::<Main>(&rows(MAX_ROWS_PER_SECTION), BIG).unwrap();
    assert_eq!(r.reader.stats().max_rows_in_a_section, MAX_ROWS_PER_SECTION);
    let e = refused(&rows(MAX_ROWS_PER_SECTION + 1), BIG);
    assert_eq!((e.kind, e.max), (LimitKind::RowsInASection, MAX_ROWS_PER_SECTION as u64));
    assert_eq!(e.kind.class(), "too_many_rows_in_a_section");

    let sections = |n: usize| el("PageSheet", &[], (0..n).map(|i| Node::El(el("Section", &[("N", &format!("s{i}"))], vec![]))).collect());
    let r = read_el::<Main>(&sections(MAX_SECTIONS_PER_SHEET), BIG).unwrap();
    assert_eq!(r.reader.stats().max_sections_in_a_sheet, MAX_SECTIONS_PER_SHEET);
    let e = refused(&sections(MAX_SECTIONS_PER_SHEET + 1), BIG);
    assert_eq!((e.kind, e.max), (LimitKind::SectionsInASheet, MAX_SECTIONS_PER_SHEET as u64));
}

#[test]
fn a_name_past_the_cap_is_refused_in_a_cell_a_section_and_a_row_and_one_at_the_cap_is_read() {
    let long = "n".repeat(MAX_NAME_BYTES + 1);
    let exact = "n".repeat(MAX_NAME_BYTES);
    let sheet = |child: Node| el("PageSheet", &[], vec![child]);
    let section = |name: &str, row: Option<El>| Node::El(el("Section", &[("N", name)], row.map(Node::El).into_iter().collect()));
    for tree in [sheet(cell(&long)), sheet(section(&long, None)), sheet(section("User", Some(el("Row", &[("N", &long)], vec![]))))] {
        let e = refused(&tree, BIG);
        assert_eq!((e.kind, e.max), (LimitKind::NameBytes, MAX_NAME_BYTES as u64));
        assert_eq!(e.kind.class(), "name_too_long");
    }
    for tree in [sheet(cell(&exact)), sheet(section(&exact, None)), sheet(section("User", Some(el("Row", &[("N", &exact)], vec![]))))] {
        assert!(read_el::<Main>(&tree, BIG).is_ok());
    }
}

#[test]
fn the_budget_of_items_refuses_the_cell_that_would_pass_it_and_every_class_word_is_the_caps() {
    // A cell is 1 + its attributes, and the sheet 1: three cells of one attribute each are 1 + 6.
    let tree = el("PageSheet", &[], (0..3).map(|i| Node::El(el("Cell", &[("N", &format!("c{i}"))], vec![]))).collect());
    assert_eq!(read_el::<Main>(&tree, Caps { items: 7, ..BIG }).unwrap().reader.items_left(), 0);
    let e = refused(&tree, Caps { items: 6, ..BIG });
    assert_eq!((e.kind, e.max), (LimitKind::Items, 6));
    assert_eq!(e.kind.class(), "too_many_items");
    let e = refused(&tree, Caps { cells: 2, ..BIG });
    assert_eq!((e.kind, e.max), (LimitKind::Cells, 2));
    assert_eq!(e.kind.class(), "too_many_cells");
    assert_eq!(e.to_string(), "the file holds more than 2 cells");
    // A budget of nothing refuses the sheet before anything is stored.
    assert_eq!(refused(&tree, Caps { items: 0, ..BIG }).kind, LimitKind::Items);
}

#[test]
fn the_class_words_and_the_wording_of_every_cap_are_fixed() {
    let all = [
        (LimitKind::Cells, "too_many_cells", "cells"),
        (LimitKind::CellsInOnePlace, "too_many_cells_in_one_place", "cells in one sheet, section or row"),
        (LimitKind::SectionsInASheet, "too_many_sections_in_a_sheet", "sections in one sheet"),
        (LimitKind::RowsInASection, "too_many_rows_in_a_section", "rows in one section"),
        (LimitKind::NameBytes, "name_too_long", "bytes in a cell, section or row name"),
        (LimitKind::Items, "too_many_items", "XML items kept in the document"),
    ];
    for (kind, class, what) in all {
        assert_eq!((kind.class(), kind.what()), (class, what));
    }
}

#[test]
fn a_cell_paid_for_again_takes_nothing_when_the_budget_cannot_pay_for_all_of_it() {
    let mut reader = diacraft_sheetxml::SheetReader::new(Caps { cells: 10, ..BIG });
    reader.charge_cells_again(6).unwrap();
    assert_eq!((reader.cells_left(), reader.stats().cells), (4, 6));
    let e = reader.charge_cells_again(5).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::Cells, 10));
    assert_eq!((reader.cells_left(), reader.stats().cells), (4, 6));
    reader.charge_cells_again(4).unwrap();
    assert_eq!(reader.cells_left(), 0);
    // No cell is left for the next one.
    assert_eq!(reader.take_cell().unwrap_err().kind, LimitKind::Cells);
    assert_eq!(reader.take_items(usize::MAX).unwrap_err().kind, LimitKind::Items);
}

#[test]
fn a_thousand_cells_of_one_name_are_kept_whole_and_each_is_paid_for_as_a_cell_and_as_its_nodes() {
    let body: String = (0..1_000).map(|_| r#"<Cell N="A" V="1"><x/></Cell>"#).collect();
    let r = read(&format!("<PageSheet>{body}</PageSheet>"));
    assert_eq!(r.sheet.cells.len(), 1);
    assert_eq!(r.sheet.passthrough.len(), 999);
    assert_eq!(r.warnings.count("duplicate_cell"), 999);
    // The first cell is a cell and its attributes and its child; each of the others is a cell, and kept whole: itself, two attributes, a child.
    let stats = r.reader.stats();
    assert_eq!(stats.cells, 1_000);
    assert_eq!(stats.items, 1 + (1 + 2 + 1) + 999 * 4);
    // Only the first warnings are worded.
    assert_eq!(r.warnings.messages.len(), 50);
}

#[test]
fn an_unknown_element_as_deep_as_the_xml_limit_allows_is_kept_on_a_small_stack() {
    // The XML tree is at most 200 levels deep by default, and the copy of an unknown element is a frame a level: a thread with a quarter of a
    // browser's main thread must hold it.
    let deep = format!("{}{}", "<z>".repeat(190), "</z>".repeat(190));
    let xml = format!(r#"<PageSheet><Trigger N="t">{deep}</Trigger><Cell N="A"><d>{deep}</d></Cell></PageSheet>"#);
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let r = read_with::<Main>(&xml, BIG).unwrap();
            // The sheet holds the deep trigger, and the cell the deep child; both are dropped on this thread, whose drop recurses once a level too.
            (r.sheet.passthrough.len(), r.sheet.cell("A").unwrap().extra.as_ref().unwrap().passthrough.len())
        })
        .unwrap();
    assert_eq!(handle.join().unwrap(), (1, 1));
}
