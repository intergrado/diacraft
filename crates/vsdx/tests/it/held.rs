//! What the reader holds beside the model's own fields (the native format's design, section 4.2): every part of the package that no field of
//! the model rebuilds, the relationships the writer cannot make for itself, the content-type rows no part needs, the rest of the property parts, `windows.xml`,
//! the part names of the pages and masters, and the elements and attributes of each place the reader types and gives no slot to.
//!
//! **What is held, and what is not.** A part is held unless the model rebuilds it from its own fields: the document part, the two indexes, the page and master
//! parts that were read as such, the themes, the property parts that were read, `windows.xml` (held whole in `VsdxHeld::windows`), the pictures that were
//! loaded, and DiaCraft's solution when nothing else is in the solutions index. A relationship row is held unless a field of the model says the whole of it: the
//! rows of the indexes that name the parts of pages and masters, and the rows of a page's or master's part that a picture was loaded through.

use std::collections::BTreeSet;

use diacraft_doc::{Document, Format, Kept, Part, PartRole, VsdxRest};
use diacraft_opc::Package;
use diacraft_testkit::package::{NS_2012, NS_R, OD, PK, PageSpec, Pkg, VIS, VsdxBuilder};
use diacraft_vsdx::{OriginalBytes, WriteOptions, read, write_with};

use crate::drops::Kind;
use crate::emit::{el, to_xml};
use crate::read_ok;

const MS: &str = "http://schemas.microsoft.com/office/2006/relationships/"; // brand-ok: a URI the format defines
const MS_UI: &str = "http://schemas.microsoft.com/office/2007/relationships/ui/extensibility"; // brand-ok: a URI the format defines
const SIG: &str = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/";
const CT_OLE: &str = "application/vnd.openxmlformats-officedocument.oleObject";
const DOC: &str = "/visio/document.xml";
const PAGE1: &str = "/visio/pages/page1.xml";

// ----------------------------------------------------------------------------------------------------------------------------- helpers

/// The entries of a slot's list with this key.
pub(crate) fn rest<'a>(list: &'a VsdxRest, key: &str) -> Vec<&'a Kept> {
    list.0.iter().filter(|(k, _)| &**k == key).map(|(_, kept)| kept).collect()
}

/// What a `Kept` holds, as text: its attributes, then each node with the number of typed children it came after.
pub(crate) fn dump(kept: &Kept) -> String {
    let mut out = String::new();
    for (k, v) in kept.raw.iter() {
        out.push_str(&format!(" {k}=\"{v}\""));
    }
    for placed in kept.kept.iter() {
        let tree = el("n", vec![], vec![diacraft_vsdx::passthrough::node_to_tree(&placed.node)]);
        out.push_str(&format!(" [after {}] {}", placed.after, to_xml(&tree)));
    }
    out
}

fn held<'a>(doc: &'a Document, name: &str) -> Option<&'a Part> {
    doc.parts.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| &**p)
}

fn held_names(doc: &Document) -> BTreeSet<String> {
    doc.parts.iter().map(|p| p.name.to_string()).collect()
}

fn rels_of(doc: &Document, source: &str) -> Vec<(String, String, String, bool)> {
    doc.vsdx
        .as_ref()
        .map(|v| {
            v.rels.iter().filter(|r| &*r.source == source).map(|r| (r.id.to_string(), r.kind.to_string(), r.target.to_string(), r.external)).collect()
        })
        .unwrap_or_default()
}

fn text_of(rest: &Kept) -> String {
    dump(rest)
}

/// A package that holds each kind of part section 4.2 lists, in the places the format puts them.
pub(crate) fn everything_package() -> Vec<u8> {
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
        .part("/visio/media/i.png", "image/png", [1u8, 2, 3])
        // The fallback picture of the embedded object: loaded as the shape's picture, so the model has it.
        .part("/visio/media/image2.emf", "image/x-emf", [9u8, 9])
        .part("/visio/embeddings/oleObject1.bin", CT_OLE, [0xD0u8, 0xCF, 0x11, 0xE0])
        .rel("/visio/embeddings/oleObject1.bin", "rId1", &format!("{OD}image"), "../media/image2.emf")
        .part("/visio/embeddings/Drawing1.vsdx", "application/vnd.ms-visio.drawing", [b'P', b'K', 3, 4]) // brand-ok: a content type the format defines
        // An unknown part no relationship reaches, typed by a `Default` row.
        .part_without_override("/extra/blob.dat", [5u8, 6, 7])
        // Custom XML: an item and its properties, the item reached by a relationship of the document and the properties by one of the item.
        .part("/customXml/item1.xml", "application/xml", "<root/>")
        .part(
            "/customXml/itemProps1.xml",
            "application/vnd.openxmlformats-officedocument.customXmlProperties+xml",
            "<ds:datastoreItem xmlns:ds=\"urn:ds\"/>",
        )
        .rel(DOC, "rId90", &format!("{OD}customXml"), "../customXml/item1.xml")
        .rel("/customXml/item1.xml", "rId1", &format!("{OD}customXmlProps"), "itemProps1.xml")
        // Comments, data connections and recordsets, validation and extensions.
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
        // A macro project, with its signature.
        .part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", [1u8, 1, 1])
        .rel(DOC, "rId96", &format!("{MS}vbaProject"), "vbaProject.bin")
        .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", [2u8, 2])
        .rel("/visio/vbaProject.bin", "rId1", &format!("{MS}vbaProjectSignature"), "vbaProjectSignature.bin")
        // The thumbnail, a customisation of the ribbon and a digital signature.
        .part("/docProps/thumbnail.emf", "image/x-emf", [3u8, 3, 3, 3])
        .rel("/", "rId10", &format!("{PK}thumbnail"), "docProps/thumbnail.emf")
        .part("/visio/customUI/customUI1.xml", "application/xml", "<customUI/>")
        .rel("/", "rId11", MS_UI, "visio/customUI/customUI1.xml")
        .part("/_xmlsignatures/origin.sigs", "application/vnd.openxmlformats-package.digital-signature-origin", [4u8])
        .rel("/", "rId12", &format!("{SIG}origin"), "_xmlsignatures/origin.sigs")
        .part("/_xmlsignatures/sig1.xml", "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml", "<Signature/>")
        .rel("/_xmlsignatures/origin.sigs", "rId1", &format!("{SIG}signature"), "sig1.xml");
    VsdxBuilder::new()
        .page(page)
        // A solution of another program: the index and its part are held, not rebuilt.
        .solution("OtherApp", "<SolutionXML Name=\"OtherApp\"><Data/></SolutionXML>")
        .with(extra)
        .build()
}

// ------------------------------------------------------------------------------------------------------------------------- the parts

#[test]
fn every_unmodelled_part_is_held_with_its_rels_and_bytes() {
    let bytes = everything_package();
    let (doc, report) = read_ok(&bytes);
    let expected: BTreeSet<String> = [
        "/visio/embeddings/oleObject1.bin",
        "/visio/embeddings/Drawing1.vsdx",
        "/extra/blob.dat",
        "/customXml/item1.xml",
        "/customXml/itemProps1.xml",
        "/visio/comments.xml",
        "/visio/data/connections.xml",
        "/visio/data/recordsets.xml",
        "/visio/data/recordset1.xml",
        "/visio/validation.xml",
        "/visio/extensions.xml",
        "/visio/vbaProject.bin",
        "/visio/vbaProjectSignature.bin",
        "/docProps/thumbnail.emf",
        "/visio/customUI/customUI1.xml",
        "/_xmlsignatures/origin.sigs",
        "/_xmlsignatures/sig1.xml",
        // Another program's solution: its index and its part.
        "/visio/solutions/solutions.xml",
        "/visio/solutions/solution1.xml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(held_names(&doc), expected);
    // What the model rebuilds is not held: the document, the indexes, the pages, the pictures it loaded, the property parts.
    for part in [
        DOC,
        "/visio/pages/pages.xml",
        PAGE1,
        "/visio/masters/masters.xml",
        "/visio/media/i.png",
        "/visio/media/image2.emf",
        "/docProps/core.xml",
        "/docProps/app.xml",
    ] {
        assert!(held(&doc, part).is_none(), "{part} is the model's, and is not held");
    }
    // The package's bytes, exactly, and the content type and role of each.
    let pkg = Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    for part in doc.parts.iter() {
        assert_eq!(part.format, Format::Vsdx);
        assert_eq!(&*part.bytes, pkg.part(&part.name).unwrap(), "{}", part.name);
    }
    let kinds = |name: &str| -> (String, PartRole) {
        let p = held(&doc, name).unwrap_or_else(|| panic!("{name} is held"));
        (p.media_type.to_string(), p.role)
    };
    assert_eq!(kinds("/visio/embeddings/oleObject1.bin"), (CT_OLE.to_owned(), PartRole::Embedding));
    assert_eq!(kinds("/visio/embeddings/Drawing1.vsdx").1, PartRole::Embedding);
    assert_eq!(kinds("/extra/blob.dat"), ("application/octet-stream".to_owned(), PartRole::Other));
    assert_eq!(kinds("/customXml/item1.xml").1, PartRole::CustomXml);
    assert_eq!(kinds("/customXml/itemProps1.xml").1, PartRole::CustomXml);
    assert_eq!(kinds("/visio/comments.xml").1, PartRole::Other);
    assert_eq!(kinds("/visio/vbaProject.bin"), ("application/vnd.ms-office.vbaProject".to_owned(), PartRole::MacroProject));
    assert_eq!(kinds("/visio/vbaProjectSignature.bin").1, PartRole::MacroSignature);
    assert_eq!(kinds("/docProps/thumbnail.emf"), ("image/x-emf".to_owned(), PartRole::Thumbnail));
    assert_eq!(kinds("/_xmlsignatures/origin.sigs").1, PartRole::Signature);
    assert_eq!(kinds("/_xmlsignatures/sig1.xml").1, PartRole::Signature);
    // The relationships from a held part are its own, with the id, type and target as written.
    let ole = held(&doc, "/visio/embeddings/oleObject1.bin").unwrap();
    assert_eq!(ole.rels.len(), 1);
    assert_eq!(
        (&*ole.rels[0].source, &*ole.rels[0].id, &*ole.rels[0].kind, &*ole.rels[0].target, ole.rels[0].external),
        ("/visio/embeddings/oleObject1.bin", "rId1", &*format!("{OD}image"), "../media/image2.emf", false)
    );
    assert_eq!(held(&doc, "/visio/vbaProject.bin").unwrap().rels.len(), 1);
    assert_eq!(held(&doc, "/visio/solutions/solutions.xml").unwrap().rels.len(), 1);
    assert!(held(&doc, "/extra/blob.dat").unwrap().rels.is_empty());
    // The report says how much is held, and the sum of the held bytes is the parts' bytes.
    assert_eq!(report.held_parts, doc.parts.len());
    assert_eq!(report.held_bytes, doc.parts.iter().map(|p| p.bytes.len() as u64).sum::<u64>());
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0, "nothing is skipped: {:?}", report.skipped_counts);
    // The model has the two pictures the page showed, by the parts they came from.
    let page = doc.pages.get(0).unwrap();
    let pictures: Vec<String> = page.shapes.iter().filter_map(|s| s.foreign.as_ref()?.picture.as_ref().map(|m| m.part.to_string())).collect();
    assert_eq!(pictures, ["/visio/media/i.png", "/visio/media/image2.emf"]);
}

#[test]
fn the_relationships_to_held_parts_from_the_parts_the_model_rebuilds_are_held_and_the_ones_it_makes_are_not() {
    let (doc, _) = read_ok(&everything_package());
    let held_rels = &doc.vsdx.as_ref().expect("a document read from a package holds what the package has").rels;
    // From the package: the document, the properties, and everything else in the order the package has it.
    let root = rels_of(&doc, "/");
    assert!(root.contains(&("rId10".into(), format!("{PK}thumbnail"), "docProps/thumbnail.emf".into(), false)), "{root:?}");
    assert!(root.iter().any(|r| r.0 == "rId11" && r.1 == MS_UI), "{root:?}");
    assert!(root.iter().any(|r| r.0 == "rId12" && r.1 == format!("{SIG}origin")), "{root:?}");
    assert!(root.iter().any(|r| r.1 == format!("{VIS}document")), "the document's own relationship is held, with its id: {root:?}");
    // From the document: the held parts, with the ids they had.
    let from_doc = rels_of(&doc, DOC);
    for (id, ty) in [("rId90", format!("{OD}customXml")), ("rId91", format!("{VIS}comments")), ("rId96", format!("{MS}vbaProject"))] {
        assert!(from_doc.iter().any(|r| r.0 == id && r.1 == ty), "{id} is held: {from_doc:?}");
    }
    // The rows of the pages index that name a page's part are the page's (`rel_id` and `part_name`), and are not held again.
    assert!(rels_of(&doc, "/visio/pages/pages.xml").is_empty(), "{:?}", rels_of(&doc, "/visio/pages/pages.xml"));
    // The page's picture row is the picture's (the `ForeignData`'s `rel_id`, the `Media`'s part); its row to the embedded object is held.
    let from_page = rels_of(&doc, PAGE1);
    assert_eq!(from_page.len(), 2, "{from_page:?}");
    assert!(from_page.contains(&("rId2".into(), format!("{OD}oleObject"), "../embeddings/oleObject1.bin".into(), false)));
    assert!(from_page.contains(&("rId3".into(), format!("{OD}package"), "../embeddings/Drawing1.vsdx".into(), false)));
    // Every held row names its source; the sources are parts the model rebuilds, and the held parts keep their own.
    let sources: BTreeSet<&str> = held_rels.iter().map(|r| &*r.source).collect();
    for source in sources {
        assert!(held(&doc, source).is_none(), "{source} is a held part, and its rows are its own");
    }
}

#[test]
fn a_rel_to_a_held_part_from_a_page_is_held() {
    let page = PageSpec::new(1, "P").shapes("").rel("rId1", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin").rel(
        "rId2",
        &format!("{OD}hyperlink"),
        "../embeddings/missing.bin",
    );
    let extra = Pkg::new().part("/visio/embeddings/oleObject1.bin", CT_OLE, [1u8]).rel_external(
        PAGE1,
        "rId7",
        &format!("{OD}hyperlink"),
        "https://example.org/x?a=1&b=2",
    );
    let (doc, _) = read_ok(&VsdxBuilder::new().page(page).with(extra).build());
    let rows = rels_of(&doc, PAGE1);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.contains(&("rId1".into(), format!("{OD}oleObject"), "../embeddings/oleObject1.bin".into(), false)));
    // A relationship to a part that is not there is held as it is.
    assert!(rows.contains(&("rId2".into(), format!("{OD}hyperlink"), "../embeddings/missing.bin".into(), false)));
    // An external one: its target is the URL, as written, and the mode is kept.
    assert!(rows.contains(&("rId7".into(), format!("{OD}hyperlink"), "https://example.org/x?a=1&b=2".into(), true)));
    assert_eq!(held_names(&doc), BTreeSet::from(["/visio/embeddings/oleObject1.bin".to_owned()]));
}

// ------------------------------------------------------------------------------------------------------------------- the properties

/// A custom property as the test compares it: its name, its type, its value and its other attributes.
type Prop = (String, String, String, Vec<(String, String)>);

#[test]
fn docprops_are_held_exactly() {
    let core = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:x=\"urn:x\"><dc:title>T</dc:title><cp:contentStatus>Final</cp:contentStatus><!--c--><dc:creator>A</dc:creator><x:extra k=\"v\">e</x:extra></cp:coreProperties>";
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application><Company>C</Company><Template>Normal.vst</Template><TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>P</vt:lpstr></vt:vector></TitlesOfParts></Properties>";
    let custom = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"2\" name=\"Name\"><vt:lpwstr>some text</vt:lpwstr></property><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"3\" name=\"Count\"><vt:i4>007</vt:i4></property><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"4\" name=\"Flag\"><vt:bool>true</vt:bool></property></Properties>";
    let extra = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    let (doc, report) = read_ok(&VsdxBuilder::new().core(core).app(app).with(extra).page(PageSpec::new(1, "P")).build());
    // The custom properties, each with its type as the file spells it and its value as written (`007` is not `7`).
    let custom: Vec<Prop> = doc
        .props
        .custom
        .iter()
        .map(|c| {
            assert_eq!(c.format, Format::Vsdx);
            (c.name.to_string(), c.value_type.to_string(), c.value.to_string(), c.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect())
        })
        .collect();
    let attrs = |pid: &str| vec![("fmtid".to_owned(), "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}".to_owned()), ("pid".to_owned(), pid.to_owned())];
    assert_eq!(
        custom,
        vec![
            ("Name".to_owned(), "vt:lpwstr".to_owned(), "some text".to_owned(), attrs("2")),
            ("Count".to_owned(), "vt:i4".to_owned(), "007".to_owned(), attrs("3")),
            ("Flag".to_owned(), "vt:bool".to_owned(), "true".to_owned(), attrs("4")),
        ]
    );
    assert!(held(&doc, "/docProps/custom.xml").is_none(), "the custom properties are typed, and the part is not held as well");
    // The part the fields read is not held either; what no field reads is, by the name of the part's root.
    assert!(held(&doc, "/docProps/core.xml").is_none() && held(&doc, "/docProps/app.xml").is_none());
    let vsdx = doc.vsdx.as_ref().expect("held");
    let core_rest: Vec<String> = vsdx.props_rest.iter().filter(|(k, _)| &**k == "coreProperties").map(|(_, kept)| text_of(kept)).collect();
    assert_eq!(core_rest.len(), 1, "{:?}", vsdx.props_rest);
    assert!(core_rest[0].contains(r#"xmlns:x="urn:x""#), "{}", core_rest[0]);
    assert!(core_rest[0].contains("<cp:contentStatus>Final</cp:contentStatus>"), "{}", core_rest[0]);
    assert!(core_rest[0].contains("<x:extra k=\"v\">e</x:extra>"), "{}", core_rest[0]);
    assert!(core_rest[0].contains("<!--c-->"), "{}", core_rest[0]);
    assert!(
        !core_rest[0].contains("dc:title") && !core_rest[0].contains("dc:creator"),
        "the elements a field reads are the field's: {}",
        core_rest[0]
    );
    let app_rest: Vec<String> = vsdx.props_rest.iter().filter(|(k, _)| &**k == "Properties").map(|(_, kept)| text_of(kept)).collect();
    assert_eq!(app_rest.len(), 1, "{:?}", vsdx.props_rest);
    assert!(app_rest[0].contains("<Template>Normal.vst</Template>"), "{}", app_rest[0]);
    assert!(app_rest[0].contains("<Application>Some Writer</Application>"), "the writer's name is not a field: {}", app_rest[0]);
    assert!(app_rest[0].contains("<TitlesOfParts>"), "{}", app_rest[0]);
    assert!(!app_rest[0].contains("<Company>"), "{}", app_rest[0]);
    assert_eq!(doc.props.company.as_deref(), Some("C"));
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0);
}

#[test]
fn a_custom_properties_part_the_list_cannot_say_exactly_is_held_whole() {
    // A property with a vector in it, and a prefix of a namespace the root declares and the list would lose.
    let custom = "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property pid=\"2\" name=\"V\"><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>a</vt:lpstr></vt:vector></property></Properties>";
    let extra = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    let (doc, _) = read_ok(&VsdxBuilder::new().with(extra).page(PageSpec::new(1, "P")).build());
    assert!(doc.props.custom.is_empty());
    let part = held(&doc, "/docProps/custom.xml").expect("a part the list cannot hold is held as bytes");
    assert_eq!(&*part.bytes, custom.as_bytes());
    assert_eq!(&*part.media_type, "application/vnd.openxmlformats-officedocument.custom-properties+xml");
}

#[test]
fn a_property_part_that_is_not_read_is_held_as_bytes() {
    let broken = "<cp:coreProperties xmlns:cp=\"urn:cp\"><unclosed>";
    let (doc, report) = read_ok(&VsdxBuilder::new().core(broken).page(PageSpec::new(1, "P")).build());
    let part = held(&doc, "/docProps/core.xml").expect("held as it was");
    assert_eq!(&*part.bytes, broken.as_bytes());
    assert_eq!(report.warning_count("properties_unreadable"), 1);
}

#[test]
fn windows_xml_is_held_whole() {
    let windows = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Windows xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ClientWidth=\"1280\" ClientHeight=\"720\" xml:space=\"preserve\"><Window ID=\"0\" WindowType=\"Drawing\" WindowState=\"1073741824\" ViewScale=\"-1\"><ShowGrid>1</ShowGrid><!--kept--><Page ID=\"2\"/></Window><x:y xmlns:x=\"urn:x\"/></Windows>";
    let extra = Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", windows).rel(
        DOC,
        "rId30",
        &format!("{VIS}windows"),
        "windows.xml",
    ); // brand-ok: a content type the format defines
    let (doc, _) = read_ok(&VsdxBuilder::new().with(extra).page(PageSpec::new(1, "P")).build());
    assert!(held(&doc, "/visio/windows.xml").is_none(), "the window's settings are held whole, not as a part as well");
    let w = doc.vsdx.as_ref().and_then(|v| v.windows.as_ref()).expect("windows.xml is held");
    assert_eq!(&*w.name, "Windows");
    let attrs: Vec<(&str, &str)> = w.attrs.iter().collect();
    assert!(
        attrs.contains(&("ClientWidth", "1280")) && attrs.contains(&("ClientHeight", "720")) && attrs.contains(&("xml:space", "preserve")),
        "{attrs:?}"
    );
    let text = to_xml(&diacraft_vsdx::passthrough::to_tree(w));
    assert!(text.contains("<Window ID=\"0\" WindowType=\"Drawing\" WindowState=\"1073741824\" ViewScale=\"-1\"><ShowGrid>1</ShowGrid><!--kept--><Page ID=\"2\"/></Window>"), "{text}");
    assert!(text.contains("<x:y xmlns:x=\"urn:x\"/>"), "{text}");
    // The row of the document that names it is held, with the id it had.
    assert!(rels_of(&doc, DOC).iter().any(|r| r.0 == "rId30" && r.1 == format!("{VIS}windows") && r.2 == "windows.xml"));
}

#[test]
fn a_windows_part_that_is_not_xml_is_held_as_bytes_and_the_file_opens() {
    let extra = Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", "<Windows><not closed").rel(
        DOC,
        "rId30",
        &format!("{VIS}windows"),
        "windows.xml",
    ); // brand-ok: a content type the format defines
    let (doc, _) = read_ok(&VsdxBuilder::new().with(extra).page(PageSpec::new(1, "P")).build());
    assert!(doc.vsdx.as_ref().is_some_and(|v| v.windows.is_none()));
    assert_eq!(&*held(&doc, "/visio/windows.xml").unwrap().bytes, b"<Windows><not closed");
}

// ----------------------------------------------------------------------------------------------- names, ids and content-type rows

#[test]
fn part_names_rel_ids_and_content_type_rows_are_held() {
    // Parts under names no builder uses, reached by ids that are not the builder's, and rows of the content types that name nothing.
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"5\" Name=\"A\"><Rel r:id=\"rId7\"/></Page><Page ID=\"6\" Name=\"B\"><Rel r:id=\"rId8\"/></Page></Pages>"
    );
    let contents = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"1\"/></Shapes></PageContents>");
    let masters = format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Master ID=\"3\" NameU=\"M\"><Rel r:id=\"rId9\"/></Master></Masters>");
    let mcontents = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"4\"/></Shapes></MasterContents>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/Page2.XML", diacraft_testkit::package::CT_PAGE, &contents)
        .part("/visio/other/zz.xml", diacraft_testkit::package::CT_PAGE, &contents)
        // One row spells its target as an absolute name, which the row of a page does not say of itself.
        .rel("/visio/pages/pages.xml", "rId7", &format!("{VIS}page"), "Page2.XML")
        .rel("/visio/pages/pages.xml", "rId8", &format!("{VIS}page"), "/visio/other/zz.xml")
        .part("/visio/masters/masters.xml", diacraft_testkit::package::CT_MASTERS, masters)
        .rel(DOC, "rId51", &format!("{VIS}masters"), "masters/masters.xml")
        .part("/visio/masters/m-three.xml", diacraft_testkit::package::CT_MASTER, mcontents)
        .rel("/visio/masters/masters.xml", "rId9", &format!("{VIS}master"), "m-three.xml")
        // Rows of the content types that no part needs.
        .default_type("zzz", "application/x-unused")
        .override_without_part("/visio/gone.xml", "application/x-gone");
    let (doc, _) = read_ok(&VsdxBuilder::new().without_pages_part().without_masters_part().with(pkg).build());
    let part_names: Vec<Option<String>> = doc.pages.iter().map(|p| p.part_name.as_ref().map(ToString::to_string)).collect();
    assert_eq!(part_names, [Some("/visio/pages/Page2.XML".to_owned()), Some("/visio/other/zz.xml".to_owned())], "the case a part arrived with");
    assert_eq!(doc.masters.get(0).unwrap().part_name.as_deref(), Some("/visio/masters/m-three.xml"));
    assert_eq!(doc.pages.get(0).unwrap().rel_id.as_deref(), Some("rId7"));
    // The row of the first page says what the page's fields say, so it is the page's. The second names its part by an absolute name, which no field says:
    // that row is held, with its id.
    let from_index = rels_of(&doc, "/visio/pages/pages.xml");
    assert_eq!(from_index, [("rId8".to_owned(), format!("{VIS}page"), "/visio/other/zz.xml".to_owned(), false)]);
    // The document's rows are held, as they are.
    let from_doc = rels_of(&doc, DOC);
    assert!(from_doc.contains(&("rId50".into(), format!("{VIS}pages"), "pages/pages.xml".into(), false)), "{from_doc:?}");
    assert!(from_doc.contains(&("rId51".into(), format!("{VIS}masters"), "masters/masters.xml".into(), false)), "{from_doc:?}");
    // The content-type rows no part needs: a `Default` for an extension no part has, and an `Override` for a part that is not there. The rows a part needs
    // (its `Override`, the `Default` of `rels` and of `xml`) are not held: the writer makes them for its parts.
    let rows: Vec<String> = doc
        .vsdx
        .as_ref()
        .unwrap()
        .content_types
        .iter()
        .map(|r| match r {
            diacraft_doc::ContentTypeRow::Default { extension, content_type } => format!("D {extension} {content_type}"),
            diacraft_doc::ContentTypeRow::Override { part, content_type } => format!("O {part} {content_type}"),
        })
        .collect();
    assert_eq!(rows, ["D zzz application/x-unused", "O /visio/gone.xml application/x-gone"]);
}

#[test]
fn a_page_whose_part_was_not_read_as_a_page_keeps_no_part_name_and_its_part_is_held() {
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        // Its root is not `PageContents`: read as unknown, kept, warned of.
        .part("/visio/pages/page1.xml", diacraft_testkit::package::CT_PAGE, "<Other><x/></Other>")
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    let (doc, report) = read_ok(&VsdxBuilder::new().without_pages_part().with(pkg).build());
    assert_eq!(doc.pages.get(0).unwrap().part_name, None);
    assert_eq!(&*held(&doc, PAGE1).unwrap().bytes, b"<Other><x/></Other>");
    assert_eq!(report.warning_count("part_unexpected_root"), 1);
    // The row that names it is held, since no field of the page says that part.
    assert_eq!(rels_of(&doc, "/visio/pages/pages.xml").len(), 1);
}

// ------------------------------------------------------------------------------------------------------------------ the places

/// Where a place's leftovers are held: the slot's list, and the key.
pub(crate) type Owner = fn(&Document) -> Option<&VsdxRest>;

fn doc_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.ext.get().map(|e| &e.vsdx)
}

fn page_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.pages.get(0)?.ext.get().map(|e| &e.vsdx)
}

fn master_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.masters.get(0)?.ext.get().map(|e| &e.vsdx)
}

fn top_shape_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.pages.get(0)?.shapes.get(0)?.ext.get().map(|e| &e.vsdx)
}

fn text_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.pages.get(0)?.shapes.get(0)?.text.as_ref()?.ext.get().map(|e| &e.vsdx)
}

fn foreign_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.pages.get(0)?.shapes.get(1)?.foreign.as_ref()?.ext.get().map(|e| &e.vsdx)
}

fn connect_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.pages.get(0)?.connects.iter().next()?.ext.get().map(|e| &e.vsdx)
}

fn face_rest(doc: &Document) -> Option<&VsdxRest> {
    doc.fonts.first()?.ext.get().map(|e| &e.vsdx)
}

/// The base package every place is injected into: each element the reader types and gives no slot to, once.
pub(crate) fn places_builder() -> VsdxBuilder {
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
        .with(Pkg::new().part("/visio/media/i.png", "image/png", [1u8, 2, 3]))
}

/// The kinds of thing a place holds in its own slot: an element whose attributes are all in a field (`raw`) holds no foreign attribute or declaration there, and an
/// element whose comments, text and children are the model's own (a document root's, a `Text`'s) holds none of those.
const ALL: [Kind; 6] = Kind::ALL;
const ONLY_ATTRIBUTES: [Kind; 2] = [Kind::Attribute, Kind::Declaration];
const ONLY_NODES: [Kind; 4] = [Kind::Element, Kind::Comment, Kind::Instruction, Kind::Text];

/// (the part, the element to inject into by its name or `*` for the root, the owner of what is held, the key, the kinds it holds).
pub(crate) type Place = (&'static str, &'static str, Owner, &'static str, &'static [Kind]);

pub(crate) const PLACES: &[Place] = &[
    // The document's root keeps its comments, text and children in the document's own passthrough.
    ("/visio/document.xml", "*", doc_rest, "VisioDocument", &ONLY_ATTRIBUTES),
    ("/visio/document.xml", "Colors", doc_rest, "Colors", &ALL),
    ("/visio/document.xml", "ColorEntry", doc_rest, "ColorEntry/24", &ALL),
    ("/visio/document.xml", "FaceNames", doc_rest, "FaceNames", &ALL),
    // A face's attributes are all in its `raw`.
    ("/visio/document.xml", "FaceName", face_rest, "FaceName", &ONLY_NODES),
    ("/visio/document.xml", "StyleSheets", doc_rest, "StyleSheets", &ALL),
    ("/visio/pages/pages.xml", "*", doc_rest, "Pages", &ALL),
    ("/visio/masters/masters.xml", "*", doc_rest, "Masters", &ALL),
    ("/visio/pages/pages.xml", "Rel", page_rest, "Rel", &ALL),
    ("/visio/masters/masters.xml", "Rel", master_rest, "Rel", &ALL),
    ("/visio/pages/page1.xml", "*", page_rest, "PageContents", &ALL),
    ("/visio/masters/master1.xml", "*", master_rest, "MasterContents", &ALL),
    ("/visio/pages/page1.xml", "Shapes", page_rest, "Shapes", &ALL),
    ("/visio/pages/page1.xml", "Connects", page_rest, "Connects", &ALL),
    // A connect's attributes are in its `raw`.
    ("/visio/pages/page1.xml", "Connect", connect_rest, "Connect", &ONLY_NODES),
    // A text's comments, instructions, text and children are its parts.
    ("/visio/pages/page1.xml", "Text", text_rest, "Text", &ONLY_ATTRIBUTES),
    ("/visio/pages/page1.xml", "Rel", foreign_rest, "Rel", &ALL),
];

/// Injects `kind` into the first element named `name` (or the root) of the part, and reads the package.
pub(crate) fn inject(builder: &VsdxBuilder, part: &str, name: &str, kind: Kind, nth: usize) -> Vec<u8> {
    use diacraft_opc::xml::{self, El, Node};
    fn find<'a>(e: &'a mut El, name: &str, nth: &mut usize) -> Option<&'a mut El> {
        let local = e.name.rsplit(':').next().unwrap_or("").to_owned();
        if local == name {
            if *nth == 0 {
                return Some(e);
            }
            *nth -= 1;
        }
        for c in &mut e.children {
            if let Node::El(c) = c
                && let Some(found) = find(c, name, nth)
            {
                return Some(found);
            }
        }
        None
    }
    builder
        .pkg()
        .map_parts(|n, bytes| {
            if n.eq_ignore_ascii_case(part) {
                let mut tree = xml::parse(bytes).unwrap();
                let mut nth = nth;
                if name == "*" {
                    kind.inject(&mut tree);
                } else {
                    kind.inject(find(&mut tree, name, &mut nth).unwrap_or_else(|| panic!("no {name} in {part}")));
                }
                *bytes = to_xml(&tree).into_bytes();
            }
        })
        .build()
}

#[test]
fn every_unheld_place_is_held_and_counts_zero() {
    let builder = places_builder();
    let mut checked = 0;
    for &(part, name, owner, key, kinds) in PLACES {
        for &kind in kinds {
            let bytes = inject(&builder, part, name, kind, 0);
            let (doc, report) = read_ok(&bytes);
            assert_eq!(report.skipped_counts.values().sum::<u64>(), 0, "{part} <{name}> {kind:?}: nothing is skipped: {:?}", report.skipped_counts);
            let at = owner(&doc).unwrap_or_else(|| panic!("{part} <{name}> {kind:?}: the owner's slot is empty"));
            let kept = rest(at, key);
            assert!(
                !kept.is_empty(),
                "{part} <{name}> {kind:?}: nothing is held under {key}: {:?}",
                at.0.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>()
            );
            let found = kept.iter().any(|k| dump(k).contains(kind.marker()));
            assert!(
                found,
                "{part} <{name}> {kind:?}: the marker is not in what is held under {key}: {:?}",
                kept.iter().map(|k| dump(k)).collect::<Vec<_>>()
            );
            checked += 1;
        }
    }
    assert_eq!(checked, PLACES.iter().map(|p| p.4.len()).sum::<usize>());
}

#[test]
fn a_nested_group_holds_the_leftovers_of_its_shapes_element_on_the_shape() {
    let builder = places_builder();
    let bytes = inject(&builder, "/visio/pages/page1.xml", "Shapes", Kind::Comment, 1);
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0);
    let kept = rest(top_shape_rest(&doc).expect("the group's slot"), "Shapes");
    assert_eq!(kept.len(), 1);
    assert!(dump(kept[0]).contains("zzqc"), "{}", dump(kept[0]));
    // The page's own `Shapes` holds nothing.
    assert!(page_rest(&doc).is_none_or(|r| rest(r, "Shapes").is_empty()));
}

#[test]
fn a_colour_entry_that_names_no_colour_and_a_child_that_is_not_an_entry_are_held_in_place() {
    let doc_part = |colors: &str| VsdxBuilder::new().colors(colors).page(PageSpec::new(1, "P")).build();
    let (doc, report) =
        read_ok(&doc_part(r##"<ColorEntry IX="24" RGB="#111111"/><ColorEntry IX="x" RGB="#222222"/><Note/><ColorEntry IX="25" RGB="#333333"/>"##));
    assert_eq!(doc.colors.len(), 2);
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0);
    let colors = rest(doc_rest(&doc).unwrap(), "Colors");
    assert_eq!(colors.len(), 1);
    // The two children that are no entries are held after the entries that came before them: the first after one entry, the second after one as well.
    let text = dump(colors[0]);
    assert!(text.contains("[after 1] <n><ColorEntry IX=\"x\" RGB=\"#222222\"/></n>"), "{text}");
    assert!(text.contains("[after 1] <n><Note/></n>"), "{text}");
}

#[test]
fn a_style_without_an_id_a_page_without_an_id_and_a_master_shortcut_are_held_in_place() {
    let styles = r#"<StyleSheet ID="1" Name="A"/><StyleSheet Name="no id"/><StyleSheet ID="2" Name="B"/>"#;
    let (doc, report) = read_ok(&VsdxBuilder::new().more_styles(styles).page(PageSpec::new(1, "P")).build());
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0);
    let kept = rest(doc_rest(&doc).unwrap(), "StyleSheets");
    // The root style and the style 1 came before it: two typed children.
    assert!(dump(kept[0]).contains("[after 2] <n><StyleSheet Name=\"no id\"/></n>"), "{}", dump(kept[0]));
    let masters = format!(
        "<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Master ID=\"1\" NameU=\"M\"/><MasterShortcut ID=\"9\" ShortcutURL=\"u\"/><Master NameU=\"no id\"/></Masters>"
    );
    let pkg = Pkg::new().part("/visio/masters/masters.xml", diacraft_testkit::package::CT_MASTERS, masters).rel(
        DOC,
        "rId51",
        &format!("{VIS}masters"),
        "masters/masters.xml",
    );
    let (doc, report) = read_ok(&VsdxBuilder::new().without_masters_part().with(pkg).build());
    assert_eq!(report.skipped_counts.values().sum::<u64>(), 0);
    let kept = rest(doc_rest(&doc).unwrap(), "Masters");
    let text = dump(kept[0]);
    assert!(text.contains("[after 1] <n><MasterShortcut ID=\"9\" ShortcutURL=\"u\"/></n>"), "{text}");
    assert!(text.contains("[after 1] <n><Master NameU=\"no id\"/></n>"), "{text}");
}

// ------------------------------------------------------------------------------------------------------- what a save does with it

/// The length and the CRC-32 of the bytes an unedited splice save of [`everything_package`] writes, as the save wrote them before the reader held anything
/// The pinned figures are of the last commit before it. The one part it rewrites is `docProps/app.xml`, which names DiaCraft as the writer.
const UNEDITED_SAVE: (usize, u32) = (9203, 2449210538);

#[test]
fn an_unedited_splice_save_is_byte_identical_as_before() {
    let bytes = everything_package();
    let (doc, _) = read_ok(&bytes);
    let written = write_with(&doc, Some(&OriginalBytes { bytes: &bytes, baseline: None }), &WriteOptions::default()).unwrap();
    assert_eq!(written.rewritten, ["/docProps/app.xml"], "{written:?}");
    assert!(written.added.is_empty() && written.removed.is_empty(), "{written:?}");
    let crc = crc32fast::hash(&written.bytes);
    eprintln!("[held] unedited save: {} bytes, crc {crc}", written.bytes.len());
    assert_eq!((written.bytes.len(), crc), UNEDITED_SAVE);
    // Every other part is the file's, byte for byte.
    let before = Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    let after = Package::read(&written.bytes, &diacraft_opc::Limits::default()).unwrap();
    for name in before.parts().filter(|n| !n.eq_ignore_ascii_case("/docProps/app.xml")) {
        assert_eq!(before.part(name), after.part(name), "{name}");
    }
    // And a second save of what the first read back is the same bytes: a save is no edit.
    let again =
        write_with(&written.document, Some(&OriginalBytes { bytes: &written.bytes, baseline: Some(&written.document) }), &WriteOptions::default())
            .unwrap();
    assert_eq!(again.bytes, written.bytes);
}

#[test]
fn the_same_file_reads_to_the_same_held_model_twice() {
    let bytes = everything_package();
    let (a, ra) = read_ok(&bytes);
    let (b, rb) = read_ok(&bytes);
    assert_eq!(a, b);
    assert_eq!((ra.held_parts, ra.held_bytes), (rb.held_parts, rb.held_bytes));
    // `read` and `read_partial` give the same document.
    let (c, _) = read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert_eq!(a, c);
}

#[test]
fn a_package_with_nothing_unmodelled_holds_no_part() {
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(r#"<Shape ID="1"/>"#)).build());
    assert!(doc.parts.is_empty(), "{:?}", held_names(&doc));
    assert_eq!((report.held_parts, report.held_bytes), (0, 0));
    // The relationships of the package's own parts are held (the document's, the properties'), and the rows of the pages are the pages'.
    let vsdx = doc.vsdx.as_ref().expect("a document read from a package holds what it has");
    assert!(vsdx.rels.iter().any(|r| &*r.kind == format!("{VIS}document").as_str()));
    assert!(vsdx.content_types.is_empty(), "{:?}", vsdx.content_types);
    assert!(rels_of(&doc, "/visio/pages/pages.xml").is_empty());
}

#[test]
fn the_bytes_of_a_held_part_are_the_packages_and_are_counted_once() {
    // F1: the bytes of a held part move out of the package into the document, so the pool counts them once, as the package it deducted at the start, and
    // the model's figure gains them when they move (the ledger of the process holds the model's own).
    let page = || PageSpec::new(1, "P").shapes(r#"<Shape ID="1"/>"#);
    let without = VsdxBuilder::new().page(page()).build();
    let with = VsdxBuilder::new().page(page()).with(Pkg::new().part("/x/big.bin", "application/octet-stream", vec![7u8; 1_000_000])).build();
    let (doc0, r0) = read_ok(&without);
    let (doc1, r1) = read_ok(&with);
    assert!(doc0.parts.is_empty() && doc1.parts.len() == 1);
    assert_eq!(&*doc1.parts.get(0).unwrap().bytes, &vec![7u8; 1_000_000][..]);
    // The model is charged for the part's bytes and for the bookkeeping of a part, a few hundred bytes: not for a copy more, and not for less.
    let added = r1.stats.memory_bytes - r0.stats.memory_bytes;
    assert!((1_000_000..1_000_000 + 2_000).contains(&added), "{added}");
    assert_eq!(r1.held_bytes, 1_000_000);
    // The pool never held them as a second copy: its peak is the drawing's, a megabyte under the figure the model ends with.
    assert!(r1.stats.memory_peak + 900_000 < r1.stats.memory_bytes, "{:?}", r1.stats);
    // The move makes one copy of a part at a time (the model's bytes are an `Arc<[u8]>`, which cannot take over a `Vec`), and the pool is asked for it before any part moves:
    // a pool that holds the drawing and no room for a megabyte more is refused in words, and one with the room reads the file.
    let room = with.len() as u64 + r1.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    let (result, _) =
        diacraft_vsdx::read_partial(&with, &diacraft_vsdx::Limits { max_memory: room + r1.stats.memory_peak, ..diacraft_vsdx::Limits::default() });
    let error = result.expect_err("the copy of the part does not fit");
    assert_eq!(error.class(), "too_much_memory", "{error}");
    assert!(error.to_string().contains("bytes of memory"), "{error}");
    let (result, report) = diacraft_vsdx::read_partial(
        &with,
        &diacraft_vsdx::Limits { max_memory: room + r1.stats.memory_peak + 1_100_000, ..diacraft_vsdx::Limits::default() },
    );
    let doc = result.expect("a pool with room for the copy reads the file");
    assert_eq!((doc.parts.len(), report.held_bytes), (1, 1_000_000));
    // The largest part is the one copy: two parts of half a megabyte are read in the pool that has room for one of them and not for the whole megabyte.
    let two = VsdxBuilder::new()
        .page(page())
        .with(Pkg::new().part("/x/a.bin", "application/octet-stream", vec![1u8; 500_000]).part(
            "/x/b.bin",
            "application/octet-stream",
            vec![2u8; 500_000],
        ))
        .build();
    let (_, r2) = read_ok(&two);
    let room = two.len() as u64 + r2.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    let (result, _) = diacraft_vsdx::read_partial(
        &two,
        &diacraft_vsdx::Limits { max_memory: room + r2.stats.memory_peak + 600_000, ..diacraft_vsdx::Limits::default() },
    );
    assert_eq!(result.expect("one copy at a time").parts.len(), 2);
}

#[test]
fn a_relationships_part_that_cannot_be_parsed_is_held_as_the_bytes_it_is_whoever_its_source_is() {
    // The relationships part of a page part the model rebuilds, and the one of a part it holds: neither is lost, and the file opens.
    let broken = b"<Relationships><not closed";
    let extra = Pkg::new()
        .part("/visio/embeddings/o.bin", CT_OLE, [1u8])
        .item(diacraft_testkit::package::ZipItem::deflated("visio/embeddings/_rels/o.bin.rels", broken))
        .item(diacraft_testkit::package::ZipItem::deflated("visio/pages/_rels/page1.xml.rels", broken));
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(r#"<Shape ID="1"/>"#)).with(extra).build());
    assert_eq!(report.warning_count("rels_unreadable"), 2);
    let names = held_names(&doc);
    assert!(names.contains("/visio/embeddings/_rels/o.bin.rels") && names.contains("/visio/pages/_rels/page1.xml.rels"), "{names:?}");
    assert_eq!(&*held(&doc, "/visio/pages/_rels/page1.xml.rels").unwrap().bytes, broken);
    assert_eq!(&*held(&doc, "/visio/embeddings/_rels/o.bin.rels").unwrap().bytes, broken);
    assert!(held(&doc, "/visio/embeddings/o.bin").unwrap().rels.is_empty());
}

#[test]
fn a_relationships_part_whose_source_is_not_in_the_package_is_held_as_bytes() {
    let rels = b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";
    let extra = Pkg::new().item(diacraft_testkit::package::ZipItem::deflated("visio/_rels/gone.xml.rels", rels));
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build());
    assert_eq!(report.warning_count("opc_orphan_relationships"), 1);
    let part = held(&doc, "/visio/_rels/gone.xml.rels").expect("an orphan is kept as the part it is");
    assert_eq!((&*part.bytes, part.role), (&rels[..], PartRole::Other));
}
