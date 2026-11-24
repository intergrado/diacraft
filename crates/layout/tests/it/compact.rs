//! Compact trees: an outline.

use diacraft_layout::{Error, Side, Style, layout};

use crate::support::{graph, near, node, overlapping, row, settings};

fn outline(run: Side, level: Side) -> Style {
    Style::CompactTree { run, level }
}

/// 0 is the root, 1 and 2 are its children, 3 is a child of 1: a walk down meets 0, 1, 3, 2.
fn small_tree() -> diacraft_layout::Graph {
    graph(row(4), &[(0, 1), (0, 2), (1, 3)])
}

#[test]
fn down_right_is_a_list_read_down_with_each_level_indented_to_the_right() {
    // Shapes 2 by 1 with a space of 1: a step down is 2, a level in is 3. Catches an outline that puts siblings side by side, and one that is not in walk order.
    let at = layout(&small_tree(), &settings(outline(Side::Down, Side::Right), 1.0, 1.0)).unwrap();
    let down = |a: usize, b: usize| at[a].1 - at[b].1;
    assert!(near(down(0, 1), 2.0) && near(down(1, 3), 2.0) && near(down(3, 2), 2.0), "meets 0, 1, 3, 2 in that order: {at:?}");
    assert!(near(at[1].0 - at[0].0, 3.0) && near(at[3].0 - at[1].0, 3.0) && near(at[2].0, at[1].0), "{at:?}");
}

#[test]
fn the_other_sides_turn_the_outline() {
    // Up and left run the other way: the signs turn, the steps do not.
    let at = layout(&small_tree(), &settings(outline(Side::Up, Side::Left), 1.0, 1.0)).unwrap();
    assert!(near(at[1].1 - at[0].1, 2.0) && near(at[0].0 - at[1].0, 3.0), "{at:?}");
    // Right and down: siblings in a row (a step along x of a width and a space, 3) with the levels below (a height and a space, 2).
    let at = layout(&small_tree(), &settings(outline(Side::Right, Side::Down), 1.0, 1.0)).unwrap();
    assert!(near(at[1].0 - at[0].0, 3.0) && near(at[3].0 - at[1].0, 3.0), "{at:?}");
    assert!(near(at[0].1 - at[1].1, 2.0) && near(at[1].1 - at[3].1, 2.0) && near(at[2].1, at[1].1), "{at:?}");
    assert!(near(at[2].0 - at[3].0, 3.0), "{at:?}");
}

#[test]
fn a_run_and_a_level_on_one_axis_are_refused_in_words() {
    // Catches an outline that runs over itself.
    let err = layout(&small_tree(), &settings(outline(Side::Down, Side::Up), 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::Invalid(words) if words.contains("axis")), "{err}");
}

#[test]
fn every_one_of_the_eight_outlines_keeps_shapes_apart() {
    let nodes =
        vec![node(3.0, 1.0, 0.0, 0.0), node(2.0, 2.0, 5.0, 0.0), node(4.0, 1.0, 9.0, 0.0), node(1.0, 3.0, 1.0, 4.0), node(2.0, 1.0, 7.0, 6.0)];
    let g = graph(nodes, &[(0, 1), (0, 2), (1, 3), (1, 4)]);
    let outlines = [
        (Side::Down, Side::Right),
        (Side::Right, Side::Down),
        (Side::Right, Side::Up),
        (Side::Up, Side::Right),
        (Side::Up, Side::Left),
        (Side::Left, Side::Up),
        (Side::Left, Side::Down),
        (Side::Down, Side::Left),
    ];
    for (run, level) in outlines {
        let at = layout(&g, &settings(outline(run, level), 0.5, 0.5)).unwrap();
        assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{run:?} {level:?}: {at:?}");
    }
}

#[test]
fn separate_trees_follow_one_another_along_the_run() {
    // Two roots: the second tree's shapes come after the first's.
    let g = graph(row(4), &[(0, 1), (2, 3)]);
    let at = layout(&g, &settings(outline(Side::Down, Side::Right), 1.0, 1.0)).unwrap();
    assert!(at[0].1 > at[1].1 && at[1].1 > at[2].1 && at[2].1 > at[3].1, "{at:?}");
}
