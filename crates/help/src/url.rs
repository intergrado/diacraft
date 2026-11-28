//! The web addresses a help link may open, and the address of a feedback page.
//!
//! A link is allowed when it is one of the host's sites exactly, or one of them followed by `/`, `?` or `#`: nothing between the scheme and the host, no user name, no port
//! (the default one too), no look-alike host that only ends with an allowed name. An address with a space, a control character, a backslash, a character that is not ASCII or
//! an `@` anywhere is refused outright, and so is one with an escape (`%` and two hex digits) that is broken.
//!
//! **The path is read the way a browser reads it.** A browser takes the segments `.` and `..` out of a path before it asks for the address, so a path that holds one can leave
//! the folder the host's site names (`https://example.org/docs/../secret`). No segment of the path may be a dot segment in any spelling: with its dots escaped (`%2e`, `%2E`,
//! mixed), escaped twice or three times (`%252e`), or with parameters after it (`..;x`). A path that holds an escaped slash, backslash or control character is refused, since a
//! server may read it as a separator. The query and the fragment are no part of the path and are left as they are. An address that passes is one that no reader takes anywhere
//! else than where it reads.
//!
//! The address of a feedback page names a repository's `issues/new` page with a body filled in, percent-encoded, at most [`MAX_FEEDBACK_BODY_BYTES`] of it.

use crate::{MAX_FEEDBACK_BODY_BYTES, host};

/// How many times an escaped path segment is unescaped when it is looked at for a dot segment (an address escaped once too many is still looked at).
const DECODE_ROUNDS: usize = 3;

/// Whether `url` is on one of the host's sites (`host::WEB_PREFIXES`).
pub fn allowed(url: &str) -> bool {
    allowed_by(&host::WEB_PREFIXES, url)
}

/// Whether `url` is one of `prefixes` or one of them followed by `/`, `?` or `#`, is plain ASCII with no white space, control character, backslash or `@`, has no broken
/// escape, and has no dot segment, escaped separator or escaped control character in its path.
pub fn allowed_by(prefixes: &[&str], url: &str) -> bool {
    if url.is_empty() || !url.bytes().all(|b| b.is_ascii_graphic()) || url.bytes().any(|b| b == b'\\' || b == b'@') {
        return false;
    }
    prefixes.iter().any(|prefix| url.strip_prefix(prefix).is_some_and(rest_ok))
}

/// What follows an allowed prefix: nothing, a query or a fragment, or a path that is plain.
fn rest_ok(rest: &str) -> bool {
    match rest.bytes().next() {
        None => true,
        Some(b'?' | b'#') => escapes_ok(rest),
        Some(b'/') => {
            let end = rest.find(['?', '#']).unwrap_or(rest.len());
            let (path, tail) = (rest.get(..end).unwrap_or_default(), rest.get(end..).unwrap_or_default());
            path.split('/').all(segment_ok) && escapes_ok(tail)
        }
        Some(_) => false,
    }
}

/// Whether every `%` of `text` is followed by two hex digits.
fn escapes_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| b != b'%' || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)))
}

/// Whether a segment of a path is no dot segment however it is spelled, and holds no escaped separator or control character.
fn segment_ok(segment: &str) -> bool {
    let mut text = segment.as_bytes().to_vec();
    for round in 0..DECODE_ROUNDS {
        // The first round is strict: a broken escape or an escaped separator or control character refuses the segment. Later rounds only look for dots.
        let Some(decoded) = unescape(&text, round == 0) else { return false };
        if decoded.split(|&b| b == b'/' || b == b'\\').any(is_dot_name) {
            return false;
        }
        if decoded == text {
            return true;
        }
        text = decoded;
    }
    // Still being unescaped after the last round: nobody writes a segment escaped so often, and one that is may hide what a reader further on finds.
    false
}

/// Whether a piece of a path is `.` or `..`, with parameters (`;` and what follows) taken off, as a server may read it.
fn is_dot_name(piece: &[u8]) -> bool {
    let name = piece.split(|&b| b == b';').next().unwrap_or_default();
    name == b"." || name == b".."
}

/// `text` with its `%XX` escapes read. In `strict` mode `None` for an escape that is broken or stands for `/`, `\` or a control character; otherwise those stay as they are.
fn unescape(text: &[u8], strict: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0usize;
    while let Some(&b) = text.get(i) {
        if b != b'%' {
            out.push(b);
            i += 1;
            continue;
        }
        let digits = (text.get(i + 1).copied().and_then(hex), text.get(i + 2).copied().and_then(hex));
        match digits {
            (Some(hi), Some(lo)) => {
                let byte = hi * 16 + lo;
                if strict && (byte == b'/' || byte == b'\\' || byte < 0x20 || byte == 0x7f) {
                    return None;
                }
                out.push(byte);
                i += 3;
            }
            _ if strict => return None,
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(out)
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `text` percent-encoded byte by byte (only the unreserved characters `A-Z a-z 0-9 - . _ ~` stay as they are, the rest is `%` and two capital hex digits), at most `max`
/// bytes, cut between characters: a character is encoded whole or not at all.
pub fn encode_body(text: &str, max: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    let mut buf = [0u8; 4];
    for c in text.chars() {
        let mut piece = String::new();
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~') {
            piece.push(c);
        } else {
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                piece.push('%');
                piece.push(char::from(HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0')));
                piece.push(char::from(HEX.get(usize::from(b & 15)).copied().unwrap_or(b'0')));
            }
        }
        if out.len() + piece.len() > max {
            break;
        }
        out.push_str(&piece);
    }
    out
}

/// The address of the new-issue page of `repository` (`owner/name`) with `body` filled in; `None` when `repository` is not of that form.
pub fn feedback_url_for(repository: &str, body: &str) -> Option<String> {
    let (owner, name) = repository.split_once('/')?;
    let word = |s: &str| !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'));
    (word(owner) && word(name)).then(|| format!("https://github.com/{repository}/issues/new?body={}", encode_body(body, MAX_FEEDBACK_BODY_BYTES)))
}

/// The address of the host's feedback page (`host::FEEDBACK_REPOSITORY`) with `body` filled in.
pub fn feedback_url(body: &str) -> String {
    feedback_url_for(host::FEEDBACK_REPOSITORY, body).unwrap_or_else(|| format!("https://github.com/{}/issues/new", host::FEEDBACK_REPOSITORY))
}
