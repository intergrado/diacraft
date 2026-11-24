//! Builders and checks the tests share.

use diacraft_layout::{Graph, Node, Settings, Style};

/// A shape of `w` by `h` with its middle at (`x`, `y`).
pub fn node(w: f64, h: f64, x: f64, y: f64) -> Node {
    Node { width: w, height: h, x, y }
}

/// `count` shapes of 2 by 1, put in a row at the places a person would have left them (x = 5 i, y = 0).
pub fn row(count: usize) -> Vec<Node> {
    (0..count).map(|i| node(2.0, 1.0, 5.0 * i as f64, 0.0)).collect()
}

pub fn graph(nodes: Vec<Node>, edges: &[(u32, u32)]) -> Graph {
    Graph { nodes, edges: edges.to_vec() }
}

pub fn settings(style: Style, gap_x: f64, gap_y: f64) -> Settings {
    Settings { style, gap_x, gap_y }
}

/// How far apart two boxes are along the axis where they are further apart: negative when they overlap on both.
pub fn clearance(a: &Node, pa: (f64, f64), b: &Node, pb: (f64, f64)) -> f64 {
    let dx = (pa.0 - pb.0).abs() - (a.width + b.width) / 2.0;
    let dy = (pa.1 - pb.1).abs() - (a.height + b.height) / 2.0;
    dx.max(dy)
}

/// The pair of shapes (by number) whose boxes overlap by more than `eps`, if any. Quadratic: for graphs of a few hundred shapes.
pub fn overlapping(nodes: &[Node], places: &[(f64, f64)], eps: f64) -> Option<(usize, usize)> {
    for i in 0..nodes.len() {
        for j in i + 1..nodes.len() {
            if clearance(&nodes[i], places[i], &nodes[j], places[j]) < -eps {
                return Some((i, j));
            }
        }
    }
    None
}

/// The top-left corner of the boxes at `places`.
pub fn top_left(nodes: &[Node], places: &[(f64, f64)]) -> (f64, f64) {
    let left = nodes.iter().zip(places).map(|(n, p)| p.0 - n.width / 2.0).fold(f64::INFINITY, f64::min);
    let top = nodes.iter().zip(places).map(|(n, p)| p.1 + n.height / 2.0).fold(f64::NEG_INFINITY, f64::max);
    (left, top)
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}
