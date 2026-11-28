//! **A scene put right equals a scene read whole** (the design's section 7; the lesson: a kept index is compared with a fresh read after random sequences that
//! include save, undo, redo, delete, duplicate, group and ungroup). One scene lives through a whole sequence of up to 40 edits of a session, and after each of them it is put right and held
//! to a scene read whole from the same document, field by field and by the answers of its indexes, and to the oracle, which works the obstacles, the glue and the legs out by readers the scene
//! does not use.
//!
//! The edits include **shapes whose cells read other shapes** (`Sheet.2!PinX`, a member of a group as the reader or the one read), moved with the shape they read; a group added whose member repeats the `ID` of a shape that is there (a
//! repeated `ID` met in the middle of a put right); and **put rights that are given too little work** and stop. What each put right that is complete reports is held to a model of the boxes the
//! scene was settled on, kept apart from the scene's own table (`Settled`): a put right that stopped, or was made again, loses nothing of what the next one reports.
//!
//! **The run is the same run every time**: the sequences come from a fixed seed (a seed taken from the clock made the redos that worked 16 to 43 of the 20 the sequences need, and the test
//! failed in 3 of 8 runs of one build and 1 of 8 of another). `ROUTE_SCENE_SEED=<n>` makes another run by hand and `ROUTE_SCENE_CASES=<n>` a longer one; the counts of what worked are held only to the fixed seed's.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use diacraft_doc::{Document, Shape};
use diacraft_route::Work;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use serde_json::{Value, json};

use super::build::{group, made, placeable};
use super::oracle::{assert_matches, expected};
use super::plenty;
use crate::Session;
use crate::draw::obstacle::Bounds;
use crate::draw::route_scene::{Changed, ChangedObstacle, MAX_SCENE_SHAPES, Scene, rect_of};

/// One edit of the sequence. The numbers pick a shape of the page by position, or a value from a short list.
#[derive(Clone, Debug)]
enum Op {
    Drop {
        master: u8,
        x: u8,
        y: u8,
    },
    Move {
        pick: u8,
        dx: i8,
        dy: i8,
    },
    Resize {
        pick: u8,
        w: u8,
        h: u8,
    },
    Rotate {
        pick: u8,
        by: u8,
    },
    Paste {
        pick: u8,
    },
    Duplicate {
        pick: u8,
    },
    Delete {
        pick: u8,
    },
    Group {
        a: u8,
        b: u8,
    },
    Ungroup {
        pick: u8,
    },
    Connect {
        a: u8,
        b: u8,
    },
    Undo,
    Redo,
    Reopen {
        format: u8,
    },
    MasterCell {
        value: u8,
    },
    PageNode {
        value: u8,
    },
    Behavior {
        pick: u8,
        placement: u8,
    },
    Layer {
        pick: u8,
        visible: bool,
    },
    /// A shape moved, a member of a group as well as a top-level shape.
    MoveInner {
        pick: u8,
        dx: i8,
        dy: i8,
    },
    /// A cell of one shape made to read a cell of another (`Sheet.2!PinX+1`), either of them a member of a group as well as a top-level shape.
    Link {
        a: u8,
        b: u8,
        cell: u8,
        plus: u8,
    },
    /// A group added at the end of the page whose member has the `ID` of a shape that is there.
    Echo {
        pick: u8,
    },
    /// A shape moved, and a put right that is given `work` steps and stops when they are spent (the edit after it puts the scene right).
    Starve {
        pick: u8,
        dx: i8,
        dy: i8,
        work: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(master, x, y)| Op::Drop { master, x, y }),
        6 => (any::<u8>(), any::<i8>(), any::<i8>()).prop_map(|(pick, dx, dy)| Op::Move { pick, dx, dy }),
        2 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(pick, w, h)| Op::Resize { pick, w, h }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(pick, by)| Op::Rotate { pick, by }),
        2 => any::<u8>().prop_map(|pick| Op::Paste { pick }),
        2 => any::<u8>().prop_map(|pick| Op::Duplicate { pick }),
        3 => any::<u8>().prop_map(|pick| Op::Delete { pick }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Group { a, b }),
        2 => any::<u8>().prop_map(|pick| Op::Ungroup { pick }),
        4 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Connect { a, b }),
        3 => Just(Op::Undo),
        4 => Just(Op::Redo),
        1 => any::<u8>().prop_map(|format| Op::Reopen { format }),
        1 => any::<u8>().prop_map(|value| Op::MasterCell { value }),
        1 => any::<u8>().prop_map(|value| Op::PageNode { value }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(pick, placement)| Op::Behavior { pick, placement }),
        1 => (any::<u8>(), any::<bool>()).prop_map(|(pick, visible)| Op::Layer { pick, visible }),
        3 => (any::<u8>(), any::<i8>(), any::<i8>()).prop_map(|(pick, dx, dy)| Op::MoveInner { pick, dx, dy }),
        4 => (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(a, b, cell, plus)| Op::Link { a, b, cell, plus }),
        1 => any::<u8>().prop_map(|pick| Op::Echo { pick }),
        3 => (any::<u8>(), any::<i8>(), any::<i8>(), any::<u8>()).prop_map(|(pick, dx, dy, work)| Op::Starve { pick, dx, dy, work }),
    ]
}

static CASES: AtomicU32 = AtomicU32::new(0);

/// How many times each command of the sequences worked (a command that fails changes nothing, and a sequence of commands that all fail proves nothing): the test holds each to a number.
static WORKED: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

fn worked(id: &str) {
    *WORKED.lock().unwrap().entry(id.to_owned()).or_insert(0) += 1;
}

/// The boxes the scene is settled on, by `ID`, worked out apart from the scene's own table: what a put right that is complete must report is the difference between these before and after it
/// (the module's "What it reports": a candidate whose box or layers moved is settled on the box it has now, one that did not keeps the box it had, and one that is no longer there goes).
#[derive(Default)]
struct Settled(BTreeMap<u32, (Bounds, bool, Option<diacraft_route::Rect>)>);

impl Settled {
    /// Takes in what the scene now holds, and gives what a put right that made it so must have reported.
    fn step(&mut self, scene: &Scene) -> Vec<ChangedObstacle> {
        let mut reports = Vec::new();
        let mut next = BTreeMap::new();
        for (id, facts, obstacle, _) in scene.dump().candidates {
            let now = obstacle.then(|| rect_of(&facts));
            let base = match self.0.get(&id) {
                None => {
                    if now.is_some() {
                        reports.push(ChangedObstacle { id, old: None, new: now });
                    }
                    now
                }
                Some((bounds, hidden, base)) => {
                    if *bounds != facts.bounds || *hidden != facts.hidden {
                        if now != *base {
                            reports.push(ChangedObstacle { id, old: *base, new: now });
                        }
                        now
                    } else {
                        *base
                    }
                }
            };
            next.insert(id, (facts.bounds, facts.hidden, base));
        }
        for (id, (_, _, base)) in &self.0 {
            if let (false, Some(old)) = (next.contains_key(id), base) {
                reports.push(ChangedObstacle { id: *id, old: Some(*old), new: None });
            }
        }
        reports.sort_by_key(|c| c.id);
        self.0 = next;
        reports
    }
}

/// A session on a page with three boxes and a connector between two, a scene that has seen it, and a folder to save into.
struct World {
    s: Session,
    scene: Scene,
    settled: Settled,
    dir: PathBuf,
}

impl World {
    fn new() -> World {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let dir = std::env::temp_dir().join(format!("diacraft-route-scene-{}-{}", std::process::id(), CASES.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let mut world = World { s, scene: Scene::new(1), settled: Settled::default(), dir };
        let mut ids = Vec::new();
        for (x, y) in [(2.0, 2.0), (6.0, 2.0), (4.0, 5.0)] {
            let made = world.s.execute("shape.drop", &json!({"master": "process", "stencil": "flowchart", "page": 1, "x": x, "y": y})).unwrap();
            ids.push(made["id"].as_u64().unwrap());
        }
        let _ = world.s.execute("connector.add", &json!({"from": {"id": ids[0], "point": 3}, "to": {"id": ids[1], "point": 1}}));
        world
    }

    /// The `ID`s of the page's top-level shapes, in page order.
    fn ids(&self) -> Vec<u32> {
        self.s.document().and_then(|d| d.page(1)).map(|p| p.shapes.iter().map(|s| s.id).collect()).unwrap_or_default()
    }

    fn pick(&self, k: u8) -> Option<u32> {
        let ids = self.ids();
        (!ids.is_empty()).then(|| ids[usize::from(k) % ids.len()])
    }

    /// The `ID`s of every shape of the page, the top-level shapes first, then the members of the groups, three levels down.
    fn all_ids(&self) -> Vec<u32> {
        fn members(shape: &Shape, depth: u8, out: &mut Vec<u32>) {
            for member in shape.children.iter() {
                out.push(member.id);
                if depth < 3 {
                    members(member, depth + 1, out);
                }
            }
        }
        let mut out = self.ids();
        if let Some(page) = self.s.document().and_then(|d| d.page(1)) {
            for top in page.shapes.iter() {
                members(top, 1, &mut out);
            }
        }
        out
    }

    /// As [`World::pick`], among every shape of the page, a member of a group too.
    fn pick_any(&self, k: u8) -> Option<u32> {
        let ids = self.all_ids();
        (!ids.is_empty()).then(|| ids[usize::from(k) % ids.len()])
    }

    fn run(&mut self, id: &str, params: Value) {
        // A command that fails changes nothing: the edits of the sequence are made to go wrong too.
        if self.s.execute(id, &params).is_ok() {
            worked(id);
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(), String> {
        let at = |k: u8| f64::from(k) / 255.0 * 12.0;
        match *op {
            Op::Drop { master, x, y } => {
                let name = ["rectangle", "process", "decision", "5-point star", "container"][usize::from(master) % 5];
                self.run("shape.drop", json!({"master": name, "page": 1, "x": at(x), "y": at(y)}));
            }
            Op::Move { pick, dx, dy } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.move", json!({"ids": [id], "dx": f64::from(dx) / 20.0, "dy": f64::from(dy) / 20.0}));
                }
            }
            Op::Resize { pick, w, h } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.resize", json!({"id": id, "x": 1.0, "y": 1.0, "width": 0.5 + at(w) / 3.0, "height": 0.5 + at(h) / 3.0}));
                }
            }
            Op::Rotate { pick, by } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.rotate", json!({"ids": [id], "by": f64::from(by)}));
                }
            }
            Op::Paste { pick } => {
                if let Some(id) = self.pick(pick) {
                    self.run("edit.copy", json!({"ids": [id]}));
                    self.run("edit.paste", json!({"page": 1}));
                }
            }
            Op::Duplicate { pick } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.duplicate", json!({"ids": [id]}));
                }
            }
            Op::Delete { pick } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.delete", json!({"ids": [id]}));
                }
            }
            Op::Group { a, b } => {
                if let (Some(x), Some(y)) = (self.pick(a), self.pick(b)) {
                    self.run("shape.group", json!({"ids": [x, y]}));
                }
            }
            Op::Ungroup { pick } => {
                if let Some(id) = self.pick(pick) {
                    self.run("shape.ungroup", json!({"ids": [id]}));
                }
            }
            Op::Connect { a, b } => {
                if let (Some(x), Some(y)) = (self.pick(a), self.pick(b)) {
                    self.run("connector.add", json!({"from": {"id": x}, "to": {"id": y}}));
                }
            }
            Op::Undo => self.run("edit.undo", json!({})),
            Op::Redo => self.run("edit.redo", json!({})),
            Op::Reopen { format } => {
                let name = ["a.vsdx", "a.odg", "a.diacraft"][usize::from(format) % 3];
                let path = self.dir.join(name).to_string_lossy().into_owned();
                if self.s.execute("file.save_as", &json!({"path": path})).is_ok() {
                    worked(&format!("file.save_as {name}"));
                    self.run("file.open", json!({"path": path}));
                }
            }
            Op::MasterCell { value } => {
                let objtype = ["0", "1", "4", "8"][usize::from(value) % 4];
                let _ = self.s.edit("master", |doc| {
                    let Some(master) = doc.masters.get(usize::from(value) / 4 % doc.masters.len().max(1)).cloned() else { return Ok(()) };
                    let mut master = (*master).clone();
                    if let Some(top) = master.shapes.get(0).cloned() {
                        let mut top = (*top).clone();
                        std::sync::Arc::make_mut(&mut top.sheet)
                            .cells
                            .insert(diacraft_doc::CellName::new("ObjType"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(objtype)));
                        master.shapes = std::iter::once(std::sync::Arc::new(top)).collect();
                    }
                    let at = usize::from(value) / 4 % doc.masters.len().max(1);
                    let _ = doc.masters.set(at, std::sync::Arc::new(master));
                    Ok(())
                });
            }
            Op::PageNode { value } => {
                let v = 0.05 + f64::from(value) / 255.0;
                self.run("page.set_routing", json!({"connector_to_shape": {"x": v, "y": v / 2.0}}));
            }
            Op::Behavior { pick, placement } => {
                if let Some(id) = self.pick(pick) {
                    let placement = ["auto", "route_around", "none"][usize::from(placement) % 3];
                    self.run("shape.set_behavior", json!({"ids": [id], "placement": placement}));
                }
            }
            Op::Layer { pick, visible } => {
                if let Some(id) = self.pick(pick) {
                    self.run("layer.add", json!({"name": "R"}));
                    self.run("layer.assign", json!({"ids": [id], "layers": ["R"]}));
                    self.run("layer.set", json!({"layer": "R", "visible": visible}));
                }
            }
            Op::MoveInner { pick, dx, dy } => {
                if let Some(id) = self.pick_any(pick) {
                    self.run("shape.move", json!({"ids": [id], "dx": f64::from(dx) / 20.0, "dy": f64::from(dy) / 20.0}));
                }
            }
            Op::Link { a, b, cell, plus } => {
                if let (Some(read), Some(id)) = (self.pick_any(a), self.pick_any(b)) {
                    let cell = ["PinX", "PinY", "Width", "Height"][usize::from(cell) % 4];
                    let formula = if cell.starts_with("Pin") {
                        format!("Sheet.{read}!{cell}+{}", f64::from(plus) / 64.0)
                    } else {
                        format!("Sheet.{read}!{cell}*0.5+0.5")
                    };
                    self.run("sheet.set_cell", json!({"id": id, "page": 1, "cell": cell, "formula": formula}));
                }
            }
            Op::Echo { pick } => {
                if let Some(id) = self.pick(pick) {
                    let top = self.ids().into_iter().max().unwrap_or(0).saturating_add(1_000);
                    let shape = made(group(top, (20.0, 0.0, 4.0, 4.0), "8", vec![placeable(id, (0.0, 0.0, 1.0, 1.0))]));
                    let added = self.s.edit("echo", |doc| {
                        if let Some(page) = crate::draw::page_mut(doc, 1) {
                            page.shapes.push(shape);
                        }
                        Ok(())
                    });
                    if added.is_ok() {
                        worked("echo");
                    }
                }
            }
            Op::Starve { pick, dx, dy, work } => {
                if let Some(id) = self.pick_any(pick) {
                    self.run("shape.move", json!({"ids": [id], "dx": f64::from(dx) / 20.0, "dy": f64::from(dy) / 20.0}));
                }
                let doc = self.s.document().ok_or("no document")?.clone();
                // Fewer steps than the walk of a page of eight shapes and a read or two.
                let steps = u64::from(work) % 6;
                let changed = self.put_right(&doc, &mut Work::new(steps), &format!("with {steps} steps of work, after {op:?}"))?;
                worked(if changed.incomplete { "put right that stopped" } else { "put right that did not" });
            }
        }
        Ok(())
    }

    /// Puts the scene right with `doc` and `work`, and holds what it says to what must hold of it: work that ran out says so and reports nothing and leaves the scene incomplete; a put right that
    /// is complete reports what the boxes it settled on, against the ones before it, make (`Settled`).
    fn put_right(&mut self, doc: &Document, work: &mut Work, what: &str) -> Result<Changed, String> {
        let changed = self.scene.put_right(doc, 1, work);
        if changed.incomplete {
            if !changed.obstacles.is_empty() || self.scene.is_complete() {
                return Err(format!(
                    "{what}: a put right that ran out of work reports {:?} and the scene is complete: {}",
                    changed.obstacles,
                    self.scene.is_complete()
                ));
            }
            return Ok(changed);
        }
        let want = self.settled.step(&self.scene);
        if changed.obstacles != want {
            return Err(format!("{what}: the put right reported {:?} and the boxes the scene was settled on say {want:?}", changed.obstacles));
        }
        Ok(changed)
    }

    /// Puts the scene right with the session's document and holds it to a scene read whole and to the oracle.
    fn check(&mut self, what: &str) -> Result<(), String> {
        let doc = self.s.document().ok_or("no document")?.clone();
        let changed = self.put_right(&doc, &mut plenty(), what)?;
        let fresh = Scene::read(&doc, 1, &mut plenty());
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::assert_same(&self.scene, &fresh, what);
            assert_matches(&self.scene, &doc, MAX_SCENE_SHAPES, what);
        }));
        if let Err(panic) = held {
            return Err(panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default());
        }
        if changed.incomplete || !self.scene.is_complete() {
            return Err(format!("{what}: the scene is not complete"));
        }
        let ids: Vec<u32> = changed.obstacles.iter().map(|c| c.id).collect();
        if !ids.windows(2).all(|w| w[0] < w[1]) {
            return Err(format!("{what}: the changes are not in the order of the IDs, once each: {ids:?}"));
        }
        if let Some(c) = changed.obstacles.iter().find(|c| c.old == c.new) {
            return Err(format!("{what}: shape {} is reported with the same box before and after: {c:?}", c.id));
        }
        // What the scene says is an obstacle is what the oracle says, whatever it reported.
        let want = expected(&doc, 1, MAX_SCENE_SHAPES);
        if self.scene.obstacles().len() != want.obstacles.len() {
            return Err(format!("{what}: the router holds {} obstacles and the oracle {}", self.scene.obstacles().len(), want.obstacles.len()));
        }
        Ok(())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The seed a run is made from: `ROUTE_SCENE_SEED`, or the fixed one the gate uses.
const FIXED_SEED: u64 = 7;

fn seed() -> Option<u64> {
    std::env::var("ROUTE_SCENE_SEED").ok().and_then(|v| v.parse().ok())
}

fn runner(cases: u32) -> TestRunner {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed().unwrap_or(FIXED_SEED).to_le_bytes());
    TestRunner::new_with_rng(Config { cases, failure_persistence: None, ..Config::default() }, TestRng::from_seed(RngAlgorithm::ChaCha, &bytes))
}

/// 300 sequences of up to 40 edits, from a fixed seed.
#[test]
fn a_scene_put_right_equals_a_fresh_one() {
    let cases = std::env::var("ROUTE_SCENE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let mut runner = runner(cases);
    let outcome = runner.run(&prop::collection::vec(op(), 1..=40), |ops| {
        let mut world = World::new();
        world.check("the page as it starts").map_err(TestCaseError::fail)?;
        for (k, op) in ops.iter().enumerate() {
            world.apply(op).map_err(TestCaseError::fail)?;
            world.check(&format!("after edit {k}, {op:?}")).map_err(TestCaseError::fail)?;
        }
        Ok(())
    });
    if let Err(e) = outcome {
        panic!("{e}");
    }
    // The sequences did what they say: each kind of edit worked many times, in many shapes of page.
    let counts = WORKED.lock().unwrap().clone();
    eprintln!("WORKED {counts:?}");
    if seed().is_some() || cases != 300 {
        return;
    }
    let at_least = |id: &str, n: u32| {
        assert!(counts.get(id).copied().unwrap_or(0) >= n, "{id} worked {} times of the {n} the sequences need", counts.get(id).copied().unwrap_or(0))
    };
    for (id, n) in [
        ("shape.drop", 200),
        ("shape.move", 300),
        ("shape.resize", 30),
        ("shape.rotate", 30),
        ("edit.paste", 30),
        ("shape.duplicate", 30),
        ("shape.delete", 50),
        ("shape.group", 30),
        ("shape.ungroup", 30),
        ("connector.add", 50),
        ("edit.undo", 100),
        ("edit.redo", 10),
        ("page.set_routing", 5),
        ("shape.set_behavior", 30),
        ("layer.set", 5),
        ("file.open", 10),
        ("file.save_as a.vsdx", 3),
        ("file.save_as a.odg", 3),
        ("file.save_as a.diacraft", 3),
        ("sheet.set_cell", 100),
        ("echo", 50),
        ("put right that stopped", 100),
        ("put right that did not", 30),
    ] {
        at_least(id, n);
    }
}
