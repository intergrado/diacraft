//! `draw:transform` as the writer edits it (the transform rows): the terms of a list found
//! with where they lie in its text, so that a move changes the last translation and keeps every other term as written, and the list of a
//! shape that is turned or resized is written whole in LibreOffice's form, `rotate (<a>) translate (<x> <y>)`.
//!
//! **The grammar is the reader's** (`read::shapes::parse_transform`): terms separated by white space or commas, each a name of letters, a
//! parenthesis, arguments separated by white space or commas, and a parenthesis; the names `rotate`, `translate`, `scale`, `skewX`, `skewY`
//! and `matrix` with one, one or two, one or two, one, one and six arguments; angles and scales plain numbers, a translation's coordinates
//! lengths with a unit or plain numbers of hundredths of a millimetre, as LibreOffice reads them (`read::units::translation`),
//! and a move writes a plain number back as one; and at most 64 terms. A list the reader cannot read is read by it as no transform at all, and is not read here either ([`parse_terms`] gives
//! `None`), so a shape whose list is unreadable is edited by its `svg:x` and `svg:y`, as the reader reads it.

use crate::read::Affine;
use crate::read::units::{self, LenUnit};
use crate::write::lengths::length_text;

/// The most terms a list may hold and be read (the reader's `MOST_TERMS`).
const MOST_TERMS: usize = 64;

/// One term: its name, and each argument, as `(start, end)` in the list's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Term {
    pub name: String,
    pub args: Vec<(usize, usize)>,
    /// From the name's first letter to after the closing parenthesis.
    pub whole: (usize, usize),
}

/// The terms of `text`, or `None` when the reader reads no list of it.
pub(crate) fn parse_terms(text: &str) -> Option<Vec<Term>> {
    let mut terms = Vec::new();
    let mut at = 0usize;
    loop {
        let skip = text.get(at..)?.len() - text.get(at..)?.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',').len();
        at += skip;
        let rest = text.get(at..)?;
        if rest.is_empty() {
            break;
        }
        if terms.len() >= MOST_TERMS {
            return None;
        }
        let name_len = rest.find(|c: char| !c.is_ascii_alphabetic())?;
        let name = rest.get(..name_len)?;
        let after_name = rest.get(name_len..)?;
        let spaces = after_name.len() - after_name.trim_start().len();
        let open = after_name.get(spaces..)?.strip_prefix('(')?;
        let inside_from = at + name_len + spaces + 1;
        let close = open.find(')')?;
        let inside = open.get(..close)?;
        let mut args = Vec::new();
        let mut piece_from: Option<usize> = None;
        for (i, c) in inside.char_indices() {
            let separator = c.is_ascii_whitespace() || c == ',';
            match (separator, piece_from) {
                (false, None) => piece_from = Some(i),
                (true, Some(from)) => {
                    args.push((inside_from + from, inside_from + i));
                    piece_from = None;
                }
                _ => {}
            }
        }
        if let Some(from) = piece_from {
            args.push((inside_from + from, inside_from + inside.len()));
        }
        let whole_end = inside_from + close + 1;
        let term = Term { name: name.to_owned(), args, whole: (at, whole_end) };
        if !valid(text, &term) {
            return None;
        }
        at = whole_end;
        terms.push(term);
    }
    // The reader also refuses a list whose composed map is not finite (a product that overflows).
    finite(text, &terms).then_some(terms)
}

/// Whether the terms compose to a map of finite numbers, by the arithmetic the reader composes them with.
fn finite(text: &str, terms: &[Term]) -> bool {
    let mut map = Affine::IDENTITY;
    for term in terms {
        let num = |i: usize| term.args.get(i).and_then(|&(a, b)| text.get(a..b)).and_then(units::number_any).unwrap_or(0.0);
        let len = |i: usize| term.args.get(i).and_then(|&(a, b)| text.get(a..b)).and_then(units::translation).map_or(0.0, |(inches, _)| inches);
        let step = match (term.name.as_str(), term.args.len()) {
            ("rotate", _) => {
                let (s, c) = num(0).sin_cos();
                Affine { a: c, b: -s, c: s, d: c, e: 0.0, f: 0.0 }
            }
            ("translate", _) => Affine { e: len(0), f: len(1), ..Affine::IDENTITY },
            ("scale", 1) => Affine { a: num(0), d: num(0), ..Affine::IDENTITY },
            ("scale", _) => Affine { a: num(0), d: num(1), ..Affine::IDENTITY },
            ("matrix", _) => Affine { a: num(0), b: num(1), c: num(2), d: num(3), e: len(4), f: len(5) },
            _ => Affine::IDENTITY,
        };
        map = map.then(&step);
    }
    [map.a, map.b, map.c, map.d, map.e, map.f].iter().all(|n| n.is_finite())
}

/// Whether the reader reads the term: a known name with its number of arguments, each a number or a length as it asks.
fn valid(text: &str, term: &Term) -> bool {
    let arg = |i: usize| term.args.get(i).and_then(|&(a, b)| text.get(a..b));
    let number = |i: usize| arg(i).is_some_and(|a| units::number_any(a).is_some());
    // A translation's coordinate: a length, or a plain number in hundredths of a millimetre, as the reader reads one (`units::translation`).
    let length = |i: usize| arg(i).is_some_and(|a| units::translation(a).is_some());
    match (term.name.as_str(), term.args.len()) {
        ("rotate" | "skewX" | "skewY", 1) => number(0),
        ("translate", 1) => length(0),
        ("translate", 2) => length(0) && length(1),
        ("scale", 1) => number(0),
        ("scale", 2) => number(0) && number(1),
        ("matrix", 6) => (0..4).all(number) && length(4) && length(5),
        _ => false,
    }
}

/// The list with its last translation moved by (`dx`, `dy`) inches, each coordinate in the unit it is written in, and every other byte of
/// the text as it was (`skewX (0.2) rotate (0.3) translate (5cm 4cm)` moved 1 cm right is `... translate (6cm 4cm)`); where the last
/// term is no translation, one is added after it, in `page` (the unit of the page's `fo:page-width`). `None` when a coordinate cannot be
/// written, and for text that is not a list.
pub(crate) fn translated(text: &str, dx: f64, dy: f64, page: LenUnit) -> Option<String> {
    let terms = parse_terms(text)?;
    let Some(last) = terms.last() else {
        return added(text, dx, dy, page);
    };
    // A translation is the term `translate` (its x and y) or a `matrix` (its e and f).
    let (x_at, y_at) = match (last.name.as_str(), last.args.len()) {
        ("translate", 1 | 2) => (0, 1),
        ("matrix", 6) => (4, 5),
        _ => return added(text, dx, dy, page),
    };
    let arg = |i: usize| last.args.get(i).and_then(|&(a, b)| Some((a, b, units::translation(text.get(a..b)?)?)));
    let (xa, xb, (x, x_unit)) = arg(x_at)?;
    let new_x = coordinate_text(x + dx, x_unit)?;
    // `translate (tx)` has no ty: it is 0 in the unit of tx, and moving it adds one.
    match arg(y_at) {
        Some((ya, yb, (y, y_unit))) => {
            let new_y = coordinate_text(y + dy, y_unit)?;
            let mut out = String::with_capacity(text.len() + 8);
            out.push_str(text.get(..xa)?);
            out.push_str(&new_x);
            out.push_str(text.get(xb..ya)?);
            out.push_str(&new_y);
            out.push_str(text.get(yb..)?);
            Some(out)
        }
        None => {
            let new_y = coordinate_text(dy, x_unit)?;
            let mut out = String::with_capacity(text.len() + 8);
            out.push_str(text.get(..xa)?);
            out.push_str(&new_x);
            out.push(' ');
            out.push_str(&new_y);
            out.push_str(text.get(xb..)?);
            Some(out)
        }
    }
}

/// A translation's coordinate written in the form it was read in: a length in its unit, or, for a plain number (hundredths of a millimetre, `units::translation`), a plain
/// number again, so that a move keeps the file's spelling.
fn coordinate_text(inches: f64, unit: Option<LenUnit>) -> Option<String> {
    match unit {
        Some(unit) => length_text(inches, unit),
        // The number of hundredths of a millimetre is the length of `inches` hundred times over in millimetres; the suffix is left off.
        None => length_text(inches * 100.0, LenUnit::Mm)?.strip_suffix("mm").map(str::to_owned),
    }
}

/// `text` with ` translate (<dx> <dy>)` after it.
fn added(text: &str, dx: f64, dy: f64, page: LenUnit) -> Option<String> {
    let (x, y) = (length_text(dx, page)?, length_text(dy, page)?);
    let sep = if text.trim().is_empty() { "" } else { " " };
    Some(format!("{}{sep}translate ({x} {y})", text.trim_end()))
}

/// The list as LibreOffice writes one for a turned shape: `rotate (<angle>) translate (<x> <y>)`, the angle in radians
/// as the shortest decimal that is the number, the place in `unit`.
pub(crate) fn turned(angle: f64, x: f64, y: f64, unit: LenUnit) -> Option<String> {
    if !angle.is_finite() {
        return None;
    }
    Some(format!("rotate ({angle}) translate ({} {})", length_text(x, unit)?, length_text(y, unit)?))
}

/// The list of a turned shape that is flipped as well: `scale (1 -1) rotate (<angle>) translate (<x> <y>)`. The reader gives the frame the turn `angle` and the flip `FlipY`
/// (the map's determinant is -1, [`crate::read::frame_read`]): the shape is flipped in its own frame and then turned, as the model has it (`own::local_to_parent`).
pub(crate) fn turned_flipped(angle: f64, x: f64, y: f64, unit: LenUnit) -> Option<String> {
    if !angle.is_finite() {
        return None;
    }
    Some(format!("scale (1 -1) rotate ({angle}) translate ({} {})", length_text(x, unit)?, length_text(y, unit)?))
}

/// Whether the linear part of the list (the product of its terms') has a negative determinant: the list flips the shape, by a `scale` with one negative
/// factor or a `matrix` that mirrors, an even number of them undoing each other. The reader reads such a flip as `FlipY`, and a list written anew would lose it.
pub(crate) fn flips_the_frame(text: &str, terms: &[Term]) -> bool {
    let negative = |v: f64| v < 0.0;
    let mut flipped = false;
    for term in terms {
        let num = |i: usize| term.args.get(i).and_then(|&(a, b)| text.get(a..b)).and_then(units::number_any).unwrap_or(0.0);
        // One factor scales both axes alike, which never flips; a turn, a shear and a translation have determinant 1.
        flipped ^= match (term.name.as_str(), term.args.len()) {
            ("scale", 2) => negative(num(0) * num(1)),
            ("matrix", 6) => negative(num(0) * num(3) - num(1) * num(2)),
            _ => false,
        };
    }
    flipped
}

/// Whether a list of terms shears or scales a shape beyond what `rotate` and `translate` say (`skewX`, `skewY`, `scale`, `matrix`): what a
/// regeneration of the list drops, with one note.
pub(crate) fn has_more_than_a_turn(terms: &[Term]) -> bool {
    terms.iter().any(|t| !matches!(t.name.as_str(), "rotate" | "translate"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_found_where_they_lie() {
        let text = " rotate (0.5) ,translate(1cm 2cm)  scale( 2 )";
        let terms = parse_terms(text).unwrap();
        let shown: Vec<(&str, Vec<&str>, &str)> =
            terms.iter().map(|t| (t.name.as_str(), t.args.iter().map(|&(a, b)| &text[a..b]).collect(), &text[t.whole.0..t.whole.1])).collect();
        assert_eq!(
            shown,
            [("rotate", vec!["0.5"], "rotate (0.5)"), ("translate", vec!["1cm", "2cm"], "translate(1cm 2cm)"), ("scale", vec!["2"], "scale( 2 )")]
        );
        assert!(parse_terms("").unwrap().is_empty());
    }

    #[test]
    fn a_list_the_reader_cannot_read_is_none() {
        for bad in [
            "rotate",
            "rotate (",
            "rotate ()",
            "rotate (1 2)",
            "rotate (1cm)",
            "translate ()",
            "scale (1 2 3)",
            "skewX ()",
            "matrix (1 2 3 4 5cm)",
            "spin (1)",
            "rotate (nan)",
            "rotate (1) x",
        ] {
            assert!(parse_terms(bad).is_none(), "{bad}");
        }
        // A translation's coordinate with no unit is hundredths of a millimetre, as LibreOffice 26.8.2.2 reads it: a list the reader reads.
        for plain in ["translate (1)", "translate (1cm 2)", "matrix (1 2 3 4 5 6)", "matrix (0.5 -0.8 0.8 0.5 0 0)", "translate (-12.5 .5)"] {
            assert!(parse_terms(plain).is_some(), "{plain}");
        }
        for still_bad in ["translate (1e3)", "translate (+1)", "matrix (1 0 0 1 1e3 0)", "translate (1 cm)"] {
            assert!(parse_terms(still_bad).is_none(), "{still_bad}");
        }
        assert!(parse_terms(&"rotate (1) ".repeat(64)).is_some());
        assert!(parse_terms(&"rotate (1) ".repeat(65)).is_none());
    }

    #[test]
    fn each_term_is_read_with_the_arguments_the_reader_asks_of_it_and_a_map_that_overflows_is_no_list() {
        // Arguments: both of a two-argument scale, six of a matrix, each a number or a length as the reader asks.
        for good in ["scale (2 3)", "scale (2)", "matrix (1 0 0 1 5cm 3cm)", "skewY (0.1)", "translate (1cm)", "translate (1cm 2cm)"] {
            assert!(parse_terms(good).is_some(), "{good}");
        }
        for bad in [
            "scale (2 x)",
            "scale (x 2)",
            "scale (x)",
            "matrix (1 0 0 1 x 3cm)",
            "matrix (1 0 0 1 5cm y)",
            "matrix (x 0 0 1 5cm 3cm)",
            "matrix (1 0 0 x 5cm 3cm)",
            "matrix (1 0 0 1 x y)",
            "matrix (1 0 0 1 5cm)",
            "matrix (1 0 0 1 5cm 3cm 1)",
            "translate (1cm 2cm 3cm)",
            "skewY (1 2)",
            "skewY (x)",
            "translate (x)",
            "translate (1cm x)",
            "translate (x 1cm)",
        ] {
            assert!(parse_terms(bad).is_none(), "{bad}");
        }
        // The reader refuses a list whose composed map is not finite: a product that overflows, in any of the six places.
        for overflow in [
            "scale (1e200) scale (1e200)",
            "scale (1e200 1) scale (1e200 1)",
            "scale (1 1e200) scale (1 1e200)",
            "translate (10000000000in 0in) scale (1e300 1e300)",
            "translate (0in 10000000000in) scale (1e300 1e300)",
            "matrix (1e200 0 0 1 0cm 0cm) matrix (1e200 0 0 1 0cm 0cm)",
            "matrix (0 1e200 1e200 0 0cm 0cm) matrix (0 1e200 1e200 0 0cm 0cm)",
            "matrix (1 0 0 1e200 0cm 0cm) matrix (1 0 0 1e200 0cm 0cm)",
            "matrix (1 0 0 1 10000000000in 0in) matrix (1e300 0 0 1e300 0cm 0cm)",
            "matrix (1 0 0 1 0in 10000000000in) matrix (1e300 0 0 1e300 0cm 0cm)",
        ] {
            assert!(parse_terms(overflow).is_none(), "{overflow}");
        }
        for fine in ["scale (1e100) scale (1e100)", "rotate (1e300) scale (1e150 1e150)", "translate (1000in 1000in) translate (1000in 1000in)"] {
            assert!(parse_terms(fine).is_some(), "{fine}");
        }
    }

    #[test]
    fn the_last_translation_moves_and_nothing_else_does() {
        let cm = |n: f64| n / 2.54;
        // The shear and the turn stay as written.
        assert_eq!(
            translated("skewX (0.2) rotate (0.3) translate (5cm 4cm)", cm(1.0), 0.0, LenUnit::Cm).as_deref(),
            Some("skewX (0.2) rotate (0.3) translate (6cm 4cm)")
        );
        // Each coordinate in its own unit; a single coordinate gains its pair.
        assert_eq!(translated("translate (1in 25.4mm)", 1.0, 1.0, LenUnit::Cm).as_deref(), Some("translate (2in 50.8mm)"));
        assert_eq!(translated("translate (1cm)", cm(1.0), cm(2.0), LenUnit::Cm).as_deref(), Some("translate (2cm 2cm)"));
        // A matrix's e and f.
        assert_eq!(translated("matrix (1 0 0 1 1cm 2cm)", cm(1.0), cm(1.0), LenUnit::Cm).as_deref(), Some("matrix (1 0 0 1 2cm 3cm)"));
        // No translation last: one is added, in the page's unit; and the same for an empty list.
        assert_eq!(translated("rotate (0.5)", cm(1.0), 0.0, LenUnit::Cm).as_deref(), Some("rotate (0.5) translate (1cm 0cm)"));
        assert_eq!(translated("", cm(1.0), 0.0, LenUnit::Mm).as_deref(), Some("translate (10mm 0mm)"));
        assert_eq!(translated("rotate (", 1.0, 1.0, LenUnit::Cm), None);
        // A plain number (hundredths of a millimetre) moves and stays a plain number; a coordinate with a unit beside it keeps its unit.
        assert_eq!(translated("matrix (1 0 0 1 2000 3000)", cm(1.0), cm(-0.5), LenUnit::Cm).as_deref(), Some("matrix (1 0 0 1 3000 2500)"));
        assert_eq!(translated("matrix (1 0 0 1 2cm 3000)", cm(1.0), cm(1.0), LenUnit::Cm).as_deref(), Some("matrix (1 0 0 1 3cm 4000)"));
        assert_eq!(translated("translate (2000)", 0.0, cm(1.5), LenUnit::Cm).as_deref(), Some("translate (2000 1500)"));
        assert_eq!(translated("matrix (0.5 -0.8 0.8 0.5 0 0)", cm(0.25), 0.0, LenUnit::Cm).as_deref(), Some("matrix (0.5 -0.8 0.8 0.5 250 0)"));
    }

    /// The list of a flipped and turned frame is read as the turn written and as a flip, and its size and place are the frame's.
    #[test]
    fn a_flipped_and_turned_shape_is_read_as_the_turn_written_and_a_flip() {
        let (w, h) = (6.0 / 2.54, 3.0 / 2.54);
        for angle in [0.0_f64, 0.5235987755982988, -1.2, 3.0] {
            let (s, c) = angle.sin_cos();
            // The frame's centre taken through the flip and the turn, as the writer places it, and the place wanted for it.
            let (hx, hy) = (w / 2.0 * c - h / 2.0 * s, -w / 2.0 * s - h / 2.0 * c);
            let centre = (7.0 / 2.54, 9.0 / 2.54);
            let text = turned_flipped(angle, centre.0 - hx, centre.1 - hy, LenUnit::Cm).unwrap();
            assert!(text.starts_with("scale (1 -1) rotate ("), "{text}");
            let read = crate::read::frame_read(0.0, 0.0, w, h, Some(&text)).unwrap();
            assert!(read.flipped && !read.sheared, "{angle}: {read:?}");
            assert!((read.angle - angle).abs() < 1e-9, "{angle}: {read:?}");
            assert!((read.size.0 - w).abs() < 1e-9 && (read.size.1 - h).abs() < 1e-9, "{angle}: {read:?}");
            // The list gives its length in a unit the writer rounds: the centre is where it was put within a ten-thousandth of an inch.
            assert!((read.centre.0 - centre.0).abs() < 1e-4 && (read.centre.1 - centre.1).abs() < 1e-4, "{angle}: {read:?}");
        }
        assert_eq!(turned_flipped(f64::NAN, 1.0, 1.0, LenUnit::Cm), None);
    }

    #[test]
    fn a_turned_shape_is_written_in_libreoffices_form() {
        assert_eq!(
            turned(0.5235987755982988, 5.0 / 2.54, 10.0 / 2.54, LenUnit::Cm).as_deref(),
            Some("rotate (0.5235987755982988) translate (5cm 10cm)")
        );
        assert_eq!(turned(f64::NAN, 1.0, 1.0, LenUnit::Cm), None);
        assert!(has_more_than_a_turn(&parse_terms("skewX (1) translate (1cm 1cm)").unwrap()));
        assert!(!has_more_than_a_turn(&parse_terms("rotate (1) translate (1cm 1cm)").unwrap()));
    }

    #[test]
    fn a_list_flips_the_frame_when_its_determinant_is_negative() {
        let flips = |text: &str| flips_the_frame(text, &parse_terms(text).unwrap());
        for flipping in [
            "scale (1 -1)",
            "scale (-1 1) translate (1cm 1cm)",
            "matrix (1 0 0 -1 0cm 0cm)",
            "matrix (0 1 1 0 0cm 0cm)",
            "matrix (1 2 3 1 0cm 0cm)",
            "matrix (-2 1 1 1 0cm 0cm)",
            "matrix (4 1 3 0.5 0cm 0cm)",
            "rotate (0.3) scale (2 -2)",
            "scale (-1 1) scale (1 -1) scale (1 -1)",
            "scale (1 -1) matrix (1 0 0 1 2cm 2cm) scale (-1 1) scale (1 -1)",
        ] {
            assert!(flips(flipping), "{flipping}");
        }
        for turning in [
            "rotate (0.5) translate (1cm 2cm)",
            "scale (-1)",
            "scale (2 3)",
            "scale (-1 -1)",
            "scale (0 1)",
            "scale (1 0)",
            "scale (-1 0)",
            "skewX (0.2)",
            "matrix (1 0 0 1 3cm 2cm)",
            "matrix (0 1 -1 0 0cm 0cm)",
            "matrix (2 1 1 1 0cm 0cm)",
            "matrix (1 10 0.1 2 0cm 0cm)",
            "matrix (0.5 1 1 4 0cm 0cm)",
            "matrix (1 1 1 1 0cm 0cm)",
            "scale (1 -1) scale (1 -1)",
            "scale (-1 1) matrix (1 0 0 -1 0cm 0cm)",
            "",
        ] {
            assert!(!flips(turning), "{turning}");
        }
    }
}
