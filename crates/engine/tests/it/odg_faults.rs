//! The faults that milestone 1 measured in the `.odg` save (failure classes F1 to F5 and the refusals
//!), one test for each of its vectors, run through the engine's own commands and the file they save. Every drawing is built by hand from the standard
//! (`odg_build.rs`); what is read back is read by the `odg` crate's reader.
//!
//! All eight are faults and block v0.1. **F1** keep to the declared version (a document of ODF 1.0 or 1.1 gets no `svg:d` on a
//! connector and no `xml:id`, and `office:version` is never raised); **F2** an edit routes again only the connectors glued to the shapes it changed;
//! **F3** the text of a picture or object frame is refused in words where no spec says where it goes; **F4** the model's fill pattern decides
//! `draw:fill`; **F5** a turn of a shape flipped both ways; **V7** a page with no shape; **V8** a `content.xml` with no `office:automatic-styles`;
//! **V9** a manifest with the OpenOffice.org document type declaration.

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing, content_of, id_of, open_bytes_in, open_drawing, package, part_bytes, part_of, read_back, rect};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, edit_cases, four, refused, save, shift};
use crate::{check_session, run};

// ---- helpers ------------------------------------------------------------------------------------------------------------------------------------

/// The start tag of the element at the start of `text`: from its `<` through its `>`.
fn start_tag(text: &str) -> &str {
    text.get(..text.find('>').map_or(text.len(), |at| at + 1)).unwrap_or_default()
}

/// Every element called `tag` in `content`, each from its `<` to its end tag.
fn elements(content: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = content[from..].find(&format!("<{tag} ")).map(|a| a + from) {
        let close = format!("</{tag}>");
        let end = content[at..].find(&close).map_or(content.len(), |e| at + e + close.len());
        out.push(content[at..end].to_owned());
        from = end;
    }
    out
}

/// The value of the attribute `name` on the start tag of the element at the start of `text`.
fn attribute(text: &str, name: &str) -> Option<String> {
    let tag = start_tag(text);
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(tag[at..].split('"').next()?.to_owned())
}

/// The drawing's parts with every `office:version="1.3"` made `version`, as a producer of that version writes them; with `""` the parts declare none.
fn declared(drawing: &Drawing, version: &str) -> Vec<u8> {
    let swap = |text: String| {
        if version.is_empty() {
            text.replace(" office:version=\"1.3\"", "")
        } else {
            text.replace("office:version=\"1.3\"", &format!("office:version=\"{version}\""))
        }
    };
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), swap(drawing.content_xml()).into_bytes()),
            ("styles.xml".to_owned(), swap(drawing.styles_xml()).into_bytes()),
            ("meta.xml".to_owned(), swap(drawing.meta_xml()).into_bytes()),
        ],
    )
}

/// `shape` (a `draw:custom-shape` made by `rect`) with the id `id` as `draw:id` alone, as a file of ODF 1.0 or 1.1 has it.
fn with_draw_id(shape: String, id: &str) -> String {
    shape.replacen("<draw:custom-shape ", &format!("<draw:custom-shape draw:id=\"{id}\" "), 1)
}

/// `shape` with the id `id` as `xml:id` and `draw:id` both, as a file of ODF 1.2 or later has it.
fn with_both_ids(shape: String, id: &str) -> String {
    shape.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1)
}

/// A small PNG, for a picture frame.
fn small_png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200u8; 16]).unwrap();
    out
}

/// A custom shape of the form gives for a rectangle, with no style at all (`draw:style-name`, `draw:text-style-name`).
fn bare(name: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    format!(
        "<draw:custom-shape draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"{w}cm\" svg:height=\"{h}cm\" svg:x=\"{x}cm\" svg:y=\"{y}cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    )
}

/// The automatic style the shape called `name` names, from `content.xml`.
fn style_of_shape(content: &str, name: &str) -> String {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name}: {content}"));
    let start = content[..at].rfind('<').unwrap();
    let style = attribute(&content[start..], "draw:style-name").unwrap_or_else(|| panic!("the shape {name} names no style: {content}"));
    let open = content.find(&format!("style:name=\"{style}\"")).unwrap_or_else(|| panic!("no style {style}: {content}"));
    let s = content[..open].rfind('<').unwrap();
    let end = content[s..].find("</style:style>").map_or(content.len(), |e| s + e + 14);
    content[s..end].to_owned()
}

/// The frame's flips as the model holds them: `FlipX` and `FlipY` of the shape called `name`.
fn flips(doc: &Document, name: &str) -> (bool, bool) {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap();
    let on = |cell: &str| shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()) == Some(1.0);
    (on("FlipX"), on("FlipY"))
}

/// Where the corners of the shape called `name` are on page 0, in the order of its own frame.
fn corners(doc: &Document, name: &str) -> Vec<(f64, f64)> {
    let page = doc.pages.get(0).unwrap().id;
    diacraft_engine::draw::shape_corners(doc, page, id_of(doc, 0, name)).unwrap().iter().map(|p| (p.0, p.1)).collect()
}

/// The session saved, and the file read again as a session: `(the drawing as saved, the drawing as read)`.
fn saved_and_read(dir: &Dir, s: &mut Session) -> (Document, Document) {
    save(s);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    (s.document().unwrap().clone(), again.document().unwrap().clone())
}

// ---- F1: keep to the declared version ----------------------------------------------------------------------------------------------------------------

/// V1 (F1): a connector made in a document of ODF 1.0 or 1.1 is written with `svg:x1` to `svg:y2` and no `svg:d`, no `svg:viewBox`, and
/// glued by `draw:id` alone: the schemas of those versions have neither `svg:d` nor `svg:viewBox` on a `draw:connector`, and no `xml:id` anywhere. The
/// version is never raised.
#[test]
fn a_connector_made_in_an_odf_1_0_or_1_1_document_has_no_svg_d_and_glues_by_draw_id_alone() {
    for version in ["1.0", "1.1", ""] {
        for ids in [true, false] {
            let what = format!("version {version:?}, shapes with ids: {ids}");
            let (a, b) = (rect("A", (2.0, 3.0, 4.0, 2.0), ""), rect("B", (12.0, 3.0, 4.0, 2.0), ""));
            let drawing = if ids {
                Drawing::new().page(&[with_draw_id(a, "s1"), with_draw_id(b, "s2")].concat())
            } else {
                Drawing::new().page(&[a, b].concat())
            };
            let dir = Dir::new();
            let mut s = open_bytes_in(&dir, "a.odg", &declared(&drawing, version));
            let doc = s.document().unwrap().clone();
            run(&mut s, "connector.add", json!({ "from": { "id": id_of(&doc, 0, "A") }, "to": { "id": id_of(&doc, 0, "B") } }));
            let saved = save(&mut s);
            assert_eq!(saved["edited"], true, "{what}");
            let bytes = dir.read("a.odg");
            let content = content_of(&bytes);
            assert_eq!(
                content.contains("office:version="),
                !version.is_empty(),
                "{what}: the version is the file's own, and none stays none: {content}"
            );
            let (read, report) = read_back(&bytes);
            assert_eq!(report.office_version.as_deref(), Some(version).filter(|v| !v.is_empty()), "{what}");
            let connectors = elements(&content, "draw:connector");
            assert_eq!(connectors.len(), 1, "{what}: {content}");
            let connector = &connectors[0];
            assert!(
                attribute(connector, "svg:d").is_none() && attribute(connector, "svg:viewBox").is_none(),
                "{what}: no route as a path: {connector}"
            );
            for end in ["svg:x1", "svg:y1", "svg:x2", "svg:y2"] {
                assert!(attribute(connector, end).is_some(), "{what}: the route is its ends: {connector}");
            }
            assert!(!content.contains("xml:id"), "{what}: no xml:id in a document of {version}: {content}");
            let (start, end) = (attribute(connector, "draw:start-shape").unwrap(), attribute(connector, "draw:end-shape").unwrap());
            let expected = if ids { ("s1", "s2") } else { ("id1", "id2") };
            assert_eq!((start.as_str(), end.as_str()), expected, "{what}: glued by draw:id");
            for id in [&start, &end] {
                assert_eq!(content.matches(&format!("draw:id=\"{id}\"")).count(), 1, "{what}: {id} names one shape by draw:id: {content}");
            }
            // It reads back glued to the two shapes, and the drawing is what the session holds.
            assert_eq!(read.pages.get(0).unwrap().connects.len(), 2, "{what}: glued at both ends");
            assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
            // The same document in ODF 1.2 keeps what it always had: `xml:id` beside `draw:id`, and the path.
        }
    }
    let dir = Dir::new();
    let drawing = Drawing::new().page(&[rect("A", (2.0, 3.0, 4.0, 2.0), ""), rect("B", (12.0, 3.0, 4.0, 2.0), "")].concat());
    let mut s = open_bytes_in(&dir, "a.odg", &declared(&drawing, "1.2"));
    let doc = s.document().unwrap().clone();
    run(&mut s, "connector.add", json!({ "from": { "id": id_of(&doc, 0, "A") }, "to": { "id": id_of(&doc, 0, "B") } }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    let connector = &elements(&content, "draw:connector")[0];
    assert!(attribute(connector, "svg:d").is_some() && attribute(connector, "svg:viewBox").is_some() && content.contains("xml:id="), "{content}");
}

/// A shape and a connector glued to it by `draw:id` alone, with no `svg:d`, as a document of ODF 1.0 or 1.1 holds them; `stale` also gives the connector an
/// `svg:d` and an `svg:viewBox` that the schemas of those versions do not have.
fn shape_and_connector(stale: bool) -> Drawing {
    let route = if stale { " svg:d=\"M 6000 4000 L 12000 9000\" svg:viewBox=\"0 0 6001 5001\"" } else { "" };
    let connector = format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"K\" draw:start-shape=\"s1\" draw:start-glue-point=\"1\" svg:x1=\"6cm\" svg:y1=\"4cm\" svg:x2=\"12cm\" svg:y2=\"9cm\"{route}><text:p/></draw:connector>"
    );
    Drawing::new().page(&[with_draw_id(rect("A", (2.0, 3.0, 4.0, 2.0), ""), "s1"), connector].concat())
}

/// V2 (F1): a shape moved 1 inch to the right in a document of ODF 1.0, and its connector, glued to it, follows by its ends alone: no `svg:d` is
/// added. A connector that has one (a file that is not valid for its version) loses it with the route it described, since the ends are the route now.
#[test]
fn a_connector_routed_again_in_an_odf_1_0_or_1_1_document_gets_no_svg_d() {
    for version in ["1.0", "1.1"] {
        for stale in [false, true] {
            let what = format!("version {version}, an svg:d in the file: {stale}");
            let dir = Dir::new();
            let mut s = open_bytes_in(&dir, "a.odg", &declared(&shape_and_connector(stale), version));
            shift(&mut s, "A", 1.0, 0.0);
            assert_eq!(save(&mut s)["edited"], true, "{what}");
            let content = content_of(&dir.read("a.odg"));
            let connector = &elements(&content, "draw:connector")[0];
            assert!(attribute(connector, "svg:d").is_none() && attribute(connector, "svg:viewBox").is_none(), "{what}: {connector}");
            let x1: f64 = attribute(connector, "svg:x1").unwrap().trim_end_matches("cm").parse().unwrap();
            assert!((x1 - 8.54).abs() < 1e-6, "{what}: the start follows the shape by 1 inch: {connector}");
            assert_eq!(attribute(connector, "svg:x2").as_deref(), Some("12cm"), "{what}: the free end stays");
            assert!(!content.contains("xml:id"), "{what}");
            assert!(content.contains(&format!("office:version=\"{version}\"")));
            assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// Every kind of edit the first milestone names, in a document of ODF 1.0 and 1.1 (`odg_file.rs`'s list): what the save adds is only what the schema of the
/// declared version has, so no `xml:id`, no `svg:d` or `svg:viewBox` on a connector, and the version as it was. The attribute names are those the writer
/// adds (`write/own.rs`'s table and `write/shapes.rs`), each checked against the OASIS RelaxNG schemas of ODF 1.0 and 1.1.
#[test]
fn every_kind_of_edit_in_an_odf_1_0_or_1_1_document_writes_what_that_version_has() {
    for version in ["1.0", "1.1"] {
        for (case, edit) in edit_cases() {
            let what = format!("version {version}, {case}");
            let dir = Dir::new();
            let mut s = open_bytes_in(&dir, "a.odg", &declared(&four(), version));
            let doc = s.document().unwrap().clone();
            edit(&mut s, &doc);
            let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(saved["edited"], true, "{what}");
            let content = content_of(&dir.read("a.odg"));
            assert!(content.contains(&format!("office:version=\"{version}\"")), "{what}: {content}");
            assert!(!content.contains("xml:id="), "{what}: {content}");
            for connector in elements(&content, "draw:connector") {
                assert!(attribute(&connector, "svg:d").is_none() && attribute(&connector, "svg:viewBox").is_none(), "{what}: {connector}");
            }
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

// ---- F2: the connectors of a neighbour --------------------------------------------------------------------------------------------------------------

/// A standard connector from `from` (glue point `from_glue`) to `to` (`to_glue`) with the route LibreOffice gives it: an absolute
/// `M` and relative commands, whose legs are `legs` (relative moves, `h` and `v` in hundredths of a millimetre).
fn routed(name: &str, (from, from_glue): (&str, u32), (to, to_glue): (&str, u32), ends: (f64, f64, f64, f64), legs: &str) -> String {
    let (x1, y1, x2, y2) = ends;
    let (w, h) = ((x2 - x1).abs() * 1000.0 + 1.0, (y2 - y1).abs() * 1000.0 + 1.0);
    format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" draw:start-shape=\"{from}\" draw:start-glue-point=\"{from_glue}\" draw:end-shape=\"{to}\" draw:end-glue-point=\"{to_glue}\" svg:x1=\"{x1}cm\" svg:y1=\"{y1}cm\" svg:x2=\"{x2}cm\" svg:y2=\"{y2}cm\" svg:d=\"M {} {} {legs}\" svg:viewBox=\"0 0 {w} {h}\"><text:p/></draw:connector>",
        x1 * 1000.0,
        y1 * 1000.0
    )
}

/// V3 (F2): shapes A, B and C, a connector from A to B and a connector from C to B, each glued to a connection point of B. A moves 0.5 inch to
/// the right: the save changes A and the connector from A to B, and every other connector keeps its bytes. **Where the extra routes came from**: the engine's
/// line jumps (`draw/jumps.rs`) rebuilt every connector whose legs cross the old or the new route of one it routed, and wrote its rows again
/// though it needed no jump, in the form the engine writes them (collinear points dropped, other formulas), so the model's connector changed and the save
/// wrote its `svg:d` again. Tried with the neighbour entering B at the same point as the moved connector, across the moved connector's old route and not
/// its new one, and glued to the point below.
#[test]
fn a_move_routes_again_only_the_connectors_glued_to_the_shape_it_moved() {
    let cases: Vec<(&str, String)> = vec![
        (
            "a neighbour that enters B at the same point and runs along it",
            routed("K2", ("c", 1), ("b", 3), (6.0, 9.0, 12.0, 4.0), "h 3000 v -5000 h 3000"),
        ),
        ("a neighbour that crosses the old route and not the new one", routed("K2", ("c", 2), ("d", 0), (6.5, 2.0, 6.5, 9.0), "L 6500 9000")),
        (
            "a neighbour with a point on its leg that crosses the old route and not the new one",
            routed("K2", ("c", 2), ("d", 0), (6.5, 2.0, 6.5, 9.0), "v 3500 v 3500"),
        ),
        ("a neighbour glued to the point below", routed("K2", ("c", 1), ("b", 2), (6.0, 9.0, 14.0, 5.0), "h 8000 v -4000")),
    ];
    for (case, neighbour) in cases {
        let shapes = [
            with_both_ids(rect("A", (2.0, 3.0, 4.0, 2.0), ""), "a"),
            with_both_ids(rect("B", (12.0, 3.0, 4.0, 2.0), ""), "b"),
            with_both_ids(rect("C", (4.5, 0.0, 4.0, 2.0), ""), "c"),
            with_both_ids(rect("D", (4.5, 9.0, 4.0, 2.0), ""), "d"),
            routed("K1", ("a", 1), ("b", 3), (6.0, 4.0, 12.0, 4.0), "h 6000"),
            neighbour,
        ]
        .concat();
        let drawing = Drawing::new().page(&shapes);
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing);
        let before = elements(&content_of(&dir.read("a.odg")), "draw:connector");
        let held = s.document().unwrap().pages.get(0).unwrap().shapes.clone();
        shift(&mut s, "A", 0.5, 0.0);
        let after_model = s.document().unwrap().pages.get(0).unwrap().shapes.clone();
        assert_eq!(save(&mut s)["edited"], true, "{case}");
        // The file: the connector of the shape that moved follows it, and the neighbour's connector keeps its bytes.
        let content = content_of(&dir.read("a.odg"));
        let after = elements(&content, "draw:connector");
        assert_eq!(after.len(), 2, "{case}");
        assert_ne!(after[0], before[0], "{case}: the connector of the shape that moved follows it");
        assert_eq!(after[1], before[1], "{case}: the connector of its neighbour keeps its bytes: {}", after[1]);
        // The engine's model: the neighbour is the one it was (the rebuild of line jumps rewrote its rows though it needed none).
        let named = |shapes: &diacraft_doc::ShapeList, name: &str| shapes.iter().find(|x| x.name_u.as_deref() == Some(name)).cloned().unwrap();
        assert!(named(&held, "K1") != named(&after_model, "K1"), "{case}: the connector of the shape that moved follows it");
        assert!(named(&held, "K2") == named(&after_model, "K2"), "{case}: the model's neighbour is as it was");
        // And the whole of `content.xml` differs by the shape and its connector, and nothing else.
        let kept = |text: &str| -> String {
            let mut text = text.to_owned();
            for tag in ["draw:custom-shape", "draw:connector"] {
                for e in elements(&text.clone(), tag).iter().filter(|e| e.contains("draw:name=\"A\"") || e.contains("draw:name=\"K1\"")) {
                    text = text.replacen(e.as_str(), "", 1);
                }
            }
            text
        };
        assert_eq!(kept(&content), kept(&content_of(&drawing.bytes())), "{case}: nothing else of the part changed");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

// ---- F3: the text of a picture frame ------------------------------------------------------------------------------------------------------------------

/// A page holding a frame with a picture (and, where `inside` says, an empty paragraph in the picture, as LibreOffice writes one) and a rectangle.
fn picture_frame(inside: &str) -> Vec<u8> {
    let frame = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"P\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"3cm\"><draw:image xlink:href=\"Pictures/p.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\">{inside}</draw:image></draw:frame>"
    );
    Drawing::new().page(&[frame, rect("A", (12.0, 3.0, 4.0, 2.0), "hello")].concat()).file("Pictures/p.png", &small_png()).bytes()
}

/// V5 (F3): text set on a picture frame. No spec says where a graphic frame's text goes in the file and the reader reads none from the picture,
/// so the save is refused in words and nothing is written; the way out is to take the text back, and the drawing then saves. The same for an object.
#[test]
fn text_set_on_a_picture_or_object_frame_is_refused_in_words_and_writes_nothing() {
    let object = String::from(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"P\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"3cm\"><draw:object xlink:href=\"./Object 1\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><text:p/></draw:object><draw:image xlink:href=\"Pictures/p.png\" xlink:type=\"simple\"/></draw:frame>",
    );
    let object =
        Drawing::new().page(&object).file("Pictures/p.png", &small_png()).file("Object 1/content.xml", b"<office:document-content/>").bytes();
    for (case, bytes) in [
        ("a picture with an empty paragraph in it", picture_frame("<text:p/>")),
        ("a picture with no paragraph", picture_frame("")),
        ("an object", object),
    ] {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &bytes);
        let frame = id_of(s.document().unwrap(), 0, "P");
        run(&mut s, "shape.set_text", json!({ "id": frame, "text": "Ab" }));
        let before = dir.read("a.odg");
        let said = refused(&mut s);
        assert!(said.contains("a text on a picture or an object frame"), "{case}: {said}");
        assert_eq!(dir.read("a.odg"), before, "{case}: nothing was written");
        assert!(s.is_dirty(), "{case}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(save(&mut s)["edited"], false, "{case}: the drawing is the file's again");
    }
    // A text frame, whose text is in its `draw:text-box`, is not refused, and what is written is read back from where it is written.
    let text_frame = "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"T\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"3cm\"><draw:text-box><text:p>old</text:p></draw:text-box></draw:frame>";
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(text_frame));
    let frame = id_of(s.document().unwrap(), 0, "T");
    run(&mut s, "shape.set_text", json!({ "id": frame, "text": "new", "start": 0, "end": 3 }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:text-box><text:p>new</text:p></draw:text-box>"), "{content}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

// ---- F4: the fill the model holds ---------------------------------------------------------------------------------------------------------------------

/// A drawing whose shape `G` is on an automatic style with `fill` (a `draw:fill` and what goes with it), and whose `office:styles` holds a gradient, a
/// hatch and a bitmap by the names the styles give.
fn filled(fill: &str) -> Vec<u8> {
    let mut drawing = Drawing::new();
    drawing.auto.push_str(&format!(
        "<style:style style:name=\"gr3\" style:family=\"graphic\"><style:graphic-properties {fill} draw:stroke=\"solid\"/></style:style>"
    ));
    let shape = rect("G", (2.0, 3.0, 4.0, 2.0), "").replace("draw:style-name=\"gr1\"", "draw:style-name=\"gr3\"");
    let drawing = drawing.page(&shape);
    let definitions = "<draw:gradient draw:name=\"g1\" draw:style=\"linear\" draw:start-color=\"#729fcf\" draw:end-color=\"#ffffff\" draw:angle=\"0deg\" draw:border=\"0%\"/><draw:hatch draw:name=\"h1\" draw:style=\"single\" draw:color=\"#3465a4\" draw:distance=\"0.1cm\" draw:rotation=\"0\"/>";
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), drawing.content_xml().into_bytes()),
            (
                "styles.xml".to_owned(),
                drawing.styles_xml().replace("<office:styles/>", &format!("<office:styles>{definitions}</office:styles>")).into_bytes(),
            ),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    )
}

/// V4 (F4): a solid fill set on a shape whose fill is a gradient writes `draw:fill="solid"` (and not `draw:fill-color` alone, which an ODF reader
/// draws as the gradient still); the model holds solid, and the names of the gradient go from the style. The same for a hatch and a bitmap.
#[test]
fn a_solid_fill_set_on_a_gradient_hatch_or_bitmap_writes_draw_fill_solid() {
    let cases = [
        ("a gradient", "draw:fill=\"gradient\" draw:fill-gradient-name=\"g1\" draw:fill-color=\"#729fcf\""),
        ("a hatch that is solid", "draw:fill=\"hatch\" draw:fill-hatch-name=\"h1\" draw:fill-hatch-solid=\"true\" draw:fill-color=\"#729fcf\""),
        ("a hatch that is not", "draw:fill=\"hatch\" draw:fill-hatch-name=\"h1\" draw:fill-color=\"#729fcf\""),
        ("a bitmap", "draw:fill=\"bitmap\" draw:fill-image-name=\"i1\" draw:fill-color=\"#729fcf\""),
    ];
    for (case, fill) in cases {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &filled(fill));
        let id = id_of(s.document().unwrap(), 0, "G");
        run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#123456" }));
        save(&mut s);
        let content = content_of(&dir.read("a.odg"));
        let style = style_of_shape(&content, "G");
        assert!(style.contains("draw:fill=\"solid\"") && style.contains("draw:fill-color=\"#123456\""), "{case}: {style}");
        for gone in ["draw:fill-gradient-name", "draw:fill-hatch-name", "draw:fill-hatch-solid", "draw:fill-image-name"] {
            assert!(!style.contains(gone), "{case}: {gone} goes from the style: {style}");
        }
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    // A fill taken away from a gradient is `none`, and a colour set on a shape that had none is solid, as before.
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &filled("draw:fill=\"gradient\" draw:fill-gradient-name=\"g1\""));
    let id = id_of(s.document().unwrap(), 0, "G");
    run(&mut s, "shape.format", json!({ "ids": [id], "fill": "none" }));
    save(&mut s);
    let style = style_of_shape(&content_of(&dir.read("a.odg")), "G");
    assert!(style.contains("draw:fill=\"none\"") && !style.contains("draw:fill-gradient-name"), "{style}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// F4's third case (the measured frame): a shape whose chain of styles states no `draw:fill` is drawn by the model with the root's fill, which is solid; a
/// colour set on it writes `draw:fill="solid"` too, or the file does not say what the model draws.
#[test]
fn a_colour_set_on_a_shape_whose_styles_state_no_fill_mode_states_one() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &filled(""));
    let id = id_of(s.document().unwrap(), 0, "G");
    run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#123456" }));
    save(&mut s);
    let style = style_of_shape(&content_of(&dir.read("a.odg")), "G");
    assert!(style.contains("draw:fill=\"solid\"") && style.contains("draw:fill-color=\"#123456\""), "{style}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

// ---- F5: a turn of a shape flipped both ways ---------------------------------------------------------------------------------------------------------

/// A custom shape `F` of 4 by 2 centimetres turned 90 degrees (`rotate` and `translate`), with the mirrors `horizontal` and `vertical` of its geometry.
fn mirrored(horizontal: bool, vertical: bool) -> Drawing {
    let shape = format!(
        "<draw:custom-shape draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"F\" svg:width=\"4cm\" svg:height=\"2cm\" draw:transform=\"rotate (1.5707963267948966) translate (8cm 6cm)\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:mirror-horizontal=\"{horizontal}\" draw:mirror-vertical=\"{vertical}\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    );
    Drawing::new().page(&shape)
}

/// V6 (F5, Ruling: against): a custom shape with both mirrors, turned 90 degrees and then to 30. The session reads `Angle` 30
/// with both flips; the file read again must give 30 with both flips (the transform holds the angle and the mirrors the flips; the measured save gave 210),
/// and draw the shape where the session does.
#[test]
fn a_turn_of_a_shape_flipped_both_ways_reads_back_as_the_angle_asked() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &mirrored(true, true));
    let id = id_of(s.document().unwrap(), 0, "F");
    run(&mut s, "shape.rotate", json!({ "ids": [id], "angle": 30 }));
    let (made, read) = saved_and_read(&dir, &mut s);
    let angle = |doc: &Document| crate::odg_build::frame_of(doc, 0, "F")[4].to_degrees();
    assert!((angle(&made) - 30.0).abs() < 1e-6, "the session holds 30 degrees: {}", angle(&made));
    assert!((angle(&read) - 30.0).abs() < 1e-6, "the file reads as 30 degrees and not 210: {}", angle(&read));
    assert_eq!(flips(&read, "F"), (true, true));
    for (a, b) in corners(&made, "F").iter().zip(corners(&read, "F").iter()) {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "the shape is drawn where the session draws it: {:?} against {:?}",
            corners(&made, "F"),
            corners(&read, "F")
        );
    }
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// All four combinations of the two flips, each turned to three angles: through the save and the read, the frame, the flips and the places of the corners
/// are what the session holds. A turn of a shape the save does not write is refused in words and never written wrong.
#[test]
fn a_turn_of_a_shape_with_any_flips_reads_back_as_the_session_holds_it_at_every_angle() {
    for (horizontal, vertical) in [(false, false), (true, false), (false, true), (true, true)] {
        for degrees in [30, 100, 250, -60] {
            let what = format!("mirrors {horizontal} and {vertical}, turned to {degrees}");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &mirrored(horizontal, vertical));
            let id = id_of(s.document().unwrap(), 0, "F");
            run(&mut s, "shape.rotate", json!({ "ids": [id], "angle": degrees }));
            let (made, read) = saved_and_read(&dir, &mut s);
            assert_eq!(flips(&made, "F"), (horizontal, vertical), "{what}: the session's flips");
            assert_eq!(flips(&read, "F"), (horizontal, vertical), "{what}: the file's flips");
            let (a, b) = (corners(&made, "F"), corners(&read, "F"));
            for (p, q) in a.iter().zip(b.iter()) {
                assert!((p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6, "{what}: {a:?} against {b:?}");
            }
            crate::odg_build::same_frame(crate::odg_build::frame_of(&made, 0, "F"), crate::odg_build::frame_of(&read, 0, "F"), &what);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// A flip that the transform holds (a negative scale) is not the mirrors of the geometry: a turn of such a shape would drop it, and the save refuses in words.
#[test]
fn a_flip_that_the_transform_holds_is_not_dropped_by_a_turn() {
    let shape = "<draw:custom-shape draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"F\" svg:width=\"4cm\" svg:height=\"2cm\" draw:transform=\"scale (1 -1) translate (8cm 6cm)\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>";
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(shape));
    let id = id_of(s.document().unwrap(), 0, "F");
    assert_eq!(flips(s.document().unwrap(), "F"), (false, true), "the negative scale reads as FlipY");
    run(&mut s, "shape.rotate", json!({ "ids": [id], "angle": 30 }));
    let said = refused(&mut s);
    assert!(said.contains("flip"), "{said}");
}

/// Two flips, one on each axis (two `shape.flip` calls), of a shape that has both mirrors and of one that has none: the frame's linear part is then a turn of half a
/// turn, which the file's mirrors would not follow, so the save refuses it in words and writes nothing (the reader would give the other angle with the old mirrors).
#[test]
fn a_change_of_both_flips_is_refused_in_words_and_the_undo_saves() {
    for (horizontal, vertical) in [(true, true), (false, false)] {
        let what = format!("mirrors {horizontal} and {vertical}, flipped on both axes");
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &mirrored(horizontal, vertical));
        let id = id_of(s.document().unwrap(), 0, "F");
        run(&mut s, "shape.flip", json!({ "ids": [id], "axis": "horizontal" }));
        run(&mut s, "shape.flip", json!({ "ids": [id], "axis": "vertical" }));
        assert_eq!(flips(s.document().unwrap(), "F"), (!horizontal, !vertical), "{what}: the session's flips");
        let said = refused(&mut s);
        assert!(said.contains("flip"), "{what}: {said}");
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(save(&mut s)["edited"], false, "{what}: the flips as they were is no edit");
    }
}

// ---- V7: a page with no shape -----------------------------------------------------------------------------------------------------------------------

/// A drawing of two pages whose first has no shape, the page written as `form` says: an empty element (`<draw:page .../>`, which LibreOffice writes),
/// one with a start tag and an end tag, or one that holds white space.
fn empty_first_page(form: &str, two_pages: bool) -> Vec<u8> {
    let mut drawing = Drawing::new().page("");
    if two_pages {
        drawing = drawing.page(&rect("A", (2.0, 3.0, 4.0, 2.0), ""));
    }
    let swap = |text: String| match form {
        "empty element" => text.replacen("></draw:page>", "/>", 1),
        "white space" => text.replacen("></draw:page>", ">\n  </draw:page>", 1),
        _ => text,
    };
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), swap(drawing.content_xml()).into_bytes()),
            ("styles.xml".to_owned(), drawing.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    )
}

/// A shape added to a page that holds no shape: a drop, a connector between two places, and a text box. The save is not refused by its
/// own read-back check, and the file reads back as the session holds it. The page that LibreOffice writes for it is an empty element, whose end is its start
/// tag's: the shape goes inside it, and the element is opened.
#[test]
fn a_shape_a_connector_and_a_text_box_added_to_an_empty_page_are_saved_and_read_back() {
    type Add = Box<dyn Fn(&mut Session)>;
    let cases: Vec<(&str, Add)> = vec![
        ("a drop", Box::new(|s| drop(run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }))))),
        ("a connector", Box::new(|s| drop(run(s, "connector.add", json!({ "from": { "x": 1.0, "y": 1.0 }, "to": { "x": 3.0, "y": 1.5 } }))))),
        ("a text box", Box::new(|s| drop(run(s, "text.box", json!({ "page": 0, "x": 4.0, "y": 2.0, "text": "a label" }))))),
        (
            "a drop and a text",
            Box::new(|s| {
                let made = run(s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 4.0, "y": 2.0 }));
                run(s, "shape.set_text", json!({ "id": made["id"], "text": "round" }));
            }),
        ),
    ];
    for (case, add) in cases {
        for form in ["empty element", "start and end tag", "white space"] {
            for two_pages in [false, true] {
                let what = format!("{case} on a page that is {form}, in a drawing of {} page(s)", if two_pages { 2 } else { 1 });
                let dir = Dir::new();
                let mut s = open_bytes_in(&dir, "a.odg", &empty_first_page(form, two_pages));
                add(&mut s);
                let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(saved["edited"], true, "{what}");
                let (read, _) = read_back(&dir.read("a.odg"));
                assert_eq!(read.pages.get(0).unwrap().shapes.len(), 1, "{what}: the page holds what was added");
                if two_pages {
                    assert_eq!(read.pages.get(1).unwrap().shapes.len(), 1, "{what}: and the other page what it held");
                }
                assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
                assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
                check_session(&s).unwrap();
            }
        }
    }
}

// ---- V8: a part with no automatic styles ------------------------------------------------------------------------------------------------------------

/// A drawing whose `content.xml` has no `office:automatic-styles` element, with two shapes that name no style.
fn no_automatic_styles() -> Vec<u8> {
    let drawing = Drawing::new().page(&[bare("A", (2.0, 3.0, 4.0, 2.0)), bare("B", (12.0, 3.0, 4.0, 2.0))].concat());
    let content = drawing.content_xml().replace(&format!("<office:automatic-styles>{}</office:automatic-styles>", drawing.auto), "");
    assert!(!content.contains("automatic-styles"), "{content}");
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), content.into_bytes()),
            ("styles.xml".to_owned(), drawing.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    )
}

/// A `content.xml` with no `office:automatic-styles`: a restyle, a new shape and a new connector each add the element, empty, before
/// `office:body` and after `office:font-face-decls` (where the schema puts it), and the new style in it.
#[test]
fn a_content_part_with_no_automatic_styles_gets_the_element_and_the_new_style_in_it() {
    type Edit = Box<dyn Fn(&mut Session)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a restyle",
            Box::new(|s| {
                let id = id_of(s.document().unwrap(), 0, "A");
                run(s, "shape.format", json!({ "ids": [id], "fill": "#123456" }));
            }),
        ),
        ("a new shape", Box::new(|s| drop(run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 6.0 }))))),
        (
            "a new connector",
            Box::new(|s| {
                let doc = s.document().unwrap().clone();
                run(s, "connector.add", json!({ "from": { "id": id_of(&doc, 0, "A") }, "to": { "id": id_of(&doc, 0, "B") } }));
            }),
        ),
        (
            "a retext",
            Box::new(|s| {
                let id = id_of(s.document().unwrap(), 0, "A");
                run(s, "shape.set_text", json!({ "id": id, "text": "x" }));
            }),
        ),
    ];
    for (case, edit) in cases {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &no_automatic_styles());
        edit(&mut s);
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_eq!(saved["edited"], true, "{case}");
        let content = content_of(&dir.read("a.odg"));
        assert_eq!(content.matches("<office:automatic-styles>").count(), 1, "{case}: one element: {content}");
        let (faces, auto, body) = (
            content.find("</office:font-face-decls>").or_else(|| content.find("<office:font-face-decls/>")).unwrap(),
            content.find("<office:automatic-styles>").unwrap(),
            content.find("<office:body").unwrap(),
        );
        assert!(faces < auto && auto < body, "{case}: after the font declarations and before the body: {content}");
        let closes = content.find("</office:automatic-styles>").unwrap();
        assert!(closes < body && content[auto..closes].contains("<style:style "), "{case}: the new style is in it, ahead of the body: {content}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    // The element of a part that has none is also made when a part binds its namespaces under other prefixes.
    let drawing = Drawing::new().page(&bare("A", (2.0, 3.0, 4.0, 2.0)));
    let content = drawing.content_xml().replace(&format!("<office:automatic-styles>{}</office:automatic-styles>", drawing.auto), "");
    // The prefixes of the namespaces of `office` and `style` are changed to `o` and `st`, the declarations and the names that use them.
    let renamed = content
        .replace("<office:", "<o:")
        .replace("</office:", "</o:")
        .replace(" office:version=", " o:version=")
        .replace(" xmlns:office=", " xmlns:o=")
        .replace("<style:", "<st:")
        .replace("</style:", "</st:")
        .replace(" style:name=", " st:name=")
        .replace(" style:family=", " st:family=")
        .replace(" xmlns:style=", " xmlns:st=");
    let bytes = package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), renamed.into_bytes()),
            ("styles.xml".to_owned(), drawing.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    );
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &bytes);
    let id = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#123456" }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<o:automatic-styles><st:style ") && content.contains("</o:automatic-styles><o:body"), "{content}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

// ---- V9: a manifest with the OpenOffice.org document type declaration --------------------------------------------------------------------------------

const OOO_DOCTYPE: &str = "<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">";

/// A drawing of ODF 1.1 with a frame that holds an embedded object (`Object 1`, with its replacement image) and a rectangle, whose manifest is the
/// OpenOffice.org 2 one: a document type declaration with an external id and no internal subset. `thumbnail` says whether the package holds a thumbnail.
fn old_manifest(thumbnail: bool) -> Vec<u8> {
    old_manifest_with(thumbnail, OOO_DOCTYPE)
}

/// [`old_manifest`] under another document type declaration.
fn old_manifest_with(thumbnail: bool, doctype: &str) -> Vec<u8> {
    let frame = "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"O\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"3cm\"><draw:object xlink:href=\"./Object 1\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/><draw:image xlink:href=\"./ObjectReplacements/Object 1\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>";
    let drawing = Drawing::new().page(&[frame.to_owned(), rect("A", (12.0, 3.0, 4.0, 2.0), "")].concat());
    let swap = |text: String| text.replace("office:version=\"1.3\"", "office:version=\"1.1\"");
    let mut files: Vec<(&str, Vec<u8>, &str)> = vec![
        ("content.xml", swap(drawing.content_xml()).into_bytes(), "text/xml"),
        ("styles.xml", swap(drawing.styles_xml()).into_bytes(), "text/xml"),
        ("meta.xml", swap(drawing.meta_xml()).into_bytes(), "text/xml"),
        ("Object 1/content.xml", b"<office:document-content/>".to_vec(), "text/xml"),
        ("ObjectReplacements/Object 1", small_png(), "image/png"),
    ];
    if thumbnail {
        files.push(("Thumbnails/thumbnail.png", small_png(), "image/png"));
    }
    let mut manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{doctype}\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">\n <manifest:file-entry manifest:media-type=\"{DRAWING}\" manifest:full-path=\"/\"/>\n <manifest:file-entry manifest:media-type=\"{DRAWING}\" manifest:full-path=\"Object 1/\"/>\n"
    );
    for (name, _, media) in &files {
        manifest.push_str(&format!(" <manifest:file-entry manifest:media-type=\"{media}\" manifest:full-path=\"{name}\"/>\n"));
    }
    manifest.push_str("</manifest:manifest>\n");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    for (name, data, media) in &files {
        w.add(name, data, if *media == "image/png" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// Deleting a frame that holds an embedded object, in a package whose manifest has the OpenOffice.org 2 document type
/// declaration, which the reader accepts. The save reads its own file with what the reader accepts: the object's entries and manifest rows go, the
/// declaration stays as the file had it, and nothing else of the manifest changes.
#[test]
fn a_frame_with_an_object_is_deleted_from_a_package_whose_manifest_has_the_old_document_type_declaration() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &old_manifest(true));
    let frame = id_of(s.document().unwrap(), 0, "O");
    run(&mut s, "shape.delete", json!({ "ids": [frame] }));
    let before = part_of(&dir.read("a.odg"), "META-INF/manifest.xml");
    assert_eq!(save(&mut s)["edited"], true);
    let bytes = dir.read("a.odg");
    for gone in ["Object 1/content.xml", "ObjectReplacements/Object 1"] {
        assert!(part_bytes(&bytes, gone).is_none(), "{gone} goes with its frame");
    }
    let manifest = part_of(&bytes, "META-INF/manifest.xml");
    assert!(manifest.contains(OOO_DOCTYPE), "the declaration stays: {manifest}");
    assert!(!manifest.contains("Object 1"), "the rows of the object go: {manifest}");
    let kept: Vec<&str> = before.lines().filter(|l| !l.contains("Object 1")).collect();
    assert_eq!(kept, manifest.lines().collect::<Vec<_>>(), "and nothing else of the manifest changes");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Any edit of such a package that adds a manifest row (the thumbnail the save writes, when the package has none) reads the manifest the same way.
#[test]
fn an_edit_of_a_package_with_the_old_manifest_and_no_thumbnail_adds_its_row() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &old_manifest(false));
    shift(&mut s, "A", 1.0, 0.0);
    assert_eq!(save(&mut s)["edited"], true);
    let bytes = dir.read("a.odg");
    let manifest = part_of(&bytes, "META-INF/manifest.xml");
    assert!(manifest.contains(OOO_DOCTYPE) && manifest.contains("manifest:full-path=\"Thumbnails/thumbnail.png\""), "{manifest}");
    assert!(part_bytes(&bytes, "Thumbnails/thumbnail.png").is_some());
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// What the reader still refuses, the save never gets to: a document type declaration with an internal subset.
#[test]
fn a_manifest_with_an_internal_subset_is_still_refused_by_the_reader() {
    let dir = Dir::new();
    dir.write("a.odg", &old_manifest_with(true, "<!DOCTYPE manifest:manifest [<!ENTITY x \"y\">]>"));
    let mut s = Session::new();
    assert!(s.execute("file.open", &json!({ "path": dir.path("a.odg") })).is_err());
}
