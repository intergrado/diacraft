//! What a snap reads of a page, once: the page's size and margins, its `Ruler & Grid` cells, its guides, and, when a snap first reaches
//! other shapes, the index of its shapes.
//!
//! A [`SnapPage`] is made for a document as it is and holds nothing that a setting or the zoom changes, so a window makes one when a drag begins
//! and asks it at every move (and again after an edit). **A command finds the read the window made** (`reads.rs`: [`crate::Session::kept_snap_page`], kept by the document's
//! revision, so an edit drops it) and, with none, makes its own for the document it works on and drops it with the command.

use std::sync::OnceLock;

use diacraft_doc::{CellAddr, Document, Owner, ShapeRef, ShapeType, SheetRef};

use super::adapter::TargetsCache;
use super::grid::{Axis, GridCells};
use super::index::{MAX_INDEXED, P, ShapeIndex};
use crate::draw::{Reader, units_per_inch};

/// The most guides a page gives to a snap.
pub const MAX_GUIDES: usize = 1_000;

/// The most `Geometry` sections, and the most rows in one (counted before they are merged: `Resolver::effective_rows_within`), that a guide is read from.
const GUIDE_SECTIONS: usize = 4;

/// The most `Geometry` sections a guide may hold, counted over the shape, its master and the styles before any is merged, to be read for an `InfiniteLine`
/// row: a guide that holds more is the line through its pin along its angle, as one with no such row is.
const GUIDE_RAW_SECTIONS: usize = 16;
const GUIDE_ROWS: usize = 8;

/// The root's value of the page margins, in paper inches (0.25 in every file).
const ROOT_MARGIN: f64 = 0.25;

/// A guide: a straight line the page's shapes are laid out along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideLine {
    /// The guide shape's `ID`.
    pub id: u32,
    /// `X` for a vertical guide, which is a place on the x axis, and `Y` for a level one.
    pub axis: Axis,
    /// Where it falls, in paper inches.
    pub at: f64,
}

/// A guide that lies along no axis: the line through `at` that runs at `angle`. A turned shape one of whose own sides is parallel to it lines up that side
/// with it; no other subject is told of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlantedGuide {
    /// The guide shape's `ID`.
    pub id: u32,
    /// A point of the line, in paper inches.
    pub at: P,
    /// The direction the line runs in, in radians (`atan2` of the guide's own x axis carried to the page).
    pub angle: f64,
}

/// The page margins, in paper inches (`PageLeftMargin`, `PageRightMargin`, `PageBottomMargin`, `PageTopMargin`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

/// One page, read for snapping.
#[derive(Debug)]
pub struct SnapPage {
    /// The page's `ID`.
    pub page: u32,
    /// The drawing inches to a paper inch.
    pub k: f64,
    /// The page's size on paper, in inches.
    pub size: (f64, f64),
    pub margins: Margins,
    /// `InhibitSnap`: 1 turns off snapping to the page's objects; the ruler and the grid stay.
    pub inhibit: bool,
    pub grid: GridCells,
    /// The page's guides that lie along an axis: the ones every snap uses (every line in files lies along one).
    pub guides: Vec<GuideLine>,
    /// The page's guides that lie along no axis: targets of a turned shape whose own side is parallel to one, at most [`MAX_GUIDES`] with `guides`.
    pub slanted: Vec<SlantedGuide>,
    /// How many guides were not along an axis, or past [`MAX_GUIDES`], and so are not in `guides` (a slanted one is in `slanted` too, as far as the cap allows).
    pub guides_left_out: usize,
    /// How many top-level shapes the read looked at to find the guides.
    pub scanned: usize,
    /// The most top-level shapes whose frames the index reads: [`MAX_INDEXED`], or what a measurement asks of [`SnapPage::read_with`].
    limit: usize,
    shapes: OnceLock<ShapeIndex>,
    /// The targets the library was last given for this page (`adapter.rs`): built for a gesture, asked at every move.
    pub(super) targets: TargetsCache,
}

impl SnapPage {
    /// Reads page `page` of `doc`; `None` for a page that is not there.
    pub fn read(doc: &Document, page: u32) -> Option<SnapPage> {
        SnapPage::read_with(doc, page, MAX_INDEXED)
    }

    /// [`SnapPage::read`] with the index reading the frames of at most `limit` shapes: the one number the cap is, which a measurement and its counted test vary.
    pub(super) fn read_with(doc: &Document, page: u32, limit: usize) -> Option<SnapPage> {
        let page_ref = doc.page(page)?;
        super::reads::count(|c| c.page_reads += 1);
        let reader = Reader::new(doc);
        let k = units_per_inch(doc, page);
        let sheet = SheetRef::Page(page);
        let size = {
            let (w, h) = (reader.number(&sheet, "PageWidth") / k, reader.number(&sheet, "PageHeight") / k);
            if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 { (w, h) } else { (0.0, 0.0) }
        };
        let margin = |name: &str| {
            if reader.is_set(&sheet, &CellAddr::direct(name)) {
                let n = reader.number(&sheet, name);
                if n.is_finite() { n } else { ROOT_MARGIN }
            } else {
                ROOT_MARGIN
            }
        };
        let margins = Margins {
            left: margin("PageLeftMargin"),
            right: margin("PageRightMargin"),
            bottom: margin("PageBottomMargin"),
            top: margin("PageTopMargin"),
        };
        let inhibit = reader.number(&sheet, "InhibitSnap") != 0.0;
        let grid = GridCells::read(&reader, page);
        let mut guides = Vec::new();
        let mut slanted = Vec::new();
        let mut left_out = 0usize;
        let mut scanned = 0usize;
        for (i, shape) in page_ref.shapes.iter().enumerate() {
            scanned += 1;
            if shape.kind != ShapeType::Guide || shape.del {
                continue;
            }
            // The cap first: a guide past it is counted and not read, so that a page of thousands of guides that inherit a big master costs the cap.
            if guides.len() + slanted.len() >= MAX_GUIDES {
                left_out += 1;
                continue;
            }
            let Ok(position) = u32::try_from(i) else { break };
            let at = ShapeRef { owner: Owner::Page(page), path: vec![position] };
            match guide_line(&reader, &at, k) {
                Some(Line::Axis(axis, place)) => guides.push(GuideLine { id: shape.id, axis, at: place }),
                Some(Line::Slanted(at, angle)) => {
                    slanted.push(SlantedGuide { id: shape.id, at, angle });
                    left_out += 1;
                }
                None => left_out += 1,
            }
        }
        Some(SnapPage {
            page,
            k,
            size,
            margins,
            inhibit,
            grid,
            guides,
            slanted,
            guides_left_out: left_out,
            scanned,
            limit,
            shapes: OnceLock::new(),
            targets: TargetsCache::default(),
        })
    }

    /// The index of the page's shapes: made the first time it is asked for (`ShapeIndex::build`), kept for as long as this page is. **Ask it with the document the page was read
    /// for, never with a draft of it that may be thrown away** (a read the session keeps outlives a command that fails): a command that snaps inside its edit makes the page ready
    /// before it (`snap_params::read_before_edit`).
    pub fn shapes(&self, doc: &Document) -> &ShapeIndex {
        self.shapes.get_or_init(|| {
            super::reads::count(|c| c.index_builds += 1);
            ShapeIndex::build(doc, self.page, self.k, self.limit)
        })
    }

    /// What this read holds in memory, in bytes, **estimated**: the guides, the index of the shapes if it has been made and the target set if one has been built. Counted from
    /// the number of entries and generous (the allocator's figure is not asked); it is what the cache of reads charges (`reads.rs`).
    pub fn held_bytes(&self) -> usize {
        let own = std::mem::size_of::<SnapPage>()
            + self.guides.len().saturating_mul(std::mem::size_of::<GuideLine>())
            + self.slanted.len().saturating_mul(std::mem::size_of::<SlantedGuide>());
        own.saturating_add(self.shapes.get().map_or(0, ShapeIndex::held_bytes)).saturating_add(self.targets.held_bytes())
    }

    /// How many target sets were built for this page (`adapter.rs`): one for a drag, whatever its moves, as long as they are asked of the same shapes that move and the same
    /// kinds of target. A test of the window's reader of pages holds a query after it to none more.
    pub fn target_builds(&self) -> usize {
        self.targets.builds()
    }

    /// What the last target set built (`adapter.rs`).
    #[cfg(test)]
    pub(super) fn target_report(&self) -> Option<craft_snap::BuildReport> {
        self.targets.report()
    }

    /// The indexed shapes, for the harness's own reading of a page.
    #[cfg(test)]
    pub(super) fn items_for_test(&self) -> &[super::index::Item] {
        self.shapes.get().map_or(&[], |s| s.items())
    }

    /// Whether the shapes have been read into an index yet.
    pub fn shapes_read(&self) -> bool {
        self.shapes.get().is_some()
    }
}

/// The line a guide draws: along an axis (its place in paper inches), or along none (a point of it and the direction it runs in).
enum Line {
    Axis(Axis, f64),
    Slanted(P, f64),
}

/// The line a guide shape draws, the unbounded line through the `InfiniteLine` row's (`X`, `Y`) and (`A`, `B`) in the shape's own frame, carried to the page; a guide
/// with no such row is the line through its pin along its angle. A line along an axis is told by its place, in paper inches, and any other by a point of it and its direction.
fn guide_line(reader: &Reader<'_>, at: &ShapeRef, k: f64) -> Option<Line> {
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    // A point of the line in the shape's frame and the way it runs there.
    let mut local: Option<(P, P)> = None;
    if let Some(Ok(keys)) = res.effective_sections_within(&sheet, "Geometry", GUIDE_RAW_SECTIONS) {
        for key in keys.iter().take(GUIDE_SECTIONS) {
            // A section of more than a handful of rows (over the shape, its master and the styles) is no guide's: it is skipped before any row is merged.
            let Some(Ok(rows)) = res.effective_rows_within(&sheet, key, GUIDE_ROWS) else { continue };
            for row in rows.iter() {
                if row.row_type.as_deref() != Some("InfiniteLine") {
                    continue;
                }
                // A cell that holds a formula and no saved value is what the formula works out to.
                let cell = |name: &str| {
                    row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(n, f)| {
                        let addr = diacraft_doc::CellAddr { section: Some(key.clone()), row: Some(row.key.clone()), cell: n.clone() };
                        reader.worked().number_of(&sheet, &addr, f)
                    })
                };
                if let (Some(x), Some(y), Some(a), Some(b)) = (cell("X"), cell("Y"), cell("A"), cell("B")) {
                    local = Some(((x, y), (a - x, b - y)));
                }
            }
        }
    }
    let (start, way) = local.unwrap_or(((reader.number(&sheet, "LocPinX"), reader.number(&sheet, "LocPinY")), (1.0, 0.0)));
    let p = reader.to_page(at, start);
    // The direction is the shape's turn and flips applied to the way the line runs in its frame, and not the difference of two points of the page: far from the origin two points
    // are only known to a few steps of their size, and a slanted guide has to agree with the turn of a shape to a billionth of a radian.
    let (dx, dy) = reader.pose(&sheet).up_vector(way);
    let length = dx.hypot(dy);
    if !(length.is_finite() && length > 0.0 && p.0.is_finite() && p.1.is_finite()) {
        return None;
    }
    // Along an axis to within a thousandth of a radian (a guide dragged from a ruler is exactly along one).
    const TILT: f64 = 1.0e-3;
    if dy.abs() / length < TILT {
        Some(Line::Axis(Axis::Y, p.1 / k)).filter(|l| matches!(l, Line::Axis(_, v) if v.is_finite()))
    } else if dx.abs() / length < TILT {
        Some(Line::Axis(Axis::X, p.0 / k)).filter(|l| matches!(l, Line::Axis(_, v) if v.is_finite()))
    } else {
        let (x, y, angle) = (p.0 / k, p.1 / k, dy.atan2(dx));
        (x.is_finite() && y.is_finite() && angle.is_finite()).then_some(Line::Slanted((x, y), angle))
    }
}
