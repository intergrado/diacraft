//! The shape a master's drop would make, found before it is dropped: what the window's ghost is drawn by and the snap of a drag pulls with (U49).
//!
//! A master dropped on a page is not the size it is listed at. Beyond the range of eight of the page's drawing scale it keeps its size on paper (
//! `draw/drop_scale.rs`), and a group whose height follows its text (UML Class's five, Crow's Foot's two entities) has the height of its compartments' text where that is taller than their least
//! heights, which no rule of scale gives: the least heights follow the page's drop scale by the factor the drop writes (a master's formula names the cell), but a text's
//! height is the text's. **So the preview is made by the drop itself, on a copy of the
//! document** (a copy shares everything with the document and is dropped at the end: a clone of a [`Document`] is a handful of reference counts), and what it reads off the shape
//! it made is what `shape.drop` reads off the shape it makes: its size on paper, which the command snaps as a box, and its connection points, which it pulls with the box. A master
//! whose drop is no one shape (BPMN's Pool is a structure of swimlanes made by code, `cmd/swimlane.rs`) has no shape to drop on a copy, so its preview is the master's size by the range
//! of eight and its places at that size, as the command takes them.
//!
//! **The copy's page holds no shapes** ([`bare_copy`]). What a drop reads of its page is the page's own sheet (its scale, its layers), never another shape, so the box and the places are
//! the same on the page as it is and on the page with nothing on it; and the work does not grow with the page: a drop outside a session's edit has no kept index of the page's formulas,
//! so its recalculation reads every cell of the page (46,896 steps on a page of 500 shapes and 300 connectors, where the bare page takes 3,144), and it walks the page for the new
//! shape's `ID` and to find the shape again.
//!
//! Where the shape goes changes nothing but where it is: the pin follows the middle of the box (`drop_scale`'s `recentre`), so a drop at another place has the same box and the same
//! places, moved by the distance. A window works a preview out **once for a drag** (and again when the drawing changes) and moves it with the pointer ([`DropPreview::rect_at`],
//! [`DropPreview::points_at`]). Every preview made on a copy is counted ([`super::Counts::drops`]), so that a test holds a drag to one.

use diacraft_doc::{Connects, Document, ShapeList};

use super::{MAX_OWN_POINTS, P, Rect, own_points};
use crate::draw::{self, drop_scale, units_per_inch};
use crate::stencil::{self, MasterSpec, OpenedStencil};

/// The master a drop is made of.
#[derive(Clone, Copy)]
pub enum DropMaster<'a> {
    /// A master of a built-in stencil.
    Stock(&'a MasterSpec),
    /// A master of a stencil opened from a file: `key` is the master as `shape.drop` takes it (its `ID` or its name).
    File { stencil: &'a OpenedStencil, key: &'a str },
}

/// The shape a drop would make: its size on paper and the places it brings (its connection points), for a drop centred at one place, and the same for any other by [`DropPreview::rect_at`]
/// and [`DropPreview::points_at`].
#[derive(Clone, Debug, PartialEq)]
pub struct DropPreview {
    /// Where the preview was made: the middle of the box, paper inches, y up.
    centre: P,
    /// The box's size on paper, in inches.
    width: f64,
    height: f64,
    /// The places, in paper inches, at most [`MAX_OWN_POINTS`], in the order of the `Connection` section.
    points: Vec<P>,
}

impl DropPreview {
    /// The width of the box the drop makes, in paper inches.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// The height of the box the drop makes, in paper inches.
    pub fn height(&self) -> f64 {
        self.height
    }

    /// The box of the shape dropped centred at `centre` (paper inches): the very box `shape.drop` snaps.
    pub fn rect_at(&self, centre: P) -> Rect {
        let (w, h) = (self.width, self.height);
        Rect { x0: centre.0 - w / 2.0, y0: centre.1 - h / 2.0, x1: centre.0 + w / 2.0, y1: centre.1 + h / 2.0 }
    }

    /// The places of the shape dropped centred at `centre` (paper inches): those the preview was made with, moved by the distance between the two places, so that for the place it
    /// was made at they are the places the drop makes to the last bit.
    pub fn points_at(&self, centre: P) -> Vec<P> {
        let shift = (centre.0 - self.centre.0, centre.1 - self.centre.1);
        self.points.iter().map(|p| (p.0 + shift.0, p.1 + shift.1)).collect()
    }
}

/// The shape dropping `master` centred at `centre` (paper inches) on `page` of `doc` would make, found by the drop itself on a copy of the document, the copy dropped at the end
/// (`doc` is not changed). `None` for a place that is not a number, a page that is not there, a master that is not in the stencil, and a drop the command would refuse (a page that is
/// full, a master with no shape).
pub fn drop_preview(doc: &Document, page: u32, master: DropMaster<'_>, centre: P) -> Option<DropPreview> {
    if !(centre.0.is_finite() && centre.1.is_finite()) || doc.page(page).is_none() {
        return None;
    }
    let k = units_per_inch(doc, page);
    let ((width, height), points) = match master {
        DropMaster::Stock(spec) if spec.makes_structure() => structure(spec, centre, k),
        DropMaster::Stock(spec) => {
            let mut scratch = bare_copy(doc, page)?;
            super::reads::count(|c| c.drops += 1);
            let info = draw::drop_shape_above(&mut scratch, page, spec, centre.0 * k, centre.1 * k, 0).ok()?;
            ((info.width / k, info.height / k), own_points(&scratch, page, &[info.id], (0.0, 0.0)))
        }
        DropMaster::File { stencil, key } => {
            let entry = stencil.find(key)?;
            let mut scratch = bare_copy(doc, page)?;
            super::reads::count(|c| c.drops += 1);
            let done = stencil::drop_opened(&mut scratch, page, stencil, entry, centre.0 * k, centre.1 * k, 0).ok()?;
            ((done.info.width / k, done.info.height / k), own_points(&scratch, page, &[done.info.id], (0.0, 0.0)))
        }
    };
    (width.is_finite() && height.is_finite()).then_some(DropPreview { centre, width, height, points })
}

/// A copy of `doc` whose page `page` holds no shapes and no connects (its sheet is kept: the scale, the layers, the rest of what the page's cells say). `None` for a page that is not there.
/// The copy shares everything else with `doc`, so it costs a few reference counts and the page's own record.
fn bare_copy(doc: &Document, page: u32) -> Option<Document> {
    let mut scratch = doc.clone();
    let record = draw::page_mut(&mut scratch, page)?;
    record.edit_shapes(|list| *list = ShapeList::new());
    record.connects = Connects::default();
    Some(scratch)
}

/// The preview of a master whose drop is a structure made by code (BPMN's Pool): the master's size by the range of eight (`drop_scale::built_in_size`, which the command takes too) and
/// its connection points at that size, the box centred at `centre`. The pin is the centre, in drawing units, and the box's lower left is half its size from it; a point is its offset from that
/// corner.
fn structure(spec: &MasterSpec, centre: P, k: f64) -> ((f64, f64), Vec<P>) {
    let (width, height) = drop_scale::built_in_size(spec.width, spec.height, k);
    let (low_x, low_y) = (centre.0 * k - width / 2.0, centre.1 * k - height / 2.0);
    let points =
        spec.connection_offsets_at(width, height).into_iter().take(MAX_OWN_POINTS).map(|(x, y)| ((low_x + x) / k, (low_y + y) / k)).collect();
    ((width / k, height / k), points)
}

/// The size on paper, in inches, of a master drawn at 1:1 of `width` by `height` drawing inches, dropped on a page of `k` drawing inches to a paper inch, **by the range of eight alone**
/// Its drawing size over `k` within the range, its own beyond it. What a window draws for a master whose drop it cannot make on a copy ([`drop_preview`] says none), and
/// no more than that: a group that follows its text has the height of its text.
pub fn size_on_paper(width: f64, height: f64, k: f64) -> (f64, f64) {
    let (w, h) = drop_scale::built_in_size(width, height, k);
    if k.is_finite() && k > 0.0 { (w / k, h / k) } else { (w, h) }
}

/// The places a master of a built-in stencil brings when it is dropped centred at `centre` (paper inches) on `page` of `doc`: its connection points, which `shape.drop` pulls with the new
/// shape's box, at most [`MAX_OWN_POINTS`]. They are what [`own_points`] reads off the shape the drop makes, in the order of its `Connection` section, for
/// every master and every scale of the page (the drop is made on a copy of the document: [`drop_preview`]). A connector has none; empty for a place that is not a number, a page that is not
/// there and a drop the command would refuse.
pub fn dropped_points(doc: &Document, page: u32, spec: &MasterSpec, centre: P) -> Vec<P> {
    drop_preview(doc, page, DropMaster::Stock(spec), centre).map(|preview| preview.points_at(centre)).unwrap_or_default()
}

/// [`dropped_points`] for a master of a stencil file (`stencil` is the opened file, `key` the master as `shape.drop` takes it: its `ID` or its name). A file's master has no `MasterSpec`:
/// its connection rows are in its own model, as formulas of its shape's size, and the dropped shape's places are what those come to on the instance, whatever the master's rows are (a point
/// off every corner and middle of its box, a formula of the size, a master of several shapes). Empty for a master that is not in the stencil, a drop the command would refuse (a page that is
/// full, a master with no shape), a page that is not there and a place that is not a number.
pub fn dropped_points_of_file(doc: &Document, page: u32, stencil: &OpenedStencil, key: &str, centre: P) -> Vec<P> {
    drop_preview(doc, page, DropMaster::File { stencil, key }, centre).map(|preview| preview.points_at(centre)).unwrap_or_default()
}
