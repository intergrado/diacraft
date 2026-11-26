//! The glyph kit of the network stencils: a device is a front-view pictogram drawn in code, inside an **icon box** whose size in inches is the master's.
//!
//! **Original figures** (`docs/provenance.md`, "Original material that could be taken for copied"). Every glyph is plain geometry made by a function from rectangles, circles, arcs and
//! strokes, in inches of its icon box, and nothing is traced, measured or copied from any icon set or any maker's mark. The forms are generic metaphors that every network diagram uses:
//! a low box with a row of ports, a tower with drive bays, a screen on a stand, signal arcs for radio, a wall of bricks for a firewall, a globe for the internet, a cloud of arcs, a cylinder
//! for a database, a folder, an envelope, a window and a printer as the badge of a server's role. No vendor's mark is drawn and no letter is set in the style of one.
//!
//! **How a glyph is built.** The body is one or more closed, filled sections that take the style's fill. The details (ports, bays, slots, bricks, arrows, vents, badges) are stroke-only
//! sections, `NoFill` 1, in the line colour, so a device follows every theme and a format a person applies. A coordinate is a share of the box ([`Pane::x`] and [`Pane::y`]), written to nine
//! places so that a cell's cached value is what its formula gives, and an arc is the arc of an ellipse whose axes follow the box's (a circle at the default size, stretched with the box).
//! Everything stays inside the box (`every_glyphs_strokes_lie_inside_its_icon_box`), and a master is held to the caps of the catalogue (64 sections, 1,024 rows).
//!
//! The kit is `pub(in crate::stencil)` so that Cloud Services draws its services from the same pieces.

use crate::stencil::{Co, Flag, OwnedPath, Seg, fh, fr, fw};

/// A point or a length pair, in inches of the icon box.
type P2 = (f64, f64);

/// One continuous run of segments: a `MoveTo` and what follows it.
type Run = Vec<Seg>;

/// A quarter circle's midpoint is `1 - sqrt(0.5)` of the radius from its bounding corner.
const INSET: f64 = 0.292_893_218_813_452_4;

/// The icon box of a glyph, in inches at the default size: the master's width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::stencil) struct Pane {
    pub w: f64,
    pub h: f64,
}

impl Pane {
    pub(in crate::stencil) const fn new(w: f64, h: f64) -> Pane {
        Pane { w, h }
    }

    /// A coordinate `inches` from the left, as a share of `Width` (written to nine places, as the cells' own text is).
    fn x(self, inches: f64) -> Co {
        fw(share(inches, self.w))
    }

    /// A coordinate `inches` from the bottom, as a share of `Height`.
    fn y(self, inches: f64) -> Co {
        fh(share(inches, self.h))
    }

    fn at(self, (x, y): P2) -> (Co, Co) {
        (self.x(x), self.y(y))
    }
}

/// `inches / whole` rounded to nine places, or 0 for a `whole` that is no length or a number that is no number.
fn share(inches: f64, whole: f64) -> f64 {
    if whole > 0.0 && inches.is_finite() { ((inches / whole) * 1.0e9).round() / 1.0e9 } else { 0.0 }
}

// ---- the sizes of the masters --------------------------------------------------------------------------------------------------------

pub(in crate::stencil) const ROUTER: Pane = Pane::new(1.0, 0.6);
pub(in crate::stencil) const SWITCH: Pane = Pane::new(1.2, 0.5);
pub(in crate::stencil) const FIREWALL: Pane = Pane::new(0.9, 0.8);
pub(in crate::stencil) const CLOUD: Pane = Pane::new(1.4, 0.9);
pub(in crate::stencil) const ACCESS_POINT: Pane = Pane::new(0.9, 0.8);
pub(in crate::stencil) const BALANCER: Pane = Pane::new(1.0, 0.6);
pub(in crate::stencil) const MODEM: Pane = Pane::new(1.0, 0.5);
pub(in crate::stencil) const INTERNET: Pane = Pane::new(0.9, 0.9);
pub(in crate::stencil) const ETHERNET: Pane = Pane::new(6.0, 0.12);
pub(in crate::stencil) const RING: Pane = Pane::new(2.0, 2.0);
pub(in crate::stencil) const SERVER: Pane = Pane::new(0.6, 1.0);
pub(in crate::stencil) const DESKTOP: Pane = Pane::new(1.0, 0.9);
pub(in crate::stencil) const LAPTOP: Pane = Pane::new(1.1, 0.75);
pub(in crate::stencil) const TOWER: Pane = Pane::new(0.8, 1.0);
pub(in crate::stencil) const RACK: Pane = Pane::new(1.2, 0.35);
pub(in crate::stencil) const TABLET: Pane = Pane::new(0.6, 0.8);
pub(in crate::stencil) const PHONE: Pane = Pane::new(0.4, 0.75);
pub(in crate::stencil) const TELEPHONE: Pane = Pane::new(0.8, 0.6);
pub(in crate::stencil) const PRINTER: Pane = Pane::new(0.9, 0.7);
pub(in crate::stencil) const STORAGE: Pane = Pane::new(0.9, 0.8);
pub(in crate::stencil) const USER: Pane = Pane::new(0.6, 0.8);

// ---- runs and sections ---------------------------------------------------------------------------------------------------------------

fn move_to(p: Pane, at: P2) -> Seg {
    let (x, y) = p.at(at);
    Seg::Move(x, y)
}

fn line_to(p: Pane, at: P2) -> Seg {
    let (x, y) = p.at(at);
    Seg::Line(x, y)
}

/// A closed polygon: the points in order, and a line back to the first.
fn polygon(p: Pane, points: &[P2]) -> Run {
    let mut run: Run = points.iter().enumerate().map(|(i, &at)| if i == 0 { move_to(p, at) } else { line_to(p, at) }).collect();
    if let Some(&first) = points.first() {
        run.push(line_to(p, first));
    }
    run
}

/// An open polyline through the points.
fn polyline(p: Pane, points: &[P2]) -> Run {
    points.iter().enumerate().map(|(i, &at)| if i == 0 { move_to(p, at) } else { line_to(p, at) }).collect()
}

fn line(p: Pane, from: P2, to: P2) -> Run {
    polyline(p, &[from, to])
}

/// A closed rectangle from (`x0`, `y0`) to (`x1`, `y1`).
fn rect(p: Pane, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Run {
    polygon(p, &[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// A rectangle with corners of `r` inches, which follow the box when it is smaller than four of them (`MIN(r, MIN(Width,Height)*0.25)`, as `kit::rounded_box` has it).
fn rounded(p: Pane, (x0, y0, x1, y1): (f64, f64, f64, f64), r: f64) -> Run {
    let (left, right, bottom, top) = (p.x(x0), p.x(x1), p.y(y0), p.y(y1));
    let (near, mid) = (fr(r, 1.0), fr(r, INSET));
    let (far, mid_far) = (fr(r, -1.0), fr(r, -INSET));
    let (l_r, b_r, r_r, t_r) = (left.plus(near), bottom.plus(near), right.plus(far), top.plus(far));
    let (l_q, b_q, r_q, t_q) = (left.plus(mid), bottom.plus(mid), right.plus(mid_far), top.plus(mid_far));
    vec![
        Seg::Move(l_r, bottom),
        Seg::Line(r_r, bottom),
        Seg::Arc { to: (right, b_r), via: (r_q, b_q) },
        Seg::Line(right, t_r),
        Seg::Arc { to: (r_r, top), via: (r_q, t_q) },
        Seg::Line(l_r, top),
        Seg::Arc { to: (left, t_r), via: (l_q, t_q) },
        Seg::Line(left, b_r),
        Seg::Arc { to: (l_r, bottom), via: (l_q, b_q) },
    ]
}

/// The point at `degrees` (anticlockwise from the right) on the ellipse of centre `c` and radii `rx`, `ry`.
fn on_ellipse(c: P2, (rx, ry): P2, degrees: f64) -> P2 {
    let a = degrees.to_radians();
    (c.0 + rx * a.cos(), c.1 + ry * a.sin())
}

/// An axis-aligned ellipse of centre `c` and radii `rx`, `ry` inches, closed: two half ellipses, from the right end.
fn ellipse(p: Pane, c: P2, (rx, ry): P2) -> Run {
    let (a, b) = (p.x(rx), p.y(ry));
    vec![
        move_to(p, (c.0 + rx, c.1)),
        Seg::Half { to: p.at((c.0 - rx, c.1)), via: p.at((c.0, c.1 + ry)), rx: a, ry: b },
        Seg::Half { to: p.at((c.0 + rx, c.1)), via: p.at((c.0, c.1 - ry)), rx: a, ry: b },
    ]
}

fn circle(p: Pane, c: P2, r: f64) -> Run {
    ellipse(p, c, (r, r))
}

/// An open arc of the circle of centre `c` and radius `r` from `from` to `to` degrees (anticlockwise), as the arc of an ellipse that follows the box.
fn arc(p: Pane, c: P2, r: f64, (from, to): P2) -> Run {
    let (start, end, mid) = (on_ellipse(c, (r, r), from), on_ellipse(c, (r, r), to), on_ellipse(c, (r, r), (from + to) / 2.0));
    vec![move_to(p, start), Seg::Half { to: p.at(end), via: p.at(mid), rx: p.x(r), ry: p.y(r) }]
}

/// The two barbs of an arrowhead whose tip is `tip` and which comes from `tail`: a polyline barb, tip, barb, with barbs `len` inches long at 28 degrees to the shaft.
fn head(p: Pane, tip: P2, tail: P2, len: f64) -> Run {
    let (dx, dy) = (tip.0 - tail.0, tip.1 - tail.1);
    let length = dx.hypot(dy);
    if length <= 0.0 || !length.is_finite() {
        return Vec::new();
    }
    let (ux, uy) = (dx / length, dy / length);
    let barb = |turn: f64| {
        let (c, s) = (turn.to_radians().cos(), turn.to_radians().sin());
        (tip.0 - len * (ux * c - uy * s), tip.1 - len * (ux * s + uy * c))
    };
    polyline(p, &[barb(28.0), tip, barb(-28.0)])
}

/// A closed, filled section: a body that takes the style's fill.
fn filled(run: Run) -> OwnedPath {
    OwnedPath { fill: true, segs: run, no_show: Flag::Off, no_line: Flag::Off }
}

/// A stroke-only section holding every run: details in the line colour.
fn stroke(runs: Vec<Run>) -> OwnedPath {
    OwnedPath { fill: false, segs: runs.into_iter().flatten().collect(), no_show: Flag::Off, no_line: Flag::Off }
}

/// `n` small rectangles of `size` (width, height) in a row from (`x`, `y`) at `pitch`: ports, bays and slots.
fn row_of(p: Pane, (x, y): P2, n: usize, pitch: f64, (w, h): P2) -> Vec<Run> {
    (0..n).map(|i| rect(p, (x + pitch * i as f64, y, x + pitch * i as f64 + w, y + h))).collect()
}

/// The whole box as a rectangle.
fn whole(p: Pane) -> Run {
    rect(p, (0.0, 0.0, p.w, p.h))
}

// ---- the network devices -------------------------------------------------------------------------------------------------------------

/// Router: a low box with four ports and a forked arrow, one line in that splits in two, on the left of its face.
pub(in crate::stencil) fn router() -> Vec<OwnedPath> {
    let p = ROUTER;
    let mut details = row_of(p, (0.54, 0.09), 4, 0.105, (0.08, 0.12));
    let fork = (0.28, 0.38);
    details.push(line(p, (0.10, fork.1), fork));
    for tip in [(0.47, 0.53), (0.47, 0.23)] {
        details.push(line(p, fork, tip));
        details.push(head(p, tip, fork, 0.07));
    }
    vec![filled(whole(p)), stroke(details)]
}

/// Switch: a low box with two rows of eight ports.
pub(in crate::stencil) fn switch() -> Vec<OwnedPath> {
    let p = SWITCH;
    let mut details = row_of(p, (0.08, 0.29), 8, 0.135, (0.095, 0.14));
    details.extend(row_of(p, (0.08, 0.07), 8, 0.135, (0.095, 0.14)));
    vec![filled(whole(p)), stroke(details)]
}

/// Firewall: a wall of bricks in a frame: four courses, the joints of every other one staggered.
pub(in crate::stencil) fn firewall() -> Vec<OwnedPath> {
    let p = FIREWALL;
    let mut bricks: Vec<Run> = [0.2, 0.4, 0.6].iter().map(|&y| line(p, (0.0, y), (p.w, y))).collect();
    for (course, joints) in [(0.0, &[0.3, 0.6][..]), (0.2, &[0.15, 0.45, 0.75][..]), (0.4, &[0.3, 0.6][..]), (0.6, &[0.15, 0.45, 0.75][..])] {
        bricks.extend(joints.iter().map(|&x| line(p, (x, course), (x, course + 0.2))));
    }
    vec![filled(whole(p)), stroke(bricks)]
}

/// The lobes of the cloud: (centre x, centre y, radius) in inches, from the left one over the top to the right one. The outer two sit on the base, so the cloud has a flat bottom, and the
/// middle one reaches the top; the box is the lobes' own, 1.4 inches by 0.9.
const LOBES: [(f64, f64, f64); 5] = [(0.30, 0.30, 0.30), (0.42, 0.58, 0.24), (0.70, 0.60, 0.30), (0.98, 0.58, 0.24), (1.10, 0.30, 0.30)];

/// The point where the circles `a` and `b` (`(x, y, r)`) cross that is farther from `away`, or `None` for circles that do not cross (apart, one inside the other, the same centre) and for numbers
/// that are no numbers.
pub(in crate::stencil) fn outer_intersection(a: (f64, f64, f64), b: (f64, f64, f64), away: P2) -> Option<P2> {
    let ((x1, y1, r1), (x2, y2, r2)) = (a, b);
    if ![x1, y1, r1, x2, y2, r2, away.0, away.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (dx, dy) = (x2 - x1, y2 - y1);
    let d = dx.hypot(dy);
    if d <= f64::EPSILON || d > r1 + r2 || d < (r1 - r2).abs() {
        return None;
    }
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let across = (r1 * r1 - along * along).max(0.0).sqrt();
    let (mx, my) = (x1 + along * dx / d, y1 + along * dy / d);
    let first = (mx - across * dy / d, my + across * dx / d);
    let second = (mx + across * dy / d, my - across * dx / d);
    let far = |q: P2| (q.0 - away.0).hypot(q.1 - away.1);
    Some(if far(first) >= far(second) { first } else { second })
}

/// The arc of lobe `lobe` clockwise from `from` to `to`, both on its circle: the segment, which passes through the point at the middle of the sweep.
fn lobe_arc(p: Pane, lobe: (f64, f64, f64), from: P2, to: P2) -> Seg {
    let (cx, cy, r) = lobe;
    let a0 = (from.1 - cy).atan2(from.0 - cx);
    let a1 = (to.1 - cy).atan2(to.0 - cx);
    let sweep = (a0 - a1).rem_euclid(std::f64::consts::TAU);
    let mid = a0 - sweep / 2.0;
    Seg::Half { to: p.at(to), via: p.at((cx + r * mid.cos(), cy + r * mid.sin())), rx: p.x(r), ry: p.y(r) }
}

/// Cloud: five arcs over a flat base. Each lobe is a circle; the arcs run between the places where neighbours cross.
pub(in crate::stencil) fn cloud() -> Vec<OwnedPath> {
    let p = CLOUD;
    let centre = (p.w / 2.0, p.h / 2.0);
    let (left, right) = (LOBES[0], LOBES[4]);
    let start = (left.0, left.1 - left.2);
    let end = (right.0, right.1 - right.2);
    let mut joints: Vec<P2> = vec![start];
    for pair in LOBES.windows(2) {
        // Neighbours of this layout always cross; the midpoint of their centres keeps the outline whole if a change to the lobes ever made two of them not.
        let at = match pair {
            [a, b] => outer_intersection(*a, *b, centre).unwrap_or(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)),
            _ => centre,
        };
        joints.push(at);
    }
    joints.push(end);
    let mut run: Run = vec![move_to(p, start)];
    for (lobe, ends) in LOBES.iter().zip(joints.windows(2)) {
        if let [from, to] = ends {
            run.push(lobe_arc(p, *lobe, *from, *to));
        }
    }
    run.push(line_to(p, start));
    vec![filled(run)]
}

/// Wireless Access Point: a low rounded box with three signal arcs above it, and the point they spread from.
pub(in crate::stencil) fn access_point() -> Vec<OwnedPath> {
    let p = ACCESS_POINT;
    let source = (0.45, 0.42);
    let mut details: Vec<Run> = [0.14, 0.26, 0.38].iter().map(|&r| arc(p, source, r, (40.0, 140.0))).collect();
    details.push(circle(p, source, 0.03));
    vec![filled(rounded(p, (0.0, 0.0, p.w, 0.30), 0.06)), stroke(details)]
}

/// Load Balancer: a low box with one arrow in that fans out to three, each ending at a small target.
pub(in crate::stencil) fn load_balancer() -> Vec<OwnedPath> {
    let p = BALANCER;
    let fork = (0.34, 0.30);
    let mut details = vec![line(p, (0.08, fork.1), fork), head(p, fork, (0.08, fork.1), 0.07)];
    for y in [0.46, 0.30, 0.14] {
        details.push(line(p, fork, (0.62, y)));
        details.push(head(p, (0.62, y), fork, 0.06));
        details.push(rect(p, (0.70, y - 0.05, 0.90, y + 0.05)));
    }
    vec![filled(whole(p)), stroke(details)]
}

/// Modem: a low box with a wave on its face (four half waves) and three small lights.
pub(in crate::stencil) fn modem() -> Vec<OwnedPath> {
    let p = MODEM;
    let (mut x, y) = (0.10, 0.25);
    let mut wave: Run = vec![move_to(p, (x, y))];
    for crest in [0.10, -0.10, 0.10, -0.10] {
        wave.push(Seg::Half { to: p.at((x + 0.10, y)), via: p.at((x + 0.05, y + crest)), rx: p.x(0.05), ry: p.y(0.10) });
        x += 0.10;
    }
    let mut details = vec![wave];
    details.extend([0.68, 0.78, 0.88].iter().map(|&lamp| circle(p, (lamp, y), 0.03)));
    vec![filled(whole(p)), stroke(details)]
}

/// Internet: a globe, with its two meridians (a narrow ellipse) and two parallels.
pub(in crate::stencil) fn internet() -> Vec<OwnedPath> {
    let p = INTERNET;
    let (c, r) = ((0.45, 0.45), 0.45);
    // A parallel at `up` inches above the equator runs between the two places where that height meets the circle.
    let parallel = |up: f64| {
        let half = (r * r - up * up).max(0.0).sqrt();
        line(p, (c.0 - half, c.1 + up), (c.0 + half, c.1 + up))
    };
    vec![filled(circle(p, c, r)), stroke(vec![ellipse(p, c, (0.2, r)), parallel(0.15), parallel(-0.15)])]
}

// ---- the computers and devices -------------------------------------------------------------------------------------------------------

/// A tower of `width` inches: three drive bays at the top and a power dot. A plain server (`vents`) also has three vents below the bays and its dot in the middle.
///
/// **A section drawn later does not hide the line of one drawn earlier** (a shape has one fill and one line, so the fills come first and every line over them), so a badge over a tower's edge
/// would show the edge through it. A badged tower (no `vents`) is therefore a filled rectangle that draws no line, and its outline is a run of its own that stops where the badge begins and
/// starts again where it ends: the left side, the top, and the right side above and below the badge.
fn tower(p: Pane, width: f64, vents: bool) -> Vec<OwnedPath> {
    let mut details: Vec<Run> = [0.86, 0.74, 0.62].iter().map(|&y| rect(p, (0.07, y, width - 0.07, y + 0.09))).collect();
    let body = rect(p, (0.0, 0.0, width, p.h));
    if vents {
        details.extend([0.30, 0.34, 0.38].iter().map(|&y| line(p, (0.15, y), (width - 0.15, y))));
        details.push(circle(p, (width / 2.0, 0.14), 0.04));
        return vec![filled(body), stroke(details)];
    }
    details.push(circle(p, (0.17, 0.20), 0.04));
    let (_, badge_bottom, _, badge_top) = BADGE_BOX;
    let outline = polyline(p, &[(width, badge_bottom), (width, 0.0), (0.0, 0.0), (0.0, p.h), (width, p.h), (width, badge_top)]);
    details.push(outline);
    vec![OwnedPath { no_line: Flag::On, ..filled(body) }, stroke(details)]
}

/// Server: a tower with drive bays and a power dot.
pub(in crate::stencil) fn server() -> Vec<OwnedPath> {
    tower(SERVER, SERVER.w, true)
}

/// The badge of a server's role, drawn inside the area (`x0`, `y0`, `x1`, `y1`) in strokes.
#[derive(Clone, Copy)]
pub(in crate::stencil) enum Badge {
    Cylinder,
    Globe,
    Folder,
    Envelope,
    Window,
    Printer,
}

/// The area of a badge on a tower: a square at the lower right, over the tower's edge.
const BADGE_BOX: (f64, f64, f64, f64) = (0.36, 0.12, 0.80, 0.56);

/// The strokes of `badge` inside the area `a`, which they stay inside. A point of the area is given as shares of its width and height.
fn badge_runs(p: Pane, badge: Badge, a: (f64, f64, f64, f64)) -> Vec<Run> {
    let (w, h) = (a.2 - a.0, a.3 - a.1);
    let at = |fx: f64, fy: f64| (a.0 + w * fx, a.1 + h * fy);
    match badge {
        Badge::Cylinder => {
            let (rx, ry) = (w * 0.4, h * 0.14);
            let (top, bottom) = (at(0.5, 0.82), at(0.5, 0.18));
            vec![
                vec![
                    move_to(p, (top.0 - rx, top.1)),
                    line_to(p, (bottom.0 - rx, bottom.1)),
                    Seg::Half { to: p.at((bottom.0 + rx, bottom.1)), via: p.at((bottom.0, bottom.1 - ry)), rx: p.x(rx), ry: p.y(ry) },
                    line_to(p, (top.0 + rx, top.1)),
                ],
                ellipse(p, top, (rx, ry)),
                vec![move_to(p, at(0.1, 0.5)), Seg::Half { to: p.at(at(0.9, 0.5)), via: p.at(at(0.5, 0.36)), rx: p.x(rx), ry: p.y(ry) }],
            ]
        }
        Badge::Globe => {
            let (c, r) = (at(0.5, 0.5), w.min(h) * 0.45);
            vec![circle(p, c, r), ellipse(p, c, (r * 0.45, r)), line(p, (c.0 - r, c.1), (c.0 + r, c.1))]
        }
        Badge::Folder => vec![polygon(p, &[at(0.08, 0.12), at(0.92, 0.12), at(0.92, 0.72), at(0.55, 0.72), at(0.46, 0.92), at(0.08, 0.92)])],
        Badge::Envelope => vec![
            rect(p, (a.0 + w * 0.05, a.1 + h * 0.15, a.0 + w * 0.95, a.1 + h * 0.85)),
            polyline(p, &[at(0.05, 0.85), at(0.5, 0.48), at(0.95, 0.85)]),
        ],
        Badge::Window => vec![
            rect(p, (a.0 + w * 0.05, a.1 + h * 0.10, a.0 + w * 0.95, a.1 + h * 0.90)),
            line(p, at(0.05, 0.70), at(0.95, 0.70)),
            rect(p, (a.0 + w * 0.12, a.1 + h * 0.76, a.0 + w * 0.22, a.1 + h * 0.84)),
        ],
        Badge::Printer => vec![
            rect(p, (a.0 + w * 0.05, a.1 + h * 0.12, a.0 + w * 0.95, a.1 + h * 0.55)),
            rect(p, (a.0 + w * 0.25, a.1 + h * 0.55, a.0 + w * 0.75, a.1 + h * 0.92)),
            line(p, at(0.12, 0.30), at(0.88, 0.30)),
        ],
    }
}

/// A server of one role: a tower, and a badge of the role on a backing at its lower right.
pub(in crate::stencil) fn badged_server(badge: Badge) -> Vec<OwnedPath> {
    let p = TOWER;
    let mut parts = tower(p, 0.56, false);
    let (x0, y0, x1, y1) = BADGE_BOX;
    parts.push(filled(rounded(p, BADGE_BOX, 0.05)));
    let inner = (x0 + 0.06, y0 + 0.06, x1 - 0.06, y1 - 0.06);
    parts.push(stroke(badge_runs(p, badge, inner)));
    parts
}

pub(in crate::stencil) fn database_server() -> Vec<OwnedPath> {
    badged_server(Badge::Cylinder)
}

pub(in crate::stencil) fn web_server() -> Vec<OwnedPath> {
    badged_server(Badge::Globe)
}

pub(in crate::stencil) fn file_server() -> Vec<OwnedPath> {
    badged_server(Badge::Folder)
}

pub(in crate::stencil) fn mail_server() -> Vec<OwnedPath> {
    badged_server(Badge::Envelope)
}

pub(in crate::stencil) fn application_server() -> Vec<OwnedPath> {
    badged_server(Badge::Window)
}

pub(in crate::stencil) fn print_server() -> Vec<OwnedPath> {
    badged_server(Badge::Printer)
}

/// Desktop: a screen on a stand. The neck and the base are drawn first, so the screen is over their tops.
pub(in crate::stencil) fn desktop() -> Vec<OwnedPath> {
    let p = DESKTOP;
    vec![
        filled(polygon(p, &[(0.40, 0.30), (0.60, 0.30), (0.63, 0.12), (0.37, 0.12)])),
        filled(rounded(p, (0.22, 0.0, 0.78, 0.12), 0.03)),
        filled(rounded(p, (0.0, 0.30, p.w, p.h), 0.05)),
        stroke(vec![rect(p, (0.07, 0.37, 0.93, 0.83))]),
    ]
}

/// Laptop: an open lid over a base seen from above, which is wider at the front.
pub(in crate::stencil) fn laptop() -> Vec<OwnedPath> {
    let p = LAPTOP;
    vec![
        filled(polygon(p, &[(0.0, 0.0), (p.w, 0.0), (0.98, 0.22), (0.12, 0.22)])),
        filled(rounded(p, (0.14, 0.22, 0.96, p.h), 0.04)),
        stroke(vec![rect(p, (0.20, 0.28, 0.90, 0.69)), line(p, (0.45, 0.08), (0.65, 0.08))]),
    ]
}

/// Rack Server: a 2U box with four drive bays, a row of vents and a power dot.
pub(in crate::stencil) fn rack_server() -> Vec<OwnedPath> {
    let p = RACK;
    let mut details = row_of(p, (0.06, 0.08), 4, 0.12, (0.10, 0.19));
    details.extend((0..6).map(|i| {
        let x = 0.66 + 0.06 * f64::from(i);
        line(p, (x, 0.10), (x, 0.25))
    }));
    details.push(circle(p, (1.08, 0.175), 0.035));
    vec![filled(whole(p)), stroke(details)]
}

/// Tablet: a rounded slab with a screen and a button.
pub(in crate::stencil) fn tablet() -> Vec<OwnedPath> {
    let p = TABLET;
    vec![filled(rounded(p, (0.0, 0.0, p.w, p.h), 0.07)), stroke(vec![rect(p, (0.06, 0.14, 0.54, 0.74)), circle(p, (0.30, 0.07), 0.03)])]
}

/// Mobile Phone: a tall rounded slab with a screen and a speaker slot.
pub(in crate::stencil) fn mobile_phone() -> Vec<OwnedPath> {
    let p = PHONE;
    vec![
        filled(rounded(p, (0.0, 0.0, p.w, p.h), 0.06)),
        stroke(vec![rect(p, (0.04, 0.10, 0.36, 0.64)), line(p, (0.15, 0.70), (0.25, 0.70)), circle(p, (0.20, 0.05), 0.025)]),
    ]
}

/// Telephone: a desk phone, a handset arched over a base with a grid of keys.
pub(in crate::stencil) fn telephone() -> Vec<OwnedPath> {
    let p = TELEPHONE;
    let handset = vec![
        move_to(p, (0.0, 0.30)),
        Seg::Half { to: p.at((0.8, 0.30)), via: p.at((0.4, 0.60)), rx: p.x(0.4), ry: p.y(0.30) },
        line_to(p, (0.72, 0.30)),
        Seg::Half { to: p.at((0.08, 0.30)), via: p.at((0.4, 0.50)), rx: p.x(0.32), ry: p.y(0.20) },
        line_to(p, (0.0, 0.30)),
    ];
    let mut keys = row_of(p, (0.26, 0.17), 3, 0.12, (0.08, 0.07));
    keys.extend(row_of(p, (0.26, 0.06), 3, 0.12, (0.08, 0.07)));
    vec![filled(polygon(p, &[(0.0, 0.0), (0.8, 0.0), (0.74, 0.30), (0.06, 0.30)])), filled(handset), stroke(keys)]
}

/// Printer: a box with a sheet coming out of its top (the sheet starts where the body ends, so no line of it shows through the body).
pub(in crate::stencil) fn printer() -> Vec<OwnedPath> {
    let p = PRINTER;
    vec![
        filled(rect(p, (0.22, 0.42, 0.68, p.h))),
        stroke(vec![line(p, (0.28, 0.60), (0.62, 0.60)), line(p, (0.28, 0.52), (0.62, 0.52))]),
        filled(rounded(p, (0.0, 0.0, p.w, 0.42), 0.05)),
        stroke(vec![line(p, (0.10, 0.30), (0.80, 0.30)), rect(p, (0.22, 0.05, 0.68, 0.20)), circle(p, (0.80, 0.12), 0.025)]),
    ]
}

/// Storage: a box with three rows of drive slots, each with a light.
pub(in crate::stencil) fn storage() -> Vec<OwnedPath> {
    let p = STORAGE;
    let mut details: Vec<Run> = Vec::new();
    for y in [0.60, 0.36, 0.12] {
        details.push(rect(p, (0.08, y, 0.62, y + 0.12)));
        details.push(circle(p, (0.76, y + 0.06), 0.03));
    }
    vec![filled(whole(p)), stroke(details)]
}

/// User: head and shoulders.
pub(in crate::stencil) fn user() -> Vec<OwnedPath> {
    let p = USER;
    let shoulders = vec![
        move_to(p, (0.02, 0.0)),
        line_to(p, (0.02, 0.16)),
        Seg::Half { to: p.at((0.58, 0.16)), via: p.at((0.30, 0.40)), rx: p.x(0.28), ry: p.y(0.24) },
        line_to(p, (0.58, 0.0)),
        line_to(p, (0.02, 0.0)),
    ];
    vec![filled(circle(p, (0.30, 0.63), 0.17)), filled(shoulders)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_written_to_nine_places_and_a_number_that_is_no_length_is_none_of_the_box() {
        assert_eq!(share(0.06, 0.9), 0.066_666_667);
        assert_eq!(share(0.9, 0.9), 1.0);
        assert_eq!(share(0.0, 0.9), 0.0);
        assert_eq!(share(1.0, 0.0), 0.0, "a box of no width has no shares");
        assert_eq!(share(1.0, -2.0), 0.0);
        assert_eq!(share(f64::NAN, 1.0), 0.0);
        assert_eq!(share(f64::INFINITY, 1.0), 0.0);
        // The share, taken at the box's width, gives the length back to a billionth of the box.
        for whole in [0.12, 0.9, 1.4, 6.0] {
            for k in 0..=50 {
                let inches = whole * f64::from(k) / 50.0;
                assert!((share(inches, whole) * whole - inches).abs() <= 5.0e-10 * whole + 1e-15, "{inches} of {whole}");
            }
        }
    }

    #[test]
    fn an_arrowhead_of_no_length_is_no_run_and_one_of_a_length_is_three_points() {
        let p = Pane::new(1.0, 1.0);
        assert!(head(p, (0.5, 0.5), (0.5, 0.5), 0.1).is_empty(), "a tip on its tail points nowhere");
        assert!(head(p, (f64::NAN, 0.5), (0.0, 0.5), 0.1).is_empty());
        assert_eq!(head(p, (0.5, 0.5), (0.0, 0.5), 0.1).len(), 3, "a barb, the tip, a barb");
    }

    #[test]
    fn a_row_of_boxes_is_one_run_of_five_segments_each_and_a_polygon_returns_to_its_first_point() {
        let p = Pane::new(2.0, 1.0);
        let boxes = row_of(p, (0.1, 0.1), 8, 0.2, (0.1, 0.1));
        assert_eq!(boxes.len(), 8);
        assert!(boxes.iter().all(|run| run.len() == 5 && matches!(run.first(), Some(Seg::Move(..)))));
        assert!(row_of(p, (0.1, 0.1), 0, 0.2, (0.1, 0.1)).is_empty());
        assert!(polygon(p, &[]).is_empty());
        let triangle = polygon(p, &[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)]);
        assert_eq!(triangle.len(), 4);
        assert!(matches!(triangle.last(), Some(Seg::Line(..))));
    }

    #[test]
    fn every_glyph_is_within_the_caps_of_a_master_and_its_filled_sections_are_one_closed_run() {
        /// A glyph by name.
        type Glyph = (&'static str, fn() -> Vec<OwnedPath>);
        let glyphs: [Glyph; 26] = [
            ("router", router),
            ("switch", switch),
            ("firewall", firewall),
            ("cloud", cloud),
            ("access_point", access_point),
            ("load_balancer", load_balancer),
            ("modem", modem),
            ("internet", internet),
            ("server", server),
            ("desktop", desktop),
            ("laptop", laptop),
            ("database_server", database_server),
            ("web_server", web_server),
            ("file_server", file_server),
            ("mail_server", mail_server),
            ("application_server", application_server),
            ("print_server", print_server),
            ("rack_server", rack_server),
            ("tablet", tablet),
            ("mobile_phone", mobile_phone),
            ("telephone", telephone),
            ("printer", printer),
            ("storage", storage),
            ("user", user),
            ("ethernet_is_a_bar", || vec![filled(whole(ETHERNET))]),
            ("ring_is_an_ellipse", || vec![stroke(vec![vec![Seg::Ellipse]])]),
        ];
        for (name, build) in glyphs {
            let paths = build();
            assert!(!paths.is_empty() && paths.len() <= crate::stencil::MAX_SECTIONS, "{name}: {} sections", paths.len());
            let rows: usize = paths.iter().map(|p| p.segs.len()).sum();
            assert!(rows <= crate::stencil::MAX_ROWS, "{name}: {rows} rows");
            for path in paths.iter().filter(|p| p.fill) {
                let runs = path.segs.iter().filter(|s| matches!(s, Seg::Move(..))).count();
                assert_eq!(runs, 1, "{name}: a filled section is one run");
                assert!(matches!(path.segs.first(), Some(Seg::Move(..))), "{name}: it starts with a MoveTo");
            }
        }
    }
}
