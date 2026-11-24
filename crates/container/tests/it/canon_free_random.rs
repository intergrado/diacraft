//! Names that are free (`Rules::free_names`) on random documents, from a fixed seed: the properties the fuzz target asserts, held over a few thousand documents of definitions
//! (two families and a kind with none), references that ask for each, an embedded scope, and a container that holds nothing when it holds only what counts as nothing:
//! a document against itself is the same; the count is symmetric; a document with every name given another is the same; a reference moved to a definition of another content is a difference.
use diacraft_container::canon::{Definition, Entry, FreeNames, IgnoredUnder, KeyFold, Keyed, Name, Reference, Rules, Takes, compare_entries};
use diacraft_container::{Work, XmlLimits};

const fn n(local: &'static str) -> Name {
    Name { ns: "", local }
}

const FREE: FreeNames = FreeNames {
    container: n("Autos"),
    definitions: &[Definition { element: n("Def"), family: Some(n("fam")) }, Definition { element: n("List"), family: None }],
    name: n("name"),
    references: &[
        Reference { attr: n("ref"), on: Some(n("Item")), takes: &[Takes { element: n("Def"), family: "a" }], list: false },
        Reference { attr: n("ref"), on: Some(n("Other")), takes: &[Takes { element: n("Def"), family: "b" }], list: false },
        Reference { attr: n("ref"), on: Some(n("Set")), takes: &[Takes { element: n("List"), family: "" }], list: false },
    ],
    definition_references: &[],
    scopes: &[n("Embedded")],
};

const RULES: Rules = Rules {
    keyed: &[Keyed { child: n("Def"), key: &[n("fam"), n("name")], fold: KeyFold::Exact }],
    absent_when_empty: &[n("Meta")],
    ignored_children: &[IgnoredUnder { parent: n("Meta"), children: &[n("Gen")] }],
    free_names: &[FREE],
    ..Rules::NONE
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 >> 11
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A document: definitions with unique (family, name) pairs and references to existing ones, built from `names` (a rename map).
struct Spec {
    defs: Vec<(String, &'static str, u64)>,
    lists: Vec<(String, u64)>,
    items: Vec<(&'static str, usize)>,
    embedded: Vec<usize>,
    gens: u64,
    meta_title: bool,
}

fn spec(rng: &mut Rng) -> Spec {
    let nd = 1 + rng.below(5) as usize;
    let mut defs = Vec::new();
    for i in 0..nd {
        let fam = ["a", "b"][rng.below(2) as usize];
        defs.push((format!("d{i}"), fam, rng.below(3)));
    }
    let lists = (0..rng.below(3)).map(|i| (format!("l{i}"), rng.below(2))).collect::<Vec<_>>();
    let mut items = Vec::new();
    for _ in 0..rng.below(6) {
        let at = rng.below(nd as u64) as usize;
        let kind = if defs.get(at).map(|d: &(String, &'static str, u64)| d.1) == Some("a") { "Item" } else { "Other" };
        items.push((kind, at));
    }
    let embedded = (0..rng.below(2)).map(|_| rng.below(nd as u64) as usize).collect();
    Spec { defs, lists, items, embedded, gens: rng.below(3), meta_title: rng.below(2) == 0 }
}

fn render(sp: &Spec, rename: &dyn Fn(&str) -> String) -> String {
    let mut s = String::from("<Doc><Autos>");
    for (name, fam, v) in &sp.defs {
        s.push_str(&format!("<Def name=\"{}\" fam=\"{fam}\"><P v=\"{v}\"/></Def>", rename(name)));
    }
    for (name, v) in &sp.lists {
        s.push_str(&format!("<List name=\"{}\"><P v=\"{v}\"/></List>", rename(name)));
    }
    s.push_str("</Autos><Body>");
    for (kind, at) in &sp.items {
        if let Some((name, _, _)) = sp.defs.get(*at) {
            s.push_str(&format!("<{kind} ref=\"{}\"/>", rename(name)));
        }
    }
    for (i, _) in sp.lists.iter().enumerate() {
        s.push_str(&format!("<Set ref=\"{}\"/>", rename(&format!("l{i}"))));
    }
    s.push_str("<Embedded>");
    for at in &sp.embedded {
        s.push_str(&format!("<Item ref=\"d{at}\"/>"));
    }
    s.push_str("</Embedded><Meta>");
    for _ in 0..sp.gens {
        s.push_str("<Gen>g</Gen>");
    }
    if sp.meta_title {
        s.push_str("<Title/>");
    }
    s.push_str("</Meta></Body></Doc>");
    s
}

fn count(a: &str, b: &str) -> u64 {
    let ea = [Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }];
    let eb = [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }];
    let ab = compare_entries(&ea, &eb, false, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    let ba = compare_entries(&eb, &ea, false, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "symmetric: {a}\n{b}");
    assert!(ab.shown.len() as u64 <= ab.count);
    ab.count
}

#[test]
fn random_documents_with_free_names_keep_the_properties() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for _ in 0..4000 {
        let sp = spec(&mut rng);
        let plain = render(&sp, &|s| s.to_owned());
        // A document against itself.
        assert_eq!(count(&plain, &plain), 0);
        // Every name given another: the embedded names are no names of this document (they are compared as written), so the property holds when it names none.
        let renamed = render(&sp, &|s| format!("zz_{s}"));
        // The embedded references keep `dN` (they are compared as written), which both sides write alike: still the same.
        assert_eq!(count(&plain, &renamed), 0, "{plain}\n{renamed}");
        // A reference moved to another definition of another content is a difference.
        if sp.defs.len() >= 2 && !sp.items.is_empty() {
            let mut other = Spec {
                defs: sp.defs.clone(),
                lists: sp.lists.clone(),
                items: sp.items.clone(),
                embedded: sp.embedded.clone(),
                gens: sp.gens,
                meta_title: sp.meta_title,
            };
            if let Some((kind, at)) = other.items.first_mut() {
                let next = (*at + 1) % other.defs.len();
                let same_family = other.defs.get(next).map(|d| d.1) == other.defs.get(*at).map(|d| d.1);
                let same_content = other.defs.get(next).map(|d| d.2) == other.defs.get(*at).map(|d| d.2);
                let right_family = other.defs.get(*at).is_some_and(|d| (*kind == "Item" && d.1 == "a") || (*kind == "Other" && d.1 == "b"));
                let next_family_ok = other.defs.get(next).is_some_and(|d| (*kind == "Item" && d.1 == "a") || (*kind == "Other" && d.1 == "b"));
                *at = next;
                if right_family && next_family_ok && same_family && !same_content {
                    let c = count(&plain, &render(&other, &|s| format!("zz_{s}")));
                    assert!(c >= 1, "a reference to another definition: {plain}");
                }
            }
        }
    }
}
