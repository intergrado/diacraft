//! In the container's content check (second round): attributes and elements of a namespace that count as nothing when only the second document has them
//! (`Rules::ignored_when_extra`), copies of one definition that count as one, and references from one definition to another that are resolved, bounded, a circle or a chain too long
//! compared as written (`FreeNames::definition_references`, with `Takes` of more than one kind and lists of names). The vocabulary is this file's own: the ODF lists are in
//! `crates/odg/tests/it/canon_extra_carrier.rs`. The check is oriented by the first rule, so no test here asks for the symmetric count of a document that has such markup.

use diacraft_container::canon::{
    ContentDiff, Definition, Entry, Found, FreeNames, KeyFold, Keyed, MAX_CHAIN, Name, Reference, Rules, SAME_FAMILY, Takes, compare_entries,
};
use diacraft_container::{Work, XmlLimits};

const Q: &str = "urn:q";
const fn n(local: &'static str) -> Name {
    Name { ns: "", local }
}

const DEF_A: &[Takes] = &[Takes { element: n("Def"), family: "a" }];
const SAME: &[Takes] = &[Takes { element: n("Def"), family: SAME_FAMILY }];
const LIST: &[Takes] = &[Takes { element: n("Lst"), family: "" }];
/// Two kinds that share one family of names.
const FORMATS: &[Takes] = &[Takes { element: n("Dat"), family: "" }, Takes { element: n("Tim"), family: "" }];

const FREE: FreeNames = FreeNames {
    container: n("Autos"),
    definitions: &[
        Definition { element: n("Def"), family: Some(n("fam")) },
        Definition { element: n("Lst"), family: None },
        Definition { element: n("Dat"), family: None },
        Definition { element: n("Tim"), family: None },
    ],
    name: n("name"),
    references: &[
        Reference { attr: n("ref"), on: Some(n("Item")), takes: DEF_A, list: false },
        Reference { attr: n("refs"), on: Some(n("Item")), takes: DEF_A, list: true },
        Reference { attr: n("fmt"), on: Some(n("Item")), takes: FORMATS, list: false },
    ],
    definition_references: &[
        Reference { attr: n("uses"), on: Some(n("Def")), takes: SAME, list: false },
        Reference { attr: n("lst"), on: Some(n("Def")), takes: LIST, list: false },
        Reference { attr: n("deep"), on: Some(n("Lvl")), takes: DEF_A, list: false },
    ],
    scopes: &[],
};

const RULES: Rules = Rules {
    keyed: &[Keyed { child: n("Def"), key: &[n("fam"), n("name")], fold: KeyFold::Exact }],
    ignored_when_extra: &[Q],
    free_names: &[FREE],
    ..Rules::NONE
};

fn compare(rules: &Rules, source: &str, written: &str) -> (ContentDiff, Work) {
    let a = [Entry { name: "/p.xml", xml: true, bytes: source.as_bytes() }];
    let b = [Entry { name: "/p.xml", xml: true, bytes: written.as_bytes() }];
    let mut work = Work::default();
    let diff = compare_entries(&a, &b, false, rules, &XmlLimits { max_nodes: 4_000_000, ..XmlLimits::default() }, &mut work).unwrap();
    (diff, work)
}

#[track_caller]
fn nothing(source: &str, written: &str) {
    let (d, _) = compare(&RULES, source, written);
    assert!(d.same(), "{source} against {written}: {:?}", d.shown);
}

#[track_caller]
fn found(source: &str, written: &str) -> ContentDiff {
    let (d, _) = compare(&RULES, source, written);
    assert!(!d.same(), "{source} against {written}: called the same");
    d
}

fn doc(autos: &str, body: &str) -> String {
    format!("<Doc xmlns:q=\"{Q}\"><Autos>{autos}</Autos><Body>{body}</Body></Doc>")
}

fn def(name: &str, family: &str, value: &str, attrs: &str) -> String {
    format!("<Def name=\"{name}\" fam=\"{family}\" {attrs}><P v=\"{value}\"/></Def>")
}

fn item(name: &str) -> String {
    format!("<Item ref=\"{name}\"/>")
}

// ------------------------------------------------------------------------------------------------------------------- what counts as nothing when extra

#[test]
fn an_attribute_of_the_namespace_counts_as_nothing_when_only_the_second_document_has_it() {
    let plain = "<r><e a=\"1\"/></r>";
    let marked = format!("<r xmlns:q=\"{Q}\"><e a=\"1\" q:x=\"1\" q:y=\"2\"/></r>");
    nothing(plain, &marked);
    // The first's own must be there, with its value.
    let lost = found(&marked, plain);
    assert_eq!((lost.count, lost.shown[0].found), (2, Found::Attribute));
    nothing(&marked, &marked);
    let other = format!("<r xmlns:q=\"{Q}\"><e a=\"1\" q:x=\"2\" q:y=\"2\"/></r>");
    assert_eq!(found(&marked, &other).shown[0].found, Found::Value);
    // Beside a change it hides none.
    let changed = format!("<r xmlns:q=\"{Q}\"><e a=\"2\" q:x=\"1\"/></r>");
    assert_eq!(found(plain, &changed).count, 1);
    // Another namespace is no such markup, and the same local name in no namespace is none.
    let foreign = "<r xmlns:o=\"urn:o\"><e a=\"1\" o:x=\"1\"/></r>";
    assert_eq!(found(plain, foreign).shown[0].found, Found::Attribute);
    assert_eq!(found(plain, "<r><e a=\"1\" x=\"1\"/></r>").shown[0].found, Found::Attribute);
    // The same with the rule left out: the count is symmetric and the markup counts.
    let (off, _) = compare(&Rules::NONE, plain, &marked);
    assert_eq!(off.count, 2);
}

#[test]
fn an_element_of_the_namespace_counts_as_nothing_when_only_the_second_document_has_it() {
    let plain = "<r><e/></r>";
    let note = |inner: &str| format!("<r xmlns:q=\"{Q}\"><e>{inner}</e></r>");
    nothing(plain, &note("<q:n a=\"1\"/>"));
    assert_eq!(found(&note("<q:n a=\"1\"/>"), plain).count, 1);
    nothing(&note("<q:n a=\"1\"/>"), &note("<q:n a=\"1\"/>"));
    assert_eq!(found(&note("<q:n a=\"1\"/>"), &note("<q:n a=\"2\"/>")).count, 1);
    // Two of the source's: both in the written file among more, one lost, one changed; copies of one element are paired in order.
    let two = note("<q:n/><q:m/>");
    nothing(&two, &note("<q:m/><q:n/><q:o/>"));
    assert_eq!(found(&two, &note("<q:m/>")).count, 1);
    nothing(&note("<q:n/><q:n/>"), &note("<q:n/><q:n/><q:n/>"));
    assert_eq!(found(&note("<q:n/><q:n/>"), &note("<q:n/>")).count, 1);
    // It sits among the keyed and the ordered children without moving them.
    let ordered = |inner: &str| format!("<r xmlns:q=\"{Q}\"><a/>{inner}<b/></r>");
    nothing(&ordered(""), &ordered("<q:n/>"));
}

#[test]
fn what_the_second_document_marked_does_not_move_the_children_off_their_places() {
    let shapes = |marks: bool, dropped: Option<usize>| -> String {
        let body: String = (0..8)
            .filter(|i| Some(*i) != dropped)
            .map(|i| if marks { format!("<s id=\"{i}\" q:id=\"{i}\"/>") } else { format!("<s id=\"{i}\"/>") })
            .collect();
        format!("<r xmlns:q=\"{Q}\">{body}</r>")
    };
    // Every child of the written file is marked and the fourth is taken out: one difference, and not a shift of all that follows.
    assert_eq!(found(&shapes(false, None), &shapes(true, Some(3))).count, 1);
    nothing(&shapes(false, None), &shapes(true, None));
}

#[test]
fn a_many_equal_elements_of_the_namespace_cost_a_pass_and_collisions_cost_no_more() {
    let many = |n: usize, unlike: bool| {
        let body: String = (0..n).map(|i| if unlike { format!("<q:n a=\"{i}\"/>") } else { "<q:n a=\"1\"/>".to_owned() }).collect();
        format!("<r xmlns:q=\"{Q}\"><e>{body}</e></r>")
    };
    let (small, large) = (compare(&RULES, &many(1_000, false), &many(1_000, false)).1, compare(&RULES, &many(20_000, false), &many(20_000, false)).1);
    assert!(large.canon_steps < small.canon_steps.saturating_mul(40), "{} against {}", large.canon_steps, small.canon_steps);
    // Different ones, in another order: each of the source's has its match.
    let n = 5_000;
    let reversed: String = (0..n).rev().map(|i| format!("<q:n a=\"{i}\"/>")).collect();
    nothing(&many(n, true), &format!("<r xmlns:q=\"{Q}\"><e>{reversed}</e></r>"));
    let (_, work) = compare(&RULES, &many(n, true), &format!("<r xmlns:q=\"{Q}\"><e>{reversed}</e></r>"));
    assert!(work.canon_steps < (n as u64) * 60, "{} steps for {n} elements", work.canon_steps);
}

// ------------------------------------------------------------------------------------------------------------------------------ copies

#[test]
fn copies_of_one_definition_count_as_one_and_a_reference_follows_the_content() {
    let a = doc(&def("x", "a", "1", ""), &format!("{}{}{}", item("x"), item("x"), item("x")));
    let b = doc(
        &format!("{}{}{}", def("p", "a", "1", ""), def("q", "a", "1", ""), def("r", "a", "1", "")),
        &format!("{}{}{}", item("p"), item("q"), item("r")),
    );
    nothing(&a, &b);
    nothing(&b, &a);
    // A copy that differs is a definition of its own, and a reference to it is another.
    let c = doc(&format!("{}{}", def("p", "a", "1", ""), def("q", "a", "2", "")), &format!("{}{}", item("p"), item("q")));
    assert!(!compare(&RULES, &a, &c).0.same());
    // Copies with no match count as one, and not each.
    let none = doc(&format!("{}{}", def("p", "a", "7", ""), def("q", "a", "7", "")), "");
    let other = doc(&def("z", "a", "8", ""), "");
    assert_eq!(found(&none, &other).count, 2, "the one style of the first is missing, the one of the second is extra");
    // The markup a writer adds to a copy is no difference, and a copy it left plain is the same one.
    let marked = doc(&format!("{}{}", def("x", "a", "1", ""), def("y", "a", "1", "q:id=\"2\"")), &format!("{}{}", item("x"), item("y")));
    nothing(&doc(&def("x", "a", "1", ""), &format!("{}{}", item("x"), item("x"))), &marked);
}

// -------------------------------------------------------------------------------------------------------------------- references between definitions

fn chained(prefix: &str, n: usize) -> String {
    // `Def` i uses `Def` i + 1 of its own family, the last uses a name no definition has.
    let autos: String = (0..n)
        .map(|i| {
            let next = if i + 1 < n { format!("{prefix}{}", i + 1) } else { "Common".to_owned() };
            def(&format!("{prefix}{i}"), "a", "v", &format!("uses=\"{next}\""))
        })
        .collect();
    doc(&autos, &item(&format!("{prefix}0")))
}

#[test]
fn a_definition_that_names_another_follows_the_content_of_what_it_names() {
    // Renamed all the way down, with the one at the end naming a name that is no definition's, as written.
    nothing(&chained("a", 6), &chained("b", 6));
    nothing(&chained("b", 6), &chained("a", 6));
    // The one at the end differs: every one before it does.
    let changed = chained("b", 6).replace("<P v=\"v\"/></Def></Autos>", "<P v=\"w\"/></Def></Autos>");
    assert!(!compare(&RULES, &chained("a", 6), &changed).0.same());
    // A name that no definition has is as written: another name there is another content.
    let common = |name: &str| doc(&def("x", "a", "1", &format!("uses=\"{name}\"")), &item("x"));
    nothing(&common("Common"), &common("Common"));
    assert!(!compare(&RULES, &common("Common"), &common("Other")).0.same());
    // A family: a style asks for a style of its own family, so a definition of another family of the name is not the one named.
    let family = |named: &str| doc(&format!("{}{}", def("x", "a", "1", &format!("uses=\"{named}\"")), def(named, "b", "2", "")), "");
    assert!(!compare(&RULES, &family("y"), &family("z")).0.same(), "the name is of a definition of another family: as written");
    let own = |named: &str| doc(&format!("{}{}", def("x", "a", "1", &format!("uses=\"{named}\"")), def(named, "a", "2", "")), "");
    nothing(&own("y"), &own("z"));
    // Of another kind and in another place: a reference from inside a definition's child.
    let level =
        |lvl: &str, target: &str| doc(&format!("<Def name=\"x\" fam=\"b\"><Lvl deep=\"{lvl}\"/></Def>{}", def(target, "a", "t", "")), &item("x"));
    nothing(&level("t1", "t1"), &level("t7", "t7"));
    assert!(!compare(&RULES, &level("t1", "t1"), &level("t1", "t7")).0.same());
}

#[test]
fn a_reference_that_asks_for_more_than_one_kind_reads_the_one_that_has_the_name_and_a_list_reads_each_name() {
    let formats = |dat: &str, tim: &str| {
        doc(
            &format!("<Dat name=\"{dat}\"><P v=\"d\"/></Dat><Tim name=\"{tim}\"><P v=\"t\"/></Tim>"),
            &format!("<Item fmt=\"{dat}\"/><Item fmt=\"{tim}\"/>"),
        )
    };
    nothing(&formats("D1", "T1"), &formats("D9", "T8"));
    // A name both kinds hold is not told: compared as written.
    let both = |name: &str| {
        doc(&format!("<Dat name=\"{name}\"><P v=\"d\"/></Dat><Tim name=\"{name}\"><P v=\"t\"/></Tim>"), &format!("<Item fmt=\"{name}\"/>"))
    };
    assert!(!compare(&RULES, &both("N1"), &both("N2")).0.same());
    let list = |a: &str, b: &str| doc(&format!("{}{}", def(a, "a", "1", ""), def(b, "a", "2", "")), &format!("<Item refs=\"{a} {b} Common\"/>"));
    nothing(&list("x", "y"), &list("p", "q"));
    let order = |l: &str| doc(&format!("{}{}", def("x", "a", "1", ""), def("y", "a", "2", "")), &format!("<Item refs=\"{l}\"/>"));
    assert!(!compare(&RULES, &order("x y"), &order("y x")).0.same(), "the list is in order");
}

#[test]
fn a_circle_of_names_and_a_chain_that_is_too_long_are_compared_as_written() {
    let circle =
        |a: &str, b: &str| doc(&format!("{}{}", def(a, "a", "1", &format!("uses=\"{b}\"")), def(b, "a", "1", &format!("uses=\"{a}\""))), &item(a));
    nothing(&circle("x", "y"), &circle("x", "y"));
    assert!(!compare(&RULES, &circle("x", "y"), &circle("p", "q")).0.same(), "a circle is compared as written");
    // A definition that is not in the circle, and a self reference.
    let beside = |free: &str| doc(&format!("{}{}", def("x", "a", "1", "uses=\"x\""), def(free, "a", "5", "")), &item(free));
    nothing(&beside("p"), &beside("q"));
    // A chain of more than MAX_CHAIN names is compared as written; one of MAX_CHAIN names is resolved.
    nothing(&chained("a", MAX_CHAIN), &chained("b", MAX_CHAIN));
    assert!(!compare(&RULES, &chained("a", MAX_CHAIN * 2), &chained("b", MAX_CHAIN * 2)).0.same());
}

#[test]
fn a_chain_ten_thousand_deep_a_circle_of_them_and_a_hundred_thousand_copies_cost_a_few_passes() {
    // The yardstick is the check with names left as written, on the same pair.
    let as_written = Rules { free_names: &[], ..RULES };
    let ratio = |a: &str, b: &str| {
        let (_, now) = compare(&RULES, a, b);
        let (_, before) = compare(&as_written, a, b);
        (now.canon_steps as f64 / before.canon_steps as f64, now.canon_bytes as f64 / before.canon_bytes as f64)
    };
    let circle = |prefix: &str, n: usize| {
        let autos: String = (0..n).map(|i| def(&format!("{prefix}{i}"), "a", "v", &format!("uses=\"{prefix}{}\"", (i + 1) % n))).collect();
        doc(&autos, &item(&format!("{prefix}0")))
    };
    let copies = |prefix: &str| doc(&(0..100_000).map(|i| def(&format!("{prefix}{i}"), "a", "v", "")).collect::<String>(), "");
    for (what, a, b) in [
        ("a chain of 10,000", chained("a", 10_000), chained("b", 10_000)),
        ("a circle of 10,000", circle("a", 10_000), circle("b", 10_000)),
        ("100,000 copies of one definition", copies("a"), copies("b")),
    ] {
        let (steps, bytes) = ratio(&a, &b);
        println!("MEASURED {what}: steps {steps:.3} bytes {bytes:.3} times the check with the names as written");
        assert!(steps <= 4.0 && bytes <= 4.0, "{what}: {steps} times the steps, {bytes} times the bytes");
    }
    // The copies are one, the chain and the circle are as written.
    assert!(compare(&RULES, &copies("a"), &copies("b")).0.same());
    assert!(!compare(&RULES, &chained("a", 10_000), &chained("b", 10_000)).0.same());
    assert!(!compare(&RULES, &circle("a", 10_000), &circle("b", 10_000)).0.same());
}
