//! Metafiles composed in code, in the notation of (Test vectors): `R(t; ...)` is an EMF record, `H` the 108-byte
//! header, `C(...)` a comment of EMF+ records, `P(t, flags; ...)` one EMF+ record, `END` the end of file. Numbers are little-endian.

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

/// `R(t; fields)`: an EMF record of type `t` whose `Size` is 8 plus its fields (padded to four bytes).
pub fn r(t: u32, fields: &[u8]) -> Vec<u8> {
    let mut body = fields.to_vec();
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    let mut out = u32s(&[t, 8 + body.len() as u32]);
    out.extend(body);
    out
}

/// `END`: `R(14; u32 0, 16, 20)`.
pub fn end() -> Vec<u8> {
    r(14, &u32s(&[0, 16, 20]))
}

/// The parts of the header `H` that a test changes.
#[derive(Clone)]
pub struct Head {
    pub bounds: [i32; 4],
    pub frame: [i32; 4],
    pub device: [i32; 2],
    pub millimeters: [i32; 2],
    pub micrometers: [u32; 2],
    pub handles: u16,
    pub records: u32,
}

impl Default for Head {
    /// `H`: bounds (0, 0, 79, 39), frame (0, 0, 2000, 1000), a device of 320 by 160 pixels in 80 by 40 millimetres: 4 pixels to the
    /// millimetre, so the frame is 80 by 40 device pixels.
    fn default() -> Head {
        Head {
            bounds: [0, 0, 79, 39],
            frame: [0, 0, 2000, 1000],
            device: [320, 160],
            millimeters: [80, 40],
            micrometers: [80_000, 40_000],
            handles: 4,
            records: 3,
        }
    }
}

impl Head {
    pub fn bytes(&self, total: u32) -> Vec<u8> {
        let mut f = i32s(&self.bounds);
        f.extend(i32s(&self.frame));
        f.extend(u32s(&[0x464D_4520, 0x10000, total, self.records]));
        f.extend(u16s(&[self.handles, 0]));
        f.extend(u32s(&[0, 0, 0]));
        f.extend(i32s(&self.device));
        f.extend(i32s(&self.millimeters));
        f.extend(u32s(&[0, 0, 0]));
        f.extend(u32s(&self.micrometers));
        r(1, &f)
    }
}

/// An EMF made of `H` and these records, then `END`.
pub fn emf(records: &[Vec<u8>]) -> Vec<u8> {
    emf_with(&Head::default(), records)
}

pub fn emf_with(head: &Head, records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = records.iter().flatten().copied().chain(end()).collect();
    let mut out = head.bytes((108 + body.len()) as u32);
    out.extend(body);
    out
}

/// `P(t, flags; data)`: one EMF+ record.
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

/// `C(...)`: an `EMR_COMMENT` holding EMF+ records.
pub fn c(plus_records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = plus_records.iter().flatten().copied().collect();
    let mut f = u32s(&[4 + body.len() as u32, 0x2B46_4D45]);
    f.extend(body);
    r(70, &f)
}

/// `HP`: an EMF+ Dual header, `C(P(0x4001, 0x0001; u32 0xDBC01002, 1, 96, 96))`.
pub fn hp() -> Vec<u8> {
    hp_flags(0x0001)
}

pub fn hp_flags(flags: u16) -> Vec<u8> {
    c(&[p(0x4001, flags, &u32s(&[0xDBC0_1002, 1, 96, 96]))])
}

/// An EMF+ colour from red, green, blue and alpha.
pub fn argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// `C(P(0x400A, 0x8000; u32 colour, 1; f32 x, y, w, h))`: one filled rectangle in EMF+ (the V3 blue square when the colour is blue).
pub fn plus_fill_rect(color: u32, rect: [f32; 4]) -> Vec<u8> {
    let mut d = u32s(&[color, 1]);
    d.extend(f32s(&rect));
    c(&[p(0x400A, 0x8000, &d)])
}

/// The GDI half of V3: a red brush selected, the null pen, and a rectangle over the right half.
pub fn gdi_red_right_half() -> Vec<Vec<u8>> {
    vec![r(39, &u32s(&[1, 0, 0x0000_00FF, 0])), r(37, &u32s(&[1])), r(37, &u32s(&[0x8000_0008])), r(43, &i32s(&[40, 0, 80, 40]))]
}

/// A WMF record: the size in words, the function, the parameters.
pub fn w(func: u16, params: &[u8]) -> Vec<u8> {
    let mut body = params.to_vec();
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    let words = (6 + body.len()) / 2;
    let mut out = (words as u32).to_le_bytes().to_vec();
    out.extend(func.to_le_bytes());
    out.extend(body);
    out
}

/// A standard WMF (memory, nine words, version 0x0300) with these records, then the end record.
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

/// A 24-bit bitmap of `w` by `h` pixels, `rgb` top row first, as `(info, bits)`: stored bottom-up, rows padded to four bytes.
pub fn dib_24(w: u32, h: u32, rgb: &[[u8; 3]]) -> (Vec<u8>, Vec<u8>) {
    let mut info = u32s(&[40]);
    info.extend(i32s(&[w as i32, h as i32]));
    info.extend(u16s(&[1, 24]));
    info.extend(u32s(&[0, 0, 0, 0, 0, 0]));
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut bits = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let px = rgb.get(y * w as usize + x).copied().unwrap_or([0, 0, 0]);
            let row = h as usize - 1 - y;
            bits[row * stride + x * 3..row * stride + x * 3 + 3].copy_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    (info, bits)
}

/// An 8 by 8 bitmap of 1 bit a pixel with a palette of black and white, in rows of alternating bits.
pub fn dib_1bit_mask() -> (Vec<u8>, Vec<u8>) {
    let mut info = u32s(&[40]);
    info.extend(i32s(&[8, 8]));
    info.extend(u16s(&[1, 1]));
    info.extend(u32s(&[0, 0, 0, 0, 2, 0]));
    info.extend(u32s(&[0x0000_0000, 0x00FF_FFFF]));
    let mut bits = Vec::new();
    for row in 0..8 {
        bits.extend([if row % 2 == 0 { 0xAA } else { 0x55 }, 0, 0, 0]);
    }
    (info, bits)
}

/// `EMR_STRETCHDIBITS`: the destination `[x, y, w, h]`, the whole bitmap as the source, a raster operation.
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
