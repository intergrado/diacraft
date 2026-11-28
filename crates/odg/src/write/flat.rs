//! Saving a flat drawing, `.fodg`: a save that changes only what an edit touched, spliced into
//! the one document.
//!
//! A flat drawing is one XML document with no package, so a save has **no entries to copy, no manifest and no thumbnail**: the document's bytes are the whole file.
//!
//! * **An unedited save** writes the bytes it was given, whatever they are (a UTF-16 document, a document with a byte-order mark, one out of order): R34.
//! * **An edited save** splices the changes in one document **as the package writer splices `content.xml` and then `styles.xml`** ([`super::splice_part`]): the
//!   document is scanned as the one part and then as the other (the scan finds each by the root's children and by the kinds of page, and no more of it), each pass
//!   makes its edits in one sweep, and the second pass scans what the first made, so the names a pass gives to new styles are numbered over the one container of
//!   automatic styles that R32 reads (the second pass finds the first's in it; no list of the first's names is needed, and one is kept all the same). What the
//!   package writer adds to a part goes into the matching child: new automatic styles and page layouts into `office:automatic-styles` (made before
//!   `office:master-styles`, or before `office:body`, when the document has none), new common styles and markers into `office:styles`, split master pages
//!   into `office:master-styles`, font faces into `office:font-face-decls` (made before the first child R30 puts after it).
//! * **What is not a shape's or a style's** is written by a last pass over the document ([`housekeeping`]), in one scan and one sweep: DiaCraft's generator in
//!   `office:meta` (made as the first child, holding only `meta:generator`, when the document has none, with the meta namespace declared on it when the root does
//!   not declare it), the media type on the root when a kind is asked for (the rule for a template), and DiaCraft's extension, the one child in
//!   DiaCraft's namespace, which is written as the last child of the root, after `office:body`, or where the document already has it.
//! * `office:version` is never written, so a document's own is kept; a document in UTF-16 is read, and an edit of it is refused in words, since the
//!   container's scan cannot splice UTF-16.
//!
//! **What it costs** is the package writer's, counted in the same units ([`super::WriteWork`]): a pass scans the document once and copies it once, and the last pass
//! once more of each, so an edit of one shape of a document of `n` bytes scans it about three times (the pass, the last pass, and the check of the bytes made) and
//! copies it twice. A document of 256 MiB can pass the 1 GiB a save may scan, and a document past that is refused as `write_limit`, with nothing written.

use std::borrow::Cow;

use diacraft_doc::{Document, RawElement, RawNode};

use super::compare::{self, PageDiff};
use super::meta::{self, FlatPlan};
use super::parts::Source;
use super::scan::PartKind;
use super::splice::{escape_value, has_bad_char};
use super::{
    Beside, PartCtx, PartOut, Saving, WriteError, WriteOptions, WriteWork, Written, baseline_error, check_index, check_memory, growth, new_index,
    pages, save_limits, splice_part,
};
use crate::flat::{self as shape, Claim, Head};
use crate::index::{Fingerprint, ITEM_BYTES, OriginalPackage, PageSource};
use crate::limits::Limits;
use crate::{Form, Variant, read};

/// The most bytes of DiaCraft's extension a save writes: it is cells in attribute values, and the reader's cap on one attribute is 1 MiB.
const MOST_EXTENSION_BYTES: usize = 16 * 1024 * 1024;

/// The deepest an element of the extension is written: the container's depth cap, which the reader refuses past.
const MOST_EXTENSION_DEPTH: usize = 200;

/// The fingerprint of a flat document for the index a save makes: the whole document is the one part, so it takes the content's place.
fn flat_fingerprint(bytes: &[u8]) -> Fingerprint {
    Fingerprint { content: shape::fingerprint(bytes), styles: (0, 0) }
}

/// Whether `name` is an XML name, as far as a name written into an element or an attribute of the extension need be: no character that ends a name or starts
/// anything else.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    let start = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == ':');
    start && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.' | '\u{B7}'))
}

/// A text as the content of an element: `&`, `<` and `>` escaped, a carriage return as a reference (a literal one reads back as a line feed).
fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
}

/// The element `root` as XML text, written as it is held: its name, each attribute in order (the `xmlns` declarations among them), and each child in order
/// (elements, text, comments and instructions). Nothing is added: what a read of the text gives is `root` again. An element past
/// [`MOST_EXTENSION_DEPTH`] levels deep, a name that is no name, a character XML does not allow, or text past [`MOST_EXTENSION_BYTES`] is refused in words.
fn element_text(root: &RawElement, work: &mut WriteWork, limits: &Limits) -> Result<String, WriteError> {
    enum Step<'a> {
        Open(&'a RawElement),
        Close(&'a str),
        Node(&'a RawNode),
    }
    let refuse = |why: &'static str| WriteError::Unsupported(why);
    let mut out = String::new();
    let mut stack = vec![Step::Open(root)];
    let mut depth = 0usize;
    while let Some(step) = stack.pop() {
        match step {
            Step::Node(RawNode::Element(el)) | Step::Open(el) => {
                depth += 1;
                if depth > MOST_EXTENSION_DEPTH {
                    return Err(refuse("DiaCraft's extension nested deeper than a document can hold"));
                }
                if !is_name(&el.name) {
                    return Err(refuse("DiaCraft's extension holds an element whose name is no XML name"));
                }
                out.push('<');
                out.push_str(&el.name);
                for (name, value) in el.attrs.iter() {
                    if !is_name(name) {
                        return Err(refuse("DiaCraft's extension holds an attribute whose name is no XML name"));
                    }
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    out.push_str(&escape_value(value, b'"'));
                    out.push('"');
                }
                if el.children.is_empty() {
                    out.push_str("/>");
                    depth -= 1;
                } else {
                    out.push('>');
                    stack
                        .try_reserve(el.children.len().saturating_add(1))
                        .map_err(|_| WriteError::Limit { what: "memory for DiaCraft's extension", max: 0 })?;
                    stack.push(Step::Close(&el.name));
                    stack.extend(el.children.iter().rev().map(Step::Node));
                }
            }
            Step::Close(name) => {
                depth = depth.saturating_sub(1);
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
            Step::Node(RawNode::Text(text)) => escape_text(text, &mut out),
            Step::Node(RawNode::Comment(text)) => {
                if text.contains("--") || text.ends_with('-') {
                    return Err(refuse("DiaCraft's extension holds a comment that XML does not allow"));
                }
                out.push_str("<!--");
                out.push_str(text);
                out.push_str("-->");
            }
            Step::Node(RawNode::Pi { target, data }) => {
                if !is_name(target) || target.eq_ignore_ascii_case("xml") || data.contains("?>") {
                    return Err(refuse("DiaCraft's extension holds a processing instruction that XML does not allow"));
                }
                out.push_str("<?");
                out.push_str(target);
                if !data.is_empty() {
                    out.push(' ');
                    out.push_str(data);
                }
                out.push_str("?>");
            }
        }
        // Held to its bytes as it grows, and at the end: the last step is checked like the others.
        if out.len() > MOST_EXTENSION_BYTES {
            return Err(WriteError::Limit { what: "bytes of DiaCraft's extension", max: MOST_EXTENSION_BYTES as u64 });
        }
    }
    if has_bad_char(&out) {
        return Err(refuse(super::splice::WORDS_XML_CHAR));
    }
    work.charge_copy(out.len(), limits)?;
    Ok(out)
}

/// What the words of a refusal call the one part a flat document is.
const THE_DOCUMENT: &str = "the document";

/// What the last pass writes: the generator, the root's media type and DiaCraft's extension.
#[derive(Default)]
struct Housekeeping<'a> {
    /// DiaCraft's generator in `office:meta`: an edited save writes it, an unedited one does not.
    generator: bool,
    /// The kind asked for, when the root does not say it.
    media: Option<Variant>,
    /// What the root says now, which [`Head`] found in the document as it was given: where its start tag and its `office:mimetype` value lie, and the
    /// namespaces it declares.
    head: Option<&'a Head>,
    /// DiaCraft's extension as the model has it, written when `write_extension` says it differs from the document's.
    extension: Option<&'a RawElement>,
    write_extension: bool,
}

/// The document with the last pass made (see the module documentation): `None` when nothing is to be written.
fn housekeeping(bytes: &[u8], wants: &Housekeeping<'_>, limits: &Limits, work: &mut WriteWork) -> Result<Option<Vec<u8>>, WriteError> {
    if !wants.generator && wants.media.is_none() && !wants.write_extension {
        return Ok(None);
    }
    let words = "a change to a flat document that cannot be scanned (it is written in UTF-16, or its XML is not well-formed)";
    let FlatPlan { mut splicer, root_open_end, root_close_start, root_bound, extension } =
        meta::plan_flat(bytes, wants.generator, limits, work)?.ok_or(WriteError::Unsupported(words))?;
    // The kind asked for, on the root's `office:mimetype`: its value replaced where it lies, or the attribute added to the root's start tag.
    if let Some(variant) = wants.media {
        let media = escape_value(variant.media_type(), b'"');
        let head =
            wants.head.ok_or(WriteError::MediaType("the document's root cannot be read within its first 64 KiB, so the kind cannot be stated"))?;
        work.charge_attribute(limits)?;
        match head.mimetype {
            Some((from, to)) => splicer.replace(from, to, media),
            None => {
                let prefix = match head.prefix() {
                    "" => head.attr_prefix.clone(),
                    own => Some(own.to_owned()),
                }
                .ok_or(WriteError::MediaType("the document's root binds no prefix to the office namespace to write office:mimetype under"))?;
                splicer.insert(head.tag.1.saturating_sub(1), format!(" {prefix}:mimetype=\"{media}\""));
            }
        }
    }
    // DiaCraft's extension: replaced where the document has it, put last when it has none, taken out when the model has none.
    if wants.write_extension {
        let (name, text) = match wants.extension {
            Some(el) => (Some(&*el.name), Some(element_text(el, work, limits)?)),
            None => (None, None),
        };
        match (extension, text) {
            (Some((_, start, end)), Some(text)) => splicer.replace(start, end, text),
            (Some((lead, _, end)), None) => splicer.remove(lead, end),
            (None, Some(text)) => {
                // After the document's last child, on a line of its own when the document is laid out in lines.
                let lines = bytes.get(root_close_start.saturating_sub(1)) == Some(&b'\n');
                splicer.insert(root_close_start, if lines { format!(" {text}\n") } else { text });
            }
            (None, None) => {}
        }
        // The element's own prefix must be bound where it is written: on itself, or on the root, which gets the declaration when it has none.
        if let (Some(name), Some(el)) = (name, wants.extension)
            && let Some((prefix, _)) = name.split_once(':')
        {
            let declared = el.attrs.get(&format!("xmlns:{prefix}")).is_some();
            let on_root = wants.head.and_then(|h| h.declarations.iter().find(|(p, _)| p == prefix)).map(|(_, uri)| uri.as_str());
            let known = root_bound.contains(prefix);
            if !declared && !known {
                work.charge_attribute(limits)?;
                splicer.insert(root_open_end.saturating_sub(1), format!(" xmlns:{prefix}=\"{}\"", diacraft_doc::DIACRAFT_URN));
            } else if !declared && on_root.is_some_and(|uri| !diacraft_doc::is_diacraft_urn(uri)) {
                return Err(WriteError::Unsupported("DiaCraft's extension in a prefix that the document's root binds to another namespace"));
            }
        }
    }
    Ok(Some(splicer.apply(work, limits)?))
}

/// The kind a save writes, and why: what the root says, and what [`WriteOptions::variant`] asks for. `None` when nothing is written of the kind (nothing was
/// asked, or the root already says what was asked).
fn media_to_write(head: Option<&Head>, asked: Option<Variant>) -> Result<Option<Variant>, WriteError> {
    let Some(variant) = asked else { return Ok(None) };
    let Some(head) = head else {
        return Err(WriteError::MediaType("the document's root cannot be read within its first 64 KiB, so the kind cannot be stated"));
    };
    let says = match &head.claim {
        Claim::Drawing => Some(Variant::Drawing),
        Claim::Template => Some(Variant::Template),
        _ => None,
    };
    Ok((says != Some(variant)).then_some(variant))
}

/// Checks the document a save made: it says the kind that was asked for on its root (so that a program that tells it by its first bytes tells the same kind).
fn check_media(bytes: &[u8], asked: Variant) -> Result<(), WriteError> {
    match shape::head(bytes).map(|h| h.claim) {
        Some(Claim::Drawing) if asked == Variant::Drawing => Ok(()),
        Some(Claim::Template) if asked == Variant::Template => Ok(()),
        _ => Err(WriteError::SelfCheck("the document made does not state the media type that was asked for, on its root".to_owned())),
    }
}

/// A copy of the document's bytes: the memory for it is asked for first, an error and not an abort when it is not had.
fn copy_of(bytes: &[u8]) -> Result<Vec<u8>, WriteError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len()).map_err(|_| WriteError::Limit { what: "memory for the document", max: bytes.len() as u64 })?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

/// Saves `doc`, read from `original`, with `options`, as a flat document: the original is one, or a package, whose parts become the
/// document's children and whose pictures go inline first ([`super::convert`]). See the module documentation.
pub(super) fn write(doc: &Document, original: &OriginalPackage<'_>, options: &WriteOptions, work: &mut WriteWork) -> Result<Written, WriteError> {
    if Form::of(original.bytes) == Form::Package {
        let limits = &options.limits;
        let mut opened = crate::package::open(original.bytes, limits, &mut work.package).map_err(WriteError::Original)?;
        if original.index.fingerprint().is_some_and(|f| *f != super::fingerprint_of(&opened)) {
            return Err(WriteError::Mismatch(
                "the index is the one a save made for other bytes than these: a save's index goes with the package that save gave",
            ));
        }
        let (converted, notes) = super::convert::flatten(&mut opened, limits, work)?;
        drop(opened);
        // The document the parts were put together as: each prefix of the root's declarations once, which a flat document is.
        super::unique::check_part(THE_DOCUMENT, &converted, limits, work)?;
        // The package's index is the flat document's: the elements are the same, in the same order. It goes with the bytes made.
        let index = original.index.clone().with_fingerprint(flat_fingerprint(&converted));
        let mut done = write_flat(doc, &OriginalPackage { bytes: &converted, index: &index }, options, work)?;
        done.notes.splice(0..0, notes);
        return Ok(done);
    }
    write_flat(doc, original, options, work)
}

/// [`write`] of a flat original.
fn write_flat(doc: &Document, original: &OriginalPackage<'_>, options: &WriteOptions, work: &mut WriteWork) -> Result<Written, WriteError> {
    let limits = &options.limits;
    if options.form == Some(Form::Package) {
        return Err(WriteError::Unsupported(
            "a flat drawing saved as a package (the pictures would move to Pictures/ and the automatic styles split by the part that uses them: no rule says which)",
        ));
    }
    let bytes = original.bytes;
    let head = shape::head(bytes);
    // What a save that asked for a kind writes of it: refused before the file is read when the root cannot say it.
    let media = media_to_write(head.as_ref(), options.variant)?;
    check_index(doc, original.index, limits)?;
    if original.index.fingerprint().is_some_and(|f| *f != flat_fingerprint(bytes)) {
        return Err(WriteError::Mismatch(
            "the index is the one a save made for other bytes than these: a save's index goes with the document that save gave",
        ));
    }
    let figures = crate::figures::of(doc);
    work.model_walk_steps = figures.steps;
    let grown = growth(&figures, &original.index.read_cost().figures);
    work.grown_bytes = grown;
    let beside = Beside::flat(bytes, original.index, grown);
    work.estimated_memory = beside.holds(original.index.read_cost());
    work.memory_cap = limits.max_write_memory;
    let baseline_limits = save_limits(limits, original.index.read_cost(), &beside)?;
    let (baseline, report) = read(bytes, &baseline_limits).map_err(|e| baseline_error(e, &baseline_limits, limits))?;
    let cost = report.work.cost();
    work.estimated_memory = work.estimated_memory.max(beside.holds(&cost));
    check_memory(&cost, &beside, limits)?;
    work.memory_room = limits.max_write_memory.saturating_sub(work.estimated_memory);
    work.set_tree_room(&cost, limits);
    work.take_markup_layers(&report.native);
    work.baseline_read = report.work;
    let diff = compare::compare_with(doc, &baseline, original.index, &report.index, true, limits, work)?;
    // **A group's own data has no place in a flat file**: a package keeps it in the document file's `group` entries, which a flat drawing has none of, and the comparison above
    // leaves a group's data to them (`compare::classify`), so a change of it was no edit and was dropped without a word. Refused in words, edited or not, with nothing written.
    let elements = crate::ext::docfile::Elements { st: &diff.structure, held: original.index, fresh: &report.index };
    crate::ext::docfile::refuse_flat_group_data(doc, &baseline, &elements, work)?;
    // The shapes read as copies of another (their style carries a DiaCraft id an earlier shape carries): said on every save, edited or not.
    let copies = super::duplicate_notes(&report.index);
    // DiaCraft's extension is a model field of its own: it is written when the model has changed it from the document's.
    let extension_changed = doc.diacraft != baseline.diacraft;
    drop(baseline);
    if !diff.any() && !extension_changed {
        // The document as given, and only the root's media type changed when a kind was asked for: the model is not edited either way.
        let wants = Housekeeping { media, head: head.as_ref(), ..Housekeeping::default() };
        let (out, index) = match housekeeping(bytes, &wants, limits, work)? {
            Some(made) => {
                if let Some(variant) = media {
                    check_media(&made, variant)?;
                }
                super::unique::check_part(THE_DOCUMENT, &made, limits, work)?;
                let index = if original.index.fingerprint().is_some() {
                    original.index.clone().with_fingerprint(flat_fingerprint(&made))
                } else {
                    original.index.clone()
                };
                (made, index)
            }
            None => (copy_of(bytes)?, original.index.clone()),
        };
        return Ok(Written { bytes: out, index, notes: copies, edited: false, work: work.clone() });
    }
    let version = report.office_version.clone();
    drop(report);
    let mut cost = cost;
    cost.items = cost.items.saturating_add(grown / ITEM_BYTES);
    cost.figures = figures;
    let saving = Saving { doc, diff: &diff, options, version: version.as_deref(), cost, retype: None, copies };
    edited(bytes, original, saving, media, head.as_ref(), extension_changed, work)
}

/// An edited save of a flat document: see the module documentation.
fn edited(
    original_bytes: &[u8],
    original: &OriginalPackage<'_>,
    saving: Saving<'_>,
    media: Option<Variant>,
    head: Option<&Head>,
    extension_changed: bool,
    work: &mut WriteWork,
) -> Result<Written, WriteError> {
    let Saving { doc, diff, options, cost, copies, .. } = saving;
    let limits = &options.limits;
    let mut notes = copies;
    let by_part = |kind: PartKind| -> Vec<&PageDiff> {
        diff.pages
            .iter()
            .filter(|p| {
                p.any() && matches!((kind, p.source), (PartKind::Content, PageSource::Page(_)) | (PartKind::Styles, PageSource::MasterPage(_)))
            })
            .collect()
    };
    let pages_plan = pages::plan(&mut Source::Flat(original_bytes), diff, doc, limits, work)?;
    let plans = &pages_plan.plans;
    let mut outs: Vec<(PartKind, PartOut)> = Vec::new();
    let mut taken: Vec<(super::scan::Fam, String)> = Vec::new();
    let mut needs: std::collections::BTreeSet<super::markers::Def> = std::collections::BTreeSet::new();
    // The markers of DiaCraft's name that a style named before the save and none names after it: the second pass takes them out of `office:styles`.
    // In a flat document the first pass looks at every style of the document, so what it finds named no more is named by none.
    let mut orphans: std::collections::BTreeSet<super::markers::Def> = std::collections::BTreeSet::new();
    // Each pass scans what the one before made: the document is held as `current`, the original's bytes until a pass makes new ones.
    let mut current: Cow<'_, [u8]> = Cow::Borrowed(original_bytes);
    for kind in [PartKind::Content, PartKind::Styles] {
        let pages = by_part(kind);
        if pages.is_empty() && !(kind == PartKind::Styles && (!plans.is_empty() || !needs.is_empty() || !orphans.is_empty())) {
            continue;
        }
        // The markers of the line ends the first pass named go into `office:styles`, which a document that has none is given first, at its place.
        if kind == PartKind::Styles && !needs.is_empty() {
            current = ensure_styles(current, limits, work)?;
        }
        let ctx = PartCtx {
            plans: &pages_plan,
            structure: &diff.structure,
            added: &[],
            doc,
            taken: &taken,
            markers: &needs,
            orphans: &orphans,
            styles: &super::newstyles::Plan::default(),
            layers: &[],
            layer_flags: &[],
        };
        // A flat document is both parts: the name of the part is no one's here (`Source::Flat` gives the document for any).
        let made = splice_part(&mut Source::Flat(&current), "office:document", kind, &pages, original.index, &ctx, None, limits, work, &mut notes)?;
        let (part, out) = made;
        taken.extend(out.styles.iter().map(|s| (s.family, s.name.clone())));
        if kind == PartKind::Content {
            needs.clone_from(&out.markers);
            orphans = super::orphaned_markers(&part, &out.marker_refs, limits, work)?;
        }
        current = Cow::Owned(part);
        outs.push((kind, out));
    }
    let wants = Housekeeping { generator: true, media, head, extension: doc.diacraft.as_deref(), write_extension: extension_changed };
    if let Some(made) = housekeeping(&current, &wants, limits, work)? {
        current = Cow::Owned(made);
    }
    let bytes: Vec<u8> = current.into_owned();
    if let Some(variant) = media {
        check_media(&bytes, variant)?;
    }
    let fingerprint = flat_fingerprint(&bytes);
    let index = new_index(original.index, &outs, cost, doc, diff, fingerprint);
    super::check_pages(&bytes, true, &pages_plan, limits, work)?;
    let mut tags: Vec<super::EditedTag> = Vec::new();
    let held: usize = outs.iter().map(|(_, out)| out.edited.len()).sum();
    tags.try_reserve_exact(held).map_err(|_| WriteError::Limit { what: "memory for the attributes written", max: held as u64 })?;
    for (_, out) in outs.iter_mut() {
        tags.append(&mut out.edited);
    }
    tags.sort_unstable_by_key(super::EditedTag::key);
    super::self_check(&bytes, true, &index, &tags, limits, work)?;
    super::check_made_styles(&bytes, true, &outs, limits, work)?;
    super::check_made_texts(&bytes, true, &outs, limits, work)?;
    super::check_read_back(&bytes, &outs, limits, work)?;
    super::check_tier2(&bytes, &outs, limits, work)?;
    super::check_glue(&bytes, &outs, limits, work)?;
    super::check_made_kids(&bytes, true, &outs, limits, work)?;
    super::check_made_markers(&bytes, true, &outs, limits, work)?;
    if extension_changed {
        check_extension(&bytes, doc.diacraft.as_deref(), limits, work)?;
    }
    // Last, so that a refusal of the checks above says what is wrong with the document first: no start tag of the document made names an attribute twice.
    super::unique::check_part(THE_DOCUMENT, &bytes, limits, work)?;
    Ok(Written { bytes, index, notes, edited: true, work: work.clone() })
}

/// The document with an empty `office:styles` where R30 puts it (before `office:automatic-styles`, else `office:master-styles`, else `office:body`) when it has
/// none: the place the second pass puts the markers of line ends in (a child the save needs and the file lacks is inserted at its place). A document that has
/// the element is given back as it is.
fn ensure_styles<'a>(bytes: Cow<'a, [u8]>, limits: &Limits, work: &mut WriteWork) -> Result<Cow<'a, [u8]>, WriteError> {
    let ask = super::scan::Ask { styles: true, ..super::scan::Ask::default() };
    let scan = super::scan::scan_part_asking(&bytes, PartKind::Styles, &ask, limits, work)?;
    if scan.common.is_some() {
        return Ok(bytes);
    }
    let anchor = [shape::Child::AutomaticStyles, shape::Child::MasterStyles, shape::Child::Body]
        .iter()
        .filter_map(|c| scan.children.get(c.rank()).and_then(Option::as_ref))
        .min_by_key(|c| c.start)
        .ok_or(WriteError::Unsupported("a flat document with no office:styles and no place to put one"))?;
    if anchor.declares {
        return Err(WriteError::Unsupported("a flat document with no office:styles whose next child declares a namespace of its own"));
    }
    let name = match anchor.qname.split_once(':') {
        Some((prefix, _)) => format!("{prefix}:styles"),
        None => "styles".to_owned(),
    };
    let mut splicer = super::splice::Splicer::new(&bytes);
    splicer.insert(anchor.start, format!("<{name}/>"));
    Ok(Cow::Owned(splicer.apply(work, limits)?))
}

/// Checks that the document made holds DiaCraft's extension as the model has it: the element the scan finds first in DiaCraft's namespace is the one text the
/// model writes as, or there is none when the model has none.
fn check_extension(bytes: &[u8], wanted: Option<&RawElement>, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let plan = meta::plan_flat(bytes, false, limits, work)?.ok_or_else(|| failed("the document made does not scan"))?;
    match (plan.extension, wanted) {
        (None, None) => Ok(()),
        (Some((_, start, end)), Some(el)) => {
            let text = element_text(el, work, limits)?;
            if bytes.get(start..end) == Some(text.as_bytes()) {
                Ok(())
            } else {
                Err(failed("DiaCraft's extension is not in the document made as the model has it"))
            }
        }
        (None, Some(_)) => Err(failed("DiaCraft's extension was not written")),
        (Some(_), None) => Err(failed("DiaCraft's extension was not taken out")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::RawAttrs;

    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
        RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
    }

    fn text_of(el: &RawElement) -> Result<String, WriteError> {
        element_text(el, &mut WriteWork::default(), &Limits::default())
    }

    #[test]
    fn a_name_is_an_xml_name_and_nothing_that_ends_one() {
        for ok in ["a", "dc:Data", "_x", ":a", "a-b.c", "a1", "é"] {
            assert!(is_name(ok), "{ok}");
        }
        for bad in ["", "1a", "-a", "a b", "a<b", "a=b", "a/b", "a\"b", "a>b", "a&b", " a", "a\n"] {
            assert!(!is_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn text_is_escaped_as_content_and_a_carriage_return_is_a_reference() {
        let mut out = String::new();
        escape_text("a&b<c>d\re'\"", &mut out);
        assert_eq!(out, "a&amp;b&lt;c&gt;d&#13;e'\"");
    }

    #[test]
    fn an_extension_is_written_as_it_is_held_and_nothing_is_added() {
        let child = el("dc:snap", &[("n", "1"), ("t", "a\"b&c\nd")], Vec::new());
        let data = el(
            "dc:Data",
            &[("xmlns:dc", "urn:x"), ("dc:version", "1")],
            vec![
                RawNode::Text(Arc::from(" ")),
                RawNode::Element(child),
                RawNode::Comment(Arc::from(" kept ")),
                RawNode::Pi { target: Arc::from("t"), data: Arc::from("d") },
                RawNode::Pi { target: Arc::from("e"), data: Arc::from("") },
                RawNode::Text(Arc::from("x<y")),
                RawNode::Element(el("dc:e", &[], Vec::new())),
            ],
        );
        assert_eq!(
            text_of(&data).unwrap(),
            "<dc:Data xmlns:dc=\"urn:x\" dc:version=\"1\"> <dc:snap n=\"1\" t=\"a&quot;b&amp;c&#10;d\"/><!-- kept --><?t d?><?e?>x&lt;y<dc:e/></dc:Data>"
        );
        assert_eq!(text_of(&el("a", &[], Vec::new())).unwrap(), "<a/>");
    }

    #[test]
    fn an_extension_with_what_xml_does_not_allow_is_refused_in_words() {
        let refused = |e: RawElement| matches!(text_of(&e), Err(WriteError::Unsupported(_)));
        assert!(refused(el("a b", &[], Vec::new())), "an element name");
        assert!(refused(el("a", &[("b c", "1")], Vec::new())), "an attribute name");
        assert!(refused(el("a", &[], vec![RawNode::Comment(Arc::from("x--y"))])), "a comment with two hyphens");
        assert!(refused(el("a", &[], vec![RawNode::Comment(Arc::from("x-"))])), "a comment that ends in one");
        assert!(refused(el("a", &[], vec![RawNode::Pi { target: Arc::from("xml"), data: Arc::from("") }])), "xml as a target");
        assert!(refused(el("a", &[], vec![RawNode::Pi { target: Arc::from("XML"), data: Arc::from("") }])), "XML as a target");
        assert!(refused(el("a", &[], vec![RawNode::Pi { target: Arc::from("t"), data: Arc::from("a?>b") }])), "an end in the data");
        assert!(refused(el("a", &[], vec![RawNode::Pi { target: Arc::from("a b"), data: Arc::from("") }])), "a target that is no name");
        assert!(refused(el("a", &[], vec![RawNode::Text(Arc::from("\u{1}"))])), "a control character");
        assert!(refused(el("a", &[("b", "\u{0}")], Vec::new())), "a control character in a value");
    }

    #[test]
    fn an_extension_is_written_to_the_depth_a_document_can_hold_and_no_deeper() {
        let nested = |depth: usize| {
            let mut inner = el("leaf", &[], Vec::new());
            for _ in 1..depth {
                inner = el("n", &[], vec![RawNode::Element(inner)]);
            }
            inner
        };
        assert!(text_of(&nested(MOST_EXTENSION_DEPTH)).is_ok());
        assert!(matches!(text_of(&nested(MOST_EXTENSION_DEPTH + 1)), Err(WriteError::Unsupported(_))));
    }

    #[test]
    fn an_extension_is_written_up_to_its_bytes_exactly_and_siblings_are_no_depth() {
        // The cap's words: sixteen mebibytes, as the documentation says.
        assert_eq!(MOST_EXTENSION_BYTES, 16_777_216);
        // `<a v="..."/>` is nine bytes and the value: exactly the cap is written, one more is not.
        let at = |bytes: usize| el("a", &[("v", &"x".repeat(bytes - 9))], Vec::new());
        assert_eq!(text_of(&at(MOST_EXTENSION_BYTES)).map(|t| t.len()).ok(), Some(MOST_EXTENSION_BYTES));
        assert!(matches!(text_of(&at(MOST_EXTENSION_BYTES + 1)), Err(WriteError::Limit { .. })));
        // A megabyte of text in one value is what the reader's attribute cap admits, and is written whole.
        assert!(text_of(&el("a", &[("v", &"y".repeat(1 << 20))], Vec::new())).is_ok());
        // Many empty children are siblings, not levels: past the depth cap by count and written all the same.
        let many = el("a", &[], (0..MOST_EXTENSION_DEPTH * 3).map(|_| RawNode::Element(el("b", &[], Vec::new()))).collect());
        let text = text_of(&many).unwrap();
        assert_eq!(text.matches("<b/>").count(), MOST_EXTENSION_DEPTH * 3);
        // Nor are the children of siblings: each closes before the next opens.
        let deep: Vec<RawNode> =
            (0..MOST_EXTENSION_DEPTH * 2).map(|_| RawNode::Element(el("c", &[], vec![RawNode::Element(el("d", &[], Vec::new()))]))).collect();
        assert!(text_of(&el("a", &[], deep)).is_ok());
    }

    #[test]
    fn an_extension_is_held_to_its_bytes_and_charged_to_the_copy() {
        let big = el("a", &[("v", &"x".repeat(MOST_EXTENSION_BYTES / 2))], Vec::new());
        let two = el("a", &[], vec![RawNode::Element(big.clone()), RawNode::Element(big.clone()), RawNode::Element(big)]);
        assert!(matches!(text_of(&two), Err(WriteError::Limit { what: "bytes of DiaCraft's extension", .. })));
        // Charged: the text's length is added to the bytes copied.
        let mut work = WriteWork::default();
        let text = element_text(&el("a", &[("b", "c")], Vec::new()), &mut work, &Limits::default()).unwrap();
        assert_eq!(work.bytes_copied, text.len() as u64);
        // And past the cap of copies, refused.
        let limits = Limits { max_write_bytes: 3, ..Limits::default() };
        assert!(matches!(element_text(&el("a", &[("b", "c")], Vec::new()), &mut WriteWork::default(), &limits), Err(WriteError::Limit { .. })));
    }

    fn head_of(text: &str) -> Option<Head> {
        shape::head(text.as_bytes())
    }

    const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

    #[test]
    fn a_kind_is_written_when_the_root_does_not_say_it() {
        let root = |media: Option<&str>| {
            let attr = media.map(|m| format!(" office:mimetype=\"{m}\"")).unwrap_or_default();
            head_of(&format!("<office:document xmlns:office=\"{OFFICE}\"{attr}/>"))
        };
        let drawing = Some("application/vnd.oasis.opendocument.graphics");
        let template = Some("application/vnd.oasis.opendocument.graphics-template");
        assert_eq!(media_to_write(root(drawing).as_ref(), None).unwrap(), None, "nothing asked");
        assert_eq!(media_to_write(root(drawing).as_ref(), Some(Variant::Drawing)).unwrap(), None, "the root says it");
        assert_eq!(media_to_write(root(template).as_ref(), Some(Variant::Template)).unwrap(), None);
        assert_eq!(media_to_write(root(drawing).as_ref(), Some(Variant::Template)).unwrap(), Some(Variant::Template));
        assert_eq!(media_to_write(root(template).as_ref(), Some(Variant::Drawing)).unwrap(), Some(Variant::Drawing));
        // Nothing, or what is no kind this crate knows, is given the kind asked for.
        for odd in [None, Some("application/vnd.oasis.opendocument.graphics-flat-xml"), Some("x")] {
            assert_eq!(media_to_write(root(odd).as_ref(), Some(Variant::Drawing)).unwrap(), Some(Variant::Drawing), "{odd:?}");
            assert_eq!(media_to_write(root(odd).as_ref(), Some(Variant::Template)).unwrap(), Some(Variant::Template), "{odd:?}");
        }
        // A root that cannot be told cannot state one.
        assert!(matches!(media_to_write(None, Some(Variant::Drawing)), Err(WriteError::MediaType(_))));
        assert_eq!(media_to_write(None, None).unwrap(), None);
    }

    #[test]
    fn the_document_made_is_checked_for_the_kind_on_its_root() {
        let doc = |m: &str| format!("<office:document xmlns:office=\"{OFFICE}\" office:mimetype=\"{m}\"/>");
        let drawing = doc("application/vnd.oasis.opendocument.graphics");
        let template = doc("application/vnd.oasis.opendocument.graphics-template");
        assert!(check_media(drawing.as_bytes(), Variant::Drawing).is_ok());
        assert!(check_media(template.as_bytes(), Variant::Template).is_ok());
        assert!(check_media(drawing.as_bytes(), Variant::Template).is_err());
        assert!(check_media(template.as_bytes(), Variant::Drawing).is_err());
        assert!(check_media(doc("x").as_bytes(), Variant::Drawing).is_err());
        assert!(check_media(b"not xml", Variant::Drawing).is_err());
    }

    #[test]
    fn the_fingerprint_of_a_flat_document_is_the_one_part() {
        let f = flat_fingerprint(b"<a/>");
        assert_eq!(f.styles, (0, 0));
        assert_eq!(f.content, shape::fingerprint(b"<a/>"));
        assert_ne!(f, flat_fingerprint(b"<b/>"));
    }

    #[test]
    fn the_extension_the_document_made_holds_is_checked_against_the_model() {
        let urn = diacraft_doc::DIACRAFT_URN;
        let doc = |inner: &str| format!("<office:document xmlns:office=\"{OFFICE}\" xmlns:dc=\"{urn}\">{inner}</office:document>").into_bytes();
        let wanted = el("dc:Data", &[("dc:version", "1")], Vec::new());
        let held = doc("<office:body/><dc:Data dc:version=\"1\"/>");
        let check = |bytes: &[u8], wanted: Option<&RawElement>| check_extension(bytes, wanted, &Limits::default(), &mut WriteWork::default());
        assert!(check(&held, Some(&wanted)).is_ok(), "the element is there as the model has it");
        assert!(check(&doc("<office:body/>"), None).is_ok(), "none wanted, none there");
        // Another text, none written, one that should be gone, and a document that does not scan: each a fault of the save.
        assert!(check(&doc("<dc:Data dc:version=\"2\"/>"), Some(&wanted)).is_err());
        assert!(check(&doc("<office:body/>"), Some(&wanted)).is_err());
        assert!(check(&held, None).is_err());
        assert!(check(b"<a><b></a>", None).is_err());
    }

    #[test]
    fn a_save_that_asks_for_nothing_of_the_root_writes_nothing_there() {
        let bytes = format!("<office:document xmlns:office=\"{OFFICE}\"/>");
        let wants = Housekeeping::default();
        assert!(housekeeping(bytes.as_bytes(), &wants, &Limits::default(), &mut WriteWork::default()).unwrap().is_none());
        // The copy of the document is the document, and the memory for it is asked for first.
        assert_eq!(copy_of(bytes.as_bytes()).unwrap(), bytes.as_bytes());
        assert_eq!(copy_of(b"").unwrap(), b"");
    }
}
