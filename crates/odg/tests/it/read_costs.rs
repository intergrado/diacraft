//! The reader's costs that found unbounded or uncounted, each held to a count of the units its loop walks
//! or to the exact items a rule charges, never to a time: pictures named many times, layers and master pages looked up by name, text past
//! its cap, concave regular polygons, dashes of many marks, rows of text, nested spans, and the charges themselves.

use std::mem::size_of;
use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellName, Row};
use diacraft_odg::{LimitKind, Limits, OdgError, Package, ReadReport, read, read_partial};

use crate::drawing::{Drawing, as_text, cell, only, page, shape, text};

/// The items a row costs, from the sizes of the model's types (the reader's rule, `read/mod.rs`): the row behind an `Arc`, and the node of
/// eleven cells a map allocates for its first cell, at 64 bytes an item.
fn row_items() -> u64 {
    ((16 + size_of::<Row>() + 16 + 11 * (size_of::<CellName>() + size_of::<Cell>())) as u64).div_ceil(64)
}

/// What the allocators give an allocation of `n` bytes by the law of the memory tests: the bytes, an eighth of them and 24.
pub(crate) fn held(n: usize) -> u64 {
    (n + n / 8 + 24) as u64
}

/// What an entry of `size` bytes holds in a list that grows by doubling, at its emptiest, with the eighth of the law.
pub(crate) fn slot(size: usize) -> u64 {
    (2 * size + 2 * size / 8) as u64
}

/// The items that pay for `need` bytes a quarter above (the rule the reader's prices of are set by), at 64 bytes an item.
pub(crate) fn priced(need: u64) -> u64 {
    (need + need / 4).div_ceil(64)
}

/// The items a page costs beside its cells, its attributes and its extension slot, from the sizes of the model's types: the page and its sheet behind
/// their `Arc`s, its two names and the names of its master page and layout, its place in the document's list of pages and in the index of the read.
pub(crate) fn page_items() -> u64 {
    priced(
        held(16 + size_of::<diacraft_doc::Page>())
            + held(16 + size_of::<diacraft_doc::Sheet>())
            + 4 * held(16)
            + slot(size_of::<Arc<diacraft_doc::Page>>())
            + slot(size_of::<diacraft_odg::PageShapes>()),
    )
}

/// A PNG's signature and then zeros.
fn png(len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    out[..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
    out
}

#[test]
fn a_picture_part_named_by_ten_thousand_frames_is_one_copy_charged_once() {
    // The review's part of 256 MiB, named by 10,000 frames. It was copied for each frame before its size was checked.
    const PART: usize = 256 * 1024 * 1024;
    let frame = r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/big.png"/></draw:frame>"#;
    let mut files = Drawing::a4().page(&frame.repeat(10_000)).files();
    files.push(("Pictures/big.png".to_owned(), png(PART)));
    let bytes = crate::build::package(crate::build::DRAWING, files);
    let (doc, report) = read(&bytes, &Limits::default()).expect("reads");
    drop(bytes);
    // The media budget is charged once, for the part's bytes.
    assert_eq!(report.work.media_bytes, PART as u64);
    assert_eq!(report.kept("picture_over_budget"), 0);
    // Every frame holds the one picture, and the picture holds the package's bytes, not a copy of them for each frame.
    let first = shape(&doc, 0, 0).foreign.as_ref().and_then(|f| f.picture.clone()).expect("a picture");
    assert_eq!(first.bytes.len(), PART);
    for at in 1..10_000 {
        let other = shape(&doc, 0, at).foreign.as_ref().and_then(|f| f.picture.clone()).expect("a picture");
        assert!(Arc::ptr_eq(&first, &other), "frame {at}");
    }
}

/// A drawing whose document layer set holds `layers` layers, and whose page holds `shapes` rectangles on a layer it does not have.
fn layered(layers: usize, shapes: usize) -> Vec<u8> {
    let mut d = Drawing::a4();
    let set: String = (0..layers).map(|k| format!(r#"<draw:layer draw:name="L{k}"/>"#)).collect();
    d.masters = format!(r#"<draw:layer-set>{set}</draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#);
    let rect = r#"<draw:rect draw:layer="missing" svg:width="1cm" svg:height="1cm"/>"#;
    d.page(&rect.repeat(shapes)).bytes()
}

#[test]
fn a_shape_s_layer_is_one_lookup_whatever_the_number_of_layers() {
    // The review's 300,000 layers and 20,000 rectangles naming a layer the drawing lacks; each shape scanned every layer.
    let limits = Limits { max_items: 100_000_000, ..Limits::default() };
    let lookups = |layers: usize| {
        let (doc, report) = read(&layered(layers, 20_000), &limits).expect("reads");
        assert!(only(&doc).sheet.cell("LayerMember").is_none());
        report.work.name_lookups
    };
    // One for each shape, and one for the page's master page.
    assert_eq!(lookups(300_000), 20_001);
    assert_eq!(lookups(3_000), 20_001);
}

#[test]
fn more_layers_than_a_section_holds_rows_are_refused_before_any_is_built() {
    let limits = Limits { max_items: 100_000_000, ..Limits::default() };
    let (result, report) = read_partial(&layered(diacraft_doc::MAX_ROWS_PER_SECTION + 1, 1), &limits);
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    // Refused before a row was built: the items are the layer set's, two or three a layer, and not a row's.
    // (Each layer is held whole besides: eleven items a layer, not three.)
    assert!(report.work.items < 11 * (diacraft_doc::MAX_ROWS_PER_SECTION as u64 + 1) + 1_000, "{}", report.work.items);
    let (result, _) = read_partial(&layered(diacraft_doc::MAX_ROWS_PER_SECTION, 1), &limits);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn the_layers_rows_are_charged_as_rows_with_their_cells_and_names() {
    // Each layer row: a row, five cells at four items, and its name three times in bytes; each layer read, one item and its name.
    let items = |layers: usize| read(&layered(layers, 0), &Limits::default()).expect("reads").1.work.items;
    let names: u64 = (0..1_000u64).map(|k| format!("L{k}").len() as u64).sum();
    let delta = items(1_000) - items(0);
    let per_row = row_items() + 5 * 4;
    // The bytes are charged a name at a time, each rounded up: 3 names of a row in one charge, and the layer's one name in another.
    let rounded: u64 = (0..1_000u64).map(|k| (3 * format!("L{k}").len() as u64).div_ceil(64) + (format!("L{k}").len() as u64).div_ceil(64)).sum();
    assert!(names > 0);
    // Each layer is also held whole in the document's layer set: its record (2 items), the element (3), its attribute (2) and its bytes (one item for a name this short).
    assert_eq!(delta, 1_000 * (per_row + 1 + 2 + 3 + 2 + 1) + rounded);
}

#[test]
fn master_pages_are_one_lookup_a_page_and_charged() {
    // The review's 2,000,000 master pages and 1,000 pages took 14 s and 1.67 GB. The reader's tree cap refuses them now.
    let masters = |n: usize| -> String {
        let list: String = (0..n).map(|k| format!(r#"<style:master-page style:name="m{k}" style:page-layout-name="PM1"/>"#)).collect();
        format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>{list}"#)
    };
    let pages = |masters_xml: String, n: usize| {
        let mut d = Drawing::a4();
        d.masters = masters_xml;
        d.pages = (0..n).map(|k| format!(r#"<draw:page draw:name="p{k}" draw:master-page-name="m{}"/>"#, k * 7)).collect();
        d.bytes()
    };
    let (result, _) = read_partial(&pages(masters(2_000_000), 1_000), &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    // Under the cap, each page's master page is one lookup, and each master page is charged.
    let (doc, report) = read(&pages(masters(200_000), 1_000), &Limits::default()).expect("reads");
    assert_eq!(doc.pages.len(), 1_000);
    assert_eq!(report.work.name_lookups, 1_000);
    assert_eq!(report.warned("master_page_not_found"), 0);
    let (_, small) = read(&pages(masters(20_000), 1_000), &Limits::default()).expect("reads");
    // Each master page: one item, and one for the bytes of its names (each name twice and its layout's twice, under 64 bytes).
    // Each master page is also held for the pages on it: two items for each of its two attributes, and the bytes of the names and attributes (two items more).
    // And each, holding no shape, is kept whole: its record (2 items), its element (3, and 2 for each attribute), the element's bytes as text (`style:master-page`, its two
    // attributes' names and values: 58 or 59 bytes, two items) and the bytes of its family word and name (one item): 12 more.
    assert_eq!(report.work.items - small.work.items, 180_000 * (7 + 12));
    // A display name is held once more: 100 bytes beside the name (30, twice) and the layout's (3, twice).
    let one = |display: &str| {
        let mut d = Drawing::a4();
        d.masters = format!(r#"<style:master-page style:name="{}"{display} style:page-layout-name="PM1"/>"#, "N".repeat(30));
        d.page("").read().1.work.items
    };
    let shown = format!(r#" style:display-name="{}""#, "D".repeat(100));
    // The attribute is also held in the master page's own attributes for the pages on it: its pair (two items), and the bytes the master page holds are
    // charged in one sum, as text (nine eighths): its name three times, the display name, the layout's name three times, and every attribute's name and value.
    let held = |display: u64| 30 * 3 + display + 3 * 3 + (10 + 30) + (22 + 3) + if display > 0 { 18 + display } else { 0 };
    // The master page kept whole holds the attribute once more: its pair (two items) and its bytes in the element's (`style:master-page` and its attributes' names and values).
    let whole = |display: u64| 17 + (10 + 30) + (22 + 3) + if display > 0 { 18 + display } else { 0 };
    assert_eq!(
        one(&shown) - one(""),
        as_text(held(100)).div_ceil(64) - as_text(held(0)).div_ceil(64) + 2 + 2 + as_text(whole(100)).div_ceil(64) - as_text(whole(0)).div_ceil(64)
    );
}

#[test]
fn text_past_the_document_s_cap_is_refused_before_it_is_made() {
    // The review's 19,000 spaces of 65,536 made 1.2 GB of text from a 2,477-byte file.
    let spaces = |n: usize| {
        let p = format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(n));
        read_partial(&Drawing::a4().page(&p).bytes(), &Limits::default())
    };
    let (result, report) = spaces(19_000);
    assert_eq!(result.err().map(|e| e.class()), Some("too_much_text"));
    assert_eq!(report.work.text_bytes, diacraft_odg::limits::MAX_TEXT_BYTES as u64, "stopped at the cap");
    let (result, report) = spaces(1_024);
    assert!(result.is_ok());
    assert_eq!(report.work.text_bytes, 1_024 * 65_536);
    // The count is clamped to 65,536 before anything is made.
    let p = r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p><text:s text:c="1000000"/>x</text:p></draw:rect>"#;
    let (_, report) = read(&Drawing::a4().page(p).bytes(), &Limits::default()).expect("reads");
    assert_eq!(report.work.text_bytes, 65_536 + 1);
}

#[test]
fn the_reader_s_trees_have_an_item_cap_of_their_own() {
    let d = Drawing::a4().page(&r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#.repeat(100));
    let limits = Limits { max_read_xml_nodes: 200, ..Limits::default() };
    assert_eq!(read(&d.bytes(), &limits).err().map(|e| e.class()), Some("xml_too_complex"));
    // The package layer keeps the container's cap.
    assert!(Package::read(&d.bytes(), &limits).is_ok());
    assert!(read(&d.bytes(), &Limits { max_read_xml_nodes: 1_000, ..Limits::default() }).is_ok());
}

#[test]
fn a_concave_polygon_takes_twice_its_corners_and_its_closing_row_from_the_page_s_budget() {
    // The corners were checked and charged, not the points, which a concave polygon has twice as many of.
    let polygon = |corners: u32, concave: bool| {
        format!(r#"<draw:regular-polygon svg:width="1cm" svg:height="1cm" draw:corners="{corners}" draw:concave="{concave}" draw:sharpness="50%"/>"#)
    };
    let units = |shapes: &str| read(&Drawing::a4().page(shapes).bytes(), &Limits::default()).expect("reads").1.work.path_units;
    assert_eq!(units(&polygon(1_000, true)), 2_001);
    assert_eq!(units(&polygon(1_000, false)), 1_001);
    // A page's budget of 10,000: two concave polygons of 2,499 corners take 5,000 each, and a third is refused.
    let limits = Limits { max_path_commands_per_page: 10_000, ..Limits::default() };
    assert!(read(&Drawing::a4().page(&polygon(2_499, true).repeat(2)).bytes(), &limits).is_ok());
    assert_eq!(read(&Drawing::a4().page(&polygon(2_499, true).repeat(3)).bytes(), &limits).err().map(|e| e.class()), Some("xml_too_complex"));
    // More points than one polygon may have: drawn as its frame, with a warning, and nothing taken.
    let (doc, report) = read(&Drawing::a4().page(&polygon(500_001, true)).bytes(), &Limits::default()).expect("reads");
    assert_eq!(report.warned("geometry_unreadable"), 1);
    assert_eq!(report.work.path_units, 0);
    assert_eq!(crate::drawing::rows(only(&doc), 0), 5);
}

#[test]
fn a_dash_of_more_marks_than_a_pattern_has_makes_no_runs() {
    // A dash's mark counts are a file's numbers: one past the table's three marks is no pattern, and no run is made for it.
    let dash = |dots: &str| {
        Drawing::a4()
            .common(&format!(r#"<draw:stroke-dash draw:name="D" {dots} draw:dots1-length="300%" draw:distance="300%"/>"#))
            .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="1pt"/></style:style>"#)
            .page(r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#)
            .read()
    };
    let (doc, report) = dash(r#"draw:dots1="10000000""#);
    assert_eq!(text(cell(only(&doc), "LinePattern")), "2", "the stand-in");
    assert_eq!(report.work.path_units, 0);
    let (doc, report) = dash(r#"draw:dots1="0""#);
    assert_eq!(text(cell(only(&doc), "LinePattern")), "2", "no marks: the stand-in");
    assert_eq!(report.work.path_units, 0);
    // One mark: its two runs, a mark and a gap, walked.
    let (doc, report) = dash(r#"draw:dots1="1""#);
    assert_eq!(text(cell(only(&doc), "LinePattern")), "9");
    assert_eq!(report.work.path_units, 2);
    // Lengths in points on a line of no weight cannot be weights: no runs, and the stand-in.
    let (doc, report) = Drawing::a4()
        .common(r#"<draw:stroke-dash draw:name="D" draw:dots1="1" draw:dots1-length="3pt" draw:distance="3pt"/>"#)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="0cm"/></style:style>"#)
        .page(r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    assert_eq!(text(cell(only(&doc), "LinePattern")), "2");
    assert_eq!(report.work.path_units, 0);
}

#[test]
fn more_rows_of_text_than_a_section_holds_are_too_complex() {
    // The review's 320,001 paragraph styles, each a paragraph row of its own.
    let styles: String = (0..320_001)
        .map(|k| {
            format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:paragraph-properties fo:margin-left="{k}pt"/></style:style>"#)
        })
        .collect();
    let paragraphs: String = (0..320_001).map(|k| format!(r#"<text:p text:style-name="P{k}">x</text:p>"#)).collect();
    let d = Drawing::a4().auto(&styles).page(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{paragraphs}</draw:rect>"#));
    assert_eq!(read(&d.bytes(), &Limits::default()).err().map(|e| e.class()), Some("xml_too_complex"));
}

#[test]
fn a_span_nested_deep_costs_what_one_alone_does() {
    // The review's finding 3: each span stacked a layer on its runs' view, so a run 190 spans deep walked 190 layers for each property.
    let steps = |depth: usize, siblings: usize| {
        let styles: String = (0..depth.max(1))
            .map(|k| {
                format!(r#"<style:style style:name="T{k}" style:family="text"><style:text-properties fo:font-size="{}pt"/></style:style>"#, 8 + k % 9)
            })
            .collect();
        let open: String = (0..depth).map(|k| format!(r#"<text:span text:style-name="T{k}">"#)).collect();
        let inner = r#"<text:span text:style-name="T0">a</text:span>"#.repeat(siblings);
        let body = format!("<text:p>{open}{inner}{}</text:p>", "</text:span>".repeat(depth));
        let d = Drawing::a4().auto(&styles).page(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{body}</draw:rect>"#));
        read(&d.bytes(), &Limits::default()).expect("reads").1.work.style_steps
    };
    let (shallow, deep) = (steps(10, 2_000), steps(150, 2_000));
    // The siblings cost the same at either depth; the extra 140 levels cost each their own, a few hundred steps, and no more.
    assert!(deep < shallow + 140 * 1_000, "{shallow} at 10 deep, {deep} at 150");
    // And linear in the siblings.
    let more = steps(150, 20_000);
    assert!(more < deep * 11, "{deep} then {more}");
}

#[test]
fn a_style_s_resolution_and_a_shape_s_local_cells_are_charged() {
    // A rectangle on an automatic style with `n` filled properties of its own over a common style with none: the shape's local cells
    // grow with `n`, and so do its style's resolution and its own charge.
    let items = |props: &str| {
        Drawing::a4()
            .common(r#"<style:style style:name="C" style:family="graphic"/>"#)
            .auto(&format!(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="C"><style:graphic-properties {props}/></style:style>"#))
            .page(r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#)
            .read()
    };
    let (one_doc, one) = items(r##"draw:fill-color="#111111""##);
    let (two_doc, two) = items(r##"draw:fill-color="#111111" svg:stroke-color="#222222""##);
    assert!(cell(only(&one_doc), "LineColor").is_none());
    assert_eq!(text(cell(only(&two_doc), "LineColor")), "#222222");
    // One more property: one more in the style as read (and 7 more bytes, within the same 64), one more entry in its resolution, one
    // more local cell (four items) in the shape's style, kept once for every pair of styles, and again as the shape's cell.
    // (An automatic style that holds only what cells read is kept as nothing.)
    assert_eq!(two.work.items - one.work.items, 1 + 1 + 1 + 4, "{} then {}", one.work.items, two.work.items);
    assert_eq!(two.stats.cells - one.stats.cells, 1);
}

#[test]
fn a_field_s_text_is_charged_twice_and_a_line_break_by_its_bytes() {
    let p = r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>ab<text:page-number>12</text:page-number><text:line-break/><text:tab/></text:p></draw:rect>"#;
    let (_, report) = read(&Drawing::a4().page(p).bytes(), &Limits::default()).expect("reads");
    // "ab", the field's "12" in the text and in its cell, U+2028's three bytes, and the tab.
    assert_eq!(report.work.text_bytes, 2 + 2 * 2 + 3 + 1);
}

#[test]
fn a_package_s_xml_bytes_are_capped_at_their_sum() {
    let d = Drawing::a4().page("");
    let files = d.files();
    let size = |name: &str| files.iter().find(|(n, _)| n == name).map_or(0, |(_, b)| b.len() as u64);
    let both = size("content.xml") + size("styles.xml");
    assert!(read(&d.bytes(), &Limits { max_xml_bytes: both, ..Limits::default() }).is_ok());
    assert_eq!(read(&d.bytes(), &Limits { max_xml_bytes: both - 1, ..Limits::default() }).err().map(|e| e.class()), Some("too_much_xml"));
}

#[test]
fn a_geometry_section_is_charged_its_rows_and_its_cells() {
    // A path of `n` lines after its move: each row (and its two cells at four items) and the section, which holds `NoFill` when open.
    let items = |lines: usize| {
        let d = format!("M0 0{}", " L1 1".repeat(lines));
        let shape = format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="{d}"/>"#);
        read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    // The path's data and its view box are also held beside the cells that stand for them: `svg:d` and `svg:viewBox`, their names and values, 64 bytes to the item.
    let held = |lines: u64| (("svg:d".len() + "M0 0".len() + 5 * lines as usize + "svg:viewBox".len() + "0 0 10 10".len()) as u64).div_ceil(64);
    assert_eq!(items(20) - items(10), 10 * (row_items() + 2 * 4) + held(20) - held(10));
    // An open path's section holds `NoFill`, and its node of cells is charged; a closed one of the same rows holds none.
    let path = |d: &str| {
        let shape = format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="{d}"/>"#);
        read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    let node = ((16 + 11 * (size_of::<CellName>() + size_of::<Cell>())) as u64).div_ceil(64);
    assert_eq!(path("M0 0 L5 5 L0 0") - path("M0 0 L5 5 L0 0 Z"), node);
    let (doc, _) = read(
        &Drawing::a4().page(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="M0 0 L1 1"/>"#).bytes(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(crate::drawing::rows(only(&doc), 0), 2);
}

#[test]
fn a_page_s_attributes_are_charged_as_pairs_and_by_their_bytes() {
    let items = |extra: &str| {
        let mut d = Drawing::a4();
        d.pages = format!(r#"<draw:page draw:name="p" draw:master-page-name="Default"{extra}/>"#);
        d.read().1.work.items
    };
    // Without: `draw:name` and `p`, and `draw:master-page-name` and `Default`, 38 bytes. With: 15 and 100 more, 153.
    let style = format!(r#" draw:style-name="{}""#, "s".repeat(100));
    assert_eq!(items(&style) - items(""), 2 + (153u64.div_ceil(64) - 38u64.div_ceil(64)));
    // The name is the page's, charged by its bytes too.
    let named = |name: &str| {
        let mut d = Drawing::a4();
        d.pages = format!(r#"<draw:page draw:name="{name}" draw:master-page-name="Default"/>"#);
        d.read().1.work.items
    };
    assert_eq!(named(&"n".repeat(101)) - named("n"), 138u64.div_ceil(64) - 38u64.div_ceil(64));
}

#[test]
fn connection_rows_of_a_new_size_are_charged_as_a_section_of_four_rows() {
    // A second rectangle of the first's size shares its connection rows (one item for the share); one of another size has its own: a
    // section, four rows, and their twenty cells at four items.
    let items = |second: &str| {
        let page = format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"/><draw:rect svg:width="{second}" svg:height="1cm"/>"#);
        read(&Drawing::a4().page(&page).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    let section = ((16 + size_of::<diacraft_doc::Section>() + 8) as u64).div_ceil(64);
    assert_eq!(items("2cm") - items("1cm"), section + 4 * row_items() + 20 * 4 - 1);
}

#[test]
fn a_line_is_charged_its_shape_its_cells_and_its_six_rows() {
    // Beside four items for each cell: the shape with its sheet's node of cells, and two sections, its geometry's two rows and the four
    // connection rows.
    let read_page = |shapes: &str| read(&Drawing::a4().page(shapes).bytes(), &Limits::default()).expect("reads").1;
    let (empty, line) = (read_page(""), read_page(r#"<draw:line svg:x1="1cm" svg:y1="1cm" svg:x2="3cm" svg:y2="2cm"/>"#));
    let node = 16 + 11 * (size_of::<CellName>() + size_of::<Cell>());
    // The shape and the sheet are allocations of their own, each rounded up to whole items, and the node a third.
    let items = |bytes: usize| (bytes as u64).div_ceil(64);
    let shape = items(16 + size_of::<diacraft_doc::Shape>()) + items(16 + size_of::<diacraft_doc::Sheet>()) + items(node);
    let section = ((16 + size_of::<diacraft_doc::Section>() + 8) as u64).div_ceil(64);
    let cells = (line.stats.cells - empty.stats.cells) as u64;
    // The formulas its cells hold (`Width*0.5` and the rest) are parsed once each and charged by their nodes, two items a node (128 bytes), and
    // their text: what the charge adds to the parts is that, within the item each charge rounds up.
    let parsed = line.work.formula_nodes - empty.work.formula_nodes;
    // And its `.odg` data: the slot of the shape, the ends it was read with (a value and a list of four attributes) and its element (a value and a one-element list).
    let held = (size_of::<diacraft_doc::Held>() as u64).div_ceil(64);
    let odf = (16 + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfShape>>()).div_ceil(64) as u64 + 2 * held + (2 * 4 + 1) + (3 + 1);
    let extra = line.work.items - empty.work.items - 4 * cells - (shape + 2 * section + 6 * row_items() + odf);
    assert!(parsed > 0 && (2 * parsed..=2 * parsed + 24).contains(&extra), "{parsed} nodes, {extra} items");
}

#[test]
fn a_style_sheet_s_rows_are_charged_one_each() {
    // A style sheet's paragraph row, run row, each gradient stop and its tab stops each cost a row, whatever else it holds: the charges of
    // two parts together are the sum of each alone.
    let parts = |paragraph: bool, run: bool, stops: bool, tabs: bool| {
        let graphic = if stops { r#"<style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="G"/>"# } else { "" };
        let tab_stops = if tabs { r#"<style:tab-stops><style:tab-stop style:position="1cm"/></style:tab-stops>"# } else { "" };
        let align = if paragraph { r#" fo:text-align="center""# } else { "" };
        let size = if run { r#"<style:text-properties fo:font-size="10pt"/>"# } else { "" };
        let style = format!(
            r#"<style:style style:name="S" style:family="graphic">{graphic}<style:paragraph-properties{align}>{tab_stops}</style:paragraph-properties>{size}</style:style>"#
        );
        Drawing::a4()
            .common(r##"<draw:gradient draw:name="G" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:border="20%"/>"##)
            .common(&style)
            .page("")
            .read()
            .1
            .work
            .items as i64
    };
    let none = parts(false, false, false, false);
    let together = |a: (bool, bool, bool, bool), b: (bool, bool, bool, bool)| {
        let both = (a.0 || b.0, a.1 || b.1, a.2 || b.2, a.3 || b.3);
        parts(both.0, both.1, both.2, both.3) - parts(a.0, a.1, a.2, a.3) - parts(b.0, b.1, b.2, b.3) + none
    };
    let (p, c, s, t) = ((true, false, false, false), (false, true, false, false), (false, false, true, false), (false, false, false, true));
    assert_eq!(together(p, c), 0);
    assert_eq!(together(p, s), 0);
    assert_eq!(together(c, t), 0);
    assert_eq!(together(s, t), 0);
    // And one part alone costs its row: the run's size is a row and a cell.
    assert_eq!(parts(false, true, false, false) - none, row_items() as i64 + 4 + 1 + 1);
}

#[test]
fn a_style_sheet_s_names_are_charged_by_their_bytes() {
    // A display name of 100 bytes beside the name `S`: held by the style as read, and again by its style sheet.
    let items = |display: &str| {
        Drawing::a4().common(&format!(r#"<style:style style:name="S"{display} style:family="graphic"/>"#)).page("").read().1.work.items
    };
    let shown = format!(r#" style:display-name="{}""#, "D".repeat(100));
    assert_eq!(items(&shown) - items(""), 2 * (101u64.div_ceil(64) - 1u64.div_ceil(64)));
}

#[test]
fn a_page_is_charged_one_item_with_its_attributes_and_cells() {
    // A second page: its cells at four items, one item for itself, a pair for its master page's name, and its 38 bytes of attributes; and the page itself, its sheet,
    // its names and its places.
    let two = |second: &str| {
        let mut d = Drawing::a4();
        d.pages = format!(r#"<draw:page draw:name="p" draw:master-page-name="Default"/>{second}"#);
        d.read().1
    };
    let (one, both) = (two(""), two(r#"<draw:page draw:name="q" draw:master-page-name="Default"/>"#));
    let cells = (both.stats.cells - one.stats.cells) as u64;
    // The page's own attributes sit in the slot of its `odf` data, which is an allocation of its own and is charged: the allocation behind its pointer, whole items.
    let slot = (16 + std::mem::size_of::<diacraft_doc::ExtData<diacraft_doc::OdfPage>>()).div_ceil(64) as u64;
    // And the master page's name as the page wrote it, a string of its own.
    assert_eq!(both.work.items - one.work.items - 4 * cells, 1 + 2 + 38u64.div_ceil(64) + slot + "Default".len().div_ceil(64) as u64 + page_items());
    // A page with no name: the same pairs, its name's bytes the less.
    let unnamed = two(r#"<draw:page draw:master-page-name="Default"/>"#);
    assert_eq!(unnamed.work.items, both.work.items - (38u64.div_ceil(64) - 28u64.div_ceil(64)));
}

/// A drawing whose font face `F` has a family of `family` characters, with `styles` common paragraph styles naming it and `rects`
/// rectangles, each on an automatic style of its own that names it, each with a line of text.
fn font_family(family: usize, styles: usize, rects: usize) -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = format!(r#"<style:font-face style:name="F" svg:font-family="{}"/>"#, "f".repeat(family));
    let common: String = (0..styles)
        .map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:text-properties style:font-name="F"/></style:style>"#))
        .collect();
    let auto: String = (0..rects)
        .map(|k| format!(r#"<style:style style:name="gr{k}" style:family="graphic"><style:text-properties style:font-name="F" fo:font-size="{}pt"/></style:style>"#, 8 + k % 7))
        .collect();
    let shapes: String = (0..rects)
        .map(|k| format!(r#"<draw:rect draw:style-name="gr{k}" svg:width="1cm" svg:height="1cm"><text:p>x</text:p></draw:rect>"#))
        .collect();
    d.common.push_str(&common);
    d.auto(&auto).page(&shapes)
}

#[test]
fn a_font_family_named_by_thousands_of_styles_is_one_value() {
    // One family of 1,000,000 characters named by 1,000 or 4,000 common styles, or by 4,000 rectangles on automatic
    // styles of their own, was copied into every conversion (0.99 GB for 1,000; an abort for 4,000). The family is now one value, charged
    // once in each part that declares it, and shared by every cell that shows it.
    let one_family = (1_000_000u64).div_ceil(64);
    for (styles, rects) in [(1_000, 0), (4_000, 0), (0, 4_000)] {
        let (doc, report) = read(&font_family(1_000_000, styles, rects).bytes(), &Limits::default()).expect("reads");
        let mut faces: Vec<*const u8> = Vec::new();
        for style in doc.styles.iter().filter(|s| s.name_u.as_deref().is_some_and(|n| n.starts_with('P'))) {
            let at = diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "Font");
            faces.push(crate::drawing::cell_at(&style.sheet, &at).and_then(|c| c.value.as_ref()).map(|v| v.text().as_ptr()).expect("a face"));
        }
        for s in page(&doc, 0).shapes.iter() {
            let at = crate::drawing::row_cell(s, "Character", None, 0, "Font");
            faces.push(at.and_then(|c| c.value.as_ref()).map(|v| v.text().as_ptr()).expect("a face"));
        }
        assert_eq!(faces.len(), styles + rects);
        assert!(faces.windows(2).all(|w| w[0] == w[1]), "one value for {styles} styles and {rects} rectangles");
        // The family's bytes are charged twice (as a font face read, then as the one value cells share), in each part that declares it,
        // and not once for each conversion.
        assert!(
            report.work.items < 4 * one_family + (styles + rects) as u64 * 400,
            "{} items for {styles} styles and {rects} rectangles",
            report.work.items
        );
    }
}

#[test]
fn style_level_strings_are_charged_where_they_are_read() {
    // Font faces, dashes, gradients, markers and page layouts were held for no item.
    let items = |fonts: &str, common: &str| {
        let mut d = Drawing::a4();
        d.fonts = fonts.to_owned();
        d.common = common.to_owned();
        d.page("").read().1.work.items
    };
    let none = items("", "");
    // A face of a 1,000-byte family, declared in both parts the drawing writes: one item and its name's and family's bytes, twice; and the model holds the declaration once (an equal
    // declaration in the other part is the same face), as the font table's face does: the face (3 items), its declaration kept whole (an element and its two
    // attributes, 3 + 2 * 2, and the bytes of the names and values, `style:font-face`, `style:name`, `svg:font-family`, 1 and 1,000), and the family it is known by.
    // The bytes are text, at nine eighths. The face is 4 items: what tells the parts apart (a byte, and its name's entry in a set) is charged with it.
    let held = |declaration_bytes: u64, family: u64| 4 + 3 + 2 * 2 + as_text(declaration_bytes).div_ceil(64) + as_text(family).div_ceil(64);
    let face = format!(r#"<style:font-face style:name="F" svg:font-family="{}"/>"#, "f".repeat(1_000));
    assert_eq!(items(&face, "") - none, 2 * (1 + as_text(1_001).div_ceil(64)) + held(15 + 10 + 15 + 1 + 1_000, 1_000));
    // The name's bytes and the family's are summed: a name of 100 and a family of 1,000.
    let named = format!(r#"<style:font-face style:name="{}" svg:font-family="{}"/>"#, "n".repeat(100), "f".repeat(1_000));
    assert_eq!(items(&named, "") - none, 2 * (1 + as_text(1_100).div_ceil(64)) + held(15 + 10 + 15 + 100 + 1_000, 1_000));
    // A dash, a gradient and a marker named with 200 bytes each: one item and the name's bytes (and for the gradient, the rows it stands in with: three for an axial one).
    let name = "n".repeat(200);
    // Each is also held as a named definition, as written: its record (2 items), its element (3 items and a pair, two items, for each attribute), the bytes of the
    // element's names and values at 64 to the item, and its name's bytes once more.
    let definition = |element: &str, attrs: &[(&str, &str)]| -> u64 {
        let bytes: usize = element.len() + attrs.iter().map(|(n, v)| n.len() + v.len()).sum::<usize>();
        2 + 3 + 2 * attrs.len() as u64 + as_text(bytes as u64).div_ceil(64) + as_text(200).div_ceil(64)
    };
    let dash = definition("draw:stroke-dash", &[("draw:name", &name), ("draw:dots1", "1"), ("draw:distance", "1%")]);
    assert_eq!(
        items("", &format!(r#"<draw:stroke-dash draw:name="{name}" draw:dots1="1" draw:distance="1%"/>"#)) - none,
        1 + as_text(200).div_ceil(64) + dash
    );
    let gradient = definition("draw:gradient", &[("draw:name", &name), ("draw:style", "axial")]);
    assert_eq!(items("", &format!(r#"<draw:gradient draw:name="{name}" draw:style="axial"/>"#)) - none, 1 + as_text(200).div_ceil(64) + 3 + gradient);
    let marker = definition("draw:marker", &[("draw:name", &name)]);
    assert_eq!(items("", &format!(r#"<draw:marker draw:name="{name}"/>"#)) - none, 1 + as_text(200).div_ceil(64) + marker);
}

#[test]
fn text_loose_in_a_shape_is_charged_by_its_bytes() {
    // Text directly under a shape, kept, was one item at any length.
    let items = |text: &str| {
        let shape = format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{text}<text:p>t</text:p></draw:rect>"#);
        read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    // As text, at nine eighths.
    assert_eq!(items(&"x".repeat(10_000)) - items("x"), as_text(10_000).div_ceil(64) - as_text(1).div_ceil(64));
}

#[test]
fn a_shape_s_name_is_charged_by_its_bytes_twice() {
    // The name and the universal name are two copies.
    let items = |name: &str| {
        let shape = format!(r#"<draw:rect draw:name="{name}" svg:width="1cm" svg:height="1cm"/>"#);
        read(&Drawing::a4().page(&shape).bytes(), &Limits::default()).expect("reads").1.work.items
    };
    // As text, at nine eighths.
    assert_eq!(items(&"n".repeat(200)) - items("n"), as_text(400).div_ceil(64) - as_text(2).div_ceil(64));
}

#[test]
fn inline_pictures_take_their_bytes_from_the_media_budget() {
    // Two inline pictures of four bytes each, under a budget of four: the first fits, and the second is past it.
    let inline =
        r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw==</office:binary-data></draw:image></draw:frame>"#;
    let (doc, report) = read(&Drawing::a4().page(&inline.repeat(2)).bytes(), &Limits { max_media_bytes: 4, ..Limits::default() }).unwrap();
    assert!(shape(&doc, 0, 0).foreign.as_ref().is_some_and(|f| f.picture.is_some()));
    assert!(shape(&doc, 0, 1).foreign.as_ref().is_some_and(|f| f.picture.is_none()));
    assert_eq!((report.work.media_bytes, report.kept("picture_over_budget")), (4, 1));
}

#[test]
fn a_style_s_text_shown_in_cells_is_charged_once_whatever_the_cells() {
    // The first style that shows the family makes its one value, one item and its 1,000 bytes; a second shows the same value, and costs
    // only what a style costs.
    let items = |styles: usize| read(&font_family(1_000, styles, 0).bytes(), &Limits::default()).expect("reads").1.work.items;
    let (first, second) = (items(1) - items(0), items(2) - items(1));
    // Its bytes as text, at nine eighths.
    assert_eq!(first - second, 1 + as_text(1_000).div_ceil(64));
}

#[test]
fn equal_texts_held_in_two_styles_are_one_value_and_one_row() {
    // Two text styles, each with its own family of 100 characters, equal: one value, so their runs share a row; one of another family has
    // a row of its own.
    let family = |c: char| c.to_string().repeat(100);
    let style = |name: &str, f: &str| {
        format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties fo:font-family="{f}"/></style:style>"#)
    };
    let (doc, _) = Drawing::a4()
        .auto(&format!("{}{}{}", style("T1", &family('a')), style("T2", &family('a')), style("T3", &family('b'))))
        .page(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>x<text:span text:style-name="T1">a</text:span>y<text:span text:style-name="T2">b</text:span>z<text:span text:style-name="T3">c</text:span></text:p></draw:rect>"#)
        .read();
    let s = only(&doc);
    let parts = &s.text.as_ref().expect("text").parts;
    let cps: Vec<u32> = parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect();
    assert_eq!(cps, [1, 0, 1, 0, 2], "T1 and T2 one row, T3 another");
    let face = |ix: u32| crate::drawing::row_cell(s, "Character", None, ix, "Font").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert_eq!(face(1), Some(family('a')));
    assert_eq!(face(2), Some(family('b')));
}

/// The bytes of the page layout's name `PM1`, which the master page of every test drawing has: looked up once, for the master page.
const LAYOUT: u64 = 3;

/// A generous bound on the wall time of a read the counts below already hold: the machine is shared, and the counts are the test.
const SLOW: Duration = Duration::from_secs(30);

/// A rectangle holding `inner`.
fn rect_with(inner: &str) -> String {
    format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{inner}</draw:rect>"#)
}

/// A list style `name` of bullet levels numbered `numbers`, each bullet `x`.
fn list_style(name: &str, numbers: impl IntoIterator<Item = u32>) -> String {
    let levels: String = numbers.into_iter().map(|k| format!(r#"<text:list-level-style-bullet text:level="{k}" text:bullet-char="x"/>"#)).collect();
    format!(r#"<text:list-style style:name="{name}">{levels}</text:list-style>"#)
}

/// Reads `d`, which must read, and says how long it took.
fn timed(d: &Drawing) -> (ReadReport, Duration) {
    let bytes = d.bytes();
    let started = Instant::now();
    let (_, report) = read(&bytes, &Limits::default()).expect("reads");
    (report, started.elapsed())
}

#[test]
fn a_long_style_name_inherited_by_nested_lists_is_hashed_once() {
    // 's HIGH: a list with a style name of 1 MB holding 100,000 empty nested lists copied and hashed the name for
    // each (14.8 s on an 18 KB file). A list that names no style reads the one of the list around it, which was found once.
    const NAME: usize = 1_000_000;
    let name = "n".repeat(NAME);
    let body =
        rect_with(&format!(r#"<text:list text:style-name="{name}"><text:list-item>{}</text:list-item></text:list>"#, "<text:list/>".repeat(100_000)));
    // The style is another's: no list finds levels, and none searches any.
    let other = list_style("S", [1]);
    let (report, took) = timed(&Drawing::a4().auto(&other).page(&body));
    assert_eq!(report.work.name_bytes, NAME as u64 + LAYOUT, "one hash of the name, whatever the lists nested in it");
    assert_eq!(report.work.list_levels, 0);
    assert!(took < SLOW, "{took:?}");
    // The style is declared, with two levels: every list searches them, and the name is still hashed once.
    let (report, took) = timed(&Drawing::a4().auto(&list_style(&name, [1, 2])).page(&body));
    assert_eq!(report.work.name_bytes, NAME as u64 + LAYOUT);
    assert_eq!(report.work.list_levels, 100_001 * 2, "a binary search of two levels visits two, for the list and each of the 100,000 in it");
    assert!(took < SLOW, "{took:?}");
}

#[test]
fn lists_nested_in_a_chain_each_inheriting_from_the_one_around_it_hash_the_name_once() {
    // Not many lists in one list, but each in the one before: 90 deep (the depth the tree allows), each naming no style.
    const NAME: usize = 1_000_000;
    let name = "n".repeat(NAME);
    let depth = 90;
    // An item holding a paragraph and, but at the innermost, a list holding the next item.
    fn nest(k: u64, depth: u64) -> String {
        let inner = if k + 1 < depth { format!("<text:list>{}</text:list>", nest(k + 1, depth)) } else { String::new() };
        format!("<text:list-item><text:p>{k}</text:p>{inner}</text:list-item>")
    }
    let body = format!(r#"<text:list text:style-name="{name}">{}</text:list>"#, nest(0, depth));
    let (report, took) = timed(&Drawing::a4().auto(&list_style(&name, 1..=3)).page(&rect_with(&body)));
    assert_eq!(report.work.name_bytes, NAME as u64 + LAYOUT);
    // Three levels are two bits: each of the 90 lists visits at most two.
    assert_eq!(report.work.list_levels, depth * 2);
    assert!(took < SLOW, "{took:?}");
    // A list in the chain that names a style of its own is hashed on its own, and the lists in it read it, not the first.
    let inner = r#"<text:list text:style-name="M"><text:list-item><text:list><text:list-item><text:p>x</text:p></text:list-item></text:list></text:list-item></text:list>"#;
    let body = format!(r#"<text:list text:style-name="L"><text:list-item>{inner}</text:list-item></text:list>"#);
    let styles = format!("{}{}", list_style("L", [1]), list_style("M", [1]));
    let (report, _) = timed(&Drawing::a4().auto(&styles).page(&rect_with(&body)));
    assert_eq!(report.work.name_bytes, 2 + LAYOUT, "L, then M; the list inside M reads M's");
    assert_eq!(report.work.list_levels, 1 + 1 + 1, "one level a style: one visit for each of the three lists");
}

#[test]
fn a_list_style_of_many_levels_is_searched_and_not_walked_for_each_list() {
    // 's HIGH: a style of 200,000 levels named by 1,000 empty lists cloned all the levels, twice, for each
    // list (4.48 s on a 530 KB file). The levels are borrowed and searched: 18 steps at most for 200,000.
    let numbers = (2..=200_000u32).chain([1]);
    let lists = r#"<text:list text:style-name="L"/>"#.repeat(1_000);
    let (report, took) = timed(&Drawing::a4().auto(&list_style("L", numbers)).page(&rect_with(&lists)));
    assert_eq!(report.work.list_levels, 1_000 * 18, "200,000 levels are 18 bits");
    assert_eq!(report.work.name_bytes, 1_000 + LAYOUT, "the name `L`, hashed for each of the lists that names it");
    assert!(took < SLOW, "{took:?}");
    // The same search for ten times the lists costs ten times the steps, and no more: it does not grow with the lists times the levels.
    let lists = r#"<text:list text:style-name="L"/>"#.repeat(10_000);
    let (report, took) = timed(&Drawing::a4().auto(&list_style("L", (2..=200_000u32).chain([1]))).page(&rect_with(&lists)));
    assert_eq!(report.work.list_levels, 10_000 * 18);
    assert!(took < SLOW, "{took:?}");
}

#[test]
fn a_face_name_held_in_one_place_is_hashed_once_however_many_runs_and_list_items_show_it() {
    // The same class of cost, reached by a run and by a list item: a face name of 1 MB in a style, shown by 10,000 spans, or in a list
    // level shown by 10,000 items. The family it finds is looked up once for each place the name is held.
    const NAME: usize = 1_000_000;
    let name = "n".repeat(NAME);
    let font = |mut d: Drawing| {
        d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
        d
    };
    let style = format!(r#"<style:style style:name="T1" style:family="text"><style:text-properties style:font-name="{name}"/></style:style>"#);
    let spans = format!("<text:p>{}</text:p>", r#"<text:span text:style-name="T1">a</text:span>"#.repeat(10_000));
    let (report, took) = timed(&font(Drawing::a4()).auto(&style).page(&rect_with(&spans)));
    assert_eq!(report.work.name_bytes, NAME as u64 + LAYOUT);
    assert!(took < SLOW, "{took:?}");
    let level = format!(
        r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="x"><style:text-properties style:font-name="{name}"/></text:list-level-style-bullet></text:list-style>"#
    );
    let items = format!(r#"<text:list text:style-name="L">{}</text:list>"#, "<text:list-item><text:p>a</text:p></text:list-item>".repeat(10_000));
    let (report, took) = timed(&font(Drawing::a4()).auto(&level).page(&rect_with(&items)));
    assert_eq!(report.work.name_bytes, NAME as u64 + 1 + LAYOUT, "the face name once, and the list's name `L`");
    assert!(took < SLOW, "{took:?}");
}

#[test]
fn a_style_s_texts_in_a_list_cost_the_paragraphs_nothing_after_the_first() {
    // A list's bullet characters and its face are made once and shared: 20 more paragraphs on the list cost what they cost on a list whose
    // bullet is one of the table's and has no face.
    let items = |bullet: &str, props: &str, paragraphs: usize| {
        let style = format!(
            r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="{bullet}">{props}</text:list-level-style-bullet></text:list-style>"#
        );
        let list =
            format!(r#"<text:list text:style-name="L">{}</text:list>"#, "<text:list-item><text:p>x</text:p></text:list-item>".repeat(paragraphs));
        let mut d = Drawing::a4();
        d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
        d.auto(&style).page(&rect_with(&list)).read().1.work.items
    };
    let more = |bullet: &str, props: &str| items(bullet, props, 40) - items(bullet, props, 20);
    let plain = more("\u{2022}", "");
    assert_eq!(more("ab", ""), plain, "the bullet's characters");
    assert_eq!(more("\u{2022}", r#"<style:text-properties style:font-name="F"/>"#), plain, "a face named in the declarations");
    assert_eq!(more("\u{2022}", r#"<style:text-properties fo:font-family="Free Serif"/>"#), plain, "a face named by its family");
}

#[test]
fn a_face_looked_up_by_its_name_costs_one_item_for_each_place_the_name_is_held() {
    // A face named in the declarations finds the same family as one named by `fo:font-family`; the lookup costs one item the first time a place is seen: for a list level's face and for
    // a text style's.
    let named = r#"<style:text-properties style:font-name="F"/>"#;
    let family = r#"<style:text-properties fo:font-family="Free Serif"/>"#;
    let drawing = || {
        let mut d = Drawing::a4();
        d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
        d
    };
    let level = |props: &str| {
        let style = format!(
            r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="x">{props}</text:list-level-style-bullet></text:list-style>"#
        );
        let list = r#"<text:list text:style-name="L"><text:list-item><text:p>x</text:p></text:list-item><text:list-item><text:p>y</text:p></text:list-item></text:list>"#;
        drawing().auto(&style).page(&rect_with(list)).read().1.work.items
    };
    assert_eq!(level(named) - level(family), 1, "a list level's face");
    let span = |props: &str, spans: usize| {
        let style = format!(r#"<style:style style:name="T1" style:family="text">{props}</style:style>"#);
        let body = format!("<text:p>{}</text:p>", r#"<text:span text:style-name="T1">a</text:span>"#.repeat(spans));
        drawing().auto(&style).page(&rect_with(&body)).read().1.work.items
    };
    // A family written as `fo:font-family` is the file's spelling, held beside the `Font` cell: the style's template is made once (an item, a pair of two, and the bytes of
    // the name and the value, one item), and each run that shows the style holds the value (an item). The face's lookup is the named one's alone.
    assert_eq!(span(family, 1) + 1 - span(named, 1), 4 + 1, "a text style's face");
    assert_eq!(span(family, 10) + 1 - span(named, 10), 4 + 10, "however many runs show it, the lookup is one");
}

#[test]
fn a_page_layout_is_charged_its_name_and_its_orientation_by_their_bytes() {
    // LOW: the name's charge could be deleted with the suite green, and the orientation string was held for nothing. A layout nothing
    // names, so that only its own charge shows.
    let items = |layout: &str| {
        let mut d = Drawing::a4();
        d.styles_auto.push_str(layout);
        d.page("").read().1.work.items
    };
    let named = |name: &str| format!(r#"<style:page-layout style:name="{name}"/>"#);
    // 200 bytes are four items and 1 byte is one.
    // The name is held by the layout as read, by the element kept whole (an attribute's value) and by the kept style's own name: three charges of its bytes.
    assert_eq!(items(&named(&"n".repeat(200))) - items(&named("n")), 3 * (200u64.div_ceil(64) - 1));
    let oriented = |orientation: &str| {
        format!(r#"<style:page-layout style:name="n"><style:page-layout-properties style:print-orientation="{orientation}"/></style:page-layout>"#)
    };
    // The orientation is held by the layout as read and by the element kept whole.
    assert_eq!(items(&oriented(&"o".repeat(200))) - items(&oriented("o")), 2 * (200u64.div_ceil(64) - 1));
    // An orientation the layout does not state costs nothing: one byte is one item.
    let bare = r#"<style:page-layout style:name="n"><style:page-layout-properties/></style:page-layout>"#;
    // (The attribute is also in the element kept whole: its pair, two items, and its bytes tip the element's over another 64.)
    assert_eq!(items(&oriented("o")) - items(bare), 1 + 2 + 1);
}

#[test]
fn the_item_cap_s_refusal_says_that_path_points_count_and_what_they_cost() {
    // LOW: a drawing of path points was refused with "items kept in the document" and no word of paths. A point is a geometry row and its
    // two cells (`a_geometry_section_is_charged_its_rows_and_its_cells`): the 29 the message gives (28 until the model's rows gained a slot for
    // their extension data: a row is then 112 bytes, and its 1,288 bytes with its node of cells are 21 items, not 20).
    assert_eq!(row_items() + 2 * 4, 29);
    let message = OdgError::Limit { kind: LimitKind::Items, max: 20_000_000 }.to_string();
    assert_eq!(
        message,
        "the package holds more than 20000000 items kept in the document (shapes, cells and text count, and each path point counts as 29, so about 690000 path points fill the default limit)"
    );
    // The reader's own refusal gives it.
    let shape = format!(r#"<draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="M0 0{}"/>"#, " L1 1".repeat(100));
    let Err(refused) = read(&Drawing::a4().page(&shape).bytes(), &Limits { max_items: 1_000, ..Limits::default() }) else { panic!("read") };
    assert_eq!(refused.class(), "too_many_items");
    assert!(refused.to_string().contains("each path point counts as 29"), "{refused}");
}

/// A number of `len` characters ending in `unit`.
fn long_number(len: usize, unit: &str) -> String {
    format!("{}{unit}", "1".repeat(len - unit.len()))
}

/// A million characters: the longest a value or a name can be (an attribute is capped at 1 MiB).
const MILLION: usize = 1_000_000;

#[test]
fn a_style_value_is_parsed_once_whatever_the_number_of_runs_that_show_it() {
    // 's HIGH: every run, paragraph and list item parsed the value strings of its style again (20.7 s for
    // 20,000 paragraphs on a style whose `fo:margin-left` is a million characters). Each value is parsed when its style is read: the
    // count is the values the styles hold, and the number of uses does not change it.
    let text_style = |attr: &str, value: &str| {
        format!(r#"<style:style style:name="T1" style:family="text"><style:text-properties {attr}="{value}"/></style:style>"#)
    };
    let spans = |n: usize| format!("<text:p>{}</text:p>", r#"<text:span text:style-name="T1">a</text:span>"#.repeat(n));
    for (attr, value, sizes) in [
        ("fo:letter-spacing", long_number(MILLION, "pt"), [2_000, 20_000]),
        ("style:text-position", long_number(MILLION, "%"), [2_000, 20_000]),
        ("fo:font-size", long_number(MILLION, "pt"), [2_000, 20_000]),
        ("style:text-scale", long_number(MILLION, "%"), [2_000, 20_000]),
        ("fo:color", format!("#{}", "f".repeat(MILLION - 1)), [2_000, 20_000]),
        ("fo:font-family", " ".repeat(MILLION), [2_000, 20_000]),
        ("fo:font-family", "'".repeat(MILLION), [2_000, 20_000]),
    ] {
        let mut counts = Vec::new();
        for uses in sizes {
            let d = Drawing::a4().auto(&text_style(attr, &value)).page(&rect_with(&spans(uses)));
            let (report, took) = timed(&d);
            assert!(took < SLOW, "{attr}: {took:?} for {uses} spans");
            counts.push(report.work.value_parses);
        }
        assert_eq!(counts, [1, 1], "{attr}: the one value, parsed once, for 2,000 spans and for 20,000");
    }
    // A paragraph style shown by 20,000 paragraphs.
    let paragraphs = r#"<text:p text:style-name="P1">a</text:p>"#.repeat(20_000);
    for (attr, value) in [
        ("fo:margin-left", long_number(MILLION, "cm")),
        ("fo:text-indent", long_number(MILLION, "cm")),
        ("fo:line-height", long_number(MILLION, "%")),
        ("fo:line-height", long_number(MILLION, "cm")),
        ("style:tab-stop-distance", long_number(MILLION, "cm")),
    ] {
        let style = format!(r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties {attr}="{value}"/></style:style>"#);
        let (report, took) = timed(&Drawing::a4().auto(&style).page(&rect_with(&paragraphs)));
        assert!(took < SLOW, "{attr}: {took:?}");
        assert_eq!(report.work.value_parses, 1, "{attr}");
    }
    // A list level shown by 20,000 list items: its font size.
    let level = format!(
        r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="x"><style:text-properties fo:font-size="{}" fo:font-family="{}"/></text:list-level-style-bullet></text:list-style>"#,
        long_number(MILLION, "pt"),
        " ".repeat(MILLION / 2)
    );
    let items = format!(r#"<text:list text:style-name="L">{}</text:list>"#, "<text:list-item><text:p>a</text:p></text:list-item>".repeat(20_000));
    let (report, took) = timed(&Drawing::a4().auto(&level).page(&rect_with(&items)));
    assert!(took < SLOW, "{took:?}");
    assert_eq!(report.work.value_parses, 1, "the level's size, once");
}

#[test]
fn the_values_a_style_holds_are_counted_one_each_and_a_size_chain_parses_each_ancestor_once() {
    // Three values: a length, a percentage and a colour.
    let style = r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:letter-spacing="1pt" style:text-scale="50%" fo:color="#112233" fo:font-weight="bold"/></style:style>"##;
    let (report, _) = timed(&Drawing::a4().auto(style).page(&rect_with(r#"<text:p><text:span text:style-name="T1">a</text:span></text:p>"#)));
    assert_eq!(report.work.value_parses, 3, "a keyword (`bold`) is not parsed");
    // Children whose sizes are percentages of a parent whose size is a million characters: the parent's is read once, each child's once.
    let parent = format!(
        r#"<style:style style:name="Base" style:family="text"><style:text-properties fo:font-size="{}"/></style:style>"#,
        long_number(MILLION, "pt")
    );
    let count = |children: usize| {
        let styles: String = (0..children)
            .map(|k| format!(r#"<style:style style:name="C{k}" style:family="text" style:parent-style-name="Base"><style:text-properties fo:font-size="{}%"/></style:style>"#, 100 + k % 50))
            .collect();
        let spans: String = (0..children).map(|k| format!(r#"<text:span text:style-name="C{k}">a</text:span>"#)).collect();
        let (report, took) = timed(&Drawing::a4().common(&parent).auto(&styles).page(&rect_with(&format!("<text:p>{spans}</text:p>"))));
        assert!(took < SLOW, "{took:?}");
        report.work.value_parses
    };
    assert_eq!(count(1_000), 1 + 1_000);
    assert_eq!(count(10_000), 1 + 10_000);
}

/// One automatic graphic style `gr1` that holds `props` itself and `common` beside it, paired with `uses` paragraph styles: `uses`
/// shape-style pairs.
fn pairs(props: &str, common: &str, uses: usize) -> Drawing {
    let paragraphs: String = (0..uses).map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"/>"#)).collect();
    let graphic = format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties {props}/></style:style>"#);
    let shapes: String =
        (0..uses).map(|k| format!(r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P{k}" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    Drawing::a4().common(common).auto(&paragraphs).auto(&graphic).page(&shapes)
}

#[test]
fn a_style_s_values_and_names_are_read_once_whatever_the_number_of_shape_style_pairs_that_show_them() {
    // One automatic graphic style that holds the names itself, paired with 20,000 paragraph styles, had its dash,
    // gradient and marker names hashed, and its DiaCraft links cloned and hashed, for every pair (2.9 to 3.5 s each). Its values were
    // parsed for every pair too. A name is looked up once, when its style is read.
    let name = "n".repeat(MILLION);
    let uses = 20_000;
    let dash = format!(r#"<draw:stroke-dash draw:name="{name}" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/>"#);
    let gradient = format!(r##"<draw:gradient draw:name="{name}" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff"/>"##);
    let marker = format!(r#"<draw:marker draw:name="{name}"/>"#);
    let third = "a".repeat(340_000);
    let (second, last) = ("m".repeat(340_000), "o".repeat(340_000));
    let linked: String = [&third, &second, &last].iter().map(|n| format!(r#"<style:style style:name="{n}" style:family="graphic"/>"#)).collect();
    let cases: [(&str, String, String, u64, u64); 6] = [
        ("a dash name", format!(r#"draw:stroke="dash" draw:stroke-dash="{name}" svg:stroke-width="1pt""#), dash, MILLION as u64, 1),
        ("a gradient name", format!(r#"draw:fill="gradient" draw:fill-gradient-name="{name}""#), gradient, MILLION as u64, 0),
        ("a marker name", format!(r#"draw:marker-start="{name}""#), marker.clone(), MILLION as u64, 0),
        ("a marker name nothing declares", format!(r#"draw:marker-end="{name}""#), String::new(), MILLION as u64, 0),
        ("three link names", format!(r#"diacraft:links="{third} {second} {last}""#), linked, 3 * 340_000, 0),
        (
            "a marker's width",
            format!(r#"draw:marker-start="{name}" draw:marker-start-width="{}""#, long_number(MILLION, "cm")),
            marker,
            MILLION as u64,
            1,
        ),
    ];
    for (what, props, common, bytes, parses) in cases {
        let (report, took) = timed(&pairs(&props, &common, uses));
        assert!(took < SLOW, "{what}: {took:?}");
        assert_eq!(report.work.name_bytes, bytes + LAYOUT, "{what}: each name hashed once, not once for each of {uses} pairs");
        assert_eq!(report.work.value_parses, parses, "{what}");
    }
    // The values a graphic style holds, parsed once for all the pairs.
    for attr in [
        "svg:stroke-width",
        "svg:stroke-opacity",
        "draw:opacity",
        "draw:shadow-offset-x",
        "draw:shadow-offset-y",
        "draw:shadow-opacity",
        "fo:padding",
        "fo:padding-top",
    ] {
        let value = long_number(MILLION, if attr.ends_with("opacity") { "%" } else { "cm" });
        let (report, took) = timed(&pairs(&format!(r#"{attr}="{value}""#), "", uses));
        assert!(took < SLOW, "{attr}: {took:?}");
        assert_eq!(report.work.value_parses, 1, "{attr}");
    }
}

#[test]
fn a_dash_gradient_marker_and_link_are_found_by_the_index_their_style_kept() {
    // The names still mean what they meant: a dash found by a name reads as its pattern, a name found by no one as the stand-in, a
    // gradient fills, a marker names the line end, and DiaCraft's links name the three styles the shape's links are made of.
    let (doc, report) = Drawing::a4()
        .common(r##"<draw:stroke-dash draw:name="D" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/><draw:gradient draw:name="G" draw:style="axial" draw:start-color="#ff0000" draw:end-color="#0000ff"/><draw:marker draw:name="DiaCraft_20_end_20_13"/><draw:marker draw:name="Other"/>"##)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="1pt" draw:marker-end="DiaCraft_20_end_20_13" draw:marker-start="Other"/></style:style>"#)
        .auto(r#"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="missing" svg:stroke-width="1pt" draw:marker-end="nothing" draw:marker-start=""/></style:style>"#)
        .auto(r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="G"/></style:style>"#)
        .page(concat!(
            r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#,
            r#"<draw:rect draw:style-name="gr2" svg:width="1cm" svg:height="1cm"/>"#,
            r#"<draw:rect draw:style-name="gr3" svg:width="1cm" svg:height="1cm"/>"#
        ))
        .read();
    let at = |ix: usize| shape(&doc, 0, ix);
    assert_eq!(text(cell(at(0), "LinePattern")), "9", "the dash D");
    assert_eq!(text(cell(at(0), "EndArrow")), "13", "DiaCraft's own marker");
    assert_eq!(text(cell(at(0), "BeginArrow")), "4", "another marker: the stand-in");
    assert_eq!(text(cell(at(1), "LinePattern")), "2", "a dash nobody declared: the stand-in");
    assert_eq!(text(cell(at(1), "EndArrow")), "0");
    assert_eq!(text(cell(at(1), "BeginArrow")), "0", "an empty marker name: no line end");
    assert_eq!(text(cell(at(2), "FillGradientEnabled")), "1", "the gradient G");
    assert_eq!(report.warned("marker_not_found"), 1);
    assert_eq!(report.kept("marker"), 1);
    assert_eq!(report.kept("dash"), 1);
}

#[test]
fn a_list_style_s_equal_numbered_levels_keep_the_order_they_were_written_at_any_size() {
    // LOW: `sort_unstable_by_key` keeps the order of a few equal levels and scrambles 500,000. The first level written with a number is the
    // one a list of that depth reads.
    let levels: String = std::iter::once(r#"<text:list-level-style-bullet text:level="2" text:bullet-char="t"/>"#.to_owned())
        .chain(std::iter::once(r#"<text:list-level-style-bullet text:level="1" text:bullet-char="f"/>"#.to_owned()))
        .chain((1..500_000).map(|_| r#"<text:list-level-style-bullet text:level="1" text:bullet-char="l"/>"#.to_owned()))
        .collect();
    let style = format!(r#"<text:list-style style:name="L">{levels}</text:list-style>"#);
    let list = r#"<text:list text:style-name="L"><text:list-item><text:p>x</text:p></text:list-item></text:list>"#;
    let (doc, _) = Drawing::a4().auto(&style).page(&rect_with(list)).read();
    let bullet = crate::drawing::row_cell(only(&doc), "Paragraph", None, 0, "BulletStr").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert_eq!(bullet.as_deref(), Some("f"));
}

#[test]
fn a_master_page_s_layout_is_looked_up_once_for_all_the_pages_on_it() {
    // The audit's page row: a layout named by a million characters, found by name for every page on its master page (0.17 s for the 1,000
    // pages the document may hold). It is found once for the master page.
    let name = "L".repeat(MILLION);
    let mut d = Drawing::a4();
    d.styles_auto.push_str(&format!(
        r#"<style:page-layout style:name="{name}"><style:page-layout-properties fo:page-width="30cm" fo:page-height="10cm"/></style:page-layout>"#
    ));
    d.masters = format!(r#"<style:master-page style:name="Default" style:page-layout-name="{name}"/>"#);
    for _ in 0..500 {
        d = d.page("");
    }
    let (doc, report) = d.read();
    assert_eq!(report.work.name_bytes, MILLION as u64);
    assert_eq!(doc.pages.len(), 500);
    for at in [0, 499] {
        crate::drawing::close(crate::drawing::num(crate::drawing::page(&doc, at).sheet.cell("PageWidth")), crate::drawing::cm(30.0));
    }
}

// Cells that hold a style's text or list are told the same, or apart, without reading it. The low finding of a re-review:
// two font faces whose families of a million characters differ only in the last one, a common style naming one and its automatic child the
// other, took 15 microseconds a shape-style pair (2.3 s at the item cap), the comparison of the two values walking them byte by byte.

/// A font family of `len` characters, all `f` but the last, which is `last`.
fn family_ending(len: usize, last: char) -> String {
    let mut family = "f".repeat(len - 1);
    family.push(last);
    family
}

/// `uses` shape-style pairs that each compare two font faces: the common paragraph style `Base` names the face `FA` (family `a`), each of
/// `uses` automatic paragraph styles, children of `Base`, names the face `FB` (family `b`), and one graphic style is paired with each.
fn face_pairs(a: &str, b: &str, uses: usize) -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = format!(r#"<style:font-face style:name="FA" svg:font-family="{a}"/><style:font-face style:name="FB" svg:font-family="{b}"/>"#);
    let paragraphs: String = (0..uses)
        .map(|k| {
            format!(
                r#"<style:style style:name="P{k}" style:family="paragraph" style:parent-style-name="Base"><style:text-properties style:font-name="FB"/></style:style>"#
            )
        })
        .collect();
    let shapes: String =
        (0..uses).map(|k| format!(r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P{k}" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    d.common(r#"<style:style style:name="Base" style:family="paragraph"><style:text-properties style:font-name="FA"/></style:style>"#)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"/>"#)
        .auto(&paragraphs)
        .page(&shapes)
}

/// The family a shape's first run row names, if the shape writes one of its own.
fn local_font(shape: &diacraft_doc::Shape) -> Option<String> {
    crate::drawing::row_cell(shape, "Character", None, 0, "Font").map(|c| text(Some(c)).to_owned())
}

#[test]
fn two_font_families_of_equal_length_are_told_apart_by_their_values_and_not_read_for_each_shape_style_pair() {
    // The review's input: families of 1,000,000 characters that differ in the last, 2,000 and 20,000 pairs. The bytes compared one by one
    // are the same for both (none: the two values are two shared texts), and each pair's one cell is looked for in the other's one list.
    let (a, b) = (family_ending(MILLION, 'a'), family_ending(MILLION, 'b'));
    for uses in [2_000, 20_000] {
        let (report, took) = timed(&face_pairs(&a, &b, uses));
        assert!(took < SLOW, "{took:?} for {uses} pairs");
        assert_eq!(report.work.compared_bytes, 0, "{uses} pairs");
        assert_eq!(report.work.cells_scanned, uses as u64, "{uses} pairs: the one cell, against the one in the other list");
    }
    // The pair still gives the right answer: the child's family, which is not the common style's.
    let (doc, _) = face_pairs(&a, &b, 3).read();
    for at in 0..3 {
        assert_eq!(local_font(shape(&doc, 0, at)).as_deref(), Some(b.as_str()), "shape {at}");
    }
}

#[test]
fn a_family_equal_to_the_common_style_s_is_not_local_and_one_that_differs_in_its_last_character_is_at_every_length() {
    // Short texts are compared as they are (at most 64 bytes, counted) and long ones are told by the value they are; the answer is the same
    // either side of the cut, and the bytes read are the short ones' only.
    for len in [1, 2, 63, 64, 65, 66, 200, 4_096, MILLION] {
        let (a, b) = (family_ending(len, 'a'), family_ending(len, 'b'));
        // Two faces declaring equal families, held in two places, are one value: the child's family is what the common style gives.
        let (doc, report) = face_pairs(&a, &a, 3).read();
        for at in 0..3 {
            assert_eq!(local_font(shape(&doc, 0, at)), None, "{len}: equal families, shape {at}");
        }
        assert_eq!(report.work.compared_bytes, 0, "{len}: one value is the same value");
        // Two that differ in the last character are two values: the child's is written, and a text no longer than 64 bytes was read for it.
        let (doc, report) = face_pairs(&a, &b, 3).read();
        for at in 0..3 {
            assert_eq!(local_font(shape(&doc, 0, at)).as_deref(), Some(b.as_str()), "{len}: different families, shape {at}");
        }
        let read = if len <= 64 { 3 * len as u64 } else { 0 };
        assert_eq!(report.work.compared_bytes, read, "{len}");
    }
}

/// As `face_pairs`, but the common style `Base` gives its family as the text `a` of an `fo:font-family` attribute and not by naming a face: the
/// two families reach their cells by different paths, a slice of an attribute and the text of a face's declaration.
fn mixed_pairs(a: &str, b: &str, uses: usize) -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = format!(r#"<style:font-face style:name="FB" svg:font-family="{b}"/>"#);
    let paragraphs: String = (0..uses)
        .map(|k| {
            format!(
                r#"<style:style style:name="P{k}" style:family="paragraph" style:parent-style-name="Base"><style:text-properties style:font-name="FB"/></style:style>"#
            )
        })
        .collect();
    let shapes: String =
        (0..uses).map(|k| format!(r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P{k}" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    d.common(&format!(r#"<style:style style:name="Base" style:family="paragraph"><style:text-properties fo:font-family="{a}"/></style:style>"#))
        .auto(r#"<style:style style:name="gr1" style:family="graphic"/>"#)
        .auto(&paragraphs)
        .page(&shapes)
}

#[test]
fn a_family_given_as_an_attribute_and_one_given_by_a_face_are_the_same_value_when_their_texts_are_equal() {
    // Equal long texts that reach their cells by two paths are one value, so they are told the same by an address; unequal ones are not.
    for len in [10, 64, 65, 200, MILLION] {
        let (a, b) = (family_ending(len, 'a'), family_ending(len, 'b'));
        let (doc, report) = mixed_pairs(&a, &a, 3).read();
        for at in 0..3 {
            assert_eq!(local_font(shape(&doc, 0, at)), None, "{len}: equal, shape {at}");
        }
        assert_eq!(report.work.compared_bytes, 0, "{len}");
        let (doc, _) = mixed_pairs(&a, &b, 3).read();
        for at in 0..3 {
            assert_eq!(local_font(shape(&doc, 0, at)).as_deref(), Some(b.as_str()), "{len}: different, shape {at}");
        }
    }
}

/// `n` tab stops at 1 mm, 2 mm and so on, with `last` as the final one's markup in place of its own when it is given.
fn stops(n: usize, last: Option<&str>) -> String {
    let list: String = (1..=n).map(|k| format!(r#"<style:tab-stop style:position="{k}mm"/>"#)).collect();
    match last {
        Some(last) => format!("{}{last}", list.trim_end_matches(&format!(r#"<style:tab-stop style:position="{n}mm"/>"#))),
        None => list,
    }
}

/// `uses` shape-style pairs that compare tab stops: the common paragraph style `C` holds the stops `common`, the automatic paragraph style
/// `P1` (its child) holds `own`, and `uses` graphic styles are each paired with `P1`.
fn tab_pairs(common: &str, own: &str, uses: usize) -> Drawing {
    let style = |name: &str, parent: &str, list: &str| {
        format!(
            r#"<style:style style:name="{name}" style:family="paragraph"{parent}><style:paragraph-properties><style:tab-stops>{list}</style:tab-stops></style:paragraph-properties></style:style>"#
        )
    };
    let graphics: String = (0..uses).map(|k| format!(r#"<style:style style:name="gr{k}" style:family="graphic"/>"#)).collect();
    let shapes: String =
        (0..uses).map(|k| format!(r#"<draw:rect draw:style-name="gr{k}" draw:text-style-name="P1" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    Drawing::a4().common(&style("C", "", common)).auto(&style("P1", r#" style:parent-style-name="C""#, own)).auto(&graphics).page(&shapes)
}

#[test]
fn tab_stops_of_two_styles_are_told_the_same_or_apart_when_the_styles_are_read_and_not_walked_for_each_shape_style_pair() {
    // The audit's row: two styles that hold 2,048 stops (the most) each, equal ones or ones that differ in the last, shown by 2,000 and by
    // 20,000 pairs. The stops are walked when the styles are read: the first list's hash, then the second's hash and its comparison with
    // the first when they are equal. A pair reads no stop.
    const MOST: usize = 2_048;
    let list = stops(MOST, None);
    let other = stops(MOST, Some(r#"<style:tab-stop style:position="3000mm"/>"#));
    // A list that differs is written whole by every pair (4,096 cells, charged: row 13 of the audit), so fewer pairs show it.
    for (own, walked, name, sizes) in
        [(&list, 3 * MOST as u64, "equal lists", [2_000, 20_000]), (&other, 2 * MOST as u64, "lists that differ in the last stop", [10, 100])]
    {
        for uses in sizes {
            let (report, took) = timed(&tab_pairs(&list, own, uses));
            assert!(took < SLOW, "{name}: {took:?} for {uses} pairs");
            assert_eq!(report.work.tab_stops_walked, walked, "{name}: {uses} pairs");
        }
    }
    // The answers: equal lists are not local, and a list that differs in its last stop is written whole.
    let (doc, _) = tab_pairs(&list, &list, 3).read();
    for at in 0..3 {
        assert!(crate::drawing::row_cell(shape(&doc, 0, at), "Tabs", None, 0, "Position0").is_none(), "equal lists, shape {at}");
    }
    let (doc, _) = tab_pairs(&list, &other, 3).read();
    for at in 0..3 {
        let s = shape(&doc, 0, at);
        crate::drawing::close(crate::drawing::num(crate::drawing::row_cell(s, "Tabs", None, 0, "Position0")), 1.0 / 25.4);
        crate::drawing::close(crate::drawing::num(crate::drawing::row_cell(s, "Tabs", None, 0, "Position2047")), 3000.0 / 25.4);
    }
}

#[test]
fn tab_stops_are_the_same_only_when_every_stop_has_the_same_position_unit_and_alignment() {
    // What `Tab == Tab` told before the lists were shared: positions equal as numbers (`-0cm` is `0cm`), the unit they were written in,
    // the alignment, and the count.
    let local = |common: &str, own: &str| {
        let (doc, _) = tab_pairs(common, own, 1).read();
        crate::drawing::row_cell(shape(&doc, 0, 0), "Tabs", None, 0, "Position0").is_some()
    };
    let at = |position: &str, kind: &str| format!(r#"<style:tab-stop style:position="{position}" style:type="{kind}"/>"#);
    let (one, two) = (at("1cm", "left"), format!("{}{}", at("1cm", "left"), at("2cm", "left")));
    assert!(!local(&one, &at("1cm", "left")), "equal");
    assert!(!local(&at("0cm", "left"), &at("-0cm", "left")), "-0 is 0");
    assert!(!local("", ""), "no stops, nor stops of no stop");
    assert!(local(&one, &at("1.5cm", "left")), "a position");
    assert!(local(&one, &at("10mm", "left")), "a unit: 10 mm is written as millimetres, 1 cm as centimetres");
    assert!(local(&one, &at("1cm", "right")), "an alignment");
    assert!(local(&one, &two), "a stop more");
    assert!(local(&two, &one), "a stop fewer");
    assert!(local(&format!("{}{}", at("1cm", "left"), at("2cm", "left")), &format!("{}{}", at("2cm", "left"), at("1cm", "left"))), "an order");
}

/// Every cell of a sheet: those directly under it, those directly under its sections, and those of its rows.
fn all_cells(sheet: &diacraft_doc::Sheet) -> Vec<&Cell> {
    let mut out: Vec<&Cell> = sheet.cells.values().collect();
    for section in &sheet.sections {
        out.extend(section.cells.values());
        for row in section.rows.iter() {
            out.extend(row.cells.values());
        }
    }
    out
}

/// The dash, gradient and marker `every_property` names, so that the style holding it gives every cell it can.
const NAMED_BY_EVERY_PROPERTY: &str = concat!(
    r#"<draw:stroke-dash draw:name="d" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/>"#,
    r##"<draw:gradient draw:name="g" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:border="20%"/>"##,
    r#"<draw:marker draw:name="m"/>"#
);

#[test]
fn no_cell_a_style_gives_holds_a_formula_so_there_is_no_formula_text_for_a_pair_to_compare() {
    // The audit's row for the formula text beside the value: the cells a conversion makes (`Out`) are the style sheets' and the shapes'
    // local cells, and a style's strings are values. A style holding every property the reader maps, in the graphic and the paragraph
    // family, with its tab stops and gradient, gives value cells only.
    let props = crate::read_hostile::every_property();
    let tabs =
        r#"<style:paragraph-properties><style:tab-stops><style:tab-stop style:position="1cm"/></style:tab-stops></style:paragraph-properties>"#;
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
    let (doc, _) = d
        .common(NAMED_BY_EVERY_PROPERTY)
        .common(&format!(r#"<style:style style:name="G" style:family="graphic">{props}{tabs}</style:style>"#))
        .common(&format!(r#"<style:style style:name="P" style:family="paragraph">{props}</style:style>"#))
        .page(&rect_with(r#"<text:p>x</text:p>"#))
        .read();
    // The root is the new drawing's (it holds the formulas of a new drawing's sheet); the two style sheets are the conversions'.
    assert_eq!(doc.styles.len(), 3);
    let mut seen = 0;
    for style in doc.styles.iter().skip(1) {
        for cell in all_cells(&style.sheet) {
            assert!(matches!(cell.formula, diacraft_doc::FormulaSlot::Absent), "{:?} in style {}", cell, style.id);
            seen += 1;
        }
    }
    assert!(seen > 60, "the two style sheets hold the cells of all their properties: {seen}");
}

/// `uses` shape-style pairs of graphic styles that each hold every property, with values other than their common parent's, so that every
/// list of a conversion is as long as it gets and nothing is equal: each cell is searched for in the whole of the other list.
fn full_pairs(uses: usize) -> Drawing {
    let props = crate::read_hostile::every_property();
    let other = props
        .replace("#112233", "#aabbcc")
        .replace("#445566", "#ddeeff")
        .replace("#778899", "#99aabb")
        .replace("1cm", "2cm")
        .replace("1mm", "3mm")
        .replace("50%", "40%")
        .replace("12pt", "14pt")
        .replace("120%", "150%")
        .replace("super 58%", "sub 33%");
    let autos: String = (0..uses)
        .map(|k| format!(r#"<style:style style:name="gr{k}" style:family="graphic" style:parent-style-name="G">{other}</style:style>"#))
        .collect();
    let shapes: String = (0..uses).map(|k| format!(r#"<draw:rect draw:style-name="gr{k}" svg:width="1cm" svg:height="1cm"/>"#)).collect();
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="F" svg:font-family="Free Serif"/>"#.to_owned();
    d.common(NAMED_BY_EVERY_PROPERTY)
        .common(&format!(r#"<style:style style:name="G" style:family="graphic">{props}</style:style>"#))
        .auto(&autos)
        .page(&shapes)
}

#[test]
fn the_search_for_a_cell_in_the_other_list_visits_what_a_conversion_can_give_and_no_more_however_many_pairs() {
    // The audit's row for `differing`'s search: both lists are the cells one conversion gives, at most one for each cell the reader
    // maps, whatever the style holds; a style that states every property, each different from its parent's, gives the longest lists
    // there are. The entries visited for each pair are the same for every pair, and a fixed number.
    let scans = |uses: usize| {
        let (report, took) = timed(&full_pairs(uses));
        assert!(took < SLOW, "{took:?} for {uses} pairs");
        report.work.cells_scanned
    };
    let (few, many) = (scans(500), scans(5_000));
    assert_eq!(few % 500, 0);
    let per_pair = few / 500;
    assert_eq!(many, 5_000 * per_pair, "the same for every pair");
    assert!(per_pair > 300 && per_pair <= 1_500, "{per_pair} entries visited for each pair: about 30 cells against about 30, a few times");
}
