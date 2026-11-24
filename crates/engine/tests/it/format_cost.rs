//! What the format commands cost on a large page: a page of 500 shapes and 300 connectors, each shape with a label.
//!
//! The cost of a command is counted, not timed (`diacraft_calc::work`: the commits it makes and the steps they take, which do not move with the
//! load on the machine): the assertions stop a command from silently becoming quadratic or from working out the page. **What the fixture
//! lacks** (so the numbers are not claimed for it): it is the page of `edit_cost.rs` with a label of one run on each shape. Every shape is an instance of one master (a rectangle of the built-in stencil) and holds only its pin; there are
//! no groups, no fields, no second run, no theme part, no styles beyond the root, and no formula that reads a format cell. A file whose formulas
//! read `FillForegnd` or `Size` (a line colour that is a shade of the fill, for one) works out those cells as well, and only those.

use diacraft_engine::draw::{End, attach_connector, drop_shape, set_text, settle};
use diacraft_engine::stencil::find_master;
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::{blank_session, check_session};

const COLUMNS: u32 = 25;
const ROWS: u32 = 20;
const CONNECTORS: usize = 300;

/// 500 rectangles with a label each, joined left to right by 300 glued connectors, made in one edit.
fn big_page() -> (Session, Vec<u32>) {
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    let mut ids = Vec::new();
    s.edit("build", |doc| {
        let fail = |p: diacraft_engine::draw::Problem| EngineError::Failed { cmd: "test".into(), msg: p.message };
        let before = doc.clone();
        for r in 0..ROWS {
            for c in 0..COLUMNS {
                let info = drop_shape(doc, 1, rectangle, 1.5 + 3.0 * f64::from(c), 1.0 + 2.0 * f64::from(r)).map_err(fail)?;
                set_text(doc, 1, info.id, &format!("Step {}", ids.len() + 1)).map_err(fail)?;
                ids.push(info.id);
            }
        }
        let mut connectors = Vec::new();
        'rows: for r in 0..ROWS {
            for c in 0..COLUMNS - 1 {
                if connectors.len() == CONNECTORS {
                    break 'rows;
                }
                let (a, b) = (ids[(r * COLUMNS + c) as usize], ids[(r * COLUMNS + c + 1) as usize]);
                connectors.push(attach_connector(doc, 1, &End::Shape { id: a, point: None }, &End::Shape { id: b, point: None }).map_err(fail)?.id);
            }
        }
        settle(doc, &before, 1, &[], &connectors).map_err(fail)
    })
    .unwrap();
    (s, ids)
}

/// The recalculation a command made: its commits and their steps (`diacraft_calc::work`).
fn recalculation_of(s: &mut Session, command: &str, params: &serde_json::Value) -> (u64, u64) {
    diacraft_calc::work::reset();
    s.execute(command, params).unwrap();
    (diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

#[test]
fn the_format_commands_on_a_page_of_500_shapes_and_300_connectors() {
    let (mut s, ids) = big_page();
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.len(), 500 + CONNECTORS);
    // The fifth shape of the sixth row has a connector on each side.
    let one = ids[(5 * COLUMNS + 4) as usize];
    let colour = |round: u32| if round.is_multiple_of(2) { "#3C78D8" } else { "#D83C78" };

    // What a format used to cost if it had worked out the page: every formula of the page, in steps, on the same page.
    let mut copy = s.document().unwrap().clone();
    let report =
        diacraft_calc::commit_in(&mut copy, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default(), diacraft_calc::Scope::Page(1));
    let whole = report.stats.steps;

    // The worst of the rounds of each command: (commits, steps).
    let worst = |s: &mut Session, rounds: u32, command: &str, params: &dyn Fn(u32) -> serde_json::Value| -> (u64, u64) {
        (0..rounds).map(|round| recalculation_of(s, command, &params(round))).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)))
    };
    let shape =
        worst(&mut s, 7, "shape.format", &|round| json!({"ids": [one], "fill": colour(round), "line": "#7F6000", "weight": 2.25, "end_arrow": 13}));
    let all_shapes = worst(&mut s, 5, "shape.format", &|round| json!({"ids": ids, "fill": colour(round), "weight": 0.75 + f64::from(round % 2)}));
    let whole_text = worst(
        &mut s,
        7,
        "text.format",
        &|round| json!({"ids": [one], "size": 11 - round % 2, "bold": round.is_multiple_of(2), "color": colour(round)}),
    );
    let all_text =
        worst(&mut s, 5, "text.format", &|round| json!({"ids": ids, "size": 12 - round % 2, "italic": round.is_multiple_of(2), "align": "left"}));
    let range = worst(&mut s, 7, "text.format", &|round| {
        // The range goes over and over the same label: each call splits a run of the label, and the next finds more runs to cut.
        json!({"ids": [one], "start": 1, "end": 2 + round as usize % 3, "underline": round.is_multiple_of(2)})
    });
    let looked = worst(&mut s, 7, "shape.formatting", &|_| json!({"ids": [one]}));
    let looked_all = worst(&mut s, 5, "shape.formatting", &|_| json!({"ids": ids}));
    eprintln!(
        "on 500 shapes and {CONNECTORS} connectors, 500 labels (commits, steps; the worst of the rounds): shape.format of one shape {shape:?}; of all \
         500 {all_shapes:?}; text.format of one shape {whole_text:?}; of all 500 {all_text:?}; of a range {range:?}; shape.formatting of one \
         {looked:?}; of all 500 {looked_all:?}; the recalculation of the whole page {whole} steps"
    );
    // One commit at the most for each command, whatever it names; a read makes none.
    for (what, (commits, _)) in [
        ("a format of one shape", shape),
        ("a format of 500 shapes", all_shapes),
        ("a text format of one shape", whole_text),
        ("a text format of 500 shapes", all_text),
        ("a text format of a range", range),
        ("the formatting of one shape", looked),
        ("the formatting of 500 shapes", looked_all),
    ] {
        assert!(commits <= 1, "{what} made {commits} commits");
    }
    assert_eq!(looked, (0, 0), "a read works nothing out");
    assert_eq!(looked_all, (0, 0), "a read of 500 shapes works nothing out");
    // The point: one shape costs less than working out the page, and so do all 500 (a format of a shape's look reaches the cells that
    // read what it wrote, and no others).
    assert!(shape.1 < whole, "a format of one shape took {} steps, and working out the whole page {whole}", shape.1);
    assert!(
        whole_text.1 < whole && range.1 < whole,
        "a text format of one shape took {} steps and of a range {}, the page {whole}",
        whole_text.1,
        range.1
    );
    assert!(
        all_shapes.1 < whole && all_text.1 < whole,
        "a format of 500 shapes took {} steps and of their text {}, the page {whole}",
        all_shapes.1,
        all_text.1
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_most_shapes_one_command_names_are_formatted_in_a_bounded_time() {
    // The hostile size: 4,096 ids (the cap) on a page of 4,100 shapes. Counted, not timed: one commit for each command at the most, and no more
    // steps than the page has.
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    s.edit("build", |doc| {
        for i in 0..4_100_u32 {
            let info = drop_shape(doc, 1, rectangle, 1.0 + f64::from(i % 80) * 2.0, 1.0 + f64::from(i / 80) * 2.0)
                .map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
            set_text(doc, 1, info.id, "x").map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
        }
        Ok(())
    })
    .unwrap();
    let mut copy = s.document().unwrap().clone();
    let whole =
        diacraft_calc::commit_in(&mut copy, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default(), diacraft_calc::Scope::Page(1))
            .stats
            .steps;
    let ids: Vec<u32> = (1..=4_096).collect();
    for (what, command, params) in [
        ("formatting 4,096 shapes", "shape.format", json!({"ids": ids, "fill": "#3C78D8", "line": "#7F6000", "weight": 1.5})),
        (
            "formatting the text of 4,096 shapes",
            "text.format",
            json!({"ids": ids, "font": "Liberation Sans", "size": 10, "bold": true, "align": "right"}),
        ),
        ("reading the formatting of 4,096 shapes", "shape.formatting", json!({"ids": ids})),
    ] {
        let (commits, steps) = recalculation_of(&mut s, command, &params);
        eprintln!("{what} on a page of 4,100: {commits} commits of {steps} steps; the whole page is {whole} steps");
        assert!(commits <= 1, "{what} made {commits} commits");
        assert!(steps < whole, "{what} took {steps} steps, the whole page {whole}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
