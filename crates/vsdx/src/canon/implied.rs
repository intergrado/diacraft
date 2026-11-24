//! What a package says twice, taken out before the content check compares (2026-11-21; classes F3 and F4 of
//!
//! The check's lists ([`super::CANON_RULES`]) say what is the same inside one part. Two kinds of row say a thing that other parts say already, and whether a
//! row is implied is not a question the one part can answer: it needs the parts the row is about. So this reads the two packages first, and **takes out of one
//! package's copy of a stream the rows that its own parts imply and the other package lacks**; the comparison then sees the rest. Nothing is added, nothing but rows
//! (and one value, below) is changed, and a row that is not shown to be implied stays and is compared as ever, so a doubt leaves a difference and never hides one.
//! The reading never refuses: a stream or a part that does not parse, is too large for the limits, or does not say what the rule needs is left as it is.
//!
//! # F4: what a content type says, per part (`[Content_Types].xml`)
//!
//! A part's content type is its `Override`, else the `Default` of its extension (ECMA-376 Part 2, 10.1.2.2). Each part both packages hold
//! is looked at on both sides, by the rows that give it its type, and these are no difference (the lookup is [`diacraft_opc::Package::content_type`]'s: a row that does
//! not give the same answer is not read):
//! * **A part that had none and is given one** (an untyped part is not legal OPC: has the writer give it one): the package that has the
//!   type has a row that the other lacks. The row is taken out: an `Override` for the part, or a `Default` for its extension **when no `Default` for that extension
//!   is in the other package and every part (both packages hold) it gives a type to has none in the other**. The type given is not judged.
//! * **A property part typed as the drawing and given its own type** (eight files of one repository): a part that a package relationship of
//!   `extended-properties`, `custom-properties` or `core-properties` reaches, whose `Override` says one of the six types of the drawing's document in the one package and
//!   the type of its kind in the other. The type of the drawing's is set to the other's, so that the two rows are alike. A part no such relationship reaches, a type of
//!   another kind and any other type are a difference.
//! * **An `Override` for a relationships part and the `Default` for `rels`** (a relationships part needs no row of its own from the 4th
//!   edition of ECMA-376 on): the part has the relationships type in both, by an `Override` in the one and the `Default` in the other. The `Override` is taken out,
//!   and the `Default` when the other package has none for `rels` and every part it gives a type to is so typed in the other.
//!
//! What stays a difference: the same part typed by an `Override` in one package and a `Default` in the other (the concern 1: the model does not record the
//! choice), two different types, a row for no part (a part only one package has included), a `Default` that gives a type to no part both hold.
//!
//! # F3: the relationship rows of a page that imply
//!
//! A page's relationships part holds a `master` row to each master its shapes use (a writer rebuilds them from the shapes'
//! `Master`) and a `page` row to its background page, none of them named by an `r:id`. For each relationships part of a page that
//! the two packages hold differently, a row of one package is taken out when **the page's own package implies it** and the other package has **no row of the
//! same type to the same part, and none of the same `Id`**:
//! * a `master` row (the type `.../relationships/master` of the format's own namespace, exactly), internal, to the part of a master that a `Shape` of the page's
//!   part names by its `Master` (shapes inside groups too, found through `Shapes` and `Shape` and no other element), the master's part being the one its row of the
//!   masters index leads to;
//! * a `page` row, internal, to the part of the page named by the `BackPage` of the page's own row of the pages index.
//!
//! The page is a row of the pages index that the document's relationship `pages` reaches, and the index and the masters index are read as the reader reads them
//! (an `ID` as a number, the `r:id` in the relationships namespace, a relationship of the type of the row's kind). A row to a master no shape uses, a row to a part that
//! is not the background page's, a `page` row where the page says no `BackPage` (R6 keeps such a row and does not know why), an external row, another type, another
//! `Id` for a part the other package has a row for: all of them stay, and are compared.
//!
//! # What it costs
//!
//! The content types: one parse of each stream and a pass over the parts and the rows, `O(parts + rows)`. The relationships: only the relationships parts the two
//! packages hold differently, each of at most 1 MiB, are parsed (twice at most: the part and its twin), and a page's part is parsed once, only when one of its rows
//! may be implied. Every parse is counted in the work and bounded by the same limits as the comparison's.

use std::collections::{HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{El, Node, XmlLimits, parse_counted, to_bytes};
use diacraft_opc::Package;
use diacraft_opc::name::{fold, is_rels_part, lenient_decode, rels_source, resolve};

use crate::ns::{Scope, ScopeExt};
use crate::passthrough::read_u32;
use crate::props::{CT_APP, CT_CORE, CT_CUSTOM};
use crate::report::Variant;

const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines
const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const PK: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/";
/// The relationships media type: what the `Default` for `rels` gives (ECMA-376 Part 2, 10.1.2.2).
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
/// The largest relationships part this reads: a page's holds a handful of rows, and one larger is compared as it is.
const MAX_RELS_BYTES: usize = 1 << 20;

/// What was taken out of one package's copy of what the check compares: bytes in place of a stream's or a part's own.
#[derive(Default)]
pub(super) struct Adjusted {
    /// `[Content_Types].xml`, when rows were taken out of it or a value set.
    pub content_types: Option<Vec<u8>>,
    /// Parts by their folded names, whose bytes are these in place of the package's own.
    pub parts: HashMap<String, Vec<u8>>,
}

/// What is taken out of each package's copy before `a` and `b` are compared. Never refuses: what it cannot read it leaves as it is.
pub(super) fn adjust(a: &Package, b: &Package, limits: &XmlLimits, work: &mut Work) -> (Adjusted, Adjusted) {
    let mut out = (Adjusted::default(), Adjusted::default());
    content_types(a, b, limits, work, &mut out);
    relationships(a, b, limits, work, &mut out.0);
    relationships(b, a, limits, work, &mut out.1);
    out
}

fn count(work: &mut Work, steps: usize) {
    work.canon_steps = work.canon_steps.saturating_add(u64::try_from(steps).unwrap_or(u64::MAX));
}

// ------------------------------------------------------------------------------------------------------------------- F4: the content types

/// A row of a content types stream that gives a type: its place among the children of the root, what it names, and the type.
struct TypeRow {
    child: usize,
    is_default: bool,
    /// The extension as OPC's reader folds it (lower case), or the part name as it folds it.
    key: String,
    content_type: String,
}

/// A content types stream as a tree, with its rows found the way OPC's reader finds them: the first row of a key wins.
struct Stream {
    root: El,
    rows: Vec<TypeRow>,
    defaults: HashMap<String, usize>,
    overrides: HashMap<String, usize>,
}

impl Stream {
    fn read(package: &Package, limits: &XmlLimits, work: &mut Work) -> Option<Stream> {
        let root = parse_counted(package.content_types_bytes(), limits, work).ok()?;
        if !root.is("Types") {
            return None;
        }
        let (mut rows, mut defaults, mut overrides) = (Vec::new(), HashMap::new(), HashMap::new());
        for (child, node) in root.children.iter().enumerate() {
            let Node::El(row) = node else { continue };
            let is_default = row.is("Default");
            if !is_default && !row.is("Override") {
                continue;
            }
            let (key_attr, map) = if is_default { ("Extension", &mut defaults) } else { ("PartName", &mut overrides) };
            let (Some(key), Some(content_type)) = (row.attr(key_attr), row.attr("ContentType")) else { continue };
            let key = if is_default { key.to_ascii_lowercase() } else { fold(&lenient_decode(key)) };
            map.entry(key.clone()).or_insert(rows.len());
            rows.push(TypeRow { child, is_default, key, content_type: content_type.to_owned() });
        }
        count(work, rows.len());
        Some(Stream { root, rows, defaults, overrides })
    }

    /// The row that gives the part its type, as OPC's lookup finds it.
    fn serving(&self, part: &str) -> Option<usize> {
        let key = fold(part);
        if let Some(&row) = self.overrides.get(&key) {
            return Some(row);
        }
        let (_, extension) = key.rsplit('/').next()?.rsplit_once('.')?;
        self.defaults.get(extension).copied()
    }
}

/// The type of its kind that a package relationship gives a part, for each part that one reaches: `None` for a part two relationships give two kinds.
fn property_kinds(packages: [&Package; 2]) -> HashMap<String, Option<&'static str>> {
    let mut kinds: HashMap<String, Option<&'static str>> = HashMap::new();
    let by_type =
        [(format!("{OD}extended-properties"), CT_APP), (format!("{OD}custom-properties"), CT_CUSTOM), (format!("{PK}core-properties"), CT_CORE)];
    for package in packages {
        let Ok(rels) = package.rels("/") else { continue };
        for rel in &rels.items {
            let Some(&(_, own)) = by_type.iter().find(|(ty, _)| *ty == rel.ty) else { continue };
            let Some(target) = rel.target.as_deref() else { continue };
            kinds.entry(fold(target)).and_modify(|k| *k = k.filter(|k| *k == own)).or_insert(Some(own));
        }
    }
    kinds
}

/// What a row gives the parts both packages hold: how many it gives a type to, and how many of them are no difference by the rules above.
#[derive(Clone, Copy, Default)]
struct Serves {
    parts: u32,
    explained: u32,
}

fn content_types(a: &Package, b: &Package, limits: &XmlLimits, work: &mut Work, out: &mut (Adjusted, Adjusted)) {
    if a.content_types_bytes() == b.content_types_bytes() {
        return;
    }
    let (Some(mut sa), Some(mut sb)) = (Stream::read(a, limits, work), Stream::read(b, limits, work)) else { return };
    let theirs: HashMap<String, &str> = b.parts().map(|name| (fold(name), name)).collect();
    let kinds = property_kinds([a, b]);
    let mut serves = (vec![Serves::default(); sa.rows.len()], vec![Serves::default(); sb.rows.len()]);
    // The value of a row that is set to the other's: the property parts typed as the drawing.
    let mut set: (HashMap<usize, &'static str>, HashMap<usize, &'static str>) = (HashMap::new(), HashMap::new());
    for name_a in a.parts() {
        let key = fold(name_a);
        let Some(&name_b) = theirs.get(&key) else { continue };
        count(work, 1);
        let (ra, rb) = (sa.serving(name_a), sb.serving(name_b));
        let (ta, tb) =
            (ra.and_then(|r| sa.rows.get(r)).map(|r| r.content_type.as_str()), rb.and_then(|r| sb.rows.get(r)).map(|r| r.content_type.as_str()));
        // A row that the package's own lookup does not read this way is not read at all: the part is compared as it is.
        if a.content_type(name_a) != ta || b.content_type(name_b) != tb {
            continue;
        }
        if let Some(r) = ra.and_then(|r| serves.0.get_mut(r)) {
            r.parts += 1;
        }
        if let Some(r) = rb.and_then(|r| serves.1.get_mut(r)) {
            r.parts += 1;
        }
        let explain = |side: &mut [Serves], row: Option<usize>| {
            if let Some(r) = row.and_then(|r| side.get_mut(r)) {
                r.explained += 1;
            }
        };
        match (ta, tb) {
            // A part that has a type in one package and none in the other: the type is given.
            (Some(_), None) => explain(&mut serves.0, ra),
            (None, Some(_)) => explain(&mut serves.1, rb),
            (Some(x), Some(y)) if x == y => {
                // The relationships type by an `Override` in one package and by the `Default` in the other.
                let (kind_a, kind_b) = (ra.and_then(|r| sa.rows.get(r)).map(|r| r.is_default), rb.and_then(|r| sb.rows.get(r)).map(|r| r.is_default));
                if x == CT_RELS && kind_a != kind_b && is_rels_part(name_a) {
                    explain(&mut serves.0, ra);
                    explain(&mut serves.1, rb);
                }
            }
            (Some(x), Some(y)) => {
                // A property part typed as the drawing in one package and as its kind in the other, both by an `Override` of the part.
                let overridden = |s: &Stream, r: usize| s.rows.get(r).is_some_and(|r| !r.is_default);
                if let (Some(own), Some(ra), Some(rb)) = (kinds.get(&key).copied().flatten(), ra, rb)
                    && overridden(&sa, ra)
                    && overridden(&sb, rb)
                {
                    if Variant::from_content_type(x).is_some() && y == own {
                        set.0.insert(ra, own);
                    } else if Variant::from_content_type(y).is_some() && x == own {
                        set.1.insert(rb, own);
                    }
                }
            }
            (None, None) => {}
        }
    }
    out.0.content_types = taken_out(&mut sa, &serves.0, &set.0, &sb, work);
    out.1.content_types = taken_out(&mut sb, &serves.1, &set.1, &sa, work);
}

/// The stream with what is explained taken out, and the values set, or `None` when nothing is: an `Override` whose part is explained, and a `Default` that the other
/// stream has none of the extension for and that gives a type to parts that are all explained (and at least one).
fn taken_out(stream: &mut Stream, serves: &[Serves], set: &HashMap<usize, &'static str>, other: &Stream, work: &mut Work) -> Option<Vec<u8>> {
    let mut gone: HashSet<usize> = HashSet::new();
    for (at, row) in stream.rows.iter().enumerate() {
        let served = serves.get(at).copied().unwrap_or_default();
        let explained = served.parts > 0 && served.explained == served.parts;
        if explained && (!row.is_default || !other.defaults.contains_key(&row.key)) {
            gone.insert(row.child);
        }
    }
    if gone.is_empty() && set.is_empty() {
        return None;
    }
    for (&at, own) in set {
        let Some(row) = stream.rows.get(at) else { continue };
        let Some(Node::El(el)) = stream.root.children.get_mut(row.child) else { continue };
        if let Some((_, value)) = el.attrs.iter_mut().find(|(k, _)| k == "ContentType") {
            *value = (*own).to_owned();
        }
    }
    let mut index = 0usize;
    stream.root.children.retain(|_| {
        index += 1;
        !gone.contains(&(index - 1))
    });
    count(work, index);
    to_bytes(&stream.root, &[], &[], false).ok()
}

// ----------------------------------------------------------------------------------------------------------- F3: the rows of a page's relationships

/// A relationship row: its place among the children of the root, and what it says. `target` is the part it leads to, folded, for an internal one.
struct RelRow {
    child: usize,
    id: String,
    ty: String,
    target: Option<String>,
}

/// The rows of a relationships part whose source is `source`, in order: the rows that have an `Id`, a `Type` and a `Target`.
fn rel_rows(root: &El, source: &str) -> Vec<RelRow> {
    let mut rows = Vec::new();
    for (child, node) in root.children.iter().enumerate() {
        let Node::El(row) = node else { continue };
        if !row.is("Relationship") {
            continue;
        }
        let (Some(id), Some(ty), Some(target)) = (row.attr("Id"), row.attr("Type"), row.attr("Target")) else { continue };
        let external = row.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
        let target = if external { None } else { resolve(source, target).map(|t| fold(&t)) };
        rows.push(RelRow { child, id: id.to_owned(), ty: ty.to_owned(), target });
    }
    rows
}

/// Which index part is read.
#[derive(Clone, Copy)]
enum Rows {
    Pages,
    Masters,
}

/// What the pages index and the masters index of a package say: which part each page and each master is, and each page's background page.
struct Index {
    /// A page's part (folded) to the `ID` of its background page.
    back_of: HashMap<String, Option<u32>>,
    /// A page's `ID` to its part (folded), the first of an `ID`.
    page_part: HashMap<u32, String>,
    /// A master's `ID` to its part (folded), the first of an `ID`.
    master_part: HashMap<u32, String>,
}

impl Index {
    fn read(package: &Package, limits: &XmlLimits, work: &mut Work) -> Option<Index> {
        let target_of = |rels: &diacraft_opc::Rels, word: &str| -> Option<String> {
            let ty = format!("{VIS}{word}");
            rels.of_type(&ty).find_map(|rel| rel.target.clone())
        };
        let document = target_of(&package.rels("/").ok()?, "document")?;
        let rels = package.rels(&document).ok()?;
        let mut index = Index { back_of: HashMap::new(), page_part: HashMap::new(), master_part: HashMap::new() };
        if let Some(pages) = target_of(&rels, "pages") {
            index.read_rows(package, &pages, Rows::Pages, limits, work)?;
        }
        if let Some(masters) = target_of(&rels, "masters") {
            index.read_rows(package, &masters, Rows::Masters, limits, work)?;
        }
        Some(index)
    }

    /// The rows of an index part `part`: each with an `ID` and a `Rel` that a relationship of the row's type answers.
    fn read_rows(&mut self, package: &Package, part: &str, which: Rows, limits: &XmlLimits, work: &mut Work) -> Option<()> {
        let (root, row, word) = match which {
            Rows::Pages => ("Pages", "Page", "page"),
            Rows::Masters => ("Masters", "Master", "master"),
        };
        let tree = parse_counted(package.part(part)?, limits, work).ok()?;
        let scope = Scope::empty().enter(&tree);
        if !scope.is(&tree, root) {
            return None;
        }
        let rels = package.rels(part).ok()?;
        let ty = format!("{VIS}{word}");
        for child in tree.elements() {
            count(work, 1);
            let sc = scope.enter(child);
            if !sc.is(child, row) {
                continue;
            }
            let Some(id) = child.attr("ID").and_then(|v| read_u32(v).0) else { continue };
            let back = child.attr("BackPage").and_then(|v| read_u32(v).0);
            let rel_id = child.elements().find_map(|g| {
                let gsc = sc.enter(g);
                if gsc.is(g, "Rel") { gsc.rel_id(g) } else { None }
            });
            let target = rel_id.and_then(|rid| rels.get(rid)).filter(|rel| rel.ty == ty).and_then(|rel| rel.target.as_deref()).map(fold);
            let Some(target) = target else {
                // A row with no part: a page with no shapes. It still has its background page.
                continue;
            };
            match which {
                Rows::Pages => {
                    self.page_part.entry(id).or_insert_with(|| target.clone());
                    self.back_of.entry(target).or_insert(back);
                }
                Rows::Masters => {
                    self.master_part.entry(id).or_insert(target);
                }
            }
        }
        Some(())
    }
}

/// The masters the shapes of a page part use, by `ID`: the `Master` of each `Shape` found through `Shapes` and `Shape`, as the reader finds shapes.
fn masters_used(bytes: &[u8], limits: &XmlLimits, work: &mut Work) -> Option<HashSet<u32>> {
    let tree = parse_counted(bytes, limits, work).ok()?;
    let scope = Scope::empty().enter(&tree);
    let mut used = HashSet::new();
    for child in tree.elements() {
        let sc = scope.enter(child);
        if sc.is(child, "Shapes") {
            shapes_used(child, &sc, &mut used, work, 0);
        }
    }
    Some(used)
}

fn shapes_used(shapes: &El, scope: &Scope, used: &mut HashSet<u32>, work: &mut Work, depth: usize) {
    // A tree the parser gave is never deeper than its ceiling: this is the same bound for the recursion, not a limit a file can reach.
    if depth > XML_DEPTH_CEILING {
        return;
    }
    for shape in shapes.elements() {
        count(work, 1);
        let sc = scope.enter(shape);
        if !sc.is(shape, "Shape") {
            continue;
        }
        if let Some(id) = shape.attr("Master").and_then(|v| read_u32(v).0) {
            used.insert(id);
        }
        for inner in shape.elements() {
            let isc = sc.enter(inner);
            if isc.is(inner, "Shapes") {
                shapes_used(inner, &isc, used, work, depth + 1);
            }
        }
    }
}

/// Takes out of `out` the rows of each page's relationships part in `x` that `x` implies and `y` has no row for.
fn relationships(x: &Package, y: &Package, limits: &XmlLimits, work: &mut Work, out: &mut Adjusted) {
    let (master_ty, page_ty) = (format!("{VIS}master"), format!("{VIS}page"));
    // Read when a row may be implied, once.
    let mut index: Option<Option<Index>> = None;
    for name in x.parts() {
        if !is_rels_part(name) {
            continue;
        }
        let Some(source) = rels_source(name).filter(|s| s != "/") else { continue };
        let (Some(mine), theirs) = (x.part(name), y.part(name)) else { continue };
        // The same bytes are the same rows; a page that the other package does not hold is a part it lacks.
        if theirs == Some(mine) || mine.len() > MAX_RELS_BYTES || x.part(&source).is_none() || y.part(&source).is_none() {
            continue;
        }
        count(work, 1);
        let Ok(root) = parse_counted(mine, limits, work) else { continue };
        if !root.is("Relationships") {
            continue;
        }
        let mine_rows = rel_rows(&root, &source);
        let their_rows = match theirs {
            None => Vec::new(),
            Some(t) if t.len() > MAX_RELS_BYTES => continue,
            Some(t) => match parse_counted(t, limits, work) {
                Ok(r) if r.is("Relationships") => rel_rows(&r, &source),
                _ => continue,
            },
        };
        let ids: HashSet<&str> = their_rows.iter().map(|r| r.id.as_str()).collect();
        let same: HashSet<(&str, &str)> = their_rows.iter().filter_map(|r| Some((r.ty.as_str(), r.target.as_deref()?))).collect();
        // The rows the other package has nothing like.
        let candidates: Vec<&RelRow> = mine_rows
            .iter()
            .filter(|r| (r.ty == master_ty || r.ty == page_ty) && !ids.contains(r.id.as_str()))
            .filter(|r| r.target.as_deref().is_some_and(|t| !same.contains(&(r.ty.as_str(), t))))
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let Some(index) = index.get_or_insert_with(|| Index::read(x, limits, work)).as_ref() else { continue };
        let key = fold(&source);
        let Some(&back) = index.back_of.get(&key) else { continue };
        let back_part = back.and_then(|id| index.page_part.get(&id));
        let used = if candidates.iter().any(|r| r.ty == master_ty) {
            let Some(ids) = x.part(&source).and_then(|bytes| masters_used(bytes, limits, work)) else { continue };
            ids.iter().filter_map(|id| index.master_part.get(id)).cloned().collect::<HashSet<String>>()
        } else {
            HashSet::new()
        };
        let implied: HashSet<usize> = candidates
            .iter()
            .filter(|r| {
                let target = r.target.as_deref();
                if r.ty == master_ty { target.is_some_and(|t| used.contains(t)) } else { target.is_some() && target == back_part.map(String::as_str) }
            })
            .map(|r| r.child)
            .collect();
        if implied.is_empty() {
            continue;
        }
        let mut root = root;
        let mut at = 0usize;
        root.children.retain(|_| {
            at += 1;
            !implied.contains(&(at - 1))
        });
        if let Ok(bytes) = to_bytes(&root, &[], &[], false) {
            out.parts.insert(fold(name), bytes);
        }
    }
}
