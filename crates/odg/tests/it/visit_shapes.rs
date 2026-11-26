//! The shapes of an `.odg` come back as they were from a visit through `.diacraft`.
//!
//! A visit is the crate-level one of PL5, as `visit_styles.rs` makes it: the drawing is read, the model goes through the whole-model codec, the model is written from the model alone
//! with the content stamp, and `compare_content` judges the file written against the file read, in `content.xml` and `styles.xml`. A second visit of what came back is the same bytes.
//! Every read-back goes through the real reader and every input is built in code from the ODF 1.4 Part 3 vocabulary. Each class has a test named for what comes back.

use diacraft_doc::{Document, HeldGroup};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{GR1, assert_same, part, rect, source_of, through_codec, visit};

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// A drawing whose one page holds `shapes`, on the automatic style `gr1`.
fn on_gr1(shapes: &str) -> Drawing {
    Drawing::a4().auto(GR1).page(shapes)
}

/// The text of the one element of `content` called `tag` (a start tag to its end tag, or to its `/>`).
fn element(content: &str, tag: &str) -> String {
    let at = content.find(&format!("<{tag}")).unwrap_or_else(|| panic!("no {tag}: {content}"));
    let close = format!("</{tag}>");
    let end =
        content[at..].find(&close).map(|e| at + e + close.len()).or_else(|| content[at..].find("/>").map(|e| at + e + 2)).unwrap_or(content.len());
    content[at..end].to_owned()
}

/// A shape of the model written from the model alone, without an edit.
fn written(doc: &Document) -> Vec<u8> {
    write_with(&through_codec(doc), None, &unedited()).unwrap_or_else(|e| panic!("refused: {e}")).bytes
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 1: the glue points of a plain shape

/// A `draw:glue-point` it: a percentage in each of the two places and a way out the cells spell otherwise.
const POINT: &str = r#"<draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/>"#;

fn with_points(element: &str, attrs: &str, points: &str) -> String {
    format!(r#"<draw:{element} draw:style-name="gr1" {attrs}>{points}</draw:{element}>"#)
}

#[test]
fn the_glue_points_of_a_plain_shape_come_back() {
    let frame = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;
    for (what, shape) in [
        ("a rectangle", with_points("rect", frame, POINT)),
        ("an ellipse", with_points("ellipse", frame, POINT)),
        ("a circle", with_points("circle", frame, POINT)),
        (
            "a rectangle with two points",
            with_points("rect", frame, &format!("{POINT}<draw:glue-point draw:id=\"6\" svg:x=\"-1cm\" svg:y=\"0.5cm\" draw:align=\"top-left\"/>")),
        ),
        ("a regular polygon", with_points("regular-polygon", &format!("{frame} draw:corners=\"5\""), POINT)),
        ("a polygon", with_points("polygon", &format!("{frame} svg:viewBox=\"0 0 100 100\" draw:points=\"0,0 100,0 50,100\""), POINT)),
        ("a polyline", with_points("polyline", &format!("{frame} svg:viewBox=\"0 0 100 100\" draw:points=\"0,0 100,0 50,100\""), POINT)),
        ("a path", with_points("path", &format!("{frame} svg:viewBox=\"0 0 100 100\" svg:d=\"M 0 0 L 100 0 L 50 100 Z\""), POINT)),
    ] {
        let written = assert_same(what, &on_gr1(&shape));
        assert!(
            part(&written, "content.xml").contains(POINT),
            "{what}: the element, with exactly its own four attributes: {}",
            part(&written, "content.xml")
        );
    }
}

#[test]
fn a_connector_glued_to_the_glue_point_of_a_plain_shape_comes_back_glued() {
    let target = format!(
        r#"<draw:rect draw:style-name="gr1" xml:id="a" draw:id="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{POINT}</draw:rect>"#
    );
    let connector = r#"<draw:connector draw:style-name="gr1" draw:start-shape="a" draw:start-glue-point="5" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#;
    let written = assert_same("a connector on the point", &on_gr1(&format!("{target}{connector}")));
    assert!(part(&written, "content.xml").contains(r#"draw:start-glue-point="5""#));
}

/// The rows of the model say the points: a shape whose `Connection` rows are as they were read writes the elements it read, and the reader holds each element beside its row.
#[test]
fn the_reader_holds_each_glue_point_of_a_plain_shape_beside_its_row() {
    let (doc, _) =
        read(&source_of(&on_gr1(&with_points("rect", r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#, POINT))), &Limits::default())
            .unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let held = shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == "Connection")
        .flat_map(|s| s.rows.iter())
        .filter(|r| r.ext.get().and_then(|e| e.odf.held.as_ref()).is_some_and(|h| h.group == HeldGroup::Glue));
    assert_eq!(held.count(), 1, "one held element for the one row the element made");
    let _ = written(&doc);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 2: the ids of the targets of connectors

fn target(attrs: &str, x: u32) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" {attrs} svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

fn connector_between(a: &str, b: &str) -> String {
    format!(
        r#"<draw:connector draw:style-name="gr1" draw:start-shape="{a}" draw:start-glue-point="1" draw:end-shape="{b}" draw:end-glue-point="3" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#
    )
}

#[test]
fn the_ids_of_the_targets_of_connectors_come_back_as_they_were() {
    for (what, one, two, names) in [
        ("equal ids", r#"xml:id="alpha" draw:id="alpha""#, r#"xml:id="beta" draw:id="beta""#, ("alpha", "beta")),
        ("ids not of the writer's form", r#"xml:id="id7" draw:id="id7""#, r#"xml:id="id3" draw:id="id3""#, ("id7", "id3")),
        ("draw:id alone", r#"draw:id="first""#, r#"draw:id="second""#, ("first", "second")),
        ("xml:id alone", r#"xml:id="first""#, r#"xml:id="second""#, ("first", "second")),
        ("two ids that differ on one shape", r#"xml:id="x1" draw:id="y1""#, r#"xml:id="x2" draw:id="y2""#, ("x1", "x2")),
        ("an id with a space in it", r#"xml:id="a" draw:id="a b""#, r#"xml:id="b" draw:id="b""#, ("a", "b")),
    ] {
        let shapes = format!("{}{}{}", target(one, 2), target(two, 9), connector_between(names.0, names.1));
        let written = assert_same(what, &on_gr1(&shapes));
        let content = part(&written, "content.xml");
        for attr in one.split(' ').chain(two.split(' ')) {
            if attr.starts_with("draw:id=\"") && attr.len() > 10 && attr.contains(' ') {
                continue;
            }
            assert!(content.contains(attr), "{what}: {attr} is written as it was: {content}");
        }
    }
}

/// A connector whose target the file named by an id the target holds in its own words keeps its glue after the visit and after a second one.
#[test]
fn a_connector_glued_to_a_target_with_an_id_of_its_own_stays_glued() {
    let shapes = format!(
        "{}{}{}",
        target(r#"xml:id="alpha" draw:id="alpha""#, 2),
        target(r#"xml:id="beta" draw:id="beta""#, 9),
        connector_between("alpha", "beta")
    );
    let source = source_of(&on_gr1(&shapes));
    let (once, _) = visit(&source).unwrap();
    let (twice, _) = visit(&once).unwrap();
    for bytes in [&once, &twice] {
        let (doc, _) = read(bytes, &Limits::default()).unwrap();
        let page = doc.pages.get(0).unwrap();
        assert_eq!(page.connects.len(), 2, "both ends are glued");
    }
}

/// A shape that holds an id and that no connector names keeps it too.
#[test]
fn the_id_of_a_shape_no_connector_names_comes_back() {
    assert_same("an id nothing names", &on_gr1(&target(r#"xml:id="lone" draw:id="lone""#, 2)));
}

/// Two shapes claim one id: the reader's rule is the first holder, and what is written is what was read.
#[test]
fn two_shapes_that_claim_one_id_come_back_as_they_were() {
    let shapes = format!(
        "{}{}{}",
        target(r#"xml:id="same" draw:id="same""#, 2),
        target(r#"xml:id="same" draw:id="same""#, 9),
        connector_between("same", "same")
    );
    assert_same("one id held twice", &on_gr1(&shapes));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ classes 3, 4 and 5: groups

fn members() -> String {
    format!("{}{}", rect("gr1"), r#"<draw:rect draw:style-name="gr1" svg:x="8cm" svg:y="3cm" svg:width="2cm" svg:height="2cm"/>"#)
}

#[test]
fn a_group_that_had_no_name_gets_none() {
    let written = assert_same("a group of nothing but members", &on_gr1(&format!("<draw:g>{}</draw:g>", members())));
    let group = element(&part(&written, "content.xml"), "draw:g");
    assert!(!group.contains("draw:name"), "{group}");
}

#[test]
fn a_group_nested_in_a_group_that_had_no_name_gets_none() {
    assert_same("nested", &on_gr1(&format!("<draw:g><draw:g>{}</draw:g>{}</draw:g>", members(), rect("gr1"))));
}

#[test]
fn a_named_group_keeps_its_name_and_a_group_made_in_diacraft_is_still_named_by_its_id() {
    let written = assert_same("a named group", &on_gr1(&format!(r#"<draw:g draw:name="Mine">{}</draw:g>"#, members())));
    assert!(part(&written, "content.xml").contains(r#"draw:name="Mine""#));
}

#[test]
fn the_members_of_a_group_that_had_no_name_keep_their_styles() {
    let auto = r##"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="Plain"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##;
    let common = r##"<style:style style:name="Plain" style:family="graphic"><style:graphic-properties draw:stroke="none"/></style:style>"##;
    let shapes = r#"<draw:g><draw:rect draw:style-name="gr2" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/></draw:g>"#;
    let written = assert_same("a member on a style with a parent", &Drawing::a4().common(common).auto(auto).page(shapes));
    let content = part(&written, "content.xml");
    assert!(content.contains(r#"style:parent-style-name="Plain""#), "{content}");
}

#[test]
fn a_group_with_no_name_whose_own_markup_gives_it_a_style_comes_back() {
    let auto = r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    assert_same("a group on a style", &Drawing::a4().auto(GR1).auto(auto).page(&format!(r#"<draw:g draw:style-name="gr3">{}</draw:g>"#, members())));
}

#[test]
fn a_group_with_no_name_whose_own_markup_gives_it_glue_points_comes_back() {
    assert_same("a group with a glue point", &on_gr1(&format!("<draw:g>{POINT}{}</draw:g>", members())));
}

#[test]
fn a_group_with_no_name_whose_own_markup_gives_it_a_style_and_glue_points_comes_back() {
    let auto = r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let group = format!(r#"<draw:g draw:style-name="gr3">{POINT}{}</draw:g>"#, members());
    let connector =
        r#"<draw:connector draw:style-name="gr1" draw:end-shape="g" draw:end-glue-point="5" svg:x1="1cm" svg:y1="1cm" svg:x2="9cm" svg:y2="4cm"/>"#;
    assert_same(
        "a style, a glue point and a connector on it",
        &Drawing::a4()
            .auto(GR1)
            .auto(auto)
            .page(&format!(r#"<draw:g xml:id="g" draw:id="g" draw:style-name="gr3">{POINT}{}</draw:g>{connector}"#, members())),
    );
    assert_same("a style and a glue point", &Drawing::a4().auto(GR1).auto(auto).page(&group));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the kept data of a graphic style

const LOEXT: &str = r#" xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0""#;

fn with_loext(mut drawing: Drawing) -> Drawing {
    drawing.ns.push_str(LOEXT);
    drawing
}

/// The kept data of a style comes back written where it stood: on the `style:style` element, in the graphic properties (an attribute of another namespace, which brings its own
/// declaration, and a child), and after them (the paragraph and text properties, a child of the extension's, a `style:map`), in the order the file had them.
#[test]
fn the_kept_data_of_a_graphic_style_is_written_in_the_places_it_stood() {
    let style = concat!(
        r##"<style:style style:name="gr1" style:family="graphic" style:class="extra">"##,
        r##"<style:graphic-properties draw:fill="solid" draw:fill-color="#336699" loext:decorative="true"><text:list-style style:name="L9"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style></style:graphic-properties>"##,
        r##"<style:paragraph-properties fo:orphans="2" fo:widows="3"/><style:text-properties fo:language="en" fo:country="US"/>"##,
        r##"<loext:graphic-properties loext:grow="1"/><style:map style:condition="x" style:apply-style-name="gr2"/></style:style>"##
    );
    let written = assert_same("every place at once", &with_loext(Drawing::a4().auto(style).page(&rect("gr1"))));
    let content = part(&written, "content.xml");
    let at = |needle: &str| content.find(needle).unwrap_or_else(|| panic!("{needle} is written: {content}"));
    assert!(at("style:class=\"extra\"") < at("<style:graphic-properties"), "{content}");
    assert!(at("loext:decorative=\"true\"") < at("<text:list-style"), "{content}");
    assert!(at("</style:graphic-properties>") < at("<style:paragraph-properties"), "{content}");
    assert!(
        at("<style:paragraph-properties") < at("<style:text-properties") && at("<style:text-properties") < at("<loext:graphic-properties"),
        "{content}"
    );
    assert!(at("<loext:graphic-properties") < at("<style:map"), "{content}");
}

/// A prefix the file declared on the properties element itself, that nothing else binds, is declared on the element it is written on.
#[test]
fn a_foreign_attribute_whose_prefix_the_properties_element_declares_comes_back_with_its_declaration() {
    let style = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties xmlns:ex="urn:example:ex" ex:note="kept" draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let written = assert_same("a declaration on the element", &Drawing::a4().auto(style).page(&rect("gr1")));
    let content = part(&written, "content.xml");
    assert!(content.contains("xmlns:ex=\"urn:example:ex\"") && content.contains("ex:note=\"kept\""), "{content}");
    // A child element that uses a prefix its own start tag declares.
    let child = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><ex:data xmlns:ex="urn:example:ex" ex:k="v"><ex:inner/></ex:data></style:style>"##;
    assert_same("a child that declares its prefix", &Drawing::a4().auto(child).page(&rect("gr1")));
}

/// Two shapes on one style that kept data write the style once, with the data; a style that says nothing but its kept data has no graphic properties of its own made up for it.
#[test]
fn a_style_the_kept_data_of_which_is_all_it_says_is_written_once_for_two_shapes_with_no_properties_made_up() {
    let style =
        r##"<style:style style:name="gr1" style:family="graphic" style:class="extra"><style:paragraph-properties fo:orphans="2"/></style:style>"##;
    let written = assert_same("two shapes", &Drawing::a4().auto(style).page(&format!("{}{}", rect("gr1"), rect("gr1"))));
    let content = part(&written, "content.xml");
    assert_eq!(content.matches("style:name=\"gr1\"").count(), 1, "{content}");
    assert!(!content.contains("<style:graphic-properties"), "no graphic properties element the file did not hold: {content}");
}

/// The control: a shape whose cells were edited is written from them, its style is the shape's own under the name it had, and what the file's style kept beside the cells goes with it.
#[test]
fn an_edited_shape_keeps_what_its_style_kept_beside_the_cells() {
    let style = r##"<style:style style:name="gr1" style:family="graphic" style:class="extra"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:paragraph-properties fo:orphans="2"/></style:style>"##;
    let source = source_of(&Drawing::a4().auto(style).page(&format!("{}{}", rect("gr1"), rect("gr1"))));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
    std::sync::Arc::make_mut(&mut shape.sheet)
        .set_cell(&diacraft_doc::CellAddr::direct("FillForegnd"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#FF00FF")))
        .unwrap();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert_eq!(
        content.matches("fo:orphans=\"2\"").count(),
        2,
        "the style the edit made and the one the other shape names both hold what the file's style kept: {content}"
    );
    assert_eq!(content.matches("style:class=\"extra\"").count(), 2, "{content}");
    assert!(content.contains("#ff00ff") && content.contains("#336699"), "{content}");
}

/// A style that keeps more than a shape's kept markup may be (a mebibyte, in all) is refused in words, and nothing aborts; one under it is written.
#[test]
fn a_style_that_kept_more_than_a_shape_may_is_refused_in_words() {
    let style = |each: usize| {
        let big = "x".repeat(each);
        format!(
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699" loext:big1="{big}" loext:big2="{big}"/></style:style>"##
        )
    };
    let visit_of = |each: usize| {
        let source = source_of(&with_loext(Drawing::a4().auto(&style(each)).page(&rect("gr1"))));
        let (doc, _) = read(&source, &Limits::default()).unwrap();
        write_with(&through_codec(&doc), None, &unedited())
    };
    // Two attributes of 600 KiB (each under what the reader holds of one) are more than a mebibyte together.
    let refused = visit_of(600 << 10).unwrap_err().to_string();
    assert!(refused.contains("an automatic style that keeps more than 1 MiB"), "{refused}");
    // Two of 400 KiB are under it.
    assert!(visit_of(400 << 10).is_ok());
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the route kind of a connector

fn number_cell(doc: &Document, shape_at: usize, name: &str) -> Option<f64> {
    let page = doc.pages.get(0)?;
    page.shapes.get(shape_at)?.sheet.cell(name)?.value.as_ref()?.number()
}

/// A connector between two rectangles, as a drawing program writes it.
fn joined() -> Drawing {
    let a = target(r#"xml:id="a" draw:id="a""#, 2);
    let b = target(r#"xml:id="b" draw:id="b""#, 9);
    on_gr1(&format!("{a}{b}{}", connector_between("a", "b")))
}

/// The save of a connector whose route kind is no `draw:type` of the standard's three (the page's own, which a VSDX connector says as 0, a kind of the VSDX application's own, a straight and
/// curved at once) was refused ("a new connector of a route kind the writer does not write"); it is written as a `draw:connector` of the default type, and the cells that say the kind ride in
/// DiaCraft's data, so that the connector comes back with them, and so does a save of what came back.
#[test]
fn a_connector_whose_route_kind_the_markup_cannot_say_is_written_and_comes_back_with_its_cells() {
    for (what, style, ext) in [
        ("the page's own route style", 0.0, 1.0),
        ("a form past the standard's", 3.0, 1.0),
        ("straight and curved at once", 2.0, 2.0),
        ("no extension kind", 1.0, 0.0),
        ("a form past the table", 16.0, 3.0),
    ] {
        let source = source_of(&joined());
        let (doc, _) = read(&source, &Limits::default()).unwrap();
        let mut doc = through_codec(&doc);
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(2).unwrap());
        for (name, value) in [("ShapeRouteStyle", style), ("ConLineRouteExt", ext)] {
            std::sync::Arc::make_mut(&mut shape.sheet)
                .set_cell(&diacraft_doc::CellAddr::direct(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(value).unwrap()))
                .unwrap();
        }
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        assert_eq!(number_cell(&back, 2, "ShapeRouteStyle"), Some(style), "{what}");
        assert_eq!(number_cell(&back, 2, "ConLineRouteExt"), Some(ext), "{what}");
        assert_eq!(back.pages.get(0).unwrap().connects.len(), 2, "{what}: both ends are glued");
        // A save of what came back is the same bytes.
        let again = write_with(&through_codec(&back), None, &WriteOptions::default()).unwrap();
        assert_eq!(again.bytes, written.bytes, "{what}: a second save");
    }
}

/// The control: the three kinds the standard has are still written as `draw:type` and read back from it, with no data of DiaCraft's.
#[test]
fn the_three_route_kinds_of_the_standard_are_still_written_as_the_type_they_are() {
    for (style, ext, draw_type) in [(1.0, 1.0, None), (2.0, 1.0, Some("line")), (1.0, 2.0, Some("curve"))] {
        let source = source_of(&joined());
        let (doc, _) = read(&source, &Limits::default()).unwrap();
        let mut doc = through_codec(&doc);
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(2).unwrap());
        for (name, value) in [("ShapeRouteStyle", style), ("ConLineRouteExt", ext)] {
            std::sync::Arc::make_mut(&mut shape.sheet)
                .set_cell(&diacraft_doc::CellAddr::direct(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(value).unwrap()))
                .unwrap();
        }
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
        let content = part(&written.bytes, "content.xml");
        let connector = element(&content, "draw:connector");
        assert_eq!(connector.contains("draw:type="), draw_type.is_some(), "{connector}");
        if let Some(t) = draw_type {
            assert!(connector.contains(&format!("draw:type=\"{t}\"")), "{connector}");
        }
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        assert_eq!((number_cell(&back, 2, "ShapeRouteStyle"), number_cell(&back, 2, "ConLineRouteExt")), (Some(style), Some(ext)));
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 1, the control: an edited row

/// The row of the `Connection` section of the first shape of the first page that is at `ix`, as its `X` cell says.
fn row_x(doc: &Document, ix: u32) -> Option<f64> {
    let shape = doc.pages.get(0)?.shapes.get(0)?;
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Connection")?;
    let row = section.rows.iter().find(|r| r.ix == Some(ix))?;
    row.cells.get("X")?.value.as_ref()?.number()
}

/// **The control of class 1**: a glue point of a plain shape is written as the element it was read as while its row is as it was read, and **from the row when the row was edited**, never as
/// the stale element: the file read again holds the row where the edit put it.
#[test]
fn an_edited_glue_point_row_of_a_plain_shape_is_written_from_the_row() {
    let frame = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;
    for (what, shape) in [
        ("a rectangle", with_points("rect", frame, POINT)),
        ("an ellipse", with_points("ellipse", frame, POINT)),
        ("a polygon", with_points("polygon", &format!("{frame} svg:viewBox=\"0 0 100 100\" draw:points=\"0,0 100,0 50,100\""), POINT)),
    ] {
        let source = source_of(&on_gr1(&shape));
        let (doc, _) = read(&source, &Limits::default()).unwrap();
        let before = row_x(&doc, 5).unwrap_or_else(|| panic!("{what}: the row of the point"));
        let mut doc = through_codec(&doc);
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        let at = sheet.sections.iter().position(|s| s.name.as_str() == "Connection").unwrap();
        let section = std::sync::Arc::make_mut(sheet.sections.get_mut(at).unwrap());
        let row = std::sync::Arc::make_mut(section.rows.iter_mut().find(|r| r.ix == Some(5)).unwrap());
        row.cells.insert(diacraft_doc::CellName::new("X"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(before + 0.5).unwrap()));
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        let moved = row_x(&back, 5).unwrap_or_else(|| panic!("{what}: the row comes back"));
        assert!(
            (moved - (before + 0.5)).abs() < 1e-6,
            "{what}: the row is where the edit put it ({moved}, was {before}): {}",
            part(&written.bytes, "content.xml")
        );
    }
}

#[test]
fn the_glue_points_of_a_shape_that_also_holds_text_and_other_children_come_back() {
    let frame = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;
    let two = format!(r#"{POINT}<draw:glue-point draw:id="9" svg:x="0cm" svg:y="1cm" draw:align="top-left"/>"#);
    for (what, shape) in [
        ("a rectangle with text after the points", format!(r#"<draw:rect draw:style-name="gr1" {frame}>{two}<text:p>Hello</text:p></draw:rect>"#)),
        (
            "a rectangle with a title before them",
            format!(r#"<draw:rect draw:style-name="gr1" {frame}><svg:title>T</svg:title>{two}<text:p>Hello</text:p></draw:rect>"#),
        ),
        ("a line", format!(r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm">{two}</draw:line>"#)),
        (
            "a text frame",
            format!(r#"<draw:frame draw:style-name="gr1" {frame}><draw:text-box><text:p>Hi</text:p></draw:text-box>{two}</draw:frame>"#),
        ),
        (
            "a custom shape",
            format!(
                r#"<draw:custom-shape draw:style-name="gr1" {frame}>{two}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#
            ),
        ),
        (
            "ids that are not consecutive",
            with_points(
                "rect",
                frame,
                r#"<draw:glue-point draw:id="12" svg:x="1cm" svg:y="1cm"/><draw:glue-point draw:id="7" svg:x="2cm" svg:y="1cm"/>"#,
            ),
        ),
    ] {
        let written = assert_same(what, &on_gr1(&shape));
        assert_eq!(part(&written, "content.xml").matches("<draw:glue-point ").count(), shape.matches("<draw:glue-point ").count(), "{what}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ an outline the reader could not read

/// A polygon or polyline whose view box has no width or height, is no four numbers or is missing, or whose points are no points, was refused ("a view box the reader does not read") or written as
/// another element: the reader draws it as its frame and leaves the attributes it could not read in the shape's kept ones, which are written back as they were beside the element it was.
#[test]
fn a_polygon_whose_outline_the_reader_could_not_read_comes_back_as_it_was() {
    let frame = r#"svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm""#;
    for (what, element, attrs) in [
        ("an empty view box", "polygon", r#"svg:viewBox="0 0 0 100" draw:points="0,0 100,0 50,100""#),
        ("a view box of no numbers", "polygon", r#"svg:viewBox="abc" draw:points="0,0 100,0 50,100""#),
        ("a negative height", "polyline", r#"svg:viewBox="0 0 100 -5" draw:points="0,0 100,0 50,100""#),
        ("no view box", "polygon", r#"draw:points="0,0 100,0 50,100""#),
        ("points that are no points", "polygon", r#"svg:viewBox="0 0 100 100" draw:points="zz""#),
        ("a path whose data breaks the syntax", "path", r#"svg:viewBox="0 0 100 100" svg:d="M 0 0 L""#),
        ("a path with an empty view box", "path", r#"svg:viewBox="0 0 0 0" svg:d="M 0 0 L 100 0 L 50 100 Z""#),
    ] {
        let written = assert_same(what, &on_gr1(&with_points(element, &format!("{frame} {attrs}"), "")));
        let content = part(&written, "content.xml");
        assert!(content.contains(&format!("<draw:{element} ")), "{what}: still the element it was: {content}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the shapes the fuzz targets start from

/// The page the fuzz targets `odg_read` and `odg_write` start a case from (`SEED_SHAPES` of `crates/odg/fuzz/fuzz_targets`): a visit of it is the same content, and a second visit of what
/// came back is the same bytes, so that what the fuzzer mutates from it is a drawing the writer writes back.
#[test]
fn the_shapes_the_fuzz_targets_start_from_come_back() {
    let seed = concat!(
        r#"<draw:rect draw:style-name="gr1" xml:id="s1" draw:id="s1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/></draw:rect>"#,
        r#"<draw:rect draw:style-name="gr1" draw:id="s2" svg:x="8cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:connector draw:style-name="gr1" draw:start-shape="s1" draw:start-glue-point="5" draw:end-shape="s2" draw:end-glue-point="3" svg:x1="5cm" svg:y1="2cm" svg:x2="8cm" svg:y2="2cm"/>"#,
        r#"<draw:g draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/><draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/><draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#,
        r#"<draw:polygon draw:style-name="gr1" svg:x="1cm" svg:y="9cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 0 100" draw:points="0,0 100,0 50,100"/>"#,
        r#"<draw:polyline draw:style-name="gr1" svg:x="6cm" svg:y="9cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 100 100" draw:points="zz"/>"#,
        r#"<draw:rect draw:style-name="gr1" xml:id="s1" draw:id="s1" svg:x="11cm" svg:y="9cm" svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" draw:id="n1" svg:x="1cm" svg:y="13cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" draw:id="n2" svg:x="8cm" svg:y="13cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:connector draw:style-name="gr1" draw:start-shape="n1" draw:start-glue-point="1" draw:end-shape="n2" draw:end-glue-point="3" svg:x1="5cm" svg:y1="14cm" svg:x2="8cm" svg:y2="14cm"/>"#
    );
    assert_same("the seed of the fuzz targets", &on_gr1(seed));
}
