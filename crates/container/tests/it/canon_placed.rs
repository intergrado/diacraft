//! The rule of that is the container's (class F7): a child that names its own place by an attribute, and says nothing in one place,
//! is read as the count of the children of its name before it (`Rules::placed`). Each spelling the rule lets pass has a planted difference it still catches. The lists
//! are this file's own: the VSDX lists are tested in `crates/vsdx/tests/it/canon_implied_rows.rs`.

use diacraft_container::canon::{ContentDiff, Entry, Found, Name, Placed, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};

const fn n(local: &'static str) -> Name {
    Name { ns: "", local }
}

/// Under `s` (no namespace), a child `r` is placed by `i`, or named by `n`. The kind of a section is its `k`: `a` is placed always, `b` never.
const RULES: Rules = Rules {
    placed: &[Placed { parent: n("s"), kind: n("k"), child: n("r"), place: n("i"), named_by: n("n"), always: &["a"], never: &["b"] }],
    ..Rules::NONE
};

/// Both directions: the count is symmetric.
fn diff_with(rules: &Rules, a: &str, b: &str) -> ContentDiff {
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    let ab = compare_entries(&ea, &eb, true, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    let ba = compare_entries(&eb, &ea, true, rules, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

fn diff(a: &str, b: &str) -> ContentDiff {
    diff_with(&RULES, a, b)
}

#[track_caller]
fn same(a: &str, b: &str) {
    let d = diff(a, b);
    assert!(d.same(), "{a} against {b}: {:?}", d.shown);
}

#[track_caller]
fn differ(a: &str, b: &str, found: Found) {
    let d = diff(a, b);
    assert!(!d.same(), "{a} against {b}: the check is blind to it");
    assert_eq!(d.shown.first().map(|x| x.found), Some(found), "{a} against {b}: {:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ an absent place is the count before it

#[test]
fn a_child_with_no_place_is_the_one_whose_place_is_the_count_of_the_children_before_it() {
    same(r#"<s k="a"><r/><r/><r/></s>"#, r#"<s k="a"><r i="0"/><r i="1"/><r i="2"/></s>"#);
    // Any of them may be the one that says nothing.
    same(r#"<s k="a"><r i="0"/><r/><r i="2"/></s>"#, r#"<s k="a"><r i="0"/><r i="1"/><r i="2"/></s>"#);
    same(r#"<s k="a"><r/><r i="1"/></s>"#, r#"<s k="a"><r i="0"/><r/></s>"#);
    // The count is of the children, whatever places they say: a row that says 7 is one row before the next.
    same(r#"<s k="a"><r i="7"/><r/></s>"#, r#"<s k="a"><r i="7"/><r i="1"/></s>"#);
    // What a child holds is compared as ever.
    same(r#"<s k="a"><r><c/></r><r><c/></r></s>"#, r#"<s k="a"><r i="0"><c/></r><r i="1"><c/></r></s>"#);
    // Each parent counts for itself.
    same(r#"<q><s k="a"><r/><r/></s><s k="a"><r/></s></q>"#, r#"<q><s k="a"><r i="0"/><r i="1"/></s><s k="a"><r i="0"/></s></q>"#);
}

#[test]
fn a_place_that_disagrees_with_the_count_is_still_a_difference() {
    differ(r#"<s k="a"><r/><r/></s>"#, r#"<s k="a"><r i="0"/><r i="2"/></s>"#, Found::Value);
    differ(r#"<s k="a"><r/><r/></s>"#, r#"<s k="a"><r i="1"/><r i="1"/></s>"#, Found::Value);
    // The row that follows one that says 7 is still the second (place 1): not 8.
    differ(r#"<s k="a"><r i="7"/><r/></s>"#, r#"<s k="a"><r i="7"/><r i="8"/></s>"#, Found::Value);
    // A row of one section takes no count from another.
    differ(r#"<q><s k="a"><r/></s><s k="a"><r/></s></q>"#, r#"<q><s k="a"><r i="0"/></s><s k="a"><r i="1"/></s></q>"#, Found::Value);
    // A place another way written is text: `01` is not 1, `+1` is not 1, and a blank one is not an absent one.
    differ(r#"<s k="a"><r/><r/></s>"#, r#"<s k="a"><r i="0"/><r i="01"/></s>"#, Found::Value);
    differ(r#"<s k="a"><r/><r/></s>"#, r#"<s k="a"><r i="0"/><r i="+1"/></s>"#, Found::Value);
    differ(r#"<s k="a"><r/></s>"#, r#"<s k="a"><r i=""/></s>"#, Found::Value);
    // A child that is no row in the list is not given a place, and is not counted.
    differ(r#"<s k="a"><o/></s>"#, r#"<s k="a"><o i="0"/></s>"#, Found::Attribute);
    differ(r#"<q><r/></q>"#, r#"<q><r i="0"/></q>"#, Found::Attribute);
}

#[test]
fn a_child_of_another_namespace_is_not_a_row_and_is_not_counted() {
    same(r#"<s k="a" xmlns:y="urn:y"><y:r/><r/></s>"#, r#"<s k="a" xmlns:y="urn:y"><y:r/><r i="0"/></s>"#);
    differ(r#"<s k="a" xmlns:y="urn:y"><y:r/></s>"#, r#"<s k="a" xmlns:y="urn:y"><y:r i="0"/></s>"#, Found::Attribute);
    // Text and elements between rows count nothing.
    same(r#"<s k="a"><r/>text<o/><r/></s>"#, r#"<s k="a"><r i="0"/>text<o/><r i="1"/></s>"#);
}

// ------------------------------------------------------------------------------------------------------------ which sections

#[test]
fn a_kind_that_is_placed_always_places_a_row_that_has_a_name_too() {
    same(r#"<s k="a"><r n="x"/></s>"#, r#"<s k="a"><r n="x" i="0"/></s>"#);
}

#[test]
fn a_kind_that_is_never_placed_keeps_an_absent_place_absent() {
    differ(r#"<s k="b"><r n="x"/></s>"#, r#"<s k="b"><r n="x" i="0"/></s>"#, Found::Attribute);
    differ(r#"<s k="b"><r/></s>"#, r#"<s k="b"><r i="0"/></s>"#, Found::Attribute);
    // And a kind that says is the same as ever: two rows that say.
    same(r#"<s k="b"><r i="1"/></s>"#, r#"<s k="b"><r i="1"/></s>"#);
}

#[test]
fn a_kind_in_neither_list_places_a_row_with_no_name_and_not_one_that_has_one() {
    same(r#"<s k="c"><r/><r/></s>"#, r#"<s k="c"><r i="0"/><r i="1"/></s>"#);
    differ(r#"<s k="c"><r n="x"/></s>"#, r#"<s k="c"><r n="x" i="0"/></s>"#, Found::Attribute);
    // The row with a name is counted all the same: the unnamed row after it is the second.
    same(r#"<s k="c"><r n="x"/><r/></s>"#, r#"<s k="c"><r n="x"/><r i="1"/></s>"#);
    // A section that says no kind is of that kind, and so is one that says it in another case.
    same(r#"<s><r/></s>"#, r#"<s><r i="0"/></s>"#);
    differ(r#"<s><r n="x"/></s>"#, r#"<s><r n="x" i="0"/></s>"#, Found::Attribute);
    differ(r#"<s k="A"><r n="x"/></s>"#, r#"<s k="A"><r n="x" i="0"/></s>"#, Found::Attribute);
}

#[test]
fn a_rule_with_no_entry_changes_nothing() {
    let d = diff_with(&Rules::NONE, r#"<s k="a"><r/></s>"#, r#"<s k="a"><r i="0"/></s>"#);
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Attribute), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ what it costs

#[test]
fn many_rows_cost_a_few_steps_each() {
    let rows = 100_000usize;
    let a = format!(r#"<s k="a">{}</s>"#, "<r/>".repeat(rows));
    let b = format!(r#"<s k="a">{}</s>"#, (0..rows).map(|i| format!(r#"<r i="{i}"/>"#)).collect::<String>());
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    let mut work = Work::default();
    let d = compare_entries(&ea, &eb, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert!(d.same(), "{:?}", d.shown);
    assert!(work.canon_steps < (rows as u64) * 40, "{} steps for {rows} rows", work.canon_steps);
}
