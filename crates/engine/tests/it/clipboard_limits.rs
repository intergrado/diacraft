//! What the clipboard, group and ungroup commands do with hostile input: the most ids one command takes, ids repeated, a page of a hundred
//! thousand shapes, shapes nested to the depth the model allows, `ID`s used up, coordinates past any page, a page with an `ID` twice, and junk
//! parameters with a clipboard to paste. The cost claims are for the fixtures built here and for nothing else.

use std::time::{Duration, Instant};

use diacraft_doc::{Document, MAX_SHAPE_DEPTH};
use diacraft_engine::draw::{MAX_GROUP_DEPTH, MAX_UNGROUP_MEMBERS};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::strategies::junk_values;
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::{blank_session, check_session, run, snapshot};

/// A plain box: a pin, a size one inch each way, and its local pin in the middle.
fn boxed(id: u32, x: f64, y: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &x.to_string())
        .cell("PinY", &y.to_string())
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
}

/// A session whose first page holds `n` boxes, `ID` 1 to `n`, 100 to a row.
fn session_with_boxes(n: u32) -> Session {
    let mut page = PageBuilder::new();
    for i in 0..n {
        page = page.shape(boxed(i + 1, 1.0 + f64::from(i % 100) * 1.5, 1.0 + f64::from(i / 100) * 1.5));
    }
    session_with(DocBuilder::new().page(1, page).build())
}

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// Runs `f`, which is one command, and counts the recalculation it made (`diacraft_calc::work`): the commits and their steps. Counted, not timed:
/// a command that names the most ids a command takes makes one commit at the most, and a time moves with the load on the machine. The one bound
/// on time is a hang guard of a minute, no measure of cost.
fn counted(what: &str, f: impl FnOnce()) -> (u64, u64) {
    diacraft_calc::work::reset();
    let started = Instant::now();
    f();
    let took = started.elapsed();
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("{what}: {commits} commits of {steps} steps");
    assert!(commits <= 1, "{what} made {commits} commits");
    assert!(took < Duration::from_secs(60), "{what} took {took:?}: a hang");
    (commits, steps)
}

#[test]
fn the_most_ids_a_command_takes_on_a_page_of_that_many_shapes_is_bounded() {
    // 4,096 ids (the cap) on a page of 4,100 shapes: copy, paste, duplicate, group, ungroup and cut, each in one command.
    let mut s = session_with_boxes(4_100);
    let ids: Vec<u32> = (1..=4_096).collect();
    counted("copying 4,096 shapes of 4,100", || {
        run(&mut s, "edit.copy", json!({"ids": ids}));
    });
    assert_eq!(s.clipboard().unwrap().len(), 4_096);
    counted("pasting 4,096 shapes on a page of 4,100", || {
        run(&mut s, "edit.paste", json!({}));
    });
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 4_100 + 4_096);
    counted("duplicating 4,096 shapes", || {
        run(&mut s, "shape.duplicate", json!({"ids": ids}));
    });
    let mut group = Value::Null;
    counted("grouping 4,096 shapes", || {
        group = run(&mut s, "shape.group", json!({"ids": ids}));
    });
    assert_eq!(group["members"], 4_096);
    let g = id_of(&group);
    counted("ungrouping a group of 4,096 shapes", || {
        run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    });
    counted("cutting 4,096 shapes", || {
        run(&mut s, "edit.cut", json!({"ids": ids}));
    });
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn ids_asked_for_again_and_again_cost_what_the_shapes_they_name_cost() {
    let mut s = session_with_boxes(200);
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": (1..=150).collect::<Vec<u32>>()})));
    // 4,096 times the id of a group of 150 shapes: one group, copied once.
    let again = vec![g; 4_096];
    counted("copying one group named 4,096 times", || {
        run(&mut s, "edit.copy", json!({"ids": again}));
    });
    assert_eq!((s.clipboard().unwrap().len(), s.clipboard().unwrap().shape_count()), (1, 151));
    counted("duplicating it", || {
        run(&mut s, "shape.duplicate", json!({"ids": again}));
    });
    // Named with its members: still the one group.
    let mut both: Vec<u32> = (1..=150).collect();
    both.extend(vec![g; 3_000]);
    run(&mut s, "edit.copy", json!({"ids": both}));
    assert_eq!(s.clipboard().unwrap().len(), 1);
    // Two shapes named 4,096 times between them are two shapes.
    let twice: Vec<u32> = (0..4_096).map(|i| 151 + (i % 2)).collect();
    let made = run(&mut s, "shape.group", json!({"ids": twice}));
    assert_eq!(made["members"], 2);
    counted("ungrouping one group named 4,096 times", || {
        run(&mut s, "shape.ungroup", json!({"ids": vec![id_of(&made); 4_096]}));
    });
}

#[test]
fn a_page_of_a_hundred_thousand_shapes_is_walked_once_for_all_the_ids_a_command_names() {
    // 100,000 shapes and 4,096 ids spread over them: a search for each id from the top of the page is about 2e8 visits; one walk is 1e5.
    let mut s = session_with_boxes(100_000);
    let ids: Vec<u32> = (0..4_096).map(|i| 1 + i * 24).collect();
    let copy = counted("copying 4,096 shapes spread over a page of 100,000", || {
        run(&mut s, "edit.copy", json!({"ids": ids}));
    });
    let paste = counted("pasting them", || {
        run(&mut s, "edit.paste", json!({}));
    });
    let group = counted("grouping 4,096 shapes spread over a page of 100,000", || {
        run(&mut s, "shape.group", json!({"ids": ids}));
    });
    eprintln!("on a page of 100,000 boxes with no connector: copy {copy:?}, paste {paste:?}, group {group:?}");
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 100_000 + 4_096 - 4_096 + 1);
}

#[test]
fn a_copy_of_more_shapes_than_the_cap_is_refused_before_it_is_made() {
    // A group of 50,001 boxes: its copy is one shape with 50,001 inside it.
    let mut group = ShapeBuilder::new(1).group().cell("PinX", "5").cell("PinY", "5").cell("Width", "10").cell("Height", "10");
    for i in 0..50_000_u32 {
        group = group.child(boxed(i + 2, 1.0, 1.0));
    }
    let mut s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(group)).build());
    let before = snapshot(&s);
    let e = s.execute("edit.copy", &json!({"ids": [1]})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("at most 50000"), "{e}");
    assert_eq!(snapshot(&s), before);
    assert!(s.clipboard().is_none());
    // A group of exactly the cap (the group and 49,999 members) is copied and pasted.
    let mut group = ShapeBuilder::new(1).group().cell("PinX", "5").cell("PinY", "5").cell("Width", "10").cell("Height", "10");
    for i in 0..49_999_u32 {
        group = group.child(boxed(i + 2, 1.0, 1.0));
    }
    let mut s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(group)).build());
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    counted("pasting a group of 50,000 shapes", || {
        run(&mut s, "edit.paste", json!({}));
    });
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), 2);
}

/// `depth` boxes nested one in the next, `ID` 1 outermost.
fn nested(depth: usize) -> Document {
    let mut shape = boxed(u32::try_from(depth).unwrap(), 1.0, 1.0);
    for id in (1..depth).rev() {
        shape = boxed(u32::try_from(id).unwrap(), 1.0, 1.0).group().child(shape);
    }
    DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(boxed(1_000, 8.0, 8.0))).build()
}

#[test]
fn shapes_nested_as_deep_as_the_model_allows_are_copied_pasted_and_taken_apart_and_no_group_goes_deeper() {
    let deepest = MAX_SHAPE_DEPTH;
    let mut s = session_with(nested(deepest));
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    assert_eq!(s.clipboard().unwrap().shape_count(), deepest);
    let pasted = run(&mut s, "edit.paste", json!({}));
    assert_eq!(pasted["created"], deepest);
    run(&mut s, "shape.duplicate", json!({"ids": [1]}));
    // The outermost group holds the next one: taking it apart is a step up the chain.
    run(&mut s, "shape.ungroup", json!({"ids": [1]}));
    assert_eq!(doc(&s).page(1).unwrap().shapes.iter().next().unwrap().id, 2);
    // A group goes no deeper than the reader of a file takes (90 levels, MAX_GROUP_DEPTH): one more level is refused, one fewer fits.
    let mut s = session_with(nested(MAX_GROUP_DEPTH));
    let before = snapshot(&s);
    let e = s.execute("shape.group", &json!({"ids": [1, 1_000]})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("too deeply"), "{e}");
    assert_eq!(snapshot(&s), before);
    let mut s = session_with(nested(MAX_GROUP_DEPTH - 1));
    run(&mut s, "shape.group", json!({"ids": [1, 1_000]}));
    assert_eq!(check_session(&s), Ok(()));
    // A drawing grouped to that depth is saved and read again.
    let dir = crate::edit_foreign::Dir::new();
    let path = dir.path("deep.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(doc(&t).page(1).unwrap().shapes.len(), doc(&s).page(1).unwrap().shapes.len());
    // A member 40 levels down copied alone is lifted out of 39 groups at once.
    let mut s = session_with(nested(40));
    run(&mut s, "edit.copy", json!({"ids": [40]}));
    let alone = run(&mut s, "edit.paste", json!({}));
    assert_eq!(alone["created"], 1);
}

#[test]
fn ids_that_are_used_up_are_an_error_and_not_a_wrap() {
    let near_the_end = DocBuilder::new()
        .page(1, PageBuilder::new().shape(boxed(1, 2.0, 2.0)).shape(boxed(2, 5.0, 2.0)).shape(boxed(u32::MAX - 1, 8.0, 2.0)))
        .build();
    let mut s = session_with(near_the_end);
    // One more `ID` is left: a group takes it.
    let g = run(&mut s, "shape.group", json!({"ids": [1, 2]}));
    assert_eq!(g["id"], u32::MAX);
    // None is left: a second group, a paste and a duplicate are refused with the document as it was.
    let before = snapshot(&s);
    run(&mut s, "edit.copy", json!({"ids": [u32::MAX - 1]}));
    for (command, params) in
        [("shape.group", json!({"ids": [u32::MAX, u32::MAX - 1]})), ("edit.paste", json!({})), ("shape.duplicate", json!({"ids": [u32::MAX - 1]}))]
    {
        let e = s.execute(command, &params).unwrap_err();
        assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("no shape ids are left"), "{command}: {e}");
    }
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn coordinates_past_any_page_are_refused_and_not_carried_into_arithmetic() {
    let far = DocBuilder::new().page(1, PageBuilder::new().shape(boxed(1, 1.0e308, 0.0)).shape(boxed(2, -1.0e308, 0.0))).build();
    let mut s = session_with(far);
    let before = snapshot(&s);
    for (command, params) in [("shape.group", json!({"ids": [1, 2]})), ("shape.duplicate", json!({"ids": [1], "dx": 1.0e308, "dy": 0.0}))] {
        let e = s.execute(command, &params).unwrap_err();
        assert!(!matches!(e, EngineError::Internal { .. }), "{command}: {e}");
    }
    assert_eq!(snapshot(&s), before);
    assert_eq!(s.panics_caught(), 0);
}

#[test]
fn a_page_that_holds_an_id_twice_is_copied_and_grouped_by_the_first_shape_with_it() {
    let twice = DocBuilder::new()
        .page(1, PageBuilder::new().shape(boxed(1, 2.0, 2.0)).shape(boxed(1, 6.0, 2.0)).shape(boxed(2, 9.0, 2.0)).shape(boxed(3, 12.0, 2.0)))
        .build();
    let mut s = session_with(twice);
    run(&mut s, "edit.copy", json!({"ids": [1, 2]}));
    let ids = run(&mut s, "edit.paste", json!({}));
    assert_eq!(ids["ids"], json!([4, 5]), "above the highest id of the page");
    let g = run(&mut s, "shape.group", json!({"ids": [1, 3]}));
    assert_eq!(g["members"], 2);
    assert_eq!(check_session(&s), Ok(()));
    // The group holds the first shape with the id 1; the second stays on the page.
    let page = doc(&s).page(1).unwrap();
    assert!(page.shapes.iter().any(|shape| shape.id == 1), "the other shape with the id is still on the page");
}

#[test]
fn hostile_parameters_with_a_clipboard_to_paste_change_nothing_and_never_panic() {
    let mut s = blank_session();
    for (x, y) in [(2.0, 2.0), (6.0, 2.0)] {
        run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": y}));
    }
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    run(&mut s, "edit.copy", json!({"ids": [1, 2, 3]}));
    let held = s.clipboard().unwrap().pastes();
    let mut calls = 0;
    for command in ["edit.copy", "edit.cut", "edit.paste", "shape.duplicate", "shape.group", "shape.ungroup"] {
        let keys = diacraft_engine::param_keys(diacraft_engine::find_command(command).unwrap().params).unwrap();
        for key in &keys {
            for junk in junk_values() {
                let mut params = serde_json::Map::new();
                // The other keys are right, so that the junk key is what is wrong.
                match command {
                    "edit.paste" => {}
                    _ => {
                        params.insert("ids".to_owned(), json!([1, 2]));
                    }
                }
                params.insert((*key).to_owned(), junk.clone());
                calls += 1;
                let before = snapshot(&s);
                let outcome = s.execute(command, &Value::Object(params.clone()));
                assert!(!matches!(outcome, Err(EngineError::Internal { .. })), "{command} {params:?}: {outcome:?}");
                if outcome.is_err() {
                    assert_eq!(snapshot(&s), before, "{command} {params:?} failed but changed the session");
                    assert_eq!(s.clipboard().unwrap().pastes(), held, "{command} {params:?}: a failed command moved the clipboard");
                } else {
                    // A command that worked: put the drawing back for the next.
                    while s.history().is_some_and(|h| h.undo_len() > 3) {
                        run(&mut s, "edit.undo", json!({}));
                    }
                    run(&mut s, "edit.copy", json!({"ids": [1, 2, 3]}));
                }
                assert_eq!(s.panics_caught(), 0, "{command} {params:?}");
                assert_eq!(check_session(&s), Ok(()), "{command} {params:?}");
            }
        }
    }
    assert!(calls > 100, "only {calls} calls");
}

/// A page holding one group, `ID` 1, of `members` boxes, with its pin away from its local pin: every member is written when it is taken apart.
fn session_with_a_group_of(members: u32) -> Session {
    let mut group = ShapeBuilder::new(1)
        .group()
        .cell("PinX", "50")
        .cell("PinY", "50")
        .cell("Width", "100")
        .cell("Height", "100")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0");
    for i in 0..members {
        group = group.child(boxed(i + 2, 1.0 + f64::from(i % 100), 1.0 + f64::from(i / 100)));
    }
    session_with(DocBuilder::new().page(1, PageBuilder::new().shape(group)).build())
}

#[test]
fn the_ungroup_of_one_group_of_many_members_costs_in_proportion_to_its_members() {
    // One id may name a group of any number of members: ids are capped, members are not, so the work of an ungroup must not grow with the square of
    // them. Counted, not timed (a ratio of two timings failed this test under load): the steps of the recalculation (`diacraft_calc::work`), which
    // for four times the members are about four times the steps (16 if the work were quadratic); the ceiling is 5. The count of the ungroup's own
    // steps is in `draw/group.rs`.
    let steps_of_ungrouping = |members: u32| {
        let mut s = session_with_a_group_of(members);
        let mut r = Value::Null;
        let (commits, steps) = counted("ungrouping a group", || r = run(&mut s, "shape.ungroup", json!({"ids": [1]})));
        assert_eq!(r["ids"].as_array().unwrap().len(), usize::try_from(members).unwrap());
        assert_eq!(commits, 1, "one commit for the members");
        steps
    };
    let (small, large) = (steps_of_ungrouping(10_000), steps_of_ungrouping(40_000));
    let ratio = large as f64 / small as f64;
    eprintln!("ungrouping a group of 10,000 members: {small} steps; of 40,000: {large}; a ratio of {ratio:.2} for four times the members");
    assert!(small > 0, "the count counts");
    // And a step count for each member, not only a ratio (nineteen measured, for each member: the cells it writes and the cells that read them).
    assert!(small <= 25 * 10_000 && large <= 25 * 40_000, "{small} steps for 10,000 members and {large} for 40,000");
    assert!(ratio < 5.0, "ungrouping 10,000 members took {small} steps and 40,000 took {large}: a ratio of {ratio:.2}");
}

#[test]
fn an_ungroup_takes_apart_no_more_members_than_the_cap_counted_over_every_group_named() {
    // The cap counts members, not ids: one id naming a group over it is refused, and so are two groups that are each under it and together over.
    let cap = u32::try_from(MAX_UNGROUP_MEMBERS).unwrap();
    let mut s = session_with_a_group_of(cap + 1);
    let before = snapshot(&s);
    let e = s.execute("shape.ungroup", &json!({"ids": [1]})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("at most 50000 members"), "{e}");
    assert_eq!(snapshot(&s), before);
    let mut two = PageBuilder::new();
    for id in [1_u32, 2] {
        let mut group = ShapeBuilder::new(id).group().cell("PinX", "5").cell("PinY", "5").cell("Width", "10").cell("Height", "10");
        for i in 0..(cap / 2 + 100) {
            group = group.child(boxed(10 + id * 100_000 + i, 1.0, 1.0));
        }
        two = two.shape(group);
    }
    let mut s = session_with(DocBuilder::new().page(1, two).build());
    let before = snapshot(&s);
    let e = s.execute("shape.ungroup", &json!({"ids": [1, 2]})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("these groups hold"), "{e}");
    assert_eq!(snapshot(&s), before);
    // Each alone is under it and is taken apart.
    let r = run(&mut s, "shape.ungroup", json!({"ids": [1]}));
    assert_eq!(r["ungrouped"], 1);
    // At the cap exactly, one group is taken apart.
    let mut s = session_with_a_group_of(cap);
    counted("ungrouping a group of 50,000 members", || {
        run(&mut s, "shape.ungroup", json!({"ids": [1]}));
    });
    assert_eq!(doc(&s).page(1).unwrap().shapes.len(), usize::try_from(cap).unwrap());
}

/// A page holding a group (`ID` 1) of two boxes and `n` connectors with plain ends, each glued at its begin to the whole of the group by a `Connect` row.
fn session_with_connectors_glued_to_a_group(n: u32) -> Session {
    let mut page = PageBuilder::new().shape(
        ShapeBuilder::new(1)
            .group()
            .cell("PinX", "5")
            .cell("PinY", "5")
            .cell("Width", "4")
            .cell("Height", "2")
            .cell("LocPinX", "2")
            .cell("LocPinY", "1")
            .child(boxed(2, 1.0, 1.0))
            .child(boxed(3, 3.0, 1.0)),
    );
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(i + 10)
                .cell("BeginX", "1")
                .cell("BeginY", "2")
                .cell("EndX", "8")
                .cell("EndY", "9")
                .cell("PinX", "4")
                .cell("PinY", "5")
                .cell("Width", "9")
                .cell("Height", "0"),
        );
    }
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    s.edit("glue", |d| {
        let rows: Vec<diacraft_doc::Connect> = (0..n)
            .map(|i| diacraft_doc::Connect {
                from_sheet: i + 10,
                from_cell: Some(std::sync::Arc::from("BeginX")),
                from_part: Some(9),
                to_sheet: 1,
                to_cell: Some(std::sync::Arc::from("PinX")),
                to_part: Some(3),
                ..diacraft_doc::Connect::default()
            })
            .collect();
        if let Some(page) = d.pages.get_mut(0) {
            std::sync::Arc::make_mut(page).connects = diacraft_doc::Connects::from_vec(rows);
        }
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn the_ungroup_of_a_group_that_very_many_connectors_are_glued_to_costs_in_proportion_to_them() {
    // Each connector glued to the group is made a plain point where it was. Finding them was a search of the page from the top for each `Connect` row,
    // about the product of the two; it is one walk of the page now (`scan::scan`). Counted, not timed: the steps of the recalculation for four times
    // the connectors are about four times the steps (16 if it were the product); the ceiling is 5. The count of the searches is in `draw/shapes.rs`.
    let steps_of_ungrouping = |n: u32| {
        let mut s = session_with_connectors_glued_to_a_group(n);
        let mut r = Value::Null;
        let (_, steps) = counted("ungrouping a group with connectors glued to it", || r = run(&mut s, "shape.ungroup", json!({"ids": [1]})));
        assert_eq!(r["ids"], json!([2, 3]));
        assert!(doc(&s).page(1).unwrap().connects.is_empty());
        steps
    };
    let (small, large) = (steps_of_ungrouping(5_000), steps_of_ungrouping(20_000));
    let ratio = large as f64 / small.max(1) as f64;
    eprintln!(
        "ungrouping a group with 5,000 connectors glued to it: {small} steps; with 20,000: {large}; a ratio of {ratio:.2} for four times the connectors"
    );
    // And a step count for each connector, not only a ratio (sixteen measured).
    assert!(small <= 20 * 5_000 && large <= 20 * 20_000, "{small} steps for 5,000 connectors and {large} for 20,000");
    assert!(ratio < 5.0, "{small} steps and {large}: a ratio of {ratio:.2}");
    // The delete of a shape that leaves the connectors glued to it (`connectors: false`) still goes through `settle`, which looks each connector up from the top of the
    // page for its route (not changed here): counted, not held to a ratio. By default the delete takes the connectors along and takes at most 4,096 of them, so that a
    // hub of 5,000 is refused in words (`draw/shapes.rs`).
    for n in [5_000, 10_000] {
        let mut s = session_with_connectors_glued_to_a_group(n);
        let refused = s.execute("shape.delete", &json!({"ids": [1]})).unwrap_err().to_string();
        assert!(refused.contains("connectors: false"), "{refused}");
        let (commits, steps) = counted("deleting a group with connectors glued to it", || {
            run(&mut s, "shape.delete", json!({"ids": [1], "connectors": false}));
        });
        eprintln!("deleting a group with {n} connectors glued to it: {commits} commits of {steps} steps");
    }
    // The most the default takes along, 4,096: the connectors go with the group in one commit, and the page holds nothing; `counted` is the hang guard.
    let mut s = session_with_connectors_glued_to_a_group(4_096);
    let mut answer = Value::Null;
    let (commits, steps) = counted("deleting a group with 4,096 connectors glued to it, which go with it", || {
        answer = run(&mut s, "shape.delete", json!({"ids": [1]}));
    });
    eprintln!("deleting a group with 4,096 connectors glued to it, taken along: {commits} commits of {steps} steps");
    assert_eq!(answer["connectors_deleted"].as_array().map(Vec::len), Some(4_096), "every connector goes");
    assert_eq!(answer["deleted"], 1 + 4_096, "the group (its members go with it, uncounted) and the connectors");
    assert!(doc(&s).page(1).unwrap().shapes.is_empty() && doc(&s).page(1).unwrap().connects.is_empty());
}
