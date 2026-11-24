//! The comments of a drawing (ODF 1.4 Part 3 `office:annotation`): a page's `office:annotation` elements, read into
//! [`diacraft_doc::Document::comments`] and taken out of the page's kept markup, so that the model is the one place they are held.
//!
//! **Where an annotation is read.** ODF 1.4 puts `office:annotation` at the end of a `draw:page` (the schema's `draw-page`), and LibreOffice writes and reads the same
//! element for a page's comments, under its own `officeooo` prefix of `http://openoffice.org/2009/office` when it saves (it reads both). A drawing's annotation has no shape it
//! belongs to in ODF: **a comment of DiaCraft's on a shape names the shape by `diacraft:shape`**, and the page by the page it stands in. An annotation anywhere else (inside a
//! shape, inside a paragraph) is not read and stays in the markup it is in, as it did before this task.
//!
//! **What is read** is what a comment of the model holds, as ODF has it: `dc:creator` the author's name, `meta:creator-initials` the initials, `dc:date` the date, the `text:p`
//! children the text (a line for each paragraph; the white space of ODF: runs collapse, `text:s` is spaces, `text:tab` a tab, `text:line-break` a line), `svg:x`, `svg:y`, `svg:width`,
//! `svg:height`, `office:display` and `office:name` as attributes the model keeps beside the fields (under those names, whatever the prefixes were), and DiaCraft's own attributes
//! (`diacraft:version` 1, `id`, `shape`, `parent`, `done`, `edited`: the comment's number, its shape, the comment it answers, the Done mark and the date of its last edit). Authors are
//! made from the names and initials, the same pair one author, numbered from 1 in the order they are met. A comment with no `diacraft:id` is numbered after the highest the page
//! of the file gives, in order.
//!
//! **What is not read, and stays where it is.** An annotation is read only when the model holds it exactly: any attribute, child or node beyond the list above (a style, a
//! `text:span`, a list, a second creator, `meta:date-string`, an XML comment), a text of more than [`diacraft_doc::MAX_COMMENT_TEXT_BYTES`] bytes, and every annotation past the
//! caps of the model (100,000 comments, 10,000 authors, 32 MiB of text between them). Each is kept whole in the page's markup (the page's `passthrough`), counted as `annotation_kept`
//! and said once. **The writers put it back where it was**, and a save that rewrites a page's comments leaves it alone (the index says which of the page's annotation elements the
//! read took, [`crate::PageShapes`]).
//!
//! **Cost.** Every comment and author is charged to the read's items before it is made, by the sizes of its own type and the bytes of its strings; the work is one walk of the
//! annotation's own nodes, bounded by the parser's caps, and nothing here searches a list: authors and numbers are found in maps.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    Comment, CommentAuthor, Comments, Document, MAX_COMMENT_AUTHORS, MAX_COMMENT_TEXT_BYTES, MAX_COMMENTS, MAX_COMMENTS_TEXT_BYTES, RawAttrs,
};
use diacraft_sheetxml::passthrough::canonical_u32;
use diacraft_sheetxml::price::raw_attr_bytes;

use super::Reader;
use super::cost::{held, priced, slot};
use super::names::{Names, Ns};
use crate::error::{LimitKind, OdgError};

/// The attributes of an annotation the model keeps beside its fields, by the name they are kept under, and the namespace and local name each is read as.
pub(crate) const KEPT_ATTRS: [(&str, Ns, &str); 6] = [
    ("svg:x", Ns::Svg, "x"),
    ("svg:y", Ns::Svg, "y"),
    ("svg:width", Ns::Svg, "width"),
    ("svg:height", Ns::Svg, "height"),
    ("office:display", Ns::Office, "display"),
    ("office:name", Ns::Office, "name"),
];

/// The most spaces one `text:s` stands for that are read: a real one stands for a handful, and no paragraph may pass the comment's cap whatever its count says.
const MOST_SPACES: usize = MAX_COMMENT_TEXT_BYTES;

/// Items a comment costs beside its strings and its kept attributes: the comment behind its pointer, its place in the list of the document and its place in the table
/// the reader finds a comment's number by, by the law of the memory tests (`cost::priced`), and the strings it holds are charged by their bytes.
const COMMENT: usize = priced(slot(size_of::<Arc<Comment>>()) + 2 * held(size_of::<Comment>() + 16) + 2 * slot(size_of::<(u32, usize)>()));

/// Items an author costs beside the bytes of the name and the initials.
const AUTHOR: usize = priced(
    slot(size_of::<Arc<CommentAuthor>>()) + held(size_of::<CommentAuthor>() + 16) + slot(size_of::<((Arc<str>, Arc<str>), u32)>()) + 2 * held(32),
);

/// One annotation, read: the fields of a comment before it has a number, a page or an author.
#[derive(Debug)]
pub(crate) struct Annotation {
    pub creator: String,
    pub initials: String,
    pub date: String,
    pub text: String,
    /// The attributes kept beside the fields, under the names of [`KEPT_ATTRS`], in the order of that list.
    pub kept: Vec<(&'static str, String)>,
    pub id: Option<u32>,
    pub shape: Option<u32>,
    pub parent: Option<u32>,
    pub done: bool,
    pub edited: Option<String>,
}

/// What reading one page's annotations found: how many `office:annotation` children the page has, and the ones the model took, by their place among them.
#[derive(Debug, Default)]
pub(crate) struct PageAnnotations {
    /// The annotation children of the page seen so far, taken or not: the place the next one has among them.
    pub seen: u32,
    /// The annotations taken, each with the place it has among the page's annotation children.
    pub taken: Vec<(u32, Annotation)>,
}

/// What a read has taken for comments so far, across the pages: the caps are the document's.
#[derive(Debug, Default)]
pub(crate) struct Gather {
    comments: usize,
    text_bytes: usize,
    /// The authors met, by name and initials.
    authors: HashSet<(String, String)>,
    /// The pages' annotations, by the place of the page among the drawing's pages.
    pub pages: Vec<(usize, Vec<Annotation>)>,
}

/// Whether `ns` and `local` name an annotation: ODF's `office:annotation`, or LibreOffice's `officeooo:annotation`.
pub(crate) fn is_annotation(ns: Ns, local: &str) -> bool {
    matches!(ns, Ns::Office | Ns::Officeooo) && local == "annotation"
}

/// Reads the annotation `el`, a child of a page, for the model, or leaves it for the page's markup: `Ok(true)` when the model took it (the caller keeps no copy), `Ok(false)` when
/// it stays where it is. The annotation's place among the page's annotations is counted either way.
///
/// **What the file's bytes make is bounded before it is made.** Once the document holds the most comments or the most text it may, the annotations after are kept without being
/// parsed; a parse is given the text the document has room for and measures what a `text:s` stands for before it makes a byte of it, and what it makes is charged to the read
/// whether or not the annotation is kept (the security scan's finding: a few bytes of `text:s` stood for 64 KiB each).
pub(crate) fn take<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, page: &mut PageAnnotations) -> Result<bool, OdgError> {
    let place = page.seen;
    page.seen = page.seen.saturating_add(1);
    r.saw_attrs(el.attrs.len());
    let g = &r.gather;
    let room = MAX_COMMENTS_TEXT_BYTES.saturating_sub(g.text_bytes);
    if g.comments >= MAX_COMMENTS || room == 0 {
        r.warn_counted("annotation_kept", 1, || {
            "a comment of the page is past what the model holds (100,000 comments, 10,000 authors, 32 MiB of text) and stays in the page's markup"
                .to_owned()
        });
        return Ok(false);
    }
    let Some(read) = parse(r, names, el, room.min(MAX_COMMENT_TEXT_BYTES))? else {
        r.warn_counted("annotation_kept", 1, || {
            "a comment of the page holds more than the model keeps of one (formatting, a list, a style, a second author or a text past the cap), and stays in the page's markup".to_owned()
        });
        return Ok(false);
    };
    // The caps of the model, before anything is made.
    let g = &mut r.gather;
    let pair = (read.creator.clone(), read.initials.clone());
    let new_author = !g.authors.contains(&pair);
    let over = (new_author && g.authors.len() >= MAX_COMMENT_AUTHORS) || g.text_bytes.saturating_add(read.text.len()) > MAX_COMMENTS_TEXT_BYTES;
    if over {
        r.warn_counted("annotation_kept", 1, || {
            "a comment of the page is past what the model holds (100,000 comments, 10,000 authors, 32 MiB of text) and stays in the page's markup"
                .to_owned()
        });
        return Ok(false);
    }
    // The memory the model keeps for it, charged before it is made.
    let mut bytes = read.text.len().saturating_add(read.date.len()).saturating_add(read.edited.as_ref().map_or(0, String::len));
    for (at, (name, value)) in read.kept.iter().enumerate() {
        bytes = bytes.saturating_add(usize::try_from(raw_attr_bytes(at, name.len(), value.len())).unwrap_or(usize::MAX));
    }
    let mut items = COMMENT;
    if new_author {
        items = items.saturating_add(AUTHOR);
        bytes = bytes.saturating_add(pair.0.len()).saturating_add(pair.1.len());
    }
    r.take_items(items)?;
    r.take_bytes(bytes)?;
    let g = &mut r.gather;
    g.comments += 1;
    g.text_bytes = g.text_bytes.saturating_add(read.text.len());
    if new_author {
        g.authors.insert(pair);
    }
    page.taken.push((place, read));
    Ok(true)
}

/// The annotation `el` as the model holds it, or `None` when it holds more than the model does or a text of more than `room` bytes. Err only for a read that is out of items.
fn parse<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, room: usize) -> Result<Option<Annotation>, OdgError> {
    let mark = names.enter(el);
    let out = parse_inner(r, names, el, room);
    names.leave(mark);
    out
}

/// The text a comment is being made of: the bytes it comes to, the room it may take, and the text itself when it is being made and not only measured.
pub(crate) struct Sink {
    text: Option<String>,
    pub(crate) len: usize,
    left: usize,
}

impl Sink {
    pub(crate) fn measuring(left: usize) -> Sink {
        Sink { text: None, len: 0, left }
    }

    /// Whether `n` more bytes are within the room.
    pub(crate) fn room(&self, n: usize) -> bool {
        self.len.checked_add(n).is_some_and(|total| total <= self.left)
    }

    pub(crate) fn char(&mut self, c: char) -> Option<()> {
        if !self.room(c.len_utf8()) {
            return None;
        }
        self.len += c.len_utf8();
        if let Some(text) = &mut self.text {
            text.push(c);
        }
        Some(())
    }

    /// `n` spaces, refused before any is made when they are past the room.
    pub(crate) fn spaces(&mut self, n: usize) -> Option<()> {
        if !self.room(n) {
            return None;
        }
        self.len += n;
        if let Some(text) = &mut self.text {
            text.extend(std::iter::repeat_n(' ', n));
        }
        Some(())
    }
}

fn parse_inner<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, room: usize) -> Result<Option<Annotation>, OdgError> {
    let mut read = Annotation {
        creator: String::new(),
        initials: String::new(),
        date: String::new(),
        text: String::new(),
        kept: Vec::new(),
        id: None,
        shape: None,
        parent: None,
        done: false,
        edited: None,
    };
    // The attributes: each is one of the list, or the annotation stays where it is.
    let attrs = names.attrs(el);
    let mut seen_version = false;
    for a in attrs.iter() {
        if let Some(&(kept, _, _)) = KEPT_ATTRS.iter().find(|(_, ns, local)| *ns == a.ns && *local == a.local) {
            if read.kept.iter().any(|(k, _)| *k == kept) {
                return Ok(None);
            }
            read.kept.push((kept, a.value.to_owned()));
            continue;
        }
        if a.ns != Ns::Diacraft {
            return Ok(None);
        }
        match a.local {
            "version" if a.value == crate::ext::names::VERSION => seen_version = true,
            "id" => read.id = canonical_u32(a.value).filter(|n| *n <= MOST_ID),
            "shape" => read.shape = canonical_u32(a.value),
            "parent" => read.parent = canonical_u32(a.value).filter(|n| *n <= MOST_ID),
            "done" => read.done = a.value == "1",
            "edited" => read.edited = Some(a.value.to_owned()),
            _ => return Ok(None),
        }
    }
    // The order of the model's own list, whatever the file's.
    read.kept.sort_by_key(|(name, _)| KEPT_ATTRS.iter().position(|(k, _, _)| k == name).unwrap_or(usize::MAX));
    if !seen_version {
        // DiaCraft's attributes come with their version; without it they are another program's and are not read.
        (read.id, read.shape, read.parent, read.done, read.edited) = (None, None, None, false, None);
    }
    // The children: the creator, the date and the initials once each, and the paragraphs. **Each is looked at and measured first**, and the text of the paragraphs is made
    // only when every child is one the model holds and the text is within the room: a rich annotation costs what its bytes do, whatever its `text:s` stand for.
    let (mut creator, mut date, mut initials) = (false, false, false);
    let mut paragraphs: Vec<&'t El> = Vec::new();
    let mut sink = Sink::measuring(room);
    for node in &el.children {
        match node {
            Node::Text(t) | Node::CData(t) => {
                if !t.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                    return Ok(None);
                }
            }
            Node::Comment(_) | Node::Pi { .. } => return Ok(None),
            Node::El(child) => {
                r.saw_element();
                let mark = names.enter(child);
                let out = match names.element(child) {
                    (Ns::Dc, "creator") if !creator => {
                        creator = true;
                        plain_field(names, child).map(|t| read.creator = t)
                    }
                    (Ns::Dc, "date") if !date => {
                        date = true;
                        plain_field(names, child).map(|t| read.date = t)
                    }
                    (Ns::Meta, "creator-initials") if !initials => {
                        initials = true;
                        plain_field(names, child).map(|t| read.initials = t)
                    }
                    (Ns::Text, "p") => {
                        let line = if paragraphs.is_empty() { Some(()) } else { sink.char('\n') };
                        line.and_then(|()| paragraph(r, names, child, &mut sink)).map(|()| paragraphs.push(child))
                    }
                    _ => None,
                };
                names.leave(mark);
                if out.is_none() {
                    return Ok(None);
                }
            }
        }
    }
    // The text is what the file's bytes make, and is charged before it is made, whether or not the annotation is kept after.
    r.take_model_bytes(sink.len)?;
    let mut made = Sink { text: Some(String::with_capacity(sink.len)), len: 0, left: sink.len };
    for (at, p) in paragraphs.iter().enumerate() {
        if at > 0 && made.char('\n').is_none() {
            return Ok(None);
        }
        if paragraph(r, names, p, &mut made).is_none() {
            return Ok(None);
        }
    }
    read.text = made.text.unwrap_or_default();
    Ok(Some(read))
}

/// The largest number of a comment this reader takes from a file: no number a file gives can make the next free one overflow.
const MOST_ID: u32 = 1 << 31;

/// The text of a field element (`dc:creator`, `dc:date`, `meta:creator-initials`): no attribute but a declaration, no child element, and the text with its ends trimmed.
fn plain_field<'t>(names: &Names<'t>, el: &'t El) -> Option<String> {
    if names.attrs(el).len() > 0 {
        return None;
    }
    let mut out = String::new();
    for node in &el.children {
        match node {
            Node::Text(t) | Node::CData(t) => {
                if out.len().saturating_add(t.len()) > MAX_COMMENT_TEXT_BYTES {
                    return None;
                }
                out.push_str(t);
            }
            _ => return None,
        }
    }
    Some(out.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r')).to_owned())
}

/// The characters of a `text:p`, by the white space of ODF, added to `sink`: `None` when it holds more than characters, `text:s`, `text:tab` and `text:line-break`, or when what it
/// comes to is past the room the sink has (a `text:s` is refused before its spaces are made).
fn paragraph<'t>(r: &mut Reader, names: &mut Names<'t>, p: &'t El, sink: &mut Sink) -> Option<()> {
    if names.attrs(p).len() > 0 {
        return None;
    }
    // White space is collapsed to one space that is written only when more content follows (a leading and a trailing run is dropped, and `text:s` writes what it says).
    let (mut pending, mut started) = (false, false);
    for node in &p.children {
        match node {
            Node::Text(t) | Node::CData(t) => {
                for c in t.chars() {
                    if matches!(c, ' ' | '\t' | '\n' | '\r') {
                        pending = started;
                    } else {
                        if pending {
                            sink.char(' ')?;
                            pending = false;
                        }
                        sink.char(c)?;
                        started = true;
                    }
                }
            }
            Node::Comment(_) | Node::Pi { .. } => return None,
            Node::El(e) => {
                r.saw_element();
                let mark = names.enter(e);
                let kind = names.element(e);
                let ok = e.elements().next().is_none() && e.children.is_empty();
                let attrs = names.attrs(e);
                let step = match (kind, ok) {
                    ((Ns::Text, "s"), true) if attrs.len() == 0 => Some(1),
                    ((Ns::Text, "s"), true) if attrs.len() == 1 => {
                        attrs.peek(Ns::Text, "c").and_then(|c| c.parse::<usize>().ok()).filter(|n| (1..=MOST_SPACES).contains(n))
                    }
                    ((Ns::Text, "tab" | "line-break"), true) if attrs.len() == 0 => Some(0),
                    _ => None,
                };
                let local = kind.1;
                names.leave(mark);
                let n = step?;
                if pending {
                    sink.char(' ')?;
                    pending = false;
                }
                match local {
                    "s" => sink.spaces(n)?,
                    "tab" => sink.char('\t')?,
                    _ => sink.char('\n')?,
                }
                started = true;
            }
        }
    }
    Some(())
}

/// Makes the document's comments of the annotations the pages gave, now that the pages have the numbers the document gives them. Nothing the caps allow is refused here: the
/// reader took only what the model holds.
pub(crate) fn finish(r: &mut Reader, doc: &mut Document) -> Result<(), OdgError> {
    let gathered = std::mem::take(&mut r.gather.pages);
    if gathered.is_empty() {
        return Ok(());
    }
    let refused = || OdgError::Limit { kind: LimitKind::Items, max: MAX_COMMENTS as u64 };
    // The authors, numbered from 1 in the order they are first met.
    let mut author_ids: HashMap<(&str, &str), u32> = HashMap::new();
    let mut authors: Vec<CommentAuthor> = Vec::new();
    // The numbers the file gives, taken by the first comment that names each; the others are numbered after the highest.
    let mut taken: HashSet<u32> = HashSet::new();
    let mut highest = 0_u32;
    for (_, list) in &gathered {
        for a in list {
            if let Some(id) = a.id
                && taken.insert(id)
            {
                highest = highest.max(id);
            }
        }
    }
    let mut next = highest;
    let mut used: HashSet<u32> = HashSet::new();
    let mut comments: Vec<Comment> = Vec::new();
    // Where each comment is, by its number, for the replies: the page, the shape, and the place in `comments`.
    let mut by_id: HashMap<u32, (u32, Option<u32>)> = HashMap::new();
    for (page_at, list) in &gathered {
        let Some(page) = doc.pages.get(*page_at).map(|p| p.id) else { continue };
        for a in list {
            let id = match a.id.filter(|id| !used.contains(id)) {
                Some(id) => id,
                None => {
                    next = next.checked_add(1).ok_or_else(refused)?;
                    next
                }
            };
            used.insert(id);
            let key = (a.creator.as_str(), a.initials.as_str());
            let author = match author_ids.get(&key) {
                Some(n) => *n,
                None => {
                    let n = u32::try_from(authors.len()).map_err(|_| refused())?.checked_add(1).ok_or_else(refused)?;
                    author_ids.insert(key, n);
                    authors.push(CommentAuthor {
                        id: n,
                        name: Arc::from(a.creator.as_str()),
                        initials: Arc::from(a.initials.as_str()),
                        raw: RawAttrs::new(),
                    });
                    n
                }
            };
            let mut raw = RawAttrs::new();
            for (name, value) in &a.kept {
                raw.push(name, value);
            }
            by_id.insert(id, (page, a.shape));
            comments.push(Comment {
                id,
                parent: a.parent,
                page,
                shape: a.shape,
                author,
                text: Arc::from(a.text.as_str()),
                date: Arc::from(a.date.as_str()),
                edit_date: a.edited.as_deref().map(Arc::from),
                done: a.done,
                raw,
            });
        }
    }
    // A reply sits where its comment sits: a parent on another page or another shape, or none, leaves the comment a thread's first.
    for c in &mut comments {
        if let Some(parent) = c.parent
            && by_id.get(&parent) != Some(&(c.page, c.shape))
        {
            c.parent = None;
        }
    }
    doc.comments = Comments::from_parts(None, RawAttrs::new(), authors, comments).map_err(|_| refused())?;
    Ok(())
}
