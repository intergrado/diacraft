//! The crafted inputs of (the plan's done criteria): the cost of holding the parts, relationships and slots of a package, on the files that make it
//! dear, counted in the units its loops walk and, behind `#[ignore]`, timed beside a yardstick.
//!
//! **What is counted** (`diacraft_vsdx::work`, which the crate's own tests turn on): the parts looked at once to decide which are held, the relationship rows looked at to hold
//! them or to find a part's role, the bytes copied into held parts, the relationships parts parsed, and the rows of the content types looked at. A count does not move with the
//! load on the machine; each bound below is the measured figure, and a loop that walked all the parts for each part, or all the rows for each row, is far past it.
//!
//! **The yardstick** of the timed cases is `Package::read` of the same bytes: the cost of opening the package, which a reader that holds nothing pays too. A case that
//! costs more than a few times that is a hole.

use std::time::Instant;

use diacraft_opc::Package;
use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, ZipItem, shape_with};
use diacraft_vsdx::{Limits, read_partial, work};

use crate::read_ok;

const BIN: &str = "application/octet-stream";

/// The package of a drawing and `n` parts `/p/n<i>.bin` of no bytes, typed by a `Default` row, and, when `chain`, a relationships part for each that names the next.
fn many_parts(n: usize, chain: bool) -> Vec<u8> {
    let mut pkg = Pkg::new().default_type("bin", BIN);
    for i in 0..n {
        pkg = pkg.part_without_override(&format!("/p/n{i}.bin"), []);
        if chain && i + 1 < n {
            let rels = format!(
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"urn:next\" Target=\"n{}.bin\"/></Relationships>",
                i + 1
            );
            pkg = pkg.item(ZipItem::deflated(&format!("p/_rels/n{i}.bin.rels"), rels.as_bytes()));
        }
    }
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build()
}

/// The counts of one read, from a fresh start.
struct Counts {
    parts: u64,
    rows: u64,
    copied: u64,
    parsed: u64,
    types: u64,
}

fn counted(bytes: &[u8]) -> (diacraft_doc::Document, diacraft_vsdx::ReadReport, Counts) {
    work::reset();
    let (doc, report) = read_ok(bytes);
    let counts = Counts {
        parts: work::held_parts_walked(),
        rows: work::held_rows_walked(),
        copied: work::held_bytes_copied(),
        parsed: work::rels_parsed(),
        types: work::type_rows_walked(),
    };
    (doc, report, counts)
}

#[test]
fn fifty_thousand_parts_of_no_bytes_are_looked_at_once_each_and_copy_nothing() {
    let (doc, report, c) = counted(&many_parts(48_000, false));
    assert_eq!(doc.parts.len(), 48_000);
    assert_eq!((report.held_parts, report.held_bytes), (48_000, 0));
    // One look at each part of the package, rels parts included, and not one more; no relationships part for the held parts to parse beyond the drawing's own.
    assert_eq!(c.parts, report.stats.parts as u64);
    assert_eq!(c.copied, 0);
    assert!(c.parsed <= 8, "{} relationships parts parsed", c.parsed);
    assert!(c.rows <= 40, "{} rows", c.rows);
    // The rows of the content types are walked once, and the part's type is a `Default` row, so no row is held for them.
    assert_eq!(c.types, report.stats.content_type_rows as u64);
    assert!(doc.vsdx.as_ref().unwrap().content_types.is_empty());
    // What each part is charged is its own: the part, its name, its type and its place, under a thousand bytes.
    assert!(report.stats.memory_bytes < 48_000 * 700 + 100_000, "{}", report.stats.memory_bytes);
}

#[test]
fn twenty_thousand_parts_that_each_name_the_next_parse_each_relationships_part_once_and_walk_each_row_once() {
    let (_, _, base) = counted(&many_parts(0, false));
    let (doc, report, c) = counted(&many_parts(20_000, true));
    assert_eq!(doc.parts.len(), 20_000);
    // Each chain of relationships is a part parsed once and a row walked once: the figures are the drawing's own and 20,000 more of each.
    assert_eq!(c.parsed - base.parsed, 19_999, "relationships parts parsed");
    assert_eq!(c.rows - base.rows, 19_999, "rows walked");
    assert_eq!(c.parts - base.parts, 20_000 + 19_999, "parts looked at: each part and each relationships part");
    assert_eq!(report.held_bytes, 0);
    assert_eq!(doc.parts.iter().filter(|p| p.rels.len() == 1).count(), 19_999);
    // A held part's relationships are its own and are not in the document's list.
    let from_parts = doc.vsdx.as_ref().unwrap().rels.iter().filter(|r| r.source.starts_with("/p/")).count();
    assert_eq!(from_parts, 0);
}

/// A drawing and one held part `/p/hub.bin` whose relationships part has `rows` rows.
fn hub(rows: usize) -> Vec<u8> {
    let body: String = (0..rows).map(|i| format!("<Relationship Id=\"r{i}\" Type=\"urn:t\" Target=\"t\"/>")).collect();
    let rels = format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{body}</Relationships>");
    let pkg =
        Pkg::new().default_type("bin", BIN).part_without_override("/p/hub.bin", []).item(ZipItem::deflated("p/_rels/hub.bin.rels", rels.as_bytes()));
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build()
}

#[test]
fn a_relationships_part_of_eighty_thousand_rows_is_walked_once_and_a_million_are_refused_in_words() {
    let (_, _, base) = counted(&hub(0));
    let (doc, _, c) = counted(&hub(80_000));
    assert_eq!(c.rows - base.rows, 80_000);
    assert_eq!(c.parsed - base.parsed, 0, "the part is parsed once, as it is for no rows");
    assert_eq!(doc.parts.get(0).unwrap().rels.len(), 80_000);
    // A million rows are far past the size a relationships part may have, and past the rows it may hold: refused, in words, without a part held.
    let (result, _) = read_partial(&hub(1_000_000), &Limits::default());
    let error = result.expect_err("a million relationships are refused");
    assert!(matches!(error.class(), "too_many_relationships" | "xml_too_complex" | "part_too_large"), "{} ({error})", error.class());
    assert!(!error.to_string().is_empty());
}

#[test]
fn rows_of_the_content_types_no_part_needs_are_held_and_each_row_is_walked_once() {
    // 20,000 `Override` rows for parts that are not there, and 20,000 `Default` rows for extensions no part has: the largest the stream can be under its 4 MiB.
    let mut pkg = Pkg::new();
    for i in 0..20_000 {
        pkg = pkg.override_without_part(&format!("/g/{i}.xml"), "application/x-gone").default_type(&format!("e{i}"), "application/x-none");
    }
    let (doc, report, c) = counted(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build());
    assert_eq!(doc.vsdx.as_ref().unwrap().content_types.len(), 40_000);
    assert_eq!(c.types, report.stats.content_type_rows as u64);
    assert_eq!(c.parts, report.stats.parts as u64);
}

#[test]
fn a_picture_named_by_every_shape_is_one_copy_and_no_row_or_part_is_held() {
    // Chained cost: each shape's relationship is the one before's, named again. The row is the picture's however many name it, the picture is copied once, and nothing is held.
    let one = |k: u32| shape_with(k, " Type=\"Foreign\"", "<ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId1\"/></ForeignData>");
    let shapes: String = (1..=5_000u32).map(one).collect();
    let page = PageSpec::new(1, "P").shapes(&shapes).rel("rId1", &format!("{OD}image"), "../media/i.png");
    let bytes = VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/i.png", "image/png", vec![7u8; 100_000])).build();
    let (doc, report, c) = counted(&bytes);
    assert_eq!((doc.parts.len(), report.held_parts), (0, 0));
    assert_eq!(report.stats.media_bytes, 100_000, "one copy of the picture");
    assert_eq!(c.copied, 0);
    assert!(doc.vsdx.as_ref().unwrap().rels.iter().all(|r| &*r.source != "/visio/pages/page1.xml"), "the picture's row is the picture's");
}

#[test]
fn groups_nested_two_elements_a_level_to_the_depth_of_the_xml_hold_what_each_level_holds_in_time_linear_in_the_levels() {
    // Chained cost: each group's `Shapes` has an attribute and a comment, held on the group, all the way down. The items and the bytes charged grow with the levels.
    let nest = |depth: usize| {
        let open: String = (0..depth).map(|d| format!("<Shape ID=\"{}\" Type=\"Group\"><Shapes foo=\"x\"><!--c-->", d + 1)).collect();
        let close = "</Shapes></Shape>".repeat(depth);
        VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("{open}{close}"))).build()
    };
    let (_, half) = read_ok(&nest(40));
    let (doc, full) = read_ok(&nest(80));
    // Twice the levels is about twice the items and the bytes (the base of the file is the same in both).
    let base = read_ok(&nest(1)).1;
    let (items_half, items_full) = (half.stats.items - base.stats.items, full.stats.items - base.stats.items);
    assert!(items_full >= items_half * 2 - 8 && items_full <= items_half * 2 + 8, "{items_half} {items_full}");
    let (bytes_half, bytes_full) = (half.stats.memory_bytes - base.stats.memory_bytes, full.stats.memory_bytes - base.stats.memory_bytes);
    assert!(bytes_full * 10 <= bytes_half * 21 && bytes_full * 10 >= bytes_half * 19, "{bytes_half} {bytes_full}");
    // The deepest group holds its own.
    let mut shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    let mut levels = 0;
    while let Some(next) = shape.children.get(0).cloned() {
        assert_eq!(crate::held::rest(&shape.ext.get().expect("the group's slot").vsdx, "Shapes").len(), 1);
        shape = next;
        levels += 1;
    }
    assert_eq!(levels, 79);
}

#[test]
fn one_part_at_the_cap_of_a_part_is_copied_once() {
    // 256 MiB of zeros in a part the drawing does not read: the one copy there is, charged as it is.
    let big: u64 = 256 * 1024 * 1024;
    let pkg = Pkg::new().default_type("bin", BIN).item(ZipItem::zeros_under_the_ratio_cap("visio/embeddings/big.bin", big));
    let (doc, report, c) = counted(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build());
    assert_eq!(doc.parts.len(), 1);
    assert_eq!((report.held_parts, report.held_bytes, c.copied), (1, big, big));
    assert_eq!(doc.parts.get(0).unwrap().bytes.len() as u64, big);
}

// -------------------------------------------------------------------------------------------------------------------- timed beside the yardstick

/// Times `read_partial` of `bytes` against `Package::read` of the same bytes, back to back, three times each, and returns the least of each, in microseconds.
fn timed(bytes: &[u8], also: &dyn Fn(&Package)) -> (u128, u128) {
    let (mut read, mut open) = (u128::MAX, u128::MAX);
    for _ in 0..3 {
        let started = Instant::now();
        if let Ok(package) = Package::read(bytes, &Limits::default().opc) {
            also(&package);
        }
        open = open.min(started.elapsed().as_micros());
        let started = Instant::now();
        let _ = read_partial(bytes, &Limits::default());
        read = read.min(started.elapsed().as_micros());
    }
    (read, open)
}

/// The most times the cost of opening the package a read that holds what it has may take, and the least it is held to (the drawing's own read costs a few milliseconds).
const TIMES: u128 = 12;
const FLOOR_US: u128 = 200_000;

fn assert_beside_the_yardstick(name: &str, bytes: &[u8]) {
    assert_beside_the_yardstick_of(name, bytes, &|_| {});
}

/// As [`assert_beside_the_yardstick`], the yardstick also doing what a read of the package cannot do without: parsing the relationships of the part it names.
fn assert_beside_the_yardstick_of(name: &str, bytes: &[u8], also: &dyn Fn(&Package)) {
    let (read, open) = timed(bytes, also);
    eprintln!("[timing] {name}: read {read} us, opening the package {open} us");
    assert!(read <= open * TIMES + FLOOR_US, "{name}: the read took {read} us, opening the package {open} us");
}

#[test]
#[ignore = "timed against the cost of opening the package"]
fn timed_fifty_thousand_parts_of_no_bytes() {
    assert_beside_the_yardstick("48,000 parts", &many_parts(48_000, false));
}

#[test]
#[ignore = "timed against the cost of opening the package"]
fn timed_twenty_thousand_parts_that_name_the_next() {
    assert_beside_the_yardstick("20,000 parts with relationships", &many_parts(20_000, true));
}

#[test]
#[ignore = "timed against the cost of opening the package"]
fn timed_a_relationships_part_of_eighty_thousand_rows() {
    assert_beside_the_yardstick_of("80,000 rows", &hub(80_000), &|package| {
        let _ = package.rels("/p/hub.bin");
    });
}

#[test]
#[ignore = "timed against the cost of opening the package"]
fn timed_one_part_at_the_cap_of_a_part() {
    let big: u64 = 256 * 1024 * 1024;
    let pkg = Pkg::new().default_type("bin", BIN).item(ZipItem::zeros_under_the_ratio_cap("visio/embeddings/big.bin", big));
    assert_beside_the_yardstick("one part of 256 MiB", &VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build());
}

// ------------------------------------------------------------------------------------- just under what a small part may be, and just over

fn windows_of(bytes: usize) -> Vec<u8> {
    let head = "<Windows xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\">"; // brand-ok: a namespace the format defines
    let tail = "</Windows>";
    let rows = (bytes - head.len() - tail.len()) / 4;
    let text = format!("{head}{}{tail}", "<w/>".repeat(rows));
    let pkg = Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", text).rel(
        // brand-ok: a content type the format defines
        "/visio/document.xml",
        "rId30",
        "http://schemas.microsoft.com/visio/2010/relationships/windows", // brand-ok: a URI the format defines
        "windows.xml",
    );
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build()
}

#[test]
fn a_windows_part_just_under_what_a_small_part_may_be_is_held_whole_and_one_just_over_is_held_as_bytes() {
    let under = diacraft_vsdx::MAX_SMALL_PART_BYTES as usize;
    let (doc, report, c) = counted(&windows_of(under));
    assert!(doc.vsdx.as_ref().unwrap().windows.is_some(), "a part of the limit is held as a tree");
    assert_eq!(doc.parts.len(), 0);
    assert!(report.stats.items >= (under / 4) - 10, "its elements are charged as items: {}", report.stats.items);
    assert_eq!(c.copied, 0);
    // One element more is past what a small part may be: it is a part held as the bytes it is, and the tree is not made.
    let (doc, report, c) = counted(&windows_of(under + 8));
    assert!(doc.vsdx.as_ref().unwrap().windows.is_none());
    assert_eq!(doc.parts.len(), 1);
    assert!(report.held_bytes > under as u64 && report.held_bytes == c.copied, "{} {}", report.held_bytes, c.copied);
}

#[test]
fn twenty_thousand_custom_properties_are_a_list_charged_by_the_property() {
    let props: String =
        (0..20_000).map(|i| format!("<property fmtid=\"{{F}}\" pid=\"{}\" name=\"n{i}\"><vt:lpwstr>v{i}</vt:lpwstr></property>", i + 2)).collect();
    let custom = format!(
        "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">{props}</Properties>"
    );
    let pkg = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    let (doc, report, c) = counted(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(pkg).build());
    assert_eq!(doc.props.custom.len(), 20_000);
    assert_eq!(doc.parts.len(), 0);
    assert_eq!(c.copied, 0);
    // Each property is its own: the property, its three strings and its two kept attributes, a few hundred bytes.
    assert!(report.stats.memory_bytes < 20_000 * 900 + 100_000, "{}", report.stats.memory_bytes);
}
