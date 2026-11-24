//! The VSDX model writer (the native format's design, sections 4.1, 4.2, 4.4 and 4.5). `write_with(doc, None, options)` writes a drawing from the model alone: the
//! parts and relationships the model holds beside its cells (`Document::parts`, `Document::vsdx`), the part names and relationship ids of pages and masters, the elements and
//! attributes of every place the reader types and gives no field to, the document properties and `windows.xml`.
//!
//! **What "the same content" is.** Every visit here goes through the real reader (`read`) and the real content check (`compare_content`, design section 4.3): a package is read, written
//! from the model, and compared with the package it came from. **What a model writer cannot say** is stated where it matters: the choice of a `Default` row for a part's type against an
//! `Override` is not recorded in the model (the report, item 7), so a part is typed as packages of the format type theirs (pictures and `.bin` parts
//! by `Default`, the format's own parts by `Override`) and a `Default` row that no part relied on is held, which is the information that every part of its extension had an `Override`.
//! The fixtures below are typed that way.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_doc::{
    CellAddr, CellEdit, Document, ForeignData, Format, Media, Owner, Part, PartRole, Rel, Shape, ShapeRef, SheetRef, Value, WriteOrigin,
};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::model::{fixture_documents, rich_vsdx_bytes, rich_vsdx_original_pkg, rich_vsdx_pkg};
use diacraft_testkit::package::{NS_2012, NS_R, OD, PK, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{Kind, Limits, OriginalBytes, Variant, WriteError, WriteOptions, Written, compare_content, read, write_with};

use crate::drops::Kind as Injected;
use crate::held::{PLACES, inject};
use crate::read_ok;

pub(crate) const NOW: i64 = 1_700_000_000;
pub(crate) const DOC: &str = "/visio/document.xml";
pub(crate) const PAGE1: &str = "/visio/pages/page1.xml";
pub(crate) const CT_OLE: &str = "application/vnd.openxmlformats-officedocument.oleObject";
pub(crate) const MS: &str = "http://schemas.microsoft.com/office/2006/relationships/"; // brand-ok: a URI the format defines
pub(crate) const SIG: &str = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/";

// ----------------------------------------------------------------------------------------------------------------------------- helpers

pub(crate) fn options(unchanged: bool) -> WriteOptions {
    WriteOptions { now: Some(NOW), content_unchanged: unchanged, ..WriteOptions::default() }
}

pub(crate) fn try_write(doc: &Document, opts: &WriteOptions) -> Result<Written, WriteError> {
    write_with(doc, None, opts)
}

/// A model save, which must succeed.
pub(crate) fn model_write(doc: &Document, unchanged: bool) -> Written {
    write_with(doc, None, &options(unchanged)).unwrap_or_else(|e| panic!("the model writer refused: {e} ({})", e.class()))
}

/// The variant of a drawing that allows macros: what a package that holds a macro project is written as.
pub(crate) fn macro_drawing() -> Variant {
    Variant { kind: Kind::Drawing, macro_enabled: true }
}

/// A model save as `variant`, which must succeed.
pub(crate) fn model_write_as(doc: &Document, unchanged: bool, variant: Variant) -> Written {
    write_with(doc, None, &WriteOptions { variant: Some(variant), ..options(unchanged) })
        .unwrap_or_else(|e| panic!("the model writer refused: {e} ({})", e.class()))
}

pub(crate) fn diff(a: &[u8], b: &[u8]) -> ContentDiff {
    compare_content(a, b, &Limits::default()).unwrap_or_else(|e| panic!("the content check refused: {e}"))
}

pub(crate) fn show(d: &ContentDiff) -> String {
    d.shown.iter().take(12).map(|x| format!("{} {} {:?}", x.part, x.path, x.found)).collect::<Vec<_>>().join("\n")
}

/// The package read, written from the model with `content_unchanged`, and compared with itself.
pub(crate) fn visit(name: &str, bytes: &[u8]) -> Written {
    let (doc, report) = read_ok(bytes);
    // The type the file had is the one it is written as, as a caller names it by the extension of the file.
    let written = model_write_as(&doc, true, report.variant.unwrap_or(Variant { kind: Kind::Drawing, macro_enabled: false }));
    let d = diff(bytes, &written.bytes);
    assert!(d.same(), "{name}: the visit changed the content ({} differences):\n{}\n{}", d.count, show(&d), rows_difference(bytes, &written.bytes));
    written
}

/// The rows of the content types that only one of the two packages has, for a message.
fn rows_difference(a: &[u8], b: &[u8]) -> String {
    let rows = |bytes: &[u8]| -> BTreeSet<String> {
        let pkg = package(bytes);
        let types = pkg.content_types();
        types
            .defaults
            .iter()
            .map(|(e, t)| format!("Default {e} {t}"))
            .chain(types.overrides.iter().map(|(p, t)| format!("Override {p} {t}")))
            .collect()
    };
    let (x, y) = (rows(a), rows(b));
    format!("only in the file: {:?}\nonly in the written: {:?}", x.difference(&y).collect::<Vec<_>>(), y.difference(&x).collect::<Vec<_>>())
}

pub(crate) fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

pub(crate) fn text_of(bytes: &[u8], name: &str) -> String {
    String::from_utf8(parts(bytes).get(name).unwrap_or_else(|| panic!("no part {name}")).clone()).unwrap()
}

pub(crate) fn package(bytes: &[u8]) -> Package {
    Package::read(bytes, &OpcLimits::default()).unwrap()
}

pub(crate) fn page_cell(doc: &mut Document, page: u32, shape_at: u32, name: &str, value: &str) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(page), shape_at));
    doc.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing).unwrap();
}

pub(crate) fn names_folded(bytes: &[u8]) -> Vec<String> {
    let pkg = package(bytes);
    let mut names: Vec<String> = pkg.parts().map(|n| n.to_ascii_lowercase()).collect();
    names.sort();
    names
}

// --------------------------------------------------------------------------------------------------------------------------- fixtures

/// A drawing with two pages, a master, an unknown part typed by a `Default` row, a picture and a thumbnail typed by theirs, an unused `Default`, and a relationship id that is no
/// builder's: what a package of another writer has beside the model's own parts.
pub(crate) fn package_with_extras() -> Vec<u8> {
    let page_shapes = format!(
        "{}{}",
        shape_with(
            5,
            " x:mine=\"1\" xmlns:x=\"urn:example\" Master=\"1\"",
            &format!(
                "<x:note>hi</x:note>{}{}<?keep me?><!-- note -->{}",
                cell("PinX", "1"),
                r#"<Cell N="Width" V="1.50" F="Height*1.5" U="MM"/>"#,
                r#"<Cell N="LineColor" V="0" F="Inh"/>"#
            )
        ),
        shape(6, &cell("PinX", "2"))
    );
    VsdxBuilder::new()
        .page(
            PageSpec::new(1, "First")
                .shapes(&page_shapes)
                .rel("rId9", &format!("{OD}image"), "../media/image1.png")
                // The page uses master 1, and has the row to it that a file has for every master a page uses: a model save rebuilds it.
                .rel("rId10", &format!("{VIS}master"), "../masters/master1.xml"),
        )
        .page(PageSpec::new(2, "Other").shapes(&shape(1, &cell("PinX", "9"))))
        .master(diacraft_testkit::package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .with(
            Pkg::new()
                .part_without_override("/foo/bar.dat", b"0123456789")
                .default_type("dat", "application/x-foo")
                .default_type("zzz", "application/x-z")
                .default_type("png", "image/png")
                .part_without_override("/visio/media/image1.png", b"\x89PNG.fake")
                .rel("/", "rId7", "urn:example:foo", "foo/bar.dat")
                .part_without_override("/docProps/thumbnail.emf", b"emf-bytes")
                .default_type("emf", "image/x-emf")
                .rel("/", "rId8", &format!("{PK}thumbnail"), "docProps/thumbnail.emf"),
        )
        .build()
}

/// The package of [`everything_package`] (every kind of part the design's section 4.2 lists), typed as a package of the format types its parts: the parts that
/// [`everything_package`] gives an `Override` of a type a `Default` can give (a custom XML item, the ribbon part, the signature origin) rely on the `Default`
/// of their extension, as and the format's own packages do.
pub(crate) fn held_package() -> Vec<u8> {
    let foreign = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape><Shape ID="2" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let page = PageSpec::new(1, "P")
        .shapes(foreign)
        .rel("rId1", &format!("{OD}image"), "../media/i.png")
        .rel("rId2", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin")
        .rel("rId3", &format!("{OD}package"), "../embeddings/Drawing1.vsdx");
    let extra = Pkg::new()
        .default_type("png", "image/png")
        .default_type("bin", "application/octet-stream")
        .default_type("emf", "image/x-emf")
        .default_type("dat", "application/octet-stream")
        .default_type("sigs", "application/vnd.openxmlformats-package.digital-signature-origin")
        .part("/visio/media/i.png", "image/png", [1u8, 2, 3])
        .part("/visio/media/image2.emf", "image/x-emf", [9u8, 9])
        .part("/visio/embeddings/oleObject1.bin", CT_OLE, [0xD0u8, 0xCF, 0x11, 0xE0])
        .rel("/visio/embeddings/oleObject1.bin", "rId1", &format!("{OD}image"), "../media/image2.emf")
        .part_without_override("/visio/embeddings/Drawing1.vsdx", [b'P', b'K', 3, 4])
        .default_type("vsdx", "application/vnd.ms-visio.drawing") // brand-ok: a content type the format defines
        .part_without_override("/extra/blob.dat", [5u8, 6, 7])
        .part_without_override("/customXml/item1.xml", "<root/>")
        .part(
            "/customXml/itemProps1.xml",
            "application/vnd.openxmlformats-officedocument.customXmlProperties+xml",
            "<ds:datastoreItem xmlns:ds=\"urn:ds\"/>",
        )
        .rel(DOC, "rId90", &format!("{OD}customXml"), "../customXml/item1.xml")
        .rel("/customXml/item1.xml", "rId1", &format!("{OD}customXmlProps"), "itemProps1.xml")
        .part("/visio/comments.xml", "application/vnd.ms-visio.comments+xml", "<Comments xmlns=\"urn:c\"/>") // brand-ok: a content type the format defines
        .rel(DOC, "rId91", &format!("{VIS}comments"), "comments.xml")
        .part("/visio/data/connections.xml", "application/vnd.ms-visio.connections+xml", "<DataConnections/>") // brand-ok: a content type the format defines
        .rel(DOC, "rId92", &format!("{VIS}connections"), "data/connections.xml")
        .part("/visio/data/recordsets.xml", "application/vnd.ms-visio.recordsets+xml", "<DataRecordSets/>") // brand-ok: a content type the format defines
        .rel(DOC, "rId93", &format!("{VIS}recordsets"), "data/recordsets.xml")
        .part("/visio/data/recordset1.xml", "application/vnd.ms-visio.recordset+xml", "<rowset/>") // brand-ok: a content type the format defines
        .rel("/visio/data/recordsets.xml", "rId1", &format!("{VIS}recordset"), "recordset1.xml")
        .part("/visio/validation.xml", "application/vnd.ms-visio.validation+xml", "<Validation/>") // brand-ok: a content type the format defines
        .rel(DOC, "rId94", &format!("{VIS}validation"), "validation.xml")
        .part("/visio/extensions.xml", "application/vnd.ms-visio.extensions+xml", "<Extensions/>") // brand-ok: a content type the format defines
        .rel(DOC, "rId95", &format!("{VIS}extensions"), "extensions.xml")
        .part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", [1u8, 1, 1])
        .rel(DOC, "rId96", &format!("{MS}vbaProject"), "vbaProject.bin")
        .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", [2u8, 2])
        .rel("/visio/vbaProject.bin", "rId1", &format!("{MS}vbaProjectSignature"), "vbaProjectSignature.bin")
        .part("/docProps/thumbnail.emf", "image/x-emf", [3u8, 3, 3, 3])
        .rel("/", "rId10", &format!("{PK}thumbnail"), "docProps/thumbnail.emf")
        .part_without_override("/visio/customUI/customUI1.xml", "<customUI/>")
        .rel("/", "rId11", "http://schemas.microsoft.com/office/2007/relationships/ui/extensibility", "visio/customUI/customUI1.xml") // brand-ok: a URI the format defines
        .part_without_override("/_xmlsignatures/origin.sigs", [4u8])
        .rel("/", "rId12", &format!("{SIG}origin"), "_xmlsignatures/origin.sigs")
        .part("/_xmlsignatures/sig1.xml", "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml", "<Signature/>")
        .rel("/_xmlsignatures/origin.sigs", "rId1", &format!("{SIG}signature"), "sig1.xml");
    VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml") // brand-ok: a content type the format defines
        .page(page)
        .solution("OtherApp", "<SolutionXML Name=\"OtherApp\"><Data/></SolutionXML>")
        .with(extra)
        .build()
}

/// The package of [`places_builder`], its picture typed as a package of the format types its pictures: by the `Default` of its extension.
pub(crate) fn places_typed() -> VsdxBuilder {
    let shapes = r#"<Shape ID="1" Type="Group"><Text><cp IX="0"/>t</Text><Shapes><Shape ID="3"/></Shapes></Shape><Shape ID="2" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .face_names(r#"<FaceName NameU="Arial"/>"#)
        .master(diacraft_testkit::package::MasterSpec::new(3, "M").shapes(r#"<Shape ID="4"/>"#))
        .page(PageSpec::new(1, "P").shapes(shapes).connects(r#"<Connect FromSheet="1" FromCell="BeginX" ToSheet="2" ToCell="PinX"/>"#).rel(
            "rId1",
            &format!("{OD}image"),
            "../media/i.png",
        ))
        .with(Pkg::new().part_without_override("/visio/media/i.png", [1u8, 2, 3]).default_type("png", "image/png"))
}

/// A page of two shapes that a thumbnail, an application name and a time of modification belong to.
pub(crate) fn bookkeeping_package() -> Vec<u8> {
    let core = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>T</dc:title><dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2021-02-03T04:05:06Z</dcterms:modified></cp:coreProperties>";
    VsdxBuilder::new()
        .core(core)
        .page(PageSpec::new(1, "P").shapes(&format!(
            "{}{}",
            shape(1, &format!("{}{}", cell("PinX", "1"), cell("PinY", "2"))),
            shape(2, &cell("PinX", "5"))
        )))
        .with(Pkg::new().part_without_override("/docProps/thumbnail.emf", b"emf-bytes").default_type("emf", "image/x-emf").rel(
            "/",
            "rId8",
            &format!("{PK}thumbnail"),
            "docProps/thumbnail.emf",
        ))
        .build()
}

pub(crate) fn props_package() -> Vec<u8> {
    let core = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:x=\"urn:x\"><dc:title>T</dc:title><dc:subject>S</dc:subject><cp:contentStatus>Final</cp:contentStatus><!--c--><dc:creator>A</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-02T03:04:05Z</dcterms:created><x:extra k=\"v\">e</x:extra></cp:coreProperties>";
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application><Company>C</Company><Template>Normal.vst</Template><TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>P</vt:lpstr></vt:vector></TitlesOfParts></Properties>";
    let custom = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"2\" name=\"Name\"><vt:lpwstr>some text</vt:lpwstr></property><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"3\" name=\"Count\"><vt:i4>007</vt:i4></property><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"4\" name=\"Flag\"><vt:bool>true</vt:bool></property></Properties>";
    let extra = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    VsdxBuilder::new().core(core).app(app).with(extra).page(PageSpec::new(1, "P")).build()
}

fn windows_package() -> Vec<u8> {
    let windows = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Windows xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ClientWidth=\"1280\" ClientHeight=\"720\" xml:space=\"preserve\"><Window ID=\"0\" WindowType=\"Drawing\" WindowState=\"1073741824\" ViewScale=\"-1\" Page=\"1\"><ShowGrid>1</ShowGrid><!--kept--><Page ID=\"2\"/></Window><x:y xmlns:x=\"urn:x\"/></Windows>";
    let extra = Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", windows).rel(
        DOC,
        "rId30",
        &format!("{VIS}windows"),
        "windows.xml",
    ); // brand-ok: a content type the format defines
    VsdxBuilder::new().with(extra).page(PageSpec::new(1, "P")).build()
}

/// Pages and masters whose parts and relationships are named as no builder names them: a part in another case, a row that spells its target as an absolute name, ids that are not
/// the builder's, rows of the content types that name nothing.
fn names_package() -> Vec<u8> {
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"5\" Name=\"A\"><PageSheet/><Rel r:id=\"rId7\"/></Page><Page ID=\"6\" Name=\"B\"><PageSheet/><Rel r:id=\"rId8\"/></Page></Pages>"
    );
    let contents = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"1\"/></Shapes></PageContents>");
    let masters =
        format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Master ID=\"3\" NameU=\"M\"><PageSheet/><Rel r:id=\"rId9\"/></Master></Masters>");
    let mcontents = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"4\"/></Shapes></MasterContents>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/Page2.XML", diacraft_testkit::package::CT_PAGE, &contents)
        .part("/visio/other/zz.xml", diacraft_testkit::package::CT_PAGE, &contents)
        .rel("/visio/pages/pages.xml", "rId7", &format!("{VIS}page"), "Page2.XML")
        .rel("/visio/pages/pages.xml", "rId8", &format!("{VIS}page"), "/visio/other/zz.xml")
        .part("/visio/masters/masters.xml", diacraft_testkit::package::CT_MASTERS, masters)
        .rel(DOC, "rId51", &format!("{VIS}masters"), "masters/masters.xml")
        .part("/visio/masters/m-three.xml", diacraft_testkit::package::CT_MASTER, mcontents)
        .rel("/visio/masters/masters.xml", "rId9", &format!("{VIS}master"), "m-three.xml")
        .default_type("zzz", "application/x-unused")
        .override_without_part("/visio/gone.xml", "application/x-gone");
    VsdxBuilder::new().without_pages_part().without_masters_part().with(pkg).build()
}

// ------------------------------------------------------------------------------------------------------------------------ the visits

#[test]
fn a_model_writer_visit_has_the_same_content() {
    let plain = VsdxBuilder::new().page(PageSpec::new(1, "P")).build();
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("a drawing with one empty page", plain),
        ("two pages, a master, an unknown part, a picture, a thumbnail", package_with_extras()),
        ("every kind of part the design lists", held_package()),
        ("every kind of element the reader types", rich_vsdx_pkg(false).build()),
        ("document properties of the three parts", props_package()),
        ("windows.xml", windows_package()),
        ("part names, relationship ids and content-type rows", names_package()),
        ("the places the reader types and gives no field to", places_typed().build()),
    ];
    for (name, bytes) in &fixtures {
        visit(name, bytes);
    }
}

#[test]
fn the_original_layout_of_the_rich_package_is_written_with_only_the_spelling_the_model_holds_no_record_of() {
    // The first layout of the package: a picture typed by an `Override`, a page that uses masters and has no row to them, the fields of the core part in an order of their own, a text before
    // a `Rel`, a `Trigger` before an element of another vocabulary. The content check finds the rows of the content types (a `Default` and an `Override` that type one part: not recorded) and
    // the two rows a page of masters has, and nothing else.
    let original = rich_vsdx_original_pkg().build();
    let (doc, report) = read_ok(&original);
    let written = model_write_as(&doc, true, report.variant.unwrap_or(Variant { kind: Kind::Drawing, macro_enabled: false }));
    let d = diff(&original, &written.bytes);
    // (Its second page has no part, and a drawing's page has one: the part, its row and its `Rel` are the writer's.)
    let page_two = |x: &diacraft_container::canon::Difference| {
        x.part == "/visio/pages/page2.xml"
            || x.path.contains("Page[1]/Rel")
            || (x.part == "/visio/pages/_rels/pages.xml.rels" && x.path.contains("Relationship#1"))
    };
    let elsewhere: Vec<String> = d
        .shown
        .iter()
        .filter(|x| x.part != "[Content_Types].xml" && x.part != "/visio/pages/_rels/page1.xml.rels" && !page_two(x))
        .map(|x| format!("{} {} {:?}", x.part, x.path, x.found))
        .collect();
    assert!(elsewhere.is_empty(), "the content differs in {elsewhere:?}");
    let rows = rows_difference(&original, &written.bytes);
    assert!(rows.contains("Override /visio/media/image1.png") && rows.contains("Default png"), "{rows}");
}

#[test]
fn every_unheld_place_is_written_back_when_the_model_is() {
    let builder = places_typed();
    let mut checked = 0;
    for &(part, name, _, _, kinds) in PLACES {
        for &kind in kinds {
            let bytes = inject(&builder, part, name, kind, 0);
            visit(&format!("{part} <{name}> {kind:?}"), &bytes);
            checked += 1;
        }
    }
    assert!(checked >= 80, "{checked} injections");
}

#[test]
fn a_nested_group_keeps_the_leftovers_of_its_shapes_element() {
    let builder = places_typed();
    let bytes = inject(&builder, PAGE1, "Shapes", Injected::Comment, 1);
    visit("a comment in the shapes of a group", &bytes);
    let bytes = inject(&builder, PAGE1, "Shapes", Injected::Element, 1);
    visit("an element in the shapes of a group", &bytes);
}

#[test]
fn a_model_writer_visit_of_the_models_of_the_repository_reads_back_as_the_same_model() {
    // Each template the engine builds, the rich package as the reader gives it, a session's drawing: written from the model alone and read again.
    let mut checked = 0;
    let mut documents: Vec<(String, Document)> = fixture_documents().into_iter().filter(|(name, _)| name.starts_with("template ")).collect();
    // The rich package as the reader gives it, its second page with a part (a page with none is a stencil's).
    documents.push(("the rich package".to_owned(), read_ok(&rich_vsdx_pkg(false).build()).0));
    // The original layout of the package: the page uses masters 1 and 2 and has no row to either, which R22 of the masters spec says every such page has. A model save makes them, in the ids
    // it walks to, and the model it reads back has them: a package that does not hold the rows is not one the writer can give back as it was.
    let mut original = read_ok(&rich_vsdx_original_pkg().build()).0;
    let mut held = (**original.vsdx.as_ref().unwrap()).clone();
    for (id, target) in [("rId2", "../masters/master1.xml"), ("rId3", "../masters/master2.xml")] {
        held.rels.push(Rel {
            source: Arc::from(PAGE1),
            id: Arc::from(id),
            kind: Arc::from(format!("{VIS}master")),
            target: Arc::from(target),
            external: false,
        });
    }
    original.vsdx = Some(Arc::new(held));
    documents.push(("the rich package as it was first built".to_owned(), original));
    for (name, doc) in documents {
        let variant = if name.starts_with("template ") {
            Variant { kind: Kind::Template, macro_enabled: false }
        } else if name.contains("first built") {
            // Its second page has no part, which only a stencil's pages are seen without.
            Variant { kind: Kind::Stencil, macro_enabled: false }
        } else {
            Variant { kind: Kind::Drawing, macro_enabled: false }
        };
        let written = write_with(&doc, None, &WriteOptions { variant: Some(variant), content_unchanged: true, ..WriteOptions::default() })
            .unwrap_or_else(|e| panic!("{name}: {e} ({})", e.class()));
        let (back, _) = read_ok(&written.bytes);
        let mut want = doc.clone();
        // What a save may change of a model: nothing here (the clock is not given and the content is unchanged).
        want.props = back.props.clone();
        assert_eq!(back.pages.len(), want.pages.len(), "{name}");
        for (a, b) in back.pages.iter().zip(want.pages.iter()) {
            assert_eq!(a, b, "{name}: page {}", b.id);
        }
        assert_eq!(back.masters.len(), want.masters.len(), "{name}");
        for (a, b) in back.masters.iter().zip(want.masters.iter()) {
            assert_eq!(a, b, "{name}: master {}", b.id);
        }
        assert_eq!(back.styles, want.styles, "{name}: styles");
        assert_eq!(back.sheet, want.sheet, "{name}: sheet");
        assert_eq!(back.colors, want.colors, "{name}: colours");
        assert_eq!(back.fonts, want.fonts, "{name}: fonts");
        assert_eq!(back.passthrough, want.passthrough, "{name}: unknown elements of the document");
        assert_eq!(back.ext, want.ext, "{name}: what the document's slot holds");
        assert_eq!(back.themes.len(), want.themes.len(), "{name}: themes");
        assert_eq!(back.parts.len(), want.parts.len(), "{name}: held parts");
        assert_eq!(back.vsdx, want.vsdx, "{name}: what the document holds of the package");
        checked += 1;
    }
    assert!(checked >= 3, "{checked} documents");
}

// -------------------------------------------------------------------------------------------------------------- the edited save

#[test]
fn an_edited_model_save_changes_only_the_edit_and_the_bookkeeping() {
    let original = bookkeeping_package();
    let (mut doc, _) = read_ok(&original);
    page_cell(&mut doc, 1, 0, "PinX", "7");
    page_cell(&mut doc, 1, 0, "PinY", "8");
    let written = model_write(&doc, false);
    // The same set of parts but for the thumbnail, which an edit makes stale; its `Default` row stays.
    let before = parts(&original);
    let after = parts(&written.bytes);
    let gone: Vec<&String> = before.keys().filter(|k| !after.contains_key(*k)).collect();
    assert_eq!(gone, ["/docProps/thumbnail.emf"]);
    assert_eq!(after.keys().filter(|k| !before.contains_key(*k)).count(), 0);
    let pkg = package(&written.bytes);
    assert!(pkg.content_types().defaults.iter().any(|(e, t)| e == "emf" && t == "image/x-emf"), "the thumbnail's Default stays");
    assert!(package(&written.bytes).rels("/").unwrap().items.iter().all(|r| !r.ty.ends_with("thumbnail")), "and its relationship goes");
    // The content differs at the edit, `modified`, `Application`, the thumbnail's part and its relationship, and nowhere else.
    let d = diff(&original, &written.bytes);
    let mut found: Vec<(String, Found)> = d.shown.iter().map(|x| (x.part.clone(), x.found)).collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then((a.1 as u8).cmp(&(b.1 as u8))));
    let mut want = vec![
        ("/_rels/.rels".to_owned(), Found::Element),
        ("/docProps/app.xml".to_owned(), Found::Text),
        ("/docProps/core.xml".to_owned(), Found::Text),
        ("/docProps/thumbnail.emf".to_owned(), Found::MissingPart),
        (PAGE1.to_owned(), Found::Value),
        (PAGE1.to_owned(), Found::Value),
    ];
    want.sort_by(|a, b| a.0.cmp(&b.0).then((a.1 as u8).cmp(&(b.1 as u8))));
    assert_eq!(found, want, "\n{}", show(&d));
    let page = text_of(&written.bytes, PAGE1);
    assert!(page.contains("V=\"7\"") && page.contains("V=\"8\""), "{page}");
    let core = text_of(&written.bytes, "/docProps/core.xml");
    assert!(
        core.contains("2023-11-14T22:13:20Z") && core.contains("2020-01-02T03:04:05Z"),
        "modified is the save's and created is the file's: {core}"
    );
    assert!(text_of(&written.bytes, "/docProps/app.xml").contains("<Application>DiaCraft</Application>"));
}

#[test]
fn an_unchanged_model_keeps_the_bookkeeping_it_read() {
    let original = bookkeeping_package();
    let (doc, _) = read_ok(&original);
    let written = model_write(&doc, true);
    let before = parts(&original);
    let after = parts(&written.bytes);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "the thumbnail is kept");
    let core = text_of(&written.bytes, "/docProps/core.xml");
    assert!(core.contains("2021-02-03T04:05:06Z") && !core.contains("2023-11-14"), "modified is as it was read: {core}");
    assert!(diff(&original, &written.bytes).same());
}

// -------------------------------------------------------------------------------------------------------------------- the page names

/// A drawing of the pages `pages` and one master whose `app.xml` lists them under labels in another language.
fn titled_package(pages: &[&str], titles: &str, counts: (usize, usize)) -> Vec<u8> {
    let app = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Another Drawing App</Application><HeadingPairs><vt:vector size=\"4\" baseType=\"variant\"><vt:variant><vt:lpstr>Seiten</vt:lpstr></vt:variant><vt:variant><vt:i4>{}</vt:i4></vt:variant><vt:variant><vt:lpstr>Masters</vt:lpstr></vt:variant><vt:variant><vt:i4>{}</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts>{titles}</TitlesOfParts></Properties>",
        counts.0, counts.1
    );
    let mut builder =
        VsdxBuilder::new().app(&app).master(diacraft_testkit::package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))));
    for (i, name) in pages.iter().enumerate() {
        builder = builder.page(PageSpec::new(u32::try_from(i + 1).unwrap(), name).shapes(&shape(1, &cell("PinX", "1"))));
    }
    builder.build()
}

pub(crate) fn lpstr_vector(names: &[&str]) -> String {
    let items: String = names.iter().map(|n| format!("<vt:lpstr>{n}</vt:lpstr>")).collect();
    format!("<vt:vector size=\"{}\" baseType=\"lpstr\">{items}</vt:vector>", names.len())
}

/// The names `TitlesOfParts` of the file's `app.xml` lists.
pub(crate) fn listed(bytes: &[u8]) -> Vec<String> {
    let app = text_of(bytes, "/docProps/app.xml");
    let titles = app.split("<TitlesOfParts>").nth(1).and_then(|t| t.split("</TitlesOfParts>").next()).unwrap_or("");
    titles.split("<vt:lpstr>").skip(1).filter_map(|t| t.split("</vt:lpstr>").next()).map(str::to_owned).collect()
}

#[test]
fn the_page_names_of_app_xml_follow_the_pages_of_an_edited_model_save_and_only_those() {
    let original = titled_package(&["A"], &lpstr_vector(&["A", "Box"]), (1, 1));
    // Renamed.
    let (mut doc, _) = read_ok(&original);
    doc.rename_page(1, "B").unwrap();
    let written = model_write(&doc, false);
    assert_eq!(listed(&written.bytes), ["B", "Box"]);
    let app = text_of(&written.bytes, "/docProps/app.xml");
    assert!(app.contains("<vt:lpstr>Seiten</vt:lpstr>") && app.contains("<vt:lpstr>Masters</vt:lpstr>"), "the labels are the user's and stay: {app}");
    assert!(app.contains("<Application>DiaCraft</Application>"), "{app}");
    // Added, and removed.
    let (mut doc, _) = read_ok(&original);
    doc.add_page(Some("C")).unwrap();
    let added = model_write(&doc, false);
    assert_eq!(listed(&added.bytes), ["A", "C", "Box"]);
    assert!(text_of(&added.bytes, "/docProps/app.xml").contains("<vt:i4>2</vt:i4>"));
    let (mut doc, _) = read_ok(&titled_package(&["A", "B"], &lpstr_vector(&["A", "B", "Box"]), (2, 1)));
    doc.pages.remove(0).unwrap();
    let removed = model_write(&doc, false);
    assert_eq!(listed(&removed.bytes), ["B", "Box"]);
    assert!(text_of(&removed.bytes, "/docProps/app.xml").contains("<vt:i4>1</vt:i4>"));
    // A drawing whose pages are as the file said is written as the file had it, and so is a save of an unchanged model.
    let (doc, _) = read_ok(&original);
    assert_eq!(listed(&model_write(&doc, false).bytes), ["A", "Box"]);
    let kept = model_write(&doc, true);
    assert!(diff(&original, &kept.bytes).same());
}

#[test]
fn lists_of_names_that_cannot_be_told_apart_are_left_and_the_save_says_so() {
    // The counts do not add up to the titles: nothing says which names are the pages'.
    let original = titled_package(&["A"], &lpstr_vector(&["A", "Box"]), (5, 1));
    let (mut doc, _) = read_ok(&original);
    doc.rename_page(1, "B").unwrap();
    let written = model_write(&doc, false);
    assert_eq!(listed(&written.bytes), ["A", "Box"]);
    assert!(written.warnings.iter().any(|w| w.contains("list of page names")), "{:?}", written.warnings);
}

/// A drawing of the page `A` and one master, `Box`, whose `app.xml` lists the pages in one group and no other, as a template the writer made from nothing does.
fn one_group_package() -> Vec<u8> {
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>DiaCraft</Application><HeadingPairs><vt:vector size=\"2\" baseType=\"variant\"><vt:variant><vt:lpstr>Pages</vt:lpstr></vt:variant><vt:variant><vt:i4>1</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>A</vt:lpstr></vt:vector></TitlesOfParts></Properties>";
    VsdxBuilder::new()
        .app(app)
        .master(diacraft_testkit::package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .build()
}

/// **A new document's lists are its own**: a drawing with no file yet (`WriteOptions::new_copy`: a drawing made from a template keeps no bytes of it, so the engine writes it
/// from the model) that has gained pages and masters since the template named its pages is given lists of the model's pages and masters, and not the warning that the lists were left as the
/// template had them, which a person who only made a drawing from a template and saved it as a VSDX type was told for nothing. A file that is not a new document is still left, and still says so.
#[test]
fn a_new_documents_lists_of_names_are_made_from_the_model_where_the_templates_cannot_be_told() {
    let original = one_group_package();
    let (mut doc, _) = read_ok(&original);
    doc.rename_page(1, "B").unwrap();
    // A save into the file is not a new document: the one group could be the pages' and the masters' both, so it is left, and the save says so.
    let left = model_write(&doc, false);
    assert_eq!(listed(&left.bytes), ["A"]);
    assert!(left.warnings.iter().any(|w| w.contains("list of page names")), "{:?}", left.warnings);
    // A new document is given lists of its own: the pages in one group and the masters in the other, each with its count, and nothing is said.
    let copy = try_write(&doc, &WriteOptions { new_copy: true, ..options(false) }).expect("a new document is written");
    assert_eq!(copy.warnings, Vec::<String>::new());
    assert_eq!(listed(&copy.bytes), ["B", "Box"]);
    let app = text_of(&copy.bytes, "/docProps/app.xml");
    assert!(app.contains("<Application>DiaCraft</Application>"), "{app}");
    assert_eq!(
        app.matches("<HeadingPairs>").count() + app.matches("<TitlesOfParts>").count(),
        2,
        "one list of each, not the template's beside the new: {app}"
    );
    assert!(read_ok(&copy.bytes).1.warnings.is_empty());
    // Where the template's lists can be told, a new document follows the same rule as any save (the pages' group is renamed in place).
    let (mut named, _) = read_ok(&titled_package(&["A"], &lpstr_vector(&["A", "Box"]), (1, 1)));
    named.rename_page(1, "B").unwrap();
    let kept = try_write(&named, &WriteOptions { new_copy: true, ..options(false) }).unwrap();
    assert_eq!(listed(&kept.bytes), ["B", "Box"]);
    assert!(text_of(&kept.bytes, "/docProps/app.xml").contains("<vt:lpstr>Seiten</vt:lpstr>"), "the template's labels stay where they could be told");
}

// ----------------------------------------------------------------------------------------------------------------- macros and embeddings

#[test]
fn a_vba_project_goes_only_into_macro_enabled_types() {
    let bytes = held_package();
    let (doc, _) = read_ok(&bytes);
    // A type that cannot hold one refuses, in words, and writes nothing.
    let plain = WriteOptions { variant: Some(Variant { kind: Kind::Drawing, macro_enabled: false }), ..options(true) };
    let refused = try_write(&doc, &plain).expect_err("a macro project in a .vsdx");
    assert_eq!(refused.class(), "macro_project_dropped");
    assert!(refused.to_string().contains("macro project"), "{refused}");
    // The default type is a `.vsdx`.
    assert_eq!(try_write(&doc, &options(true)).expect_err("the default type cannot hold macros").class(), "macro_project_dropped");
    // A macro-enabled type keeps the project, its signature and the relationships, as bytes.
    for kind in [Kind::Drawing, Kind::Stencil, Kind::Template] {
        let macro_type = WriteOptions { variant: Some(Variant { kind, macro_enabled: true }), ..options(true) };
        let written = try_write(&doc, &macro_type).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        let p = parts(&written.bytes);
        assert_eq!(p.get("/visio/vbaProject.bin").map(Vec::as_slice), Some(&[1u8, 1, 1][..]));
        assert_eq!(p.get("/visio/vbaProjectSignature.bin").map(Vec::as_slice), Some(&[2u8, 2][..]));
        let pkg = package(&written.bytes);
        assert!(pkg.rels(DOC).unwrap().items.iter().any(|r| r.ty.ends_with("/vbaProject")));
        assert_eq!(pkg.content_type("/visio/vbaProject.bin"), Some("application/vnd.ms-office.vbaProject"));
        assert_eq!(read_ok(&written.bytes).1.variant, Some(Variant { kind, macro_enabled: true }));
    }
    // The caller's choice to leave it out: the project, its signature and the relationships to them go, with a warning that says so.
    let dropped = try_write(&doc, &WriteOptions { drop_macros: true, ..plain }).expect("the caller chose to drop the macros");
    let p = parts(&dropped.bytes);
    assert!(!p.contains_key("/visio/vbaProject.bin") && !p.contains_key("/visio/vbaProjectSignature.bin"));
    assert_eq!(dropped.macros_dropped.len(), 2, "{:?}", dropped.macros_dropped);
    assert!(dropped.warnings.iter().any(|w| w.contains("macro project was left out")), "{:?}", dropped.warnings);
    assert!(package(&dropped.bytes).rels(DOC).unwrap().items.iter().all(|r| !r.ty.ends_with("/vbaProject")));
    // Every other part is still there.
    assert!(p.contains_key("/visio/embeddings/oleObject1.bin") && p.contains_key("/extra/blob.dat"));
}

#[test]
fn vsdx_embeddings_are_written_natively() {
    // An embedded object and an embedded package are bytes, written under their names with the rows that name them, and the picture an object falls back to is the model's.
    let bytes = held_package();
    let (doc, _) = read_ok(&bytes);
    let written = model_write_as(&doc, true, macro_drawing());
    let p = parts(&written.bytes);
    assert_eq!(p.get("/visio/embeddings/oleObject1.bin").map(Vec::as_slice), Some(&[0xD0u8, 0xCF, 0x11, 0xE0][..]));
    assert_eq!(p.get("/visio/embeddings/Drawing1.vsdx").map(Vec::as_slice), Some(&[b'P', b'K', 3, 4][..]));
    assert_eq!(p.get("/visio/media/image2.emf").map(Vec::as_slice), Some(&[9u8, 9][..]), "the fallback picture of the object");
    let pkg = package(&written.bytes);
    let page_rels = pkg.rels(PAGE1).unwrap();
    assert_eq!(
        page_rels.get("rId2").map(|r| (r.ty.as_str(), r.target_raw.as_str())),
        Some((&*format!("{OD}oleObject"), "../embeddings/oleObject1.bin"))
    );
    assert_eq!(page_rels.get("rId3").map(|r| r.target_raw.as_str()), Some("../embeddings/Drawing1.vsdx"));
    assert_eq!(
        page_rels.get("rId1").map(|r| (r.ty.as_str(), r.target_raw.as_str())),
        Some((&*format!("{OD}image"), "../media/i.png")),
        "the picture's own row"
    );
    assert_eq!(pkg.rels("/visio/embeddings/oleObject1.bin").unwrap().get("rId1").map(|r| r.target_raw.as_str()), Some("../media/image2.emf"));
    // The model that is read back holds the same: the shapes show the same pictures, and the parts are the same.
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(report.warnings.len(), 0, "{:?}", report.warnings);
    let (orig, _) = read_ok(&bytes);
    let names = |d: &Document| d.parts.iter().map(|p| p.name.to_string()).collect::<BTreeSet<_>>();
    assert_eq!(names(&back), names(&orig));
    let pictures = |d: &Document| -> Vec<String> {
        d.pages.get(0).unwrap().shapes.iter().filter_map(|s| s.foreign.as_ref()?.picture.as_ref().map(|m| m.part.to_string())).collect()
    };
    assert_eq!(pictures(&back), pictures(&orig));
}

// ------------------------------------------------------------------------------------------------------------------------- properties

#[test]
fn title_and_property_changes_are_written() {
    let bytes = props_package();
    let (mut doc, _) = read_ok(&bytes);
    doc.title = Arc::from("New title & more");
    {
        let props = Arc::make_mut(&mut doc.props);
        props.subject = Some(Arc::from("Subject"));
        props.creator = None;
        props.company = Some(Arc::from("Company"));
        props.keywords = Some(Arc::from("k1, k2"));
        props.custom.push(diacraft_doc::CustomProp {
            format: Format::Vsdx,
            name: Arc::from("Added"),
            value_type: Arc::from("vt:lpwstr"),
            value: Arc::from("v"),
            raw: diacraft_doc::RawAttrs::from_pairs([("fmtid", "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"), ("pid", "5")]),
        });
    }
    let check = |written: &Written, how: &str| {
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(&*back.title, "New title & more", "{how}");
        assert_eq!(back.props.subject.as_deref(), Some("Subject"), "{how}");
        assert_eq!(back.props.creator, None, "{how}: a field taken away is taken out");
        assert_eq!(back.props.company.as_deref(), Some("C").map(|_| "Company"), "{how}");
        assert_eq!(back.props.keywords.as_deref(), Some("k1, k2"), "{how}");
        let custom: Vec<(String, String)> = back.props.custom.iter().map(|c| (c.name.to_string(), c.value.to_string())).collect();
        assert_eq!(
            custom,
            [
                ("Name".to_owned(), "some text".to_owned()),
                ("Count".to_owned(), "007".to_owned()),
                ("Flag".to_owned(), "true".to_owned()),
                ("Added".to_owned(), "v".to_owned())
            ],
            "{how}"
        );
        // What the model holds beside the fields is still there.
        let core = text_of(&written.bytes, "/docProps/core.xml");
        assert!(core.contains("contentStatus") && core.contains("<x:extra k=\"v\">e</x:extra>"), "{how}: {core}");
        let app = text_of(&written.bytes, "/docProps/app.xml");
        assert!(app.contains("<Template>Normal.vst</Template>"), "{how}: {app}");
    };
    // The model writer writes every field.
    check(&model_write(&doc, false), "the model writer");
    // A save into the file the drawing was read from writes them too (it refused a change to any of them but the title).
    let spliced = write_with(&doc, Some(&OriginalBytes { bytes: &bytes, baseline: None }), &options(false)).unwrap_or_else(|e| panic!("{e}"));
    check(&spliced, "the splice writer");
}

#[test]
fn the_unknown_elements_of_the_document_are_written_by_both_writers() {
    let bytes = VsdxBuilder::new()
        .document_extra(r#"<DocumentSettings TopPage="0"><GlueSettings>9</GlueSettings></DocumentSettings><EventList/>"#)
        .page(PageSpec::new(1, "P"))
        .build();
    let (mut doc, _) = read_ok(&bytes);
    // A drawing that was never saved and holds unknown elements of its document is written (it was refused).
    let written = model_write(&doc, true);
    let text = text_of(&written.bytes, DOC);
    assert!(text.contains("<DocumentSettings TopPage=\"0\">") && text.contains("<EventList/>"), "{text}");
    // A change to them, in a save into the file, is written too (it was refused).
    let mut list = doc.passthrough.nodes_mut().clone();
    list.push(diacraft_doc::Placed {
        after: 3,
        node: diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
            name: Arc::from("HeaderFooter"),
            attrs: diacraft_doc::RawAttrs::new(),
            children: vec![],
        }),
    });
    *doc.passthrough.nodes_mut() = list;
    let spliced = write_with(&doc, Some(&OriginalBytes { bytes: &bytes, baseline: None }), &options(false)).unwrap_or_else(|e| panic!("{e}"));
    let text = text_of(&spliced.bytes, DOC);
    assert!(text.contains("<HeaderFooter/>") && text.contains("<EventList/>"), "{text}");
    assert_eq!(read_ok(&spliced.bytes).0.passthrough, doc.passthrough);
}

// ----------------------------------------------------------------------------------------------------------------- templates, signatures

#[test]
fn every_built_in_templates_model_writes_with_no_original() {
    let mut count = 0;
    for (name, doc) in fixture_documents() {
        let Some(id) = name.strip_prefix("template ") else { continue };
        let variant = Variant { kind: Kind::Template, macro_enabled: false };
        let written = write_with(&doc, None, &WriteOptions { variant: Some(variant), ..WriteOptions::default() })
            .unwrap_or_else(|e| panic!("{id}: {e} ({})", e.class()));
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(report.variant, Some(variant), "{id}");
        assert_eq!(back.pages.len(), doc.pages.len(), "{id}");
        assert_eq!(back.themes.len(), doc.themes.len(), "{id}");
        assert_eq!(back.styles, doc.styles, "{id}");
        assert_eq!(back.passthrough, doc.passthrough, "{id}: the document settings of the template are written back");
        assert_eq!(back.diacraft, doc.diacraft, "{id}");
        count += 1;
    }
    assert!(count >= 2, "{count} templates");
}

#[test]
fn signature_parts_are_kept_and_the_save_says_they_no_longer_match() {
    let bytes = held_package();
    let (doc, _) = read_ok(&bytes);
    let written = model_write_as(&doc, true, macro_drawing());
    let p = parts(&written.bytes);
    assert_eq!(p.get("/_xmlsignatures/sig1.xml").map(Vec::as_slice), Some(&b"<Signature/>"[..]));
    assert_eq!(p.get("/_xmlsignatures/origin.sigs").map(Vec::as_slice), Some(&[4u8][..]));
    let pkg = package(&written.bytes);
    assert!(pkg.rels("/").unwrap().items.iter().any(|r| r.ty == format!("{SIG}origin")));
    assert!(written.warnings.iter().any(|w| w == "1 digital signature(s) in this file no longer match it."), "{:?}", written.warnings);
    // A file with none says nothing of them.
    let (plain, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P")).build());
    assert!(model_write(&plain, true).warnings.iter().all(|w| !w.contains("digital signature")));
}

// ---------------------------------------------------------------------------------------------- names, ids, pictures and held rows

/// A picture in a shape that names no relationship: what another format's reader makes.
pub(crate) fn new_picture_shape(id: u32, picture: &Arc<Media>) -> Shape {
    let attrs = diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG")]);
    let foreign = ForeignData { attrs, rel_id: None, picture: Some(Arc::clone(picture)), ..ForeignData::default() };
    Shape { id, kind: diacraft_doc::ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() }
}

pub(crate) fn picture(part: &str, bytes: &[u8]) -> Arc<Media> {
    Arc::new(Media { part: Arc::from(part), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })
}

pub(crate) fn add_page_with_picture(doc: &mut Document, media: &Arc<Media>) -> u32 {
    let id = doc.add_page(Some("Added")).unwrap();
    let slot = doc.pages.iter().position(|p| p.id == id).unwrap();
    let page = Arc::make_mut(doc.pages.get_mut(slot).unwrap());
    page.shapes.push(Arc::new(new_picture_shape(1, media)));
    id
}

#[test]
fn added_page_and_picture_never_reuse_a_held_name_or_rel_id() {
    // A held part named `Page2.XML` beside the page folder's parts, and a held relationship `rId3` of the document.
    let pkg = Pkg::new()
        .part("/visio/pages/Page2.XML", "application/x-held", [1u8, 2, 3])
        .part("/visio/media/image1.png", "image/png", [7u8, 7])
        .rel(DOC, "rId3", "urn:example:held", "../pages/Page2.XML")
        .rel(DOC, "rId4", "urn:example:held", "../media/image1.png");
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).with(pkg).build();
    let (mut doc, _) = read_ok(&bytes);
    assert!(doc.parts.iter().any(|p| p.name.eq_ignore_ascii_case("/visio/pages/page2.xml")), "the held part is the model's");
    let media = picture("not a part name of this format", &[0x89, b'P', b'N', b'G', 1, 2, 3, 4]);
    add_page_with_picture(&mut doc, &media);
    let written = model_write(&doc, false);
    // No two parts are one name, case folded.
    let names = names_folded(&written.bytes);
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(names.len(), unique.len(), "{names:?}");
    // Every relationship targets the part it did: the held rows are as written, and the rows made reach their own parts.
    let pkg = package(&written.bytes);
    let from_doc = pkg.rels(DOC).unwrap();
    assert_eq!(from_doc.get("rId3").map(|r| (r.ty.as_str(), r.target_raw.as_str())), Some(("urn:example:held", "../pages/Page2.XML")));
    assert_eq!(from_doc.get("rId4").map(|r| r.target_raw.as_str()), Some("../media/image1.png"));
    assert!(from_doc.items.iter().all(|r| r.id.eq_ignore_ascii_case("rId3") || r.id.eq_ignore_ascii_case("rId4") || !r.id.is_empty()));
    let ids: Vec<String> = from_doc.items.iter().map(|r| r.id.to_ascii_lowercase()).collect();
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len(), "no two rows of the document have one id: {ids:?}");
    // The real reader reads the page count and the picture's bytes back, and the held parts are the same.
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.pages.len(), 2, "{:?}", report.warnings);
    let added = back.pages.get(1).unwrap();
    let shown = added.shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.as_ref().expect("the picture is loaded");
    assert_eq!(&*shown.bytes, &[0x89, b'P', b'N', b'G', 1, 2, 3, 4]);
    assert_eq!(
        back.parts.iter().filter(|p| p.name.eq_ignore_ascii_case("/visio/pages/page2.xml")).map(|p| &*p.bytes).collect::<Vec<_>>(),
        [&[1u8, 2, 3][..]]
    );
    assert_eq!(back.pages.get(0).unwrap().part_name.as_deref(), Some(PAGE1));
}

#[test]
fn a_new_picture_without_a_rel_id_gets_a_media_part() {
    let mut doc = Document::new_blank();
    let media = picture("Pictures/100000000000000A0000000A.png", &[0x89, b'P', b'N', b'G', 9, 9]);
    let page = doc.pages.get(0).unwrap().id;
    let slot = doc.pages.iter().position(|p| p.id == page).unwrap();
    let page_mut = Arc::make_mut(doc.pages.get_mut(slot).unwrap());
    page_mut.shapes.push(Arc::new(new_picture_shape(1, &media)));
    // Two shapes that show one picture are one part.
    page_mut.shapes.push(Arc::new(new_picture_shape(2, &media)));
    // A document that came from no package has no relationships of its own: the writer makes them.
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    let written = model_write(&doc, false);
    let p = parts(&written.bytes);
    let images: Vec<&String> = p.keys().filter(|k| k.starts_with("/visio/media/")).collect();
    assert_eq!(images.len(), 1, "{images:?}");
    assert!(images[0].ends_with(".png"), "{images:?}");
    assert_eq!(p.get(images[0]).map(Vec::as_slice), Some(&[0x89, b'P', b'N', b'G', 9, 9][..]));
    let pkg = package(&written.bytes);
    assert_eq!(pkg.content_type(images[0]), Some("image/png"));
    let rels = pkg.rels(PAGE1).unwrap();
    assert_eq!(rels.of_type(&format!("{OD}image")).count(), 1);
    let (back, report) = read_ok(&written.bytes);
    assert!(report.warnings.iter().all(|w| !w.contains("picture")), "{:?}", report.warnings);
    for s in back.pages.get(0).unwrap().shapes.iter() {
        assert_eq!(s.foreign.as_ref().unwrap().picture.as_ref().map(|m| m.bytes.to_vec()), Some(vec![0x89, b'P', b'N', b'G', 9, 9]));
    }
}

#[test]
fn a_held_rel_from_a_deleted_page_is_dropped() {
    // Page 2 names an embedded object and a hyperlink that only it reaches, and a held rel row of its own.
    let page2 = PageSpec::new(2, "Second")
        .shapes(&shape(1, &cell("PinX", "1")))
        .rel("rId1", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin")
        .rel("rId2", "urn:example:keep", "../embeddings/shared.bin");
    let extra = Pkg::new().part("/visio/embeddings/oleObject1.bin", CT_OLE, [1u8]).part("/visio/embeddings/shared.bin", CT_OLE, [2u8]).rel(
        "/",
        "rId40",
        "urn:example:shared",
        "visio/embeddings/shared.bin",
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "First")).page(page2).with(extra).build();
    let (mut doc, _) = read_ok(&bytes);
    assert!(doc.vsdx.as_ref().unwrap().rels.iter().any(|r| &*r.source == "/visio/pages/page2.xml"));
    let id2 = doc.pages.get(1).unwrap().id;
    let at = doc.pages.iter().position(|p| p.id == id2).unwrap();
    doc.pages.remove(at).unwrap();
    let written = model_write(&doc, false);
    let p = parts(&written.bytes);
    assert!(!p.contains_key("/visio/pages/page2.xml") && !p.contains_key("/visio/pages/_rels/page2.xml.rels"), "{:?}", p.keys());
    // The embedded object only the deleted page reached goes with it; the one another row reaches stays.
    assert!(!p.contains_key("/visio/embeddings/oleObject1.bin"), "{:?}", p.keys());
    assert!(p.contains_key("/visio/embeddings/shared.bin"));
    // No relationship is left that comes from a part that is not there.
    let pkg = package(&written.bytes);
    for name in pkg.parts() {
        if let Some(source) = diacraft_opc::name::rels_source(name) {
            assert!(source == "/" || pkg.part(&source).is_some(), "{name} belongs to no part");
        }
    }
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 1);
}

#[test]
fn two_pages_that_name_one_part_are_written_one_part() {
    let shared = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"1\"/></Shapes></PageContents>");
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><PageSheet/><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><PageSheet/><Rel r:id=\"rId2\"/></Page></Pages>"
    );
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/one.xml", diacraft_testkit::package::CT_PAGE, shared)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "one.xml")
        .rel("/visio/pages/pages.xml", "rId2", &format!("{VIS}page"), "one.xml");
    let bytes = VsdxBuilder::new().without_pages_part().with(pkg).build();
    let written = visit("two pages that name one part", &bytes);
    assert_eq!(
        names_folded(&written.bytes).iter().filter(|n| n.starts_with("/visio/pages/") && !n.contains("_rels") && !n.ends_with("pages.xml")).count(),
        1
    );
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.pages.len(), 2);
    assert_eq!(back.pages.get(0).unwrap().part_name, back.pages.get(1).unwrap().part_name);
}

// ------------------------------------------------------------------------------------------ the lists of names, read as they are

/// `HeadingPairs` of these `(label, count)` groups, the counts as the text says them.
pub(crate) fn pairs(groups: &[(&str, &str)]) -> String {
    let items: String = groups
        .iter()
        .map(|(label, count)| format!("<vt:variant><vt:lpstr>{label}</vt:lpstr></vt:variant><vt:variant><vt:i4>{count}</vt:i4></vt:variant>"))
        .collect();
    format!("<HeadingPairs><vt:vector size=\"{}\" baseType=\"variant\">{items}</vt:vector></HeadingPairs>", groups.len() * 2)
}

/// A drawing of the page `A` and the master `Box`, whose `app.xml` holds the lists given (`heading` and `titles` are whole elements, in the order they are written).
pub(crate) fn app_with(heading: &str, titles: &str) -> Vec<u8> {
    let app = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Another Drawing App</Application>{heading}{titles}</Properties>"
    );
    VsdxBuilder::new()
        .app(&app)
        .master(diacraft_testkit::package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .build()
}

#[test]
fn only_the_first_list_of_titles_is_rewritten_and_a_second_that_holds_none_is_left_as_it_is() {
    // The groups say two titles, and the first list holds them. A second list of titles that holds none is not the one the groups were counted against: it is not rewritten (it
    // was, by counts that do not fit it, which is a subtraction that goes below zero).
    let titles = format!("<TitlesOfParts>{}</TitlesOfParts>", lpstr_vector(&["A", "Box"]));
    let second = "<TitlesOfParts><vt:vector size=\"0\" baseType=\"lpstr\"/></TitlesOfParts>";
    let original = app_with(&pairs(&[("Seiten", "1"), ("Masters", "1")]), &format!("{titles}{second}"));
    let (mut doc, _) = read_ok(&original);
    doc.rename_page(1, "B").unwrap();
    let written = model_write(&doc, false);
    assert_eq!(listed(&written.bytes), ["B", "Box"]);
    let app = text_of(&written.bytes, "/docProps/app.xml");
    assert!(app.contains(second), "the second list is the file's: {app}");
}

#[test]
fn a_count_that_is_not_a_number_leaves_the_lists_as_they_are_and_the_save_says_so() {
    // Three groups, the first with a count that says nothing. The other two counts and the two titles agree, so the lists look like two groups, and the second would be taken for the
    // pages' and its count rewritten where the numbers of the groups no longer fit the lists.
    let titles = format!("<TitlesOfParts>{}</TitlesOfParts>", lpstr_vector(&["Box", "Z"]));
    let original = app_with(&pairs(&[("P", "x"), ("M", "1"), ("Z", "1")]), &titles);
    let (mut doc, _) = read_ok(&original);
    doc.add_page(Some("C")).unwrap();
    let written = model_write(&doc, false);
    assert_eq!(listed(&written.bytes), ["Box", "Z"], "the lists are the file's");
    let app = text_of(&written.bytes, "/docProps/app.xml");
    assert!(app.contains("<vt:i4>x</vt:i4>") && !app.contains("<vt:i4>2</vt:i4>"), "{app}");
    assert!(written.warnings.iter().any(|w| w.contains("list of page names")), "{:?}", written.warnings);
}

/// A drawing whose lists are these (`groups` as the `HeadingPairs` of them, `names` the titles), edited by `edit`, saved from the model, and the titles it lists.
fn listed_after(groups: &[(&str, &str)], names: &[&str], edit: impl FnOnce(&mut Document)) -> (Vec<String>, Vec<String>) {
    let titles = format!("<TitlesOfParts>{}</TitlesOfParts>", lpstr_vector(names));
    let original = app_with(&pairs(groups), &titles);
    let (mut doc, _) = read_ok(&original);
    edit(&mut doc);
    let written = model_write(&doc, false);
    (listed(&written.bytes), written.warnings)
}

fn rename_master(doc: &mut Document, to: &str) {
    Arc::make_mut(doc.masters.get_mut(0).unwrap()).name = Some(Arc::from(to));
}

#[test]
fn the_masters_group_is_never_taken_for_the_pages_when_the_masters_come_first_and_one_is_renamed() {
    // The masters are listed first, a master is renamed, and a page is: neither group is the model's masters, so the lists cannot be told and are left (they were rewritten: the first group is
    // the masters', and was given the names of the pages).
    let (titles, warnings) = listed_after(&[("Masters", "1"), ("Pages", "1")], &["Box", "A"], |doc| {
        rename_master(doc, "Box2");
        doc.rename_page(1, "B").unwrap();
    });
    assert_eq!(titles, ["Box", "A"]);
    assert!(warnings.iter().any(|w| w.contains("list of page names")), "{warnings:?}");
}

#[test]
fn a_master_renamed_to_the_name_of_a_page_leaves_the_lists_as_they_are() {
    // The pages' group and the masters' are both `A` for the model now: the lists cannot be told apart (the masters' was given the names of the pages).
    let (titles, _) = listed_after(&[("Pages", "1"), ("Masters", "1")], &["A", "Box"], |doc| rename_master(doc, "A"));
    assert_eq!(titles, ["A", "Box"]);
}

#[test]
fn one_group_that_lists_the_pages_and_the_masters_is_not_the_pages_when_the_model_has_masters() {
    let (titles, _) = listed_after(&[("Pages and masters", "2")], &["A", "Box"], |doc| doc.rename_page(1, "B").unwrap());
    assert_eq!(titles, ["A", "Box"], "a group that lists both was given the names of the pages");
}

#[test]
fn the_pages_group_is_told_by_the_masters_group_and_follows_the_pages() {
    for groups in [[("Pages", "1"), ("Masters", "1")], [("Masters", "1"), ("Pages", "1")]] {
        let names: &[&str] = if groups[0].0 == "Pages" { &["A", "Box"] } else { &["Box", "A"] };
        let (titles, _) = listed_after(&groups, names, |doc| {
            doc.rename_page(1, "B").unwrap();
            doc.add_page(Some("C")).unwrap();
        });
        let want: Vec<&str> = if groups[0].0 == "Pages" { vec!["B", "C", "Box"] } else { vec!["Box", "B", "C"] };
        assert_eq!(titles, want);
    }
}

// ----------------------------------------------------------------------------------------------- kept content is charged

/// `n` unknown text nodes of a mebibyte each that name one allocation: the model holds a mebibyte, and the part they make is more than a part may be.
fn heavy_nodes() -> Vec<diacraft_doc::RawNode> {
    let text: Arc<str> = Arc::from("x".repeat(1 << 20).as_str());
    (0..520).map(|_| diacraft_doc::RawNode::Text(Arc::clone(&text))).collect()
}

fn heavy() -> diacraft_doc::Passthrough {
    diacraft_doc::Passthrough::from_nodes(heavy_nodes().into_iter().map(|node| diacraft_doc::Placed { after: 0, node }).collect())
}

fn heavy_kept() -> diacraft_doc::Kept {
    diacraft_doc::Kept { raw: diacraft_doc::RawAttrs::new(), kept: heavy() }
}

fn heavy_element(name: &str) -> diacraft_doc::RawElement {
    diacraft_doc::RawElement { name: Arc::from(name), attrs: diacraft_doc::RawAttrs::new(), children: heavy_nodes() }
}

/// A drawing the model writer writes, with nothing else in it.
fn model_only() -> Document {
    let mut doc = Document::new_blank();
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    doc
}

/// The save is refused, in the words of the part's room, and not made.
fn refused_for_the_room_of_a_part(result: Result<Written, WriteError>, what: &str) {
    let refused = result.expect_err(what);
    assert_eq!(refused.class(), "write_limit", "{what}: {refused}");
    assert!(refused.to_string().contains("bytes in one page or master part"), "{what}: {refused}");
}

#[test]
fn the_unknown_elements_of_the_document_are_charged_to_the_part_before_they_are_made_by_both_writers() {
    let mut doc = model_only();
    doc.passthrough = heavy();
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "the model writer");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "P")).build();
    let (mut doc, _) = read_ok(&original);
    doc.passthrough = heavy();
    let spliced = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: None }), &options(false));
    refused_for_the_room_of_a_part(spliced, "a save into the file");
}

#[test]
fn what_a_colour_entry_a_face_the_windows_and_the_solution_keep_is_charged_before_it_is_made() {
    let mut doc = model_only();
    doc.colors = Arc::new(vec![diacraft_doc::ColorEntry { ix: 24, rgb: Arc::from("#000000") }]);
    doc.ext.make_mut().vsdx.0.push((Arc::from("ColorEntry/24"), heavy_kept()));
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "a colour entry");

    let mut doc = model_only();
    let mut face = diacraft_doc::FaceName { name_u: Arc::from("Arial"), raw: diacraft_doc::RawAttrs::new(), ext: Default::default() };
    face.ext.make_mut().vsdx.0.push((Arc::from("FaceName"), heavy_kept()));
    doc.fonts = Arc::new(vec![face]);
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "a face");

    let mut doc = model_only();
    let held = diacraft_doc::VsdxHeld { windows: Some(Arc::new(heavy_element("Windows"))), ..diacraft_doc::VsdxHeld::default() };
    doc.vsdx = Some(Arc::new(held));
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "windows.xml");

    let mut doc = model_only();
    doc.diacraft = Some(Arc::new(heavy_element("Data")));
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "the DiaCraft data");
}

// ------------------------------------------------------------------------------------------------------------ stencils, windows, hostile names

#[test]
fn a_stencil_is_written_as_a_stencil_with_pages_that_have_no_part() {
    let bytes = VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.stencil.main+xml") // brand-ok: a content type the format defines
        .master(diacraft_testkit::package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "P").without_part())
        .build();
    let written = visit("a stencil", &bytes);
    assert!(written.warnings.is_empty(), "{:?}", written.warnings);
    assert!(!parts(&written.bytes).contains_key("/visio/pages/page1.xml"), "a stencil's page has no part");
    // The same model written as a drawing gives the page the part a drawing's page has.
    let (doc, _) = read_ok(&bytes);
    let as_drawing = model_write_as(&doc, true, Variant { kind: Kind::Drawing, macro_enabled: false });
    assert!(parts(&as_drawing.bytes).contains_key("/visio/pages/page1.xml"));
    assert_eq!(read_ok(&as_drawing.bytes).0.pages.len(), 1);
}

#[test]
fn a_drawing_saved_as_a_stencil_keeps_its_pages_and_says_so() {
    let (doc, _) = read_ok(&package_with_extras());
    let written = model_write_as(&doc, true, Variant { kind: Kind::Stencil, macro_enabled: false });
    assert!(written.warnings.iter().any(|w| w.contains("saved as a stencil")), "{:?}", written.warnings);
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 2);
}

#[test]
fn a_window_that_names_a_deleted_page_is_dropped() {
    let (mut doc, _) = read_ok(&windows_package());
    let at = doc.pages.iter().position(|p| p.id == 1).unwrap();
    doc.pages.remove(at).unwrap();
    let written = model_write(&doc, false);
    let windows = text_of(&written.bytes, "/visio/windows.xml");
    assert!(!windows.contains(r#"Page="1""#) && !windows.contains("<!--kept-->"), "the window of the page taken out goes: {windows}");
    assert!(windows.contains("x:y"), "what else the part holds stays: {windows}");
    assert!(written.warnings.iter().any(|w| w.contains("a page or master was removed")), "{:?}", written.warnings);
    // **Its settings stay**: a drawing window of their own holds them, and names no page.
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.window_flags().and_then(|f| f.grid), Some(true), "{windows}");
    assert_eq!(windows.matches("<Window ").count(), 1, "{windows}");
}

#[test]
fn names_and_ids_no_package_may_have_are_replaced_and_held_names_are_refused_in_words() {
    let (mut doc, _) = read_ok(&package_with_extras());
    // A page that names a part no package may have, and two pages that name one row.
    let (first, second) = (doc.pages.get(0).unwrap().clone(), doc.pages.get(1).unwrap().clone());
    {
        let mut a = (*first).clone();
        a.part_name = Some(Arc::from("/visio/../outside.xml"));
        let mut b = (*second).clone();
        b.rel_id = a.rel_id.clone();
        doc.pages.set(0, Arc::new(a)).unwrap();
        doc.pages.set(1, Arc::new(b)).unwrap();
    }
    let written = model_write(&doc, true);
    let names = names_folded(&written.bytes);
    assert!(names.iter().all(|n| !n.contains("..")), "{names:?}");
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.pages.len(), 2, "{:?}", report.warnings);
    assert!(back.pages.iter().all(|p| p.part_name.is_some()), "each page has a part of its own");
    let parts_of: BTreeSet<String> = back.pages.iter().filter_map(|p| p.part_name.as_ref().map(|n| n.to_ascii_lowercase())).collect();
    assert_eq!(parts_of.len(), 2);
    let rels: BTreeSet<String> = back.pages.iter().filter_map(|p| p.rel_id.as_ref().map(|n| n.to_ascii_lowercase())).collect();
    assert_eq!(rels.len(), 2, "two pages do not name one row for two parts");
    // Two held parts that are one name are one part, and the save says so; a held part no package can hold is refused, in words.
    let mut dup = Document::new_blank();
    dup.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    let held = |name: &str| -> Arc<Part> {
        Arc::new(Part {
            format: Format::Vsdx,
            name: Arc::from(name),
            media_type: Arc::from("application/x-a"),
            role: PartRole::Other,
            manifest: diacraft_doc::RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(&b"x"[..]),
        })
    };
    dup.parts.push(held("/a/b.dat"));
    dup.parts.push(held("/A/B.DAT"));
    let written = model_write(&dup, true);
    assert_eq!(read_ok(&written.bytes).0.parts.len(), 1);
    assert!(written.warnings.iter().any(|w| w.contains("another part has its name")), "{:?}", written.warnings);
    let mut bad = Document::new_blank();
    bad.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    bad.parts.push(held("no-slash.dat"));
    let refused = try_write(&bad, &options(true)).expect_err("a name with no slash");
    assert!(refused.to_string().contains("cannot be made") || refused.to_string().contains("part"), "{refused}");
}

// ------------------------------------------------------------------------------------------------------------------ a second save

#[test]
fn a_second_model_save_is_no_edit() {
    for (name, bytes) in [("with parts", held_package()), ("with bookkeeping", bookkeeping_package()), ("rich", rich_vsdx_bytes())] {
        let (doc, _) = read_ok(&bytes);
        // The same model twice: the same bytes.
        let first = model_write_as(&doc, false, macro_drawing());
        let again = model_write_as(&doc, false, macro_drawing());
        assert_eq!(first.bytes, again.bytes, "{name}: one model, two saves");
        // A model read from what the first save wrote, saved again: the same content (the clock is pinned).
        let (reread, _) = read_ok(&first.bytes);
        let second = model_write_as(&reread, false, macro_drawing());
        let d = diff(&first.bytes, &second.bytes);
        assert!(d.same(), "{name}: a second save of the file just written changed its content:\n{}", show(&d));
        // And with the content unchanged the bookkeeping is kept as it was written.
        let third = model_write_as(&reread, true, macro_drawing());
        let d = diff(&first.bytes, &third.bytes);
        assert!(d.same(), "{name}: an unchanged save of the file just written changed its content:\n{}", show(&d));
    }
}

#[test]
fn a_save_into_the_file_a_model_save_made_is_no_edit() {
    // A drawing saved from the model (Save As) and then saved again in place, with no edit between: the second save is a splice into the file the first made, and finds nothing to write.
    let mut fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("with parts", held_package()),
        ("with bookkeeping", bookkeeping_package()),
        ("rich", rich_vsdx_pkg(false).build()),
        ("properties", props_package()),
    ];
    fixtures.push(("two pages", package_with_extras()));
    for (name, bytes) in fixtures {
        let (doc, _) = read_ok(&bytes);
        let first = model_write_as(&doc, false, macro_drawing());
        let spliced = write_with(
            &doc,
            Some(&OriginalBytes { bytes: &first.bytes, baseline: Some(&first.document) }),
            &WriteOptions { variant: Some(macro_drawing()), ..options(false) },
        )
        .unwrap_or_else(|e| panic!("{name}: a save into the file the model save made refused: {e}"));
        let d = diff(&first.bytes, &spliced.bytes);
        assert!(d.same(), "{name}: a second save changed the content ({} differences):\n{}", d.count, show(&d));
    }
    // The same for a drawing made new: the model writer gives it the settings and faces a new drawing's file has, and the model it hands back is the caller's.
    let mut doc = Document::new_blank();
    doc.set_snap_words(&diacraft_doc::SnapWords { glue: Some(Arc::from("9")), ..diacraft_doc::SnapWords::default() });
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    let first = model_write(&doc, false);
    let spliced = write_with(&doc, Some(&OriginalBytes { bytes: &first.bytes, baseline: Some(&first.document) }), &options(false))
        .unwrap_or_else(|e| panic!("{e}"));
    let d = diff(&first.bytes, &spliced.bytes);
    assert!(d.same(), "a new drawing's second save changed the content:\n{}", show(&d));
}

#[test]
fn the_model_writer_reads_back_what_it_wrote_without_a_warning() {
    for (name, bytes) in [("with parts", held_package()), ("rich", rich_vsdx_bytes()), ("every package", crate::held::everything_package())] {
        let (doc, _) = read_ok(&bytes);
        let (_, before) = read(&bytes, &Limits::default()).unwrap();
        let written = model_write_as(&doc, true, macro_drawing());
        // The warnings of the read of the file written are the warnings of the read of the file (the model keeps what it was read as).
        assert_eq!(written.read_back.warnings.len(), before.warnings.len(), "{name}: {:?}", written.read_back.warnings);
    }
}

#[test]
fn model_parts_and_rels_are_what_the_model_holds_for_a_package_made_of_nothing_else() {
    // A document that holds parts and rows of its own and no file at all: the model is the only record.
    let mut doc = Document::new_blank();
    let part = |name: &str, ct: &str, bytes: &[u8], rels: Vec<Rel>| {
        Arc::new(Part {
            format: Format::Vsdx,
            name: Arc::from(name),
            media_type: Arc::from(ct),
            role: PartRole::Other,
            manifest: diacraft_doc::RawAttrs::new(),
            rels: Arc::new(rels),
            bytes: Arc::from(bytes),
        })
    };
    doc.parts.push(part(
        "/custom/a.bin",
        "application/x-a",
        b"aaa",
        vec![Rel {
            source: Arc::from("/custom/a.bin"),
            id: Arc::from("rId1"),
            kind: Arc::from("urn:t"),
            target: Arc::from("b.bin"),
            external: false,
        }],
    ));
    doc.parts.push(part("/custom/b.bin", "application/x-a", b"bbb", Vec::new()));
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld {
        rels: vec![Rel {
            source: Arc::from("/"),
            id: Arc::from("rId77"),
            kind: Arc::from("urn:root"),
            target: Arc::from("custom/a.bin"),
            external: false,
        }],
        ..diacraft_doc::VsdxHeld::default()
    }));
    let written = model_write(&doc, true);
    let p = parts(&written.bytes);
    assert_eq!(p.get("/custom/a.bin").map(Vec::as_slice), Some(&b"aaa"[..]));
    assert_eq!(p.get("/custom/b.bin").map(Vec::as_slice), Some(&b"bbb"[..]));
    let pkg = package(&written.bytes);
    assert_eq!(pkg.rels("/custom/a.bin").unwrap().get("rId1").map(|r| r.target_raw.as_str()), Some("b.bin"));
    let root = pkg.rels("/").unwrap();
    assert_eq!(root.get("rId77").map(|r| r.target_raw.as_str()), Some("custom/a.bin"));
    assert_eq!(root.of_type(&format!("{VIS}document")).count(), 1, "a document relationship is made when the rows hold none");
    assert_eq!(read_ok(&written.bytes).0.parts.len(), 2);
}

// -----------------------------------------------------------------------------------------: what a save hands back

#[test]
fn the_document_a_save_hands_back_holds_the_bytes_of_the_models_parts_and_no_copy_of_them() {
    // The check of the file a save made reads it without moving the parts into the document it hands back (the engine keeps that document as the baseline of the next save): the parts of
    // the document are the model's own `Arc`s, the same bytes, and not a second copy of every embedded object and VBA project.
    let original = held_package();
    let (doc, _) = read_ok(&original);
    assert!(doc.parts.len() > 5, "the package holds parts");
    let model = model_write_as(&doc, false, macro_drawing());
    let spliced = write_with(
        &doc,
        Some(&OriginalBytes { bytes: &original, baseline: Some(&doc) }),
        &WriteOptions { variant: Some(macro_drawing()), ..options(false) },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for (how, written) in [("the model writer", &model), ("the splice writer", &spliced)] {
        assert_eq!(written.document.parts.len(), doc.parts.len(), "{how}");
        for (back, own) in written.document.parts.iter().zip(doc.parts.iter()) {
            assert!(Arc::ptr_eq(back, own), "{how}: the part {} is a copy", own.name);
        }
    }
}

fn splice_save(original: &[u8], edit: impl FnOnce(&mut Document)) -> Written {
    let (base, _) = read_ok(original);
    let mut doc = base.clone();
    edit(&mut doc);
    write_with(&doc, Some(&OriginalBytes { bytes: original, baseline: Some(&base) }), &options(false)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_change_to_any_field_of_the_properties_is_an_edit_the_save_stamps_and_a_time_or_a_custom_list_alone_is_not() {
    let original = props_package();
    let stamp = "2023-11-14T22:13:20Z";
    type Setter = fn(&mut diacraft_doc::DocProps);
    let fields: [(&str, &str, Setter); 12] = [
        ("subject", "/docProps/core.xml", |p| p.subject = Some(Arc::from("v"))),
        ("creator", "/docProps/core.xml", |p| p.creator = Some(Arc::from("v"))),
        ("keywords", "/docProps/core.xml", |p| p.keywords = Some(Arc::from("v"))),
        ("description", "/docProps/core.xml", |p| p.description = Some(Arc::from("v"))),
        ("lastModifiedBy", "/docProps/core.xml", |p| p.last_modified_by = Some(Arc::from("v"))),
        ("lastPrinted", "/docProps/core.xml", |p| p.last_printed = Some(Arc::from("v"))),
        ("category", "/docProps/core.xml", |p| p.category = Some(Arc::from("v"))),
        ("language", "/docProps/core.xml", |p| p.language = Some(Arc::from("v"))),
        ("Company", "/docProps/app.xml", |p| p.company = Some(Arc::from("v"))),
        ("Manager", "/docProps/app.xml", |p| p.manager = Some(Arc::from("v"))),
        ("HyperlinkBase", "/docProps/app.xml", |p| p.hyperlink_base = Some(Arc::from("v"))),
        ("AppVersion", "/docProps/app.xml", |p| p.app_version = Some(Arc::from("v"))),
    ];
    for (name, part, set) in fields {
        let written = splice_save(&original, |doc| set(Arc::make_mut(&mut doc.props)));
        assert!(text_of(&written.bytes, part).contains(">v</"), "{name} is written to {part}");
        let core = text_of(&written.bytes, "/docProps/core.xml");
        assert!(core.contains(stamp), "a change to {name} is an edit, and the save stamps `modified`: {core}");
        assert!(text_of(&written.bytes, "/docProps/app.xml").contains("<Application>DiaCraft</Application>"), "{name}");
    }
    // A time of the model alone is not a field of the properties: nothing is edited, and the core part is the file's own.
    for (name, set) in [
        ("created", (|p: &mut diacraft_doc::DocProps| p.created = Some(Arc::from("2000-01-01T00:00:00Z"))) as Setter),
        ("modified", |p| p.modified = Some(Arc::from("2000-01-01T00:00:00Z"))),
    ] {
        let written = splice_save(&original, |doc| set(Arc::make_mut(&mut doc.props)));
        // (The writer's own name goes into `app.xml` of every save.)
        assert_eq!(
            text_of(&written.bytes, "/docProps/core.xml"),
            text_of(&original, "/docProps/core.xml"),
            "a change to {name} alone is no edit: nothing is stamped"
        );
    }
    // The custom list is the part's own: it is written, and the core part is not stamped for it.
    let written = splice_save(&original, |doc| {
        let props = Arc::make_mut(&mut doc.props);
        let mut added = props.custom.first().cloned().unwrap();
        added.name = Arc::from("Another");
        props.custom.push(added);
    });
    assert!(text_of(&written.bytes, "/docProps/custom.xml").contains("Another"));
    assert_eq!(
        text_of(&written.bytes, "/docProps/core.xml"),
        text_of(&original, "/docProps/core.xml"),
        "a custom property alone does not stamp the core part"
    );
}

#[test]
fn the_custom_properties_of_the_model_are_charged_to_the_part_before_they_are_made_by_both_writers() {
    // 520 properties of one mebibyte that name one allocation: the model holds a mebibyte, and the part they make is more than a part may be.
    let text: Arc<str> = Arc::from("x".repeat(1 << 20).as_str());
    let props: Vec<diacraft_doc::CustomProp> = (0..520)
        .map(|i| diacraft_doc::CustomProp {
            format: Format::Vsdx,
            name: Arc::from(format!("P{i}")),
            value_type: Arc::from("vt:lpwstr"),
            value: Arc::clone(&text),
            raw: diacraft_doc::RawAttrs::new(),
        })
        .collect();
    let mut doc = model_only();
    Arc::make_mut(&mut doc.props).custom = props.clone();
    refused_for_the_room_of_a_part(try_write(&doc, &options(true)), "the model writer");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "P")).build();
    let (mut doc, _) = read_ok(&original);
    Arc::make_mut(&mut doc.props).custom = props;
    refused_for_the_room_of_a_part(
        write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: None }), &options(false)),
        "a save into the file",
    );
}
