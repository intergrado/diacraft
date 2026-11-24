//! A package written as a flat document: the parts of a package become the children of one
//! `office:document`, in the order, and its pictures go inline.
//!
//! Nothing is written again from a model: every child is a span of the part it was in, **copied as it was**, so what the flat reader reads of the result is what the
//! package reader read of the package, and the writer then splices the model's edits into the document as it splices any flat document
//! ([`super::flat`]).
//!
//! * `office:meta` is `meta.xml`'s, `office:settings` is `settings.xml`'s, `office:scripts` and `office:body` are `content.xml`'s, `office:styles` and
//!   `office:master-styles` are `styles.xml`'s, and `office:font-face-decls` and `office:automatic-styles` are both parts' at once: their contents are joined, `styles.xml`'s
//!   first. The root declares every namespace the parts' roots declare, in the order they first appear, and carries `office:version` as the first part that
//!   states one does (never raised), and `office:mimetype` as the package states it.
//! * **A picture goes inline**: a `draw:image` or a `draw:fill-image` whose `xlink:href` names a file of the package loses the href and holds the picture as
//!   `office:binary-data`, base64 in lines of 72 characters; a `draw:image` also gains `draw:mime-type` (the manifest's media type, else the picture's own signature),
//!   and a `draw:fill-image` does not (name that attribute for `draw:image` only).
//! * **What is refused, in words** (Q8 settles no rule for them): a part whose root holds what a flat document has no place for (a second child of one name, a
//!   foreign child or one of another part's kind, an attribute that is neither a namespace declaration nor `office:version`), an image with inline data beside an
//!   href that names a picture of the package, the two parts' automatic styles share a name (the flat document holds one container, and a name
//!   in both would be two styles of one name: renaming would have to rewrite every reference), two parts that bind one prefix to two namespaces, an
//!   embedded object or any other file a part's `xlink:href` names, `Basic/` or `Scripts/` (a flat document has no place for the code: macros are kept as bytes
//!   and these cannot be), a package part nothing here places, and a package with no `content.xml`. The thumbnail, `Configurations2/`, `manifest.rdf`, the manifest
//!   and `mimetype` have no place in a flat document and are left out, as R32 says; a signature file is left out and the save says the signature is gone.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::xml_scan::{AttrSpan, Tag, TagKind, scan_tags, tag_attributes};

use super::scan::{Ask, Fam, PartKind, scan_part_asking};
use super::splice::{Splicer, escape_value};
use super::{SaveNote, WriteError, WriteWork};
use crate::Variant;
use crate::limits::Limits;
use crate::package::{DRAWING_MEDIA_TYPE, Opened, TEMPLATE_MEDIA_TYPE, href_entry, unescape};

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const XLINK: &str = "http://www.w3.org/1999/xlink";

fn refuse<T>(words: &'static str) -> Result<T, WriteError> {
    Err(WriteError::Unsupported(words))
}

/// Base64, RFC 4648's alphabet and padding, in lines of 72 characters separated by line feeds. The memory for it is asked for first: an error and not an abort
/// when it is not had.
pub(crate) fn base64_lines(bytes: &[u8]) -> Result<String, WriteError> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // Four characters for three bytes, padded; a line feed between two lines of 72.
    let characters = bytes.len().div_ceil(3).saturating_mul(4);
    let wanted = characters.saturating_add(characters.saturating_sub(1) / 72);
    let mut out = String::new();
    out.try_reserve_exact(wanted).map_err(|_| WriteError::Limit { what: "memory for a picture written inline", max: wanted as u64 })?;
    let mut line = 0usize;
    for chunk in bytes.chunks(3) {
        let n = u32::from_be_bytes([0, chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)]);
        for k in 0..4usize {
            if line == 72 {
                out.push('\n');
                line = 0;
            }
            out.push(if k <= chunk.len() { char::from(ALPHABET.get(((n >> (18 - 6 * k)) & 63) as usize).copied().unwrap_or(b'A')) } else { '=' });
            line += 1;
        }
    }
    Ok(out)
}

/// The most pictures one conversion writes inline: each is a copy of a picture where an image names it, and the edits and the lists that hold them are kept.
const MOST_PICTURES: usize = 200_000;

/// The bytes of memory one picture inline costs besides its data: its start tag, the attributes made, the element of its data, and the edits that make them.
const PER_PICTURE_BYTES: u64 = 512;

/// A child of a part's root.
#[derive(Debug, Clone)]
struct Held {
    /// The vocabulary and local name, resolved by namespace.
    uri: String,
    local: String,
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
    empty: bool,
}

/// A `draw:image` or `draw:fill-image`: where its start tag is and the attributes of it.
struct Image {
    end: usize,
    empty: bool,
    qname: String,
    /// A `draw:fill-image`, which gets no `draw:mime-type` (name that attribute for `draw:image` only).
    fill: bool,
    /// The image holds an `office:binary-data` child of its own already.
    has_data: bool,
    attrs: Vec<AttrSpan>,
}

/// The words of a refusal of an image that holds data of its own beside an `xlink:href` that names a picture of the package: a package reader follows the href and a
/// flat reader takes the data, so the flat document would read as another picture where the two differ, and the element would hold two data children.
const WORDS_TWO_SOURCES: &str = "an image that holds inline data beside an href that names a picture of the package (a package reader follows the href and a flat reader takes the data, so the picture could change)";

/// The text at `span` of a part the scan has read as UTF-8, or a fault of the conversion's own reading (`Mismatch`): an attribute name that cannot be read is never
/// taken for an empty one. The scan reads a part whole as UTF-8 and the spans end at ASCII delimiters, so this is not met; the
/// name is not guessed at if it is.
fn text_at(bytes: &[u8], span: (usize, usize)) -> Result<&str, WriteError> {
    bytes
        .get(span.0..span.1)
        .and_then(|b| std::str::from_utf8(b).ok())
        .ok_or(WriteError::Mismatch("an attribute of a part the conversion cannot read as text"))
}

/// Whether one of the attributes `attrs` of a tag is a prefixed name whose local part is `local`.
fn has_local(bytes: &[u8], attrs: &[AttrSpan], local: &str) -> Result<bool, WriteError> {
    for a in attrs {
        if text_at(bytes, a.name)?.split_once(':').is_some_and(|(_, l)| l == local) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// What one pass over a part finds.
#[derive(Default)]
struct Look {
    root: (usize, usize),
    /// The root's declarations, in the order written.
    decls: Vec<(String, String)>,
    version: Option<String>,
    /// How many attributes the root has that are neither a namespace declaration nor the office namespace's `version`: a flat document's one root has no place for
    /// them.
    root_attrs: usize,
    children: Vec<Held>,
    images: Vec<Image>,
    /// Every `xlink:href` of the part, as its text: the pictures are among them.
    hrefs: Vec<String>,
}

/// The URI `prefix` is bound to in the stack of bindings.
fn uri_of<'a>(bound: &'a HashMap<String, Vec<String>>, prefix: &str) -> Option<&'a str> {
    bound.get(prefix).and_then(|stack| stack.last()).map(String::as_str)
}

/// Looks through the part `bytes` once: its root, its children, its pictures and its references.
fn look(bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<Look, WriteError> {
    work.charge_scan(bytes.len(), limits)?;
    let mut found = Look::default();
    let mut bound: HashMap<String, Vec<String>> = HashMap::new();
    let mut made: Vec<Vec<String>> = Vec::new();
    let mut spans: Vec<AttrSpan> = Vec::new();
    // The images (by place in `found.images`) whose end tag is not read yet, with their depth: the ones an `office:binary-data` may be a child of.
    let mut open_images: Vec<(usize, usize)> = Vec::new();
    let mut container = Work::default();
    let result = scan_tags::<WriteError>(bytes, &limits.container.xml, &mut container, |tag: &Tag<'_>| {
        if tag.kind == TagKind::Close {
            for prefix in made.pop().unwrap_or_default() {
                if let Some(stack) = bound.get_mut(&prefix) {
                    stack.pop();
                }
            }
            while open_images.last().is_some_and(|&(_, depth)| depth == tag.depth) {
                open_images.pop();
            }
            if tag.depth == 1
                && let Some(child) = found.children.last_mut()
                && child.end == child.open_end
                && !child.empty
            {
                (child.close_start, child.end) = (tag.start, tag.end);
            }
            return Ok(());
        }
        spans.clear();
        if tag_attributes(bytes, tag.start, tag.end, &mut spans).is_none() {
            return Err(WriteError::Mismatch("a start tag the conversion cannot read the attributes of"));
        }
        let mut declared = Vec::new();
        for a in &spans {
            let name = text_at(bytes, a.name)?;
            let prefix = if name == "xmlns" { Some("") } else { name.strip_prefix("xmlns:") };
            if let Some(prefix) = prefix {
                let uri = unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default());
                if tag.depth == 0 {
                    found.decls.push((prefix.to_owned(), uri.clone()));
                }
                bound.entry(prefix.to_owned()).or_default().push(uri);
                declared.push(prefix.to_owned());
            }
        }
        let (prefix, local) = tag.name.split_once(':').unwrap_or(("", tag.name));
        let uri = uri_of(&bound, prefix).unwrap_or_default().to_owned();
        let attr_uri = |name: &str, b: &HashMap<String, Vec<String>>| name.split_once(':').and_then(|(p, _)| uri_of(b, p).map(str::to_owned));
        if tag.depth == 0 {
            found.root = (tag.start, tag.end);
            for a in &spans {
                let name = text_at(bytes, a.name)?;
                if name.split_once(':').is_some_and(|(_, l)| l == "version") && attr_uri(name, &bound).as_deref() == Some(OFFICE) {
                    found.version = Some(unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default()));
                } else if !(name == "xmlns" || name.starts_with("xmlns:")) {
                    found.root_attrs = found.root_attrs.saturating_add(1);
                }
            }
        } else if tag.depth == 1 {
            let empty = tag.kind == TagKind::Empty;
            found.children.push(Held {
                uri: uri.clone(),
                local: local.to_owned(),
                start: tag.start,
                open_end: tag.end,
                close_start: tag.end,
                end: tag.end,
                empty,
            });
        }
        // An `office:binary-data` that is a child of the image whose start tag is the innermost one open: the image has data of its own.
        if uri == OFFICE
            && local == "binary-data"
            && let Some(&(at, depth)) = open_images.last()
            && depth.checked_add(1) == Some(tag.depth)
            && let Some(image) = found.images.get_mut(at)
        {
            image.has_data = true;
        }
        if uri == DRAW && matches!(local, "image" | "fill-image") {
            // An image that names no file is no picture to put inline: it is not kept.
            if has_local(bytes, &spans, "href")? {
                if found.images.len() >= MOST_PICTURES {
                    return Err(WriteError::Limit { what: "pictures a conversion to a flat document may write inline", max: MOST_PICTURES as u64 });
                }
                found
                    .images
                    .try_reserve(1)
                    .map_err(|_| WriteError::Limit { what: "memory for the pictures of a part", max: found.images.len() as u64 })?;
                let empty = tag.kind == TagKind::Empty;
                if !empty {
                    // One open image for each image kept at most, and no more than the depth the scan allows.
                    open_images
                        .try_reserve(1)
                        .map_err(|_| WriteError::Limit { what: "memory for the pictures of a part", max: found.images.len() as u64 })?;
                    open_images.push((found.images.len(), tag.depth));
                }
                found.images.push(Image {
                    end: tag.end,
                    empty,
                    qname: tag.name.to_owned(),
                    fill: local == "fill-image",
                    has_data: false,
                    attrs: spans.clone(),
                });
            }
        }
        for a in &spans {
            let name = text_at(bytes, a.name)?;
            if name.split_once(':').is_some_and(|(_, l)| l == "href") && attr_uri(name, &bound).as_deref() == Some(XLINK) {
                if found.hrefs.len() >= 4_000_000 {
                    return Err(WriteError::Limit { what: "href attributes in a part", max: 4_000_000 });
                }
                found.hrefs.push(unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default()));
            }
        }
        if tag.kind == TagKind::Empty {
            for prefix in declared {
                if let Some(stack) = bound.get_mut(&prefix) {
                    stack.pop();
                }
            }
        } else {
            made.push(declared);
        }
        Ok(())
    });
    work.package.scan_events = work.package.scan_events.saturating_add(container.xml_events);
    work.elements_walked = work.elements_walked.saturating_add(container.xml_events);
    match result {
        Ok(()) => Ok(found),
        Err(diacraft_container::xml_scan::ScanError::Visit(e)) => Err(e),
        Err(diacraft_container::xml_scan::ScanError::Xml(e)) => {
            Err(WriteError::Original(crate::OdgError::Xml { part: "a part of the package".to_owned(), error: e }))
        }
    }
}

/// The package files a flat document has no place for and leaves out: the thumbnail, a folder of the producer's settings, the manifest and
/// `mimetype`, `manifest.rdf`.
fn left_out(name: &str) -> bool {
    matches!(name, "mimetype" | "META-INF/manifest.xml" | "manifest.rdf" | "Thumbnails/thumbnail.png") || name.starts_with("Configurations2/")
}

/// The flat document of the package `opened`, and what the save says of it. See the module documentation.
pub(crate) fn flatten(opened: &mut Opened<'_>, limits: &Limits, work: &mut WriteWork) -> Result<(Vec<u8>, Vec<SaveNote>), WriteError> {
    let names: Vec<(String, bool)> = opened.archive.entries().iter().map(|e| (e.name.clone(), e.folder)).collect();
    let index: HashMap<String, usize> = names.iter().enumerate().map(|(i, (n, _))| (n.clone(), i)).collect();
    let part = |opened: &mut Opened<'_>, name: &str, work: &mut WriteWork| -> Result<Option<Vec<u8>>, WriteError> {
        match opened.archive.position(name) {
            Some(at) => Ok(Some(opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?)),
            None => Ok(None),
        }
    };
    let Some(content) = part(opened, "content.xml", work)? else { return refuse("a package with no content.xml") };
    let styles = part(opened, "styles.xml", work)?;
    let meta = part(opened, "meta.xml", work)?;
    let settings = part(opened, "settings.xml", work)?;
    for bytes in [Some(&content), styles.as_ref(), meta.as_ref(), settings.as_ref()].into_iter().flatten() {
        if diacraft_container::xml_scan::is_utf16(bytes) {
            return refuse("a package part written in UTF-16 saved as a flat document");
        }
    }
    let look_at =
        |bytes: Option<&Vec<u8>>, work: &mut WriteWork| -> Result<Option<Look>, WriteError> { bytes.map(|b| look(b, limits, work)).transpose() };
    let parts_bytes = [Some(&content), styles.as_ref(), meta.as_ref(), settings.as_ref()].into_iter().flatten().map(|b| b.len() as u64).sum::<u64>();
    let (mut inline_bytes, mut pictures_inline) = (0u64, 0usize);
    let content_look = look_at(Some(&content), work)?.ok_or(WriteError::Mismatch("the content part is gone"))?;
    let styles_look = look_at(styles.as_ref(), work)?;
    // What the package holds beyond the parts: pictures its images name, which go inline, and what has no place.
    let mut inlined: HashSet<usize> = HashSet::new();
    let mut notes = Vec::new();
    let mut part_looks: Vec<(&str, &Vec<u8>, &Look)> = vec![("content.xml", &content, &content_look)];
    if let (Some(b), Some(l)) = (styles.as_ref(), styles_look.as_ref()) {
        part_looks.push(("styles.xml", b, l));
    }
    // Each part with its pictures inlined: the edits are against the part's own bytes.
    let mut made_parts: Vec<(&str, Vec<u8>)> = Vec::new();
    for (name, bytes, found) in &part_looks {
        let mut splicer = Splicer::new(bytes);
        let office = found.decls.iter().filter(|(p, u)| !p.is_empty() && u == OFFICE).map(|(p, _)| p.clone()).min();
        let mut inline_here: HashSet<usize> = HashSet::new();
        for image in &found.images {
            let Some((href_at, entry)) = image
                .attrs
                .iter()
                .enumerate()
                .filter_map(|(k, a)| {
                    let name = std::str::from_utf8(bytes.get(a.name.0..a.name.1)?).ok()?;
                    (name.split_once(':').is_some_and(|(_, l)| l == "href") && !name.starts_with("xmlns")).then_some((k, a))
                })
                .find_map(|(k, a)| href_entry(&unescape(bytes.get(a.value.0..a.value.1)?), &index).map(|e| (k, e)))
            else {
                continue;
            };
            let (entry_name, folder) = names.get(entry).cloned().unwrap_or_default();
            if folder {
                return refuse(
                    "a package with an embedded object (a folder an href names): a flat document holds it as an inline document, which no rule here writes",
                );
            }
            if image.has_data {
                return refuse(WORDS_TWO_SOURCES);
            }
            let Some(office) = &office else {
                return refuse("a package whose root binds no prefix to the office namespace to write an inline picture under");
            };
            // The picture: inflated only now, and charged to the bytes a save may copy; its media type from the manifest, else the signature.
            let at = opened.archive.position(&entry_name).ok_or(WriteError::Mismatch("a picture the package lists is gone"))?;
            // Before the picture is inflated: what the conversion holds is estimated from the sizes the archive states, and refused past what a save may hold. Each
            // reference to a picture is a copy of it inline: the picture inflated, its characters, and the part they are spliced into, and the markup round it.
            let stated = opened.archive.entries().get(at).map_or(0, |e| e.size);
            inline_bytes = inline_bytes.saturating_add(stated.saturating_mul(4)).saturating_add(PER_PICTURE_BYTES);
            pictures_inline += 1;
            if inline_bytes.saturating_add(parts_bytes.saturating_mul(3)) > limits.max_write_memory || pictures_inline > MOST_PICTURES {
                return Err(WriteError::Limit {
                    what: "bytes of memory a conversion to a flat document holds (the package's pictures, each copied inline where an image names it)",
                    max: limits.max_write_memory,
                });
            }
            let data = opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?;
            let media = opened
                .manifest
                .entry(&entry_name)
                .and_then(|e| e.media_type.as_deref())
                .filter(|m| !m.is_empty())
                .map_or_else(|| signature_media(&data).to_owned(), str::to_owned);
            let has_mime = has_local(bytes, &image.attrs, "mime-type")?;
            work.charge_copy(data.len().saturating_mul(2), limits)?;
            let encoded = base64_lines(&data)?;
            let binary = format!("<{office}:binary-data>{encoded}</{office}:binary-data>");
            let attr = image.attrs.get(href_at).copied().ok_or(WriteError::Mismatch("an image's href is gone"))?;
            // The attribute's own text, and the white space before it, taken out.
            let mut from = attr.name.0;
            while let Some(before) = from.checked_sub(1)
                && bytes.get(before).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            {
                from = before;
            }
            splicer.remove(from, attr.value.1 + 1);
            let draw = found.decls.iter().filter(|(p, u)| !p.is_empty() && u == DRAW).map(|(p, _)| p.clone()).min();
            let draw_prefix = image.qname.split_once(':').map(|(p, _)| p.to_owned()).or(draw);
            // Only a `draw:image` is given its media type; a `draw:fill-image` is read by the bytes it holds.
            if !has_mime && !image.fill {
                let Some(draw_prefix) = draw_prefix else {
                    return refuse("an image whose element binds no prefix to the drawing namespace to write draw:mime-type under");
                };
                let at = if image.empty { image.end.saturating_sub(2) } else { image.end.saturating_sub(1) };
                splicer.insert(at, format!(" {draw_prefix}:mime-type=\"{}\"", escape_value(&media, b'"')));
            }
            if image.empty {
                splicer.replace(image.end.saturating_sub(2), image.end, format!(">{binary}</{}>", image.qname));
            } else {
                splicer.insert(image.end, binary);
            }
            inline_here.insert(entry);
        }
        inlined.extend(inline_here.iter().copied());
        // An href that names a file of the package and is no picture of an image: an object, a font, a list's bullet picture.
        let named: HashSet<String> = found.hrefs.iter().cloned().collect();
        for href in &named {
            if let Some(entry) = href_entry(href, &index)
                && !inlined.contains(&entry)
            {
                return refuse("a package part a flat document has no place for is named by an href (an embedded object, a font or another file)");
            }
        }
        let made = splicer.apply(work, limits)?;
        made_parts.push((name, made));
    }
    // What else the package holds.
    for (name, folder) in &names {
        let at = index.get(name).copied().unwrap_or(usize::MAX);
        if *folder {
            if !left_out(name) {
                return refuse("a package folder a flat document has no place for (an object, or code)");
            }
            continue;
        }
        let placed = matches!(name.as_str(), "content.xml" | "styles.xml" | "meta.xml" | "settings.xml") || left_out(name) || inlined.contains(&at);
        let signature = name.strip_prefix("META-INF/").is_some_and(|rest| rest.contains("signatures"));
        if signature {
            notes.push(SaveNote::SignatureStale);
        } else if !placed && !name.starts_with("META-INF/") {
            return refuse("a package part a flat document has no place for (code, an object, or a file nothing names)");
        }
    }
    notes.dedup();
    let part_of = |name: &str| made_parts.iter().find(|(n, _)| *n == name).map(|(_, b)| b.as_slice());
    let content_made = part_of("content.xml").unwrap_or(&content);
    let styles_made = part_of("styles.xml");
    compose(
        opened,
        ComposeInput { content: content_made, styles: styles_made, meta: meta.as_deref(), settings: settings.as_deref() },
        limits,
        work,
        notes,
    )
}

/// The media type a picture's own bytes say it is.
fn signature_media(bytes: &[u8]) -> &'static str {
    super::picture::picture_type(bytes).map_or("application/octet-stream", |t| t.media)
}

/// The parts, after their pictures went inline.
struct ComposeInput<'a> {
    content: &'a [u8],
    styles: Option<&'a [u8]>,
    meta: Option<&'a [u8]>,
    settings: Option<&'a [u8]>,
}

/// The office children of each part's root that a flat document places: `content.xml`'s `office:scripts`, `office:font-face-decls`, `office:automatic-styles` and
/// `office:body`, `styles.xml`'s `office:font-face-decls`, `office:styles`, `office:automatic-styles` and `office:master-styles`, `meta.xml`'s `office:meta` and
/// `settings.xml`'s `office:settings`.
const CONTENT_CHILDREN: [&str; 4] = ["scripts", "font-face-decls", "automatic-styles", "body"];
const STYLES_CHILDREN: [&str; 4] = ["font-face-decls", "styles", "automatic-styles", "master-styles"];
const META_CHILDREN: [&str; 1] = ["meta"];
const SETTINGS_CHILDREN: [&str; 1] = ["settings"];

const WORDS_ROOT_ATTRIBUTE: &str = "a package part whose root element has an attribute that is neither a namespace declaration nor office:version (a flat document's one root has no place for it)";
const WORDS_ROOT_CHILD: &str =
    "a package part whose root has a child a flat document has no place for (a foreign element, or an office element that belongs to another part)";
const WORDS_ROOT_REPEAT: &str =
    "a package part whose root has two children of one name (a flat document holds one of each, and the second would be dropped)";

/// Whether the root of the part `found` holds only what a flat document places: no attribute but namespace declarations and `office:version`, and each child an
/// office element of `placed` (by local name) once; the words of the refusal if not. The work is at most `placed.len()` children and one more.
fn root_is_placed(found: &Look, placed: &[&str]) -> Result<(), WriteError> {
    if found.root_attrs > 0 {
        return refuse(WORDS_ROOT_ATTRIBUTE);
    }
    let mut seen: Vec<&str> = Vec::new();
    for child in &found.children {
        if child.uri != OFFICE || !placed.contains(&child.local.as_str()) {
            return refuse(WORDS_ROOT_CHILD);
        }
        if seen.contains(&child.local.as_str()) {
            return refuse(WORDS_ROOT_REPEAT);
        }
        // At most `placed.len()` entries: a repeat is refused above.
        seen.push(child.local.as_str());
    }
    Ok(())
}

/// The children of a part by what they are, after the part was edited its places are looked for again.
fn children_of(bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<Look, WriteError> {
    look(bytes, limits, work)
}

/// One child's text: the element of the part as it is.
fn text_of<'a>(bytes: &'a [u8], child: &Held) -> &'a str {
    std::str::from_utf8(bytes.get(child.start..child.end).unwrap_or_default()).unwrap_or_default()
}

/// What is inside a child's element (nothing for an empty one).
fn inner_of<'a>(bytes: &'a [u8], child: &Held) -> &'a str {
    if child.empty { "" } else { std::str::from_utf8(bytes.get(child.open_end..child.close_start).unwrap_or_default()).unwrap_or_default() }
}

fn compose(
    opened: &Opened<'_>,
    input: ComposeInput<'_>,
    limits: &Limits,
    work: &mut WriteWork,
    notes: Vec<SaveNote>,
) -> Result<(Vec<u8>, Vec<SaveNote>), WriteError> {
    // The parts as they are now, looked through again: the pictures went inline, so the places moved.
    let content = children_of(input.content, limits, work)?;
    let styles = input.styles.map(|b| children_of(b, limits, work)).transpose()?;
    let meta = input.meta.map(|b| children_of(b, limits, work)).transpose()?;
    let settings = input.settings.map(|b| children_of(b, limits, work)).transpose()?;
    // What each part's root holds must have a place in the flat document: a child is taken once, by name, from its own part, and a root attribute has no place at
    // all, so each is refused in words and not dropped.
    for (found, placed) in [
        (Some(&content), &CONTENT_CHILDREN[..]),
        (styles.as_ref(), &STYLES_CHILDREN[..]),
        (meta.as_ref(), &META_CHILDREN[..]),
        (settings.as_ref(), &SETTINGS_CHILDREN[..]),
    ] {
        if let Some(found) = found {
            root_is_placed(found, placed)?;
        }
    }
    // The two parts' automatic styles must not share a name: the one container would hold two styles of one name.
    names_are_disjoint(input.content, input.styles, limits, work)?;
    // The root: the declarations of every part's root, in the order they first appear, and the kind and the version the package states.
    let mut decls: Vec<(String, String)> = Vec::new();
    for found in [styles.as_ref(), Some(&content), meta.as_ref(), settings.as_ref()].into_iter().flatten() {
        for (prefix, uri) in &found.decls {
            match decls.iter().find(|(p, _)| p == prefix) {
                Some((_, held)) if held != uri => {
                    return refuse("two parts of the package bind one prefix to two namespaces (a flat document's root binds each prefix once)");
                }
                Some(_) => {}
                None => decls.push((prefix.clone(), uri.clone())),
            }
        }
    }
    let office = decls.iter().filter(|(p, u)| !p.is_empty() && u == OFFICE).map(|(p, _)| p.clone()).min();
    let Some(office) = office else { return refuse("a package whose parts bind no prefix to the office namespace") };
    let version = [Some(&content), styles.as_ref(), meta.as_ref(), settings.as_ref()].into_iter().flatten().find_map(|l| l.version.clone());
    let variant = match opened.media_type.as_deref() {
        Some(TEMPLATE_MEDIA_TYPE) => Variant::Template,
        _ => Variant::Drawing,
    };
    let media = match variant {
        Variant::Drawing => DRAWING_MEDIA_TYPE,
        Variant::Template => TEMPLATE_MEDIA_TYPE,
    };
    // The root's start tag, then the children in the order, each on a line of its own: every piece is a span of a part (borrowed), or the join of two; the
    // document is made in one buffer whose memory is asked for first.
    let mut head = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    head.push_str(&format!("<{office}:document"));
    for (prefix, uri) in &decls {
        let name = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
        head.push_str(&format!(" {name}=\"{}\"", escape_value(uri, b'"')));
    }
    if let Some(version) = &version {
        head.push_str(&format!(" {office}:version=\"{}\"", escape_value(version, b'"')));
    }
    head.push_str(&format!(" {office}:mimetype=\"{media}\">"));
    let mut pieces: Vec<Cow<'_, str>> = Vec::new();
    let two = |local: &str| both(styles.as_ref(), input.styles, &content, input.content, local);
    for (look, bytes, local) in
        [(meta.as_ref(), input.meta, "meta"), (settings.as_ref(), input.settings, "settings"), (Some(&content), Some(input.content), "scripts")]
    {
        if let Some((bytes, child)) = child(look, bytes, local) {
            pieces.push(Cow::Borrowed(text_of(bytes, &child)));
        }
    }
    // The faces and the automatic styles: the two parts' contents joined, `styles.xml`'s first, under the first start tag.
    if let Some(joined) = join(&two("font-face-decls"), &office, "font-face-decls")? {
        pieces.push(joined);
    }
    if let Some((bytes, child)) = child(styles.as_ref(), input.styles, "styles") {
        pieces.push(Cow::Borrowed(text_of(bytes, &child)));
    }
    if let Some(joined) = join(&two("automatic-styles"), &office, "automatic-styles")? {
        pieces.push(joined);
    }
    if let Some((bytes, child)) = child(styles.as_ref(), input.styles, "master-styles") {
        pieces.push(Cow::Borrowed(text_of(bytes, &child)));
    }
    let Some((bytes, child)) = child(Some(&content), Some(input.content), "body") else {
        return refuse("a package whose content part has no office:body");
    };
    pieces.push(Cow::Borrowed(text_of(bytes, &child)));
    let tail = format!("\n</{office}:document>\n");
    let total = pieces.iter().map(|p| p.len() + 2).fold(head.len() + tail.len(), usize::saturating_add);
    work.charge_copy(total, limits)?;
    let mut out = String::new();
    out.try_reserve_exact(total).map_err(|_| WriteError::Limit { what: "memory for the flat document", max: total as u64 })?;
    out.push_str(&head);
    for piece in &pieces {
        out.push_str("\n ");
        out.push_str(piece);
    }
    out.push_str(&tail);
    // The estimate is the length made, to the byte (no growth, no waste).
    debug_assert_eq!(out.len(), total, "the memory asked for is the document's length");
    Ok((out.into_bytes(), notes))
}

/// The child of `local` of the part `bytes`, with the bytes it is in.
fn child<'a>(look: Option<&Look>, bytes: Option<&'a [u8]>, local: &str) -> Option<(&'a [u8], Held)> {
    let (look, bytes) = (look?, bytes?);
    look.children.iter().find(|c| c.uri == OFFICE && c.local == local).cloned().map(|c| (bytes, c))
}

/// The children of `local` that `styles.xml` and `content.xml` hold, in that order.
fn both<'a>(styles: Option<&Look>, styles_bytes: Option<&'a [u8]>, content: &Look, content_bytes: &'a [u8], local: &str) -> Vec<(&'a [u8], Held)> {
    [child(styles, styles_bytes, local), child(Some(content), Some(content_bytes), local)].into_iter().flatten().collect()
}

/// The children of one name that the parts hold, joined into one element: the first one's start tag, the contents of each in turn, and the first one's end tag.
/// `None` when no part holds it. A later one whose start tag has attributes of its own (a declaration of a prefix its contents use, for one) is refused: they
/// would be lost.
fn join<'a>(parts: &[(&'a [u8], Held)], office: &str, local: &str) -> Result<Option<Cow<'a, str>>, WriteError> {
    let Some((first_bytes, first)) = parts.first() else { return Ok(None) };
    if parts.len() == 1 {
        return Ok(Some(Cow::Borrowed(text_of(first_bytes, first))));
    }
    let tag_of = |bytes: &[u8], c: &Held| std::str::from_utf8(bytes.get(c.start..c.open_end).unwrap_or_default()).unwrap_or_default().to_owned();
    for (bytes, child) in parts.iter().skip(1) {
        let tag = tag_of(bytes, child);
        if tag.trim_end_matches('>').trim_end_matches('/').trim_end().contains(|c: char| c.is_whitespace()) {
            return refuse("a part whose automatic styles or font declarations element has attributes of its own, joined with another's");
        }
    }
    let open = tag_of(first_bytes, first);
    let open = if first.empty { format!("{}>", open.trim_end_matches('>').trim_end_matches('/').trim_end()) } else { open };
    let name = open
        .trim_start_matches('<')
        .split([' ', '\t', '\r', '\n', '>', '/'])
        .next()
        .filter(|n| !n.is_empty())
        .map_or_else(|| format!("{office}:{local}"), str::to_owned);
    let inner: usize = parts.iter().map(|(b, c)| inner_of(b, c).len()).sum();
    let wanted = open.len().saturating_add(inner).saturating_add(name.len()).saturating_add(3);
    let mut out = String::new();
    out.try_reserve_exact(wanted).map_err(|_| WriteError::Limit { what: "memory for the joined styles", max: wanted as u64 })?;
    out.push_str(&open);
    for (bytes, child) in parts {
        out.push_str(inner_of(bytes, child));
    }
    out.push_str(&format!("</{name}>"));
    Ok(Some(Cow::Owned(out)))
}

/// The two parts' automatic styles share no name (family and name), nor their page layouts: the flat document's one container would hold two of one name
/// (Q8 asks for a rule that renames; until it exists the save is refused).
fn names_are_disjoint(content: &[u8], styles: Option<&[u8]>, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let Some(styles) = styles else { return Ok(()) };
    let ask = Ask { styles: true, ..Ask::default() };
    let a = scan_part_asking(content, PartKind::Content, &ask, limits, work)?;
    let b = scan_part_asking(styles, PartKind::Styles, &ask, limits, work)?;
    let mut seen: HashSet<(Fam, String)> = HashSet::new();
    seen.try_reserve(b.defs.len().saturating_add(b.layouts.len()))
        .map_err(|_| WriteError::Limit { what: "memory for the names of the automatic styles", max: 0 })?;
    seen.extend(b.defs.iter().map(|d| (d.family, d.name.clone())));
    seen.extend(b.layouts.iter().map(|l| (Fam::Layout, l.name.clone())));
    let steps = [a.defs.len(), a.layouts.len(), b.defs.len(), b.layouts.len()].iter().fold(0u64, |sum, n| sum.saturating_add(*n as u64));
    work.order_steps = work.order_steps.saturating_add(steps);
    let clash =
        a.defs.iter().any(|d| seen.contains(&(d.family, d.name.clone()))) || a.layouts.iter().any(|l| seen.contains(&(Fam::Layout, l.name.clone())));
    if clash {
        return refuse(
            "the package's two parts hold automatic styles of one name (a flat document has one container for them: no rule says how to rename)",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_rfc_4648_padded_in_lines_of_72() {
        for (plain, coded) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64_lines(plain.as_bytes()).unwrap(), coded, "{plain}");
        }
        assert_eq!(base64_lines(&[0xFB, 0xFF, 0xBE]).unwrap(), "+/++", "the last two characters of the alphabet");
        // 54 bytes make 72 characters: exactly one line, no line feed; one byte more starts the next.
        assert_eq!(base64_lines(&[7u8; 54]).unwrap().len(), 72);
        assert!(!base64_lines(&[7u8; 54]).unwrap().contains('\n'));
        let two = base64_lines(&[7u8; 55]).unwrap();
        assert_eq!(two.split('\n').map(str::len).collect::<Vec<_>>(), [72, 4]);
        // The memory asked for is the length made, to the byte, whatever the length (no growth, no waste).
        for n in [0usize, 1, 2, 3, 53, 54, 55, 56, 107, 108, 109, 1000, 5000] {
            let made = base64_lines(&vec![9u8; n]).unwrap();
            assert_eq!(made.capacity(), made.len(), "{n}");
            assert_eq!(made.len(), n.div_ceil(3) * 4 + (n.div_ceil(3) * 4).saturating_sub(1) / 72, "{n}");
        }
        let many = base64_lines(&[1u8; 1000]).unwrap();
        let lines: Vec<&str> = many.split('\n').collect();
        assert!(lines.iter().rev().skip(1).all(|l| l.len() == 72) && lines.last().is_some_and(|l| l.len() <= 72 && !l.is_empty()));
    }

    #[test]
    fn a_picture_is_told_by_its_own_bytes() {
        assert_eq!(signature_media(&[0x89, b'P', b'N', b'G', 1]), "image/png");
        assert_eq!(signature_media(&[0xFF, 0xD8, 0]), "image/jpeg");
        assert_eq!(signature_media(b"GIF89a"), "image/gif");
        assert_eq!(signature_media(b"BM\0\0"), "image/bmp");
        let mut emf = vec![0u8; 44];
        emf.get_mut(40..44).unwrap().copy_from_slice(b" EMF");
        assert_eq!(signature_media(&emf), "image/emf");
        assert_eq!(signature_media(b"plain"), "application/octet-stream");
        assert_eq!(signature_media(&[]), "application/octet-stream");
    }

    #[test]
    fn what_a_flat_document_has_no_place_for_is_left_out_by_name() {
        for name in [
            "mimetype",
            "META-INF/manifest.xml",
            "manifest.rdf",
            "Thumbnails/thumbnail.png",
            "Configurations2/",
            "Configurations2/accelerator/current.xml",
        ] {
            assert!(left_out(name), "{name}");
        }
        for name in ["content.xml", "Pictures/a.png", "Thumbnails/other.png", "META-INF/documentsignatures.xml", "Basic/x.xml"] {
            assert!(!left_out(name), "{name}");
        }
    }

    #[test]
    fn a_part_is_looked_through_once_for_its_root_children_images_and_references() {
        let part = format!(
            "<office:document-content xmlns:office=\"{OFFICE}\" xmlns:draw=\"{DRAW}\" xmlns:xlink=\"{XLINK}\" office:version=\"1.3\"><office:scripts/><office:body><draw:image xlink:href=\"a.png\"/><x:other xmlns:x=\"urn:x\" xlink:href=\"b\"/></office:body></office:document-content>"
        );
        let found = look(part.as_bytes(), &Limits::default(), &mut WriteWork::default()).unwrap();
        assert_eq!(found.version.as_deref(), Some("1.3"));
        assert_eq!(found.decls.len(), 3);
        let kids: Vec<(&str, &str, bool)> = found.children.iter().map(|c| (c.uri.as_str(), c.local.as_str(), c.empty)).collect();
        assert_eq!(kids, [(OFFICE, "scripts", true), (OFFICE, "body", false)]);
        assert_eq!(found.images.len(), 1);
        assert_eq!(found.hrefs, ["a.png", "b"]);
        let body = &found.children[1];
        assert_eq!(&part[body.start..body.open_end], "<office:body>");
        assert_eq!(&part[body.close_start..body.end], "</office:body>");
        assert_eq!(&part[body.open_end..body.close_start], inner_of(part.as_bytes(), body));
        assert_eq!(text_of(part.as_bytes(), body), &part[body.start..body.end]);
        assert_eq!(inner_of(part.as_bytes(), &found.children[0]), "", "an empty element holds nothing");
        // A part that is no XML is a fault of the package.
        assert!(look(b"<a><b></a>", &Limits::default(), &mut WriteWork::default()).is_err());
    }

    /// Bytes that are not text, or a span outside the part, are a fault of the conversion's reading and never an empty name taken for none.
    #[test]
    fn an_attribute_name_that_cannot_be_read_is_a_fault_and_not_an_empty_name() {
        let bytes = b"<a xlink:href=\"x\" \xFF=\"y\"/>";
        assert_eq!(text_at(bytes, (3, 13)).unwrap(), "xlink:href");
        assert_eq!(text_at(bytes, (3, 3)).unwrap(), "", "an empty span is an empty name");
        assert!(matches!(text_at(bytes, (18, 19)), Err(WriteError::Mismatch(_))), "a byte that is not UTF-8");
        assert!(matches!(text_at(bytes, (3, 500)), Err(WriteError::Mismatch(_))), "a span past the part");
        assert!(matches!(text_at(bytes, (9, 3)), Err(WriteError::Mismatch(_))), "a span that runs backwards");
        let attrs = |text: &[u8]| {
            let mut out = Vec::new();
            tag_attributes(text, 0, text.len(), &mut out).expect("a tag");
            out
        };
        let good = b"<draw:image xlink:href=\"x\" draw:mime-type=\"y\"/>";
        assert_eq!(has_local(good, &attrs(good), "href").ok(), Some(true));
        assert_eq!(has_local(good, &attrs(good), "mime-type").ok(), Some(true));
        assert_eq!(has_local(good, &attrs(good), "name").ok(), Some(false));
        assert_eq!(has_local(good, &attrs(good), "").ok(), Some(false), "a local part is not empty");
        let bare = b"<draw:image href=\"x\"/>";
        assert_eq!(has_local(bare, &attrs(bare), "href").ok(), Some(false), "a name with no prefix has no local part");
        let broken = b"<draw:image \xFF:href=\"x\"/>";
        assert!(matches!(has_local(broken, &attrs(broken), "href"), Err(WriteError::Mismatch(_))), "an attribute that cannot be read is no answer");
    }

    /// A root's children are placed when each is an office element of the part's own kind, once; its attributes when they are declarations and
    /// the office version.
    #[test]
    fn a_root_is_placed_when_its_children_are_the_parts_own_once_and_it_has_no_other_attribute() {
        let root = |open: &str| {
            let part = format!("<office:document-styles xmlns:office=\"{OFFICE}\" xmlns:x=\"urn:x\" {open}>");
            look(format!("{part}</office:document-styles>").as_bytes(), &Limits::default(), &mut WriteWork::default()).unwrap()
        };
        assert_eq!((root("").root_attrs, root("office:version=\"1.3\"").root_attrs), (0, 0), "declarations and the version are placed");
        assert_eq!(root("office:version=\"1.3\" x:a=\"1\" office:b=\"2\" xml:space=\"preserve\"").root_attrs, 3);
        // The version is the office namespace's, whichever order and whatever else of that local name the root has: the other is an attribute with no place.
        for open in ["x:version=\"9\" office:version=\"1.3\"", "office:version=\"1.3\" x:version=\"9\""] {
            let found = root(open);
            assert_eq!((found.version.as_deref(), found.root_attrs), (Some("1.3"), 1), "{open}");
        }
        let body = |kids: &str| {
            let part = format!("<office:document-styles xmlns:office=\"{OFFICE}\" xmlns:x=\"urn:x\">{kids}</office:document-styles>");
            look(part.as_bytes(), &Limits::default(), &mut WriteWork::default()).unwrap()
        };
        let words = |kids: &str, placed: &[&str]| match root_is_placed(&body(kids), placed) {
            Ok(()) => "placed".to_owned(),
            Err(WriteError::Unsupported(words)) => words.to_owned(),
            Err(other) => panic!("{other}"),
        };
        let own = ["styles", "automatic-styles"];
        assert_eq!(words("", &own), "placed");
        assert_eq!(words("<office:styles/><office:automatic-styles></office:automatic-styles>", &own), "placed");
        assert_eq!(words("<office:styles/><office:styles/>", &own), WORDS_ROOT_REPEAT);
        assert_eq!(words("<office:styles/><office:automatic-styles/><office:styles/>", &own), WORDS_ROOT_REPEAT, "not next to each other");
        assert_eq!(words("<office:master-styles/>", &own), WORDS_ROOT_CHILD, "an office element of another part's kind");
        assert_eq!(words("<x:styles/>", &own), WORDS_ROOT_CHILD, "a foreign element of the same local name");
        assert_eq!(words("<styles/>", &own), WORDS_ROOT_CHILD, "no namespace is no office element");
        // Each class has its own words, the attribute first.
        let attr = look(
            format!("<office:document-styles xmlns:office=\"{OFFICE}\" x:a=\"1\" xmlns:x=\"urn:x\"><x:y/><office:styles/><office:styles/></office:document-styles>")
                .as_bytes(),
            &Limits::default(),
            &mut WriteWork::default(),
        )
        .unwrap();
        assert!(matches!(root_is_placed(&attr, &own), Err(WriteError::Unsupported(w)) if w == WORDS_ROOT_ATTRIBUTE));
        // The four lists are the children the composition takes, one each from its own part.
        assert_eq!(CONTENT_CHILDREN, ["scripts", "font-face-decls", "automatic-styles", "body"]);
        assert_eq!(STYLES_CHILDREN, ["font-face-decls", "styles", "automatic-styles", "master-styles"]);
        assert_eq!((META_CHILDREN, SETTINGS_CHILDREN), (["meta"], ["settings"]));
        // A hostile part of many children stops at the first it cannot place (the work is not the part's).
        let many = "<office:styles/><x:late/>";
        assert_eq!(words(many, &own), WORDS_ROOT_CHILD);
        let repeated = "<office:styles/>".repeat(50_000);
        assert_eq!(words(&repeated, &own), WORDS_ROOT_REPEAT);
    }

    /// An image has data of its own when an `office:binary-data` is its child, and not when it is a child of its child or of a sibling.
    #[test]
    fn an_image_knows_whether_it_holds_data_of_its_own() {
        let part = |inside: &str| {
            format!(
                "<office:document-content xmlns:office=\"{OFFICE}\" xmlns:draw=\"{DRAW}\" xmlns:xlink=\"{XLINK}\" xmlns:text=\"urn:t\"><office:body>{inside}</office:body></office:document-content>"
            )
        };
        let data = |inside: &str| {
            let found = look(part(inside).as_bytes(), &Limits::default(), &mut WriteWork::default()).unwrap();
            found.images.iter().map(|i| (i.has_data, i.fill)).collect::<Vec<_>>()
        };
        assert_eq!(data(r#"<draw:image xlink:href="a"><office:binary-data>AA</office:binary-data></draw:image>"#), [(true, false)]);
        assert_eq!(data(r#"<draw:image xlink:href="a"><office:binary-data/></draw:image>"#), [(true, false)]);
        assert_eq!(data(r#"<draw:fill-image xlink:href="a"><office:binary-data>AA</office:binary-data></draw:fill-image>"#), [(true, true)]);
        assert_eq!(data(r#"<draw:image xlink:href="a"/>"#), [(false, false)]);
        assert_eq!(data(r#"<draw:image xlink:href="a"></draw:image>"#), [(false, false)]);
        // Further down is not a child; a sibling's data is not the image's; the next image is its own.
        assert_eq!(data(r#"<draw:image xlink:href="a"><text:p><office:binary-data>AA</office:binary-data></text:p></draw:image>"#), [(false, false)]);
        assert_eq!(data(r#"<draw:image xlink:href="a"/><office:binary-data>AA</office:binary-data>"#), [(false, false)]);
        assert_eq!(
            data(
                r#"<draw:image xlink:href="a"></draw:image><draw:image xlink:href="b"><office:binary-data>AA</office:binary-data></draw:image><draw:image xlink:href="c"/>"#
            ),
            [(false, false), (true, false), (false, false)]
        );
        // An image nested in an image: the data belongs to the one it is a child of.
        assert_eq!(
            data(r#"<draw:image xlink:href="a"><draw:image xlink:href="b"><office:binary-data>AA</office:binary-data></draw:image></draw:image>"#),
            [(false, false), (true, false)]
        );
        assert_eq!(
            data(r#"<draw:image xlink:href="a"><draw:image xlink:href="b"/><office:binary-data>AA</office:binary-data></draw:image>"#),
            [(true, false), (false, false)],
            "data after a nested image is the outer image's"
        );
        // An element of the image that is closed before its data (an end tag at the depth of the data, not at the image's own) does not close the image.
        assert_eq!(
            data(r#"<draw:image xlink:href="a"><text:p></text:p><office:binary-data>AA</office:binary-data></draw:image>"#),
            [(true, false)],
            "a closed child before the data"
        );
        assert_eq!(
            data(r#"<draw:image xlink:href="a"><draw:image xlink:href="b"></draw:image><office:binary-data>AA</office:binary-data></draw:image>"#),
            [(true, false), (false, false)],
            "a nested image closed before the data closes itself and not the outer one"
        );
        // Another namespace's `binary-data` is none, and an image with no href is not an image the conversion keeps.
        assert_eq!(data(r#"<draw:image xlink:href="a"><text:binary-data>AA</text:binary-data></draw:image>"#), [(false, false)]);
        assert_eq!(data(r#"<draw:image><office:binary-data>AA</office:binary-data></draw:image>"#), Vec::<(bool, bool)>::new());
    }

    #[test]
    fn two_children_of_one_name_are_joined_under_the_first_start_tag() {
        let a = "<office:automatic-styles><s1/></office:automatic-styles>";
        let b = "<office:automatic-styles/>";
        let c = "<office:automatic-styles><s3/></office:automatic-styles>";
        let held = |text: &str| Held {
            uri: OFFICE.into(),
            local: "automatic-styles".into(),
            start: 0,
            open_end: text.find('>').map_or(0, |i| i + 1),
            close_start: text.rfind("</").unwrap_or(text.len()),
            end: text.len(),
            empty: text.ends_with("/>"),
        };
        let parts: Vec<(&[u8], Held)> = vec![(a.as_bytes(), held(a)), (b.as_bytes(), held(b)), (c.as_bytes(), held(c))];
        let joined = |parts: &[(&[u8], Held)]| join(parts, "office", "automatic-styles").unwrap().map(Cow::into_owned);
        assert_eq!(joined(&parts).as_deref(), Some("<office:automatic-styles><s1/><s3/></office:automatic-styles>"));
        assert!(matches!(join(&parts[..1], "office", "automatic-styles"), Ok(Some(Cow::Borrowed(t))) if t == a), "one is itself, and not copied");
        assert_eq!(joined(&[]), None);
        // The first one written empty is opened for the others' contents.
        let parts: Vec<(&[u8], Held)> = vec![(b.as_bytes(), held(b)), (c.as_bytes(), held(c))];
        assert_eq!(joined(&parts).as_deref(), Some("<office:automatic-styles><s3/></office:automatic-styles>"));
        // The end tag is the first one's name, in whatever prefix it is written.
        let other = "<o:automatic-styles><s5/></o:automatic-styles>";
        let parts: Vec<(&[u8], Held)> = vec![(other.as_bytes(), held(other)), (a.as_bytes(), held(a))];
        assert_eq!(joined(&parts).as_deref(), Some("<o:automatic-styles><s5/><s1/></o:automatic-styles>"));
        // A later one with attributes of its own is refused, and the first may have some.
        let with = "<office:automatic-styles xmlns:foo=\"urn:foo\"><s4/></office:automatic-styles>";
        let parts: Vec<(&[u8], Held)> = vec![(a.as_bytes(), held(a)), (with.as_bytes(), held(with))];
        assert!(matches!(join(&parts, "office", "automatic-styles"), Err(WriteError::Unsupported(_))));
        let parts: Vec<(&[u8], Held)> = vec![(with.as_bytes(), held(with)), (a.as_bytes(), held(a))];
        assert_eq!(joined(&parts).as_deref(), Some("<office:automatic-styles xmlns:foo=\"urn:foo\"><s4/><s1/></office:automatic-styles>"));
    }
}
