//! The outline of a shape on the page, for the snaps that reach it: the paths its `Geometry` sections draw, in paper inches on the page.
//!
//! The rows are the shape's effective ones (inherited from its master and the styles), turned into paths by `diacraft_geom::build_paths`
//! exactly as the renderer draws them (`diacraft_render::geom_rows` reads a row's cells), so a snap lands on the curve that is drawn: an ellipse is an ellipse
//! and not a polygon. The paths are carried to the page through the shape's pose and its groups' and divided into paper inches by one affine map, and the
//! points they give (`osnap`: endpoints, nearest, intersections) are kurbo's.
//!
//! **A section whose `NoSnap` is 1 is left out** (1 stops other shapes snapping to the path). **The reading is bounded in work, not
//! only in what it keeps**: at most [`MAX_SECTIONS`] sections, and a section of more than [`MAX_ROWS`] rows, counted over the shape, its master and the styles
//! before any is merged, is not read at all (`Resolver::effective_rows_within`): an instance of a master of 100,000 rows costs a count and no merge, however many
//! instances there are.
//!
//! **What the rows hold is bounded as well as how many there are.** A `PolylineTo` or a `NURBSTo` row carries a constructor in a cell, which may hold tens of
//! thousands of points (`diacraft_geom::MAX_CONSTRUCTOR_POINTS` is 50,000), and a section of 400 rows of them is read, parsed and built into a curve for each
//! of the 16 shapes a query looks at. So a query holds **one [`Budget`] across all its shapes**, charged before each section is built: the bytes of the
//! constructor cells (their length, which costs nothing to read), the points they hold (counted in the text, which the byte budget bounds), the elements the
//! built path may have and the spline work (`diacraft_geom::Bound`). A section the budget cannot pay for is left out, as one of too many rows is, and the query
//! says so (`too_big`, which makes the answer `truncated`).

use diacraft_doc::{CellAddr, EffectiveRow, ShapeRef, SheetRef};
use diacraft_geom::{Bound, build_paths_bounded, rules};
use kurbo::{Affine, BezPath, Size};

use crate::draw::Reader;

/// The most `Geometry` sections of a shape that are read.
pub(crate) const MAX_SECTIONS: usize = 8;

/// The most rows a section may have (over the shape, its master and the styles) to be read.
pub(crate) const MAX_ROWS: usize = 400;

/// The most `Geometry` sections a shape may hold, counted over the shape, its master and the styles before any is merged, for them to be listed at all: a
/// master that holds thousands of sections (an instance of it inherits every one) is not listed, and the shape is a target by its box only.
pub(crate) const MAX_RAW_SECTIONS: usize = 64;

/// The most bytes of constructor text (the cells `A` of `PolylineTo` rows and `E` of `NURBSTo` rows) that a query reads, over all its shapes. The sample files'
/// largest constructors are 490 pairs and 773 tuples (`diacraft_geom::MAX_CONSTRUCTOR_POINTS`), some 30 kilobytes.
pub const MAX_CONSTRUCTOR_BYTES: usize = 128 * 1024;

/// The most points (the rows of a section, and the points and tuples of its constructors) that a query builds into paths, over all its shapes.
pub const MAX_BUILD_POINTS: usize = 4_096;

/// The most path elements that a query's builds may make, over all its shapes.
pub const MAX_BUILD_ELEMENTS: usize = 16_384;

/// The most spline work, in multiplications (`diacraft_geom::Bound::curve_work`), that a query's builds may spend, over all its shapes: some milliseconds.
pub const MAX_BUILD_WORK: u64 = 5_000_000;

/// What a page may spend building the boxes round the paths of its turned shapes, over all of them: **32 times a query's budget** (the design's own
/// proposal, taken when the index came to read 10,000 shapes, and the commands came to find the page the reader made), which is 4 MiB of constructor text,
/// 131,072 points, 524,288 path elements and 160 million spline multiplications. **Measured** (`a_page_of_the_most_shapes_the_index_reads_all_turned_stock_shapes_is_paid_for_in_full`):
/// a page of 10,000 stock shapes, every one turned, spends about 86,100 points and 121,000 elements and no text or spline work, so this is 1.5 times the points and over 4
/// times the elements such a page needs; 16 times a query's paid for 7,617 of them and left 2,383 to the box round their corners (`truncated`). A crafted page spends the whole of
/// it (points, or spline work) in some 150 to 320 milliseconds on the reader's thread, beyond the frames' 60 to 90 (the review's measurements: 148 to 322 against 134 to 215 at 16 times).
pub const PAGE_BUILD_BYTES: usize = 32 * MAX_CONSTRUCTOR_BYTES;
pub const PAGE_BUILD_POINTS: usize = 32 * MAX_BUILD_POINTS;
pub const PAGE_BUILD_ELEMENTS: usize = 32 * MAX_BUILD_ELEMENTS;
pub const PAGE_BUILD_WORK: u64 = 32 * MAX_BUILD_WORK;

/// The most shapes (a group and its members, counted over the tree) whose paths one box is made of: a group of more keeps the box round its corners.
pub const MAX_BOX_MEMBERS: usize = 256;

/// What a query may still spend on building the outlines of the shapes it looks at, and what it has spent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    bytes: usize,
    points: usize,
    elements: usize,
    work: u64,
    spent: OutlineWork,
}

impl Budget {
    /// The whole budget of a query.
    pub(crate) fn new() -> Budget {
        Budget {
            bytes: MAX_CONSTRUCTOR_BYTES,
            points: MAX_BUILD_POINTS,
            elements: MAX_BUILD_ELEMENTS,
            work: MAX_BUILD_WORK,
            spent: OutlineWork::default(),
        }
    }

    /// The whole budget of a page, for the boxes round the paths of its turned shapes.
    pub(crate) fn page() -> Budget {
        Budget {
            bytes: PAGE_BUILD_BYTES,
            points: PAGE_BUILD_POINTS,
            elements: PAGE_BUILD_ELEMENTS,
            work: PAGE_BUILD_WORK,
            spent: OutlineWork::default(),
        }
    }

    /// What has been spent.
    pub(crate) fn spent(&self) -> OutlineWork {
        self.spent
    }
}

/// What a query spent on the outlines of the shapes it looked at: the counts that its budget caps, for the tests and for a window that wants to say why an
/// outline is missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutlineWork {
    /// Bytes of constructor text read.
    pub bytes: usize,
    /// Rows merged: the rows of the sections read, counted over the shape, its master and the styles, and charged before any was merged.
    pub rows: usize,
    /// Points charged: those rows, and the points the constructors of the sections built hold.
    pub points: usize,
    /// Path elements the builds made.
    pub elements: usize,
    /// Spline work spent.
    pub curve_work: u64,
    /// Sections built.
    pub built: usize,
    /// Sections left out because the budget (or the rows) could not pay for them.
    pub left_out: usize,
}

/// What a shape's geometry gives a snap.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outline {
    /// The path of each section read, in paper inches on the page.
    pub paths: Vec<BezPath>,
    /// How many sections were left out because they had more than [`MAX_ROWS`] rows.
    pub too_big: usize,
}

/// The map from a shape's own frame (drawing units) to the page in paper inches: its pose and its groups' (three points carried through
/// `Reader::to_page` fix an affine map), and the scale of the drawing.
fn to_paper(reader: &Reader<'_>, at: &ShapeRef, k: f64) -> Affine {
    let o = reader.to_page(at, (0.0, 0.0));
    let ex = reader.to_page(at, (1.0, 0.0));
    let ey = reader.to_page(at, (0.0, 1.0));
    Affine::scale(1.0 / k) * Affine::new([ex.0 - o.0, ex.1 - o.1, ey.0 - o.0, ey.1 - o.1, o.0, o.1])
}

/// The text of a row's constructor cell, if the row has one: `A` of a `PolylineTo`, `E` of a `NURBSTo` (the cells `diacraft_render::geom_rows` reads).
fn constructor_text<'r>(row: &'r EffectiveRow<'_>) -> Option<&'r str> {
    let name = match row.row_type.as_deref()? {
        "PolylineTo" => "A",
        "NURBSTo" => "E",
        _ => return None,
    };
    row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell).and_then(|c| c.value.as_ref()).map(|v| v.text())
}

/// What a section's constructors will cost to build: the bytes of their text, which are known from the lengths alone, and, only when those fit in
/// `bytes_left`, the points they hold (a number for each comma, over two for a pair and four for a tuple, counted conservatively). The rows were charged
/// before they were merged.
fn cost_of(rows: &[EffectiveRow<'_>], bytes_left: usize) -> Option<(usize, usize)> {
    let mut bytes = 0usize;
    for row in rows {
        bytes = bytes.saturating_add(constructor_text(row).map_or(0, str::len));
        if bytes > bytes_left {
            return None;
        }
    }
    let mut points = 0usize;
    for row in rows {
        let Some(text) = constructor_text(row) else { continue };
        let numbers = text.bytes().filter(|b| *b == b',').count().saturating_add(1);
        // A pair for a polyline, a tuple of four for a NURBS: the fewest numbers to a point, so the most points.
        points = points.saturating_add(numbers / 2);
    }
    Some((bytes, points))
}

/// What the paths are read for: the points other shapes snap to (a section whose `NoSnap` is 1 is left out), or the box round what the shape draws (a section whose
/// `NoShow` is 1 is left out, and `NoSnap` does not matter: it stops snapping to the path's own points, not the shape's alignment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    Snap,
    Box,
}

/// The outline of the shape `at`; `k` is the drawing units to a paper inch. `budget` is the query's, charged before each section is built.
pub(crate) fn outline(reader: &Reader<'_>, at: &ShapeRef, k: f64, budget: &mut Budget) -> Outline {
    outline_for(reader, at, k, budget, Purpose::Snap)
}

/// [`outline`] for a purpose.
pub(crate) fn outline_for(reader: &Reader<'_>, at: &ShapeRef, k: f64, budget: &mut Budget, purpose: Purpose) -> Outline {
    let mut out = Outline::default();
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    // The sections are counted over the layers before any is merged, and a shape that holds thousands (an instance of such a master) is not listed.
    let keys = match res.effective_sections_within(&sheet, "Geometry", MAX_RAW_SECTIONS) {
        Some(Ok(keys)) => keys,
        Some(Err(_)) => {
            out.too_big += 1;
            budget.spent.left_out += 1;
            return out;
        }
        None => return out,
    };
    let size = Size::new(reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let map = to_paper(reader, at, k);
    if !map.as_coeffs().iter().all(|c| c.is_finite()) {
        return out;
    }
    for key in keys.iter().take(MAX_SECTIONS) {
        let skip = if purpose == Purpose::Snap { "NoSnap" } else { "NoShow" };
        if reader.addr_number(&sheet, &CellAddr::in_section("Geometry", key.ix, skip)) != 0.0 {
            continue;
        }
        // The rows are counted over the layers and **charged to the query before any is merged**: a section over the cap of rows, or over what the query has
        // left, is not merged, so the rows a query merges are never more than its budget, however many shapes and sections it looks at.
        let Some(raw) = res.rows_held(&sheet, key) else { continue };
        if raw == 0 {
            continue;
        }
        if raw > MAX_ROWS || raw > budget.points || budget.elements == 0 || budget.work == 0 {
            out.too_big += 1;
            budget.spent.left_out += 1;
            continue;
        }
        budget.points -= raw;
        budget.spent.rows += raw;
        budget.spent.points += raw;
        let Some(Ok(rows)) = res.effective_rows_within(&sheet, key, raw) else { continue };
        // The constructors are charged before they are parsed or anything is built.
        let Some((bytes, points)) = cost_of(&rows, budget.bytes) else {
            out.too_big += 1;
            budget.spent.left_out += 1;
            continue;
        };
        if points > budget.points {
            out.too_big += 1;
            budget.spent.left_out += 1;
            continue;
        }
        budget.bytes -= bytes;
        budget.points -= points;
        budget.spent.bytes += bytes;
        budget.spent.points += points;
        let (geom, _) = diacraft_render::geom_rows(&rows);
        let bound = Bound { curve_work: budget.work, path_elements: budget.elements };
        let Ok(built) = build_paths_bounded(&geom, size, rules::FILL_RUN_JOIN, bound) else {
            // A section whose path would pass what is left of the budget is left out, and what is left is gone: the next section is not built either.
            out.too_big += 1;
            budget.spent.left_out += 1;
            budget.elements = 0;
            continue;
        };
        budget.spent.built += 1;
        budget.work = budget.work.saturating_sub(built.curve_work);
        budget.spent.curve_work += built.curve_work;
        // The spline work ran out inside the section: its splines are skipped with a warning, and the outline is not all of the shape's.
        if built.curve_work >= bound.curve_work {
            out.too_big += 1;
        }
        let mut path = built.stroke;
        let elements = path.elements().len();
        budget.elements = budget.elements.saturating_sub(elements);
        budget.spent.elements += elements;
        if path.elements().is_empty() {
            continue;
        }
        path.apply_affine(map);
        out.paths.push(path);
    }
    out
}

/// What [`path_box`] made of a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PathBox {
    /// The level and upright box round every path the shape draws.
    Box(kurbo::Rect),
    /// The shape draws no path (a picture or other embedded object, or only hidden sections): it keeps the box round its corners, which costs nothing.
    Nothing,
    /// A path was left out for what it costs (the budget, a section of too many rows, or a group of more than [`MAX_BOX_MEMBERS`] shapes): the shape keeps the box round
    /// its corners, and a result that lines something up by it says so.
    Cut,
}

/// The level and upright box round every path the shape `at` draws, in paper inches on the page, line weight ignored; a group's is round its members' paths. **The
/// budget is charged before each section is built**; [`PathBox::Cut`] when it could not pay for one (the shape keeps the box round its corners, and the page counts it) or
/// when the tree holds more than [`MAX_BOX_MEMBERS`] shapes, and [`PathBox::Nothing`] when nothing is drawn. A picture or other embedded object keeps the box round its corners.
pub(crate) fn path_box(reader: &Reader<'_>, doc: &diacraft_doc::Document, at: &ShapeRef, k: f64, budget: &mut Budget) -> PathBox {
    use diacraft_doc::ShapeType;
    use kurbo::Shape as _;
    let mut stack = vec![at.clone()];
    let mut seen = 0usize;
    let mut rect: Option<kurbo::Rect> = None;
    let mut grow = |r: kurbo::Rect| rect = Some(rect.map_or(r, |u| u.union(r)));
    while let Some(cur) = stack.pop() {
        seen += 1;
        if seen > MAX_BOX_MEMBERS {
            return PathBox::Cut;
        }
        let Some(shape) = doc.shape(&cur) else { continue };
        if shape.del || shape.kind == ShapeType::Guide {
            continue;
        }
        if cur.path.len() < diacraft_doc::MAX_SHAPE_DEPTH {
            for i in 0..shape.children.len() {
                let Ok(i) = u32::try_from(i) else { break };
                let mut path = cur.path.clone();
                path.push(i);
                stack.push(ShapeRef { owner: cur.owner, path });
            }
        }
        if shape.kind == ShapeType::Foreign {
            let c = reader.corners(&cur).map(|p| kurbo::Point::new(p.0 / k, p.1 / k));
            if c.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                grow(c.iter().fold(kurbo::Rect::from_points(c[0], c[0]), |u, p| u.union_pt(*p)));
            }
            continue;
        }
        let out = outline_for(reader, &cur, k, budget, Purpose::Box);
        if out.too_big > 0 {
            return PathBox::Cut;
        }
        for p in &out.paths {
            let b = p.bounding_box();
            if b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite() {
                grow(b);
            }
        }
    }
    rect.map_or(PathBox::Nothing, PathBox::Box)
}
