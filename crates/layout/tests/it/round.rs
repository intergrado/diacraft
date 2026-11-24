//! Radial and circular layouts.

use diacraft_layout::{Style, layout};

use crate::support::{graph, near, node, overlapping, row, settings};

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[test]
fn a_radial_tree_has_the_children_of_a_root_on_a_ring_about_it() {
    // A root and five children: the children at one distance from the root, all different places. Catches children in a row and a root off the middle of its ring.
    let g = graph(row(6), &[(0, 1), (0, 2), (0, 3), (0, 4), (0, 5)]);
    let at = layout(&g, &settings(Style::Radial, 1.0, 1.0)).unwrap();
    let r = distance(at[0], at[1]);
    assert!(r > 0.0);
    for kid in 2..6 {
        assert!(near(distance(at[0], at[kid]), r), "{at:?}");
    }
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
}

#[test]
fn the_rings_of_a_deeper_tree_are_farther_out_and_no_shapes_meet() {
    // A root, three children, each with two: the grandchildren are farther from the root than their parents.
    let nodes = row(10);
    let g = graph(nodes, &[(0, 1), (0, 2), (0, 3), (1, 4), (1, 5), (2, 6), (2, 7), (3, 8), (3, 9)]);
    let at = layout(&g, &settings(Style::Radial, 0.5, 0.5)).unwrap();
    for (parent, kid) in [(1, 4), (1, 5), (2, 6), (2, 7), (3, 8), (3, 9)] {
        assert!(distance(at[0], at[kid]) > distance(at[0], at[parent]) + 1.0, "{kid} is out past {parent}: {at:?}");
    }
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
}

#[test]
fn a_circle_has_every_shape_at_one_distance_from_its_middle_and_none_meeting() {
    // Eight shapes joined in a path: all on one circle. Catches shapes on two rings, and a circle too small for its shapes.
    let g = graph(row(8), &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)]);
    let at = layout(&g, &settings(Style::Circular, 1.0, 1.0)).unwrap();
    let (mx, my) = (at.iter().map(|p| p.0).sum::<f64>() / 8.0, at.iter().map(|p| p.1).sum::<f64>() / 8.0);
    let r = distance((mx, my), at[0]);
    for p in &at {
        assert!((distance((mx, my), *p) - r).abs() < 1e-6, "{at:?}");
    }
    assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{at:?}");
}

#[test]
fn two_shapes_on_a_circle_are_as_far_apart_as_their_circles_and_the_space_need() {
    // Two 3 by 4 shapes (the circle each fits in is 5 across) and a space of 2: 5 + 2 = 7 between the middles. Catches a radius that ignores the space.
    let g = graph(vec![node(3.0, 4.0, 0.0, 0.0), node(3.0, 4.0, 9.0, 0.0)], &[(0, 1)]);
    let at = layout(&g, &settings(Style::Circular, 2.0, 2.0)).unwrap();
    assert!(near(distance(at[0], at[1]), 7.0), "{at:?}");
}

#[test]
fn a_circle_of_a_thousand_and_a_radial_tree_of_a_thousand_keep_shapes_apart() {
    let n = 1_000_usize;
    let nodes: Vec<_> = (0..n).map(|i| node(1.0 + (i % 5) as f64, 1.0 + (i % 3) as f64, (i * 7 % 50) as f64, (i * 13 % 50) as f64)).collect();
    let edges: Vec<(u32, u32)> = (1..n as u32).map(|i| ((i - 1) / 4, i)).collect();
    let g = graph(nodes, &edges);
    for style in [Style::Radial, Style::Circular] {
        let at = layout(&g, &settings(style, 0.25, 0.25)).unwrap();
        assert!(overlapping(&g.nodes, &at, 1e-9).is_none(), "{style:?}");
    }
}

#[test]
fn shapes_of_no_size_and_no_space_are_laid_out_without_a_division_by_nothing() {
    // Catches a NaN from an angle of 0.
    let nodes = vec![node(0.0, 0.0, 0.0, 0.0); 6];
    let g = graph(nodes, &[(0, 1), (0, 2), (0, 3), (1, 4), (1, 5)]);
    for style in [Style::Radial, Style::Circular] {
        let at = layout(&g, &settings(style, 0.0, 0.0)).unwrap();
        assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{style:?}: {at:?}");
    }
}
