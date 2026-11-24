//! A later save wrote again the route of a connector no edit had touched, and a new connector glued to a shape whose id another shape holds named
//! the wrong shape (the milestone-1 rerun's two saves that read back different from what was asked, with no word).
//!
//! **W1** rests on the Connectors rows (`svg:d` and `svg:viewBox`: "regenerate both ... when DiaCraft routes the connector anew", the route
//! kept "until then"; `draw:line-skew`: "remove when DiaCraft routes the connector anew", kept until then) and R9 (an edit touches the connectors glued to the
//! shapes it changed, and nothing else), and on the engine's rule `docs/editing-commands.md`: a connector no command routed keeps its bytes, and a
//! hop, which the `.odg` has no place for, is said ("the route could not be written") and the file's own path kept. **W2** rests: a
//! connector's reference resolves against `xml:id`, else `draw:id`, and a reference that names the wrong shape is never written.
//!
//! Every drawing is built by hand; the numbers are the composed ones (three rectangles 3.877 by 0.807 cm at x 7.805 cm).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Shape, Value};
use diacraft_odg::{Limits, OriginalPackage, SaveNote, ShapeIndex, WriteOptions, Written, write_with};

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, num_of, open, open_bytes, part, real, set_num, shape_mut};
use crate::write_new::{connector, glue_part, push_shape};

/// The page's height, centimetres (`Drawing::a4`).
pub(crate) const PAGE_HEIGHT: f64 = 29.7;

/// A custom shape of rectangle type `name`, `w` by `h` cm at (`x`, `y`), with `xml:id` and `draw:id` both `id`.
pub(crate) fn custom(name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    format!(
        "<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" draw:name=\"{name}\" svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"{w}cm\" svg:height=\"{h}cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\" draw:glue-points=\"10800 0 0 10800 10800 21600 21600 10800\"/></draw:custom-shape>"
    )
}

/// W1's drawing: T, S and U at x 7.805 cm and y 2.381, 3.768 and 10.385 cm; K1 a line from T's bottom to S's top; K2 a standard connector from U's bottom to S's left
/// side with a skew, whose path (`d`, absolute) goes round S's left side. Shape ids 1 to 5 in this order.
pub(crate) fn w1_drawing(d: &str) -> Drawing {
    let frame = |y| (7.805, y, 3.877, 0.807);
    let k1 = "<draw:connector draw:name=\"K1\" draw:type=\"line\" draw:start-shape=\"t\" draw:start-glue-point=\"2\" draw:end-shape=\"s\" draw:end-glue-point=\"4\" svg:x1=\"9.7435cm\" svg:y1=\"3.188cm\" svg:x2=\"9.7435cm\" svg:y2=\"3.768cm\" svg:d=\"M 9744 3188 L 9744 3768\" svg:viewBox=\"0 0 1 581\"><text:p/></draw:connector>";
    let k2 = format!(
        "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"s\" draw:end-glue-point=\"5\" draw:line-skew=\"0cm -0.612cm\" svg:x1=\"9.7435cm\" svg:y1=\"11.192cm\" svg:x2=\"7.805cm\" svg:y2=\"4.1715cm\" svg:d=\"{d}\" svg:viewBox=\"0 0 2501 7633\"><text:p/></draw:connector>"
    );
    Drawing::a4().page(&[custom("T", "t", frame(2.381)), custom("S", "s", frame(3.768)), custom("U", "u", frame(10.385)), k1.to_owned(), k2].concat())
}

/// K2's path as the file has it: a point on the first leg (9.744, 11.5) that a rebuild of the route takes out.
pub(crate) const K2_FILE: &str = "M 9744 11192 L 9744 11500 L 9744 11804 L 7305 11804 L 7305 4172 L 7805 4172";

pub(crate) const K2: u32 = 5;

/// The connector `name`'s start tag in `content`.
pub(crate) fn tag_of<'a>(content: &'a str, name: &str) -> &'a str {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no {name}: {content}"));
    let start = content[..at].rfind('<').unwrap();
    let end = at + content[at..].find('>').unwrap();
    &content[start..=end]
}

/// A point of a page `page_cm` centimetres high, in centimetres from its top left, as the connector's own frame has it (its begin the origin, its x axis toward its end),
/// inches.
pub(crate) fn in_frame_on(shape: &Shape, (x, y): (f64, f64), page_cm: f64) -> (f64, f64) {
    let (bx, by, ex, ey) = (num_of(shape, "BeginX"), num_of(shape, "BeginY"), num_of(shape, "EndX"), num_of(shape, "EndY"));
    let (s, c) = (ey - by).atan2(ex - bx).sin_cos();
    let (dx, dy) = (x / 2.54 - bx, (page_cm - y) / 2.54 - by);
    (dx * c + dy * s, -dx * s + dy * c)
}

pub(crate) fn number(n: f64) -> Cell {
    Cell::constant(Value::from_number(n).unwrap_or_else(|| Value::from_text("0")))
}

/// Gives the connector the rows of a route the engine writes (`Geometry` section 0): `rows` are the types of the rows and the points of the page (cm, y down). An
/// `ArcTo` row has its bow, as a jump's does.
pub(crate) fn set_route(shape: &mut Shape, rows: &[(&str, (f64, f64))]) {
    set_route_on(shape, rows, PAGE_HEIGHT);
}

/// [`set_route`] with the points measured from the top of a page `page_cm` centimetres high.
pub(crate) fn set_route_on(shape: &mut Shape, rows: &[(&str, (f64, f64))], page_cm: f64) {
    let made: Vec<Arc<Row>> = rows
        .iter()
        .enumerate()
        .map(|(i, (kind, point))| {
            let (u, v) = in_frame_on(shape, *point, page_cm);
            let mut row = Row { ix: Some(i as u32 + 1), row_type: Some(Arc::from(*kind)), ..Row::default() };
            row.cells.insert(CellName::new("X"), number(u));
            row.cells.insert(CellName::new("Y"), number(v));
            if *kind == "ArcTo" {
                row.cells.insert(CellName::new("A"), number(0.04));
            }
            Arc::new(row)
        })
        .collect();
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).unwrap();
    Arc::make_mut(section).rows = made.into();
}

/// The route the file's K2 has, without the point on its first leg, and with a jump on its long leg when `hop` says so.
pub(crate) fn engine_route(hop: bool) -> Vec<(&'static str, (f64, f64))> {
    let mut rows = vec![("MoveTo", (9.744, 11.192)), ("LineTo", (9.744, 11.804)), ("LineTo", (7.305, 11.804))];
    if hop {
        rows.extend([("LineTo", (7.305, 8.2)), ("ArcTo", (7.305, 7.9))]);
    }
    rows.extend([("LineTo", (7.305, 4.172)), ("LineTo", (7.805, 4.172))]);
    rows
}

pub(crate) fn save(doc: &Document, from: &Open) -> Written {
    real(doc, &from.bytes, &from.index).unwrap_or_else(|e| panic!("{e}"))
}

/// The save after a save, as the engine makes it: the file and the index the last save gave, the model as it is now.
pub(crate) fn save_again(doc: &Document, last: &Written) -> Written {
    real(doc, &last.bytes, &last.index).unwrap_or_else(|e| panic!("{e}"))
}

pub(crate) fn open_of(w: &Written) -> Open {
    Open { bytes: w.bytes.clone(), doc: open_bytes(w.bytes.clone()).doc, index: ShapeIndex::default() }
}

fn skew_and_path(content: &str) -> (bool, bool) {
    let tag = tag_of(content, "K2");
    (tag.contains("draw:line-skew=\"0cm -0.612cm\""), tag.contains(&format!("svg:d=\"{K2_FILE}\"")))
}

/// W1, the chain: a hop the rebuild made on K2 (the save keeps the file's path and says so), then K1 moved again and the rebuild took the hop away. K2's
/// ends and route are where the file has them: no route is written, no skew goes, and no word is said. Before the fix the second save wrote `svg:d` again and
/// took `draw:line-skew` with it, and the file drew K2's middle leg 6.1 mm elsewhere.
#[test]
fn w1_a_hop_taken_away_by_a_later_edit_leaves_the_connector_no_edit_touched_as_the_file_has_it() {
    let o = open(&w1_drawing(K2_FILE));
    let before = part(&o.bytes, "content.xml");
    // Edit one: the rebuild gives K2 a hop (K1 crosses it). The route has no place for it: K2 keeps its bytes and the save says so.
    let mut doc = o.doc.clone();
    set_route(shape_mut(&mut doc, 0, K2), &engine_route(true));
    let first = save(&doc, &o);
    assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "the hop is said");
    assert_eq!(skew_and_path(&part(&first.bytes, "content.xml")), (true, true), "and K2 keeps its skew and its path");
    // Edit two: K1 moved again, the hop is gone, and the rows are the engine's (the point on the first leg is not among them).
    set_route(shape_mut(&mut doc, 0, K2), &engine_route(false));
    let second = save_again(&doc, &first);
    assert!(!second.edited, "the model's K2 is the file's route: nothing to write ({:?})", second.notes);
    assert!(second.notes.is_empty(), "and nothing to say: {:?}", second.notes);
    assert_eq!(part(&second.bytes, "content.xml"), part(&first.bytes, "content.xml"));
    // The chain without the save between: the file's rows, then the engine's rows with no hop, one save.
    let mut doc = o.doc.clone();
    set_route(shape_mut(&mut doc, 0, K2), &engine_route(false));
    let once = save(&doc, &o);
    assert!(!once.edited && once.notes.is_empty(), "{:?}", once.notes);
    assert_eq!(part(&once.bytes, "content.xml"), before);
}

/// W1 across undo: the hop comes back (the engine's undo puts the rows back) and is said again, then the model is the file's own rows: no route, no word.
#[test]
fn w1_the_hop_that_comes_back_with_an_undo_is_said_and_the_file_s_own_rows_say_nothing() {
    let o = open(&w1_drawing(K2_FILE));
    let mut doc = o.doc.clone();
    set_route(shape_mut(&mut doc, 0, K2), &engine_route(false));
    let plain = save(&doc, &o);
    set_route(shape_mut(&mut doc, 0, K2), &engine_route(true));
    let hopped = save_again(&doc, &plain);
    assert_eq!(hopped.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }]);
    assert_eq!(skew_and_path(&part(&hopped.bytes, "content.xml")), (true, true));
    // Saved again with the hop still in the model: no edit, and nothing said twice.
    let again = save_again(&doc, &hopped);
    assert!(!again.edited && again.notes.is_empty(), "{:?}", again.notes);
    // The model is the file's own rows again (the undo of every edit): no route, no word.
    let mut back = open_of(&hopped);
    let file_rows = back.doc.clone();
    back.doc = file_rows;
    let undone = save_again(&o.doc, &hopped);
    assert!(!undone.edited && undone.notes.is_empty(), "{:?}", undone.notes);
    assert_eq!(skew_and_path(&part(&undone.bytes, "content.xml")), (true, true));
}

/// R8, `svg:d` and `draw:line-skew`: a route that really moved (a corner of K2's own route is elsewhere) is written, and the skew goes with it: the connector
/// is routed anew. The control of the two tests above: the comparison is of the route, not a way to keep every connector's bytes.
#[test]
fn w1_a_route_that_really_moved_is_written_and_takes_the_skew() {
    let o = open(&w1_drawing(K2_FILE));
    let mut doc = o.doc.clone();
    let mut rows = engine_route(false);
    rows[2] = ("LineTo", (7.0, 11.804));
    rows[3] = ("LineTo", (7.0, 4.172));
    set_route(shape_mut(&mut doc, 0, K2), &rows);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let content = part(&saved.bytes, "content.xml");
    let tag = tag_of(&content, "K2");
    assert!(tag.contains("svg:d=\"M 9744 11192 v 612 h -2744 v -7632 h 805\""), "{tag}");
    assert!(!tag.contains("draw:line-skew"), "{tag}");
    // The same model saved again is no edit.
    let again = save_again(&doc, &saved);
    assert!(!again.edited && again.notes.is_empty());
}

/// W2's drawing: A (`id1`), B and C (both `id9`), 3 by 1 cm. Shape ids 1, 2 and 3.
fn w2_drawing(c_id_attributes: &str) -> Drawing {
    let shape = |name: &str, ids: &str, (x, y): (f64, f64)| {
        format!(
            "<draw:custom-shape {ids} draw:name=\"{name}\" svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"3cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
        )
    };
    Drawing::a4().page(
        &[
            shape("A", "xml:id=\"id1\" draw:id=\"id1\"", (2.0, 2.0)),
            shape("B", "xml:id=\"id9\" draw:id=\"id9\"", (12.0, 2.0)),
            shape("C", c_id_attributes, (12.0, 8.0)),
        ]
        .concat(),
    )
}

/// The model with a new connector from A (shape 1) to the shape `to`, glued at both ends to the whole shape.
fn with_connector_to(o: &Open, to: u32) -> Document {
    let mut doc = o.doc.clone();
    let c = o.index.next_id(0).unwrap();
    push_shape(&mut doc, connector(c, (3.5, PAGE_HEIGHT - 3.0, 13.5, PAGE_HEIGHT - 8.5)));
    glue_part(&mut doc, c, true, 1, 3);
    glue_part(&mut doc, c, false, to, 3);
    doc
}

/// What a connector of the page reads back as glued to, by the glued shapes' names (a member of a group is found too): `(begin, end)`.
fn glued_to(bytes: &[u8]) -> (Option<String>, Option<String>) {
    fn find(shapes: &diacraft_doc::ShapeList, id: u32) -> Option<String> {
        shapes.iter().find_map(|s| if s.id == id { s.name.as_deref().map(str::to_owned) } else { find(&s.children, id) })
    }
    let back = open_bytes(bytes.to_vec()).doc;
    let page = back.pages.get(0).unwrap();
    let connector = page.shapes.iter().find(|s| s.sheet.cell("BeginX").is_some()).unwrap().id;
    let name = |end: &str| {
        let row = page.connects.iter().find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(end))?;
        find(&page.shapes, row.to_sheet)
    };
    (name("BeginX"), name("EndX"))
}

/// W2 (the `draw:end-shape` row): a package whose shapes A (`id1`), B and C (both `id9`) are 3 by 1 cm; a new connector from A
/// to C. The reader resolves `id9` to the first shape that holds it, B, so a `draw:end-shape="id9"` would read back glued to B (moving B would move the end,
/// moving C would not). The spec says nothing of what to write for a shape whose id another shape holds, so the save is refused in words and nothing is written.
#[test]
fn w2_a_new_connector_glued_to_a_shape_whose_id_an_earlier_shape_holds_is_refused_in_words() {
    for (case, c_ids) in [
        ("both ids repeated", "xml:id=\"id9\" draw:id=\"id9\""),
        ("the later shape has an xml:id of it only", "xml:id=\"id9\""),
        ("the later shape has a draw:id of it only, the earlier an xml:id", "draw:id=\"id9\""),
    ] {
        let o = open(&w2_drawing(c_ids));
        let doc = with_connector_to(&o, 3);
        let said = match real(&doc, &o.bytes, &o.index) {
            Err(e) => e.to_string(),
            Ok(saved) => panic!("{case}: the save was written and says {:?}: {}", saved.notes, part(&saved.bytes, "content.xml")),
        };
        assert!(said.contains("a connector glued to a shape whose id an earlier shape of the page holds"), "{case}: {said}");
    }
}

/// W2, the cases that are not refused: the first holder of the id (the reader resolves it to that shape: it reads back glued to it); a shape whose id nothing
/// else on its page holds, though another page holds it (the names are the page's); and shapes with ids of their own.
#[test]
fn w2_the_first_holder_of_an_id_and_a_shape_that_nothing_else_on_its_page_holds_are_glued_and_read_back_so() {
    let o = open(&w2_drawing("xml:id=\"id9\" draw:id=\"id9\""));
    let saved = real(&with_connector_to(&o, 2), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(part(&saved.bytes, "content.xml").contains("draw:end-shape=\"id9\""));
    assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("B".to_owned())), "B is the first holder of id9");
    // A repeats nothing: C holds `id3` and the connector is glued to it.
    let o = open(&w2_drawing("xml:id=\"id3\" draw:id=\"id3\""));
    let saved = real(&with_connector_to(&o, 3), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("C".to_owned())));
    // The same id on another page is another page's.
    let two_pages = w2_drawing("xml:id=\"id3\" draw:id=\"id3\"").page(&custom("D", "id1", (1.0, 1.0, 3.0, 1.0)));
    let o = open(&two_pages);
    let saved = real(&with_connector_to(&o, 3), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("C".to_owned())));
    // A shape with no id is given a new one, as before.
    let o = open(&w2_drawing(""));
    let saved = real(&with_connector_to(&o, 3), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("C".to_owned())));
}

/// W2, in the reader's order: the shapes of a group are numbered with the page's, and a shape inside a group holds its id first when the group comes first.
#[test]
fn w2_a_member_of_an_earlier_group_holds_an_id_before_a_later_shape_does() {
    let member = custom("B", "id9", (12.0, 2.0, 3.0, 1.0));
    let later = custom("C", "id9", (12.0, 8.0, 3.0, 1.0));
    let body = [custom("A", "id1", (2.0, 2.0, 3.0, 1.0)), format!("<draw:g draw:name=\"G\">{member}</draw:g>"), later].concat();
    let o = open(&Drawing::a4().page(&body));
    // Shapes 1 to 4: A, the group, its member B, and C.
    let saved = real(&with_connector_to(&o, 3), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("B".to_owned())), "the member holds id9 first");
    let said = real(&with_connector_to(&o, 4), &o.bytes, &o.index).map(|_| String::new()).unwrap_or_else(|e| e.to_string());
    assert!(said.contains("an earlier shape of the page holds"), "C is the second holder: {said}");
}

/// What the save of `m` new connectors, each glued at its begin to one of the `n` shapes of a page (each with an id of its own), costs in bytes of start tags looked at.
fn tag_bytes(n: usize, m: usize) -> u64 {
    let body: String = (1..=n).map(|k| custom(&format!("s{k}"), &format!("k{k}"), (1.0, 1.0, 1.0, 1.0))).collect();
    let mut o = open(&Drawing::a4().page(&body));
    let first = o.index.next_id(0).unwrap();
    for i in 0..m {
        let id = first + i as u32;
        push_shape(&mut o.doc, connector(id, (3.5, PAGE_HEIGHT - 3.0, 13.5, PAGE_HEIGHT - 8.5)));
        glue_part(&mut o.doc, id, true, 1 + i as u32, 3);
    }
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}")).work.attribute_bytes
}

/// W2's cost: the ids of a page are read once, into a map, when a connector is glued to a shape that has one: forty connectors glued to forty of three hundred shapes
/// cost the tags of the page once, and each of them one look at its own shape's tag, and not the page again for each (which would be forty times three hundred tags).
#[test]
fn w2_the_ids_of_a_page_are_read_once_for_all_the_connectors_glued_to_it() {
    let (one, forty) = (tag_bytes(300, 1), tag_bytes(300, 40));
    // The page's tags are looked at at least once (a tag of it is about 260 bytes), and each further connector costs a few looks at its own shape's tag (about 1,300
    // bytes measured): a look at the page again for each would be forty times the page's.
    assert!(one >= 300 * 150, "the page's tags are looked at once: {one}");
    assert!(forty - one <= 39 * 3_000, "each further connector costs a look at its shape's tag, not the page: {one} for one, {forty} for forty");
    assert!(forty >= one, "and more connectors cost no less");
}

/// W2, the first holder is the page's first shape: a connector glued at its begin to it is written, and read back glued to it; glued to the shape that repeats its id, it is
/// refused.
#[test]
fn w2_the_first_shape_of_the_page_holds_its_id_before_the_one_that_repeats_it() {
    let both = |name: &str, y: f64| custom(name, "id9", (12.0, y, 3.0, 1.0));
    let o = open(&Drawing::a4().page(&[both("B", 2.0), both("C", 8.0)].concat()));
    let glue_begin_to = |to: u32| {
        let mut doc = o.doc.clone();
        let c = o.index.next_id(0).unwrap();
        push_shape(&mut doc, connector(c, (13.5, PAGE_HEIGHT - 3.0, 5.0, PAGE_HEIGHT - 4.0)));
        glue_part(&mut doc, c, true, to, 3);
        real(&doc, &o.bytes, &o.index)
    };
    let saved = glue_begin_to(1).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(glued_to(&saved.bytes), (Some("B".to_owned()), None));
    let said = glue_begin_to(2).map(|_| String::new()).unwrap_or_else(|e| e.to_string());
    assert!(said.contains("an earlier shape of the page holds"), "{said}");
}

/// The connector rows: a page that grows (`PageHeight` edited) moves every shape down it, a connector's ends among them, but
/// it is no edit of the connector: no cell of its rows changed, so it keeps its own path, **shifted** by what the page grew, and its skew (it is not routed anew). The rows
/// being the file's own or the engine's spelling of the same corners (a point on a leg left out) makes no difference. (Before a change the connector was written again
/// against the new height and its skew went with it.)
#[test]
fn w1_a_connector_of_a_page_that_grew_keeps_its_own_path_shifted_whatever_its_rows_are() {
    let o = open(&w1_drawing(K2_FILE));
    for (case, doc) in [
        ("the file's own rows", page_taller(&o.doc, 2.0)),
        ("the engine's spelling of the same corners", k2_model(&page_taller(&o.doc, 2.0), &engine_route(false))),
    ] {
        let saved = save(&doc, &o);
        assert!(saved.edited && saved.notes.is_empty(), "{case}: {:?}", saved.notes);
        let content = part(&saved.bytes, "content.xml");
        let tag = tag_of(&content, "K2");
        assert!(tag.contains(&format!("svg:d=\"{}\"", k2_file_shifted(2.0))), "{case}: the path is the file's, 2 cm further down the page: {tag}");
        assert!(
            tag.contains("draw:line-skew=\"0cm -0.612cm\"") && tag.contains("svg:viewBox=\"0 0 2501 7633\""),
            "{case}: skew and view box kept: {tag}"
        );
        // The same model saved again is no edit.
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{case}");
    }
}

/// K2's path in the file (`K2_FILE`: absolute points) moved `by_cm` centimetres down the page: each `y`, in hundredths of a millimetre, `by_cm` times 1,000 more.
pub(crate) fn k2_file_shifted(by_cm: f64) -> String {
    let down = |y: f64| format!("{}", y + by_cm * 1000.0);
    format!(
        "M 9744 {} L 9744 {} L 9744 {} L 7305 {} L 7305 {} L 7805 {}",
        down(11192.0),
        down(11500.0),
        down(11804.0),
        down(11804.0),
        down(4172.0),
        down(4172.0)
    )
}

/// What a save that keeps K2's path through a change of the page's height (said once, when it was kept, and not at the saves after it) must have written, as bytes and
/// as geometry: the file's path shifted `by_cm` down the page with its skew and view box as they were, the ends `by_cm` lower than the file's, **no word**, and a file that
/// reads back with the route the model has without its hop (the one the file holds). `o` is the file the chain began from.
pub(crate) fn assert_k2_went_with_the_page(saved: &Written, o: &Open, by_cm: f64, label: &str) {
    assert!(saved.notes.is_empty(), "{label}: a route kept through a change of the page's height is not named again: {:?}", saved.notes);
    let tag = k2_of(saved);
    assert!(tag.contains(&format!("svg:d=\"{}\"", k2_file_shifted(by_cm))), "{label}: the file's path, shifted {by_cm} cm: {tag}");
    assert!(
        tag.contains("draw:line-skew=\"0cm -0.612cm\"") && tag.contains("svg:viewBox=\"0 0 2501 7633\""),
        "{label}: skew and view box as the file has them: {tag}"
    );
    let cm = |name: &str| -> f64 {
        let key = format!(" {name}=\"");
        let at = tag.find(&key).unwrap_or_else(|| panic!("{label}: no {name}: {tag}")) + key.len();
        tag[at..at + tag[at..].find("cm\"").unwrap_or(0)].parse().unwrap_or_else(|_| panic!("{label}: {name} in {tag}"))
    };
    for (name, was) in [("svg:y1", 11.192), ("svg:y2", 4.1715)] {
        assert!((cm(name) - (was + by_cm)).abs() < 1e-4, "{label}: {name} is {by_cm} cm lower than the file's: {tag}");
    }
    assert!(
        same_corners(&corners_of(&open_of(saved).doc, K2), &corners_of(&k2_on_a_page(&o.doc, by_cm, false), K2)),
        "{label}: the file reads back with the route the model has, without its hop"
    );
}

/// A connector whose ends are one point (the fuzz target's first find on the fix: a route of two rows at one place has no corner to compare by, and was written as
/// `M 0 0 v 0`): respelled, it has the one corner it had and the save writes nothing of it.
#[test]
fn w1_a_connector_whose_ends_are_one_point_is_the_same_route_in_other_rows() {
    let body = [
        custom("A", "a", (2.0, 2.0, 3.0, 1.0)),
        "<draw:connector draw:name=\"Z\" draw:start-shape=\"a\" draw:start-glue-point=\"0\" svg:x1=\"3cm\" svg:y1=\"3cm\" svg:x2=\"3cm\" svg:y2=\"3cm\"><text:p/></draw:connector>".to_owned(),
    ]
    .concat();
    let o = open(&Drawing::a4().page(&body));
    let before = tag_of(&part(&o.bytes, "content.xml"), "Z").to_owned();
    let mut doc = o.doc.clone();
    set_route(shape_mut(&mut doc, 0, 2), &[("MoveTo", (3.0, 3.0)), ("LineTo", (3.0, 3.0))]);
    let saved = save(&doc, &o);
    assert!(!saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    assert_eq!(tag_of(&part(&saved.bytes, "content.xml"), "Z"), before);
}

// ----: the route a save kept is the file's, and the index says so -----------------------------------------------------------------------------

/// The route of K2 in the engine's spelling with its long left leg at `x` cm (the file's is 7.305), and a jump on that leg when `hop` says so.
pub(crate) fn route_at(x: f64, hop: bool) -> Vec<(&'static str, (f64, f64))> {
    let mut rows = vec![("MoveTo", (9.744, 11.192)), ("LineTo", (9.744, 11.804)), ("LineTo", (x, 11.804))];
    if hop {
        rows.extend([("LineTo", (x, 8.2)), ("ArcTo", (x, 7.9))]);
    }
    rows.extend([("LineTo", (x, 4.172)), ("LineTo", (7.805, 4.172))]);
    rows
}

pub(crate) fn k2_model(doc: &Document, rows: &[(&str, (f64, f64))]) -> Document {
    let mut doc = doc.clone();
    set_route(shape_mut(&mut doc, 0, K2), rows);
    doc
}

/// The tag of K2 in a save's `content.xml`, and whether it is the file's own (its skew and its path).
pub(crate) fn k2_of(w: &Written) -> String {
    tag_of(&part(&w.bytes, "content.xml"), "K2").to_owned()
}

pub(crate) fn is_the_files(tag: &str) -> bool {
    tag.contains("draw:line-skew=\"0cm -0.612cm\"") && tag.contains(&format!("svg:d=\"{K2_FILE}\""))
}

/// On the writer: a save keeps the file's path for K2 (its route has a hop and corners the file does not), and says so. The index it gives holds
/// the model as saved, whose corners are the hop-free route the model has: the next save, once the hop is gone, finds the corners the same as that baseline's. They are not
/// **the file's**, which are the old ones: the route is written, and the skew goes. (Before the fix nothing was written and nothing said.)
#[test]
fn w1_a_route_a_save_kept_is_written_by_the_next_save_whose_route_has_no_hop() {
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_model(&o.doc, &route_at(7.0, true)), &o);
    assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }]);
    assert!(is_the_files(&k2_of(&first)), "kept");
    let second = save_again(&k2_model(&o.doc, &route_at(7.0, false)), &first);
    assert!(second.edited && second.notes.is_empty(), "{:?}", second.notes);
    let tag = k2_of(&second);
    assert!(tag.contains("svg:d=\"M 9744 11192 v 612 h -2744 v -7632 h 805\"") && !tag.contains("draw:line-skew"), "written anew: {tag}");
    // The model saved again is no edit, and a respelling of the same route is not written again.
    let again = save_again(&k2_model(&o.doc, &route_at(7.0, false)), &second);
    assert!(!again.edited && again.notes.is_empty());
}

/// The index carries it past a save that does not touch K2: an edit of another shape between the save that kept the route and the one that has no hop.
#[test]
fn w1_the_route_a_save_kept_stays_the_files_across_a_save_of_another_shape() {
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_model(&o.doc, &route_at(7.0, true)), &o);
    let mut other = k2_model(&o.doc, &route_at(7.0, true));
    crate::write_core::move_by(&mut other, 0, 3, 1.0, 0.0);
    let between = save_again(&other, &first);
    assert!(between.edited && between.notes.is_empty(), "{:?}", between.notes);
    assert!(is_the_files(&k2_of(&between)), "K2 is still the file's");
    let mut gone = k2_model(&o.doc, &route_at(7.0, false));
    crate::write_core::move_by(&mut gone, 0, 3, 1.0, 0.0);
    let last = save_again(&gone, &between);
    assert!(last.edited && !k2_of(&last).contains("draw:line-skew"), "written: {}", k2_of(&last));
}

/// A save that keeps the route again (its hop moved, its corners changed once more) leaves the file's route the file's: the corners the file holds are the ones it was
/// opened with. When the route then comes back to those corners, nothing is written: the file has them.
#[test]
fn w1_a_route_kept_twice_is_still_compared_with_the_route_the_file_holds() {
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_model(&o.doc, &route_at(7.0, true)), &o);
    let second = save_again(&k2_model(&o.doc, &route_at(6.5, true)), &first);
    assert_eq!(second.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "kept again, and said again");
    assert!(is_the_files(&k2_of(&second)));
    // The hop goes and the route is the file's again (7.305): the file has it.
    let third = save_again(&k2_model(&o.doc, &route_at(7.305, false)), &second);
    assert!(third.notes.is_empty(), "{:?}", third.notes);
    assert!(is_the_files(&k2_of(&third)), "nothing is written: the file has these corners: {}", k2_of(&third));
}

/// A route written is the file's: the entry goes with it. After K2 is written (the hop gone), a respelling of that same route (a point more on a leg, no hop) is no route
/// to write, whatever the file held before.
#[test]
fn w1_a_route_that_was_written_is_no_longer_compared_with_the_route_it_replaced() {
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_model(&o.doc, &route_at(7.0, true)), &o);
    let second = save_again(&k2_model(&o.doc, &route_at(7.0, false)), &first);
    let written = k2_of(&second);
    assert!(!is_the_files(&written));
    // The same corners with a point more on the first leg.
    let rows = vec![
        ("MoveTo", (9.744, 11.192)),
        ("LineTo", (9.744, 11.5)),
        ("LineTo", (9.744, 11.804)),
        ("LineTo", (7.0, 11.804)),
        ("LineTo", (7.0, 4.172)),
        ("LineTo", (7.805, 4.172)),
    ];
    let third = save_again(&k2_model(&o.doc, &rows), &second);
    assert!(!third.edited && third.notes.is_empty(), "a respelling of what the file holds: {:?}", third.notes);
    assert_eq!(k2_of(&third), written);
}

/// On the writer: a glue target whose id is empty (`draw:id=""`, or `xml:id=""` beside a draw:id that is not, which the reader reads as the empty name) is
/// refused in words, and the first holder of a name is not affected.
#[test]
fn w2_a_target_whose_id_is_empty_is_refused_in_words() {
    for (case, ids) in [
        ("an empty draw:id", "draw:id=\"\""),
        ("an empty xml:id beside a draw:id that is not", "xml:id=\"\" draw:id=\"id5\""),
        ("an empty xml:id alone", "xml:id=\"\""),
    ] {
        let o = open(&w2_drawing(ids));
        let said = real(&with_connector_to(&o, 3), &o.bytes, &o.index).map(|_| String::new()).unwrap_or_else(|e| e.to_string());
        assert!(said.contains("a connector glued to a shape whose id is empty"), "{case}: {said}");
    }
    // Two shapes with empty ids, the connector to the first: refused for the empty name too, not glued to the second.
    let o = open(&Drawing::a4().page(&[custom("A", "id1", (2.0, 2.0, 3.0, 1.0)), custom("B", "", (12.0, 2.0, 3.0, 1.0))].concat()));
    let said = real(&with_connector_to(&o, 2), &o.bytes, &o.index).map(|_| String::new()).unwrap_or_else(|e| e.to_string());
    assert!(said.contains("whose id is empty"), "{said}");
}

/// On the writer: the reference is the name the reader gives the target, so the real reader finds the shape again. A literal tab, line feed or
/// carriage return in the target's id is a space to the reader; a tab by reference stays one and is written as one.
#[test]
fn w2_a_target_whose_id_holds_white_space_is_written_by_the_name_the_reader_gives_it() {
    for (case, ids, written) in [
        ("a tab", "draw:id=\"id\t9\"", "draw:end-shape=\"id 9\""),
        ("a line feed", "draw:id=\"id\n9\"", "draw:end-shape=\"id 9\""),
        ("a carriage return and a line feed", "draw:id=\"id\r\n9\"", "draw:end-shape=\"id 9\""),
        ("a lone carriage return", "draw:id=\"id\r9\"", "draw:end-shape=\"id 9\""),
        ("a tab by reference", "draw:id=\"id&#9;9\"", "draw:end-shape=\"id&#9;9\""),
    ] {
        let o = open(&w2_drawing(ids));
        let saved = real(&with_connector_to(&o, 3), &o.bytes, &o.index).unwrap_or_else(|e| panic!("{case}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(content.contains(written), "{case}: {content}");
        assert_eq!(glued_to(&saved.bytes), (Some("A".to_owned()), Some("C".to_owned())), "{case}: the reader finds the shape");
        let back = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap().1;
        assert!(back.warnings.is_empty(), "{case}: the reader says nothing: {:?}", back.warnings);
    }
}

/// A route kept for a hop **after the ends moved too** (the review's Repro 1: U moved, then T, in one save) is the file's as well: the first save writes K2's new ends
/// and keeps its path, the index holds the corners of the route the file holds, and the save once the hop is gone writes the route the model has.
#[test]
fn w1_a_route_kept_in_a_save_that_moved_the_ends_too_is_the_files_route_as_well() {
    let o = open(&w1_drawing(K2_FILE));
    let mut moved = o.doc.clone();
    {
        let k2 = shape_mut(&mut moved, 0, K2);
        let x = num_of(k2, "BeginX");
        set_num(k2, "BeginX", x - cm(2.0));
    }
    let hop = k2_model(&moved, &route_at(7.0, true));
    let first = save(&hop, &o);
    assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "kept and said");
    let tag = k2_of(&first);
    assert!(tag.contains("draw:line-skew=\"0cm -0.612cm\"") && tag.contains(&format!("svg:d=\"{K2_FILE}\"")), "the path is the file's: {tag}");
    assert!(tag.contains("svg:x1=\"7.7435cm\""), "the ends are written: {tag}");
    // The hop goes; the ends are where the first save wrote them: the route the model has is not the file's, and is written.
    let second = save_again(&k2_model(&moved, &route_at(7.0, false)), &first);
    assert!(second.edited && second.notes.is_empty(), "{:?}", second.notes);
    assert!(!k2_of(&second).contains("draw:line-skew"), "written anew: {}", k2_of(&second));
}

/// What the index keeps of the routes a save kept is charged to the memory a save may hold: a save that keeps K2's path for a hop (one connector, five corners: 144
/// bytes) is refused, in those words, at the one limit below the least that lets it through, and the limit is the one the check of the model's own size left room for.
#[test]
fn w1_the_routes_a_save_kept_are_charged_to_the_memory_a_save_may_hold() {
    let o = open(&w1_drawing(K2_FILE));
    let doc = k2_model(&o.doc, &route_at(7.305, true));
    let with = |limit: u64| {
        let options = WriteOptions { limits: Limits { max_write_memory: limit, ..Limits::default() }, ..WriteOptions::default() };
        write_with(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options)
    };
    let (mut low, mut high) = (0u64, Limits::default().max_write_memory);
    assert!(with(high).is_ok());
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if with(mid).is_ok() { high = mid } else { low = mid }
    }
    let said = match with(high - 1) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the least limit is not the least"),
    };
    assert!(said.contains("memory for the routes a save kept"), "{said}");
}

// ----: the page's height changes while a route is kept -----------------------------------------------------------------------------------------
//
// The file's path is a fixed distance from the page's top (`svg:d` is in hundredths of a millimetre from the top left). (a save
// that grew the page recorded the old page's corners, and the save after it, with the hop gone, found the model's corners the same as those and wrote nothing, with K2
// reopening 2.307 inches too high) raised the corners the index keeps by what the page grew, since the path stayed where it was. **Shifts
// the path instead**: the save moves the file's path down the page with the page's change, so the corners of the route the file holds, measured from the page's bottom, are
// where they were, and these tests hold the same chains to that rule (`write_pagekeep.rs` holds the vectors).

/// The model with page 0's height `cm` centimetres taller (shorter when it is below 0).
pub(crate) fn page_taller(doc: &Document, by_cm: f64) -> Document {
    let mut doc = doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut page.sheet);
    sheet.cells.get_mut("PageHeight").unwrap().value = Value::from_number((PAGE_HEIGHT + by_cm) / 2.54);
    doc
}

/// The model with page 0's width `by_cm` centimetres wider.
pub(crate) fn page_wider(doc: &Document, by_cm: f64) -> Document {
    let mut doc = doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut page.sheet);
    let width = sheet.cell("PageWidth").and_then(|c| c.value.as_ref()?.number()).unwrap();
    sheet.cells.get_mut("PageWidth").unwrap().value = Value::from_number(width + by_cm / 2.54);
    doc
}

/// K2 on a page `by_cm` taller than the page the file has: its model, a hop on the long leg or none, the file's own corners.
pub(crate) fn k2_on_a_page(doc: &Document, by_cm: f64, hop: bool) -> Document {
    k2_model(&page_taller(doc, by_cm), &route_at(7.305, hop))
}

/// The start of K2's path in a save, in hundredths of a millimetre from the page's top left.
pub(crate) fn path_start(w: &Written) -> String {
    let tag = k2_of(w);
    let at = tag.find("svg:d=\"").map(|a| a + 7).unwrap_or(0);
    tag[at..].chars().take_while(|c| *c != 'v' && *c != 'h' && *c != 'l' && *c != 'L' && *c != '"').collect::<String>().trim().to_owned()
}

/// The re-review's repro, on the writer, under the rule: the first save keeps K2's path for a hop on a page that grew (or shrank), saying so, and
/// **shifts it** by what the page grew; the second has the hop gone, the model's corners the file's own as the file was read, and the page as the first save left it. The
/// file's path is where the model's corners are (measured from the page's bottom, in the page of now), so the route is the file's and **nothing is written and nothing
/// said**, the skew staying. The file reads back with the model's route. (Before a change the path stayed, the route was written in the page of now and the skew went.)
#[test]
fn a_route_kept_on_a_page_that_grew_or_shrank_is_the_files_shifted_and_is_not_written_when_the_hop_goes() {
    for by_cm in [2.0, -3.0, 0.5] {
        let o = open(&w1_drawing(K2_FILE));
        let first = save(&k2_on_a_page(&o.doc, by_cm, true), &o);
        assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "{by_cm}: the hop is said");
        let kept = k2_of(&first);
        assert!(
            kept.contains(&format!("svg:d=\"{}\"", k2_file_shifted(by_cm))) && kept.contains("draw:line-skew=\"0cm -0.612cm\""),
            "{by_cm}: the file's own path, shifted by what the page grew, with its skew: {kept}"
        );
        let second = save_again(&k2_on_a_page(&o.doc, by_cm, false), &first);
        assert!(!second.edited && second.notes.is_empty(), "{by_cm}: the file holds the route the model has: {:?}", second.notes);
        assert_eq!(k2_of(&second), kept, "{by_cm}");
        // The real reader finds the route the model has.
        let back = open_of(&second);
        assert!(
            same_corners(&corners_of(&back.doc, K2), &corners_of(&k2_on_a_page(&o.doc, by_cm, false), K2)),
            "{by_cm}: the file reopens with the model's route"
        );
        // The same model saved again is no edit.
        let again = save_again(&k2_on_a_page(&o.doc, by_cm, false), &second);
        assert!(!again.edited && again.notes.is_empty(), "{by_cm}: {:?}", again.notes);
    }
}

/// The corners of K2's route in `doc`, on the page, y up, by the test's own reading of the rows (the points of the rows in the connector's frame): a hop's points and a
/// point on the leg between its two neighbours are no corner (the file holds a point on a leg, and the engine's spelling of the same route leaves it out).
pub(crate) fn corners_of(doc: &Document, id: u32) -> Vec<(f64, f64)> {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    let (bx, by, ex, ey) = (num_of(shape, "BeginX"), num_of(shape, "BeginY"), num_of(shape, "EndX"), num_of(shape, "EndY"));
    let (s, c) = (ey - by).atan2(ex - bx).sin_cos();
    let section = shape.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
    let mut out: Vec<(f64, f64)> = Vec::new();
    for row in section.rows.iter() {
        let at = |n: &str| row.cells.get(n).and_then(|cell| cell.value.as_ref()?.number()).unwrap();
        let p = (bx + at("X") * c - at("Y") * s, by + at("X") * s + at("Y") * c);
        if row.row_type.as_deref() == Some("ArcTo") || out.last().is_some_and(|q| (q.0 - p.0).abs() < 1e-4 && (q.1 - p.1).abs() < 1e-4) {
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

pub(crate) fn same_corners(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 2e-3 && (p.1 - q.1).abs() < 2e-3)
}

/// The corners the index carries are carried **across a later save that changes the page again** while the hop stays (the entry is then not the baseline's but an earlier
/// save's): the second save grows the page and keeps the path again, shifted by the growth, the third shrinks it back some and keeps it, shifted back, and the fourth has the
/// hop gone and **writes nothing**: the file holds the route, in the page of the third save, with its skew.
#[test]
fn a_route_carried_across_a_later_save_that_changes_the_page_is_the_files_shifted_by_each() {
    // 2 cm taller, then 1 cm shorter than that, then no hop: net 1 cm taller.
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_on_a_page(&o.doc, 0.0, true), &o);
    assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }]);
    let second = save_again(&k2_on_a_page(&o.doc, 2.0, true), &first);
    assert!(second.edited, "the page changed");
    assert_k2_went_with_the_page(&second, &o, 2.0, "the page changed and the hop stays: kept, shifted, said no more");
    let third = save_again(&k2_on_a_page(&o.doc, 1.0, true), &second);
    assert!(third.edited, "the page changed again");
    assert_k2_went_with_the_page(&third, &o, 1.0, "and again");
    let fourth = save_again(&k2_on_a_page(&o.doc, 1.0, false), &third);
    assert!(!fourth.edited && fourth.notes.is_empty(), "the file holds the route in the page of the third save: {:?}", fourth.notes);
    assert_eq!(k2_of(&fourth), k2_of(&third));
    assert!(k2_of(&fourth).contains("draw:line-skew=\"0cm -0.612cm\""), "never routed anew: {}", k2_of(&fourth));
    let again = save_again(&k2_on_a_page(&o.doc, 1.0, false), &fourth);
    assert!(!again.edited && again.notes.is_empty(), "{:?}", again.notes);
    // Taken back to the page the file has with the hop still there, the path is the file's own bytes again: the hop goes and nothing is written.
    let o = open(&w1_drawing(K2_FILE));
    let first = save(&k2_on_a_page(&o.doc, 2.0, true), &o);
    let back = save_again(&k2_on_a_page(&o.doc, 0.0, true), &first);
    assert!(back.edited && back.notes.is_empty(), "the hop was said when it was kept: {:?}", back.notes);
    assert!(is_the_files(&k2_of(&back)), "{}", k2_of(&back));
    assert_k2_went_with_the_page(&back, &o, 0.0, "back at the file's height");
    let gone = save_again(&k2_on_a_page(&o.doc, 0.0, false), &back);
    assert!(!gone.edited && gone.notes.is_empty(), "the page is the file's again and so is the route: {:?}", gone.notes);
    assert!(is_the_files(&k2_of(&gone)));
}

/// A page whose **width** grows: no x of the file moves (the page's origin is its top left corner, and `svg:x` is from it), so the corners the index carries are the file's
/// whatever the width, and a hop that goes after the width changed finds the model's route the file's: nothing to write, the file's skew and path stay.
#[test]
fn a_page_that_grows_in_width_needs_no_shift_of_x() {
    for by_cm in [3.0, -2.0] {
        let o = open(&w1_drawing(K2_FILE));
        let first = save(&k2_model(&o.doc, &route_at(7.305, true)), &o);
        assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }]);
        let wide = save_again(&k2_model(&page_wider(&o.doc, by_cm), &route_at(7.305, true)), &first);
        assert!(wide.edited, "{by_cm}: the width is written");
        assert!(is_the_files(&k2_of(&wide)), "{by_cm}: the path is kept");
        let gone = save_again(&k2_model(&page_wider(&o.doc, by_cm), &route_at(7.305, false)), &wide);
        assert!(!gone.edited && gone.notes.is_empty(), "{by_cm}: the file holds this route at any width: {:?}", gone.notes);
        assert!(is_the_files(&k2_of(&gone)), "{by_cm}: {}", k2_of(&gone));
        // The width and the hop in one save: kept the same way, and written when the hop goes after a route that really moved.
        let both = save(&k2_model(&page_wider(&o.doc, by_cm), &route_at(7.305, true)), &o);
        assert_eq!(both.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "{by_cm}");
        let off = save_again(&k2_model(&page_wider(&o.doc, by_cm), &route_at(7.0, false)), &both);
        assert!(off.edited && off.notes.is_empty() && !is_the_files(&k2_of(&off)), "{by_cm}: a route that moved is written: {}", k2_of(&off));
    }
}

/// K2 on a page `by_cm` taller than the file's whose route is at **the distance from the page's top the file's own path had before the save shifted it** (the model's y, up
/// from the bottom, is `by_cm` more than the file's, a route no command makes: a height edit keeps `PinY`), a hop on the long leg or none. The file's shifted path is where
/// [`k2_on_a_page`] has the route; this is a route the file does not hold.
pub(crate) fn k2_where_the_file_draws_it(doc: &Document, by_cm: f64, hop: bool) -> Document {
    let mut doc = page_taller(doc, by_cm);
    set_route_on(shape_mut(&mut doc, 0, K2), &route_at(7.305, hop), PAGE_HEIGHT + by_cm);
    doc
}

/// The corners kept for a route are the file's **where they are**, not only different from the model's (a control at the writer: no command makes this route): a model
/// whose route is where the file's path was **before** the save shifted it (the same distance from the page's top, so `dh` higher up, y up) is **not** the file's, and is
/// written, with the skew gone, where corners left unshifted, or shifted the wrong way, would find it the file's and write nothing (the hand mutants of the sign and of the
/// frame). The route where the shifted path is drawn is the file's and is not written. The page grows or shrinks in the save that keeps the path; and in a later save that
/// keeps it again, with the entry carried; then the hop goes.
#[test]
fn a_route_the_model_puts_where_the_files_path_was_before_the_shift_is_a_route_to_write_and_the_one_where_it_is_now_is_not() {
    for by in [2.0, -3.0] {
        let o = open(&w1_drawing(K2_FILE));
        let first = save(&k2_on_a_page(&o.doc, by, true), &o);
        assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }], "{by}");
        let kept = k2_of(&first);
        let second = save_again(&k2_on_a_page(&o.doc, by, false), &first);
        assert!(!second.edited && second.notes.is_empty(), "{by}: the file holds this route: {:?} {}", second.notes, k2_of(&second));
        assert_eq!(k2_of(&second), kept, "{by}");
        let third = save_again(&k2_where_the_file_draws_it(&o.doc, by, false), &first);
        assert!(third.edited && third.notes.is_empty(), "{by}: this route is not the file's: {:?}", third.notes);
        let tag = k2_of(&third);
        assert_eq!(
            path_start(&third),
            "M 9744 11192",
            "{by}: written in the page of now, at the distance from its top the file's path was read at: {tag}"
        );
        assert!(!tag.contains("draw:line-skew"), "{by}: routed anew: {tag}");
    }
    // Carried: kept in a save with the page as the file has it, kept again in one that changes it, and the hop gone in the next.
    for (by_second, by_third) in [(2.0, 2.0), (-3.0, -3.0), (2.0, -1.0), (-1.0, 0.0)] {
        let o = open(&w1_drawing(K2_FILE));
        let first = save(&k2_on_a_page(&o.doc, 0.0, true), &o);
        let second = save_again(&k2_on_a_page(&o.doc, by_second, true), &first);
        assert_k2_went_with_the_page(&second, &o, by_second, &format!("{by_second} then {by_third}: kept again, and not said again"));
        // A save that changes the page again, with the hop still there, carries the entry once more.
        let kept = save_again(&k2_on_a_page(&o.doc, by_third, true), &second);
        assert_k2_went_with_the_page(&kept, &o, by_third, &format!("{by_second} then {by_third}: carried"));
        let third = save_again(&k2_on_a_page(&o.doc, by_third, false), &kept);
        assert!(!third.edited && third.notes.is_empty(), "{by_second} then {by_third}: {:?} {}", third.notes, k2_of(&third));
        assert_eq!(k2_of(&third), k2_of(&kept), "{by_second} then {by_third}");
        // Where the path was before the shifts is no longer the file's route.
        let moved = save_again(&k2_where_the_file_draws_it(&o.doc, by_third, false), &kept);
        assert!(by_third == 0.0 || (moved.edited && !k2_of(&moved).contains("draw:line-skew")), "{by_second} then {by_third}: {}", k2_of(&moved));
    }
}
