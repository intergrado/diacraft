//! Hierarchy layouts: a parent over its children.

use diacraft_layout::{Align, Direction, Style, layout};

use crate::support::{graph, near, node, overlapping, row, settings};

fn tree(dir: Direction, align: Align) -> Style {
    Style::Hierarchy(dir, align)
}

#[test]
fn a_parent_is_in_the_middle_over_its_children() {
    // 0 -> 1, 0 -> 2: the parent over the middle of the pair, the children a width and a space apart, one layer down. Catches a parent over its first child.
    let g = graph(row(3), &[(0, 1), (0, 2)]);
    let at = layout(&g, &settings(tree(Direction::TopToBottom, Align::Center), 1.0, 1.0)).unwrap();
    assert!(near(at[1].1, at[2].1) && near(at[0].1 - at[1].1, 2.0), "{at:?}");
    assert!(near(at[2].0 - at[1].0, 3.0), "{at:?}");
    assert!(near(at[0].0, (at[1].0 + at[2].0) / 2.0), "{at:?}");
}

#[test]
fn the_alignment_puts_the_parent_over_the_first_child_or_the_last() {
    // Catches an alignment that is ignored, and one that is swapped.
    let g = graph(row(4), &[(0, 1), (0, 2), (0, 3)]);
    let at_for = |a| layout(&g, &settings(tree(Direction::TopToBottom, a), 1.0, 1.0)).unwrap();
    let start = at_for(Align::Start);
    assert!(near(start[0].0, start[1].0), "over the first: {start:?}");
    let end = at_for(Align::End);
    assert!(near(end[0].0, end[3].0), "over the last: {end:?}");
    let centre = at_for(Align::Center);
    assert!(near(centre[0].0, centre[2].0), "over the middle one of three: {centre:?}");
}

#[test]
fn each_subtree_has_the_width_its_children_need() {
    // 0 has children 1 and 2; 1 has three children, 2 has none: the three are in a row under 1 and 2 is not over them. No overlap anywhere, and the leaves are in order.
    let nodes = vec![
        node(2.0, 1.0, 0.0, 0.0),
        node(2.0, 1.0, 0.0, 0.0),
        node(2.0, 1.0, 20.0, 0.0),
        node(2.0, 1.0, 0.0, 0.0),
        node(2.0, 1.0, 5.0, 0.0),
        node(2.0, 1.0, 9.0, 0.0),
    ];
    let g = graph(nodes, &[(0, 1), (0, 2), (1, 3), (1, 4), (1, 5)]);
    let at = layout(&g, &settings(tree(Direction::TopToBottom, Align::Center), 1.0, 1.0)).unwrap();
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
    assert!(at[3].0 < at[4].0 && at[4].0 < at[5].0 && at[5].0 < at[2].0, "{at:?}");
    assert!(near(at[1].0, at[4].0), "1 is over its three: {at:?}");
}

#[test]
fn a_flow_across_puts_the_parent_on_the_left_and_the_children_one_above_the_other() {
    // Children left at y = 9, 3, 5 go top to bottom in the order of their places: 9 above 5 above 3. Catches children that are put in the order of their numbers.
    let nodes = vec![node(2.0, 1.0, 0.0, 5.0), node(2.0, 1.0, 5.0, 9.0), node(2.0, 1.0, 5.0, 3.0), node(2.0, 1.0, 5.0, 5.0)];
    let g = graph(nodes, &[(0, 1), (0, 2), (0, 3)]);
    let at = layout(&g, &settings(tree(Direction::LeftToRight, Align::Center), 1.0, 1.0)).unwrap();
    assert!(at[0].0 < at[1].0 && near(at[1].0, at[2].0) && near(at[1].0, at[3].0), "{at:?}");
    assert!(at[1].1 > at[3].1 && at[3].1 > at[2].1, "{at:?}");
    assert!(near(at[0].1, at[3].1), "the parent is across from the middle child: {at:?}");
}

#[test]
fn a_connector_to_a_shape_that_was_reached_already_is_not_part_of_the_tree() {
    // 0 -> 1, 0 -> 2, 1 -> 2: 2 is a child of 0 (reached first, by the connector from 0), and 1 -> 2 is only drawn. Both are in the layer below 0.
    let g = graph(row(3), &[(0, 1), (0, 2), (1, 2)]);
    let at = layout(&g, &settings(tree(Direction::TopToBottom, Align::Center), 1.0, 1.0)).unwrap();
    assert!(near(at[1].1, at[2].1), "{at:?}");
}

#[test]
fn a_diagram_with_two_roots_has_them_side_by_side_and_a_ring_has_a_root_too() {
    // 0 -> 2 and 1 -> 2: two roots. And a ring 3 -> 4 -> 5 -> 3 that nothing points into from outside: its first shape to read is the root.
    let g = graph(row(6), &[(0, 2), (1, 2), (3, 4), (4, 5), (5, 3)]);
    let at = layout(&g, &settings(tree(Direction::TopToBottom, Align::Center), 1.0, 1.0)).unwrap();
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
    assert!(near(at[0].1, at[1].1), "both roots on the top layer: {at:?}");
    assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
}

#[test]
fn a_chain_of_a_hundred_thousand_shapes_is_laid_out_with_no_stack() {
    // Catches a recursive walk. 100,000 shapes in a chain, a layer each.
    let n = 100_000;
    let nodes = vec![node(2.0, 1.0, 0.0, 0.0); n];
    let edges: Vec<(u32, u32)> = (0..n as u32 - 1).map(|i| (i, i + 1)).collect();
    let g = graph(nodes, &edges);
    let at = layout(&g, &settings(tree(Direction::TopToBottom, Align::Center), 1.0, 1.0)).unwrap();
    assert!(near(at[0].1 - at[n - 1].1, 2.0 * (n as f64 - 1.0)));
    assert!(near(at[0].0, at[n - 1].0));
}
