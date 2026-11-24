//! Where a drag of the window lands: the engine's snap, asked while the drag goes on, and the marks drawn of what it snapped to.
//!
//! **The window does not snap by itself.** A move, a resize, a turn, a drop and a connector's free end are sent with `snap: {zoom, view, ...}` and the engine
//! snaps them by the document's Snap and Glue settings, guides and other shapes included, and holds a box pulled to a guide to it. The preview
//! asks the engine the same question first, through the same functions the commands call (`diacraft_engine::snap`: `snap_ext`, `bounds_of`, `frame_of_shapes`,
//! `own_points`, `dropped_points`, `snap_resize_ext`, `snap_turn`) with the same words, so that it shows where the release lands
//! (`docs/editing-commands.md`, "Snap, grid and guides"):
//!
//! * a **move** snaps the box of the shapes where the drag puts them, with their own connection points pulled with it, as `shape.move` does; the box is the one the
//!   command aligns by (`bounds_of`: for a shape turned by other than whole quarter turns, the box round the paths it draws), and **one turned shape
//!   also lines up its own sides** (`frame_of_shapes`), the box, the points and the frame worked out once for the drag ([`MoveSubject`]) and moved by the
//!   distance at each move. **Shift keeps the move to the page axis the whole drag has gone farther along** (`lock`), and the free axis snaps;
//! * a **resize** is `snap_resize_ext`, as `shape.resize`'s: a shape that is turned or flipped is snapped along its own axes, a corner keeps the
//!   proportions and a side only with Shift (`keep`), and a width or height stops at that of a shape in the view (`view`);
//! * a **turn** by the rotation handle is `snap_turn`, as `shape.rotate`'s with `snap: {zoom, radius}`: another shape's angle within reach, else the
//!   step the distance from the pin gives;
//! * a **drop** snaps the box of the master at the pointer, with the master's own connection points pulled with it as `shape.drop` pulls the new shape's
//!   (`dropped_points` for a stock master and `dropped_points_of_file` for a master of a stencil file);
//! * a connector's **free end** snaps as the end of a connector (`Use::Connect`), and is drawn glued where it lands on a connection point;
//! * a **text box** of the Text tool is dragged out from the corner the press landed at, its moved sides snapped as a resize's are, with the width or height of a shape in the view
//!   (`text_box_snap`), and the Text Block tool's drags snap places (their commands take no `snap`, so what is sent is what the snap gave).
//!
//! **What the snap draws is the engine's marks** ([`Mark`], `snap.marks`): every line the landed box lies on from the box to the nearest aligned shape (along a turned
//! shape's own side too), every equal gap with its value, the places met, the sizes and the angles; `paint.rs` draws each by its segment.
//!
//! **The page as the snap reads it is kept** ([`SnapCache`]: the guides, the grid, the settings) while the drawing, its revision and the page stay as they are, and
//! **what the snap asks of the shapes of the page (their index and the target set) is made by [`SnapReader`](crate::snap_reader::SnapReader), off the window's thread
//! **A drag begins by asking for it, the first frames go as the pointer made them (no snap is asked or sent: the preview is the release), and the moves after
//! that are queries (the review: 36 ms a move when each move read the page again; 81 ms for the first on 5,000 shapes).
//!
//! **Alt held turns snapping off for the drag** (the window's own, as the docs say): nothing is asked and no `snap` is sent.
//!
//! Lengths are paper inches, as the editing commands have them. What is sent is rounded to a millionth of an inch ([`super::inch`]), and the preview is
//! worked out from the rounded numbers, so that it is the release to the last digit the engine sees.

use std::sync::Arc;

use diacraft_engine::snap::{
    Axis, BoxAt, Edges, Extras, Glue, Marks, OwnFrame, Query, Rect, SnapMark, SnapPage, SnapSettings, Snapped, Subject, Turn, Use, bounds_of,
    frame_of_shapes, own_points, pin_of, resized_by, snap_ext, snap_resize_ext, snap_resize_set, snap_turn, snaps_in_its_own_axes,
};

use diacraft_engine::draw::LegSubject;

use super::Cx;
use super::geom::P;
use crate::app::DiacraftApp;
use crate::snap_reader::{Job, Kind, PageKey, Warm};

/// A length as the window sends it: rounded to a millionth of an inch, as [`super::inch`] writes it.
pub fn sent(v: f64) -> f64 {
    (v * 1.0e6).round() / 1.0e6
}

/// Notes that the reader has made a page ready for `done`, in the list of what it is ready for (`SnapCache::warm`). **The page is ready for the shapes of the last answer only**
/// (of the re-review): the engine holds one set of targets for a page, the one it built last, and that set leaves out the shapes that moved in the request
/// it was built for, so what was made ready for other shapes (a drag of another shape at the same revision, which was cancelled) is no longer ready, and a window that still said
/// it was would build that set on its own thread. The kinds of query the one landing asks for (the Text tool's two) move the same shapes, and stay, **each once**: there are as
/// many as there are kinds of query (seven), so the list is short and needs no cap.
fn note_ready(warm: &mut Vec<Warm>, done: Warm) {
    warm.retain(|held| held.moving == done.moving);
    if !warm.contains(&done) {
        warm.push(done);
    }
}

/// Something a drag snapped to, as the window draws it: one of the engine's marks.
pub type Mark = SnapMark;

/// A mark as `ui.inspect` tells it, so that a client reads what the window draws: `{mark: "line" | "gap" | "place" | "size" | "angle", ...}` with the segment of a line or a
/// gap (`from`, `to`) and the `label` of a gap, a size or an angle, as the preview paints them. Lengths are paper inches, rounded to a millionth.
pub fn mark_json(mark: &Mark) -> serde_json::Value {
    use serde_json::json;
    let at = |p: P| json!([sent(p.0), sent(p.1)]);
    match mark {
        Mark::Line { axis, normal, at: line, from, to, targets, shape, count, hit } => json!({
            "mark": "line", "axis": normal.map_or(json!(axis.name()), |_| serde_json::Value::Null), "normal": normal.map(|n| sent(n.to_degrees())), "at": sent(*line),
            "from": at(*from), "to": at(*to), "targets": targets.iter().map(|t| t.name()).collect::<Vec<_>>(), "shape": shape, "count": count, "hit": hit,
        }),
        Mark::Gap { axis, normal, from, to, value, label, chain, own, hit } => json!({
            "mark": "gap", "axis": normal.map_or(json!(axis.name()), |_| serde_json::Value::Null), "normal": normal.map(|n| sent(n.to_degrees())),
            "from": at(*from), "to": at(*to), "value": sent(*value), "label": label, "chain": chain, "own": own, "hit": hit,
        }),
        Mark::Place { at: place, target, shape, point, hit } => json!({
            "mark": "place", "at": at(*place), "target": target.name(), "shape": shape, "point": point, "hit": hit,
        }),
        Mark::Size { value, label, side, matched, count, hit } => json!({
            "mark": "size", "value": sent(*value), "label": label, "side": [at(side.0), at(side.1)],
            "matched": matched.map(|(id, l)| json!({"shape": id, "side": [at(l.0), at(l.1)]})), "count": count, "hit": hit,
        }),
        Mark::Angle { pivot, degrees, label, matched, step, hit } => json!({
            "mark": "angle", "pivot": at(*pivot), "degrees": sent(*degrees), "label": label, "matched": matched, "step": step.map(sent), "hit": hit,
        }),
    }
}

/// The visible part of the page, as the window sends it with every snapping command (`view`: left, bottom, right, top, paper inches), each number rounded to a millionth
/// of an inch so that the preview asks what the release is asked.
pub fn view_of(cx: &Cx) -> [f64; 4] {
    let (lo, hi) = cx.view.visible(cx.area);
    [sent(lo.0), sent(lo.1), sent(hi.0), sent(hi.1)]
}

/// Where a place lands: the place, the connection point it is glued to (`Use::Connect` only), and what it snapped to.
#[derive(Clone, Debug, PartialEq)]
pub struct PointSnap {
    pub at: P,
    pub glued: Option<(u32, u32)>,
    pub marks: Vec<Mark>,
}

/// The page as the snap reads it, kept while the document and the page stay as they are: what the guides are drawn from, the grid's lines, and every
/// snap of a drag (`SnapPage`'s own advice: made when a drag begins and again after an edit). Keyed by the drawing's generation as well as the
/// revision, since an opened file's session counts its revisions from 1 again.
pub struct SnapCache {
    key: PageKey,
    /// Shared with the reader's thread, which makes its index of the shapes and its set of targets there.
    pub page: Arc<SnapPage>,
    /// The document's Snap and Glue settings, as the commands read them (`SnapSettings::of`).
    pub settings: SnapSettings,
    /// What the reader has made the page ready for (**every kind, for the shapes of the last answer only**: the engine holds the one set of targets it built last, see
    /// [`note_ready`]): a snap of one of these kinds by these shapes is a query, where any other would build
    /// the index and the targets on the window's thread. A landing may ask for two kinds (the Text tool's drag), and a page kept ready for the last alone would be asked for the
    /// other again at every frame.
    warm: Vec<Warm>,
    /// Whether the reader's work on this page panicked: the page is not snapped by the preview (and the release goes without `snap`).
    failed: bool,
}

impl SnapCache {
    /// The drawing's generation, the revision and the page it was read for.
    pub fn key(&self) -> PageKey {
        self.key
    }

    /// Whether the page is ready for `want`.
    pub fn is_ready_for(&self, want: &Warm) -> bool {
        self.warm.contains(want)
    }
}

/// What a move snaps, worked out once for the drag (not at each move of the pointer: finding each shape is a walk of the page): the shapes that move,
/// the box they align by now, their own connection points now and, for one turned shape, its own frame now, at the drawing's generation and revision it was read at.
#[derive(Clone, Debug)]
pub struct MoveSubject {
    pub key: (u64, u64),
    /// The shapes that move (for a copy, the shapes copied).
    pub ids: Vec<u32>,
    /// The box they align by, on the page: what `shape.move` makes of them (`bounds_of`, the box round the paths of a turned shape and the box round the
    /// corners of the others); `None` past [`diacraft_engine::snap::MAX_SNAPPED_SHAPES`] (no `snap` is sent) or when one is not there.
    pub now: Option<Rect>,
    /// Their connection points where they are (`own_points` with no shift), which a move carries by its distance. None for a copy.
    pub own: Vec<P>,
    /// The own frame of the one shape that moves when it is turned by other than whole quarter turns (`frame_of_shapes`), where it is: its own sides line up with the
    /// like sides of other shapes turned alike, as `shape.move` does. None for a copy, a selection of several shapes and a shape that is not turned.
    pub frame: Option<OwnFrame>,
}

impl DiacraftApp {
    /// The document's Snap & Glue settings now, as the commands read them (`SnapSettings::of`), read once for each revision of a drawing and kept (an opened
    /// file's session counts its revisions from 1 again, so the drawing's generation is part of the key). `None` with no document.
    pub(crate) fn snap_settings_now(&self) -> Option<SnapSettings> {
        let doc = self.session.document()?;
        let key = (self.ui.generation, self.session.revision());
        if let Some((held, settings)) = self.ui.edit.settings.get()
            && held == key
        {
            return Some(settings);
        }
        let settings = SnapSettings::of(doc);
        self.ui.edit.settings.set(Some((key, settings)));
        Some(settings)
    }

    /// Makes the snap's reading of `page` if the one held is of another drawing, revision or page. → whether there is one. **This reads the guides and the grid
    /// only** (a pass over the page's shapes for the guides): the index of the shapes and the targets are the reader's ([`DiacraftApp::snap_ready`]).
    pub(crate) fn ensure_snap_page(&mut self, page: u32) -> bool {
        let key = (self.ui.generation, self.session.revision(), page);
        if self.ui.edit.snap_page.as_ref().is_some_and(|c| c.key == key) {
            return true;
        }
        crate::work::count(|w| w.snap_page_reads += 1);
        // The read is the session's, kept for this revision (`Session::snap_page`): the reader's thread makes the index of the shapes and the targets on it, and the commands
        // the drag sends find it there.
        self.ui.edit.snap_page = self.session.document().zip(self.session.snap_page(page)).map(|(doc, snap_page)| SnapCache {
            key,
            page: snap_page,
            settings: SnapSettings::of(doc),
            warm: Vec::new(),
            failed: false,
        });
        self.ui.edit.snap_page.is_some()
    }

    /// Takes the answers the reader has for the pages it was asked to make ready: a page still the one held is ready for what was asked (or, when the work did not
    /// finish, is not snapped by the preview). → how many answers there were.
    /// What a page is ready for after an answer is [`note_ready`]'s.
    fn take_reads(&mut self) -> usize {
        let mut taken = 0;
        for done in self.ui.edit.reader.poll() {
            taken += 1;
            self.ui.edit.reads_done += 1;
            if let Some(cache) = self.ui.edit.snap_page.as_mut().filter(|c| c.key == done.key) {
                if done.made {
                    note_ready(&mut cache.warm, done.warm);
                } else {
                    cache.failed = true;
                }
            }
        }
        taken
    }

    /// Once a frame, before the drag is landed: the answers the reader's thread has for the pages it was asked to make ready, and a frame asked for when there were some.
    pub(crate) fn poll_snap_reads(&mut self, ctx: &egui::Context) {
        self.ui.edit.reader.attach(ctx);
        if self.take_reads() > 0 {
            ctx.request_repaint();
        }
    }

    /// Whether the page `page` is ready for a snap of the kind `kind` by the shapes `moving`: the reader has made its index of the shapes and its targets. If it has not
    /// been asked, it is asked now; **the answer is then no, and the drag goes as the pointer made it until it is yes** (inline, where the work is done here, it is yes at
    /// once). A page whose reading failed is never ready.
    pub(crate) fn snap_ready(&mut self, page: u32, moving: &[u32], kind: Kind) -> bool {
        if !self.ensure_snap_page(page) {
            return false;
        }
        let want = Warm::new(moving, kind);
        let Some(cache) = self.ui.edit.snap_page.as_ref() else { return false };
        if cache.failed {
            return false;
        }
        if cache.is_ready_for(&want) {
            return true;
        }
        if self.ui.edit.reader.is_pending(cache.key, &want) {
            return false;
        }
        let Some(doc) = self.session.document() else { return false };
        let job = Job { key: cache.key, warm: want.clone(), doc: Arc::new(doc.clone()), page: Arc::clone(&cache.page), settings: cache.settings };
        self.ui.edit.reader.submit(job);
        // Inline, the answer is waiting: take it.
        if self.ui.edit.reader.is_inline() {
            self.take_reads();
        }
        self.ui.edit.snap_page.as_ref().is_some_and(|c| !c.failed && c.is_ready_for(&want))
    }

    /// Asks the reader to make `page` ready for a move of the shapes `ids` that is sent with `snap` (an arrow key's press): the box of shapes, with the own frame of one turned
    /// shape as the command asks it. The answer is the page the session keeps for this revision (`Session::snap_page`), which `shape.move` finds, so that the press builds neither
    /// the index of the shapes nor the targets on this thread. → whether it is ready now (inline, where the work is done here, it is).
    pub(crate) fn make_ready_for_move(&mut self, page: u32, ids: &[u32]) -> bool {
        let framed = self.session.document().is_some_and(|doc| frame_of_shapes(doc, page, ids).is_some());
        self.snap_ready(page, ids, Kind::Box { framed })
    }

    /// Whether the reader has a page to make ready that it has not finished with: the window is not yet at rest.
    pub fn snap_reading(&self) -> bool {
        self.ui.edit.reader.busy()
    }

    /// The lines of the grid of `page` at `zoom` screen points to a paper inch, on each axis, as the snap has them (in the drawing's units and at its
    /// scale: in drawing inches). `None` on an axis with no line (a fixed grid of spacing 0).
    pub(crate) fn grid_marks(&mut self, page: u32, zoom: f64) -> [Option<Marks>; 2] {
        if !self.ensure_snap_page(page) {
            return [None, None];
        }
        match &self.ui.edit.snap_page {
            Some(cache) => [cache.page.grid.grid(Axis::X, cache.page.k, zoom), cache.page.grid.grid(Axis::Y, cache.page.k, zoom)],
            None => [None, None],
        }
    }

    /// What a move of `ids` snaps (copied, for a Ctrl copy-drag), read once: the shapes, the box they align by, their own connection points and, for one turned
    /// shape, its own frame. `moving` are the shapes the command is sent with.
    pub(crate) fn move_subject(&self, page: u32, moving: Vec<u32>, copy: bool) -> MoveSubject {
        crate::work::count(|w| w.move_subjects += 1);
        let key = (self.ui.generation, self.session.revision());
        let doc = self.session.document();
        let within = moving.len() <= diacraft_engine::snap::MAX_SNAPPED_SHAPES;
        // The box `shape.move` snaps (`bounds_of`), so that the preview lands where the release does: a turned shape lines up by the box round its paths.
        let now = doc.filter(|_| within && !moving.is_empty()).and_then(|doc| bounds_of(doc, page, &moving)).map(|b| b.rect);
        // A copy is snapped as a box alone, its originals targets (no shapes move, so none of their points are pulled and none lines up by its own sides).
        let (own, frame) = match (doc, now, copy) {
            (Some(doc), Some(_), false) => (own_points(doc, page, &moving, (0.0, 0.0)), frame_of_shapes(doc, page, &moving)),
            _ => (Vec::new(), None),
        };
        MoveSubject { key, ids: moving, now, own, frame }
    }

    /// Where a box on `page` lands: the engine's snap of it on the kept page, with `moving` the shapes that move (never targets), `own` their places that are pulled with it
    /// and `extras` the words of the command (`lock`, `view`, `frame`). `None` when there is no page to snap on, or it is not ready yet (the preview then shows the drag as
    /// it is, and the release is sent without `snap`).
    pub(crate) fn snap_box(&mut self, page: u32, rect: Rect, moving: &[u32], own: &[P], zoom: f64, extras: Extras) -> Option<Snapped> {
        let kind = Kind::Box { framed: extras.frame.is_some() };
        if !self.snap_ready(page, moving, kind) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        let query = Query { subject: Subject::Box(rect), using: Use::Place, moving, zoom, own_points: own };
        let snapped = counting_builds(|| snap_ext(doc, &cache.page, &cache.settings, &query, extras));
        (snapped.dx.is_finite() && snapped.dy.is_finite()).then_some(snapped)
    }

    /// Where a place on `page` lands: put down (`Use::Place`) or the end of a connector (`Use::Connect`), which glues to a connection point (`glued`), as
    /// `connector.add` and `connector.reconnect` take it. `ids` are never targets; `view` is sent with it, and a place has no use for it.
    pub(crate) fn snap_place(&mut self, page: u32, p: P, using: Use, ids: &[u32], zoom: f64, view: [f64; 4]) -> Option<PointSnap> {
        if !self.snap_ready(page, ids, Kind::Point(using)) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        let query = Query { subject: Subject::Point(p), using, moving: ids, zoom, own_points: &[] };
        let extras = Extras { view: Some(view), ..Extras::default() };
        let snapped = counting_builds(|| snap_ext(doc, &cache.page, &cache.settings, &query, extras));
        let at = snapped.apply(p);
        if !(at.0.is_finite() && at.1.is_finite()) {
            return None;
        }
        let glued = match snapped.glue {
            Some(Glue::ConnectionPoint { shape, point }) if using == Use::Connect => Some((shape, point)),
            _ => None,
        };
        Some(PointSnap { at, glued, marks: snapped.marks })
    }

    /// Where a leg dragged by hand lands (`legs.rs`): the middle of the leg, on the line the pointer gives, is the place (`Use::Place`, the connector never a target), and it moves
    /// along the one axis the leg can move on (`Extras::lock`), as `connector.edit_path` with `snap` asks it (`draw::leg_subject` makes the place and the axis for
    /// both). → the line the leg comes to, in paper inches, and what the snap pulled it to; `None` while the page is not ready (the drag then goes as the pointer made it, and the
    /// release is sent without `snap`).
    pub(crate) fn snap_leg(&mut self, page: u32, id: u32, subject: LegSubject, zoom: f64, view: [f64; 4]) -> Option<(f64, Vec<Mark>)> {
        if !self.snap_ready(page, &[id], Kind::Point(Use::Place)) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        let query = Query { subject: Subject::Point(subject.place), using: Use::Place, moving: &[id], zoom, own_points: &[] };
        let extras = Extras { lock: Some(subject.axis), view: Some(view), ..Extras::default() };
        let snapped = counting_builds(|| snap_ext(doc, &cache.page, &cache.settings, &query, extras));
        let at = snapped.apply(subject.place);
        let to = if subject.axis == Axis::Y { at.1 } else { at.0 };
        to.is_finite().then_some((to, snapped.marks))
    }

    /// A resize's snap, by the engine's own (`snap_resize_ext`, as `shape.resize` works it out) on the kept page: the box `sized` (the numbers sent: the corner at the
    /// shape's own (0, 0), then its size along its own axes) of shape `id`, snapped by the edges that differ from its box now, with `extras` (`view`, `keep`). → `None`
    /// while the page is not ready; else what `snap_resize_ext` says: the snap and the box it comes to, or `None` when no edge moves, when the snap would collapse the box, or
    /// when the shape is not one the engine snaps (a shape in a turned group, or one whose corners make no rectangle).
    pub(crate) fn resize_snap(&mut self, page: u32, id: u32, sized: BoxAt, zoom: f64, extras: Extras) -> Option<Option<(Snapped, BoxAt)>> {
        let framed = self.session.document().is_some_and(|doc| snaps_in_its_own_axes(doc, page, id));
        if !self.snap_ready(page, &[id], Kind::Edges { framed }) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        Some(counting_builds(|| snap_resize_ext(doc, page, Some(cache.page.as_ref()), id, sized, zoom, extras)))
    }

    /// A resize of several shapes' snap, by the engine's own (`snap_resize_set`, as `shape.resize` with `ids` works it out) on the kept page: the joint box of `ids` made the box
    /// `sized` (lower left, then width and height), snapped by the edges that differ from the joint box now, with `extras` (`view`, `keep`). → `None` while the page is not ready;
    /// else what `snap_resize_set` says: the snap and the box it comes to, or `None` when no edge moves or the snap would collapse the box.
    pub(crate) fn resize_snap_set(&mut self, page: u32, ids: &[u32], sized: BoxAt, zoom: f64, extras: Extras) -> Option<Option<(Snapped, BoxAt)>> {
        if !self.snap_ready(page, ids, Kind::Edges { framed: false }) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        Some(counting_builds(|| snap_resize_set(doc, page, Some(cache.page.as_ref()), ids, sized, zoom, extras)))
    }

    /// Where the Text tool's drag of a new box lands: the box from the corner `first` (where the press landed) to the pointer `now`, **its moved sides snapped as a resize's
    /// are**: the right or left side the pointer is on and the top or bottom one, to the lines of the shapes, the guides and the grid, and to the width or height
    /// of a shape in the view, with the marks the snap gives (a size mark on both boxes, labelled). `text.box` takes no `snap`, so what the drag lands at is what is sent, and the
    /// preview is the release. → the corner opposite `first` where the box comes to, and the marks; `None` while the page is not ready, or when the snap would leave the box
    /// no size (a drag with no width or no height has none to match): the drag then goes as the pointer made it.
    pub(crate) fn text_box_snap(&mut self, page: u32, first: P, now: P, zoom: f64, view: [f64; 4]) -> Option<(P, Vec<Mark>)> {
        if !self.snap_ready(page, &[], Kind::Edges { framed: false }) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        let rect = Rect { x0: first.0.min(now.0), y0: first.1.min(now.1), x1: first.0.max(now.0), y1: first.1.max(now.1) };
        // The sides the pointer moves: the far side in each direction from the press.
        let edges = Edges { left: now.0 < first.0, right: now.0 >= first.0, bottom: now.1 < first.1, top: now.1 >= first.1 };
        crate::work::count(|w| w.snaps += 1);
        let query = Query { subject: Subject::Edges(rect, edges), using: Use::Place, moving: &[], zoom, own_points: &[] };
        let extras = Extras { view: Some(view), ..Extras::default() };
        let snapped = counting_builds(|| snap_ext(doc, &cache.page, &cache.settings, &query, extras));
        let (x, y, w, h) = resized_by((rect.x0, rect.y0, rect.width(), rect.height()), &snapped)?;
        // `first` stays where it is: the corner of the new box that is not the pointer's.
        let second = (if edges.right { x + w } else { x }, if edges.top { y + h } else { y });
        (second.0.is_finite() && second.1.is_finite()).then_some((second, snapped.marks))
    }

    /// A turn's snap: the engine's `snap_turn` of shape `id` to the angle `raw` degrees (as sent) with the pointer `radius` inches from the pin (as sent), as
    /// `shape.rotate` with `snap: {zoom, radius}` works it out. `None` while the page is not ready.
    pub(crate) fn turn_snap(&mut self, page: u32, id: u32, raw: f64, radius: f64, zoom: f64) -> Option<Snapped> {
        if !self.snap_ready(page, &[id], Kind::Turn) {
            return None;
        }
        let (doc, cache) = (self.session.document()?, self.ui.edit.snap_page.as_ref()?);
        crate::work::count(|w| w.snaps += 1);
        // The pin the shape turns about, where the angle mark is drawn: `pin_of`, which the command takes too (it was the middle of the shape's box, which is
        // the pin only for a shape whose pin is in the middle).
        let pivot = pin_of(doc, page, id).unwrap_or((0.0, 0.0));
        let turn = Turn { pivot, angle: raw.to_radians(), radius, zoom };
        Some(counting_builds(|| snap_turn(doc, &cache.page, &cache.settings, &[id], turn)))
    }
}

/// Runs a query of the snap, and counts the index of a page's shapes and the sets of targets it made on this thread (the engine's own counts, `snap::counts`): there should be
/// none, since the reader made them before the query was asked (a test of the window holds both counts to 0).
fn counting_builds<T>(query: impl FnOnce() -> T) -> T {
    let before = diacraft_engine::snap::counts();
    let answer = query();
    let after = diacraft_engine::snap::counts();
    crate::work::count(|w| {
        w.index_built_here += after.index_builds.saturating_sub(before.index_builds);
        w.targets_built_here += after.target_builds.saturating_sub(before.target_builds);
    });
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> Cx {
        Cx {
            area: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 480.0)),
            view: crate::canvas::view::View { zoom: 1.0, center: (5.0, 2.5), fitted: false },
            page: 1,
            settings: crate::canvas::page_drawing::PageSettings::default(),
        }
    }

    #[test]
    fn the_view_sent_is_the_visible_part_of_the_page_rounded_to_a_millionth() {
        // 96 points to an inch: 960 by 480 points are 10 by 5 inches round the middle (5, 2.5).
        assert_eq!(view_of(&cx()), [0.0, 0.0, 10.0, 5.0]);
        let shifted = Cx { view: crate::canvas::view::View { zoom: 1.0, center: (5.0 + 1.0 / 3.0, 2.5), fitted: false }, ..cx() };
        let view = view_of(&shifted);
        assert_eq!(view, [sent(1.0 / 3.0), 0.0, sent(10.0 + 1.0 / 3.0), 5.0]);
        assert_eq!(view[0], 0.333333, "a number as the command takes it");
    }

    #[test]
    fn a_mark_is_told_with_its_segment_and_its_label() {
        use diacraft_engine::snap::Target;
        let gap = Mark::Gap {
            axis: Axis::X,
            normal: None,
            from: (1.0, 2.0),
            to: (2.5, 2.0),
            value: 1.5,
            label: "1.5 in".to_owned(),
            chain: 3,
            own: true,
            hit: true,
        };
        let told = mark_json(&gap);
        assert_eq!(told["mark"], "gap");
        assert_eq!((&told["axis"], &told["label"], &told["chain"]), (&serde_json::json!("x"), &serde_json::json!("1.5 in"), &serde_json::json!(3)));
        assert_eq!((&told["from"], &told["to"]), (&serde_json::json!([1.0, 2.0]), &serde_json::json!([2.5, 2.0])));
        // A line along no axis is told by its normal in degrees, and has no axis.
        let line = Mark::Line {
            axis: Axis::Y,
            normal: Some(std::f64::consts::FRAC_PI_3 * 2.0),
            at: 0.5,
            from: (0.0, 0.0),
            to: (1.0, 1.0),
            targets: vec![Target::Alignment],
            shape: Some(7),
            count: 2,
            hit: false,
        };
        let told = mark_json(&line);
        assert_eq!(
            (&told["axis"], &told["normal"], &told["targets"], &told["shape"]),
            (&serde_json::Value::Null, &serde_json::json!(120.0), &serde_json::json!(["alignment"]), &serde_json::json!(7))
        );
        let place = Mark::Place { at: (1.0, 2.0), target: Target::ConnectionPoint, shape: Some(1), point: Some(2), hit: true };
        assert_eq!(
            (mark_json(&place)["target"].clone(), mark_json(&place)["point"].clone()),
            (serde_json::json!("connection_point"), serde_json::json!(2))
        );
        let angle = Mark::Angle { pivot: (1.0, 1.0), degrees: 33.0, label: "33 deg".to_owned(), matched: Some(4), step: None, hit: true };
        assert_eq!((mark_json(&angle)["label"].clone(), mark_json(&angle)["matched"].clone()), (serde_json::json!("33 deg"), serde_json::json!(4)));
        let size = Mark::Size {
            value: 2.0,
            label: "2 in".to_owned(),
            side: ((0.0, 0.0), (2.0, 0.0)),
            matched: Some((5, ((0.0, 3.0), (2.0, 3.0)))),
            count: 1,
            hit: true,
        };
        let told = mark_json(&size);
        assert_eq!((&told["label"], &told["matched"]["shape"]), (&serde_json::json!("2 in"), &serde_json::json!(5)));
    }

    /// `counting_builds` counts what a query made on this thread: the index of the shapes and the set of targets, each once for a page nobody made ready, and none on a page
    /// that was made ready (the count the tests of the window hold to 0 is not one that counts nothing).
    #[test]
    fn counting_builds_counts_the_index_and_the_targets_a_query_made() {
        use diacraft_engine::snap::{Extras, Kind, Query, Rect, SnapPage, SnapSettings, Subject, Use, make_ready, snap_ext};
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
        let shape = |id: u32, x: f64| {
            ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "4")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
        };
        let doc = DocBuilder::new()
            .page(1, PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(shape(1, 3.0)).shape(shape(2, 7.0)))
            .build();
        let settings = SnapSettings { alignment_box: true, ..SnapSettings::of(&doc) };
        let ask = |page: &SnapPage| {
            let query = Query {
                subject: Subject::Box(Rect { x0: 2.0, y0: 3.0, x1: 3.0, y1: 4.0 }),
                using: Use::Place,
                moving: &[1],
                zoom: 96.0,
                own_points: &[],
            };
            counting_builds(|| snap_ext(&doc, page, &settings, &query, Extras::default()));
        };
        crate::work::reset();
        let cold = SnapPage::read(&doc, 1).unwrap();
        ask(&cold);
        assert_eq!(
            (crate::work::work().index_built_here, crate::work::work().targets_built_here),
            (1, 1),
            "a page nobody made ready is made by the query"
        );
        ask(&cold);
        assert_eq!((crate::work::work().index_built_here, crate::work::work().targets_built_here), (1, 1), "and once");
        crate::work::reset();
        let ready = SnapPage::read(&doc, 1).unwrap();
        make_ready(&doc, &ready, &settings, &[1], Kind::Box { framed: false });
        crate::work::reset();
        ask(&ready);
        assert_eq!((crate::work::work().index_built_here, crate::work::work().targets_built_here), (0, 0), "a page made ready builds nothing");
    }

    /// What a page is ready for after each answer (of the re-review): the kinds of one landing stay together, an answer that comes twice is one
    /// entry, and the answer for other shapes takes the others away (the engine holds the one set it built last).
    #[test]
    fn a_page_is_ready_for_the_kinds_of_the_shapes_of_the_last_answer_each_once() {
        use diacraft_engine::snap::{Kind, Use};
        let of = |moving: &[u32], kind: Kind| Warm::new(moving, kind);
        let (boxed, place) = (Kind::Box { framed: false }, Kind::Point(Use::Place));
        let mut warm = Vec::new();
        note_ready(&mut warm, of(&[1], boxed));
        note_ready(&mut warm, of(&[1], place));
        assert_eq!(warm, [of(&[1], boxed), of(&[1], place)], "the two kinds of one landing wait together, in the order they were answered");
        note_ready(&mut warm, of(&[1], boxed));
        assert_eq!(warm.len(), 2, "an answer that comes twice is one entry and does not push the others out");
        note_ready(&mut warm, of(&[1], Kind::Turn));
        assert_eq!(warm, [of(&[1], boxed), of(&[1], place), of(&[1], Kind::Turn)]);
        note_ready(&mut warm, of(&[2], boxed));
        assert_eq!(warm, [of(&[2], boxed)], "another drag's shapes: what was made ready for the first is not ready any more");
        note_ready(&mut warm, of(&[1], boxed));
        assert_eq!(warm, [of(&[1], boxed)], "and the first drag asked again is ready only for itself");
        // The order the shapes were named in is no difference.
        note_ready(&mut warm, of(&[5, 3], Kind::Turn));
        note_ready(&mut warm, of(&[3, 5], boxed));
        assert_eq!(warm.len(), 2, "{warm:?}");
    }

    #[test]
    fn lengths_are_sent_to_a_millionth() {
        assert_eq!(sent(1.0 / 3.0), 0.333333);
        assert_eq!(sent(2.0000004), 2.0);
    }
}
