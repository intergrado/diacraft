//! New flipped shapes saved to an `.odg` through the commands, against what LibreOffice 26.8.2.2 draws of the file (a black box: hand-written flat drawings
//! converted to SVG with its own profile; the rules are in the odg crate's `read_flipglue.rs`).
//!
//! - **A new connector that crosses another** has line jumps in the session (an `ArcTo` hop), which the writer refused ("a new connector whose route is not a path the writer
//!   writes"): connectors glued to a flipped triangle cross often, so 10 of 12 such drawings were refused. The file's path has no place for a hop, and LibreOffice draws
//!   none: the path is now the route's corners, and the session's rows come back from DiaCraft's own data.
//! - **Its end** names the glue point LibreOffice puts where the session has the end: beside exactly one flip LibreOffice's points 1 and 3 are the frame's right and left
//!   as drawn, which are the model's rows 1 and 3, not 3 and 1.
//! - **The text of a new shape with `FlipY`** is written with its own half turn (`draw:text-rotate-angle="180"`), which LibreOffice adds to the half turn it gives the
//!   text of a vertically mirrored shape, so it reads as the session's.

use diacraft_doc::{Document, Shape};
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, open_drawing};
use crate::odg_file::{assert_saves_again_as_no_edit, save};
use crate::run;

const PAGE_H: f64 = 29.7 / 2.54;

/// A right triangle dropped with its centre at (3, 8) inches, 1 by 0.5, flipped as `flips` says (`h`, `v`, in that order) and turned `angle` degrees: its `ID`.
fn triangle(s: &mut Session, flips: &str, angle: f64) -> u64 {
    dropped(s, "right_triangle", flips, angle)
}

/// A shape of the master `master` dropped and made as [`triangle`] makes one.
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
    /// LibreOffice's picture of the frame point (`u`, `v`) (y down from the frame's top left): the transform, then each mirror across the page's line through the centre.
    fn picture(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.turn.sin_cos();
        let at = |u: f64, v: f64| (self.t.0 + u * c + v * s, self.t.1 - u * s + v * c);
        let centre = at(self.w / 2.0, self.h / 2.0);
        let (x, y) = at(u, v);
        (if self.mirror.0 { 2.0 * centre.0 - x } else { x }, if self.mirror.1 { 2.0 * centre.1 - y } else { y })
    }

    /// Where LibreOffice ends a connector at standard point `id` (0 top, 1 right, 2 bottom, 3 left of the frame as drawn): the picture of that side's middle, the right and
    /// the left exchanged when the picture is mirrored once.
    fn standard_point(&self, id: u32) -> (f64, f64) {
        let id = if self.mirror.0 != self.mirror.1 && (id == 1 || id == 3) { 4 - id } else { id };
        let side = [(self.w / 2.0, 0.0), (self.w, self.h / 2.0), (self.w / 2.0, self.h), (0.0, self.h / 2.0)][id as usize];
        self.picture(side)
    }

    /// The turn LibreOffice gives the shape's text, counterclockwise, degrees: the frame's as drawn, half a turn more for a vertical mirror, and the text's own.
    fn text_turn(&self, own: f64) -> f64 {
        let drawn = if self.mirror.0 != self.mirror.1 { -self.turn } else { self.turn }.to_degrees();
        (drawn + if self.mirror.1 { 180.0 } else { 0.0 } + own).rem_euclid(360.0)
    }
}

#[track_caller]
fn same_turn(got: f64, want: f64, what: &str) {
    let d = (got - want).rem_euclid(360.0);
    assert!(d.min(360.0 - d) < 1e-6, "{what}: {got} degrees against {want}");
}

const FLIPS: [&str; 4] = ["", "h", "v", "hv"];

/// Bug 5 (and the glue of bug 2 on the writer's side): four straight connectors from four small squares to points 0 to 3 of a new triangle, and of a new rectangle (whose
/// points are the four side middles), flipped and turned; where they cross the session gives them line jumps. **Every save is made** (on main 10 of the 12 triangles were
/// refused), each connector's path in the file is lines only, a connector to the rectangle names the glue point LibreOffice draws where the session has the end, the drawing
/// reopens with the session's routes, and a second save is no edit. (The right triangle's own points are not the four side middles, which the writer does not write: the
/// ends of its connectors are left to the rows' own task.)
#[test]
fn flipglue_new_connectors_glued_to_a_new_flipped_triangle_save_and_end_where_libreoffice_ends_them() {
    let mut hops = 0;
    for (master, flips, angle) in
        ["right_triangle", "rectangle"].into_iter().flat_map(|m| FLIPS.into_iter().flat_map(move |f| [0.0, 30.0, 135.0].map(|a| (m, f, a))))
    {
        {
            let what = format!("a {master} flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            let t = dropped(&mut s, master, flips, angle);
            let mut connectors = Vec::new();
            for k in 0..4u32 {
                let x = 2.0 + f64::from(k);
                let sq = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": x, "y": 6.0 }))["id"].as_u64().unwrap();
                run(&mut s, "shape.resize", json!({ "id": sq, "x": x - 0.05, "y": 5.95, "width": 0.1, "height": 0.1 }));
                let c = run(&mut s, "connector.add", json!({ "from": { "id": sq }, "to": { "id": t, "point": k } }))["id"].as_u64().unwrap();
                run(&mut s, "connector.style", json!({ "style": "straight", "ids": [c] }));
                connectors.push(c);
            }
            let rows = |doc: &Document, c: u64| -> Vec<String> {
                shape(doc, c)
                    .sheet
                    .sections
                    .iter()
                    .filter(|x| x.name.as_str() == "Geometry")
                    .flat_map(|x| x.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()))
                    .collect()
            };
            let session = s.document().unwrap().clone();
            hops += connectors.iter().filter(|c| rows(&session, **c).iter().any(|r| r == "ArcTo")).count();
            let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save is made: {e:?}"));
            assert_eq!(saved["edited"], true, "{what}");
            let content = content_of(&dir.read("a.odg"));
            let tri = custom_shape(&content, &attr(elements(&content, "draw:connector").first().unwrap(), "draw:end-shape").unwrap()).to_owned();
            let frame = written(&tri);
            for (k, element) in elements(&content, "draw:connector").into_iter().enumerate() {
                let path = attr(element, "svg:d").expect("a path");
                assert!(path.chars().all(|ch| "Mhvl -0123456789".contains(ch)), "{what}: connector {k}'s path is lines: {path}");
                if master != "rectangle" {
                    continue;
                }
                let id: u32 = attr(element, "draw:end-glue-point").expect("glued to a point").parse().unwrap();
                let c = shape(&session, connectors[k]);
                let end = (n(c, "EndX"), PAGE_H - n(c, "EndY"));
                let lo = frame.standard_point(id);
                let off = (end.0 - lo.0).hypot(end.1 - lo.1);
                assert!(
                    off < 2e-3,
                    "{what}: connector {k} ends at {end:?} in the session, and LibreOffice ends it at point {id}, {lo:?} ({off:.3} inch off)"
                );
            }
            // The routes come back as the session had them (the hops from DiaCraft's own data), and saving again is no edit.
            let mut again = Session::new();
            run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
            for c in &connectors {
                assert_eq!(rows(again.document().unwrap(), *c), rows(&session, *c), "{what}: connector {c}'s rows");
            }
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
    assert!(hops >= 10, "the matrix has connectors with line jumps: {hops}");
}

/// Bug 4 on the writer's side: a new shape with `FlipY` and a text has the text's own half turn in the file, so LibreOffice turns the text as the session does (it gave it
/// half a turn more), and it reopens with no `TxtAngle`. A shape with no `FlipY` has none.
#[test]
fn flipglue_the_text_of_a_new_vertically_flipped_shape_is_written_to_read_as_the_sessions() {
    for flips in FLIPS {
        for angle in [0.0, 30.0, 135.0] {
            let what = format!("flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            let t = triangle(&mut s, flips, angle);
            run(&mut s, "shape.set_text", json!({ "id": t, "text": "Ab" }));
            let saved = save(&mut s);
            assert_eq!(saved["warning_count"], 0, "{what}: {saved}");
            let content = content_of(&dir.read("a.odg"));
            let element = content[content.rfind("<draw:custom-shape ").unwrap()..].to_owned();
            let own = attr(&element, "draw:text-rotate-angle");
            assert_eq!(own.as_deref(), flips.contains('v').then_some("180"), "{what}: {element}");
            let session = shape(s.document().unwrap(), t);
            let flipped = usize::from(n(session, "FlipX") == 1.0) + usize::from(n(session, "FlipY") == 1.0);
            let (a, txt) = (n(session, "Angle").to_degrees(), n(session, "TxtAngle").to_degrees());
            let net = if flipped == 1 { a - txt } else { a + txt };
            same_turn(written(&element).text_turn(own.map_or(0.0, |o| o.parse().unwrap())), net, &what);
            let mut again = Session::new();
            run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
            assert!(shape(again.document().unwrap(), t).sheet.cell("TxtAngle").is_none(), "{what}: no TxtAngle read back");
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// Bug 2 on the writer's side, alone: a new connector from a square on the right to point 3 (the model's right side's middle) and one from a square on the left to point 1 of
/// a new rectangle mirrored once, turned or not, end where LibreOffice ends them. The writer named the other side (point 1 for row 3): LibreOffice drew each across the
/// rectangle, its width off.
#[test]
fn flipglue_a_new_connector_to_a_side_of_a_new_shape_flipped_once_names_the_point_libreoffice_puts_there() {
    for flips in ["h", "v", "", "hv"] {
        for angle in [0.0, 30.0] {
            let what = format!("flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            let t = dropped(&mut s, "rectangle", flips, angle);
            let mut connectors = Vec::new();
            for (x, point) in [(5.0, 3u32), (1.0, 1)] {
                let sq = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": x, "y": 8.0 }))["id"].as_u64().unwrap();
                run(&mut s, "shape.resize", json!({ "id": sq, "x": x - 0.05, "y": 7.95, "width": 0.1, "height": 0.1 }));
                let c = run(&mut s, "connector.add", json!({ "from": { "id": sq }, "to": { "id": t, "point": point } }))["id"].as_u64().unwrap();
                run(&mut s, "connector.style", json!({ "style": "straight", "ids": [c] }));
                connectors.push(c);
            }
            save(&mut s);
            let content = content_of(&dir.read("a.odg"));
            let frame = written(custom_shape(&content, &attr(elements(&content, "draw:connector").first().unwrap(), "draw:end-shape").unwrap()));
            let session = s.document().unwrap();
            for (k, element) in elements(&content, "draw:connector").into_iter().enumerate() {
                let id: u32 = attr(element, "draw:end-glue-point").expect("glued to a point").parse().unwrap();
                let c = shape(session, connectors[k]);
                let end = (n(c, "EndX"), PAGE_H - n(c, "EndY"));
                let lo = frame.standard_point(id);
                let off = (end.0 - lo.0).hypot(end.1 - lo.1);
                assert!(
                    off < 2e-3,
                    "{what}: connector {k} ends at {end:?} in the session, and LibreOffice ends it at point {id}, {lo:?} ({off:.3} inch off)"
                );
            }
        }
    }
}
