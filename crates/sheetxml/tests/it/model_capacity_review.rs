//! Part A: the full review's findings. Each case that would take the address space runs in a child under
//! `ulimit -v 3000000` and ends refused in words or saved; an abort is the failure.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, FormulaSlot, Page, Section, Shape, ShapeList, Sheet, Text, TextPart, Value};
use diacraft_sheetxml::model::{
    CodecError, MemBlobs, OPEN_MODEL_BYTES, OPEN_PART_BYTES, OPEN_POOL_BYTES, PartRole, decode, encode, encode_for_save, grown_bytes_until,
    held_bytes, held_bytes_until, open_limits, save_parts,
};
use proptest::prelude::*;

const SCENARIO: &str = "SHEETXML_CAPACITY2_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// One page of `n` shapes that all hold one sheet whose one cell is a value of `len` bytes: what a duplicate of a shape makes (the sheet is an `Arc`).
fn long_value_doc(n: usize, len: usize) -> Document {
    let big: String = "x".repeat(len);
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text(&big)));
    let sheet = Arc::new(sheet);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let shapes: Vec<Arc<Shape>> = (0..n)
        .map(|i| {
            let mut shape = Shape::new(u32::try_from(i + 1).unwrap());
            shape.sheet = Arc::clone(&sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    doc
}

/// One page of `n` shapes that each hold a sheet of its own, whose one cell is a value of `len` bytes that no other has.
fn distinct_value_doc(n: usize, len: usize) -> Document {
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let shapes: Vec<Arc<Shape>> = (0..n)
        .map(|i| {
            let value = format!("{i:08}{}", "x".repeat(len - 8));
            let mut sheet = Sheet::default();
            sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text(&value)));
            let mut shape = Shape::new(u32::try_from(i + 1).unwrap());
            shape.sheet = Arc::new(sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    doc
}

/// A page of `shapes` shapes that each hold 8,192 sections of a cell, each of a value of its own, so that nothing is shared: a model of about 1.5 KB for a section.
fn many_sections_doc(shapes: usize) -> Document {
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let list: Vec<Arc<Shape>> = (0..shapes)
        .map(|i| {
            let mut sheet = Sheet::default();
            for k in 0..diacraft_doc::MAX_SECTIONS_PER_SHEET {
                let mut section = Section::new(&format!("s{k}"), None);
                section.cells.insert(CellName::new("c"), Cell::constant(Value::from_text(&format!("{i}-{k}"))));
                sheet.sections.push(Arc::new(section));
            }
            let mut shape = Shape::new(u32::try_from(i + 1).unwrap());
            shape.sheet = Arc::new(sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(list);
    doc.pages.push(Arc::new(page));
    doc
}

/// A document of one page and one shape whose sheet is `sheet` and whose text is `text`.
fn shape_doc(sheet: Sheet, text: Option<Text>) -> Document {
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    shape.text = text.map(Arc::new);
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    doc.pages.push(Arc::new(page));
    doc
}

/// What `save_parts` says of `doc`: the error, or the number of parts it handed over.
fn saved_parts(doc: &Document) -> Result<usize, CodecError> {
    let mut parts = 0usize;
    save_parts(doc, &mut MemBlobs::new(), &open_limits(), &mut |_| {
        parts += 1;
        Ok(())
    })
    .map(|()| parts)
}

fn limit_words(r: Result<impl std::fmt::Debug, CodecError>) -> String {
    match r {
        Err(CodecError::Limit { what, max }) => format!("{what}:{max}"),
        other => format!("not a limit: {other:?}"),
    }
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn capacity2_child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    match scenario.as_str() {
        // A1, as the review found it: twelve thousand shapes that share a sheet of one cell of 256 KiB. The model holds it once, and the file says it once: saved as
        // what it holds (it was written for each shape, 3 GiB, and the process aborted), and read back as it was.
        "long_value_shared" => {
            let doc = long_value_doc(12_000, 256 << 10);
            assert_eq!(saved_parts(&doc), Ok(2), "save_parts");
            let held = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("encode_for_save");
            let again = encode(&doc, &mut MemBlobs::new()).expect("encode");
            assert_eq!((held.pages.len(), again.pages.len()), (1, 1));
            let pages: Vec<&_> = held.pages.iter().map(|(_, e)| e).collect();
            let mut budget = diacraft_sheetxml::model::Budget::for_open(&open_limits());
            let opened = decode(&held.document, &pages, &[], &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("opens");
            assert!(opened == doc, "the document that was saved is the one that was opened");
            println!("CAP2 shared: peak {} MiB", status_kib("VmHWM") >> 10);
            assert!(status_kib("VmHWM") >> 10 < 400);
        }
        // A1: the same value, a copy of its own on each shape: 400 of 256 KiB are 100 MiB of part, saved and read back.
        "long_value_400" => {
            let doc = distinct_value_doc(400, 256 << 10);
            assert_eq!(saved_parts(&doc), Ok(2), "save_parts");
            let held = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("encode_for_save");
            assert_eq!(held.pages.len(), 1);
            let pages: Vec<&_> = held.pages.iter().map(|(_, e)| e).collect();
            let mut budget = diacraft_sheetxml::model::Budget::for_open(&open_limits());
            let opened = decode(&held.document, &pages, &[], &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("opens");
            assert!(opened == doc, "the document that was saved is the one that was opened");
        }
        // A1: 600 values of 256 KiB, a copy of its own each, are 150 MiB of model (within the pool of an open) and 150 MiB of part, past the 128 MiB a part may be:
        // refused **while the part is made**, at the byte that passes the cap, so that the process never holds the tree it refuses.
        "long_value_600" => {
            let doc = distinct_value_doc(600, 256 << 10);
            assert!(held_bytes(&doc) < OPEN_MODEL_BYTES, "within the pool, so that only the part's bytes refuse it");
            let before = status_kib("VmHWM") >> 10;
            assert_eq!(limit_words(saved_parts(&doc)), format!("bytes in one part as written:{OPEN_PART_BYTES}"), "save_parts");
            assert_eq!(limit_words(encode(&doc, &mut MemBlobs::new())), format!("bytes in one part as written:{OPEN_PART_BYTES}"), "encode");
            let peak = status_kib("VmHWM") >> 10;
            println!("CAP2 refused at a peak of {peak} MiB (the model {before} MiB)");
            assert!(peak < before + 200, "the refused part was held: {peak} MiB against the model's {before} MiB");
        }
        // A1: 3,000 values of 256 KiB are 750 MiB of model, within what a save holds (1.5 GiB) and 750 MiB of part: refused at the byte that passes a part's cap.
        "long_value_3000" => {
            let doc = distinct_value_doc(3_000, 256 << 10);
            let before = status_kib("VmHWM") >> 10;
            assert_eq!(limit_words(saved_parts(&doc)), format!("bytes in one part as written:{OPEN_PART_BYTES}"), "save_parts");
            let peak = status_kib("VmHWM") >> 10;
            assert!(peak < before + 200, "the refused part was held: {peak} MiB against the model's {before} MiB");
        }
        // A1: 6,400 values of 256 KiB are 1.6 GiB of model: past what a save holds, counted as it is held before any tree is made.
        "long_value_6400" => {
            let doc = distinct_value_doc(6_400, 256 << 10);
            let before = status_kib("VmHWM") >> 10;
            let want = format!("bytes of the model in memory:{}", diacraft_sheetxml::model::EDIT_MODEL_BYTES);
            assert_eq!(limit_words(saved_parts(&doc)), want, "save_parts");
            assert_eq!(limit_words(encode_for_save(&doc, &mut MemBlobs::new(), &open_limits())), want, "encode_for_save");
            let peak = status_kib("VmHWM") >> 10;
            assert!(peak < before + 50, "a tree was made: {peak} MiB against the model's {before} MiB");
        }
        // A4: a page that the pre-flight's first pass takes (its items, its bytes and the floor are within their caps) and that an open refuses as a model of more than the
        // pool of the model: the second pass reads each part as an open does, and refuses it there, after the document part was handed over.
        "model_pool" => {
            // The pool is 1,344 MiB (the model's 960 and the blobs' 384, which this drawing does not spend), and a shape of these is 13.4 MiB (12 before a change priced a node of
            // cells at the law of the memory tests): 108 of them, 1,450 MiB, are past it and within the 1,536 MiB the first pass lets by (120 were, at 12). A page of them is a
            // stream (each shape is a child), so that the items of the page are not what refuses it.
            let doc = many_sections_doc(108);
            let floor = held_bytes(&doc);
            println!("CAP2 model pool: held {} MiB of {} MiB", floor >> 20, OPEN_POOL_BYTES >> 20);
            let mut roles = Vec::new();
            let out = diacraft_sheetxml::model::save_parts_bytes(&doc, &mut MemBlobs::new(), &open_limits(), &mut |part| {
                roles.push(part.role);
                Ok(())
            });
            println!("CAP2 model pool: {:?}, roles {roles:?}, peak {} MiB", out.as_ref().err().map(ToString::to_string), status_kib("VmHWM") >> 10);
            assert_eq!(limit_words(out), format!("bytes of the model in memory:{OPEN_POOL_BYTES}"));
            assert_eq!(roles, vec![PartRole::Document], "the page the open refuses was not handed over");
        }
        other => panic!("no scenario {other}"),
    }
    println!("capacity2: no abort");
}

#[cfg(target_os = "linux")]
fn run(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_capacity_review::capacity2_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("capacity2: no abort"), "{scenario}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("CAP2")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
fn the_floor_counts_the_characters_of_each_cell_and_of_the_text_and_the_element_of_a_text() {
    use diacraft_sheetxml::model::cost::element_bytes;
    let floor = |sheet: Sheet, text: Option<Text>| held_bytes(&shape_doc(sheet, text));
    let sheet_of = |cell: Cell| {
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("Name"), cell);
        sheet
    };
    let bare = floor(sheet_of(Cell::new()), None);
    // The value, the unit, the error and the text of a formula of its own: each is counted by its bytes.
    let mut budget = 1usize << 20;
    let mut full = Cell::constant(Value::from_text("abcde"));
    full.unit = Some(Arc::from("mm"));
    full.error = Some(Arc::from("#N/A!"));
    full.formula = FormulaSlot::read("1+1+1", &mut budget);
    assert_eq!(floor(sheet_of(full.clone()), None) - bare, 5 + 2 + 5 + 5);
    for (what, cell, bytes) in [
        ("value", Cell::constant(Value::from_text("abc")), 3),
        ("unit", Cell { unit: Some(Arc::from("mm")), ..Cell::new() }, 2),
        ("error", Cell { error: Some(Arc::from("#N/A")), ..Cell::new() }, 4),
        ("formula", Cell { formula: FormulaSlot::read("SUM(1,2)", &mut budget), ..Cell::new() }, 8),
        ("inherited formula", Cell { formula: FormulaSlot::Inherit, ..Cell::new() }, 0),
    ] {
        assert_eq!(floor(sheet_of(cell), None) - bare, bytes, "{what}");
    }
    // The name of a cell too.
    let mut longer = Sheet::default();
    longer.cells.insert(CellName::new("A longer name"), Cell::new());
    assert_eq!(floor(longer, None) - bare, "A longer name".len() as u64 - "Name".len() as u64);
    // A shape that has a text is the element of the text and the characters of its parts: the characters of a run and of a field, and not a marker.
    let none = floor(Sheet::default(), None);
    let empty = Text::default();
    assert_eq!(floor(Sheet::default(), Some(empty)) - none, element_bytes("text"), "the element of a text");
    let parts = Text {
        parts: vec![TextPart::Chars(Arc::from("hello")), TextPart::Cp(2), TextPart::Fld { ix: 1, text: Arc::from("xy") }, TextPart::Pp(3)],
        ..Text::default()
    };
    assert_eq!(floor(Sheet::default(), Some(parts)) - none, element_bytes("text") + 5 + 2, "and what its runs and fields hold");
}

#[test]
#[cfg(target_os = "linux")]
fn twelve_thousand_shapes_that_share_a_long_value_are_saved_as_what_the_model_holds_and_none_aborts() {
    run("long_value_shared");
}

#[test]
#[cfg(target_os = "linux")]
fn a_page_that_the_first_pass_takes_and_an_open_refuses_for_the_pool_of_the_model_is_refused_at_save() {
    run("model_pool");
}

#[test]
#[cfg(target_os = "linux")]
fn four_hundred_shapes_that_hold_a_long_value_each_are_saved_and_read_back() {
    run("long_value_400");
}

#[test]
#[cfg(target_os = "linux")]
fn a_part_of_long_values_is_refused_at_the_byte_that_passes_its_cap_while_it_is_made() {
    run("long_value_600");
}

#[test]
#[cfg(target_os = "linux")]
fn long_values_that_the_model_holds_within_what_a_save_holds_are_still_refused_at_the_byte_that_passes_a_part() {
    run("long_value_3000");
}

#[test]
#[cfg(target_os = "linux")]
fn long_values_that_the_model_holds_past_what_a_save_holds_are_counted_before_any_tree_is_made() {
    run("long_value_6400");
}

// ------------------------------------------------------------------------------------------------------------------------------- what an edit adds

/// A shape with a sheet, sections and rows of its own (nothing in it is held by another shape), a text when `text`, and `depth` groups under it, each the same.
fn fresh_shape(id: u32, seed: u32, text: bool, depth: usize) -> Shape {
    let mut sheet = Sheet::default();
    for c in 0..(seed % 3) {
        sheet.cells.insert(CellName::new(&format!("c{c}")), Cell::constant(Value::from_text(&format!("{seed}-{c}"))));
    }
    for k in 0..(seed % 4) {
        let mut section = Section::new(&format!("s{seed}-{k}"), None);
        section.cells.insert(CellName::new("x"), Cell::constant(Value::from_text(&format!("{seed}{k}"))));
        for r in 0..(seed % 3) {
            let mut row = diacraft_doc::Row::default();
            row.cells.insert(CellName::new("y"), Cell::constant(Value::from_text(&format!("{k}{r}"))));
            section.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(section));
    }
    let mut shape = Shape::new(id);
    shape.sheet = Arc::new(sheet);
    if text {
        shape.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("t"))], ..Text::default() }));
    }
    if depth > 0 {
        shape.children = ShapeList::from_vec(vec![Arc::new(fresh_shape(id + 1, seed + 1, text, depth - 1))]);
    }
    shape
}

/// `before`, and a drawing that is `before` with one page more: what an edit adds, and no allocation of `before` replaced by it (a replacement frees what it replaces,
/// which the count of an addition does not see).
fn with_a_page_more(before: &Document, shapes: Vec<Shape>) -> Document {
    let mut next = before.clone();
    let mut page = Page::new(9_999, "new");
    page.shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
    next.pages.push(Arc::new(page));
    next
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// The count of what an edit added is the difference of the counts of the drawings, for an edit that only adds: shapes that have a text, shapes under shapes, and
    /// sheets that two of the new shapes share.
    #[test]
    fn what_an_edit_adds_is_what_the_drawing_holds_more(
        before in diacraft_testkit::model::any_document(diacraft_testkit::model::ModelSize::Small),
        seeds in prop::collection::vec((0u32..9, any::<bool>(), 0usize..6), 0..6),
        shared in any::<bool>(),
    ) {
        let mut shapes: Vec<Shape> = seeds.iter().enumerate().map(|(i, (seed, text, depth))| fresh_shape(1 + 10 * u32::try_from(i).unwrap(), *seed, *text, *depth)).collect();
        if shared && shapes.len() >= 2 {
            let sheet = Arc::clone(&shapes[0].sheet);
            shapes[1].sheet = sheet;
        }
        let next = with_a_page_more(&before, shapes);
        let (grown, held_before, held_next) =
            (grown_bytes_until(&before, &next, u64::MAX), held_bytes_until(&before, u64::MAX), held_bytes_until(&next, u64::MAX));
        prop_assert_eq!(grown, held_next - held_before);
    }
}

#[test]
fn a_new_shape_that_has_a_text_is_counted_with_the_element_of_its_text_and_a_group_nested_deep_is_counted_without_the_stack_of_its_depth() {
    use diacraft_sheetxml::model::cost::element_bytes;
    let before = Document::empty();
    let plain = with_a_page_more(&before, vec![fresh_shape(1, 0, false, 0)]);
    let with_text = with_a_page_more(&before, vec![fresh_shape(1, 0, true, 0)]);
    assert_eq!(
        grown_bytes_until(&before, &with_text, u64::MAX) - grown_bytes_until(&before, &plain, u64::MAX),
        element_bytes("text") + 1,
        "a shape that has a text is the element of the text more, and the character of its one part"
    );
    // A group 195 levels deep, each with a text: counted as the drawing holds it, in a thread whose stack is too small for the frames of a walk that recurses.
    let deep = with_a_page_more(&before, vec![fresh_shape(1, 2, true, 195)]);
    let counted = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(48 << 10)
            .spawn_scoped(scope, || {
                (grown_bytes_until(&before, &deep, u64::MAX), held_bytes_until(&deep, u64::MAX) - held_bytes_until(&before, u64::MAX))
            })
            .expect("a thread")
            .join()
            .expect("no stack overflow")
    });
    assert_eq!(counted.0, counted.1);
    assert!(counted.0 > 195 * element_bytes("text"), "{}", counted.0);
}

/// `before`: one page of one shape whose sheet holds three sections, each its own.
fn one_shape_three_sections() -> Document {
    let mut sheet = Sheet::default();
    for k in 0..3 {
        let mut section = Section::new(&format!("s{k}"), None);
        section.cells.insert(CellName::new("c"), Cell::constant(Value::from_text("v")));
        sheet.sections.push(Arc::new(section));
    }
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    doc.pages.push(Arc::new(page));
    doc
}

/// `before` with its shape edited by `edit`: what a command does, a copy of what it changes and the pointers of what it does not.
fn edited(before: &Document, edit: impl FnOnce(&mut Shape)) -> Document {
    let mut next = before.clone();
    let page = Arc::make_mut(next.pages.get_mut(0).unwrap());
    edit(Arc::make_mut(page.shapes.get_mut(0).unwrap()));
    next
}

#[test]
fn an_edit_adds_what_it_copied_and_not_the_sections_of_a_sheet_that_it_left_as_they_were() {
    let before = one_shape_three_sections();
    let grown = |next: &Document| grown_bytes_until(&before, next, u64::MAX);
    // A shape renamed: the page and the shape are copied, and the sheet is the very one.
    let renamed = edited(&before, |shape| shape.name = Some(Arc::from("n")));
    // A sheet that has a cell more: the sheet is copied, and its three sections are the very ones.
    let sheet_edited = edited(&before, |shape| {
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("extra"), Cell::new());
    });
    // The same and one section changed, and then every section.
    let one_section = edited(&before, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("extra"), Cell::new());
        Arc::make_mut(&mut sheet.sections[0]).del = true;
    });
    let every_section = edited(&before, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("extra"), Cell::new());
        for section in &mut sheet.sections {
            Arc::make_mut(section).del = true;
        }
    });
    let (g0, g_sheet, g1, g3) = (grown(&renamed), grown(&sheet_edited), grown(&one_section), grown(&every_section));
    let section = g1 - g_sheet;
    assert!(section > 0 && g_sheet > g0, "{g0} {g_sheet} {g1} {g3}");
    assert!(g_sheet - g0 < section, "a sheet's sections that the edit left are not counted: the sheet {} and a section {section}", g_sheet - g0);
    assert_eq!(g3 - g_sheet, 3 * section, "and each that it changed is");
    // Nothing edited is nothing added.
    assert_eq!(grown(&before.clone()), 0);
}

#[test]
fn the_count_of_what_an_edit_added_stops_where_it_passes_the_figure_and_not_at_it() {
    let before = Document::empty();
    let with = |n: usize| with_a_page_more(&before, (0..n).map(|i| fresh_shape(1 + 10 * u32::try_from(i).unwrap(), 2, true, 0)).collect());
    let full = |n: usize| grown_bytes_until(&before, &with(n), u64::MAX);
    let (g10, g11) = (full(10), full(11));
    assert!(g10 < g11);
    // One byte under what ten shapes are, the walk stops at the end of the tenth, the first count past the figure; at the figure it goes on into the eleventh, and
    // stops in it where it passes (a sheet's sections are counted one at a time): the count is what it is when the walk stops, which is not the whole.
    assert_eq!(grown_bytes_until(&before, &with(40), g10 - 1), g10, "stopped at the first count past the figure");
    let inside = grown_bytes_until(&before, &with(40), g10);
    assert!(inside > g10 && inside < g11, "at the figure the walk goes on and stops in the next shape: {inside} against {g10} and {g11}");
    assert!(grown_bytes_until(&before, &with(40), g10 / 2) > g10 / 2);
    assert!(grown_bytes_until(&before, &with(40), g10 / 2) < g10, "stopped early, and not counted whole");
    // A figure that is the whole of it is not past.
    assert_eq!(grown_bytes_until(&before, &with(10), g10), g10);
}

#[test]
fn the_count_of_what_a_drawing_holds_stops_where_it_passes_the_figure_and_not_at_it() {
    let with = |n: usize| with_a_page_more(&Document::empty(), (0..n).map(|i| fresh_shape(1 + 10 * u32::try_from(i).unwrap(), 2, true, 0)).collect());
    let held = |n: usize, most: u64| held_bytes_until(&with(n), most);
    let (h10, h11) = (held(10, u64::MAX), held(11, u64::MAX));
    assert!(h10 < h11 && held_bytes(&with(10)) == h10);
    // One byte under what ten shapes hold the count stops at the end of the tenth; at the figure it goes on and stops in the eleventh; under half of it, early.
    assert_eq!(held(40, h10 - 1), h10, "the first count past the figure");
    let inside = held(40, h10);
    assert!(inside > h10 && inside <= h11, "at the figure the walk goes on: {inside} against {h10} and {h11}");
    assert!(held(40, h10 / 2) > h10 / 2 && held(40, h10 / 2) < h10, "stopped early and not counted whole");
    assert_eq!(held(10, h10), h10, "a figure that is the whole of it is not past");
}

/// The spec's 6.1 and 6.4: the format's limits are those of 1.0 (a part of 6,000,000 items), and DiaCraft's own are lower (2,000,000): **a part within 1.0's
/// limits and past DiaCraft's is refused in words that name DiaCraft's limit**, and not as damaged.
#[test]
fn a_part_within_the_limits_of_1_0_and_past_the_opens_own_is_refused_in_words_that_name_the_opens_limit() {
    let items = usize::try_from(diacraft_sheetxml::model::OPEN_TREE_ITEMS).unwrap() + 1;
    assert!(items < 6_000_000, "a part of 1.0's limits");
    let mut text = String::with_capacity(items * 4 + 64);
    text.push_str("<a>");
    for _ in 0..items - 1 {
        text.push_str("<b/>");
    }
    text.push_str("</a>");
    let refused =
        diacraft_container::xml::parse_with(text.as_bytes(), &diacraft_sheetxml::model::open_limits().xml).expect_err("past the open's items");
    let words = refused.to_string();
    assert!(words.contains("2000000") && words.contains("items"), "the limit is named: {words}");
    assert_eq!(refused.class(), "xml_too_complex", "a limit of the reader's own, and not damage");
}
