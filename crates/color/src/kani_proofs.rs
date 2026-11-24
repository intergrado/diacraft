//! Kani proof harnesses for the byte code of the print in black and white: the luma of a colour ([`luma`]), the grey of a colour ([`Rgba::gray`]) and the
//! grey of a picture's pixels ([`gray_pixels`]). Run with `cargo xtask kani` (`docs/development.md`, Proofs).

use crate::{Rgba, gray_pixels, luma};

/// The weighted sum the luma rounds, in thousandths: 0.299 R + 0.587 G + 0.114 B, the weights of ITU-R BT.601.
fn thousandths(r: u8, g: u8, b: u8) -> u32 {
    299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)
}

/// `luma`: for **every** colour, the luma is the BT.601 sum rounded to the nearest whole number, a half up (`1000 y - 500 <= S < 1000 y + 500`, where `S` is the
/// sum in thousandths), so it lies between the least and the most of the three channels, and it does not overflow. Two-sided: the rounding is pinned from both sides, so a luma
/// one too high or one too low fails. **So the luma is monotone in each channel** (the sum is, and so is its rounding): a harness that asked `luma` of two colours to show it
/// directly, two dividers side by side, ran past 150 s, where this one takes 2 s.
#[kani::proof]
fn the_luma_is_the_weighted_sum_rounded_half_up_and_lies_between_the_channels() {
    let (r, g, b): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    let y = luma(r, g, b);
    let s = thousandths(r, g, b);
    let y1000 = 1000 * u32::from(y);
    assert!(y1000 <= s + 500 && s + 500 < y1000 + 1000);
    assert!(y >= r.min(g).min(b) && y <= r.max(g).max(b));
    kani::cover!(y1000 == s + 500, "a sum that is exactly a half, rounded up");
    kani::cover!(y == 255, "white");
    kani::cover!((y == 0) & (r | g | b != 0), "a colour too dark to be anything but black");
}

/// `luma` of a grey is the grey (the weights add up to 1000): **a picture in black and white is left as it is**.
#[kani::proof]
fn a_grey_is_its_own_luma() {
    let v: u8 = kani::any();
    assert!(luma(v, v, v) == v);
}

/// `Rgba::gray`: the three channels are the luma of the colour's, the alpha is kept, and the grey of a grey is itself (idempotent).
#[kani::proof]
fn the_grey_of_a_colour_has_its_luma_in_each_channel_and_its_alpha() {
    let c = Rgba { r: kani::any(), g: kani::any(), b: kani::any(), a: kani::any() };
    let grey = c.gray();
    let y = luma(c.r, c.g, c.b);
    assert!(grey.r == y && grey.g == y && grey.b == y && grey.a == c.a);
    assert!(grey.gray() == grey);
}

/// `gray_pixels`, on 11 bytes of any values (two whole pixels and three bytes past them): each pixel's red, green and blue become the luma of the three and its
/// alpha is kept (the luma by the rounding law of the first harness); the bytes past the last whole pixel are left as they are; and a premultiplied pixel (no colour channel above its alpha) stays one.
#[kani::proof]
#[kani::unwind(12)] // the comparison of the 11 bytes is a loop of 11
fn the_pixels_turn_grey_in_place_and_keep_their_alpha_and_the_bytes_past_them() {
    let before: [u8; 11] = kani::any();
    let mut after = before;
    gray_pixels(&mut after);
    for pixel in 0..2 {
        let at = pixel * 4;
        let (r, g, b, a) = (before[at], before[at + 1], before[at + 2], before[at + 3]);
        // The luma of the three, by the law above (no second divider beside the code's).
        let y = after[at];
        let (s, y1000) = (thousandths(r, g, b), 1000 * u32::from(y));
        assert!(y1000 <= s + 500 && s + 500 < y1000 + 1000);
        assert!(after[at + 1] == y && after[at + 2] == y);
        assert!(after[at + 3] == a);
        if r <= a && g <= a && b <= a {
            assert!(y <= a);
        }
    }
    assert!(after[8] == before[8] && after[9] == before[9] && after[10] == before[10]);
    kani::cover!(after != before, "a picture with a colour");
}
