//! The text a field shows: the result of the `Value` cell of a `Field` row, put through the row's `Format`
//! picture. The pictures are the ones `FORMAT` reads, so a field shows what `FORMAT(value, picture)` gives.
//!
//! **Project rule (ours; the specs say only that `Format` turns the value into text).** A picture that is empty leaves a text as it is, and a number as plain digits
//! (the form); a date or a duration with no picture is not worked out here (`None`: the caller keeps the text the field already shows), since no document says what the default
//! picture of a date is. A length whose cell names no unit (the drawing's default unit) shows in inches. A value that is an error, a colour, a point or an unsupported function's has no text (`None`).

use diacraft_units::{Quantity, Unit};
use jiff::SignedDuration;
use jiff::civil::DateTime;

use crate::eval::Clock;
use crate::picture::{self, Cast, Locale, Subject};
use crate::store::{datetime_to_serial, serial_to_iso};
use crate::strnum::format_number;
use crate::value::Value;

/// The text `value` shows through the picture `picture` (a `Field` row's `Format`), in the marks of the language `language` (`en-US` when `None`); `None` when it is no text this can say
/// (see the module).
pub fn field_text(value: &Value, picture: &str, language: Option<&str>) -> Option<String> {
    let subject = match value {
        Value::Text(text) => Subject::Text(text.to_string()),
        Value::Number(q) if q.unit == Unit::Date => Subject::Date(q.stored),
        // A length in the drawing's default unit (a cell that no `U` says the unit of) shows in inches, which is what the drawing unit is (ours).
        Value::Number(q) if q.unit == Unit::DrawingDefault => Subject::Number(Quantity::new(q.stored, Unit::Inch)),
        Value::Number(q) => Subject::Number(*q),
        Value::Currency { amount, .. } => Subject::Number(Quantity::new(*amount, Unit::Unitless)),
        _ => return None,
    };
    let picture = picture.trim();
    if picture.is_empty() {
        return match subject {
            Subject::Text(text) => Some(text),
            Subject::Number(q) => Some(format_number(q.stored)),
            Subject::Date(_) => None,
        };
    }
    let marks = language.map_or(Locale::EN_US, Locale::of_tag);
    picture::format(&subject, picture, Cast::NONE, marks).ok()
}

/// What `FIELDPICTURE(code)` gives: the text a `Format` cell holds for a field picture code. Empty for a code that names none.
pub fn field_picture(code: u32) -> String {
    picture::field_picture(code)
}

/// The date and time of `unix_seconds` (since 1970, UTC) on the UTC calendar; `None` past the years the calendar holds.
fn utc_datetime(unix_seconds: i64) -> Option<DateTime> {
    let at = jiff::Timestamp::from_second(unix_seconds).ok()?;
    Some(jiff::tz::TimeZone::UTC.to_datetime(at))
}

/// The clock of a wall time given as seconds since 1970 (UTC) and the machine's offset from UTC in seconds: what `NOW()` reads. `now` is `None` past the years the calendar holds.
pub fn clock_at(unix_seconds: i64, utc_offset_seconds: i32) -> Clock {
    let now = utc_datetime(unix_seconds).and_then(|dt| dt.checked_add(SignedDuration::from_secs(i64::from(utc_offset_seconds))).ok());
    Clock { now, utc_offset_seconds }
}

/// `unix_seconds` as the `YYYY-MM-DDThh:mm:ssZ` text the core properties hold their dates in (`created`, `modified`, `lastPrinted`); `None` past the years the calendar holds.
pub fn utc_iso(unix_seconds: i64) -> Option<String> {
    let iso = serial_to_iso(datetime_to_serial(utc_datetime(unix_seconds)?))?;
    Some(format!("{iso}Z"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn text(s: &str) -> Value {
        Value::Text(Arc::from(s))
    }

    #[test]
    fn a_text_is_itself_with_no_picture_and_a_number_is_plain_digits() {
        assert_eq!(field_text(&text("Ann"), "", None).as_deref(), Some("Ann"));
        assert_eq!(field_text(&text("Ann"), "  ", None).as_deref(), Some("Ann"));
        assert_eq!(field_text(&Value::Number(Quantity::new(3.0, Unit::Unitless)), "", None).as_deref(), Some("3"));
        assert_eq!(field_text(&Value::Number(Quantity::new(2.5, Unit::Unitless)), "", None).as_deref(), Some("2.5"));
    }

    #[test]
    fn a_length_in_the_drawings_default_unit_shows_in_inches() {
        let width = Value::Number(Quantity::new(3.0, Unit::DrawingDefault));
        assert_eq!(field_text(&width, "0.00 u", None), field_text(&Value::Number(Quantity::new(3.0, Unit::Inch)), "0.00 u", None));
        assert!(field_text(&width, "0.00 u", None).is_some_and(|s| s.starts_with("3.00")));
    }

    #[test]
    fn a_picture_shapes_the_value() {
        assert_eq!(field_text(&Value::Number(Quantity::new(12.4321, Unit::Unitless)), "0.00", None).as_deref(), Some("12.43"));
        assert_eq!(field_text(&text("ann"), "@", None).as_deref(), Some("ann"));
        // 2025-01-02 as a serial.
        let serial = crate::iso_to_serial("2025-01-02T00:00:00").expect("a date");
        let date = Value::date(serial);
        assert_eq!(field_text(&date, "yyyy-MM-dd", None).as_deref(), Some("2025-01-02"));
        assert_eq!(field_text(&date, "MMMM d, yyyy", None).as_deref(), Some("January 2, 2025"));
        assert_eq!(field_text(&date, &field_picture(204), None).as_deref(), Some("2025-01-02"));
    }

    #[test]
    fn the_clock_and_the_property_text_of_a_unix_time_are_the_utc_calendar_shifted_by_the_offset() {
        // 2026-01-01T00:00:00Z.
        let seconds = 1_767_225_600;
        assert_eq!(utc_iso(seconds).as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(clock_at(seconds, 0).now.map(|d| d.to_string()).as_deref(), Some("2026-01-01T00:00:00"));
        assert_eq!(clock_at(seconds, 3_600).now.map(|d| d.to_string()).as_deref(), Some("2026-01-01T01:00:00"));
        assert_eq!(clock_at(seconds, -3_600).now.map(|d| d.to_string()).as_deref(), Some("2025-12-31T23:00:00"));
        assert_eq!(clock_at(seconds, 3_600).utc_offset_seconds, 3_600);
        // Past the years the calendar holds there is no time, and no panic.
        assert_eq!(utc_iso(i64::MAX), None);
        assert_eq!(clock_at(i64::MIN, 0).now, None);
    }

    #[test]
    fn a_date_with_no_picture_and_a_value_with_no_text_say_nothing() {
        let date = Value::date(crate::iso_to_serial("2025-01-02T00:00:00").expect("a date"));
        assert_eq!(field_text(&date, "", None), None);
        assert_eq!(field_text(&Value::Error(crate::CellError::Na), "0", None), None);
        assert_eq!(field_text(&Value::unsupported("NOW"), "yyyy", None), None);
        // A picture that is not one is no text, not a panic.
        assert_eq!(field_text(&text("x"), &"0".repeat(5_000), None), None);
    }
}
