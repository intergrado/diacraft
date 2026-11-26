//! Through the engine (PL5's second leg): an `.odg` is opened, Save As `.diacraft`, a new session opens that file, Save As `.odg`, and the content check judges the file that
//! came back against the file that was opened, in the two parts that say what the drawing is (`content.xml` and `styles.xml`). A second visit of what came back is the same bytes (clock
//! pinned). The crate-level tests of the same classes are `crates/odg/tests/it/visit_shapes.rs`.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{GR1, GR2, P1, part_of};
use crate::run;

/// The parts a visit is judged by.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// A drawing as ODF 1.4, the version the model writer writes (the version is no class of this task), whose automatic styles are `auto` beside the ones the builder gives it and
/// whose page holds `shapes`.
fn source(auto: &str, shapes: &str) -> Vec<u8> {
    source_with("", auto, shapes)
}

/// [`source`] with the common styles `common` (`office:styles`).
fn source_with(common: &str, auto: &str, shapes: &str) -> Vec<u8> {
    let mut drawing = crate::odg_build::Drawing::new().page(shapes);
    drawing.auto = format!("{GR1}{GR2}{P1}{auto}");
    drawing.common = common.to_owned();
    let (content, styles) = (
        drawing.content_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
        drawing.styles_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
    );
    let files = vec![
        ("content.xml".to_owned(), content.into_bytes()),
        ("styles.xml".to_owned(), styles.into_bytes()),
        ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
    ];
    crate::odg_build::package(crate::odg_build::DRAWING, files)
}

/// What the content check says of the drawing that went through `.diacraft` and came back, in the drawing's two parts, and the file that came back.
fn visit(dir: &Dir, source: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    if let Err(e) = t.execute("file.save_as", &json!({ "path": dir.path("w.odg") })) {
        return Err(format!("{e}"));
    }
    let written = dir.read("w.odg");
    let diff = diacraft_odg::compare_content(source, &written, &diacraft_odg::Limits::default()).unwrap();
    let shown =
        diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    Ok((written, shown))
}

/// The visit is the same content, and a visit of what came back is the same bytes.
fn assert_same(what: &str, source: &[u8]) -> Vec<u8> {
    let (written, found) = visit(&Dir::new(), source).unwrap_or_else(|e| panic!("{what}: refused: {e}"));
    assert!(found.is_empty(), "{what}: the visit differs from the file: {found:#?}");
    let (again, found) = visit(&Dir::new(), &written).unwrap_or_else(|e| panic!("{what}: the second visit is refused: {e}"));
    assert!(found.is_empty(), "{what}: the second visit differs from the file: {found:#?}");
    assert_eq!(again, written, "{what}: a visit of what came back is the same bytes (clock pinned)");
    written
}

fn plain(attrs: &str, x: u32) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" {attrs} svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

fn members() -> String {
    format!("{}{}", plain("", 2), plain("", 9))
}

const POINT: &str = r#"<draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/>"#;

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 1

#[test]
fn the_glue_points_of_a_plain_shape_come_back() {
    let shape = format!(
        r#"<draw:rect draw:style-name="gr1" xml:id="a" draw:id="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{POINT}</draw:rect>"#
    );
    let connector = r#"<draw:connector draw:style-name="gr1" draw:start-shape="a" draw:start-glue-point="5" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#;
    let written = assert_same("a plain shape with a glue point and a connector on it", &source("", &format!("{shape}{connector}")));
    assert!(part_of(&written, "content.xml").contains(POINT), "the element, with exactly its own four attributes");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 2

#[test]
fn the_ids_of_the_targets_of_connectors_come_back_as_they_were() {
    let connector = |a: &str, b: &str| {
        format!(
            r#"<draw:connector draw:style-name="gr1" draw:start-shape="{a}" draw:start-glue-point="1" draw:end-shape="{b}" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#
        )
    };
    for (what, one, two, names) in [
        ("equal ids", r#"xml:id="alpha" draw:id="alpha""#, r#"xml:id="beta" draw:id="beta""#, ("alpha", "beta")),
        ("draw:id alone", r#"draw:id="first""#, r#"draw:id="second""#, ("first", "second")),
        ("ids that differ on one shape", r#"xml:id="x1" draw:id="y1""#, r#"xml:id="x2" draw:id="y2""#, ("x1", "x2")),
    ] {
        let shapes = format!("{}{}{}", plain(one, 2), plain(two, 9), connector(names.0, names.1));
        let written = assert_same(what, &source("", &shapes));
        let content = part_of(&written, "content.xml");
        assert!(content.contains(one) && content.contains(two), "{what}: written as they were: {content}");
    }
}

#[test]
fn two_shapes_that_claim_one_id_come_back_as_they_were() {
    let connector = r#"<draw:connector draw:style-name="gr1" draw:start-shape="same" draw:start-glue-point="1" draw:end-shape="same" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#;
    let shapes = format!("{}{}{connector}", plain(r#"xml:id="same" draw:id="same""#, 2), plain(r#"xml:id="same" draw:id="same""#, 9));
    assert_same("one id held twice", &source("", &shapes));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ classes 3, 4 and 5

#[test]
fn a_group_that_had_no_name_gets_none_and_its_members_keep_their_styles() {
    let common = r##"<style:style style:name="Plain" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let auto = r##"<style:style style:name="gr4" style:family="graphic" style:parent-style-name="Plain"><style:graphic-properties draw:fill-color="#00ff00"/></style:style>"##;
    let member = r#"<draw:rect draw:style-name="gr4" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    let written = assert_same("a group with no attribute", &source_with(common, auto, &format!("<draw:g>{member}{}</draw:g>", plain("", 9))));
    let content = part_of(&written, "content.xml");
    assert!(!content.contains("Group_"), "{content}");
    assert!(content.contains(r#"style:parent-style-name="Plain""#), "the member keeps its parent: {content}");
    // A group in a group, and a member beside it: none is named, and the members keep their parents.
    let nested = format!("<draw:g><draw:g>{member}</draw:g>{}</draw:g>", plain("", 9));
    let written = assert_same("a group in a group", &source_with(common, auto, &nested));
    let content = part_of(&written, "content.xml");
    assert!(!content.contains("Group_") && content.contains(r#"style:parent-style-name="Plain""#), "{content}");
}

/// The control: a group a person makes in DiaCraft holds no connection points (its entry in DiaCraft's document file says so), so it needs a key and is named after its `ID`, and the key is
/// unique on its page; a group the file had, with the four it gives every shape, needs none (the test above).
#[test]
fn a_group_made_in_diacraft_is_still_named_by_its_id() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", &source("", &members())) }));
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
    let group = run(&mut s, "shape.group", json!({ "ids": ids }))["id"].as_u64().unwrap();
    run(&mut s, "file.save_as", json!({ "path": dir.path("g.diacraft") }));
    let mut t = opened(&dir.path("g.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("g.odg") }));
    let content = part_of(&dir.read("g.odg"), "content.xml");
    assert!(content.contains(&format!("draw:name=\"Group_{group}\"")), "{content}");
}

#[test]
fn a_group_with_no_name_whose_own_markup_gives_it_a_style_comes_back() {
    let auto = r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    assert_same("a group on a style", &source(auto, &format!(r#"<draw:g draw:style-name="gr3">{}</draw:g>"#, members())));
}

#[test]
fn a_group_with_no_name_whose_own_markup_gives_it_a_style_and_glue_points_comes_back() {
    let auto = r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let connector =
        r#"<draw:connector draw:style-name="gr1" draw:end-shape="g" draw:end-glue-point="5" svg:x1="1cm" svg:y1="1cm" svg:x2="9cm" svg:y2="4cm"/>"#;
    assert_same(
        "a style, a glue point and a connector on it",
        &source(auto, &format!(r#"<draw:g xml:id="g" draw:id="g" draw:style-name="gr3">{POINT}{}</draw:g>{connector}"#, members())),
    );
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the route kind of a connector

/// Two rectangles joined by a connector whose `ShapeRouteStyle` is 0 (the page's own route style, which a VSDX connector says) and that holds no
/// `ConLineRouteExt`, saved as an `.odg` by Save As: the dry run and the save are no refusal ("a new connector of a route kind the writer does not write"), the check names no class, and the
/// file read again holds the connector with the cells it had.
#[test]
fn a_connector_whose_route_style_is_the_pages_own_is_saved_as_an_odg_and_the_check_names_no_class() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.new", json!({}));
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3.0, "y": 5.0 }))["id"].as_u64().unwrap();
    let b = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 8.0, "y": 5.0 }))["id"].as_u64().unwrap();
    let c = run(&mut s, "connector.add", json!({ "from": { "id": a, "point": 3 }, "to": { "id": b, "point": 1 } }))["id"].as_u64().unwrap();
    s.edit("Test setup: the page's own route style", |doc: &mut diacraft_doc::Document| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| u64::from(x.id) == c).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        sheet
            .cells
            .insert(diacraft_doc::CellName::new("ShapeRouteStyle"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(0.0).unwrap()));
        sheet.cells.remove("ConLineRouteExt");
        Ok(())
    })
    .unwrap();
    let path = dir.path("v9.odg");
    let answer = run(&mut s, "file.check_compatibility", json!({ "path": path }));
    assert_eq!(answer["writer"], "odg_model", "{answer}");
    assert!(answer["checked"]["classes"].as_array().is_some_and(Vec::is_empty), "the check names no class: {answer}");
    let saved = run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!((&answer["carried"], &answer["checked"]), (&saved["carried"], &saved["checked"]), "the dry run says what the save says");
    let mut again = pinned();
    run(&mut again, "file.open", json!({ "path": path }));
    let page = again.document().unwrap().pages.get(0).unwrap().clone();
    let connector = page.shapes.iter().find(|x| x.sheet.cell("ShapeRouteStyle").is_some()).expect("the connector");
    let number = |name: &str| connector.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
    // (The copy a save of a drawing made in DiaCraft writes gives a connector both route cells: the extension the engine draws it in is 1.)
    assert_eq!((number("ShapeRouteStyle"), number("ConLineRouteExt")), (Some(0.0), Some(1.0)), "the style it had");
    assert_eq!(page.connects.len(), 2, "both ends are still glued");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ a new target

/// A connector made in DiaCraft between two shapes of the file, one that holds an id and one that holds none, written from the model alone: the first is named by the id it holds, the second
/// is given an id no shape holds (the number above the largest the file's ids have), and the file read again has both ends glued, and a visit of it is the same bytes.
#[test]
fn a_new_target_is_given_an_id_no_other_shape_holds_and_the_one_that_holds_one_keeps_it() {
    let shapes = format!("{}{}{}", plain(r#"xml:id="id7" draw:id="id7""#, 2), plain("", 9), plain(r#"xml:id="other" draw:id="other""#, 16),);
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", &source("", &shapes)) }));
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
    run(&mut s, "connector.add", json!({ "from": { "id": ids[0], "point": 3 }, "to": { "id": ids[1], "point": 1 } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("c.diacraft") }));
    let mut t = opened(&dir.path("c.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("c.odg") }));
    let written = dir.read("c.odg");
    let content = part_of(&written, "content.xml");
    assert!(
        content.contains(r#"xml:id="id7" draw:id="id7""#) && content.contains(r#"xml:id="other" draw:id="other""#),
        "the ids the shapes held: {content}"
    );
    assert!(content.contains(r#"xml:id="id8" draw:id="id8""#), "the new target is given the number above the largest: {content}");
    assert!(content.contains(r#"draw:start-shape="id7""#) && content.contains(r#"draw:end-shape="id8""#), "{content}");
    let mut again = pinned();
    run(&mut again, "file.open", json!({ "path": dir.path("c.odg") }));
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().connects.len(), 2, "both ends are glued");
}

/// **A copy of a shape of the file** (a duplicate, a paste) is a shape of its own: the engine's copy of the drawing hands the writer the copy with none of the ids of the shape it was copied from
/// (`ext_without_ids`), so a connector made in DiaCraft that is glued to the copy is written and names the id the copy is given, and the id of the shape of the file is written once, for it.
#[test]
fn a_connector_glued_to_a_copy_of_a_shape_of_the_file_is_saved() {
    let shapes = format!("{}{}", plain(r#"xml:id="orig" draw:id="orig""#, 2), plain("", 9));
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", &source("", &shapes)) }));
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
    let copy = run(&mut s, "shape.duplicate", json!({ "ids": [ids[0]] }));
    let copy_id = copy["ids"][0].as_u64().or(copy["id"].as_u64()).unwrap_or_else(|| panic!("{copy}"));
    run(&mut s, "connector.add", json!({ "from": { "id": copy_id, "point": 3 }, "to": { "id": ids[1], "point": 1 } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("d.diacraft") }));
    let mut t = opened(&dir.path("d.diacraft"));
    let saved = t.execute("file.save_as", &json!({ "path": dir.path("d.odg") }));
    assert!(saved.is_ok(), "{saved:?}");
    let mut again = pinned();
    run(&mut again, "file.open", json!({ "path": dir.path("d.odg") }));
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().connects.len(), 2, "both ends of the connector are glued");
    let content = part_of(&dir.read("d.odg"), "content.xml");
    assert_eq!(content.matches(r#"xml:id="orig""#).count(), 1, "the id of the shape of the file is written once, for it: {content}");
}
