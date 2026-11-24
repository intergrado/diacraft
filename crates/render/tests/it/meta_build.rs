//! Metafiles composed in code for the render tests, in the notation of the metafile vectors (Test vectors): `r(t, fields)` is an EMF
//! record, `emf` a file made of the header `H` and its records, `c` a comment of EMF+ records, `p` one EMF+ record. The header's frame is
//! 80 by 40 device pixels, so a picture box of 2 inches by 1 inch has 40 pixels to the inch.

pub fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn i32s(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn u16s(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn i16s(v: &[i16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn r(t: u32, fields: &[u8]) -> Vec<u8> {
    let mut body = fields.to_vec();
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    let mut out = u32s(&[t, 8 + body.len() as u32]);
    out.extend(body);
    out
}

/// The header `H`: bounds (0, 0, 79, 39), frame 2000 by 1000 hundredths of a millimetre, a device of 320 by 160 pixels in 80 by 40
/// millimetres.
pub fn header(total: u32) -> Vec<u8> {
    let mut f = i32s(&[0, 0, 79, 39, 0, 0, 2000, 1000]);
    f.extend(u32s(&[0x464D_4520, 0x10000, total, 3]));
    f.extend(u16s(&[8, 0]));
    f.extend(u32s(&[0, 0, 0]));
    f.extend(i32s(&[320, 160, 80, 40]));
    f.extend(u32s(&[0, 0, 0, 80_000, 40_000]));
    r(1, &f)
}

pub fn emf(records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = records.iter().flatten().copied().chain(r(14, &u32s(&[0, 16, 20]))).collect();
    let mut out = header(108 + body.len() as u32);
    out.extend(body);
    out
}

pub fn p(t: u16, flags: u16, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    while !d.len().is_multiple_of(4) {
        d.push(0);
    }
    let mut out = u16s(&[t, flags]);
    out.extend(u32s(&[12 + d.len() as u32, d.len() as u32]));
    out.extend(d);
    out
}

pub fn c(plus: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = plus.iter().flatten().copied().collect();
    let mut f = u32s(&[4 + body.len() as u32, 0x2B46_4D45]);
    f.extend(body);
    r(70, &f)
}

pub fn hp_flags(flags: u16) -> Vec<u8> {
    c(&[p(0x4001, flags, &u32s(&[0xDBC0_1002, 1, 96, 96]))])
}

pub fn hp() -> Vec<u8> {
    hp_flags(1)
}

pub fn argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// A COLORREF.
pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

pub fn plus_fill_rect(color: u32, rect: [f32; 4]) -> Vec<u8> {
    let mut d = u32s(&[color, 1]);
    d.extend(f32s(&rect));
    c(&[p(0x400A, 0x8000, &d)])
}

pub fn end_plus() -> Vec<u8> {
    c(&[p(0x4002, 0, &[])])
}

/// A solid brush as object `i`, selected, and the null pen selected: the next shapes are filled with the colour and have no outline.
pub fn brush_no_pen(i: u32, color: u32) -> Vec<Vec<u8>> {
    vec![r(39, &u32s(&[i, 0, color, 0])), r(37, &u32s(&[i])), r(37, &u32s(&[0x8000_0008]))]
}

/// A pen of `width` logical units in `color` as object `i`, selected, and the null brush selected: the next shapes are outlines only.
pub fn pen_no_brush(i: u32, width: i32, color: u32) -> Vec<Vec<u8>> {
    vec![r(38, &[u32s(&[i, 0]), i32s(&[width, 0]), u32s(&[color])].concat()), r(37, &u32s(&[i])), r(37, &u32s(&[0x8000_0005]))]
}

pub fn rect_record(l: i32, t: i32, rr: i32, b: i32) -> Vec<u8> {
    r(43, &i32s(&[l, t, rr, b]))
}

/// A 24-bit bitmap, `rgb` top row first, as `(info, bits)`.
pub fn dib_24(w: u32, h: u32, px: &[[u8; 3]]) -> (Vec<u8>, Vec<u8>) {
    let mut info = u32s(&[40]);
    info.extend(i32s(&[w as i32, h as i32]));
    info.extend(u16s(&[1, 24]));
    info.extend(u32s(&[0, 0, 0, 0, 0, 0]));
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut bits = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let c = px.get(y * w as usize + x).copied().unwrap_or([0, 0, 0]);
            let row = h as usize - 1 - y;
            bits[row * stride + x * 3..row * stride + x * 3 + 3].copy_from_slice(&[c[2], c[1], c[0]]);
        }
    }
    (info, bits)
}

/// `EMR_STRETCHDIBITS` of the whole bitmap into `dest` (`[x, y, w, h]` in device pixels).
pub fn stretch_dibits(dest: [i32; 4], dib: &(Vec<u8>, Vec<u8>), rop: u32) -> Vec<u8> {
    let (info, bits) = dib;
    let info_len = info.len().div_ceil(4) * 4;
    let (sw, sh) = (i32::from_le_bytes(info[4..8].try_into().unwrap()), i32::from_le_bytes(info[8..12].try_into().unwrap()));
    let mut f = i32s(&[dest[0], dest[1], dest[0] + dest[2], dest[1] + dest[3]]);
    f.extend(i32s(&[dest[0], dest[1], 0, 0, sw, sh]));
    f.extend(u32s(&[80, info.len() as u32, 80 + info_len as u32, bits.len() as u32, 0, rop]));
    f.extend(i32s(&[dest[2], dest[3]]));
    let mut padded = info.clone();
    padded.resize(info_len, 0);
    f.extend(padded);
    f.extend(bits);
    r(81, &f)
}

/// `EMR_EXTCREATEFONTINDIRECTW` as object `index`: a height, a face name and a character set.
pub fn font_record(index: u32, height: i32, face: &str, charset: u8) -> Vec<u8> {
    let mut f = u32s(&[index]);
    f.extend(i32s(&[height, 0, 0, 0, 400]));
    f.extend([0, 0, 0, charset, 0, 0, 0, 0]);
    let mut name: Vec<u16> = face.encode_utf16().collect();
    name.resize(32, 0);
    f.extend(u16s(&name));
    f.extend(vec![0u8; 356 - 8 - 4 - 92]);
    r(82, &f)
}

/// `EMR_EXTTEXTOUTW` at `reference`, with the string and optional advances.
pub fn text_record(reference: [i32; 2], chars: &str, advances: &[i32], options: u32) -> Vec<u8> {
    let units: Vec<u16> = chars.encode_utf16().collect();
    let n = units.len() as u32;
    let mut f = i32s(&[0, 0, 0, 0]);
    f.extend(u32s(&[1]));
    f.extend(f32s(&[0.0, 0.0]));
    f.extend(i32s(&reference));
    let dx_at = 76 + (n * 2).div_ceil(4) * 4;
    f.extend(u32s(&[n, 76, options]));
    f.extend(i32s(&[0, 0, 0, 0]));
    f.extend(u32s(&[if advances.is_empty() { 0 } else { dx_at }]));
    let mut s = u16s(&units);
    while !s.len().is_multiple_of(4) {
        s.push(0);
    }
    f.extend(s);
    f.extend(i32s(advances));
    r(84, &f)
}

pub fn w(func: u16, params: &[u8]) -> Vec<u8> {
    let mut body = params.to_vec();
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    let mut out = (((6 + body.len()) / 2) as u32).to_le_bytes().to_vec();
    out.extend(func.to_le_bytes());
    out.extend(body);
    out
}

pub fn wmf(records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = records.iter().flatten().copied().chain(w(0, &[])).collect();
    let mut out = u16s(&[1, 9, 0x0300]);
    out.extend(u32s(&[((18 + body.len()) / 2) as u32]));
    out.extend(u16s(&[8]));
    out.extend(u32s(&[0]));
    out.extend(u16s(&[0]));
    out.extend(body);
    out
}
