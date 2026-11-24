//! Every cap, each a named constant, and the budget a picture spends them from. A cap that is reached ends the work it limits
//! and is reported as [`Detail::CapReached`](crate::Detail::CapReached); none of them panics. The budget is charged before anything is
//! allocated and shared by a metafile and the metafiles inside it, so nesting cannot multiply it.

/// Records walked in one EMF (the largest in files holds 205,183).
pub const MAX_RECORDS: u64 = 4_000_000;
/// Painted items (fills, strokes, bitmaps, text runs) in one picture (the largest in files, counting each plain `LINETO`, holds
/// 139,143).
pub const MAX_ITEMS: u64 = 1_000_000;
/// Slots of an EMF or WMF object table by format, allocated on use.
pub const MAX_OBJECT_SLOTS: usize = 65_535;
/// Saved device contexts (the deepest in files is 10).
pub const MAX_SAVE_DEPTH: usize = 1_024;
/// EMF+ objects are numbered 0 to 63.
pub const MAX_PLUS_OBJECTS: usize = 64;
/// The bytes of one EMF+ object split over records.
pub const MAX_PLUS_OBJECT_BYTES: usize = 64 * 1024 * 1024;
/// Pixels of one bitmap, and its side.
pub const MAX_BITMAP_PIXELS: u64 = 40_000_000;
pub const MAX_BITMAP_SIDE: u32 = 16_384;
/// Nested metafiles inside metafiles (the deepest in files is 2).
pub const MAX_NESTING: u32 = 4;
/// Nodes deep in an EMF+ region.
pub const MAX_REGION_DEPTH: usize = 64;

/// Path points (a line, a curve's three points) all the items of one picture hold. Project rule, after the spec's "charge work in points": a
/// picture that took the item cap with the points of the largest record in files (11,643) would hold 11 billion; this holds 8 million
/// (about 220 MB at the size of a segment) and the 99th percentile picture uses a few tens of thousands. The points are charged before the
/// path that holds them is built, from the count a record's bytes allow (`Budget::has_points`), and again when an item is kept.
pub const MAX_POINTS: u64 = 8_000_000;
/// Parts of one clip (an intersection of paths), and the points all of them hold. In files a clip is one rectangle; a hostile file
/// that intersects a path clip after a path clip would otherwise grow the clip by a part for each record and copy all of it each time.
/// A combination past either is refused and reported as a cap.
pub const MAX_CLIP_PARTS: usize = 64;
pub const MAX_CLIP_POINTS: u64 = 250_000;
/// Pixels all the bitmaps of one picture decode to (project rule: R40 caps one bitmap at 40 million, which in 4 bytes is 160 MB).
pub const MAX_TOTAL_PIXELS: u64 = 64_000_000;
/// Clips a picture keeps (each is a list of parts that are paths; the points they hold are charged with the rest).
pub const MAX_CLIPS: usize = 100_000;
/// Units of work: a unit is about four bytes read, or a point made, or a test of a path's edge ("charge work in bytes, points and
/// pixels, not only records"). A picture of the biggest size in files (26 MB) uses 6.5 million; the cap is where a hostile picture that
/// still holds a few hundred thousand tiny records stops, a few seconds of work at most.
pub const MAX_WORK: u64 = 40_000_000;
/// Characters in one text run (a record's bytes bound it; this bounds a table that is built from it).
pub const MAX_TEXT_CHARS: usize = 65_536;

/// What a picture may still spend.
#[derive(Debug)]
pub(crate) struct Budget {
    records: u64,
    items: u64,
    points: u64,
    pixels: u64,
    work: u64,
    /// Set when any cap ended work.
    pub reached: Option<&'static str>,
}

impl Budget {
    pub fn new() -> Budget {
        Budget { records: MAX_RECORDS, items: MAX_ITEMS, points: MAX_POINTS, pixels: MAX_TOTAL_PIXELS, work: MAX_WORK, reached: None }
    }

    fn take(left: &mut u64, n: u64, reached: &mut Option<&'static str>, what: &'static str) -> bool {
        if n > *left {
            *left = 0;
            reached.get_or_insert(what);
            false
        } else {
            *left -= n;
            true
        }
    }

    pub fn record(&mut self) -> bool {
        Self::take(&mut self.records, 1, &mut self.reached, "records")
    }

    pub fn item(&mut self) -> bool {
        Self::take(&mut self.items, 1, &mut self.reached, "painted items")
    }

    pub fn points(&mut self, n: u64) -> bool {
        Self::take(&mut self.points, n, &mut self.reached, "path points")
    }

    pub fn pixels(&mut self, n: u64) -> bool {
        Self::take(&mut self.pixels, n, &mut self.reached, "bitmap pixels")
    }

    /// Whether `n` more points are within what is left, without spending them: asked before a path of `n` points is built, so that the
    /// path of a record that could not be kept is not built at all. A refusal is a cap reached.
    pub fn has_points(&mut self, n: u64) -> bool {
        if n > self.points {
            self.reached.get_or_insert("path points");
            false
        } else {
            true
        }
    }

    /// The points still to be spent.
    pub fn points_left(&self) -> u64 {
        self.points
    }

    pub fn work(&mut self, n: u64) -> bool {
        Self::take(&mut self.work, n, &mut self.reached, "work")
    }

    /// Whether anything is left to spend on records, items and work: the loop of a walk stops when it is not, and the cap it stopped at is
    /// noted (a walk that ends because its budget is spent has not read the file to its end).
    pub fn alive(&mut self) -> bool {
        let what = if self.records == 0 {
            "records"
        } else if self.items == 0 {
            "painted items"
        } else if self.work == 0 {
            "work"
        } else {
            return true;
        };
        self.reached.get_or_insert(what);
        false
    }

    pub fn note_cap(&mut self, what: &'static str) {
        self.reached.get_or_insert(what);
    }
}
