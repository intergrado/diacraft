//! Item 3: **a reader that shares gives equal sections and sheets one allocation**, as the decoder of `.diacraft` does, and gives back what the copies it
//! drops were charged, so that the bytes it has charged are the bytes the model holds. It is a choice of the reader (`with_sharing`): a reader that does not choose it builds every
//! unit as it always did. What is equal is decided by `==` and never by a hash.
use std::sync::Arc;

use diacraft_doc::Sheet;
use diacraft_sheetxml::{Cx, Scope, SheetReader, SheetTally, Warnings};

use super::{BIG, Main, parse};

const PATH: &str = r#"<Section N="Geometry" IX="0"><Row IX="1" T="MoveTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row IX="2" T="LineTo"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row></Section>"#;

/// Reads `n` shapes' sheets, each from the element `<Shape>` the xml gives, through `sheet_child` and `share_sheet` as the `.vsdx` reader does, with a reader that shares or not.
/// Gives the sheets, and what the reader holds as charged.
fn read_shapes(shapes: &[String], sharing: bool) -> (Vec<Arc<Sheet>>, u64) {
    let mut reader = SheetReader::new(BIG).with_bytes(1 << 30);
    if sharing {
        reader = reader.with_sharing();
    }
    read_with(shapes, reader)
}

/// [`read_shapes`] with the reader given.
fn read_with(shapes: &[String], mut reader: SheetReader) -> (Vec<Arc<Sheet>>, u64) {
    let mut warnings = Warnings::new(50);
    let mut sheets = Vec::new();
    for xml in shapes {
        let el = parse(xml);
        let sc = Scope::<Main>::empty().enter(&el);
        let mut sheet = Sheet::default();
        let mut tally = SheetTally::default();
        for (typed, child) in el.elements().enumerate() {
            let csc = sc.enter(child);
            let mut cx = Cx::new(&mut warnings, "/page1.xml");
            reader.sheet_child(&mut cx, &mut sheet, &mut tally, child, &csc, typed).unwrap();
        }
        sheets.push(reader.share_sheet(sheet, &tally));
    }
    (sheets, reader.stats().bytes)
}

fn shape(pin: u32, with_path: bool) -> String {
    format!("<Shape><Cell N=\"PinX\" V=\"{pin}\"/><Cell N=\"PinY\" V=\"2\"/>{}</Shape>", if with_path { PATH } else { "" })
}

#[test]
fn equal_sections_are_one_allocation_and_what_the_copies_were_charged_is_given_back() {
    let shapes: Vec<String> = (0..20).map(|i| shape(i, true)).collect();
    let (alone, spent_alone) = read_shapes(&shapes, false);
    let (shared, spent_shared) = read_shapes(&shapes, true);
    for (a, b) in alone.iter().zip(&shared) {
        assert_eq!(**a, **b, "the same drawing");
    }
    assert!(!Arc::ptr_eq(&alone[0].sections[0], &alone[1].sections[0]), "a reader that does not share builds each section");
    for s in &shared {
        assert!(Arc::ptr_eq(&s.sections[0], &shared[0].sections[0]), "one section for twenty shapes");
    }
    // Nineteen sections are not held, and what they were charged is given back: what is charged is a section and twenty slots, and the sections' own bytes were the most of it.
    assert!(spent_shared + 19 * 1_000 < spent_alone, "{spent_shared} against {spent_alone}");
}

#[test]
fn a_section_that_differs_is_not_shared_and_a_key_that_agrees_proves_nothing() {
    let mut shapes: Vec<String> = (0..4).map(|i| shape(i, true)).collect();
    shapes.push(shape(9, false));
    shapes.push(format!("<Shape><Cell N=\"PinX\" V=\"1\"/>{}</Shape>", PATH.replace("V=\"1\"", "V=\"7\"")));
    let (sheets, _) = read_shapes(&shapes, true);
    assert!(Arc::ptr_eq(&sheets[0].sections[0], &sheets[3].sections[0]));
    assert!(sheets[4].sections.is_empty());
    assert!(!Arc::ptr_eq(&sheets[0].sections[0], &sheets[5].sections[0]));
    assert_ne!(sheets[0].sections[0], sheets[5].sections[0], "the section that differs in a value keeps its own");
}

#[test]
fn equal_sheets_are_one_allocation_and_their_cells_are_given_back_too() {
    // The shapes that are equal in every cell: the same sheet.
    let shapes: Vec<String> = (0..10).map(|_| shape(5, true)).collect();
    let (alone, spent_alone) = read_shapes(&shapes, false);
    let (shared, spent_shared) = read_shapes(&shapes, true);
    for s in &shared {
        assert!(Arc::ptr_eq(s, &shared[0]), "one sheet for ten equal shapes");
    }
    assert_eq!(alone[0], shared[0]);
    assert!(spent_shared < spent_alone / 3, "{spent_shared} against {spent_alone}");
}

#[test]
fn a_reader_that_shares_charges_no_more_than_one_that_does_not_and_the_budget_is_never_refused_for_the_table() {
    // A drawing in which nothing is equal: the table costs bytes, and it is taken only while the budget has them, so that a file that fits is read.
    let shapes: Vec<String> = (0..30).map(|i| shape(i, false)).collect();
    let (_, plain) = read_shapes(&shapes, false);
    let (_, shared) = read_shapes(&shapes, true);
    assert!(shared <= plain + 30 * 128, "{shared} against {plain}");
}

// ------------------------------------------------------------------------------------------------------------------------------- where the table's entries are (B2)

/// A shape of its own on every line: no two are equal, so that nothing is shared and the table only grows. Each has six cells and a path of five rows of two cells.
fn distinct_shape(i: u32) -> String {
    let rows: String = (1..=5)
        .map(|r| format!("<Row IX=\"{r}\" T=\"LineTo\"><Cell N=\"X\" V=\"{r}\" F=\"Width*{r}\"/><Cell N=\"Y\" V=\"{i}\" F=\"Height*1\"/></Row>"))
        .collect();
    format!(
        "<Shape><Cell N=\"PinX\" V=\"{i}.5\"/><Cell N=\"PinY\" V=\"2\"/><Cell N=\"Width\" V=\"1\"/><Cell N=\"Height\" V=\"0.75\"/><Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><Section N=\"Geometry\" IX=\"0\">{rows}</Section></Shape>"
    )
}

/// The CPU time the process has used, in hundredths of a second.
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    fields.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + fields.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// Reads `SHARED_READ_N` distinct shapes with a reader that shares (`SHARED_READ_SHARING=1`) or not, and walks every cell of the model two hundred times, in an order that is not the order it was read in: the walk's CPU time, which is
/// what a calculation over the model costs, beside the size of the model (it is the same either way).
#[test]
#[ignore = "run by the test below in a process of its own"]
fn walk_of_distinct_shapes_child() {
    let n: u32 = std::env::var("SHARED_READ_N").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sharing = std::env::var("SHARED_READ_SHARING").is_ok_and(|v| v == "1");
    let shapes: Vec<String> = (0..n).map(distinct_shape).collect();
    let (sheets, spent) = read_shapes(&shapes, sharing);
    drop(shapes);
    let start = cpu_ticks();
    let mut seen = 0usize;
    let touch = |name: &str, cell: &diacraft_doc::Cell, seen: &mut usize| {
        *seen = seen.wrapping_add(name.len()).wrapping_add(usize::from(cell.value.is_some()));
        if let diacraft_doc::FormulaSlot::Local(formula) = &cell.formula {
            *seen = seen.wrapping_add(formula.node_count());
        }
    };
    // The shapes in an order that is not the order they were read in, as a calculation that follows references reads them.
    let count = sheets.len().max(1);
    for pass in 0..200usize {
        for k in 0..sheets.len() {
            let Some(sheet) = sheets.get((k * 7_919 + pass) % count) else { continue };
            for (name, cell) in &sheet.cells {
                touch(name.as_str(), cell, &mut seen);
            }
            for section in &sheet.sections {
                for row in section.rows.iter() {
                    for (name, cell) in &row.cells {
                        touch(name.as_str(), cell, &mut seen);
                    }
                }
            }
        }
    }
    println!("WALK sharing={sharing} n={n} ticks={} seen={seen} charged={spent}", cpu_ticks() - start);
}

/// **A read that shares costs the model nothing in speed**: a drawing of distinct shapes, read with a table that finds nothing equal, is walked as fast as the same drawing read
/// without one. The table's entries were small allocations between the shapes' own, and the holes they left when the table was dropped made the walk (and the edits and the
/// calculation of a drawing the `.vsdx` reader had read) half as slow again. Each reader is run in a process of its own, three times, and the least of the three is compared.
/// `cargo test -p diacraft-sheetxml --test it -- --ignored distinct_shapes_are_walked`
#[test]
#[ignore = "a timing: it reads and walks 12,000 shapes six times, two hundred passes each"]
fn distinct_shapes_are_walked_as_fast_after_a_read_that_shares_as_after_one_that_does_not() {
    let least = |sharing: &str| -> u64 {
        (0..3)
            .map(|_| {
                let out = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "shared_read::walk_of_distinct_shapes_child", "--ignored", "--nocapture", "--test-threads=1"])
                    .env("SHARED_READ_N", "12000")
                    .env("SHARED_READ_SHARING", sharing)
                    .output()
                    .unwrap();
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                assert!(out.status.success(), "{text}");
                text.split("ticks=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).expect(&text)
            })
            .min()
            .unwrap()
    };
    let (alone, shared) = (least("0"), least("1"));
    println!("WALK least of three: {alone} ticks without the table, {shared} with it");
    assert!(shared * 100 <= alone * 125 + 100, "a model read with the table is walked in {shared} ticks, the same read without it in {alone}");
}

// ------------------------------------------------------------------------------------------------------------------------------- what the guards of the reader hold

/// A shape of its own pin (so that its sheet is never another's) with the one section `Geometry` number `ix`, whose one row has the value `v`.
fn shape_with_path(pin: u32, v: u32) -> String {
    format!(
        "<Shape><Cell N=\"PinX\" V=\"{pin}\"/><Section N=\"Geometry\" IX=\"0\"><Row IX=\"1\" T=\"MoveTo\"><Cell N=\"X\" V=\"{v}\"/><Cell N=\"Y\" V=\"0\"/></Row></Section></Shape>"
    )
}

#[test]
fn a_key_that_agrees_proves_nothing_to_the_reader_and_a_key_has_room_for_four_different_sections() {
    // Every key is the same: only `==` and the room of a key (four different units) decide. Six different sections, and then a twin of each.
    let shapes: Vec<String> = (0..6).map(|i| shape_with_path(i, i)).chain((0..6).map(|i| shape_with_path(100 + i, i))).collect();
    let (sheets, _) = read_with(&shapes, SheetReader::new(BIG).with_bytes(1 << 30).with_colliding_keys());
    for i in 0..6 {
        for j in 0..6 {
            if i != j {
                assert!(
                    !Arc::ptr_eq(&sheets[i].sections[0], &sheets[j].sections[0]),
                    "section {i} and section {j} differ, and a key never makes two one"
                );
            }
        }
        assert_eq!(*sheets[i].sections[0], *sheets[6 + i].sections[0], "a twin is equal");
    }
    for i in 0..4 {
        assert!(Arc::ptr_eq(&sheets[i].sections[0], &sheets[6 + i].sections[0]), "section {i} is within the room of its key: its twin is it");
    }
    for i in 4..6 {
        assert!(
            !Arc::ptr_eq(&sheets[i].sections[0], &sheets[6 + i].sections[0]),
            "section {i} was past the room of its key: it was never entered, and its twin is its own"
        );
    }
}

#[test]
fn a_key_that_agrees_proves_nothing_to_the_reader_of_sheets_and_has_room_for_four_different_sheets() {
    let shape = |i: u32| format!("<Shape><Cell N=\"PinX\" V=\"{i}\"/></Shape>");
    let shapes: Vec<String> = (0..6).chain(0..6).map(shape).collect();
    let (sheets, _) = read_with(&shapes, SheetReader::new(BIG).with_bytes(1 << 30).with_colliding_keys());
    for i in 0..6 {
        for j in 0..6 {
            if i != j {
                assert!(!Arc::ptr_eq(&sheets[i], &sheets[j]), "sheet {i} and sheet {j} differ");
            }
        }
        assert_eq!(*sheets[i], *sheets[6 + i], "a twin is equal");
    }
    for i in 0..4 {
        assert!(Arc::ptr_eq(&sheets[i], &sheets[6 + i]), "sheet {i} is within the room of its key");
    }
    for i in 4..6 {
        assert!(!Arc::ptr_eq(&sheets[i], &sheets[6 + i]), "sheet {i} was past the room of its key");
    }
}

#[test]
fn a_shared_section_keeps_its_place_in_its_sheets_list_charged_and_nothing_else_of_it() {
    use diacraft_sheetxml::model::cost::push_bytes;
    use diacraft_sheetxml::price::{POINTER_BYTES, with_margin};
    // Shapes of a pin each (so no sheet is shared) that hold nine sections, which are the same nine in every shape: what the third shape costs the reader, against what it costs
    // with no section at all, is the places of the nine sections in its list and nothing more, to the rounding of the margin.
    let k = 9u32;
    let sections: String = (0..k)
        .map(|ix| {
            format!(
                "<Section N=\"Geometry\" IX=\"{ix}\"><Row IX=\"1\" T=\"MoveTo\"><Cell N=\"X\" V=\"{ix}\"/><Cell N=\"Y\" V=\"0\"/></Row></Section>"
            )
        })
        .collect();
    let steady = |with: &str| {
        let shapes = |n: u32| (0..n).map(|pin| format!("<Shape><Cell N=\"PinX\" V=\"{pin}\"/>{with}</Shape>")).collect::<Vec<_>>();
        read_shapes(&shapes(3), true).1 - read_shapes(&shapes(2), true).1
    };
    let (full, none) = (steady(&sections), steady(""));
    let places: u64 = (0..k as usize).map(|before| with_margin(push_bytes(before, POINTER_BYTES))).sum();
    assert!(places > 100, "the nine places are a few dozen bytes each at the most and more than a hundred together: {places}");
    assert!(
        (full - none).abs_diff(places) <= u64::from(k),
        "the third shape costs {full} with nine shared sections and {none} with none: {} more, and their places are {places}",
        full - none
    );
}
