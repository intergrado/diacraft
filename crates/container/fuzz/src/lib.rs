//! The properties of the fuzz target `canon_compare` (done-criterion D5): the content check on two mutated copies of one package. They are a library so
//! that the container's tests replay the inputs the target crashed on through the very same law (`crates/container/tests/it/canon_fuzz.rs` includes this file).
//!
//! The input is a package and a list of edits. Up to four parts are cut from the front of the input at 0xFF bytes, each part's first byte
//! saying whether it is meant as XML (an even byte) and whether it is named as a relationships part (bit 2), and the rest of it the part's bytes as
//! they are (so the fuzzer explores the XML reader, the namespace scope and every list at once, and a damaged part is the ordinary case); what
//! follows the last 0xFF is read three bytes at a time as edits of a second copy: a byte changed, one put in, one taken out, two parts swapped, a
//! part's name put in another case, a part dropped, one copied. The properties:
//!
//! * the package against itself is the same, and so is it against a copy whose parts all end in a comment (when each parses: the comment is
//!   outside the root, so nothing is added to the content);
//! * **the count is symmetric where nothing orients it**. One rule orients the check here, [`Rules::ignored_when_extra`] (the markup of
//!   [`QUIET`] counts as nothing when only the second package has it, so that `count(a, b)` is not `count(b, a)` for a package that has some; the other, [`Rules::twinned`] of
//!   Is listed by no rule set of this target, which `crates/container/tests/it/canon_twin.rs` holds to its own cases). Without that
//!   rule ([`SYMMETRIC`]) `count(a, b) == count(b, a)` for every pair, when neither is refused. With it ([`RULES`]), a pair where neither package declares
//!   [`QUIET`] anywhere counts exactly what it counts without it, both ways round, refusals included;
//! * **the orientation**: an attribute of [`QUIET`] added to the root of a part that has none of it, and an element of it added beside the
//!   root's children, are no difference when only the second package has them and a difference when only the first does; without the rule they are a
//!   difference either way round;
//! * the only refusal is `xml_too_complex` (a limit), never a panic, a hang, or a count that is not exact: the list shown is at most 200
//!   and no longer than the count;
//! * planted changes: a changed byte of a part that is not XML, an instruction added to a root, and an empty child that a rule lets go only under
//!   another parent, added to a root, are each a difference; a part written back from its tree is the same; and **a dropped part is a difference
//!   unless it is exactly what lets go**, a relationships part by its name whose root is a listed one and holds nothing, which is the
//!   same (the rule once read the root alone, and a dropped part of another name with an empty `Empty` root passed as the same);
//! * **`xmlns=""` where a rule reads a name**: a child that undeclares the default namespace is in none. Planted on the root of a part that parses: a child that
//!   counts as nothing under a parent of `urn:a` is no difference, and the same child with the default namespace undeclared (on it, or on its parent) is one; and an empty
//!   relationships part whose root's child undeclares the default namespace is a part, where the same part with the child of the root's namespace is none;
//! * **a part that breaks a constraint of XML Namespaces 1.0 is compared as bytes**, so the properties that read a part written another way as the same (written
//!   back from its tree, a comment after the root, the orientation) ask it only of a part that keeps them ([`keeps_namespaces`], read from the standard's words).

use diacraft_container::canon::{
    CanonError, ContentDiff, Definition, EmptyPart, EmptyUnder, Entry, FreeNames, IgnoredUnder, KeyFold, Keyed, MAX_EMPTY_PART_BYTES, MAX_SHOWN,
    Name, ODF_TEXT_NAMESPACE, Placed, QKind, Qualified, Reference, Rules, Takes, Unordered, compare_entries,
};
use diacraft_container::xml::{El, Node, parse_with, to_bytes};
use diacraft_container::{Work, XmlLimits};

/// The namespace whose attributes and elements count as nothing when only the second package has them: the one rule that orients the check.
pub const QUIET: &str = "urn:x";

const KEY: &[Name] = &[Name { ns: "", local: "N" }];
/// The rules the target holds the check to: every rule of the container, each with a list that a small input reaches.
pub const RULES: Rules = Rules {
    keyed: &[
        Keyed { child: Name { ns: "", local: "Cell" }, key: KEY, fold: KeyFold::Exact },
        Keyed { child: Name { ns: "urn:a", local: "c" }, key: KEY, fold: KeyFold::Exact },
        Keyed {
            child: Name { ns: "urn:b", local: "Cell" },
            key: &[Name { ns: "urn:a", local: "k" }, Name { ns: "", local: "id" }],
            fold: KeyFold::PartName,
        },
    ],
    numbers: &[Name { ns: "", local: "V" }, Name { ns: "urn:a", local: "len" }],
    lengths: &[Name { ns: "", local: "L" }, Name { ns: "urn:a", local: "len" }],
    qnames: &[
        Qualified { attr: Name { ns: "", local: "k" }, on: None, kind: QKind::Prefixes },
        Qualified { attr: Name { ns: "urn:a", local: "t" }, on: None, kind: QKind::QNames },
        Qualified { attr: Name { ns: "", local: "Requires" }, on: Some(Name { ns: "urn:a", local: "c" }), kind: QKind::Prefixes },
    ],
    paragraphs: &[Name { ns: ODF_TEXT_NAMESPACE, local: "p" }, Name { ns: ODF_TEXT_NAMESPACE, local: "span" }],
    exact_text: &[Name { ns: "", local: "Text" }],
    absent_when_empty: &[Name { ns: "", local: "Empty" }, Name { ns: "urn:a", local: "e" }],
    absent_when_empty_under: &[EmptyUnder { parent: SHAPE, children: &[UNDER_SHAPE, Name { ns: "", local: "Shapes" }] }],
    unordered: &[Unordered { parent: SHAPE, children: &[Name { ns: "", local: "Text" }, Name { ns: "urn:a", local: "s" }] }],
    parts_absent_when_empty: &[
        EmptyPart { named: rels_named, root: EMPTY_ROOT, children: &[] },
        EmptyPart { named: rels_named, root: EMPTY_E, children: &[] },
        // A kind whose root may hold empty children of its namespace, so that the fuzzer reads a child's name through its own declarations there.
        EmptyPart { named: rels_named, root: EMPTY_W, children: &[EMPTY_V] },
    ],
    ignored_attributes: &[],
    // A child that counts as nothing under `Shape`, and names that are free (definitions `Def` of the container `Autos`, told by `fam`, named by `name`; `ref` names one
    // of family `a`; `Embedded` is a scope of its own), so that the fuzzer reaches both rules: the properties below (the count is symmetric, a part against itself is the same, a part
    // written back from its tree is the same) hold under them.
    // And one under a parent of a namespace, so that a child that undeclares the default namespace in force is read where it stands.
    ignored_children: &[
        IgnoredUnder { parent: SHAPE, children: &[Name { ns: "", local: "Gen" }] },
        IgnoredUnder { parent: QUIET_PARENT, children: &[QUIET_CHILD] },
    ],
    // The attributes and elements of `urn:x` count as nothing when only the second part has them. The one rule that orients the check.
    ignored_when_extra: &[QUIET],
    twinned: &[],
    free_names: &[FreeNames {
        container: Name { ns: "", local: "Autos" },
        definitions: &[Definition { element: Name { ns: "", local: "Def" }, family: Some(Name { ns: "", local: "fam" }) }],
        name: Name { ns: "", local: "name" },
        references: &[Reference {
            attr: Name { ns: "", local: "ref" },
            on: None,
            takes: &[Takes { element: Name { ns: "", local: "Def" }, family: "a" }],
            list: false,
        }],
        definition_references: &[],
        scopes: &[Name { ns: "", local: "Embedded" }],
    }],
    placed: &[Placed {
        parent: Name { ns: "", local: "Section" },
        kind: Name { ns: "", local: "N" },
        child: Name { ns: "", local: "Row" },
        place: Name { ns: "", local: "IX" },
        named_by: Name { ns: "", local: "N" },
        always: &["Geometry"],
        never: &["User"],
    }],
    aliases: &[],
    // The attributes that say which thing an element is (`id` of no namespace, and `k` of `urn:a`), so that the fuzzer reaches the alignment of children by their
    // identity, and a part that may begin with a DOCTYPE that holds only an external id, so that it reaches that reading of a part.
    identified_by: &[Name { ns: "", local: "id" }, Name { ns: "urn:a", local: "k" }],
    doctype_parts: &["/p0.xml"],
};

/// [`RULES`] without the one rule that orients the check ([`Rules::ignored_when_extra`]): every other rule of the container, under which the count is symmetric.
pub const SYMMETRIC: Rules = Rules { ignored_when_extra: &[], ..RULES };

const SHAPE: Name = Name { ns: "", local: "Shape" };
/// A child the rules let go when empty only under `Shape`.
const UNDER_SHAPE: Name = Name { ns: "urn:a", local: "u" };
/// The roots of a relationships part that is the same as none when it holds nothing.
const EMPTY_ROOT: Name = Name { ns: "", local: "Empty" };
const EMPTY_E: Name = Name { ns: "urn:a", local: "e" };
/// The root of a relationships part that is the same as none when it holds nothing but empty children [`EMPTY_V`].
const EMPTY_W: Name = Name { ns: "urn:a", local: "w" };
const EMPTY_V: Name = Name { ns: "urn:a", local: "v" };
/// A child that counts as nothing under a parent of a namespace.
const QUIET_PARENT: Name = Name { ns: "urn:a", local: "m" };
const QUIET_CHILD: Name = Name { ns: "urn:a", local: "g" };
/// The URIs of XML Namespaces 1.0 that its constraints reserve.
const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";
/// The prefix the orientation's planted markup binds to [`QUIET`]: planted only in a part that spells it nowhere, so that it binds nothing that was there.
const PLANTED_PREFIX: &str = "zq";

/// A relationships part's name, as OPC gives it: `.rels` in a folder `_rels`, ASCII case folded.
fn rels_named(name: &str) -> bool {
    let mut segments = name.rsplit('/');
    let (file, folder) = (segments.next().unwrap_or(""), segments.next().unwrap_or(""));
    folder.eq_ignore_ascii_case("_rels") && file.len() >= 5 && file.get(file.len() - 5..).is_some_and(|end| end.eq_ignore_ascii_case(".rels"))
}

/// Small limits, so that every limit is within reach of a small input.
fn limits() -> XmlLimits {
    XmlLimits { max_depth: 24, max_nodes: 3000, max_attr_bytes: 256 }
}

/// A part of a package, as the input gives it.
pub struct Part {
    pub name: String,
    pub xml: bool,
    pub bytes: Vec<u8>,
}

/// The parts the front of `data` holds, and the edits that follow them.
pub fn parts_of(data: &[u8]) -> (Vec<Part>, &[u8]) {
    let mut parts = Vec::new();
    let mut rest = data;
    while parts.len() < 4 {
        let Some(end) = rest.iter().position(|&b| b == 0xFF) else { break };
        let (body, after) = rest.split_at(end);
        rest = after.get(1..).unwrap_or_default();
        let xml = body.first().is_some_and(|b| b % 2 == 0);
        let rels = body.first().is_some_and(|b| b & 4 != 0);
        let name = if rels { format!("/_rels/p{}.xml.rels", parts.len()) } else { format!("/p{}.xml", parts.len()) };
        parts.push(Part { name, xml, bytes: body.get(1..).unwrap_or_default().to_vec() });
    }
    (parts, rest)
}

/// The second package: `parts` with the `edits` made, three bytes each, sixteen at the most.
pub fn mutate(parts: &mut Vec<Part>, edits: &[u8]) {
    for &[op, at, byte] in edits.as_chunks::<3>().0.iter().take(16) {
        let (op, at) = (op % 7, usize::from(at));
        let n = parts.len().max(1);
        match op {
            0..=2 => {
                let Some(part) = parts.get_mut(at % n) else { continue };
                if part.bytes.is_empty() {
                    part.bytes.push(byte);
                    continue;
                }
                let place = at % part.bytes.len();
                match op {
                    0 => part.bytes[place] = byte,
                    1 => part.bytes.insert(place, byte),
                    _ => {
                        part.bytes.remove(place);
                    }
                }
            }
            3 => {
                if parts.len() > 1 {
                    let len = parts.len();
                    parts.swap(at % len, usize::from(byte) % len);
                }
            }
            4 => {
                if let Some(part) = parts.get_mut(at % n) {
                    part.name = part.name.to_ascii_uppercase();
                }
            }
            5 => {
                if !parts.is_empty() {
                    parts.remove(at % parts.len());
                }
            }
            _ => {
                if let Some(part) = parts.get(at % n) {
                    let copy = Part { name: format!("{}.copy", part.name), xml: part.xml, bytes: part.bytes.clone() };
                    parts.push(copy);
                }
            }
        }
    }
}

fn entries(parts: &[Part]) -> Vec<Entry<'_>> {
    parts.iter().map(|p| Entry { name: &p.name, xml: p.xml, bytes: &p.bytes }).collect()
}

/// The differences between `a` and `b` under `rules`, with the target's limits.
pub fn compare_with(rules: &Rules, a: &[Part], b: &[Part]) -> Result<ContentDiff, CanonError> {
    compare_entries(&entries(a), &entries(b), true, rules, &limits(), &mut Work::default())
}

/// The count of the differences between `a` and `b` under `rules`, or none when the check refuses them (a limit, and only a limit).
pub fn count_with(rules: &Rules, a: &[Part], b: &[Part]) -> Option<u64> {
    match compare_with(rules, a, b) {
        Ok(d) => {
            assert!(d.shown.len() <= MAX_SHOWN && d.shown.len() as u64 <= d.count, "the list is capped and within the count");
            Some(d.count)
        }
        Err(e) => {
            assert_eq!(e.class(), "xml_too_complex", "a refusal is a limit: {e}");
            None
        }
    }
}

/// The count under the target's own [`RULES`].
fn count(a: &[Part], b: &[Part]) -> Option<u64> {
    count_with(&RULES, a, b)
}

fn copy(parts: &[Part]) -> Vec<Part> {
    parts.iter().map(|p| Part { name: p.name.clone(), xml: p.xml, bytes: p.bytes.clone() }).collect()
}

/// A change that is always a difference, so that a check that says "the same" for everything is found out: each must count for at least one,
/// the same either way round.
fn planted(a: &[Part]) {
    let both_ways = |x: &[Part], y: &[Part], what: &str| {
        for (p, q) in [(x, y), (y, x)] {
            if let Some(n) = count(p, q) {
                assert!(n >= 1, "{what} is a difference");
            }
        }
    };
    // A byte changed in a part that is not XML.
    if let Some(i) = a.iter().position(|p| !p.xml && !p.bytes.is_empty()) {
        let mut c = copy(a);
        if let Some(byte) = c.get_mut(i).and_then(|p| p.bytes.first_mut()) {
            *byte ^= 1;
        }
        both_ways(a, &c, "a changed byte of a part that is not XML");
    }
    // A part dropped: a difference, except exactly what lets go, which is the same (the names of `a` are all different, so the part is on one
    // side only).
    if let Some(last) = a.last() {
        let mut c = copy(a);
        c.pop();
        if lets_go(last) {
            for (p, q) in [(a, &c[..]), (&c[..], a)] {
                if let Some(n) = count(p, q) {
                    assert_eq!(n, 0, "an empty relationships part dropped is no difference");
                }
            }
        } else {
            both_ways(a, &c, "a dropped part");
        }
    }
    // Through the tree: a part that parses and keeps the constraints of namespaces (one that breaks one is its bytes), written back from its tree as it is (the same
    // content, other bytes), and with an instruction added at the end of its root (a difference, found in the tree and not in the bytes).
    if let Some(i) = a.iter().position(|p| p.xml && parse_with(&p.bytes, &limits()).is_ok_and(|root| keeps_namespaces(&root))) {
        let Some(part) = a.get(i) else { return };
        let Ok(mut tree) = parse_with(&part.bytes, &limits()) else { return };
        let Ok(written) = to_bytes(&tree, b"", b"", false) else { return };
        let mut same = copy(a);
        if let Some(p) = same.get_mut(i) {
            p.bytes = written;
        }
        if let Some(n) = count(a, &same) {
            assert_eq!(n, 0, "a part written back from its tree");
        }
        // An empty child that the rules let go only under `Shape`, added to a root that is no `Shape` of any namespace: a difference.
        if tree.name.rsplit(':').next() != Some(SHAPE.local) {
            let mut under = tree.clone();
            under.children.push(Node::El(El {
                name: "u".to_owned(),
                attrs: vec![("xmlns".to_owned(), UNDER_SHAPE.ns.to_owned())],
                children: Vec::new(),
            }));
            if let Ok(written) = to_bytes(&under, b"", b"", false) {
                let mut c = copy(a);
                if let Some(p) = c.get_mut(i) {
                    p.bytes = written;
                }
                both_ways(a, &c, "an empty child under a parent no rule names for it");
            }
        }
        tree.children.push(Node::Pi { target: "fz".to_owned(), data: "1".to_owned() });
        if let Ok(written) = to_bytes(&tree, b"", b"", false) {
            let mut c = copy(a);
            if let Some(p) = c.get_mut(i) {
                p.bytes = written;
            }
            both_ways(a, &c, "an instruction added to the root");
        }
    }
}

/// Whether lets the part go when it is on one side only, read from the rule's words and not from the check's code: a relationships part by its
/// name, meant as XML, at most `MAX_EMPTY_PART_BYTES`, that parses within the limits and keeps the constraints of namespaces ([`keeps_namespaces`]), whose root is one of the
/// listed roots by the namespace its own declarations bind, and whose root has no attribute but declarations and no child but comments, white space and the empty children its
/// kind lists (by the namespace their own declarations bind, then the root's: `xmlns=""` binds none), each with no attribute but declarations and no child but comments and
/// white space.
fn lets_go(p: &Part) -> bool {
    if !rels_named(&p.name) || !p.xml || p.bytes.len() > MAX_EMPTY_PART_BYTES {
        return false;
    }
    let Ok(root) = parse_with(&p.bytes, &limits()) else { return false };
    if !keeps_namespaces(&root) {
        return false;
    }
    // The namespace and local name of `el`, its prefix read through its own declarations and then `outer`'s (the last declaration of a prefix on an element binds it).
    let name_of = |el: &El, outer: Option<&El>| -> Option<(String, String)> {
        let declared = |prefix: &str| {
            let key = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
            let on = |e: &El| e.attrs.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
            on(el).or_else(|| outer.and_then(on))
        };
        match el.name.split_once(':') {
            None => Some((declared("").unwrap_or_default(), el.name.clone())),
            Some(("xml", _)) => None,
            // A prefix nothing binds is the name as written, which no listed name is.
            Some((prefix, local)) => declared(prefix).map(|uri| (uri, local.to_owned())),
        }
    };
    let is = |found: &Option<(String, String)>, n: &Name| found.as_ref().is_some_and(|(ns, local)| ns == n.ns && local == n.local);
    let root_name = name_of(&root, None);
    let kinds: [(Name, &[Name]); 3] = [(EMPTY_ROOT, &[]), (EMPTY_E, &[]), (EMPTY_W, &[EMPTY_V])];
    let Some((_, children)) = kinds.iter().find(|(kind, _)| is(&root_name, kind)) else { return false };
    let blank = |t: &str| t.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    let no_attribute = |el: &El| el.attrs.iter().all(|(k, _)| k == "xmlns" || k.starts_with("xmlns:"));
    let holds_nothing = |el: &El| {
        no_attribute(el)
            && el.children.iter().all(|node| match node {
                Node::Comment(_) => true,
                Node::Text(t) | Node::CData(t) => blank(t),
                Node::El(_) | Node::Pi { .. } => false,
            })
    };
    let let_go = |child: &El| children.iter().any(|n| is(&name_of(child, Some(&root)), n)) && holds_nothing(child);
    no_attribute(&root)
        && root.children.iter().all(|node| match node {
            Node::Comment(_) => true,
            Node::Text(t) | Node::CData(t) => blank(t),
            Node::El(child) => let_go(child),
            Node::Pi { .. } => false,
        })
}

/// Whether the tree of `root` keeps the constraints of XML Namespaces 1.0 (sections 3 and 4), read from the standard's words and not from the check's code: every element and
/// attribute name has at most one colon and nothing empty on either side of it, no element has the prefix `xmlns`, and no declaration binds a prefix to the empty URI, declares
/// `xmlns`, binds `xml` to another URI than its own or that URI to another prefix or as the default, or binds the URI of `xmlns`. A part that breaks one is compared as bytes.
pub fn keeps_namespaces(root: &El) -> bool {
    let qualified = |name: &str| {
        let pieces: Vec<&str> = name.split(':').collect();
        pieces.len() <= 2 && pieces.iter().all(|piece| !piece.is_empty())
    };
    let keeps = |el: &El| {
        let element = qualified(&el.name) && !el.name.starts_with("xmlns:");
        element
            && el.attrs.iter().all(|(k, v)| {
                if k == "xmlns" {
                    v != XML_URI && v != XMLNS_URI
                } else if let Some(prefix) = k.strip_prefix("xmlns:") {
                    let xml = if prefix == "xml" { v == XML_URI } else { v != XML_URI };
                    !prefix.is_empty() && !prefix.contains(':') && prefix != "xmlns" && !v.is_empty() && v != XMLNS_URI && xml
                } else {
                    qualified(k)
                }
            })
    };
    !any_element(root, |el| !keeps(el))
}

/// Whether `hit` is true of some element of `root`'s tree, `root` included (walked with a stack of its own, as deep as the parser let the tree be).
fn any_element(root: &El, hit: impl Fn(&El) -> bool) -> bool {
    let mut stack = vec![root];
    while let Some(el) = stack.pop() {
        if hit(el) {
            return true;
        }
        stack.extend(el.children.iter().filter_map(|node| match node {
            Node::El(child) => Some(child),
            _ => None,
        }));
    }
    false
}

/// Whether a declaration of the tree binds [`QUIET`]: no name can be in that namespace otherwise, since a name's namespace is always one a declaration gives.
fn declares_quiet(root: &El) -> bool {
    any_element(root, |el| el.attrs.iter().any(|(k, v)| (k == "xmlns" || k.starts_with("xmlns:")) && v == QUIET))
}

/// Whether some part of the package the check reads as a tree (meant as XML, and parsing within the limits) declares [`QUIET`]. A part that does not parse is compared as
/// bytes, where no name is read; the check parses with the limits this target gives it.
fn has_quiet(parts: &[Part]) -> bool {
    parts.iter().any(|p| p.xml && parse_with(&p.bytes, &limits()).is_ok_and(|root| declares_quiet(&root)))
}

/// The law of symmetry, where it holds. The check is oriented by one rule, [`Rules::ignored_when_extra`]: without it the count is the same either way round; with it, a
/// pair that declares [`QUIET`] nowhere counts exactly what it counts without it, so it is symmetric too, and the rule changes nothing there.
fn symmetry(a: &[Part], b: &[Part]) {
    let (ab, ba) = (count_with(&SYMMETRIC, a, b), count_with(&SYMMETRIC, b, a));
    if let (Some(x), Some(y)) = (ab, ba) {
        assert_eq!(x, y, "count(a, b) == count(b, a), without the rule that orients the check");
    }
    if !has_quiet(a) && !has_quiet(b) {
        assert_eq!(count(a, b), ab, "with nothing of {QUIET}, the rule that orients the check changes no count (a, b)");
        assert_eq!(count(b, a), ba, "with nothing of {QUIET}, the rule that orients the check changes no count (b, a)");
    }
}

/// The tree of the first part that parses, keeps the constraints of namespaces ([`keeps_namespaces`]), declares nothing of [`QUIET`], and spells [`PLANTED_PREFIX`] in no name and
/// no value (so that a declaration of it binds nothing that was there, a value that names prefixes included), with its place.
fn plain_tree(a: &[Part]) -> Option<(usize, El)> {
    a.iter().enumerate().find_map(|(i, p)| {
        let root = parse_with(&p.bytes, &limits()).ok().filter(|root| p.xml && keeps_namespaces(root))?;
        let spelled =
            |el: &El| el.name.contains(PLANTED_PREFIX) || el.attrs.iter().any(|(k, v)| k.contains(PLANTED_PREFIX) || v.contains(PLANTED_PREFIX));
        (!declares_quiet(&root) && !any_element(&root, spelled)).then_some((i, root))
    })
}

/// The orientation itself: what only the second package has of [`QUIET`] is no difference, and what the first has and the second lost is one; without the rule it is a
/// difference either way round. `marked` is the tree of part `i` of `a` with the markup added.
fn marked_one_way(a: &[Part], i: usize, marked: &El, what: &str) {
    let Ok(bytes) = to_bytes(marked, b"", b"", false) else { return };
    let mut c = copy(a);
    let Some(part) = c.get_mut(i) else { return };
    part.bytes = bytes;
    if let Some(n) = count(a, &c) {
        assert_eq!(n, 0, "{what} that only the second package has is no difference");
    }
    if let Some(n) = count(&c, a) {
        assert!(n >= 1, "{what} that the second package lost is a difference");
    }
    for (x, y) in [(a, &c[..]), (&c[..], a)] {
        if let Some(n) = count_with(&SYMMETRIC, x, y) {
            assert!(n >= 1, "{what} is a difference either way round without the rule");
        }
    }
}

/// The orientation, planted on the root of a part with nothing of [`QUIET`]: an attribute of it, and an element of it beside the root's children when the root has an element child
/// already (so that blank text beside the children is read as it was) and is no paragraph of any namespace (where an element would end a run of text).
fn oriented(a: &[Part]) {
    let Some((i, tree)) = plain_tree(a) else { return };
    let mut attribute = tree.clone();
    attribute.attrs.push((format!("xmlns:{PLANTED_PREFIX}"), QUIET.to_owned()));
    attribute.attrs.push((format!("{PLANTED_PREFIX}:m"), "1".to_owned()));
    marked_one_way(a, i, &attribute, "an attribute of the namespace");
    let local = tree.name.rsplit(':').next().unwrap_or_default();
    let paragraph = RULES.paragraphs.iter().any(|n| n.local == local);
    if tree.children.iter().any(|node| matches!(node, Node::El(_))) && !paragraph {
        let mut element = tree;
        element.children.push(Node::El(El {
            name: format!("{PLANTED_PREFIX}:n"),
            attrs: vec![(format!("xmlns:{PLANTED_PREFIX}"), QUIET.to_owned()), ("a".to_owned(), "1".to_owned())],
            children: Vec::new(),
        }));
        marked_one_way(a, i, &element, "an element of the namespace");
    }
}

/// `tree` with `child` added at the end of its root, written back.
fn with_child(tree: &El, child: El) -> Option<Vec<u8>> {
    let mut grown = tree.clone();
    grown.children.push(Node::El(child));
    to_bytes(&grown, b"", b"", false).ok()
}

/// `a` with part `i`'s bytes `bytes`.
fn with_bytes(a: &[Part], i: usize, bytes: Vec<u8>) -> Vec<Part> {
    let mut c = copy(a);
    if let Some(part) = c.get_mut(i) {
        part.bytes = bytes;
    }
    c
}

/// An element `name` with the attributes `attrs` and the children `children`.
fn element(name: &str, attrs: &[(&str, &str)], children: Vec<El>) -> El {
    El {
        name: name.to_owned(),
        attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
        children: children.into_iter().map(Node::El).collect(),
    }
}

/// `xmlns=""` where a rule reads a name, planted. On the root of a part with nothing of [`QUIET`]: a parent `m` of `urn:a` with the child `g` of `urn:a` that counts as
/// nothing under it is the parent alone, and the same child with the default namespace undeclared (on itself, or on its parent written with a prefix) is a `g` of no namespace,
/// which is content. And beside the parts of the package, a relationships part whose root `w` of `urn:a` holds an empty `v` of `urn:a` is no part, and the same with the `v`
/// undeclaring the default namespace is a part. Each under the target's rules and without the one that orients the check, either way round.
fn undeclared(a: &[Part]) {
    let both_ways = |x: &[Part], y: &[Part], equal: bool, what: &str| {
        for rules in [&RULES, &SYMMETRIC] {
            for (p, q) in [(x, y), (y, x)] {
                if let Some(n) = count_with(rules, p, q) {
                    if equal {
                        assert_eq!(n, 0, "{what} is no difference");
                    } else {
                        assert!(n >= 1, "{what} is a difference");
                    }
                }
            }
        }
    };
    if let Some((i, tree)) = plain_tree(a) {
        let ns = [("xmlns", QUIET_PARENT.ns)];
        let parent = |children: Vec<El>| element("m", &ns, children);
        if let (Some(alone), Some(quiet), Some(undeclared), Some(under_prefix)) = (
            with_child(&tree, parent(Vec::new())),
            with_child(&tree, parent(vec![element("g", &[], Vec::new())])),
            with_child(&tree, parent(vec![element("g", &[("xmlns", "")], Vec::new())])),
            with_child(
                &tree,
                element(
                    &format!("{PLANTED_PREFIX}:m"),
                    &[(&format!("xmlns:{PLANTED_PREFIX}"), QUIET_PARENT.ns), ("xmlns", "")],
                    vec![element("g", &[], Vec::new())],
                ),
            ),
        ) {
            let alone = with_bytes(a, i, alone);
            both_ways(&with_bytes(a, i, quiet), &alone, true, "a child that counts as nothing");
            both_ways(&with_bytes(a, i, undeclared), &alone, false, "a child that undeclares the default namespace");
            both_ways(&with_bytes(a, i, under_prefix), &alone, false, "a child under a parent that undeclares the default namespace");
        }
    }
    let rels = |child: El| {
        let root = element("w", &[("xmlns", EMPTY_W.ns)], vec![child]);
        let bytes = to_bytes(&root, b"", b"", false).ok()?;
        let mut c = copy(a);
        c.push(Part { name: "/_rels/planted.xml.rels".to_owned(), xml: true, bytes });
        Some(c)
    };
    if let (Some(empty), Some(undeclared)) = (rels(element("v", &[], Vec::new())), rels(element("v", &[("xmlns", "")], Vec::new()))) {
        both_ways(a, &empty, true, "an empty relationships part");
        both_ways(a, &undeclared, false, "a relationships part whose child undeclares the default namespace");
    }
}

/// The properties, on one input.
pub fn check(data: &[u8]) {
    let (a, edits) = parts_of(data);
    planted(&a);
    oriented(&a);
    undeclared(&a);
    // The echo control: a package against itself is the same.
    if let Some(c) = count(&a, &a) {
        assert_eq!(c, 0, "a package against itself");
    }
    let mut b = copy(&a);
    mutate(&mut b, edits);
    symmetry(&a, &b);
    // A comment after the root is no content, when the part parses and keeps the constraints of namespaces (one that breaks one is its bytes): so appending one to every
    // part changes nothing.
    let noted: Vec<Part> = a
        .iter()
        .map(|p| Part {
            name: p.name.clone(),
            xml: p.xml,
            bytes: if p.xml { [p.bytes.as_slice(), b"\n<!-- end -->\n"].concat() } else { p.bytes.clone() },
        })
        .collect();
    let parsed = a.iter().all(|p| !p.xml || parse_with(&p.bytes, &limits()).is_ok_and(|root| keeps_namespaces(&root)));
    if parsed && let Some(c) = count(&a, &noted) {
        assert_eq!(c, 0, "a comment after the root");
    }
}
