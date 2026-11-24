//! The comments of a document: ids, replies, the caps, and what a removal takes with it.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{
    Comment, CommentAuthor, CommentError, Comments, Document, MAX_COMMENT_AUTHORS, MAX_COMMENT_TEXT_BYTES, MAX_COMMENTS, MAX_COMMENTS_TEXT_BYTES,
    NewComment, RawAttrs,
};

fn author(id: u32, name: &str) -> CommentAuthor {
    CommentAuthor { id, name: Arc::from(name), initials: Arc::from("X"), raw: RawAttrs::new() }
}

fn comment(id: u32, page: u32, shape: Option<u32>, parent: Option<u32>, text: &str) -> Comment {
    Comment {
        id,
        parent,
        page,
        shape,
        author: 1,
        text: Arc::from(text),
        date: Arc::from("2031-03-07T20:05:09Z"),
        edit_date: None,
        done: false,
        raw: RawAttrs::new(),
    }
}

fn new(page: u32, shape: Option<u32>, parent: Option<u32>, text: &str) -> NewComment {
    NewComment { parent, page, shape, author: 1, text: Arc::from(text), date: Arc::from("2031-03-07T20:05:09Z") }
}

fn with_author() -> Comments {
    Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], Vec::new()).unwrap()
}

#[test]
fn a_new_document_has_no_comments() {
    let doc = Document::new_blank();
    assert!(doc.comments.is_empty());
    assert_eq!(doc.comments.len(), 0);
    assert!(doc.comments.get(1).is_none());
}

#[test]
fn comments_get_the_next_id_and_are_found_by_it() {
    let mut c = with_author();
    assert_eq!(c.add(new(1, None, None, "first")).unwrap(), 1);
    assert_eq!(c.add(new(1, Some(5), None, "second")).unwrap(), 2);
    assert_eq!(c.len(), 2);
    assert_eq!(&*c.get(2).unwrap().text, "second");
    assert_eq!(c.get(2).unwrap().shape, Some(5));
    assert!(!c.is_empty());
}

#[test]
fn a_comment_needs_a_known_author_and_some_text() {
    let mut c = with_author();
    let mut n = new(1, None, None, "text");
    n.author = 9;
    assert_eq!(c.add(n), Err(CommentError::NoSuchAuthor(9)));
    assert_eq!(c.add(new(1, None, None, "  \n ")), Err(CommentError::EmptyText));
    assert_eq!(c.len(), 0);
}

#[test]
fn text_with_a_control_character_is_refused_and_a_line_break_is_not() {
    let mut c = with_author();
    assert_eq!(c.add(new(1, None, None, "a\u{0}b")), Err(CommentError::ControlCharacter));
    assert_eq!(c.add(new(1, None, None, "a\u{1b}[0m")), Err(CommentError::ControlCharacter));
    assert!(c.add(new(1, None, None, "line one\nline two\tindented")).is_ok());
}

#[test]
fn one_comment_is_at_most_the_per_comment_cap_in_bytes() {
    let mut c = with_author();
    let longest = "x".repeat(MAX_COMMENT_TEXT_BYTES);
    assert!(c.add(new(1, None, None, &longest)).is_ok());
    let too_long = "x".repeat(MAX_COMMENT_TEXT_BYTES + 1);
    assert_eq!(c.add(new(1, None, None, &too_long)), Err(CommentError::TextTooLong));
    assert_eq!(c.len(), 1);
}

#[test]
fn the_text_of_all_comments_is_capped_together() {
    let mut c = with_author();
    let big = "y".repeat(MAX_COMMENT_TEXT_BYTES);
    let fits = MAX_COMMENTS_TEXT_BYTES / MAX_COMMENT_TEXT_BYTES;
    for _ in 0..fits {
        c.add(new(1, None, None, &big)).unwrap();
    }
    assert_eq!(c.add(new(1, None, None, "one more byte or so")), Err(CommentError::TotalTextTooLong));
    // Taking one away makes room again.
    assert!(c.remove(1).unwrap() >= 1);
    assert!(c.add(new(1, None, None, "room")).is_ok());
}

#[test]
fn at_most_max_comments() {
    let items: Vec<Comment> = (1..=MAX_COMMENTS as u32).map(|id| comment(id, 1, None, None, "c")).collect();
    let mut c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], items).unwrap();
    assert_eq!(c.len(), MAX_COMMENTS);
    assert_eq!(c.add(new(1, None, None, "one too many")), Err(CommentError::TooMany));
    let too_many: Vec<Comment> = (1..=MAX_COMMENTS as u32 + 1).map(|id| comment(id, 1, None, None, "c")).collect();
    assert_eq!(Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], too_many).err(), Some(CommentError::TooMany));
}

#[test]
fn at_most_max_authors() {
    let authors: Vec<CommentAuthor> = (1..=MAX_COMMENT_AUTHORS as u32).map(|id| author(id, &format!("a{id}"))).collect();
    let mut c = Comments::from_parts(None, RawAttrs::new(), authors, Vec::new()).unwrap();
    assert_eq!(c.author_for("somebody new", "SN"), Err(CommentError::TooManyAuthors));
    assert_eq!(c.author_for("a7", "X"), Ok(7), "an author that is there is found, not added");
    let too_many: Vec<CommentAuthor> = (1..=MAX_COMMENT_AUTHORS as u32 + 1).map(|id| author(id, "a")).collect();
    assert_eq!(Comments::from_parts(None, RawAttrs::new(), too_many, Vec::new()).err(), Some(CommentError::TooManyAuthors));
}

#[test]
fn an_author_is_found_by_name_and_initials_and_added_once() {
    let mut c = Comments::new();
    let a = c.author_for("Ada Lovelace", "AL").unwrap();
    let b = c.author_for("Ada Lovelace", "AL").unwrap();
    let other = c.author_for("Ada Lovelace", "ADA").unwrap();
    assert_eq!(a, b);
    assert_ne!(a, other);
    assert_eq!(c.authors().count(), 2);
    assert_eq!(&*c.author(a).unwrap().name, "Ada Lovelace");
}

#[test]
fn from_parts_refuses_a_repeated_id() {
    let twice = vec![comment(3, 1, None, None, "a"), comment(3, 1, None, None, "b")];
    assert_eq!(Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], twice).err(), Some(CommentError::DuplicateId(3)));
    let authors = vec![author(1, "Ada"), author(1, "Bob")];
    assert_eq!(Comments::from_parts(None, RawAttrs::new(), authors, Vec::new()).err(), Some(CommentError::DuplicateAuthorId(1)));
}

#[test]
fn a_reply_is_a_comment_of_its_parents_anchor_and_a_reply_to_a_reply_joins_the_thread() {
    let mut c = with_author();
    let root = c.add(new(2, Some(7), None, "root")).unwrap();
    let reply = c.add(new(9, None, Some(root), "reply")).unwrap();
    let r = c.get(reply).unwrap();
    assert_eq!((r.parent, r.page, r.shape), (Some(root), 2, Some(7)), "a reply sits where its comment sits");
    let again = c.add(new(0, None, Some(reply), "reply to the reply")).unwrap();
    assert_eq!(c.get(again).unwrap().parent, Some(root), "the thread is one level deep");
    assert_eq!(c.add(new(2, None, Some(99), "orphan")), Err(CommentError::NoSuchComment(99)));
}

#[test]
fn removing_a_comment_takes_its_replies_with_it() {
    let mut c = with_author();
    let a = c.add(new(1, None, None, "a")).unwrap();
    let b = c.add(new(1, None, None, "b")).unwrap();
    let r1 = c.add(new(1, None, Some(a), "r1")).unwrap();
    let r2 = c.add(new(1, None, Some(a), "r2")).unwrap();
    assert_eq!(c.remove(a).unwrap(), 3);
    assert!(c.get(a).is_none() && c.get(r1).is_none() && c.get(r2).is_none());
    assert!(c.get(b).is_some());
    assert_eq!(c.remove(a), Err(CommentError::NoSuchComment(a)));
    // A reply alone goes alone.
    let r3 = c.add(new(1, None, Some(b), "r3")).unwrap();
    assert_eq!(c.remove(r3).unwrap(), 1);
    assert!(c.get(b).is_some());
}

#[test]
fn editing_changes_the_text_and_the_edit_date_and_nothing_else() {
    let mut c = with_author();
    let id = c.add(new(1, Some(3), None, "before")).unwrap();
    c.edit(id, "after", Arc::from("2031-03-08T00:00:00Z")).unwrap();
    let got = c.get(id).unwrap();
    assert_eq!(&*got.text, "after");
    assert_eq!(got.edit_date.as_deref(), Some("2031-03-08T00:00:00Z"));
    assert_eq!((got.page, got.shape, got.author, &*got.date), (1, Some(3), 1, "2031-03-07T20:05:09Z"));
    assert_eq!(c.edit(id, "", Arc::from("x")), Err(CommentError::EmptyText));
    assert_eq!(c.edit(77, "x", Arc::from("x")), Err(CommentError::NoSuchComment(77)));
    // The total counts the new text and not the old.
    let big = "z".repeat(MAX_COMMENT_TEXT_BYTES + 1);
    assert_eq!(c.edit(id, &big, Arc::from("x")), Err(CommentError::TextTooLong));
}

#[test]
fn done_is_set_and_cleared() {
    let mut c = with_author();
    let id = c.add(new(1, None, None, "t")).unwrap();
    c.set_done(id, true, Arc::from("2031-03-08T00:00:00Z")).unwrap();
    assert!(c.get(id).unwrap().done);
    c.set_done(id, false, Arc::from("2031-03-09T00:00:00Z")).unwrap();
    assert!(!c.get(id).unwrap().done);
}

#[test]
fn a_page_taken_away_takes_its_comments() {
    let mut c = with_author();
    c.add(new(1, None, None, "on 1")).unwrap();
    let on2 = c.add(new(2, Some(4), None, "on 2")).unwrap();
    c.add(new(2, None, Some(on2), "reply on 2")).unwrap();
    assert_eq!(c.remove_page(2), 2);
    assert_eq!(c.len(), 1);
    assert_eq!(c.remove_page(2), 0);
}

#[test]
fn shapes_taken_away_take_their_comments_and_the_replies() {
    let mut c = with_author();
    let on5 = c.add(new(1, Some(5), None, "on 5")).unwrap();
    c.add(new(1, Some(6), None, "on 6")).unwrap();
    c.add(new(1, None, None, "on the page")).unwrap();
    c.add(new(2, Some(5), None, "same id, other page")).unwrap();
    c.add(new(1, None, Some(on5), "reply to the one on 5")).unwrap();
    let gone: BTreeSet<u32> = [5].into_iter().collect();
    assert_eq!(c.remove_shapes(1, &gone), 2);
    assert_eq!(c.len(), 3);
    assert_eq!(c.remove_shapes(1, &BTreeSet::new()), 0);
}

#[test]
fn from_parts_straightens_threads_that_are_not_one_level_deep() {
    let items = vec![
        comment(1, 1, None, None, "root"),
        comment(2, 1, None, Some(1), "reply"),
        comment(3, 1, None, Some(2), "reply to a reply"),
        comment(4, 1, None, Some(4), "its own parent"),
        comment(5, 1, None, Some(6), "a cycle"),
        comment(6, 1, None, Some(5), "a cycle"),
        comment(7, 1, None, Some(99), "a parent that is not there"),
    ];
    let c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], items).unwrap();
    assert_eq!(c.get(2).unwrap().parent, Some(1));
    assert_eq!(c.get(3).unwrap().parent, Some(1));
    assert_eq!(c.get(4).unwrap().parent, None);
    for id in [5, 6] {
        let parent = c.get(id).unwrap().parent;
        assert!(parent.is_none_or(|p| c.get(p).is_some_and(|q| q.parent.is_none())), "no cycle is left: comment {id} has {parent:?}");
    }
    assert_eq!(c.get(7).unwrap().parent, None);
}

#[test]
fn a_chain_of_replies_a_hostile_file_holds_is_read_in_one_pass_and_made_flat() {
    // 20,000 comments, each the reply of the one before it.
    let n = 20_000_u32;
    let items: Vec<Comment> = (1..=n).map(|id| comment(id, 1, None, id.checked_sub(1).filter(|p| *p > 0), "c")).collect();
    let c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], items).unwrap();
    assert_eq!(c.len(), n as usize);
    for item in c.iter() {
        assert!(item.parent.is_none_or(|p| c.get(p).is_some_and(|q| q.parent.is_none())));
    }
}

#[test]
fn an_edit_shares_what_it_did_not_touch() {
    let items: Vec<Comment> = (1..=1_000_u32).map(|id| comment(id, 1, None, None, "c")).collect();
    let base = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], items).unwrap();
    let mut next = base.clone();
    next.edit(1_000, "changed", Arc::from("2031-03-08T00:00:00Z")).unwrap();
    assert_eq!(&*base.get(1_000).unwrap().text, "c", "the older copy is unchanged");
    assert!(next.shared_chunks_with(&base) >= 14, "only the chunk of the edited comment was copied");
}

#[test]
fn show_tags_is_part_of_the_value() {
    let mut a = Comments::new();
    let b = a.clone();
    assert_eq!(a, b);
    a.show_tags = Some(false);
    assert_ne!(a, b);
}
