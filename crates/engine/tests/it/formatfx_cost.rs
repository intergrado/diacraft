//! What the commands cost on a large page: counted, not timed (`diacraft_calc::work`: the commits a command makes and the steps they take, which
//! do not move with the load on the machine), and the refusals that bound them. The page is 4,100 rectangles of one master, made in one edit, on a page that has
//! a theme.
//!
//! **What the fixture lacks** (so the numbers are not claimed for it): every shape holds only its pin, follows the `Theme` style for all three families, and has
//! no text, no group, no connector and no formula that reads a format cell.

use diacraft_engine::draw::drop_shape;
use diacraft_engine::stencil::find_master;
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const SHAPES: u32 = 4_100;

/// A page with Tidepool and `SHAPES` rectangles on it, the first `4,096` of them in the order made.
fn big_page() -> (Session, Vec<u32>) {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "page": 1}));
    let rectangle = find_master("rectangle").unwrap();
    let mut ids = Vec::new();
    s.edit("build", |doc| {
        let fail = |p: diacraft_engine::draw::Problem| EngineError::Failed { cmd: "test".into(), msg: p.message };
        for i in 0..SHAPES {
            let (c, r) = (i % 60, i / 60);
            let info = drop_shape(doc, 1, rectangle, 0.5 + 0.25 * f64::from(c), 0.5 + 0.25 * f64::from(r)).map_err(fail)?;
            diacraft_engine::theme::follow_page_theme(doc, 1, info.id).map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
            ids.push(info.id);
        }
        Ok(())
    })
    .unwrap();
    (s, ids)
}

/// The commits a command made and their steps (`diacraft_calc::work`).
fn recalculation_of(s: &mut Session, command: &str, params: &serde_json::Value) -> (u64, u64) {
    diacraft_calc::work::reset();
    s.execute(command, params).unwrap();
    (diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

#[test]
fn a_quick_style_on_a_page_of_thousands_is_one_commit_of_the_cells_it_wrote() {
    let (mut s, ids) = big_page();
    let few = &ids[..4_096];
    let (commits, steps) = recalculation_of(&mut s, "shape.quick_style", &json!({"ids": few, "row": 2, "column": 3}));
    assert!(commits <= 1, "one commit: {commits}");
    // Six Quick Style cells for each of the 4,096 shapes, and a shape that follows the style reads the theme's call and holds no other: no recalculation of the page.
    assert!(steps < 4_096 * 6 * 40, "{steps} steps for {} cells", 4_096 * 6);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_quick_style_on_more_shapes_than_the_cap_is_refused_in_words_and_changes_nothing() {
    let (mut s, ids) = big_page();
    // `ids` holds at most 4,096 names, so the cap is reached through groups: two groups of 2,050 shapes are 4,100 shapes to put the style on.
    let first = crate::edit::id_of(&run(&mut s, "shape.group", json!({"ids": &ids[..2_050]})));
    let second = crate::edit::id_of(&run(&mut s, "shape.group", json!({"ids": &ids[2_050..4_100]})));
    let before = snapshot(&s);
    let e = s.execute("shape.quick_style", &json!({"ids": [first, second], "row": 1, "column": 1})).unwrap_err();
    let msg = match &e {
        EngineError::Failed { msg, .. } => msg.clone(),
        other => bad_params_msg(other).to_owned(),
    };
    assert!(msg.contains("at most 4096") && msg.contains("4100"), "{msg}");
    assert_eq!(snapshot(&s), before);
    // One group of them alone is within the cap.
    let r = run(&mut s, "shape.quick_style", json!({"ids": [first], "row": 1, "column": 1}));
    assert_eq!(r["formatted"], 2_050);
}

#[test]
fn a_gradient_and_a_shadow_on_thousands_of_shapes_are_one_pass_and_one_commit_each() {
    let (mut s, ids) = big_page();
    let all = &ids[..4_096];
    let (commits, steps) = recalculation_of(
        &mut s,
        "shape.gradient",
        &json!({"ids": all, "angle": 45, "stops": [
        {"position": 0, "color": "#FF0000"}, {"position": 0.5, "color": "#00FF00", "transparency": 10}, {"position": 1, "color": "#0000FF"}]}),
    );
    assert!(commits <= 1, "one commit: {commits} ({steps} steps)");
    let (commits, steps) = recalculation_of(&mut s, "shape.shadow", &json!({"ids": all, "preset": "bottom_right"}));
    assert!(commits <= 1, "one commit: {commits} ({steps} steps)");
    // The same again changes nothing, and so costs no commit.
    let (commits, _) = recalculation_of(&mut s, "shape.shadow", &json!({"ids": all, "preset": "bottom_right"}));
    assert_eq!(commits, 0);
    assert_eq!(check_session(&s), Ok(()));
}
