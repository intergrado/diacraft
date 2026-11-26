//! Plain `f64` geometry, x right and y up, and the number checks every input passes before any arithmetic.

/// The farthest a coordinate may be from the origin: a point or a box beyond it (or not a finite number) is left out or refused.
pub const MAX_COORD: f64 = 1.0e6;

/// Lines and points closer than this are one.
pub const EPS: f64 = 1.0e-9;

/// A number that is finite and within [`MAX_COORD`].
pub fn coord_ok(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

/// A length or a cost put right: a number that is not finite or is below 0 is 0, and one above `cap` is `cap`.
pub fn clean(v: f64, cap: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v.min(cap) } else { 0.0 }
}

/// A point on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// Whether both numbers are finite and within [`MAX_COORD`].
    pub fn is_ok(&self) -> bool {
        coord_ok(self.x) && coord_ok(self.y)
    }

    /// The point `d` along `way` from this one.
    pub fn moved(&self, way: Dir, d: f64) -> Point {
        let (dx, dy) = way.vector();
        Point { x: self.x + dx * d, y: self.y + dy * d }
    }
}

/// An axis-aligned box, `x0 <= x1` and `y0 <= y1` when made by [`Rect::of`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    /// The box through two corners, whichever way round they come.
    pub fn of(a: Point, b: Point) -> Rect {
        Rect { x0: a.x.min(b.x), y0: a.y.min(b.y), x1: a.x.max(b.x), y1: a.y.max(b.y) }
    }

    /// The box grown by `dx` on its left and right and by `dy` at its top and bottom.
    pub fn grown(&self, dx: f64, dy: f64) -> Rect {
        Rect { x0: self.x0 - dx, y0: self.y0 - dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }

    /// Whether the two boxes share a point (touching counts).
    pub fn meets(&self, other: &Rect) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    /// Whether the two boxes share more than their edges.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    /// The box round both.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    /// Whether every number is finite and within [`MAX_COORD`] and the corners are the right way round.
    pub fn is_ok(&self) -> bool {
        coord_ok(self.x0) && coord_ok(self.y0) && coord_ok(self.x1) && coord_ok(self.y1) && self.x0 <= self.x1 && self.y0 <= self.y1
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Whether `p` is inside the box or on its edge.
    pub fn holds(&self, p: Point) -> bool {
        self.x0 <= p.x && p.x <= self.x1 && self.y0 <= p.y && p.y <= self.y1
    }
}

/// A way along an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dir {
    Right,
    Up,
    Left,
    Down,
}

/// The four ways, in the order of [`Dir::index`].
pub const DIRS: [Dir; 4] = [Dir::Right, Dir::Up, Dir::Left, Dir::Down];

impl Dir {
    pub const fn index(self) -> usize {
        match self {
            Dir::Right => 0,
            Dir::Up => 1,
            Dir::Left => 2,
            Dir::Down => 3,
        }
    }

    pub const fn from_index(i: usize) -> Dir {
        match i & 3 {
            0 => Dir::Right,
            1 => Dir::Up,
            2 => Dir::Left,
            _ => Dir::Down,
        }
    }

    pub const fn opposite(self) -> Dir {
        Dir::from_index(self.index() + 2)
    }

    /// The unit step along the way.
    pub const fn vector(self) -> (f64, f64) {
        match self {
            Dir::Right => (1.0, 0.0),
            Dir::Up => (0.0, 1.0),
            Dir::Left => (-1.0, 0.0),
            Dir::Down => (0.0, -1.0),
        }
    }

    /// Whether the way runs along the x axis.
    pub const fn is_level(self) -> bool {
        matches!(self, Dir::Right | Dir::Left)
    }
}

/// The bends a move from heading `from` to heading `to` makes: 0 for the same way, 1 for a quarter turn, `None` for a reversal (a route never turns back on itself).
pub const fn turn_cost(from: Dir, to: Dir) -> Option<u8> {
    match (to.index() + 4 - from.index()) % 4 {
        0 => Some(0),
        2 => None,
        _ => Some(1),
    }
}

/// The ways an end may leave: one, or any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirSet(u8);

impl DirSet {
    /// Any of the four ways.
    pub const ANY: DirSet = DirSet(0b1111);

    /// Just `way`.
    pub const fn only(way: Dir) -> DirSet {
        DirSet(1 << way.index())
    }

    pub const fn contains(self, way: Dir) -> bool {
        self.0 >> way.index() & 1 == 1
    }

    /// Whether more than one way is allowed.
    pub const fn is_any(self) -> bool {
        self.0.count_ones() != 1
    }

    /// The single way, when there is just one.
    pub const fn single(self) -> Option<Dir> {
        match self.0 {
            0b0001 => Some(Dir::Right),
            0b0010 => Some(Dir::Up),
            0b0100 => Some(Dir::Left),
            0b1000 => Some(Dir::Down),
            _ => None,
        }
    }
}

/// The sign of `v` with a dead band of [`EPS`]: -1, 0 or 1.
pub fn sign(v: f64) -> i8 {
    if v > EPS {
        1
    } else if v < -EPS {
        -1
    } else {
        0
    }
}
