//! The rule of empty containers (`Rules::absent_when_empty`): a listed element that holds nothing is the same as an absent one, and only when every
//! copy of its name among the siblings is empty. Every rule that lets two spellings pass is paired with a planted difference that it still catches, as in
//! `canon.rs`. The lists are this file's own: the formats' are tested in their crates. The same rule under one parent only (`Rules::absent_when_empty_under`)
//! is at the end.

use diacraft_container::canon::{ContentDiff, EmptyUnder, Entry, Found, KeyFold, Keyed, Name, ODF_TEXT_NAMESPACE, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};

const E: Name = Name { ns: "urn:e", local: "e" };
const F: Name = Name { ns: "", local: "f" };
const K: Name = Name { ns: "", local: "k" };
const KEY: &[Name] = &[Name { ns: "", local: "n" }];

/// `e` of `urn:e` and `f` in no namespace are listed; `k` is listed and keyed; `text:p` is a paragraph.
const RULES: Rules = Rules {
    keyed: &[Keyed { child: K, key: KEY, fold: KeyFold::Exact }],
    paragraphs: &[Name { ns: ODF_TEXT_NAMESPACE, local: "p" }],
    absent_when_empty: &[E, F, K],
    ..Rules::NONE
};

/// A document element that binds `e:` to `urn:e`, around `inner`.
fn doc(inner: &str) -> String {
    format!("<r xmlns:e=\"urn:e\" xmlns:t=\"{ODF_TEXT_NAMESPACE}\">{inner}</r>")
}

fn run(a: &str, b: &str, rules: &Rules, work: &mut Work) -> ContentDiff {
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    compare_entries(&ea, &eb, true, rules, &XmlLimits::default(), work).unwrap()
}

/// The comparison of two documents' insides, in both directions: the count is symmetric.
fn diff_in(rules: &Rules, a: &str, b: &str) -> ContentDiff {
    let (ab, ba) = (run(&doc(a), &doc(b), rules, &mut Work::default()), run(&doc(b), &doc(a), rules, &mut Work::default()));
    assert_eq!(ab.count, ba.count, "the count is symmetric: {a} against {b}");
    ab
}

#[track_caller]
fn same(a: &str, b: &str) {
    let d = diff_in(&RULES, a, b);
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

/// One difference, of kind `found`, at `path`, and no other.
#[track_caller]
fn only(a: &str, b: &str, found: Found, path: &str) {
    let d = diff_in(&RULES, a, b);
    assert_eq!(d.count, 1, "{a} against {b}: {:?}", d.shown);
    let first = d.shown.first().map(|x| (x.found, x.path.as_str()));
    assert_eq!(first, Some((found, path)), "{a} against {b}");
}

// ------------------------------------------------------------------------------------------------------------ empty is absent

#[test]
fn a_listed_child_that_holds_nothing_is_the_same_as_none_in_both_orders() {
    let spellings = [
        "<e:e/>",
        "<e:e></e:e>",
        "<e:e> </e:e>",
        "<e:e>\r\n\t \n</e:e>",
        "<e:e><!-- nothing --></e:e>",
        "<e:e> <!-- c --> </e:e>",
        "<e:e><![CDATA[]]></e:e>",
        "<e:e><![CDATA[ \n ]]></e:e>",
        "<e:e>&#32;&#10;</e:e>",
        r#"<e:e xmlns:z="urn:z"/>"#,
        r#"<e:e xmlns="urn:z"/>"#,
        // Another prefix, and the namespace as the default one: the name is the namespace and the local part.
        r#"<z:e xmlns:z="urn:e"/>"#,
        r#"<e xmlns="urn:e"/>"#,
        // `f` is in no namespace.
        "<f/>",
        "<f></f>",
    ];
    for spelling in spellings {
        same(spelling, "");
        same(&format!("<a/>{spelling}<b/>"), "<a/><b/>");
        same(&format!("<a>x</a>{spelling}"), "<a>x</a>");
        // Beside the spacing between children.
        same(&format!(" {spelling} <a/> "), "<a/>");
        // Two of them, and an empty one of each name.
        same(&format!("{spelling}{spelling}"), "");
        same(&format!("{spelling}<f/>"), "");
    }
}

#[test]
fn an_empty_listed_child_is_the_same_as_another_empty_one_and_as_itself() {
    same("<e:e/>", "<e:e></e:e>");
    same("<e:e/>", "<f/>");
    same("<e:e/>", "<e:e/>");
    // Deeper in the tree too: the rule is for any parent.
    same("<a><b><e:e/></b></a>", "<a><b/></a>");
}

#[test]
fn an_unlisted_empty_child_is_kept() {
    only("<x/>", "", Found::Element, "/r[0]/x[0]");
    only("<e:x/>", "", Found::Element, "/r[0]/x[0]");
    // The name is the namespace and the local part: `e` of another namespace, `E` and `ee` are other names.
    only(r#"<z:e xmlns:z="urn:other"/>"#, "", Found::Element, "/r[0]/e[0]");
    only("<e:E/>", "", Found::Element, "/r[0]/E[0]");
    only("<e:ee/>", "", Found::Element, "/r[0]/ee[0]");
    only("<e/>", "", Found::Element, "/r[0]/e[0]");
    // `f` of a namespace is not `f` of none.
    only("<e:f/>", "", Found::Element, "/r[0]/f[0]");
}

#[test]
fn the_rule_is_off_for_a_format_that_lists_none() {
    let d = diff_in(&Rules::NONE, "<e:e/>", "");
    assert_eq!((d.count, d.shown.first().map(|x| x.found)), (1, Some(Found::Element)));
    // And for one that lists another name.
    let other = Rules { absent_when_empty: &[Name { ns: "urn:e", local: "other" }], ..Rules::NONE };
    assert_eq!(diff_in(&other, "<e:e/>", "").count, 1);
    assert!(diff_in(&other, "<e:other/>", "").same());
}

// --------------------------------------------------------------------------------------------------- the controls: what holds anything is kept

#[test]
fn a_listed_child_that_holds_anything_is_kept() {
    let holding = [
        r#"<e:e a="1"/>"#,
        r#"<e:e a=""/>"#,
        r#"<e:e xml:space="preserve"/>"#,
        r#"<e:e z:a="1" xmlns:z="urn:z"/>"#,
        "<e:e><x/></e:e>",
        "<e:e><e:e/></e:e>",
        "<e:e><?go now?></e:e>",
        "<e:e><?go?></e:e>",
        "<e:e>x</e:e>",
        "<e:e>  x\n</e:e>",
        "<e:e>&#160;</e:e>",
        "<e:e><![CDATA[x]]></e:e>",
        "<e:e> <!-- c -->x</e:e>",
        r#"<f a="1"/>"#,
        "<f>x</f>",
    ];
    for one in holding {
        let name = if one.starts_with("<f") { "f" } else { "e" };
        let path = format!("/r[0]/{name}[0]");
        // Against none, and against an empty one: either way the one that holds something is an element on one side only.
        only(one, "", Found::Element, &path);
        only(one, "<e:e/>", Found::Element, &path);
        only(&format!("<a/>{one}<b/>"), "<a/><b/>", Found::Element, &format!("/r[0]/{name}[1]"));
    }
}

#[test]
fn a_listed_child_that_holds_something_is_compared_as_every_element_is() {
    only(r#"<e:e a="1"/>"#, r#"<e:e a="2"/>"#, Found::Value, "/r[0]/e[0]/@a");
    only(r#"<e:e a="1" c="x"/>"#, r#"<e:e a="1"/>"#, Found::Attribute, "/r[0]/e[0]/@c");
    only("<e:e>x</e:e>", "<e:e>y</e:e>", Found::Text, "/r[0]/e[0]/#text[0]");
    same(r#"<e:e a="1"/>"#, r#"<e:e a="1"></e:e>"#);
    // The rule leaves what stands beside it alone: a changed sibling is one difference, whether or not an empty one stands on either side.
    only(r#"<e:e/><a x="1"/>"#, r#"<a x="2"/>"#, Found::Value, "/r[0]/a[0]/@x");
    only(r#"<a x="1"/><e:e/>"#, r#"<e:e/><a x="2"/>"#, Found::Value, "/r[0]/a[0]/@x");
}

#[test]
fn what_is_in_an_empty_container_is_not_empty_even_if_that_is_left_out() {
    // Read on the tree as written: a container that holds an empty container holds a child element, so it is not empty. The inner one is left out of it.
    only("<e:e><e:e/></e:e>", "", Found::Element, "/r[0]/e[0]");
    same("<e:e><e:e/></e:e>", "<e:e><f/></e:e>");
    only("<e:e><e:e/></e:e>", "<e:e/>", Found::Element, "/r[0]/e[0]");
}

// -------------------------------------------------------------------------------------------------------------------- a name written twice

#[test]
fn a_name_written_twice_is_left_out_only_when_every_copy_of_it_is_empty() {
    // All empty: all left out.
    same("<e:e/><e:e/><e:e/>", "");
    // One that holds something beside an empty one: the empty one stays, before it and after it.
    only("<e:e/><e:e>x</e:e>", "<e:e>x</e:e>", Found::Element, "/r[0]/e[0]");
    only("<e:e>x</e:e><e:e/>", "<e:e>x</e:e>", Found::Element, "/r[0]/e[1]");
    // An empty one on each side of the full one: the empty ones are not left out, so three copies against one.
    assert_eq!(diff_in(&RULES, "<e:e/><e:e>x</e:e><e:e/>", "<e:e>x</e:e>").count, 3);
    // Each name on its own: an empty `e` beside a full `f` is left out, and an empty `f` beside a full `f` is not.
    same("<e:e/><f>x</f>", "<f>x</f>");
    only("<e:e/><f>x</f><f/>", "<f>x</f>", Found::Element, "/r[0]/f[1]");
    // The copies need not be neighbours.
    only("<e:e/><a/><e:e>x</e:e>", "<a/><e:e>x</e:e>", Found::Element, "/r[0]/e[0]");
    // Another parent's copy has nothing to do with it: the full one stands in another element.
    same("<a><e:e>x</e:e></a><e:e/>", "<a><e:e>x</e:e></a>");
}

// --------------------------------------------------------------------------------------------------------------------- where it does not apply

#[test]
fn a_keyed_listed_element_is_never_left_out() {
    // `k` is listed and keyed: the set of keyed siblings is as it was.
    only("<k/>", "", Found::Element, "/r[0]/k#0");
    only(r#"<k n="1"/>"#, "", Found::Element, "/r[0]/k#0");
    same(r#"<k n="1"/><k n="2"/>"#, r#"<k n="2"/><k n="1"/>"#);
    // What stands beside a keyed one is left alone too: the keyed one is not an item that the rule can take out, whatever it is beside.
    only("<k/><a/>", "<a/>", Found::Element, "/r[0]/k#0");
    only("<a/><k/><b/>", "<a/><b/>", Found::Element, "/r[0]/k#0");
    only("<k/><e:e/><a/>", "<a/>", Found::Element, "/r[0]/k#0");
}

#[test]
fn an_empty_listed_element_in_a_paragraph_is_kept() {
    let (with, without) = ("<t:p>a<e:e/>b</t:p>", "<t:p>ab</t:p>");
    let d = diff_in(&RULES, with, without);
    assert!(!d.same(), "{:?}", d.shown);
    same(with, with);
    // As a paragraph with a mark in it, and not as one without.
    assert!(!diff_in(&RULES, "<t:p>a <e:e/></t:p>", "<t:p>a</t:p>").same());
    // However deep inside the paragraph: an element that is no paragraph element (a link) holds the empty one, and it is still content there.
    let deeper = diff_in(&RULES, "<t:p><x><e:e/></x></t:p>", "<t:p><x/></t:p>");
    assert_eq!((deeper.count, deeper.shown.first().map(|d| d.found)), (1, Some(Found::Element)), "{:?}", deeper.shown);
    let deepest = diff_in(&RULES, "<t:p><x><y><e:e/><f/></y></x></t:p>", "<t:p><x><y/></x></t:p>");
    assert_eq!(deepest.count, 2, "{:?}", deepest.shown);
    // Beside the paragraph, outside it, the rule is as everywhere.
    same("<t:p><x/></t:p><e:e/>", "<t:p><x/></t:p>");
    same("<a><e:e/></a><t:p>x</t:p>", "<a/><t:p>x</t:p>");
}

// ------------------------------------------------------------------------------------------------------------------------- the spacing

#[test]
fn the_spacing_around_a_left_out_child_is_read_while_the_child_stands() {
    // An element that has a child has no text of blank spacing: `<r> <e/> </r>` is `<r/>`, and is not `<r> </r>`, which holds a text of white space.
    same(" <e:e/> ", "");
    same("\n <e:e/>\n <e:e/>\n", "");
    only(" <e:e/> ", " ", Found::Text, "/r[0]/#text[0]");
    only("\n", "\n <e:e/>\n", Found::Text, "/r[0]/#text[0]");
    // The spacing beside other children is dropped as it always was.
    same(" <e:e/> <a/> ", "<a/>");
    same(" <a/> <e:e/> ", "<a/>");
    // Text that is not blank is content, and is the same either side of a left-out child only when it is joined: the texts are two.
    same("x<e:e/>", "x");
    assert!(!diff_in(&RULES, "x<e:e/>y", "xy").same(), "two runs of text are not one");
    only("x<e:e/>y", "x<a/>y", Found::Element, "/r[0]/a[1]");
}

// ------------------------------------------------------------------------------------------------------------------------------- counts pinned

/// What a comparison of two documents' insides that are the same cost, as the check counts it: its steps and its bytes.
fn cost(a: &str, b: &str) -> (u64, u64) {
    let mut work = Work::default();
    let d = run(&doc(a), &doc(b), &RULES, &mut work);
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
    (work.canon_steps, work.canon_bytes)
}

#[test]
fn the_counts_of_small_comparisons_are_pinned() {
    // Exact counts, so that no statement that counts the work can be taken out unseen: an upper bound alone lets a mutant that counts less pass. The first
    // row has nothing left out, the others each look at what an empty element holds (a step for it, a step for each attribute and child it looks at, and the
    // bytes of its text), and a pass over the children once one is left out.
    let got = [
        cost("<a/>", " <a/> "),
        cost("<e:e/>", ""),
        cost("<e:e> </e:e>", ""),
        cost("<e:e>  \n <!-- c --> </e:e>", ""),
        cost(r#"<e:e xmlns:z="urn:z" xmlns="urn:y"/>"#, ""),
        cost("<a/><e:e/><b/><f/>", "<a/><b/>"),
        cost("<e:e/><e:e/><f/>", ""),
        cost("<e:e>x</e:e>", " <e:e>x</e:e> "),
        cost("<e:e><e:e/></e:e>", "<e:e><f/></e:e>"),
        cost(r#"<e:e a="1" b="2" c="3"/>"#, r#"<e:e c="3" b="2" a="1"/>"#),
        cost("<e:e><x/><x/><x/></e:e>", " <e:e><x/><x/><x/></e:e> "),
        cost("<e:e>x<!-- c -->y<x/></e:e>", "<e:e>xy<x/></e:e>"),
        cost("<e:e><?go now?><x/></e:e>", "<e:e><?go now?><x/></e:e>\n"),
    ];
    println!("PINNED {got:?}");
    assert_eq!(got, PINNED);
}

/// A child kept, a child left out, white space in one, white space and a comment, two declarations, one left out beside others and one that is not, three left out, one
/// that holds something, one that holds one that is left out, one that has three attributes (the look stops at the first), one that has three children (at the first), one that
/// starts with text that is not blank (at it), one that starts with an instruction.
const PINNED: [(u64, u64); 13] =
    [(12, 6), (9, 6), (10, 8), (12, 16), (11, 6), (26, 6), (15, 6), (17, 11), (22, 6), (30, 15), (34, 6), (23, 15), (23, 9)];

// ------------------------------------------------------------------------------------------------------------------------------- cost

/// How many steps the comparison counted for these two documents, which are the same.
fn steps(a: &str, b: &str) -> u64 {
    let mut work = Work::default();
    let d = run(a, b, &RULES, &mut work);
    assert!(d.same(), "{:?}", d.shown);
    work.canon_steps
}

#[test]
fn a_part_of_many_empty_containers_costs_a_pass_over_them() {
    let nothing = doc("");
    // An element with `n` empty children costs a few steps for each, and doubling them doubles the cost: no search of the list for each.
    let flood = |n: usize| doc(&"<e:e/>".repeat(n));
    let (small, large) = (steps(&flood(20_000), &nothing), steps(&flood(40_000), &nothing));
    assert!(small <= 12 * 20_000, "{small} steps for 20,000 empty children");
    assert!(large * 10 <= small * 21, "doubling the children ({small} to {large} steps) does not do more than double the work");
    // Two names, one of them with a full copy that keeps its empty ones: the empty copies of the other are left out, the first's are kept, and each is a step.
    let mixed = |n: usize| doc(&format!("<e:e>x</e:e>{}{}", "<e:e/>".repeat(n), "<f/>".repeat(n)));
    let (small, large) = (steps(&mixed(10_000), &mixed(10_000)), steps(&mixed(20_000), &mixed(20_000)));
    assert!(small <= 24 * 10_000, "{small} steps for 10,000 kept and 10,000 left out");
    assert!(large * 10 <= small * 21, "doubling the children ({small} to {large} steps) does not do more than double the work");
    // Many names that hold something: the names kept in mind are those of the list, never the children.
    let full = |n: usize| doc(&(0..n).map(|i| format!("<e:e a=\"{i}\"/>")).collect::<String>());
    let (small, large) = (steps(&full(10_000), &full(10_000)), steps(&full(20_000), &full(20_000)));
    assert!(large * 10 <= small * 21, "doubling the full children ({small} to {large} steps) does not do more than double the work");
}

// ------------------------------------------------------------------------------------------------------------ under one parent

/// `u` of `urn:e` is listed under `s` (no namespace) only, and `f` under `q` only; nothing is listed anywhere.
const UNDER: Rules = Rules {
    absent_when_empty_under: &[
        EmptyUnder { parent: Name { ns: "", local: "s" }, children: &[Name { ns: "urn:e", local: "u" }] },
        EmptyUnder { parent: Name { ns: "", local: "q" }, children: &[F] },
    ],
    ..Rules::NONE
};

#[track_caller]
fn same_under(a: &str, b: &str) {
    let d = diff_in(&UNDER, a, b);
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

/// One difference, an element, at `path`.
#[track_caller]
fn kept_under(a: &str, b: &str, path: &str) {
    let d = diff_in(&UNDER, a, b);
    assert_eq!(d.count, 1, "{a} against {b}: {:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| (x.found, x.path.as_str())), Some((Found::Element, path)), "{a} against {b}");
}

#[test]
fn a_child_listed_under_its_parent_that_holds_nothing_is_the_same_as_none_there() {
    for spelling in ["<e:u/>", "<e:u></e:u>", "<e:u> \n </e:u>", "<e:u><!-- c --></e:u>", r#"<z:u xmlns:z="urn:e"/>"#] {
        same_under(&format!("<s>{spelling}</s>"), "<s/>");
        same_under(&format!("<s><a/>{spelling}<b/></s>"), "<s><a/><b/></s>");
        same_under(&format!("<s>{spelling}{spelling}</s>"), "<s></s>");
    }
    same_under("<q><f/></q>", "<q/>");
    // Each parent at any depth.
    same_under("<a><s><e:u/></s></a>", "<a><s/></a>");
}

#[test]
fn the_same_child_under_any_other_parent_is_kept() {
    // Under the document element, under another parent, under a child of its parent, and under its parent's name in a namespace.
    kept_under("<e:u/>", "", "/r[0]/u[0]");
    kept_under("<q><e:u/></q>", "<q/>", "/r[0]/q[0]/u[0]");
    kept_under("<s><a><e:u/></a></s>", "<s><a/></s>", "/r[0]/s[0]/a[0]/u[0]");
    kept_under("<e:s><e:u/></e:s>", "<e:s/>", "/r[0]/s[0]/u[0]");
    // `f` is listed under `q`, not under `s`; and `u` of no namespace is not `u` of `urn:e`.
    kept_under("<s><f/></s>", "<s/>", "/r[0]/s[0]/f[0]");
    kept_under("<s><u/></s>", "<s/>", "/r[0]/s[0]/u[0]");
}

#[test]
fn a_child_listed_under_its_parent_that_holds_anything_is_kept() {
    kept_under(r#"<s><e:u a="1"/></s>"#, "<s/>", "/r[0]/s[0]/u[0]");
    kept_under("<s><e:u>x</e:u></s>", "<s/>", "/r[0]/s[0]/u[0]");
    kept_under("<s><e:u><?p q?></e:u></s>", "<s/>", "/r[0]/s[0]/u[0]");
    // An empty copy beside a full one of its name stays.
    kept_under("<s><e:u/><e:u>x</e:u></s>", "<s><e:u>x</e:u></s>", "/r[0]/s[0]/u[0]");
    // Inside a paragraph the rule is off.
    let para = Rules { paragraphs: &[Name { ns: ODF_TEXT_NAMESPACE, local: "p" }], ..UNDER };
    let d = diff_in(&para, "<t:p><s><e:u/></s></t:p>", "<t:p><s/></t:p>");
    assert_eq!(d.count, 1, "{:?}", d.shown);
}
