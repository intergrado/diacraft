//! The masters of Basic Flowchart: eighteen symbols, DiaCraft's own drawings.
//!
//! **Names** are the generic ones of the public notation. Thirteen of the eighteen are in the "Common symbols" and "Other symbols" tables
//! of <https://en.wikipedia.org/wiki/Flowchart> (read 2026-11-01), which names ISO 5807: terminal (our Terminator), process, decision,
//! input/output (our Data), predefined process, on-page connector, off-page connector, data file or database, document, multiple
//! documents, manual operation, manual input, and preparation. (The article's annotation symbol, an open bracket with a single place to
//! join, is left out: a master here has at least four connection points.) Delay, display, internal storage, summing junction and "or" are
//! the other symbols that general descriptions of ISO 5807 flowcharts list; the article does not name them, and no source read gives any
//! drawing, which none needs, for each is a plain figure.
//!
//! **Drawings** are ours: lines, circular arcs through three points and half-ellipses (`EllipticalArcTo`, which three points fix with no sign
//! convention to get wrong), written as formulas of `Width` and `Height`. A slant or a rounded end is a share of the *height*, so that
//! widening a symbol keeps its angles and its round ends. The sizes (1.5 inches by 0.8 for a rectangle-like symbol) are our choice, and so
//! are the connection points: the middles of the four sides, moved onto the outline where a side is slanted or curved.

use super::{Co, Flag, MasterSpec, More, Outline, PathSpec, SIDES, Seg, TextBox, Xy, fh, fw, mix, pc};

/// The right and the top of the box.
const W: Co = fw(1.0);
const H: Co = fh(1.0);
/// The left and the bottom.
const Z: Co = fw(0.0);

/// A share of `Width`, used as an x.
const fn x(share: f64) -> Co {
    fw(share)
}

/// A share of `Height`, used as a y (or, with a slant, as an x that grows with the height).
const fn y(share: f64) -> Co {
    fh(share)
}

const fn mv(x: Co, y: Co) -> Seg {
    Seg::Move(x, y)
}

const fn ln(x: Co, y: Co) -> Seg {
    Seg::Line(x, y)
}

/// A circular arc from the pen to `to`, through `via`.
const fn arc(to: Xy, via: Xy) -> Seg {
    Seg::Arc { to, via }
}

/// An arc of the ellipse with semi-axes `rx` and `ry` along the box's, from the pen to `to`, through `via`.
const fn half(to: Xy, via: Xy, rx: Co, ry: Co) -> Seg {
    Seg::Half { to, via, rx, ry }
}

/// A path that takes the fill.
const fn solid(segs: &'static [Seg]) -> PathSpec {
    PathSpec { fill: true, segs, no_show: Flag::Off, no_line: Flag::Off }
}

/// A path that only strokes (`NoFill` 1), drawn inside the fill of another.
const fn open(segs: &'static [Seg]) -> PathSpec {
    PathSpec { fill: false, segs, no_show: Flag::Off, no_line: Flag::Off }
}

const fn text(x: f64, y: f64, w: f64, h: f64) -> TextBox {
    TextBox { x, y, w, h }
}

/// The box as a closed path.
const BOX: PathSpec = solid(&[mv(Z, Z), ln(W, Z), ln(W, H), ln(Z, H), ln(Z, Z)]);

/// The whole ellipse of the box.
const OVAL: PathSpec = solid(&[Seg::Ellipse]);

/// Half the height, as an x: the radius of a round end.
const CAP: Co = y(0.5);
/// `Width` less half the height: where a round end on the right begins.
const CAP_AT_RIGHT: Co = mix(1.0, -0.5);

/// The wave of a document: its bottom edge, from the right corner, dips to the bottom of the box and comes back up to a crest a fifth up.
/// The corners are at a tenth of the height, so the dip and the crest are each a tenth from the corners' level.
const BASE: Co = y(0.1);

/// The step of a stack of three documents: each sheet 0.07 of the width to the right of the one before it, and 0.08 of the height above it.
const SHEET_X: f64 = 0.07;

/// The rim of a database: half-ellipses 0.12 of the height tall.
const RIM: Co = y(0.12);

/// The slant of a data symbol, as an x that grows with the height.
const SLANT: Co = y(0.25);

pub(super) static FLOWCHART: [MasterSpec; 18] = [
    MasterSpec {
        identity: (2, 1),
        more: &More::PLACEABLE,
        id: "process",
        name: "Process",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths { paths: &[BOX], text: TextBox::FULL },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 2),
        more: &More::PLACEABLE,
        id: "decision",
        name: "Decision",
        width: 1.5,
        height: 1.0,
        outline: Outline::Paths {
            paths: &[solid(&[mv(x(0.5), Z), ln(W, y(0.5)), ln(x(0.5), H), ln(Z, y(0.5)), ln(x(0.5), Z)])],
            // The inside of a diamond: the text of a decision is a short question.
            text: text(0.25, 0.25, 0.5, 0.5),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 3),
        more: &More::PLACEABLE,
        id: "terminator",
        name: "Terminator",
        width: 1.5,
        height: 0.6,
        outline: Outline::Paths {
            // A stadium: straight top and bottom, and a half circle of radius half the height at each end.
            paths: &[solid(&[mv(CAP, Z), ln(CAP_AT_RIGHT, Z), arc((CAP_AT_RIGHT, H), (W, y(0.5))), ln(CAP, H), arc((CAP, Z), (Z, y(0.5)))])],
            text: text(0.1, 0.0, 0.8, 1.0),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 4),
        more: &More::PLACEABLE,
        id: "data",
        name: "Data",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A parallelogram leaning right, the slant a quarter of the height.
            paths: &[solid(&[mv(Z, Z), ln(mix(1.0, -0.25), Z), ln(W, H), ln(SLANT, H), ln(Z, Z)])],
            text: text(0.1, 0.0, 0.8, 1.0),
        },
        points: &[
            pc(mix(0.5, 0.125), H, 0.0, 1.0),
            pc(y(0.125), y(0.5), -1.0, 0.0),
            pc(mix(0.5, -0.125), Z, 0.0, -1.0),
            pc(mix(1.0, -0.125), y(0.5), 1.0, 0.0),
        ],
    },
    MasterSpec {
        identity: (2, 5),
        more: &More::PLACEABLE,
        id: "document",
        name: "Document",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A rectangle with a wavy bottom: two circular arcs, the right one dipping and the left one bulging, meeting with one tangent.
            paths: &[solid(&[mv(Z, BASE), ln(Z, H), ln(W, H), ln(W, BASE), arc((x(0.5), BASE), (x(0.75), Z)), arc((Z, BASE), (x(0.25), y(0.2)))])],
            text: text(0.0, 0.2, 1.0, 0.8),
        },
        points: &[pc(x(0.5), H, 0.0, 1.0), pc(Z, y(0.55), -1.0, 0.0), pc(x(0.5), BASE, 0.0, -1.0), pc(W, y(0.55), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 6),
        more: &More::PLACEABLE,
        id: "multiple_documents",
        name: "Multiple Documents",
        width: 1.5,
        height: 1.0,
        outline: Outline::Paths {
            // Three sheets stepped up and to the right. The front one is a whole document; the two behind it are the strips that show
            // above and beside the one in front, so that no region is in two paths (the fill is odd crossings).
            paths: &[
                solid(&[
                    mv(Z, y(0.08)),
                    ln(Z, y(0.84)),
                    ln(x(0.86), y(0.84)),
                    ln(x(0.86), y(0.08)),
                    arc((x(0.43), y(0.08)), (x(0.645), Z)),
                    arc((Z, y(0.08)), (x(0.215), y(0.16))),
                ]),
                solid(&[
                    mv(x(SHEET_X), y(0.84)),
                    ln(x(SHEET_X), y(0.92)),
                    ln(x(0.93), y(0.92)),
                    ln(x(0.93), y(0.16)),
                    ln(x(0.86), y(0.16)),
                    ln(x(0.86), y(0.84)),
                    ln(x(SHEET_X), y(0.84)),
                ]),
                solid(&[
                    mv(x(2.0 * SHEET_X), y(0.92)),
                    ln(x(2.0 * SHEET_X), H),
                    ln(W, H),
                    ln(W, y(0.24)),
                    ln(x(0.93), y(0.24)),
                    ln(x(0.93), y(0.92)),
                    ln(x(2.0 * SHEET_X), y(0.92)),
                ]),
            ],
            text: text(0.0, 0.16, 0.86, 0.68),
        },
        points: &[pc(x(0.5), H, 0.0, 1.0), pc(Z, y(0.46), -1.0, 0.0), pc(x(0.43), y(0.08), 0.0, -1.0), pc(W, y(0.62), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 7),
        more: &More::PLACEABLE,
        id: "predefined_process",
        name: "Predefined Process",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A rectangle with a vertical line near each end, a fifth of the height in.
            paths: &[BOX, open(&[mv(y(0.2), Z), ln(y(0.2), H), mv(mix(1.0, -0.2), Z), ln(mix(1.0, -0.2), H)])],
            text: text(0.12, 0.0, 0.76, 1.0),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 8),
        more: &More::PLACEABLE,
        id: "manual_input",
        name: "Manual Input",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A rectangle whose top slopes up to the right from four fifths of the height.
            paths: &[solid(&[mv(Z, Z), ln(W, Z), ln(W, H), ln(Z, y(0.8)), ln(Z, Z)])],
            text: text(0.0, 0.0, 1.0, 0.8),
        },
        points: &[pc(x(0.5), y(0.9), 0.0, 1.0), pc(Z, y(0.4), -1.0, 0.0), pc(x(0.5), Z, 0.0, -1.0), pc(W, y(0.5), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 9),
        more: &More::PLACEABLE,
        id: "manual_operation",
        name: "Manual Operation",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A trapezoid wide at the top, each side slanting in by a quarter of the height.
            paths: &[solid(&[mv(Z, H), ln(W, H), ln(mix(1.0, -0.25), Z), ln(SLANT, Z), ln(Z, H)])],
            text: text(0.14, 0.0, 0.72, 1.0),
        },
        points: &[pc(x(0.5), H, 0.0, 1.0), pc(y(0.125), y(0.5), -1.0, 0.0), pc(x(0.5), Z, 0.0, -1.0), pc(mix(1.0, -0.125), y(0.5), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 10),
        more: &More::PLACEABLE,
        id: "preparation",
        name: "Preparation",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // An elongated hexagon with a point at each end, each slanting in by two fifths of the height.
            paths: &[solid(&[
                mv(Z, y(0.5)),
                ln(y(0.4), H),
                ln(mix(1.0, -0.4), H),
                ln(W, y(0.5)),
                ln(mix(1.0, -0.4), Z),
                ln(y(0.4), Z),
                ln(Z, y(0.5)),
            ])],
            text: text(0.15, 0.0, 0.7, 1.0),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 11),
        more: &More::PLACEABLE,
        id: "on_page_reference",
        name: "On-Page Reference",
        width: 0.5,
        height: 0.5,
        outline: Outline::Paths { paths: &[OVAL], text: text(0.15, 0.15, 0.7, 0.7) },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 12),
        more: &More::PLACEABLE,
        id: "off_page_reference",
        name: "Off-Page Reference",
        width: 0.6,
        height: 0.8,
        outline: Outline::Paths {
            // A box that ends in a point at the bottom, the shoulders at 0.4 of the height.
            paths: &[solid(&[mv(Z, H), ln(W, H), ln(W, y(0.4)), ln(x(0.5), Z), ln(Z, y(0.4)), ln(Z, H)])],
            text: text(0.0, 0.4, 1.0, 0.6),
        },
        points: &[pc(x(0.5), H, 0.0, 1.0), pc(Z, y(0.7), -1.0, 0.0), pc(x(0.5), Z, 0.0, -1.0), pc(W, y(0.7), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 13),
        more: &More::PLACEABLE,
        id: "database",
        name: "Database",
        width: 1.0,
        height: 1.2,
        outline: Outline::Paths {
            // A cylinder: the body outlined with a half-ellipse at the bottom and one on top, and the front half of the top ellipse as a
            // stroke of its own (the fill is the body).
            paths: &[
                solid(&[
                    mv(Z, y(0.88)),
                    ln(Z, y(0.12)),
                    half((W, y(0.12)), (x(0.5), Z), x(0.5), RIM),
                    ln(W, y(0.88)),
                    half((Z, y(0.88)), (x(0.5), H), x(0.5), RIM),
                ]),
                open(&[mv(Z, y(0.88)), half((W, y(0.88)), (x(0.5), y(0.76)), x(0.5), RIM)]),
            ],
            text: text(0.05, 0.1, 0.9, 0.62),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 14),
        more: &More::PLACEABLE,
        id: "internal_storage",
        name: "Internal Storage",
        width: 1.0,
        height: 1.0,
        outline: Outline::Paths {
            // A rectangle with a vertical line and a horizontal line, each 0.15 in from the left and the top.
            paths: &[BOX, open(&[mv(x(0.15), Z), ln(x(0.15), H), mv(Z, y(0.85)), ln(W, y(0.85))])],
            text: text(0.15, 0.0, 0.85, 0.85),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 15),
        more: &More::PLACEABLE,
        id: "delay",
        name: "Delay",
        width: 1.2,
        height: 0.8,
        outline: Outline::Paths {
            // A rectangle with a half circle for its right end.
            paths: &[solid(&[mv(Z, Z), ln(CAP_AT_RIGHT, Z), arc((CAP_AT_RIGHT, H), (W, y(0.5))), ln(Z, H), ln(Z, Z)])],
            text: text(0.0, 0.0, 0.75, 1.0),
        },
        points: &[pc(mix(0.5, -0.25), H, 0.0, 1.0), pc(Z, y(0.5), -1.0, 0.0), pc(mix(0.5, -0.25), Z, 0.0, -1.0), pc(W, y(0.5), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 16),
        more: &More::PLACEABLE,
        id: "display",
        name: "Display",
        width: 1.5,
        height: 0.8,
        outline: Outline::Paths {
            // A point at the left, slanting out by three tenths of the height, and a half circle at the right.
            paths: &[solid(&[mv(Z, y(0.5)), ln(y(0.3), H), ln(CAP_AT_RIGHT, H), arc((CAP_AT_RIGHT, Z), (W, y(0.5))), ln(y(0.3), Z), ln(Z, y(0.5))])],
            text: text(0.15, 0.0, 0.7, 1.0),
        },
        points: &[pc(mix(0.5, -0.1), H, 0.0, 1.0), pc(Z, y(0.5), -1.0, 0.0), pc(mix(0.5, -0.1), Z, 0.0, -1.0), pc(W, y(0.5), 1.0, 0.0)],
    },
    MasterSpec {
        identity: (2, 17),
        more: &More::PLACEABLE,
        id: "summing_junction",
        name: "Summing Junction",
        width: 0.5,
        height: 0.5,
        outline: Outline::Paths {
            // A circle crossed by its two diagonals; the ends of the diagonals are on the circle at 45 degrees.
            paths: &[
                OVAL,
                open(&[
                    mv(x(0.1464466), y(0.1464466)),
                    ln(x(0.8535534), y(0.8535534)),
                    mv(x(0.1464466), y(0.8535534)),
                    ln(x(0.8535534), y(0.1464466)),
                ]),
            ],
            text: text(0.2, 0.2, 0.6, 0.6),
        },
        points: &SIDES,
    },
    MasterSpec {
        identity: (2, 18),
        more: &More::PLACEABLE,
        id: "or",
        name: "Or",
        width: 0.5,
        height: 0.5,
        outline: Outline::Paths {
            // A circle crossed by its horizontal and vertical diameters.
            paths: &[OVAL, open(&[mv(x(0.5), Z), ln(x(0.5), H), mv(Z, y(0.5)), ln(W, y(0.5))])],
            text: text(0.2, 0.2, 0.6, 0.6),
        },
        points: &SIDES,
    },
];
