//! What a one-dimensional master looks like: its line ends, its pattern and its route (`docs/editing-commands.md`, "DiaCraft's own stencils, second batch").
//!
//! A connector master used to be one fixed thing, a line with a solid arrowhead at its end (the Gane-Sarson Data Flow). It is now data: a [`ConnectorLook`] says which end sits at
//! the begin and at the end, the line pattern (and whether the master guards it, as BPMN's flows do), the route and the weight. The cells go on the **master's** shape, and an instance
//! inherits them (an instance holds only what differs), so a drop writes none of them.
#![allow(dead_code, reason = "the vocabulary a connector master is written in (STENCIL2): a variant is used when the family that needs it lands")]

use diacraft_doc::{Cell, Value};

use super::{number_text, plain};

/// The line end a custom end (a master of ours or of a file, `USE("...")`) draws as in a plain reader: the number the cell holds for it.
pub(crate) const CUSTOM_END: u8 = 254;

/// The most a line-end number is, before the custom end: the 45 numbered ends run 0 to 45.
pub(crate) const MAX_NUMBERED_END: u8 = 45;

/// What sits at one end of a connector master.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineEnd {
    /// No mark: the cell holds 0.
    None,
    /// One of the numbers.
    Number(u8),
    /// A formula of the shape's Shape Data (a crow's foot cardinality, BPMN's condition), with the number it gives for the Shape Data the master holds.
    Formula { formula: &'static str, value: u8 },
    /// A hidden master of ours drawn as the end (`USE("<name>")`, which holds the custom number [`CUSTOM_END`]). `id` is the built-in master's id, which the connector `brings`.
    Master { id: &'static str, name: &'static str },
}

impl LineEnd {
    /// The number the end's cell resolves to.
    pub(crate) fn number(&self) -> u8 {
        match self {
            LineEnd::None => 0,
            LineEnd::Number(n) => *n,
            LineEnd::Formula { value, .. } => *value,
            LineEnd::Master { .. } => CUSTOM_END,
        }
    }

    /// The cell: a plain number, or a formula with the value it gives.
    fn cell(&self) -> Cell {
        match self {
            LineEnd::None => plain(0.0),
            LineEnd::Number(n) => plain(f64::from(*n)),
            LineEnd::Formula { formula, value } => Cell::with_formula(formula, Some(Value::from_text(&value.to_string()))),
            LineEnd::Master { name, .. } => {
                Cell::with_formula(&format!("USE(\"{}\")", name.replace('"', "\"\"")), Some(Value::from_text(&CUSTOM_END.to_string())))
            }
        }
    }

    /// Whether the end is in the set a catalogue master may hold: a number gives, a formula whose value is one, or a master of ours (the custom number).
    pub(crate) fn is_known(&self) -> bool {
        self.number() <= MAX_NUMBERED_END || matches!(self, LineEnd::Master { .. })
    }
}

/// How a connector master is routed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// The page's route style: the master writes none.
    Page,
    /// Straight between its ends (`ShapeRouteStyle` 2, as the Connectors menu's Straight writes it).
    Straight,
    /// Right-angled (`ShapeRouteStyle` 1).
    RightAngle,
}

/// A number or a formula of a flag or a pattern cell of a master (`NoShow`, `NoLine`, a `LinePattern`): constant 0 or 1, or a formula of the shape's Shape Data, which the master's cells settle
/// to the value it gives at the default Shape Data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flag {
    Off,
    On,
    Formula(&'static str),
}

impl Flag {
    /// The cell: 0, 1, or the formula with no value yet (the master's build settles it).
    pub(crate) fn cell(&self) -> Cell {
        match self {
            Flag::Off => plain(0.0),
            Flag::On => plain(1.0),
            Flag::Formula(formula) => Cell::with_formula(formula, None),
        }
    }
}

/// The look of a one-dimensional master.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ConnectorLook {
    pub begin: LineEnd,
    pub end: LineEnd,
    /// The pattern table: 1 solid, 2 dash, 3 dot, 4 dash-dot.
    pub pattern: u8,
    /// The master writes `LinePattern` as `GUARD(n)`, so that no format command changes it (BPMN's flows).
    pub guard_pattern: bool,
    pub route: Route,
    /// The line weight in points, when the master sets one.
    pub weight_pt: Option<f64>,
    /// `CompoundType` (a double line is 1), when the master sets one.
    pub compound: Option<u8>,
}

impl ConnectorLook {
    /// The look the one connector master of before had (the Gane-Sarson Data Flow): no begin end, a solid arrowhead at the end, a solid line, the page's route.
    pub(crate) const DATA_FLOW: ConnectorLook = ConnectorLook::plain(LineEnd::None, LineEnd::Number(4));

    /// A solid line in the page's route with these ends.
    pub(crate) const fn plain(begin: LineEnd, end: LineEnd) -> ConnectorLook {
        ConnectorLook { begin, end, pattern: 1, guard_pattern: false, route: Route::Page, weight_pt: None, compound: None }
    }

    /// The numbers `stencil.list` gives a connector master as `ends: {begin, end, pattern}`: the ends and the pattern its cells resolve to.
    pub(crate) fn numbers(&self) -> (u8, u8, u8) {
        (self.begin.number(), self.end.number(), self.pattern)
    }

    /// The cells the master's shape holds for this look, besides its ends' places.
    ///
    /// `BeginArrow`, `EndArrow` and `FillPattern` are always there, as they were for the one connector master of before. The others are there only when the look says something: a solid
    /// line holds no `LinePattern` (the style's is solid) unless the master guards it.
    pub(crate) fn cells(&self) -> Vec<(&'static str, Cell)> {
        let mut cells = vec![("BeginArrow", self.begin.cell()), ("EndArrow", self.end.cell()), ("FillPattern", plain(0.0))];
        if self.pattern != 1 || self.guard_pattern {
            let n = number_text(f64::from(self.pattern));
            let cell = if self.guard_pattern {
                Cell::with_formula(&format!("GUARD({n})"), Some(diacraft_doc::Value::from_text(&n)))
            } else {
                plain(f64::from(self.pattern))
            };
            cells.push(("LinePattern", cell));
        }
        if let Some(points) = self.weight_pt {
            cells.push(("LineWeight", Cell::constant(Value::from_text(&number_text(points / 72.0))).with_unit("IN")));
        }
        if let Some(compound) = self.compound {
            cells.push(("CompoundType", plain(f64::from(compound))));
        }
        match self.route {
            Route::Page => {}
            Route::Straight => {
                cells.push(("ShapeRouteStyle", plain(2.0)));
                cells.push(("ConLineRouteExt", plain(1.0)));
            }
            Route::RightAngle => {
                cells.push(("ShapeRouteStyle", plain(1.0)));
                cells.push(("ConLineRouteExt", plain(1.0)));
            }
        }
        cells
    }
}
