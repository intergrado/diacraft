//! The splice: a part's bytes with a list of edits applied, every other byte copied as it was (the design's "Saving": a save splices only the changed elements into the source bytes and never writes a whole part again).
//!
//! An edit replaces the bytes from `start` to `end` of the source with text, takes them out, or puts text at a place; a **move** takes a
//! *unit* (an element and the white space before it) out where it is and puts it where it goes, carrying every edit made inside it. The edits
//! are applied in one sweep of the source in order of place: each byte is looked at once, and copied once, so **the work is linear in the part**
//! and in the text the edits add, and it does not depend on where the edits are: an edit at the start of a part costs the part's length to
//! copy after it, and an edit at its end costs the same. Each copy is counted ([`WriteWork::bytes_copied`]) and charged against
//! [`Limits::max_write_bytes`] before it is made.
//!
//! **What an edit may not do**: overlap another (two edits that change the same byte are a fault of the caller, and refuse the save), or reach
//! outside the source. The edits made inside a unit that is moved belong to its payload and may lie inside the place it is taken out of. The
//! sweep of a payload is recursive for a moved unit inside a moved unit, which is no deeper than the elements nest.

use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::names::Ns;
use crate::write::own::static_local;

/// The deepest a moved unit inside a moved unit is followed: the container's depth cap with room to spare.
const MOST_NESTING: usize = 256;

/// What an edit does at its place.
enum What {
    /// Puts this text where the bytes were (nothing for a removal).
    Text(String),
    /// Takes a unit out: the edits inside it are its payload's, and the sweep skips them here.
    MoveOut,
    /// Puts a unit, with the edits made inside it, here.
    MoveIn(usize),
}

struct Edit {
    start: usize,
    end: usize,
    what: What,
    /// The order the edit was made in, to keep edits at one place in that order.
    seq: usize,
    /// The edit goes before the others at its place (`insert_first`).
    first: bool,
}

/// An attribute the writer wrote on a start tag: where the tag is in the part the edits were made to, and which attribute (ODF's namespace and
/// local name). What the check of the bytes made reads back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WrittenAttr {
    pub tag_start: usize,
    pub ns: Ns,
    pub local: &'static str,
}

/// A part's bytes and the edits to make in them. See the [module documentation](self).
pub(crate) struct Splicer<'a> {
    source: &'a [u8],
    edits: Vec<Edit>,
    units: Vec<(usize, usize)>,
    /// The attributes written, for the check of the part made.
    pub(crate) written: Vec<WrittenAttr>,
    /// An edit or a note could not be kept for want of memory: [`Splicer::apply`] refuses.
    starved: bool,
}

impl<'a> Splicer<'a> {
    pub(crate) fn new(source: &'a [u8]) -> Splicer<'a> {
        Splicer { source, edits: Vec::new(), units: Vec::new(), written: Vec::new(), starved: false }
    }

    /// An edit is kept by `try_reserve`, never by the abort a `Vec` makes: memory that is not had is noted, and [`Splicer::apply`] refuses.
    fn push(&mut self, start: usize, end: usize, what: What) {
        self.push_at(start, end, what, false);
    }

    fn push_at(&mut self, start: usize, end: usize, what: What, first: bool) {
        let seq = self.edits.len();
        if self.edits.try_reserve(1).is_err() {
            self.starved = true;
            return;
        }
        self.edits.push(Edit { start, end, what, seq, first });
    }

    /// Notes that the attribute `local` of `ns` was written on the start tag at `tag_start` of the source.
    pub(crate) fn note_written(&mut self, tag_start: usize, ns: Ns, local: &str) {
        // Only an attribute the table owns is ever written, so every one has a name in the table.
        let Some(local) = static_local(local) else { return };
        if self.written.try_reserve(1).is_err() {
            self.starved = true;
            return;
        }
        self.written.push(WrittenAttr { tag_start, ns, local });
    }

    /// How many edits are waiting: what a caller that writes some may compare before and after, to tell whether it changed anything.
    pub(crate) fn edits_len(&self) -> usize {
        self.edits.len()
    }

    /// Replaces the bytes from `start` to `end` with `text`.
    pub(crate) fn replace(&mut self, start: usize, end: usize, text: String) {
        self.push(start, end, What::Text(text));
    }

    /// Takes the bytes from `start` to `end` out.
    pub(crate) fn remove(&mut self, start: usize, end: usize) {
        self.push(start, end, What::Text(String::new()));
    }

    /// Puts `text` at `at`, after any text put there before.
    pub(crate) fn insert(&mut self, at: usize, text: String) {
        self.push(at, at, What::Text(text));
    }

    /// Puts `text` at `at`, **before** any text put there by [`Splicer::insert`] or any other edit made before it: a declaration that must come first where
    /// something else is put too (a flat drawing's `office:font-face-decls`, which stands before the `office:automatic-styles` a save makes at the same place).
    pub(crate) fn insert_first(&mut self, at: usize, text: String) {
        self.push_at(at, at, What::Text(text), true);
    }

    /// Takes the unit from `unit.0` to `unit.1` out of its place and puts it at `to`, with the edits made inside it.
    pub(crate) fn move_unit(&mut self, unit: (usize, usize), to: usize) {
        let id = self.units.len();
        self.units.push(unit);
        self.push(unit.0, unit.1, What::MoveOut);
        self.push(to, to, What::MoveIn(id));
    }

    /// The part with every edit made. Refuses edits that overlap or reach outside the source, and a part past the write cap, with nothing
    /// made; counts every byte copied and every edit.
    pub(crate) fn apply(mut self, work: &mut WriteWork, limits: &Limits) -> Result<Vec<u8>, WriteError> {
        work.check_steps()?;
        if self.starved {
            return Err(WriteError::Limit { what: "memory for the edits to a part", max: self.edits.len() as u64 });
        }
        work.edits = work.edits.saturating_add(self.edits.len() as u64);
        if self.edits.len() > limits.max_write_edits {
            return Err(WriteError::Limit { what: "edits to one part", max: limits.max_write_edits as u64 });
        }
        self.edits.sort_by_key(|e| (e.start, e.end, !e.first, e.seq));
        if self.edits.iter().any(|e| e.start > e.end || e.end > self.source.len()) {
            return Err(WriteError::Mismatch("an edit reaches outside the part"));
        }
        let mut out = Vec::new();
        out.try_reserve(self.source.len()).map_err(|_| WriteError::Limit { what: "memory for a part", max: self.source.len() as u64 })?;
        let sweep = Sweep { source: self.source, edits: &self.edits, units: &self.units, limits };
        sweep.run(0, self.source.len(), false, 0, &mut out, work)?;
        Ok(out)
    }
}

/// One pass over the source, or over a unit of it.
struct Sweep<'a> {
    source: &'a [u8],
    edits: &'a [Edit],
    units: &'a [(usize, usize)],
    limits: &'a Limits,
}

impl Sweep<'_> {
    /// Copies the source from `lo` to `hi` into `out` with the edits made in it: those that start at `lo` or `hi` too, unless `inside`, which
    /// is a unit's payload (its edits start strictly inside it: an edit at its edges is another place's).
    fn run(&self, lo: usize, hi: usize, inside: bool, depth: usize, out: &mut Vec<u8>, work: &mut WriteWork) -> Result<(), WriteError> {
        if depth > MOST_NESTING {
            return Err(WriteError::Mismatch("moved units nest deeper than elements do"));
        }
        let mut pos = lo;
        let mut at = if inside { self.edits.partition_point(|e| e.start <= lo) } else { self.edits.partition_point(|e| e.start < lo) };
        while let Some(edit) = self.edits.get(at) {
            if if inside { edit.start >= hi } else { edit.start > hi } {
                break;
            }
            if edit.start < pos {
                return Err(WriteError::Mismatch("two edits change the same bytes"));
            }
            self.copy(pos, edit.start, out, work)?;
            pos = edit.start;
            at += 1;
            match &edit.what {
                What::Text(text) => {
                    // Whatever the edit puts in is well-formed: a name or a text that is not is refused here, where every edit passes.
                    if has_bad_char(text) {
                        return Err(WriteError::Unsupported(WORDS_XML_CHAR));
                    }
                    work.charge_copy(text.len(), self.limits)?;
                    work.bytes_written = work.bytes_written.saturating_add(text.len() as u64);
                    put(out, text.as_bytes())?;
                    pos = edit.end;
                }
                What::MoveOut => {
                    pos = edit.end;
                }
                What::MoveIn(id) => {
                    let &(from, to) = self.units.get(*id).ok_or(WriteError::Mismatch("an edit names a unit that is not there"))?;
                    self.run(from, to, true, depth + 1, out, work)?;
                }
            }
            // The edits inside a unit that moved are its payload's, and are skipped here; inside any other span an edit replaced they are an
            // overlap, which the head of the loop refuses.
            if matches!(edit.what, What::MoveOut) {
                while self.edits.get(at).is_some_and(|e| e.start < pos) {
                    at += 1;
                }
            }
        }
        self.copy(pos, hi, out, work)
    }

    /// Copies `from..to` of the source, counted and charged first.
    fn copy(&self, from: usize, to: usize, out: &mut Vec<u8>, work: &mut WriteWork) -> Result<(), WriteError> {
        let Some(slice) = self.source.get(from..to) else { return Ok(()) };
        work.charge_copy(slice.len(), self.limits)?;
        put(out, slice)
    }
}

/// Appends `bytes`, an error and not an abort when the memory is not had.
fn put(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), WriteError> {
    out.try_reserve(bytes.len()).map_err(|_| WriteError::Limit { what: "memory for a part", max: bytes.len() as u64 })?;
    out.extend_from_slice(bytes);
    Ok(())
}

/// The words of a refusal of a name or text that holds a character XML 1.0 does not allow (such a character was written as it
/// was, and the file no longer opened).
pub(crate) const WORDS_XML_CHAR: &str = "a name or text with a character XML 1.0 does not allow (a control character other than tab, line feed and carriage return, or U+FFFE or U+FFFF): the file would not open";

/// The words of a refusal of a text with a carriage return: a reader takes a literal one for a line feed and a referenced one for white space, so
/// neither is the character the text had.
pub(crate) const WORDS_CARRIAGE_RETURN: &str = "a text with a carriage return (the file reads it as white space; a new line is a line feed)";

/// Whether `text` holds a character XML 1.0 does not allow, by the rule the reader's own parser applies.
pub(crate) fn has_bad_char(text: &str) -> bool {
    diacraft_container::xml::first_bad_char(text).is_some()
}

/// What a character of an attribute value is written as in `quote`'s quotes, when it is not itself.
pub(crate) fn escaped(c: char, quote: u8) -> Option<&'static str> {
    match c {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '"' if quote == b'"' => Some("&quot;"),
        '\'' if quote == b'\'' => Some("&apos;"),
        '\n' => Some("&#10;"),
        '\r' => Some("&#13;"),
        '\t' => Some("&#9;"),
        _ => None,
    }
}

/// Text as an attribute value in double quotes: `&`, `<` and `"` escaped, a line feed, a carriage return and a tab as references (a literal one
/// would read back as a space).
pub(crate) fn escape_value(text: &str, quote: u8) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match escaped(c, quote) {
            Some(reference) => out.push_str(reference),
            None => out.push(c),
        }
    }
    out
}

/// The bytes [`escape_value`] writes for `text`, counted without making them: what a value costs the memory a save holds before it is made.
pub(crate) fn escaped_len(text: &str, quote: u8) -> u64 {
    text.chars().fold(0u64, |sum, c| sum.saturating_add(escaped(c, quote).map_or(c.len_utf8(), str::len) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, build: impl FnOnce(&mut Splicer<'_>)) -> String {
        let mut s = Splicer::new(source.as_bytes());
        build(&mut s);
        let mut work = WriteWork::default();
        String::from_utf8(s.apply(&mut work, &Limits::default()).unwrap()).unwrap()
    }

    #[test]
    fn edits_are_applied_in_the_order_of_their_place_and_the_rest_is_copied() {
        let out = run("0123456789", |s| {
            s.replace(6, 8, "XY".to_owned());
            s.remove(1, 3);
            s.insert(10, "!".to_owned());
            s.insert(0, "<".to_owned());
            s.insert(0, "<".to_owned());
        });
        assert_eq!(out, "<<0345XY89!");
    }

    #[test]
    fn an_insertion_that_is_first_goes_before_the_others_at_its_place() {
        let out = run("0123", |s| {
            s.insert(2, "a".to_owned());
            s.insert(2, "b".to_owned());
            s.insert_first(2, "F".to_owned());
            s.insert_first(2, "G".to_owned());
            s.insert(0, "<".to_owned());
        });
        assert_eq!(out, "<01FGab23", "the first ones in the order they were made, then the rest in theirs");
    }

    #[test]
    fn a_unit_moves_with_the_edits_made_inside_it() {
        // Units `[a]` at 1..4 and `[b]` at 4..7: `[a]` goes after `[b]`, and an edit inside `[a]` goes with it.
        let out = run("<[a][b]>", |s| {
            s.replace(2, 3, "A".to_owned());
            s.move_unit((1, 4), 7);
        });
        assert_eq!(out, "<[b][A]>");
        // An edit at the edge of the unit is another place's: one at its end stays where the unit was, one at its start does as well.
        let out = run("<[a][b]>", |s| {
            s.insert(4, "!".to_owned());
            s.insert(1, "?".to_owned());
            s.move_unit((1, 4), 7);
        });
        assert_eq!(out, "<?![b][a]>");
        // A unit inside a unit that moves, moved as well.
        let out = run("<{[a][b]}[c]>", |s| {
            s.move_unit((2, 5), 8);
            s.move_unit((1, 9), 12);
        });
        assert_eq!(out, "<[c]{[b][a]}>");
    }

    #[test]
    fn overlapping_edits_and_edits_outside_the_part_are_refused() {
        let refused = |build: &dyn Fn(&mut Splicer<'_>)| {
            let mut s = Splicer::new(b"0123456789");
            build(&mut s);
            s.apply(&mut WriteWork::default(), &Limits::default()).is_err()
        };
        assert!(refused(&|s| {
            s.replace(1, 5, String::new());
            s.replace(3, 6, String::new());
        }));
        assert!(refused(&|s| {
            s.remove(1, 5);
            s.insert(3, "x".to_owned());
        }));
        assert!(refused(&|s| s.remove(8, 12)));
    }

    #[test]
    fn the_edits_to_a_part_are_held_to_the_cap_at_exactly_the_cap() {
        let with = |cap: usize| {
            let mut s = Splicer::new(b"0123456789");
            s.replace(0, 1, "a".to_owned());
            s.replace(2, 3, "b".to_owned());
            s.insert(5, "c".to_owned());
            let mut work = WriteWork::default();
            let result = s.apply(&mut work, &Limits { max_write_edits: cap, ..Limits::default() });
            (result.map(|bytes| bytes.len()), work.edits)
        };
        assert_eq!(with(3), (Ok(11), 3), "three edits at a cap of three");
        assert!(matches!(with(2).0, Err(WriteError::Limit { max: 2, .. })));
        assert_eq!(with(2).1, 3, "the edits are counted whether the part is made or not");
    }

    #[test]
    fn units_moved_into_units_are_followed_to_the_depth_elements_nest_and_no_further() {
        // `n` units of three bytes each, the first moved to the end of the part and each of the others moved into the one before it: a chain `n`
        // deep, each followed by a pass of its own.
        let chain = |n: usize| {
            let source = vec![b'x'; 3 * n];
            let mut s = Splicer::new(&source);
            s.move_unit((0, 3), 3 * n);
            for k in 1..n {
                s.move_unit((3 * k, 3 * k + 3), 3 * (k - 1) + 1);
            }
            s.apply(&mut WriteWork::default(), &Limits::default()).map(|out| out.len())
        };
        assert_eq!(chain(MOST_NESTING), Ok(3 * MOST_NESTING), "a chain as deep as the limit");
        assert!(matches!(chain(MOST_NESTING + 2), Err(WriteError::Mismatch(_))), "past it");
    }

    #[test]
    fn values_are_escaped_for_their_quote() {
        assert_eq!(escape_value("a&b<c\"d'e\nf", b'"'), "a&amp;b&lt;c&quot;d'e&#10;f");
        assert_eq!(escape_value("a\"b'c", b'\''), "a\"b&apos;c");
    }

    /// The bytes a value costs the memory a save holds are counted without making them: the count is the length of the text
    /// [`escape_value`] makes, for every character it writes another way, in either quote, and for those that are one byte and those that are more.
    #[test]
    fn the_length_of_an_escaped_value_is_counted_as_it_is_made() {
        for text in ["", "plain", "a&b", "<<", "\"", "'", "\n", "\r", "\t", "a\r\nb", "M 1 2\n v 3\t&<\"'", "caf\u{e9} \u{4e2d}\u{1F600} \n"] {
            for quote in *b"\"'" {
                assert_eq!(escaped_len(text, quote), escape_value(text, quote).len() as u64, "{text:?} in {}", quote as char);
            }
        }
        // The quote that is not the value's own costs nothing more, and one line feed is five bytes, a tab four.
        assert_eq!(escaped_len("\"", b'\''), 1);
        assert_eq!(escaped_len("\"", b'"'), 6);
        assert_eq!(escaped_len("\n", b'"'), 5);
        assert_eq!(escaped_len("\t", b'"'), 4);
        assert_eq!(escaped_len(&"\n".repeat(1000), b'"'), 5000);
    }
}
