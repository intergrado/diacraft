//! What the Size & Position commands cost: counted, not timed, as `edit_cost.rs` counts a move. A time moves with the load on the
//! machine; the commits an edit makes and the steps they take do not (`diacraft_calc::work`).
//!
//! The recalculation of the whole page is what an edit cost before it worked out only what it changed, so its steps are the number each command is
//! set against: a command works out the cells it wrote and what reads them, and nothing else. The finding of the shapes (one walk of the
//! page, no search for each) is counted by the unit tests of `draw/placement.rs`, which read the walk counters of the crate.

use diacraft_engine::draw::drop_shape;
use diacraft_engine::stencil::find_master;
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::edit_cost::{big_page, recalculation_of, steps_of_the_whole_page};
use crate::{blank_session, check_session, run};

#[test]
fn a_pin_a_size_and_a_line_end_on_a_page_of_500_shapes_and_300_connectors_work_out_what_they_changed_in_one_commit() {
    let mut s = big_page();
    let whole = steps_of_the_whole_page(&s);
    // The first four rows of rectangles, 100 of them, pinned at the top left: nothing is routed again, the shapes draw where they did.
    let hundred: Vec<u32> = (1..=100).collect();
    let (commits, steps) = recalculation_of(&mut s, "shape.pin", &json!({"ids": hundred, "at": "top-left"}));
    eprintln!("a pin of 100 shapes on 500 shapes and 300 connectors: {commits} commits of {steps} steps; the whole page is {whole} steps");
    assert_eq!(commits, 1, "one commit for the 100 shapes");
    // Measured: a third of the page, of which about 44,000 steps are the fixed cost of any edit of a page this size (a line end costs that alone) and the
    // rest the connectors glued to the hundred, whose glue formulas read the pins that were written.
    assert!(steps * 2 < whole, "{steps} steps against {whole} for the whole page");
    // One shape that has a connector on each side, sized: its connectors follow, and the commit works out those and no others.
    let one = 5 * crate::edit_cost::COLUMNS + 4 + 1;
    let mut worst = 0;
    for width in [2.0, 1.5, 2.5] {
        let (commits, steps) = recalculation_of(&mut s, "shape.size", &json!({"ids": [one], "width": width}));
        eprintln!("one shape sized on 500 shapes and 300 connectors: {commits} commits of {steps} steps");
        assert_eq!(commits, 1, "one commit");
        worst = worst.max(steps);
    }
    assert!(worst * 3 < whole, "{worst} steps against {whole} for the whole page");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_end_and_a_length_of_a_line_on_a_page_of_500_shapes_and_300_connectors_are_one_commit_each() {
    let mut s = big_page();
    let whole = steps_of_the_whole_page(&s);
    let line = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 19.0}, "to": {"x": 4.0, "y": 19.0}}))["id"].as_u64().unwrap();
    let (commits, steps) = recalculation_of(&mut s, "shape.set_end", &json!({"id": line, "end": "end", "x": 6.0, "y": 19.5}));
    eprintln!("a line end on 500 shapes and 300 connectors: {commits} commits of {steps} steps; the whole page is {whole} steps");
    assert!(commits <= 2, "{commits} commits");
    assert!(steps * 3 < whole, "{steps} steps against {whole}");
    let (commits, steps) = recalculation_of(&mut s, "shape.set_length", &json!({"id": line, "length": 7.0}));
    eprintln!("a line length on 500 shapes and 300 connectors: {commits} commits of {steps} steps");
    assert!(commits <= 2, "{commits} commits");
    assert!(steps * 3 < whole, "{steps} steps against {whole}");
}

/// A page of `n` rectangles, made in one edit.
fn page_of_rectangles(n: u32) -> Session {
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    s.edit("build", |doc| {
        for i in 0..n {
            drop_shape(doc, 1, rectangle, 1.0 + f64::from(i % 80) * 2.0, 1.0 + f64::from(i / 80) * 2.0)
                .map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
        }
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn the_most_ids_one_command_takes_on_a_page_of_that_many_shapes_is_one_commit_and_work_in_step_with_the_shapes_named() {
    // The hostile size: 4,096 ids (the cap of every command that takes ids) on a page of 4,100 shapes.
    let mut s = page_of_rectangles(4_100);
    let whole = steps_of_the_whole_page(&s);
    let ids: Vec<u32> = (1..=4_096).collect();
    let (commits, steps) = recalculation_of(&mut s, "shape.pin", &json!({"ids": ids, "at": "bottom-right"}));
    eprintln!("a pin of 4,096 shapes on a page of 4,100: {commits} commits of {steps} steps; the whole page is {whole} steps");
    assert_eq!(commits, 1, "one commit for the 4,096 ids");
    assert!(steps < whole, "{steps} steps for the pin of 4,096 shapes, {whole} for the whole page");
    // A size changes what every formula of a shape reads (its pin, its connection points, its geometry, its text block), so 4,096 shapes sized cost
    // about what the whole page does, and the claim is not "under the page" but "in step with the shapes sized": the steps for one shape, for 64 and
    // for the rest are a fixed cost and so much for each shape, and no more (a command that looked each shape up in the others, or recalculated for
    // each, would show a term in the square of the number named).
    let steps_for = |s: &mut Session, from: u32, to: u32| {
        let named: Vec<u32> = (from..=to).collect();
        let (commits, steps) = recalculation_of(s, "shape.size", &json!({"ids": named, "width": 1.0, "height": 0.5}));
        eprintln!("a size of {} shapes on a page of 4,100: {commits} commits of {steps} steps; the whole page is {whole} steps", to - from + 1);
        assert_eq!(commits, 1, "one commit for the {} ids", to - from + 1);
        steps
    };
    let (one, sixty_four) = (steps_for(&mut s, 1, 1), steps_for(&mut s, 2, 65));
    let each = (sixty_four - one) as f64 / 63.0;
    let fixed = one as f64 - each;
    let rest = steps_for(&mut s, 66, 4_100);
    let wanted = fixed + each * 4_035.0;
    eprintln!("a fixed {fixed:.0} steps and {each:.1} for each shape sized: {wanted:.0} expected for 4,035, {rest} counted");
    assert!(
        rest as f64 <= wanted * 1.15,
        "{rest} steps for 4,035 shapes where {wanted:.0} are expected from one and 64: the work grows faster than the shapes"
    );
    // The read of them is a query: it works out nothing.
    diacraft_calc::work::reset();
    let r = run(&mut s, "shape.placement", json!({"ids": ids}));
    assert_eq!(r["shapes"].as_array().unwrap().len(), 4_096);
    assert_eq!((diacraft_calc::work::commits(), diacraft_calc::work::steps()), (0, 0), "a query recalculates nothing");
    assert_eq!(check_session(&s), Ok(()));
}
