//! `PolylineTo` and the `POLYLINE` constructor.

use diacraft_geom::kurbo::ParamCurve;
use diacraft_geom::{GeomError, GeomRow, MAX_CONSTRUCTOR_POINTS, Polyline, WarningKind};

use crate::{EPS, assert_pt, build, mv, segments, vertices};

fn poly(x: f64, y: f64, args: &[f64]) -> GeomRow {
    GeomRow::PolylineTo { x, y, polyline: Some(Polyline::from_args(args).unwrap()) }
}

/// Given `Width` 4, `Height` 2, a pen at (0, 0), `X` 2, `Y` 2 and `A` `POLYLINE(0,0,0.25,0.75,0.6,0.1)`, expect the vertices (1, 1.5),
/// (2.4, 0.2), then (2, 2). Leaving out `X` and `Y` stops at (2.4, 0.2). Reading the pairs as inches gives (0.25, 0.75).
#[test]
fn poly_a_polyline_runs_through_its_pairs_to_the_end() {
    let built = build(&[mv(0.0, 0.0), poly(2.0, 2.0, &[0.0, 0.0, 0.25, 0.75, 0.6, 0.1])], 4.0, 2.0);
    let v = vertices(&built.stroke);
    assert_eq!(v.len(), 4, "{v:?}");
    assert_pt(v[0], 0.0, 0.0, EPS, "pen");
    assert_pt(v[1], 1.0, 1.5, EPS, "first pair");
    assert_pt(v[2], 2.4, 0.2, EPS, "second pair");
    assert_pt(v[3], 2.0, 2.0, EPS, "the end (X, Y)");
    assert!(built.warnings.is_empty());
}

/// Given types (1, 0) in `POLYLINE(1,0,0.25,0.75)` with the same size, expect (0.25, 1.5). Scaling both gives (1, 1.5).
#[test]
fn poly_a_type_other_than_zero_is_a_length() {
    let built = build(&[mv(0.0, 0.0), poly(2.0, 2.0, &[1.0, 0.0, 0.25, 0.75])], 4.0, 2.0);
    let v = vertices(&built.stroke);
    assert_pt(v[1], 0.25, 1.5, EPS, "x a length, y a share");
    // MS-VSDX says 0 and any other value; the Learn page's 0 and 1 are the two that files hold.
    let two = build(&[mv(0.0, 0.0), poly(2.0, 2.0, &[2.0, 7.0, 0.25, 0.75])], 4.0, 2.0);
    assert_pt(vertices(&two.stroke)[1], 0.25, 0.75, EPS, "types 2 and 7 are lengths");
    let share = Polyline::from_args(&[0.0, 1.0, 0.5, 0.5]).unwrap();
    assert!(share.x_is_share && !share.y_is_share);
}

/// Given `POLYLINE(0,0,0.25)`, expect `#VALUE!`. Reading the odd number as a pair with 0 passes silently.
#[test]
fn poly_a_wrong_count_of_arguments_is_an_error() {
    for args in [&[0.0, 0.0, 0.25][..], &[0.0][..], &[][..], &[0.0, 0.0, 1.0, 2.0, 3.0][..]] {
        assert!(matches!(Polyline::from_args(args), Err(GeomError::BadConstructor { name: "POLYLINE", .. })), "{args:?}");
    }
    assert!(Polyline::from_args(&[0.0, 0.0]).is_ok(), "no pairs is a polyline of the end alone");
    assert_eq!(Polyline::from_args(&[0.0, 0.0, 1.0, 2.0]).unwrap().points, vec![(1.0, 2.0)]);
    assert!(matches!(Polyline::from_text("POLYLINE(0,0,0.25)"), Err(GeomError::BadConstructor { .. })));
}

/// Given a `V` of `POLYLINE(0,0,0.5,0.5)`, a formula naming `User.Spot` in `F`, and `U` `POLYLINE`, expect the vertex from `V` at
/// load, and the formula kept verbatim. Parsing the formula's text for numbers finds none. This crate reads `V`; the formula is the document's.
#[test]
fn poly_the_constructor_is_read_from_the_cached_value_text() {
    let from_v = Polyline::from_text("POLYLINE(0,0,0.5,0.5)").unwrap();
    assert_eq!(from_v, Polyline { x_is_share: true, y_is_share: true, points: vec![(0.5, 0.5)] });
    // The formula text names a cell and holds no numbers to read.
    assert!(Polyline::from_text("POLYLINE(0,0,User.Spot,0.5)").is_err());
    // Case, spaces and signs and exponents as the number grammar has them (formula-language R16).
    let loose = Polyline::from_text("  polyline ( 0 , 1 , -.5 , 1.5E1 , +2. , 3e-1 )  ").unwrap();
    assert_eq!(loose.points, vec![(-0.5, 15.0), (2.0, 0.3)]);
    assert!(loose.x_is_share && !loose.y_is_share);
}

/// The text of a constructor that is no constructor is an error, not a guess.
#[test]
fn polyline_text_that_is_not_a_polyline_call_is_refused() {
    for text in [
        "",
        "POLYLINE",
        "POLYLINE(",
        "POLYLINE)",
        "POLYLINE()",
        "POLYLINE(0,0",
        "POLYLINE(0,0,1,2)x",
        "NURBS(0,0,1,2)",
        "POLYLINE(0,0,,1)",
        "POLYLINE(0,0,nan,1)",
        "POLYLINE(0,0,inf,1)",
        "POLYLINE(0,0,1.#INF,1)",
        "POLYLINE(0;0;1;2)",
        "XPOLYLINE(0,0,1,2)",
        "POLYLINE (0,0,1,2) (3)",
        "POLYLINE(0,0,1e999,2)",
    ] {
        assert!(Polyline::from_text(text).is_err(), "{text:?}");
    }
    assert!(Polyline::from_args(&[0.0, 0.0, f64::NAN, 1.0]).is_err());
    assert!(Polyline::from_args(&[0.0, 0.0, f64::INFINITY, 1.0]).is_err());
}

/// Caps: a constructor of more points than any file holds (490 pairs) is refused at the cap, and a huge text is refused before it is read.
#[test]
fn a_constructor_is_capped() {
    let mut args = vec![0.0, 0.0];
    args.extend(std::iter::repeat_n(0.5, MAX_CONSTRUCTOR_POINTS * 2));
    assert!(Polyline::from_args(&args).is_ok(), "exactly the cap");
    args.extend([0.5, 0.5]);
    assert!(Polyline::from_args(&args).is_err(), "one more");
    let huge = format!("POLYLINE(0,0{})", ",1".repeat(diacraft_geom::MAX_CONSTRUCTOR_TEXT));
    assert!(Polyline::from_text(&huge).is_err());
}

/// The last pair equals (`X`, `Y`) on most rows, so the final vertex is added either way, and a repeat is a segment of no length.
#[test]
fn poly_the_final_vertex_is_added_even_when_it_repeats() {
    let built = build(&[mv(0.0, 0.0), poly(2.0, 2.0, &[1.0, 1.0, 1.0, 1.0, 2.0, 2.0])], 4.0, 2.0);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 3, "(0,0)-(1,1)-(2,2)-(2,2)");
    assert_pt(segs[2].start(), 2.0, 2.0, EPS, "repeat start");
    assert_pt(segs[2].end(), 2.0, 2.0, EPS, "repeat end");
}

/// Our rule: a `PolylineTo` with no readable `A` is a straight line to (`X`, `Y`), with a warning.
#[test]
fn poly_an_unreadable_constructor_is_a_line_to_the_end() {
    let built = build(&[mv(0.0, 0.0), GeomRow::PolylineTo { x: 2.0, y: 2.0, polyline: None }], 4.0, 2.0);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    assert_pt(segs[0].end(), 2.0, 2.0, EPS, "end");
    assert_eq!(built.warnings.len(), 1);
    assert_eq!(built.warnings[0].kind, WarningKind::PolylineUnreadable);
    assert!(built.warnings[0].to_string().contains("the polyline has no readable points"));
}

/// A polyline row needs a pen; a row built by hand with more points than the cap is read as unreadable, not allocated.
#[test]
fn a_polyline_with_no_pen_is_skipped_and_one_over_the_cap_is_unreadable() {
    let none = build(&[poly(2.0, 2.0, &[0.0, 0.0, 1.0, 1.0])], 4.0, 2.0);
    assert_eq!(none.warnings[0].kind, WarningKind::NoPen);
    let big = Polyline { x_is_share: false, y_is_share: false, points: vec![(1.0, 1.0); MAX_CONSTRUCTOR_POINTS + 1] };
    let built = build(&[mv(0.0, 0.0), GeomRow::PolylineTo { x: 2.0, y: 2.0, polyline: Some(big) }], 4.0, 2.0);
    assert_eq!(built.warnings[0].kind, WarningKind::PolylineUnreadable);
    assert_eq!(segments(&built.stroke).len(), 1);
    let nan = Polyline { x_is_share: false, y_is_share: false, points: vec![(f64::NAN, 1.0)] };
    let skipped = build(&[mv(0.0, 0.0), GeomRow::PolylineTo { x: 2.0, y: 2.0, polyline: Some(nan) }], 4.0, 2.0);
    assert_eq!(skipped.warnings[0].kind, WarningKind::NonFinite);
}
