//! Two rules: children that count as nothing under one parent (`Rules::ignored_children`) and names that are free (`Rules::free_names`): the definitions a
//! document gives its own things and the references to them are compared by what the definition says, and not by the name the document chose. Each rule that lets two documents pass
//! is paired with the planted differences it must not hide. The lists are this file's own: the ODF lists are tested in `crates/odg/tests/it/canon_free_names.rs`.

use diacraft_container::canon::{
    ContentDiff, Definition, EmptyPart, Entry, Found, FreeNames, IgnoredUnder, KeyFold, Keyed, Name, Reference, Rules, Takes, Unordered,
    compare_entries, part_holds_nothing,
};
use diacraft_container::{Work, XmlLimits};

const S: &str = "urn:s";
const fn n(local: &'static str) -> Name {
    Name { ns: S, local }
}

/// `s:autos` holds `s:def` (told by `s:fam`) and `s:list` (no family), each named by `s:name`. `s:ref` names a definition: of an `s:item` an `s:def` of family `a`, of an `s:other` one of
/// family `b`, of an `s:set` a `s:list`. `s:embedded` is a scope of its own. `s:meta` holds nothing but fields that count: `s:gen` counts as nothing, and `s:meta` and `s:box` are
/// containers that hold nothing when empty. `s:def` is keyed by its family and name, as the styles of a drawing are.
const FREE: FreeNames = FreeNames {
    container: n("autos"),
    definitions: &[Definition { element: n("def"), family: Some(n("fam")) }, Definition { element: n("list"), family: None }],
    name: n("name"),
    references: &[
        Reference { attr: n("ref"), on: Some(n("item")), takes: &[Takes { element: n("def"), family: "a" }], list: false },
        Reference { attr: n("ref"), on: Some(n("other")), takes: &[Takes { element: n("def"), family: "b" }], list: false },
        Reference { attr: n("ref"), on: Some(n("set")), takes: &[Takes { element: n("list"), family: "" }], list: false },
    ],
    definition_references: &[],
    scopes: &[n("embedded")],
};

const RULES: Rules = Rules {
    keyed: &[Keyed { child: n("def"), key: &[n("fam"), n("name")], fold: KeyFold::Exact }],
    absent_when_empty: &[n("meta"), n("box")],
    unordered: &[Unordered { parent: n("meta"), children: &[n("gen"), n("title"), n("keyword"), n("author")] }],
    ignored_children: &[IgnoredUnder { parent: n("meta"), children: &[n("gen")] }],
    free_names: &[FREE],
    parts_absent_when_empty: &[EmptyPart { named: meta_named, root: n("part"), children: &[n("meta")] }],
    ..Rules::NONE
};

fn meta_named(name: &str) -> bool {
    name == "/meta.xml"
}

fn compare(rules: &Rules, a: &str, b: &str) -> (ContentDiff, Work) {
    let ea = [Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }];
    let eb = [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }];
    let mut work = Work::default();
    let ab = compare_entries(&ea, &eb, false, rules, &XmlLimits::default(), &mut work).unwrap();
    let ba = compare_entries(&eb, &ea, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    (ab, work)
}

#[track_caller]
fn same(a: &str, b: &str) {
    let (d, _) = compare(&RULES, a, b);
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

#[track_caller]
fn differ(a: &str, b: &str) -> ContentDiff {
    let (d, _) = compare(&RULES, a, b);
    assert!(!d.same(), "{a} against {b}: called the same");
    d
}

#[track_caller]
fn one(a: &str, b: &str, found: Found) {
    let d = differ(a, b);
    assert_eq!(d.count, 1, "{a} against {b}: {:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| x.found), Some(found), "{a} against {b}");
}

fn doc(autos: &str, body: &str) -> String {
    format!("<s:doc xmlns:s=\"{S}\"><s:autos>{autos}</s:autos><s:body>{body}</s:body></s:doc>")
}

fn def(name: &str, family: &str, value: &str) -> String {
    format!("<s:def s:name=\"{name}\" s:fam=\"{family}\"><s:p v=\"{value}\"/></s:def>")
}

fn item(name: &str) -> String {
    format!("<s:item s:ref=\"{name}\"/>")
}

// ------------------------------------------------------------------------------------------------------------------------ free names

#[test]
fn definitions_that_say_the_same_under_other_names_are_the_same_and_the_references_follow() {
    let a = doc(&format!("{}{}", def("x1", "a", "1"), def("x2", "a", "2")), &format!("{}{}", item("x1"), item("x2")));
    // Other names, in the other order: each reference names the definition with its content.
    let b = doc(&format!("{}{}", def("q", "a", "2"), def("p", "a", "1")), &format!("{}{}", item("p"), item("q")));
    same(&a, &b);
    same(&b, &a);
    // The control: a reference to the other definition is the one difference, and where it is.
    let wrong = doc(&format!("{}{}", def("q", "a", "2"), def("p", "a", "1")), &format!("{}{}", item("q"), item("q")));
    let found = differ(&a, &wrong);
    assert_eq!((found.count, found.shown[0].found), (1, Found::Value), "{:?}", found.shown);
    assert!(found.shown[0].path.ends_with("/@ref"), "{:?}", found.shown);
}

#[test]
fn a_definitions_content_is_still_compared_and_a_changed_one_under_its_name_is_one_difference() {
    let a = doc(&def("x1", "a", "1"), &item("x1"));
    // A value changed under the same name: the definitions pair by name and the one difference is the value.
    one(&a, &doc(&def("x1", "a", "2"), &item("x1")), Found::Value);
    // Changed and renamed: no match for either, so each keeps its name, and a name is a difference.
    assert!(!compare(&RULES, &a, &doc(&def("y", "a", "2"), &item("y"))).0.same());
    // Another family is another definition, a child added is one, and a definition added is one.
    assert!(!compare(&RULES, &a, &doc(&def("x1", "b", "1"), &item("x1"))).0.same());
    assert!(!compare(&RULES, &a, &doc("<s:def s:name=\"x1\" s:fam=\"a\"><s:p v=\"1\"/><s:p v=\"2\"/></s:def>", &item("x1"))).0.same());
    one(&a, &doc(&format!("{}{}", def("x1", "a", "1"), def("x9", "a", "9")), &item("x1")), Found::Element);
}

#[test]
fn a_reference_asks_for_a_family_and_a_definition_of_another_is_not_the_one() {
    // `s:item` asks for family `a`: the definition `x` of family `b` is not the one it names, so it is compared as written and `x` is not `y`.
    let build = |name: &str| doc(&def(name, "b", "1"), &item(name));
    assert!(!compare(&RULES, &build("x"), &build("y")).0.same());
    // Asked for by an `s:other`, which wants family `b`, the same definitions are the same under two names.
    let other = |name: &str| doc(&def(name, "b", "1"), &format!("<s:other s:ref=\"{name}\"/>"));
    same(&other("x"), &other("y"));
    // A reference to a definition that is of no kind it asks for, in each way: the same name in two families, and one that is no definition.
    let both = |name: &str| doc(&format!("{}{}", def(name, "a", "1"), def(name, "b", "2")), &format!("{}<s:other s:ref=\"{name}\"/>", item(name)));
    same(&both("x"), &both("y"));
    let missing = |name: &str| doc(&def("d", "a", "1"), &item(name));
    assert!(!compare(&RULES, &missing("gone"), &missing("other")).0.same());
}

#[test]
fn a_kind_with_no_family_is_read_by_its_own_references() {
    let list = |name: &str| doc(&format!("<s:list s:name=\"{name}\"><s:p v=\"1\"/></s:list>"), &format!("<s:set s:ref=\"{name}\"/>"));
    same(&list("L1"), &list("L9"));
    // A `s:set` names a list and not a definition: the same name for a definition is not the list.
    let wrong = |name: &str| doc(&def(name, "a", "1"), &format!("<s:set s:ref=\"{name}\"/>"));
    assert!(!compare(&RULES, &wrong("x"), &wrong("y")).0.same());
}

#[test]
fn a_name_two_definitions_of_one_kind_and_family_hold_is_compared_as_written() {
    let twice = |name: &str| doc(&format!("{}{}", def(name, "a", "1"), def(name, "a", "2")), &item(name));
    assert!(!compare(&RULES, &twice("x"), &twice("y")).0.same(), "which of the two a reference names is not told");
    // One of the two families holds it once: that one is renamed, and the other is not.
    let mixed = |name: &str| {
        doc(&format!("{}{}{}", def(name, "a", "1"), def(name, "a", "2"), def("m", "b", "3")), &format!("<s:other s:ref=\"m\"/>{}", item(name)))
    };
    assert!(!compare(&RULES, &mixed("x"), &mixed("y")).0.same());
}

#[test]
fn a_name_inside_the_container_or_an_embedded_document_is_compared_as_written() {
    // A definition's own child that names another definition, as a style names its parent, is compared by the name.
    let inner = |name: &str, target: &str| {
        doc(&format!("<s:def s:name=\"{name}\" s:fam=\"a\"><s:p s:ref=\"{target}\"/></s:def>{}", def(target, "a", "1")), &item(name))
    };
    assert!(!compare(&RULES, &inner("x", "t"), &inner("y", "u")).0.same(), "a reference inside a definition is the name as written");
    // An embedded element's names are its own: the references and the definitions in it are not read.
    let embedded = |name: &str| doc(&def("d", "a", "1"), &format!("<s:embedded>{}</s:embedded>", item(name)));
    assert!(!compare(&RULES, &embedded("x"), &embedded("y")).0.same());
    let outside = |name: &str| doc(&def(name, "a", "1"), &format!("{}<s:embedded>{}</s:embedded>", item(name), item("z")));
    same(&outside("x"), &outside("y"));
}

#[test]
fn two_containers_or_none_leave_every_name_as_written() {
    let two =
        |name: &str| format!("<s:doc xmlns:s=\"{S}\"><s:autos>{}</s:autos><s:autos/><s:body>{}</s:body></s:doc>", def(name, "a", "1"), item(name));
    assert!(!compare(&RULES, &two("x"), &two("y")).0.same(), "which container a name is in is not told");
    // A document with none has no definition: its references are as written, against a document that has definitions.
    let none = format!("<s:doc xmlns:s=\"{S}\"><s:body>{}</s:body></s:doc>", item("x"));
    assert!(!compare(&RULES, &none, &doc(&def("x", "a", "1"), &item("x"))).0.same());
    let empty = |name: &str| format!("<s:doc xmlns:s=\"{S}\"><s:body>{}</s:body></s:doc>", item(name));
    assert!(!compare(&RULES, &empty("x"), &empty("y")).0.same());
}

#[test]
fn the_markers_are_no_name_a_document_can_spell() {
    // The matched definition is read as a name that no document can write, so no document's own name can be taken for it: a definition named as the marker would read is refused by
    // the parser, and a reference to an unmatched one is its own name.
    let a = doc(&def("x", "a", "1"), &item("x"));
    let b = doc(&def("y", "a", "1"), &item("y"));
    same(&a, &b);
    let c = doc(&format!("{}{}", def("y", "a", "1"), def("z", "a", "9")), &item("z"));
    assert!(!compare(&RULES, &a, &c).0.same());
}

#[test]
fn a_comparison_with_names_free_still_counts_its_differences_exactly_and_symmetrically() {
    // Three definitions renamed, one changed and one added: the count is the same whichever is first, and each difference is one.
    let a = doc(
        &format!("{}{}{}", def("a1", "a", "1"), def("a2", "a", "2"), def("a3", "a", "3")),
        &format!("{}{}{}", item("a1"), item("a2"), item("a3")),
    );
    let b = doc(
        &format!("{}{}{}{}", def("b3", "a", "3"), def("b1", "a", "1"), def("b2", "a", "22"), def("b4", "a", "4")),
        &format!("{}{}{}", item("b1"), item("b2"), item("b3")),
    );
    let (d, _) = compare(&RULES, &a, &b);
    assert!(!d.same());
    assert_eq!(d.count, d.shown.len() as u64, "{:?}", d.shown);
}

#[test]
fn the_work_of_matching_the_names_is_counted() {
    let build = |names: &[&str]| doc(&names.iter().map(|name| def(name, "a", "1")).collect::<String>(), "");
    let (_, small) = compare(&RULES, &build(&["a", "b"]), &build(&["c", "d"]));
    let many: Vec<String> = (0..400).map(|i| format!("n{i}")).collect();
    let others: Vec<String> = (0..400).map(|i| format!("m{i}")).collect();
    let (da, db) = (build(&many.iter().map(String::as_str).collect::<Vec<_>>()), build(&others.iter().map(String::as_str).collect::<Vec<_>>()));
    let (d, large) = compare(&RULES, &da, &db);
    assert!(d.same());
    assert!(
        large.canon_steps > small.canon_steps * 20 && large.canon_steps < small.canon_steps * 400,
        "{} against {}",
        large.canon_steps,
        small.canon_steps
    );
}

// ------------------------------------------------------------------------------------------------------------------- ignored children

#[test]
fn a_child_that_counts_as_nothing_is_left_out_wherever_it_stands_and_only_under_its_parent() {
    same(
        "<s:meta xmlns:s=\"urn:s\"><s:gen>a</s:gen><s:title>t</s:title></s:meta>",
        "<s:meta xmlns:s=\"urn:s\"><s:title>t</s:title><s:gen>b</s:gen></s:meta>",
    );
    same("<s:meta xmlns:s=\"urn:s\"><s:gen>a</s:gen><s:title>t</s:title></s:meta>", "<s:meta xmlns:s=\"urn:s\"><s:title>t</s:title></s:meta>");
    // Its prefix may be bound by the parent or by the child, and the prefix means the namespace.
    same(
        "<m:meta xmlns:m=\"urn:s\"><m:gen>a</m:gen><m:title>t</m:title></m:meta>",
        "<s:meta xmlns:s=\"urn:s\"><x:gen xmlns:x=\"urn:s\">z</x:gen><s:title>t</s:title></s:meta>",
    );
    // The controls: another child, the same name under another parent, and the same local name in another namespace.
    one(
        "<s:meta xmlns:s=\"urn:s\"><s:gen>a</s:gen></s:meta>",
        "<s:meta xmlns:s=\"urn:s\"><s:gen>a</s:gen><s:title>t</s:title></s:meta>",
        Found::Element,
    );
    one("<s:row xmlns:s=\"urn:s\"><s:gen>a</s:gen></s:row>", "<s:row xmlns:s=\"urn:s\"><s:gen>b</s:gen></s:row>", Found::Text);
    one("<s:meta xmlns:s=\"urn:s\"><x:gen xmlns:x=\"urn:x\">a</x:gen></s:meta>", "<s:meta xmlns:s=\"urn:s\"/>", Found::Element);
}

#[test]
fn a_container_that_holds_only_children_that_count_as_nothing_holds_nothing() {
    // `s:meta` is a container that is the same as none when it holds nothing: with a generator and nothing else, it holds nothing.
    same("<s:r xmlns:s=\"urn:s\"><s:meta><s:gen>a</s:gen></s:meta></s:r>", "<s:r xmlns:s=\"urn:s\"/>");
    same("<m:r xmlns:m=\"urn:s\"><m:meta>\n <m:gen>a</m:gen>\n</m:meta></m:r>", "<m:r xmlns:m=\"urn:s\"/>");
    // The prefix of the child is bound by the container (not in force any more where the container's emptiness is judged).
    same("<s:r xmlns:s=\"urn:s\"><x:meta xmlns:x=\"urn:s\"><x:gen/></x:meta></s:r>", "<s:r xmlns:s=\"urn:s\"/>");
    // Anything else in it is content: a field, a text, an attribute, another element, a generator of another namespace.
    for held in ["<s:title/>", "x", "<s:gen/><s:title>t</s:title>", "<o:gen xmlns:o=\"urn:o\"/>"] {
        one(&format!("<s:r xmlns:s=\"urn:s\"><s:meta>{held}</s:meta></s:r>"), "<s:r xmlns:s=\"urn:s\"/>", Found::Element);
    }
    one("<s:r xmlns:s=\"urn:s\"><s:meta a=\"1\"><s:gen/></s:meta></s:r>", "<s:r xmlns:s=\"urn:s\"/>", Found::Element);
    // A box is a container the rule does not list: a generator in it is a generator in an element it is not the parent of.
    one("<s:r xmlns:s=\"urn:s\"><s:box><s:gen/></s:box></s:r>", "<s:r xmlns:s=\"urn:s\"/>", Found::Element);
}

#[test]
fn a_part_whose_only_content_is_children_that_count_as_nothing_is_no_part() {
    let part = |inner: &str| format!("<s:part xmlns:s=\"urn:s\">{inner}</s:part>");
    let entry = |bytes: &'static str| Entry { name: "/meta.xml", xml: true, bytes: bytes.as_bytes() };
    let held = |bytes: &str| {
        part_holds_nothing(&Entry { name: "/meta.xml", xml: true, bytes: bytes.as_bytes() }, &RULES, &XmlLimits::default(), &mut Work::default())
    };
    assert!(held(&part("<s:meta><s:gen>g</s:gen></s:meta>")));
    assert!(held(&part("<s:meta/>")));
    assert!(!held(&part("<s:meta><s:gen>g</s:gen><s:title>t</s:title></s:meta>")));
    assert!(!held(&part("<s:meta><s:gen>g</s:gen></s:meta><s:other/>")));
    let base = vec![Entry { name: "/p.xml", xml: true, bytes: b"<p/>" }];
    let with = |bytes: &'static str| vec![Entry { name: "/p.xml", xml: true, bytes: b"<p/>" }, entry(bytes)];
    let d = compare_entries(
        &base,
        &with("<s:part xmlns:s=\"urn:s\"><s:meta><s:gen>g</s:gen></s:meta></s:part>"),
        false,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap();
    assert!(d.same(), "{:?}", d.shown);
    let d = compare_entries(
        &base,
        &with("<s:part xmlns:s=\"urn:s\"><s:meta><s:title/></s:meta></s:part>"),
        false,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap();
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart));
}

#[test]
fn the_order_of_a_set_and_a_child_that_counts_as_nothing_hold_together() {
    // The ignored generator is out before the set is ordered: a field moved past it is moved among the fields alone.
    same(
        "<s:meta xmlns:s=\"urn:s\"><s:author>a</s:author><s:gen>g</s:gen><s:title>t</s:title><s:keyword>1</s:keyword><s:keyword>2</s:keyword></s:meta>",
        "<s:meta xmlns:s=\"urn:s\"><s:keyword>1</s:keyword><s:keyword>2</s:keyword><s:title>t</s:title><s:author>a</s:author></s:meta>",
    );
    // The copies of a field are in the order written.
    let d = differ(
        "<s:meta xmlns:s=\"urn:s\"><s:keyword>1</s:keyword><s:keyword>2</s:keyword></s:meta>",
        "<s:meta xmlns:s=\"urn:s\"><s:keyword>2</s:keyword><s:keyword>1</s:keyword></s:meta>",
    );
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Order));
}

#[test]
fn rules_that_list_none_leave_everything_as_it_was() {
    let none = Rules::NONE;
    let (d, _) = compare(&none, "<s:meta xmlns:s=\"urn:s\"><s:gen>a</s:gen></s:meta>", "<s:meta xmlns:s=\"urn:s\"><s:gen>b</s:gen></s:meta>");
    assert_eq!(d.count, 1, "a generator is text with no rule");
    let a = doc(&def("x", "a", "1"), &item("x"));
    let b = doc(&def("y", "a", "1"), &item("y"));
    assert!(!compare(&none, &a, &b).0.same(), "a name is text with no rule");
}
