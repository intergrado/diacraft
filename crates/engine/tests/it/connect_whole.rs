//! A connector glued to a shape that has no connection point (a public report: `connector.add` refused such a shape, "shape 1 has no connection points to glue to", where the
//! parity table says a target `{id}` glues the end to the whole shape). The end is glued to the whole of the shape (`ToPart` 3, plain end cells that the engine places on the
//! shape's box, as it places a file's whole-shape end), moves with the shape, and comes back from a save in each format.
//!
//! The drawing is built in code. Shape 1 has cells of its own and no `Connection` section; shape 2 is an instance of a master that has none; shape 3 is an instance of a master
//! that has four points; shape 4 holds an empty `Connection` section; shape 5 has no size.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::draw::shape_corners;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::edit::{doc, num};
use crate::edit_foreign::Dir;
use crate::{check_session, run};

type P = (f64, f64);

/// The line of a shape, which a file holds on the shape or on its style: black, a hundredth of an inch.
const LINE: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/>"##;

/// The page's size: 12 by 12 inches, which an `.odg` save asks of the first page.
const PAGE: &str = r#"<Cell N="PageWidth" V="12"/><Cell N="PageHeight" V="12"/>"#;

/// The `Geometry` of a box of `w` by `h` from its lower left corner.
fn outline(w: f64, h: f64) -> String {
    format!(
        r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{w}"/><Cell N="Y" V="{h}"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="{h}"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
    )
}

/// The cells of a box of `w` by `h` with its pin at (`x`, `y`).
fn cells(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/>
<Cell N="LocPinX" V="{lx}" F="Width*0.5"/><Cell N="LocPinY" V="{ly}" F="Height*0.5"/>{LINE}"#,
        lx = w / 2.0,
        ly = h / 2.0
    )
}

/// A master of a 2 by 1 box with no connection point.
fn blob_master() -> String {
    shape(
        4,
        &format!(
            r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{}"#,
            outline(2.0, 1.0)
        ),
    )
}

/// The same box with a connection point on its right (row 0), left (1), top (2) and bottom (3).
fn box_master() -> String {
    shape(
        4,
        &format!(
            r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{}
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row>
<Row IX="2"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="1" F="Height"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row>
<Row IX="3"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="0"/><Cell N="DirX" V="0"/><Cell N="DirY" V="-1"/></Row></Section>"#,
            outline(2.0, 1.0)
        ),
    )
}

/// An instance of master `master` with its pin at (`x`, `y`).
fn instance(id: u32, master: u32, x: f64, y: f64) -> String {
    shape_with(id, &format!(r#" Master="{master}""#), &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/>{LINE}"#))
}

/// The page's shapes: 1 a box of its own at (2, 2) with no `Connection` section, 2 an instance of the blob master at (8, 2), 3 an instance of the box master at (5, 6),
/// 4 a box at (2, 9) with an empty `Connection` section, and 5 a shape of no size at (8, 9).
fn page_shapes() -> String {
    [
        shape(1, &format!("{}{}", cells(2.0, 2.0, 2.0, 1.0), outline(2.0, 1.0))),
        instance(2, 4, 8.0, 2.0),
        instance(3, 7, 5.0, 6.0),
        shape(4, &format!(r#"{}{}<Section N="Connection"/>"#, cells(2.0, 9.0, 2.0, 1.0), outline(2.0, 1.0))),
        shape(5, &format!("{}{}", cells(8.0, 9.0, 0.0, 0.0), outline(0.0, 0.0))),
    ]
    .concat()
}

/// The package: masters 4 (the blob) and 7 (the box with points), and the page of [`page_shapes`].
pub(crate) fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(4, "Blob").shapes(&blob_master()))
        .master(MasterSpec::new(7, "Box").shapes(&box_master()))
        .page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&page_shapes()))
        .build()
}

/// A session that has opened the drawing.
pub(crate) fn opened(dir: &Dir) -> Session {
    let path = dir.path("in.vsdx");
    std::fs::write(&path, drawing()).unwrap();
    reopened(&path)
}

pub(crate) fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// The `Connect` rows of page 1 from connector `id`: the end's cell, `FromPart`, the shape, `ToPart` and `ToCell`.
pub(crate) fn glue_rows(d: &Document, id: u32) -> Vec<(String, u32, u32, u32, String)> {
    glue_rows_on(d, 1, id)
}

/// [`glue_rows`] on page `page`.
fn glue_rows_on(d: &Document, page: u32, id: u32) -> Vec<(String, u32, u32, u32, String)> {
    let page = d.page(page).unwrap();
    page.connects
        .iter()
        .filter(|c| c.from_sheet == id)
        .map(|c| {
            (
                c.from_cell.as_deref().unwrap_or("").to_owned(),
                c.from_part.unwrap_or(0),
                c.to_sheet,
                c.to_part.unwrap_or(0),
                c.to_cell.as_deref().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// The connector's end: its place on the page.
pub(crate) fn end_at(d: &Document, id: u32, begin: bool) -> P {
    let (x, y) = if begin { ("BeginX", "BeginY") } else { ("EndX", "EndY") };
    (num(d, id, x), num(d, id, y))
}

/// The box of shape `id` on the page: lower left and upper right.
pub(crate) fn box_of(d: &Document, id: u32) -> (P, P) {
    let c = shape_corners(d, 1, id).unwrap();
    (c[0], c[2])
}

/// Whether `p` is on the boundary of the box, to a millionth of an inch.
pub(crate) fn on_outline(p: P, (lo, hi): (P, P)) -> bool {
    let e = 1.0e-6;
    let within = |v: f64, a: f64, b: f64| v >= a - e && v <= b + e;
    let on_side = |v: f64, side: f64| (v - side).abs() < e;
    ((on_side(p.0, lo.0) || on_side(p.0, hi.0)) && within(p.1, lo.1, hi.1)) || ((on_side(p.1, lo.1) || on_side(p.1, hi.1)) && within(p.0, lo.0, hi.0))
}

/// Whether `p` is on the boundary of the quadrilateral `corners` (a shape's box on the page, turned or not), to a millionth of an inch.
pub(crate) fn on_box(corners: [P; 4], p: P) -> bool {
    (0..4).any(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let ab = (b.0 - a.0, b.1 - a.1);
        let length_squared = ab.0 * ab.0 + ab.1 * ab.1;
        let t = if length_squared > 0.0 { (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / length_squared).clamp(0.0, 1.0) } else { 0.0 };
        (a.0 + ab.0 * t - p.0).hypot(a.1 + ab.1 * t - p.1) < 1.0e-6
    })
}

pub(crate) fn distance(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The id a command answered.
pub(crate) fn id_of(answer: &Value) -> u32 {
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

#[test]
fn the_drawing_is_as_built() {
    let dir = Dir::new();
    let s = opened(&dir);
    let d = doc(&s);
    for id in [1, 2, 4] {
        assert!(diacraft_engine::draw::connection_points(d, 1, id).is_empty(), "shape {id} has no connection point");
    }
    assert_eq!(diacraft_engine::draw::connection_points(d, 1, 3).len(), 4);
    assert_eq!(box_of(d, 1), ((1.0, 1.5), (3.0, 2.5)));
    assert_eq!(box_of(d, 2), ((7.0, 1.5), (9.0, 2.5)));
}

#[test]
fn a_target_with_no_connection_points_is_glued_to_the_whole_shape_and_the_end_is_on_its_box() {
    // The report's edit: a connector from a plain box (no `Connection` rows, its own or its master's) to a shape that has points.
    let dir = Dir::new();
    let mut s = opened(&dir);
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}}));
    let c = id_of(&made);
    assert_eq!(made["begin"], json!({"id": 1, "point": null}), "{made}");
    assert_eq!(made["end"]["id"], 3, "{made}");
    assert!(made["end"]["point"].is_u64(), "the shape that has points is glued at one: {made}");
    let d = doc(&s);
    let rows = glue_rows(d, c);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let begin = rows.iter().find(|r| r.0 == "BeginX").expect("a row for the begin");
    assert_eq!((begin.1, begin.2, begin.3, begin.4.as_str()), (9, 1, 3, "PinX"), "the whole of shape 1: {rows:?}");
    let end = rows.iter().find(|r| r.0 == "EndX").expect("a row for the end");
    assert_eq!((end.1, end.2), (12, 3));
    assert!(end.3 >= 100, "a point of shape 3: {rows:?}");
    // The begin is on the box of shape 1, on the side that faces the other end (above and to the right of it).
    let at = end_at(d, c, true);
    assert!(on_outline(at, box_of(d, 1)), "the begin {at:?} is on shape 1's box");
    let far = end_at(d, c, false);
    assert!(at.0 > 2.0 - 1.0e-6 || at.1 > 2.0 - 1.0e-6, "on a side that faces the end at {far:?}: {at:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn both_ends_on_shapes_with_no_points_are_glued_to_the_whole_of_each() {
    // Shape 2 is an instance of a master that has no connection point, shape 1 holds none of its own.
    let dir = Dir::new();
    let mut s = opened(&dir);
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}}));
    let c = id_of(&made);
    assert_eq!(made["begin"], json!({"id": 1, "point": null}), "{made}");
    assert_eq!(made["end"], json!({"id": 2, "point": null}), "{made}");
    let d = doc(&s);
    let rows = glue_rows(d, c);
    assert_eq!(rows.iter().map(|r| (r.0.as_str(), r.1, r.2, r.3)).collect::<Vec<_>>(), [("BeginX", 9, 1, 3), ("EndX", 12, 2, 3)], "{rows:?}");
    // Facing each other: the begin on the right side of shape 1 and the end on the left side of shape 2, which are level.
    let (begin, end) = (end_at(d, c, true), end_at(d, c, false));
    assert!(on_outline(begin, box_of(d, 1)) && on_outline(end, box_of(d, 2)), "{begin:?} {end:?}");
    assert!(begin.0 > 2.9 && end.0 < 7.1, "the ends face each other: {begin:?} {end:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_named_point_of_a_shape_with_none_is_refused_in_words_and_nothing_is_added() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = doc(&s).clone();
    let e = s.execute("connector.add", &json!({"page": 1, "from": {"id": 1, "point": 0}, "to": {"id": 3}})).unwrap_err().to_string();
    assert!(e.contains("shape 1 has no connection point 0"), "{e}");
    assert_eq!(*doc(&s), before);
}

/// The ends of connector `id` and its frame, as the cells hold them: the frame is between the ends whatever placed them.
fn assert_frame_between_the_ends(d: &Document, id: u32) {
    let (begin, end) = (end_at(d, id, true), end_at(d, id, false));
    let near = |a: f64, b: f64| (a - b).abs() < 1.0e-6;
    assert!(near(num(d, id, "Width"), distance(begin, end)), "Width {} is the distance between {begin:?} and {end:?}", num(d, id, "Width"));
    assert!(
        near(num(d, id, "PinX"), (begin.0 + end.0) / 2.0) && near(num(d, id, "PinY"), (begin.1 + end.1) / 2.0),
        "the pin is the middle of the ends"
    );
    let angle = (end.1 - begin.1).atan2(end.0 - begin.0);
    assert!(near(num(d, id, "Angle"), angle), "Angle {} is the direction from {begin:?} to {end:?}", num(d, id, "Angle"));
}

#[test]
fn the_connector_made_has_its_frame_between_its_ends_and_a_route_that_starts_and_ends_on_them() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    let d = doc(&s);
    assert_frame_between_the_ends(d, c);
    let route = crate::edit::route(d, c);
    let (begin, end) = (end_at(d, c, true), end_at(d, c, false));
    assert!(
        distance(*route.first().unwrap(), begin) < 1.0e-6 && distance(*route.last().unwrap(), end) < 1.0e-6,
        "{route:?} runs from {begin:?} to {end:?}"
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_made_is_where_the_next_change_to_its_shapes_leaves_it() {
    // The end is made where the engine's rule puts it, and not near it: a change that touches either shape does not move it.
    for to in [2, 3] {
        let dir = Dir::new();
        let mut s = opened(&dir);
        let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": to}})));
        let (begin, end) = (end_at(doc(&s), c, true), end_at(doc(&s), c, false));
        for shape in [1, to] {
            run(&mut s, "shape.move", json!({"ids": [shape], "dx": 0.0001, "dy": 0.0}));
            run(&mut s, "shape.move", json!({"ids": [shape], "dx": -0.0001, "dy": 0.0}));
        }
        let d = doc(&s);
        let (now_begin, now_end) = (end_at(d, c, true), end_at(d, c, false));
        assert!(
            distance(begin, now_begin) < 1.0e-3 && distance(end, now_end) < 1.0e-3,
            "to {to}: {begin:?} {end:?} became {now_begin:?} {now_end:?}"
        );
        assert_eq!(glue_rows(d, c).len(), 2);
    }
}

#[test]
fn the_end_follows_its_shape_when_the_shape_moves_and_stays_on_its_box() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}})));
    let before = end_at(doc(&s), c, true);
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 3.0}));
    let d = doc(&s);
    let after = end_at(d, c, true);
    assert!(on_outline(after, box_of(d, 1)), "the begin {after:?} is on the moved box {:?}", box_of(d, 1));
    assert!(after.1 > before.1 + 2.0, "it went up with the shape: {before:?} to {after:?}");
    assert_frame_between_the_ends(d, c);
    let route = crate::edit::route(d, c);
    assert!(distance(*route.first().unwrap(), after) < 1.0e-6, "the route starts at the end: {route:?}");
    assert_eq!(glue_rows(d, c).iter().filter(|r| r.3 == 3).count(), 1, "and is still glued to the whole of the shape");
    // A resize and a turn carry it too.
    run(&mut s, "shape.resize", json!({"id": 1, "x": 1.5, "y": 4.0, "width": 3.0, "height": 2.0}));
    let d = doc(&s);
    assert!(on_outline(end_at(d, c, true), box_of(d, 1)), "after a resize: {:?} on {:?}", end_at(d, c, true), box_of(d, 1));
    run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 90}));
    let d = doc(&s);
    let corners = shape_corners(d, 1, 1).unwrap();
    let at = end_at(d, c, true);
    let on_turned_box = (0..4).any(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let (ab, ap) = ((b.0 - a.0, b.1 - a.1), (at.0 - a.0, at.1 - a.1));
        let t = ((ab.0 * ap.0 + ab.1 * ap.1) / (ab.0 * ab.0 + ab.1 * ab.1)).clamp(0.0, 1.0);
        distance((a.0 + ab.0 * t, a.1 + ab.1 * t), at) < 1.0e-6
    });
    assert!(on_turned_box, "after a turn {at:?} is on the box {corners:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn undo_takes_the_connector_back_and_redo_makes_it_again() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = doc(&s).clone();
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 2}})));
    let made = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*doc(&s), before, "one undo takes the connector, its glue and the placeable mark off");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(*doc(&s), made);
    assert_eq!(glue_rows(doc(&s), c).len(), 2);
}

#[test]
fn reconnect_glues_an_end_to_the_whole_of_a_shape_with_no_points_and_back_to_a_point_or_free() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    // A connector from shape 3's right point to a free place.
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 3, "point": 0}, "to": {"x": 9.0, "y": 4.0}}));
    let c = id_of(&made);
    assert_eq!(made["end"], Value::Null);
    let begin_before = end_at(doc(&s), c, true);
    // The free end to shape 2, which has none.
    let answer = run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 2}}));
    assert_eq!(answer["end"], json!({"id": 2, "point": null}), "{answer}");
    assert_eq!(answer["begin"]["id"], 3, "{answer}");
    let d = doc(&s);
    let rows = glue_rows(d, c);
    let end_row = rows.iter().find(|r| r.0 == "EndX").expect("the end is glued");
    assert_eq!((end_row.1, end_row.2, end_row.3, end_row.4.as_str()), (12, 2, 3, "PinX"), "{rows:?}");
    assert!(on_outline(end_at(d, c, false), box_of(d, 2)), "{:?} on {:?}", end_at(d, c, false), box_of(d, 2));
    assert!(distance(begin_before, end_at(d, c, true)) < 1.0e-9, "the other end stays");
    assert_frame_between_the_ends(d, c);
    // A point named on it is refused and nothing changes.
    let before = d.clone();
    let e = s.execute("connector.reconnect", &json!({"id": c, "end": "end", "target": {"id": 2, "point": 1}})).unwrap_err().to_string();
    assert!(e.contains("shape 2 has no connection point 1"), "{e}");
    assert_eq!(*doc(&s), before);
    // The end to a point of shape 3: the whole-shape row goes and a point's takes its place, with the formulas.
    let answer = run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 3, "point": 1}}));
    assert_eq!(answer["end"], json!({"id": 3, "point": 1}), "{answer}");
    let d = doc(&s);
    let rows = glue_rows(d, c);
    assert_eq!(rows.iter().filter(|r| r.0 == "EndX").map(|r| (r.2, r.3)).collect::<Vec<_>>(), [(3, 101)], "{rows:?}");
    // And the end back to the whole of shape 1, then let go.
    let answer = run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": 1}}));
    assert_eq!(answer["end"], json!({"id": 1, "point": null}), "{answer}");
    let answer = run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"x": 9.0, "y": 8.0}}));
    assert_eq!(answer["end"], Value::Null, "{answer}");
    let d = doc(&s);
    assert_eq!(glue_rows(d, c).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["BeginX"], "only the begin is glued");
    assert_eq!(end_at(d, c, false), (9.0, 8.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_an_empty_connection_section_is_glued_to_as_a_whole() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    assert!(diacraft_engine::draw::connection_points(doc(&s), 1, 4).is_empty());
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 4}, "to": {"id": 3}}));
    assert_eq!(made["begin"], json!({"id": 4, "point": null}), "{made}");
    let d = doc(&s);
    assert!(on_outline(end_at(d, id_of(&made), true), box_of(d, 4)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_of_no_size_is_glued_to_at_its_place_and_nothing_goes_wrong_when_it_moves() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 5}, "to": {"id": 3}}));
    let c = id_of(&made);
    assert_eq!(made["begin"], json!({"id": 5, "point": null}), "{made}");
    let d = doc(&s);
    let begin = end_at(d, c, true);
    assert!(begin.0.is_finite() && begin.1.is_finite());
    assert!(distance(begin, (8.0, 9.0)) < 1.0e-9, "the whole of a shape of no size is its place: {begin:?}");
    run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": -1.0}));
    run(&mut s, "shape.move", json!({"ids": [3], "dx": -1.0, "dy": 0.0}));
    let d = doc(&s);
    assert!(distance(end_at(d, c, true), (9.0, 8.0)) < 1.0e-9, "it follows: {:?}", end_at(d, c, true));
    // Both ends on it: a connector of no length, with its frame still a frame.
    let both = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 5}, "to": {"id": 5}}));
    let both = id_of(&both);
    assert!(num(doc(&s), both, "Width").abs() < 1.0e-9);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_with_no_points_is_glued_to_as_a_whole_and_the_end_follows_the_group() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2]})));
    assert!(diacraft_engine::draw::connection_points(doc(&s), 1, group).is_empty(), "a group made here has no connection point");
    let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": group}, "to": {"id": 3}}));
    let c = id_of(&made);
    assert_eq!(made["begin"], json!({"id": group, "point": null}), "{made}");
    let d = doc(&s);
    assert!(on_outline(end_at(d, c, true), box_of(d, group)), "{:?} on the group's box {:?}", end_at(d, c, true), box_of(d, group));
    run(&mut s, "shape.move", json!({"ids": [group], "dx": 0.0, "dy": 4.0}));
    let d = doc(&s);
    assert!(on_outline(end_at(d, c, true), box_of(d, group)), "{:?} on the moved group's box {:?}", end_at(d, c, true), box_of(d, group));
    assert_frame_between_the_ends(d, c);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_as_large_as_a_number_can_say_or_that_is_no_number_is_glued_to_or_refused_in_words_and_the_document_stays_valid() {
    // Shape 6 is as large as a number says, 7 has a width that is not a number and 8 one that is past the largest.
    let odd = |id: u32, width: &str| {
        let own = format!(r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="3"/><Cell N="Width" V="{width}"/><Cell N="Height" V="1"/>{LINE}"#);
        shape(id, &format!("{own}{}", outline(2.0, 1.0)))
    };
    let huge = shape(6, &format!("{}{}", cells(1.0e300, 1.0e300, 1.0e300, 1.0e300), outline(1.0e300, 1.0e300)));
    let shapes = format!("{}{huge}{}{}", page_shapes(), odd(7, "NaN"), odd(8, "1e999"));
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build();
    let dir = Dir::new();
    let path = dir.path("odd.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = reopened(&path);
    for id in [6, 7, 8] {
        match s.execute("connector.add", &json!({"page": 1, "from": {"id": id}, "to": {"id": 1}})) {
            Ok(made) => {
                let d = doc(&s);
                let c = id_of(&made);
                for cell in ["BeginX", "BeginY", "EndX", "EndY"] {
                    assert!(num(d, c, cell).is_finite(), "shape {id}: {cell} is a number");
                }
            }
            Err(e) => assert!(!e.to_string().is_empty(), "shape {id}: a refusal has words"),
        }
        assert_eq!(check_session(&s), Ok(()), "shape {id}");
    }
}

/// The connector of the first page of a reopened drawing, and for each end the box of the shape it is glued to and its `ToPart`: `(connector, begin, end)`, each `(lo, hi, to_part)`.
type Ends = Option<(P, P, u32)>;

fn glued_ends(d: &Document) -> (u32, Ends, Ends) {
    let page = d.pages.get(0).expect("a page");
    let from: std::collections::BTreeSet<u32> = page.connects.iter().map(|c| c.from_sheet).collect();
    assert_eq!(from.len(), 1, "one connector holds glue on the page: {from:?}");
    let connector = *from.iter().next().unwrap();
    let end_of = |cell: &str| -> Ends {
        let row = page.connects.iter().find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(cell))?;
        let corners = shape_corners(d, page.id, row.to_sheet)?;
        Some((corners[0], corners[2], row.to_part.unwrap_or(0)))
    };
    (connector, end_of("BeginX"), end_of("EndX"))
}

fn same_box(a: (P, P), b: (P, P)) -> bool {
    distance(a.0, b.0) < 1.0e-3 && distance(a.1, b.1) < 1.0e-3
}

/// What a cell of shape `id` on page `page` holds, by inheritance.
pub(crate) fn num_on(d: &Document, page: u32, id: u32, cell: &str) -> f64 {
    use diacraft_doc::{CellAddr, Owner, Resolver, SheetRef};
    let res = Resolver::new(d);
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    let found = res.resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("{cell} holds no number"))
}

#[test]
fn the_glue_comes_back_from_a_save_in_each_format_and_from_a_save_into_the_file_that_was_opened() {
    // A connector from shape 1 (no points) to shape 3 (four points), saved as a `.diacraft`, a VSDX and an `.odg`, and into the VSDX file the drawing was opened from.
    for ext in ["diacraft", "vsdx", "odg", "in"] {
        let dir = Dir::new();
        let mut s = opened(&dir);
        let made = run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}}));
        let c = id_of(&made);
        let (box_1, box_3) = (box_of(doc(&s), 1), box_of(doc(&s), 3));
        let begin = end_at(doc(&s), c, true);
        let path = if ext == "in" {
            run(&mut s, "file.save", json!({}));
            dir.path("in.vsdx")
        } else {
            let path = dir.path(&format!("out.{ext}"));
            run(&mut s, "file.save_as", json!({ "path": path }));
            path
        };
        let mut back = reopened(&path);
        let d = doc(&back);
        let (connector, begin_glue, end_glue) = glued_ends(d);
        let page = d.pages.get(0).unwrap().id;
        let (lo, hi, part) = begin_glue.unwrap_or_else(|| panic!("{ext}: the begin is glued"));
        assert!(same_box((lo, hi), box_1), "{ext}: the begin is glued to the shape that was shape 1: {:?}", (lo, hi));
        assert_eq!(part, 3, "{ext}: to the whole of the shape");
        let (lo, hi, _) = end_glue.unwrap_or_else(|| panic!("{ext}: the end is glued"));
        assert!(same_box((lo, hi), box_3), "{ext}: the end is glued to the shape that was shape 3: {:?}", (lo, hi));
        // And the end is still where it was, on the box.
        let at = (num_on(d, page, connector, "BeginX"), num_on(d, page, connector, "BeginY"));
        assert!(distance(at, begin) < 1.0e-3, "{ext}: the begin {at:?} is where it was, {begin:?}");
        assert!(on_outline(at, box_1), "{ext}: and on the box");
        // The shape moves in the file as it did: the end goes with it.
        let shape = d.page(page).unwrap().connects.iter().find(|c| c.from_cell.as_deref() == Some("BeginX")).map(|c| c.to_sheet).unwrap();
        run(&mut back, "shape.move", json!({"page": page, "ids": [shape], "dx": 0.0, "dy": 3.0}));
        let d = doc(&back);
        let moved = shape_corners(d, page, shape).unwrap();
        let at = (num_on(d, page, connector, "BeginX"), num_on(d, page, connector, "BeginY"));
        assert!(on_outline(at, (moved[0], moved[2])), "{ext}: after a move the begin {at:?} is on the box {moved:?}");
        assert_eq!(check_session(&back), Ok(()), "{ext}");
    }
}

#[test]
fn a_shape_with_no_points_on_a_layer_that_allows_no_glue_is_refused_in_words_like_any_shape() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    run(&mut s, "layer.add", json!({"name": "Bare", "glue": false}));
    run(&mut s, "layer.assign", json!({"ids": [1], "layers": ["Bare"]}));
    let before = doc(&s).clone();
    let e = s.execute("connector.add", &json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}})).unwrap_err().to_string();
    assert!(e.contains("shape 1 ") && e.contains("Glue box is off"), "{e}");
    assert_eq!(*doc(&s), before, "a refusal changes nothing");
    // A shape of no layer is glued to, and the end cannot then be moved to the one on the layer.
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 2}, "to": {"id": 3}})));
    let e = s.execute("connector.reconnect", &json!({"id": c, "end": "begin", "target": {"id": 1}})).unwrap_err().to_string();
    assert!(e.contains("shape 1 ") && e.contains("Glue box is off"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_is_no_whole_shape_to_glue_to_and_is_refused_in_words_as_before() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let c = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 1}, "to": {"id": 3}})));
    let before = doc(&s).clone();
    for params in [json!({"page": 1, "from": {"id": c}, "to": {"id": 2}}), json!({"page": 1, "from": {"id": 2}, "to": {"id": c}})] {
        let e = s.execute("connector.add", &params).unwrap_err().to_string();
        assert!(e.contains(&format!("shape {c} is a connector or a line")), "{e}");
        assert_eq!(*doc(&s), before, "nothing was made");
    }
    let other = id_of(&run(&mut s, "connector.add", json!({"page": 1, "from": {"id": 2}, "to": {"id": 3}})));
    let before = doc(&s).clone();
    let e = s.execute("connector.reconnect", &json!({"id": other, "end": "begin", "target": {"id": c}})).unwrap_err().to_string();
    assert!(e.contains(&format!("shape {c} is a connector or a line")), "{e}");
    assert_eq!(*doc(&s), before);
}
