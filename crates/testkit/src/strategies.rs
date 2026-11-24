//! Generators of hostile input.

use serde_json::{Value, json};

/// Hostile JSON values: the wrong type for most parameters, numbers at and past the edges of every
/// integer width, text that is blank, huge, multi-byte or made of control characters, and empty,
/// odd and nested containers. NaN and infinity have no JSON form (they become `null`).
pub fn junk_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(false),
        json!(-1),
        json!(0),
        json!(1),
        json!(0.5),
        json!(-1e9),
        json!(1e12),
        json!(1e308),
        json!(-1e308),
        json!(f64::MIN_POSITIVE),
        json!(u32::MAX),
        json!(u64::from(u32::MAX) + 1),
        json!(u64::MAX),
        json!(i64::MIN),
        json!(""),
        json!(" "),
        json!("junk"),
        json!("#zzzzzz"),
        json!("tab\there"),
        json!("two\nlines"),
        json!("nul\u{0}byte"),
        json!("😀 outside the BMP"),
        json!("\u{202e}right to left override"),
        json!("x".repeat(100_000)),
        json!([]),
        json!([1]),
        json!([f64::MAX, -f64::MAX]),
        json!([[1, 2], [3]]),
        json!({}),
        json!({"x": "y"}),
        json!([[[[[]]]]]),
    ]
}
