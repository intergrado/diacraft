//! An attribute that says again what another attribute of its element says (`Rules::twinned`): it counts as nothing when only the second document has it, beside an
//! equal twin. The check is oriented by it, as it is by `ignored_when_extra`; each case that lets a spelling pass is paired with a planted difference it still catches. The names are
//! this file's own: `x:id` is the attribute that says again what `y:id` says.

use diacraft_container::canon::{ContentDiff, Entry, Found, Name, Rules, Twin, compare_entries};
use diacraft_container::{Work, XmlLimits};

const X: &str = "urn:x";
const Y: &str = "urn:y";

const RULES: Rules = Rules { twinned: &[Twin { attr: Name { ns: X, local: "id" }, with: Name { ns: Y, local: "id" } }], ..Rules::NONE };

fn one(xml: &str) -> Vec<Entry<'_>> {
    vec![Entry { name: "/p.xml", xml: true, bytes: xml.as_bytes() }]
}

fn compare(rules: &Rules, a: &str, b: &str) -> ContentDiff {
    compare_entries(&one(a), &one(b), false, rules, &XmlLimits::default(), &mut Work::default()).unwrap()
}

/// The count of differences between the first and the second.
fn count(a: &str, b: &str) -> u64 {
    compare(&RULES, a, b).count
}

fn root(inner: &str) -> String {
    format!(r#"<r xmlns:x="{X}" xmlns:y="{Y}">{inner}</r>"#)
}

#[test]
fn a_twin_only_the_second_has_beside_an_equal_one_counts_as_nothing() {
    let alone = root(r#"<s y:id="a"/>"#);
    let pair = root(r#"<s x:id="a" y:id="a"/>"#);
    assert_eq!(count(&alone, &pair), 0, "the attribute the later version adds");
    // The same under other prefixes, and with the attributes in the other order.
    let other = format!(r#"<r xmlns:p="{X}" xmlns:q="{Y}"><s q:id="a" p:id="a"/></r>"#);
    assert_eq!(count(&alone, &other), 0);
    // Without the rule, it is an attribute on one side only.
    let d = compare(&Rules::NONE, &alone, &pair);
    assert_eq!((d.count, d.shown.iter().map(|x| x.found).collect::<Vec<_>>()), (1, vec![Found::Attribute]), "{:?}", d.shown);
}

#[test]
fn the_check_is_oriented_what_the_first_holds_is_in_the_second() {
    let alone = root(r#"<s y:id="a"/>"#);
    let pair = root(r#"<s x:id="a" y:id="a"/>"#);
    // The first has the pair and the second only the twin it is a twin of: the attribute is lost, which is a difference.
    let d = compare(&RULES, &pair, &alone);
    assert_eq!((d.count, d.shown.iter().map(|x| x.found).collect::<Vec<_>>()), (1, vec![Found::Attribute]), "{:?}", d.shown);
    // Both have the pair: the same.
    assert_eq!(count(&pair, &pair), 0);
    // Both have it, with the values the same on each side but another than its twin's: no twin, so it counts as the attribute it is, and the same on both sides is the same.
    let apart = root(r#"<s x:id="b" y:id="a"/>"#);
    assert_eq!(count(&apart, &apart), 0);
}

#[test]
fn a_twin_of_another_value_or_with_no_twin_beside_it_is_an_attribute_like_any() {
    let alone = root(r#"<s y:id="a"/>"#);
    // Another value than the twin's.
    let d = compare(&RULES, &alone, &root(r#"<s x:id="b" y:id="a"/>"#));
    assert_eq!((d.count, d.shown.iter().map(|x| x.found).collect::<Vec<_>>()), (1, vec![Found::Attribute]), "{:?}", d.shown);
    // No twin beside it (the element holds none, or the twin of another element does not count).
    let bare = root(r#"<s/>"#);
    assert_eq!(count(&bare, &root(r#"<s x:id="a"/>"#)), 1);
    assert_eq!(count(&root(r#"<s y:id="a"/><t/>"#), &root(r#"<s y:id="a"/><t x:id="a"/>"#)), 1);
    // The name is read through the declarations in force: the same local name of another namespace is none.
    let elsewhere = format!(r#"<r xmlns:x="{X}" xmlns:y="{Y}" xmlns:z="urn:z"><s z:id="a" y:id="a"/></r>"#);
    assert_eq!(count(&alone, &elsewhere), 1);
    // The value is compared as written: `a` and `a ` are two values.
    assert_eq!(count(&alone, &root(r#"<s x:id="a " y:id="a"/>"#)), 1);
}

#[test]
fn the_twin_is_left_out_of_what_aligns_the_children() {
    // Two children that differ only in the twin are the same child, so a difference in a third is found where it stands and nothing is moved.
    let first = root(r#"<s y:id="a"/><s y:id="b" k="1"/>"#);
    let second = root(r#"<s x:id="a" y:id="a"/><s x:id="b" y:id="b" k="2"/>"#);
    let d = compare(&RULES, &first, &second);
    assert_eq!((d.count, d.shown.iter().map(|x| x.found).collect::<Vec<_>>()), (1, vec![Found::Value]), "{:?}", d.shown);
    assert_eq!(d.shown[0].path, "/r[0]/s[1]/@k");
}
