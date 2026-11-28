//! Who writes `.odg` files, by the census: one hand-built
//! vector per family, each with the traits the census counts for it (names only: no sample's bytes), read to cells. Also the two tests the
//! plan names beside the vectors: the shape index's numbering, and markup the reader does not model being kept or counted.

use diacraft_odg::{Limits, read};

use crate::build::{DRAWING, manifest, write};
use crate::drawing::{Drawing, NS, cell, close, cm, meta, num, odf, odf_page, only, page, settings, shape};

/// A frame holding text and a rectangle, the two things every family writes.
const BODY: &str = r#"<draw:rect draw:name="r" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"/><draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="1cm"><draw:text-box><text:p>t</text:p></draw:text-box></draw:frame>"#;

/// Reads `d` and checks the two shapes of [`BODY`].
#[track_caller]
fn reads_body(bytes: &[u8]) -> diacraft_odg::ReadReport {
    let (doc, report) = match read(bytes, &Limits::default()) {
        Ok(done) => done,
        Err(e) => panic!("{} ({e})", e.class()),
    };
    let r = shape(&doc, 0, 0);
    close(num(cell(r, "PinY")), 10.019685039370078);
    close(num(cell(r, "Width")), cm(4.0));
    assert!(shape(&doc, 0, 1).text.is_some());
    report
}

/// LibreOffice's package of `version`: a producer namespace with its own elements and attributes, a settings part, a thumbnail and
/// `Configurations2/` (census vectors 1 to 3).
pub fn libreoffice(version: &str) -> Vec<u8> {
    let mut d = Drawing::a4().page(BODY);
    d.version = version.to_owned();
    d.ns.push_str(r#" xmlns:loext="urn:example:producer-extension""#);
    d.common = r#"<style:style style:name="standard" style:family="graphic"><style:graphic-properties loext:decorative="false" svg:stroke-width="0cm"/><loext:graphic-properties/></style:style>"#.to_owned();
    d.meta = Some(meta("LibreOffice/26.2.6.3$Linux_X86_64 LibreOffice_project/0"));
    d.settings = Some(settings(
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">1</config:config-item>"#,
    ));
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), vec![0x89, b'P', b'N', b'G']));
    d.files.push(("Configurations2/".to_owned(), Vec::new()));
    d.bytes()
}

/// An OpenOffice.org-line package of `version`: no producer extension, no `office:font-face-decls` in `content.xml`, and, with `doctype`,
/// a manifest whose DOCTYPE names only an external id (census vectors 5 and 6).
pub fn openoffice(version: &str, doctype: bool) -> Vec<u8> {
    let mut d = Drawing::a4().page(BODY);
    d.version = version.to_owned();
    d.meta = Some(meta("OpenOffice.org/2.4$Linux OpenOffice.org_project/680m17$Build-9310"));
    let content = d.content_xml().replace("<office:font-face-decls></office:font-face-decls>", "");
    let files = vec![
        ("content.xml".to_owned(), content.into_bytes()),
        ("styles.xml".to_owned(), d.styles_xml().into_bytes()),
        ("meta.xml".to_owned(), d.meta.clone().unwrap_or_default().into_bytes()),
    ];
    let rows: Vec<(&str, &str)> = files.iter().map(|(n, _)| (n.as_str(), "text/xml")).collect();
    let mut man = manifest(DRAWING, &rows);
    if doctype {
        man = man.replacen(
            "<manifest:manifest",
            "<!DOCTYPE manifest:manifest PUBLIC \"-//Example//DTD Manifest 1.0//EN\" \"Manifest.dtd\">\n<manifest:manifest",
            1,
        );
    }
    let mut entries = files;
    entries.push(("META-INF/manifest.xml".to_owned(), man.into_bytes()));
    write(Some(DRAWING), &entries)
}

#[test]
fn reads_every_generator_family_the_census_found() {
    // 1 to 3: LibreOffice packages at 1.2, 1.3 and 1.4.
    for version in ["1.2", "1.3", "1.4"] {
        let report = reads_body(&libreoffice(version));
        assert_eq!(report.office_version.as_deref(), Some(version));
        assert!(report.generator.as_deref().is_some_and(|g| g.starts_with("LibreOffice/")));
        assert!(report.kept("style_element") >= 1 && report.kept("property") >= 1, "the producer's own markup is counted");
    }
    // 4: flat `.fodg` is read as the package that holds the same elements (the reader's side is `flat.rs`): `sniff` tells it by its root.
    let flat = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:version=\"1.3\" office:mimetype=\"application/vnd.oasis.opendocument.graphics\"><office:body><office:drawing/></office:body></office:document>"
    );
    assert_eq!(diacraft_odg::sniff(flat.as_bytes()), Some(diacraft_odg::Variant::Drawing));
    assert_eq!(read(flat.as_bytes(), &Limits::default()).map(|(doc, _)| doc.pages.len()).ok(), Some(0), "a flat drawing with no page reads");
    // 5 and 6: the OpenOffice.org line, 1.0 and 1.1 with the manifest's DOCTYPE, and 1.2 without.
    for (version, doctype) in [("1.0", true), ("1.1", true), ("1.2", false)] {
        let report = reads_body(&openoffice(version, doctype));
        assert_eq!(report.office_version.as_deref(), Some(version));
        assert!(report.generator.as_deref().is_some_and(|g| g.starts_with("OpenOffice.org/")));
    }
    // 7: pre-standard namespace URIs for `fo` and `svg`: frames and formatting read all the same.
    let mut d = Drawing::a4().page(BODY);
    d.ns = NS
        .replace("urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0", "http://www.w3.org/1999/XSL/Format")
        .replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "http://www.w3.org/2000/svg");
    d.version = "1.0".to_owned();
    d.auto = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties svg:stroke-width="0.1cm" fo:padding="0.2cm"/></style:style>"##.to_owned();
    d.pages = d.pages.replace("<draw:rect draw:name=\"r\"", "<draw:rect draw:style-name=\"gr1\" draw:name=\"r\"");
    let bytes = d.bytes();
    reads_body(&bytes);
    let (doc, _) = read(&bytes, &Limits::default()).unwrap();
    close(num(cell(only(&doc), "LineWeight")), cm(0.1));
    close(num(cell(only(&doc), "TopMargin")), cm(0.2));
    close(num(page(&doc, 0).sheet.cell("PageHeight")), cm(29.7));
    // 8: no generator: no meta.xml, no settings, no thumbnail, no styles at all, and no office:version.
    let mut d = Drawing::a4().page(BODY);
    d.no_styles = true;
    d.version = String::new();
    let bytes = d.bytes();
    let bytes_without_version = String::from_utf8(d.content_xml().into_bytes()).unwrap().replace(" office:version=\"\"", "");
    let entries = vec![("content.xml".to_owned(), bytes_without_version.into_bytes())];
    let rows = [("content.xml", "text/xml")];
    let mut all = entries;
    all.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
    let report = {
        let (doc, report) = read(&write(Some(DRAWING), &all), &Limits::default()).unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(page(&doc, 0).shapes.len(), 2);
        report
    };
    assert_eq!((report.generator, report.office_version), (None, None));
    drop(bytes);
    // 9: Calligra: 1.2, no office:scripts, and a namespace of its own beside ODF's.
    let mut d = Drawing::a4().page(BODY);
    d.version = "1.2".to_owned();
    d.ns.push_str(r#" xmlns:calligra="http://www.calligra.org/2005/""#);
    d.pages = d.pages.replace("<draw:rect draw:name=\"r\"", "<draw:rect calligra:id=\"x\" draw:name=\"r\"");
    d.meta = Some(meta("Calligra/2.8"));
    let report = reads_body(&d.bytes());
    assert_eq!(report.kept("foreign_attribute"), 1);
    // 10: a library writer: minimal, 1.4, no thumbnail, no Configurations2/, no settings part.
    let mut d = Drawing::a4().page(BODY);
    d.version = "1.4".to_owned();
    d.meta = Some(meta("odfpy/1.4"));
    let report = reads_body(&d.bytes());
    assert_eq!(report.generator.as_deref(), Some("odfpy/1.4"));
}

#[test]
fn shape_index_numbers_document_order_per_page() {
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:rect draw:name="a" draw:z-index="1" svg:width="1cm" svg:height="1cm"/><draw:custom-shape draw:name="c"/><draw:rect draw:name="b" draw:z-index="0" svg:width="1cm" svg:height="1cm"/>"#)
        .page(r#"<draw:ellipse draw:name="d" svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    let index = &report.index;
    assert_eq!(index.pages(), 2);
    // Page 1: a is 1, the custom shape 2, b 3; drawing order b, a, then the custom shape (no z-index: its place is its document position),
    // numbering by document order.
    let names: Vec<(u32, &str)> = page(&doc, 0).shapes.iter().map(|s| (s.id, s.name.as_deref().unwrap_or(""))).collect();
    assert_eq!(names, [(3, "b"), (1, "a"), (2, "c")]);
    assert_eq!(index.ordinal(0, 1), Some(1));
    assert_eq!(index.ordinal(0, 3), Some(3));
    assert_eq!(index.ordinal(0, 2), Some(2), "the custom shape is modelled");
    assert!(index.page(0).unwrap().kept().is_empty());
    assert_eq!(index.high_water(0), Some(3));
    assert_eq!(index.next_id(0), Some(4));
    // Page 2 numbers from 1 again.
    assert_eq!(shape(&doc, 1, 0).id, 1);
    assert_eq!(index.ordinal(1, 1), Some(1));
    assert_eq!(index.page(1).map(|p| p.source), Some(diacraft_odg::PageSource::Page(1)));
    assert_eq!(index.page(2), None);
}

#[test]
fn unknown_markup_is_counted_not_dropped() {
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:unknown""#);
    d.common = r#"<style:style style:name="g" style:family="graphic"><style:graphic-properties x:cell="1" draw:auto-grow-height="false"/></style:style><x:thing/>"#.to_owned();
    let (doc, report) = d
        .page(concat!(
            r#"<draw:rect x:cell="2" draw:id="id7" svg:width="1cm" svg:height="1cm"><x:cells><x:c/></x:cells><svg:title>T</svg:title><text:p>a<text:a xlink:href="u">b</text:a></text:p></draw:rect>"#,
            r#"<draw:custom-shape svg:width="1cm" svg:height="1cm"/><draw:connector/><draw:g><draw:rect/></draw:g><x:page-child/>"#
        ))
        .read();
    let s = only(&doc);
    // Attributes the reader does not map: in the shape's `raw`, as written, and counted.
    assert_eq!(odf(s).raw.get("x:cell"), Some("2"));
    assert_eq!(odf(s).raw.get("draw:id"), Some("id7"));
    assert_eq!(report.kept("attribute"), 2);
    assert_eq!(report.kept("foreign_attribute"), 1);
    // Children it does not read: in the shape's passthrough, and counted.
    // (The title is a row of the User section, and no kept child.)
    assert_eq!(odf(s).kept.len(), 1);
    assert_eq!((report.kept("shape_child"), report.kept("title")), (1, 0));
    // A hyperlink in text: its text read, the element counted.
    assert_eq!(report.kept("text_element"), 1);
    // Custom shapes, connectors and groups are modelled; an unknown page child is in the page's passthrough, and counted.
    assert_eq!(page(&doc, 0).shapes.len(), 4);
    assert_eq!(odf_page(page(&doc, 0)).kept.len(), 1);
    assert_eq!((report.kept("custom_shape"), report.kept("connector"), report.kept("group"), report.kept("page_child")), (0, 0, 0, 1));
    // Style markup with no cell: counted (its bytes stay in the file the caller keeps).
    assert_eq!(report.kept("property"), 2);
    assert_eq!(report.kept("styles_element"), 1);
    assert_eq!(report.stats.kept_shapes, 0);
}

#[test]
fn odg_package_the_reader_refuses_what_holds_no_drawing() {
    let class = |bytes: &[u8]| read(bytes, &Limits::default()).err().map(|e| e.class());
    // No content.xml.
    let d = Drawing::a4().page("");
    let no_content: Vec<(String, Vec<u8>)> = d.files().into_iter().filter(|(n, _)| n != "content.xml").collect();
    assert_eq!(class(&crate::build::package(DRAWING, no_content)), Some("no_drawing_content"));
    // A root that is not the part's fixed one.
    let wrong_root = crate::build::replace(d.files(), "content.xml", format!("<office:document-styles {NS}/>").into_bytes());
    assert_eq!(class(&crate::build::package(DRAWING, wrong_root)), Some("xml_malformed"));
    let wrong_root = crate::build::replace(d.files(), "styles.xml", format!("<office:document-content {NS}/>").into_bytes());
    assert_eq!(class(&crate::build::package(DRAWING, wrong_root)), Some("xml_malformed"));
    // A styles part that does not parse refuses the drawing too.
    let broken = crate::build::replace(d.files(), "styles.xml", b"<office:document-styles".to_vec());
    assert_eq!(class(&crate::build::package(DRAWING, broken)), Some("xml_malformed"));
    // A presentation's body, and a presentation's media type.
    let body = d.content_xml().replace("<office:drawing>", "<office:presentation>").replace("</office:drawing>", "</office:presentation>");
    assert_eq!(class(&crate::build::package(DRAWING, crate::build::replace(d.files(), "content.xml", body.into_bytes()))), Some("not_a_drawing"));
    assert_eq!(class(&crate::build::package(crate::build::PRESENTATION, d.files())), Some("not_a_drawing"));
    // A body with neither: no drawing.
    let body = d.content_xml().replace("<office:drawing>", "<x:graphics xmlns:x=\"urn:example:x\">").replace("</office:drawing>", "</x:graphics>");
    assert_eq!(
        class(&crate::build::package(DRAWING, crate::build::replace(d.files(), "content.xml", body.into_bytes()))),
        Some("no_drawing_content")
    );
    // A template reads, and says so.
    let (_, report) = read(&crate::build::package(crate::build::TEMPLATE, d.files()), &Limits::default()).unwrap();
    assert_eq!(report.variant, Some(diacraft_odg::Variant::Template));
}

#[test]
fn shape_index_every_kept_kind_is_numbered_and_counted() {
    // A hyperlink wrapper is the one kind still kept whole: its child is numbered after it. A custom shape holds no shapes, so the
    // rectangle inside it is not numbered; the objects are modelled as `Foreign` shapes and counted by kind.
    let (doc, report) = Drawing::a4()
        .page(concat!(
            r#"<draw:measure/><draw:caption/><draw:control/><dr3d:scene/>"#,
            r#"<draw:a xlink:href="u"><draw:rect svg:width="1cm" svg:height="1cm"/></draw:a>"#,
            r#"<draw:custom-shape><draw:rect svg:width="1cm" svg:height="1cm"/></draw:custom-shape>"#
        ))
        .read();
    for kind in ["measure", "caption", "control", "scene_3d", "hyperlink_wrapper"] {
        assert_eq!(report.kept(kind), 1, "{kind}");
    }
    assert_eq!(page(&doc, 0).shapes.len(), 5, "the four objects and the custom shape");
    let index = report.index.page(0).expect("page");
    assert_eq!(index.kept(), [5, 6], "the wrapper and its child");
    assert_eq!(index.modelled(), 5);
    assert_eq!(index.high_water(), 7);
    assert_eq!(report.stats.max_shapes_in_a_page, 7);
}

#[test]
fn odg_package_the_message_of_no_drawing_content_fits_the_case() {
    let message = |bytes: &[u8]| read(bytes, &Limits::default()).err().map(|e| e.to_string());
    let d = Drawing::a4().page("");
    let no_content: Vec<(String, Vec<u8>)> = d.files().into_iter().filter(|(n, _)| n != "content.xml").collect();
    let no_part = message(&crate::build::package(DRAWING, no_content)).unwrap();
    assert!(no_part.contains("no content.xml"), "{no_part}");
    // A content.xml whose body holds no drawing is not a package with no content.xml.
    let body = d.content_xml().replace("<office:drawing>", "<x:graphics xmlns:x=\"urn:example:x\">").replace("</office:drawing>", "</x:graphics>");
    let no_body = message(&crate::build::package(DRAWING, crate::build::replace(d.files(), "content.xml", body.into_bytes()))).unwrap();
    assert!(no_body.contains("office:drawing") && !no_body.contains("no content.xml"), "{no_body}");
    // No office:body at all reads the same as a body with nothing in it.
    let no_office_body = format!("<office:document-content {NS}/>");
    let none = message(&crate::build::package(DRAWING, crate::build::replace(d.files(), "content.xml", no_office_body.into_bytes()))).unwrap();
    assert_eq!(none, no_body);
}
