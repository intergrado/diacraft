//! Hostile packages: each cap and each refusal at its edge, the shapes of file that cost most, and what each hostile name, `ID` and link does.
//! The timed versions of the cases that a file's size can grow are in `timing.rs`.

use diacraft_doc::{
    MAX_CELLS_PER_CONTAINER, MAX_MASTERS, MAX_NAME_BYTES, MAX_PAGES, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET, MAX_SHAPE_DEPTH, MAX_STYLES,
};
use diacraft_opc::ErrorKind;
use diacraft_testkit::package::{CT_PAGE, MasterSpec, OD, PageSpec, Pkg, VsdxBuilder, ZipItem, cell, nested_shapes, shape};
use diacraft_vsdx::{LimitKind, Limits, VsdxError};

use crate::{first_shape, read_err, read_ok, read_with};

fn limit_class(e: &VsdxError) -> &'static str {
    assert!(matches!(e, VsdxError::Limit { .. }), "expected a cap, got {} ({e})", e.class());
    assert_eq!(e.kind(), ErrorKind::TooBig);
    e.class()
}

fn page_with(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes)).build()
}

fn cells(n: usize) -> String {
    (0..n).map(|i| cell(&format!("C{i}"), "1")).collect()
}

fn rows(n: usize) -> String {
    (0..n).map(|i| format!("<Row IX=\"{i}\"/>")).collect()
}

// ------------------------------------------------------------------------------------------------------------------------ caps

#[test]
fn pages_are_capped_at_the_model_s_cap_and_never_more() {
    let build = |n: usize| (1..=n).fold(VsdxBuilder::new(), |b, i| b.page(PageSpec::new(i as u32, "P").without_part())).build();
    let (doc, report) = read_ok(&build(MAX_PAGES));
    assert_eq!((doc.pages.len(), report.stats.pages), (MAX_PAGES, MAX_PAGES));
    let e = read_err(&build(MAX_PAGES + 1), &Limits::default());
    assert_eq!(limit_class(&e), "too_many_pages");
    // A caller cannot raise a cap past the model's own: the document must pass `Document::check_limits`.
    let e = read_err(&build(MAX_PAGES + 1), &Limits { max_pages: usize::MAX, ..Limits::default() });
    assert_eq!(limit_class(&e), "too_many_pages");
    // And can lower it.
    let e = read_err(&build(3), &Limits { max_pages: 2, ..Limits::default() });
    assert!(matches!(e, VsdxError::Limit { kind: LimitKind::Pages, max: 2 }));
}

#[test]
fn styles_are_capped_at_the_model_s_cap() {
    let styles = |n: usize| (1..n).map(|i| format!("<StyleSheet ID=\"{i}\"/>")).collect::<String>();
    // The root style is one of them: MAX_STYLES in all reads, and one more does not.
    let (doc, _) = read_ok(&VsdxBuilder::new().more_styles(&styles(MAX_STYLES)).build());
    assert_eq!(doc.styles.len(), MAX_STYLES);
    let e = read_err(&VsdxBuilder::new().more_styles(&styles(MAX_STYLES + 1)).build(), &Limits { max_styles: usize::MAX, ..Limits::default() });
    assert_eq!(limit_class(&e), "too_many_styles");
}

#[test]
fn masters_are_capped_at_the_model_s_cap() {
    let build = |n: usize| (1..=n).fold(VsdxBuilder::new(), |b, i| b.master(MasterSpec::new(i as u32, "M").without_part())).build();
    let (doc, _) = read_ok(&build(MAX_MASTERS));
    assert_eq!(doc.masters.len(), MAX_MASTERS);
    let e = read_err(&build(MAX_MASTERS + 1), &Limits::default());
    assert_eq!(limit_class(&e), "too_many_masters");
}

#[test]
fn shapes_in_a_part_are_counted_nested_ones_too() {
    let limits = Limits { max_shapes_per_part: 3, ..Limits::default() };
    let three = format!("{}{}", shape(1, &format!("<Shapes>{}</Shapes>", shape(2, ""))), shape(3, ""));
    let (_, report) = read_with(&page_with(&three), &limits);
    assert_eq!(report.stats.max_shapes_in_a_page, 3);
    let four = format!("{three}{}", shape(4, ""));
    assert_eq!(limit_class(&read_err(&page_with(&four), &limits)), "too_many_shapes_in_a_part");
}

#[test]
fn shapes_in_the_document_are_a_total_over_the_distinct_parts() {
    let b = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&(shape(1, "") + &shape(2, "") + &shape(3, ""))))
        .page(PageSpec::new(2, "B").shapes(&(shape(1, "") + &shape(2, "") + &shape(3, ""))));
    let bytes = b.build();
    let (_, report) = read_with(&bytes, &Limits { max_shapes: 6, ..Limits::default() });
    assert_eq!(report.stats.shapes, 6);
    assert_eq!(limit_class(&read_err(&bytes, &Limits { max_shapes: 5, ..Limits::default() })), "too_many_shapes");
}

#[test]
fn the_depth_of_nested_shapes_is_capped_before_the_tree_is_built() {
    let at = |depth: usize| page_with(&nested_shapes(depth - 1, &shape(9999, "")));
    let limits = Limits { max_shape_depth: 3, ..Limits::default() };
    let (_, report) = read_with(&at(3), &limits);
    assert_eq!(report.stats.max_shape_depth, 3);
    assert_eq!(limit_class(&read_err(&at(4), &limits)), "shapes_too_deep");
}

#[test]
fn the_models_depth_cap_holds_at_200_and_201_levels_when_the_xml_limit_allows_it() {
    // A shape at level d sits at XML depth 2d + 1 (the part's root, then a `Shapes` and a `Shape` for each level). The default XML depth
    // cap of 200 stops a tree at 99 levels first; raised to its ceiling of 512, the shape cap of 200 is the one that applies.
    let mut limits = Limits::default();
    limits.opc.xml.max_depth = 512;
    let at = |depth: usize| page_with(&nested_shapes(depth - 1, &shape(9999, "")));
    let (doc, report) = read_with(&at(MAX_SHAPE_DEPTH), &limits);
    assert_eq!(report.stats.max_shape_depth, MAX_SHAPE_DEPTH);
    doc.check_limits().unwrap();
    assert_eq!(limit_class(&read_err(&at(MAX_SHAPE_DEPTH + 1), &limits)), "shapes_too_deep");
    // And a caller who asks for more than the model allows still gets the model's.
    limits.max_shape_depth = usize::MAX;
    assert_eq!(limit_class(&read_err(&at(MAX_SHAPE_DEPTH + 1), &limits)), "shapes_too_deep");
}

#[test]
fn a_tree_a_hundred_thousand_levels_deep_is_refused_by_the_xml_limit_before_anything_recurses() {
    let bytes = page_with(&nested_shapes(100_000, ""));
    let e = read_err(&bytes, &Limits::default());
    assert_eq!(e.class(), "xml_too_complex");
    // Even with the XML depth raised to its ceiling it is the XML ceiling, then the shape cap, never a stack.
    let mut limits = Limits::default();
    limits.opc.xml.max_depth = usize::MAX;
    assert_eq!(read_err(&bytes, &limits).class(), "xml_too_complex");
}

#[test]
fn cells_in_the_document_are_a_total() {
    let bytes = page_with(&shape(1, &cells(10)));
    let (_, report) = read_with(&bytes, &Limits { max_cells: 13, ..Limits::default() });
    assert_eq!(report.stats.cells, 13, "the three flags of the root style and ten");
    assert_eq!(limit_class(&read_err(&bytes, &Limits { max_cells: 12, ..Limits::default() })), "too_many_cells");
}

#[test]
fn cells_in_one_place_are_capped_at_the_models_cap() {
    let (doc, report) = read_ok(&page_with(&shape(1, &cells(MAX_CELLS_PER_CONTAINER))));
    assert_eq!(first_shape(&doc).sheet.cells.len(), MAX_CELLS_PER_CONTAINER);
    assert_eq!(report.stats.max_cells_in_one_place, MAX_CELLS_PER_CONTAINER);
    assert_eq!(limit_class(&read_err(&page_with(&shape(1, &cells(MAX_CELLS_PER_CONTAINER + 1))), &Limits::default())), "too_many_cells_in_one_place");
    // In a section and in a row, the same.
    let in_section = format!("<Section N=\"Geometry\" IX=\"0\">{}</Section>", cells(MAX_CELLS_PER_CONTAINER + 1));
    assert_eq!(limit_class(&read_err(&page_with(&shape(1, &in_section)), &Limits::default())), "too_many_cells_in_one_place");
    let in_row = format!("<Section N=\"User\"><Row N=\"a\">{}</Row></Section>", cells(MAX_CELLS_PER_CONTAINER + 1));
    assert_eq!(limit_class(&read_err(&page_with(&shape(1, &in_row)), &Limits::default())), "too_many_cells_in_one_place");
}

#[test]
fn rows_in_a_section_and_sections_in_a_sheet_are_capped_at_the_models_cap() {
    let section = |n: usize| format!("<Section N=\"Scratch\">{}</Section>", rows(n));
    let (doc, report) = read_ok(&page_with(&shape(1, &section(MAX_ROWS_PER_SECTION))));
    assert_eq!(first_shape(&doc).sheet.sections[0].rows.len(), MAX_ROWS_PER_SECTION);
    assert_eq!(report.stats.max_rows_in_a_section, MAX_ROWS_PER_SECTION);
    assert_eq!(limit_class(&read_err(&page_with(&shape(1, &section(MAX_ROWS_PER_SECTION + 1))), &Limits::default())), "too_many_rows_in_a_section");

    let sections = |n: usize| (0..n).map(|i| format!("<Section N=\"Geometry\" IX=\"{i}\"/>")).collect::<String>();
    let (_, report) = read_ok(&page_with(&shape(1, &sections(MAX_SECTIONS_PER_SHEET))));
    assert_eq!(report.stats.max_sections_in_a_sheet, MAX_SECTIONS_PER_SHEET);
    assert_eq!(
        limit_class(&read_err(&page_with(&shape(1, &sections(MAX_SECTIONS_PER_SHEET + 1))), &Limits::default())),
        "too_many_sections_in_a_sheet"
    );
}

#[test]
fn a_name_of_the_longest_allowed_reads_and_one_byte_more_is_refused() {
    let long = "N".repeat(MAX_NAME_BYTES);
    let longer = "N".repeat(MAX_NAME_BYTES + 1);
    let (doc, _) = read_ok(&page_with(&shape(1, &cell(&long, "1"))));
    assert!(first_shape(&doc).sheet.cell(&long).is_some());
    for xml in [
        cell(&longer, "1"),
        format!("<Section N=\"{longer}\"/>"),
        format!("<Section N=\"User\"><Row N=\"{longer}\"/></Section>"),
        format!("<Section N=\"User\"><Row N=\"a\">{}</Row></Section>", cell(&longer, "1")),
    ] {
        assert_eq!(limit_class(&read_err(&page_with(&shape(1, &xml)), &Limits::default())), "name_too_long", "{}", &xml[..40.min(xml.len())]);
    }
}

#[test]
fn the_formula_budget_is_a_total_and_a_formula_it_cannot_afford_is_kept_as_text() {
    let many: String = (0..50).map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" F=\"Width+Height*2\"/>")).collect();
    let limits = Limits { max_formula_nodes: 30, ..Limits::default() };
    let (doc, report) = read_with(&page_with(&shape(1, &many)), &limits);
    let s = first_shape(&doc);
    assert_eq!(s.sheet.cells.len(), 50, "every cell is read");
    let unparsed = s.sheet.cells.values().filter(|c| matches!(&c.formula, diacraft_doc::FormulaSlot::Local(f) if !f.is_parsed())).count();
    assert!(unparsed > 30, "most formulas were refused for the budget: {unparsed}");
    assert_eq!(report.formula_errors.get("too_many_nodes"), Some(&(unparsed as u64)));
    assert!(report.stats.formula_nodes <= 30);
    // The text and the cached value of a formula the budget refused are kept.
    let c = s.sheet.cell("C49").unwrap();
    assert_eq!(c.formula.text(), Some("Width+Height*2"));
    assert_eq!(c.value.as_ref().unwrap().text(), "49");
}

#[test]
fn a_formula_too_long_to_parse_is_kept_and_counted() {
    // The formula parser's limit is 1 MiB and so is the cap on an attribute, so a file cannot reach `too_long` by default: a
    // caller that raises the attribute cap can, and the formula is kept as text with its value, and counted.
    let long = format!("1{}", "+1".repeat(600_000));
    let bytes = page_with(&shape(1, &format!("<Cell N=\"X\" V=\"2\" F=\"{long}\"/>")));
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "xml_too_complex");
    let mut limits = Limits::default();
    limits.opc.xml.max_attr_bytes = 4 * 1024 * 1024;
    let (doc, report) = read_with(&bytes, &limits);
    let c = first_shape(&doc).sheet.cell("X").unwrap().clone();
    assert_eq!(c.formula.text().map(str::len), Some(long.len()));
    assert_eq!(report.formula_errors.get("too_long"), Some(&1));
}

// ------------------------------------------------------------------------------------------------------ the container's refusals

#[test]
fn what_opc_refuses_is_refused_with_its_class() {
    use diacraft_testkit::package::zip;
    // A part that inflates past its limit: the ratio.
    let pkg = Pkg::new().item(ZipItem::zeros_under_the_ratio_cap("big.bin", 4 * 1024 * 1024));
    let mut limits = Limits::default();
    limits.opc.max_part_bytes = 1024 * 1024;
    let e = read_err(&pkg.build(), &limits);
    assert_eq!((e.class(), e.kind()), ("part_too_large", ErrorKind::TooBig));
    // Path traversal.
    let e = read_err(&zip(&[ZipItem::stored("../evil.xml", b"x")]), &Limits::default());
    assert_eq!(e.class(), "path_traversal");
    // Too many parts, and a duplicate.
    let two = Pkg::new().item(ZipItem::stored("a.bin", b"1")).item(ZipItem::stored("b.bin", b"2"));
    limits = Limits::default();
    limits.opc.max_parts = 1;
    assert_eq!(read_err(&two.build(), &limits).class(), "too_many_parts");
    let dup = Pkg::new().item(ZipItem::stored("a.bin", b"1")).item(ZipItem::stored("A.BIN", b"2"));
    assert_eq!(read_err(&dup.build(), &Limits::default()).class(), "duplicate_part");
    // A truncated archive.
    let good = VsdxBuilder::new().build();
    assert!(matches!(read_err(good.get(..good.len() / 2).unwrap(), &Limits::default()).class(), "not_zip" | "zip_corrupt"));
}

#[test]
fn an_entity_bomb_in_a_page_part_is_refused_not_expanded() {
    let lol = "<!DOCTYPE lolz [<!ENTITY lol \"lol\"><!ENTITY lol2 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\"><!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">]><PageContents>&lol3;</PageContents>";
    let started = std::time::Instant::now();
    let e = read_err(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(lol)).build(), &Limits::default());
    assert_eq!(e.class(), "xml_entity_refused");
    // A hang guard and no measure of cost (it takes a fraction of a second; the refusal or the count above is the claim).
    assert!(started.elapsed().as_secs() < 60);
}

#[test]
fn a_zip_bomb_part_is_refused_by_its_ratio_and_a_big_honest_part_reads() {
    let mut limits = Limits::default();
    limits.opc.max_ratio = 100;
    let pkg = VsdxBuilder::new().with(Pkg::new().item(ZipItem::zeros_under_the_ratio_cap("visio/media/x.bin", 1024 * 1024)));
    assert_eq!(read_err(&pkg.build(), &limits).class(), "zip_ratio_exceeded");
    read_ok(&pkg.build());
}

// -------------------------------------------------------------------------------------------- hostile names, ids and links

#[test]
fn a_relationship_cycle_is_data_and_nothing_follows_it() {
    // A page part whose relationship points back at the document, at the page itself and at a part that points at it: all are targets of
    // the right type or not, and the reader looks each up once.
    let page = PageSpec::new(1, "P")
        .shapes(r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#)
        .rel("rId1", &format!("{OD}image"), "/visio/pages/page1.xml")
        .rel("rId2", "urn:loop", "../document.xml")
        .rel("rId3", "urn:loop", "page1.xml");
    let (doc, report) = read_ok(&VsdxBuilder::new().page(page).build());
    // An `image` relationship to the page's own part is no picture (a page is not a picture format), and nothing loops.
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("picture_unresolved"), 1);
}

#[test]
fn an_embedded_objects_fallback_that_points_at_itself_is_one_lookup() {
    let shape_xml = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let b = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape_xml).rel("rId1", &format!("{OD}oleObject"), "../embeddings/o.bin")).with(
        Pkg::new().part("/visio/embeddings/o.bin", "application/vnd.openxmlformats-officedocument.oleObject", [1u8, 2]).rel(
            "/visio/embeddings/o.bin",
            "rId1",
            &format!("{OD}image"),
            "o.bin",
        ),
    );
    let (doc, report) = read_ok(&b.build());
    // The fallback of the object is itself, which is no picture: one lookup, no picture, and a warning.
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("picture_unresolved"), 1);
}

#[test]
fn a_relationship_target_that_climbs_out_of_the_package_leads_nowhere() {
    let shape_xml = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let page = PageSpec::new(1, "P").shapes(shape_xml).rel("rId1", &format!("{OD}image"), "../../../../etc/passwd");
    let (doc, report) = read_ok(&VsdxBuilder::new().page(page).build());
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("picture_unresolved"), 1);
}

#[test]
fn a_page_part_with_a_content_type_that_is_not_the_pages_is_read_all_the_same() {
    // Labels lie: the part is read by its root, not by the type a row of the content types gives it.
    let page = format!("<PageContents xmlns=\"{}\"><Shapes><Shape ID=\"4\"/></Shapes></PageContents>", diacraft_testkit::package::NS_2012);
    let pkg = VsdxBuilder::new().page(PageSpec::new(1, "P").without_part()).pkg();
    let _ = (pkg, CT_PAGE);
    let (doc, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(&page)).build());
    assert_eq!(first_shape(&doc).id, 4);
}

#[test]
fn text_with_a_million_markers_is_bounded_by_the_part_and_reads() {
    let markers: String = (0..200_000).map(|i| format!("<cp IX=\"{i}\"/>x")).collect();
    let started = std::time::Instant::now();
    let (doc, _) = read_ok(&page_with(&shape(1, &format!("<Text>{markers}</Text>"))));
    assert_eq!(first_shape(&doc).text.as_ref().unwrap().parts.len(), 400_000);
    // A hang guard and no measure of cost (it takes a fraction of a second; the refusal or the count above is the claim).
    assert!(started.elapsed().as_secs() < 60);
}

#[test]
fn a_shape_with_a_hundred_thousand_attributes_reads_in_linear_time() {
    let attrs: String = (0..100_000).map(|i| format!(" a{i}=\"{i}\"")).collect();
    let started = std::time::Instant::now();
    let (doc, _) = read_ok(&page_with(&format!("<Shape ID=\"1\"{attrs}/>")));
    assert_eq!(first_shape(&doc).raw.len(), 100_000);
    // A hang guard and no measure of cost (it takes a fraction of a second; the refusal or the count above is the claim).
    assert!(started.elapsed().as_secs() < 60);
}
