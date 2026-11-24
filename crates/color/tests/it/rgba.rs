//! `Rgba`: hex text, HSL, contrast and mixing.

use diacraft_color::Rgba;

#[test]
fn from_hex_rejects_multiple_hashes() {
    assert_eq!(Rgba::from_hex("##FF0000"), None);
    assert_eq!(Rgba::from_hex("####FF0000"), None);
    assert_eq!(Rgba::from_hex("#FF0000"), Some(Rgba::rgb(255, 0, 0)));
    assert_eq!(Rgba::from_hex("FF0000"), Some(Rgba::rgb(255, 0, 0)));
    assert_eq!(Rgba::from_hex("#FF000080"), Some(Rgba::rgba(255, 0, 0, 128)));
}

#[test]
fn from_hex_rejects_signs() {
    assert_eq!(Rgba::from_hex("+12345"), None);
    assert_eq!(Rgba::from_hex("#+1+2+3"), None);
    assert_eq!(Rgba::from_hex("+1+2+3+4"), None);
    assert_eq!(Rgba::from_hex("+12"), None);
}

#[test]
fn hex_roundtrip() {
    assert_eq!(Rgba::from_hex("#156082"), Some(Rgba::rgb(0x15, 0x60, 0x82)));
    assert_eq!(Rgba::rgb(0x15, 0x60, 0x82).hex(), "156082");
    assert_eq!(Rgba::from_hex("fff"), Some(Rgba::WHITE));
    assert_eq!(Rgba::from_hex("zz0000"), None);
    assert_eq!(Rgba::from_hex("\u{e9}\u{e9}\u{e9}"), None);
    assert_eq!(Rgba::from_hex(""), None);
}

/// A colour literal is `#` and six hex digits, red first, and `V` holds the same form.
#[test]
fn a_formula_colour_literal_reads_red_first() {
    assert_eq!(Rgba::from_hex("#FF8000"), Some(Rgba::rgb(255, 128, 0)));
    assert_eq!(Rgba::from_hex("#ff8000"), Some(Rgba::rgb(255, 128, 0)));
    assert_eq!(Rgba::rgb(255, 128, 0).hex(), "FF8000");
}

#[test]
fn hsl_roundtrip() {
    for c in [Rgba::rgb(10, 200, 30), Rgba::rgb(255, 0, 0), Rgba::rgb(128, 128, 128), Rgba::rgb(21, 96, 130)] {
        let (h, s, l) = c.to_hsl();
        let d = Rgba::from_hsl(h, s, l, 255);
        assert!((d.r as i32 - c.r as i32).abs() <= 1 && (d.g as i32 - c.g as i32).abs() <= 1 && (d.b as i32 - c.b as i32).abs() <= 1, "{c:?} {d:?}");
    }
}

#[test]
fn contrast_ratio() {
    assert!((Rgba::BLACK.contrast(Rgba::WHITE) - 21.0).abs() < 0.01);
    assert!((Rgba::WHITE.contrast(Rgba::WHITE) - 1.0).abs() < 1e-9);
}

/// Hostile numbers give a colour and never panic: a NaN or an infinity is black or white, never a cast of NaN.
#[test]
fn non_finite_components_give_a_colour() {
    assert_eq!(Rgba::from_f64(f64::NAN, f64::NAN, f64::NAN, f64::NAN), Rgba::TRANSPARENT);
    assert_eq!(Rgba::from_f64(f64::INFINITY, f64::NEG_INFINITY, 2.0, -1.0), Rgba { r: 0, g: 0, b: 255, a: 0 });
    let _ = Rgba::from_hsl(f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 255);
    assert_eq!(Rgba::BLACK.lerp(Rgba::WHITE, f64::NAN), Rgba::BLACK);
    assert_eq!(Rgba::BLACK.lerp(Rgba::WHITE, 2.0), Rgba::WHITE);
    assert_eq!(Rgba::BLACK.lerp(Rgba::WHITE, 0.5), Rgba::rgb(128, 128, 128));
}

/// The grey a print in black and white draws for a colour. BT.601's weights in thousandths, rounded half up, alpha kept.
#[test]
fn the_grey_of_a_colour_is_its_luma_by_the_bt601_weights_and_keeps_its_alpha() {
    use diacraft_color::luma;
    // 0.299 * 255 = 76.245, 0.587 * 255 = 149.685, 0.114 * 255 = 29.07.
    assert_eq!((luma(255, 0, 0), luma(0, 255, 0), luma(0, 0, 255)), (76, 150, 29));
    assert_eq!(luma(255, 255, 0), 226, "76.245 + 149.685 = 225.93");
    assert_eq!((luma(0, 0, 0), luma(255, 255, 255)), (0, 255));
    // 0.299 * 224 + 0.587 * 32 + 0.114 * 32 = 89.408.
    assert_eq!(Rgba::rgba(224, 32, 32, 128).gray(), Rgba::rgba(89, 89, 89, 128));
    assert_eq!(Rgba::TRANSPARENT.gray(), Rgba::TRANSPARENT);
}

/// Every colour: the grey lies between the least and the most of the three, and a grey is its own. The theme transform `Gray` works the same
/// weights in floating point: it gives the same grey but at the 3,464 colours whose luma is exactly a half, where the sum in floating point
/// lands a hair under the half and rounds down, one step darker.
#[test]
fn every_colours_grey_lies_between_its_channels_and_a_grey_is_its_own() {
    use diacraft_color::{ColorTransform, apply, luma};
    for v in 0..=255u8 {
        assert_eq!(luma(v, v, v), v);
    }
    let disagree = std::sync::atomic::AtomicU64::new(0);
    std::thread::scope(|scope| {
        for r in (0..=255u8).step_by(32) {
            let disagree = &disagree;
            scope.spawn(move || {
                for r in r..=r.saturating_add(31) {
                    for g in 0..=255u8 {
                        for b in 0..=255u8 {
                            let y = luma(r, g, b);
                            assert!(y >= r.min(g).min(b) && y <= r.max(g).max(b), "{r} {g} {b} -> {y}");
                            let theirs = apply(Rgba::rgb(r, g, b), &[ColorTransform::Gray]);
                            if theirs != Rgba::rgb(y, y, y) {
                                let exact = 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b);
                                assert!(exact % 1000 == 500 && theirs == Rgba::rgb(y - 1, y - 1, y - 1), "{r} {g} {b}: {y} against {theirs:?}");
                                disagree.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                }
            });
        }
    });
    assert_eq!(disagree.into_inner(), 3_464, "colours whose grey is a step lighter than the theme transform's");
}

#[test]
fn pixels_turn_to_grey_in_place_and_a_premultiplied_pixel_stays_one() {
    use diacraft_color::gray_pixels;
    // Straight red, a premultiplied half-transparent blue, a clear pixel, and two bytes that are not a whole pixel.
    let mut pixels = [255, 0, 0, 255, 0, 0, 128, 128, 0, 0, 0, 0, 9, 9];
    gray_pixels(&mut pixels);
    assert_eq!(pixels, [76, 76, 76, 255, 15, 15, 15, 128, 0, 0, 0, 0, 9, 9]);
    // No channel of a premultiplied pixel is above its alpha, and its grey is not either.
    for a in 0..=255u8 {
        let mut px = [a, 0, a, a];
        gray_pixels(&mut px);
        assert!(px[0] <= a && px[0] == px[1] && px[1] == px[2] && px[3] == a, "{a}: {px:?}");
    }
}
