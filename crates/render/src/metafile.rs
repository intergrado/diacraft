//! Metafile pictures: the display list of `diacraft-metafile` drawn through the [`Target`] seam, so that the window, the PNG, the SVG and
//! the PDF all get the same picture.
//!
//! * **Where.** The picture's frame (device pixels, y down, origin at its top-left corner) is stretched onto the picture box, top edge to
//!   top edge, each axis on its own. The caller has pushed the cut to the frame or to `ClippingPath`; the
//!   picture adds the clips of its own items.
//! * **Items** are filled and stroked paths (solid colours and the gradients of EMF+), bitmaps (decoded once, when the picture is kept)
//!   and text. Text goes through the font database: the family is found ignoring case, the letters are placed where the file says (by the
//!   advances it gives, or by the face's own when it gives none), and sized from the em or the cell. It is written as outlines.
//! * **The report.** What the crate left out (`Picture::report`) and what is left out here (a symbol font that cannot be found, a bitmap
//!   that does not decode, the frame's time running out) become one row for the picture, naming the first detail of the list.
//! * **Cost.** The items are walked with the frame's time budget checked between small groups of them; a bitmap that is drawn by many
//!   items is decoded once; a clip is pushed once for the run of items that share it.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use diacraft_color::Rgba;
use diacraft_fonts::{Cover, Face, FontStyle};
use diacraft_metafile as mf;
use kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello_cpu::Pixmap;

use crate::frame::{Frame, ShapeCtx};
use crate::limits::{
    MAX_DASHES, MAX_GRADIENT_STOPS, MAX_METAFILE_ITEMS_PER_FRAME, MAX_PICTURE_PIXELS, MAX_STROKE_PX, MIN_DASH_PERIOD_PX, WORK_PER_DASH,
    WORK_PER_GLYPH, WORK_PER_META_ITEM,
};
use crate::picture::{Decoded, Format, check_size, decode_bmp, decode_jpeg, decode_png, header_size, pixmap_of, sniff, to_pixmap};
use crate::skips::SkipKind;
use crate::target::{Gradient, GradientKind, GradientStop, Paint, Refused, Target};

/// The most glyphs one metafile picture draws in a frame. A text run is at most 65,536 characters (the crate's cap), and each glyph is a
/// path to build and fill.
const MAX_GLYPHS_PER_PICTURE: usize = 100_000;
/// The slant of a made-up italic, as a share of the height.
const SLANT: f64 = 0.2;
/// The weight of a made-up bold, as a share of the em.
const EMBOLDEN: f64 = 0.035;
/// Items between two looks at the clock.
const ITEMS_PER_TIME_CHECK: usize = 64;
/// The most path elements of clips one metafile picture pushes in a draw. A clip of 100,000 points entered again and again by items that
/// alternate between two clips is a rasterisation of it each time; past this many elements the items under a clip are not drawn.
const MAX_CLIP_PUSH_ELEMENTS: usize = 4_000_000;

/// A bitmap decoded for drawing: premultiplied, perhaps reduced, and the size it had.
struct Bitmap {
    pixmap: Arc<Pixmap>,
    /// The size of the image in its own pixels, which source rectangles count in.
    size: (f64, f64),
}

/// Why a bitmap is not drawn.
enum NotDrawn {
    /// The bytes are not an image this renderer decodes.
    Undecodable(String),
    /// A cap held it back: the pixels a picture decodes, or the frame's time. It was not decoded.
    Cap(String),
}

/// An image not yet decoded: where its items find it, what it is, and how big its header says it is.
struct Pending {
    stub: Arc<Vec<u8>>,
    image: mf::Image,
    size: (u32, u32),
}

/// What a frame has for decoding the bitmaps of its pictures, all of them together: the time it has left, and one decode it may force when
/// the time is gone. The forced decode is what lets a picture move on in a frame that has no time (every picture would otherwise wait for
/// time that does not come); it is **one a frame**, not one a picture, so a page of many pictures costs the frame's time and one bitmap.
#[derive(Clone, Debug)]
pub(crate) struct DecodeRoom {
    pub left: Option<Duration>,
    pub force: bool,
    /// Where the time is read (the frame's clock).
    pub clock: crate::clock::Clock,
    /// The pixels the room may still decode (the frame's work left, in pixels), which each decode takes from **before** it is made: an image past
    /// what is left waits, with the images after it, as one past the time does. `u64::MAX` is a frame with no work budget.
    pub pixels: u64,
    /// Set when an image waited for want of pixels: the frame's work is spent.
    pub short: bool,
}

impl DecodeRoom {
    /// All the time there is, and the forced decode unused.
    #[cfg(test)]
    pub(crate) fn unlimited() -> DecodeRoom {
        DecodeRoom { left: None, force: true, clock: crate::clock::Clock::system(), pixels: u64::MAX, short: false }
    }

    /// No time at all, and the forced decode unused.
    #[cfg(test)]
    pub(crate) fn spent() -> DecodeRoom {
        DecodeRoom { left: Some(Duration::ZERO), force: true, clock: crate::clock::Clock::system(), pixels: u64::MAX, short: false }
    }
}

/// The most calls ([`MetaPicture::work`], one a frame) that decode a picture's bitmaps. A picture whose bitmaps need more frames' time than
/// this has the rest marked as not drawn, for a reason that is stated, and is whole after that.
pub(crate) const MAX_DECODING_FRAMES: u32 = 8;

/// A metafile read, with its bitmaps decoded, kept between frames. The parse is kept at once; the bitmaps are decoded in the time a frame
/// has, from the first not yet decoded, a frame at a time, so that a picture that needs several frames' time finishes and stays kept.
pub(crate) struct MetaPicture {
    pic: mf::Picture,
    /// The bitmaps by the address of the (emptied) data an item holds: items that draw one image share one entry.
    bitmaps: HashMap<usize, Result<Bitmap, NotDrawn>>,
    /// The images still to be decoded, in the order the picture first draws them.
    pending: VecDeque<Pending>,
    /// Pixels decoded so far, for the running total (`MAX_PICTURE_PIXELS`).
    total: u64,
    /// How many times decoding has been given a frame's time.
    frames: u32,
    /// How many bitmaps this picture has decoded to pixels, in all its frames.
    decodes: u64,
    /// What the parsed picture costs to keep, worked out once (a million items are not walked each frame).
    base_cost: u64,
}

fn key_of(image: &mf::Image) -> usize {
    match image {
        mf::Image::Rgba { data, .. } => Arc::as_ptr(data) as usize,
        mf::Image::Encoded(data) => Arc::as_ptr(data) as usize,
    }
}

/// The size of an image in pixels, from the item for raw pixels and from the header of an encoded file, without decoding anything. **A size
/// no picture could decode** (none, or more pixels than `MAX_PICTURE_PIXELS`) is refused here, as not drawn with that reason, so that it
/// never goes into the queue of images to decode or adds to the weight of the picture: a crafted header cannot name a size that overflows a
/// sum.
fn image_size(image: &mf::Image) -> Result<(u32, u32), NotDrawn> {
    let bad = |why: &str| NotDrawn::Undecodable(why.to_owned());
    let (width, height) = match image {
        mf::Image::Rgba { width, height, .. } => (*width, *height),
        mf::Image::Encoded(data) => match sniff(data) {
            Format::Png | Format::Jpeg | Format::Bmp => header_size(data).ok_or_else(|| bad("an image whose header cannot be read"))?,
            Format::Gif => return Err(bad("a GIF, which is not decoded yet")),
            Format::Tiff => return Err(bad("a TIFF, which is not decoded yet")),
            _ => return Err(bad("bytes of no picture format")),
        },
    };
    if width == 0 || height == 0 {
        return Err(bad("an image of no size"));
    }
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > MAX_PICTURE_PIXELS {
        return Err(NotDrawn::Cap(format!("an image of {width} by {height} pixels, past the {MAX_PICTURE_PIXELS} a picture decodes")));
    }
    Ok((width, height))
}

fn decode_bitmap(image: &mf::Image, (width, height): (u32, u32)) -> Result<Bitmap, NotDrawn> {
    let bad = |why: String| NotDrawn::Undecodable(why);
    check_size(width, height).map_err(bad)?;
    let size = (f64::from(width), f64::from(height));
    let decoded = match image {
        mf::Image::Rgba { data, .. } => pixmap_of(width, height, data),
        mf::Image::Encoded(data) => {
            let raw = match sniff(data) {
                Format::Png => decode_png(data, Some((width, height))),
                Format::Jpeg => decode_jpeg(data, Some((width, height))),
                Format::Bmp => decode_bmp(data, Some((width, height))),
                _ => Err("bytes of no picture format".to_owned()),
            }
            .map_err(bad)?;
            // The header and the decoder agree on the size, or the source rectangles would count in the wrong pixels.
            let size = (f64::from(raw.width), f64::from(raw.height));
            return match to_pixmap(raw) {
                Decoded::Raster(pixmap) => Ok(Bitmap { pixmap, size }),
                Decoded::Undecodable(why) => Err(NotDrawn::Undecodable(why)),
                _ => Err(NotDrawn::Undecodable("a bitmap that does not decode".to_owned())),
            };
        }
    };
    match decoded {
        Decoded::Raster(pixmap) => Ok(Bitmap { pixmap, size }),
        Decoded::Undecodable(why) => Err(NotDrawn::Undecodable(why)),
        _ => Err(NotDrawn::Undecodable("a bitmap that does not decode".to_owned())),
    }
}

impl MetaPicture {
    /// Keeps a picture: each distinct bitmap is found (by its header's size, not decoded), the bytes of the picture's items are let go, and
    /// decoding starts with `time_left`.
    pub(crate) fn new(mut pic: mf::Picture, room: &mut DecodeRoom) -> MetaPicture {
        let mut bitmaps: HashMap<usize, Result<Bitmap, NotDrawn>> = HashMap::new();
        let mut pending: VecDeque<Pending> = VecDeque::new();
        // The emptied data that stands for an image in its items, by the address of the data it replaced.
        let mut stubs: HashMap<usize, Arc<Vec<u8>>> = HashMap::new();
        for item in &mut pic.items {
            let mf::Drawn::Bitmap(b) = &mut item.drawn else { continue };
            let old = key_of(&b.image);
            let stub = match stubs.get(&old) {
                Some(stub) => Arc::clone(stub),
                None => {
                    let stub = Arc::new(Vec::new());
                    match image_size(&b.image) {
                        Err(why) => {
                            bitmaps.insert(Arc::as_ptr(&stub) as usize, Err(why));
                        }
                        Ok(size) => pending.push_back(Pending { stub: Arc::clone(&stub), image: b.image.clone(), size }),
                    }
                    stubs.insert(old, Arc::clone(&stub));
                    stub
                }
            };
            b.image = match &b.image {
                mf::Image::Rgba { width, height, .. } => mf::Image::Rgba { width: *width, height: *height, data: stub },
                mf::Image::Encoded(_) => mf::Image::Encoded(stub),
            };
        }
        let base_cost = pic.cost();
        let mut meta = MetaPicture { pic, bitmaps, pending, total: 0, frames: 0, decodes: 0, base_cost };
        meta.work(room);
        meta
    }

    /// Decodes the images not yet decoded, in order, while the room's time lasts, and one more when the frame's forced decode is unused (and
    /// then it is used): a frame with no time left still moves one picture on, so that pictures finish (one bitmap's decode is bounded by the
    /// caps). **A running total of pixels** is kept (`MAX_PICTURE_PIXELS`), and an image that would take it past the cap is not decoded: it is
    /// marked as not drawn, with the cap as the reason. After [`MAX_DECODING_FRAMES`] calls what is left is marked as not drawn too, for the
    /// time it needs, so a picture that the forced decode never reaches (a page of many pictures) is whole all the same.
    pub(crate) fn work(&mut self, room: &mut DecodeRoom) {
        let started = room.clock.now();
        while !self.pending.is_empty() {
            if room.left.is_some_and(|left| room.clock.since(started) >= left) {
                if !room.force {
                    break;
                }
                room.force = false;
            }
            // The work of a decode is the pixels the header gives, taken from the room before it is made; past it, the image waits (a forced decode is the
            // clock's, and never the work's).
            let wanted = self.pending.front().map_or(0, |p| u64::from(p.size.0).saturating_mul(u64::from(p.size.1)));
            if wanted > room.pixels {
                room.short = true;
                break;
            }
            let Some(p) = self.pending.pop_front() else { break };
            let pixels = u64::from(p.size.0).saturating_mul(u64::from(p.size.1));
            let answer = if self.total.saturating_add(pixels) > MAX_PICTURE_PIXELS {
                Err(NotDrawn::Cap(format!("{pixels} pixels past the {MAX_PICTURE_PIXELS} a picture decodes ({} already)", self.total)))
            } else {
                self.total += pixels;
                room.pixels = room.pixels.saturating_sub(pixels);
                let decoded = decode_bitmap(&p.image, p.size);
                if decoded.is_ok() {
                    self.decodes = self.decodes.saturating_add(1);
                }
                decoded
            };
            self.bitmaps.insert(Arc::as_ptr(&p.stub) as usize, answer);
        }
        self.frames += 1;
        if self.frames >= MAX_DECODING_FRAMES {
            self.give_up("the frame's time did not suffice to decode it in 8 frames");
        }
    }

    /// Marks the images still waiting as not drawn, for `why`: the picture is whole after this.
    pub(crate) fn give_up(&mut self, why: &str) {
        for p in self.pending.drain(..) {
            self.bitmaps.insert(Arc::as_ptr(&p.stub) as usize, Err(NotDrawn::Cap(why.to_owned())));
        }
    }

    /// How many bitmaps this picture has decoded to pixels so far, over all the frames that worked on it.
    pub(crate) fn decodes(&self) -> u64 {
        self.decodes
    }

    /// How many bitmaps are decoded, and how many wait.
    #[cfg(test)]
    pub(crate) fn decoded_count(&self) -> usize {
        self.decoded()
    }

    #[cfg(test)]
    fn decoded(&self) -> usize {
        self.bitmaps.values().filter(|b| b.is_ok()).count()
    }

    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.pending.len()
    }

    /// Whether every bitmap has been looked at (decoded, or marked as not drawn for a reason that stays).
    pub(crate) fn is_complete(&self) -> bool {
        self.pending.is_empty()
    }

    /// About how many bytes the picture holds, for the cache: the parse, the pixmaps decoded, and for each image waiting its bytes and the
    /// **most its decoded pixels can take** (4 bytes a pixel of the header's size). A decode replaces the second by something no bigger, so
    /// the cost never rises as the picture is worked on: what the cache reserved when it took the picture covers everything that follows.
    pub(crate) fn cost(&self) -> u64 {
        // Saturating throughout: a weight that wrapped would let the cache take what it cannot hold.
        let pixels: u64 = self
            .bitmaps
            .values()
            .filter_map(|b| b.as_ref().ok())
            .map(|b| u64::from(b.pixmap.width()).saturating_mul(u64::from(b.pixmap.height())).saturating_mul(4))
            .fold(0, u64::saturating_add);
        let waiting: u64 = self
            .pending
            .iter()
            .map(|p| {
                let held = match &p.image {
                    mf::Image::Rgba { data, .. } => data.len() as u64,
                    mf::Image::Encoded(data) => data.len() as u64,
                };
                held.saturating_add(u64::from(p.size.0).saturating_mul(u64::from(p.size.1)).saturating_mul(4))
            })
            .fold(0, u64::saturating_add);
        self.base_cost.saturating_add(pixels).saturating_add(waiting)
    }
}

fn rgba(c: [u8; 4]) -> Rgba {
    Rgba { r: c[0], g: c[1], b: c[2], a: c[3] }
}

fn bez_of(p: &mf::Path) -> BezPath {
    let pt = |q: mf::Pt| Point::new(f64::from(q[0]), f64::from(q[1]));
    let mut out = BezPath::new();
    for s in &p.segs {
        match *s {
            mf::Seg::Move(a) => out.move_to(pt(a)),
            mf::Seg::Line(a) => out.line_to(pt(a)),
            mf::Seg::Cubic(a, b, c) => out.curve_to(pt(a), pt(b), pt(c)),
            mf::Seg::Close => out.close_path(),
        }
    }
    out
}

/// The length of a path's control polygon, in the path's units: an upper bound on its length, for the count of dashes.
fn polygon_length(p: &mf::Path) -> f64 {
    let mut total = 0.0;
    let mut last: Option<mf::Pt> = None;
    let dist = |a: mf::Pt, b: mf::Pt| (f64::from(b[0]) - f64::from(a[0])).hypot(f64::from(b[1]) - f64::from(a[1]));
    for s in &p.segs {
        match *s {
            mf::Seg::Move(a) => last = Some(a),
            mf::Seg::Line(a) => {
                if let Some(l) = last {
                    total += dist(l, a);
                }
                last = Some(a);
            }
            mf::Seg::Cubic(a, b, c) => {
                if let Some(l) = last {
                    total += dist(l, a) + dist(a, b) + dist(b, c);
                }
                last = Some(c);
            }
            mf::Seg::Close => {}
        }
    }
    total
}

/// The stops of a gradient for a target: in order, in 0 to 1, and no more than `MAX_GRADIENT_STOPS`... times four, since a file's
/// gradients (EMF+ blend patterns) hold more than a shape's.
fn stops_of(stops: &[(f64, [u8; 4])]) -> Vec<GradientStop> {
    let mut sorted: Vec<GradientStop> = stops.iter().map(|(o, c)| GradientStop { offset: o.clamp(0.0, 1.0), color: rgba(*c) }).collect();
    sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let most = MAX_GRADIENT_STOPS * 4;
    if sorted.len() > most {
        let step = sorted.len().div_ceil(most);
        sorted = sorted.into_iter().step_by(step).collect();
    }
    sorted
}

/// The stops of a gradient, converted once for the picture: items filled by one brush share the list (an `Arc`), and converting it (a sort of
/// up to 4,096 stops) for each of 400,000 items would cost more than drawing them.
fn stops_cached(cache: &mut HashMap<usize, Vec<GradientStop>>, stops: &Arc<[(f64, [u8; 4])]>) -> Vec<GradientStop> {
    let key = Arc::as_ptr(stops) as *const u8 as usize;
    cache.entry(key).or_insert_with(|| stops_of(stops)).clone()
}

/// A paint, and the transform to give with it (for a gradient whose own space is not paper).
fn paint_of(p: &mf::Paint, to_paper: Affine, cache: &mut HashMap<usize, Vec<GradientStop>>) -> (Paint, Option<Affine>) {
    match p {
        mf::Paint::Solid(c) => (Paint::Solid(rgba(*c)), None),
        mf::Paint::Linear(l) => {
            let (a, b) = (to_paper * Point::new(l.start[0], l.start[1]), to_paper * Point::new(l.end[0], l.end[1]));
            (Paint::Gradient(Gradient { kind: GradientKind::Linear { start: a, end: b }, stops: stops_cached(cache, &l.stops) }), None)
        }
        mf::Paint::Radial(r) => {
            let own = Affine::new([r.basis[0], r.basis[1], r.basis[2], r.basis[3], r.center[0], r.center[1]]);
            (
                Paint::Gradient(Gradient { kind: GradientKind::Radial { center: Point::ORIGIN, radius: 1.0 }, stops: stops_cached(cache, &r.stops) }),
                Some(to_paper * own),
            )
        }
    }
}

/// The colour that stands for a paint where only a colour can be used (a stroke by a radial gradient): its first stop.
fn flat_of(p: &mf::Paint) -> Rgba {
    match p {
        mf::Paint::Solid(c) => rgba(*c),
        mf::Paint::Linear(l) => l.stops.first().map_or(Rgba::BLACK, |s| rgba(s.1)),
        mf::Paint::Radial(r) => r.stops.first().map_or(Rgba::BLACK, |s| rgba(s.1)),
    }
}

/// The clips pushed on the target for the item being drawn, and which clip of the picture they are.
#[derive(Default)]
struct ClipState {
    current: u32,
    pushed: usize,
    /// The current clip lets nothing in the view through.
    hidden: bool,
}

/// What drawing a picture met that its report does not hold.
struct Drawing {
    report: mf::Report,
    glyphs: usize,
    out_of_range: usize,
    /// Path elements of clips pushed on the target so far: a clip is a path to rasterise each time it is entered (`MAX_CLIP_PUSH_ELEMENTS`).
    clip_elements: usize,
    /// The converted stops of the gradients met, by the address of the list.
    stops: HashMap<usize, Vec<GradientStop>>,
}

impl<T: Target> Frame<'_, '_, '_, T> {
    /// Draws a metafile into the box of a picture shape (the caller has cut to the frame or the clipping path: `clip`).
    pub(crate) fn draw_metafile(&mut self, sc: &ShapeCtx<'_>, meta: &MetaPicture, box_local: Rect, clip: &BezPath, who: &dyn Fn() -> String) {
        let pic = &meta.pic;
        let (pw, ph) = (pic.width, pic.height);
        if !(pw.is_finite() && ph.is_finite() && pw > 0.0 && ph > 0.0 && box_local.width() > 0.0 && box_local.height() > 0.0) {
            return;
        }
        // The frame's top edge on the box's top, each axis on its own: the frame is y down and the box y up.
        let to_paper = sc.to_paper * Affine::new([box_local.width() / pw, 0.0, 0.0, -box_local.height() / ph, box_local.x0, box_local.y1]);
        if !to_paper.as_coeffs().iter().all(|c| c.is_finite()) {
            return;
        }
        let mut drawing = Drawing { report: pic.report.clone(), glyphs: 0, out_of_range: 0, clip_elements: 0, stops: HashMap::new() };
        let mark = self.canvas.mark();
        self.canvas.push_clip(clip);
        let mut state = ClipState::default();
        for (n, item) in pic.items.iter().enumerate() {
            if self.meta_items >= MAX_METAFILE_ITEMS_PER_FRAME {
                drawing
                    .report
                    .note(mf::Detail::CapReached, || format!("more than {MAX_METAFILE_ITEMS_PER_FRAME} items of metafile pictures in a frame"));
                break;
            }
            self.meta_items += 1;
            // An item is work, counted before it is drawn.
            if !self.charge(WORK_PER_META_ITEM, who) {
                drawing.report.note(mf::Detail::CapReached, || "the frame's work ran out while the picture was drawn".to_owned());
                break;
            }
            // The clock is looked at every few items, and at every bitmap: one can cover the whole view.
            if (n % ITEMS_PER_TIME_CHECK == 0 || matches!(item.drawn, mf::Drawn::Bitmap(_))) && self.over_budget() {
                drawing.report.note(mf::Detail::CapReached, || "the frame's time ran out while the picture was drawn".to_owned());
                break;
            }
            if !self.enter_clip(&mut state, pic, item.clip, to_paper, &mut drawing) {
                continue;
            }
            match &item.drawn {
                mf::Drawn::Path(p) => self.meta_path(p, to_paper, &mut drawing, who),
                mf::Drawn::Bitmap(b) => self.meta_bitmap(b, meta, to_paper, &mut drawing, who),
                mf::Drawn::Text(t) => self.meta_text(t, to_paper, &mut drawing, who),
            }
        }
        // Closes the item clips and the cut to the frame.
        self.canvas.rewind(mark);
        self.note_metafile(&drawing, who);
    }

    /// Makes the clips on the target those of `index`. False when the item cannot show through them.
    fn enter_clip(&mut self, state: &mut ClipState, pic: &mf::Picture, index: u32, to_paper: Affine, drawing: &mut Drawing) -> bool {
        if state.current == index {
            return !state.hidden;
        }
        while state.pushed > 0 {
            self.canvas.pop_clip();
            state.pushed -= 1;
        }
        state.current = index;
        state.hidden = false;
        let Some(clip) = usize::try_from(index).ok().and_then(|i| pic.clips.get(i)) else { return true };
        for part in &clip.parts {
            if part.path.is_empty() {
                state.hidden = true;
                break;
            }
            let paper = to_paper * bez_of(&part.path);
            drawing.clip_elements = drawing.clip_elements.saturating_add(part.path.segs.len());
            if drawing.clip_elements > MAX_CLIP_PUSH_ELEMENTS {
                drawing.report.note(mf::Detail::CapReached, || "the clips of a metafile picture are entered too many times".to_owned());
                state.hidden = true;
                break;
            }
            // A clip that reaches past what the target takes (the frame with a hole, which excludes) is cut to a rectangle round the view; a
            // fill by the odd-crossings rule is the same inside it.
            match self.canvas.bring_in(paper, false, 0.0) {
                Ok(Some((cut, _))) => {
                    self.canvas.push_clip(&cut);
                    state.pushed += 1;
                }
                // The clip lies wholly outside the view: nothing of the item shows.
                Ok(None) => {
                    state.hidden = true;
                    break;
                }
                // A clip too far out to hand over is left off, and the frame says so.
                Err(Refused::OutOfRange) => drawing.out_of_range += 1,
            }
        }
        !state.hidden
    }

    fn meta_path(&mut self, p: &mf::PathItem, to_paper: Affine, drawing: &mut Drawing, who: &dyn Fn() -> String) {
        let paper = to_paper * bez_of(&p.path);
        if paper.is_empty() {
            return;
        }
        if let Some(fill) = &p.fill {
            let (paint, transform) = paint_of(&fill.paint, to_paper, &mut drawing.stops);
            match self.canvas.bring_in(paper.clone(), false, 0.0) {
                Ok(Some((path, _))) => match transform {
                    Some(t) => self.canvas.fill_with_paint_transform(&path, &paint, fill.even_odd, t),
                    None => self.canvas.fill(&path, &paint, fill.even_odd),
                },
                Ok(None) => {}
                Err(Refused::OutOfRange) => drawing.out_of_range += 1,
            }
        }
        if let Some(stroke) = &p.stroke {
            self.meta_stroke(stroke, &p.path, paper, to_paper, drawing, who);
        }
    }

    fn meta_stroke(
        &mut self,
        s: &mf::Stroke,
        frame_path: &mf::Path,
        paper: BezPath,
        to_paper: Affine,
        drawing: &mut Drawing,
        who: &dyn Fn() -> String,
    ) {
        let scale = to_paper.determinant().abs().sqrt();
        let ppi = self.px_per_inch.max(1.0e-9);
        let width = (s.width * scale).clamp(0.0, MAX_STROKE_PX / ppi);
        if !(width.is_finite() && width > 0.0) {
            return;
        }
        let cap = match s.cap {
            mf::Cap::Butt => kurbo::Cap::Butt,
            mf::Cap::Round => kurbo::Cap::Round,
            mf::Cap::Square => kurbo::Cap::Square,
        };
        let join = match s.join {
            mf::Join::Miter => kurbo::Join::Miter,
            mf::Join::Round => kurbo::Join::Round,
            mf::Join::Bevel => kurbo::Join::Bevel,
        };
        // A pen under a device pixel wide is drawn one wide; the dashes keep the lengths the pen gave them.
        let drawn = width.max(self.hairline());
        let mut stroke = Stroke::new(drawn).with_caps(cap).with_join(join).with_miter_limit(s.miter_limit.clamp(1.0, 100.0));
        if !s.dashes.is_empty() {
            // The dashes are dropped when they would make too many, or are too fine to see (as a shape's line is: `dash.rs`).
            let dashes: Vec<f64> = s.dashes.iter().map(|d| (d * scale).max(width * 1.0e-3)).collect();
            let period: f64 = dashes.iter().sum();
            let count = polygon_length(frame_path) * scale / period.max(f64::MIN_POSITIVE) * dashes.len() as f64;
            if period.is_finite() && period * ppi >= MIN_DASH_PERIOD_PX && count.is_finite() && count <= MAX_DASHES {
                // The dashes are work, counted before the stroke makes them; at most `MAX_DASHES`, so the cast keeps the count.
                if !self.charge((count.max(0.0).ceil() as u64).saturating_mul(WORK_PER_DASH), who) {
                    return;
                }
                stroke = stroke.with_dashes(s.dash_offset * scale, dashes);
            }
        }
        let paint = match &s.paint {
            mf::Paint::Linear(_) => paint_of(&s.paint, to_paper, &mut drawing.stops).0,
            other => Paint::Solid(flat_of(other)),
        };
        match self.canvas.bring_in(paper, true, drawn * ppi / 2.0 + 1.0) {
            Ok(Some((path, _))) => self.canvas.stroke(&path, &stroke, &paint),
            Ok(None) => {}
            Err(Refused::OutOfRange) => drawing.out_of_range += 1,
        }
    }

    fn meta_bitmap(&mut self, b: &mf::BitmapItem, meta: &MetaPicture, to_paper: Affine, drawing: &mut Drawing, who: &dyn Fn() -> String) {
        let Some(found) = meta.bitmaps.get(&key_of(&b.image)) else {
            // Not decoded yet: a later frame has it.
            drawing.report.note(mf::Detail::CapReached, || "a bitmap that is decoded over the next frames".to_owned());
            return;
        };
        let bitmap = match found {
            Ok(bitmap) => bitmap,
            Err(NotDrawn::Undecodable(why)) => {
                drawing.report.note(mf::Detail::BitmapUndecodable, || why.clone());
                return;
            }
            Err(NotDrawn::Cap(why)) => {
                drawing.report.note(mf::Detail::CapReached, || why.clone());
                return;
            }
        };
        let (iw, ih) = bitmap.size;
        // The source rectangle in the image's own pixels (all of it when the item names none), onto the parallelogram of three corners.
        let [sx, sy, sw, sh] = b.src.filter(|s| s[2] > 0.0 && s[3] > 0.0 && s.iter().all(|v| v.is_finite())).unwrap_or([0.0, 0.0, iw, ih]);
        let [ul, ur, ll] = b.quad;
        let (a, bb) = ((ur[0] - ul[0]) / sw, (ur[1] - ul[1]) / sw);
        let (c, d) = ((ll[0] - ul[0]) / sh, (ll[1] - ul[1]) / sh);
        let to_frame = Affine::new([a, bb, c, d, ul[0] - (sx * a + sy * c), ul[1] - (sx * bb + sy * d)]);
        // The pixmap may be the image reduced by a whole factor: its pixel space is scaled up to the image's.
        let (kx, ky) = (iw / f64::from(bitmap.pixmap.width()).max(1.0), ih / f64::from(bitmap.pixmap.height()).max(1.0));
        let place = to_paper * to_frame * Affine::scale_non_uniform(kx, ky);
        // Nothing outside the parallelogram of the draw is shown: the image has more than the source rectangle.
        let lr = [ur[0] + ll[0] - ul[0], ur[1] + ll[1] - ul[1]];
        let mut quad = BezPath::new();
        for (i, p) in [ul, ur, lr, ll].iter().enumerate() {
            let q = to_paper * Point::new(p[0], p[1]);
            if i == 0 { quad.move_to(q) } else { quad.line_to(q) }
        }
        quad.close_path();
        if !place.as_coeffs().iter().all(|v| v.is_finite()) {
            return;
        }
        match self.canvas.check(&quad, 0.0) {
            Ok(true) => {
                // Drawing the bitmap is work, by the pixels it covers.
                if !self.charge(self.image_work(f64::from(bitmap.pixmap.width()), f64::from(bitmap.pixmap.height()), place), who) {
                    return;
                }
                self.canvas.push_clip(&quad);
                self.canvas.image(&bitmap.pixmap, place);
                self.canvas.pop_clip();
            }
            Ok(false) => {}
            Err(Refused::OutOfRange) => drawing.out_of_range += 1,
        }
    }

    /// A face for `c` in the family's face, or in another that has it, and the glyph.
    fn meta_glyph(&self, face: &Face, c: char, symbol: bool, found: &mut HashMap<char, Option<(Face, u32)>>) -> Option<(Face, u32)> {
        if let Some(known) = found.get(&c) {
            return known.clone();
        }
        let answer = if face.covers(c) {
            Some((face.clone(), face.glyph_for(c)))
        } else if symbol && (c as u32) < 0x100 && face.covers(char::from_u32(0xF000 + c as u32).unwrap_or(c)) {
            // A symbol font maps its codes into the private area.
            let shifted = char::from_u32(0xF000 + c as u32).unwrap_or(c);
            Some((face.clone(), face.glyph_for(shifted)))
        } else {
            match self.fonts.covering_checked(&[c]) {
                Cover::Found(other) => {
                    let gid = other.glyph_for(c);
                    Some((other, gid))
                }
                Cover::Missing | Cover::Cut => None,
            }
        };
        found.insert(c, answer.clone());
        answer
    }

    fn meta_text(&mut self, t: &mf::TextItem, to_paper: Affine, drawing: &mut Drawing, who: &dyn Fn() -> String) {
        let started = self.clock.now();
        if self.limits.text_time.is_some_and(|limit| self.text_time > limit) {
            drawing.report.note(mf::Detail::CapReached, || "the time for text ran out".to_owned());
            return;
        }
        let symbol_missing = t.font.symbol && !self.fonts.has_family(&t.font.family);
        if symbol_missing {
            drawing.report.note(mf::Detail::FontMissing, || format!("a symbol font, {}", t.font.family));
            return;
        }
        let resolved = self.fonts.resolve(&t.font.family, FontStyle { bold: t.font.bold, italic: t.font.italic });
        let face = resolved.face.clone();
        if face.is_null() {
            return;
        }
        let chars: Vec<char> = t.text.chars().collect();
        if drawing.glyphs.saturating_add(chars.len()) > MAX_GLYPHS_PER_PICTURE {
            drawing.report.note(mf::Detail::CapReached, || "the glyphs of a metafile picture".to_owned());
            return;
        }
        // The characters of a frame are counted for every picture on it, as the text of shapes is.
        if self.text_chars.saturating_add(chars.len()) > self.limits.max_text_chars {
            drawing.report.note(mf::Detail::CapReached, || "the characters of text a frame lays out".to_owned());
            return;
        }
        // The glyphs are work, counted before they are painted.
        if !self.charge((chars.len() as u64).saturating_mul(WORK_PER_GLYPH), who) {
            return;
        }
        self.text_chars += chars.len();
        drawing.glyphs += chars.len();
        let (ascent, descent) = (face.ascent_share(), face.descent_share());
        // The size is an em, or the cell of a line: the cell is the face's ascent and descent together.
        let em = if t.cell && ascent + descent > 0.0 { t.size / (ascent + descent) } else { t.size };
        let slant = if resolved.synthetic_italic { SLANT } else { 0.0 };
        let right = [t.basis[0] * em, t.basis[1] * em];
        let up = [t.basis[2] * em + right[0] * slant, t.basis[3] * em + right[1] * slant];
        let mut found: HashMap<char, Option<(Face, u32)>> = HashMap::new();
        // Finding a face for letters the family lacks searches the database: the clock is looked at as it goes, and what is not found in time
        // is not drawn.
        let mut glyphs: Vec<Option<(Face, u32)>> = Vec::with_capacity(chars.len());
        let mut out_of_time = false;
        for (i, c) in chars.iter().enumerate() {
            if !out_of_time
                && i % 256 == 255
                && self.limits.text_time.is_some_and(|limit| self.text_time.saturating_add(self.clock.since(started)) > limit)
            {
                out_of_time = true;
                drawing.report.note(mf::Detail::CapReached, || "the time for text ran out while letters were looked for".to_owned());
            }
            glyphs.push(if c.is_control() || out_of_time { None } else { self.meta_glyph(&face, *c, t.font.symbol, &mut found) });
        }
        // The advance of each character in ems, from the glyph's own face.
        let advance = |g: &Option<(Face, u32)>| g.as_ref().map_or(0.5, |(f, gid)| f.advance(*gid) / f.units_per_em().max(1.0));
        let shares: Vec<f64> = glyphs.iter().map(advance).collect();
        let mut origins: Vec<[f64; 2]> = match &t.placement {
            mf::Placement::Origins(o) => o.clone(),
            mf::Placement::Flow { origin, h_align } => {
                let total: f64 = shares.iter().sum();
                let k = match h_align {
                    mf::HAlign::Left => 0.0,
                    mf::HAlign::Center => 0.5,
                    mf::HAlign::Right => 1.0,
                };
                let mut x = -total * k;
                shares
                    .iter()
                    .map(|a| {
                        let p = [origin[0] + right[0] * x, origin[1] + right[1] * x];
                        x += a;
                        p
                    })
                    .collect()
            }
        };
        // Where the reference point sits against the line: the baseline is an ascent below the top, or a descent above the bottom.
        let lift = match t.v_align {
            mf::VAlign::Top => -ascent,
            mf::VAlign::Baseline => 0.0,
            mf::VAlign::Bottom => descent,
        };
        if lift != 0.0 {
            for o in &mut origins {
                o[0] += up[0] * lift;
                o[1] += up[1] * lift;
            }
        }
        let color = rgba(t.color);
        if color.a == 0 || origins.is_empty() {
            return;
        }
        let (first, last) = (origins.first().copied().unwrap_or([0.0; 2]), origins.last().copied().unwrap_or([0.0; 2]));
        let end = [last[0] + right[0] * shares.last().copied().unwrap_or(0.0), last[1] + right[1] * shares.last().copied().unwrap_or(0.0)];
        let band = |lo: f64, hi: f64| -> BezPath {
            let at = |p: [f64; 2], k: f64| to_paper * Point::new(p[0] + up[0] * k, p[1] + up[1] * k);
            let mut path = BezPath::new();
            path.move_to(at(first, lo));
            path.line_to(at(end, lo));
            path.line_to(at(end, hi));
            path.line_to(at(first, hi));
            path.close_path();
            path
        };
        if let Some(bg) = t.background {
            self.meta_fill(&band(-descent, ascent), rgba(bg), drawing);
        }
        let mut combined = BezPath::new();
        for ((g, o), _) in glyphs.iter().zip(origins.iter()).zip(chars.iter()) {
            let Some((gface, gid)) = g else { continue };
            let Some(outline) = self.caches.outline(gface, *gid) else { continue };
            let gu = gface.units_per_em().max(1.0);
            let place = to_paper * Affine::new([right[0] / gu, right[1] / gu, up[0] / gu, up[1] / gu, o[0], o[1]]);
            combined.extend((place * (*outline).clone()).elements().iter().copied());
        }
        if !combined.is_empty() {
            let embolden = resolved.synthetic_bold;
            self.meta_fill(&combined, color, drawing);
            if embolden {
                let width = em * to_paper.determinant().abs().sqrt() * EMBOLDEN;
                self.meta_outline(&combined, width, color, drawing);
            }
        }
        // Underline and strike-out are bands the face recommends, or a plain one.
        if t.font.underline {
            let u = face.underline().unwrap_or(diacraft_fonts::Stroke { top: -0.1, thickness: 0.05 });
            self.meta_fill(&band(u.top - u.thickness, u.top), color, drawing);
        }
        if t.font.strike_out {
            let s = face.strikeout().unwrap_or(diacraft_fonts::Stroke { top: 0.3, thickness: 0.05 });
            self.meta_fill(&band(s.top - s.thickness, s.top), color, drawing);
        }
        self.text_time += self.clock.since(started);
    }

    fn meta_fill(&mut self, path: &BezPath, color: Rgba, drawing: &mut Drawing) {
        match self.canvas.bring_in(path.clone(), false, 0.0) {
            Ok(Some((p, _))) => self.canvas.fill(&p, &Paint::Solid(color), false),
            Ok(None) => {}
            Err(Refused::OutOfRange) => drawing.out_of_range += 1,
        }
    }

    fn meta_outline(&mut self, path: &BezPath, width: f64, color: Rgba, drawing: &mut Drawing) {
        let stroke = Stroke::new(width).with_join(kurbo::Join::Round).with_caps(kurbo::Cap::Round);
        match self.canvas.check(path, width * self.px_per_inch) {
            Ok(true) => self.canvas.stroke(path, &stroke, &Paint::Solid(color)),
            Ok(false) => {}
            Err(Refused::OutOfRange) => drawing.out_of_range += 1,
        }
    }

    /// One row for the picture, naming the first detail that applies, and the others in its words.
    fn note_metafile(&mut self, drawing: &Drawing, who: &dyn Fn() -> String) {
        if drawing.out_of_range > 0 {
            self.cx
                .note(SkipKind::OutOfRange, || format!("{}: {} parts of an EMF or WMF picture too far from the page", who(), drawing.out_of_range));
        }
        let Some(first) = drawing.report.first() else { return };
        let kind = match first {
            mf::Detail::GdiFallback => SkipKind::MetafileGdiFallback,
            mf::Detail::EmfPlusMisplaced => SkipKind::MetafileEmfPlusMisplaced,
            mf::Detail::RecordSkipped => SkipKind::MetafileRecordSkipped,
            mf::Detail::GlyphIndexText => SkipKind::MetafileGlyphIndexText,
            mf::Detail::FontMissing => SkipKind::MetafileFontMissing,
            mf::Detail::BitmapUndecodable => SkipKind::MetafileBitmapUndecodable,
            mf::Detail::CutShort => SkipKind::MetafileCutShort,
            mf::Detail::CapReached => SkipKind::MetafileCapReached,
        };
        self.cx.note(kind, || {
            let all: Vec<String> = drawing.report.entries().iter().map(|(d, n, why)| format!("{} ({n}): {why}", d.name())).collect();
            format!("{}: {}", who(), all.join("; "))
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A picture of `n` bitmap items, each its own 2 by 2 image.
    fn picture_of(n: usize, side: u32) -> mf::Picture {
        let items = (0..n)
            .map(|_| mf::Item {
                clip: 0,
                drawn: mf::Drawn::Bitmap(Box::new(mf::BitmapItem {
                    image: mf::Image::Rgba { width: side, height: side, data: Arc::new(vec![255; side as usize * side as usize * 4]) },
                    src: None,
                    quad: [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
                })),
            })
            .collect();
        mf::Picture { kind: mf::Kind::Emf, width: 10.0, height: 10.0, items, clips: vec![mf::Clip::default()], report: mf::Report::default() }
    }

    #[test]
    fn bitmaps_are_decoded_while_the_total_is_under_the_cap_and_marked_with_the_cap_after() {
        // Three images of 5,000 by 5,000 pixels (25 million each): the first two make 50 million, the third would make 75, past the 64
        // million a picture decodes.
        let meta = MetaPicture::new(picture_of(3, 5_000), &mut DecodeRoom::unlimited());
        let mut results: Vec<bool> = Vec::new();
        for item in &meta.pic.items {
            let mf::Drawn::Bitmap(b) = &item.drawn else { panic!("a bitmap") };
            results.push(matches!(meta.bitmaps.get(&key_of(&b.image)), Some(Ok(_))));
        }
        assert_eq!(results, [true, true, false]);
        let last = meta.pic.items.last().and_then(|i| if let mf::Drawn::Bitmap(b) = &i.drawn { meta.bitmaps.get(&key_of(&b.image)) } else { None });
        assert!(matches!(last, Some(Err(NotDrawn::Cap(_)))), "the cap is the reason");
        assert!(meta.is_complete());
    }

    #[test]
    fn a_frame_with_no_time_left_decodes_one_bitmap_in_all_so_that_a_picture_goes_on() {
        // The forced decode is the frame's, not the picture's: the first call has it, the second in the same frame does not.
        let mut room = DecodeRoom::spent();
        let mut meta = MetaPicture::new(picture_of(3, 2), &mut room);
        assert_eq!((meta.decoded(), meta.waiting(), room.force), (1, 2, false), "one, whatever the time");
        meta.work(&mut room);
        assert_eq!((meta.decoded(), meta.waiting()), (1, 2), "and no more in the same frame");
        // The next frame has its own.
        meta.work(&mut DecodeRoom::spent());
        assert_eq!((meta.decoded(), meta.waiting()), (2, 1));
        meta.work(&mut DecodeRoom::spent());
        assert_eq!((meta.decoded(), meta.waiting()), (3, 0));
        assert!(meta.is_complete());
        meta.work(&mut DecodeRoom::spent());
        assert_eq!(meta.decoded(), 3, "work on a whole picture changes nothing");
    }

    #[test]
    fn a_picture_whose_bitmaps_need_more_than_the_most_frames_has_the_rest_marked_not_drawn_with_the_reason() {
        let mut meta = MetaPicture::new(picture_of(30, 2), &mut DecodeRoom::spent());
        for _ in 1..MAX_DECODING_FRAMES {
            meta.work(&mut DecodeRoom::spent());
        }
        assert!(meta.is_complete(), "after {MAX_DECODING_FRAMES} frames it is whole");
        assert_eq!(meta.decoded(), MAX_DECODING_FRAMES as usize, "one a frame was decoded");
        let marked = meta.bitmaps.values().filter(|b| matches!(b, Err(NotDrawn::Cap(why)) if why.contains("frames"))).count();
        assert_eq!(marked, 30 - MAX_DECODING_FRAMES as usize, "and the others say why they are not drawn");
    }

    #[test]
    fn the_cost_of_a_picture_never_rises_as_its_bitmaps_are_decoded() {
        // The cache weighs a picture by the most it can come to; decoding replaces what waits by something no bigger.
        let mut meta = MetaPicture::new(picture_of(6, 40), &mut DecodeRoom::spent());
        let mut last = meta.cost();
        while !meta.is_complete() {
            meta.work(&mut DecodeRoom::spent());
            let now = meta.cost();
            assert!(now <= last, "{now} after {last}");
            last = now;
        }
    }

    /// The start of a PNG: the signature and an `IHDR` that gives this size (a crafted header; nothing follows it).
    fn png_header_of(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 0, 0, 0, 0]);
        bytes
    }

    /// A picture of one bitmap item for each header in `sizes`, each its own bytes.
    fn picture_of_headers(sizes: &[(u32, u32)]) -> mf::Picture {
        let mut pic = picture_of(0, 2);
        for (w, h) in sizes {
            pic.items.push(mf::Item {
                clip: 0,
                drawn: mf::Drawn::Bitmap(Box::new(mf::BitmapItem {
                    image: mf::Image::Encoded(Arc::new(png_header_of(*w, *h))),
                    src: None,
                    quad: [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
                })),
            });
        }
        pic
    }

    #[test]
    fn a_header_of_65535_by_65535_is_marked_not_drawn_before_it_waits_and_adds_nothing_to_the_weight() {
        // A room with no time, so that nothing is decoded to take the image out of the queue: only the header's size can keep it out.
        let meta = MetaPicture::new(picture_of_headers(&[(65_535, 65_535), (65_535, 65_535)]), &mut DecodeRoom::spent());
        assert_eq!(meta.waiting(), 0, "it never went into the queue");
        assert!(meta.is_complete());
        let only = meta.pic.items.iter().find_map(|i| if let mf::Drawn::Bitmap(b) = &i.drawn { meta.bitmaps.get(&key_of(&b.image)) } else { None });
        assert!(matches!(only, Some(Err(NotDrawn::Cap(why))) if why.contains("pixels")), "past the pixels a picture decodes, and it says so");
        assert!(meta.cost() < 100_000, "and it weighs nothing: {}", meta.cost());
    }

    #[test]
    fn headers_so_big_that_their_sizes_would_overflow_the_weight_are_refused_and_the_weight_stays_a_number() {
        // 2^31 - 1 on both sides, in several images: four bytes a pixel of one is about 2^64, and the sum of two is past it.
        let huge = (0x7FFF_FFFF, 0x7FFF_FFFF);
        let meta = MetaPicture::new(picture_of_headers(&[huge, huge, huge, (0, 5), (5, 0)]), &mut DecodeRoom::spent());
        assert_eq!(meta.waiting(), 0);
        assert!(meta.cost() < 100_000, "{}", meta.cost());
        assert_eq!(meta.decoded(), 0);
    }
}
