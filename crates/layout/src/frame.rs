//! The frame a layer-by-layer layout works in: **across** (`c`, to the right of the flow) and **along** (`m`, the way the flow runs), whatever the direction of
//! the page's flow is. A style is laid out in it, and [`Frame::to_page`] turns the places back (the page's `y` runs up).

use crate::model::{Direction, Node, size};

/// One shape in the frame: its size across and along, and where it is now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Item {
    /// The size across the flow, and along it.
    pub cs: f64,
    pub ms: f64,
    /// Where the middle is now, across and along.
    pub c0: f64,
    pub m0: f64,
}

/// A direction of flow, as a map between the page and the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    dir: Direction,
}

impl Frame {
    pub(crate) fn new(dir: Direction) -> Frame {
        Frame { dir }
    }

    /// The frame of a flow down the page, which the styles with no direction (a radial tree, a circle) use.
    pub(crate) fn down() -> Frame {
        Frame { dir: Direction::TopToBottom }
    }

    /// Whether the flow runs along the page's x axis, so that a shape's width is its size along the flow.
    fn across_page(self) -> bool {
        matches!(self.dir, Direction::LeftToRight | Direction::RightToLeft)
    }

    /// The shape in the frame.
    pub(crate) fn item(self, node: &Node) -> Item {
        let (w, h) = (crate::model::size(node.width), size(node.height));
        let (c0, m0) = self.to_frame(node.x, node.y);
        if self.across_page() { Item { cs: h, ms: w, c0, m0 } } else { Item { cs: w, ms: h, c0, m0 } }
    }

    /// A place of the page (`y` up) in the frame.
    pub(crate) fn to_frame(self, x: f64, y: f64) -> (f64, f64) {
        match self.dir {
            Direction::TopToBottom => (x, -y),
            Direction::BottomToTop => (x, y),
            Direction::LeftToRight => (-y, x),
            Direction::RightToLeft => (-y, -x),
        }
    }

    /// A place of the frame on the page.
    pub(crate) fn to_page(self, c: f64, m: f64) -> (f64, f64) {
        match self.dir {
            Direction::TopToBottom => (c, -m),
            Direction::BottomToTop => (c, m),
            Direction::LeftToRight => (m, -c),
            Direction::RightToLeft => (-m, -c),
        }
    }

    /// The space across the flow and the space along it, from the page's spaces along x and along y.
    pub(crate) fn gaps(self, gap_x: f64, gap_y: f64) -> (f64, f64) {
        if self.across_page() { (gap_y, gap_x) } else { (gap_x, gap_y) }
    }
}
