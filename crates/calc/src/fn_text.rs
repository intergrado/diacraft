//! Strings, formats and text measure: `LEN`, `LEFT`, `RIGHT`, `MID`, `FIND`, `REPLACE`, `SUBSTITUTE`, `TRIM`, `REPT`,
//! `UPPER`, `LOWER`, `CHAR`, `UNICHAR`, `INDEX`, `LOOKUP`, `STRSAME`, `STRSAMEEX`, `FORMAT`, `FORMATEX`, `FIELDPICTURE`, `DECIMALSEP`,
//! `LISTSEP`, `REWIDEN` and `SHAPETEXT`; `TEXTWIDTH` and `TEXTHEIGHT` check their argument and measure the text with `fn_measure.rs`.
//!
//! **What a character is (project rule):** a UTF-16 code unit. A cut between the halves of a pair gives U+FFFD, and a string cannot
//! hold a lone half, so `UNICHAR` of one is U+FFFD too. Every search is linear in the text, every result is capped at
//! [`MAX_TEXT_BYTES`], and every argument is read once.

use std::sync::Arc;

use diacraft_doc::{CellAddr, ShapeRef, SheetRef, TextPart};
use diacraft_formula::Expr;
use diacraft_units::{Quantity, Unit};

use crate::convert::{signed_int, signed_long, slot, unsigned_int};
use crate::eval::{Env, Evaluator, Site};
use crate::picture::{self, Cast, Dst, Locale, PictureFailure, Subject};
use crate::registry::{FnDef, eager, eager_seeing, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_TEXT_BYTES};
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

fn text_value(s: String) -> Value {
    Value::Text(Arc::from(s))
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from_units(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

/// An argument by `vString`, or the value the function must return instead. Its length is charged at once: every function
/// below reads, cuts, searches or builds from its arguments, so a step stands for 64 bytes of them and no more.
fn arg_str(ev: &mut Evaluator<'_, '_>, slots: Slots<'_>, i: usize) -> Result<Arc<str>, Value> {
    let t = ev.arg_text(slots, i)?;
    charge(ev, t.len())?;
    Ok(t)
}

/// A string argument already worked out, by `vString`, charged the same way.
fn read_str(ev: &mut Evaluator<'_, '_>, v: &Value) -> Result<Arc<str>, Value> {
    let t = ev.read_text(v)?;
    charge(ev, t.len())?;
    Ok(t)
}

/// A whole number argument by `vSignedLong`, or `default` when the slot is empty or absent.
fn long_or(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, i: usize, default: i64) -> Result<i64, Value> {
    match slots.get(i).and_then(Option::as_ref) {
        None => Ok(default),
        Some(v) => ev.read_double(v, site.env).and_then(signed_long),
    }
}

fn required_long(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, i: usize) -> Result<i64, Value> {
    let v = slot(slots, i)?;
    ev.read_double(v, site.env).and_then(signed_long)
}

fn flag_or(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, i: usize, default: bool) -> Result<bool, Value> {
    match slots.get(i).and_then(Option::as_ref) {
        None => Ok(default),
        Some(v) => ev.read_bool(v, site.env),
    }
}

/// The locale marks: an LCID if one is given, else the file's `Language`, else en-US.
fn locale(ev: &Evaluator<'_, '_>, lcid: Option<u32>) -> Locale {
    match lcid {
        Some(l) if l != 0 => Locale::of_lcid(l),
        _ => ev.doc.props.language.as_deref().map_or(Locale::EN_US, Locale::of_tag),
    }
}

/// Charges the work of `len` bytes, at [`crate::rules::UNITS_PER_STEP`] a step.
fn charge(ev: &mut Evaluator<'_, '_>, len: usize) -> Result<(), Value> {
    if ev.work(len) { Ok(()) } else { Err(Value::Error(LOOP_ERROR)) }
}

// ------------------------------------------------------------------------------------------------------------------------------
// Length and cuts.

fn len(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    match arg_str(ev, slots, 0) {
        Ok(t) => Value::num(t.encode_utf16().count() as f64),
        Err(e) => e,
    }
}

fn left(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => units(&t),
        Err(e) => return e,
    };
    let n = match long_or(ev, site, slots, 1, 1) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // A count past the end or below zero is all the text.
    let take = if n < 0 || n as usize > t.len() { t.len() } else { n as usize };
    text_value(from_units(t.get(..take).unwrap_or(&[])))
}

fn right(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => units(&t),
        Err(e) => return e,
    };
    let n = match long_or(ev, site, slots, 1, 1) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let take = if n < 0 || n as usize > t.len() { t.len() } else { n as usize };
    text_value(from_units(t.get(t.len() - take..).unwrap_or(&[])))
}

fn mid(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => units(&t),
        Err(e) => return e,
    };
    let start = match required_long(ev, site, slots, 1) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let count = match required_long(ev, site, slots, 2) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // A start of 0 is an error; below 0 or past the end, empty text.
    if start == 0 {
        return Value::Error(CellError::Value);
    }
    if start < 0 || start as usize > t.len() {
        return text_value(String::new());
    }
    let from = start as usize - 1;
    let end = if count < 0 { t.len() } else { from.saturating_add(count as usize).min(t.len()) };
    text_value(from_units(t.get(from..end).unwrap_or(&[])))
}

fn replace(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => units(&t),
        Err(e) => return e,
    };
    let start = match required_long(ev, site, slots, 1) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let count = match required_long(ev, site, slots, 2) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let new = match arg_str(ev, slots, 3) {
        Ok(t) => units(&t),
        Err(e) => return e,
    };
    let mut out: Vec<u16> = Vec::new();
    if start <= 0 || start as usize > t.len() {
        out.extend_from_slice(&t);
        out.extend_from_slice(&new);
    } else {
        let from = start as usize - 1;
        out.extend_from_slice(t.get(..from).unwrap_or(&[]));
        out.extend_from_slice(&new);
        if count >= 0 && from.saturating_add(count as usize) <= t.len() {
            out.extend_from_slice(t.get(from + count as usize..).unwrap_or(&[]));
        }
    }
    if let Err(e) = charge(ev, out.len() * 2) {
        return e;
    }
    let result = from_units(&out);
    if result.len() > MAX_TEXT_BYTES {
        return Value::Error(CellError::Value);
    }
    text_value(result)
}

fn rept(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let n = match required_long(ev, site, slots, 1) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // A negative count is `#VALUE!`; the result is capped so no file can exhaust memory.
    if n < 0 {
        return Value::Error(CellError::Value);
    }
    match t.len().checked_mul(n as usize) {
        Some(bytes) if bytes <= MAX_TEXT_BYTES => {
            if let Err(e) = charge(ev, bytes) {
                return e;
            }
            text_value(t.repeat(n as usize))
        }
        _ => Value::Error(CellError::Value),
    }
}

fn is_trim_space(c: char) -> bool {
    matches!(c, '\t' | '\u{a}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn trim(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut out = String::with_capacity(t.len());
    let mut pending_space = false;
    for c in t.chars() {
        if is_trim_space(c) {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    text_value(out)
}

fn upper(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    match arg_str(ev, slots, 0) {
        Ok(t) => text_value(picture::to_upper(&t)),
        Err(e) => e,
    }
}

fn lower(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    match arg_str(ev, slots, 0) {
        Ok(t) => text_value(picture::to_lower(&t)),
        Err(e) => e,
    }
}

// ------------------------------------------------------------------------------------------------------------------------------
// Searching: linear in the text (Knuth, Morris and Pratt), so a long needle in a long text costs their sum.

fn find_chars(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    let mut fail = vec![0usize; needle.len()];
    let mut k = 0;
    for i in 1..needle.len() {
        while k > 0 && needle.get(i) != needle.get(k) {
            k = fail.get(k - 1).copied().unwrap_or(0);
        }
        if needle.get(i) == needle.get(k) {
            k += 1;
        }
        if let Some(f) = fail.get_mut(i) {
            *f = k;
        }
    }
    let mut k = 0;
    for (i, c) in hay.iter().enumerate().skip(from) {
        while k > 0 && needle.get(k) != Some(c) {
            k = fail.get(k - 1).copied().unwrap_or(0);
        }
        if needle.get(k) == Some(c) {
            k += 1;
        }
        if k == needle.len() {
            return Some(i + 1 - k);
        }
    }
    None
}

fn fold_chars(s: &str, ignore_case: bool) -> Vec<char> {
    if ignore_case { s.chars().map(picture::lower_char).collect() } else { s.chars().collect() }
}

/// The index in characters of the UTF-16 unit `unit`, rounding a place inside a pair up to the next character; `None` past the end.
fn char_at_unit(chars: &[char], unit: usize) -> Option<usize> {
    let mut u = 0;
    for (i, c) in chars.iter().enumerate() {
        if u >= unit {
            return Some(i);
        }
        u += c.len_utf16();
    }
    (u >= unit).then_some(chars.len())
}

fn unit_of_char(chars: &[char], index: usize) -> usize {
    chars.iter().take(index).map(|c| c.len_utf16()).sum()
}

fn find(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let needle = match arg_str(ev, slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let within = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let start = match slots.get(2).and_then(Option::as_ref) {
        None => Ok(1),
        Some(v) => ev.read_double(v, site.env).and_then(unsigned_int).map(i64::from),
    };
    let start = match start {
        Ok(n) => n,
        Err(e) => return e,
    };
    let ignore_case = match flag_or(ev, site, slots, 3, false) {
        Ok(b) => b,
        Err(e) => return e,
    };
    if let Err(e) = charge(ev, needle.len() + within.len()) {
        return e;
    }
    // A start of 0 is an error.
    if start == 0 {
        return Value::Error(CellError::Value);
    }
    let hay = fold_chars(&within, ignore_case);
    let pin = fold_chars(&needle, ignore_case);
    let units_total: usize = hay.iter().map(|c| c.len_utf16()).sum();
    if pin.is_empty() {
        // An empty text is found at the start, or at the length plus 1 when the start is past the end.
        return Value::num(start.min(units_total as i64 + 1) as f64);
    }
    let Some(from) = char_at_unit(&hay, start as usize - 1) else { return Value::Error(CellError::Value) };
    match find_chars(&hay, &pin, from) {
        Some(at) => Value::num((unit_of_char(&hay, at) + 1) as f64),
        None => Value::Error(CellError::Value),
    }
}

fn substitute(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let t = match arg_str(ev, slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let old = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let new = match arg_str(ev, slots, 2) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let which = match slots.get(3).and_then(Option::as_ref) {
        None => Ok(None),
        Some(v) => ev.read_double(v, site.env).and_then(signed_int).map(Some),
    };
    let which = match which {
        Ok(w) => w,
        Err(e) => return e,
    };
    let ignore_case = match flag_or(ev, site, slots, 4, false) {
        Ok(b) => b,
        Err(e) => return e,
    };
    if let Err(e) = charge(ev, t.len() + old.len()) {
        return e;
    }
    // `which` of 0 is an error. A negative one, and an empty `old`, change nothing.
    if which == Some(0) {
        return Value::Error(CellError::Value);
    }
    if which.is_some_and(|w| w < 0) || old.is_empty() {
        return Value::Text(t);
    }
    let original: Vec<char> = t.chars().collect();
    let hay = fold_chars(&t, ignore_case);
    let pin = fold_chars(&old, ignore_case);
    let mut out = String::with_capacity(t.len());
    let (mut at, mut seen, mut copied) = (0usize, 0i64, 0usize);
    while let Some(found) = find_chars(&hay, &pin, at) {
        seen += 1;
        at = found + pin.len();
        if which.is_none() || which == Some(seen) {
            out.extend(original.get(copied..found).unwrap_or(&[]));
            out.push_str(&new);
            copied = at;
            if which.is_some() {
                break;
            }
        }
        if out.len() > MAX_TEXT_BYTES {
            return Value::Error(CellError::Value);
        }
        if which.is_some_and(|w| seen > w) {
            break;
        }
    }
    out.extend(original.get(copied..).unwrap_or(&[]));
    if out.len() > MAX_TEXT_BYTES {
        return Value::Error(CellError::Value);
    }
    text_value(out)
}

// ------------------------------------------------------------------------------------------------------------------------------
// Lists.

fn delimiter(ev: &mut Evaluator<'_, '_>, slots: Slots<'_>, i: usize) -> Result<Arc<str>, Value> {
    match slots.get(i).and_then(Option::as_ref) {
        None => Ok(Arc::from(";")),
        Some(v) => read_str(ev, v),
    }
}

fn index(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    // `errorvalue` may be an error that is the result: so this function sees the failures, and gives back those of the arguments it reads.
    for i in 0..3 {
        if let Some(Some(v)) = slots.get(i)
            && v.is_failure()
        {
            return v.clone();
        }
    }
    let position = match slots.first().and_then(Option::as_ref) {
        Some(v) => match ev.read_double(v, site.env).and_then(signed_int) {
            Ok(n) => n,
            Err(e) => return e,
        },
        None => return bad(),
    };
    let list = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let delim = match delimiter(ev, slots, 2) {
        Ok(d) => d,
        Err(e) => return e,
    };
    if let Err(e) = charge(ev, list.len()) {
        return e;
    }
    let item = if position < 0 {
        None
    } else if delim.is_empty() {
        (position == 0).then(|| list.to_string())
    } else {
        list.split(&*delim).nth(position as usize).map(str::to_owned)
    };
    match item {
        Some(s) => text_value(s),
        None => match slots.get(3).and_then(Option::as_ref) {
            Some(Value::Error(e)) => Value::Error(*e),
            Some(v @ Value::Unsupported(_)) => v.clone(),
            Some(v) => match read_str(ev, v) {
                Ok(t) => Value::Text(t),
                Err(e) => e,
            },
            None => text_value(String::new()),
        },
    }
}

fn lookup(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    let key = match arg_str(ev, slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let list = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let delim = match delimiter(ev, slots, 2) {
        Ok(d) => d,
        Err(e) => return e,
    };
    if let Err(e) = charge(ev, list.len()) {
        return e;
    }
    // A key holding the delimiter finds nothing. Case is ignored, and so are the spaces around a key or an item.
    if !delim.is_empty() && key.contains(&*delim) {
        return Value::num(-1.0);
    }
    let wanted = picture::to_lower(key.trim());
    let found = if delim.is_empty() {
        (picture::to_lower(list.trim()) == wanted).then_some(0)
    } else {
        list.split(&*delim).position(|item| picture::to_lower(item.trim()) == wanted)
    };
    Value::num(found.map_or(-1.0, |i| i as f64))
}

// ------------------------------------------------------------------------------------------------------------------------------
// Equality.

fn strsame(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    // Project rule: an error argument gives FALSE; an unsupported function gives an unknown.
    for i in 0..2 {
        match slots.get(i).and_then(Option::as_ref) {
            Some(Value::Error(_)) => return Value::Bool(false),
            Some(v @ Value::Unsupported(_)) => return v.clone(),
            _ => {}
        }
    }
    let (a, b) = match (arg_str(ev, slots, 0), arg_str(ev, slots, 1)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let ignore_case = match flag_or(ev, site, slots, 2, false) {
        Ok(b) => b,
        Err(e) => return e,
    };
    Value::Bool(if ignore_case { picture::to_lower(&a) == picture::to_lower(&b) } else { a == b })
}

fn strsameex(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let (a, b) = match (arg_str(ev, slots, 0), arg_str(ev, slots, 1)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let lcid = match required_long(ev, site, slots, 2) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let flags = match required_long(ev, site, slots, 3) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // A bad locale or flag is FALSE, not an error. A locale is 0 (invariant) or a 16-bit id with a primary language.
    let locale_ok = lcid == 0 || ((1..=0xFFFF).contains(&lcid) && lcid & 0x3FF != 0);
    const KNOWN: i64 = 1 | 2 | 4 | 4096 | 0x10000 | 0x20000;
    if !locale_ok || flags < 0 || flags & !KNOWN != 0 {
        return Value::Bool(false);
    }
    // Non-spacing marks, kana type and width need Unicode tables this crate does not hold.
    if flags & (2 | 0x10000 | 0x20000) != 0 {
        return Value::unsupported("STRSAMEEX");
    }
    let prepare = |s: &str| -> String {
        let s = if flags & 1 != 0 { picture::to_lower(s) } else { s.to_owned() };
        if flags & 4 != 0 { s.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect() } else { s }
    };
    Value::Bool(prepare(&a) == prepare(&b))
}

// ------------------------------------------------------------------------------------------------------------------------------
// Characters.

/// Windows-1252 (project rule): Latin-1 apart from 128 to 159.
fn cp1252(code: u16) -> char {
    const HIGH: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018,
        0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    let c = if (128..160).contains(&code) { HIGH.get(usize::from(code) - 128).copied().unwrap_or(u32::from(code)) } else { u32::from(code) };
    char::from_u32(c).unwrap_or('\u{fffd}')
}

fn char_fn(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let n = match slot(slots, 0).and_then(|v| ev.read_double(v, site.env)).and_then(unsigned_int) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // Below 1 or above 255 is an error; a wrapped number counts as the wrapped one.
    if !(1..=255).contains(&n) {
        return Value::Error(CellError::Value);
    }
    text_value(cp1252(n).to_string())
}

fn unichar(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let n = match slot(slots, 0).and_then(|v| ev.read_double(v, site.env)).and_then(unsigned_int) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if n == 0 {
        return Value::Error(CellError::Value);
    }
    // A lone surrogate half cannot be held by a string: U+FFFD.
    text_value(char::from_u32(u32::from(n)).unwrap_or('\u{fffd}').to_string())
}

fn decimalsep(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    text_value(locale(ev, None).decimal.to_string())
}

fn listsep(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    // Project rule: a comma, which parses in any locale.
    text_value(",".to_owned())
}

fn rewiden(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    // Project rule: the text unchanged.
    if let Err(e) = arg_str(ev, slots, 0).and_then(|_| arg_str(ev, slots, 1)) {
        return e;
    }
    match arg_str(ev, slots, 2) {
        Ok(t) => Value::Text(t),
        Err(e) => e,
    }
}

// ------------------------------------------------------------------------------------------------------------------------------
// Pictures.

/// The value as a picture's subject.
fn subject(ev: &mut Evaluator<'_, '_>, v: &Value) -> Result<Subject, Value> {
    Ok(match v {
        Value::Number(q) if q.unit == Unit::Date => Subject::Date(q.stored),
        Value::Number(q) => Subject::Number(*q),
        Value::Currency { amount, .. } => Subject::Number(Quantity::new(*amount, Unit::Unitless)),
        Value::Text(s) => {
            charge(ev, s.len())?;
            Subject::Text(s.to_string())
        }
        other => Subject::Text(read_str(ev, other)?.to_string()),
    })
}

fn failure(f: PictureFailure, name: &str) -> Value {
    match f {
        PictureFailure::Error(e) => Value::Error(e),
        PictureFailure::Unsupported => Value::unsupported(name),
    }
}

fn format_fn(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: Slots<'_>) -> Value {
    let value = match slot(slots, 0) {
        Ok(v) => v.clone(),
        Err(e) => return e,
    };
    let picture = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let subject = match subject(ev, &value) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let marks = locale(ev, None);
    match picture::format(&subject, &picture, Cast::NONE, marks) {
        Ok(s) => match charge(ev, s.len() + picture.len()) {
            Ok(()) => text_value(s),
            Err(e) => e,
        },
        Err(f) => failure(f, "FORMAT"),
    }
}

fn formatex(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let value = match slot(slots, 0) {
        Ok(v) => v.clone(),
        Err(e) => return e,
    };
    let picture = match arg_str(ev, slots, 1) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let unit_arg = |ev: &mut Evaluator<'_, '_>, i: usize| -> Result<Option<Unit>, Value> {
        match slots.get(i).and_then(Option::as_ref) {
            None => Ok(None),
            Some(v) => {
                let t = read_str(ev, v)?;
                picture::parse_cast_unit(&t).map_err(|f| failure(f, "FORMATEX"))
            }
        }
    };
    let src = match unit_arg(ev, 2) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let dst = match unit_arg(ev, 3) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let lcid = match long_or(ev, site, slots, 4, 0) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // `calID` is read for its errors; only the Western calendar is worked.
    if let Err(e) = long_or(ev, site, slots, 5, 0) {
        return e;
    }
    let subject = match subject(ev, &value) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let marks = locale(ev, u32::try_from(lcid).ok());
    let cast = Cast { src, dst: dst.map_or(Dst::Own, Dst::Unit) };
    match picture::format(&subject, &picture, cast, marks) {
        Ok(s) => match charge(ev, s.len() + picture.len()) {
            Ok(()) => text_value(s),
            Err(e) => e,
        },
        Err(f) => failure(f, "FORMATEX"),
    }
}

fn fieldpicture(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match slot(slots, 0).and_then(|v| ev.read_double(v, site.env)).and_then(signed_long) {
        Ok(n) if n >= 0 => text_value(picture::field_picture(n as u32)),
        Ok(_) => text_value(String::new()),
        Err(e) => e,
    }
}

// ------------------------------------------------------------------------------------------------------------------------------
// The shape's text.

impl Evaluator<'_, '_> {
    /// The cell a reference points at, for a function that wants a cell and not its value.
    fn target_of(&mut self, e: &Expr, env: &Env) -> Result<Option<(SheetRef, CellAddr)>, Value> {
        match e {
            Expr::Ref(r) => self.locate(r, env).map(Some),
            _ => Ok(None),
        }
    }
}

/// The characters of a shape's own text (`SHAPETEXT`, `EVALTEXT`), with each part charged a step and the size capped. Two cases are not
/// worked, and the call is `Unsupported` so that the saved value stands (the round, I5): a text that holds a field, whose
/// expansion is the field row's own value through its picture (no corner case, and not done here), unless the caller leaves fields
/// out (`SHAPETEXT` flag 2); and a text the shape inherits from its master, which a page shape shows with its own field values.
pub(crate) fn own_text(ev: &mut Evaluator<'_, '_>, at: &ShapeRef, skip_fields: bool, name: &str) -> Result<String, Value> {
    let Some(shape) = ev.res.shape(at) else { return Ok(String::new()) };
    let Some(text) = shape.text.as_ref() else {
        let inherited = matches!(ev.res.master_step(at), Ok(Some(step)) if ev.res.shape(&step.shape).is_some_and(|m| m.text.is_some()));
        return if inherited { Err(Value::unsupported(name)) } else { Ok(String::new()) };
    };
    let mut raw = String::new();
    for part in &text.parts {
        if !ev.steps(1) {
            return Err(Value::Error(LOOP_ERROR));
        }
        match part {
            TextPart::Chars(c) => raw.push_str(c),
            TextPart::Fld { .. } if skip_fields => {}
            TextPart::Fld { .. } => return Err(Value::unsupported(name)),
            _ => {}
        }
        if raw.len() > MAX_TEXT_BYTES {
            return Err(Value::Error(CellError::Value));
        }
    }
    charge(ev, raw.len())?;
    Ok(raw)
}

fn shapetext(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(arg) = site.arg(0) else { return bad() };
    let target = match ev.target_of(arg, site.env) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let flags = match site.arg(1) {
        None => 0,
        Some(e) => {
            let v = ev.eval(e, site.env);
            match ev.read_double(&v, site.env).and_then(unsigned_int) {
                Ok(f) => f,
                Err(e) => return e,
            }
        }
    };
    // Anything but `TheText` is empty text.
    let Some((sheet, addr)) = target else { return text_value(String::new()) };
    if addr.cell.as_str() != "TheText" {
        return text_value(String::new());
    }
    let SheetRef::Shape(at) = &sheet else { return text_value(String::new()) };
    let raw = match own_text(ev, at, flags & 2 != 0, "SHAPETEXT") {
        Ok(r) => r,
        Err(v) => return v,
    };
    let shown = apply_flags(&raw, flags);
    // Flag 8 turns a tab into eight spaces, so a text of tabs grows.
    if shown.len() > MAX_TEXT_BYTES {
        return Value::Error(CellError::Value);
    }
    text_value(shown)
}

/// `SHAPETEXT`'s operations on the stored text.
fn apply_flags(raw: &str, flags: u16) -> String {
    // The final line feed goes; a line end inside is a carriage return and a line feed, or a space under flag 16.
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    let mut out = String::with_capacity(body.len() + 8);
    let mut prev = '\0';
    for c in body.chars() {
        match c {
            '\n' if flags & 16 != 0 => {
                if prev != '\r' {
                    out.push(' ');
                }
            }
            '\n' if prev != '\r' => out.push_str("\r\n"),
            '\r' if flags & 16 != 0 => out.push(' '),
            '\t' if flags & 4 != 0 => out.push(' '),
            '\t' if flags & 8 != 0 => out.push_str("        "),
            '\u{2018}' | '\u{2019}' if flags & 32 != 0 => out.push('\''),
            '\u{201c}' | '\u{201d}' if flags & 32 != 0 => out.push('"'),
            '\u{ad}' if flags & 1 == 0 => {}
            other => out.push(other),
        }
        prev = c;
    }
    if flags & 64 != 0 {
        let mut collapsed = String::with_capacity(out.len());
        let mut in_run = false;
        for c in out.chars() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                if !in_run {
                    collapsed.push(' ');
                }
                in_run = true;
            } else {
                in_run = false;
                collapsed.push(c);
            }
        }
        return collapsed;
    }
    out
}

/// `TEXTWIDTH` and `TEXTHEIGHT`: the argument must be a `TheText` cell; the text of the shape that holds it is
/// then measured (`fn_measure.rs`), within `maximumWidth` when one is given.
fn text_measure(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, which: crate::fn_measure::Measure) -> Value {
    let Some(arg) = site.arg(0) else { return Value::Error(CellError::Value) };
    let sheet = match ev.target_of(arg, site.env) {
        Ok(Some((sheet, addr))) if addr.cell.as_str() == "TheText" && addr.section.is_none() => sheet,
        Ok(_) => return Value::Error(CellError::Value),
        Err(e) => return e,
    };
    let mut limit = None;
    if let Some(width) = site.arg(1) {
        let v = ev.eval(width, site.env);
        if v.is_failure() {
            return v;
        }
        match ev.read_double(&v, site.env) {
            Ok(q) if q.stored.is_finite() => limit = Some(q.stored),
            Ok(_) => return bad(),
            Err(e) => return e,
        }
    }
    crate::fn_measure::measure(ev, &sheet, limit, which)
}

fn textwidth(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    text_measure(ev, site, crate::fn_measure::Measure::Width)
}

fn textheight(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    text_measure(ev, site, crate::fn_measure::Measure::Height)
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("CHAR", 1, 1, char_fn),
    eager("DECIMALSEP", 0, 0, decimalsep),
    eager("FIELDPICTURE", 1, 1, fieldpicture),
    eager("FIND", 2, 4, find),
    eager("FORMAT", 2, 2, format_fn),
    eager("FORMATEX", 2, 6, formatex),
    eager_seeing("INDEX", 2, 4, index),
    eager("LEFT", 1, 2, left),
    eager("LEN", 1, 1, len),
    eager("LISTSEP", 0, 0, listsep),
    eager("LOOKUP", 2, 3, lookup),
    eager("LOWER", 1, 1, lower),
    eager("MID", 3, 3, mid),
    eager("REPLACE", 4, 4, replace),
    eager("REPT", 2, 2, rept),
    eager("REWIDEN", 3, 3, rewiden),
    eager("RIGHT", 1, 2, right),
    lazy("SHAPETEXT", 1, 2, shapetext),
    eager_seeing("STRSAME", 2, 3, strsame),
    eager("STRSAMEEX", 4, 4, strsameex),
    eager("SUBSTITUTE", 3, 5, substitute),
    lazy("TEXTHEIGHT", 1, 2, textheight),
    lazy("TEXTWIDTH", 1, 2, textwidth),
    eager("TRIM", 1, 1, trim),
    eager("UNICHAR", 1, 1, unichar),
    eager("UPPER", 1, 1, upper),
];
