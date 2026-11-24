//! The GDI drawing state shared by EMF, the GetDC sections of EMF+, and WMF: the mapping from logical units to the frame, the objects
//! selected, the clip, the path being built, and what drawing with them makes.
//!
//! The record readers (`emf.rs`, `wmf.rs`) decode a record's bytes and call these methods; nothing here reads bytes.

use std::rc::Rc;
use std::sync::Arc;

use crate::canvas::Canvas;
use crate::clip::{self, Clip, ClipError, Mode, effective_of};
use crate::geom::{self, Op, P};
use crate::limits::{MAX_CLIP_POINTS, MAX_OBJECT_SLOTS};
use crate::model::{Cap, Detail, Drawn, Fill, Font, HAlign, Join, Paint, PathItem, Placement, Stroke, TextItem, VAlign};
use crate::xf::Xf;

/// `MM_*` map modes.
pub(crate) const MM_TEXT: u32 = 1;
pub(crate) const MM_ISOTROPIC: u32 = 7;
pub(crate) const MM_ANISOTROPIC: u32 = 8;

pub(crate) const WHITE: [u8; 4] = [255, 255, 255, 255];
pub(crate) const BLACK: [u8; 4] = [0, 0, 0, 255];

/// A COLORREF (0x00BBGGRR) as opaque red, green, blue and alpha.
pub(crate) fn colorref(v: u32) -> [u8; 4] {
    [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8, 255]
}

#[derive(Clone, Debug)]
pub(crate) enum PenStyle {
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
    User(Vec<f64>),
}

#[derive(Clone, Debug)]
pub(crate) struct Pen {
    pub null: bool,
    /// A cosmetic pen is one device pixel wide whatever its width says.
    pub cosmetic: bool,
    /// In logical units.
    pub width: f64,
    pub color: [u8; 4],
    pub dash: PenStyle,
    pub cap: Cap,
    pub join: Join,
    /// `PS_INSIDEFRAME`: the stroke of a closed figure stays inside it.
    pub inside: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Brush {
    Null,
    Solid([u8; 4]),
    /// A hatch or a pattern, which this crate does not draw (patterns occur only in GDI halves).
    Unsupported,
}

#[derive(Clone, Debug)]
pub(crate) struct FontObj {
    pub family: Arc<str>,
    /// The height field of the log font: negative is the em, positive the cell, 0 the default.
    pub height: f64,
    /// Tenths of a degree, counterclockwise.
    pub escapement: f64,
    pub weight: i32,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub charset: u8,
}

#[derive(Clone, Debug)]
pub(crate) enum Obj {
    Pen(Rc<Pen>),
    Brush(Rc<Brush>),
    Font(Rc<FontObj>),
    /// Something that is not drawn with (a palette, a colour space): the slot is taken.
    Other,
}

pub(crate) fn default_pen() -> Rc<Pen> {
    Rc::new(Pen { null: false, cosmetic: true, width: 1.0, color: BLACK, dash: PenStyle::Solid, cap: Cap::Round, join: Join::Round, inside: false })
}

pub(crate) fn null_pen() -> Rc<Pen> {
    Rc::new(Pen { null: true, ..(*default_pen()).clone() })
}

pub(crate) fn default_font() -> Rc<FontObj> {
    Rc::new(FontObj {
        family: Arc::from(""),
        height: 0.0,
        escapement: 0.0,
        weight: 400,
        italic: false,
        underline: false,
        strike_out: false,
        charset: 0,
    })
}

/// Where logical units land: an EMF's device, or a WMF's frame.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Surface {
    /// Device pixels per millimetre on each axis (from `Device` over `Millimeters`), and the frame's top-left corner in device pixels.
    Emf { px_per_mm: P, origin: P },
    /// A WMF: the window maps onto a frame of this size, whatever the map mode says.
    Wmf { frame: P },
}

/// The state a device context saves.
#[derive(Clone)]
pub(crate) struct Dc {
    pub map_mode: u32,
    pub win_org: P,
    pub win_ext: P,
    pub vp_org: P,
    pub vp_ext: P,
    pub world: Xf,
    pub pen: Rc<Pen>,
    pub brush: Rc<Brush>,
    pub font: Rc<FontObj>,
    pub text_color: [u8; 4],
    pub bk_color: [u8; 4],
    /// 1 transparent, 2 opaque.
    pub bk_mode: u32,
    /// 1 alternate (even-odd), 2 winding.
    pub poly_fill: u32,
    pub rop2: u32,
    pub text_align: u32,
    pub pos: P,
    pub miter: f64,
    pub clip: Clip,
    pub meta: Clip,
    /// The clip and the meta region together; kept so that the clips named in the output stay the same objects.
    pub effective: Clip,
}

impl Dc {
    /// The starting state.
    pub fn start() -> Dc {
        Dc {
            map_mode: MM_TEXT,
            win_org: [0.0, 0.0],
            win_ext: [1.0, 1.0],
            vp_org: [0.0, 0.0],
            vp_ext: [1.0, 1.0],
            world: Xf::IDENTITY,
            pen: default_pen(),
            brush: Rc::new(Brush::Solid(WHITE)),
            font: default_font(),
            text_color: BLACK,
            bk_color: WHITE,
            bk_mode: 2,
            poly_fill: 1,
            rop2: 13,
            text_align: 0,
            pos: [0.0, 0.0],
            miter: 10.0,
            clip: clip::unbounded(),
            meta: clip::unbounded(),
            effective: clip::unbounded(),
        }
    }
}

/// The GDI interpreter.
pub(crate) struct Gdi {
    pub dc: Dc,
    stack: Vec<Dc>,
    objects: Vec<Option<Obj>>,
    /// No slot below this one is free: a WMF object takes the first free slot, and this keeps the search from starting at the top each time
    /// (a table of 65,535 pens and a record to replace the last, over and over, would be 4 billion steps).
    first_free: usize,
    /// How many slots the file says it uses.
    handles: usize,
    pub surface: Surface,
    /// The path being built between BEGINPATH and ENDPATH, in frame coordinates.
    building: Option<Vec<Op>>,
    /// The path of the last bracket, waiting to be used once.
    built: Option<Vec<Op>>,
    /// The reference device's pixels to the inch on the y axis, for the default font size.
    pub dpi_y: f64,
}

impl Gdi {
    pub fn new(surface: Surface, handles: usize, dpi_y: f64) -> Gdi {
        Gdi {
            dc: Dc::start(),
            stack: Vec::new(),
            objects: Vec::new(),
            first_free: 0,
            handles: handles.min(MAX_OBJECT_SLOTS),
            surface,
            building: None,
            built: None,
            dpi_y: if dpi_y.is_finite() && dpi_y > 0.0 { dpi_y } else { 96.0 },
        }
    }

    // ---- mapping ----

    /// Logical units to device units or, for a WMF, to the frame: the window and the viewport.
    fn page_map(&self) -> Xf {
        let dc = &self.dc;
        match self.surface {
            Surface::Wmf { frame } => {
                let sx = if dc.win_ext[0] != 0.0 { frame[0] / dc.win_ext[0] } else { 1.0 };
                let sy = if dc.win_ext[1] != 0.0 { frame[1] / dc.win_ext[1] } else { 1.0 };
                let ex = if sx < 0.0 { frame[0] } else { 0.0 };
                let ey = if sy < 0.0 { frame[1] } else { 0.0 };
                Xf { a: sx, b: 0.0, c: 0.0, d: sy, e: -dc.win_org[0] * sx + ex, f: -dc.win_org[1] * sy + ey }
            }
            Surface::Emf { px_per_mm, .. } => {
                // The fixed modes: millimetres a unit, y up (the conversion to pixels is the header's).
                let fixed = |mm: f64| Some((px_per_mm[0] * mm, -px_per_mm[1] * mm));
                let (sx, sy) = match dc.map_mode {
                    2 => fixed(0.1),
                    3 => fixed(0.01),
                    4 => fixed(0.254),
                    5 => fixed(0.0254),
                    6 => fixed(25.4 / 1440.0),
                    MM_ISOTROPIC | MM_ANISOTROPIC => {
                        let (sx, sy) = (dc.vp_ext[0] / dc.win_ext[0], dc.vp_ext[1] / dc.win_ext[1]);
                        if dc.map_mode == MM_ISOTROPIC {
                            let m = sx.abs().min(sy.abs());
                            Some((m.copysign(sx), m.copysign(sy)))
                        } else {
                            Some((sx, sy))
                        }
                    }
                    _ => None,
                }
                .unwrap_or((1.0, 1.0));
                Xf { a: sx, b: 0.0, c: 0.0, d: sy, e: dc.vp_org[0] - dc.win_org[0] * sx, f: dc.vp_org[1] - dc.win_org[1] * sy }
            }
        }
    }

    /// Logical units to frame coordinates: the world transform, the page map and the move to the frame's corner.
    pub fn to_frame(&self) -> Xf {
        let shift = match self.surface {
            Surface::Emf { origin, .. } => Xf::translate(-origin[0], -origin[1]),
            Surface::Wmf { .. } => Xf::IDENTITY,
        };
        self.dc.world.then(self.page_map()).then(shift)
    }

    pub fn set_map_mode(&mut self, mode: u32) {
        // A mode outside 1 to 8 leaves the state as it was. A WMF's map mode does not change its mapping.
        if (1..=8).contains(&mode) && matches!(self.surface, Surface::Emf { .. }) {
            self.dc.map_mode = mode;
        }
    }

    pub fn set_window_org(&mut self, p: P) {
        self.dc.win_org = p;
    }

    pub fn set_viewport_org(&mut self, p: P) {
        self.dc.vp_org = p;
    }

    /// The extents count only in the modes that take them, and a zero leaves the state.
    pub fn set_window_ext(&mut self, p: P) {
        let takes = matches!(self.surface, Surface::Wmf { .. }) || matches!(self.dc.map_mode, MM_ISOTROPIC | MM_ANISOTROPIC);
        if takes && p[0] != 0.0 && p[1] != 0.0 {
            self.dc.win_ext = p;
        }
    }

    pub fn set_viewport_ext(&mut self, p: P) {
        let takes = matches!(self.dc.map_mode, MM_ISOTROPIC | MM_ANISOTROPIC) && matches!(self.surface, Surface::Emf { .. });
        if takes && p[0] != 0.0 && p[1] != 0.0 {
            self.dc.vp_ext = p;
        }
    }

    /// `EMR_SETWORLDTRANSFORM` and `EMR_MODIFYWORLDTRANSFORM`: mode 1 resets, 2 puts the record's matrix first, 3 last, 4 replaces. A
    /// matrix with a value that is not finite, or no inverse, leaves the transform.
    pub fn modify_world(&mut self, m: Xf, mode: u32) {
        let new = match mode {
            1 => Xf::IDENTITY,
            2 if m.is_usable() => m.then(self.dc.world),
            3 if m.is_usable() => self.dc.world.then(m),
            4 if m.is_usable() => m,
            _ => return,
        };
        if new.is_usable() {
            self.dc.world = new;
        }
    }

    // ---- state ----

    pub fn save(&mut self, cv: &mut Canvas) {
        if self.stack.len() >= crate::limits::MAX_SAVE_DEPTH {
            cv.budget.note_cap("saved states");
            return;
        }
        self.stack.push(self.dc.clone());
    }

    /// `EMR_RESTOREDC`: a negative count pops that many saved states (−1 is the latest). Anything else, or deeper than the stack, is ignored.
    pub fn restore(&mut self, n: i32) {
        let Some(count) = n.checked_neg().and_then(|c| usize::try_from(c).ok()).filter(|c| *c > 0) else {
            return;
        };
        let Some(at) = self.stack.len().checked_sub(count) else {
            return;
        };
        if let Some(state) = self.stack.get(at).cloned() {
            self.dc = state;
            self.stack.truncate(at);
        }
    }

    // ---- objects ----

    /// Puts an object in slot `ix`. Slot 0, and a slot at or past `Handles`, are not used.
    pub fn create(&mut self, ix: u32, obj: Obj) {
        let Ok(ix) = usize::try_from(ix) else { return };
        if ix == 0 || ix >= self.handles || ix >= MAX_OBJECT_SLOTS {
            return;
        }
        if self.objects.len() <= ix {
            self.objects.resize(ix + 1, None);
        }
        if let Some(slot) = self.objects.get_mut(ix) {
            *slot = Some(obj);
        }
    }

    /// WMF objects take the first free slot.
    pub fn create_next(&mut self, obj: Obj) {
        let mut free = self.first_free;
        while self.objects.get(free).is_some_and(Option::is_some) {
            crate::work::slot_passed();
            free += 1;
        }
        if free >= MAX_OBJECT_SLOTS {
            return;
        }
        if free >= self.objects.len() {
            self.objects.push(Some(obj));
        } else if let Some(slot) = self.objects.get_mut(free) {
            *slot = Some(obj);
        }
        self.first_free = free + 1;
    }

    fn stock(index: u32) -> Option<Obj> {
        let solid = |c: [u8; 4]| Some(Obj::Brush(Rc::new(Brush::Solid(c))));
        let pen = |c: [u8; 4]| Some(Obj::Pen(Rc::new(Pen { color: c, ..(*default_pen()).clone() })));
        match index {
            0 => solid(WHITE),
            1 => solid([192, 192, 192, 255]),
            2 => solid([128, 128, 128, 255]),
            3 => solid([64, 64, 64, 255]),
            4 => solid(BLACK),
            5 => Some(Obj::Brush(Rc::new(Brush::Null))),
            6 => pen(WHITE),
            7 => pen(BLACK),
            8 => Some(Obj::Pen(null_pen())),
            0x0A..=0x11 => Some(Obj::Font(default_font())),
            _ => None,
        }
    }

    /// `EMR_SELECTOBJECT`: replaces the current object of the same kind. An index that names no object is ignored.
    pub fn select(&mut self, index: u32) {
        let obj = if index & 0x8000_0000 != 0 {
            Self::stock(index & 0x7FFF_FFFF)
        } else {
            usize::try_from(index).ok().and_then(|i| self.objects.get(i)).and_then(|o| o.clone())
        };
        match obj {
            Some(Obj::Pen(p)) => self.dc.pen = p,
            Some(Obj::Brush(b)) => self.dc.brush = b,
            Some(Obj::Font(f)) => self.dc.font = f,
            Some(Obj::Other) | None => {}
        }
    }

    /// `EMR_DELETEOBJECT`: frees the slot; an object that is current gives way to the stock default of its kind.
    pub fn delete(&mut self, index: u32) {
        let Some(slot) = usize::try_from(index).ok().and_then(|i| self.objects.get_mut(i)) else {
            return;
        };
        // The slot is free again: a search for one starts no later than here.
        self.first_free = self.first_free.min(usize::try_from(index).unwrap_or(usize::MAX));
        match slot.take() {
            Some(Obj::Pen(p)) if Rc::ptr_eq(&p, &self.dc.pen) => self.dc.pen = default_pen(),
            Some(Obj::Brush(b)) if Rc::ptr_eq(&b, &self.dc.brush) => self.dc.brush = Rc::new(Brush::Solid(WHITE)),
            Some(Obj::Font(f)) if Rc::ptr_eq(&f, &self.dc.font) => self.dc.font = default_font(),
            _ => {}
        }
    }

    // ---- clipping ----

    pub fn reset_clip(&mut self) {
        self.dc.clip = clip::unbounded();
        self.dc.effective = self.dc.meta.clone();
    }

    /// Combines a region of rectangles in logical units with the clip by `mode`. A region of more rectangles than a clip may hold the points
    /// of is refused.
    pub fn clip_rects(&mut self, cv: &mut Canvas, rects: &[[f64; 4]], mode: Mode) -> Result<(), ClipError> {
        if rects.len().saturating_mul(5) as u64 > MAX_CLIP_POINTS {
            cv.budget.note_cap("clip size");
            return Err(ClipError::TooBig);
        }
        let Some(new) = clip::region_of_rects(rects, &self.to_frame()) else {
            return Err(ClipError::Cannot);
        };
        self.combine_clip(cv, &new, mode)
    }

    /// Combines `new` with the clip by `mode`. The points of the clips that are copied are charged as work first, the points the result
    /// keeps are charged after, and a result past the caps is refused with the clip as it was: no record can make a clip grow without
    /// the budget seeing it.
    pub fn combine_clip(&mut self, cv: &mut Canvas, new: &Clip, mode: Mode) -> Result<(), ClipError> {
        let copied = self.dc.clip.points().saturating_add(new.points()).saturating_add(self.dc.meta.points());
        if !cv.budget.work(copied.saturating_add(1)) {
            return Err(ClipError::TooBig);
        }
        let made = clip::capped(cv, clip::combine(&self.dc.clip, new, mode))?;
        let effective = clip::capped(cv, effective_of(&made, &self.dc.meta))?;
        if !cv.budget.points(made.points()) {
            return Err(ClipError::TooBig);
        }
        self.dc.clip = made;
        self.dc.effective = effective;
        Ok(())
    }

    /// `EMR_SETMETARGN`: the meta region becomes its intersection with the clip, and the clip is reset.
    pub fn set_meta_region(&mut self, cv: &mut Canvas) {
        let copied = self.dc.clip.points().saturating_add(self.dc.meta.points());
        if !cv.budget.work(copied.saturating_add(1)) {
            return;
        }
        let Ok(meta) = clip::capped(cv, clip::intersect(&self.dc.meta, &self.dc.clip)) else { return };
        if !cv.budget.points(meta.points()) {
            return;
        }
        self.dc.effective = meta.clone();
        self.dc.meta = meta;
        self.dc.clip = clip::unbounded();
    }

    // ---- paths ----

    pub fn begin_path(&mut self) {
        self.building = Some(Vec::new());
        self.built = None;
    }

    pub fn end_path(&mut self) {
        if let Some(ops) = self.building.take() {
            self.built = Some(ops);
        }
    }

    pub fn close_figure(&mut self) {
        if let Some(ops) = self.building.as_mut()
            && !ops.is_empty()
            && ops.last() != Some(&Op::Z)
        {
            ops.push(Op::Z);
        }
    }

    /// Takes the path of the last bracket, for one use.
    pub fn take_built(&mut self) -> Option<Vec<Op>> {
        self.built.take()
    }

    /// Adds ops (in frame coordinates) to the path being built, starting a figure at the current position first when `connect` and none
    /// is open.
    ///
    /// The points are charged as they are added: a path built from many records and never painted is held all the same.
    fn append(&mut self, cv: &mut Canvas, ops: Vec<Op>, connect: bool) {
        if !cv.budget.points(geom::point_count(&ops).saturating_add(1)) {
            return;
        }
        let start = self.to_frame().apply(self.dc.pos);
        let Some(path) = self.building.as_mut() else { return };
        let open = !path.is_empty() && path.last() != Some(&Op::Z);
        if connect && !open {
            path.push(Op::M(start));
        }
        path.extend(ops);
        crate::work::bracket_points_held(path.len());
    }

    // ---- drawing ----

    /// The clip every item of the moment is cut by.
    fn clip(&self) -> Clip {
        self.dc.effective.clone()
    }

    /// The stroke of the selected pen, when there is one.
    fn stroke(&self, xf: &Xf) -> Option<Stroke> {
        let pen = &self.dc.pen;
        if pen.null {
            return None;
        }
        let width = if pen.cosmetic || pen.width <= 0.0 { 1.0 } else { pen.width * xf.mean_scale() };
        if !width.is_finite() {
            return None;
        }
        // A cosmetic pen's dashes are in pixels, a geometric pen's in widths of the pen.
        let pattern: (&[f64], &[f64]) = match &pen.dash {
            PenStyle::Solid | PenStyle::User(_) => (&[], &[]),
            PenStyle::Dash => (&[18.0, 6.0], &[3.0, 1.0]),
            PenStyle::Dot => (&[3.0, 3.0], &[1.0, 1.0]),
            PenStyle::DashDot => (&[9.0, 6.0, 3.0, 6.0], &[3.0, 1.0, 1.0, 1.0]),
            PenStyle::DashDotDot => (&[9.0, 3.0, 3.0, 3.0, 3.0, 3.0], &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
        };
        let dashes: Vec<f64> = match &pen.dash {
            PenStyle::User(entries) => {
                let k = if pen.cosmetic { 1.0 } else { xf.mean_scale() };
                entries.iter().map(|v| v * k).collect()
            }
            _ if pen.cosmetic => pattern.0.to_vec(),
            _ => pattern.1.iter().map(|v| v * width.max(1.0)).collect(),
        };
        let dashes = if dashes.iter().all(|d| d.is_finite() && *d > 0.0) && !dashes.is_empty() { dashes } else { Vec::new() };
        Some(Stroke {
            paint: Paint::Solid(pen.color),
            width,
            cap: pen.cap,
            join: pen.join,
            miter_limit: self.dc.miter.max(1.0),
            dashes,
            dash_offset: 0.0,
        })
    }

    /// The fill of the selected brush, when there is one. A brush this crate does not draw is reported.
    fn fill(&self, cv: &mut Canvas, what: &str) -> Option<Fill> {
        match &*self.dc.brush {
            Brush::Null => None,
            Brush::Solid(c) => Some(Fill { paint: Paint::Solid(*c), even_odd: self.dc.poly_fill == 1 }),
            Brush::Unsupported => {
                cv.note(Detail::RecordSkipped, || format!("a hatch or pattern brush in {what}"));
                None
            }
        }
    }

    /// Paints a path in frame coordinates by the pen and the brush, or adds it to the path being built.
    fn put(&mut self, cv: &mut Canvas, ops: Vec<Op>, fill: bool, stroke: bool, what: &str) {
        let xf = self.to_frame();
        let fill = if fill { self.fill(cv, what) } else { None };
        let stroke = if stroke { self.stroke(&xf) } else { None };
        if fill.is_none() && stroke.is_none() {
            return;
        }
        let points = geom::point_count(&ops);
        let Some(path) = clip::path_of(&ops) else { return };
        cv.emit(&self.clip(), Drawn::Path(Box::new(PathItem { path, fill, stroke })), points);
    }

    /// A shape in logical units: painted at once, or added to the path being built.
    fn shape(&mut self, cv: &mut Canvas, logical: &[Op], fill: bool, stroke: bool, what: &str) {
        if !cv.budget.work(geom::point_count(logical).saturating_add(1)) {
            return;
        }
        let ops = geom::transformed(logical, &self.to_frame());
        if self.building.is_some() {
            self.append(cv, ops, false);
        } else {
            self.put(cv, ops, fill, stroke, what);
        }
    }

    /// Strokes a figure that stays inside its outline when the pen is `PS_INSIDEFRAME`: drawn here as the figure drawn smaller by half the
    /// pen. Returns the rectangle to draw.
    pub fn inset(&self, r: [f64; 4]) -> [f64; 4] {
        let pen = &self.dc.pen;
        if !pen.inside || pen.cosmetic || pen.null {
            return r;
        }
        let half = pen.width.abs() / 2.0;
        let (l, t, rr, b) = (r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3]));
        let (dx, dy) = (half.min((rr - l) / 2.0), half.min((b - t) / 2.0));
        [l + dx, t + dy, rr - dx, b - dy]
    }

    pub fn rectangle(&mut self, cv: &mut Canvas, r: [f64; 4]) {
        let r = self.inset(r);
        self.shape(cv, &geom::rect(r[0], r[1], r[2], r[3]), true, true, "a rectangle");
    }

    pub fn ellipse(&mut self, cv: &mut Canvas, r: [f64; 4]) {
        let r = self.inset(r);
        self.shape(cv, &geom::ellipse(r[0], r[1], r[2], r[3]), true, true, "an ellipse");
    }

    pub fn round_rect(&mut self, cv: &mut Canvas, r: [f64; 4], corner: P) {
        self.shape(cv, &geom::round_rect(r[0], r[1], r[2], r[3], corner[0], corner[1]), true, true, "a rounded rectangle");
    }

    /// ARC, CHORD and PIE.
    pub fn arc(&mut self, cv: &mut Canvas, r: [f64; 4], start: P, end: P, kind: geom::ArcKind) {
        let closed = kind != geom::ArcKind::Arc;
        self.shape(cv, &geom::arc_shape(r[0], r[1], r[2], r[3], start, end, kind), closed, true, "an arc");
    }

    /// POLYGON: filled by the brush and stroked by the pen, closed.
    pub fn polygon(&mut self, cv: &mut Canvas, pts: &[P]) {
        self.shape(cv, &geom::poly(pts, true), true, true, "a polygon");
    }

    /// POLYLINE: an open figure through the points; the current position is left alone.
    pub fn polyline(&mut self, cv: &mut Canvas, pts: &[P]) {
        self.shape(cv, &geom::poly(pts, false), false, true, "a polyline");
    }

    /// POLYGON and POLYLINE records in groups (POLYPOLYGON, POLYPOLYLINE).
    pub fn poly_groups(&mut self, cv: &mut Canvas, pts: &[P], counts: &[usize], close: bool) {
        self.shape(cv, &geom::subpaths(pts, counts, close), close, true, "a poly-polygon");
    }

    pub fn move_to(&mut self, p: P) {
        self.dc.pos = p;
        if self.building.is_some() {
            let q = self.to_frame().apply(p);
            if let Some(path) = self.building.as_mut() {
                path.push(Op::M(q));
            }
        }
    }

    /// LINETO: a line from the current position, which moves to the end.
    pub fn line_to(&mut self, cv: &mut Canvas, p: P) {
        let from = self.dc.pos;
        self.dc.pos = p;
        if !cv.budget.work(2) {
            return;
        }
        let xf = self.to_frame();
        let ops = vec![Op::L(xf.apply(p))];
        if self.building.is_some() {
            self.append(cv, ops, true);
        } else {
            let ops = vec![Op::M(xf.apply(from)), ops[0]];
            self.put(cv, ops, false, true, "a line");
        }
    }

    /// POLYLINETO: lines from the current position through the points; the position moves to the last.
    pub fn polyline_to(&mut self, cv: &mut Canvas, pts: &[P]) {
        let Some(last) = pts.last().copied() else { return };
        let from = self.dc.pos;
        self.dc.pos = last;
        if !cv.budget.work(pts.len() as u64) {
            return;
        }
        let xf = self.to_frame();
        if self.building.is_some() {
            self.append(cv, pts.iter().map(|p| Op::L(xf.apply(*p))).collect(), true);
        } else {
            let mut ops = vec![Op::M(xf.apply(from))];
            ops.extend(pts.iter().map(|p| Op::L(xf.apply(*p))));
            self.put(cv, ops, false, true, "a polyline");
        }
    }

    /// POLYBEZIER (from the first point) and POLYBEZIERTO (from the current position): three points a curve.
    pub fn bezier(&mut self, cv: &mut Canvas, pts: &[P], from_first: bool) {
        let (start, rest) = if from_first {
            match pts.split_first() {
                Some((first, rest)) => (*first, rest),
                None => return,
            }
        } else {
            (self.dc.pos, pts)
        };
        let curves = rest.len() / 3;
        let Some(last) = rest.get(..curves * 3).and_then(<[P]>::last).copied() else { return };
        if !from_first {
            self.dc.pos = last;
        }
        if !cv.budget.work(rest.len() as u64) {
            return;
        }
        let xf = self.to_frame();
        let mut ops = Vec::with_capacity(curves + 1);
        let cubic: Vec<Op> = rest.as_chunks::<3>().0.iter().map(|[a, b, c]| Op::C(xf.apply(*a), xf.apply(*b), xf.apply(*c))).collect();
        if self.building.is_some() {
            if from_first {
                ops.push(Op::M(xf.apply(start)));
                ops.extend(cubic);
                self.append(cv, ops, false);
            } else {
                self.append(cv, cubic, true);
            }
        } else {
            ops.push(Op::M(xf.apply(start)));
            ops.extend(cubic);
            self.put(cv, ops, false, true, "a curve");
        }
    }

    // ---- using a built path ----

    /// FILLPATH, STROKEPATH and STROKEANDFILLPATH use the last bracket's path once.
    pub fn paint_built(&mut self, cv: &mut Canvas, fill: bool, stroke: bool) {
        let Some(ops) = self.take_built() else { return };
        if !cv.budget.work(geom::point_count(&ops).saturating_add(1)) {
            return;
        }
        self.put(cv, ops, fill, stroke, "a path");
    }

    /// SELECTCLIPPATH: the last bracket's path combined with the clip by `mode`. A path of more points than a clip may hold is
    /// refused.
    pub fn clip_built(&mut self, cv: &mut Canvas, mode: Mode) -> Result<(), ClipError> {
        let Some(ops) = self.take_built() else { return Ok(()) };
        let points = geom::point_count(&ops);
        if points > MAX_CLIP_POINTS {
            cv.budget.note_cap("clip size");
            return Err(ClipError::TooBig);
        }
        if !cv.budget.work(points.saturating_add(1)) {
            return Err(ClipError::TooBig);
        }
        let Some(part) = clip::part_of(&ops, self.dc.poly_fill == 1) else { return Err(ClipError::Cannot) };
        self.combine_clip(cv, &clip::single(part), mode)
    }

    // ---- text ----

    /// The vectors one logical unit of text size becomes in the frame: along the baseline, then up from it. Text stays upright in
    /// `GM_COMPATIBLE`, where only the mapping of pages scales it, and follows the full transform in `GM_ADVANCED`; the escapement turns it
    /// counterclockwise on the frame.
    pub fn text_basis(&self, advanced: bool, escapement_tenths: f64) -> [f64; 4] {
        let page = self.page_map();
        let lin = if advanced { self.dc.world.then(page) } else { page };
        // Up is the way logical y runs against the page's y.
        let up_logical = if page.d >= 0.0 { [0.0, -1.0] } else { [0.0, 1.0] };
        let (mut right, mut up) =
            if advanced { (lin.apply_vector([1.0, 0.0]), lin.apply_vector(up_logical)) } else { ([page.a.abs(), 0.0], [0.0, -page.d.abs()]) };
        let theta = (escapement_tenths / 10.0).to_radians();
        if theta != 0.0 && theta.is_finite() {
            let (s, c) = theta.sin_cos();
            let turn = |v: [f64; 2]| [v[0] * c + v[1] * s, -v[0] * s + v[1] * c];
            right = turn(right);
            up = turn(up);
        }
        [right[0], right[1], up[0], up[1]]
    }

    /// The default text size, in logical units, for a font that asks for none: 12 points at the reference device's resolution.
    fn default_size(&self, basis: &[f64; 4]) -> f64 {
        let em_px = 12.0 / 72.0 * self.dpi_y;
        let per_unit = basis[2].hypot(basis[3]);
        if per_unit > 0.0 && per_unit.is_finite() { em_px / per_unit } else { 0.0 }
    }

    /// Writes a string at the reference point `reference` (logical units) in the selected font, as `EMR_EXTTEXTOUTW` and `META_EXTTEXTOUT`
    /// ask. `advances` are the characters' advances in logical units, one per character, when the record has them. Returns the width of the
    /// text when it is known, for a record that moves the current position.
    pub fn text(&mut self, cv: &mut Canvas, t: TextOut<'_>) -> Option<f64> {
        let TextOut { reference, chars, advances, advanced, rect, options } = t;
        if chars.is_empty() || !cv.budget.work(chars.len() as u64) {
            return None;
        }
        let font = Rc::clone(&self.dc.font);
        let basis = self.text_basis(advanced, font.escapement);
        let size = if font.height == 0.0 { self.default_size(&basis) } else { font.height.abs() };
        if !(size.is_finite() && size > 0.0 && basis.iter().all(|v| v.is_finite())) {
            return None;
        }
        let xf = self.to_frame();
        let align = self.dc.text_align;
        let reference = if align & 1 != 0 { self.dc.pos } else { reference };
        let width: Option<f64> = advances.map(|a| a.iter().sum());
        let h_align = match align & 0x6 {
            0x2 => HAlign::Right,
            0x6 => HAlign::Center,
            _ => HAlign::Left,
        };
        let v_align = match align & 0x18 {
            0x8 => VAlign::Bottom,
            0x18 => VAlign::Baseline,
            _ => VAlign::Top,
        };
        let origin = xf.apply(reference);
        let along = [basis[0], basis[1]];
        let placement = match (advances, width) {
            (Some(adv), Some(total)) if adv.len() == chars.len() => {
                let shift = match h_align {
                    HAlign::Left => 0.0,
                    HAlign::Center => -total / 2.0,
                    HAlign::Right => -total,
                };
                let mut x = shift;
                let mut origins = Vec::with_capacity(adv.len());
                for a in adv {
                    origins.push([origin[0] + along[0] * x, origin[1] + along[1] * x]);
                    x += a;
                }
                Placement::Origins(origins)
            }
            _ => Placement::Flow { origin, h_align },
        };
        let font_spec = Font {
            family: font.family.clone(),
            bold: font.weight >= 600,
            italic: font.italic,
            underline: font.underline,
            strike_out: font.strike_out,
            symbol: font.charset == 2,
        };
        let background = (self.dc.bk_mode == 2).then_some(self.dc.bk_color);
        let clip = self.text_clip(cv, rect, options);
        if options & 0x2 != 0
            && let Some(r) = rect
        {
            // ETO_OPAQUE: the rectangle is filled with the background colour first.
            let ops = geom::transformed(&geom::rect(r[0], r[1], r[2], r[3]), &xf);
            if let Some(path) = clip::path_of(&ops) {
                let item = PathItem { path, fill: Some(Fill { paint: Paint::Solid(self.dc.bk_color), even_odd: false }), stroke: None };
                cv.emit(&clip, Drawn::Path(Box::new(item)), geom::point_count(&ops));
            }
        }
        let item = TextItem {
            text: chars.iter().collect(),
            font: font_spec,
            color: self.dc.text_color,
            size,
            cell: font.height > 0.0,
            basis,
            placement,
            v_align,
            background: if options & 0x2 != 0 { None } else { background },
        };
        // The characters (and their positions) a text item keeps are charged as points: a file of long strings is held in full.
        cv.emit(&clip, Drawn::Text(Box::new(item)), chars.len() as u64);
        width
    }

    /// The clip a text run is cut by: the clip, and the record's rectangle under `ETO_CLIPPED`.
    fn text_clip(&mut self, cv: &mut Canvas, rect: Option<[f64; 4]>, options: u32) -> Clip {
        let Some(r) = rect.filter(|_| options & 0x4 != 0) else { return self.clip() };
        let Some(n) = clip::region_of_rects(&[r], &self.to_frame()) else { return self.clip() };
        if !cv.budget.work(self.dc.effective.points().saturating_add(n.points()).saturating_add(1)) {
            return clip::nothing();
        }
        clip::capped(cv, clip::intersect(&self.dc.effective, &n)).unwrap_or_else(|_| clip::nothing())
    }

    /// The width of one record's text moves the current position when the alignment says so.
    pub fn advance_position(&mut self, width: f64) {
        if self.dc.text_align & 1 != 0 && width.is_finite() {
            let dir = if self.dc.text_align & 0x6 == 0x2 {
                -1.0
            } else if self.dc.text_align & 0x6 == 0x6 {
                0.0
            } else {
                1.0
            };
            self.dc.pos[0] += width * dir;
        }
    }
}

/// What a text record asks for.
pub(crate) struct TextOut<'a> {
    pub reference: P,
    pub chars: &'a [char],
    pub advances: Option<&'a [f64]>,
    /// `GM_ADVANCED`: the text follows the transform.
    pub advanced: bool,
    pub rect: Option<[f64; 4]>,
    pub options: u32,
}
