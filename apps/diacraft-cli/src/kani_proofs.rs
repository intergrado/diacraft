//! Kani proof harnesses for the number code of the command line: the centre of a page's extent that `roundtrip --via diacraft --edit add` drops a shape at
//! (`roundtrip::native::half_of`). The crate is a binary: `src/main.rs` declares this file, which `cargo xtask kani` reads as it reads a library's
//! `src/lib.rs`. Run with `cargo xtask kani` (`docs/development.md`, Proofs).

use crate::roundtrip::half_of;

/// `half_of`: for **every** `Option<f64>`, the centre is a finite number that is not below 0; it is 0 exactly for no figure, NaN, either infinity, either
/// zero and a negative figure, which are no extent; and for any other figure it is half of it to the last bit, so no larger than the figure. Two-sided: a figure above 0 is
/// halved, so a `half_of` that always gave 0 fails, and one that gave the figure itself fails too.
#[kani::proof]
fn the_centre_of_an_extent_is_half_of_it_and_the_corner_for_a_figure_that_is_no_extent() {
    let there: bool = kani::any();
    let v: f64 = kani::any();
    let extent = there.then_some(v);
    let centre = half_of(extent);
    assert!(centre.is_finite() && centre >= 0.0);
    // Not -0.0: the corner is the plus zero, and so is a half that rounds away.
    assert!(centre.is_sign_positive());
    let an_extent = there && v.is_finite() && v > 0.0;
    if an_extent {
        assert!(centre == v / 2.0 && centre <= v);
        // Half to the last bit: twice the centre is the figure wherever halving it loses no bit (every figure from twice the least normal number up).
        if v >= 2.0 * f64::MIN_POSITIVE {
            assert!(centre + centre == v);
        }
    } else {
        assert!(centre.to_bits() == 0);
    }
    kani::cover!(!there, "no figure");
    kani::cover!(there & v.is_nan(), "a figure that is no number");
    kani::cover!(there & (v == f64::INFINITY), "an extent past every number");
    kani::cover!(there & (v < 0.0), "a negative figure");
    kani::cover!(there & (v.to_bits() == (-0.0f64).to_bits()), "the minus zero");
    kani::cover!(an_extent & (centre == 0.0), "the least figure halves to 0");
    kani::cover!(an_extent & (v == f64::MAX), "the largest figure");
    kani::cover!(an_extent & (centre > 0.0), "a page's centre");
}
