//! Kani proof harnesses for the integer code of the graph layout: the budget of work every pass charges before it runs (`model::charged`, the
//! arithmetic of `Work::charge`, taken out of it with no change of behaviour) and the lists the graph is kept in (`forest::csr`). Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved here:** the placing of the shapes (`layered`, `tree`, `round`, `pack`: many symbolic floats, sorts of lists and walks of the graph, the traps of
//! `docs/development.md`, Proofs); the engine's `relayout::spacing::spread` (the sort of the intervals by their first edge beside six to eight floats) and its count of
//! the connection points the connectors hold (`connectors::Held`, two hash maps: the futex trap). Their unit tests hold them.

use crate::MAX_WORK;
use crate::forest::csr;
use crate::model::charged;

/// `model::charged` (`Work::charge`'s arithmetic, the budget of work a layout charges before each pass) for **three** charges in a row of any steps, each taken
/// as `charge` takes it: a charge is taken exactly when the steps so far and it are within [`MAX_WORK`], and then counted to the step; a refused one leaves the count as it was;
/// so the work charged never passes the budget and never wraps. (`Work::charge` itself ran past 300 s: its refusal is made with `format!`, the trap of
/// `docs/development.md`, Proofs.)
#[kani::proof]
fn a_layouts_work_is_charged_before_each_pass_and_never_passes_its_budget() {
    let mut used: u64 = 0;
    let mut taken: u64 = 0;
    for _ in 0..3 {
        let steps: u64 = kani::any();
        let fits = u128::from(used) + u128::from(steps) <= u128::from(MAX_WORK);
        match charged(used, steps) {
            Some(total) => {
                assert!(fits && total == used + steps);
                used = total;
                taken += 1;
            }
            None => {
                assert!(!fits);
            }
        }
        assert!(used <= MAX_WORK);
    }
    kani::cover!(taken == 3, "three passes charged");
    kani::cover!((taken == 1) & (used == MAX_WORK), "a pass that takes the whole budget, and two refused");
}

/// `forest::csr` (the graph as one list for each shape, kept in one vector) for **every** four pairs of shapes of numbers 0 to 4 among three shapes (a number past
/// the last is no shape, which no caller gives): the list of each shape is the second shapes of the pairs that start from it, in the order of the pairs, and a shape that is
/// not there has none.
#[kani::proof]
#[kani::unwind(6)]
fn the_list_of_each_shape_holds_the_shapes_its_pairs_lead_to_in_their_order() {
    const N: usize = 3;
    let pairs: [(u32, u32); 4] = kani::any();
    kani::assume(pairs.iter().all(|(v, w)| *v <= 4 && *w <= 4));
    let lists = csr(N, &pairs);
    let mut listed = 0usize;
    for v in 0..N {
        let row = lists.row(v);
        let mut at = 0usize;
        for (from, to) in pairs {
            if from as usize == v {
                assert!(row.get(at) == Some(&to));
                at += 1;
            }
        }
        assert!(row.len() == at);
        listed += row.len();
    }
    assert!(lists.row(N).is_empty() && lists.row(N + 1).is_empty());
    let valid = pairs.iter().filter(|(v, _)| (*v as usize) < N).count();
    assert!(listed == valid);
    kani::cover!((valid == 4) & (lists.row(0).len() == 2), "a shape with two pairs");
    kani::cover!(valid < 4, "a pair from no shape");
}
