//! A connector end glued to a glue point of a shape of the file follows that point when the shape is moved, resized or turned, whatever kind of
//! glue point it is: a standard one, a custom shape's own, or a `draw:glue-point` child of a frame, a custom shape, another draw shape or a group.
//!
//! - **The regression** (found by the full gate's sample-file check: 16 ends in 3 files left 0.25 inch behind a 0.25 inch move): made a frame's
//!   `draw:glue-point` no row and an end naming it whole-shape glue (LibreOffice 26.8.2.2 gives a frame no glue point of its own), and nothing placed that end when the
//!   frame moved. A frame's points are rows again, placed where LibreOffice places them (in the page's axes, from the box the frame takes up: as a custom shape's,
//!   measured by this task; the frames had their points before the text box or the picture, which LibreOffice drops), and the end is glued to its row, which a move
//!   and a resize carry; a turn is refused at save in words, as for a custom shape.
//! - **Whole-shape glue of an `.odg` connector**: the reader gives such an end plain values for the engine to place, and marks every connector
//!   it reads as one DiaCraft may route again, which the engine took for a connector of its own, glued by formulas, and placed nothing: the end stayed where the file had it.
//!
//! Each drawing is written by hand from the OASIS ODF standard (`odg_build.rs`); the oracle is the place of the glue point on the page, worked out here from the markup.

use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_bytes_in, open_drawing};
use crate::odg_file::{refused, save};
use crate::run;

const PAGE_H: f64 = 29.7 / 2.54;

/// The words of the refusal of a turn of a shape of the file glued at a point LibreOffice places on the box the shape takes up (`odg` crate, `write/compare.rs`).
const WORDS_TURNED_GLUE: &str =
    "a turn of a shape of the file whose connectors are glued to a glue point that LibreOffice places on the box the shape takes up on the page";

/// A length of the file (`cm`, `mm`, `in`) in inches.
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

fn cm(v: f64) -> f64 {
    v / 2.54
}

fn attr(element: &str, name: &str) -> Option<String> {
    let at = element.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(element[at..at + element[at..].find('"')?].to_owned())
}

/// The start tags of the file's elements named `tag`, in order.
fn elements<'a>(content: &'a str, tag: &str) -> Vec<&'a str> {
    content.match_indices(&format!("<{tag} ")).map(|(at, _)| &content[at..at + content[at..].find('>').expect("a tag's end")]).collect()
}

/// The connector called `name`'s start tag.
fn connector_tag<'a>(content: &'a str, name: &str) -> &'a str {
    elements(content, "draw:connector")
        .into_iter()
        .find(|e| attr(e, "draw:name").as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no connector {name} in {content}"))
}

/// The stated end (`svg:x2`, `svg:y2`, inches from the page's top left) of the connector called `name`.
fn stated_end(content: &str, name: &str) -> (f64, f64) {
    let c = connector_tag(content, name);
    (inches(&attr(c, "svg:x2").expect("an x2")), inches(&attr(c, "svg:y2").expect("a y2")))
}

/// The end of the connector called `name` in the session's model, inches from the page's top left.
fn model_end(s: &Session, name: &str) -> (f64, f64) {
    let doc = s.document().unwrap();
    let id = id_of(doc, 0, name);
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap();
    let n = |cell: &str| shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    (n("EndX"), PAGE_H - n("EndY"))
}

#[track_caller]
fn near(got: (f64, f64), want: (f64, f64), what: &str) {
    let off = (got.0 - want.0).hypot(got.1 - want.1);
    assert!(off < 1.0e-4, "{what}: at ({:.5}, {:.5}) inches, want ({:.5}, {:.5}) ({off:.5} inch off)", got.0, got.1, want.0, want.1);
}

/// A connector called `name` from the free place `from` to glue point `point` of the shape `to` (none: the whole shape), its stated end at `end` (centimetres from the
/// page's top left), drawn as a straight line.
fn connector(name: &str, to: &str, point: Option<u32>, from: (f64, f64), end: (f64, f64)) -> String {
    let glue = point.map(|p| format!(" draw:end-glue-point=\"{p}\"")).unwrap_or_default();
    format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" draw:type=\"line\" draw:end-shape=\"{to}\"{glue} svg:x1=\"{}cm\" svg:y1=\"{}cm\" svg:x2=\"{}cm\" svg:y2=\"{}cm\" svg:d=\"M {} {} L {} {}\" svg:viewBox=\"0 0 {} {}\"><text:p/></draw:connector>",
        from.0,
        from.1,
        end.0,
        end.1,
        from.0 * 1000.0,
        from.1 * 1000.0,
        end.0 * 1000.0,
        end.1 * 1000.0,
        from.0.max(end.0) * 1000.0 + 1.0,
        from.1.max(end.1) * 1000.0 + 1.0
    )
}

/// A one-pixel picture, for a picture frame.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGNgAAAAAgABSK+kcQAAAABJRU5ErkJggg==";

/// The glue point of every shape below: id 4, a quarter of the box's width right of its centre on its top side, in LibreOffice's own form (2.5 cm and -5 cm are 25 % and
/// -50 % of the box, from its centre).
const POINT: &str = r#"<draw:glue-point draw:id="4" svg:x="2.5cm" svg:y="-5cm" draw:escape-direction="auto"/>"#;

/// Where [`POINT`] is on a shape whose frame is 4 cm by 2 cm at (4 cm, 4 cm), unturned: (7 cm, 4 cm).
const AT: (f64, f64) = (7.0, 4.0);

/// The kinds of shape that hold `draw:glue-point` children, by the ODF standard's content of each element (Part 3, 10.3 and 10.4): frames (a text box, a picture),
/// rectangles, ellipses, circles, polygons, polylines, paths, regular polygons, custom shapes and groups.
const KINDS: [&str; 11] =
    ["text frame", "picture frame", "rectangle", "ellipse", "circle", "polygon", "polyline", "path", "regular polygon", "custom shape", "group"];

/// A shape of `kind` called `S`, with `xml:id` and `draw:id` `s1`, its frame 4 cm by 2 cm at (4 cm, 4 cm) (a group's is the box of its one member, so), holding the glue
/// points `points`, and `transform` in place of `svg:x` and `svg:y` when it is given. Each child is where the standard puts it: a glue point before a shape's text, after a
/// frame's content, before a group's members.
fn shape_of(kind: &str, points: &str, transform: Option<&str>) -> String {
    let place = match transform {
        Some(t) => format!(r#"draw:transform="{t}""#),
        None => r#"svg:x="4cm" svg:y="4cm""#.to_owned(),
    };
    let head = format!(
        r#"draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="S" xml:id="s1" draw:id="s1" svg:width="4cm" svg:height="2cm" {place}"#
    );
    match kind {
        "text frame" => format!("<draw:frame {head}><draw:text-box><text:p>T</text:p></draw:text-box>{points}</draw:frame>"),
        "picture frame" => {
            format!("<draw:frame {head}><draw:image><office:binary-data>{PNG}</office:binary-data></draw:image>{points}</draw:frame>")
        }
        "rectangle" => format!("<draw:rect {head}>{points}<text:p/></draw:rect>"),
        "ellipse" => format!("<draw:ellipse {head}>{points}<text:p/></draw:ellipse>"),
        "circle" => format!("<draw:circle {head}>{points}<text:p/></draw:circle>"),
        "polygon" => {
            format!(r#"<draw:polygon {head} svg:viewBox="0 0 4000 2000" draw:points="0,0 4000,0 4000,2000 0,2000">{points}<text:p/></draw:polygon>"#)
        }
        "polyline" => format!(
            r#"<draw:polyline {head} svg:viewBox="0 0 4000 2000" draw:points="0,0 4000,0 4000,2000 0,2000 0,0">{points}<text:p/></draw:polyline>"#
        ),
        "path" => format!(r#"<draw:path {head} svg:viewBox="0 0 4000 2000" svg:d="M 0 0 L 4000 0 4000 2000 0 2000 Z">{points}<text:p/></draw:path>"#),
        "regular polygon" => format!(r#"<draw:regular-polygon {head} draw:corners="4">{points}<text:p/></draw:regular-polygon>"#),
        "custom shape" => format!(
            r#"<draw:custom-shape {head}>{points}<text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        ),
        "group" => {
            assert!(transform.is_none(), "a group's frame is its members' box");
            format!(
                r#"<draw:g draw:name="S" xml:id="s1" draw:id="s1">{points}<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="M" svg:width="4cm" svg:height="2cm" svg:x="4cm" svg:y="4cm"><text:p/></draw:rect></draw:g>"#
            )
        }
        other => panic!("no kind {other}"),
    }
}

/// A drawing of a shape of `kind` with [`POINT`] and the connector `C` glued to it from (1 cm, 1 cm), its stated end on the point.
fn drawing(kind: &str) -> Drawing {
    Drawing::new().page(&format!("{}{}", shape_of(kind, POINT, None), connector("C", "s1", Some(4), (1.0, 1.0), AT)))
}

/// The saved file opened in a new session, the shape moved right and back so that the engine works the end out from the file's own glue: it is where the file states it.
#[track_caller]
fn reads_back_on_the_point(dir: &Dir, what: &str) {
    let bytes = dir.read("a.odg");
    let stated = stated_end(&content_of(&bytes), "C");
    let again = Dir::new();
    let mut s = open_bytes_in(&again, "a.odg", &bytes);
    let doc = s.document().unwrap();
    let connect = doc.pages.get(0).unwrap().connects.iter().find(|c| c.from_cell.as_deref() == Some("EndX")).cloned().expect("the end is glued");
    assert_eq!(connect.to_part, Some(104), "{what}: read back, the end is glued to the shape's point 4: {connect:?}");
    let id = id_of(doc, 0, "S");
    run(&mut s, "shape.move", json!({ "ids": [id], "dx": 0.1, "dy": 0.0 }));
    run(&mut s, "shape.move", json!({ "ids": [id], "dx": -0.1, "dy": 0.0 }));
    near(model_end(&s, "C"), stated, &format!("{what}: read back and moved right and back, the end is where the file states it"));
}

/// The reproducer: a move of a text frame by 0.25 inch moves the stated end of a connector glued to its `draw:glue-point` child by 0.25 inch, and leaves its other
/// coordinate where it was (on main the end stayed where it was, 0.25 inch from the moved point).
#[test]
fn frameglue_a_move_of_a_frame_moves_the_end_glued_to_its_own_glue_point() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing("text frame"));
    let before = stated_end(&content_of(&dir.read("a.odg")), "C");
    near(before, (cm(AT.0), cm(AT.1)), "the file states the end on the point");
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let after = stated_end(&content_of(&dir.read("a.odg")), "C");
    near(after, (before.0 + 0.25, before.1), "the end follows the frame's glue point by the move");
    reads_back_on_the_point(&dir, "a text frame moved");
}

/// Every kind of shape that holds `draw:glue-point` children: a move by 0.25 inch moves the end glued to the child by the move.
#[test]
fn frameglue_a_move_of_any_shape_moves_the_end_glued_to_its_glue_point_child() {
    for kind in KINDS {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing(kind));
        let doc = s.document().unwrap();
        let id = id_of(doc, 0, "S");
        let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap();
        let rows: Vec<Option<u32>> =
            shape.sheet.sections.iter().filter(|x| x.name.as_str() == "Connection").flat_map(|x| x.rows.iter().map(|r| r.ix)).collect();
        assert!(rows.contains(&Some(4)), "a {kind}: its glue point is a row: {rows:?}");
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 0.25, "dy": 0.0 }));
        save(&mut s);
        let after = stated_end(&content_of(&dir.read("a.odg")), "C");
        near(after, (cm(AT.0) + 0.25, cm(AT.1)), &format!("a {kind} moved 0.25 inch: the end follows its glue point"));
        reads_back_on_the_point(&dir, &format!("a {kind} moved"));
    }
}

/// Every kind of shape that holds `draw:glue-point` children, resized from 4 cm by 2 cm to 6 cm by 3 cm about its bottom left corner: the end glued to the child goes
/// to the point's share of the new box, (8.5 cm, 3 cm).
#[test]
fn frameglue_a_resize_of_any_shape_takes_the_end_glued_to_its_glue_point_child_to_the_points_new_place() {
    for kind in KINDS {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing(kind));
        let id = id_of(s.document().unwrap(), 0, "S");
        run(&mut s, "shape.resize", json!({ "id": id, "x": cm(4.0), "y": PAGE_H - cm(6.0), "width": cm(6.0), "height": cm(3.0) }));
        let saved = save(&mut s);
        let after = stated_end(&content_of(&dir.read("a.odg")), "C");
        near(after, (cm(8.5), cm(3.0)), &format!("a {kind} resized: the end follows its glue point ({saved})"));
        reads_back_on_the_point(&dir, &format!("a {kind} resized"));
    }
}

/// A text frame and a picture frame turned to 30 degrees about their centre: in the session the end turns with the row of the frame's `draw:glue-point`. LibreOffice
/// places a frame's `draw:glue-point` in the page's axes from the box the frame takes up on the page, as it places a custom shape's (measured by this task: text and picture
/// frames turned 0 and 30 degrees, every end within 0.001 cm), so a turn the file does not say would put the point elsewhere than the row: the save is refused in words, as
/// it is for a custom shape, and a turn of a frame glued only at a standard point is saved.
#[test]
fn frameglue_a_turn_of_a_frame_glued_at_its_glue_point_turns_the_end_and_is_refused_at_save_in_words() {
    for kind in ["text frame", "picture frame"] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing(kind));
        run(&mut s, "shape.rotate", json!({ "ids": [1], "angle": 30.0 }));
        // The point is 1 cm right of the centre (6 cm, 5 cm) and 1 cm above it; turned 30 degrees counterclockwise on the page with the frame.
        let (sn, cs) = 30f64.to_radians().sin_cos();
        near(
            model_end(&s, "C"),
            (cm(6.0 + (cs - sn)), cm(5.0 - (sn + cs))),
            &format!("a {kind} turned 30 degrees: the end turns with its glue point"),
        );
        let said = refused(&mut s);
        assert!(said.contains(WORDS_TURNED_GLUE), "a {kind}: the turn is refused in words: {said}");
        // Glued at a standard point only, the turn is saved.
        let dir = Dir::new();
        let points = Drawing::new().page(&format!("{}{}", shape_of(kind, POINT, None), connector("C", "s1", Some(0), (1.0, 1.0), (6.0, 4.0))));
        let mut s = open_drawing(&dir, &points);
        run(&mut s, "shape.rotate", json!({ "ids": [1], "angle": 30.0 }));
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "a {kind} glued at a standard point: the turn is saved: {saved}");
    }
}

/// A text frame the file turns 30 degrees, moved: the end glued to its `draw:glue-point` is where LibreOffice puts the point (in the page's axes, at the point's shares of the
/// box the turned frame takes up on the page), and follows it by the move.
#[test]
fn frameglue_a_move_of_a_turned_frame_moves_the_end_glued_to_its_glue_point_by_the_move() {
    // `rotate (a)` then the translation that puts the centre of the 4 by 2 frame at (6 cm, 5 cm): the frame's corner (0, 0) goes to the centre less the turned half
    // diagonal.
    let a = 30f64.to_radians();
    let (sn, cs) = a.sin_cos();
    let (hx, hy) = (2.0 * cs + 1.0 * sn, -2.0 * sn + 1.0 * cs);
    let transform = format!("rotate ({a:?}) translate ({}cm {}cm)", 6.0 - hx, 5.0 - hy);
    // The point, 25 % right of the centre of the turned frame's box and on its top side (-50 %): LibreOffice's place for it, measured.
    let (bw, bh) = (4.0 * cs + 2.0 * sn, 4.0 * sn + 2.0 * cs);
    let at = (6.0 + 0.25 * bw, 5.0 - 0.5 * bh);
    let drawing =
        Drawing::new().page(&format!("{}{}", shape_of("text frame", POINT, Some(&transform)), connector("C", "s1", Some(4), (1.0, 1.0), at)));
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing);
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
    save(&mut s);
    near(stated_end(&content_of(&dir.read("a.odg")), "C"), (cm(at.0) + 0.25, cm(at.1)), "a turned text frame moved: the end follows its glue point");
    reads_back_on_the_point(&dir, "a turned text frame moved");
}

/// A frame whose `draw:glue-point` comes before its text box or its picture (the drawings; the standard puts the point after them): LibreOffice drops such a
/// point and glues a connector that names it to the frame as a whole (measured by this task), but the file names the point, and by an end glued to it follows
/// it: the element is a row, and a move carries the end by the move.
#[test]
fn frameglue_a_frames_glue_point_before_its_content_is_a_row_too_and_the_end_follows_it() {
    for kind in ["text frame", "picture frame"] {
        let made = shape_of(kind, "", None);
        let open_end = made.find('>').expect("a start tag") + 1;
        let first = format!("{}{POINT}{}", &made[..open_end], &made[open_end..]);
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &Drawing::new().page(&format!("{first}{}", connector("C", "s1", Some(4), (1.0, 1.0), AT))));
        run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
        save(&mut s);
        near(stated_end(&content_of(&dir.read("a.odg")), "C"), (cm(AT.0) + 0.25, cm(AT.1)), &format!("a {kind} with its point first, moved"));
        reads_back_on_the_point(&dir, &format!("a {kind} with its point first, moved"));
    }
}

/// Kept (what was right): an end glued to a custom shape's own glue point (the fifth its geometry lists, id 8) moves 0.25 inch right with the shape, and no other way.
#[test]
fn frameglue_an_end_glued_to_a_custom_shapes_listed_glue_point_moves_with_it() {
    // Five listed points in the view box 0 0 21600 21600; the fifth (id 8) is three quarters along the top side: (7 cm, 4 cm).
    let custom = concat!(
        r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="S" xml:id="s1" draw:id="s1" svg:width="4cm" svg:height="2cm" svg:x="4cm" svg:y="4cm"><text:p/>"#,
        r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:glue-points="0 0 21600 0 21600 21600 0 21600 16200 0" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    );
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&format!("{custom}{}", connector("C", "s1", Some(8), (1.0, 1.0), AT))));
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
    save(&mut s);
    near(stated_end(&content_of(&dir.read("a.odg")), "C"), (cm(AT.0) + 0.25, cm(AT.1)), "a custom shape's listed point 8");
}

/// Kept (what was right): ends glued to the standard point 0 of a polygon turned half a turn move 0.25 inch right with it. LibreOffice puts that point at the middle of the
/// top side of the box of the polygon's outline on the page, so the end is on the page's top side, which a move carries right.
#[test]
fn frameglue_an_end_glued_to_point_0_of_a_polygon_turned_half_a_turn_moves_with_it() {
    // `rotate (pi)` then `translate (8cm 6cm)`: the frame's corners (0, 0) and (4 cm, 2 cm) go to (8 cm, 6 cm) and (4 cm, 4 cm).
    let transform = format!("rotate ({:?}) translate (8cm 6cm)", std::f64::consts::PI);
    let shapes = format!(
        "{}{}{}",
        shape_of("polygon", "", Some(&transform)),
        connector("C", "s1", Some(0), (1.0, 1.0), (6.0, 4.0)),
        connector("D", "s1", Some(0), (9.0, 1.0), (6.0, 4.0))
    );
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&shapes));
    run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    for name in ["C", "D"] {
        near(stated_end(&content, name), (cm(6.0) + 0.25, cm(4.0)), &format!("connector {name} to point 0 of a polygon turned half a turn"));
    }
}

/// An end of an `.odg` connector glued to the whole of a shape (`draw:end-shape` and no glue point) is at the shape's connection point nearest the other end
/// after the shape moves: the left side's middle, 0.25 inch right after a move of 0.25 inch. On main the end stayed where the file had it.
#[test]
fn frameglue_an_end_glued_to_the_whole_of_a_moved_shape_follows_it() {
    for kind in ["rectangle", "custom shape", "text frame"] {
        let shapes = format!("{}{}", shape_of(kind, "", None), connector("C", "s1", None, (1.0, 5.0), (4.0, 5.0)));
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &Drawing::new().page(&shapes));
        let connect = s.document().unwrap().pages.get(0).unwrap().connects.iter().next().cloned().expect("a glued end");
        assert_eq!(connect.to_part, Some(3), "a {kind}: the end is glued to the whole shape: {connect:?}");
        run(&mut s, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
        save(&mut s);
        let content = content_of(&dir.read("a.odg"));
        near(stated_end(&content, "C"), (cm(4.0) + 0.25, cm(5.0)), &format!("a {kind} moved: the end glued to the whole of it follows"));
        let tag = connector_tag(&content, "C");
        assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("s1"), "a {kind}: still glued: {tag}");
        assert_eq!(attr(tag, "draw:end-glue-point"), None, "a {kind}: to the whole shape: {tag}");
    }
}

/// The model path (an `.odg` written from the model alone, `write_with(doc, None, ..)`, the writer a drawing with no `.odg` file is saved by): a shape of every
/// kind that holds a `draw:glue-point` child, moved 0.25 inch, is written with the element, the connector names it, its stated end is on the moved point, and the end is
/// on it when the file is read again.
#[test]
fn frameglue_the_model_writer_writes_the_glue_point_child_of_a_moved_shape_and_the_end_on_it() {
    for kind in KINDS {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing(kind));
        let id = id_of(s.document().unwrap(), 0, "S");
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 0.25, "dy": 0.0 }));
        let written = diacraft_odg::write_with(s.document().unwrap(), None, &diacraft_odg::WriteOptions::default())
            .unwrap_or_else(|e| panic!("a {kind}: the model writer refuses: {e}"));
        let content = content_of(&written.bytes);
        near(stated_end(&content, "C"), (cm(AT.0) + 0.25, cm(AT.1)), &format!("a {kind} moved, written from the model: the stated end"));
        assert_eq!(attr(connector_tag(&content, "C"), "draw:end-glue-point").as_deref(), Some("4"), "a {kind}: the connector names the point");
        assert!(content.contains("<draw:glue-point "), "a {kind}: the element is written: {content}");
        // A frame's glue point after its picture, where the standard puts it: LibreOffice drops one that comes before (measured).
        if kind == "picture frame" {
            assert!(content.find("<draw:glue-point ") > content.find("<draw:image"), "a {kind}: the point after the picture: {content}");
        }
        let out = Dir::new();
        out.write("a.odg", &written.bytes);
        reads_back_on_the_point(&out, &format!("a {kind} moved, written from the model"));
    }
}

/// The model path writes a picture's `draw:glue-point` back only while the row it made is unchanged: a row edited since (its held element's stamp is no longer the row's)
/// is no point the file can name, and a connector glued to it is refused in words, as a connector to any later row of a new picture is.
#[test]
fn frameglue_the_model_writer_refuses_a_connector_to_an_edited_glue_point_of_a_picture() {
    let dir = Dir::new();
    let s = open_drawing(&dir, &drawing("picture frame"));
    let mut doc = s.document().unwrap().clone();
    let id = id_of(&doc, 0, "S");
    let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).expect("a first page");
    let at = page.shapes.iter().position(|x| x.id == id).expect("the picture");
    let shape = page.shapes.get_mut(at).map(std::sync::Arc::make_mut).expect("the picture");
    let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|x| x.name.as_str() == "Connection").map(std::sync::Arc::make_mut).expect("its points");
    let row = section.rows.iter_mut().find(|r| r.ix == Some(4)).map(std::sync::Arc::make_mut).expect("point 4");
    row.cells.insert(diacraft_doc::CellName::new("X"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("0.1")).with_unit("IN"));
    match diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions::default()) {
        Err(e) => assert!(e.to_string().contains("a new connector glued to a connection point of a new picture past the four"), "{e}"),
        Ok(_) => panic!("a connector to an edited point of a picture is written"),
    }
    // The same drawing unedited is written.
    assert!(diacraft_odg::write_with(s.document().unwrap(), None, &diacraft_odg::WriteOptions::default()).is_ok());
}
