//! What a package holds that a drawing in another format has no place for:
//! the macro project, the embedded objects and the other parts. Every package is built in code; the VBA bytes are made up and nothing is run.

use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Holds, Limits, holds, holds_of};

const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const SIG_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature";
const VBA_TYPE: &str = "application/vnd.ms-office.vbaProject";
const SIG_TYPE: &str = "application/vnd.ms-office.vbaProjectSignature";
const MACRO_DOCUMENT: &str = "application/vnd.ms-visio.drawing.macroEnabled.main+xml";

fn base() -> VsdxBuilder {
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))))
}

fn project(builder: VsdxBuilder) -> VsdxBuilder {
    builder.with(
        Pkg::new()
            .default_type("bin", VBA_TYPE)
            .part_without_override("/visio/vbaProject.bin", b"not code")
            .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
            .part("/visio/vbaProjectSignature.bin", SIG_TYPE, b"sig")
            .rel("/visio/vbaProject.bin", "rId1", SIG_REL, "vbaProjectSignature.bin"),
    )
}

fn found(bytes: &[u8]) -> Holds {
    holds(bytes, &Limits::default()).unwrap()
}

/// A drawing with pages, masters and pictures holds none of the three: what the model holds, or carries in another way, is not counted.
#[test]
fn a_plain_package_holds_nothing_that_another_format_lacks_a_place_for() {
    assert_eq!(found(&base().build()), Holds::default());
}

/// The project is found by the relationship from the document part, with the signature part the project's own relationship reaches; both are
/// named with their slash, once each.
#[test]
fn the_project_and_its_signature_part_are_found_by_their_relationships() {
    let h = found(&project(base().document_type(MACRO_DOCUMENT)).build());
    assert_eq!(h.macro_parts, ["/visio/vbaProject.bin", "/visio/vbaProjectSignature.bin"]);
    assert_eq!((h.object_parts, h.other_parts), (0, 0), "{h:?}");
}

/// A macro-enabled type with no project is normal, and holds no macro.
#[test]
fn a_macro_enabled_type_with_no_project_holds_none() {
    let h = found(&base().document_type(MACRO_DOCUMENT).build());
    assert!(h.macro_parts.is_empty(), "{h:?}");
}

/// A part of a VBA content type is VBA whether or not a relationship reaches it (the last sentence); it is named once even when it is reached both ways, and a
/// relationship to a part the package does not have, or an external one, names nothing.
#[test]
fn a_part_of_a_vba_type_is_found_without_a_relationship_and_a_relationship_to_nothing_finds_nothing() {
    let loose = base().with(Pkg::new().part("/visio/stray.bin", VBA_TYPE, b"x"));
    assert_eq!(found(&loose.build()).macro_parts, ["/visio/stray.bin"]);
    let twice = project(base()).with(Pkg::new().rel("/", "rId9", VBA_REL, "visio/vbaProject.bin"));
    assert_eq!(found(&twice.build()).macro_parts, ["/visio/vbaProject.bin", "/visio/vbaProjectSignature.bin"]);
    let missing = base().with(Pkg::new().rel("/visio/document.xml", "rId50", VBA_REL, "nothing-here.bin"));
    assert!(found(&missing.build()).macro_parts.is_empty());
}

/// Embedded objects and ActiveX controls are counted by their folders (their relationships parts are not parts), in any case; any other part the model
/// does not hold is one more of the others; pages, masters, themes, pictures and properties are neither.
#[test]
fn embedded_objects_and_the_other_parts_are_counted_by_where_they_are() {
    let builder = base().with(
        Pkg::new()
            .part("/visio/embeddings/Object1.bin", "application/vnd.openxmlformats-officedocument.oleObject", b"ole")
            .part("/visio/EMBEDDINGS/two.bin", "application/vnd.openxmlformats-officedocument.oleObject", b"ole")
            .part("/visio/activeX/a.bin", "application/x-activex", b"ax")
            .part("/visio/solutions/s.xml", "application/xml", b"<a/>")
            .part("/customXml/item1.xml", "application/xml", b"<a/>")
            .part_without_override("/foo/bar.dat", b"0123456789")
            .default_type("dat", "application/x-foo"),
    );
    let h = found(&builder.build());
    assert_eq!((h.object_parts, h.other_parts), (3, 3), "{h:?}");
    assert!(h.macro_parts.is_empty());
}

/// Hostile input is the container's refusal and nothing else: not a package, and a package past a limit.
#[test]
fn bytes_that_are_not_a_package_are_the_containers_error() {
    let e = holds(b"not a zip at all", &Limits::default()).unwrap_err();
    assert_eq!(e.class(), "not_zip");
    let limits = Limits { opc: OpcLimits { max_parts: 2, ..OpcLimits::default() }, ..Limits::default() };
    assert!(Package::read(&base().build(), &limits.opc).is_err());
    assert!(holds(&base().build(), &limits).is_err());
}

/// The document part is the target of the document relationship, whatever other relationships come before it in the package's own: a package built by hand, whose first
/// relationship is the core properties' and whose project (typed as opaque bytes) is reached only from the document part, has its project found.
#[test]
fn the_document_relationship_is_found_whatever_comes_before_it() {
    use diacraft_testkit::package::{ZipItem, zip};
    const DOCUMENT: &str = "http://schemas.microsoft.com/visio/2010/relationships/document";
    const CORE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    let rels = |rows: &str| format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"{RELS}\">{rows}</Relationships>");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>";
    let root = rels(&format!(
        "<Relationship Id=\"rId9\" Type=\"{CORE}\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId1\" Type=\"{DOCUMENT}\" Target=\"visio/document.xml\"/>"
    ));
    let document = rels(&format!("<Relationship Id=\"rId50\" Type=\"{VBA_REL}\" Target=\"vbaProject.bin\"/>"));
    let bytes = zip(&[
        ZipItem::deflated("[Content_Types].xml", types.as_bytes()),
        ZipItem::deflated("_rels/.rels", root.as_bytes()),
        ZipItem::deflated("docProps/core.xml", b"<a/>"),
        ZipItem::deflated("visio/document.xml", b"<a/>"),
        ZipItem::deflated("visio/_rels/document.xml.rels", document.as_bytes()),
        ZipItem::deflated("visio/vbaProject.bin", b"not code"),
    ]);
    assert_eq!(found(&bytes).macro_parts, ["/visio/vbaProject.bin"]);
}

/// The project and its signature are found by their relationships, whatever content type the package gives them (a part reached by the project's relationship is the
/// project's, and one the project's own relationships reach by another kind of relationship is not): a project and a signature typed as opaque bytes are found, and a part
/// the project relates to as an object is one of the others.
#[test]
fn a_project_is_found_by_its_relationships_though_its_content_type_says_nothing() {
    let opaque = "application/octet-stream";
    let builder = base().document_type(MACRO_DOCUMENT).with(
        Pkg::new()
            .part("/visio/vbaProject.bin", opaque, b"not code")
            .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
            .part("/visio/sig.bin", opaque, b"sig")
            .part("/visio/other.dat", opaque, b"other")
            .rel("/visio/vbaProject.bin", "rId1", SIG_REL, "sig.bin")
            .rel("/visio/vbaProject.bin", "rId2", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject", "other.dat"),
    );
    let h = found(&builder.build());
    assert_eq!(h.macro_parts, ["/visio/vbaProject.bin", "/visio/sig.bin"]);
    assert_eq!((h.object_parts, h.other_parts), (0, 1), "{h:?}");
    // A relationship to the project that is external reaches no part.
    let external = base().document_type(MACRO_DOCUMENT).with(Pkg::new().part("/visio/vbaProject.bin", opaque, b"x").rel_external(
        "/visio/document.xml",
        "rId50",
        VBA_REL,
        "http://example.invalid/p",
    ));
    assert!(found(&external.build()).macro_parts.is_empty());
}

/// **The model holds what the package holds**: the engine asks the model, and the bytes are the independent measure. For a package with a project, its signature part and an embedding, the
/// reader's model has the same project parts and the same number of object parts as the package says.
#[test]
fn the_model_holds_what_the_package_holds() {
    let builder = project(base().document_type(MACRO_DOCUMENT)).with(Pkg::new().part(
        "/visio/embeddings/Object1.bin",
        "application/vnd.openxmlformats-officedocument.oleObject",
        b"ole",
    ));
    let bytes = builder.build();
    let (doc, _) = diacraft_vsdx::read(&bytes, &Limits::default()).unwrap();
    let (from_bytes, from_model) = (found(&bytes), holds_of(&doc));
    let sorted = |mut names: Vec<String>| {
        names.sort();
        names
    };
    assert_eq!(sorted(from_model.macro_parts.clone()), sorted(from_bytes.macro_parts.clone()));
    assert_eq!(from_model.object_parts, from_bytes.object_parts);
    assert_eq!(holds_of(&diacraft_vsdx::read(&base().build(), &Limits::default()).unwrap().0), Holds::default());
}

/// **A package whose relationships all name one project is looked at once** (the security scan's finding: the project was pushed once for each relationship that named it, and the second loop parsed its
/// relationships part for each). 60,000 relationships (30,000 of the root and 30,000 of the document part: a relationships part is at most 4 MiB) name `vbaProject.bin`, the project's own relationships part holds 50 rows of signatures: the answer is the project and its
/// signature parts, once each, and **three relationships parts are parsed** (the root's, the document's and the project's, once), with the items of their trees within a small constant of the rows the
/// package holds (the old loop parsed the project's 60,000 times).
#[test]
fn a_project_that_60000_relationships_name_is_looked_into_once() {
    let wrap = |rows: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rows}</Relationships>"
        )
    };
    let mut project_rows = String::new();
    for k in 0..50 {
        project_rows.push_str(&format!("<Relationship Id=\"s{k}\" Type=\"{SIG_REL}\" Target=\"sig{k}.bin\"/>"));
    }
    let mut pkg = Pkg::new().default_type("bin", VBA_TYPE).part_without_override("/visio/vbaProject.bin", b"not code").part(
        "/visio/_rels/vbaProject.bin.rels",
        "application/vnd.openxmlformats-package.relationships+xml",
        wrap(&project_rows).as_bytes(),
    );
    // (A relationships part is at most 4 MiB, about 30,000 rows: the rows are spread over the root's relationships and the document's, 60,000 in all, which is what one package can name one project by.)
    for k in 0..30_000 {
        pkg = pkg.rel("/visio/document.xml", &format!("n{k}"), VBA_REL, "vbaProject.bin").rel("/", &format!("m{k}"), VBA_REL, "visio/vbaProject.bin");
    }
    for k in 0..50 {
        pkg = pkg.part(&format!("/visio/sig{k}.bin"), SIG_TYPE, b"sig");
    }
    let builder = base().document_type(MACRO_DOCUMENT).with(pkg);
    let (h, work) = diacraft_vsdx::holds_counted(&builder.build(), &Limits::default()).unwrap();
    assert_eq!(h.macro_parts.first().map(String::as_str), Some("/visio/vbaProject.bin"), "{:?}", h.macro_parts);
    assert_eq!(h.macro_parts.iter().filter(|p| p.as_str() == "/visio/vbaProject.bin").count(), 1, "named by 60,000 relationships, found once");
    assert_eq!(work.parts_parsed, 3, "the root's, the document's and the project's relationships, once each: {work:?}");
    assert!(work.items <= 60_000 + 1_000, "the items of the three trees are the rows of the package: {work:?}");
}
