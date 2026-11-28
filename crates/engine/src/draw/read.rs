//! Reading a drawing for an edit: cell numbers found by inheritance, the pose of a shape, where its connection points are on the page.
//!
//! Values are the saved ones; an edit command has recalculated the page before it reads what a formula gave. **A cell that holds a formula and no value
//! saved anywhere on the way to it is read by what its formula works out to**, as the picture draws it ([`super::worked`]): a shape pinned at `=1+1` with no
//! `V` is at 2, and is picked, moved, snapped to and shown there, and not at the origin. Everything is in the page's drawing units; the commands convert paper inches at their edge.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, SectionKey, ShapeRef, SheetRef};

use super::route::P;
use super::worked::Worked;

// How many times a shape was looked for by its `ID` from the top of a page, for the tests that hold a command to doing it once for all the shapes it names.
#[cfg(test)]
thread_local! {
    pub(crate) static FINDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many poses were read (each is eight cells found by inheritance), for the tests that hold a command to reading each group above its shapes once.
    pub(crate) static POSES_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The cells that set a shape's frame, with the page's `x` right and `y` up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pin: P,
    pub loc: P,
    pub angle: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Pose {
    /// A point of the shape's frame in its parent's: subtract the local pin, flip, turn counterclockwise, add the pin.
    pub fn up(&self, p: P) -> P {
        let (x, y) = self.turn((p.0 - self.loc.0, p.1 - self.loc.1));
        (x + self.pin.0, y + self.pin.1)
    }

    /// A direction (a vector) of the shape's frame in its parent's: flips and the turn, and no move.
    pub fn up_vector(&self, v: P) -> P {
        self.turn(v)
    }

    fn turn(&self, v: P) -> P {
        let (x, y) = (if self.flip_x { -v.0 } else { v.0 }, if self.flip_y { -v.1 } else { v.1 });
        let (s, c) = self.angle.sin_cos();
        (x * c - y * s, x * s + y * c)
    }

    /// A direction of the parent's frame in the shape's: the steps of [`Pose::up_vector`] in reverse.
    pub fn down_vector(&self, v: P) -> P {
        let (s, c) = (-self.angle).sin_cos();
        let (x, y) = (v.0 * c - v.1 * s, v.0 * s + v.1 * c);
        (if self.flip_x { -x } else { x }, if self.flip_y { -y } else { y })
    }

    /// A point of the parent's frame in the shape's: the steps of [`Pose::up`] in reverse.
    pub fn down(&self, p: P) -> P {
        let (s, c) = (-self.angle).sin_cos();
        let (dx, dy) = (p.0 - self.pin.0, p.1 - self.pin.1);
        let (x, y) = (dx * c - dy * s, dx * s + dy * c);
        (if self.flip_x { -x } else { x } + self.loc.0, if self.flip_y { -y } else { y } + self.loc.1)
    }

    /// Whether the shape is turned or flipped.
    pub fn is_turned_or_flipped(&self) -> bool {
        self.angle.abs() > 1.0e-9 || self.flip_x || self.flip_y
    }
}

/// A shape's frame in its parent's: the lower-left corner of its box **as it would be unturned and unflipped** (the pin less the local pin), its
/// size, its pin, its turn and its flips. For where a turned or flipped shape's box is, see [`shape_corners`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub pin_x: f64,
    pub pin_y: f64,
    pub angle: f64,
    /// `FlipX` and `FlipY`.
    pub flip_x: bool,
    pub flip_y: bool,
}

/// A connection point of a shape, on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectionPoint {
    /// The row: its `IX` (the first row is 0), the number `Connections.X<index + 1>` names, and `ToPart` is 100 plus it.
    pub index: u32,
    pub x: f64,
    pub y: f64,
    /// The way a connector leaves it, in the page's frame (not unit length: the cells' own numbers, turned with the shape).
    pub dir_x: f64,
    pub dir_y: f64,
}

/// Reads cells of a document through its resolver, and works out the cells that hold a formula and no saved value ([`Worked`]).
pub(crate) struct Reader<'a> {
    doc: &'a Document,
    res: Resolver<'a>,
    worked: Worked<'a>,
}

impl<'a> Reader<'a> {
    pub fn new(doc: &'a Document) -> Reader<'a> {
        Reader { doc, res: Resolver::new(doc), worked: Worked::new(doc) }
    }

    pub fn resolver(&self) -> &Resolver<'a> {
        &self.res
    }

    /// The reader of the cells that hold a formula and no saved value, for a caller that reads cells through [`Reader::resolver`] and wants them as [`Reader::number`] gives them.
    pub fn worked(&self) -> &Worked<'a> {
        &self.worked
    }

    pub fn doc(&self) -> &'a Document {
        self.doc
    }

    /// The number a cell directly under `sheet` holds (0 when it holds none, as a cell found nowhere reads). A cell that holds a formula and no saved value
    /// is the number the formula works out to (0 when it gives none).
    pub fn number(&self, sheet: &SheetRef, name: &str) -> f64 {
        self.addr_number(sheet, &CellAddr::direct(name))
    }

    /// As [`Reader::number`], for any address.
    pub fn addr_number(&self, sheet: &SheetRef, addr: &CellAddr) -> f64 {
        self.res.resolve(sheet, addr).map_or(0.0, |found| self.worked.number_of(sheet, addr, &found).unwrap_or(0.0))
    }

    /// Whether some sheet on the way (the shape, its master) holds the cell: a formula or a value.
    pub fn is_set(&self, sheet: &SheetRef, addr: &CellAddr) -> bool {
        self.res.resolve(sheet, addr).is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some())
    }

    /// Whether the shape is one-dimensional: all four endpoint cells are there after inheritance.
    pub fn is_one_d(&self, sheet: &SheetRef) -> bool {
        ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| self.is_set(sheet, &CellAddr::direct(name)))
    }

    /// Whether the shape's route is one DiaCraft wrote and keeps (a `User` row `DiaCraftRoute`, see `connectors.rs`).
    pub fn is_routed(&self, sheet: &SheetRef) -> bool {
        self.is_set(sheet, &route_marker())
    }

    /// The pose of a shape.
    pub fn pose(&self, sheet: &SheetRef) -> Pose {
        #[cfg(test)]
        POSES_READ.with(|c| c.set(c.get().saturating_add(1)));
        let n = |name: &str| self.number(sheet, name);
        Pose {
            pin: (n("PinX"), n("PinY")),
            loc: (n("LocPinX"), n("LocPinY")),
            angle: n("Angle"),
            flip_x: n("FlipX") != 0.0,
            flip_y: n("FlipY") != 0.0,
        }
    }

    /// The shape and its ancestors' poses, the shape's first.
    fn poses_up(&self, at: &ShapeRef) -> Vec<Pose> {
        (1..=at.path.len())
            .rev()
            .filter_map(|k| {
                let prefix = ShapeRef { owner: at.owner, path: at.path.get(..k)?.to_vec() };
                Some(self.pose(&SheetRef::Shape(prefix)))
            })
            .collect()
    }

    /// A point of the shape's frame, carried up through every group to the page.
    pub fn to_page(&self, at: &ShapeRef, p: P) -> P {
        self.poses_up(at).iter().fold(p, |p, pose| pose.up(p))
    }

    /// A direction of the shape's frame in the page's.
    pub fn vector_to_page(&self, at: &ShapeRef, v: P) -> P {
        self.poses_up(at).iter().fold(v, |v, pose| pose.up_vector(v))
    }

    /// The poses of the groups a shape is in, the outermost first.
    fn groups_down(&self, at: &ShapeRef) -> Vec<Pose> {
        (1..at.path.len())
            .filter_map(|k| {
                let prefix = ShapeRef { owner: at.owner, path: at.path.get(..k)?.to_vec() };
                Some(self.pose(&SheetRef::Shape(prefix)))
            })
            .collect()
    }

    /// A direction of the page, as the shape's parent has it: carried down through the groups the shape is in (none for a top-level shape, which
    /// has the page for its parent). A distance to move a member by, in the units of its own cells.
    pub fn vector_from_page(&self, at: &ShapeRef, v: P) -> P {
        self.groups_down(at).iter().fold(v, |v, pose| pose.down_vector(v))
    }

    /// A point of the page, as the shape's parent has it.
    pub fn point_from_page(&self, at: &ShapeRef, p: P) -> P {
        self.groups_down(at).iter().fold(p, |p, pose| pose.down(p))
    }

    /// A point of the shape's parent's frame (where its pin and the ends of a connector are), carried up through the groups it is in to the page:
    /// the inverse of [`Reader::point_from_page`].
    pub fn parent_to_page(&self, at: &ShapeRef, p: P) -> P {
        self.poses_up(at).iter().skip(1).fold(p, |p, pose| pose.up(p))
    }

    /// The corners of a shape's box on the page, in the order of its own frame: (0, 0), (`Width`, 0), (`Width`, `Height`), (0, `Height`). A turned
    /// or flipped shape's box is the same four corners carried through its pose and its groups'.
    pub fn corners(&self, at: &ShapeRef) -> [P; 4] {
        let sheet = SheetRef::Shape(at.clone());
        let (w, h) = (self.number(&sheet, "Width"), self.number(&sheet, "Height"));
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|p| self.to_page(at, p))
    }

    /// The flips of the groups a shape is in, one bit for each axis: two flips of one axis, at any two levels, cancel. The shape's own are not in
    /// it. The renderer cancels these in a text block as well as the shape's own (`render/src/text.rs`).
    pub fn group_flips(&self, at: &ShapeRef) -> (bool, bool) {
        self.groups_down(at).iter().fold((false, false), |(x, y), pose| (x != pose.flip_x, y != pose.flip_y))
    }

    /// Whether a group the shape is in is turned or flipped: its members' boxes are not square to the page.
    pub fn groups_turn(&self, at: &ShapeRef) -> bool {
        self.groups_down(at).iter().any(Pose::is_turned_or_flipped)
    }

    /// Whether the shape's picture on the page is a mirror image: an odd number of the shapes from it up through its groups are flipped once (a shape
    /// flipped both ways is turned by a half turn, not mirrored).
    pub fn is_mirrored(&self, at: &ShapeRef) -> bool {
        self.poses_up(at).iter().filter(|pose| pose.flip_x != pose.flip_y).count() % 2 == 1
    }

    /// The frame of a top-level or nested shape, unturned: where its box sits in its parent's frame.
    pub fn frame(&self, at: &ShapeRef) -> Frame {
        let sheet = SheetRef::Shape(at.clone());
        let pose = self.pose(&sheet);
        let (width, height) = (self.number(&sheet, "Width"), self.number(&sheet, "Height"));
        Frame {
            x: pose.pin.0 - pose.loc.0,
            y: pose.pin.1 - pose.loc.1,
            width,
            height,
            pin_x: pose.pin.0,
            pin_y: pose.pin.1,
            angle: pose.angle,
            flip_x: pose.flip_x,
            flip_y: pose.flip_y,
        }
    }

    /// The connection points of a shape on the page, in row order.
    pub fn connection_points(&self, at: &ShapeRef) -> Vec<ConnectionPoint> {
        let sheet = SheetRef::Shape(at.clone());
        let section = SectionKey::new("Connection", None);
        let Some(rows) = self.res.effective_rows(&sheet, &section) else {
            return Vec::new();
        };
        self.points_of_rows(at, &section, &rows)
    }

    /// [`Reader::connection_points`] for a shape whose `Connection` section has at most `max_rows` rows, counted over the shape, its master and the styles
    /// before any is merged (a snap reads the points of every shape near a pointer, at every move, and a master of 100,000 rows must not cost 100,000
    /// for each instance). `None` for a section with more, which is then not read at all.
    pub fn connection_points_within(&self, at: &ShapeRef, max_rows: usize) -> Option<Vec<ConnectionPoint>> {
        let sheet = SheetRef::Shape(at.clone());
        let section = SectionKey::new("Connection", None);
        match self.res.effective_rows_within(&sheet, &section, max_rows) {
            None => Some(Vec::new()),
            Some(Ok(rows)) => Some(self.points_of_rows(at, &section, &rows)),
            Some(Err(_)) => None,
        }
    }

    /// How many rows the `Connection` section of a shape holds, counted over the shape, its master and the styles before any is merged: what a caller that
    /// keeps a budget of rows charges before it reads the points ([`Reader::connection_points_within`] with this count merges no more).
    pub fn connection_rows(&self, at: &ShapeRef) -> usize {
        self.res.rows_held(&SheetRef::Shape(at.clone()), &SectionKey::new("Connection", None)).unwrap_or(0)
    }

    fn points_of_rows(&self, at: &ShapeRef, section: &SectionKey, rows: &[diacraft_doc::EffectiveRow<'_>]) -> Vec<ConnectionPoint> {
        let sheet = SheetRef::Shape(at.clone());
        rows.iter()
            .enumerate()
            .map(|(position, row)| {
                let index = match &row.key {
                    RowKey::Ix(ix) => *ix,
                    RowKey::Name(_) => u32::try_from(position).unwrap_or(u32::MAX),
                };
                // A cell that holds a formula and no saved value is what the formula works out to, on the row of the section it is in.
                let cell = |name: &str| {
                    row.cells.iter().find(|(n, _)| n.as_str() == name).map_or(0.0, |(n, found)| {
                        let addr = CellAddr { section: Some(section.clone()), row: Some(row.key.clone()), cell: n.clone() };
                        self.worked.number_of(&sheet, &addr, found).unwrap_or(0.0)
                    })
                };
                let (x, y) = self.to_page(at, (cell("X"), cell("Y")));
                let (dir_x, dir_y) = self.vector_to_page(at, (cell("DirX"), cell("DirY")));
                ConnectionPoint { index, x, y, dir_x, dir_y }
            })
            .collect()
    }

    /// The shape of a page with this `ID`: a search from the top of the page, which a command that has many to find does once for all (`scan::scan`).
    pub fn find(&self, page: u32, id: u32) -> Option<ShapeRef> {
        #[cfg(test)]
        FINDS.with(|n| n.set(n.get().saturating_add(1)));
        self.doc.find_shape(Owner::Page(page), id)
    }
}

/// The connection points of the shapes the ends of one call's connectors are glued to, **each shape read once for the call**, and all of them within a number of rows
/// for the call (as read them for the crossing test): the placing of the ends calc cannot work out, the test of a fixed route and the routing
/// of every connector an edit moved read a shape's points here, not once for each end (ten thousand ends on a shape of ten thousand points were a hundred million rows).
/// A shape whose `Connection` section holds more rows than are left, counted over the shape, its master and the styles before any is merged, has none, and is never
/// read: an end glued to it is placed, tested and routed as an end glued to a shape with no points is.
#[derive(Debug)]
pub(crate) struct GluedPoints {
    /// By shape `ID`, the rows the section holds and the points, once read; `None` for a shape that was past what was left.
    held: std::collections::HashMap<u32, Option<(usize, Vec<ConnectionPoint>)>>,
    rows_left: usize,
}

impl GluedPoints {
    /// Room for `rows` rows of `Connection` sections in all.
    pub(crate) fn new(rows: usize) -> GluedPoints {
        GluedPoints { held: std::collections::HashMap::new(), rows_left: rows }
    }

    /// The rows the section of shape `id` (at `at`) holds and its points, read the first time it is asked for: `None` for a shape whose section holds more rows than
    /// were left then.
    pub(crate) fn of(&mut self, reader: &Reader<'_>, id: u32, at: &ShapeRef) -> Option<(usize, &[ConnectionPoint])> {
        let rows_left = &mut self.rows_left;
        let held = self.held.entry(id).or_insert_with(|| {
            let rows = reader.connection_rows(at);
            if rows > *rows_left {
                return None;
            }
            *rows_left = rows_left.saturating_sub(rows);
            reader.connection_points_within(at, rows).map(|points| (rows, points))
        });
        held.as_ref().map(|(rows, points)| (*rows, points.as_slice()))
    }

    /// The points of shape `id` (at `at`): none for a shape past what was left.
    pub(crate) fn points(&mut self, reader: &Reader<'_>, id: u32, at: &ShapeRef) -> &[ConnectionPoint] {
        self.of(reader, id, at).map_or(&[], |(_, points)| points)
    }

    /// The points of shape `id` as read already: none for a shape not read yet, or past what was left. For a caller that looks at two shapes' points together.
    pub(crate) fn held(&self, id: u32) -> &[ConnectionPoint] {
        self.held.get(&id).and_then(Option::as_ref).map_or(&[], |(_, points)| points.as_slice())
    }
}

/// The marker cell of a shape whose route DiaCraft keeps: `User.DiaCraftRoute`. A user-defined cell, which other readers leave alone.
pub(crate) fn route_marker() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("DiaCraftRoute"), "Value")
}

/// The frame of the shape `id` on a page: `None` for a page or shape that is not there.
pub fn shape_frame(doc: &Document, page: u32, id: u32) -> Option<Frame> {
    let reader = Reader::new(doc);
    let at = reader.find(page, id)?;
    Some(reader.frame(&at))
}

/// The four corners of the box of the shape `id` on a page, **on the page**, in the order of the shape's own frame: the corner at (0, 0), then the
/// one at (`Width`, 0), (`Width`, `Height`) and (0, `Height`). For a shape that is turned or flipped (or in a group that is) this is where its
/// box is, which [`shape_frame`] does not say; `shape.resize` of such a shape takes the first corner. `None` for a page or shape
/// that is not there.
pub fn shape_corners(doc: &Document, page: u32, id: u32) -> Option<[P; 4]> {
    let reader = Reader::new(doc);
    let at = reader.find(page, id)?;
    Some(reader.corners(&at))
}

/// The connection points of the shape `id` on a page, with their places on the page: what a window highlights while a connector is dragged.
/// Empty for a page or shape that is not there, and for a shape with none.
pub fn connection_points(doc: &Document, page: u32, id: u32) -> Vec<ConnectionPoint> {
    let reader = Reader::new(doc);
    reader.find(page, id).map(|at| reader.connection_points(&at)).unwrap_or_default()
}

/// [`connection_points`] for a window that reads them as it draws: `None` for a shape whose `Connection` section holds more than `max_rows`
/// rows, counted over the shape, its master and the styles before any is merged ([`Reader::connection_points_within`], as the snap reads them), and
/// which is then not read at all. Empty for a page or shape that is not there, and for a shape with none.
pub fn connection_points_within(doc: &Document, page: u32, id: u32, max_rows: usize) -> Option<Vec<ConnectionPoint>> {
    let reader = Reader::new(doc);
    match reader.find(page, id) {
        Some(at) => reader.connection_points_within(&at, max_rows),
        None => Some(Vec::new()),
    }
}

/// Drawing units to a paper inch on a page: `DrawingScale` over `PageScale`; 1 when either is missing or not
/// positive.
pub fn units_per_inch(doc: &Document, page: u32) -> f64 {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Page(page);
    let (paper, drawing) = (reader.number(&sheet, "PageScale"), reader.number(&sheet, "DrawingScale"));
    let k = drawing / paper;
    if paper > 0.0 && drawing > 0.0 && k.is_finite() && k > 0.0 { k } else { 1.0 }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn near(a: P, b: P) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    /// A top-level shape with `PinX` 5, `PinY` 3, `LocPinX` 1, `LocPinY` 0.5 and `Angle` 90 degrees.
    fn frame_1(flip_x: bool, flip_y: bool) -> Pose {
        Pose { pin: (5.0, 3.0), loc: (1.0, 0.5), angle: std::f64::consts::FRAC_PI_2, flip_x, flip_y }
    }

    #[test]
    fn a_point_reaches_the_parent_as_the_specifications_vectors_say() {
        // `PAR(PNT(2,1))` is (4.5, 4); with FlipX 1 (4.5, 2); with FlipY 1 (5.5, 4); with both (5.5, 2).
        assert!(near(frame_1(false, false).up((2.0, 1.0)), (4.5, 4.0)));
        assert!(near(frame_1(true, false).up((2.0, 1.0)), (4.5, 2.0)));
        assert!(near(frame_1(false, true).up((2.0, 1.0)), (5.5, 4.0)));
        assert!(near(frame_1(true, true).up((2.0, 1.0)), (5.5, 2.0)));
    }

    #[test]
    fn going_down_undoes_going_up_for_every_pose() {
        for (fx, fy) in [(false, false), (true, false), (false, true), (true, true)] {
            for angle in [0.0, 0.3, std::f64::consts::FRAC_PI_2, 3.0, -2.0] {
                let pose = Pose { pin: (5.0, 3.0), loc: (1.0, 0.5), angle, flip_x: fx, flip_y: fy };
                for p in [(0.0, 0.0), (2.0, 1.0), (-3.5, 7.25)] {
                    assert!(near(pose.down(pose.up(p)), p), "{pose:?} {p:?}");
                    assert!(near(pose.down_vector(pose.up_vector(p)), p), "a direction goes down as it went up: {pose:?} {p:?}");
                }
            }
        }
        // A direction is turned and flipped but not moved.
        assert!(near(frame_1(false, false).up_vector((1.0, 0.0)), (0.0, 1.0)));
    }

    #[test]
    fn a_pose_with_a_turn_or_a_flip_says_so() {
        assert!(!Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: 0.0, flip_x: false, flip_y: false }.is_turned_or_flipped());
        assert!(frame_1(false, false).is_turned_or_flipped());
        assert!(Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: 0.0, flip_x: true, flip_y: false }.is_turned_or_flipped());
    }
}
