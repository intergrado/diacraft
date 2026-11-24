//! The comments of a document as the elements `comment-list`, `comment-author` and `comment-entry` of `vocab.rs` and back.
//!
//! The list stands in the document part, after the solution. A comment keeps what it has no field for in its `attr` children and what a newer build
//! wrote on it under `comment-entry[<id>]` in the document's native slot, as a custom property does (NP7). A decode is charged before anything is built: the
//! element and its attributes, the bytes of each text, and the count of authors and comments against the model's caps, before a comment is made.

use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{Comment, CommentAuthor, CommentError, Comments, MAX_COMMENT_AUTHORS, MAX_COMMENTS, Passthrough};

use super::CodecError;
use super::decode::{Dec, Ours, limit};
use super::encode::Encoder;
use super::ext::Keyed;
use super::kept::Rest;
use super::vocab::{at, el};
use crate::ns::Scope;

impl Encoder<'_> {
    /// The `comment-list` element: the setting, the raw attributes, then each author and each comment in the order the model holds them.
    pub(super) fn comment_list(&mut self, c: &Comments, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let mut e = self.elem(el::COMMENT_LIST);
        if let Some(show) = c.show_tags {
            e.attrs.push((at::SHOW_TAGS.to_owned(), if show { "1" } else { "0" }.to_owned()));
        }
        self.put_opt(&mut e, at::PART, c.part.as_ref())?;
        self.raw_attrs(&mut e, &c.raw)?;
        for a in c.authors() {
            let a = self.comment_author(a, keyed)?;
            self.add(&mut e, a)?;
        }
        for m in c.iter() {
            let m = self.comment_entry(m, keyed)?;
            self.add(&mut e, m)?;
        }
        Ok(e)
    }

    fn comment_author(&mut self, a: &CommentAuthor, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let CommentAuthor { id, name, initials, raw } = a;
        let mut e = self.elem(el::COMMENT_AUTHOR);
        e.attrs.push((at::ID.to_owned(), id.to_string()));
        self.put_req(&mut e, at::NAME, name)?;
        self.put_req(&mut e, at::INITIALS, initials)?;
        self.raw_attrs(&mut e, raw)?;
        self.take_keyed(&mut e, keyed, "comment-author", &id.to_string())?;
        Ok(e)
    }

    fn comment_entry(&mut self, m: &Comment, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let Comment { id, parent, page, shape, author, text, date, edit_date, done, raw } = m;
        let mut e = self.elem(el::COMMENT_ENTRY);
        e.attrs.push((at::ID.to_owned(), id.to_string()));
        Self::put_opt_u32(&mut e, at::PARENT, *parent);
        Self::put_u32(&mut e, at::PAGE_ID, *page, 0);
        Self::put_opt_u32(&mut e, at::SHAPE_ID, *shape);
        Self::put_u32(&mut e, at::AUTHOR, *author, 0);
        self.put_req(&mut e, at::TEXT, text)?;
        self.put_req(&mut e, at::DATE, date)?;
        self.put_opt(&mut e, at::EDIT_DATE, edit_date.as_ref())?;
        Self::put_flag(&mut e, at::DONE, *done);
        self.raw_attrs(&mut e, raw)?;
        self.take_keyed(&mut e, keyed, "comment-entry", &id.to_string())?;
        Ok(e)
    }
}

/// What a comment list's refusal says: the cap that was passed, or the id used twice.
fn refusal(dec: &Dec<'_>, e: &CommentError) -> CodecError {
    match e {
        CommentError::TooMany => limit("comments", MAX_COMMENTS),
        CommentError::TooManyAuthors => limit("comment authors", MAX_COMMENT_AUTHORS),
        CommentError::TextTooLong => limit("bytes of text in a comment", diacraft_doc::MAX_COMMENT_TEXT_BYTES),
        CommentError::TotalTextTooLong => limit("bytes of comment text", diacraft_doc::MAX_COMMENTS_TEXT_BYTES),
        CommentError::DuplicateId(_) => dec.bad("two comments with one number"),
        CommentError::DuplicateAuthorId(_) => dec.bad("two comment authors with one number"),
        _ => dec.bad("a list of comments that is not valid"),
    }
}

impl Dec<'_> {
    /// The `comment-list` element. Each author and comment is counted against its cap before it is made.
    pub(super) fn comment_list(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Comments, CodecError> {
        let (mut show_tags, mut part) = (None, None);
        self.fields(e, sc, |_, n, v| {
            if n == at::PART {
                part = Some(Arc::<str>::from(&*v));
                return Ok(true);
            }
            if n == at::SHOW_TAGS {
                // 1 or 0 (`true` and `false` are read); any other text is no setting.
                show_tags = match &*v {
                    "1" | "true" => Some(true),
                    "0" | "false" => Some(false),
                    _ => None,
                };
                return Ok(true);
            }
            Ok(false)
        })?;
        let attrs = self.native_attrs(sc)?;
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        let (mut authors, mut comments): (Vec<CommentAuthor>, Vec<Comment>) = (Vec::new(), Vec::new());
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            match local {
                el::COMMENT_AUTHOR => {
                    if authors.len() >= MAX_COMMENT_AUTHORS {
                        return Err(limit("comment authors", MAX_COMMENT_AUTHORS));
                    }
                    authors.push(d.comment_author(child, csc)?);
                }
                el::COMMENT_ENTRY => {
                    if comments.len() >= MAX_COMMENTS {
                        return Err(limit("comments", MAX_COMMENTS));
                    }
                    comments.push(d.comment_entry(child, csc)?);
                }
                el::ATTR => return d.rest_child(&mut rest, child, csc),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.finish_record(attrs, nodes, "comment-list", "");
        let mut list = Comments::from_parts(show_tags, rest.raw, authors, comments).map_err(|why| refusal(self, &why))?;
        list.part = part;
        Ok(list)
    }

    fn comment_author(&mut self, e: &El, sc: &Scope<Ours>) -> Result<CommentAuthor, CodecError> {
        let (mut id, mut name, mut initials) = (None, String::new(), String::new());
        self.fields(e, sc, |d, n, v| {
            match n {
                at::ID => id = Some(d.u32(&v)?),
                at::NAME => name = v.into_owned(),
                at::INITIALS => initials = v.into_owned(),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let Some(id) = id else { return Err(self.bad("a comment author with no number")) };
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(
            e,
            sc,
            Some(&mut nodes),
            |d, child, local, csc| if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) },
        )?;
        self.finish_record(attrs, nodes, "comment-author", &id.to_string());
        Ok(CommentAuthor { id, name: Arc::from(name.as_str()), initials: Arc::from(initials.as_str()), raw: rest.raw })
    }

    fn comment_entry(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Comment, CodecError> {
        let (mut id, mut parent, mut page, mut shape, mut author) = (None, None, 0_u32, None, 0_u32);
        let (mut text, mut date, mut edit_date, mut done) = (String::new(), String::new(), None, false);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::ID => id = Some(d.u32(&v)?),
                at::PARENT => parent = Some(d.u32(&v)?),
                at::PAGE_ID => page = d.u32(&v)?,
                at::SHAPE_ID => shape = Some(d.u32(&v)?),
                at::AUTHOR => author = d.u32(&v)?,
                at::TEXT => text = v.into_owned(),
                at::DATE => date = v.into_owned(),
                at::EDIT_DATE => edit_date = Some(v.into_owned()),
                at::DONE => done = d.flag(&v)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let Some(id) = id else { return Err(self.bad("a comment with no number")) };
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(
            e,
            sc,
            Some(&mut nodes),
            |d, child, local, csc| if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) },
        )?;
        self.finish_record(attrs, nodes, "comment-entry", &id.to_string());
        Ok(Comment {
            id,
            parent,
            page,
            shape,
            author,
            text: Arc::from(text.as_str()),
            date: Arc::from(date.as_str()),
            edit_date: edit_date.map(|d| Arc::from(d.as_str())),
            done,
            raw: rest.raw,
        })
    }
}
