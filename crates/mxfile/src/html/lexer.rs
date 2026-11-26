//! The tokenizer of an HTML label: tags, ends of tags and text (S-R25).
//!
//! Adapted from WordCraft's `lexer.rs` (`docs/provenance.md`): the cases of `next` (text up to the next `<`, a comment up to `-->`, a declaration or an instruction up to `>`,
//! an end tag, a start tag with its attributes, a `<` that starts nothing), `skip_past_end_tag` for the content of a raw-text element, and the attribute loop. What changed: a
//! token **borrows** the label (text and names and values are slices of it, with no `String` for a character or a tag) and the references of a value are decoded only when a
//! reader asks for it; nothing is indexed (`.get()` and `.get(a..b)` on positions that are always on an ASCII byte); a tag keeps at most [`MAX_ATTRS`] attributes (the rest are
//! walked and not kept, where WordCraft's kept 64 and copied each) and lives in the lexer, which reuses it for each start tag; **a tag with no `>` before the end of the label is
//! dropped** (HTML's end of file in a tag) where WordCraft's was kept, so that no unclosed tag, quote or comment makes a second pass over the text; `</` with no name is skipped to
//! the next `>` as a bogus comment, as HTML has it, where WordCraft's was the text `</`; `<!-->` and `<!--->` are the empty comments HTML has; and the search for the end tag of
//! a raw-text element does not build a string for the name.
//!
//! **Linear.** Every method moves `pos` forward, and each byte is passed over once: a search for a byte that is not there moves `pos` to the end of the label, never back to try
//! again. A hundred thousand unclosed `<script` cost one walk to the end, and the first one takes the rest of the label.

/// Most attributes a tag keeps. A tag with more has the rest read and left out: the attributes this reader looks at (`style`, `face`, `size`, `color`, `href`) come first in any
/// real label.
pub(super) const MAX_ATTRS: usize = 16;

/// A token of the label.
#[derive(Debug)]
pub(super) enum Tok<'a> {
    /// Text up to the next `<`, as written (its references are not decoded).
    Text(&'a str),
    /// A start tag, which [`Lexer::tag`] gives.
    Start,
    /// An end tag's name, as written.
    End(&'a str),
    /// A comment, a declaration, an instruction, an end tag with no name, or a tag with no `>`: nothing.
    Skip,
}

/// A start tag: its name as written, its first attributes as written, and whether it ended in `/>`.
#[derive(Debug)]
pub(super) struct Tag<'a> {
    pub(super) name: &'a str,
    attrs: [(&'a str, &'a str); MAX_ATTRS],
    count: usize,
    pub(super) self_close: bool,
}

impl<'a> Tag<'a> {
    fn empty() -> Tag<'a> {
        Tag { name: "", attrs: [("", ""); MAX_ATTRS], count: 0, self_close: false }
    }

    fn reset(&mut self, name: &'a str) {
        self.name = name;
        self.count = 0;
        self.self_close = false;
    }

    fn push(&mut self, name: &'a str, value: &'a str) {
        if let Some(slot) = self.attrs.get_mut(self.count) {
            *slot = (name, value);
            self.count += 1;
        }
    }

    /// The value, as written, of the first attribute called `name` (ASCII case does not matter): empty for an attribute with no value.
    pub(super) fn attr(&self, name: &str) -> Option<&'a str> {
        self.attrs.get(..self.count)?.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| *v)
    }
}

/// HTML's white space: space, tab, line feed, form feed and carriage return.
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0C | b'\r')
}

/// A byte of a tag's name: ASCII letters and digits, `:` and `-`.
fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b':' || c == b'-'
}

/// The position after the run of bytes from `at` that `keep` accepts.
fn run_end(bytes: &[u8], at: usize, keep: impl Fn(u8) -> bool) -> usize {
    let mut i = at;
    while bytes.get(i).is_some_and(|c| keep(*c)) {
        i = i.saturating_add(1);
    }
    i
}

/// The tokenizer: the label, how far it has been read, and the start tag it last gave.
pub(super) struct Lexer<'a> {
    s: &'a str,
    pos: usize,
    tag: Tag<'a>,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(s: &'a str) -> Lexer<'a> {
        Lexer { s, pos: 0, tag: Tag::empty() }
    }

    /// The start tag that the last [`Tok::Start`] was.
    pub(super) fn tag(&self) -> &Tag<'a> {
        &self.tag
    }

    fn bytes(&self) -> &'a [u8] {
        self.s.as_bytes()
    }

    /// The text from `a` to `b`: both are always on an ASCII byte or at an end, so this is never `None`; empty if it were.
    fn slice(&self, a: usize, b: usize) -> &'a str {
        self.s.get(a..b).unwrap_or("")
    }

    /// The first `needle` at or after `from`.
    fn find_byte(&self, from: usize, needle: u8) -> Option<usize> {
        let k = self.bytes().get(from..)?.iter().position(|b| *b == needle)?;
        from.checked_add(k)
    }

    /// Moves past the next `needle`, or to the end when there is none.
    fn skip_past_byte(&mut self, needle: u8) {
        self.pos = match self.find_byte(self.pos, needle) {
            Some(k) => k.saturating_add(1),
            None => self.bytes().len(),
        };
    }

    /// Moves to the end of the label: what a tag, a quote or a comment with no end takes.
    fn drop_rest(&mut self) -> Tok<'a> {
        self.pos = self.bytes().len();
        Tok::Skip
    }

    /// The next token, or `None` at the end of the label.
    pub(super) fn next(&mut self) -> Option<Tok<'a>> {
        let bytes = self.bytes();
        let first = *bytes.get(self.pos)?;
        if first != b'<' {
            let end = self.find_byte(self.pos, b'<').unwrap_or(bytes.len());
            let text = self.slice(self.pos, end);
            self.pos = end;
            return Some(Tok::Text(text));
        }
        let after = self.pos.saturating_add(1);
        Some(match bytes.get(after) {
            Some(b'!') => self.declaration(),
            Some(b'?') => {
                self.skip_past_byte(b'>');
                Tok::Skip
            }
            Some(b'/') => self.end_tag(),
            Some(c) if c.is_ascii_alphabetic() => self.start_tag(),
            // A `<` that starts nothing is a character.
            _ => {
                let text = self.slice(self.pos, after);
                self.pos = after;
                Tok::Text(text)
            }
        })
    }

    /// `<!--` up to `-->`, or `<!` up to `>`; with no end, up to the end of the label.
    fn declaration(&mut self) -> Tok<'a> {
        let bytes = self.bytes();
        let open = self.pos.saturating_add(4);
        if bytes.get(self.pos..open) != Some(b"<!--") {
            self.skip_past_byte(b'>');
            return Tok::Skip;
        }
        // `<!-->` and `<!--->` are comments with nothing in them.
        if bytes.get(open) == Some(&b'>') {
            self.pos = open.saturating_add(1);
            return Tok::Skip;
        }
        if bytes.get(open..open.saturating_add(2)) == Some(b"->") {
            self.pos = open.saturating_add(2);
            return Tok::Skip;
        }
        let mut from = open;
        while let Some(dash) = self.find_byte(from, b'-') {
            if bytes.get(dash..dash.saturating_add(3)) == Some(b"-->") {
                self.pos = dash.saturating_add(3);
                return Tok::Skip;
            }
            from = dash.saturating_add(1);
        }
        self.drop_rest()
    }

    /// `</name ...>`.
    fn end_tag(&mut self) -> Tok<'a> {
        let start = self.pos.saturating_add(2);
        let end = run_end(self.bytes(), start, is_name_byte);
        let name = self.slice(start, end);
        match self.find_byte(end, b'>') {
            Some(gt) => {
                self.pos = gt.saturating_add(1);
                if name.is_empty() { Tok::Skip } else { Tok::End(name) }
            }
            None => self.drop_rest(),
        }
    }

    /// `<name attr=value ...>`.
    fn start_tag(&mut self) -> Tok<'a> {
        let bytes = self.bytes();
        let start = self.pos.saturating_add(1);
        let mut i = run_end(bytes, start, is_name_byte);
        self.tag.reset(self.slice(start, i));
        loop {
            i = run_end(bytes, i, is_ws);
            match bytes.get(i) {
                None => return self.drop_rest(),
                Some(b'>') => {
                    i = i.saturating_add(1);
                    break;
                }
                Some(b'/') => {
                    i = i.saturating_add(1);
                    if bytes.get(i) == Some(&b'>') {
                        self.tag.self_close = true;
                        i = i.saturating_add(1);
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            let Some(next) = attribute(self.s, i, &mut self.tag) else { return self.drop_rest() };
            i = next;
        }
        self.pos = i;
        Tok::Start
    }

    /// Moves past the end tag of the raw-text element `name` (`script`, `style`, `iframe`), the content before it taken with it: the first `</name` that ends the name (white
    /// space, `>`, `/` or the end of the label), and then the `>`. With no such end tag, to the end of the label.
    pub(super) fn skip_raw_text(&mut self, name: &str) {
        let bytes = self.bytes();
        let mut from = self.pos;
        while let Some(lt) = self.find_byte(from, b'<') {
            let name_at = lt.saturating_add(2);
            let name_end = name_at.saturating_add(name.len());
            let is_end = bytes.get(lt.saturating_add(1)) == Some(&b'/')
                && bytes.get(name_at..name_end).is_some_and(|w| w.eq_ignore_ascii_case(name.as_bytes()))
                && bytes.get(name_end).is_none_or(|c| is_ws(*c) || *c == b'>' || *c == b'/');
            if is_end {
                self.pos = name_end;
                self.skip_past_byte(b'>');
                return;
            }
            from = lt.saturating_add(1);
        }
        self.pos = bytes.len();
    }
}

/// One attribute whose name starts at `at` in `s`, added to `tag`; the position after it, or `None` when a quote is never closed (the tag has no end).
fn attribute<'a>(s: &'a str, at: usize, tag: &mut Tag<'a>) -> Option<usize> {
    let bytes = s.as_bytes();
    let slice = |a: usize, b: usize| s.get(a..b).unwrap_or("");
    let mut i = run_end(bytes, at, |c| !is_ws(c) && !matches!(c, b'=' | b'>' | b'/'));
    let name = slice(at, i);
    if name.is_empty() {
        // A `=` with no name: passed over.
        return Some(i.saturating_add(1));
    }
    i = run_end(bytes, i, is_ws);
    let mut value = "";
    if bytes.get(i) == Some(&b'=') {
        i = run_end(bytes, i.saturating_add(1), is_ws);
        match bytes.get(i) {
            Some(&quote) if quote == b'"' || quote == b'\'' => {
                let from = i.saturating_add(1);
                let end = from.checked_add(bytes.get(from..)?.iter().position(|b| *b == quote)?)?;
                value = slice(from, end);
                i = end.saturating_add(1);
            }
            _ => {
                let end = run_end(bytes, i, |c| !is_ws(c) && c != b'>');
                value = slice(i, end);
                i = end;
            }
        }
    }
    tag.push(name, value);
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<String> {
        let mut lexer = Lexer::new(s);
        let mut out = Vec::new();
        while let Some(tok) = lexer.next() {
            out.push(match tok {
                Tok::Text(t) => format!("text:{t}"),
                Tok::Start => format!("start:{}{}", lexer.tag().name, if lexer.tag().self_close { "/" } else { "" }),
                Tok::End(n) => format!("end:{n}"),
                Tok::Skip => "skip".to_owned(),
            });
        }
        out
    }

    #[test]
    fn text_tags_and_ends_come_out_in_order() {
        assert_eq!(kinds("a<b>c</b>d<br/>e"), ["text:a", "start:b", "text:c", "end:b", "text:d", "start:br/", "text:e"]);
        assert_eq!(kinds("<DIV Class=x>y</DIV >"), ["start:DIV", "text:y", "end:DIV"]);
    }

    #[test]
    fn a_less_than_that_starts_nothing_is_a_character() {
        assert_eq!(kinds("a < b <3 <"), ["text:a ", "text:<", "text: b ", "text:<", "text:3 ", "text:<"]);
        assert_eq!(kinds("</ x>y"), ["skip", "text:y"], "an end tag with no name is a bogus comment");
    }

    #[test]
    fn comments_declarations_and_instructions_are_skipped() {
        assert_eq!(kinds("a<!-- x -- y -->b<!DOCTYPE html>c<?php x ?>d"), ["text:a", "skip", "text:b", "skip", "text:c", "skip", "text:d"]);
        assert_eq!(kinds("a<!-- never closed <b>x</b>"), ["text:a", "skip"]);
        assert_eq!(kinds("a<!-->b<!--->c<!---->d<!----->e"), ["text:a", "skip", "text:b", "skip", "text:c", "skip", "text:d", "skip", "text:e"]);
    }

    #[test]
    fn a_tag_with_no_end_and_a_quote_never_closed_are_dropped_with_the_rest() {
        assert_eq!(kinds("a<b"), ["text:a", "skip"]);
        assert_eq!(kinds("a<b class=\"x>y"), ["text:a", "skip"]);
        assert_eq!(kinds("a</b"), ["text:a", "skip"]);
        assert_eq!(kinds("a<b c='d'"), ["text:a", "skip"]);
    }

    #[test]
    fn attributes_are_read_with_quotes_without_and_with_no_value() {
        let mut lexer = Lexer::new("<font face = 'A B' size=5 COLOR=\"#fff\" hidden = x/y data-x style>");
        assert!(matches!(lexer.next(), Some(Tok::Start)));
        let tag = lexer.tag();
        assert_eq!(tag.attr("face"), Some("A B"));
        assert_eq!(tag.attr("SIZE"), Some("5"));
        assert_eq!(tag.attr("color"), Some("#fff"));
        assert_eq!(tag.attr("hidden"), Some("x/y"));
        assert_eq!(tag.attr("data-x"), Some(""));
        assert_eq!(tag.attr("style"), Some(""));
        assert_eq!(tag.attr("href"), None);
        assert!(!tag.self_close);
    }

    #[test]
    fn the_first_attribute_of_a_name_wins_and_only_sixteen_are_kept() {
        let mut lexer = Lexer::new("<a x=1 x=2 a3 a4 a5 a6 a7 a8 a9 a10 a11 a12 a13 a14 a15 a16 late=1>");
        assert!(matches!(lexer.next(), Some(Tok::Start)));
        assert_eq!(lexer.tag().attr("x"), Some("1"));
        assert_eq!(lexer.tag().attr("a16"), Some(""));
        assert_eq!(lexer.tag().attr("late"), None, "past the sixteenth");
        assert!(lexer.next().is_none(), "but the tag ended where it ends");
    }

    #[test]
    fn the_tag_the_lexer_keeps_is_the_last_ones_and_holds_nothing_of_the_one_before() {
        let mut lexer = Lexer::new("<a href=1><b>");
        lexer.next();
        assert_eq!(lexer.tag().attr("href"), Some("1"));
        lexer.next();
        assert_eq!((lexer.tag().name, lexer.tag().attr("href")), ("b", None));
    }

    #[test]
    fn a_stray_equals_slash_and_quote_do_not_stop_the_walk() {
        assert_eq!(kinds("<b = x>t</b>"), ["start:b", "text:t", "end:b"]);
        assert_eq!(kinds("<b / x>t"), ["start:b", "text:t"]);
        assert_eq!(kinds("<b a=\"x\"\"y\">t"), ["start:b", "text:t"]);
        assert_eq!(kinds("<br/><br />"), ["start:br/", "start:br/"]);
    }

    #[test]
    fn raw_text_runs_to_its_end_tag_and_to_the_end_without_one() {
        let mut lexer = Lexer::new("<script>x</scriptx>y</SCRIPT >z");
        assert!(matches!(lexer.next(), Some(Tok::Start)));
        lexer.skip_raw_text("script");
        assert!(matches!(lexer.next(), Some(Tok::Text("z"))), "an end tag whose name goes on is not the end");
        let mut lexer = Lexer::new("<script>x</script");
        lexer.next();
        lexer.skip_raw_text("script");
        assert!(lexer.next().is_none());
        let mut lexer = Lexer::new("<script>x</scrip>y");
        lexer.next();
        lexer.skip_raw_text("script");
        assert!(lexer.next().is_none(), "no end tag: the rest goes");
    }

    #[test]
    fn a_hundred_thousand_unclosed_scripts_are_one_walk() {
        // 100,000 `<script` with no `>`: the first tag has no end, so it takes all the rest.
        let s = "<script".repeat(100_000);
        let mut lexer = Lexer::new(&s);
        assert!(matches!(lexer.next(), Some(Tok::Skip)));
        assert!(lexer.next().is_none());
        // And with a `>`: the raw text of the first takes the rest in one search.
        let s = "<script>".repeat(100_000);
        let mut lexer = Lexer::new(&s);
        assert!(matches!(lexer.next(), Some(Tok::Start)));
        lexer.skip_raw_text("script");
        assert!(lexer.next().is_none());
    }

    #[test]
    fn multibyte_text_is_sliced_only_at_the_marks() {
        assert_eq!(kinds("\u{e9}<b>\u{4e2d}</b>\u{1F600}"), ["text:\u{e9}", "start:b", "text:\u{4e2d}", "end:b", "text:\u{1F600}"]);
        assert_eq!(kinds("<b \u{e9}=\u{4e2d}>"), ["start:b"]);
        assert_eq!(kinds("<\u{e9}>"), ["text:<", "text:\u{e9}>"]);
    }
}
