//! Comments: `comment.add`, `comment.reply`, `comment.edit`, `comment.delete`, `comment.list`, `comment.markers`, `comment.next`, `comment.previous`
//! and `comment.author`. The rules are named in `docs/editing-commands.md`, "Comments",
//! and in `draw/comments.rs` (the marker's place and a card's head).
//!
//! A comment is a row of the document's comment list (`diacraft_doc::Comments`): it names its page and its shape by `ID`, so it stays with the shape, whatever moves it.
//! An edit of the list is one undo step; **Show Markup** (`comment.markers`) is a setting of the drawing and is none (the View settings' rule).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Comment, CommentError, Comments, Document, NewComment, Owner};
use serde_json::{Map, Value, json};

use super::params::{optional_whole, page_for};
use super::{CommandSpec, has_document, optional_text, required_text, required_u32};
use crate::Session;
use crate::draw::comments::{self as marker};
use crate::error::{Result, bad, failed};

/// The most comments one `comment.list` answers: the default and the highest `limit`.
pub const DEFAULT_LIST: usize = 1_000;
pub const MAX_LIST: usize = 10_000;

/// The most characters of an author's name or initials (the rule for names, `diacraft_doc::MAX_NAME_CHARS`).
const MAX_AUTHOR_CHARS: usize = 255;
const MAX_INITIALS_CHARS: usize = 8;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "comment.add",
            "New Comment",
            ["Review", "Comments"],
            None,
            "text: text, the comment: at most 65536 bytes, not blank, no control character but a line break or a tab; \
             id: whole number, optional, the shape the comment is on (default: the page); \
             page: whole number, optional, the id of the page (default: the first page with the shape, else the first page). \
             The author is the one `comment.author` says, and the date and time are now, in UTC. A comment on a shape stays with it when it moves, and adding one turns the markers on. \
             A drawing holds at most 100000 comments and 32 MiB of their text. One undo step",
            has_document,
            add
        ),
        cmd!(
            "comment.reply",
            "Reply",
            ["Review", "Comments"],
            None,
            "comment: whole number, the comment to answer (a reply to a reply joins the thread of the comment it answers); text: text, as for comment.add. \
             The reply sits where its comment sits. One undo step",
            has_document,
            reply
        ),
        cmd!(
            "comment.edit",
            "Edit Comment",
            ["Review", "Comments"],
            None,
            "comment: whole number, the comment or reply to change; text: text, the new text, as for comment.add. Its edit date is now. One undo step",
            has_document,
            edit
        ),
        cmd!(
            "comment.delete",
            "Delete Comment",
            ["Review", "Comments"],
            None,
            "comment: whole number, the comment or reply to take away. The replies to a comment go with it. One undo step",
            has_document,
            delete
        ),
        cmd!(
            query "comment.list",
            "Comments",
            ["Review", "Comments"],
            None,
            "page: whole number, optional, only the comments of this page; id: whole number, optional, only the comments on this shape (with page, else the first page holding it); \
             details: true or false, optional (default false), adds each comment's `anchor` ({x, y}: where its marker is, in paper inches), `shape_text` (the head of its card: the start of the \
             shape's text) and `shape_gone` (the shape is not on the page: the marker is the page's); after: whole number, optional, only the comments after this one in the list; \
             limit: whole number, optional, from 1 to 10000 (default 1000), at most this many. Says `markers` (whether the markers show), `total` (how many match) and `more`",
            has_document,
            list
        ),
        cmd!(
            "comment.markers",
            "Show Markup",
            ["Review", "Comments"],
            None,
            "show: true or false, optional, whether the comments' markers show on the page (default: the other of what it is now). It is a setting of the drawing and no undo step, as the View settings are",
            has_document,
            markers
        ),
        cmd!(
            query "comment.next",
            "Next Comment",
            ["Review", "Comments"],
            None,
            "from: whole number, optional, the comment to step from (a reply stands for its thread; default: before the first). Answers the next thread's first comment, by page order and then by number, \
             and `wrapped` when it went round from the last to the first. The window shows its page and shape. `comment` is null when the drawing has none",
            has_document,
            next
        ),
        cmd!(
            query "comment.previous",
            "Previous Comment",
            ["Review", "Comments"],
            None,
            "from: whole number, optional, the comment to step from (a reply stands for its thread; default: after the last). Answers the previous thread's first comment, as comment.next does",
            has_document,
            previous
        ),
        cmd!(
            query "comment.author",
            "Comment Author",
            ["Review", "Comments"],
            None,
            "name: text, optional, the name the author of new comments is given (not blank, at most 255 characters, no control character); \
             initials: text, optional, the author's initials (at most 8 characters; default: the first letters of the words of the name, at most three, in capitals). \
             It is a setting of the session and not of the drawing: it says what it is now, and the name is written into a drawing only by a comment",
            always_open,
            author
        ),
    ]
}

/// Enabled with or without a drawing: the author is the session's.
fn always_open(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------ words

/// The refusal a model error is, in the words of the command.
fn refuse(cmd: &str, e: CommentError) -> crate::error::EngineError {
    match e {
        CommentError::TooMany
        | CommentError::TooManyAuthors
        | CommentError::TotalTextTooLong
        | CommentError::IdsExhausted
        | CommentError::DuplicateId(_)
        | CommentError::DuplicateAuthorId(_) => failed(cmd, e.to_string()),
        CommentError::TextTooLong
        | CommentError::EmptyText
        | CommentError::ControlCharacter
        | CommentError::NoSuchComment(_)
        | CommentError::NoSuchAuthor(_) => bad(cmd, e.to_string()),
    }
}

/// The refusal of a drawing whose comments part the reader could not take into the model and kept as bytes (too large, or more than the model holds exactly): a comment added would be written
/// as a second comments part beside it, so none is added until the drawing's own is dealt with.
fn unread_part(doc: &Document, cmd: &str) -> Result<()> {
    const COMMENTS: &str = "application/vnd.ms-visio.comments+xml"; // brand-ok: a content type the format defines
    if doc.parts.iter().any(|p| p.media_type.trim().eq_ignore_ascii_case(COMMENTS)) {
        return Err(failed(
            cmd,
            "this drawing's comments part could not be read (it is past the size or the shape the model holds), so a comment cannot be added beside it without making a second one",
        ));
    }
    Ok(())
}

/// The time now as the text a comment holds, UTC: the session's clock, and the epoch when there is none (a clock before 1970).
fn now_text(s: &Session) -> Arc<str> {
    Arc::from(s.now().and_then(diacraft_calc::utc_iso).unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned()))
}

/// The edit date: now, and never before the date the comment was written (the dates are text of one shape, so the order of the text is the order of the times).
fn edit_text(now: &Arc<str>, written: &str) -> Arc<str> {
    if &**now < written { Arc::from(written) } else { Arc::clone(now) }
}

// ------------------------------------------------------------------------------------------------------------ the answers

fn comment_json(c: &Comment, comments: &Comments) -> Value {
    let author = comments.author(c.author);
    json!({
        "id": c.id,
        "parent": c.parent,
        "page": c.page,
        "shape": c.shape,
        "author": author.map_or("", |a| &*a.name),
        "initials": author.map_or("", |a| &*a.initials),
        "author_id": c.author,
        "text": &*c.text,
        "date": &*c.date,
        "edit_date": c.edit_date.as_deref(),
        "done": c.done,
    })
}

fn one(c: &Comment, comments: &Comments) -> Value {
    json!({ "comment": comment_json(c, comments) })
}

// ------------------------------------------------------------------------------------------------------------ the commands

/// A new comment on a shape or on the page → `{comment}`.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.add";
    let text = required_text(p, "text", ID)?.to_owned();
    let shape = optional_whole(p, "id", ID)?;
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    unread_part(doc, ID)?;
    let ids: Vec<u32> = shape.into_iter().collect();
    let page = page_for(doc, named, &ids, ID)?;
    if doc.page(page).is_none() {
        return Err(bad(ID, format!("no page has id {page}")));
    }
    if let Some(id) = shape
        && doc.find_shape(Owner::Page(page), id).is_none()
    {
        return Err(bad(ID, format!("page {page} has no shape {id}")));
    }
    let (name, initials) = s.comment_author();
    let date = now_text(s);
    s.edit("New Comment", |doc| {
        let author = doc.comments.author_for(&name, &initials).map_err(|e| refuse(ID, e))?;
        let id = doc.comments.add(NewComment { parent: None, page, shape, author, text: Arc::from(text), date }).map_err(|e| refuse(ID, e))?;
        // Comments show only while the markers are on, which adding one turns on (inventory-review: "Comment markers on the page").
        if doc.comments.show_tags == Some(false) {
            doc.comments.show_tags = Some(true);
        }
        let c = doc.comments.get(id).ok_or_else(|| failed(ID, "the comment just added is missing"))?;
        Ok(one(c, &doc.comments))
    })
}

/// A reply to a comment → `{comment}`.
fn reply(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.reply";
    let to = required_u32(p, "comment", ID)?;
    let text = required_text(p, "text", ID)?.to_owned();
    let (name, initials) = s.comment_author();
    let date = now_text(s);
    s.edit("Reply to Comment", |doc| {
        unread_part(doc, ID)?;
        let author = doc.comments.author_for(&name, &initials).map_err(|e| refuse(ID, e))?;
        let id = doc
            .comments
            .add(NewComment { parent: Some(to), page: 0, shape: None, author, text: Arc::from(text), date })
            .map_err(|e| refuse(ID, e))?;
        let c = doc.comments.get(id).ok_or_else(|| failed(ID, "the reply just added is missing"))?;
        Ok(one(c, &doc.comments))
    })
}

/// Another text for a comment → `{comment}`.
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.edit";
    let id = required_u32(p, "comment", ID)?;
    let text = required_text(p, "text", ID)?.to_owned();
    let now = now_text(s);
    s.edit_if("Edit Comment", |doc| {
        let written = doc.comments.get(id).ok_or_else(|| refuse(ID, CommentError::NoSuchComment(id)))?.date.clone();
        // The same text is no edit: no undo step, and the edit date stays.
        let same = doc.comments.get(id).is_some_and(|c| *c.text == *text.replace("\r\n", "\n").replace('\r', "\n"));
        if same {
            let c = doc.comments.get(id).ok_or_else(|| refuse(ID, CommentError::NoSuchComment(id)))?;
            return Ok((one(c, &doc.comments), false));
        }
        doc.comments.edit(id, &text, edit_text(&now, &written)).map_err(|e| refuse(ID, e))?;
        let c = doc.comments.get(id).ok_or_else(|| refuse(ID, CommentError::NoSuchComment(id)))?;
        Ok((one(c, &doc.comments), true))
    })
}

/// A comment and its replies taken away → `{deleted}`.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.delete";
    let id = required_u32(p, "comment", ID)?;
    s.edit("Delete Comment", |doc| {
        let deleted = doc.comments.remove(id).map_err(|e| refuse(ID, e))?;
        Ok(json!({ "deleted": deleted }))
    })
}

/// Show Markup → `{markers}`.
fn markers(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.markers";
    let want = match p.get("show") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad(ID, "`show` must be true or false")),
    };
    s.edit_view(ID, |doc| {
        let now = doc.comments.show_tags.unwrap_or(true);
        let next = want.unwrap_or(!now);
        if next != now {
            doc.comments.show_tags = Some(next);
        }
        Ok((json!({ "markers": next }), next != now))
    })
}

/// The comments → `{comments, markers, total, more}`.
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.list";
    let page = optional_whole(p, "page", ID)?;
    let shape = optional_whole(p, "id", ID)?;
    let after = optional_whole(p, "after", ID)?;
    let details = match p.get("details") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(ID, "`details` must be true or false")),
    };
    let limit = match optional_whole(p, "limit", ID)? {
        None => DEFAULT_LIST,
        Some(n) => {
            usize::try_from(n).ok().filter(|n| (1..=MAX_LIST).contains(n)).ok_or_else(|| bad(ID, format!("`limit` must be from 1 to {MAX_LIST}")))?
        }
    };
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    // A shape given with no page is looked for as the commands look for one.
    let page = match (page, shape) {
        (None, Some(id)) => Some(page_for(doc, None, &[id], ID)?),
        (named, _) => named,
    };
    let c = &doc.comments;
    let matching: Vec<&Arc<Comment>> =
        c.iter().filter(|m| page.is_none_or(|pg| m.page == pg) && shape.is_none_or(|sh| m.shape == Some(sh))).collect();
    let start = match after {
        None => 0,
        Some(id) => {
            matching.iter().position(|m| m.id == id).map(|at| at + 1).ok_or_else(|| bad(ID, format!("no comment of this list has id {id}")))?
        }
    };
    let picked: Vec<&Arc<Comment>> = matching.iter().skip(start).take(limit).copied().collect();
    let mut out: Vec<Value> = picked.iter().map(|m| comment_json(m, c)).collect();
    if details {
        add_details(doc, &picked, &mut out, ID)?;
    }
    let more = start.saturating_add(picked.len()) < matching.len();
    Ok(json!({ "comments": out, "markers": c.show_tags.unwrap_or(true), "total": matching.len(), "more": more }))
}

/// The `anchor`, `shape_text` and `shape_gone` of each comment of the answer: one walk of each page the comments are on.
fn add_details(doc: &Document, picked: &[&Arc<Comment>], out: &mut [Value], cmd: &str) -> Result<()> {
    let mut wanted: HashMap<u32, HashSet<u32>> = HashMap::new();
    for m in picked {
        if let Some(shape) = m.shape {
            wanted.entry(m.page).or_default().insert(shape);
        }
    }
    let mut facts: HashMap<u32, HashMap<u32, marker::ShapeFacts>> = HashMap::with_capacity(wanted.len());
    for (page, ids) in &wanted {
        if doc.page(*page).is_some() {
            facts.insert(*page, marker::shape_facts(doc, *page, ids).map_err(|e| e.into_engine(cmd))?);
        }
    }
    let mut corners: HashMap<u32, (f64, f64)> = HashMap::new();
    for (m, value) in picked.iter().zip(out.iter_mut()) {
        let found = m.shape.and_then(|sh| facts.get(&m.page).and_then(|f| f.get(&sh)));
        let (x, y) = match found {
            Some(f) => (f.x, f.y),
            None => *corners.entry(m.page).or_insert_with(|| marker::page_corner(doc, m.page)),
        };
        if let Value::Object(map) = value {
            map.insert("anchor".to_owned(), json!({ "x": x, "y": y }));
            map.insert("shape_text".to_owned(), found.map_or(Value::Null, |f| json!(f.head)));
            map.insert("shape_gone".to_owned(), json!(m.shape.is_some() && found.is_none()));
        }
    }
    Ok(())
}

/// The threads' first comments in the order a person steps through them: the pages' order in the drawing, then the comments' numbers.
fn stops(doc: &Document) -> Vec<&Arc<Comment>> {
    let mut place: HashMap<u32, usize> = HashMap::with_capacity(doc.pages.len());
    for (at, page) in doc.pages.iter().enumerate() {
        place.entry(page.id).or_insert(at);
    }
    let mut roots: Vec<&Arc<Comment>> = doc.comments.iter().filter(|c| c.parent.is_none()).collect();
    // A comment on a page the drawing no longer has stands after the pages that are there.
    roots.sort_by_key(|c| (place.get(&c.page).copied().unwrap_or(usize::MAX), c.id));
    roots
}

fn step(s: &Session, p: &Value, cmd: &str, forward: bool) -> Result<Value> {
    let from = optional_whole(p, "from", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let all = stops(doc);
    if all.is_empty() {
        return Ok(json!({ "comment": Value::Null, "wrapped": false }));
    }
    let here = match from {
        None => None,
        Some(id) => {
            let c = doc.comments.get(id).ok_or_else(|| bad(cmd, format!("no comment has id {id}")))?;
            let root = c.parent.unwrap_or(c.id);
            all.iter().position(|r| r.id == root)
        }
    };
    let last = all.len() - 1;
    let (at, wrapped) = match (here, forward) {
        (None, true) => (0, false),
        (None, false) => (last, false),
        (Some(i), true) if i == last => (0, true),
        (Some(i), true) => (i + 1, false),
        (Some(0), false) => (last, true),
        (Some(i), false) => (i - 1, false),
    };
    let Some(found) = all.get(at) else { return Ok(json!({ "comment": Value::Null, "wrapped": false })) };
    let mut answer = one(found, &doc.comments);
    if let Value::Object(map) = &mut answer {
        map.insert("wrapped".to_owned(), json!(wrapped));
    }
    Ok(answer)
}

fn next(s: &mut Session, p: &Value) -> Result<Value> {
    step(s, p, "comment.next", true)
}

fn previous(s: &mut Session, p: &Value) -> Result<Value> {
    step(s, p, "comment.previous", false)
}

/// The initials of a name: the first letters of its words, at most three, in capitals.
fn initials_of(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(3).flat_map(char::to_uppercase).collect()
}

/// The author of new comments → `{name, initials}`.
fn author(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "comment.author";
    let name = optional_text(p, "name", ID)?;
    let initials = optional_text(p, "initials", ID)?;
    let check = |what: &str, text: &str, most: usize| -> Result<()> {
        if text.trim().is_empty() {
            return Err(bad(ID, format!("the {what} must not be blank")));
        }
        if text.chars().take(most + 1).count() > most {
            return Err(bad(ID, format!("the {what} must be at most {most} characters")));
        }
        if text.chars().any(char::is_control) {
            return Err(bad(ID, format!("the {what} must not contain a control character")));
        }
        Ok(())
    };
    if let Some(n) = name {
        check("name", n, MAX_AUTHOR_CHARS)?;
    }
    if let Some(i) = initials {
        check("initials", i, MAX_INITIALS_CHARS)?;
    }
    if name.is_some() || initials.is_some() {
        let (old_name, old_initials) = s.comment_author();
        let new_name = name.map_or(old_name, |n| n.trim().to_owned());
        // Initials follow a new name unless they were given.
        let new_initials = initials.map_or_else(|| if name.is_some() { initials_of(&new_name) } else { old_initials }, |i| i.trim().to_owned());
        s.set_comment_author(new_name, new_initials);
    }
    let (name, initials) = s.comment_author();
    Ok(Value::Object(Map::from_iter([("name".to_owned(), json!(name)), ("initials".to_owned(), json!(initials))])))
}
