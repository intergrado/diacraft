//! Device-independent bitmaps: a `BITMAPINFOHEADER` (or a later version), an optional colour table or set of masks, and
//! rows of pixels, at 1, 4, 8, 16, 24 and 32 bits, uncompressed or by bit fields. They decode to straight RGBA, rows from the top.
//! Run-length, JPEG and PNG bitmaps are not decoded: none occurs in files.
//!
//! Adapted from DeckCraft's `crates/metafile/src/dib.rs` (see `docs/provenance.md`): the header reading and the pixel formats follow it;
//! the caps are the spec's, every size is checked against the bytes the record holds before anything is allocated, and the raster operations
//! are the spec's.

use crate::bytes::Bytes;
use crate::limits::{Budget, MAX_BITMAP_PIXELS, MAX_BITMAP_SIDE};

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

pub(crate) const SRCCOPY: u32 = 0x00CC_0020;
pub(crate) const NOTSRCCOPY: u32 = 0x0033_0008;
pub(crate) const SRCAND: u32 = 0x0088_00C6;
pub(crate) const SRCPAINT: u32 = 0x00EE_0086;
pub(crate) const SRCINVERT: u32 = 0x0066_0046;
pub(crate) const PATCOPY: u32 = 0x00F0_0021;
/// A raster operation that leaves the destination as it is.
pub(crate) const DST_NOP: u32 = 0x00AA_0029;

/// Why a bitmap was not decoded.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The header, the colour table or the pixels are not all there, or the format is not one this crate decodes.
    Undecodable,
    /// A size past a cap.
    TooBig,
}

/// A decoded bitmap: straight RGBA, rows from the top.
pub(crate) struct Decoded {
    pub width: u32,
    pub height: u32,
    /// The bitmap was stored bottom-up, which source rectangles in bitmap records count from.
    pub bottom_up: bool,
    pub rgba: Vec<u8>,
}

struct Header {
    w: u32,
    h: u32,
    top_down: bool,
    bpp: u32,
    masks: [u32; 3],
    pal_off: usize,
    pal_n: usize,
}

impl Header {
    fn read(info: Bytes) -> Result<Header, Refusal> {
        let bad = || Refusal::Undecodable;
        let size = info.count(0).ok_or_else(bad)?;
        let w = info.i32(4).ok_or_else(bad)?;
        let h = info.i32(8).ok_or_else(bad)?;
        let bpp = u32::from(info.u16(14).ok_or_else(bad)?);
        let comp = info.u32(16).ok_or_else(bad)?;
        let clr_used = info.count(32).ok_or_else(bad)?;
        if size < 40 || w <= 0 || h == 0 || !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) || !matches!(comp, BI_RGB | BI_BITFIELDS) {
            return Err(Refusal::Undecodable);
        }
        let (w, height) = (w.unsigned_abs(), h.unsigned_abs());
        if w > MAX_BITMAP_SIDE || height > MAX_BITMAP_SIDE || u64::from(w) * u64::from(height) > MAX_BITMAP_PIXELS {
            return Err(Refusal::TooBig);
        }
        let masks = if comp == BI_BITFIELDS {
            [info.u32(40).ok_or_else(bad)?, info.u32(44).ok_or_else(bad)?, info.u32(48).ok_or_else(bad)?]
        } else if bpp == 16 {
            [0x7C00, 0x03E0, 0x001F]
        } else {
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF]
        };
        // Masks follow a 40-byte header as three words; a later header holds them inside.
        let masks_len = if comp == BI_BITFIELDS && size == 40 { 12 } else { 0 };
        let pal_off = size.checked_add(masks_len).ok_or_else(bad)?;
        let pal_n = match (bpp <= 8, clr_used) {
            (true, 0) => 1usize << bpp,
            (true, n) => n.min(1usize << bpp),
            // Deeper formats may carry a colour table the pixels do not use.
            (false, _) => 0,
        };
        Ok(Header { w, h: height, top_down: h < 0, bpp, masks, pal_off, pal_n })
    }
}

/// How many bytes the info of a packed bitmap takes (its header, masks and colour table), so that the pixels follow: a WMF stores the bitmap
/// as one run of bytes.
pub(crate) fn packed_info_len(dib: &[u8]) -> Option<usize> {
    let b = Bytes(dib);
    let size = b.count(0)?;
    let bpp = u32::from(b.u16(14)?);
    let comp = b.u32(16)?;
    let clr_used = b.count(32)?;
    if size < 40 {
        return None;
    }
    let masks_len = if comp == BI_BITFIELDS && size == 40 { 12 } else { 0 };
    let n = match (bpp <= 8, clr_used) {
        (true, 0) => 1usize << bpp.min(8),
        (true, n) => n.min(1usize << bpp.min(8)),
        (false, n) => n.min(256),
    };
    size.checked_add(masks_len)?.checked_add(n.checked_mul(4)?)
}

/// Decodes a bitmap given as its info (header, masks, colour table) and its pixel bits. The 32-bit fourth byte is alpha only when
/// `keep_alpha` is set; otherwise pixels are opaque (the rule for BMP files, and Windows documents the byte as unused).
pub(crate) fn decode(info: &[u8], bits: &[u8], keep_alpha: bool, budget: &mut Budget) -> Result<Decoded, Refusal> {
    let info = Bytes(info);
    let hd = Header::read(info)?;
    let pixels = u64::from(hd.w) * u64::from(hd.h);
    // The pixels the bytes hold are checked before anything is allocated for the pixels the header claims.
    let stride = (usize::try_from(hd.w).map_err(|_| Refusal::TooBig)?)
        .checked_mul(hd.bpp as usize)
        .and_then(|v| v.checked_add(31))
        .map(|v| v / 32 * 4)
        .ok_or(Refusal::TooBig)?;
    let rows = hd.h as usize;
    let needed = stride.checked_mul(rows).ok_or(Refusal::TooBig)?;
    let data = bits.get(..needed).ok_or(Refusal::Undecodable)?;
    let palette = if hd.pal_n == 0 { &[][..] } else { info.slice(hd.pal_off, hd.pal_n * 4).ok_or(Refusal::Undecodable)? };
    if !budget.pixels(pixels) || !budget.work(pixels / 4 + 1) {
        return Err(Refusal::TooBig);
    }
    let width = hd.w as usize;
    let mut rgba = vec![0u8; usize::try_from(pixels * 4).map_err(|_| Refusal::TooBig)?];
    for y in 0..rows {
        let src_row = if hd.top_down { y } else { rows - 1 - y };
        let line = data.get(src_row * stride..(src_row + 1) * stride).ok_or(Refusal::Undecodable)?;
        let out = rgba.get_mut(y * width * 4..(y + 1) * width * 4).ok_or(Refusal::Undecodable)?;
        for (x, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let p = pixel(line, x, &hd, palette, keep_alpha).ok_or(Refusal::Undecodable)?;
            px.copy_from_slice(&p);
        }
    }
    Ok(Decoded { width: hd.w, height: hd.h, bottom_up: !hd.top_down, rgba })
}

fn pixel(line: &[u8], x: usize, hd: &Header, pal: &[u8], keep_alpha: bool) -> Option<[u8; 4]> {
    let [m_r, m_g, m_b] = hd.masks;
    let rgb = match hd.bpp {
        1 => palette(pal, usize::from((*line.get(x / 8)? >> (7 - x % 8)) & 1)),
        4 => {
            let b = *line.get(x / 2)?;
            palette(pal, usize::from(if x.is_multiple_of(2) { b >> 4 } else { b & 0x0F }))
        }
        8 => palette(pal, usize::from(*line.get(x)?)),
        24 => {
            let [b, g, r] = <[u8; 3]>::try_from(line.get(x * 3..x * 3 + 3)?).ok()?;
            [r, g, b]
        }
        16 => {
            let v = u32::from(u16::from_le_bytes(line.get(x * 2..x * 2 + 2)?.try_into().ok()?));
            [field(v, m_r), field(v, m_g), field(v, m_b)]
        }
        _ => {
            let v = u32::from_le_bytes(line.get(x * 4..x * 4 + 4)?.try_into().ok()?);
            let a = if keep_alpha { (v >> 24) as u8 } else { 255 };
            return Some([field(v, m_r), field(v, m_g), field(v, m_b), a]);
        }
    };
    Some([rgb[0], rgb[1], rgb[2], 255])
}

/// Palette entries are blue, green, red, unused; an index past the table reads as black rather than failing the bitmap.
fn palette(pal: &[u8], i: usize) -> [u8; 3] {
    pal.get(i * 4..i * 4 + 4).map_or([0, 0, 0], |c| [c[2], c[1], c[0]])
}

/// One channel of a masked pixel, scaled to 8 bits.
fn field(v: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = u64::from(mask >> shift);
    let val = u64::from((v & mask) >> shift);
    ((val * 255 + max / 2) / max.max(1)).min(255) as u8
}

/// How a bitmap meets what is under it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Blend {
    /// A raster operation.
    Rop(u32),
    /// `EMR_ALPHABLEND`: a constant alpha, times the per-pixel alpha when `per_pixel` (the colours are premultiplied by it).
    Alpha { constant: u8, per_pixel: bool },
}

impl Blend {
    pub fn keeps_alpha(self) -> bool {
        matches!(self, Blend::Alpha { per_pixel: true, .. })
    }
}

/// Applies a blend to decoded pixels. The picture has no destination pixels, so the raster operations that combine with them are
/// drawn as they look over white paper: `SRCAND` leaves white pixels out and draws the rest, `SRCPAINT` and `SRCINVERT`
/// leave black pixels out. Returns false for a raster operation this crate does not draw.
pub(crate) fn prepare(img: &mut Decoded, blend: Blend) -> bool {
    match blend {
        Blend::Rop(SRCCOPY) => true,
        Blend::Rop(SRCAND) => {
            clear_where(img, [255, 255, 255]);
            true
        }
        Blend::Rop(SRCPAINT | SRCINVERT) => {
            clear_where(img, [0, 0, 0]);
            true
        }
        Blend::Rop(NOTSRCCOPY) => {
            for p in img.rgba.as_chunks_mut::<4>().0 {
                for c in p.iter_mut().take(3) {
                    *c = 255 - *c;
                }
            }
            true
        }
        Blend::Rop(_) => false,
        Blend::Alpha { constant, per_pixel } => {
            for p in img.rgba.as_chunks_mut::<4>().0 {
                if per_pixel {
                    unpremultiply(p);
                }
                if let Some(a) = p.get_mut(3) {
                    *a = (u16::from(*a) * u16::from(constant) / 255) as u8;
                }
            }
            true
        }
    }
}

/// Makes the pixels of colour `rgb` transparent: where a raster operation keeps what is under them.
fn clear_where(img: &mut Decoded, rgb: [u8; 3]) {
    for p in img.rgba.as_chunks_mut::<4>().0 {
        if p[..3] == rgb {
            p[3] = 0;
        }
    }
}

/// Sources of `AC_SRC_ALPHA` are premultiplied; the list holds straight pixels, so divide the colour by the alpha.
fn unpremultiply(p: &mut [u8]) {
    let a = u32::from(p.get(3).copied().unwrap_or(255));
    if a == 0 || a == 255 {
        return;
    }
    for c in p.iter_mut().take(3) {
        *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
    }
}
