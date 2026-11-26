//! A bounded XML tree that keeps what a later save needs to write back unchanged: attribute order, prefixes and `xmlns` declarations
//! (as ordinary attributes, in their place), the whitespace between elements, text, CDATA, comments and processing instructions
//! Values are the strings the file gave: nothing is read as a number.
//!
//! **What the parser refuses**: a DTD of any kind, an entity declaration, and a reference to any entity but the five
//! predefined ones, as `xml_entity_refused`, before anything is expanded: no entity is ever expanded, so there is nothing to
//! blow up, and nothing is fetched, since no external identifier is ever followed. One exception, off unless a caller asks for it
//! ([`ParseOptions::allow_external_doctype`], which the ODF package layer asks for in its manifest alone): a DOCTYPE that holds a name and
//! an external id and no internal subset is read and ignored; it declares nothing, so every other refusal holds after it. A declared encoding that is not UTF-8 or UTF-16
//! is an error, except `US-ASCII` on an all-ASCII document when a caller asks for it ([`ParseOptions::allow_us_ascii`]), as is a document that is not well-formed: a mismatched or missing end tag, a second root, text outside the
//! root, a duplicate attribute, a bad character reference, a character XML 1.0 does not allow (a literal control character or NUL, in
//! text, an attribute, CDATA, a comment or an instruction), `]]>` in text, `--` in a comment, and attributes not separated by white
//! space. The sibling this module was copied from folded unclosed elements up and dropped unknown entities silently; both are errors
//! here, because a damaged part must be heard.
//!
//! **What it bounds** ([`XmlLimits`]): depth, items (elements, text runs, comments, instructions and attributes, each one), and the
//! size of an attribute name or value. The tree is built with an explicit stack, so no input makes the parser recurse; the tree itself
//! is never deeper than [`XML_DEPTH_CEILING`], so the recursion of dropping or cloning one is bounded too.
//!
//! **What it does not do**: resolve namespaces. Names are as written (`x:note`, `draw:custom-shape`), and matching is by the name as
//! written or by its local part, which is how VSDX parts are best read (the namespace a part states is not the
//! one the samples use). The XML declaration, and any comment or instruction before or after the root, are not in the tree: the raw bytes
//! of the part have them ([`split_document`] finds the root in them), and a save that edits a part patches those bytes.
//!
//! **What it costs** is counted in [`Work::xml_events`] by [`parse_counted`]: one for each event the pull parser gives, so that a test can
//! hold a parse to the size of its input and not to a time.
//!
//! The tree and the way it is read come from DeckCraft's `pptx` crate (`docs/provenance.md`); the limits, the refusals and everything
//! else about hostile input are this crate's.

use std::borrow::Cow;
use std::fmt;

use quick_xml::events::{BytesStart, Event};

use crate::limits::XML_DEPTH_CEILING;
pub use crate::limits::XmlLimits;
use crate::work::Work;
pub use crate::xml_out::{ChildSpans, RootWriter, SpanKind, XmlWriteError, child_spans, split_document, to_bytes, write_element};

/// Why an XML part could not be read. The class words are those, which keeps for ODF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    /// Not well-formed XML, or in an encoding R24 does not allow. Class `xml_malformed`.
    Malformed { offset: usize, message: String },
    /// A DTD, an entity declaration, or a reference to an entity other than the five predefined ones. Nothing is ever fetched or
    /// expanded: this is the refusal. Class `xml_entity_refused`.
    EntityRefused { offset: usize, what: String },
    /// Elements nested deeper than the limit. Class `xml_too_complex` (the spec's table has no word for a limit on XML).
    TooDeep { limit: usize },
    /// More items than the limit. Class `xml_too_complex`.
    TooManyNodes { limit: usize },
    /// An attribute name or value longer than the limit. Class `xml_too_complex`.
    AttributeTooLarge { limit: usize },
}

impl XmlError {
    /// The class word: `xml_malformed`, `xml_entity_refused` (both R22) or `xml_too_complex` (this crate's).
    pub fn class(&self) -> &'static str {
        match self {
            XmlError::Malformed { .. } => "xml_malformed",
            XmlError::EntityRefused { .. } => "xml_entity_refused",
            XmlError::TooDeep { .. } | XmlError::TooManyNodes { .. } | XmlError::AttributeTooLarge { .. } => "xml_too_complex",
        }
    }
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmlError::Malformed { offset, message } => write!(f, "XML is not well-formed at byte {offset}: {}", message.escape_debug()),
            XmlError::EntityRefused { offset, what } => write!(f, "XML at byte {offset} has {}, which is refused", what.escape_debug()),
            XmlError::TooDeep { limit } => write!(f, "XML is nested deeper than {limit} elements"),
            XmlError::TooManyNodes { limit } => write!(f, "XML has more than {limit} items"),
            XmlError::AttributeTooLarge { limit } => write!(f, "an XML attribute is longer than {limit} bytes"),
        }
    }
}

impl std::error::Error for XmlError {}

/// A parsed document: its root element, which holds the rest. [`parse`] gives one, and [`to_bytes`] writes one back.
pub type Tree = El;

/// An element: its name as written, its attributes in the order written, and its children in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct El {
    /// The qualified name as written (`x:note`, `Cell`).
    pub name: String,
    /// Every attribute, `xmlns` declarations included, as `(name as written, value)`, in the order written. Values are unescaped
    /// and normalised as XML 1.0 section 3.3.3 says (a literal line break reads as a space; `&#10;` stays a line feed).
    pub attrs: Vec<(String, String)>,
    /// The children, in order: elements, text (whitespace between elements included), and the rest.
    pub children: Vec<Node>,
}

/// A child of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A child element.
    El(El),
    /// A run of text, references resolved, `CR LF` read as `LF`. Adjacent text and references are one run; a CDATA section is
    /// not part of one.
    Text(String),
    /// A CDATA section's content.
    CData(String),
    /// A comment's content, between `<!--` and `-->`.
    Comment(String),
    /// A processing instruction: its target and the rest of it (`<?NewValue V?>` is target `NewValue`, data `V`).
    Pi { target: String, data: String },
}

impl El {
    /// The part of the name before the first colon, or nothing.
    pub fn prefix(&self) -> &str {
        self.name.split_once(':').map_or("", |(prefix, _)| prefix)
    }

    /// The part of the name after the first colon, or all of it.
    pub fn local(&self) -> &str {
        self.name.split_once(':').map_or(self.name.as_str(), |(_, local)| local)
    }

    /// Whether the local part of the name is `local`.
    pub fn is(&self, local: &str) -> bool {
        self.local() == local
    }

    /// The value of the attribute written as `name` (`r:id`, `N`), the first if it is written twice (which a parsed tree never has).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The value of the first attribute, in the order written, whose local part is `local` whatever its prefix. Namespace
    /// declarations (`xmlns`, `xmlns:x`) are not attributes of this kind and are not found.
    pub fn attr_local(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:") && k.split_once(':').map_or(k.as_str(), |(_, l)| l) == local)
            .map(|(_, v)| v.as_str())
    }

    /// The child elements, in order.
    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
    }

    /// The first child element whose local name is `local`.
    pub fn child(&self, local: &str) -> Option<&El> {
        self.elements().find(|e| e.is(local))
    }

    /// The child elements whose local name is `local`, in order.
    pub fn children_named<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.elements().filter(move |e| e.is(local))
    }

    /// The text and CDATA that are direct children, joined. (Not the text of descendants.)
    pub fn text(&self) -> String {
        let mut out = String::new();
        for node in &self.children {
            if let Node::Text(t) | Node::CData(t) = node {
                out.push_str(t);
            }
        }
        out
    }
}

/// Parses `bytes` with the default limits ([`XmlLimits::default`]). Gives the root element.
pub fn parse(bytes: &[u8]) -> Result<El, XmlError> {
    parse_with(bytes, &XmlLimits::default())
}

/// Parses `bytes` as one XML document with `limits`. Gives the root element.
pub fn parse_with(bytes: &[u8], limits: &XmlLimits) -> Result<El, XmlError> {
    parse_counted(bytes, limits, &mut Work::default())
}

/// Parses `bytes` as one XML document with `limits`, and adds the events read to `work` (whether it parses or is refused). Gives the
/// root element.
pub fn parse_counted(bytes: &[u8], limits: &XmlLimits, work: &mut Work) -> Result<El, XmlError> {
    parse_with_options(bytes, limits, ParseOptions::default(), work)
}

/// [`parse_counted`], and the items the parse counted against `limits.max_nodes`, which are the items of the tree it made (the elements, the attributes and the other nodes, as the limit
/// counts them): for a caller that parses ahead of its turn with a limit that is not the one it will hold the tree to, and keeps the tree when its items are within that one
/// **A tree of `n` items parsed within `limits` is the tree that a parse within any limit of `n` or more makes**, the same events and the same bytes, since a limit only ever ends a parse.
pub fn parse_measured(bytes: &[u8], limits: &XmlLimits, work: &mut Work) -> Result<(El, usize), XmlError> {
    let (text, encoding) = decode(bytes)?;
    let mut parser = Parser::new(&text, encoding, limits, ParseOptions::default());
    let out = parser.run();
    work.xml_events = work.xml_events.saturating_add(parser.events);
    out.map(|root| (root, parser.nodes))
}

/// What a parse accepts beyond the default, each off by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    /// Accept, before the root, one DOCTYPE that carries only an external id (`<!DOCTYPE name PUBLIC "pubid" "system">` or
    /// `<!DOCTYPE name SYSTEM "system">`) and no internal subset, and ignore it: nothing is fetched and nothing is declared, so a reference
    /// to any entity but the five predefined ones is refused as it always is. Old OpenDocument writers put one in a package's manifest
    /// (the census); every other DTD stays `xml_entity_refused`.
    pub allow_external_doctype: bool,
    /// Accept a declaration of the encoding `US-ASCII` (in any case) on a document that is UTF-8 with every byte of its text below 0x80, and read it as UTF-8 (a draw.io file from a tool that writes it).
    /// Off by default: the declaration is refused, as every encoding but UTF-8 and UTF-16 is. A document
    /// with a byte at or above 0x80, or that is UTF-16, is refused though this is on, and so is any other encoding: ASCII is the one text that is UTF-8 whatever it is
    /// called. The text is what follows a byte order mark, when there is one.
    pub allow_us_ascii: bool,
}

/// [`parse_counted`], with `options`.
pub fn parse_with_options(bytes: &[u8], limits: &XmlLimits, options: ParseOptions, work: &mut Work) -> Result<El, XmlError> {
    let (text, encoding) = decode(bytes)?;
    let mut parser = Parser::new(&text, encoding, limits, options);
    let out = parser.run();
    work.xml_events = work.xml_events.saturating_add(parser.events);
    out
}

/// Whether the content of a DOCTYPE (what stands between `<!DOCTYPE` and its `>`) is a name and an external id and nothing else (XML 1.0,
/// productions 28 and 75): `name SYSTEM "literal"` or `name PUBLIC "pubid" "literal"`, either quote, white space between, and no internal
/// subset. Anything else, a bare name included, is not.
pub(crate) fn external_id_only(content: &str) -> bool {
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
    // A literal in either quote, with no such quote inside it; gives its text and what follows it.
    fn literal(s: &str) -> Option<(&str, &str)> {
        let quote = s.chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let body = s.get(1..)?;
        let end = body.find(quote)?;
        Some((body.get(..end)?, body.get(end + 1..)?))
    }
    let pubid_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '\r' | '\n') || "-'()+,./:=?;!*#@$_%".contains(c);
    let rest = content.trim_start_matches(is_space);
    let Some(name_end) = rest.find(is_space) else { return false };
    let (name, rest) = rest.split_at(name_end);
    let rest = rest.trim_start_matches(is_space);
    let after = if let Some(system) = rest.strip_prefix("SYSTEM") {
        if !system.starts_with(is_space) {
            return false;
        }
        literal(system.trim_start_matches(is_space)).map(|(_, after)| after)
    } else if let Some(public) = rest.strip_prefix("PUBLIC") {
        if !public.starts_with(is_space) {
            return false;
        }
        match literal(public.trim_start_matches(is_space)) {
            Some((pubid, after)) if pubid.chars().all(pubid_char) && after.starts_with(is_space) => {
                literal(after.trim_start_matches(is_space)).map(|(_, after)| after)
            }
            _ => None,
        }
    } else {
        None
    };
    is_name(name) && after.is_some_and(|after| after.chars().all(is_space))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16,
}

fn malformed(offset: usize, message: impl Into<String>) -> XmlError {
    XmlError::Malformed { offset, message: message.into() }
}

/// UTF-16 to text. An unpaired surrogate, or half a unit at the end, is an error and not a replacement character. **The text is made at the size it ends with**:
/// the first pass measures it (and finds the unpaired surrogate), the second fills a string that has room for exactly that, so that the string is never grown
/// by doubling, which would hold up to twice the text for a moment, and a caller that counts the memory of a parse knows what it holds ([`parse_size`]).
fn decode_utf16(body: &[u8], little: bool) -> Result<String, XmlError> {
    let (pairs, rest) = body.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(malformed(body.len(), "UTF-16 that ends in half a unit"));
    }
    let units = || pairs.iter().map(|pair| if little { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) });
    let mut size = 0usize;
    for decoded in char::decode_utf16(units()) {
        size = size.saturating_add(decoded.map_err(|_| malformed(0, "an unpaired surrogate in UTF-16"))?.len_utf8());
    }
    let mut text = String::with_capacity(size);
    text.extend(char::decode_utf16(units()).flatten());
    Ok(text)
}

fn decode_utf8(body: &[u8]) -> Result<&str, XmlError> {
    std::str::from_utf8(body).map_err(|e| malformed(e.valid_up_to(), "bytes that are not UTF-8"))
}

/// How the bytes of a document are to be read, from their first bytes: the encoding, and the body that follows the byte order mark if there is one.
enum Form<'b> {
    Utf32,
    Utf8(&'b [u8]),
    Utf16 { body: &'b [u8], little: bool },
}

/// The encoding of `bytes` (UTF-8 or UTF-16, nothing else). A byte order mark is dropped; a document with no mark is UTF-16 only when it begins `<?` in that
/// encoding, as XML 1.0 appendix F says, and UTF-8 otherwise.
fn form_of(bytes: &[u8]) -> Form<'_> {
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        Form::Utf32
    } else if let Some(body) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Form::Utf8(body)
    } else if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        Form::Utf16 { body, little: true }
    } else if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        Form::Utf16 { body, little: false }
    } else if bytes.starts_with(&[b'<', 0, b'?', 0]) {
        Form::Utf16 { body: bytes, little: true }
    } else if bytes.starts_with(&[0, b'<', 0, b'?']) {
        Form::Utf16 { body: bytes, little: false }
    } else {
        Form::Utf8(bytes)
    }
}

/// The text of the document, and the encoding it was in.
fn decode(bytes: &[u8]) -> Result<(Cow<'_, str>, Encoding), XmlError> {
    match form_of(bytes) {
        Form::Utf32 => Err(malformed(0, "UTF-32 is not an encoding of this reader")),
        Form::Utf8(body) => decode_utf8(body).map(|s| (Cow::Borrowed(s), Encoding::Utf8)),
        Form::Utf16 { body, little } => decode_utf16(body, little).map(|text| (Cow::Owned(text), Encoding::Utf16)),
    }
}

/// What a parse of some bytes holds besides the tree it builds, at the most, told from the bytes' first bytes and their length and without reading them: for a caller that
/// keeps a budget of memory and counts what a parse needs before it parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseSize {
    /// The text the parse reads, at the most, in bytes of UTF-8. A tree holds no more of its characters than this, in the strings of its names, values and text.
    /// The bytes themselves in UTF-8; in UTF-16 a unit of two bytes is at most three bytes of UTF-8, so one and a half times the bytes.
    pub text_bytes: u64,
    /// The bytes of a copy of the text that the parse makes and holds until it has done: none for UTF-8, which is read where it is, and all of the text for UTF-16,
    /// which is decoded into a string of its own.
    pub copy_bytes: u64,
}

/// What a parse of `bytes` holds besides the tree ([`ParseSize`]). Bytes that are not text of this reader's encodings (UTF-32) are refused by the parse before
/// it makes anything, and are sized as UTF-8.
#[must_use]
pub fn parse_size(bytes: &[u8]) -> ParseSize {
    match form_of(bytes) {
        Form::Utf16 { body, .. } => {
            let text = (body.len() as u64).saturating_mul(3) / 2;
            ParseSize { text_bytes: text, copy_bytes: text }
        }
        Form::Utf8(_) | Form::Utf32 => ParseSize { text_bytes: bytes.len() as u64, copy_bytes: 0 },
    }
}

/// Whether a declared encoding is the one the bytes were in.
fn encoding_matches(declared: &str, actual: Encoding) -> bool {
    let declared = declared.trim().to_ascii_lowercase();
    match actual {
        Encoding::Utf8 => declared == "utf-8",
        Encoding::Utf16 => matches!(declared.as_str(), "utf-16" | "utf-16le" | "utf-16be"),
    }
}

/// XML 1.0 `Char`: what may appear in a document at all.
pub fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' '..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// The place and the character of the first one in `text` that XML 1.0 does not allow in a document (a control character other than tab, line
/// feed and carriage return, NUL included, or U+FFFE or U+FFFF), found a byte at a time: in UTF-8 those are the bytes below 0x20 and the sequences
/// EF BF BE and EF BF BF (surrogates cannot be in a `str`).
pub fn first_bad_char(text: &str) -> Option<(usize, char)> {
    let bytes = text.as_bytes();
    for (at, &b) in bytes.iter().enumerate() {
        let bad = (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r')) || (b == 0xEF && matches!(bytes.get(at + 1..at + 3), Some([0xBF, 0xBE | 0xBF])));
        if bad {
            return text.get(at..).and_then(|rest| rest.chars().next()).map(|c| (at, c));
        }
    }
    None
}

fn name_start_char(c: char) -> bool {
    matches!(c, ':' | 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

fn name_char(c: char) -> bool {
    name_start_char(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// XML 1.0 `Name`: an element or attribute name, and the target of a processing instruction. What the writer takes as one: a caller that keeps a name it was
/// given (a processing instruction's target) asks it before it keeps one that no writer could write.
pub fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(name_start_char) && chars.all(name_char)
}

/// A literal CR LF or lone CR in a document reads as LF (XML 1.0 section 2.11). A reference to CR is not literal and is not
/// passed through here.
///
/// **One pass, into a string of the size it ends with**: `str::replace` twice made two strings, each grown from nothing by doubling, so that the text of
/// a large part, which has line breaks as a rule, was held two or three times for a moment and the memory was counted by nobody. A text with no CR is not copied.
fn line_ends(s: &str) -> Cow<'_, str> {
    if !s.contains('\r') {
        return Cow::Borrowed(s);
    }
    // A CR LF is one byte less in the result, a lone CR as many bytes as it was.
    let mut out = String::with_capacity(s.len().saturating_sub(s.matches("\r\n").count()));
    let mut rest = s;
    while let Some((head, tail)) = rest.split_once('\r') {
        out.push_str(head);
        out.push('\n');
        rest = tail.strip_prefix('\n').unwrap_or(tail);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// XML 1.0 section 2.2: every character of the document must be a `Char`. A literal control character, NUL, or U+FFFE is not
/// well-formed, in text, in CDATA, in a comment, in an instruction, or in an attribute value.
fn check_chars(s: &str, offset: usize) -> Result<(), XmlError> {
    match s.chars().find(|c| !xml_char(*c)) {
        Some(c) => Err(malformed(offset, format!("the character U+{:04X} is not allowed in XML", u32::from(c)))),
        None => Ok(()),
    }
}

/// Whether the attributes of a start tag, as written after the element name, are each followed by white space or the end of the tag
/// (XML 1.0 section 3.1: `b="1"c="2"` is not well-formed, though a lenient reader takes it). The tag's names and quotes have already
/// been read by the parser, so this looks only at what follows a closing quote, and anything it cannot follow is the parser's to say.
fn attributes_are_separated(raw: &[u8]) -> bool {
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
    let mut i = 0;
    loop {
        while raw.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        if i >= raw.len() || raw.get(i) == Some(&b'/') {
            return true;
        }
        while raw.get(i).is_some_and(|&b| !is_space(b) && b != b'=') {
            i += 1;
        }
        while raw.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        if raw.get(i) != Some(&b'=') {
            return true;
        }
        i += 1;
        while raw.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        let Some(&quote) = raw.get(i).filter(|&&q| q == b'"' || q == b'\'') else { return true };
        let Some(length) = raw.get(i + 1..).and_then(|rest| rest.iter().position(|&b| b == quote)) else { return true };
        i += length + 2;
        if raw.get(i).copied().is_some_and(|b| !is_space(b) && !(b == b'/' && i + 1 == raw.len())) {
            return false;
        }
    }
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}

/// The longest entity or character reference this reader looks for the end of: `&#x10FFFF;` is 11 bytes. A `&` with no `;` within
/// this many bytes is not a reference, and the search never runs on.
const MAX_REFERENCE_BYTES: usize = 64;

/// What a reference stands for.
fn resolve_reference(body: &str, offset: usize) -> Result<Cow<'static, str>, XmlError> {
    if let Some(number) = body.strip_prefix('#') {
        let (digits, radix) = match number.strip_prefix('x') {
            Some(hex) => (hex, 16),
            None => (number, 10),
        };
        if digits.is_empty() || body.len() > MAX_REFERENCE_BYTES || !digits.chars().all(|c| c.is_digit(radix)) {
            return Err(malformed(offset, "a character reference that is not a number"));
        }
        // Leading zeros are legal (`&#0000000065;` is `A`) and do not count toward the length of the number; zero itself is no character.
        let significant = digits.trim_start_matches('0');
        let value = if significant.is_empty() { Some(0) } else { u32::from_str_radix(significant, radix).ok().filter(|_| significant.len() <= 8) };
        let c = value.and_then(char::from_u32).filter(|c| xml_char(*c));
        return match c {
            Some(c) => Ok(Cow::Owned(c.to_string())),
            None => Err(malformed(offset, "a character reference to a character XML does not allow")),
        };
    }
    match body {
        "amp" => Ok(Cow::Borrowed("&")),
        "lt" => Ok(Cow::Borrowed("<")),
        "gt" => Ok(Cow::Borrowed(">")),
        "quot" => Ok(Cow::Borrowed("\"")),
        "apos" => Ok(Cow::Borrowed("'")),
        other => Err(XmlError::EntityRefused { offset, what: format!("a reference to the entity &{};", other.chars().take(32).collect::<String>()) }),
    }
}

/// An attribute value as XML 1.0 section 3.3.3 reads it: references resolved, and each literal line break or tab (a `CR LF` is one
/// break) a space. `raw` is as written, between the quotes.
fn attribute_value(raw: &str, offset: usize) -> Result<String, XmlError> {
    check_chars(raw, offset)?;
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut run = 0; // the start of the text not yet copied
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if !matches!(b, b'&' | b'<' | b'\r' | b'\n' | b'\t') {
            i += 1;
            continue;
        }
        out.push_str(raw.get(run..i).ok_or_else(|| malformed(offset, "a split character"))?);
        match b {
            b'<' => return Err(malformed(offset, "`<` in an attribute value")),
            b'&' => {
                let rest = bytes.get(i + 1..).unwrap_or_default();
                let end = rest
                    .iter()
                    .take(MAX_REFERENCE_BYTES + 1)
                    .position(|c| *c == b';')
                    .ok_or_else(|| malformed(offset, "`&` that does not start a reference"))?;
                let body = raw.get(i + 1..i + 1 + end).ok_or_else(|| malformed(offset, "a split character"))?;
                out.push_str(&resolve_reference(body, offset)?);
                i += end + 2;
            }
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                out.push(' ');
                i += 2;
            }
            _ => {
                out.push(' ');
                i += 1;
            }
        }
        run = i;
    }
    out.push_str(raw.get(run..).ok_or_else(|| malformed(offset, "a split character"))?);
    Ok(out)
}

/// Pushes onto a list that grows by an eighth and not by doubling: a list that doubles holds up to twice what it needs while it is made (a part of four million
/// siblings made a list of eight million places, 640 MiB, to hold 336 MiB), and the peak of a parse is what a process must have. The room it makes is at least four
/// places, so a small list is as it was, and the growth is `reserve_exact`, which the allocator makes by moving pages where the list is large and by copying
/// where it is small.
fn push_tight<T>(list: &mut Vec<T>, item: T) {
    if list.len() == list.capacity() {
        list.reserve_exact((list.len() / 8).max(4));
    }
    list.push(item);
}

struct Parser<'t> {
    limits: XmlLimits,
    options: ParseOptions,
    /// An accepted DOCTYPE was read: a second one is refused.
    doctype_seen: bool,
    max_depth: usize,
    /// The items of the tree being made (of the child being made, in a stream), against `limits.max_nodes`.
    nodes: usize,
    /// The items of the whole document so far, against `max_total`: a tree's own count, and the sum of the children of a stream.
    total: usize,
    max_total: usize,
    stack: Vec<El>,
    root: Option<El>,
    /// The events read: added to the caller's [`Work`] when the parse is done, whether it read or was refused.
    events: u64,
    text: &'t str,
    encoding: Encoding,
    reader: quick_xml::Reader<&'t [u8]>,
    /// True once anything but blanks has been seen before the root: an XML declaration may only come first.
    prolog_content: bool,
    /// A stream: the children of the root are given one at a time ([`Pull`]) and are not kept in it.
    stream: bool,
    /// A child of the root that is complete, and has not been given yet.
    ready: Option<El>,
    /// The items of the child of a stream that was last given.
    last_child_nodes: usize,
    /// The last node under the root of a stream was a run of text: the text that follows it is the same run, and is not another item.
    root_text: bool,
}

impl<'t> Parser<'t> {
    fn new(text: &'t str, encoding: Encoding, limits: &XmlLimits, options: ParseOptions) -> Parser<'t> {
        let mut reader = quick_xml::Reader::from_str(text);
        let config = reader.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = false;
        config.check_end_names = true;
        config.allow_unmatched_ends = false;
        config.allow_dangling_amp = false;
        // XML 1.0 section 2.5: `--` may not appear inside a comment, and a comment may not end in `-`.
        config.check_comments = true;
        Parser {
            limits: limits.clone(),
            options,
            doctype_seen: false,
            max_depth: limits.max_depth.min(XML_DEPTH_CEILING),
            nodes: 0,
            total: 0,
            max_total: usize::MAX,
            stack: Vec::new(),
            root: None,
            events: 0,
            text,
            encoding,
            reader,
            prolog_content: false,
            stream: false,
            ready: None,
            last_child_nodes: 0,
            root_text: false,
        }
    }

    /// Counts `n` more items against the limit of the tree and the limit of the document.
    fn count(&mut self, n: usize) -> Result<(), XmlError> {
        self.nodes = self.nodes.saturating_add(n);
        if self.nodes > self.limits.max_nodes {
            return Err(XmlError::TooManyNodes { limit: self.limits.max_nodes });
        }
        self.total = self.total.saturating_add(n);
        if self.total > self.max_total { Err(XmlError::TooManyNodes { limit: self.max_total }) } else { Ok(()) }
    }

    /// Counts `n` items of the document that a stream reads and does not keep (a text, a comment or an instruction of the root): they count toward the document's
    /// limit and not toward the child that follows.
    fn count_dropped(&mut self, n: usize) -> Result<(), XmlError> {
        self.total = self.total.saturating_add(n);
        if self.total > self.max_total { Err(XmlError::TooManyNodes { limit: self.max_total }) } else { Ok(()) }
    }

    /// Whether a declaration of `declared` is `US-ASCII` that the caller asked to have read ([`ParseOptions::allow_us_ascii`]) and that the text is: UTF-8 with no byte at or
    /// above 0x80. The scan is one pass over the text, and is made only for a declaration of that name.
    fn read_as_us_ascii(&self, declared: &str) -> bool {
        self.options.allow_us_ascii && self.encoding == Encoding::Utf8 && declared.trim().eq_ignore_ascii_case("us-ascii") && self.text.is_ascii()
    }

    /// Whether the node being read is directly under the root of a stream, where nothing is kept.
    fn under_stream_root(&self) -> bool {
        self.stream && self.stack.len() == 1
    }

    /// Reads the whole document into a tree.
    fn run(&mut self) -> Result<El, XmlError> {
        while !self.step()? {}
        self.close()
    }

    /// The end of the document: nothing is open, and there was a root.
    fn close(&mut self) -> Result<El, XmlError> {
        if let Some(open) = self.stack.last() {
            return Err(malformed(self.text.len(), format!("the element <{}> is never closed", open.name)));
        }
        self.root.take().ok_or_else(|| malformed(self.text.len(), "no root element"))
    }

    /// Reads one event. Gives `true` at the end of the document.
    fn step(&mut self) -> Result<bool, XmlError> {
        let text = self.text;
        let offset = usize::try_from(self.reader.buffer_position()).unwrap_or(usize::MAX);
        self.events = self.events.saturating_add(1);
        let event = match self.reader.read_event() {
            Ok(event) => event,
            Err(e) => return Err(self.read_error(text, offset, &e)),
        };
        let in_prolog = self.stack.is_empty() && self.root.is_none();
        match event {
            Event::Decl(decl) => {
                if !in_prolog || self.prolog_content {
                    return Err(malformed(offset, "an XML declaration that is not at the start"));
                }
                self.prolog_content = true;
                match decl.version() {
                    Ok(v) if v == "1.0" => {}
                    _ => return Err(malformed(offset, "an XML version other than 1.0")),
                }
                if let Some(declared) = decl.encoding() {
                    let declared = declared.map_err(|e| malformed(offset, e.to_string()))?.into_owned();
                    if !encoding_matches(&declared, self.encoding) && !self.read_as_us_ascii(&declared) {
                        return Err(malformed(
                            offset,
                            format!("the declared encoding {declared:?} is not the encoding of the bytes, or is not UTF-8 or UTF-16"),
                        ));
                    }
                }
            }
            Event::DocType(doctype) => {
                // Only the external-id form, when the caller allows it, before the root and once; it is read and ignored. The pull
                // parser takes the keyword in any case and with no space after it, which XML 1.0 (production 28) does not: a caller
                // that asks for a DOCTYPE is given only one written `<!DOCTYPE` and a space, and any other is not well-formed.
                let spelled =
                    text.get(offset..).and_then(|at| at.strip_prefix("<!DOCTYPE")).is_some_and(|after| after.starts_with([' ', '\t', '\r', '\n']));
                if self.options.allow_external_doctype && !spelled {
                    return Err(malformed(offset, "a DOCTYPE not written `<!DOCTYPE` and a space"));
                }
                let content = doctype.into_inner();
                let accepted = self.options.allow_external_doctype && in_prolog && !self.doctype_seen && external_id_only(&content);
                if !accepted {
                    return Err(XmlError::EntityRefused { offset, what: "a DTD".to_owned() });
                }
                check_chars(&content, offset)?;
                self.doctype_seen = true;
                self.prolog_content = true;
            }
            Event::Start(tag) => {
                self.open(&tag, offset)?;
            }
            Event::Empty(tag) => {
                let el = self.element(&tag, offset)?;
                self.attach(el);
            }
            Event::End(_) => match self.stack.pop() {
                Some(el) => self.attach(el),
                None => return Err(malformed(offset, "an end tag with nothing open")),
            },
            Event::Text(t) => {
                // The text as written: no line-end normalisation here, `line_ends` below is the one place that does it.
                let s = t.into_inner();
                check_chars(&s, offset)?;
                // XML 1.0 section 2.4: `]]>` may not appear in character data (`]]&gt;` may).
                if s.contains("]]>") {
                    return Err(malformed(offset, "`]]>` in character data"));
                }
                self.text(line_ends(&s), offset, false)?;
            }
            Event::GeneralRef(r) => {
                let body = r.into_inner();
                let s = resolve_reference(&body, offset)?;
                // A reference is not a literal line break: no `line_ends`, a `&#13;` stays a CR.
                self.text(s, offset, true)?;
            }
            Event::CData(c) => {
                let s = c.into_inner();
                check_chars(&s, offset)?;
                self.leaf(Node::CData(line_ends(&s).into_owned()), offset)?;
            }
            Event::Comment(c) => {
                self.prolog_content |= in_prolog;
                let s = c.into_inner();
                check_chars(&s, offset)?;
                if !self.stack.is_empty() {
                    self.leaf(Node::Comment(line_ends(&s).into_owned()), offset)?;
                }
            }
            Event::PI(pi) => {
                self.prolog_content |= in_prolog;
                let target = pi.target().to_owned();
                if target.eq_ignore_ascii_case("xml") {
                    return Err(malformed(offset, "`xml` is not a processing instruction target"));
                }
                let data = pi.content();
                check_chars(data, offset)?;
                if !self.stack.is_empty() {
                    self.leaf(Node::Pi { target, data: line_ends(data.trim_start()).into_owned() }, offset)?;
                }
            }
            Event::Eof => return Ok(true),
        }
        Ok(false)
    }

    // ------------------------------------------------------------------------------------------------------------ a stream

    /// The root's start tag of a stream, read: its name and attributes, with no children. A root that is empty (`<a/>`) is the whole document up to its end.
    fn root_head(&mut self) -> Result<El, XmlError> {
        loop {
            if let Some(head) = self.stack.first().or(self.root.as_ref()) {
                let head = El { name: head.name.clone(), attrs: head.attrs.clone(), children: Vec::new() };
                // What the root itself cost is the document's, and the first child starts its own count.
                self.nodes = 0;
                return Ok(head);
            }
            if self.step()? {
                return Err(malformed(self.text.len(), "no root element"));
            }
        }
    }

    /// The next child element of the root of a stream, whole, or `None` when the root ends. A text, a comment or an instruction directly under the root is read and
    /// let go. The child is counted against the limit of one tree; every item against the limit of the document.
    fn next_child(&mut self) -> Result<Option<El>, XmlError> {
        loop {
            if let Some(child) = self.ready.take() {
                self.last_child_nodes = self.nodes;
                self.nodes = 0;
                return Ok(Some(child));
            }
            if self.stack.is_empty() && self.root.is_some() {
                return Ok(None);
            }
            if self.step()? {
                return match self.stack.last() {
                    Some(open) => Err(malformed(self.text.len(), format!("the element <{}> is never closed", open.name))),
                    None => Ok(None),
                };
            }
        }
    }

    /// Reads to the end of the document, and refuses what follows the root as a tree would: a second root, text, an end tag. The children not yet given are read and
    /// let go.
    fn finish(&mut self) -> Result<(), XmlError> {
        while self.next_child()?.is_some() {}
        while !self.step()? {}
        self.close().map(drop)
    }

    // ------------------------------------------------------------------------------------------------------------ what a parse of a tree and a stream share

    /// What a failed read was. A DTD that is itself broken is still a DTD: the refusal is named, not the breakage.
    fn read_error(&self, text: &str, offset: usize, error: &quick_xml::errors::Error) -> XmlError {
        let at = text.get(offset..).unwrap_or("").trim_start();
        if self.root.is_none() && self.stack.is_empty() && (at.starts_with("<!DOCTYPE") || at.starts_with("<!ENTITY")) {
            return XmlError::EntityRefused { offset, what: "a DTD".to_owned() };
        }
        malformed(offset, error.to_string())
    }

    /// Text, or a reference's text, in the document. Blanks are all that may stand outside the root.
    ///
    /// The text arrives as it will be kept when it is the first of its run (`Cow::Owned` is moved into the tree, and only a borrowed one is copied, once).
    fn text(&mut self, s: Cow<'_, str>, offset: usize, from_reference: bool) -> Result<(), XmlError> {
        if self.stack.is_empty() {
            return if !from_reference && is_blank(&s) { Ok(()) } else { Err(malformed(offset, "text outside the root element")) };
        }
        if s.is_empty() {
            return Ok(());
        }
        if self.under_stream_root() {
            if !self.root_text {
                self.count_dropped(1)?;
            }
            self.root_text = true;
            return Ok(());
        }
        let extend = matches!(self.stack.last().and_then(|el| el.children.last()), Some(Node::Text(_)));
        if !extend {
            self.count(1)?;
        }
        if let Some(parent) = self.stack.last_mut() {
            match parent.children.last_mut() {
                Some(Node::Text(t)) => t.push_str(&s),
                _ => push_tight(&mut parent.children, Node::Text(s.into_owned())),
            }
        }
        Ok(())
    }

    /// A child that is not an element or text: CDATA, a comment, an instruction. Only inside the root.
    fn leaf(&mut self, node: Node, offset: usize) -> Result<(), XmlError> {
        if self.stack.is_empty() {
            return if matches!(node, Node::CData(_)) { Err(malformed(offset, "CDATA outside the root element")) } else { Ok(()) };
        }
        if self.under_stream_root() {
            self.root_text = false;
            return self.count_dropped(1);
        }
        self.count(1)?;
        if let Some(parent) = self.stack.last_mut() {
            push_tight(&mut parent.children, node);
        }
        Ok(())
    }

    fn open(&mut self, tag: &BytesStart<'_>, offset: usize) -> Result<(), XmlError> {
        let el = self.element(tag, offset)?;
        self.stack.push(el);
        Ok(())
    }

    /// A finished element goes to its parent, or is the root. In a stream a child of the root is held to be given, and is not put in the root.
    fn attach(&mut self, mut el: El) {
        // The children grow from room for four by an eighth ([`push_tight`]), so an element with one child holds room for three more of 80 bytes each: what it
        // holds beyond what it needs is given back when the element is done (when it is two or more entries, which is nearly all of the waste).
        if el.children.capacity() >= el.children.len().saturating_add(2) {
            el.children.shrink_to_fit();
        }
        if self.under_stream_root() {
            self.root_text = false;
            self.ready = Some(el);
            return;
        }
        match self.stack.last_mut() {
            Some(parent) => push_tight(&mut parent.children, Node::El(el)),
            None => self.root = Some(el),
        }
    }

    /// An element from its tag: refused if it is too deep, a second root, or too much; its attributes read.
    fn element(&mut self, tag: &BytesStart<'_>, offset: usize) -> Result<El, XmlError> {
        if self.stack.is_empty() && self.root.is_some() {
            return Err(malformed(offset, "more than one root element"));
        }
        if self.stack.len() >= self.max_depth {
            return Err(XmlError::TooDeep { limit: self.max_depth });
        }
        self.count(1)?;
        let name = tag.name().into_inner().to_owned();
        if !is_name(&name) {
            return Err(malformed(offset, "an element name that is not an XML name"));
        }
        if !attributes_are_separated(tag.attributes_raw().as_bytes()) {
            return Err(malformed(offset, "attributes that are not separated by white space"));
        }
        // The room the attributes need, counted before they are read (the list is made once, to its size: a list that grows holds the old and the new at the
        // moment it moves, and an element of a million attributes was 195 bytes an item at the peak), and no more than the items the tree may still take.
        let room = self.limits.max_nodes.saturating_sub(self.nodes);
        let mut attrs = Vec::with_capacity(attribute_count(tag.attributes_raw().as_bytes()).min(room));
        for attribute in tag.attributes() {
            let attribute = attribute.map_err(|e| malformed(offset, e.to_string()))?;
            let key = attribute.key.into_inner();
            let raw = attribute.value.as_ref();
            if key.len() > self.limits.max_attr_bytes || raw.len() > self.limits.max_attr_bytes {
                return Err(XmlError::AttributeTooLarge { limit: self.limits.max_attr_bytes });
            }
            self.count(1)?;
            if !is_name(key) {
                return Err(malformed(offset, "an attribute name that is not an XML name"));
            }
            attrs.push((key.to_owned(), attribute_value(raw, offset)?));
        }
        Ok(El { name, attrs, children: Vec::new() })
    }
}

/// What a stream of an XML document may be ([`XmlText::pull`]): the limits of one tree, which apply to the root's start tag and to each child of the root on its own,
/// and the items of the whole document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullLimits {
    /// The limits of a tree. `max_nodes` is the most items **one child of the root** may have (and the root's own start tag, with its attributes, counts toward the
    /// document's); the depth is counted from the root, as a tree's is.
    pub xml: XmlLimits,
    /// The most items the whole document may have: every element, attribute, text run, comment and instruction, in the children and outside them.
    pub max_document_nodes: usize,
}

/// The text of a document, decoded: what [`XmlText::pull`] reads a stream from. UTF-8 is read where it is, and UTF-16 is made into a string of its own, which is held
/// as long as this is ([`ParseSize::copy_bytes`] says how much).
pub struct XmlText<'b> {
    text: Cow<'b, str>,
    encoding: Encoding,
}

impl<'b> XmlText<'b> {
    /// Decodes `bytes` (UTF-8 or UTF-16, with or without a byte order mark), refusing what [`parse`] refuses of the bytes' encoding.
    pub fn new(bytes: &'b [u8]) -> Result<XmlText<'b>, XmlError> {
        let (text, encoding) = decode(bytes)?;
        Ok(XmlText { text, encoding })
    }

    /// The document's text as it was decoded (UTF-8 as it is, UTF-16 as the string made of it): what a caller that scans a part for a name reads, once, without making a tree.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Begins to read the document as a stream: gives the root's start tag (its name and attributes, and no children) and a [`Pull`] that gives each child of the root
    /// whole, one at a time, so that what is held is one child's tree and not the document's. **What a tree would refuse, a stream refuses**: the same
    /// documents are well-formed, with the same refusals, at the child they are found in (a part that is damaged late has given its earlier children), and
    /// [`Pull::finish`] reads what follows the root.
    pub fn pull(&self, limits: &PullLimits, options: ParseOptions) -> Result<(Pull<'_>, El), XmlError> {
        let mut parser = Parser::new(&self.text, self.encoding, &limits.xml, options);
        parser.stream = true;
        parser.max_total = limits.max_document_nodes;
        let head = parser.root_head()?;
        Ok((Pull { parser }, head))
    }
}

/// The children of the root of a document, read one at a time ([`XmlText::pull`]).
pub struct Pull<'t> {
    parser: Parser<'t>,
}

impl Pull<'_> {
    /// The next child element of the root, with everything in it, or `None` when the root ends. A text, a comment or an instruction directly under the root is read
    /// and let go: only elements are given.
    pub fn next_child(&mut self) -> Result<Option<El>, XmlError> {
        self.parser.next_child()
    }

    /// Reads what is left of the document, the children not yet given included, and refuses what a tree would: a root that is never closed, a second root, text after
    /// the root.
    pub fn finish(&mut self) -> Result<(), XmlError> {
        self.parser.finish()
    }

    /// Sets the most items **the next child** may have, in place of the limit the stream was begun with: a reader that holds a tree beside a budget that changes as it reads
    /// (a child whose tree is more than what the memory is sized for takes the room of what the model has not spent) gives each child the room it has then. It applies from
    /// the child that is read next, and a child that is read refuses past it as a tree of that limit would.
    pub fn set_child_items(&mut self, max_nodes: usize) {
        self.parser.limits.max_nodes = max_nodes;
    }

    /// The items of the child [`Pull::next_child`] gave last, as its tree counts them (each element, attribute, text, comment and instruction under and including it): what
    /// the tree that was given holds. Zero before a child is given.
    pub fn last_child_items(&self) -> usize {
        self.parser.last_child_nodes
    }

    /// The events read so far, in the units of [`Work::xml_events`].
    pub fn events(&self) -> u64 {
        self.parser.events
    }

    /// How far into the decoded text the stream has read, in bytes of it (UTF-8, whatever the document was written in): after the root's start tag when the stream is
    /// begun, at the end of the child [`Pull::next_child`] gave last, and at the end of the document once [`Pull::finish`] has read it. A reader that caps the text of each
    /// child of the root measures a child by the difference of the position before it and after.
    pub fn position(&self) -> usize {
        usize::try_from(self.parser.reader.buffer_position()).unwrap_or(usize::MAX)
    }
}

/// How many attributes the raw text of a tag's attributes holds, by a scan of its quotes and not a parse of it: each is a name, an `=`, and a value between quotes of
/// one kind (the other kind may be in the value, and so may an `=`). This is the room the list of attributes is made with, and a count that is wrong is a list that is
/// too small or too large and never a wrong tree (the attributes are then read by the parser, which refuses what is not one); the scan is the speed of a search for a
/// byte, where the count of the parser's own iterator was a second parse of the tag.
fn attribute_count(raw: &[u8]) -> usize {
    let (mut n, mut i) = (0usize, 0usize);
    while let Some(rest) = raw.get(i..) {
        let Some(eq) = rest.iter().position(|&b| b == b'=') else { break };
        i = i.saturating_add(eq).saturating_add(1);
        while raw.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let Some(&quote) = raw.get(i) else { break };
        if quote != b'"' && quote != b'\'' {
            break;
        }
        let Some(inside) = raw.get(i + 1..) else { break };
        let Some(end) = inside.iter().position(|&b| b == quote) else { break };
        i = i.saturating_add(end).saturating_add(2);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_of_a_tag_counts_the_attributes_the_parser_reads_whatever_is_in_their_values() {
        let cases: [(&str, usize); 10] = [
            ("", 0),
            (" a=\"1\"", 1),
            (" a='1' b=\"2\"", 2),
            (" a = \"1\"\n\tb\t=\t'2'", 2),
            (" a=\"it's\" b='say \"x\"'", 2),
            (" a=\"x=y=z\" b=\"=\"", 2),
            (" a=\"\" b=''", 2),
            (" a=\"'\" b=\"1\"", 2),
            (" a=\"1\" b=\"unfinished", 1),
            (" a=1", 0),
        ];
        for (raw, want) in cases {
            assert_eq!(attribute_count(raw.as_bytes()), want, "{raw:?}");
        }
        // What the parser reads is what the scan counts, for a tag of a thousand attributes whose values are every awkward thing at once.
        let attrs: String = (0..1000).map(|i| format!(" a{i}=\"{}\"", ["'", "=", "='=", "", "&amp;'"][i % 5])).collect();
        let tree = parse_with(format!("<r{attrs}/>").as_bytes(), &XmlLimits::default()).unwrap();
        assert_eq!(attribute_count(attrs.as_bytes()), tree.attrs.len());
        assert_eq!((tree.attrs.len(), tree.attrs.capacity()), (1000, 1000), "made once, to the count");
    }

    /// The growth of the address space of a parse of a tag of `n` attributes under a cap of 1,000 items, which refuses it.
    #[test]
    #[ignore = "run by the test below in a process of its own"]
    fn tag_of_many_attributes_child() {
        let n: usize = std::env::var("XML_TAG_ATTRS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let tag: String = (0..n).map(|i| format!(" a{i}=\"1\"")).collect();
        let doc = format!("<r{tag}/>");
        drop(tag);
        let peak = |key: &str| -> u64 {
            let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            text.lines()
                .find_map(|l| l.strip_prefix(key))
                .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
                .unwrap_or(0)
        };
        let before = peak("VmPeak");
        let out = parse_with(doc.as_bytes(), &XmlLimits { max_nodes: 1_000, ..XmlLimits::default() });
        assert!(matches!(out, Err(XmlError::TooManyNodes { limit: 1_000 })), "{out:?}");
        println!("XMLTAG grew {} KiB", peak("VmPeak").saturating_sub(before));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_tag_of_millions_of_attributes_is_refused_without_a_list_made_for_all_of_them() {
        // Three million attributes are 144 MB of list if the room is the count of the attributes and 48 KB if it is no more than the items the tree may still take.
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", "xml::tests::tag_of_many_attributes_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env("XML_TAG_ATTRS", "3000000")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{text}");
        let grew: u64 = text.split("XMLTAG grew ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).expect(&text);
        assert!(grew < 20_000, "the parse of a tag past the cap grew the address space by {grew} KiB");
    }

    #[test]
    fn a_tag_of_stray_quotes_makes_a_list_of_the_attributes_it_has_and_not_of_the_quotes_it_holds() {
        // A value of a million single quotes in a double-quoted attribute: a scan that counted quotes would reserve half a million places for one attribute.
        let value = "'".repeat(500_000);
        let tree = parse_with(format!("<r a=\"{value}\"/>").as_bytes(), &XmlLimits::default()).unwrap();
        assert_eq!((tree.attrs.len(), tree.attrs.capacity()), (1, 1));
    }

    #[test]
    fn a_list_that_grows_holds_at_most_an_eighth_or_four_places_more_than_it_has() {
        let mut list: Vec<u64> = Vec::new();
        let mut moves = 0usize;
        let mut last = 0usize;
        for i in 0..200_000u64 {
            push_tight(&mut list, i);
            assert!(list.capacity() <= list.len() + (list.len() / 8).max(4), "{} entries in room for {}", list.len(), list.capacity());
            if list.capacity() != last {
                moves += 1;
                last = list.capacity();
            }
        }
        assert_eq!(list.len(), 200_000);
        assert!(list.iter().copied().eq(0..200_000), "nothing is lost or moved out of order by the growth");
        // An eighth is not a copy of the list for each push: the room is made a few score of times, not a hundred thousand.
        assert!(moves < 120, "{moves} times");
        // A list of one is room for four, as a vector makes.
        let mut small = Vec::new();
        push_tight(&mut small, 1u8);
        assert_eq!(small.capacity(), 4);
    }

    #[test]
    fn text_references_and_cdata_that_follow_one_another_are_a_run_and_a_node_each_where_the_parser_says() {
        // Text and the references in it are one run; a CDATA section is a node of its own, and what follows it is a new run.
        let tree = parse_with(b"<a>x&amp;y&#65;z<![CDATA[c]]>w<!--k-->v</a>", &XmlLimits::default()).unwrap();
        assert_eq!(
            tree.children,
            vec![
                Node::Text("x&yAz".to_owned()),
                Node::CData("c".to_owned()),
                Node::Text("w".to_owned()),
                Node::Comment("k".to_owned()),
                Node::Text("v".to_owned()),
            ]
        );
        // A reference that stands first is a run of its own, and the same text follows it.
        let tree = parse_with(b"<a>&lt;b&gt;</a>", &XmlLimits::default()).unwrap();
        assert_eq!(tree.children, vec![Node::Text("<b>".to_owned())]);
    }

    #[test]
    fn an_element_holds_no_room_beyond_what_it_needs_by_more_than_one_place_when_it_is_done() {
        let limits = XmlLimits::default();
        let attrs: String = (0..100).map(|i| format!(" a{i}=\"1\"")).collect();
        let body: String = (0..5).map(|_| "<b/>").collect();
        let tree = parse_with(format!("<r{attrs}><one><b/></one><three><b/><b/><b/></three><five>{body}</five></r>").as_bytes(), &limits).unwrap();
        // The attributes are made to their count, once.
        assert_eq!((tree.attrs.len(), tree.attrs.capacity()), (100, 100));
        let kids: Vec<&El> = tree.elements().collect();
        for (child, entries) in kids.iter().zip([1usize, 3, 5]) {
            assert_eq!(child.children.len(), entries);
            assert!(child.children.capacity() < entries + 2, "{} children in room for {}", entries, child.children.capacity());
        }
        // An element of attributes that a tag does not hold twice is as it was: one place for one attribute.
        let one = parse_with(b"<a b=\"1\"/>", &limits).unwrap();
        assert_eq!((one.attrs.len(), one.attrs.capacity()), (1, 1));
        // The room of the attributes is no more than the items the tree may still take: a tag of many attributes past the limit is refused before a list is made of them.
        let many: String = (0..1000).map(|i| format!(" a{i}=\"1\"")).collect();
        let refused = parse_with(format!("<r{many}/>").as_bytes(), &XmlLimits { max_nodes: 50, ..XmlLimits::default() });
        assert!(matches!(refused, Err(XmlError::TooManyNodes { limit: 50 })), "{refused:?}");
    }

    #[test]
    fn utf16_is_decoded_into_a_string_that_has_room_for_what_it_holds_and_no_more() {
        let units = |text: &str, little: bool| -> Vec<u8> {
            text.encode_utf16().flat_map(|u| if little { u.to_le_bytes() } else { u.to_be_bytes() }).collect()
        };
        // One byte, two, three and four bytes of UTF-8 (a surrogate pair is four in each), in both orders: the capacity is the length, never more.
        for text in ["", "abc", "caf\u{e9}", "\u{4E00}\u{4E01}x", "\u{1F600}\u{1F601}", "a\u{e9}\u{4E00}\u{1F600}"] {
            for little in [true, false] {
                let decoded = decode_utf16(&units(text, little), little).unwrap();
                assert_eq!((decoded.as_str(), decoded.capacity()), (text, text.len()), "{text:?} little={little}");
            }
        }
        // A large text of three-byte characters is the case the doubling of a growing string was dearest in: 3 bytes for 2, never more than that held.
        let text = "\u{4E00}".repeat(100_000);
        let decoded = decode_utf16(&units(&text, true), true).unwrap();
        assert_eq!(decoded.capacity(), 300_000);
        // The refusals are the same: half a unit, and a surrogate with no partner (first, last and alone).
        assert!(decode_utf16(&[0x61, 0x00, 0x62], true).is_err());
        assert!(decode_utf16(&[0x00, 0xD8], true).is_err());
        assert!(decode_utf16(&[0x61, 0x00, 0x00, 0xDC], true).is_err());
        assert!(decode_utf16(&[0x00, 0xD8, 0x61, 0x00], true).is_err());
    }

    #[test]
    fn line_ends_makes_one_string_of_the_size_it_ends_with_and_none_when_there_is_nothing_to_change() {
        assert!(matches!(line_ends(""), Cow::Borrowed("")));
        assert!(matches!(line_ends("a\nb"), Cow::Borrowed("a\nb")));
        for (text, expected) in [
            ("\r", "\n"),
            ("\r\n", "\n"),
            ("\n\r", "\n\n"),
            ("\r\r", "\n\n"),
            ("\r\r\n", "\n\n"),
            ("\r\n\r\n", "\n\n"),
            ("a\r\nb\rc\nd\r", "a\nb\nc\nd\n"),
            ("\u{4E00}\r\n\u{4E01}\r\u{4E02}", "\u{4E00}\n\u{4E01}\n\u{4E02}"),
        ] {
            match line_ends(text) {
                Cow::Owned(out) => assert_eq!((out.as_str(), out.capacity()), (expected, expected.len()), "{text:?}"),
                Cow::Borrowed(_) => panic!("{text:?} has a CR and is copied"),
            }
        }
    }
}
