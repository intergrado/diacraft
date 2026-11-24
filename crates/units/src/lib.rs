//! Units of measure of the ShapeSheet formula language, and values held in internal units.
//!
//! The spec is: R22 lists the suffixes, the `U` attribute values and the internal
//! units per unit, R24 percent, R25 the internal units (inches for length and type, radians for angles, days for durations),
//! R26 the areas, R29 the compound literals, R30 the ruler-only variants and the default units.
//!
//! A [`Quantity`] holds the number a cell stores (`V`) and the unit it is shown in (`U`). The unit never rescales the stored
//! number: arithmetic works on stored values, and the unit only decides how the number reads
//! when shown. A quantity also carries its dimension, the power of its unit (`3 IN^2` is dimension 2). What is not here,
//! on purpose: the range check of a power (`#DIM!`) and the date arithmetic, which the evaluator owns.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::f64::consts::PI;

#[cfg(kani)]
mod kani_proofs;

/// What a unit measures. A value may be shown only in a unit of its own measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Measure {
    /// A number with no unit.
    None,
    /// Inches inside: lengths, type sizes, and the implicit drawing, page and type units.
    Length,
    /// Radians inside.
    Angle,
    /// Days inside.
    Duration,
    /// Square inches inside: the acre and the hectare.
    Area,
    /// The fraction inside: 1 is 100 percent.
    Percent,
    /// A date and time, days since 30 December 1899.
    Date,
}

/// What one unit is: its `U` attribute value, its formula suffix, its measure, and how many internal units one of it is.
/// The internal units per unit are a product over a quotient, `mul / div`, so that `2.54 cm` and `25.4 mm` store exactly 1
/// (a single factor of 1 / 2.54 would store 1.0000000000000002).
struct Row {
    code: &'static str,
    suffix: Option<&'static str>,
    measure: Measure,
    mul: f64,
    div: f64,
}

/// The cm and mm are defined through 25.4 mm to the inch (the table: `1 / 25.4` and `1 / 2.54`).
const MM_PER_INCH: f64 = 25.4;
/// The didot of R22: 0.3759715104 mm.
const DIDOT_MM: f64 = 0.3759715104;

macro_rules! units {
    ($($(#[$doc:meta])* $variant:ident = ($code:expr, $suffix:expr, $measure:ident, $mul:expr, $div:expr)),* $(,)?) => {
        /// A unit: every suffix and `U` value of R22, the areas of R26, the compound units of R29 and the date, and the absence
        /// of a unit.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Unit {
            $($(#[$doc])* $variant),*
        }

        impl Unit {
            /// Every unit, once.
            pub const ALL: [Unit; [$(Unit::$variant),*].len()] = [$(Unit::$variant),*];

            fn row(self) -> Row {
                match self {
                    $(Unit::$variant => Row { code: $code, suffix: $suffix, measure: Measure::$measure, mul: $mul, div: $div }),*
                }
            }
        }
    };
}

units! {
    /// A number with no unit: a cell with no `U`, or a plain number.
    Unitless = ("", None, None, 1.0, 1.0),
    /// Inch, `IN`.
    Inch = ("IN", Some("IN"), Length, 1.0, 1.0),
    /// `IN_F`: an inch whose ruler is divided in eighths. Only the rulers see it; the formula still says `IN`.
    InchFine = ("IN_F", Some("IN"), Length, 1.0, 1.0),
    /// Foot, `FT`.
    Foot = ("FT", Some("FT"), Length, 12.0, 1.0),
    /// Yard, `YD`.
    Yard = ("YD", Some("YD"), Length, 36.0, 1.0),
    /// Mile, `MI`.
    Mile = ("MI", Some("MI"), Length, 63360.0, 1.0),
    /// `MI_F`: a mile with the ruler variant of R30.
    MileFine = ("MI_F", Some("MI"), Length, 63360.0, 1.0),
    /// Nautical mile, `NM`: 1852 m.
    NauticalMile = ("NM", Some("NM"), Length, 1_852_000.0, MM_PER_INCH),
    /// Millimetre, `MM`.
    Millimetre = ("MM", Some("MM"), Length, 1.0, MM_PER_INCH),
    /// Centimetre, `CM`.
    Centimetre = ("CM", Some("CM"), Length, 1.0, 2.54),
    /// Metre, `M`.
    Metre = ("M", Some("M"), Length, 1000.0, MM_PER_INCH),
    /// Kilometre, `KM`.
    Kilometre = ("KM", Some("KM"), Length, 1_000_000.0, MM_PER_INCH),
    /// The drawing default unit, `DL`: an implicit unit whose number is internal inches.
    DrawingDefault = ("DL", Some("DL"), Length, 1.0, 1.0),
    /// The page default unit, `DP`.
    PageDefault = ("DP", Some("DP"), Length, 1.0, 1.0),
    /// Point, `PT`: 1/72 inch.
    Point = ("PT", Some("PT"), Length, 1.0, 72.0),
    /// The type default unit, `DT`: an implicit unit whose number is internal inches, like `DL` (samples
    /// disagree with MS-VSDX's 1/72).
    TypeDefault = ("DT", Some("DT"), Length, 1.0, 1.0),
    /// Pica, `P`: 1/6 inch.
    Pica = ("P", Some("P"), Length, 1.0, 6.0),
    /// Didot, `D`.
    Didot = ("D", Some("D"), Length, DIDOT_MM, MM_PER_INCH),
    /// Cicero, `C`: 12 didots.
    Cicero = ("C", Some("C"), Length, 12.0 * DIDOT_MM, MM_PER_INCH),
    /// Degree, `DEG`.
    Degree = ("DEG", Some("DEG"), Angle, PI, 180.0),
    /// Radian, `RAD`.
    Radian = ("RAD", Some("RAD"), Angle, 1.0, 1.0),
    /// The angle default unit, `DA`.
    AngleDefault = ("DA", Some("DA"), Angle, 1.0, 1.0),
    /// Elapsed day, `ED`.
    ElapsedDay = ("ED", Some("ED"), Duration, 1.0, 1.0),
    /// Elapsed hour, `EH`.
    ElapsedHour = ("EH", Some("EH"), Duration, 1.0, 24.0),
    /// Elapsed minute, `EM`.
    ElapsedMinute = ("EM", Some("EM"), Duration, 1.0, 1440.0),
    /// Elapsed second, `ES`.
    ElapsedSecond = ("ES", Some("ES"), Duration, 1.0, 86400.0),
    /// Elapsed week, `EW`: 7 days.
    ElapsedWeek = ("EW", Some("EW"), Duration, 7.0, 1.0),
    /// The duration default unit, `DE`.
    DurationDefault = ("DE", Some("DE"), Duration, 1.0, 1.0),
    /// Percent: the formula suffix is `%` and `U` is `PER`. The stored value is the fraction.
    Percent = ("PER", Some("%"), Percent, 1.0, 100.0),
    /// Acre, `ACRE` in a formula and `AC` in `U`: 43,560 square feet.
    Acre = ("AC", Some("ACRE"), Area, 6_272_640.0, 1.0),
    /// Hectare, `HA`: 10,000 square metres, in square inches.
    Hectare = ("HA", Some("HA"), Area, 10_000.0 * 1_000.0 * 1_000.0, MM_PER_INCH * MM_PER_INCH),
    /// `F_I`: feet and inches, written `8FT 4IN`. Its number is in inches.
    FeetInches = ("F_I", None, Length, 1.0, 1.0),
    /// `AD`: degrees with minutes and seconds, written with a degree sign, a prime and a double prime. Its number is in degrees.
    DegreesMinutesSeconds = ("AD", None, Angle, PI, 180.0),
    /// `C_D`: ciceros and didots, written `2C4`. Its number is in didots.
    CiceroDidot = ("C_D", None, Length, DIDOT_MM, MM_PER_INCH),
    /// `P_PT`: picas and points, written `3P6`. Its number is in points.
    PicaPoints = ("P_PT", None, Length, 1.0, 72.0),
    /// A date and time, `DATE`: days since 30 December 1899.
    Date = ("DATE", None, Date, 1.0, 1.0),
}

/// Spellings that are neither a unit's `U` value nor its suffix: the longer names and the dotted short forms the other
/// requirements accept (acres and hectares; durations), and the display names of the compound units that R29 gives in one sentence: the
/// degree sign, `FEET/INCH`, `CICERO/DIDOT` and `PICAPOINTS`. A parser takes the union of the spellings the sources show
/// (How to read this file).
const EXTRA_SPELLINGS: &[(&str, Unit)] = &[
    ("ACRES", Unit::Acre),
    ("HA.", Unit::Hectare),
    ("HECTARE", Unit::Hectare),
    ("HECTARES", Unit::Hectare),
    ("\u{b0}", Unit::DegreesMinutesSeconds),
    ("FEET/INCH", Unit::FeetInches),
    ("CICERO/DIDOT", Unit::CiceroDidot),
    ("PICAPOINTS", Unit::PicaPoints),
    ("EWEEK", Unit::ElapsedWeek),
    ("EDAY", Unit::ElapsedDay),
    ("EHOUR", Unit::ElapsedHour),
    ("EMINUTE", Unit::ElapsedMinute),
    ("ESECOND", Unit::ElapsedSecond),
    ("EW.", Unit::ElapsedWeek),
    ("ED.", Unit::ElapsedDay),
    ("EH.", Unit::ElapsedHour),
    ("EM.", Unit::ElapsedMinute),
    ("ES.", Unit::ElapsedSecond),
];

/// The longest spelling is `CICERO/DIDOT` (12 bytes); the degree sign is two. Longer text is no unit, and the cap keeps
/// a hostile text from costing a scan.
const LONGEST_SPELLING: usize = 16;

impl Unit {
    /// The value of the `U` attribute that holds this unit, empty for [`Unit::Unitless`]. Percent is `PER`, the acre `AC`.
    pub fn code(self) -> &'static str {
        self.row().code
    }

    /// The suffix a formula writes after a number, or `None` for a unit with no single-number form: the compounds, which
    /// are written as two numbers, the date, and no unit. `IN_F` and `MI_F` write `IN` and `MI`.
    pub fn suffix(self) -> Option<&'static str> {
        self.row().suffix
    }

    /// What this unit measures.
    pub fn measure(self) -> Measure {
        self.row().measure
    }

    /// How many internal units (inches, radians, days, square inches, the fraction) one of this unit is: the last column of
    /// R22. The compounds count in their smallest unit (see [`Quantity::feet_inches`]).
    pub fn base_per_unit(self) -> f64 {
        let row = self.row();
        row.mul / row.div
    }

    /// A number written in this unit, as internal units.
    ///
    /// A number whose true value fits a double is finite here: the product with the numerator comes first, as the units are defined (so
    /// that `2.54 cm` stores exactly 1), and a product that overflows is worked again dividing first (`NauticalMile.to_internal(1e302)` is 7.3E306, where the product with
    /// 1,852,000 is infinity). A number that does not fit is an infinity, with its sign.
    pub fn to_internal(self, value: f64) -> f64 {
        let row = self.row();
        times_over(value, row.mul, row.div)
    }

    /// Internal units, as the number this unit shows: the inverse of [`Unit::to_internal`], and finite wherever its true value fits.
    pub fn from_internal(self, stored: f64) -> f64 {
        let row = self.row();
        times_over(stored, row.div, row.mul)
    }
}

/// `value * num / den`, and `value / den * num` where the first overflowed in the middle of a number that is finite: the result is the same
/// wherever the first is finite, to the last bit, and the second only runs where the first was an infinity, so that no number that fits is lost
/// to the product. A number that is not finite, and a true overflow, are the infinity or NaN the first gave.
fn times_over(value: f64, num: f64, den: f64) -> f64 {
    let direct = value * num / den;
    if direct.is_infinite() && value.is_finite() { value / den * num } else { direct }
}

/// `x * scale`, with a zero or a number that is not finite left as it is: a zero is zero in any unit (`0 * inf` and `0 / 0` are NaN, and the scale of a unit
/// to a large power, `63360^-128`, is 0, and to another, `63360^127`, is infinity), and an infinity is an infinity in any.
fn scaled(x: f64, scale: f64) -> f64 {
    if x == 0.0 || !x.is_finite() { x } else { x * scale }
}

/// `x / scale`: see [`scaled`].
fn per_scale(x: f64, scale: f64) -> f64 {
    if x == 0.0 || !x.is_finite() { x } else { x / scale }
}

/// The unit a suffix or `U` value names, in any case, or `None`. Accepts every suffix of R22, every `U` value (`IN_F`,
/// `PER`, `AC`, `F_I`, `AD`, `C_D`, `P_PT`, `DATE`), and the longer spellings. White space around the text is
/// ignored. Not units: an empty text (a cell with no `U` is [`Unit::Unitless`], which the caller chooses), a power (`IN^2`,
/// which is a multidimensional value) and a name the spec does not list.
pub fn parse_unit(text: &str) -> Option<Unit> {
    let text = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if text.is_empty() || text.len() > LONGEST_SPELLING {
        return None;
    }
    let named = |spelling: &str| spelling.eq_ignore_ascii_case(text);
    // `Unit::ALL` lists `Inch` before `InchFine`, and `Mile` before `MileFine`: both write the same suffix, and a bare `IN` is the inch.
    Unit::ALL
        .iter()
        .copied()
        .filter(|u| *u != Unit::Unitless)
        .find(|u| named(u.code()) || u.suffix().is_some_and(named))
        .or_else(|| EXTRA_SPELLINGS.iter().find(|(spelling, _)| named(spelling)).map(|(_, unit)| *unit))
}

/// A number as a cell stores it, in the internal unit of its measure, with the unit it is shown in and the power of that
/// unit (its dimension).
///
/// The dimension is 0 for a plain number, 1 for a unit named once (a length, an angle, a duration, a date, a percent), and 2
/// for the acre and the hectare, which already hold square inches. A product of two lengths is dimension 2 in a length unit
/// (`3 IN^2`): the stored number is then inches to that power, and the number shown is the stored one in the unit named, to the same
/// power (`2.54 cm * 2.54 cm` is stored as 1 square inch and shows 6.4516 `CM^2`). The range
/// of a power, -128 to 127, is the evaluator's to check: an `i8` holds exactly that range.
///
/// It is `#[non_exhaustive]`: a struct literal written in another crate would stop compiling when a field is added. Build one with
/// [`Quantity::new`], [`Quantity::from_value`] or [`Quantity::with_dim`]; reading the fields is as before.
///
/// ```compile_fail
/// let _ = diacraft_units::Quantity { stored: 1.0, unit: diacraft_units::Unit::Inch };
/// ```
///
/// ```
/// let q = diacraft_units::Quantity::new(1.0, diacraft_units::Unit::Inch);
/// assert_eq!((q.stored, q.unit, q.dim()), (1.0, diacraft_units::Unit::Inch, 1));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Quantity {
    /// The stored number: inches, radians, days, square inches or a fraction, to the power of the dimension. It is never rescaled
    /// for the unit.
    pub stored: f64,
    /// The unit the number is shown in; it decides only how the stored number reads.
    pub unit: Unit,
    dim: i8,
}

impl Unit {
    /// The dimension of one of this unit: 0 for no unit, 2 for the acre and the hectare (which hold square inches), 1 for the rest.
    pub fn natural_dim(self) -> i8 {
        match self.measure() {
            Measure::None => 0,
            Measure::Area => 2,
            Measure::Length | Measure::Angle | Measure::Duration | Measure::Percent | Measure::Date => 1,
        }
    }
}

impl Quantity {
    /// A stored number and the unit it is shown in, at the unit's own dimension ([`Unit::natural_dim`]).
    pub const fn new(stored: f64, unit: Unit) -> Self {
        let dim = match unit {
            Unit::Unitless => 0,
            Unit::Acre | Unit::Hectare => 2,
            _ => 1,
        };
        Quantity { stored, unit, dim }
    }

    /// A stored number, the unit it is shown in, and the power of that unit. A power of 0 is a plain number, so the unit is dropped:
    /// No range is checked here (R28 is the evaluator's).
    pub const fn with_dim(stored: f64, unit: Unit, dim: i8) -> Self {
        if dim == 0 { Quantity { stored, unit: Unit::Unitless, dim: 0 } } else { Quantity { stored, unit, dim } }
    }

    /// A number as typed in a unit (`2.54 cm`): the stored number is that many internal units. Non-finite numbers pass through.
    pub fn from_value(value: f64, unit: Unit) -> Self {
        Quantity::new(unit.to_internal(value), unit)
    }

    /// A number as shown in `unit` to the power `dim`: the stored number is that many internal units to the power (the inverse of
    /// [`Quantity::shown`]). A power of 0 is a plain number.
    pub fn from_shown(shown: f64, unit: Unit, dim: i8) -> Self {
        let q = Quantity { stored: 0.0, unit, dim };
        Quantity::with_dim(scaled(shown, q.scale(unit)), unit, dim)
    }

    /// The power of the unit: 0 for a plain number, 1 for a unit, 2 for an area, and any other for a multidimensional value.
    pub const fn dim(self) -> i8 {
        self.dim
    }

    /// Whether this is a unit raised to a power other than its own (a multidimensional value, `U` `MULTIDIM`): a length squared, a
    /// length per angle, and the acre at a dimension other than 2.
    pub fn is_multidimensional(self) -> bool {
        self.dim != self.unit.natural_dim()
    }

    /// How many stored units one shown unit is, to the power of the dimension: 1 for a plain number and an area unit (an acre holds
    /// square inches already), and the unit's factor to the power otherwise.
    fn scale(self, unit: Unit) -> f64 {
        match unit.measure() {
            Measure::None | Measure::Area => unit.base_per_unit(),
            _ => unit.base_per_unit().powi(i32::from(self.dim)),
        }
    }

    /// The number as shown in its own unit, to the power of the dimension.
    pub fn shown(self) -> f64 {
        per_scale(self.stored, self.scale(self.unit))
    }

    /// The number as shown in another unit of the same measure, or `None` for a unit of another measure (a length is not an
    /// angle). This is how a drawing unit that is not inches shows an implicit `DL` value. A multidimensional value is shown in
    /// the other unit to its own power.
    pub fn value_in(self, unit: Unit) -> Option<f64> {
        (unit.measure() == self.unit.measure()).then(|| per_scale(self.stored, self.scale(unit)))
    }

    /// The same stored number shown in another unit of the same measure, with the same dimension, or `None` for another measure.
    pub fn convert(self, unit: Unit) -> Option<Quantity> {
        (unit.measure() == self.unit.measure()).then_some(Quantity { stored: self.stored, unit, dim: self.dim })
    }

    /// `8FT 4IN`: feet and inches, stored in inches, shown as [`Unit::FeetInches`].
    pub fn feet_inches(feet: f64, inches: f64) -> Self {
        Quantity::new(Unit::Foot.to_internal(feet) + Unit::Inch.to_internal(inches), Unit::FeetInches)
    }

    /// Degrees, minutes and seconds, as `10` degrees `15` minutes `30` seconds, stored in radians, shown as [`Unit::DegreesMinutesSeconds`].
    pub fn degrees_minutes_seconds(degrees: f64, minutes: f64, seconds: f64) -> Self {
        let total = degrees + minutes / 60.0 + seconds / 3600.0;
        Quantity::new(Unit::Degree.to_internal(total), Unit::DegreesMinutesSeconds)
    }

    /// `2C4`: ciceros then didots, stored in inches, shown as [`Unit::CiceroDidot`].
    pub fn cicero_didot(ciceros: f64, didots: f64) -> Self {
        Quantity::new(Unit::Cicero.to_internal(ciceros) + Unit::Didot.to_internal(didots), Unit::CiceroDidot)
    }

    /// `3P6`: picas then points, stored in inches, shown as [`Unit::PicaPoints`].
    pub fn pica_points(picas: f64, points: f64) -> Self {
        Quantity::new(Unit::Pica.to_internal(picas) + Unit::Point.to_internal(points), Unit::PicaPoints)
    }
}
