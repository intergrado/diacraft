//! Bounds-checked little-endian access to hostile input. Every read returns `None` past the end, so a truncated or lying size field stops
//! the read instead of panicking.

#[derive(Clone, Copy)]
pub(crate) struct Bytes<'a>(pub(crate) &'a [u8]);

impl<'a> Bytes<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `len` bytes from `off`, or `None` when they are not all there.
    pub fn slice(&self, off: usize, len: usize) -> Option<&'a [u8]> {
        self.0.get(off..off.checked_add(len)?)
    }

    /// The bytes from `off` to `end`, as a new reader.
    pub fn sub(&self, off: usize, len: usize) -> Option<Bytes<'a>> {
        self.slice(off, len).map(Bytes)
    }

    fn arr<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        self.slice(off, N)?.try_into().ok()
    }

    pub fn u8(&self, off: usize) -> Option<u8> {
        self.0.get(off).copied()
    }

    pub fn u16(&self, off: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.arr(off)?))
    }

    pub fn i16(&self, off: usize) -> Option<i16> {
        Some(i16::from_le_bytes(self.arr(off)?))
    }

    pub fn u32(&self, off: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.arr(off)?))
    }

    pub fn i32(&self, off: usize) -> Option<i32> {
        Some(i32::from_le_bytes(self.arr(off)?))
    }

    /// A 32-bit float; `None` past the end and for a value that is not a finite number.
    pub fn f32(&self, off: usize) -> Option<f32> {
        let v = f32::from_le_bytes(self.arr(off)?);
        v.is_finite().then_some(v)
    }

    pub fn f64_i32(&self, off: usize) -> Option<f64> {
        self.i32(off).map(f64::from)
    }

    pub fn f64_f32(&self, off: usize) -> Option<f64> {
        self.f32(off).map(f64::from)
    }

    /// A `u32` count as a `usize`.
    pub fn count(&self, off: usize) -> Option<usize> {
        usize::try_from(self.u32(off)?).ok()
    }

    /// The `RECTL` at `off` (left, top, right, bottom) as numbers.
    pub fn rect_i32(&self, off: usize) -> Option<[f64; 4]> {
        Some([self.f64_i32(off)?, self.f64_i32(off + 4)?, self.f64_i32(off + 8)?, self.f64_i32(off + 12)?])
    }

    /// The `POINTL` at `off` as numbers.
    pub fn point_i32(&self, off: usize) -> Option<[f64; 2]> {
        Some([self.f64_i32(off)?, self.f64_i32(off + 4)?])
    }

    /// The `XForm` at `off`: six floats, M11, M12, M21, M22, Dx, Dy.
    pub fn xform(&self, off: usize) -> Option<[f64; 6]> {
        let mut m = [0.0; 6];
        for (i, v) in m.iter_mut().enumerate() {
            *v = self.f64_f32(off.checked_add(i.checked_mul(4)?)?)?;
        }
        Some(m)
    }

    /// `n` UTF-16 code units from `off`.
    pub fn units(&self, off: usize, n: usize) -> Option<Vec<u16>> {
        let raw = self.slice(off, n.checked_mul(2)?)?;
        Some(raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }
}

/// UTF-16 to characters: a lone surrogate becomes U+FFFD.
pub(crate) fn decode_utf16(units: &[u16]) -> Vec<char> {
    char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}
