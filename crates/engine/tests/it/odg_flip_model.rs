//! A flip (or a turn, a resize, a move, a fill, a line or a text) made in the session is never lost silently by the `.odg` model writer.
//!
//! The model writer wrote a custom shape of a file with the `draw:enhanced-geometry` its file held, whose own mirrors (`draw:mirror-horizontal`, `draw:mirror-vertical`) said what the
//! shape's flips were **when the file was read**. A flip made in the session since was in no file it wrote: the shape came back unflipped, with nothing said. Each case here opens a
//! drawing of one shape by a route that is written by the model writer (a `.odg` saved as `.diacraft` and opened again, so that no file is there to splice into; a flat `.fodg`; a drawing of
//! another format), makes the edit through the commands, saves it as an `.odg`, opens that, and holds what the session had: the pose, the turn and both flips, **read from the file's own
//! markup as well as with DiaCraft's data**. A save that cannot keep an edit says so in words; nothing is dropped without being said.

use diacraft_engine::{EngineError, Session};
use proptest::prelude::*;
use serde_json::json;

use crate::conn_points_files::without_extension_attributes;
use crate::edit::doc;
use crate::native_common::opened;
use crate::odg_build::{Dir, Drawing, content_of, open_bytes_in, open_drawing};
use crate::odg_file::{look, text_at};
use crate::run;

/// The shapes of the drawings, as `(kind, how)`: **what** it is (a custom shape with an asymmetric outline, so that a mirror is no no-op, a rectangle, an ellipse, a circle by its centre, a
/// polygon, a polyline, a path, a text box, a regular polygon, a custom shape of a 3-D extrusion) and what its file already holds of its pose (`plain`, `turned` by a `draw:transform`,
/// `flipped` by a transform that mirrors it, `mirrored` by the geometry's own mirror, `text-turned`, `glued`: a glue point of its own).
pub(crate) const CASES: [(&str, &str); 25] = [
    ("custom", "plain"),
    ("custom", "turned"),
    ("custom", "mirrored"),
    ("custom", "mirrored-v"),
    ("custom", "flipped"),
    ("custom", "text-turned"),
    ("custom", "glued"),
    ("custom", "extruded"),
    ("rect", "plain"),
    ("rect", "turned"),
    ("rect", "flipped"),
    ("rect", "glued"),
    ("ellipse", "plain"),
    ("ellipse", "turned"),
    ("ellipse", "glued"),
    ("ellipse", "centred"),
    ("circle", "centred"),
    ("polygon", "plain"),
    ("polygon", "turned"),
    ("polyline", "plain"),
    ("path", "plain"),
    ("path", "flipped"),
    ("frame", "plain"),
    ("frame", "turned"),
    ("regular-polygon", "plain"),
];

/// The element of a shape named `S`, 6 by 3 centimetres at (3, 3) when it holds its place and else by the transform that puts it about there, as the file of another program writes it.
pub(crate) fn element(kind: &str, how: &str) -> String {
    let frame = match how {
        "turned" => "svg:width=\"6cm\" svg:height=\"3cm\" draw:transform=\"rotate (0.5235987755982988) translate (7cm 6cm)\"",
        "flipped" => "svg:width=\"6cm\" svg:height=\"3cm\" draw:transform=\"scale (1 -1) translate (3cm 9cm)\"",
        _ => "svg:width=\"6cm\" svg:height=\"3cm\" svg:x=\"3cm\" svg:y=\"3cm\"",
    };
    let head = |tag: &str, more: &str| {
        format!(
            "<draw:{tag} draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"S\" xml:id=\"s\" draw:id=\"s\" {frame}{more}>"
        )
    };
    let glue_point = if how == "glued" { "<draw:glue-point draw:id=\"4\" svg:x=\"2cm\" svg:y=\"1cm\" draw:escape-direction=\"right\"/>" } else { "" };
    let centred = |tag: &str, size: &str| {
        format!(
            "<draw:{tag} draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"S\" xml:id=\"s\" draw:id=\"s\" svg:cx=\"6cm\" svg:cy=\"4.5cm\" {size}><text:p>t</text:p></draw:{tag}>"
        )
    };
    match kind {
        "custom" => {
            let mirror = match how {
                "mirrored" => " draw:mirror-horizontal=\"true\"",
                "mirrored-v" => " draw:mirror-vertical=\"true\"",
                "text-turned" => " draw:text-rotate-angle=\"30\"",
                "extruded" => " draw:extrusion=\"true\"",
                _ => "",
            };
            format!(
                "{}{glue_point}<text:p>t</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 21600 0 21600 Z N\"{mirror}/></draw:custom-shape>",
                head("custom-shape", "")
            )
        }
        "rect" => format!("{}{glue_point}<text:p>t</text:p></draw:rect>", head("rect", "")),
        "ellipse" if how == "centred" => centred("ellipse", "svg:rx=\"3cm\" svg:ry=\"1.5cm\""),
        "ellipse" => format!("{}{glue_point}<text:p>t</text:p></draw:ellipse>", head("ellipse", "")),
        "circle" => centred("circle", "svg:r=\"2cm\""),
        "polygon" => {
            format!("{}<text:p/></draw:polygon>", head("polygon", " svg:viewBox=\"0 0 6000 3000\" draw:points=\"0,3000 3000,0 6000,3000\""))
        }
        "polyline" => {
            format!("{}<text:p/></draw:polyline>", head("polyline", " svg:viewBox=\"0 0 6000 3000\" draw:points=\"0,3000 3000,0 6000,3000\""))
        }
        "path" => format!("{}<text:p/></draw:path>", head("path", " svg:viewBox=\"0 0 6000 3000\" svg:d=\"M 0 3000 L 3000 0 L 6000 3000 Z\"")),
        "frame" => format!("{}<draw:text-box><text:p>hello</text:p></draw:text-box></draw:frame>", head("frame", "")),
        _ => {
            format!("{}<text:p/></draw:regular-polygon>", head("regular-polygon", " draw:corners=\"5\" draw:concave=\"false\" draw:sharpness=\"0\""))
        }
    }
}

/// How a drawing of one shape reaches the session, by what writes its next Save As to an `.odg`: **`model`**, the `.odg` opened and saved as a `.diacraft` file and opened again (no `.odg`
/// is there to splice into, so the model writer writes the shape as the shape it was read as), **`flat`**, a flat `.fodg` (no Save As splices into it either), and **`splice`**, the `.odg`
/// itself (its Save As splices into a copy of it, and refuses a flip in words).
pub(crate) const ROUTES: [&str; 3] = ["model", "flat", "splice"];

/// The session that has opened the drawing of one shape `element` by `route`.
pub(crate) fn open_by(route: &str, dir: &Dir, element: &str) -> Session {
    let drawing = Drawing::new().page(element);
    match route {
        "flat" => open_bytes_in(dir, "a.fodg", &drawing.flat_bytes()),
        "model" => {
            let mut s = open_drawing(dir, &drawing);
            run(&mut s, "file.save_as", json!({ "path": dir.path("through.diacraft") }));
            opened(&dir.path("through.diacraft"))
        }
        _ => open_drawing(dir, &drawing),
    }
}

/// The first shape of the first page that is no connector.
pub(crate) fn the_shape(s: &Session) -> u32 {
    let page = doc(s).pages.get(0).expect("a page");
    page.shapes.iter().find(|x| x.sheet.cell("BeginX").is_none()).expect("a shape").id
}

/// The numbers that say a shape's pose and its flips, as the engine resolves them (a shape of a drawing of another format may hold none of them itself and have them of its master): what an
/// edit of the frame changes.
fn pose_cells(s: &Session, shape: &diacraft_doc::Shape) -> Vec<(&'static str, f64)> {
    let page = doc(s).pages.get(0).expect("a page").id;
    let f = diacraft_engine::draw::shape_frame(doc(s), page, shape.id).expect("a frame");
    let flag = |on: bool| f64::from(u8::from(on));
    vec![
        ("PinX", f.pin_x),
        ("PinY", f.pin_y),
        ("Width", f.width),
        ("Height", f.height),
        ("Angle", f.angle),
        ("FlipX", flag(f.flip_x)),
        ("FlipY", flag(f.flip_y)),
    ]
}

/// The pose of the first shape of `s` that is no connector.
pub(crate) fn pose_of(s: &Session) -> Vec<(&'static str, f64)> {
    let page = doc(s).pages.get(0).expect("a page");
    pose_cells(s, page.shapes.iter().find(|x| x.sheet.cell("BeginX").is_none()).expect("a shape"))
}

/// The pose of the last shape of the first page: the one a session of a drawing of another format has dropped.
fn pose_of_last(s: &Session) -> Vec<(&'static str, f64)> {
    pose_cells(s, doc(s).pages.get(0).and_then(|p| p.shapes.iter().next_back()).expect("a shape"))
}

/// Whether two numbers of a pose are the same: an angle by its sine and cosine (a turn and the same turn a whole turn away are one), the rest within a millionth of an inch.
fn same_number(name: &str, x: f64, y: f64) -> bool {
    if name == "Angle" { (x - y).sin().abs() < 1e-6 && (x - y).cos() > 0.0 } else { (x - y).abs() < 1e-6 }
}

/// Whether two poses are the same.
pub(crate) fn same_pose(a: &[(&'static str, f64)], b: &[(&'static str, f64)]) -> Result<(), String> {
    for ((name, x), (_, y)) in a.iter().zip(b) {
        if !same_number(name, *x, *y) {
            return Err(format!("{name}: the session has {x}, the file gives {y}"));
        }
    }
    Ok(())
}

/// The cells of a pose that a file says by its own markup (the flips and the turn): all that matter to a program that reads nothing of DiaCraft's.
const MARKUP: [&str; 3] = ["Angle", "FlipX", "FlipY"];

/// Whether the turn and the flips the file's markup alone gives are the session's.
fn same_markup(made: &[(&'static str, f64)], read: &[(&'static str, f64)]) -> Result<(), String> {
    for ((name, x), (_, y)) in made.iter().zip(read) {
        if MARKUP.contains(name) && !same_number(name, *x, *y) {
            return Err(format!("the file's own markup gives {name} {y}, the session has {x}"));
        }
    }
    Ok(())
}

/// Applies edit `edit` to shape `id` through the commands: a flip (`h`, `v`, `hv`), a turn (`turn`), both (`h+turn`), a resize (`resize`) or a move (`move`).
fn edit(s: &mut Session, id: u32, edit: &str) {
    let flip = |s: &mut Session, axis: &str| drop(run(s, "shape.flip", json!({ "ids": [id], "axis": axis })));
    match edit {
        "h" => flip(s, "horizontal"),
        "v" => flip(s, "vertical"),
        "hv" => {
            flip(s, "horizontal");
            flip(s, "vertical");
        }
        "turn" => drop(run(s, "shape.rotate", json!({ "ids": [id], "angle": 20.0 }))),
        "h+turn" => {
            flip(s, "horizontal");
            drop(run(s, "shape.rotate", json!({ "ids": [id], "angle": 20.0 })));
        }
        "resize" => resize(s, id, 1.5, 0.75),
        "move" => drop(run(s, "shape.move", json!({ "ids": [id], "dx": 1.25, "dy": -0.5 }))),
        other => panic!("an edit nobody wrote: {other}"),
    }
}

/// Resizes shape `id` by the factors `fw` and `fh` about its corner.
fn resize(s: &mut Session, id: u32, fw: f64, fh: f64) {
    let page = doc(s).pages.get(0).expect("a page").id;
    let f = diacraft_engine::draw::shape_frame(doc(s), page, id).expect("a frame");
    run(s, "shape.resize", json!({ "id": id, "x": f.x, "y": f.y, "width": f.width * fw, "height": f.height * fh }));
}

/// What a Save As did: the words of its refusal, or the session reading the file back and the bytes of the file.
pub(crate) enum Saved {
    Refused(String),
    File(Box<Session>, Vec<u8>),
}

/// Saves `s` as `name` in `dir` (an `.odg`), and opens it again.
pub(crate) fn save_and_reopen(s: &mut Session, dir: &Dir, name: &str) -> Saved {
    match s.execute("file.save_as", &json!({ "path": dir.path(name) })) {
        Ok(_) => Saved::File(Box::new(opened(&dir.path(name))), dir.read(name)),
        Err(EngineError::Failed { msg, .. }) => Saved::Refused(msg),
        Err(other) => panic!("the save failed in a way nobody expected: {other:?}"),
    }
}

/// The session that reads `bytes` as another program's file: DiaCraft's attributes in its markup taken out.
fn read_without_data(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    opened(&dir.write(name, &without_extension_attributes(bytes)))
}

const EDITS: [&str; 7] = ["h", "v", "hv", "turn", "h+turn", "resize", "move"];

/// The words a save that does not keep an edit holds, for a case that may be refused: `None` for a case that must be kept.
///
/// * the splice (a Save As of an `.odg` opened as it is, which writes into a copy of the file) refuses every flip, and a turn of a shape whose transform holds a flip, in words;
/// * a flip of a shape whose geometry is an object the reader does not draw (a 3-D extrusion) is refused by the model writer in words, as it keeps no mirror for it.
fn may_refuse(route: &str, (kind, how): (&str, &str), what: &str) -> Option<&'static str> {
    let flips = what.contains('h') || what.contains('v');
    match route {
        "splice" if flips || how == "flipped" => Some("flip"),
        "splice" if how == "centred" && what == "turn" => Some("turn of a circle or an ellipse"),
        // A circle given by its centre has one radius: a resize to another height than width is refused in words, by every route.
        _ if kind == "circle" && what == "resize" => Some("circle given by its centre"),
        _ if kind == "custom" && how == "extruded" && flips => Some("extrusion"),
        _ => None,
    }
}

/// Every case of the matrix: the route, the shape, the edit. Collects every miss before it asserts. **A case that may be refused is either refused with the words it names or kept; any
/// other is kept**: the file reads back the pose the session has, in the file's own markup (the turn and both flips) as in DiaCraft's data.
#[test]
fn an_edit_of_the_pose_is_in_the_odg_a_save_as_writes_or_the_save_is_refused_in_words() {
    let mut misses: Vec<String> = Vec::new();
    let mut kept = 0usize;
    for route in ROUTES {
        for (kind, how) in CASES {
            for what in EDITS {
                let dir = Dir::new();
                let case = format!("{route}, {kind} {how}, {what}");
                let mut s = open_by(route, &dir, &element(kind, how));
                if s.document().is_none_or(|d| d.pages.get(0).is_none_or(|p| p.shapes.is_empty())) {
                    misses.push(format!("{case}: the drawing holds no shape"));
                    continue;
                }
                let id = the_shape(&s);
                edit(&mut s, id, what);
                let want = pose_of(&s);
                match (save_and_reopen(&mut s, &dir, "out.odg"), may_refuse(route, (kind, how), what)) {
                    (Saved::Refused(words), Some(part)) => {
                        if !words.contains(part) {
                            misses.push(format!("{case}: refused, but not in the words that name {part:?}: {words}"));
                        }
                    }
                    (Saved::Refused(words), None) => misses.push(format!("{case}: refused: {words}")),
                    (Saved::File(again, bytes), _) => {
                        kept += 1;
                        if let Err(why) = same_pose(&want, &pose_of(&again)) {
                            misses.push(format!("{case}: {why}"));
                        }
                        // A program that reads none of DiaCraft's data: the turn and the flips are in the markup.
                        if let Err(why) = same_markup(&want, &pose_of(&read_without_data(&dir, "plain.odg", &bytes))) {
                            misses.push(format!("{case}: {why}"));
                        }
                    }
                }
            }
        }
    }
    assert!(misses.is_empty(), "{} silent losses:\n{}", misses.len(), misses.join("\n"));
    assert!(kept > 300, "most cases keep their edit, and {kept} did");
}

/// **A flat drawing saved in place** (Save, which writes the flat file again): each edit of the pose is in the file the save writes, or the save is refused in words. The same matrix as the
/// Save As to an `.odg`, read from the flat file the save wrote.
#[test]
fn an_edit_of_the_pose_of_a_flat_drawing_is_in_the_flat_file_a_save_writes_or_the_save_is_refused_in_words() {
    let mut misses: Vec<String> = Vec::new();
    let mut kept = 0usize;
    for (kind, how) in CASES {
        for what in EDITS {
            let dir = Dir::new();
            let case = format!("{kind} {how}, {what}");
            let mut s = open_by("flat", &dir, &element(kind, how));
            let id = the_shape(&s);
            edit(&mut s, id, what);
            let want = pose_of(&s);
            match s.execute("file.save", &json!({})) {
                Err(EngineError::Failed { msg, .. }) if msg.contains("could not be saved") && msg.len() > 40 => {}
                Err(other) => misses.push(format!("{case}: refused in no words: {other:?}")),
                Ok(_) => {
                    kept += 1;
                    let again = opened(&dir.path("a.fodg"));
                    if let Err(why) = same_pose(&want, &pose_of(&again)) {
                        misses.push(format!("{case}: {why}"));
                    }
                }
            }
        }
    }
    assert!(misses.is_empty(), "{} silent losses:\n{}", misses.len(), misses.join("\n"));
    assert!(kept > 50, "most cases keep their edit, and {kept} did");
}

/// The repro of the F41 note: open an `.odg`, save it as `.diacraft`, open that, flip a custom shape, a rectangle and an ellipse, save as an `.odg`, open it. The shape comes back with
/// the flip, **and the file says it as a mirror of the geometry** (`draw:mirror-horizontal`, `draw:mirror-vertical`) for the programs that read nothing of DiaCraft's.
#[test]
fn a_flip_of_a_custom_shape_a_rectangle_and_an_ellipse_of_an_odg_is_in_the_odg_a_save_as_writes() {
    for route in ["model", "flat"] {
        for kind in ["custom", "rect", "ellipse"] {
            for (what, mirrors) in [("h", [true, false]), ("v", [false, true]), ("hv", [true, true])] {
                let dir = Dir::new();
                let mut s = open_by(route, &dir, &element(kind, "plain"));
                let id = the_shape(&s);
                edit(&mut s, id, what);
                let Saved::File(again, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{route} {kind} {what}: refused") };
                let want = pose_of(&s);
                same_pose(&want, &pose_of(&again)).unwrap_or_else(|why| panic!("{route} {kind} {what}: {why}"));
                let content = content_of(&bytes);
                let geometry = &content[content.find("<draw:enhanced-geometry").expect("a geometry element")..];
                let geometry = &geometry[..geometry.find('>').expect("its end")];
                for (local, wanted) in [("mirror-horizontal", mirrors[0]), ("mirror-vertical", mirrors[1])] {
                    let says = geometry.contains(&format!("draw:{local}=\"true\""));
                    assert_eq!(says, wanted, "{route} {kind} {what}: {local} in {geometry}");
                }
            }
        }
    }
}

/// A flip that undoes the file's own mirror writes the mirror away: the shape is unflipped in the session and in the file.
#[test]
fn a_flip_that_undoes_the_mirror_a_file_held_is_unflipped_in_the_file() {
    for route in ["model", "flat"] {
        let dir = Dir::new();
        let mut s = open_by(route, &dir, &element("custom", "mirrored"));
        let id = the_shape(&s);
        assert_eq!(pose_of(&s).iter().find(|(n, _)| *n == "FlipX").map(|p| p.1), Some(1.0), "{route}: the file's mirror is the session's flip");
        edit(&mut s, id, "h");
        let Saved::File(again, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{route}: refused") };
        assert_eq!(pose_of(&again).iter().find(|(n, _)| *n == "FlipX").map(|p| p.1), Some(0.0), "{route}");
        assert!(!content_of(&bytes).contains("mirror-horizontal=\"true\""), "{route}: no mirror is left in the file");
    }
}

/// The text of a custom shape turned in its file and then flipped in the session reads as the file turned it: the file's number is the one that reads back as the session's turn of the
/// text beside the flips, so a program that draws it turns the text as the session does.
#[test]
fn the_text_turn_of_a_flipped_custom_shape_is_the_one_that_reads_back_as_the_sessions() {
    for how in ["plain", "text-turned"] {
        for what in ["h", "v", "hv"] {
            let dir = Dir::new();
            let mut s = open_by("model", &dir, &element("custom", how));
            let id = the_shape(&s);
            edit(&mut s, id, what);
            let Saved::File(_, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{how} {what}: refused") };
            let plain = read_without_data(&dir, "plain.odg", &bytes);
            let text_turn = |s: &Session| {
                let page = doc(s).pages.get(0).expect("a page");
                let shape = page.shapes.iter().find(|x| x.sheet.cell("BeginX").is_none()).expect("a shape");
                shape.sheet.cell("TxtAngle").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
            };
            let (made, read) = (text_turn(&s), text_turn(&plain));
            assert!((made - read).abs() < 1e-6, "{how} {what}: the session turns the text {made}, the file's markup reads {read}");
        }
    }
}

/// A drawing of another format: shapes dropped in a session, saved as a VSDX file and opened again (their sheets are the VSDX reader's, not the `.odg` reader's), flipped, saved as an
/// `.odg` and opened. Each keeps its flips and its turn.
#[test]
fn a_flip_of_a_shape_read_from_a_vsdx_file_is_in_the_odg_a_save_as_writes() {
    for master in ["rectangle", "ellipse", "right_triangle"] {
        for what in ["h", "v", "hv", "h+turn"] {
            let dir = Dir::new();
            let mut first = crate::native_common::pinned();
            run(&mut first, "shape.drop", json!({ "master": master, "page": 1, "x": 4.0, "y": 5.0 }));
            run(&mut first, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
            let mut s = opened(&dir.path("a.vsdx"));
            let id = doc(&s).pages.get(0).and_then(|p| p.shapes.iter().next_back()).expect("the dropped shape").id;
            edit(&mut s, id, what);
            let want = pose_of_last(&s);
            let Saved::File(again, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{master} {what}: refused") };
            same_pose(&want, &pose_of_last(&again)).unwrap_or_else(|why| panic!("{master} {what}: {why}"));
            same_markup(&want, &pose_of_last(&read_without_data(&dir, "plain.odg", &bytes))).unwrap_or_else(|why| panic!("{master} {what}: {why}"));
        }
    }
}

/// A text typed into a shape read from an `.odg` is in the `.odg` a Save As writes, for every kind of shape and every spacing of the text.
#[test]
fn a_text_given_to_a_shape_of_an_odg_is_in_the_odg_a_save_as_writes() {
    let mut misses: Vec<String> = Vec::new();
    for route in ["model", "flat"] {
        for (kind, how) in CASES {
            for text in [" ", "a", " a", "a ", "a  b", "ab cd"] {
                let dir = Dir::new();
                let mut s = open_by(route, &dir, &element(kind, how));
                let id = the_shape(&s);
                apply(&mut s, id, &Edit::Text(text.to_owned()));
                let want = text_at(&s, 0, 0);
                match save_and_reopen(&mut s, &dir, "out.odg") {
                    // The reader reads no text from a 3-D extrusion, so a text given to one is refused in words.
                    Saved::Refused(words) if how == "extruded" && words.contains("a text on a shape whose geometry is a 3-D extrusion") => {}
                    Saved::Refused(words) => misses.push(format!("{route} {kind} {how} {text:?}: refused: {words}")),
                    Saved::File(again, _) => {
                        let got = text_at(&again, 0, 0);
                        if got != want {
                            misses.push(format!("{route} {kind} {how} {text:?}: the session shows {want:?}, the file gives {got:?}"));
                        }
                    }
                }
            }
        }
    }
    assert!(misses.is_empty(), "{} texts lost:\n{}", misses.len(), misses.join("\n"));
}

/// The places of the connection rows of the first shape: (`X`, `Y`) of each, in the shape's own frame.
fn row_places(s: &Session) -> Vec<(f64, f64)> {
    let id = the_shape(s);
    crate::conn_points::points(s, id).iter().map(|p| (p.x, p.y)).collect()
}

/// **A glue point element a shape's file held stays where the page reads it**: LibreOffice places a `draw:glue-point` in the page's axes, so one the file gave a shape of its own
/// is the row it makes only for the turn and the flips the shape had when it was read. A turn or a flip made in the session since leaves the element in the old axes, and the
/// next reader gives the row another place than the session's (and a connector glued to it, another end). The model writer writes the elements again for the shape's frame as it is, so a
/// program that reads none of DiaCraft's data reads the rows the session has.
#[test]
fn the_glue_points_a_shape_of_a_file_held_are_written_where_the_page_reads_them_after_a_turn_or_a_flip() {
    let mut misses: Vec<String> = Vec::new();
    for route in ["model", "flat", "splice"] {
        for kind in ["custom", "rect", "ellipse"] {
            for what in ["turn", "h", "v", "h+turn", "hv", "resize"] {
                let dir = Dir::new();
                let mut s = open_by(route, &dir, &element(kind, "glued"));
                let id = the_shape(&s);
                if row_places(&s).len() != 5 {
                    misses.push(format!("{route} {kind} {what}: the shape has {} rows, not 5", row_places(&s).len()));
                    continue;
                }
                edit(&mut s, id, what);
                let want = row_places(&s);
                let case = format!("{route} {kind} {what}");
                match save_and_reopen(&mut s, &dir, "out.odg") {
                    // The splice refuses a flip in words (and writes a resize and a turn that has no connector glued to a point of the file).
                    Saved::Refused(words)
                        if route == "splice" && may_refuse(route, (kind, "glued"), what).is_some_and(|part| words.contains(part)) => {}
                    Saved::Refused(words) => misses.push(format!("{case}: refused: {words}")),
                    Saved::File(again, bytes) => {
                        for (read, how) in
                            [(row_places(&again), "with DiaCraft's data"), (row_places(&read_without_data(&dir, "plain.odg", &bytes)), "without it")]
                        {
                            // A shape written as a custom shape that lists its rows as its own glue points reads, without the marker that says so, as the four standard rows and then those.
                            let read = if read.len() == want.len() + 4 { read[4..].to_vec() } else { read };
                            if read.len() != want.len() || read.iter().zip(&want).any(|(p, q)| (p.0 - q.0).abs() > 1e-6 || (p.1 - q.1).abs() > 1e-6) {
                                misses.push(format!("{case}: the rows {how} are {read:?}, the session has {want:?}"));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(misses.is_empty(), "{} glue points moved:\n{}", misses.len(), misses.join("\n"));
}

/// A connector of the file glued to the glue point element of a custom shape, a rectangle or an ellipse of the file (`draw:glue-point` with id 4, so that the rows are a list the writer can say
/// again), the shape then turned and the file saved into: **the save is made, the element is written again for the turn, and the end of the connector is where the row is**. (An element whose
/// id leaves a gap in the rows cannot be written again, and the turn of a shape that a connector is glued to there stays refused in words: `odg_glue_writes`.)
#[test]
fn a_turn_of_a_file_shape_a_connector_is_glued_to_at_its_glue_point_is_saved_with_the_element_written_again() {
    for kind in ["custom", "rect", "ellipse"] {
        let dir = Dir::new();
        let connector = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"C\" draw:start-shape=\"s\" draw:start-glue-point=\"4\" svg:x1=\"6cm\" svg:y1=\"2cm\" svg:x2=\"16cm\" svg:y2=\"8cm\" svg:d=\"M 6000 2000 L 16000 8000\" svg:viewBox=\"0 0 16001 8001\"><text:p/></draw:connector>";
        let mut s = open_drawing(&dir, &Drawing::new().page(&format!("{}{connector}", element(kind, "glued"))));
        let id = the_shape(&s);
        edit(&mut s, id, "turn");
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(saved["edited"], true, "{kind}: {saved}");
        let want_rows = row_places(&s);
        let want_ends = ends_in_cm(&s);
        let bytes = dir.read("a.odg");
        // The file's own markup (no DiaCraft data): the rows are where the session has them, and the connector's glued end is on its row.
        let plain = read_without_data(&dir, "plain.odg", &bytes);
        let rows = row_places(&plain);
        assert_eq!(rows.len(), want_rows.len(), "{kind}");
        for (read, want) in rows.iter().zip(&want_rows) {
            assert!(
                (read.0 - want.0).abs() < 1e-6 && (read.1 - want.1).abs() < 1e-6,
                "{kind}: the file's markup reads a row at {read:?}, the session has it at {want:?}"
            );
        }
        let got = ends_in_cm(&plain);
        for ((name, a), (_, b)) in want_ends.iter().zip(&got) {
            // The connector's begin is the point (the glued end), its end is free: both are the session's within 0.002 cm.
            assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.002), "{kind}: {name}: the file has the ends {b:?}, the session has {a:?}");
        }
    }
}

/// A glue point the session put so far from its shape that the writer cannot say its place as an element (a place is written as a number of centimetres within 10^15): a turn of the shape, saved
/// into the file, is **refused in words** and not saved with the element left where it was, which the next reader would read as another row.
#[test]
fn a_turn_of_a_file_shape_whose_glue_point_element_cannot_be_written_again_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(&element("custom", "glued")));
    let id = the_shape(&s);
    run(&mut s, "sheet.set_cell", json!({ "id": id, "cell": "Connections.X5", "formula": "100000000000000000 in" }));
    edit(&mut s, id, "turn");
    match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => {
            assert!(msg.contains("a turn of a shape of the file whose connectors are glued to a glue point"), "{msg}")
        }
        other => panic!("the turn was saved with the element where it was: {other:?}"),
    }
}

// ---- the property ----------------------------------------------------------------------------------------------------------------------

/// One edit of the property test: the commands a person edits a shape with.
#[derive(Clone, Debug)]
enum Edit {
    Move(f64, f64),
    Resize(f64, f64),
    Rotate(i32),
    Flip(u8),
    Fill(u32),
    Line(f64),
    Text(String),
    TextTurn(i32),
    TextMove(f64, f64),
}

fn edits() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (-2.0..2.0f64, -2.0..2.0f64).prop_map(|(dx, dy)| Edit::Move(dx, dy)),
        (0.5..2.0f64, 0.5..2.0f64).prop_map(|(w, h)| Edit::Resize(w, h)),
        (-180..=180i32).prop_map(Edit::Rotate),
        (0..3u8).prop_map(Edit::Flip),
        (0..0x0100_0000u32).prop_map(Edit::Fill),
        (0.5..4.0f64).prop_map(Edit::Line),
        "[a-z ]{0,8}".prop_map(Edit::Text),
        (-180..=180i32).prop_map(Edit::TextTurn),
        (-0.5..0.5f64, -0.5..0.5f64).prop_map(|(dx, dy)| Edit::TextMove(dx, dy)),
    ]
}

fn apply(s: &mut Session, id: u32, edit: &Edit) {
    match edit {
        Edit::Move(dx, dy) => drop(run(s, "shape.move", json!({ "ids": [id], "dx": dx, "dy": dy }))),
        Edit::Resize(fw, fh) => resize(s, id, *fw, *fh),
        Edit::Rotate(degrees) => drop(run(s, "shape.rotate", json!({ "ids": [id], "angle": f64::from(*degrees) }))),
        Edit::Flip(axis) => {
            let axes: &[&str] = match axis % 3 {
                0 => &["horizontal"],
                1 => &["vertical"],
                _ => &["horizontal", "vertical"],
            };
            for axis in axes {
                run(s, "shape.flip", json!({ "ids": [id], "axis": axis }));
            }
        }
        Edit::Fill(rgb) => drop(run(s, "shape.format", json!({ "ids": [id], "fill": format!("#{rgb:06x}") }))),
        Edit::Line(weight) => drop(run(s, "shape.format", json!({ "ids": [id], "weight": weight }))),
        // A shape's text is what an editor shows: an insertion at the start, so that a shape that had none gets one.
        Edit::Text(text) => drop(run(s, "shape.set_text", json!({ "id": id, "text": text, "start": 0, "end": 0 }))),
        Edit::TextTurn(degrees) => drop(run(s, "text.block_rotate", json!({ "id": id, "angle": f64::from(*degrees) }))),
        Edit::TextMove(dx, dy) => drop(run(s, "text.block_move", json!({ "id": id, "dx": dx, "dy": dy }))),
    }
}

/// Where the text block of the first shape is on the page: its four corners, its pin and the way the text runs, as numbers.
fn block_of(s: &Session) -> Vec<f64> {
    let page = doc(s).pages.get(0).expect("a page");
    let id = page.shapes.iter().find(|x| x.sheet.cell("BeginX").is_none()).expect("a shape").id;
    let b = diacraft_engine::draw::text_block_place(doc(s), page.id, id).expect("a text block");
    let mut out: Vec<f64> = b.corners.iter().flat_map(|c| [c.0, c.1]).collect();
    out.extend([b.pin.0, b.pin.1, b.direction.cos(), b.direction.sin()]);
    out
}

/// What the property compares: the look the window's controls show (its fill, its line, the rest), without the `ID`, and the text.
fn shown(s: &mut Session) -> (serde_json::Value, String) {
    (look(s, 0, 0), text_at(s, 0, 0))
}

/// Whether two looks are the same: every number within a hundredth (a weight written in another unit reads back as the same weight), every other value as it is.
fn same_look(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value::{Array, Number, Object};
    match (a, b) {
        (Number(x), Number(y)) => (x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(0.0)).abs() < 0.01,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_look(p, q)),
        (Object(x), Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_look(v, w))),
        _ => a == b,
    }
}

/// What one case of the property did.
enum Outcome {
    Kept,
    Refused,
}

/// One case of the property: random edits `steps` of the shape `CASES[at]`, saved by the model writer by `route` and opened again. A save is either refused in words or reads back with
/// the pose, the look and the text of the session.
fn case(at: usize, flat: bool, steps: &[Edit]) -> Result<Outcome, String> {
    let (kind, how) = CASES.get(at).copied().ok_or("no such shape")?;
    let route = if flat { "flat" } else { "model" };
    let what = format!("{route} {kind} {how} {steps:?}");
    let dir = Dir::new();
    let mut s = open_by(route, &dir, &element(kind, how));
    let id = the_shape(&s);
    for step in steps {
        apply(&mut s, id, step);
    }
    let (want, want_text) = shown(&mut s);
    let (want_pose, want_block) = (pose_of(&s), block_of(&s));
    match save_and_reopen(&mut s, &dir, "out.odg") {
        Saved::Refused(words) if words.contains("could not be saved") && words.len() > 40 => Ok(Outcome::Refused),
        Saved::Refused(words) => Err(format!("{what}: the refusal's words: {words}")),
        Saved::File(mut again, _) => {
            same_pose(&want_pose, &pose_of(&again)).map_err(|why| format!("{what}: {why}"))?;
            if let Some((k, (x, y))) = want_block.iter().zip(block_of(&again)).enumerate().find(|(_, (x, y))| (**x - *y).abs() > 1e-6) {
                return Err(format!("{what}: the text block's number {k} was {x}, the file gives {y}"));
            }
            let (look, text) = shown(&mut again);
            if !same_look(&want, &look) {
                return Err(format!("{what}: the look was {want}, the file gives {look}"));
            }
            if want_text != text {
                return Err(format!("{what}: the text was {want_text:?}, the file gives {text:?}"));
            }
            Ok(Outcome::Kept)
        }
    }
}

/// **No edit is lost silently**: random edits of a shape read from an `.odg` (a move, a resize, a turn, a flip, a fill, a line, a text), saved as an `.odg` by the model writer
/// (through a `.diacraft` file, or from a flat file) and opened, each survives in the pose, the look and the text, or the save is refused in words. The generator is proptest's, on a fixed
/// seed so that a failure repeats, and `cases` is explicit; the cases that are saved are the great majority, so a property of refusals is no property.
#[test]
fn random_edits_of_a_shape_read_from_an_odg_survive_the_model_writers_save_as_or_it_is_refused_in_words() {
    use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
    let (kept, refused) = (std::cell::Cell::new(0usize), std::cell::Cell::new(0usize));
    let mut seed = [0_u8; 32];
    seed[..8].copy_from_slice(&0x0F11_B0DE_u64.to_le_bytes());
    let config = Config { cases: 160, failure_persistence: None, max_shrink_iters: 200, ..Config::default() };
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &seed));
    let strategy = (0..CASES.len(), any::<bool>(), proptest::collection::vec(edits(), 1..5));
    let result = runner.run(&strategy, |(at, flat, steps)| match case(at, flat, &steps).map_err(TestCaseError::fail)? {
        Outcome::Kept => {
            kept.set(kept.get() + 1);
            Ok(())
        }
        Outcome::Refused => {
            refused.set(refused.get() + 1);
            Ok(())
        }
    });
    if let Err(why) = result {
        panic!("{why}");
    }
    assert!(kept.get() > refused.get() * 3, "{} kept and {} refused", kept.get(), refused.get());
}

// ---- the files for the LibreOffice check -----------------------------------------------------------------------------------------------

/// The page's height in inches.
fn page_height(s: &Session) -> f64 {
    doc(s).pages.get(0).and_then(|p| p.sheet.cell("PageHeight")).and_then(|c| c.value.as_ref()?.number()).expect("a page height")
}

/// The ends of every connector of the first page, in centimetres from the page's top left: (connector's name, [begin x, begin y, end x, end y]).
fn ends_in_cm(s: &Session) -> Vec<(String, [f64; 4])> {
    let height = page_height(s);
    let page = doc(s).pages.get(0).expect("a page");
    page.shapes
        .iter()
        .filter(|x| x.sheet.cell("BeginX").is_some())
        .map(|x| {
            let n = |cell: &str| x.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).expect("a number");
            let cm = |px: f64, py: f64| [px * 2.54, (height - py) * 2.54];
            let (b, e) = (cm(n("BeginX"), n("BeginY")), cm(n("EndX"), n("EndY")));
            (x.name_u.as_deref().unwrap_or("?").to_owned(), [b[0], b[1], e[0], e[1]])
        })
        .collect()
}

/// Makes the files LibreOffice is asked to open for the check is a script outside the repository: `lo_flip.py` and `lo_compare.py` in the run folder of the task: a shape of an
/// `.odg` file (a custom shape, a rectangle, an ellipse, each with a glue point of its own) given an edit by each route that the model writer writes (`.diacraft`, `.fodg`) and saved as an
/// `.odg`, a connector glued to its own point and one to a standard point, and what the session has of each file: the flips, the turn and the ends of the connectors. Run with `ODGFLIP_LO_DIR` set
/// to an empty folder: `cargo test -p diacraft-engine --test it write_the_files_for_libreoffice -- --ignored`.
#[test]
#[ignore = "writes files for a check by another program"]
fn write_the_files_for_libreoffice() {
    let out = std::path::PathBuf::from(std::env::var("ODGFLIP_LO_DIR").expect("ODGFLIP_LO_DIR names the folder the files go to"));
    let mut expected = serde_json::Map::new();
    for route in ["model", "flat"] {
        for kind in ["custom", "rect", "ellipse"] {
            for what in ["none", "h", "v", "hv", "turn", "h+turn"] {
                let dir = Dir::new();
                let mut s = open_by(route, &dir, &element(kind, "glued"));
                let id = the_shape(&s);
                if what != "none" {
                    edit(&mut s, id, what);
                }
                // A connector from the glue point the file gave the shape (row 4) and one from a standard point (row 2: the bottom's middle), to free places on the page.
                for (point, to) in [(4, (7.5, 9.0)), (2, (2.0, 8.5))] {
                    run(&mut s, "connector.add", json!({ "from": { "id": id, "point": point }, "to": { "x": to.0, "y": to.1 } }));
                }
                let Saved::File(_, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{route} {kind} {what}: refused") };
                let name = format!("{route}_{kind}_{}", what.replace('+', "_"));
                std::fs::write(out.join(format!("{name}.odg")), bytes).expect("a file");
                let pose = pose_of(&s);
                let get = |n: &str| pose.iter().find(|(k, _)| *k == n).map_or(0.0, |p| p.1);
                expected.insert(name, json!({ "ends": ends_in_cm(&s), "flip": [get("FlipX"), get("FlipY")], "angle": get("Angle") }));
            }
        }
    }
    // A connector the file holds, glued to the side middles of two shapes, one of which is edited: its ends follow the shape, and name the point LibreOffice numbers for the edited shape's frame.
    for route in ["model", "flat"] {
        for kind in ["custom", "rect"] {
            for what in ["h", "v", "turn", "h+turn"] {
                let dir = Dir::new();
                let shapes = [
                    element(kind, "plain"),
                    element(kind, "plain")
                        .replace("draw:name=\"S\"", "draw:name=\"T\"")
                        .replace("xml:id=\"s\" draw:id=\"s\"", "xml:id=\"t\" draw:id=\"t\"")
                        .replace("svg:x=\"3cm\"", "svg:x=\"14cm\""),
                ];
                // From glue point 1 of the first (LibreOffice's right side) to glue point 3 of the second (its left side).
                let connector = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"C\" draw:start-shape=\"s\" draw:start-glue-point=\"1\" draw:end-shape=\"t\" draw:end-glue-point=\"3\" svg:x1=\"9cm\" svg:y1=\"4.5cm\" svg:x2=\"14cm\" svg:y2=\"4.5cm\" svg:d=\"M 9000 4500 L 14000 4500\" svg:viewBox=\"0 0 14001 4501\"><text:p/></draw:connector>";
                let page = format!("{}{}{connector}", shapes[0], shapes[1]);
                let drawing = Drawing::new().page(&page);
                let mut s = if route == "flat" {
                    open_bytes_in(&dir, "a.fodg", &drawing.flat_bytes())
                } else {
                    let mut first = open_drawing(&dir, &drawing);
                    run(&mut first, "file.save_as", json!({ "path": dir.path("through.diacraft") }));
                    opened(&dir.path("through.diacraft"))
                };
                let id = the_shape(&s);
                edit(&mut s, id, what);
                let Saved::File(_, bytes) = save_and_reopen(&mut s, &dir, "out.odg") else { panic!("{route} {kind} {what}: refused") };
                let name = format!("{route}_held_connector_{kind}_{}", what.replace('+', "_"));
                std::fs::write(out.join(format!("{name}.odg")), bytes).expect("a file");
                let pose = pose_of(&s);
                let get = |n: &str| pose.iter().find(|(k, _)| *k == n).map_or(0.0, |p| p.1);
                expected.insert(name, json!({ "ends": ends_in_cm(&s), "flip": [get("FlipX"), get("FlipY")], "angle": get("Angle") }));
            }
        }
    }
    // A connector the file holds, glued to the glue point element of a shape of the file that is turned, saved into the file (the splice): the element is written again for the turn.
    for kind in ["custom", "rect", "ellipse"] {
        for what in ["turn", "move"] {
            let dir = Dir::new();
            let connector = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"C\" draw:start-shape=\"s\" draw:start-glue-point=\"4\" svg:x1=\"6cm\" svg:y1=\"2cm\" svg:x2=\"16cm\" svg:y2=\"8cm\" svg:d=\"M 6000 2000 L 16000 8000\" svg:viewBox=\"0 0 16001 8001\"><text:p/></draw:connector>";
            let mut s = open_drawing(&dir, &Drawing::new().page(&format!("{}{connector}", element(kind, "glued"))));
            let id = the_shape(&s);
            edit(&mut s, id, what);
            run(&mut s, "file.save", json!({}));
            let name = format!("splice_file_connector_{kind}_{what}");
            std::fs::write(out.join(format!("{name}.odg")), dir.read("a.odg")).expect("a file");
            let pose = pose_of(&s);
            let get = |n: &str| pose.iter().find(|(k, _)| *k == n).map_or(0.0, |p| p.1);
            expected.insert(name, json!({ "ends": ends_in_cm(&s), "flip": [get("FlipX"), get("FlipY")], "angle": get("Angle") }));
        }
    }
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&expected).expect("json")).expect("a file");
}
