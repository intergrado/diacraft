//! Fuzzing (AGENTS.md: importers and parsers are fuzzed): bytes of any kind, and valid metafiles with bytes flipped, cut, inserted and
//! repeated, give a picture or an error and never a panic, and what they give holds the rules of the display list: every number is finite,
//! every item names a clip that is there, and a clip is within its caps.
//!
//! These run under `proptest` with a fixed budget of cases; a failure prints the input that made it.

use diacraft_metafile::limits::{MAX_CLIP_PARTS, MAX_CLIP_POINTS};
use diacraft_metafile::{Drawn, Paint, Picture, Placement, Seg, parse};
use proptest::prelude::*;

use crate::build::*;

fn finite_path(path: &diacraft_metafile::Path) -> bool {
    path.segs.iter().all(|s| match *s {
        Seg::Move(a) | Seg::Line(a) => a.iter().all(|v| v.is_finite()),
        Seg::Cubic(a, b, c) => [a, b, c].iter().all(|p| p.iter().all(|v| v.is_finite())),
        Seg::Close => true,
    })
}

fn finite_paint(p: &Paint) -> bool {
    match p {
        Paint::Solid(_) => true,
        Paint::Linear(l) => l.start.iter().chain(l.end.iter()).all(|v| v.is_finite()) && l.stops.iter().all(|s| s.0.is_finite()),
        Paint::Radial(r) => r.center.iter().chain(r.basis.iter()).all(|v| v.is_finite()),
    }
}

/// What every picture that reads must hold.
fn check(pic: &Picture) {
    assert!(pic.width.is_finite() && pic.width > 0.0 && pic.height.is_finite() && pic.height > 0.0, "{} by {}", pic.width, pic.height);
    assert!(pic.clips.first().is_some_and(|c| c.parts.is_empty()), "clip 0 is no clip");
    for c in &pic.clips {
        assert!(c.parts.len() <= MAX_CLIP_PARTS);
        let points: usize = c.parts.iter().map(|p| p.path.segs.len()).sum();
        assert!((points as u64) <= MAX_CLIP_POINTS);
        assert!(c.parts.iter().all(|p| finite_path(&p.path)));
    }
    for it in &pic.items {
        assert!((it.clip as usize) < pic.clips.len(), "an item names clip {} of {}", it.clip, pic.clips.len());
        match &it.drawn {
            Drawn::Path(p) => {
                assert!(finite_path(&p.path));
                assert!(p.fill.as_ref().is_none_or(|f| finite_paint(&f.paint)));
                assert!(p.stroke.as_ref().is_none_or(|s| finite_paint(&s.paint) && s.width.is_finite() && s.dashes.iter().all(|d| d.is_finite())));
            }
            Drawn::Bitmap(b) => {
                assert!(b.quad.iter().all(|p| p.iter().all(|v| v.is_finite())));
                if let diacraft_metafile::Image::Rgba { width, height, data } = &b.image {
                    assert_eq!(data.len() as u64, u64::from(*width) * u64::from(*height) * 4);
                }
            }
            Drawn::Text(t) => {
                assert!(t.size.is_finite() && t.size > 0.0 && t.basis.iter().all(|v| v.is_finite()));
                match &t.placement {
                    Placement::Origins(o) => {
                        assert!(o.iter().all(|p| p.iter().all(|v| v.is_finite())));
                        assert_eq!(o.len(), t.text.chars().count(), "a position for each character");
                    }
                    Placement::Flow { origin, .. } => assert!(origin.iter().all(|v| v.is_finite())),
                }
            }
        }
    }
}

/// The seeds: valid metafiles of every kind the crate reads, each of which must read to something.
fn seeds() -> Vec<Vec<u8>> {
    let blue = argb(0, 0, 255, 255);
    let g = gdi_red_right_half();
    let mut dual = vec![hp(), plus_fill_rect(blue, [0.0, 0.0, 40.0, 40.0])];
    dual.extend(g.iter().cloned());
    dual.push(c(&[p(0x4002, 0, &[])]));
    let mut unknown = dual.clone();
    unknown.insert(1, c(&[p(0x40F0, 0, &[])]));
    let brush = p(
        0x4008,
        0x0101,
        &[u32s(&[0xDBC0_1002, 4, 0, 0]), f32s(&[0.0, 0.0, 80.0, 40.0]), u32s(&[argb(255, 0, 0, 255), argb(0, 0, 255, 255), 0, 0])].concat(),
    );
    let gradient = vec![hp(), c(&[brush, p(0x400A, 0, &[u32s(&[1, 1]), f32s(&[0.0, 0.0, 80.0, 40.0])].concat())])];
    let mut triangle = i32s(&[0, 0, 80, 40]);
    triangle.extend(u32s(&[3]));
    triangle.extend(i16s(&[0, 0, 80, 0, 40, 40]));
    let clipped = vec![r(59, &[]), r(86, &triangle), r(60, &[]), r(67, &u32s(&[1])), r(43, &i32s(&[0, 0, 80, 40]))];
    let bitmaps = vec![
        stretch_dibits([0, 0, 80, 40], &dib_1bit_mask(), 0x00EE_0086),
        stretch_dibits([0, 0, 80, 40], &dib_24(2, 2, &[[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]]), 0x00CC_0020),
    ];
    let inner = emf(&[r(43, &i32s(&[50, 25, 51, 26]))]);
    let mut image = u32s(&[0xDBC0_1002, 2, 4, inner.len() as u32]);
    image.extend(&inner);
    let draw = p(
        0x401B,
        0x0001,
        &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 100.0, 50.0]), u32s(&[3]), f32s(&[10.0, 10.0, 110.0, 10.0, 10.0, 60.0])].concat(),
    );
    let nested = vec![hp(), c(&[p(0x4008, 0x0501, &image), draw])];
    let text = {
        let mut f = i32s(&[0, 0, 0, 0]);
        f.extend(u32s(&[1]));
        f.extend(f32s(&[0.0, 0.0]));
        f.extend(i32s(&[10, 20]));
        f.extend(u32s(&[2, 76, 0]));
        f.extend(i32s(&[0, 0, 0, 0]));
        f.extend(u32s(&[0]));
        f.extend(u16s(&[0x41, 0x42]));
        r(84, &f)
    };
    let walk = vec![wmf(&[w(0x020B, &i16s(&[0, 0])), w(0x020C, &i16s(&[100, 200])), w(0x0325, &[i16s(&[2]), i16s(&[0, 0, 200, 100])].concat())])];
    let mut seeds: Vec<Vec<u8>> = [dual, unknown, gradient, clipped, bitmaps, nested, vec![text]].iter().map(|recs| emf(recs)).collect();
    seeds.extend(walk);
    seeds
}

#[test]
fn the_seeds_of_the_mutation_fuzz_read_so_that_the_mutations_start_from_something() {
    for (i, seed) in seeds().iter().enumerate() {
        let pic = parse(seed).unwrap_or_else(|e| panic!("seed {i} does not read: {e}"));
        assert!(!pic.items.is_empty(), "seed {i} draws nothing");
        check(&pic);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn bytes_of_any_kind_give_a_picture_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        if let Ok(pic) = parse(&bytes) {
            check(&pic);
        }
    }

    #[test]
    fn bytes_that_start_like_an_emf_give_a_picture_and_hold_the_rules(tail in proptest::collection::vec(any::<u8>(), 0..3000)) {
        // The first 88 bytes of a header that reads, then anything.
        let mut bytes = emf(&[])[..88].to_vec();
        bytes.extend(tail);
        if let Ok(pic) = parse(&bytes) {
            check(&pic);
        }
    }

    #[test]
    fn bytes_that_start_like_an_emf_plus_file_hold_the_rules(tail in proptest::collection::vec(any::<u8>(), 0..3000)) {
        let mut bytes = emf(&[hp()]);
        bytes.truncate(bytes.len() - 20);
        bytes.extend(tail);
        if let Ok(pic) = parse(&bytes) {
            check(&pic);
        }
    }

    #[test]
    fn bytes_that_start_like_a_wmf_hold_the_rules(tail in proptest::collection::vec(any::<u8>(), 0..3000)) {
        let mut bytes = wmf(&[])[..18].to_vec();
        bytes.extend(tail);
        if let Ok(pic) = parse(&bytes) {
            check(&pic);
        }
    }

    #[test]
    fn a_valid_metafile_with_bytes_changed_cut_or_added_holds_the_rules(
        which in 0usize..8,
        edits in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>(), 0u8..4), 0..24),
        cut in proptest::option::of(any::<prop::sample::Index>()),
    ) {
        let seeds = seeds();
        let mut bytes = seeds[which % seeds.len()].clone();
        for (at, value, how) in edits {
            if bytes.is_empty() {
                break;
            }
            let i = at.index(bytes.len());
            match how {
                0 => bytes[i] = value,
                1 => bytes[i] ^= 1 << (value % 8),
                2 => bytes.insert(i, value),
                _ => {
                    // A size field made huge or small.
                    let j = i & !3;
                    if j + 4 <= bytes.len() {
                        bytes[j..j + 4].copy_from_slice(&(u32::from(value) << (value % 25)).to_le_bytes());
                    }
                }
            }
        }
        if let Some(c) = cut {
            let n = c.index(bytes.len() + 1);
            bytes.truncate(n);
        }
        if let Ok(pic) = parse(&bytes) {
            check(&pic);
        }
    }
}
