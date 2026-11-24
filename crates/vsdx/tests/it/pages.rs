//! Pages, their parts, connects and pictures.

use std::sync::Arc;

use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape, shape_with};
use diacraft_vsdx::{Limits, ReadReport};

use crate::{doc_of, first_shape, read_ok, read_with};

fn pages_of(builder: VsdxBuilder) -> (diacraft_doc::Document, ReadReport) {
    read_ok(&builder.build())
}

#[test]
fn a_page_with_no_rel_is_a_page_with_a_sheet_and_no_shapes() {
    let (doc, report) = pages_of(VsdxBuilder::new().page(PageSpec::new(0, "P").sheet(r#"<Cell N="PageWidth" V="8.5"/>"#).without_part()));
    assert_eq!(doc.pages.len(), 1);
    let page = doc.pages.get(0).unwrap();
    assert_eq!((page.id, &*page.name, page.name_u.as_deref()), (0, "P", Some("P")));
    assert_eq!(page.rel_id, None);
    assert!(page.shapes.is_empty());
    assert_eq!(page.sheet.cell("PageWidth").unwrap().value.as_ref().unwrap().text(), "8.5");
    assert_eq!(report.stats.shapes_per_page, [0]);
    assert!(report.warnings.is_empty());
}

#[test]
fn background_and_back_page_and_an_absent_background_is_a_foreground() {
    let (doc, _) =
        pages_of(VsdxBuilder::new().page(PageSpec::new(1, "Bg").attrs(r#" Background="1""#)).page(PageSpec::new(2, "Fg").attrs(r#" BackPage="1""#)));
    let (bg, fg) = (doc.pages.get(0).unwrap(), doc.pages.get(1).unwrap());
    assert!(bg.background && !fg.background);
    assert_eq!((bg.back_page, fg.back_page), (None, Some(1)));
    assert!(bg.raw.is_empty() && fg.raw.is_empty(), "a typed attribute that gives its text back is not kept twice");
    assert_eq!(doc.background_chain(2).len(), 1);
}

#[test]
fn a_background_flag_that_is_not_the_word_one_keeps_its_text() {
    let (doc, _) = pages_of(
        VsdxBuilder::new().page(PageSpec::new(1, "A").attrs(r#" Background="0""#)).page(PageSpec::new(2, "B").attrs(r#" Background="true""#)),
    );
    let (a, b) = (doc.pages.get(0).unwrap(), doc.pages.get(1).unwrap());
    assert!(!a.background && b.background);
    assert_eq!(a.raw.get("Background"), Some("0"));
    assert_eq!(b.raw.get("Background"), Some("true"));
}

#[test]
fn a_page_part_is_reached_by_its_rel_and_not_by_its_number_or_its_id() {
    // Page 1's part is `page1.xml` whatever its ID, and an ID of 7 on the first row does not look for `page7.xml`.
    let (doc, _) = pages_of(VsdxBuilder::new().page(PageSpec::new(7, "A").shapes(&shape(70, ""))).page(PageSpec::new(0, "B").shapes(&shape(80, ""))));
    let ids: Vec<u32> = doc.pages.iter().map(|p| p.shapes.get(0).unwrap().id).collect();
    assert_eq!(ids, [70, 80]);
}

#[test]
fn a_page_row_whose_rel_names_a_missing_relationship_gives_a_page_with_no_shapes() {
    let (doc, report) = pages_of(VsdxBuilder::new().without_pages_part().with(dangling_pages()));
    let page = doc.pages.get(0).unwrap();
    assert!(page.shapes.is_empty());
    assert_eq!(page.rel_id.as_deref(), Some("rId99"), "the row is kept as it was");
    assert_eq!(report.warning_count("page_part_unresolved"), 1);
}

fn dangling_pages() -> Pkg {
    let pages_xml = r#"<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><Page ID="3" Name="Q" NameU="Q"><PageSheet/><Rel r:id="rId99"/></Page></Pages>"#;
    Pkg::new().part("/visio/pages/pages.xml", "application/vnd.ms-visio.pages+xml", pages_xml).rel(
        "/visio/document.xml",
        "rId50",
        "http://schemas.microsoft.com/visio/2010/relationships/pages",
        "pages/pages.xml",
    )
}

#[test]
fn a_page_with_no_name_takes_its_nameu_and_with_neither_is_called_nothing() {
    let pages_xml =
        r#"<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Page ID="1" NameU="U"/><Page ID="2"/><Page ID="3" Name=""/></Pages>"#;
    let pkg = Pkg::new().part("/visio/pages/pages.xml", "application/vnd.ms-visio.pages+xml", pages_xml).rel(
        "/visio/document.xml",
        "rId50",
        "http://schemas.microsoft.com/visio/2010/relationships/pages",
        "pages/pages.xml",
    );
    let (doc, report) = pages_of(VsdxBuilder::new().without_pages_part().with(pkg));
    let names: Vec<&str> = doc.pages.iter().map(|p| &*p.name).collect();
    assert_eq!(names, ["U", "", ""]);
    assert_eq!(report.warning_count("page_without_name"), 1);
    assert_eq!(report.warning_count("page_name_not_valid"), 2, "a blank name is kept as it is and warned of");
}

#[test]
fn the_view_attributes_and_the_custom_name_flags_are_kept_in_order() {
    let (doc, _) = pages_of(
        VsdxBuilder::new()
            .page(PageSpec::new(1, "P").attrs(r#" ViewScale="-1" ViewCenterX="4" ViewCenterY="5" IsCustomName="1" ReviewerID="2" Extra="x""#)),
    );
    let raw: Vec<_> = doc.pages.get(0).unwrap().raw.iter().collect();
    assert_eq!(raw, [("ViewScale", "-1"), ("ViewCenterX", "4"), ("ViewCenterY", "5"), ("IsCustomName", "1"), ("ReviewerID", "2"), ("Extra", "x")]);
}

#[test]
fn a_page_with_no_id_is_skipped_and_reported() {
    let pages_xml = r#"<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Page Name="A"/><Page ID="x" Name="B"/><Page ID="4" Name="C"/></Pages>"#;
    let pkg = Pkg::new().part("/visio/pages/pages.xml", "application/vnd.ms-visio.pages+xml", pages_xml).rel(
        "/visio/document.xml",
        "rId50",
        "http://schemas.microsoft.com/visio/2010/relationships/pages",
        "pages/pages.xml",
    );
    let (doc, report) = pages_of(VsdxBuilder::new().without_pages_part().with(pkg));
    assert_eq!(doc.pages.iter().map(|p| p.id).collect::<Vec<_>>(), [4]);
    // The two rows that have no usable `ID` are held in place, before the one that has.
    assert_eq!(report.skipped_counts.len(), 0);
    let held = crate::held::dump(crate::held::rest(&doc.ext.get().expect("the document's slot").vsdx, "Pages")[0]);
    assert!(held.contains("[after 0] <n><Page Name=\"A\"/></n>") && held.contains("[after 0] <n><Page ID=\"x\" Name=\"B\"/></n>"), "{held}");
}

#[test]
fn a_page_id_used_twice_keeps_both_and_the_first_is_the_one_found() {
    let (doc, report) = pages_of(VsdxBuilder::new().page(PageSpec::new(1, "First")).page(PageSpec::new(1, "Second")));
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(&*doc.page(1).unwrap().name, "First");
    assert_eq!(report.warning_count("duplicate_page_id"), 1);
}

#[test]
fn a_back_page_that_names_nothing_is_warned_of_and_a_cycle_of_back_pages_ends() {
    let (doc, report) = pages_of(
        VsdxBuilder::new()
            .page(PageSpec::new(1, "A").attrs(r#" BackPage="2""#))
            .page(PageSpec::new(2, "B").attrs(r#" BackPage="1""#))
            .page(PageSpec::new(3, "C").attrs(r#" BackPage="99""#)),
    );
    assert_eq!(report.warning_count("back_page_not_found"), 1);
    // The chain of a cycle ends at the first page it has already given.
    assert_eq!(doc.background_chain(1).len(), 1);
    assert_eq!(doc.background_chain(3).len(), 0);
}

#[test]
fn document_order_is_display_order_and_kept() {
    // Shape 6 draws in front of shape 5.
    let (doc, _) = doc_of(&format!("{}{}", shape(5, ""), shape(6, "")));
    let ids: Vec<u32> = doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    assert_eq!(ids, [5, 6]);
}

#[test]
fn a_group_holds_its_members_in_order() {
    let inner = format!("<Shapes>{}{}</Shapes>", shape(11, ""), shape(12, ""));
    let (doc, report) = doc_of(&shape_with(10, r#" Type="Group""#, &inner));
    let group = first_shape(&doc);
    assert_eq!(group.children.iter().map(|s| s.id).collect::<Vec<_>>(), [11, 12]);
    assert_eq!((report.stats.shapes, report.stats.max_shape_depth), (3, 2));
}

#[test]
fn a_shapes_element_with_a_foreign_child_or_an_attribute_is_noted_and_the_shapes_read() {
    let (doc, report) = doc_of(r#"<Shape ID="1"><Shapes foo="x"><Shape ID="2"/><y:z xmlns:y="urn:q"/><!--c--></Shapes></Shape>"#);
    assert_eq!(first_shape(&doc).children.len(), 1);
    // What the `Shapes` element holds besides its shape is held on the group: its attribute, the foreign element and the comment, after the shape before them.
    assert_eq!(report.skipped_counts.len(), 0);
    let group = first_shape(&doc);
    let held = crate::held::dump(crate::held::rest(&group.ext.get().expect("the group's slot").vsdx, "Shapes")[0]);
    assert_eq!(held, " foo=\"x\" [after 1] <n><y:z xmlns:y=\"urn:q\"/></n> [after 1] <n><!--c--></n>");
}

#[test]
fn a_connect_row_is_read() {
    let connect = r#"<Connect FromSheet="7" FromCell="BeginX" FromPart="9" ToSheet="5" ToCell="Connections.X1" ToPart="100" Extra="x"/>"#;
    let (doc, report) =
        pages_of(VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("{}{}", shape(5, ""), shape(7, ""))).connects(connect)));
    let c = doc.pages.get(0).unwrap().connects.get(0).unwrap().clone();
    assert_eq!((c.from_sheet, c.to_sheet, c.from_part, c.to_part), (7, 5, Some(9), Some(100)));
    assert_eq!((c.from_cell.as_deref(), c.to_cell.as_deref()), (Some("BeginX"), Some("Connections.X1")));
    assert_eq!(c.raw.get("Extra"), Some("x"));
    assert_eq!(report.stats.connects, 1);
}

#[test]
fn a_connect_row_without_a_readable_sheet_is_skipped() {
    let (doc, report) = pages_of(VsdxBuilder::new().page(
        PageSpec::new(1, "P").connects(r#"<Connect ToSheet="5"/><Connect FromSheet="x" ToSheet="1"/><Connect FromSheet="1" ToSheet="2"/><Other/>"#),
    ));
    assert_eq!(doc.pages.get(0).unwrap().connects.len(), 1);
    // The two rows that are no connection and the element that is no row are held in place: the rows before the connection, the element after it.
    assert_eq!(report.skipped_counts.len(), 0);
    let page = doc.pages.get(0).unwrap();
    let held = crate::held::dump(crate::held::rest(&page.ext.get().expect("the page's slot").vsdx, "Connects")[0]);
    assert_eq!(
        held,
        " [after 0] <n><Connect ToSheet=\"5\"/></n> [after 0] <n><Connect FromSheet=\"x\" ToSheet=\"1\"/></n> [after 1] <n><Other/></n>"
    );
}

#[test]
fn two_pages_that_name_one_part_share_what_it_gave_and_it_is_read_once() {
    // A hostile index can point a thousand rows at one part: it must cost one read and one copy.
    let pages_xml = r#"<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><Page ID="1" Name="A"><Rel r:id="rId1"/></Page><Page ID="2" Name="B"><Rel r:id="rId1"/></Page></Pages>"#;
    let page =
        r#"<PageContents xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Shapes><Shape ID="5"/><Shape ID="6"/></Shapes></PageContents>"#;
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", "application/vnd.ms-visio.pages+xml", pages_xml)
        .rel("/visio/document.xml", "rId50", "http://schemas.microsoft.com/visio/2010/relationships/pages", "pages/pages.xml")
        .part("/visio/pages/page1.xml", "application/vnd.ms-visio.page+xml", page)
        .rel("/visio/pages/pages.xml", "rId1", "http://schemas.microsoft.com/visio/2010/relationships/page", "page1.xml");
    let (doc, report) = pages_of(VsdxBuilder::new().without_pages_part().with(pkg));
    let (a, b) = (doc.pages.get(0).unwrap(), doc.pages.get(1).unwrap());
    assert!(a.shapes.is_same_list_as(&b.shapes), "one list for both pages");
    assert_eq!(report.stats.shapes, 4, "charged for each use: a walker meets them in each page");
    assert_eq!(report.stats.shapes_per_page, [2, 2]);
}

#[test]
fn a_picture_is_the_part_the_relationship_reaches_and_is_copied_once() {
    let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
    let one = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let two = one.replace("ID=\"1\"", "ID=\"2\"");
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{one}{two}")).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png));
    let (doc, report) = pages_of(builder);
    let shapes = &doc.pages.get(0).unwrap().shapes;
    let (a, b) = (shapes.get(0).unwrap(), shapes.get(1).unwrap());
    let (pa, pb) = (a.foreign.as_ref().unwrap().picture.clone().unwrap(), b.foreign.as_ref().unwrap().picture.clone().unwrap());
    assert!(Arc::ptr_eq(&pa, &pb), "one copy for both shapes");
    assert_eq!(&*pa.bytes, png.as_slice());
    assert_eq!((&*pa.part, &*pa.content_type), ("/visio/media/image1.png", "image/png"));
    assert_eq!(report.stats.media_bytes, png.len() as u64);
    let foreign = a.foreign.as_ref().unwrap();
    assert_eq!(foreign.rel_id.as_deref(), Some("rId1"));
    assert_eq!(foreign.attr("ForeignType"), Some("Bitmap"));
    assert_eq!(foreign.attr("CompressionType"), Some("PNG"));
}

#[test]
fn an_embedded_object_shows_its_fallback_picture() {
    // ForeignType Object, a Rel to a .bin part, and that part's .rels with an image relationship to an EMF: the EMF is the picture.
    let emf = [1u8, 0, 0, 0, 9, 9];
    let shape_xml = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let builder =
        VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape_xml).rel("rId1", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin")).with(
            Pkg::new()
                .part("/visio/embeddings/oleObject1.bin", "application/vnd.openxmlformats-officedocument.oleObject", [0xD0u8, 0xCF, 0x11, 0xE0])
                .part("/visio/media/image1.emf", "image/x-emf", emf)
                .rel("/visio/embeddings/oleObject1.bin", "rId1", &format!("{OD}image"), "../media/image1.emf"),
        );
    let (doc, report) = pages_of(builder);
    let picture = first_shape(&doc).foreign.as_ref().unwrap().picture.clone().unwrap();
    assert_eq!(&*picture.bytes, emf.as_slice());
    assert_eq!(&*picture.content_type, "image/x-emf");
    assert_eq!(report.warning_count("picture_unresolved"), 0);
}

#[test]
fn an_embedded_object_with_no_picture_shows_nothing_and_says_so() {
    let shape_xml = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(shape_xml).rel("rId1", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin"))
        .with(Pkg::new().part("/visio/embeddings/oleObject1.bin", "application/vnd.openxmlformats-officedocument.oleObject", [1u8]));
    let (doc, report) = pages_of(builder);
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("picture_unresolved"), 1);
}

#[test]
fn a_picture_whose_part_is_missing_or_whose_rel_is_missing_is_none() {
    let one = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let two = r#"<Shape ID="2" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let (doc, report) = pages_of(VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("{one}{two}")).rel(
        "rId1",
        &format!("{OD}image"),
        "../media/gone.png",
    )));
    for s in &doc.pages.get(0).unwrap().shapes {
        assert!(s.foreign.as_ref().unwrap().picture.is_none());
    }
    assert_eq!(report.warning_count("picture_unresolved"), 2);
}

#[test]
fn pictures_past_the_budget_are_left_out_and_counted() {
    let one = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let two = r#"<Shape ID="2" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{one}{two}")).rel("rId1", &format!("{OD}image"), "../media/a.png").rel(
            "rId2",
            &format!("{OD}image"),
            "../media/b.png",
        ))
        .with(Pkg::new().part("/visio/media/a.png", "image/png", [0u8; 60]).part("/visio/media/b.png", "image/png", [0u8; 60]));
    let limits = Limits { max_media_bytes: 100, ..Limits::default() };
    let (doc, report) = read_with(&builder.build(), &limits);
    let shapes = &doc.pages.get(0).unwrap().shapes;
    assert!(shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.is_some());
    assert!(shapes.get(1).unwrap().foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("media_over_budget"), 1);
    assert_eq!(report.stats.media_bytes, 60);
}

#[test]
fn an_ink_shape_is_read_like_any_other_with_its_foreign_data() {
    // `Ink` shapes draw; reading keeps everything, as for any foreign data.
    let ink = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Ink"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(ink).rel("rId1", &format!("{OD}oleObject"), "../embeddings/ink1.bin"))
        .with(Pkg::new().part("/visio/embeddings/ink1.bin", "application/vnd.openxmlformats-officedocument.oleObject", [7u8]));
    let (doc, _) = pages_of(builder);
    assert_eq!(first_shape(&doc).foreign.as_ref().unwrap().attr("ForeignType"), Some("Ink"));
}
