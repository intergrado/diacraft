//! The file and its pages: an `mxfile`'s attributes, a `diagram`'s model and its settings, and the pages handed on one at a
//! time with their cells ([`PageSource`]), which the model is built from.
//!
//! [`read_sources`] is the front of the reader: it holds the file to the caps (its size), says what it is (the sniff), refuses what is not a drawing, and walks the
//! pages (`decode::walk`: one `diagram` at a time, each plain or compressed), reading each page's model into its settings, its `defs` and its cells
//! ([`cells`](crate::cells)). **What is held is one page's tree and its cells**: the tree is consumed as the cells are read, so that its strings are moved and not copied.
//!
//! The JSON of two attributes (`vars` on the file, `backgroundImage` on a model) is read by a small reader of its own (`json`): one pass, an explicit stack for nesting, at most the
//! limits' `max_json_depth` levels, and the bytes charged to the read's work before the pass. Text that is not JSON of the shape wanted is no value and one warning
//! (`drawio_attribute_unreadable`); nothing in it is ever fetched.

use diacraft_container::xml::{El, Node};

use crate::cells::{self, Graph, PageReading, warn};
use crate::decode::{self, Page, warn as file_warn};
use crate::error::{self, DrawioError, ErrorKind, class, size_words};
use crate::geom::parse_px;
use crate::limits::{MAX_PAGE_SCALE, MIN_PAGE_PX, MIN_PAGE_SCALE};
use crate::report::{NAME_CHARS, Reading, Unit};
use crate::sniff::{self, Kind};

/// What the file says beside its pages: the properties its `vars` hold. No other attribute of the `mxfile` element changes the drawing, and none is kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mxfile {
    /// The file's `vars` (a JSON object: name and text), in the order written, at most `Limits::max_vars`. Placeholders look a name up here last; makes them the
    /// document's custom properties. A name written twice has its last value, at its first place; a value that is a number or a flag is its text as written.
    pub vars: Vec<(String, String)>,
}

/// The settings of a page's `mxGraphModel` that the mapping uses. The others (`dx`, `dy`, `grid`, `gridSize`, `guides`, `tooltips`, `connect`, `arrows`, `fold`, `page`,
/// `adaptiveColors`, `measureScale`, `extFonts` and the rest) are editor settings and are not kept.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSettings {
    /// `pageWidth` in px: 850 when missing or no number, held to 1 and the limits' `max_page_px` (1,000,000).
    pub page_width: f64,
    /// `pageHeight` in px: 1,100 by the same rule.
    pub page_height: f64,
    /// `pageScale`: 1 when missing or no number, held to `MIN_PAGE_SCALE` and `MAX_PAGE_SCALE`.
    pub page_scale: f64,
    /// `background`, as written (`#rrggbb`, `none`, or another colour form), when the model has one that is not blank.
    pub background: Option<String>,
    /// The `src` of `backgroundImage` (a JSON object): a page link or a picture address, when the model has one that is text.
    pub background_image_src: Option<String>,
    /// `math="1"`.
    pub math: bool,
    /// `shadow="1"`.
    pub shadow: bool,
}

impl Default for PageSettings {
    fn default() -> Self {
        PageSettings {
            page_width: 850.0,
            page_height: 1100.0,
            page_scale: 1.0,
            background: None,
            background_image_src: None,
            math: false,
            shadow: false,
        }
    }
}

/// A page, ready for the model: what the walk found (its place, id and name) and what its model holds.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSource {
    /// The page's place among the pages, from 1.
    pub position: usize,
    /// The page's id: the file's, or its place counting from 0.
    pub id: String,
    /// The page's `name` as the file has it; `None` when it has none.
    pub name: Option<String>,
    /// The settings of its model: the defaults for an empty page.
    pub settings: PageSettings,
    /// The `data` of each `def` of the model's `defs`, in order: `image=def(n)` and `shape=def(n)` name the `n`-th. Held as the tree held them, never expanded here.
    pub defs: Vec<String>,
    /// The cells; empty for an empty page.
    pub graph: Graph,
}

/// Reads the file in `bytes` page by page, within the read's limits, and hands each page to `each` as it is read, so that what is held is one page. This is the front of
/// [`read`](crate::read): the size of the file, the sniff (a library, an image and anything else are refused or declined in words), the encoding, and the
/// walk. It gives the file's own properties, after its pages. **A page with no model is a [`PageSource`] with the defaults and no cell.**
///
/// # Errors
/// A refusal, with its class, as [`read`](crate::read) gives it; and any error `each` gives.
pub fn read_sources(
    bytes: &[u8],
    reading: &mut Reading<'_>,
    mut each: impl FnMut(&mut Reading<'_>, PageSource) -> Result<(), DrawioError>,
) -> Result<Mxfile, DrawioError> {
    let limits = reading.limits;
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > limits.max_file_bytes {
        return Err(DrawioError::too_large(format!(
            "The file is {len} bytes, more than the {} that DiaCraft reads.",
            size_words(limits.max_file_bytes)
        )));
    }
    let (kind, looked) = sniff::sniff_counted(bytes);
    reading.charge(Unit::SniffBytes, looked)?;
    let kind = match kind {
        Some(kind @ (Kind::Document | Kind::Page)) => kind,
        Some(Kind::Library) => {
            return Err(DrawioError::new(
                ErrorKind::Refused,
                class::LIBRARY_NOT_DRAWING,
                "This file is a library of shapes, not a drawing, and DiaCraft does not open shape libraries yet.",
            ));
        }
        // An image that may hold a drawing: reads it. Until it does, it is not a drawing of this reader's.
        Some(Kind::Svg | Kind::Png) | None => return Err(DrawioError::not_a_drawing()),
    };
    if let Some(declared) = sniff::declared_encoding(bytes).filter(|declared| !sniff::encoding_supported(declared)) {
        return Err(DrawioError::new(
            ErrorKind::Refused,
            class::ENCODING_UNSUPPORTED,
            format!(
                "The file declares the text encoding \"{}\", which DiaCraft does not read (it reads UTF-8, UTF-16 and US-ASCII).",
                error::shown(&declared, 32)
            ),
        ));
    }
    let walked = decode::walk(bytes, kind, reading, |reading, page| {
        let source = page_source(page, reading)?;
        each(reading, source)
    })?;
    mxfile_of(walked.head.as_ref(), reading)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the file

/// The file's own properties from the `mxfile` element's start tag: its `vars`, read as the reader of JSON meets them: **at most `max_vars` names are kept as they come**, and
/// the members past them are checked and skipped, never held.
fn mxfile_of(head: Option<&El>, reading: &mut Reading<'_>) -> Result<Mxfile, DrawioError> {
    let Some(text) = head.and_then(|h| h.attr("vars")) else { return Ok(Mxfile::default()) };
    reading.charge(Unit::JsonBytes, text.len() as u64)?;
    let limits = reading.limits;
    match json::parse(text, limits.max_json_depth, json::Want::Vars { max: limits.max_vars }) {
        Some(json::Parsed::Vars { vars, dropped }) => {
            if dropped {
                reading.note(warn::TOO_MANY_PROPERTIES, "mxfile: vars");
            }
            Ok(Mxfile { vars })
        }
        _ => {
            reading.note(warn::ATTRIBUTE_UNREADABLE, "mxfile: vars");
            Ok(Mxfile::default())
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ a page

/// One page of the walk as a [`PageSource`]: its settings, its `defs` and its cells.
fn page_source(page: Page, reading: &mut Reading<'_>) -> Result<PageSource, DrawioError> {
    let Page { position, id, name, model } = page;
    let label = decode::page_label(name.as_deref(), position);
    let mut pr = PageReading::new(reading, label);
    let mut source = PageSource { position, id, name, settings: PageSettings::default(), defs: Vec::new(), graph: Graph::default() };
    if let Some(model) = model {
        read_model(model, &mut source, &mut pr)?;
    }
    pr.finish();
    Ok(source)
}

/// How a page is named in the words of a refusal.
fn the_page(label: &str) -> String {
    format!("The page \"{}\"", error::shown(label, NAME_CHARS))
}

/// Counts the elements of a model before any is read, by an explicit stack, and refuses a model that has more than the limits' `max_page_elements`. The nodes of each element
/// are charged before its children are visited.
fn count_elements(model: &El, pr: &mut PageReading<'_, '_>) -> Result<(), DrawioError> {
    let max = pr.reading.limits.max_page_elements;
    let mut count = 0usize;
    let mut stack: Vec<&El> = vec![model];
    while let Some(el) = stack.pop() {
        count += 1;
        pr.reading.charge(Unit::TreeNodes, (el.attrs.len() + el.children.len()) as u64)?;
        let kids = || el.children.iter().filter_map(|n| if let Node::El(child) = n { Some(child) } else { None });
        // Every element on the stack is counted when it is taken, so the page is past the cap as soon as these and the ones before them are: the stack is never longer than the cap.
        if count.saturating_add(stack.len()).saturating_add(kids().count()) > max {
            return Err(DrawioError::too_complex(format!("{} has more than {max} elements, more than DiaCraft reads.", the_page(&pr.label))));
        }
        stack.extend(kids());
    }
    Ok(())
}

/// A model: its settings, its `defs` and the cells of its `root`. Elements of the model that are none of these are counted and skipped.
fn read_model(model: El, source: &mut PageSource, pr: &mut PageReading<'_, '_>) -> Result<(), DrawioError> {
    count_elements(&model, pr)?;
    let El { attrs, children, .. } = model;
    source.settings = settings_of(attrs, pr)?;
    pr.reading.charge(Unit::TreeNodes, children.len() as u64)?;
    let (mut root, mut defs_seen) = (None, false);
    for node in children {
        let Node::El(child) = node else { continue };
        match child.name.as_str() {
            "root" if root.is_none() => root = Some(child),
            "defs" if !defs_seen => {
                defs_seen = true;
                source.defs = read_defs(child, pr)?;
            }
            _ => pr.unknown_element(&child.name),
        }
    }
    match root {
        Some(root) => source.graph = cells::build(root, pr)?,
        // A page whose model has no `root` is empty, and unreadable.
        None => pr.reading.note(file_warn::PAGE_UNREADABLE, &pr.label),
    }
    Ok(())
}

/// The settings of a model from its attributes.
fn settings_of(attrs: Vec<(String, String)>, pr: &mut PageReading<'_, '_>) -> Result<PageSettings, DrawioError> {
    let mut s = PageSettings::default();
    for (k, v) in attrs {
        match k.as_str() {
            "pageWidth" => s.page_width = page_size(&v, s.page_width, pr),
            "pageHeight" => s.page_height = page_size(&v, s.page_height, pr),
            "pageScale" => s.page_scale = page_scale(&v, pr),
            "background" => s.background = Some(v).filter(|v| !v.trim().is_empty()),
            "backgroundImage" => s.background_image_src = background_src(&v, pr)?,
            "math" => s.math = v == "1",
            "shadow" => s.shadow = v == "1",
            _ => {}
        }
    }
    Ok(s)
}

/// A page size in px: a decimal number held to 1 and the limits' `max_page_px`; anything else is `default`; each repair is counted.
fn page_size(text: &str, default: f64, pr: &mut PageReading<'_, '_>) -> f64 {
    let limits = pr.reading.limits;
    let Some(n) = parse_px(text, limits.max_number_bytes) else {
        pr.bad_number();
        return default;
    };
    let held = n.clamp(MIN_PAGE_PX, limits.max_page_px.max(MIN_PAGE_PX));
    if held != n {
        pr.bad_number();
    }
    held
}

/// A page scale: a decimal number held to `MIN_PAGE_SCALE` and `MAX_PAGE_SCALE`; anything else is 1; each repair is counted.
fn page_scale(text: &str, pr: &mut PageReading<'_, '_>) -> f64 {
    let Some(n) = parse_px(text, pr.reading.limits.max_number_bytes) else {
        pr.bad_number();
        return 1.0;
    };
    let held = n.clamp(MIN_PAGE_SCALE, MAX_PAGE_SCALE);
    if held != n {
        pr.bad_number();
    }
    held
}

/// The `src` of a model's `backgroundImage`: a JSON object's member `src`, the last one, when it is text. JSON that is not an object has none, and says nothing; text that is no
/// JSON is counted. **Nothing but the `src` is kept**: the other members are checked and skipped as they are read.
fn background_src(text: &str, pr: &mut PageReading<'_, '_>) -> Result<Option<String>, DrawioError> {
    pr.reading.charge(Unit::JsonBytes, text.len() as u64)?;
    Ok(match json::parse(text, pr.reading.limits.max_json_depth, json::Want::Text { key: "src" }) {
        Some(json::Parsed::Text(src)) => src,
        Some(_) => None,
        None => {
            pr.note(warn::ATTRIBUTE_UNREADABLE, "backgroundImage");
            None
        }
    })
}

/// The `data` of each `def` of a model's `defs`, by position; a `def` with no `data` is empty, and an element that is no `def` is counted and skipped.
fn read_defs(defs: El, pr: &mut PageReading<'_, '_>) -> Result<Vec<String>, DrawioError> {
    pr.reading.charge(Unit::TreeNodes, defs.children.len() as u64)?;
    let mut out = Vec::new();
    for node in defs.children {
        let Node::El(child) = node else { continue };
        if child.name == "def" {
            pr.reading.charge(Unit::TreeNodes, child.attrs.len() as u64)?;
            out.push(child.attrs.into_iter().find(|(k, _)| k == "data").map(|(_, v)| v).unwrap_or_default());
        } else {
            pr.unknown_element(&child.name);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ JSON

/// A reader of JSON for the two attributes that hold it. It follows RFC 8259's grammar and nothing else (a lone surrogate, a control character in a string, a leading zero, a
/// trailing comma and text after the value are all refused), and it keeps **only what the caller wants of a top-level object, as it reads it** ([`Want`]): the caller's own cap is
/// applied while the members go by, so that a member past it is validated and skipped and never held, and a small attribute cannot make a large allocation before any cap acts. What
/// is nested is validated and skipped: each loop advances at least a byte, nesting is an explicit stack held to the depth the caller allows, and a name is read into one buffer
/// that is reused. The work is the bytes of the text, which the caller charges before the pass.
mod json {
    use std::collections::HashMap;

    use crate::decode::hex_digit;

    /// A value of a member as the reader makes it: with its content when it was kept, with none when it was only checked.
    enum Scalar {
        /// A string, unescaped.
        Text(String),
        /// A number, as it was written.
        Number(String),
        /// `true` or `false`.
        Flag(bool),
        /// `null`, an object or an array: valid, and never kept.
        Other,
    }

    /// What the caller wants of a top-level object.
    pub(super) enum Want<'a> {
        /// The members that are text, numbers or flags, by name, at most `max` names: a name written twice has its last value at its first place, and a member that would be a name
        /// past `max` is checked, skipped and counted as dropped.
        Vars {
            /// Most names kept.
            max: usize,
        },
        /// The text of the last member named `key`, and nothing else.
        Text {
            /// The name.
            key: &'a str,
        },
    }

    /// What the text is.
    pub(super) enum Parsed {
        /// An object read for [`Want::Vars`]: the names kept in the order they came, and whether any member was dropped for the cap.
        Vars {
            /// The names and their text.
            vars: Vec<(String, String)>,
            /// A member that would have been kept was past the cap.
            dropped: bool,
        },
        /// An object read for [`Want::Text`]: the text of the member, when the last member of that name is text.
        Text(Option<String>),
        /// JSON that is not an object.
        NotObject,
    }

    /// What is kept of the members of the object, as they go by.
    enum Sink<'a> {
        Vars { max: usize, vars: Vec<(String, String)>, index: HashMap<String, usize>, dropped: bool },
        Text { key: &'a str, found: Option<String> },
    }

    impl Sink<'_> {
        /// Whether the value of the member named `name` is to be kept (made into text) or only checked: a name that is already kept, or that there is room for, or the key sought.
        fn wants(&self, name: &str) -> bool {
            match self {
                Sink::Vars { max, vars, index, .. } => index.contains_key(name) || vars.len() < *max,
                Sink::Text { key, .. } => name == *key,
            }
        }

        /// The member `name` with `value`, which has its content when `kept` and none when it was only checked.
        fn put(&mut self, name: &str, value: Scalar, kept: bool) {
            match self {
                Sink::Vars { vars, index, dropped, .. } => {
                    if !kept {
                        // A member that would have been kept, if there had been room.
                        *dropped |= !matches!(value, Scalar::Other);
                        return;
                    }
                    let text = match value {
                        Scalar::Text(t) | Scalar::Number(t) => t,
                        Scalar::Flag(f) => f.to_string(),
                        Scalar::Other => return,
                    };
                    if let Some(slot) = index.get(name).and_then(|at| vars.get_mut(*at)) {
                        slot.1 = text;
                    } else {
                        index.insert(name.to_owned(), vars.len());
                        vars.push((name.to_owned(), text));
                    }
                }
                Sink::Text { found, .. } => {
                    if kept {
                        *found = if let Scalar::Text(t) = value { Some(t) } else { None };
                    }
                }
            }
        }

        fn finish(self) -> Parsed {
            match self {
                Sink::Vars { vars, dropped, .. } => Parsed::Vars { vars, dropped },
                Sink::Text { found, .. } => Parsed::Text(found),
            }
        }
    }

    struct Reader<'a> {
        text: &'a str,
        bytes: &'a [u8],
        at: usize,
    }

    impl<'a> Reader<'a> {
        fn peek(&self) -> Option<u8> {
            self.bytes.get(self.at).copied()
        }

        fn ws(&mut self) {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.at += 1;
            }
        }

        /// Whether any digit was read.
        fn digits(&mut self) -> bool {
            let from = self.at;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            self.at > from
        }

        fn hex4(&mut self) -> Option<u32> {
            let mut v = 0u32;
            for _ in 0..4 {
                v = v << 4 | u32::from(hex_digit(self.peek()?)?);
                self.at += 1;
            }
            Some(v)
        }

        /// The character an escape after the backslash makes: a surrogate pair is one character, and a lone surrogate is none.
        fn escape(&mut self) -> Option<char> {
            let e = self.peek()?;
            self.at += 1;
            Some(match e {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hi = self.hex4()?;
                    match hi {
                        0xD800..=0xDBFF => {
                            for expected in *b"\\u" {
                                if self.peek()? != expected {
                                    return None;
                                }
                                self.at += 1;
                            }
                            let lo = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&lo) {
                                return None;
                            }
                            char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))?
                        }
                        0xDC00..=0xDFFF => return None,
                        _ => char::from_u32(hi)?,
                    }
                }
                _ => return None,
            })
        }

        /// A string, from its opening quote: its text is appended to `out` when `keep`, else nothing is made.
        fn string_to(&mut self, keep: bool, out: &mut String) -> Option<()> {
            self.at += 1;
            let mut run = self.at;
            loop {
                match self.peek()? {
                    b'"' => {
                        if keep {
                            out.push_str(self.text.get(run..self.at)?);
                        }
                        self.at += 1;
                        return Some(());
                    }
                    b'\\' => {
                        if keep {
                            out.push_str(self.text.get(run..self.at)?);
                        }
                        self.at += 1;
                        let c = self.escape()?;
                        if keep {
                            out.push(c);
                        }
                        run = self.at;
                    }
                    0..=0x1f => return None,
                    _ => self.at += 1,
                }
            }
        }

        /// A string as text when `keep`, else checked and no text made.
        fn string(&mut self, keep: bool) -> Option<String> {
            let mut out = String::new();
            self.string_to(keep, &mut out)?;
            Some(out)
        }

        /// A number by RFC 8259's grammar, as written.
        fn number(&mut self) -> Option<&'a str> {
            let start = self.at;
            if self.peek() == Some(b'-') {
                self.at += 1;
            }
            match self.peek()? {
                b'0' => self.at += 1,
                b'1'..=b'9' => {
                    self.digits();
                }
                _ => return None,
            }
            if self.peek() == Some(b'.') {
                self.at += 1;
                if !self.digits() {
                    return None;
                }
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.at += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.at += 1;
                }
                if !self.digits() {
                    return None;
                }
            }
            self.text.get(start..self.at)
        }

        fn literal(&mut self, word: &str) -> Option<()> {
            let end = self.at.checked_add(word.len())?;
            if self.bytes.get(self.at..end)? != word.as_bytes() {
                return None;
            }
            self.at = end;
            Some(())
        }

        /// A member's name and the colon after it: the name is appended to `out` when `keep`, else only checked.
        fn key_to(&mut self, keep: bool, out: &mut String) -> Option<()> {
            self.ws();
            if self.peek()? != b'"' {
                return None;
            }
            self.string_to(keep, out)?;
            self.ws();
            if self.peek()? != b':' {
                return None;
            }
            self.at += 1;
            Some(())
        }

        /// Skips one value and what is in it, checking its grammar. `base` levels are open above it, and no more than `max` may be open at once.
        fn skip_value(&mut self, base: usize, max: usize) -> Option<()> {
            let mut open: Vec<u8> = Vec::new();
            let mut value = true;
            let mut none = String::new();
            loop {
                if value {
                    self.ws();
                    match self.peek()? {
                        b'{' | b'[' => {
                            let kind = self.peek()?;
                            open.push(kind);
                            if base.saturating_add(open.len()) > max {
                                return None;
                            }
                            self.at += 1;
                            self.ws();
                            let close = if kind == b'{' { b'}' } else { b']' };
                            if self.peek()? == close {
                                open.pop();
                                self.at += 1;
                                value = false;
                            } else if kind == b'{' {
                                self.key_to(false, &mut none)?;
                            }
                        }
                        b'"' => {
                            self.string_to(false, &mut none)?;
                            value = false;
                        }
                        b'-' | b'0'..=b'9' => {
                            self.number()?;
                            value = false;
                        }
                        b't' => {
                            self.literal("true")?;
                            value = false;
                        }
                        b'f' => {
                            self.literal("false")?;
                            value = false;
                        }
                        b'n' => {
                            self.literal("null")?;
                            value = false;
                        }
                        _ => return None,
                    }
                } else {
                    let Some(&top) = open.last() else { return Some(()) };
                    self.ws();
                    match (top, self.peek()?) {
                        (b'{', b',') => {
                            self.at += 1;
                            self.key_to(false, &mut none)?;
                            value = true;
                        }
                        (b'[', b',') => {
                            self.at += 1;
                            value = true;
                        }
                        (b'{', b'}') | (b'[', b']') => {
                            open.pop();
                            self.at += 1;
                        }
                        _ => return None,
                    }
                }
            }
        }

        /// The value of a member of the top-level object: with its content when `keep`, else checked and none made.
        fn member(&mut self, max: usize, keep: bool) -> Option<Scalar> {
            self.ws();
            Some(match self.peek()? {
                b'"' => Scalar::Text(self.string(keep)?),
                b'-' | b'0'..=b'9' => {
                    let written = self.number()?;
                    Scalar::Number(if keep { written.to_owned() } else { String::new() })
                }
                b't' => {
                    self.literal("true")?;
                    Scalar::Flag(true)
                }
                b'f' => {
                    self.literal("false")?;
                    Scalar::Flag(false)
                }
                b'{' | b'[' => {
                    self.skip_value(1, max)?;
                    Scalar::Other
                }
                _ => {
                    self.literal("null")?;
                    Scalar::Other
                }
            })
        }

        /// The members of the object whose `{` is at the reader, each given to `sink` as it is read. One buffer holds each name in turn.
        fn members(&mut self, max: usize, sink: &mut Sink<'_>) -> Option<()> {
            self.at += 1;
            self.ws();
            if self.peek()? == b'}' {
                self.at += 1;
                return Some(());
            }
            let mut name = String::new();
            loop {
                name.clear();
                self.key_to(true, &mut name)?;
                let kept = sink.wants(&name);
                let value = self.member(max, kept)?;
                sink.put(&name, value, kept);
                self.ws();
                match self.peek()? {
                    b',' => self.at += 1,
                    b'}' => {
                        self.at += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
    }

    /// `text` as JSON of at most `max_depth` levels, read for `want`: the top-level object is the first level, and what the caller does not want of it is checked and skipped as it
    /// goes by. `None` for text that is no JSON; [`Parsed::NotObject`] for JSON that is not an object.
    pub(super) fn parse(text: &str, max_depth: usize, want: Want<'_>) -> Option<Parsed> {
        let mut r = Reader { text, bytes: text.as_bytes(), at: 0 };
        r.ws();
        let parsed = if r.peek()? == b'{' {
            if max_depth < 1 {
                return None;
            }
            let mut sink = match want {
                Want::Vars { max } => Sink::Vars { max, vars: Vec::new(), index: HashMap::new(), dropped: false },
                Want::Text { key } => Sink::Text { key, found: None },
            };
            r.members(max_depth, &mut sink)?;
            sink.finish()
        } else {
            r.skip_value(0, max_depth)?;
            Parsed::NotObject
        };
        r.ws();
        (r.at == r.bytes.len()).then_some(parsed)
    }
}
