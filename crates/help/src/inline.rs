//! The inlines of Help Markdown: emphasis, code and links, and the targets of links.
//!
//! One pass over the bytes of a run of text, with a stack of the emphasis and link frames that are open (at most two emphasis frames deep, and one link): linear in the
//! bytes, with no backtracking. A link's target is looked for in a window of the target cap and no further, so a run of `](` cannot make the work grow with the square
//! of the line. Every delimiter that is an ASCII byte, and the scan is by bytes, so a character that takes several bytes is never cut.

use crate::chord::Chord;
use crate::md::{Span, Target};
use crate::{Code, MAX_SPANS, MAX_TARGET_BYTES, url};

/// A problem found at a byte of the source.
#[derive(Debug)]
pub(crate) struct Diag {
    pub offset: usize,
    pub code: Code,
    pub detail: String,
}

/// The kind of an open frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Root,
    Strong,
    Em,
    Link,
}

struct Frame {
    kind: Kind,
    spans: Vec<Span>,
    open_at: usize,
    /// For a link: how many `[` inside its text are open and shown as text (a link inside a link is a problem, and what is in it stays plain).
    inner: usize,
}

struct Reader<'a> {
    src: &'a str,
    bytes: &'a [u8],
    stack: Vec<Frame>,
    diags: Vec<Diag>,
    /// How many spans exist (the ones inside frames too).
    count: usize,
    stopped: bool,
}

/// Reads `source` as inlines: the spans, and each rule it breaks (in the order of their bytes). What breaks a rule stays in the text as it was written, except a link whose
/// target is refused, which is shown as its text.
pub(crate) fn parse(source: &str) -> (Vec<Span>, Vec<Diag>) {
    let mut reader = Reader {
        src: source,
        bytes: source.as_bytes(),
        stack: vec![Frame { kind: Kind::Root, spans: Vec::new(), open_at: 0, inner: 0 }],
        diags: Vec::new(),
        count: 0,
        stopped: false,
    };
    reader.run();
    reader.finish();
    let spans = reader.stack.pop().map(|f| f.spans).unwrap_or_default();
    let mut diags = reader.diags;
    diags.sort_by_key(|d| d.offset);
    (spans, diags)
}

impl Reader<'_> {
    fn diag(&mut self, offset: usize, code: Code, detail: &str) {
        self.diags.push(Diag { offset, code, detail: detail.chars().take(80).collect() });
    }

    /// Makes room for one more span, or stops the read at the cap.
    fn reserve(&mut self) -> bool {
        if self.count >= MAX_SPANS {
            if !self.stopped {
                self.stopped = true;
                self.diag(self.bytes.len(), Code::TooManySpans, "");
            }
            return false;
        }
        self.count += 1;
        true
    }

    /// Plain text into the open frame, joined to the text before it.
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let joins = matches!(self.stack.last().and_then(|f| f.spans.last()), Some(Span::Text(_)));
        if !joins && !self.reserve() {
            return;
        }
        if let Some(frame) = self.stack.last_mut() {
            if let Some(Span::Text(last)) = frame.spans.last_mut() {
                last.push_str(s);
            } else {
                frame.spans.push(Span::Text(s.to_owned()));
            }
        }
    }

    /// A span that already exists (and is counted), into the open frame.
    fn put(&mut self, span: Span) {
        if let Span::Text(t) = &span
            && let Some(Span::Text(last)) = self.stack.last_mut().and_then(|f| f.spans.last_mut())
        {
            last.push_str(t);
            // The two texts are one span now.
            self.count = self.count.saturating_sub(1);
            return;
        }
        if let Some(frame) = self.stack.last_mut() {
            frame.spans.push(span);
        }
    }

    fn flush(&mut self, from: usize, to: usize) {
        if let Some(s) = self.src.get(from..to) {
            self.text(s);
        }
    }

    fn run(&mut self) {
        let n = self.bytes.len();
        let (mut i, mut lit) = (0usize, 0usize);
        while i < n && !self.stopped {
            let Some(&b) = self.bytes.get(i) else { break };
            match b {
                b'\\' => {
                    self.flush(lit, i);
                    i = self.escape(i);
                    lit = i;
                }
                b'*' => {
                    self.flush(lit, i);
                    i = self.stars(i);
                    lit = i;
                }
                b'`' => {
                    self.flush(lit, i);
                    i = self.code(i);
                    lit = i;
                }
                b'[' => {
                    self.flush(lit, i);
                    i = self.open_link(i);
                    lit = i;
                }
                b']' => {
                    self.flush(lit, i);
                    i = self.close_link(i);
                    lit = i;
                }
                b'<' => {
                    if self.bytes.get(i + 1).is_some_and(|&c| c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')) {
                        self.diag(i, Code::Html, &self.src.get(i..).unwrap_or_default().chars().take(12).collect::<String>());
                    }
                    i += 1;
                }
                b'!' => {
                    if self.bytes.get(i + 1) == Some(&b'[') {
                        self.diag(i, Code::PictureAlone, "");
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        if !self.stopped {
            self.flush(lit, n);
        }
    }

    /// A backslash at `i`: the escaped character, or the backslash itself and a problem.
    fn escape(&mut self, i: usize) -> usize {
        match self.bytes.get(i + 1) {
            Some(&c) if is_escapable(c) => {
                let one = [c];
                self.text(std::str::from_utf8(&one).unwrap_or_default());
                i + 2
            }
            _ => {
                let shown: String = self.src.get(i..).unwrap_or_default().chars().take(2).collect();
                self.diag(i, Code::UnknownEscape, &shown);
                self.text("\\");
                i + 1
            }
        }
    }

    fn emphasis_depth(&self) -> usize {
        self.stack.iter().filter(|f| matches!(f.kind, Kind::Strong | Kind::Em)).count()
    }

    fn top_kind(&self) -> Kind {
        self.stack.last().map_or(Kind::Root, |f| f.kind)
    }

    /// A run of stars at `i`.
    fn stars(&mut self, i: usize) -> usize {
        let run = self.bytes.get(i..).unwrap_or_default().iter().take_while(|&&b| b == b'*').count();
        if run >= 4 {
            self.diag(i, Code::StarRun, "");
            self.text(&"*".repeat(run));
            return i + run;
        }
        let mut left = run;
        while left > 0 && !self.stopped {
            left = match self.top_kind() {
                Kind::Em if left == 1 || self.emphasis_depth() == 2 => {
                    self.close(Kind::Em);
                    left - 1
                }
                Kind::Em => {
                    self.open(Kind::Strong, i);
                    left - 2
                }
                Kind::Strong if left >= 2 => {
                    self.close(Kind::Strong);
                    left - 2
                }
                Kind::Root | Kind::Link if left >= 2 => {
                    self.open(Kind::Strong, i);
                    left - 2
                }
                _ => {
                    self.open(Kind::Em, i);
                    left - 1
                }
            };
        }
        i + run
    }

    /// Opens an emphasis frame, or says why not and keeps the stars as text.
    fn open(&mut self, kind: Kind, at: usize) {
        let delim = if kind == Kind::Strong { "**" } else { "*" };
        if self.emphasis_depth() >= 2 {
            self.diag(at, Code::EmphasisNesting, delim);
            self.text(delim);
            return;
        }
        if self.reserve() {
            self.stack.push(Frame { kind, spans: Vec::new(), open_at: at, inner: 0 });
        }
    }

    /// Closes the open emphasis frame: a span, or, when nothing is in it, the text it was written as.
    fn close(&mut self, kind: Kind) {
        let Some(frame) = self.stack.pop() else { return };
        let delim = if kind == Kind::Strong { "**" } else { "*" };
        if !has_content(&frame.spans) {
            self.diag(frame.open_at, Code::EmptySpan, delim);
            self.spill(frame, delim, Some(delim));
            return;
        }
        let span = if kind == Kind::Strong { Span::Strong(frame.spans) } else { Span::Em(frame.spans) };
        self.put(span);
    }

    /// A frame that did not become a span: its opening as text, what was in it, and (when given) its closing as text.
    fn spill(&mut self, frame: Frame, open: &str, close: Option<&str>) {
        // The frame's own span was counted when it opened and is gone now; the text that stands for it is counted again by `text`.
        self.count = self.count.saturating_sub(1);
        self.text(open);
        for span in frame.spans {
            self.put(span);
        }
        if let Some(close) = close {
            self.text(close);
        }
    }

    /// A backtick at `i`: a code span, or an empty one or an open one as text.
    fn code(&mut self, i: usize) -> usize {
        let rest = self.src.get(i + 1..).unwrap_or_default();
        let Some(len) = rest.find('`') else {
            self.diag(i, Code::UnclosedCode, "");
            self.text("`");
            return i + 1;
        };
        let content = rest.get(..len).unwrap_or_default();
        let end = i + 1 + len + 1;
        if content.trim().is_empty() {
            self.diag(i, Code::EmptySpan, "`");
            self.text(self.src.get(i..end).unwrap_or_default());
        } else if self.reserve() {
            let span = Span::Code(content.to_owned());
            self.put(span);
        }
        end
    }

    fn open_link(&mut self, i: usize) -> usize {
        if let Some(link) = self.stack.iter_mut().find(|f| f.kind == Kind::Link) {
            link.inner += 1;
            self.diag(i, Code::LinkInLink, "");
            self.text("[");
        } else if self.reserve() {
            self.stack.push(Frame { kind: Kind::Link, spans: Vec::new(), open_at: i, inner: 0 });
        }
        i + 1
    }

    /// A `]` at `i`: the end of a link's text, and the target after it.
    fn close_link(&mut self, i: usize) -> usize {
        if let Some(link) = self.stack.iter_mut().find(|f| f.kind == Kind::Link && f.inner > 0) {
            // The end of a bracket that was opened inside a link's text: it is text, and so is what follows it.
            link.inner -= 1;
            self.text("]");
            return i + 1;
        }
        if !self.stack.iter().any(|f| f.kind == Kind::Link) {
            self.diag(i, Code::UnmatchedBracket, "]");
            self.text("]");
            return i + 1;
        }
        // What was opened inside the link's text and not closed is text.
        while self.top_kind() != Kind::Link {
            self.unclose_top();
        }
        let Some(link) = self.stack.pop() else { return i + 1 };
        if self.bytes.get(i + 1) != Some(&b'(') {
            self.diag(link.open_at, Code::UnmatchedBracket, "[");
            self.spill(link, "[", Some("]"));
            return i + 1;
        }
        let from = i + 2;
        match self.find_target_end(from) {
            Ok(end) => {
                let raw = self.src.get(from..end).unwrap_or_default();
                self.finish_link(link, raw, end)
            }
            Err(code) => {
                self.diag(link.open_at, code, "");
                self.spill(link, "[", Some("]"));
                i + 1
            }
        }
    }

    /// The index of the `)` that ends the target starting at `from`, looked for no further than the cap allows (a backslash escapes the byte after it).
    fn find_target_end(&self, from: usize) -> Result<usize, Code> {
        let window = MAX_TARGET_BYTES + 1;
        let mut k = from;
        while k <= from + window {
            match self.bytes.get(k) {
                None => return Err(Code::UnmatchedBracket),
                Some(b')') => return Ok(k),
                Some(b'\\') => k += 2,
                Some(_) => k += 1,
            }
        }
        Err(Code::TargetTooLong)
    }

    /// The link whose text is in `link` and whose target is `raw`, ending at `end` (the `)`).
    fn finish_link(&mut self, link: Frame, raw: &str, end: usize) -> usize {
        match resolve(raw) {
            Err((code, detail)) => {
                self.diag(link.open_at, code, &detail);
                // The link is shown as its text.
                self.count = self.count.saturating_sub(1);
                for span in link.spans {
                    self.put(span);
                }
            }
            Ok(_) if !has_content(&link.spans) => {
                self.diag(link.open_at, Code::EmptySpan, "[]");
                self.count = self.count.saturating_sub(1);
                self.text(self.src.get(link.open_at..=end).unwrap_or_default());
            }
            Ok(target) => self.put(Span::Link { target, text: link.spans }),
        }
        end + 1
    }

    /// Takes the top frame off as text, saying what was left open.
    fn unclose_top(&mut self) {
        let Some(frame) = self.stack.pop() else { return };
        let (delim, code) = match frame.kind {
            Kind::Strong => ("**", Code::UnclosedEmphasis),
            Kind::Em => ("*", Code::UnclosedEmphasis),
            Kind::Link => ("[", Code::UnmatchedBracket),
            Kind::Root => {
                self.stack.push(frame);
                return;
            }
        };
        if !self.stopped {
            self.diag(frame.open_at, code, delim);
        }
        self.spill(frame, delim, None);
    }

    /// The end of the text: what is still open is text.
    fn finish(&mut self) {
        while self.stack.len() > 1 {
            self.unclose_top();
        }
    }
}

/// Whether a run of spans shows anything: text with something besides white space, or any other span.
fn has_content(spans: &[Span]) -> bool {
    spans.iter().any(|s| match s {
        Span::Text(t) => t.chars().any(|c| !c.is_whitespace()),
        _ => true,
    })
}

/// The characters a backslash may escape.
fn is_escapable(b: u8) -> bool {
    matches!(b, b'*' | b'[' | b']' | b'(' | b')' | b'`' | b'|' | b'\\')
}

// ------------------------------------------------------------------------------------------------ targets

/// Reads a link's target (with its escapes) as one of the kinds, or says which rule it breaks.
fn resolve(raw: &str) -> Result<Target, (Code, String)> {
    let target = unescape(raw).ok_or_else(|| (Code::TargetSyntax, raw.to_owned()))?;
    if target.len() > MAX_TARGET_BYTES {
        return Err((Code::TargetTooLong, target));
    }
    if target.is_empty() || target.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err((Code::TargetSyntax, target));
    }
    if let Some(rest) = target.strip_prefix("topic:") {
        let (id, anchor) = match rest.split_once('#') {
            Some((id, anchor)) => (id, Some(anchor)),
            None => (rest, None),
        };
        if valid_id(id) && anchor.is_none_or(valid_id) {
            return Ok(Target::Topic { id: id.to_owned(), anchor: anchor.map(str::to_owned) });
        }
        return Err((Code::TargetId, target));
    }
    if let Some(anchor) = target.strip_prefix('#') {
        return if valid_id(anchor) { Ok(Target::Anchor(anchor.to_owned())) } else { Err((Code::TargetId, target)) };
    }
    if let Some(id) = target.strip_prefix("ctl:") {
        return if valid_control(id) { Ok(Target::Control(id.to_owned())) } else { Err((Code::TargetId, target)) };
    }
    if let Some(id) = target.strip_prefix("cmd:") {
        return if valid_command(id) { Ok(Target::Command(id.to_owned())) } else { Err((Code::TargetId, target)) };
    }
    if let Some(key) = target.strip_prefix("key:") {
        return Chord::parse(key).map(Target::Key).ok_or((Code::TargetKey, target));
    }
    if target.starts_with("https://") || target.starts_with("http://") {
        return if url::allowed(&target) { Ok(Target::Web(target)) } else { Err((Code::TargetWeb, target)) };
    }
    Err((Code::TargetSyntax, target))
}

/// A target with its escapes read, or `None` for a backslash before a character that is not escaped.
fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(e) if e.is_ascii() && is_escapable(e as u8) => out.push(e),
            _ => return None,
        }
    }
    Some(out)
}

/// A topic id or an anchor: lowercase letters and digits in words joined by single dashes, at most 64 bytes.
pub(crate) fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A ribbon control's id: a letter or an underscore, then letters, digits and underscores, at most 64 bytes.
pub(crate) fn valid_control(s: &str) -> bool {
    s.len() <= 64
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A command's id: two or more words of letters, digits and underscores joined by dots, the first starting with a letter, at most 96 bytes.
pub(crate) fn valid_command(s: &str) -> bool {
    let mut parts = s.split('.');
    let first_ok = parts
        .next()
        .is_some_and(|p| p.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()) && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    let mut more = 0usize;
    let rest_ok = parts.all(|p| {
        more += 1;
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    });
    s.len() <= 96 && first_ok && rest_ok && more >= 1
}
