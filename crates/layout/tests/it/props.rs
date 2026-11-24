//! Random diagrams: whatever the connectors (rings, repeats, a shape joined to itself), every style gives a place for every shape, no two boxes overlap, and a second
//! layout of the same diagram is the same.

use diacraft_layout::{Align, Direction, Node, Side, Style, layout};
use proptest::prelude::*;

use crate::support::{graph, overlapping, settings, top_left};

fn style_of(k: u8) -> Style {
    match k % 14 {
        0 => Style::Flowchart(Direction::TopToBottom),
        1 => Style::Flowchart(Direction::LeftToRight),
        2 => Style::Flowchart(Direction::BottomToTop),
        3 => Style::Flowchart(Direction::RightToLeft),
        4 => Style::Hierarchy(Direction::TopToBottom, Align::Start),
        5 => Style::Hierarchy(Direction::LeftToRight, Align::Center),
        6 => Style::Hierarchy(Direction::RightToLeft, Align::End),
        7 => Style::Hierarchy(Direction::BottomToTop, Align::Center),
        8 => Style::CompactTree { run: Side::Down, level: Side::Right },
        9 => Style::CompactTree { run: Side::Left, level: Side::Up },
        10 => Style::Radial,
        11 => Style::Circular,
        12 => Style::CompactTree { run: Side::Right, level: Side::Down },
        _ => Style::CompactTree { run: Side::Up, level: Side::Left },
    }
}

fn diagram() -> impl Strategy<Value = (Vec<Node>, Vec<(u32, u32)>)> {
    (1_usize..40).prop_flat_map(|n| {
        let shapes = proptest::collection::vec((0.2_f64..6.0, 0.2_f64..4.0, -30.0_f64..30.0, -30.0_f64..30.0), n);
        let joins = proptest::collection::vec((0..n as u32, 0..n as u32), 0..(3 * n));
        (shapes, joins).prop_map(|(s, e)| (s.into_iter().map(|(w, h, x, y)| Node { width: w, height: h, x, y }).collect(), e))
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    #[test]
    fn no_two_boxes_overlap_and_the_corner_stays((nodes, edges) in diagram(), k in 0_u8..14, gap in 0.1_f64..3.0) {
        let g = graph(nodes, &edges);
        let at = layout(&g, &settings(style_of(k), gap, gap)).unwrap();
        prop_assert_eq!(at.len(), g.nodes.len());
        prop_assert!(overlapping(&g.nodes, &at, 1e-6).is_none(), "{:?}: {:?}", style_of(k), at);
        let (old, new) = (top_left(&g.nodes, &g.nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>()), top_left(&g.nodes, &at));
        prop_assert!((old.0 - new.0).abs() < 1e-6 && (old.1 - new.1).abs() < 1e-6, "{:?}", style_of(k));
    }

    #[test]
    fn a_layout_is_the_same_the_second_time((nodes, edges) in diagram(), k in 0_u8..14) {
        let g = graph(nodes, &edges);
        let s = settings(style_of(k), 1.0, 1.0);
        prop_assert_eq!(layout(&g, &s).unwrap(), layout(&g, &s).unwrap());
    }
}
