//! A master's icon as pixels, for a tile of the Shapes window.
//!
//! A stencil file keeps an icon for a master as base64 text, and the engine hands the window the decoded bytes (`OpenedStencil::icon_bytes`) with the
//! kind of picture its first bytes say: mostly **ICO**, then PNG and BMP. This reads those three; a GIF, a JPEG, a metafile or anything else is not
//! read, and the tile is drawn from the master itself instead (`stencils.rs`).
//!
//! **The bytes are a file's, so they are hostile.** Every offset and length is looked up with `get` and added with a checked sum, the sides are
//! held to [`MAX_SIDE`] before any pixel is made, an icon holding several pictures is read at the largest one that is not over the cap, and
//! what does not fit is `None`, never a panic and never a guess. Compressed bitmaps (run-length) are not read.

use std::io::Cursor;

/// The longest side of an icon, in pixels, that is read. Real icons are 16 to 48; the format allows 256.
pub const MAX_SIDE: usize = 256;

/// An icon's pixels: RGBA, straight (not premultiplied), rows from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Pixels {
    /// The same pixels with each colour multiplied by its alpha, as egui's textures take them.
    pub fn premultiplied(&self) -> Vec<u8> {
        let scale = |c: u8, a: u8| u8::try_from((u16::from(c) * u16::from(a) + 127) / 255).unwrap_or(u8::MAX);
        self.rgba.as_chunks::<4>().0.iter().flat_map(|[r, g, b, a]| [scale(*r, *a), scale(*g, *a), scale(*b, *a), *a]).collect()
    }
}

/// The pixels of an icon of the kind `format` (the engine's word: `ico`, `png`, `bmp`, ...), or `None` when the kind is not one read or the bytes
/// are not a picture of it.
pub fn decode(format: &str, bytes: &[u8]) -> Option<Pixels> {
    match format {
        "png" => png(bytes),
        "ico" => ico(bytes),
        "bmp" => bmp(bytes),
        _ => None,
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(usize::from(u16::from_le_bytes([*b.first()?, *b.get(1)?])))
}

fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    usize::try_from(u32::from_le_bytes([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?])).ok()
}

fn side_ok(width: usize, height: usize) -> bool {
    (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height)
}

/// A PNG, whatever its colour type or depth, as RGBA.
fn png(bytes: &[u8]) -> Option<Pixels> {
    let mut decoder = ::png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(::png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = {
        let info = reader.info();
        (usize::try_from(info.width).ok()?, usize::try_from(info.height).ok()?)
    };
    if !side_ok(width, height) {
        return None;
    }
    let mut buf = vec![0_u8; reader.output_buffer_size().filter(|n| *n <= MAX_SIDE * MAX_SIDE * 8)?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = buf.get(..frame.buffer_size())?;
    let mut rgba = Vec::with_capacity(width * height * 4);
    match frame.color_type {
        ::png::ColorType::Rgba => rgba.extend_from_slice(data),
        ::png::ColorType::Rgb => data.as_chunks::<3>().0.iter().for_each(|[r, g, b]| rgba.extend_from_slice(&[*r, *g, *b, 255])),
        ::png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().for_each(|[g, a]| rgba.extend_from_slice(&[*g, *g, *g, *a])),
        ::png::ColorType::Grayscale => data.iter().for_each(|g| rgba.extend_from_slice(&[*g, *g, *g, 255])),
        ::png::ColorType::Indexed => return None,
    }
    (rgba.len() == width * height * 4).then_some(Pixels { width, height, rgba })
}

/// An ICO file: a directory of pictures, each a PNG or a bitmap with no file header. The largest that is within the cap is read.
fn ico(bytes: &[u8]) -> Option<Pixels> {
    if le16(bytes, 0)? != 0 || le16(bytes, 2)? != 1 {
        return None;
    }
    let count = le16(bytes, 4)?.min(256);
    let mut best: Option<(usize, usize, usize)> = None;
    for n in 0..count {
        let at = 6 + n * 16;
        let (w, h) = (usize::from(*bytes.get(at)?), usize::from(*bytes.get(at + 1)?));
        // A side of 0 means 256.
        let (w, h) = (if w == 0 { 256 } else { w }, if h == 0 { 256 } else { h });
        let (size, offset) = (le32(bytes, at + 8)?, le32(bytes, at + 12)?);
        if side_ok(w, h) && best.is_none_or(|(area, ..)| w * h > area) {
            best = Some((w * h, offset, size));
        }
    }
    let (_, offset, size) = best?;
    let picture = bytes.get(offset..offset.checked_add(size)?)?;
    if picture.starts_with(&[0x89, b'P', b'N', b'G']) { png(picture) } else { dib(picture, true) }
}

/// A BMP file: its 14-byte header, then a bitmap.
fn bmp(bytes: &[u8]) -> Option<Pixels> {
    if !bytes.starts_with(b"BM") {
        return None;
    }
    dib(bytes.get(14..)?, false)
}

/// What a bitmap's header says: its sides, depth, whether its rows run from the top, and where its palette and rows are.
struct Dib<'a> {
    width: usize,
    height: usize,
    bits: usize,
    top_down: bool,
    palette: &'a [u8],
    rows: &'a [u8],
    stride: usize,
    /// The 1-bit mask under the rows (an icon's), whose set bits are clear pixels.
    mask: Option<&'a [u8]>,
    mask_stride: usize,
}

/// The header of a device-independent bitmap and the slices it points at. `in_icon`: the height counts the picture and the mask under it.
fn dib_header(bytes: &[u8], in_icon: bool) -> Option<Dib<'_>> {
    let header = le32(bytes, 0)?;
    if header < 40 || bytes.len() < header {
        return None;
    }
    let width = le32(bytes, 4)?;
    let raw_height = i64::from(i32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?));
    let bits = le16(bytes, 14)?;
    let compression = le32(bytes, 16)?;
    let used = le32(bytes, 32)?;
    // Uncompressed, or 32 bits by bit fields (read as the usual BGRA).
    if compression != 0 && !(compression == 3 && bits == 32) {
        return None;
    }
    let mut height = usize::try_from(raw_height.unsigned_abs()).ok()?;
    if in_icon {
        height /= 2;
    }
    if !side_ok(width, height) || !matches!(bits, 1 | 4 | 8 | 24 | 32) {
        return None;
    }
    let colours = if bits > 8 {
        0
    } else if used == 0 {
        1_usize << bits
    } else {
        used.min(1_usize << bits)
    };
    let palette = bytes.get(header..header.checked_add(colours.checked_mul(4)?)?)?;
    let rows_at = header + colours * 4;
    let stride = (width.checked_mul(bits)?.checked_add(31)? / 32) * 4;
    let rows = bytes.get(rows_at..rows_at.checked_add(stride.checked_mul(height)?)?)?;
    let mask_stride = width.div_ceil(32) * 4;
    let mask_at = rows_at + stride * height;
    let mask = in_icon.then(|| bytes.get(mask_at..mask_at + mask_stride * height)).flatten();
    Some(Dib { width, height, bits, top_down: raw_height < 0, palette, rows, stride, mask, mask_stride })
}

impl Dib<'_> {
    /// The pixel `x` of a row as red, green, blue and alpha.
    fn pixel(&self, row: &[u8], x: usize) -> Option<[u8; 4]> {
        Some(match self.bits {
            32 => [*row.get(x * 4 + 2)?, *row.get(x * 4 + 1)?, *row.get(x * 4)?, *row.get(x * 4 + 3)?],
            24 => [*row.get(x * 3 + 2)?, *row.get(x * 3 + 1)?, *row.get(x * 3)?, 255],
            bits => {
                let per_byte = 8 / bits;
                let byte = *row.get(x / per_byte)?;
                let entry = usize::from((byte >> (8 - bits * (x % per_byte + 1))) & (0xff >> (8 - bits)));
                let p = self.palette.get(entry * 4..entry * 4 + 3)?;
                [p[2], p[1], p[0], 255]
            }
        })
    }

    /// Whether the mask makes pixel `x` of the picture's row `y` (from the top) clear.
    fn masked(&self, x: usize, y: usize) -> bool {
        let from = if self.top_down { y } else { self.height - 1 - y };
        self.mask.and_then(|mask| mask.get(from * self.mask_stride + x / 8)).is_some_and(|byte| (byte >> (7 - x % 8)) & 1 == 1)
    }
}

/// A device-independent bitmap: its header, a palette for 8 bits or fewer, then rows from the bottom. `in_icon`: the height counts the picture and
/// a 1-bit mask under it (the mask's set bits are clear pixels), as an icon holds it.
fn dib(bytes: &[u8], in_icon: bool) -> Option<Pixels> {
    let d = dib_header(bytes, in_icon)?;
    let mut rgba = vec![0_u8; d.width * d.height * 4];
    let mut any_alpha = false;
    for y in 0..d.height {
        // The file's rows run from the bottom unless the height was negative.
        let from = if d.top_down { y } else { d.height - 1 - y };
        let row = d.rows.get(from * d.stride..(from + 1) * d.stride)?;
        for x in 0..d.width {
            let pixel = d.pixel(row, x)?;
            any_alpha |= pixel[3] != 0;
            let at = (y * d.width + x) * 4;
            rgba.get_mut(at..at + 4)?.copy_from_slice(&pixel);
        }
    }
    // A 32-bit picture with no alpha at all has its transparency in the mask, like the others.
    if !(d.bits == 32 && any_alpha) {
        for y in 0..d.height {
            for x in 0..d.width {
                if let Some(alpha) = rgba.get_mut((y * d.width + x) * 4 + 3) {
                    *alpha = if d.masked(x, y) { 0 } else { 255 };
                }
            }
        }
    }
    Some(Pixels { width: d.width, height: d.height, rgba })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A 2 by 2 icon of 32-bit pixels: red, green over blue, white with alpha 0, 128, 255, 255 (rows from the bottom in the file).
    pub(crate) fn ico_32() -> Vec<u8> {
        let mut dib = Vec::new();
        for n in [40_u32, 2, 4, 0x0020_0001, 0, 0, 0, 0, 0, 0] {
            dib.extend_from_slice(&n.to_le_bytes());
        }
        // Bottom row first: blue and white; then the top row: red and green. Pixels are B, G, R, A.
        dib.extend_from_slice(&[255, 0, 0, 255, 255, 255, 255, 255]);
        dib.extend_from_slice(&[0, 0, 255, 128, 0, 255, 0, 0]);
        // The mask: 2 rows of 4 bytes, all clear bits.
        dib.extend_from_slice(&[0; 8]);
        let mut out = vec![0, 0, 1, 0, 1, 0, 2, 2, 0, 0, 1, 0, 32, 0];
        out.extend_from_slice(&u32::try_from(dib.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&22_u32.to_le_bytes());
        out.extend_from_slice(&dib);
        out
    }

    #[test]
    fn a_32_bit_icon_is_read_with_its_alpha_and_its_rows_from_the_top() {
        let p = decode("ico", &ico_32()).unwrap();
        assert_eq!((p.width, p.height), (2, 2));
        assert_eq!(&p.rgba[0..4], [255, 0, 0, 128], "the top left is red at alpha 128");
        assert_eq!(&p.rgba[4..8], [0, 255, 0, 0], "the top right is green and clear");
        assert_eq!(&p.rgba[8..12], [0, 0, 255, 255], "the bottom left is blue");
        assert_eq!(&p.rgba[12..16], [255, 255, 255, 255]);
    }

    #[test]
    fn premultiplying_scales_each_colour_by_its_alpha() {
        let p = Pixels { width: 2, height: 1, rgba: vec![200, 100, 50, 128, 10, 20, 30, 255] };
        assert_eq!(p.premultiplied(), [100, 50, 25, 128, 10, 20, 30, 255]);
        let clear = Pixels { width: 1, height: 1, rgba: vec![255, 255, 255, 0] };
        assert_eq!(clear.premultiplied(), [0, 0, 0, 0]);
    }

    #[test]
    fn a_24_bit_icon_takes_its_clear_pixels_from_the_mask() {
        let mut dib = Vec::new();
        for n in [40_u32, 2, 4, 0x0018_0001, 0, 0, 0, 0, 0, 0] {
            dib.extend_from_slice(&n.to_le_bytes());
        }
        // Rows of 2 pixels (6 bytes) padded to 8: the bottom row blue, blue; the top row red, red.
        dib.extend_from_slice(&[255, 0, 0, 255, 0, 0, 0, 0]);
        dib.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]);
        // The mask: the bottom row clear at its first pixel (bit 7 of the first byte), the top row all solid.
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0, 0, 0, 0, 0]);
        let mut out = vec![0, 0, 1, 0, 1, 0, 2, 2, 0, 0, 1, 0, 24, 0];
        out.extend_from_slice(&u32::try_from(dib.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&22_u32.to_le_bytes());
        out.extend_from_slice(&dib);
        let p = decode("ico", &out).unwrap();
        assert_eq!(&p.rgba[0..4], [255, 0, 0, 255]);
        assert_eq!(&p.rgba[8..12], [0, 0, 255, 0], "the masked pixel is clear");
        assert_eq!(&p.rgba[12..16], [0, 0, 255, 255]);
    }

    /// A 4 by 4 PNG of one colour.
    pub(crate) fn png_4x4(rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = ::png::Encoder::new(&mut out, 4, 4);
        encoder.set_color(::png::ColorType::Rgba);
        encoder.set_depth(::png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba.repeat(16)).unwrap();
        writer.finish().unwrap();
        out
    }

    #[test]
    fn a_png_and_a_png_inside_an_icon_are_read() {
        let png_bytes = png_4x4([10, 20, 30, 255]);
        let p = decode("png", &png_bytes).unwrap();
        assert_eq!((p.width, p.height, &p.rgba[0..4]), (4, 4, [10, 20, 30, 255].as_slice()));
        let mut ico = vec![0, 0, 1, 0, 1, 0, 4, 4, 0, 0, 1, 0, 32, 0];
        ico.extend_from_slice(&u32::try_from(png_bytes.len()).unwrap().to_le_bytes());
        ico.extend_from_slice(&22_u32.to_le_bytes());
        ico.extend_from_slice(&png_bytes);
        assert_eq!(decode("ico", &ico).unwrap(), p);
    }

    #[test]
    fn the_larger_picture_of_an_icon_is_the_one_read() {
        // Two entries: a 2 by 2 bitmap and a 4 by 4 PNG; the PNG is larger.
        let small = ico_32();
        let dib = &small[22..];
        let png_bytes = png_4x4([1, 2, 3, 255]);
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        ico.extend_from_slice(&[2, 2, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&u32::try_from(dib.len()).unwrap().to_le_bytes());
        ico.extend_from_slice(&38_u32.to_le_bytes());
        ico.extend_from_slice(&[4, 4, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&u32::try_from(png_bytes.len()).unwrap().to_le_bytes());
        ico.extend_from_slice(&u32::try_from(38 + dib.len()).unwrap().to_le_bytes());
        ico.extend_from_slice(dib);
        ico.extend_from_slice(&png_bytes);
        let p = decode("ico", &ico).unwrap();
        assert_eq!((p.width, p.height), (4, 4));
    }

    #[test]
    fn hostile_bytes_are_none_and_never_a_panic() {
        // Every prefix and every single-byte change of a good icon, a PNG and a bitmap.
        let good = [ico_32(), png_4x4([1, 2, 3, 4])];
        for bytes in &good {
            for cut in 0..bytes.len() {
                let _ = decode("ico", &bytes[..cut]);
                let _ = decode("png", &bytes[..cut]);
                let _ = decode("bmp", &bytes[..cut]);
            }
            for at in 0..bytes.len() {
                for value in [0_u8, 1, 0x7f, 0x80, 0xff] {
                    let mut bad = bytes.clone();
                    bad[at] = value;
                    let _ = decode("ico", &bad);
                    let _ = decode("png", &bad);
                    let _ = decode("bmp", &bad);
                }
            }
        }
        assert!(decode("ico", &[]).is_none() && decode("gif", &ico_32()).is_none() && decode("emf", &[0; 64]).is_none());
        // A picture that says it is 65,535 pixels wide and has no pixels: refused before anything is made.
        let mut huge = ico_32();
        huge[22 + 4..22 + 8].copy_from_slice(&65_535_u32.to_le_bytes());
        assert!(decode("ico", &huge).is_none());
        // An icon that says it holds a picture far past its end.
        let mut far = ico_32();
        far[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode("ico", &far).is_none());
    }

    #[test]
    fn a_bitmap_file_with_a_palette_is_read() {
        // A 2 by 2, 8-bit bitmap file: a palette of two entries (black, orange), rows from the bottom, each padded to 4 bytes.
        let mut dib = Vec::new();
        for n in [40_u32, 2, 2, 0x0008_0001, 0, 0, 0, 0, 2, 0] {
            dib.extend_from_slice(&n.to_le_bytes());
        }
        dib.extend_from_slice(&[0, 0, 0, 0, 0, 128, 255, 0]);
        dib.extend_from_slice(&[1, 0, 0, 0, 0, 1, 0, 0]);
        let mut file = b"BM".to_vec();
        file.extend_from_slice(&[0; 12]);
        file.extend_from_slice(&dib);
        let p = decode("bmp", &file).unwrap();
        assert_eq!((p.width, p.height), (2, 2));
        // The top row is the file's second: palette 0 then 1 is black then orange; the bottom row is orange then black.
        assert_eq!(&p.rgba[0..4], [0, 0, 0, 255]);
        assert_eq!(&p.rgba[4..8], [255, 128, 0, 255]);
        assert_eq!(&p.rgba[8..12], [255, 128, 0, 255]);
    }
}
