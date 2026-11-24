//! Reading the params of the editing commands (`shape.*`, `connector.*`): numbers, shape ids, pages and the ends of a connector.
//!
//! Params are hostile (`AGENTS.md`): a number is finite, an array is capped, and an object holds only the keys that are named.

use diacraft_doc::Document;
use serde_json::Value;

use crate::draw::{self, End};
use crate::error::{Result, bad};

/// Most shape ids one command takes.
pub(super) const MAX_IDS: usize = 4_096;

/// The number `p` gives for `key`, which it must give: finite.
pub(super) fn required_number(p: &Value, key: &str, cmd: &str) -> Result<f64> {
    number_of(p.get(key), key, cmd)?.ok_or_else(|| bad(cmd, format!("missing `{key}`")))
}

/// The number `p` gives for `key`, if it gives one: finite.
pub(super) fn optional_number(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    number_of(p.get(key), key, cmd)
}

fn number_of(value: Option<&Value>, key: &str, cmd: &str) -> Result<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(bad(cmd, format!("`{key}` must be a number"))),
        },
    }
}

fn whole_of(value: &Value, key: &str, cmd: &str) -> Result<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from 0 to {}", u32::MAX)))
}

/// The whole number `p` gives for `key`, if it gives one.
pub(super) fn optional_whole(p: &Value, key: &str, cmd: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => whole_of(v, key, cmd).map(Some),
    }
}

/// The range of characters `p` gives as `start` and `end`: both or neither, whole numbers.
pub(super) fn optional_range(p: &Value, cmd: &str) -> Result<Option<(usize, usize)>> {
    let to_usize = |n: u32| usize::try_from(n).map_err(|_| bad(cmd, "`start` and `end` are too large"));
    match (optional_whole(p, "start", cmd)?, optional_whole(p, "end", cmd)?) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) => Ok(Some((to_usize(start)?, to_usize(end)?))),
        _ => Err(bad(cmd, "`start` and `end` go together: give both or neither")),
    }
}

/// The shape ids `p` gives for `key`: a non-empty array of whole numbers, at most [`MAX_IDS`].
pub(super) fn required_ids(p: &Value, key: &str, cmd: &str) -> Result<Vec<u32>> {
    let Some(Value::Array(items)) = p.get(key) else {
        return Err(bad(cmd, format!("`{key}` must be an array of shape ids")));
    };
    if items.is_empty() || items.len() > MAX_IDS {
        return Err(bad(cmd, format!("`{key}` must hold between 1 and {MAX_IDS} shape ids")));
    }
    items.iter().map(|v| whole_of(v, key, cmd)).collect()
}

/// The page a command works on: the one named, or else the first page holding every one of `ids`, or else the first page. **Each page is walked once for all the `ID`s** (`draw::page_holding`):
/// a search for each by `Document::find_shape` walked a page from the top for each, so 4,096 names on a page of 30,000 shapes were 58 million visits.
pub(super) fn page_for(doc: &Document, named: Option<u32>, ids: &[u32], cmd: &str) -> Result<u32> {
    draw::page_holding(doc, named, ids).ok_or_else(|| bad(cmd, "the document has no pages"))
}

/// An end of a connector: `{id, point?}` for a shape or `{x, y}` for a place (paper inches, turned into drawing units by `k`).
pub(super) fn end_of(p: &Value, key: &str, cmd: &str, k: f64) -> Result<End> {
    let Some(Value::Object(map)) = p.get(key) else {
        return Err(bad(cmd, format!("`{key}` must be an object: {{id, point}} for a shape or {{x, y}} for a place")));
    };
    if let Some(unknown) = map.keys().find(|name| !matches!(name.as_str(), "id" | "point" | "x" | "y")) {
        return Err(bad(cmd, format!("`{key}` has an unknown key `{}`; it takes id and point, or x and y", crate::error::shown(unknown))));
    }
    let has_shape = map.get("id").is_some_and(|v| !v.is_null());
    let has_place = map.get("x").is_some_and(|v| !v.is_null()) || map.get("y").is_some_and(|v| !v.is_null());
    match (has_shape, has_place) {
        (true, false) => {
            let id = whole_of(map.get("id").unwrap_or(&Value::Null), "id", cmd)?;
            let point = map.get("point").filter(|v| !v.is_null()).map(|v| whole_of(v, "point", cmd)).transpose()?;
            Ok(End::Shape { id, point })
        }
        (false, true) if !map.contains_key("point") => {
            let x = required_number(&Value::Object(map.clone()), "x", cmd)?;
            let y = required_number(&Value::Object(map.clone()), "y", cmd)?;
            Ok(End::Point { x: x * k, y: y * k })
        }
        _ => Err(bad(cmd, format!("`{key}` must be {{id, point}} for a shape or {{x, y}} for a place, and not both"))),
    }
}

/// Drawing units per paper inch on a page.
pub(super) fn units_per_inch(doc: &Document, page: u32) -> f64 {
    draw::units_per_inch(doc, page)
}
