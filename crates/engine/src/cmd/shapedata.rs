//! The Data tab's side of the engine: the Shape Data of a shape (`shapedata.list`, `shapedata.set`, `shapedata.define`, `shapedata.types`). The rules are named in `draw/shapedata.rs`
//! and in `docs/editing-commands.md`, "Shape Data".

use serde_json::{Map, Value, json};

use super::params::{optional_whole, page_for};
use super::{CommandSpec, has_document, required_u32};
use crate::Session;
use crate::draw::shapedata::{self, Applied, Field, FieldSpec, Kind, MAX_FIELDS, MAX_VALUES};
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "shapedata.list",
            "Shape Data",
            ["Data", "Show/Hide"],
            None,
            "id: whole number, the shape, whose Shape Data section (its own rows, and those it takes from its master) is listed in the order the Shape Data window shows: by SortKey ignoring case, \
             then row order, the fields with no key last, and each field's value as typed (value, with unit and formula when it has them) and as shown through its Format (shown). \
             The list is at most 512 fields and the answer says how many are hidden (hidden, left out unless asked for). Answers {page, id, name, fields, hidden}, each field {row, label, type, type_code, \
             format, value, unit, formula, shown, prompt, sort_key, invisible, ask, lang_id, calendar, calendar_name, items}, where type is unknown for a Type cell that holds no number from 0 to 7 \
             (type_code is the whole number it holds, null for a fraction or a text, and the value is a string's), ask is the Verify cell, lang_id is the LangID cell's text (a tag such as en-US, \
             or a number a file holds, kept as it is), calendar_name is the name of the calendar number and a date never set (a plain 0 with no unit) shows nothing, and items is a list's items; \
             hidden: true or false, optional (default false), true lists the fields whose Invisible cell is on too; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            list
        ),
        cmd!(
            "shapedata.set",
            "Enter Shape Data",
            ["Data", "Show/Hide"],
            None,
            "id: whole number, the shape; \
             row: text, the name of one of the shape's rows (the name formulas read, Prop.<row>), with value; \
             value: text, or a number, or true or false, read as its field's type (a string and a list item as they are, a number with a unit or without, true or false, a date such as 2026-03-04 or 3/4/2026 \
             with a time after it if wanted, a duration such as 36 hours, a currency such as $1,234.50), an empty text clears the field, a fixed list takes only its items, and a variable list takes a new value \
             into its Format. The value replaces any formula the Value cell has, even one under GUARD, and what reads Prop.<row> is worked out again. A text that is no value of the type is refused in words; \
             values: array of objects {row, value}, optional, instead of row and value: at most 512 values that are all set in one undo step; \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             Answers {page, id, changed, values, fields}: the fields as shapedata.define gives them. A value that is the one a field has is no undo step",
            has_document,
            set
        ),
        cmd!(
            "shapedata.define",
            "Define Shape Data",
            ["Data", "Show/Hide"],
            None,
            "id: whole number, the shape; \
             fields: array of objects, the shape's whole list of fields, each a row of its Property section. A field that gives row keeps that row (the cells it gives change and the others stay), a field \
             with no row is a new row after the others, and a row the list leaves out is taken away, hidden or not. A field gives label (text, the caption the window shows), name (text, only for a new field: \
             the row's name, letters, digits and underscores, not starting with a digit, default: the label made into one, else Property1, Property2 and so on, and a row is never renamed), \
             type (text: string, fixed_list, number, boolean, variable_list, date, duration or currency, default string, and a value is read again as the new type when it can be, else kept as it was, \
             and a Type cell of an unknown type is kept as written unless a type is given), \
             format (text, the picture the value is shown through, or for a list its items apart at semicolons), value (read as shapedata.set reads it), prompt (text, shown as a tip on the field), \
             sort_key (text, the fields are listed by it ignoring case and the unkeyed last), invisible (true or false, a hidden field keeps its value and leaves the window), \
             ask (true or false, Ask on drop, the Verify cell), lang_id (text, a language tag such as en-US: letters and digits apart at hyphens, the first part 2 to 8 letters, a new field has none unless \
             one is given) and calendar (whole number from 0 to 13, the calendars shapedata.types lists, which only a date reads, default 0 for a new field). \
             At most 512 fields and 4096 characters of each text but a value and a format, which take 32768. One undo step. A list that changes nothing is no undo step. \
             Answers {page, id, changed, fields, notes}: every field, hidden ones too, in the order listed, and notes are what a change of type left as it was; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            define
        ),
        cmd!(
            query "shapedata.types",
            "Shape Data Types",
            ["Data", "Show/Hide"],
            None,
            "none",
            has_document,
            types
        ),
    ]
}

/// The page the command works on (see [`page_for`]).
fn target_page(s: &Session, p: &Value, id: u32, cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, &[id], cmd)
}

fn field_json(f: &Field) -> Value {
    let mut out = json!({
        "row": f.row,
        "label": f.label,
        "type": f.type_word(),
        "type_code": f.type_code,
        "format": f.format,
        "value": f.value,
        "unit": f.unit,
        "formula": f.formula,
        "shown": f.shown,
        "prompt": f.prompt,
        "sort_key": f.sort_key,
        "invisible": f.invisible,
        "ask": f.ask,
        "lang_id": f.lang_id,
        "calendar": f.calendar,
        "calendar_name": f.calendar.and_then(shapedata::calendar_name),
    });
    if let (true, Some(map)) = (f.kind.is_list(), out.as_object_mut()) {
        map.insert("items".to_owned(), json!(f.items));
    }
    out
}

/// A flag the call gives, `None` when it leaves it out.
fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// A text a value or a sort key is given as: text, a number or true or false, as the text it would be typed as.
fn typed(value: &Value, label: &str, cmd: &str) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(on) => Ok(if *on { "true" } else { "false" }.to_owned()),
        _ => Err(bad(cmd, format!("{label} must be text, a number, or true or false"))),
    }
}

fn text_of(map: &Map<String, Value>, key: &str, cmd: &str, n: usize) -> Result<Option<String>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(bad(cmd, format!("field {n}: `{key}` must be text"))),
    }
}

/// A calendar's number: one of the fourteen ([`shapedata::CALENDARS`]).
fn calendar_of(map: &Map<String, Value>, cmd: &str, n: usize) -> Result<Option<u32>> {
    match map.get("calendar") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|w| u32::try_from(w).ok())
            .filter(|w| shapedata::calendar_name(*w).is_some())
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("field {n}: `calendar` must be {}", shapedata::CALENDAR_WORDS))),
    }
}

/// A language: a tag, as text ([`shapedata::is_language_tag`]; the engine checks its form).
fn language_of(map: &Map<String, Value>, cmd: &str, n: usize) -> Result<Option<String>> {
    match map.get("lang_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(tag)) => Ok(Some(tag.clone())),
        Some(_) => Err(bad(cmd, format!("field {n}: `lang_id` must be a language tag such as en-US, as text"))),
    }
}

fn flag_of(map: &Map<String, Value>, key: &str, cmd: &str, n: usize) -> Result<Option<bool>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(bad(cmd, format!("field {n}: `{key}` must be true or false"))),
    }
}

/// The fields a `fields` param gives, each key checked.
fn specs_of(p: &Value, cmd: &str) -> Result<Vec<FieldSpec>> {
    let Some(Value::Array(items)) = p.get("fields") else {
        return Err(bad(cmd, "`fields` must be an array of fields"));
    };
    if items.len() > MAX_FIELDS {
        return Err(bad(cmd, format!("a shape holds at most {MAX_FIELDS} shape data fields")));
    }
    const KEYS: [&str; 12] = ["row", "name", "label", "type", "format", "value", "prompt", "sort_key", "invisible", "ask", "lang_id", "calendar"];
    let mut out = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let n = at + 1;
        let Value::Object(map) = item else {
            return Err(bad(cmd, format!("field {n} must be an object")));
        };
        if let Some(unknown) = map.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(bad(cmd, format!("field {n} has an unknown key `{}`; a field takes {}", crate::error::shown(unknown), KEYS.join(", "))));
        }
        let kind = match text_of(map, "type", cmd, n)? {
            None => None,
            Some(word) => Some(Kind::from_word(&word).ok_or_else(|| {
                let words: Vec<&str> = Kind::ALL.iter().map(|k| k.word()).collect();
                bad(cmd, format!("field {n}: there is no type `{}`; the types are {}", crate::error::shown(&word), words.join(", ")))
            })?),
        };
        let typed_key = |key: &str| -> Result<Option<String>> {
            match map.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => typed(v, &format!("field {n}: `{key}`"), cmd).map(Some),
            }
        };
        out.push(FieldSpec {
            row: text_of(map, "row", cmd, n)?,
            name: text_of(map, "name", cmd, n)?,
            label: text_of(map, "label", cmd, n)?,
            kind,
            format: text_of(map, "format", cmd, n)?,
            value: typed_key("value")?,
            prompt: text_of(map, "prompt", cmd, n)?,
            sort_key: typed_key("sort_key")?,
            invisible: flag_of(map, "invisible", cmd, n)?,
            ask: flag_of(map, "ask", cmd, n)?,
            lang_id: language_of(map, cmd, n)?,
            calendar: calendar_of(map, cmd, n)?,
        });
    }
    Ok(out)
}

/// The values a `set` gives: `row` and `value`, or `values`.
fn values_of(p: &Value, cmd: &str) -> Result<Vec<FieldSpec>> {
    let (row, value, values) =
        (p.get("row").filter(|v| !v.is_null()), p.get("value").filter(|v| !v.is_null()), p.get("values").filter(|v| !v.is_null()));
    match (row, value, values) {
        (None, None, None) => Err(bad(cmd, "give `row` and `value`, or `values`")),
        (_, _, Some(_)) if row.is_some() || value.is_some() => Err(bad(cmd, "give `values` or `row` and `value`, not both")),
        (_, _, Some(Value::Array(items))) => {
            if items.len() > MAX_VALUES {
                return Err(bad(cmd, format!("`values` holds at most {MAX_VALUES} values")));
            }
            let mut out = Vec::with_capacity(items.len());
            for (at, item) in items.iter().enumerate() {
                let n = at + 1;
                let Value::Object(map) = item else {
                    return Err(bad(cmd, format!("value {n} must be an object {{row, value}}")));
                };
                if let Some(unknown) = map.keys().find(|k| !matches!(k.as_str(), "row" | "value")) {
                    return Err(bad(cmd, format!("value {n} has an unknown key `{}`; a value takes row and value", crate::error::shown(unknown))));
                }
                let row = match map.get("row") {
                    Some(Value::String(row)) => row.clone(),
                    _ => return Err(bad(cmd, format!("value {n}: `row` must be text, the name of a row"))),
                };
                let value = match map.get("value") {
                    Some(v) if !v.is_null() => typed(v, &format!("value {n}: `value`"), cmd)?,
                    _ => return Err(bad(cmd, format!("value {n}: missing `value`"))),
                };
                out.push(FieldSpec { row: Some(row), value: Some(value), ..FieldSpec::default() });
            }
            Ok(out)
        }
        (_, _, Some(_)) => Err(bad(cmd, "`values` must be an array of {row, value}")),
        (Some(Value::String(row)), Some(value), None) => {
            Ok(vec![FieldSpec { row: Some(row.clone()), value: Some(typed(value, "`value`", cmd)?), ..FieldSpec::default() }])
        }
        (Some(Value::String(_)), None, None) => Err(bad(cmd, "missing `value`")),
        (Some(_), _, None) => Err(bad(cmd, "`row` must be text, the name of a row")),
        (None, Some(_), None) => Err(bad(cmd, "missing `row`")),
    }
}

/// The list the shape has now, every field, as the answers give it.
fn answer(s: &Session, page: u32, id: u32, applied: &Applied, cmd: &str) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let listed = shapedata::read_fields(doc, page, id).map_err(|p| p.into_engine(cmd))?;
    Ok(json!({
        "page": page,
        "id": id,
        "changed": applied.changed,
        "values": applied.values,
        "fields": listed.fields.iter().map(field_json).collect::<Vec<_>>(),
        "notes": applied.notes,
    }))
}

/// The fields the shape has → `{page, id, name, fields, hidden}`.
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shapedata.list";
    let id = required_u32(p, "id", ID)?;
    let all = flag(p, "hidden", ID)?.unwrap_or(false);
    let page = target_page(s, p, id, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let listed = shapedata::read_fields(doc, page, id).map_err(|e| e.into_engine(ID))?;
    let name = shapedata::shape_title(doc, page, id).map_err(|e| e.into_engine(ID))?;
    let hidden = if all { 0 } else { listed.hidden() };
    let fields: Vec<Value> = listed.fields.iter().filter(|f| all || !f.invisible).map(field_json).collect();
    Ok(json!({"page": page, "id": id, "name": name, "fields": fields, "hidden": hidden}))
}

/// Enters values → `{page, id, changed, values, fields, notes}`.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shapedata.set";
    let id = required_u32(p, "id", ID)?;
    let specs = values_of(p, ID)?;
    let page = target_page(s, p, id, ID)?;
    let applied = s.edit_if("Enter shape data", |doc| {
        let applied = shapedata::apply(doc, page, id, &specs, false).map_err(|problem| problem.into_engine(ID))?;
        let changed = applied.changed;
        Ok((applied, changed))
    })?;
    answer(s, page, id, &applied, ID)
}

/// Defines the shape's whole list of fields → `{page, id, changed, values, fields, notes}`.
fn define(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shapedata.define";
    let id = required_u32(p, "id", ID)?;
    let specs = specs_of(p, ID)?;
    let page = target_page(s, p, id, ID)?;
    let applied = s.edit_if("Define shape data", |doc| {
        let applied = shapedata::apply(doc, page, id, &specs, true).map_err(|problem| problem.into_engine(ID))?;
        let changed = applied.changed;
        Ok((applied, changed))
    })?;
    answer(s, page, id, &applied, ID)
}

/// The eight types with the formats each offers, and the fourteen calendars → `{types, calendars}`.
fn types(_s: &mut Session, _p: &Value) -> Result<Value> {
    let types: Vec<Value> = Kind::ALL
        .iter()
        .map(|k| {
            json!({
                "code": k.code(),
                "id": k.word(),
                "label": k.label(),
                "list": k.is_list(),
                "formats": k.formats().into_iter().map(|(picture, label)| json!({"picture": picture, "label": label})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let calendars: Vec<Value> = shapedata::CALENDARS.iter().enumerate().map(|(code, name)| json!({"code": code, "name": name})).collect();
    Ok(json!({ "types": types, "calendars": calendars }))
}
