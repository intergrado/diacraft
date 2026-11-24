//! The fuzz target `canon_compare` (`crates/container/fuzz`) replayed on the inputs it crashed on: its properties are its library's, which this file includes, so the test
//! runs the very law the fuzzer holds the check to, and a seeded run of generated inputs holds that law where the fuzzer took millions of runs to reach.
//!
//! The full gate's run on `786463e8` stopped at `count(a, b) == count(b, a)` with 2 against 1. The cause is the one rule that orients the check,
//! `Rules::ignored_when_extra`: what only the second package has of its namespace counts as nothing, and what the first has must be in the second. The
//! target listed that rule and still asked every pair for symmetry. Without the rule the same pair counts 1 either way round; the law now asks for symmetry without it,
//! for the same count with it wherever nothing of its namespace is declared, and for the orientation itself where it is planted.

use diacraft_container::canon::{ContentDiff, Found, Rules};
use proptest::prelude::*;

#[path = "../../fuzz/src/lib.rs"]
mod harness;

use harness::{Part, RULES, SYMMETRIC, compare_with, count_with, mutate, parts_of};

/// The input the full gate's 10-minute run on `786463e8` crashed on, after about 7.95 million runs (`crash-8e3be10ced42182842b64582dc246ca98218a0f5`, 226 bytes, the fuzzer's own):
/// four parts, the first an element `X:-` twice, each with a child `r` of the namespace `urn:x`, and one edit that takes the `s` out of the second child's `xmlns`.
const CRASH_8E3B: &[u8] = b"\xa4<f><f>s[1|||||\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"<X:- QQQQQQ=\"  \"><r xmlns=\"urn:x\"/></X:->\"\"\"\
    |\"\"\"||||||||~|||||||\"\"\"\"\"\"\"\"\"\"\"\"\"<X:- Q-QQQQQ=\"  \"><r xmlns=\"urn:x\"/></X:->\"\"\"\
    |\"\"\"||||||||~||||||||||||</f >0m$\x0df</f >\xff \x00|||||</f >0m$\x0df</f >\xff \x00\x09\xff\xb4\xff\
    r\x84<";

/// The two packages of an input of the target: the parts it cuts, and a copy with its edits made.
fn packages(data: &[u8]) -> (Vec<Part>, Vec<Part>) {
    let (a, edits) = parts_of(data);
    let (mut b, _) = parts_of(data);
    mutate(&mut b, edits);
    (a, b)
}

/// One XML part of the given name.
fn part(name: &str, xml: &str) -> Vec<Part> {
    vec![Part { name: name.to_owned(), xml: true, bytes: xml.as_bytes().to_vec() }]
}

/// The count under `rules` both ways round.
fn both(rules: &Rules, a: &[Part], b: &[Part]) -> (Option<u64>, Option<u64>) {
    (count_with(rules, a, b), count_with(rules, b, a))
}

#[test]
fn the_gates_crash_on_786463e8_holds_every_property_of_the_target() {
    assert_eq!(CRASH_8E3B.len(), 226, "the input as the fuzzer saved it");
    harness::check(CRASH_8E3B);
}

#[test]
fn the_crash_is_the_orientation_of_the_rule_and_without_that_rule_the_count_is_symmetric() {
    let (a, b) = packages(CRASH_8E3B);
    // The edit took the `s` out of the second `r`'s declaration: in the second package that `r` is no element of `urn:x` but a plain one with an attribute.
    let first = String::from_utf8_lossy(&a[0].bytes).into_owned();
    let second = String::from_utf8_lossy(&b[0].bytes).into_owned();
    assert_eq!((first.matches("xmlns=\"urn:x\"").count(), second.matches("xmlns=\"urn:x\"").count()), (2, 1));
    assert_eq!(second.matches("<r xmln=\"urn:x\"/>").count(), 1);
    assert!(a[1..].iter().zip(&b[1..]).all(|(x, y)| x.name == y.name && x.bytes == y.bytes), "the edit is in the first part alone");
    // With the rule that orients the check: the first's `r` of `urn:x` is missing from the second, and the second's plain `r` is extra (2); the other way round the plain `r`
    // is missing (1) and the `r` of `urn:x` is what only the second has (nothing).
    assert_eq!(both(&RULES, &a, &b), (Some(2), Some(1)));
    // Without it, every other rule of the target in force: the two `r` are one element that differs, either way round.
    assert_eq!(both(&SYMMETRIC, &a, &b), (Some(1), Some(1)));
}

#[test]
fn the_crash_made_small_counts_two_one_way_and_one_the_other_only_under_the_rule_that_orients_it() {
    let source = part("/p.xml", "<f><X:- Q=\"1\"><r xmlns=\"urn:x\"/></X:-></f>");
    let written = part("/p.xml", "<f><X:- Q=\"1\"><r xmln=\"urn:x\"/></X:-></f>");
    let compare = |rules: &Rules, a: &[Part], b: &[Part]| compare_with(rules, a, b).unwrap();
    let at = |d: &ContentDiff| d.shown.iter().map(|x| (x.path.clone(), x.found)).collect::<Vec<_>>();
    let place = "/f[0]/X:-[0]/r[0]".to_owned();
    let forward = compare(&RULES, &source, &written);
    assert_eq!(at(&forward), vec![(place.clone(), Found::Element), (place.clone(), Found::Element)]);
    let back = compare(&RULES, &written, &source);
    assert_eq!(at(&back), vec![(place.clone(), Found::Element)]);
    for (a, b) in [(&source, &written), (&written, &source)] {
        assert_eq!(at(&compare(&SYMMETRIC, a, b)), vec![(place.clone(), Found::Element)]);
    }
    // The rule changes nothing where nothing of its namespace is declared: the plain `r` against no `r` at all is one difference either way round, with the rule and without.
    let none = part("/p.xml", "<f><X:- Q=\"1\"/></f>");
    for rules in [&RULES, &SYMMETRIC] {
        assert_eq!(both(rules, &written, &none), (Some(1), Some(1)));
    }
}

#[test]
fn the_orientation_is_planted_on_a_part_with_nothing_of_the_namespace() {
    // A part that parses and declares nothing of `urn:x`, with an element child: the target plants an attribute and an element of `urn:x` on its root, and holds them to
    // the orientation (no difference when only the second has them, a difference when only the first does, and either way round without the rule).
    harness::check(b"\x00<f a=\"1\"><g/> t </f>\xff");
    // The same, with edits that change the second package, and parts that the rules read (a shape with its text, a section of rows, an empty relationships part).
    harness::check(b"\x00<Shape><Text> a </Text><Gen/></Shape>\xff\x04<Empty/>\xff\x00<Section N=\"Geometry\"><Row/><Row/></Section>\xff\x00\x01\x05\x02\x00\x07");
}

#[test]
fn the_undeclarations_are_planted_on_a_plain_part_and_a_broken_part_is_its_bytes() {
    // A part that parses, with nothing of `urn:x`: the target plants a child that counts as nothing under a parent of `urn:a`, the same child with the default
    // namespace undeclared on it or on its parent, and an empty relationships part whose root's child undeclares the default namespace (the check read each such child as
    // the default namespace's in force, and found no difference in any).
    harness::check(b"\x00<f a=\"1\"><g/> t </f>\xff");
    // A relationships part of the kind whose root may hold empty children, which the dropped-part property reads by the rule's words, and one whose child undeclares the
    // default namespace (a part).
    harness::check(b"\x00<f><e b=\"2\"/></f>\xff\x04<w xmlns=\"urn:a\"><v/> </w>\xff");
    harness::check(b"\x00<f><e b=\"2\"/></f>\xff\x04<w xmlns=\"urn:a\"><v xmlns=\"\"/></w>\xff");
    // A part that breaks a constraint of XML Namespaces 1.0 is its bytes: the properties that read it written another way as the same are not asked of it, and the rest hold.
    harness::check(b"\x00<f xmlns:z=\"\"><g/></f>\xff\x00<f/>\xff\x00\x05\x20");
}

/// Self-contained fragments of the parts of a generated input: markup of `urn:x` in every place it can stand (an element by a default declaration and by a prefix, an
/// attribute, a declaration nothing uses), beside what every other rule of the target reads; and children that undeclare the default namespace where a rule reads
/// their names, a declaration XML Namespaces 1.0 does not allow, and one attribute under two prefixes.
const FRAGMENTS: [&str; 24] = [
    "<r xmlns=\"urn:x\"/>",
    "<q:n xmlns:q=\"urn:x\" a=\"1\"/>",
    "<e xmlns:q=\"urn:x\" q:a=\"1\" b=\"2\"/>",
    "<e xmlns:q=\"urn:x\"/>",
    "<e b=\"2\"/>",
    "<Gen/>",
    " ",
    "t",
    "<Autos><Def name=\"d\" fam=\"a\"><P v=\"1\"/></Def></Autos>",
    "<I ref=\"d\"/>",
    "<Empty/>",
    "<Cell N=\"a\" V=\"1.0\"/>",
    "<Section N=\"Geometry\"><Row/><Row IX=\"1\"/></Section>",
    "<?p d?>",
    "<!-- c -->",
    "<X:- Q=\"1\"><r xmlns=\"urn:x\"/></X:->",
    "<Text> </Text>",
    "<p xmlns=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"> a <span xmlns:q=\"urn:x\" q:m=\"1\">b</span></p>",
    "<m xmlns=\"urn:a\"><g xmlns=\"\"/></m>",
    "<m xmlns=\"urn:a\"><g/></m>",
    "<v/>",
    "<v xmlns=\"\"/>",
    "<b xmlns:z=\"\"/>",
    "<k xmlns:p=\"urn:a\" xmlns:q=\"urn:a\" p:k=\"1\" q:k=\"2\"/>",
];
const ROOTS: [(&str, &str); 5] =
    [("<f>", "</f>"), ("<Shape>", "</Shape>"), ("<f xmlns:q=\"urn:x\" q:r=\"1\">", "</f>"), ("<Empty>", "</Empty>"), ("<w xmlns=\"urn:a\">", "</w>")];

/// A generated input of the target: up to four parts, each a root and fragments (some nested in a shape), as XML, as a relationships part or as bytes, and the edits.
fn input() -> impl Strategy<Value = Vec<u8>> {
    let one = (0u8..3, 0..ROOTS.len(), prop::collection::vec(0..FRAGMENTS.len(), 0..6), prop::collection::vec(0..FRAGMENTS.len(), 0..3));
    (prop::collection::vec(one, 1..5), prop::collection::vec(any::<u8>(), 0..18)).prop_map(|(parts, edits)| {
        let mut data = Vec::new();
        for (kind, root, outer, inner) in parts {
            let (open, close) = ROOTS[root];
            let mut xml = open.to_owned();
            outer.iter().for_each(|&f| xml.push_str(FRAGMENTS[f]));
            if !inner.is_empty() {
                xml.push_str("<Shape>");
                inner.iter().for_each(|&f| xml.push_str(FRAGMENTS[f]));
                xml.push_str("</Shape>");
            }
            xml.push_str(close);
            data.push([0x00, 0x04, 0x01][usize::from(kind)]);
            data.extend_from_slice(xml.as_bytes());
            data.push(0xFF);
        }
        data.extend(edits);
        data
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, ..ProptestConfig::default() })]

    /// The target's law holds on generated inputs that hold the markup of `urn:x` in every place, which the fuzzer reached only after millions of runs.
    #[test]
    fn the_targets_law_holds_on_generated_packages(data in input()) {
        harness::check(&data);
    }
}
