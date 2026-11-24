//! Pictures: `Foreign` shapes, decoded from the bytes of their part and drawn clipped to their frame.
//!
//! * **The decoder is chosen from the bytes**, not the label: a part typed GIF may hold PNG, and one typed EMF may hold WMF.
//! * **Where.** The box runs from (`ImgOffsetX`, `ImgOffsetY`) by `ImgWidth` and `ImgHeight` in the shape's own coordinates, so it moves,
//!   turns and flips with the shape; the whole image is stretched to it, and its first row is along the upper edge.
//! * **Clip.** The picture is cut to the frame, or to the section `ClippingPath` names.
//! * **Zero.** A box or a frame of no size draws nothing and says nothing.
//! * **Metafiles** (EMF, EMF+, WMF) are read by `diacraft-metafile` to a display list and drawn by `metafile.rs`; one whose header no reader
//!   takes is a placeholder and says so.
//! * **Ink.** A shape marked as ink draws its fallback picture, or nothing with a warning.
//! * **Hostile bytes.** Dimensions are read from the header and checked before any pixel is allocated; a picture past the limits is a
//!   placeholder; a decoder that panics is caught and the picture is a placeholder.
//!
//! The adjustments (`Brightness`, `Contrast`, `Gamma`, `Blur`, `Sharpen`, `Denoise`, `Transparency`) are not applied.

use std::collections::HashMap;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::{ForeignData, Media, ShapeType};
use kurbo::{Affine, BezPath, Rect, Shape as _};
use vello_cpu::Pixmap;
use vello_cpu::color::PremulRgba8;

use crate::frame::{Frame, ShapeCtx};
use crate::geometry::ShapeGeometry;
use crate::limits::{BYTES_PER_WORK, MAX_PICTURE_BYTES, MAX_PICTURE_PIXELS, MAX_PICTURE_SIDE, PIXELS_PER_WORK};
use crate::metafile::{DecodeRoom, MetaPicture};
use crate::skips::SkipKind;
use crate::target::{Paint, Refused, Target};

/// The decoders' guess from the first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    Emf,
    Wmf,
    Unknown,
}

pub(crate) fn sniff(bytes: &[u8]) -> Format {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Format::Png
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Format::Jpeg
    } else if bytes.starts_with(b"BM") {
        Format::Bmp
    } else if bytes.starts_with(b"GIF8") {
        Format::Gif
    } else if bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        Format::Tiff
    } else if bytes.get(40..44) == Some(b" EMF".as_slice()) {
        Format::Emf
    } else if bytes.starts_with(&[0x01, 0x00, 0x09, 0x00])
        || bytes.starts_with(&[0x02, 0x00, 0x09, 0x00])
        || bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
    {
        Format::Wmf
    } else {
        Format::Unknown
    }
}

/// What a part decodes to.
#[derive(Clone)]
pub(crate) enum Decoded {
    Raster(Arc<Pixmap>),
    /// An EMF, EMF+ or WMF, read: drawn by `metafile.rs`.
    Meta(Arc<MetaPicture>),
    /// An EMF or WMF whose header no reader takes: drawn as a placeholder.
    Metafile,
    /// Bytes that could not be decoded, with the reason in words.
    Undecodable(String),
}

/// Decoded pictures kept between frames, by the part's identity.
pub(crate) struct Pictures {
    map: HashMap<usize, (Arc<Media>, Decoded)>,
    /// Bytes held: for each entry, its part's bytes (which the entry keeps alive) and, for a raster, its decoded pixels.
    held: u64,
    max_pictures: usize,
    max_bytes: u64,
    /// Images decoded to pixels since the cache was made, however many frames it took and whatever was let go since: the pictures that
    /// are rasters and the bitmaps of metafile pictures. A count of work done, for a test or a status line to compare from frame to frame.
    decoded: u64,
}

/// The most pictures kept, and the most bytes: pixels decoded, and the parts' own bytes that the entries keep alive.
/// The pale grey a placeholder box is filled with (a picture that is not drawn, an object that cannot be: `placeholder.rs`).
pub(crate) const PLACEHOLDER_FILL: Rgba = Rgba::rgb(232, 232, 232);

const MAX_KEPT_PICTURES: usize = 64;
const MAX_KEPT_BYTES: u64 = 512 * 1024 * 1024;

impl Default for Pictures {
    fn default() -> Pictures {
        Pictures::with_limits(MAX_KEPT_PICTURES, MAX_KEPT_BYTES)
    }
}

impl Pictures {
    fn with_limits(max_pictures: usize, max_bytes: u64) -> Pictures {
        Pictures { map: HashMap::new(), held: 0, max_pictures, max_bytes, decoded: 0 }
    }

    /// How many images have been decoded to pixels since the cache was made (it does not go back to 0 when the cache is cleared).
    pub(crate) fn decoded(&self) -> u64 {
        self.decoded
    }

    /// Whether the decoded picture of `media` is kept: finding it costs no decode.
    pub(crate) fn holds(&self, media: &Arc<Media>) -> bool {
        self.map.contains_key(&(Arc::as_ptr(media) as usize))
    }

    /// Drops every picture, and with them the parts they hold on to.
    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.held = 0;
    }

    /// How many pictures are kept.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }

    /// What an entry holds: the bytes of its part, which it keeps alive whatever it decoded to (an EMF, a part of no picture format and a
    /// picture that does not decode are all kept as their raw bytes and nothing else), and a raster's pixels besides.
    fn weight(media: &Media, decoded: &Decoded) -> u64 {
        let pixels = match decoded {
            Decoded::Raster(p) => u64::from(p.width()) * u64::from(p.height()) * 4,
            Decoded::Meta(m) => m.cost(),
            _ => 0,
        };
        (media.bytes.len() as u64).saturating_add(pixels)
    }

    /// The decoded picture of a part, kept. `room` is what the frame has for decoding a metafile's bitmaps (the time left, and one forced
    /// decode for the whole frame): they are decoded while it lasts, the first not yet decoded first. A picture the room did not suffice
    /// for is kept all the same, with the bitmaps decoded so far, and each frame that asks for it again goes on from where the last stopped
    /// (and after a few frames what is left is marked as not drawn).
    ///
    /// **What the cache holds cannot grow by decoding.** A picture is weighed by the most it can come to (its parse, its pixmaps, and for
    /// each image still waiting its bytes and four bytes a pixel of the size its header gives: [`MetaPicture::cost`]), and a decode replaces
    /// the last by something no bigger. So the check at insertion covers all the work that follows, and the work on a kept entry only lowers
    /// the weight.
    fn get(&mut self, media: &Arc<Media>, room: &mut DecodeRoom) -> Decoded {
        let key = Arc::as_ptr(media) as usize;
        if let Some((_, known)) = self.map.get_mut(&key) {
            if let Decoded::Meta(meta) = known
                && !meta.is_complete()
                // Nothing else holds the picture between two frames; a frame that does still has its own copy and goes on next time.
                && let Some(inner) = Arc::get_mut(meta)
            {
                let (before, decoded_before) = (inner.cost(), inner.decodes());
                inner.work(room);
                let after = inner.cost();
                self.decoded = self.decoded.saturating_add(inner.decodes().saturating_sub(decoded_before));
                self.held = self.held.saturating_sub(before.saturating_sub(after));
            }
            return known.clone();
        }
        let mut decoded = decode(&media.bytes, room);
        self.decoded = self.decoded.saturating_add(match &decoded {
            Decoded::Raster(_) => 1,
            Decoded::Meta(meta) => meta.decodes(),
            Decoded::Metafile | Decoded::Undecodable(_) => 0,
        });
        let bytes = Self::weight(media, &decoded);
        // An entry that alone is more than all the cache may hold is not kept: it is decoded again when it is next asked for, which for
        // such a part is a length check or a refusal.
        if bytes > self.max_bytes {
            // Not kept, so no later frame goes on with it: what is waiting is marked as not drawn, and the picture is whole this once.
            if let Decoded::Meta(meta) = &mut decoded
                && let Some(inner) = Arc::get_mut(meta)
            {
                inner.give_up("the picture is too big to keep between frames");
            }
            return decoded;
        }
        if self.map.len() >= self.max_pictures || self.held.saturating_add(bytes) > self.max_bytes {
            self.clear();
        }
        self.held = self.held.saturating_add(bytes);
        // The `Arc<Media>` is kept so that its address cannot be taken by another part while the entry exists.
        self.map.insert(key, (Arc::clone(media), decoded.clone()));
        decoded
    }
}

/// An image decoded to straight 8-bit RGBA.
pub(crate) struct Raw {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// The work of decoding a part of `bytes`, counted before it is decoded: a raster by the pixels its header gives (a header past what a picture decodes
/// to is refused at once, and costs its bytes), and anything else by its bytes, which a metafile is read from. Its bytes at the least.
pub(crate) fn decode_work(bytes: &[u8]) -> u64 {
    let read = (bytes.len() as u64).div_ceil(BYTES_PER_WORK);
    let pixels = match sniff(bytes) {
        Format::Png | Format::Jpeg | Format::Bmp => header_size(bytes).map_or(0, |(w, h)| u64::from(w).saturating_mul(u64::from(h))),
        _ => 0,
    };
    let decode = if pixels <= MAX_PICTURE_PIXELS { pixels.div_ceil(PIXELS_PER_WORK) } else { 0 };
    read.max(decode).max(1)
}

fn decode(bytes: &[u8], room: &mut DecodeRoom) -> Decoded {
    if bytes.len() > MAX_PICTURE_BYTES {
        return Decoded::Undecodable(format!("a part of {} bytes", bytes.len()));
    }
    let format = sniff(bytes);
    let attempt = catch_unwind(AssertUnwindSafe(|| match format {
        Format::Png => decode_png(bytes, None),
        Format::Jpeg => decode_jpeg(bytes, None),
        Format::Bmp => decode_bmp(bytes, None),
        Format::Emf | Format::Wmf => Err(String::new()),
        Format::Gif => Err("a GIF, which is not decoded yet".to_owned()),
        Format::Tiff => Err("a TIFF, which is not decoded yet".to_owned()),
        Format::Unknown => Err("bytes of no picture format".to_owned()),
    }));
    match (format, attempt) {
        (Format::Emf | Format::Wmf, _) => decode_metafile(bytes, room),
        (_, Ok(Ok(raw))) => to_pixmap(raw),
        (_, Ok(Err(why))) => Decoded::Undecodable(why),
        (_, Err(_)) => Decoded::Undecodable("a decoder that met a bug".to_owned()),
    }
}

/// An EMF or WMF, by the metafile crate's own reading of the bytes, kept with its bitmaps decoded. A header it refuses is
/// the placeholder's; a bug in either is caught and is the placeholder too.
fn decode_metafile(bytes: &[u8], room: &mut DecodeRoom) -> Decoded {
    match catch_unwind(AssertUnwindSafe(|| diacraft_metafile::parse(bytes).map(|pic| crate::metafile::MetaPicture::new(pic, room)))) {
        Ok(Ok(meta)) => Decoded::Meta(Arc::new(meta)),
        Ok(Err(_)) | Err(_) => Decoded::Metafile,
    }
}

/// The size the decoder reads from the body is the size the header said, when a caller counted pixels from the header (a metafile's running
/// total): else the count would be a lie the decoder could spend.
fn agree(width: u32, height: u32, expect: Option<(u32, u32)>) -> Result<(), String> {
    match expect {
        Some(want) if want != (width, height) => Err(format!("an image of {width} by {height} pixels whose header says {} by {}", want.0, want.1)),
        _ => Ok(()),
    }
}

pub(crate) fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("an image of no size".to_owned());
    }
    if u64::from(width) * u64::from(height) > MAX_PICTURE_PIXELS {
        return Err(format!("an image of {width} by {height} pixels"));
    }
    Ok(())
}

pub(crate) fn decode_png(bytes: &[u8], expect: Option<(u32, u32)>) -> Result<Raw, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("a PNG that does not read: {e}"))?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    check_size(width, height)?;
    agree(width, height, expect)?;
    let size = reader.output_buffer_size().ok_or_else(|| "a PNG too large to hold".to_owned())?;
    if size as u64 > MAX_PICTURE_PIXELS * 8 {
        return Err("a PNG too large to hold".to_owned());
    }
    let mut buf = vec![0u8; size];
    let frame = reader.next_frame(&mut buf).map_err(|e| format!("a PNG that does not decode: {e}"))?;
    let data = buf.get(..frame.buffer_size()).ok_or_else(|| "a PNG that does not decode".to_owned())?;
    let pixels = width as usize * height as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    match frame.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().for_each(|[r, g, b]| rgba.extend_from_slice(&[*r, *g, *b, 255])),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().for_each(|[g, a]| rgba.extend_from_slice(&[*g, *g, *g, *a])),
        png::ColorType::Grayscale => data.iter().for_each(|g| rgba.extend_from_slice(&[*g, *g, *g, 255])),
        png::ColorType::Indexed => return Err("a PNG whose palette was not expanded".to_owned()),
    }
    if rgba.len() != pixels * 4 {
        return Err("a PNG whose size does not fit its header".to_owned());
    }
    Ok(Raw { width, height, rgba })
}

pub(crate) fn decode_jpeg(bytes: &[u8], expect: Option<(u32, u32)>) -> Result<Raw, String> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(MAX_PICTURE_SIDE as usize * 8)
        .set_max_height(MAX_PICTURE_SIDE as usize * 8);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(|e| format!("a JPEG that does not read: {e:?}"))?;
    let info = decoder.info().ok_or_else(|| "a JPEG with no header".to_owned())?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    check_size(width, height)?;
    agree(width, height, expect)?;
    let rgba = decoder.decode().map_err(|e| format!("a JPEG that does not decode: {e:?}"))?;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("a JPEG whose size does not fit its header".to_owned());
    }
    Ok(Raw { width, height, rgba })
}

/// A BMP file: the file header, then a core (12), `BITMAPINFOHEADER` (40) or later (108, 124) header, an optional palette or masks, and
/// rows bottom-up unless the height is negative. 1, 4, 8, 16, 24 and 32 bits, uncompressed or by bit fields; run-length coding is not
/// decoded. The top byte of a 32-bit pixel is not alpha (Windows documents it as unused).
pub(crate) fn decode_bmp(bytes: &[u8], expect: Option<(u32, u32)>) -> Result<Raw, String> {
    let le16 = |at: usize| bytes.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
    let le32 = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let bad = || "a BMP that is cut short".to_owned();
    let data_at = le32(10).ok_or_else(bad)? as usize;
    let header = le32(14).ok_or_else(bad)? as usize;
    let (width, raw_height, bits, compression, colors_used) = if header == 12 {
        (le16(18).ok_or_else(bad)?, i64::from(le16(20).ok_or_else(bad)?), le16(24).ok_or_else(bad)?, 0, 0)
    } else if header >= 40 {
        (
            le32(18).ok_or_else(bad)?,
            i64::from(le32(22).ok_or_else(bad)? as i32),
            le16(28).ok_or_else(bad)?,
            le32(30).ok_or_else(bad)?,
            le32(46).ok_or_else(bad)?,
        )
    } else {
        return Err("a BMP with a header of an unknown size".to_owned());
    };
    let height = raw_height.unsigned_abs();
    let height = u32::try_from(height).map_err(|_| "a BMP too tall".to_owned())?;
    check_size(width, height)?;
    agree(width, height, expect)?;
    let top_down = raw_height < 0;
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) || !matches!(compression, 0 | 3) {
        return Err(format!("a BMP of {bits} bits and compression {compression}"));
    }
    let palette_at = 14 + header;
    // Bit-field masks: after a 40-byte header, three words; inside a later header, at offset 40.
    let masks = if compression == 3 {
        let at = if header == 40 { palette_at } else { 14 + 40 };
        [le32(at).ok_or_else(bad)?, le32(at + 4).ok_or_else(bad)?, le32(at + 8).ok_or_else(bad)?]
    } else if bits == 16 {
        [0x7C00, 0x03E0, 0x001F]
    } else {
        [0x00FF_0000, 0x0000_FF00, 0x0000_00FF]
    };
    let palette: Vec<[u8; 3]> = if bits <= 8 {
        let n = if colors_used == 0 { 1usize << bits } else { (colors_used as usize).min(1usize << bits) };
        let entry = if header == 12 { 3 } else { 4 };
        let start = palette_at + if compression == 3 && header == 40 { 12 } else { 0 };
        (0..n).map(|i| bytes.get(start + i * entry..start + i * entry + 3).map_or([0, 0, 0], |b| [b[2], b[1], b[0]])).collect()
    } else {
        Vec::new()
    };
    let stride = (u64::from(width) * u64::from(bits)).div_ceil(32) * 4;
    let stride = usize::try_from(stride).map_err(|_| "a BMP too wide".to_owned())?;
    if data_at.checked_add(stride.checked_mul(height as usize).ok_or_else(bad)?).is_none_or(|end| end > bytes.len()) {
        return Err(bad());
    }
    let channel = |value: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let max = mask >> shift;
        (((value & mask) >> shift) * 255 / max.max(1)) as u8
    };
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let source_row = if top_down { row } else { height as usize - 1 - row };
        let line = bytes.get(data_at + source_row * stride..data_at + (source_row + 1) * stride).ok_or_else(bad)?;
        for x in 0..width as usize {
            let out = (row * width as usize + x) * 4;
            let px: [u8; 3] = match bits {
                1 => palette.get(usize::from(line[x / 8] >> (7 - x % 8) & 1)).copied().unwrap_or([0; 3]),
                4 => palette.get(usize::from(if x % 2 == 0 { line[x / 2] >> 4 } else { line[x / 2] & 15 })).copied().unwrap_or([0; 3]),
                8 => palette.get(usize::from(line[x])).copied().unwrap_or([0; 3]),
                16 => {
                    let v = u32::from(u16::from_le_bytes([line[x * 2], line[x * 2 + 1]]));
                    [channel(v, masks[0]), channel(v, masks[1]), channel(v, masks[2])]
                }
                24 => [line[x * 3 + 2], line[x * 3 + 1], line[x * 3]],
                _ => {
                    let v = u32::from_le_bytes([line[x * 4], line[x * 4 + 1], line[x * 4 + 2], line[x * 4 + 3]]);
                    [channel(v, masks[0]), channel(v, masks[1]), channel(v, masks[2])]
                }
            };
            rgba[out..out + 3].copy_from_slice(&px);
            rgba[out + 3] = 255;
        }
    }
    Ok(Raw { width, height, rgba })
}

/// Premultiplies and, when an image is larger than the side the renderer keeps, reduces it by a whole factor (a box filter).
pub(crate) fn to_pixmap(raw: Raw) -> Decoded {
    pixmap_of(raw.width, raw.height, &raw.rgba)
}

/// As [`to_pixmap`], over straight RGBA that is not given up: the bytes are not copied first. The slice must hold `width * height * 4`
/// bytes, else the image does not decode.
pub(crate) fn pixmap_of(width: u32, height: u32, rgba: &[u8]) -> Decoded {
    if u64::try_from(rgba.len()).ok() != Some(u64::from(width) * u64::from(height) * 4) {
        return Decoded::Undecodable("an image whose pixels do not fit its size".to_owned());
    }
    let factor = width.max(height).div_ceil(MAX_PICTURE_SIDE).max(1) as usize;
    let (w, h) = (width as usize, height as usize);
    let (nw, nh) = (w.div_ceil(factor), h.div_ceil(factor));
    let (Ok(pw), Ok(ph)) = (u16::try_from(nw), u16::try_from(nh)) else {
        return Decoded::Undecodable("an image too large to hold".to_owned());
    };
    // `Pixmap::from_parts` of 0.2 took the pixels as they are; the one of 0.3 takes bytes and panics on a length or a capacity it does not
    // like, so the pixmap is made empty and written through its slots, which cannot panic. It still says that it may have transparency.
    let mut pixmap = Pixmap::new(pw, ph);
    let mut slots = pixmap.data_mut().iter_mut();
    for ny in 0..nh {
        for nx in 0..nw {
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in ny * factor..((ny + 1) * factor).min(h) {
                for x in nx * factor..((nx + 1) * factor).min(w) {
                    let i = (y * w + x) * 4;
                    let (pr, pg, pb, pa) = (u64::from(rgba[i]), u64::from(rgba[i + 1]), u64::from(rgba[i + 2]), u64::from(rgba[i + 3]));
                    // Averaged premultiplied, so a transparent pixel adds no colour.
                    (r, g, b, a, n) = (r + pr * pa, g + pg * pa, b + pb * pa, a + pa, n + 1);
                }
            }
            let n = n.max(1);
            // r, g, b hold sum(c * a); premultiplied 8-bit is c * a / 255.
            let px = PremulRgba8 { r: (r / n / 255) as u8, g: (g / n / 255) as u8, b: (b / n / 255) as u8, a: (a / n) as u8 };
            if let Some(slot) = slots.next() {
                *slot = px;
            }
        }
    }
    Decoded::Raster(Arc::new(pixmap))
}

/// The size of a PNG, JPEG or BMP in pixels from its header, without decoding it. `None` for bytes that are not one of them, or whose
/// header is cut short. A JPEG's is found by walking its segments to the first frame header, a bounded number of them.
pub(crate) fn header_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let le32 = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let le16 = |at: usize| bytes.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
    match sniff(bytes) {
        Format::Png => (bytes.get(12..16) == Some(b"IHDR")).then_some(()).and(be32(16).zip(be32(20))),
        Format::Bmp => {
            if le32(14)? == 12 {
                le16(18).zip(le16(20))
            } else {
                let (w, h) = (le32(18)? as i32, le32(22)? as i32);
                Some((w.unsigned_abs(), h.unsigned_abs()))
            }
        }
        Format::Jpeg => {
            let mut at = 2usize;
            for _ in 0..1_000 {
                // A marker is 0xFF and a code; fill bytes of 0xFF may come before the code.
                while bytes.get(at) == Some(&0xFF) && bytes.get(at + 1) == Some(&0xFF) {
                    at += 1;
                }
                if bytes.get(at) != Some(&0xFF) {
                    return None;
                }
                let code = *bytes.get(at + 1)?;
                let length = bytes.get(at + 2..at + 4).map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))?;
                // The frame headers are 0xC0 to 0xCF, less the tables 0xC4 and 0xCC and the reserved 0xC8.
                if (0xC0..=0xCF).contains(&code) && !matches!(code, 0xC4 | 0xC8 | 0xCC) {
                    let height = bytes.get(at + 5..at + 7).map(|b| u32::from(u16::from_be_bytes([b[0], b[1]])))?;
                    let width = bytes.get(at + 7..at + 9).map(|b| u32::from(u16::from_be_bytes([b[0], b[1]])))?;
                    return Some((width, height));
                }
                at = at.checked_add(2)?.checked_add(length)?;
            }
            None
        }
        _ => None,
    }
}

/// The foreign data a shape shows: its own, or its master shape's.
pub(crate) fn foreign_of<T: Target>(frame: &Frame<'_, '_, '_, T>, sc: &ShapeCtx<'_>) -> Option<Arc<ForeignData>> {
    let cx = frame.cx;
    let shape = cx.res.shape(&sc.at)?;
    if let Some(own) = &shape.foreign {
        return Some(Arc::clone(own));
    }
    match cx.res.master_step(&sc.at) {
        Ok(Some(step)) => cx.res.shape(&step.shape).and_then(|m| m.foreign.clone()),
        _ => None,
    }
}

impl<T: Target> Frame<'_, '_, '_, T> {
    /// Draws the picture of the shape, if it is a `Foreign` one.
    pub(crate) fn draw_picture(&mut self, sc: &ShapeCtx<'_>, geom: &ShapeGeometry, who: &dyn Fn() -> String) {
        let cx = self.cx;
        let Some(shape) = cx.res.shape(&sc.at) else {
            return;
        };
        let is_foreign = shape.kind == ShapeType::Foreign;
        let foreign = foreign_of(self, sc);
        let (Some(foreign), true) = (foreign, is_foreign || shape.foreign.is_some()) else {
            return;
        };
        let ink = foreign.attr("ForeignType") == Some("Ink");
        let Some(media) = &foreign.picture else {
            // An object that names what it is is drawn as a labelled box by `placeholder.rs`, which says so: a second word about a missing picture is not needed.
            if !ink && crate::placeholder::label_of(cx, sc).is_some() {
                return;
            }
            let kind = if ink { SkipKind::InkNoPicture } else { SkipKind::NoPicture };
            cx.note(kind, who);
            return;
        };
        // The frame and the box, in the shape's own coordinates; a size of 0 or less draws nothing, and says nothing.
        let (w, h) = (cx.num(&sc.sheet, "Width"), cx.num(&sc.sheet, "Height"));
        let (iw, ih) = (cx.num(&sc.sheet, "ImgWidth"), cx.num(&sc.sheet, "ImgHeight"));
        let (ox, oy) = (cx.num(&sc.sheet, "ImgOffsetX"), cx.num(&sc.sheet, "ImgOffsetY"));
        if !(w > 0.0 && h > 0.0 && iw > 0.0 && ih > 0.0 && [w, h, iw, ih, ox, oy].iter().all(|n| n.is_finite())) {
            return;
        }
        if self.speculative {
            // The kept pictures and the one forced decode of the frame serve the shapes in order: this walk is done in order, on one thread.
            self.bail();
            return;
        }
        // A decode is work, counted before it is made: a raster by the pixels its header gives, a metafile by its bytes, and the bitmaps in a
        // metafile by their pixels as the room decodes them, which it holds to the work left. A picture kept from before costs nothing to find.
        if !self.caches.pictures.holds(media) && !self.charge(decode_work(&media.bytes), who) {
            return;
        }
        let pixels = self.work_left().saturating_mul(PIXELS_PER_WORK);
        let mut room = DecodeRoom { left: self.time_left(), force: self.decode_forced_unused, clock: self.clock.clone(), pixels, short: false };
        let decoded = self.caches.pictures.get(media, &mut room);
        self.decode_forced_unused = room.force;
        self.add_work(pixels.saturating_sub(room.pixels).div_ceil(PIXELS_PER_WORK));
        if room.short {
            self.out_of_work(who);
        }
        let frame_path = Rect::new(0.0, 0.0, w, h).to_path(0.1);
        let clip_local = clipping_path(cx.text_at(&sc.sheet, &diacraft_doc::CellAddr::direct("ClippingPath")).as_deref(), geom)
            .unwrap_or_else(|| frame_path.clone());
        let clip = sc.to_paper * clip_local;
        match self.canvas.check(&clip, 0.0) {
            Ok(true) => {}
            Ok(false) => return,
            Err(Refused::OutOfRange) => {
                cx.note(SkipKind::OutOfRange, || format!("{}: a picture", who()));
                return;
            }
        }
        let box_local = Rect::new(ox, oy, ox + iw, oy + ih);
        match decoded {
            Decoded::Raster(pixmap) => {
                let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                // The image's pixel space (y down, first row at the top) onto the box: its top row along the box's upper edge.
                let place = sc.to_paper
                    * Affine::translate((box_local.x0, box_local.y1))
                    * Affine::scale_non_uniform(box_local.width() / pw, -box_local.height() / ph);
                // Drawing it is work, by the pixels it covers.
                if !self.charge(self.image_work(pw, ph, place), who) {
                    return;
                }
                self.canvas.push_clip(&clip);
                self.canvas.image(&pixmap, place);
                self.canvas.pop_clip();
            }
            Decoded::Meta(meta) => self.draw_metafile(sc, &meta, box_local, &clip, who),
            Decoded::Metafile => {
                cx.note(SkipKind::MetafilePlaceholder, || format!("{}: an EMF or WMF picture whose header cannot be read", who()));
                self.placeholder(sc, box_local, &clip);
            }
            Decoded::Undecodable(why) => {
                cx.note(SkipKind::PictureUndecoded, || format!("{}: {why}", who()));
                self.placeholder(sc, box_local, &clip);
            }
        }
    }

    /// A picture that is not drawn: a pale box with a cross in it, our own art, so that a viewer sees something is there.
    pub(crate) fn placeholder(&mut self, sc: &ShapeCtx<'_>, box_local: Rect, clip: &BezPath) {
        let outline = sc.to_paper * box_local.to_path(0.1);
        let mut cross = BezPath::new();
        let corners = [(box_local.x0, box_local.y0), (box_local.x1, box_local.y1), (box_local.x0, box_local.y1), (box_local.x1, box_local.y0)];
        for pair in corners.chunks(2) {
            if let [a, b] = pair {
                cross.move_to(sc.to_paper * kurbo::Point::new(a.0, a.1));
                cross.line_to(sc.to_paper * kurbo::Point::new(b.0, b.1));
            }
        }
        self.canvas.push_clip(clip);
        self.canvas.fill(&outline, &Paint::Solid(PLACEHOLDER_FILL), false);
        // A hundredth of an inch, and no less than the narrowest stroke the target draws.
        let stroke = kurbo::Stroke::new(0.01_f64.max(self.hairline()));
        self.canvas.stroke(&outline, &stroke, &Paint::Solid(Rgba::rgb(150, 150, 150)));
        self.canvas.stroke(&cross, &stroke, &Paint::Solid(Rgba::rgb(170, 170, 170)));
        self.canvas.pop_clip();
    }
}

/// The ring of the section `ClippingPath` names (`Geometry<n>.Path`, section `IX` n - 1), if it names one that the shape has.
fn clipping_path(name: Option<&str>, geom: &ShapeGeometry) -> Option<BezPath> {
    let name = name?.trim();
    let rest = name.get(..8).filter(|p| p.eq_ignore_ascii_case("geometry")).and_then(|_| name.get(8..))?;
    let (digits, tail) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len()));
    if !tail.eq_ignore_ascii_case(".path") {
        return None;
    }
    let ix = diacraft_geom::rules::geometry_section_ix(digits.parse().ok()?)?;
    geom.sections.iter().find(|s| s.ix == Some(ix)).map(|s| s.fill.clone()).filter(|p| !p.is_empty())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn media(bytes: Vec<u8>) -> Arc<Media> {
        Arc::new(Media { part: Arc::from("/visio/media/image1"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })
    }

    /// Bytes of no picture format.
    fn noise(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + 3) as u8).collect()
    }

    /// The start of an EMF: " EMF" at offset 40.
    fn emf(n: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; n];
        bytes[40..44].copy_from_slice(b" EMF");
        bytes
    }

    /// A PNG of 8 by 8 pixels.
    fn png() -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&[200u8; 8 * 8 * 4]).unwrap();
        out
    }

    #[test]
    fn a_picture_that_does_not_decode_and_a_metafile_count_their_raw_bytes() {
        // Both were counted as 0, so 64 parts of 64 MiB (4 GiB) were kept under a limit of 512 MiB. The limit here is 1,000 bytes.
        for (what, make) in [("undecodable", noise as fn(usize) -> Vec<u8>), ("metafile", emf)] {
            let mut cache = Pictures::with_limits(64, 1_000);
            let parts: Vec<_> = (0..3).map(|_| media(make(400))).collect();
            cache.get(&parts[0], &mut DecodeRoom::unlimited());
            cache.get(&parts[1], &mut DecodeRoom::unlimited());
            assert_eq!((cache.len(), cache.held), (2, 800), "{what}: two parts of 400 bytes are 800 bytes held");
            cache.get(&parts[2], &mut DecodeRoom::unlimited());
            // 1,200 would be past the limit: the cache is emptied, and the newest is the only one kept.
            assert_eq!((cache.len(), cache.held), (1, 400), "{what}");
            assert_eq!(Arc::strong_count(&parts[0]), 1, "{what}: the emptied entry no longer holds its part");
            assert_eq!(Arc::strong_count(&parts[2]), 2, "{what}: the newest is held by the cache");
        }
    }

    #[test]
    fn a_raster_counts_its_pixels_and_its_part() {
        let bytes = png();
        let raw = bytes.len() as u64;
        let mut cache = Pictures::with_limits(64, 1 << 20);
        let part = media(bytes);
        assert!(matches!(cache.get(&part, &mut DecodeRoom::unlimited()), Decoded::Raster(_)));
        assert_eq!(cache.held, raw + 8 * 8 * 4);
    }

    #[test]
    fn an_entry_larger_than_the_whole_cache_is_not_kept_and_does_not_empty_it() {
        let mut cache = Pictures::with_limits(64, 1_000);
        let small = media(noise(300));
        let big = media(noise(5_000));
        cache.get(&small, &mut DecodeRoom::unlimited());
        assert!(matches!(cache.get(&big, &mut DecodeRoom::unlimited()), Decoded::Undecodable(_)), "it is still answered");
        assert_eq!((cache.len(), cache.held), (1, 300));
        assert_eq!(Arc::strong_count(&big), 1, "and not held");
    }

    #[test]
    fn a_picture_asked_for_again_is_not_counted_twice() {
        let mut cache = Pictures::with_limits(64, 1_000);
        let part = media(noise(400));
        for _ in 0..5 {
            cache.get(&part, &mut DecodeRoom::unlimited());
        }
        assert_eq!((cache.len(), cache.held), (1, 400));
    }

    #[test]
    fn the_count_of_pictures_is_limited_too() {
        let mut cache = Pictures::with_limits(2, 1 << 20);
        let parts: Vec<_> = (0..3).map(|i| media(noise(10 + i))).collect();
        for p in &parts {
            cache.get(p, &mut DecodeRoom::unlimited());
        }
        assert_eq!((cache.len(), cache.held), (1, 12));
    }

    #[test]
    fn clearing_lets_go_of_every_part() {
        let mut cache = Pictures::default();
        let parts: Vec<_> = (0..5).map(|i| media(noise(100 + i))).collect();
        for p in &parts {
            cache.get(p, &mut DecodeRoom::unlimited());
        }
        assert!(parts.iter().all(|p| Arc::strong_count(p) == 2));
        cache.clear();
        assert!(parts.iter().all(|p| Arc::strong_count(p) == 1));
        assert_eq!((cache.len(), cache.held), (0, 0));
    }

    #[test]
    fn the_size_of_an_image_is_read_from_its_header_without_decoding_it() {
        assert_eq!(header_size(&png()), Some((8, 8)));
        // A BMP of 3 by 2 pixels (BITMAPINFOHEADER), and one of the old 12-byte header.
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 12]);
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&3i32.to_le_bytes());
        bmp.extend_from_slice(&(-2i32).to_le_bytes());
        assert_eq!(header_size(&bmp), Some((3, 2)), "a negative height is a top-down bitmap of the same size");
        let mut core = b"BM".to_vec();
        core.extend_from_slice(&[0; 12]);
        core.extend_from_slice(&12u32.to_le_bytes());
        core.extend_from_slice(&[5, 0, 7, 0]);
        assert_eq!(header_size(&core), Some((5, 7)));
        // A JPEG: the start, a segment that is skipped by its length, a table, then the frame header (height before width).
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46, 0xFF, 0xC4, 0x00, 0x03, 0x00];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8, 0x01, 0x23, 0x00, 0x45, 3]);
        assert_eq!(header_size(&jpeg), Some((0x45, 0x123)));
        // Cut short, a segment that runs past the end, bytes of no format, and a PNG with no header chunk.
        assert_eq!(header_size(&jpeg[..16]), None);
        assert_eq!(header_size(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF, 0, 0]), None);
        assert_eq!(header_size(&noise(100)), None);
        assert_eq!(header_size(&png()[..20]), None);
    }

    /// An EMF of `n` `STRETCHDIBITS`, each of a 2 by 2, 24-bit bitmap of its own, into a frame of 80 by 40 pixels.
    fn emf_with_bitmaps(n: usize) -> Vec<u8> {
        emf_with_bitmaps_of(n, 2)
    }

    /// As [`emf_with_bitmaps`], with bitmaps of `side` by `side` pixels.
    fn emf_with_bitmaps_of(n: usize, side: u32) -> Vec<u8> {
        let u32s = |v: &[u32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let i32s = |v: &[i32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let record = |t: u32, fields: Vec<u8>| -> Vec<u8> {
            let mut out = u32s(&[t, 8 + fields.len() as u32]);
            out.extend(fields);
            out
        };
        let mut info = u32s(&[40]);
        info.extend(i32s(&[side as i32, side as i32]));
        info.extend([1, 0, 24, 0]);
        info.extend(u32s(&[0, 0, 0, 0, 0, 0]));
        let mut body: Vec<u8> = Vec::new();
        for i in 0..n {
            let bits = vec![(i % 250) as u8; ((side * 3).div_ceil(4) * 4 * side) as usize];
            let mut f = i32s(&[0, 0, 80, 40, 0, 0, 0, 0, side as i32, side as i32]);
            f.extend(u32s(&[80, info.len() as u32, 80 + info.len() as u32, bits.len() as u32, 0, 0x00CC_0020]));
            f.extend(i32s(&[80, 40]));
            f.extend(&info);
            f.extend(&bits);
            body.extend(record(81, f));
        }
        body.extend(record(14, u32s(&[0, 16, 20])));
        let mut fields = i32s(&[0, 0, 79, 39, 0, 0, 2000, 1000]);
        fields.extend(u32s(&[0x464D_4520, 0x10000, 108 + body.len() as u32, 2, 4, 0, 0, 0]));
        fields.extend(i32s(&[320, 160, 80, 40]));
        fields.extend(u32s(&[0, 0, 0, 80_000, 40_000]));
        let mut out = record(1, fields);
        out.extend(body);
        out
    }

    fn meta_of(decoded: &Decoded) -> &Arc<MetaPicture> {
        match decoded {
            Decoded::Meta(m) => m,
            _ => panic!("a metafile"),
        }
    }

    #[test]
    fn a_metafile_whose_bitmaps_need_several_frames_is_kept_and_goes_on_from_where_the_last_frame_stopped() {
        let mut cache = Pictures::default();
        let part = media(emf_with_bitmaps(3));
        // Frames with no time to spare: each decodes one bitmap, the picture is kept from the first, and the third frame finishes it.
        let first = cache.get(&part, &mut DecodeRoom::spent());
        assert!(!meta_of(&first).is_complete());
        assert_eq!(cache.len(), 1, "kept at once, not decoded again from the start");
        drop(first);
        let second = cache.get(&part, &mut DecodeRoom::spent());
        assert!(!meta_of(&second).is_complete());
        drop(second);
        let third = cache.get(&part, &mut DecodeRoom::spent());
        assert!(meta_of(&third).is_complete(), "three bitmaps, three frames");
        let held = cache.held;
        drop(third);
        // And it stays: nothing is decoded again, and the bytes held do not change.
        let fourth = cache.get(&part, &mut DecodeRoom::spent());
        assert!(meta_of(&fourth).is_complete());
        assert_eq!((cache.len(), cache.held), (1, held));
        assert!(held > part.bytes.len() as u64, "the pixmaps are counted as they are decoded");
    }

    /// An EMF+ file of `n` image objects, each a PNG of one grey, `side` by `side` pixels (a few hundred bytes for `side * side * 4` bytes of
    /// pixmap), each drawn into the frame.
    fn emf_plus_with_pngs(n: u16, side: u32) -> Vec<u8> {
        let u32s = |v: &[u32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let i32s = |v: &[i32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let f32s = |v: &[f32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let record = |t: u32, fields: Vec<u8>| -> Vec<u8> {
            let mut out = u32s(&[t, 8 + fields.len() as u32]);
            out.extend(fields);
            out
        };
        let plus = |t: u16, flags: u16, data: Vec<u8>| -> Vec<u8> {
            let mut d = data;
            while !d.len().is_multiple_of(4) {
                d.push(0);
            }
            let mut out = [t.to_le_bytes(), flags.to_le_bytes()].concat();
            out.extend(u32s(&[12 + d.len() as u32, d.len() as u32]));
            out.extend(d);
            out
        };
        let comment = |records: Vec<Vec<u8>>| -> Vec<u8> {
            let body: Vec<u8> = records.concat();
            let mut f = u32s(&[4 + body.len() as u32, 0x2B46_4D45]);
            f.extend(body);
            record(70, f)
        };
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, side, side);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&vec![90u8; (side * side) as usize]).unwrap();
        }
        let mut body: Vec<u8> = comment(vec![plus(0x4001, 1, u32s(&[0xDBC0_1002, 1, 96, 96]))]);
        for id in 1..=n {
            let mut image = u32s(&[0xDBC0_1002, 1]);
            image.extend(i32s(&[side as i32, side as i32, 0]));
            image.extend(u32s(&[0x0026_200A, 1]));
            image.extend(&png_bytes);
            let draw =
                [u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, side as f32, side as f32]), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat();
            body.extend(comment(vec![plus(0x4008, 0x0500 | id, image), plus(0x401B, id, draw)]));
        }
        body.extend(comment(vec![plus(0x4002, 0, Vec::new())]));
        body.extend(record(14, u32s(&[0, 16, 20])));
        let mut fields = i32s(&[0, 0, 79, 39, 0, 0, 2000, 1000]);
        fields.extend(u32s(&[0x464D_4520, 0x10000, 108 + body.len() as u32, 2, 4, 0, 0, 0]));
        fields.extend(i32s(&[320, 160, 80, 40]));
        fields.extend(u32s(&[0, 0, 0, 80_000, 40_000]));
        let mut out = record(1, fields);
        out.extend(body);
        out
    }

    #[test]
    fn the_cache_stays_under_its_limit_while_pictures_are_decoded_over_several_frames() {
        // Two pictures of four PNGs of 400 by 400 pixels each (a kilobyte of file, 640 KB of pixels), in a cache that holds one picture whole
        // and not two. A picture is weighed by the most it can come to when it is taken (the pixels its headers say), so the check at
        // insertion covers the decoding that follows, and no frame lifts the weight.
        let (a, b) = (media(emf_plus_with_pngs(4, 400)), media(emf_plus_with_pngs(4, 400)));
        let limit = 3_000_000;
        let mut cache = Pictures::with_limits(64, limit);
        // One picture whole is about 2.6 MB of pixmaps; with the parse and the file it is under the limit, and two together are over it.
        for (name, part) in [("first", &a), ("second", &b)] {
            let mut frames = 0;
            loop {
                frames += 1;
                let got = cache.get(part, &mut DecodeRoom::spent());
                assert!(cache.held <= limit, "{name} picture, frame {frames}: {} held, past {limit}", cache.held);
                let done = meta_of(&got).is_complete();
                drop(got);
                if done || frames > 8 {
                    break;
                }
            }
            assert!(frames <= 5, "{name}: four bitmaps are four frames and the frame that sees them all: {frames}");
            assert_eq!(cache.len(), 1, "{name}: the other was let go to make room, and this one is kept");
        }
        // The second is kept whole, and asking again changes nothing.
        let held = cache.held;
        assert!(meta_of(&cache.get(&b, &mut DecodeRoom::spent())).is_complete());
        assert_eq!((cache.len(), cache.held), (1, held));
        assert!(held > 2_000_000, "the pixmaps are in the weight: {held}");
    }

    #[test]
    fn a_picture_too_big_for_the_cache_is_whole_the_one_time_it_is_drawn_and_not_decoded_again_and_again() {
        // A picture bigger than all the cache may hold cannot be kept, so there is no next frame to finish it in: what waits is marked as not
        // drawn, for that reason.
        let part = media(emf_with_bitmaps_of(3, 64));
        let mut cache = Pictures::with_limits(64, 100);
        let got = cache.get(&part, &mut DecodeRoom::spent());
        assert!(meta_of(&got).is_complete());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_frame_with_no_time_forces_one_decode_for_all_its_pictures_and_not_one_for_each() {
        // Twelve pictures asked for in one frame that has no time: the first gets the frame's forced decode, the other eleven none. Each
        // moves on in the frames after, when it has a turn, and is whole after the most frames or when it is decoded.
        let parts: Vec<_> = (0..12).map(|_| media(emf_with_bitmaps(3))).collect();
        let mut cache = Pictures::default();
        let mut room = DecodeRoom::spent();
        let decoded: Vec<usize> = parts
            .iter()
            .map(|p| {
                let got = cache.get(p, &mut room);
                meta_of(&got).decoded_count()
            })
            .collect();
        assert_eq!(decoded.iter().sum::<usize>(), 1, "one decode in the frame: {decoded:?}");
        assert_eq!(decoded.first(), Some(&1));
        assert!(!room.force, "and it is used");
        // After the most frames of calls every picture is whole, whether or not it had its turn.
        for _ in 0..crate::metafile::MAX_DECODING_FRAMES {
            let mut room = DecodeRoom::spent();
            for p in &parts {
                cache.get(p, &mut room);
            }
        }
        assert!(parts.iter().all(|p| meta_of(&cache.get(p, &mut DecodeRoom::spent())).is_complete()));
    }

    #[test]
    fn the_count_of_images_decoded_moves_with_each_decode_and_not_with_a_picture_kept() {
        let mut cache = Pictures::default();
        assert_eq!(cache.decoded(), 0);
        // A raster is decoded once, and asking again leaves the count where it was.
        let raster = media(png());
        cache.get(&raster, &mut DecodeRoom::unlimited());
        assert_eq!(cache.decoded(), 1);
        cache.get(&raster, &mut DecodeRoom::unlimited());
        assert_eq!(cache.decoded(), 1, "kept, not decoded again");
        // Bytes that do not decode, and a metafile whose header cannot be read, are no image decoded.
        cache.get(&media(noise(50)), &mut DecodeRoom::unlimited());
        cache.get(&media(emf(400)), &mut DecodeRoom::unlimited());
        assert_eq!(cache.decoded(), 1);
        // A metafile of three bitmaps in frames with no time: the frame's one forced decode each, counted as it happens, and none once whole.
        let slow = media(emf_with_bitmaps(3));
        for want in [2, 3, 4, 4, 4] {
            cache.get(&slow, &mut DecodeRoom::spent());
            assert_eq!(cache.decoded(), want);
        }
        // Another with all the time there is: its three in the one frame.
        cache.get(&media(emf_with_bitmaps(3)), &mut DecodeRoom::unlimited());
        assert_eq!(cache.decoded(), 7);
    }

    #[test]
    fn the_count_of_images_decoded_does_not_go_back_when_the_cache_is_cleared_and_counts_a_decode_that_is_done_again() {
        let mut cache = Pictures::default();
        let raster = media(png());
        cache.get(&raster, &mut DecodeRoom::unlimited());
        cache.clear();
        assert_eq!(cache.decoded(), 1, "work done stays done");
        cache.get(&raster, &mut DecodeRoom::unlimited());
        assert_eq!(cache.decoded(), 2, "a picture let go is decoded again, and that is counted");
    }
}
