//! Folding text for matching, and cutting a query.
//!
//! Two texts match when their folded forms do: lowercase by the language's own rule, decomposed (the compatibility decomposition, so that a full-width letter or a ligature
//! is the plain letters), the combining marks dropped (so `ecran` finds `Écran`), the invisible format characters dropped, and white space collapsed to single spaces. The
//! folding is deliberately coarse: it only ever adds matches. A *word* of a folded text is a run of letters and digits, and a query word matches a word that it is the start
//! of; in a language written with no spaces (the characters of Han, Hiragana and Katakana) every character also starts a word, so a query finds a text from any of its
//! characters.
//!
//! The pairs of the Turkish i are copied from WordCraft (see `docs/provenance.md`): `İ` with `i` and `I` with `ı`, which Unicode's default casing would turn into `i` and
//! a combining dot, and `ı`.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::{MAX_QUERY_CHARS, MAX_QUERY_WORDS};

/// How letters are lowercased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Casing {
    /// Unicode's default.
    Standard,
    /// Turkish and Azerbaijani: two pairs of i (`İ` with `i`, `I` with `ı`).
    Turkic,
}

impl Casing {
    /// The casing of a language tag (`tr`, `az-Latn`, `tr_TR`): the primary subtag decides.
    pub fn for_language(tag: &str) -> Casing {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("tr") || primary.eq_ignore_ascii_case("az") { Casing::Turkic } else { Casing::Standard }
    }
}

/// `s` folded: lowercased by `casing`, decomposed, the combining marks and the invisible format characters dropped, white space collapsed and trimmed.
pub fn fold(s: &str, casing: Casing) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    let turkic = casing == Casing::Turkic;
    for c in s.chars().map(|c| if turkic { turkic_pair(c) } else { c }).nfkd() {
        for lower in c.to_lowercase() {
            if is_combining_mark(lower) || is_invisible(lower) {
                continue;
            }
            if lower.is_whitespace() {
                pending_space = !out.is_empty();
                continue;
            }
            if std::mem::take(&mut pending_space) {
                out.push(' ');
            }
            out.push(lower);
        }
    }
    out
}

/// The Turkish pairs: `İ` is `i` and `I` is `ı`.
fn turkic_pair(c: char) -> char {
    match c {
        'İ' => 'i',
        'I' => 'ı',
        other => other,
    }
}

/// A character that shows nothing and is not a separator: a soft hyphen, zero-width and direction marks, a word joiner, the byte order mark.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// The longest prefix of `s` of at most `max_chars` characters (it ends on a character boundary).
pub fn cut_query(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => s.get(..i).unwrap_or(s),
        None => s,
    }
}

/// The words of a folded text: its runs of letters and digits.
pub fn words(folded: &str) -> impl Iterator<Item = &str> {
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

/// The words of a query: cut at the character cap, folded, and no more than the word cap.
pub fn query_words(query: &str, casing: Casing) -> Vec<String> {
    let folded = fold(cut_query(query, MAX_QUERY_CHARS), casing);
    words(&folded).take(MAX_QUERY_WORDS).map(str::to_owned).collect()
}

/// Whether `query` is the start of `word` (both folded), or of a part of it that starts at a character of a language with no spaces.
pub fn starts_word(word: &str, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    word.char_indices().any(|(i, c)| (i == 0 || is_spaceless(c)) && word.get(i..).is_some_and(|tail| tail.starts_with(query)))
}

/// The characters of Han, Hiragana and Katakana, which start a word each.
fn is_spaceless(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{3134F}')
}
