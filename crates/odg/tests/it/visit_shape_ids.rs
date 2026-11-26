//! The id of a shape of an ODF 1.0 or 1.1 drawing is written as ODF 1.4 takes it (ODF 1.4 Part 3 §19.8,
//! the schema's `common-draw-id-attlist`).
//!
//! **What the full gate found.** A visit of an ODF 1.0 or 1.1 drawing is written from the model and says `office:version="1.4"`, and its shapes carried `draw:id` and no `xml:id`, as the file
//! had them (a part of those versions has no `xml:id`). The ODF 1.4 schema takes `draw:id` only beside an `xml:id`, so the part failed it (`invalid_attribute id rect at rect`).
//!
//! **What comes back.** A shape written with `draw:id` is written with an `xml:id` of the same value, and a connector that names a shape names it by that. An id the schema cannot take (a name
//! that is no NCName) is no id: the shape is written with none, and a connector glued to it gets one of the writer's own (`id<n>`). A part that declares ODF 1.0 or 1.1 and is saved into (the
//! splice) keeps what it says: `draw:id` alone. What a file of ODF 1.2 or later says of its ids, both attributes, comes back as it was.
//!
//! A visit is the crate-level one of PL5, as `visit_shapes.rs` makes it. Every read-back goes through the real reader, every part written is held to the ODF 1.4 RelaxNG schema when
//! `ODF_SCHEMA_RNG` names it (`model_safe::schema_errors`), and the tests also assert the attributes themselves, so that they are red without the schema. Every input is built in code from
//! the ODF 1.4 Part 3 vocabulary.

use diacraft_container::xml::{El, Node};
use diacraft_doc::Document;
use diacraft_odg::{Limits, OriginalPackage, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::gate_valid::old;
use crate::model_safe::{model_same, schema_errors};
use crate::visit_styles::{GR1, part, through_codec, visit};

const FRAME: &str = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;

/// One element of a part that holds an id: its name, its `xml:id` and its `draw:id`.
type Held = (String, Option<String>, Option<String>);

/// Every element of `el` and below that holds an `xml:id` or a `draw:id`, in document order. A `draw:glue-point` is left out: its `draw:id` is the number of the point, which no `xml:id`
/// goes beside.
fn collect(el: &El, out: &mut Vec<Held>) {
    let find = |name: &str| el.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    if el.name != "draw:glue-point" {
        let (xml, draw) = (find("xml:id"), find("draw:id"));
        if xml.is_some() || draw.is_some() {
            out.push((el.name.clone(), xml, draw));
        }
    }
    for child in el.children.iter() {
        if let Node::El(e) = child {
            collect(e, out);
        }
    }
}

/// The ids the content of `bytes` holds.
fn ids(bytes: &[u8]) -> Vec<Held> {
    let root = diacraft_container::xml::parse(part(bytes, "content.xml").as_bytes()).unwrap();
    let mut out = Vec::new();
    collect(&root, &mut out);
    out
}

fn some(s: &str) -> Option<String> {
    Some(s.to_owned())
}

/// What the ODF 1.4 schema says of every part of `bytes` (nothing when no schema is named), **but for the element `draw:connector`**: a connector the file of ODF 1.0 or 1.1 wrote with no
/// `svg:viewBox` (the schema of those versions has none, ODF 1.4's asks for it) is written with none, as it was before this task, and is a class of its own that this task leaves alone
/// (the report names it). Every other error, an id's above all, fails.
fn assert_valid(bytes: &[u8], what: &str) {
    let Some(checked) = schema_errors(bytes, what) else { return };
    if let Err(e) = checked {
        let others: Vec<&str> = e.split(" | ").filter(|line| !line.contains("element connector:") && !line.contains("fails to validate")).collect();
        assert!(others.is_empty(), "{what}: not valid ODF 1.4:\n{}", others.join("\n"));
    }
}

/// The version `content.xml` and `styles.xml` of `bytes` declare.
fn versions(bytes: &[u8]) -> Vec<String> {
    ["content.xml", "styles.xml"]
        .iter()
        .map(|name| {
            let text = part(bytes, name);
            let at = text.find("office:version=\"").unwrap() + "office:version=\"".len();
            text[at..].split('"').next().unwrap().to_owned()
        })
        .collect()
}

/// The package of `shapes` on one page, as ODF 1.0.
fn source(shapes: &str) -> Vec<u8> {
    old(&Drawing::a4().auto(GR1).page(shapes))
}

fn rect(id: &str, x: u32) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" draw:id="{id}" svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

fn rect_with(attrs: &str, x: u32) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" {attrs} svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

const CONNECTOR: &str = r#"<draw:connector draw:style-name="gr1" draw:start-shape="n1" draw:start-glue-point="1" draw:end-shape="n2" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="9cm" svg:y2="4cm"/>"#;

/// The visit of `source` (written from the model alone, with the content stamp), twice: the file written, and what a second visit of it wrote. Both parts are held to the schema.
fn visited_twice(what: &str, source: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (once, _) = visit(source).unwrap_or_else(|e| panic!("{what}: {e}"));
    let (twice, _) = visit(&once).unwrap_or_else(|e| panic!("{what}: the second visit: {e}"));
    assert_valid(&once, what);
    assert_valid(&twice, what);
    (once, twice)
}

/// [`visited_twice`], and the visit is **the same drawing**: the content check finds no difference between the file visited and the file written (nor between that and the second one),
/// and the model read from the file written is the one read from the file visited (`xml:id` is the pair's twin, which the content check counts as nothing when only the file written has it,
/// and which the reader holds beside the `draw:id` of a part of ODF 1.0 or 1.1).
fn same_twice(what: &str, source: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (once, found) = visit(source).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(found.is_empty(), "{what}: the content check finds the visit differs: {found:#?}");
    let (twice, found) = visit(&once).unwrap_or_else(|e| panic!("{what}: the second visit: {e}"));
    assert!(found.is_empty(), "{what}: the content check finds the second visit differs: {found:#?}");
    assert_valid(&once, what);
    assert_valid(&twice, what);
    // (A page with a connector is not held to the model: a shape that shares an automatic style with a connector is written under a name of its own, as it was before this task, and a
    // model holds the name the file gave the style.)
    if !part(source, "content.xml").contains("<draw:connector") {
        assert_eq!(model_same(&model(source), &model(&once)), Ok(()), "{what}: the model read from the file written is the file visited's");
        assert_eq!(model_same(&model(&once), &model(&twice)), Ok(()), "{what}: and the second visit's");
    }
    (once, twice)
}

/// The model of the file `bytes`.
fn model(bytes: &[u8]) -> Document {
    read(bytes, &Limits::default()).unwrap().0
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ variants A and B

/// **Variant A**: two rectangles with `draw:id` and no `xml:id`, in a file of ODF 1.0, and no connector.
#[test]
fn two_shapes_with_draw_id_alone_in_an_odf_1_0_file_come_back_with_an_xml_id() {
    let source = source(&format!("{}{}", rect("n1", 2), rect("n2", 9)));
    assert_eq!(versions(&source), ["1.0", "1.0"]);
    assert_eq!(ids(&source), [("draw:rect".into(), None, some("n1")), ("draw:rect".into(), None, some("n2"))], "the input has draw:id alone");
    let (once, twice) = same_twice("variant A", &source);
    assert_eq!(versions(&once), ["1.4", "1.4"], "a visit writes ODF 1.4");
    let pairs = [("draw:rect".into(), some("n1"), some("n1")), ("draw:rect".into(), some("n2"), some("n2"))];
    assert_eq!(ids(&once), pairs, "xml:id beside draw:id, equal");
    assert_eq!(ids(&twice), pairs, "a second visit writes the same ids");
    assert_eq!(part(&once, "content.xml"), part(&twice, "content.xml"), "and the same content");
}

/// **Variant B**: the two rectangles and a connector that names them by `draw:start-shape` and `draw:end-shape`. The connector still names the shapes, and the glue is read back after the
/// visit and after a second one.
#[test]
fn a_connector_glued_to_shapes_with_draw_id_alone_in_an_odf_1_0_file_stays_glued() {
    let source = source(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)));
    let (before, _) = read(&source, &Limits::default()).unwrap();
    assert_eq!(before.pages.get(0).unwrap().connects.len(), 2, "the input is glued at both ends");
    let (once, twice) = same_twice("variant B", &source);
    let pairs = [("draw:rect".into(), some("n1"), some("n1")), ("draw:rect".into(), some("n2"), some("n2"))];
    assert_eq!(ids(&once), pairs);
    assert_eq!(ids(&twice), pairs);
    for bytes in [&once, &twice] {
        let content = part(bytes, "content.xml");
        assert!(content.contains(r#"draw:start-shape="n1""#) && content.contains(r#"draw:end-shape="n2""#), "the ends name the shapes: {content}");
        let (doc, _) = read(bytes, &Limits::default()).unwrap();
        let page = doc.pages.get(0).unwrap();
        assert_eq!(page.connects.len(), 2, "both ends are glued");
        let mut to: Vec<u32> = page.connects.iter().map(|c| c.to_sheet).collect();
        to.sort_unstable();
        let mut shapes: Vec<u32> = page.shapes.iter().filter(|s| s.sheet.cell("BeginX").is_none()).map(|s| s.id).collect();
        shapes.sort_unstable();
        assert_eq!(to, shapes, "the ends are glued to the two rectangles");
    }
    assert_eq!(part(&once, "content.xml"), part(&twice, "content.xml"));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ every kind of shape

/// Each kind of shape the full gate named, in the form ODF 1.0 gives it, with `draw:id` alone.
fn kinds() -> Vec<(&'static str, String, &'static str)> {
    vec![
        ("a rectangle", format!(r#"<draw:rect draw:style-name="gr1" draw:id="k1" {FRAME}/>"#), "draw:rect"),
        ("an ellipse", format!(r#"<draw:ellipse draw:style-name="gr1" draw:id="k1" {FRAME}/>"#), "draw:ellipse"),
        ("a circle", format!(r#"<draw:circle draw:style-name="gr1" draw:id="k1" {FRAME}/>"#), "draw:circle"),
        ("a line", r#"<draw:line draw:style-name="gr1" draw:id="k1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="4cm"/>"#.to_owned(), "draw:line"),
        (
            "a custom shape",
            format!(
                r#"<draw:custom-shape draw:style-name="gr1" draw:id="k1" {FRAME}><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#
            ),
            "draw:custom-shape",
        ),
        (
            "a text frame",
            format!(r#"<draw:frame draw:style-name="gr1" draw:id="k1" {FRAME}><draw:text-box><text:p>Hi</text:p></draw:text-box></draw:frame>"#),
            "draw:frame",
        ),
    ]
}

#[test]
fn every_kind_of_shape_with_draw_id_alone_comes_back_with_an_xml_id() {
    for (what, shape, element) in kinds() {
        let source = source(&shape);
        assert_eq!(ids(&source), [(element.into(), None, some("k1"))], "{what}: the input has draw:id alone");
        let (once, twice) = same_twice(what, &source);
        assert_eq!(ids(&once), [(element.into(), some("k1"), some("k1"))], "{what}: xml:id beside draw:id");
        assert_eq!(ids(&twice), ids(&once), "{what}: a second visit writes the same ids");
        assert_eq!(part(&once, "content.xml"), part(&twice, "content.xml"), "{what}: and the same content");
    }
}

/// A shape with a glue point keeps the point's own `draw:id`, a number, and gets no `xml:id` for it.
#[test]
fn a_glue_point_keeps_its_own_number_and_gets_no_xml_id() {
    let rect = format!(
        r#"<draw:rect draw:style-name="gr1" draw:id="k1" {FRAME}><draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/></draw:rect>"#
    );
    let (once, _) = same_twice("a glue point", &source(&rect));
    let content = part(&once, "content.xml");
    assert!(content.contains(r#"<draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/>"#), "{content}");
    assert_eq!(ids(&once), [("draw:rect".into(), some("k1"), some("k1"))], "the shape's id pair, and none for the point");
}

/// A group and the members of it that hold ids: each is written with the pair, and a connector glued to the group names it.
#[test]
fn a_group_and_its_members_with_draw_id_alone_come_back_with_an_xml_id() {
    let group = format!(r#"<draw:g draw:id="g1">{}{}</draw:g>"#, rect("m1", 2), rect("m2", 9));
    let (once, twice) = same_twice("a group", &source(&group));
    let pairs =
        [("draw:g".into(), some("g1"), some("g1")), ("draw:rect".into(), some("m1"), some("m1")), ("draw:rect".into(), some("m2"), some("m2"))];
    assert_eq!(ids(&once), pairs);
    assert_eq!(ids(&twice), pairs);
    // A connector glued to the group, and to a member of it.
    let connector =
        r#"<draw:connector draw:style-name="gr1" draw:start-shape="g1" draw:end-shape="m2" svg:x1="1cm" svg:y1="1cm" svg:x2="9cm" svg:y2="4cm"/>"#;
    let (once, twice) = same_twice("a group with a connector", &source(&format!("{group}{connector}")));
    assert_eq!(ids(&once), pairs);
    for bytes in [&once, &twice] {
        let content = part(bytes, "content.xml");
        assert!(content.contains(r#"draw:start-shape="g1""#) && content.contains(r#"draw:end-shape="m2""#), "{content}");
        let (doc, _) = read(bytes, &Limits::default()).unwrap();
        assert_eq!(doc.pages.get(0).unwrap().connects.len(), 2, "both ends are glued");
    }
}

/// Every kind of shape on one page, each with an id of its own, and a connector between two of them.
#[test]
fn a_page_of_every_kind_comes_back_valid_and_the_same_twice() {
    let all: String = kinds().iter().enumerate().map(|(n, (_, shape, _))| shape.replace("\"k1\"", &format!("\"k{n}\""))).collect();
    let source = source(&format!(
        r#"{all}<draw:connector draw:style-name="gr1" draw:start-shape="k0" draw:end-shape="k2" svg:x1="1cm" svg:y1="1cm" svg:x2="9cm" svg:y2="4cm"/>"#
    ));
    let (once, twice) = same_twice("a page of every kind", &source);
    let found = ids(&once);
    assert_eq!(found.len(), 6);
    assert!(found.iter().all(|(_, xml, draw)| xml.is_some() && xml == draw), "{found:?}");
    assert_eq!(ids(&twice), found);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ what is not an id

/// A `draw:id` that is no NCName cannot be an `xml:id` (the schema types both as names): it is no id, the shape is written with none, and a connector glued to it names a new one.
#[test]
fn a_draw_id_that_is_no_ncname_is_written_as_no_id_and_a_connector_gets_one_of_the_writers() {
    for (what, bad) in [("a space", "a b"), ("a leading digit", "1x"), ("a colon", "a:b"), ("empty", ""), ("a leading dash", "-x")] {
        let shapes = format!("{}{}", rect(bad, 2), rect("n2", 9));
        let (once, twice) = visited_twice(what, &source(&shapes));
        assert_eq!(ids(&once), [("draw:rect".into(), some("n2"), some("n2"))], "{what}: the shape that has no id keeps none");
        assert_eq!(ids(&twice), ids(&once), "{what}: a second visit");
        // Glued: the target is named by an id of the writer's own. (An empty name names no shape: the reader glues that end to nothing.)
        let glued = format!(
            r#"{shapes}<draw:connector draw:style-name="gr1" draw:start-shape="{bad}" draw:end-shape="n2" svg:x1="6cm" svg:y1="4cm" svg:x2="9cm" svg:y2="4cm"/>"#
        );
        let (once, twice) = visited_twice(what, &source(&glued));
        let found = ids(&once);
        let ends = if bad.is_empty() { 1 } else { 2 };
        assert_eq!(found.len(), ends, "{what}: {found:?}");
        assert!(found.iter().all(|(_, xml, draw)| xml.is_some() && xml == draw), "{what}: {found:?}");
        assert_eq!(found.last(), Some(&("draw:rect".into(), some("n2"), some("n2"))), "{what}: the valid one as it was");
        for bytes in [&once, &twice] {
            let (doc, _) = read(bytes, &Limits::default()).unwrap();
            assert_eq!(doc.pages.get(0).unwrap().connects.len(), ends, "{what}: the glue survives");
        }
        assert_eq!(ids(&twice), found, "{what}: a second visit");
    }
}

/// The id the writer makes for a shape that holds none is above every number an id of the form `id<n>` holds in the file.
#[test]
fn an_id_the_writer_makes_is_above_every_id_the_file_holds() {
    let shapes = format!(
        r#"{}{}<draw:connector draw:style-name="gr1" draw:start-shape="a b" draw:end-shape="id7" svg:x1="1cm" svg:y1="1cm" svg:x2="9cm" svg:y2="4cm"/>"#,
        rect("a b", 2),
        rect("id7", 9)
    );
    let (once, twice) = visited_twice("a made id", &source(&shapes));
    let pairs = [("draw:rect".into(), some("id8"), some("id8")), ("draw:rect".into(), some("id7"), some("id7"))];
    assert_eq!(ids(&once), pairs);
    assert_eq!(ids(&twice), pairs);
}

/// A `draw:id` beside an `xml:id` is as it was read, as are two that differ and the ids of a file of ODF 1.2 or later: the pair a file carries is the file's.
#[test]
fn ids_that_a_file_carries_both_of_come_back_as_they_were() {
    let shapes =
        format!("{}{}{}", rect_with(r#"xml:id="a" draw:id="a""#, 2), rect_with(r#"xml:id="b" draw:id="c""#, 9), rect_with(r#"xml:id="d""#, 16));
    let mut drawing = Drawing::a4().auto(GR1).page(&shapes);
    drawing.version = "1.4".to_owned();
    let (once, twice) = same_twice("both", &drawing.bytes());
    let pairs = [("draw:rect".into(), some("a"), some("a")), ("draw:rect".into(), some("b"), some("c")), ("draw:rect".into(), some("d"), None)];
    assert_eq!(ids(&once), pairs);
    assert_eq!(ids(&twice), pairs);
}

/// A file of ODF 1.2 or later that holds `draw:id` alone is not one the schema takes: what is written from it is, and the content check counts the `xml:id` it gains as nothing.
#[test]
fn a_file_of_odf_1_3_with_draw_id_alone_is_written_as_a_valid_one() {
    let mut drawing = Drawing::a4().auto(GR1).page(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)));
    drawing.version = "1.3".to_owned();
    let source = drawing.bytes();
    let (once, found) = visit(&source).unwrap();
    assert!(found.is_empty(), "{found:#?}");
    assert_valid(&once, "odf 1.3");
    assert_eq!(ids(&once), [("draw:rect".into(), some("n1"), some("n1")), ("draw:rect".into(), some("n2"), some("n2"))]);
    let (doc, _) = read(&once, &Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).unwrap().connects.len(), 2, "the glue survives");
}

/// The content check is oriented: an `xml:id` the visit **loses** is a difference, and so is one that no longer says what the `draw:id` beside it says.
#[test]
fn the_content_check_still_finds_an_xml_id_that_was_lost_or_changed() {
    let mut drawing =
        Drawing::a4().auto(GR1).page(&format!("{}{}", rect_with(r#"xml:id="n1" draw:id="n1""#, 2), rect_with(r#"xml:id="n2" draw:id="n2""#, 9)));
    drawing.version = "1.4".to_owned();
    let pair = drawing.bytes();
    let alone = {
        let mut d = Drawing::a4().auto(GR1).page(&format!("{}{}", rect("n1", 2), rect("n2", 9)));
        d.version = "1.4".to_owned();
        d.bytes()
    };
    let changed = {
        let mut d = Drawing::a4().auto(GR1).page(&format!(
            "{}{}",
            rect_with(r#"xml:id="n1" draw:id="n1""#, 2),
            rect_with(r#"xml:id="other" draw:id="n2""#, 9)
        ));
        d.version = "1.4".to_owned();
        d.bytes()
    };
    let found = |a: &[u8], b: &[u8]| {
        diacraft_odg::compare_content(a, b, &Limits::default()).unwrap().shown.iter().filter(|d| d.part == "content.xml").count()
    };
    assert_eq!(found(&alone, &pair), 0, "the twin the written file gained");
    assert_eq!(found(&pair, &pair), 0);
    assert_eq!(found(&pair, &alone), 2, "the twin the written file lost");
    assert_eq!(found(&pair, &changed), 1, "the twin that says another id");
    assert_eq!(found(&alone, &changed), 1, "and one that says another id than its draw:id's, which only the written file holds");
}

/// A `draw:id` that is no name and is written in neither attribute is said in the note of the save (a new shape's attribute the writer did not write).
#[test]
fn a_draw_id_that_is_no_name_is_said_in_the_notes_of_the_save() {
    let (doc, _) = read(&source(&format!("{}{}", rect("a b", 2), rect("n2", 9))), &Limits::default()).unwrap();
    let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let said: Vec<String> =
        written.notes.iter().filter(|n| matches!(n, diacraft_odg::SaveNote::NewShapeUnwritten { .. })).map(ToString::to_string).collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("1 attribute"), "{said:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ a flat drawing

/// A flat drawing of ODF 1.0 or 1.1 is read as the package that holds the same elements and written from the model as a package of ODF 1.4: its ids come back as pairs.
#[test]
fn a_flat_drawing_of_odf_1_1_with_draw_id_alone_comes_back_with_an_xml_id() {
    for version in ["1.0", "1.1"] {
        let mut drawing = Drawing::a4().auto(GR1).page(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)));
        drawing.version = version.to_owned();
        let flat = Flat::of(&drawing).bytes();
        let (doc, _) = read(&flat, &Limits::default()).unwrap();
        let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
        assert_valid(&written.bytes, "a flat visit");
        let pairs = [("draw:rect".into(), some("n1"), some("n1")), ("draw:rect".into(), some("n2"), some("n2"))];
        assert_eq!(ids(&written.bytes), pairs, "{version}");
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        assert_eq!(back.pages.get(0).unwrap().connects.len(), 2, "{version}: the glue survives");
    }
}

/// The package written is saved as a flat document (`Form::Flat`): the document says what the package said, ids included.
#[test]
fn the_package_a_visit_wrote_is_saved_as_a_flat_document_with_its_ids_as_pairs() {
    let (once, _) = visited_twice("a package to flat", &source(&format!("{}{}", rect("n1", 2), rect("n2", 9))));
    let (doc, report) = read(&once, &Limits::default()).unwrap();
    let flat = write_with(
        &doc,
        Some(&OriginalPackage { bytes: &once, index: &report.index }),
        &WriteOptions { form: Some(diacraft_odg::Form::Flat), ..WriteOptions::default() },
    )
    .unwrap();
    let text = String::from_utf8(flat.bytes).unwrap();
    assert_eq!(text.matches(r#"xml:id="n1" draw:id="n1""#).count(), 1, "{text}");
    assert_eq!(text.matches(r#"xml:id="n2" draw:id="n2""#).count(), 1, "{text}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the splice: a part keeps the version it declares

fn moved(source: &[u8]) -> (Document, diacraft_odg::ShapeIndex) {
    let (mut doc, report) = read(source, &Limits::default()).unwrap();
    crate::write_core::move_by(&mut doc, 0, 1, 1.0, 0.0);
    (doc, report.index)
}

/// A save into a file of ODF 1.0 keeps what the file says: its version, and `draw:id` alone (the schema of that version has no `xml:id`). A connector glued to a shape that held no id gets
/// `draw:id` alone there too.
#[test]
fn a_save_into_a_file_of_odf_1_0_keeps_draw_id_alone() {
    let source = source(&format!("{}{}{CONNECTOR}", rect("n1", 2), rect("n2", 9)));
    let (doc, index) = moved(&source);
    let saved = write_with(&doc, Some(&OriginalPackage { bytes: &source, index: &index }), &WriteOptions::default()).unwrap();
    assert!(saved.edited);
    assert_eq!(versions(&saved.bytes), ["1.0", "1.0"], "the version is the file's");
    assert_eq!(ids(&saved.bytes), [("draw:rect".into(), None, some("n1")), ("draw:rect".into(), None, some("n2"))]);
}

/// A save into a file of ODF 1.3 that holds the pair keeps it as it was.
#[test]
fn a_save_into_a_file_of_odf_1_3_keeps_the_pair() {
    let shapes = format!("{}{}", rect_with(r#"xml:id="n1" draw:id="n1""#, 2), rect_with(r#"xml:id="n2" draw:id="n2""#, 9));
    let source = Drawing::a4().auto(GR1).page(&shapes).bytes();
    let (doc, index) = moved(&source);
    let saved = write_with(&doc, Some(&OriginalPackage { bytes: &source, index: &index }), &WriteOptions::default()).unwrap();
    assert_eq!(versions(&saved.bytes), ["1.3", "1.3"]);
    assert_eq!(ids(&saved.bytes), [("draw:rect".into(), some("n1"), some("n1")), ("draw:rect".into(), some("n2"), some("n2"))]);
}
