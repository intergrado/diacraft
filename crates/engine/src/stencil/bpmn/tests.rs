//! BPMN Shapes (tests first): the nineteen masters, their Shape Data in the reference application's row names and words, the markers a value of
//! each variant field shows, the throwing markers dark and the catching ones hollow, the dashed, dotted and thick borders, the flows' ends and guarded patterns, and
//! the Pool and the Lane that make and join a structure of swimlanes. The catalogue's invariants (`stencil/catalogue_tests.rs`) and the five per-stencil checks
//! (`tests/it/stencils2_bpmn.rs`) run on every master beside these.
//!
//! The pictures are looked at by pixels in paper inches, round the shape's centre, as `flowchart_tests.rs` does: **a value that shows a marker changes the pixels inside that marker's box, and
//! none outside it**.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value, json};

use crate::stencil::{MasterSpec, builtin_stencil};
use crate::{Session, draw};

// ---- what the design and the spec list ------------------------------------------------------------------------------------------------

/// The nineteen masters of BPMN Shapes in the order the Shapes window lists them (the Quick Shapes are the first four that are no lines: Task, Gateway, Start Event, End Event): id, name,
/// width, height (inches) and how many connection points.
const BPMN: [(&str, &str, f64, f64, usize); 19] = [
    ("bpmn.task", "Task", 1.4, 0.9, 12),
    ("bpmn.gateway", "Gateway", 0.6, 0.6, 4),
    ("bpmn.start-event", "Start Event", 0.4, 0.4, 8),
    ("bpmn.end-event", "End Event", 0.4, 0.4, 8),
    ("bpmn.intermediate-event", "Intermediate Event", 0.4, 0.4, 8),
    ("bpmn.sub-process", "Sub-Process", 1.4, 0.9, 12),
    ("bpmn.expanded-sub-process", "Expanded Sub-Process", 3.0, 2.0, 12),
    ("bpmn.call-activity", "Call Activity", 1.4, 0.9, 12),
    ("bpmn.data-object", "Data Object", 0.5, 0.65, 4),
    ("bpmn.data-store", "Data Store", 0.7, 0.6, 4),
    ("bpmn.message", "Message", 0.6, 0.4, 4),
    ("bpmn.group", "Group", 2.5, 1.6, 12),
    ("bpmn.text-annotation", "Text Annotation", 1.6, 0.6, 5),
    ("bpmn.pool", "Pool", 8.0, 2.0, 4),
    ("bpmn.lane", "Lane", 7.6, 1.5, 4),
    ("bpmn.sequence-flow", "Sequence Flow", 1.5, 0.0, 0),
    ("bpmn.message-flow", "Message Flow", 1.5, 0.0, 0),
    ("bpmn.association", "Association", 1.5, 0.0, 0),
    ("bpmn.data-association", "Data Association", 1.5, 0.0, 0),
];

/// The Shape Data rows of each master, as name them: the master's id, then each row's name, what it holds (`list:` with its items joined by `;`, or `bool`) and its default.
type Row = (&'static str, &'static str, &'static str);
const LOOP_WORDS: &str = "list:None;Standard;Parallel MultiInstance;Sequential MultiInstance";
const BOUNDARY_WORDS: &str = "list:Default;Call;Event;Transaction";
const START_WORDS: &str = "list:None;Message;Timer;Conditional;Signal;Multiple;Parallel Multiple;Escalation;Error;Compensation";
const INTERMEDIATE_WORDS: &str = "list:None;Message;Timer;Escalation;Conditional;Link;Error;Cancel;Compensation;Signal;Multiple;Parallel Multiple";
const END_WORDS: &str = "list:None;Message;Escalation;Error;Cancel;Compensation;Signal;Terminate;Multiple";
const TASK_WORDS: &str = "list:None;Abstract;Service;Send;Receive;Instantiating Receive;User;Manual;Business Rule;Script";

fn fields_of(id: &str) -> Vec<Row> {
    let activity = |boundary: &'static str| -> Vec<Row> {
        vec![
            ("BpmnLoopType", LOOP_WORDS, "None"),
            ("BpmnIsForCompensation", "bool", "0"),
            ("BpmnBoundaryType", BOUNDARY_WORDS, boundary),
            ("BpmnAdHoc", "bool", "0"),
        ]
    };
    match id {
        "bpmn.task" => vec![("BpmnTaskType", TASK_WORDS, "None"), ("BpmnLoopType", LOOP_WORDS, "None"), ("BpmnIsForCompensation", "bool", "0")],
        "bpmn.sub-process" | "bpmn.expanded-sub-process" => activity("Default"),
        "bpmn.call-activity" => activity("Call"),
        "bpmn.start-event" => vec![("BpmnEventType", "list:Start;Start (Non-Interrupting)", "Start"), ("BpmnTriggerOrResult", START_WORDS, "None")],
        "bpmn.intermediate-event" => vec![
            ("BpmnEventType", "list:Intermediate;Intermediate (Non-Interrupting);Intermediate (Throwing)", "Intermediate"),
            ("BpmnTriggerOrResult", INTERMEDIATE_WORDS, "None"),
        ],
        "bpmn.end-event" => vec![("BpmnEventType", "list:End", "End"), ("BpmnTriggerOrResult", END_WORDS, "None")],
        "bpmn.gateway" => vec![
            ("BpmnGatewayType", "list:Exclusive;Exclusive Event (Instantiate);Inclusive;Parallel;Parallel Event (Instantiate);Complex", "Exclusive"),
            ("BpmnExclusiveType", "list:Data;Event", "Data"),
            ("BpmnMarkerVisible", "bool", "0"),
        ],
        "bpmn.data-object" => vec![("BpmnCollection", "bool", "0"), ("BpmnDataRole", "list:None;Input;Output", "None")],
        "bpmn.sequence-flow" => vec![("BpmnConditionType", "list:None;Expression;Default", "None")],
        "bpmn.association" => vec![("BpmnDirection", "list:None;One;Both", "None")],
        // A data association is drawn as a directed association: its direction begins as One.
        "bpmn.data-association" => vec![("BpmnDirection", "list:None;One;Both", "One")],
        _ => Vec::new(),
    }
}

// ---- helpers --------------------------------------------------------------------------------------------------------------------------

fn masters() -> &'static [MasterSpec] {
    builtin_stencil("bpmn").map_or(&[][..], |s| s.masters())
}

fn master(id: &str) -> &'static MasterSpec {
    masters().iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))
}

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn id_of(v: &Value) -> u32 {
    u32::try_from(v["id"].as_u64().unwrap()).unwrap()
}

/// Where the one shape of a test is dropped: its centre, on page 1 (11 by 8.5 inches).
const CENTRE: (f64, f64) = (5.5, 4.25);

fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

/// Drops a master of the BPMN stencil at (`x`, `y`) → the shape's `ID`.
fn drop_at(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let page = first_page(s);
    id_of(&run(s, "shape.drop", json!({ "master": id, "stencil": "bpmn", "page": page, "x": x, "y": y })))
}

/// A new drawing with one master dropped at the centre.
fn one(id: &str) -> (Session, u32) {
    let mut s = session();
    let shape = drop_at(&mut s, id, CENTRE);
    (s, shape)
}

fn set_data(s: &mut Session, id: u32, row: &str, value: Value) {
    run(s, "shapedata.set", json!({ "id": id, "row": row, "value": value }));
}

/// The value of a cell of a shape, read through its master: the number, or the text.
fn read(doc: &Document, page: u32, id: u32, cell: &str) -> Option<String> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

fn number(doc: &Document, page: u32, id: u32, cell: &str) -> Option<f64> {
    read(doc, page, id, cell).and_then(|t| t.parse().ok())
}

/// The number a `User` row of a shape holds.
fn user_value(doc: &Document, page: u32, id: u32, row: &str) -> Option<f64> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::in_row("User", None, diacraft_doc::RowKey::name(row), "Value"))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

// ---- the pictures ---------------------------------------------------------------------------------------------------------------------

const PPI: f64 = 200.0;
/// The window of the page that is drawn, in inches round the centre: `HALF` each way. A pool and a container are looked at in a window of their own.
const HALF: f64 = 1.6;

/// A box in inches, round the centre of the shape: (left, bottom, right, top).
type Box4 = (f64, f64, f64, f64);

/// The page's `window` (left, bottom, right, top, in inches) drawn on white paper at 200 pixels to the inch.
fn render_window(doc: &Document, page: u32, window: Box4) -> Rendered {
    let (x0, _, x1, y1) = window;
    let mut view = View::fit_page(doc, page, PPI).unwrap();
    view.to_px = kurbo::Affine::new([PPI, 0.0, 0.0, -PPI, -x0 * PPI, y1 * PPI]);
    view.width = ((x1 - x0) * PPI).ceil() as u32;
    view.height = ((window.3 - window.1) * PPI).ceil() as u32;
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    frame
}

fn around_centre(half: f64) -> Box4 {
    (CENTRE.0 - half, CENTRE.1 - half, CENTRE.0 + half, CENTRE.1 + half)
}

/// The shape alone, drawn round its centre.
fn picture(s: &Session) -> Rendered {
    render_window(s.document().unwrap(), first_page(s), around_centre(HALF))
}

/// The pixel at an offset (`dx`, `dy` inches, y up) from the centre of the shape.
fn pixel(r: &Rendered, (dx, dy): (f64, f64)) -> [u8; 4] {
    let (col, row) = (((HALF + dx) * PPI) as usize, ((HALF - dy) * PPI) as usize);
    let at = (row.min(r.height as usize - 1) * r.width as usize + col.min(r.width as usize - 1)) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2], p[3]]
}

/// Ink: a pixel clearly darker than white.
fn is_dark(p: [u8; 4]) -> bool {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300
}

/// Paper: a pixel clearly lighter than a line is.
fn is_light(p: [u8; 4]) -> bool {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) > 600
}

/// The box, in offsets from the centre, of the pixels that differ between two pictures of the same window: `None` when they are the same.
fn difference(a: &Rendered, b: &Rendered) -> Option<Box4> {
    assert_eq!((a.width, a.height), (b.width, b.height), "the same window");
    let (mut left, mut right, mut top, mut bottom) = (usize::MAX, 0, usize::MAX, 0);
    let width = a.width as usize;
    for (at, (p, q)) in a.rgba_premul.as_chunks::<4>().0.iter().zip(b.rgba_premul.as_chunks::<4>().0.iter()).enumerate() {
        if p.iter().zip(q).any(|(x, y)| (i32::from(*x) - i32::from(*y)).abs() > 8) {
            let (col, row) = (at % width, at / width);
            left = left.min(col);
            right = right.max(col);
            top = top.min(row);
            bottom = bottom.max(row);
        }
    }
    (right >= left && bottom >= top)
        .then(|| (left as f64 / PPI - HALF, HALF - (bottom as f64 + 1.0) / PPI, (right as f64 + 1.0) / PPI - HALF, HALF - top as f64 / PPI))
}

/// Whether `inner` is within `outer` grown by `slack` inches.
fn within(inner: Box4, outer: Box4, slack: f64) -> bool {
    inner.0 >= outer.0 - slack && inner.1 >= outer.1 - slack && inner.2 <= outer.2 + slack && inner.3 <= outer.3 + slack
}

/// A square of `size` inches round the offset (`x`, `y`).
fn square(x: f64, y: f64, size: f64) -> Box4 {
    (x - size / 2.0, y - size / 2.0, x + size / 2.0, y + size / 2.0)
}

// ---- the catalogue of the design ------------------------------------------------------------------------------------------------------

#[test]
fn bpmn_holds_the_nineteen_masters_in_order_with_their_sizes_and_points() {
    assert_eq!(masters().len(), 19, "the stencil ships its nineteen masters");
    for (at, (m, (id, name, w, h, points))) in masters().iter().zip(BPMN).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (15, u16::try_from(at + 1).unwrap()), "{id}: series 15, in the window's order");
    }
}

#[test]
fn the_stencil_ships_under_flowchart_and_stays_closed_in_a_new_session() {
    let stencil = builtin_stencil("bpmn").expect("bpmn ships");
    assert_eq!((stencil.name, stencil.category, stencil.default_open), ("BPMN Shapes", "Flowchart", false));
    let listed = crate::stencil::list();
    let open: Vec<&str> = listed["stencils"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!open.contains(&"bpmn"), "a new session does not open it: {open:?}");
    let available: Vec<&str> = listed["available"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(available.contains(&"bpmn"), "More Shapes lists it: {available:?}");
}

#[test]
fn the_quick_shapes_are_task_gateway_start_event_and_end_event_and_the_flows_parts_and_containers_stay_off_the_strip() {
    let first_four: Vec<&str> = masters().iter().filter(|m| !m.is_connector() && !m.is_hidden()).take(4).map(|m| m.name).collect();
    assert_eq!(first_four, ["Task", "Gateway", "Start Event", "End Event"]);
    for m in masters() {
        // The Expanded Sub-Process and the Group are containers, which the strip passes over for every stencil (the `is_container`, met by this test).
        let expect = !m.is_connector() && !matches!(m.id, "bpmn.pool" | "bpmn.lane" | "bpmn.expanded-sub-process" | "bpmn.group");
        assert_eq!(m.on_strip(), expect, "{}: the AutoConnect strip offers it or not", m.id);
    }
}

#[test]
fn every_master_has_search_words_and_the_inventorys_names_find_their_masters() {
    for m in masters() {
        assert!(!super::keywords(m.id).is_empty(), "{} has search words", m.id);
    }
    // The names of C5's samples (Task, Gateway, Start Event, Sequence Flow, End Event, Pool / Lane, Collapsed Sub-Process, Data Store) find the masters that serve them.
    for (query, id) in [
        ("Collapsed Sub-Process", "bpmn.sub-process"),
        ("Pool / Lane", "bpmn.pool"),
        ("Pool / Lane", "bpmn.lane"),
        ("Sequence Flow", "bpmn.sequence-flow"),
        ("Data Store", "bpmn.data-store"),
        ("exclusive", "bpmn.gateway"),
        ("timer", "bpmn.start-event"),
    ] {
        let words = super::keywords(id).to_lowercase();
        let wanted = master(id).name.to_lowercase();
        let found =
            query.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).all(|w| words.contains(w) || wanted.contains(w));
        assert!(found, "`{query}` finds {id} by its name or its words: {words}");
    }
}

// ---- the Shape Data: the reference application's rows and words ------------------------------------------------------

/// The `Property` rows of a master as (name, `Type` code, `Format`, `Value`).
fn property_rows(spec: &MasterSpec) -> Vec<(String, String, String, String)> {
    let mut doc = Document::new_blank();
    let id = crate::stencil::ensure_master(&mut doc, spec).unwrap();
    let shape = doc.master(id).unwrap().shapes.get(0).unwrap().clone();
    let Some(section) = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Property") else { return Vec::new() };
    let text = |row: &diacraft_doc::Row, cell: &str| {
        row.cells.iter().find(|(n, _)| n.as_str() == cell).and_then(|(_, c)| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default()
    };
    section.rows.iter().map(|r| (r.name.as_deref().unwrap_or("").to_owned(), text(r, "Type"), text(r, "Format"), text(r, "Value"))).collect()
}

#[test]
fn every_master_has_the_reference_rows_with_their_words_and_defaults_and_no_other() {
    let mut checked = 0;
    for (id, ..) in BPMN {
        let found = property_rows(master(id));
        let wanted = fields_of(id);
        let names: Vec<&str> = found.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(names, wanted.iter().map(|(n, ..)| *n).collect::<Vec<_>>(), "{id}: the rows, in order");
        for ((name, kind, format, value), (_, what, default)) in found.iter().zip(&wanted) {
            if let Some(items) = what.strip_prefix("list:") {
                assert_eq!((kind.as_str(), format.as_str()), ("1", items), "{id}.{name}: a fixed list of the reference words");
                assert_eq!(value, default, "{id}.{name}: its default");
            } else {
                assert_eq!(kind, "3", "{id}.{name}: a boolean");
                assert_eq!(value, default, "{id}.{name}: false to begin with");
            }
            checked += 1;
        }
    }
    assert!(checked >= 28, "the loop looked at the rows: {checked}");
    // The intermediate event must not hold `BpmnDirection` (in the reference application's files it is the association's direction), nor any row of the design's that the reference rows replace.
    for gone in [
        "BpmnDirection",
        "BpmnTrigger",
        "BpmnResult",
        "BpmnInterrupting",
        "BpmnSubProcessKind",
        "BpmnMarkerShown",
        "BpmnCondition",
        "BpmnAssociationDirection",
        "BpmnLoop",
    ] {
        for m in masters() {
            let association = matches!(m.id, "bpmn.association" | "bpmn.data-association");
            let held = property_rows(m).iter().any(|(n, ..)| n == gone);
            assert!(!held || (gone == "BpmnDirection" && association), "{}: no row `{gone}`", m.id);
        }
    }
}

// ---- 2-D masters hold ObjType 1 and the swimlane parts 4 (the owner's ruling) --------------------------------------------------------

#[test]
fn every_two_dimensional_master_holds_objtype_1_and_the_pool_and_the_lane_hold_4() {
    assert_eq!(masters().iter().filter(|m| !m.is_connector()).count(), 15, "fifteen figures and four lines");
    for m in masters().iter().filter(|m| !m.is_connector()) {
        let mut doc = Document::new_blank();
        let id = crate::stencil::ensure_master(&mut doc, m).unwrap();
        let top = doc.master(id).unwrap().shapes.get(0).unwrap().clone();
        let held = top.sheet.cell("ObjType").and_then(|c| c.value.as_ref()).and_then(|v| v.number());
        let wanted = if matches!(m.id, "bpmn.pool" | "bpmn.lane") { 4.0 } else { 1.0 };
        assert_eq!(held, Some(wanted), "{}: ObjType", m.id);
    }
}

// ---- the markers: every value of every variant field shows exactly its marker ----------------------------------------------------------------

/// A variant to try: the master, the row, and the value; and the box (offsets from the shape's centre) that the pixels it changes lie in, or `None` for a value that shows no marker.
type Try = (&'static str, &'static str, Value, Option<Box4>);

/// The boxes of the markers, as offsets from the centre of the shape (`stencil/bpmn/markers.rs` and `bpmn.rs` give each its place).
mod at {
    use super::{Box4, square};
    /// A task's type marker: 0.2 inch, 0.07 inch in from the top left corner of the 1.4 by 0.9 inch box.
    pub fn task_type() -> Box4 {
        square(-0.53, 0.28, 0.2)
    }
    /// The one marker in the row along the bottom of an activity (loop, compensation, ad hoc, collapsed): 0.16 inch, centred.
    pub fn bottom_one() -> Box4 {
        square(0.0, -0.31, 0.16)
    }
    /// The row of two or more: the markers stand 0.2 apart, centred together.
    pub fn bottom_row() -> Box4 {
        (-0.4, -0.39, 0.4, -0.23)
    }
    /// An event's marker: 0.22 inch at the middle.
    pub fn event() -> Box4 {
        square(0.0, 0.0, 0.22)
    }
    /// A gateway's marker: inside the diamond, 0.42 inch (the double ring is the widest).
    pub fn gateway() -> Box4 {
        square(0.0, 0.0, 0.42)
    }
    /// A data object's arrow (top left) and its collection bars (bottom middle).
    pub fn data_arrow() -> Box4 {
        square(-0.13, 0.205, 0.16)
    }
    pub fn data_bars() -> Box4 {
        square(0.0, -0.215, 0.14)
    }
}

fn tries() -> Vec<Try> {
    let mut all: Vec<Try> = Vec::new();
    for value in ["Service", "Send", "Receive", "Instantiating Receive", "User", "Manual", "Business Rule", "Script"] {
        all.push(("bpmn.task", "BpmnTaskType", json!(value), Some(at::task_type())));
    }
    // None and Abstract show no type marker: what separates them in the reference application is unknown, and the project's rule is that both are bare.
    all.push(("bpmn.task", "BpmnTaskType", json!("Abstract"), None));
    for value in ["Standard", "Parallel MultiInstance", "Sequential MultiInstance"] {
        all.push(("bpmn.task", "BpmnLoopType", json!(value), Some(at::bottom_one())));
    }
    all.push(("bpmn.task", "BpmnIsForCompensation", json!(true), Some(at::bottom_one())));
    // A sub-process shows its collapsed marker at the bottom: another marker makes a row of two, centred together.
    for master in ["bpmn.sub-process", "bpmn.call-activity"] {
        for value in ["Standard", "Parallel MultiInstance", "Sequential MultiInstance"] {
            all.push((master, "BpmnLoopType", json!(value), Some(at::bottom_row())));
        }
        all.push((master, "BpmnIsForCompensation", json!(true), Some(at::bottom_row())));
        all.push((master, "BpmnAdHoc", json!(true), Some(at::bottom_row())));
    }
    for value in ["Message", "Timer", "Conditional", "Signal", "Multiple", "Parallel Multiple", "Escalation", "Error", "Compensation"] {
        all.push(("bpmn.start-event", "BpmnTriggerOrResult", json!(value), Some(at::event())));
    }
    for value in
        ["Message", "Timer", "Escalation", "Conditional", "Link", "Error", "Cancel", "Compensation", "Signal", "Multiple", "Parallel Multiple"]
    {
        all.push(("bpmn.intermediate-event", "BpmnTriggerOrResult", json!(value), Some(at::event())));
    }
    for value in ["Message", "Escalation", "Error", "Cancel", "Compensation", "Signal", "Terminate", "Multiple"] {
        all.push(("bpmn.end-event", "BpmnTriggerOrResult", json!(value), Some(at::event())));
    }
    for value in ["Exclusive Event (Instantiate)", "Inclusive", "Parallel", "Parallel Event (Instantiate)", "Complex"] {
        all.push(("bpmn.gateway", "BpmnGatewayType", json!(value), Some(at::gateway())));
    }
    all.push(("bpmn.gateway", "BpmnExclusiveType", json!("Event"), Some(at::gateway())));
    all.push(("bpmn.gateway", "BpmnMarkerVisible", json!(true), Some(at::gateway())));
    all.push(("bpmn.data-object", "BpmnCollection", json!(true), Some(at::data_bars())));
    for value in ["Input", "Output"] {
        all.push(("bpmn.data-object", "BpmnDataRole", json!(value), Some(at::data_arrow())));
    }
    all
}

#[test]
fn every_value_of_every_variant_field_shows_exactly_its_marker() {
    let tried = tries();
    assert!(tried.len() >= 60, "the table is the variants of the masters: {}", tried.len());
    let mut seen = std::collections::BTreeSet::new();
    for (id, row, value, shows) in &tried {
        seen.insert((*id, *row));
        let (mut s, shape) = one(id);
        let none = picture(&s);
        set_data(&mut s, shape, row, value.clone());
        let now = picture(&s);
        let key = format!("{id} {row} = {value}");
        match (difference(&none, &now), shows) {
            (None, None) => {}
            (Some(box_), Some(marker)) => assert!(within(box_, *marker, 0.02), "{key}: it changes {box_:?}, outside its marker {marker:?}"),
            (None, Some(marker)) => panic!("{key}: it shows nothing, and its marker is in {marker:?}"),
            (Some(box_), None) => panic!("{key}: it shows no marker and it changes {box_:?}"),
        }
    }
    // Every row of the rows that carry a marker was tried.
    for (id, row) in
        [("bpmn.task", "BpmnTaskType"), ("bpmn.task", "BpmnLoopType"), ("bpmn.gateway", "BpmnGatewayType"), ("bpmn.data-object", "BpmnDataRole")]
    {
        assert!(seen.contains(&(id, row)), "{id} {row} was tried");
    }
}

#[test]
fn a_variant_text_that_matches_no_item_shows_the_none_marker() {
    // The Shape Data window refuses a text that is no item of a fixed list, so the text is written to the cell as a file holds it.
    let long = "x".repeat(32_768);
    for (id, row, bad) in [
        ("bpmn.task", "BpmnTaskType", "Courier"),
        ("bpmn.task", "BpmnLoopType", "Forever"),
        ("bpmn.start-event", "BpmnTriggerOrResult", "Carrier pigeon"),
        ("bpmn.intermediate-event", "BpmnTriggerOrResult", "message"),
        ("bpmn.end-event", "BpmnTriggerOrResult", ""),
        ("bpmn.gateway", "BpmnGatewayType", "Quantum"),
        ("bpmn.data-object", "BpmnDataRole", "Sideways"),
        // A value as long as a field may be (32,768 characters) is a word that matches no item, and costs no more than any other.
        ("bpmn.task", "BpmnTaskType", long.as_str()),
        ("bpmn.gateway", "BpmnExclusiveType", long.as_str()),
    ] {
        let (mut s, shape) = one(id);
        let none = picture(&s);
        run(&mut s, "sheet.set_cell", json!({ "id": shape, "cell": format!("Prop.{row}"), "formula": format!("\"{bad}\"") }));
        let now = picture(&s);
        assert_eq!(difference(&none, &now), None, "{id} {row} = `{bad}`: the figure is the one with no marker");
    }
}

#[test]
fn a_throwing_marker_is_dark_and_a_catching_one_hollow() {
    // The signal triangle's middle, the envelope's lower body and the disc's middle: ink where the marker is filled, paper where it is outlined.
    let inside_triangle = (0.0, -0.03);
    let inside_envelope = (0.0, -0.06);
    for (id, set, probe, dark) in [
        ("bpmn.intermediate-event", vec![("BpmnTriggerOrResult", json!("Signal"))], inside_triangle, false),
        (
            "bpmn.intermediate-event",
            vec![("BpmnEventType", json!("Intermediate (Throwing)")), ("BpmnTriggerOrResult", json!("Signal"))],
            inside_triangle,
            true,
        ),
        ("bpmn.start-event", vec![("BpmnTriggerOrResult", json!("Message"))], inside_envelope, false),
        ("bpmn.end-event", vec![("BpmnTriggerOrResult", json!("Message"))], inside_envelope, true),
        ("bpmn.end-event", vec![("BpmnTriggerOrResult", json!("Terminate"))], (0.0, 0.0), true),
        ("bpmn.end-event", vec![("BpmnTriggerOrResult", json!("Signal"))], inside_triangle, true),
        // A task's send envelope is filled and its receive envelope is not: the envelope of 0.2 inch in the top left corner, its lower body.
        ("bpmn.task", vec![("BpmnTaskType", json!("Send"))], (-0.53, 0.224), true),
        ("bpmn.task", vec![("BpmnTaskType", json!("Receive"))], (-0.53, 0.224), false),
    ] {
        let (mut s, shape) = one(id);
        for (row, value) in &set {
            set_data(&mut s, shape, row, value.clone());
        }
        let p = pixel(&picture(&s), probe);
        let key = format!("{id} {set:?} at {probe:?}");
        if dark {
            assert!(is_dark(p), "{key}: a throwing marker is dark, {p:?}");
        } else {
            assert!(is_light(p), "{key}: a catching marker is hollow, {p:?}");
        }
    }
}

#[test]
fn the_dark_of_a_throwing_marker_follows_its_line_colour_under_every_theme() {
    // The marker's fill is its own line colour (`LineColor`), as dark as the outline under any theme (the design's 4.7).
    let (mut s, shape) = one("bpmn.end-event");
    set_data(&mut s, shape, "BpmnTriggerOrResult", json!("Terminate"));
    let themes: Vec<String> = run(&mut s, "theme.list", json!({}))["themes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["id"].as_str().map(str::to_owned))
        .take(3)
        .collect();
    assert_eq!(themes.len(), 3);
    for theme in &themes {
        run(&mut s, "theme.apply", json!({ "theme": theme }));
        let disc = pixel(&picture(&s), (0.0, 0.0));
        let ring = pixel(&picture(&s), (0.0, 0.2));
        assert!(disc[3] > 200, "{theme}: the disc is there, {disc:?}");
        let sum = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
        assert!(sum(disc) <= sum(ring) + 60, "{theme}: the disc is as dark as the outline, {disc:?} against {ring:?}");
    }
}

// ---- dashes, dots and thick borders -----------------------------------------------------------------------------------------------------

/// How many of 120 points round the circle of `radius` inches are ink.
fn ink_round(r: &Rendered, radius: f64) -> usize {
    (0..120)
        .filter(|k| {
            let a = f64::from(*k) * std::f64::consts::TAU / 120.0;
            (-1..=1).any(|d| {
                let rr = radius + f64::from(d) / PPI;
                let p = pixel(r, (rr * a.cos(), rr * a.sin()));
                p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 450
            })
        })
        .count()
}

#[test]
fn a_non_interrupting_event_is_dashed() {
    for (id, solid, dashed) in
        [("bpmn.start-event", "Start", "Start (Non-Interrupting)"), ("bpmn.intermediate-event", "Intermediate", "Intermediate (Non-Interrupting)")]
    {
        let (mut s, shape) = one(id);
        let (page, a) = (first_page(&s), ink_round(&picture(&s), 0.2));
        assert!(a >= 110, "{id}: a solid ring is ink all the way round, {a} of 120");
        set_data(&mut s, shape, "BpmnEventType", json!(dashed));
        let b = ink_round(&picture(&s), 0.2);
        assert!(b + 20 <= a, "{id}: a dashed ring has gaps, {b} of 120 against {a}");
        // The pattern is a cell of the ring (dash is 2), and the solid one is 1.
        let ring = member_index(id, "Ring");
        assert_eq!(member_number(s.document().unwrap(), page, shape, ring, "LinePattern"), Some(2.0), "{id}: LinePattern 2");
        set_data(&mut s, shape, "BpmnEventType", json!(solid));
        assert_eq!(member_number(s.document().unwrap(), page, shape, ring, "LinePattern"), Some(1.0), "{id}: LinePattern 1 again");
    }
}

/// The place, among the members of the instance, of the member the master calls `name`.
fn member_index(id: &str, name: &str) -> usize {
    let spec = master(id);
    let mut doc = Document::new_blank();
    let at = crate::stencil::ensure_master(&mut doc, spec).unwrap();
    let top = doc.master(at).unwrap().shapes.get(0).unwrap().clone();
    top.children.iter().position(|m| m.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("{id} has a member `{name}`"))
}

fn member_number(doc: &Document, page: u32, shape: u32, member: usize, cell: &str) -> Option<f64> {
    let at = doc.find_shape(Owner::Page(page), shape)?.child(u32::try_from(member).ok()?);
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// How many separate runs of ink the horizontal line at height `y` (an offset from the centre) crosses from `x0` to `x1`: a solid border is one, a dotted one many.
fn runs_along(r: &Rendered, y: f64, (x0, x1): (f64, f64)) -> usize {
    let (mut runs, mut was) = (0, false);
    let mut x = x0;
    while x < x1 {
        let now = (-2..=2).any(|d| is_dark(pixel(r, (x, y + f64::from(d) / PPI))));
        runs += usize::from(now && !was);
        was = now;
        x += 1.0 / PPI;
    }
    runs
}

/// How many rows of ink the vertical line at `x` crosses within 0.06 inch of the edge at `y`: how thick the line is.
fn thickness_at(r: &Rendered, (x, y): (f64, f64)) -> usize {
    (-12..=12).filter(|d| is_dark(pixel(r, (x, y + f64::from(*d) / PPI)))).count()
}

#[test]
fn an_event_sub_process_has_a_dotted_border_and_a_call_has_a_thick_one_and_a_transaction_a_second() {
    // The top edge of the 1.4 by 0.9 inch box is at 0.45 inch; the corners are clear of the stretch looked at.
    let (mut s, shape) = one("bpmn.sub-process");
    let plain = picture(&s);
    assert_eq!(runs_along(&plain, 0.45, (-0.5, 0.5)), 1, "a thin solid border is one run");
    let thin = thickness_at(&plain, (0.0, 0.45));
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Event"));
    let event = picture(&s);
    assert!(runs_along(&event, 0.45, (-0.5, 0.5)) >= 10, "an event sub-process border is dotted: {} runs", runs_along(&event, 0.45, (-0.5, 0.5)));
    // Dotted is pattern 3 (DD7), not dashed (2): the body that draws it holds 3.
    let dotted = member_index("bpmn.sub-process", "Body (dotted)");
    assert_eq!(member_number(s.document().unwrap(), first_page(&s), shape, dotted, "LinePattern"), Some(3.0));
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Call"));
    let thick = thickness_at(&picture(&s), (0.0, 0.45));
    assert!(thick >= thin * 2, "a call border is thick: {thick} rows against {thin}");
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Transaction"));
    let double = picture(&s);
    let second = (0..8).any(|k| is_dark(pixel(&double, (0.0, 0.40 + f64::from(k) * 0.004))));
    assert!(second && thickness_at(&double, (0.0, 0.45)) > thin, "a transaction has a second border just inside the first");
}

#[test]
fn the_expanded_sub_process_has_the_same_borders_in_its_own_size() {
    // A container of 3 by 2 inches: its top edge is at 1.0 inch. It draws its look from a formula of the shape's own cells.
    let (mut s, shape) = one("bpmn.expanded-sub-process");
    let page = first_page(&s);
    let plain = picture(&s);
    assert_eq!(runs_along(&plain, 1.0, (-1.2, 1.2)), 1, "a thin solid border is one run");
    let thin = thickness_at(&plain, (0.0, 1.0));
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Event"));
    assert_eq!(number(s.document().unwrap(), page, shape, "LinePattern"), Some(3.0), "dotted is pattern 3");
    assert!(runs_along(&picture(&s), 1.0, (-1.2, 1.2)) >= 10, "the border is dotted");
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Call"));
    assert!(thickness_at(&picture(&s), (0.0, 1.0)) >= thin * 2, "a call border is thick");
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Transaction"));
    let double = picture(&s);
    assert!((0..8).any(|k| is_dark(pixel(&double, (0.0, 0.96 + f64::from(k) * 0.004)))), "a transaction has a second border just inside the first");
    // Its markers stand in slots of their own along the bottom: loop, ad hoc and compensation.
    set_data(&mut s, shape, "BpmnBoundaryType", json!("Default"));
    let none = picture(&s);
    for (row, value, slot) in
        [("BpmnLoopType", json!("Standard"), -0.2), ("BpmnAdHoc", json!(true), 0.0), ("BpmnIsForCompensation", json!(true), 0.2)]
    {
        set_data(&mut s, shape, row, value.clone());
        let shown = difference(&none, &picture(&s)).unwrap_or_else(|| panic!("{row} shows a marker"));
        assert!(within(shown, square(slot, -0.86, 0.16), 0.02), "{row}: its marker is in its slot, {shown:?}");
        set_data(&mut s, shape, row, if value.is_boolean() { json!(false) } else { json!("None") });
    }
}

// ---- the markers at the bottom are centred together --------------------------------------------------------------------------------

/// The box of the ink inside `window` (offsets from the centre) in a picture whose base is `base`: where the two differ.
fn changed_in(base: &Rendered, now: &Rendered) -> Box4 {
    difference(base, now).expect("something changed")
}

#[test]
fn the_markers_along_the_bottom_of_a_task_are_centred_together() {
    let (mut s, shape) = one("bpmn.task");
    let none = picture(&s);
    set_data(&mut s, shape, "BpmnLoopType", json!("Standard"));
    let one_marker = changed_in(&none, &picture(&s));
    assert!((one_marker.0 + one_marker.2).abs() < 0.03, "a loop alone is centred: {one_marker:?}");
    set_data(&mut s, shape, "BpmnIsForCompensation", json!(true));
    let both = changed_in(&none, &picture(&s));
    assert!((both.0 + both.2).abs() < 0.03, "the loop and the compensation marker are centred as a pair: {both:?}");
    assert!(both.2 - both.0 > one_marker.2 - one_marker.0 + 0.1, "a pair is wider than one marker: {both:?} against {one_marker:?}");
    set_data(&mut s, shape, "BpmnLoopType", json!("None"));
    let comp_only = changed_in(&none, &picture(&s));
    assert!((comp_only.0 + comp_only.2).abs() < 0.03, "a compensation marker alone is centred: {comp_only:?}");
    // Sequential multi-instance turns the three bars a quarter: the two pictures differ, both in the marker's place.
    set_data(&mut s, shape, "BpmnIsForCompensation", json!(false));
    set_data(&mut s, shape, "BpmnLoopType", json!("Parallel MultiInstance"));
    let parallel = picture(&s);
    set_data(&mut s, shape, "BpmnLoopType", json!("Sequential MultiInstance"));
    let sequential = picture(&s);
    assert!(difference(&parallel, &sequential).is_some_and(|b| within(b, at::bottom_one(), 0.02)), "the bars turn in place");
}

#[test]
fn a_sub_process_shows_its_plus_marker_and_the_ad_hoc_tilde_beside_it() {
    let (mut s, shape) = one("bpmn.sub-process");
    let plain = picture(&s);
    // The [+] marker is in the picture of a sub-process that has no other marker: its box is the one at the bottom middle.
    let (_, bare) = one("bpmn.call-activity");
    let _ = bare;
    assert!((0..16).any(|k| is_dark(pixel(&plain, (-0.08 + f64::from(k) * 0.01, -0.31)))), "the collapsed marker is at the bottom middle");
    set_data(&mut s, shape, "BpmnAdHoc", json!(true));
    let with_tilde = picture(&s);
    let moved = difference(&plain, &with_tilde).unwrap();
    assert!(within(moved, at::bottom_row(), 0.02) && moved.2 - moved.0 > 0.2, "the tilde stands beside the plus, and the pair is centred: {moved:?}");
    assert!((moved.0 + moved.2).abs() < 0.03, "centred: {moved:?}");
}

// ---- the label is the last member and the group's own text is empty ---------------------------------------------------------------------

#[test]
fn the_text_of_an_event_or_a_task_is_its_last_member_so_typing_on_the_group_reaches_it() {
    for id in [
        "bpmn.task",
        "bpmn.start-event",
        "bpmn.intermediate-event",
        "bpmn.end-event",
        "bpmn.gateway",
        "bpmn.data-object",
        "bpmn.sub-process",
        "bpmn.call-activity",
    ] {
        let (mut s, shape) = one(id);
        let page = first_page(&s);
        assert_eq!(number(s.document().unwrap(), page, shape, "IsTextEditTarget"), Some(0.0), "{id}: typing on the group goes to its members");
        assert_eq!(number(s.document().unwrap(), page, shape, "SelectMode"), Some(1.0), "{id}: the group is picked first");
        let doc = s.document().unwrap();
        let group = doc.shape(&doc.find_shape(Owner::Page(page), shape).unwrap()).unwrap().clone();
        let last = group.children.get(group.children.len() - 1).unwrap().id;
        run(&mut s, "shape.set_text", json!({ "page": page, "id": last, "text": "Check the order" }));
        assert_eq!(draw::text_view(s.document().unwrap(), page, last).unwrap().chars, "Check the order", "{id}");
        assert_eq!(
            draw::text_view(s.document().unwrap(), page, shape).unwrap().chars,
            "",
            "{id}: the group itself holds no text (an .odg save refuses one)"
        );
    }
}

// ---- the flows: ends, patterns and the guard ------------------------------------------------------------------------------------

fn flow(s: &mut Session, id: &str) -> u32 {
    drop_at(s, id, CENTRE)
}

fn ends(s: &Session, id: u32) -> (Option<f64>, Option<f64>, Option<f64>) {
    let doc = s.document().unwrap();
    let page = first_page(s);
    (number(doc, page, id, "BeginArrow"), number(doc, page, id, "EndArrow"), number(doc, page, id, "LinePattern"))
}

#[test]
fn a_default_sequence_flow_begins_with_23_and_a_conditional_one_with_22() {
    let mut s = session();
    let id = flow(&mut s, "bpmn.sequence-flow");
    assert_eq!(ends(&s, id), (Some(0.0), Some(4.0), Some(1.0)), "a sequence flow: no begin mark, a solid head, solid");
    set_data(&mut s, id, "BpmnConditionType", json!("Expression"));
    assert_eq!(ends(&s, id), (Some(22.0), Some(4.0), Some(1.0)), "conditional: a hollow diamond at the start");
    set_data(&mut s, id, "BpmnConditionType", json!("Default"));
    assert_eq!(ends(&s, id), (Some(23.0), Some(4.0), Some(1.0)), "default: a slash across the start");
    set_data(&mut s, id, "BpmnConditionType", json!("None"));
    assert_eq!(ends(&s, id), (Some(0.0), Some(4.0), Some(1.0)));
    // The master says the same numbers, and an instance holds none of them.
    let look = master("bpmn.sequence-flow");
    assert_eq!(look.look().map(|l| (l.numbers(), l.guard_pattern)), Some(((0, 4, 1), true)));
}

#[test]
fn the_message_flow_and_the_associations_have_their_ends_and_their_guarded_patterns() {
    let mut s = session();
    let message = flow(&mut s, "bpmn.message-flow");
    assert_eq!(ends(&s, message), (Some(20.0), Some(16.0), Some(2.0)), "a message flow: a hollow disc, a hollow triangle, dashed");
    let association = flow(&mut s, "bpmn.association");
    assert_eq!(ends(&s, association), (Some(0.0), Some(0.0), Some(3.0)), "an association: no marks, dotted");
    for (direction, begin, end) in [("One", 0.0, 3.0), ("Both", 3.0, 3.0), ("None", 0.0, 0.0)] {
        set_data(&mut s, association, "BpmnDirection", json!(direction));
        assert_eq!(ends(&s, association), (Some(begin), Some(end), Some(3.0)), "association direction {direction}");
    }
    let data = flow(&mut s, "bpmn.data-association");
    assert_eq!(ends(&s, data), (Some(0.0), Some(3.0), Some(3.0)), "a data association: an open head at the target, dotted");
    for id in ["bpmn.sequence-flow", "bpmn.message-flow", "bpmn.association", "bpmn.data-association"] {
        let look = master(id).look().unwrap();
        assert!(look.guard_pattern, "{id}: its pattern is guarded");
    }
}

#[test]
fn a_dash_on_a_sequence_flow_is_refused_in_words() {
    for id in ["bpmn.sequence-flow", "bpmn.message-flow", "bpmn.association"] {
        let mut s = session();
        let shape = flow(&mut s, id);
        let before = ends(&s, shape);
        let page = first_page(&s);
        let wrong = if before.2 == Some(2.0) { 1 } else { 2 };
        let refused = s.execute("shape.format", &json!({ "ids": [shape], "page": page, "pattern": wrong }));
        let words = refused.expect_err(&format!("{id}: a pattern that is not the notation's is refused")).to_string();
        assert!(words.to_lowercase().contains("guard"), "{id}: the refusal says why: {words}");
        assert_eq!(ends(&s, shape), before, "{id}: nothing changed");
    }
}

// ---- the Pool and the Lane are parts of the swimlane structure ---------------------------------------------------------------------------

fn info(s: &mut Session) -> Value {
    run(s, "swimlane.info", json!({}))
}

/// **Meets the Pool**. The Pool is made by code, so the range of eight of the page's scale is taken where it is dropped: on a page whose drawing scale is beyond it (100
/// to 1) the pool keeps its size on paper (8 by 2 inches) and is no longer refused for a width of 0.08 inch; within the range (1:2 to 1:8) it is the master's size in drawing units, 8 by 2 of
/// them (a quarter inch tall on paper at 1:8, which was refused there for holding the least of a lane, 0.4 inch, to drawing units of the page).
#[test]
fn a_pool_dropped_on_a_page_of_another_scale_keeps_its_paper_size_beyond_the_range_of_eight_and_its_drawing_size_within_it() {
    for (k, (width, height)) in [(1.0, (8.0, 2.0)), (2.0, (4.0, 1.0)), (4.0, (2.0, 0.5)), (8.0, (1.0, 0.25)), (16.0, (8.0, 2.0)), (100.0, (8.0, 2.0))]
    {
        let mut s = session();
        if k != 1.0 {
            run(&mut s, "page.scale", json!({ "page": 1, "paper": 1.0, "drawing": k }));
        }
        let dropped = run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
        let reported = (dropped["width"].as_f64().unwrap_or(f64::NAN), dropped["height"].as_f64().unwrap_or(f64::NAN));
        assert!(
            (reported.0 - width).abs() < 1e-9 && (reported.1 - height).abs() < 1e-9,
            "at {k} to 1 the pool is {width} by {height} inches: {dropped}"
        );
        let found = info(&mut s);
        let rect: Vec<f64> = found["structures"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(
            (rect[2] - rect[0] - width).abs() < 1e-6 && (rect[3] - rect[1] - height).abs() < 1e-6,
            "at {k} to 1 the structure is {width} by {height} paper inches: {rect:?}"
        );
    }
}

#[test]
fn a_pool_dropped_on_a_page_is_a_structure_with_one_lane_and_its_name_strip_on_the_left() {
    let mut s = session();
    let dropped = run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
    let frame = id_of(&dropped);
    assert_eq!((dropped["name"].as_str(), dropped["width"].as_f64(), dropped["height"].as_f64()), (Some("Pool"), Some(8.0), Some(2.0)), "{dropped}");
    let found = info(&mut s);
    let structures = found["structures"].as_array().unwrap();
    assert_eq!(structures.len(), 1, "one structure: {found}");
    let st = &structures[0];
    assert_eq!(st["frame"].as_u64(), Some(u64::from(frame)));
    assert_eq!(st["lanes"].as_array().unwrap().len(), 1, "one lane");
    assert!(st["phase_list"].is_null() && st["phases"].as_array().unwrap().is_empty(), "no phases");
    let rect: Vec<f64> = st["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(
        (rect[0] - 1.5).abs() < 1e-6 && (rect[2] - 9.5).abs() < 1e-6 && (rect[1] - 3.25).abs() < 1e-6 && (rect[3] - 5.25).abs() < 1e-6,
        "the pool is 8 by 2 inches about the drop point: {rect:?}"
    );
    // The name strip is the frame's heading on its left, one strip wide: the lane starts after it and keeps the lane's own strip.
    let doc = s.document().unwrap();
    assert_eq!(user_value(doc, 1, frame, "DiaCraftContainerHeading"), Some(3.0), "the heading is on the left");
    let lane = st["lanes"][0].clone();
    let lane_rect: Vec<f64> = lane["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(
        (lane_rect[0] - (rect[0] + 0.3)).abs() < 1e-6 && (lane_rect[2] - rect[2]).abs() < 1e-6,
        "the lane lies right of the strip: {lane_rect:?}"
    );
    assert_eq!(draw::text_view(doc, 1, frame).unwrap().chars, "Pool", "the pool's name is the frame's text");
    assert_eq!(lane["text"].as_str(), Some("Lane"), "the lane's label");
    // The pool's label is separated from its contents by a line, the lane's is not (DD11).
    let strips = |id: u32| {
        doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry").count()
    };
    assert_eq!(strips(frame), 2, "the pool: its box and the line beside its label");
    assert_eq!(strips(u32::try_from(lane["id"].as_u64().unwrap()).unwrap()), 1, "the lane: its box only, no line beside its label");
    // The structure keeps its strip when a lane is added and the pool is laid out again.
    run(&mut s, "swimlane.insert", json!({ "lane": lane["id"], "where": "after", "label": "Lane 2" }));
    let again = info(&mut s);
    let st = &again["structures"][0];
    assert_eq!(st["lanes"].as_array().unwrap().len(), 2);
    let rect: Vec<f64> = st["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let first: Vec<f64> = st["lanes"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((first[0] - (rect[0] + 0.3)).abs() < 1e-6, "the pool is still one strip wider than its lanes: pool {rect:?}, lane {first:?}");
    assert!((rect[3] - rect[1] - 4.0).abs() < 1e-6, "and as tall as its two lanes: {rect:?}");
}

#[test]
fn a_lane_dropped_on_a_pool_makes_two_lanes() {
    let mut s = session();
    run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
    let lane = run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 3.5 }));
    assert!(lane["master"].is_null(), "a lane put into a pool is a part of it and no shape of the lane master: {lane}");
    let found = info(&mut s);
    let st = &found["structures"][0];
    let lanes = st["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 2, "{found}");
    assert_eq!(found["structures"].as_array().unwrap().len(), 1, "no second structure");
    // Dropped in the lower half of the lane, it goes after it.
    assert_eq!(lanes[1]["id"].as_u64(), lane["id"].as_u64(), "the new lane is the second");
    for l in lanes {
        let r: Vec<f64> = l["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(((r[3] - r[1]) - 2.0).abs() < 1e-6, "each lane is as thick as the one it was put beside: {r:?}");
        assert_eq!(l["heading"].as_str(), Some("left"));
    }
    // Dropped in the upper half of the first lane (the pool's top, at 5.25 inches, is above the point), a third goes before it.
    let third = run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 5.0 }));
    let lanes = info(&mut s)["structures"][0]["lanes"].clone();
    assert_eq!(lanes.as_array().unwrap().len(), 3, "{lanes}");
    assert_eq!(lanes[0]["id"].as_u64(), third["id"].as_u64(), "the new lane is the first, above the one it was dropped on");
}

#[test]
fn a_lane_dropped_on_a_page_with_no_pool_is_a_shape() {
    let mut s = session();
    let dropped = run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
    assert!(dropped["master"].is_number(), "it is a shape of the lane master: {dropped}");
    assert_eq!((dropped["width"].as_f64(), dropped["height"].as_f64()), (Some(7.6), Some(1.5)));
    assert!(info(&mut s)["structures"].as_array().unwrap().is_empty(), "no structure was made");
    // Over a cross-functional structure that is no pool, the lane is a shape too: it joins a pool only.
    let mut s = session();
    run(&mut s, "swimlane.create", json!({ "lanes": 2 }));
    let before = info(&mut s)["structures"][0]["lanes"].as_array().unwrap().len();
    let over = run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 4.0, "y": 7.5 }));
    assert!(over["master"].is_number(), "{over}");
    assert_eq!(info(&mut s)["structures"][0]["lanes"].as_array().unwrap().len(), before, "the cross-functional lanes are as they were");
}

#[test]
fn a_pool_survives_a_save_and_a_reopen_in_vsdx_and_diacraft_and_an_odg_takes_it() {
    for ext in ["vsdx", "diacraft", "odg"] {
        let mut s = session();
        run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
        run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 3.5 }));
        let path = scratch(&format!("pool.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": path }));
        let found = run(&mut again, "swimlane.info", json!({}));
        let st = &found["structures"][0];
        assert_eq!(st["lanes"].as_array().unwrap().len(), 2, "{ext}: two lanes after the reopen: {found}");
        let rect: Vec<f64> = st["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let first: Vec<f64> = st["lanes"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!((first[0] - (rect[0] + 0.3)).abs() < 1e-3, "{ext}: the pool is still one strip wider than its lanes: pool {rect:?}, lane {first:?}");
        // The folder is this test's own, in the temporary folder the run names: nothing is left behind.
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
    }
}

/// A path for a file the test writes, in a folder of this process's own in the temporary folder (`TMPDIR` when the run names one).
fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("bpmn-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().into_owned()
}

#[test]
fn turning_a_pool_puts_its_name_strip_on_top_and_turning_it_back_on_the_left() {
    let mut s = session();
    run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
    run(&mut s, "swimlane.orientation", json!({ "orientation": "vertical" }));
    let found = info(&mut s);
    let st = &found["structures"][0];
    let rect: Vec<f64> = st["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let lane: Vec<f64> = st["lanes"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((rect[3] - lane[3] - 0.3).abs() < 1e-6, "the strip is now above the lanes: pool {rect:?}, lane {lane:?}");
    run(&mut s, "swimlane.orientation", json!({ "orientation": "horizontal" }));
    let back = info(&mut s);
    let rect: Vec<f64> = back["structures"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let lane: Vec<f64> = back["structures"][0]["lanes"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((lane[0] - rect[0] - 0.3).abs() < 1e-6, "and on the left again: pool {rect:?}, lane {lane:?}");
}

// ---- containers -------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_expanded_sub_process_and_the_group_are_container_masters_that_take_a_dropped_shape() {
    for id in ["bpmn.expanded-sub-process", "bpmn.group"] {
        let mut s = session();
        let container = drop_at(&mut s, id, CENTRE);
        let task = drop_at(&mut s, "bpmn.task", (CENTRE.0, CENTRE.1 - 0.2));
        let page = first_page(&s);
        let held = run(&mut s, "container.info", json!({ "ids": [container], "page": page }));
        assert!(held.to_string().contains(&task.to_string()), "{id}: the task dropped inside joined it: {held}");
        run(&mut s, "shape.move", json!({ "ids": [container], "dx": 1.0, "dy": 0.0 }));
        let frame = draw::shape_frame(s.document().unwrap(), page, task).unwrap();
        assert!((frame.pin_x - (CENTRE.0 + 1.0)).abs() < 1e-6, "{id}: a move of the container takes its member: {}", frame.pin_x);
    }
}

// ---- the figures that are no groups -----------------------------------------------------------------------------------------------------

#[test]
fn the_data_store_the_message_and_the_text_annotation_draw_inside_their_boxes_and_the_annotation_is_an_open_bracket() {
    // The text annotation's bracket is the left edge and two short arms: nothing on its right side.
    let (s, _) = one("bpmn.text-annotation");
    let r = picture(&s);
    assert!(is_dark(pixel(&r, (-0.8, 0.0))), "the bracket's spine is at the left edge");
    assert!(
        !is_dark(pixel(&r, (0.8, 0.0))) && !is_dark(pixel(&r, (0.0, 0.3))),
        "the bracket is open on the right and its top and bottom between the arms"
    );
    let blank = picture(&session());
    for id in ["bpmn.data-store", "bpmn.message"] {
        let (s, _) = one(id);
        let m = master(id);
        let ink = difference(&blank, &picture(&s));
        assert!(
            ink.is_some_and(|b| within(b, (-m.width / 2.0, -m.height / 2.0, m.width / 2.0, m.height / 2.0), 0.05)),
            "{id}: its ink is in its box, {ink:?}"
        );
    }
}

// ---- which marker an event shows for which kind ------------------------------------------------------------------------

#[test]
fn an_event_shows_a_trigger_only_where_the_notation_allows_it() {
    // Table 10.93 of BPMN 2.0.2 as the spec reads it: for each master and kind of event, the triggers it may show. A trigger the table does not allow for the kind shows none.
    const CATCHING: [&str; 7] = ["Message", "Timer", "Escalation", "Conditional", "Signal", "Multiple", "Parallel Multiple"];
    let all_start: Vec<&str> = START_WORDS.strip_prefix("list:").unwrap().split(';').filter(|w| *w != "None").collect();
    let all_intermediate: Vec<&str> = INTERMEDIATE_WORDS.strip_prefix("list:").unwrap().split(';').filter(|w| *w != "None").collect();
    let all_end: Vec<&str> = END_WORDS.strip_prefix("list:").unwrap().split(';').filter(|w| *w != "None").collect();
    let cases: Vec<(&str, &str, Vec<&str>, Vec<&str>)> = vec![
        ("bpmn.start-event", "Start", all_start.clone(), all_start.clone()),
        ("bpmn.start-event", "Start (Non-Interrupting)", all_start, CATCHING.to_vec()),
        ("bpmn.intermediate-event", "Intermediate", all_intermediate.clone(), all_intermediate.clone()),
        ("bpmn.intermediate-event", "Intermediate (Non-Interrupting)", all_intermediate.clone(), CATCHING.to_vec()),
        (
            "bpmn.intermediate-event",
            "Intermediate (Throwing)",
            all_intermediate,
            vec!["Message", "Escalation", "Compensation", "Link", "Signal", "Multiple"],
        ),
        ("bpmn.end-event", "End", all_end.clone(), all_end),
    ];
    let mut shown = 0;
    for (id, kind, words, allowed) in cases {
        let (mut s, shape) = one(id);
        set_data(&mut s, shape, "BpmnEventType", json!(kind));
        let none = picture(&s);
        for word in words {
            set_data(&mut s, shape, "BpmnTriggerOrResult", json!(word));
            let key = format!("{id} {kind} {word}");
            match (difference(&none, &picture(&s)), allowed.contains(&word)) {
                (Some(b), true) => {
                    assert!(within(b, at::event(), 0.02), "{key}: its marker is in the middle, {b:?}");
                    shown += 1;
                }
                (None, false) => {}
                (Some(b), false) => panic!("{key}: the table does not allow it, and it shows {b:?}"),
                (None, true) => panic!("{key}: the table allows it, and it shows nothing"),
            }
        }
    }
    assert!(shown >= 40, "the table was walked: {shown} markers");
}

#[test]
fn a_start_event_with_no_trigger_is_a_thin_circle_and_an_end_event_a_thick_one() {
    let (s, _) = one("bpmn.start-event");
    let start = picture(&s);
    let (e, _) = one("bpmn.end-event");
    let end = picture(&e);
    assert!(is_dark(pixel(&start, (0.0, 0.2))) && is_light(pixel(&start, (0.0, 0.0))), "a thin ring with an open centre");
    let rows = |r: &Rendered| (-16..=16).filter(|d| is_dark(pixel(r, (0.0, 0.2 + f64::from(*d) / PPI)))).count();
    assert!(rows(&end) >= rows(&start) * 2, "the end event's ring is the thick one: {} rows against {}", rows(&end), rows(&start));
    // An intermediate event is two thin rings: ink at 0.2 and again just inside at 0.16, and paper between them.
    let (i, _) = one("bpmn.intermediate-event");
    let inter = picture(&i);
    assert!(is_dark(pixel(&inter, (0.0, 0.2))) && is_dark(pixel(&inter, (0.0, 0.16))), "two rings");
    assert!(is_light(pixel(&inter, (0.0, 0.18))), "with paper between them");
}

#[test]
fn the_gateway_is_a_diamond_and_its_marker_is_the_one_its_type_says() {
    let (mut s, shape) = one("bpmn.gateway");
    let diamond = picture(&s);
    assert!(is_dark(pixel(&diamond, (0.0, 0.3))) && is_dark(pixel(&diamond, (0.3, 0.0))), "the diamond's corners are on its box");
    assert!(is_light(pixel(&diamond, (0.0, 0.0))), "a gateway of the default type shows no marker");
    // Exclusive shows an X when it is asked to: ink along the diagonal and none on the axis through the middle, which a plus has.
    set_data(&mut s, shape, "BpmnMarkerVisible", json!(true));
    let x = picture(&s);
    assert!(is_dark(pixel(&x, (0.08, 0.08))) && is_light(pixel(&x, (0.12, 0.0))), "an X");
    set_data(&mut s, shape, "BpmnMarkerVisible", json!(false));
    set_data(&mut s, shape, "BpmnGatewayType", json!("Parallel"));
    let plus = picture(&s);
    assert!(is_dark(pixel(&plus, (0.1, 0.0))) && is_light(pixel(&plus, (0.08, 0.08))), "a plus");
    set_data(&mut s, shape, "BpmnGatewayType", json!("Inclusive"));
    let circle = picture(&s);
    assert!(is_dark(pixel(&circle, (0.0, 0.12))) && is_light(pixel(&circle, (0.0, 0.0))), "a circle");
    set_data(&mut s, shape, "BpmnGatewayType", json!("Complex"));
    let asterisk = picture(&s);
    assert!(is_dark(pixel(&asterisk, (0.1, 0.0))) && is_dark(pixel(&asterisk, (0.07, 0.07))), "an asterisk has the plus and the X");
    // The event-based forms: a ring inside the diamond (a second one for the exclusive form that waits for an event).
    set_data(&mut s, shape, "BpmnGatewayType", json!("Exclusive"));
    set_data(&mut s, shape, "BpmnExclusiveType", json!("Event"));
    let double = picture(&s);
    set_data(&mut s, shape, "BpmnExclusiveType", json!("Data"));
    set_data(&mut s, shape, "BpmnGatewayType", json!("Exclusive Event (Instantiate)"));
    let single = picture(&s);
    assert!(is_dark(pixel(&double, (0.0, 0.2))) && is_dark(pixel(&double, (0.0, 0.164))), "two rings");
    assert!(is_dark(pixel(&single, (0.0, 0.2))) && is_light(pixel(&single, (0.0, 0.164))), "one ring");
}

#[test]
fn the_group_is_a_dash_dot_container_with_no_fill_and_a_call_activity_begins_thick() {
    let (s, group) = one("bpmn.group");
    let doc = s.document().unwrap();
    assert_eq!(number(doc, 1, group, "LinePattern"), Some(4.0), "dash-dot");
    assert_eq!(number(doc, 1, group, "FillPattern"), Some(0.0), "no fill");
    assert_eq!(user_value(doc, 1, group, "DiaCraftContainerHeading"), Some(0.0), "the category name is at the top");
    let (call, _) = one("bpmn.call-activity");
    let (sub, _) = one("bpmn.sub-process");
    assert!(
        thickness_at(&picture(&call), (0.0, 0.45)) >= thickness_at(&picture(&sub), (0.0, 0.45)) * 2,
        "the call activity's default border is the thick one"
    );
}

#[test]
fn the_lane_list_and_the_lanes_of_a_pool_hold_objtype_4_and_the_pool_holds_1() {
    let mut s = session();
    let frame = id_of(&run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 })));
    let found = info(&mut s);
    let st = &found["structures"][0];
    let doc = s.document().unwrap();
    let lane = u32::try_from(st["lanes"][0]["id"].as_u64().unwrap()).unwrap();
    let list = u32::try_from(st["lane_list"].as_u64().unwrap()).unwrap();
    assert_eq!(number(doc, 1, frame, "ObjType"), Some(1.0), "the pool itself is the structure's outer container");
    assert_eq!(
        (number(doc, 1, list, "ObjType"), number(doc, 1, lane, "ObjType")),
        (Some(4.0), Some(4.0)),
        "the list of lanes and a lane: no route goes round them"
    );
}

// ---- a pool keeps its strip when it is resized or moved ------------------------------------------------------------------------------------

/// The box of a shape in a `swimlane.info` answer.
fn rect_of(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

#[test]
fn a_pool_resized_by_its_width_is_that_wide_and_its_lane_is_the_rest_after_the_strip() {
    let mut s = session();
    let frame = id_of(&run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 })));
    // A corner drag of the frame: 6 inches wide, from the same left edge, as tall as it was.
    run(&mut s, "shape.resize", json!({ "id": frame, "x": 1.5, "y": 3.25, "width": 6.0, "height": 2.0 }));
    let found = info(&mut s);
    let st = &found["structures"][0];
    let (pool, lane) = (rect_of(&st["rect"]), rect_of(&st["lanes"][0]["rect"]));
    assert!((pool[2] - pool[0] - 6.0).abs() < 1e-6, "the pool is as wide as it was made: {pool:?}");
    assert!(
        (lane[0] - pool[0] - 0.3).abs() < 1e-6 && (lane[2] - pool[2]).abs() < 1e-6,
        "the lane is the rest after the strip: pool {pool:?}, lane {lane:?}"
    );
}

#[test]
fn a_pool_moved_takes_its_lanes_and_keeps_its_strip() {
    let mut s = session();
    let frame = id_of(&run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 })));
    run(&mut s, "shape.drop", json!({ "master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 3.5 }));
    let before = info(&mut s);
    run(&mut s, "shape.move", json!({ "ids": [frame], "dx": 0.75, "dy": -0.5 }));
    let after = info(&mut s);
    let (a, b) = (&before["structures"][0], &after["structures"][0]);
    for (was, now) in [(&a["rect"], &b["rect"]), (&a["lanes"][0]["rect"], &b["lanes"][0]["rect"]), (&a["lanes"][1]["rect"], &b["lanes"][1]["rect"])] {
        let (was, now) = (rect_of(was), rect_of(now));
        for k in 0..4 {
            let step = if k % 2 == 0 { 0.75 } else { -0.5 };
            assert!((now[k] - was[k] - step).abs() < 1e-6, "everything moved together: {was:?} to {now:?}");
        }
    }
}

// ---- the template -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_template_is_listed_now_and_says_based_on_bpmn_2_0_2_and_claims_no_conformance_class() {
    let listed = run(&mut session(), "template.list", json!({}));
    let found = listed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "bpmn_diagram")
        .unwrap_or_else(|| panic!("the BPMN Diagram template is listed: {listed}"));
    let text = found["description"].as_str().unwrap();
    assert!(text.contains("based on BPMN 2.0.2"), "{text}");
    for word in ["compliant", "conformant", "certified", "Descriptive", "Analytic"] {
        assert!(!text.contains(word), "the description claims no conformance class: `{word}` in {text}");
    }
    // A drawing started from it opens the BPMN stencil beside the basic shapes.
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "bpmn_diagram" }));
    let open: Vec<String> =
        run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str().map(str::to_owned)).collect();
    assert!(open.iter().any(|id| id == "bpmn") && open.iter().any(|id| id == "basic"), "{open:?}");
}

#[test]
fn a_pool_on_a_page_drawn_at_two_to_one_is_the_masters_size_in_drawing_units() {
    // The master's size is in drawing units, as every master's is: on a page drawn at 2 drawing units to a paper inch the 8 by 2 pool is 4 by 1 inches on paper.
    let mut s = session();
    run(&mut s, "page.scale", json!({ "page": 1, "paper": 1.0, "drawing": 2.0 }));
    let dropped = run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25 }));
    assert_eq!((dropped["width"].as_f64(), dropped["height"].as_f64()), (Some(4.0), Some(1.0)), "{dropped}");
    let found = info(&mut s);
    let st = &found["structures"][0];
    let (pool, lane) = (rect_of(&st["rect"]), rect_of(&st["lanes"][0]["rect"]));
    assert!(
        (pool[0] - 3.5).abs() < 1e-6 && (pool[2] - 7.5).abs() < 1e-6 && (pool[1] - 3.75).abs() < 1e-6 && (pool[3] - 4.75).abs() < 1e-6,
        "centred on the drop point: {pool:?}"
    );
    assert!((lane[0] - pool[0] - 0.3).abs() < 1e-6, "the strip is 0.3 paper inch: pool {pool:?}, lane {lane:?}");
}
