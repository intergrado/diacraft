//! Text from a file or a program, made safe to put in a sentence a person reads.
//!
//! The same rules as `diacraft_engine::shown`, which this crate cannot use (it is below the engine): a control character, a line or paragraph separator and a character that changes
//! the direction text runs in are written as an escape, and the text is cut with `…` at a number of characters. A note names a section, a path or a file name that a hostile
//! file chose, and none of that may reach a status bar or a terminal as it is.

/// Whether `ch` must not reach a message as it is.
fn is_dangerous(ch: char) -> bool {
    ch.is_control()
        || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// How `ch` is written when it is dangerous: `\n`, `\r`, `\t`, `\0`, else `\u{1b}` (lower-case hexadecimal).
fn escape(ch: char) -> String {
    match ch {
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        '\0' => "\\0".to_owned(),
        other => format!("\\u{{{:x}}}", u32::from(other)),
    }
}

/// `text` for a sentence: dangerous characters escaped, cut at `most` characters of the escaped text with `…`. A very long text costs only the characters shown.
pub(crate) fn shown(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut width = 0_usize;
    for ch in text.chars() {
        let piece = if is_dangerous(ch) { escape(ch) } else { ch.to_string() };
        let len = piece.chars().count();
        if width.saturating_add(len) > most {
            out.push('…');
            break;
        }
        out.push_str(&piece);
        width += len;
    }
    out
}
