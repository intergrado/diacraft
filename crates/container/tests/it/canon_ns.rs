//! Namespaces where a rule of the content check reads a name: every place resolves a name as XML Namespaces 1.0 does, `xmlns=""` included, and a part that
//! breaks a constraint of XML Namespaces 1.0 is compared as bytes, as one that does not parse is. Each rule that lets two spellings pass is paired with a planted difference it
//! still catches. The lists are this file's own.
//!
//! The false sames found (its concern 1): a child that undeclares the default namespace (`xmlns=""`) was read, by the rules that read a child's name through its own
//! declarations (the children that count as nothing, the children of an empty part's root, the container of the free names), as if it had no declaration at all, and so in
//! the default namespace in force where it stands.

use diacraft_container::canon::{
    ContentDiff, Definition, EmptyPart, Entry, Found, FreeNames, IgnoredUnder, Name, Reference, Rules, Takes, compare_entries, part_holds_nothing,
};
use diacraft_container::xml::parse_with;
use diacraft_container::{Work, XmlLimits};

const Q: &str = "urn:q";
const X: &str = "urn:x";

/// The comparison of two packages, each way round: the count is the same either way (no rule here orients the check, but for the one test that says so).
fn diff_with(rules: &Rules, a: &[Entry<'_>], b: &[Entry<'_>]) -> ContentDiff {
    let ab = compare_entries(a, b, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    let ba = compare_entries(b, a, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

fn one(xml: &str) -> Vec<Entry<'_>> {
    vec![Entry { name: "/p.xml", xml: true, bytes: xml.as_bytes() }]
}

fn count(rules: &Rules, a: &str, b: &str) -> u64 {
    diff_with(rules, &one(a), &one(b)).count
}

/// The kind of each difference, in order.
fn kinds(d: &ContentDiff) -> Vec<Found> {
    d.shown.iter().map(|x| x.found).collect()
}

// ---------------------------------------------------------------------------------------------- `xmlns=""` where a rule reads a name

/// `g` of `urn:q` counts as nothing under `p` of `urn:q`; `m` of `urn:q` is the same as none when it holds nothing.
const IGNORED: Rules = Rules {
    ignored_children: &[IgnoredUnder { parent: Name { ns: Q, local: "p" }, children: &[Name { ns: Q, local: "g" }] }],
    absent_when_empty: &[Name { ns: Q, local: "m" }],
    ..Rules::NONE
};

/// The same, with `m` the parent the child counts as nothing under.
const IGNORED_IN_M: Rules = Rules {
    ignored_children: &[IgnoredUnder { parent: Name { ns: Q, local: "m" }, children: &[Name { ns: Q, local: "g" }] }],
    absent_when_empty: &[Name { ns: Q, local: "m" }],
    ..Rules::NONE
};

#[test]
fn a_child_that_undeclares_the_default_namespace_is_in_none_and_is_not_the_child_that_counts_as_nothing() {
    // The first case: the child is `g` of no namespace, which no rule lists, and not the `g` of `urn:q` that counts as nothing.
    let d = diff_with(&IGNORED, &one(r#"<p xmlns="urn:q"><g xmlns=""/></p>"#), &one(r#"<p xmlns="urn:q"/>"#));
    assert_eq!((d.count, kinds(&d)), (1, vec![Found::Element]), "{:?}", d.shown);
    assert_eq!(d.shown[0].path, "/p[0]/g[0]");
    // The control: the child of `urn:q` counts as nothing, under the default namespace or a prefix bound to it.
    assert_eq!(count(&IGNORED, r#"<p xmlns="urn:q"><g/></p>"#, r#"<p xmlns="urn:q"/>"#), 0);
    assert_eq!(count(&IGNORED, r#"<z:p xmlns:z="urn:q"><z:g/></z:p>"#, r#"<p xmlns="urn:q"/>"#), 0);
    // Under a parent with a prefix, with the default namespace in force and undeclared on the child; and undeclared on the parent, so that its child is in none too, until
    // the child declares it again.
    assert_eq!(count(&IGNORED, r#"<z:p xmlns:z="urn:q" xmlns="urn:q"><g xmlns=""/></z:p>"#, r#"<p xmlns="urn:q"/>"#), 1);
    assert_eq!(count(&IGNORED, r#"<d xmlns="urn:q"><z:p xmlns:z="urn:q" xmlns=""><g/></z:p></d>"#, r#"<d xmlns="urn:q"><p/></d>"#), 1);
    assert_eq!(count(&IGNORED, r#"<d xmlns="urn:q"><z:p xmlns:z="urn:q" xmlns=""><g xmlns="urn:q"/></z:p></d>"#, r#"<d xmlns="urn:q"><p/></d>"#), 0);
    // The same document under no rule: the undeclared child is not the child of `urn:q`.
    assert_eq!(count(&Rules::NONE, r#"<p xmlns="urn:q"><g xmlns=""/></p>"#, r#"<p xmlns="urn:q"><g/></p>"#), 1);
}

#[test]
fn an_empty_container_holds_a_child_that_undeclares_the_default_namespace() {
    // `m` holds a `g` of no namespace, which is content: `m` is not empty, and stays. Read twice (when the child is built, and when `m` is asked whether it holds anything),
    // both readings must agree.
    let d = diff_with(&IGNORED_IN_M, &one(r#"<r xmlns="urn:q"><m><g xmlns=""/></m></r>"#), &one(r#"<r xmlns="urn:q"/>"#));
    assert_eq!((d.count, kinds(&d)), (1, vec![Found::Element]), "{:?}", d.shown);
    // The controls: the `g` of `urn:q` counts as nothing, so `m` holds nothing and is gone.
    assert_eq!(count(&IGNORED_IN_M, r#"<r xmlns="urn:q"><m><g/></m></r>"#, r#"<r xmlns="urn:q"/>"#), 0);
    assert_eq!(count(&IGNORED_IN_M, r#"<r xmlns="urn:q"><m xmlns:z="urn:q"><z:g/></m></r>"#, r#"<r xmlns="urn:q"/>"#), 0);
    assert_eq!(count(&IGNORED_IN_M, r#"<r xmlns="urn:q"><m><g xmlns="urn:w"/></m></r>"#, r#"<r xmlns="urn:q"/>"#), 1);
    // The parent undeclares the default namespace, under a prefix of its own: its child `g` is in none. Whether the parent holds anything is asked where the parent's declarations
    // are no longer in force, so its own are read for it (they were read as none, and the child as the `g` of `urn:q` that counts as nothing: the parent was let go, while the
    // walk that built it had kept `g`).
    let d = diff_with(&IGNORED_IN_M, &one(r#"<r xmlns="urn:q" xmlns:z="urn:q"><z:m xmlns=""><g/></z:m></r>"#), &one(r#"<r xmlns="urn:q"/>"#));
    assert_eq!((d.count, kinds(&d)), (1, vec![Found::Element]), "{:?}", d.shown);
    assert_eq!(count(&IGNORED_IN_M, r#"<r xmlns="urn:q" xmlns:z="urn:q"><z:m xmlns=""><g xmlns="urn:q"/></z:m></r>"#, r#"<r xmlns="urn:q"/>"#), 0);
}

fn s_named(name: &str) -> bool {
    name == "/s.xml"
}

/// `/s.xml` whose root `r` of `urn:q` holds nothing but an empty `c` of `urn:q` is no part.
const EMPTY: Rules = Rules {
    parts_absent_when_empty: &[EmptyPart { named: s_named, root: Name { ns: Q, local: "r" }, children: &[Name { ns: Q, local: "c" }] }],
    ..Rules::NONE
};

fn with_part(bytes: &str) -> Vec<Entry<'_>> {
    vec![Entry { name: "/o.xml", xml: true, bytes: b"<o/>" }, Entry { name: "/s.xml", xml: true, bytes: bytes.as_bytes() }]
}

#[track_caller]
fn is_no_part(bytes: &str, expected: bool) {
    let base = vec![Entry { name: "/o.xml", xml: true, bytes: b"<o/>" }];
    let d = diff_with(&EMPTY, &base, &with_part(bytes));
    assert_eq!(d.same(), expected, "{bytes}: {:?}", d.shown);
    if !expected {
        assert_eq!(kinds(&d), [Found::ExtraPart], "{bytes}");
    }
    let entry = Entry { name: "/s.xml", xml: true, bytes: bytes.as_bytes() };
    assert_eq!(part_holds_nothing(&entry, &EMPTY, &XmlLimits::default(), &mut Work::default()), expected, "{bytes}");
}

#[test]
fn an_empty_part_whose_child_undeclares_the_default_namespace_is_a_part() {
    // The second case: the root's child is a `c` of no namespace, which the kind does not let go.
    is_no_part(r#"<r xmlns="urn:q"><c xmlns=""/></r>"#, false);
    // The controls: the `c` of `urn:q` that holds nothing is let go, by the default namespace or by a prefix bound to it (the same URI as the default); another child is not.
    is_no_part(r#"<r xmlns="urn:q"><c/></r>"#, true);
    is_no_part(r#"<r xmlns="urn:q" xmlns:z="urn:q"><z:c/></r>"#, true);
    is_no_part(r#"<r xmlns="urn:q"><d/></r>"#, false);
    // A root that undeclares the default namespace is the `r` of none, whatever its child.
    is_no_part(r#"<r xmlns=""><c xmlns="urn:q"/></r>"#, false);
}

#[test]
fn a_prefix_declared_again_on_the_child_is_the_childs_where_a_rule_reads_its_name() {
    // The child's own declaration of the prefix wins over the one in force where it stands, both ways round.
    is_no_part(r#"<z:r xmlns:z="urn:q"><z:c xmlns:z="urn:w"/></z:r>"#, false);
    is_no_part(r#"<r xmlns="urn:q" xmlns:z="urn:w"><z:c xmlns:z="urn:q"/></r>"#, true);
    assert_eq!(count(&IGNORED, r#"<p xmlns="urn:q" xmlns:z="urn:q"><z:g xmlns:z="urn:w"/></p>"#, r#"<p xmlns="urn:q"/>"#), 1);
    assert_eq!(count(&IGNORED, r#"<p xmlns="urn:q" xmlns:z="urn:w"><z:g xmlns:z="urn:q"/></p>"#, r#"<p xmlns="urn:q"/>"#), 0);
    // A declaration ends with its element: after the child that declared it again, the prefix is the parent's.
    assert_eq!(count(&IGNORED, r#"<p xmlns:z="urn:q" xmlns="urn:q"><s xmlns:z="urn:w"/><z:g/></p>"#, r#"<p xmlns="urn:q"><s/></p>"#), 0);
}

#[test]
fn a_prefix_bound_to_the_uri_of_the_default_namespace_names_the_elements_the_default_names_and_no_attribute_of_none() {
    let rules = Rules::NONE;
    assert_eq!(count(&rules, r#"<r xmlns="urn:a" xmlns:p="urn:a"><p:x/><x/></r>"#, r#"<r xmlns="urn:a"><x/><x/></r>"#), 0);
    // An unprefixed attribute is in no namespace, and one with the prefix is in its namespace: two different attributes.
    assert_eq!(count(&rules, r#"<r xmlns="urn:a" xmlns:p="urn:a" p:k="1"/>"#, r#"<r xmlns="urn:a" k="1"/>"#), 2);
}

// ---------------------------------------------------------------------------------------------------------- the container of free names

/// Definitions `Def` (of the namespace `def_ns`) of a container `Autos` (of `container_ns`), told by `fam` and named by `name`; `ref` on any element names one of family `a`.
const fn free(container_ns: &'static str) -> FreeNames {
    FreeNames {
        container: Name { ns: container_ns, local: "Autos" },
        definitions: &[Definition { element: Name { ns: Q, local: "Def" }, family: Some(Name { ns: "", local: "fam" }) }],
        name: Name { ns: "", local: "name" },
        references: &[Reference {
            attr: Name { ns: "", local: "ref" },
            on: None,
            takes: &[Takes { element: Name { ns: Q, local: "Def" }, family: "a" }],
            list: false,
        }],
        definition_references: &[],
        scopes: &[],
    }
}

const FREE_IN_NONE: Rules = Rules { free_names: &[free("")], ..Rules::NONE };
const FREE_IN_Q: Rules = Rules { free_names: &[free(Q)], ..Rules::NONE };

/// A document under the default namespace `urn:q` whose child `Autos` says `autos` of itself (its declarations), holding two definitions of one content, `x` and `y`, and a
/// shape that names `named`.
fn styled(autos: &str, named: &str) -> String {
    format!(
        r#"<d xmlns="urn:q"><Autos{autos}><Def xmlns="urn:q" name="x" fam="a" v="1"/><Def xmlns="urn:q" name="y" fam="a" v="1"/></Autos><item ref="{named}"/></d>"#
    )
}

#[test]
fn a_container_of_free_names_is_the_container_its_own_declarations_say() {
    // A rule whose container is the `Autos` of `urn:q` finds none here: the child that undeclares the default namespace is the `Autos` of none. The names are compared as they are
    // written, and the shape that names `x` is not the shape that names `y` (the child was read as the `Autos` of `urn:q`, its definitions taken for the container's, and
    // the two shapes for the same, while the document's own container of free names holds no definition of either name).
    let d = diff_with(&FREE_IN_Q, &one(&styled(r#" xmlns="""#, "x")), &one(&styled(r#" xmlns="""#, "y")));
    assert_eq!((d.count, kinds(&d)), (1, vec![Found::Value]), "{:?}", d.shown);
    // The container is `Autos` of no namespace, written as the default undeclared under a document of `urn:q`. Its definitions `x` and `y` say one thing, so a shape that names
    // `x` is the same as one that names `y`; and with another name the definitions are other definitions.
    assert_eq!(count(&FREE_IN_NONE, &styled(r#" xmlns="""#, "x"), &styled(r#" xmlns="""#, "y")), 0);
    // The controls: the `Autos` of `urn:q` (the default in force, or a prefix of its own) is that rule's container, and the `Autos` of none is not.
    assert_eq!(count(&FREE_IN_Q, &styled("", "x"), &styled("", "y")), 0);
    assert_eq!(count(&FREE_IN_NONE, &styled("", "x"), &styled("", "y")), 1);
    let prefixed = |named: &str| {
        format!(
            r#"<d xmlns="urn:q" xmlns:z="urn:q"><z:Autos xmlns=""><Def xmlns="urn:q" name="x" fam="a" v="1"/><Def xmlns="urn:q" name="y" fam="a" v="1"/></z:Autos><item ref="{named}"/></d>"#
        )
    };
    assert_eq!(count(&FREE_IN_Q, &prefixed("x"), &prefixed("y")), 0);
    assert_eq!(count(&FREE_IN_NONE, &prefixed("x"), &prefixed("y")), 1);
}

// ------------------------------------------------------------------------------------------------- a part that breaks a namespace constraint

/// Pairs: a part that breaks a constraint of XML Namespaces 1.0, and a part the check read as the same before a change while a reader of ours does not (or, where none does, the
/// part written another way). Each is compared as bytes, so each pair differs; and the part against itself (the same bytes) is the same.
const BROKEN: [(&str, &str, &str); 14] = [
    // No Prefix Undeclaring: a prefix bound to the empty URI. The sheet reader takes `p:Cell` for a cell in no namespace, and an unbound prefix for none.
    ("a prefix declared empty", r#"<r><p:Cell xmlns:p=""/></r>"#, r#"<r><p:Cell/></r>"#),
    // A prefix declared empty on a child that a rule reads through its own declarations: the child was read as the parent's prefix said.
    ("a prefix declared empty on a child", r#"<p xmlns="urn:q" xmlns:z="urn:q"><z:g xmlns:z=""/></p>"#, r#"<p xmlns="urn:q"/>"#),
    // Reserved Prefixes: the XML namespace bound to another prefix. The drawing reader reads `xml:id` by its prefix, and `foo:id` is no id to it.
    ("another prefix for the XML namespace", r#"<r xmlns:foo="http://www.w3.org/XML/1998/namespace" foo:id="s1"/>"#, r#"<r xml:id="s1"/>"#),
    ("the XML namespace as the default", r#"<r xmlns="http://www.w3.org/XML/1998/namespace"/>"#, r#"<xml:r/>"#),
    // `xml` bound to another namespace: the sheet reader binds it as declared, and the drawing reader does not.
    ("`xml` bound to another namespace", r#"<r><xml:c xmlns:xml="urn:m"/></r>"#, r#"<r><xml:c/></r>"#),
    // `xmlns` declared, an element of the prefix `xmlns`, and the namespace of declarations bound.
    ("`xmlns` declared as a prefix", r#"<r xmlns:xmlns="urn:a"><xmlns:x/></r>"#, r#"<r xmlns:xmlns="urn:a"><xmlns:x /></r>"#),
    (
        "the namespace of declarations bound to a prefix",
        r#"<r xmlns:p="http://www.w3.org/2000/xmlns/" p:k="1"/>"#,
        r#"<r xmlns:p="http://www.w3.org/2000/xmlns/"  p:k="1"/>"#,
    ),
    (
        "the namespace of declarations as the default",
        r#"<r xmlns="http://www.w3.org/2000/xmlns/"/>"#,
        r#"<r xmlns="http://www.w3.org/2000/xmlns/" />"#,
    ),
    // Names that are no qualified names: an empty prefix declared (it bound the default namespace), an empty prefix (read as the default namespace's, an attribute's
    // too) or local part, and a name with two colons.
    ("an empty prefix declared", r#"<r xmlns:="urn:a"><x/></r>"#, r#"<r xmlns="urn:a"><x/></r>"#),
    ("an element of an empty prefix", r#"<r xmlns="urn:a"><:x/></r>"#, r#"<r xmlns="urn:a"><x/></r>"#),
    ("an attribute of an empty prefix", r#"<r xmlns="urn:a" xmlns:p="urn:a" :k="1"/>"#, r#"<r xmlns="urn:a" xmlns:p="urn:a" p:k="1"/>"#),
    ("an element of an empty local part", r#"<r xmlns:x="urn:a"><x:/></r>"#, r#"<r xmlns:y="urn:a"><y:/></r>"#),
    ("an element of two colons", r#"<a:b:c xmlns:a="urn:a"/>"#, r#"<z:b:c xmlns:z="urn:a"/>"#),
    ("an attribute of two colons", r#"<r xmlns:a="urn:a" a:b:c="1"/>"#, r#"<r xmlns:z="urn:a" z:b:c="1"/>"#),
];

#[test]
fn a_part_that_breaks_a_namespace_constraint_is_compared_as_bytes() {
    for (what, broken, other) in BROKEN {
        assert!(parse_with(broken.as_bytes(), &XmlLimits::default()).is_ok(), "{what}: the parser takes it, so the check must read it");
        let d = diff_with(&IGNORED, &one(broken), &one(other));
        assert_eq!((d.count, kinds(&d)), (1, vec![Found::Bytes]), "{what}: {broken} against {other}: {:?}", d.shown);
        assert_eq!(d.shown[0].path, "", "{what}: bytes have no place");
        assert!(diff_with(&IGNORED, &one(broken), &one(broken)).same(), "{what}: the same bytes are the same content");
        // Written another way, it is other bytes, and so a difference.
        let spaced = broken.replacen("/>", " />", 1);
        assert_eq!(count(&IGNORED, broken, &spaced), 1, "{what}: {spaced}");
    }
    // The controls: what the constraints allow is read as before. `xml` declared as its own namespace; a prefix that nothing binds (it is the name as written); a
    // default declared empty; the default namespace and a prefix bound to one URI.
    for (a, b) in [
        (r#"<r xmlns:xml="http://www.w3.org/XML/1998/namespace" xml:lang="en"/>"#, r#"<r  xml:lang="en" />"#),
        (r#"<p:r k="1"/>"#, r#"<p:r  k="1" />"#),
        (r#"<r xmlns="urn:a"><x xmlns=""/></r>"#, r#"<r xmlns="urn:a" ><x xmlns="" /></r>"#),
        (r#"<r xmlns="urn:a" xmlns:p="urn:a"><p:x/></r>"#, r#"<r xmlns="urn:a"><x/></r>"#),
    ] {
        let d = diff_with(&IGNORED, &one(a), &one(b));
        assert!(d.same(), "{a} against {b}: {:?}", d.shown);
    }
}

#[test]
fn a_broken_declaration_anywhere_makes_the_part_bytes_in_a_child_that_counts_as_nothing_and_in_a_definition_too() {
    // In a child that counts as nothing (it is not read, but the part is not one the rules can read), and in a definition of the free names (built before the walk of the rest).
    let ignored = r#"<p xmlns="urn:q"><g><k xmlns:z=""/></g></p>"#;
    assert_eq!(kinds(&diff_with(&IGNORED, &one(ignored), &one(r#"<p xmlns="urn:q"/>"#))), [Found::Bytes]);
    let defined =
        |extra: &str| format!(r#"<d xmlns="urn:q"><Autos xmlns=""><Def xmlns="urn:q" name="x" fam="a" v="1"{extra}/></Autos><item ref="x"/></d>"#);
    let broken = defined(r#" xmlns:z="""#);
    assert_eq!(kinds(&diff_with(&FREE_IN_NONE, &one(&broken), &one(&broken.replacen("/>", " />", 1)))), [Found::Bytes]);
    // The control: the same documents with nothing broken.
    assert!(diff_with(&IGNORED, &one(r#"<p xmlns="urn:q"><g><k xmlns:z="urn:z"/></g></p>"#), &one(r#"<p xmlns="urn:q"/>"#)).same());
    let whole = defined(r#" xmlns:z="urn:z""#);
    assert!(diff_with(&FREE_IN_NONE, &one(&whole), &one(&whole.replacen("/>", " />", 1))).same());
}

#[test]
fn an_empty_part_that_breaks_a_namespace_constraint_is_a_part() {
    is_no_part(r#"<r xmlns="urn:q" xmlns:z=""/>"#, false);
    is_no_part(r#"<r xmlns="urn:q"><c xmlns:z="http://www.w3.org/XML/1998/namespace"/></r>"#, false);
    // Both sides hold the part: each is compared as bytes, and the same bytes are the same.
    let (a, b) = (with_part(r#"<r xmlns="urn:q" xmlns:z=""/>"#), with_part(r#"<r xmlns="urn:q" xmlns:z="" />"#));
    assert_eq!(kinds(&diff_with(&EMPTY, &a, &b)), [Found::Bytes]);
    assert!(diff_with(&EMPTY, &a, &a).same());
    // The control: the part with nothing broken is no part.
    is_no_part(r#"<r xmlns="urn:q" xmlns:z="urn:z"/>"#, true);
}

// ------------------------------------------------------------------------------------------------ one attribute under two prefixes

#[test]
fn one_attribute_under_two_prefixes_is_compared_in_the_order_written() {
    // XML Namespaces 1.0 allows no two attributes of one name on an element; a reader takes the first. So the values are compared in the order they are written, and two
    // documents that swap them are two readings (they were sorted by value, and the swap passed as the same).
    let a = r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="1" q:k="2"/>"#;
    let d = diff_with(&Rules::NONE, &one(a), &one(r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="2" q:k="1"/>"#));
    assert_eq!(kinds(&d), [Found::Value, Found::Value], "{:?}", d.shown);
    // The prefixes are free, and the order of the values is what counts: the same values in the same order under the prefixes swapped are the same.
    assert!(diff_with(&Rules::NONE, &one(a), &one(r#"<r xmlns:p="urn:a" xmlns:q="urn:a" q:k="1" p:k="2"/>"#)).same());
    // Another attribute between them changes nothing of their order.
    assert_eq!(kinds(&diff_with(&Rules::NONE, &one(a), &one(r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="1" j="0" q:k="2"/>"#))), [Found::Attribute]);
    // A value changed in one of them is found, and so is one that was dropped.
    assert_eq!(kinds(&diff_with(&Rules::NONE, &one(a), &one(r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="1" q:k="3"/>"#))), [Found::Value]);
    assert_eq!(kinds(&diff_with(&Rules::NONE, &one(a), &one(r#"<r xmlns:p="urn:a" p:k="1"/>"#))), [Found::Attribute]);
}

#[test]
fn one_attribute_that_counts_as_nothing_when_extra_under_two_prefixes_is_read_in_the_order_written() {
    // The first document holds `m` of `urn:x` once; the second holds it twice, the source's value second. A reader takes the second's first value, which is not the source's.
    let rules = Rules { ignored_when_extra: &[X], ..Rules::NONE };
    let source = r#"<r xmlns:d="urn:x" d:m="1"/>"#;
    let after = |first: &str, second: &str| format!(r#"<r xmlns:d="urn:x" xmlns:e="urn:x" e:m="{first}" d:m="{second}"/>"#);
    let run = |b: &str| compare_entries(&one(source), &one(b), false, &rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(kinds(&run(&after("2", "1"))), [Found::Value]);
    // The control: the source's value first, and another after it that only the second document has.
    assert!(run(&after("1", "2")).same());
}
