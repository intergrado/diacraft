//! Names that are free (for the copies and the references between definitions; [`FreeNames`]): [`resolve`] matches the definitions of two documents by what they
//! say, before either document's canonical form is built, and gives each side what the walk that builds it needs, so that a definition and every reference to it read as the definition's
//! content and not as the name its document chose.
//!
//! **The passes.** One pass over each side builds the children of the container (the definitions) as the whole check builds an element. The definitions that name one another
//! ([`FreeNames::definition_references`]) are put in order, a definition after every definition it names, by a walk of the references on an explicit stack; **a definition that is in a
//! circle, whose chain of references is longer than [`MAX_CHAIN`], or that names one of those, is compared as written** (it is not in the order, and keeps its name and its references).
//! Then the content of each definition in the order is written into one string of bytes, its **signature**, in which a reference to a definition is the number of the class of that
//! definition: so a definition's signature is its content with what it names resolved, whatever the names are. The signatures of both sides are interned in one map, so that two
//! definitions with one signature, on either side, have one number, its **class**; a class that has a definition on each side is **matched**. **Definitions of one class on one side are
//! copies, and count as one** (the first stays, the others are left out). The second pass is the walk that builds the whole tree, which reads the container's children from the first pass
//! with the name of each matched definition replaced by its class and the references inside it read as classes, and reads every reference to a matched definition as the class too.
//!
//! **Why it never calls different content the same.** Two names read alike only when the signatures of the definitions they name are the same string (the map is keyed by the
//! string, so a collision of hashes is only a slower lookup), and a reference reads as a class only when it asks for the kind and the family of a definition of that name that the
//! container holds once. A definition with no match keeps its name, and so does every reference to it; a name in the container that no rule reads, or in an element of
//! [`FreeNames::scopes`], is not read.
//!
//! **What it costs.** The container's children are built once, as they would be, and their signatures are written once: a step for each element and attribute, and a byte for each
//! byte of the signature, counted; the walk of the references is a step for each definition and each reference, and no definition is visited twice. A name (a namespace or a local name)
//! is a number of four bytes in a signature, and is kept once. The map keeps one copy of each different signature and the table of names. **The budget is charged before anything is
//! built**: a side's container is weighed (an explicit walk of it, counted: [`weight`]) and one that weighs more than [`MAX_NAMED_BYTES`] has every name of both documents compared as
//! written, so that the answer is the same whichever document is first, and what the map and the table hold is less than that weight. The lookup of a name is one hash of it.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};

use super::{
    Builder, CAttr, Canon, Ctx, FreeNames, Item, KeyedEl, MAX_CHAIN, MAX_NAMED_BYTES, Name, QName, QUIET, Reference, Refusal, Rules, SAME_FAMILY,
    Takes, Txt, Val, seal,
};
use crate::limits::XML_DEPTH_CEILING;
use crate::work::Work;
use crate::xml::{El, Node};

/// What a definition that has a match is called, and a reference to it: this character, which no document holds (XML 1.0 has no place for it), and the number of its class.
const MARK: char = '\u{4}';

/// The name a matched definition and the references to it are read as.
pub(super) fn marker(class: usize) -> String {
    format!("{MARK}{class}")
}

/// What each different signature costs besides its bytes: the vector and its place in the map.
const ENTRY_BYTES: u64 = 64;

/// What each element of the container weighs besides its name, attributes and text ([`weight`]): more than a signature's fixed bytes and the place it is kept in, so that what a
/// container weighs is more than what its signatures hold.
const ELEMENT_WEIGHT: u64 = 2 * ENTRY_BYTES;

/// A bound on what the content of `container` is written out as, charged before anything is built or written: for each element a fixed weight and its name, for each attribute its name
/// and value, for each text its bytes. A side whose container weighs more than [`MAX_NAMED_BYTES`] has every name read as written, **and so has the other side's**: the decision is of
/// each side alone, so that the answer is the same whichever document is first. The walk is on an explicit stack, and counted: a step for each element (its attributes and children are
/// read in the same visit, in a loop that does no more than add their lengths).
fn weight(container: &El) -> (u64, usize) {
    let (mut total, mut looked) = (0u64, 0usize);
    let mut stack = vec![container];
    while let Some(el) = stack.pop() {
        looked = looked.saturating_add(1);
        total = total.saturating_add(ELEMENT_WEIGHT).saturating_add(el.name.len() as u64);
        for (key, value) in &el.attrs {
            total = total.saturating_add((key.len() as u64).saturating_add(value.len() as u64)).saturating_add(8);
        }
        for node in &el.children {
            match node {
                Node::El(child) => stack.push(child),
                Node::Text(t) | Node::CData(t) => total = total.saturating_add(t.len() as u64).saturating_add(8),
                Node::Pi { target, data } => total = total.saturating_add((target.len() as u64).saturating_add(data.len() as u64)).saturating_add(8),
                Node::Comment(_) => {}
            }
        }
    }
    (total, looked)
}

/// A definition's kind (its element), its family (`""` for a kind with none) and its name.
type Key<'a> = (&'a str, &'a str, &'a str, &'a str);

/// A child of the container as the walk that builds the whole tree finds it.
pub(super) enum Slot<'a> {
    /// Built in the first pass, its name and its references read as classes where the definition has a match.
    Built(Canon<'a>),
    /// A copy of a definition that is already there: it counts as that one, and is left out.
    Dropped,
}

/// What the walk that builds one side's canonical form is given: the children of the container, and the matched definitions a reference may name.
pub(super) struct Naming<'a> {
    children: VecDeque<Slot<'a>>,
    classes: HashMap<Key<'a>, usize>,
    /// The bytes the qualified values of the first pass expanded to ([`super::MAX_QUALIFIED_BYTES`]): the walk goes on from there.
    pub(super) qualified: u64,
    /// Whether an element the first pass built breaks a constraint of XML Namespaces 1.0 (the part is then bytes): the walk does not build those children again.
    pub(super) breaks: bool,
}

impl<'a> Naming<'a> {
    /// What the attribute `attr` of the element `owner`, with the value `value`, reads as when it is a reference to a matched definition (the first of `refs` that asks for that
    /// attribute on that element decides), or none when it is not one.
    pub(super) fn read(&self, refs: &[Reference], owner: QName<'_>, attr: QName<'a>, value: &'a str) -> Option<String> {
        let a = CAttr { name: attr, val: Val::Text(value), quiet: false };
        let tokens = resolve_attr(refs, owner, &a, "", &self.classes)?;
        Some(
            tokens
                .iter()
                .map(|t| match t {
                    Tok::Found(class) => marker(*class),
                    Tok::Raw(text) => (*text).to_owned(),
                })
                .collect::<Vec<String>>()
                .join(" "),
        )
    }

    /// The next child of the container, when `parent` is the container.
    pub(super) fn next_child(&mut self, rules: &[FreeNames], parent: QName<'_>) -> Option<Slot<'a>> {
        let rule = rules.first()?;
        if parent.is(&rule.container) { self.children.pop_front() } else { None }
    }
}

/// What one name of a reference reads as: the definition it names, or the text it was.
enum Tok<'a, T> {
    Found(T),
    Raw(&'a str),
}

/// The family a reference asks for, given the family of the definition it is in.
fn family_of<'f>(takes: &Takes, own: &'f str) -> &'f str {
    if takes.family == SAME_FAMILY { own } else { takes.family }
}

/// The one definition of `map` that `name` names among the kinds `takes` asks for: none when none has it, and none when more than one kind has it (the name is not told apart).
fn resolve_one<'a, T: Copy>(takes: &[Takes], own: &'a str, name: &'a str, map: &HashMap<Key<'a>, T>) -> Option<T> {
    let mut found = None;
    for t in takes {
        if let Some(&hit) = map.get(&(t.element.ns, t.element.local, family_of(t, own), name)) {
            if found.is_some() {
                return None;
            }
            found = Some(hit);
        }
    }
    found
}

/// What the attribute `a` of the element `owner` reads as: for the first of `refs` that asks for that attribute on that element, the definitions of `map` it names (a list of names
/// names each), and the text of a name that names none; none when no reference asks for it or no name in it names a definition. `own` is the family of the definition it is in.
fn resolve_attr<'a, T: Copy>(
    refs: &[Reference],
    owner: QName<'_>,
    a: &CAttr<'a>,
    own: &'a str,
    map: &HashMap<Key<'a>, T>,
) -> Option<Vec<Tok<'a, T>>> {
    let r = refs.iter().find(|r| a.name.is(&r.attr) && r.on.is_none_or(|on| owner.is(&on)))?;
    let Val::Text(value) = a.val else { return None };
    if !r.list {
        return resolve_one(r.takes, own, value, map).map(|hit| vec![Tok::Found(hit)]);
    }
    let mut any = false;
    let tokens: Vec<Tok<'a, T>> = value
        .split_ascii_whitespace()
        .map(|name| match resolve_one(r.takes, own, name, map) {
            Some(hit) => {
                any = true;
                Tok::Found(hit)
            }
            None => Tok::Raw(name),
        })
        .collect();
    any.then_some(tokens)
}

/// A definition of the first pass: where it is among the container's children, its key, the definitions it names, and where the walk of the references has it.
struct Def<'a> {
    at: usize,
    key: Key<'a>,
    refs: Vec<usize>,
    state: u8,
    height: usize,
    bad: bool,
}

/// A definition the first pass told: where it is among the children, its key, its class (none for one that is compared as written: a circle, a chain too long, or one that names such a
/// one), and whether it names another definition.
struct Out<'a> {
    at: usize,
    key: Key<'a>,
    class: Option<usize>,
    names_one: bool,
}

/// The definitions of one side, from the first pass.
struct Found<'a> {
    /// Every child of the container that is an element, built, in order.
    children: Vec<Canon<'a>>,
    /// The definitions that can be told from the rest: a name that two of a kind and family hold is not one.
    defs: Vec<Out<'a>>,
    qualified: u64,
    /// Whether an element the pass built breaks a constraint of XML Namespaces 1.0.
    breaks: bool,
}

enum Scan<'a> {
    /// A document with two containers, or one whose container weighs more than [`MAX_NAMED_BYTES`]: every name is read as written.
    Off,
    Found(Found<'a>),
}

/// The signatures of the definitions of both sides, one number for each different one, and which sides have it; and the names the signatures are written in.
struct Interner {
    map: HashMap<Vec<u8>, usize>,
    seen: Vec<[bool; 2]>,
    /// The names (a namespace, a local name) that the signatures hold, each as a number: a name is written once in a signature as four bytes, however long it is.
    names: HashMap<String, u32>,
    /// What the map and the names hold, in bytes.
    held: u64,
    /// The most a side's container may weigh ([`weight`]; [`MAX_NAMED_BYTES`]). What the map and the names hold is bounded by twice it for each side, since the weight is more than the
    /// signatures are, and is held to four times it besides, a bound that is never reached.
    budget: u64,
}

impl Interner {
    fn new(budget: u64) -> Interner {
        Interner { map: HashMap::new(), seen: Vec::new(), names: HashMap::new(), held: 0, budget }
    }

    /// The class of `signature`, which `side` holds: its number if the map has it, else a new one; none when it would pass the budget.
    fn intern(&mut self, signature: Vec<u8>, side: usize) -> Result<usize, Stop> {
        // What a different signature holds: its bytes and the place the map keeps it in.
        let cost = (signature.len() as u64).saturating_add(ENTRY_BYTES);
        let next = self.map.len();
        let class = match self.map.entry(signature) {
            Entry::Occupied(known) => *known.get(),
            Entry::Vacant(free) => {
                let held = self.held.saturating_add(cost);
                if held > self.budget.saturating_mul(4) {
                    return Err(Stop);
                }
                free.insert(next);
                self.seen.push([false; 2]);
                self.held = held;
                next
            }
        };
        if let Some(slot) = self.seen.get_mut(class).and_then(|seen| seen.get_mut(side)) {
            *slot = true;
        }
        Ok(class)
    }
}

/// Why a signature was not made: it would pass the budget, so every name is read as written.
struct Stop;

/// What a signature needs to write a reference as the class of the definition it names.
struct Resolver<'r, 'a> {
    refs: &'r [Reference],
    index: &'r HashMap<Key<'a>, usize>,
    class_of: &'r [Option<usize>],
    /// The family of the definition the signature is of.
    family: &'a str,
}

/// The content of a definition written out: unambiguous (every text is led by its length, every name is a number and every reference to a definition is the number of its class), and
/// equal for two definitions exactly when the whole check compares them as equal with what they name resolved.
struct Sig<'i, 'a> {
    out: Vec<u8>,
    /// The names of the signatures, and what they and the signatures hold ([`Interner`]).
    names: &'i mut HashMap<String, u32>,
    held: &'i mut u64,
    budget: u64,
    resolver: Resolver<'i, 'a>,
    /// Elements and attributes written, for the count of the work.
    nodes: usize,
}

impl<'a> Sig<'_, 'a> {
    fn put(&mut self, data: &[u8]) -> Result<(), Stop> {
        let after = (self.out.len() as u64).saturating_add(data.len() as u64);
        if after > self.budget.saturating_mul(2) {
            return Err(Stop);
        }
        self.out.extend_from_slice(data);
        Ok(())
    }

    fn num(&mut self, n: usize) -> Result<(), Stop> {
        self.put(&u32::try_from(n).map_err(|_| Stop)?.to_le_bytes())
    }

    fn text(&mut self, s: &str) -> Result<(), Stop> {
        self.num(s.len())?;
        self.put(s.as_bytes())
    }

    /// A name, as its number.
    fn word(&mut self, s: &str) -> Result<(), Stop> {
        let id = match self.names.get(s) {
            Some(&id) => id,
            None => {
                let cost = (s.len() as u64).saturating_add(ENTRY_BYTES);
                let held = self.held.saturating_add(cost);
                if held > self.budget.saturating_mul(4) {
                    return Err(Stop);
                }
                let id = u32::try_from(self.names.len()).map_err(|_| Stop)?;
                self.names.insert(s.to_owned(), id);
                *self.held = held;
                id
            }
        };
        self.put(&id.to_le_bytes())
    }

    fn name(&mut self, n: QName<'_>) -> Result<(), Stop> {
        self.word(n.ns)?;
        self.word(n.local)
    }

    fn val(&mut self, v: &Val<'_>) -> Result<(), Stop> {
        match v {
            // A text and an owned text of one string are one value ([`Val`]).
            Val::Text(s) => {
                self.put(&[0])?;
                self.text(s)
            }
            Val::Owned(s) => {
                self.put(&[0])?;
                self.text(s)
            }
            Val::Num(bits, unit) => {
                self.put(&[1])?;
                self.put(&bits.to_le_bytes())?;
                self.text(unit)
            }
        }
    }

    /// An attribute's value: the classes of the definitions it names when it is a reference to some, else as the value is.
    fn value(&mut self, owner: QName<'_>, a: &CAttr<'a>) -> Result<(), Stop> {
        let tokens = resolve_attr(self.resolver.refs, owner, a, self.resolver.family, self.resolver.index);
        let Some(tokens) = tokens else { return self.val(&a.val) };
        let mut classes = Vec::with_capacity(tokens.len());
        for token in &tokens {
            classes.push(match token {
                // The definition it names has a class: it was put in order before this one. A definition with none is a bug of the order, and is refused.
                Tok::Found(i) => Tok::Found(self.resolver.class_of.get(*i).copied().flatten().ok_or(Stop)?),
                Tok::Raw(text) => Tok::Raw(text),
            });
        }
        self.put(&[2])?;
        self.num(classes.len())?;
        for token in classes {
            match token {
                Tok::Found(class) => {
                    self.put(&[1])?;
                    self.num(class)?;
                }
                Tok::Raw(text) => {
                    self.put(&[0])?;
                    self.text(text)?;
                }
            }
        }
        Ok(())
    }

    /// `c`, without its attribute `skip` (a definition's name) and what counts as nothing when extra, the children after it, to the depth the parser allows.
    fn element(&mut self, c: &Canon<'a>, skip: Option<&Name>, depth: usize) -> Result<(), Stop> {
        if depth > XML_DEPTH_CEILING {
            return Err(Stop);
        }
        self.nodes = self.nodes.saturating_add(1).saturating_add(c.attrs.len());
        self.put(b"E")?;
        self.name(c.name)?;
        let counted = |a: &&CAttr<'_>| !a.quiet && !skip.is_some_and(|n| a.name.is(n));
        let held = c.attrs.iter().filter(counted).count();
        self.num(held)?;
        for a in c.attrs.iter().filter(counted) {
            self.name(a.name)?;
            self.value(c.name, a)?;
        }
        self.num(c.items.len())?;
        for item in &c.items {
            match item {
                Item::El(e) => {
                    self.put(b"e")?;
                    self.element(e, None, depth + 1)?;
                }
                Item::Text(Txt::Plain(t), _) => {
                    self.put(b"t")?;
                    self.text(t)?;
                }
                Item::Text(Txt::Para(t), _) => {
                    self.put(b"u")?;
                    self.text(t)?;
                }
                Item::Pi(target, data, _) => {
                    self.put(b"p")?;
                    self.text(target)?;
                    self.text(data)?;
                }
            }
        }
        let keyed: Vec<&KeyedEl<'_>> = c.keyed.iter().filter(|k| k.class != QUIET).collect();
        self.num(keyed.len())?;
        for k in keyed {
            self.num(k.class)?;
            self.num(k.key.len())?;
            for v in &k.key {
                match v {
                    None => self.put(&[0])?,
                    Some(v) => {
                        self.put(&[1])?;
                        self.val(v)?;
                    }
                }
            }
            self.element(&k.el, None, depth + 1)?;
        }
        Ok(())
    }
}

/// The kind, family and name of the definition `c` is, or none when it is not one that can be told from the rest: a kind the rule does not list, no name or more than one, a name
/// or family that is not plain text.
fn key_of<'a>(rule: &FreeNames, c: &Canon<'a>) -> Option<Key<'a>> {
    let definition = rule.definitions.iter().find(|d| c.name.is(&d.element))?;
    let plain = |a: &CAttr<'a>| match &a.val {
        Val::Text(t) => Some(*t),
        Val::Num(..) | Val::Owned(_) => None,
    };
    let mut names = c.attrs.iter().filter(|a| a.name.is(&rule.name));
    let name = plain(names.next()?)?;
    if names.next().is_some() {
        return None;
    }
    let family = match definition.family {
        None => "",
        Some(attr) => {
            let mut families = c.attrs.iter().filter(|a| a.name.is(&attr));
            match (families.next(), families.next()) {
                (None, _) => "",
                (Some(only), None) => plain(only)?,
                (Some(_), Some(_)) => return None,
            }
        }
    };
    Some((c.name.ns, c.name.local, family, name))
}

/// The definitions of `index` that `c` names, in the order the references stand (the first pass over `c` and what is below it, to the depth the parser allows; counted by `looked`).
fn named_by<'a>(
    c: &Canon<'a>,
    rule: &FreeNames,
    family: &'a str,
    index: &HashMap<Key<'a>, usize>,
    out: &mut Vec<usize>,
    looked: &mut usize,
    depth: usize,
) {
    if depth > XML_DEPTH_CEILING {
        return;
    }
    *looked = looked.saturating_add(1).saturating_add(c.attrs.len());
    for a in c.attrs.iter().filter(|a| !a.quiet) {
        if let Some(tokens) = resolve_attr(rule.definition_references, c.name, a, family, index) {
            out.extend(tokens.into_iter().filter_map(|t| if let Tok::Found(i) = t { Some(i) } else { None }));
        }
    }
    for item in &c.items {
        if let Item::El(e) = item {
            named_by(e, rule, family, index, out, looked, depth + 1);
        }
    }
    for k in c.keyed.iter().filter(|k| k.class != QUIET) {
        named_by(&k.el, rule, family, index, out, looked, depth + 1);
    }
}

/// Puts the definitions in an order in which each comes after every definition it names, and marks the others bad (`bad`): a definition in a circle, one whose chain of names is longer
/// than [`MAX_CHAIN`], and one that names a bad one. A walk on an explicit stack that visits each definition once: the answer does not depend on the order the definitions stand in.
/// Returns the order of the good ones and what it looked at.
fn put_in_order(defs: &mut [Def<'_>]) -> (Vec<usize>, usize) {
    let (mut order, mut looked) = (Vec::new(), 0usize);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..defs.len() {
        if defs.get(start).is_none_or(|d| d.state != 0) {
            continue;
        }
        if let Some(d) = defs.get_mut(start) {
            d.state = 1;
        }
        stack.push((start, 0));
        while let Some(&(v, next)) = stack.last() {
            looked = looked.saturating_add(1);
            let named = defs.get(v).and_then(|d| d.refs.get(next)).copied();
            if let Some(r) = named {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                match defs.get(r).map(|d| d.state) {
                    Some(0) => {
                        if let Some(d) = defs.get_mut(r) {
                            d.state = 1;
                        }
                        stack.push((r, 0));
                    }
                    // A definition that is being walked: the reference closes a circle through it and through this one.
                    Some(1) => {
                        if let Some(d) = defs.get_mut(r) {
                            d.bad = true;
                        }
                        if let Some(d) = defs.get_mut(v) {
                            d.bad = true;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            // Every definition this one names is done (or is being walked, which made this one bad).
            let (mut bad, mut height) = (false, 0usize);
            let refs: Vec<usize> = defs.get(v).map(|d| d.refs.clone()).unwrap_or_default();
            for r in refs {
                match defs.get(r) {
                    Some(d) if d.state == 2 => {
                        bad |= d.bad;
                        height = height.max(d.height.saturating_add(1));
                    }
                    _ => bad = true,
                }
            }
            if let Some(d) = defs.get_mut(v) {
                d.bad |= bad || height > MAX_CHAIN;
                d.height = height;
                d.state = 2;
                if !d.bad {
                    order.push(v);
                }
            }
            stack.pop();
        }
    }
    (order, looked)
}

/// The first pass over one side: the container's children, built, and the class of each definition that can be told and is not in a circle or too deep a chain.
fn scan<'r, 'a>(
    rules: &'r Rules,
    rule: &FreeNames,
    root: &'a El,
    interner: &mut Interner,
    side: usize,
    work: &mut Work,
) -> Result<Scan<'a>, Refusal> {
    let mut builder = Builder::new(rules, work);
    let root_bound = builder.bind(root);
    let containers: Vec<&'a El> = root
        .children
        .iter()
        .filter_map(|node| if let Node::El(child) = node { Some(child) } else { None })
        .filter(|child| {
            let child: &'a El = child;
            builder.expand_with(child, &child.name).is(&rule.container)
        })
        .collect();
    let [container] = containers.as_slice() else {
        let found = Found { children: Vec::new(), defs: Vec::new(), qualified: 0, breaks: false };
        builder.unbind(root_bound);
        return Ok(if containers.is_empty() { Scan::Found(found) } else { Scan::Off });
    };
    let container: &'a El = container;
    // What the container weighs, before anything is built for it ([`weight`]).
    let (weighs, looked) = weight(container);
    builder.steps(looked);
    if weighs > interner.budget {
        builder.unbind(root_bound);
        return Ok(Scan::Off);
    }
    let bound = builder.bind(container);
    let ctx = Ctx { in_para: false };
    let mut children: Vec<Canon<'a>> = Vec::new();
    let mut candidates: Vec<(usize, Key<'a>)> = Vec::new();
    for node in &container.children {
        let Node::El(definition) = node else { continue };
        // The depth a child of the container has in the walk that builds the whole tree.
        let built = builder.build(definition, ctx, 2)?;
        if let Some(key) = key_of(rule, &built) {
            candidates.push((children.len(), key));
        }
        children.push(built);
    }
    builder.unbind(bound);
    builder.unbind(root_bound);
    builder.steps(candidates.len());
    // A name two definitions of one kind and family hold is not one the check can tell apart: both are compared as written.
    let mut counts: HashMap<Key<'a>, u32> = HashMap::new();
    for (_, key) in &candidates {
        *counts.entry(*key).or_default() += 1;
    }
    let mut defs: Vec<Def<'a>> = Vec::new();
    let mut index: HashMap<Key<'a>, usize> = HashMap::new();
    for (at, key) in candidates {
        if counts.get(&key) == Some(&1) {
            index.insert(key, defs.len());
            defs.push(Def { at, key, refs: Vec::new(), state: 0, height: 0, bad: false });
        }
    }
    // What each definition names.
    let mut looked = 0usize;
    for d in 0..defs.len() {
        let Some((at, family)) = defs.get(d).map(|def| (def.at, def.key.2)) else { continue };
        let Some(child) = children.get(at) else { continue };
        let mut named = Vec::new();
        named_by(child, rule, family, &index, &mut named, &mut looked, 0);
        if let Some(def) = defs.get_mut(d) {
            def.refs = named;
        }
    }
    builder.steps(looked);
    let (order, looked) = put_in_order(&mut defs);
    builder.steps(looked);
    // The class of each, after every one it names.
    let mut class_of: Vec<Option<usize>> = vec![None; defs.len()];
    for v in order {
        let Some((at, family)) = defs.get(v).map(|def| (def.at, def.key.2)) else { continue };
        let Some(built) = children.get(at) else { continue };
        // One signature may be as long as the whole budget (the same content twice is held once), and the different ones together may not pass it.
        let mut signature = Sig {
            out: Vec::new(),
            names: &mut interner.names,
            held: &mut interner.held,
            budget: interner.budget,
            resolver: Resolver { refs: rule.definition_references, index: &index, class_of: &class_of, family },
            nodes: 0,
        };
        if signature.element(built, Some(&rule.name), 0).is_err() {
            return Ok(Scan::Off);
        }
        let (nodes, out) = (signature.nodes, signature.out);
        builder.steps(nodes);
        builder.bytes(out.len());
        let Ok(class) = interner.intern(out, side) else { return Ok(Scan::Off) };
        if let Some(slot) = class_of.get_mut(v) {
            *slot = Some(class);
        }
    }
    let found: Vec<Out<'a>> = defs
        .iter()
        .enumerate()
        .map(|(i, d)| Out { at: d.at, key: d.key, class: class_of.get(i).copied().flatten(), names_one: !d.refs.is_empty() })
        .collect();
    Ok(Scan::Found(Found { children, defs: found, qualified: builder.qualified, breaks: builder.breaks }))
}

/// `c`, a definition of the family `family`, with the value of its attribute `name` replaced by `marker`, and every reference below it to a matched definition read as the class.
fn rewritten<'a>(c: Canon<'a>, rule: &FreeNames, name: Option<&str>, family: &'a str, classes: &HashMap<Key<'a>, usize>, depth: usize) -> Canon<'a> {
    let Canon { name: element, mut attrs, items, keyed, .. } = c;
    for a in &mut attrs {
        if let Some(marker) = name.filter(|_| a.name.is(&rule.name)) {
            a.val = Val::Owned(marker.to_owned());
        } else if let Some(tokens) = resolve_attr(rule.definition_references, element, a, family, classes) {
            let joined: Vec<String> = tokens
                .iter()
                .map(|t| match t {
                    Tok::Found(class) => marker(*class),
                    Tok::Raw(text) => (*text).to_owned(),
                })
                .collect();
            a.val = Val::Owned(joined.join(" "));
        }
    }
    let below = depth + 1;
    let items = items
        .into_iter()
        .map(|item| match item {
            Item::El(e) if below <= XML_DEPTH_CEILING => Item::El(Box::new(rewritten(*e, rule, None, family, classes, below))),
            other => other,
        })
        .collect();
    let keyed = keyed
        .into_iter()
        .map(|k| {
            if k.class != QUIET && below <= XML_DEPTH_CEILING { KeyedEl { el: rewritten(k.el, rule, None, family, classes, below), ..k } } else { k }
        })
        .collect();
    // Only values were rewritten: the attributes stay in the order the comparison merges them in (by name, the copies of one name in the order written).
    seal(element, attrs, items, keyed)
}

/// What the walk that builds a side is given: the definitions that have a match on the other side, their names read as their classes, the copies of a class left out, and **the
/// references in every definition that names another read as the class of the one it names when that one has a match**: a definition that has no match keeps its name and its
/// references to what has none, and a reference of it to a definition that has one is read as its class, so that a name that stands for one definition in the first document and for
/// none in the second is a difference (and not the same text on both sides).
fn finish<'a>(rule: &FreeNames, found: Found<'a>, matched: &[bool]) -> Naming<'a> {
    let Found { children, defs, qualified, breaks } = found;
    let mut classes: HashMap<Key<'a>, usize> = HashMap::new();
    // Copies: the first definition of a class stays, and every other is left out.
    let mut first_of: HashMap<usize, usize> = HashMap::new();
    let mut dropped: HashSet<usize> = HashSet::new();
    for o in &defs {
        let Some(class) = o.class else { continue };
        if matched.get(class).copied().unwrap_or(false) {
            classes.insert(o.key, class);
        }
        if *first_of.entry(class).or_insert(o.at) != o.at {
            dropped.insert(o.at);
        }
    }
    let by_at: HashMap<usize, &Out<'a>> = defs.iter().map(|o| (o.at, o)).collect();
    let children = children
        .into_iter()
        .enumerate()
        .map(|(at, child)| {
            if dropped.contains(&at) {
                return Slot::Dropped;
            }
            let Some(o) = by_at.get(&at) else { return Slot::Built(child) };
            let name = o.class.filter(|class| matched.get(*class).copied().unwrap_or(false)).map(marker);
            Slot::Built(if name.is_some() || o.names_one { rewritten(child, rule, name.as_deref(), o.key.2, &classes, 0) } else { child })
        })
        .collect();
    Naming { children, classes, qualified, breaks }
}

/// Matches the definitions of the two documents `a` and `b` by their content, and gives each side what the walk that builds its canonical form needs; none when the rules have no
/// names to free, or the documents are such that every name is compared as written (see [`Scan::Off`]).
pub(super) fn resolve<'a, 'b>(rules: &Rules, a: &'a El, b: &'b El, work: &mut Work) -> Result<Option<(Naming<'a>, Naming<'b>)>, Refusal> {
    resolve_within(rules, a, b, work, MAX_NAMED_BYTES)
}

/// [`resolve`] with a budget of `budget` bytes for what a container may weigh, for the tests that bring the budget within reach.
fn resolve_within<'a, 'b>(rules: &Rules, a: &'a El, b: &'b El, work: &mut Work, budget: u64) -> Result<Option<(Naming<'a>, Naming<'b>)>, Refusal> {
    let Some(rule) = rules.free_names.first() else { return Ok(None) };
    let mut interner = Interner::new(budget);
    let Scan::Found(first) = scan(rules, rule, a, &mut interner, 0, work)? else { return Ok(None) };
    let Scan::Found(second) = scan(rules, rule, b, &mut interner, 1, work)? else { return Ok(None) };
    // A class is matched when both sides hold a definition of it.
    let matched: Vec<bool> = interner.seen.iter().map(|sides| sides.iter().all(|held| *held)).collect();
    Work::add(&mut work.canon_steps, first.defs.len().saturating_add(second.defs.len()));
    Ok(Some((finish(rule, first, &matched), finish(rule, second, &matched))))
}

#[cfg(test)]
mod tests {
    use super::super::Definition;
    use super::*;
    use crate::limits::XmlLimits;
    use crate::xml;

    const S: &str = "urn:s";
    const NAME: Name = Name { ns: S, local: "name" };
    const DEF: Name = Name { ns: S, local: "def" };
    const TAKES: &[Takes] = &[Takes { element: DEF, family: "" }];

    const RULE: FreeNames = FreeNames {
        container: Name { ns: S, local: "autos" },
        definitions: &[Definition { element: DEF, family: None }],
        name: NAME,
        references: &[],
        definition_references: &[Reference { attr: Name { ns: S, local: "ref" }, on: None, takes: TAKES, list: false }],
        scopes: &[],
    };
    const RULES: Rules = Rules { free_names: &[RULE], ..Rules::NONE };

    fn parse(inner: &str) -> El {
        xml::parse_with(format!("<s:doc xmlns:s=\"{S}\"><s:autos>{inner}</s:autos></s:doc>").as_bytes(), &XmlLimits::default()).unwrap()
    }

    fn doc(names: &[&str], body: &str) -> El {
        parse(&names.iter().map(|n| format!("<s:def s:name=\"{n}\"><s:p v=\"{body}\"/></s:def>")).collect::<String>())
    }

    #[test]
    fn a_marker_is_the_character_no_document_holds_and_the_number() {
        assert_eq!(marker(0), "\u{4}0");
        assert_eq!(marker(12), "\u{4}12");
        assert!(xml::parse_with("<a v=\"\u{4}\"/>".as_bytes(), &XmlLimits::default()).is_err(), "a document cannot say the marker itself");
    }

    #[test]
    fn the_signatures_are_interned_and_matched_by_what_they_say_without_the_name() {
        let (a, b) = (doc(&["x", "y"], "1"), doc(&["p", "q"], "1"));
        let (na, nb) = resolve(&RULES, &a, &b, &mut Work::default()).unwrap().unwrap();
        // Two definitions of one content are one class, whichever names they have: the four are read as the class.
        let class = |n: &Naming<'_>, name: &str| n.classes.iter().find(|(k, _)| k.3 == name).map(|(_, c)| *c);
        assert_eq!([class(&na, "x"), class(&na, "y"), class(&nb, "p"), class(&nb, "q")], [Some(0); 4]);
        assert_eq!(class(&na, "z"), None);
        // A different content has no match: it keeps its name, and no reference reads as its class.
        let c = doc(&["x"], "2");
        let (na, nc) = resolve(&RULES, &a, &c, &mut Work::default()).unwrap().unwrap();
        assert_eq!((class(&na, "x"), class(&nc, "x")), (None, None));
    }

    #[test]
    fn copies_of_a_class_on_one_side_count_as_one() {
        let (a, b) = (doc(&["x", "y", "z"], "1"), doc(&["p"], "1"));
        let (mut na, mut nb) = resolve(&RULES, &a, &b, &mut Work::default()).unwrap().unwrap();
        let rules = [RULE];
        let parent = QName { ns: S, local: "autos" };
        let kinds = |n: &mut Naming<'_>| (0..3).map(|_| n.next_child(&rules, parent).map(|s| matches!(s, Slot::Built(_)))).collect::<Vec<_>>();
        assert_eq!(kinds(&mut na), [Some(true), Some(false), Some(false)], "the first stays, the other two are left out");
        assert_eq!(kinds(&mut nb), [Some(true), None, None]);
        // Unmatched copies are one as well.
        let c = doc(&["x", "y"], "9");
        let (mut nc, _) = resolve(&RULES, &c, &b, &mut Work::default()).unwrap().unwrap();
        assert_eq!(kinds(&mut nc)[..2], [Some(true), Some(false)]);
    }

    #[test]
    fn past_the_budget_every_name_is_read_as_written_whichever_document_is_first() {
        let (small, big) = (doc(&["x"], "1"), doc(&["x", "y", "z", "w"], &"1".repeat(100)));
        let mut work = Work::default();
        let (w_small, w_big) = (weight(find(&small)).0, weight(find(&big)).0);
        assert!(w_small < w_big);
        // A budget the larger container passes: every name is read as written, in both orders; one it does not: the names are read.
        for budget in [w_small, w_big - 1] {
            assert!(resolve_within(&RULES, &small, &big, &mut work, budget).unwrap().is_none(), "{budget}");
            assert!(resolve_within(&RULES, &big, &small, &mut work, budget).unwrap().is_none(), "{budget}, the other way");
        }
        assert!(resolve_within(&RULES, &small, &big, &mut work, w_big).unwrap().is_some());
        assert!(resolve_within(&RULES, &big, &small, &mut work, w_big).unwrap().is_some());
        // What the weight bounds is what is held.
        let mut interner = Interner::new(u64::MAX / 8);
        let _ = scan(&RULES, &RULE, &big, &mut interner, 0, &mut Work::default()).unwrap();
        assert!(interner.held < w_big, "{} held, {w_big} weighed", interner.held);
    }

    /// The `s:autos` of a document.
    fn find(root: &El) -> &El {
        root.children
            .iter()
            .find_map(|node| match node {
                Node::El(el) if el.name == "s:autos" => Some(el),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn the_same_content_under_any_number_of_names_is_held_once() {
        let held = |names: &[&str]| {
            let mut interner = Interner::new(u64::MAX / 8);
            let _ = scan(&RULES, &RULE, &doc(names, &"1".repeat(100)), &mut interner, 0, &mut Work::default()).unwrap();
            interner.held
        };
        assert_eq!(held(&["x"]), held(&["x", "y", "z"]), "three of one content are held as one");
        assert!(held(&["x"]) > 0);
    }

    fn def_naming(refs: Vec<usize>) -> Def<'static> {
        Def { at: 0, key: ("", "", "", ""), refs, state: 0, height: 0, bad: false }
    }

    #[test]
    fn definitions_are_put_in_order_after_what_they_name_and_a_circle_or_a_long_chain_is_bad() {
        // 0 names 1, which names 2; 3 names 1 too: the order has each after what it names.
        let mut defs = vec![def_naming(vec![1]), def_naming(vec![2]), def_naming(vec![]), def_naming(vec![1])];
        let (order, _) = put_in_order(&mut defs);
        let place = |d: usize| order.iter().position(|&o| o == d);
        assert!(defs.iter().all(|d| !d.bad));
        assert!(place(2) < place(1) && place(1) < place(0) && place(1) < place(3));
        // A circle of two, one that names it, one that is named by it, and a self reference: the first three are bad, and the last is good.
        let mut defs = vec![def_naming(vec![1]), def_naming(vec![0, 3]), def_naming(vec![0]), def_naming(vec![]), def_naming(vec![4])];
        let (order, _) = put_in_order(&mut defs);
        assert_eq!(defs.iter().map(|d| d.bad).collect::<Vec<_>>(), [true, true, true, false, true]);
        assert_eq!(order, [3]);
        // The answer does not depend on the order the definitions stand in.
        let mut reversed = vec![def_naming(vec![]), def_naming(vec![2, 0]), def_naming(vec![3]), def_naming(vec![1])];
        let (_, _) = put_in_order(&mut reversed);
        assert_eq!(reversed.iter().map(|d| d.bad).collect::<Vec<_>>(), [false, true, true, true], "a circle of 1, 2, 3 and the one it does not name");
        // A chain of MAX_CHAIN + 1 names is the longest that is good, and the next is bad, whichever end the walk starts at.
        for n in [MAX_CHAIN + 1, MAX_CHAIN + 2] {
            let chain = |reversed: bool| {
                let mut defs: Vec<Def<'static>> = (0..n).map(|i| def_naming(if i + 1 < n { vec![i + 1] } else { vec![] })).collect();
                if reversed {
                    defs = (0..n).map(|i| def_naming(if i > 0 { vec![i - 1] } else { vec![] })).collect();
                }
                put_in_order(&mut defs);
                defs.iter().map(|d| d.bad).collect::<Vec<_>>()
            };
            let (forward, backward) = (chain(false), chain(true));
            assert_eq!(forward.iter().filter(|b| **b).count(), n.saturating_sub(MAX_CHAIN + 1), "{n} names in a chain");
            assert_eq!(backward.iter().filter(|b| **b).count(), n.saturating_sub(MAX_CHAIN + 1));
        }
    }
}
