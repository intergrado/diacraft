//! What a layout takes and gives: the graph, the settings and the refusals.

use std::fmt;

/// The most shapes one layout places.
pub const MAX_NODES: usize = 100_000;

/// The most connectors one layout reads (each joins two of the shapes).
pub const MAX_EDGES: usize = 400_000;

/// The most steps of work one layout makes, counted by [`crate::Work`] before each pass (a step is one shape or one connector visited once).
pub const MAX_WORK: u64 = 200_000_000;

/// The most stand-in nodes a flowchart makes for the layers its connectors cross: a connector that runs over `k` layers has `k - 1` of them.
pub const MAX_DUMMIES: usize = 400_000;

/// The most a size or a gap is taken as, in the caller's unit: past it, the sum of 100,000 of them could not be a number.
pub const MAX_SIZE: f64 = 1.0e7;

/// One shape: its box and where its middle is now. Lengths are the caller's unit (the engine gives drawing units), `y` runs up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub width: f64,
    pub height: f64,
    /// The middle of the box now. The layout starts from this order, so a diagram is changed no more than its connectors ask.
    pub x: f64,
    pub y: f64,
}

/// The shapes and the connectors between them. A connector goes **from** its begin **to** its end; one that joins a shape to itself, and a repeated one, are
/// harmless and ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// `(from, to)`, each an index of `nodes`.
    pub edges: Vec<(u32, u32)>,
}

/// The way a flow runs, which is the way the connectors point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    TopToBottom,
    BottomToTop,
    LeftToRight,
    RightToLeft,
}

/// Where a parent sits over its children in a hierarchy: with the first of them (`Start`: the left of a flow down or up, the top of one across), in the middle of
/// them, or with the last (`End`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// A side of the page, for the compact tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Up,
    Down,
    Left,
    Right,
}

impl Side {
    /// Whether the side is up or down.
    pub fn is_vertical(self) -> bool {
        matches!(self, Side::Up | Side::Down)
    }
}

/// How the shapes are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Layers of shapes along the flow, a connector pointing from one layer to a later one: the shapes ordered in their layers so that connectors cross as little
    /// as the pass finds, and set across so that connectors run straight where they can.
    Flowchart(Direction),
    /// A tree (the connectors from the shapes that nothing points to, each shape taken once): a parent over its children, which share the width their own subtrees need.
    Hierarchy(Direction, Align),
    /// A tree as an outline: the shapes of a parent's children one after another along `run`, each level set off from the one before along `level`, which is across
    /// `run`. `CompactTree { run: Down, level: Right }` is a list read down the page with each level indented to the right.
    CompactTree { run: Side, level: Side },
    /// A tree in rings about its root, a shape's children in the arc of the ring that its own subtree needs.
    Radial,
    /// Every shape of a part of the diagram on one circle.
    Circular,
}

/// How a manager's reports are set out in an org chart (`org.rs`), in the flow's frame: across is the way a row of shapes runs, to the right of a flow down the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spread {
    /// The reports side by side under the manager, which is over the middle of them.
    #[default]
    Across,
    /// The reports in a column under the manager, a space to the **right** of its middle line (for a flow down the page; the side of the frame's across axis).
    After,
    /// The reports in a column under the manager, a space to the **left** of its middle line.
    Before,
}

/// What an org chart layout is asked: the way the chart flows, how each shape's reports are set out, and the spaces between shapes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrgSettings<'a> {
    pub direction: Direction,
    /// The [`Spread`] of each shape, by its number in the graph; a shape past the end of the list is [`Spread::Across`].
    pub spreads: &'a [Spread],
    /// As [`Settings`]: the least space along the page's x and along its y, each a length of at least 0.
    pub gap_x: f64,
    pub gap_y: f64,
}

impl OrgSettings<'_> {
    pub(crate) fn check(&self) -> Result<(), Error> {
        if !(self.gap_x.is_finite() && self.gap_y.is_finite() && self.gap_x >= 0.0 && self.gap_y >= 0.0) {
            return Err(Error::Invalid("the spaces between shapes must be numbers of at least 0".to_owned()));
        }
        Ok(())
    }

    /// The gap that is used: cut to [`MAX_SIZE`].
    pub(crate) fn gaps(&self) -> (f64, f64) {
        (self.gap_x.min(MAX_SIZE), self.gap_y.min(MAX_SIZE))
    }
}

/// What a layout is asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub style: Style,
    /// The least space between two shapes across a layer or along an outline's run (`AvenueSizeX`) and the least between two layers (`AvenueSizeY`), for a flow
    /// along y; they swap for a flow along x. Each is a length of at least 0.
    pub gap_x: f64,
    pub gap_y: f64,
}

/// Why a layout was not made. The shapes are not changed: a layout only gives places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// What was given is not a graph that can be laid out: a size or a place that is not a number, an end of a connector that names no shape, a compact tree whose two
    /// sides are on the same axis. The words say which.
    Invalid(String),
    /// The diagram is larger than a layout is made for. The words say the limit.
    TooLarge(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(words) | Error::TooLarge(words) => f.write_str(words),
        }
    }
}

impl std::error::Error for Error {}

/// The work a layout has charged, and the most it may. A pass charges before it runs, so a layout that would pass the limit stops as a refusal, and no pass starts
/// that it could not finish.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    used: u64,
}

impl Work {
    pub fn new() -> Work {
        Work { used: 0 }
    }

    /// Steps charged so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Charges `steps` more.
    pub fn charge(&mut self, steps: u64) -> Result<(), Error> {
        match charged(self.used, steps) {
            Some(total) => {
                self.used = total;
                Ok(())
            }
            None => Err(Error::TooLarge(format!("the layout would take more than {MAX_WORK} steps of work: lay out part of the diagram"))),
        }
    }
}

/// The steps charged once `steps` more are charged on `used`, or `None` past [`MAX_WORK`]: [`Work::charge`]'s arithmetic.
pub(crate) fn charged(used: u64, steps: u64) -> Option<u64> {
    used.checked_add(steps).filter(|total| *total <= MAX_WORK)
}

impl Graph {
    /// Checks what [`crate::layout`] needs before it allocates anything: the counts within [`MAX_NODES`] and [`MAX_EDGES`], every size and place a number
    /// (a size is cut to `0..=MAX_SIZE`), and every end of a connector a shape.
    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.nodes.len() > MAX_NODES {
            return Err(Error::TooLarge(format!("a layout places at most {MAX_NODES} shapes; this diagram has {}", self.nodes.len())));
        }
        if self.edges.len() > MAX_EDGES {
            return Err(Error::TooLarge(format!("a layout reads at most {MAX_EDGES} connectors; this diagram has {}", self.edges.len())));
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if !(n.width.is_finite() && n.height.is_finite() && n.x.is_finite() && n.y.is_finite()) {
                return Err(Error::Invalid(format!("shape {i} has a size or a place that is not a number")));
            }
        }
        let count = self.nodes.len();
        if let Some((a, b)) = self.edges.iter().find(|(a, b)| (*a as usize) >= count || (*b as usize) >= count) {
            return Err(Error::Invalid(format!("a connector joins shape {a} to shape {b}, and the diagram has {count} shapes")));
        }
        Ok(())
    }
}

impl Settings {
    pub(crate) fn check(&self) -> Result<(), Error> {
        if !(self.gap_x.is_finite() && self.gap_y.is_finite() && self.gap_x >= 0.0 && self.gap_y >= 0.0) {
            return Err(Error::Invalid("the spaces between shapes must be numbers of at least 0".to_owned()));
        }
        if let Style::CompactTree { run, level } = self.style
            && run.is_vertical() == level.is_vertical()
        {
            return Err(Error::Invalid("a compact tree runs along one axis and sets its levels off along the other".to_owned()));
        }
        Ok(())
    }

    /// The gap that is used: cut to [`MAX_SIZE`].
    pub(crate) fn gaps(&self) -> (f64, f64) {
        (self.gap_x.min(MAX_SIZE), self.gap_y.min(MAX_SIZE))
    }
}

/// A size as a layout uses it: not below 0, not above [`MAX_SIZE`].
pub(crate) fn size(v: f64) -> f64 {
    v.clamp(0.0, MAX_SIZE)
}
