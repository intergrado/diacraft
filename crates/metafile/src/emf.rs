//! Enhanced metafiles: the header, the frame, the walk over records, and the records of GDI.
//!
//! A record is a type and a size followed by fields; offsets inside a record (a bitmap's header and bits, a string) count from the
//! record's start. The EMF+ half of a file is read by `plus.rs`; how the two halves are told apart and which is played is decided here.

use std::rc::Rc;
use std::sync::Arc;

use crate::bytes::{Bytes, decode_utf16};
use crate::canvas::Canvas;
use crate::clip::{ClipError, Mode};
use crate::dib::{self, Blend, DST_NOP, PATCOPY, Refusal, SRCCOPY};
use crate::gdi::{Brush, FontObj, Gdi, Obj, Pen, PenStyle, Surface, TextOut, colorref};
use crate::geom::{ArcKind, P};
use crate::limits::{MAX_CLIP_POINTS, MAX_TEXT_CHARS};
use crate::model::{BitmapItem, Cap, Detail, Drawn, Image, Join};
use crate::plus::{Plus, PlusOutcome};
use crate::xf::Xf;
use crate::{Error, Frame};

pub(crate) const EMF_SIGNATURE: [u8; 4] = [0x20, 0x45, 0x4D, 0x46];
/// "EMF+", the identifier of a comment that carries EMF+ records.
pub(crate) const EMF_PLUS_ID: u32 = 0x2B46_4D45;

/// The header's fields this crate uses.
#[derive(Clone, Debug)]
pub(crate) struct Header {
    pub bounds: [f64; 4],
    /// In 0.01 mm.
    pub frame: [f64; 4],
    pub device: P,
    pub millimeters: P,
    pub handles: usize,
    /// Where the first record after the header starts.
    pub next: usize,
}

/// Whether the bytes start like an EMF: a type-1 record with the signature at byte 40.
pub(crate) fn is_emf(b: &[u8]) -> bool {
    Bytes(b).u32(0) == Some(1) && b.get(40..44) == Some(&EMF_SIGNATURE[..])
}

/// Reads the header. The header's length is not its `Size`: it is 88 bytes unless the size, the description and a pixel format block
/// say more, and the fields past it are not header fields. Everything this crate uses lies in the first 88.
pub(crate) fn read_header(b: Bytes) -> Result<Header, Error> {
    let bad = |why: &'static str| Error::Undecodable(why);
    if !is_emf(b.0) || b.len() < 88 {
        return Err(bad("an EMF whose header is cut short"));
    }
    let size = b.count(4).ok_or(bad("an EMF whose header is cut short"))?;
    let handles = usize::from(b.u16(56).ok_or(bad("an EMF whose header is cut short"))?);
    let bounds = b.rect_i32(8).ok_or(bad("an EMF whose header is cut short"))?;
    let frame = b.rect_i32(24).ok_or(bad("an EMF whose header is cut short"))?;
    let device = b.point_i32(72).ok_or(bad("an EMF whose header is cut short"))?;
    let millimeters = b.point_i32(80).ok_or(bad("an EMF whose header is cut short"))?;
    // The header ends at the size if that is at least 88, at the description if that follows 88 and ends within the size, and at the
    // pixel format block if that follows 100 and starts before the current end. Only where it ends matters for which fields exist, and
    // none of the fields past 88 is read, so this decides the first record's place and nothing else.
    let mut end = if size >= 88 { size } else { 88 };
    if let (Some(n_desc), Some(off_desc)) = (b.count(60), b.count(64))
        && off_desc >= 88
        && n_desc.checked_mul(2).and_then(|n| off_desc.checked_add(n)).is_some_and(|e| e <= end)
    {
        end = off_desc;
    }
    // The first record is where the header record's own size says (the description sits inside that record).
    let next = if size >= 88 { size } else { end };
    Ok(Header { bounds, frame, device, millimeters, handles, next })
}

/// The frame in device pixels: `Frame` (0.01 mm) times `Device` over 100 × `Millimeters`, on each axis.
fn frame_px(h: &Header) -> Option<[f64; 4]> {
    let (sx, sy) = (h.device[0] / (100.0 * h.millimeters[0]), h.device[1] / (100.0 * h.millimeters[1]));
    let f = [h.frame[0] * sx, h.frame[1] * sy, h.frame[2] * sx, h.frame[3] * sy];
    (f.iter().all(|v| v.is_finite()) && f[2] > f[0] && f[3] > f[1]).then_some(f)
}

/// Device pixels per millimetre on each axis, from `Device` over `Millimeters`; 96 dpi when the header gives none.
fn px_per_mm(h: &Header) -> P {
    let (x, y) = (h.device[0] / h.millimeters[0], h.device[1] / h.millimeters[1]);
    let ok = |v: f64| v.is_finite() && v > 0.0;
    [if ok(x) { x } else { 96.0 / 25.4 }, if ok(y) { y } else { 96.0 / 25.4 }]
}

/// Which half of the file is played.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode2 {
    /// Every EMF record.
    Plain,
    /// The EMF+ records, and the EMF records of GetDC sections.
    Plus { dual: bool },
}

/// What a walk found.
pub(crate) struct Walked {
    pub cut_short: bool,
}

/// The frame a picture is drawn in and the device it was recorded for.
pub(crate) struct Setup {
    pub frame: Frame,
    pub gdi: Gdi,
    /// The frame's top-left corner in device pixels.
    pub origin: P,
}

pub(crate) fn setup(h: &Header) -> Setup {
    let ppm = px_per_mm(h);
    let (rect, known) = match frame_px(h) {
        Some(f) => (f, true),
        None => {
            // No usable frame: the bounds stand in (inclusive on both sides), and failing that what is drawn decides.
            let [l, t, r, b] = h.bounds;
            if r >= l && b >= t { ([l, t, r + 1.0, b + 1.0], true) } else { ([0.0, 0.0, 1.0, 1.0], false) }
        }
    };
    let origin = [rect[0], rect[1]];
    let gdi = Gdi::new(Surface::Emf { px_per_mm: ppm, origin }, h.handles, ppm[1] * 25.4);
    Setup { frame: Frame { width: rect[2] - rect[0], height: rect[3] - rect[1], known }, gdi, origin }
}

/// Whether the second record starts an EMF+ stream: an `EMR_COMMENT` with the EMF+ identifier whose first EMF+ record is the header.
/// Returns the header's Dual bit.
fn plus_start(b: Bytes, first: usize) -> Option<bool> {
    let size = b.count(first.checked_add(4)?)?;
    if b.u32(first)? != 70 || size < 28 || b.u32(first + 12)? != EMF_PLUS_ID {
        return None;
    }
    // The first EMF+ record is at +16: type, flags, size, data size.
    (b.u16(first + 16)? == 0x4001).then(|| b.u16(first + 18).is_some_and(|f| f & 1 != 0))
}

/// Reads the records of an EMF into `cv`. Returns whether the walk was cut short and which half was played.
pub(crate) fn read(b: Bytes, h: &Header, cv: &mut Canvas, depth: u32) -> (Walked, bool) {
    let plus_dual = plus_start(b, h.next);
    if let Some(dual) = plus_dual {
        let mut s = setup(h);
        let mut plus = Plus::new(depth, dual, s.origin);
        let walked = walk(b, h, cv, &mut s, Mode2::Plus { dual }, Some(&mut plus));
        match plus.outcome() {
            PlusOutcome::Fine => return (walked, true),
            PlusOutcome::Unplayable(why) => {
                if dual {
                    // The EMF half draws the whole picture, and the halves are never mixed.
                    cv.reset();
                    cv.note(Detail::GdiFallback, || why);
                } else {
                    cv.note(Detail::RecordSkipped, || why);
                    return (walked, true);
                }
            }
        }
    }
    let mut s = setup(h);
    let walked = walk(b, h, cv, &mut s, Mode2::Plain, None);
    (walked, false)
}

/// The walk over the records after the header.
fn walk(b: Bytes, h: &Header, cv: &mut Canvas, s: &mut Setup, mode: Mode2, mut plus: Option<&mut Plus>) -> Walked {
    let mut pos = h.next;
    let mut cut_short = false;
    let mut in_getdc = false;
    let mut plus_header_seen = false;
    loop {
        if pos >= b.len() || !cv.budget.alive() {
            break;
        }
        let (Some(typ), Some(size)) = (b.u32(pos), b.count(pos + 4)) else {
            cut_short = true;
            break;
        };
        // A size under 8, not a multiple of 4 or past the data ends the walk.
        let Some(rec) = (size >= 8 && size % 4 == 0).then(|| b.sub(pos, size)).flatten() else {
            cut_short = true;
            break;
        };
        if !cv.budget.record() || !cv.budget.work(size as u64 / 4 + 1) {
            break;
        }
        pos += size;
        if typ == 14 {
            break;
        }
        if typ == 70 {
            if let Some(stream) = comment_plus(rec) {
                match (&mut plus, mode) {
                    (Some(p), Mode2::Plus { .. }) => in_getdc = p.comment(stream, cv, in_getdc),
                    _ => {
                        if !plus_header_seen && stream.u16(0) == Some(0x4001) {
                            plus_header_seen = true;
                            cv.note(Detail::EmfPlusMisplaced, || "an EMF+ header where an EMF+ file has none".to_owned());
                        }
                    }
                }
            }
            continue;
        }
        if mode == Mode2::Plain || in_getdc {
            gdi_record(&mut s.gdi, cv, h, typ, rec);
        }
    }
    Walked { cut_short }
}

/// The EMF+ records of a comment, when it carries them: the bytes after the identifier, bounded by the comment's own data size.
fn comment_plus(rec: Bytes) -> Option<Bytes> {
    let data = rec.count(8)?;
    if data < 4 || rec.u32(12)? != EMF_PLUS_ID {
        return None;
    }
    rec.sub(16, data.checked_sub(4)?.min(rec.len().checked_sub(16)?))
}

// ---- the records of GDI ----

/// Plays one EMF record on the GDI state. A record type outside is skipped and reported.
pub(crate) fn gdi_record(g: &mut Gdi, cv: &mut Canvas, h: &Header, typ: u32, r: Bytes) {
    let known = state_record(g, cv, typ, r)
        || object_record(g, typ, r)
        || shape_record(g, cv, typ, r)
        || poly_record(g, cv, typ, r)
        || path_record(g, cv, typ, r);
    if known || clip_record(g, cv, typ, r) || text_record(g, cv, h, typ, r) || bitmap_record(g, cv, typ, r) {
        return;
    }
    // Records that change nothing this crate draws, and the first and last.
    if matches!(typ, 1 | 13 | 14 | 21 | 48 | 49 | 50 | 51 | 52 | 98 | 115) {
        return;
    }
    cv.note(Detail::RecordSkipped, || format!("EMF record type {typ}"));
}

fn state_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    let word = |at: usize| r.u32(at);
    match typ {
        9 => {
            if let Some(p) = r.point_i32(8) {
                g.set_window_ext(p)
            }
        }
        10 => {
            if let Some(p) = r.point_i32(8) {
                g.set_window_org(p)
            }
        }
        11 => {
            if let Some(p) = r.point_i32(8) {
                g.set_viewport_ext(p)
            }
        }
        12 => {
            if let Some(p) = r.point_i32(8) {
                g.set_viewport_org(p)
            }
        }
        17 => {
            if let Some(m) = word(8) {
                g.set_map_mode(m)
            }
        }
        18 => {
            // Only 1 and 2 are modes.
            if let Some(m) = word(8).filter(|m| matches!(m, 1 | 2)) {
                g.dc.bk_mode = m
            }
        }
        19 => {
            if let Some(m) = word(8).filter(|m| matches!(m, 1 | 2)) {
                g.dc.poly_fill = m
            }
        }
        20 => {
            if let Some(m) = word(8) {
                g.dc.rop2 = m
            }
        }
        22 => {
            if let Some(a) = word(8) {
                g.dc.text_align = a
            }
        }
        24 => {
            if let Some(c) = word(8) {
                g.dc.text_color = colorref(c)
            }
        }
        25 => {
            if let Some(c) = word(8) {
                g.dc.bk_color = colorref(c)
            }
        }
        58 => {
            if let Some(m) = word(8).map(f64::from).filter(|m| *m >= 1.0) {
                g.dc.miter = m
            }
        }
        33 => g.save(cv),
        34 => {
            if let Some(n) = r.i32(8) {
                g.restore(n)
            }
        }
        35 => {
            if let Some(m) = r.xform(8) {
                g.modify_world(Xf::from_array(m), 4)
            }
        }
        36 => {
            if let (Some(m), Some(mode)) = (r.xform(8), word(32)) {
                g.modify_world(Xf::from_array(m), mode)
            }
        }
        _ => return false,
    }
    true
}

fn object_record(g: &mut Gdi, typ: u32, r: Bytes) -> bool {
    match typ {
        37 => {
            if let Some(i) = r.u32(8) {
                g.select(i)
            }
        }
        40 => {
            if let Some(i) = r.u32(8) {
                g.delete(i)
            }
        }
        38 => create_pen(g, r),
        95 => create_ext_pen(g, r),
        39 => create_brush(g, r),
        93 | 94 => {
            if let Some(i) = r.u32(8) {
                g.create(i, Obj::Brush(Rc::new(Brush::Unsupported)))
            }
        }
        82 => create_font(g, r),
        _ => return false,
    }
    true
}

fn cap_of(style: u32) -> Cap {
    match style & 0x0F00 {
        0x0100 => Cap::Square,
        0x0200 => Cap::Butt,
        _ => Cap::Round,
    }
}

fn join_of(style: u32) -> Join {
    match style & 0xF000 {
        0x1000 => Join::Bevel,
        0x2000 => Join::Miter,
        _ => Join::Round,
    }
}

fn dash_of(style: u32, entries: Vec<f64>) -> PenStyle {
    match style & 0x0F {
        1 => PenStyle::Dash,
        2 => PenStyle::Dot,
        3 => PenStyle::DashDot,
        4 => PenStyle::DashDotDot,
        7 => PenStyle::User(entries),
        _ => PenStyle::Solid,
    }
}

/// `EMR_CREATEPEN`: handle, then a `LogPen` (style, width, colour).
fn create_pen(g: &mut Gdi, r: Bytes) {
    let (Some(ix), Some(style), Some(width), Some(color)) = (r.u32(8), r.u32(12), r.f64_i32(16), r.u32(24)) else { return };
    let pen = Pen {
        null: style & 0x0F == 5,
        cosmetic: width <= 1.0,
        width,
        color: colorref(color),
        dash: dash_of(style, Vec::new()),
        cap: Cap::Round,
        join: Join::Round,
        inside: style & 0x0F == 6,
    };
    g.create(ix, Obj::Pen(Rc::new(pen)));
}

/// `EMR_EXTCREATEPEN`: handle, bitmap offsets, then an `ExtLogPen`: style, width, brush style, colour, hatch, and the user style
/// entries.
fn create_ext_pen(g: &mut Gdi, r: Bytes) {
    let (Some(ix), Some(style), Some(width), Some(brush), Some(color)) = (r.u32(8), r.u32(28), r.f64_i32(32), r.u32(36), r.u32(40)) else {
        return;
    };
    let n = r.count(48).unwrap_or(0);
    let entries: Vec<f64> = if n > 0 && n <= 16 { (0..n).filter_map(|i| r.u32(52 + i * 4).map(f64::from)).collect() } else { Vec::new() };
    let geometric = style & 0x000F_0000 == 0x0001_0000;
    let pen = Pen {
        // A pen whose brush is null draws nothing; so does the null style.
        null: style & 0x0F == 5 || brush == 1,
        cosmetic: !geometric,
        width: if geometric { width } else { 1.0 },
        color: colorref(color),
        dash: dash_of(style, entries),
        cap: cap_of(style),
        join: join_of(style),
        inside: style & 0x0F == 6,
    };
    g.create(ix, Obj::Pen(Rc::new(pen)));
}

/// `EMR_CREATEBRUSHINDIRECT`: handle, then a `LogBrush32` (style, colour, hatch).
fn create_brush(g: &mut Gdi, r: Bytes) {
    let (Some(ix), Some(style), Some(color)) = (r.u32(8), r.u32(12), r.u32(16)) else { return };
    let brush = match style {
        0 => Brush::Solid(colorref(color)),
        1 => Brush::Null,
        _ => Brush::Unsupported,
    };
    g.create(ix, Obj::Brush(Rc::new(brush)));
}

/// `EMR_EXTCREATEFONTINDIRECTW`: handle, then a 92-byte `LogFont` whatever the record's length.
fn create_font(g: &mut Gdi, r: Bytes) {
    let (Some(ix), Some(height), Some(escapement), Some(weight)) = (r.u32(8), r.f64_i32(12), r.f64_i32(20), r.i32(28)) else { return };
    let byte = |at: usize| r.u8(at).unwrap_or(0);
    let units: Vec<u16> = (0..32).map_while(|i| r.u16(40 + 2 * i)).take_while(|u| *u != 0).collect();
    let family: Arc<str> = Arc::from(decode_utf16(&units).into_iter().collect::<String>().trim());
    let font =
        FontObj { family, height, escapement, weight, italic: byte(32) != 0, underline: byte(33) != 0, strike_out: byte(34) != 0, charset: byte(35) };
    g.create(ix, Obj::Font(Rc::new(font)));
}

fn shape_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    match typ {
        43 => {
            if let Some(rect) = r.rect_i32(8) {
                g.rectangle(cv, rect)
            }
        }
        42 => {
            if let Some(rect) = r.rect_i32(8) {
                g.ellipse(cv, rect)
            }
        }
        44 => {
            if let (Some(rect), Some(w), Some(h)) = (r.rect_i32(8), r.f64_i32(24), r.f64_i32(28)) {
                g.round_rect(cv, rect, [w, h])
            }
        }
        45..=47 => {
            if let (Some(rect), Some(s), Some(e)) = (r.rect_i32(8), r.point_i32(24), r.point_i32(32)) {
                let kind = match typ {
                    45 => ArcKind::Arc,
                    46 => ArcKind::Chord,
                    _ => ArcKind::Pie,
                };
                g.arc(cv, rect, s, e, kind)
            }
        }
        27 => {
            if let Some(p) = r.point_i32(8) {
                g.move_to(p)
            }
        }
        54 => {
            if let Some(p) = r.point_i32(8) {
                g.line_to(cv, p)
            }
        }
        15 => {
            if let (Some(p), Some(c)) = (r.point_i32(8), r.u32(16)) {
                set_pixel(g, cv, p, colorref(c))
            }
        }
        _ => return false,
    }
    true
}

/// `EMR_SETPIXELV`: a pixel, drawn as a 1 by 1 device square in the colour.
fn set_pixel(g: &mut Gdi, cv: &mut Canvas, p: P, color: [u8; 4]) {
    use crate::gdi::BLACK;
    let saved = (g.dc.brush.clone(), g.dc.pen.clone());
    g.dc.brush = Rc::new(Brush::Solid(color));
    g.dc.pen = crate::gdi::null_pen();
    g.rectangle(cv, [p[0], p[1], p[0] + 1.0, p[1] + 1.0]);
    (g.dc.brush, g.dc.pen) = saved;
    let _ = BLACK;
}

/// The points of a poly record: `n` pairs of 32-bit or 16-bit integers at `at`. The count is checked against the record's bytes before
/// anything is allocated.
fn points(r: Bytes, at: usize, n: usize, wide: bool) -> Option<Vec<P>> {
    let size = if wide { 8 } else { 4 };
    let raw = r.slice(at, n.checked_mul(size)?)?;
    crate::work::points(n);
    Some(
        raw.chunks_exact(size)
            .map(|c| {
                if wide {
                    [f64::from(i32::from_le_bytes([c[0], c[1], c[2], c[3]])), f64::from(i32::from_le_bytes([c[4], c[5], c[6], c[7]]))]
                } else {
                    [f64::from(i16::from_le_bytes([c[0], c[1]])), f64::from(i16::from_le_bytes([c[2], c[3]]))]
                }
            })
            .collect(),
    )
}

/// Polygons and polylines: count at 24, points at 28; the 16-bit forms are the records from 85 on.
fn poly_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    let wide = matches!(typ, 2..=6);
    let small = matches!(typ, 85..=89);
    if !(wide || small) {
        return poly_groups_record(g, cv, typ, r);
    }
    let Some(n) = r.count(24) else { return true };
    // The points are counted against what is left to keep before a vector is made of them.
    if !cv.budget.has_points(n as u64) {
        return true;
    }
    let Some(pts) = points(r, 28, n, wide) else { return true };
    match typ {
        3 | 86 => g.polygon(cv, &pts),
        4 | 87 => g.polyline(cv, &pts),
        6 | 89 => g.polyline_to(cv, &pts),
        2 | 85 => g.bezier(cv, &pts, true),
        _ => g.bezier(cv, &pts, false),
    }
    true
}

/// POLYPOLYLINE and POLYPOLYGON: the count of figures at 24, of points at 28, the figures' counts, then the points.
fn poly_groups_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    let (wide, close) = match typ {
        7 => (true, false),
        8 => (true, true),
        90 => (false, false),
        91 => (false, true),
        _ => return false,
    };
    let (Some(n), Some(total)) = (r.count(24), r.count(28)) else { return true };
    // The record must hold the counts before anything is allocated for them.
    let Some(raw) = n.checked_mul(4).and_then(|len| r.slice(32, len)) else { return true };
    if !cv.budget.has_points(n as u64) {
        return true;
    }
    let counts: Vec<usize> = raw.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c) as usize).collect();
    let summed = counts.iter().try_fold(0usize, |a, c| a.checked_add(*c));
    if summed != Some(total) || !cv.budget.has_points(total as u64) {
        return true;
    }
    let Some(pts) = points(r, 32 + n * 4, total, wide) else { return true };
    g.poly_groups(cv, &pts, &counts, close);
    true
}

fn path_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    match typ {
        59 => g.begin_path(),
        60 => g.end_path(),
        61 => g.close_figure(),
        62 => g.paint_built(cv, true, false),
        63 => g.paint_built(cv, true, true),
        64 => g.paint_built(cv, false, true),
        68 => {
            // ABORTPATH: the path under construction and the last one are gone.
            g.begin_path();
            g.end_path();
            g.take_built();
        }
        67 => {
            if let Some(mode) = r.u32(8).and_then(region_mode) {
                let done = g.clip_built(cv, mode);
                refused(cv, done, "a path clip");
            }
        }
        _ => return false,
    }
    true
}

/// `RGN_AND` 1, `RGN_OR` 2, `RGN_XOR` 3, `RGN_DIFF` 4, `RGN_COPY` 5.
fn region_mode(m: u32) -> Option<Mode> {
    match m {
        1 => Some(Mode::And),
        2 => Some(Mode::Or),
        3 => Some(Mode::Xor),
        4 => Some(Mode::Exclude),
        5 => Some(Mode::Replace),
        _ => None,
    }
}

fn clip_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    match typ {
        28 => g.set_meta_region(cv),
        29 | 30 => {
            if let Some(rect) = r.rect_i32(8) {
                let mode = if typ == 30 { Mode::And } else { Mode::Exclude };
                let done = g.clip_rects(cv, &[rect], mode);
                refused(cv, done, "a clip");
            }
        }
        75 => ext_select_clip(g, cv, r),
        _ => return false,
    }
    true
}

/// `EMR_EXTSELECTCLIPRGN`: the byte count of the region data, a mode, then `RGNDATA`. Copy with no data resets the clip to the
/// default; other modes without data change nothing. Region data is rectangles in logical units.
fn ext_select_clip(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let (Some(cb), Some(mode)) = (r.count(8), r.u32(12)) else { return };
    let Some(mode) = region_mode(mode) else { return };
    if cb == 0 {
        if mode == Mode::Replace {
            g.reset_clip();
        }
        return;
    }
    // RGNDATAHEADER: size, type (1 = rectangles), count, buffer size, bounds; then the rectangles.
    let (Some(kind), Some(count)) = (r.u32(20), r.count(24)) else { return };
    if kind != 1 {
        return;
    }
    let Some(end) = 16usize.checked_add(cb).filter(|e| *e <= r.len()) else { return };
    let room = end.saturating_sub(48) / 16;
    if count > room || !cv.budget.work(count as u64 + 1) {
        return;
    }
    // A region of more rectangles than a clip may hold the points of is refused before the rectangles are gathered.
    if count.saturating_mul(5) as u64 > MAX_CLIP_POINTS {
        cv.budget.note_cap("clip size");
        return;
    }
    crate::work::region_rects(count);
    let rects: Vec<[f64; 4]> = (0..count).filter_map(|i| r.rect_i32(48 + i * 16)).collect();
    let done = g.clip_rects(cv, &rects, mode);
    refused(cv, done, "a region");
}

/// A clip that could not be combined is reported: as a record skipped when the parts cannot make it, and as a cap when it was too big
/// (which the combination has already noted on the budget).
fn refused(cv: &mut Canvas, done: Result<(), ClipError>, what: &str) {
    if done == Err(ClipError::Cannot) {
        cv.note(Detail::RecordSkipped, || format!("{what} that this crate cannot combine"));
    }
}

// ---- text ----

const ETO_OPAQUE: u32 = 0x2;
const ETO_GLYPH_INDEX: u32 = 0x10;
const ETO_NO_RECT: u32 = 0x100;
const ETO_PDY: u32 = 0x2000;

fn text_record(g: &mut Gdi, cv: &mut Canvas, h: &Header, typ: u32, r: Bytes) -> bool {
    let _ = h;
    match typ {
        84 => ext_text_out(g, cv, r),
        83 | 96 | 97 | 108 | 56 | 92 | 41 | 55 | 120 => {
            cv.note(Detail::RecordSkipped, || format!("EMF record type {typ}"));
        }
        _ => return false,
    }
    true
}

/// `EMR_EXTTEXTOUTW`: bounds, the graphics mode and scales at 24, then an `EmrText` at 36: the reference point, the character count, the
/// string's offset, options, the rectangle (absent under `ETO_NO_RECT`) and the advances' offset.
fn ext_text_out(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let (Some(mode), Some(reference), Some(n), Some(off), Some(opts)) = (r.u32(24), r.point_i32(36), r.count(44), r.count(48), r.u32(52)) else {
        return;
    };
    let (rect, dx_off) = if opts & ETO_NO_RECT != 0 { (None, r.count(56)) } else { (r.rect_i32(56), r.count(72)) };
    if n == 0 || n > MAX_TEXT_CHARS {
        return;
    }
    if opts & ETO_GLYPH_INDEX != 0 {
        cv.note(Detail::GlyphIndexText, || "text of glyph indices in an EMF".to_owned());
        return;
    }
    let Some(units) = r.units(off, n) else { return };
    let chars = decode_utf16(&units);
    // One advance for each code unit, or a pair of them under `ETO_PDY`; the advances of a unit that is the second of a surrogate pair
    // are added to the first.
    let step = if opts & ETO_PDY != 0 { 8 } else { 4 };
    let advances: Option<Vec<f64>> = dx_off.filter(|o| *o != 0).and_then(|o| {
        let raw: Vec<f64> = (0..n).map(|i| r.f64_i32(o.checked_add(i.checked_mul(step)?)?)).collect::<Option<_>>()?;
        Some(merge_pairs(&units, raw))
    });
    let advanced = mode == 2;
    let text = TextOut { reference, chars: &chars, advances: advances.as_deref(), advanced, rect, options: opts };
    let width = g.text(cv, text);
    if let Some(w) = width {
        g.advance_position(w);
    }
    let _ = ETO_OPAQUE;
}

/// The advance of each character: a surrogate pair is two code units with two advances, and one character with their sum.
fn merge_pairs(units: &[u16], raw: Vec<f64>) -> Vec<f64> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let a = raw.get(i).copied().unwrap_or(0.0);
        let is_pair = units.get(i).is_some_and(|u| (0xD800..0xDC00).contains(u)) && units.get(i + 1).is_some_and(|u| (0xDC00..0xE000).contains(u));
        if is_pair {
            out.push(a + raw.get(i + 1).copied().unwrap_or(0.0));
            i += 2;
        } else {
            out.push(a);
            i += 1;
        }
    }
    out
}

// ---- bitmaps ----

fn bitmap_record(g: &mut Gdi, cv: &mut Canvas, typ: u32, r: Bytes) -> bool {
    match typ {
        81 => stretch_dibits(g, cv, r),
        76 | 77 => bit_blt(g, cv, r, typ == 77),
        114 => alpha_blend(g, cv, r),
        80 | 78 | 79 | 116 | 118 => cv.note(Detail::RecordSkipped, || format!("EMF record type {typ}")),
        _ => return false,
    }
    true
}

/// `EMR_STRETCHDIBITS`: destination at 24, source at 32, the bitmap's header and bits as offsets and sizes at 48 and 56, the raster
/// operation at 68, the destination's size at 72.
fn stretch_dibits(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let (Some(dx), Some(dy), Some(sx), Some(sy), Some(sw), Some(sh), Some(rop), Some(dw), Some(dh)) =
        (r.f64_i32(24), r.f64_i32(28), r.f64_i32(32), r.f64_i32(36), r.f64_i32(40), r.f64_i32(44), r.u32(68), r.f64_i32(72), r.f64_i32(76))
    else {
        return;
    };
    blit(g, cv, r, [dx, dy, dw, dh], [sx, sy, sw, sh], (48, 56), Blend::Rop(rop));
}

/// `EMR_BITBLT` and `EMR_STRETCHBLT`: destination at 24, the raster operation at 40, source at 44, header and bits at 84 and 92;
/// `STRETCHBLT` has the source's size at 100. A raster operation that uses no source fills the destination by it instead.
fn bit_blt(g: &mut Gdi, cv: &mut Canvas, r: Bytes, stretch: bool) {
    let (Some(dx), Some(dy), Some(dw), Some(dh), Some(rop), Some(sx), Some(sy)) =
        (r.f64_i32(24), r.f64_i32(28), r.f64_i32(32), r.f64_i32(36), r.u32(40), r.f64_i32(44), r.f64_i32(48))
    else {
        return;
    };
    let has_bitmap = r.count(88).is_some_and(|n| n > 0);
    if !has_bitmap {
        pattern_fill(g, cv, [dx, dy, dx + dw, dy + dh], rop);
        return;
    }
    let (sw, sh) = if stretch { (r.f64_i32(100).unwrap_or(dw), r.f64_i32(104).unwrap_or(dh)) } else { (dw, dh) };
    blit(g, cv, r, [dx, dy, dw, dh], [sx, sy, sw, sh], (84, 92), Blend::Rop(rop));
}

/// A bit block transfer with no bitmap: `PATCOPY` paints the brush, the destination-only operations leave it.
fn pattern_fill(g: &mut Gdi, cv: &mut Canvas, rect: [f64; 4], rop: u32) {
    if rop == DST_NOP {
        return;
    }
    if rop != PATCOPY {
        cv.note(Detail::RecordSkipped, || format!("a bit block transfer by raster operation {rop:#010x}"));
        return;
    }
    let pen = std::mem::replace(&mut g.dc.pen, crate::gdi::null_pen());
    g.rectangle(cv, rect);
    g.dc.pen = pen;
}

/// `EMR_ALPHABLEND`: like `STRETCHBLT`, with a blend function at 40: the constant alpha at byte 2, the alpha format at byte 3.
fn alpha_blend(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let (Some(dx), Some(dy), Some(dw), Some(dh), Some(constant), Some(format), Some(sx), Some(sy), Some(sw), Some(sh)) = (
        r.f64_i32(24),
        r.f64_i32(28),
        r.f64_i32(32),
        r.f64_i32(36),
        r.u8(42),
        r.u8(43),
        r.f64_i32(44),
        r.f64_i32(48),
        r.f64_i32(100),
        r.f64_i32(104),
    ) else {
        return;
    };
    blit(g, cv, r, [dx, dy, dw, dh], [sx, sy, sw, sh], (84, 92), Blend::Alpha { constant, per_pixel: format & 1 != 0 });
}

/// Decodes the bitmap whose header and bits the record names at `at` (the offset and size pairs), and draws the source rectangle into
/// the destination, both `[x, y, width, height]`; a source with no size is the whole bitmap.
fn blit(g: &mut Gdi, cv: &mut Canvas, r: Bytes, dst: [f64; 4], src: [f64; 4], at: (usize, usize), blend: Blend) {
    let (Some(bo), Some(bl), Some(po), Some(pl)) = (r.count(at.0), r.count(at.0 + 4), r.count(at.1), r.count(at.1 + 4)) else { return };
    if pl == 0 {
        return;
    }
    let (Some(info), Some(bits)) = (r.slice(bo, bl), r.slice(po, pl)) else {
        cv.note(Detail::BitmapUndecodable, || "a bitmap whose header or pixels are not in its record".to_owned());
        return;
    };
    let mut img = match dib::decode(info, bits, blend.keeps_alpha(), cv.budget) {
        Ok(img) => img,
        Err(Refusal::TooBig) => {
            cv.budget.note_cap("bitmap size");
            return;
        }
        Err(Refusal::Undecodable) => {
            cv.note(Detail::BitmapUndecodable, || "a bitmap that does not decode".to_owned());
            return;
        }
    };
    if !dib::prepare(&mut img, blend) {
        cv.note(Detail::RecordSkipped, || "a bitmap by a raster operation this crate does not draw".to_owned());
        return;
    }
    // The source rectangle counts rows from the bottom of a bottom-up bitmap; the list counts from the top. A size of 0 is all of it.
    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
    let whole = !(src[2] != 0.0 && src[3] != 0.0);
    let (sx, sw) = if whole { (0.0, iw) } else { (src[0], src[2]) };
    let (sy, sh) = if whole { (0.0, ih) } else { (src[1], src[3]) };
    let top = if img.bottom_up && !whole { ih - sy - sh.abs() } else { sy };
    let (mut x0, mut x1) = (sx, sx + sw);
    let (mut y0, mut y1) = (top, top + sh.abs());
    if sw < 0.0 {
        (x0, x1) = (x1, x0);
    }
    if sh < 0.0 {
        (y0, y1) = (y1, y0);
    }
    let xf = g.to_frame();
    // The destination rectangle with its sign: a negative width or height mirrors the bitmap.
    let ul = xf.apply([dst[0], dst[1]]);
    let ur = xf.apply([dst[0] + dst[2], dst[1]]);
    let ll = xf.apply([dst[0], dst[1] + dst[3]]);
    let item = BitmapItem {
        image: Image::Rgba { width: img.width, height: img.height, data: std::sync::Arc::new(img.rgba) },
        src: Some([x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs()]),
        quad: [ul, ur, ll],
    };
    let mirrored = (x1 < x0, y1 < y0);
    let item = mirror(item, mirrored);
    cv.emit(&g.dc.effective.clone(), Drawn::Bitmap(Box::new(item)), 0);
    let _ = SRCCOPY;
}

/// A bitmap drawn from a source rectangle given backwards is drawn mirrored: its corners are swapped.
fn mirror(mut b: BitmapItem, (flip_x, flip_y): (bool, bool)) -> BitmapItem {
    if flip_x {
        let [ul, ur, ll] = b.quad;
        b.quad = [ur, ul, [ll[0] + (ur[0] - ul[0]), ll[1] + (ur[1] - ul[1])]];
    }
    if flip_y {
        let [ul, ur, ll] = b.quad;
        b.quad = [ll, [ur[0] + (ll[0] - ul[0]), ur[1] + (ll[1] - ul[1])], ul];
    }
    b
}
