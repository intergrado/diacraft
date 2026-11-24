//! The reader keeps the package, `meta.xml`, `settings.xml` and the font faces in the model, so that a drawing opened from an `.odg` can be saved as `.diacraft` without
//! losing what the `.odg` holds beside its drawing (the native-format design's §3.4 and §4.2). Every input is built here, by hand, from the ODF standard (Part 2 for the package and the
//! manifest, Part 3 for `meta.xml`, `settings.xml` and the font faces).
//!
//! * the font faces: one face for each distinct declaration, the declaration held exactly;
//! * `meta.xml`: the fields both formats say are the title and the properties, the rest is kept whole where it stood;
//! * `settings.xml`: the whole tree;
//! * the roots, `mimetype`, and the manifest's rows, on the parts they name and by name;
//! * the package entries nothing reads: the parts of the model, their bytes the package's own.
//!
//! The crafted inputs and their costs are in `held_package_cost.rs`, and the memory of the largest in `held_package_memory.rs`.

use std::sync::Arc;

use diacraft_doc::{Document, Format, PartRole, RawNode};
use diacraft_odg::{Limits, read, read_partial};
use diacraft_sheetxml::model::{Budget, MemBlobs, decode, encode};

use crate::build::{DRAWING, write};
use crate::drawing::{Drawing, NS, without_package};
use crate::write_core::{open_bytes, real};

/// The manifest's namespace declaration.
const MANIFEST_NS: &str = r#"xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0""#;

/// A manifest of the rows `rows`, with a namespace of its own declared on the root.
pub fn manifest(rows: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest {MANIFEST_NS} manifest:version=\"1.3\" xmlns:x=\"urn:example:extension\">{rows}</manifest:manifest>"
    )
}

/// The package of `d` with `more` entries (a name ending in `/` is a folder), and the manifest `rows` in place of the one the tests' writer makes.
pub fn package_with_rows(d: &Drawing, more: &[(&str, &[u8])], rows: &str) -> Vec<u8> {
    let mut entries = d.files();
    entries.extend(more.iter().map(|(n, b)| ((*n).to_owned(), b.to_vec())));
    entries.push(("META-INF/manifest.xml".to_owned(), manifest(rows).into_bytes()));
    write(Some(DRAWING), &entries)
}

/// A `meta.xml` whose root declares the namespaces the tests use, and `x` besides, around the fields `fields`.
pub fn meta_of(fields: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} xmlns:x=\"urn:example:extension\" office:version=\"1.3\">\n <office:meta>{fields}\n </office:meta>\n</office:document-meta>"
    )
}

fn read_ok(bytes: &[u8]) -> (Document, diacraft_odg::ReadReport) {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()))
}

fn attrs_of(attrs: &diacraft_doc::RawAttrs) -> Vec<(&str, &str)> {
    attrs.iter().collect()
}

fn element_names(kept: &diacraft_doc::Passthrough) -> Vec<(u32, String)> {
    kept.iter()
        .map(|p| {
            let what = match &p.node {
                RawNode::Element(e) => e.name.to_string(),
                RawNode::Comment(c) => format!("<!--{c}-->"),
                RawNode::Text(t) => format!("text {t}"),
                RawNode::Pi { target, .. } => format!("<?{target}"),
            };
            (p.after, what)
        })
        .collect()
}

// ---- the font faces --------------------------------------------------------------------------------------------------------------------

/// A face is a declaration held exactly: every attribute in order and every child, in the face's `odf` slot, and the family it names (quotes off) is what the face is called, which is
/// what a run's font cell holds.
#[test]
fn a_font_face_is_a_declaration_held_exactly_and_is_known_by_its_family() {
    let mut d = Drawing::a4().page("");
    d.fonts = concat!(
        r#"<style:font-face style:name="Liberation Sans" svg:font-family="&apos;Liberation Sans&apos;" style:font-family-generic="swiss" style:font-pitch="variable"/>"#,
        r#"<style:font-face style:name="Big" svg:font-family="Big"><svg:font-face-src><svg:font-face-uri xlink:href="Fonts/big.ttf" xlink:type="simple"><svg:font-face-format svg:string="truetype"/></svg:font-face-uri></svg:font-face-src></style:font-face>"#,
    )
    .to_owned();
    let (doc, _) = d.read();
    assert_eq!(doc.fonts.len(), 2, "both parts declare the two faces: each is held once");
    let (sans, big) = (&doc.fonts[0], &doc.fonts[1]);
    assert_eq!((&*sans.name_u, &*big.name_u), ("Liberation Sans", "Big"));
    assert!(sans.raw.is_empty() && big.raw.is_empty(), "the VSDX attributes of a face are empty");
    let odf = &sans.ext.get().expect("the declaration is held").odf;
    assert_eq!(
        attrs_of(&odf.raw),
        [
            ("style:name", "Liberation Sans"),
            ("svg:font-family", "'Liberation Sans'"),
            ("style:font-family-generic", "swiss"),
            ("style:font-pitch", "variable"),
        ],
        "every attribute, in order, the value as written once its entities are resolved"
    );
    assert!(odf.kept.is_empty());
    let odf = &big.ext.get().expect("the declaration is held").odf;
    assert_eq!(attrs_of(&odf.raw), [("style:name", "Big"), ("svg:font-family", "Big")]);
    assert_eq!(element_names(&odf.kept), [(0, "svg:font-face-src".to_owned())], "the child, whole");
    let Some(RawNode::Element(src)) = odf.kept.iter().next().map(|p| &p.node) else { panic!("an element") };
    let Some(RawNode::Element(uri)) = src.children.first() else { panic!("the uri") };
    assert_eq!(attrs_of(&diacraft_doc::RawAttrs::from_pairs(uri.attrs.iter())), [("xlink:href", "Fonts/big.ttf"), ("xlink:type", "simple")]);
}

/// A declaration equal to one held is the same face, and one that differs, by an attribute or by a child, is another: two faces of one name stay two.
#[test]
fn an_equal_declaration_is_one_face_and_a_different_one_is_another() {
    let mut d = Drawing::a4().page("");
    d.fonts = concat!(
        r#"<style:font-face style:name="A" svg:font-family="A" style:font-pitch="fixed"/>"#,
        r#"<style:font-face style:name="A" svg:font-family="A" style:font-pitch="variable"/>"#,
        r#"<style:font-face style:name="B" svg:font-family="B"><svg:font-face-src><svg:font-face-uri xlink:href="Fonts/one.ttf"/></svg:font-face-src></style:font-face>"#,
        r#"<style:font-face style:name="B" svg:font-family="B"><svg:font-face-src><svg:font-face-uri xlink:href="Fonts/two.ttf"/></svg:font-face-src></style:font-face>"#,
        r#"<style:font-face style:name="B" svg:font-family="B"><svg:font-face-src><svg:font-face-uri xlink:href="Fonts/two.ttf"/></svg:font-face-src></style:font-face>"#,
    )
    .to_owned();
    let (doc, report) = d.read();
    assert_eq!(doc.fonts.iter().map(|f| &*f.name_u).collect::<Vec<_>>(), ["A", "A", "B", "B"], "five declarations in each part, four distinct faces");
    // The reader's own lookup by name (the first of a name wins) is as it was.
    assert_eq!(report.warned("style_name_repeated"), 0);
}

/// A declaration with no family is known by its name, and one with neither by the empty name.
#[test]
fn a_face_with_no_family_is_known_by_its_name() {
    let mut d = Drawing::a4().page("");
    d.fonts = r#"<style:font-face style:name="Nameless"/><style:font-face svg:font-family="  &quot;Quoted&quot; "/><style:font-face/>"#.to_owned();
    let (doc, _) = d.read();
    assert_eq!(doc.fonts.iter().map(|f| &*f.name_u).collect::<Vec<_>>(), ["Nameless", "Quoted", ""]);
}

/// The font table is what the other readers of it find: the engine looks a face up by the family a run names.
#[test]
fn the_face_table_of_a_flat_drawing_is_the_package_s() {
    let mut d = Drawing::a4().page("");
    d.fonts = r#"<style:font-face style:name="F" svg:font-family="F" style:font-pitch="fixed"/>"#.to_owned();
    let package = read_ok(&d.bytes()).0;
    let flat = read_ok(&crate::flat::Flat::of(&d).bytes()).0;
    assert_eq!(package.fonts.len(), 1);
    assert_eq!(package.fonts, flat.fonts);
}

// ---- meta.xml --------------------------------------------------------------------------------------------------------------------------

fn full_meta() -> String {
    meta_of(concat!(
        "\n  <meta:generator>LibreOffice/7.3$Linux</meta:generator>",
        "\n  <dc:title>My title</dc:title>",
        "\n  <dc:description>A description</dc:description>",
        "\n  <dc:subject>A subject</dc:subject>",
        "\n  <meta:keyword>k1</meta:keyword>",
        "\n  <meta:keyword>k2</meta:keyword>",
        "\n  <meta:initial-creator>Ann</meta:initial-creator>",
        "\n  <dc:creator>Bob</dc:creator>",
        "\n  <meta:creation-date>2024-05-06T07:08:09.123456789</meta:creation-date>",
        "\n  <dc:date>2025-01-02T03:04:05</dc:date>",
        "\n  <meta:print-date>2025-02-03T00:00:00</meta:print-date>",
        "\n  <dc:language>en-US</dc:language>",
        "\n  <meta:editing-cycles>3</meta:editing-cycles>",
        "\n  <meta:editing-duration>PT5M</meta:editing-duration>",
        "\n  <meta:document-statistic meta:object-count=\"4\"/>",
        "\n  <meta:user-defined meta:name=\"Project\" meta:value-type=\"string\">Alpha</meta:user-defined>",
        "\n  <meta:user-defined meta:name=\"Rev\" meta:value-type=\"float\" x:note=\"n\">2</meta:user-defined>",
        "\n  <meta:user-defined meta:name=\"Bare\"/>",
        "\n  <!-- a comment -->",
        "\n  <dc:title>a second title</dc:title>",
        "\n  <meta:user-defined>no name</meta:user-defined>",
        "\n  <dc:subject xml:lang=\"de\">a subject with an attribute</dc:subject>"
    ))
}

/// The fields both formats say are the title and the properties; each user-defined field is a custom property of the `.odg`; everything else is kept whole, in place, each node tied to
/// the number of typed fields before it; and white space between the fields, which is the file's layout, is no content.
#[test]
fn meta_fields_are_the_title_and_the_properties_and_the_rest_is_kept_in_place() {
    let mut d = Drawing::a4().page("");
    d.meta = Some(full_meta());
    let (doc, report) = d.read();
    assert_eq!(&*doc.title, "My title");
    let p = &*doc.props;
    let text = |t: &Option<Arc<str>>| t.as_deref().map(str::to_owned);
    assert_eq!(text(&p.description).as_deref(), Some("A description"));
    assert_eq!(text(&p.subject).as_deref(), Some("A subject"));
    assert_eq!(text(&p.keywords).as_deref(), Some("k1"), "the first keyword; the others are kept");
    assert_eq!(text(&p.creator).as_deref(), Some("Ann"), "the initial creator is the creator");
    assert_eq!(text(&p.last_modified_by).as_deref(), Some("Bob"), "the creator of ODF is who saved it last");
    assert_eq!(text(&p.created).as_deref(), Some("2024-05-06T07:08:09.123456789"));
    assert_eq!(text(&p.modified).as_deref(), Some("2025-01-02T03:04:05"));
    assert_eq!(text(&p.last_printed).as_deref(), Some("2025-02-03T00:00:00"));
    assert_eq!(text(&p.language).as_deref(), Some("en-US"));
    assert_eq!(p.category, None);
    let custom: Vec<_> = p.custom.iter().map(|c| (c.format, &*c.name, &*c.value_type, &*c.value, attrs_of(&c.raw))).collect();
    assert_eq!(
        custom,
        [
            (Format::Odf, "Project", "string", "Alpha", vec![]),
            (Format::Odf, "Rev", "float", "2", vec![("x:note", "n")]),
            (Format::Odf, "Bare", "", "", vec![]),
        ]
    );
    let odf = &doc.ext.get().expect("the document's ODF data").odf;
    // The typed fields are the title, description, subject, first keyword, initial creator, creator, the three dates, the language and the three user-defined: 13.
    assert_eq!(
        element_names(&odf.meta),
        [
            (0, "meta:generator".to_owned()),
            (4, "meta:keyword".to_owned()),
            (10, "meta:editing-cycles".to_owned()),
            (10, "meta:editing-duration".to_owned()),
            (10, "meta:document-statistic".to_owned()),
            (13, "<!-- a comment -->".to_owned()),
            (13, "dc:title".to_owned()),
            (13, "meta:user-defined".to_owned()),
            (13, "dc:subject".to_owned()),
        ],
        "kept whole, each after the number of typed fields before it: a second keyword and title, a user-defined with no name, a subject with an attribute"
    );
    assert_eq!(report.generator.as_deref(), Some("LibreOffice/7.3$Linux"), "the generator is read as it was");
    // `meta.xml` is read into fields, so it is no entry of the package.
    assert!(doc.parts.iter().all(|p| &*p.name != "meta.xml"));
    // The root's attributes and declarations are held, in order.
    let root = odf.roots.iter().find(|(n, _)| &**n == "meta.xml").map(|(_, a)| attrs_of(a)).expect("the root of meta.xml");
    assert!(root.contains(&("xmlns:x", "urn:example:extension")) && root.contains(&("office:version", "1.3")), "{root:?}");
    assert_eq!(root.last(), Some(&("office:version", "1.3")));
}

/// A `meta.xml` whose shape the fields cannot hold without losing something (an `office:meta` with an attribute of its own) is not read into fields: it is an entry, kept as bytes.
#[test]
fn a_meta_that_the_fields_cannot_hold_is_kept_as_an_entry_of_the_package() {
    let text = meta_of("<dc:title>T</dc:title>").replace("<office:meta>", "<office:meta xml:lang=\"en\">");
    let mut d = Drawing::a4().page("");
    d.meta = Some(text.clone());
    let (doc, _) = d.read();
    assert_eq!(&*doc.title, "", "no field of it is read");
    assert_eq!(*doc.props, diacraft_doc::DocProps::default());
    let part = doc.parts.iter().find(|p| &*p.name == "meta.xml").expect("meta.xml is an entry");
    assert_eq!(part.bytes.as_ref(), text.as_bytes());
    assert!(doc.ext.get().is_none_or(|d| d.odf.meta.is_empty() && !d.odf.roots.iter().any(|(n, _)| &**n == "meta.xml")));
}

/// A field with no text is not typed (an empty string is no title): it is kept whole.
#[test]
fn an_empty_field_is_kept_and_is_no_property() {
    let mut d = Drawing::a4().page("");
    d.meta = Some(meta_of("<dc:title/><dc:language></dc:language><dc:title>real</dc:title>"));
    let (doc, _) = d.read();
    assert_eq!(&*doc.title, "real");
    assert_eq!(doc.props.language, None);
    let odf = &doc.ext.get().expect("data").odf;
    assert_eq!(element_names(&odf.meta), [(0, "dc:title".to_owned()), (0, "dc:language".to_owned())]);
}

// ---- settings.xml ----------------------------------------------------------------------------------------------------------------------

/// `settings.xml` is held whole, as a tree, whatever it holds; the drawing's scale is read from it as it was.
#[test]
fn settings_are_held_whole() {
    let items = concat!(
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item>"#,
        r#"<config:config-item config:name="ScaleDenominator" config:type="int">2</config:config-item>"#,
        r#"<config:config-item config:name="IsSnapToGrid" config:type="boolean">true</config:config-item>"#,
    );
    let mut d = Drawing::a4().page("");
    d.settings = Some(crate::drawing::settings(items));
    let (doc, _) = d.read();
    let odf = &doc.ext.get().expect("data").odf;
    let root = odf.settings.as_ref().expect("the tree");
    assert_eq!(&*root.name, "office:document-settings");
    assert_eq!(root.attrs.get("office:version"), Some("1.3"));
    assert_eq!(
        root.attrs.get("xmlns:config"),
        Some("urn:oasis:names:tc:opendocument:xmlns:config:1.0"),
        "the namespace declarations are the root's attributes"
    );
    let Some(RawNode::Element(settings)) = root.children.first() else { panic!("office:settings") };
    let Some(RawNode::Element(set)) = settings.children.first() else { panic!("the item set") };
    assert_eq!(set.attrs.get("config:name"), Some("ooo:configuration-settings"));
    assert_eq!(set.children.len(), 3);
    let Some(RawNode::Element(item)) = set.children.last() else { panic!("an item") };
    assert_eq!((item.attrs.get("config:name"), item.children.first()), (Some("IsSnapToGrid"), Some(&RawNode::Text(Arc::from("true")))));
    assert!(doc.parts.iter().all(|p| &*p.name != "settings.xml"), "read into a tree, it is no entry");
    // The scale is still read from it, into the page's cells.
    let scale = doc.pages.get(0).and_then(|p| p.sheet.cell("DrawingScale")).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number);
    assert!(scale.is_some());
}

/// A `settings.xml` past the cap of a small part is not read into a tree: it is an entry of the package, kept as bytes that are the package's own, so that none of them is held twice.
#[test]
fn a_settings_part_past_the_cap_is_an_entry_sharing_the_packages_bytes() {
    let mut d = Drawing::a4().page("");
    let settings = crate::drawing::settings(&r#"<config:config-item config:name="N" config:type="int">1</config:config-item>"#.repeat(40));
    d.settings = Some(settings.clone());
    let limits = Limits { max_small_part_bytes: 1_000, ..Limits::default() };
    let (doc, report) = read(&d.bytes(), &limits).expect("reads");
    assert_eq!(report.warned("part_left_unread"), 1);
    assert_eq!(report.work.small_part_bytes, 0, "no tree was made");
    assert!(doc.ext.get().is_none_or(|d| d.odf.settings.is_none()));
    let part = doc.parts.iter().find(|p| &*p.name == "settings.xml").expect("an entry");
    assert_eq!(part.bytes.as_ref(), settings.as_bytes());
    assert_eq!(part.role, PartRole::Other);
}

// ---- the roots, `mimetype` and the manifest ------------------------------------------------------------------------------------------------

/// The rows of a manifest for a drawing of these files: the root's, the four XML parts', a picture a frame shows, a font with an attribute besides its media type, two folders, the
/// thumbnail, a second row for the font, a row for a file the package lacks, and one with no name.
fn rows() -> String {
    [
        r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
        r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="settings.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="Pictures/a.png" manifest:media-type="image/png"/>"#,
        r#"<manifest:file-entry manifest:full-path="Fonts/f.ttf" manifest:media-type="application/x-font-ttf" manifest:size="3" x:extra="e"/>"#,
        r#"<manifest:file-entry manifest:full-path="Configurations2/" manifest:media-type="application/vnd.sun.xml.ui.configuration"/>"#,
        r#"<manifest:file-entry manifest:full-path="Configurations2/accelerator/" manifest:media-type=""/>"#,
        r#"<manifest:file-entry manifest:full-path="Thumbnails/thumbnail.png" manifest:media-type="image/png"/>"#,
        r#"<manifest:file-entry manifest:full-path="Fonts/f.ttf" manifest:media-type="application/x-font-otf"/>"#,
        r#"<manifest:file-entry manifest:full-path="Missing/gone.bin" manifest:media-type="application/octet-stream"/>"#,
        r#"<manifest:file-entry manifest:media-type="text/plain" x:orphan="1"/>"#,
    ]
    .concat()
}

fn with_rows() -> Vec<u8> {
    let mut d = Drawing::a4().page(r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>"#);
    d.meta = Some(crate::drawing::meta("G/1"));
    d.settings = Some(crate::drawing::settings(""));
    let png = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    package_with_rows(
        &d,
        &[
            ("Pictures/a.png", &png),
            ("Fonts/f.ttf", b"abc"),
            ("Configurations2/", b""),
            ("Configurations2/accelerator/", b""),
            ("Thumbnails/thumbnail.png", &png),
            ("Extra/unnamed.bin", b"zz"),
        ],
        &rows(),
    )
}

/// The package says what it is in `mimetype`, and the roots hold the attributes and declarations of each part, by the name of the part.
#[test]
fn the_roots_and_mimetype_are_held() {
    let (doc, _) = read_ok(&with_rows());
    let odf = &doc.ext.get().expect("data").odf;
    assert_eq!(odf.mimetype.as_deref(), Some(DRAWING));
    assert_eq!(
        odf.roots.iter().map(|(n, _)| &**n).collect::<Vec<_>>(),
        ["meta.xml", "settings.xml", "styles.xml", "content.xml", "META-INF/manifest.xml"]
    );
    let root = |name: &str| odf.roots.iter().find(|(n, _)| &**n == name).map(|(_, a)| attrs_of(a)).unwrap_or_default();
    assert_eq!(
        root("META-INF/manifest.xml"),
        [("xmlns:manifest", "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"), ("manifest:version", "1.3"), ("xmlns:x", "urn:example:extension")]
    );
    let content = root("content.xml");
    assert!(
        content.contains(&("xmlns:draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0")) && content.contains(&("office:version", "1.3")),
        "{content:?}"
    );
    assert!(root("styles.xml").contains(&("xmlns:style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0")));
}

/// Each entry no field reads is a part, in the order of the directory, with the media type and the other attributes of its row, its bytes and what it is for; a folder is a part with
/// no bytes. What the model reads as something else is no part: the XML parts, `mimetype`, the manifest and the picture a frame shows.
#[test]
fn the_entries_nothing_reads_are_parts_with_the_rows_that_name_them() {
    let (doc, report) = read_ok(&with_rows());
    let parts: Vec<_> = doc.parts.iter().map(|p| (&*p.name, &*p.media_type, p.role, attrs_of(&p.manifest), p.bytes.len())).collect();
    assert_eq!(
        parts,
        [
            ("Fonts/f.ttf", "application/x-font-ttf", PartRole::Font, vec![("manifest:size", "3"), ("x:extra", "e")], 3),
            ("Configurations2/", "application/vnd.sun.xml.ui.configuration", PartRole::Other, vec![], 0),
            ("Configurations2/accelerator/", "", PartRole::Other, vec![], 0),
            ("Thumbnails/thumbnail.png", "image/png", PartRole::Thumbnail, vec![], 8),
            ("Extra/unnamed.bin", "", PartRole::Other, vec![], 2),
        ]
    );
    assert!(doc.parts.iter().all(|p| p.format == Format::Odf && p.rels.is_empty()));
    // The picture the frame shows is a picture, and no part: nothing is held twice.
    let picture =
        doc.pages.get(0).and_then(|p| p.shapes.get(0)).and_then(|s| s.foreign.as_ref()).and_then(|f| f.picture.as_ref()).expect("the picture");
    assert_eq!(&*picture.part, "Pictures/a.png");
    assert_eq!(report.warned("part_not_kept"), 0);
}

/// Every row no part took is held by the name it gives, in the manifest's order, with all its attributes but the name; the second row for a name and a row with no name too.
#[test]
fn the_rows_no_part_took_are_held_by_name() {
    let (doc, _) = read_ok(&with_rows());
    let odf = &doc.ext.get().expect("data").odf;
    let held: Vec<(&str, Vec<(&str, &str)>)> = odf.manifest_rows.iter().map(|(n, a)| (&**n, attrs_of(a))).collect();
    assert_eq!(
        held,
        [
            ("/", vec![("manifest:version", "1.3"), ("manifest:media-type", "application/vnd.oasis.opendocument.graphics")]),
            ("content.xml", vec![("manifest:media-type", "text/xml")]),
            ("styles.xml", vec![("manifest:media-type", "text/xml")]),
            ("meta.xml", vec![("manifest:media-type", "text/xml")]),
            ("settings.xml", vec![("manifest:media-type", "text/xml")]),
            ("Pictures/a.png", vec![("manifest:media-type", "image/png")]),
            ("Fonts/f.ttf", vec![("manifest:media-type", "application/x-font-otf")]),
            ("Missing/gone.bin", vec![("manifest:media-type", "application/octet-stream")]),
            ("", vec![("manifest:media-type", "text/plain"), ("x:orphan", "1")]),
        ]
    );
}

/// A file of the package that is damaged where nothing reads it does not stop the read, as it never did: it is left out of the parts and said once, and the package names it.
#[test]
fn a_damaged_entry_nothing_reads_is_left_out_and_said() {
    let d = Drawing::a4().page("");
    // A `.png` is stored, so that the marker is in the file as it is.
    let mut bytes = package_with_rows(&d, &[("Pictures/marker.png", b"MARKERMARKERMARKER"), ("Pictures/ok.png", b"fine")], "");
    let at = bytes.windows(18).position(|w| w == b"MARKERMARKERMARKER").expect("stored");
    bytes[at] ^= 0xFF;
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.parts.iter().map(|p| &*p.name).collect::<Vec<_>>(), ["Pictures/ok.png"]);
    assert_eq!(report.warned("part_not_kept"), 1);
    let mut work = diacraft_container::Work::default();
    let package = diacraft_odg::package::Package::read_kept(&bytes, &Limits::default(), &mut work).unwrap();
    assert_eq!(package.damaged(), ["Pictures/marker.png"]);
}

/// An entry that does not fit the budget of the media is refused before it is inflated, whole, and never left out: a model that is to hold the package holds all of it or refuses it.
#[test]
fn an_entry_past_the_media_budget_refuses_the_read_before_it_is_inflated() {
    let d = Drawing::a4().page("");
    let bytes = package_with_rows(&d, &[("Fonts/a.ttf", &vec![1u8; 600]), ("Fonts/b.ttf", &vec![2u8; 600])], "");
    let ok = Limits { max_media_bytes: 1_200, ..Limits::default() };
    assert_eq!(read(&bytes, &ok).expect("fits").0.parts.len(), 2);
    let short = Limits { max_media_bytes: 1_199, ..Limits::default() };
    let mut work = diacraft_container::Work::default();
    let refused = diacraft_odg::package::Package::read_kept(&bytes, &short, &mut work);
    assert_eq!(refused.err().map(|e| e.class()), Some("xml_too_complex"));
    let inflated_before_b = work.inflated;
    let mut all = diacraft_container::Work::default();
    diacraft_odg::package::Package::read_kept(&bytes, &ok, &mut all).unwrap();
    assert_eq!(inflated_before_b + 600, all.inflated, "the entry that does not fit is not inflated");
    match read(&bytes, &short) {
        Err(e) => assert_eq!(e.to_string(), "the drawing holds more than 1199 bytes of package entries kept"),
        Ok(_) => panic!("the entries were left out"),
    }
}

// ---- what the model gives back -----------------------------------------------------------------------------------------------------------

/// A model read from an `.odg` that holds all of this comes back from the `.diacraft` codec the same: what the reader keeps is data the codec writes and reads.
#[test]
fn the_package_data_comes_back_from_the_codec_the_same() {
    let mut d = Drawing::a4().page("");
    d.fonts = r#"<style:font-face style:name="Big" svg:font-family="Big"><svg:font-face-src><svg:font-face-uri xlink:href="Fonts/f.ttf"/></svg:font-face-src></style:font-face>"#.to_owned();
    d.meta = Some(full_meta());
    d.settings = Some(crate::drawing::settings(r#"<config:config-item config:name="N" config:type="int">1</config:config-item>"#));
    let bytes = package_with_rows(&d, &[("Fonts/f.ttf", b"abc"), ("Extra/unnamed.bin", b"zz"), ("Configurations2/", b"")], &rows());
    let (doc, _) = read_ok(&bytes);
    assert!(doc.parts.len() == 3 && doc.fonts.len() == 1 && doc.title.as_ref() == "My title", "the model holds the data the codec is to give back");
    assert!(doc.ext.get().is_some_and(|d| d.odf.settings.is_some() && !d.odf.roots.is_empty() && !d.odf.manifest_rows.is_empty()));
    let mut blobs = MemBlobs::new();
    let encoded = encode(&doc, &mut blobs).unwrap_or_else(|e| panic!("{e:?}"));
    let pages: Vec<&diacraft_container::xml::El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&diacraft_container::xml::El> = encoded.masters.iter().map(|(_, e)| e).collect();
    let mut budget = Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4);
    let mut notes = Vec::new();
    let back = decode(&encoded.document, &pages, &masters, &blobs, &mut budget, &mut notes).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(back.title, doc.title);
    assert_eq!(back.props, doc.props);
    assert_eq!(back.fonts, doc.fonts);
    assert_eq!(back.ext, doc.ext);
    assert_eq!(back.parts.len(), doc.parts.len());
    assert!(back.parts.iter().zip(doc.parts.iter()).all(|(a, b)| a == b), "the parts, bytes and all");
}

/// An unedited save of a drawing that holds all of this is the file, byte for byte; a save after an edit keeps every part and row the file has, as they were, and the file it makes
/// reads back to the same package data but for what the save writes of its own (the generator, the thumbnail).
#[test]
fn a_splice_save_keeps_the_package_as_it_was() {
    let bytes = with_rows();
    let o = open_bytes(bytes.clone());
    assert_eq!(o.doc.parts.len(), 5, "the package's entries are the model's");
    let same = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!same.edited && same.bytes == bytes, "an unedited save is the file");
    // An edit: every entry the model keeps is in the saved file with its bytes.
    let mut doc = o.doc.clone();
    let id = doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).expect("a shape");
    crate::write_core::move_by(&mut doc, 0, id, 0.5, 0.0);
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    for part in o.doc.parts.iter().filter(|p| &*p.name != "Thumbnails/thumbnail.png" && !p.name.ends_with('/')) {
        assert_eq!(crate::write_core::part_bytes(&saved.bytes, &part.name), part.bytes.as_ref(), "{}", part.name);
    }
    let again = open_bytes(saved.bytes.clone());
    assert_eq!(without_package(&again.doc).pages, without_package(&doc).pages);
    let names = |d: &Document| d.parts.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
    assert_eq!(names(&again.doc), names(&o.doc), "the thumbnail is written in place of the one the file had");
    // The second save of what the save made is no edit.
    assert!(!real(&again.doc, &again.bytes, &again.index).unwrap().edited);
}

/// The model of a drawing read twice is the same model, whatever the package data it holds.
#[test]
fn the_same_file_reads_to_the_same_model() {
    let bytes = with_rows();
    assert_eq!(read_ok(&bytes).0, read_ok(&bytes).0);
    assert_eq!(read_ok(&bytes).0.parts.len(), 5);
    let (doc, report) = read_partial(&bytes, &Limits::default());
    assert!(doc.is_ok() && report.work.items > 0);
}

/// A row of the manifest keeps every attribute but its name, as written and in order, and says where its media type stands, found by the namespace and not the prefix; the root keeps
/// its attributes and declarations, and a row with no name is kept apart from the others.
#[test]
fn a_manifest_row_keeps_its_attributes_in_order_and_the_place_of_its_media_type() {
    let manifest = diacraft_odg::Manifest::read(&with_rows(), &Limits::default()).expect("the manifest");
    let font = manifest.entry("Fonts/f.ttf").expect("a row");
    let pairs = |row: &diacraft_odg::ManifestEntry| row.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>();
    assert_eq!(
        pairs(font),
        [
            ("manifest:media-type".to_owned(), "application/x-font-ttf".to_owned()),
            ("manifest:size".to_owned(), "3".to_owned()),
            ("x:extra".to_owned(), "e".to_owned())
        ]
    );
    assert_eq!(font.media_type_at, Some(0));
    let root = manifest.root().expect("the root row");
    assert_eq!((root.attrs.len(), root.media_type_at), (2, Some(1)), "the version comes first and the media type second, as written");
    assert_eq!(manifest.root_attrs.len(), 3);
    assert_eq!(manifest.unnamed_rows.len(), 1);
    assert_eq!(manifest.unnamed_rows.first().map(Vec::len), Some(2));
    assert!(
        manifest.entries.iter().all(|row| row.attrs.iter().all(|(k, _)| !k.ends_with(":full-path"))),
        "the name is the row's and is no attribute of it"
    );
    // Another prefix for the manifest's namespace: the media type is found all the same.
    let rows = concat!(
        r#"<m:file-entry m:full-path="/" m:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
        r#"<m:file-entry m:media-type="text/xml" m:full-path="content.xml"/>"#
    );
    let text = format!(
        "<?xml version=\"1.0\"?>\n<m:manifest xmlns:m=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" m:version=\"1.3\">{rows}</m:manifest>"
    );
    let mut entries = Drawing::a4().page("").files();
    entries.push(("META-INF/manifest.xml".to_owned(), text.into_bytes()));
    let bytes = write(Some(DRAWING), &entries);
    let manifest = diacraft_odg::Manifest::read(&bytes, &Limits::default()).expect("the manifest");
    let content = manifest.entry("content.xml").expect("a row");
    assert_eq!((content.media_type.as_deref(), content.media_type_at, content.attrs.len()), (Some("text/xml"), Some(0), 1));
}
