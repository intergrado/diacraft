//! Two rules: attributes that count as nothing wherever they stand (`Rules::ignored_attributes`), and the children of an empty part's root that are no
//! content when they hold nothing (`EmptyPart::children`), with an empty part of a kind on both sides the same whatever empty form each takes. Each rule that lets two spellings pass
//! is paired with a planted difference it still catches. The lists are this file's own: the ODF lists are tested in `crates/odg/tests/it/canon_versions_settings.rs`.

use diacraft_container::canon::{ContentDiff, EmptyPart, Entry, Found, Name, Rules, compare_entries, part_holds_nothing};
use diacraft_container::{Work, XmlLimits};

const V: Name = Name { ns: "urn:v", local: "version" };
const ROOT: Name = Name { ns: "urn:v", local: "settings" };
const KID: Name = Name { ns: "urn:v", local: "items" };
const BOX: Name = Name { ns: "", local: "box" };

fn settings_named(name: &str) -> bool {
    name == "/s.xml"
}

/// `v:version` (of `urn:v`) counts as nothing; `box` is an empty container; `/s.xml` whose root `v:settings` holds nothing but an empty `v:items` is no part.
const RULES: Rules = Rules {
    absent_when_empty: &[BOX],
    parts_absent_when_empty: &[EmptyPart { named: settings_named, root: ROOT, children: &[KID] }],
    ignored_attributes: &[V],
    ..Rules::NONE
};

fn diff_with(rules: &Rules, a: &[Entry<'_>], b: &[Entry<'_>]) -> ContentDiff {
    let ab = compare_entries(a, b, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    let ba = compare_entries(b, a, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

fn one(xml: &str) -> Vec<Entry<'_>> {
    vec![Entry { name: "/p.xml", xml: true, bytes: xml.as_bytes() }]
}

#[track_caller]
fn same(a: &str, b: &str) {
    let d = diff_with(&RULES, &one(a), &one(b));
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

#[track_caller]
fn differ(a: &str, b: &str, found: Found) {
    let d = diff_with(&RULES, &one(a), &one(b));
    assert_eq!(d.count, 1, "{a} against {b}: {:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| x.found), Some(found), "{a} against {b}");
}

#[test]
fn an_ignored_attribute_counts_as_nothing_by_its_namespace_wherever_it_stands() {
    same(r#"<r xmlns:v="urn:v" v:version="1.2" a="1"/>"#, r#"<r xmlns:v="urn:v" v:version="1.4" a="1"/>"#);
    same(r#"<r xmlns:v="urn:v" v:version="1.2"/>"#, "<r/>");
    // Under any prefix, and on any element, its own declaration included.
    same(r#"<r xmlns:w="urn:v" w:version="1"><c xmlns:q="urn:v" q:version="9"/></r>"#, "<r><c/></r>");
    // The controls: another attribute beside it, the same local name in another namespace or in none, and the same prefix bound elsewhere.
    differ(r#"<r xmlns:v="urn:v" v:version="1" a="1"/>"#, r#"<r xmlns:v="urn:v" v:version="1" a="2"/>"#, Found::Value);
    differ(r#"<r xmlns:v="urn:w" v:version="1"/>"#, "<r/>", Found::Attribute);
    differ(r#"<r version="1"/>"#, "<r/>", Found::Attribute);
    differ(r#"<r v:version="1"/>"#, "<r/>", Found::Attribute);
}

#[test]
fn an_ignored_attribute_is_no_content_of_an_empty_container_and_another_is() {
    same(r#"<r><box xmlns:v="urn:v" v:version="1"/></r>"#, "<r/>");
    same(r#"<r xmlns:v="urn:v"><box v:version="1"/></r>"#, "<r/>");
    differ(r#"<r><box a="1"/></r>"#, "<r/>", Found::Element);
    differ(r#"<r><box xmlns:v="urn:w" v:version="1"/></r>"#, "<r/>", Found::Element);
}

fn with_settings(bytes: &'static str) -> Vec<Entry<'static>> {
    vec![Entry { name: "/p.xml", xml: true, bytes: b"<p/>" }, Entry { name: "/s.xml", xml: true, bytes: bytes.as_bytes() }]
}

#[test]
fn a_part_whose_root_holds_only_empty_listed_children_is_no_part_and_two_empty_forms_are_one() {
    let base = vec![Entry { name: "/p.xml", xml: true, bytes: b"<p/>" }];
    let empty = [
        r#"<v:settings xmlns:v="urn:v"/>"#,
        r#"<v:settings xmlns:v="urn:v" v:version="1.4"/>"#,
        r#"<v:settings xmlns:v="urn:v"><v:items/></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v" v:version="1.2"> <v:items>  </v:items> </v:settings>"#,
        r#"<v:settings xmlns:v="urn:v"><v:items xmlns:w="urn:v" w:version="1"/></v:settings>"#,
    ];
    for bytes in empty {
        let d = diff_with(&RULES, &base, &with_settings(bytes));
        assert!(d.same(), "{bytes}: {:?}", d.shown);
        let entry = Entry { name: "/s.xml", xml: true, bytes: bytes.as_bytes() };
        assert!(part_holds_nothing(&entry, &RULES, &XmlLimits::default(), &mut Work::default()), "{bytes}");
        // Against each other empty form, both sides holding the part.
        for other in empty {
            let d = diff_with(&RULES, &with_settings(bytes), &with_settings(other));
            assert!(d.same(), "{bytes} against {other}: {:?}", d.shown);
        }
    }
    // The controls: a listed child that holds something, a child that is not listed, an attribute of the root, and the same root under another name.
    for bytes in [
        r#"<v:settings xmlns:v="urn:v"><v:items a="1"/></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v"><v:items><v:item/></v:items></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v"><v:other/></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v"><v:items/><v:items>x</v:items></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v" a="1"><v:items/></v:settings>"#,
        r#"<v:settings xmlns:v="urn:v">text</v:settings>"#,
    ] {
        let d = diff_with(&RULES, &base, &with_settings(bytes));
        assert!(!d.same(), "{bytes}");
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::ExtraPart), "{bytes}: {:?}", d.shown);
        let entry = Entry { name: "/s.xml", xml: true, bytes: bytes.as_bytes() };
        assert!(!part_holds_nothing(&entry, &RULES, &XmlLimits::default(), &mut Work::default()), "{bytes}");
        // Beside an empty part, the part that holds something is compared as XML: one difference inside it.
        let d = diff_with(&RULES, &with_settings(r#"<v:settings xmlns:v="urn:v"/>"#), &with_settings(bytes));
        assert!(!d.same(), "{bytes} beside an empty part");
    }
    let named_otherwise =
        vec![Entry { name: "/p.xml", xml: true, bytes: b"<p/>" }, Entry { name: "/t.xml", xml: true, bytes: br#"<v:settings xmlns:v="urn:v"/>"# }];
    assert_eq!(diff_with(&RULES, &base, &named_otherwise).count, 1, "a part of another name is a part");
    // A kind with no children lists none: its root must hold nothing at all.
    let bare = Rules { parts_absent_when_empty: &[EmptyPart { named: settings_named, root: ROOT, children: &[] }], ..RULES };
    assert!(!diff_with(&bare, &base, &with_settings(r#"<v:settings xmlns:v="urn:v"><v:items/></v:settings>"#)).same());
}
