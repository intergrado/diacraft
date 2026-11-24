//! The edges of the reader's accounting and of what it keeps, one test for each place a mutation run (every comparison moved by one, every
//! budget left unspent, every cap ignored) found the other tests did not look: the items each kind of node costs, a shared part's second
//! use, the exact byte at each budget, the keys of sections and rows, and the order a duplicate is resolved in.
//!
//! The numbers here are the cost model written down, not what the reader printed: an element costs one item and one for each attribute, and
//! a text, comment or instruction it keeps costs one (`Limits::max_items`).

use diacraft_doc::{Document, MAX_MASTERS, MAX_NAME_BYTES, Section, TextPart};
use diacraft_opc::Package;
use diacraft_testkit::package::{CT_DRAWING, CT_PAGES, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{Limits, VsdxError};

use crate::timing::pages_sharing_one_part;
use crate::{doc_of, first_shape, read_err, read_ok, read_with};

fn class(e: &VsdxError) -> &'static str {
    e.class()
}

fn sections_of(doc: &Document) -> Vec<std::sync::Arc<Section>> {
    first_shape(doc).sheet.sections.to_vec()
}

// ------------------------------------------------------------------------------------------------------------------ names and caps

#[test]
fn a_name_of_the_longest_allowed_reads_in_every_place_a_name_is_read() {
    let long = "N".repeat(MAX_NAME_BYTES);
    let xml = format!(
        "<Section N=\"{long}\"/><Section N=\"User\"><Row N=\"{long}\"><Cell N=\"{long}\" V=\"1\"/></Row><Cell N=\"{long}\" V=\"2\"/></Section>"
    );
    let (doc, _) = doc_of(&shape(1, &xml));
    let sections = sections_of(&doc);
    assert_eq!(sections[0].name.as_str(), long.as_str());
    let user = &sections[1];
    assert_eq!(user.rows[0].name.as_deref(), Some(long.as_str()));
    assert!(user.rows[0].cells.contains_key(long.as_str()));
    assert!(user.cells.contains_key(long.as_str()));
}

#[test]
fn the_models_masters_cap_is_the_ceiling_of_the_reader_s() {
    let bytes = (1..=MAX_MASTERS + 1).fold(VsdxBuilder::new(), |b, i| b.master(MasterSpec::new(i as u32, "M").without_part())).build();
    let e = read_err(&bytes, &Limits { max_masters: usize::MAX, ..Limits::default() });
    assert_eq!(class(&e), "too_many_masters");
}

#[test]
fn unnamed_cells_are_charged_to_the_cell_budget_as_named_ones_are() {
    let base = doc_of(&shape(1, "")).1.stats.cells;
    let five: String = (0..5).map(|_| "<Cell V=\"1\"/>").collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &five))).build();
    let (_, report) = read_with(&bytes, &Limits { max_cells: base + 5, ..Limits::default() });
    assert_eq!(report.stats.cells, base + 5, "they count as cells read");
    assert_eq!(class(&read_err(&bytes, &Limits { max_cells: base + 4, ..Limits::default() })), "too_many_cells");
}

// ------------------------------------------------------------------------------------------------------------------ the items budget

fn items(inner: &str) -> usize {
    doc_of(&shape(1, inner)).1.stats.items
}

#[test]
fn each_kind_of_node_costs_the_items_the_cost_model_says() {
    let base = items("");
    let cost = |inner: &str| items(inner) - base;
    // A section: itself and each attribute.
    assert_eq!(cost(r#"<Section N="User"/>"#), 2);
    assert_eq!(cost(r#"<Section N="User" Del="1"/>"#), 3);
    // A row: itself and each attribute, and the section it is in.
    assert_eq!(cost(r#"<Section N="User"><Row N="a"/></Section>"#), 4);
    assert_eq!(cost(r#"<Section N="User"><Row N="a" LocalName="b" T="c"/></Section>"#), 6);
    // A cell: itself and each attribute; the text it holds is one more; an element in it costs what it costs anywhere.
    assert_eq!(cost(r#"<Cell N="A" V="1"/>"#), 3);
    assert_eq!(cost(r#"<Cell N="A">hi</Cell>"#), 3);
    assert_eq!(cost(r#"<Cell N="A"><RefBy ID="1"/></Cell>"#), 4);
    assert_eq!(cost(r#"<Cell N="A"><!--c--></Cell>"#), 3);
    // An element the model keeps whole: itself, each attribute, each node in it, all the way down.
    assert_eq!(cost(r#"<X/>"#), 1);
    assert_eq!(cost(r#"<X a="1" b="2"/>"#), 3);
    assert_eq!(cost(r#"<X>t</X>"#), 2);
    assert_eq!(cost(r#"<X a="1" b="2">t</X>"#), 4);
    assert_eq!(cost(r#"<X><Y c="1">u<Z/></Y></X>"#), 5);
    assert_eq!(cost(r#"<Cell V="1"/>"#), 2, "a cell with no name is kept whole");
    // A node that is not an element, kept between the elements of a shape; blanks between them are layout and cost nothing.
    assert_eq!(cost("some text"), 1);
    assert_eq!(cost("<!--c-->"), 1);
    assert_eq!(cost("\n  <X/>\n  "), 1);
}

#[test]
fn the_items_budget_is_exact_at_each_node_kind() {
    // Each of these costs the number of items the test above says, so a budget of the base plus that reads and one less refuses.
    let base = items("");
    for (inner, cost) in [
        (r#"<Section N="User"/>"#, 2),
        (r#"<Section N="User"><Row N="a" T="c"/></Section>"#, 5),
        (r#"<X a="1" b="2">t</X>"#, 4),
        (r#"<Cell N="A">hi</Cell>"#, 3),
    ] {
        let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, inner))).build();
        read_with(&bytes, &Limits { max_items: base + cost, ..Limits::default() });
        assert_eq!(class(&read_err(&bytes, &Limits { max_items: base + cost - 1, ..Limits::default() })), "too_many_items", "{inner}");
    }
}

#[test]
fn a_part_that_pages_share_is_charged_its_items_for_each_use() {
    // Each shape of this part costs 8 items (itself and its ID, then two cells of three each). Three pages naming one part of six shapes
    // against three naming one of five: the sixth shape is paid for at each of the three uses.
    let five = items_of_pages(3, 5);
    let six = items_of_pages(3, 6);
    assert_eq!(six - five, 3 * 8);
    // And the budget is the same for the second use as for the first.
    let bytes = pages_sharing_one_part(3, 5);
    read_with(&bytes, &Limits { max_items: five, ..Limits::default() });
    assert_eq!(class(&read_err(&bytes, &Limits { max_items: five - 1, ..Limits::default() })), "too_many_items");
}

fn items_of_pages(pages: usize, shapes: usize) -> usize {
    read_ok(&pages_sharing_one_part(pages, shapes)).1.stats.items
}

#[test]
fn xml_bytes_are_the_sizes_of_the_parts_parsed_and_the_budget_is_exact() {
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("Width", "1")))).master(MasterSpec::new(1, "M")).build();
    let (_, report) = read_ok(&bytes);
    let package = Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    let size = |name: &str| package.part(name).unwrap().len() as u64;
    // The XML parts parsed, and the relationships parts read (a relationships part is charged to the same budget before it is parsed: eighty
    // pages with a hundred thousand relationships each cost 21 seconds and 1.1 GB that no budget saw).
    let rels = |part: &str| package.part(&diacraft_opc::name::rels_part_name(part)).map_or(0, |b| b.len() as u64);
    let expected = size(&report.document_part)
        + size("/visio/pages/pages.xml")
        + size("/visio/pages/page1.xml")
        + size("/visio/masters/masters.xml")
        + size("/visio/masters/master1.xml")
        + rels("/")
        + rels(&report.document_part)
        + rels("/visio/pages/pages.xml")
        + rels("/visio/pages/page1.xml")
        + rels("/visio/masters/masters.xml")
        + rels("/visio/masters/master1.xml");
    assert_eq!(report.stats.xml_bytes, expected);
    read_with(&bytes, &Limits { max_xml_bytes: expected, ..Limits::default() });
    assert_eq!(class(&read_err(&bytes, &Limits { max_xml_bytes: expected - 1, ..Limits::default() })), "too_much_xml");
}

// -------------------------------------------------------------------------------------------------------- the other budgets, exactly

#[test]
fn theme_bytes_are_spent_and_the_budget_is_exact() {
    const THEME: &str = r#"<a:theme xmlns:a="urn:x" name="t"/>"#;
    let size = THEME.len() as u64;
    let bytes = VsdxBuilder::new().theme(THEME.as_bytes()).theme(THEME.as_bytes()).build();
    // A budget of exactly one theme takes one and not the second.
    let (doc, report) = read_with(&bytes, &Limits { max_theme_bytes: size, ..Limits::default() });
    assert_eq!((doc.themes.len(), report.warning_count("themes_over_budget")), (1, 1));
    let (doc, report) = read_with(&bytes, &Limits { max_theme_bytes: size * 2, ..Limits::default() });
    assert_eq!((doc.themes.len(), report.warning_count("themes_over_budget")), (2, 0));
    let (doc, _) = read_with(&bytes, &Limits { max_theme_bytes: size - 1, ..Limits::default() });
    assert_eq!(doc.themes.len(), 0);
}

#[test]
fn picture_bytes_are_spent_and_the_budget_is_exact() {
    let one = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let two = r#"<Shape ID="2" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{one}{two}")).rel("rId1", &format!("{OD}image"), "../media/a.png").rel(
            "rId2",
            &format!("{OD}image"),
            "../media/b.png",
        ))
        .with(Pkg::new().part("/visio/media/a.png", "image/png", [0u8; 60]).part("/visio/media/b.png", "image/png", [1u8; 60]))
        .build();
    let (_, report) = read_with(&bytes, &Limits { max_media_bytes: 120, ..Limits::default() });
    assert_eq!((report.stats.media_bytes, report.warning_count("media_over_budget")), (120, 0));
    let (_, report) = read_with(&bytes, &Limits { max_media_bytes: 119, ..Limits::default() });
    assert_eq!((report.stats.media_bytes, report.warning_count("media_over_budget")), (60, 1));
}

#[test]
fn a_property_part_and_the_solutions_index_over_the_small_part_budget_are_not_read_and_say_so() {
    let (_, report) = read_ok(&VsdxBuilder::new().solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"/>").build());
    assert_eq!(report.warning_count("properties_over_budget"), 0);
    let limits = Limits { max_small_part_bytes: 40, ..Limits::default() };
    let (_, report) = read_with(&VsdxBuilder::new().solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"/>").build(), &limits);
    assert_eq!(report.warning_count("properties_over_budget"), 2, "core and app, both over 40 bytes");
    assert_eq!(report.diacraft, Some(diacraft_vsdx::DiacraftPart::Unreadable("solutions index too large")));
}

#[test]
fn a_listed_report_is_cut_at_the_entries_cap_and_the_count_goes_on() {
    // Pages index rows that are no pages are held, none of them skipped (the report's skipped list is empty now), and the thirty pages that
    // use one `ID` are warned of, the first ten in words and every one counted.
    let rows: String = (0..30).map(|_| "<NotAPage/>").collect();
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}{}</Pages>", "<Page ID=\"1\" Name=\"P\"/>".repeat(30));
    let pkg =
        Pkg::new().part("/visio/pages/pages.xml", CT_PAGES, pages).rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml");
    let bytes = VsdxBuilder::new().without_pages_part().with(pkg).build();
    let (doc, report) = read_with(&bytes, &Limits { max_report_entries: 10, ..Limits::default() });
    assert_eq!(report.skipped.len(), 0);
    assert_eq!(report.warnings.len(), 10);
    assert_eq!(report.warning_count("duplicate_page_id"), 29);
    let held = crate::held::dump(crate::held::rest(&doc.ext.get().expect("the document's slot").vsdx, "Pages")[0]);
    assert_eq!(held.matches("<NotAPage/>").count(), 30, "every row that is no page is held");
}

// ------------------------------------------------------------------------------------------------------------ what finds the parts

#[test]
fn a_document_relationship_that_leaves_the_package_is_no_document() {
    // Its target names a part the package has, and it is external: the reader does not follow it.
    let bytes = Pkg::new()
        .part("/visio/document.xml", CT_DRAWING, "<VisioDocument/>")
        .rel_external("/", "rId1", &format!("{VIS}document"), "/visio/document.xml")
        .build();
    let e = read_err(&bytes, &Limits::default());
    assert!(matches!(e, VsdxError::NoDrawingParts("no relationship leads to a document part")), "{e}");
}

#[test]
fn a_pages_part_whose_root_is_not_pages_gives_no_pages_and_a_warning() {
    let pages = format!("<Other xmlns=\"{NS_2012}\"><Page ID=\"1\" Name=\"A\"/></Other>");
    let pkg =
        Pkg::new().part("/visio/pages/pages.xml", CT_PAGES, pages).rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml");
    let (doc, report) = read_ok(&VsdxBuilder::new().without_pages_part().with(pkg).build());
    assert_eq!(doc.pages.len(), 0);
    assert_eq!(report.warning_count("part_unexpected_root"), 1);
}

#[test]
fn a_part_that_a_master_row_and_a_page_row_both_name_is_read_as_each_and_warned_of_for_the_wrong_one() {
    // The part is a master's: read as a page, its root is not `PageContents`. The two reads are not one.
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "../masters/master1.xml");
    let bytes = VsdxBuilder::new().without_pages_part().master(MasterSpec::new(1, "M").shapes(&shape(3, ""))).with(pkg).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.warning_count("part_unexpected_root"), 1);
    assert_eq!((doc.masters.len(), doc.pages.len()), (1, 1));
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 0, "a part that is not a page's gives the page nothing");
    assert_eq!(doc.masters.get(0).unwrap().shapes.len(), 1, "and is still the master's");
}

#[test]
fn a_relationship_id_is_found_by_the_namespace_its_prefix_is_bound_to() {
    // `x:id` is another vocabulary's `id`: the row has no part, and `r:id` under any prefix bound to the relationships namespace is.
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\" xmlns:x=\"urn:other\" xmlns:q=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel x:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><Rel q:id=\"rId1\"/></Page></Pages>"
    );
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{}</Shapes></PageContents>", shape(5, ""));
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", diacraft_testkit::package::CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    let (doc, _) = read_ok(&VsdxBuilder::new().without_pages_part().with(pkg).build());
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 0, "x:id is not a relationship id");
    assert_eq!(doc.pages.get(1).unwrap().shapes.len(), 1, "q:id is");
}

#[test]
fn an_element_with_a_prefix_nobody_bound_is_no_vsdx_element() {
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><u:Shape ID=\"1\"/><Shape ID=\"2\"/></Shapes></PageContents>");
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").part(&page)).build();
    let (doc, _) = read_ok(&bytes);
    let shapes = &doc.pages.get(0).unwrap().shapes;
    assert_eq!((shapes.len(), shapes.get(0).unwrap().id), (1, 2));
}

// -------------------------------------------------------------------------------------------------------------------- masters

fn master_with(id: u32, shapes: &str) -> MasterSpec {
    MasterSpec::new(id, "M").shapes(shapes)
}

#[test]
fn of_two_masters_with_one_id_the_first_is_the_one_a_shape_finds() {
    // Master 5 twice: the first holds shape 10, the second shape 20. A shape that names master 5 and a member that names shape 10 is right.
    let page = shape_with(1, r#" Master="5""#, &format!("<Shapes>{}</Shapes>", shape_with(2, r#" MasterShape="10""#, "")));
    let bytes = VsdxBuilder::new()
        .master(master_with(5, &shape(10, "")))
        .master(master_with(5, &shape(20, "")))
        .page(PageSpec::new(1, "P").shapes(&page))
        .build();
    let (_, report) = read_ok(&bytes);
    assert_eq!(report.warning_count("duplicate_master_id"), 1);
    assert_eq!(report.warning_count("master_shape_not_found"), 0);
}

#[test]
fn a_member_looks_its_master_shape_up_in_the_master_of_the_nearest_ancestor_that_has_one() {
    // Shape 1 has the master, shape 2 has none and names shape 10, and shape 3 below it names shape 10 as well: all three are the master's.
    let inner = shape_with(3, r#" MasterShape="10""#, "");
    let middle = shape_with(2, r#" MasterShape="10""#, &format!("<Shapes>{inner}</Shapes>"));
    let page = shape_with(1, r#" Master="5""#, &format!("<Shapes>{middle}</Shapes>"));
    let bytes = VsdxBuilder::new().master(master_with(5, &shape(10, ""))).page(PageSpec::new(1, "P").shapes(&page)).build();
    let (_, report) = read_ok(&bytes);
    assert_eq!(report.warning_count("master_shape_without_master"), 0);
    assert_eq!(report.warning_count("master_shape_not_found"), 0);
}

#[test]
fn a_shape_with_a_master_is_not_checked_for_a_master_shape_above_it() {
    // `Master` and `MasterShape` on one top-level shape: the shape's own master is the tie, and nothing above it is asked.
    let bytes = VsdxBuilder::new()
        .master(master_with(5, &shape(10, "")))
        .page(PageSpec::new(1, "P").shapes(&shape_with(1, r#" Master="5" MasterShape="10""#, "")))
        .build();
    let (_, report) = read_ok(&bytes);
    assert_eq!(report.warning_count("master_shape_without_master"), 0);
    // A shape with a `MasterShape` and no master above it is the case the warning is for.
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape_with(1, r#" MasterShape="10""#, ""))).build();
    assert_eq!(read_ok(&bytes).1.warning_count("master_shape_without_master"), 1);
}

#[test]
fn a_master_part_s_connects_are_the_master_s() {
    let part = format!(
        "<MasterContents xmlns=\"{NS_2012}\"><Shapes>{}{}</Shapes><Connects><Connect FromSheet=\"1\" ToSheet=\"2\"/></Connects></MasterContents>",
        shape(1, ""),
        shape(2, "")
    );
    let (doc, report) = read_ok(&VsdxBuilder::new().master(MasterSpec::new(1, "M").part(&part)).build());
    assert_eq!(doc.masters.get(0).unwrap().connects.len(), 1);
    assert_eq!(report.stats.connects, 1);
}

// ------------------------------------------------------------------------------------------------- keys, links and ids of sheets

#[test]
fn an_absent_geometry_ix_counts_the_geometry_sections_before_and_no_other() {
    let xml = r#"<Section N="Geometry"/><Section N="User"/><Section N="Geometry"/><Section N="Geometry" IX="7"/><Section N="Geometry"/>"#;
    let (doc, report) = doc_of(&shape(1, xml));
    let ix: Vec<Option<u32>> = sections_of(&doc).iter().map(|s| s.ix).collect();
    // The fifth section is the fourth Geometry, whatever number the one before it carried.
    assert_eq!(ix, [Some(0), None, Some(1), Some(7), Some(3)]);
    assert_eq!(report.warning_count("geometry_ix_not_rising"), 1, "3 after 7");
}

#[test]
fn geometry_sections_that_do_not_number_upward_are_warned_of_and_equal_is_not_upward() {
    let warned = |xml: &str| doc_of(&shape(1, xml)).1.warning_count("geometry_ix_not_rising");
    assert_eq!(warned(r#"<Section N="Geometry" IX="1"/><Section N="Geometry" IX="2"/>"#), 0);
    assert_eq!(warned(r#"<Section N="Geometry" IX="3"/><Section N="Geometry" IX="3"/>"#), 1);
    assert_eq!(warned(r#"<Section N="Geometry" IX="3"/><Section N="Geometry" IX="2"/>"#), 1);
}

#[test]
fn a_row_ix_that_is_not_a_number_is_kept_and_the_row_is_not_numbered_over_it() {
    let xml = r#"<Section N="Geometry" IX="0"><Row T="LineTo" IX="x"/><Row T="LineTo"/><Row T="LineTo" IX="9"/><Row T="LineTo"/></Section>"#;
    let (doc, _) = doc_of(&shape(1, xml));
    let section = &sections_of(&doc)[0];
    let ix: Vec<Option<u32>> = section.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ix, [None, Some(1), Some(9), Some(3)], "an absent IX is the count of rows before");
    assert_eq!(section.rows[0].raw.get("IX"), Some("x"), "and the unreadable one is kept as it was");
}

#[test]
fn a_row_key_that_two_rows_share_is_warned_of_by_name_and_by_number() {
    let warned = |xml: &str| doc_of(&shape(1, xml)).1.warning_count("duplicate_row_key");
    assert_eq!(warned(r#"<Section N="User"><Row N="a"/><Row N="b"/></Section>"#), 0);
    assert_eq!(warned(r#"<Section N="User"><Row N="a"/><Row N="a"/></Section>"#), 1);
    assert_eq!(warned(r#"<Section N="Geometry" IX="0"><Row IX="1"/><Row IX="2"/></Section>"#), 0);
    assert_eq!(warned(r#"<Section N="Geometry" IX="0"><Row IX="1"/><Row IX="1"/></Section>"#), 1);
}

#[test]
fn an_empty_style_link_is_no_link_and_is_kept_as_written() {
    let (doc, _) = doc_of(&shape_with(1, r#" LineStyle="" FillStyle="3" TextStyle="03""#, ""));
    let s = first_shape(&doc);
    assert_eq!((s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text), (None, Some(3), Some(3)));
    assert_eq!(s.sheet.raw.get("LineStyle"), Some(""));
    assert_eq!(s.sheet.raw.get("TextStyle"), Some("03"));
    assert_eq!(s.sheet.raw.get("FillStyle"), None, "a plain number is the field and nothing else");
}

#[test]
fn a_shape_id_that_is_not_a_number_is_zero_and_is_kept_as_written() {
    let (doc, report) = doc_of(r#"<Shape ID="x"/>"#);
    let s = first_shape(&doc);
    assert_eq!(s.id, 0);
    assert_eq!(s.raw.get("ID"), Some("x"));
    assert_eq!(report.warning_count("shape_id_not_a_number"), 1);
}

// ------------------------------------------------------------------------------------------------------------------------ text

#[test]
fn a_text_marker_is_one_only_when_its_number_is_plain_and_it_holds_what_a_marker_holds() {
    let (doc, _) = doc_of(&shape(
        1,
        r#"<Text><cp IX="7"/><cp IX="007"/><cp IX="+8"/><cp IX="1">x</cp><pp IX="2">y</pp><fld IX="3"><b/></fld><fld IX="4">ok</fld></Text>"#,
    ));
    let parts = first_shape(&doc).text.clone().unwrap();
    let kinds: Vec<&str> = parts
        .parts
        .iter()
        .map(|p| match p {
            TextPart::Cp(_) => "cp",
            TextPart::Pp(_) => "pp",
            TextPart::Fld { .. } => "fld",
            TextPart::Raw(_) => "raw",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["cp", "raw", "raw", "raw", "raw", "raw", "fld"]);
    assert_eq!(parts.parts[0], TextPart::Cp(7));
}

// ------------------------------------------------------------------------------------------------------------- shared parts, again

#[test]
fn a_part_that_pages_share_is_charged_its_cells_for_each_use() {
    // Three pages naming one part of five shapes of two cells: ten cells a use, and the budget is for the uses, not the parse.
    let bytes = pages_sharing_one_part(3, 5);
    let (_, report) = read_ok(&bytes);
    let base = read_ok(&VsdxBuilder::new().build()).1.stats.cells;
    assert_eq!(report.stats.cells, base + 3 * 10);
    read_with(&bytes, &Limits { max_cells: base + 30, ..Limits::default() });
    assert_eq!(class(&read_err(&bytes, &Limits { max_cells: base + 29, ..Limits::default() })), "too_many_cells");
}

// ------------------------------------------------------------------------------------------------------------ the DiaCraft version

#[test]
fn the_version_the_diacraft_part_states_is_cut_to_thirty_two_characters() {
    let version = "v".repeat(40);
    let xml = format!(
        "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"{version}\"><dc:Item/></dc:Data></SolutionXML>"
    );
    let (_, report) = read_ok(&VsdxBuilder::new().solution("DiaCraft", &xml).build());
    assert_eq!(report.diacraft, Some(diacraft_vsdx::DiacraftPart::Loaded { version: "v".repeat(32), children: 1 }));
}
