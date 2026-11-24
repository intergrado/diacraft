//! **Membership is a tree** (`docs/editing-commands.md`). A container is never its own member, directly or through the shapes it holds: `container.add` round a
//! container, a move of containers of one box, `container.add_member`, `container.attach_edge`, a paste, an undo and a redo leave no loop, and a file whose containers make one opens with the loop
//! broken, in words, and nothing else changed. The walks that follow a move and the fit stay bounded even with a loop that a command put in by hand.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, Shape, Value};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::container::{add_container, boxed, info, members, ties};
use crate::edit::{doc, drop_shape, num};
use crate::odg_build::Dir;
use crate::shapeops::{error_of, put, rect, session_with};
use crate::{blank_session, check_session, run, snapshot};

fn id_list(v: &Json) -> Vec<u32> {
    v.as_array().map(|a| a.iter().map(|m| u32::try_from(m.as_u64().unwrap()).unwrap()).collect()).unwrap_or_default()
}

/// The containers that hold the shape `id`, as `container.info` gives them.
fn inside(s: &mut Session, id: u32) -> Vec<u32> {
    let all = info(s, &[id]);
    all.first().map(|c| id_list(&c["inside"])).unwrap_or_default()
}

/// The `ID`s of page 1's own shapes, back to front.
fn page_ids(s: &Session) -> Vec<u32> {
    doc(s).page(1).unwrap().shapes.iter().map(|sh| sh.id).collect()
}

/// Panics when a container of page 1 holds itself, directly or through the shapes it holds, by its members (code 1) or by the containers its shapes are in (codes 4 and 7 to 10).
fn assert_tree(s: &mut Session) {
    let ids = page_ids(s);
    let mut down: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut up: HashMap<u32, Vec<u32>> = HashMap::new();
    for c in info(s, &ids) {
        let id = u32::try_from(c["id"].as_u64().unwrap()).unwrap();
        down.insert(id, id_list(&c["members"]));
        up.insert(id, id_list(&c["inside"]));
    }
    for (what, graph) in [("members", &down), ("inside", &up)] {
        for start in graph.keys() {
            let mut seen: HashSet<u32> = HashSet::new();
            let mut stack = vec![*start];
            while let Some(next) = stack.pop() {
                for m in graph.get(&next).into_iter().flatten() {
                    assert_ne!(m, start, "container {start} holds itself through its {what}: {graph:?}");
                    if seen.insert(*m) {
                        stack.push(*m);
                    }
                }
            }
        }
    }
}

// ---- the bug: a container put round a container ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_container_added_round_a_container_holds_it_and_is_not_held_by_it_and_both_are_fitted() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let inner = add_container(&mut s, json!({"ids": [a]}));
    let made = run(&mut s, "container.add", json!({"ids": [inner]}));
    let outer = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    assert_eq!(made["inside"], Json::Null, "the container put round the inner one is in no container: {made}");
    assert_eq!(members(&mut s, outer), [inner]);
    assert_eq!(members(&mut s, inner), [a], "the inner container holds the shape and not the one round it");
    assert_eq!(inside(&mut s, inner), [outer]);
    assert!(inside(&mut s, outer).is_empty(), "the outer container is in none");
    assert_tree(&mut s);
    // Both are fitted again when the shape grows: a loop would leave both as they were.
    let b = boxed(doc(&s), a);
    run(&mut s, "shape.resize", json!({"id": a, "x": b.0, "y": b.1, "width": b.2 - b.0 + 2.0, "height": b.3 - b.1}));
    let (grown, held, round) = (boxed(doc(&s), a), boxed(doc(&s), inner), boxed(doc(&s), outer));
    assert!(held.2 >= grown.2 + 0.25 - 1e-6, "the inner container grew round the shape: {held:?} {grown:?}");
    assert!(round.2 >= held.2 + 0.25 - 1e-6, "the outer container grew round the inner one: {round:?} {held:?}");
    // Attaching the outer container to an edge of the inner one is refused in words.
    let e = error_of(&mut s, "container.attach_edge", &json!({"id": outer, "container": inner, "edge": "left"}));
    assert!(e.to_string().contains("a container is not attached to what it holds"), "{e}");
    // So is putting it in the inner one by Add to Underlying Container.
    let e = error_of(&mut s, "container.add_member", &json!({"ids": [outer], "container": inner}));
    assert!(e.to_string().contains("a container is not put inside what it holds"), "{e}");
    // Undo and redo go through trees only.
    for _ in 0..2 {
        run(&mut s, "edit.undo", json!({}));
        assert_tree(&mut s);
    }
    assert_eq!(members(&mut s, inner), [a]);
    assert!(!page_ids(&s).contains(&outer), "the container put round it is undone");
    for _ in 0..2 {
        run(&mut s, "edit.redo", json!({}));
        assert_tree(&mut s);
    }
    assert_eq!((members(&mut s, outer), members(&mut s, inner)), (vec![inner], vec![a]));
    check_session(&s).unwrap();
}

#[test]
fn a_nest_a_thousand_deep_made_round_the_last_container_each_time_is_a_tree_and_moves_as_one() {
    const DEPTH: usize = 1_000;
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let mut chain = vec![a];
    for _ in 0..DEPTH {
        let last = *chain.last().unwrap();
        chain.push(add_container(&mut s, json!({"ids": [last]})));
    }
    let said = info(&mut s, &chain[1..]);
    assert_eq!(said.len(), DEPTH);
    for (k, c) in said.iter().enumerate() {
        let level = k + 1;
        assert_eq!(id_list(&c["members"]), [chain[level - 1]], "level {level} holds the one inside it and nothing else");
        let want: Vec<u32> = chain.get(level + 1).copied().into_iter().collect();
        assert_eq!(id_list(&c["inside"]), want, "level {level} is in the one round it");
    }
    // The outermost takes every level with it, each once.
    let outermost = chain[DEPTH];
    let was: Vec<f64> = chain.iter().map(|id| num(doc(&s), *id, "PinX")).collect();
    run(&mut s, "shape.move", json!({"ids": [outermost], "dx": 1.0, "dy": 0.0}));
    for (id, x) in chain.iter().zip(&was) {
        assert!((num(doc(&s), *id, "PinX") - (x + 1.0)).abs() < 1e-6, "shape {id} went the distance once");
    }
    check_session(&s).unwrap();
}

// ---- the other ways in ---------------------------------------------------------------------------------------------------------------------------------------------

/// A shape with the box `at` whose `Relationships` holds the shapes `holds` under 1 and names the containers `held_by` under 4, typed a container when `container`.
fn tied(id: u32, at: (f64, f64, f64, f64), holds: &[u32], held_by: &[u32], container: bool) -> Shape {
    let list = |ids: &[u32]| ids.iter().map(|m| format!(",Sheet.{m}!SheetRef()")).collect::<String>();
    let mut calls: Vec<String> = Vec::new();
    if container || !holds.is_empty() {
        calls.push(format!("DEPENDSON(1{})", list(holds)));
    }
    if !held_by.is_empty() {
        calls.push(format!("DEPENDSON(4{})", list(held_by)));
    }
    let mut shape = rect(id, at).build();
    let sheet = Arc::make_mut(&mut shape.sheet);
    if !calls.is_empty() {
        sheet
            .set_cell(&CellAddr::direct("Relationships"), Cell::with_formula(&format!("SUM({})", calls.join(",")), Some(Value::from_text("0"))))
            .unwrap();
    }
    if container {
        sheet
            .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
            .unwrap();
    }
    shape
}

#[test]
fn two_containers_of_one_box_moved_together_do_not_each_join_the_other() {
    // Each is wholly inside the other: the first joins the second, and the second, which now holds the first, joins none.
    let mut s = session_with(vec![tied(1, (0.0, 0.0, 10.0, 10.0), &[], &[], true), tied(2, (0.0, 0.0, 10.0, 10.0), &[], &[], true)]);
    run(&mut s, "shape.move", json!({"ids": [1, 2], "dx": 0.5, "dy": 0.0}));
    assert_tree(&mut s);
    let (one, two) = (inside(&mut s, 1), inside(&mut s, 2));
    assert!(one.is_empty() || two.is_empty(), "one of them is in no container: {one:?} {two:?}");
    check_session(&s).unwrap();
}

#[test]
fn add_to_underlying_container_of_two_containers_over_each_other_puts_neither_in_what_it_holds() {
    // The middle of each is in the other's box: the first goes into the second, and then the second, which holds the first, has no container under it to go into.
    let mut s = session_with(vec![tied(1, (0.0, 0.0, 10.0, 10.0), &[], &[], true), tied(2, (4.0, 0.0, 14.0, 10.0), &[], &[], true)]);
    let before = snapshot(&s);
    let e = error_of(&mut s, "container.add_member", &json!({"ids": [1, 2]}));
    assert!(e.to_string().contains("no container lies under shape 2"), "{e}");
    assert_eq!(snapshot(&s), before, "a refusal leaves the drawing as it was");
    assert_tree(&mut s);
    // One at a time: the first goes in, and the second is then refused the same way.
    run(&mut s, "container.add_member", json!({"ids": [1]}));
    assert_eq!(inside(&mut s, 1), [2]);
    let e = error_of(&mut s, "container.add_member", &json!({"ids": [2]}));
    assert!(e.to_string().contains("no container lies under shape 2"), "{e}");
    let e = error_of(&mut s, "container.add_member", &json!({"ids": [2], "container": 1}));
    assert!(e.to_string().contains("a container is not put inside what it holds"), "{e}");
    assert_tree(&mut s);
}

#[test]
fn a_copy_of_a_nest_is_a_nest_of_its_own_and_a_loop_put_in_by_hand_is_broken_in_the_copies() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let inner = add_container(&mut s, json!({"ids": [a]}));
    let outer = add_container(&mut s, json!({"ids": [inner]}));
    let made = run(&mut s, "shape.duplicate", json!({"ids": [outer], "dx": 20.0, "dy": 0.0}));
    let copy = id_list(&made["ids"])[0];
    assert_tree(&mut s);
    let held = members(&mut s, copy);
    assert_eq!(held.len(), 1, "the copy holds the copy of the inner container");
    assert_eq!(members(&mut s, held[0]).len(), 1, "which holds the copy of the shape");
    assert!(inside(&mut s, copy).is_empty());
    // A loop a library caller wrote by hand, which no command makes: the copies of it are a tree, and the answer says the loop was broken.
    let mut s = session_with(vec![
        tied(1, (0.0, 0.0, 10.0, 10.0), &[2], &[3], true),
        tied(2, (1.0, 1.0, 9.0, 9.0), &[3], &[1], true),
        tied(3, (2.0, 2.0, 8.0, 8.0), &[1], &[2], true),
    ]);
    let made = run(&mut s, "shape.duplicate", json!({"ids": [1, 2, 3], "dx": 30.0, "dy": 0.0}));
    let notes = made["notes"].to_string();
    assert!(notes.contains("made a loop"), "{made}");
    let copies = id_list(&made["ids"]);
    let mut down: HashMap<u32, Vec<u32>> = HashMap::new();
    for c in info(&mut s, &copies) {
        down.insert(u32::try_from(c["id"].as_u64().unwrap()).unwrap(), id_list(&c["members"]));
    }
    let total: usize = down.values().map(Vec::len).sum();
    assert_eq!(total, 2, "one tie of the three that made the loop is taken out of the copies: {down:?}");
}

#[test]
fn a_loop_put_in_by_hand_moves_each_shape_once_and_is_not_fitted() {
    // The walk that follows a move and the fit stay bounded with a loop in the drawing, which no command makes.
    let mut s = session_with(vec![
        tied(1, (0.0, 0.0, 10.0, 10.0), &[2], &[3], true),
        tied(2, (1.0, 1.0, 9.0, 9.0), &[3], &[1], true),
        tied(3, (2.0, 2.0, 8.0, 8.0), &[1], &[2], true),
    ]);
    let fitted = run(&mut s, "container.fit", json!({"ids": [1]}));
    assert_eq!(fitted["resized"], json!([]), "a container in a loop is left alone: {fitted}");
    let was: Vec<f64> = [1, 2, 3].iter().map(|id| num(doc(&s), *id, "PinX")).collect();
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    for (id, x) in [1_u32, 2, 3].iter().zip(&was) {
        assert!((num(doc(&s), *id, "PinX") - (x + 1.0)).abs() < 1e-9, "shape {id} went the distance once");
    }
    // The shape moved held the container it was in, through the others: it leaves it, and the loop is no more.
    assert_tree(&mut s);
}

// ---- a file that holds a loop --------------------------------------------------------------------------------------------------------------------------------------

/// A page where the containers 1 and 2 hold each other, 1 holds the shape 3 as well and names the callout 4, which points at it: the loop is the only thing wrong.
fn two_that_hold_each_other() -> Session {
    let mut one = tied(1, (0.0, 0.0, 10.0, 10.0), &[2, 3], &[2], true);
    let mut text = ties_text(&one);
    text.insert_str(text.len() - 1, ",DEPENDSON(3,Sheet.4!SheetRef())");
    Arc::make_mut(&mut one.sheet).set_cell(&CellAddr::direct("Relationships"), Cell::with_formula(&text, Some(Value::from_text("0")))).unwrap();
    let mut callout = rect(4, (11.0, 11.0, 12.0, 12.0)).build();
    Arc::make_mut(&mut callout.sheet)
        .set_cell(&CellAddr::direct("Relationships"), Cell::with_formula("SUM(DEPENDSON(6,Sheet.1!SheetRef()))", Some(Value::from_text("0"))))
        .unwrap();
    session_with(vec![one, tied(2, (1.0, 1.0, 9.0, 9.0), &[1], &[1], true), tied(3, (2.0, 2.0, 3.0, 3.0), &[], &[1], false), callout])
}

fn ties_text(shape: &Shape) -> String {
    shape.sheet.cell("Relationships").and_then(|c| c.formula.text().map(str::to_owned)).unwrap_or_default()
}

/// Every shape of every page, with its `Relationships` cell taken out: what must come back unchanged.
fn without_ties(d: &Document) -> Vec<Shape> {
    d.pages
        .iter()
        .flat_map(|p| p.shapes.iter())
        .map(|sh| {
            let mut shape = (**sh).clone();
            Arc::make_mut(&mut shape.sheet).cells.remove("Relationships");
            shape
        })
        .collect()
}

#[test]
fn a_file_whose_two_containers_hold_each_other_opens_with_the_loop_broken_in_words_and_nothing_else_changed() {
    for ext in ["vsdx", "diacraft", "odg"] {
        let mut s = two_that_hold_each_other();
        let dir = Dir::new();
        let path = dir.path(&format!("loop.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = Session::new();
        let opened = run(&mut back, "file.open", json!({ "path": path }));
        let warnings = opened["file"]["warnings"].to_string();
        assert!(
            warnings.contains("made a loop") && warnings.contains("shape 1 is no longer a member of container 2"),
            "{ext}: the open says what it broke: {warnings}"
        );
        assert!(opened["file"]["warning_count"].as_u64().unwrap() >= 1);
        assert_tree(&mut back);
        assert_eq!(members(&mut back, 1), [2, 3], "{ext}: the container drawn first holds the other and its shape");
        assert!(members(&mut back, 2).is_empty(), "{ext}");
        assert_eq!((inside(&mut back, 1), inside(&mut back, 2)), (vec![], vec![1]), "{ext}");
        let d = doc(&back);
        assert_eq!(
            ties(d, 1),
            "SUM(DEPENDSON(1,Sheet.2!SheetRef(),Sheet.3!SheetRef()),DEPENDSON(3,Sheet.4!SheetRef()))",
            "{ext}: the tie to the callout is kept in its place"
        );
        assert_eq!(ties(d, 2), "SUM(DEPENDSON(1),DEPENDSON(4,Sheet.1!SheetRef()))", "{ext}: still a container, in the other");
        assert_eq!(ties(d, 3), "SUM(DEPENDSON(4,Sheet.1!SheetRef()))", "{ext}");
        assert_eq!(ties(d, 4), "SUM(DEPENDSON(6,Sheet.1!SheetRef()))", "{ext}");
        assert!(!back.is_dirty(), "{ext}: an open is not an edit");
        // Nothing else changed: the drawing as the reader alone makes it, but for the two cells of the loop.
        if ext == "vsdx" {
            let bytes = std::fs::read(&path).unwrap();
            let (raw, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
            assert_eq!(without_ties(d), without_ties(&raw), "the open changed nothing but the ties of the loop");
            assert_eq!(ties(&raw, 3), ties(d, 3));
            assert_eq!(ties(&raw, 4), ties(d, 4));
        }
        // Undo has nothing to take back to the loop.
        assert!(back.execute("edit.undo", &json!({})).is_err(), "{ext}: the open is no step of the history");
        // An edit and its undo stay trees.
        run(&mut back, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        run(&mut back, "edit.undo", json!({}));
        assert_tree(&mut back);
    }
}

/// The concern 1: a VSDX file whose containers hold each other, opened and **saved in place with no edit**, gets the repaired ties: the file as
/// read is the drawing's baseline, so the splice writes the page the open changed. The reopen finds no loop and says nothing of one, the drawing stays clean, the only part of the
/// drawing written is that page, and a second save writes the same parts.
#[test]
fn a_vsdx_file_whose_containers_hold_each_other_saved_in_place_with_no_edit_keeps_the_repair() {
    let dir = Dir::new();
    let path = dir.path("loop.vsdx");
    run(&mut two_that_hold_each_other(), "file.save_as", json!({ "path": path }));
    let looped = std::fs::read(&path).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    assert!(!s.is_dirty(), "an open is not an edit");
    run(&mut s, "file.save", json!({}));
    assert!(!s.is_dirty());
    let saved = std::fs::read(&path).unwrap();
    let (raw, _) = diacraft_vsdx::read(&saved, &diacraft_vsdx::Limits::default()).unwrap();
    assert_eq!(ties(&raw, 1), ties(doc(&s), 1), "the file holds the repaired ties");
    assert_eq!(ties(&raw, 2), "SUM(DEPENDSON(1),DEPENDSON(4,Sheet.1!SheetRef()))");
    let parts = |bytes: &[u8]| {
        let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
        pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect::<std::collections::BTreeMap<String, Vec<u8>>>()
    };
    let (before, after) = (parts(&looped), parts(&saved));
    let changed: Vec<&String> = after.keys().filter(|name| before.get(*name) != after.get(*name)).collect();
    assert!(
        changed.iter().all(|name| name.starts_with("/visio/pages/page") || name.starts_with("/docProps/")),
        "only the page and the bookkeeping: {changed:?}"
    );
    assert!(changed.iter().any(|name| name.starts_with("/visio/pages/page")), "{changed:?}");
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "no part added or taken out");
    let mut back = Session::new();
    let opened = run(&mut back, "file.open", json!({ "path": path }));
    assert!(!opened["file"]["warnings"].to_string().contains("made a loop"), "the reopen finds no loop: {opened}");
    assert_tree(&mut back);
    run(&mut back, "file.save", json!({}));
    assert_eq!(parts(&std::fs::read(&path).unwrap()), parts(&saved), "a second save writes the same parts");
}

/// A page of `n` containers, each holding the next and named back by it; when `ring`, the last holds the first too.
fn chain_of(n: u32, ring: bool) -> Vec<Shape> {
    (1..=n)
        .map(|id| {
            let next = if id < n {
                Some(id + 1)
            } else if ring {
                Some(1)
            } else {
                None
            };
            let prev = if id > 1 {
                Some(id - 1)
            } else if ring {
                Some(n)
            } else {
                None
            };
            let (x, y) = (f64::from(id % 100) * 2.0, f64::from(id / 100) * 2.0);
            tied(id, (x, y, x + 1.0, y + 1.0), &next.into_iter().collect::<Vec<_>>(), &prev.into_iter().collect::<Vec<_>>(), true)
        })
        .collect()
}

#[test]
fn a_file_with_a_loop_of_ten_thousand_containers_opens_as_a_chain_with_one_tie_taken_out() {
    const N: u32 = 10_000;
    let mut s = session_with(chain_of(N, true));
    let dir = Dir::new();
    let path = dir.path("ring.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = Session::new();
    let opened = run(&mut back, "file.open", json!({ "path": path }));
    let warnings = opened["file"]["warnings"].to_string();
    assert!(warnings.contains(&format!("shape 1 is no longer a member of container {N}")), "{warnings}");
    assert!(warnings.contains("1 tie"), "one tie closed the loop: {warnings}");
    let d = doc(&back);
    assert_eq!(ties(d, N), "SUM(DEPENDSON(1),DEPENDSON(4,Sheet.9999!SheetRef()))");
    assert_eq!(ties(d, 1), "SUM(DEPENDSON(1,Sheet.2!SheetRef()))");
    assert_eq!(ties(d, 5_000), "SUM(DEPENDSON(1,Sheet.5001!SheetRef()),DEPENDSON(4,Sheet.4999!SheetRef()))", "the rest is as the file held it");
    let first = d.find_shape(Owner::Page(1), 1).unwrap();
    assert_eq!(first.path, [0]);
}

/// How long a closure takes, in seconds.
fn seconds(f: impl FnOnce()) -> f64 {
    let t = std::time::Instant::now();
    f();
    t.elapsed().as_secs_f64()
}

#[test]
#[ignore = "a time, against the open of the same containers in a chain and of the same shapes with no ties: run with --ignored"]
fn a_file_with_a_loop_of_ten_thousand_containers_opens_within_a_small_multiple_of_a_plain_one() {
    const N: u32 = 10_000;
    let dir = Dir::new();
    let write = |name: &str, shapes: Vec<Shape>| {
        let path = dir.path(name);
        let mut s = session_with(shapes);
        run(&mut s, "file.save_as", json!({ "path": path }));
        path
    };
    let ring = write("ring.diacraft", chain_of(N, true));
    let chain = write("chain.diacraft", chain_of(N, false));
    let plain = write(
        "plain.diacraft",
        chain_of(N, false)
            .into_iter()
            .map(|mut sh| {
                Arc::make_mut(&mut sh.sheet).cells.remove("Relationships");
                sh
            })
            .collect(),
    );
    let open = |path: &str| {
        seconds(|| {
            let mut s = Session::new();
            run(&mut s, "file.open", json!({ "path": path }));
        })
    };
    // Each twice, the faster kept: the first open of a file pays for the page cache.
    let best = |path: &str| open(path).min(open(path));
    let (ring_s, chain_s, plain_s) = (best(&ring), best(&chain), best(&plain));
    eprintln!("open of {N} containers: in a loop {ring_s:.3} s, in a chain {chain_s:.3} s, with no ties {plain_s:.3} s");
    assert!(ring_s < 1.5 * chain_s + 0.2, "the loop {ring_s} s against the chain {chain_s} s");
    assert!(ring_s < 3.0 * plain_s + 0.5, "the loop {ring_s} s against the same shapes with no ties {plain_s} s");
    let _ = put;
}
