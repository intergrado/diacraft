//! The two saves the milestone-1 rerun found that read back different from what was asked, with no word. **W1** a later save wrote again the route
//! of a connector no edit had touched (`docs/editing-commands.md`; the connector rows). **W2** a new connector
//! glued to a shape whose id another shape holds named the wrong shape.
//!
//! Every drawing is built by hand from the standard (`odg_build.rs`), with numbers composed for the test; what is read back is read by the `odg`
//! crate's reader.

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing, read_back};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, save};
use crate::run;

/// Every element called `tag` in `content`, each from its `<` to its end tag.
pub(crate) fn elements(content: &str, tag: &str) -> Vec<String> {
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

/// The connector called `name` in `content`, from its `<` to its end tag.
pub(crate) fn connector_named(content: &str, name: &str) -> String {
    elements(content, "draw:connector")
        .into_iter()
        .find(|e| e.contains(&format!("draw:name=\"{name}\"")))
        .unwrap_or_else(|| panic!("no connector {name}: {content}"))
}

/// A custom shape of rectangle type, `w` by `h` centimetres at (`x`, `y`), with `xml:id` and `draw:id` `id`, that lists four glue points of its own (ids 4 to 7):
/// `glue` is their positions in the order listed, in a view box twice as many units as the shape has thousandths of a centimetre (7754 by 1614 for 3.877 by 0.807 cm),
/// so that a position is a whole number of units and the point is where the file says, to the last digit.
pub(crate) fn glue_rect(name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64), glue: &str) -> String {
    let (vw, vh) = ((w * 2000.0).round(), (h * 2000.0).round());
    format!(
        "<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"{w}cm\" svg:height=\"{h}cm\" svg:x=\"{x}cm\" svg:y=\"{y}cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 {vw} {vh}\" draw:type=\"rectangle\" draw:glue-points=\"{glue}\" draw:enhanced-path=\"M 0 0 L {vw} 0 {vw} {vh} 0 {vh} 0 0 Z N\"/></draw:custom-shape>"
    )
}

/// The glue points S lists, in the view box of [`glue_rect`]: id 4 its top at the middle (x 9.7435 cm), id 5 a point inside it at x 9.44 cm, y 4.3 cm, then its bottom and
/// its right side.
const S_GLUE: &str = "3877 0 3270 1064 3877 1614 7754 807";

/// K2's path as the file has it: it leaves U's bottom, goes down and round to the left of everything, up to y 4.000 cm, along to x 9.440 cm and down to its end on S.
const K2_PATH: &str = "M 9744 11192 L 9744 11692 L 6500 11692 L 6500 4000 L 9440 4000 L 9440 4300";

/// W1's drawing: three custom shapes T, S and U of rectangle type, 3.877 by 0.807 cm, at x 7.805 cm and y 2.381, 3.768 and 10.385 cm; K1, a line from T (glue point 2,
/// its bottom) to S (glue point 4); K2, a standard connector from U (glue point 2) to S (glue point 5) with `draw:line-skew` and its path.
///
/// **The numbers the vectors did not give are ours**: S's glue points and K2's path. Their effect, with the session below (T moved by (-0.7905, -0.6382)
/// inches, which is 2.008 cm left and **1.621 cm down** the page, then sized): K1 runs from T's bottom to S's top. Once T has moved it crosses K2's horizontal leg
/// (y 4.000 cm) at x 9.296 cm, 1.44 mm from the leg's right end, more than half a jump's chord (1.058 mm: `LineToLineX` 0.125 inch times 2/3, half of it; a jump
/// lies at least half a chord from both ends of its leg), so the engine gives K2, the flatter of the two legs, a hop. Once T is sized K1 crosses it at
/// x 9.375 cm, 0.65 mm from the end, and the hop is taken away. K2's final leg and K1 never meet, so K1 has no hop.
fn w1_drawing() -> Drawing {
    let frame = |y| (7.805, y, 3.877, 0.807);
    let k1 = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"K1\" draw:type=\"line\" draw:start-shape=\"t\" draw:start-glue-point=\"2\" draw:end-shape=\"s\" draw:end-glue-point=\"4\" svg:x1=\"9.7435cm\" svg:y1=\"3.188cm\" svg:x2=\"9.7435cm\" svg:y2=\"3.768cm\" svg:d=\"M 9744 3188 L 9744 3768\" svg:viewBox=\"0 0 1 581\"><text:p/></draw:connector>";
    let k2 = format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"s\" draw:end-glue-point=\"5\" draw:line-skew=\"0cm -0.612cm\" svg:x1=\"9.7435cm\" svg:y1=\"11.192cm\" svg:x2=\"9.44cm\" svg:y2=\"4.3cm\" svg:d=\"{K2_PATH}\" svg:viewBox=\"0 0 3245 7693\"><text:p/></draw:connector>"
    );
    Drawing::new().page(
        &[
            glue_rect("T", "t", frame(2.381), S_GLUE),
            glue_rect("S", "s", frame(3.768), S_GLUE),
            glue_rect("U", "u", frame(10.385), S_GLUE),
            k1.to_owned(),
            k2,
        ]
        .concat(),
    )
}

/// The move and the size of the composed session, on T.
fn move_t(s: &mut Session) {
    let t = id_of(s.document().unwrap(), 0, "T");
    run(s, "shape.move", json!({ "ids": [t], "dx": -0.7905, "dy": -0.6382 }));
}

fn size_t(s: &mut Session) {
    let t = id_of(s.document().unwrap(), 0, "T");
    run(s, "shape.size", json!({ "ids": [t], "width": 1.8993, "height": 0.4932 }));
}

/// Whether the model's K2 has a hop (an `ArcTo` row) in its route.
fn k2_has_hop(s: &Session) -> bool {
    let doc = s.document().unwrap();
    let k2 = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some("K2")).unwrap();
    let route = k2.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
    route.rows.iter().any(|r| r.row_type.as_deref() == Some("ArcTo"))
}

/// The warnings of a save's answer.
pub(crate) fn warnings(saved: &Value) -> Vec<String> {
    saved["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_owned)).collect()
}

pub(crate) const HOP_WORDS: &str = "could not be written, and the file's own is kept";

/// The `content.xml` of the file, with the elements named `names` taken out: what a save that edited those must have left as it was.
fn without(content: &str, names: &[&str]) -> String {
    let mut text = content.to_owned();
    for tag in ["draw:custom-shape", "draw:connector"] {
        for e in elements(&text.clone(), tag).iter().filter(|e| names.iter().any(|n| e.contains(&format!("draw:name=\"{n}\"")))) {
            text = text.replacen(e.as_str(), "", 1);
        }
    }
    text
}

/// W1: T moved by (-0.7905, -0.6382) inches and sized to 1.8993 by 0.4932 inches, then one `file.save_as`. K2 is glued to S and to U, neither of
/// which was edited: it keeps its bytes (its `draw:line-skew` and its `svg:d`), and the save says nothing of it. Before the fix the second routing edit took the
/// hop the first had given K2 away, the model's rows were the engine's spelling of the file's route, and the save wrote `svg:d` again and took the skew with it:
/// LibreOffice drew K2's middle leg 6.1 mm elsewhere.
#[test]
fn w1_a_second_routing_edit_leaves_the_connector_no_edit_touched_as_it_was() {
    let dir = Dir::new();
    let drawing = w1_drawing();
    let mut s = open_drawing(&dir, &drawing);
    let original = content_of(&dir.read("a.odg"));
    let before = connector_named(&original, "K2");
    assert!(before.contains("draw:line-skew=\"0cm -0.612cm\"") && before.contains(&format!("svg:d=\"{K2_PATH}\"")), "{before}");
    move_t(&mut s);
    assert!(k2_has_hop(&s), "the first edit gives K2 a hop (K1 crosses it)");
    size_t(&mut s);
    assert!(!k2_has_hop(&s), "the second takes it away: the rebuild writes K2's rows in the engine's spelling");
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(warnings(&saved), Vec::<String>::new(), "K2 is as the file has it: nothing is said");
    let content = content_of(&dir.read("b.odg"));
    assert_eq!(connector_named(&content, "K2"), before, "K2 keeps its skew and its path");
    // The edit changed T and K1, which follows it, and nothing else of the part.
    assert_ne!(connector_named(&content, "K1"), connector_named(&original, "K1"), "K1 follows T");
    assert_eq!(without(&content, &["T", "K1"]), without(&original, &["T", "K1"]), "nothing else of the part changed");
    // The real reader reads K2 as it read it in the file: its route and its skew.
    let (was, _) = read_back(&drawing.bytes());
    let (now, report) = read_back(&dir.read("b.odg"));
    let k2 = |d: &diacraft_doc::Document| d.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some("K2")).unwrap().clone();
    assert_eq!(k2(&now).sheet.sections, k2(&was).sheet.sections, "the route K2 reads as");
    // The skew is among the attributes the reader keeps of the connector, in its `odf` slot.
    let skew = k2(&now).ext.get().and_then(|d| d.odf.kept.raw.get("draw:line-skew").map(str::to_owned));
    assert_eq!(skew.as_deref(), Some("0cm -0.612cm"));
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_saves_again_as_no_edit(&dir, &mut s, "b.odg");
}

/// W1 with a save between the two edits, and with a save after a save: the first save keeps K2's path and says the hop could not be written (a hop is a
/// decoration the `.odg` has no place for); the save after the second edit, which takes the hop away, writes nothing of K2 and says nothing; and neither does the
/// one after. Then the undo of each edit in turn: the hop is back and said, then the file's own rows say nothing.
#[test]
fn w1_a_save_between_the_two_edits_says_the_hop_once_and_the_later_save_leaves_the_connector_as_it_was() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &w1_drawing());
    let before = connector_named(&content_of(&dir.read("a.odg")), "K2");
    move_t(&mut s);
    let first = save(&mut s);
    assert_eq!(first["edited"], true);
    let said = warnings(&first);
    assert_eq!(said.len(), 1, "{first}");
    assert!(said[0].contains("the route of connector") && said[0].contains(HOP_WORDS), "{}", said[0]);
    assert_eq!(connector_named(&content_of(&dir.read("a.odg")), "K2"), before, "the file's own path is kept");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    size_t(&mut s);
    assert!(!k2_has_hop(&s));
    let second = save(&mut s);
    assert_eq!(second["edited"], true, "T and K1 changed");
    assert_eq!(warnings(&second), Vec::<String>::new(), "no hop is left to say: {second}");
    assert_eq!(connector_named(&content_of(&dir.read("a.odg")), "K2"), before, "K2 keeps its skew and its path");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    run(&mut s, "edit.undo", json!({}));
    assert!(k2_has_hop(&s), "undone, the hop is back");
    let undone = save(&mut s);
    assert_eq!(warnings(&undone).len(), 1, "{undone}");
    assert_eq!(connector_named(&content_of(&dir.read("a.odg")), "K2"), before);
    run(&mut s, "edit.undo", json!({}));
    let all = save(&mut s);
    assert_eq!(warnings(&all), Vec::<String>::new(), "{all}");
    assert_eq!(connector_named(&content_of(&dir.read("a.odg")), "K2"), before);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The control of the two tests above: a connector whose own shape moved is routed anew by the engine, and the save writes its route and takes its skew away
/// (`draw:line-skew`: "remove when DiaCraft routes the connector anew"). The fix compares the route and keeps no connector's bytes by a rule of its own.
#[test]
fn w1_a_connector_whose_shape_moved_is_still_written_and_loses_its_skew() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &w1_drawing());
    let u = id_of(s.document().unwrap(), 0, "U");
    run(&mut s, "shape.move", json!({ "ids": [u], "dx": 0.0, "dy": 1.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let k2 = connector_named(&content_of(&dir.read("a.odg")), "K2");
    assert!(!k2.contains("draw:line-skew"), "routed anew: {k2}");
    assert!(!k2.contains(&format!("svg:d=\"{K2_PATH}\"")), "{k2}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

// ---- W2 ---------------------------------------------------------------------------------------------------------------------------------------------

/// W2's drawing: an empty `office:styles`, and three custom shapes 3 by 1 cm: A (`id1`, x 2 cm, y 2 cm), B (`id9`, 12 cm, 2 cm) and C (`id9`, 12 cm, 8 cm).
fn w2_drawing() -> Drawing {
    let shape = |name: &str, id: &str, at: (f64, f64)| glue_rect(name, id, (at.0, at.1, 3.0, 1.0), "3000 0 0 1000 3000 2000 6000 1000");
    Drawing::new().page(&[shape("A", "id1", (2.0, 2.0)), shape("B", "id9", (12.0, 2.0)), shape("C", "id9", (12.0, 8.0))].concat())
}

/// What the connector of a file reads back as glued to: the names of the shapes at its two ends.
fn glued_to(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let (doc, _) = read_back(bytes);
    let page = doc.pages.get(0).unwrap();
    let connector = page.shapes.iter().find(|s| s.sheet.cell("BeginX").is_some()).unwrap().id;
    let end = |cell: &str| {
        let row = page.connects.iter().find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(cell))?;
        page.shapes.iter().find(|s| s.id == row.to_sheet)?.name_u.as_deref().map(str::to_owned)
    };
    (end("BeginX"), end("EndX"))
}

/// W2 (a connector's reference resolves against `xml:id`, else `draw:id`, to the first shape that holds the name): `connector.add` from shape 1
/// (A) to shape 3 (C), whose id (`id9`) B holds first, then `file.save_as`. The spec says nothing of what to write for a shape whose id another shape holds, so
/// the save is **refused in words** and writes nothing: before the fix the connector held `draw:end-shape="id9"` with its end drawn at C, and read back glued to B
/// (moving B moved its end, moving C did not). Never a reference that names the wrong shape.
#[test]
fn w2_a_new_connector_glued_to_a_shape_whose_id_an_earlier_shape_holds_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &w2_drawing());
    let (a, c) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "C"));
    assert_eq!((a, c), (1, 3));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": c } }));
    let out = dir.path("b.odg");
    let said = match s.execute("file.save_as", &json!({ "path": out })) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("the save was not refused as a failure: {other:?}"),
    };
    assert!(said.contains("a connector glued to a shape whose id an earlier shape of the page holds"), "{said}");
    assert!(!dir.files().contains(&"b.odg".to_owned()), "nothing was written: {:?}", dir.files());
    // The way out: the connector taken back, and the drawing saves as the file it was.
    run(&mut s, "edit.undo", json!({}));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("c.odg") }));
    assert_eq!(saved["edited"], false, "{saved}");
}

/// W2, the same drawing glued to B, the first holder of `id9`: the save is written, and the file reads back glued to B, by the real reader.
#[test]
fn w2_a_connector_glued_to_the_first_holder_of_an_id_is_written_and_reads_back_glued_to_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &w2_drawing());
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(warnings(&saved), Vec::<String>::new());
    let bytes = dir.read("a.odg");
    assert!(elements(&content_of(&bytes), "draw:connector").iter().any(|c| c.contains("draw:end-shape=\"id9\"")));
    assert_eq!(glued_to(&bytes), (Some("A".to_owned()), Some("B".to_owned())));
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A custom shape 3 by 1 cm at (`x`, `y`) cm, whose ids are `ids` as written (attributes, or none).
fn w2_shape(name: &str, ids: &str, (x, y): (f64, f64)) -> String {
    format!(
        "<draw:custom-shape {ids} draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"3cm\" svg:height=\"1cm\" svg:x=\"{x}cm\" svg:y=\"{y}cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    )
}

/// A drawing of A (`id1`) and the shapes in `rest`, a session that has opened it, and the connector A to the shape `to` added: the outcome of `file.save`.
fn w2_save(rest: &[String], to: &str) -> (Dir, Session, Result<Value, EngineError>) {
    let dir = Dir::new();
    let body = [vec![w2_shape("A", "xml:id=\"id1\" draw:id=\"id1\"", (2.0, 2.0))], rest.to_vec()].concat().concat();
    let mut s = open_drawing(&dir, &Drawing::new().page(&body));
    let (a, c) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, to));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": c } }));
    let saved = s.execute("file.save", &json!({}));
    (dir, s, saved)
}

/// The glue of the connector the save made, by names, and the warnings the real reader gave the file.
fn glue_and_read_warnings(dir: &Dir) -> ((Option<String>, Option<String>), Vec<String>) {
    let bytes = dir.read("a.odg");
    (glued_to(&bytes), read_back(&bytes).1.warnings)
}

fn refusal(saved: Result<Value, EngineError>) -> String {
    match saved {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("the save was not refused as a failure: {other:?}"),
    }
}

/// The review, a blocker by the bar; on main before this task: a connector glued to a shape whose id is **empty** was written as `draw:end-shape=""`, which
/// the reader reads as a free end, with no word. The save is refused in words, as W2's is, and nothing is written: in each spelling the reader reads as the empty name
/// (`xml:id=""` is the name even beside a `draw:id`).
#[test]
fn w2_a_target_whose_id_is_empty_is_refused_in_words_and_nothing_is_written() {
    for (case, ids) in [
        ("an empty draw:id", "draw:id=\"\""),
        ("an empty xml:id beside a draw:id that is not", "xml:id=\"\" draw:id=\"id5\""),
        ("an empty xml:id alone", "xml:id=\"\""),
        ("two empty ids", "xml:id=\"\" draw:id=\"\""),
    ] {
        let (_dir, mut s, saved) = w2_save(&[w2_shape("C", ids, (12.0, 8.0))], "C");
        let said = refusal(saved);
        assert!(said.contains("a connector glued to a shape whose id is empty"), "{case}: {said}");
        // Nothing was written; the way out is to take the connector back, and the drawing then saves as the file it was.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(save(&mut s)["edited"], false, "{case}");
    }
    // The file is the bytes it was after a refusal.
    let dir = Dir::new();
    let body = [w2_shape("A", "xml:id=\"id1\" draw:id=\"id1\"", (2.0, 2.0)), w2_shape("C", "draw:id=\"\"", (12.0, 8.0))].concat();
    let mut s = open_drawing(&dir, &Drawing::new().page(&body));
    let before = dir.read("a.odg");
    let (a, c) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "C"));
    run(&mut s, "connector.add", json!({ "from": { "id": c }, "to": { "id": a } }));
    assert!(refusal(s.execute("file.save", &json!({}))).contains("whose id is empty"), "the begin end is judged too");
    assert_eq!(dir.read("a.odg"), before);
}

/// A target whose id holds a literal tab (or a line break) had the id unescaped to the tab and written as `&#9;`, a reference the reader keeps as a
/// tab, while the reader had read the shape's own id with each literal white space made a space (XML 1.0 section 3.3.3): the end came back free. The reference is
/// written by the name the reader gives the shape. A shape that holds the name another does (here a space and a tab) is judged by that name.
#[test]
fn w2_a_target_whose_id_holds_white_space_is_written_by_the_name_the_reader_gives_it() {
    for (case, ids, written) in [
        ("a tab", "draw:id=\"id\t9\"", "id 9"),
        ("a line feed", "draw:id=\"id\n9\"", "id 9"),
        ("a carriage return and a line feed", "draw:id=\"id\r\n9\"", "id 9"),
        ("a tab by reference, which the reader keeps", "draw:id=\"id&#9;9\"", "id&#9;9"),
        ("an xml:id with a tab beside a draw:id", "xml:id=\"id\t9\" draw:id=\"x\"", "id 9"),
    ] {
        let (dir, mut s, saved) = w2_save(&[w2_shape("C", ids, (12.0, 8.0))], "C");
        saved.unwrap_or_else(|e| panic!("{case}: {e:?}"));
        let content = content_of(&dir.read("a.odg"));
        assert!(content.contains(&format!("draw:end-shape=\"{written}\"")), "{case}: {content}");
        assert_eq!(
            glue_and_read_warnings(&dir),
            ((Some("A".to_owned()), Some("C".to_owned())), Vec::<String>::new()),
            "{case}: read back glued to the shape"
        );
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    // The name a shape has is the name the others are judged by: a tab and a space are one name, the first holder is B.
    let rest = [w2_shape("B", "draw:id=\"id 9\"", (12.0, 2.0)), w2_shape("C", "draw:id=\"id\t9\"", (12.0, 8.0))];
    let (_, _, saved) = w2_save(&rest, "C");
    assert!(refusal(saved).contains("an earlier shape of the page holds"), "C's name is B's");
    let (dir, _, saved) = w2_save(&rest, "B");
    saved.unwrap();
    assert_eq!(glue_and_read_warnings(&dir).0, (Some("A".to_owned()), Some("B".to_owned())));
}

// ---- item 3 -----------------------------------------------------------------------------------------------------------------------------------------

/// A custom shape `A` holding `paragraphs` (the markup of its `text:p` elements), on a drawing with the text style `T1`.
fn text_drawing(paragraphs: &str) -> Drawing {
    let t1 = "<style:style style:name=\"T1\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>";
    let mut drawing = Drawing::new();
    drawing.auto.push_str(t1);
    drawing.page(&format!(
        "<draw:custom-shape draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"A\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"3cm\">{paragraphs}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    ))
}

/// The paragraphs of the shape `A` in the saved `content.xml`: what is between the shape's start tag and its geometry.
fn paragraphs_of(content: &str) -> String {
    let at = content.find("draw:name=\"A\"").unwrap();
    let end = content[at..].find("<draw:enhanced-geometry").map_or(content.len(), |e| at + e);
    let start = content[at..end].find('>').map_or(at, |g| at + g + 1);
    content[start..end].to_owned()
}

/// Item 3 (LOW): a retext that keeps a span and replaces the characters up to the paragraph's end left the space the old run ended with (`World ` for `Hello `), which
/// ODF 1.3's white-space rules drop and DiaCraft reads back right. The replaced bytes now go on to the end of the node. A space that is a character (a run's space
/// before the next run, or before the edit) stays, and so does a paragraph the edit did not touch.
#[test]
fn item3_a_retext_that_reaches_the_end_of_a_run_takes_the_space_the_old_run_ended_with() {
    let bold = |text: &str| format!("<text:span text:style-name=\"T1\">{text}</text:span>");
    let cases: Vec<(&str, String, Value, String)> = vec![
        (
            "the whole text of a span",
            format!("<text:p>{}</text:p>", bold("Hello ")),
            json!({ "text": "World" }),
            format!("<text:p>{}</text:p>", bold("World")),
        ),
        (
            "the characters of a span",
            format!("<text:p>{}</text:p>", bold("Hello ")),
            json!({ "text": "World", "start": 0, "end": 5 }),
            format!("<text:p>{}</text:p>", bold("World")),
        ),
        (
            "text typed at the end of a span",
            format!("<text:p>{}</text:p>", bold("Hello ")),
            json!({ "text": "!", "start": 5, "end": 5 }),
            format!("<text:p>{}</text:p>", bold("Hello!")),
        ),
        (
            "the last characters of the last of two spans",
            format!("<text:p>{}{}</text:p>", bold("Hello"), bold(" there ")),
            json!({ "text": "you", "start": 6, "end": 11 }),
            format!("<text:p>{}{}</text:p>", bold("Hello"), bold(" you")),
        ),
        (
            "a space that is a character: the run is followed by another",
            format!("<text:p>Hello {}</text:p>", bold("x")),
            json!({ "text": "World", "start": 0, "end": 5 }),
            format!("<text:p>World {}</text:p>", bold("x")),
        ),
        (
            "an edit that stops before the last character",
            format!("<text:p>{}</text:p>", bold("Hello ")),
            json!({ "text": "J", "start": 0, "end": 1 }),
            format!("<text:p>{}</text:p>", bold("Jello ")),
        ),
        (
            "a paragraph the edit does not touch",
            format!("<text:p>{}</text:p><text:p>{}</text:p>", bold("one "), bold("two ")),
            json!({ "text": "uno", "start": 0, "end": 3 }),
            format!("<text:p>{}</text:p><text:p>{}</text:p>", bold("uno"), bold("two ")),
        ),
    ];
    for (name, paragraphs, params, saved_as) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &text_drawing(&paragraphs));
        let a = id_of(s.document().unwrap(), 0, "A");
        let mut p = params.clone();
        p["id"] = json!(a);
        run(&mut s, "shape.set_text", p);
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{name}: {saved}");
        assert_eq!(paragraphs_of(&content_of(&dir.read("a.odg"))), saved_as, "{name}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

// ----: the route a save kept is the file's, not the model's --------------------------------------------------------------------------------

pub(crate) fn number_of(shape: &diacraft_doc::Shape, cell: &str) -> f64 {
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

/// The corners of the route of connector `name` on page 0, on the page (inches, y up), with the hops and the points on a leg taken out: what the route is, however its
/// rows spell it (the test's own reading of the rows, not the writer's).
pub(crate) fn page_corners(doc: &diacraft_doc::Document, name: &str) -> Vec<(f64, f64)> {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some(name)).unwrap();
    let (bx, by, ex, ey) = (number_of(shape, "BeginX"), number_of(shape, "BeginY"), number_of(shape, "EndX"), number_of(shape, "EndY"));
    let (sn, cs) = (ey - by).atan2(ex - bx).sin_cos();
    let route = shape.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
    let mut out: Vec<(f64, f64)> = Vec::new();
    for row in route.rows.iter().filter(|r| !r.del) {
        let at = |c: &str| row.cells.get(c).and_then(|cell| cell.value.as_ref()?.number()).unwrap_or(f64::NAN);
        let (u, v) = (at("X"), at("Y"));
        let p = (bx + u * cs - v * sn, by + u * sn + v * cs);
        if out.last().is_some_and(|q| (q.0 - p.0).abs() < 1e-4 && (q.1 - p.1).abs() < 1e-4) {
            continue;
        }
        while let [.., a, b] = out[..] {
            let (ac, ab) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
            let length = ac.0.hypot(ac.1);
            let (along, off) = ((ab.0 * ac.0 + ab.1 * ac.1) / length, (ab.0 * ac.1 - ab.1 * ac.0).abs() / length);
            if off < 1e-3 && along > 1e-4 && along < length - 1e-4 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}

pub(crate) fn same_route(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 2e-3 && (p.1 - q.1).abs() < 2e-3)
}

/// What the saves have told of each connector whose file route is not the model's: the route of the model the save said it did not write.
pub(crate) type Told = std::collections::HashMap<String, Vec<(f64, f64)>>;

/// Whether the model's route of connector `name` has a hop (the `.odg` has no place for it): a row that is not a `LineTo`, other than the route's first row, which is
/// its `MoveTo`. **The gap style's hop is a `MoveTo` in the middle of the route** (the pen lifts over the leg it crosses): it is a hop too (the property
/// took it for a corner, and a route the writer keeps and says so of was held to be one it should have written).
pub(crate) fn has_hop(doc: &diacraft_doc::Document, name: &str) -> bool {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some(name)).unwrap();
    let route = shape.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
    route.rows.iter().enumerate().any(|(at, r)| !matches!((r.row_type.as_deref(), at), (Some("MoveTo"), 0) | (Some("LineTo"), 1..)))
}

/// After a save, for each connector of `names`: the route the file reopens with is the model's, **or** the model's route has a hop, which the file has no place for, and a
/// save said so of this route (a save that edits something else leaves it as it was and says nothing more). Two ways to be silent: a model route that **can** be
/// written (no hop) at a save that edited, and the file still holding another (the review's Repro 1 and 2); and a route with a hop that changed since the save that told
/// of it, which no save told of. Gives the silent ones.
pub(crate) fn check_told(s: &Session, dir: &Dir, file: &str, saved: &Value, names: &[&str], told: &mut Told, step: &str) -> Vec<String> {
    let said = warnings(saved);
    let edited = saved["edited"] == true;
    let (back, _) = read_back(&dir.read(file));
    let mut silent = Vec::new();
    for name in names {
        let id = id_of(s.document().unwrap(), 0, name);
        let (model, file_route) = (page_corners(s.document().unwrap(), name), page_corners(&back, name));
        let hop = has_hop(s.document().unwrap(), name);
        if same_route(&model, &file_route) {
            told.remove(*name);
        } else if hop && said.iter().any(|w| w.contains(&format!("connector {id} "))) {
            told.insert((*name).to_owned(), model);
        } else if !hop && edited {
            silent.push(format!("{step}: {name} has a route the file can hold and does not: the model's {model:?}, the file's {file_route:?}"));
        } else if hop && !told.get(*name).is_some_and(|was| same_route(was, &model)) {
            silent.push(format!("{step}: {name} has a hop no save told of: the model's {model:?}, the file's {file_route:?}"));
        }
    }
    silent
}

/// The composed session with K2 routed anew (U moved) and given a hop (T moved): the save keeps the file's path and says so. Then the hop is taken away by an
/// edit that moves none of K2's ends. **The file's route is the old one**, so the model's new route is written now, and its skew goes (the review's Repro 1: the
/// save was silent, the file kept `M 9744 11192 L 9744 11692 ...` and the model began 0.2 inch to the left).
#[test]
fn w1_a_route_kept_with_a_word_is_the_files_own_until_a_save_writes_the_route_the_model_has() {
    for way in ["undo", "move T back", "move T away"] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &w1_drawing());
        let u = id_of(s.document().unwrap(), 0, "U");
        run(&mut s, "shape.move", json!({ "ids": [u], "dx": -0.2, "dy": 0.0 }));
        move_t(&mut s);
        assert!(k2_has_hop(&s), "{way}: K2 has a hop");
        let mut told = Told::new();
        let first = save(&mut s);
        assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, way), Vec::<String>::new());
        assert_eq!(warnings(&first).len(), 1, "{way}: the hop is said: {first}");
        let t = id_of(s.document().unwrap(), 0, "T");
        match way {
            "undo" => drop(run(&mut s, "edit.undo", json!({}))),
            "move T back" => drop(run(&mut s, "shape.move", json!({ "ids": [t], "dx": 0.7905, "dy": 0.6382 }))),
            _ => drop(run(&mut s, "shape.move", json!({ "ids": [t], "dx": 2.0, "dy": 0.0 }))),
        }
        assert!(!k2_has_hop(&s), "{way}: the hop is gone");
        let second = save(&mut s);
        assert_eq!(warnings(&second), Vec::<String>::new(), "{way}: nothing to say, the route is written: {second}");
        let (back, _) = read_back(&dir.read("a.odg"));
        assert!(same_route(&page_corners(s.document().unwrap(), "K2"), &page_corners(&back, "K2")), "{way}: the file reopens with the model's route");
        let k2 = connector_named(&content_of(&dir.read("a.odg")), "K2");
        assert!(!k2.contains("draw:line-skew") && !k2.contains(&format!("svg:d=\"{K2_PATH}\"")), "{way}: routed anew: {k2}");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// The review's Repro 2: a waypoint changed by a command (`connector.edit_path`) with T moved; the save keeps the file's path and says so; then T is sized and the
/// hop goes. The route the model has is not the file's: it is written.
#[test]
fn w1_a_waypoint_a_command_moved_is_written_when_the_hop_that_kept_it_goes() {
    for (leg, to) in [(2, 2.3), (1, 3.0)] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &w1_drawing());
        move_t(&mut s);
        let k2 = id_of(s.document().unwrap(), 0, "K2");
        run(&mut s, "connector.edit_path", json!({ "id": k2, "leg": leg, "to": to }));
        let mut told = Told::new();
        let first = save(&mut s);
        assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, "first"), Vec::<String>::new());
        assert_eq!(warnings(&first).len(), 1, "leg {leg}: kept and said: {first}");
        size_t(&mut s);
        let second = save(&mut s);
        assert_eq!(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, "second"), Vec::<String>::new(), "leg {leg}");
        let (back, _) = read_back(&dir.read("a.odg"));
        assert!(same_route(&page_corners(s.document().unwrap(), "K2"), &page_corners(&back, "K2")), "leg {leg}: written");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// A small generator for the chains below (xorshift): the same chains on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// The text of the attribute `name` of a start tag.
pub(crate) fn attr_of(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// The `y` of the first point of a path (its first `M`), in hundredths of a millimetre.
pub(crate) fn first_y(path: Option<String>) -> Option<f64> {
    let path = path?;
    let mut numbers = path.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c.is_whitespace()).split(|c: char| c.is_whitespace() || c == ',');
    numbers.next()?;
    numbers.next()?.parse().ok()
}

/// What the last save left of one connector, for the rule: its element in the file, the model's corners and ends, and whether the file held the model's route.
struct SeenConnector {
    tag: String,
    corners: Vec<(f64, f64)>,
    ends: [f64; 4],
    file_has_model_route: bool,
}

/// What the property has seen at the last save (or at the open), and the checks of **the rule of **: across a save that changed the page's
/// height, a connector no edit touched (the file held the route the model had, and the model's corners and ends are what they were) keeps its own path, **shifted** by
/// what the page grew, its `svg:viewBox` and `draw:line-skew` as they were, and the file reads back with the model's route.
struct Seen {
    height: f64,
    connectors: std::collections::HashMap<String, SeenConnector>,
    /// The connectors the last `check_page_keep` held to the rule.
    kept: usize,
}

impl Seen {
    fn new() -> Seen {
        Seen { height: 0.0, connectors: std::collections::HashMap::new(), kept: 0 }
    }

    fn ends_of(doc: &diacraft_doc::Document, name: &str) -> [f64; 4] {
        let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some(name)).unwrap();
        ["BeginX", "BeginY", "EndX", "EndY"].map(|cell| number_of(shape, cell))
    }

    /// Records the connectors `names` as the file and the model have them now.
    fn look(&mut self, s: &Session, dir: &Dir, file: &str, names: &[&str]) {
        let content = content_of(&dir.read(file));
        let (back, _) = read_back(&dir.read(file));
        let doc = s.document().unwrap();
        self.height = page_cell(s, "PageHeight");
        for name in names {
            let model = page_corners(doc, name);
            let file_has_model_route = same_route(&model, &page_corners(&back, name));
            let seen = SeenConnector { tag: connector_named(&content, name), corners: model, ends: Seen::ends_of(doc, name), file_has_model_route };
            self.connectors.insert((*name).to_owned(), seen);
        }
    }

    /// The findings of the rule at a save made (`saved` is its answer), against what the last save left.
    fn check_page_keep(&mut self, s: &Session, dir: &Dir, file: &str, saved: &Value, names: &[&str], step: &str) -> Vec<String> {
        self.kept = 0;
        let mut bad = Vec::new();
        let dh = page_cell(s, "PageHeight") - self.height;
        if saved["edited"] != true || dh == 0.0 {
            return bad;
        }
        let content = content_of(&dir.read(file));
        let (back, _) = read_back(&dir.read(file));
        let doc = s.document().unwrap();
        for name in names {
            let Some(was) = self.connectors.get(*name) else { continue };
            let model = page_corners(doc, name);
            let ends = Seen::ends_of(doc, name);
            let untouched =
                was.file_has_model_route && same_route(&was.corners, &model) && ends.iter().zip(was.ends).all(|(a, b)| (a - b).abs() < 1e-6);
            if !untouched {
                continue;
            }
            self.kept += 1;
            let tag = connector_named(&content, name);
            for attribute in ["draw:line-skew", "svg:viewBox"] {
                let (now, then) = (attr_of(&tag, attribute), attr_of(&was.tag, attribute));
                if now != then {
                    bad.push(format!("{step}: {name}'s {attribute} is {now:?} where it was {then:?}, and no edit touched the connector"));
                }
            }
            match (first_y(attr_of(&was.tag, "svg:d")), first_y(attr_of(&tag, "svg:d"))) {
                (Some(before), Some(now)) if (now - before - dh * 2540.0).abs() < 2e-3 => {}
                other => bad.push(format!(
                    "{step}: {name}'s path starts at {other:?} (it was shifted {} hundredths of a millimetre by the page)",
                    dh * 2540.0
                )),
            }
            if !same_route(&model, &page_corners(&back, name)) {
                bad.push(format!("{step}: {name} reopens with another route than the model's"));
            }
        }
        bad
    }
}

/// A property over chains of edits, saves, undos and redos on W1's drawing: after every save, the route each connector reopens with is the model's, or a save
/// has said of this route that it was not written. (No divergence the user was not told of, whatever the edits and the saves between them.) **And the rule
/// **Across a save that changed the page's height, a connector no edit touched keeps its own path shifted by the change, with its skew and view box.
#[test]
fn w1_over_chains_of_edits_saves_and_undos_no_route_of_the_file_parts_from_the_model_in_silence() {
    let moves_t = [(-0.7905, -0.6382), (0.7905, 0.6382), (2.0, 0.0), (-0.3, 0.2), (0.0, -1.0)];
    let moves_u = [(-0.2, 0.0), (0.2, 0.0), (0.0, -0.1), (0.3, 0.1)];
    let moves_s = [(0.3, 0.1), (-0.2, 0.0)];
    let sizes_t = [(1.8993, 0.4932), (1.0, 0.3), (2.5, 0.6)];
    let paths = [(2, 2.3), (1, 3.0), (2, 1.5), (1, 2.0)];
    let mut silent = Vec::new();
    let (mut saves, mut page_keeps) = (0, 0);
    for seed in 1..=100u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &w1_drawing());
        let mut told = Told::new();
        let mut seen = Seen::new();
        seen.look(&s, &dir, "a.odg", &["K1", "K2"]);
        let mut steps = Vec::new();
        for _ in 0..14 {
            let id = |s: &Session, name: &str| id_of(s.document().unwrap(), 0, name);
            // The edits that give K2 a hop and take it away weigh more than the rest: the composed session, a move of U, undo. **Adds 11, the page's
            // size** (a page that grows, one that shrinks, one that is wider or narrower): the corners the index keeps for a route a save kept are in the page as that save
            // left it, and a save that changes the page's height changes the frame they are in.
            let table = [0, 0, 0, 1, 1, 2, 3, 3, 4, 5, 6, 6, 6, 7, 8, 9, 10, 11, 11];
            let step = match table[rng.next(table.len())] {
                11 => {
                    let (width, height) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
                    // The shapes sit in the top 4.7 inches of the page: a shorter page is kept above 7.
                    let (w, h) = match rng.next(4) {
                        0 => (width, height + [0.5, 2.0][rng.next(2)]),
                        1 => (width, (height - [0.5, 1.5][rng.next(2)]).max(7.0)),
                        2 => (width, [14.0, 9.0][rng.next(2)]),
                        _ => ([6.0, 8.27, 9.5][rng.next(3)], height),
                    };
                    // (A size the page has is no step, and a refusal leaves the page as it was: the chain goes on.)
                    let _ = s.execute("page.size", &json!({ "width": w, "height": h }));
                    format!("page.size {w} {h}")
                }
                10 => {
                    let t = id(&s, "T");
                    let _ = s.execute("shape.move", &json!({ "ids": [t], "dx": -0.7905, "dy": -0.6382 }));
                    "move T the composed session's way".to_owned()
                }
                0 => {
                    let (dx, dy) = moves_t[rng.next(moves_t.len())];
                    let t = id(&s, "T");
                    let _ = s.execute("shape.move", &json!({ "ids": [t], "dx": dx, "dy": dy }));
                    format!("move T {dx} {dy}")
                }
                1 => {
                    let (dx, dy) = moves_u[rng.next(moves_u.len())];
                    let u = id(&s, "U");
                    let _ = s.execute("shape.move", &json!({ "ids": [u], "dx": dx, "dy": dy }));
                    format!("move U {dx} {dy}")
                }
                2 => {
                    let (dx, dy) = moves_s[rng.next(moves_s.len())];
                    let sh = id(&s, "S");
                    let _ = s.execute("shape.move", &json!({ "ids": [sh], "dx": dx, "dy": dy }));
                    format!("move S {dx} {dy}")
                }
                3 => {
                    let (w, h) = sizes_t[rng.next(sizes_t.len())];
                    let t = id(&s, "T");
                    let _ = s.execute("shape.size", &json!({ "ids": [t], "width": w, "height": h }));
                    format!("size T {w} {h}")
                }
                4 => {
                    let (leg, to) = paths[rng.next(paths.len())];
                    let k2 = id(&s, "K2");
                    let _ = s.execute("connector.edit_path", &json!({ "id": k2, "leg": leg, "to": to }));
                    format!("edit path {leg} {to}")
                }
                5 => {
                    let k2 = id(&s, "K2");
                    let _ = s.execute("connector.reset", &json!({ "ids": [k2] }));
                    "reset K2".to_owned()
                }
                6 => {
                    let _ = s.execute("edit.undo", &json!({}));
                    "undo".to_owned()
                }
                7 => {
                    let _ = s.execute("edit.redo", &json!({}));
                    "redo".to_owned()
                }
                _ => "save".to_owned(),
            };
            steps.push(step);
            if rng.next(10) < 6 {
                steps.push("save".to_owned());
                if let Ok(saved) = s.execute("file.save", &json!({})) {
                    saves += 1;
                    let step = format!("seed {seed} after {steps:?}");
                    silent.extend(check_told(&s, &dir, "a.odg", &saved, &["K1", "K2"], &mut told, &step));
                    // **A page that changed its height keeps each connector no edit touched as it was, shifted**.
                    silent.extend(seen.check_page_keep(&s, &dir, "a.odg", &saved, &["K1", "K2"], &step));
                    page_keeps += seen.kept;
                    seen.look(&s, &dir, "a.odg", &["K1", "K2"]);
                }
            }
        }
    }
    assert!(saves > 150, "the chains saved {saves} times");
    // The new rule was met: connectors no edit touched went through a change of the page's height in some of the chains.
    assert!(page_keeps > 20, "{page_keeps} connectors were kept through a change of the page's height");
    assert!(silent.is_empty(), "{} silent saves, the first: {:#?}", silent.len(), silent.iter().take(3).collect::<Vec<_>>());
}

// ----: the page's height changes while a route is kept -------------------------------------------------------------------------------------

/// A page cell of page 0 in inches.
pub(crate) fn page_cell(s: &Session, cell: &str) -> f64 {
    s.document().unwrap().pages.get(0).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap()
}

/// `page.size` on page 0.
pub(crate) fn size_page(s: &mut Session, width: f64, height: f64) {
    run(s, "page.size", json!({ "width": width, "height": height }));
}

/// The starts of K2's route as a file read again by the real reader holds them: its first corner, in inches from the page's top left (what the file's `svg:d` fixes).
fn k2_from_the_top(dir: &Dir, file: &str) -> (f64, f64) {
    let (back, _) = read_back(&dir.read(file));
    let height = back.pages.get(0).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap();
    let first = page_corners(&back, "K2")[0];
    (first.0, height - first.1)
}

fn near_point(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 2e-3 && (a.1 - b.1).abs() < 2e-3
}

/// A number as a path writes it: to four decimals, with no trailing zeros.
pub(crate) fn path_number(v: f64) -> String {
    let text = format!("{v:.4}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text };
    if text == "-0" { "0".to_owned() } else { text }
}

/// K2's path in W1's file (`K2_PATH`, absolute points) moved `dh` inches down the page: every `y`, in hundredths of a millimetre, `dh` times 2,540 more.
pub(crate) fn k2_path_shifted(dh: f64) -> String {
    let down = |y: f64| path_number(y + dh * 2540.0);
    format!(
        "M 9744 {} L 9744 {} L 6500 {} L 6500 {} L 9440 {} L 9440 {}",
        down(11192.0),
        down(11692.0),
        down(11692.0),
        down(4000.0),
        down(4000.0),
        down(4300.0)
    )
}

/// The way the hop of K2 goes in the review's repro: a resize of T, a move of T away or back, or undo (as many steps as it takes: the step that gave K2 its hop is the
/// last one when the page was sized first, the one before it when the move came first, which takes the page's change back too).
fn take_the_hop_away(s: &mut Session, way: &str) {
    match way {
        "size T" => size_t(s),
        "move T away" => {
            let t = id_of(s.document().unwrap(), 0, "T");
            run(s, "shape.move", json!({ "ids": [t], "dx": 2.0, "dy": 0.0 }));
        }
        "move T back" => {
            let t = id_of(s.document().unwrap(), 0, "T");
            run(s, "shape.move", json!({ "ids": [t], "dx": 0.7905, "dy": 0.6382 }));
        }
        _ => {
            for _ in 0..2 {
                if k2_has_hop(s) {
                    run(s, "edit.undo", json!({}));
                }
            }
        }
    }
    assert!(!k2_has_hop(s), "{way}: the hop is gone");
}

/// Blocker: the corners kept for a route a save kept are the file's **in the page as the save leaves it**. The file's path stays the same distance
/// from the page's top whatever the page's height does, so on a page that grew by `dh` it reads `dh` higher up (y up) than the corners the baseline holds, which are in the
/// page as it was. The repro: T moved (K2 gets a hop), the page sized, a save (said once), the hop taken away, a save: it was silent and K2 reopened 2.307 inches too high.
/// Sixteen cases: the page grows or shrinks, before or after the move, and the hop goes by a resize of T, a move of T away, a move of T back or undo. (Undo after the
/// move that came first takes the page's change back as well, which leaves the page the file has: those two cases held before the fix, and are the control.) After the
/// second save the route K2 reopens with is the model's, no word is said (the route can be written now), and a save again is no edit.
#[test]
fn a_page_that_grows_or_shrinks_while_a_route_is_kept_is_in_the_page_the_file_has_when_the_hop_goes() {
    let (mut cases, mut bad) = (0, Vec::new());
    for (height, how) in [(14.0, "grows"), (10.0, "shrinks")] {
        for page_first in [true, false] {
            for way in ["size T", "move T away", "move T back", "undo"] {
                let label = format!("the page {how}, {} the move, then {way}", if page_first { "before" } else { "after" });
                let dir = Dir::new();
                let mut s = open_drawing(&dir, &w1_drawing());
                let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
                let from_the_top = k2_from_the_top(&dir, "a.odg");
                if page_first {
                    size_page(&mut s, width, height);
                    move_t(&mut s);
                } else {
                    move_t(&mut s);
                    size_page(&mut s, width, height);
                }
                assert!(k2_has_hop(&s), "{label}: K2 has a hop");
                let mut told = Told::new();
                let first = save(&mut s);
                bad.extend(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, &label));
                assert_eq!(warnings(&first).len(), 1, "{label}: the hop is said: {first}");
                // The file's path goes down the page with the page's change (a height edit keeps `PinY`, so every end moves with it):
                // shifted by what the page grew, its view box and its skew kept. The word said is that the hop has no place in the file.
                let dh = page_cell(&s, "PageHeight") - was;
                let kept = connector_named(&content_of(&dir.read("a.odg")), "K2");
                assert!(
                    kept.contains(&format!("svg:d=\"{}\"", k2_path_shifted(dh))) && kept.contains("draw:line-skew"),
                    "{label}: the file's own path is kept, shifted by {dh}: {kept}"
                );
                assert!(
                    near_point(k2_from_the_top(&dir, "a.odg"), (from_the_top.0, from_the_top.1 + dh)),
                    "{label}: the path went down the page with the page (the page was {was} inches high)"
                );
                take_the_hop_away(&mut s, way);
                let second = save(&mut s);
                bad.extend(warnings(&second).into_iter().map(|w| format!("{label}: the route can be written and the save says: {w}")));
                bad.extend(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, &label));
                // Nothing of K2 is written for the hop going: the file holds the route, shifted by what the page is taller than the file's (an undo that takes the page's
                // change back too puts the file's own bytes back).
                let after = connector_named(&content_of(&dir.read("a.odg")), "K2");
                let dh_now = page_cell(&s, "PageHeight") - was;
                if !after.contains(&format!("svg:d=\"{}\"", k2_path_shifted(dh_now))) || !after.contains("draw:line-skew") {
                    bad.push(format!("{label}: K2 holds {after}, not the file's path shifted by {dh_now} with its skew"));
                }
                let (back, _) = read_back(&dir.read("a.odg"));
                let (model, file) = (page_corners(s.document().unwrap(), "K2"), page_corners(&back, "K2"));
                if !same_route(&model, &file) {
                    bad.push(format!("{label}: K2 reopens with {file:?} for the model's {model:?}"));
                } else {
                    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
                    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 16);
    assert!(bad.is_empty(), "{} findings in {cases} cases: {bad:#?}", bad.len());
}

/// The same entry **carried** across the saves of a page that changes while the hop stays: the first save keeps K2's path, then the page grows and a save keeps it again
/// (the index holds the corners the file's path reads as in the page as that save leaves it), the page shrinks and another save keeps it, and then the hop goes. Each
/// chain of heights ends with the hop gone and the route written in the page it is on. (On the commit before the fix the corners were those of the page the first save left.)
#[test]
fn a_route_kept_through_several_saves_of_a_page_that_changes_is_the_files_in_the_page_of_the_last() {
    let mut bad = Vec::new();
    for heights in [vec![14.0], vec![14.0, 10.0], vec![10.0, 14.0, 12.0], vec![14.0, 14.0, 9.0, 13.0]] {
        for way in ["size T", "move T away", "move T back"] {
            let label = format!("heights {heights:?}, {way}");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &w1_drawing());
            let width = page_cell(&s, "PageWidth");
            let from_the_top = k2_from_the_top(&dir, "a.odg");
            move_t(&mut s);
            assert!(k2_has_hop(&s));
            let mut told = Told::new();
            let first = save(&mut s);
            bad.extend(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, &label));
            let was = page_cell(&s, "PageHeight");
            for (n, height) in heights.iter().enumerate() {
                size_page(&mut s, width, *height);
                let kept = save(&mut s);
                bad.extend(check_told(&s, &dir, "a.odg", &kept, &["K1", "K2"], &mut told, &format!("{label}, save {n}")));
                // The path went down the page with each change of the height: it is where it was from the page's top, plus what the page is taller than the file's.
                assert!(
                    near_point(k2_from_the_top(&dir, "a.odg"), (from_the_top.0, from_the_top.1 + height - was)),
                    "{label}, save {n}: the path went down the page with the page"
                );
            }
            take_the_hop_away(&mut s, way);
            let last = save(&mut s);
            bad.extend(warnings(&last).into_iter().map(|w| format!("{label}: the route can be written and the save says: {w}")));
            bad.extend(check_told(&s, &dir, "a.odg", &last, &["K1", "K2"], &mut told, &label));
            // The hop going writes nothing: the file holds the route, shifted by what the page is taller than the file's, with its skew.
            let end = connector_named(&content_of(&dir.read("a.odg")), "K2");
            let dh = page_cell(&s, "PageHeight") - was;
            if !end.contains(&format!("svg:d=\"{}\"", k2_path_shifted(dh))) || !end.contains("draw:line-skew") {
                bad.push(format!("{label}: K2 holds {end}, not the file's path shifted by {dh} with its skew"));
            }
            let (back, _) = read_back(&dir.read("a.odg"));
            let (model, file) = (page_corners(s.document().unwrap(), "K2"), page_corners(&back, "K2"));
            if same_route(&model, &file) {
                assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
            } else {
                bad.push(format!("{label}: K2 reopens with {file:?} for the model's {model:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} findings: {bad:#?}", bad.len());
}

/// A page whose **width** changes: the page's origin is its top left corner, so the width moves no x and the file's path is where the entry says whatever the width is.
/// The hop stays through a save that sets another width (the path kept) and goes in the next: the route is written, and in the same x.
#[test]
fn a_page_that_changes_its_width_while_a_route_is_kept_needs_no_shift_of_x() {
    for widths in [vec![9.5], vec![6.0], vec![9.5, 6.0, 8.0]] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &w1_drawing());
        let height = page_cell(&s, "PageHeight");
        move_t(&mut s);
        let mut told = Told::new();
        let first = save(&mut s);
        assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, "first"), Vec::<String>::new());
        for (n, width) in widths.iter().enumerate() {
            size_page(&mut s, *width, height);
            let kept = save(&mut s);
            assert_eq!(check_told(&s, &dir, "a.odg", &kept, &["K1", "K2"], &mut told, &format!("width {width}")), Vec::<String>::new(), "save {n}");
            // (No route is routed anew by a width, so nothing is said again: the hop was said by the save before.)
            assert!(warnings(&kept).len() <= 1, "width {width}: {kept}");
            let k2 = connector_named(&content_of(&dir.read("a.odg")), "K2");
            assert!(k2.contains(&format!("svg:d=\"{K2_PATH}\"")), "width {width}: {k2}");
        }
        size_t(&mut s);
        assert!(!k2_has_hop(&s));
        let last = save(&mut s);
        assert_eq!(warnings(&last), Vec::<String>::new(), "{widths:?}: {last}");
        assert_eq!(check_told(&s, &dir, "a.odg", &last, &["K1", "K2"], &mut told, "last"), Vec::<String>::new(), "{widths:?}");
        let (back, _) = read_back(&dir.read("a.odg"));
        assert!(same_route(&page_corners(s.document().unwrap(), "K2"), &page_corners(&back, "K2")), "{widths:?}: K2 reopens with the model's route");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// The gap style's hop is a `MoveTo` in the middle of the route, which the property's `has_hop` took for a corner (a route the writer keeps, and says so of, held to be
/// one it should have written). With the page's jump style a gap (`connector.line_jumps` style 2), T moved gives K2 a gap, not an arc: the helper sees it (and the
/// older `k2_has_hop`, which looks for an `ArcTo`, does not). The save keeps K2's path and says so, the page grows, the hop goes, and the route is written in the page
/// it is on.
#[test]
fn the_gap_styles_hop_is_a_move_to_in_the_middle_of_the_route_and_is_kept_with_a_word_as_an_arc_is() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &w1_drawing());
    assert!(!has_hop(s.document().unwrap(), "K2"), "the file's own rows have no hop");
    run(&mut s, "connector.line_jumps", json!({ "style": 2 }));
    move_t(&mut s);
    assert!(has_hop(s.document().unwrap(), "K2"), "T moved: K1 crosses K2, which has a gap");
    assert!(!k2_has_hop(&s), "a gap is no `ArcTo`");
    let (width, height) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    size_page(&mut s, width, height + 2.0);
    let mut told = Told::new();
    let first = save(&mut s);
    assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, "the gap"), Vec::<String>::new());
    assert_eq!(warnings(&first).len(), 1, "the gap is said: {first}");
    size_t(&mut s);
    assert!(!has_hop(s.document().unwrap(), "K2"), "the gap is gone");
    let second = save(&mut s);
    assert_eq!(warnings(&second), Vec::<String>::new(), "{second}");
    assert_eq!(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, "the gap gone"), Vec::<String>::new());
    let (back, _) = read_back(&dir.read("a.odg"));
    assert!(same_route(&page_corners(s.document().unwrap(), "K2"), &page_corners(&back, "K2")), "K2 reopens with the model's route");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

// ---- the black-box LibreOffice check ----------------------------------------------------------------------------------------------------------------

/// Writes what the vectors save, for the LibreOffice check by hand (`ODG_M1RFIX_DIR/in/*.odg`; the script converts each to an `.odg` and a PDF in a profile of its own
/// and only a missing output file counts as a failure): W1's drawing as it is, saved after the move and the size, and saved between them; W2's drawing with a connector
/// glued to B.
#[test]
#[ignore = "writes files for the LibreOffice check by hand"]
fn lo_write_1() {
    let Some(base) = std::env::var_os("ODG_M1RFIX_DIR") else { return };
    let dir = std::path::Path::new(&base).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let to = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    std::fs::write(to("w1-input.odg"), w1_drawing().bytes()).unwrap();
    std::fs::write(to("w2-input.odg"), w2_drawing().bytes()).unwrap();
    let scratch = Dir::new();
    let mut s = open_drawing(&scratch, &w1_drawing());
    move_t(&mut s);
    size_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-chain.odg") }));
    let mut s = open_drawing(&scratch, &w1_drawing());
    move_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-between-1.odg") }));
    size_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-between-2.odg") }));
    let mut s = open_drawing(&scratch, &w2_drawing());
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    run(&mut s, "file.save_as", json!({ "path": to("w2-glued-to-b.odg") }));
    // The review's Repro 1 (K2 routed anew, a hop, kept with a word, the hop gone: written) after its second save, and Repro 2 after its second; the save
    // of a connector to a shape whose id holds a tab.
    let mut s = open_drawing(&scratch, &w1_drawing());
    let u = id_of(s.document().unwrap(), 0, "U");
    run(&mut s, "shape.move", json!({ "ids": [u], "dx": -0.2, "dy": 0.0 }));
    move_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-repro1-save1.odg") }));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "file.save_as", json!({ "path": to("w1-repro1-save2.odg") }));
    let mut s = open_drawing(&scratch, &w1_drawing());
    move_t(&mut s);
    let k2 = id_of(s.document().unwrap(), 0, "K2");
    run(&mut s, "connector.edit_path", json!({ "id": k2, "leg": 2, "to": 2.3 }));
    run(&mut s, "file.save_as", json!({ "path": to("w1-repro2-save1.odg") }));
    size_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-repro2-save2.odg") }));
    let body = [w2_shape("A", "xml:id=\"id1\" draw:id=\"id1\"", (2.0, 2.0)), w2_shape("C", "draw:id=\"id\t9\"", (12.0, 8.0))].concat();
    let mut s = open_drawing(&scratch, &Drawing::new().page(&body));
    let (a, c) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "C"));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": c } }));
    run(&mut s, "file.save_as", json!({ "path": to("w2-tab-id.odg") }));
    // The re-review's repro (T moved, the page sized taller or shorter, a save that keeps the path; the hop taken away by a resize of T, a save that writes the
    // route in the page it is on), and the same carried across a save of the page's size that keeps it again.
    for (how, height) in [("grow", 14.0), ("shrink", 10.0)] {
        let mut s = open_drawing(&scratch, &w1_drawing());
        let width = page_cell(&s, "PageWidth");
        move_t(&mut s);
        size_page(&mut s, width, height);
        run(&mut s, "file.save_as", json!({ "path": to(&format!("w1-f2-{how}-save1.odg")) }));
        size_t(&mut s);
        run(&mut s, "file.save_as", json!({ "path": to(&format!("w1-f2-{how}-save2.odg")) }));
    }
    let mut s = open_drawing(&scratch, &w1_drawing());
    let width = page_cell(&s, "PageWidth");
    move_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-f2-carried-save1.odg") }));
    size_page(&mut s, width, 14.0);
    run(&mut s, "file.save_as", json!({ "path": to("w1-f2-carried-save2.odg") }));
    size_page(&mut s, width, 12.0);
    run(&mut s, "file.save_as", json!({ "path": to("w1-f2-carried-save3.odg") }));
    size_t(&mut s);
    run(&mut s, "file.save_as", json!({ "path": to("w1-f2-carried-save4.odg") }));
}

/// Writes what the page-height check of saves of W1's drawing, for the LibreOffice check by hand (`ODG_PAGEKEEP_DIR/in/w1-*.odg`; the script converts each in a
/// profile of its own and compares the legs of K2 that LibreOffice draws): K2 holds `draw:line-skew="0cm -0.612cm"`, with which LibreOffice draws a leg 116.22 mm long and
/// without 122.34 mm (the recheck). The drawing as it is, then saved after the page's height set 2 cm greater and 3 cm less, and the chain of three saves.
#[test]
#[ignore = "writes files for the LibreOffice check by hand"]
fn lo_pagekeep_w1_write() {
    let Some(base) = std::env::var_os("ODG_PAGEKEEP_DIR") else { return };
    let dir = std::path::Path::new(&base).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let to = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    std::fs::write(to("w1-input.odg"), w1_drawing().bytes()).unwrap();
    let scratch = Dir::new();
    for (name, by) in [("plus2", 2.0), ("minus3", -3.0)] {
        let mut s = open_drawing(&scratch, &w1_drawing());
        let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
        size_page(&mut s, width, was + by / 2.54);
        run(&mut s, "file.save_as", json!({ "path": to(&format!("w1-{name}.odg")) }));
    }
    let mut s = open_drawing(&scratch, &w1_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    for (n, cm) in [2.0, 5.0, -1.0].iter().enumerate() {
        size_page(&mut s, width, was + cm / 2.54);
        run(&mut s, "file.save_as", json!({ "path": to(&format!("w1-chain-{n}.odg")) }));
    }
}
