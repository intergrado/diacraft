//! Two rules: children whose order among themselves has no meaning under one parent (`Rules::unordered`), and a part on one side
//! only that is of a listed kind (a relationships part, by its name) and holds an empty listed root (`Rules::parts_absent_when_empty`; a part of any other
//! name is a part whatever its root holds). Each rule that lets two spellings pass is paired with a planted difference it still catches.
//! The lists are this file's own: the VSDX lists are tested in `crates/vsdx/tests/it/canon_empty_parts.rs`.

use diacraft_container::canon::{
    ContentDiff, EmptyPart, Entry, Found, KeyFold, Keyed, MAX_EMPTY_PART_BYTES, Name, Rules, Unordered, compare_entries,
};
use diacraft_container::{Work, XmlLimits};

const T: Name = Name { ns: "", local: "t" };
const X: Name = Name { ns: "urn:x", local: "x" };
const ROOT: Name = Name { ns: "urn:r", local: "Rels" };

/// A relationships part's name as OPC gives it: `.rels` in a folder `_rels`, ASCII case folded (the rule `diacraft_opc::name::is_rels_part` holds, written
/// again here since the container does not depend on OPC).
fn rels_named(name: &str) -> bool {
    let mut segments = name.rsplit('/');
    let (file, folder) = (segments.next().unwrap_or(""), segments.next().unwrap_or(""));
    folder.eq_ignore_ascii_case("_rels") && file.len() >= 5 && file.get(file.len() - 5..).is_some_and(|end| end.eq_ignore_ascii_case(".rels"))
}

/// Under `s` (no namespace) the set is `t`, then `x` of `urn:x`; `c` is keyed by `n`; a relationships part whose root is an empty `Rels` of `urn:r` is no part.
const RULES: Rules = Rules {
    keyed: &[Keyed { child: Name { ns: "", local: "c" }, key: &[Name { ns: "", local: "n" }], fold: KeyFold::Exact }],
    unordered: &[Unordered { parent: Name { ns: "", local: "s" }, children: &[T, X] }],
    parts_absent_when_empty: &[EmptyPart { named: rels_named, root: ROOT, children: &[] }],
    ignored_attributes: &[],
    ..Rules::NONE
};

fn part(name: &'static str, bytes: &'static str) -> Entry<'static> {
    Entry { name, xml: true, bytes: bytes.as_bytes() }
}

/// Both directions: the count is symmetric.
fn diff(a: &[Entry<'_>], b: &[Entry<'_>]) -> ContentDiff {
    let ab = compare_entries(a, b, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    let ba = compare_entries(b, a, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

fn inside(xml: &'static str) -> Vec<Entry<'static>> {
    vec![part("/p.xml", xml)]
}

#[track_caller]
fn same(a: &'static str, b: &'static str) {
    let d = diff(&inside(a), &inside(b));
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

#[track_caller]
fn differ(a: &'static str, b: &'static str, found: Found) {
    let d = diff(&inside(a), &inside(b));
    assert!(!d.same(), "{a} against {b}");
    assert_eq!(d.shown.first().map(|x| x.found), Some(found), "{a} against {b}: {:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ the order of a set

#[test]
fn the_children_of_a_set_in_another_order_are_the_same() {
    same(r#"<r xmlns:x="urn:x"><s><t/><x:x/></s></r>"#, r#"<r xmlns:x="urn:x"><s><x:x/><t/></s></r>"#);
    // Around a keyed child and text-free spacing, and with several of each name.
    same(
        r#"<r xmlns:x="urn:x"><s><c n="1"/> <t a="1"/> <x:x/> <t a="2"/></s></r>"#,
        r#"<r xmlns:x="urn:x"><s><t a="1"/><t a="2"/><x:x/><c n="1"/></s></r>"#,
    );
    // The set's children around another child keep the place the set had: one before, one after, either way round.
    same(r#"<r xmlns:x="urn:x"><s><t/><o/><x:x/></s></r>"#, r#"<r xmlns:x="urn:x"><s><x:x/><o/><t/></s></r>"#);
}

#[test]
fn the_copies_of_one_name_keep_their_order_and_everything_else_keeps_its_place() {
    // Two `t`s swapped: their order among themselves is content.
    differ(r#"<r><s><t a="1"/><t a="2"/></s></r>"#, r#"<r><s><t a="2"/><t a="1"/></s></r>"#, Found::Order);
    // A child outside the set and one of the set swapped: the set keeps its places, so this is an order.
    differ(r#"<r><s><t/><o/></s></r>"#, r#"<r><s><o/><t/></s></r>"#, Found::Order);
    // The same names under another parent: no rule there.
    differ(r#"<r xmlns:x="urn:x"><q><t/><x:x/></q></r>"#, r#"<r xmlns:x="urn:x"><q><x:x/><t/></q></r>"#, Found::Order);
    // The local name of the set in another namespace is not in the set.
    differ(r#"<r xmlns:y="urn:y"><s><t/><y:x/></s></r>"#, r#"<r xmlns:y="urn:y"><s><y:x/><t/></s></r>"#, Found::Order);
    // What the children hold is compared as ever, in the order the rule gives.
    differ(r#"<r xmlns:x="urn:x"><s><t a="1"/><x:x/></s></r>"#, r#"<r xmlns:x="urn:x"><s><x:x/><t a="2"/></s></r>"#, Found::Value);
    // One of the set added is one difference.
    let d = diff(&inside(r#"<r xmlns:x="urn:x"><s><t/><x:x/></s></r>"#), &inside(r#"<r xmlns:x="urn:x"><s><x:x/><t/><t/></s></r>"#));
    assert_eq!(d.count, 1, "{:?}", d.shown);
}

#[test]
fn text_and_instructions_keep_their_places_among_the_set() {
    differ(r#"<r xmlns:x="urn:x"><s><t/>a<x:x/></s></r>"#, r#"<r xmlns:x="urn:x"><s><t/><x:x/>a</s></r>"#, Found::Order);
    same(r#"<r xmlns:x="urn:x"><s><t/>a<x:x/></s></r>"#, r#"<r xmlns:x="urn:x"><s><x:x/>a<t/></s></r>"#);
    differ(r#"<r><s><?p q?><t/></s></r>"#, r#"<r><s><t/><?p q?></s></r>"#, Found::Order);
}

#[test]
fn a_set_of_many_children_costs_a_sort() {
    // 20,000 children of the set in the reverse of the rule's order on one side: the same, in steps that grow as n log n and not n squared.
    let n = 20_000usize;
    let a: String = format!("<r xmlns:x=\"urn:x\"><s>{}{}</s></r>", "<x:x/>".repeat(n), "<t/>".repeat(n));
    let b: String = format!("<r xmlns:x=\"urn:x\"><s>{}{}</s></r>", "<t/>".repeat(n), "<x:x/>".repeat(n));
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    let mut work = Work::default();
    let d = compare_entries(&ea, &eb, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert!(d.same(), "{:?}", d.shown);
    let items = (4 * n) as u64;
    assert!(work.canon_steps < items * 40, "{} steps for {items} children", work.canon_steps);
}

// ------------------------------------------------------------------------------------------------------------ an empty part

const EMPTY: &str = r#"<?xml version="1.0"?><Rels xmlns="urn:r"/>"#;

#[test]
fn a_part_on_one_side_only_that_holds_an_empty_listed_root_is_no_difference() {
    let base = vec![part("/a.xml", "<a/>")];
    for spelling in [
        EMPTY,
        r#"<Rels xmlns="urn:r"></Rels>"#,
        "<Rels xmlns=\"urn:r\">\n  </Rels>",
        r#"<Rels xmlns="urn:r"><!-- none --></Rels>"#,
        r#"<p:Rels xmlns:p="urn:r"/>"#,
        r#"<!-- a comment outside --><Rels xmlns="urn:r"/>"#,
    ] {
        let mut with = base.clone();
        with.push(Entry { name: "/_rels/a.xml.rels", xml: true, bytes: spelling.as_bytes() });
        let d = diff(&base, &with);
        assert!(d.same(), "{spelling}: {:?}", d.shown);
    }
    // On both sides it is compared as a part: the same.
    let both = vec![part("/a.xml", "<a/>"), part("/_rels/a.xml.rels", EMPTY)];
    assert!(diff(&both, &both).same());
}

#[test]
fn a_part_that_holds_anything_or_is_no_listed_root_is_still_a_part() {
    let base = vec![part("/a.xml", "<a/>")];
    let cases: [(&str, bool); 9] = [
        // A row.
        (r#"<Rels xmlns="urn:r"><Rel Id="1"/></Rels>"#, true),
        // An attribute of the root.
        (r#"<Rels xmlns="urn:r" a="1"/>"#, true),
        // Text and an instruction.
        (r#"<Rels xmlns="urn:r">x</Rels>"#, true),
        (r#"<Rels xmlns="urn:r"><?p q?></Rels>"#, true),
        // Another namespace, none, another name.
        (r#"<Rels xmlns="urn:other"/>"#, true),
        (r#"<Rels/>"#, true),
        (r#"<Relationships xmlns="urn:r"/>"#, true),
        // XML that does not parse.
        (r#"<Rels xmlns="urn:r">"#, true),
        // Not meant as XML.
        (EMPTY, false),
    ];
    for (bytes, xml) in cases {
        let mut with = base.clone();
        with.push(Entry { name: "/_rels/a.xml.rels", xml, bytes: bytes.as_bytes() });
        let d = diff(&base, &with);
        assert_eq!(d.count, 1, "{bytes} ({xml}): {:?}", d.shown);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart), "{bytes}");
        let d = diff(&with, &base);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart), "{bytes}");
    }
}

#[test]
fn an_empty_part_larger_than_the_rule_reads_is_a_part() {
    // White space past 64 KiB inside an empty root: the rule does not read it, so it is the extra part it is (the safe side).
    let padded = format!("<Rels xmlns=\"urn:r\">{}</Rels>", " ".repeat(MAX_EMPTY_PART_BYTES));
    let base = vec![part("/a.xml", "<a/>")];
    let mut with = base.clone();
    with.push(Entry { name: "/_rels/a.xml.rels", xml: true, bytes: padded.as_bytes() });
    let d = diff(&base, &with);
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart));
    // Just under it, it is no difference.
    let fits = format!("<Rels xmlns=\"urn:r\">{}</Rels>", " ".repeat(MAX_EMPTY_PART_BYTES - 40));
    let mut with = base.clone();
    with.push(Entry { name: "/_rels/a.xml.rels", xml: true, bytes: fits.as_bytes() });
    assert!(diff(&base, &with).same());
}

#[test]
fn a_relationships_part_named_in_any_case_and_the_packages_own_are_let_go_when_empty() {
    let base = vec![part("/a.xml", "<a/>")];
    for name in ["/_RELS/A.XML.RELS", "/_Rels/a.xml.Rels", "/_rels/.rels", "/x/y/_rels/z.bin.rels", "_rels/a.xml.rels"] {
        let mut with = base.clone();
        with.push(Entry { name, xml: true, bytes: EMPTY.as_bytes() });
        assert!(diff(&base, &with).same(), "{name}");
    }
}

#[test]
fn a_part_of_another_name_is_a_part_whatever_its_root_holds() {
    // A format keeps a part it does not know: one that is not of a listed kind, dropped, is a difference however empty its root and whatever its root is.
    let base = vec![part("/a.xml", "<a/>")];
    for name in
        ["/p.xml", "/_rels/a.xml", "/rels/a.xml.rels", "/a/_rels.rels", "/_rels.rels", "/_rels/a.xml.rels.copy", "/_rels/a.xml.rel", "/_rels_/a.rels"]
    {
        for bytes in [EMPTY, r#"<Rels xmlns="urn:r"></Rels>"#, "<Empty/>"] {
            let mut with = base.clone();
            with.push(Entry { name, xml: true, bytes: bytes.as_bytes() });
            let d = diff(&base, &with);
            assert_eq!(d.count, 1, "{name} {bytes}: {:?}", d.shown);
            assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart), "{name} {bytes}");
            assert_eq!(diff(&with, &base).shown.first().map(|x| x.found), Some(Found::MissingPart), "{name} {bytes}");
        }
    }
}

/// A part of the kind `.empty`, which no format has: its root is `Other` of `urn:o`.
fn empty_named(name: &str) -> bool {
    name.ends_with(".empty")
}

#[test]
fn each_kind_lets_go_only_its_own_root() {
    // Two kinds: a relationships part with the root of the other kind, and a part of the other kind with the relationships root, are parts.
    const OTHER: Name = Name { ns: "urn:o", local: "Other" };
    const TWO: Rules = Rules {
        parts_absent_when_empty: &[
            EmptyPart { named: rels_named, root: ROOT, children: &[] },
            EmptyPart { named: empty_named, root: OTHER, children: &[] },
        ],
        ..RULES
    };
    let two = TWO;
    let base = vec![part("/a.xml", "<a/>")];
    let count = |name: &'static str, bytes: &'static str| {
        let mut with = base.clone();
        with.push(Entry { name, xml: true, bytes: bytes.as_bytes() });
        let ab = compare_entries(&base, &with, true, &two, &XmlLimits::default(), &mut Work::default()).unwrap();
        let ba = compare_entries(&with, &base, true, &two, &XmlLimits::default(), &mut Work::default()).unwrap();
        assert_eq!(ab.count, ba.count);
        ab.count
    };
    assert_eq!(count("/_rels/a.xml.rels", EMPTY), 0);
    assert_eq!(count("/a.empty", r#"<Other xmlns="urn:o"/>"#), 0);
    assert_eq!(count("/_rels/a.xml.rels", r#"<Other xmlns="urn:o"/>"#), 1);
    assert_eq!(count("/a.empty", EMPTY), 1);
}

#[test]
fn the_fuzz_crash_of_an_unknown_part_with_an_empty_root_dropped_is_a_difference() {
    // The full gate's `canon_compare` crash on main 758acd4f, rebuilt in code: the four parts the target cut from its input (two XML parts that do not parse,
    // an empty part that is not XML, and `/p3.xml`, an XML part that is `<Empty/>`), under the target's rules, which let an empty `Empty` go. Dropping `/p3.xml`
    // was counted the same: the rule read its root and not its name. It is no relationships part, so it is the part it is, both ways round.
    let rules =
        Rules { parts_absent_when_empty: &[EmptyPart { named: rels_named, root: Name { ns: "", local: "Empty" }, children: &[] }], ..Rules::NONE };
    let limits = XmlLimits { max_depth: 24, max_nodes: 3000, max_attr_bytes: 256 };
    let parts = [
        Entry { name: "/p0.xml", xml: true, bytes: b"\x00\xd7ffffJ\x11" },
        Entry { name: "/p1.xml", xml: true, bytes: b"ff\xfa" },
        Entry { name: "/p2.xml", xml: false, bytes: b"" },
        Entry { name: "/p3.xml", xml: true, bytes: b"<Empty/>" },
    ];
    let dropped = &parts[..3];
    for (a, b, found) in [(&parts[..], dropped, Found::MissingPart), (dropped, &parts[..], Found::ExtraPart)] {
        let d = compare_entries(a, b, true, &rules, &limits, &mut Work::default()).unwrap();
        assert_eq!(d.count, 1, "{:?}", d.shown);
        assert_eq!(d.shown.first().map(|x| (x.found, x.part.as_str())), Some((found, "/p3.xml")));
    }
    // The control: the same bytes as a relationships part are what the rule lets go.
    let mut named = parts.to_vec();
    named[3].name = "/_rels/p3.xml.rels";
    let d = compare_entries(&named, dropped, true, &rules, &limits, &mut Work::default()).unwrap();
    assert!(d.same(), "{:?}", d.shown);
}

#[test]
fn a_format_that_lists_no_root_counts_every_part() {
    let base = vec![part("/a.xml", "<a/>")];
    let mut with = base.clone();
    with.push(part("/_rels/a.xml.rels", EMPTY));
    let d = compare_entries(&base, &with, true, &Rules::NONE, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart));
}
