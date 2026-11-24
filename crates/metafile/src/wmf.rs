//! Windows metafiles: a header, then records of 16-bit words. A standard WMF has no frame: the window its
//! `META_SETWINDOWORG` and `META_SETWINDOWEXT` records set is the frame, and the records store y before x. A placeable WMF carries a
//! bounding box in logical units and the units to the inch.
//!
//! The mapping mode of a WMF is ignored (project rule), and the EMF copy that a WMF can carry in its escapes is ignored.

use std::rc::Rc;
use std::sync::Arc;

use crate::bytes::Bytes;
use crate::canvas::Canvas;
use crate::clip::{ClipError, Mode};
use crate::dib::{self, Blend, Refusal};
use crate::gdi::{Brush, FontObj, Gdi, Obj, Pen, PenStyle, Surface, TextOut, colorref};
use crate::geom::P;
use crate::limits::{Budget, MAX_TEXT_CHARS};
use crate::model::{BitmapItem, Cap, Detail, Drawn, Image, Join, Kind, Picture};
use crate::{Error, Frame};

const PLACEABLE_KEY: [u8; 4] = [0xD7, 0xCD, 0xC6, 0x9A];

/// Whether the bytes start like a WMF: a standard header of either type with its nine words, or the placeable key.
pub(crate) fn is_wmf(b: &[u8]) -> bool {
    b.starts_with(&[0x01, 0x00, 0x09, 0x00]) || b.starts_with(&[0x02, 0x00, 0x09, 0x00]) || b.starts_with(&PLACEABLE_KEY)
}

/// A placeable record's bounding box (left, top, right, bottom) and units to the inch.
struct Placeable {
    bbox: [f64; 4],
    inch: f64,
}

/// Where the records start, and the placeable record if there is one.
fn header(b: Bytes) -> Result<(usize, Option<Placeable>), Error> {
    let bad = Error::Undecodable("a WMF whose header is cut short");
    let (start, placeable) = if b.slice(0, 4) == Some(&PLACEABLE_KEY[..]) {
        let bbox = [
            f64::from(b.i16(6).ok_or(bad.clone())?),
            f64::from(b.i16(8).ok_or(bad.clone())?),
            f64::from(b.i16(10).ok_or(bad.clone())?),
            f64::from(b.i16(12).ok_or(bad.clone())?),
        ];
        let inch = f64::from(b.u16(14).ok_or(bad.clone())?);
        // Two zero bytes may follow the placeable record: the standard header is then 24 bytes in.
        let at = if b.u16(22) == Some(0) && matches!(b.u16(24), Some(1 | 2)) && b.u16(26) == Some(9) { 24 } else { 22 };
        (at, Some(Placeable { bbox, inch }))
    } else {
        (0, None)
    };
    let kind = b.u16(start).ok_or(bad.clone())?;
    if !matches!(kind, 1 | 2) || b.u16(start + 2) != Some(9) {
        return Err(Error::Undecodable("a WMF whose standard header is not nine words"));
    }
    Ok((start + 18, placeable))
}

/// Reads a WMF to a picture.
pub(crate) fn read(b: Bytes, budget: &mut Budget, _depth: u32) -> Result<Picture, Error> {
    let (first, placeable) = header(b)?;
    // The frame: the placeable box, else the window the records set, else what is drawn.
    // The scan for the window walks as far as the records go, and is charged as the walk is.
    let window = if budget.work(b.len() as u64 / 4 + 1) { scan_window(b, first) } else { None };
    let (frame, win_org, win_ext, known) = match (&placeable, window) {
        (Some(p), _) => {
            let (w, h) = ((p.bbox[2] - p.bbox[0]).abs(), (p.bbox[3] - p.bbox[1]).abs());
            let inch = if p.inch > 0.0 { p.inch } else { 1440.0 };
            let k = 96.0 / inch;
            ([w * k, h * k], [p.bbox[0], p.bbox[1]], [p.bbox[2] - p.bbox[0], p.bbox[3] - p.bbox[1]], w > 0.0 && h > 0.0)
        }
        (None, Some((org, ext))) => ([ext[0].abs(), ext[1].abs()], org, ext, true),
        (None, None) => ([1.0, 1.0], [0.0, 0.0], [1.0, 1.0], false),
    };
    let mut gdi = Gdi::new(Surface::Wmf { frame }, usize::MAX, 96.0);
    gdi.dc.win_org = win_org;
    gdi.dc.win_ext = if win_ext[0] != 0.0 && win_ext[1] != 0.0 { win_ext } else { [1.0, 1.0] };
    let mut cv = Canvas::new(budget);
    let cut_short = walk(b, first, &mut gdi, &mut cv);
    if cut_short {
        cv.note(Detail::CutShort, || "a WMF record whose size is wrong".to_owned());
    }
    let (items, clips, report) = cv.finish();
    Ok(crate::assemble(Kind::Wmf, Frame { width: frame[0], height: frame[1], known }, items, clips, report))
}

/// The window the records set, the first of each (they store y before x), if both are set.
fn scan_window(b: Bytes, first: usize) -> Option<(P, P)> {
    let (mut org, mut ext) = (None, None);
    let mut pos = first;
    for _ in 0..crate::limits::MAX_RECORDS {
        let (Some(size), Some(func)) = (b.count(pos), b.u16(pos + 4)) else { break };
        let len = size.checked_mul(2).filter(|l| *l >= 6)?;
        if pos.checked_add(len)? > b.len() {
            break;
        }
        match func {
            0x020B if org.is_none() => org = Some([f64::from(b.i16(pos + 8)?), f64::from(b.i16(pos + 6)?)]),
            0x020C if ext.is_none() => ext = Some([f64::from(b.i16(pos + 8)?), f64::from(b.i16(pos + 6)?)]),
            0 => break,
            _ => {}
        }
        if org.is_some() && ext.is_some() {
            break;
        }
        pos += len;
    }
    match (org, ext) {
        (Some(o), Some(e)) if e[0] != 0.0 && e[1] != 0.0 => Some((o, e)),
        // A window with an origin only has the origin as its corner: the frame is what is drawn.
        _ => None,
    }
}

/// The walk: records of a size in words, a function and parameters. Returns whether it was cut short.
fn walk(b: Bytes, first: usize, g: &mut Gdi, cv: &mut Canvas) -> bool {
    let mut pos = first;
    loop {
        if pos >= b.len() || !cv.budget.alive() {
            return false;
        }
        let (Some(words), Some(func)) = (b.count(pos), b.u16(pos + 4)) else { return true };
        let len = words.checked_mul(2);
        let Some(rec) = len.filter(|l| *l >= 6).and_then(|l| b.sub(pos, l)) else { return true };
        if !cv.budget.record() || !cv.budget.work(rec.len() as u64 / 4 + 1) {
            return false;
        }
        pos += rec.len();
        if func == 0 {
            return false;
        }
        record(g, cv, func, rec);
    }
}

/// A point stored y then x, 16 bits each, at `at`.
fn yx(r: Bytes, at: usize) -> Option<P> {
    Some([f64::from(r.i16(at + 2)?), f64::from(r.i16(at)?)])
}

/// A rectangle stored bottom, right, top, left, with the last parameter first: parameters at `at` are bottom, right, top, left in file
/// order, so the rectangle is (left, top, right, bottom).
fn rect(r: Bytes, at: usize) -> Option<[f64; 4]> {
    Some([f64::from(r.i16(at + 6)?), f64::from(r.i16(at + 4)?), f64::from(r.i16(at + 2)?), f64::from(r.i16(at)?)])
}

fn record(g: &mut Gdi, cv: &mut Canvas, func: u16, r: Bytes) {
    if state(g, cv, func, r) || objects(g, func, r) || shapes(g, cv, func, r) || other(g, cv, func, r) {
        return;
    }
    // Escapes, palettes, and the settings that change nothing drawn.
    if matches!(func, 0x0626 | 0x0107 | 0x0035 | 0x0234 | 0x00F7 | 0x0037 | 0x0103 | 0x020D | 0x020E) {
        return;
    }
    cv.note(Detail::RecordSkipped, || format!("WMF function {func:#06x}"));
}

fn state(g: &mut Gdi, cv: &mut Canvas, func: u16, r: Bytes) -> bool {
    let word = |at: usize| r.u16(at).map(u32::from);
    match func {
        0x001E => g.save(cv),
        0x0127 => {
            if let Some(n) = r.i16(6) {
                g.restore(i32::from(n))
            }
        }
        0x0102 => {
            if let Some(m) = word(6).filter(|m| matches!(m, 1 | 2)) {
                g.dc.bk_mode = m
            }
        }
        0x0106 => {
            if let Some(m) = word(6).filter(|m| matches!(m, 1 | 2)) {
                g.dc.poly_fill = m
            }
        }
        0x0104 => {
            if let Some(m) = word(6) {
                g.dc.rop2 = m
            }
        }
        0x012E => {
            if let Some(a) = word(6) {
                g.dc.text_align = a
            }
        }
        0x0209 => {
            if let Some(c) = r.u32(6) {
                g.dc.text_color = colorref(c)
            }
        }
        0x0201 => {
            if let Some(c) = r.u32(6) {
                g.dc.bk_color = colorref(c)
            }
        }
        0x020B => {
            if let Some(p) = yx(r, 6) {
                g.set_window_org(p)
            }
        }
        0x020C => {
            if let Some(p) = yx(r, 6) {
                g.set_window_ext(p)
            }
        }
        _ => return false,
    }
    true
}

fn objects(g: &mut Gdi, func: u16, r: Bytes) -> bool {
    match func {
        0x012D => {
            if let Some(i) = r.u16(6) {
                g.select(u32::from(i))
            }
        }
        0x01F0 => {
            if let Some(i) = r.u16(6) {
                g.delete(u32::from(i))
            }
        }
        0x02FA => pen(g, r),
        0x02FC => brush(g, r),
        0x02FB => font(g, r),
        // A pattern brush is not drawn (the fill that uses it is reported); a region and a palette are taken as objects that are not drawn with.
        0x0142 => g.create_next(Obj::Brush(Rc::new(Brush::Unsupported))),
        0x06FF | 0x00F7 => g.create_next(Obj::Other),
        _ => return false,
    }
    true
}

/// `META_CREATEPENINDIRECT`: a style, a width as a point, a colour.
fn pen(g: &mut Gdi, r: Bytes) {
    let (Some(style), Some(width), Some(color)) = (r.u16(6), r.i16(8), r.u32(12)) else { return };
    let width = f64::from(width);
    let dash = match style & 0x0F {
        1 => PenStyle::Dash,
        2 => PenStyle::Dot,
        3 => PenStyle::DashDot,
        4 => PenStyle::DashDotDot,
        _ => PenStyle::Solid,
    };
    let pen = Pen {
        null: style & 0x0F == 5,
        cosmetic: width <= 1.0,
        width,
        color: colorref(color),
        dash,
        cap: Cap::Round,
        join: Join::Round,
        inside: style & 0x0F == 6,
    };
    g.create_next(Obj::Pen(Rc::new(pen)));
}

/// `META_CREATEBRUSHINDIRECT`: a style, a colour, a hatch.
fn brush(g: &mut Gdi, r: Bytes) {
    let (Some(style), Some(color)) = (r.u16(6), r.u32(8)) else { return };
    let brush = match style {
        0 => Brush::Solid(colorref(color)),
        1 => Brush::Null,
        _ => Brush::Unsupported,
    };
    g.create_next(Obj::Brush(Rc::new(brush)));
}

/// `META_CREATEFONTINDIRECT`: a `LogFont` of 16-bit sizes and a face name in single bytes.
fn font(g: &mut Gdi, r: Bytes) {
    let (Some(height), Some(escapement), Some(weight)) = (r.i16(6), r.i16(10), r.i16(14)) else { return };
    let byte = |at: usize| r.u8(at).unwrap_or(0);
    let name: String = r.0.get(24..).unwrap_or(&[]).iter().take(32).take_while(|c| **c != 0).map(|c| char::from(*c)).collect();
    let font = FontObj {
        family: Arc::from(name.trim()),
        height: f64::from(height),
        escapement: f64::from(escapement),
        weight: i32::from(weight),
        italic: byte(16) != 0,
        underline: byte(17) != 0,
        strike_out: byte(18) != 0,
        charset: byte(19),
    };
    g.create_next(Obj::Font(Rc::new(font)));
}

/// Points of 16-bit pairs: a count, then the points.
fn points(r: Bytes, at: usize) -> Option<Vec<P>> {
    let n = usize::try_from(r.i16(at)?).ok()?;
    let raw = r.slice(at + 2, n.checked_mul(4)?)?;
    Some(raw.as_chunks::<4>().0.iter().map(|c| [f64::from(i16::from_le_bytes([c[0], c[1]])), f64::from(i16::from_le_bytes([c[2], c[3]]))]).collect())
}

fn shapes(g: &mut Gdi, cv: &mut Canvas, func: u16, r: Bytes) -> bool {
    match func {
        0x0214 => {
            if let Some(p) = yx(r, 6) {
                g.move_to(p)
            }
        }
        0x0213 => {
            if let Some(p) = yx(r, 6) {
                g.line_to(cv, p)
            }
        }
        0x0324 => {
            if let Some(pts) = points(r, 6) {
                g.polygon(cv, &pts)
            }
        }
        0x0325 => {
            if let Some(pts) = points(r, 6) {
                g.polyline(cv, &pts)
            }
        }
        0x0538 => poly_polygon(g, cv, r),
        0x041B => {
            if let Some(rc) = rect(r, 6) {
                g.rectangle(cv, rc)
            }
        }
        0x0418 => {
            if let Some(rc) = rect(r, 6) {
                g.ellipse(cv, rc)
            }
        }
        _ => return false,
    }
    true
}

/// `META_POLYPOLYGON`: a count of polygons, the points of each, then the points.
fn poly_polygon(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let Some(n) = r.u16(6).map(usize::from) else { return };
    let Some(raw) = r.slice(8, n * 2) else { return };
    let counts: Vec<usize> = raw.as_chunks::<2>().0.iter().map(|c| usize::from(u16::from_le_bytes(*c))).collect();
    let total: usize = counts.iter().sum();
    let Some(bytes) = total.checked_mul(4).and_then(|len| r.slice(8 + n * 2, len)) else { return };
    if !cv.budget.has_points(total as u64) {
        return;
    }
    let pts: Vec<P> =
        bytes.as_chunks::<4>().0.iter().map(|c| [f64::from(i16::from_le_bytes([c[0], c[1]])), f64::from(i16::from_le_bytes([c[2], c[3]]))]).collect();
    g.poly_groups(cv, &pts, &counts, true);
}

fn other(g: &mut Gdi, cv: &mut Canvas, func: u16, r: Bytes) -> bool {
    match func {
        0x0416 => {
            if let Some(rc) = rect(r, 6)
                && g.clip_rects(cv, &[rc], Mode::And) == Err(ClipError::Cannot)
            {
                cv.note(Detail::RecordSkipped, || "a clip this crate cannot combine".to_owned());
            }
        }
        0x0A32 => ext_text_out(g, cv, r),
        0x0940 | 0x0B41 => dib_blit(g, cv, r, func == 0x0B41),
        0x012C => cv.note(Detail::RecordSkipped, || "META_SELECTCLIPREGION".to_owned()),
        _ => return false,
    }
    true
}

/// `META_EXTTEXTOUT`: y, x, a count, options, a rectangle if the options say, the string (padded to a word), and advances.
fn ext_text_out(g: &mut Gdi, cv: &mut Canvas, r: Bytes) {
    let (Some(reference), Some(n), Some(options)) = (yx(r, 6), r.i16(10).and_then(|n| usize::try_from(n).ok()), r.u16(12).map(u32::from)) else {
        return;
    };
    if n == 0 || n > MAX_TEXT_CHARS {
        return;
    }
    let has_rect = options & 0x6 != 0;
    let rect_at = 14usize;
    let rect = if has_rect { crate::wmf::rect(r, rect_at) } else { None };
    let text_at = if has_rect { rect_at + 8 } else { rect_at };
    let Some(bytes) = r.slice(text_at, n) else { return };
    // The string is in single bytes (a code page; Latin-1 here), padded to a word.
    let chars: Vec<char> = bytes.iter().map(|c| char::from(*c)).collect();
    let advances: Option<Vec<f64>> = {
        let at = text_at + n + n % 2;
        (0..n).map(|i| r.i16(at + i * 2).map(f64::from)).collect()
    };
    // The record's options use ETO_OPAQUE 0x2 and ETO_CLIPPED 0x4 as EMF's do.
    let text = TextOut { reference, chars: &chars, advances: advances.as_deref(), advanced: false, rect, options };
    let width = g.text(cv, text);
    if let Some(w) = width {
        g.advance_position(w);
    }
}

/// `META_DIBBITBLT` and `META_DIBSTRETCHBLT`: a raster operation, the source and destination rectangles in 16 bits, then a packed bitmap
/// A `DIBBITBLT` with no bitmap fills nothing here.
fn dib_blit(g: &mut Gdi, cv: &mut Canvas, r: Bytes, stretch: bool) {
    let Some(rop) = r.u32(6) else { return };
    let get = |i: usize| r.i16(10 + i * 2).map(f64::from);
    let (dst, src, dib_at) = if stretch {
        let (Some(sh), Some(sw), Some(sy), Some(sx), Some(dh), Some(dw), Some(dy), Some(dx)) =
            (get(0), get(1), get(2), get(3), get(4), get(5), get(6), get(7))
        else {
            return;
        };
        ([dx, dy, dw, dh], [sx, sy, sw, sh], 26)
    } else {
        let (Some(sy), Some(sx), Some(h), Some(w), Some(dy), Some(dx)) = (get(0), get(1), get(2), get(3), get(4), get(5)) else { return };
        ([dx, dy, w, h], [sx, sy, w, h], 22)
    };
    let Some(packed) = r.0.get(dib_at..).filter(|d| d.len() > 40) else { return };
    let Some(info_len) = dib::packed_info_len(packed) else {
        cv.note(Detail::BitmapUndecodable, || "a WMF bitmap whose header does not read".to_owned());
        return;
    };
    let (info, bits) = packed.split_at(info_len.min(packed.len()));
    let mut img = match dib::decode(info, bits, false, cv.budget) {
        Ok(i) => i,
        Err(Refusal::TooBig) => {
            cv.budget.note_cap("bitmap size");
            return;
        }
        Err(Refusal::Undecodable) => {
            cv.note(Detail::BitmapUndecodable, || "a WMF bitmap that does not decode".to_owned());
            return;
        }
    };
    if !dib::prepare(&mut img, Blend::Rop(rop)) {
        cv.note(Detail::RecordSkipped, || "a bitmap by a raster operation this crate does not draw".to_owned());
        return;
    }
    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
    let whole = !(src[2] > 0.0 && src[3] > 0.0);
    let (sx, sw) = if whole { (0.0, iw) } else { (src[0], src[2]) };
    let sy = if whole {
        0.0
    } else if img.bottom_up {
        ih - src[1] - src[3]
    } else {
        src[1]
    };
    let sh = if whole { ih } else { src[3] };
    let xf = g.to_frame();
    let quad = [xf.apply([dst[0], dst[1]]), xf.apply([dst[0] + dst[2], dst[1]]), xf.apply([dst[0], dst[1] + dst[3]])];
    let item = BitmapItem {
        image: Image::Rgba { width: img.width, height: img.height, data: std::sync::Arc::new(img.rgba) },
        src: Some([sx, sy, sw, sh]),
        quad,
    };
    cv.emit(&g.dc.effective.clone(), Drawn::Bitmap(Box::new(item)), 0);
}
