//! The picture on the Layout and Routing tab (the preview): two boxes with a third between them, joined by a connector
//! that **the router routes** (`diacraft-route`, the same crate the engine routes with) with the style, the direction, the appearance and the clearance chosen in the draft, and a
//! second line that crosses it once, where the jump of the draft's style is drawn. No source says what the preview draws, so this is our design call.
//!
//! **The scene** is made in the frame of a flow that runs to the right, in paper inches (the boxes are one inch wide, and the clearance and the spacing are the draft's own lengths on
//! paper), and turned to the flow the style has (top to bottom, right to left...) when it is put on the page. The first box and the last stand a little off each other's line, so
//! that a straight connector is seen to slant and the third box is in the way of it; the connector leaves the first box to the right, arrives at the last from the left, and goes
//! round the third (a right angle, network or directional style goes round; a straight connector and a centre-to-centre one are one leg through anything).
//! **A curved connector** is drawn through the right-angle route's corners (draws it so), and carries no jump.
//!
//! **The jump** is the one `connector.line_jumps` would make (`jumps.rs`): which of the two lines carries it follows Add line jumps to (1 the flatter, 2 the steeper, 3 and 4 the
//! line that is later in the drawing order, which is the second line, 5 the one that is earlier), its chord is the spacing between connectors times the size of its direction (the
//! horizontal size on a level line, the vertical on an upright one), and its look is the style's: an arc, a gap, a square or a many-sided arc. **It is drawn four times its size**
//! ([`SHOWN_LARGER`]) so that the look of one can be told from another at the size of the picture: its real chord is 1/12 inch at the root's values.
//!
//! The scene is computed again only when something it shows changes ([`Preview::scene`]), and counted (`computed`, in the window's state) so that a test can tell.

use diacraft_engine::draw::{Flow, RouteStyle};
use diacraft_route::{Dir, DirSet, End, Obstacle, Obstacles, Outcome, Point, Rect, Request, Settings, Wires, Work, route};
use egui::{CornerRadius, Painter, Pos2, Stroke, StrokeKind, pos2};
use serde_json::{Value, json};

use crate::theme::Tokens;

/// A point of the scene: paper inches, x to the right and y up.
pub(super) type P = (f64, f64);

/// The boxes in the frame of a flow to the right: the first, the third (in the way) and the last, as `[x0, y0, x1, y1]`.
const BOXES: [[f64; 4]; 3] = [[0.0, -0.7, 1.0, 0.1], [2.0, -0.9, 3.0, 0.5], [4.8, -0.1, 5.8, 0.7]];
/// Where the connector leaves the first box and arrives at the last.
const BEGIN: P = (1.0, -0.3);
const END: P = (4.8, 0.3);
/// Where the second line crosses the flow: clear of the legs a centred route stands on (half way between the first box and the third, 1.5, and half way between the third and the last,
/// 3.9, whatever the clearance), with room for a chord of an inch on its leg to the right of it; and how far it reaches on each side of the route.
const WIRE_AT: f64 = 3.3;
const WIRE_REACH: f64 = 0.7;
/// The most the picture's window shows at least, in the frame of the flow: `[u0, v0, u1, v1]`, so that it does not grow and shrink as the clearance is typed.
const WINDOW: [f64; 4] = [-0.15, -1.1, 5.95, 1.0];
/// How much larger than it is a jump is drawn.
pub(super) const SHOWN_LARGER: f64 = 4.0;
/// The longest chord a jump is drawn with, in inches.
const LONGEST_CHORD: f64 = 1.0;
/// The most a clearance or a spacing may be, in inches (one paper inch).
const LONGEST_GAP: f64 = 1.0;
/// The cost of a bend, a crossing and a first leg against the flow, in paper inches (item 6), and the work a preview's route may take.
const WORK: u64 = 200_000;
const LEAN: f64 = 0.5;
/// How many times the corners of a curved route are cut.
const SMOOTHING: usize = 3;

/// The look of a jump (`LineJumpStyle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Look {
    Arc,
    Gap,
    Square,
    /// A many-sided arc of this many sides, 2 to 7.
    Sides(u8),
}

impl Look {
    /// The look a `LineJumpStyle` code says: 0 and 1 an arc, 2 a gap, 3 a square, 4 to 9 two to seven sides; another is an arc.
    pub(super) fn of_code(code: u32) -> Look {
        match code {
            2 => Look::Gap,
            3 => Look::Square,
            4..=9 => Look::Sides(u8::try_from(code - 2).unwrap_or(2)),
            _ => Look::Arc,
        }
    }

    pub(super) fn word(self) -> &'static str {
        match self {
            Look::Arc => "arc",
            Look::Gap => "gap",
            Look::Square => "square",
            Look::Sides(2) => "sides2",
            Look::Sides(3) => "sides3",
            Look::Sides(4) => "sides4",
            Look::Sides(5) => "sides5",
            Look::Sides(6) => "sides6",
            Look::Sides(_) => "sides7",
        }
    }
}

/// What the picture shows: the draft's choices and lengths, all in paper inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Inputs {
    pub style: RouteStyle,
    pub curved: bool,
    /// The clearance from a box (Connector to shape) beside upright legs and beside level legs.
    pub clearance: (f64, f64),
    /// The spacing between connectors (Connector to connector), along x and along y.
    pub spacing: (f64, f64),
    /// Add line jumps to, as the cell's code: 0 to 5.
    pub jump_to: u32,
    pub look: Look,
    /// The sizes of a jump on a level line and on an upright one (Horizontal size and Vertical size), as multiples of the spacing.
    pub sizes: (f64, f64),
}

/// Which of the two lines carries the jump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Carrier {
    Route,
    Wire,
}

/// The jump drawn at the crossing.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Jump {
    pub carrier: Carrier,
    /// `level` or `upright`: the carrier's direction (a slanting line that is flatter than steep is level).
    pub on: &'static str,
    pub look: Look,
    /// The chord as drawn, in inches.
    pub chord: f64,
    /// Where the line leaves its own course for the jump and where it comes back to it.
    pub entry: P,
    pub exit: P,
    /// The points between them (none for a gap).
    pub shape: Vec<P>,
}

/// What the picture is made of, on the page's own frame (x to the right, y up).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Scene {
    pub boxes: [[f64; 4]; 3],
    /// `routed` (the router's route), `straight` (a one-leg style), or what the router said when it found none: `no_way`, `out_of_work` or `refused`; the last three draw a straight line.
    pub outcome: &'static str,
    pub route: Vec<P>,
    pub curved: bool,
    pub wire: [P; 2],
    pub crossing: Option<P>,
    pub jump: Option<Jump>,
}

/// The legs of a polyline: each point with the next.
fn legs(points: &[P]) -> impl Iterator<Item = (P, P)> + '_ {
    points.iter().copied().zip(points.iter().copied().skip(1))
}

/// The flow a style has on the page (the way its connectors run): to the right where it has none.
fn flow_of(style: RouteStyle) -> Flow {
    match style {
        RouteStyle::OrgChart(flow) | RouteStyle::Flowchart(flow) | RouteStyle::Tree(flow) | RouteStyle::Simple(flow) => flow,
        RouteStyle::SimpleUprightFirst => Flow::TopToBottom,
        _ => Flow::LeftToRight,
    }
}

/// Whether the style leans (a flow of its own), and so is given the lean cost.
fn leans(style: RouteStyle) -> bool {
    matches!(style, RouteStyle::OrgChart(_) | RouteStyle::Flowchart(_) | RouteStyle::Tree(_) | RouteStyle::Simple(_))
}

/// A point of the frame of a flow to the right (`u` along the flow, `v` across it, up) on the page, for the flow the style has.
fn place(flow: Flow, (u, v): P) -> P {
    match flow {
        Flow::LeftToRight => (u, v),
        Flow::RightToLeft => (-u, v),
        Flow::TopToBottom => (v, -u),
        Flow::BottomToTop => (v, u),
    }
}

/// A box of the frame of the flow on the page.
fn place_box(flow: Flow, b: [f64; 4]) -> [f64; 4] {
    let (p, q) = (place(flow, (b[0], b[1])), place(flow, (b[2], b[3])));
    [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)]
}

fn length(gap: f64) -> f64 {
    if gap.is_finite() { gap.clamp(0.0, LONGEST_GAP) } else { 0.0 }
}

/// The router's route from the first box to the last, in the frame of the flow, or why it made none.
fn routed(i: &Inputs) -> (Vec<P>, &'static str) {
    let boxes: Vec<Obstacle> = BOXES
        .iter()
        .zip(1_u32..)
        .map(|(b, id)| Obstacle { id, rect: Rect { x0: b[0], y0: b[1], x1: b[2], y1: b[3] }, pass_level: false, pass_upright: false })
        .collect();
    let begin = End { at: Point::new(BEGIN.0, BEGIN.1), ways: DirSet::only(Dir::Right), stands_on: Some(1) };
    let end = End { at: Point::new(END.0, END.1), ways: DirSet::only(Dir::Left), stands_on: Some(3) };
    let request = Request { begin, end, exempt: Vec::new(), glued: [Some(1), Some(3)] };
    let settings = Settings {
        clearance: (length(i.clearance.0), length(i.clearance.1)),
        spacing: (length(i.spacing.0), length(i.spacing.1)),
        lean: leans(i.style).then_some((Dir::Right, LEAN)),
        ..Settings::default()
    };
    match route(&Obstacles::new(boxes), &Wires::new(Vec::new()), &request, &settings, &mut Work::new(WORK)) {
        Outcome::Routed { points, .. } => (points.iter().map(|p| (p.x, p.y)).collect(), "routed"),
        Outcome::NoWay => (vec![BEGIN, END], "no_way"),
        Outcome::OutOfWork => (vec![BEGIN, END], "out_of_work"),
        Outcome::Refused(_) => (vec![BEGIN, END], "refused"),
    }
}

/// The middle of a box of the frame of the flow.
fn middle(b: [f64; 4]) -> P {
    ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
}

/// The route in the frame of the flow, and how it was made.
fn route_in_flow(i: &Inputs) -> (Vec<P>, &'static str) {
    match i.style {
        RouteStyle::Straight => (vec![BEGIN, END], "straight"),
        RouteStyle::CentreToCentre => (vec![middle(BOXES[0]), middle(BOXES[2])], "straight"),
        _ => routed(i),
    }
}

/// The steepness of a line: 0 for a level one, large for an upright one.
fn steepness(a: P, b: P) -> f64 {
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    if dx < 1.0e-12 { f64::INFINITY } else { dy / dx }
}

/// Where the segment `a`-`b` crosses `c`-`d` strictly inside both: the point and how far along `a`-`b` it is (0 to 1).
fn crossing_of(a: P, b: P, c: P, d: P) -> Option<(P, f64)> {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    let q = (c.0 - a.0, c.1 - a.1);
    let t = (q.0 * s.1 - q.1 * s.0) / denominator;
    let u = (q.0 * r.1 - q.1 * r.0) / denominator;
    let inside = |x: f64| x > 1.0e-9 && x < 1.0 - 1.0e-9;
    (inside(t) && inside(u)).then_some(((a.0 + t * r.0, a.1 + t * r.1), t))
}

/// The points of the bump a jump of `look` makes over a chord of `2 * a`, as (along the line, off it), from one end of the chord to the other. A gap has none.
fn bump(look: Look, a: f64) -> Vec<(f64, f64)> {
    let steps = |n: u32| -> Vec<(f64, f64)> {
        (0..=n)
            .map(|k| {
                let angle = std::f64::consts::PI * f64::from(k) / f64::from(n.max(1));
                (-a * angle.cos(), a * angle.sin())
            })
            .collect()
    };
    match look {
        Look::Arc => steps(12),
        Look::Gap => Vec::new(),
        Look::Square => vec![(-a, 0.0), (-a, a), (a, 0.0 + a), (a, 0.0)],
        Look::Sides(n) => steps(u32::from(n)),
    }
}

/// The jump at `at` on the line `a`-`b`: its ends on the line and its bump, as the page's points. The bump is to the left of the way the line runs (up on a level line, to the left on an upright one).
fn jump_on(i: &Inputs, carrier: Carrier, (a, b): (P, P), at: P) -> Option<Jump> {
    let flat = steepness(a, b) <= 1.0;
    let (size, spacing) = if flat { (i.sizes.0, i.spacing.0) } else { (i.sizes.1, i.spacing.1) };
    let chord = (size * length(spacing) * SHOWN_LARGER).min(LONGEST_CHORD);
    let reach = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    if !(chord.is_finite() && chord > 1.0e-9 && reach > 1.0e-9) {
        return None;
    }
    // The line's direction, turned to run to the right (or up, for an upright one), and the normal to its left.
    let mut d = ((b.0 - a.0) / reach, (b.1 - a.1) / reach);
    if d.0 < -1.0e-9 || (d.0.abs() <= 1.0e-9 && d.1 < 0.0) {
        d = (-d.0, -d.1);
    }
    let n = (-d.1, d.0);
    // A jump lies at least half a chord from both ends of its line: the engine makes none that does not fit. The picture's is drawn smaller than asked instead, so that it
    // is still seen while the size is being typed.
    let (to_a, to_b) = (((at.0 - a.0).powi(2) + (at.1 - a.1).powi(2)).sqrt(), ((at.0 - b.0).powi(2) + (at.1 - b.1).powi(2)).sqrt());
    let chord = chord.min(2.0 * to_a.min(to_b));
    if chord <= 1.0e-9 {
        return None;
    }
    let half = chord / 2.0;
    let on_page = |s: f64, h: f64| (at.0 + s * d.0 + h * n.0, at.1 + s * d.1 + h * n.1);
    Some(Jump {
        carrier,
        on: if flat { "level" } else { "upright" },
        look: i.look,
        chord,
        entry: on_page(-half, 0.0),
        exit: on_page(half, 0.0),
        shape: bump(i.look, half).into_iter().map(|(s, h)| on_page(s, h)).collect(),
    })
}

/// Which line carries the jump the page's `LineJumpCode` asks for, of the route's leg and the second line, or none.
fn carrier_of(code: u32, route: (P, P), wire: (P, P)) -> Option<Carrier> {
    let (leg, other) = (steepness(route.0, route.1), steepness(wire.0, wire.1));
    match code {
        1 => Some(if leg <= other { Carrier::Route } else { Carrier::Wire }),
        2 => Some(if leg > other { Carrier::Route } else { Carrier::Wire }),
        3 | 4 => Some(Carrier::Wire),
        5 => Some(Carrier::Route),
        _ => None,
    }
}

/// The scene the inputs make.
pub(super) fn build(i: &Inputs) -> Scene {
    let flow = flow_of(i.style);
    let (local, outcome) = route_in_flow(i);
    let route: Vec<P> = local.iter().map(|p| place(flow, *p)).collect();
    // The second line crosses the route where it passes `WIRE_AT`, and reaches a little more than half an inch to each side of it.
    let across = legs(&local)
        .find_map(|(a, b)| {
            let at = WIRE_AT;
            ((a.0 < at && at < b.0) || (b.0 < at && at < a.0)).then(|| a.1 + (b.1 - a.1) * (at - a.0) / (b.0 - a.0))
        })
        .unwrap_or(0.0);
    let wire = [place(flow, (WIRE_AT, across - WIRE_REACH)), place(flow, (WIRE_AT, across + WIRE_REACH))];
    // Where they cross, and the jump (a curved connector has none).
    let crossing = legs(&route).find_map(|(a, b)| crossing_of(a, b, wire[0], wire[1]).map(|(at, _)| ((a, b), at)));
    let jump = match (crossing, i.curved) {
        (Some((leg, at)), false) => carrier_of(i.jump_to, leg, (wire[0], wire[1])).and_then(|carrier| {
            let line = if carrier == Carrier::Route { leg } else { (wire[0], wire[1]) };
            jump_on(i, carrier, line, at)
        }),
        _ => None,
    };
    Scene { boxes: BOXES.map(|b| place_box(flow, b)), outcome, route, curved: i.curved, wire, crossing: crossing.map(|(_, at)| at), jump }
}

/// The lines to draw for the connector and for the second line, as polylines on the page: the route's corners cut when it is curved, and each line with its jump.
pub(super) fn strokes(scene: &Scene) -> (Vec<Vec<P>>, Vec<Vec<P>>) {
    let route = if scene.curved { smoothed(&scene.route) } else { scene.route.clone() };
    let wire = scene.wire.to_vec();
    let Some(jump) = &scene.jump else { return (vec![route], vec![wire]) };
    // The leg the crossing is on: the one whose two ends have the jump's ends between them.
    let split = |line: &[P]| -> Vec<Vec<P>> {
        let n = legs(line).position(|(a, b)| on_segment(a, b, jump.entry, jump.exit));
        let Some(n) = n else { return vec![line.to_vec()] };
        let (before, after) = (line.get(..=n).unwrap_or_default(), line.get(n + 1..).unwrap_or_default());
        let mut first: Vec<P> = before.to_vec();
        first.push(jump.entry);
        let mut second: Vec<P> = vec![jump.exit];
        second.extend_from_slice(after);
        if jump.shape.is_empty() {
            return vec![first, second];
        }
        first.extend(jump.shape.iter().copied());
        first.push(jump.exit);
        first.extend_from_slice(after);
        vec![first]
    };
    match jump.carrier {
        Carrier::Route => (split(&route), vec![wire]),
        Carrier::Wire => (vec![route], split(&wire)),
    }
}

/// Whether `entry` and `exit` both lie on the segment `a`-`b`.
fn on_segment(a: P, b: P, entry: P, exit: P) -> bool {
    let near = |p: P| {
        let (ab, ap) = ((b.0 - a.0, b.1 - a.1), (p.0 - a.0, p.1 - a.1));
        let cross = ab.0 * ap.1 - ab.1 * ap.0;
        let along = ab.0 * ap.0 + ab.1 * ap.1;
        cross.abs() < 1.0e-6 && along >= -1.0e-9 && along <= ab.0 * ab.0 + ab.1 * ab.1 + 1.0e-9
    };
    near(entry) && near(exit)
}

/// The corners of a route cut `SMOOTHING` times (Chaikin's cut, the ends kept): the curve a curved connector is drawn as.
fn smoothed(points: &[P]) -> Vec<P> {
    let mut line = points.to_vec();
    for _ in 0..SMOOTHING {
        if line.len() < 3 {
            break;
        }
        let last = line.len() - 1;
        let mut next: Vec<P> = Vec::with_capacity(line.len() * 2);
        next.extend(line.first().copied());
        for (n, (a, b)) in legs(&line).enumerate() {
            if n > 0 {
                next.push((0.75 * a.0 + 0.25 * b.0, 0.75 * a.1 + 0.25 * b.1));
            }
            if n + 1 < last {
                next.push((0.25 * a.0 + 0.75 * b.0, 0.25 * a.1 + 0.75 * b.1));
            }
        }
        next.extend(line.last().copied());
        line = next;
    }
    line
}

/// What the tab keeps of the picture: the scene for the inputs it was made for, and how many times it was made.
#[derive(Default)]
pub(super) struct Preview {
    made_for: Option<Inputs>,
    scene: Option<Scene>,
    computed: u64,
}

impl Preview {
    /// The scene for `inputs`, made again only when they are not the ones the last was made for.
    pub(super) fn scene(&mut self, inputs: Inputs) -> &Scene {
        if self.made_for != Some(inputs) || self.scene.is_none() {
            self.scene = Some(build(&inputs));
            self.made_for = Some(inputs);
            self.computed = self.computed.saturating_add(1);
        }
        self.scene.get_or_insert_with(|| build(&inputs))
    }

    /// What the window reports of it: the facts of the scene and how many times it was made (`null` before the tab has drawn it).
    pub(super) fn state_json(&self) -> Value {
        let Some(scene) = &self.scene else { return Value::Null };
        let points = |list: &[P]| -> Vec<Value> { list.iter().map(|(x, y)| json!([x, y])).collect() };
        json!({
            "computed": self.computed,
            "outcome": scene.outcome,
            "boxes": scene.boxes,
            "route": points(&scene.route),
            "curved": scene.curved,
            "wire": points(&scene.wire),
            "crossing": scene.crossing.map(|(x, y)| json!([x, y])),
            "jump": scene.jump.as_ref().map(|j| json!({
                "on": j.on,
                "carrier": if j.carrier == Carrier::Route { "route" } else { "wire" },
                "style": j.look.word(),
                "chord": j.chord,
                "shape": points(&j.shape),
            })),
        })
    }
}

/// The page's rectangle the picture shows, in the frame of the page: the window of [`WINDOW`] turned to the flow, and everything the scene draws.
fn bounds(scene: &Scene, flow: Flow) -> [f64; 4] {
    let (p, q) = (place(flow, (WINDOW[0], WINDOW[1])), place(flow, (WINDOW[2], WINDOW[3])));
    let mut b = [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)];
    let mut take = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    };
    for r in &scene.boxes {
        take(r[0], r[1]);
        take(r[2], r[3]);
    }
    for (x, y) in scene.route.iter().chain(scene.wire.iter()) {
        take(*x, *y);
    }
    if let Some(j) = &scene.jump {
        for (x, y) in &j.shape {
            take(*x, *y);
        }
    }
    b
}

/// Paints the scene in `rect`, in the tokens' colours, fitted and centred with room round it.
pub(super) fn paint(painter: &Painter, rect: egui::Rect, t: &Tokens, scene: &Scene, style: RouteStyle) {
    // The picture is the tab's own, on the colour of a box of the window and in the ink of an icon, so that it reads in the dark theme as in the light one.
    painter.rect(rect, CornerRadius::same(3), t.input_fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let b = bounds(scene, flow_of(style));
    let inside = rect.shrink(8.0);
    let (w, h) = ((b[2] - b[0]).max(1.0e-6), (b[3] - b[1]).max(1.0e-6));
    let k = (f64::from(inside.width()) / w).min(f64::from(inside.height()) / h);
    if !(k.is_finite() && k > 0.0) {
        return;
    }
    let (used_w, used_h) = ((w * k) as f32, (h * k) as f32);
    let origin = pos2(inside.center().x - used_w / 2.0, inside.center().y - used_h / 2.0);
    // The page's y runs up and the screen's down.
    let screen = |(x, y): P| -> Pos2 { pos2(origin.x + ((x - b[0]) * k) as f32, origin.y + ((b[3] - y) * k) as f32) };
    for r in &scene.boxes {
        let area = egui::Rect::from_two_pos(screen((r[0], r[1])), screen((r[2], r[3])));
        painter.rect(area, CornerRadius::same(2), t.icon_fill, Stroke::new(1.0, t.icon_ink), StrokeKind::Inside);
    }
    let (route, wire) = strokes(scene);
    let line = |points: &[P], stroke: Stroke| {
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points.iter().map(|p| screen(*p)).collect(), stroke));
        }
    };
    let found = matches!(scene.outcome, "routed" | "straight");
    let ink = if found { t.accent } else { t.text_dim };
    for part in &wire {
        line(part, Stroke::new(1.5, t.icon_ink));
    }
    for part in &route {
        line(part, Stroke::new(2.0, ink));
    }
    for end in scene.wire {
        painter.circle_filled(screen(end), 2.5, t.glue_off);
    }
    // The connector is glued at its begin (a dot) and ends in an arrow, so that the way it flows is seen.
    if let Some(begin) = scene.route.first() {
        painter.circle_filled(screen(*begin), 3.0, t.glue_on);
    }
    let last_leg = route.last().and_then(|part| match part.as_slice() {
        [.., before, tip] => Some((screen(*before), screen(*tip))),
        _ => None,
    });
    if let Some((before, tip)) = last_leg {
        let away = tip - before;
        if away.length() > 1.0e-3 {
            let along = away / away.length();
            let across = egui::vec2(-along.y, along.x);
            let (back, half) = (8.0, 4.0);
            let base = tip - along * back;
            painter.add(egui::Shape::convex_polygon(vec![tip, base + across * half, base - across * half], ink, Stroke::NONE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(style: RouteStyle) -> Inputs {
        Inputs {
            style,
            curved: false,
            clearance: (0.125, 0.125),
            spacing: (0.125, 0.125),
            jump_to: 1,
            look: Look::Arc,
            sizes: (2.0 / 3.0, 2.0 / 3.0),
        }
    }

    fn near(a: P, b: P) -> bool {
        (a.0 - b.0).abs() < 1.0e-9 && (a.1 - b.1).abs() < 1.0e-9
    }

    #[test]
    fn the_look_of_a_style_code_is_the_jump_rules() {
        assert_eq!(
            [0, 1, 2, 3, 4, 9, 10, 99].map(Look::of_code),
            [Look::Arc, Look::Arc, Look::Gap, Look::Square, Look::Sides(2), Look::Sides(7), Look::Arc, Look::Arc]
        );
        for n in 2..=7 {
            assert_eq!(Look::Sides(n).word(), format!("sides{n}"));
        }
    }

    #[test]
    fn a_bump_has_the_points_each_look_needs_and_starts_and_ends_on_the_line() {
        for (look, count) in [(Look::Arc, 13), (Look::Gap, 0), (Look::Square, 4), (Look::Sides(2), 3), (Look::Sides(7), 8)] {
            let shape = bump(look, 0.5);
            assert_eq!(shape.len(), count, "{look:?}");
            if let (Some(first), Some(last)) = (shape.first(), shape.last()) {
                assert!(near(*first, (-0.5, 0.0)) && near(*last, (0.5, 0.0)), "{look:?}: {shape:?}");
            }
            assert!(shape.iter().all(|(_, h)| *h >= -1.0e-12 && *h <= 0.5 + 1.0e-12), "{look:?}: it rises to half a chord at most");
        }
    }

    #[test]
    fn the_flow_turns_the_scene_and_keeps_its_shape() {
        let right = build(&inputs(RouteStyle::RightAngle));
        let down = build(&inputs(RouteStyle::Flowchart(Flow::TopToBottom)));
        assert_eq!(right.route.len(), down.route.len(), "the same route turned a quarter");
        for (r, d) in right.route.iter().zip(&down.route) {
            assert!(near(*d, (r.1, -r.0)), "{r:?} turned is {d:?}");
        }
    }

    #[test]
    fn a_curve_keeps_its_ends_and_has_more_points_than_its_corners() {
        let corners = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (2.0, 1.0)];
        let curve = smoothed(&corners);
        assert!(near(curve[0], corners[0]) && near(*curve.last().unwrap(), corners[3]));
        assert!(curve.len() > corners.len());
        assert_eq!(smoothed(&corners[..2]), corners[..2].to_vec(), "one leg stays one leg");
    }

    #[test]
    fn a_chord_is_never_drawn_longer_than_an_inch_and_a_jump_of_no_size_is_none() {
        let mut i = inputs(RouteStyle::RightAngle);
        i.sizes = (10.0, 10.0);
        let big = build(&i).jump.expect("a jump of the largest size is still drawn");
        assert!((big.chord - LONGEST_CHORD).abs() < 1.0e-9, "an inch at most, and the third box and the last leave room for it: {}", big.chord);
        i.sizes = (1.0, 1.0);
        let small = build(&i).jump.expect("a jump of the size 1 is drawn");
        assert!((small.chord - 0.5).abs() < 1.0e-9, "the spacing 1/8 inch, times 1, drawn four times as large: {}", small.chord);
        i.sizes = (0.0, 0.0);
        assert!(build(&i).jump.is_none(), "No jump of no size");
    }

    #[test]
    fn the_gap_cuts_the_line_in_two_and_a_bump_makes_it_one_longer_line() {
        let mut i = inputs(RouteStyle::RightAngle);
        i.look = Look::Gap;
        let (route, wire) = strokes(&build(&i));
        assert_eq!((route.len(), wire.len()), (2, 1), "the connector carries the jump and is in two pieces");
        i.look = Look::Arc;
        let (route, _) = strokes(&build(&i));
        assert_eq!(route.len(), 1);
        assert!(route[0].len() > build(&i).route.len(), "the bump's points are in the line");
    }

    #[test]
    fn hostile_numbers_make_a_scene_and_no_panic() {
        for gap in [f64::NAN, f64::INFINITY, -1.0, 1.0e300] {
            let mut i = inputs(RouteStyle::RightAngle);
            i.clearance = (gap, gap);
            i.spacing = (gap, gap);
            i.sizes = (gap, gap);
            let scene = build(&i);
            assert_eq!(scene.boxes.len(), 3);
            assert!(scene.route.len() >= 2);
        }
    }
}
