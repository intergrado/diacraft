//! The comments of a document: what a reviewer says about a shape or a page.
//!
//! The VSDX comments part is an author list and a comment list, each comment with the page and
//! optionally the shape it is on, its author, its dates and its plain text. The window's rules: a comment on
//! a shape stays with the shape when it moves (it names the shape by its id, so no move can part them), a comment with no shape is the page's, and
//! the Comments pane lists comments with their replies. VSDX's comments part has no replies, so a reply is DiaCraft's own and is a comment with a
//! `parent` (the save notes of each format say what they keep of it).
//!
//! **The model.** [`Comments`] is two persistent lists ([`SharedVec`]): the authors and the comments, in file order. A clone of a document copies
//! two pointers, and an edit of one comment copies one chunk of the list, so a drawing of 100,000 comments is not copied by an undo step each.
//! **A thread is one level deep:** a reply's `parent` is a comment that has no parent, and a reply to a reply joins the thread of the comment it
//! answers. A reply sits where its comment sits (same page, same shape).
//!
//! **Caps** (project rules; no spec gives them): at most [`MAX_COMMENTS`] comments, [`MAX_COMMENT_AUTHORS`] authors, [`MAX_COMMENT_TEXT_BYTES`]
//! bytes of text in a comment and [`MAX_COMMENTS_TEXT_BYTES`] bytes of text between all of them. They hold at each door: [`Comments::from_parts`]
//! is the door a file enters by, and [`Comments::add`], [`Comments::edit`] and [`Comments::author_for`] are the commands'. A walk over the comments
//! is one pass; nothing here searches the list once for each comment (the ids of a file are checked with a set).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::raw::RawAttrs;
use crate::shared::{Iter, SharedVec};

/// Most comments a document holds: 100,000. A project rule: a real drawing holds a few dozen, and a pane of a hundred thousand is no pane.
pub const MAX_COMMENTS: usize = 100_000;

/// Most authors a document holds: 10,000. A project rule.
pub const MAX_COMMENT_AUTHORS: usize = 10_000;

/// Most bytes of text in one comment: 64 KiB. A project rule: a comment is a note, and the window shows it in a card.
pub const MAX_COMMENT_TEXT_BYTES: usize = 64 * 1024;

/// Most bytes of text between all the comments of a document: 32 MiB. A project rule.
pub const MAX_COMMENTS_TEXT_BYTES: usize = 32 * 1024 * 1024;

/// The steps a walk up a chain of replies takes at most before it stops (a hostile `.diacraft` file can hold a chain as long as the file).
const MAX_THREAD_STEPS: usize = 64;

/// What can be wrong with a comment, an author or a list of them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommentError {
    #[error("a document holds at most {max} comments", max = MAX_COMMENTS)]
    TooMany,
    #[error("a document holds at most {max} comment authors", max = MAX_COMMENT_AUTHORS)]
    TooManyAuthors,
    #[error("a comment must be at most {max} bytes of text", max = MAX_COMMENT_TEXT_BYTES)]
    TextTooLong,
    #[error("the comments of a document hold at most {max} bytes of text between them", max = MAX_COMMENTS_TEXT_BYTES)]
    TotalTextTooLong,
    #[error("a comment needs some text")]
    EmptyText,
    #[error("a comment's text must not contain a control character other than a line break or a tab")]
    ControlCharacter,
    #[error("no comment has id {0}")]
    NoSuchComment(u32),
    #[error("no comment author has id {0}")]
    NoSuchAuthor(u32),
    #[error("comment id {0} is used by more than one comment")]
    DuplicateId(u32),
    #[error("comment author id {0} is used by more than one author")]
    DuplicateAuthorId(u32),
    #[error("no comment ids are left")]
    IdsExhausted,
}

/// One author of the comments (`AuthorEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentAuthor {
    /// `ID`: at least 1 and unique among the authors.
    pub id: u32,
    pub name: Arc<str>,
    pub initials: Arc<str>,
    /// `ResolutionID`, `SMTP`, `SIP` and any other attribute, as the file gave them.
    pub raw: RawAttrs,
}

/// One comment (`CommentEntry`), or a reply to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// `CommentID`: unique among the comments, and kept as it is across saves.
    pub id: u32,
    /// The comment this one answers, which has no parent itself: DiaCraft's own, as VSDX's comments part has no replies.
    pub parent: Option<u32>,
    /// `PageID`.
    pub page: u32,
    /// `ShapeID`; `None` is a comment on the page.
    pub shape: Option<u32>,
    /// `AuthorID`: an author of the list, or not (a file may name one it does not list).
    pub author: u32,
    /// The text, with `\n` between its lines.
    pub text: Arc<str>,
    /// `Date`: when it was written, as the file gave it (`2031-03-07T20:05:09Z` for one DiaCraft writes).
    pub date: Arc<str>,
    /// `EditDate`: when the text last changed.
    pub edit_date: Option<Arc<str>>,
    /// `Done`: the comment is marked as dealt with.
    pub done: bool,
    /// `AutoCommentType` and any other attribute, as the file gave them.
    pub raw: RawAttrs,
}

/// What [`Comments::add`] needs of a new comment: the rest (its id) is made there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewComment {
    /// The comment answered, or `None` for a new thread. The new comment takes the page and shape of its thread, and `page` and `shape` below are
    /// for a thread's first comment only.
    pub parent: Option<u32>,
    pub page: u32,
    pub shape: Option<u32>,
    pub author: u32,
    pub text: Arc<str>,
    pub date: Arc<str>,
}

/// The comments of a document: its authors and its comments (`Comments`).
#[derive(Clone, Debug, Default)]
pub struct Comments {
    /// `ShowCommentTags`: whether the markers of the comments show on the page. `None` is the attribute left out, and the markers show.
    pub show_tags: Option<bool>,
    /// The attributes of the `Comments` element the model has no field for.
    pub raw: RawAttrs,
    /// The name of the package part the comments were read from (`/visio/comments.xml`), as the package gave it: **the one part a save into that package rewrites** (the writers never take a part
    /// from a relationship alone, which a file can point anywhere). `None` for comments that were not read from a package part. Not content: two documents that differ in it only are equal.
    pub part: Option<Arc<str>>,
    authors: SharedVec<Arc<CommentAuthor>>,
    items: SharedVec<Arc<Comment>>,
    /// The bytes of the text of every comment, kept as comments come and go so that a cap is one comparison.
    text_bytes: u64,
}

impl PartialEq for Comments {
    fn eq(&self, other: &Comments) -> bool {
        // The part's name is where a package keeps the comments, not what they say.
        self.show_tags == other.show_tags
            && self.raw == other.raw
            && self.authors == other.authors
            && self.items == other.items
            && self.text_bytes == other.text_bytes
    }
}

/// Whether `text` holds a character a comment may not: a control character but a line break or a tab, and the two characters XML cannot hold.
pub(crate) fn bad_char(c: char) -> bool {
    (c.is_control() && !matches!(c, '\n' | '\t' | '\r')) || matches!(c, '\u{fffe}' | '\u{ffff}')
}

/// The text of a comment as it is kept: its line breaks `\n`. The bytes are checked against the cap before any copy is made.
pub(crate) fn clean(text: &str) -> Result<Arc<str>, CommentError> {
    if text.len() > MAX_COMMENT_TEXT_BYTES {
        return Err(CommentError::TextTooLong);
    }
    if text.trim().is_empty() {
        return Err(CommentError::EmptyText);
    }
    if text.chars().any(bad_char) {
        return Err(CommentError::ControlCharacter);
    }
    if text.contains('\r') {
        return Ok(Arc::from(text.replace("\r\n", "\n").replace('\r', "\n")));
    }
    Ok(Arc::from(text))
}

impl Comments {
    /// No comments, no authors.
    pub fn new() -> Comments {
        Comments::default()
    }

    /// Whether there is nothing at all: no comment, no author and no setting. A document that holds none writes no comments part.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.authors.is_empty() && self.show_tags.is_none() && self.raw.is_empty()
    }

    /// How many comments (replies included).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The comments, in order.
    pub fn iter(&self) -> Iter<'_, Arc<Comment>> {
        self.items.iter()
    }

    /// The authors, in order.
    pub fn authors(&self) -> Iter<'_, Arc<CommentAuthor>> {
        self.authors.iter()
    }

    /// The comment with `id`. A scan of the list: at most [`MAX_COMMENTS`] steps.
    pub fn get(&self, id: u32) -> Option<&Arc<Comment>> {
        self.items.iter().find(|c| c.id == id)
    }

    /// The author with `id`.
    pub fn author(&self, id: u32) -> Option<&Arc<CommentAuthor>> {
        self.authors.iter().find(|a| a.id == id)
    }

    /// How many chunks of the comment list are the very chunks `other` holds: what an edit left shared with an older copy.
    pub fn shared_chunks_with(&self, other: &Comments) -> usize {
        self.items.shared_chunks_with(&other.items)
    }

    /// The bytes of text all the comments hold.
    pub fn text_bytes(&self) -> u64 {
        self.text_bytes
    }

    /// The list of comments, behind the pointer a clone shares it by (`None` when there are none): what a count of the memory a model holds counts by.
    pub fn items_list(&self) -> &SharedVec<Arc<Comment>> {
        &self.items
    }

    /// The comments of a file: every list is checked once (the caps, a repeated id), and the replies are made one level deep. Nothing is refused for a
    /// comment that names an author, a page or a shape the file does not have: a file may, and the comment is kept.
    ///
    /// # Errors
    /// A cap passed, or an id used twice. A reader keeps the part as bytes then, and says so.
    pub fn from_parts(
        show_tags: Option<bool>,
        raw: RawAttrs,
        authors: Vec<CommentAuthor>,
        mut comments: Vec<Comment>,
    ) -> Result<Comments, CommentError> {
        if authors.len() > MAX_COMMENT_AUTHORS {
            return Err(CommentError::TooManyAuthors);
        }
        if comments.len() > MAX_COMMENTS {
            return Err(CommentError::TooMany);
        }
        let mut seen: HashSet<u32> = HashSet::with_capacity(authors.len());
        for a in &authors {
            if !seen.insert(a.id) {
                return Err(CommentError::DuplicateAuthorId(a.id));
            }
        }
        let mut ids: HashMap<u32, usize> = HashMap::with_capacity(comments.len());
        let mut text_bytes: u64 = 0;
        for (at, c) in comments.iter().enumerate() {
            if ids.insert(c.id, at).is_some() {
                return Err(CommentError::DuplicateId(c.id));
            }
            if c.text.len() > MAX_COMMENT_TEXT_BYTES {
                return Err(CommentError::TextTooLong);
            }
            text_bytes = text_bytes.saturating_add(c.text.len() as u64);
        }
        if text_bytes > MAX_COMMENTS_TEXT_BYTES as u64 {
            return Err(CommentError::TotalTextTooLong);
        }
        straighten(&mut comments, &ids);
        Ok(Comments {
            show_tags,
            raw,
            part: None,
            authors: SharedVec::from_vec(authors.into_iter().map(Arc::new).collect()),
            items: SharedVec::from_vec(comments.into_iter().map(Arc::new).collect()),
            text_bytes,
        })
    }

    /// The id of the author called `name` with these `initials`, added when the list has none like it.
    ///
    /// # Errors
    /// [`CommentError::TooManyAuthors`] when it would be a new author past the cap.
    pub fn author_for(&mut self, name: &str, initials: &str) -> Result<u32, CommentError> {
        if let Some(found) = self.authors.iter().find(|a| &*a.name == name && &*a.initials == initials) {
            return Ok(found.id);
        }
        if self.authors.len() >= MAX_COMMENT_AUTHORS {
            return Err(CommentError::TooManyAuthors);
        }
        let id = self.authors.iter().map(|a| a.id).max().map_or(Some(1), |m| m.checked_add(1)).ok_or(CommentError::IdsExhausted)?;
        self.authors.push(Arc::new(CommentAuthor { id, name: Arc::from(name), initials: Arc::from(initials), raw: RawAttrs::new() }));
        Ok(id)
    }

    /// Adds a comment, and returns its id: one more than the highest in use.
    ///
    /// # Errors
    /// A cap passed, no text, a character a comment may not hold, an unknown author or an unknown comment to answer. Nothing changes then.
    pub fn add(&mut self, new: NewComment) -> Result<u32, CommentError> {
        let text = clean(&new.text)?;
        if self.items.len() >= MAX_COMMENTS {
            return Err(CommentError::TooMany);
        }
        let Some(total) = text_total(self.text_bytes, 0, text.len() as u64) else {
            return Err(CommentError::TotalTextTooLong);
        };
        if self.author(new.author).is_none() {
            return Err(CommentError::NoSuchAuthor(new.author));
        }
        // A reply joins the thread of the comment it answers, and sits where that comment sits.
        let (parent, page, shape) = match new.parent {
            None => (None, new.page, new.shape),
            Some(answered) => {
                let to = self.get(answered).ok_or(CommentError::NoSuchComment(answered))?;
                let root_id = to.parent.unwrap_or(to.id);
                let root = if root_id == to.id { to } else { self.get(root_id).unwrap_or(to) };
                (Some(root.id), root.page, root.shape)
            }
        };
        let id = self.items.iter().map(|c| c.id).max().map_or(Some(1), |m| m.checked_add(1)).ok_or(CommentError::IdsExhausted)?;
        self.text_bytes = total;
        self.items.push(Arc::new(Comment {
            id,
            parent,
            page,
            shape,
            author: new.author,
            text,
            date: new.date,
            edit_date: None,
            done: false,
            raw: RawAttrs::new(),
        }));
        Ok(id)
    }

    /// Changes the text of a comment and its edit date.
    ///
    /// # Errors
    /// No such comment, no text, a character a comment may not hold, or a cap passed. Nothing changes then.
    pub fn edit(&mut self, id: u32, text: &str, edit_date: Arc<str>) -> Result<(), CommentError> {
        let text = clean(text)?;
        let at = self.items.iter().position(|c| c.id == id).ok_or(CommentError::NoSuchComment(id))?;
        let old = self.items.get(at).map_or(0, |c| c.text.len() as u64);
        let total = text_total(self.text_bytes, old, text.len() as u64).ok_or(CommentError::TotalTextTooLong)?;
        let slot = self.items.get_mut(at).ok_or(CommentError::NoSuchComment(id))?;
        let mut next = Comment::clone(slot);
        next.text = text;
        next.edit_date = Some(edit_date);
        *slot = Arc::new(next);
        self.text_bytes = total;
        Ok(())
    }

    /// Marks a comment done or not done, and sets its edit date.
    ///
    /// # Errors
    /// No such comment.
    pub fn set_done(&mut self, id: u32, done: bool, edit_date: Arc<str>) -> Result<(), CommentError> {
        let at = self.items.iter().position(|c| c.id == id).ok_or(CommentError::NoSuchComment(id))?;
        let slot = self.items.get_mut(at).ok_or(CommentError::NoSuchComment(id))?;
        if slot.done == done {
            return Ok(());
        }
        let mut next = Comment::clone(slot);
        next.done = done;
        next.edit_date = Some(edit_date);
        *slot = Arc::new(next);
        Ok(())
    }

    /// Makes replies of the comments `links` name, `(reply, comment answered)`, and says how many were made (a VSDX file's comments part holds no thread, and DiaCraft's own
    /// part says which comments answer which). A link is made only when both comments are here, the reply is not the comment it answers, neither is a reply already or named as one
    /// by another link, and they are on the same page and the same shape: any other is left out. One pass of the list whatever the number of links.
    pub fn apply_threads(&mut self, links: &[(u32, u32)]) -> usize {
        if links.is_empty() {
            return 0;
        }
        let at_of: HashMap<u32, usize> = self.items.iter().enumerate().map(|(at, c)| (c.id, at)).collect();
        let replies: HashSet<u32> = links.iter().filter(|l| l.0 != l.1).map(|l| l.0).collect();
        let mut made: HashMap<usize, u32> = HashMap::with_capacity(links.len().min(self.items.len()));
        for &(reply, answered) in links {
            if reply == answered || replies.contains(&answered) {
                continue;
            }
            let (Some(&r), Some(&a)) = (at_of.get(&reply), at_of.get(&answered)) else { continue };
            let (Some(rc), Some(ac)) = (self.items.get(r), self.items.get(a)) else { continue };
            if rc.parent.is_none() && ac.parent.is_none() && rc.page == ac.page && rc.shape == ac.shape {
                made.insert(r, answered);
            }
        }
        if made.is_empty() {
            return 0;
        }
        let rebuilt: Vec<Arc<Comment>> = self
            .items
            .iter()
            .enumerate()
            .map(|(at, c)| match made.get(&at) {
                Some(&answered) => {
                    let mut next = Comment::clone(c);
                    next.parent = Some(answered);
                    Arc::new(next)
                }
                None => Arc::clone(c),
            })
            .collect();
        self.items = SharedVec::from_vec(rebuilt);
        made.len()
    }

    /// Takes a comment and the replies to it out, and says how many went (a reply alone goes alone).
    ///
    /// # Errors
    /// No such comment.
    pub fn remove(&mut self, id: u32) -> Result<usize, CommentError> {
        if self.get(id).is_none() {
            return Err(CommentError::NoSuchComment(id));
        }
        Ok(self.drop_where(|c| c.id == id || c.parent == Some(id)))
    }

    /// Takes out the comments of the page `page`, replies included, and says how many went. A page taken out of the drawing takes its comments.
    pub fn remove_page(&mut self, page: u32) -> usize {
        self.drop_where(|c| c.page == page)
    }

    /// Takes out the comments on the shapes `shapes` of the page `page`, and the replies to them (a reply sits where its comment sits, so they are
    /// among them), and says how many went. Shapes taken out of the drawing take their comments.
    pub fn remove_shapes(&mut self, page: u32, shapes: &BTreeSet<u32>) -> usize {
        if shapes.is_empty() {
            return 0;
        }
        self.drop_where(|c| c.page == page && c.shape.is_some_and(|s| shapes.contains(&s)))
    }

    /// Takes out every comment `gone` says, and says how many went. A few comments of a long list are taken out one at a time from the end, each
    /// rebuilding only the chunks from its place on; many are taken out by one rebuild of the list from the first of them on.
    fn drop_where(&mut self, gone: impl Fn(&Comment) -> bool) -> usize {
        let Some(first) = self.items.iter().position(|c| gone(c)) else { return 0 };
        let (mut dropped, mut freed) = (0_usize, 0_u64);
        for c in self.items.iter().skip(first).filter(|c| gone(c)) {
            dropped += 1;
            freed = freed.saturating_add(c.text.len() as u64);
        }
        let tail = self.items.len().saturating_sub(first);
        if dropped.saturating_mul(8) < tail.max(8) {
            let mut at = self.items.len();
            while at > first {
                at -= 1;
                if self.items.get(at).is_some_and(|c| gone(c)) {
                    self.items.remove(at);
                }
            }
        } else {
            let rebuilt: Vec<Arc<Comment>> = self.items.iter().filter(|c| !gone(c)).cloned().collect();
            self.items = SharedVec::from_vec(rebuilt);
        }
        self.text_bytes = self.text_bytes.saturating_sub(freed);
        dropped
    }
}

/// The bytes of text the comments hold once a text of `old` bytes among them is replaced by one of `new` bytes (`old` is 0 for a comment added), or `None` past
/// [`MAX_COMMENTS_TEXT_BYTES`].
pub(crate) fn text_total(held: u64, old: u64, new: u64) -> Option<u64> {
    let total = held.saturating_sub(old).saturating_add(new);
    (total <= MAX_COMMENTS_TEXT_BYTES as u64).then_some(total)
}

/// Makes every reply's parent a comment that has none, in one pass: a parent that is not in the list, a comment its own parent and a chain that
/// loops, or is longer than [`MAX_THREAD_STEPS`], leave the comment a thread's first; a longer chain that ends in one moves the reply to it.
fn straighten(comments: &mut [Comment], ids: &HashMap<u32, usize>) {
    let parents: Vec<Option<u32>> = comments.iter().map(|c| c.parent).collect();
    for (at, c) in comments.iter_mut().enumerate() {
        let Some(first) = parents.get(at).copied().flatten() else { continue };
        c.parent = thread_root(c.id, first, &parents, |id| ids.get(&id).copied());
    }
}

/// The first comment of the thread of the comment `id` whose parent is `first` ([`straighten`]'s walk): up the parents of the list (`parents`, in its order, a comment found
/// by its id at the place `place` gives), at most [`MAX_THREAD_STEPS`] steps, to a comment that has none. `None` when a parent is not in the list, or the chain comes back to the
/// comment, reaches a comment that is its own parent, or is longer.
pub(crate) fn thread_root(id: u32, first: u32, parents: &[Option<u32>], place: impl Fn(u32) -> Option<usize>) -> Option<u32> {
    let mut cursor = first;
    let mut root: Option<u32> = None;
    for _ in 0..MAX_THREAD_STEPS {
        let Some(idx) = place(cursor) else { break };
        match parents.get(idx).copied().flatten() {
            None => {
                root = Some(cursor);
                break;
            }
            Some(next) if next == id || next == cursor => break,
            Some(next) => cursor = next,
        }
    }
    root.filter(|r| *r != id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn comment(id: u32, page: u32, shape: Option<u32>) -> Comment {
        Comment {
            id,
            parent: None,
            page,
            shape,
            author: 1,
            text: Arc::from("t"),
            date: Arc::from("d"),
            edit_date: None,
            done: false,
            raw: RawAttrs::new(),
        }
    }

    fn comments(list: Vec<Comment>) -> Comments {
        let author = CommentAuthor { id: 1, name: Arc::from("A"), initials: Arc::from("A"), raw: RawAttrs::new() };
        Comments::from_parts(None, RawAttrs::new(), vec![author], list).unwrap()
    }

    #[test]
    fn threads_are_applied_where_both_comments_are_on_one_page_and_shape_and_nowhere_else() {
        let mut c = comments(vec![
            comment(1, 1, Some(4)),
            comment(2, 1, Some(4)),
            comment(3, 1, Some(5)),
            comment(4, 2, Some(4)),
            comment(5, 1, Some(4)),
            comment(6, 1, Some(4)),
        ]);
        // 2 answers 1 (made); 3 is on another shape; 4 is on another page; 9 is no comment; 5 and 6 answer each other (a reply is no comment answered); 1 answers itself.
        let made = c.apply_threads(&[(2, 1), (3, 1), (4, 1), (9, 1), (5, 6), (6, 5), (1, 1)]);
        assert_eq!(made, 1);
        let parents: Vec<Option<u32>> = c.iter().map(|x| x.parent).collect();
        assert_eq!(parents, [None, Some(1), None, None, None, None]);
        // A thread already made is not made again, nor answered.
        assert_eq!(c.apply_threads(&[(2, 1)]), 0);
        assert_eq!(c.apply_threads(&[(5, 2)]), 0, "a reply is no comment to answer");
        assert_eq!(c.apply_threads(&[]), 0);
    }

    #[test]
    fn applying_threads_leaves_the_rest_of_the_list_shared() {
        let mut c = comments((1..=200).map(|id| comment(id, 1, None)).collect());
        let before = c.clone();
        assert_eq!(c.apply_threads(&[(200, 1)]), 1);
        assert_eq!(c.get(200).map(|x| x.parent), Some(Some(1)));
        assert_eq!(before.get(200).map(|x| x.parent), Some(None), "the older copy is as it was");
        assert_eq!(c.text_bytes(), before.text_bytes());
    }
}
