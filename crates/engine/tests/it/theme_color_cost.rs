//! What the palette, a theme pick and the theme preview cost, counted and never timed (`diacraft_calc::work`: the commits a command makes and the steps they take) on
//! the page of `edit_cost.rs`, 500 rectangles and 300 glued connectors, and on a page past the preview's budget. A hostile parameter is held to a short answer.

use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_engine::theme::MAX_PREVIEW_SHAPES;
use serde_json::{Value, json};

use crate::edit_cost::{CONNECTORS, big_page, steps_of_the_whole_page};
use crate::{check_session, run, snapshot};

fn counted(s: &mut Session, id: &str, params: Value) -> (Value, u64, u64) {
    diacraft_calc::work::reset();
    let answer = run(s, id, params);
    (answer, diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

fn all_ids(s: &Session) -> Vec<u32> {
    s.document().unwrap().page(1).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

#[test]
fn the_palette_of_a_big_page_is_the_palette_of_a_small_one_and_works_nothing_out_of_the_page() {
    let mut big = big_page();
    run(&mut big, "theme.apply", json!({"theme": "tidepool"}));
    let mut small = crate::blank_session();
    run(&mut small, "theme.apply", json!({"theme": "tidepool"}));
    let before = snapshot(&big);
    let (on_big, commits, _) = counted(&mut big, "theme.colors", json!({}));
    assert_eq!(commits, 0, "a query commits nothing");
    assert_eq!(snapshot(&big), before);
    assert_eq!(on_big, run(&mut small, "theme.colors", json!({})), "sixty-one swatches whatever the page holds");
    assert_eq!(on_big["theme_colors"].as_array().unwrap().iter().map(|c| c["swatches"].as_array().unwrap().len()).sum::<usize>(), 60);
}

#[test]
fn a_theme_pick_on_800_shapes_is_one_commit_and_costs_no_more_than_a_fixed_colour_does_by_much() {
    let mut s = big_page();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let ids = all_ids(&s);
    assert_eq!(ids.len(), 500 + CONNECTORS);
    let whole = steps_of_the_whole_page(&s);
    let (r, commits, steps) = counted(&mut s, "shape.format", json!({"ids": ids, "fill": "theme:accent2:40", "line": "theme:dark"}));
    assert_eq!(r["formatted"], 500 + CONNECTORS, "{r}");
    assert_eq!(commits, 1, "the cells written, and what reads them, are worked out once");
    assert!(steps <= whole, "{steps} steps against {whole} for the whole page worked out, only the cells written and what reads them");
    // The same pick again changes nothing and works nothing out.
    let again = all_ids(&s);
    let (r, commits, steps) = counted(&mut s, "shape.format", json!({"ids": again, "fill": "theme:accent2:40", "line": "theme:dark"}));
    assert_eq!((r["formatted"].clone(), commits, steps), (json!(0), 0, 0), "{r}");
    // A theme applied afterwards works the page out once, as it always did.
    let (_, commits, _) = counted(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    assert_eq!(commits, 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_preview_of_a_page_of_800_shapes_is_one_commit_on_a_copy_and_the_drawing_is_not_touched() {
    let mut s = big_page();
    let before = snapshot(&s);
    let whole = steps_of_the_whole_page(&s);
    let (r, commits, steps) = counted(&mut s, "theme.preview", json!({"theme": "tidepool"}));
    assert_eq!(r["shapes_linked"], 3 * (500 + CONNECTORS), "the copy is linked as an applied theme is: {r}");
    assert_eq!(commits, 1, "the page of the copy is worked out once");
    assert!(steps <= 4 * whole, "{steps} steps against {whole}: a theme works the page out and the masters' cells too");
    assert_eq!(snapshot(&s), before, "the drawing is as it was: no revision, no history");
    // A preview asked for again is made again (nothing is kept), and costs the same.
    let (_, commits_again, steps_again) = counted(&mut s, "theme.preview", json!({"theme": "tidepool"}));
    assert_eq!((commits_again, steps_again), (commits, steps));
}

/// A drawing whose first page holds `n` shapes (copies of one rectangle), made in one edit.
fn page_of(n: usize) -> Session {
    let mut s = crate::blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}));
    s.edit("build", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<diacraft_doc::Shape>> = page.shapes.iter().cloned().collect();
        let model = Arc::clone(&shapes[0]);
        for k in 1..n as u32 {
            let mut shape = (*model).clone();
            shape.id = 100 + k;
            shapes.push(Arc::new(shape));
        }
        page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn a_page_of_more_shapes_than_a_preview_is_made_for_is_refused_in_words_before_any_work_is_done() {
    let mut at_the_cap = page_of(MAX_PREVIEW_SHAPES);
    let (_, commits, _) = counted(&mut at_the_cap, "theme.preview", json!({"theme": "tidepool"}));
    assert_eq!(commits, 1, "a page of exactly the cap is previewed");
    let mut s = page_of(MAX_PREVIEW_SHAPES + 1);
    let before = snapshot(&s);
    diacraft_calc::work::reset();
    let e = s.execute("theme.preview", &json!({"theme": "tidepool"})).unwrap_err().to_string();
    assert!(e.contains(&format!("more than {MAX_PREVIEW_SHAPES} shapes")) && e.contains("apply the theme to see it"), "{e}");
    assert_eq!((diacraft_calc::work::commits(), diacraft_calc::work::steps()), (0, 0), "nothing was worked out: the count came before the copy");
    assert_eq!(snapshot(&s), before);
    // The theme itself still applies to the page (the preview's budget is the preview's).
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
}

#[test]
fn a_hostile_colour_is_refused_in_a_short_answer_and_a_page_is_not_asked_for_past_the_cap() {
    let (mut s, id) = {
        let mut s = crate::blank_session();
        let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}))["id"].as_u64().unwrap();
        (s, id)
    };
    let before = snapshot(&s);
    let long = "a".repeat(1_000_000);
    for (key, value) in [
        ("fill", format!("theme:{long}")),
        ("line", format!("theme:accent2:{long}")),
        ("fill", long.clone()),
        ("fill", format!("theme:accent2:{}", "9".repeat(300))),
    ] {
        let e = s.execute("shape.format", &json!({"ids": [id], key: value})).unwrap_err().to_string();
        assert!(e.len() < 600, "the refusal is short: {} bytes", e.len());
    }
    let e = s.execute("text.format", &json!({"ids": [id], "color": format!("theme:{long}")})).unwrap_err().to_string();
    assert!(e.len() < 600, "{} bytes", e.len());
    let e = s.execute("theme.colors", &json!({"page": 99})).unwrap_err().to_string();
    assert!(e.contains("99"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_page_whose_recalculation_passes_the_previews_steps_is_refused_in_the_words_of_a_preview() {
    // Shapes with many formulas: under the cap of shapes and past the budget of steps.
    let mut s = page_of(1_000);
    s.edit("heavy", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<diacraft_doc::Shape>> = page.shapes.iter().cloned().collect();
        for shape in shapes.iter_mut() {
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            for k in 0..400 {
                sheet.cells.insert(diacraft_doc::CellName::new(&format!("Prop.Heavy{k}")), diacraft_doc::Cell::with_formula("Width*Height+1", None));
            }
        }
        page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let e = s.execute("theme.preview", &json!({"theme": "tidepool"})).unwrap_err().to_string();
    assert!(e.contains("too large to preview") && e.contains("apply the theme to see it"), "{e}");
    assert_eq!(snapshot(&s), before);
}

/// A themed page of `n` shapes: one shape follows the `Theme` style of a theme applied to the page, and the other `n - 1` are copies of it (so every one follows it, and the page is not
/// worked out `n` times by a `theme.apply`). With `grouped` the `n` copies are the members of one group (the first shape of the page); else they are shapes of the page.
fn themed_page_of(n: usize, grouped: bool) -> (Session, Vec<u32>) {
    let mut s = crate::blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    s.edit("build", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<diacraft_doc::Shape>> = page.shapes.iter().cloned().collect();
        let model = Arc::clone(&shapes[0]);
        let copies: Vec<Arc<diacraft_doc::Shape>> = (1..n as u32)
            .map(|k| {
                let mut shape = (*model).clone();
                shape.id = 100 + k;
                Arc::new(shape)
            })
            .collect();
        if grouped {
            let mut group = diacraft_doc::Shape::new(50);
            group.kind = diacraft_doc::ShapeType::Group;
            group.children = diacraft_doc::ShapeList::from_vec(copies);
            shapes.push(Arc::new(group));
        } else {
            shapes.extend(copies);
        }
        page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    let ids = s.document().unwrap().page(1).unwrap().shapes.iter().map(|shape| shape.id).collect();
    (s, ids)
}

/// The steps of the recalculation a command made, and the walks of the page and the shapes planned that the passes making shapes follow the theme did.
fn followed(s: &mut Session, id: &str, params: Value) -> (u64, u64, diacraft_engine::theme::FollowWork) {
    diacraft_engine::theme::reset_follow_work();
    diacraft_calc::work::reset();
    run(s, id, params);
    (diacraft_calc::work::commits(), diacraft_calc::work::steps(), diacraft_engine::theme::follow_work())
}

/// The most shape ids a command takes (`cmd/params.rs`).
const MAX_IDS: usize = 4_096;

#[test]
fn a_theme_colour_on_a_selection_follows_the_theme_in_one_pass_and_costs_what_a_plain_colour_does_by_a_small_constant() {
    // A page of 100,000 shapes, 4,096 of them named (the most a command takes).
    let (mut s, ids) = themed_page_of(100_000, false);
    let chosen: Vec<u32> = ids[..MAX_IDS].to_vec();
    let (plain_commits, plain_steps, plain) = followed(&mut s, "shape.format", json!({"ids": chosen, "fill": "#336699", "line": "#112233"}));
    assert_eq!((plain_commits, plain.page_walks), (1, 0), "a plain colour follows no theme");
    let (commits, steps, work) = followed(&mut s, "shape.format", json!({"ids": chosen, "fill": "theme:accent2:40", "line": "theme:dark"}));
    assert_eq!(commits, 1, "one recalculation");
    assert_eq!(work.page_walks, 0, "the shapes' places are known: no walk of a page of 100,000 shapes to find one by its ID: {work:?}");
    assert!(work.shapes_planned <= chosen.len() as u64, "each shape once: {work:?}");
    assert!(steps <= 4 * plain_steps.max(1) + 1_000, "{steps} steps against {plain_steps} for a plain fill and line");
    // Twice the shapes cost twice the work and not four times: the cost is linear in the shapes named, whatever the page holds.
    let (_, half_steps, half) = followed(
        &mut s,
        "shape.format",
        json!({"ids": ids[MAX_IDS..MAX_IDS + MAX_IDS / 2].to_vec(), "fill": "theme:accent3:40", "line": "theme:dark"}),
    );
    assert_eq!((half.page_walks, half.shapes_planned), (0, (MAX_IDS / 2) as u64), "{half:?}");
    assert!(steps <= 2 * half_steps + 40_000, "{steps} steps for {MAX_IDS} shapes against {half_steps} for {}", MAX_IDS / 2);
    // The same for the colour of the text.
    let (commits, _, work) = followed(&mut s, "text.format", json!({"ids": chosen, "color": "theme:accent3"}));
    assert_eq!((commits, work.page_walks), (1, 0), "{work:?}");
    assert!(work.shapes_planned <= chosen.len() as u64, "{work:?}");
    // Every shape got the colour, and follows the theme.
    for id in [chosen[0], chosen[chosen.len() - 1]] {
        let fill = run(&mut s, "shape.formatting", json!({"ids": [id]}))["shapes"][0]["fill"].clone();
        assert!(fill.as_str().is_some_and(|c| c.starts_with('#')), "a colour: {fill}");
        assert_ne!(fill, "#336699", "the pick replaced the plain colour");
    }
    // (That the colour follows a theme applied afterwards is `theme_color.rs`; applying a theme to a page of 100,000 shapes is past the steps of a recalculation.)
}

#[test]
fn a_theme_colour_on_a_group_of_more_shapes_than_the_cap_is_refused_in_words_before_any_pass_is_made() {
    // One group named by one id holds 15,000 members: past the cap of a theme colour, under the cap of the shapes a command formats.
    let (mut s, _) = themed_page_of(15_001, true);
    for (what, id, params) in [
        ("a fill", "shape.format", json!({"ids": [50], "fill": "theme:accent2"})),
        ("a text colour", "text.format", json!({"ids": [50], "color": "theme:accent2"})),
    ] {
        diacraft_engine::theme::reset_follow_work();
        let e = s.execute(id, &params).unwrap_err().to_string();
        assert!(e.contains("at most 10000 shapes at a time") && e.contains("apply the theme"), "{what}: {e}");
        assert_eq!(diacraft_engine::theme::follow_work(), diacraft_engine::theme::FollowWork::default(), "{what}: no pass was made");
    }
    // A plain colour on the same group is not held to the cap of a colour of the theme.
    run(&mut s, "shape.format", json!({"ids": [50], "fill": "#336699"}));
    // A group of 100,000 members: past the cap of the shapes a command formats at all, the old refusal, in words.
    let (mut big, _) = themed_page_of(100_001, true);
    let e = big.execute("shape.format", &json!({"ids": [50], "fill": "theme:accent2"})).unwrap_err().to_string();
    assert!(e.contains("more than"), "{e}");
}

// ---- one walk of the page for the shapes a command names -------------------------------------------------------------------------

/// What a command did on this thread, counted: the shapes `Document::find_shape` visited (a walk of the page from the top for one `ID`: `diacraft_doc::work`) and what the passes
/// that make shapes follow the theme did.
fn looked_up(s: &mut Session, id: &str, params: Value) -> (Value, u64, diacraft_engine::theme::FollowWork) {
    diacraft_doc::work::reset();
    diacraft_engine::theme::reset_follow_work();
    let answer = run(s, id, params);
    (answer, diacraft_doc::work::finds_visited(), diacraft_engine::theme::follow_work())
}

/// The ids `count` shapes of a page spread from its first shape to its last: the selection that costs most to find one by one (each is found after about half the page).
fn spread(ids: &[u32], count: usize) -> Vec<u32> {
    let stride = ids.len() / count;
    (0..count).map(|k| ids[k * stride]).collect()
}

#[test]
fn the_shapes_a_command_names_are_found_in_one_walk_of_the_page_and_not_one_walk_each() {
    // A themed page of 30,000 shapes and the 4,096 ids a command takes, spread over all of it. Found one by one by `find_shape`, each costs about half the page, and the commands
    // below did it three times over (the page the ids are on, the shapes to format, and each shape made to follow the theme): about 180 million visits.
    let (mut s, ids) = themed_page_of(30_000, false);
    let chosen = spread(&ids, MAX_IDS);
    assert_eq!(chosen.len(), MAX_IDS);
    for (what, id, params) in [
        ("a plain fill and line", "shape.format", json!({"ids": chosen, "fill": "#336699", "line": "#112233"})),
        ("a theme colour", "shape.format", json!({"ids": chosen, "fill": "theme:accent3:40", "line": "theme:dark"})),
        ("a text colour", "text.format", json!({"ids": chosen, "color": "theme:accent2"})),
        ("a Quick Style", "shape.quick_style", json!({"ids": chosen, "row": 2, "column": 3})),
    ] {
        let (answer, finds, work) = looked_up(&mut s, id, params);
        assert!(answer["formatted"].as_u64().is_some_and(|n| n > 0), "{what} formatted shapes: {answer}");
        assert_eq!(finds, 0, "{what}: no shape was looked for by a walk of the page from the top");
        assert_eq!(work.page_walks, 0, "{what}: {work:?}");
        assert!(work.shapes_planned <= chosen.len() as u64, "{what}: each shape once: {work:?}");
    }
}

#[test]
fn a_quick_style_makes_the_shapes_follow_the_theme_in_one_pass_over_the_places_it_has() {
    // Shapes that follow no style of their own (as the shapes of a drawing made before the theme do): the Quick Style links them, and each is planned once.
    let (mut s, ids) = themed_page_of(2_000, false);
    s.edit("unlink", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<diacraft_doc::Shape>> = page.shapes.iter().cloned().collect();
        for shape in shapes.iter_mut() {
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            sheet.links = diacraft_doc::Links::default();
        }
        page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    let chosen = spread(&ids, 1_000);
    let (answer, finds, work) = looked_up(&mut s, "shape.quick_style", json!({"ids": chosen, "row": 1, "column": 2}));
    assert_eq!(answer["formatted"], 1_000, "{answer}");
    assert_eq!((finds, work.page_walks), (0, 0), "{work:?}");
    assert_eq!(work.shapes_planned, 1_000, "each shape named is planned once: {work:?}");
    // The shapes now follow the Theme style, so a theme applied afterwards changes the fill of one the style was put on.
    let fill = |s: &mut Session, id: u32| run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0]["fill"].clone();
    let named = chosen[10];
    let before = fill(&mut s, named);
    run(&mut s, "theme.apply", json!({"theme": "cinderglow"}));
    assert_ne!(fill(&mut s, named), before, "a shape the style was put on follows the theme");
}
