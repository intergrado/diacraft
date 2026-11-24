//! What the theme commands cost on a large page: the page of `edit_cost.rs`, 500 rectangles and 300 glued connectors.
//!
//! The cost is counted (`diacraft_calc::work`: the commits a command makes and the steps they take), not timed: a time moves with the load on the
//! machine. **What the fixture lacks** (so the numbers are not claimed for it): its shapes follow no style before the first application, it holds no
//! groups, no master whose shapes hold theme formulas and no number cached under one, so the recalculation writes nothing. A theme asks for the whole
//! page to be worked out (`theme/apply.rs`: what `THEMEVAL` reads of the sheet's cells is not followed), once, with the cells of the masters its shapes
//! use, in the drawing now and as the command found it (a master's cell the theme changed is worked out, and an instance is given a cell it
//! inherits only where the theme changed it); the first application also links every shape (three families each) and so copies each shape once, and the
//! next ones change the page's seven cells.

use diacraft_calc::{Budget, EvalOptions, Scope, commit_in};
use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit_cost::{CONNECTORS, big_page, steps_of_the_whole_page};
use crate::{check_session, run};

/// Runs a command and gives its answer, the commits the recalculation made for it and the steps they took.
fn counted(s: &mut Session, id: &str, params: Value) -> (Value, u64, u64) {
    diacraft_calc::work::reset();
    let answer = run(s, id, params);
    (answer, diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

/// The steps the recalculation of every sheet of the drawing takes once the page holds no shape: the document's, the styles', the masters' and the page's
/// own, each worked out by an evaluator that has worked nothing else out. At least the steps of the cells of the masters the page's shapes use, which a
/// theme works out twice: in the drawing now (a step for each and its working out, after the page's) and as the command found it (by an evaluator of its own).
fn steps_of_the_rest(s: &Session) -> u64 {
    let mut copy = s.document().unwrap().clone();
    let page = std::sync::Arc::make_mut(copy.pages.get_mut(0).unwrap());
    page.shapes = Default::default();
    page.connects = Default::default();
    let report = commit_in(&mut copy, &Budget::default(), EvalOptions::default(), Scope::Document);
    assert!(!report.budget_exhausted, "{report:?}");
    report.stats.steps
}

#[test]
fn the_theme_commands_on_a_page_of_500_shapes_and_300_connectors() {
    let mut s = big_page();
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.len(), 500 + CONNECTORS);
    let page = steps_of_the_whole_page(&s);
    let rest = steps_of_the_rest(&s);
    // The page once, and the masters' cells twice: a step and a working out each in the drawing, and a working out in the drawing as it was found.
    let whole = page + 3 * rest;
    // The first theme links the three families of each of the 800 shapes, writes the page's cells, and works the page out once.
    let (r, commits, steps) = counted(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(r["shapes_linked"], 3 * (500 + CONNECTORS), "{r}");
    eprintln!(
        "the first theme.apply on 800 shapes: {commits} commit of {steps} steps; the whole page is {page} steps, the rest of the drawing {rest}"
    );
    assert_eq!(commits, 1, "the page is worked out once");
    assert!(steps <= whole, "{steps} steps against {whole} for the page as it was and its masters twice: a theme works them out and no more");
    // The same theme again finds nothing to do, and works out nothing.
    let (r, commits, steps) = counted(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!((r["changed_pages"].clone(), commits, steps), (json!([]), 0, 0), "{r}");
    // Another theme, a variant and no theme each change the page's seven cells and work the page out once.
    for (id, params) in [
        ("theme.apply", json!({"theme": "cinderglow", "variant": 1})),
        ("theme.variant", json!({"variant": 3})),
        ("theme.variant", json!({"colors": 0})),
        ("theme.apply", json!({"theme": "none"})),
    ] {
        let (r, commits, steps) = counted(&mut s, id, params.clone());
        assert_eq!(r["shapes_linked"], 0, "{id} {params}: the shapes follow already");
        assert_eq!(commits, 1, "{id} {params}");
        assert!(steps <= whole, "{id} {params}: {steps} steps against {whole}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
