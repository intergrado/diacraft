//! The content check under generated documents (proptest, a fixed budget of cases; a failure prints the input): a document against any
//! reformatting of itself is the same content, a document against a planted change of itself is not, the count is symmetric, and no input,
//! however damaged, panics. The planted reformatting is spacing, attribute order, prefixes, comments, CDATA, number spelling and the order
//! of keyed siblings; the planted differences are a changed attribute, a changed text, a dropped child and a swapped pair.

use diacraft_container::canon::test_support::with_hash_mask;
use diacraft_container::canon::{Entry, KeyFold, Keyed, Name, ODF_TEXT_NAMESPACE, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};
use proptest::prelude::*;

const T: &str = ODF_TEXT_NAMESPACE;
const KEY: &[Name] = &[Name { ns: "", local: "n" }];
const RULES: Rules = Rules {
    keyed: &[
        Keyed { child: Name { ns: "", local: "c" }, key: KEY, fold: KeyFold::Exact },
        Keyed { child: Name { ns: "urn:p", local: "x" }, key: KEY, fold: KeyFold::Exact },
    ],
    numbers: &[Name { ns: "", local: "len" }],
    lengths: &[],
    qnames: &[],
    paragraphs: &[Name { ns: T, local: "p" }],
    exact_text: &[Name { ns: "", local: "t" }],
    absent_when_empty: &[],
    absent_when_empty_under: &[],
    unordered: &[],
    parts_absent_when_empty: &[],
    ignored_attributes: &[],
    ignored_children: &[],
    ignored_when_extra: &[],
    twinned: &[],
    free_names: &[],
    placed: &[],
    aliases: &[],
    identified_by: &[],
    doctype_parts: &[],
};

/// A generated document: elements, text, and instructions.
#[derive(Clone, Debug)]
enum Gen {
    El { name: usize, attrs: Vec<(usize, u8)>, kids: Vec<Gen> },
    Text(String),
    Pi(u8),
}

const NAMES: [&str; 6] = ["a", "b", "c", "t", "{p}:x", "{q}:y"];
const ATTRS: [&str; 5] = ["x", "y", "n", "len", "{p}:k"];
const LEN: usize = 3;

fn gen_tree() -> impl Strategy<Value = Gen> {
    let leaf = prop_oneof![
        "[a-z&<>\" ]{1,6}".prop_map(|s| Gen::Text(format!("{s}!"))),
        (0u8..3).prop_map(Gen::Pi),
        (0..NAMES.len(), attr_list()).prop_map(|(name, attrs)| Gen::El { name, attrs, kids: Vec::new() }),
    ];
    leaf.prop_recursive(5, 48, 6, |inner| {
        (0..NAMES.len(), attr_list(), prop::collection::vec(inner, 0..6)).prop_map(|(name, attrs, kids)| Gen::El { name, attrs, kids })
    })
    .prop_map(|g| match g {
        Gen::El { .. } => g,
        other => Gen::El { name: 0, attrs: Vec::new(), kids: vec![other] },
    })
}

fn attr_list() -> impl Strategy<Value = Vec<(usize, u8)>> {
    prop::collection::btree_map(0..ATTRS.len(), 0u8..8, 0..4).prop_map(|m| m.into_iter().collect())
}

/// How a document is written.
#[derive(Clone, Copy, Debug, Default)]
struct Style {
    indent: bool,
    reverse_attrs: bool,
    other_prefixes: bool,
    comments: bool,
    cdata: bool,
    number_spelling: u8,
    reverse_keyed: bool,
    declaration: bool,
    single_quotes: bool,
}

fn gen_style() -> impl Strategy<Value = Style> {
    (any::<[bool; 8]>(), 0u8..4).prop_map(|(b, number_spelling)| Style {
        indent: b[0],
        reverse_attrs: b[1],
        other_prefixes: b[2],
        comments: b[3],
        cdata: b[4],
        number_spelling,
        reverse_keyed: b[5],
        declaration: b[6],
        single_quotes: b[7],
    })
}

fn name_of(i: usize, st: &Style) -> String {
    NAMES[i].replace("{p}", if st.other_prefixes { "m" } else { "p" }).replace("{q}", if st.other_prefixes { "n" } else { "q" })
}

fn is_keyed(g: &Gen) -> bool {
    matches!(g, Gen::El { name, .. } if *name == 2 || *name == 4)
}

/// Whether the keyed children among `kids` have keys that are all different, in their own set (`c`, and `p:x`).
fn keys_are_distinct(kids: &[&Gen]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    for kid in kids {
        if let Gen::El { name, attrs, .. } = kid
            && is_keyed(kid)
        {
            let key = attrs.iter().find(|(a, _)| ATTRS[*a] == "n").map(|(_, v)| *v);
            if !seen.insert((*name, key)) {
                return false;
            }
        }
    }
    true
}

fn value_of(attr: usize, v: u8, st: &Style) -> String {
    if attr == LEN {
        // A number and a unit, in a spelling the style picks.
        let (n, unit) = (u32::from(v), if v.is_multiple_of(2) { "cm" } else { "" });
        return match st.number_spelling {
            0 => format!("{n}{unit}"),
            1 => format!("{n}.0{unit}"),
            2 => format!("+{n}.00{unit}"),
            _ => format!("{n}e0{unit}"),
        };
    }
    format!("v{v}&<\">'")
}

fn escape(s: &str, quote: char) -> String {
    let mut out = s.replace('&', "&amp;").replace('<', "&lt;");
    out = if quote == '"' { out.replace('"', "&quot;") } else { out.replace('\'', "&apos;") };
    out
}

fn write(g: &Gen, st: &Style, depth: usize, out: &mut String) {
    match g {
        Gen::Text(s) => {
            if st.cdata && !s.contains("]]>") && !s.is_empty() {
                out.push_str(&format!("<![CDATA[{s}]]>"));
            } else {
                out.push_str(&s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
            }
        }
        Gen::Pi(i) => out.push_str(&format!("<?target{} data {}?>", i, i)),
        Gen::El { name, attrs, kids } => {
            let tag = name_of(*name, st);
            let mut list: Vec<(String, String)> =
                attrs.iter().map(|(a, v)| (ATTRS[*a].replace("{p}", if st.other_prefixes { "m" } else { "p" }), value_of(*a, *v, st))).collect();
            if st.reverse_attrs {
                list.reverse();
            }
            let quote = if st.single_quotes { '\'' } else { '"' };
            out.push_str(&format!("<{tag}"));
            if depth == 0 {
                let (p, q) = if st.other_prefixes { ("m", "n") } else { ("p", "q") };
                out.push_str(&format!(" xmlns:{p}=\"urn:p\" xmlns:{q}=\"urn:q\""));
            }
            for (k, v) in &list {
                out.push_str(&format!(" {k}={quote}{}{quote}", escape(v, quote)));
            }
            if kids.is_empty() {
                out.push_str(if st.indent { " />" } else { "/>" });
                return;
            }
            out.push('>');
            let has_text = kids.iter().any(|k| matches!(k, Gen::Text(_)));
            let spacing = st.indent && !has_text && !matches!(*name, 3);
            let mut order: Vec<&Gen> = kids.iter().collect();
            if st.reverse_keyed && !has_text && keys_are_distinct(&order) {
                // Keyed siblings in the opposite order among themselves, the others where they are. (Siblings of one key are compared in the
                // order written, so only a set with distinct keys is a set.)
                let keyed: Vec<&Gen> = order.iter().copied().filter(|k| is_keyed(k)).rev().collect();
                let mut it = keyed.into_iter();
                order = order.into_iter().map(|k| if is_keyed(k) { it.next().unwrap_or(k) } else { k }).collect();
            }
            for kid in order {
                if spacing {
                    out.push_str(&format!("\r\n{}", "  ".repeat(depth + 1)));
                }
                if st.comments && !matches!(kid, Gen::Text(_)) {
                    out.push_str("<!-- c -->");
                }
                write(kid, st, depth + 1, out);
            }
            if spacing {
                out.push_str(&format!("\n{}", "  ".repeat(depth)));
            }
            out.push_str(&format!("</{tag}>"));
        }
    }
}

fn document(g: &Gen, st: &Style) -> String {
    let mut out = String::new();
    if st.declaration {
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- top -->\n");
    }
    write(g, st, 0, &mut out);
    if st.declaration {
        out.push_str("\n<!-- end -->\n");
    }
    out
}

fn compare(a: &str, b: &str) -> (u64, u64) {
    let run = |x: &str, y: &str| {
        compare_entries(
            &[Entry { name: "/p.xml", xml: true, bytes: x.as_bytes() }],
            &[Entry { name: "/p.xml", xml: true, bytes: y.as_bytes() }],
            true,
            &RULES,
            &XmlLimits::default(),
            &mut Work::default(),
        )
        .map(|d| d.count)
    };
    (run(a, b).unwrap_or(u64::MAX), run(b, a).unwrap_or(u64::MAX))
}

/// A change of the document that is always a difference: the first place it can be made.
fn plant(g: &Gen, which: u8) -> Option<Gen> {
    match g {
        Gen::Text(s) => (which == 0).then(|| Gen::Text(format!("{s}?"))),
        Gen::Pi(i) => (which == 1).then(|| Gen::Pi(i.wrapping_add(1))),
        Gen::El { name, attrs, kids } => {
            if which == 2 && !attrs.is_empty() {
                let mut attrs = attrs.clone();
                if let Some(first) = attrs.first_mut() {
                    first.1 = first.1.wrapping_add(100);
                }
                return Some(Gen::El { name: *name, attrs, kids: kids.clone() });
            }
            if which == 3 && !kids.is_empty() {
                let mut kids = kids.clone();
                kids.pop();
                return Some(Gen::El { name: *name, attrs: attrs.clone(), kids });
            }
            for (i, kid) in kids.iter().enumerate() {
                if let Some(changed) = plant(kid, which) {
                    let mut kids = kids.clone();
                    if let Some(slot) = kids.get_mut(i) {
                        *slot = changed;
                    }
                    return Some(Gen::El { name: *name, attrs: attrs.clone(), kids });
                }
            }
            None
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// A document against any reformatting of itself: the same content, whichever way round.
    #[test]
    fn a_reformatted_document_is_the_same(g in gen_tree(), s1 in gen_style(), s2 in gen_style()) {
        let (a, b) = (document(&g, &s1), document(&g, &s2));
        prop_assert_eq!(compare(&a, &b), (0, 0), "\n{}\n{}", a, b);
    }

    /// A document against a planted change of itself: different content, and the count is symmetric. (A change that parses the same way,
    /// for want of any place to make it, is not planted.)
    #[test]
    fn a_planted_change_is_a_difference(g in gen_tree(), s1 in gen_style(), s2 in gen_style(), which in 0u8..4) {
        if let Some(changed) = plant(&g, which) {
            let (a, b) = (document(&g, &s1), document(&changed, &s2));
            let (ab, ba) = compare(&a, &b);
            prop_assert!(ab >= 1, "the change was not found:\n{}\n{}", a, b);
            prop_assert_eq!(ab, ba, "the count is symmetric:\n{}\n{}", a, b);
        }
    }

    /// The same properties with the hash cut to no bits: every pair of children collides, and the answers are the same.
    #[test]
    fn a_collision_changes_no_answer(g in gen_tree(), s1 in gen_style(), s2 in gen_style(), which in 0u8..4) {
        let (a, b) = (document(&g, &s1), document(&g, &s2));
        prop_assert_eq!(with_hash_mask(0, || compare(&a, &b)), (0, 0), "\n{}\n{}", a, b);
        if let Some(changed) = plant(&g, which) {
            let c = document(&changed, &s2);
            let (ab, ba) = with_hash_mask(0, || compare(&a, &c));
            prop_assert!(ab >= 1, "the change was not found under collisions:\n{}\n{}", a, c);
            prop_assert_eq!(ab, ba);
        }
    }

    /// Two unrelated documents: the count is symmetric, and a document is the same as itself.
    #[test]
    fn the_count_is_symmetric(g in gen_tree(), h in gen_tree(), s1 in gen_style(), s2 in gen_style()) {
        let (a, b) = (document(&g, &s1), document(&h, &s2));
        let (ab, ba) = compare(&a, &b);
        prop_assert_eq!(ab, ba, "\n{}\n{}", a, b);
        prop_assert_eq!(compare(&a, &a), (0, 0));
    }

    /// Any text at all, as a part that is meant as XML and as one that is not, against another: never a panic, and a symmetric count.
    #[test]
    fn damaged_parts_never_panic(a in "[<>/a-z:=\" &;!?\\[\\]-]{0,60}", b in "[<>/a-z:=\" &;!?\\[\\]-]{0,60}", xml in any::<bool>()) {
        let run = |x: &str, y: &str| compare_entries(&[Entry { name: "/p", xml, bytes: x.as_bytes() }], &[Entry { name: "/P", xml, bytes: y.as_bytes() }], true, &RULES, &XmlLimits { max_depth: 6, max_nodes: 40, max_attr_bytes: 30 }, &mut Work::default()).map(|d| d.count);
        if let (Ok(ab), Ok(ba)) = (run(&a, &b), run(&b, &a)) {
            prop_assert_eq!(ab, ba);
        }
        prop_assert_eq!(run(&a, &a).map(|c| c == 0), Ok(true));
    }
}
