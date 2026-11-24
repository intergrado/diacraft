//! Part names and the targets of relationships.
//!
//! A part name is `/` and then non-empty segments joined by `/`, as ECMA-376 Part 2 section 6.2.2 has it. Here it is always held
//! **decoded**: R4 says the ZIP item name has its non-ASCII characters percent-encoded and that "on read, those are decoded and the
//! slash added back", so `visio/media/caf%C3%A9.png` is the part `/visio/media/café.png`. Two names are the same part when they are
//! equal after ASCII case is folded ([`fold`]); the case a part arrived with is the case it keeps.
//!
//! Nothing here touches a file system, and nothing a name says can: a name is a key into the package's own map. The checks are
//! about what a *writer* or another reader could be made to do with a hostile name (`path_traversal`), and about keeping
//! two different names from being one.

use crate::content_types::CONTENT_TYPES_ITEM;
use crate::error::OpcError;
use crate::limits::MAX_PART_NAME_BYTES;

/// The key two names are compared by: with its leading slash (added when the name has none) and ASCII letters lowered.
/// Letters outside ASCII are not folded: `É` and `é` are different parts.
pub fn fold(name: &str) -> String {
    let mut key = String::with_capacity(name.len().saturating_add(1));
    if !name.starts_with('/') {
        key.push('/');
    }
    key.push_str(name);
    key.make_ascii_lowercase();
    key
}

/// What decoding the percent-escapes of a name gave.
pub(crate) enum Decoded {
    Text(String),
    /// The text, and at least one escape spelled an unreserved character (a letter, a digit, `-`, `.`, `_` or `~`), which R3 forbids in
    /// a part name: `%41` for `A`, `%2e` for a dot.
    Unreserved(String),
    /// An escape that spells `/` or `\`: a separator smuggled past a check that looked at the encoded form.
    Smuggled,
    /// The escapes do not make UTF-8. The text is the bytes with each bad sequence replaced by U+FFFD: only for a look at the slashes,
    /// dots and colons in it, which are the same bytes.
    NotUtf8(String),
}

/// Decodes `%XX` escapes. A `%` that does not start two hex digits is only a `%`: the reader does not guess at what was meant.
pub(crate) fn percent_decode(s: &str) -> Decoded {
    if !s.contains('%') {
        return Decoded::Text(s.to_owned());
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut smuggled = false;
    let mut unreserved = false;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let (Some(&h), Some(&l)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let (Some(h), Some(l)) = ((h as char).to_digit(16), (l as char).to_digit(16))
        {
            // Two hex digits make a value below 256.
            let value = (h * 16 + l) as u8;
            smuggled |= value == b'/' || value == b'\\';
            unreserved |= value.is_ascii_alphanumeric() || matches!(value, b'-' | b'.' | b'_' | b'~');
            out.push(value);
            i += 3;
            continue;
        }
        out.push(b);
        i += 1;
    }
    if smuggled {
        return Decoded::Smuggled;
    }
    match String::from_utf8(out) {
        Ok(text) if unreserved => Decoded::Unreserved(text),
        Ok(text) => Decoded::Text(text),
        Err(e) => Decoded::NotUtf8(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

/// The text with its escapes decoded when they decode, and as it is when they do not. For names that are only looked up (a row of
/// the content types), where a name that cannot be a part simply matches none.
pub fn lenient_decode(s: &str) -> String {
    match percent_decode(s) {
        Decoded::Text(t) | Decoded::Unreserved(t) => t,
        Decoded::Smuggled | Decoded::NotUtf8(_) => s.to_owned(),
    }
}

/// Whether a name with its escapes decoded leaves the package (`path_traversal`): a backslash, a leading slash, a drive
/// letter, or a `..` segment.
fn decoded_is_traversal(decoded: &str) -> bool {
    let bytes = decoded.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    decoded.contains('\\') || decoded.starts_with('/') || drive || decoded.split('/').any(|s| s == "..")
}

/// A segment of a part name is legal when it is not empty and does not end in a dot. `.` and `..` end in a dot.
fn segment_is_legal(segment: &str) -> bool {
    !segment.is_empty() && !segment.ends_with('.')
}

/// The part name for a ZIP item name, or the reason it is not one. `item` has no trailing slash: a directory item's name is
/// judged as the folder it names.
///
/// Refused as `path_traversal`: a backslash, a leading slash, a drive letter, a `..` segment, and an escape that spells a slash
/// or a backslash. Refused as `invalid_part_name`: nothing at all, an empty segment, a segment that ends in a dot, a control
/// character, escapes that are not UTF-8 (when they hold no traversal), escapes of unreserved characters, the name of the content types stream, more than
/// `max_len` bytes once decoded.
pub(crate) fn from_item_name(item: &str, max_len: usize) -> Result<String, OpcError> {
    let traversal = || OpcError::PathTraversal { name: item.to_owned() };
    let invalid = |reason: &'static str| OpcError::InvalidPartName { name: item.to_owned(), reason };
    let (decoded, escaped_unreserved) = match percent_decode(item) {
        Decoded::Text(t) => (t, false),
        Decoded::Unreserved(t) => (t, true),
        Decoded::Smuggled => return Err(traversal()),
        // The bytes are read for a traversal all the same: `%FF/../x` is one, whatever else it is.
        Decoded::NotUtf8(t) if decoded_is_traversal(&t) => return Err(traversal()),
        Decoded::NotUtf8(_) => return Err(invalid("percent-encoded bytes that are not UTF-8")),
    };
    // What leaves the package, first: a name with both a traversal and another fault is a traversal.
    if decoded_is_traversal(&decoded) {
        return Err(traversal());
    }
    // A part name must not percent-encode an unreserved character. Judged after the traversal check, so that `%2e%2e/` stays one.
    if escaped_unreserved {
        return Err(invalid("a percent-escape of a letter, a digit, or one of - . _ ~"));
    }
    if decoded.is_empty() {
        return Err(invalid("empty"));
    }
    // The content types stream is not a part. The stream itself is told by its name as written, before this; a name that only
    // *decodes* to it (`%5BContent_Types%5D.xml`) would be a second item with the stream's name once written back.
    if decoded.eq_ignore_ascii_case(CONTENT_TYPES_ITEM) {
        return Err(invalid("the name of the content types stream, which is not a part"));
    }
    if decoded.len() > max_len {
        return Err(invalid("longer than the limit"));
    }
    if decoded.chars().any(char::is_control) {
        return Err(invalid("a control character"));
    }
    if !decoded.split('/').all(segment_is_legal) {
        return Err(invalid("an empty segment, or one that ends in a dot"));
    }
    Ok(format!("/{decoded}"))
}

/// Judges the name of a folder row, without its trailing slash. A folder is no part, so the rule that a segment must not end in a dot does
/// not bind it, and a `.` segment is the folder one is in and no traversal: a folder called `./` is read, and the caller says so in a
/// warning (two real files from one source have one). Every other rule holds: a `..` segment is still `path_traversal`,
/// and `a/./b` is judged as `a/b`.
pub(crate) fn check_folder_name(item: &str, max_len: usize) -> Result<(), OpcError> {
    if !item.split('/').any(|segment| segment == ".") {
        return from_item_name(item, max_len).map(|_| ());
    }
    let rest: Vec<&str> = item.split('/').filter(|segment| *segment != ".").collect();
    if rest.is_empty() {
        return Ok(());
    }
    from_item_name(&rest.join("/"), max_len).map(|_| ())
}

/// Whether a part name (with its leading slash) is a relationships part: `.rels` in a folder called `_rels`.
pub fn is_rels_part(name: &str) -> bool {
    let mut segments = name.rsplit('/');
    let file = segments.next().unwrap_or("");
    let folder = segments.next().unwrap_or("");
    folder.eq_ignore_ascii_case("_rels")
        && file.len() >= ".rels".len()
        && file.get(file.len() - ".rels".len()..).is_some_and(|e| e.eq_ignore_ascii_case(".rels"))
}

/// The source of a relationships part: `/visio/_rels/document.xml.rels` is the relationships of `/visio/document.xml`, and
/// `/_rels/.rels` those of the package, which is `/`. `None` for a name that is not a relationships part. The source may not be a
/// part of any package (E9 of the format spec: an orphan); this only reads the name.
pub fn rels_source(rels_part: &str) -> Option<String> {
    let name = rels_part.trim_start_matches('/');
    let (folder, file) = name.rsplit_once('/')?;
    let (parent, rels_folder) = folder.rsplit_once('/').unwrap_or(("", folder));
    if !rels_folder.eq_ignore_ascii_case("_rels") || file.len() < ".rels".len() {
        return None;
    }
    let (source, suffix) = file.split_at_checked(file.len() - ".rels".len())?;
    if !suffix.eq_ignore_ascii_case(".rels") {
        return None;
    }
    Some(match (parent.is_empty(), source.is_empty()) {
        (true, true) => "/".to_owned(),
        (true, false) => format!("/{source}"),
        (false, _) => format!("/{parent}/{source}"),
    })
}

/// The name of the relationships part of `part`: `/visio/document.xml` has `/visio/_rels/document.xml.rels`, and the package
/// itself (`/`, or nothing) has `/_rels/.rels`.
pub fn rels_part_name(part: &str) -> String {
    let part = part.trim_start_matches('/');
    if part.is_empty() {
        return "/_rels/.rels".to_owned();
    }
    match part.rsplit_once('/') {
        Some((folder, file)) => format!("/{folder}/_rels/{file}.rels"),
        None => format!("/_rels/{part}.rels"),
    }
}

/// The part a relationship's `Target` names, resolved against `source`, or `None` when it names no part.
///
/// The rule is the usual one for references (RFC 3986 section 5.2): an absolute target (`/visio/media/image1.png`) is that part; a
/// relative one starts in the folder of `source`, `.` is dropped and `..` goes up a folder, and goes no higher than the root. The
/// fragment (`#...`) is dropped, percent-escapes are decoded, and the result is a part name like any other ([`fold`] it to compare).
/// `source` is a part name, or `/` or nothing for the package. The name need not be a part of any package: a dangling target is
/// data, and this function never looks.
///
/// `None`: an empty target, one that ends in a slash, `.` or `..`, an escape that spells a slash or a backslash, a control character,
/// escapes that are not UTF-8, a segment that ends in a dot (so no part has that name), or a result longer than the name limit.
///
/// A backslash is read as a slash (a labelled choice: some writers have used the wrong one in targets, and a target is only a key,
/// so reading the intent costs nothing; an *item name* with a backslash is refused).
pub fn resolve(source: &str, target: &str) -> Option<String> {
    resolve_within(source, target, MAX_PART_NAME_BYTES)
}

pub(crate) fn resolve_within(source: &str, target: &str, max_len: usize) -> Option<String> {
    let path = target.split('#').next().unwrap_or("").replace('\\', "/");
    let (Decoded::Text(path) | Decoded::Unreserved(path)) = percent_decode(&path) else { return None };
    if path.is_empty() || path.ends_with('/') || matches!(path.rsplit('/').next(), Some("." | "..")) || path.chars().any(char::is_control) {
        return None;
    }
    let mut segments: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        // The folder of the source: everything but its last segment.
        let mut folder: Vec<&str> = source.split('/').filter(|s| !s.is_empty()).collect();
        folder.pop();
        folder
    };
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    // Every segment of the result, including those that came from `source`, which the caller gave: a name that could not be a part
    // name is no target.
    let usable = |s: &&str| segment_is_legal(s) && !s.chars().any(|c| c.is_control() || c == '\\');
    if segments.is_empty() || !segments.iter().all(usable) {
        return None;
    }
    let name = format!("/{}", segments.join("/"));
    (name.len() <= max_len.saturating_add(1)).then_some(name)
}
