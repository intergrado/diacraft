//! Three bugs that found around the drawing scale.
//!
//! 1. **A one-dimensional shape read from an `.odg` on a scaled page held its text block in the master's numbers** (`TxtWidth` 1.5 where the drawing units are 150 at 1:100). The save left the
//!    cells of a label out of a connector with no text, so the shape read again inherited the cached values of its master's own cells, which are the master's size.
//! 2. **BPMN's Pool was refused on a page drawn at 1:6, 1:7 or 1:8** ("`height` must be at least 0.4 inch"): within the range of eight the pool is the master's size in drawing units (2 tall, a
//!    quarter inch on paper at 1:8), and the least of 0.4 inch was held to it in the page's units (0.4 times the drawing units to a paper inch, 3.2 at 1:8).
//! 3. **A group instance read from an `.odg` named other shapes in the formulas it copied from its master.** The copy of a member holds the cells its master gives it as its own, and a formula
//!    that names the master's group (`Sheet.5!DropOnPageScale`, `Sheet.5!Prop.BpmnLoopType`, `Sheet.5!Prop.Abstract`) names a shape of the master's part, not of the page: read again it named the
//!    page's shape 5, so after a resize the factor read 1 and the markers followed another shape.

use std::collections::BTreeMap;

use diacraft_doc::{CellAddr, Document, Member, Owner, Resolved, Resolver, RowKey, ShapeRef, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::stencil::all_masters;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::figure_scale::{drop_on, first_page, lengths_of, near, page_at};
use crate::stencil_open::Dir;
use crate::{check_session, run};

/// A cell's value: a number, or the text of one that is no number.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    N(f64),
    T(String),
}

fn val_of(found: &Resolved<'_>) -> Option<Val> {
    let v = found.value.cell?.value.as_ref()?;
    Some(v.number().filter(|n| n.is_finite()).map_or_else(|| Val::T(v.text().to_owned()), Val::N))
}

fn differ(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::N(x), Val::N(y)) => !near(*x, *y),
        _ => a != b,
    }
}

/// The cells of a shape that a drawing shows, by a name that says where each is.
const DIRECT: [&str; 15] = [
    "Width",
    "Height",
    "PinX",
    "PinY",
    "LocPinX",
    "LocPinY",
    "Angle",
    "TxtWidth",
    "TxtHeight",
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "DropOnPageScale",
    "LinePattern",
];

/// Every value the shape `at` and its members resolve to: the frame and the text block, the flags and the rows of each `Geometry` section (a marker is a section whose `NoShow` says whether it is
/// drawn), the connection points and the shape data. Through the master: what the shape reads is what a drawing shows.
fn snapshot(d: &Document, at: &ShapeRef, path: &str, out: &mut BTreeMap<String, Val>) {
    let res = Resolver::new(d);
    let sheet = SheetRef::Shape(at.clone());
    for name in DIRECT {
        if let Some(v) = res.resolve(&sheet, &CellAddr::direct(name)).as_ref().and_then(val_of) {
            out.insert(format!("{path}{name}"), v);
        }
    }
    for section in ["Geometry", "Connection", "User", "Property"] {
        for key in res.effective_sections(&sheet, section).unwrap_or_default() {
            for flag in ["NoFill", "NoLine", "NoShow", "NoSnap"] {
                if let Some(v) = res.resolve(&sheet, &CellAddr::in_section(section, key.ix, flag)).as_ref().and_then(val_of) {
                    out.insert(format!("{path}{section}{:?}.{flag}", key.ix), v);
                }
            }
            for (r, row) in res.effective_rows(&sheet, &key).unwrap_or_default().iter().enumerate() {
                for (cell, found) in &row.cells {
                    if let Some(v) = val_of(found) {
                        out.insert(format!("{path}{section}{:?}#{r}.{}", key.ix, cell.as_str()), v);
                    }
                }
            }
        }
    }
    for (i, member) in res.effective_members(at).unwrap_or_default().into_iter().enumerate() {
        if let Member::Local(child) = member {
            snapshot(d, &child, &format!("{path}m{i}."), out);
        }
    }
}

fn snap_of(d: &Document, id: u32) -> BTreeMap<String, Val> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let mut out = BTreeMap::new();
    snapshot(d, &at, "", &mut out);
    out
}

/// What differs between two snapshots, as words: a name of the first that the second has no value for, or another value.
fn differences(a: &BTreeMap<String, Val>, b: &BTreeMap<String, Val>) -> Vec<String> {
    let mut bad = Vec::new();
    for (k, v) in a {
        match b.get(k) {
            None => bad.push(format!("{k}: {v:?} against none")),
            Some(w) if differ(v, w) => bad.push(format!("{k}: {v:?} against {w:?}")),
            _ => {}
        }
    }
    bad
}

/// The first shape of the first page of a session that read a file.
fn first_shape(s: &Session) -> u32 {
    doc(s).pages.get(0).unwrap().shapes.get(0).unwrap().id
}

/// A session that read the drawing `s` saved as `file`.
fn reopened(s: &mut Session, dir: &Dir, file: &str) -> Session {
    let path = dir.path(file);
    run(s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    t
}

/// The shape made wider by half of its width: a resize works its formulas out again, which is when a formula that names the wrong shape shows. `false` when the command refuses.
fn resize_wider(s: &mut Session, id: u32, k: f64) -> bool {
    let page = first_page(doc(s));
    let Some(f) = diacraft_engine::draw::shape_frame(doc(s), page, id) else { return false };
    let params = json!({"id": id, "page": page, "x": f.x / k, "y": f.y / k, "width": f.width * 1.5 / k, "height": f.height / k});
    s.execute("shape.resize", &params).is_ok()
}

// ---- 1. a one-dimensional shape's text block in an `.odg` -------------------------------------------------------------------------------------

/// Every master of the built-in stencils that is a line (a connector, or a figure along a line), dropped at the scale `k`: its lengths, by the master's id, all of them.
fn one_d_lengths(k: f64) -> BTreeMap<&'static str, (Session, u32)> {
    let mut out = BTreeMap::new();
    for spec in all_masters().filter(|spec| spec.is_connector()) {
        let mut s = page_at(k);
        let r = drop_on(&mut s, spec.id);
        if !r["master"].is_null() {
            let id = id_of(&r);
            out.insert(spec.id, (s, id));
        }
    }
    out
}

/// **The first bug.** A line dropped at 1:100 holds its text block in drawing units (`TxtWidth` 150 for a line 150 long); read from an `.odg` it held the master's numbers (1.5). Every line
/// master of every built-in stencil, dropped at 1:100, 1:20, 1:8 and 1:2, has the same text block after a save as `.odg` and a read, and the same lengths in every other cell.
#[test]
fn a_one_dimensional_shape_read_from_an_odg_holds_its_text_block_in_drawing_units() {
    let mut failed: Vec<String> = Vec::new();
    let mut counted = 0;
    for k in [100.0, 20.0, 8.0, 2.0, 0.05] {
        for (id, (mut s, shape)) in one_d_lengths(k) {
            counted += 1;
            let before = lengths_of(doc(&s), shape);
            assert!(before.contains_key("TxtWidth"), "{id} at 1:{k}: the dropped line holds a text block");
            let dir = Dir::new();
            let t = reopened(&mut s, &dir, "line.odg");
            let after = lengths_of(doc(&t), first_shape(&t));
            let wrong: Vec<String> = before
                .iter()
                .filter(|(name, n)| after.get(*name).is_none_or(|m| !near(**n, *m)))
                .map(|(name, n)| format!("{name}: {n} before, {:?} after", after.get(name)))
                .collect();
            if !wrong.is_empty() {
                failed.push(format!(
                    "{id} at 1:{k}: {} lengths differ, e.g. {}",
                    wrong.len(),
                    wrong.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
                ));
            }
        }
    }
    assert!(counted > 20, "the sweep covers the lines of the catalogue: {counted} drops");
    assert!(failed.is_empty(), "{} drops differ after a trip through an .odg:\n{}", failed.len(), failed.join("\n"));
}

/// What a cell of a shape of the first page is, by inheritance.
fn number(d: &Document, id: u32, cell: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    match val_of(&found) {
        Some(Val::N(n)) => n,
        other => panic!("shape {id}: {cell} holds {other:?}"),
    }
}

/// The text block a line holds is the line's, not the master's, whatever the file: the cells of `network.link` after the second trip are the same as after the first, and a line stretched after the
/// reopen has a text block as long as it is.
#[test]
fn a_line_read_from_an_odg_has_its_text_block_worked_out_by_the_line_it_is() {
    let dir = Dir::new();
    let mut s = page_at(100.0);
    drop_on(&mut s, "network.link");
    let mut t = reopened(&mut s, &dir, "one.odg");
    let once = lengths_of(doc(&t), first_shape(&t));
    assert!(near(once["TxtWidth"], 150.0), "the text block is as long as the line (150 drawing units at 1:100): {}", once["TxtWidth"]);
    // A second trip changes nothing.
    let mut u = reopened(&mut t, &dir, "two.odg");
    let twice = lengths_of(doc(&u), first_shape(&u));
    assert_eq!(twice.keys().collect::<Vec<_>>(), once.keys().collect::<Vec<_>>(), "the same cells");
    assert!(twice.iter().all(|(name, n)| near(*n, once[name])), "the same lengths after a second trip: {twice:?} against {once:?}");
    // A stretch of the reread line works its text block out again: twice as long, so is the block.
    let line = first_shape(&u);
    let page = first_page(doc(&u));
    let end = number(doc(&u), line, "BeginX") + 2.0 * once["Width"];
    run(&mut u, "sheet.set_cell", json!({"id": line, "page": page, "cell": "EndX", "formula": format!("{end}")}));
    let stretched = lengths_of(doc(&u), line);
    assert!(near(stretched["Width"], 2.0 * once["Width"]), "the line is twice as long: {}", stretched["Width"]);
    assert!(near(stretched["TxtWidth"], 2.0 * once["Width"]), "and so is its text block: {}", stretched["TxtWidth"]);
}

// ---- 2. BPMN's Pool at every scale ---------------------------------------------------------------------------------------------------------

/// The scales a pool is dropped on, as drawing inches to a paper inch: within the range of eight (1:1, 1:2, 1:4, 1:6, 1:7, 1:8 and a hair over it, and the enlargements 2:1 and 8:1) and beyond it
/// (1:16, 1:100, 20:1, the least and the greatest a page may have).
const POOL_SCALES: [f64; 14] = [1.0, 2.0, 4.0, 6.0, 7.0, 8.0, 16.0, 100.0, 90_000.0, 0.5, 0.125, 0.05, 0.001, 8.000_000_004];

/// **The second bug.** The Pool is a structure made by code, and its size is the range of eight's: the master's 8 by 2 drawing units within the range (1 by a quarter inch on paper at 1:8), and
/// 8 by 2 inches of paper beyond it. A page at 1:6, 1:7 or 1:8 refused it with "`height` must be at least 0.4 inch", the least of a lane held to a drawing size in the page's units.
#[test]
fn bpmn_pool_is_dropped_at_every_scale_at_the_size_the_range_of_eight_gives() {
    for k in POOL_SCALES {
        let mut s = page_at(k);
        let dropped = run(&mut s, "shape.drop", json!({"master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25}));
        let within = (0.125..=8.0 + 1e-8).contains(&k);
        let (width, height) = if within { (8.0 / k, 2.0 / k) } else { (8.0, 2.0) };
        let reported = (dropped["width"].as_f64().unwrap_or(f64::NAN), dropped["height"].as_f64().unwrap_or(f64::NAN));
        assert!(near(reported.0, width) && near(reported.1, height), "at 1:{k} the pool is {width} by {height} inches on paper: {dropped}");
        let found = run(&mut s, "swimlane.info", json!({}));
        let rect: Vec<f64> = found["structures"][0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(
            near(rect[2] - rect[0], width) && near(rect[3] - rect[1], height),
            "at 1:{k} the structure is {width} by {height} paper inches: {rect:?}"
        );
        assert_eq!(found["structures"][0]["lanes"].as_array().map(Vec::len), Some(1), "at 1:{k} one lane");
        assert_eq!(check_session(&s), Ok(()), "at 1:{k}");
    }
}

/// **The same bug in another master.** Every master of every built-in stencil is dropped at the scales round the edge of the range of eight, 1:6, 1:7, 1:8, a hair over 1:8 (the edge counts a
/// little over) and 8:1: none is refused, and the drawing is a valid one. (Only the Pool was, at the first three: the others take the drawing size of the master with no
/// least held to it.)
#[test]
fn every_master_is_dropped_at_the_scales_round_the_edge_of_the_range() {
    let mut failed: Vec<String> = Vec::new();
    for k in [6.0, 7.0, 8.0, 8.000_000_004, 0.125] {
        for spec in all_masters() {
            let mut s = page_at(k);
            match s.execute("shape.drop", &json!({"master": spec.id, "page": 1, "x": 5.3, "y": 4.1})) {
                Err(e) => failed.push(format!("{} at 1:{k}: {e}", spec.id)),
                Ok(_) => {
                    if let Err(e) = check_session(&s) {
                        failed.push(format!("{} at 1:{k}: {e}", spec.id));
                    }
                }
            }
        }
    }
    assert!(failed.is_empty(), "{} drops failed:\n{}", failed.len(), failed.join("\n"));
}

/// A pool dropped at 1:8 is a pool: a lane can be added to it and it survives a trip through each file type, as it does at 1:1.
#[test]
fn a_pool_dropped_at_the_edge_of_the_range_is_worked_on_as_any_other() {
    for file in ["pool.diacraft", "pool.vsdx", "pool.odg"] {
        let dir = Dir::new();
        let mut s = page_at(8.0);
        run(&mut s, "shape.drop", json!({"master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.5, "y": 4.25}));
        run(&mut s, "swimlane.insert", json!({"page": 1}));
        let found = run(&mut s, "swimlane.info", json!({}));
        assert_eq!(found["structures"][0]["lanes"].as_array().map(Vec::len), Some(2), "{file}: two lanes");
        let mut t = reopened(&mut s, &dir, file);
        let after = run(&mut t, "swimlane.info", json!({}));
        assert_eq!(after["structures"][0]["lanes"].as_array().map(Vec::len), Some(2), "{file}: two lanes after the reopen");
        assert_eq!(check_session(&t), Ok(()), "{file}");
    }
}

// ---- 3. the formulas of a group instance read from an `.odg` -------------------------------------------------------------------------------------

/// Sets a row of shape data on the shape `id`.
fn set_data(s: &mut Session, id: u32, row: &str, value: Value) {
    run(s, "shapedata.set", json!({ "id": id, "row": row, "value": value }));
}

/// A drop of `master` at 1:100 with the shape data `data` set, saved as `.odg` and read again: the original session, its shape, the reread session and its shape.
fn trip_group(master: &str, data: &[(&str, Value)]) -> (Session, u32, Session, u32, Dir) {
    let dir = Dir::new();
    let mut s = page_at(100.0);
    let id = id_of(&drop_on(&mut s, master));
    for (row, value) in data {
        set_data(&mut s, id, row, value.clone());
    }
    let t = reopened(&mut s, &dir, "group.odg");
    let read = first_shape(&t);
    (s, id, t, read, dir)
}

/// Whether the key of a snapshot is a member's (`m0.Width`), and not the group's own.
fn is_member_key(key: &str) -> bool {
    key.strip_prefix('m').and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_digit())
}

/// The part of a snapshot whose key `keep` holds for.
fn only(snap: &BTreeMap<String, Val>, keep: impl Fn(&str) -> bool) -> BTreeMap<String, Val> {
    snap.iter().filter(|(key, _)| keep(key)).map(|(key, v)| (key.clone(), v.clone())).collect()
}

/// Asserts that the cells of `keep` that the drawing `a` holds are the drawing `b`'s too, in words that say which.
#[track_caller]
fn assert_same(what: &str, a: &BTreeMap<String, Val>, b: &BTreeMap<String, Val>, keep: impl Fn(&str) -> bool) {
    let (a, b) = (only(a, &keep), only(b, &keep));
    assert!(!a.is_empty(), "{what}: there are cells to compare");
    let bad = differences(&a, &b);
    assert!(bad.is_empty(), "{what}: {} of {} cells differ, e.g. {}", bad.len(), a.len(), bad.iter().take(4).cloned().collect::<Vec<_>>().join("; "));
}

/// **The third bug, BPMN.** A task dropped at 1:100 with its markers set, saved as `.odg` and read again: every cell is what it was; after a resize, the body's geometry (its corner a tenth of an
/// inch on paper: `0.1*IF(Sheet.5!DropOnPageScale>0,...)`, 10 drawing units, which read the factor of the page's shape 5 and came out 0.1) and every marker's `NoShow` are what they were; and a marker
/// set or cleared afterwards shows or hides as it does in the drawing that was never saved (`Sheet.5!Prop.BpmnLoopType` named the page's shape 5). (The size of a marker is a value in the file, as
/// every member's frame is, so what a resize stretches of a marker is left to the limit that says so.)
#[test]
fn a_bpmn_task_read_from_an_odg_keeps_its_lengths_and_its_markers_by_the_group_it_is_in() {
    let data = [("BpmnTaskType", json!("Service")), ("BpmnLoopType", json!("Standard")), ("BpmnIsForCompensation", json!(true))];
    let (mut s, id, mut t, read, _dir) = trip_group("bpmn.task", &data);
    let (a, b) = (snap_of(doc(&s), id), snap_of(doc(&t), read));
    assert!(a.len() > 100, "a task has many cells: {}", a.len());
    assert_same("the task after the reopen", &a, &b, |_| true);
    // A resize works every formula out again.
    assert!(resize_wider(&mut s, id, 100.0) && resize_wider(&mut t, read, 100.0), "the task is resized");
    let (a, b) = (snap_of(doc(&s), id), snap_of(doc(&t), read));
    assert_same("the body after a resize", &a, &b, |key| key.starts_with("m0."));
    assert_same("the group after a resize", &a, &b, |key| !is_member_key(key));
    assert_same("the markers after a resize", &a, &b, |key| key.ends_with(".NoShow"));
    assert!(
        matches!(b.get("m0.GeometrySome(0)#0.X"), Some(Val::N(n)) if near(*n, 10.0)),
        "the corner is 10 drawing units: {:?}",
        b.get("m0.GeometrySome(0)#0.X")
    );
    // Markers set or cleared after the reopen follow the group's data, as in the drawing that was never saved.
    let mut shown: Vec<BTreeMap<String, Val>> = vec![only(&b, |key| key.ends_with(".NoShow"))];
    for (row, value) in [
        ("BpmnLoopType", json!("None")),
        ("BpmnTaskType", json!("None")),
        ("BpmnIsForCompensation", json!(false)),
        ("BpmnLoopType", json!("Parallel MultiInstance")),
        ("BpmnTaskType", json!("User")),
        ("BpmnIsForCompensation", json!(true)),
    ] {
        set_data(&mut s, id, row, value.clone());
        set_data(&mut t, read, row, value);
        let (a, b) = (snap_of(doc(&s), id), snap_of(doc(&t), read));
        assert_same(&format!("the markers with {row} set"), &a, &b, |key| key.ends_with(".NoShow"));
        assert_same(&format!("the body with {row} set"), &a, &b, |key| key.starts_with("m0."));
        shown.push(only(&b, |key| key.ends_with(".NoShow")));
    }
    assert!(
        shown.windows(2).filter(|pair| pair[0] != pair[1]).count() >= 5,
        "the markers show and hide as the shape data is set: they are not frozen"
    );
    assert_eq!(check_session(&t), Ok(()));
}

/// The `Style` of the first `Character` row of each member of a group: the name of a class is italic when it is abstract.
fn name_styles(d: &Document, id: u32) -> Vec<Option<f64>> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let res = Resolver::new(d);
    res.effective_members(&at)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| match m {
            Member::Local(child) => Some(child),
            _ => None,
        })
        .map(|child| {
            let addr = CellAddr::in_row("Character", None, RowKey::Ix(0), "Style");
            res.resolve(&SheetRef::Shape(child), &addr).as_ref().and_then(val_of).and_then(|v| if let Val::N(n) = v { Some(n) } else { None })
        })
        .collect()
}

/// **The third bug, UML.** A class dropped at 1:100 and read again from an `.odg`: every cell is what it was, also after a resize, and its name is italic when `Abstract` is set after the
/// reopen (`IF(Sheet.5!Prop.Abstract,3,1)` in the name's `Character` row named the page's shape 5, and the name was never italic).
#[test]
fn a_uml_class_read_from_an_odg_keeps_its_lengths_and_its_abstract_name_by_the_group_it_is_in() {
    let (mut s, id, mut t, read, _dir) = trip_group("uml-class.class", &[]);
    assert_same("the class after the reopen", &snap_of(doc(&s), id), &snap_of(doc(&t), read), |_| true);
    assert!(resize_wider(&mut s, id, 100.0) && resize_wider(&mut t, read, 100.0), "the class is resized");
    assert_same("the class after a resize", &snap_of(doc(&s), id), &snap_of(doc(&t), read), |_| true);
    let plain = name_styles(doc(&t), read);
    set_data(&mut s, id, "Abstract", json!(true));
    set_data(&mut t, read, "Abstract", json!(true));
    let (italic_before, italic_after) = (name_styles(doc(&s), id), name_styles(doc(&t), read));
    assert_ne!(italic_before, plain, "an abstract class's name changes its style in the drawing that was never saved");
    assert_eq!(italic_after, italic_before, "and in the drawing that was read from an .odg");
    set_data(&mut t, read, "Abstract", json!(false));
    assert_eq!(name_styles(doc(&t), read), plain, "a class that is no longer abstract is as it was");
}

/// **The style cells of a member that follow the group's shape data.** A BPMN event's ring is dashed when the event is non-interrupting (`LinePattern` is `IF(STRSAME(Sheet.5!Prop.BpmnEventType,
/// "Intermediate (Non-Interrupting)"),2,1)` in the member), and the fill of a filled marker is its own line colour (`LineColor`). A style cell holds the value the file's attribute says
/// and the formula beside it, which named the page's shape 5 when it was read again, and the ring never turned dashed.
#[test]
fn a_bpmn_event_read_from_an_odg_follows_its_shape_data_in_the_pattern_of_its_ring() {
    let (mut s, id, mut t, read, _dir) = trip_group("bpmn.intermediate-event", &[]);
    assert_same("the event after the reopen", &snap_of(doc(&s), id), &snap_of(doc(&t), read), |_| true);
    let patterns = |snap: &BTreeMap<String, Val>| only(snap, |key| key.ends_with(".LinePattern"));
    let plain = patterns(&snap_of(doc(&t), read));
    assert!(!plain.is_empty(), "a member of the event holds a pattern");
    for event in ["Intermediate (Non-Interrupting)", "Intermediate (Throwing)", "Intermediate"] {
        set_data(&mut s, id, "BpmnEventType", json!(event));
        set_data(&mut t, read, "BpmnEventType", json!(event));
        let (a, b) = (snap_of(doc(&s), id), snap_of(doc(&t), read));
        assert_same(&format!("the pattern of the ring of {event}"), &a, &b, |key| key.ends_with(".LinePattern"));
        if event == "Intermediate (Non-Interrupting)" {
            assert_ne!(patterns(&b), plain, "the ring of a non-interrupting event is dashed in the drawing that was read from an .odg");
        }
    }
    assert_eq!(check_session(&t), Ok(()));
}

/// The `ID`s that the formula `source` names by `Sheet.<id>!`.
fn named_ids(source: &str) -> Vec<u32> {
    let lower = source.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("sheet.") {
        let digits_at = from + at + "sheet.".len();
        let digits: String = lower[digits_at..].chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && lower[digits_at + digits.len()..].starts_with('!') {
            out.extend(digits.parse::<u32>());
        }
        from = digits_at;
    }
    out
}

/// Every formula a shape of the tree holds in a cell of its own, with where it is, and the `ID` of each shape of the tree.
fn formulas_of(shape: &diacraft_doc::Shape, ids: &mut Vec<u32>, out: &mut Vec<(u32, String, String)>) {
    ids.push(shape.id);
    let mut note = |place: String, cell: &diacraft_doc::Cell| {
        if let diacraft_doc::FormulaSlot::Local(f) = &cell.formula {
            out.push((shape.id, place, f.source.to_string()));
        }
    };
    for (name, cell) in shape.sheet.cells.iter() {
        note(name.as_str().to_owned(), cell);
    }
    for section in shape.sheet.sections.iter() {
        for (name, cell) in section.cells.iter() {
            note(format!("{}{:?}.{}", section.name.as_str(), section.ix, name.as_str()), cell);
        }
        for (r, row) in section.rows.iter().enumerate() {
            for (name, cell) in row.cells.iter() {
                note(format!("{}{:?}#{r}.{}", section.name.as_str(), section.ix, name.as_str()), cell);
            }
        }
    }
    for child in shape.children.iter() {
        formulas_of(child, ids, out);
    }
}

/// A group master of two members, the first of which says a point of its outline by the width of the second (`Sheet.7!Width`), and a page that holds an instance with the first member only: the second
/// was taken out of the instance, so nothing of the instance stands for the shape the formula names.
fn lacking_member_drawing() -> Vec<u8> {
    let frame = |pin_x: &str, w: &str| {
        format!(
            r#"<Cell N="PinX" V="{pin_x}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="{w}"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="0.5"/>"#
        )
    };
    let outline = |x: &str| {
        format!(
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" {x}/><Cell N="Y" V="0"/></Row></Section>"#
        )
    };
    let master_shapes = format!(
        r#"<Shape ID="5" Type="Group"><Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/><Shapes><Shape ID="6" Type="Shape">{}{}</Shape><Shape ID="7" Type="Shape">{}</Shape></Shapes></Shape>"#,
        frame("0.5", "1"),
        outline(r#"F="Sheet.7!Width*0.5""#),
        frame("1.5", "1"),
    );
    let page_shapes = r#"<Shape ID="1" Type="Group" Master="1"><Cell N="PinX" V="3"/><Cell N="PinY" V="3"/><Shapes><Shape ID="2" Type="Shape" MasterShape="6"/></Shapes></Shape>"#;
    let sheet = r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Group").shapes(&master_shapes))
        .page(PageSpec::new(1, "Page-1").sheet(sheet).shapes(page_shapes))
        .build()
}

/// **A formula that names a shape nothing of the instance stands for** is the constant of its value in the copy, and no formula of the file is left naming the master's number for it.
/// The instance was opened from a VSDX file, saved as `.odg` and read again: the member's outline has the value the formula gave, no other shape of the page is named, and the drawing is valid.
#[test]
fn a_master_formula_that_names_a_shape_the_instance_lacks_is_the_constant_of_its_value() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("lacking.vsdx", &lacking_member_drawing())}));
    let mut t = reopened(&mut s, &dir, "lacking.odg");
    assert_eq!(check_session(&t), Ok(()));
    let top = doc(&t).pages.get(0).unwrap().shapes.get(0).unwrap();
    let (mut ids, mut formulas) = (Vec::new(), Vec::new());
    formulas_of(top, &mut ids, &mut formulas);
    assert!(
        formulas.iter().all(|(_, _, f)| !named_ids(f).contains(&7)),
        "no formula names the master's shape 7, which the page's shape 7 would be: {formulas:?}"
    );
    // The member kept the value the formula gave in the drawing that was saved.
    let member = top.children.get(0).unwrap().id;
    let at = doc(&t).find_shape(Owner::Page(first_page(doc(&t))), member).unwrap();
    let res = Resolver::new(doc(&t));
    let rows = res.effective_rows(&SheetRef::Shape(at), &diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap();
    let x = rows.get(1).and_then(|row| row.cells.iter().find(|(n, _)| n.as_str() == "X")).and_then(|(_, found)| val_of(found));
    assert!(matches!(x, Some(Val::N(n)) if near(n, 1.0)), "the outline's second point keeps the value the master gave it: {x:?}");
    let _ = &mut t;
}

/// **Every group master of the catalogue.** Dropped at 1:100, saved as `.odg` and read again, each group holds every cell it held, and **each formula that a member holds in a cell of its own names
/// the group or a member of it by the `ID` it has in the page** (a formula the copy took from the master said `Sheet.5` for the master's group, the page's shape 5 when it was read again).
#[test]
fn every_group_master_read_from_an_odg_names_its_own_group_in_the_formulas_it_holds() {
    let mut groups = 0;
    let mut failed: Vec<String> = Vec::new();
    for spec in all_masters().filter(|spec| spec.is_group()) {
        let dir = Dir::new();
        let mut s = page_at(100.0);
        let r = drop_on(&mut s, spec.id);
        if r["master"].is_null() {
            continue;
        }
        groups += 1;
        let id = id_of(&r);
        let t = reopened(&mut s, &dir, "sweep.odg");
        let read = first_shape(&t);
        let bad = differences(&snap_of(doc(&s), id), &snap_of(doc(&t), read));
        if !bad.is_empty() {
            failed.push(format!(
                "{} after the reopen: {} cells differ, e.g. {}",
                spec.id,
                bad.len(),
                bad.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
            ));
        }
        let top = doc(&t).pages.get(0).unwrap().shapes.get(0).unwrap();
        let (mut ids, mut formulas) = (Vec::new(), Vec::new());
        formulas_of(top, &mut ids, &mut formulas);
        let stray: Vec<String> = formulas
            .iter()
            .filter(|(_, _, f)| named_ids(f).iter().any(|named| !ids.contains(named)))
            .map(|(shape, place, f)| format!("shape {shape} {place}: {f}"))
            .collect();
        if !stray.is_empty() {
            failed.push(format!(
                "{} holds {} formulas that name a shape outside the group (its shapes are {ids:?}), e.g. {}",
                spec.id,
                stray.len(),
                stray.iter().take(2).cloned().collect::<Vec<_>>().join("; ")
            ));
        }
    }
    assert!(groups >= 20, "the sweep covers the group masters: {groups}");
    assert!(failed.is_empty(), "{} group masters differ:\n{}", failed.len(), failed.join("\n"));
}
