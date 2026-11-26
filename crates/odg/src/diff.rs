//! An element-level comparison of two OpenDocument packages (design, "Order of work" step 3): which parts are the same bytes, which differ
//! in bytes only, and, in each XML part whose tree differs, which elements were added, removed, moved or changed, each named by its path
//! (`/office:document-content/office:body/office:drawing/draw:page[2]/draw:custom-shape[7]`), and which changed in white space alone.
//!
//! # What is compared
//!
//! * **Entries** by exact name: those only in the second are `added`, those only in the first `removed`, files in both with the same
//!   bytes are `identical`. A file in both whose compression method differs is in `methods_changed`; entries in both that lie in another
//!   order in the file make `order_changed`; entries in both whose headers say something else, besides where they lie (the versions, the
//!   flags, the time or the date, the attributes, the extra field of either header, or the row's comment), are in `headers_changed`, with
//!   what changed, as an edited save must keep them. **One flag is the writer's own on an entry whose bytes differ**: the
//!   data descriptor's bit (bit 3). A save that changes an entry writes it under headers that state its sizes and checksum, and no descriptor
//!   (valid ZIP, the sizes are known: [`ArchiveWriter::replace`](diacraft_container::zip::ArchiveWriter::replace)), so a descriptor the file had
//!   goes with the entry's old bytes and is no change of the header (census: 16 of 19 real templates carry one on their manifest). On
//!   an entry whose bytes are the same the bit is compared like the rest, since a copy that gives it up has rewritten the header.
//! * **XML parts** (a name that ends in `.xml` or `.rdf`) whose bytes differ are parsed, each side within the container's XML limits and
//!   this crate's [`Limits::max_xml_bytes`], and compared as trees. Equal trees (written differently: quotes, a declaration, references)
//!   put the part in `bytes_only`; a part that does not parse on either side is in `unparsed` with the XML class. Any other part whose
//!   bytes differ is in `bytes_only` (a picture, the thumbnail).
//!
//! # How two trees are compared
//!
//! Names are compared as written, prefixes included, and attributes by name as written; a round trip keeps both. Each element of each tree
//! is given, in one pass from the leaves up, a 128-bit digest of everything in it (its name, its attributes in order, and its children in
//! order, text, CDATA, comments and instructions included), and two elements with equal digests are taken as equal. Then the walk starts
//! at the two roots and goes down only where digests differ, once for each pair of elements:
//!
//! 1. The pair's own attributes: each added, removed or changed attribute is named in an [`ElementChange`]; the same attributes in another
//!    order make it `reordered`.
//! 2. The pair's own content: its text, CDATA, comments and instructions in order, white-space-only text left out, make `text` when they
//!    differ; when they do not and the white-space-only text does, the element is listed in `whitespace` and not in `changed`.
//! 3. The pair's child elements: the longest run of equal children at the front and at the back is set aside. Of the children between, those
//!    equal to one on the other side are matched (in order, by digest); those of them outside the longest run that keeps its order are
//!    `moved`, each with its path before and after. The children left are matched by name, the k-th left of a name with the k-th left of
//!    that name on the other side, and each such pair is walked in turn; a child with no partner is `added` or `removed`, with its whole
//!    content, and nothing under it is listed.
//!
//! A pair whose digests differ always lists something: when nothing above applies, its text and child elements are where they were but
//! interleaved otherwise, which is listed as `whitespace` when only white-space-only text moved and as a change of `text` otherwise.
//!
//! A path names each element from the root by its name as written and, where its parent has more than one child element of that name, its
//! place among them from 1 (`draw:page[2]`). A path of the first package is given for what was removed or changed, and of the second for
//! what was added; a move gives both. The two numberings differ where elements were added or removed before.
//!
//! Each path, and each attribute named, also has a **label** for a report that a person who must not see the file reads: the
//! same steps and places, each name resolved by its namespace URI (never by the prefix the file wrote) and shown under that namespace's own
//! prefix when the specs record it, and `(unlisted)` otherwise (`vocab.rs`). So `<d:rect>` with `d` bound to the drawing namespace is
//! labelled `draw:rect`, and `<draw:MyPassword>` is labelled `(unlisted)`.
//!
//! **The digests are no defence against a file made to fool them.** They are SipHash with fixed keys, which anyone can collide on purpose
//! with about 2^64 work, and two subtrees with equal digests are taken as equal unchecked. The bytes of a part are compared before any
//! digest, so a collision cannot make two different parts the same; at worst it hides which element of a part already listed changed. The
//! comparison is a tool for tests and harnesses, and never a security decision.
//!
//! # What it costs
//!
//! Each tree is walked once for its digests (each element, attribute and text once) and the comparison visits each element at most once
//! as a child of a pair it walks, so the work is linear in the two parts, however deep, however many siblings follow a change, and however
//! much white space there is; the longest ordered run of the matched children costs `k log k` for `k` of them. [`DiffWork`] counts each loop
//! in its units. Lists are capped: at most [`Limits::max_diff_entries`] entries are listed over the whole comparison, and the counts of
//! each part ([`DiffCounts`]) say how many there were; a path is cut at [`MAX_PATH_BYTES`] and each name in it at [`MAX_PATH_NAME_BYTES`].

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use diacraft_container::Work;
use diacraft_container::xml::{self, El, Node};
use diacraft_container::zip::Entry;

use crate::error::{LimitKind, OdgError};
use crate::limits::{Limits, MAX_PATH_BYTES, MAX_PATH_NAME_BYTES};
use crate::ns::{self, Flow};
use crate::package::{self, Package};
use crate::vocab;

/// The most attribute names one [`ElementChange`] lists; `attribute_count` says how many changed.
const MOST_ATTRIBUTES: usize = 32;

/// What differs between two packages, as [`diff_packages`] finds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageDiff {
    /// Files in both packages with the same bytes.
    pub identical: usize,
    /// The XML parts in both whose trees differ, in the order of the first package.
    pub parts: Vec<PartDiff>,
    /// Parts in both whose bytes differ and whose trees do not, or that are not XML.
    pub bytes_only: Vec<String>,
    /// XML parts in both whose bytes differ and that do not parse on one side or both.
    pub unparsed: Vec<Unparsed>,
    /// Entries, files and folders, only in the second package, in its order.
    pub added: Vec<String>,
    /// Entries only in the first package, in its order.
    pub removed: Vec<String>,
    /// Files in both whose compression method differs.
    pub methods_changed: Vec<String>,
    /// Entries in both whose headers say something else (besides where they lie, and the method, which `methods_changed` lists).
    pub headers_changed: Vec<HeaderChange>,
    /// The entries in both lie in another order in the second file.
    pub order_changed: bool,
}

impl PackageDiff {
    /// Whether the two packages hold the same entries with the same bytes, methods and headers, in the same order.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
            && self.bytes_only.is_empty()
            && self.unparsed.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.methods_changed.is_empty()
            && self.headers_changed.is_empty()
            && !self.order_changed
    }
}

/// An entry in both packages whose headers say something else: its name, and which of `version_made_by`, `version_needed`, `flags`,
/// `time`, `date`, `attributes` (internal or external), `extra` (the row's extra field), `local_extra` (the local header's) and `comment`
/// (the row's) differ, in that order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderChange {
    pub name: String,
    pub fields: Vec<&'static str>,
}

/// An element's path, as written, and its label for a report (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementPath {
    /// Each step the element's name as the file writes it, prefix included, and its place where the name is shared.
    pub path: String,
    /// The same steps, each name by its namespace and the vocabulary of the specs, `(unlisted)` otherwise.
    pub label: String,
}

impl PartialEq<String> for ElementPath {
    /// An element path is its path as written, for a test that says which element it expects.
    fn eq(&self, other: &String) -> bool {
        self.path == *other
    }
}

impl PartialEq<&str> for ElementPath {
    fn eq(&self, other: &&str) -> bool {
        self.path == *other
    }
}

/// An XML part that could not be compared as a tree, and the XML class of why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unparsed {
    pub part: String,
    pub class: &'static str,
}

/// What differs in one XML part.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartDiff {
    /// The part's name.
    pub part: String,
    /// Elements only in the second tree (paths in it), each with all it holds.
    pub added: Vec<ElementPath>,
    /// Elements only in the first tree (paths in it).
    pub removed: Vec<ElementPath>,
    /// Elements in both whose own attributes or content differ (paths in the first).
    pub changed: Vec<ElementChange>,
    /// Elements equal on both sides that are in another order among their siblings.
    pub moved: Vec<Moved>,
    /// Elements in both whose only own difference is white-space-only text (paths in the first).
    pub whitespace: Vec<ElementPath>,
    /// How many of each there were, listed or not.
    pub counts: DiffCounts,
}

impl PartDiff {
    /// Whether nothing was found to differ.
    fn is_empty(&self) -> bool {
        self.counts == DiffCounts::default()
    }
}

/// How many entries of each kind a part's comparison found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffCounts {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub moved: usize,
    pub whitespace: usize,
}

/// An element whose own attributes or content differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementChange {
    /// Its path in the first tree, as written.
    pub path: String,
    /// The path's label (see the module documentation).
    pub label: String,
    /// The attributes added, removed or changed, by name as written: those of the first in their order, then those only in the second. At
    /// most 32 are named, each cut as a path's names are.
    pub attributes: Vec<String>,
    /// Each of `attributes` by its namespace and the vocabulary of the specs, `(unlisted)` otherwise (a namespace declaration included).
    pub attribute_labels: Vec<&'static str>,
    /// How many attributes were added, removed or changed.
    pub attribute_count: usize,
    /// The same attributes with the same values, in another order.
    pub reordered: bool,
    /// Its own text, CDATA, comments or instructions differ (white-space-only text aside), or are interleaved with its children otherwise.
    pub text: bool,
}

/// An element that is equal on both sides and is in another order among its siblings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// Its path in the first tree.
    pub from: ElementPath,
    /// Its path in the second.
    pub to: ElementPath,
}

/// What a comparison cost, in the units each loop walks. Every field only grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffWork {
    /// Reading the two packages and parsing their XML parts: the container's counts.
    pub xml: Work,
    /// Elements given a digest: each element of each tree compared, once.
    pub hashed: u64,
    /// Bytes put through the digest: names, attributes and text, once each.
    pub hashed_bytes: u64,
    /// Pairs of elements walked.
    pub pairs: u64,
    /// Child elements looked at by the walk of their parents' pair.
    pub children: u64,
    /// Steps of the search for the longest ordered run of matched children: one for each matched child and each comparison of its search.
    pub ordered_steps: u64,
    /// Elements given a label: each element of each tree compared, once.
    pub labelled: u64,
    /// Bytes of paths written, and of their labels.
    pub path_bytes: u64,
}

/// Compares the packages in `a` and `b`, read within `limits`. Either that does not read is the error it gives ([`Package::read`]).
pub fn diff_packages(a: &[u8], b: &[u8], limits: &Limits) -> Result<PackageDiff, OdgError> {
    diff_packages_counted(a, b, limits, &mut DiffWork::default())
}

/// [`diff_packages`], adding what it cost to `work`.
pub fn diff_packages_counted(a: &[u8], b: &[u8], limits: &Limits, work: &mut DiffWork) -> Result<PackageDiff, OdgError> {
    let first = package::read_package(a, limits, &mut work.xml, false, package::Inflate::All)?;
    let second = package::read_package(b, limits, &mut work.xml, false, package::Inflate::All)?;
    let mut diff = PackageDiff::default();
    let mut budget = limits.max_diff_entries;
    let mut parsed = [0u64; 2];
    for entry in first.entries() {
        let name = entry.name.as_str();
        let Some(other) = second.entry(name) else {
            diff.removed.push(name.to_owned());
            continue;
        };
        // An entry whose bytes differ was written anew by the save, which states its sizes and checksum in its headers and has no descriptor for them.
        let rewritten = !entry.folder && matches!((first.part(name), second.part(name)), (Some(x), Some(y)) if x != y);
        let fields = header_changes((entry, a), (other, b), if rewritten { FLAG_DESCRIPTOR } else { 0 });
        if !fields.is_empty() {
            diff.headers_changed.push(HeaderChange { name: name.to_owned(), fields });
        }
        if entry.folder {
            continue;
        }
        if entry.method != other.method {
            diff.methods_changed.push(name.to_owned());
        }
        let (Some(x), Some(y)) = (first.part(name), second.part(name)) else { continue };
        if x == y {
            diff.identical += 1;
            continue;
        }
        if !is_xml_name(name) {
            diff.bytes_only.push(name.to_owned());
            continue;
        }
        let trees = (parse_side(name, x, &mut parsed[0], limits, work)?, parse_side(name, y, &mut parsed[1], limits, work)?);
        match trees {
            (Ok(ta), Ok(tb)) => {
                let part = compare_trees(name, &ta, &tb, &mut budget, work);
                if part.is_empty() {
                    diff.bytes_only.push(name.to_owned());
                } else {
                    diff.parts.push(part);
                }
            }
            (Err(class), _) | (_, Err(class)) => diff.unparsed.push(Unparsed { part: name.to_owned(), class }),
        }
    }
    for entry in second.entries() {
        if first.entry(&entry.name).is_none() {
            diff.added.push(entry.name.clone());
        }
    }
    diff.order_changed = order_in_file(&first, &second) != order_in_file(&second, &first);
    Ok(diff)
}

/// The general-purpose flag of a data descriptor (APPNOTE 4.4.4, bit 3): the sizes and the checksum follow the data.
const FLAG_DESCRIPTOR: u16 = 0x0008;

/// Which header fields of one entry differ between its two files (see [`HeaderChange`]): numbers from the rows the reader gave, and the
/// extra fields and the comment read again from each file's bytes, where the reader found them (so they are always there to read). The bits of
/// the flags in `owned` are not compared: they are the save's to write.
fn header_changes((x, a): (&Entry, &[u8]), (y, b): (&Entry, &[u8]), owned: u16) -> Vec<&'static str> {
    let (row_x, row_y) = (x.row_extra_and_comment(a), y.row_extra_and_comment(b));
    let differs = [
        ("version_made_by", x.version_made_by != y.version_made_by),
        ("version_needed", x.version_needed != y.version_needed),
        ("flags", x.flags & !owned != y.flags & !owned),
        ("time", x.time != y.time),
        ("date", x.date != y.date),
        ("attributes", (x.internal_attributes, x.external_attributes) != (y.internal_attributes, y.external_attributes)),
        ("extra", row_x.map(|r| r.0) != row_y.map(|r| r.0)),
        ("local_extra", x.local_extra(a) != y.local_extra(b)),
        ("comment", row_x.map(|r| r.1) != row_y.map(|r| r.1)),
    ];
    differs.into_iter().filter(|(_, differ)| *differ).map(|(field, _)| field).collect()
}

/// The names of `this`'s entries that `other` has too, in the order they lie in `this` file.
fn order_in_file<'p>(this: &'p Package, other: &Package) -> Vec<&'p str> {
    let mut entries: Vec<(usize, &str)> =
        this.entries().filter(|e| other.entry(&e.name).is_some()).map(|e| (e.local_offset, e.name.as_str())).collect();
    entries.sort_unstable();
    entries.into_iter().map(|(_, name)| name).collect()
}

/// Whether a part is compared as XML: by its name, `.xml` or `.rdf` in any case.
fn is_xml_name(name: &str) -> bool {
    let lower = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    lower.ends_with(".xml") || lower.ends_with(".rdf")
}

/// One side's part `name` parsed: the tree, or the XML class of why not. Its bytes are added to that side's total first, which may not pass
/// [`Limits::max_xml_bytes`]. The manifest is read as the package reader reads it, with a DOCTYPE of an external id alone allowed (ruling
///); every other part is not.
fn parse_side(name: &str, bytes: &[u8], parsed: &mut u64, limits: &Limits, work: &mut DiffWork) -> Result<Result<El, &'static str>, OdgError> {
    *parsed = parsed.saturating_add(bytes.len() as u64);
    if *parsed > limits.max_xml_bytes {
        return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
    }
    let options = xml::ParseOptions { allow_external_doctype: name == package::MANIFEST_PATH, ..xml::ParseOptions::default() };
    Ok(xml::parse_with_options(bytes, &limits.container.xml, options, &mut work.xml).map_err(|e| e.class()))
}

// ------------------------------------------------------------------------------------------------------------------------ the digests

/// A tree's elements in document order, each with its digest and the number of elements in it (itself included), so that the first child
/// of the element at `i` is at `i + 1` and each next one is after the last one's elements.
struct Flat<'t> {
    els: Vec<&'t El>,
    digest: Vec<u128>,
    size: Vec<usize>,
    /// Each element's label, and its attributes' labels, the attributes of the element at `i` from `attr_start[i]` on, in their order.
    labels: Vec<&'static str>,
    attr_labels: Vec<&'static str>,
    attr_start: Vec<usize>,
}

/// The labels of every element of `root` and of every attribute, in document order, which is the order of [`flatten`]: one walk that
/// resolves each name by its namespace ([`crate::ns::walk`]), one look at the vocabulary for each name.
fn label_tree(root: &El, work: &mut DiffWork) -> (Vec<&'static str>, Vec<&'static str>, Vec<usize>) {
    let (mut labels, mut attr_labels, mut attr_start) = (Vec::new(), Vec::new(), Vec::new());
    let walked = ns::walk(root, |el, _, scopes| {
        let (uri, local) = scopes.element(&el.name);
        labels.push(vocab::label(uri, local));
        attr_start.push(attr_labels.len());
        attr_labels.extend(el.attrs.iter().map(|(key, _)| {
            if ns::is_declaration(key) {
                vocab::UNLISTED
            } else {
                let (uri, local) = scopes.attribute(key);
                vocab::label(uri, local)
            }
        }));
        Flow::Into
    });
    work.labelled = work.labelled.saturating_add(walked);
    (labels, attr_labels, attr_start)
}

/// Whether text is white space only, as XML defines it.
fn is_blank(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// A 128-bit digest: two SipHash states started differently, fed the same.
struct Digest(DefaultHasher, DefaultHasher);

impl Digest {
    fn new() -> Digest {
        let (mut a, mut b) = (DefaultHasher::new(), DefaultHasher::new());
        0x5A_u8.hash(&mut a);
        0xA5_u8.hash(&mut b);
        Digest(a, b)
    }

    fn put<T: Hash + ?Sized>(&mut self, value: &T) {
        value.hash(&mut self.0);
        value.hash(&mut self.1);
    }

    fn finish(&self) -> u128 {
        (u128::from(self.0.finish()) << 64) | u128::from(self.1.finish())
    }
}

/// The tree of `root` flattened, with every element's digest, from the leaves up.
fn flatten<'t>(root: &'t El, work: &mut DiffWork) -> Flat<'t> {
    let mut els: Vec<&'t El> = Vec::new();
    let mut stack = vec![root];
    while let Some(el) = stack.pop() {
        els.push(el);
        stack.extend(el.children.iter().rev().filter_map(|n| match n {
            Node::El(child) => Some(child),
            _ => None,
        }));
    }
    let n = els.len();
    let (mut digest, mut size) = (vec![0u128; n], vec![1usize; n]);
    for i in (0..n).rev() {
        let Some(el) = els.get(i) else { continue };
        work.hashed = work.hashed.saturating_add(1);
        let mut d = Digest::new();
        d.put(el.name.as_str());
        d.put(&el.attrs.len());
        let mut bytes = el.name.len();
        for (k, v) in &el.attrs {
            d.put(k.as_str());
            d.put(v.as_str());
            bytes = bytes.saturating_add(k.len()).saturating_add(v.len());
        }
        let mut child = i + 1;
        let mut total = 1usize;
        for node in &el.children {
            match node {
                Node::El(_) => {
                    d.put(&0u8);
                    d.put(&digest.get(child).copied().unwrap_or_default());
                    let s = size.get(child).copied().unwrap_or(1);
                    total = total.saturating_add(s);
                    child = child.saturating_add(s);
                }
                Node::Text(t) => {
                    d.put(&1u8);
                    d.put(t.as_str());
                    bytes = bytes.saturating_add(t.len());
                }
                Node::CData(t) => {
                    d.put(&2u8);
                    d.put(t.as_str());
                    bytes = bytes.saturating_add(t.len());
                }
                Node::Comment(t) => {
                    d.put(&3u8);
                    d.put(t.as_str());
                    bytes = bytes.saturating_add(t.len());
                }
                Node::Pi { target, data } => {
                    d.put(&4u8);
                    d.put(target.as_str());
                    d.put(data.as_str());
                    bytes = bytes.saturating_add(target.len()).saturating_add(data.len());
                }
            }
        }
        work.hashed_bytes = work.hashed_bytes.saturating_add(bytes as u64);
        if let Some(slot) = digest.get_mut(i) {
            *slot = d.finish();
        }
        if let Some(slot) = size.get_mut(i) {
            *slot = total;
        }
    }
    let (labels, attr_labels, attr_start) = label_tree(root, work);
    Flat { els, digest, size, labels, attr_labels, attr_start }
}

impl<'t> Flat<'t> {
    fn el(&self, i: usize) -> Option<&'t El> {
        self.els.get(i).copied()
    }

    fn digest(&self, i: usize) -> u128 {
        self.digest.get(i).copied().unwrap_or_default()
    }

    /// The label of the element at `i`.
    fn label(&self, i: usize) -> &'static str {
        self.labels.get(i).copied().unwrap_or(vocab::UNLISTED)
    }

    /// The label of the `j`-th attribute of the element at `i`.
    fn attr_label(&self, i: usize, j: usize) -> &'static str {
        self.attr_start.get(i).and_then(|start| start.checked_add(j)).and_then(|at| self.attr_labels.get(at)).copied().unwrap_or(vocab::UNLISTED)
    }

    /// The indexes of the child elements of the element at `i`, in order.
    fn children(&self, i: usize) -> Vec<usize> {
        let Some(el) = self.el(i) else { return Vec::new() };
        let mut out = Vec::new();
        let mut child = i + 1;
        for node in &el.children {
            if let Node::El(_) = node {
                out.push(child);
                child = child.saturating_add(self.size.get(child).copied().unwrap_or(1));
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the paths

/// No parent.
const NONE: usize = usize::MAX;

/// One step of a path: the element's name, its place among its parent's children of that name, whether that place is written, and the
/// step above it.
struct Seg<'t> {
    parent: usize,
    name: &'t str,
    /// The element's label (see the module documentation).
    label: &'static str,
    ordinal: usize,
    indexed: bool,
}

/// `name` cut to at most `most` bytes at a character's end, with `…` when it was cut.
fn cut(name: &str, most: usize) -> String {
    if name.len() <= most {
        return name.to_owned();
    }
    // The end of the last character that ends within `most` bytes: at most `most` bytes are looked at.
    let end = name.char_indices().map(|(at, c)| at + c.len_utf8()).take_while(|&end| end <= most).last().unwrap_or(0);
    format!("{}…", name.get(..end).unwrap_or_default())
}

/// The path of the step at `at`, from the root, and its label, each cut at [`MAX_PATH_BYTES`].
fn path(segs: &[Seg<'_>], at: usize, work: &mut DiffWork) -> ElementPath {
    let mut chain = Vec::new();
    let mut cur = at;
    // Each step's parent was made before it, so the walk up ends at the root (whose parent is no step); it is bounded by the steps there
    // are, too, so that no mistake can make it go round.
    for _ in 0..segs.len() {
        let Some(seg) = segs.get(cur) else { break };
        chain.push(seg);
        cur = seg.parent;
    }
    chain.reverse();
    let place = |seg: &Seg<'_>| seg.indexed.then_some(seg.ordinal);
    let written = joined(chain.iter().map(|seg| (cut(seg.name, MAX_PATH_NAME_BYTES), place(seg))));
    let label = joined(chain.iter().map(|seg| (seg.label.to_owned(), place(seg))));
    work.path_bytes = work.path_bytes.saturating_add(written.len() as u64).saturating_add(label.len() as u64);
    ElementPath { path: written, label }
}

/// Steps joined into a path, each `/name` with its place where its name is shared, cut at [`MAX_PATH_BYTES`] with `/…`.
fn joined(steps: impl Iterator<Item = (String, Option<usize>)>) -> String {
    let mut out = String::new();
    for (name, place) in steps {
        let mut piece = format!("/{name}");
        if let Some(place) = place {
            let _ = write!(piece, "[{place}]"); // Writing to a String does not fail.
        }
        if out.len().saturating_add(piece.len()) > MAX_PATH_BYTES {
            out.push_str("/…");
            break;
        }
        out.push_str(&piece);
    }
    out
}

/// The place of each child among its siblings of its name, from 1, and whether its name is shared (so that the place is written).
fn places<'t>(flat: &Flat<'t>, children: &[usize]) -> Vec<(usize, bool)> {
    let mut totals: HashMap<&str, usize> = HashMap::new();
    for &c in children {
        if let Some(el) = flat.el(c) {
            *totals.entry(el.name.as_str()).or_default() += 1;
        }
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    children
        .iter()
        .map(|&c| {
            let name = flat.el(c).map_or("", |el| el.name.as_str());
            let place = seen.entry(name).or_default();
            *place += 1;
            (*place, totals.get(name).copied().unwrap_or(0) > 1)
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------------------------------ the walk

/// One side of the walk: its tree and its path steps.
struct Side<'f, 't> {
    flat: &'f Flat<'t>,
    segs: Vec<Seg<'t>>,
}

impl<'t> Side<'_, 't> {
    /// A step for the child at `index` (whose place is `place`) under the step `parent`.
    fn step(&mut self, parent: usize, index: usize, place: (usize, bool)) -> usize {
        let name = self.flat.el(index).map_or("", |el| el.name.as_str());
        self.segs.push(Seg { parent, name, label: self.flat.label(index), ordinal: place.0, indexed: place.1 });
        self.segs.len() - 1
    }
}

/// What the walk lists, within the comparison's budget.
struct Report<'b> {
    part: PartDiff,
    budget: &'b mut usize,
}

impl Report<'_> {
    /// Whether one more entry may be listed; takes it from the budget when so.
    fn take(&mut self) -> bool {
        if *self.budget == 0 {
            return false;
        }
        *self.budget -= 1;
        true
    }
}

/// Compares two trees of the part `name`.
fn compare_trees(name: &str, a: &El, b: &El, budget: &mut usize, work: &mut DiffWork) -> PartDiff {
    let (fa, fb) = (flatten(a, work), flatten(b, work));
    let mut report = Report { part: PartDiff { part: name.to_owned(), ..PartDiff::default() }, budget };
    let mut left = Side { flat: &fa, segs: Vec::new() };
    let mut right = Side { flat: &fb, segs: Vec::new() };
    left.segs.push(Seg { parent: NONE, name: a.name.as_str(), label: fa.label(0), ordinal: 1, indexed: false });
    right.segs.push(Seg { parent: NONE, name: b.name.as_str(), label: fb.label(0), ordinal: 1, indexed: false });
    if fa.digest(0) == fb.digest(0) {
        return report.part;
    }
    if a.name != b.name {
        report.part.counts.removed += 1;
        report.part.counts.added += 1;
        if report.take() {
            report.part.removed.push(path(&left.segs, 0, work));
        }
        if report.take() {
            report.part.added.push(path(&right.segs, 0, work));
        }
        return report.part;
    }
    // (index in the first tree, index in the second, step in the first, step in the second)
    let mut stack: Vec<(usize, usize, usize, usize)> = vec![(0, 0, 0, 0)];
    // Each element of the first tree is in one pair at most, so the walk ends after that many; the bound is there so that no mistake can make
    // it go round.
    let mut left_to_walk = fa.els.len();
    while let Some((ia, ib, sa, sb)) = stack.pop() {
        let Some(left_now) = left_to_walk.checked_sub(1) else { break };
        left_to_walk = left_now;
        work.pairs = work.pairs.saturating_add(1);
        let mut pushed = walk_pair((ia, ib, sa, sb), &mut left, &mut right, &mut report, work);
        // Children in reverse, so that they are walked, and listed, in document order.
        pushed.reverse();
        stack.extend(pushed);
    }
    report.part
}

/// One pair of the walk: lists what differs of its own, and gives the pairs of children to walk.
fn walk_pair(
    (ia, ib, sa, sb): (usize, usize, usize, usize),
    left: &mut Side<'_, '_>,
    right: &mut Side<'_, '_>,
    report: &mut Report<'_>,
    work: &mut DiffWork,
) -> Vec<(usize, usize, usize, usize)> {
    let (fa, fb) = (left.flat, right.flat);
    let (Some(ea), Some(eb)) = (fa.el(ia), fb.el(ib)) else { return Vec::new() };
    let (ca, cb) = (fa.children(ia), fb.children(ib));
    work.children = work.children.saturating_add((ca.len() + cb.len()) as u64);
    let (pa, pb) = (places(fa, &ca), places(fb, &cb));
    let mut listed_something = false;
    // The equal runs at the front and at the back.
    let (na, nb) = (ca.len(), cb.len());
    let same = |(x, y): &(&usize, &usize)| fa.digest(**x) == fb.digest(**y);
    // The run at the front ends where either side does; the run at the back may not reach into it.
    let front = ca.iter().zip(&cb).take_while(same).count();
    let back = ca.iter().rev().zip(cb.iter().rev()).take(na.min(nb) - front).take_while(same).count();
    let (ma, mb) = (front..na - back, front..nb - back);
    // Children equal to one on the other side, matched in order by digest.
    let mut by_digest: HashMap<u128, VecDeque<usize>> = HashMap::new();
    for y in mb.clone() {
        if let Some(&c) = cb.get(y) {
            by_digest.entry(fb.digest(c)).or_default().push_back(y);
        }
    }
    let mut partner_a: Vec<Option<usize>> = vec![None; na];
    let mut taken_b: Vec<bool> = vec![false; nb];
    let mut matched: Vec<(usize, usize)> = Vec::new();
    for x in ma.clone() {
        let Some(&c) = ca.get(x) else { continue };
        if let Some(y) = by_digest.get_mut(&fa.digest(c)).and_then(VecDeque::pop_front) {
            matched.push((x, y));
            if let Some(slot) = partner_a.get_mut(x) {
                *slot = Some(y);
            }
            if let Some(slot) = taken_b.get_mut(y) {
                *slot = true;
            }
        }
    }
    // Those outside the longest run that keeps its order moved.
    let kept = longest_ordered(&matched, work);
    for (k, &(x, y)) in matched.iter().enumerate() {
        if kept.get(k).copied().unwrap_or(false) {
            continue;
        }
        listed_something = true;
        report.part.counts.moved += 1;
        if report.take() {
            let (Some(&c), Some(&d)) = (ca.get(x), cb.get(y)) else { continue };
            let from = left.step(sa, c, pa.get(x).copied().unwrap_or((1, false)));
            let to = right.step(sb, d, pb.get(y).copied().unwrap_or((1, false)));
            let moved = Moved { from: path(&left.segs, from, work), to: path(&right.segs, to, work) };
            report.part.moved.push(moved);
        }
    }
    // The rest matched by name, in order.
    let mut by_name: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for y in mb.clone() {
        if taken_b.get(y).copied().unwrap_or(true) {
            continue;
        }
        if let Some(el) = cb.get(y).and_then(|&c| fb.el(c)) {
            by_name.entry(el.name.as_str()).or_default().push_back(y);
        }
    }
    let mut pushed = Vec::new();
    for x in ma.clone() {
        if partner_a.get(x).copied().flatten().is_some() {
            continue;
        }
        let Some(&c) = ca.get(x) else { continue };
        let name = fa.el(c).map_or("", |el| el.name.as_str());
        listed_something = true;
        match by_name.get_mut(name).and_then(VecDeque::pop_front) {
            Some(y) => {
                if let Some(slot) = taken_b.get_mut(y) {
                    *slot = true;
                }
                let Some(&d) = cb.get(y) else { continue };
                let from = left.step(sa, c, pa.get(x).copied().unwrap_or((1, false)));
                let to = right.step(sb, d, pb.get(y).copied().unwrap_or((1, false)));
                pushed.push((c, d, from, to));
            }
            None => {
                report.part.counts.removed += 1;
                if report.take() {
                    let at = left.step(sa, c, pa.get(x).copied().unwrap_or((1, false)));
                    report.part.removed.push(path(&left.segs, at, work));
                }
            }
        }
    }
    for y in mb {
        if taken_b.get(y).copied().unwrap_or(true) {
            continue;
        }
        let Some(&d) = cb.get(y) else { continue };
        listed_something = true;
        report.part.counts.added += 1;
        if report.take() {
            let at = right.step(sb, d, pb.get(y).copied().unwrap_or((1, false)));
            report.part.added.push(path(&right.segs, at, work));
        }
    }
    // The pair's own attributes and content.
    let (attributes, attribute_labels, attribute_count, reordered) = attribute_changes((fa, ia), (fb, ib));
    let own = content(ea) != content(eb);
    let blanks = blanks(ea) != blanks(eb);
    if attribute_count > 0 || reordered || own {
        report.part.counts.changed += 1;
        if report.take() {
            let ElementPath { path, label } = path(&left.segs, sa, work);
            let change = ElementChange { path, label, attributes, attribute_labels, attribute_count, reordered, text: own };
            report.part.changed.push(change);
        }
    } else if blanks || !listed_something {
        // Nothing else of this pair differs, so its digests differ by its white space, or by how its content and children interleave.
        let interleaved = !blanks && interleaving(ea) != interleaving(eb);
        if interleaved {
            report.part.counts.changed += 1;
            if report.take() {
                let ElementPath { path, label } = path(&left.segs, sa, work);
                let change = ElementChange {
                    path,
                    label,
                    attributes: Vec::new(),
                    attribute_labels: Vec::new(),
                    attribute_count: 0,
                    reordered: false,
                    text: true,
                };
                report.part.changed.push(change);
            }
        } else {
            report.part.counts.whitespace += 1;
            if report.take() {
                report.part.whitespace.push(path(&left.segs, sa, work));
            }
        }
    }
    pushed
}

/// Which of `matched` (pairs of places, in the first side's order) belong to one longest run whose places on the second side rise: patience
/// sorting, `k log k` for `k` pairs, each step counted.
fn longest_ordered(matched: &[(usize, usize)], work: &mut DiffWork) -> Vec<bool> {
    // tails[l]: the index in `matched` of the smallest last second-side place of a rising run of length l + 1; back[k]: the one before k.
    let mut tails: Vec<usize> = Vec::new();
    let mut back: Vec<usize> = vec![NONE; matched.len()];
    for (k, &(_, y)) in matched.iter().enumerate() {
        work.ordered_steps = work.ordered_steps.saturating_add(1);
        let at = tails.partition_point(|&t| {
            work.ordered_steps = work.ordered_steps.saturating_add(1);
            matched.get(t).is_some_and(|&(_, ty)| ty < y)
        });
        if at > 0
            && let (Some(slot), Some(&before)) = (back.get_mut(k), tails.get(at - 1))
        {
            *slot = before;
        }
        if at == tails.len() {
            tails.push(k);
        } else if let Some(slot) = tails.get_mut(at) {
            *slot = k;
        }
    }
    let mut kept = vec![false; matched.len()];
    let mut cur = tails.last().copied().unwrap_or(NONE);
    // Back from the run's last pair to its first: each step goes to an earlier pair, so there are at most as many steps as pairs, and the
    // bound is there so that no mistake can make it go round.
    for _ in 0..matched.len() {
        let Some(slot) = kept.get_mut(cur) else { break };
        *slot = true;
        cur = back.get(cur).copied().unwrap_or(NONE);
    }
    kept
}

/// The attributes of the elements at `ia` of `fa` and `ib` of `fb` that differ, by name as written (at most [`MOST_ATTRIBUTES`] named) and by
/// label, how many, and whether the two hold the same attributes in another order.
fn attribute_changes((fa, ia): (&Flat<'_>, usize), (fb, ib): (&Flat<'_>, usize)) -> (Vec<String>, Vec<&'static str>, usize, bool) {
    let (Some(a), Some(b)) = (fa.el(ia), fb.el(ib)) else { return (Vec::new(), Vec::new(), 0, false) };
    if a.attrs == b.attrs {
        return (Vec::new(), Vec::new(), 0, false);
    }
    let in_b: HashMap<&str, &str> = b.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let in_a: HashMap<&str, &str> = a.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (mut names, mut labels) = (Vec::new(), Vec::new());
    let mut count = 0usize;
    let mut note = |name: &str, label: &'static str| {
        count += 1;
        if names.len() < MOST_ATTRIBUTES {
            names.push(cut(name, MAX_PATH_NAME_BYTES));
            labels.push(label);
        }
    };
    for (j, (k, v)) in a.attrs.iter().enumerate() {
        if in_b.get(k.as_str()) != Some(&v.as_str()) {
            note(k, fa.attr_label(ia, j));
        }
    }
    for (j, (k, _)) in b.attrs.iter().enumerate() {
        if !in_a.contains_key(k.as_str()) {
            note(k, fb.attr_label(ib, j));
        }
    }
    let reordered = count == 0;
    (names, labels, count, reordered)
}

/// A piece of an element's own content.
#[derive(PartialEq, Eq)]
enum Piece<'t> {
    Text(&'t str),
    CData(&'t str),
    Comment(&'t str),
    Pi(&'t str, &'t str),
}

/// The piece a child is, unless it is an element or white-space-only text.
fn piece(node: &Node) -> Option<Piece<'_>> {
    match node {
        Node::El(_) => None,
        Node::Text(t) if is_blank(t) => None,
        Node::Text(t) => Some(Piece::Text(t)),
        Node::CData(t) => Some(Piece::CData(t)),
        Node::Comment(t) => Some(Piece::Comment(t)),
        Node::Pi { target, data } => Some(Piece::Pi(target, data)),
    }
}

/// An element's own content, white-space-only text left out, in order.
fn content(el: &El) -> Vec<Piece<'_>> {
    el.children.iter().filter_map(piece).collect()
}

/// An element's white-space-only text, in order.
fn blanks(el: &El) -> Vec<&str> {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::Text(t) if is_blank(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// An element's own content with, for each piece, how many child elements come before it.
fn interleaving(el: &El) -> Vec<(usize, Piece<'_>)> {
    let mut before = 0usize;
    let mut out = Vec::new();
    for node in &el.children {
        match node {
            Node::El(_) => before += 1,
            other => {
                if let Some(p) = piece(other) {
                    out.push((before, p));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_digest_holds_both_of_its_halves() {
        // The two SipHash states, started differently and fed the same, are the high and the low 64 bits: a digest is as strong as both.
        let mut d = Digest::new();
        d.put("draw:custom-shape");
        let (high, low) = (u128::from(d.0.finish()), u128::from(d.1.finish()));
        assert_ne!(high, low, "the two states were started differently");
        assert_eq!(d.finish() >> 64, high);
        assert_eq!(d.finish() & u128::from(u64::MAX), low);
    }

    #[test]
    fn a_name_is_cut_at_the_end_of_a_character() {
        // Three-byte characters after a three-byte prefix: byte 128 falls inside one, so the cut is at 126.
        let name = format!("xy:{}", "\u{3042}".repeat(50));
        let cut_name = cut(&name, 128);
        assert_eq!(cut_name.len(), 126 + '…'.len_utf8());
        assert!(cut_name.ends_with('…') && name.starts_with(cut_name.trim_end_matches('…')));
        // After a one-byte prefix the characters end at 4, 7, ... 127: the cut is at 127.
        let odd = format!("x{}", "\u{3042}".repeat(50));
        assert_eq!(cut(&odd, 128).len(), 127 + '…'.len_utf8());
        assert_eq!(cut("short", 128), "short");
        assert_eq!(cut(&"a".repeat(129), 128), format!("{}…", "a".repeat(128)));
    }
}
