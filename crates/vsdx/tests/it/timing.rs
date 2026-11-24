//! Crafted inputs, timed: every list, map or scan that the size of a file can grow (the lesson, where 25 million clean fuzz runs missed a 972 KB
//! file that hung the container, and, where a 75 KB file hung a lookup for hours).
//!
//! Each test builds the file that makes one thing as large as it can be, in code, and reads it under a **hang guard of a minute** (a test
//! machine under load is several times slower than a quiet one, so no cost is held to a time: the budgets, caps and peak memory are what the
//! tests assert). The measured times and peak memory are printed (`--nocapture`) and are in the report of the task. A cost claim holds only
//! for the fixture measured, so these fixtures have masters, styles, many pages and inherited cells, and not one of them
//! is the single shape of the others. The heaviest cases (a gigabyte of XML, the most items and the most cells in four parts) are `memory_price.rs`'s, each read in
//! a child under `ulimit -v 3000000`.

use std::time::{Duration, Instant};

use diacraft_testkit::package::{
    CT_MASTER, CT_MASTERS, CT_PAGE, CT_PAGES, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, nested_shapes, shape, shape_with,
};
use diacraft_vsdx::{Limits, VsdxError, read};

use crate::{read_err, read_ok, read_with};

/// A hang guard and no measure of cost (a cost is bounded by the reader's budgets and caps, which these tests assert): the slowest read here
/// takes a few seconds at load 50, so a minute tells a hang from a slow machine.
const BOUND: Duration = Duration::from_secs(60);

/// A field of `/proc/self/status` in KiB (`VmHWM`, the peak resident memory; `VmRSS`, what is resident now), where the system says.
fn status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| l.strip_prefix(field)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

fn peak_kib() -> Option<u64> {
    status_kib("VmHWM:")
}

/// Starts the peak over, so that what follows is measured and not what came before (Linux: writing 5 to `clear_refs`).
fn reset_peak() {
    // Where the system does not allow it, the figure is the process's whole peak, which is more and not less.
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// Reads `bytes`, prints the time and the memory the read added (the peak during it, less what was resident before: the input included in
/// neither), and fails past the bound. The peak is the process's, so a test run alone (`--exact --test-threads=1`) gives the figure of the report.
fn timed(label: &str, bytes: &[u8], limits: &Limits) -> Result<(diacraft_doc::Document, diacraft_vsdx::ReadReport), VsdxError> {
    timed_measured(label, bytes, limits).0
}

type Read = Result<(diacraft_doc::Document, diacraft_vsdx::ReadReport), VsdxError>;

/// As [`timed`], and the memory the read added in KiB too, for a test that asserts a peak. **The figure is the process's own peak**, so it is
/// the read's when no other test is running a large read beside it: every timed read takes [`HEAVY`], and the CI runs each test in a process
/// of its own.
fn timed_measured(label: &str, bytes: &[u8], limits: &Limits) -> (Read, Option<u64>) {
    timed_within(label, bytes, limits, BOUND)
}

/// As [`timed_measured`] with a bound of its own: the ignored cases that read the most a hostile file can ask for (minutes of a debug build) are
/// held to a minute, which the release build of the report keeps well within.
fn timed_within(label: &str, bytes: &[u8], limits: &Limits, bound: Duration) -> (Read, Option<u64>) {
    // One timed read at a time among these tests, so that the peak is the read's and not two reads' (the test binary runs its tests on threads).
    let _alone = HEAVY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    reset_peak();
    let before = status_kib("VmRSS:");
    let started = Instant::now();
    let result = read(bytes, limits);
    let took = started.elapsed();
    let added = peak_kib().zip(before).map(|(peak, before)| peak.saturating_sub(before));
    eprintln!(
        "[timing] {label}: {took:?} for {} bytes, read added {added:?} KiB at peak, {}",
        bytes.len(),
        if result.is_ok() { "read" } else { "refused" }
    );
    assert!(took < bound, "{label} took {took:?}");
    (result, added)
}

/// Held by every timed read, so that two of them under `cargo test` do not measure each other.
static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What the read of a part adds for each XML item it holds, as a measured bound in bytes: the tree `opc` builds (about 150 bytes an item) and
/// the model made from it (a kept element is 100 to 150 more), both alive while a part is read. The bound is two and a half times the figure
/// measured, since the allocator and the test profile move it; the figures are in the report of the task.
const BYTES_PER_ITEM: u64 = 600;

fn flat_shapes(n: usize) -> String {
    let mut s = String::with_capacity(n * 24);
    for i in 0..n {
        s.push_str("<Shape ID=\"");
        s.push_str(&i.to_string());
        s.push_str("\"/>");
    }
    s
}

/// A pages index of `n` rows that all name one page part of `shapes` shapes with two cells each.
pub(crate) fn pages_sharing_one_part(n: usize, shapes: usize) -> Vec<u8> {
    let one: String = (0..shapes).map(|i| shape(i as u32, &(cell("Width", "1") + &cell("Height", "2")))).collect();
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{one}</Shapes></PageContents>");
    let mut rows = String::new();
    for i in 0..n {
        rows.push_str(&format!("<Page ID=\"{i}\" Name=\"P{i}\"><Rel r:id=\"rId1\"/></Page>"));
    }
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn a_thousand_pages_that_name_one_part_parse_it_once_and_share_it_but_pay_for_it_a_thousand_times() {
    // 1,000 rows, one part of 1,200 shapes: one parse and one list, and a document of 1,200,000 shapes by the count a walker meets (9.6 million items, under the
    // 12 million of the items budget: with 1,500 shapes the file is refused as `too_many_items`, which the next test holds).
    let bytes = pages_sharing_one_part(1_000, 1_200);
    let (doc, report) = timed("1,000 pages, one part of 1,200 shapes", &bytes, &Limits::default()).unwrap();
    assert_eq!((doc.pages.len(), report.stats.shapes), (1_000, 1_200_000), "the shapes are charged for each use");
    assert!(doc.pages.get(0).unwrap().shapes.is_same_list_as(&doc.pages.get(999).unwrap().shapes), "and held once");
    assert_eq!(report.stats.cells, 2_400_000 + 3);
    // A walker of the whole document meets every use: the model's own check.
    let started = Instant::now();
    doc.check_limits().unwrap();
    eprintln!("[timing] check_limits over 1,200,000 uses of shapes: {:?}", started.elapsed());
}

#[test]
fn a_thousand_pages_that_name_a_part_of_fifteen_hundred_shapes_are_refused_by_the_items_budget_which_each_use_is_charged_to() {
    // 8 items a shape, 1,500 shapes, 1,000 uses: 12,004,022 items against the budget of 12 million, though the model holds one list of 1,500 shapes.
    let e = timed("1,000 pages, one part of 1,500 shapes", &pages_sharing_one_part(1_000, 1_500), &Limits::default()).err().unwrap();
    assert_eq!(e.class(), "too_many_items");
}

#[test]
fn a_thousand_pages_that_name_a_part_of_fifty_thousand_shapes_are_refused_when_the_budget_is_spent() {
    // 50 million shapes of walking for a file of 150 KB: the shapes budget (2,000,000) is spent at the forty-first page, and the file is refused
    // there, long before the thousandth.
    let e = timed("1,000 pages, one part of 50,000 shapes", &pages_sharing_one_part(1_000, 50_000), &Limits::default()).err().unwrap();
    // The items budget (12 million, about ten to a shape here) runs out well before the shapes one does.
    assert!(matches!(e.class(), "too_many_items" | "too_many_shapes"), "{}", e.class());
}

#[test]
fn thirty_thousand_masters_in_one_index_with_one_relationships_part_of_the_most_bytes_allowed() {
    // (30,000 rows of 112 bytes, not 37,449: the part must also pass the XML budget the relationships are now charged to.)
    // The relationships part is looked up by `Id` for each of the masters: a scan of 30,000 rows each would be 900 million comparisons. 30,000 rows
    // of the right type are about 4 MB, which is what the 4 MiB cap on a relationships part allows (it was 32 MiB).
    let mut rows = String::new();
    let mut rels = String::new();
    for i in 0..30_000u32 {
        rows.push_str(&format!("<Master ID=\"{i}\" NameU=\"M{i}\"><Rel r:id=\"rId{}\"/></Master>", 100_000 - i));
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{VIS}master\" Target=\"master1.xml\"/>", 100_000 - i));
    }
    let masters = format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Masters>");
    let rels_xml = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>"
    );
    let master = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></MasterContents>", shape(4, ""));
    let bytes = VsdxBuilder::new()
        .without_masters_part()
        .with(
            Pkg::new()
                .part("/visio/masters/masters.xml", CT_MASTERS, masters)
                .rel("/visio/document.xml", "rId60", &format!("{VIS}masters"), "masters/masters.xml")
                .part("/visio/masters/master1.xml", CT_MASTER, master)
                .part("/visio/masters/_rels/masters.xml.rels", "application/vnd.openxmlformats-package.relationships+xml", rels_xml),
        )
        .build();
    let (doc, report) = timed("30,000 masters, 30,000 relationships", &bytes, &Limits::default()).unwrap();
    assert_eq!(doc.masters.len(), 30_000);
    assert_eq!(report.stats.shapes, 30_000, "the one part is read once and charged for each of the masters that name it");
    assert_eq!(report.stats.max_rels_rows, 30_000);
}

#[test]
fn a_thousand_pages_with_thirty_thousand_relationships_in_the_index() {
    let mut rows = String::new();
    for i in 0..1_000u32 {
        rows.push_str(&format!("<Page ID=\"{i}\" Name=\"P{i}\"><Rel r:id=\"rId{}\"/></Page>", 100_000 - i));
    }
    let mut rels = String::new();
    for j in 0..30_000u32 {
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{VIS}page\" Target=\"page{}.xml\"/>", 100_000 - j, j % 3 + 1));
    }
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Pages>");
    let rels_xml = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>"
    );
    let mut pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/_rels/pages.xml.rels", "application/vnd.openxmlformats-package.relationships+xml", rels_xml);
    for k in 1..=3 {
        pkg = pkg.part(
            &format!("/visio/pages/page{k}.xml"),
            CT_PAGE,
            format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></PageContents>", shape(k, "")),
        );
    }
    let (doc, _) =
        timed("1,000 pages, 30,000 relationships", &VsdxBuilder::new().without_pages_part().with(pkg).build(), &Limits::default()).unwrap();
    assert_eq!(doc.pages.len(), 1_000);
}

#[test]
fn a_chain_of_sixty_five_thousand_styles_and_one_that_goes_round() {
    let n = 65_535u32;
    // 1 to n-1 each linked to the next in all three families, the last back to the first: one circle of every family through all of them.
    let styles: String = (1..=n)
        .map(|i| format!("<StyleSheet ID=\"{i}\" LineStyle=\"{0}\" FillStyle=\"{0}\" TextStyle=\"{0}\"/>", if i == n { 1 } else { i + 1 }))
        .collect();
    let (doc, report) = timed("65,536 styles in one circle", &VsdxBuilder::new().more_styles(&styles).build(), &Limits::default()).unwrap();
    assert_eq!(doc.styles.len(), 65_536);
    assert_eq!(report.warning_count("style_link_cycle"), 3, "one circle in each family");
}

#[test]
fn a_page_of_a_million_shapes_and_one_more_is_refused_before_it_is_built() {
    let bytes = |n: usize| VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&flat_shapes(n))).build();
    let (doc, report) = timed("1,000,000 flat shapes", &bytes(1_000_000), &Limits::default()).unwrap();
    assert_eq!((doc.pages.get(0).unwrap().shapes.len(), report.stats.shapes), (1_000_000, 1_000_000));
    // A million and one: refused when the million-and-first is reached, so the work is the parse and a million shapes, not a second million.
    let over = bytes(1_000_001);
    let e = timed("1,000,001 flat shapes", &over, &Limits::default()).err().unwrap();
    assert_eq!(e.class(), "too_many_shapes_in_a_part");
    // A caller cannot raise the cap past the model's: asking for no cap at all is still a million.
    let e = timed("1,000,001 flat shapes, no cap asked for", &over, &Limits { max_shapes_per_part: usize::MAX, ..Limits::default() }).err().unwrap();
    assert_eq!(e.class(), "too_many_shapes_in_a_part");
}

#[test]
fn the_cap_is_met_before_the_shapes_past_it_are_built() {
    // A part of 600,000 shapes against a cap of 10: the reader stops at the eleventh, and the time is the parse (which `opc` bounds) and no more. The
    // bound is a fraction of the time to model all 600,000, which the same read under no cap takes.
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&flat_shapes(600_000))).build();
    let limits = Limits { max_shapes_per_part: 10, ..Limits::default() };
    let started = Instant::now();
    let e = read_err(&bytes, &limits);
    let took = started.elapsed();
    eprintln!("[timing] 600,000 shapes: refused at a cap of 10 in {took:?}");
    assert_eq!(e.class(), "too_many_shapes_in_a_part");
    assert!(took < BOUND);
}

#[test]
fn seven_hundred_thousand_cells_across_seven_hundred_shapes_a_part_near_the_xml_cap() {
    // Five items a cell: 3.5 million of the 4 million items `opc` allows a part's tree.
    let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" U=\"IN\" F=\"Width*{i}\"/>")).collect();
    let shapes: String = (0..700).map(|i| shape(i, &cells)).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build();
    let limits = Limits::default();
    let (_, report) = timed("700,000 cells with formulas", &bytes, &limits).unwrap();
    assert_eq!(report.stats.cells, 700_000 + 3);
    // 8,000,000 nodes of budget: 700,000 formulas of three nodes each are 2,100,000, all parsed.
    assert_eq!(report.formula_errors.values().sum::<u64>(), 0, "{:?}", report.formula_errors);
}

#[test]
fn the_formula_budget_bounds_what_a_file_of_formulas_costs() {
    // 200 shapes of 1,000 cells of some twenty nodes each: some 4 million nodes asked for against a budget of 2 million, so half are parsed and the rest kept as
    // text. (At the default budget a file of 800 shapes of these is ended by the memory the read may hold before the node budget is spent: `memory_price.rs`.)
    let cells: String =
        (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"1\" F=\"(Width+Height)*(PinX-PinY)+(Width+Height)*(PinX-PinY)+{i}\"/>")).collect();
    let shapes: String = (0..200).map(|i| shape(i, &cells)).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build();
    let limits = Limits { max_formula_nodes: 2_000_000, ..Limits::default() };
    let (_, report) = timed("200,000 formulas of twenty nodes against a budget of two million nodes", &bytes, &limits).unwrap();
    assert!(report.stats.formula_nodes <= 2_000_000);
    let refused = report.formula_errors.get("too_many_nodes").copied().unwrap_or(0);
    assert!(refused > 50_000 && refused < 150_000, "{:?}: some parsed, the rest refused", report.formula_errors);
    assert_eq!(report.stats.formula_cells, 200_000);
}

#[test]
fn a_thousand_geometry_sections_of_a_hundred_rows() {
    let rows: String = (0..100).map(|r| format!("<Row T=\"LineTo\" IX=\"{r}\"><Cell N=\"X\" V=\"{r}\"/><Cell N=\"Y\" V=\"1\"/></Row>")).collect();
    let sections: String = (0..1_024).map(|g| format!("<Section N=\"Geometry\" IX=\"{g}\">{rows}</Section>")).collect();
    let shapes: String = (0..3).map(|i| shape(i, &sections)).collect();
    let (_, report) = timed(
        "3 shapes of 1,024 geometry sections of 100 rows",
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(report.stats.max_sections_in_a_sheet, 1_024);
}

#[test]
fn a_geometry_section_without_ix_in_a_sheet_of_a_thousand_is_still_linear() {
    // The number of an absent IX is a count of the sections before it: a scan of the sections so far, at most 1,024 each.
    let sections: String = (0..1_024).map(|_| "<Section N=\"Geometry\"/>".to_owned()).collect();
    let shapes: String = (0..500).map(|i| shape(i, &sections)).collect();
    timed(
        "500 shapes of 1,024 geometry sections with no IX",
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build(),
        &Limits::default(),
    )
    .unwrap();
}

#[test]
fn one_instance_with_a_hundred_thousand_members_that_each_name_a_shape_of_a_master_of_a_hundred_thousand() {
    // The set of a master's shape IDs is built once, when the first member asks, and not for each member.
    let master = MasterSpec::new(1, "M").shapes(&flat_shapes(100_000));
    let members: String = (0..100_000).map(|i| shape_with(i + 200_000, &format!(" MasterShape=\"{i}\""), "")).collect();
    let instance = shape_with(500_000, " Master=\"1\"", &format!("<Shapes>{members}</Shapes>"));
    let bytes = VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&instance)).build();
    let (_, report) = timed("100,000 members of an instance of a 100,000-shape master", &bytes, &Limits::default()).unwrap();
    assert_eq!(report.warning_count("master_shape_not_found"), 0);
}

#[test]
fn half_a_million_shapes_that_name_a_master_and_a_style_that_are_not_there() {
    // Each is a warning: counted exactly, worded for the first two hundred, and costing nothing more for the rest.
    let shapes: String = (0..500_000).map(|i| shape_with(i, " Master=\"999\" LineStyle=\"999\"", "")).collect();
    let (_, report) =
        timed("500,000 dangling links", &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build(), &Limits::default()).unwrap();
    assert_eq!(report.warning_count("master_not_found"), 500_000);
    assert_eq!(report.warning_count("style_not_found"), 500_000);
    assert!(report.warnings.len() <= diacraft_vsdx::MAX_REPORT_ENTRIES);
}

#[test]
fn a_hundred_thousand_cells_with_one_name_in_one_place_are_kept_and_cost_a_share_of_the_budget() {
    let dup: String = (0..100_000).map(|i| format!("<Cell N=\"X\" V=\"{i}\"/>")).collect();
    let (doc, report) = timed(
        "100,000 cells named X in one shape",
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &dup))).build(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cells.len(), 1);
    assert_eq!(report.warning_count("duplicate_cell"), 99_999);
    // The budgets bound it too: a cap on cells counts the cells kept as elements.
    let limits = Limits { max_cells: 5_000, ..Limits::default() };
    assert_eq!(read_err(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &dup))).build(), &limits).class(), "too_many_cells");
}

#[test]
fn an_attribute_of_the_most_an_xml_tree_allows_and_a_formula_that_long() {
    // The most an attribute may be is 1 MiB (the longest in the sample-file run is 43,350 bytes, and the cap was 16 MiB).
    let value = "9".repeat(1024 * 1024);
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &format!("<Cell N=\"X\" V=\"{value}\" F=\"{value}\"/>")))).build();
    let (doc, report) = timed("a 1 MiB value and a 1 MiB formula", &bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("X").unwrap().value.as_ref().unwrap().text().len(), value.len());
    // A formula of exactly 1 MiB is at the parser's limit and not past it.
    assert_eq!(report.formula_errors.get("too_long"), None);
    // One more byte of attribute is refused by the attribute cap before the formula parser sees it.
    let over = "9".repeat(1024 * 1024 + 1);
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &format!("<Cell N=\"X\" V=\"{over}\"/>")))).build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "xml_too_complex");
}

#[test]
fn unknown_elements_nested_as_deep_as_the_xml_limit_allows_a_thousand_times() {
    let deep = format!("{}{}", "<z>".repeat(150), "</z>".repeat(150));
    let shapes: String = (0..1_000).map(|i| shape(i, &deep)).collect();
    timed(
        "1,000 shapes with an unknown element 150 deep",
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build(),
        &Limits::default(),
    )
    .unwrap();
}

#[test]
fn half_a_million_connect_rows() {
    let connects: String = (0..500_000)
        .map(|i| format!("<Connect FromSheet=\"{i}\" FromCell=\"BeginX\" FromPart=\"9\" ToSheet=\"1\" ToCell=\"PinX\" ToPart=\"3\"/>"))
        .collect();
    let page = PageSpec::new(1, "P").shapes(&flat_shapes(10)).connects(&connects);
    let (_, report) = timed("500,000 connects", &VsdxBuilder::new().page(page).build(), &Limits::default()).unwrap();
    assert_eq!(report.stats.connects, 500_000);
}

#[test]
fn a_hundred_thousand_shapes_that_show_one_picture_copy_it_once() {
    let one = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let shapes: String = (0..100_000).map(|_| one.to_owned()).collect();
    let png = vec![7u8; 4 * 1024 * 1024];
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&shapes).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", &png));
    let (_, report) = timed("100,000 shapes, one 4 MiB picture", &builder.build(), &Limits::default()).unwrap();
    assert_eq!(report.stats.media_bytes, png.len() as u64);
}

#[test]
fn the_items_budget_bounds_the_model_of_many_parts() {
    // Five page parts of 40,000 shapes with cells each are 200,000 shapes and some 1.4 million items. A budget of a million refuses it, at the part that
    // passes it, though no single part is near `opc`'s cap.
    let one: String = (0..40_000).map(|i| shape(i, &cell("Width", "1"))).collect();
    let mut b = VsdxBuilder::new();
    for i in 1..=5 {
        b = b.page(PageSpec::new(i, "P").shapes(&one));
    }
    let bytes = b.build();
    let limits = Limits { max_items: 1_000_000, ..Limits::default() };
    let e = timed("five parts against a budget of a million items", &bytes, &limits).err().unwrap();
    assert_eq!(e.class(), "too_many_items");
    read_with(&bytes, &Limits { max_items: 3_000_000, ..Limits::default() });
}

#[test]
fn the_xml_budget_bounds_the_parsing_of_many_parts() {
    let one: String = (0..20_000).map(|i| shape(i, &cell("Width", "1"))).collect();
    let mut b = VsdxBuilder::new();
    for i in 1..=4 {
        b = b.page(PageSpec::new(i, "P").shapes(&one));
    }
    let bytes = b.build();
    let limits = Limits { max_xml_bytes: 1_000_000, ..Limits::default() };
    assert_eq!(timed("four parts against a budget of a million bytes of XML", &bytes, &limits).err().unwrap().class(), "too_much_xml");
    let (_, report) = read_ok(&bytes);
    assert!(report.stats.xml_bytes > 1_000_000);
}

#[test]
fn a_hundred_themes_of_the_biggest_that_fits_the_budget() {
    // The theme budget is 4 MiB (the largest total in the sample-file run is 198,535 bytes): a hundred of 41,000 bytes fit it.
    let theme = format!("<a:theme xmlns:a=\"urn:x\" name=\"{}\"><a:themeElements/></a:theme>", "t".repeat(40_900));
    let mut b = VsdxBuilder::new();
    for _ in 0..100 {
        b = b.theme(theme.as_bytes());
    }
    let (doc, report) = timed("100 themes of 41 KB", &b.build(), &Limits::default()).unwrap();
    assert_eq!(doc.themes.len(), 100);
    assert_eq!(report.warning_count("themes_over_budget"), 0);
}

#[test]
fn many_unknown_parts_and_many_content_type_rows() {
    // The container's own caps hold: 20,000 parts that the document never mentions cost the container and nothing here.
    let mut pkg = Pkg::new();
    for i in 0..20_000 {
        pkg = pkg.part(&format!("/foo/p{i}.bin"), "application/x-foo", [1u8]);
    }
    let bytes = VsdxBuilder::new().with(pkg).build();
    timed("20,000 unknown parts", &bytes, &Limits::default()).unwrap();
}

#[test]
fn the_deepest_a_default_read_allows_reads_on_a_small_stack() {
    // A shape at level d is at XML depth 2d + 1, so 90 levels of shapes and 15 of an unknown element are depth 198 of the 200 allowed, and an
    // unknown element 190 deep in a shape of the first level is depth 193. The reader's recursion is a frame a level, and the copy of an unknown
    // element is one too: a thread with a small stack must hold both, 256 KiB being an eighth of a test thread's and a quarter of the 1 MiB of a
    // browser's main thread. A frame of the test profile is larger than a release one, so this holds there with room.
    let unknown = format!("{}{}", "<z>".repeat(15), "</z>".repeat(15));
    let deep_shapes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&nested_shapes(90, &shape(9999, &unknown)))).build();
    let deep_unknown = format!("{}{}", "<z>".repeat(190), "</z>".repeat(190));
    let deep_element = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &deep_unknown))).build();
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let (doc, report) = read(&deep_shapes, &Limits::default()).unwrap();
            let levels = report.stats.max_shape_depth;
            let first = doc.pages.get(0).map(|p| p.shapes.len());
            drop(doc);
            let (doc, _) = read(&deep_element, &Limits::default()).unwrap();
            // The model is dropped on this thread too: its drop recurses once a level as well.
            (first, levels, doc.pages.get(0).unwrap().shapes.get(0).unwrap().passthrough.len())
        })
        .unwrap();
    assert_eq!(handle.join().unwrap(), (Some(1), 91, 1));
}

// ------------------------------------------------------------------------------------------------------------- the heaviest, in a child

// The heaviest cases (a gigabyte of XML, four parts at the items budget, four parts of the most empty cells) are `memory_price.rs`'s: each is read in a child under
// `ulimit -v 3000000`, where it is read or refused in words, and an abort fails the test.

// -------------------------------------------------------------------------------------------- first review: scans, relationships, memory

/// `shapes` shapes, each holding `sections` sections made by `section(i)`.
fn shapes_of_sections(shapes: usize, sections: usize, section: impl Fn(usize) -> String) -> Vec<u8> {
    let inner: String = (0..sections).map(section).collect();
    let one: String = (0..shapes).map(|i| format!("<Shape ID=\"{i}\">{inner}</Shape>")).collect();
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&one)).build()
}

#[test]
fn a_hundred_shapes_of_8_192_geometry_sections_without_ix_are_linear_in_the_sections() {
    // The counter of the earlier Geometry sections was a scan of the sections before each one: 8,192 of them is 33 million comparisons for a shape,
    // and a hundred shapes some 3 billion (7,500 shapes of 1,024 took 7.3 s, in a file of 1.3 MB).
    let bytes = shapes_of_sections(100, 8_192, |_| "<Section N=\"Geometry\"/>".to_owned());
    let (doc, report) = timed("100 shapes of 8,192 Geometry sections", &bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 100);
    assert_eq!(report.stats.max_sections_in_a_sheet, 8_192);
    let last = doc.pages.get(0).unwrap().shapes.get(99).unwrap().sheet.sections.last().unwrap().ix;
    assert_eq!(last, Some(8_191), "numbered by how many came before");
}

#[test]
fn a_hundred_shapes_of_8_192_sections_of_distinct_names_are_linear_in_the_sections() {
    // The repeated-name check was a scan too, for every section that is not `Geometry`.
    let bytes = shapes_of_sections(100, 8_192, |i| format!("<Section N=\"S{i}\"/>"));
    let (_, report) = timed("100 shapes of 8,192 sections of distinct names", &bytes, &Limits::default()).unwrap();
    assert_eq!(report.warning_count("duplicate_section"), 0);
    let bytes = shapes_of_sections(100, 8_192, |_| "<Section N=\"Same\"/>".to_owned());
    let (_, report) = timed("100 shapes of 8,192 sections of one name", &bytes, &Limits::default()).unwrap();
    assert_eq!(report.warning_count("duplicate_section"), 100 * 8_191);
}

#[test]
fn a_rows_section_of_the_most_rows_that_asks_for_each_of_them_by_key_is_linear() {
    // 320,000 rows, and a page of shapes that each name one: the row lookup in the model is a table (`doc` `Rows`), and the reader does none.
    let body: String = (0..320_000).map(|i| format!("<Row IX=\"{i}\" T=\"RelLineTo\"/>")).collect();
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("<Shape ID=\"1\"><Section N=\"Geometry\" IX=\"0\">{body}</Section></Shape>")))
        .build();
    let (doc, report) = timed("one section of 320,000 rows", &bytes, &Limits::default()).unwrap();
    assert_eq!(report.stats.max_rows_in_a_section, 320_000);
    let s = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    let started = Instant::now();
    for ix in (0..320_000u32).step_by(7) {
        assert!(s.sheet.sections[0].row(&diacraft_doc::RowKey::Ix(ix)).is_some());
    }
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}

/// Eighty pages that each have a relationships part of 29,000 rows (1.9 MB): charged to the XML budget as they are read, so the budget ends the
/// read where 80 of them (the review's: 100,000 rows each, 21 seconds and 1.1 GB) were read in full and no budget saw them.
#[test]
fn eighty_pages_with_a_relationships_part_each_are_charged_to_the_xml_budget() {
    let part = |n: usize| format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></PageContents>", shape(n as u32, ""));
    let rels: String = (0..29_000).map(|j| format!("<Relationship Id=\"r{j}\" Type=\"urn:filler\" Target=\"x.bin\"/>")).collect();
    let rels_xml = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>"
    );
    let mut pkg = Pkg::new();
    let mut rows = String::new();
    for k in 1..=80usize {
        rows.push_str(&format!("<Page ID=\"{k}\" Name=\"P{k}\"><Rel r:id=\"rId{k}\"/></Page>"));
        pkg = pkg
            .part(&format!("/visio/pages/page{k}.xml"), CT_PAGE, part(k))
            .part(&format!("/visio/pages/_rels/page{k}.xml.rels"), "application/vnd.openxmlformats-package.relationships+xml", rels_xml.clone())
            .rel("/visio/pages/pages.xml", &format!("rId{k}"), &format!("{VIS}page"), &format!("page{k}.xml"));
    }
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Pages>");
    pkg = pkg.part("/visio/pages/pages.xml", CT_PAGES, pages).rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml");
    let bytes = VsdxBuilder::new().without_pages_part().with(pkg).build();
    // A budget of 20 MB of XML is spent by about the eleventh page's relationships (each is about 1.9 MB): refused there, and the report says where.
    let limits = Limits { max_xml_bytes: 20_000_000, ..Limits::default() };
    let started = Instant::now();
    let (result, report) = diacraft_vsdx::read_partial(&bytes, &limits);
    let took = started.elapsed();
    eprintln!("[timing] 80 pages of 29,000 relationships each, a budget of 20 MB of XML: refused in {took:?} ({} bytes)", bytes.len());
    assert_eq!(result.unwrap_err().class(), "too_much_xml");
    let at = report.refused_at.unwrap();
    assert!(at.part.contains("_rels"), "the part it was reading: {}", at.part);
    // Each relationships part is about 1.9 MB, so a budget of 20 MB is spent by the eleventh.
    assert!(report.stats.pages <= 12, "the pages read: {}", report.stats.pages);
    assert!(took < BOUND, "{took:?}");
    // And the rows are charged to the items.
    let limits = Limits { max_items: 100_000, ..Limits::default() };
    assert_eq!(read_err(&bytes, &limits).class(), "too_many_items");
}

/// The name of the environment variable that tells the test binary, run as a child, which scenario to measure.
const SCENARIO: &str = "DIACRAFT_VSDX_TIMING_SCENARIO";

/// Runs `scenario` in a child of this test binary, so that its peak is not the peak of the other tests beside it, and gives what it printed.
fn run_child(scenario: &str) -> String {
    let exe = std::env::current_exe().expect("the path of this test binary");
    // Under `ulimit -v 3000000`, as every case that claims a memory bound is: a read that runs out of memory aborts, and the child does not end well.
    let out = std::process::Command::new("sh")
        .args(["-c", "ulimit -v 3000000 && exec \"$0\" --exact timing::child --ignored --nocapture --test-threads=1"])
        .arg(exe)
        .env(SCENARIO, scenario)
        .output()
        .expect("run the test binary");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the child failed: {stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    stdout
}

fn number_after(stdout: &str, key: &str) -> u64 {
    // The first line a test prints is on the line of its own name (`test timing::child ... ADDED 1`).
    stdout
        .lines()
        .find_map(|l| l.find(key).map(|at| &l[at + key.len()..]))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("no {key} in {stdout}"))
}

/// The scenarios the parent measures. `ignored`, so that a plain run of the suite does not run it as a test of its own.
#[test]
#[ignore = "run as a child of the tests that measure a peak"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO) else { return };
    let bytes = match scenario.as_str() {
        "million_unknown_elements" => VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &"<z/>".repeat(1_000_000)))).build(),
        "million_empty_cells" => {
            let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\"/>")).collect();
            let shapes: String = (0..1_000).map(|i| shape(i, &cells)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        "million_empty_rows" => {
            // 1,000 shapes, each with a `User` section of 1,000 rows that hold nothing: a row and its name, two items each.
            let rows: String = (0..1_000).map(|i| format!("<Row N=\"r{i}\"/>")).collect();
            let section = format!("<Section N=\"User\">{rows}</Section>");
            let shapes: String = (0..1_000).map(|i| shape(i, &section)).collect();
            VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build()
        }
        other => panic!("no scenario {other}"),
    };
    reset_peak();
    let before = status_kib("VmRSS:").unwrap_or(0);
    let started = Instant::now();
    let (_, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    let took = started.elapsed();
    let peak = peak_kib().unwrap_or(0);
    println!("ADDED {}", peak.saturating_sub(before));
    println!("ITEMS {}", report.stats.items);
    println!("CELLS {}", report.stats.cells);
    println!("MILLIS {}", took.as_millis());
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_million_unknown_elements_in_a_shape_cost_a_bounded_number_of_bytes_each() {
    // Measured in a process of its own: 191 bytes for each item in the test profile, with the tree `opc` builds and the model made from it alive
    // together. The bound is three times that.
    let out = run_child("million_unknown_elements");
    let (added, items) = (number_after(&out, "ADDED"), number_after(&out, "ITEMS"));
    let per_item = added * 1024 / items;
    eprintln!(
        "[timing] {per_item} bytes added for each of {items} items (1,000,000 unknown elements), {added} KiB in all, in {} ms",
        number_after(&out, "MILLIS")
    );
    assert!(items >= 1_000_000);
    assert!(per_item < BYTES_PER_ITEM, "{per_item} bytes for each item");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_million_cells_with_no_value_cost_a_bounded_number_of_bytes_each() {
    // The review: 122 MB of empty cells added 2.33 GB. A cell with a name only costs a map entry and the items for it.
    let out = run_child("million_empty_cells");
    let (added, cells) = (number_after(&out, "ADDED"), number_after(&out, "CELLS"));
    let per_cell = added * 1024 / cells;
    eprintln!("[timing] {per_cell} bytes added for each of {cells} cells, {added} KiB in all, in {} ms", number_after(&out, "MILLIS"));
    assert!(cells >= 1_000_000);
    assert!(per_cell < 1_200, "{per_cell} bytes for each cell");
}

/// What a row that holds nothing costs, for the figure in `limits.rs`: a million of them in `User` sections, in a process of its own under `ulimit -v 3000000`. The
/// figure is the peak the read added, with the tree `opc` builds alive beside the model: 562 bytes a row (281 an item) on beast, 2026-11-19, release before
/// (560 and 280 in the test profile), where an empty cell is 296 an item.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_million_rows_with_no_cell_cost_a_bounded_number_of_bytes_each() {
    let out = run_child("million_empty_rows");
    let (added, items) = (number_after(&out, "ADDED"), number_after(&out, "ITEMS"));
    let per_row = added * 1024 / 1_000_000;
    eprintln!(
        "[timing] {per_row} bytes added for each of 1,000,000 empty rows ({items} items), {added} KiB in all, in {} ms",
        number_after(&out, "MILLIS")
    );
    assert!(items >= 2_000_000);
    assert!(per_row < 1_200, "{per_row} bytes for each row");
}
