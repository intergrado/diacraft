//! Through the engine (PL5's second leg): the ids of the shapes of an ODF 1.0 drawing, `draw:id` and no `xml:id`, are written as ODF 1.4 takes them. A drawing is opened,
//! Save As `.diacraft`, a new session opens that file, Save As `.odg`: every shape written with a `draw:id` carries an `xml:id` of the same value, a connector's ends still name the
//! shapes and the glue is read back after the visit and after a second one, the content check finds nothing, and a visit of what came back is the same bytes (clock pinned). The
//! crate-level tests of the same classes, with the ODF 1.4 schema, are `crates/odg/tests/it/visit_shape_ids.rs`.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{DRAWING, Drawing, GR1, GR2, P1, package, part_of};
use crate::run;

/// The parts a visit is judged by.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// A drawing whose parts say `version`, whose page holds `shapes`.
fn drawing(shapes: &str) -> Drawing {
    let mut d = Drawing::new().page(shapes);
    d.auto = format!("{GR1}{GR2}{P1}");
    d
}

fn as_version(text: String, version: &str) -> String {
    text.replace("office:version=\"1.3\"", &format!("office:version=\"{version}\""))
}

/// The package of the drawing as ODF `version`.
fn package_of(shapes: &str, version: &str) -> Vec<u8> {
    let d = drawing(shapes);
    let files = vec![
        ("content.xml".to_owned(), as_version(d.content_xml(), version).into_bytes()),
        ("styles.xml".to_owned(), as_version(d.styles_xml(), version).into_bytes()),
        ("meta.xml".to_owned(), as_version(d.meta_xml(), version).into_bytes()),
    ];
    package(DRAWING, files)
}

/// The flat document of the drawing as ODF `version`.
fn flat_of(shapes: &str, version: &str) -> Vec<u8> {
    as_version(drawing(shapes).flat_xml(DRAWING), version).into_bytes()
}

fn rect(id: &str, x: u32) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" draw:id="{id}" svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

const CONNECTOR: &str = r#"<draw:connector draw:style-name="gr1" draw:start-shape="n1" draw:start-glue-point="1" draw:end-shape="n2" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="9cm" svg:y2="4cm"/>"#;

/// The visit of `source`, named `name` (`v.odg` or `v.fodg`): the file that came back, and what the content check finds in the drawing's two parts. `Err` is a refusal.
fn visit(dir: &Dir, name: &str, source: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    if let Err(e) = t.execute("file.save_as", &json!({ "path": dir.path("w.odg") })) {
        return Err(format!("{e}"));
    }
    let written = dir.read("w.odg");
    let diff = diacraft_odg::compare_content(source, &written, &diacraft_odg::Limits::default()).unwrap();
    let found =
        diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    Ok((written, found))
}

/// The visit is the same content and a visit of what came back is the same bytes. Gives the file that came back.
fn assert_same(what: &str, name: &str, source: &[u8]) -> Vec<u8> {
    let (written, found) = visit(&Dir::new(), name, source).unwrap_or_else(|e| panic!("{what}: refused: {e}"));
    assert!(found.is_empty(), "{what}: the visit differs from the file: {found:#?}");
    let (again, found) = visit(&Dir::new(), "v.odg", &written).unwrap_or_else(|e| panic!("{what}: the second visit is refused: {e}"));
    assert!(found.is_empty(), "{what}: the second visit differs: {found:#?}");
    assert_eq!(again, written, "{what}: a visit of what came back is the same bytes (clock pinned)");
    written
}

/// How many ends of connectors the file glues to a shape.
fn glued_ends(bytes: &[u8]) -> usize {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    doc.pages.get(0).unwrap().connects.len()
}

/// The two rectangles of the vector, with the ids as a pair.
fn assert_pairs(what: &str, content: &str, ids: &[&str]) {
    for id in ids {
        assert!(content.contains(&format!(r#"xml:id="{id}" draw:id="{id}""#)), "{what}: {id} is written with an xml:id of the same value: {content}");
        assert_eq!(content.matches(&format!(r#"draw:id="{id}""#)).count(), 1, "{what}: and once: {content}");
    }
}

#[test]
fn two_shapes_with_draw_id_alone_in_an_odf_1_0_package_come_back_with_an_xml_id() {
    // Variant A: no connector.
    let written = assert_same("variant A", "v.odg", &package_of(&format!("{}{}", rect("n1", 2), rect("n2", 9)), "1.0"));
    let content = part_of(&written, "content.xml");
    assert!(content.contains("office:version=\"1.4\""), "a visit writes ODF 1.4: {content}");
    assert_pairs("variant A", &content, &["n1", "n2"]);
}

#[test]
fn a_connector_glued_to_shapes_with_draw_id_alone_in_an_odf_1_0_package_stays_glued() {
    // Variant B: the connector names the shapes.
    let source = package_of(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)), "1.0");
    assert_eq!(glued_ends(&source), 2, "the input is glued at both ends");
    let written = assert_same("variant B", "v.odg", &source);
    let content = part_of(&written, "content.xml");
    assert_pairs("variant B", &content, &["n1", "n2"]);
    assert!(content.contains(r#"draw:start-shape="n1""#) && content.contains(r#"draw:end-shape="n2""#), "the ends name the shapes: {content}");
    assert_eq!(glued_ends(&written), 2, "the glue survives the visit");
    // And a second one.
    let (again, _) = visit(&Dir::new(), "v.odg", &written).unwrap();
    assert_eq!(glued_ends(&again), 2, "and the second visit");
}

#[test]
fn the_flat_drawings_of_odf_1_0_and_1_1_with_draw_id_alone_come_back_the_same_way() {
    for version in ["1.0", "1.1"] {
        let source = flat_of(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)), version);
        let written = assert_same(&format!("a flat drawing of {version}"), "v.fodg", &source);
        let content = part_of(&written, "content.xml");
        assert_pairs(version, &content, &["n1", "n2"]);
        assert_eq!(glued_ends(&written), 2, "{version}: the glue survives");
    }
}

#[test]
fn every_kind_of_shape_with_draw_id_alone_comes_back_with_an_xml_id() {
    let frame = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;
    let shapes = [
        format!(r#"<draw:rect draw:style-name="gr1" draw:id="k0" {frame}/>"#),
        format!(r#"<draw:ellipse draw:style-name="gr1" draw:id="k1" {frame}/>"#),
        format!(r#"<draw:circle draw:style-name="gr1" draw:id="k2" {frame}/>"#),
        r#"<draw:line draw:style-name="gr1" draw:id="k3" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="4cm"/>"#.to_owned(),
        format!(
            r#"<draw:custom-shape draw:style-name="gr1" draw:id="k4" {frame}><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#
        ),
        format!(r#"<draw:frame draw:style-name="gr1" draw:id="k5" {frame}><draw:text-box><text:p>Hi</text:p></draw:text-box></draw:frame>"#),
        format!(r#"<draw:g draw:id="k6">{}{}</draw:g>"#, rect("k7", 2), rect("k8", 9)),
    ]
    .concat();
    let written = assert_same("every kind", "v.odg", &package_of(&shapes, "1.0"));
    assert_pairs("every kind", &part_of(&written, "content.xml"), &["k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8"]);
}

/// A `draw:id` that is no NCName cannot be an `xml:id`: it is no id (written in neither attribute), a connector glued to it is given one of the writer's own and stays glued.
#[test]
fn a_draw_id_that_is_no_name_is_written_as_no_id_and_a_glued_connector_stays_glued() {
    let connector = CONNECTOR.replace("\"n1\"", "\"a b\"");
    let source = package_of(&format!("{}{}{connector}", rect("a b", 2), rect("n2", 9)), "1.0");
    assert_eq!(glued_ends(&source), 2);
    let (written, _) = visit(&Dir::new(), "v.odg", &source).unwrap();
    let content = part_of(&written, "content.xml");
    assert!(!content.contains("a b"), "the value that is no name is written nowhere: {content}");
    assert_pairs("a name that is none", &content, &["n2", "id1"]);
    assert_eq!(glued_ends(&written), 2, "both ends are still glued");
}
