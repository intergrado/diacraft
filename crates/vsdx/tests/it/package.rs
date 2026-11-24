//! The package: how the document part is found, the six variants, the two namespaces, and what refuses a file and what only warns.

use diacraft_opc::ErrorKind;
use diacraft_testkit::package::{
    CT_DRAWING, CT_PAGE, CT_PAGES, MasterSpec, NS_2011, NS_2012, NS_R, Namespace, PageSpec, Pkg, VIS, VsdxBuilder, ZipItem, shape, zip,
};
use diacraft_vsdx::{Kind, Limits, Variant, VsdxError};

use crate::{doc_of, first_shape, read_err, read_ok};

fn doc_xml(ns: &str) -> String {
    format!("<VisioDocument xmlns=\"{ns}\"><StyleSheets/></VisioDocument>")
}

fn pkg_with_document(name: &str, xml: &str) -> Pkg {
    Pkg::new().part(name, CT_DRAWING, xml).rel("/", "rId1", &format!("{VIS}document"), name.trim_start_matches('/'))
}

#[test]
fn the_document_is_found_by_relationship_and_not_by_its_name() {
    // No part at `visio/document.xml`: a document relationship to `custom/main.xml` holding a `VisioDocument`.
    let bytes = pkg_with_document("/custom/main.xml", &doc_xml(NS_2012)).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.document_part, "/custom/main.xml");
    assert_eq!(doc.pages.len(), 0);
}

#[test]
fn a_part_named_document_xml_is_not_the_document_without_a_relationship() {
    let bytes = Pkg::new().part("/visio/document.xml", CT_DRAWING, doc_xml(NS_2012)).build();
    let e = read_err(&bytes, &Limits::default());
    assert_eq!(e.class(), "no_drawing_parts");
    assert_eq!(e.kind(), ErrorKind::NotAPackage);
}

#[test]
fn a_document_relationship_to_a_part_the_package_lacks_is_no_drawing_parts() {
    let bytes = Pkg::new().rel("/", "rId1", &format!("{VIS}document"), "visio/document.xml").build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "no_drawing_parts");
}

#[test]
fn an_external_document_relationship_is_not_followed() {
    let bytes = Pkg::new().rel_external("/", "rId1", &format!("{VIS}document"), "https://example.invalid/d.xml").build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "no_drawing_parts");
}

#[test]
fn a_relationship_type_is_compared_exactly() {
    // V5 of the package spec: `.../Page` is not `.../page`, and the same for the document type.
    let bytes = Pkg::new()
        .part("/visio/document.xml", CT_DRAWING, doc_xml(NS_2012))
        .rel("/", "rId1", "http://schemas.microsoft.com/visio/2010/relationships/Document", "visio/document.xml")
        .build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "no_drawing_parts");
}

#[test]
fn a_root_that_is_not_the_document_element_is_no_drawing_parts() {
    // A part reached by the document relationship whose root is not the expected one.
    for xml in ["<Pages/>", "<VisioDocument xmlns=\"urn:other\"/>", "<x:VisioDocument xmlns:x=\"urn:other\"/>"] {
        let bytes = pkg_with_document("/visio/document.xml", xml).build();
        assert_eq!(read_err(&bytes, &Limits::default()).class(), "no_drawing_parts", "{xml}");
    }
}

#[test]
fn a_package_with_no_relationships_at_all_is_no_drawing_parts() {
    let bytes = Pkg::new().build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "no_drawing_parts");
}

#[test]
fn what_is_not_a_package_is_the_containers_class() {
    for (bytes, class) in
        [(b"<html>not a zip</html>".to_vec(), "not_zip"), (Vec::new(), "not_zip"), (zip(&[ZipItem::stored("a.txt", b"x")]), "missing_content_types")]
    {
        let e = read_err(&bytes, &Limits::default());
        assert_eq!(e.class(), class);
        assert!(matches!(e, VsdxError::Package(_)));
        assert_eq!(e.kind(), ErrorKind::NotAPackage);
    }
}

#[test]
fn a_dtd_in_the_document_part_is_refused() {
    let xml = "<!DOCTYPE VisioDocument [<!ENTITY a \"b\">]><VisioDocument xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"/>";
    let e = read_err(&pkg_with_document("/visio/document.xml", xml).build(), &Limits::default());
    assert_eq!(e.class(), "xml_entity_refused");
    assert_eq!(e.kind(), ErrorKind::Damaged);
}

#[test]
fn a_document_part_that_is_not_well_formed_is_xml_malformed() {
    let e = read_err(&pkg_with_document("/visio/document.xml", "<VisioDocument><Colors></VisioDocument>").build(), &Limits::default());
    assert_eq!(e.class(), "xml_malformed");
    let e = read_err(&pkg_with_document("/visio/document.xml", "not xml at all").build(), &Limits::default());
    assert_eq!(e.class(), "xml_malformed");
}

#[test]
fn a_page_part_that_is_not_xml_we_accept_refuses_the_file() {
    // The same refusal for a page as for the document: a page that opened empty with a warning would look like a drawing with nothing on it.
    for (xml, class) in [
        ("<PageContents><Shapes></PageContents>", "xml_malformed"),
        ("<!DOCTYPE x [<!ENTITY a \"b\">]><PageContents/>", "xml_entity_refused"),
        ("<PageContents>&nope;</PageContents>", "xml_entity_refused"),
    ] {
        let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").part(xml)).build();
        assert_eq!(read_err(&bytes, &Limits::default()).class(), class, "{xml}");
    }
}

#[test]
fn a_master_part_that_is_not_xml_we_accept_refuses_the_file() {
    let bytes = VsdxBuilder::new().master(MasterSpec::new(1, "M").part("<MasterContents><Shapes></MasterContents>")).build();
    assert_eq!(read_err(&bytes, &Limits::default()).class(), "xml_malformed");
}

#[test]
fn an_xml_limit_passed_in_a_page_part_is_too_big_and_names_the_xml_class() {
    let mut limits = Limits::default();
    limits.opc.xml.max_depth = 8;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&diacraft_testkit::package::nested_shapes(6, ""))).build();
    let e = read_err(&bytes, &limits);
    assert_eq!(e.class(), "xml_too_complex");
    assert_eq!(e.kind(), ErrorKind::TooBig);
}

#[test]
fn the_six_variants_read_from_the_document_content_type() {
    for (ct, ext, kind, macros) in [
        ("application/vnd.ms-visio.drawing.main+xml", "vsdx", Kind::Drawing, false),
        ("application/vnd.ms-visio.drawing.macroEnabled.main+xml", "vsdm", Kind::Drawing, true),
        ("application/vnd.ms-visio.stencil.main+xml", "vssx", Kind::Stencil, false),
        ("application/vnd.ms-visio.stencil.macroEnabled.main+xml", "vssm", Kind::Stencil, true),
        ("application/vnd.ms-visio.template.main+xml", "vstx", Kind::Template, false),
        ("application/vnd.ms-visio.template.macroEnabled.main+xml", "vstm", Kind::Template, true),
    ] {
        let (_, report) = read_ok(&VsdxBuilder::new().document_type(ct).build());
        let variant = report.variant.expect("a variant");
        assert_eq!((variant.kind, variant.macro_enabled), (kind, macros), "{ct}");
        assert_eq!(variant.extension(), ext);
        assert_eq!(Variant::from_extension(&ext.to_ascii_uppercase()), Some(variant));
        assert_eq!(Variant::from_content_type(ct), Some(variant));
    }
    assert_eq!(Variant::from_extension("vsd"), None);
}

#[test]
fn a_vsdm_whose_content_type_says_stencil_reads_with_the_kind_from_the_content_type() {
    // The bytes carry no file name, so the extension check is the caller's: the variant says what the package is, and the extension a caller
    // sees can be compared with `Variant::from_extension`.
    let (_, report) = read_ok(&VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml").build());
    let variant = report.variant.unwrap();
    assert_eq!(variant.kind, Kind::Stencil);
    assert_ne!(Variant::from_extension("vsdm"), Some(variant));
}

#[test]
fn a_document_content_type_that_is_none_of_the_six_is_warned_of_and_read() {
    let (doc, report) = read_ok(&VsdxBuilder::new().document_type("application/xml").build());
    assert_eq!(report.variant, None);
    assert_eq!(report.warning_count("document_content_type_unknown"), 1);
    assert_eq!(doc.pages.len(), 0);
}

#[test]
fn a_macro_enabled_package_with_a_vba_project_reads_and_the_project_is_not_touched() {
    let bytes = VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml")
        .with(Pkg::new().part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", [0xD0u8, 0xCF, 0x11, 0xE0, 1, 2, 3]).rel(
            "/visio/document.xml",
            "rId90",
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
        ))
        .build();
    let (_, report) = read_ok(&bytes);
    assert!(report.variant.unwrap().macro_enabled);
    assert_eq!(report.stats.parts, 8, "six parts and the two relationships parts, which are parts too");
}

#[test]
fn of_the_document_spec_a_page_in_the_2011_namespace_reads_as_a_page() {
    let bytes = VsdxBuilder::new().namespace(Namespace::Core2011).page(PageSpec::new(1, "P").shapes(&shape(5, ""))).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(first_shape(&doc).id, 5);
    assert_eq!((report.namespaces.core_2011, report.namespaces.main_2012), (4, 0), "the document, the pages index, the masters index and the page");
}

#[test]
fn both_namespaces_in_one_package_are_one_vocabulary() {
    // A 2012 pages index and a 2011 page part, say: each part is read in its own namespace and the two count apart.
    let page = format!("<PageContents xmlns=\"{NS_2011}\"><Shapes><Shape ID=\"7\"/></Shapes></PageContents>");
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(&page)).build());
    assert_eq!(first_shape(&doc).id, 7);
    assert_eq!((report.namespaces.main_2012, report.namespaces.core_2011), (3, 1));
}

#[test]
fn a_part_with_no_namespace_is_read_as_the_formats_and_counted() {
    // Not V2 of the document spec (a new document's parts are all in `2012/main`: that is the writer's, M3). Reading a hand-written part with no
    // namespace at all as the reference app's is this reader's labelled choice, and the report counts such parts.
    let (doc, report) = read_ok(&VsdxBuilder::new().namespace(Namespace::None).page(PageSpec::new(1, "P").shapes(&shape(5, ""))).build());
    assert_eq!(first_shape(&doc).id, 5);
    assert_eq!(report.namespaces.none, 4);
}

#[test]
fn a_part_in_a_namespace_that_is_not_the_formats_is_not_read_and_says_so() {
    let page = "<PageContents xmlns=\"urn:other\"><Shapes><Shape ID=\"7\"/></Shapes></PageContents>";
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(page)).build());
    assert!(doc.pages.get(0).unwrap().shapes.is_empty());
    assert_eq!(report.warning_count("part_unexpected_root"), 1);
    // The part is not the page's: it is held as the bytes it is.
    assert_eq!(doc.pages.get(0).unwrap().part_name, None);
    let held = doc.parts.iter().find(|p| &*p.name == "/visio/pages/page1.xml").expect("the part is held");
    assert_eq!(&*held.bytes, page.as_bytes());
}

#[test]
fn the_relationship_attribute_may_use_any_prefix_bound_to_the_relationships_namespace() {
    // `q:id` where `q` is bound to the relationships namespace, and an `r:id` where `r` is bound to something else: only the first is the Rel.
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:q=\"{NS_R}\" xmlns:r=\"urn:not-relationships\"><Page ID=\"1\" Name=\"P\"><Rel q:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"Q\"><Rel r:id=\"rId1\"/></Page></Pages>"
    );
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"9\"/></Shapes></PageContents>");
    let bytes = Pkg::new()
        .part("/visio/document.xml", CT_DRAWING, doc_xml(NS_2012))
        .rel("/", "rId1", &format!("{VIS}document"), "visio/document.xml")
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId1", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml")
        .build();
    let (doc, _) = read_ok(&bytes);
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 1);
    assert!(doc.pages.get(1).unwrap().shapes.is_empty());
    assert_eq!(doc.pages.get(0).unwrap().rel_id.as_deref(), Some("rId1"));
    assert_eq!(doc.pages.get(1).unwrap().rel_id, None);
}

#[test]
fn a_directory_item_and_an_orphan_relationships_part_are_counted_and_the_file_reads() {
    let orphan = Pkg::new().item(ZipItem::stored("visio/pages/", b"")).part(
        "/visio/_rels/ghost.xml.rels",
        "application/vnd.openxmlformats-package.relationships+xml",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
    );
    let (_, report) = read_ok(&VsdxBuilder::new().with(orphan).build());
    assert_eq!(report.warning_count("opc_directory_item"), 1);
    assert_eq!(report.warning_count("opc_orphan_relationships"), 1);
}

#[test]
fn a_part_the_document_names_twice_is_used_once_and_the_second_relationship_is_warned_of() {
    let bytes = VsdxBuilder::new().with(Pkg::new().rel("/visio/document.xml", "rId77", &format!("{VIS}pages"), "pages/pages.xml")).build();
    let (_, report) = read_ok(&bytes);
    assert_eq!(report.warning_count("several_relationships_of_one_type"), 1);
}

#[test]
fn the_documents_properties_are_read_from_the_two_property_parts() {
    // The fixture.
    let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Plant A</dc:title><dc:subject>Wiring</dc:subject><dc:creator>J. Noor</dc:creator><cp:lastModifiedBy>T. Bell</cp:lastModifiedBy><cp:keywords>pipes valves</cp:keywords><dc:description>First issue</dc:description><cp:category>Survey</cp:category></cp:coreProperties>"#;
    let app = r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Another Drawing App</Application><Company>Noor Ltd</Company><Manager></Manager><HyperlinkBase>https://example.org/docs/</HyperlinkBase><AppVersion>16.0000</AppVersion></Properties>"#;
    let (doc, report) = read_ok(&VsdxBuilder::new().core(core).app(app).build());
    assert_eq!(&*doc.title, "Plant A");
    let p = &doc.props;
    assert_eq!(p.subject.as_deref(), Some("Wiring"));
    assert_eq!(p.creator.as_deref(), Some("J. Noor"));
    assert_eq!(p.last_modified_by.as_deref(), Some("T. Bell"));
    assert_eq!(p.keywords.as_deref(), Some("pipes valves"));
    assert_eq!(p.description.as_deref(), Some("First issue"));
    assert_eq!(p.category.as_deref(), Some("Survey"));
    assert_eq!(p.company.as_deref(), Some("Noor Ltd"));
    assert_eq!(p.manager.as_deref(), Some(""), "present and empty is an empty string");
    assert_eq!(p.hyperlink_base.as_deref(), Some("https://example.org/docs/"));
    assert_eq!(p.app_version.as_deref(), Some("16.0000"));
    assert_eq!(report.application.as_deref(), Some("Another Drawing App"));
}

#[test]
fn a_property_element_that_is_missing_is_none_and_a_broken_part_is_a_warning() {
    let bytes = VsdxBuilder::new().core("<cp:coreProperties xmlns:cp=\"urn:x\"/>").app("<Properties><Company>").build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(&*doc.title, "");
    assert_eq!(doc.props.subject, None);
    assert_eq!(doc.props.company, None);
    assert_eq!(report.warning_count("properties_unreadable"), 1);
}

#[test]
fn a_package_with_no_property_parts_reads() {
    let bytes = pkg_with_document("/visio/document.xml", &doc_xml(NS_2012)).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(&*doc.title, "");
    assert_eq!(report.application, None);
}

#[test]
fn a_picture_relationship_to_a_url_is_never_followed() {
    let shape = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId5"/></ForeignData></Shape>"#;
    let external = Pkg::new().rel_external(
        "/visio/pages/page1.xml",
        "rId5",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
        "https://example.invalid/x.png",
    );
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape)).with(external).build());
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.warning_count("picture_unresolved"), 1);
}

#[test]
fn a_one_page_drawing_reads_with_its_counts_in_the_report() {
    let (doc, report) = doc_of(&shape(5, ""));
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(report.stats.pages, 1);
    assert_eq!(report.stats.shapes, 1);
}
