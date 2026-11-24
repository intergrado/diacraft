//! Dates, times and currency: `DATE`, `DATETIME`, `DATEVALUE`, `TIME`, `TIMEVALUE`, the parts of a date, the clock and the document's dates,
//! and `CY`.
//!
//! A date is one number, whole days from 30 December 1899 and the part of the day gone, and every calendar step goes through
//! `jiff` (`AGENTS.md`). A string is read under en-US rules whatever the file's language (project rule). The clock and the zone
//! are inputs ([`crate::Clock`]): nothing here reads the machine, so the web build compiles it, and `NOW` and the document dates are
//! unsupported where there is none.

use diacraft_formula::{currency_by_code, currency_by_id};
use jiff::civil::{Date, DateTime, Time};
use jiff::{SignedDuration, Span};

use crate::convert::{signed_int, signed_long};
use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::store::{EPOCH, datetime_to_serial, serial_to_datetime};
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

fn value_error() -> Value {
    Value::Error(CellError::Value)
}

/// What a date string said: a date (a year only when it was written), and a time.
struct Parsed {
    /// Year if written, month, day.
    date: Option<(Option<i32>, u32, u32)>,
    time: Option<Time>,
}

/// A date from a year, a month and a day as the parser has them: `None` for one the calendar does not have, which includes a year past 9999.
fn ymd(year: i32, month: u32, day: u32) -> Option<Date> {
    Date::new(i16::try_from(year).ok()?, i8::try_from(month).ok()?, i8::try_from(day).ok()?).ok()
}

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

/// The month a word names: the full name or its first three letters, with `sept` also.
fn month_of(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    MONTHS.iter().position(|m| *m == w || (w.len() >= 3 && m.starts_with(w.as_str()) && w.len() <= m.len())).map(|i| i as u32 + 1)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(String),
    Word(String),
    Punct(char),
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut n = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    n.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() {
            let mut w = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_alphabetic() {
                    w.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Word(w));
        } else {
            chars.next();
            if matches!(c, '/' | '-' | ':' | ',' | '.') {
                out.push(Tok::Punct(c));
            }
        }
    }
    out
}

fn num(t: &Tok) -> Option<u32> {
    match t {
        Tok::Num(n) if n.len() <= 9 => n.parse().ok(),
        _ => None,
    }
}

fn ampm(t: &Tok) -> Option<bool> {
    match t {
        Tok::Word(w) => match w.to_ascii_lowercase().as_str() {
            "a" | "am" => Some(false),
            "p" | "pm" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// A year as written: two digits are 1930 to 2029.
fn year_of(text: &str, value: u32) -> Option<i32> {
    let v = i32::try_from(value).ok()?;
    Some(if text.len() <= 2 { if v < 30 { 2000 + v } else { 1900 + v } } else { v })
}

/// The time in a token list, removed from it: `h:mm`, `h:mm:ss`, each with an optional `a`, `am`, `p` or `pm`, or an hour and one of those
/// `Err` for a time that is not one (a minute past 59).
fn take_time(toks: &mut Vec<Tok>) -> Result<Option<Time>, ()> {
    let mut at = None;
    for i in 0..toks.len() {
        if num(&toks[i]).is_some() && matches!(toks.get(i + 1), Some(Tok::Punct(':'))) && toks.get(i + 2).and_then(num).is_some() {
            at = Some(i);
            break;
        }
        if num(&toks[i]).is_some() && toks.get(i + 1).and_then(ampm).is_some() {
            at = Some(i);
            break;
        }
    }
    let Some(i) = at else { return Ok(None) };
    let hour = toks.get(i).and_then(num).ok_or(())?;
    let mut end = i + 1;
    let (mut minute, mut second) = (0, 0);
    if matches!(toks.get(end), Some(Tok::Punct(':'))) {
        minute = toks.get(end + 1).and_then(num).ok_or(())?;
        end += 2;
        if matches!(toks.get(end), Some(Tok::Punct(':'))) && toks.get(end + 1).and_then(num).is_some() {
            second = toks.get(end + 1).and_then(num).ok_or(())?;
            end += 2;
        }
    }
    let mark = toks.get(end).and_then(ampm);
    if mark.is_some() {
        end += 1;
    }
    toks.drain(i..end);
    let hour = match mark {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return Err(());
            }
            (hour % 12) + if pm { 12 } else { 0 }
        }
        None => hour,
    };
    let (Ok(hour), Ok(minute), Ok(second)) = (i8::try_from(hour), i8::try_from(minute), i8::try_from(second)) else { return Err(()) };
    Time::new(hour, minute, second, 0).map(Some).map_err(|_| ())
}

/// A date and a time from a string, under en-US; `None` when it does not read.
fn parse_text(text: &str) -> Option<Parsed> {
    let s = text.trim();
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    // ISO 8601 reads the same in every locale.
    let iso = s.strip_suffix('Z').unwrap_or(s);
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(dt) = DateTime::strptime(fmt, iso) {
            return Some(Parsed {
                date: Some((Some(i32::from(dt.year())), u32::from(dt.month().unsigned_abs()), u32::from(dt.day().unsigned_abs()))),
                time: Some(dt.time()),
            });
        }
    }
    let mut toks = tokens(s);
    let time = take_time(&mut toks).ok()?;
    // Drop commas and stray points.
    toks.retain(|t| !matches!(t, Tok::Punct(',' | '.')));
    if toks.is_empty() {
        return time.map(|t| Parsed { date: None, time: Some(t) });
    }
    let words: Vec<&String> = toks.iter().filter_map(|t| if let Tok::Word(w) = t { Some(w) } else { None }).collect();
    let numbers: Vec<&String> = toks.iter().filter_map(|t| if let Tok::Num(n) = t { Some(n) } else { None }).collect();
    let date = if words.is_empty() {
        // `m/d/y`, `m-d-y` or `m/d`, the separators mixed as the page lists them.
        let separators = toks.iter().filter(|t| matches!(t, Tok::Punct('/' | '-'))).count();
        if toks.iter().any(|t| matches!(t, Tok::Punct(':'))) || separators + 1 != numbers.len() || !(2..=3).contains(&numbers.len()) {
            return None;
        }
        let n: Vec<u32> = numbers.iter().map(|x| x.parse().ok()).collect::<Option<_>>()?;
        if numbers.len() == 3 {
            if numbers[0].len() == 4 { (year_of(numbers[0], n[0]), n[1], n[2]) } else { (year_of(numbers[2], n[2]), n[0], n[1]) }
        } else {
            (None, n[0], n[1])
        }
    } else {
        if words.len() != 1 || toks.iter().any(|t| matches!(t, Tok::Punct(':' | '/'))) {
            return None;
        }
        let month = month_of(words[0])?;
        let n: Vec<u32> = numbers.iter().map(|x| x.parse().ok()).collect::<Option<_>>()?;
        match n.as_slice() {
            [] => return None,
            [one] => {
                if *one >= 32 || numbers[0].len() == 4 {
                    (year_of(numbers[0], *one), month, 1)
                } else {
                    (None, month, *one)
                }
            }
            [first, second] => {
                let (day, year_text, year) =
                    if *first <= 31 && numbers[0].len() <= 2 { (*first, numbers[1], *second) } else { (*second, numbers[0], *first) };
                (year_of(year_text, year), month, day)
            }
            _ => return None,
        }
    };
    let (_, month, day) = date;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(Parsed { date: Some(date), time })
}

impl Evaluator<'_, '_> {
    /// A date argument as a serial: a string is read and anything else by `vDouble`. `name` is the function, for the
    /// unsupported value a string that needs the clock gives where there is none.
    fn serial_of(&mut self, site: &Site<'_>, slots: Slots<'_>, i: usize, name: &str) -> Result<f64, Value> {
        let v = crate::convert::slot(slots, i)?;
        if let Value::Text(s) = v {
            return self.parse_serial(s, name);
        }
        Ok(self.read_double(v, site.env)?.stored)
    }

    fn parse_serial(&mut self, s: &str, name: &str) -> Result<f64, Value> {
        // The string is trimmed before it is judged too long to be a date: its length is charged.
        if !self.work(s.len()) {
            return Err(Value::Error(crate::rules::LOOP_ERROR));
        }
        let parsed = parse_text(s).ok_or_else(value_error)?;
        // A string with no year or no date reads the clock, so the cell that holds it is a clock reader whether or not a clock was
        // given (the oracle skips it). A time alone given to a date part reads nothing.
        let time_alone_ok = parsed.date.is_none() && parsed.time.is_some() && !matches!(name, "DATETIME" | "DATEVALUE");
        if !time_alone_ok && !matches!(parsed.date, Some((Some(_), _, _))) {
            self.ctx.clock_touched = true;
        }
        let today = self.ctx.opts.clock.now.map(|n| n.date());
        let date = match parsed.date {
            Some((Some(y), m, d)) => ymd(y, m, d),
            Some((None, m, d)) => match today {
                Some(t) => ymd(i32::from(t.year()), m, d),
                None => return Err(Value::unsupported(name)),
            },
            // A time alone needs no date; the day is the epoch's.
            None if parsed.time.is_some() && !matches!(name, "DATETIME" | "DATEVALUE") => Some(EPOCH),
            None => match today {
                Some(t) => Some(t),
                None => return Err(Value::unsupported(name)),
            },
        };
        let date = date.ok_or_else(value_error)?;
        let time = parsed.time.unwrap_or(Time::midnight());
        Ok(datetime_to_serial(date.to_datetime(time)))
    }

    /// The date and time a serial is, for a part: `#VALUE!` beyond what the calendar holds.
    fn parts_of(&mut self, site: &Site<'_>, slots: Slots<'_>, name: &str) -> Result<DateTime, Value> {
        let serial = self.serial_of(site, slots, 0, name)?;
        serial_to_datetime(serial).ok_or_else(value_error)
    }
}

fn date_value(serial: f64) -> Value {
    Value::date(serial)
}

fn date(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let year = match ev.arg_double(site, slots, 0).and_then(signed_long) {
        Ok(y) => y,
        Err(e) => return e,
    };
    let month = match ev.arg_double(site, slots, 1).and_then(signed_int) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let day = match ev.arg_double(site, slots, 2).and_then(signed_int) {
        Ok(d) => d,
        Err(e) => return e,
    };
    // `vUnsignedInt` wraps a year at 65536; a year of 0 to 29 is 2000 to 2029, 30 to 100 is 1930 to 2000.
    let year = year & 0xFFFF;
    let year = if year <= 29 {
        2000 + year
    } else if year <= 100 {
        1900 + year
    } else {
        year
    };
    let months = year * 12 + (month - 1);
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12) + 1);
    let Some(first) = i32::try_from(y).ok().and_then(|y| ymd(y, m as u32, 1)) else { return Value::Error(CellError::Num) };
    let Some(date) = Span::new().try_days(day - 1).ok().and_then(|days| first.checked_add(days).ok()) else { return Value::Error(CellError::Num) };
    // A year past 9999 is `#NUM!`, here by jiff's range (the first of the month, or the sum of it and the days, has no such year), and so is a date before
    // 1 January 1899; there is none in 1900.
    if date < Date::constant(1899, 1, 1) {
        return Value::Error(CellError::Num);
    }
    date_value(datetime_to_serial(date.to_datetime(Time::midnight())))
}

fn datetime(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.serial_of(site, slots, 0, "DATETIME") {
        Ok(serial) => date_value(serial),
        Err(e) => e,
    }
}

fn datevalue(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.serial_of(site, slots, 0, "DATEVALUE") {
        Ok(serial) => date_value(serial.trunc()),
        Err(e) => e,
    }
}

fn timevalue(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.serial_of(site, slots, 0, "TIMEVALUE") {
        Ok(serial) => date_value((serial - serial.trunc()).abs()),
        Err(e) => e,
    }
}

/// `TIME(hour, minute, second)`: the offsets added up as a fraction of a day, whole days gone. A negative argument is kept, and
/// 65536 or more wraps.
fn time(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let mut total = 0i64;
    for (i, unit) in [(0usize, 3600i64), (1, 60), (2, 1)] {
        let n = match ev.arg_double(site, slots, i).and_then(signed_long) {
            Ok(n) => n,
            Err(e) => return e,
        };
        let n = if n >= 65536 { n % 65536 } else { n };
        total += n * unit;
    }
    let days = total as f64 / 86_400.0;
    date_value(days - days.trunc())
}

fn part(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, name: &str, f: fn(&DateTime) -> f64) -> Value {
    match ev.parts_of(site, slots, name) {
        Ok(dt) => Value::num(f(&dt)),
        Err(e) => e,
    }
}

fn year(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "YEAR", |d| f64::from(d.year()))
}

fn month(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "MONTH", |d| f64::from(d.month()))
}

fn day(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "DAY", |d| f64::from(d.day()))
}

fn dayofyear(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "DAYOFYEAR", |d| f64::from(d.day_of_year()))
}

/// Monday is 1 and Sunday is 7, with no flag to change it.
fn weekday(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "WEEKDAY", |d| f64::from(d.weekday().to_monday_one_offset()))
}

fn hour(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "HOUR", |d| f64::from(d.hour()))
}

fn minute(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "MINUTE", |d| f64::from(d.minute()))
}

fn second(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    part(ev, site, slots, "SECOND", |d| f64::from(d.second()))
}

/// `NOW()`: the wall clock the options carry; unsupported where there is none.
fn now(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    ev.ctx.clock_touched = true;
    match ev.ctx.opts.clock.now {
        Some(n) => date_value(datetime_to_serial(n)),
        None => Value::unsupported("NOW"),
    }
}

/// A document date: the core property in UTC, shifted by the machine's zone to the wall clock; unsupported where the file holds
/// none or there is no zone to ask.
fn doc_date(ev: &mut Evaluator<'_, '_>, name: &str, pick: fn(&diacraft_doc::DocProps) -> Option<&std::sync::Arc<str>>) -> Value {
    ev.ctx.clock_touched = true;
    let Some(text) = pick(&ev.doc.props) else { return Value::unsupported(name) };
    let Some(utc) = parse_text(text).and_then(|p| match (p.date, p.time) {
        (Some((Some(y), m, d)), Some(t)) => ymd(y, m, d).map(|d| d.to_datetime(t)),
        _ => None,
    }) else {
        return Value::unsupported(name);
    };
    let local = utc.checked_add(SignedDuration::from_secs(i64::from(ev.ctx.opts.clock.utc_offset_seconds))).ok();
    match local {
        Some(l) => date_value(datetime_to_serial(l)),
        None => Value::unsupported(name),
    }
}

fn doccreation(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    doc_date(ev, "DOCCREATION", |p| p.created.as_ref())
}

/// `DOCLASTEDIT` and `DOCLASTSAVE` read the same property, `modified`.
fn doclastedit(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    doc_date(ev, "DOCLASTEDIT", |p| p.modified.as_ref())
}

fn doclastsave(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    doc_date(ev, "DOCLASTSAVE", |p| p.modified.as_ref())
}

fn doclastprint(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    doc_date(ev, "DOCLASTPRINT", |p| p.last_printed.as_ref())
}

/// `CY(value [, cyID])`: the amount with a code, a number being an ID (`-1` and no argument the undefined currency) and a string a
/// code with its case folded. An amount beyond a 64-bit count of ten-thousandths is `#VALUE!`.
fn cy(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let amount = match ev.arg_double(site, slots, 0) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    // A 64-bit count of ten-thousandths: 922,337,203,685,477.5807 either way. A double cannot tell the last digits apart, so the limit is the
    // nearest double.
    if amount.abs() >= 922_337_203_685_477.6 {
        return value_error();
    }
    let currency = match slots.get(1) {
        None | Some(None) => None,
        Some(Some(Value::Text(code))) => match currency_by_code(&code.trim().to_ascii_uppercase()) {
            Some(c) => Some(c),
            None => return value_error(),
        },
        Some(Some(other)) => {
            let id = match ev.read_double(other, site.env) {
                Ok(q) => q.stored.trunc(),
                Err(e) => return e,
            };
            if id == -1.0 {
                None
            } else {
                match i32::try_from(id as i64).ok().and_then(currency_by_id) {
                    Some(c) => Some(c),
                    None => return value_error(),
                }
            }
        }
    };
    Value::Currency { amount, currency }
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("CY", 1, 2, cy),
    eager("DATE", 3, 3, date),
    eager("DATETIME", 1, 2, datetime),
    eager("DATEVALUE", 1, 2, datevalue),
    eager("DAY", 1, 2, day),
    eager("DAYOFYEAR", 1, 2, dayofyear),
    eager("DOCCREATION", 0, 0, doccreation),
    eager("DOCLASTEDIT", 0, 0, doclastedit),
    eager("DOCLASTPRINT", 0, 0, doclastprint),
    eager("DOCLASTSAVE", 0, 0, doclastsave),
    eager("HOUR", 1, 2, hour),
    eager("MINUTE", 1, 2, minute),
    eager("MONTH", 1, 2, month),
    eager("NOW", 0, 0, now),
    eager("SECOND", 1, 2, second),
    eager("TIME", 3, 3, time),
    eager("TIMEVALUE", 1, 2, timevalue),
    eager("WEEKDAY", 1, 2, weekday),
    eager("YEAR", 1, 2, year),
];
