//! XML text mutations: hostile changes to the text of one XML part that leave it as XML a parser can read, or break it in one chosen way.
//!
//! A [`Profile`] says what a format's elements are called (its formula cells, its references from one element to another, the unit it nests groups in), and the mutators work on
//! the text with a small scanner of their own ([`scan`]) and never build a tree: a part that is no longer well formed is just text. Each mutator takes the text and a stream of
//! random numbers and gives the new text with a line that says what it did, or `None` when the part has nothing for it to change.

use super::rng::Rng;

/// What a format's XML calls the things the mutators work on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Profile {
    /// The element that holds a formula, with the attribute that names it, the one that holds its formula and the one that holds its value.
    pub cell: Option<CellNames>,
    /// Pairs of (an attribute that refers to another element, the attribute that names an element): what a loop is made of.
    pub refs: &'static [(&'static str, &'static str)],
    /// The text that opens and the text that closes one nested group of shapes.
    pub nest_unit: Option<(&'static str, &'static str)>,
    /// The elements a nested group is put into, as their first child.
    pub nest_into: &'static [&'static str],
    /// A chain of linked items: where it goes, what an item is and how long it is.
    pub chain: Option<Chain>,
    /// The parts of a package that are masters: the start of their names (the master's number follows), the element of a shape in one and the attribute that makes the shape an
    /// instance of a master. A loop of masters is made across these parts.
    pub masters: Option<(&'static str, &'static str, &'static str)>,
}

/// A chain of items that are each linked to the next: the element they are put into (as its last children), the text of item `k` of `n`, and how long a chain is (the sizes
/// every sweep picks from, and the heavy ones that only `DIACRAFT_DOOR_BIG` adds: an item is a shape or two, and what a shape costs to read and write differs by format).
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    pub into: &'static str,
    pub item: fn(usize, usize) -> String,
    pub modest: &'static [usize],
    pub heavy: &'static [usize],
}

/// The names of a formula cell's element and attributes.
#[derive(Clone, Copy, Debug)]
pub struct CellNames {
    pub element: &'static str,
    pub name: &'static str,
    pub value: &'static str,
    pub formula: &'static str,
}

/// Which kind of tag a [`Tag`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Open,
    Close,
    Empty,
}

/// One tag of the text: where it begins (`<`), where it ends (after `>`), its name and its kind.
#[derive(Clone, Copy, Debug)]
pub struct Tag {
    pub start: usize,
    pub end: usize,
    pub name: (usize, usize),
    pub kind: Kind,
}

/// One attribute of a tag: its name and the text of its value (inside the quotes).
#[derive(Clone, Copy, Debug)]
pub struct Attr {
    pub name: (usize, usize),
    pub value: (usize, usize),
}

/// The most tags [`scan`] gives, so that a part of millions of tags costs the sweep no more than the reader.
const MAX_TAGS: usize = 400_000;

fn skip_to(bytes: &[u8], from: usize, end: &[u8]) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes.get(i..i + end.len()) == Some(end) {
            return i + end.len();
        }
        i += 1;
    }
    bytes.len()
}

/// The end of a declaration that begins `<!`: past the `>` that closes it, and past an internal subset in square brackets.
fn skip_declaration(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    let mut brackets = 0usize;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'>' if brackets == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// The end of the tag that begins at `from`: past the `>` that closes it, with quoted values passed over. The flag is true for `/>`.
fn tag_end(bytes: &[u8], from: usize) -> (usize, bool) {
    let mut i = from;
    let mut quote = 0u8;
    while let Some(&b) = bytes.get(i) {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return (i + 1, i > from && bytes.get(i - 1) == Some(&b'/'));
        }
        i += 1;
    }
    (bytes.len(), false)
}

/// Every start tag, end tag and empty tag of `text`, in order; comments, instructions, CDATA and declarations are passed over.
pub fn scan(text: &str) -> Vec<Tag> {
    let bytes = text.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0;
    while let Some(offset) = bytes.get(i..).and_then(|rest| rest.iter().position(|&b| b == b'<')) {
        let start = i + offset;
        let rest = bytes.get(start..).unwrap_or_default();
        if rest.starts_with(b"<!--") {
            i = skip_to(bytes, start + 4, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            i = skip_to(bytes, start + 9, b"]]>");
        } else if rest.starts_with(b"<?") {
            i = skip_to(bytes, start + 2, b"?>");
        } else if rest.starts_with(b"<!") {
            i = skip_declaration(bytes, start + 2);
        } else {
            let close = rest.starts_with(b"</");
            let name_start = start + if close { 2 } else { 1 };
            let name_end =
                (name_start..bytes.len()).find(|&k| matches!(bytes.get(k), Some(b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>'))).unwrap_or(bytes.len());
            let (end, empty) = tag_end(bytes, name_end);
            let kind = if close {
                Kind::Close
            } else if empty {
                Kind::Empty
            } else {
                Kind::Open
            };
            tags.push(Tag { start, end, name: (name_start, name_end), kind });
            i = end;
        }
        if tags.len() >= MAX_TAGS {
            break;
        }
    }
    tags
}

/// The name of a tag.
pub fn name<'a>(text: &'a str, tag: &Tag) -> &'a str {
    text.get(tag.name.0..tag.name.1).unwrap_or("")
}

/// The attributes of a start tag, as `name="value"` pairs (a malformed rest of the tag ends the list).
pub fn attrs(text: &str, tag: &Tag) -> Vec<Attr> {
    let bytes = text.as_bytes();
    let stop = if tag.kind == Kind::Empty { tag.end.saturating_sub(2) } else { tag.end.saturating_sub(1) };
    let mut out = Vec::new();
    let mut i = tag.name.1;
    while i < stop {
        while bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        let name_start = i;
        while i < stop && bytes.get(i).is_some_and(|&b| b != b'=' && !b.is_ascii_whitespace()) {
            i += 1;
        }
        let name_end = i;
        while bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') || name_end == name_start {
            break;
        }
        i += 1;
        while bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        let Some(&quote) = bytes.get(i).filter(|&&q| q == b'"' || q == b'\'') else { break };
        let value_start = i + 1;
        let Some(close) = (value_start..stop.max(value_start)).find(|&k| bytes.get(k) == Some(&quote)) else { break };
        out.push(Attr { name: (name_start, name_end), value: (value_start, close) });
        i = close + 1;
    }
    out
}

/// The value of the attribute `wanted` of `tag`.
pub fn attr_value<'a>(text: &'a str, tag: &Tag, wanted: &str) -> Option<&'a str> {
    let found = attrs(text, tag).into_iter().find(|a| text.get(a.name.0..a.name.1) == Some(wanted))?;
    text.get(found.value.0..found.value.1)
}

/// The index in `tags` of the end tag that matches the start tag at `open`.
pub fn matching_close(text: &str, tags: &[Tag], open: usize) -> Option<usize> {
    let opening = tags.get(open)?;
    if opening.kind != Kind::Open {
        return None;
    }
    let wanted = name(text, opening);
    let mut depth = 0usize;
    for (k, tag) in tags.iter().enumerate().skip(open) {
        if name(text, tag) != wanted {
            continue;
        }
        match tag.kind {
            Kind::Open => depth += 1,
            Kind::Close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            Kind::Empty => {}
        }
    }
    None
}

/// A change to the text: the bytes `start..end` become `with`.
#[derive(Clone, Debug)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub with: String,
}

/// The text with `edits` made (they must not overlap; the order given does not matter).
pub fn apply(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut out = text.to_owned();
    for e in edits {
        if e.start <= e.end && e.end <= out.len() && out.is_char_boundary(e.start) && out.is_char_boundary(e.end) {
            out.replace_range(e.start..e.end, &e.with);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------- numbers

/// Numbers that no honest file holds: past the range of a float, at its edge, past that of every integer width, and long.
pub fn huge_numbers() -> Vec<String> {
    let mut all: Vec<String> = [
        "1e308",
        "-1e308",
        "1.7976931348623157e308",
        "1e-308",
        "4.9e-324",
        "9007199254740993",
        "18446744073709551616",
        "340282366920938463463374607431768211456",
        "-0",
        "0.0000000000000000000000000000000000000001",
        "2147483648",
        "-2147483649",
        "4294967296",
        "1e999",
        "-1e999",
        "1e19",
        "0.1e400",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    all.push("9".repeat(400));
    all.push(format!("0.{}1", "0".repeat(500)));
    all.push(format!("-{}.5", "7".repeat(310)));
    all
}

/// What stands for no number: not a number, an infinity, and text that a number parser may take or refuse.
pub fn not_numbers() -> Vec<&'static str> {
    vec![
        "NaN",
        "nan",
        "-nan",
        "Inf",
        "-Inf",
        "inf",
        "Infinity",
        "-Infinity",
        "INF",
        "#NUM!",
        "#DIV/0!",
        "1/0",
        "0/0",
        "1e",
        "e5",
        "--5",
        "+-5",
        "0x7FFFFFFF",
        "1,5",
        ".",
        "-",
        "",
    ]
}

/// Whether `value` is a number with an optional unit after it (`2`, `-1.5`, `3cm`, `50%`); the length of the number part.
fn number_part(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    let digits = i;
    while bytes.get(i).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
        i += 1;
    }
    if i == digits || !bytes.get(digits..i)?.iter().any(u8::is_ascii_digit) {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut k = i + 1;
        if matches!(bytes.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        let exponent = k;
        while bytes.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
        if k > exponent {
            i = k;
        }
    }
    bytes.get(i..)?.iter().all(|b| b.is_ascii_alphabetic() || *b == b'%').then_some(i)
}

/// The numeric attribute values of `text`: where each value begins, and the length of its number part.
fn numeric_values(text: &str, tags: &[Tag]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for tag in tags.iter().filter(|t| t.kind != Kind::Close) {
        for a in attrs(text, tag) {
            if let Some(len) = text.get(a.value.0..a.value.1).and_then(number_part) {
                found.push((a.value.0, len));
            }
        }
    }
    found
}

/// The numeric values of the cells of `text` (the `value` attribute of the formula cells the profile names).
fn numeric_cell_values(text: &str, tags: &[Tag], names: &CellNames) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for tag in tags.iter().filter(|t| t.kind != Kind::Close && name(text, t) == names.element) {
        for a in attrs(text, tag).into_iter().filter(|a| text.get(a.name.0..a.name.1) == Some(names.value)) {
            if let Some(len) = text.get(a.value.0..a.value.1).and_then(number_part) {
                found.push((a.value.0, len));
            }
        }
    }
    found
}

/// Replaces the number part of up to `how_many` numeric attribute values with numbers from `pool`, each unit kept. Three times in four, where the profile has cells and the part
/// has numbers in them, the values are those of cells.
fn replace_numbers(text: &str, profile: &Profile, rng: &mut Rng, pool: &[String], how_many: usize, what: &str) -> Option<(String, String)> {
    let tags = scan(text);
    let mut values = numeric_values(text, &tags);
    if let Some(names) = &profile.cell {
        let in_cells = numeric_cell_values(text, &tags, names);
        if !in_cells.is_empty() && !rng.one_in(4) {
            values = in_cells;
        }
    }
    if values.is_empty() || pool.is_empty() {
        return None;
    }
    let mut edits = Vec::new();
    let mut said = Vec::new();
    for _ in 0..how_many.min(values.len()) {
        let &(start, len) = rng.pick(&values)?;
        if edits.iter().any(|e: &Edit| e.start == start) {
            continue;
        }
        let with = rng.pick(pool)?.clone();
        said.push(with.chars().take(24).collect::<String>());
        edits.push(Edit { start, end: start + len, with });
    }
    let line = format!("{what}: {} value(s) of {} numeric attribute(s): {}", edits.len(), values.len(), said.join(" | "));
    Some((apply(text, edits), line))
}

/// Numbers past every range in up to three numeric attributes.
pub fn huge_number(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let how_many = rng.between(1, 3);
    replace_numbers(text, profile, rng, &huge_numbers(), how_many, "huge numbers")
}

/// Text that is no number in up to three numeric attributes.
pub fn not_a_number(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let pool: Vec<String> = not_numbers().into_iter().map(str::to_owned).collect();
    let how_many = rng.between(1, 3);
    replace_numbers(text, profile, rng, &pool, how_many, "not numbers")
}

// ---------------------------------------------------------------------------------------------------------------- depth, breadth, size

/// A nest of `depth` copies of `unit` (the opening and the closing text of one level) with `inner` in the middle.
fn nested(unit: (&str, &str), depth: usize, inner: &str) -> String {
    let mut out = String::with_capacity(depth * (unit.0.len() + unit.1.len()) + inner.len());
    for _ in 0..depth {
        out.push_str(unit.0);
    }
    out.push_str(inner);
    for _ in 0..depth {
        out.push_str(unit.1);
    }
    out
}

/// How deep a nest goes: past what a reader may take on a small stack, and past what its XML limit allows.
fn depths(rng: &mut Rng) -> usize {
    rng.size(&[64, 200, 400, 600, 3000], &[20_000, 100_000])
}

/// A chain of nested groups, `depth` deep, inside the first element the profile names; or, when the profile names none that the text has, the same element inside itself.
pub fn nest(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let tags = scan(text);
    let depth = depths(rng);
    if let Some(unit) = profile.nest_unit {
        let targets: Vec<&Tag> = tags.iter().filter(|t| t.kind == Kind::Open && profile.nest_into.contains(&name(text, t))).collect();
        if let Some(tag) = rng.pick(&targets) {
            let at = tag.end;
            let line = format!("a nest of {depth} groups as the first child of `{}`", name(text, tag));
            return Some((apply(text, vec![Edit { start: at, end: at, with: nested(unit, depth, "") }]), line));
        }
    }
    // The same element inside itself, below the root.
    let candidates: Vec<usize> = (1..tags.len()).filter(|&k| tags.get(k).is_some_and(|t| t.kind != Kind::Close)).collect();
    let &k = rng.pick(&candidates)?;
    let tag = tags.get(k)?;
    let element = name(text, tag).to_owned();
    let bare = (format!("<{element}>"), format!("</{element}>"));
    let line = format!("`{element}` inside itself, {depth} deep");
    let with = if tag.kind == Kind::Empty {
        let open = format!("{}>", text.get(tag.start..tag.end.saturating_sub(2))?);
        format!("{open}{}</{element}>", nested((&bare.0, &bare.1), depth, ""))
    } else {
        nested((&bare.0, &bare.1), depth, "")
    };
    let edit = if tag.kind == Kind::Empty { Edit { start: tag.start, end: tag.end, with } } else { Edit { start: tag.end, end: tag.end, with } };
    Some((apply(text, vec![edit]), line))
}

/// A flood of attributes on one tag, one attribute with a very long value, or one name repeated.
pub fn flood(text: &str, rng: &mut Rng) -> Option<(String, String)> {
    let tags = scan(text);
    let candidates: Vec<&Tag> = tags.iter().filter(|t| t.kind != Kind::Close).collect();
    let tag = *rng.pick(&candidates)?;
    let at = tag.name.1;
    let (with, line) = match rng.below(3) {
        0 => {
            let n = rng.size(&[50, 400, 5000], &[60_000]);
            ((0..n).map(|k| format!(" flood{k}=\"{k}\"")).collect::<String>(), format!("{n} attributes on `{}`", name(text, tag)))
        }
        1 => {
            let len = rng.size(&[1 << 10, 1 << 16], &[1 << 20]);
            (format!(" long=\"{}\"", "x".repeat(len)), format!("one attribute of {len} bytes on `{}`", name(text, tag)))
        }
        _ => {
            let n = *rng.pick(&[2usize, 50, 2000]).unwrap_or(&2);
            (" same=\"1\"".repeat(n), format!("one attribute name {n} times on `{}`", name(text, tag)))
        }
    };
    Some((apply(text, vec![Edit { start: at, end: at, with }]), line))
}

/// Many more copies of one element, next to it.
pub fn wide(text: &str, rng: &mut Rng) -> Option<(String, String)> {
    let tags = scan(text);
    let candidates: Vec<usize> = (1..tags.len()).filter(|&k| tags.get(k).is_some_and(|t| t.kind != Kind::Close)).collect();
    let &k = rng.pick(&candidates)?;
    let open = tags.get(k)?;
    let end = if open.kind == Kind::Empty { open.end } else { tags.get(matching_close(text, &tags, k)?)?.end };
    let element = text.get(open.start..end)?;
    let n = rng.size(&[100, 1000], &[30_000]).min((2 << 20) / element.len().max(1)).max(2);
    let line = format!("{n} more copies of `{}`", name(text, open));
    Some((apply(text, vec![Edit { start: end, end, with: element.repeat(n) }]), line))
}

// ---------------------------------------------------------------------------------------------------------------- entities and text

/// Text with references that no honest writer makes.
fn entity_text(rng: &mut Rng) -> (String, &'static str) {
    match rng.below(7) {
        0 => ("&amp;".repeat(rng.size(&[1000, 8000], &[60_000])), "many &amp;"),
        1 => ("&#x41;".repeat(rng.size(&[1000, 8000], &[60_000])), "many character references"),
        2 => ("&#0;&#xD800;&#xDFFF;&#x110000;&#99999999999;&#-1;&#x;&#;".to_owned(), "invalid character references"),
        3 => ("&nbsp;&foo;&amp&lt".to_owned(), "unknown and unterminated references"),
        4 => ("&lol9;".to_owned(), "a reference to the last of a ladder of entities"),
        5 => ("&xxe;".to_owned(), "a reference to an external entity"),
        _ => ("&#x202E;\u{0}\u{1}\u{FFFE}\u{FFFF}\u{202E}".to_owned(), "control and bidirectional characters"),
    }
}

/// The declaration of an internal subset: a ladder of entities that each hold ten of the one before (a billion laughs), and one that names a file.
fn doctype(root: &str) -> String {
    let mut out = format!("<!DOCTYPE {root} [<!ENTITY lol0 \"lol\">");
    for k in 1..=9 {
        let prev = format!("&lol{};", k - 1);
        out.push_str(&format!("<!ENTITY lol{k} \"{}\">", prev.repeat(10)));
    }
    out.push_str("<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>");
    out
}

/// A reference to an entity, or a hostile run of text, in the value of an attribute or in the text under an element, with a declaration of the entities in front of the root when
/// the reference is to one.
pub fn entities(text: &str, rng: &mut Rng) -> Option<(String, String)> {
    let tags = scan(text);
    let (with, what) = entity_text(rng);
    let root = tags.iter().find(|t| t.kind != Kind::Close)?;
    let with_doctype = with.starts_with("&lol") || with.starts_with("&xxe");
    let mut edits = Vec::new();
    let valued: Vec<(&Tag, Attr)> =
        tags.iter().filter(|t| t.kind != Kind::Close).flat_map(|t| attrs(text, t).into_iter().map(move |a| (t, a))).collect();
    let in_a_value = rng.one_in(2);
    let where_ = match rng.pick(&valued).filter(|_| in_a_value) {
        Some((tag, attr)) => {
            edits.push(Edit { start: attr.value.1, end: attr.value.1, with: with.clone() });
            format!("the value of an attribute of `{}`", name(text, tag))
        }
        None => {
            let opens: Vec<&Tag> = tags.iter().filter(|t| t.kind == Kind::Open).collect();
            let tag = *rng.pick(&opens)?;
            edits.push(Edit { start: tag.end, end: tag.end, with: with.clone() });
            format!("the text of `{}`", name(text, tag))
        }
    };
    if with_doctype {
        let decl = doctype(name(text, root));
        edits.push(Edit { start: root.start, end: root.start, with: decl });
    }
    Some((apply(text, edits), format!("{what} in {where_}")))
}

// ---------------------------------------------------------------------------------------------------------------- formulas and references

/// A formula that reads the cell `target` in one of the ways a formula may name a cell.
fn formula_reading(target: &str, rng: &mut Rng) -> String {
    match rng.below(5) {
        0 => target.to_owned(),
        1 => format!("{target}+1"),
        2 => format!("Sheet.{}!{target}", rng.between(1, 12)),
        3 => format!("IF({target}>0,{target},{target})"),
        _ => format!("SUM({target},{target})*2"),
    }
}

/// The formula cells of `text`: the tag of each and its name.
fn cells<'a>(text: &'a str, tags: &[Tag], names: &CellNames) -> Vec<(Tag, &'a str)> {
    tags.iter()
        .filter(|t| t.kind != Kind::Close && name(text, t) == names.element)
        .filter_map(|t| attr_value(text, t, names.name).map(|n| (*t, n)))
        .collect()
}

/// Sets the attribute `attr` of `tag` to `value`: the existing value is replaced, or the attribute is put in after the name.
pub fn set_attr(text: &str, tag: &Tag, attr: &str, value: &str) -> Edit {
    match attrs(text, tag).into_iter().find(|a| text.get(a.name.0..a.name.1) == Some(attr)) {
        Some(found) => Edit { start: found.value.0, end: found.value.1, with: value.to_owned() },
        None => Edit { start: tag.name.1, end: tag.name.1, with: format!(" {attr}=\"{value}\"") },
    }
}

/// A formula built to cost a parser or a walk of the references: a ring of cells that read each other, one that reads itself, one that reads hundreds, and parentheses, signs and calls
/// nested thousands deep.
pub fn formula(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let names = profile.cell?;
    let tags = scan(text);
    let all = cells(text, &tags, &names);
    if all.is_empty() {
        return None;
    }
    match rng.below(5) {
        0 => {
            let k = rng.between(2, 8).min(all.len());
            let ring: Vec<&(Tag, &str)> = (0..k).filter_map(|_| rng.pick(&all)).collect();
            let edits: Vec<Edit> = ring
                .iter()
                .enumerate()
                .filter_map(|(i, (tag, _))| ring.get((i + 1) % ring.len()).map(|(_, next)| (tag, *next)))
                .map(|(tag, next)| set_attr(text, tag, names.formula, &formula_reading(next, rng)))
                .collect();
            let mut seen = std::collections::BTreeSet::new();
            let edits: Vec<Edit> = edits.into_iter().filter(|e| seen.insert(e.start)).collect();
            let line = format!("a ring of {} cells that read each other", edits.len());
            Some((apply(text, edits), line))
        }
        1 => {
            let (tag, cell) = *rng.pick(&all)?;
            let with = formula_reading(cell, rng);
            Some((apply(text, vec![set_attr(text, &tag, names.formula, &with)]), format!("the cell `{cell}` reads itself: {with}")))
        }
        2 => {
            let (tag, _) = *rng.pick(&all)?;
            let n = rng.between(50, 400);
            let reads: Vec<&str> = (0..n).filter_map(|_| rng.pick(&all).map(|(_, c)| *c)).collect();
            let with = format!("SUM({})", reads.join(","));
            Some((apply(text, vec![set_attr(text, &tag, names.formula, &with)]), format!("a cell that reads {n} cells")))
        }
        3 => {
            let (tag, _) = *rng.pick(&all)?;
            let n = rng.size(&[100, 1000, 5000], &[50_000]);
            let (with, what) = match rng.below(3) {
                0 => (format!("{}1{}", "(".repeat(n), ")".repeat(n)), "parentheses"),
                1 => (format!("{}1", "-".repeat(n)), "minus signs"),
                _ => (format!("{}1{}", "ABS(".repeat(n), ")".repeat(n)), "calls"),
            };
            Some((apply(text, vec![set_attr(text, &tag, names.formula, &with)]), format!("a formula of {n} nested {what}")))
        }
        _ => {
            let (tag, _) = *rng.pick(&all)?;
            let n = rng.size(&[10_000], &[200_000]);
            let with = format!("1{}", "+1".repeat(n));
            Some((apply(text, vec![set_attr(text, &tag, names.formula, &with)]), format!("a formula of {n} additions")))
        }
    }
}

/// For each tag, the value of `id_attr` on it or, if it has none, on the nearest element around it (`None` for an end tag and where no element around has one).
fn owners<'a>(text: &'a str, tags: &[Tag], id_attr: &str) -> Vec<Option<&'a str>> {
    let mut stack: Vec<Option<&str>> = Vec::new();
    let mut out = Vec::with_capacity(tags.len());
    for tag in tags {
        if tag.kind == Kind::Close {
            stack.pop();
            out.push(None);
            continue;
        }
        let owner = attr_value(text, tag, id_attr).or_else(|| stack.last().copied().flatten());
        out.push(owner);
        if tag.kind == Kind::Open {
            stack.push(owner);
        }
    }
    out
}

/// A loop of references: elements that name each other, one that names itself, by an attribute the profile lists. An element is named by the `id` attribute it has, or that of the
/// element it is in (a style's `LineStyle` is on the sheet inside the style that has the `id`).
pub fn reference_loop(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let tags = scan(text);
    let usable: Vec<(&str, &str)> =
        profile.refs.iter().copied().filter(|(_, id)| tags.iter().any(|t| t.kind != Kind::Close && attr_value(text, t, id).is_some())).collect();
    let &(refer, id) = rng.pick(&usable)?;
    let owner = owners(text, &tags, id);
    let holding: Vec<usize> = (0..tags.len())
        .filter(|&k| owner.get(k).copied().flatten().is_some() && tags.get(k).is_some_and(|t| attr_value(text, t, refer).is_some()))
        .collect();
    // With no element that has the reference yet, it is put on elements that have the name.
    let named: Vec<usize> =
        (0..tags.len()).filter(|&k| tags.get(k).is_some_and(|t| t.kind != Kind::Close && attr_value(text, t, id).is_some())).collect();
    let pool = if holding.is_empty() { named } else { holding };
    let k = if rng.one_in(3) { 1 } else { rng.between(2, 6).min(pool.len()) };
    let ring: Vec<usize> = (0..k).filter_map(|_| rng.pick(&pool).copied()).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for (i, &at) in ring.iter().enumerate() {
        let next = ring.get((i + 1) % ring.len()).and_then(|&n| owner.get(n).copied().flatten());
        if let (Some(tag), Some(next)) = (tags.get(at), next) {
            let edit = set_attr(text, tag, refer, next);
            if seen.insert(edit.start) {
                edits.push(edit);
            }
        }
    }
    let line = format!("`{refer}` makes a loop of {} element(s) named by `{id}`", edits.len());
    Some((apply(text, edits), line))
}

/// A chain of linked items, as many as the profile's maker gives, put in as the last children of the element it names.
pub fn chain(text: &str, profile: &Profile, rng: &mut Rng) -> Option<(String, String)> {
    let Chain { into, item: make, modest, heavy } = profile.chain?;
    let tags = scan(text);
    let opens: Vec<usize> = (0..tags.len()).filter(|&k| tags.get(k).is_some_and(|t| t.kind == Kind::Open && name(text, t) == into)).collect();
    let &open = rng.pick(&opens)?;
    let close = tags.get(matching_close(text, &tags, open)?)?;
    let n = rng.size(modest, heavy);
    let items: String = (0..n).map(|k| make(k, n)).collect();
    Some((apply(text, vec![Edit { start: close.start, end: close.start, with: items }]), format!("a chain of {n} linked items in `{into}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?><!-- <c/> --><Root a="1"><Shape ID="1" W="2cm"><Cell N="Width" V="2" F="Height"/><Cell N="Height" V="1.5"/></Shape><Shape ID="2" LineStyle="1"/><![CDATA[<z>]]></Root>"#;

    const PROFILE: Profile = Profile {
        cell: Some(CellNames { element: "Cell", name: "N", value: "V", formula: "F" }),
        refs: &[("LineStyle", "ID")],
        nest_unit: Some(("<Shape ID=\"9\"><Shapes>", "</Shapes></Shape>")),
        nest_into: &["Root"],
        chain: Some(Chain { into: "Root", item: |k, n| format!("<Link ID=\"{k}\" To=\"{}\"/>", (k + 1) % n), modest: &[50, 300], heavy: &[5000] }),
        masters: None,
    };

    fn names(text: &str) -> Vec<String> {
        scan(text).iter().map(|t| format!("{:?}:{}", t.kind, name(text, t))).collect()
    }

    #[test]
    fn the_scanner_finds_tags_and_passes_over_comments_cdata_and_declarations() {
        assert_eq!(names(SAMPLE), ["Open:Root", "Open:Shape", "Empty:Cell", "Empty:Cell", "Close:Shape", "Empty:Shape", "Close:Root"]);
        let text = "<!DOCTYPE r [<!ENTITY e \"<x>\">]><r a='>'/>";
        assert_eq!(names(text), ["Empty:r"], "an internal subset and a quoted `>` do not end a tag");
    }

    #[test]
    fn attributes_are_read_with_their_values_and_a_value_is_found_by_name() {
        let tags = scan(SAMPLE);
        let cell = tags.get(2).copied().unwrap();
        let read: Vec<&str> = attrs(SAMPLE, &cell).iter().map(|a| &SAMPLE[a.value.0..a.value.1]).collect();
        assert_eq!(read, ["Width", "2", "Height"]);
        assert_eq!(attr_value(SAMPLE, &cell, "F"), Some("Height"));
        assert_eq!(attr_value(SAMPLE, &cell, "none"), None);
        assert_eq!(matching_close(SAMPLE, &tags, 1), Some(4));
        assert_eq!(matching_close(SAMPLE, &tags, 2), None, "an empty tag has no end tag");
    }

    #[test]
    fn a_number_keeps_its_unit_and_text_that_is_no_number_is_left_alone() {
        assert_eq!(number_part("2cm"), Some(1));
        assert_eq!(number_part("-1.5e3pt"), Some(6));
        assert_eq!(number_part("50%"), Some(2));
        assert_eq!(number_part("e5"), None);
        assert_eq!(number_part("Width"), None);
        assert_eq!(number_part("."), None);
        let mut rng = Rng::new(1);
        let (out, _) = huge_number(SAMPLE, &PROFILE, &mut rng).unwrap();
        assert_ne!(out, SAMPLE);
        assert_eq!(scan(&out).len(), scan(SAMPLE).len(), "a number is replaced and no tag is made or lost");
    }

    #[test]
    fn every_mutator_changes_the_text_and_says_what_it_did_and_a_part_with_nothing_to_change_gives_none() {
        for case in 0..40 {
            let mut rng = Rng::for_case(5, "xml", case);
            for (what, out) in [
                ("not a number", not_a_number(SAMPLE, &PROFILE, &mut rng)),
                ("nest", nest(SAMPLE, &PROFILE, &mut rng)),
                ("flood", flood(SAMPLE, &mut rng)),
                ("wide", wide(SAMPLE, &mut rng)),
                ("entities", entities(SAMPLE, &mut rng)),
                ("formula", formula(SAMPLE, &PROFILE, &mut rng)),
                ("loop", reference_loop(SAMPLE, &PROFILE, &mut rng)),
                ("chain", chain(SAMPLE, &PROFILE, &mut rng)),
            ] {
                let (text, line) = out.unwrap_or_else(|| panic!("{what} found nothing to change in case {case}"));
                assert_ne!(text, SAMPLE, "{what}: {line}");
                assert!(!line.is_empty());
            }
        }
        let none = Profile::default();
        let mut rng = Rng::new(2);
        assert!(
            formula(SAMPLE, &none, &mut rng).is_none()
                && chain(SAMPLE, &none, &mut rng).is_none()
                && reference_loop(SAMPLE, &none, &mut rng).is_none()
        );
        assert!(huge_number("<a b=\"c\"/>", &none, &mut rng).is_none(), "no number in the part");
    }

    #[test]
    fn a_nest_is_balanced_and_as_deep_as_it_says() {
        let mut rng = Rng::new(9);
        let (text, _) = nest(SAMPLE, &PROFILE, &mut rng).unwrap();
        let tags = scan(&text);
        let (opens, closes) = (tags.iter().filter(|t| t.kind == Kind::Open).count(), tags.iter().filter(|t| t.kind == Kind::Close).count());
        assert_eq!(opens, closes, "balanced");
        assert!(opens >= 2 * 64);
    }

    #[test]
    fn a_loop_ring_closes_on_itself_and_a_chain_links_each_item_to_the_next() {
        let mut rng = Rng::new(4);
        let (text, _) = reference_loop(SAMPLE, &PROFILE, &mut rng).unwrap();
        let tags = scan(&text);
        let looped: Vec<&str> = tags.iter().filter_map(|t| attr_value(&text, t, "LineStyle")).collect();
        assert!(!looped.is_empty() && looped.iter().all(|v| *v == "1" || *v == "2"));
        let (text, line) = chain(SAMPLE, &PROFILE, &mut rng).unwrap();
        assert!(line.starts_with("a chain of "));
        assert!(text.contains("<Link ID=\"0\" To=\"1\"/>"));
    }

    #[test]
    fn edits_that_fall_off_the_text_or_inside_a_character_are_not_made() {
        assert_eq!(apply("héllo", vec![Edit { start: 2, end: 3, with: "x".into() }]), "héllo");
        assert_eq!(apply("abc", vec![Edit { start: 9, end: 10, with: "x".into() }]), "abc");
        assert_eq!(apply("abc", vec![Edit { start: 1, end: 2, with: "XY".into() }, Edit { start: 0, end: 0, with: ">".into() }]), ">aXYc");
    }
}
