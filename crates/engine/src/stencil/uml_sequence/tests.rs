//! UML Sequence (tests first): the fourteen masters, a lifeline's head and stem points, the messages' ends and patterns (the design's section 6.1), a self message's loop, a combined
//! fragment's tag and its place as a container, the activation's fill (DD2 of the batch's notation spec), the lifelines' locked width and the search words. The catalogue's invariants
//! (`stencil/catalogue_tests.rs`) and the five per-stencil checks (`tests/it/stencils2_uml_sequence.rs`) run on every master beside these.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, SectionKey, SheetRef, TextPart};
use serde_json::{Value, json};

use crate::stencil::{MasterSpec, Role, builtin_stencil, find_master, stencils};
use crate::{Session, draw};

// ---- what the design lists ------------------------------------------------------------------------------------------------------------

/// The fourteen masters of UML Sequence as the design's section 6.1 gives them, in the order of the Shapes window (the Quick Shapes are its first four that are not lines): id, name, width and height
/// in inches, how many connection points, and whether the master is a line.
const MASTERS: [(&str, &str, f64, f64, usize, bool); 14] = [
    ("uml-sequence.lifeline", "Lifeline", 1.2, 3.0, 43, false),
    ("uml-sequence.actor-lifeline", "Actor Lifeline", 1.0, 3.0, 43, false),
    ("uml-sequence.activation", "Activation", 0.16, 1.0, 12, false),
    ("uml-sequence.fragment", "Combined Fragment", 3.0, 1.6, 12, false),
    ("uml-sequence.destruction", "Destruction", 0.3, 0.3, 4, false),
    ("uml-sequence.operand-separator", "Operand Separator", 3.0, 0.25, 4, false),
    ("uml-sequence.interaction-use", "Interaction Use", 2.4, 0.8, 12, false),
    ("uml-sequence.note", "Note", 1.4, 0.8, 4, false),
    ("uml-sequence.message", "Message", 1.5, 0.0, 0, true),
    ("uml-sequence.async-message", "Asynchronous Message", 1.5, 0.0, 0, true),
    ("uml-sequence.reply-message", "Reply Message", 1.5, 0.0, 0, true),
    ("uml-sequence.create-message", "Create Message", 1.5, 0.0, 0, true),
    ("uml-sequence.self-message", "Self Message", 0.6, 0.0, 0, true),
    ("uml-sequence.note-link", "Note Link", 1.5, 0.0, 0, true),
];

/// The twelve operators of a combined fragment, in the design's order.
const OPERATORS: [&str; 12] = ["alt", "opt", "loop", "par", "break", "critical", "neg", "assert", "strict", "seq", "ignore", "consider"];

fn master(id: &str) -> &'static MasterSpec {
    super::MASTERS.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))
}

/// A session with a new drawing.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// Drops a built-in master of a session's drawing at (x, y) and answers its `ID`.
fn put(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": id, "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

/// A page with the master dropped at (5, 5): the document, the page and the shape's `ID`.
fn dropped(spec: &MasterSpec) -> (Document, u32, u32) {
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, spec, 5.0, 5.0).unwrap();
    (doc, page, info.id)
}

/// What a cell of the shape reads, through its master.
fn cell(doc: &Document, page: u32, id: u32, name: &str) -> Option<f64> {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id)?);
    Resolver::new(doc).resolve(&sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number())
}

fn number(doc: &Document, page: u32, id: u32, name: &str) -> f64 {
    cell(doc, page, id, name).unwrap_or_else(|| panic!("no number in {name} of shape {id}"))
}

/// The shown text of the fields of the text a shape shows, in order, and the characters between them: its own text, or when it has none its master shape's, as the renderer reads it. A
/// member of a group holds no text of its own until it is typed in, so its placeholder is its master's.
fn label(doc: &Document, page: u32, id: u32) -> (Vec<String>, String) {
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    let res = Resolver::new(doc);
    let text = doc
        .shape(&at)
        .unwrap()
        .text
        .clone()
        .or_else(|| res.master_step(&at).ok().flatten().and_then(|step| res.shape(&step.shape).and_then(|master| master.text.clone())));
    let (mut fields, mut chars) = (Vec::new(), String::new());
    for part in text.iter().flat_map(|t| t.parts.iter()) {
        match part {
            TextPart::Chars(c) => chars.push_str(c),
            TextPart::Fld { text, .. } => fields.push(text.to_string()),
            _ => {}
        }
    }
    (fields, chars)
}

// ---- the catalogue of the design ------------------------------------------------------------------------------------------------------

#[test]
fn uml_sequence_holds_the_fourteen_masters_of_the_design_in_order_with_their_sizes_and_points() {
    assert_eq!(super::MASTERS.len(), 14);
    for (at, (m, (id, name, w, h, points, line))) in super::MASTERS.iter().zip(MASTERS).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.is_connector(), line, "{id}: a line or a figure");
        assert_eq!(m.identity, (11, u16::try_from(at + 1).unwrap()), "{id}: series 11, in the window's order");
    }
}

#[test]
fn the_stencil_ships_and_stays_closed_in_a_new_session_and_its_template_is_listed() {
    let stencil = builtin_stencil("uml-sequence");
    assert_eq!(stencil.map(|s| (s.name, s.category, s.default_open, s.masters().len())), Some(("UML Sequence", "Software and Database", false, 14)));
    let listed = crate::stencil::list();
    let open: Vec<&str> = listed["stencils"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!open.contains(&"uml-sequence"), "a new session does not open it: {open:?}");
    let available: Vec<&str> = listed["available"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(available.contains(&"uml-sequence"), "More Shapes lists it: {available:?}");
    assert!(crate::template::templates().iter().any(|t| t.id == "uml_sequence"), "the UML Sequence template is listed now that its stencils ship");
    assert!(stencils().iter().any(|s| s.id == "uml-sequence"));
}

#[test]
fn every_two_dimensional_master_holds_objtype_1_and_the_lines_hold_none() {
    assert_eq!(super::MASTERS.len(), 14, "the stencil holds its fourteen masters, so that this is not a loop over nothing");
    for m in super::MASTERS.iter() {
        let (doc, page, id) = dropped(m);
        if m.is_fixed_1d() {
            assert_eq!(cell(&doc, page, id, "ObjType"), None, "{}: a fixed figure holds none", m.id);
        } else if m.is_connector() {
            // A line holds no 1: it reads the routable 2 it is made with.
            assert_eq!(cell(&doc, page, id, "ObjType"), Some(2.0), "{}: a line holds no 1, and is routable", m.id);
        } else {
            assert_eq!(cell(&doc, page, id, "ObjType"), Some(1.0), "{}: a shape people join", m.id);
        }
    }
}

#[test]
fn a_line_or_a_figure_dropped_holds_its_placeholder_text_as_its_own() {
    // An `.odg` holds no master to take a text from, so the placeholder of a master is the shape's own text (a line's as a figure's), and a line that has none holds none.
    let with: [(&str, &str); 4] = [
        ("uml-sequence.message", "message()"),
        ("uml-sequence.async-message", "signal"),
        ("uml-sequence.create-message", "\u{ab}create\u{bb}"),
        ("uml-sequence.operand-separator", "[else]"),
    ];
    for (id, text) in with {
        let (doc, page, shape) = dropped(master(id));
        let own = draw::text_view(&doc, page, shape).unwrap();
        assert_eq!((own.chars.as_str(), own.fields.len()), (text, 0), "{id}: the shape's own text");
    }
    // (The self message has none: its text block is as long as the line, a few tenths of an inch between two steps of a stem, and a text turns with a line that runs down.)
    for id in [
        "uml-sequence.reply-message",
        "uml-sequence.note-link",
        "uml-sequence.self-message",
        "uml-sequence.note",
        "uml-sequence.activation",
        "uml-sequence.destruction",
    ] {
        let (doc, page, shape) = dropped(master(id));
        assert_eq!(draw::text_view(&doc, page, shape).unwrap().chars, "", "{id}: no placeholder");
    }
}

#[test]
fn the_quick_shapes_are_the_lifeline_the_actor_lifeline_the_activation_and_the_combined_fragment() {
    // The first four that are no line (the default group): the table's order is the Quick Shapes' order.
    let first_four: Vec<&str> = super::MASTERS.iter().filter(|m| !m.is_connector()).take(4).map(|m| m.id).collect();
    assert_eq!(first_four, ["uml-sequence.lifeline", "uml-sequence.actor-lifeline", "uml-sequence.activation", "uml-sequence.fragment"]);
}

// ---- lifelines ------------------------------------------------------------------------------------------------------------------------

/// The twenty steps of a lifeline's stem: a pair of points at each, one facing left and one right, after the three points of the head.
const STEMS: usize = 20;

/// How far under the top of the box the steps of the stem begin, on every kind of lifeline: the tallest head (an actor's figure and its name), so that a step is at one height on all of them.
const STEP_TOP: f64 = 0.85;

/// Where the pair of the stem step `k` is on a lifeline whose box has its bottom at `bottom` and its height `height`: the steps are the twentieths of the box under [`STEP_TOP`], from 0.95 of the way
/// up it (step 0) to the foot (step 19).
fn stem_y(bottom: f64, height: f64, k: usize) -> f64 {
    bottom + (height - STEP_TOP) * ((STEMS - 1 - k) as f64 / STEMS as f64)
}

#[test]
fn a_lifelines_stem_points_follow_its_height() {
    for (id, head) in [("uml-sequence.lifeline", 0.4), ("uml-sequence.actor-lifeline", 0.85)] {
        let mut s = session();
        let lifeline = put(&mut s, id, (5.0, 5.0));
        let (doc, page) = (s.document().unwrap(), 1);
        let points = draw::connection_points(doc, page, lifeline);
        assert_eq!(points.len(), 43, "{id}: three of the head and twenty pairs");
        // At the default size: the box is 3 inches tall, from 3.5 to 6.5.
        for k in 0..STEMS {
            let (left, right) = (&points[3 + 2 * k], &points[3 + 2 * k + 1]);
            let y = stem_y(3.5, 3.0, k);
            assert!((left.y - y).abs() < 1e-6 && (right.y - y).abs() < 1e-6, "{id}: step {k} at {y}: {left:?} {right:?}");
            assert!((left.x - 5.0).abs() < 1e-6 && (right.x - 5.0).abs() < 1e-6, "{id}: down the middle of the stem");
            assert_eq!(((left.dir_x, left.dir_y), (right.dir_x, right.dir_y)), ((-1.0, 0.0), (1.0, 0.0)), "{id}: one faces left and one right");
        }
        // The steps lie on the stem: the first under the head, the last at the foot, and 0.05 of the stem between two.
        let first = stem_y(3.5, 3.0, 0);
        let last = stem_y(3.5, 3.0, STEMS - 1);
        assert!(first < 6.5 - head && (last - 3.5).abs() < 1e-9, "{id}: on the stem and not in the head ({first} .. {last})");
        assert!(((first - last) / (STEMS - 1) as f64 - (3.0 - STEP_TOP) / STEMS as f64).abs() < 1e-9, "{id}: even steps");
        // Taller by two inches with the top where it was: every step is the same share of the stem, from the new foot.
        run(&mut s, "shape.resize", json!({ "id": lifeline, "x": 4.4, "y": 1.5, "width": 1.2, "height": 5.0 }));
        let doc = s.document().unwrap();
        let again = draw::connection_points(doc, page, lifeline);
        assert_eq!(again.len(), 43);
        for k in 0..STEMS {
            let y = stem_y(1.5, 5.0, k);
            assert!((again[3 + 2 * k].y - y).abs() < 1e-6, "{id}: step {k} follows the height: {} for {y}", again[3 + 2 * k].y);
        }
        // The head's three: the middle of its top and the middles of its sides, 0.2 inch under the top for the lifeline.
        assert!(
            (again[0].y - 6.5).abs() < 1e-6 && (again[0].x - 5.0).abs() < 1e-6 && (again[0].dir_y - 1.0).abs() < 1e-9,
            "{id}: the top of the head"
        );
    }
}

#[test]
fn a_step_is_at_one_height_on_a_lifeline_and_on_an_actor_lifeline_of_the_same_height() {
    // The heads differ (0.4 and 0.85 inch), and a message from an actor to an object glued to the same step is level all the same: the steps are shares of the box under the tallest head.
    let mut s = session();
    let actor = put(&mut s, "uml-sequence.actor-lifeline", (2.5, 5.0));
    let object = put(&mut s, "uml-sequence.lifeline", (7.5, 5.0));
    let call = put(&mut s, "uml-sequence.message", (5.0, 8.0));
    for step in [0, 9, 19] {
        run(&mut s, "connector.reconnect", json!({ "page": 1, "id": call, "end": "begin", "target": { "id": actor, "point": 3 + 2 * step + 1 } }));
        run(&mut s, "connector.reconnect", json!({ "page": 1, "id": call, "end": "end", "target": { "id": object, "point": 3 + 2 * step } }));
        let doc = s.document().unwrap();
        let [by, ey] = ["BeginY", "EndY"].map(|n| number(doc, 1, call, n));
        assert!(
            (by - ey).abs() < 1e-9 && (by - stem_y(3.5, 3.0, step)).abs() < 1e-6,
            "step {step} is level from an actor to an object: {by} and {ey}"
        );
    }
}

#[test]
fn a_message_glued_between_two_lifelines_stem_points_is_horizontal() {
    let mut s = session();
    let a = put(&mut s, "uml-sequence.lifeline", (2.5, 5.0));
    let b = put(&mut s, "uml-sequence.lifeline", (7.5, 5.0));
    let call = put(&mut s, "uml-sequence.message", (5.0, 8.0));
    // The right-facing point of step 6 of A (the 7th pair: index 3 + 2 * 6 + 1) to the left-facing point of step 6 of B.
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": call, "end": "begin", "target": { "id": a, "point": 3 + 2 * 6 + 1 } }));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": call, "end": "end", "target": { "id": b, "point": 3 + 2 * 6 } }));
    let doc = s.document().unwrap();
    assert_eq!(doc.page(1).unwrap().connects.len(), 2, "glued at both ends");
    let ends = |doc: &Document| ["BeginX", "BeginY", "EndX", "EndY"].map(|n| number(doc, 1, call, n));
    let [bx, by, ex, ey] = ends(doc);
    assert!((by - ey).abs() < 1e-9, "level: {by} and {ey}");
    assert!((by - stem_y(3.5, 3.0, 6)).abs() < 1e-6, "at the step of the points: {by}");
    assert!((bx - 2.5).abs() < 1e-6 && (ex - 7.5).abs() < 1e-6, "from one stem to the other: {bx} to {ex}");
    // Both lifelines moved down together: the message follows and is level still. Its angle is 0.
    run(&mut s, "shape.move", json!({ "ids": [a, b], "dx": 0.0, "dy": -0.7 }));
    let doc = s.document().unwrap();
    let [_, by2, _, ey2] = ends(doc);
    assert!((by2 - ey2).abs() < 1e-9 && (by2 - (by - 0.7)).abs() < 1e-6, "level after the move: {by2} and {ey2}");
    assert!(number(doc, 1, call, "Angle").abs() < 1e-9, "a level message is not turned");
}

#[test]
fn each_message_has_the_end_and_pattern_of_6_1() {
    // The design's section 6.1 and V2: the synchronous call a solid head (4), the asynchronous one and the reply and the create message an open one (3), the reply and the create message dashed (2),
    // every begin plain; the note link a dashed line with no end; the messages straight, and the self message not routed.
    let table: [(&str, (u8, u8, u8)); 6] = [
        ("uml-sequence.message", (0, 4, 1)),
        ("uml-sequence.async-message", (0, 3, 1)),
        ("uml-sequence.reply-message", (0, 3, 2)),
        ("uml-sequence.create-message", (0, 3, 2)),
        ("uml-sequence.self-message", (0, 4, 1)),
        ("uml-sequence.note-link", (0, 0, 2)),
    ];
    let mut s = session();
    let listed = run(&mut s, "stencil.open", json!({ "id": "uml-sequence" }));
    assert_eq!(listed["id"], "uml-sequence", "{listed}");
    let listing = run(&mut s, "stencil.list", json!({}));
    let stencil = listing["stencils"].as_array().unwrap().iter().find(|t| t["id"] == "uml-sequence").expect("the stencil is open now");
    for (id, (begin, end, pattern)) in table {
        let m = master(id);
        assert_eq!(m.look().map(|l| l.numbers()), Some((begin, end, pattern)), "{id}: the master's look");
        let entry = stencil["masters"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap();
        assert_eq!(entry["ends"], json!({ "begin": begin, "end": end, "pattern": pattern }), "{id}: what the listing gives the tile");
        // The instance inherits them: its own cells hold none, and what it reads is the master's.
        let (doc, page, shape) = dropped(m);
        let at = doc.find_shape(Owner::Page(page), shape).unwrap();
        assert!(doc.shape(&at).unwrap().sheet.cell("BeginArrow").is_none(), "{id}: no begin end of its own");
        assert!(doc.shape(&at).unwrap().sheet.cell("EndArrow").is_none(), "{id}: no end of its own");
        assert_eq!(number(&doc, page, shape, "BeginArrow") as u8, begin, "{id}: the begin end it reads");
        assert_eq!(number(&doc, page, shape, "EndArrow") as u8, end, "{id}: the end it reads");
        assert_eq!(cell(&doc, page, shape, "LinePattern").map_or(1, |p| p as u8), pattern, "{id}: the pattern it reads (a solid line holds none)");
        let route = number(&doc, page, shape, "ShapeRouteStyle") as u8;
        assert_eq!(route, if id == "uml-sequence.self-message" { 16 } else { 2 }, "{id}: straight, and the self message not routed");
    }
    assert!(master("uml-sequence.self-message").is_fixed_1d(), "the self message draws its own loop");
    for line in MASTERS.iter().filter(|m| m.5 && m.0 != "uml-sequence.self-message") {
        assert!(!master(line.0).is_fixed_1d(), "{}: an ordinary connector", line.0);
    }
}

/// The rows of the first `Geometry` section of a shape: the kind of each row, and its X and Y.
fn loop_rows(doc: &Document, page: u32, id: u32) -> Vec<(String, f64, f64)> {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
    let rows = Resolver::new(doc).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    rows.iter()
        .map(|row| {
            let at =
                |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap();
            (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
        })
        .collect()
}

#[test]
fn a_self_message_glues_both_ends_to_one_stem_and_keeps_its_loop() {
    let mut s = session();
    let lifeline = put(&mut s, "uml-sequence.lifeline", (5.0, 5.0));
    let to_self = put(&mut s, "uml-sequence.self-message", (8.0, 8.0));
    // The right-facing points of step 5 and step 7: the message runs down the stem a quarter of an inch, 0.25 apart.
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": to_self, "end": "begin", "target": { "id": lifeline, "point": 3 + 2 * 5 + 1 } }));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": to_self, "end": "end", "target": { "id": lifeline, "point": 3 + 2 * 7 + 1 } }));
    let doc = s.document().unwrap();
    let connects = &doc.page(1).unwrap().connects;
    assert_eq!(connects.len(), 2, "glued at both ends");
    assert!(connects.iter().all(|c| c.to_sheet == lifeline), "both ends are on the one lifeline");
    let ends = |doc: &Document| ["BeginX", "BeginY", "EndX", "EndY"].map(|n| number(doc, 1, to_self, n));
    let [bx, by, ex, ey] = ends(doc);
    assert!((bx - 5.0).abs() < 1e-6 && (ex - 5.0).abs() < 1e-6, "both on the stem: {bx} and {ex}");
    assert!((by - stem_y(3.5, 3.0, 5)).abs() < 1e-6 && (ey - stem_y(3.5, 3.0, 7)).abs() < 1e-6, "at the steps: {by} and {ey}");
    assert!(by > ey, "later steps are lower, so the message runs down");
    // It keeps its loop: the master's four rows, out 0.4 inch across the line and back, whose far side is the line's own length.
    let rows = loop_rows(doc, 1, to_self);
    assert_eq!(
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["MoveTo", "LineTo", "LineTo", "LineTo"],
        "the same four rows: no route was laid over the loop"
    );
    let width = number(doc, 1, to_self, "Width");
    assert!((width - (by - ey)).abs() < 1e-6, "the length of the line is the distance between the steps");
    let expected = [(0.0, 0.0), (0.0, 0.4), (width, 0.4), (width, 0.0)];
    for (row, (x, y)) in rows.iter().zip(expected) {
        assert!((row.1 - x).abs() < 1e-6 && (row.2 - y).abs() < 1e-6, "the loop is out 0.4 inch by a fixed length: {rows:?}");
    }
    assert_eq!((number(doc, 1, to_self, "BeginArrow"), number(doc, 1, to_self, "EndArrow")), (0.0, 4.0), "a solid head at the end");
    // The lifeline moves: the ends follow, the loop is stretched along the line and no route is laid over it.
    run(&mut s, "shape.move", json!({ "ids": [lifeline], "dx": 1.0, "dy": -0.5 }));
    let doc = s.document().unwrap();
    let [bx2, by2, ex2, ey2] = ends(doc);
    assert!((bx2 - 6.0).abs() < 1e-6 && (ex2 - 6.0).abs() < 1e-6 && (by2 - (by - 0.5)).abs() < 1e-6 && (ey2 - (ey - 0.5)).abs() < 1e-6);
    let kinds: Vec<String> = loop_rows(doc, 1, to_self).into_iter().map(|r| r.0).collect();
    assert_eq!(kinds, ["MoveTo", "LineTo", "LineTo", "LineTo"], "still the loop after the move");
    // The lifeline grows taller: the steps move apart, and the loop is stretched with them.
    run(&mut s, "shape.resize", json!({ "id": lifeline, "x": 5.4, "y": 1.0, "width": 1.2, "height": 5.0 }));
    let doc = s.document().unwrap();
    let grown = number(doc, 1, to_self, "Width");
    assert!(grown > width + 0.1, "the loop is longer along the line: {width} to {grown}");
    let rows = loop_rows(doc, 1, to_self);
    assert!((rows[2].1 - grown).abs() < 1e-6 && (rows[2].2 - 0.4).abs() < 1e-9, "the far side follows the length and stays 0.4 inch out: {rows:?}");
}

// ---- the combined fragment and its tag ------------------------------------------------------------------------------------------------

/// The shape's members, in the order of the group.
fn members(doc: &Document, page: u32, id: u32) -> Vec<u32> {
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    doc.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

#[test]
fn the_fragments_operator_field_shows_in_its_tag() {
    let mut s = session();
    let fragment = put(&mut s, "uml-sequence.fragment", (5.0, 5.0));
    let doc = s.document().unwrap();
    let parts = members(doc, 1, fragment);
    assert_eq!(parts.len(), 3, "the frame, the tag and the guard");
    let (tag, guard) = (parts[1], parts[2]);
    // A new fragment is an alt: the tag shows the field, which reads the group's Shape Data, and the guard is the last member, so that typing on the selected fragment goes to it.
    assert_eq!(label(doc, 1, tag), (vec!["alt".to_owned()], String::new()), "the tag shows the operator, as a field and not as typed text");
    assert_eq!(label(doc, 1, guard), (Vec::<String>::new(), "[condition]".to_owned()), "the guard is placeholder text");
    assert_eq!(number(doc, 1, fragment, "IsTextEditTarget"), 0.0, "typing goes to a member");
    let listed = run(&mut s, "shapedata.list", json!({ "id": fragment }));
    let field = listed["fields"].as_array().unwrap().iter().find(|f| f["row"] == "Operator").expect("an Operator field");
    assert_eq!((field["type_code"].clone(), field["value"].clone()), (json!(1), json!("alt")), "a fixed list that starts as alt");
    assert_eq!(field["format"], json!(OPERATORS.join(";")), "its twelve words, in the design's order");
    // Each of the twelve words reaches the tag.
    for word in OPERATORS {
        run(&mut s, "shapedata.set", json!({ "id": fragment, "row": "Operator", "value": word }));
        run(&mut s, "text.update_fields", json!({}));
        assert_eq!(label(s.document().unwrap(), 1, tag).0, [word], "the tag shows {word}");
    }
    // A word that is no item is refused in words, and the tag is what it was.
    let said = s.execute("shapedata.set", &json!({ "id": fragment, "row": "Operator", "value": "whenever" })).unwrap_err().to_string();
    assert!(said.contains("whenever") || said.contains("list"), "the words say why: {said}");
    assert_eq!(label(s.document().unwrap(), 1, tag).0, ["consider"]);
    // The tag's field is a formula of the group's Shape Data.
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), fragment).unwrap();
    let master = doc.master(doc.shape(&at).unwrap().master.unwrap()).unwrap();
    let tag_shape = master.shapes.get(0).unwrap().children.get(1).unwrap().clone();
    let formulas: Vec<String> = tag_shape
        .sheet
        .sections
        .iter()
        .filter(|sec| sec.name.as_str() == "Field")
        .flat_map(|sec| sec.rows.iter())
        .filter_map(|r| match &r.cells.get("Value")?.formula {
            FormulaSlot::Local(f) => Some(f.source.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(formulas, ["Sheet.5!Prop.Operator"]);
}

#[test]
fn a_fragment_is_a_container_that_takes_a_dropped_shape_and_moves_it_and_the_tag_stays_a_fixed_size() {
    let m = master("uml-sequence.fragment");
    assert!(matches!(m.dress().map(|d| d.role), Some(Role::Container { .. })), "a container master");
    let mut s = session();
    let fragment = put(&mut s, "uml-sequence.fragment", (5.0, 5.0));
    let info = run(&mut s, "container.info", json!({ "ids": [fragment] }));
    assert_eq!(info["containers"][0]["id"], fragment, "an instance of it is a container: {info}");
    // An activation dropped inside joins it, a shape outside does not.
    let inside = put(&mut s, "uml-sequence.activation", (5.0, 4.8));
    let outside = put(&mut s, "uml-sequence.activation", (9.5, 8.0));
    let info = run(&mut s, "container.info", json!({ "ids": [fragment] }));
    assert_eq!(info["containers"][0]["members"], json!([inside]), "{outside} is not a member: {info}");
    let before = draw::shape_corners(s.document().unwrap(), 1, inside).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [fragment], "dx": 1.0, "dy": -0.5 }));
    let after = draw::shape_corners(s.document().unwrap(), 1, inside).unwrap();
    assert!(
        before.iter().zip(after.iter()).all(|(a, b)| (a.0 + 1.0 - b.0).abs() < 1e-6 && (a.1 - 0.5 - b.1).abs() < 1e-6),
        "a move of the fragment takes its member"
    );
    // Fit to Contents passes over a group (it is not resized to its members): the fragment is as big as it was.
    let was = draw::shape_frame(s.document().unwrap(), 1, fragment).unwrap();
    let fitted = run(&mut s, "container.fit", json!({ "ids": [fragment] }));
    assert_eq!(draw::shape_frame(s.document().unwrap(), 1, fragment).unwrap(), was, "a group is not resized by Fit to Contents: {fitted}");
    // A wider fragment keeps its tag 0.6 by 0.25 inch: the tag is a fixed size, whatever the frame.
    run(&mut s, "shape.resize", json!({ "id": fragment, "x": 3.0, "y": 3.5, "width": 6.0, "height": 2.4 }));
    let doc = s.document().unwrap();
    let tag = members(doc, 1, fragment)[1];
    assert!((number(doc, 1, tag, "Width") - 0.6).abs() < 1e-6 && (number(doc, 1, tag, "Height") - 0.25).abs() < 1e-6, "the tag is 0.6 by 0.25 inch");
    // The tag is at the frame's top left corner: its top left is the group's.
    let frame = draw::shape_frame(doc, 1, fragment).unwrap();
    let (left, top) = (frame.pin_x - frame.width / 2.0, frame.pin_y + frame.height / 2.0);
    let corners = draw::shape_corners(doc, 1, tag).unwrap();
    let (tag_left, tag_top) =
        (corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min), corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max));
    assert!(
        (tag_left - left).abs() < 1e-6 && (tag_top - top).abs() < 1e-6,
        "the tag's corner is the frame's: ({tag_left}, {tag_top}) for ({left}, {top})"
    );
}

// ---- the activation, the lifelines' width and the words -------------------------------------------------------------------------------

#[test]
fn an_activation_takes_the_styles_fill_and_not_the_line_colour() {
    // DD2 and V3: a thin rectangle in white or light grey over the lifeline. On a page with no theme the middle of the bar is light and its edge is dark.
    let mut s = session();
    let bar = put(&mut s, "uml-sequence.activation", (5.5, 4.25));
    assert_eq!(cell(s.document().unwrap(), 1, bar, "FillPattern").unwrap_or(1.0), 1.0, "a solid fill");
    let drawn = super::super::flowchart_tests::render(&s);
    let centre = super::super::flowchart_tests::px(&drawn, 5.5, 4.25);
    assert!(
        centre[3] > 128 && u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 600,
        "the middle of the bar is light: {centre:?}"
    );
    let edge = super::super::flowchart_tests::px(&drawn, 5.5 - 0.08, 4.25);
    assert!(edge[3] > 128 && u32::from(edge[0]) + u32::from(edge[1]) + u32::from(edge[2]) < 400, "its edge is dark: {edge:?}");
}

#[test]
fn a_set_of_lifelines_stretched_keeps_each_head_width_and_sizes_each_stem() {
    // LOCKAXIS: a lifeline holds `LockWidth`, so a stretch of several keeps the width of each head, moves their middles with the stretch and sizes each along its height.
    for id in ["uml-sequence.lifeline", "uml-sequence.actor-lifeline"] {
        let mut s = session();
        let a = put(&mut s, id, (3.0, 5.0));
        let b = put(&mut s, id, (7.0, 5.0));
        assert_eq!(number(s.document().unwrap(), 1, a, "LockWidth"), 1.0, "{id}: the width is locked");
        let widths = [number(s.document().unwrap(), 1, a, "Width"), number(s.document().unwrap(), 1, b, "Width")];
        // The joint box of the two (neither is turned): from the left of the first to the right of the second, and the height of both. Stretch it to 8 inches wide and an inch taller.
        let (fa, fb) = (draw::shape_frame(s.document().unwrap(), 1, a).unwrap(), draw::shape_frame(s.document().unwrap(), 1, b).unwrap());
        let (left, bottom, tall) = (fa.pin_x - fa.width / 2.0, fa.pin_y - fa.height / 2.0, fa.height.max(fb.height));
        let result = run(&mut s, "shape.resize", json!({ "ids": [a, b], "x": left, "y": bottom - 0.5, "width": 8.0, "height": tall + 1.0 }));
        assert_eq!(result["resized"], 2, "{id}: both are sized: {result}");
        let doc = s.document().unwrap();
        for (shape, was) in [a, b].into_iter().zip(widths) {
            assert!((number(doc, 1, shape, "Width") - was).abs() < 1e-9, "{id}: the head keeps its width");
            assert!(number(doc, 1, shape, "Height") > 3.9, "{id}: the stem is longer");
        }
        let (fa, fb) = (draw::shape_frame(doc, 1, a).unwrap(), draw::shape_frame(doc, 1, b).unwrap());
        assert!(fb.pin_x - fa.pin_x > 4.0 + 1.0, "{id}: the lifelines are further apart: {} and {}", fa.pin_x, fb.pin_x);
    }
}

#[test]
fn every_master_has_search_words_and_the_inventorys_names_find_the_master_that_serves_them() {
    let mut s = session();
    for m in super::MASTERS.iter() {
        assert!(!super::keywords(m.id).is_empty(), "{} has no search words", m.id);
    }
    // The names of the inventory's sample lists for this notation (C68, C75), as a person types them, and the master each is for.
    let names: [(&str, &str); 11] = [
        ("Object lifeline", "uml-sequence.lifeline"),
        ("Actor lifeline", "uml-sequence.actor-lifeline"),
        ("Activation bar", "uml-sequence.activation"),
        ("Alternative fragment", "uml-sequence.fragment"),
        ("Interaction operand", "uml-sequence.operand-separator"),
        ("Return Message", "uml-sequence.reply-message"),
        ("Asynchronous Message", "uml-sequence.async-message"),
        ("Self Message", "uml-sequence.self-message"),
        ("Message", "uml-sequence.message"),
        ("Loop", "uml-sequence.fragment"),
        ("Optional", "uml-sequence.fragment"),
    ];
    for (typed, wanted) in names {
        let answer = run(&mut s, "stencil.search", json!({ "text": typed }));
        let found: Vec<&str> = answer["results"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["masters"].as_array().unwrap().iter().filter_map(|m| m["id"].as_str()))
            .collect();
        assert!(found.contains(&wanted), "`{typed}` should find {wanted}: {found:?}");
    }
    assert!(find_master("uml-sequence.lifeline").is_some(), "`shape.drop` finds it by id");
}

// ---- the lifeline's text --------------------------------------------------------------------------------------------------------------

#[test]
fn typing_on_a_selected_lifeline_goes_to_its_head_the_last_member_and_the_stem_is_dashed() {
    let mut s = session();
    let lifeline = put(&mut s, "uml-sequence.lifeline", (5.0, 5.0));
    let doc = s.document().unwrap();
    let parts = members(doc, 1, lifeline);
    assert_eq!(parts.len(), 2, "the stem and the head");
    let (stem, head) = (parts[0], parts[1]);
    assert_eq!(number(doc, 1, lifeline, "IsTextEditTarget"), 0.0, "the members hold the text");
    assert_eq!(label(doc, 1, head), (Vec::<String>::new(), "object : Class".to_owned()), "the head starts as the notation writes an instance");
    assert_eq!(number(doc, 1, stem, "LinePattern"), 2.0, "the stem is dashed");
    assert!((number(doc, 1, head, "Height") - 0.4).abs() < 1e-6, "the head is 0.4 inch tall");
    assert!(
        (number(doc, 1, head, "Width") - 1.2).abs() < 1e-6 && (number(doc, 1, stem, "Height") - 2.6).abs() < 1e-6,
        "the stem is the rest of the height"
    );
    // A taller lifeline has a longer stem and the same head.
    run(&mut s, "shape.resize", json!({ "id": lifeline, "x": 4.4, "y": 1.5, "width": 1.2, "height": 5.0 }));
    let doc = s.document().unwrap();
    assert!((number(doc, 1, head, "Height") - 0.4).abs() < 1e-6 && (number(doc, 1, stem, "Height") - 4.6).abs() < 1e-6);
    // Typing in the head changes its text and nothing else.
    run(&mut s, "shape.set_text", json!({ "page": 1, "id": head, "text": "order : Basket" }));
    assert_eq!(label(s.document().unwrap(), 1, head).1, "order : Basket");
}

#[test]
fn an_actor_lifeline_has_a_figure_a_name_under_it_and_a_dashed_stem_below_both() {
    let mut s = session();
    let lifeline = put(&mut s, "uml-sequence.actor-lifeline", (5.0, 5.0));
    let doc = s.document().unwrap();
    let parts = members(doc, 1, lifeline);
    assert_eq!(parts.len(), 3, "the stem, the figure and the name");
    let (stem, figure, name) = (parts[0], parts[1], parts[2]);
    assert_eq!(label(doc, 1, name), (Vec::<String>::new(), "Actor".to_owned()), "the name is the last member");
    assert_eq!(number(doc, 1, stem, "LinePattern"), 2.0, "the stem is dashed");
    assert!((number(doc, 1, figure, "Height") - 0.6).abs() < 1e-6, "the figure is 0.6 inch tall (the design)");
    // The name is under the figure and the stem under the name: from the top, 0.6 and 0.25 inch, the stem the rest of the 3 inches.
    assert!((number(doc, 1, stem, "Height") - (3.0 - 0.85)).abs() < 1e-6, "the stem starts under the figure and its name");
}

// ---- hostile input ----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_very_long_text_typed_in_a_head_or_a_guard_leaves_the_geometry_finite_and_the_points_where_they_were() {
    let long = "w".repeat(100_000);
    for (master_id, member) in [("uml-sequence.lifeline", 1), ("uml-sequence.fragment", 2)] {
        let mut s = session();
        let shape = put(&mut s, master_id, (5.0, 5.0));
        let target = members(s.document().unwrap(), 1, shape)[member];
        let before = draw::connection_points(s.document().unwrap(), 1, shape);
        // The text is taken up to the cap, or refused in words: the document is whole either way, and no height depends on a text here.
        if let Err(refused) = s.execute("shape.set_text", &json!({ "page": 1, "id": target, "text": long })) {
            assert!(!refused.to_string().is_empty(), "{master_id}: a refusal says why");
        }
        let doc = s.document().unwrap();
        let frame = draw::shape_frame(doc, 1, shape).unwrap();
        assert!(frame.width.is_finite() && frame.height.is_finite() && frame.height > 0.0, "{master_id}: finite: {frame:?}");
        assert_eq!(draw::connection_points(doc, 1, shape), before, "{master_id}: the text moves no point");
    }
}

#[test]
fn a_lifeline_made_shorter_than_its_head_stays_finite_and_its_points_stay_on_its_box() {
    // The head is 0.4 inch and the steps begin 0.85 inch under the top: a lifeline of a tenth of an inch, or of 0.5, has a stem of nothing, and no cell is negative, infinite or not a number.
    for id in ["uml-sequence.lifeline", "uml-sequence.actor-lifeline"] {
        for height in [0.1, 0.5, 0.85] {
            let mut s = session();
            let lifeline = put(&mut s, id, (5.0, 5.0));
            run(&mut s, "shape.resize", json!({ "id": lifeline, "x": 4.4, "y": 5.0, "width": 1.2, "height": height }));
            let doc = s.document().unwrap();
            let frame = draw::shape_frame(doc, 1, lifeline).unwrap();
            assert!((frame.height - height).abs() < 1e-9, "{id}: sized to {height}: {frame:?}");
            for member in members(doc, 1, lifeline) {
                let (w, h) = (number(doc, 1, member, "Width"), number(doc, 1, member, "Height"));
                assert!(w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0, "{id} at {height}: member {member} is {w} by {h}");
            }
            let points = draw::connection_points(doc, 1, lifeline);
            assert_eq!(points.len(), 43, "{id} at {height}");
            assert!(points.iter().all(|p| p.x.is_finite() && p.y.is_finite()), "{id} at {height}: every point is a number");
        }
    }
}
