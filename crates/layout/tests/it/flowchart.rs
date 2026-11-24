//! Flowchart layouts: layers along the flow.

use diacraft_layout::{Direction, Style, layout};

use crate::support::{graph, near, node, overlapping, row, settings, top_left};

fn flow(dir: Direction) -> Style {
    Style::Flowchart(dir)
}

#[test]
fn a_chain_runs_down_one_column_with_the_space_between_layers() {
    // Three 2 by 1 shapes left in a row, joined 0 -> 1 -> 2: a flow down puts them in a column, a layer apart: 1 high and a space of 1 between, so 2 from middle to middle.
    // Catches a flow that leaves the shapes where they were, and one that forgets the space.
    let g = graph(row(3), &[(0, 1), (1, 2)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(near(at[0].0, at[1].0) && near(at[1].0, at[2].0), "one column: {at:?}");
    assert!(near(at[0].1 - at[1].1, 2.0) && near(at[1].1 - at[2].1, 2.0), "a layer apart, the first on top: {at:?}");
    let g = graph(row(3), &[(0, 1), (1, 2)]);
    let wide = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 4.0)).unwrap();
    assert!(near(wide[0].1 - wide[1].1, 5.0), "the space along y is the space between layers: {wide:?}");
}

#[test]
fn the_four_directions_run_the_way_they_say() {
    // Catches a mirrored axis: LeftToRight must go to the right and put the first shape on the left, RightToLeft the other way, BottomToTop up.
    let g = graph(row(3), &[(0, 1), (1, 2)]);
    let run = |dir| layout(&g, &settings(flow(dir), 1.0, 1.0)).unwrap();
    let lr = run(Direction::LeftToRight);
    assert!(lr[0].0 < lr[1].0 && lr[1].0 < lr[2].0, "{lr:?}");
    assert!(near(lr[1].0 - lr[0].0, 3.0), "shapes 2 wide and a space of 1: {lr:?}");
    assert!(near(lr[0].1, lr[1].1) && near(lr[1].1, lr[2].1));
    let rl = run(Direction::RightToLeft);
    assert!(rl[0].0 > rl[1].0 && rl[1].0 > rl[2].0, "{rl:?}");
    let bt = run(Direction::BottomToTop);
    assert!(bt[0].1 < bt[1].1 && bt[1].1 < bt[2].1, "{bt:?}");
    assert!(near(bt[0].0, bt[1].0));
    let tb = run(Direction::TopToBottom);
    assert!(tb[0].1 > tb[1].1 && tb[1].1 > tb[2].1, "{tb:?}");
}

#[test]
fn the_space_across_a_layer_is_the_space_along_x_for_a_flow_down_and_along_y_for_a_flow_across() {
    // Two shapes in one layer: 0 -> 1 and 0 -> 2. Flow down: they are `gap_x` apart. Flow right: `gap_y` apart. Catches a swapped pair of spaces.
    let g = graph(row(3), &[(0, 1), (0, 2)]);
    let down = layout(&g, &settings(flow(Direction::TopToBottom), 3.0, 1.0)).unwrap();
    assert!(near((down[1].0 - down[2].0).abs(), 2.0 + 3.0), "2 wide and a space of 3: {down:?}");
    let right = layout(&g, &settings(flow(Direction::LeftToRight), 1.0, 3.0)).unwrap();
    assert!(near((right[1].1 - right[2].1).abs(), 1.0 + 3.0), "1 high and a space of 3: {right:?}");
}

#[test]
fn a_diamond_has_its_ends_over_the_middle_of_its_sides() {
    // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3. Catches a layout that leaves the top and the bottom off the middle of the pair.
    let g = graph(row(4), &[(0, 1), (0, 2), (1, 3), (2, 3)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(near(at[1].1, at[2].1), "one layer: {at:?}");
    assert!((at[1].0 - at[2].0).abs() >= 3.0 - 1e-9, "a shape's width and the space apart: {at:?}");
    let middle = (at[1].0 + at[2].0) / 2.0;
    assert!((at[0].0 - middle).abs() < 1e-6 && (at[3].0 - middle).abs() < 1e-6, "{at:?}");
}

#[test]
fn connectors_that_cross_between_two_layers_are_uncrossed() {
    // The top shapes are left A (x 0) and B (x 5), the bottom C (x 0) and D (x 5), and the connectors go A -> D and B -> C: crossed as they stand. A layout turns the
    // order of one layer so they do not. Catches a layout that keeps the order it was given.
    let nodes = vec![node(2.0, 1.0, 0.0, 10.0), node(2.0, 1.0, 5.0, 10.0), node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 5.0, 0.0)];
    let g = graph(nodes, &[(0, 3), (1, 2)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(near(at[0].1, at[1].1) && near(at[2].1, at[3].1) && at[0].1 > at[2].1, "{at:?}");
    assert!((at[0].0 - at[1].0) * (at[3].0 - at[2].0) > 0.0, "A -> D and B -> C do not cross: {at:?}");
}

#[test]
fn a_cycle_is_laid_out_in_layers_with_one_connector_turned() {
    // 0 -> 1 -> 2 -> 0. Catches an endless walk, and a ring that is left in one layer.
    let g = graph(row(3), &[(0, 1), (1, 2), (2, 0)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    let mut layers: Vec<i64> = at.iter().map(|p| (p.1 * 1000.0).round() as i64).collect();
    layers.sort_unstable();
    layers.dedup();
    assert_eq!(layers.len(), 3, "three layers: {at:?}");
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none());
}

#[test]
fn shapes_joined_both_ways_and_a_shape_joined_to_itself_are_harmless() {
    // 0 <-> 1 and 0 -> 0: the same layout as the one connector 0 -> 1. Catches a self-loop that counts as a cycle and a pair that makes two layers of one.
    let plain = graph(row(2), &[(0, 1)]);
    let odd = graph(row(2), &[(0, 1), (1, 0), (0, 0), (0, 1)]);
    let s = settings(flow(Direction::TopToBottom), 1.0, 1.0);
    assert_eq!(layout(&plain, &s).unwrap(), layout(&odd, &s).unwrap());
}

#[test]
fn a_shape_nothing_points_at_comes_down_to_the_layer_before_its_nearest() {
    // P -> Q -> D puts D three layers down; X -> D has no reason to be at the top: X goes in Q's layer. Catches a longest path that leaves X at the top, a layer
    // away from nothing, with a long connector.
    let nodes = vec![node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 9.0, 0.0)];
    let g = graph(nodes, &[(0, 1), (1, 2), (3, 2)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(near(at[3].1, at[1].1), "X is in the layer of Q: {at:?}");
}

#[test]
fn a_long_connector_keeps_the_shapes_between_its_ends_off_its_way() {
    // 0 -> 1 -> 2 -> 3 and 0 -> 3: the long connector passes layers 1 and 2, where 1 and 2 are. No overlap, layers strictly down.
    let g = graph(row(4), &[(0, 1), (1, 2), (2, 3), (0, 3)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(at[0].1 > at[1].1 && at[1].1 > at[2].1 && at[2].1 > at[3].1, "{at:?}");
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none());
}

#[test]
fn separate_parts_are_set_apart_and_the_top_left_corner_stays_where_it_was() {
    // Two chains with nothing between them, and the corner of the whole diagram at (10, 20) minus half a box. Catches parts laid on top of each other, and a layout that
    // moves the diagram off its place.
    let nodes = vec![node(2.0, 1.0, 10.0, 20.0), node(2.0, 1.0, 10.0, 15.0), node(2.0, 1.0, 20.0, 20.0), node(2.0, 1.0, 20.0, 15.0)];
    let g = graph(nodes, &[(0, 1), (2, 3)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
    let (left, top) = top_left(&g.nodes, &at);
    assert!(near(left, 9.0) && near(top, 20.5), "the corner is where it was: {left} {top}");
    // Two parts of one size fit in a row: the second is to the right of the first, not below it.
    assert!(at[2].0 > at[0].0 && near(at[2].1, at[0].1), "{at:?}");
}

#[test]
fn many_separate_shapes_make_a_block_and_not_a_line() {
    // A hundred shapes nothing joins: set in rows, so the block is about as wide as it is tall. Catches parts in one row, a line a hundred shapes long.
    let nodes: Vec<_> = (0..100).map(|i| node(2.0, 1.0, f64::from(i) * 3.0, 0.0)).collect();
    let g = graph(nodes, &[]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none());
    let (xs, ys): (Vec<f64>, Vec<f64>) = at.iter().copied().unzip();
    let span = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min);
    let ratio = span(&xs) / span(&ys);
    assert!(ratio > 0.5 && ratio < 8.0, "a block, {} wide and {} tall", span(&xs), span(&ys));
}

#[test]
fn the_order_the_shapes_have_is_kept_where_the_connectors_do_not_ask_for_another() {
    // Three shapes under one, left in the order 7, 3, 5 across: they stay in that order. Catches a layout that sorts by number.
    let nodes = vec![node(2.0, 1.0, 5.0, 10.0), node(2.0, 1.0, 7.0, 0.0), node(2.0, 1.0, 3.0, 0.0), node(2.0, 1.0, 5.0, 0.0)];
    let g = graph(nodes, &[(0, 1), (0, 2), (0, 3)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 1.0, 1.0)).unwrap();
    assert!(at[2].0 < at[3].0 && at[3].0 < at[1].0, "{at:?}");
}

#[test]
fn a_layout_is_the_same_the_second_time() {
    // Catches a pass that reads uninitialised or hash order: the same graph twice, and the layout laid out again, is the same.
    let g = graph(row(8), &[(0, 1), (0, 2), (1, 3), (2, 3), (2, 4), (4, 5), (3, 6), (5, 6), (6, 7), (7, 0)]);
    let s = settings(flow(Direction::LeftToRight), 1.0, 1.0);
    let once = layout(&g, &s).unwrap();
    assert_eq!(once, layout(&g, &s).unwrap());
    let mut again = g.clone();
    for (n, p) in again.nodes.iter_mut().zip(&once) {
        (n.x, n.y) = *p;
    }
    let twice = layout(&again, &s).unwrap();
    for (a, b) in once.iter().zip(&twice) {
        assert!(near(a.0, b.0) && near(a.1, b.1), "a diagram that is laid out is laid out the same: {once:?} {twice:?}");
    }
}

#[test]
fn shapes_of_different_sizes_keep_their_spaces() {
    let nodes = vec![node(6.0, 2.0, 0.0, 0.0), node(1.0, 4.0, 0.0, 0.0), node(3.0, 1.0, 0.0, 0.0), node(2.0, 2.0, 0.0, 0.0)];
    let g = graph(nodes, &[(0, 1), (0, 2), (0, 3)]);
    let at = layout(&g, &settings(flow(Direction::TopToBottom), 0.5, 0.5)).unwrap();
    for i in 0..4 {
        for j in i + 1..4 {
            let c = crate::support::clearance(&g.nodes[i], at[i], &g.nodes[j], at[j]);
            assert!(c >= 0.5 - 1e-9, "shapes {i} and {j} are {c} apart: {at:?}");
        }
    }
}
