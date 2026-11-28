//! Rows of a large section: 320,000 of them are allowed (the sample files have a real section of 31,273), and a row is found by
//! key in a table, never by a scan of them all. Each shape here counts the tables of rows it builds (`diacraft_doc::work`), where a scan
//! for each of 320,000 keys would be some 50 billion comparisons; the bound on time is a hang guard and no measure of cost.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{
    Cell, CellAddr, CellName, DocError, MAX_ROWS_PER_SECTION, Owner, Resolver, Row, RowKey, Section, SectionKey, Shape, ShapeList, ShapeRef, Sheet,
    SheetRef, Value,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder};

/// A hang guard and no measure of cost: the slowest of these takes about two seconds at load 50.
const BOUND: Duration = Duration::from_secs(60);

fn row(ix: u32, value: &str) -> Row {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from("LineTo")), ..Row::default() };
    row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(value)));
    row
}

fn geometry(n: usize) -> Section {
    let mut section = Section::new("Geometry", Some(0));
    section.rows = (0..n).map(|i| Arc::new(row(u32::try_from(i).unwrap(), "1"))).collect();
    section
}

fn x_of(sheet: &Sheet, ix: u32) -> Option<String> {
    let section = sheet.sections.first()?;
    let row = section.row(&RowKey::Ix(ix))?;
    Some(row.cells.get("X")?.value.as_ref()?.text().to_owned())
}

#[test]
fn the_cap_is_320_000_and_a_section_of_that_many_rows_is_allowed_and_one_more_is_not() {
    assert_eq!(MAX_ROWS_PER_SECTION, 320_000);
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(geometry(MAX_ROWS_PER_SECTION)));
    assert_eq!(sheet.check_limits(), Ok(()));
    // One more by a write is refused, and so is a sheet that has it by hand.
    let key = SectionKey::new("Geometry", Some(0));
    let started = Instant::now();
    let at = u32::try_from(MAX_ROWS_PER_SECTION).unwrap();
    let write = sheet.row_mut(&key, &RowKey::Ix(at)).map(|_| ());
    assert_eq!(write, Err(DocError::Limit { what: "rows in a section", max: MAX_ROWS_PER_SECTION }));
    Arc::make_mut(&mut sheet.sections[0]).rows.push(Arc::new(row(at, "1")));
    assert_eq!(sheet.check_limits(), Err(DocError::Limit { what: "rows in a section", max: MAX_ROWS_PER_SECTION }));
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}

#[test]
fn every_row_of_a_section_of_320_000_is_found_by_key_in_far_less_than_a_scan_of_them_costs() {
    let section = geometry(MAX_ROWS_PER_SECTION);
    let started = Instant::now();
    diacraft_doc::work::reset();
    for ix in 0..u32::try_from(MAX_ROWS_PER_SECTION).unwrap() {
        let found = section.row(&RowKey::Ix(ix)).unwrap_or_else(|| panic!("row {ix} not found"));
        assert_eq!(found.ix, Some(ix), "the row found is the one asked for");
    }
    assert!(section.row(&RowKey::Ix(320_000)).is_none());
    assert!(section.row(&RowKey::name("N")).is_none());
    let took = started.elapsed();
    eprintln!("[timing] 320,000 lookups of 320,000 rows: {took:?}");
    assert!(took < BOUND, "{took:?}");
    // Counted: one table of the rows for all the lookups (a scan for each would be 50 billion comparisons).
    assert_eq!(diacraft_doc::work::row_tables_built(), 1, "the table of the rows is built once");
}

#[test]
fn the_first_of_two_rows_with_one_key_is_the_one_found_and_a_table_does_not_change_that() {
    let mut section = geometry(100);
    section.rows.push(Arc::new(row(7, "again")));
    section.rows.insert(50, Arc::new(row(99, "mid")));
    for _ in 0..2 {
        // The table is built on the first lookup and used on the second.
        assert_eq!(
            section.row(&RowKey::Ix(7)).and_then(|r| r.cells.get("X")).map(|c| c.value.as_ref().unwrap().text().to_owned()).as_deref(),
            Some("1")
        );
        assert_eq!(section.row(&RowKey::Ix(99)).map(|r| r.ix), Some(Some(99)));
    }
    // A section whose rows are edited by hand after a lookup is not answered from a stale table.
    section.rows.truncate(40);
    assert!(section.row(&RowKey::Ix(60)).is_none(), "a row that is gone is gone");
    assert!(section.row(&RowKey::Ix(39)).is_some());
    section.rows[10] = Arc::new(row(1_000, "swapped"));
    assert!(section.row(&RowKey::Ix(1_000)).is_some(), "a row put in the place of another is found");
    assert!(section.row(&RowKey::Ix(10)).is_none(), "and the one it replaced is not");
}

#[test]
fn a_clone_shares_the_table_and_a_write_to_one_does_not_reach_the_other() {
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(geometry(1_000)));
    let key = SectionKey::new("Geometry", Some(0));
    assert_eq!(x_of(&sheet, 500).as_deref(), Some("1"));
    let older = sheet.clone();
    let mut newer = sheet.clone();
    newer.row_mut(&key, &RowKey::Ix(2_000)).unwrap().cells.insert(CellName::new("X"), Cell::constant(Value::from_text("new")));
    newer.row_mut(&key, &RowKey::Ix(500)).unwrap().cells.insert(CellName::new("X"), Cell::constant(Value::from_text("changed")));
    assert_eq!(x_of(&newer, 2_000).as_deref(), Some("new"));
    assert_eq!(x_of(&newer, 500).as_deref(), Some("changed"));
    assert_eq!(x_of(&older, 2_000), None, "the older sheet does not have the new row");
    assert_eq!(x_of(&older, 500).as_deref(), Some("1"), "and keeps the old value");
    assert_eq!(x_of(&sheet, 2_000), None);
}

#[test]
fn twenty_thousand_writes_that_each_make_a_row_in_a_section_of_300_000_are_not_twenty_thousand_scans_or_rebuilds() {
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(geometry(300_000)));
    let key = SectionKey::new("Geometry", Some(0));
    let started = Instant::now();
    diacraft_doc::work::reset();
    for i in 0..20_000u32 {
        let ix = 300_000 + i;
        let addr = CellAddr { section: Some(key.clone()), row: Some(RowKey::Ix(ix)), cell: CellName::new("X") };
        sheet.set_cell(&addr, Cell::constant(Value::from_text("w"))).unwrap();
    }
    // And writes to rows that are there.
    for ix in (0..300_000u32).step_by(15) {
        let addr = CellAddr { section: Some(key.clone()), row: Some(RowKey::Ix(ix)), cell: CellName::new("X") };
        sheet.set_cell(&addr, Cell::constant(Value::from_text("c"))).unwrap();
    }
    let took = started.elapsed();
    eprintln!("[timing] 40,000 writes into a section of 300,000 rows: {took:?}");
    assert!(took < BOUND, "{took:?}");
    // Counted: the table is kept as the writes go, so 40,000 writes build it once at the most (a rebuild for each is 40,000 of 300,000 keys).
    let tables = diacraft_doc::work::row_tables_built();
    assert!(tables <= 1, "{tables} tables of the rows built by 40,000 writes");
    assert_eq!(sheet.sections[0].rows.len(), 320_000);
    assert_eq!(x_of(&sheet, 319_999).as_deref(), Some("w"));
    assert_eq!(x_of(&sheet, 15).as_deref(), Some("c"));
    assert_eq!(x_of(&sheet, 16).as_deref(), Some("1"));
    // The table agrees with the rows: every key is where the table says.
    for (at, r) in sheet.sections[0].rows.iter().enumerate() {
        assert_eq!(r.ix, Some(u32::try_from(at).unwrap()));
    }
}

#[test]
fn effective_rows_and_a_resolve_over_a_section_of_320_000_stay_near_linear() {
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(geometry(MAX_ROWS_PER_SECTION)));
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    let mut doc = DocBuilder::new().page(1, PageBuilder::new()).build();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let at = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] });
    let key = SectionKey::new("Geometry", Some(0));

    let started = Instant::now();
    diacraft_doc::work::reset();
    let resolver = Resolver::new(&doc);
    let rows = resolver.effective_rows(&at, &key).unwrap();
    let took = started.elapsed();
    eprintln!("[timing] effective_rows over 320,000 rows of one cell: {took:?}");
    assert_eq!(rows.len(), MAX_ROWS_PER_SECTION);
    assert_eq!(rows[0].key, RowKey::Ix(0));
    assert_eq!(rows[319_999].key, RowKey::Ix(319_999));
    assert_eq!(rows[319_999].cells.len(), 1);
    assert!(took < BOUND, "{took:?}");

    // A resolver for each of 2,000 reads (what a call of `resolve` builds) is not a rebuilt table for each: the section holds its own.
    let started = Instant::now();
    for ix in (0..320_000u32).step_by(160) {
        let addr = CellAddr { section: Some(key.clone()), row: Some(RowKey::Ix(ix)), cell: CellName::new("X") };
        let resolved = Resolver::new(&doc).resolve(&at, &addr).unwrap();
        assert_eq!(resolved.value.cell.and_then(|c| c.value.as_ref()).map(Value::text), Some("1"));
    }
    let took = started.elapsed();
    eprintln!("[timing] 2,000 one-off resolves in a section of 320,000 rows: {took:?}");
    assert!(took < BOUND, "{took:?}");
    // Counted: the section holds its own table, so the 2,000 resolvers (and the rows read before them) built it once.
    assert_eq!(diacraft_doc::work::row_tables_built(), 1, "a table of the rows for each resolver");
}

/// A row a write makes goes among the `Geometry` rows by its `IX` (they rise through the section, and a program that draws the rows in the file's order draws the path in it),
/// and **the table of the rows is kept as it goes**: 20,000 writes that each put a row just ahead of the last row of 50,000 are not 20,000 rebuilds of a table of 50,000 keys.
#[test]
fn twenty_thousand_writes_that_each_make_a_row_ahead_of_the_last_of_50_000_do_not_rebuild_the_table_for_each() {
    let mut section = Section::new("Geometry", Some(0));
    let mut rows: Vec<Arc<Row>> = (0..49_999u32).map(|i| Arc::new(row(i * 10, "1"))).collect();
    rows.push(Arc::new(row(4_000_000_000, "1")));
    section.rows = rows.into_iter().collect();
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(section));
    let key = SectionKey::new("Geometry", Some(0));
    let write = |sheet: &mut Sheet, ix: u32| {
        let addr = CellAddr { section: Some(key.clone()), row: Some(RowKey::Ix(ix)), cell: CellName::new("X") };
        sheet.set_cell(&addr, Cell::constant(Value::from_text("w"))).unwrap();
    };
    // The table is built by the first lookup of a section of more than 32 rows, and by no write after it.
    assert_eq!(x_of(&sheet, 10).as_deref(), Some("1"));
    diacraft_doc::work::reset();
    for i in 0..20_000u32 {
        write(&mut sheet, 600_000 + i);
    }
    // Rising keys that each go ahead of the ones just made, and ahead of the last row: the place is found through the last rows.
    for j in 0..50u32 {
        write(&mut sheet, 700_000 - j);
    }
    let tables = diacraft_doc::work::row_tables_built();
    assert!(tables <= 1, "{tables} tables of the rows built by 20,050 writes that put a row among the last");
    let rows = &sheet.sections[0].rows;
    assert_eq!(rows.len(), 50_000 + 20_050);
    let ixs: Vec<u32> = rows.iter().map(|r| r.ix.unwrap()).collect();
    assert!(ixs.windows(2).all(|pair| pair[0] < pair[1]), "the rows rise by their IX");
    // The table agrees with the rows: every row is found by its key, the shifted ones too.
    for (at, r) in rows.iter().enumerate().skip(49_000) {
        assert_eq!(sheet.sections[0].row(&RowKey::Ix(r.ix.unwrap())).map(|found| Arc::ptr_eq(found, r)), Some(true), "row {at}");
    }
    assert_eq!(x_of(&sheet, 600_000 + 19_999).as_deref(), Some("w"));
    assert_eq!(x_of(&sheet, 700_000).as_deref(), Some("w"));
    assert_eq!(x_of(&sheet, 4_000_000_000).as_deref(), Some("1"));
    assert_eq!(x_of(&sheet, 100).as_deref(), Some("1"));
}

/// The table kept as a row is put among the last still finds the first of two rows with one key, as a scan does (a hostile file may hold two).
#[test]
fn a_row_made_ahead_of_two_rows_with_one_key_leaves_the_first_of_them_the_one_found() {
    let mut section = Section::new("Geometry", Some(0));
    let mut rows: Vec<Arc<Row>> = (0..=36u32).map(|i| Arc::new(row(i, "1"))).collect();
    rows.extend([Arc::new(row(38, "first")), Arc::new(row(38, "second")), Arc::new(row(4_000_000_000, "last"))]);
    section.rows = rows.into_iter().collect();
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(section));
    assert!(sheet.sections[0].rows.len() > diacraft_doc::ROW_SCAN_LIMIT, "the section holds a table");
    assert_eq!(x_of(&sheet, 38).as_deref(), Some("first"), "the table is built here");
    let addr = CellAddr { section: Some(SectionKey::new("Geometry", Some(0))), row: Some(RowKey::Ix(37)), cell: CellName::new("X") };
    sheet.set_cell(&addr, Cell::constant(Value::from_text("new"))).unwrap();
    let ixs: Vec<Option<u32>> = sheet.sections[0].rows.iter().map(|r| r.ix).skip(36).collect();
    assert_eq!(ixs, [Some(36), Some(37), Some(38), Some(38), Some(4_000_000_000)], "the new row goes ahead of the two");
    assert_eq!(x_of(&sheet, 37).as_deref(), Some("new"));
    assert_eq!(x_of(&sheet, 38).as_deref(), Some("first"));
    assert_eq!(x_of(&sheet, 4_000_000_000).as_deref(), Some("last"));
    assert_eq!(x_of(&sheet, 36).as_deref(), Some("1"));
}
