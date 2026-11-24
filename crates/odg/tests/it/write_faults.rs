//! The faults that milestone 1 measured (classes F1 to F5 and the refusals), each at the writer: the model is
//! changed as the engine's commands change it, the bytes are made by `write_with`, and what they say is read by the crate's own reader. The engine's own tests
//! (`crates/engine/tests/it/odg_faults.rs`) run the same vectors through the commands and the file they save; these are here so that the writer is held by its
//! own crate's tests, which a mutation of it is run against.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_doc::{Cell, CellName, Shape, Text, TextPart, Value};
use diacraft_odg::WriteError;

use crate::build::{DRAWING, package};
use crate::drawing::{Drawing, close};
use crate::write_core::{Open, PNG, custom, num_of, object_with_manifest_under, open, open_bytes, part, real, set_num, shape_mut};
use crate::write_more::{CONNECTOR_BODY, set_route_end};
use crate::write_new::{connector, glue, push_shape, rectangle};

/// The package of `d` with every `office:version="1.3"` of its two XML parts made `version`, or taken out when there is none.
pub(crate) fn versioned(d: &Drawing, version: Option<&str>) -> Vec<u8> {
    let swap = |text: String| match version {
        Some(v) => text.replace("office:version=\"1.3\"", &format!("office:version=\"{v}\"")),
        None => text.replace(" office:version=\"1.3\"", ""),
    };
    let mut files = d.files();
    for (name, bytes) in &mut files {
        if name == "content.xml" || name == "styles.xml" {
            *bytes = swap(String::from_utf8(std::mem::take(bytes)).unwrap()).into_bytes();
        }
    }
    package(DRAWING, files)
}

/// The start tag of the first element called `tag` in `content`.
fn start_tag<'a>(content: &'a str, tag: &str) -> &'a str {
    let at = content.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no <{tag} in {content}"));
    &content[at..at + content[at..].find('>').unwrap() + 1]
}

/// Whether the tag has the attribute `name`.
fn has(tag: &str, name: &str) -> bool {
    tag.contains(&format!(" {name}=\""))
}

/// A second save of the model as the save left it is no edit.
#[track_caller]
fn saves_again_as_no_edit(o: &Open, saved: &diacraft_odg::Written) {
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "the same model saved again is no edit");
}

// ---- F1 -------------------------------------------------------------------------------------------------------------------------------------------

/// V1 (F1): a connector made in a part of ODF 1.0 or 1.1, or one that declares no version, is its ends alone: no `svg:d`, no `svg:viewBox` on it,
/// no `xml:id` anywhere, and the shapes it is glued to are named by `draw:id`, which it names; the version is never raised. In ODF 1.2 and 1.3 it is written as it was.
#[test]
fn a_connector_made_in_a_part_of_odf_1_0_or_1_1_or_none_has_no_svg_d_and_no_xml_id() {
    for version in [Some("1.0"), Some("1.1"), None, Some("1.2"), Some("1.3")] {
        let legacy = !matches!(version, Some("1.2" | "1.3"));
        for new_shape in [false, true] {
            let what = format!("version {version:?}, a new shape: {new_shape}");
            let d = Drawing::a4().page(&format!("{}{}", custom("s1", "1cm"), custom("s2", "8cm")));
            let mut o = open_bytes(versioned(&d, version));
            let target = if new_shape {
                push_shape(&mut o.doc, rectangle(3, "R", (14.0, 3.0, 3.0, 2.0)));
                3
            } else {
                2
            };
            push_shape(&mut o.doc, connector(4, (5.0, 25.0, 8.0, 25.0)));
            glue(&mut o.doc, 4, true, 1, 3);
            glue(&mut o.doc, 4, false, target, 1);
            let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            let after = part(&saved.bytes, "content.xml");
            let tag = start_tag(&after, "draw:connector");
            assert_eq!(has(tag, "svg:d") || has(tag, "svg:viewBox"), !legacy, "{what}: the route as a path is 1.2's: {tag}");
            assert_eq!(after.contains(" xml:id=\""), !legacy, "{what}: xml:id is 1.2's: {after}");
            match version {
                Some(v) => assert!(after.contains(&format!("office:version=\"{v}\"")), "{what}: the version is the file's own"),
                None => assert!(!after.contains("office:version"), "{what}: and none stays none"),
            }
            // Glue by the id the shapes carry, which is `draw:id` in a part of 1.0 or 1.1, and `xml:id` with `draw:id` after it.
            assert!(has(tag, "draw:start-shape") && has(tag, "draw:end-shape"), "{what}: {tag}");
            assert_eq!(after.matches(" draw:id=\"id1\"").count(), 1, "{what}: {after}");
            assert_eq!(after.matches(" draw:id=\"id2\"").count(), 1, "{what}: {after}");
            let back = open_bytes(saved.bytes.clone());
            assert_eq!(back.doc.pages.get(0).unwrap().connects.len(), 2, "{what}: glued at both ends, as the model is");
            saves_again_as_no_edit(&o, &saved);
        }
    }
}

/// A new line (a 1-D shape with no route cells, which the writer writes as a `draw:line`) that a connector is glued to is named as a new shape is: by `draw:id` alone in a
/// part of ODF 1.0 or 1.1, and by `xml:id` and `draw:id` after it in ODF 1.3.
#[test]
fn a_new_line_a_connector_is_glued_to_is_named_by_draw_id_alone_in_a_part_of_odf_1_1() {
    for (version, legacy) in [("1.1", true), ("1.3", false)] {
        let mut o = open_bytes(versioned(&Drawing::a4().page(&custom("s1", "1cm")), Some(version)));
        let mut line = connector(3, (3.0, 20.0, 9.0, 20.0));
        {
            let sheet = Arc::make_mut(&mut line.sheet);
            sheet.cells.remove("ShapeRouteStyle");
            sheet.cells.remove("ConLineRouteExt");
            sheet.sections.retain(|s| s.name.as_str() == "Geometry");
        }
        push_shape(&mut o.doc, line);
        push_shape(&mut o.doc, connector(4, (5.0, 25.0, 8.0, 25.0)));
        glue(&mut o.doc, 4, true, 1, 3);
        glue(&mut o.doc, 4, false, 3, 1);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{version}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        let line_tag = start_tag(&after, "draw:line");
        assert!(has(line_tag, "draw:id"), "{version}: the connector names the line: {line_tag}");
        assert_eq!(has(line_tag, "xml:id"), !legacy, "{version}: {line_tag}");
        assert_eq!(after.contains(" xml:id=\""), !legacy, "{version}: {after}");
        assert_eq!(open_bytes(saved.bytes.clone()).doc.pages.get(0).unwrap().connects.len(), 2, "{version}");
        saves_again_as_no_edit(&o, &saved);
    }
}

/// V2 (F1): a connector glued to a shape that moved is routed again by the model; in a part of ODF 1.0 or 1.1 its ends are written and no `svg:d` is added, and one
/// the file holds (which that version's schema does not have) goes with the `svg:viewBox` beside it, since the ends no longer say that route. In ODF 1.2 `svg:d`
/// is written as before.
#[test]
fn a_connector_routed_again_in_a_part_of_odf_1_0_or_1_1_gets_no_svg_d_and_loses_one_it_had() {
    let bare =
        CONNECTOR_BODY.replacen("xml:id=\"a\"", "draw:id=\"a\"", 1).replacen(" svg:d=\"M 6000 4000 h 4000\" svg:viewBox=\"0 0 4001 1\"", "", 1);
    assert!(!bare.contains("svg:d"));
    for (version, body) in [("1.0", bare.as_str()), ("1.1", bare.as_str()), ("1.1", CONNECTOR_BODY), ("1.0", CONNECTOR_BODY), ("1.2", CONNECTOR_BODY)]
    {
        let what = format!("version {version}, {}", if body.contains("svg:d") { "an svg:d in the file" } else { "none in the file" });
        let mut o = open_bytes(versioned(&Drawing::a4().page(body), Some(version)));
        crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        {
            let c = shape_mut(&mut o.doc, 0, 2);
            let x = num_of(c, "BeginX");
            set_num(c, "BeginX", x + 1.0 / 2.54);
            set_route_end(c, 3.0 / 2.54);
        }
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        let tag = start_tag(&after, "draw:connector");
        assert!(tag.contains("svg:x1=\"7cm\" svg:y1=\"4cm\" svg:x2=\"10cm\" svg:y2=\"4cm\""), "{what}: the ends follow the shape: {tag}");
        if version == "1.2" {
            assert!(tag.contains("svg:d=\"M 7000 4000 h 3000\" svg:viewBox=\"0 0 3001 1\""), "{what}: {tag}");
        } else {
            assert!(!has(tag, "svg:d") && !has(tag, "svg:viewBox"), "{what}: {tag}");
            assert!(tag.contains("draw:start-shape=\"a\" draw:start-glue-point=\"1\""), "{what}: the glue stays: {tag}");
        }
        // A route the part has no place for is said, and one it has is written and not.
        if version == "1.2" {
            assert!(saved.notes.is_empty(), "{what}: {:?}", saved.notes);
        } else {
            assert_eq!(saved.notes, vec![diacraft_odg::SaveNote::RouteDroppedForVersion { page: 0, shape: 2 }], "{what}");
        }
        assert!(after.contains(&format!("office:version=\"{version}\"")), "{what}");
        let back = open_bytes(saved.bytes.clone());
        close(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "BeginX"), 7.0 / 2.54);
        saves_again_as_no_edit(&o, &saved);
    }
}

// ---- F3 -------------------------------------------------------------------------------------------------------------------------------------------

fn text_of(s: &str) -> Option<Arc<Text>> {
    Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(s))], ..Default::default() }))
}

/// V5 (F3): a text set on a picture frame or an object frame is refused in words, whatever the frame holds beside the picture (an empty paragraph,
/// none), since no spec says where in the file the text of a graphic frame goes and the reader reads none from there. A text frame is written, in its `draw:text-box`.
#[test]
fn a_text_set_on_a_picture_or_an_object_frame_is_refused_in_words_and_on_a_text_frame_it_is_written() {
    let frames = [
        r#"<draw:frame draw:name="P" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="3cm"><draw:image xlink:href="Pictures/p.png"><text:p/></draw:image></draw:frame>"#,
        r#"<draw:frame draw:name="P" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="3cm"><draw:image xlink:href="Pictures/p.png"/></draw:frame>"#,
        r#"<draw:frame draw:name="P" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="3cm"><draw:object xlink:href="./Object 1"><text:p/></draw:object><draw:image xlink:href="Pictures/p.png"/></draw:frame>"#,
    ];
    for frame in frames {
        let mut d = Drawing::a4().page(frame);
        d.files.push(("Pictures/p.png".to_owned(), PNG.to_vec()));
        d.files.push(("Object 1/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
        let mut o = open(&d);
        assert_eq!(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().kind, diacraft_doc::ShapeType::Foreign, "{frame}");
        shape_mut(&mut o.doc, 0, 1).text = text_of("Ab");
        match real(&o.doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(words)) => assert!(words.contains("a text on a picture or an object frame"), "{words}"),
            other => panic!("{frame}: expected a refusal: {other:?}"),
        }
        // The frame's text taken back (the undo) saves.
        shape_mut(&mut o.doc, 0, 1).text = None;
        assert!(!real(&o.doc, &o.bytes, &o.index).unwrap().edited);
    }
    let frame = r#"<draw:frame draw:name="T" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="3cm"><draw:text-box><text:p>old</text:p></draw:text-box></draw:frame>"#;
    let mut o = open(&Drawing::a4().page(frame));
    shape_mut(&mut o.doc, 0, 1).text = text_of("new");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(part(&saved.bytes, "content.xml").contains("<draw:text-box><text:p>new</text:p></draw:text-box>"));
    saves_again_as_no_edit(&o, &saved);
}

// ---- F4 -------------------------------------------------------------------------------------------------------------------------------------------

/// A drawing whose shape `G` is on the automatic style `gr3` with `fill` as its fill, and whose `office:styles` has the gradient and the hatch the styles name.
fn filled(fill: &str) -> Drawing {
    let definitions = r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/><draw:hatch draw:name="h1" draw:style="single" draw:color="#3465a4" draw:distance="0.1cm" draw:rotation="0"/>"##;
    Drawing::a4()
        .common(definitions)
        .auto(&format!(
            r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties {fill} draw:stroke="solid"/></style:style>"##
        ))
        .page(r#"<draw:rect draw:name="G" draw:style-name="gr3" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p/></draw:rect>"#)
}

fn put(shape: &mut Shape, cell: &str, text: &str) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

fn style_gr3(content: &str) -> String {
    let at = content.find("<style:style style:name=\"gr3\"").unwrap_or_else(|| panic!("no gr3: {content}"));
    content[at..at + content[at..].find("</style:style>").unwrap() + 14].to_owned()
}

/// V4 (F4): a colour set, and a gradient turned off, on a shape whose fill is a gradient, a hatch or a bitmap writes `draw:fill="solid"` and takes the
/// names the new kind does not use out of the style; one set on a shape whose styles state no fill mode states `solid`; a
/// gradient that is still on keeps its mode; and a style that states the pattern already is not changed in it.
#[test]
fn a_colour_set_on_a_gradient_hatch_bitmap_or_no_fill_mode_states_the_fill_the_pattern_says() {
    let gradient = r##"draw:fill="gradient" draw:fill-gradient-name="g1" draw:fill-color="#729fcf""##;
    type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case<'_>; 7] = [
        ("a gradient", gradient, &[("FillGradientEnabled", "0")], "solid"),
        (
            "a hatch that is solid",
            r##"draw:fill="hatch" draw:fill-hatch-name="h1" draw:fill-hatch-solid="true" draw:fill-color="#729fcf""##,
            &[],
            "solid",
        ),
        ("a hatch that is not", r##"draw:fill="hatch" draw:fill-hatch-name="h1" draw:fill-color="#729fcf""##, &[("FillPattern", "1")], "solid"),
        ("a bitmap", r##"draw:fill="bitmap" draw:fill-image-name="i1" draw:fill-color="#729fcf""##, &[("FillPattern", "1")], "solid"),
        ("no fill mode", r##"draw:fill-color="#729fcf""##, &[], "solid"),
        ("a style that says solid", r##"draw:fill="solid" draw:fill-color="#729fcf""##, &[], "solid"),
        ("a style that says none", r##"draw:fill="none""##, &[], "none"),
    ];
    for (case, fill, also, kind) in cases {
        let mut o = open(&filled(fill));
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        put(shape_mut(&mut o.doc, 0, id), "FillForegnd", "#123456");
        for (cell, value) in also {
            put(shape_mut(&mut o.doc, 0, id), cell, value);
        }
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{case}: {e}"));
        let style = style_gr3(&part(&saved.bytes, "content.xml"));
        assert!(style.contains(&format!("draw:fill=\"{kind}\"")) && style.contains("draw:fill-color=\"#123456\""), "{case}: {style}");
        for gone in ["draw:fill-gradient-name", "draw:fill-hatch-name", "draw:fill-hatch-solid", "draw:fill-image-name"] {
            assert!(!style.contains(gone), "{case}: {gone} goes: {style}");
        }
        assert_eq!(style.matches("draw:fill=").count(), 1, "{case}: stated once: {style}");
        saves_again_as_no_edit(&o, &saved);
    }
    // A gradient still on, with a colour: the mode stays what the file has it, and nothing is claimed of the fill.
    let mut o = open(&filled(gradient));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    put(shape_mut(&mut o.doc, 0, id), "FillForegnd", "#123456");
    let style = style_gr3(&part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml"));
    assert!(style.contains("draw:fill=\"gradient\"") && style.contains("draw:fill-gradient-name=\"g1\"") && style.contains("#123456"), "{style}");
    // A colour that is set on a shape whose own style says solid, with `solid` already in it, adds nothing but the colour.
    let o = open(&filled(r##"draw:fill="solid" draw:fill-color="#729fcf""##));
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "FillForegnd", "#123456");
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    assert_eq!(style_gr3(&part(&saved.bytes, "content.xml")), style_gr3(&part(&o.bytes, "content.xml")).replace("#729fcf", "#123456"));
}

/// The fill pattern of a shape on a common style, which holds no cell of its own, is found through the style sheets: a style that states none hands it to its parent,
/// and a chain that states none ends at the root, which the model draws solid. A chain that gives 0 (`none`) states nothing: the colour is written alone and the
/// fill stays the one the parent states.
#[test]
fn a_colour_set_on_a_shape_on_a_common_style_states_the_fill_its_chain_gives() {
    let common = r#"<style:style style:name="Base" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style><style:style style:name="Child" style:family="graphic" style:parent-style-name="Base"/><style:style style:name="GrandChild" style:family="graphic" style:parent-style-name="Child"/><style:style style:name="Bare" style:family="graphic"/>"#;
    for (style, kind) in [("Base", None), ("Child", None), ("GrandChild", None), ("Bare", Some("solid"))] {
        let shape = format!(
            r#"<draw:rect draw:name="G" draw:style-name="{style}" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p/></draw:rect>"#
        );
        let mut o = open(&Drawing::a4().common(common).page(&shape));
        put(shape_mut(&mut o.doc, 0, 1), "FillForegnd", "#123456");
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{style}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        let fill = kind.map_or(String::new(), |k| format!(" draw:fill=\"{k}\""));
        assert!(
            after.contains(&format!("style:parent-style-name=\"{style}\"><style:graphic-properties draw:fill-color=\"#123456\"{fill}/>")),
            "{style}: {after}"
        );
        saves_again_as_no_edit(&o, &saved);
    }
}

/// A fill cell written on its own (not through `shape.format`, which sets the pattern and the gradient with the colour): a colour on a bitmap, or on a hatch that is not
/// solid (the reader reads both as pattern 0), writes only the colour and keeps the file's fill and its image or hatch name, and nothing else changes.
#[test]
fn a_colour_written_alone_on_a_bitmap_or_a_hatch_that_is_not_solid_keeps_the_fill_the_file_states() {
    for (case, fill, name) in [
        ("a bitmap", r##"draw:fill="bitmap" draw:fill-image-name="i1" draw:fill-color="#729fcf""##, "draw:fill-image-name=\"i1\""),
        ("a hatch", r##"draw:fill="hatch" draw:fill-hatch-name="h1" draw:fill-color="#729fcf""##, "draw:fill-hatch-name=\"h1\""),
    ] {
        let o = open(&filled(fill));
        assert_eq!(num_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "FillPattern"), 0.0, "{case}: the reader reads pattern 0");
        let mut doc = o.doc.clone();
        put(shape_mut(&mut doc, 0, 1), "FillForegnd", "#123456");
        let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{case}: {e}"));
        let style = style_gr3(&part(&saved.bytes, "content.xml"));
        assert_eq!(style, style_gr3(&part(&o.bytes, "content.xml")).replace("#729fcf", "#123456"), "{case}: only the colour changes");
        assert!(style.contains(name) && !style.contains("draw:fill=\"none\"") && !style.contains("draw:fill=\"solid\""), "{case}: {style}");
    }
}

/// A colour written alone on a shape whose gradient is its common style's, which the shape's own sheet holds no cell of: the gradient is found through the style
/// sheets as the pattern is, so the colour is not the fill and nothing is stated of it. A gradient the shape's own sheet turns off, over the same common style, is.
#[test]
fn a_colour_written_alone_on_a_gradient_a_common_style_gives_does_not_state_solid() {
    let definitions = r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/>"##;
    let common = format!(
        r##"{definitions}<style:style style:name="GradBase" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="g1"/></style:style>"##
    );
    let drawing = Drawing::a4()
        .common(&common)
        .auto(r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="GradBase"><style:graphic-properties draw:stroke="solid"/></style:style>"#)
        .page(r#"<draw:rect draw:name="G" draw:style-name="gr3" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p/></draw:rect>"#);
    let o = open(&drawing);
    let shape = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert!(shape.sheet.cell("FillGradientEnabled").is_none(), "the shape's own sheet holds no gradient cell");
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "FillForegnd", "#123456");
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    let style = style_gr3(&part(&saved.bytes, "content.xml"));
    assert!(style.contains("draw:fill-color=\"#123456\"") && !style.contains("draw:fill=\""), "the colour alone is written: {style}");
    // The gradient turned off on the shape's own sheet, whose pattern its common style gives as 1: solid, and the colour.
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "FillGradientEnabled", "0");
    put(shape_mut(&mut doc, 0, 1), "FillForegnd", "#123456");
    let saved = real(&doc, &o.bytes, &o.index).unwrap();
    let style = style_gr3(&part(&saved.bytes, "content.xml"));
    assert!(style.contains("draw:fill=\"solid\"") && style.contains("draw:fill-color=\"#123456\""), "{style}");
}

// ---- F5 -------------------------------------------------------------------------------------------------------------------------------------------

/// A custom shape `F` of 4 by 2 centimetres turned 90 degrees, with the mirrors `h` and `v` of its geometry and the transform `transform`.
fn mirrored(h: bool, v: bool, transform: &str) -> Drawing {
    Drawing::a4().page(&format!(
        r#"<draw:custom-shape draw:name="F" svg:width="4cm" svg:height="2cm" draw:transform="{transform}"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:mirror-horizontal="{h}" draw:mirror-vertical="{v}" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    ))
}

const QUARTER: &str = "rotate (1.5707963267948966) translate (8cm 6cm)";

fn flips(shape: &Shape) -> (f64, f64) {
    (num_of(shape, "FlipX"), num_of(shape, "FlipY"))
}

/// F5 and V6: a turn of a shape, with each of the four combinations of flips and four angles, is written as the turn of the frame and not of the frame and its flips:
/// the file read again has the angle asked, the same flips (the mirrors, which stay), and the same place; a shape flipped both ways was read half a turn off.
#[test]
fn a_turn_of_a_shape_with_any_flips_reads_back_at_the_angle_asked_with_the_flips_it_had() {
    for (h, v) in [(false, false), (true, false), (false, true), (true, true)] {
        for degrees in [30.0f64, 100.0, 250.0, -60.0, 0.0] {
            let what = format!("mirrors {h} and {v}, turned to {degrees}");
            let mut o = open(&mirrored(h, v, QUARTER));
            let before = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
            assert_eq!(flips(&before), (f64::from(u8::from(h)), f64::from(u8::from(v))), "{what}");
            set_num(shape_mut(&mut o.doc, 0, 1), "Angle", degrees.to_radians());
            let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            let after = part(&saved.bytes, "content.xml");
            assert!(after.contains(&format!("draw:mirror-horizontal=\"{h}\" draw:mirror-vertical=\"{v}\"")), "{what}: the mirrors stay: {after}");
            let back = open_bytes(saved.bytes.clone());
            let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
            let angle = num_of(&read, "Angle");
            let asked = degrees.to_radians();
            assert!((angle - asked).sin().abs() < 1e-9 && (angle - asked).cos() > 0.0, "{what}: the file reads {} degrees", angle.to_degrees());
            assert_eq!(flips(&read), flips(&before), "{what}");
            for cell in ["PinX", "PinY", "Width", "Height"] {
                close(num_of(&read, cell), num_of(&before, cell));
            }
            saves_again_as_no_edit(&o, &saved);
        }
    }
}

/// A turn that is written with the flips the file holds as they were is the only kind of change of a flipped frame the save writes: a change of the flips is not, and nor
/// is a turn of a shape whose `draw:transform` holds the flip itself (a negative scale), which a frame written whole would lose.
#[test]
fn a_flip_changed_or_one_the_transform_holds_is_refused_in_words_and_never_written_wrong() {
    let refused = |doc: &Document, o: &Open, what: &str| match real(doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("flip"), "{what}: {words}"),
        other => panic!("{what}: expected a refusal: {other:?}"),
    };
    let o = open(&mirrored(true, false, QUARTER));
    // Both flips set, or one taken away or added, with the turn left: the flips are not written.
    for (cell, value) in [("FlipX", "0"), ("FlipY", "1")] {
        let mut doc = o.doc.clone();
        put(shape_mut(&mut doc, 0, 1), cell, value);
        refused(&doc, &o, &format!("{cell} {value}"));
    }
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "FlipY", "1");
    put(shape_mut(&mut doc, 0, 1), "Angle", "0.3");
    refused(&doc, &o, "a flip and a turn");
    // Both flips changed at once (two `shape.flip` calls): the frame's linear part is then a turn of half a turn, which the turn of the frame alone does not refuse, so
    // only the check of the flips does. Written, the file would read back as the other angle with the old mirrors, and the model holds its own angle with the new flips.
    for (what, h, v, to) in [("both mirrors taken away", true, true, "0"), ("both flips put on an unmirrored shape", false, false, "1")] {
        let o = open(&mirrored(h, v, QUARTER));
        let mut doc = o.doc.clone();
        put(shape_mut(&mut doc, 0, 1), "FlipX", to);
        put(shape_mut(&mut doc, 0, 1), "FlipY", to);
        refused(&doc, &o, what);
        // And the flips put back, with nothing else changed, is no edit.
        let mut back = doc.clone();
        let (x, y) = (
            num_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "FlipX"),
            num_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "FlipY"),
        );
        put(shape_mut(&mut back, 0, 1), "FlipX", &x.to_string());
        put(shape_mut(&mut back, 0, 1), "FlipY", &y.to_string());
        assert!(!real(&back, &o.bytes, &o.index).unwrap().edited, "{what}: the flips as they were");
    }
    // A negative scale in the transform reads as `FlipY`; a turn would drop it.
    let o = open(&mirrored(false, false, "scale (1 -1) translate (8cm 6cm)"));
    assert_eq!(flips(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap()).1, 1.0);
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "Angle", "0.5");
    refused(&doc, &o, "a negative scale and a turn");
    // And the same with a mirror beside it, which cancels the flip in the cells.
    let o = open(&mirrored(false, true, "scale (1 -1) translate (8cm 6cm)"));
    let mut doc = o.doc.clone();
    put(shape_mut(&mut doc, 0, 1), "Angle", "0.5");
    refused(&doc, &o, "a negative scale, a mirror and a turn");
}

// ---- V7 -------------------------------------------------------------------------------------------------------------------------------------------

/// A drawing of one page that holds no shape, written as `form` says (an empty element, as LibreOffice writes it; a start tag and an end tag; white space).
fn empty_page(form: &str, second_page: bool) -> Vec<u8> {
    let mut d = Drawing::a4().page("");
    if second_page {
        d = d.page(&custom("s1", "1cm"));
    }
    let content = d.content_xml();
    let content = match form {
        "empty element" => content.replacen("></draw:page>", "/>", 1),
        "white space" => content.replacen("></draw:page>", ">\n  </draw:page>", 1),
        _ => content,
    };
    crate::build::package(DRAWING, crate::build::replace(d.files(), "content.xml", content.into_bytes()))
}

/// A rectangle, a connector and both on a page with no shape: the page that is an empty element is opened and closed round what is put in it, in the order it was put,
/// and the part is one the reader reads and the save's own check accepts.
#[test]
fn a_shape_put_on_a_page_with_no_shape_is_saved_whatever_form_the_page_is_written_in() {
    for form in ["empty element", "start and end tag", "white space"] {
        for second_page in [false, true] {
            for added in 1..=3 {
                let what = format!("a page that is {form}, {added} shapes put on it, a second page: {second_page}");
                let mut o = open_bytes(empty_page(form, second_page));
                push_shape(&mut o.doc, rectangle(10, "R1", (2.0, 3.0, 4.0, 2.0)));
                if added >= 2 {
                    push_shape(&mut o.doc, connector(11, (1.0, 1.0, 3.0, 1.5)));
                }
                if added >= 3 {
                    push_shape(&mut o.doc, rectangle(12, "R2", (8.0, 3.0, 4.0, 2.0)));
                }
                let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
                let after = part(&saved.bytes, "content.xml");
                assert_eq!(after.matches("<draw:page ").count(), if second_page { 2 } else { 1 }, "{what}: {after}");
                assert!(
                    after.contains("draw:master-page-name=\"Default\"><draw:custom-shape"),
                    "{what}: the page is opened and the shape put in it: {after}"
                );
                let back = open_bytes(saved.bytes.clone());
                assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), added, "{what}");
                let names: Vec<String> = back.doc.pages.get(0).unwrap().shapes.iter().filter_map(|s| s.name.as_deref().map(str::to_owned)).collect();
                assert_eq!(names.first().map(String::as_str), Some("R1"), "{what}: in the order they were put in: {names:?}");
                if second_page {
                    assert_eq!(back.doc.pages.get(1).unwrap().shapes.len(), 1, "{what}: the other page is as it was");
                }
                saves_again_as_no_edit(&o, &saved);
            }
        }
    }
}

// ---- V8 -------------------------------------------------------------------------------------------------------------------------------------------

/// A drawing of the shape `s1` whose `content.xml` has no `office:automatic-styles` element.
fn no_automatic_styles() -> (Drawing, String) {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let content = d.content_xml().replace("<office:automatic-styles></office:automatic-styles>", "");
    assert!(!content.contains("automatic-styles"));
    (d, content)
}

fn opened_without_styles(content: &str, d: &Drawing) -> Open {
    open_bytes(package(DRAWING, crate::build::replace(d.files(), "content.xml", content.as_bytes().to_vec())))
}

/// A restyle and a new shape (and both, and a connector) in a part with no `office:automatic-styles` make the element, empty, before `office:body` and after
/// `office:font-face-decls` (where the schema puts it), with the new styles in it in the order they were made, closed after the last; a save that puts no style in
/// makes no element; and the element is written under the prefix the body has. A body that declares a namespace of its own is refused in words.
#[test]
fn a_part_with_no_automatic_styles_gets_the_element_before_the_body_when_a_style_is_put_in_it() {
    let (d, content) = no_automatic_styles();
    for what in ["a restyle", "a new shape", "both", "a connector"] {
        let mut o = opened_without_styles(&content, &d);
        if what == "a restyle" || what == "both" {
            put(shape_mut(&mut o.doc, 0, 1), "FillForegnd", "#336699");
        }
        if what == "a new shape" || what == "both" {
            push_shape(&mut o.doc, rectangle(3, "R", (8.0, 12.0, 4.0, 2.0)));
        }
        if what == "a connector" {
            push_shape(&mut o.doc, connector(3, (1.0, 1.0, 3.0, 1.0)));
        }
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert_eq!(after.matches("<office:automatic-styles>").count(), 1, "{what}: {after}");
        let (faces, auto, close_auto, body) = (
            after.find("</office:font-face-decls>").unwrap(),
            after.find("<office:automatic-styles>").unwrap(),
            after.find("</office:automatic-styles>").unwrap(),
            after.find("<office:body").unwrap(),
        );
        assert!(faces < auto && auto < close_auto && close_auto < body, "{what}: after the font declarations, before the body: {after}");
        assert!(after[auto..close_auto].contains("<style:style style:name=\"gr1\" style:family=\"graphic\""), "{what}: {after}");
        if what == "a new shape" || what == "both" || what == "a connector" {
            assert!(
                after[auto..close_auto].contains("style:name=\"P1\" style:family=\"paragraph\""),
                "{what}: the paragraph style after the graphic style: {after}"
            );
        }
        assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "{what}: styles.xml is as it was");
        let back = open_bytes(saved.bytes.clone());
        assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), o.doc.pages.get(0).unwrap().shapes.len(), "{what}");
        saves_again_as_no_edit(&o, &saved);
    }
    // No style put in: no element made.
    let mut o = opened_without_styles(&content, &d);
    move_first(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited && !part(&saved.bytes, "content.xml").contains("automatic-styles"));
    // The element is written under the prefixes the part uses.
    let renamed = content
        .replace("<office:", "<o:")
        .replace("</office:", "</o:")
        .replace(" office:version=", " o:version=")
        .replace(" xmlns:office=", " xmlns:o=")
        .replace("<style:", "<st:")
        .replace(" xmlns:style=", " xmlns:st=");
    let mut o = opened_without_styles(&renamed, &d);
    put(shape_mut(&mut o.doc, 0, 1), "FillForegnd", "#336699");
    let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("<o:automatic-styles><st:style ") && after.contains("</st:style></o:automatic-styles><o:body>"), "{after}");
    // A body that declares a prefix: the element, which is not inside it, cannot use that prefix, so the save is refused in words.
    let declaring = content.replacen("<office:body>", "<office:body xmlns:x=\"urn:x\">", 1);
    let mut o = opened_without_styles(&declaring, &d);
    put(shape_mut(&mut o.doc, 0, 1), "FillForegnd", "#336699");
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("no office:automatic-styles") && words.contains("declares"), "{words}"),
        other => panic!("expected a refusal: {other:?}"),
    }
    // The same body and an edit that puts no style in: nothing is made and nothing is refused. A move asks the scan for no styles; a page made taller does (its
    // master page and layout are edited), and puts none in `content.xml`.
    let mut o = opened_without_styles(&declaring, &d);
    move_first(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited && !part(&saved.bytes, "content.xml").contains("automatic-styles"));
    let mut o = opened_without_styles(&declaring, &d);
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let cell = Arc::make_mut(&mut page.sheet).cells.get_mut("PageHeight").unwrap();
        let height = cell.value.as_ref().and_then(Value::number).unwrap();
        cell.value = Value::from_number(height + 1.0);
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(
        saved.edited
            && !part(&saved.bytes, "content.xml").contains("automatic-styles")
            && part(&saved.bytes, "styles.xml") != part(&o.bytes, "styles.xml")
    );
}

fn move_first(o: &mut Open) {
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
}

// ---- V9 -------------------------------------------------------------------------------------------------------------------------------------------

const OOO_DOCTYPE: &str = "<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">";

/// Deleting a frame that holds an object, and a save that adds a row, in a package whose manifest has the OpenOffice.org document type declaration, which the reader
/// reads and ignores: the save reads its manifest as the reader does, takes the object's rows out, adds the thumbnail's, and leaves the declaration as it was.
#[test]
fn a_manifest_with_the_open_office_document_type_declaration_is_edited_with_the_declaration_kept() {
    let bytes = object_with_manifest_under(OOO_DOCTYPE);
    assert!(part(&bytes, "META-INF/manifest.xml").contains(OOO_DOCTYPE));
    let mut o = open_bytes(bytes);
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let manifest = part(&saved.bytes, "META-INF/manifest.xml");
    assert!(manifest.contains(OOO_DOCTYPE), "the declaration stays: {manifest}");
    assert!(
        !manifest.contains("Object 1") && manifest.contains("Object 2/content.xml") && manifest.contains("Thumbnails/thumbnail.png"),
        "{manifest}"
    );
    assert!(crate::write_core::has_part(&saved.bytes, "Object 2/content.xml") && !crate::write_core::has_part(&saved.bytes, "Object 1/content.xml"));
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 1);
    saves_again_as_no_edit(&o, &saved);
    // A move alone adds the thumbnail's row, which the same scan makes.
    let mut o = open_bytes(object_with_manifest_under(OOO_DOCTYPE));
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let manifest = part(&saved.bytes, "META-INF/manifest.xml");
    assert!(manifest.contains(OOO_DOCTYPE) && manifest.contains("Thumbnails/thumbnail.png"), "{manifest}");
}
