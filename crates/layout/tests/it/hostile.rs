//! Hostile diagrams: bad numbers, too many shapes, rings, deep chains, long connectors. Each is refused in words or finishes, and costs what the work counter says.

use diacraft_layout::{Align, Direction, Error, MAX_EDGES, MAX_NODES, Style, Work, layout, layout_with};

use crate::support::{graph, node, row, settings};

fn all_styles() -> Vec<Style> {
    vec![
        Style::Flowchart(Direction::TopToBottom),
        Style::Flowchart(Direction::RightToLeft),
        Style::Hierarchy(Direction::BottomToTop, Align::End),
        Style::CompactTree { run: diacraft_layout::Side::Down, level: diacraft_layout::Side::Right },
        Style::Radial,
        Style::Circular,
    ]
}

#[test]
fn a_size_or_a_place_that_is_not_a_number_is_refused_in_words() {
    // Catches a NaN that gets into a sort and a place that is not a number.
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for which in 0..4 {
            let mut nodes = row(3);
            match which {
                0 => nodes[1].width = bad,
                1 => nodes[1].height = bad,
                2 => nodes[1].x = bad,
                _ => nodes[1].y = bad,
            }
            let g = graph(nodes, &[(0, 1), (1, 2)]);
            for style in all_styles() {
                let err = layout(&g, &settings(style, 1.0, 1.0)).unwrap_err();
                assert!(matches!(&err, Error::Invalid(words) if words.contains("shape 1")), "{style:?}: {err}");
            }
        }
    }
}

#[test]
fn a_connector_to_a_shape_that_is_not_there_is_refused_in_words() {
    let g = graph(row(2), &[(0, 5)]);
    let err = layout(&g, &settings(Style::Radial, 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::Invalid(words) if words.contains("shape 5")), "{err}");
}

#[test]
fn a_space_that_is_not_a_number_or_is_below_nothing_is_refused() {
    for gap in [f64::NAN, -1.0, f64::INFINITY] {
        let err = layout(&graph(row(2), &[(0, 1)]), &settings(Style::Circular, gap, 1.0)).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{gap}: {err}");
    }
}

#[test]
fn more_shapes_or_connectors_than_a_layout_takes_are_refused_with_the_limit() {
    // Catches a limit that is checked after the allocation it guards: the refusal is of the counts alone.
    let g = graph(vec![node(1.0, 1.0, 0.0, 0.0); MAX_NODES + 1], &[]);
    let err = layout(&g, &settings(Style::Circular, 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::TooLarge(words) if words.contains(&MAX_NODES.to_string())), "{err}");
    let g = graph(row(3), &vec![(0, 1); MAX_EDGES + 1]);
    let err = layout(&g, &settings(Style::Circular, 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::TooLarge(words) if words.contains(&MAX_EDGES.to_string())), "{err}");
}

#[test]
fn connectors_that_cross_a_great_many_layers_between_them_are_refused_before_they_are_made() {
    // A chain of 2,000 shapes (2,000 layers) and 300 connectors, each from the first to the last: 300 times 1,998 stand-in points, more than the limit. Only the
    // flowchart makes stand-ins; the refusal says what to do.
    let n = 2_000_u32;
    let mut edges: Vec<(u32, u32)> = (0..n - 1).map(|i| (i, i + 1)).collect();
    for from in 0..20 {
        for to in 0..15 {
            edges.push((from, n - 1 - to));
        }
    }
    let g = graph(vec![node(1.0, 1.0, 0.0, 0.0); n as usize], &edges);
    let err = layout(&g, &settings(Style::Flowchart(Direction::TopToBottom), 1.0, 1.0)).unwrap_err();
    assert!(matches!(&err, Error::TooLarge(words) if words.contains("layers")), "{err}");
    // The same graph in a hierarchy needs no stand-ins and is laid out.
    assert!(layout(&g, &settings(Style::Hierarchy(Direction::TopToBottom, Align::Center), 1.0, 1.0)).is_ok());
}

#[test]
fn a_ring_of_a_hundred_thousand_shapes_is_laid_out_in_every_style() {
    // Catches a walk that recurses along the ring and a cycle that is never broken.
    let n = 100_000_u32;
    let edges: Vec<(u32, u32)> = (0..n).map(|i| (i, (i + 1) % n)).collect();
    let g = graph(vec![node(1.0, 1.0, 0.0, 0.0); n as usize], &edges);
    for style in all_styles() {
        let at = layout(&g, &settings(style, 1.0, 1.0)).unwrap();
        assert_eq!(at.len(), n as usize);
        assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{style:?}");
    }
}

#[test]
fn a_hundred_thousand_shapes_nothing_joins_are_laid_out_in_a_block() {
    // Many parts: each gets a place in a row. 100,000 parts, and the work counted is a few steps each.
    let n = 100_000_usize;
    let g = graph(vec![node(1.0, 1.0, 0.0, 0.0); n], &[]);
    let mut work = Work::new();
    let at = layout_with(&g, &settings(Style::Flowchart(Direction::TopToBottom), 1.0, 1.0), &mut work).unwrap();
    assert_eq!(at.len(), n);
    assert!(work.used() <= 120 * n as u64, "work {} for {n} shapes", work.used());
}

#[test]
fn the_work_grows_in_step_with_the_diagram() {
    // Counted, not timed: a diagram of twice the size charges at most about twice the work, in every style. Catches a pass that is quadratic.
    for style in all_styles() {
        let work_for = |n: u32| {
            let edges: Vec<(u32, u32)> = (1..n).map(|i| (i / 2, i)).collect();
            let g = graph(vec![node(1.0, 1.0, 0.0, 0.0); n as usize], &edges);
            let mut work = Work::new();
            layout_with(&g, &settings(style, 1.0, 1.0), &mut work).unwrap();
            work.used()
        };
        let (small, big) = (work_for(5_000), work_for(10_000));
        assert!(big <= small * 21 / 10, "{style:?}: {small} for 5,000 shapes, {big} for 10,000");
    }
}

#[test]
fn a_dense_diagram_of_a_thousand_shapes_and_twenty_thousand_connectors_is_laid_out() {
    let n = 1_000_u32;
    let edges: Vec<(u32, u32)> = (0..20_000_u32).map(|i| ((i * 7919) % n, (i * 104_729 + 13) % n)).collect();
    let nodes: Vec<_> = (0..n).map(|i| node(2.0, 1.0, f64::from(i % 40), f64::from(i / 40))).collect();
    let g = graph(nodes, &edges);
    for style in all_styles() {
        let at = layout(&g, &settings(style, 1.0, 1.0)).unwrap();
        assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{style:?}");
    }
}

#[test]
fn shapes_of_the_largest_size_are_laid_out_to_numbers() {
    let nodes = vec![node(1.0e7, 1.0e7, 0.0, 0.0), node(1.0e300, -5.0, 0.0, 0.0), node(1.0, 1.0, 1.0e300, -1.0e300)];
    let g = graph(nodes, &[(0, 1), (1, 2)]);
    for style in all_styles() {
        if let Ok(at) = layout(&g, &settings(style, 1.0e300, 1.0)) {
            assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{style:?}");
        }
    }
}

#[test]
fn no_shapes_give_no_places() {
    for style in all_styles() {
        assert!(layout(&graph(Vec::new(), &[]), &settings(style, 1.0, 1.0)).unwrap().is_empty());
    }
}
