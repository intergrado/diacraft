//! Kani proof harnesses for the calculator's number code: the conversions of `convert.rs`, and the arithmetic and comparisons of `ops.rs`.
//! Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! Every `f64` from `kani::any()` ranges over all 2^64 bit patterns, NaN and both infinities included, unless a harness assumes
//! otherwise. Kani models `+ - * /` and comparisons bit-precisely; `powf` is an over-approximation, so a harness that goes through it
//! asserts only what holds for any result. `Cargo.toml` switches Kani's NaN check off for this crate (the reason is there).
//!
//! **The fixes of F29 are proved here**: `wrap_turn` lands from 0 up to a turn (`fold_remainder`, for every
//! remainder), `MODULUS` is below its divisor in size and takes its sign (`fold_modulo`), and the symbol comparisons follow IEEE for an infinite
//! number. Kani over-approximates the float remainder `%` (what it gives is not bounded by the divisor: a harness of `wrap_turn` or `floor_modulo` as
//! a whole fails on a counterexample no real `%` gives), so the two folds are proved for every remainder a `%` can give, which is below the divisor in
//! size and of the sign of the number, and the remainder itself, the exact `fmod` of the standard library, is tested on the real code (`fn_math.rs`,
//! `tests/it/math.rs`).
//!
//! **Two traps for a harness that does arithmetic on symbolic floats.** (1) Do not compute the same `*`, `/` or `+` again in the harness
//! to compare with the code's result: the solver has to prove two floating-point circuits equal, which it cannot do for a multiplier or a
//! divider, and the proof times out. State the answer from the classes of the operands (NaN, infinity, zero) instead, as
//! `assert_arithmetic_result` does. (2) Keep a choice between kinds of operand out of the harness (one `kani::any()` that picks a pair): make
//! each kind its own call, as `for_each_pair` does.

use diacraft_formula::{BinaryOp, CURRENCIES, Currency};
use diacraft_units::{Quantity, Unit};

use crate::convert::{float_range, signed_int, signed_long, unsigned_int};
use crate::fn_math::{fold_modulo, fold_remainder};
use crate::ops::{Operand, add_sub, compare, div, finish, mul, pow};
use crate::rules::BAD_ARGUMENT_ERROR;
use crate::value::{CellError, Value};

fn q(x: f64) -> Quantity {
    Quantity::new(x, Unit::Unitless)
}

/// Whether `v` is the error a conversion gives for an argument that is not valid.
fn is_bad_argument(v: &Value) -> bool {
    matches!(v, Value::Error(e) if *e == BAD_ARGUMENT_ERROR)
}

// ---- convert.rs: the whole-number and single-precision conversions ----------------------------------------------------------

/// `vSignedLong`: fractions are cut toward zero and only -2147483648 to 2147483647 is valid, and nothing else is: the call is
/// refused for NaN, for an infinity, and for exactly the numbers whose whole part is outside the range. "Toward zero" is stated
/// without `trunc`, the code's own primitive: the result has `x`'s sign, is no further from zero than `x`, and is less than 1 from it.
#[kani::proof]
fn signed_long_cuts_toward_zero_and_refuses_exactly_the_numbers_out_of_range() {
    let x: f64 = kani::any();
    match signed_long(q(x)) {
        Ok(n) => {
            assert!((-2_147_483_648..=2_147_483_647).contains(&n));
            let whole = n as f64; // exact: a 32-bit number
            if x >= 0.0 {
                assert!(n >= 0 && whole <= x && x < whole + 1.0);
            } else {
                assert!(n <= 0 && whole >= x && x > whole - 1.0);
            }
            kani::cover!(x >= 1.0 && x != whole, "a positive fraction is cut");
            kani::cover!(x <= -1.0 && x != whole, "a negative fraction is cut");
            kani::cover!(n == 2_147_483_647, "the largest number");
            kani::cover!(n == -2_147_483_648, "the smallest number");
        }
        Err(e) => {
            assert!(is_bad_argument(&e));
            // The whole part is out of range exactly when `x` is at least 2147483648 or at most -2147483649; NaN fails both.
            assert!(!(x > -2_147_483_649.0 && x < 2_147_483_648.0));
            kani::cover!(x.is_nan(), "NaN is refused");
            kani::cover!(x.is_infinite(), "an infinity is refused");
            kani::cover!(x == 2_147_483_648.0, "the first number past the top is refused");
        }
    }
}

/// `vSignedInt`: the same, limited to -32768 to 32767.
#[kani::proof]
fn signed_int_cuts_toward_zero_and_refuses_exactly_the_numbers_out_of_range() {
    let x: f64 = kani::any();
    match signed_int(q(x)) {
        Ok(n) => {
            assert!((-32_768..=32_767).contains(&n));
            let whole = n as f64;
            if x >= 0.0 {
                assert!(n >= 0 && whole <= x && x < whole + 1.0);
            } else {
                assert!(n <= 0 && whole >= x && x > whole - 1.0);
            }
            kani::cover!(x >= 1.0 && x != whole, "a positive fraction is cut");
            kani::cover!(n == -32_768, "the smallest number");
        }
        Err(e) => {
            assert!(is_bad_argument(&e));
            assert!(!(x > -32_769.0 && x < 32_768.0));
            kani::cover!(x == 32_768.0, "the first number past the top is refused");
            kani::cover!(x == 2_147_483_647.0, "a number `vSignedLong` takes is refused");
        }
    }
}

/// `vUnsignedInt`: refused exactly when `vSignedLong` refuses, and otherwise the whole number wraps modulo 65536 (a negative
/// one as sixteen bits of its two's complement).
#[kani::proof]
fn unsigned_int_is_refused_with_signed_long_and_otherwise_wraps_modulo_65536() {
    let x: f64 = kani::any();
    let (unsigned, signed) = (unsigned_int(q(x)), signed_long(q(x)));
    assert!(unsigned.is_err() == signed.is_err());
    match (unsigned, signed) {
        (Ok(u), Ok(n)) => {
            assert!(i64::from(u) == n.rem_euclid(65_536));
            kani::cover!(n < 0, "a negative number wraps");
            kani::cover!(n > 65_535, "a number past sixteen bits wraps");
        }
        (Err(e), Err(_)) => {
            assert!(is_bad_argument(&e));
            kani::cover!(x.is_nan(), "NaN is refused");
        }
        _ => {
            assert!(false, "refused by one and not the other");
        }
    }
}

/// `vFloat`: refused exactly for NaN and for a number larger in size than the largest single-precision one, and a number that passes
/// comes back with its own bits (a range check, which does not round). The numbers between `f32::MAX` and what rounds to it pass
/// the cast `as f32` and are still refused: the check is not that cast.
#[kani::proof]
fn float_range_refuses_exactly_nan_and_what_is_beyond_single_precision() {
    let x: f64 = kani::any();
    let largest = f64::from(f32::MAX);
    match float_range(q(x)) {
        Ok(v) => {
            assert!(v.to_bits() == x.to_bits());
            assert!(x >= -largest && x <= largest);
            kani::cover!(x == largest, "the largest single-precision number passes");
            kani::cover!(x < -1.0 && x > -1e38, "a large negative number passes");
            kani::cover!(x == 0.0, "zero passes");
        }
        Err(e) => {
            assert!(is_bad_argument(&e));
            assert!(x.is_nan() || x < -largest || x > largest);
            kani::cover!(x.is_nan(), "NaN is refused");
            kani::cover!(x.is_infinite(), "an infinity is refused");
            kani::cover!(x > largest && (x as f32) == f32::MAX, "a number that rounds to the largest one is refused");
        }
    }
}

// ---- ops.rs: arithmetic -----------------------------------------------------------------------------------------------------

/// Any unit, the date and the percent among them.
fn any_unit() -> Unit {
    let i: usize = kani::any();
    kani::assume(i < Unit::ALL.len());
    Unit::ALL[i]
}

/// Any number: all `f64`, any unit, any dimension (a dimension of 0 drops the unit, as `Quantity::with_dim` does).
fn any_quantity() -> Quantity {
    Quantity::with_dim(kani::any(), any_unit(), kani::any())
}

/// A number: any `f64`, any unit and any dimension.
fn number() -> Operand {
    Operand::Num(any_quantity())
}

/// A currency amount with no currency (the currency matters only when two operands have one: `two_currencies_fit_...`).
fn amount() -> Operand {
    Operand::Cy { amount: kani::any(), currency: None }
}

/// Runs `check` on a pair of two numbers, a currency amount and a number, and a number and a currency amount, each a fresh pair of any
/// operands. Two amounts are left out: `currency_rule` compares their currencies (`Currency`'s `==`, down to the bytes of the code), a loop
/// that Kani cannot bound while the amounts are symbolic floats, so a proof over them never ends. `two_currencies_fit_...` takes them.
/// The three kinds are three calls in one harness and not one pair picked by a symbolic choice: the choice makes the proof time out.
fn for_each_pair(mut check: impl FnMut(Operand, Operand)) {
    check(number(), number());
    check(amount(), number());
    check(number(), amount());
}

fn stored(o: Operand) -> f64 {
    match o {
        Operand::Num(n) => n.stored,
        Operand::Cy { amount, .. } => amount,
    }
}

fn is_currency(o: Operand) -> bool {
    matches!(o, Operand::Cy { .. })
}

fn dim(o: Operand) -> i32 {
    match o {
        Operand::Num(n) => i32::from(n.dim()),
        Operand::Cy { .. } => 0,
    }
}

/// Which kinds of result an arithmetic harness has seen over its pairs, for the `kani::cover!` at its end. A cover inside the checks would be
/// one per pair, and a pair of two numbers cannot give a currency amount: it would be unreachable there and say nothing about the proof.
#[derive(Default)]
struct Seen {
    finite: bool,
    infinity: bool,
    amount: bool,
    num_error: bool,
    dim_error: bool,
}

impl Seen {
    fn cover(&self) {
        kani::cover!(self.finite, "a finite number");
        kani::cover!(self.infinity, "a number too large to hold is an infinity, not an error");
        kani::cover!(self.amount, "a currency amount");
        kani::cover!(self.num_error, "#NUM!");
    }
}

/// What every `+ - * /` result must be, given `nan`, whether the operation on these stored numbers is NaN, and the operands. A number or an
/// amount is never NaN, and exists only where the operation is not; a NaN is `#NUM!`; two currencies that do not fit are `#VALUE!`; a
/// dimension out of range is `#DIM!`, which only two numbers can give and only when `dim_out_of_range` says it is.
///
/// `nan` is worked out from the classes of the operands (`inf - inf`, `0 * inf`, `inf / inf`), and the harnesses never multiply, divide or
/// add the stored numbers again to compare with the code's result: a SAT solver must then show two floating-point circuits equal, which is
/// the one thing it cannot do for a multiplier or a divider, and the proofs time out.
fn assert_arithmetic_result(v: &Value, nan: bool, a: Operand, b: Operand, dim_out_of_range: bool, seen: &mut Seen) {
    match v {
        Value::Number(n) => {
            assert!(!is_currency(a) && !is_currency(b));
            assert!(!nan && !n.stored.is_nan());
            seen.finite |= n.stored.is_finite();
            seen.infinity |= n.stored.is_infinite();
        }
        Value::Currency { amount, .. } => {
            assert!(is_currency(a) || is_currency(b));
            assert!(!nan && !amount.is_nan());
            seen.amount = true;
        }
        Value::Error(CellError::Num) => {
            assert!(nan);
            seen.num_error = true;
        }
        Value::Error(CellError::Value) => {
            assert!(is_currency(a) && is_currency(b));
        }
        Value::Error(CellError::Dim) => {
            assert!(!is_currency(a) && !is_currency(b) && !nan && dim_out_of_range);
            seen.dim_error = true;
        }
        _ => {
            assert!(false, "a result no arithmetic gives");
        }
    }
}

/// `finish`: NaN is `#NUM!`, a dimension outside -128 to 127 is `#DIM!`, and anything else is the number with its own bits and that
/// dimension, in a plain number when the dimension is 0 and in `DL` for an acre or a hectare at a dimension other than 2.
#[kani::proof]
fn finish_refuses_nan_and_a_dimension_out_of_range_and_changes_nothing_else() {
    let (number, unit, dim): (f64, Unit, i32) = (kani::any(), any_unit(), kani::any());
    let v = finish(number, unit, dim);
    if number.is_nan() {
        assert!(matches!(v, Value::Error(CellError::Num)));
    } else if !(-128..=127).contains(&dim) {
        assert!(matches!(v, Value::Error(CellError::Dim)));
        kani::cover!(dim == 128, "#DIM! just past the top");
    } else {
        match v {
            Value::Number(n) => {
                assert!(n.stored.to_bits() == number.to_bits());
                assert!(i32::from(n.dim()) == dim);
                if dim == 0 {
                    assert!(n.unit == Unit::Unitless);
                } else if matches!(unit, Unit::Acre | Unit::Hectare) && dim != 2 {
                    assert!(n.unit == Unit::DrawingDefault);
                } else {
                    assert!(n.unit == unit);
                }
                kani::cover!(dim == 127 && number.is_infinite(), "an infinity is a number, at the largest dimension");
                kani::cover!(dim == -128, "the smallest dimension");
                kani::cover!(dim == 0, "a plain number");
                kani::cover!(unit == Unit::Acre && dim == 1, "an acre at dimension 1 is in DL");
            }
            _ => {
                assert!(false, "neither a number nor an error");
            }
        }
    }
}

/// `+` and `-`: no number or amount is NaN, however infinite the operands, and the dimension of a result is one of
/// the operands' or 1, so it is never `#DIM!`.
#[kani::proof]
fn add_sub_never_gives_nan_and_never_runs_out_of_dimensions() {
    let mut seen = Seen::default();
    for_each_pair(|a, b| {
        let subtract: bool = kani::any();
        let (x, y) = (stored(a), stored(b));
        // `inf + -inf` and `inf - inf`: the infinities of the same sign cancel in a difference, of opposite signs in a sum.
        let cancels = x.is_infinite() && y.is_infinite() && (x.is_sign_positive() == y.is_sign_positive()) == subtract;
        assert_arithmetic_result(&add_sub(a, b, subtract), x.is_nan() || y.is_nan() || cancels, a, b, false, &mut seen);
    });
    seen.cover();
}

/// `*`: no NaN, and `#DIM!` only when the dimensions' sum leaves -128 to 127.
#[kani::proof]
fn mul_never_gives_nan_and_runs_out_of_dimensions_only_past_the_sum() {
    let mut seen = Seen::default();
    for_each_pair(|a, b| {
        let (x, y) = (stored(a), stored(b));
        let nan = x.is_nan() || y.is_nan() || (x.is_infinite() && y == 0.0) || (x == 0.0 && y.is_infinite());
        let sum = dim(a) + dim(b);
        assert_arithmetic_result(&mul(a, b), nan, a, b, !(-128..=127).contains(&sum), &mut seen);
    });
    seen.cover();
    kani::cover!(seen.dim_error, "#DIM!");
}

/// `/`: a divisor that is exactly zero is `#DIV/0!` before anything else, in any unit and for a currency, and no other
/// divisor is; no result is NaN; and `#DIM!` only when the dimensions' difference, or the divisor's dimension negated, leaves -128 to 127.
#[kani::proof]
fn div_refuses_exactly_a_zero_divisor_and_never_gives_nan() {
    let (mut seen, mut zero, mut negative_zero) = (Seen::default(), false, false);
    for_each_pair(|a, b| {
        let call: bool = kani::any();
        let v = div(a, b, call);
        let (x, y) = (stored(a), stored(b));
        if y == 0.0 {
            assert!(matches!(v, Value::Error(CellError::DivZero)));
            zero = true;
            negative_zero |= y.is_sign_negative();
        } else {
            let nan = x.is_nan() || y.is_nan() || (x.is_infinite() && y.is_infinite());
            let out_of_range = !(-128..=127).contains(&(dim(a) - dim(b))) || dim(b) == -128;
            assert_arithmetic_result(&v, nan, a, b, out_of_range, &mut seen);
        }
    });
    seen.cover();
    kani::cover!(seen.dim_error, "#DIM!");
    kani::cover!(zero, "a zero divisor");
    kani::cover!(negative_zero, "a negative zero divides by nothing too");
}

/// A currency from the head of the table, by number: none (id -1, the undefined currency), `SYS` (0), `XXX` (1) and `EUR` (10). Picked by
/// a `match` so that each is a constant: a pointer into the table chosen by a symbolic index makes the comparison of two currencies a loop.
fn nth_currency(i: u8) -> (Option<&'static Currency>, i32) {
    match i {
        0 => (None, -1),
        1 => (CURRENCIES.first(), 0),
        2 => (CURRENCIES.get(1), 1),
        _ => (CURRENCIES.get(2), 10),
    }
}

/// Two currencies (the rows, which repeat): they fit when they are the same or one is `XXX` (id 1, which
/// shows no symbol), and the result has the other's currency; any other two are `#VALUE!`. The amounts are constants (3 and 5): what is
/// proved is which currency comes out, and the amounts' floating point is the other harnesses'. The `unwind` bound is for the loop that
/// compares the two three-letter codes.
#[kani::proof]
#[kani::unwind(5)]
fn two_currencies_fit_when_they_are_equal_or_one_is_xxx_and_are_a_value_error_otherwise() {
    let (i, j): (u8, u8) = (kani::any(), kani::any());
    kani::assume(i < 4 && j < 4);
    let ((ca, ia), (cb, ib)) = (nth_currency(i), nth_currency(j));
    let (a, b) = (Operand::Cy { amount: 3.0, currency: ca }, Operand::Cy { amount: 5.0, currency: cb });
    let fit = ia == ib || ia == 1 || ib == 1;
    let wanted = if ib == 1 {
        ia
    } else if ia == 1 {
        ib
    } else {
        ia
    };
    let which: u8 = kani::any();
    let (v, amount_wanted) = match which % 4 {
        0 => (add_sub(a, b, false), 8.0),
        1 => (add_sub(a, b, true), -2.0),
        2 => (mul(a, b), 15.0),
        _ => (div(a, b, false), 0.6),
    };
    match v {
        Value::Currency { amount, currency } => {
            assert!(fit && amount == amount_wanted && currency.map_or(-1, |c| c.id) == wanted);
            kani::cover!(ia != ib, "two different currencies that fit");
            kani::cover!(ia == ib, "two of the same currency");
        }
        Value::Error(CellError::Value) => {
            assert!(!fit);
            kani::cover!(true, "two currencies that do not fit");
        }
        _ => {
            assert!(false, "a result two currencies do not give");
        }
    }
}

/// `POW` and `^`, through `powf`, which Kani over-approximates, so only what holds for any result is asserted: the zero base's
/// three rows, `#DIM!` for a negative base and a fractional exponent, and no NaN in a number. A number's dimension is the base's times the
/// exponent when that is a whole number from -128 to 127 (and 1 for a percent), and `#DIM!` is what the other cases give.
#[kani::proof]
fn pow_zero_base_negative_base_and_dimension_follow_the_steps() {
    let (base, exponent) = (any_quantity(), any_quantity());
    let (b, e) = (base.stored, exponent.stored);
    let v = pow(base, exponent);
    if b == 0.0 && e == 0.0 {
        assert!(matches!(v, Value::Error(CellError::Num)));
    } else if b == 0.0 && e < 0.0 {
        assert!(matches!(v, Value::Error(CellError::DivZero)));
    } else if b == 0.0 && e > 0.0 {
        assert!(matches!(v, Value::Number(n) if n.stored == 0.0 && n.unit == Unit::Unitless));
        kani::cover!(true, "zero to a positive power is 0");
    } else if b == 0.0 {
        // Only a NaN exponent is left, which no cell holds: it gives 0 (`e < 0.0` and `e == 0.0` are both false).
        assert!(e.is_nan() && matches!(v, Value::Number(n) if n.stored == 0.0));
    } else if b < 0.0 && e.is_finite() && e.floor() != e {
        assert!(matches!(v, Value::Error(CellError::Dim)));
        kani::cover!(true, "a negative base and a fractional exponent");
    } else {
        match v {
            Value::Number(n) => {
                assert!(!n.stored.is_nan());
                if base.unit == Unit::Percent {
                    assert!(n.unit == Unit::Percent && n.dim() == 1);
                } else {
                    // The dimension is the base's times the exponent, which is a whole number in range.
                    assert!(f64::from(n.dim()) == f64::from(base.dim()) * e);
                }
                kani::cover!(base.unit == Unit::Percent, "a percent");
                kani::cover!(base.unit != Unit::Percent && n.dim() != 0, "a number with a dimension");
                kani::cover!(base.unit != Unit::Percent && n.dim() == 0, "a plain number");
            }
            Value::Error(CellError::Num) => {
                kani::cover!(true, "#NUM!");
            }
            Value::Error(CellError::Dim) => {
                // A negative base gives #DIM! here only with an exponent that is infinite or no number: a finite fractional one is the arm
                // above, and a negative base with a whole exponent is a number. `b < 0.0` alone here would let a POW that refused every
                // negative base pass.
                let negative_base_and_no_whole_exponent = b < 0.0 && !e.is_finite();
                let dim = f64::from(base.dim()) * e;
                assert!(base.unit != Unit::Percent || negative_base_and_no_whole_exponent);
                assert!(negative_base_and_no_whole_exponent || dim.is_nan() || dim.floor() != dim || dim < -128.0 || dim > 127.0);
                kani::cover!(true, "#DIM!");
            }
            _ => {
                assert!(false, "a result POW does not give");
            }
        }
    }
}

// ---- ops.rs: comparisons ----------------------------------------------------------------------------------------------------

/// Two finite numbers.
fn two_finite() -> (f64, f64) {
    let (a, b): (f64, f64) = (kani::any(), kani::any());
    kani::assume(a.is_finite() && b.is_finite());
    (a, b)
}

/// The laws of `compare` that follow from the symbol comparisons' tolerance, each for finite
/// numbers. An infinite number has its own harness (`compare_follows_ieee_when_a_number_is_infinite`): the tolerance's gap `a - b` is NaN for
/// two infinities of one sign, and those are equal.
///
/// Equality is reflexive, and `<>` is the opposite of `=`.
#[kani::proof]
fn compare_eq_is_reflexive_and_ne_is_not_eq() {
    let (a, b) = two_finite();
    assert!(compare(BinaryOp::Eq, a, a));
    assert!(compare(BinaryOp::Ne, a, b) == !compare(BinaryOp::Eq, a, b));
    kani::cover!(compare(BinaryOp::Eq, a, b) && a != b, "equal within the tolerance and not exactly");
    kani::cover!(compare(BinaryOp::Ne, a, b), "different");
}

/// `<` is `>` the other way round, and `<=` is the opposite of `>`.
#[kani::proof]
fn compare_lt_is_gt_the_other_way_round_and_le_is_not_gt() {
    let (a, b) = two_finite();
    assert!(compare(BinaryOp::Lt, a, b) == compare(BinaryOp::Gt, b, a));
    assert!(compare(BinaryOp::Le, a, b) == !compare(BinaryOp::Gt, a, b));
    kani::cover!(compare(BinaryOp::Lt, a, b), "less");
    kani::cover!(compare(BinaryOp::Le, a, b) && a > b, "at most, and exactly more");
}

/// `>=` is the opposite of `<`: the tolerance is the same on both sides.
#[kani::proof]
fn compare_ge_is_not_lt() {
    let (a, b) = two_finite();
    assert!(compare(BinaryOp::Ge, a, b) == !compare(BinaryOp::Lt, a, b));
    kani::cover!(compare(BinaryOp::Ge, a, b) && a < b, "at least, and exactly less");
}

/// Exactly one of `<`, `=` and `>` holds.
#[kani::proof]
fn compare_exactly_one_of_lt_eq_gt_holds() {
    let (a, b) = two_finite();
    let held = [BinaryOp::Lt, BinaryOp::Eq, BinaryOp::Gt].map(|op| u8::from(compare(op, a, b)));
    assert!(held.iter().sum::<u8>() == 1);
    kani::cover!(held[0] == 1, "less");
    kani::cover!(held[1] == 1, "equal");
    kani::cover!(held[2] == 1, "greater");
}

/// The tolerance only widens equality: what is `<` or `>` within it is so exactly, and what is equal exactly is equal within it.
#[kani::proof]
fn compare_tolerance_only_widens_equality() {
    let (a, b) = two_finite();
    assert!(!compare(BinaryOp::Lt, a, b) || a < b);
    assert!(!compare(BinaryOp::Gt, a, b) || a > b);
    assert!(a != b || compare(BinaryOp::Eq, a, b));
    kani::cover!(a == b, "equal");
}

/// The exact comparisons are IEEE's, for every number: NaN and the infinities included.
#[kani::proof]
fn exact_comparisons_are_ieee() {
    let (a, b): (f64, f64) = (kani::any(), kani::any());
    assert!(compare(BinaryOp::ExactEq, a, b) == (a == b));
    assert!(compare(BinaryOp::ExactNe, a, b) == (a != b));
    assert!(compare(BinaryOp::ExactLt, a, b) == (a < b));
    assert!(compare(BinaryOp::ExactLe, a, b) == (a <= b));
    assert!(compare(BinaryOp::ExactGt, a, b) == (a > b));
    assert!(compare(BinaryOp::ExactGe, a, b) == (a >= b));
    kani::cover!(a.is_nan() && !compare(BinaryOp::ExactEq, a, a), "NaN is not equal to itself");
}

/// The symbol comparisons follow IEEE when a number is infinite: two infinities of one sign are equal (`=`, `>=` and `<=` hold, `<>`, `>` and `<`
/// do not), and an infinity against anything else is the plain order. Before the fix `(1E308*10) = (1E308*10)` was FALSE, and so were `<>`, `>=` and
/// `<=`, because `inf - inf` is NaN. A NaN, which no cell holds, makes every symbol comparison false (`<>` too), as it did.
#[kani::proof]
fn compare_follows_ieee_when_a_number_is_infinite() {
    let (a, b): (f64, f64) = (kani::any(), kani::any());
    kani::assume(a.is_infinite() || b.is_infinite());
    let ieee = [a == b, a != b, a >= b, a > b, a <= b, a < b];
    let got = [BinaryOp::Eq, BinaryOp::Ne, BinaryOp::Ge, BinaryOp::Gt, BinaryOp::Le, BinaryOp::Lt].map(|op| compare(op, a, b));
    if a.is_nan() || b.is_nan() {
        assert!(got == [false; 6]);
        kani::cover!(a.is_nan() && b.is_infinite(), "NaN against an infinity");
    } else {
        assert!(got == ieee);
        kani::cover!(a == b && a.is_infinite(), "two infinities of one sign");
        kani::cover!(a.is_infinite() && b.is_infinite() && a != b, "the two infinities");
        kani::cover!(a.is_infinite() && b.is_finite(), "an infinity against a number");
    }
}

// ---- fn_math.rs: the turn of an angle and MODULUS ----------------------------------------------------------------------------

/// `fold_remainder`, the last step of `wrap_turn`: for the remainder `r` of an angle by a turn, which is below a turn in size and of the sign of
/// the angle, the result is from 0 up to but not including the turn, a positive remainder is itself, and a negative one is a turn short of it or, when
/// that rounds to a whole turn, 0. The turn is every number `fold_remainder` can be given from 1E-300 up to 1E300 (the real one is `TAU`).
#[kani::proof]
fn fold_remainder_lands_from_zero_up_to_a_turn() {
    let (r, turn): (f64, f64) = (kani::any(), kani::any());
    kani::assume(turn.is_finite() && turn > 1e-300 && turn < 1e300);
    kani::assume(r.abs() < turn);
    let folded = fold_remainder(r, turn);
    assert!(folded >= 0.0 && folded < turn);
    assert!(folded.is_sign_positive());
    if r > 0.0 {
        assert!(folded == r);
    }
    let (negative, short_of_a_turn) = (r < 0.0, folded > 0.0);
    kani::cover!(r > 0.0, "a positive remainder");
    kani::cover!(negative & short_of_a_turn, "a negative remainder a turn short of it");
    kani::cover!(negative & !short_of_a_turn, "a negative remainder so small that the sum rounds to a turn");
    kani::cover!(r == 0.0, "a zero remainder");
}

/// `fold_modulo`, the last step of `floor_modulo`: for the remainder `r` of `n % d`, which is below `d` in size and of the sign of `n`, the result is
/// below `d` in size, has the sign of `d` (or is zero), is `r` when the signs agree, and is a divisor from `r` otherwise or, when that rounds to the
/// divisor, the largest number below it in size. `MODULUS(-1E-20, 1)` was 1, `MODULUS(-2, 3E38)` was 3E38.
#[kani::proof]
fn fold_modulo_is_below_the_divisor_and_takes_its_sign() {
    let (r, d): (f64, f64) = (kani::any(), kani::any());
    kani::assume(d.is_finite() && d != 0.0 && r.abs() < d.abs());
    let folded = fold_modulo(r, d);
    assert!(folded.abs() < d.abs());
    assert!(folded == 0.0 || (folded < 0.0) == (d < 0.0));
    if r != 0.0 && (r < 0.0) == (d < 0.0) {
        assert!(folded == r);
    }
    kani::cover!(r < 0.0 && d > 0.0 && folded > 0.0 && folded < d, "a negative remainder under a positive divisor");
    kani::cover!(r > 0.0 && d < 0.0 && folded < 0.0, "a positive remainder under a negative divisor");
    kani::cover!(r < 0.0 && d > 0.0 && folded == d.next_down(), "a remainder so small that the sum rounds to the divisor");
    kani::cover!(r != 0.0 && (r < 0.0) == (d < 0.0) && folded == r, "a remainder that already has the divisor's sign");
}

// --- The order of a page's graph (`index.rs`) ---

/// A reader of any cell: the ordering of the graph reads only its place, not the cell.
fn any_reader() -> crate::affected::Reader {
    (diacraft_doc::SheetRef::Document, diacraft_doc::CellAddr::direct("PinX"))
}

/// `walk_ord` and `connect_ord`: for **every** pair of numbers, the place of a formula in the walk of the page is its number in the low half and phase 0, the
/// place of an end of a `Connect` row is its number in the low half and phase 1 (`Entry::phase` says so), and so **every `Connect` row's entry sorts after every formula's**,
/// whatever the two numbers. Each place is one number for one input, so no two formulas, and no two ends, share one.
#[kani::proof]
fn a_connect_rows_place_is_after_every_formulas_and_keeps_its_number() {
    use crate::index::{Entry, connect_ord, walk_ord};
    let (a, b): (u32, u32) = (kani::any(), kani::any());
    let (w, c) = (walk_ord(a), connect_ord(b));
    assert!(w >> 32 == 0 && w as u32 == a);
    assert!(c >> 32 == 1 && c as u32 == b);
    assert!(w < c);
    assert!(Entry::new(w, any_reader()).phase() == 0);
    assert!(Entry::new(c, any_reader()).phase() == 1);
    let other: u32 = kani::any();
    assert!((walk_ord(other) == w) == (other == a) && (connect_ord(other) == c) == (other == b));
    kani::cover!((a == u32::MAX) & (b == 0), "the last formula of the walk before the first end");
}
