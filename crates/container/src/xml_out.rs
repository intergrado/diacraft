//! Writing an XML tree back to bytes.
//!
//! The tree of [`crate::xml`] keeps names as written, attributes in order, the whitespace between elements, text, CDATA, comments and
//! processing instructions, and this writes all of them back. What a part states outside its root (the XML declaration, a byte order
//! mark, a comment before or after the root) is not in the tree: [`split_document`] finds where the root starts and ends in the part's
//! own bytes, so a save that edits a part can keep those bytes (`xml.rs`, "What it does not do").
//!
//! **What is written, and how.** Names and values are checked first, so that nothing written is not well-formed and a damaged tree is an
//! [`XmlWriteError`] and not a damaged part: an element or attribute name that is not an XML name, a character XML 1.0 does not allow
//! (a control character, NUL), two attributes of one name on one element, `--` in a comment, and a processing instruction that would end
//! early. Attribute values are in double quotes, with `&`, `<` and `"` escaped, and a line feed, a carriage return and a tab as character
//! references (a literal one would read back as a space). Text escapes `&`, `<` and `>`, and a carriage return as a reference; a line
//! feed is written as it is. CDATA is written as CDATA, split where it holds `]]>`. An element with no children is written empty (`<a/>`).
//! The quote style of an element read with single quotes changes to double quotes: R9 says a writer need not match it.
//!
//! The tree is written with an explicit stack, so no tree, however deep, makes the writer recurse. A tree read from a file is never deeper
//! than [`crate::limits::XML_DEPTH_CEILING`].

use std::collections::HashSet;
use std::fmt;

use crate::xml::{El, Node, is_name, xml_char};

/// Why a tree could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlWriteError {
    /// A character XML 1.0 does not allow, in text, an attribute value, CDATA, a comment or an instruction. `what` says where; `ch` is the
    /// character's number.
    NotXmlChar { what: &'static str, ch: u32 },
    /// An element or attribute name that is not an XML name. The name is cut to 64 characters.
    BadName(String),
    /// An element with two attributes of one name.
    DuplicateAttribute(String),
    /// A comment or an instruction that cannot be written as it is: `--` or a final `-` in a comment, `?>` in an instruction, an
    /// instruction target that is not a name or is `xml`.
    BadNode(&'static str),
}

impl fmt::Display for XmlWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmlWriteError::NotXmlChar { what, ch } => write!(f, "{what} holds the character U+{ch:04X}, which XML cannot hold"),
            XmlWriteError::BadName(name) => write!(f, "\"{}\" is not an XML name", name.escape_debug()),
            XmlWriteError::DuplicateAttribute(name) => write!(f, "an element has the attribute \"{}\" twice", name.escape_debug()),
            XmlWriteError::BadNode(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for XmlWriteError {}

/// Writes `root` as a document: `prolog` (the bytes before the root; see [`split_document`]), the root, then `epilog`. The prolog and epilog are
/// copied as they are: they were read from a part that parsed, and they are not checked again. With neither, the document starts with
/// `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` and a line break, when `declaration` is true.
pub fn to_bytes(root: &El, prolog: &[u8], epilog: &[u8], declaration: bool) -> Result<Vec<u8>, XmlWriteError> {
    let mut out = Vec::new();
    if prolog.is_empty() {
        if declaration {
            out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
        }
    } else {
        out.extend_from_slice(prolog);
    }
    write_element(&mut out, root)?;
    out.extend_from_slice(epilog);
    Ok(out)
}

/// Writes one element and everything in it, with no declaration.
pub fn write_element(out: &mut Vec<u8>, root: &El) -> Result<(), XmlWriteError> {
    // (element, index of the next child to write)
    let mut stack: Vec<(&El, usize)> = Vec::new();
    open(out, root)?;
    stack.push((root, 0));
    while let Some((el, at)) = stack.pop() {
        let Some(node) = el.children.get(at) else {
            if !el.children.is_empty() {
                out.extend_from_slice(b"</");
                out.extend_from_slice(el.name.as_bytes());
                out.push(b'>');
            }
            continue;
        };
        stack.push((el, at + 1));
        match node {
            Node::El(child) => {
                open(out, child)?;
                stack.push((child, 0));
            }
            Node::Text(t) => text(out, t, "text")?,
            Node::CData(c) => cdata(out, c)?,
            Node::Comment(c) => comment(out, c)?,
            Node::Pi { target, data } => pi(out, target, data)?,
        }
    }
    Ok(())
}

/// A document written **one child of its root at a time**, for a part whose tree is never held whole: the declaration (when asked), the root's start tag, each child as it
/// is given, and the end tag. What it makes is what [`to_bytes`] makes of the root with those children, byte for byte (the start tag is left open until the first child
/// or the end says whether the root is empty: `<a/>` and not `<a></a>`). It holds the bytes it has written, and gives them with [`RootWriter::finish`].
pub struct RootWriter {
    out: Vec<u8>,
    name: String,
    children: bool,
}

impl RootWriter {
    /// Starts a document: the XML declaration when `declaration` is true, and the start tag of `head` (its name and its attributes: the children it holds, if any, are not
    /// written).
    pub fn begin(head: &El, declaration: bool) -> Result<RootWriter, XmlWriteError> {
        check_name(&head.name)?;
        check_attributes(head)?;
        let mut out = Vec::new();
        if declaration {
            out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
        }
        out.push(b'<');
        out.extend_from_slice(head.name.as_bytes());
        for (k, v) in &head.attrs {
            out.push(b' ');
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b"=\"");
            escape_attribute(&mut out, v)?;
            out.push(b'"');
        }
        Ok(RootWriter { out, name: head.name.clone(), children: false })
    }

    /// Writes one child of the root, and everything in it.
    pub fn child(&mut self, node: &Node) -> Result<(), XmlWriteError> {
        if !self.children {
            self.out.push(b'>');
            self.children = true;
        }
        match node {
            Node::El(e) => write_element(&mut self.out, e),
            Node::Text(t) => text(&mut self.out, t, "text"),
            Node::CData(c) => cdata(&mut self.out, c),
            Node::Comment(c) => comment(&mut self.out, c),
            Node::Pi { target, data } => pi(&mut self.out, target, data),
        }
    }

    /// The bytes written so far, the declaration and the start tag included (the end tag is not written yet).
    pub fn len(&self) -> usize {
        self.out.len()
    }

    /// Whether nothing has been written (never so once [`RootWriter::begin`] has made one).
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Ends the document: the root's end tag, or `/>` when it had no children. Gives the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        if self.children {
            self.out.extend_from_slice(b"</");
            self.out.extend_from_slice(self.name.as_bytes());
            self.out.push(b'>');
        } else {
            self.out.extend_from_slice(b"/>");
        }
        self.out
    }
}

/// The start tag of `el`: `<name a="v">`, or `<name a="v"/>` when it has no children.
fn open(out: &mut Vec<u8>, el: &El) -> Result<(), XmlWriteError> {
    check_name(&el.name)?;
    out.push(b'<');
    out.extend_from_slice(el.name.as_bytes());
    check_attributes(el)?;
    for (k, v) in &el.attrs {
        out.push(b' ');
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b"=\"");
        escape_attribute(out, v)?;
        out.push(b'"');
    }
    if el.children.is_empty() {
        out.extend_from_slice(b"/>");
    } else {
        out.push(b'>');
    }
    Ok(())
}

fn check_name(name: &str) -> Result<(), XmlWriteError> {
    if is_name(name) { Ok(()) } else { Err(XmlWriteError::BadName(name.chars().take(64).collect())) }
}

/// Every attribute name is a name, and no name is used twice. Most elements have a handful of attributes, so they are compared in pairs;
/// an element with many uses a set.
fn check_attributes(el: &El) -> Result<(), XmlWriteError> {
    for (k, _) in &el.attrs {
        check_name(k)?;
    }
    let twice = |k: &str| XmlWriteError::DuplicateAttribute(k.chars().take(64).collect());
    if el.attrs.len() <= 16 {
        for (i, (k, _)) in el.attrs.iter().enumerate() {
            if el.attrs.iter().skip(i + 1).any(|(other, _)| other == k) {
                return Err(twice(k));
            }
        }
    } else {
        let mut seen = HashSet::with_capacity(el.attrs.len());
        for (k, _) in &el.attrs {
            if !seen.insert(k.as_str()) {
                return Err(twice(k));
            }
        }
    }
    Ok(())
}

fn check_chars(s: &str, what: &'static str) -> Result<(), XmlWriteError> {
    match s.chars().find(|c| !xml_char(*c)) {
        Some(c) => Err(XmlWriteError::NotXmlChar { what, ch: u32::from(c) }),
        None => Ok(()),
    }
}

fn escape_attribute(out: &mut Vec<u8>, value: &str) -> Result<(), XmlWriteError> {
    check_chars(value, "an attribute value")?;
    for c in value.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '"' => out.extend_from_slice(b"&quot;"),
            '\n' => out.extend_from_slice(b"&#10;"),
            '\r' => out.extend_from_slice(b"&#13;"),
            '\t' => out.extend_from_slice(b"&#9;"),
            other => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    Ok(())
}

fn text(out: &mut Vec<u8>, value: &str, what: &'static str) -> Result<(), XmlWriteError> {
    check_chars(value, what)?;
    for c in value.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '\r' => out.extend_from_slice(b"&#13;"),
            other => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    Ok(())
}

/// A CDATA section. A carriage return cannot be written inside one (it would read back as a line feed), so a section that holds one is
/// written as text, which says the same thing.
fn cdata(out: &mut Vec<u8>, value: &str) -> Result<(), XmlWriteError> {
    check_chars(value, "a CDATA section")?;
    if value.contains('\r') {
        return text(out, value, "a CDATA section");
    }
    out.extend_from_slice(b"<![CDATA[");
    out.extend_from_slice(value.replace("]]>", "]]]]><![CDATA[>").as_bytes());
    out.extend_from_slice(b"]]>");
    Ok(())
}

fn comment(out: &mut Vec<u8>, value: &str) -> Result<(), XmlWriteError> {
    check_chars(value, "a comment")?;
    if value.contains("--") || value.ends_with('-') {
        return Err(XmlWriteError::BadNode("a comment holds `--` or ends in `-`"));
    }
    out.extend_from_slice(b"<!--");
    out.extend_from_slice(value.as_bytes());
    out.extend_from_slice(b"-->");
    Ok(())
}

fn pi(out: &mut Vec<u8>, target: &str, data: &str) -> Result<(), XmlWriteError> {
    if !is_name(target) || target.eq_ignore_ascii_case("xml") {
        return Err(XmlWriteError::BadNode("a processing instruction has no valid target"));
    }
    check_chars(data, "a processing instruction")?;
    if data.contains("?>") {
        return Err(XmlWriteError::BadNode("a processing instruction holds `?>`"));
    }
    out.extend_from_slice(b"<?");
    out.extend_from_slice(target.as_bytes());
    if !data.is_empty() {
        out.push(b' ');
        out.extend_from_slice(data.as_bytes());
    }
    out.extend_from_slice(b"?>");
    Ok(())
}

/// Where the root element starts and ends in the bytes of a part: `(start, end)`, so that `bytes[..start]` is the prolog (a byte order
/// mark, the XML declaration, comments, white space) and `bytes[end..]` the epilog. `None` for a part that is not UTF-8 text with a
/// root (UTF-16, or bytes that do not parse far enough): the caller then writes a fresh declaration and no epilog.
///
/// The scan is one pass of the XML reader over the bytes and allocates nothing in proportion to them.
pub fn split_document(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[b'<', 0]) || bytes.starts_with(&[0, b'<']) {
        return None;
    }
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let config = reader.config_mut();
    config.trim_text(false);
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    // The reader skips a byte order mark and counts from after it.
    let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let at = |reader: &quick_xml::Reader<&[u8]>| usize::try_from(reader.buffer_position()).ok().and_then(|p| p.checked_add(bom));
    // Where the tag that was just read starts: the last `<` before the position after it. A start tag holds no `<` (the tree reader refuses
    // one in a value).
    let tag_start = |end: usize| bytes.get(..end).and_then(|head| head.iter().rposition(|b| *b == b'<'));
    let (mut start, mut depth) = (None, 0usize);
    loop {
        match reader.read_event().ok()? {
            quick_xml::events::Event::Start(_) => {
                if depth == 0 {
                    start = tag_start(at(&reader)?);
                }
                depth += 1;
            }
            quick_xml::events::Event::Empty(_) => {
                if depth == 0 {
                    let end = at(&reader)?;
                    return Some((tag_start(end)?, end));
                }
            }
            quick_xml::events::Event::End(_) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((start?, at(&reader)?));
                }
            }
            quick_xml::events::Event::Eof => return None,
            _ => {}
        }
    }
}

/// What a direct child of the root element is, for [`child_spans`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// An element, with everything in it, from its `<` to the `>` that closes it (or ends it, if it is written empty).
    Element,
    /// Text, character references included: a run of them is one child, as the tree makes one.
    Text,
    /// A CDATA section.
    CData,
    /// A comment.
    Comment,
    /// A processing instruction.
    Pi,
}

/// Where the direct children of the root element lie in the bytes of a part: see [`child_spans`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildSpans {
    /// Where the root's content starts (just after its start tag) and ends (just before its end tag): the bytes before and after are the part, but for
    /// its children.
    pub inner: (usize, usize),
    /// The children in order, each as its kind and its `[start, end)` bytes. They follow one another and cover `inner` exactly.
    pub children: Vec<(SpanKind, usize, usize)>,
}

/// Where the direct children of the root element lie in the bytes of a part, so that a writer that changes a few of them can write the others as
/// they were (their quotes, their spacing and their line ends, which the tree does not keep). `None` for a part that is not UTF-8 text with a root that
/// has content (UTF-16, a root written empty, bytes that do not parse far enough), and for one with more than `max_children` children: the caller
/// then writes the tree.
///
/// The children are the ones the tree of [`parse`](crate::xml::parse) makes, in the same order and of the same kinds, and a caller that depends on it
/// should compare them (a run of text and references is one child, a comment or an instruction outside the root is none). The scan is one pass of
/// the XML reader over the bytes and allocates what the spans take.
pub fn child_spans(bytes: &[u8], max_children: usize) -> Option<ChildSpans> {
    let (root_start, root_end) = split_document(bytes)?;
    let slice = bytes.get(root_start..root_end)?;
    let mut reader = quick_xml::Reader::from_reader(slice);
    let config = reader.config_mut();
    config.trim_text(false);
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    let position = |reader: &quick_xml::Reader<&[u8]>| usize::try_from(reader.buffer_position()).ok();
    let mut children: Vec<(SpanKind, usize, usize)> = Vec::new();
    let (mut depth, mut inner_start, mut element_start, mut before) = (0usize, None, 0usize, 0usize);
    // The run of text and references being read, as (start, end).
    let mut run: Option<(usize, usize)> = None;
    loop {
        let event = reader.read_event().ok()?;
        let after = position(&reader)?;
        let mut push = |kind: SpanKind, start: usize, end: usize, run: &mut Option<(usize, usize)>| {
            if let Some((a, b)) = run.take() {
                children.push((SpanKind::Text, a, b));
            }
            children.push((kind, start, end));
        };
        match event {
            quick_xml::events::Event::Start(_) => {
                if depth == 0 {
                    inner_start = Some(after);
                } else if depth == 1 {
                    if let Some((a, b)) = run.take() {
                        children.push((SpanKind::Text, a, b));
                    }
                    element_start = before;
                }
                depth += 1;
            }
            quick_xml::events::Event::Empty(_) => {
                if depth == 0 {
                    return None;
                }
                if depth == 1 {
                    push(SpanKind::Element, before, after, &mut run);
                }
            }
            quick_xml::events::Event::End(_) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    if let Some((a, b)) = run.take() {
                        children.push((SpanKind::Text, a, b));
                    }
                    let start = inner_start?;
                    // The scan counted from the root's start: the spans are in the part's bytes.
                    let shifted = children
                        .into_iter()
                        .map(|(kind, a, b)| Some((kind, root_start.checked_add(a)?, root_start.checked_add(b)?)))
                        .collect::<Option<Vec<_>>>()?;
                    return Some(ChildSpans { inner: (root_start.checked_add(start)?, root_start.checked_add(before)?), children: shifted });
                }
                if depth == 1 {
                    push(SpanKind::Element, element_start, after, &mut run);
                }
            }
            quick_xml::events::Event::Text(_) | quick_xml::events::Event::GeneralRef(_) => {
                if depth == 1 {
                    run = Some((run.map_or(before, |(a, _)| a), after));
                }
            }
            quick_xml::events::Event::CData(_) if depth == 1 => push(SpanKind::CData, before, after, &mut run),
            quick_xml::events::Event::Comment(_) if depth == 1 => push(SpanKind::Comment, before, after, &mut run),
            quick_xml::events::Event::PI(_) if depth == 1 => push(SpanKind::Pi, before, after, &mut run),
            quick_xml::events::Event::Eof | quick_xml::events::Event::Decl(_) | quick_xml::events::Event::DocType(_) => return None,
            _ => {}
        }
        if children.len() > max_children {
            return None;
        }
        before = after;
    }
}
