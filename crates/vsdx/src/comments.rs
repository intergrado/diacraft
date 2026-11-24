//! The comments part (`visio/comments.xml`): read into [`Document::comments`], and written from it by every writer.
//!
//! **Read.** The part is found by the document's relationship of the type `comments` (never by name). Its `Comments` root holds an `AuthorList` of `AuthorEntry` and a
//! `CommentList` of `CommentEntry` (the text of a comment is the text of its element). **A part the model cannot hold exactly stays a part held as bytes**, as `windows.xml`
//! does, and the read says why: a root that is not `Comments`, more than [`MAX_COMMENTS`] comments or [`MAX_COMMENT_AUTHORS`] authors, a comment of more than
//! [`MAX_COMMENT_TEXT_BYTES`] bytes of text or all of them more than [`MAX_COMMENTS_TEXT_BYTES`], a number that is not a number, an `AuthorID`, `PageID`, `CommentID` or `Date`
//! missing, an id used twice, a comment with an element inside it, and a part over the small-part size ([`crate::limits::Limits::max_small_part_bytes`]). The attributes the
//! model has no field for (`ResolutionID`, `SMTP`, `SIP`, `AutoCommentType`, and the root's others) are kept in the entry's `raw`. **What is left out, and said:** an element of
//! the part that is neither of the two lists, or a list's child that is neither entry, is not kept (`comments_unread_elements`, with the count), and a save that rewrites the part
//! says so again.
//!
//! **Written** by [`root`]: `Comments` in the VSDX namespace, with the `ShowCommentTags` the model holds, then both lists. DiaCraft's replies ride as comments of the same page and
//! shape after their comment (VSDX's part has no thread: `docs/diacraft-format.md`, the save note of comments), in the order the model holds them.
//!
//! **Never crash.** The tree is held in the pool and its items are charged before a comment is made; the count of authors and comments is checked against the caps before the list
//! is built, and the text of each against the caps of the model ([`Comments::from_parts`] checks them all again).

use std::sync::Arc;

use diacraft_doc::{
    Comment, CommentAuthor, Comments, Document, MAX_COMMENT_AUTHORS, MAX_COMMENT_TEXT_BYTES, MAX_COMMENTS, MAX_COMMENTS_TEXT_BYTES, RawAttrs,
};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::price::{arc_str, raw_attr_bytes};

use crate::error::VsdxError;
use crate::ns::{NS_2012, NS_REL, Scope, ScopeExt};
use crate::reader::Reader;
use crate::work;

const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines

/// Most attributes the model keeps beside the fields of one entry or of the root (`ResolutionID`, `SMTP`, `AutoCommentType` and their like are a handful): a part with more is not read.
const MAX_RAW_ATTRS: usize = 32;

/// How many relationships of the type `comments` the reader tries before it gives up (a file can name any number, each at a part of its choosing).
const MAX_CANDIDATES: usize = 4;

/// What a comment costs the model beside its strings: the entry, its place in a chunk of the list and the `Arc` it is behind, at the codec's price for it.
const COMMENT_BYTES: u64 = 352;
const AUTHOR_BYTES: u64 = 256;

/// The relationship type of the comments part.
#[must_use]
pub fn comments_type() -> String {
    format!("{VIS}comments")
}

/// A whole number written as digits and nothing else.
pub(crate) fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

fn flag(text: &str) -> Option<bool> {
    match text {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// Keeps one more attribute of an entry the model has no field for, within [`MAX_RAW_ATTRS`] (checked before the pair is made).
fn keep(raw: &mut RawAttrs, name: &str, value: &str) -> Result<(), Why> {
    if raw.len() >= MAX_RAW_ATTRS {
        return Err(Why::Cap("an entry with more unknown attributes than the model keeps"));
    }
    raw.push(name, value);
    Ok(())
}

fn declaration(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

/// Why a comments part is not read into the model.
enum Why {
    /// What is wrong, in the words of the warning.
    Shape(&'static str),
    /// A cap of the model.
    Cap(&'static str),
}

/// The comments of a `Comments` element, or why it is not one the model holds exactly. `unread` counts the elements left out.
fn parse(root: &El, unread: &mut usize) -> Result<Comments, Why> {
    let mut show_tags = None;
    let mut raw = RawAttrs::new();
    for (name, value) in &root.attrs {
        // The declarations are the writer's. `xml:space` is kept where the file had it (in the root's `raw`), so that the part is written with it only then, or where a text needs it.
        if declaration(name) {
            continue;
        }
        if name == "ShowCommentTags" {
            match flag(value) {
                Some(on) => show_tags = Some(on),
                None => keep(&mut raw, name, value)?,
            }
        } else {
            keep(&mut raw, name, value)?;
        }
    }
    let (mut authors, mut comments): (Vec<CommentAuthor>, Vec<Comment>) = (Vec::new(), Vec::new());
    let mut text_bytes: usize = 0;
    for node in &root.children {
        let Node::El(list) = node else {
            continue;
        };
        if list.is("AuthorList") {
            for entry in list.elements() {
                if !entry.is("AuthorEntry") {
                    *unread = unread.saturating_add(1);
                    continue;
                }
                if authors.len() >= MAX_COMMENT_AUTHORS {
                    return Err(Why::Cap("more authors than the model holds"));
                }
                work::comment_entry();
                authors.push(author(entry)?);
            }
        } else if list.is("CommentList") {
            for entry in list.elements() {
                if !entry.is("CommentEntry") {
                    *unread = unread.saturating_add(1);
                    continue;
                }
                if comments.len() >= MAX_COMMENTS {
                    return Err(Why::Cap("more comments than the model holds"));
                }
                work::comment_entry();
                let c = comment(entry)?;
                if c.text.len() > MAX_COMMENT_TEXT_BYTES {
                    return Err(Why::Cap("a comment longer than the model holds"));
                }
                text_bytes = text_bytes.saturating_add(c.text.len());
                if text_bytes > MAX_COMMENTS_TEXT_BYTES {
                    return Err(Why::Cap("more text of comments than the model holds"));
                }
                comments.push(c);
            }
        } else {
            *unread = unread.saturating_add(1);
        }
    }
    Comments::from_parts(show_tags, raw, authors, comments).map_err(|_| Why::Shape("an id used twice"))
}

fn author(e: &El) -> Result<CommentAuthor, Why> {
    let mut id = None;
    let (mut name, mut initials) = (Arc::<str>::from(""), Arc::<str>::from(""));
    let mut raw = RawAttrs::new();
    for (k, v) in &e.attrs {
        match k.as_str() {
            "ID" => id = Some(number(v).ok_or(Why::Shape("an author id that is not a number"))?),
            // An attribute the file had and left empty is kept in `raw`: the part is written with one only where the file had one.
            "Name" => {
                name = Arc::from(v.as_str());
                if v.is_empty() {
                    keep(&mut raw, k, v)?;
                }
            }
            "Initials" => {
                initials = Arc::from(v.as_str());
                if v.is_empty() {
                    keep(&mut raw, k, v)?;
                }
            }
            _ if declaration(k) => {}
            _ => keep(&mut raw, k, v)?,
        }
    }
    if e.elements().next().is_some() {
        return Err(Why::Shape("an author with an element inside it"));
    }
    Ok(CommentAuthor { id: id.ok_or(Why::Shape("an author with no ID"))?, name, initials, raw })
}

fn comment(e: &El) -> Result<Comment, Why> {
    let (mut id, mut page, mut author, mut date) = (None, None, None, None);
    let (mut shape, mut edit_date, mut done) = (None, None, false);
    let mut raw = RawAttrs::new();
    for (k, v) in &e.attrs {
        match k.as_str() {
            "CommentID" => id = Some(number(v).ok_or(Why::Shape("a comment id that is not a number"))?),
            "PageID" => page = Some(number(v).ok_or(Why::Shape("a page id that is not a number"))?),
            "AuthorID" => author = Some(number(v).ok_or(Why::Shape("an author id that is not a number"))?),
            "ShapeID" => shape = Some(number(v).ok_or(Why::Shape("a shape id that is not a number"))?),
            "Date" => date = Some(Arc::<str>::from(v.as_str())),
            "EditDate" => edit_date = Some(Arc::<str>::from(v.as_str())),
            "Done" => match flag(v) {
                Some(on) => done = on,
                None => keep(&mut raw, k, v)?,
            },
            _ if declaration(k) => {}
            _ => keep(&mut raw, k, v)?,
        }
    }
    if e.elements().next().is_some() {
        return Err(Why::Shape("a comment with an element inside it"));
    }
    Ok(Comment {
        id: id.ok_or(Why::Shape("a comment with no CommentID"))?,
        parent: None,
        page: page.ok_or(Why::Shape("a comment with no PageID"))?,
        shape,
        author: author.ok_or(Why::Shape("a comment with no AuthorID"))?,
        text: Arc::from(e.text().as_str()),
        date: date.ok_or(Why::Shape("a comment with no Date"))?,
        edit_date,
        done,
        raw,
    })
}

impl<'p> Reader<'p> {
    /// The comments part, found by the document's relationships of the type `comments`, into [`Document::comments`]: see the module documentation. **A relationship of the type leads wherever a
    /// file says**, so the first [`MAX_CANDIDATES`] of them (their targets once each) are tried in order, and the first whose part is a `Comments` of the format's namespace the model can hold is
    /// the comments part. A part the model cannot hold exactly stays a part held as bytes.
    pub(crate) fn read_comments(&mut self, doc_rels: &diacraft_opc::Rels, doc: &mut Document) -> Result<(), VsdxError> {
        let ty = comments_type();
        let mut tried: Vec<String> = Vec::new();
        for target in doc_rels.items.iter().filter(|r| r.ty == ty).filter_map(|r| r.target.as_ref()) {
            let key = diacraft_opc::name::fold(target);
            if tried.contains(&key) {
                continue;
            }
            if tried.len() >= MAX_CANDIDATES {
                break;
            }
            tried.push(key);
            if self.read_comments_part(target, doc)? {
                break;
            }
        }
        Ok(())
    }

    /// One candidate: whether it was taken into the model as the comments part.
    fn read_comments_part(&mut self, part: &str, doc: &mut Document) -> Result<bool, VsdxError> {
        let Some(bytes) = self.bytes_of(part) else { return Ok(false) };
        let kept = |what: &str| format!("the comments part is {what}, and is held as the bytes it is");
        let Ok((root, tree)) = self.parse_held(part, bytes) else {
            self.notes.warn("comments_unreadable", || kept("not read as XML or past the size of a small part"));
            return Ok(false);
        };
        // A root in another namespace is some other writer's part: it stays as the bytes it is, with no word (it is not this reader's to judge).
        if !root.is("Comments") || !Scope::empty().enter(&root).ns_of(&root).is_ours() {
            self.give_bytes(tree);
            return Ok(false);
        }
        let mut unread = 0_usize;
        let read = parse(&root, &mut unread);
        let comments = match read {
            Ok(c) => c,
            Err(why) => {
                self.give_bytes(tree);
                let text = match why {
                    Why::Shape(w) | Why::Cap(w) => w,
                };
                self.notes.warn("comments_unreadable", || kept(&format!("one the model cannot hold exactly ({text})")));
                return Ok(false);
            }
        };
        // What the model keeps of the comments, charged now that the count is known (the tree they were read from is held in the pool beside it).
        let strings = comments.iter().fold(0_u64, |sum, c| {
            sum.saturating_add(arc_str(c.text.len()))
                .saturating_add(arc_str(c.date.len()))
                .saturating_add(c.edit_date.as_ref().map_or(0, |d| arc_str(d.len())))
        });
        let authors = comments
            .authors()
            .fold(0_u64, |sum, a| sum.saturating_add(AUTHOR_BYTES).saturating_add(arc_str(a.name.len())).saturating_add(arc_str(a.initials.len())));
        // The attributes kept beside the fields: each pair of every entry and of the root, as the model's list of them costs.
        let kept = |raw: &RawAttrs| raw.iter().enumerate().fold(0_u64, |sum, (at, (k, v))| sum.saturating_add(raw_attr_bytes(at, k.len(), v.len())));
        let raws = comments
            .iter()
            .map(|c| kept(&c.raw))
            .chain(comments.authors().map(|a| kept(&a.raw)))
            .chain(std::iter::once(kept(&comments.raw)))
            .fold(0_u64, u64::saturating_add);
        let price = strings.saturating_add((comments.len() as u64).saturating_mul(COMMENT_BYTES)).saturating_add(authors).saturating_add(raws);
        if self.take_bytes(price).is_err() {
            self.give_bytes(tree);
            self.notes.warn("comments_unreadable", || "the memory is spent: the comments part is held as the bytes it is".to_owned());
            return Ok(false);
        }
        self.give_bytes(tree);
        if unread > 0 {
            self.notes.warn("comments_unread_elements", || {
                format!("{unread} elements of the comments part are neither an author nor a comment and are not kept; a save that rewrites the part leaves them out")
            });
        }
        let mut comments = comments;
        // The part this read took: the one part a save rewrites (a relationship alone leads wherever a file says).
        comments.part = Some(Arc::from(part));
        doc.comments = comments;
        self.rebuild(part)?;
        Ok(true)
    }
}

/// Whether `text` has white space a reader may collapse: a line break, a tab, two blanks in a row, or a blank at either end (`xml:space="preserve"` keeps it).
fn text_needs_space(text: &str) -> bool {
    text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) || text.contains(['\n', '\r', '\t']) || text.contains("  ")
}

/// Whether the attribute `name` is already in `attrs`.
fn attr_written(attrs: &[(String, String)], name: &str) -> bool {
    attrs.iter().any(|(k, _)| k == name)
}

/// The `Comments` element the model's comments are written as: both lists, always.
#[must_use]
pub fn root(c: &Comments) -> El {
    // `xml:space` is written where the file had it (its own value, kept in `raw`); a part **made new** (comments that no package part was read into) gets it only where a text needs it:
    // a line break, a tab, a run of blanks or a blank at an end, which a reader that collapses white space would not give back. **A part read from a file that had none is written with
    // none, whatever its texts say**: the file's own comments, line breaks included, were written that way (GATE.ea42: it was written on every part, and the file had none).
    let mut attrs = vec![("xmlns".to_owned(), NS_2012.to_owned()), ("xmlns:r".to_owned(), NS_REL.to_owned())];
    if let Some(value) = c.raw.get("xml:space") {
        attrs.push(("xml:space".to_owned(), value.to_owned()));
    } else if c.part.is_none() && c.iter().any(|m| text_needs_space(&m.text)) {
        attrs.push(("xml:space".to_owned(), "preserve".to_owned()));
    }
    if let Some(on) = c.show_tags {
        attrs.push(("ShowCommentTags".to_owned(), if on { "1" } else { "0" }.to_owned()));
    }
    for (k, v) in c.raw.iter() {
        if k != "ShowCommentTags" && k != "xml:space" {
            attrs.push((k.to_owned(), v.to_owned()));
        }
    }
    let authors: Vec<Node> = c
        .authors()
        .map(|a| {
            // `Name` and `Initials` are written where they say something, or where the file had them (empty, kept in `raw`): never an attribute the file lacked.
            let mut attrs = vec![("ID".to_owned(), a.id.to_string())];
            if !a.name.is_empty() {
                attrs.push(("Name".to_owned(), a.name.to_string()));
            }
            if !a.initials.is_empty() {
                attrs.push(("Initials".to_owned(), a.initials.to_string()));
            }
            let kept: Vec<(String, String)> = a
                .raw
                .iter()
                .filter(|(k, v)| !(matches!(*k, "Name" | "Initials") && (!v.is_empty() || attr_written(&attrs, k))))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();
            attrs.extend(kept);
            Node::El(El { name: "AuthorEntry".to_owned(), attrs, children: Vec::new() })
        })
        .collect();
    let entries: Vec<Node> = c
        .iter()
        .map(|m| {
            let mut attrs = vec![("AuthorID".to_owned(), m.author.to_string()), ("PageID".to_owned(), m.page.to_string())];
            if let Some(shape) = m.shape {
                attrs.push(("ShapeID".to_owned(), shape.to_string()));
            }
            attrs.push(("Date".to_owned(), m.date.to_string()));
            if let Some(edit) = &m.edit_date {
                attrs.push(("EditDate".to_owned(), edit.to_string()));
            }
            attrs.push(("CommentID".to_owned(), m.id.to_string()));
            if m.done {
                attrs.push(("Done".to_owned(), "1".to_owned()));
            }
            // What another format kept beside a comment is prefixed (`svg:x` of an `.odg`); a VSDX attribute is not, and the part holds only those.
            attrs.extend(m.raw.iter().filter(|(k, _)| !k.contains(':')).map(|(k, v)| (k.to_owned(), v.to_owned())));
            Node::El(El { name: "CommentEntry".to_owned(), attrs, children: vec![Node::Text(m.text.to_string())] })
        })
        .collect();
    El {
        name: "Comments".to_owned(),
        attrs,
        children: vec![
            Node::El(El { name: "AuthorList".to_owned(), attrs: Vec::new(), children: authors }),
            Node::El(El { name: "CommentList".to_owned(), attrs: Vec::new(), children: entries }),
        ],
    }
}
