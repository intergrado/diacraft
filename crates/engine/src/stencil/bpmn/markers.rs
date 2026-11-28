//! The markers of BPMN Shapes: the small figures an event, a task, a gateway or a data object shows inside its outline, drawn in code as strokes and fills.
//!
//! **Original figures** (what each marker is, in words: an envelope, a clock face, a page with lines, a triangle, a pentagon, a plus, a zigzag bolt, an upward
//! arrowhead, an X, two rewind triangles, an arrow, a disc, a gear pair, a head and shoulders, a hand, a small table, a curled arrow, three bars, a tilde). Every figure is a handful of
//! points in the **unit square** made here, and nothing is traced or copied from a standard's figure or another application's.
//!
//! **Two forms of a figure.** A [`Glyph`] has a *hollow* form, which is outlines only (a catching event's marker, a task's type marker), and a *dark* form for the markers that are filled
//! (a throwing or ending event's, a send task's envelope): the filled shapes of the figure, each its own `Geometry` section, and the strokes that stay strokes. A shape has one fill, so a
//! figure that is both light and dark is two members of the master, one of each form, and which shows is the `NoShow` formula of each of its sections. The dark members fill with
//! their own line colour (`LineColor`), so the marker is as dark as the outline under any theme (the design's 4.7) and takes the line colour a person puts on the figure, which a format
//! writes in the members.
//!
//! **Where a figure goes** ([`Place`]). In a member of its own the figure fills the member's box and a point is a share of `Width` and `Height`. In a shape that draws its own markers (the
//! expanded sub-process) it is a square of fixed size at a place, and a point is that place and a length in inches.

use super::super::{Co, Flag, OwnedPath, Seg, fk, fw};

/// What a glyph is drawn from, in the unit square.
#[derive(Clone, Copy, Debug)]
pub(super) enum Prim {
    /// A polygon: the path comes back to its first point.
    Closed(&'static [(f64, f64)]),
    /// A run of lines.
    Open(&'static [(f64, f64)]),
    /// A circle: its centre and its radius.
    Circle(f64, f64, f64),
}

/// A figure: its hollow form, and its dark form where it has one (a figure that is only ever hollow has none).
#[derive(Debug)]
pub(super) struct Glyph {
    pub hollow: &'static [Prim],
    pub dark: &'static [Prim],
}

/// Where a figure is put.
#[derive(Clone, Copy, Debug)]
pub(super) enum Place {
    /// The figure fills its member's box: a point is a share of the box.
    Share,
    /// The figure is a square of `size` inches whose lower left corner is at (`x`, `y`): a point is that place and a length.
    Fixed { x: Co, y: Co, size: f64 },
}

impl Place {
    /// The coordinate of the unit square's `u` along x.
    fn x(self, u: f64) -> Co {
        match self {
            Place::Share => fw(u),
            Place::Fixed { x, size, .. } => x.plus(fk(u * size)),
        }
    }

    /// The coordinate of the unit square's `v` along y.
    fn y(self, v: f64) -> Co {
        match self {
            Place::Share => super::super::fh(v),
            Place::Fixed { y, size, .. } => y.plus(fk(v * size)),
        }
    }

    /// A length of `u` of the square's width, as a semi-axis along x.
    fn rx(self, u: f64) -> Co {
        match self {
            Place::Share => fw(u),
            Place::Fixed { size, .. } => fk(u * size),
        }
    }

    /// A length of `v` of the square's height, as a semi-axis along y.
    fn ry(self, v: f64) -> Co {
        match self {
            Place::Share => super::super::fh(v),
            Place::Fixed { size, .. } => fk(v * size),
        }
    }
}

type Xy = (Co, Co);

/// The segments of a polygon, back to its first point.
fn polygon(points: &[(f64, f64)], at: Place) -> Vec<Seg> {
    let mut segs = Vec::with_capacity(points.len() + 1);
    for (i, (u, v)) in points.iter().chain(points.first()).enumerate() {
        let (x, y) = (at.x(*u), at.y(*v));
        segs.push(if i == 0 { Seg::Move(x, y) } else { Seg::Line(x, y) });
    }
    segs
}

/// The segments of a run of lines.
fn run(points: &[(f64, f64)], at: Place) -> Vec<Seg> {
    points
        .iter()
        .enumerate()
        .map(|(i, (u, v))| {
            let (x, y) = (at.x(*u), at.y(*v));
            if i == 0 { Seg::Move(x, y) } else { Seg::Line(x, y) }
        })
        .collect()
}

/// The segments of a circle: two half arcs, as the kit's stick figure's head is.
fn circle(cx: f64, cy: f64, r: f64, at: Place) -> Vec<Seg> {
    let (left, right): (Xy, Xy) = ((at.x(cx - r), at.y(cy)), (at.x(cx + r), at.y(cy)));
    let (rx, ry) = (at.rx(r), at.ry(r));
    vec![
        Seg::Move(left.0, left.1),
        Seg::Half { to: right, via: (at.x(cx), at.y(cy + r)), rx, ry },
        Seg::Half { to: left, via: (at.x(cx), at.y(cy - r)), rx, ry },
    ]
}

fn prim_segs(prim: &Prim, at: Place) -> Vec<Seg> {
    match prim {
        Prim::Closed(points) => polygon(points, at),
        Prim::Open(points) => run(points, at),
        Prim::Circle(cx, cy, r) => circle(*cx, *cy, *r, at),
    }
}

/// The sections that draw `glyph` in its dark form (`dark`) or its hollow one, all hidden by `hide` (`NoShow`: a formula of the shape's Shape Data, or a constant).
///
/// * **Hollow**: one section of strokes, every polygon and circle of the figure a run in it, with no fill.
/// * **Dark**: each polygon and circle of the dark form a filled section of its own (a filled path is one closed run), and the open runs one section of strokes.
pub(super) fn sections(glyph: &Glyph, dark: bool, at: Place, hidden: Flag) -> Vec<OwnedPath> {
    if !dark {
        let segs: Vec<Seg> = glyph.hollow.iter().flat_map(|p| prim_segs(p, at)).collect();
        return vec![OwnedPath { fill: false, segs, no_show: hidden, no_line: Flag::Off }];
    }
    let mut out: Vec<OwnedPath> = Vec::new();
    let mut strokes: Vec<Seg> = Vec::new();
    for prim in glyph.dark {
        match prim {
            Prim::Open(_) => strokes.extend(prim_segs(prim, at)),
            _ => out.push(OwnedPath { fill: true, segs: prim_segs(prim, at), no_show: hidden, no_line: Flag::Off }),
        }
    }
    if !strokes.is_empty() {
        out.push(OwnedPath { fill: false, segs: strokes, no_show: hidden, no_line: Flag::Off });
    }
    out
}

/// The sections of a member that shows one of several figures: for each (figure, what hides it) the figure's sections.
pub(super) fn member(list: &[(&Glyph, Flag)], dark: bool, at: Place) -> Vec<OwnedPath> {
    list.iter().flat_map(|(glyph, hide)| sections(glyph, dark, at, *hide)).collect()
}

// ---- the figures --------------------------------------------------------------------------------------------------------------------

const ENVELOPE_BOX: [(f64, f64); 4] = [(0.0, 0.12), (1.0, 0.12), (1.0, 0.88), (0.0, 0.88)];
const ENVELOPE_FLAP: [(f64, f64); 3] = [(0.0, 0.88), (0.5, 0.5), (1.0, 0.88)];
/// The dark envelope is two shapes with a band of paper between them where the flap's line is: a shape has one line colour, so the flap cannot be drawn light over the dark.
const ENVELOPE_BODY: [(f64, f64); 5] = [(0.0, 0.12), (1.0, 0.12), (1.0, 0.72), (0.5, 0.34), (0.0, 0.72)];
const ENVELOPE_LID: [(f64, f64); 3] = [(0.0, 0.88), (1.0, 0.88), (0.5, 0.5)];
/// A message: an envelope.
pub(super) static ENVELOPE: Glyph =
    Glyph { hollow: &[Prim::Closed(&ENVELOPE_BOX), Prim::Open(&ENVELOPE_FLAP)], dark: &[Prim::Closed(&ENVELOPE_BODY), Prim::Closed(&ENVELOPE_LID)] };

const TICKS: [[(f64, f64); 2]; 4] = [[(0.5, 1.0), (0.5, 0.88)], [(1.0, 0.5), (0.88, 0.5)], [(0.5, 0.0), (0.5, 0.12)], [(0.0, 0.5), (0.12, 0.5)]];
const HANDS: [(f64, f64); 3] = [(0.5, 0.82), (0.5, 0.5), (0.72, 0.36)];
/// A timer: a clock face with its hands and four ticks.
pub(super) static CLOCK: Glyph = Glyph {
    hollow: &[
        Prim::Circle(0.5, 0.5, 0.5),
        Prim::Open(&HANDS),
        Prim::Open(&TICKS[0]),
        Prim::Open(&TICKS[1]),
        Prim::Open(&TICKS[2]),
        Prim::Open(&TICKS[3]),
    ],
    dark: &[],
};

const PAGE: [(f64, f64); 4] = [(0.15, 0.0), (0.85, 0.0), (0.85, 1.0), (0.15, 1.0)];
const PAGE_LINES: [[(f64, f64); 2]; 4] =
    [[(0.28, 0.8), (0.72, 0.8)], [(0.28, 0.6), (0.72, 0.6)], [(0.28, 0.4), (0.72, 0.4)], [(0.28, 0.2), (0.72, 0.2)]];
/// A condition and a script: a page with lines.
pub(super) static LINED_PAGE: Glyph = Glyph {
    hollow: &[Prim::Closed(&PAGE), Prim::Open(&PAGE_LINES[0]), Prim::Open(&PAGE_LINES[1]), Prim::Open(&PAGE_LINES[2]), Prim::Open(&PAGE_LINES[3])],
    dark: &[],
};

const TRIANGLE: [(f64, f64); 3] = [(0.05, 0.1), (0.95, 0.1), (0.5, 0.92)];
/// A signal: a triangle.
pub(super) static SIGNAL: Glyph = Glyph { hollow: &[Prim::Closed(&TRIANGLE)], dark: &[Prim::Closed(&TRIANGLE)] };

const PENTAGON: [(f64, f64); 5] = [(0.5, 1.0), (0.02, 0.65), (0.2, 0.05), (0.8, 0.05), (0.98, 0.65)];
/// Multiple: a pentagon.
pub(super) static MULTIPLE: Glyph = Glyph { hollow: &[Prim::Closed(&PENTAGON)], dark: &[Prim::Closed(&PENTAGON)] };

const CROSS: [(f64, f64); 12] = [
    (0.35, 0.0),
    (0.65, 0.0),
    (0.65, 0.35),
    (1.0, 0.35),
    (1.0, 0.65),
    (0.65, 0.65),
    (0.65, 1.0),
    (0.35, 1.0),
    (0.35, 0.65),
    (0.0, 0.65),
    (0.0, 0.35),
    (0.35, 0.35),
];
/// Parallel multiple: an outlined plus.
pub(super) static OUTLINED_PLUS: Glyph = Glyph { hollow: &[Prim::Closed(&CROSS)], dark: &[Prim::Closed(&CROSS)] };

const BOLT: [(f64, f64); 6] = [(0.0, 0.1), (0.3, 0.62), (0.48, 0.42), (1.0, 0.95), (0.72, 0.38), (0.52, 0.58)];
/// An error: a zigzag bolt.
pub(super) static ERROR: Glyph = Glyph { hollow: &[Prim::Closed(&BOLT)], dark: &[Prim::Closed(&BOLT)] };

const HEAD: [(f64, f64); 4] = [(0.5, 1.0), (0.95, 0.0), (0.5, 0.35), (0.05, 0.0)];
/// An escalation: an upward arrowhead.
pub(super) static ESCALATION: Glyph = Glyph { hollow: &[Prim::Closed(&HEAD)], dark: &[Prim::Closed(&HEAD)] };

const X_SHAPE: [(f64, f64); 12] =
    [(0.1, 0.0), (0.5, 0.4), (0.9, 0.0), (1.0, 0.1), (0.6, 0.5), (1.0, 0.9), (0.9, 1.0), (0.5, 0.6), (0.1, 1.0), (0.0, 0.9), (0.4, 0.5), (0.0, 0.1)];
/// A cancel: a thick X.
pub(super) static CANCEL: Glyph = Glyph { hollow: &[Prim::Closed(&X_SHAPE)], dark: &[Prim::Closed(&X_SHAPE)] };

const REWIND_LEFT: [(f64, f64); 3] = [(0.5, 0.1), (0.5, 0.9), (0.02, 0.5)];
const REWIND_RIGHT: [(f64, f64); 3] = [(0.98, 0.1), (0.98, 0.9), (0.5, 0.5)];
/// A compensation: two triangles that point left.
pub(super) static REWIND: Glyph =
    Glyph { hollow: &[Prim::Closed(&REWIND_LEFT), Prim::Closed(&REWIND_RIGHT)], dark: &[Prim::Closed(&REWIND_LEFT), Prim::Closed(&REWIND_RIGHT)] };

const ARROW: [(f64, f64); 7] = [(0.0, 0.3), (0.55, 0.3), (0.55, 0.05), (1.0, 0.5), (0.55, 0.95), (0.55, 0.7), (0.0, 0.7)];
/// A link, and the arrow of a data input and output: a block arrow that points right.
pub(super) static BLOCK_ARROW: Glyph = Glyph { hollow: &[Prim::Closed(&ARROW)], dark: &[Prim::Closed(&ARROW)] };

/// A terminate: a disc.
pub(super) static DISC: Glyph = Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.5)], dark: &[Prim::Circle(0.5, 0.5, 0.5)] };

// ---- the task types ------------------------------------------------------------------------------------------------------------------

const GEAR_A: [(f64, f64); 24] = [
    (0.650, 0.620),
    (0.728, 0.712),
    (0.691, 0.801),
    (0.571, 0.811),
    (0.561, 0.931),
    (0.472, 0.968),
    (0.380, 0.890),
    (0.288, 0.968),
    (0.199, 0.931),
    (0.189, 0.811),
    (0.069, 0.801),
    (0.032, 0.712),
    (0.110, 0.620),
    (0.032, 0.528),
    (0.069, 0.439),
    (0.189, 0.429),
    (0.199, 0.309),
    (0.288, 0.272),
    (0.380, 0.350),
    (0.472, 0.272),
    (0.561, 0.309),
    (0.571, 0.429),
    (0.691, 0.439),
    (0.728, 0.528),
];
const GEAR_B: [(f64, f64); 18] = [
    (0.930, 0.240),
    (0.978, 0.311),
    (0.940, 0.376),
    (0.855, 0.370),
    (0.817, 0.447),
    (0.743, 0.447),
    (0.705, 0.370),
    (0.620, 0.376),
    (0.582, 0.311),
    (0.630, 0.240),
    (0.582, 0.169),
    (0.620, 0.104),
    (0.705, 0.110),
    (0.743, 0.033),
    (0.817, 0.033),
    (0.855, 0.110),
    (0.940, 0.104),
    (0.978, 0.169),
];
/// A service task: a pair of gears, each with its hole.
pub(super) static GEARS: Glyph =
    Glyph { hollow: &[Prim::Closed(&GEAR_A), Prim::Circle(0.38, 0.62, 0.1), Prim::Closed(&GEAR_B), Prim::Circle(0.78, 0.24, 0.06)], dark: &[] };

const SHOULDERS: [(f64, f64); 8] = [(0.1, 0.0), (0.1, 0.28), (0.2, 0.42), (0.35, 0.5), (0.65, 0.5), (0.8, 0.42), (0.9, 0.28), (0.9, 0.0)];
/// A user task: a head and shoulders.
pub(super) static PERSON: Glyph = Glyph { hollow: &[Prim::Circle(0.5, 0.76, 0.2), Prim::Closed(&SHOULDERS)], dark: &[] };

const HAND: [(f64, f64); 19] = [
    (0.22, 0.0),
    (0.78, 0.0),
    (0.84, 0.38),
    (0.98, 0.5),
    (0.9, 0.6),
    (0.76, 0.5),
    (0.76, 0.92),
    (0.64, 0.92),
    (0.64, 0.56),
    (0.58, 0.56),
    (0.58, 1.0),
    (0.46, 1.0),
    (0.46, 0.56),
    (0.4, 0.56),
    (0.4, 0.92),
    (0.28, 0.92),
    (0.28, 0.56),
    (0.22, 0.5),
    (0.18, 0.28),
];
/// A manual task: a hand of four fingers and a thumb.
pub(super) static HAND_GLYPH: Glyph = Glyph { hollow: &[Prim::Closed(&HAND)], dark: &[] };

const TABLE: [(f64, f64); 4] = [(0.0, 0.05), (1.0, 0.05), (1.0, 0.95), (0.0, 0.95)];
const TABLE_ROWS: [[(f64, f64); 2]; 2] = [[(0.0, 0.7), (1.0, 0.7)], [(0.0, 0.4), (1.0, 0.4)]];
const TABLE_COLUMN: [(f64, f64); 2] = [(0.33, 0.05), (0.33, 0.95)];
/// A business rule task: a small table with its header row.
pub(super) static GRID: Glyph =
    Glyph { hollow: &[Prim::Closed(&TABLE), Prim::Open(&TABLE_ROWS[0]), Prim::Open(&TABLE_ROWS[1]), Prim::Open(&TABLE_COLUMN)], dark: &[] };

const SMALL_ENVELOPE: [(f64, f64); 4] = [(0.22, 0.33), (0.78, 0.33), (0.78, 0.67), (0.22, 0.67)];
const SMALL_FLAP: [(f64, f64); 3] = [(0.22, 0.67), (0.5, 0.5), (0.78, 0.67)];
/// An instantiating receive task: an envelope in a thin circle, as a message start event looks.
pub(super) static CIRCLED_ENVELOPE: Glyph =
    Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.5), Prim::Closed(&SMALL_ENVELOPE), Prim::Open(&SMALL_FLAP)], dark: &[] };

// ---- what stands along the bottom of an activity -------------------------------------------------------------------------------------

const LOOP_ARC: [(f64, f64); 15] = [
    (0.680, 0.812),
    (0.563, 0.855),
    (0.437, 0.855),
    (0.320, 0.812),
    (0.224, 0.731),
    (0.162, 0.623),
    (0.140, 0.500),
    (0.162, 0.377),
    (0.224, 0.269),
    (0.320, 0.188),
    (0.437, 0.145),
    (0.563, 0.145),
    (0.680, 0.188),
    (0.776, 0.269),
    (0.838, 0.377),
];
const LOOP_HEAD: [(f64, f64); 3] = [(0.895, 0.164), (0.838, 0.377), (0.658, 0.251)];
/// A loop: an arrow that curls back on itself.
pub(super) static LOOP: Glyph = Glyph { hollow: &[Prim::Open(&LOOP_ARC), Prim::Open(&LOOP_HEAD)], dark: &[] };

const UPRIGHT_BARS: [[(f64, f64); 2]; 3] = [[(0.2, 0.1), (0.2, 0.9)], [(0.5, 0.1), (0.5, 0.9)], [(0.8, 0.1), (0.8, 0.9)]];
/// A parallel multi-instance activity, and a data collection: three vertical bars.
pub(super) static BARS_UPRIGHT: Glyph =
    Glyph { hollow: &[Prim::Open(&UPRIGHT_BARS[0]), Prim::Open(&UPRIGHT_BARS[1]), Prim::Open(&UPRIGHT_BARS[2])], dark: &[] };

const LYING_BARS: [[(f64, f64); 2]; 3] = [[(0.1, 0.2), (0.9, 0.2)], [(0.1, 0.5), (0.9, 0.5)], [(0.1, 0.8), (0.9, 0.8)]];
/// A sequential multi-instance activity: the same bars turned a quarter.
pub(super) static BARS_LYING: Glyph =
    Glyph { hollow: &[Prim::Open(&LYING_BARS[0]), Prim::Open(&LYING_BARS[1]), Prim::Open(&LYING_BARS[2])], dark: &[] };

const TILDE: [(f64, f64); 7] = [(0.0, 0.35), (0.15, 0.65), (0.3, 0.75), (0.5, 0.5), (0.7, 0.25), (0.85, 0.35), (1.0, 0.65)];
/// An ad hoc sub-process: a tilde.
pub(super) static WAVE: Glyph = Glyph { hollow: &[Prim::Open(&TILDE)], dark: &[] };

const BOX: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
const PLUS_ACROSS: [(f64, f64); 2] = [(0.25, 0.5), (0.75, 0.5)];
const PLUS_DOWN: [(f64, f64); 2] = [(0.5, 0.25), (0.5, 0.75)];
/// A collapsed sub-process: a small square holding a plus.
pub(super) static PLUS_BOX: Glyph = Glyph { hollow: &[Prim::Closed(&BOX), Prim::Open(&PLUS_ACROSS), Prim::Open(&PLUS_DOWN)], dark: &[] };

// ---- gateways -----------------------------------------------------------------------------------------------------------------------

const DIAGONAL_UP: [(f64, f64); 2] = [(0.1, 0.1), (0.9, 0.9)];
const DIAGONAL_DOWN: [(f64, f64); 2] = [(0.1, 0.9), (0.9, 0.1)];
/// An exclusive gateway's X.
pub(super) static CROSS_X: Glyph = Glyph { hollow: &[Prim::Open(&DIAGONAL_UP), Prim::Open(&DIAGONAL_DOWN)], dark: &[] };

const ACROSS: [(f64, f64); 2] = [(0.05, 0.5), (0.95, 0.5)];
const DOWN: [(f64, f64); 2] = [(0.5, 0.05), (0.5, 0.95)];
/// A parallel gateway's plus.
pub(super) static PLUS: Glyph = Glyph { hollow: &[Prim::Open(&ACROSS), Prim::Open(&DOWN)], dark: &[] };

/// A complex gateway's asterisk: the plus and the X together.
pub(super) static ASTERISK: Glyph =
    Glyph { hollow: &[Prim::Open(&ACROSS), Prim::Open(&DOWN), Prim::Open(&DIAGONAL_UP), Prim::Open(&DIAGONAL_DOWN)], dark: &[] };

/// An inclusive gateway's circle.
pub(super) static RING: Glyph = Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.4)], dark: &[] };

const SMALL_PENTAGON: [(f64, f64); 5] = [(0.5, 0.76), (0.27, 0.59), (0.36, 0.32), (0.64, 0.32), (0.73, 0.59)];
/// An event-based gateway: a pentagon in two rings, as a catching multiple intermediate event looks.
pub(super) static DOUBLE_RING_PENTAGON: Glyph =
    Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.5), Prim::Circle(0.5, 0.5, 0.41), Prim::Closed(&SMALL_PENTAGON)], dark: &[] };
/// An event-based gateway that starts a process: a pentagon in one ring, as a multiple start event looks.
pub(super) static RING_PENTAGON: Glyph = Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.5), Prim::Closed(&SMALL_PENTAGON)], dark: &[] };

const SMALL_CROSS: [(f64, f64); 12] = [
    (0.4, 0.25),
    (0.6, 0.25),
    (0.6, 0.4),
    (0.75, 0.4),
    (0.75, 0.6),
    (0.6, 0.6),
    (0.6, 0.75),
    (0.4, 0.75),
    (0.4, 0.6),
    (0.25, 0.6),
    (0.25, 0.4),
    (0.4, 0.4),
];
/// A parallel event-based gateway that starts a process: an outlined plus in one ring, as a parallel multiple start event looks.
pub(super) static RING_PLUS: Glyph = Glyph { hollow: &[Prim::Circle(0.5, 0.5, 0.5), Prim::Closed(&SMALL_CROSS)], dark: &[] };

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph, with its name.
    fn all() -> Vec<(&'static str, &'static Glyph)> {
        vec![
            ("envelope", &ENVELOPE),
            ("clock", &CLOCK),
            ("lined page", &LINED_PAGE),
            ("signal", &SIGNAL),
            ("multiple", &MULTIPLE),
            ("outlined plus", &OUTLINED_PLUS),
            ("error", &ERROR),
            ("escalation", &ESCALATION),
            ("cancel", &CANCEL),
            ("rewind", &REWIND),
            ("block arrow", &BLOCK_ARROW),
            ("disc", &DISC),
            ("gears", &GEARS),
            ("person", &PERSON),
            ("hand", &HAND_GLYPH),
            ("grid", &GRID),
            ("circled envelope", &CIRCLED_ENVELOPE),
            ("loop", &LOOP),
            ("upright bars", &BARS_UPRIGHT),
            ("lying bars", &BARS_LYING),
            ("wave", &WAVE),
            ("plus box", &PLUS_BOX),
            ("cross x", &CROSS_X),
            ("plus", &PLUS),
            ("asterisk", &ASTERISK),
            ("ring", &RING),
            ("double ring pentagon", &DOUBLE_RING_PENTAGON),
            ("ring pentagon", &RING_PENTAGON),
            ("ring plus", &RING_PLUS),
        ]
    }

    fn points_of(prim: &Prim) -> Vec<(f64, f64)> {
        match prim {
            Prim::Closed(p) | Prim::Open(p) => p.to_vec(),
            // A circle reaches its centre plus and minus its radius along each axis.
            Prim::Circle(cx, cy, r) => vec![(cx - r, cy - r), (cx + r, cy + r)],
        }
    }

    #[test]
    fn every_glyphs_strokes_lie_inside_its_box() {
        for (name, glyph) in all() {
            for prim in glyph.hollow.iter().chain(glyph.dark) {
                for (u, v) in points_of(prim) {
                    assert!((-1e-9..=1.0 + 1e-9).contains(&u) && (-1e-9..=1.0 + 1e-9).contains(&v), "{name}: ({u}, {v}) is outside the unit square");
                }
            }
        }
    }

    #[test]
    fn a_dark_form_is_made_of_shapes_that_close_and_a_hollow_form_of_one_stroke_section() {
        let hide = Flag::Off;
        for (name, glyph) in all() {
            let hollow = sections(glyph, false, Place::Share, hide);
            assert_eq!(hollow.len(), 1, "{name}: one section of strokes");
            assert!(!hollow[0].fill, "{name}: a hollow marker takes no fill");
            let dark = sections(glyph, true, Place::Share, hide);
            assert_eq!(dark.is_empty(), glyph.dark.is_empty(), "{name}: a glyph with a dark form draws it");
            for path in dark.iter().filter(|p| p.fill) {
                let first = path.segs.first();
                let last = path.segs.last();
                let (Some(Seg::Move(x0, y0)), Some(end)) = (first, last) else { panic!("{name}: a filled path starts with a MoveTo") };
                let (x1, y1) = match end {
                    Seg::Line(x, y) => (x, y),
                    Seg::Half { to, .. } | Seg::Arc { to, .. } => (&to.0, &to.1),
                    other => panic!("{name}: a filled path ends with a line or an arc, not {other:?}"),
                };
                assert!(
                    (x0.value(1.0, 1.0) - x1.value(1.0, 1.0)).abs() < 1e-9 && (y0.value(1.0, 1.0) - y1.value(1.0, 1.0)).abs() < 1e-9,
                    "{name}: it closes"
                );
                assert_eq!(path.segs.iter().filter(|s| matches!(s, Seg::Move(..))).count(), 1, "{name}: one run");
            }
        }
    }

    #[test]
    fn a_fixed_place_puts_the_figure_in_a_square_of_inches() {
        let at = Place::Fixed { x: fw(0.5).plus(fk(-0.08)), y: fk(0.06), size: 0.16 };
        let segs = sections(&PLUS_BOX, false, at, Flag::Off);
        let first = segs[0].segs.first().expect("a first segment");
        let Seg::Move(x, y) = first else { panic!("a MoveTo first") };
        // At a box 3 inches wide, the unit square's lower left corner is 0.08 inch left of the middle, 0.06 inch up.
        assert!((x.value(3.0, 2.0) - 1.42).abs() < 1e-9 && (y.value(3.0, 2.0) - 0.06).abs() < 1e-9, "({}, {})", x.value(3.0, 2.0), y.value(3.0, 2.0));
        let far = segs[0].segs.iter().filter_map(|s| if let Seg::Line(x, _) = s { Some(x.value(3.0, 2.0)) } else { None }).fold(0.0_f64, f64::max);
        assert!((far - 1.58).abs() < 1e-9, "the square is 0.16 inch wide: {far}");
    }
}
