//! `[Content_Types].xml` as the file wrote it, edited at the rows an edit changes and nowhere else.
//!
//! **Kept as it was, as far as it can be.** R4 puts the stream among the regenerated parts and R5 asks that every row be carried;
//! says a writer rewrites it from its own model "so they stay consistent". What that asks for is a stream that gives every part a type and loses no
//! row, and a stream whose rows are the file's is that as it stands, so no rule asks for a layout of the writer's own. A stream read from a file is
//! kept as its children (the rows, the white space between them, comments), each with the bytes it had, and written back:
//!
//! * **if the rows are still the file's** (the `Default` and `Override` rows with the attributes they carry, in order), **as the file's bytes**
//!   ([`TypesTree::bytes`]);
//! * **if a row was added, changed or taken out, as the file's bytes with the rows that changed written anew**: every other row, the white space,
//!   the comments, the declaration, the root's tag, the quotes, the CR LF line ends and the spelling of everything are the file's own bytes. A new row
//!   goes after the last row, set apart from it as that row is from the one before it (a short gap is repeated; a long one is cut to a line end, or
//!   to nothing, so that a stream with a megabyte of spaces before its last row does not grow by a megabyte for each row added), in the quotes the
//!   last row has, and in the root's prefix if it has one. A row taken out takes the white space before it. A row whose type is changed is written
//!   anew in the same way, with its attributes in order. A new `Default` goes after the last `Default`.
//! * **where the children cannot be found in the file's bytes** (a stream in UTF-16, a root written empty) **the tree is written**: the declaration
//!   and what follows the root are the file's, and so are the root's attributes, the order of attributes, the white space and the comments; the quotes
//!   are double, an empty element is `<a/>`, a line end is a line feed and references are spelled as the writer spells them (
//!   a writer need not match them). The scan and the tree are checked to agree on the children, and the tree is written if they do not; they agree on
//!   every part that parses (the `opc` crate's tests of the scan hold it), so that case has no input a test can make.
//!
//! **What a stream may cost.** New rows are kept apart from the file's children and joined to them when the stream is written, so each is a push and
//! not an insert among the children that follow the last row (a stream may have hundreds of thousands of comments there). The size of what they add is
//! counted as they are added. **A stream that would pass `MAX_PACKAGE_XML_BYTES` is written as its rows alone**: the byte order mark and the XML
//! declaration (with the line end after it) and the root's tag stay; the comments (before the root, between the rows and after it), the instructions and
//! the white space go, and the quotes are double. That is how the writer before a change wrote every stream: a save it could make must not become a
//! refusal when the rows fit, and the save says the comments went ([`TypesTree::compacted`]). Only rows that do not fit by themselves are
//! [`WriteError::Limit`], not a part that cannot be read back; the count of what the new rows add is not a reason to refuse, only to skip building the
//! stream that cannot fit.

use std::collections::{HashMap, HashSet};

use diacraft_opc::MAX_PACKAGE_XML_BYTES;
use diacraft_opc::name::{fold, lenient_decode};
use diacraft_opc::xml::{self, Node, SpanKind};
use diacraft_opc::{CONTENT_TYPES_ITEM, item_name};

use super::WriteError;
use super::build::{element, with_prefix};
use super::parts::{NS_TYPES, Tree};

/// The longest white space, in bytes, that is repeated before a new row. A longer gap is cut to its line end, if it has one.
const MAX_GAP: usize = 64;

/// The rows of a stream: the `Default` rows as `(Extension, ContentType)` and the `Override` rows as `(PartName, ContentType)`, each in the order
/// of the stream. A row that lacks an attribute is not a row, as `diacraft_opc` reads the stream.
type Rows = (Vec<(String, String)>, Vec<(String, String)>);

/// Where the bytes of a child come from.
#[derive(Clone, Copy)]
enum Raw {
    /// The file's own bytes `[start, end)`.
    Bytes(usize, usize),
    /// A line end the writer repeats from the file's: `\n` or `\r\n`.
    Line(&'static [u8]),
    /// Nothing yet: a new child, or one that was changed, written from its node.
    New,
}

/// A child of the root, and the bytes it was read from.
#[derive(Clone)]
struct Entry {
    node: Node,
    raw: Raw,
}

fn rows_of<'a>(entries: impl Iterator<Item = &'a Entry>) -> Rows {
    let (mut defaults, mut overrides) = (Vec::new(), Vec::new());
    for entry in entries {
        let Node::El(row) = &entry.node else { continue };
        let (list, key) = if row.is("Default") {
            (&mut defaults, "Extension")
        } else if row.is("Override") {
            (&mut overrides, "PartName")
        } else {
            continue;
        };
        if let (Some(k), Some(c)) = (row.attr(key), row.attr("ContentType")) {
            list.push((k.to_owned(), c.to_owned()));
        }
    }
    (defaults, overrides)
}

fn kind_of(node: &Node) -> SpanKind {
    match node {
        Node::El(_) => SpanKind::Element,
        Node::Text(_) => SpanKind::Text,
        Node::CData(_) => SpanKind::CData,
        Node::Comment(_) => SpanKind::Comment,
        Node::Pi { .. } => SpanKind::Pi,
    }
}

fn is_blank(node: &Node) -> bool {
    matches!(node, Node::Text(t) if crate::passthrough::is_blank(t))
}

/// The key of a part named by a row: its name with the escapes decoded and the case folded, as the reader of the stream (`diacraft_opc`) matches it.
fn row_key(part_name: &str) -> String {
    fold(&lenient_decode(part_name))
}

/// The stream a file had.
struct Original {
    bytes: Vec<u8>,
    /// Its rows when it was read.
    rows: Rows,
    /// Where the root's content starts and ends in `bytes`, when the children could be found there and are the tree's.
    inner: Option<(usize, usize)>,
    /// The quote the rows were written with: `'` or `"`.
    quote: u8,
}

/// The quote the last row that has attributes was written with: the first quote after its first `=`.
fn quote_of(bytes: &[u8], entries: &[Entry]) -> Option<u8> {
    entries.iter().rev().find_map(|entry| {
        let (Node::El(_), Raw::Bytes(a, b)) = (&entry.node, entry.raw) else { return None };
        let tag = bytes.get(a..b)?;
        let after_equals = tag.get(tag.iter().position(|c| *c == b'=')?.checked_add(1)?..)?;
        after_equals.iter().find(|c| !c.is_ascii_whitespace()).copied().filter(|c| matches!(*c, b'\'' | b'"'))
    })
}

/// `text`, an element written with double quotes (as the tree writer writes), with single quotes: the delimiters change, and a `'` in a value is
/// `&apos;`. (A `"` in a value is written `&quot;`, so every `"` in the text is a delimiter.)
fn single_quoted(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len().saturating_add(8));
    let mut i = 0usize;
    while let Some(&byte) = text.get(i) {
        match byte {
            b'"' => out.push(b'\''),
            b'\'' => out.extend_from_slice(b"&apos;"),
            b'&' if text.get(i..).is_some_and(|rest| rest.starts_with(b"&quot;")) => {
                out.push(b'"');
                i += "&quot;".len() - 1;
            }
            other => out.push(other),
        }
        i += 1;
    }
    out
}

/// Whether the white space of an entry is short enough to repeat.
fn is_short(entry: &Entry) -> bool {
    match entry.raw {
        Raw::Bytes(a, b) => b.saturating_sub(a) <= MAX_GAP,
        Raw::Line(_) => true,
        Raw::New => matches!(&entry.node, Node::Text(t) if t.len() <= MAX_GAP),
    }
}

/// The byte order mark, the XML declaration and the line end after it that a stream starts with, of the bytes before its root: nothing else of them (a
/// comment before the root is not kept by the rows-alone form).
fn declaration_of(prolog: &[u8]) -> &[u8] {
    let bom = if prolog.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let rest = prolog.get(bom..).unwrap_or_default();
    if !rest.starts_with(b"<?xml") {
        return prolog.get(..bom).unwrap_or_default();
    }
    let Some(close) = rest.windows(2).position(|w| w == b"?>") else { return prolog.get(..bom).unwrap_or_default() };
    let mut end = bom.saturating_add(close).saturating_add(2);
    match prolog.get(end..) {
        Some(after) if after.starts_with(b"\r\n") => end = end.saturating_add(2),
        Some(after) if after.starts_with(b"\n") || after.starts_with(b"\r") => end = end.saturating_add(1),
        _ => {}
    }
    prolog.get(..end).unwrap_or_default()
}

/// The bytes of an entry's white space as it is written.
fn gap_len(entry: &Entry) -> usize {
    match entry.raw {
        Raw::Bytes(a, b) => b.saturating_sub(a),
        Raw::Line(line) => line.len(),
        Raw::New => match &entry.node {
            Node::Text(t) => t.len(),
            _ => 0,
        },
    }
}

/// Where new rows go and the gap that sets one apart from the row before it.
struct Anchor {
    /// The new rows go after `entries[..at]`: after the last element, or at the end if there is none.
    at: usize,
    gap: Option<Entry>,
}

/// The anchor of `entries`: looks at the children from the end back to the last element, and at the white space before it. `bytes` are the file's, for
/// the line end of a gap that is cut.
fn anchor_of(entries: &[Entry], bytes: Option<&[u8]>) -> Anchor {
    let last = entries.iter().rposition(|e| matches!(e.node, Node::El(_)));
    crate::work::type_scan(entries.len().saturating_sub(last.unwrap_or(0)));
    let Some(last) = last else { return Anchor { at: entries.len(), gap: None } };
    let before = last.checked_sub(1).and_then(|at| entries.get(at)).filter(|e| is_blank(&e.node));
    // A gap that is short is repeated. A long one is cut to a line end, in the file's style, if it has one, and to nothing if not.
    let gap = before.and_then(|e| {
        if is_short(e) {
            return Some(e.clone());
        }
        let Node::Text(text) = &e.node else { return None };
        if !text.contains('\n') {
            return None;
        }
        let crlf = match (e.raw, bytes) {
            (Raw::Bytes(a, b), Some(bytes)) => bytes.get(a..b).is_some_and(|gap| gap.windows(2).any(|w| w == b"\r\n")),
            _ => false,
        };
        Some(Entry { node: Node::Text("\n".to_owned()), raw: Raw::Line(if crlf { b"\r\n" } else { b"\n" }) })
    });
    Anchor { at: last.saturating_add(1), gap }
}

/// `[Content_Types].xml` as its children, each with the bytes it had. See the [module documentation](self).
pub(super) struct TypesTree {
    /// The root, with its attributes and no children (they are in `entries` and `tail`).
    tree: Tree,
    entries: Vec<Entry>,
    /// New rows, and the gaps between them, in the order they were added; they sit in the stream after `entries[..anchor.at]`.
    tail: Vec<Entry>,
    /// Where the tail goes. Found at the first new row, and again after rows were taken out or put in among the others.
    anchor: Option<Anchor>,
    /// What the file had, if this was read from one (a new package has no original).
    original: Option<Original>,
    /// The place of the first `Override` for each part, by decoded and folded name: an index in `entries`, or `entries.len()` and the index in `tail`
    /// past that. `None` when it is not built, or since rows moved. Built at the first lookup, so that one lookup is a hash and not a pass over the rows.
    by_part: Option<HashMap<String, usize>>,
    /// The bytes the new rows will add, as near as they can be counted when they are added.
    grown: usize,
    /// Set by [`TypesTree::bytes`] when the stream had to be written as its rows alone to fit.
    compacted: std::cell::Cell<bool>,
}

impl TypesTree {
    /// A stream with the `Default` rows given and no `Override`.
    pub(super) fn new(defaults: &[(&str, &str)]) -> TypesTree {
        let entries = defaults
            .iter()
            .map(|(e, c)| {
                let row = element("Default", vec![("Extension".to_owned(), (*e).to_owned()), ("ContentType".to_owned(), (*c).to_owned())], vec![]);
                Entry { node: Node::El(row), raw: Raw::New }
            })
            .collect();
        let root = element("Types", vec![("xmlns".to_owned(), NS_TYPES.to_owned())], Vec::new());
        TypesTree {
            tree: Tree::new(root),
            entries,
            tail: Vec::new(),
            anchor: None,
            original: None,
            by_part: None,
            grown: 0,
            compacted: std::cell::Cell::new(false),
        }
    }

    /// The stream of a file, from its bytes as they were inflated.
    pub(super) fn read(bytes: &[u8]) -> Result<TypesTree, WriteError> {
        let mut tree = Tree::parse(CONTENT_TYPES_ITEM, bytes)?;
        if !tree.root.is("Types") {
            return Err(WriteError::Mismatch("the content types stream has another root"));
        }
        let children = std::mem::take(&mut tree.root.children);
        // The bytes of each child, if the scan and the tree agree on the children (their number and their kinds).
        let spans = xml::child_spans(bytes, children.len())
            .filter(|s| s.children.len() == children.len() && s.children.iter().zip(&children).all(|((kind, _, _), node)| *kind == kind_of(node)));
        let entries: Vec<Entry> = children
            .into_iter()
            .enumerate()
            .map(|(i, node)| {
                let raw = spans.as_ref().and_then(|s| s.children.get(i)).map_or(Raw::New, |(_, a, b)| Raw::Bytes(*a, *b));
                Entry { node, raw }
            })
            .collect();
        let original = Original {
            bytes: bytes.to_vec(),
            rows: rows_of(entries.iter()),
            quote: if spans.is_some() { quote_of(bytes, &entries).unwrap_or(b'"') } else { b'"' },
            inner: spans.map(|s| s.inner),
        };
        Ok(TypesTree {
            tree,
            entries,
            tail: Vec::new(),
            anchor: None,
            original: Some(original),
            by_part: None,
            grown: 0,
            compacted: std::cell::Cell::new(false),
        })
    }

    /// The children in the order of the stream: those before the new rows, the new rows, and those after.
    fn each(&self) -> impl Iterator<Item = &Entry> {
        let at = self.anchor.as_ref().map_or(self.entries.len(), |a| a.at.min(self.entries.len()));
        self.entries.iter().take(at).chain(self.tail.iter()).chain(self.entries.iter().skip(at))
    }

    /// Joins the new rows to the others, in the order of the stream, for an edit that works among them. Moves rows, so the places are not good.
    fn flatten(&mut self) {
        if !self.tail.is_empty() {
            crate::work::type_scan(self.entries.len());
            let at = self.anchor.as_ref().map_or(self.entries.len(), |a| a.at.min(self.entries.len()));
            let rest = self.entries.split_off(at);
            self.entries.append(&mut self.tail);
            self.entries.extend(rest);
        }
        self.anchor = None;
        self.by_part = None;
    }

    /// The stream as bytes: the file's own when the rows are the file's, the file's with the rows that changed written anew when the children could
    /// be found in it, else the tree written. A new stream gets the standard declaration. **When the file's bytes with the changed rows would pass
    /// `MAX_PACKAGE_XML_BYTES`, the rows alone** (see the module documentation); [`WriteError::Limit`] only when the rows alone pass it.
    pub(super) fn bytes(&self) -> Result<Vec<u8>, WriteError> {
        let xml_error = |error| WriteError::Xml { part: CONTENT_TYPES_ITEM.to_owned(), error };
        let over = |len: usize| u64::try_from(len).is_ok_and(|len| len > MAX_PACKAGE_XML_BYTES);
        let mut root = self.tree.root.clone();
        let Some(original) = &self.original else {
            root.children = self.each().map(|e| e.node.clone()).collect();
            return xml::to_bytes(&root, &self.tree.prolog, &self.tree.epilog, true).map_err(xml_error);
        };
        if rows_of(self.each()) == original.rows {
            return Ok(original.bytes.clone());
        }
        // The file's bytes with the rows that changed, unless the count of what the new rows add says they cannot fit (then the stream that cannot is
        // not built), and unless they do not.
        if !over(original.bytes.len().saturating_add(self.grown)) {
            let out = match self.spliced(original)? {
                Some(out) => out,
                None => {
                    // The tree, with the declaration and what follows the root as the file's bytes.
                    root.children = self.each().map(|e| e.node.clone()).collect();
                    let mut out = self.tree.prolog.clone();
                    xml::write_element(&mut out, &root).map_err(xml_error)?;
                    out.extend_from_slice(&self.tree.epilog);
                    out
                }
            };
            if !over(out.len()) {
                return Ok(out);
            }
        }
        // The rows alone, as the stream was written before the file's bytes were kept: the file's byte order mark and XML declaration (with the line
        // end after it), the root's tag, and the rows in their order with no comment, instruction or white space between them, and nothing after the
        // root. (A comment before the root or after it is as much a comment as one between rows, and a stream can be as large with them.)
        root.children =
            self.each().filter(|e| matches!(&e.node, Node::El(r) if r.is("Default") || r.is("Override"))).map(|e| e.node.clone()).collect();
        let mut rows = declaration_of(&self.tree.prolog).to_vec();
        xml::write_element(&mut rows, &root).map_err(xml_error)?;
        if over(rows.len()) {
            return Err(WriteError::Limit("the rows of the content types alone would be larger than a package allows for the stream"));
        }
        self.compacted.set(true);
        Ok(rows)
    }

    /// Whether the stream had to be written as its rows alone, with its comments and white space gone, to fit in the size a package allows for it.
    pub(super) fn compacted(&self) -> bool {
        self.compacted.get()
    }

    /// The file's bytes with every child that has bytes of the file's written as they were and the others written anew; `None` if that cannot be done
    /// (the children could not be found in the file, or one that is new is not an element, or one with content is to be written in single quotes).
    fn spliced(&self, original: &Original) -> Result<Option<Vec<u8>>, WriteError> {
        let Some((start, end)) = original.inner else { return Ok(None) };
        let (Some(head), Some(tail)) = (original.bytes.get(..start), original.bytes.get(end..)) else { return Ok(None) };
        let mut out = head.to_vec();
        for entry in self.each() {
            match (&entry.node, entry.raw) {
                (_, Raw::Bytes(a, b)) => match original.bytes.get(a..b) {
                    Some(raw) => out.extend_from_slice(raw),
                    None => return Ok(None),
                },
                (_, Raw::Line(line)) => out.extend_from_slice(line),
                (Node::El(row), Raw::New) => {
                    let mut text = Vec::new();
                    xml::write_element(&mut text, row).map_err(|error| WriteError::Xml { part: CONTENT_TYPES_ITEM.to_owned(), error })?;
                    if original.quote == b'\'' {
                        if !row.children.is_empty() {
                            return Ok(None);
                        }
                        text = single_quoted(&text);
                    }
                    out.append(&mut text);
                }
                _ => return Ok(None),
            }
        }
        out.extend_from_slice(tail);
        Ok(Some(out))
    }

    /// The place of the first `Override` for each part, built once and kept up to date by the rows added after it.
    fn index(&mut self) -> &mut HashMap<String, usize> {
        if self.by_part.is_none() {
            // The index is built over the file's children, so the new rows are joined to them first (there are none when a lookup was made before the
            // first new row, as one always is).
            self.flatten();
        }
        let entries = &self.entries;
        self.by_part.get_or_insert_with(|| {
            crate::work::type_scan(entries.len());
            let mut map = HashMap::new();
            for (at, entry) in entries.iter().enumerate() {
                if let Node::El(e) = &entry.node
                    && e.is("Override")
                    && let Some(part) = e.attr("PartName")
                {
                    map.entry(row_key(part)).or_insert(at);
                }
            }
            map
        })
    }

    /// Gives the part `name` the type `content_type` by an `Override`: the one it has is changed, or one is added after the other rows.
    pub(super) fn set_override(&mut self, name: &str, content_type: &str) {
        let key = fold(name);
        if let Some(place) = self.index().get(&key).copied() {
            let entries = self.entries.len();
            let slot = if place < entries { self.entries.get_mut(place) } else { self.tail.get_mut(place - entries) };
            if let Some(Entry { node: Node::El(e), raw }) = slot
                && e.attr("ContentType") != Some(content_type)
            {
                super::patch::set_attr(e, "ContentType", content_type);
                *raw = Raw::New;
            }
            return;
        }
        // The name goes into the row as a part name is written in one (`diacraft_opc` reads it back the same): escaped where a name must be.
        let written = item_name(name).map_or_else(|| name.to_owned(), |item| format!("/{item}"));
        // `<Override PartName="" ContentType=""/>` is 38 bytes more than its two values.
        self.grown = self.grown.saturating_add(written.len()).saturating_add(content_type.len()).saturating_add(38);
        let mut row = element("Override", vec![("PartName".to_owned(), written), ("ContentType".to_owned(), content_type.to_owned())], vec![]);
        with_prefix(&mut row, self.tree.root.prefix().to_owned().as_str());
        if self.anchor.is_none() {
            self.anchor = Some(anchor_of(&self.entries, self.original.as_ref().map(|o| o.bytes.as_slice())));
        }
        if let Some(gap) = self.anchor.as_ref().and_then(|a| a.gap.clone()) {
            self.grown = self.grown.saturating_add(gap_len(&gap));
            self.tail.push(gap);
        }
        self.tail.push(Entry { node: Node::El(row), raw: Raw::New });
        let place = self.entries.len().saturating_add(self.tail.len()).saturating_sub(1);
        // The rows before this one did not move: the index stays good, with this row in it.
        if let Some(map) = &mut self.by_part {
            map.insert(key, place);
        }
    }

    /// Takes out the `Override` of the part `name`, if it has one, with the white space before it.
    pub(super) fn remove_override(&mut self, name: &str) {
        let key = fold(name);
        self.drop_overrides(|folded| folded == key);
    }

    /// Takes out the `Override` of every part whose folded name is in `names`, in one pass.
    pub(super) fn remove_overrides(&mut self, names: &HashSet<String>) {
        self.drop_overrides(|folded| names.contains(folded));
    }

    /// One pass over the rows: every `Override` whose part name (decoded and folded, as the reader of the stream matches it) `goes` is taken out, and
    /// the white space before it (so that a stream with a row to a line has no empty lines where the rows were).
    fn drop_overrides(&mut self, goes: impl Fn(&str) -> bool) {
        self.flatten();
        let entries = std::mem::take(&mut self.entries);
        crate::work::overrides(entries.len());
        let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
        for entry in entries {
            let drop = matches!(&entry.node, Node::El(e) if e.is("Override") && e.attr("PartName").is_some_and(|p| goes(&row_key(p))));
            if drop {
                if out.last().is_some_and(|last| is_blank(&last.node)) {
                    out.pop();
                }
            } else {
                out.push(entry);
            }
        }
        self.entries = out;
    }

    /// The type the stream's `Default` for `extension` gives (compared ASCII case-insensitively), when it has one that gives a type.
    pub(super) fn default_of(&self, extension: &str) -> Option<String> {
        self.each().find_map(|e| match &e.node {
            Node::El(r) if r.is("Default") && r.attr("Extension").is_some_and(|x| x.eq_ignore_ascii_case(extension)) => {
                r.attr("ContentType").map(str::to_owned)
            }
            _ => None,
        })
    }

    /// Adds a `Default` for `extension` if there is none that gives a type (a row with no `ContentType` is not a row: the reader skips it), after the
    /// other `Default` rows.
    pub(super) fn ensure_default(&mut self, extension: &str, content_type: &str) {
        let has = self.each().any(|e| {
            matches!(&e.node, Node::El(r) if r.is("Default")
                && r.attr("ContentType").is_some()
                && r.attr("Extension").is_some_and(|x| x.eq_ignore_ascii_case(extension)))
        });
        if has {
            return;
        }
        self.flatten();
        let mut row =
            element("Default", vec![("Extension".to_owned(), extension.to_owned()), ("ContentType".to_owned(), content_type.to_owned())], vec![]);
        with_prefix(&mut row, self.tree.root.prefix().to_owned().as_str());
        self.grown = self.grown.saturating_add(extension.len()).saturating_add(content_type.len()).saturating_add(38 + MAX_GAP);
        let row = Entry { node: Node::El(row), raw: Raw::New };
        // `Default` rows come first, as in every sample. The new one is set apart from the row it follows as that row is from the one before it; with
        // no `Default`, it goes before the first row and is set apart from it as that row is from what is before it.
        let gap_before = |entries: &[Entry], at: usize| {
            at.checked_sub(1).and_then(|before| entries.get(before)).filter(|e| is_blank(&e.node) && is_short(e)).cloned()
        };
        match self.entries.iter().rposition(|e| matches!(&e.node, Node::El(r) if r.is("Default"))) {
            Some(last) => {
                let gap = gap_before(&self.entries, last);
                let mut at = last.saturating_add(1).min(self.entries.len());
                if let Some(gap) = gap {
                    self.entries.insert(at, gap);
                    at = at.saturating_add(1);
                }
                self.entries.insert(at, row);
            }
            None => {
                let first = self.entries.iter().position(|e| matches!(e.node, Node::El(_))).unwrap_or(self.entries.len());
                let gap = gap_before(&self.entries, first);
                self.entries.insert(first, row);
                if let Some(gap) = gap {
                    self.entries.insert(first.saturating_add(1), gap);
                }
            }
        }
    }
}
