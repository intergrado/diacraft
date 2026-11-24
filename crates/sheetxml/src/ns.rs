//! Which namespace an element is in, so that an element of another vocabulary is never read as a sheet's own.
//!
//! `container`'s tree keeps names as written (`x:note`) and resolves no namespace. A reader that matched by local name would take a third
//! party's `<x:Cell xmlns:x="urn:z">` for a cell. So the reader keeps a [`Scope`]: the default namespace and the prefixes in force, each
//! element's own `xmlns` declarations applied when it is entered. Which namespaces an element may be in to be read as the sheet's own is the
//! caller's [`Vocabulary`]: a VSDX part reads two namespaces and no namespace at all, an `.odg` reads its extension namespace and nothing else.
//!
//! **The cost is bounded by the file, whatever the file declares.** A scope is a chain of frames, one for each element that declares
//! anything, and a frame holds only its own declarations (a map) and the default namespace in force. Entering an element costs one scan of
//! its attributes for `xmlns` names, and a reference count when it declares nothing; a prefix is looked up in the frames from the nearest
//! up, and the answer is kept in the frame the question was asked of, so a root that declares ten thousand prefixes and ten thousand
//! shapes that each declare one are twenty thousand map insertions and as many lookups, not a hundred million copies (the first version
//! copied every prefix in force at every element that declared one, and took 47 seconds over a 190 KB file).
//!
//! **A lookup does not walk the frames** (each distinct prefix of a part used to walk the whole chain, and a part of 800,000 names with
//! distinct prefixes under a chain of 200 declaring frames took 25 times what the same part with one prefix did). The scopes made from one root share a
//! table of the frames that are open and declare a prefix, by the prefix: a prefix that no open frame declares is unbound at the price of one lookup in
//! it, and one that some do is found among them, nearest first, by asking whether a candidate is an ancestor, which the frames answer in at most
//! log2 of the depth steps (each keeps pointers to its 1st, 2nd, 4th, 8th ancestor and so on). [`Scope::lookup_steps`] counts those steps.

//! **What the frames cost is charged, and bounded**. A frame is made for each element that declares a namespace, and holds a table
//! of its prefixes, and the table of open frames holds each of them again: 337 bytes for each prefix were built by the first version before any charge, and a
//! part of millions of them was an abort. The decoder charges [`frame_bytes`] before it makes a frame, an element may declare at most
//! [`MAX_PREFIXES_PER_ELEMENT`] prefixes, the first frame to declare a prefix is held in the table's own entry (no vector), and a frame keeps the answers of the
//! first eight prefixes it was asked and no more, so that what a part of frames holds is what [`FRAME_BYTES`] and [`PREFIX_BYTES`] were measured to be.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use diacraft_container::xml::El;

/// The most answers a frame keeps of the prefixes asked of it. A frame is made for each element that declares a namespace, and a part can have millions: what
/// each keeps is bounded, so that what a part of them holds is bounded by what [`FRAME_BYTES`] and [`PREFIX_BYTES`] charge, and not by the names its elements use.
const ASKED_MAX: usize = 8;

/// The longest prefix whose answer a frame keeps, in bytes: the answer is kept under a copy of the prefix, and a name in a part may be a
/// mebibyte long, so that eight of them in each of sixteen frames were 137 MiB of keys that nothing charged. A real prefix is a few bytes; a longer one is looked
/// up in the table of open frames each time it is asked, which costs its bytes and keeps none.
const ASKED_KEY_MAX: usize = 64;

/// The most prefixes one element may declare, and the decoder refuses an element that declares more. A real element declares a
/// few (the most the sample files have on one is under thirty); the declarations of one element are made into a table before anything of the element is read.
pub const MAX_PREFIXES_PER_ELEMENT: usize = 1_024;

/// What a frame costs, whatever it declares, **at the worst**: the frame itself, the pointers up to its ancestors, its tables, and the answers it may keep
/// ([`ASKED_MAX`] of them). Measured (`model_scopes.rs`).
pub const FRAME_BYTES: u64 = 1_808;

/// What each prefix an element declares costs, beside the bytes of its name (kept twice, in the frame and in the table of open frames) and of its namespace:
/// an entry in the frame's table, an entry in the table of open frames, and the strings' headers. Measured (`model_scopes.rs`).
pub const PREFIX_BYTES: u64 = 320;

/// What a frame for an element that declares `prefixes` prefixes, `bytes` long in all (the names twice and the namespaces once), costs the model: what a decoder
/// charges before it makes the frame. An element that declares nothing makes none. Saturating.
pub fn frame_bytes(prefixes: usize, bytes: u64) -> u64 {
    FRAME_BYTES.saturating_add(u64::try_from(prefixes).unwrap_or(u64::MAX).saturating_mul(PREFIX_BYTES)).saturating_add(bytes)
}

/// The namespace an unbound prefix is in: none a vocabulary knows, so the element is not read as its own. A [`Vocabulary`] is asked about it
/// like about any other URI, and a vocabulary that reads "no namespace at all" must not read this.
pub const UNBOUND: &str = "urn:unbound-prefix";

/// The namespaces an element may be in to be read as the sheet's own.
///
/// `reads` is given the namespace URI an element's name is bound to: `None` when the name has no prefix and no default namespace is in
/// force, `Some("")` for a default namespace that was declared empty, [`UNBOUND`] for a prefix nothing binds. It is a small `Copy` value
/// (usually a unit struct), so that a [`Scope`] of it is as cheap to clone as a reference count.
pub trait Vocabulary: Copy {
    /// Whether an element in the namespace `uri` is read as this vocabulary's own.
    fn reads(self, uri: Option<&str>) -> bool;
}

/// What the frames made from one root share: the frames that are open and declare a prefix, by the prefix, and the work the lookups did.
struct Shared {
    /// For each prefix, the open frames that declare it, in the order they were made.
    declared: RefCell<HashMap<String, Declarers>>,
    /// The candidates and the ancestors looked at by prefix lookups: the work, counted.
    steps: Cell<u64>,
    next_id: Cell<u64>,
}

struct Declarer {
    id: u64,
    depth: usize,
    frame: Weak<Frame>,
}

/// The open frames that declare one prefix, in the order they were made. **The first is held in the table's own entry**, so that a prefix declared once costs
/// no vector (which makes room for four at its first push): a part of millions of prefixes is millions of these.
enum Declarers {
    One(Declarer),
    Many(Vec<Declarer>),
}

impl Declarers {
    fn push(&mut self, declarer: Declarer) {
        match self {
            Declarers::Many(list) => list.push(declarer),
            Declarers::One(_) => {
                let Declarers::One(first) = std::mem::replace(self, Declarers::Many(Vec::new())) else { return };
                *self = Declarers::Many(vec![first, declarer]);
            }
        }
    }

    /// Takes out the latest frame of this `id`; whether none are left.
    fn remove(&mut self, id: u64) -> bool {
        match self {
            Declarers::One(only) => only.id == id,
            Declarers::Many(list) => {
                if let Some(at) = list.iter().rposition(|d| d.id == id) {
                    list.remove(at);
                }
                list.is_empty()
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        match self {
            Declarers::One(_) => 1,
            Declarers::Many(list) => list.len(),
        }
    }

    /// The frames, the latest first.
    fn latest_first(&self) -> Box<dyn Iterator<Item = &Declarer> + '_> {
        match self {
            Declarers::One(only) => Box::new(std::iter::once(only)),
            Declarers::Many(list) => Box::new(list.iter().rev()),
        }
    }
}

struct Frame {
    /// How many frames are above this one.
    depth: usize,
    /// The 1st, 2nd, 4th, 8th (and so on) ancestor, as far up as there is one: a level ancestor is found in log2 of the distance.
    up: Vec<Rc<Frame>>,
    id: u64,
    /// The default namespace in force here: this element's own declaration, else the nearest above, else none.
    default: Option<Rc<str>>,
    /// The prefixes this element declares, and nothing it inherits.
    own: HashMap<String, Rc<str>>,
    /// Prefixes already looked up from this frame, with the answer (`None` for one the open frames above and here do not bind).
    asked: RefCell<HashMap<String, Option<Rc<str>>>>,
    shared: Rc<Shared>,
}

impl Drop for Frame {
    /// A frame that is closed is no longer a candidate for the prefixes it declared (usually the last of its prefix's list: frames close in the order they
    /// were opened, last first).
    fn drop(&mut self) {
        if self.own.is_empty() {
            return;
        }
        let mut declared = self.shared.declared.borrow_mut();
        for prefix in self.own.keys() {
            if let Some(list) = declared.get_mut(prefix)
                && list.remove(self.id)
            {
                declared.remove(prefix);
            }
        }
    }
}

impl Frame {
    /// The ancestor of this frame at `depth` (itself when `depth` is its own), found by jumps of a power of two; `steps` counts the jumps.
    fn ancestor_at(&self, depth: usize, steps: &Cell<u64>) -> Option<&Frame> {
        let mut at = self;
        let mut distance = self.depth.checked_sub(depth)?;
        while distance > 0 {
            let jump = usize::try_from(distance.ilog2()).ok()?;
            at = at.up.get(jump)?;
            steps.set(steps.get().saturating_add(1));
            distance -= 1usize << jump;
        }
        Some(at)
    }
}

/// The namespaces in force at an element, and the [`Vocabulary`] that says which of them are the sheet's own. Cheap to clone.
#[derive(Clone)]
pub struct Scope<V: Vocabulary> {
    frame: Rc<Frame>,
    vocab: V,
}

impl<V: Vocabulary + Default> Scope<V> {
    /// The scope above the root, for a vocabulary that has one value.
    #[must_use]
    pub fn empty() -> Scope<V> {
        Scope::new(V::default())
    }
}

impl<V: Vocabulary> Scope<V> {
    /// The scope above the root: no default namespace and no prefix.
    #[must_use]
    pub fn new(vocab: V) -> Scope<V> {
        let shared = Rc::new(Shared { declared: RefCell::new(HashMap::new()), steps: Cell::new(0), next_id: Cell::new(0) });
        let frame = Frame { depth: 0, up: Vec::new(), id: 0, default: None, own: HashMap::new(), asked: RefCell::new(HashMap::new()), shared };
        Scope { frame: Rc::new(frame), vocab }
    }

    /// The scope inside `el`: this one with the declarations on `el` applied. A reference count when `el` declares nothing.
    #[must_use]
    pub fn enter(&self, el: &El) -> Scope<V> {
        if !el.attrs.iter().any(|(k, _)| k == "xmlns" || k.starts_with("xmlns:")) {
            return self.clone();
        }
        let mut default = self.frame.default.clone();
        let mut own: HashMap<String, Rc<str>> = HashMap::new();
        for (k, v) in &el.attrs {
            if k == "xmlns" {
                default = Some(Rc::from(v.as_str()));
            } else if let Some(prefix) = k.strip_prefix("xmlns:") {
                // The first declaration of a prefix on one element wins, as the parser's duplicate-attribute rule would have it.
                own.entry(prefix.to_owned()).or_insert_with(|| Rc::from(v.as_str()));
            }
        }
        let shared = Rc::clone(&self.frame.shared);
        let id = shared.next_id.get().saturating_add(1);
        shared.next_id.set(id);
        // The pointers up: the parent, and then each is the one above the one before it.
        let mut up: Vec<Rc<Frame>> = vec![Rc::clone(&self.frame)];
        while let Some(last) = up.last() {
            match last.up.get(up.len() - 1) {
                Some(above) => {
                    let above = Rc::clone(above);
                    up.push(above);
                }
                None => break,
            }
        }
        let depth = self.frame.depth.saturating_add(1);
        let frame = Rc::new(Frame { depth, up, id, default, own, asked: RefCell::new(HashMap::new()), shared: Rc::clone(&shared) });
        if !frame.own.is_empty() {
            let mut declared = shared.declared.borrow_mut();
            for prefix in frame.own.keys() {
                let declarer = Declarer { id, depth, frame: Rc::downgrade(&frame) };
                match declared.get_mut(prefix) {
                    Some(list) => list.push(declarer),
                    None => {
                        declared.insert(prefix.clone(), Declarers::One(declarer));
                    }
                }
            }
        }
        Scope { frame, vocab: self.vocab }
    }

    /// What `classify` makes of the namespace of `el`, which must be the element this scope was entered for. `classify` is given what
    /// [`Vocabulary::reads`] is given: no URI for a name with no namespace, [`UNBOUND`] for a prefix nothing binds.
    pub fn classify<T>(&self, el: &El, classify: impl FnOnce(Option<&str>) -> T) -> T {
        match el.name.split_once(':') {
            None => classify(self.frame.default.as_deref()),
            // An unbound prefix is no namespace anyone knows.
            Some((prefix, _)) => match self.binding(prefix) {
                Some(uri) => classify(Some(&uri)),
                None => classify(Some(UNBOUND)),
            },
        }
    }

    /// Whether `el`, which must be the element this scope was entered for, is in a namespace the vocabulary reads as its own.
    #[must_use]
    pub fn is_ours(&self, el: &El) -> bool {
        self.classify(el, |uri| self.vocab.reads(uri))
    }

    /// Whether `el` is an element called `local` in a namespace the vocabulary reads as its own: its local name, and its namespace.
    #[must_use]
    pub fn is(&self, el: &El, local: &str) -> bool {
        el.is(local) && self.is_ours(el)
    }

    /// How many frames the prefix lookups of this scope and every scope made from the same root have visited: the work they did, for a test to hold to a
    /// count (a time moves with the load on the machine).
    #[must_use]
    pub fn lookup_steps(&self) -> u64 {
        self.frame.shared.steps.get()
    }

    /// The URI `prefix` is bound to here, or `None`: the nearest open frame at or above this one that declares it. A prefix that no open frame declares is
    /// unbound at once; otherwise the frames that declare it are asked, latest first, whether they are above this one, and the answer is kept in this frame.
    fn binding(&self, prefix: &str) -> Option<Rc<str>> {
        if let Some(answer) = self.frame.asked.borrow().get(prefix) {
            return answer.clone();
        }
        let shared = &self.frame.shared;
        let answer = {
            let declared = shared.declared.borrow();
            // No open frame declares it: the answer is none, and is not kept (a part of millions of distinct prefixes would keep millions).
            let candidates = declared.get(prefix)?;
            let mut found = None;
            for candidate in candidates.latest_first() {
                shared.steps.set(shared.steps.get().saturating_add(1));
                if candidate.depth > self.frame.depth {
                    continue;
                }
                let Some(frame) = candidate.frame.upgrade() else { continue };
                if let Some(above) = self.frame.ancestor_at(candidate.depth, &shared.steps)
                    && std::ptr::eq(above, Rc::as_ptr(&frame))
                {
                    found = frame.own.get(prefix).cloned();
                    break;
                }
            }
            found
        };
        let mut asked = self.frame.asked.borrow_mut();
        if asked.len() < ASKED_MAX && prefix.len() <= ASKED_KEY_MAX {
            asked.insert(prefix.to_owned(), answer.clone());
        }
        answer
    }

    /// The namespace URI of `el`, which must be the element this scope was entered for: `None` for no namespace (and for a prefix nothing binds).
    #[must_use]
    pub fn uri_of_element(&self, el: &El) -> Option<String> {
        match el.name.split_once(':') {
            None => self.frame.default.as_deref().filter(|d| !d.is_empty()).map(str::to_owned),
            Some((prefix, _)) => self.binding(prefix).map(|uri| uri.to_string()),
        }
    }

    /// The URI `prefix` is bound to here, or `None` (an empty `prefix` asks for the default namespace, and an empty one is none): what a name that is kept
    /// and written back needs declared with it.
    #[must_use]
    pub fn bound(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return self.frame.default.as_deref().filter(|d| !d.is_empty()).map(str::to_owned);
        }
        self.binding(prefix).map(|uri| uri.to_string())
    }

    /// Whether the attribute `name` is `local` in the namespace `ns` here: it has a prefix, and the prefix is bound to `ns`.
    #[must_use]
    pub fn is_attr_in(&self, name: &str, local: &str, ns: &str) -> bool {
        name.split_once(':').is_some_and(|(prefix, l)| l == local && self.binding(prefix).as_deref() == Some(ns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Default)]
    struct Anything;

    impl Vocabulary for Anything {
        fn reads(self, _uri: Option<&str>) -> bool {
            true
        }
    }

    fn declaring(prefix: &str, uri: &str) -> El {
        El { name: "x:e".to_owned(), attrs: vec![(format!("xmlns:{prefix}"), uri.to_owned())], children: Vec::new() }
    }

    /// How many prefixes the open frames are listed under, and how many frames in all.
    fn listed(scope: &Scope<Anything>) -> (usize, usize) {
        let declared = scope.frame.shared.declared.borrow();
        (declared.len(), declared.values().map(Declarers::len).sum())
    }

    #[test]
    fn a_frame_that_closes_leaves_no_entry_for_its_prefixes_and_no_empty_list() {
        // A part of a million distinct prefixes, each declared by an element that closes, must not leave a million entries (or a million empty lists).
        let root = Scope::<Anything>::empty();
        assert_eq!(listed(&root), (0, 0));
        {
            let outer = root.enter(&declaring("a", "urn:a"));
            assert_eq!(listed(&outer), (1, 1));
            {
                let inner = outer.enter(&declaring("a", "urn:a2"));
                let other = inner.enter(&declaring("b", "urn:b"));
                assert_eq!(listed(&other), (2, 3));
            }
            assert_eq!(listed(&outer), (1, 1), "the two inner frames are out of the list, and the prefix only they declared is out of the table");
        }
        assert_eq!(listed(&root), (0, 0));
        for i in 0..1_000 {
            let _closed = root.enter(&declaring(&format!("p{i}"), "urn:p"));
        }
        assert_eq!(listed(&root), (0, 0));
    }

    #[test]
    fn a_frame_that_declares_no_prefix_is_never_listed() {
        // A default namespace is no prefix: the frame is made, and nothing is listed under any name for it.
        let root = Scope::<Anything>::empty();
        let default = El { name: "e".to_owned(), attrs: vec![("xmlns".to_owned(), "urn:d".to_owned())], children: Vec::new() };
        let inside = root.enter(&default);
        assert_eq!(listed(&inside), (0, 0));
    }

    /// What a frame keeps under the prefixes asked of it: the number of answers, and the bytes of the keys.
    fn kept_answers(scope: &Scope<Anything>) -> (usize, usize) {
        let asked = scope.frame.asked.borrow();
        (asked.len(), asked.keys().map(String::len).sum())
    }

    #[test]
    fn a_frame_keeps_no_answer_under_a_prefix_longer_than_the_most_a_key_may_be() {
        // A prefix may be a mebibyte long and a frame kept eight of them, uncharged (sixteen frames, 137 MiB).
        let root = Scope::<Anything>::empty();
        let long = "p".repeat(ASKED_KEY_MAX + 1);
        let outer = root.enter(&declaring(&long, "urn:long"));
        let inner = outer.enter(&declaring("q", "urn:q"));
        // Asked of the frame that declares it, and of one below: found, and kept under no key.
        assert_eq!(outer.bound(&long).as_deref(), Some("urn:long"));
        assert_eq!(inner.bound(&long).as_deref(), Some("urn:long"));
        assert_eq!(kept_answers(&outer), (0, 0));
        assert_eq!(kept_answers(&inner), (0, 0));
        // An unbound one is no more kept (it never was).
        assert_eq!(inner.bound(&"z".repeat(1 << 20)), None);
        assert_eq!(kept_answers(&inner), (0, 0));
    }

    #[test]
    fn a_frame_keeps_the_answer_under_a_prefix_of_exactly_the_most_a_key_may_be() {
        let root = Scope::<Anything>::empty();
        let longest = "p".repeat(ASKED_KEY_MAX);
        let outer = root.enter(&declaring(&longest, "urn:long"));
        let inner = outer.enter(&declaring("q", "urn:q"));
        assert_eq!(inner.bound(&longest).as_deref(), Some("urn:long"));
        assert_eq!(kept_answers(&inner), (1, ASKED_KEY_MAX));
    }

    #[test]
    fn a_frame_keeps_the_answers_of_the_first_eight_prefixes_asked_of_it_and_no_more() {
        let root = Scope::<Anything>::empty();
        let many = El {
            name: "x:e".to_owned(),
            attrs: (0..ASKED_MAX + 4).map(|i| (format!("xmlns:w{i}"), "urn:w".to_owned())).collect(),
            children: Vec::new(),
        };
        let outer = root.enter(&many);
        let busy = outer.enter(&declaring("busy", "urn:busy"));
        for i in 0..ASKED_MAX + 4 {
            assert_eq!(busy.bound(&format!("w{i}")).as_deref(), Some("urn:w"));
        }
        assert_eq!(kept_answers(&busy).0, ASKED_MAX);
    }

    #[test]
    fn each_frame_has_an_id_of_its_own() {
        let root = Scope::<Anything>::empty();
        let first = root.enter(&declaring("a", "urn:a"));
        let second = root.enter(&declaring("b", "urn:b"));
        let third = first.enter(&declaring("c", "urn:c"));
        let ids = [root.frame.id, first.frame.id, second.frame.id, third.frame.id];
        for (i, a) in ids.iter().enumerate() {
            for b in ids.iter().skip(i + 1) {
                assert_ne!(a, b, "{ids:?}");
            }
        }
    }
}
