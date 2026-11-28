//! Find and Replace: `edit.find` lists the matches of some text in the shapes selected, a page or every page, and `edit.replace` replaces them as one
//! undo step. The window's Find and Replace dialog runs these (Find Next is `edit.find` from the place of the last match with a limit of one); a program calls them
//! straight. Neither opens a dialog.

use diacraft_doc::Document;
use serde_json::{Map, Value, json};

use super::params::{optional_whole, page_for};
use super::{CommandSpec, has_document, required_text};
use crate::Session;
use crate::draw::find::{self, Cursor, Found, Kind, MAX_LISTED, Pattern, Scope, Skipped, Spot, Within};
use crate::error::{Result, bad, failed};

/// Most shape ids a Selection scope takes: a page can hold a million shapes, and Select All sends them all.
const MAX_SELECTED: usize = 200_000;

/// The listing's default length.
const DEFAULT_LIMIT: usize = 100;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "edit.find",
            "Find",
            ["Home", "Editing"],
            None,
            "what: text, what to find, up to 1,024 characters, with the special characters ^t a tab, ^l a manual line break, ^- an optional hyphen, ^? any one character and ^^ a caret; \
             scope: selection, page or all, default page (selection looks in `ids` and the shapes inside them, all in every page in order); \
             page: whole number, optional, the id of the page for the scope selection or page (default: the first page holding all of `ids`, else the first page that is not a background); \
             ids: array of whole numbers, the shapes selected, for the scope selection; \
             in: array of text, shape_data, shape_name or user_cell, where to look, default [\"text\"]; \
             match_case: true or false, default false; \
             whole_words: true or false, default false (a word is letters, digits and `_`, so `for` is not in `forest`); \
             after: {page, id, place, row, offset}, optional, a match's place: the list starts at the first match after the characters before `offset` there, and goes round to the start of the scope when it reaches the end (see wrap); \
             wrap: true or false, optional, default true, see after; \
             limit: whole number from 1 to 10,000, default 100 → \
             {matches: [{page, id, place (text, shape_data, shape_name or user_cell), row, start, end (in characters of the text the place shows), shape_name, context}], truncated, wrapped, shapes, unread (the sections of shape data or user-defined cells with more than 20,000 rows in a shape, which are not read)}",
            has_document,
            find_command
        ),
        cmd!(
            "edit.replace",
            "Replace",
            ["Home", "Editing"],
            None,
            "what: text, what to find, as in edit.find; \
             with: text, what takes its place, at most 100,000 characters (empty takes the matches out), and a replacement may write at most 64 MiB of text in all; \
             scope: selection, page or all, as in edit.find; \
             page: whole number, optional, as in edit.find; \
             ids: array of whole numbers, the shapes selected, as in edit.find; \
             in: array of text, shape_data, shape_name or user_cell, where to look, default [\"text\"], as in edit.find; \
             match_case: true or false, default false; \
             whole_words: true or false, default false; \
             at: {page, id, place, row, start, end}, optional, one match as edit.find lists it, so that only that one is replaced, and it is an error if it is no longer there (without it every match in the scope is replaced, as one undo step) → \
             {replaced, shapes, pages, skipped: [{page, id, place, row, start, end, why}], skipped_total, locked, unread, next: {page, id, place, row, offset}}, \
             where a match is left when it is in a field's result, in a cell a formula works out, or when the result would not be allowed there (a name that is empty, \
             text of a shape that is too long, a number in a cell of text), and every match of a shape on a locked layer is left, a member of a group by its own layers, \
             and counted in locked as well as in skipped_total",
            has_document,
            replace_command
        ),
    ]
}

/// What both commands take: the pattern, the scope and the places to look in.
struct Request {
    pattern: Pattern,
    scope: Scope,
    within: Within,
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn whole(p: &Value, key: &str, cmd: &str) -> Result<Option<usize>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number"))),
    }
}

/// The shapes selected, from `ids`: an array of whole numbers, which may be empty.
fn selected(p: &Value, cmd: &str) -> Result<Vec<u32>> {
    let Some(Value::Array(items)) = p.get("ids") else {
        return Err(bad(cmd, "the scope selection needs `ids`, an array of the ids of the shapes selected"));
    };
    if items.len() > MAX_SELECTED {
        return Err(bad(cmd, format!("`ids` must hold at most {MAX_SELECTED} shape ids")));
    }
    items.iter().map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(cmd, "`ids` must be whole numbers"))).collect()
}

/// Where to look: `in` as a set of kinds. The default is shape text.
fn within_of(p: &Value, cmd: &str) -> Result<Within> {
    match p.get("in") {
        None | Some(Value::Null) => Ok(Within::TEXT),
        Some(Value::Array(items)) => {
            let mut within = Within { name: false, text: false, data: false, user: false };
            for item in items {
                match item.as_str() {
                    Some("text") => within.text = true,
                    Some("shape_name") => within.name = true,
                    Some("shape_data") => within.data = true,
                    Some("user_cell") => within.user = true,
                    _ => return Err(bad(cmd, "`in` holds text, shape_data, shape_name or user_cell")),
                }
            }
            if !(within.text || within.name || within.data || within.user) {
                return Err(bad(cmd, "`in` must name something to look in: text, shape_data, shape_name or user_cell"));
            }
            Ok(within)
        }
        Some(_) => Err(bad(cmd, "`in` must be an array of text, shape_data, shape_name or user_cell")),
    }
}

/// The scope `p` names, with the page it is on, which must be in `doc`.
fn scope_of(doc: &Document, p: &Value, cmd: &str) -> Result<Scope> {
    let word = match p.get("scope") {
        None | Some(Value::Null) => "page",
        Some(Value::String(w)) => w.as_str(),
        Some(_) => return Err(bad(cmd, "`scope` must be selection, page or all")),
    };
    let named = optional_whole(p, "page", cmd)?;
    match word {
        "all" => Ok(Scope::All),
        "page" => Ok(Scope::Page(page_for(doc, named, &[], cmd)?)),
        "selection" => {
            let ids = selected(p, cmd)?;
            let page = page_for(doc, named, &ids, cmd)?;
            Ok(Scope::Selection { page, ids })
        }
        _ => Err(bad(cmd, "`scope` must be selection, page or all")),
    }
}

fn request(doc: &Document, p: &Value, cmd: &str) -> Result<Request> {
    let what = required_text(p, "what", cmd)?;
    let pattern =
        Pattern::parse(what, flag(p, "match_case", cmd)?.unwrap_or(false), flag(p, "whole_words", cmd)?.unwrap_or(false)).map_err(|e| bad(cmd, e))?;
    Ok(Request { pattern, scope: scope_of(doc, p, cmd)?, within: within_of(p, cmd)? })
}

/// A place from `{page, id, place, row}` in `object`.
fn spot_of(object: &Map<String, Value>, key: &str, cmd: &str) -> Result<Spot> {
    let value = Value::Object(object.clone());
    let need = |name: &str| optional_whole(&value, name, cmd)?.ok_or_else(|| bad(cmd, format!("`{key}` needs `{name}`")));
    let (page, id) = (need("page")?, need("id")?);
    let place = object
        .get("place")
        .and_then(Value::as_str)
        .and_then(Kind::from_word)
        .ok_or_else(|| bad(cmd, format!("`{key}` needs `place`: text, shape_data, shape_name or user_cell")))?;
    let row = match object.get("row") {
        None | Some(Value::Null) => None,
        Some(Value::String(r)) => Some(r.clone()),
        Some(_) => return Err(bad(cmd, format!("`{key}`'s `row` must be text"))),
    };
    Ok(Spot { page, id, kind: place, row })
}

fn object<'a>(p: &'a Value, key: &str, cmd: &str) -> Result<Option<&'a Map<String, Value>>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be an object"))),
    }
}

fn spot_json(spot: &Spot) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("page".to_owned(), json!(spot.page));
    m.insert("id".to_owned(), json!(spot.id));
    m.insert("place".to_owned(), json!(spot.kind.word()));
    if let Some(row) = &spot.row {
        m.insert("row".to_owned(), json!(row));
    }
    m
}

fn found_json(f: &Found) -> Value {
    let mut m = spot_json(&f.spot);
    m.insert("start".to_owned(), json!(f.start));
    m.insert("end".to_owned(), json!(f.end));
    if let Some(name) = &f.shape_name {
        m.insert("shape_name".to_owned(), json!(crate::shown(name, 64)));
    }
    m.insert("context".to_owned(), json!(f.context));
    Value::Object(m)
}

fn skipped_json(s: &Skipped) -> Value {
    let mut m = spot_json(&s.spot);
    m.insert("start".to_owned(), json!(s.start));
    m.insert("end".to_owned(), json!(s.end));
    m.insert("why".to_owned(), json!(s.why.words()));
    Value::Object(m)
}

/// Lists matches → `{matches, truncated, wrapped, shapes}`.
fn find_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.find";
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let req = request(doc, p, ID)?;
    let limit = match whole(p, "limit", ID)? {
        None => DEFAULT_LIMIT,
        Some(n) if (1..=MAX_LISTED).contains(&n) => n,
        Some(_) => return Err(bad(ID, format!("`limit` must be a whole number from 1 to {MAX_LISTED}"))),
    };
    let cursor = match object(p, "after", ID)? {
        None => None,
        Some(after) => {
            let offset = whole(&Value::Object(after.clone()), "offset", ID)?.unwrap_or(0);
            Some(Cursor { spot: spot_of(after, "after", ID)?, offset })
        }
    };
    let wrap = flag(p, "wrap", ID)?.unwrap_or(true);
    let outcome = find::find(doc, &req.scope, &req.pattern, req.within, cursor.as_ref(), limit, wrap).map_err(|problem| problem.into_engine(ID))?;
    Ok(json!({
        "matches": outcome.found.iter().map(found_json).collect::<Vec<_>>(),
        "truncated": outcome.truncated,
        "wrapped": outcome.wrapped,
        "shapes": outcome.shapes,
        "unread": outcome.unread,
    }))
}

/// Replaces → `{replaced, shapes, pages, skipped, skipped_total, locked, next}`.
fn replace_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.replace";
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let req = request(doc, p, ID)?;
    let with = required_text(p, "with", ID)?.to_owned();
    let one = match object(p, "at", ID)? {
        None => None,
        Some(at) => {
            let value = Value::Object(at.clone());
            let start = whole(&value, "start", ID)?.ok_or_else(|| bad(ID, "`at` needs `start`"))?;
            let end = whole(&value, "end", ID)?.ok_or_else(|| bad(ID, "`at` needs `end`"))?;
            Some((spot_of(at, "at", ID)?, start, end))
        }
    };
    // One match: the scope is the page it is on.
    let scope = match &one {
        Some((spot, _, _)) => Scope::Page(spot.page),
        None => req.scope,
    };
    let label = if one.is_some() { "Replace" } else { "Replace All" };
    let done = s.edit_if(label, |doc| {
        let at = one.as_ref().map(|(spot, start, end)| (spot, *start, *end));
        let result = find::replace(doc, &scope, &req.pattern, &with, req.within, at).map_err(|problem| problem.into_engine(ID))?;
        let changed = result.replaced > 0;
        Ok((result, changed))
    })?;
    let mut out = Map::new();
    out.insert("replaced".to_owned(), json!(done.replaced));
    out.insert("shapes".to_owned(), json!(done.shapes));
    out.insert("pages".to_owned(), json!(done.pages));
    out.insert("skipped".to_owned(), Value::Array(done.skipped.iter().map(skipped_json).collect()));
    out.insert("skipped_total".to_owned(), json!(done.skipped_total));
    out.insert("locked".to_owned(), json!(done.locked));
    out.insert("unread".to_owned(), json!(done.unread));
    if let Some(next) = &done.next {
        let mut m = spot_json(&next.spot);
        m.insert("offset".to_owned(), json!(next.offset));
        out.insert("next".to_owned(), Value::Object(m));
    }
    Ok(Value::Object(out))
}
