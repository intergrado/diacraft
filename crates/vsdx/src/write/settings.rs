//! The Snap & Glue words of `DocumentSettings` in the document part.
//!
//! A save patches the file's own `DocumentSettings` element: a word that differs is given the new text where it sits, a word the element lacks
//! is added where files put it (`GlueSettings`, `SnapSettings`, `SnapExtensions`, `SnapAngles`, `DynamicGridEnabled`, in that order), a word the
//! model dropped is taken out, and nothing else of the element or the part changes (its attributes, a child this crate does not know, a blank
//! between two children, a word that is the same). A file with no `DocumentSettings` is given one, first in the document, as the files hold it
//! (every sample has it first). A new drawing's skeleton writes the same words ([`words_el`]).
//!
//! **A save that changes only words changes only the bytes of those words**: when the document part's tree is otherwise
//! as it was, the text of each word that changed is replaced in the file's own bytes ([`patch_bytes`]) and every other byte of the part (the spelling of an
//! empty element, the quotes, the line ends, the white space between elements) is the file's. That is tried only for a word that is there, whose element has
//! no attribute and holds plain text, and its result is read back and **must be the tree the words were patched into**; if anything differs, the tree is
//! written as before (a word that is added, taken out or written with a reference).

use diacraft_doc::SnapWords;
use diacraft_doc::{is_other_namespace, namespace_attribute};
use diacraft_opc::xml::{self, El, Node, SpanKind, XmlLimits};

use super::build::element;
use super::{R, WriteError};
use crate::ns::Scope;

/// The element that holds the words.
const SETTINGS: &str = "DocumentSettings";

/// The words in the order files write them, with `SnapAngles` (which no command changes) in its place, so that an added word lands in the right
/// slot among the children.
const ORDER: [&str; 5] = ["GlueSettings", "SnapSettings", "SnapExtensions", "SnapAngles", "DynamicGridEnabled"];

/// A word's element name and the text the model gives it, for the four words the model holds.
fn named(words: &SnapWords) -> [(&'static str, Option<&str>); 4] {
    [
        ("GlueSettings", words.glue.as_deref()),
        ("SnapSettings", words.snap.as_deref()),
        ("SnapExtensions", words.extensions.as_deref()),
        ("DynamicGridEnabled", words.dynamic_grid.as_deref()),
    ]
}

/// The text of an element: its text nodes together.
fn text_of(el: &El) -> String {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::Text(t) | Node::CData(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// The children of a new drawing's `DocumentSettings` for these words: the elements, in the order files write them, for the words the model has, with the
/// empty `SnapAngles` between `SnapExtensions` and `DynamicGridEnabled` that every sample holds. None for a model with no word.
pub(super) fn words_el(words: &SnapWords) -> Vec<Node> {
    let given = named(words);
    if given.iter().all(|(_, text)| text.is_none()) {
        return Vec::new();
    }
    ORDER
        .iter()
        .filter_map(|name| {
            if *name == "SnapAngles" {
                return Some(Node::El(element("SnapAngles", Vec::new(), Vec::new())));
            }
            let text = given.iter().find(|(n, _)| n == name).and_then(|(_, text)| *text)?;
            Some(Node::El(element(name, Vec::new(), vec![Node::Text(text.to_owned())])))
        })
        .collect()
}

/// Whether `e` is the element the **model** means by `local`: that local name, and not a namespace of another application declared on the element itself
/// (`diacraft_doc::is_other_namespace`, the one rule).
fn model_means(e: &El, local: &str) -> bool {
    e.is(local) && !e.attr(&namespace_attribute(&e.name)).is_some_and(is_other_namespace)
}

/// The position of the element called `local` among `children`, if the model and the namespace-aware writer pick the same one (or both pick none):
/// `Err(())` when they pick different elements, and the words are then not written.
fn agreed(children: &[Node], sc: &Scope, local: &str) -> Result<Option<usize>, ()> {
    let model = children.iter().position(|n| matches!(n, Node::El(e) if model_means(e, local)));
    let writer = children.iter().position(|n| matches!(n, Node::El(e) if sc.enter(e).is(e, local)));
    if model == writer { Ok(writer) } else { Err(()) }
}

/// What [`patch_words`] did.
pub(super) enum Patched {
    /// The `DocumentSettings` says the words.
    Done,
    /// Nothing was written: the model and the writer would not have meant the same element by a word, and the text says why, for a warning.
    Left(&'static str),
}

/// The warning for words left alone.
const LEFT: &str = "DiaCraft's Snap and Glue options were not saved: the file's DocumentSettings holds an element named like one of them in a namespace this build does not read as the file's own, and the model and the writer would not agree on which element is the word, so the words were left as the file has them";

/// Where a child called `name` goes in `children`: after the last child that files write before it, else before the first they write after it, else
/// at the end.
fn place_for(children: &[Node], sc: &Scope, name: &str) -> usize {
    let rank = |n: &str| ORDER.iter().position(|o| *o == n);
    let Some(mine) = rank(name) else { return children.len() };
    let known = |c: &Node| match c {
        Node::El(el) => ORDER.iter().position(|o| sc.enter(el).is(el, o)),
        _ => None,
    };
    if let Some(at) = children.iter().rposition(|c| known(c).is_some_and(|r| r < mine)) {
        return at + 1;
    }
    children.iter().position(|c| known(c).is_some_and(|r| r > mine)).unwrap_or(children.len())
}

/// Brings the `DocumentSettings` of the document part `root` to say `words`. `prefix` is the document element's own, so that an element made here
/// is written as the file writes its names.
pub(super) fn patch_words(root: &mut El, sc: &Scope, prefix: &str, words: &SnapWords) -> R<Patched> {
    // The element the model reads and the element the writer would write must be the same one, for the settings and for each word.
    let Ok(at) = agreed(&root.children, sc, SETTINGS) else { return Ok(Patched::Left(LEFT)) };
    let at = match at {
        Some(at) => at,
        None => {
            let mut made = element(SETTINGS, Vec::new(), words_el(words));
            if made.children.is_empty() {
                return Ok(Patched::Done);
            }
            super::build::with_prefix(&mut made, prefix);
            root.children.insert(0, Node::El(made));
            return Ok(Patched::Done);
        }
    };
    let Some(Node::El(settings)) = root.children.get_mut(at) else {
        return Err(WriteError::Mismatch("the document's settings went away"));
    };
    let inner = sc.enter(settings);
    if named(words).iter().any(|(name, _)| agreed(&settings.children, &inner, name).is_err()) {
        return Ok(Patched::Left(LEFT));
    }
    for (name, wanted) in named(words) {
        let child = settings.children.iter().position(|n| matches!(n, Node::El(e) if inner.enter(e).is(e, name)));
        match (child, wanted) {
            (Some(at), Some(text)) => {
                if let Some(Node::El(el)) = settings.children.get_mut(at)
                    && text_of(el).trim() != text
                {
                    // The blanks round the old text stay, as they do when the bytes are patched in place.
                    let old = text_of(el);
                    let blank = old.trim().is_empty();
                    let lead = if blank { "" } else { &old[..old.len() - old.trim_start().len()] };
                    let trail = if blank { "" } else { &old[old.trim_end().len()..] };
                    el.children = vec![Node::Text(format!("{lead}{text}{trail}"))];
                }
            }
            (Some(at), None) => {
                settings.children.remove(at);
            }
            (None, Some(text)) => {
                let mut made = element(name, Vec::new(), vec![Node::Text(text.to_owned())]);
                super::build::with_prefix(&mut made, prefix);
                let at = place_for(&settings.children, &inner, name);
                settings.children.insert(at, Node::El(made));
            }
            (None, None) => {}
        }
    }
    Ok(Patched::Done)
}

/// What the document part's tree held of `DocumentSettings` before the words were patched: the kinds of the root's children, where the element is among them,
/// and the element itself, so that its bytes can be found ([`patch_bytes`]).
pub(super) struct Before {
    kinds: Vec<SpanKind>,
    at: usize,
    settings: El,
}

fn span_kind(node: &Node) -> SpanKind {
    match node {
        Node::El(_) => SpanKind::Element,
        Node::Text(_) => SpanKind::Text,
        Node::CData(_) => SpanKind::CData,
        Node::Comment(_) => SpanKind::Comment,
        Node::Pi { .. } => SpanKind::Pi,
    }
}

/// The tree's `DocumentSettings` as it is now, if it has one.
pub(super) fn snapshot(root: &El, sc: &Scope) -> Option<Before> {
    let at = root.children.iter().position(|n| matches!(n, Node::El(e) if sc.enter(e).is(e, SETTINGS)))?;
    let Some(Node::El(settings)) = root.children.get(at) else { return None };
    Some(Before { kinds: root.children.iter().map(span_kind).collect(), at, settings: settings.clone() })
}

/// The part's own `bytes` with the text of each word that differs from `words` replaced in place, and nothing else changed; `None` when that cannot be done
/// (see the module documentation) or does not give the tree `after`, the root the words were patched into, and the tree is to be written instead.
pub(super) fn patch_bytes(bytes: &[u8], before: &Before, sc: &Scope, words: &SnapWords, after: &El) -> Option<Vec<u8>> {
    let root_spans = xml::child_spans(bytes, before.kinds.len())?;
    if root_spans.children.len() != before.kinds.len() || !root_spans.children.iter().zip(&before.kinds).all(|((kind, _, _), seen)| kind == seen) {
        return None;
    }
    let &(SpanKind::Element, a, b) = root_spans.children.get(before.at)? else { return None };
    let slice = bytes.get(a..b)?;
    let spans = xml::child_spans(slice, before.settings.children.len())?;
    if spans.children.len() != before.settings.children.len()
        || !spans.children.iter().zip(&before.settings.children).all(|((kind, _, _), node)| *kind == span_kind(node))
    {
        return None;
    }
    let inner = sc.enter(&before.settings);
    let mut edits: Vec<(usize, usize, &str)> = Vec::new();
    for (name, wanted) in named(words) {
        let at = before.settings.children.iter().position(|n| matches!(n, Node::El(e) if inner.enter(e).is(e, name)));
        let (at, text) = match (at, wanted) {
            (None, None) => continue,
            // A word added or taken out is the tree's.
            (None, Some(_)) | (Some(_), None) => return None,
            (Some(at), Some(text)) => (at, text),
        };
        let Some(Node::El(el)) = before.settings.children.get(at) else { return None };
        if text_of(el).trim() == text {
            continue;
        }
        if !el.attrs.is_empty() || !matches!(el.children.as_slice(), [Node::Text(_)]) || !text.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return None;
        }
        let &(SpanKind::Element, c, d) = spans.children.get(at)? else { return None };
        let element = slice.get(c..d)?;
        let open_end = element.iter().position(|c| *c == b'>')?;
        let close_start = element.windows(2).rposition(|w| w == b"</")?;
        if close_start <= open_end || element.get(open_end.checked_sub(1)?) == Some(&b'/') {
            return None;
        }
        let content = element.get(open_end + 1..close_start)?;
        if content.iter().any(|c| *c == b'&' || *c == b'<') || content.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let lead = content.iter().take_while(|c| c.is_ascii_whitespace()).count();
        let trail = content.iter().rev().take_while(|c| c.is_ascii_whitespace()).count();
        edits.push((a + c + open_end + 1 + lead, a + c + close_start - trail, text));
    }
    if edits.is_empty() {
        return None;
    }
    // From the last to the first, so that the offsets of the others hold.
    edits.sort_by_key(|x| std::cmp::Reverse(x.0));
    let mut out = bytes.to_vec();
    for (start, end, text) in edits {
        out.splice(start..end, text.bytes());
    }
    // The check: what was written reads as the tree the words were patched into.
    let reread = xml::parse_with(&out, &XmlLimits::default()).ok()?;
    (reread == *after).then_some(out)
}
