//! Text from a client, made safe to show.

/// Whether `ch` must not reach a message, a log line, a status bar or a terminal as it is: a control character
/// (a newline, an escape that clears a terminal, NUL, DEL, the C1 range); a line or paragraph separator (U+2028 and
/// U+2029, which are not control characters but end a line all the same, so client text could fake one); or one
/// that changes the direction text runs in and so can make it read as something else (the Arabic letter mark, the
/// left-to-right and right-to-left marks, the embeddings and overrides, the isolates).
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

// How many characters of the text `shown` has looked at, for the test that holds a huge text to a count. Not kept outside tests.
#[cfg(test)]
thread_local! {
    static WALKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `text` for a message, a log line or a window: control, line-separator and direction characters escaped (see
/// above), every other character as it is (an apostrophe, a backslash, an accent made of a combining mark, the joiner
/// in an emoji, other scripts), cut at `most` characters of the escaped text with `…` when there was more.
///
/// Ids, keys, paths and method names come from the control channel and MCP, which are hostile, so what they hold
/// never reaches an error as it is. An escape is kept whole or dropped whole, never cut, and a very long text
/// costs only the characters that are shown. A backslash is not escaped, so `\n` in the output can be a newline
/// that was there or the two characters that were.
pub fn shown(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut width = 0_usize;
    for ch in text.chars() {
        #[cfg(test)]
        WALKED.with(|c| c.set(c.get() + 1));
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

#[cfg(test)]
mod tests {
    use super::{WALKED, shown};

    #[test]
    fn a_huge_text_is_not_walked_to_its_end() {
        // Counted, not timed (a time moves with the load on the machine): the walk stops at the character that does not fit, 65 looks for 64
        // characters shown, whatever the text holds after them.
        let huge = "x".repeat(64 * 1024 * 1024);
        WALKED.with(|c| c.set(0));
        let out = shown(&huge, 64);
        assert_eq!(out.chars().count(), 65);
        assert_eq!(WALKED.with(std::cell::Cell::get), 65, "the characters looked at");
        // And the same with escapes, which are longer than the character they stand for.
        let escapes = "\u{1b}".repeat(1_000_000);
        WALKED.with(|c| c.set(0));
        let _ = shown(&escapes, 64);
        assert_eq!(WALKED.with(std::cell::Cell::get), 11, "ten escapes of six characters fit in 64, and the eleventh does not");
    }
}
