//! The walk of a label into paragraphs of runs (S-R24 to S-R27).
//!
//! [`read_html`] walks the tokens of the label with a stack of the elements that are open (at most `max_label_depth` of them, 64, each holding the format it gives its text) and
//! builds the paragraphs as the text comes: white space is collapsed as HTML collapses it, a block ends the paragraph, and a run is merged into the one before it when their formats
//! are equal. [`read_plain`] reads a label that is not HTML: its text as it stands, a line feed a line break.
//!
//! **What an element does** (S-R25's table). The formatting elements (`b` `strong` `i` `em` `u` `ins` `s` `strike` `del` `sub` `sup`, `font` with `face` `size` `color`, `span`,
//! `a`) give their text a format, and a `style` attribute on any of them adds the seven properties [`css`](super::css) reads. The blocks (`div` `p` `h1` to `h6` `li` `tr`
//! `blockquote` `pre`, and `ul` `ol` `table` and the parts of a table, which hold blocks) end the paragraph before and after, and what is empty between two blocks makes no
//! paragraph; `br` is a [`LINE_BREAK`] and `hr` ends the paragraph; a `li` starts with "• ", or with "1. ", "2. " in an `ol`; a `td` or `th` after another in its row puts a tab
//! in. What it drops: `script` and `style`, which go with their content and are not counted, `template` and `noscript` likewise; `img` `svg` `math` `iframe` `object` `embed`
//! `video` `audio` `canvas` `input` `form` `button` `select` `textarea`, which go with their content and are counted once each in `drawio_label_content_dropped`. Any other element
//! is no more than its text: its tags are not followed, so a `style` on it is not read.
//!
//! **Beyond the table, as a browser shows it** (so that a label reads as it looks): the first line feed after `<pre>` is not text; a `br` at the end of a block makes no line of its
//! own (`<div>A<br></div>` is one line, and `<div><br></div>` is an empty one); white space at the start and the end of a line, and a second space after a space, is not kept; an
//! element HTML lets a page leave open is ended where the next begins (a `p` by a block, a `li` by a `li`, a `td` by a `td`, a `tr` by a `tr`: the one at the top of the stack, no
//! more); a tag past the depth cap is not followed and its text is read as the text of the element it is in.
//!
//! **The budget.** Every pass charges its steps before it runs ([`Unit::LabelSteps`]); see the module [`html`](super).

use std::borrow::Cow;

use crate::decode::keep_char;
use crate::error::DrawioError;
use crate::report::{Reading, Unit};

use super::css::{self, Ctx};
use super::entity;
use super::lexer::{Lexer, Tag, Tok};
use super::{Align, LINE_BREAK, Label, Link, Options, PARA_STEPS, Para, RUN_STEPS, Run, RunFmt, Script, warn};

/// The elements this reader follows. A kind that is not here is not followed (its tags are skipped and its text is read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum El {
    B,
    Strong,
    I,
    Em,
    U,
    Ins,
    S,
    Strike,
    Del,
    Sub,
    Sup,
    Font,
    Span,
    A,
    Div,
    P,
    Heading,
    Blockquote,
    Pre,
    Ul,
    Ol,
    Li,
    Table,
    Section,
    Tr,
    Cell,
}

/// The number of kinds of [`El`]: the size of the count of what is open.
const EL_COUNT: usize = 26;

impl El {
    /// A block ends the paragraph before it and after it.
    fn is_block(self) -> bool {
        matches!(self, El::Div | El::P | El::Heading | El::Blockquote | El::Pre | El::Ul | El::Ol | El::Li | El::Table | El::Section | El::Tr)
    }

    /// What the element gives the format of its text before its own `style` is read.
    fn give(self, fmt: &mut RunFmt) {
        match self {
            El::B | El::Strong | El::Heading => fmt.bold = Some(true),
            El::I | El::Em => fmt.italic = Some(true),
            El::U | El::Ins => fmt.underline = Some(true),
            El::S | El::Strike | El::Del => fmt.strike = Some(true),
            El::Sub => fmt.script = Script::Sub,
            El::Sup => fmt.script = Script::Super,
            _ => {}
        }
    }
}

/// How an element that is dropped is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// It has no content: its tag is all there is.
    Void,
    /// Its content is text, to the end tag of its name: `script`, `style`, `iframe`, `textarea`.
    Raw,
    /// Its content is markup, to the end tag that closes it, elements of its own name inside it counted.
    Nested,
}

/// An element that is dropped with what is inside it.
#[derive(Debug, Clone, Copy)]
struct Dropping {
    name: &'static str,
    mode: Mode,
    /// Whether it is counted in `drawio_label_content_dropped`.
    counted: bool,
}

/// What a tag's name says.
enum Class {
    Kept(El),
    Br,
    Hr,
    Drop(Dropping),
    Other,
}

const fn dropped(name: &'static str, mode: Mode, counted: bool) -> Class {
    Class::Drop(Dropping { name, mode, counted })
}

/// What `name` (as written, in any case) is.
fn classify(name: &str) -> Class {
    let mut buf = [0u8; 10];
    let Some(dst) = buf.get_mut(..name.len()) else { return Class::Other };
    for (d, s) in dst.iter_mut().zip(name.bytes()) {
        *d = s.to_ascii_lowercase();
    }
    let Ok(lower) = std::str::from_utf8(dst) else { return Class::Other };
    match lower {
        "b" => Class::Kept(El::B),
        "strong" => Class::Kept(El::Strong),
        "i" => Class::Kept(El::I),
        "em" => Class::Kept(El::Em),
        "u" => Class::Kept(El::U),
        "ins" => Class::Kept(El::Ins),
        "s" => Class::Kept(El::S),
        "strike" => Class::Kept(El::Strike),
        "del" => Class::Kept(El::Del),
        "sub" => Class::Kept(El::Sub),
        "sup" => Class::Kept(El::Sup),
        "font" => Class::Kept(El::Font),
        "span" => Class::Kept(El::Span),
        "a" => Class::Kept(El::A),
        "div" => Class::Kept(El::Div),
        "p" => Class::Kept(El::P),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Class::Kept(El::Heading),
        "blockquote" => Class::Kept(El::Blockquote),
        "pre" => Class::Kept(El::Pre),
        "ul" => Class::Kept(El::Ul),
        "ol" => Class::Kept(El::Ol),
        "li" => Class::Kept(El::Li),
        "table" => Class::Kept(El::Table),
        "thead" | "tbody" | "tfoot" => Class::Kept(El::Section),
        "tr" => Class::Kept(El::Tr),
        "td" | "th" => Class::Kept(El::Cell),
        "br" => Class::Br,
        "hr" => Class::Hr,
        "script" => dropped("script", Mode::Raw, false),
        "style" => dropped("style", Mode::Raw, false),
        "template" => dropped("template", Mode::Nested, false),
        "noscript" => dropped("noscript", Mode::Nested, false),
        "img" => dropped("img", Mode::Void, true),
        "input" => dropped("input", Mode::Void, true),
        "embed" => dropped("embed", Mode::Void, true),
        "svg" => dropped("svg", Mode::Nested, true),
        "math" => dropped("math", Mode::Nested, true),
        "iframe" => dropped("iframe", Mode::Raw, true),
        "object" => dropped("object", Mode::Nested, true),
        "video" => dropped("video", Mode::Nested, true),
        "audio" => dropped("audio", Mode::Nested, true),
        "canvas" => dropped("canvas", Mode::Nested, true),
        "form" => dropped("form", Mode::Nested, true),
        "button" => dropped("button", Mode::Nested, true),
        "select" => dropped("select", Mode::Nested, true),
        "textarea" => dropped("textarea", Mode::Raw, true),
        _ => Class::Other,
    }
}

/// HTML's white space as text collapses it: space, tab, line feed, form feed and carriage return.
fn is_html_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r')
}

/// An element that is open: its kind, the format and alignment it gives what is inside it, and whether white space is kept there.
struct Frame {
    el: El,
    fmt: RunFmt,
    align: Option<Align>,
    pre: bool,
}

/// A list that is open: whether it counts its items, and the number the next one gets.
struct ListState {
    ordered: bool,
    next: u32,
}

/// The label being built.
struct Builder<'r, 'l, 'o> {
    reading: &'r mut Reading<'l>,
    opts: &'o Options<'o>,
    /// The cell's font size, for `em`.
    base_px: f64,
    /// The largest size a run is given.
    max_px: f64,
    /// The most elements kept open.
    depth_cap: usize,
    paras: Vec<Para>,
    /// The paragraph being built.
    cur: Para,
    /// Whether it holds anything yet (a paragraph with only a line break is an empty line, and is one).
    started: bool,
    /// White space is not kept here: at the start of a paragraph or a line or a cell.
    at_start: bool,
    /// The last character of the paragraph is a space that collapsing made, which an end of a line takes back.
    trailing_space: bool,
    /// The last run of the paragraph is the one the open elements' format writes into.
    run_ready: bool,
    stack: Vec<Frame>,
    /// How many elements of each kind are open, so that an end tag for one that is not is no walk of the stack.
    open: [u32; EL_COUNT],
    lists: Vec<ListState>,
    cells_in_row: bool,
    /// The next character is the first of a `pre`, whose line feed is not text.
    skip_lf: bool,
    /// The last character in a `pre` was a carriage return, so that a line feed after it is the same break.
    last_cr: bool,
    /// The format of text with no element open.
    root: RunFmt,
    link: Option<Link>,
    link_seen: bool,
    /// The steps owed for the paragraphs and runs built since the last token: charged when the token is done ([`Builder::flush`]).
    owed: u64,
}

impl<'r, 'l, 'o> Builder<'r, 'l, 'o> {
    fn new(reading: &'r mut Reading<'l>, opts: &'o Options<'o>) -> Builder<'r, 'l, 'o> {
        let sane = |v: f64, fallback: f64| if v.is_finite() && v > 0.0 { v } else { fallback };
        Builder {
            base_px: sane(opts.font_px, 12.0),
            max_px: sane(reading.limits.max_coord_px, 10_000_000.0),
            depth_cap: reading.limits.max_label_depth,
            reading,
            opts,
            paras: Vec::new(),
            cur: Para::default(),
            started: false,
            at_start: true,
            trailing_space: false,
            run_ready: false,
            stack: Vec::new(),
            open: [0; EL_COUNT],
            lists: Vec::new(),
            cells_in_row: false,
            skip_lf: false,
            last_cr: false,
            root: RunFmt::default(),
            link: None,
            link_seen: false,
            owed: 0,
        }
    }

    /// Charges the steps owed for what the last token built: a paragraph and a run each cost more than a byte of text (an allocation, a format to compare), so that the work counted
    /// is the work done, and a label of a million paragraphs is not as cheap as a million bytes.
    fn flush(&mut self) -> Result<(), DrawioError> {
        let owed = std::mem::take(&mut self.owed);
        if owed == 0 { Ok(()) } else { self.reading.charge(Unit::LabelSteps, owed) }
    }

    fn charge(&mut self, steps: usize) -> Result<(), DrawioError> {
        self.reading.charge(Unit::LabelSteps, u64::try_from(steps).unwrap_or(u64::MAX))
    }

    fn into_label(self) -> Label {
        Label { paras: self.paras, link: self.link }
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- the walk

    /// Walks the tokens of `text`.
    fn run(&mut self, text: &str) -> Result<(), DrawioError> {
        let mut lexer = Lexer::new(text);
        while let Some(tok) = lexer.next() {
            self.charge(1)?;
            match tok {
                Tok::Text(raw) => self.text(raw)?,
                Tok::Start => self.start(&mut lexer)?,
                Tok::End(name) => self.end(name),
                Tok::Skip => {}
            }
            self.flush()?;
        }
        Ok(())
    }

    /// A piece of text: its references decoded, then each character as the paragraph takes it.
    fn text(&mut self, raw: &str) -> Result<(), DrawioError> {
        self.reading.charge(Unit::LabelSteps, entity::cost(raw))?;
        for c in entity::unescape(raw).chars() {
            self.html_char(c);
        }
        Ok(())
    }

    /// The value of the attribute `name` of `tag`, its references decoded, its steps charged.
    fn attribute<'t>(&mut self, tag: &Tag<'t>, name: &str) -> Result<Option<Cow<'t, str>>, DrawioError> {
        let Some(raw) = tag.attr(name) else { return Ok(None) };
        self.reading.charge(Unit::LabelSteps, entity::cost(raw))?;
        Ok(Some(entity::unescape(raw)))
    }

    /// A start tag, which the lexer holds.
    fn start(&mut self, lexer: &mut Lexer<'_>) -> Result<(), DrawioError> {
        match classify(lexer.tag().name) {
            Class::Kept(el) => {
                if self.push_frame(el, lexer.tag())? && lexer.tag().self_close {
                    self.close(el);
                }
            }
            Class::Br => self.line_break(),
            Class::Hr => self.finish_paragraph(),
            Class::Drop(dropping) => self.drop_element(dropping, lexer)?,
            Class::Other => {}
        }
        Ok(())
    }

    /// An end tag: it closes the element of its kind that is open nearest the top, and what is open above it; one for a kind that is not open is nothing.
    fn end(&mut self, name: &str) {
        if let Class::Kept(el) = classify(name) {
            self.close(el);
        }
    }

    /// Notes an element that is dropped, and skips what it holds.
    fn drop_element(&mut self, dropping: Dropping, lexer: &mut Lexer<'_>) -> Result<(), DrawioError> {
        if dropping.counted {
            let case = self.case(&format!("<{}>", dropping.name));
            self.reading.note(warn::LABEL_CONTENT_DROPPED, &case);
        }
        match dropping.mode {
            Mode::Void => {}
            Mode::Raw => lexer.skip_raw_text(dropping.name),
            Mode::Nested if lexer.tag().self_close => {}
            Mode::Nested => self.skip_nested(lexer, dropping.name)?,
        }
        Ok(())
    }

    /// Skips the content of an element of markup: to the end tag that closes it, one for each token on the way.
    fn skip_nested(&mut self, lexer: &mut Lexer<'_>, name: &str) -> Result<(), DrawioError> {
        let mut depth = 1usize;
        while depth > 0 {
            let Some(tok) = lexer.next() else { break };
            self.charge(1)?;
            match tok {
                Tok::Start if lexer.tag().name.eq_ignore_ascii_case(name) && !lexer.tag().self_close => depth = depth.saturating_add(1),
                Tok::End(end) if end.eq_ignore_ascii_case(name) => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        Ok(())
    }

    /// `what`, with the cell's id in front of it when the label has one.
    fn case(&self, what: &str) -> String {
        if self.opts.cell.is_empty() { what.to_owned() } else { format!("{}: {what}", self.opts.cell) }
    }

    // ----------------------------------------------------------------------------------------------------------------------------------------- elements

    /// Whether opening `el` ends the element at the top of the stack, which HTML lets a page leave open: a `p` is ended by a block, a `li` by another `li`, a cell by another
    /// cell, and a cell and a row by a row. Only the top of the stack is looked at, so that this costs nothing and a nested list or table is not touched.
    fn implied_end(&self, el: El) -> bool {
        match (el, self.stack.last().map(|f| f.el)) {
            (El::Li, Some(El::Li)) | (El::Cell, Some(El::Cell)) | (El::Tr, Some(El::Cell | El::Tr)) => true,
            (el, Some(El::P)) => el.is_block(),
            _ => false,
        }
    }

    /// Opens an element: the frame it is, the break a block makes, and what its kind does at its start. `false` when it was not opened, as past the depth cap.
    fn push_frame(&mut self, el: El, tag: &Tag<'_>) -> Result<bool, DrawioError> {
        while self.implied_end(el) {
            self.pop_frame();
        }
        if self.stack.len() >= self.depth_cap {
            return Ok(false);
        }
        if el.is_block() {
            self.finish_paragraph();
        }
        let frame = self.make_frame(el, tag)?;
        if let Some(n) = self.open.get_mut(el as usize) {
            *n = n.saturating_add(1);
        }
        self.stack.push(frame);
        self.run_ready = false;
        self.on_open(el);
        Ok(true)
    }

    /// The format and alignment the element gives: what is outside it, then what its kind gives, then its own attributes.
    fn make_frame(&mut self, el: El, tag: &Tag<'_>) -> Result<Frame, DrawioError> {
        let (mut fmt, mut align, pre) = match self.stack.last() {
            Some(top) => (top.fmt.clone(), top.align, top.pre),
            None => (RunFmt::default(), None, false),
        };
        el.give(&mut fmt);
        match el {
            El::Font => self.font_attributes(tag, &mut fmt)?,
            El::A => self.note_link(tag)?,
            _ => {}
        }
        if let Some(style) = self.attribute(tag, "style")? {
            self.charge(style.len())?;
            let ctx = Ctx { base_px: self.base_px, max_px: self.max_px, block: el.is_block() };
            css::apply(&style, &ctx, &mut fmt, &mut align);
        }
        Ok(Frame { el, fmt, align, pre: pre || el == El::Pre })
    }

    /// `face`, `size` and `color` of a `font`.
    fn font_attributes(&mut self, tag: &Tag<'_>, fmt: &mut RunFmt) -> Result<(), DrawioError> {
        if let Some(face) = self.attribute(tag, "face")?
            && let Some(family) = css::first_family(&face)
        {
            fmt.family = Some(family);
        }
        if let Some(size) = self.attribute(tag, "size")?
            && let Some(n) = css::legacy_font_size(&size)
        {
            fmt.size_px = Some(css::html_font_px(n));
        }
        if let Some(color) = self.attribute(tag, "color")?
            && let Some(rgb) = css::colour(&color)
        {
            fmt.color = Some(rgb);
        }
        Ok(())
    }

    /// The first `a` with an `href` is the label's link, when its address is one that is kept.
    fn note_link(&mut self, tag: &Tag<'_>) -> Result<(), DrawioError> {
        if self.link_seen {
            return Ok(());
        }
        if let Some(href) = self.attribute(tag, "href")? {
            self.link_seen = true;
            self.charge(href.len())?;
            self.link = css::link_of(&href);
        }
        Ok(())
    }

    /// What an element does when it opens, once its frame is on the stack.
    fn on_open(&mut self, el: El) {
        match el {
            El::Ul | El::Ol => self.lists.push(ListState { ordered: el == El::Ol, next: 1 }),
            El::Li => self.list_item(),
            El::Table | El::Tr => self.cells_in_row = false,
            El::Cell => self.cell_start(),
            El::Pre => {
                self.skip_lf = true;
                self.last_cr = false;
            }
            _ => {}
        }
    }

    /// The bullet or the number a list item starts with.
    fn list_item(&mut self) {
        match self.lists.last_mut() {
            Some(list) if list.ordered => {
                let n = list.next;
                list.next = n.saturating_add(1);
                self.emit_number(n);
                self.emit('.');
            }
            _ => self.emit('\u{2022}'),
        }
        self.emit(' ');
        self.at_start = true;
        self.trailing_space = false;
    }

    /// The decimal digits of `n`, written into the paragraph with no string made for them.
    fn emit_number(&mut self, mut n: u32) {
        let mut digits = [b'0'; 10];
        let mut at = digits.len();
        while at > 0 {
            at -= 1;
            if let Some(slot) = digits.get_mut(at) {
                *slot = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
            }
            n /= 10;
            if n == 0 {
                break;
            }
        }
        for digit in digits.get(at..).unwrap_or(&[]) {
            self.emit(char::from(*digit));
        }
    }

    /// A cell after another in its row is set off by a tab.
    fn cell_start(&mut self) {
        if self.cells_in_row {
            self.trim_trailing_space();
            self.emit('\t');
        }
        self.cells_in_row = true;
        self.at_start = true;
        self.trailing_space = false;
    }

    /// Closes the element of kind `el` nearest the top of the stack, and the ones above it. Each frame is popped once, which a push paid for.
    fn close(&mut self, el: El) {
        if self.open.get(el as usize).is_none_or(|n| *n == 0) {
            return;
        }
        while let Some(top) = self.stack.last() {
            let found = top.el == el;
            self.pop_frame();
            if found {
                break;
            }
        }
    }

    fn pop_frame(&mut self) {
        let Some(frame) = self.stack.pop() else { return };
        if let Some(n) = self.open.get_mut(frame.el as usize) {
            *n = n.saturating_sub(1);
        }
        match frame.el {
            El::Ul | El::Ol => {
                self.lists.pop();
            }
            El::Cell => self.trim_trailing_space(),
            _ => {}
        }
        if frame.el.is_block() {
            self.finish_paragraph();
        }
        self.run_ready = false;
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- paragraphs

    /// The alignment of the block the text is in.
    fn top_align(&self) -> Option<Align> {
        self.stack.last().and_then(|f| f.align)
    }

    /// Writes `c` into the paragraph, in the format of the elements that are open.
    fn emit(&mut self, c: char) {
        if !self.started {
            self.cur.align = self.top_align();
            self.started = true;
        }
        if !self.run_ready {
            self.open_run();
        }
        if let Some(run) = self.cur.runs.last_mut() {
            run.text.push(c);
        }
    }

    /// Makes the last run of the paragraph the one for the format now in force: it is the last run when its format is equal, else a new one.
    fn open_run(&mut self) {
        let fmt = self.stack.last().map_or(&self.root, |f| &f.fmt);
        if self.cur.runs.last().is_none_or(|last| last.fmt != *fmt) {
            self.cur.runs.push(Run { text: String::new(), fmt: fmt.clone() });
            self.owed = self.owed.saturating_add(RUN_STEPS);
        }
        self.run_ready = true;
    }

    /// Takes back the space that collapsing wrote at the end of the paragraph, if that is the last thing in it.
    fn trim_trailing_space(&mut self) {
        if !self.trailing_space {
            return;
        }
        self.trailing_space = false;
        self.run_ready = false;
        if let Some(run) = self.cur.runs.last_mut()
            && run.text.ends_with(' ')
        {
            run.text.pop();
            if run.text.is_empty() {
                self.cur.runs.pop();
            }
        }
    }

    /// A line break inside the paragraph, taking back the space before it.
    fn line_break(&mut self) {
        self.trim_trailing_space();
        self.emit(LINE_BREAK);
        self.at_start = true;
        self.trailing_space = false;
    }

    /// Ends the paragraph, if it holds anything. A break at its very end makes no line of its own, and the paragraph that is left with nothing is an empty line.
    fn finish_paragraph(&mut self) {
        if !self.started {
            return;
        }
        self.trim_trailing_space();
        if let Some(run) = self.cur.runs.last_mut()
            && run.text.ends_with(LINE_BREAK)
        {
            run.text.pop();
            if run.text.is_empty() {
                self.cur.runs.pop();
            }
        }
        let para = std::mem::take(&mut self.cur);
        self.paras.push(para);
        self.owed = self.owed.saturating_add(PARA_STEPS);
        self.started = false;
        self.at_start = true;
        self.trailing_space = false;
        self.run_ready = false;
    }

    // ------------------------------------------------------------------------------------------------------------------------------------------ text

    /// One character of the text of an HTML label.
    fn html_char(&mut self, c: char) {
        let first_of_pre = std::mem::take(&mut self.skip_lf);
        if first_of_pre && (c == '\n' || c == '\r') {
            self.last_cr = c == '\r';
            return;
        }
        if !keep_char(c) {
            return;
        }
        if self.stack.last().is_some_and(|f| f.pre) {
            self.pre_char(c);
        } else if is_html_ws(c) {
            if !self.at_start && !self.trailing_space {
                self.emit(' ');
                self.trailing_space = true;
            }
        } else {
            self.emit(c);
            self.at_start = false;
            self.trailing_space = false;
        }
    }

    /// One character inside a `pre`: kept as it is, and a line feed (or a carriage return, or both) is a line break.
    fn pre_char(&mut self, c: char) {
        match c {
            '\n' if self.last_cr => self.last_cr = false,
            '\r' | '\n' => {
                self.last_cr = c == '\r';
                self.emit(LINE_BREAK);
            }
            _ => {
                self.last_cr = false;
                self.emit(c);
            }
        }
        self.at_start = false;
        self.trailing_space = false;
    }

    /// The text of a plain label: as it stands, each line feed (a carriage return, or a pair) a line break, a character XML cannot hold left out.
    fn plain(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    self.emit(LINE_BREAK);
                }
                '\n' => self.emit(LINE_BREAK),
                c if keep_char(c) => self.emit(c),
                _ => {}
            }
        }
    }
}

/// `text` cut at `max` bytes, back to a character, with the work of reading it charged and a label past the cap noted.
fn bounded<'t>(text: &'t str, opts: &Options<'_>, reading: &mut Reading<'_>) -> Result<&'t str, DrawioError> {
    let max = reading.limits.max_label_bytes;
    let kept = if text.len() > max {
        let case = if opts.cell.is_empty() { format!("{} bytes", text.len()) } else { format!("{}: {} bytes", opts.cell, text.len()) };
        reading.note(warn::LABEL_TOO_LONG, &case);
        let mut end = max;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.get(..end).unwrap_or("")
    } else {
        text
    };
    reading.charge(Unit::LabelSteps, u64::try_from(kept.len()).unwrap_or(u64::MAX))?;
    Ok(kept)
}

/// The first text of the label between `$$` and `$$`, or between two backquotes (AsciiMath), delimiters included.
fn math_span(text: &str) -> Option<&str> {
    if let Some(open) = text.find("$$") {
        let from = open.saturating_add(2);
        if let Some(len) = text.get(from..).and_then(|rest| rest.find("$$")) {
            return text.get(open..from.saturating_add(len).saturating_add(2));
        }
    }
    let open = text.find('`')?;
    let from = open.saturating_add(1);
    let len = text.get(from..)?.find('`')?;
    text.get(open..from.saturating_add(len).saturating_add(1))
}

/// Counts the label in `drawio_math_shown_as_text` when the drawing's `math` is on and the label has text that would be typeset (S-R27).
fn note_math(label: &Label, opts: &Options<'_>, reading: &mut Reading<'_>) -> Result<(), DrawioError> {
    if !opts.math {
        return Ok(());
    }
    let text = label.text();
    reading.charge(Unit::LabelSteps, u64::try_from(text.len()).unwrap_or(u64::MAX))?;
    if let Some(span) = math_span(&text) {
        let case = if opts.cell.is_empty() { span.to_owned() } else { format!("{}: {span}", opts.cell) };
        reading.note(warn::MATH_SHOWN_AS_TEXT, &case);
    }
    Ok(())
}

/// Reads an HTML label (the cell's style holds `html=1`) into paragraphs of runs (S-R25): by its own tokenizer, with the budgets of the reader, and with nothing in it run or
/// fetched. A label past the cap of a label is cut and noted (`drawio_label_too_long`); an element that holds what DiaCraft does not show is dropped and counted
/// (`drawio_label_content_dropped`); a drawing with `math` on counts a label with text to typeset (`drawio_math_shown_as_text`).
///
/// An error is a refusal of the read's work (`drawio_too_complex`): no label is refused for what it holds.
pub fn read_html(text: &str, opts: &Options<'_>, reading: &mut Reading<'_>) -> Result<Label, DrawioError> {
    let text = bounded(text, opts, reading)?;
    let mut builder = Builder::new(reading, opts);
    builder.run(text)?;
    builder.finish_paragraph();
    builder.flush()?;
    let label = builder.into_label();
    note_math(&label, opts, reading)?;
    Ok(label)
}

/// Reads a plain label (the cell's style holds no `html=1`) as it stands (S-R26): its text, each line feed a line break, `<` and `&` and every other character themselves. The
/// cap of a label and the count of `drawio_math_shown_as_text` are as [`read_html`]'s.
pub fn read_plain(text: &str, opts: &Options<'_>, reading: &mut Reading<'_>) -> Result<Label, DrawioError> {
    let text = bounded(text, opts, reading)?;
    let mut builder = Builder::new(reading, opts);
    builder.plain(text);
    builder.finish_paragraph();
    builder.flush()?;
    let label = builder.into_label();
    note_math(&label, opts, reading)?;
    Ok(label)
}
