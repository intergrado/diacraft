//! What the `.odg` writer writes of glue points and transforms, against what LibreOffice 26.8.2.2 draws of the file (a black box: hand-written flat drawings
//! and DiaCraft's own saves converted to SVG with its own profile; the reader's rules are in the odg crate's `read_glue_points.rs`).
//!
//! - **A new shape whose connection points are not the four side middles** (a right triangle: a corner, the middle of the left side, the middle of the bottom and the middle
//!   of the slope) has them written as its geometry's own glue points (`draw:glue-points`), its style says its rows start at 0 with the first of them
//!   (`diacraft:glue="own"`), and a connector to row *r* names glue point *r* + 4. LibreOffice puts such a point where the session has the row, mirrored and turned with the
//!   shape (on main the writer wrote none and named a standard point: 24 of 32 connectors ended 0.75 inch from the session's ends).
//! - **A move of a shape whose transform's translation has no unit** (hundredths of a millimetre, as LibreOffice reads it) writes the translation in the same form.

use diacraft_doc::{Document, Shape};
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, open_drawing, read_back};
use crate::odg_file::{assert_saves_again_as_no_edit, save};
use crate::run;

const PAGE_H: f64 = 29.7 / 2.54;

fn shape(doc: &Document, id: u64) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|x| u64::from(x.id) == id).unwrap()
}

fn n(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

fn attr(element: &str, name: &str) -> Option<String> {
    let at = element.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(element[at..at + element[at..].find('"')?].to_owned())
}

fn inches(text: &str) -> f64 {
    let (n, unit) = text.split_at(text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len()));
    let n: f64 = n.trim().parse().unwrap_or_else(|_| panic!("a length: {text}"));
    n / match unit {
        "cm" => 2.54,
        "mm" => 25.4,
        "in" => 1.0,
        other => panic!("a unit the writer writes: {other}"),
    }
}

/// The start tags of the file's elements named `tag`, in order.
fn elements<'a>(content: &'a str, tag: &str) -> Vec<&'a str> {
    content.match_indices(&format!("<{tag} ")).map(|(at, _)| &content[at..at + content[at..].find('>').expect("a tag's end")]).collect()
}

/// The whole of the custom shape element whose `draw:id` is `id`.
fn custom_shape<'a>(content: &'a str, id: &str) -> &'a str {
    let start = content.find(&format!("draw:id=\"{id}\"")).and_then(|at| content[..at].rfind("<draw:custom-shape ")).expect("the shape");
    &content[start..start + content[start..].find("</draw:custom-shape>").expect("its end")]
}

/// The frame of a custom shape's element: its size, the turn of its `draw:transform`, and its map (y down) from a frame point to the page; its mirrors.
struct Written {
    w: f64,
    h: f64,
    turn: f64,
    t: (f64, f64),
    mirror: (bool, bool),
}

fn written(element: &str) -> Written {
    let (w, h) = (inches(&attr(element, "svg:width").expect("a width")), inches(&attr(element, "svg:height").expect("a height")));
    let (turn, t) = match attr(element, "draw:transform") {
        Some(tr) => {
            let rotate: f64 =
                tr.split("rotate (").nth(1).and_then(|r| r.split(')').next()).and_then(|a| a.trim().parse().ok()).expect("a rotate term");
            let translate = tr.split("translate (").nth(1).and_then(|r| r.split(')').next()).expect("a translate term");
            let mut parts = translate.split_whitespace();
            (rotate, (inches(parts.next().expect("an x")), inches(parts.next().expect("a y"))))
        }
        None => (0.0, (inches(&attr(element, "svg:x").expect("an x")), inches(&attr(element, "svg:y").expect("a y")))),
    };
    let mirror = (element.contains(r#"draw:mirror-horizontal="true""#), element.contains(r#"draw:mirror-vertical="true""#));
    Written { w, h, turn, t, mirror }
}

impl Written {
    /// LibreOffice's picture of the frame point (`u`, `v`) (y down from the frame's top left): the transform, then each mirror across the page's line through the centre
    /// A custom shape's own glue point (`draw:glue-points`) is drawn so, with its geometry.
    fn picture(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.turn.sin_cos();
        let at = |u: f64, v: f64| (self.t.0 + u * c + v * s, self.t.1 - u * s + v * c);
        let centre = at(self.w / 2.0, self.h / 2.0);
        let (x, y) = at(u, v);
        (if self.mirror.0 { 2.0 * centre.0 - x } else { x }, if self.mirror.1 { 2.0 * centre.1 - y } else { y })
    }
}

/// A shape of the master `master` dropped with its centre at (3, 8) inches, 1 by 0.5, flipped as `flips` says (`h`, `v`, in that order) and turned `angle` degrees: its `ID`.
fn dropped(s: &mut Session, master: &str, flips: &str, angle: f64) -> u64 {
    let t = run(s, "shape.drop", json!({ "master": master, "page": 0, "x": 3.0, "y": 8.0 }))["id"].as_u64().unwrap();
    run(s, "shape.resize", json!({ "id": t, "x": 2.5, "y": 7.75, "width": 1.0, "height": 0.5 }));
    for flip in flips.chars() {
        let axis = if flip == 'h' { "horizontal" } else { "vertical" };
        run(s, "shape.flip", json!({ "ids": [t], "axis": axis }));
    }
    if angle != 0.0 {
        run(s, "shape.rotate", json!({ "ids": [t], "angle": angle }));
    }
    t
}

const FLIPS: [&str; 4] = ["", "h", "v", "hv"];

/// Bug 4: straight connectors from four small squares to the connection points 0 to 3 of a new right triangle, in each flip state and turned 0, 30 and 135 degrees, end in
/// LibreOffice where they end in the session: the triangle's points are written as its geometry's own glue points, its style says so, and each connector names the point
/// four past its row. The file reads back with the connectors glued to the same rows, and a second save is no edit.
#[test]
fn gluef2_a_new_shapes_own_connection_points_are_written_as_its_glue_points_and_connectors_end_on_them() {
    for flips in FLIPS {
        for angle in [0.0, 30.0, 135.0] {
            let what = format!("a right triangle flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            let t = dropped(&mut s, "right_triangle", flips, angle);
            let mut connectors = Vec::new();
            for k in 0..4u32 {
                let x = 1.5 + f64::from(k);
                let sq = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": x, "y": 6.0 }))["id"].as_u64().unwrap();
                run(&mut s, "shape.resize", json!({ "id": sq, "x": x - 0.05, "y": 5.95, "width": 0.1, "height": 0.1 }));
                let c = run(&mut s, "connector.add", json!({ "from": { "id": sq }, "to": { "id": t, "point": k } }))["id"].as_u64().unwrap();
                run(&mut s, "connector.style", json!({ "style": "straight", "ids": [c] }));
                connectors.push(c);
            }
            let saved = save(&mut s);
            assert_eq!(saved["warning_count"], 0, "{what}: {saved}");
            let content = content_of(&dir.read("a.odg"));
            let session = s.document().unwrap().clone();
            let target = attr(elements(&content, "draw:connector").first().unwrap(), "draw:end-shape").unwrap();
            let tri = custom_shape(&content, &target).to_owned();
            let geometry = &tri[tri.find("<draw:enhanced-geometry ").expect("a geometry")..];
            let listed = attr(geometry, "draw:glue-points").unwrap_or_else(|| panic!("{what}: the points are listed on the geometry: {geometry}"));
            let numbers: Vec<f64> = listed.split_whitespace().map(|v| v.parse().unwrap()).collect();
            assert_eq!(numbers.len(), 8, "{what}: four points: {listed}");
            let frame = written(&tri);
            for (k, element) in elements(&content, "draw:connector").into_iter().enumerate() {
                let id: usize = attr(element, "draw:end-glue-point").expect("glued to a point").parse().unwrap();
                assert_eq!(id, k + 4, "{what}: connector {k} names the point four past its row");
                let (vx, vy) = (numbers[2 * (id - 4)], numbers[2 * (id - 4) + 1]);
                let lo = frame.picture((vx / 21600.0 * frame.w, vy / 21600.0 * frame.h));
                let c = shape(&session, connectors[k]);
                let end = (n(c, "EndX"), PAGE_H - n(c, "EndY"));
                let off = (end.0 - lo.0).hypot(end.1 - lo.1);
                assert!(
                    off < 2e-3,
                    "{what}: connector {k} ends at {end:?} in the session, and LibreOffice ends it at point {id}, {lo:?} ({off:.3} inch off)"
                );
            }
            // The style of the triangle says its rows start at 0 with its first listed point.
            let style = attr(&tri, "draw:style-name").unwrap();
            let style_at = content.find(&format!("style:name=\"{style}\"")).expect("its style");
            let style_text = &content[style_at..style_at + content[style_at..].find("</style:style>").expect("the style's end")];
            assert!(style_text.contains(r#"diacraft:glue="own""#), "{what}: {style_text}");
            // Read back, each connector is glued to the same row as in the session, and the triangle has the session's rows.
            let (doc, report) = read_back(&dir.read("a.odg"));
            assert_eq!(report.warned("glue_point_id"), 0, "{what}");
            for c in &connectors {
                let had = session
                    .pages
                    .get(0)
                    .unwrap()
                    .connects
                    .iter()
                    .find(|k| u64::from(k.from_sheet) == *c && k.from_cell.as_deref() == Some("EndX"))
                    .unwrap();
                let got = doc
                    .pages
                    .get(0)
                    .unwrap()
                    .connects
                    .iter()
                    .find(|k| u64::from(k.from_sheet) == *c && k.from_cell.as_deref() == Some("EndX"))
                    .unwrap();
                assert_eq!(got.to_part, had.to_part, "{what}: connector {c}");
            }
            // The triangle reads back with its own four rows and no standard ones beside them (the session's rows are its master's, which a read of the file holds on
            // the shape itself).
            let section = shape(&doc, t).sheet.sections.iter().find(|x| x.name.as_str() == "Connection").cloned().expect("rows");
            assert_eq!(section.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(0), Some(1), Some(2), Some(3)], "{what}: the rows come back");
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// Bug 3 on the writer's side: a shape whose `draw:transform` is a `matrix` with a translation of plain numbers (hundredths of a millimetre, as LibreOffice and now the
/// reader read it) moved 1 inch right has the translation written in the same form, and reads back where the session has it. On main the reader dropped the transform, so
/// the move wrote another place beside it and left the transform as it was.
#[test]
fn gluef2_a_move_of_a_shape_whose_translation_has_no_unit_writes_it_in_that_form() {
    let shapes = r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="M" svg:width="2cm" svg:height="1cm" draw:transform="matrix (1 0 0 1 2000 3000)"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#;
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(shapes));
    let id = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let before = (n(shape(s.document().unwrap(), u64::from(id)), "PinX"), n(shape(s.document().unwrap(), u64::from(id)), "PinY"));
    assert!((before.0 - 3.0 / 2.54).abs() < 1e-9 && (PAGE_H - before.1 - 3.5 / 2.54).abs() < 1e-9, "read where LibreOffice draws it: {before:?}");
    run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.0 }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    let element = elements(&content, "draw:custom-shape").into_iter().find(|e| e.contains(r#"draw:name="M""#)).unwrap();
    assert_eq!(attr(element, "draw:transform").as_deref(), Some("matrix (1 0 0 1 4540 3000)"), "{element}");
    assert!(attr(element, "svg:x").is_none(), "no place beside the transform: {element}");
    let (doc, _) = read_back(&dir.read("a.odg"));
    let m = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert!((n(m, "PinX") - (before.0 + 1.0)).abs() < 1e-9, "{}", n(m, "PinX"));
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The first concern: LibreOffice places a `draw:glue-point` in the page's axes from the box the shape takes up on the page, and a polygon's standard points
/// on the box of its outline, so after a turn the file does not say (and which the writer cannot write the inverse of yet) the points are elsewhere than the model's rows,
/// which turn with the shape. A turn of such a shape of the file **that a connector is glued to at such a point** is refused in words; a turn of a shape glued only at the
/// standard points of a custom shape, and a resize, are saved. (A flip of a shape of the file is refused already.)
#[test]
fn gluef2_a_turn_of_a_file_shape_glued_at_a_point_libreoffice_places_on_its_box_is_refused_in_words() {
    let custom = |point: u32| {
        format!(
            r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="T" xml:id="T" draw:id="T" svg:width="3cm" svg:height="1.5cm" svg:x="5cm" svg:y="5cm"><draw:glue-point draw:id="5" svg:x="-3cm" svg:y="2cm" draw:escape-direction="auto"/><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape><draw:connector draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="C" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="1cm" svg:y2="1cm" draw:end-shape="T" draw:end-glue-point="{point}"><text:p/></draw:connector>"#
        )
    };
    let polygon = |point: u32| {
        format!(
            r#"<draw:polygon draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="T" xml:id="T" draw:id="T" svg:width="3cm" svg:height="1.5cm" svg:x="5cm" svg:y="5cm" svg:viewBox="0 0 3000 1500" draw:points="0,0 0,1500 3000,1500"><text:p/></draw:polygon><draw:connector draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="C" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="1cm" svg:y2="1cm" draw:end-shape="T" draw:end-glue-point="{point}"><text:p/></draw:connector>"#
        )
    };
    let id_of = |s: &Session| s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.name.as_deref() == Some("T")).unwrap().id;
    for (what, shapes, refused) in [
        ("a custom shape glued at its draw:glue-point", custom(5), true),
        ("a custom shape glued at a standard point", custom(1), false),
        ("a polygon glued at a standard point", polygon(1), true),
    ] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &Drawing::new().page(&shapes));
        let t = id_of(&s);
        run(&mut s, "shape.rotate", json!({ "ids": [t], "angle": 30 }));
        let result = s.execute("file.save", &json!({}));
        if refused {
            let msg = match result {
                Err(diacraft_engine::EngineError::Failed { msg, .. }) => msg,
                other => panic!("{what}: the save was not refused: {other:?}"),
            };
            assert!(
                msg.contains("a turn of a shape of the file whose connectors are glued to a glue point that LibreOffice places on the box"),
                "{what}: {msg}"
            );
        } else {
            assert!(result.is_ok(), "{what}: {result:?}");
        }
    }
    // A resize of the custom shape glued at its draw:glue-point is saved: the row is a formula of the size, as LibreOffice's point is a share of the box.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&custom(5)));
    let t = id_of(&s);
    run(&mut s, "shape.resize", json!({ "id": t, "x": 5.0 / 2.54, "y": PAGE_H - 6.5 / 2.54, "width": 4.0 / 2.54, "height": 1.5 / 2.54 }));
    assert!(s.execute("file.save", &json!({})).is_ok());
}
