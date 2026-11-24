//! Where the tags of an XML part lie in its bytes, so that a writer can change a few of them and copy every other byte as it was (`/
//! ` R1: a save splices an edit into the source bytes and never writes the part again from a tree).
//!
//! [`scan_tags`] reads a part once with the XML pull reader the tree is read with ([`crate::xml`]) and gives, for every start tag, empty-element
//! tag and end tag, where it starts and ends, how deep it is, and the run of text that ends where it starts. It builds nothing: no tree, no
//! name is copied, and it is bounded as the tree reader is: depth ([`XmlLimits::max_depth`], never past [`XML_DEPTH_CEILING`]) and the number of
//! events ([`XmlLimits::max_nodes`], which a tree's items never pass). Each event is counted in [`Work::xml_events`]. A part that is not
//! well-formed, has a DTD, or is in UTF-16 (whose bytes a splice cannot edit) is an error, never a half-scan.
//!
//! [`tag_attributes`] then finds the attributes of one tag: where each name and each value lies, so that a value can be replaced, an attribute
//! taken out or one added with every other byte of the tag as it was. It looks at one tag and nothing else, and a tag that is not as a
//! well-formed one is written gives `None`.
//!
//! **What this does not do**: resolve a name to a namespace (the caller does, with the `xmlns` attributes it reads from the tags), or unescape
//! a value (a value is given as the bytes between its quotes).

use quick_xml::events::Event;

use crate::limits::{XML_DEPTH_CEILING, XmlLimits};
use crate::work::Work;
use crate::xml::{ParseOptions, XmlError};

/// What a tag is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    /// `<name ...>`: the element has content, and an end tag follows.
    Open,
    /// `<name .../>`: an element with none.
    Empty,
    /// `</name>`.
    Close,
}

/// One tag of a part, and where it lies.
#[derive(Clone, Copy, Debug)]
pub struct Tag<'a> {
    pub kind: TagKind,
    /// The qualified name as written.
    pub name: &'a str,
    /// Where the tag starts (its `<`), and where it ends (after its `>`), from byte 0 of the part.
    pub start: usize,
    pub end: usize,
    /// How many elements enclose the element: 0 for the root. An end tag has the depth of the element it ends.
    pub depth: usize,
    /// The run of text and references that ends exactly where this tag starts, as `(start, end)`, when the event before the tag was text;
    /// a comment, an instruction or a CDATA section between the two ends the run.
    pub text_before: Option<(usize, usize)>,
}

/// Why a scan gave nothing: the part, or what the caller's visit refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError<E> {
    /// The part is not one this scan reads: the same error the tree reader gives for it.
    Xml(XmlError),
    /// The caller's visit stopped the scan.
    Visit(E),
}

fn malformed(offset: usize, message: &str) -> XmlError {
    XmlError::Malformed { offset, message: message.to_owned() }
}

/// Whether the bytes are UTF-16, by a byte order mark or by beginning `<` and a zero byte (XML 1.0 appendix F).
pub fn is_utf16(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[b'<', 0]) || bytes.starts_with(&[0, b'<'])
}

/// Calls `visit` for every tag of `bytes`, in document order, within `limits`, adding each event to `work` (`xml_events`). A tag's name is
/// the name as written, and must be UTF-8. A part that does not scan is [`ScanError::Xml`] and `visit` has seen the tags before the fault.
pub fn scan_tags<E>(bytes: &[u8], limits: &XmlLimits, work: &mut Work, visit: impl FnMut(&Tag<'_>) -> Result<(), E>) -> Result<(), ScanError<E>> {
    scan_tags_with(bytes, limits, ParseOptions::default(), work, visit)
}

/// [`scan_tags`], accepting what `options` asks to accept as the tree reader does ([`crate::xml::parse_with_options`]): with
/// [`ParseOptions::allow_external_doctype`], one DOCTYPE before the root that holds a name and an external id and no internal subset, read and ignored, so
/// that a writer scans a manifest the reader reads (OpenOffice.org's). Every other DTD is an error.
pub fn scan_tags_with<E>(
    bytes: &[u8],
    limits: &XmlLimits,
    options: ParseOptions,
    work: &mut Work,
    mut visit: impl FnMut(&Tag<'_>) -> Result<(), E>,
) -> Result<(), ScanError<E>> {
    if is_utf16(bytes) {
        return Err(ScanError::Xml(malformed(0, "a part in UTF-16 cannot be scanned for a splice")));
    }
    // The reader skips a byte order mark and counts from after it.
    let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let body = std::str::from_utf8(bytes.get(bom..).unwrap_or_default())
        .map_err(|e| ScanError::Xml(malformed(bom.saturating_add(e.valid_up_to()), "bytes that are not UTF-8")))?;
    // XML 1.0 section 2.2, as the tree's parser checks it: a character that is not a `Char` is not well-formed anywhere in the part. A scan that
    // passed one would pass a part the reader refuses.
    if let Some((at, c)) = crate::xml::first_bad_char(body) {
        return Err(ScanError::Xml(malformed(bom.saturating_add(at), &format!("the character U+{:04X} is not allowed in XML", u32::from(c)))));
    }
    let mut reader = quick_xml::Reader::from_str(body);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = false;
    config.check_end_names = true;
    config.allow_unmatched_ends = false;
    config.allow_dangling_amp = false;
    let max_depth = limits.max_depth.min(XML_DEPTH_CEILING);
    let at = |reader: &quick_xml::Reader<&[u8]>| usize::try_from(reader.buffer_position()).ok().and_then(|p| p.checked_add(bom));
    let (mut depth, mut events, mut roots) = (0usize, 0usize, 0usize);
    let mut doctype_seen = false;
    let mut run: Option<(usize, usize)> = None;
    let mut before = bom;
    loop {
        let event = reader.read_event().map_err(|e| ScanError::Xml(malformed(before, &e.to_string())))?;
        let after = at(&reader).ok_or_else(|| ScanError::Xml(malformed(before, "a position past what a part can hold")))?;
        work.xml_events = work.xml_events.saturating_add(1);
        events = events.saturating_add(1);
        if events > limits.max_nodes {
            return Err(ScanError::Xml(XmlError::TooManyNodes { limit: limits.max_nodes }));
        }
        let text_before = run.take();
        match event {
            Event::Start(e) => {
                if depth >= max_depth {
                    return Err(ScanError::Xml(XmlError::TooDeep { limit: max_depth }));
                }
                roots += usize::from(depth == 0);
                let tag = Tag { kind: TagKind::Open, name: e.name().into_inner(), start: before, end: after, depth, text_before };
                visit(&tag).map_err(ScanError::Visit)?;
                depth += 1;
            }
            Event::Empty(e) => {
                if depth >= max_depth {
                    return Err(ScanError::Xml(XmlError::TooDeep { limit: max_depth }));
                }
                roots += usize::from(depth == 0);
                let tag = Tag { kind: TagKind::Empty, name: e.name().into_inner(), start: before, end: after, depth, text_before };
                visit(&tag).map_err(ScanError::Visit)?;
            }
            Event::End(e) => {
                depth = depth.checked_sub(1).ok_or_else(|| ScanError::Xml(malformed(before, "an end tag with nothing open")))?;
                let tag = Tag { kind: TagKind::Close, name: e.name().into_inner(), start: before, end: after, depth, text_before };
                visit(&tag).map_err(ScanError::Visit)?;
            }
            Event::Text(_) | Event::GeneralRef(_) => {
                // A run of text and references is one run, as the tree makes it one text.
                run = Some((text_before.map_or(before, |(start, _)| start), after));
            }
            Event::DocType(doctype) => {
                // As the tree reader takes it: only with the option, before the root and once, written `<!DOCTYPE` and a space (the pull reader takes the
                // keyword with no space after it, which XML 1.0 does not), with an external id and no internal subset. It is read and ignored.
                let spelled = body
                    .get(before.saturating_sub(bom)..)
                    .and_then(|at| at.strip_prefix("<!DOCTYPE"))
                    .is_some_and(|after| after.starts_with([' ', '\t', '\r', '\n']));
                if options.allow_external_doctype && !spelled {
                    return Err(ScanError::Xml(malformed(before, "a DOCTYPE not written `<!DOCTYPE` and a space")));
                }
                let content = doctype.into_inner();
                let accepted = options.allow_external_doctype && depth == 0 && roots == 0 && !doctype_seen && crate::xml::external_id_only(&content);
                if !accepted {
                    return Err(ScanError::Xml(XmlError::EntityRefused { offset: before, what: "a DTD".to_owned() }));
                }
                doctype_seen = true;
            }
            Event::Eof => break,
            Event::CData(_) | Event::Comment(_) | Event::PI(_) | Event::Decl(_) => {}
        }
        before = after;
    }
    if depth != 0 {
        return Err(ScanError::Xml(malformed(before, "an element that is never closed")));
    }
    if roots != 1 {
        return Err(ScanError::Xml(malformed(before, if roots == 0 { "no root element" } else { "more than one root element" })));
    }
    Ok(())
}

/// One attribute of a tag, as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrSpan {
    /// Where the attribute's name starts and ends, from byte 0 of the part.
    pub name: (usize, usize),
    /// Where the value is: the bytes between the quotes, as written (references not resolved).
    pub value: (usize, usize),
    /// Where the white space before the name starts: taking the attribute out takes `lead..end`, so that what follows keeps its place.
    pub lead: usize,
    /// Where the attribute ends, after its closing quote.
    pub end: usize,
    /// The quote the value is written in: `"` or `'`.
    pub quote: u8,
}

/// Where a tag's name ends and where its attributes end: the places a new attribute may be put in (`attrs_end`, after the last attribute or,
/// with none, after the name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagParts {
    pub name_end: usize,
    pub attrs_end: usize,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// The attributes of the tag at `bytes[start..end]` (a start tag or an empty-element tag, as [`scan_tags`] gave its bounds), in order, added to
/// `out` (which is not cleared), and where its name and its attributes end. `None` for a tag that is not written as a well-formed one is: no
/// attribute without a quoted value, no unclosed quote. The walk is one pass over the tag.
pub fn tag_attributes(bytes: &[u8], start: usize, end: usize, out: &mut Vec<AttrSpan>) -> Option<TagParts> {
    let tag = bytes.get(start..end)?;
    if tag.first() != Some(&b'<') {
        return None;
    }
    let mut i = 1;
    while tag.get(i).is_some_and(|&b| !is_space(b) && b != b'/' && b != b'>') {
        i += 1;
    }
    let name_end = start.checked_add(i)?;
    let mut attrs_end = name_end;
    loop {
        let lead = i;
        while tag.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        match tag.get(i) {
            None | Some(b'/' | b'>') => break,
            Some(_) => {}
        }
        let name_from = i;
        while tag.get(i).is_some_and(|&b| !is_space(b) && b != b'=' && b != b'/' && b != b'>') {
            i += 1;
        }
        if i == name_from {
            return None;
        }
        let name_to = i;
        while tag.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        if tag.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while tag.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        let quote = *tag.get(i).filter(|&&q| q == b'"' || q == b'\'')?;
        let value_from = i + 1;
        let length = tag.get(value_from..)?.iter().position(|&b| b == quote)?;
        let value_to = value_from + length;
        i = value_to + 1;
        let abs = |n: usize| start.checked_add(n);
        out.push(AttrSpan {
            name: (abs(name_from)?, abs(name_to)?),
            value: (abs(value_from)?, abs(value_to)?),
            lead: abs(lead)?,
            end: abs(i)?,
            quote,
        });
        attrs_end = abs(i)?;
    }
    Some(TagParts { name_end, attrs_end })
}

/// Whether the bytes are white space only, as XML defines it (and so, if empty, are).
pub fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().copied().all(is_space)
}
