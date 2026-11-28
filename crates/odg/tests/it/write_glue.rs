//! A connector of the file whose glue the model changed is saved in place, glue and all (the rows for `draw:start-shape`, `draw:end-shape`,
//! `draw:start-glue-point` and `draw:end-glue-point`).
//!
//! What the writer does: for each end of a connector of the file that is glued differently than the file has it, it writes the name of the shape the end is glued to (the shape's own
//! `xml:id`, else its `draw:id`, else an id it gives the shape) and, for a connection point, the glue id the point has on the shape (the map); an end that lost its glue has both
//! attributes taken out, and an end glued to the whole of a shape has no glue point. The ends and the path are written as for any connector that moved. Everything else keeps its
//! bytes, and a save that changes no glue writes none.
//!
//! Every case is a function of a *saver*, as in `write_core`: the real writer must pass each, and an echo that gives the file back must fail each (`echo_control_fails_every_glue_case`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Connects, Document, Row, Section, Shape, Value};

use crate::drawing::{Drawing, close, cm};
use crate::write_core::{Saver, echo, echo_says_edited, num_of, open, open_bytes, part, real, set_num, shape_mut};

/// The page's height, centimetres.
const PAGE_H: f64 = 29.7;

/// A `draw:rect` called `name` at (`x`, `y`) centimetres from the page's top left, `w` by `h`, with the id `id` in both attributes when it has one.
fn rect(name: &str, id: Option<&str>, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    let ids = id.map(|i| format!(" xml:id=\"{i}\" draw:id=\"{i}\"")).unwrap_or_default();
    format!("<draw:rect draw:name=\"{name}\"{ids} svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"{w}cm\" svg:height=\"{h}cm\"/>")
}

/// An end of a connector: the shape it names and the glue point it names, if any.
type End<'a> = Option<(&'a str, Option<u32>)>;

/// A `draw:connector` called `name` with its ends at (`x1`, `y1`) and (`x2`, `y2`) centimetres from the page's top left, a straight path, and the glue given.
fn connector(name: &str, start: End<'_>, end: End<'_>, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> String {
    let glue = |label: &str, e: End<'_>| match e {
        Some((shape, Some(point))) => format!(" draw:{label}-shape=\"{shape}\" draw:{label}-glue-point=\"{point}\""),
        Some((shape, None)) => format!(" draw:{label}-shape=\"{shape}\""),
        None => String::new(),
    };
    format!(
        "<draw:connector draw:name=\"{name}\"{}{} svg:x1=\"{x1}cm\" svg:y1=\"{y1}cm\" svg:x2=\"{x2}cm\" svg:y2=\"{y2}cm\" svg:d=\"M {} {} L {} {}\" svg:viewBox=\"0 0 {} {}\"/>",
        glue("start", start),
        glue("end", end),
        x1 * 1000.0,
        y1 * 1000.0,
        x2 * 1000.0,
        y2 * 1000.0,
        (x1.max(x2) - x1.min(x2)) * 1000.0 + 1.0,
        (y1.max(y2) - y1.min(y2)) * 1000.0 + 1.0
    )
}

/// The drawing every case starts from: A (`a`) at (2, 3), B (`b`) at (12, 3) and D (no id) at (12, 10), each 4 cm by 2 cm, and connector `c` from A's right side to B's left side.
/// Their `ID`s are 1, 2, 3 and 4. The glue ids of a rectangle: 0 top, 1 right, 2 bottom, 3 left.
fn three() -> Drawing {
    Drawing::a4().page(&format!(
        "{}{}{}{}",
        rect("A", Some("a"), (2.0, 3.0, 4.0, 2.0)),
        rect("B", Some("b"), (12.0, 3.0, 4.0, 2.0)),
        rect("D", None, (12.0, 10.0, 4.0, 2.0)),
        connector("c", Some(("a", Some(1))), Some(("b", Some(3))), (6.0, 4.0, 12.0, 4.0)),
    ))
}

const A: u32 = 1;
const B: u32 = 2;
const D: u32 = 3;
const C: u32 = 4;

/// A cell with `formula` (when given) and the number `inches`, in the unit of `like`.
fn cell_like(like: &Cell, formula: Option<&str>, inches: f64) -> Cell {
    let mut cell = Cell::read(formula, Some(&inches.to_string()), None, None, &mut 10_000);
    cell.unit = like.unit.clone();
    cell
}

/// An end to glue as the engine's `connector.reconnect` leaves the model: the connector, which end (the begin when true), what it goes to (`Some((shape, part))`: 3 for the whole
/// shape, 100 and the row for a point; none for a free end) and the place the end is at, centimetres from the page's lower left.
type Glued = (u32, bool, Option<(u32, u32)>, (f64, f64));

/// Glues the end of connector `c` (the begin when `begin`) to `to` as the engine's `connector.reconnect` leaves the model: the old `Connect` row of the end replaced by one that names the
/// part (`Some((shape, part))`: 3 for the whole shape, 100 and the row for a point), or none (a free end), and the end's two cells holding the glue's formula for a point, or the place as plain values,
/// with `at` (centimetres from the page's lower left) as the place.
fn glue_end(doc: &mut Document, c: u32, begin: bool, to: Option<(u32, u32)>, at: (f64, f64)) {
    glue_ends(doc, &[(c, begin, to, at)]);
}

/// [`glue_end`] for many ends in one pass over the page (a thousand ends are not a thousand passes).
fn glue_ends(doc: &mut Document, edits: &[Glued]) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let name_of = |begin: bool| if begin { "BeginX" } else { "EndX" };
    let touched: std::collections::HashSet<(u32, &str)> = edits.iter().map(|(c, begin, _, _)| (*c, name_of(*begin))).collect();
    let mut rows: Vec<Connect> =
        page.connects.iter().filter(|r| !touched.contains(&(r.from_sheet, r.from_cell.as_deref().unwrap_or_default()))).cloned().collect();
    for &(c, begin, to, _) in edits {
        if let Some((shape, part)) = to {
            let to_cell = if part == 3 { "PinX".to_owned() } else { format!("Connections.X{}", part - 100 + 1) };
            rows.push(Connect {
                from_sheet: c,
                from_cell: Some(Arc::from(name_of(begin))),
                from_part: Some(if begin { 9 } else { 12 }),
                to_sheet: shape,
                to_cell: Some(Arc::from(to_cell)),
                to_part: Some(part),
                ..Connect::default()
            });
        }
    }
    page.connects = Connects::from_vec(rows);
    let at_of: std::collections::HashMap<u32, usize> = page.shapes.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    for &(c, begin, to, at) in edits {
        let connector = Arc::make_mut(page.shapes.get_mut(*at_of.get(&c).unwrap()).unwrap());
        let sheet = Arc::make_mut(&mut connector.sheet);
        let own: [&str; 2] = if begin { ["BeginX", "BeginY"] } else { ["EndX", "EndY"] };
        for (name, inches) in [(own[0], cm(at.0)), (own[1], cm(at.1))] {
            let like = sheet.cells.get(name).cloned().unwrap_or_default();
            let formula = match to {
                Some((shape, part)) if part >= 100 => {
                    let n = part - 100 + 1;
                    Some(format!("PAR(PNT(Sheet.{shape}!Connections.X{n},Sheet.{shape}!Connections.Y{n}))"))
                }
                _ => None,
            };
            sheet.cells.insert(CellName::new(name), cell_like(&like, formula.as_deref(), inches));
        }
    }
}

/// The straight route of connector `c` between its ends as the model has them now (centimetres from the page's lower left): the frame cells' values and `Geometry` section 0, as the
/// engine's calculation and routing leave them for a straight connector.
fn straight(doc: &mut Document, c: u32, [(bx, by), (ex, ey)]: [(f64, f64); 2]) {
    let length = (ex - bx).hypot(ey - by);
    let connector = shape_mut(doc, 0, c);
    for (name, value) in [
        ("BeginX", cm(bx)),
        ("BeginY", cm(by)),
        ("EndX", cm(ex)),
        ("EndY", cm(ey)),
        ("Width", cm(length)),
        ("LocPinX", cm(length) / 2.0),
        ("PinX", cm((bx + ex) / 2.0)),
        ("PinY", cm((by + ey) / 2.0)),
        ("Angle", (ey - by).atan2(ex - bx)),
    ] {
        set_num(connector, name, value);
    }
    let mut geometry = Section::new("Geometry", Some(0));
    for (i, (kind, x)) in [("MoveTo", 0.0), ("LineTo", cm(length))].into_iter().enumerate() {
        let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(i as u32 + 1), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_number(x).unwrap()).with_unit("IN"));
        row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_number(0.0).unwrap()).with_unit("IN"));
        geometry.rows.push(Arc::new(row));
    }
    let sheet = Arc::make_mut(&mut connector.sheet);
    sheet.sections.retain(|s| !(s.name.as_str() == "Geometry" && s.ix == Some(0)));
    sheet.sections.push(Arc::new(geometry));
}

/// A rectangle's connection point `glue_id`, in centimetres from the page's lower left, given the rectangle's top-left corner and size from the page's top left.
fn point_of((x, y, w, h): (f64, f64, f64, f64), glue_id: u32) -> (f64, f64) {
    let (cx, cy) = (x + w / 2.0, PAGE_H - y - h / 2.0);
    match glue_id {
        0 => (cx, PAGE_H - y),
        1 => (x + w, cy),
        2 => (cx, PAGE_H - y - h),
        _ => (x, cy),
    }
}

const FRAME_A: (f64, f64, f64, f64) = (2.0, 3.0, 4.0, 2.0);
const FRAME_B: (f64, f64, f64, f64) = (12.0, 3.0, 4.0, 2.0);
const FRAME_D: (f64, f64, f64, f64) = (12.0, 10.0, 4.0, 2.0);

/// The glue of page 0 as `(connector, end, target, part)`, sorted: the `Connect` rows as a person would list them.
fn glue_list(doc: &Document) -> Vec<(u32, &'static str, u32, u32)> {
    let mut list: Vec<(u32, &'static str, u32, u32)> = doc
        .pages
        .get(0)
        .unwrap()
        .connects
        .iter()
        .map(|r| {
            let end = if r.from_cell.as_deref().is_some_and(|c| c.starts_with("Begin")) { "begin" } else { "end" };
            (r.from_sheet, end, r.to_sheet, r.to_part.unwrap_or(0))
        })
        .collect();
    list.sort_unstable();
    list
}

/// The start tag of the `draw:connector` named `name` in the part.
fn tag_of<'a>(content: &'a str, name: &str) -> &'a str {
    let at = content.find(&format!("draw:name=\"{name}\"")).and_then(|at| content[..at].rfind("<draw:connector ")).expect("the connector");
    &content[at..at + content[at..].find('>').expect("its end")]
}

/// The value of attribute `name` on a start tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// A length written in centimetres, as a number.
fn cms(text: &str) -> f64 {
    text.strip_suffix("cm").unwrap_or_else(|| panic!("a length in centimetres: {text}")).parse().unwrap()
}

/// The saved content and the drawing as read from it.
fn saved(save: &Saver, o: &crate::write_core::Open) -> (String, crate::write_core::Open) {
    let written = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    assert!(written.edited, "a glue that changed is an edit");
    (part(&written.bytes, "content.xml"), open_bytes(written.bytes))
}

#[track_caller]
fn same_cells(a: &Document, b: &Document, ids: &[u32], names: &[&str]) {
    for id in ids {
        for name in names {
            let (x, y) = (shape_of(a, *id), shape_of(b, *id));
            close(num_of(x, name), num_of(y, name));
        }
    }
}

fn shape_of(doc: &Document, id: u32) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).map(|s| s.as_ref()).unwrap()
}

/// An end reconnected to another shape's connection point is written glued to it: the name of the shape (which has no id, so it is given one), the glue id of the point, the end
/// and the path, and the file reads back with the glue the model has.
fn reconnect_to_a_point(save: &Saver) {
    let mut o = open(&three());
    assert_eq!(glue_list(&o.doc), [(C, "begin", A, 103), (C, "end", B, 101)]);
    // D's left side (glue id 3, row 1): the end of c goes there, from B's.
    let to = point_of(FRAME_D, 3);
    glue_end(&mut o.doc, C, false, Some((D, 101)), to);
    straight(&mut o.doc, C, [point_of(FRAME_A, 1), to]);
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:start-shape").as_deref(), Some("a"), "the begin is as it was: {tag}");
    assert_eq!(attr(tag, "draw:start-glue-point").as_deref(), Some("1"), "{tag}");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("id1"), "the end names D by the id it is given: {tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("3"), "{tag}");
    assert_eq!(
        (content.matches(r#"xml:id="id1""#).count(), content.matches(r#"draw:id="id1""#).count()),
        (1, 1),
        "D is given the id once: {content}"
    );
    close(cms(&attr(tag, "svg:x2").unwrap()), 12.0);
    close(cms(&attr(tag, "svg:y2").unwrap()), 11.0);
    assert!(attr(tag, "svg:d").unwrap().starts_with("M 6000 4000"), "{tag}");
    assert_eq!(glue_list(&back.doc), [(C, "begin", A, 103), (C, "end", D, 101)], "the file reads back with the glue the model has");
    same_cells(&o.doc, &back.doc, &[C], &["BeginX", "BeginY", "EndX", "EndY"]);
}

/// A move of the end onto the same shape's other point rewrites the glue id in place.
fn reconnect_to_another_point_of_the_same_shape(save: &Saver) {
    let mut o = open(&three());
    let to = point_of(FRAME_B, 0);
    glue_end(&mut o.doc, C, false, Some((B, 100)), to);
    straight(&mut o.doc, C, [point_of(FRAME_A, 1), to]);
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("b"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("0"), "B's top is glue id 0: {tag}");
    assert_eq!(glue_list(&back.doc), [(C, "begin", A, 103), (C, "end", B, 100)]);
}

/// An end taken off its shape is written free: both attributes go, and the end stays where it is.
fn unglue_an_end(save: &Saver) {
    let mut o = open(&three());
    glue_end(&mut o.doc, C, false, None, (12.0, 4.0));
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape"), None, "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point"), None, "{tag}");
    assert_eq!(attr(tag, "draw:start-shape").as_deref(), Some("a"), "{tag}");
    assert_eq!(glue_list(&back.doc), [(C, "begin", A, 103)]);
}

/// A free end glued anew to the whole of a shape: the shape's name and no glue point.
fn free_end_glued_to_a_whole_shape(save: &Saver) {
    let d = Drawing::a4().page(&format!(
        "{}{}{}",
        rect("A", Some("a"), FRAME_A),
        rect("B", Some("b"), FRAME_B),
        connector("c", Some(("a", Some(1))), None, (6.0, 4.0, 12.0, 4.0)),
    ));
    let mut o = open(&d);
    assert_eq!(glue_list(&o.doc), [(3, "begin", A, 103)]);
    glue_end(&mut o.doc, 3, false, Some((B, 3)), (12.0, PAGE_H - 4.0));
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("b"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point"), None, "the whole shape has no glue point: {tag}");
    assert_eq!(glue_list(&back.doc), [(3, "begin", A, 103), (3, "end", B, 3)]);
}

/// A point of a shape's glue, then the whole of the shape: the glue point attribute goes and the shape's name stays.
fn a_point_end_made_a_whole_shape_end(save: &Saver) {
    let mut o = open(&three());
    glue_end(&mut o.doc, C, false, Some((B, 3)), point_of(FRAME_B, 3));
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("b"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point"), None, "{tag}");
    assert_eq!(glue_list(&back.doc), [(C, "begin", A, 103), (C, "end", B, 3)]);
}

/// Reverse Ends: the begin and the end swap, with their glue, their places and their route.
fn reverse_the_ends(save: &Saver) {
    let mut o = open(&three());
    let (a, b) = (point_of(FRAME_A, 1), point_of(FRAME_B, 3));
    // begin -> B's left side (row 1, part 101), end -> A's right side (row 3, part 103).
    glue_end(&mut o.doc, C, true, Some((B, 101)), b);
    glue_end(&mut o.doc, C, false, Some((A, 103)), a);
    straight(&mut o.doc, C, [b, a]);
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:start-shape").as_deref(), Some("b"), "{tag}");
    assert_eq!(attr(tag, "draw:start-glue-point").as_deref(), Some("3"), "{tag}");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("a"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("1"), "{tag}");
    close(cms(&attr(tag, "svg:x1").unwrap()), 12.0);
    close(cms(&attr(tag, "svg:x2").unwrap()), 6.0);
    assert_eq!(glue_list(&back.doc), [(C, "begin", B, 101), (C, "end", A, 103)]);
    same_cells(&o.doc, &back.doc, &[C], &["BeginX", "BeginY", "EndX", "EndY"]);
}

/// Two connectors glued to a shape that has no id name it by one id, which the shape is given once.
fn two_connectors_to_one_unnamed_shape(save: &Saver) {
    let d = Drawing::a4().page(&format!(
        "{}{}{}{}",
        rect("A", Some("a"), FRAME_A),
        rect("D", None, FRAME_D),
        connector("c1", Some(("a", Some(1))), None, (6.0, 4.0, 9.0, 4.0)),
        connector("c2", Some(("a", Some(2))), None, (4.0, 5.0, 4.0, 8.0)),
    ));
    let mut o = open(&d);
    let (c1, c2, shape_d) = (3, 4, 2);
    glue_end(&mut o.doc, c1, false, Some((shape_d, 103)), point_of(FRAME_D, 1));
    glue_end(&mut o.doc, c2, false, Some((shape_d, 101)), point_of(FRAME_D, 3));
    let (content, back) = saved(save, &o);
    assert_eq!(content.matches("draw:id=\"id1\"").count(), 1, "the shape is given its id once: {content}");
    for name in ["c1", "c2"] {
        assert_eq!(attr(tag_of(&content, name), "draw:end-shape").as_deref(), Some("id1"), "{name}");
    }
    assert_eq!(attr(tag_of(&content, "c1"), "draw:end-glue-point").as_deref(), Some("1"));
    assert_eq!(attr(tag_of(&content, "c2"), "draw:end-glue-point").as_deref(), Some("3"));
    assert_eq!(glue_list(&back.doc).len(), 4);
}

/// Both ends of a connector given other glue in one save, one of them free now.
fn one_end_free_and_the_other_to_a_new_shape_in_one_save(save: &Saver) {
    let mut o = open(&three());
    glue_end(&mut o.doc, C, true, None, point_of(FRAME_A, 1));
    glue_end(&mut o.doc, C, false, Some((D, 100)), point_of(FRAME_D, 0));
    straight(&mut o.doc, C, [point_of(FRAME_A, 1), point_of(FRAME_D, 0)]);
    let (content, back) = saved(save, &o);
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:start-shape"), None, "{tag}");
    assert_eq!(attr(tag, "draw:start-glue-point"), None, "{tag}");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("id1"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("0"), "{tag}");
    assert_eq!(glue_list(&back.doc), [(C, "end", D, 100)]);
}

/// A case of the table below: its name and the function that runs it with a saver.
type Case = (&'static str, fn(&Saver));

const CASES: [Case; 8] = [
    ("an end reconnected to a point of another shape", reconnect_to_a_point),
    ("an end moved to another point of its shape", reconnect_to_another_point_of_the_same_shape),
    ("an end taken off its shape", unglue_an_end),
    ("a free end glued to the whole of a shape", free_end_glued_to_a_whole_shape),
    ("a point end made a whole-shape end", a_point_end_made_a_whole_shape_end),
    ("the ends reversed", reverse_the_ends),
    ("two connectors to one shape that has no id", two_connectors_to_one_unnamed_shape),
    ("one end free and the other glued anew", one_end_free_and_the_other_to_a_new_shape_in_one_save),
];

macro_rules! case_test {
    ($name:ident, $case:ident) => {
        #[test]
        fn $name() {
            $case(&real);
        }
    };
}

case_test!(an_end_reconnected_to_a_point_of_another_shape_is_written_glued_to_it, reconnect_to_a_point);
case_test!(an_end_moved_to_another_point_of_its_shape_has_its_glue_id_written_in_place, reconnect_to_another_point_of_the_same_shape);
case_test!(an_end_taken_off_its_shape_is_written_free, unglue_an_end);
case_test!(a_free_end_glued_to_the_whole_of_a_shape_names_the_shape_and_no_point, free_end_glued_to_a_whole_shape);
case_test!(a_point_end_made_a_whole_shape_end_loses_its_glue_point_and_keeps_its_shape, a_point_end_made_a_whole_shape_end);
case_test!(reversed_ends_swap_their_glue_their_places_and_their_route, reverse_the_ends);
case_test!(two_connectors_glued_to_a_shape_with_no_id_name_it_by_one_id, two_connectors_to_one_unnamed_shape);
case_test!(one_end_taken_off_and_the_other_glued_anew_in_one_save, one_end_free_and_the_other_to_a_new_shape_in_one_save);

/// The control: an echo, which gives the file back, fails every case, with or without saying it edited.
#[test]
fn echo_control_fails_every_glue_case() {
    for (name, case) in CASES {
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo))).is_err(), "the echo writer passed the case {name}: the case checks nothing");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
    }
}

/// A save that changes no glue writes none: a move of a shape a connector is glued to leaves the glue attributes with the bytes they had, and the unedited file is the file.
#[test]
fn a_save_that_changes_no_glue_leaves_the_glue_attributes_alone() {
    let mut o = open(&three());
    let unedited = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!unedited.edited && unedited.bytes == o.bytes, "an unedited save is the file");
    let (b_to, c_to) = (point_of((14.0, 3.0, 4.0, 2.0), 3), point_of(FRAME_A, 1));
    crate::write_core::move_by(&mut o.doc, 0, B, 2.0, 0.0);
    glue_end(&mut o.doc, C, false, Some((B, 101)), b_to);
    straight(&mut o.doc, C, [c_to, b_to]);
    let content = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:start-shape").as_deref(), Some("a"));
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("b"));
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("3"));
    assert_eq!(content.matches("xml:id=").count(), 2, "no id was given: {content}");
}

/// A `draw:custom-shape` called `name` with `xml:id` `id`, 4 cm by 2 cm at (`x`, `y`), whose enhanced geometry is `geometry` (attributes, verbatim) and whose style is `style`.
fn custom_shape(name: &str, id: &str, (x, y): (f64, f64), style: &str, geometry: &str) -> String {
    format!(
        "<draw:custom-shape{style} draw:name=\"{name}\" xml:id=\"{id}\" draw:id=\"{id}\" svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"4cm\" svg:height=\"2cm\"><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"{geometry}/></draw:custom-shape>"
    )
}

/// A line is no connector: the model gluing a `draw:line` to a shape cannot be written, and is refused in words with nothing written.
#[test]
fn a_line_the_model_glued_is_refused_in_words() {
    let d = Drawing::a4()
        .page(&format!("{}<draw:line draw:name=\"l\" svg:x1=\"6cm\" svg:y1=\"4cm\" svg:x2=\"9cm\" svg:y2=\"4cm\"/>", rect("A", Some("a"), FRAME_A)));
    let mut o = open(&d);
    // The line is shape 2; its end glued to A's right side.
    glue_end(&mut o.doc, 2, false, Some((A, 103)), (9.0, 4.0));
    match real(&o.doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(words.contains("a line glued to a shape"), "{words}"),
        other => panic!("a glued line was not refused in words: {:?}", other.map(|w| w.edited)),
    }
}

/// The other end of a connector whose one end was glued anew keeps the rule: a formula set on it by hand is refused in words, as it is beside no change of glue at all.
#[test]
fn a_formula_on_the_end_whose_glue_is_as_it_was_is_still_refused_beside_a_changed_glue() {
    let mut o = open(&three());
    glue_end(&mut o.doc, C, true, None, point_of(FRAME_A, 1));
    {
        let connector = shape_mut(&mut o.doc, 0, C);
        let sheet = Arc::make_mut(&mut connector.sheet);
        let like = sheet.cells.get("EndX").cloned().unwrap_or_default();
        sheet.cells.insert(CellName::new("EndX"), cell_like(&like, Some("BeginX+1"), cm(12.0)));
    }
    match real(&o.doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => {
            assert!(words.contains("a formula or another attribute set on the end of a connector"), "{words}")
        }
        other => panic!("a formula on an end whose glue is as it was was not refused: {:?}", other.map(|w| w.edited)),
    }
}

/// A custom shape whose style says its connection points are its own (`diacraft:glue="own"`) numbers them from 4 at its first listed point: a connector glued to its row 2 names point 6.
#[test]
fn a_connector_glued_to_a_shape_whose_points_are_its_own_names_the_row_four_on() {
    let own = r#" draw:style-name="gr9""#;
    let geometry = r#" draw:glue-points="10800 0 0 10800 10800 21600 21600 10800""#;
    let d = Drawing::a4()
        .auto(r#"<style:style style:name="gr9" style:family="graphic"><style:graphic-properties diacraft:glue="own"/></style:style>"#)
        .page(&format!(
            "{}{}{}",
            rect("A", Some("a"), FRAME_A),
            custom_shape("T", "t", (12.0, 10.0), own, geometry),
            connector("c", Some(("a", Some(1))), None, (6.0, 4.0, 14.0, 12.0)),
        ));
    let mut o = open(&d);
    let target = 2;
    glue_end(&mut o.doc, 3, false, Some((target, 102)), point_of(FRAME_D, 2));
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("t"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("6"), "row 2 of a shape with its own points is point 6: {tag}");
    assert_eq!(glue_list(&open_bytes(written.bytes).doc), [(3, "begin", A, 103), (3, "end", target, 102)]);
}

/// The standard points of a shape that is mirrored once are numbered across the frame as LibreOffice draws it: row 1 is point 1 there, and point 3 on a shape that is not mirrored.
#[test]
fn a_connector_glued_to_a_shape_mirrored_once_names_the_point_the_file_numbers_so() {
    for (geometry, row_1_is) in [("", "3"), (r#" draw:mirror-horizontal="true""#, "1")] {
        let d = Drawing::a4().page(&format!(
            "{}{}{}",
            rect("A", Some("a"), FRAME_A),
            custom_shape("T", "t", (12.0, 10.0), "", geometry),
            connector("c", Some(("a", Some(1))), None, (6.0, 4.0, 12.0, 11.0)),
        ));
        let mut o = open(&d);
        glue_end(&mut o.doc, 3, false, Some((2, 101)), point_of(FRAME_D, 3));
        let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{geometry}: the save was refused: {e}"));
        let content = part(&written.bytes, "content.xml");
        assert_eq!(attr(tag_of(&content, "c"), "draw:end-glue-point").as_deref(), Some(row_1_is), "{geometry}: {content}");
        assert_eq!(glue_list(&open_bytes(written.bytes).doc), [(3, "begin", A, 103), (3, "end", 2, 101)], "{geometry}");
    }
}

/// A connector a group holds, glued to a shape another group holds: the writer names both by their ids wherever they are on the page.
#[test]
fn a_connector_inside_a_group_is_glued_to_a_shape_inside_another() {
    let d = Drawing::a4().page(&format!(
        "<draw:g>{}{}</draw:g><draw:g>{}</draw:g>",
        rect("A", Some("a"), FRAME_A),
        connector("c", Some(("a", Some(1))), None, (6.0, 4.0, 9.0, 4.0)),
        rect("D", None, FRAME_D),
    ));
    let mut o = open(&d);
    let (c, shape_d) =
        (crate::drawing::shape(&o.doc, 0, 0).children.get(1).unwrap().id, crate::drawing::shape(&o.doc, 0, 1).children.get(0).unwrap().id);
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        // The engine glues a member the same way; the places do not matter to the names.
        let mut rows: Vec<Connect> = page.connects.iter().cloned().collect();
        rows.push(Connect {
            from_sheet: c,
            from_cell: Some(Arc::from("EndX")),
            from_part: Some(12),
            to_sheet: shape_d,
            to_cell: Some(Arc::from("Connections.X2")),
            to_part: Some(101),
            ..Connect::default()
        });
        page.connects = Connects::from_vec(rows);
    }
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    let tag = tag_of(&content, "c");
    assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("id1"), "{tag}");
    assert_eq!(attr(tag, "draw:end-glue-point").as_deref(), Some("3"), "{tag}");
}

/// A shape whose id another shape of the page holds first cannot be named by it (the reader would resolve it to the first): refused in words, as for a new connector.
#[test]
fn a_connector_glued_to_a_shape_whose_id_an_earlier_shape_holds_is_refused_in_words() {
    let d = Drawing::a4().page(&format!(
        "{}{}{}{}",
        rect("A", Some("a"), FRAME_A),
        rect("B", Some("a"), FRAME_B),
        rect("D", Some("d"), FRAME_D),
        connector("c", Some(("a", Some(1))), None, (6.0, 4.0, 12.0, 4.0)),
    ));
    let mut o = open(&d);
    // The connector is shape 4 and B (shape 2) holds the id `a` second.
    glue_end(&mut o.doc, 4, false, Some((2, 101)), point_of(FRAME_B, 3));
    match real(&o.doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(words.contains("an earlier shape of the page holds"), "{words}"),
        other => panic!("an end glued to a second holder of an id was not refused: {:?}", other.map(|w| w.edited)),
    }
}

/// A drawing of A (`a`), B (`b`) and D (no id), and `n` connectors named `c0` to `c<n-1>`, each from A's right side to B's left side, one below the other. Their `ID`s are 4 on.
fn many(n: usize) -> Drawing {
    let mut body = format!("{}{}{}", rect("A", Some("a"), FRAME_A), rect("B", Some("b"), FRAME_B), rect("D", None, FRAME_D));
    for k in 0..n {
        body.push_str(&connector(&format!("c{k}"), Some(("a", Some(1))), Some(("b", Some(3))), (6.0, 4.0, 12.0, 4.0)));
    }
    Drawing::a4().page(&body)
}

/// The figures of one save that the cost of the glue is held to.
struct Cost {
    attributes: u64,
    steps: u64,
    compared: u64,
    order: u64,
    edits: u64,
}

fn cost(w: &diacraft_odg::Written) -> Cost {
    Cost {
        attributes: w.work.attributes_written,
        steps: w.work.steps_used(),
        compared: w.work.shapes_compared,
        order: w.work.order_steps,
        edits: w.work.edits,
    }
}

/// Every one of `n` connectors reconnected at its end to D's left side, saved: the cost of the save and the glue the file reads back as.
fn reconnect_all(n: usize) -> (Cost, Vec<(u32, &'static str, u32, u32)>) {
    let mut o = open(&many(n));
    let to = point_of(FRAME_D, 3);
    let edits: Vec<Glued> = (0..n as u32).map(|k| (4 + k, false, Some((D, 101)), to)).collect();
    glue_ends(&mut o.doc, &edits);
    for k in 0..n as u32 {
        straight(&mut o.doc, 4 + k, [point_of(FRAME_A, 1), to]);
    }
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{n} connectors reconnected at once: the save was refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    assert_eq!(content.matches(r#"draw:end-shape="id1""#).count(), n, "every end names D");
    assert_eq!(content.matches(r#"draw:end-glue-point="3""#).count(), n, "and D's left side");
    assert_eq!(content.matches(r#"xml:id="id1""#).count(), 1, "D is given its id once");
    (cost(&written), glue_list(&open_bytes(written.bytes).doc))
}

/// **Counted** (the hostile list): 10,000 connectors reconnected in one save cost what 1,000 do, ten times over, and no more. Each figure of the save grows with the number of ends
/// written and not with its square, and the file reads back with every end glued to D.
#[test]
fn ten_thousand_connectors_reconnected_at_once_cost_their_count_and_not_its_square() {
    let (small, _) = reconnect_all(1_000);
    let (big, glue) = reconnect_all(10_000);
    let mut starts = glue.iter().filter(|g| g.1 == "begin" && g.2 == A && g.3 == 103).count();
    assert_eq!(starts, 10_000, "the begins are as they were");
    starts = glue.iter().filter(|g| g.1 == "end" && g.2 == D && g.3 == 101).count();
    assert_eq!(starts, 10_000, "every end is glued to D's left side after the save");
    println!(
        "GLUECOST 1000: {:?}\nGLUECOST 10000: {:?}",
        (small.attributes, small.steps, small.compared, small.order, small.edits),
        (big.attributes, big.steps, big.compared, big.order, big.edits)
    );
    for (what, a, b) in [
        ("attributes written", small.attributes, big.attributes),
        ("steps", small.steps, big.steps),
        ("shapes compared", small.compared, big.compared),
        ("order steps", small.order, big.order),
        ("edits", small.edits, big.edits),
    ] {
        assert!(
            b <= a.saturating_mul(10).saturating_add(a / 20 + 64),
            "{what}: {a} for 1,000 connectors and {b} for 10,000 is more than ten times as much"
        );
    }
    // What the glue needs, held at what was measured (4,002 attributes for 1,000 connectors and 40,002 for 10,000: the shape and the point of each end, the end's two places, which are
    // written beside the path that goes with them; D's id is the 2): the cost of an end is a constant, however many are written.
    assert!(big.attributes <= 4 * 10_000 + 8, "{} attributes for 10,000 connectors", big.attributes);
    assert!(big.edits <= 4 * 10_000 + 8, "{} edits for 10,000 connectors", big.edits);
    assert!(big.compared <= 2 * 10_000 + 16, "{} shapes compared for 10,000 connectors and 3 shapes", big.compared);
    assert!(big.order <= 10_000 + 16, "{} order steps for 10,000 connectors", big.order);
    assert!(big.steps <= 64 * 10_000, "{} steps for 10,000 connectors", big.steps);
}

/// The same, timed: ignored, since a test of time says what a machine does and not what the writer does.
#[test]
#[ignore = "a timed run of the 10,000 connectors"]
fn timed_ten_thousand_connectors_reconnected_at_once() {
    let started = std::time::Instant::now();
    reconnect_all(10_000);
    let seconds = started.elapsed().as_secs_f64();
    println!("GLUETIME 10000 connectors: {seconds:.2} s");
    assert!(seconds < 60.0, "{seconds} s");
}

/// A custom shape that lists 4,096 glue points and a connector for each, every one reglued to its own point: the glue ids are the points' (4 on, as the file numbers them), the work is counted, and the file
/// reads back with each connector on the row it was sent to.
#[test]
fn a_shape_with_4096_glue_points_takes_a_connector_on_each_of_them() {
    const POINTS: u32 = 4_096;
    let list: String = (0..POINTS).map(|i| format!("{} {}", (i % 64) * 330, (i / 64) * 330)).collect::<Vec<_>>().join(" ");
    let geometry = format!(" draw:glue-points=\"{list}\"");
    let mut body = format!("{}{}", rect("A", Some("a"), FRAME_A), custom_shape("T", "t", (12.0, 10.0), "", &geometry));
    for k in 0..POINTS {
        body.push_str(&connector(&format!("c{k}"), Some(("a", Some(1))), None, (6.0, 4.0, 14.0, 12.0)));
    }
    let mut o = open(&Drawing::a4().page(&body));
    let target = 2;
    // The last row of the shape the reader made is 4 + 4,095.
    let edits: Vec<Glued> = (0..POINTS).map(|k| (3 + k, false, Some((target, 100 + 4 + k)), (14.0, 11.0))).collect();
    glue_ends(&mut o.doc, &edits);
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    for k in [0u32, 1, 63, 64, 2_047, 4_094, POINTS - 1] {
        let tag = tag_of(&content, &format!("c{k}"));
        assert_eq!(attr(tag, "draw:end-shape").as_deref(), Some("t"), "c{k}: {tag}");
        assert_eq!(attr(tag, "draw:end-glue-point"), Some((4 + k).to_string()), "c{k}: the point is the file's own number: {tag}");
    }
    let back = glue_list(&open_bytes(written.bytes).doc);
    assert_eq!(back.iter().filter(|g| g.1 == "end" && g.2 == target).count() as u32, POINTS);
    assert!(back.contains(&(3 + POINTS - 1, "end", target, 100 + 4 + POINTS - 1)), "the last connector is on the last point");
    // Each connector: the end's shape and point, its end's place (`svg:x2`, `svg:y2`) and its path (`svg:d`, `svg:viewBox`) but for what is as it was, which the writer leaves: at most five
    // (measured: 20,480 for 4,096), a constant for each, and the shape's id is not written (it has one).
    assert!(written.work.attributes_written <= 5 * u64::from(POINTS) + 8, "{} attributes for {POINTS} connectors", written.work.attributes_written);
}

/// A drawing of A, B and D and three connectors: `c0` from A's right side to B's left side, `c1` from A's bottom to nothing, and `c2` glued to the whole of A and of B.
fn mixed() -> Drawing {
    Drawing::a4().page(&format!(
        "{}{}{}{}{}{}",
        rect("A", Some("a"), FRAME_A),
        rect("B", Some("b"), FRAME_B),
        rect("D", None, FRAME_D),
        connector("c0", Some(("a", Some(1))), Some(("b", Some(3))), (6.0, 4.0, 12.0, 4.0)),
        connector("c1", Some(("a", Some(2))), None, (4.0, 5.0, 4.0, 8.0)),
        connector("c2", Some(("a", None)), Some(("b", None)), (4.0, 4.0, 14.0, 4.0)),
    ))
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 128, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// Any run of changes to the glue of the three connectors (an end taken off its shape, glued to a point of A, B or D, or to the whole of one), each as the engine leaves the model, is
    /// saved as the model has it or refused in words and nothing is written: the file reads back with the glue the model has, and a second save through the index the first gave is no edit.
    #[test]
    fn any_run_of_glue_changes_is_saved_as_the_model_has_it_or_refused_in_words(
        edits in proptest::collection::vec((0u32..3, 0u8..6, 0u32..3, 0u32..4), 1..7),
    ) {
        let mut o = open(&mixed());
        for (k, kind, target, row) in edits {
            let (connector, shape) = (4 + k, 1 + target);
            let place = (14.0, 11.0);
            match kind {
                0 => glue_end(&mut o.doc, connector, true, None, place),
                1 => glue_end(&mut o.doc, connector, false, None, place),
                2 => glue_end(&mut o.doc, connector, true, Some((shape, 100 + row)), place),
                3 => glue_end(&mut o.doc, connector, false, Some((shape, 100 + row)), place),
                4 => glue_end(&mut o.doc, connector, true, Some((shape, 3)), place),
                _ => glue_end(&mut o.doc, connector, false, Some((shape, 3)), place),
            }
        }
        let written = match real(&o.doc, &o.bytes, &o.index) {
            Ok(written) => written,
            Err(diacraft_odg::WriteError::Unsupported(_)) => return Ok(()),
            Err(other) => return Err(proptest::test_runner::TestCaseError::fail(format!("a run of glue changes was refused as a fault of the save: {other}"))),
        };
        let back = open_bytes(written.bytes.clone());
        proptest::prop_assert_eq!(glue_list(&back.doc), glue_list(&o.doc));
        let again = real(&o.doc, &written.bytes, &written.index).map_err(|e| proptest::test_runner::TestCaseError::fail(format!("the second save was refused: {e}")))?;
        proptest::prop_assert!(!again.edited, "the second save of the same model is an edit");
    }
}

/// Keeps two saved files for a check by another program: with `GLUEWRITE_KEEP` set to a folder, `many-1000.odg` (1,000 connectors reconnected at once) and `points-4096.odg` (a shape that
/// lists 4,096 glue points with a connector glued to each). Ignored: it writes where the environment says, and checks nothing the tests above do not.
#[test]
#[ignore = "writes two saved files to the folder GLUEWRITE_KEEP names, for a check by another program"]
fn keep_the_hostile_files() {
    let Some(keep) = std::env::var_os("GLUEWRITE_KEEP") else { return };
    let keep = std::path::PathBuf::from(keep);
    std::fs::create_dir_all(&keep).unwrap();
    let mut o = open(&many(1_000));
    let to = point_of(FRAME_D, 3);
    let edits: Vec<Glued> = (0..1_000u32).map(|k| (4 + k, false, Some((D, 101)), to)).collect();
    glue_ends(&mut o.doc, &edits);
    for k in 0..1_000u32 {
        straight(&mut o.doc, 4 + k, [point_of(FRAME_A, 1), to]);
    }
    std::fs::write(keep.join("many-1000.odg"), real(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    const POINTS: u32 = 4_096;
    let list: String = (0..POINTS).map(|i| format!("{} {}", (i % 64) * 330, (i / 64) * 330)).collect::<Vec<_>>().join(" ");
    let geometry = format!(" draw:glue-points=\"{list}\"");
    let mut body = format!("{}{}", rect("A", Some("a"), FRAME_A), custom_shape("T", "t", (12.0, 10.0), "", &geometry));
    for k in 0..POINTS {
        body.push_str(&connector(&format!("c{k}"), Some(("a", Some(1))), None, (6.0, 4.0, 14.0, 12.0)));
    }
    let mut o = open(&Drawing::a4().page(&body));
    let edits: Vec<Glued> = (0..POINTS).map(|k| (3 + k, false, Some((2, 100 + 4 + k)), (14.0, 11.0))).collect();
    glue_ends(&mut o.doc, &edits);
    std::fs::write(keep.join("points-4096.odg"), real(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
}

// ---- CPDOUBLE.ODG: the glue points of a shape of the file whose connection rows the model changed ------------------------------------------

/// A `draw:rect` called `R` (id `r`) holding `n` glue point elements, ids 4 on, each a place the shape's own width and height make of its number.
fn rect_with_points(n: u32) -> String {
    let points: String = (0..n)
        .map(|i| {
            format!(
                "<draw:glue-point draw:id=\"{}\" svg:x=\"{}cm\" svg:y=\"{}cm\" draw:escape-direction=\"auto\"/>",
                4 + i,
                (i % 9) as f64 - 4.0,
                (i / 9 % 9) as f64 - 4.0
            )
        })
        .collect();
    format!(
        "<draw:rect draw:name=\"R\" xml:id=\"r\" draw:id=\"r\" svg:x=\"2cm\" svg:y=\"2cm\" svg:width=\"8cm\" svg:height=\"6cm\">{points}</draw:rect>"
    )
}

/// The rows of the `Connection` section of shape `id`: its length.
fn rows_of(doc: &Document, id: u32) -> usize {
    shape_of(doc, id).sheet.sections.iter().find(|s| s.name.as_str() == "Connection").map_or(0, |s| s.rows.len())
}

/// Sets the number of rows of the shape's `Connection` section to `rows`: the rows past the section's are new ones (constants at a place the row number makes), and the section's last rows
/// are dropped when it has more.
fn set_rows(doc: &mut Document, id: u32, rows: usize) {
    let shape = shape_mut(doc, 0, id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Connection").expect("a Connection section");
    let section = Arc::make_mut(section);
    section.rows.truncate(rows);
    while section.rows.len() < rows {
        let ix = section.rows.len();
        let mut row = Row { ix: Some(ix as u32), ..Row::default() };
        for (name, value) in [("X", 0.5 + (ix % 7) as f64 / 20.0), ("Y", 0.25 + (ix % 5) as f64 / 10.0), ("DirX", 0.0), ("DirY", 1.0), ("Type", 0.0)]
        {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(value).expect("a number")));
        }
        section.rows.push(Arc::new(row));
    }
}

/// How many `draw:glue-point` elements the saved content holds, and their ids.
fn glue_point_ids(content: &str) -> Vec<u32> {
    content
        .match_indices("<draw:glue-point ")
        .filter_map(|(at, _)| attr(&content[at..at + content[at..].find('>').unwrap_or(0)], "draw:id")?.parse().ok())
        .collect()
}

/// **A shape of the file whose connection rows the model changed gets a glue point element for each row past the four standard ones and the geometry's own** (`glue.rs`): the rows the
/// model adds are written with the next ids (the rows' numbers), the file's own elements whose rows are as they were stay, and the rows the model takes away take their elements.
#[test]
fn a_shape_of_the_file_gets_a_glue_point_element_for_each_row_the_model_adds_and_loses_those_of_the_rows_it_takes() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(2)));
    assert_eq!(rows_of(&o.doc, 1), 6, "the four standard rows and the two the file lists");
    set_rows(&mut o.doc, 1, 9);
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    let content = part(&written.bytes, "content.xml");
    assert_eq!(
        glue_point_ids(&content),
        [4, 5, 6, 7, 8],
        "the file's two points as they were, and a point for each of the three rows the model added"
    );
    assert!(content.contains(r#"draw:id="4" svg:x="-4cm" svg:y="-4cm""#), "the file's own element is as it was written: {content}");
    let back = open_bytes(written.bytes.clone());
    assert_eq!(rows_of(&back.doc, 1), 9, "the file reads back with the rows the model has");
    // Each new element holds the share of the frame the row stands at, in centimetres from the frame's centre, y down the page (10 cm for a whole width or height).
    let (w, h) = (8.0 / 2.54, 6.0 / 2.54);
    for k in 6..9usize {
        let at = content.find(&format!("draw:id=\"{k}\"")).expect("an element");
        let tag = &content[at..at + content[at..].find('>').unwrap()];
        let cm = |name: &str| attr(tag, name).unwrap().trim_end_matches("cm").parse::<f64>().unwrap();
        let (x, y) = (0.5 + (k % 7) as f64 / 20.0, 0.25 + (k % 5) as f64 / 10.0);
        assert!((cm("svg:x") - 10.0 * (x / w - 0.5)).abs() < 1e-4 && (cm("svg:y") - 10.0 * (0.5 - y / h)).abs() < 1e-4, "{tag}");
    }
    let again = real(&o.doc, &written.bytes, &written.index).unwrap();
    assert!(!again.edited, "a second save of the same model is no edit");
    // The rows taken away: back to the four standard rows and the file's two.
    let mut p = open_bytes(written.bytes);
    set_rows(&mut p.doc, 1, 6);
    let written = real(&p.doc, &p.bytes, &p.index).unwrap_or_else(|e| panic!("the save was refused: {e}"));
    assert_eq!(glue_point_ids(&part(&written.bytes, "content.xml")), [4, 5], "the elements of the rows that went are gone");
    assert_eq!(rows_of(&open_bytes(written.bytes).doc, 1), 6);
}

/// The work of one save that rewrites the glue points of a shape of the file: the steps, the attributes written and the bytes copied.
fn glue_rewrite_cost(file: u32, now: usize) -> (u64, u64, u64) {
    let mut o = open(&Drawing::a4().page(&rect_with_points(file)));
    set_rows(&mut o.doc, 1, now);
    let written = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{file} elements and {now} rows: the save was refused: {e}"));
    let ids = glue_point_ids(&part(&written.bytes, "content.xml"));
    assert_eq!(ids.len(), now.saturating_sub(4), "an element for each row past the four standard ones");
    (written.work.steps_used(), written.work.attributes_written, written.work.edits)
}

/// **Counted** (the hostile list): a shape of the file with 500 glue point elements that the model gives 500 rows more costs what 250 and 250 more do, twice over, and no more; each new
/// element costs its attributes and a constant number of steps. The most rows the data of a shape carries is far past what a doubling makes (256).
#[test]
fn the_cost_of_rewriting_the_glue_points_of_a_shape_grows_with_their_number_and_not_its_square() {
    let (small, big) = (glue_rewrite_cost(250, 504), glue_rewrite_cost(500, 1_004));
    // 500 rows past the four standard ones against 250 more: the ratio of each figure is two, with a quarter of room.
    for (name, a, b) in [("steps", small.0, big.0), ("attributes", small.1, big.1), ("edits", small.2, big.2)] {
        assert!(b as f64 <= 2.5 * a as f64 + 64.0, "{name}: {a} for 500 rows, {b} for 1,000");
    }
    // A section of more rows than the data of a shape carries in its attribute (512 KiB) is refused in words, as before, and nothing is written.
    let mut o = open(&Drawing::a4().page(&rect_with_points(2)));
    set_rows(&mut o.doc, 1, 4_097);
    match real(&o.doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(words.contains("past what one attribute"), "{words}"),
        other => panic!("4,097 rows were not refused in words: {:?}", other.map(|w| w.edited)),
    }
}

/// **Timed**, beside the counted case above: a shape of 500 elements given 500 rows more is written in a few seconds at the most.
#[test]
#[ignore = "a timed check, beside the counted one"]
fn timed_the_glue_points_of_a_thousand_rows_are_written() {
    let t = std::time::Instant::now();
    let (steps, attributes, edits) = glue_rewrite_cost(500, 1_004);
    let seconds = t.elapsed().as_secs_f64();
    eprintln!("1,000 glue points written in {seconds:.2} s: {steps} steps, {attributes} attributes, {edits} edits");
    assert!(seconds < 30.0, "{seconds} s");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 64, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// Any run of changes to the number of connection rows of a shape of the file (rows put in, rows taken away, the file saved after each) is saved as the model has them or refused in
    /// words and nothing is written: the file reads back with as many rows as the model, holds a glue point element for each past the four standard ones, and a second save of the
    /// same model is no edit.
    #[test]
    fn any_run_of_changes_to_the_rows_of_a_shape_is_saved_as_the_model_has_them_or_refused_in_words(
        file in 0u32..6,
        steps in proptest::collection::vec(4usize..30, 1..5),
    ) {
        let mut o = open(&Drawing::a4().page(&rect_with_points(file)));
        for rows in steps {
            set_rows(&mut o.doc, 1, rows);
            let written = match real(&o.doc, &o.bytes, &o.index) {
                Ok(written) => written,
                Err(diacraft_odg::WriteError::Unsupported(_)) => return Ok(()),
                Err(other) => return Err(proptest::test_runner::TestCaseError::fail(format!("rows changed to {rows}: the save was refused as a fault of the save: {other}"))),
            };
            let back = open_bytes(written.bytes.clone());
            proptest::prop_assert_eq!(rows_of(&back.doc, 1), rows);
            proptest::prop_assert_eq!(glue_point_ids(&part(&written.bytes, "content.xml")).len(), rows - 4, "an element for each row past the four standard ones");
            let again = real(&o.doc, &written.bytes, &written.index)
                .map_err(|e| proptest::test_runner::TestCaseError::fail(format!("the second save was refused: {e}")))?;
            proptest::prop_assert!(!again.edited, "the second save of the same model is an edit");
            o = crate::write_core::Open { bytes: written.bytes, doc: o.doc, index: written.index };
        }
    }
}

// ----: the model writer, and a shape turned or mirrored ---------------------------------------------------------------------------

/// The file the model writer makes of `doc` (no file to splice into: a Save As of a drawing that holds no `.odg`).
fn from_the_model(doc: &Document) -> Vec<u8> {
    diacraft_odg::write_with(doc, None, &diacraft_odg::WriteOptions::default()).unwrap_or_else(|e| panic!("the model writer refused: {e}")).bytes
}

/// Gives shape `id` the turn `radians` (the cell is put in when the shape has none).
fn set_angle(doc: &mut Document, id: u32, radians: f64) {
    let sheet = Arc::make_mut(&mut shape_mut(doc, 0, id).sheet);
    sheet.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_number(radians).unwrap()));
}

/// The text of the start tag of the `draw:glue-point` element with the id `id` in `content`.
fn glue_tag(content: &str, id: u32) -> String {
    let at = content.find(&format!("draw:id=\"{id}\"")).unwrap_or_else(|| panic!("no glue point {id} in {content}"));
    let start = content[..at].rfind("<draw:glue-point ").expect("an element before its id");
    content[start..at + content[at..].find('>').expect("a tag's end")].to_owned()
}

/// **A shape the reader made and the model writer writes again gets a glue point element for each row the model added, and loses those of the rows it took**: the elements its
/// file held are among the children it keeps, so the rows the session added had none (a program that does not read DiaCraft's data had no point to attach a connector to), and the elements
/// of the rows a halving took stayed.
#[test]
fn the_model_writer_gives_a_shape_of_the_file_an_element_for_each_row_the_model_adds_and_takes_those_of_the_rows_it_takes() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(2)));
    assert_eq!(rows_of(&o.doc, 1), 6, "the four standard rows and the two the file lists");
    set_rows(&mut o.doc, 1, 9);
    let content = part(&from_the_model(&o.doc), "content.xml");
    assert_eq!(
        glue_point_ids(&content),
        [4, 5, 6, 7, 8],
        "the file's two points as they were, and a point for each of the three rows the model added: {content}"
    );
    assert!(content.contains(r#"draw:id="4" svg:x="-4cm" svg:y="-4cm""#), "the file's own element is as it was written: {content}");
    // Each new element holds the share of the frame the row stands at, in centimetres from the frame's centre, y down the page.
    let (w, h) = (8.0 / 2.54, 6.0 / 2.54);
    for k in 6..9usize {
        let tag = glue_tag(&content, k as u32);
        let cm = |name: &str| attr(&tag, name).unwrap().trim_end_matches("cm").parse::<f64>().unwrap();
        let (x, y) = (0.5 + (k % 7) as f64 / 20.0, 0.25 + (k % 5) as f64 / 10.0);
        assert!((cm("svg:x") - 10.0 * (x / w - 0.5)).abs() < 1e-4 && (cm("svg:y") - 10.0 * (0.5 - y / h)).abs() < 1e-4, "{tag}");
        assert_eq!(attr(&tag, "draw:escape-direction").as_deref(), Some("up"), "{tag}");
    }
    assert_eq!(rows_of(&open_bytes(from_the_model(&o.doc)).doc, 1), 9, "the file reads back with the rows the model has");
    // The rows taken away take their elements: the four standard rows and the file's two again.
    let mut p = open_bytes(from_the_model(&o.doc));
    set_rows(&mut p.doc, 1, 6);
    let content = part(&from_the_model(&p.doc), "content.xml");
    assert_eq!(glue_point_ids(&content), [4, 5], "the elements of the rows that went are gone: {content}");
    // Taken away down to one of the file's own: the element of row 5 goes too.
    let mut q = open(&Drawing::a4().page(&rect_with_points(2)));
    set_rows(&mut q.doc, 1, 5);
    assert_eq!(glue_point_ids(&part(&from_the_model(&q.doc), "content.xml")), [4]);
}

/// **A row of the file edited in the session is written from the row**, and its element as it was read is not: the element says where the point was, and a program that does not read
/// DiaCraft's data would put the connector there.
#[test]
fn the_model_writer_writes_an_edited_row_of_the_file_from_the_row_and_not_as_its_element_was_read() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(2)));
    let before = glue_tag(&part(&from_the_model(&o.doc), "content.xml"), 5);
    {
        let shape = shape_mut(&mut o.doc, 0, 1);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let at = sheet.sections.iter().position(|s| s.name.as_str() == "Connection").unwrap();
        let section = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
        let row = Arc::make_mut(section.rows.iter_mut().find(|r| r.ix == Some(5)).unwrap());
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_number(8.0 / 2.54 * 0.75).unwrap()));
    }
    let content = part(&from_the_model(&o.doc), "content.xml");
    assert_eq!(glue_point_ids(&content), [4, 5], "{content}");
    let after = glue_tag(&content, 5);
    assert_ne!(before, after, "the element of an edited row is made again");
    let x = attr(&after, "svg:x").unwrap().trim_end_matches("cm").parse::<f64>().unwrap();
    assert!((x - 2.5).abs() < 1e-4, "three quarters across the frame is 25% right of its centre: {after}");
    assert!(!content.contains(&before), "the stale element is not written beside it");
}

/// **A shape that is turned has its elements in the page's axes**: LibreOffice places a `draw:glue-point` there, from the box the shape takes up on the page. A frame 8 by 6
/// cm turned a quarter turn takes up 6 by 8, and a row three quarters across and a quarter up the frame (2 cm right and 1.5 cm below its centre) is on the page 1.5 cm right of the
/// centre and 2 cm above it: 25% of 6 cm right and 25% of 8 cm up.
#[test]
fn the_model_writer_writes_the_element_of_a_turned_shape_in_the_pages_axes() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(0)));
    set_rows(&mut o.doc, 1, 5);
    {
        // Row 4 at (0.75, 0.25) of the frame, and the way out up.
        set_angle(&mut o.doc, 1, std::f64::consts::FRAC_PI_2);
        let shape = shape_mut(&mut o.doc, 0, 1);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let at = sheet.sections.iter().position(|s| s.name.as_str() == "Connection").unwrap();
        let section = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
        let row = Arc::make_mut(section.rows.iter_mut().find(|r| r.ix == Some(4)).unwrap());
        for (name, v) in [("X", 8.0 / 2.54 * 0.75), ("Y", 6.0 / 2.54 * 0.25), ("DirX", 0.0), ("DirY", 1.0)] {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap()));
        }
    }
    let content = part(&from_the_model(&o.doc), "content.xml");
    assert_eq!(glue_point_ids(&content), [4], "{content}");
    let tag = glue_tag(&content, 4);
    let cm = |name: &str| attr(&tag, name).unwrap().trim_end_matches("cm").parse::<f64>().unwrap();
    assert!((cm("svg:x") - 2.5).abs() < 1e-4 && (cm("svg:y") + 2.5).abs() < 1e-4, "{tag}");
    assert_eq!(
        attr(&tag, "draw:escape-direction").as_deref(),
        Some("left"),
        "the way out up, turned a quarter turn counterclockwise, is left: {tag}"
    );
}

/// A shape that is turned is written the same by the save into the file: the splice writes the same element at the same place as the model writer.
#[test]
fn the_save_into_the_file_writes_the_same_element_for_a_turned_shape_as_the_model_writer() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(0)));
    set_rows(&mut o.doc, 1, 6);
    set_angle(&mut o.doc, 1, 0.5);
    let model = part(&from_the_model(&o.doc), "content.xml");
    let spliced = part(&real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {e}")).bytes, "content.xml");
    assert_eq!(glue_point_ids(&model), [4, 5]);
    assert_eq!(glue_point_ids(&spliced), [4, 5]);
    for id in [4, 5] {
        let (a, b) = (glue_tag(&model, id), glue_tag(&spliced, id));
        for name in ["svg:x", "svg:y", "draw:escape-direction"] {
            assert_eq!(attr(&a, name), attr(&b, name), "{name} of glue point {id}: {a} and {b}");
        }
    }
}

/// The work of one model write of a shape of the file whose rows the model changed: the steps and the attributes written.
fn model_glue_cost(file: u32, now: usize) -> (u64, u64) {
    let mut o = open(&Drawing::a4().page(&rect_with_points(file)));
    set_rows(&mut o.doc, 1, now);
    let written = diacraft_odg::write_with(&o.doc, None, &diacraft_odg::WriteOptions::default())
        .unwrap_or_else(|e| panic!("{file} elements and {now} rows: the model writer refused: {e}"));
    let ids = glue_point_ids(&part(&written.bytes, "content.xml"));
    assert_eq!(ids.len(), now.saturating_sub(4), "an element for each row past the four standard ones");
    (written.work.steps_used(), written.work.attributes_written)
}

/// **Counted**: the model writer's work on the glue points of a shape of the file grows with their number and not its square: 500 elements and 500 rows more cost what 250
/// and 250 more do, twice over, and no more.
#[test]
fn the_cost_of_the_model_writers_glue_points_grows_with_their_number_and_not_its_square() {
    let (small, big) = (model_glue_cost(250, 504), model_glue_cost(500, 1_004));
    for (name, a, b) in [("steps", small.0, big.0), ("attributes", small.1, big.1)] {
        assert!(b as f64 <= 2.5 * a as f64 + 64.0, "{name}: {a} for 500 rows, {b} for 1,000");
    }
}

/// A section of more rows than a shape's glue points are written for (4,096) is saved as before: the elements the file had stay, the rows ride in DiaCraft's data, and nothing is written
/// that the reader would number otherwise; or the save is refused in words. It never panics.
#[test]
fn the_model_writer_leaves_the_elements_as_they_were_for_a_section_past_what_it_writes() {
    let mut o = open(&Drawing::a4().page(&rect_with_points(3)));
    set_rows(&mut o.doc, 1, 4_200);
    match diacraft_odg::write_with(&o.doc, None, &diacraft_odg::WriteOptions::default()) {
        Ok(written) => assert_eq!(glue_point_ids(&part(&written.bytes, "content.xml")), [4, 5, 6], "the file's own elements, as it had them"),
        Err(diacraft_odg::WriteError::Unsupported(words)) => assert!(!words.is_empty()),
        Err(other) => panic!("the save was refused as a fault of the save: {other}"),
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 64, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// Any run of changes to the number of connection rows of a shape of the file, written again from the model with the shape turned or not: the file reads back with as many rows as the
    /// model, holds a glue point element for each past the four standard ones, and a second write of the same model is the same bytes; or the save is refused in words.
    #[test]
    fn any_run_of_changes_to_the_rows_of_a_shape_is_written_from_the_model_as_it_has_them_or_refused_in_words(
        file in 0u32..6,
        steps in proptest::collection::vec(4usize..30, 1..4),
        turn in 0u8..4,
    ) {
        let mut o = open(&Drawing::a4().page(&rect_with_points(file)));
        set_angle(&mut o.doc, 1, [0.0, 0.5, std::f64::consts::FRAC_PI_2, std::f64::consts::PI][usize::from(turn)]);
        for rows in steps {
            set_rows(&mut o.doc, 1, rows);
            let written = match diacraft_odg::write_with(&o.doc, None, &diacraft_odg::WriteOptions::default()) {
                Ok(written) => written,
                Err(diacraft_odg::WriteError::Unsupported(_)) => return Ok(()),
                Err(other) => return Err(proptest::test_runner::TestCaseError::fail(format!("rows changed to {rows}: the save was refused as a fault of the save: {other}"))),
            };
            let back = open_bytes(written.bytes.clone());
            proptest::prop_assert_eq!(rows_of(&back.doc, 1), rows);
            proptest::prop_assert_eq!(glue_point_ids(&part(&written.bytes, "content.xml")).len(), rows - 4, "an element for each row past the four standard ones");
            let again = diacraft_odg::write_with(&o.doc, None, &diacraft_odg::WriteOptions::default()).unwrap();
            proptest::prop_assert_eq!(again.bytes, written.bytes, "the same model gives the same bytes");
        }
    }
}
