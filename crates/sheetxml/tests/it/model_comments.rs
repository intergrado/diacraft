//! The comments of a document in the `.diacraft` codec: the `comment-list` element, its round trip, what a newer build wrote on a comment, and the refusals.

use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, Node};
use diacraft_doc::{Comment, CommentAuthor, Comments, Document, MAX_COMMENTS, RawAttrs};
use diacraft_sheetxml::model::{Budget, MemBlobs, decode, encode};

use super::model_util::{pack, round_trip};

fn author(id: u32, name: &str) -> CommentAuthor {
    CommentAuthor { id, name: Arc::from(name), initials: Arc::from("X"), raw: RawAttrs::from_pairs([("SMTP", "a@example.org")]) }
}

fn comment(id: u32, parent: Option<u32>, text: &str) -> Comment {
    Comment {
        id,
        parent,
        page: 1,
        shape: Some(7),
        author: 1,
        text: Arc::from(text),
        date: Arc::from("2031-03-07T20:05:09Z"),
        edit_date: Some(Arc::from("2031-03-08T00:00:00Z")),
        done: id.is_multiple_of(2),
        raw: RawAttrs::from_pairs([("AutoCommentType", "2")]),
    }
}

fn document_with(comments: Comments) -> Document {
    let mut d = Document::empty();
    d.comments = comments;
    d
}

#[test]
fn a_document_with_no_comments_writes_no_comment_list() {
    let packed = pack(&Document::new_blank());
    assert!(!String::from_utf8(packed.document).unwrap().contains("comment-list"));
}

#[test]
fn comments_authors_replies_and_the_setting_come_back_exactly() {
    let long = "a long text, line\nbreaks, \"quotes\" & <angles>, tabs\there, and unicode: \u{1F600} \u{00E9}".repeat(40);
    let items = vec![comment(1, None, "first"), comment(2, Some(1), "a reply"), comment(3, None, &long), comment(4, None, "no shape")];
    let c = Comments::from_parts(Some(false), RawAttrs::from_pairs([("Future", "x")]), vec![author(1, "Ada"), author(2, "Bob")], items).unwrap();
    let d = document_with(c);
    let back = round_trip(&d);
    assert!(back == d);
    assert_eq!(back.comments.get(2).unwrap().parent, Some(1));
    assert_eq!(&*back.comments.get(3).unwrap().text, long.as_str());
    assert_eq!(back.comments.show_tags, Some(false));
    assert_eq!(back.comments.author(1).unwrap().raw.get("SMTP"), Some("a@example.org"));
    // A second pass writes the same bytes.
    assert_eq!(pack(&back).document, pack(&d).document);
}

#[test]
fn the_text_of_a_comment_with_a_character_xml_forbids_is_kept() {
    // A VSDX file cannot hold it (XML 1.0), but a model may (a control character the commands refuse can still be in a file's text): it travels as base64.
    let c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], vec![comment(1, None, "a\u{1}b")]).unwrap();
    let d = document_with(c);
    assert!(round_trip(&d) == d);
}

fn decode_text(document: &El) -> Result<Document, diacraft_sheetxml::model::CodecError> {
    decode(document, &[], &[], &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new())
}

fn encoded_document(d: &Document) -> El {
    let mut blobs = MemBlobs::new();
    encode(d, &mut blobs).unwrap().document
}

fn list_mut(root: &mut El) -> &mut El {
    root.children
        .iter_mut()
        .find_map(|n| match n {
            Node::El(e) if e.local() == "comment-list" => Some(e),
            _ => None,
        })
        .expect("the comment list")
}

#[test]
fn what_a_newer_build_wrote_on_a_comment_comes_back_on_it() {
    let c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], vec![comment(1, None, "t")]).unwrap();
    let mut root = encoded_document(&document_with(c));
    let entry = list_mut(&mut root)
        .children
        .iter_mut()
        .find_map(|n| match n {
            Node::El(e) if e.local() == "comment-entry" => Some(e),
            _ => None,
        })
        .unwrap();
    entry.attrs.push(("later".to_owned(), "1".to_owned()));
    let doc = decode_text(&root).unwrap();
    assert_eq!(doc.comments.len(), 1, "the comment is read");
    let kept = doc.ext.get().map_or(0, |d| d.native.raw.len() + d.native.kept.len());
    assert!(kept >= 1, "what is unknown is kept in the document's native slot");
}

#[test]
fn a_list_with_a_repeated_id_or_too_many_comments_is_refused_in_words() {
    let c = Comments::from_parts(None, RawAttrs::new(), vec![author(1, "Ada")], vec![comment(1, None, "a"), comment(2, None, "b")]).unwrap();
    let mut root = encoded_document(&document_with(c));
    // The second comment takes the first's number.
    let mut seen = false;
    for node in &mut list_mut(&mut root).children {
        if let Node::El(e) = node
            && e.local() == "comment-entry"
        {
            if seen {
                for (k, v) in &mut e.attrs {
                    if k == "id" {
                        *v = "1".to_owned();
                    }
                }
            }
            seen = true;
        }
    }
    let error = decode_text(&root).unwrap_err().to_string();
    assert!(error.contains("one number"), "{error}");

    let mut root = encoded_document(&document_with(Comments::new()));
    let entries: Vec<Node> = (0..=MAX_COMMENTS)
        .map(|i| {
            Node::El(El { name: "diacraft:comment-entry".to_owned(), attrs: vec![("id".to_owned(), (i + 1).to_string())], children: Vec::new() })
        })
        .collect();
    root.children.push(Node::El(El { name: "diacraft:comment-list".to_owned(), attrs: Vec::new(), children: entries }));
    let error = decode_text(&root).unwrap_err().to_string();
    assert!(error.contains("comments"), "{error}");
}

#[test]
fn a_comment_with_no_number_is_damaged() {
    let mut root = encoded_document(&document_with(Comments::new()));
    root.children.push(Node::El(El {
        name: "diacraft:comment-list".to_owned(),
        attrs: Vec::new(),
        children: vec![Node::El(El { name: "diacraft:comment-entry".to_owned(), attrs: Vec::new(), children: Vec::new() })],
    }));
    let error = decode_text(&root).unwrap_err().to_string();
    assert!(error.contains("no number"), "{error}");
}
