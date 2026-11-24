//! The org chart layout: a tree in which each manager says how its reports are set out, side by side under it or in a column to one side.
//! The rules are ours, so a vector is a property of the result.

use diacraft_layout::{Direction, Error, Node, OrgSettings, Spread, Work, org_chart, org_chart_with};
use proptest::prelude::*;

use crate::support::{near, node, overlapping, top_left};

fn at<'a>(direction: Direction, spreads: &'a [Spread], gap_x: f64, gap_y: f64) -> OrgSettings<'a> {
    OrgSettings { direction, spreads, gap_x, gap_y }
}

/// A boss (shape 0) over `kids` reports, all 2 by 1, put in a row as a person would have left them.
fn boss_and_reports(kids: usize) -> (Vec<Node>, Vec<(u32, u32)>) {
    let mut nodes = vec![node(2.0, 1.0, 0.0, 10.0)];
    let mut edges = Vec::new();
    for k in 0..kids {
        nodes.push(node(2.0, 1.0, 5.0 * k as f64, 0.0));
        edges.push((0, k as u32 + 1));
    }
    (nodes, edges)
}

fn graph(nodes: Vec<Node>, edges: Vec<(u32, u32)>) -> diacraft_layout::Graph {
    diacraft_layout::Graph { nodes, edges }
}

#[test]
fn across_puts_the_reports_side_by_side_under_the_boss() {
    // Catches reports that are not on one line, a boss that is not over them, and a space that is not kept.
    let (nodes, edges) = boss_and_reports(3);
    let g = graph(nodes, edges);
    let p = org_chart(&g, &at(Direction::TopToBottom, &[], 0.5, 1.0)).unwrap();
    assert!(near(p[1].1, p[2].1) && near(p[2].1, p[3].1), "{p:?}");
    assert!(p[0].1 > p[1].1, "the boss is above: {p:?}");
    assert!(near(p[0].0, (p[1].0 + p[3].0) / 2.0), "the boss is over the middle of its reports: {p:?}");
    assert!(near(p[2].0 - p[1].0, 2.5) && near(p[3].0 - p[2].0, 2.5), "2 wide and 0.5 apart: {p:?}");
    assert!(near(p[0].1 - p[1].1, 2.0), "1 high and 1.0 apart: {p:?}");
    assert!(p[1].0 < p[2].0 && p[2].0 < p[3].0, "the reports keep the order they have on the page: {p:?}");
}

#[test]
fn hanging_after_puts_the_reports_in_a_column_to_the_right_of_the_boss_line() {
    // Catches a column whose reports are side by side, one that is not under the boss, and one whose left edges are not on one line.
    let (nodes, edges) = boss_and_reports(3);
    let g = graph(nodes, edges);
    let spreads = [Spread::After, Spread::Across, Spread::Across, Spread::Across];
    let p = org_chart(&g, &at(Direction::TopToBottom, &spreads, 0.5, 0.25)).unwrap();
    assert!(near(p[1].0, p[2].0) && near(p[2].0, p[3].0), "one column: {p:?}");
    assert!(p[1].1 > p[2].1 && p[2].1 > p[3].1, "down the page in the order they had: {p:?}");
    assert!(near(p[1].1 - p[2].1, 1.25), "1 high and 0.25 apart: {p:?}");
    assert!(near(p[1].0 - 1.0, p[0].0 + 0.5), "the column's left edge is a space to the right of the boss's middle: {p:?}");
    assert!(p[0].1 > p[1].1, "{p:?}");
}

#[test]
fn hanging_before_is_the_mirror_of_hanging_after() {
    // Catches a column that goes to the same side whichever way it is asked for.
    let (nodes, edges) = boss_and_reports(3);
    let g = graph(nodes, edges);
    let after = org_chart(&g, &at(Direction::TopToBottom, &[Spread::After], 0.5, 0.25)).unwrap();
    let before = org_chart(&g, &at(Direction::TopToBottom, &[Spread::Before], 0.5, 0.25)).unwrap();
    for k in 1..4 {
        assert!(near(before[k].0 - before[0].0, -(after[k].0 - after[0].0)), "{after:?} {before:?}");
        assert!(near(before[k].1, after[k].1));
    }
}

#[test]
fn a_hanging_manager_among_managers_leaves_room() {
    // A boss over two managers; the first hangs three reports, the second has two across. Catches blocks that overlap and a second manager that is not to the right of the first's column.
    let nodes = vec![
        node(2.0, 1.0, 0.0, 20.0),
        node(2.0, 1.0, -5.0, 10.0),
        node(2.0, 1.0, 5.0, 10.0),
        node(2.0, 1.0, -8.0, 0.0),
        node(2.0, 1.0, -5.0, 0.0),
        node(2.0, 1.0, -2.0, 0.0),
        node(2.0, 1.0, 4.0, 0.0),
        node(2.0, 1.0, 7.0, 0.0),
    ];
    let edges = vec![(0, 1), (0, 2), (1, 3), (1, 4), (1, 5), (2, 6), (2, 7)];
    let g = graph(nodes.clone(), edges);
    let spreads = [Spread::Across, Spread::After, Spread::Across];
    let p = org_chart(&g, &at(Direction::TopToBottom, &spreads, 0.5, 0.5)).unwrap();
    assert!(overlapping(&nodes, &p, 1e-9).is_none(), "{p:?}");
    assert!(near(p[3].0, p[4].0) && near(p[4].0, p[5].0), "the hanging column of the first manager: {p:?}");
    assert!(p[1].0 < p[2].0, "{p:?}");
    assert!(near(p[6].1, p[7].1), "the second manager's reports are on one line: {p:?}");
    assert!(p[6].0 > p[3].0 + 2.0, "and clear of the first's column: {p:?}");
}

#[test]
fn the_flow_runs_the_way_the_direction_says() {
    // Catches a direction that is ignored or mirrored: bottom to top has the boss lowest, left to right has it leftmost, right to left rightmost.
    let (nodes, edges) = boss_and_reports(2);
    let g = graph(nodes, edges);
    let up = org_chart(&g, &at(Direction::BottomToTop, &[], 0.5, 0.5)).unwrap();
    assert!(up[0].1 < up[1].1 && near(up[1].1, up[2].1), "{up:?}");
    let right = org_chart(&g, &at(Direction::LeftToRight, &[], 0.5, 0.5)).unwrap();
    assert!(right[0].0 < right[1].0 && near(right[1].0, right[2].0), "{right:?}");
    assert!(near(right[0].1, (right[1].1 + right[2].1) / 2.0), "{right:?}");
    let left = org_chart(&g, &at(Direction::RightToLeft, &[], 0.5, 0.5)).unwrap();
    assert!(left[0].0 > left[1].0 && near(left[1].0, left[2].0), "{left:?}");
}

#[test]
fn the_corner_of_the_chart_stays_where_it_was() {
    // Catches a chart that jumps across the page when it is laid out again.
    let (nodes, edges) = boss_and_reports(4);
    let g = graph(nodes.clone(), edges);
    let p = org_chart(&g, &at(Direction::TopToBottom, &[Spread::After], 0.5, 0.5)).unwrap();
    let old = top_left(&nodes, &nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>());
    let new = top_left(&nodes, &p);
    assert!(near(old.0, new.0) && near(old.1, new.1), "{old:?} {new:?}");
}

#[test]
fn a_reporting_loop_and_a_second_boss_are_laid_out() {
    // A manages B manages A (a loop), C reports to both A and B, and a shape reports to itself. Catches a hang on a loop and a missing place.
    let nodes = vec![node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 3.0, 0.0), node(2.0, 1.0, 6.0, 0.0), node(2.0, 1.0, 9.0, 0.0)];
    let edges = vec![(0, 1), (1, 0), (0, 2), (1, 2), (3, 3)];
    let g = graph(nodes.clone(), edges);
    let p = org_chart(&g, &at(Direction::TopToBottom, &[], 0.5, 0.5)).unwrap();
    assert_eq!(p.len(), 4);
    assert!(overlapping(&nodes, &p, 1e-9).is_none(), "{p:?}");
}

#[test]
fn separate_charts_are_laid_out_apart() {
    // Two bosses with no link between them. Catches charts that are drawn over each other.
    let nodes = vec![node(2.0, 1.0, 0.0, 5.0), node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 20.0, 5.0), node(2.0, 1.0, 20.0, 0.0)];
    let g = graph(nodes.clone(), vec![(0, 1), (2, 3)]);
    let p = org_chart(&g, &at(Direction::TopToBottom, &[], 0.5, 0.5)).unwrap();
    assert!(overlapping(&nodes, &p, 1e-9).is_none(), "{p:?}");
}

#[test]
fn a_bad_graph_is_refused_in_words() {
    // Catches a NaN size, a connector to a shape that is not there and a space below nothing getting into the layout.
    let mut nodes = vec![node(2.0, 1.0, 0.0, 0.0), node(2.0, 1.0, 5.0, 0.0)];
    nodes[1].width = f64::NAN;
    let err = org_chart(&graph(nodes, vec![(0, 1)]), &at(Direction::TopToBottom, &[], 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::Invalid(words) if words.contains("shape 1")), "{err}");
    let err = org_chart(&graph(vec![node(1.0, 1.0, 0.0, 0.0)], vec![(0, 3)]), &at(Direction::TopToBottom, &[], 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::Invalid(words) if words.contains("shape 3")), "{err}");
    let err = org_chart(&graph(vec![node(1.0, 1.0, 0.0, 0.0)], vec![]), &at(Direction::TopToBottom, &[], -1.0, 1.0)).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
}

#[test]
fn a_chain_of_a_hundred_thousand_costs_no_stack_and_is_charged() {
    // Catches a layout that recurses (a chain this deep overflows the stack) and one that is not charged.
    let n = 100_000_usize;
    let nodes: Vec<Node> = (0..n).map(|i| node(2.0, 1.0, 0.0, -(i as f64))).collect();
    let edges: Vec<(u32, u32)> = (1..n as u32).map(|i| (i - 1, i)).collect();
    let g = graph(nodes, edges);
    let mut work = Work::new();
    let spreads = vec![Spread::After; n];
    let p = org_chart_with(&g, &at(Direction::TopToBottom, &spreads, 0.5, 0.5), &mut work).unwrap();
    assert_eq!(p.len(), n);
    assert!(work.used() >= n as u64, "{}", work.used());
    assert!(work.used() < 100 * n as u64, "{}", work.used());
}

#[test]
fn a_boss_with_ten_thousand_reports_is_laid_out_in_one_row_or_one_column() {
    // Catches a quadratic walk over a boss's reports: the work is counted, not timed.
    let (nodes, edges) = boss_and_reports(10_000);
    let g = graph(nodes, edges);
    for spreads in [vec![Spread::Across], vec![Spread::After]] {
        let mut work = Work::new();
        let p = org_chart_with(&g, &at(Direction::TopToBottom, &spreads, 0.5, 0.5), &mut work).unwrap();
        assert_eq!(p.len(), 10_001);
        assert!(work.used() < 40 * 10_001, "{}", work.used());
    }
}

#[test]
fn too_much_work_is_refused_before_it_is_done() {
    // Catches a pass that is not charged: a counter with almost nothing left refuses the layout.
    let (nodes, edges) = boss_and_reports(50);
    let g = graph(nodes, edges);
    let mut work = Work::new();
    work.charge(diacraft_layout::MAX_WORK - 10).unwrap();
    let err = org_chart_with(&g, &at(Direction::TopToBottom, &[], 0.5, 0.5), &mut work).unwrap_err();
    assert!(matches!(err, Error::TooLarge(_)), "{err}");
}

fn forest() -> impl Strategy<Value = (Vec<Node>, Vec<(u32, u32)>, Vec<u8>)> {
    (1_usize..40).prop_flat_map(|n| {
        let shapes = proptest::collection::vec((0.2_f64..6.0, 0.2_f64..4.0, -30.0_f64..30.0, -30.0_f64..30.0), n);
        let joins = proptest::collection::vec((0..n as u32, 0..n as u32), 0..(2 * n));
        let spreads = proptest::collection::vec(0_u8..3, n);
        (shapes, joins, spreads).prop_map(|(s, e, sp)| (s.into_iter().map(|(w, h, x, y)| Node { width: w, height: h, x, y }).collect(), e, sp))
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn no_two_boxes_overlap_and_a_second_layout_is_the_same((nodes, edges, sp) in forest(), d in 0_u8..4, gap in 0.1_f64..3.0) {
        let dir = [Direction::TopToBottom, Direction::BottomToTop, Direction::LeftToRight, Direction::RightToLeft][usize::from(d)];
        let spreads: Vec<Spread> = sp.iter().map(|k| [Spread::Across, Spread::After, Spread::Before][usize::from(*k)]).collect();
        let g = graph(nodes.clone(), edges);
        let s = at(dir, &spreads, gap, gap);
        let p = org_chart(&g, &s).unwrap();
        prop_assert_eq!(p.len(), nodes.len());
        prop_assert!(overlapping(&nodes, &p, 1e-6).is_none(), "{:?}: {:?}", dir, p);
        prop_assert_eq!(&p, &org_chart(&g, &s).unwrap());
        let (old, new) = (top_left(&nodes, &nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>()), top_left(&nodes, &p));
        prop_assert!((old.0 - new.0).abs() < 1e-6 && (old.1 - new.1).abs() < 1e-6);
    }
}
