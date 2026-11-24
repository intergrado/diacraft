//! Selecting shapes by what they are and by where they lie: the engine's side of Home, Editing, Select (Select by Type, Area Select and Lasso Select).
//!
//! **Selection itself is the window's** (`docs/editing-commands.md`: "selection stays in the window"). What the engine answers is **which shapes a selection would hold**: the
//! `ID`s of the page's top-level shapes, in the page's order (back to front), that a type, a role and a layer name ([`by_type`]) or a region of the page ([`by_region`]) take. Nothing
//! here changes the document.
//!
//! **By type** ([`TypeQuery`]): a shape's *type* is what the file says it is (`Type` and, for a picture or an object, the `ForeignType` of its `ForeignData`: Shapes, Groups, Guides, OLE
//! objects, Metafiles, Bitmaps and Ink objects, the dialog's list), its *role* is what it does (a connector, a container), and its *layers* are the ones its `LayerMember` names. The
//! entries of one list are alternatives and the lists are put together by *and*, an empty list asking nothing. **A shape on a locked layer is never taken** (
//! Lock "stops shapes on the layer from being selected"), and the answer counts how many it left out for that.
//!
//! **By region** ([`by_region`]): a polygon of the page in paper inches (a net is its four corners, a lasso the points the pointer went through; the last point joins the first). By
//! default a shape is taken only when it lies **wholly** inside it; with `partial` (the Advanced option "Select shapes partially within area") a shape the polygon touches, crosses or
//! lies inside of is taken too. A shape's place is **its box** carried through its turn (the four corners) together with **the paths it draws** (a section with `NoShow` 1 is not drawn), as
//! the pointer's own hit test reads them; a one-dimensional shape (a connector) is its path alone, and a straight line from its begin to its end when it draws none, for the box of one
//! is that line and a bend of its route is outside it. A box that does not touch the polygon's own box is out of reach, and a shape is first tried by its box before its paths are read.
//!
//! **The work is bounded before it is done.** A page is read to [`MAX_SELECT_TYPE_SHAPES`] shapes (by type) or [`MAX_SELECT_REGION_SHAPES`] (by region), and the answer says how many it left
//! unread. A polygon has at most [`MAX_SELECT_POINTS`] points. The paths of the shapes come from the snap's reader of outlines under **one page budget** (`snap::Budget::page`): a shape whose paths
//! it cannot pay for is taken by its box and the answer counts it (`coarse`). A shape's paths are cut into at most [`MAX_SHAPE_POINTS`] straight pieces, and **every test of a point or a
//! segment against the polygon's edges is counted** and charged before it is made: a polygon and a page that need more than [`MAX_SELECT_WORK`] of them are refused in words.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, ShapeType, SheetRef};
use kurbo::{BezPath, PathEl, Shape as _};

use super::layers::{Layer, MemberBudget, read_layers, shape_layers_charged, standing_of};
use super::{Problem, Reader, require_page, units_per_inch};
use crate::snap::{Budget, P, Purpose, Rect, outline_for};

/// The most top-level shapes a selection by type reads: past them the page is left alone and the answer says how many.
pub const MAX_SELECT_TYPE_SHAPES: usize = 50_000;

/// The most top-level shapes a selection by region reads (the snap's index reads 10,000, and a shape of a region is read for its place and its paths).
pub const MAX_SELECT_REGION_SHAPES: usize = 10_000;

/// The most points a polygon has, counted before any is read: a lasso is simplified by the window long before it comes to this.
pub const MAX_SELECT_POINTS: usize = 2_048;

/// The most tests of a point or a segment against the edges of the polygon that one selection by region makes, about a fifth of a second.
pub const MAX_SELECT_WORK: usize = 40_000_000;

/// The most straight pieces the paths of one shape are cut into: past them the shape is taken by its box.
pub const MAX_SHAPE_POINTS: usize = 4_096;

// The layer table and the bounded reader of a `LayerMember` are the layers module's (`layers::read_layers`, `layers::read_members` and `layers::MemberBudget`): one table and one set
// of caps (a list of at most 16 KiB, a call of at most 8 MiB of lists, at most 1,024 pieces of a list, at most 4,096 layers) for the window's pointer, the snap, the commands that make
// layers and these queries.

/// What a shape is, in the words of the Select by Type dialog ("Shapes, Groups, Guides, OLE, Objects, Metafiles, Bitmaps and Ink Objects", read as seven types).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Shape,
    Group,
    Guide,
    Ole,
    Metafile,
    Bitmap,
    Ink,
}

impl Kind {
    /// Every type, in the dialog's order.
    pub const ALL: [Kind; 7] = [Kind::Shape, Kind::Group, Kind::Guide, Kind::Ole, Kind::Metafile, Kind::Bitmap, Kind::Ink];

    /// The word the commands take.
    pub const fn word(self) -> &'static str {
        match self {
            Kind::Shape => "shape",
            Kind::Group => "group",
            Kind::Guide => "guide",
            Kind::Ole => "ole",
            Kind::Metafile => "metafile",
            Kind::Bitmap => "bitmap",
            Kind::Ink => "ink",
        }
    }

    /// The words the dialog lists it by.
    pub const fn label(self) -> &'static str {
        match self {
            Kind::Shape => "Shapes",
            Kind::Group => "Groups",
            Kind::Guide => "Guides",
            Kind::Ole => "OLE objects",
            Kind::Metafile => "Metafiles",
            Kind::Bitmap => "Bitmaps",
            Kind::Ink => "Ink objects",
        }
    }

    /// The type a word names, whatever its case.
    pub fn from_word(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.word().eq_ignore_ascii_case(word.trim()))
    }
}

/// What a shape does: the roles of the Select by Type dialog's heading ("shapes, connectors or containers", Q6 of the spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Connector,
    Container,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Connector, Role::Container];

    pub const fn word(self) -> &'static str {
        match self {
            Role::Connector => "connector",
            Role::Container => "container",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Role::Connector => "Connectors",
            Role::Container => "Containers",
        }
    }

    pub fn from_word(word: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.word().eq_ignore_ascii_case(word.trim()))
    }
}

/// A layer a query names: by its `IX` in the page's `Layer` section or by its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerPick {
    Ix(u32),
    Name(String),
}

/// A selection by type: any of the `kinds`, any of the `roles`, on any of the `layers`; an empty list asks nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeQuery {
    pub kinds: Vec<Kind>,
    pub roles: Vec<Role>,
    pub layers: Vec<LayerPick>,
}

/// The shapes a selection would hold, and what it cost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked {
    pub page: u32,
    /// The `ID`s, in the page's order.
    pub ids: Vec<u32>,
    /// How many top-level shapes were read, and how many were left unread because the page has more than the limit.
    pub looked_at: usize,
    pub unread: usize,
    /// How many shapes the query would have taken but for a locked layer.
    pub locked: usize,
    /// By region: how many shapes were taken by their box alone because their paths could not be paid for.
    pub coarse: usize,
    /// By region: the tests of a point or a segment against the polygon's edges that were made.
    pub work: usize,
    /// By region: the straight pieces the flattening of curves made, and by type or region, the bytes of `LayerMember` text that were read.
    pub steps: usize,
    pub read: usize,
    /// How many shapes had a `LayerMember` too long to read ([`super::layers::MAX_MEMBER_BYTES`]), which is read as empty.
    pub overlong: usize,
}

/// A layer of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub ix: u32,
    pub name: String,
    pub locked: bool,
    /// How many shapes of the page are on it.
    pub count: usize,
}

/// What the Select by Type dialog offers for a page, and how many shapes each choice holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub page: u32,
    pub kinds: Vec<(Kind, usize)>,
    pub roles: Vec<(Role, usize)>,
    pub layers: Vec<LayerInfo>,
    pub looked_at: usize,
    pub unread: usize,
    /// The bytes of `LayerMember` text that were read, and the layers compared while the shapes were put on their layers.
    pub read: usize,
    pub lookups: usize,
    /// How many shapes had a `LayerMember` too long to read, which is read as empty.
    pub overlong: usize,
}

// ---------------------------------------------------------------------------------------------------------------------- what a shape is

/// The type of a shape: `None` for one the file gives none (a `Type` it does not name, or a picture with no `ForeignData`).
fn kind_of(shape: &diacraft_doc::Shape) -> Option<Kind> {
    match &shape.kind {
        ShapeType::Guide => Some(Kind::Guide),
        ShapeType::Group => Some(Kind::Group),
        ShapeType::Shape => Some(if shape.children.is_empty() { Kind::Shape } else { Kind::Group }),
        ShapeType::Foreign => foreign_kind(shape),
        ShapeType::Unspecified | ShapeType::Other(_) => None,
    }
}

/// The type of a picture or an object: its `ForeignType`, else what its picture is (`Bitmap`, `Metafile` and `EnhMetaFile`,
/// `Object`, `Ink`).
fn foreign_kind(shape: &diacraft_doc::Shape) -> Option<Kind> {
    let data = shape.foreign.as_ref()?;
    match data.attr("ForeignType") {
        Some("Bitmap") => Some(Kind::Bitmap),
        Some("Metafile" | "EnhMetaFile") => Some(Kind::Metafile),
        Some("Object") => Some(Kind::Ole),
        Some("Ink") => Some(Kind::Ink),
        Some(_) => None,
        None => data.picture.as_ref().map(|media| {
            let kind = media.content_type.to_ascii_lowercase();
            if kind.contains("emf") || kind.contains("wmf") { Kind::Metafile } else { Kind::Bitmap }
        }),
    }
}

/// The roles of a shape: a one-dimensional shape is a connector, and one whose `User.msvStructureType` says Container or List is a container (a list is also a container).
fn roles_of(reader: &Reader<'_>, sheet: &SheetRef) -> (bool, bool) {
    let connector = reader.is_one_d(sheet);
    let addr = CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value");
    let container = text_of(reader, sheet, &addr).is_some_and(|t| matches!(t.trim(), "Container" | "List"));
    (connector, container)
}

/// The text of a cell, found by inheritance.
fn text_of(reader: &Reader<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<String> {
    let found = reader.resolver().resolve(sheet, addr)?;
    found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The numbers of the layers a query names, which the page must have.
fn wanted_layers(layers: &[Layer], picks: &[LayerPick]) -> Result<HashSet<u32>, Problem> {
    let mut wanted = HashSet::new();
    for pick in picks {
        let found = match pick {
            LayerPick::Ix(ix) => layers.iter().find(|l| l.ix == *ix).map(|l| l.ix),
            LayerPick::Name(name) => layers.iter().find(|l| l.name == *name).or_else(|| layers.iter().find(|l| l.name_univ == *name)).map(|l| l.ix),
        };
        match (found, pick) {
            (Some(ix), _) => {
                wanted.insert(ix);
            }
            (None, LayerPick::Ix(ix)) => return Err(Problem::bad(format!("the page has no layer {ix}"))),
            (None, LayerPick::Name(name)) => {
                let named: Vec<String> = layers.iter().take(8).map(|l| format!("`{}`", crate::error::shown(&l.name))).collect();
                let tail = if named.is_empty() { "it has no layers".to_owned() } else { format!("its layers include {}", named.join(", ")) };
                return Err(Problem::bad(format!("the page has no layer `{}`; {tail}", crate::error::shown(name))));
            }
        }
    }
    Ok(wanted)
}

/// The shape at index `index` of the page's own list.
fn top_ref(page: u32, index: usize) -> Option<ShapeRef> {
    Some(ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(index).ok()?] })
}

// ---------------------------------------------------------------------------------------------------------------------- by type

/// The shapes of `page` a query takes.
pub fn by_type(doc: &Document, page: u32, query: &TypeQuery) -> Result<Picked, Problem> {
    require_page(doc, page)?;
    let part = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let reader = Reader::new(doc);
    let layers = read_layers(doc, page)?;
    let wanted = wanted_layers(&layers, &query.layers)?;
    // A shape is left out when it is on a locked layer (and counted) or only on hidden ones (a hidden shape cannot be selected): the layers' own rule.
    let table: HashMap<u32, &Layer> = layers.iter().map(|l| (l.ix, l)).collect();
    let needs_lists = layers.iter().any(|l| l.lock || !l.visible);
    let mut out = Picked { page, unread: part.shapes.len().saturating_sub(MAX_SELECT_TYPE_SHAPES), ..Picked::default() };
    let mut tally = MemberBudget::default();
    for (index, shape) in part.shapes.iter().enumerate().take(MAX_SELECT_TYPE_SHAPES) {
        out.looked_at += 1;
        if shape.del {
            continue;
        }
        if !query.kinds.is_empty() && !kind_of(shape).is_some_and(|k| query.kinds.contains(&k)) {
            continue;
        }
        let Some(at) = top_ref(page, index) else { continue };
        let sheet = SheetRef::Shape(at);
        if !query.roles.is_empty() {
            let (connector, container) = roles_of(&reader, &sheet);
            if !query.roles.iter().any(|r| (*r == Role::Connector && connector) || (*r == Role::Container && container)) {
                continue;
            }
        }
        let on = if !needs_lists && wanted.is_empty() { Vec::new() } else { shape_layers_charged(shape, &mut tally)? };
        if !query.layers.is_empty() && !on.iter().any(|ix| wanted.contains(ix)) {
            continue;
        }
        let standing = standing_of(&on, &table);
        if standing.locked {
            out.locked += 1;
            continue;
        }
        if standing.hidden {
            continue;
        }
        out.ids.push(shape.id);
    }
    (out.read, out.overlong) = (tally.read, tally.overlong);
    Ok(out)
}

/// What the dialog offers for `page`: the types, the roles and the layers, each with how many shapes of the page it holds (a shape on a locked layer counted as any other).
pub fn choices(doc: &Document, page: u32) -> Result<Choices, Problem> {
    require_page(doc, page)?;
    let part = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let reader = Reader::new(doc);
    let layers = read_layers(doc, page)?;
    let mut kinds = Kind::ALL.map(|k| (k, 0usize));
    let mut roles = Role::ALL.map(|r| (r, 0usize));
    let mut on_layer: Vec<usize> = vec![0; layers.len()];
    let mut looked_at = 0;
    let mut tally = MemberBudget::default();
    let mut lookups = 0usize;
    // The place of each layer in the table, found once: the first row of a number a hostile file repeats.
    let mut place: HashMap<u32, usize> = HashMap::with_capacity(layers.len());
    for (at, layer) in layers.iter().enumerate() {
        place.entry(layer.ix).or_insert(at);
    }
    for (index, shape) in part.shapes.iter().enumerate().take(MAX_SELECT_TYPE_SHAPES) {
        looked_at += 1;
        if shape.del {
            continue;
        }
        if let Some(kind) = kind_of(shape)
            && let Some(slot) = kinds.iter_mut().find(|(k, _)| *k == kind)
        {
            slot.1 += 1;
        }
        let Some(at) = top_ref(page, index) else { continue };
        let sheet = SheetRef::Shape(at);
        let (connector, container) = roles_of(&reader, &sheet);
        for (role, count) in &mut roles {
            if (*role == Role::Connector && connector) || (*role == Role::Container && container) {
                *count += 1;
            }
        }
        if !layers.is_empty() {
            let mut seen = HashSet::new();
            for ix in shape_layers_charged(shape, &mut tally)? {
                if seen.insert(ix) {
                    lookups += 1;
                    if let Some(count) = place.get(&ix).and_then(|at| on_layer.get_mut(*at)) {
                        *count += 1;
                    }
                }
            }
        }
    }
    let layers = layers.into_iter().zip(on_layer).map(|(l, count)| LayerInfo { name: layer_label(&l), ix: l.ix, locked: l.lock, count }).collect();
    Ok(Choices {
        page,
        kinds: kinds.to_vec(),
        roles: roles.to_vec(),
        layers,
        looked_at,
        unread: part.shapes.len().saturating_sub(MAX_SELECT_TYPE_SHAPES),
        read: tally.read,
        lookups,
        overlong: tally.overlong,
    })
}

/// What a layer is called: its name, else its universal name, else its number.
fn layer_label(layer: &Layer) -> String {
    if !layer.name.is_empty() {
        layer.name.clone()
    } else if !layer.name_univ.is_empty() {
        layer.name_univ.clone()
    } else {
        format!("Layer {}", layer.ix)
    }
}

// ---------------------------------------------------------------------------------------------------------------------- by region

/// One run of points of a shape's place: its box, or a path it draws.
struct Piece {
    points: Vec<P>,
    /// Whether the last point joins the first and the piece has an inside.
    closed: bool,
}

/// The polygon's own points with what repeats removed: a point equal to the one before it, and a closing point equal to the first.
fn normalised(points: &[P]) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// The polygon's edges, the last point joining the first.
fn edges(poly: &[P]) -> impl Iterator<Item = (P, P)> + '_ {
    poly.iter().copied().zip(poly.iter().copied().cycle().skip(1))
}

/// The rectangle the polygon is, when it is one: four points, each step along one axis, and so the net of a rectangular drag.
fn as_rect(poly: &[P]) -> Option<Rect> {
    if poly.len() != 4 {
        return None;
    }
    let r = Rect::round(poly)?;
    let on_corner = |p: &P| (p.0 == r.x0 || p.0 == r.x1) && (p.1 == r.y0 || p.1 == r.y1);
    let one_axis = edges(poly).all(|(a, b)| (a.0 == b.0) != (a.1 == b.1));
    (r.width() > 0.0 && r.height() > 0.0 && poly.iter().all(on_corner) && one_axis).then_some(r)
}

/// Whether the box `inner` lies inside the box `outer`, an edge in common allowed.
pub(crate) fn box_inside(inner: &Rect, outer: &Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// Whether `p` is on the segment `a` to `b`, to within a billionth of an inch.
fn on_segment(a: P, b: P, p: P) -> bool {
    const NEAR: f64 = 1.0e-9;
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::MIN_POSITIVE {
        return (p.0 - a.0).hypot(p.1 - a.1) <= NEAR;
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy)) <= NEAR
}

/// Whether `p` is inside the polygon or on its edge (even-odd rule, the polygon closing itself).
fn inside(poly: &[P], p: P) -> bool {
    let mut odd = false;
    for (a, b) in edges(poly) {
        if on_segment(a, b, p) {
            return true;
        }
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            odd = !odd;
        }
    }
    odd
}

/// Which side of the line `a` to `b` the point `c` is on: positive to the left, negative to the right, 0 on it.
fn side(a: P, b: P, c: P) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Whether the segments `a`-`b` and `c`-`d` share a point, one that is an end of either included.
fn meet(a: P, b: P, c: P, d: P) -> bool {
    let (s1, s2, s3, s4) = (side(a, b, c), side(a, b, d), side(c, d, a), side(c, d, b));
    if ((s1 > 0.0 && s2 < 0.0) || (s1 < 0.0 && s2 > 0.0)) && ((s3 > 0.0 && s4 < 0.0) || (s3 < 0.0 && s4 > 0.0)) {
        return true;
    }
    on_segment(a, b, c) || on_segment(a, b, d) || on_segment(c, d, a) || on_segment(c, d, b)
}

/// How many straight pieces a curve needs at most, for pieces no farther than `tolerance` from it, and `None` for one that needs more than `room` or whose numbers are not numbers. **Worked
/// out from the curve's control points, before it is cut**: Wang's bound for a Bezier curve, `n <= sqrt(d (d - 1) / 8 * m / tolerance)` for a curve of degree `d` whose greatest second
/// difference of control points is `m`, with its constant taken as 1 (it is 1/4 for a quadratic and 3/4 for a cubic), so the number is above what the cutting makes. A curve with
/// control points a million inches away at a fine tolerance is refused here, not cut into millions of pieces first.
fn pieces_needed(points: &[kurbo::Point], tolerance: f64, room: usize) -> Option<usize> {
    let second = |a: kurbo::Point, b: kurbo::Point, c: kurbo::Point| ((a.x - 2.0 * b.x + c.x).hypot(a.y - 2.0 * b.y + c.y)).abs();
    let m = match points {
        [p0, p1, p2] => second(*p0, *p1, *p2),
        [p0, p1, p2, p3] => second(*p0, *p1, *p2).max(second(*p1, *p2, *p3)),
        _ => return None,
    };
    let n = (m / tolerance).sqrt().ceil() + 1.0;
    // A number that is not one (a curve with control points that are not numbers) is not finite, and is refused.
    if n.is_finite() && n <= room as f64 { Some(n as usize) } else { None }
}

/// Cuts the paths into pieces of straight segments no farther than `tolerance` from them; `None` when they come to more than `room` pieces. **Each curve is paid for before it is cut**
/// ([`pieces_needed`]), and `steps` counts the pieces the cutting made.
fn flatten(paths: &[BezPath], tolerance: f64, room: usize, steps: &mut usize) -> Option<Vec<Piece>> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return None;
    }
    let mut left = room;
    let mut pieces: Vec<Piece> = Vec::new();
    let done = |current: &mut Vec<P>, closed: bool, pieces: &mut Vec<Piece>| {
        if current.len() > 1 {
            pieces.push(Piece { points: std::mem::take(current), closed });
        } else {
            current.clear();
        }
    };
    for path in paths {
        let mut current: Vec<P> = Vec::new();
        let (mut at, mut first) = (kurbo::Point::ZERO, kurbo::Point::ZERO);
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    done(&mut current, false, &mut pieces);
                    (at, first) = (p, p);
                    left = left.checked_sub(1)?;
                    current.push((p.x, p.y));
                }
                PathEl::LineTo(p) => {
                    at = p;
                    left = left.checked_sub(1)?;
                    current.push((p.x, p.y));
                }
                PathEl::ClosePath => {
                    done(&mut current, true, &mut pieces);
                    at = first;
                }
                PathEl::QuadTo(p1, p2) => {
                    left = left.checked_sub(pieces_needed(&[at, p1, p2], tolerance, left)?)?;
                    kurbo::flatten([PathEl::MoveTo(at), *el], tolerance, |e| cut(e, &mut current, steps));
                    at = p2;
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    left = left.checked_sub(pieces_needed(&[at, p1, p2, p3], tolerance, left)?)?;
                    kurbo::flatten([PathEl::MoveTo(at), *el], tolerance, |e| cut(e, &mut current, steps));
                    at = p3;
                }
            }
        }
        done(&mut current, false, &mut pieces);
    }
    if pieces.iter().flat_map(|p| p.points.iter()).any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return None;
    }
    Some(pieces)
}

/// One element the cutting of a curve made: a line to a point joins the piece being made.
fn cut(el: PathEl, current: &mut Vec<P>, steps: &mut usize) {
    *steps += 1;
    if let PathEl::LineTo(p) = el {
        current.push((p.x, p.y));
    }
}

/// The tests that one shape's place costs against the polygon, charged to `work`: refused in words past [`MAX_SELECT_WORK`].
fn charge(work: &mut usize, tests: usize, poly_points: usize) -> Result<(), Problem> {
    *work = work.saturating_add(tests);
    if *work > MAX_SELECT_WORK {
        return Err(Problem::failed(format!(
            "the region has too many points ({poly_points}) for the shapes it is set over: more than {MAX_SELECT_WORK} tests would be needed; draw a simpler loop or a smaller one"
        )));
    }
    Ok(())
}

/// The pieces of a shape's place: the box for a shape (a one-dimensional shape has none: its box is the line between its ends), and the paths it draws; a one-dimensional shape that
/// draws none has the straight line between its ends.
fn pieces_of(corners: &[P; 4], one_d: bool, mut drawn: Vec<Piece>) -> Vec<Piece> {
    if one_d {
        if drawn.is_empty() {
            drawn.push(Piece { points: vec![corners[0], corners[1]], closed: false });
        }
        return drawn;
    }
    let mut all = vec![Piece { points: corners.to_vec(), closed: true }];
    all.append(&mut drawn);
    all
}

/// Whether the shape whose place is `pieces` lies wholly inside the polygon.
fn wholly(poly: &[P], pieces: &[Piece], work: &mut usize) -> Result<bool, Problem> {
    let points: usize = pieces.iter().map(|p| p.points.len()).sum();
    charge(work, points.saturating_mul(poly.len()), poly.len())?;
    Ok(pieces.iter().all(|piece| piece.points.iter().all(|p| inside(poly, *p))))
}

/// Whether the polygon touches, crosses or lies inside the shape whose place is `pieces`.
fn partly(poly: &[P], pbox: &Rect, pieces: &[Piece], work: &mut usize) -> Result<bool, Problem> {
    let segments: usize = pieces.iter().map(|p| p.points.len().saturating_sub(1) + usize::from(p.closed)).sum();
    charge(work, segments.saturating_add(pieces.len()).saturating_mul(poly.len()).saturating_add(segments), poly.len())?;
    // A segment of the shape on a segment of the polygon.
    for piece in pieces {
        let mut run: Vec<(P, P)> = piece.points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect();
        if piece.closed
            && let (Some(last), Some(first)) = (piece.points.last(), piece.points.first())
        {
            run.push((*last, *first));
        }
        for (a, b) in run {
            let reach = Rect::of(a, b);
            if reach.meets(pbox) && edges(poly).any(|(c, d)| meet(a, b, c, d)) {
                return Ok(true);
            }
        }
    }
    // No crossing: each piece is wholly inside the polygon or wholly outside it, and the polygon is wholly inside a closed piece or wholly outside it.
    if pieces.iter().any(|piece| piece.points.first().is_some_and(|p| inside(poly, *p))) {
        return Ok(true);
    }
    let Some(first) = poly.first() else { return Ok(false) };
    Ok(pieces.iter().any(|piece| piece.closed && piece.points.len() >= 3 && inside(&piece.points, *first)))
}

/// The shapes of `page` the polygon `polygon` takes, in paper inches: those wholly inside it, and with `partial` those it touches or crosses as well.
pub fn by_region(doc: &Document, page: u32, polygon: &[P], partial: bool) -> Result<Picked, Problem> {
    require_page(doc, page)?;
    if polygon.len() > MAX_SELECT_POINTS {
        return Err(Problem::bad(format!("`polygon` holds at most {MAX_SELECT_POINTS} points, and this has {}", polygon.len())));
    }
    if polygon.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return Err(Problem::bad("each point of `polygon` must be a pair of finite numbers"));
    }
    let poly = normalised(polygon);
    let mut distinct = poly.clone();
    distinct.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    distinct.dedup();
    let Some(pbox) = Rect::round(&poly).filter(|_| distinct.len() >= 3) else {
        return Err(Problem::bad("a region needs at least three different points"));
    };
    let rect = as_rect(&poly);
    let part = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let k = units_per_inch(doc, page);
    let reader = Reader::new(doc);
    let layers = read_layers(doc, page)?;
    let table: HashMap<u32, &Layer> = layers.iter().map(|l| (l.ix, l)).collect();
    let needs_lists = layers.iter().any(|l| l.lock || !l.visible);
    let mut budget = Budget::page();
    let mut tally = MemberBudget::default();
    let mut out = Picked { page, unread: part.shapes.len().saturating_sub(MAX_SELECT_REGION_SHAPES), ..Picked::default() };
    for (index, shape) in part.shapes.iter().enumerate().take(MAX_SELECT_REGION_SHAPES) {
        out.looked_at += 1;
        if shape.del || shape.kind == ShapeType::Guide {
            continue;
        }
        let Some(at) = top_ref(page, index) else { continue };
        let corners = reader.corners(&at).map(|p| (p.0 / k, p.1 / k));
        let Some(cbox) = Rect::round(&corners) else { continue };
        let sheet = SheetRef::Shape(at.clone());
        // A shape that is to lie wholly inside has its box inside the polygon's, and one that is to be touched has its box meeting the polygon's: **but a connector's box is the
        // line between its ends and its route bends away from it**, so a connector is tried by its path whatever its box.
        if !partial && !box_inside(&cbox, &pbox) {
            continue;
        }
        let one_d = reader.is_one_d(&sheet);
        if partial && !one_d && !cbox.meets(&pbox) {
            continue;
        }
        if needs_lists {
            let standing = standing_of(&shape_layers_charged(shape, &mut tally)?, &table);
            if standing.locked {
                out.locked += 1;
                continue;
            }
            if standing.hidden {
                continue;
            }
        }
        let built = outline_for(&reader, &at, k, &mut budget, Purpose::Box);
        // Whether the shape is taken by less than all it draws: a path the budget could not pay for, or paths that came to too many pieces.
        let mut coarse = built.too_big > 0;
        let taken = match (rect, partial) {
            // A net, wholly: the box round the corners and the paths, in one test.
            (Some(net), false) => {
                let mut all = cbox;
                for path in &built.paths {
                    let b = path.bounding_box();
                    if b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite() {
                        all = Rect { x0: all.x0.min(b.x0), y0: all.y0.min(b.y0), x1: all.x1.max(b.x1), y1: all.y1.max(b.y1) };
                    }
                }
                box_inside(&all, &net)
            }
            _ => {
                let tolerance = (cbox.width().hypot(cbox.height()) / 1000.0).clamp(0.002, 50.0);
                let drawn = flatten(&built.paths, tolerance, MAX_SHAPE_POINTS, &mut out.steps).unwrap_or_else(|| {
                    coarse = true;
                    Vec::new()
                });
                let pieces = pieces_of(&corners, one_d, drawn);
                if partial { partly(&poly, &pbox, &pieces, &mut out.work)? } else { wholly(&poly, &pieces, &mut out.work)? }
            }
        };
        out.coarse += usize::from(coarse);
        if taken {
            out.ids.push(shape.id);
        }
    }
    (out.read, out.overlong) = (tally.read, tally.overlong);
    Ok(out)
}
