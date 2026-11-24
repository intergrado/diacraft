//! The hyperlinks of a shape: Insert, Links, Link (the cells are).
//!
//! **What is kept in cells.** A shape's links are the rows of its `Hyperlink` section, keyed by name ("one row per link on a shape"). A link holds `Description` (the
//! name the shape's shortcut menu lists), `Address`, `SubAddress`, `ExtraInfo`, `Frame`, `SortKey`, `NewWindow`, `Default` and `Invisible`. The dialog writes `Description`,
//! `Address` and `SubAddress`, and `NewWindow`, `Default`, `Invisible`, `ExtraInfo` and `Frame` only when they are set or were there. A cell a row lacks reads as empty or off.
//!
//! **Project rules** (ours; the specs leave them open, and the check list lists them):
//!
//! * A new row is named `Row_<n>` with the least `n` from 1 that no row of the shape has now. A row a set names keeps its name.
//! * A set is the shape's whole list: a row it names is kept (the cells it gives change, the others stay), a row it leaves out is taken away, and a link with no name is a
//!   new row after the others. A set that changes nothing is no undo step. At most one link is the default, and a link needs an `Address` or a `SubAddress`.
//! * A link with no `Address` goes to **this drawing**: its `SubAddress` names a page, or a shape of a page as `<page name>/<shape name>` (a shape's `Name` or
//!   `NameU`) or `<page name>/Sheet.<id>`. The form is ours (only that a page or shape of this drawing is a target; Q8 asks what the dialog's Default does).
//! * A click follows **web (`http`, `https`), FTP (`ftp`, `ftps`) and mail (`mailto`) addresses, and the links of this drawing, and nothing else**: a `file:`
//!   address, a path, another program's scheme (`javascript:`, `ms-msdt:`) or a script is refused in words, since a drawing from anywhere must not start a program by a click. Spaces in
//!   an address go as `%20`. The `SubAddress` of a web address is the fragment, unless the address has one.
//! * What a click hands to the browser or the mail client is **written again from what was checked**. A web or FTP address **whose percent sequences all decode** (a lone percent sign, or bytes that are no text, refuse it: the browser would read other text than was checked) has the characters no address holds (`"<>\`{}|^\`
//!   and the space) written as percent sequences, and is refused when it or what its percent sequences stand for holds a control character or an invisible mark. Its sub-address, the fragment,
//!   is refused for the same, and every character that is not safe in a fragment is written as the percent sequences of its bytes. A mail address passes on its recipients and
//!   `subject`, `body`, `cc` and `bcc`, read as text and written again; any other parameter (`attach`, `attachment` and the rest) is refused in words, since some mail clients attach a
//!   file by one. A line break in a body is refused with the rest of the controls (ours: a body of one line is what a link of a drawing needs).
//! * The link a click follows is the one marked `Default`, else the first that is not `Invisible`, else the first.
//!
//! **Hostile input.** A section of more rows than [`MAX_ROWS`] is refused before any row is read, and the text a call reads is charged to a budget of [`MAX_READ_BYTES`], each text cut at
//! [`MAX_TEXT_CHARS`] characters when it is read.

use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellEdit, CellName, Document, Owner, Resolved, Resolver, Row, RowKey, SectionKey, ShapeList, ShapeRef, SheetRef, Value,
};

use super::{Problem, require_shape, shape_mut, write_cells};

/// The most links one shape lists or is given, and the most rows of the section that are read.
pub const MAX_LINKS: usize = 256;
/// The most rows of a `Hyperlink` section a call reads: past it the call is refused, in words, and nothing is read.
pub const MAX_ROWS: usize = 4_096;
/// The longest each text of a link is, in characters.
pub const MAX_TEXT_CHARS: usize = 4_096;
/// The most bytes of text one call reads from a shape's links.
pub const MAX_READ_BYTES: usize = 8 * 1024 * 1024;

/// One link of a shape, as its row holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    /// The row's name.
    pub name: String,
    pub description: String,
    pub address: String,
    pub sub_address: String,
    pub extra_info: String,
    pub frame: String,
    pub new_window: bool,
    pub default: bool,
    pub invisible: bool,
}

/// What a link is given in a set: the cells it names. A link that names a `name` is that row, and the cells it leaves out stay; a link with no name is a new row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkSpec {
    pub name: Option<String>,
    pub description: Option<String>,
    pub address: Option<String>,
    pub sub_address: Option<String>,
    pub extra_info: Option<String>,
    pub frame: Option<String>,
    pub new_window: Option<bool>,
    pub default: Option<bool>,
    pub invisible: Option<bool>,
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A page of this drawing.
    Page { page: u32 },
    /// A shape of a page of this drawing.
    Shape { page: u32, shape: u32 },
    /// A web or FTP address: the address to open, spaces written as `%20`.
    Web { url: String },
    /// A mail address (`mailto:`).
    Mail { url: String },
}

fn key() -> SectionKey {
    SectionKey::new("Hyperlink", None)
}

fn too_many(n: usize) -> Problem {
    Problem::failed(format!("the shape's Hyperlink section has {n} rows, and at most {MAX_ROWS} are read"))
}

fn text_of(cells: &[(CellName, Resolved<'_>)], name: &str, spent: &mut usize) -> Result<String, Problem> {
    let Some(value) = cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()) else {
        return Ok(String::new());
    };
    let text = value.text();
    // The length is charged before a byte is copied; a text past the cap is cut where a character ends.
    let take = text.char_indices().nth(MAX_TEXT_CHARS).map_or(text.len(), |(at, _)| at);
    *spent = spent.saturating_add(take);
    if *spent > MAX_READ_BYTES {
        return Err(Problem::failed(format!("the shape's links hold more text than is read at once: at most {MAX_READ_BYTES} bytes")));
    }
    Ok(text.get(..take).unwrap_or_default().to_owned())
}

fn flag_of(cells: &[(CellName, Resolved<'_>)], name: &str) -> bool {
    cells
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .and_then(|(_, r)| r.value.cell?.value.as_ref()?.number())
        .is_some_and(|n| n.is_finite() && n != 0.0)
}

/// The links of shape `id` on a page, in row order (the shape's own rows, and those it takes from its master). Refused, in words, past [`MAX_ROWS`] rows or [`MAX_READ_BYTES`] of text.
pub fn read_links(doc: &Document, page: u32, id: u32) -> Result<Vec<Link>, Problem> {
    let at = require_shape(doc, page, id)?;
    read_links_at(doc, &at)
}

fn read_links_at(doc: &Document, at: &ShapeRef) -> Result<Vec<Link>, Problem> {
    let res = Resolver::new(doc);
    let rows = match res.effective_rows_within(&SheetRef::Shape(at.clone()), &key(), MAX_ROWS) {
        None => return Ok(Vec::new()),
        Some(Err(n)) => return Err(too_many(n)),
        Some(Ok(rows)) => rows,
    };
    let mut spent = 0usize;
    let mut out = Vec::with_capacity(rows.len().min(MAX_LINKS));
    for row in &rows {
        let RowKey::Name(name) = &row.key else { continue };
        if out.len() >= MAX_LINKS {
            return Err(Problem::failed(format!("the shape has more than {MAX_LINKS} hyperlinks, and at most {MAX_LINKS} are read")));
        }
        let cells = &row.cells;
        out.push(Link {
            name: name.to_string(),
            description: text_of(cells, "Description", &mut spent)?,
            address: text_of(cells, "Address", &mut spent)?,
            sub_address: text_of(cells, "SubAddress", &mut spent)?,
            extra_info: text_of(cells, "ExtraInfo", &mut spent)?,
            frame: text_of(cells, "Frame", &mut spent)?,
            new_window: flag_of(cells, "NewWindow"),
            default: flag_of(cells, "Default"),
            invisible: flag_of(cells, "Invisible"),
        });
    }
    Ok(out)
}

/// A character a link's text may not hold: the controls, and the invisible and direction marks that make an address read as another.
fn is_hidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

fn check_text(what: &str, text: &str) -> Result<(), Problem> {
    if text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        return Err(Problem::bad(format!("`{what}` must be at most {MAX_TEXT_CHARS} characters")));
    }
    if text.chars().any(is_hidden) {
        return Err(Problem::bad(format!("`{what}` must not hold control characters or invisible marks")));
    }
    Ok(())
}

/// The first name `Row_<n>` that none of `taken` has.
fn fresh_name(taken: &[String], made: &[String]) -> String {
    let mut n = 1u64;
    loop {
        let name = format!("Row_{n}");
        if !taken.contains(&name) && !made.contains(&name) {
            return name;
        }
        n += 1;
    }
}

/// What a set made of a shape's links, for the command's answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetOutcome {
    /// Whether anything changed.
    pub changed: bool,
}

/// Sets the links of shape `id` on a page to `specs`. Refused, with the document as it was, for a name the shape has no row of, two links named alike, text a link may not hold,
/// no address and no sub-address, more than one default, more than [`MAX_LINKS`] links, and a shape that lists more rows than [`MAX_ROWS`].
pub fn set_links(doc: &mut Document, page: u32, id: u32, specs: &[LinkSpec]) -> Result<SetOutcome, Problem> {
    if specs.len() > MAX_LINKS {
        return Err(Problem::bad(format!("a shape holds at most {MAX_LINKS} hyperlinks")));
    }
    let at = require_shape(doc, page, id)?;
    let current = read_links_at(doc, &at)?;
    let names: Vec<String> = current.iter().map(|l| l.name.clone()).collect();
    // What each spec makes: the link as the row will hold it, and whether the row is new.
    let mut finals: Vec<(Link, bool)> = Vec::with_capacity(specs.len());
    let mut made: Vec<String> = Vec::new();
    for (n, spec) in specs.iter().enumerate() {
        let label = n + 1;
        for (what, text) in [
            ("description", &spec.description),
            ("address", &spec.address),
            ("sub_address", &spec.sub_address),
            ("extra_info", &spec.extra_info),
            ("frame", &spec.frame),
        ] {
            if let Some(text) = text {
                check_text(what, text).map_err(|p| Problem::bad(format!("link {label}: {}", p.message)))?;
            }
        }
        let (mut link, is_new) = match &spec.name {
            Some(name) => {
                let kept = current
                    .iter()
                    .find(|l| l.name == *name)
                    .ok_or_else(|| Problem::bad(format!("the shape has no link named {}", crate::error::shown(name))))?;
                if finals.iter().any(|(l, _)| l.name == *name) {
                    return Err(Problem::bad(format!("link {label}: {} is named twice", crate::error::shown(name))));
                }
                (kept.clone(), false)
            }
            None => {
                let name = fresh_name(&names, &made);
                made.push(name.clone());
                (Link { name, ..Link::default() }, true)
            }
        };
        if let Some(t) = &spec.description {
            link.description.clone_from(t);
        }
        if let Some(t) = &spec.address {
            link.address.clone_from(t);
        }
        if let Some(t) = &spec.sub_address {
            link.sub_address.clone_from(t);
        }
        if let Some(t) = &spec.extra_info {
            link.extra_info.clone_from(t);
        }
        if let Some(t) = &spec.frame {
            link.frame.clone_from(t);
        }
        link.new_window = spec.new_window.unwrap_or(link.new_window);
        link.default = spec.default.unwrap_or(link.default);
        link.invisible = spec.invisible.unwrap_or(link.invisible);
        if link.address.trim().is_empty() && link.sub_address.trim().is_empty() {
            return Err(Problem::bad(format!("link {label} needs an address or a sub-address")));
        }
        finals.push((link, is_new));
    }
    if finals.iter().filter(|(l, _)| l.default).count() > 1 {
        return Err(Problem::bad("only one link can be the default"));
    }
    let gone: Vec<&String> = names.iter().filter(|n| !finals.iter().any(|(l, _)| l.name == **n)).collect();
    // The cells that change, in the order of the links.
    let sheet = SheetRef::Shape(at.clone());
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let mut put = |name: &str, cell: &str, text: &str| {
        writes.push((sheet.clone(), CellAddr::in_row("Hyperlink", None, RowKey::name(name), cell), CellEdit::Value(Value::from_text(text))));
    };
    for (link, is_new) in &finals {
        let was = current.iter().find(|l| l.name == link.name);
        let strings = [
            ("Description", &link.description, was.map(|l| &l.description)),
            ("Address", &link.address, was.map(|l| &l.address)),
            ("SubAddress", &link.sub_address, was.map(|l| &l.sub_address)),
            ("ExtraInfo", &link.extra_info, was.map(|l| &l.extra_info)),
            ("Frame", &link.frame, was.map(|l| &l.frame)),
        ];
        for (cell, now, before) in strings {
            // A new row always has the three the dialog shows; the others only when they hold something.
            let always = *is_new && matches!(cell, "Description" | "Address" | "SubAddress");
            if before != Some(now) && (always || !now.is_empty() || before.is_some()) {
                put(&link.name, cell, now);
            }
        }
        for (cell, now, before) in [
            ("NewWindow", link.new_window, was.map(|l| l.new_window)),
            ("Default", link.default, was.map(|l| l.default)),
            ("Invisible", link.invisible, was.map(|l| l.invisible)),
        ] {
            if before != Some(now) && (now || before.is_some()) {
                put(&link.name, cell, if now { "1" } else { "0" });
            }
        }
    }
    if writes.is_empty() && gone.is_empty() {
        return Ok(SetOutcome { changed: false });
    }
    // The rows left out go first: the shape's own rows are taken away, and a row it takes from its master is marked deleted here.
    if !gone.is_empty() {
        let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        if shape.sheet.section(&key()).is_some() {
            let sheet = Arc::make_mut(&mut shape.sheet);
            let section = sheet.section_mut(&key()).map_err(|e| Problem::failed(e.to_string()))?;
            section.rows.retain(|row| !row.name.as_deref().is_some_and(|n| gone.iter().any(|g| g.as_str() == n)));
        }
        let still: Vec<String> = read_links_at(doc, &at)?.into_iter().map(|l| l.name).collect();
        for name in gone.iter().filter(|g| still.contains(g)) {
            let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
            let row: &mut Row = Arc::make_mut(&mut shape.sheet).row_mut(&key(), &RowKey::name(name)).map_err(|e| Problem::failed(e.to_string()))?;
            row.del = true;
        }
    }
    let cells: Vec<(SheetRef, CellAddr)> = writes.iter().map(|(s, a, _)| (s.clone(), a.clone())).collect();
    write_cells(doc, writes)?;
    // A section left with no row is no section.
    if let Some(shape) = shape_mut(doc, page, &at.path)
        && shape.sheet.section(&key()).is_some_and(|s| s.rows.is_empty() && s.cells.is_empty())
    {
        Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "Hyperlink");
    }
    super::settle::settle_cells(doc, page, cells)?;
    Ok(SetOutcome { changed: true })
}

/// What kind of address a link has, the way a click treats it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Web,
    Mail,
    /// A scheme a click does not follow.
    Other,
    /// A path: no scheme (or a single letter, a drive).
    File,
}

/// The scheme of an address and the text after its colon, if it starts with one: letters, digits, `+`, `-` and `.`, two characters or more (so that `C:` is a drive).
fn scheme_of(address: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = address.split_once(':')?;
    let ok = scheme.chars().count() >= 2
        && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    ok.then_some((scheme, rest))
}

fn classify(address: &str) -> Scheme {
    match scheme_of(address) {
        Some((s, _)) if ["http", "https", "ftp", "ftps"].iter().any(|w| s.eq_ignore_ascii_case(w)) => Scheme::Web,
        Some((s, _)) if s.eq_ignore_ascii_case("mailto") => Scheme::Mail,
        Some(_) => Scheme::Other,
        None => Scheme::File,
    }
}

/// The two hexadecimal digits that follow a percent sign, as the byte they stand for.
pub(crate) fn percent_byte(digits: &[u8]) -> Option<u8> {
    let hex = |b: u8| char::from(b).to_digit(16);
    match digits {
        [hi, lo, ..] => u8::try_from(hex(*hi)? * 16 + hex(*lo)?).ok(),
        _ => None,
    }
}

/// `text` with its percent sequences turned into the characters they stand for. `None` when a percent sign starts no sequence or the bytes are no text, which a strict reader refuses.
pub(crate) fn percent_decode(text: &str) -> Option<String> {
    percent_decode_bytes(text.as_bytes())
}

/// The bytes of a text with their percent sequences turned into the bytes they stand for, as a text: [`percent_decode`]'s work.
pub(crate) fn percent_decode_bytes(bytes: &[u8]) -> Option<String> {
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0usize;
    while let Some(&b) = bytes.get(at) {
        if b == b'%' {
            out.push(percent_byte(bytes.get(at + 1..)?)?);
            at += 3;
        } else {
            out.push(b);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether `text`, or what its percent sequences stand for, holds a control character or an invisible mark. **Strict**: a text whose percent sequences do not decode (a percent sign that starts
/// none, bytes that are no text) is `Err`, since a checker that read it as it was written would be looking at other text than the browser decodes.
pub(crate) fn hides_something(text: &str) -> Result<bool, ()> {
    let decoded = percent_decode(text).ok_or(())?;
    Ok(hides(text, &decoded))
}

/// Whether `text` or `decoded`, what its percent sequences stand for, holds a control character or an invisible mark.
pub(crate) fn hides(text: &str, decoded: &str) -> bool {
    text.chars().any(is_hidden) || decoded.chars().any(is_hidden)
}

/// `text` with each character `keep` does not allow written as the percent sequences of its bytes; a percent sign that starts a sequence is kept when `keep_sequences` is set.
pub(crate) fn percent_encode(text: &str, keep: impl Fn(char) -> bool, keep_sequences: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        let sequence = c == '%' && keep_sequences && bytes.get(at + 1..).and_then(percent_byte).is_some();
        if sequence || (c != '%' && keep(c)) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// The characters of an address that are written as they are: all but the space and `"<>\`{}|^\`, which no address holds.
fn address_safe(c: char) -> bool {
    !matches!(c, ' ' | '"' | '<' | '>' | '\\' | '`' | '{' | '}' | '|' | '^')
}

/// The characters of a fragment that are written as they are: RFC 3986's unreserved ones and the marks a fragment may hold.
fn fragment_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~!$&'()*+,;=:@/?".contains(c)
}

/// The characters of a mail link's recipient that are written as they are: **none of the marks that start or part the parameters** (`?`, `&`, `=`, `#`, `%`) and not the comma that parts the
/// recipients, which a recipient made of them would otherwise re-make a query or another recipient with.
fn recipient_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~!$'()*+;:@/".contains(c)
}

/// The characters of a mail parameter's value that are written as they are: the same, with the comma (a `cc` lists recipients), and **not the question mark** (a client could take it for the
/// start of another query).
fn value_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~!$'()*+,;:@/".contains(c)
}

/// The parameters of a mail link a click passes on: the subject, the body and the copies. Anything else, an attachment above all, is not passed.
const MAIL_PARAMS: [&str; 4] = ["subject", "body", "cc", "bcc"];

/// A mail link: recipients, and `subject`, `body`, `cc` and `bcc` after a question mark. Each recipient and value is read as text (a percent sequence that is none, bytes that
/// are no text, a control character or an invisible mark refuse the link) and **written again** in the form this module gives, so that what reaches the mail client is exactly what was
/// checked. A parameter that is not one of the four, an `attach` or an `attachment` among them, is refused, and so is an empty one.
fn follow_mail(address: &str, rest: &str) -> Result<Target, Problem> {
    let refuse = |why: String| Problem::failed(format!("the link's mail address {} is not followed: {why}", crate::error::shown(address)));
    if address.chars().any(is_hidden) {
        return Err(refuse("it holds control characters or invisible marks".to_owned()));
    }
    let (recipients, query) = match rest.split_once('?') {
        Some((to, query)) => (to, Some(query)),
        None => (rest, None),
    };
    if recipients.trim().is_empty() && query.is_none_or(|q| q.trim().is_empty()) {
        return Err(refuse("it names no one".to_owned()));
    }
    let text = |part: &str, what: &str| -> Result<String, Problem> {
        let decoded = percent_decode(part)
            .ok_or_else(|| refuse(format!("its {what} has a percent sign that starts no sequence, or bytes that are no text")))?;
        if decoded.chars().any(is_hidden) {
            return Err(refuse(format!("its {what} holds control characters or invisible marks")));
        }
        Ok(decoded)
    };
    let mut url = String::from("mailto:");
    let mut people: Vec<String> = Vec::new();
    for person in recipients.split(',').filter(|r| !r.trim().is_empty()) {
        people.push(percent_encode(text(person.trim(), "recipient")?.trim(), recipient_safe, false));
    }
    url.push_str(&people.join(","));
    let mut params: Vec<String> = Vec::new();
    for part in query.into_iter().flat_map(|q| q.split('&')) {
        if part.is_empty() {
            return Err(refuse("it has an empty parameter".to_owned()));
        }
        let Some((name, value)) = part.split_once('=') else {
            return Err(refuse(format!("its parameter {} has no value", crate::error::shown(part))));
        };
        let lower = text(name, "parameter name")?.trim().to_ascii_lowercase();
        if !MAIL_PARAMS.contains(&lower.as_str()) {
            return Err(refuse(format!(
                "its parameter {} is not passed on: a click passes on only subject, body, cc and bcc",
                crate::error::shown(&lower)
            )));
        }
        params.push(format!("{lower}={}", percent_encode(&text(value, &format!("{lower} value"))?, value_safe, false)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    Ok(Target::Mail { url })
}

/// How an address is followed, or why it is not: a web or FTP address is written with the characters no address holds as percent sequences, and the sub-address is
/// its fragment when the address has none, **held to the rules of the address** (no control character or invisible mark, also as percent sequences) and written with every character that is not
/// safe in a fragment as percent sequences; a mail address is read and written again by [`follow_mail`]. No other scheme, and no path, is followed.
fn follow_address(address: &str, sub_address: &str) -> Result<Target, Problem> {
    let address = address.trim();
    // A mail address is looked at by `follow_mail`, in its own words.
    if classify(address) != Scheme::Mail && address.chars().any(is_hidden) {
        return Err(Problem::failed("the link's address holds control characters or invisible marks, and is not followed"));
    }
    match classify(address) {
        Scheme::Web => {
            // The browser decodes the percent sequences, so what is looked at is what they stand for, and an address whose do not decode is not followed.
            match hides_something(address) {
                Err(()) => {
                    return Err(Problem::failed(
                        "the link's address has a percent sign that starts no sequence, or bytes that are no text, and is not followed",
                    ));
                }
                Ok(true) => {
                    return Err(Problem::failed(
                        "the link's address holds control characters or invisible marks, also as percent sequences, and is not followed",
                    ));
                }
                Ok(false) => {}
            }
            let rest = scheme_of(address).map_or("", |(_, r)| r);
            let host = rest.strip_prefix("//").unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
            if host.is_empty() {
                return Err(Problem::failed(format!("the link's address {} names no host, and is not followed", crate::error::shown(address))));
            }
            let base = percent_encode(address, address_safe, true);
            let sub = sub_address.trim();
            if sub.is_empty() || address.contains('#') {
                return Ok(Target::Web { url: base });
            }
            let decoded = percent_decode(sub).ok_or_else(|| {
                Problem::failed("the link's sub-address has a percent sign that starts no sequence, or bytes that are no text, and is not followed")
            })?;
            if sub.chars().any(is_hidden) || decoded.chars().any(is_hidden) {
                return Err(Problem::failed(
                    "the link's sub-address holds control characters or invisible marks, also as percent sequences, and is not followed",
                ));
            }
            Ok(Target::Web { url: format!("{base}#{}", percent_encode(sub, fragment_safe, true)) })
        }
        Scheme::Mail => follow_mail(address, scheme_of(address).map_or("", |(_, r)| r)),
        Scheme::Other => Err(Problem::failed(format!(
            "the link's address {} is not followed: a click follows web (http, https), FTP and mail (mailto) addresses and the links of this drawing, and opens no file or program",
            crate::error::shown(address)
        ))),
        Scheme::File => Err(Problem::failed(format!(
            "the link's address {} is a file or a path, and is not followed: a click opens no file or program (the links of this drawing and web, FTP and mail addresses are followed)",
            crate::error::shown(address)
        ))),
    }
}

/// The shape of a page that a name or `Sheet.<id>` names.
fn shape_named(doc: &Document, page: u32, text: &str) -> Option<u32> {
    if let Some(number) = text.strip_prefix("Sheet.").and_then(|n| n.parse::<u32>().ok())
        && doc.find_shape(Owner::Page(page), number).is_some()
    {
        return Some(number);
    }
    let top: &ShapeList = doc.shapes_of(Owner::Page(page))?;
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
    let mut seen = 0usize;
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(shape) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        seen += 1;
        if seen > diacraft_doc::MAX_SHAPES_PER_PAGE {
            return None;
        }
        if shape.name.as_deref() == Some(text) || shape.name_u.as_deref() == Some(text) {
            return Some(shape.id);
        }
        if stack.len() < diacraft_doc::MAX_SHAPE_DEPTH {
            stack.push((&shape.children, 0));
        }
    }
    None
}

/// Where a link of this drawing goes: the page its sub-address names, or the shape of a page it names after a slash.
fn in_drawing(doc: &Document, sub_address: &str) -> Result<Target, Problem> {
    let text = sub_address.trim();
    if text.chars().any(is_hidden) {
        return Err(Problem::failed("the link's sub-address holds control characters or invisible marks, and is not followed"));
    }
    let page_named = |name: &str| doc.pages.iter().find(|p| &*p.name == name).map(|p| p.id);
    if let Some(page) = page_named(text) {
        return Ok(Target::Page { page });
    }
    // Every slash may be the one that ends the page's name, so the shape is the text after it; a page's name may hold a slash itself.
    for (at, _) in text.match_indices('/').take(64) {
        let (left, right) = (text.get(..at).unwrap_or(""), text.get(at + 1..).unwrap_or(""));
        if let Some(page) = page_named(left) {
            return match shape_named(doc, page, right) {
                Some(shape) => Ok(Target::Shape { page, shape }),
                None => Err(Problem::failed(format!("page {} has no shape {}", crate::error::shown(left), crate::error::shown(right)))),
            };
        }
    }
    Err(Problem::failed(format!("this drawing has no page named {}", crate::error::shown(text))))
}

/// Where `link` goes. The links of this drawing are found in `doc`.
pub fn resolve(doc: &Document, link: &Link) -> Result<Target, Problem> {
    if link.address.trim().is_empty() {
        if link.sub_address.trim().is_empty() {
            return Err(Problem::failed("the link has no address and no sub-address"));
        }
        return in_drawing(doc, &link.sub_address);
    }
    follow_address(&link.address, &link.sub_address)
}

/// The kind of a link for a listing: `page` and `shape` (found in this drawing), `missing` (a link of this drawing that finds nothing), `web`, `mail`, `file` (a path), `other` (a scheme a click
/// does not follow) and `none`.
pub fn kind_of(doc: &Document, link: &Link) -> &'static str {
    if link.address.trim().is_empty() {
        if link.sub_address.trim().is_empty() {
            return "none";
        }
        return match in_drawing(doc, &link.sub_address) {
            Ok(Target::Page { .. }) => "page",
            Ok(_) => "shape",
            Err(_) => "missing",
        };
    }
    match classify(link.address.trim()) {
        Scheme::Web => "web",
        Scheme::Mail => "mail",
        Scheme::Other => "other",
        Scheme::File => "file",
    }
}

/// The link a click follows: the default one, else the first that is not hidden, else the first; or the one named `name`.
pub fn pick<'a>(links: &'a [Link], name: Option<&str>) -> Result<&'a Link, Problem> {
    match name {
        Some(name) => {
            links.iter().find(|l| l.name == name).ok_or_else(|| Problem::bad(format!("the shape has no link named {}", crate::error::shown(name))))
        }
        None => links
            .iter()
            .find(|l| l.default)
            .or_else(|| links.iter().find(|l| !l.invisible))
            .or_else(|| links.first())
            .ok_or_else(|| Problem::failed("the shape has no hyperlink")),
    }
}
