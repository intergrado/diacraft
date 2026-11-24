//! The after-save check's misses and false alarms on the sample sets (GATE.ea42-inspect). Each test is a trip a person can make in the engine, one leg a session (`file.open`,
//! then `file.save_as`), on files built in code from the OASIS ODF standard and the testkit's VSDX builder.

use std::collections::HashMap;

use diacraft_container::xml::El;
use serde_json::{Value, json};

use crate::native_common::{Dir, opened};
use crate::odg_build::Drawing;

/// One leg of a trip: `from` opened in a session of its own and saved as `to` in `dir`, with what the save said.
pub(crate) fn leg(dir: &Dir, from: &str, to: &str) -> Value {
    let mut s = opened(&dir.path(from));
    s.execute("file.save_as", &json!({ "path": dir.path(to) })).unwrap_or_else(|e| panic!("{from} to {to}: {e}"))
}

/// The ids of the classes a save's `checked` field names.
pub(crate) fn named(said: &Value) -> Vec<String> {
    said["checked"]["classes"].as_array().map(|list| list.iter().filter_map(|c| c["class"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

pub(crate) fn parse(text: &str) -> El {
    diacraft_container::xml::parse(text.as_bytes()).unwrap()
}

/// Every element under `el`, depth first.
pub(crate) fn all<'a>(el: &'a El, out: &mut Vec<&'a El>) {
    for child in el.elements() {
        out.push(child);
        all(child, out);
    }
}

/// A graphic style as [`graphic_styles`] gives it: its parent, and the attributes of its graphic properties.
pub(crate) type GraphicStyle = (Option<String>, Vec<(String, String)>);

/// The graphic styles of a package (common and automatic, of both parts): each name with its parent and the attributes of its graphic properties.
pub(crate) fn graphic_styles(bytes: &[u8]) -> HashMap<String, GraphicStyle> {
    let mut out = HashMap::new();
    for part in ["styles.xml", "content.xml"] {
        let root = parse(&crate::odg_build::part_of(bytes, part));
        let mut list = Vec::new();
        all(&root, &mut list);
        for el in list {
            if el.name != "style:style" || el.attr("style:family") != Some("graphic") {
                continue;
            }
            let Some(name) = el.attr("style:name") else { continue };
            let props =
                el.child("graphic-properties").map(|p| p.attrs.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()).unwrap_or_default();
            out.insert(name.to_owned(), (el.attr("style:parent-style-name").map(str::to_owned), props));
        }
    }
    out
}

/// What the style chain of one shape says of each attribute asked for, by the shape's name.
pub(crate) type Said = (String, Vec<(String, Option<String>)>);

/// What the style chain of each named shape of a package says of the attributes `wanted` (the nearest style that says one wins), by the shape's `draw:name`.
pub(crate) fn resolved(bytes: &[u8], wanted: &[&str]) -> Vec<Said> {
    let styles = graphic_styles(bytes);
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    let mut out = Vec::new();
    for el in list {
        let Some(name) = el.attr("draw:name") else { continue };
        let mut said = Vec::new();
        for attr in wanted {
            let mut at = el.attr("draw:style-name").map(str::to_owned);
            let mut found = None;
            let mut steps = 0;
            while let Some(style) = at.take() {
                steps += 1;
                if steps > 32 {
                    break;
                }
                let Some((parent, props)) = styles.get(&style) else { break };
                if let Some((_, v)) = props.iter().find(|(n, _)| n == attr) {
                    found = Some(v.clone());
                    break;
                }
                at = parent.clone();
            }
            said.push(((*attr).to_owned(), found));
        }
        out.push((name.to_owned(), said));
    }
    out.sort();
    out
}

pub(crate) const ENDS: [&str; 6] = [
    "draw:marker-start",
    "draw:marker-start-width",
    "draw:marker-start-center",
    "draw:marker-end",
    "draw:marker-end-width",
    "draw:marker-end-center",
];

/// **A control of item 1** (line ends lost in 30 files of the plain visit, mostly a `draw:marker-start-width`): a width with no marker, in each form built from the class alone (an automatic style with
/// no parent, one over LibreOffice's `standard`, both ends, beside a stroke width; on a line, a rectangle and a custom shape), comes back by the chain each shape's style resolves through, and the
/// check names nothing. Passes on `e6020b3b`: the sample files' form is still to be told (a vector is asked for).
#[test]
fn a_line_end_s_width_with_no_marker_comes_back_from_the_plain_visit() {
    let arrow = r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"#;
    let standard = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-width="0cm" svg:stroke-color="#3465a4" draw:marker-start-width="0.2cm" draw:marker-start-center="false" draw:marker-end-width="0.2cm" draw:marker-end-center="false" draw:fill="solid" draw:fill-color="#729fcf"/></style:style>"##;
    let autos = [
        r#"<style:style style:name="gw1" style:family="graphic"><style:graphic-properties draw:marker-start-width="0.3cm"/></style:style>"#,
        r#"<style:style style:name="gw2" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-end-width="0.45cm"/></style:style>"#,
        r#"<style:style style:name="gw3" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-start-width="0.3cm" draw:marker-end-width="0.3cm"/></style:style>"#,
        r#"<style:style style:name="gw4" style:family="graphic" style:parent-style-name="standard"/>"#,
        r#"<style:style style:name="gw5" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties svg:stroke-width="0.1cm" draw:marker-start-width="0.3cm"/></style:style>"#,
    ];
    let mut d = Drawing::new().common(arrow).common(standard);
    for a in autos {
        d.auto.push_str(a);
    }
    let mut shapes = String::new();
    for (k, style) in ["gw1", "gw2", "gw3", "gw4", "gw5"].iter().enumerate() {
        let y = 2 + 2 * k;
        shapes.push_str(&format!(
            r#"<draw:line draw:style-name="{style}" draw:layer="layout" draw:name="L{k}" svg:x1="2cm" svg:y1="{y}cm" svg:x2="8cm" svg:y2="{y}cm"><text:p/></draw:line>"#
        ));
        shapes.push_str(&format!(
            r#"<draw:rect draw:style-name="{style}" draw:layer="layout" draw:name="R{k}" svg:x="10cm" svg:y="{y}cm" svg:width="3cm" svg:height="1cm"/>"#
        ));
        shapes.push_str(&format!(
            r#"<draw:custom-shape draw:style-name="{style}" draw:layer="layout" draw:name="C{k}" svg:x="14cm" svg:y="{y}cm" svg:width="3cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        ));
    }
    let d = d.page(&shapes);
    let source = d.bytes();
    let dir = Dir::new();
    dir.write("a.odg", &source);
    let first = leg(&dir, "a.odg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    assert_eq!((named(&first), named(&second)), (Vec::<String>::new(), Vec::<String>::new()));
    let was = resolved(&source, &ENDS);
    assert!(was.iter().any(|(_, said)| said.iter().any(|(attr, v)| attr == "draw:marker-start-width" && v.is_some())), "the source says widths");
    assert_eq!(resolved(&dir.read("c.odg"), &ENDS), was, "every shape's line ends, by its style chain");
}

pub(crate) const LINKS: &str = r#"xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad""#;

/// A PNG of 2 by 2 pixels.
pub(crate) fn png() -> Vec<u8> {
    diacraft_render::export::encode_png(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255], None).unwrap()
}

/// An `.odg` package with the entries given, each with the media type of its manifest row.
pub(crate) fn package(content: &str, styles: &str, files: &[(&str, &str, Vec<u8>)]) -> Vec<u8> {
    use diacraft_container::zip::{ArchiveWriter, Method};
    let media = crate::odg_build::DRAWING;
    let mut all: Vec<(String, String, Vec<u8>)> = vec![
        ("content.xml".into(), "text/xml".into(), content.as_bytes().to_vec()),
        ("styles.xml".into(), "text/xml".into(), styles.as_bytes().to_vec()),
    ];
    all.extend(files.iter().map(|(n, m, b)| ((*n).to_owned(), (*m).to_owned(), b.clone())));
    let mut manifest = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
    );
    manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{media}\"/>\n"));
    for (name, media_type, _) in &all {
        manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media_type}\"/>\n"));
    }
    manifest.push_str("</manifest:manifest>\n");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
    for (name, _, data) in &all {
        w.add(name, data, Method::Deflated).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// An `.odg` of one page holding `shapes`, on the automatic styles `auto`, with the entries `files`.
pub(crate) fn drawing(auto: &str, shapes: &str, files: &[(&str, &str, Vec<u8>)]) -> Vec<u8> {
    let mut d = Drawing::new();
    d.auto.push_str(auto);
    let d = d.page(shapes);
    package(&d.content_xml(), &d.styles_xml(), files)
}

/// The names of the entries of a package.
fn entries(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut names: Vec<String> = package.parts().map(str::to_owned).collect();
    names.sort();
    names
}

// ------------------------------------------------------------------------------------------------------------------ item 7: glue

/// A custom shape `name` with an id a connector can name, at `x` cm.
fn glue_target(name: &str, x: f64) -> String {
    format!(
        r#"<draw:custom-shape xml:id="{name}" draw:id="{name}" draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" svg:width="3cm" svg:height="2cm" svg:x="{x}cm" svg:y="2cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    )
}

/// A connector `name` with an id, and the glue `glue`.
fn named_connector(name: &str, glue: &str) -> String {
    format!(
        r#"<draw:connector xml:id="{name}" draw:id="{name}" draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" {glue} svg:x1="5cm" svg:y1="3cm" svg:x2="10cm" svg:y2="3cm" svg:d="M 5000 3000 L 10000 3000" svg:viewBox="0 0 10001 3001"><text:p/></draw:connector>"#
    )
}

/// The connections of the first page of a drawing, each as the `NameU` of the shape glued, its cell, and the `NameU` of the shape it is glued to, sorted.
fn glue_by_name(doc: &diacraft_doc::Document) -> Vec<(String, String, String)> {
    let page = doc.pages.get(0).unwrap();
    let name = |id: u32| page.shapes.iter().find(|s| s.id == id).and_then(|s| s.name_u.as_deref().map(str::to_owned)).unwrap_or_default();
    let mut out: Vec<(String, String, String)> =
        page.connects.iter().map(|c| (name(c.from_sheet), c.from_cell.as_deref().unwrap_or("").to_owned(), name(c.to_sheet))).collect();
    out.sort();
    out
}

/// **A connector glued to a connector keeps its glue on the plain visit** (item 7): the `.odg` model writer gave the end that names the other connector the id `id<n>`, and wrote that connector
/// with no id, so the file read back had the end glued to nothing: its glue lost and its ends values, which the check told as a connection and two formulas lost. Red on `e6020b3b`: the second
/// connector's begin is not glued in the file read back.
#[test]
fn a_connector_glued_to_a_connector_comes_back_glued_from_the_plain_visit() {
    let shapes = format!(
        "{}{}{}{}",
        glue_target("A", 2.0),
        glue_target("B", 10.0),
        named_connector("C0", r#"draw:start-shape="A" draw:start-glue-point="3" draw:end-shape="B" draw:end-glue-point="1""#),
        named_connector("C1", r#"draw:start-shape="C0" draw:start-glue-point="3" draw:end-shape="B" draw:end-glue-point="2""#)
    );
    let source = drawing("", &shapes, &[]);
    let dir = Dir::new();
    dir.write("a.odg", &source);
    let was = glue_by_name(opened(&dir.path("a.odg")).document().unwrap());
    assert!(was.contains(&("C1".to_owned(), "BeginX".to_owned(), "C0".to_owned())), "the source glues C1 to C0: {was:?}");
    let first = leg(&dir, "a.odg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    let now = glue_by_name(opened(&dir.path("c.odg")).document().unwrap());
    assert_eq!(now, was, "every connection comes back between the same shapes");
    for said in [&first, &second] {
        let classes = named(said);
        assert!(!classes.iter().any(|c| c == "connections" || c == "formulas"), "nothing of the glue is said lost: {classes:?}");
    }
    // The connector glued to is named in the file, by the id the other's end names.
    let content = crate::odg_build::part_of(&dir.read("c.odg"), "content.xml");
    let named_c0 = content.split("<draw:connector").find(|tag| tag.contains(r#"draw:name="C0""#)).unwrap();
    assert!(named_c0.contains("xml:id=") && named_c0.contains("draw:id="), "C0 carries an id: {named_c0}");
}

// ------------------------------------------------------------------------------------------------------------------ item 8: the properties of a crossing

/// An `.odg` of one rectangle whose `meta.xml` says when it was made, by whom, and of what.
fn dated_odg() -> Vec<u8> {
    let d = Drawing::new().page(&crate::odg_build::rect("R", (2.0, 2.0, 3.0, 2.0), "hi"));
    let meta = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator><dc:title>A title</dc:title><dc:subject>A subject</dc:subject><meta:keyword>words</meta:keyword><dc:description>About it</dc:description><meta:initial-creator>Ann</meta:initial-creator><meta:creation-date>2019-04-05T06:07:08Z</meta:creation-date><dc:date>2020-01-01T00:00:00Z</dc:date><dc:language>en-GB</dc:language></office:meta></office:document-meta>",
        crate::odg_build::NS
    );
    let mut files = vec![
        ("content.xml".to_owned(), d.content_xml().into_bytes()),
        ("styles.xml".to_owned(), d.styles_xml().into_bytes()),
        ("meta.xml".to_owned(), meta.into_bytes()),
    ];
    files.extend(d.files.iter().cloned());
    crate::odg_build::package(crate::odg_build::DRAWING, files)
}

/// The text of the element `name` in a VSDX file's `docProps/core.xml`.
fn core_field(bytes: &[u8], name: &str) -> Option<String> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let text = String::from_utf8(package.part("/docProps/core.xml").unwrap().to_vec()).unwrap();
    let root = parse(&text);
    root.elements().find(|e| e.name == name).map(El::text)
}

/// **A file written in another format keeps the drawing's creation date** (item 8): the save across from an `.odg` to VSDX wrote the save's time as `created`, and the check named
/// "document properties" on 1,074 of 1,212 such saves. Now the VSDX file says when the drawing was made, and the check names nothing of the properties; the other fields the target holds
/// come back too. Red on `e6020b3b`: `dcterms:created` is the save's time, and `properties` is named.
#[test]
fn a_crossing_from_an_odg_keeps_the_drawing_s_creation_date_and_properties() {
    let dir = Dir::new();
    dir.write("a.odg", &dated_odg());
    for (from, to) in [("a.odg", "b.vsdx"), ("a.odg", "c.diacraft"), ("c.diacraft", "d.vsdx")] {
        let said = leg(&dir, from, to);
        assert!(!named(&said).iter().any(|c| c == "properties"), "{from} to {to}: no property is lost: {}", said["checked"]);
    }
    for vsdx in ["b.vsdx", "d.vsdx"] {
        let bytes = dir.read(vsdx);
        assert_eq!(core_field(&bytes, "dcterms:created").as_deref(), Some("2019-04-05T06:07:08Z"), "{vsdx}: when the drawing was made");
        for (field, text) in
            [("dc:title", "A title"), ("dc:subject", "A subject"), ("cp:keywords", "words"), ("dc:creator", "Ann"), ("dc:language", "en-GB")]
        {
            assert_eq!(core_field(&bytes, field).as_deref(), Some(text), "{vsdx}: {field}");
        }
        let back = opened(&dir.path(vsdx));
        assert_eq!(back.document().unwrap().props.created.as_deref(), Some("2019-04-05T06:07:08Z"), "{vsdx}: read back");
    }
}

/// The other direction: a VSDX drawing saved as an `.odg` keeps its creation date in `meta:creation-date`, and the check names no property.
#[test]
fn a_crossing_from_vsdx_keeps_the_drawing_s_creation_date() {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let core = r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>A title</dc:title><dc:subject>A subject</dc:subject><dc:creator>Ann</dc:creator><dcterms:created xsi:type="dcterms:W3CDTF">2018-02-03T04:05:06Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2018-03-03T04:05:06Z</dcterms:modified></cp:coreProperties>"#;
    let bytes = VsdxBuilder::new()
        .page(
            PageSpec::new(0, "Page-1")
                .sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#)
                .shapes(&shape(1, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#)),
        )
        .core(core)
        .build();
    let dir = Dir::new();
    dir.write("a.vsdx", &bytes);
    for (from, to) in [("a.vsdx", "b.odg"), ("a.vsdx", "c.diacraft"), ("c.diacraft", "d.odg")] {
        let said = leg(&dir, from, to);
        assert!(!named(&said).iter().any(|c| c == "properties"), "{from} to {to}: no property is lost: {}", said["checked"]);
    }
    for odg in ["b.odg", "d.odg"] {
        let meta = crate::odg_build::part_of(&dir.read(odg), "meta.xml");
        assert!(meta.contains("<meta:creation-date>2018-02-03T04:05:06Z</meta:creation-date>"), "{odg}: {meta}");
    }
}

// ------------------------------------------------------------------------------------------------------------------ item 9: templates

/// **A drawing made from a template file is checked when it is saved as `.diacraft`** (item 9): it has no file of its own, so the check after the save took it for a new drawing, and `checked`
/// was `null` for 56 template saves (36 `.vstx` and `.vstm`, 20 `.otg`). It came from the VSDX or the `.odg` reader as a drawing opened from such a file does, and is checked as one is. A
/// drawing made new (`file.new`) holds no other format's data and is still not checked (`null`). Red on `e6020b3b`: `checked` is `null` for both templates.
#[test]
fn a_drawing_made_from_a_template_file_is_checked_when_saved_as_diacraft() {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let dir = Dir::new();
    let vstx = VsdxBuilder::new()
        .document_type(diacraft_vsdx::Variant::from_extension("vstx").unwrap().content_type())
        .page(
            PageSpec::new(1, "First")
                .sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#)
                .shapes(&shape(5, r#"<Cell N="PinX" V="1"/>"#)),
        )
        .build();
    dir.write("t.vstx", &vstx);
    let otg = Drawing::new().page(&crate::odg_build::rect("R", (2.0, 2.0, 3.0, 2.0), "hi")).bytes_of_type(crate::odg_build::TEMPLATE);
    dir.write("t.otg", &otg);
    for (template, to) in [("t.vstx", "a.diacraft"), ("t.otg", "b.diacraft"), ("t.vstx", "c.diacraft-template"), ("t.otg", "d.diacraft-template")] {
        let mut s = opened(&dir.path(template));
        assert!(s.source().is_none(), "{template} opens as a new drawing made from it");
        let said = s.execute("file.save_as", &json!({ "path": dir.path(to) })).unwrap();
        assert_eq!(said["checked"]["done"], json!(true), "{template} to {to}: checked: {}", said["checked"]);
        assert_eq!(said["checked"]["classes"], json!([]), "{template} to {to}: nothing differs");
    }
    // A drawing made new holds nothing of another format: not checked, as before.
    let mut s = crate::native_common::pinned();
    let said = s.execute("file.save_as", &json!({ "path": dir.path("e.diacraft") })).unwrap();
    assert_eq!(said["checked"], json!(null));
}

// ------------------------------------------------------------------------------------------------------------------ items 2 to 4: a drawing object in a paragraph

/// A picture frame anchored as a character, named `name`, showing `Pictures/a.png`.
fn inline_picture(name: &str) -> String {
    format!(
        r#"<draw:frame draw:style-name="gr1" draw:name="{name}" text:anchor-type="as-char" svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/a.png" {LINKS}><text:p/></draw:image></draw:frame>"#
    )
}

/// The `draw:frame` elements of `content.xml` that hold a `draw:image`, as the text of each, and whether any `text:p` holds a `text:p`.
fn image_frames(bytes: &[u8]) -> (Vec<String>, bool) {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    let frames = list
        .iter()
        .filter(|e| e.name == "draw:frame" && e.elements().any(|c| c.name == "draw:image"))
        .map(|e| e.attr("draw:name").unwrap_or("").to_owned())
        .collect();
    let nested = list.iter().filter(|e| e.name == "text:p").any(|p| {
        let mut inner = Vec::new();
        all(p, &mut inner);
        inner.iter().any(|e| e.name == "text:p" && !std::ptr::eq(*e, *p) && !inside_frame(p, e))
    });
    (frames, nested)
}

/// Whether `e` is under a `draw:frame` that is under `p` (a frame's own paragraphs are no paragraph of `p`'s).
fn inside_frame(p: &El, e: &El) -> bool {
    let mut frames = Vec::new();
    all(p, &mut frames);
    frames.iter().filter(|f| f.name == "draw:frame").any(|f| {
        let mut under = Vec::new();
        all(f, &mut under);
        under.iter().any(|u| std::ptr::eq(*u, e))
    })
}

/// **A drawing object in a paragraph comes back whole** (items 2 to 4): a picture frame anchored as a character in a shape's text was read as a run of the characters it holds, and written
/// as the frame's tags with nothing between them, its picture and its own paragraph after it **inside the paragraph** (a `text:p` in a `text:p`): the frame showed no picture, and the check named
/// nothing, since the model held the frame as a run of no characters. It is held whole where it stands now, as a mark is, and written back whole; the check tells such an object that does not
/// come back. Red on `e6020b3b`: the frame holds no `draw:image` after the visit, and a paragraph holds a paragraph.
#[test]
fn a_picture_anchored_as_a_character_in_a_shape_s_text_comes_back_whole() {
    let custom = |name: &str, x: f64, text: &str| {
        format!(
            r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" svg:width="6cm" svg:height="3cm" svg:x="{x}cm" svg:y="2cm"><text:p>{text}</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        )
    };
    let shapes = format!("{}{}", custom("C", 2.0, &format!("before {} after", inline_picture("I1"))), custom("D", 10.0, &inline_picture("I2")));
    let source = drawing("", &shapes, &[("Pictures/a.png", "image/png", png())]);
    let (frames, nested) = image_frames(&source);
    assert_eq!((frames.clone(), nested), (vec!["I1".to_owned(), "I2".to_owned()], false), "the source");
    let dir = Dir::new();
    dir.write("a.odg", &source);
    let first = leg(&dir, "a.odg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    let back = dir.read("c.odg");
    assert_eq!(image_frames(&back), (frames, false), "both frames hold their picture, and no paragraph holds a paragraph");
    assert!(entries(&back).contains(&"Pictures/a.png".to_owned()), "the picture's entry: {:?}", entries(&back));
    let text_of = |doc: &diacraft_doc::Document, name: &str| {
        let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).cloned().unwrap();
        shape
            .text
            .as_ref()
            .map(|t| {
                t.parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect::<String>()
            })
            .unwrap_or_default()
    };
    let read = opened(&dir.path("c.odg"));
    assert_eq!(
        text_of(read.document().unwrap(), "C"),
        "before after",
        "the paragraph's characters are its own (the spaces round the frame collapse across it, as across a mark), and none of the frame's"
    );
    for said in [&first, &second] {
        assert_eq!(said["checked"]["classes"], json!([]), "nothing differs: {}", said["checked"]);
    }
}

/// A VSDX drawing of instances of masters whose paragraphs, geometry and text the instances leave to the master, on two pages, with a style sheet.
fn inheriting_vsdx() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};
    let master_shape = concat!(
        r#"<Shape ID="5" Type="Shape" LineStyle="1" FillStyle="1" TextStyle="1"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#,
        r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="2"/><Cell N="IndFirst" V="0"/></Row></Section>"#,
        r#"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.2"/></Row></Section>"#,
        r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#,
        r#"<Text>master text</Text></Shape>"#
    );
    let mut shapes = String::new();
    for id in 1..=12u32 {
        let x = f64::from(id);
        let extra = match id % 4 {
            0 => r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="0"/></Row></Section>"#.to_owned(),
            1 => r#"<Section N="Geometry" IX="0"><Row T="LineTo" IX="3"><Cell N="X" V="3"/></Row></Section>"#.to_owned(),
            2 => r#"<Text>own text</Text>"#.to_owned(),
            _ => r#"<Section N="Paragraph"><Row IX="0"><Cell N="IndFirst" V="0.1"/></Row><Row IX="1"><Cell N="HorzAlign" V="1"/></Row></Section><Text><pp IX="0"/>one<pp IX="1"/>two</Text>"#.to_owned(),
        };
        shapes.push_str(&format!(
            r#"<Shape ID="{id}" Type="Shape" Master="2" MasterShape="5"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/>{extra}</Shape>"#
        ));
    }
    let style = r#"<StyleSheet ID="1" NameU="Box" Name="Box" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="EnableLineProps" V="1"/><Cell N="EnableFillProps" V="1"/><Cell N="EnableTextProps" V="1"/><Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row></Section></StyleSheet>"#;
    VsdxBuilder::new()
        .more_styles(style)
        .master(MasterSpec::new(2, "Box").shapes(master_shape))
        .page(PageSpec::new(0, "Page-1").sheet(r#"<Cell N="PageWidth" V="20"/><Cell N="PageHeight" V="11"/>"#).shapes(&shapes))
        .page(PageSpec::new(1, "Page-2").sheet(r#"<Cell N="PageWidth" V="20"/><Cell N="PageHeight" V="11"/>"#).shapes(&shapes))
        .build()
}

/// **A control of item 6** (three VSDX files named in both legs of the trip through `.diacraft`, which the gate traced to the engine's drawing after `file.open` differing from the plain reader's): a
/// drawing of master instances that leave their paragraphs, geometry and text to the master is the reader's model after `file.open`, at one thread and at four, and the trip names nothing. Passes on
/// `e6020b3b`: the sample files' form is still to be told (a vector is asked for).
#[test]
fn the_engine_s_open_gives_the_reader_s_model_at_any_thread_count() {
    let bytes = inheriting_vsdx();
    let dir = Dir::new();
    dir.write("a.vsdx", &bytes);
    let s = opened(&dir.path("a.vsdx"));
    let engine = s.document().unwrap().clone();
    for threads in [1usize, 4] {
        let limits = diacraft_vsdx::Limits { threads, ..diacraft_vsdx::Limits::default() };
        let (plain, _) = diacraft_vsdx::read(&bytes, &limits).unwrap();
        assert!(plain == engine, "{threads} threads: {:?}", diacraft_engine::check::explain(&engine, &plain));
    }
    let first = leg(&dir, "a.vsdx", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.vsdx");
    assert_eq!((named(&first), named(&second)), (Vec::<String>::new(), Vec::<String>::new()));
}

/// Two rectangles of an `.odg`, the first holding a `draw:glue-point` of its own numbered `point` when there is one, and a connector from that point (or the first's standard point 3)
/// to the second's standard point 1: the structure of the gate's vectors for the `connections` class, with values of our own.
fn glue_point_drawing(point: Option<u32>) -> Vec<u8> {
    let own = point.map_or_else(String::new, |id| format!(r#"<draw:glue-point draw:id="{id}" svg:x="1cm" svg:y="0.5cm"/>"#));
    let start = point.unwrap_or(3);
    let shapes = format!(
        concat!(
            r#"<draw:rect draw:id="id1" draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm">{own}</draw:rect>"#,
            r#"<draw:rect draw:id="id2" draw:style-name="gr1" svg:x="10cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"/>"#,
            r#"<draw:connector draw:style-name="gr1" draw:start-shape="id1" draw:start-glue-point="{start}" draw:end-shape="id2" draw:end-glue-point="1" svg:x1="3cm" svg:y1="2.5cm" svg:x2="10cm" svg:y2="3cm"/>"#
        ),
        own = own,
        start = start
    );
    Drawing::new().page(&shapes).bytes()
}

/// **A connector glued to a shape's own glue point visits `.diacraft` and comes back with nothing named** (the gate's vectors `odg-glue-point-custom`,
/// `odg-glue-point-id4` and the control `odg-glue-point-default`): the `.odg` model writer writes the point back as the `draw:glue-point` it was read from, which the reader reads again as
/// the same row with the element held; it carried the whole `Connection` section in the shape's style as well, and the rows read back from that had lost their elements, so the check named
/// "1 connection differs". The point's element is held after the trip, and the file's markup holds it as it was.
#[test]
fn a_connector_glued_to_a_shapes_own_glue_point_visits_diacraft_and_comes_back_with_nothing_named() {
    for point in [Some(10u32), Some(4), None] {
        let dir = Dir::new();
        dir.write("a.odg", &glue_point_drawing(point));
        let first = leg(&dir, "a.odg", "b.diacraft");
        let second = leg(&dir, "b.diacraft", "c.odg");
        assert_eq!((named(&first), named(&second)), (Vec::<String>::new(), Vec::<String>::new()), "point {point:?}: {second}");
        let content = crate::odg_build::content_of(&dir.read("c.odg"));
        assert!(!content.contains("diacraft:s-Connection"), "point {point:?}: the rows the markup gives are not carried: {content}");
        if let Some(id) = point {
            assert!(content.contains(&format!(r#"<draw:glue-point draw:id="{id}" svg:x="1cm" svg:y="0.5cm"/>"#)), "point {point:?}: {content}");
            let back = opened(&dir.path("c.odg"));
            let shape = back.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().clone();
            let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Connection").unwrap().clone();
            let held = section.rows.iter().filter(|r| r.ext.get().and_then(|e| e.odf.held.as_ref()).is_some()).count();
            assert_eq!(held, 1, "point {point:?}: the element is held beside its row");
        }
    }
}
