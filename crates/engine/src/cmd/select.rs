//! The selection commands: `select.by_type`, `select.by_region` and `select.choices`, the engine's side of Home, Editing, Select. The rules are in `draw/select.rs` and
//! `docs/editing-commands.md`.
//!
//! **Selection stays in the window**: these are queries that answer with the `ID`s a selection would hold, change nothing, and are not undo steps. A window selects what they answer;
//! a program passes it to the command that works on shapes. Lengths are paper inches, as the other editing commands have them.

use serde_json::{Map, Value, json};

use super::params::{optional_whole, page_for};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw::{self, LayerPick, MAX_SELECT_POINTS, Picked, SelectKind, SelectRole, TypeQuery};
use crate::error::{Result, bad, failed, shown};

/// The most words one list of `types`, `roles` or `layers` holds, counted before any is read.
const MAX_WORDS: usize = 64;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query
            "select.by_type",
            "Select by Type",
            ["Home", "Editing", "Select"],
            None,
            "types: array of words, optional, any of shape, group, guide, ole, metafile, bitmap and ink: the shapes of any of these types (a connector or a container is a shape too); \
             roles: array of words, optional, connector and container: the shapes that are any of these (a list is a container too); \
             layers: array of layer names or numbers, optional, the numbers being the IX of the page's layers: the shapes on any of them; \
             page: whole number, optional, the id of the page (default: the first page). \
             The three lists are put together by and, an empty one asks nothing, and at least one must be given. \
             The answer is {page, ids, count, looked_at, unread?, locked?}: the ids of the page's top-level shapes in its order (back to front) that the query takes, and nothing is changed. \
             A shape on a locked layer is never taken, and locked says how many that left out",
            has_document,
            by_type_command
        ),
        cmd!(
            query
            "select.by_region",
            "Select in a Region",
            ["Home", "Editing", "Select"],
            None,
            "polygon: array of [x, y] pairs, inches from the page's left and bottom edges, at most 2048 of them: the region, the last point joining the first; \
             rect: array of four numbers [x0, y0, x1, y1], inches, the corners of a rectangle in any order, instead of polygon (give polygon or rect, not both); \
             partial: true or false, optional (default false): a shape is taken only when it lies wholly inside the region, and with partial also when the region touches, crosses or lies inside it; \
             page: whole number, optional, the id of the page (default: the first page). \
             The answer is {page, ids, count, partial, looked_at, work, unread?, locked?, coarse?}: the ids of the page's top-level shapes in its order (back to front), and nothing is changed. \
             A shape on a locked layer is never taken, and coarse counts the shapes taken by their box because their paths could not be paid for",
            has_document,
            by_region_command
        ),
        cmd!(
            query
            "select.choices",
            "Select by Type Choices",
            ["Home", "Editing", "Select"],
            None,
            "page: whole number, optional, the id of the page (default: the first page). \
             The answer is {page, types, roles, layers, looked_at, unread?}: what the Select by Type dialog offers, each type and role as {word, label, count}, \
             and each layer of the page as {ix, name, locked, count}, count being how many of the page's shapes it holds",
            has_document,
            choices_command
        ),
    ]
}

/// The page the command works on: the one named, else the first.
fn page_of(s: &Session, p: &Value, cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, &[], cmd)
}

/// The list `p` gives for `key`, which may be left out: an array of at most [`MAX_WORDS`] entries.
fn list<'a>(p: &'a Value, key: &str, cmd: &str, what: &str) -> Result<&'a [Value]> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) if items.len() > MAX_WORDS => {
            Err(bad(cmd, format!("`{key}` holds at most {MAX_WORDS} {what}, and this has {}", items.len())))
        }
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(bad(cmd, format!("`{key}` must be an array of {what}"))),
    }
}

/// The query `p` gives.
fn query_of(p: &Value, cmd: &str) -> Result<TypeQuery> {
    let mut query = TypeQuery::default();
    for item in list(p, "types", cmd, "words")? {
        let Value::String(word) = item else { return Err(bad(cmd, "`types` must hold words")) };
        let kind = SelectKind::from_word(word).ok_or_else(|| {
            let known: Vec<&str> = SelectKind::ALL.iter().map(|k| k.word()).collect();
            bad(cmd, format!("no shape type `{}`; the types are {} and {}", shown(word), known[..known.len() - 1].join(", "), known[known.len() - 1]))
        })?;
        if !query.kinds.contains(&kind) {
            query.kinds.push(kind);
        }
    }
    for item in list(p, "roles", cmd, "words")? {
        let Value::String(word) = item else { return Err(bad(cmd, "`roles` must hold words")) };
        let role =
            SelectRole::from_word(word).ok_or_else(|| bad(cmd, format!("no role `{}`; the roles are connector and container", shown(word))))?;
        if !query.roles.contains(&role) {
            query.roles.push(role);
        }
    }
    for item in list(p, "layers", cmd, "layer names or numbers")? {
        let pick = match item {
            Value::String(name) => LayerPick::Name(name.clone()),
            Value::Number(n) => LayerPick::Ix(
                n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(cmd, "a layer number is a whole number from 0 to 4294967295"))?,
            ),
            _ => return Err(bad(cmd, "`layers` must hold layer names or numbers")),
        };
        if !query.layers.contains(&pick) {
            query.layers.push(pick);
        }
    }
    if query.kinds.is_empty() && query.roles.is_empty() && query.layers.is_empty() {
        return Err(bad(
            cmd,
            "give types, roles or layers: the types are shape, group, guide, ole, metafile, bitmap and ink, the roles connector and container",
        ));
    }
    Ok(query)
}

/// What a selection answers: the page, the ids and how many, and the counts that are not 0.
fn answer(picked: &Picked) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("page".to_owned(), json!(picked.page));
    map.insert("ids".to_owned(), json!(picked.ids));
    map.insert("count".to_owned(), json!(picked.ids.len()));
    map.insert("looked_at".to_owned(), json!(picked.looked_at));
    for (key, n) in
        [("unread", picked.unread), ("locked", picked.locked), ("coarse", picked.coarse), ("read", picked.read), ("overlong", picked.overlong)]
    {
        if n > 0 {
            map.insert(key.to_owned(), json!(n));
        }
    }
    map
}

/// The shapes a type, a role and a layer take → `{page, ids, count, looked_at, unread?, locked?}`.
fn by_type_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "select.by_type";
    let query = query_of(p, ID)?;
    let page = page_of(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let picked = draw::by_type(doc, page, &query).map_err(|problem| problem.into_engine(ID))?;
    Ok(Value::Object(answer(&picked)))
}

/// A point `[x, y]` of finite numbers.
fn point_of(item: &Value, cmd: &str) -> Result<(f64, f64)> {
    match item {
        Value::Array(pair) => match pair.as_slice() {
            [Value::Number(x), Value::Number(y)] => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x, y)),
                _ => Err(bad(cmd, "each point of `polygon` must be a pair of finite numbers")),
            },
            [_, _] => Err(bad(cmd, "each point of `polygon` must be a pair of finite numbers")),
            _ => Err(bad(cmd, "each point of `polygon` must be a pair of numbers: [x, y]")),
        },
        _ => Err(bad(cmd, "each point of `polygon` must be a pair of numbers: [x, y]")),
    }
}

/// The region `p` gives, as polygon points: `polygon`, or the four corners of `rect`.
fn region_of(p: &Value, cmd: &str) -> Result<Vec<(f64, f64)>> {
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    match (given("polygon"), given("rect")) {
        (true, true) => Err(bad(cmd, "give `polygon` or `rect`, not both")),
        (false, false) => Err(bad(cmd, "give `polygon` or `rect`: the region to select in")),
        (true, false) => {
            let Some(Value::Array(items)) = p.get("polygon") else {
                return Err(bad(cmd, "`polygon` must be an array of [x, y] pairs, inches from the page's left and bottom edges"));
            };
            if items.len() > MAX_SELECT_POINTS {
                return Err(bad(cmd, format!("`polygon` holds at most {MAX_SELECT_POINTS} points, and this has {}", items.len())));
            }
            let points = items.iter().map(|item| point_of(item, cmd)).collect::<Result<Vec<_>>>()?;
            check_within(&points, cmd)?;
            Ok(points)
        }
        (false, true) => {
            let numbers: Option<Vec<f64>> = match p.get("rect") {
                Some(Value::Array(items)) if items.len() == 4 => items.iter().map(|v| v.as_f64().filter(|n| n.is_finite())).collect(),
                _ => None,
            };
            match numbers.as_deref() {
                Some([x0, y0, x1, y1]) => {
                    let points = vec![(*x0, *y0), (*x1, *y0), (*x1, *y1), (*x0, *y1)];
                    check_within(&points, cmd)?;
                    Ok(points)
                }
                _ => Err(bad(cmd, "`rect` must be [x0, y0, x1, y1]: four finite numbers, inches")),
            }
        }
    }
}

/// Every point within [`draw::MAX_COORD`] drawing units of the page's origin: past any page, short of arithmetic that has lost its digits.
fn check_within(points: &[(f64, f64)], cmd: &str) -> Result<()> {
    if points.iter().any(|p| p.0.abs() > draw::MAX_COORD || p.1.abs() > draw::MAX_COORD) {
        return Err(bad(cmd, format!("each point of the region must be within {} of the page's origin", draw::MAX_COORD)));
    }
    Ok(())
}

/// The shapes a region takes → `{page, ids, count, partial, looked_at, work, unread?, locked?, coarse?}`.
fn by_region_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "select.by_region";
    let polygon = region_of(p, ID)?;
    let partial = match p.get("partial") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(ID, "`partial` must be true or false")),
    };
    let page = page_of(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let picked = draw::by_region(doc, page, &polygon, partial).map_err(|problem| problem.into_engine(ID))?;
    let mut map = answer(&picked);
    map.insert("partial".to_owned(), json!(partial));
    map.insert("work".to_owned(), json!(picked.work));
    map.insert("steps".to_owned(), json!(picked.steps));
    Ok(Value::Object(map))
}

/// What the Select by Type dialog offers → `{page, types, roles, layers, looked_at, unread?}`.
fn choices_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "select.choices";
    let page = page_of(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let found = draw::select_choices(doc, page).map_err(|problem| problem.into_engine(ID))?;
    let types: Vec<Value> = found.kinds.iter().map(|(k, count)| json!({"word": k.word(), "label": k.label(), "count": count})).collect();
    let roles: Vec<Value> = found.roles.iter().map(|(r, count)| json!({"word": r.word(), "label": r.label(), "count": count})).collect();
    let layers: Vec<Value> = found.layers.iter().map(|l| json!({"ix": l.ix, "name": l.name, "locked": l.locked, "count": l.count})).collect();
    let mut map = Map::new();
    map.insert("page".to_owned(), json!(found.page));
    map.insert("types".to_owned(), Value::Array(types));
    map.insert("roles".to_owned(), Value::Array(roles));
    map.insert("layers".to_owned(), Value::Array(layers));
    map.insert("looked_at".to_owned(), json!(found.looked_at));
    map.insert("read".to_owned(), json!(found.read));
    map.insert("lookups".to_owned(), json!(found.lookups));
    if found.overlong > 0 {
        map.insert("overlong".to_owned(), json!(found.overlong));
    }
    if found.unread > 0 {
        map.insert("unread".to_owned(), json!(found.unread));
    }
    Ok(Value::Object(map))
}
