//! Work a file sets the size of and a formula repeats (item 2): a list searched item by item, a name searched among the
//! shapes, masters or styles that hold it, a property copied whole. Each is built at a size that was hostile and run to its end under a
//! hang guard (a minute: no cost is held to a time), and each asserts the steps its work is charged, which is how the work is bounded
//! whatever the load: a step must stand for a bounded cost.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_calc::{Budget, EvalOptions, OperandMode, RecalcReport, recalc_all_with};
use diacraft_doc::{Document, RowKey};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

/// A hang guard and no measure of cost: the slowest of these takes three seconds at load 50, and the steps are asserted.
const BOUND: Duration = Duration::from_secs(60);

fn accounted(r: &RecalcReport) -> u64 {
    r.agreed + r.differed.len() as u64 + r.differed_unlisted + r.skipped.total() + r.unsupported_cells() + r.unreached
}

fn run(doc: &Document, operands: OperandMode) -> RecalcReport {
    let started = Instant::now();
    let r = recalc_all_with(doc, &Budget::default(), EvalOptions { operands, ..EvalOptions::default() });
    let took = started.elapsed();
    assert!(took < BOUND, "took {took:?}: {r:?}");
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    r
}

fn reader(k: u32, formula: &str, saved: &str) -> (String, String, String) {
    (format!("R{k}"), formula.to_owned(), saved.to_owned())
}

fn readers_shape(id: u32, readers: Vec<(String, String, String)>) -> ShapeBuilder {
    let mut s = ShapeBuilder::new(id);
    for (name, formula, saved) in readers {
        s = s.row_formula("User", None, RowKey::name(&name), "Value", &formula, &saved);
    }
    s
}

/// The readers in shapes of 10,000 cells (a section holds at most 16,384 rows), the first shape with id `first`.
fn page_of_readers(mut page: PageBuilder, first: u32, readers: Vec<(String, String, String)>) -> PageBuilder {
    for (at, chunk) in readers.chunks(10_000).enumerate() {
        page = page.shape(readers_shape(first + at as u32, chunk.to_vec()));
    }
    page
}

#[test]
fn a_category_list_of_a_hundred_thousand_items_is_compared_in_linear_time() {
    // HASCATEGORY searched the shape's list for each item of its argument: 100,000 items each side took 24 s for 3,134 steps.
    let held: Vec<String> = (0..100_000).map(|i| format!("a{i}")).collect();
    let wanted: Vec<String> = (0..100_000).map(|i| format!("b{i}")).collect();
    let shape = ShapeBuilder::new(1)
        .row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", &held.join(";"))
        .row_cell("User", None, RowKey::name("Wanted"), "Value", &wanted.join(";"))
        .row_formula("User", None, RowKey::name("R"), "Value", "HASCATEGORY(User.Wanted)", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 1, "{r:?}");
    // Both lists are read (1.6 MB: 25,000 steps) and each of the 200,000 items is hashed or looked up (a step each).
    assert!(r.stats.steps >= 200_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn a_category_list_that_is_a_megabyte_of_one_item_each_side() {
    // `REPT("a;",524288)` against `REPT("b;",524288)` did not finish in 240 s. Each side is 524,288 items.
    let shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", &"a;".repeat(524_288)).row_formula(
        "User",
        None,
        RowKey::name("R"),
        "Value",
        "HASCATEGORY(REPT(\"b;\",524288))",
        "0",
    );
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 1, "{r:?}");
    assert!(r.stats.steps >= 524_288, "{} steps: {r:?}", r.stats.steps);
}

/// `n` top-level shapes all named `S`, with ids 1 to `n`, and a shape of `readers` cells that read the `Width` of one of them by name and `ID`.
fn same_named_shapes(n: u32, readers: u32) -> Document {
    let mut page = PageBuilder::new();
    for id in 1..=n {
        page = page.shape(ShapeBuilder::new(id).named("S", "S").cell("Width", "2"));
    }
    let cells = (0..readers)
        .map(|k| {
            // Half by the shape's peers, half by the page.
            let formula = if k % 2 == 0 { format!("S.{}!Width", n - k) } else { format!("ThePage!S.{}!Width", n - k) };
            reader(k, &formula, "2")
        })
        .collect();
    DocBuilder::new().page(1, page.shape(readers_shape(n + 1, cells))).build()
}

#[test]
fn a_name_among_two_hundred_thousand_shapes_of_that_name_is_found_by_its_id_without_a_search() {
    // `S.<id>!Width` walked the places that held the name and built a path for each: 1,179 microseconds a step, 6.5 hours at 20 million.
    let doc = same_named_shapes(200_000, 3_000);
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 3_000, "{r:?}");
    // The names of the level are listed once, a step for each shape.
    assert!(r.stats.steps >= 200_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn a_title_of_sixteen_megabytes_is_not_copied_by_each_call() {
    // `TITLE()` copied the title at one step a call: 139 microseconds a step.
    let cells = (0..8_000).map(|k| reader(k, "ISERROR(TITLE())", "0")).collect();
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(readers_shape(1, cells))).build();
    doc.title = Arc::from("t".repeat(16 << 20));
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 8_000, "{r:?}");
}

#[test]
fn a_page_name_a_shape_name_and_data_of_sixteen_megabytes_are_not_copied_by_each_call() {
    // The names a function reads and `DATA1` are text a file sets; sharing them is free and copying them is charged by length.
    let big = "n".repeat(16 << 20);
    let mut b = DocBuilder::new().master(1, MasterBuilder::new().name(&big).shape(ShapeBuilder::new(1)));
    let cells = (0..4_000)
        .map(|k| match k % 3 {
            0 => reader(k, "ISERROR(PAGENAME())", "0"),
            1 => reader(k, "ISERROR(DATA1())", "0"),
            _ => reader(k, "ISERROR(NAME())", "0"),
        })
        .collect();
    b = b.page(1, PageBuilder::new().name(&big).shape(readers_shape(1, cells).data(1, &big).named(&big, &big)));
    let r = run(&b.build(), OperandMode::Cached);
    assert_eq!(r.agreed, 4_000, "{r:?}");
}

#[test]
fn a_master_among_sixty_five_thousand_is_found_by_its_name_without_a_search() {
    // `Masters[x]!` searched the masters for each reference: 26 microseconds a step.
    let mut b = DocBuilder::new();
    for m in 1..=65_535u32 {
        b = b.master(m, MasterBuilder::new().name(&format!("m{m}")).shape(ShapeBuilder::new(1).cell("Width", "2")));
    }
    let cells = (0..20_000).map(|k| reader(k, &format!("Masters[m{}]!Sheet.1!Width", 65_535 - (k % 1_000)), "2")).collect();
    let doc = b.page(1, page_of_readers(PageBuilder::new(), 1, cells)).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 20_000, "{r:?}");
    // The masters are listed once, a step for each.
    assert!(r.stats.steps >= 65_535, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn a_style_among_sixty_five_thousand_is_found_by_its_name_without_a_search() {
    // `Styles!x!` searched the styles for each reference: 13 microseconds a step.
    let mut b = DocBuilder::new();
    for s in 1..=65_000u32 {
        b = b.style(s, StyleBuilder::new().name(&format!("s{s}")).cell("LineWeight", "2"));
    }
    let cells = (0..2_000).map(|k| reader(k, &format!("Styles!s{}!LineWeight", 65_000 - (k % 1_000)), "2")).collect();
    let doc = b.page(1, page_of_readers(PageBuilder::new(), 1, cells)).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 2_000, "{r:?}");
    // The reads cost a few steps each, 2,000 of them; the styles are listed once, a step for each.
    assert!(r.stats.steps >= 65_000, "{} steps: {r:?}", r.stats.steps);
}

// ---- the sweep for the same class: a walk or a copy that a file or a formula sets the size of ---------------------------------------

fn relationships_of(code: u32, first: u32, last: u32) -> String {
    let targets: Vec<u32> = (first..=last).collect();
    crate::relationships(&[(code, &targets)])
}

#[test]
fn the_targets_of_a_relationships_cell_are_charged_by_the_call_that_copies_them() {
    // `LISTMEMBERCOUNT()` and its kin copied the 2,000 live targets under a code at one step a call.
    let mut shape = ShapeBuilder::new(1).formula("Relationships", &relationships_of(2, 2, 2001), "0");
    for k in 0..300 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "LISTMEMBERCOUNT()", "2000");
    }
    let mut page = PageBuilder::new().shape(shape);
    for id in 2..=2001 {
        page = page.shape(ShapeBuilder::new(id));
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 301, "{r:?}");
    assert!(r.stats.steps >= 300 * 2_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn the_category_lists_of_the_containers_are_charged_by_their_length_in_every_mode() {
    // `CONTAINERSHEETREF(1,"Lane")` read through the 100 KB category list of each of 20 containers at one step each in Live mode, where the
    // cells are shared and not decoded again.
    let mut shape = ShapeBuilder::new(1).formula("Relationships", &relationships_of(4, 2, 21), "0");
    for k in 0..20 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "ISERROR(CONTAINERSHEETREF(1,\"Lane\")!Height)", "1");
    }
    let mut page = PageBuilder::new().shape(shape);
    for id in 2..=21 {
        page = page.shape(ShapeBuilder::new(id).row_cell("User", None, RowKey::name("msvShapeCategories"), "Value", &"x;".repeat(50_000)));
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = run(&doc, OperandMode::Live);
    assert!(r.stats.steps >= 20 * 20 * 1_500, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn the_bindings_of_an_evalcell_are_charged_by_the_arg_calls_that_search_them() {
    // `ARG(name)` searched the pairs `EVALCELL` bound, in order, at one step a call: 1,000 calls over 1,000 pairs. Each call is now 16 steps
    // (1,000 to 64 a step) more.
    let sum: Vec<String> = (0..1_000).map(|k| format!("ARG(\"a{k}\")")).collect();
    let pairs: Vec<String> = (0..1_000).map(|k| format!("\"a{k}\",{k}")).collect();
    let shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("F"), "Value", &format!("SUM({})", sum.join(",")), "0").row_formula(
        "User",
        None,
        RowKey::name("R"),
        "Value",
        &format!("EVALCELL(User.F,{})", pairs.join(",")),
        "499500",
    );
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let r = run(&doc, OperandMode::Cached);
    let by_search = 1_000 * (1_000 / 64 + 1);
    // What the call costs without the search: the nodes of the two formulas and a few steps for each pair.
    assert!(r.stats.steps >= by_search + 5_000, "{} steps: {r:?}", r.stats.steps);
    assert!(r.stats.steps < 100_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn the_arguments_of_a_call_that_qualifies_a_reference_are_charged_when_they_are_copied() {
    // The qualifier call is built from a copy of its arguments, and its arity is checked before they are worked out: 3,000 nodes copied at
    // no step by each of 500 cells.
    let args = vec!["1"; 3_000].join(",");
    let mut shape = ShapeBuilder::new(1);
    for k in 0..500 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", &format!("ISERROR(SHEETREF(SUM({args}))!Width)"), "1");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let r = run(&doc, OperandMode::Cached);
    assert!(r.stats.steps >= 500 * 3_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn a_megabyte_of_white_space_read_as_a_truth_value_is_charged_by_its_length_in_live_mode() {
    // `IF(User.Big,1,2)` trimmed the text before charging it, and Live mode shares the cell's text between readers.
    let mut shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("Big"), "Value", &" ".repeat(1 << 20));
    for k in 0..100 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "IF(User.Big,1,2)", "2");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let r = run(&doc, OperandMode::Live);
    assert!(r.stats.steps >= 100 * 16_384, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn text_a_file_sets_is_charged_by_its_length_by_each_function_that_copies_or_parses_it() {
    // In Live mode the cell's text is shared between its readers, so a function that copies or parses it pays for it itself. Each of these
    // takes a megabyte (16,384 steps) from each of 20 cells.
    let big = "x".repeat(1 << 20);
    for formula in [
        "ISERROR(DATEVALUE(User.Big))",
        "ISERROR(FONT(User.Big))",
        "ISERROR(THEMEVAL(User.Big))",
        "ISERROR(EVALCELL(User.F,User.Big,1))",
        "ISERROR(SETF(User.X,User.Big))",
    ] {
        let mut shape = ShapeBuilder::new(1)
            .row_cell("User", None, RowKey::name("Big"), "Value", &big)
            .row_formula("User", None, RowKey::name("F"), "Value", "1", "1")
            .row_cell("User", None, RowKey::name("X"), "Value", "0");
        for k in 0..20 {
            shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", formula, "1");
        }
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        let r = run(&doc, OperandMode::Live);
        assert!(r.stats.steps >= 20 * 16_384, "{formula}: {} steps: {r:?}", r.stats.steps);
    }
}

#[test]
fn the_saved_value_of_a_master_cell_is_charged_by_its_length_for_each_instance_a_commit_adds_to() {
    // An instance that does not hold a cell its master's formula gives is compared with the master's saved value, which is decoded for each
    // instance: a megabyte, 50 times.
    let big = "x".repeat(1 << 20);
    let master = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("C"), "Value", "\"abc\"", &big);
    let mut page = PageBuilder::new();
    for id in 1..=50 {
        page = page.shape(ShapeBuilder::new(id).master(1));
    }
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(master)).page(1, page).build();
    let started = Instant::now();
    let c = diacraft_calc::commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert!(started.elapsed() < BOUND, "{c:?}");
    assert!(c.stats.steps >= 50 * 16_384, "{} steps: {c:?}", c.stats.steps);
}

#[test]
fn a_shape_among_two_hundred_thousand_is_found_by_its_id_through_one_listing_of_the_part() {
    // `Sheet.<id>!` builds the part's index of ids on the first lookup, a step for each shape; 3,000 reads then cost a few steps each.
    let mut page = PageBuilder::new();
    for id in 1..=200_000u32 {
        page = page.shape(ShapeBuilder::new(id).cell("Width", "2"));
    }
    let cells = (0..3_000).map(|k| reader(k, &format!("Sheet.{}!Width", 200_000 - k), "2")).collect();
    let doc = DocBuilder::new().page(1, page.shape(readers_shape(200_001, cells))).build();
    let r = run(&doc, OperandMode::Cached);
    assert_eq!(r.agreed, 3_000, "{r:?}");
    assert!(r.stats.steps >= 200_000, "{} steps: {r:?}", r.stats.steps);
}

#[test]
fn a_difference_prints_both_values_and_a_text_of_sixteen_megabytes_is_charged_for_the_printing() {
    // A cell whose result is the shared title and whose saved value is `x` differs; listing it prints the title, a pass over 16 MiB that no
    // step paid for: 1,500 such cells were minutes.
    let cells = (0..1_500).map(|k| reader(k, "TITLE()", "x")).collect();
    let mut doc = DocBuilder::new().page(1, readers_shape_page(cells)).build();
    doc.title = Arc::from("t".repeat(16 << 20));
    let r = run(&doc, OperandMode::Cached);
    // Each printing is 262,144 steps, so about 75 fit in the budget and the rest are not reached.
    assert!(r.budget_exhausted && r.unreached > 1_000 && !r.differed.is_empty(), "{r:?}");
}

fn readers_shape_page(cells: Vec<(String, String, String)>) -> PageBuilder {
    PageBuilder::new().shape(readers_shape(1, cells))
}
