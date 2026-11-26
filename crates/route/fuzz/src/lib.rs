//! The input and the properties of the fuzz target `route_scene`: bytes become obstacles, wires, settings and a request, and the router is held to its
//! contract on them. It is a library so that the router's own tests include this file (`crates/route/tests/it/props.rs`) and hold every random scene to the same law the
//! fuzzer holds the bytes to.
//!
//! **The input.** A byte stream read by a cursor that never fails (a stream that runs out reads zeros): a work budget, up to 24 obstacles, up to 8 wires of up to 4 legs, the
//! settings, and the two ends. A number is a signed 16-bit value over 16 now and then replaced by one of the numbers a hostile file holds (NaN, an infinity, the limit and a
//! little past it, a millionth, a billion), so the router is asked about every bad number as well as every good one.
//!
//! **The properties.** The router never panics and never hangs; it never passes the work it was given; the loops walked what they charged (while the work lasted); the same input
//! gives the same answer twice; and a [`Outcome::Routed`] holds [`invariants`]: at most 48 points, the begin and the end exact, every leg level or upright and of some length,
//! leaving by a way the begin allows and arriving against a way the end allows, never turning back on itself, and entering no obstacle grown by the clearance the outcome
//! names unless it is let through or exempt (an obstacle an end stands on is held to its box alone: the stub is let through its clearance).

use diacraft_route::{
    Clearance, Dir, DirSet, End, MAX_COORD, MAX_ROUTE_POINTS, Obstacle, Obstacles, Outcome, Overlap, Point, Rect, Request, Separate, Settings, Wire,
    Wires, Work, route,
};

/// A cursor over the input: a read past the end gives zeros.
pub struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Cursor<'a> {
        Cursor { data, at: 0 }
    }

    pub fn u8(&mut self) -> u8 {
        let b = self.data.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        b
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }

    /// A coordinate: a 16-bit value over 16, and now and then a number a hostile file holds.
    pub fn coord(&mut self) -> f64 {
        let v = f64::from(self.u16() as i16) / 16.0;
        if self.u8().is_multiple_of(40) {
            const SPECIAL: [f64; 8] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_COORD, MAX_COORD + 1.0, -1.0e7, 1.0e-7, 0.0];
            return SPECIAL.get(usize::from(self.u8() % 8)).copied().unwrap_or(0.0);
        }
        v
    }

    /// A length: a byte over 16, and now and then a billion.
    pub fn length(&mut self) -> f64 {
        let v = f64::from(self.u8()) / 16.0;
        if self.u8().is_multiple_of(64) { 1.0e9 } else { v }
    }

    pub fn dir(&mut self) -> Dir {
        Dir::from_index(usize::from(self.u8()))
    }
}

/// Everything a request holds, with the work it may spend.
#[derive(Clone, Debug)]
pub struct Scene {
    pub obstacles: Vec<Obstacle>,
    pub wires: Vec<Wire>,
    pub request: Request,
    pub settings: Settings,
    pub budget: u64,
}

fn end(r: &mut Cursor<'_>, ids: u32) -> End {
    let at = Point::new(r.coord(), r.coord());
    let ways = if r.u8().is_multiple_of(3) { DirSet::ANY } else { DirSet::only(r.dir()) };
    let stands_on = (r.u8().is_multiple_of(2) && ids > 0).then(|| u32::from(r.u8()) % ids);
    End { at, ways, stands_on }
}

/// The scene a byte stream describes.
pub fn scene_from_bytes(data: &[u8]) -> Scene {
    let mut r = Cursor::new(data);
    let budget = 1 + u64::from(r.u16()) * 50;
    let count = u32::from(r.u8() % 25);
    let mut obstacles = Vec::new();
    for id in 0..count {
        let (x0, y0) = (r.coord(), r.coord());
        let (w, h) = (r.length(), r.length());
        let rect = Rect { x0, y0, x1: x0 + w, y1: y0 + h };
        let flags = r.u8();
        obstacles.push(Obstacle { id, rect, pass_level: flags & 1 == 1, pass_upright: flags & 2 == 2 });
    }
    let mut wires = Vec::new();
    for id in 0..u32::from(r.u8() % 9) {
        let mut legs = Vec::new();
        for _ in 0..(r.u8() % 5) {
            let a = Point::new(r.coord(), r.coord());
            let b = if r.u8().is_multiple_of(2) { Point::new(a.x + r.length(), a.y) } else { Point::new(a.x, a.y + r.length()) };
            legs.push((a, b));
        }
        let glued = [(r.u8().is_multiple_of(2)).then(|| u32::from(r.u8() % 4)), None];
        wires.push(Wire { id, legs, glued });
    }
    let settings = Settings {
        clearance: (r.length(), r.length()),
        spacing: (r.length(), r.length()),
        bend: r.length(),
        cross: r.length(),
        near: r.length(),
        lean: (r.u8().is_multiple_of(4)).then(|| (r.dir(), r.length())),
        separate: match r.u8() % 3 {
            0 => Separate::Unrelated,
            1 => Separate::All,
            _ => Separate::None,
        },
        overlap: match r.u8() % 3 {
            0 => Overlap::Related,
            1 => Overlap::Never,
            _ => Overlap::Any,
        },
        margin: r.length(),
    };
    let (begin, end_) = (end(&mut r, count), end(&mut r, count));
    let exempt = (0..count).filter(|_| r.u8().is_multiple_of(8)).collect();
    let glued = [(r.u8().is_multiple_of(2)).then(|| u32::from(r.u8() % 4)), None];
    Scene { obstacles, wires, request: Request { begin, end: end_, exempt, glued }, settings, budget }
}

/// What the fuzz target does with the bytes: panics when a property fails (a crash the fuzzer reports).
pub fn check(data: &[u8]) {
    let scene = scene_from_bytes(data);
    if let Err(words) = check_scene(&scene) {
        panic!("{words}");
    }
}

/// The properties of one scene, `Err` with the words of the first that fails.
pub fn check_scene(scene: &Scene) -> Result<(), String> {
    let obstacles = Obstacles::new(scene.obstacles.clone());
    let wires = Wires::new(scene.wires.clone());
    let mut work = Work::new(scene.budget);
    let first = route(&obstacles, &wires, &scene.request, &scene.settings, &mut work);
    if work.spent() > scene.budget {
        return Err(format!("the work spent {} passes the budget {}", work.spent(), scene.budget));
    }
    if !work.is_spent() && work.charged() != work.walked() {
        return Err(format!("the loops walked {:?} and charged {:?}", work.walked(), work.charged()));
    }
    let mut again = Work::new(scene.budget);
    let second = route(&obstacles, &wires, &scene.request, &scene.settings, &mut again);
    if first != second || work.spent() != again.spent() {
        return Err("the same input gave two answers".to_owned());
    }
    if let Outcome::Routed { points, clearance, .. } = &first {
        invariants(scene, &obstacles, points, *clearance)?;
    }
    Ok(())
}

/// The numbers the router clamps its settings to: a length below 0 or not finite is 0, and above the limit is the limit.
fn clamp(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v.min(MAX_COORD) } else { 0.0 }
}

/// The direction of the leg from `a` to `b` when it is level or upright within `tol` (its smaller side no more than that), and `None` when it is neither or has no length
/// (both sides no more than a billionth).
fn leg_dir(a: Point, b: Point, tol: f64) -> Option<Dir> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax <= 1.0e-9 && ay <= 1.0e-9 || ax.min(ay) > tol {
        return None;
    }
    if ax >= ay { Some(if dx > 0.0 { Dir::Right } else { Dir::Left }) } else { Some(if dy > 0.0 { Dir::Up } else { Dir::Down }) }
}

/// How far the open leg `a`-`b` runs inside the open box `g`, as the longer side of the overlap.
fn depth(a: Point, b: Point, g: &Rect) -> f64 {
    let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
    let across_x = (x1.min(g.x1) - x0.max(g.x0)).max(0.0);
    let across_y = (y1.min(g.y1) - y0.max(g.y0)).max(0.0);
    // A level leg is inside when its `y` is strictly between the box's, and for as long as its `x` span overlaps.
    if (y1 - y0).abs() <= 1.0e-9 {
        if y0 > g.y0 && y0 < g.y1 { across_x } else { 0.0 }
    } else if x0 > g.x0 && x0 < g.x1 {
        across_y
    } else {
        0.0
    }
}

/// What a route that was found must hold (see the module's words).
pub fn invariants(scene: &Scene, obstacles: &Obstacles, points: &[Point], clearance: Clearance) -> Result<(), String> {
    let tol = 1.0e-6;
    let (first, last) = (points.first().copied(), points.last().copied());
    let (Some(first), Some(last)) = (first, last) else { return Err("a route with no points".to_owned()) };
    if points.len() < 2 || points.len() > MAX_ROUTE_POINTS {
        return Err(format!("a route of {} points", points.len()));
    }
    if first != scene.request.begin.at || last != scene.request.end.at {
        return Err("the route does not start and end on the ends".to_owned());
    }
    let mut dirs = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair.first().copied().unwrap_or_default(), pair.get(1).copied().unwrap_or_default());
        let Some(d) = leg_dir(a, b, tol) else { return Err(format!("the leg {a:?} to {b:?} is slanted or has no length")) };
        dirs.push(d);
    }
    if dirs.windows(2).any(|w| matches!(w, [a, b] if *a == b.opposite())) {
        return Err("the route turns back on itself".to_owned());
    }
    match (dirs.first(), dirs.last()) {
        (Some(f), Some(l)) if scene.request.begin.ways.contains(*f) && scene.request.end.ways.contains(l.opposite()) => {}
        _ => return Err("the route leaves or arrives by a way the end does not allow".to_owned()),
    }
    let factor = match clearance {
        Clearance::Full => 1.0,
        Clearance::Half => 0.5,
        Clearance::None => 0.0,
    };
    let (cx, cy) = (clamp(scene.settings.clearance.0) * factor, clamp(scene.settings.clearance.1) * factor);
    let mut ids: Vec<u32> = scene.obstacles.iter().map(|o| o.id).collect();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let Some(o) = obstacles.get(id) else { continue };
        if scene.request.exempt.contains(&id) {
            continue;
        }
        let stands = scene.request.begin.stands_on == Some(id) || scene.request.end.stands_on == Some(id);
        let g = if stands { o.rect } else { o.rect.grown(cx, cy) };
        for (pair, d) in points.windows(2).zip(&dirs) {
            let (a, b) = (pair.first().copied().unwrap_or_default(), pair.get(1).copied().unwrap_or_default());
            let passes = if d.is_level() { o.pass_level } else { o.pass_upright };
            if !passes && depth(a, b, &g) > tol {
                return Err(format!("the leg {a:?} to {b:?} enters the obstacle {id} grown to {g:?}"));
            }
        }
    }
    Ok(())
}
