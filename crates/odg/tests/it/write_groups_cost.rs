//! What saving many new groups costs: work that follows their number, counted (never timed), and ten thousand groups saved and read back.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, ShapeList, ShapeType};
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::{Drawing, cm};
use crate::write_core::{custom, open, open_bytes, part, real, set_num};
use crate::write_new::{push_shape, rectangle};

fn cell(formula: Option<&str>, value: f64) -> Cell {
    Cell::read(formula, Some(&value.to_string()), Some("IN"), None, &mut 1_000)
}

/// A group of two members, the box 8 by 6 centimetres at (`x`, 12), whose first member is `first` and the group `first + 2`; with a cell of its own when `data`.
fn pair(first: u32, x: f64, data: bool) -> Shape {
    let member = |id: u32, name: &str, size: (f64, f64), pin: (f64, f64)| {
        let mut m = rectangle(id, name, (0.0, 0.0, size.0, size.1));
        set_num(&mut m, "PinX", cm(pin.0));
        set_num(&mut m, "PinY", cm(pin.1));
        m
    };
    let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
    let mut shape = Shape::new(first + 2);
    shape.kind = ShapeType::Group;
    // A group that holds data is found by its name, which is unique on the page: each is named after its `ID`.
    shape.name = Some(Arc::from(format!("G{first}").as_str()));
    shape.name_u = shape.name.clone();
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, formula, value) in [
        ("PinX", None, cm(x + 4.0)),
        ("PinY", None, cm(15.0)),
        ("Width", None, cm(8.0)),
        ("Height", None, cm(6.0)),
        ("LocPinX", Some("Width*0.5"), cm(4.0)),
        ("LocPinY", Some("Height*0.5"), cm(3.0)),
    ] {
        sheet.cells.insert(CellName::new(name), cell(formula, value));
    }
    if data {
        sheet.cells.insert(CellName::new("Cost"), cell(None, 3.0));
    }
    shape.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
    shape
}

/// The page with `n` new groups on it.
fn with_groups(n: usize, data: bool) -> crate::write_core::Open {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut shapes: Vec<Arc<Shape>> = o.doc.pages.get(0).unwrap().shapes.iter().cloned().collect();
    for k in 0..n {
        shapes.push(Arc::new(pair(first + u32::try_from(3 * k).unwrap(), 10.0, data)));
    }
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(shapes);
    o
}

fn cost_of(n: usize, data: bool) -> WriteWork {
    let o = with_groups(n, data);
    let mut work = WriteWork::default();
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{n} groups: {e}"));
    work
}

/// Four times the groups is at most about four times the work, in each counter a save keeps, with or without data (which the document file carries): the entries of
/// the file, the placement of the members, the check of what was made.
#[test]
fn many_new_groups_cost_work_in_proportion_to_their_number() {
    for data in [false, true] {
        let (small, big) = (cost_of(100, data), cost_of(400, data));
        let fields: [(&str, u64, u64); 8] = [
            ("bytes_copied", small.bytes_copied, big.bytes_copied),
            ("bytes_written", small.bytes_written, big.bytes_written),
            ("attributes_written", small.attributes_written, big.attributes_written),
            ("cells_compared", small.cells_compared, big.cells_compared),
            ("scan_bytes", small.scan_bytes, big.scan_bytes),
            ("order_steps", small.order_steps, big.order_steps),
            ("read_back_bytes", small.read_back_bytes, big.read_back_bytes),
            ("steps", small.steps_used(), big.steps_used()),
        ];
        for (what, s, b) in fields {
            assert!(b <= s.saturating_mul(4).saturating_add(4_000), "data {data}, {what}: {s} for 100 groups and {b} for 400");
            assert!(s > 0 || matches!(what, "order_steps" | "read_back_bytes" | "scan_bytes"), "data {data}, {what} counted nothing");
        }
        assert!(big.bytes_written > small.bytes_written * 2, "what is written grows with the groups");
    }
}

/// The steps a save takes for the document file's entries are counted in the save's own steps, and only for a page that holds a group.
#[test]
fn the_entries_of_the_document_file_are_counted_in_the_steps_and_a_page_with_no_group_pays_nothing_for_them() {
    let none = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let mut work = WriteWork::default();
    // A page of the file with a new plain shape: no group, so the model's pages are not walked for entries.
    let mut o = none;
    push_shape(&mut o.doc, rectangle(o.index.next_id(0).unwrap(), "R", (2.0, 2.0, 1.0, 1.0)));
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work).unwrap();
    let plain = work.cells_compared;
    let (one, four) = (cost_of(1, true).cells_compared, cost_of(4, true).cells_compared);
    assert!(four > one + 3, "{one} compared for one group and {four} for four: the entries are counted");
    assert!(plain < one, "a page with no group: {plain}, with one: {one}");
}

/// Ten thousand new groups, thirty thousand shapes, are saved, read back with their members and saved again as no edit.
#[test]
fn ten_thousand_new_groups_are_saved_read_back_and_saved_again_as_no_edit() {
    let o = with_groups(10_000, true);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(part(&saved.bytes, "content.xml").matches("<draw:g").count(), 10_000);
    // Ten thousand groups, each with a name of its own and a cell: each has its entry, found by its name.
    assert_eq!(part(&saved.bytes, "DiaCraft/document.xml").matches("<diacraft:group").count(), 10_000);
    let back = open_bytes(saved.bytes.clone());
    let page = back.doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 10_001);
    assert!(page.shapes.iter().skip(1).all(|g| g.children.len() == 2 && g.sheet.cell("Cost").is_some()));
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

// ------------------------------------------------------------------------------------------------------------------ the walks for the document file are charged

/// A save of `doc` over `o`'s file with the step cap `cap` (the default when `None`): the work it counted, whether it was saved or refused.
fn work_of(o: &crate::write_core::Open, doc: &diacraft_doc::Document, cap: Option<u64>) -> (WriteWork, Result<(), diacraft_odg::WriteError>) {
    let mut options = WriteOptions::default();
    if let Some(cap) = cap {
        options.limits.max_write_steps = cap;
    }
    let mut work = WriteWork::default();
    let saved = write_counted(doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work).map(|_| ());
    (work, saved)
}

/// The model of `o` with one new group, which `shape` is made from.
fn with_group_of(o: &crate::write_core::Open, make: impl Fn(u32) -> Shape) -> diacraft_doc::Document {
    let first = o.index.next_id(0).unwrap();
    let mut doc = o.doc.clone();
    let mut shapes: Vec<Arc<Shape>> = doc.pages.get(0).unwrap().shapes.iter().cloned().collect();
    shapes.push(Arc::new(make(first)));
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(shapes);
    doc
}

/// A group whose own sheet holds one section of `rows` rows of ten cells each.
fn heavy_group(first: u32, rows: usize) -> Shape {
    let mut g = pair(first, 10.0, false);
    let mut section = diacraft_doc::Section::new("User", None);
    for r in 0..rows {
        let mut row = diacraft_doc::Row { name: Some(Arc::from(format!("r{r}").as_str())), ..diacraft_doc::Row::default() };
        for c in 0..10 {
            row.cells.insert(CellName::new(&format!("C{c}")), cell(None, 1.0));
        }
        section.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut g.sheet).sections.push(Arc::new(section));
    g
}

/// The scan's first finding: the walks of a group's sheet cover every row and every cell of a row, which were not charged (the rows of a section were walked for one step);
/// and the charge is made before the walk, so a cap stops it: a group of 2,000 rows of ten cells costs at least twice its cells, and with a cap under that the save is
/// refused with the count held at the cap and not run to the end.
#[test]
fn the_rows_and_the_row_cells_of_a_groups_sheet_are_charged_before_they_are_walked_and_a_cap_stops_the_walk() {
    let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let doc = with_group_of(&o, |first| heavy_group(first, 2_000));
    let (work, saved) = work_of(&o, &doc, None);
    saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(work.cells_compared >= 2 * 2_000 * 10, "twenty thousand cells walked twice are charged: {}", work.cells_compared);
    let (small, big) =
        (work_of(&o, &with_group_of(&o, |f| heavy_group(f, 500)), None).0, work_of(&o, &with_group_of(&o, |f| heavy_group(f, 2_000)), None).0);
    assert!(
        big.cells_compared <= small.cells_compared * 4 + 4_000,
        "in proportion: {} for 500 rows and {} for 2,000",
        small.cells_compared,
        big.cells_compared
    );
    // A cap under it: refused, and the walk stopped at the cap (20,000 rows of ten cells, 200,000 cells, against a cap of 30,000).
    let doc = with_group_of(&o, |first| heavy_group(first, 20_000));
    let (work, saved) = work_of(&o, &doc, Some(30_000));
    assert!(matches!(saved, Err(diacraft_odg::WriteError::Limit { .. })), "{saved:?}");
    assert!(work.cells_compared <= 60_000, "the walk stopped at the cap: {} steps counted for 200,000 cells", work.cells_compared);
}

/// A formula of 512 KiB that names sheets, in a member of each of 40 groups (one text, shared): each is charged by the bytes the scan of it walks (a step for each 64
/// bytes), over 320,000 steps for the 40, and a cap of 150,000 stops the scan; and the set of the `ID`s named is the groups' and no bigger, whatever the formulas name.
#[test]
fn a_formula_is_charged_by_its_bytes_and_the_ids_it_names_are_only_kept_for_groups() {
    let big: String = "1+".repeat(262_000) + "Sheet.3!X+Sheet.4!X+Sheet.99999!X";
    let long = Cell::with_formula(&big, Some(diacraft_doc::Value::from_text("1")));
    let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let make = |first: u32| {
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        for k in 0..40u32 {
            let mut g = pair(first + 3 * k, 10.0, false);
            let member = Arc::make_mut(g.children.get_mut(0).unwrap());
            Arc::make_mut(&mut member.sheet).cells.insert(CellName::new("Cost"), long.clone());
            shapes.push(Arc::new(g));
        }
        shapes
    };
    let first = o.index.next_id(0).unwrap();
    let mut doc = o.doc.clone();
    let mut shapes: Vec<Arc<Shape>> = doc.pages.get(0).unwrap().shapes.iter().cloned().collect();
    shapes.extend(make(first));
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(shapes);
    let (work, saved) = work_of(&o, &doc, None);
    saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(work.cells_compared >= 40 * (big.len() as u64 / 64), "40 scans of {} bytes: {} steps", big.len(), work.cells_compared);
    let (work, saved) = work_of(&o, &doc, Some(150_000));
    assert!(matches!(saved, Err(diacraft_odg::WriteError::Limit { .. })), "{saved:?}");
    assert!(work.cells_compared <= 200_000, "the scan stopped at the cap: {}", work.cells_compared);
}

/// A group of one member: a rectangle 1 by 1 centimetre, the group's box the same, at (`x`, 12) centimetres; the member is `first` and the group `first + 1`.
fn one_member(first: u32, x: f64) -> Shape {
    let mut m = rectangle(first, "A", (0.0, 0.0, 1.0, 1.0));
    set_num(&mut m, "PinX", cm(0.5));
    set_num(&mut m, "PinY", cm(0.5));
    let mut shape = Shape::new(first + 1);
    shape.kind = ShapeType::Group;
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, formula, value) in [
        ("PinX", None, cm(x + 0.5)),
        ("PinY", None, cm(12.5)),
        ("Width", None, cm(1.0)),
        ("Height", None, cm(1.0)),
        ("LocPinX", Some("Width*0.5"), cm(0.5)),
        ("LocPinY", Some("Height*0.5"), cm(0.5)),
    ] {
        sheet.cells.insert(CellName::new(name), cell(formula, value));
    }
    shape.children = ShapeList::from_vec(vec![Arc::new(m)]);
    shape
}

/// The page with `n` new groups of one member each, and the cost of saving it.
fn one_member_cost(n: usize) -> (WriteWork, Vec<u8>) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut shapes: Vec<Arc<Shape>> = o.doc.pages.get(0).unwrap().shapes.iter().cloned().collect();
    for k in 0..n {
        shapes.push(Arc::new(one_member(first + u32::try_from(2 * k).unwrap(), 10.0)));
    }
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(shapes);
    let mut work = WriteWork::default();
    let written = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{n} groups of one member were not saved: {e}"));
    (work, written.bytes)
}

/// The review: the members of each new group were found by a scan of every new shape, for each group (twice), so that saving groups of one member took
/// time that followed the square of their number (40,000 of them 34 s, 60,000 over 75 s). The members are listed once under their group and each group finds its own,
/// so the members gathered are three for each member (listed, then looked at by each of the two passes that make the elements), whatever the number of groups: counted,
/// never timed; and **50,000 groups of one member are saved**, and read back in full.
#[test]
fn the_members_of_new_groups_are_gathered_once_for_each_and_fifty_thousand_groups_of_one_member_are_saved() {
    let counts: Vec<(usize, u64)> = [100, 400, 1_600].iter().map(|&n| (n, one_member_cost(n).0.group_members)).collect();
    for (n, gathered) in &counts {
        assert_eq!(*gathered, 3 * *n as u64, "{n} groups of one member: each member is gathered three times, and no more");
    }
    // 50,000 (the file of 60,000 no longer reads under the 20 million items, a rectangle and a group holding more of them).
    let (n, saved) = (50_000usize, one_member_cost(50_000));
    assert_eq!(saved.0.group_members, 3 * n as u64, "fifty thousand groups: the same three for each member");
    let (doc, report) = diacraft_odg::read(&saved.1, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let page = doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 1 + n, "the shape of the file and the groups");
    assert!(page.shapes.iter().skip(1).all(|g| g.children.len() == 1), "each group holds its one member");
}
