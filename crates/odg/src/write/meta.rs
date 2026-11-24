//! What an edited save writes of the package's own bookkeeping: `meta:generator`, the thumbnail and the `mimetype` entry
//! (and P7).
//!
//! * **`meta.xml`'s generator** is DiaCraft's own string, `DiaCraft/` and the version, on any save that writes: the text of the element
//!   `meta:generator` is replaced where there is one, and an element is added to `office:meta` where there is none. Nothing else of `meta.xml` is
//!   touched: the creation date, the title and every other field keep their bytes. **The generator is the save's own and no part of the drawing**, and so is the thumbnail; the
//!   modification date, the editing cycles and the duration are not renewed. The model's package data follows the file where a model writer needs it (`package_data.rs`). An unedited save leaves `meta.xml` as it is. A package
//!   with no `meta.xml` gains a small one holding the generator, with the `office:version` the drawing has.
//! * **The thumbnail** is the PNG the caller passes, written as `Thumbnails/thumbnail.png` in place of the one the package has,
//!   or added with a manifest row `image/png`; an unedited save keeps the old bytes, and an edited save passed no picture keeps them too. The
//!   crate renders nothing.
//! * **`mimetype`** is written first, stored and with no extra field, on an edited save of a package whose `mimetype` was not so:
//!   absent, not first, deflated, with an extra field or unreadable. The media type is the one the package says it is.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::xml_scan::{Tag, TagKind, scan_tags, tag_attributes};

use super::count::{self, Counted};
use super::splice::{Splicer, escape_value};
use super::{WriteError, WriteWork};
use crate::limits::Limits;

/// The generator DiaCraft writes: the application and its version, in the form of a user-agent string.
pub(crate) const GENERATOR: &str = concat!("DiaCraft/", env!("CARGO_PKG_VERSION"));

/// The thumbnail's name in the package.
pub(crate) const THUMBNAIL: &str = "Thumbnails/thumbnail.png";

const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const META_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";

/// The PNG signature: the first eight bytes of every PNG.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Whether `bytes` are a thumbnail the writer accepts: a PNG, and no larger than [`Limits::max_thumbnail_bytes`].
pub(crate) fn check_thumbnail(bytes: &[u8], limits: &Limits) -> Result<(), WriteError> {
    if bytes.len() > limits.max_thumbnail_bytes {
        return Err(WriteError::Limit { what: "bytes of the thumbnail", max: limits.max_thumbnail_bytes as u64 });
    }
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(WriteError::Thumbnail("the picture passed as the thumbnail is not a PNG"));
    }
    Ok(())
}

/// An element of `meta.xml` that the scan notes: where its tags are.
#[derive(Clone, Debug)]
struct Element {
    name: String,
    /// Where the element's unit starts: the white space before it when that is all that stands between it and what came before, else its own start.
    lead: usize,
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
    empty: bool,
    /// Its end tag has been seen (an empty element has none).
    closed: bool,
}

/// What `meta.xml` holds of what is looked for.
#[derive(Default)]
struct Found {
    /// The root, and the prefixes bound on its own tag: where a flat drawing without `office:meta` gets one.
    root: Option<Element>,
    root_bound: HashSet<Counted<String>>,
    root_meta_prefix: Option<String>,
    /// The first child of the root in DiaCraft's namespace, and how many there are.
    extension: Option<Element>,
    meta: Option<Element>,
    generator: Option<Element>,
    /// The prefix that names the meta namespace where `office:meta` is, and the prefixes bound there (to choose a new one by).
    meta_prefix: Option<String>,
    bound: HashSet<Counted<String>>,
}

struct Walker<'a> {
    bytes: &'a [u8],
    found: Found,
    /// Each prefix in force, to the URIs bound to it, the innermost last (one lookup of a prefix is one probe, however many are declared), and the
    /// prefixes each open element declared.
    bound: HashMap<Counted<String>, Vec<String>>,
    made: Vec<Vec<String>>,
    spans: Vec<diacraft_container::xml_scan::AttrSpan>,
    /// Looks in the prefixes in force: one for each prefix looked up, bound, unbound or collected (counted in [`WriteWork::meta_probes`]).
    probes: Cell<u64>,
}

impl Walker<'_> {
    /// The namespace URI of `prefix` here.
    fn uri(&self, prefix: &str) -> Option<&str> {
        self.probes.set(self.probes.get().saturating_add(1));
        self.bound.get(&Counted(prefix.to_owned())).and_then(|stack| stack.last()).map(String::as_str)
    }

    /// Whether the element `name` is `local` of the namespace `uri` here; an empty `local` is any element of the namespace.
    fn is(&self, name: &str, uri: &str, local: &str) -> bool {
        let (prefix, l) = name.split_once(':').unwrap_or(("", name));
        (local.is_empty() || l == local) && self.uri(prefix) == Some(uri)
    }

    /// Reads the tag's declarations into the prefixes in force, and gives the prefixes it declared.
    fn declare(&mut self, tag: &Tag<'_>) -> Vec<String> {
        self.spans.clear();
        let _readable = tag_attributes(self.bytes, tag.start, tag.end, &mut self.spans);
        let mut made = Vec::new();
        for a in &self.spans {
            let name = String::from_utf8_lossy(self.bytes.get(a.name.0..a.name.1).unwrap_or_default());
            let prefix = if name == "xmlns" { Some(String::new()) } else { name.strip_prefix("xmlns:").map(str::to_owned) };
            if let Some(prefix) = prefix {
                let uri = crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default());
                self.probes.set(self.probes.get().saturating_add(1));
                self.bound.entry(Counted(prefix.clone())).or_default().push(uri);
                made.push(prefix);
            }
        }
        made
    }

    /// Undoes the declarations an element made.
    fn undeclare(&mut self, made: Vec<String>) {
        for prefix in made {
            self.probes.set(self.probes.get().saturating_add(1));
            if let Some(stack) = self.bound.get_mut(&Counted(prefix)) {
                stack.pop();
            }
        }
    }

    fn visit(&mut self, tag: &Tag<'_>) {
        if tag.kind == TagKind::Close {
            let made = self.made.pop().unwrap_or_default();
            self.undeclare(made);
            self.closed(tag);
            return;
        }
        let made = self.declare(tag);
        let empty = tag.kind == TagKind::Empty;
        let lead = match tag.text_before {
            Some((from, to)) if diacraft_container::xml_scan::is_blank(self.bytes.get(from..to).unwrap_or_default()) => from,
            _ => tag.start,
        };
        let element = || Element {
            name: tag.name.to_owned(),
            lead,
            start: tag.start,
            open_end: tag.end,
            close_start: tag.end,
            end: tag.end,
            empty,
            closed: empty,
        };
        if tag.depth == 0 {
            self.found.root = Some(element());
            self.found.root_bound = self.bound.iter().filter(|(_, stack)| !stack.is_empty()).map(|(p, _)| p.clone()).collect();
            self.found.root_meta_prefix = self
                .bound
                .iter()
                .filter(|(p, stack)| !p.0.is_empty() && stack.last().map(String::as_str) == Some(META_URI))
                .map(|(p, _)| p.0.clone())
                .min();
        }
        if tag.depth == 1
            && self.found.extension.is_none()
            && (self.is(tag.name, diacraft_doc::DIACRAFT_URN, "") || self.is(tag.name, diacraft_doc::DIACRAFT_URN_LEGACY, ""))
        {
            self.found.extension = Some(element());
        }
        if tag.depth == 1 && self.found.meta.is_none() && self.is(tag.name, OFFICE_URI, "meta") {
            self.found.meta = Some(element());
            // The prefixes in force are looked through twice, once for the set and once for the least: each prefix twice.
            self.probes.set(self.probes.get().saturating_add(2 * self.bound.len() as u64));
            self.found.bound = self.bound.iter().filter(|(_, stack)| !stack.is_empty()).map(|(p, _)| p.clone()).collect();
            self.found.meta_prefix = self
                .bound
                .iter()
                .filter(|(p, stack)| !p.0.is_empty() && stack.last().map(String::as_str) == Some(META_URI))
                .map(|(p, _)| p.0.clone())
                .min();
        } else if tag.depth == 2
            && self.found.meta.as_ref().is_some_and(|m| !m.closed)
            && self.found.generator.is_none()
            && self.is(tag.name, META_URI, "generator")
        {
            self.found.generator = Some(element());
        }
        if empty {
            self.undeclare(made);
        } else {
            self.made.push(made);
        }
    }

    /// Notes where the end tag of a noted element is.
    fn closed(&mut self, tag: &Tag<'_>) {
        let note = |slot: &mut Option<Element>| {
            if let Some(e) = slot.as_mut().filter(|e| !e.closed) {
                e.close_start = tag.start;
                e.end = tag.end;
                e.closed = true;
            }
        };
        if tag.depth == 0 {
            note(&mut self.found.root);
        } else if tag.depth == 1 {
            note(&mut self.found.meta);
            note(&mut self.found.extension);
        } else if tag.depth == 2 {
            note(&mut self.found.generator);
        }
    }
}

/// A start tag with the slash of an empty element taken out: the text of the tag up to the `>` that ends it.
fn opened(bytes: &[u8], e: &Element) -> String {
    let raw = String::from_utf8_lossy(bytes.get(e.start..e.open_end).unwrap_or_default());
    format!("{}>", raw.trim_end_matches('>').trim_end_matches('/').trim_end())
}

/// Walks `bytes` once for what is looked for. The scan is charged to the save's bytes before it starts; whether the part scanned is told apart from what was found.
fn look(bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<(Found, bool), WriteError> {
    work.charge_scan(bytes.len(), limits)?;
    let mut walker = Walker { bytes, found: Found::default(), bound: HashMap::new(), made: Vec::new(), spans: Vec::new(), probes: Cell::new(0) };
    let mut container = Work::default();
    let compared_before = count::compared();
    let scanned = scan_tags::<()>(bytes, &limits.container.xml, &mut container, |tag| {
        walker.visit(tag);
        Ok(())
    });
    work.package.scan_events = work.package.scan_events.saturating_add(container.xml_events);
    work.elements_walked = work.elements_walked.saturating_add(container.xml_events);
    work.meta_probes = work.meta_probes.saturating_add(walker.probes.get());
    work.meta_compares = work.meta_compares.saturating_add(count::compared().saturating_sub(compared_before));
    Ok((walker.found, scanned.is_ok()))
}

/// The edit that writes DiaCraft's generator into `office:meta` at `meta`: the text replaced where there is a generator, an element added where there is none.
fn put_generator(bytes: &[u8], found: &Found, meta: &Element, splicer: &mut Splicer<'_>, work: &mut WriteWork) {
    let text = escape_value(GENERATOR, b'"');
    match &found.generator {
        Some(g) if !g.empty => splicer.replace(g.open_end, g.close_start, text),
        Some(g) => splicer.replace(g.start, g.end, format!("{}{text}</{}>", opened(bytes, g), g.name)),
        None => {
            // An element of our own, in the prefix the file binds to the meta namespace, or in one declared on it.
            let (prefix, declare) = match &found.meta_prefix {
                Some(p) => (p.clone(), String::new()),
                None => (fresh_meta_prefix(&found.bound, work), String::new()),
            };
            let declare = if found.meta_prefix.is_some() { declare } else { format!(" xmlns:{prefix}=\"{META_URI}\"") };
            let element = format!("<{prefix}:generator{declare}>{text}</{prefix}:generator>");
            if meta.empty {
                splicer.replace(meta.start, meta.end, format!("{}{element}</{}>", opened(bytes, meta), meta.name));
            } else {
                splicer.insert(meta.open_end, element);
            }
        }
    }
}

/// A prefix for the meta namespace that no element in scope binds: `meta`, or `meta` and the first number that is free. Each prefix looked at is a probe.
fn fresh_meta_prefix(bound: &HashSet<Counted<String>>, work: &mut WriteWork) -> String {
    (0usize..)
        .map(|n| if n == 0 { "meta".to_owned() } else { format!("meta{n}") })
        .find(|c| {
            work.meta_probes = work.meta_probes.saturating_add(1);
            let before = count::compared();
            let taken = bound.contains(&Counted(c.clone()));
            work.meta_compares = work.meta_compares.saturating_add(count::compared().saturating_sub(before));
            !taken
        })
        .unwrap_or_else(|| "meta".to_owned())
}

/// `meta.xml` with the generator DiaCraft's. `None` when the part cannot be edited (it is not scanned, or has no `office:meta`): the caller keeps
/// it as it is and says so. The part is edited whatever its size (the reader's cap on `meta.xml` is for what it reads, and a generator another
/// producer wrote must not stay in a file DiaCraft saved, ODF 1.3 section 4.3.2.1): the scan is linear and charged to the
/// save's bytes, so a part past what the save may scan is refused as `write_limit`.
pub(crate) fn with_generator(bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<Option<Vec<u8>>, WriteError> {
    let (found, scanned) = look(bytes, limits, work)?;
    let Some(meta) = found.meta.clone().filter(|_| scanned) else { return Ok(None) };
    let mut splicer = Splicer::new(bytes);
    put_generator(bytes, &found, &meta, &mut splicer, work);
    Ok(Some(splicer.apply(work, limits)?))
}

/// Where the root of a flat drawing and its children of interest are, for the edits of a save that are not a shape's or a style's:
/// the root's start tag and end tag, the prefixes bound on it, and DiaCraft's extension.
pub(crate) struct FlatPlan<'a> {
    /// The edits made so far, against the document's bytes.
    pub splicer: Splicer<'a>,
    /// Where the root's start tag ends and where its end tag starts.
    pub root_open_end: usize,
    pub root_close_start: usize,
    /// The prefixes bound on the root's own tag, with their URIs where a caller asks (`bound_to`).
    pub root_bound: HashSet<String>,
    /// DiaCraft's extension: the first child in its namespace, as where its unit starts (the white space before it), where its element starts and where it ends.
    pub extension: Option<(usize, usize, usize)>,
}

/// Plans the flat drawing `bytes`: with the generator written (`generator`), `office:meta` made as the root's first child when there is none (holding only
/// `meta:generator`, with the meta namespace declared on it when the root does not declare it); and the places of the root and of the extension. `None` when the
/// document does not scan.
pub(crate) fn plan_flat<'a>(bytes: &'a [u8], generator: bool, limits: &Limits, work: &mut WriteWork) -> Result<Option<FlatPlan<'a>>, WriteError> {
    let (found, scanned) = look(bytes, limits, work)?;
    let Some(root) = found.root.clone().filter(|_| scanned) else { return Ok(None) };
    let mut splicer = Splicer::new(bytes);
    if generator {
        match &found.meta {
            Some(meta) => put_generator(bytes, &found, meta, &mut splicer, work),
            None => {
                let text = escape_value(GENERATOR, b'"');
                let office = root.name.split_once(':').map_or(String::new(), |(p, _)| format!("{p}:"));
                let (prefix, declare) = match &found.root_meta_prefix {
                    Some(p) => (p.clone(), String::new()),
                    None => {
                        let free = fresh_meta_prefix(&found.root_bound, work);
                        let declaration = format!(" xmlns:{free}=\"{META_URI}\"");
                        (free, declaration)
                    }
                };
                splicer.insert(root.open_end, format!("<{office}meta{declare}><{prefix}:generator>{text}</{prefix}:generator></{office}meta>"));
            }
        }
    }
    let root_bound = found.root_bound.iter().map(|c| c.0.clone()).collect();
    let extension = found.extension.as_ref().map(|e| (e.lead, e.start, e.end));
    Ok(Some(FlatPlan { splicer, root_open_end: root.open_end, root_close_start: root.close_start, root_bound, extension }))
}

/// A `meta.xml` made for a package that has none, with the `office:version` of the drawing, and none when the drawing declares none (a save never raises the
/// version a package declares).
pub(crate) fn new_meta(version: Option<&str>) -> String {
    let version = version.map_or_else(String::new, |v| format!(" office:version=\"{}\"", escape_value(v, b'"')));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"{OFFICE_URI}\" xmlns:meta=\"{META_URI}\"{version}><office:meta><meta:generator>{}</meta:generator></office:meta></office:document-meta>\n",
        escape_value(GENERATOR, b'"')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part with its generator written, or `None` when the part cannot be edited.
    fn written(part: &str) -> Option<String> {
        match with_generator(part.as_bytes(), &Limits::default(), &mut WriteWork::default()) {
            Ok(Some(bytes)) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            _ => None,
        }
    }

    /// A `meta.xml` whose root declares `declarations` and holds `body`.
    fn part(declarations: &str, body: &str) -> String {
        format!("<office:document-meta xmlns:office=\"{OFFICE_URI}\"{declarations}>{body}</office:document-meta>")
    }

    fn meta_ns() -> String {
        format!(" xmlns:meta=\"{META_URI}\"")
    }

    fn generator() -> String {
        format!("<meta:generator>{GENERATOR}</meta:generator>")
    }

    #[test]
    fn a_generator_is_replaced_where_there_is_one_and_added_where_there_is_not() {
        let ns = meta_ns();
        let g = generator();
        // Replaced, in either form of an element; nothing else of the part moves.
        let old = part(
            &ns,
            "<office:meta><meta:keyword>k</meta:keyword><meta:generator>Other/9</meta:generator><meta:keyword>j</meta:keyword></office:meta>",
        );
        assert_eq!(
            written(&old),
            Some(part(&ns, &format!("<office:meta><meta:keyword>k</meta:keyword>{g}<meta:keyword>j</meta:keyword></office:meta>")))
        );
        let empty = part(&ns, "<office:meta><meta:generator/></office:meta>");
        assert_eq!(written(&empty), Some(part(&ns, &format!("<office:meta>{g}</office:meta>"))));
        let spaced = part(&ns, "<office:meta><meta:generator a=\"1\" /></office:meta>");
        assert_eq!(written(&spaced), Some(part(&ns, &format!("<office:meta><meta:generator a=\"1\">{GENERATOR}</meta:generator></office:meta>"))));
        // Added at the start of `office:meta`, in the prefix the file binds, or into an empty `office:meta`.
        let none = part(&ns, "<office:meta><meta:keyword>k</meta:keyword></office:meta>");
        assert_eq!(written(&none), Some(part(&ns, &format!("<office:meta>{g}<meta:keyword>k</meta:keyword></office:meta>"))));
        let bare = part(&ns, "<office:meta/>");
        assert_eq!(written(&bare), Some(part(&ns, &format!("<office:meta>{g}</office:meta>"))));
        let other_prefix = part(&format!(" xmlns:z=\"{META_URI}\""), "<office:meta></office:meta>");
        assert_eq!(
            written(&other_prefix),
            Some(part(&format!(" xmlns:z=\"{META_URI}\""), &format!("<office:meta><z:generator>{GENERATOR}</z:generator></office:meta>")))
        );
    }

    #[test]
    fn the_prefix_of_an_added_generator_is_the_least_one_bound_to_the_namespace_or_a_free_one_declared_on_it() {
        let body = "<office:meta></office:meta>";
        let declared = |prefix: &str| format!("<{prefix}:generator xmlns:{prefix}=\"{META_URI}\">{GENERATOR}</{prefix}:generator>");
        // The least of the prefixes bound to the namespace, and never the default namespace's.
        let two = format!(" xmlns:b=\"{META_URI}\" xmlns:a=\"{META_URI}\"");
        assert_eq!(written(&part(&two, body)), Some(part(&two, &format!("<office:meta><a:generator>{GENERATOR}</a:generator></office:meta>"))));
        let default = format!(" xmlns=\"{META_URI}\"");
        assert_eq!(written(&part(&default, body)), Some(part(&default, &format!("<office:meta>{}</office:meta>", declared("meta")))));
        // None bound: `meta`, declared on the element; or `meta1` when `meta` is bound to another namespace in force there, `meta2` after it.
        assert_eq!(written(&part("", body)), Some(part("", &format!("<office:meta>{}</office:meta>", declared("meta")))));
        let taken = " xmlns:meta=\"urn:other\"";
        assert_eq!(written(&part(taken, body)), Some(part(taken, &format!("<office:meta>{}</office:meta>", declared("meta1")))));
        let taken2 = " xmlns:meta=\"urn:other\" xmlns:meta1=\"urn:another\"";
        assert_eq!(written(&part(taken2, body)), Some(part(taken2, &format!("<office:meta>{}</office:meta>", declared("meta2")))));
        // A binding of `meta` that is out of force where `office:meta` is, because the element that made it is closed, takes nothing: the empty
        // element and the one with an end tag, and the one the other is nested in.
        for sibling in [
            "<x:s xmlns:x=\"urn:x\" xmlns:meta=\"urn:other\"/>",
            "<x:s xmlns:x=\"urn:x\" xmlns:meta=\"urn:other\"></x:s>",
            "<x:s xmlns:x=\"urn:x\" xmlns:meta=\"urn:other\"><x:t xmlns:meta=\"urn:third\"/></x:s>",
        ] {
            let text = part("", &format!("{sibling}{body}"));
            assert_eq!(written(&text), Some(part("", &format!("{sibling}<office:meta>{}</office:meta>", declared("meta")))), "{sibling}");
        }
        // A binding made on `office:meta` itself is in force inside it.
        let on_meta = part("", "<office:meta xmlns:meta=\"urn:other\"></office:meta>");
        assert_eq!(written(&on_meta), Some(part("", &format!("<office:meta xmlns:meta=\"urn:other\">{}</office:meta>", declared("meta1")))));
    }

    #[test]
    fn it_is_the_first_office_meta_below_the_root_and_the_first_generator_in_it_that_count() {
        let ns = meta_ns();
        let g = generator();
        // A sibling before it, an `office:meta` nested in another element, and a second `office:meta` after it are none of them the one.
        let text = part(&ns, "<x:s xmlns:x=\"urn:x\"><office:meta></office:meta></x:s><office:meta></office:meta><office:meta></office:meta>");
        assert_eq!(
            written(&text),
            Some(part(
                &ns,
                &format!("<x:s xmlns:x=\"urn:x\"><office:meta></office:meta></x:s><office:meta>{g}</office:meta><office:meta></office:meta>")
            ))
        );
        let sibling = part(&ns, "<x:s xmlns:x=\"urn:x\"/><office:meta></office:meta>");
        assert_eq!(written(&sibling), Some(part(&ns, &format!("<x:s xmlns:x=\"urn:x\"/><office:meta>{g}</office:meta>"))));
        // The first generator, at the depth of `office:meta`'s children and in the meta namespace; a deeper one or another namespace's is not it.
        let text = part(
            &ns,
            "<office:meta><x:generator xmlns:x=\"urn:x\"/><meta:other><meta:generator>deep</meta:generator></meta:other><meta:generator>one</meta:generator><meta:generator>two</meta:generator></office:meta>",
        );
        assert_eq!(
            written(&text),
            Some(part(
                &ns,
                &format!(
                    "<office:meta><x:generator xmlns:x=\"urn:x\"/><meta:other><meta:generator>deep</meta:generator></meta:other>{g}<meta:generator>two</meta:generator></office:meta>"
                )
            ))
        );
        // A generator outside `office:meta`, after it, is not one of its children.
        let outside = part(&ns, "<office:meta></office:meta><meta:generator>out</meta:generator>");
        assert_eq!(written(&outside), Some(part(&ns, &format!("<office:meta>{g}</office:meta><meta:generator>out</meta:generator>"))));
    }

    #[test]
    fn a_part_that_cannot_be_edited_is_left_as_it_is() {
        let ns = meta_ns();
        assert_eq!(written(&part(&ns, "<office:body/>")), None, "no office:meta");
        assert_eq!(written(&part(&ns, "<office:meta><meta:keyword></office:meta>")), None, "not well formed");
        let text = part(&ns, "<office:meta></office:meta>");
        // The reader's cap on a small part is no cap on the edit: a part past it is edited, and one past what a save may scan is refused.
        let tight = Limits { max_small_part_bytes: text.len() as u64 - 1, ..Limits::default() };
        assert!(matches!(with_generator(text.as_bytes(), &tight, &mut WriteWork::default()), Ok(Some(_))), "past the small-part cap");
        let room = Limits { max_write_bytes: 2 * text.len() as u64 + 100, ..Limits::default() };
        assert!(matches!(with_generator(text.as_bytes(), &room, &mut WriteWork::default()), Ok(Some(_))), "room to scan it and to copy it made");
        let less = Limits { max_write_bytes: text.len() as u64 - 1, ..Limits::default() };
        assert!(
            matches!(with_generator(text.as_bytes(), &less, &mut WriteWork::default()), Err(WriteError::Limit { .. })),
            "a part past what a save may scan"
        );
        // The scan is charged: bytes scanned and the events walked.
        let mut work = WriteWork::default();
        assert!(with_generator(text.as_bytes(), &Limits::default(), &mut work).is_ok());
        assert_eq!(work.scan_bytes, text.len() as u64);
        assert_eq!(work.elements_walked, 5, "the root, office:meta, its end, the root's end, and the end of the input");
        assert_eq!(work.package.scan_events, 5);
    }

    #[test]
    fn a_meta_part_made_for_a_package_that_has_none_holds_the_generator_and_the_version() {
        let text = new_meta(Some("1.3"));
        assert!(text.contains("office:version=\"1.3\"") && text.contains(&generator()), "{text}");
        assert!(new_meta(Some("1.\"2")).contains("office:version=\"1.&quot;2\""), "the version is escaped");
        assert!(!new_meta(None).contains("office:version"), "a drawing that declares none gets a part that declares none");
        assert_eq!(written(&text).map(|t| t.contains(&generator())), Some(true), "and is a part the writer reads again");
    }

    #[test]
    fn a_thumbnail_is_a_png_of_at_most_the_limit() {
        let limits = Limits { max_thumbnail_bytes: 12, ..Limits::default() };
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&[1, 2, 3, 4]);
        assert!(check_thumbnail(&png, &limits).is_ok(), "twelve bytes at a limit of twelve");
        png.push(5);
        assert!(matches!(check_thumbnail(&png, &limits), Err(WriteError::Limit { max: 12, .. })));
        assert!(matches!(check_thumbnail(b"GIF89a....", &limits), Err(WriteError::Thumbnail(_))));
        assert!(matches!(check_thumbnail(&PNG_SIGNATURE[..7], &limits), Err(WriteError::Thumbnail(_))), "a signature cut short");
        assert!(check_thumbnail(&PNG_SIGNATURE, &limits).is_ok());
    }
}
