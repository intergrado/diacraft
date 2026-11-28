//! A small deterministic random stream (SplitMix64): the same seed gives the same cases on every run, on every machine.

/// SplitMix64: a 64-bit state, and a mixed 64-bit word out for each step. Not for anything but making test cases.
#[derive(Clone, Debug)]
pub struct Rng(u64, bool);

impl Rng {
    /// A stream from `seed`.
    pub fn new(seed: u64) -> Rng {
        Rng(seed, false)
    }

    /// The stream, asked for the heavy sizes as well (`DIACRAFT_DOOR_BIG`): [`Rng::size`] then picks among them too.
    #[must_use]
    pub fn big(mut self, on: bool) -> Rng {
        self.1 = on;
        self
    }

    /// A stream for one case: the base seed, the name of what is swept and the case's number, mixed, so that each case has a stream of its own and a case can be replayed alone.
    pub fn for_case(base: u64, what: &str, case: usize) -> Rng {
        let mut h = base ^ 0xCBF2_9CE4_8422_2325;
        for b in what.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        let mut rng = Rng(h ^ (case as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15), false);
        // Two steps to spread a small case number through the state.
        rng.word();
        rng.word();
        rng
    }

    /// The next 64 bits.
    pub fn word(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::try_from(self.word() % (n as u64)).unwrap_or(0) }
    }

    /// A number in `lo..=hi` (`lo` when `hi` is not above it).
    pub fn between(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo { lo } else { lo + self.below(hi - lo + 1) }
    }

    /// True once in `n`.
    pub fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    /// One of `items`, or `None` for an empty list.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }

    /// One of `modest` sizes, or, when the stream is asked for the heavy ones, of `modest` and `heavy` together.
    pub fn size(&mut self, modest: &[usize], heavy: &[usize]) -> usize {
        let pool = if self.1 { [modest, heavy].concat() } else { modest.to_vec() };
        pool.get(self.below(pool.len())).copied().unwrap_or(0)
    }

    /// A byte.
    pub fn byte(&mut self) -> u8 {
        u8::try_from(self.word() & 0xFF).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_the_same_on_every_run_and_a_case_has_a_stream_of_its_own() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!((0..8).map(|_| a.word()).collect::<Vec<_>>(), (0..8).map(|_| b.word()).collect::<Vec<_>>());
        assert_eq!(Rng::new(0).word(), 0xE220_A839_7B1D_CDAF, "the first word of SplitMix64 from 0, a published value");
        let first = |case| Rng::for_case(1, "vsdx", case).word();
        assert_ne!(first(0), first(1));
        assert_ne!(Rng::for_case(1, "vsdx", 5).word(), Rng::for_case(1, "odg", 5).word());
        assert_eq!(first(3), first(3));
    }

    #[test]
    fn below_and_between_stay_in_their_ranges_and_an_empty_range_is_its_start() {
        let mut r = Rng::new(3);
        for _ in 0..1000 {
            assert!(r.below(7) < 7);
            assert!((5..=9).contains(&r.between(5, 9)));
        }
        assert_eq!((r.below(0), r.between(4, 4), r.between(9, 2)), (0, 4, 9));
        assert!(r.pick::<u8>(&[]).is_none());
    }

    #[test]
    fn a_size_is_a_modest_one_unless_the_stream_is_asked_for_the_heavy_ones() {
        let mut modest = Rng::new(5);
        assert!((0..200).all(|_| matches!(modest.size(&[1, 2], &[1000]), 1 | 2)));
        let mut heavy = Rng::new(5).big(true);
        let sizes: Vec<usize> = (0..200).map(|_| heavy.size(&[1, 2], &[1000])).collect();
        assert!(sizes.contains(&1000) && sizes.contains(&1) && sizes.iter().all(|s| matches!(s, 1 | 2 | 1000)));
        assert_eq!(Rng::new(1).size(&[], &[7]), 0, "no modest size and no heavy ones asked for: nothing");
    }
}
