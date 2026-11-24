//! The format picture language: a value through a picture, as text.
//!
//! A picture is read in four steps: the casting prefix `<src,dst>` is cut off, the tokens `esc(n)` and `{<n>}` are replaced by the
//! pictures, the family is decided (string, numeric, date or literal), and the value is printed by that family's rules.
//! Every loop is over the picture or over a number's fifteen digits, and a picture is capped, so no input costs more than its length.

use diacraft_units::{Quantity, Unit, parse_unit};
use jiff::civil::{DateTime, Weekday};

use crate::store::serial_to_datetime;
use crate::value::CellError;

/// The longest picture worked. A project rule: a picture is a few tokens, and a file cannot make one cost more than this.
const MAX_PICTURE_CHARS: usize = 4096;
/// The most digits a fraction's denominator may have (MS-VSDX allows five).
const MAX_DENOMINATOR_DIGITS: usize = 5;
/// The field picture codes whose picture is a cast and a format (the table): they carry a `<,dst>` prefix of their own.
const SIGNIFICANT_DIGITS: usize = 15;

/// The marks of a locale: the decimal mark and the grouping mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Locale {
    pub decimal: char,
    pub group: char,
}

impl Locale {
    pub(crate) const EN_US: Locale = Locale { decimal: '.', group: ',' };

    /// The marks of a language tag such as `de-DE`: those of its primary language. Any tag not listed has en-US's marks.
    pub(crate) fn of_tag(tag: &str) -> Locale {
        let primary = tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        match primary.as_str() {
            "de" | "es" | "it" | "nl" | "pt" | "da" | "tr" | "id" | "el" | "ro" | "hr" | "sl" => Locale { decimal: ',', group: '.' },
            "fr" | "ru" | "pl" | "sv" | "fi" | "nb" | "no" | "cs" | "hu" | "sk" | "uk" | "bg" | "lt" | "lv" | "et" => {
                Locale { decimal: ',', group: '\u{a0}' }
            }
            _ => Locale::EN_US,
        }
    }

    /// The marks of an LCID: the low ten bits are the primary language.
    pub(crate) fn of_lcid(lcid: u32) -> Locale {
        match lcid & 0x3FF {
            0x07 | 0x0A | 0x10 | 0x13 | 0x16 | 0x06 | 0x1F | 0x21 | 0x08 | 0x18 | 0x1A | 0x24 => Locale { decimal: ',', group: '.' },
            0x0C | 0x19 | 0x15 | 0x1D | 0x0B | 0x14 | 0x05 | 0x0E | 0x1B | 0x22 | 0x02 | 0x27 | 0x25 | 0x26 => {
                Locale { decimal: ',', group: '\u{a0}' }
            }
            _ => Locale::EN_US,
        }
    }
}

/// What a picture is applied to.
#[derive(Clone, Debug)]
pub(crate) enum Subject {
    /// A number, with its unit.
    Number(Quantity),
    /// A date or duration, by its serial.
    Date(f64),
    /// A string.
    Text(String),
}

/// The unit a cast shows in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dst {
    /// The value's own unit.
    Own,
    /// This unit.
    Unit(Unit),
}

/// A cast: the unit a unit-free value is read in, and the unit shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cast {
    pub src: Option<Unit>,
    pub dst: Dst,
}

impl Cast {
    pub(crate) const NONE: Cast = Cast { src: None, dst: Dst::Own };
}

/// Why a picture gave no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PictureFailure {
    Error(CellError),
    /// A feature this evaluator does not have (a compound unit cast): the saved value stands.
    Unsupported,
}

const BAD: PictureFailure = PictureFailure::Error(CellError::Value);

/// A unit given as a string in a cast: `None` for the empty string, `$` and `NOCAST` (no cast), and an error for a name no unit has. A
/// compound unit (`FEET/INCH`) is not supported.
pub(crate) fn parse_cast_unit(text: &str) -> Result<Option<Unit>, PictureFailure> {
    let t = text.trim();
    if t.is_empty() || t == "$" || t.eq_ignore_ascii_case("NOCAST") {
        return Ok(None);
    }
    match parse_unit(t) {
        Some(Unit::FeetInches | Unit::CiceroDidot | Unit::PicaPoints | Unit::DegreesMinutesSeconds) => Err(PictureFailure::Unsupported),
        Some(u) => Ok(Some(u)),
        None => Err(BAD),
    }
}

/// The table: the picture a field picture code stands for.
pub(crate) fn code_picture(code: u32) -> Option<&'static str> {
    Some(match code {
        0 => "0.####",
        1 => "0.#### u",
        2 => "0",
        3 => "0 u",
        4 => "0.0",
        5 => "0.0 u",
        6 => "0.00",
        7 => "0.00 u",
        8 => "0.000",
        9 => "0.000 u",
        10 => "<,FEET/INCH>0.000 u",
        11 => "<,rad>0.#### u",
        12 => "<,deg>0.# u",
        13 => "<,FEET/INCH># #/# u",
        14 => "<,FEET/INCH># #/## u",
        15 => "0 #/#",
        16 => "0 #/# u",
        17 => "0 #/##",
        18 => "0 #/## u",
        20 => "ddddd",
        21 => "dddddd",
        22 => "M/d/y",
        23 => "MM/dd/yy",
        24 => "MMM d, yyyy",
        25 => "MMMM d, yyyy",
        26 => "d/M/YY",
        27 => "dd/MM/yy",
        28 => "d MMM, yyyy",
        29 => "d MMMM, yyyy",
        30 => "T",
        31 => "h:mm",
        32 => "hh:mm",
        33 => "H:mm",
        34 => "HH:mm",
        35 => "h:mm tt",
        36 => "hh:mm tt",
        37 => "@",
        38 => "@-",
        39 => "@+",
        40..=81 | 200 | 217..=219 => "M/d/yyyy",
        201 => "dddd, MMMM dd, yyyy",
        202 => "MMMM d, yyyy",
        203 => "M/d/yy",
        204 => "yyyy-MM-dd",
        205 => "d-MMM-yy",
        206 => "M.d.yyyy",
        207 => "MMM. d, yy",
        208 => "D MMMM yyyy",
        209 => "MMMM yy",
        210 => "MMM-yy",
        211 => "M/d/yyyy h:mm am/pm",
        212 => "M/d/yyyy h:mm:ss am/pm",
        213 => "H:mm am/pm",
        214 => "H:m:ss am/pm",
        215 => "HH:mm",
        216 => "HH:mm:ss",
        _ => return None,
    })
}

/// What `FIELDPICTURE(code)` gives: `esc(n)` for 0 to 18, the table's picture for 20 to 81, `{<n>}` from 200.
pub(crate) fn field_picture(code: u32) -> String {
    match code {
        0..=19 => format!("esc({code})"),
        20..=81 => code_picture(code).unwrap_or("").to_owned(),
        200.. => format!("{{<{code}>}}"),
        // 82 to 199 name no picture.
        _ => String::new(),
    }
}

/// A piece of a picture as scanned: text that is not a token, or one character to be read as a token.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Lit(String),
    Ch(char),
}

/// Scans a picture: `\` and a character, and `"text"` and `'text'`, are literal text; an unclosed quote runs to the end.
fn scan(picture: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = picture.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(Tok::Lit(next.to_string()));
                }
            }
            '"' | '\'' => {
                let mut text = String::new();
                for q in chars.by_ref() {
                    if q == c {
                        break;
                    }
                    text.push(q);
                }
                out.push(Tok::Lit(text));
            }
            other => out.push(Tok::Ch(other)),
        }
    }
    out
}

fn is_ch(t: &Tok, c: char) -> bool {
    matches!(t, Tok::Ch(x) if *x == c)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Text,
    Numeric,
    Date,
    Literal,
}

fn family(toks: &[Tok]) -> Family {
    let has = |f: &dyn Fn(char) -> bool| toks.iter().any(|t| matches!(t, Tok::Ch(c) if f(*c)));
    if has(&|c| c == '@') {
        Family::Text
    } else if has(&|c| c == '#' || c == '0') {
        Family::Numeric
    } else if has(&|c| "cdeghmnstwy".contains(c.to_ascii_lowercase())) {
        Family::Date
    } else {
        Family::Literal
    }
}

/// Cuts a `<src,dst>` prefix off a picture: the two strings, and the rest. `None` when there is no prefix; an error when it is not closed.
fn split_prefix(picture: &str) -> Result<Option<(String, String, &str)>, PictureFailure> {
    let Some(rest) = picture.strip_prefix('<') else { return Ok(None) };
    let Some(end) = rest.find('>') else { return Err(BAD) };
    let (inside, after) = (&rest[..end], &rest[end + 1..]);
    let (src, dst) = inside.split_once(',').unwrap_or((inside, ""));
    Ok(Some((src.trim().to_owned(), dst.trim().to_owned(), after)))
}

/// Replaces `esc(n)` and `{<n>}` with the pictures. In `esc`, a code above 18 takes code 0; in `{<n>}`, a code that is
/// not in the table takes code 0 for a number, 200 for a date or duration, and gives nothing for a string.
fn expand(picture: &str, subject: &Subject) -> String {
    let chars: Vec<char> = picture.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while let Some(&c) = chars.get(i) {
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                if let Some(&n) = chars.get(i + 1) {
                    out.push(n);
                }
                i += 2;
            }
            '"' | '\'' => {
                quote = Some(c);
                out.push(c);
                i += 1;
            }
            'e' if chars.get(i..i + 4).is_some_and(|w| w == ['e', 's', 'c', '(']) => {
                if let Some((n, next)) = number_then(&chars, i + 4, ')') {
                    let code = if n > 18 { 0 } else { n };
                    out.push_str(code_picture(code).unwrap_or(""));
                    i = next;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            '{' if chars.get(i + 1) == Some(&'<') => {
                if let Some((n, next)) = number_then(&chars, i + 2, '>').and_then(|(n, at)| (chars.get(at) == Some(&'}')).then_some((n, at + 1))) {
                    let picture = code_picture(n).unwrap_or(match subject {
                        Subject::Number(_) => "0.####",
                        Subject::Date(_) => "M/d/yyyy",
                        Subject::Text(_) => "",
                    });
                    out.push_str(picture);
                    i = next;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Digits at `at`, then `close`: the number, and the index after `close`.
fn number_then(chars: &[char], at: usize, close: char) -> Option<(u32, usize)> {
    let mut n: u32 = 0;
    let mut j = at;
    while let Some(d) = chars.get(j).and_then(|c| c.to_digit(10)) {
        n = n.saturating_mul(10).saturating_add(d);
        j += 1;
        if j - at > 9 {
            return None;
        }
    }
    (j > at && chars.get(j) == Some(&close)).then_some((n, j + 1))
}

/// The value through the picture.
pub(crate) fn format(subject: &Subject, picture: &str, cast: Cast, locale: Locale) -> Result<String, PictureFailure> {
    if picture.chars().count() > MAX_PICTURE_CHARS {
        return Err(BAD);
    }
    let mut cast = cast;
    // The prefix of the picture as written, then, after the tokens are replaced, that of what a token stood for (codes 10 to 14).
    let mut working = picture.to_owned();
    for pass in 0..2 {
        if let Some((src, dst, rest)) = split_prefix(&working)? {
            if let Some(u) = parse_cast_unit(&src)? {
                cast.src = Some(u);
            }
            if let Some(u) = parse_cast_unit(&dst)? {
                cast.dst = Dst::Unit(u);
            }
            working = rest.to_owned();
        }
        if pass == 0 {
            working = expand(&working, subject);
        }
    }
    // An Office date picture in `{{ }}` is read by the same date tokens.
    let trimmed = working.trim();
    if trimmed.len() >= 4 && trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        working = trimmed[2..trimmed.len() - 2].to_owned();
    }
    if working.is_empty() {
        return Ok(String::new());
    }
    let toks = scan(&working);
    let fam = family(&toks);
    match (fam, subject) {
        (Family::Literal, _) => Ok(literal(&toks)),
        (Family::Text, Subject::Text(s)) => string_picture(s, &toks),
        (Family::Text, Subject::Number(q)) => string_picture(&crate::convert::number_to_text(*q), &toks),
        (Family::Text, Subject::Date(d)) => string_picture(&crate::store::serial_to_iso(*d).unwrap_or_default(), &toks),
        // A string under a numeric or date picture comes back unchanged.
        (Family::Numeric | Family::Date, Subject::Text(s)) => Ok(s.clone()),
        (Family::Numeric, Subject::Number(q)) => numeric(*q, &toks, cast, locale),
        // A date under a numeric picture is the general date.
        (Family::Numeric, Subject::Date(d)) => date_picture(*d, &scan("c"), locale),
        (Family::Date, Subject::Number(q)) => date_picture(q.stored, &toks, locale),
        (Family::Date, Subject::Date(d)) => date_picture(*d, &toks, locale),
    }
}

fn literal(toks: &[Tok]) -> String {
    toks.iter()
        .map(|t| match t {
            Tok::Lit(s) => s.clone(),
            Tok::Ch(c) => c.to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Case.

/// A character in capitals, one for one: where Unicode maps it to several it stays, as do `µ` and `ſ`.
pub(crate) fn upper_char(c: char) -> char {
    if matches!(c, '\u{b5}' | '\u{17f}') {
        return c;
    }
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// A character in lower case, one for one: `İ` becomes `i`, and a letter that maps to several stays.
pub(crate) fn lower_char(c: char) -> char {
    if c == '\u{130}' {
        return 'i';
    }
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

pub(crate) fn to_upper(s: &str) -> String {
    s.chars().map(upper_char).collect()
}

pub(crate) fn to_lower(s: &str) -> String {
    s.chars().map(lower_char).collect()
}

// ---------------------------------------------------------------------------------------------------------------------------------
// String pictures.

fn string_picture(text: &str, toks: &[Tok]) -> Result<String, PictureFailure> {
    let mut out = String::new();
    let mut i = 0;
    while let Some(t) = toks.get(i) {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Ch('@') => match toks.get(i + 1) {
                Some(n) if is_ch(n, '+') => {
                    out.push_str(&to_upper(text));
                    i += 1;
                }
                Some(n) if is_ch(n, '-') => {
                    out.push_str(&to_lower(text));
                    i += 1;
                }
                _ => out.push_str(text),
            },
            Tok::Ch(c) => out.push(*c),
        }
        // A long text under a picture of thousands of `@` would otherwise ask for gigabytes.
        if out.len() > crate::rules::MAX_TEXT_BYTES {
            return Err(BAD);
        }
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Numbers: the fifteen-digit decimal form that rounding is judged on.

/// A non-negative number as up to fifteen decimal digits: the value is `0.d1d2d3... * 10^pow`. Zero has no digits.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dec {
    digits: Vec<u8>,
    pow: i32,
}

impl Dec {
    fn of(x: f64) -> Dec {
        let x = x.abs();
        if x == 0.0 || !x.is_finite() {
            return Dec { digits: Vec::new(), pow: 0 };
        }
        let s = format!("{:.*e}", SIGNIFICANT_DIGITS - 1, x);
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let e: i32 = exp.parse().unwrap_or(0);
        let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
        while digits.last() == Some(&0) {
            digits.pop();
        }
        Dec { digits, pow: e + 1 }
    }

    fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    fn digit(&self, i: i32) -> u8 {
        usize::try_from(i).ok().and_then(|i| self.digits.get(i)).copied().unwrap_or(0)
    }

    /// Rounded half away from zero to `places` places after the point (a negative count rounds to tens and beyond).
    fn round(&self, places: i32) -> Dec {
        let keep = self.pow.saturating_add(places);
        if keep < 0 {
            return Dec { digits: Vec::new(), pow: 0 };
        }
        let keep = keep as usize;
        let mut d: Vec<u8> = self.digits.iter().copied().take(keep).collect();
        let mut pow = self.pow;
        if self.digits.get(keep).is_some_and(|n| *n >= 5) {
            let mut i = d.len();
            let mut carry = true;
            while carry && i > 0 {
                i -= 1;
                if d[i] == 9 {
                    d[i] = 0;
                } else {
                    d[i] += 1;
                    carry = false;
                }
            }
            if carry {
                d.insert(0, 1);
                pow += 1;
            }
        }
        while d.last() == Some(&0) {
            d.pop();
        }
        if d.is_empty() { Dec { digits: d, pow: 0 } } else { Dec { digits: d, pow } }
    }

    /// The whole part's digits, with no leading zero; empty for a value below 1.
    fn int_digits(&self) -> String {
        (0..self.pow.max(0)).map(|i| char::from(b'0' + self.digit(i))).collect()
    }

    /// The first `places` digits after the point.
    fn dec_digits(&self, places: usize) -> Vec<u8> {
        (0..places as i32).map(|j| self.digit(self.pow + j)).collect()
    }
}

/// A numeric picture's pieces.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Lit(String),
    Space,
    Int(char),
    Dec(char),
    Point,
    Group,
    /// `E+`, `e-` and the like with the placeholders after it.
    Exp {
        letter: char,
        plus: bool,
        width: usize,
    },
    Label {
        upper: bool,
        n: usize,
    },
}

fn pieces(toks: &[Tok]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut after_point = false;
    let mut i = 0;
    while let Some(t) = toks.get(i) {
        match t {
            Tok::Lit(s) => out.push(Piece::Lit(s.clone())),
            Tok::Ch(' ') => out.push(Piece::Space),
            Tok::Ch(c @ ('#' | '0')) => out.push(if after_point { Piece::Dec(*c) } else { Piece::Int(*c) }),
            Tok::Ch('.') => {
                if after_point {
                    // The later marks are dropped.
                } else {
                    after_point = true;
                    out.push(Piece::Point);
                }
            }
            Tok::Ch(',') => out.push(Piece::Group),
            Tok::Ch(letter @ ('E' | 'e'))
                if toks.get(i + 1).is_some_and(|n| is_ch(n, '+') || is_ch(n, '-'))
                    && toks.get(i + 2).is_some_and(|n| is_ch(n, '#') || is_ch(n, '0')) =>
            {
                let plus = toks.get(i + 1).is_some_and(|n| is_ch(n, '+'));
                let mut width = 0;
                let mut j = i + 2;
                while let Some(Tok::Ch(c @ ('#' | '0'))) = toks.get(j) {
                    if *c == '0' {
                        width += 1;
                    }
                    j += 1;
                }
                out.push(Piece::Exp { letter: *letter, plus, width });
                i = j;
                continue;
            }
            Tok::Ch(c @ ('u' | 'U')) => {
                let mut n = 1;
                while n < 3 && toks.get(i + n).is_some_and(|t| is_ch(t, *c)) {
                    n += 1;
                }
                out.push(Piece::Label { upper: *c == 'U', n });
                i += n;
                continue;
            }
            Tok::Ch(c) => out.push(Piece::Lit(c.to_string())),
        }
        i += 1;
    }
    out
}

/// A number's unit, short and long and universal names.
fn unit_names(unit: Unit) -> (String, String) {
    match unit {
        Unit::Inch | Unit::InchFine | Unit::DrawingDefault | Unit::PageDefault | Unit::TypeDefault => ("in".to_owned(), "inches".to_owned()),
        Unit::Foot => ("ft".to_owned(), "feet".to_owned()),
        Unit::Millimetre => ("mm".to_owned(), "millimeters".to_owned()),
        Unit::Centimetre => ("cm".to_owned(), "centimeters".to_owned()),
        Unit::Degree => ("deg".to_owned(), "degrees".to_owned()),
        Unit::Radian => ("rad".to_owned(), "radians".to_owned()),
        other => {
            // By pattern, the lower-case universal name and its English plural.
            let name = other.suffix().unwrap_or_else(|| other.code()).to_ascii_lowercase();
            let long = format!("{name}s");
            (name, long)
        }
    }
}

fn is_metric(unit: Unit) -> bool {
    matches!(unit, Unit::Millimetre | Unit::Centimetre | Unit::Metre | Unit::Kilometre)
}

fn label_text(unit: Unit, n: usize, upper: bool, spaced: bool) -> String {
    let (short, long) = unit_names(unit);
    let s = match n {
        1 => {
            // A metric unit has no point when no space comes before its label.
            if is_metric(unit) && !spaced { short } else { format!("{short}.") }
        }
        2 => long,
        _ => short,
    };
    if upper { s.to_ascii_uppercase() } else { s }
}

/// Reads a quantity through a cast: the number to print and the unit it is in, or `None` for a bare number.
fn cast_number(q: Quantity, cast: Cast) -> Result<(f64, Option<Unit>), PictureFailure> {
    if q.unit == Unit::Unitless {
        let Some(src) = cast.src else { return Ok((q.stored, None)) };
        // A unit-free value is first read in `src`.
        let read = Quantity::from_shown(q.stored, src, src.natural_dim());
        return match cast.dst {
            Dst::Own => Ok((read.shown(), Some(src))),
            Dst::Unit(d) => read.convert(d).map(|c| (c.shown(), Some(d))).ok_or(BAD),
        };
    }
    match cast.dst {
        // `DL`, `DP` and `DT` are shown in the document's own unit (a page in metres prints metres), which
        // this crate does not look up: the picture is unsupported and the saved text stands, where it printed inches.
        Dst::Own if matches!(q.unit, Unit::DrawingDefault | Unit::PageDefault | Unit::TypeDefault) => Err(PictureFailure::Unsupported),
        Dst::Own => Ok((q.shown(), Some(q.unit))),
        Dst::Unit(d) => q.convert(d).map(|c| (c.shown(), Some(d))).ok_or(BAD),
    }
}

fn group_digits(digits: &str, mark: char) -> String {
    let n = digits.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(mark);
        }
        out.push(c);
    }
    out
}

/// What the printed digits of a number need: the placeholders as the picture holds them.
struct Holes {
    /// The whole part's placeholders, left to right.
    int: Vec<char>,
    /// The fraction's.
    dec: Vec<char>,
    grouped: bool,
}

fn holes(pieces: &[Piece]) -> Holes {
    let mut h = Holes { int: Vec::new(), dec: Vec::new(), grouped: false };
    let mut seen_int = false;
    for p in pieces {
        match p {
            Piece::Exp { .. } => break,
            Piece::Int(c) => {
                h.int.push(*c);
                seen_int = true;
            }
            Piece::Dec(c) => h.dec.push(*c),
            Piece::Group if seen_int => h.grouped = true,
            _ => {}
        }
    }
    h
}

/// The fraction digits as the placeholders print them: trailing zeros under `#` drop.
fn dec_text(digits: &[u8], dec_holes: &[char]) -> String {
    let mut n = digits.len();
    while n > 0 && digits.get(n - 1) == Some(&0) && dec_holes.get(n - 1) == Some(&'#') {
        n -= 1;
    }
    digits.iter().take(n).map(|d| char::from(b'0' + d)).collect()
}

struct Printer<'a> {
    locale: Locale,
    unit: Option<Unit>,
    negative: bool,
    out: String,
    holes: &'a Holes,
}

impl Printer<'_> {
    fn sign(&mut self) {
        if self.negative {
            self.out.push('-');
            self.negative = false;
        }
    }

    /// The pieces in order, with `int` and `dec` as the digits to print. `mantissa_end` stops at an exponent piece.
    fn run(&mut self, pieces: &[Piece], int: &str, dec: &str, int_done: &mut bool, point_done: &mut bool) {
        for p in pieces {
            match p {
                Piece::Lit(s) => self.out.push_str(s),
                Piece::Space => self.out.push(' '),
                Piece::Int(_) => {
                    if !*int_done {
                        *int_done = true;
                        self.sign();
                        let text = if self.holes.grouped { group_digits(int, self.locale.group) } else { int.to_owned() };
                        self.out.push_str(&text);
                    }
                }
                Piece::Group => {}
                Piece::Point => {
                    if !*point_done {
                        *point_done = true;
                        if !dec.is_empty() {
                            if !*int_done {
                                self.sign();
                            }
                            self.out.push(self.locale.decimal);
                        }
                    }
                }
                Piece::Dec(_) => {
                    if !*point_done {
                        // A fraction placeholder with no mark before it: no mark to print.
                    }
                }
                Piece::Exp { .. } => {}
                Piece::Label { upper, n } => self.label(*upper, *n),
            }
        }
    }

    fn label(&mut self, upper: bool, n: usize) {
        match self.unit {
            Some(unit) => {
                let spaced = self.out.ends_with(' ');
                let text = label_text(unit, n, upper, spaced);
                self.out.push_str(&text);
            }
            // A bare number prints alone, and the spaces before the label go with it.
            None => {
                while self.out.ends_with(' ') {
                    self.out.pop();
                }
            }
        }
    }
}

/// Prints the fraction digits after `run` has done the whole part: the fraction digits go right after the mark, so `run` is given the
/// text; this lays them in.
fn lay_fraction(pieces: &[Piece], printer: &mut Printer<'_>, int: &str, dec: &str) {
    // The digits of the fraction follow the mark as one run, whatever the placeholders: `run` writes the mark and this the digits.
    let mut int_done = false;
    let mut point_done = false;
    let mut before_dec = true;
    for p in pieces {
        match p {
            Piece::Dec(_) if before_dec => {
                before_dec = false;
                printer.out.push_str(dec);
            }
            Piece::Dec(_) => {}
            other => printer.run(std::slice::from_ref(other), int, dec, &mut int_done, &mut point_done),
        }
    }
}

/// A scientific mantissa and exponent for a value that does not fit a fixed picture: the digits, with the mark.
fn fallback(d: &Dec, places: usize, large: bool, holes: &Holes) -> (String, i32) {
    // Small values keep `places` significant digits; large ones `places` decimals.
    let sig = if large { places + 1 } else { places };
    let r = d.round(sig as i32 - d.pow);
    let carried = r.pow > d.pow;
    let exp = r.pow - 1;
    let first = char::from(b'0' + r.digit(0));
    let tail_len = if carried && !large { places } else { sig.saturating_sub(1) };
    let tail: Vec<u8> = (1..=tail_len as i32).map(|i| r.digit(i)).collect();
    let tail_text = dec_text(&tail, &holes.dec);
    let mantissa = if tail_text.is_empty() && holes.dec.iter().all(|c| *c == '#') { first.to_string() } else { format!("{first}.{tail_text}") };
    (mantissa, exp)
}

fn numeric(q: Quantity, toks: &[Tok], cast: Cast, locale: Locale) -> Result<String, PictureFailure> {
    let (x, unit) = cast_number(q, cast)?;
    if !x.is_finite() {
        return Ok(if x < 0.0 { "-1.#INF".to_owned() } else { "1.#INF".to_owned() });
    }
    let ps = pieces(toks);
    let h = holes(&ps);
    let lowercase_marks = toks.iter().any(|t| is_ch(t, 'u'));
    if toks.iter().any(|t| is_ch(t, '/')) {
        return fraction(x, unit, toks, locale);
    }
    let d = Dec::of(x);
    let negative = x < 0.0;
    let mut printer = Printer { locale, unit, negative, out: String::new(), holes: &h };
    // Scientific by the picture.
    if let Some(at) = ps.iter().position(|p| matches!(p, Piece::Exp { .. })) {
        let Some(Piece::Exp { letter, plus, width }) = ps.get(at).cloned() else { return Err(BAD) };
        let nint = h.int.len().max(1) as i32;
        let (digits, exp) = if d.is_zero() {
            (Dec { digits: Vec::new(), pow: 0 }, 0)
        } else {
            let shifted = Dec { digits: d.digits.clone(), pow: nint };
            let r = shifted.round(h.dec.len() as i32);
            if r.pow > nint { (Dec { digits: r.digits, pow: nint }, d.pow - nint + 1) } else { (r, d.pow - nint) }
        };
        let int_text = {
            let t = digits.int_digits();
            pad_int(t, &h.int)
        };
        let dec_digits = digits.dec_digits(h.dec.len());
        let dec = dec_text(&dec_digits, &h.dec);
        printer.negative = negative && !digits.is_zero();
        lay_fraction(&ps[..at], &mut printer, &int_text, &dec);
        let mut e = exp.unsigned_abs().to_string();
        while e.len() < width {
            e.insert(0, '0');
        }
        let sign = if exp < 0 {
            "-"
        } else if plus {
            "+"
        } else {
            ""
        };
        printer.out.push(letter);
        printer.out.push_str(sign);
        printer.out.push_str(&e);
        let (mut a, mut b) = (true, true);
        printer.run(&ps[at + 1..], "", "", &mut a, &mut b);
        return Ok(printer.out);
    }
    let places = h.dec.len();
    let rounded = d.round(places as i32);
    let too_large = x.abs() >= 1e15 && places > 0;
    if places > 0 && ((rounded.is_zero() && !d.is_zero()) || too_large) {
        let (mantissa, exp) = fallback(&d, places, too_large, &h);
        let letter = if lowercase_marks { 'e' } else { 'E' };
        let digit_like = |p: &Piece| matches!(p, Piece::Dec(_) | Piece::Int(_) | Piece::Point | Piece::Group);
        let first = ps.iter().position(digit_like).unwrap_or(0);
        let last = ps.iter().rposition(digit_like).map_or(0, |i| i + 1);
        let (mut a, mut b) = (true, true);
        printer.negative = false;
        printer.run(ps.get(..first).unwrap_or(&[]), "", "", &mut a, &mut b);
        if negative {
            printer.out.push('-');
        }
        printer.out.push_str(&format!("{mantissa}{letter}{exp}"));
        printer.run(ps.get(last..).unwrap_or(&[]), "", "", &mut a, &mut b);
        return Ok(printer.out);
    }
    let int_digits = rounded.int_digits();
    let int_text = pad_int(int_digits, &h.int);
    let dec_digits = rounded.dec_digits(places);
    let dec = dec_text(&dec_digits, &h.dec);
    // The sign shows only when something non-zero is printed (no negative zero).
    printer.negative = negative && !rounded.is_zero();
    lay_fraction(&ps, &mut printer, &int_text, &dec);
    Ok(printer.out)
}

/// The whole part padded by the `0` placeholders: a `0` prints a digit in its place.
fn pad_int(digits: String, holes: &[char]) -> String {
    let first_zero = holes.iter().position(|c| *c == '0');
    let min_len = first_zero.map_or(0, |i| holes.len() - i);
    let mut s = digits;
    while s.len() < min_len {
        s.insert(0, '0');
    }
    s
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Fractions.

fn gcd(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The fraction `n/d` with `d <= max` nearest to `x`, a smaller denominator winning a tie: gaps are judged on the 15-digit form.
fn nearest(x: f64, max_den: u64) -> (u64, u64) {
    let value = x;
    let mut best = (value.round() as u64, 1u64);
    let mut best_gap = f64::INFINITY;
    for d in 1..=max_den {
        let n = (value * d as f64).round();
        let gap = (value - n / d as f64).abs();
        if gap < best_gap - 1e-12 {
            best_gap = gap;
            best = (n as u64, d);
        }
    }
    best
}

fn fraction(x: f64, unit: Option<Unit>, toks: &[Tok], locale: Locale) -> Result<String, PictureFailure> {
    let Some(slash) = toks.iter().position(|t| is_ch(t, '/')) else { return Err(BAD) };
    let placeholder = |t: &Tok| matches!(t, Tok::Ch('#' | '0'));
    // The numerator's placeholders end at the slash; a space and more placeholders before them are the whole part.
    let mut num_start = slash;
    while num_start > 0 && toks.get(num_start - 1).is_some_and(placeholder) {
        num_start -= 1;
    }
    let mut whole_start = None;
    if num_start > 0 && toks.get(num_start - 1).is_some_and(|t| is_ch(t, ' ')) {
        let mut w = num_start - 1;
        while w > 0 && toks.get(w - 1).is_some_and(placeholder) {
            w -= 1;
        }
        if w < num_start - 1 {
            whole_start = Some(w);
        }
    }
    let prefix_end = whole_start.unwrap_or(num_start);
    // The denominator: `#`s then digits.
    let mut den_end = slash + 1;
    let mut hashes = 0;
    while toks.get(den_end).is_some_and(|t| is_ch(t, '#')) {
        hashes += 1;
        den_end += 1;
    }
    let mut den_digits = String::new();
    while let Some(Tok::Ch(c)) = toks.get(den_end) {
        if c.is_ascii_digit() {
            den_digits.push(*c);
            den_end += 1;
        } else {
            break;
        }
    }
    let fixed: Option<u64> = den_digits.parse().ok().filter(|n: &u64| *n > 0 && *n <= 1_000_000);
    let simplify = hashes > 0 && fixed.is_some();
    let max_den = if hashes > 0 && fixed.is_none() { 10u64.pow(hashes.min(MAX_DENOMINATOR_DIGITS) as u32) - 1 } else { 0 };
    let magnitude = Dec::of(x);
    let abs = if magnitude.is_zero() { 0.0 } else { x.abs() };
    let has_whole = whole_start.is_some();
    let (whole, frac) = if has_whole { (abs.trunc(), abs - abs.trunc()) } else { (0.0, abs) };
    let (mut n, mut d) = match fixed {
        Some(den) => ((frac * den as f64).round() as u64, den),
        None => nearest(frac, max_den.max(1)),
    };
    let mut whole_n = whole as u64;
    if has_whole && n >= d {
        whole_n += n / d;
        n %= d;
    }
    if simplify && n > 0 {
        let g = gcd(n, d);
        n /= g;
        d /= g;
    }
    let mut body = String::new();
    let negative = x < 0.0 && whole_n + n > 0;
    if negative {
        body.push('-');
    }
    if has_whole {
        match (whole_n, n) {
            (0, 0) => body.push('0'),
            (0, _) => body.push_str(&format!("{n}/{d}")),
            (w, 0) => body.push_str(&w.to_string()),
            (w, _) => body.push_str(&format!("{w} {n}/{d}")),
        }
    } else if n == 0 {
        body.push('0');
    } else {
        body.push_str(&format!("{n}/{d}"));
    }
    let ps_before = pieces(toks.get(..prefix_end).unwrap_or(&[]));
    let ps_after = pieces(toks.get(den_end..).unwrap_or(&[]));
    let holes_none = Holes { int: Vec::new(), dec: Vec::new(), grouped: false };
    let mut printer = Printer { locale, unit, negative: false, out: String::new(), holes: &holes_none };
    let (mut a, mut b) = (true, true);
    printer.run(&ps_before, "", "", &mut a, &mut b);
    printer.out.push_str(&body);
    printer.run(&ps_after, "", "", &mut a, &mut b);
    Ok(printer.out)
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Dates.

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn weekday_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

fn short(name: &str) -> String {
    name.chars().take(3).collect()
}

fn hour12(h: i8) -> i8 {
    match h % 12 {
        0 => 12,
        n => n,
    }
}

fn two(n: impl Into<i32>) -> String {
    format!("{:02}", n.into())
}

/// The name of the month, `""` for none (a month is 1 to 12).
fn month_name(dt: DateTime) -> &'static str {
    usize::from(dt.month().unsigned_abs()).checked_sub(1).and_then(|i| MONTHS.get(i)).copied().unwrap_or("")
}

fn date_picture(serial: f64, toks: &[Tok], locale: Locale) -> Result<String, PictureFailure> {
    let _ = locale;
    let dt = serial_to_datetime(serial).ok_or(BAD)?;
    let total_seconds = (serial.abs() * 86_400.0).round() as u64;
    let mut out = String::new();
    // The largest unit of an elapsed token takes the whole amount.
    let elapsed_units: Vec<char> = (0..toks.len()).filter_map(|i| bracket_at(toks, i).map(|(l, _)| l)).collect();
    let largest = ['d', 'h', 'm', 's'].into_iter().find(|u| elapsed_units.contains(u));
    let mut i = 0;
    while let Some(t) = toks.get(i) {
        let run = |c: char| toks.iter().skip(i).take_while(|t| is_ch(t, c)).count();
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Ch('[') => {
                if let Some((l, n)) = bracket_at(toks, i) {
                    let value = match l {
                        'd' => total_seconds / 86_400,
                        'h' if largest == Some('h') => total_seconds / 3600,
                        'h' => (total_seconds % 86_400) / 3600,
                        'm' if largest == Some('m') => total_seconds / 60,
                        'm' => (total_seconds % 3600) / 60,
                        _ if largest == Some('s') => total_seconds,
                        _ => total_seconds % 60,
                    };
                    out.push_str(&if n == 2 { format!("{value:02}") } else { value.to_string() });
                    i += n + 2;
                    continue;
                }
                out.push('[');
            }
            Tok::Ch('d' | 'D') => {
                let n = toks.iter().skip(i).take_while(|t| matches!(t, Tok::Ch('d' | 'D'))).count();
                date_day(&mut out, dt, n);
                i += n;
                continue;
            }
            Tok::Ch('w') => {
                let n = run('w');
                out.push_str(&if n >= 2 { weekday_name(dt.weekday()).to_owned() } else { short(weekday_name(dt.weekday())) });
                i += n;
                continue;
            }
            Tok::Ch('M') => {
                let n = run('M');
                let name = month_name(dt);
                out.push_str(&match n {
                    1 => dt.month().to_string(),
                    2 => two(dt.month()),
                    3 => short(name),
                    _ => name.to_owned(),
                });
                i += n;
                continue;
            }
            Tok::Ch('y' | 'Y') => {
                let n = toks.iter().skip(i).take_while(|t| matches!(t, Tok::Ch('y' | 'Y'))).count();
                let year = dt.year();
                out.push_str(&if n >= 3 { format!("{year}") } else { two(year.rem_euclid(100)) });
                i += n;
                continue;
            }
            Tok::Ch('h') => {
                let n = run('h');
                out.push_str(&if n >= 2 { two(hour12(dt.hour())) } else { hour12(dt.hour()).to_string() });
                i += n;
                continue;
            }
            Tok::Ch('H') => {
                let n = run('H');
                out.push_str(&if n >= 2 { two(dt.hour()) } else { dt.hour().to_string() });
                i += n;
                continue;
            }
            Tok::Ch('m') => {
                let n = run('m');
                out.push_str(&if n >= 2 { two(dt.minute()) } else { dt.minute().to_string() });
                i += n;
                continue;
            }
            Tok::Ch('s') => {
                let n = run('s');
                out.push_str(&if n >= 2 { two(dt.second()) } else { dt.second().to_string() });
                i += n;
                continue;
            }
            Tok::Ch('t') => {
                let n = run('t');
                let am = dt.hour() < 12;
                out.push_str(match (n >= 2, am) {
                    (true, true) => "AM",
                    (true, false) => "PM",
                    (false, true) => "A",
                    (false, false) => "P",
                });
                i += n;
                continue;
            }
            Tok::Ch('a' | 'A') if am_pm_at(toks, i) => {
                out.push_str(if dt.hour() < 12 { "AM" } else { "PM" });
                i += 5;
                continue;
            }
            Tok::Ch('c') => {
                out.push_str(&format!("{}/{}/{}", dt.month(), dt.day(), dt.year()));
                if dt.hour() != 0 || dt.minute() != 0 || dt.second() != 0 {
                    out.push(' ');
                    out.push_str(&time_part(dt));
                }
            }
            Tok::Ch('C') => {
                out.push_str(&long_date(dt));
                out.push(' ');
                out.push_str(&time_part(dt));
            }
            Tok::Ch('T') => out.push_str(&time_part(dt)),
            Tok::Ch(c) => out.push(*c),
        }
        i += 1;
    }
    Ok(out)
}

/// `[h]`, `[mm]` and the like at `i`: the letter in lower case and how many times it is written (one or two).
fn bracket_at(toks: &[Tok], i: usize) -> Option<(char, usize)> {
    if !toks.get(i).is_some_and(|t| is_ch(t, '[')) {
        return None;
    }
    let Some(Tok::Ch(c)) = toks.get(i + 1) else { return None };
    let letter = c.to_ascii_lowercase();
    if !"dhms".contains(letter) {
        return None;
    }
    let n = toks.iter().skip(i + 1).take_while(|t| matches!(t, Tok::Ch(x) if x.to_ascii_lowercase() == letter)).count();
    ((1..=2).contains(&n) && toks.get(i + 1 + n).is_some_and(|t| is_ch(t, ']'))).then_some((letter, n))
}

/// `am/pm` in any letter case, written as five tokens.
fn am_pm_at(toks: &[Tok], i: usize) -> bool {
    let word: String = toks.iter().skip(i).take(5).filter_map(|t| if let Tok::Ch(c) = t { Some(c.to_ascii_lowercase()) } else { None }).collect();
    word == "am/pm"
}

fn time_part(dt: DateTime) -> String {
    format!("{}:{:02}:{:02} {}", hour12(dt.hour()), dt.minute(), dt.second(), if dt.hour() < 12 { "AM" } else { "PM" })
}

fn long_date(dt: DateTime) -> String {
    let month = month_name(dt);
    format!("{}, {month} {:02}, {}", weekday_name(dt.weekday()), dt.day(), dt.year())
}

fn date_day(out: &mut String, dt: DateTime, n: usize) {
    match n {
        1 => out.push_str(&dt.day().to_string()),
        2 => out.push_str(&two(dt.day())),
        3 => out.push_str(&short(weekday_name(dt.weekday()))),
        4 => out.push_str(weekday_name(dt.weekday())),
        5 => out.push_str(&format!("{}/{}/{}", dt.month(), dt.day(), dt.year())),
        _ => out.push_str(&long_date(dt)),
    }
}
