//! EMF+ (MS-EMFPLUS): the drawing records that ride in `EMR_COMMENT` records.
//!
//! This is the half of an EMF+ file that the viewer plays. It plays what the files hold, ranked by R14, and when any record type, or
//! an object a record needs, is outside what it can play, it says so ([`Plus::outcome`]) and the caller decides between the EMF half
//! (Dual files) and the partial picture (EMF+ Only files) by R5.
//!
//! A record type outside is not guessed at: a record the crate has no reading for would draw wrongly, and R5 exists for it.

use std::rc::Rc;
use std::sync::Arc;

use crate::bytes::{Bytes, decode_utf16};
use crate::canvas::Canvas;
use crate::clip::{self, Clip, ClipError, Mode};
use crate::dib;
use crate::geom::{self, Op, P};
use crate::limits::{
    MAX_BITMAP_PIXELS, MAX_BITMAP_SIDE, MAX_CLIP_POINTS, MAX_NESTING, MAX_PLUS_OBJECT_BYTES, MAX_PLUS_OBJECTS, MAX_REGION_DEPTH, MAX_SAVE_DEPTH,
};
use crate::model::{
    BitmapItem, Cap, Detail, Drawn, Fill, Font, HAlign, Image, Join, LinearPaint, Paint, PathItem, Placement, RadialPaint, Stroke, TextItem, VAlign,
};
use crate::nest;
use crate::xf::Xf;

/// What playing the EMF+ records came to.
pub(crate) enum PlusOutcome {
    Fine,
    /// A record or an object could not be played; the words say which.
    Unplayable(String),
}

/// A graphics state: what Save and Restore keep.
#[derive(Clone)]
struct State {
    world: Xf,
    /// The page unit (`UnitType`) and the page scale.
    unit: u8,
    scale: f64,
    clip: Clip,
}

/// A brush.
#[derive(Clone)]
enum Brush {
    Solid([u8; 4]),
    Linear {
        rect: [f64; 4],
        stops: Arc<[(f64, [u8; 4])]>,
        transform: Xf,
    },
    Path {
        center: P,
        radii: P,
        stops: Arc<[(f64, [u8; 4])]>,
        transform: Xf,
    },
    /// A brush of a kind this crate does not play (hatch, texture).
    Other,
}

#[derive(Clone)]
struct Pen {
    /// In world units.
    width: f64,
    brush: Brush,
    cap: Cap,
    join: Join,
    miter: f64,
    /// Dashes in widths of the pen: on, off, on...
    dashes: Vec<f64>,
    dash_offset: f64,
}

/// The nodes of a region.
enum Node {
    Rect([f64; 4]),
    Path(Rc<Vec<Op>>),
    Empty,
    Infinite,
    Combine(Mode, Box<Node>, Box<Node>),
}

enum Meta {
    Bitmap(Image),
    /// A whole metafile of any kind.
    Metafile(Arc<Vec<u8>>),
}

enum Object {
    Brush(Rc<Brush>),
    Pen(Rc<Pen>),
    Path(Rc<Vec<Op>>),
    /// The tree, and what evaluating it costs: its nodes and the points of its paths.
    Region(Rc<Node>, u64),
    Image(Rc<Meta>),
    Font(Rc<FontObject>),
    /// An object this crate keeps no reading of and does not need (image attributes, string formats, custom caps).
    Unused,
    /// An object whose bytes did not parse, decode or stay within the caps: using it makes the picture unplayable.
    Bad(&'static str),
}

struct FontObject {
    em: f64,
    style: u32,
    family: Arc<str>,
}

/// An object arriving in pieces.
struct Partial {
    id: usize,
    kind: u8,
    total: usize,
    data: Vec<u8>,
}

pub(crate) struct Plus {
    depth: u32,
    dual: bool,
    /// The frame's top-left corner in device pixels.
    origin: P,
    dpi: P,
    state: State,
    saved: Vec<(u32, State)>,
    objects: Vec<Option<Object>>,
    partial: Option<Partial>,
    failed: Option<String>,
}

const IDENTITY_STATE: fn() -> State = || State { world: Xf::IDENTITY, unit: 2, scale: 1.0, clip: clip::unbounded() };

/// An EMF+ colour, 0xAARRGGBB, as red, green, blue and alpha.
fn argb(v: u32) -> [u8; 4] {
    [((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8, (v >> 24) as u8]
}

fn lerp(a: [u8; 4], b: [u8; 4], t: f64) -> [u8; 4] {
    let mut c = [0u8; 4];
    for (i, v) in c.iter_mut().enumerate() {
        let (x, y) = (f64::from(a.get(i).copied().unwrap_or(0)), f64::from(b.get(i).copied().unwrap_or(0)));
        *v = (x + (y - x) * t.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8;
    }
    c
}

impl Plus {
    pub fn new(depth: u32, dual: bool, origin: P) -> Plus {
        let mut objects = Vec::new();
        objects.resize_with(MAX_PLUS_OBJECTS, || None);
        Plus { depth, dual, origin, dpi: [96.0, 96.0], state: IDENTITY_STATE(), saved: Vec::new(), objects, partial: None, failed: None }
    }

    pub fn outcome(&self) -> PlusOutcome {
        match &self.failed {
            Some(why) => PlusOutcome::Unplayable(why.clone()),
            None => PlusOutcome::Fine,
        }
    }

    fn fail(&mut self, why: impl FnOnce() -> String) {
        if self.failed.is_none() {
            self.failed = Some(why());
        }
    }

    /// Plays the EMF+ records of one comment (the bytes after the identifier). Returns whether a GetDC section starts after it: the last
    /// record was a GetDC.
    pub fn comment(&mut self, stream: Bytes, cv: &mut Canvas, in_getdc: bool) -> bool {
        let mut pos = 0usize;
        let mut getdc = in_getdc;
        while pos < stream.len() && cv.budget.alive() {
            let (Some(typ), Some(flags), Some(size), Some(dsize)) =
                (stream.u16(pos), stream.u16(pos + 2), stream.count(pos + 4), stream.count(pos + 8))
            else {
                break;
            };
            // A record must fit in its comment, and its data in the record; one that does not ends the comment.
            let Some(rec) = size.checked_sub(12).filter(|room| dsize <= *room).and_then(|_| stream.sub(pos, size)) else { break };
            let Some(data) = rec.sub(12, dsize) else { break };
            if !cv.budget.record() || !cv.budget.work(size as u64 / 4 + 1) {
                break;
            }
            pos += size.max(12);
            getdc = typ == 0x4004;
            if self.failed.is_some() && self.dual {
                // The EMF half will draw the picture: the rest of this half is not worth playing.
                continue;
            }
            self.record(typ, flags, data, cv);
        }
        getdc
    }

    fn record(&mut self, typ: u16, flags: u16, d: Bytes, cv: &mut Canvas) {
        match typ {
            0x4001 => self.header(d),
            // EndOfFile, Comment, GetDC, and the settings a vector renderer has no use for.
            0x4002..=0x4004 | 0x401D..=0x4022 | 0x4024 => {}
            0x4008 => self.object(flags, d, cv),
            0x400A | 0x4014 | 0x4015 | 0x4036 | 0x401B => self.draw(typ, flags, d, cv),
            0x4025..=0x4030 => self.state_record(typ, flags, d, cv),
            0x4032..=0x4034 => self.clip_record(typ, flags, d, cv),
            _ => self.fail(|| format!("EMF+ record type {typ:#06x}")),
        }
    }

    fn header(&mut self, d: Bytes) {
        if let (Some(x), Some(y)) = (d.u32(8), d.u32(12)) {
            let ok = |v: u32| (1..=100_000).contains(&v);
            if ok(x) && ok(y) {
                self.dpi = [f64::from(x), f64::from(y)];
            }
        }
    }

    // ---- transforms ----

    /// A page unit as device pixels: pixels are the unit of the records, and the others use the header's logical resolution.
    fn unit_pixels(&self, unit: u8) -> P {
        match unit {
            3 => [self.dpi[0] / 72.0, self.dpi[1] / 72.0],
            4 => self.dpi,
            5 => [self.dpi[0] / 300.0, self.dpi[1] / 300.0],
            6 => [self.dpi[0] / 25.4, self.dpi[1] / 25.4],
            _ => [1.0, 1.0],
        }
    }

    /// World coordinates to frame coordinates: the world transform, the page transform, and the move to the frame's corner.
    fn to_frame(&self) -> Xf {
        let k = self.unit_pixels(self.state.unit);
        self.state.world.then(Xf::scale(k[0] * self.state.scale, k[1] * self.state.scale)).then(Xf::translate(-self.origin[0], -self.origin[1]))
    }

    fn state_record(&mut self, typ: u16, flags: u16, d: Bytes, cv: &mut Canvas) {
        match typ {
            0x4025 | 0x4028 => {
                if self.saved.len() >= MAX_SAVE_DEPTH {
                    cv.budget.note_cap("saved states");
                } else if let Some(i) = d.u32(0) {
                    self.saved.push((i, self.state.clone()));
                }
            }
            0x4026 | 0x4029 => {
                // Looking for the state through the saved ones is a walk of as many as are saved: charged.
                if !cv.budget.work(self.saved.len() as u64 / 4 + 1) {
                    return;
                }
                if let Some(i) = d.u32(0)
                    && let Some(at) = self.saved.iter().rposition(|(k, _)| *k == i)
                    && let Some((_, state)) = self.saved.get(at).cloned()
                {
                    self.state = state;
                    self.saved.truncate(at);
                }
            }
            0x402A => {
                if let Some(m) = d.xform(0).map(Xf::from_array).filter(Xf::is_usable) {
                    self.state.world = m;
                }
            }
            0x402B => self.state.world = Xf::IDENTITY,
            0x402C..=0x402F => self.multiply(typ, flags, d),
            0x4030 => {
                if let Some(s) = d.f32(0).map(f64::from).filter(|s| *s > 0.0) {
                    self.state.unit = (flags & 0xFF) as u8;
                    self.state.scale = s;
                }
            }
            // The two records of other numbers in the range are not state records.
            _ => self.fail(|| format!("EMF+ record type {typ:#06x}")),
        }
    }

    /// The four records that put a matrix before or after the world transform: bit `0x2000` asks for it last.
    fn multiply(&mut self, typ: u16, flags: u16, d: Bytes) {
        let m = match typ {
            0x402C => d.xform(0).map(Xf::from_array),
            0x402D => d.f64_f32(0).zip(d.f64_f32(4)).map(|(x, y)| Xf::translate(x, y)),
            0x402E => d.f64_f32(0).zip(d.f64_f32(4)).map(|(x, y)| Xf::scale(x, y)),
            _ => d.f64_f32(0).map(Xf::rotate_degrees),
        };
        let Some(m) = m.filter(Xf::is_usable) else { return };
        let world = if flags & 0x2000 != 0 { self.state.world.then(m) } else { m.then(self.state.world) };
        if world.is_usable() {
            self.state.world = world;
        }
    }

    // ---- clipping ----

    fn clip_record(&mut self, typ: u16, flags: u16, d: Bytes, cv: &mut Canvas) {
        let mode = match (flags >> 8) & 0xF {
            0 => Mode::Replace,
            1 => Mode::And,
            2 => Mode::Or,
            3 => Mode::Xor,
            4 => Mode::Exclude,
            5 => Mode::Complement,
            _ => return,
        };
        let xf = self.to_frame();
        let id = usize::from(flags & 0xFF);
        let built: Result<Clip, ClipError> = match typ {
            0x4032 => d.rect_f(0).and_then(|r| clip::region_of_rects(&[rect_ltrb(r)], &xf)).ok_or(ClipError::Cannot),
            0x4033 => {
                let Some(ops) = self.path_object(id) else {
                    self.fail(|| "a clip by a path that is not there".to_owned());
                    return;
                };
                // The path is charged before it is moved, as a fill's is, and a path of more points than a clip may hold is not made one.
                if !cv.budget.work(ops.len() as u64 + 1) {
                    return;
                }
                if geom::point_count(&ops) > MAX_CLIP_POINTS {
                    cv.budget.note_cap("clip size");
                    return;
                }
                crate::work::clip_points_moved(ops.len());
                clip::part_of(&geom::transformed(&ops, &xf), true).map(clip::single).ok_or(ClipError::Cannot)
            }
            _ => match self.objects.get(id) {
                Some(Some(Object::Region(node, cost))) => {
                    let (node, cost) = (Rc::clone(node), *cost);
                    if !cv.budget.work(cost) {
                        return;
                    }
                    eval_region(&node, &xf, 0)
                }
                Some(Some(Object::Bad(why))) => {
                    let why = *why;
                    self.fail(|| format!("a region object that cannot be played: {why}"));
                    return;
                }
                _ => return,
            },
        };
        let new = match clip::capped(cv, built) {
            Ok(new) => new,
            Err(ClipError::Cannot) => {
                self.fail(|| format!("a clip of EMF+ record {typ:#06x} that cannot be read"));
                return;
            }
            Err(ClipError::TooBig) => return,
        };
        // The parts that are copied are charged as work, the points the result keeps as points: a clip changed again and again is
        // not free.
        if !cv.budget.work(self.state.clip.points().saturating_add(new.points()).saturating_add(1)) {
            return;
        }
        match clip::capped(cv, clip::combine(&self.state.clip, &new, mode)) {
            Ok(c) => {
                if cv.budget.points(c.points()) {
                    self.state.clip = c;
                }
            }
            Err(ClipError::Cannot) => self.fail(|| "a clip combination this crate cannot make".to_owned()),
            Err(ClipError::TooBig) => {}
        }
    }

    fn path_object(&mut self, id: usize) -> Option<Rc<Vec<Op>>> {
        match self.objects.get(id) {
            Some(Some(Object::Path(p))) => Some(Rc::clone(p)),
            Some(Some(Object::Bad(why))) => {
                let why = *why;
                self.fail(|| format!("a path object that cannot be played: {why}"));
                None
            }
            _ => None,
        }
    }

    // ---- objects ----

    fn object(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) {
        // The id is the low byte and the kind the next seven bits; an id over 63 skips the record.
        let id = usize::from(flags & 0xFF);
        let kind = ((flags >> 8) & 0x7F) as u8;
        if id >= MAX_PLUS_OBJECTS {
            return;
        }
        if flags & 0x8000 != 0 {
            self.piece(id, kind, d, cv);
            return;
        }
        // A piece without the continue bit ends an open object of the same id and type (project rule).
        if let Some(mut p) = self.partial.take()
            && p.id == id
            && p.kind == kind
        {
            let room = p.total.saturating_sub(p.data.len());
            p.data.extend_from_slice(d.0.get(..d.0.len().min(room)).unwrap_or(&[]));
            self.finish_object(id, kind, &p.data, cv);
            return;
        }
        self.finish_object(id, kind, d.0, cv);
    }

    /// A piece of an object split over records: the first four data bytes are the object's total size, the rest are its bytes.
    fn piece(&mut self, id: usize, kind: u8, d: Bytes, cv: &mut Canvas) {
        let Some(total) = d.count(0) else { return };
        let Some(chunk) = d.0.get(4..) else { return };
        if total > MAX_PLUS_OBJECT_BYTES {
            cv.budget.note_cap("EMF+ object bytes");
            self.put(id, Object::Bad("an object past the size cap"));
            self.partial = None;
            return;
        }
        let mut p = match self.partial.take() {
            Some(p) if p.id == id && p.kind == kind && p.total == total => p,
            _ => Partial { id, kind, total, data: Vec::new() },
        };
        let room = p.total.saturating_sub(p.data.len());
        p.data.extend_from_slice(chunk.get(..chunk.len().min(room)).unwrap_or(&[]));
        if !cv.budget.work(chunk.len() as u64 / 4 + 1) {
            return;
        }
        if p.data.len() >= p.total {
            self.finish_object(id, kind, &p.data, cv);
        } else {
            self.partial = Some(p);
        }
    }

    fn put(&mut self, id: usize, obj: Object) {
        if let Some(slot) = self.objects.get_mut(id) {
            *slot = Some(obj);
        }
    }

    fn finish_object(&mut self, id: usize, kind: u8, bytes: &[u8], cv: &mut Canvas) {
        let b = Bytes(bytes);
        if !cv.budget.work(b.len() as u64 / 4 + 1) {
            return;
        }
        // No path, in any object, may have more points than are left to keep: it is refused before a vector is made of it.
        let limit = cv.budget.points_left();
        let obj = match kind {
            1 => parse_brush(b, 0, limit).map(|(br, _)| Object::Brush(Rc::new(br))).unwrap_or(Object::Bad("a brush that does not parse")),
            2 => parse_pen(b, limit).map(|p| Object::Pen(Rc::new(p))).unwrap_or(Object::Bad("a pen that does not parse")),
            3 => match parse_path(b, limit) {
                // A path object is held until another takes its place: its points are charged.
                Some(p) if cv.budget.points(geom::point_count(&p)) => {
                    crate::work::object_points(geom::point_count(&p));
                    Object::Path(Rc::new(p))
                }
                Some(_) => Object::Bad("a path past the points left"),
                None => Object::Bad("a path that does not parse"),
            },
            4 => match parse_region(b, limit) {
                Some((n, cost)) if cv.budget.points(cost) => Object::Region(Rc::new(n), cost),
                Some(_) => Object::Bad("a region past the points left"),
                None => Object::Bad("a region that does not parse"),
            },
            5 => match parse_image(b, cv) {
                Ok(m) => Object::Image(Rc::new(m)),
                Err(why) => Object::Bad(why),
            },
            6 => parse_font(b).map(|f| Object::Font(Rc::new(f))).unwrap_or(Object::Bad("a font that does not parse")),
            _ => Object::Unused,
        };
        self.put(id, obj);
    }

    // ---- drawing ----

    fn draw(&mut self, typ: u16, flags: u16, d: Bytes, cv: &mut Canvas) {
        let ok = match typ {
            0x400A => self.fill_rects(flags, d, cv),
            0x4014 => self.fill_path(flags, d, cv),
            0x4015 => self.draw_path(flags, d, cv),
            0x4036 => self.driver_string(flags, d, cv),
            _ => self.draw_image(flags, d, cv),
        };
        if ok.is_none() {
            self.fail(|| format!("EMF+ record type {typ:#06x} that cannot be played"));
        }
    }

    /// The brush a fill record names: its own colour when the record's bit `0x8000` is set, else an object. `None` is a brush that
    /// cannot be played.
    fn brush(&mut self, flags: u16, word: u32) -> Option<Rc<Brush>> {
        if flags & 0x8000 != 0 {
            return Some(Rc::new(Brush::Solid(argb(word))));
        }
        match self.objects.get((word & 0xFF) as usize) {
            Some(Some(Object::Brush(b))) => Some(Rc::clone(b)),
            _ => None,
        }
    }

    fn paint(brush: &Brush, xf: &Xf) -> Option<Paint> {
        match brush {
            Brush::Solid(c) => Some(Paint::Solid(*c)),
            Brush::Linear { rect, stops, transform } => {
                // The gradient runs across the rectangle's width in brush space.
                let to = transform.then(*xf);
                let line = LinearPaint { start: [rect[0], rect[1]], end: [rect[0] + rect[2], rect[1]], stops: stops.clone() };
                if rect[2] <= 0.0 {
                    return stops.first().map(|s| Paint::Solid(s.1));
                }
                Some(Paint::Linear(Arc::new(linear_through(&to, &line))))
            }
            Brush::Path { center, radii, stops, transform } => {
                let to = transform.then(*xf);
                let c = to.apply(*center);
                let right = to.apply_vector([radii[0], 0.0]);
                let up = to.apply_vector([0.0, radii[1]]);
                Some(Paint::Radial(Arc::new(RadialPaint { center: c, basis: [right[0], right[1], up[0], up[1]], stops: stops.clone() })))
            }
            Brush::Other => None,
        }
    }

    fn fill_rects(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) -> Option<()> {
        let brush = self.brush(flags, d.u32(0)?)?;
        let n = d.count(4)?;
        let small = flags & 0x4000 != 0;
        let each = if small { 8 } else { 16 };
        // The count is checked against the record's bytes before anything is made from it.
        let raw = d.slice(8, n.checked_mul(each)?)?;
        // Five points for each rectangle, counted against what is left to keep before they are made.
        if !cv.budget.has_points((n as u64).saturating_mul(5)) {
            return Some(());
        }
        let xf = self.to_frame();
        let paint = Self::paint(&brush, &xf)?;
        if !cv.budget.work(n as u64 + 1) {
            return Some(());
        }
        let mut ops = Vec::with_capacity(n * 5);
        for c in raw.chunks_exact(each) {
            let r = Bytes(c);
            let rect = if small {
                [f64::from(r.i16(0)?), f64::from(r.i16(2)?), f64::from(r.i16(0)?) + f64::from(r.i16(4)?), f64::from(r.i16(2)?) + f64::from(r.i16(6)?)]
            } else {
                rect_ltrb(r.rect_f(0)?)
            };
            ops.extend(geom::transformed(&geom::rect(rect[0], rect[1], rect[2], rect[3]), &xf));
        }
        self.emit(cv, ops, Some(Fill { paint, even_odd: false }), None);
        Some(())
    }

    fn fill_path(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) -> Option<()> {
        let brush = self.brush(flags, d.u32(0)?)?;
        let ops = self.path_object(usize::from(flags & 0xFF))?;
        let xf = self.to_frame();
        let paint = Self::paint(&brush, &xf)?;
        if !cv.budget.work(ops.len() as u64 + 1) || !cv.budget.has_points(ops.len() as u64) {
            return Some(());
        }
        // The fill rule of a path is not documented; even-odd by E6.
        self.emit(cv, geom::transformed(&ops, &xf), Some(Fill { paint, even_odd: true }), None);
        Some(())
    }

    fn draw_path(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) -> Option<()> {
        let pen = match self.objects.get((d.u32(0)? & 0xFF) as usize) {
            Some(Some(Object::Pen(p))) => Rc::clone(p),
            _ => return None,
        };
        let ops = self.path_object(usize::from(flags & 0xFF))?;
        let xf = self.to_frame();
        let stroke = stroke_of(&pen, &xf)?;
        if !cv.budget.work(ops.len() as u64 + 1) || !cv.budget.has_points(ops.len() as u64) {
            return Some(());
        }
        self.emit(cv, geom::transformed(&ops, &xf), None, Some(stroke));
        Some(())
    }

    fn emit(&mut self, cv: &mut Canvas, ops: Vec<Op>, fill: Option<Fill>, stroke: Option<Stroke>) {
        let points = geom::point_count(&ops);
        let Some(path) = clip::path_of(&ops) else { return };
        cv.emit(&self.state.clip, Drawn::Path(Box::new(PathItem { path, fill, stroke })), points);
    }

    /// `DrawDriverString`: glyph codes with a position each, in a font and a colour.
    fn driver_string(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) -> Option<()> {
        let brush = self.brush(flags, d.u32(0)?)?;
        let options = d.u32(4)?;
        let has_matrix = d.u32(8)? != 0;
        let n = d.count(12)?;
        if n == 0 || n > crate::limits::MAX_TEXT_CHARS {
            return Some(());
        }
        let font = match self.objects.get(usize::from(flags & 0xFF)) {
            Some(Some(Object::Font(f))) => Rc::clone(f),
            _ => return None,
        };
        let units = d.units(16, n)?;
        // Option 0x4: only the first position is given and the letters follow by the font's widths.
        let positions = if options & 0x4 != 0 { 1 } else { n };
        let raw = d.slice(16 + n * 2, positions.checked_mul(8)?)?;
        let matrix = if has_matrix { Xf::from_array(d.xform(16 + n * 2 + positions * 8)?) } else { Xf::IDENTITY };
        if options & 0x1 == 0 {
            cv.note(Detail::GlyphIndexText, || "an EMF+ string of glyph indices".to_owned());
            return Some(());
        }
        let xf = matrix.then(self.to_frame());
        let at: Vec<P> = raw.as_chunks::<8>().0.iter().map(|c| xf.apply([f64::from(f32_at(c, 0)), f64::from(f32_at(c, 4))])).collect();
        let chars = decode_utf16(&units);
        let placement = if positions == 1 {
            Placement::Flow { origin: at.first().copied()?, h_align: HAlign::Left }
        } else if at.len() == chars.len() {
            Placement::Origins(at)
        } else {
            // A surrogate pair is two codes and one character: the positions of the second are dropped.
            let keep: Vec<P> = merge_positions(&units, &at);
            Placement::Origins(keep)
        };
        let color = match Self::paint(&brush, &xf)? {
            Paint::Solid(c) => c,
            Paint::Linear(l) => l.stops.first().map_or([0, 0, 0, 255], |s| s.1),
            Paint::Radial(r) => r.stops.first().map_or([0, 0, 0, 255], |s| s.1),
        };
        // The world y runs down, so the letters' up is -y; the frame vectors follow the whole transform.
        let right = xf.apply_vector([1.0, 0.0]);
        let up = xf.apply_vector([0.0, -1.0]);
        let item = TextItem {
            text: chars.iter().collect(),
            font: Font {
                family: font.family.clone(),
                bold: font.style & 1 != 0,
                italic: font.style & 2 != 0,
                underline: font.style & 4 != 0,
                strike_out: font.style & 8 != 0,
                symbol: false,
            },
            color,
            size: font.em,
            cell: false,
            basis: [right[0], right[1], up[0], up[1]],
            placement,
            v_align: VAlign::Baseline,
            background: None,
        };
        cv.emit(&self.state.clip, Drawn::Text(Box::new(item)), chars.len() as u64);
        Some(())
    }

    /// `DrawImagePoints`: an image or a metafile drawn from a source rectangle into the parallelogram of three points.
    fn draw_image(&mut self, flags: u16, d: Bytes, cv: &mut Canvas) -> Option<()> {
        let image = match self.objects.get(usize::from(flags & 0xFF)) {
            Some(Some(Object::Image(m))) => Rc::clone(m),
            Some(Some(Object::Bad(_))) => return None,
            _ => return None,
        };
        let src = d.rect_f(8)?;
        let n = d.count(24)?;
        if n != 3 {
            return None;
        }
        let small = flags & 0x4000 != 0;
        let pts: Vec<P> = if small {
            (0..3).map(|i| Some([f64::from(d.i16(28 + i * 4)?), f64::from(d.i16(30 + i * 4)?)])).collect::<Option<_>>()?
        } else {
            (0..3).map(|i| Some([d.f64_f32(28 + i * 8)?, d.f64_f32(32 + i * 8)?])).collect::<Option<_>>()?
        };
        let xf = self.to_frame();
        let quad = [xf.apply(*pts.first()?), xf.apply(*pts.get(1)?), xf.apply(*pts.get(2)?)];
        let src = [src[0], src[1], src[2], src[3]];
        match &*image {
            Meta::Bitmap(img) => {
                let item = BitmapItem { image: img.clone(), src: Some(src), quad };
                cv.emit(&self.state.clip, Drawn::Bitmap(Box::new(item)), 0);
                Some(())
            }
            Meta::Metafile(bytes) => self.nested(bytes, src, quad, cv),
        }
    }

    /// A metafile inside the picture: its pixels map one to one into the source rectangle, which lands on the parallelogram. The
    /// metafile shares this picture's budget and counts one deeper.
    fn nested(&mut self, bytes: &Arc<Vec<u8>>, src: [f64; 4], quad: [P; 3], cv: &mut Canvas) -> Option<()> {
        if self.depth + 1 > MAX_NESTING {
            cv.budget.note_cap("nested metafiles");
            return Some(());
        }
        if !(src[2] > 0.0 && src[3] > 0.0) {
            return None;
        }
        // Each draw is charged the metafile's size, whether or not its records are read: bytes after a record that ends the file cost
        // nothing to skip, and a draw of them a million times would otherwise be free. The bytes are read where they are (a placeable
        // WMF's own reader finds its standard header 22 or 24 bytes in): nothing is copied.
        if !cv.budget.work(bytes.len() as u64 / 4 + 1) {
            return Some(());
        }
        let Ok(inner) = crate::parse_with(bytes, cv.budget, self.depth + 1) else { return None };
        let [p0, p1, p2] = quad;
        let (a, b) = ((p1[0] - p0[0]) / src[2], (p1[1] - p0[1]) / src[2]);
        let (c, dd) = ((p2[0] - p0[0]) / src[3], (p2[1] - p0[1]) / src[3]);
        let xf = Xf { a, b, c, d: dd, e: p0[0] - (src[0] * a + src[1] * c), f: p0[1] - (src[0] * b + src[1] * dd) };
        if !xf.is_usable() {
            return Some(());
        }
        // The draw covers the parallelogram and nothing else.
        let p3 = [p1[0] + p2[0] - p0[0], p1[1] + p2[1] - p0[1]];
        let cover = clip::part_of(&[Op::M(p0), Op::L(p1), Op::L(p3), Op::L(p2), Op::Z], false).map(clip::single);
        if !nest::merge(cv, inner, &xf, &self.state.clip, cover.as_ref()) {
            cv.budget.note_cap("items");
        }
        Some(())
    }
}

fn f32_at(c: &[u8], at: usize) -> f32 {
    c.get(at..at + 4).and_then(|s| <[u8; 4]>::try_from(s).ok()).map_or(0.0, f32::from_le_bytes)
}

/// The positions of the characters of a string whose code units are in pairs for the characters outside the basic plane.
fn merge_positions(units: &[u16], at: &[P]) -> Vec<P> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = at.get(i) {
        out.push(*p);
        let pair = units.get(i).is_some_and(|u| (0xD800..0xDC00).contains(u)) && units.get(i + 1).is_some_and(|u| (0xDC00..0xE000).contains(u));
        i += if pair { 2 } else { 1 };
    }
    out
}

/// A rectangle `(x, y, width, height)` as left, top, right, bottom.
fn rect_ltrb(r: [f64; 4]) -> [f64; 4] {
    [r[0], r[1], r[0] + r[2], r[1] + r[3]]
}

trait RectF {
    fn rect_f(&self, at: usize) -> Option<[f64; 4]>;
}

impl RectF for Bytes<'_> {
    /// The `RectF` at `at`: x, y, width, height.
    fn rect_f(&self, at: usize) -> Option<[f64; 4]> {
        Some([self.f64_f32(at)?, self.f64_f32(at + 4)?, self.f64_f32(at + 8)?, self.f64_f32(at + 12)?])
    }
}

/// A linear gradient given in some space, taken through `xf` (see `nest::linear`).
fn linear_through(xf: &Xf, l: &LinearPaint) -> LinearPaint {
    match nest_paint(xf, &Paint::Linear(Arc::new(l.clone()))) {
        Paint::Linear(out) => (*out).clone(),
        _ => l.clone(),
    }
}

fn nest_paint(xf: &Xf, p: &Paint) -> Paint {
    let item = crate::model::Item {
        clip: 0,
        drawn: Drawn::Path(Box::new(PathItem {
            path: crate::model::Path::default(),
            fill: Some(Fill { paint: p.clone(), even_odd: false }),
            stroke: None,
        })),
    };
    match nest::item(xf, &item).drawn {
        Drawn::Path(path) => path.fill.map_or_else(|| p.clone(), |f| f.paint),
        _ => p.clone(),
    }
}

/// The stroke of a pen, through `xf`: the width is in world units, a width of 0 is one pixel, and dashes are in widths of the pen.
fn stroke_of(pen: &Pen, xf: &Xf) -> Option<Stroke> {
    let paint = Plus::paint(&pen.brush, xf)?;
    let width = if pen.width <= 0.0 { 1.0 } else { pen.width * xf.mean_scale() };
    if !width.is_finite() {
        return None;
    }
    let unit = width.max(1.0);
    let dashes: Vec<f64> = pen.dashes.iter().map(|d| d * unit).collect();
    let dashes = if dashes.iter().all(|d| d.is_finite() && *d > 0.0) { dashes } else { Vec::new() };
    Some(Stroke { paint, width, cap: pen.cap, join: pen.join, miter_limit: pen.miter.max(1.0), dashes, dash_offset: pen.dash_offset * unit })
}

// ---- reading objects ----

/// Colour stops: an offset and a colour each.
type Stops = Arc<[(f64, [u8; 4])]>;

fn colors_stops(start: [u8; 4], end: [u8; 4], b: Bytes, flags: u32, at: usize) -> Option<(Stops, usize)> {
    let mut at = at;
    let stops: Vec<(f64, [u8; 4])> = if flags & 0x4 != 0 {
        // Preset colours: a count, positions, then colours.
        let n = b.count(at)?;
        if n == 0 || n > 4096 || at.checked_add(4)?.checked_add(n.checked_mul(8)?)? > b.len() {
            return None;
        }
        let pos: Vec<f64> = (0..n).map(|i| b.f64_f32(at + 4 + i * 4)).collect::<Option<_>>()?;
        let cols: Vec<[u8; 4]> = (0..n).map(|i| b.u32(at + 4 + n * 4 + i * 4).map(argb)).collect::<Option<_>>()?;
        at += 4 + n * 8;
        pos.into_iter().zip(cols).collect()
    } else if flags & 0x8 != 0 {
        // Blend factors: a count, positions, then a factor for each between the two colours.
        let n = b.count(at)?;
        if n == 0 || n > 4096 || at.checked_add(4)?.checked_add(n.checked_mul(8)?)? > b.len() {
            return None;
        }
        let pos: Vec<f64> = (0..n).map(|i| b.f64_f32(at + 4 + i * 4)).collect::<Option<_>>()?;
        let fac: Vec<f64> = (0..n).map(|i| b.f64_f32(at + 4 + n * 4 + i * 4)).collect::<Option<_>>()?;
        at += 4 + n * 8;
        pos.into_iter().zip(fac).map(|(p, f)| (p, lerp(start, end, f))).collect()
    } else {
        vec![(0.0, start), (1.0, end)]
    };
    Some((Arc::from(stops), at))
}

/// A brush object and the offset after it. `at` is where it starts in `b`.
fn parse_brush(b: Bytes, at: usize, limit: u64) -> Option<(Brush, usize)> {
    let kind = b.u32(at + 4)?;
    let d = at + 8;
    match kind {
        0 => Some((Brush::Solid(argb(b.u32(d)?)), d + 4)),
        1 => Some((Brush::Other, d + 12)),
        2 => Some((Brush::Other, b.len())),
        3 => parse_path_gradient(b, d, limit),
        4 => parse_linear_gradient(b, d),
        _ => None,
    }
}

fn parse_linear_gradient(b: Bytes, d: usize) -> Option<(Brush, usize)> {
    let flags = b.u32(d)?;
    let rect = [b.f64_f32(d + 8)?, b.f64_f32(d + 12)?, b.f64_f32(d + 16)?, b.f64_f32(d + 20)?];
    let (start, end) = (argb(b.u32(d + 24)?), argb(b.u32(d + 28)?));
    let mut at = d + 40;
    let transform = if flags & 0x2 != 0 {
        let t = Xf::from_array(b.xform(at)?);
        at += 24;
        t
    } else {
        Xf::IDENTITY
    };
    let (stops, at) = colors_stops(start, end, b, flags, at)?;
    Some((Brush::Linear { rect, stops, transform: if transform.is_usable() { transform } else { Xf::IDENTITY } }, at))
}

/// A path gradient is played as a radial one: the centre colour at the centre point and the first surrounding colour at the boundary's
/// extent (project rule: R23 counts path gradients in 2,648 parts, and a radial fill is the nearest a target draws).
fn parse_path_gradient(b: Bytes, d: usize, limit: u64) -> Option<(Brush, usize)> {
    let flags = b.u32(d)?;
    let center_color = argb(b.u32(d + 8)?);
    let center = [b.f64_f32(d + 12)?, b.f64_f32(d + 16)?];
    let n = b.count(d + 20)?;
    if n == 0 || n > 4096 || d.checked_add(24)?.checked_add(n.checked_mul(4)?)? > b.len() {
        return None;
    }
    let surround = argb(b.u32(d + 24)?);
    let mut at = d + 24 + n * 4;
    // The boundary: a path (flag 0x1) or a list of points.
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut grow = |p: P| {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    if flags & 0x1 != 0 {
        let size = b.count(at)?;
        let ops = parse_path(b.sub(at + 4, size)?, limit)?;
        for op in &ops {
            match *op {
                Op::M(p) | Op::L(p) => grow(p),
                Op::C(a, c, e) => {
                    grow(a);
                    grow(c);
                    grow(e);
                }
                Op::Z => {}
            }
        }
        at = at.checked_add(4)?.checked_add(size)?;
    } else {
        let count = b.count(at)?;
        let raw = b.slice(at + 4, count.checked_mul(8)?)?;
        for c in raw.as_chunks::<8>().0 {
            grow([f64::from(f32_at(c, 0)), f64::from(f32_at(c, 4))]);
        }
        at += 4 + count * 8;
    }
    let transform = if flags & 0x2 != 0 {
        let t = Xf::from_array(b.xform(at)?);
        at += 24;
        t
    } else {
        Xf::IDENTITY
    };
    let radii = [((hi[0] - lo[0]) / 2.0).max(1e-3), ((hi[1] - lo[1]) / 2.0).max(1e-3)];
    let radii = if radii.iter().all(|r| r.is_finite()) { radii } else { [1.0, 1.0] };
    // The boundary's colour is the end of the ramp and the centre's the start.
    Some((
        Brush::Path {
            center,
            radii,
            stops: Arc::from(vec![(0.0, center_color), (1.0, surround)]),
            transform: if transform.is_usable() { transform } else { Xf::IDENTITY },
        },
        at,
    ))
}

/// A cursor over the optional data of an object: each piece is taken only when it is all there.
struct Cursor {
    at: usize,
    len: usize,
}

impl Cursor {
    /// The offset of the next `n` bytes, which are then passed.
    fn take(&mut self, n: usize) -> Option<usize> {
        let here = self.at;
        let end = here.checked_add(n)?;
        if end > self.len {
            return None;
        }
        self.at = end;
        Some(here)
    }

    /// As `take`, only when `flags` has `bit`.
    fn take_if(&mut self, flags: u32, bit: u32, n: usize) -> Option<Option<usize>> {
        if flags & bit == 0 { Some(None) } else { self.take(n).map(Some) }
    }
}

fn parse_pen(b: Bytes, limit: u64) -> Option<Pen> {
    if b.u32(4)? != 0 {
        return None;
    }
    let flags = b.u32(8)?;
    let width = b.f64_f32(16)?;
    let mut cur = Cursor { at: 20, len: b.len() };
    cur.take_if(flags, 0x1, 24)?;
    let start_cap = cur.take_if(flags, 0x2, 4)?.and_then(|o| b.i32(o));
    cur.take_if(flags, 0x4, 4)?;
    let join = cur.take_if(flags, 0x8, 4)?.and_then(|o| b.i32(o));
    let miter = cur.take_if(flags, 0x10, 4)?.and_then(|o| b.f64_f32(o));
    let style = cur.take_if(flags, 0x20, 4)?.and_then(|o| b.i32(o));
    cur.take_if(flags, 0x40, 4)?;
    let offset = cur.take_if(flags, 0x80, 4)?.and_then(|o| b.f64_f32(o));
    let mut custom: Vec<f64> = Vec::new();
    if flags & 0x100 != 0 {
        let n = b.count(cur.take(4)?)?;
        if n > 4096 {
            return None;
        }
        let at = cur.take(n.checked_mul(4)?)?;
        custom = b.slice(at, n * 4)?.as_chunks::<4>().0.iter().map(|c| f64::from(f32_at(c, 0))).collect();
    }
    cur.take_if(flags, 0x200, 4)?;
    if flags & 0x400 != 0 {
        let n = b.count(cur.take(4)?)?;
        cur.take(n.checked_mul(4)?)?;
    }
    for bit in [0x800u32, 0x1000] {
        if flags & bit != 0 {
            let n = b.count(cur.take(4)?)?;
            cur.take(n)?;
        }
    }
    let (brush, _) = parse_brush(b, cur.at, limit)?;
    let dashes = match style {
        Some(1) => vec![3.0, 1.0],
        Some(2) => vec![1.0, 1.0],
        Some(3) => vec![3.0, 1.0, 1.0, 1.0],
        Some(4) => vec![3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        Some(5) => custom,
        _ => Vec::new(),
    };
    let cap = match start_cap {
        Some(1) => Cap::Square,
        Some(2) => Cap::Round,
        _ => Cap::Butt,
    };
    let join = match join {
        Some(1) => Join::Bevel,
        Some(2) => Join::Round,
        _ => Join::Miter,
    };
    Some(Pen { width, brush, cap, join, miter: miter.unwrap_or(10.0), dashes, dash_offset: offset.unwrap_or(0.0) })
}

/// A path object: float points, or 16-bit points when flag `0x4000` is set; a type for each point; fits the object's bytes under
/// that reading or is skipped.
fn parse_path(b: Bytes, limit: u64) -> Option<Vec<Op>> {
    let n = b.count(4)?;
    // A path of more points than are left to keep is not made; the count is checked before the vector for it is.
    if n as u64 > limit {
        return None;
    }
    let flags = b.u32(8)?;
    let small = flags & 0x4000 != 0;
    let each = if small { 4 } else { 8 };
    let pts_len = n.checked_mul(each)?;
    let raw = b.slice(12, pts_len)?;
    let types = b.slice(12 + pts_len, n)?;
    let point = |i: usize| -> Option<P> {
        let c = raw.get(i * each..(i + 1) * each)?;
        if small {
            Some([f64::from(i16::from_le_bytes([c[0], c[1]])), f64::from(i16::from_le_bytes([c[2], c[3]]))])
        } else {
            let p = [f64::from(f32_at(c, 0)), f64::from(f32_at(c, 4))];
            (p[0].is_finite() && p[1].is_finite()).then_some(p)
        }
    };
    let mut ops = Vec::with_capacity(n + 4);
    let mut i = 0;
    while i < n {
        let t = *types.get(i)?;
        let p = point(i)?;
        match t & 0x0F {
            0 => ops.push(Op::M(p)),
            1 => ops.push(Op::L(p)),
            3 => {
                let (c2, e) = (point(i + 1)?, point(i + 2)?);
                ops.push(Op::C(p, c2, e));
                i += 2;
            }
            _ => return None,
        }
        if types.get(i)? & 0x80 != 0 {
            ops.push(Op::Z);
        }
        i += 1;
    }
    Some(ops)
}

fn parse_region(b: Bytes, limit: u64) -> Option<(Node, u64)> {
    let count = b.count(4)?;
    // Each node is at least four bytes: the count is checked against the object before the tree is read.
    if count == 0 || count.checked_mul(4)? > b.len() {
        return None;
    }
    let mut walk = RegionWalk { at: 8, seen: 0, count, cost: 0, limit };
    let node = read_node(b, &mut walk, 0)?;
    Some((node, walk.cost))
}

/// Where the reading of a region has got to, and what the tree it makes costs to evaluate.
struct RegionWalk {
    at: usize,
    seen: usize,
    count: usize,
    /// The nodes and the points of their paths: charged as work each time the region is used.
    cost: u64,
    limit: u64,
}

fn read_node(b: Bytes, w: &mut RegionWalk, depth: usize) -> Option<Node> {
    if depth > MAX_REGION_DEPTH {
        return None;
    }
    w.seen += 1;
    w.cost += 1;
    if w.seen > w.count {
        return None;
    }
    let kind = b.u32(w.at)?;
    w.at += 4;
    let mode = match kind {
        1 => Some(Mode::And),
        2 => Some(Mode::Or),
        3 => Some(Mode::Xor),
        4 => Some(Mode::Exclude),
        5 => Some(Mode::Complement),
        _ => None,
    };
    if let Some(mode) = mode {
        let left = read_node(b, w, depth + 1)?;
        let right = read_node(b, w, depth + 1)?;
        return Some(Node::Combine(mode, Box::new(left), Box::new(right)));
    }
    match kind {
        0x1000_0000 => {
            let r = b.rect_f(w.at)?;
            w.at += 16;
            Some(Node::Rect(rect_ltrb(r)))
        }
        0x1000_0001 => {
            let size = b.count(w.at)?;
            let ops = parse_path(b.sub(w.at + 4, size)?, w.limit)?;
            w.at = w.at.checked_add(4)?.checked_add(size)?;
            w.cost = w.cost.saturating_add(ops.len() as u64);
            Some(Node::Path(Rc::new(ops)))
        }
        0x1000_0002 => Some(Node::Empty),
        0x1000_0003 => Some(Node::Infinite),
        _ => None,
    }
}

/// A region as a clip in frame coordinates, by the nodes' combinations. The caller has charged `Object::Region`'s cost.
fn eval_region(node: &Node, xf: &Xf, depth: usize) -> Result<Clip, ClipError> {
    if depth > MAX_REGION_DEPTH {
        return Err(ClipError::Cannot);
    }
    match node {
        Node::Rect(r) => clip::region_of_rects(&[*r], xf).ok_or(ClipError::Cannot),
        Node::Path(ops) => {
            if geom::point_count(ops) > MAX_CLIP_POINTS {
                return Err(ClipError::TooBig);
            }
            clip::part_of(&geom::transformed(ops, xf), true).map(clip::single).ok_or(ClipError::Cannot)
        }
        Node::Empty => Ok(clip::nothing()),
        Node::Infinite => Ok(clip::unbounded()),
        Node::Combine(mode, l, r) => {
            let (l, r) = (eval_region(l, xf, depth + 1)?, eval_region(r, xf, depth + 1)?);
            clip::combine(&l, &r, *mode)
        }
    }
}

fn parse_font(b: Bytes) -> Option<FontObject> {
    let em = b.f64_f32(4)?;
    let style = b.u32(12)?;
    let n = b.count(20)?;
    if n > 256 {
        return None;
    }
    let units = b.units(24, n)?;
    let family: Arc<str> = Arc::from(decode_utf16(&units).into_iter().take_while(|c| *c != '\0').collect::<String>());
    (em > 0.0).then_some(FontObject { em, style, family })
}

/// An image object: a bitmap, raw or compressed, or a whole metafile.
fn parse_image(b: Bytes, cv: &mut Canvas) -> Result<Meta, &'static str> {
    let kind = b.u32(4).ok_or("an image that is cut short")?;
    match kind {
        1 => {
            let (w, h, stride) = (
                b.i32(8).ok_or("a bitmap that is cut short")?,
                b.i32(12).ok_or("a bitmap that is cut short")?,
                b.i32(16).ok_or("a bitmap that is cut short")?,
            );
            let (format, compressed) = (b.u32(20).ok_or("a bitmap that is cut short")?, b.u32(24).ok_or("a bitmap that is cut short")? == 1);
            let data = b.0.get(28..).ok_or("a bitmap that is cut short")?;
            if compressed {
                return known_image(data);
            }
            raw_bitmap(w, h, stride, format, data, cv)
        }
        2 => {
            let size = b.count(12).ok_or("a metafile image that is cut short")?;
            let data = b.slice(16, size).ok_or("a metafile image that is cut short")?;
            if crate::sniff(data).is_none() && data.get(..4) != Some(&[0xD7, 0xCD, 0xC6, 0x9A][..]) {
                return Err("a metafile image of no known format");
            }
            Ok(Meta::Metafile(Arc::new(data.to_vec())))
        }
        _ => Err("an image of an unknown kind"),
    }
}

/// A compressed image: its format is read from its first bytes, and `render` decodes it. A file of no known image format is refused.
fn known_image(data: &[u8]) -> Result<Meta, &'static str> {
    let known = data.starts_with(&[0x89, b'P', b'N', b'G'])
        || data.starts_with(&[0xFF, 0xD8])
        || data.starts_with(b"GIF8")
        || data.starts_with(b"BM")
        || data.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || data.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]);
    if known && data.len() <= MAX_PLUS_OBJECT_BYTES {
        Ok(Meta::Bitmap(Image::Encoded(Arc::new(data.to_vec()))))
    } else {
        Err("an image of no known format")
    }
}

/// A bitmap of raw pixels in one of the four formats files use: 32 bits with alpha, with premultiplied alpha, without, and 24 bits.
fn raw_bitmap(w: i32, h: i32, stride: i32, format: u32, data: &[u8], cv: &mut Canvas) -> Result<Meta, &'static str> {
    let (w, h) = (u32::try_from(w).map_err(|_| "a bitmap of no size")?, u32::try_from(h).map_err(|_| "a bitmap of no size")?);
    let stride = usize::try_from(stride).map_err(|_| "a bottom-up bitmap")?;
    if w == 0 || h == 0 || w > MAX_BITMAP_SIDE || h > MAX_BITMAP_SIDE || u64::from(w) * u64::from(h) > MAX_BITMAP_PIXELS {
        cv.budget.note_cap("bitmap size");
        return Err("a bitmap past the size cap");
    }
    let bytes = match format {
        0x0026_200A | 0x000E_200B | 0x0002_2009 => 4usize,
        0x0002_1808 => 3,
        _ => return Err("a bitmap of a pixel format this crate does not read"),
    };
    let (width, rows) = (w as usize, h as usize);
    if stride < width * bytes || stride.checked_mul(rows).is_none_or(|n| n > data.len()) {
        return Err("a bitmap whose pixels are not all there");
    }
    if !cv.budget.pixels(u64::from(w) * u64::from(h)) {
        return Err("a bitmap past the size cap");
    }
    let mut rgba = vec![0u8; width * rows * 4];
    for y in 0..rows {
        let line = data.get(y * stride..y * stride + width * bytes).ok_or("a bitmap whose pixels are not all there")?;
        let out = rgba.get_mut(y * width * 4..(y + 1) * width * 4).ok_or("a bitmap whose pixels are not all there")?;
        for (px, o) in line.chunks_exact(bytes).zip(out.as_chunks_mut::<4>().0.iter_mut()) {
            let alpha = if bytes == 4 && format != 0x0002_2009 { px[3] } else { 255 };
            let (b, g, r) = (px[0], px[1], px[2]);
            let unmul = |c: u8| {
                if format == 0x000E_200B && alpha != 0 && alpha != 255 {
                    ((u32::from(c) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8
                } else {
                    c
                }
            };
            o.copy_from_slice(&[unmul(r), unmul(g), unmul(b), alpha]);
        }
    }
    let _ = dib::SRCCOPY;
    Ok(Meta::Bitmap(Image::Rgba { width: w, height: h, data: Arc::new(rgba) }))
}
