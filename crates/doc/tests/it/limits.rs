//! The caps of a document, which hold at each door a document enters: the commands, and the reader of a file, which runs
//! `Document::check_limits` on what it built. A document built in memory by a test may be past one, and is read all the same.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, DocError, MAX_CELLS_PER_CONTAINER, MAX_MASTERS, MAX_NAME_BYTES, MAX_PAGES, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE,
    MAX_STYLES, Master, Owner, Page, Resolver, Shape, ShapeList, ShapeRef, SheetRef, Style,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

/// A chain of `depth` nested shapes, built from the inside out with no recursion.
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
fn the_caps_are_the_ones_the_rulings_name() {
    // Each is a project rule that Task 5's sample-file run confirms against real files; the depth is.
    assert_eq!(MAX_STYLES, 65_536);
    assert_eq!(MAX_MASTERS, 65_536);
    assert_eq!(MAX_SHAPES_PER_PAGE, 1_000_000);
    assert_eq!(MAX_PAGES, 1_000);
    assert_eq!(MAX_SHAPE_DEPTH, 200);
    // Each beside its real maximum in the sample-file run: 726 sections in a sheet, 258 cells in one place, 31,273 rows in a section.
    assert_eq!(diacraft_doc::MAX_SECTIONS_PER_SHEET, 8_192);
    assert_eq!(diacraft_doc::MAX_CELLS_PER_CONTAINER, 4_096);
    assert_eq!(diacraft_doc::MAX_ROWS_PER_SECTION, 320_000);
    assert_eq!(diacraft_doc::MAX_COLOR_ENTRIES, 4_096);
}

#[test]
fn a_sheet_a_place_and_a_colour_table_pass_at_their_cap_and_fail_one_past_it() {
    use diacraft_doc::{CellName, ColorEntry, MAX_CELLS_PER_CONTAINER, MAX_COLOR_ENTRIES, MAX_SECTIONS_PER_SHEET, Section, Sheet, Value};
    let mut sheet = Sheet {
        sections: (0..MAX_SECTIONS_PER_SHEET).map(|i| Arc::new(Section::new("Geometry", Some(u32::try_from(i).unwrap())))).collect(),
        ..Sheet::default()
    };
    assert_eq!(sheet.check_limits(), Ok(()));
    sheet.sections.push(Arc::new(Section::new("Geometry", None)));
    assert_eq!(sheet.check_limits(), Err(DocError::Limit { what: "sections in a sheet", max: MAX_SECTIONS_PER_SHEET }));

    let mut sheet = Sheet::default();
    for i in 0..MAX_CELLS_PER_CONTAINER {
        sheet.cells.insert(CellName::new(&format!("C{i}")), Cell::constant(Value::from_text("1")));
    }
    assert_eq!(sheet.check_limits(), Ok(()));
    sheet.cells.insert(CellName::new("One more"), Cell::constant(Value::from_text("1")));
    assert_eq!(sheet.check_limits(), Err(DocError::Limit { what: "cells in one place", max: MAX_CELLS_PER_CONTAINER }));

    let mut doc = DocBuilder::new().build();
    let entry = |ix: u32| ColorEntry { ix, rgb: Arc::from("#000000") };
    doc.colors = Arc::new((0..u32::try_from(MAX_COLOR_ENTRIES).unwrap()).map(entry).collect());
    assert_eq!(doc.check_limits(), Ok(()));
    Arc::make_mut(&mut doc.colors).push(entry(9_999));
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "colour table entries", max: MAX_COLOR_ENTRIES }));
}

#[test]
fn a_document_at_each_cap_passes_and_one_past_it_does_not() {
    let blank = DocBuilder::new().build();
    assert_eq!(blank.check_limits(), Ok(()));

    // Styles, masters, pages: the same `Arc` as often as the cap says, so that the test costs a list and not a million allocations.
    let style = Arc::new(Style::default());
    let mut doc = blank.clone();
    doc.styles = std::iter::repeat_n(Arc::clone(&style), MAX_STYLES).collect();
    assert_eq!(doc.check_limits(), Ok(()));
    doc.styles.push(Arc::clone(&style));
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "styles", max: MAX_STYLES }));

    let master = Arc::new(Master::default());
    let mut doc = blank.clone();
    doc.masters = std::iter::repeat_n(Arc::clone(&master), MAX_MASTERS).collect();
    assert_eq!(doc.check_limits(), Ok(()));
    doc.masters.push(Arc::clone(&master));
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "masters", max: MAX_MASTERS }));

    let page = Arc::new(Page::new(1, "p"));
    let mut doc = blank.clone();
    doc.pages = std::iter::repeat_n(Arc::clone(&page), MAX_PAGES).collect();
    assert_eq!(doc.check_limits(), Ok(()));
    doc.pages.push(Arc::clone(&page));
    assert_eq!(doc.check_limits(), Err(DocError::TooManyPages));
}

#[test]
fn the_shapes_of_a_part_are_counted_with_the_nested_ones() {
    let one = Arc::new(Shape::new(7));
    let mut doc = DocBuilder::new().page(1, PageBuilder::new()).build();
    // A flat part at the cap.
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.shapes = std::iter::repeat_n(Arc::clone(&one), MAX_SHAPES_PER_PAGE).collect();
    assert_eq!(doc.check_limits(), Ok(()));
    // One more, at the top level.
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.shapes.push(Arc::clone(&one));
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "shapes in a part", max: MAX_SHAPES_PER_PAGE }));
    // The same count made of one group that holds the rest: a group is a shape too.
    let mut group = Shape::new(1);
    group.children = std::iter::repeat_n(Arc::clone(&one), MAX_SHAPES_PER_PAGE).collect();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.shapes = std::iter::once(Arc::new(group)).collect();
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "shapes in a part", max: MAX_SHAPES_PER_PAGE }));
    // A master part is a part too.
    let mut doc = DocBuilder::new().build();
    let master = Master { id: 1, shapes: std::iter::repeat_n(Arc::clone(&one), MAX_SHAPES_PER_PAGE + 1).collect(), ..Master::default() };
    doc.masters.push(Arc::new(master));
    assert!(matches!(doc.check_limits(), Err(DocError::Limit { what: "shapes in a part", .. })));
}

#[test]
fn groups_nested_to_the_depth_limit_pass_and_one_level_more_does_not() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new()).build();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = nested(MAX_SHAPE_DEPTH);
    assert_eq!(doc.check_limits(), Ok(()));
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = nested(MAX_SHAPE_DEPTH + 1);
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "levels of nested shapes", max: MAX_SHAPE_DEPTH }));
    // Far past it, the check still ends, and says the same.
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = nested(1_000);
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "levels of nested shapes", max: MAX_SHAPE_DEPTH }));
}

#[test]
fn the_limits_of_each_sheet_are_checked_where_the_sheet_is() {
    let cells = |n: usize| {
        let mut sheet = diacraft_doc::Sheet::default();
        for i in 0..n {
            sheet.cells.insert(CellName::new(&format!("c{i}")), Cell::new());
        }
        sheet
    };
    // On a shape, a style, a page and the document sheet.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    assert_eq!(doc.check_limits(), Ok(()));
    let over = Arc::new(cells(MAX_CELLS_PER_CONTAINER + 1));
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(page.shapes.get_mut(0).unwrap()).sheet = Arc::clone(&over);
    assert!(matches!(doc.check_limits(), Err(DocError::Limit { what: "cells in one place", .. })));
    let mut doc = DocBuilder::new().build();
    doc.sheet = Arc::clone(&over);
    assert!(matches!(doc.check_limits(), Err(DocError::Limit { .. })));
    // A name too long to write is a name too long to read.
    let mut doc = DocBuilder::new().build();
    let mut long = diacraft_doc::Sheet::default();
    long.cells.insert(CellName::new(&"n".repeat(MAX_NAME_BYTES + 1)), Cell::new());
    doc.styles.push(Arc::new(Style { id: 5, sheet: Arc::new(long), ..Style::default() }));
    assert!(matches!(doc.check_limits(), Err(DocError::BadName(_))));
}

#[test]
fn the_commands_door_refuses_at_the_cap_and_changes_nothing() {
    let mut doc = DocBuilder::new().build();
    let style = Arc::new(Style::default());
    doc.styles = std::iter::repeat_n(Arc::clone(&style), MAX_STYLES - 1).collect();
    assert_eq!(doc.add_style(Style { id: 70_000, ..Style::default() }), Ok(()));
    let before = doc.clone();
    assert_eq!(doc.add_style(Style::default()), Err(DocError::Limit { what: "styles", max: MAX_STYLES }));
    assert_eq!(doc, before);

    let mut doc = DocBuilder::new().build();
    doc.masters = std::iter::repeat_n(Arc::new(Master::default()), MAX_MASTERS - 1).collect();
    assert_eq!(doc.add_master(Master::default()), Ok(()));
    let before = doc.clone();
    assert_eq!(doc.add_master(Master::default()), Err(DocError::Limit { what: "masters", max: MAX_MASTERS }));
    assert_eq!(doc, before);

    // Pages were already met at the write.
    let mut doc = DocBuilder::new().build();
    while doc.pages.len() < MAX_PAGES {
        doc.add_page(None).unwrap();
    }
    assert_eq!(doc.add_page(None), Err(DocError::TooManyPages));
}

#[test]
fn a_document_past_a_cap_that_a_test_built_is_read_and_cloned_all_the_same() {
    // Nothing in the model panics, or slows to a crawl, past a cap: the cap is for the doors.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(3), None, None).cell("Width", "1"))).build();
    let extra = (1..=u32::try_from(MAX_STYLES + 5_000).unwrap()).map(|id| Arc::new(Style { id, ..Style::default() }));
    doc.styles = extra.collect();
    assert!(doc.check_limits().is_err());
    let at = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] });
    let r = Resolver::new(&doc).resolve(&at, &CellAddr::direct("LineWeight")).unwrap();
    assert!(r.warnings.is_empty());
    assert!(doc.clone().check_limits().is_err());
}
