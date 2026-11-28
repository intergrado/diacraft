//! A picture from a file: Insert, Illustrations, Pictures (`stencil/picture.rs` says what the shape holds).
//!
//! **What is a picture.** The file's bytes, judged by themselves and never by the name: PNG, JPEG, GIF, BMP and TIFF are bitmaps (`ForeignType` `Bitmap`), and EMF and WMF are
//! metafiles (`EnhMetaFile`). A WMF is typed `image/x-emf`, as the drawings of the sample files hold theirs (a part typed EMF holding a WMF is
//! the ordinary case), since the six types MS-VSDX lists have none for it. The bytes are copied into the model **as they are** and never decoded here: the renderer decodes them, with
//! its own checks.
//!
//! **Project rules** (ours; the specs say only that a bitmap stays a bitmap, that a picture may be up to 6 MB, and what File, Open lists):
//!
//! * The seven formats above are taken. The renderer draws PNG, JPEG, BMP, EMF and WMF; **a GIF or a TIFF is held and drawn as a placeholder** (the renderer does not decode
//!   them yet), and the answer says so, so that nothing a person inserted is lost. An SVG, and any other bytes, are refused in words.
//! * A picture is as big as its header says at 96 pixels to the inch, a metafile as its frame (an EMF's `rclFrame`, a placeable WMF's box over its units to the inch), and is
//!   made to fit within 80 percent of the page when it is bigger. A width or a height the caller gives keeps the picture's proportions when it is the only one.
//! * The budgets are the readers': a file of more than [`MAX_PICTURE_BYTES`] (the renderer decodes no more), a picture of more than [`MAX_PICTURE_PIXELS`] pixels once
//!   decoded, and pictures that together hold more than [`MAX_DRAWING_MEDIA_BYTES`] (what the VSDX reader copies into one drawing). Each is refused in words, before the file is read where its
//!   size says so. The same bytes are **one picture however many shapes show them**.
//!
//! **Hostile input.** A header is read at fixed places, within the bytes there are (a JPEG's segments within [`MAX_SEGMENTS`], a TIFF's entries within [`MAX_TIFF_ENTRIES`]), and the pixels
//! are counted from it before anything is made. The walk that sums the drawing's pictures stops at [`MAX_WALK`] shapes.

use std::sync::Arc;

use diacraft_doc::{Document, Media, ShapeList};

use super::{Problem, checked, page_mut, require_page};

pub use diacraft_render::limits::MAX_PICTURE_PIXELS;

/// The most bytes of a picture file: the renderer decodes no more.
pub const MAX_PICTURE_BYTES: u64 = diacraft_render::limits::MAX_PICTURE_BYTES as u64;
/// The most bytes of pictures one drawing holds: what the VSDX reader copies into a drawing.
pub const MAX_DRAWING_MEDIA_BYTES: u64 = diacraft_vsdx::MAX_MEDIA_BYTES;
/// The most segments of a JPEG that are looked through for its size.
pub const MAX_SEGMENTS: usize = 4_096;
/// The most entries of a TIFF's first directory that are looked through.
pub const MAX_TIFF_ENTRIES: usize = 4_096;
/// The most shapes the walk that sums a drawing's pictures looks at.
pub const MAX_WALK: usize = 2_000_000;
/// The pixels to the inch a bitmap is taken to have.
const PIXELS_PER_INCH: f64 = 96.0;
/// The share of the page a picture that is too big is made to fit within.
const FIT: f64 = 0.8;

/// A kind of picture, by its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
    Emf,
    Wmf,
}

impl Format {
    /// The word the answer says.
    pub fn word(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Gif => "gif",
            Format::Bmp => "bmp",
            Format::Tiff => "tiff",
            Format::Emf => "emf",
            Format::Wmf => "wmf",
        }
    }

    /// The `ForeignType` of the shape.
    pub fn foreign_type(self) -> &'static str {
        match self {
            Format::Emf | Format::Wmf => "EnhMetaFile",
            _ => "Bitmap",
        }
    }

    /// The content type the picture is held under, which the writers type its part by.
    pub fn content_type(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Gif => "image/gif",
            Format::Bmp => "image/bmp",
            Format::Tiff => "image/tiff",
            Format::Emf | Format::Wmf => "image/x-emf",
        }
    }

    /// The extension a part of the picture has.
    fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Gif => "gif",
            Format::Bmp => "bmp",
            Format::Tiff => "tiff",
            Format::Emf => "emf",
            Format::Wmf => "wmf",
        }
    }

    /// Whether the renderer draws it.
    pub fn drawn(self) -> bool {
        !matches!(self, Format::Gif | Format::Tiff)
    }
}

/// What a picture's header says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    pub format: Format,
    /// The picture's own size in inches.
    pub width: f64,
    pub height: f64,
}

const WORDS_FORMATS: &str = "PNG, JPEG, GIF, BMP, TIFF, EMF or WMF";

fn cut_short(what: &str) -> Problem {
    Problem::bad(format!("the {what} is cut short or holds no size: its header is not all there"))
}

fn no_size() -> Problem {
    Problem::bad("the picture has no size: its width or its height is 0")
}

pub(crate) fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

pub(crate) fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

pub(crate) fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

pub(crate) fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

pub(crate) fn le_i32(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// The format the first bytes say (the renderer's own table), and `None` for bytes of no format.
fn sniff(b: &[u8]) -> Option<Format> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(Format::Png)
    } else if b.starts_with(&[0xFF, 0xD8]) {
        Some(Format::Jpeg)
    } else if b.starts_with(b"BM") {
        Some(Format::Bmp)
    } else if b.starts_with(b"GIF8") {
        Some(Format::Gif)
    } else if b.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || b.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        Some(Format::Tiff)
    } else if b.get(40..44) == Some(b" EMF".as_slice()) {
        Some(Format::Emf)
    } else if b.starts_with(&[0x01, 0x00, 0x09, 0x00]) || b.starts_with(&[0x02, 0x00, 0x09, 0x00]) || b.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        Some(Format::Wmf)
    } else {
        None
    }
}

/// The pixels a JPEG's first frame header says: the segments are walked from the start, at most [`MAX_SEGMENTS`] of them.
pub(crate) fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2usize;
    for _ in 0..MAX_SEGMENTS {
        // A marker is 0xFF and a code; extra 0xFF bytes are fill.
        while *b.get(at)? != 0xFF {
            at += 1;
        }
        while *b.get(at)? == 0xFF {
            at += 1;
        }
        let code = *b.get(at)?;
        at += 1;
        match code {
            // Markers with no length.
            0x00 | 0x01 | 0xD0..=0xD7 => continue,
            // The end of the image, or the start of its data: no frame header was met.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let length = usize::try_from(be16(b, at)?).ok()?;
        if length < 2 {
            return None;
        }
        // The frame headers: C0 to CF but the tables of Huffman codes (C4), the arithmetic ones (CC) and the reserved C8.
        if matches!(code, 0xC0..=0xCF) && !matches!(code, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(b, at + 5)?, be16(b, at + 3)?));
        }
        at = at.checked_add(length)?;
    }
    None
}

/// The size a TIFF's first directory says: `ImageWidth` (256) and `ImageLength` (257), as shorts or longs, in either byte order.
pub(crate) fn tiff_size(b: &[u8]) -> Option<(u32, u32)> {
    let little = b.starts_with(&[0x49, 0x49]);
    let u16_at = |at: usize| if little { le16(b, at) } else { be16(b, at) };
    let u32_at = |at: usize| if little { le32(b, at) } else { be32(b, at) };
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::try_from(u16_at(ifd)?).ok()?;
    if count > MAX_TIFF_ENTRIES {
        return None;
    }
    let (mut width, mut height) = (None, None);
    for n in 0..count {
        let entry = ifd.checked_add(2)?.checked_add(n.checked_mul(12)?)?;
        let (tag, kind) = (u16_at(entry)?, u16_at(entry + 2)?);
        let value = match kind {
            3 => u16_at(entry + 8)?,
            4 => u32_at(entry + 8)?,
            _ => continue,
        };
        match tag {
            256 => width = Some(value),
            257 => height = Some(value),
            _ => {}
        }
    }
    Some((width?, height?))
}

/// What a picture's header says of it. Refused, in words, for bytes of no format, a header cut short, a size of 0 and a picture of more than [`MAX_PICTURE_PIXELS`] pixels.
pub fn inspect(bytes: &[u8]) -> Result<Info, Problem> {
    if bytes.is_empty() {
        return Err(Problem::bad("the file is empty"));
    }
    let Some(format) = sniff(bytes) else {
        let head = bytes.get(..1024.min(bytes.len())).unwrap_or(bytes);
        let text = String::from_utf8_lossy(head);
        if text.contains("<svg") {
            return Err(Problem::bad(format!(
                "the file is an SVG drawing, which a drawing here does not hold as a picture: insert a {WORDS_FORMATS} picture"
            )));
        }
        return Err(Problem::bad(format!(
            "the file is not a picture DiaCraft holds: it takes {WORDS_FORMATS}, told by the bytes and not by the name"
        )));
    };
    let pixels = |w: u32, h: u32| -> Result<Info, Problem> {
        if w == 0 || h == 0 {
            return Err(no_size());
        }
        if u64::from(w) * u64::from(h) > MAX_PICTURE_PIXELS {
            return Err(Problem::bad(format!(
                "the picture is {w} by {h} pixels, which is more than the {} megapixels a picture may have",
                MAX_PICTURE_PIXELS / (1024 * 1024)
            )));
        }
        Ok(Info { format, width: f64::from(w) / PIXELS_PER_INCH, height: f64::from(h) / PIXELS_PER_INCH })
    };
    match format {
        Format::Png => {
            if bytes.get(12..16) != Some(b"IHDR".as_slice()) {
                return Err(cut_short("PNG"));
            }
            pixels(be32(bytes, 16).ok_or_else(|| cut_short("PNG"))?, be32(bytes, 20).ok_or_else(|| cut_short("PNG"))?)
        }
        Format::Jpeg => {
            let (w, h) = jpeg_size(bytes).ok_or_else(|| cut_short("JPEG"))?;
            pixels(w, h)
        }
        Format::Gif => pixels(le16(bytes, 6).ok_or_else(|| cut_short("GIF"))?, le16(bytes, 8).ok_or_else(|| cut_short("GIF"))?),
        Format::Bmp => {
            let header = le32(bytes, 14).ok_or_else(|| cut_short("BMP"))?;
            let (w, h) = if header == 12 {
                (i64::from(le16(bytes, 18).ok_or_else(|| cut_short("BMP"))?), i64::from(le16(bytes, 20).ok_or_else(|| cut_short("BMP"))?))
            } else {
                (i64::from(le_i32(bytes, 18).ok_or_else(|| cut_short("BMP"))?), i64::from(le_i32(bytes, 22).ok_or_else(|| cut_short("BMP"))?))
            };
            // A negative height is a picture stored top down; a negative width is none.
            if w <= 0 || h == 0 {
                return Err(no_size());
            }
            pixels(u32::try_from(w).map_err(|_| no_size())?, u32::try_from(h.unsigned_abs()).map_err(|_| no_size())?)
        }
        Format::Tiff => {
            let (w, h) = tiff_size(bytes).ok_or_else(|| cut_short("TIFF"))?;
            pixels(w, h)
        }
        Format::Emf => {
            // The record's size is at least the 88 bytes of the header, and the frame (hundredths of a millimetre) is the picture's size.
            if bytes.len() < 88 {
                return Err(cut_short("EMF"));
            }
            let at = |n: usize| le_i32(bytes, n).map(i64::from).ok_or_else(|| cut_short("EMF"));
            let (w, h) = (at(32)? - at(24)?, at(36)? - at(28)?);
            if w <= 0 || h <= 0 {
                return Err(no_size());
            }
            Ok(Info { format, width: w as f64 / 2_540.0, height: h as f64 / 2_540.0 })
        }
        Format::Wmf => {
            if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
                if bytes.len() < 22 {
                    return Err(cut_short("WMF"));
                }
                let at = |n: usize| le16(bytes, n).map(|v| i64::from(v as u16 as i16)).ok_or_else(|| cut_short("WMF"));
                let (w, h) = ((at(10)? - at(6)?).abs(), (at(12)? - at(8)?).abs());
                let per_inch = i64::from(le16(bytes, 14).ok_or_else(|| cut_short("WMF"))?);
                if w == 0 || h == 0 || per_inch == 0 {
                    return Err(no_size());
                }
                Ok(Info { format, width: w as f64 / per_inch as f64, height: h as f64 / per_inch as f64 })
            } else {
                // A WMF that is not placeable says no size: two inches by two.
                Ok(Info { format, width: 2.0, height: 2.0 })
            }
        }
    }
}

/// What a walk of a drawing's pictures found.
struct Held {
    /// The bytes of the pictures the drawing holds, each picture once however many shapes show it.
    bytes: u64,
    /// How many pictures.
    pictures: usize,
    /// A picture of the same bytes, if the drawing has one.
    same: Option<Arc<Media>>,
}

/// Walks every page and master once: the sum of the pictures held (a picture is counted once by its allocation), and a picture of the same bytes as `bytes`, if there is one. Refused, in
/// words, past [`MAX_WALK`] shapes.
fn held(doc: &Document, bytes: &[u8]) -> Result<Held, Problem> {
    let mut out = Held { bytes: 0, pictures: 0, same: None };
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut walked = 0usize;
    let lists: Vec<&ShapeList> = doc.pages.iter().map(|p| &p.shapes).chain(doc.masters.iter().map(|m| &m.shapes)).collect();
    for top in lists {
        let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
        while let Some(frame) = stack.last_mut() {
            let (list, at) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            walked += 1;
            if walked > MAX_WALK {
                return Err(Problem::failed(format!(
                    "the drawing has more than {MAX_WALK} shapes, and its pictures are not counted: too many to insert one more"
                )));
            }
            if let Some(media) = shape.foreign.as_ref().and_then(|f| f.picture.as_ref())
                && seen.insert(Arc::as_ptr(media) as usize)
            {
                out.bytes = out.bytes.saturating_add(media.bytes.len() as u64);
                out.pictures += 1;
                if out.same.is_none() && media.bytes.len() == bytes.len() && media.bytes.as_ref() == bytes {
                    out.same = Some(Arc::clone(media));
                }
            }
            if stack.len() < diacraft_doc::MAX_SHAPE_DEPTH {
                stack.push((&shape.children, 0));
            }
        }
    }
    Ok(out)
}

/// What an insertion made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inserted {
    pub id: u32,
    /// The size of the shape in paper inches.
    pub width: f64,
    pub height: f64,
    /// Whether the picture is one the drawing already held (its bytes are held once).
    pub shared: bool,
}

/// The media a picture of `bytes` (which `info` says what they are) is held as in `doc` → it and whether the drawing held the same bytes already. The same bytes are one picture
/// And the drawing's pictures are held to what a file may hold: a picture that would pass [`MAX_DRAWING_MEDIA_BYTES`] is refused in words.
pub(crate) fn media_for(doc: &Document, info: &Info, bytes: Arc<[u8]>) -> Result<(Arc<Media>, bool), Problem> {
    let walked = held(doc, &bytes)?;
    match walked.same {
        Some(same) => Ok((same, true)),
        None => {
            let total = walked.bytes.saturating_add(bytes.len() as u64);
            if total > MAX_DRAWING_MEDIA_BYTES {
                return Err(Problem::failed(format!(
                    "the pictures of a drawing hold at most {} MiB between them, and this one would make {} MiB: {} MiB are held now",
                    MAX_DRAWING_MEDIA_BYTES / (1024 * 1024),
                    total.div_ceil(1024 * 1024),
                    walked.bytes.div_ceil(1024 * 1024)
                )));
            }
            let part = format!("media/image{}.{}", walked.pictures + 1, info.format.extension());
            Ok((Arc::new(Media { part: Arc::from(part), content_type: Arc::from(info.format.content_type()), bytes }), false))
        }
    }
}

/// How big a picture is made and where it goes, as the caller asked, in paper inches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Where {
    pub centre: Option<(f64, f64)>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// Puts a picture of `bytes` (which `info` says what they are) on a page as a shape, centred where `at` says (the middle of the page without), as big as `at` says or,
/// in the front, as one edit. Its `ID` is above `above` and above every `ID` of the page. Refused, with the document as it was, for a page that holds the most shapes, a box or a place that
/// is not within the page's limits, and pictures that would hold more than [`MAX_DRAWING_MEDIA_BYTES`] between them.
pub fn insert_picture(doc: &mut Document, page: u32, info: &Info, bytes: Arc<[u8]>, at: &Where, above: u32) -> Result<Inserted, Problem> {
    require_page(doc, page)?;
    let setup = super::read_setup(doc, page)?;
    let k = setup.units_per_inch;
    // The size: the one asked, in proportion when one side is, else the picture's own made to fit the page.
    let aspect = info.height / info.width;
    let (width, height) = match (at.width, at.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * aspect),
        (None, Some(h)) => (h / aspect, h),
        (None, None) => {
            let fit = (FIT * setup.width / info.width).min(FIT * setup.height / info.height).min(1.0);
            (info.width * fit, info.height * fit)
        }
    };
    let (width, height) = (checked("width", width)?, checked("height", height)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(Problem::bad("`width` and `height` must be more than 0"));
    }
    let (x, y) = at.centre.unwrap_or((setup.width / 2.0, setup.height / 2.0));
    let (x, y) = (checked("x", x)?, checked("y", y)?);
    let (x, y, w, h) = (checked("x", x * k)?, checked("y", y * k)?, checked("width", width * k)?, checked("height", height * k)?);
    let (highest, count) = super::page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= diacraft_doc::MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {} shapes", diacraft_doc::MAX_SHAPES_PER_PAGE)));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let (media, shared) = media_for(doc, info, bytes)?;
    let shape = crate::stencil::picture_shape(id, (x, y), (w, h), info.format.foreign_type(), media);
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.push_shape(Arc::new(shape));
    super::layers::join_active(doc, page, id)?;
    // A page with Auto Size on grows to take the picture in, in the same undo step.
    super::fit_page(doc, page, &[id])?;
    Ok(Inserted { id, width, height, shared })
}
