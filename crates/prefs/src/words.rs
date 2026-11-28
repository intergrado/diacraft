//! The text format of a custom dictionary, and the edits that keep two programs' words (design 6.2, design call).
//!
//! **The file** is UTF-8 text, one word to a line, a line that starts with `#` a comment; UTF-8 with a byte order mark and UTF-16 with one are read too, so that a file another program
//! made can be attached, and a file edited here is written as UTF-8. A line that is not a word (a space, a control character, an invisible mark or a noncharacter in it, more than 64
//! characters) is skipped, counted and **kept as it was** when the file is written again. Caps: 1,000,000 bytes, 100,000 words, 64 characters a word.
//!
//! **Words are matched by their lower-case form.** craft-proof matches by lower case and composed (NFC) form; this crate has no table of compositions, so two spellings of one
//! accented word differ here and both are kept, which the checker then treats as one.
//!
//! The checker's own reading of the file (`diacraft-proof`'s dictionary list) takes the same format.

use std::collections::HashSet;

use thiserror::Error;

use crate::shown;

/// The most bytes of a word list.
pub const MAX_DICT_FILE_BYTES: usize = 1_000_000;

/// The most words of a word list.
pub const MAX_WORDS: usize = 100_000;

/// The most characters of a word (craft-proof's `MAX_CUSTOM_WORD_CHARS`).
pub const MAX_WORD_CHARS: usize = 64;

/// Why a word list or a word is refused. Each says it in words for the person.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum WordError {
    #[error("the file is not UTF-8 or UTF-16 text")]
    NotText,
    #[error("the file is more than {} bytes", commas(*.0))]
    TooBig(usize),
    #[error("the list would have more than {} words", commas(MAX_WORDS))]
    TooMany,
    #[error("a word cannot be empty")]
    Empty,
    #[error("a word is at most {MAX_WORD_CHARS} characters")]
    TooLong,
    #[error("`{0}` is not one word: a word has no space, control character or invisible mark")]
    NotAWord(String),
}

/// `n` with a comma between thousands.
fn commas(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A line of a word list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    Word(String),
    /// A comment, a blank line, or a line that is not a word, as it was.
    Other(String),
}

/// A word list read: its lines, in order.
#[derive(Clone, Debug, Default)]
pub struct WordFile {
    lines: Vec<Line>,
    /// The lower-case form of each word, for the test of a word already there.
    keys: HashSet<String>,
    skipped: usize,
    /// The bytes of the list as [`WordFile::render`] writes it.
    bytes: usize,
}

/// Whether `c` is an invisible or direction mark a word never holds: U+200B to U+200F, U+202A to U+202E, U+2066 to U+2069, U+FEFF, and the noncharacters
/// U+FDD0 to U+FDEF and U+FFFE and U+FFFF at the end of every plane. They draw nothing, a U+FEFF written first would be read as the file's byte order mark and
/// the word lost, and a U+FFFF written after one came back as a word the first read never held.
fn is_hidden(c: char) -> bool {
    let n = c as u32;
    c.is_control()
        || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
        || matches!(n, 0xfdd0..=0xfdef)
        || (n & 0xfffe) == 0xfffe
}

/// `text` if it is one word: not empty, at most 64 characters, no space, control character, invisible mark or noncharacter.
pub fn check_word(text: &str) -> Result<&str, WordError> {
    let word = text.trim();
    if word.is_empty() {
        return Err(WordError::Empty);
    }
    if word.chars().any(|c| c.is_whitespace() || is_hidden(c)) {
        return Err(WordError::NotAWord(shown(word, 40)));
    }
    if word.chars().count() > MAX_WORD_CHARS {
        return Err(WordError::TooLong);
    }
    Ok(word)
}

/// The text of a file's bytes: UTF-8 (with or without a byte order mark) or UTF-16 with one.
fn decode(bytes: &[u8]) -> Result<String, WordError> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(rest).map(str::to_owned).map_err(|_| WordError::NotText);
    }
    let units = |rest: &[u8], le: bool| -> Result<String, WordError> {
        // An odd byte at the end is not a code unit.
        let (pairs, odd) = rest.as_chunks::<2>();
        if !odd.is_empty() {
            return Err(WordError::NotText);
        }
        let units = pairs.iter().map(|pair| if le { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) });
        char::decode_utf16(units).collect::<Result<String, _>>().map_err(|_| WordError::NotText)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return units(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return units(rest, false);
    }
    std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| WordError::NotText)
}

/// Reads a word list. A file past a cap, or that is not text, is refused and the caller leaves it as it is.
pub fn read(bytes: &[u8]) -> Result<WordFile, WordError> {
    if bytes.len() > MAX_DICT_FILE_BYTES {
        return Err(WordError::TooBig(MAX_DICT_FILE_BYTES));
    }
    let text = decode(bytes)?;
    // A U+FEFF at the very start is a byte order mark too, whether it follows a UTF-16 byte order mark or is a second one in UTF-8: kept as a line it would be
    // written first and read back as a marker, so the line would lose it.
    let text = text.trim_start_matches('\u{feff}');
    let mut file = WordFile::default();
    if text.is_empty() {
        return Ok(file);
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    for raw in body.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            file.push(Line::Other(String::new()));
        } else if trimmed.starts_with('#') {
            file.push(Line::Other(trimmed.to_owned()));
        } else if check_word(trimmed).is_ok() {
            if file.keys.len() >= MAX_WORDS && !file.keys.contains(&trimmed.to_lowercase()) {
                return Err(WordError::TooMany);
            }
            file.keys.insert(trimmed.to_lowercase());
            file.push(Line::Word(trimmed.to_owned()));
        } else {
            file.skipped += 1;
            // Without what trails it (a second carriage return, a space): written again it reads as the same line.
            file.push(Line::Other(line.trim_end().to_owned()));
        }
    }
    Ok(file)
}

impl Line {
    fn text(&self) -> &str {
        let (Line::Word(text) | Line::Other(text)) = self;
        text
    }
}

impl WordFile {
    fn push(&mut self, line: Line) {
        self.bytes = self.bytes.saturating_add(line.text().len()).saturating_add(1);
        self.lines.push(line);
    }

    /// The words, in order.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().filter_map(|line| match line {
            Line::Word(word) => Some(word.as_str()),
            Line::Other(_) => None,
        })
    }

    /// How many lines were not a word, a comment or blank.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Whether `word` is in the list, in any case.
    pub fn contains(&self, word: &str) -> bool {
        self.keys.contains(&word.trim().to_lowercase())
    }

    /// Adds `word` at the end. `Ok(false)` when it is there already, in any case. A word that is not one word, a full list and a full file refuse.
    pub fn add(&mut self, word: &str) -> Result<bool, WordError> {
        let word = check_word(word)?;
        if self.contains(word) {
            return Ok(false);
        }
        if self.keys.len() >= MAX_WORDS {
            return Err(WordError::TooMany);
        }
        if self.bytes.saturating_add(word.len()).saturating_add(1) > MAX_DICT_FILE_BYTES {
            return Err(WordError::TooBig(MAX_DICT_FILE_BYTES));
        }
        self.keys.insert(word.to_lowercase());
        self.push(Line::Word(word.to_owned()));
        Ok(true)
    }

    /// Removes `word`, in any case. Whether it was there.
    pub fn remove(&mut self, word: &str) -> bool {
        let key = word.trim().to_lowercase();
        if !self.keys.remove(&key) {
            return false;
        }
        self.lines.retain(|line| !matches!(line, Line::Word(w) if w.to_lowercase() == key));
        self.bytes = self.lines.iter().map(|line| line.text().len().saturating_add(1)).sum();
        true
    }

    /// The list as UTF-8 text, a line each, ending with a newline when there is any line.
    pub fn render(&self) -> Vec<u8> {
        let mut out = String::with_capacity(self.bytes);
        for line in &self.lines {
            out.push_str(line.text());
            out.push('\n');
        }
        out.into_bytes()
    }
}
