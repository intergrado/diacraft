//! The stylesheet's default style of a cell (S-R7): what a shape or a connector has before its own style says anything.
//!
//! Resolution starts from a copy of the default of the cell's kind, **unless the style string starts with `;`** (S-R2), and then applies the string's pieces over it. The pairs are
//! written as the style strings they are and read by the same rules as any style, so that `default` is the colour keyword and a number is a number.

/// What a cell is, for the default it starts from (S-R2, S-R7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellKind {
    /// A shape: a vertex, a layer, a group, a connector's label or a port.
    Shape,
    /// A connector: `edge="1"`.
    Connector,
}

impl CellKind {
    /// The kind's place in the pair of tables a cache keeps.
    pub(crate) fn slot(self) -> usize {
        match self {
            CellKind::Shape => 0,
            CellKind::Connector => 1,
        }
    }
}

/// What every shape has before its own style (S-R7): `shape=label`, the rectangle perimeter, 12 px Helvetica centred in the middle of its box, and `default` colours.
const SHAPE: &[(&str, &str)] = &[
    ("shape", "label"),
    ("perimeter", "rectanglePerimeter"),
    ("fontSize", "12"),
    ("fontFamily", "Helvetica"),
    ("align", "center"),
    ("verticalAlign", "middle"),
    ("fillColor", "default"),
    ("strokeColor", "default"),
    ("fontColor", "default"),
];

/// What every connector has before its own style (S-R7): `shape=connector`, a `default` label background, a `classic` end mark, 11 px Helvetica centred, rounded bends, and `default`
/// line and text colours.
const CONNECTOR: &[(&str, &str)] = &[
    ("shape", "connector"),
    ("labelBackgroundColor", "default"),
    ("endArrow", "classic"),
    ("fontSize", "11"),
    ("fontFamily", "Helvetica"),
    ("align", "center"),
    ("verticalAlign", "middle"),
    ("rounded", "1"),
    ("strokeColor", "default"),
    ("fontColor", "default"),
];

/// The default style of a cell of `kind`, as the pairs to apply in order.
pub fn pairs(kind: CellKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        CellKind::Shape => SHAPE,
        CellKind::Connector => CONNECTOR,
    }
}
