//! Item 5 of the review (**lossless means the same content**): what the model writer writes back is what the reader saw, in the places where the model holds
//! a field and not the spelling of the element it came from. The reader holds, in the `vsdx` side of the slot of what the element belongs to, **what the file had** (`dc:title` was not there,
//! a time had no `xsi:type`, an element had `xml:lang`, a part was in the namespace of 2011, a row had no `PageSheet`, a text came before a `Rel`, the kept nodes of a shape and of its sheet
//! came in an order), and the writer writes it back. Each test reads a package, writes it from the model with the content unchanged and compares the content with the package's own, by
//! the real content check (`visit`), which finds what the writer changed in the content it did not mean to.
//!
//! **What is not recorded** (and the content check is to compare effective types and key the children of the property parts instead): whether a part's content type was a `Default` or an
//! `Override` when the type is the same, and the order of the elements of `core.xml` and `app.xml` among themselves.

use diacraft_testkit::package::{MasterSpec, NS_2011, Namespace, OD, PageSpec, Pkg, VsdxBuilder, cell, shape};

use crate::model_writer::{DOC, text_of, visit};

const CORE_OPEN: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">";

fn with_core(inner: &str) -> Vec<u8> {
    VsdxBuilder::new().core(&format!("{CORE_OPEN}{inner}</cp:coreProperties>")).page(PageSpec::new(1, "P")).build()
}

#[test]
fn a_core_part_with_no_title_is_not_given_one() {
    let written = visit("no title", &with_core("<dc:creator>A</dc:creator>"));
    assert!(!text_of(&written.bytes, "/docProps/core.xml").contains("title"), "{}", text_of(&written.bytes, "/docProps/core.xml"));
    // And one with an empty title keeps it.
    let written = visit("an empty title", &with_core("<dc:title/><dc:creator>A</dc:creator>"));
    assert!(text_of(&written.bytes, "/docProps/core.xml").contains("title"));
}

#[test]
fn a_time_with_no_type_and_a_field_with_a_language_are_written_as_they_were() {
    let inner = "<dc:title xml:lang=\"en\">T</dc:title><dc:subject xml:lang=\"de\">S</dc:subject><dcterms:created>2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2021-02-03T04:05:06Z</dcterms:modified>";
    let written = visit("times and languages", &with_core(inner));
    let core = text_of(&written.bytes, "/docProps/core.xml");
    assert!(core.contains("xml:lang=\"en\"") && core.contains("xml:lang=\"de\""), "{core}");
    assert!(core.contains("<dcterms:created>2020-01-02T03:04:05Z</dcterms:created>"), "the time the file had no type for has none: {core}");
    assert!(core.contains("xsi:type=\"dcterms:W3CDTF\">2021-02-03T04:05:06Z"), "{core}");
}

#[test]
fn what_a_field_of_the_app_part_holds_beside_its_text_is_written_back() {
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application><Company xml:lang=\"en\">C</Company><Manager>M<x:note xmlns:x=\"urn:x\" k=\"v\"/><!--c--></Manager></Properties>";
    let bytes = VsdxBuilder::new().app(app).page(PageSpec::new(1, "P")).build();
    let written = visit("fields with more than their text", &bytes);
    let out = text_of(&written.bytes, "/docProps/app.xml");
    assert!(out.contains("xml:lang=\"en\"") && out.contains("<x:note") && out.contains("<!--c-->"), "{out}");
}

#[test]
fn a_part_in_the_namespace_of_2011_is_written_in_it() {
    let shapes = shape(1, &cell("PinX", "1"));
    let bytes = VsdxBuilder::new()
        .namespace(Namespace::Core2011)
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "P").shapes(&shapes))
        .build();
    let written = visit("the namespace of 2011", &bytes);
    for part in [DOC, "/visio/pages/pages.xml", "/visio/pages/page1.xml", "/visio/masters/masters.xml", "/visio/masters/master1.xml"] {
        let text = text_of(&written.bytes, part);
        assert!(text.contains(NS_2011), "{part} is not in the namespace it was in: {text:.300}");
    }
}

#[test]
fn a_row_with_no_pagesheet_is_written_with_none_and_one_with_an_empty_one_with_one() {
    let mut bare_page = PageSpec::new(1, "Bare");
    bare_page.sheet = None;
    let mut bare_master = MasterSpec::new(1, "Bare");
    bare_master.sheet = None;
    let bytes = VsdxBuilder::new().page(bare_page).page(PageSpec::new(2, "Empty")).master(bare_master).master(MasterSpec::new(2, "Empty")).build();
    let written = visit("rows with and without a PageSheet", &bytes);
    let pages = text_of(&written.bytes, "/visio/pages/pages.xml");
    assert_eq!(pages.matches("PageSheet").count(), 1, "only the row that had one: {pages}");
    let masters = text_of(&written.bytes, "/visio/masters/masters.xml");
    assert_eq!(masters.matches("PageSheet").count(), 1, "{masters}");
}

#[test]
fn the_text_of_a_picture_that_came_before_its_relationship_is_written_before_it() {
    let foreign = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap">before<Rel r:id="rId1"/></ForeignData></Shape><Shape ID="2" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/>after</ForeignData></Shape>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(foreign).rel("rId1", &format!("{OD}image"), "../media/i.png"))
        .with(Pkg::new().part_without_override("/visio/media/i.png", [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4]).default_type("png", "image/png"))
        .build();
    let written = visit("text and Rel", &bytes);
    let page = text_of(&written.bytes, "/visio/pages/page1.xml");
    assert!(page.contains("before<Rel") && page.contains("/>after"), "{page}");
}

#[test]
fn the_kept_nodes_of_a_shape_and_of_its_sheet_come_in_the_order_the_file_had_them() {
    // `Trigger`s belong to the sheet and an element of another vocabulary and a comment to the shape: they followed the same cell, and each came where the file had it.
    let inner = format!("{}<Trigger N=\"t1\"/><x:a xmlns:x=\"urn:x\"/><!--c--><Trigger N=\"t2\"/>{}", cell("PinX", "1"), cell("PinY", "2"));
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&shape(1, &inner)))
        .more_styles(
            r#"<StyleSheet ID="3" NameU="S"><Cell N="LineWeight" V="1"/><Trigger N="s1"/><x:a xmlns:x="urn:x"/><Trigger N="s2"/></StyleSheet>"#,
        )
        .build();
    let written = visit("the order of kept nodes", &bytes);
    let page = text_of(&written.bytes, "/visio/pages/page1.xml");
    let at = |needle: &str| page.find(needle).unwrap_or_else(|| panic!("{needle} is not in {page}"));
    assert!(at("t1") < at("<x:a") && at("<x:a") < at("<!--c-->") && at("<!--c-->") < at("t2"), "{page}");
    let document = text_of(&written.bytes, DOC);
    assert!(
        document.find("s1").unwrap() < document.find("<x:a").unwrap() && document.find("<x:a").unwrap() < document.find("s2").unwrap(),
        "{document}"
    );
}
