//! Basic Gane-Sarson notation; original dimensions and contour recipes in
//! –D5. One ordinary label per node.
use super::{Co, MasterSpec, Outline, PathSpec, SIDES, Seg, TextBox, fh, fw, pt};

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

/// A corner coordinate: shares of width, height and the shorter side. The radius
/// is .12 of that shorter side even after a nonuniform resize.
const fn corner(w: f64, h: f64, radius: f64) -> Co {
    Co { w, h, min: radius * 0.12 }
}
const R: Co = corner(0.0, 0.0, 1.0);
const WR: Co = corner(1.0, 0.0, -1.0);
const HR: Co = corner(0.0, 1.0, -1.0);
// A quarter-circle's midpoint is (1 - sqrt(.5)) radii from its bounding corner.
const INSET: f64 = 0.2928932188134524;
const Q: Co = corner(0.0, 0.0, INSET);
const WQ: Co = corner(1.0, 0.0, -INSET);
const HQ: Co = corner(0.0, 1.0, -INSET);
const LABEL: TextBox = TextBox { x: 0.08, y: 0.08, w: 0.84, h: 0.84 };

pub(super) static MASTERS: [MasterSpec; 4] = [
    MasterSpec {
        id: "gane-sarson.interface",
        name: "Interface",
        width: 1.6,
        height: 0.9,
        identity: (6, 1),
        outline: Outline::Paths {
            paths: &[PathSpec { fill: true, segs: &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)] }],
            text: LABEL,
        },
        points: &SIDES,
    },
    MasterSpec {
        id: "gane-sarson.process",
        name: "Process",
        width: 1.6,
        height: 1.0,
        identity: (6, 2),
        outline: Outline::Paths {
            paths: &[PathSpec {
                fill: true,
                segs: &[
                    Seg::Move(R, Z),
                    Seg::Line(WR, Z),
                    Seg::Arc { to: (W, R), via: (WQ, Q) },
                    Seg::Line(W, HR),
                    Seg::Arc { to: (WR, H), via: (WQ, HQ) },
                    Seg::Line(R, H),
                    Seg::Arc { to: (Z, HR), via: (Q, HQ) },
                    Seg::Line(Z, R),
                    Seg::Arc { to: (R, Z), via: (Q, Q) },
                ],
            }],
            text: LABEL,
        },
        points: &SIDES,
    },
    MasterSpec {
        id: "gane-sarson.data-store",
        name: "Data Store",
        width: 1.8,
        height: 0.8,
        identity: (6, 3),
        // One open, unfilled path: neither page rendering nor preview may close its right side.
        outline: Outline::Paths {
            paths: &[PathSpec { fill: false, segs: &[Seg::Move(W, H), Seg::Line(Z, H), Seg::Line(Z, Z), Seg::Line(W, Z)] }],
            text: LABEL,
        },
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 1.0, 1.0, 0.0)],
    },
    MasterSpec {
        id: "gane-sarson.data-flow",
        name: "Data Flow",
        width: 1.5,
        height: 0.0,
        identity: (6, 4),
        outline: Outline::Connector,
        points: &[],
    },
];
