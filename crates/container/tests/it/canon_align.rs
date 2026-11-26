//! The ordered children of an element are set side by side by what they are, and not by their place: an element one side has and the
//! other lacks is one difference, and every sibling after it is still compared with its own match, in depth. A masking control in each: a change planted
//! deep inside a sibling that comes after an insertion or a drop is counted.
//!
//! The lists are this file's own (`IDENTIFIED` names the attribute `id` as what says which thing an element is). The formats' lists are tested in their crates.

use diacraft_container::canon::test_support::with_hash_mask;
use diacraft_container::canon::{ContentDiff, Entry, Found, MAX_ALIGNED, Name, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};
use proptest::prelude::*;

/// No list at all: the children are in order, and what tells them apart is what they are called and what they hold.
const PLAIN: Rules = Rules::NONE;
/// The attribute `id` says which thing an element is (a page's or a shape's own name), so that siblings of one name are told apart by it.
const IDENTIFIED: Rules = Rules { identified_by: &[Name { ns: "", local: "id" }], ..Rules::NONE };
/// A part named `META-INF/manifest.xml` may begin with a DOCTYPE that holds only an external id.
const DOCTYPED: Rules = Rules { doctype_parts: &["META-INF/manifest.xml"], ..Rules::NONE };

fn run_with(rules: &Rules, a: &str, b: &str) -> (ContentDiff, Work) {
    let mut work = Work::default();
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    let d = compare_entries(&ea, &eb, true, rules, &XmlLimits { max_depth: 512, ..XmlLimits::default() }, &mut work).unwrap();
    (d, work)
}

/// The differences between `a` and `b` as `(kind, path)`, sorted, with the count; and the same the other way round must count the same.
fn seen_with(rules: &Rules, a: &str, b: &str) -> Vec<(&'static str, String)> {
    let (d, _) = run_with(rules, a, b);
    let (back, _) = run_with(rules, b, a);
    assert_eq!(d.count, back.count, "the count is symmetric: {a} against {b}");
    assert_eq!(d.count as usize, d.shown.len(), "all of them are shown: {:?}", d.shown);
    let mut out: Vec<(&'static str, String)> = d.shown.iter().map(|x| (x.found.word(), x.path.clone())).collect();
    out.sort();
    out
}

fn seen(a: &str, b: &str) -> Vec<(&'static str, String)> {
    seen_with(&PLAIN, a, b)
}

fn count_with(rules: &Rules, a: &str, b: &str) -> u64 {
    run_with(rules, a, b).0.count
}

fn at(found: Found, path: &str) -> (&'static str, String) {
    (found.word(), path.to_owned())
}

// ---------------------------------------------------------------------------------------------------------------- an inserted or a dropped child

#[test]
fn an_inserted_child_is_one_difference_and_the_siblings_after_it_are_compared_with_their_own() {
    // `q` and `s` changed, so that nothing about them is equal; `n` is the one element put in. The old reading set them side by side by place: `q` against `n`, `s`
    // against `q`, and `s` of the second against nothing, which says three elements and looks inside none of them.
    let (a, b) = (r#"<r><p/><q v="1"/><s w="1"/></r>"#, r#"<r><p/><n/><q v="2"/><s w="2"/></r>"#);
    let want = vec![at(Found::Element, "/r[0]/n[1]"), at(Found::Value, "/r[0]/q[1]/@v"), at(Found::Value, "/r[0]/s[2]/@w")];
    assert_eq!(seen(a, b), want);
}

#[test]
fn a_dropped_child_is_one_difference_and_the_siblings_after_it_are_compared_with_their_own() {
    let (a, b) = (r#"<r><p/><n/><q v="1"/><s w="1"/></r>"#, r#"<r><p/><q v="2"/><s w="2"/></r>"#);
    let want = vec![at(Found::Element, "/r[0]/n[1]"), at(Found::Value, "/r[0]/q[2]/@v"), at(Found::Value, "/r[0]/s[3]/@w")];
    assert_eq!(seen(a, b), want);
}

#[test]
fn a_change_planted_deep_inside_a_sibling_after_an_insertion_is_counted() {
    // The masking control: the second document with one more change, deep inside the last sibling, counts one more. (Set side by side by place, the last sibling
    // was compared with the one before it, and a change inside it was never reached.)
    let a = r#"<r><p/><q v="1"/><s w="1"><k x="0"><m y="0"/></k></s></r>"#;
    let b = r#"<r><p/><n/><q v="2"/><s w="2"><k x="0"><m y="0"/></k></s></r>"#;
    let planted = r#"<r><p/><n/><q v="2"/><s w="2"><k x="0"><m y="PLANTED"/></k></s></r>"#;
    let (base, plant) = (count_with(&PLAIN, a, b), count_with(&PLAIN, a, planted));
    assert_eq!(base, 3);
    assert_eq!(plant, base + 1, "the plant after the insertion is counted");
    assert_eq!(count_with(&PLAIN, planted, a), plant, "and the same the other way round");
    // And it is found where it is.
    assert!(seen(a, planted).contains(&at(Found::Value, "/r[0]/s[2]/k[0]/m[0]/@y")));
}

#[test]
fn one_element_put_in_at_any_place_among_changed_siblings_is_one_difference_each_sibling_is_compared_in_depth() {
    // Five siblings, each of its own name and each changed, and one more put in at every place from the first to the last: the insertion is one, and each
    // sibling's change is one.
    let list = |salt: bool, put: Option<usize>| {
        let mut s = String::from("<r>");
        for i in 0..=5 {
            if put == Some(i) {
                s.push_str("<new/>");
            }
            if i < 5 {
                s.push_str(&format!("<s{i} v=\"{}\"><k/></s{i}>", if salt { "salted" } else { "plain" }));
            }
        }
        s.push_str("</r>");
        s
    };
    for put in 0..=5 {
        let found = seen(&list(false, None), &list(true, Some(put)));
        assert_eq!(found.len(), 6, "put in at {put}: {found:?}");
        assert_eq!(found.iter().filter(|(kind, _)| *kind == "value").count(), 5, "put in at {put}: {found:?}");
        assert_eq!(found.iter().filter(|(kind, _)| *kind == "element").count(), 1, "put in at {put}: {found:?}");
    }
}

#[test]
fn two_insertions_and_a_drop_are_three_differences_and_the_changes_between_them_are_found() {
    let a = r#"<r><a v="1"/><b v="1"/><c v="1"/><d v="1"/><e v="1"/></r>"#;
    let b = r#"<r><x/><a v="2"/><c v="2"/><y/><d v="2"/><e v="2"/></r>"#;
    let found = seen(a, b);
    // x and y put in, b taken out, and four values changed.
    assert_eq!(found.iter().filter(|(kind, _)| *kind == "element").count(), 3, "{found:?}");
    assert_eq!(found.iter().filter(|(kind, _)| *kind == "value").count(), 4, "{found:?}");
    assert_eq!(found.len(), 7);
}

#[test]
fn a_replaced_child_is_still_one_difference() {
    // `b` replaced by `x`: as before, one difference at the place, and nothing else moves.
    assert_eq!(seen("<r><a/><b/><c/></r>", "<r><a/><x/><c/></r>"), vec![at(Found::Element, "/r[0]/b[1]")]);
    assert_eq!(seen(r#"<r><a v="1"/><b/><c v="1"/></r>"#, r#"<r><a v="2"/><x/><c v="2"/></r>"#).len(), 3);
}

// -------------------------------------------------------------------------------------------------------------------------- what is told by its id

#[test]
fn siblings_of_one_name_are_told_apart_by_the_attribute_that_says_which_they_are() {
    // Two pages, both changed, and one put in before them: with `id` as what a page is, each page is compared with its own.
    let a = r#"<r><page id="A" v="1"><s/></page><page id="B" v="1"><s/></page></r>"#;
    let b = r#"<r><page id="N" v="0"/><page id="A" v="2"><s/></page><page id="B" v="2"><s/></page></r>"#;
    let want = vec![at(Found::Element, "/r[0]/page[0]"), at(Found::Value, "/r[0]/page[0]/@v"), at(Found::Value, "/r[0]/page[1]/@v")];
    assert_eq!(seen_with(&IDENTIFIED, a, b), want);
}

#[test]
fn an_identity_that_two_siblings_share_is_not_told() {
    // Two `page`s of the one id on each side and one more on the second: nothing says which is which, so they are set side by side by place, and the
    // count is the same either way round.
    let (a, b) = (r#"<r><page id="A" v="1"/><page id="A" v="2"/></r>"#, r#"<r><page id="A" v="3"/><page id="A" v="4"/><page id="A" v="5"/></r>"#);
    let (ab, ba) = (count_with(&IDENTIFIED, a, b), count_with(&IDENTIFIED, b, a));
    assert_eq!(ab, ba);
    assert!(ab >= 3);
}

// ---------------------------------------------------------------------------------------------------------------------------- order still counts

#[test]
fn the_same_children_in_another_order_are_one_order() {
    only_order("<r><x/><y/></r>", "<r><y/><x/></r>");
    only_order("<r><a/><b/><c/><d/></r>", "<r><a/><c/><d/><b/></r>");
    only_order(r#"<r><s ID="1"/><s ID="2"/><s ID="3"/></r>"#, r#"<r><s ID="3"/><s ID="1"/><s ID="2"/></r>"#);
    only_order("<r><x/><y/><z/><w/></r>", "<r><w/><z/><y/><x/></r>");
}

fn only_order(a: &str, b: &str) {
    assert_eq!(seen(a, b), vec![at(Found::Order, "/r[0]")], "{a} against {b}");
}

#[test]
fn a_reorder_and_a_change_inside_a_moved_child_are_one_order_and_one_value() {
    // The children are still compared in depth when they stand in another order: one order, and the change inside `x`.
    let found = seen(r#"<r><x v="1"/><y/></r>"#, r#"<r><y/><x v="2"/></r>"#);
    assert_eq!(found, vec![at(Found::Order, "/r[0]"), at(Found::Value, "/r[0]/x[0]/@v")]);
    // With an element put in beside them, still one order, one value and one element.
    let found = seen(r#"<r><x v="1"/><y/><z/></r>"#, r#"<r><y/><n/><x v="2"/><z/></r>"#);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(found.iter().filter(|(kind, _)| *kind == "order").count(), 1, "{found:?}");
}

#[test]
fn a_reorder_is_one_order_however_many_children_cross() {
    // A hundred siblings of one name and distinct ids, put in the reverse order: one difference.
    let list = |reverse: bool| {
        let mut ids: Vec<usize> = (0..100).collect();
        if reverse {
            ids.reverse();
        }
        let mut s = String::from("<r>");
        for i in ids {
            s.push_str(&format!("<g id=\"{i}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    assert_eq!(seen_with(&IDENTIFIED, &list(false), &list(true)), vec![at(Found::Order, "/r[0]")]);
    // Each, as a shape in the drawing's order, changed as well: the order and the hundred changes.
    let changed = |reverse: bool, salt: bool| {
        let mut s = list(reverse);
        if salt {
            s = s.replace("/>", " v=\"1\"/>");
        }
        s
    };
    assert_eq!(count_with(&IDENTIFIED, &changed(false, false), &changed(true, true)), 101);
}

// ----------------------------------------------------------------------------------------------------------------------------- symmetry, in general

/// An element of a generated list: its name (one of three), its `id` (one of three or none) and its `v` (one of three or none).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    name: u8,
    id: Option<u8>,
    v: Option<u8>,
}

impl Item {
    fn xml(&self) -> String {
        let id = self.id.map(|i| format!(" id=\"{i}\"")).unwrap_or_default();
        let v = self.v.map(|v| format!(" v=\"{v}\"")).unwrap_or_default();
        format!("<{}{id}{v}/>", ["a", "b", "c"].get(usize::from(self.name)).copied().unwrap_or("a"))
    }
}

fn items() -> impl Strategy<Value = Vec<Item>> {
    prop::collection::vec((0u8..3, prop::option::of(0u8..3), prop::option::of(0u8..3)).prop_map(|(name, id, v)| Item { name, id, v }), 0..9)
}

fn document(items: &[Item]) -> String {
    format!("<r>{}</r>", items.iter().map(Item::xml).collect::<String>())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 600, ..ProptestConfig::default() })]

    /// Two lists of elements: the count is the same either way round, a list is the same as itself and as no other list, and so it is with the hash cut to a few bits,
    /// where unrelated children collide (the hash only aligns: every pair it sets side by side is compared in full).
    #[test]
    fn the_count_is_symmetric_and_a_list_differs_from_every_other(a in items(), b in items()) {
        let (xa, xb) = (document(&a), document(&b));
        for mask in [u64::MAX, 0, 1, 0xF] {
            with_hash_mask(mask, || {
                for rules in [&PLAIN, &IDENTIFIED] {
                    let (ab, ba) = (count_with(rules, &xa, &xb), count_with(rules, &xb, &xa));
                    prop_assert_eq!(ab, ba, "mask {:#x}:\n{}\n{}", mask, xa, xb);
                    prop_assert_eq!(count_with(rules, &xa, &xa), 0);
                    prop_assert_eq!(ab == 0, a == b, "mask {:#x}:\n{}\n{}", mask, xa, xb);
                }
                Ok(())
            })?;
        }
    }
}

/// What is done to a list of elements of distinct names (`e0`, `e1`, ...): some are taken out, the rest changed (each gets a new value, so nothing about it is equal),
/// one new element is put in, and the ones that are kept are put in the reverse order or not.
#[derive(Clone, Debug)]
struct Plan {
    /// How many elements the first document has.
    n: usize,
    /// For each: its new value, or none when it is taken out in the second.
    kept: Vec<Option<u8>>,
    /// Where the new element is put in (at the end, when past the end).
    put: usize,
    /// Whether the elements that are kept are in the reverse order in the second.
    reversed: bool,
}

fn plans() -> impl Strategy<Value = Plan> {
    (2usize..9).prop_flat_map(|n| {
        (Just(n), prop::collection::vec(prop::option::weighted(0.75, 1u8..3), n), 0..=n, any::<bool>()).prop_map(|(n, kept, put, reversed)| Plan {
            n,
            kept,
            put,
            reversed,
        })
    })
}

impl Plan {
    fn first(&self) -> String {
        format!("<r>{}</r>", (0..self.n).map(|i| format!("<e{i} v=\"0\"/>")).collect::<String>())
    }

    /// The second document, with the new element put in when `fresh`.
    fn second(&self, fresh: bool) -> String {
        let mut items: Vec<String> = self.kept.iter().enumerate().filter_map(|(i, v)| v.map(|v| format!("<e{i} v=\"{v}\"/>"))).collect();
        if self.reversed {
            items.reverse();
        }
        if fresh {
            items.insert(self.put.min(items.len()), "<fresh/>".to_owned());
        }
        format!("<r>{}</r>", items.concat())
    }

    fn kept(&self) -> usize {
        self.kept.iter().flatten().count()
    }

    /// The order difference: the kept elements that are in the reverse order, when there are two to be.
    fn order(&self) -> usize {
        usize::from(self.reversed && self.kept() >= 2)
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 600, ..ProptestConfig::default() })]

    /// Elements of distinct names, one put in anywhere and each of the others changed, in the order they were or the reverse: the insertion is one difference, each
    /// change is one, and the order is one however many children cross. Wherever they stand, and whatever stands around them.
    #[test]
    fn what_was_put_in_and_changed_is_what_the_count_says(plan in plans()) {
        // Nothing is taken out here: every element is kept.
        let plan = Plan { kept: plan.kept.iter().map(|v| Some(v.unwrap_or(1))).collect(), ..plan };
        let (first, second) = (plan.first(), plan.second(true));
        let want = (1 + plan.kept() + plan.order()) as u64;
        for rules in [&PLAIN, &IDENTIFIED] {
            prop_assert_eq!(count_with(rules, &first, &second), want, "\n{}\n{}", first, second);
            prop_assert_eq!(count_with(rules, &second, &first), want, "\n{}\n{}", second, first);
        }
    }

    /// Some taken out and the rest changed and in the reverse order or not, and nothing put in: each of those that goes is one difference, each change is one, and the
    /// order is one.
    #[test]
    fn what_was_taken_out_and_changed_is_what_the_count_says(plan in plans()) {
        let (first, second) = (plan.first(), plan.second(false));
        let want = ((plan.n - plan.kept()) + plan.kept() + plan.order()) as u64;
        for rules in [&PLAIN, &IDENTIFIED] {
            prop_assert_eq!(count_with(rules, &first, &second), want, "\n{}\n{}", first, second);
            prop_assert_eq!(count_with(rules, &second, &first), want, "\n{}\n{}", second, first);
        }
    }

    /// Both: one put in and some taken out. An element that goes and the one that comes in, in one place with nothing kept between them, are one replaced element, as
    /// they always were, so the count is the sum less at most one.
    #[test]
    fn what_was_put_in_and_what_was_taken_out_are_at_most_one_replaced_element(plan in plans()) {
        let (first, second) = (plan.first(), plan.second(true));
        let sum = (1 + (plan.n - plan.kept()) + plan.kept() + plan.order()) as u64;
        for rules in [&PLAIN, &IDENTIFIED] {
            let (ab, ba) = (count_with(rules, &first, &second), count_with(rules, &second, &first));
            prop_assert_eq!(ab, ba, "\n{}\n{}", first, second);
            let dropped = plan.n > plan.kept();
            prop_assert!(ab == sum || (dropped && ab + 1 == sum), "{} against the sum {}:\n{}\n{}", ab, sum, first, second);
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------------- cost, counted

/// A list of `n` siblings of one name, each with an id, each with its own value, and the same with `new` put in first and every value changed.
fn pages(n: usize, changed: bool, put_first: bool) -> String {
    let mut s = String::with_capacity(n * 24 + 32);
    s.push_str("<r>");
    if put_first {
        s.push_str("<page id=\"fresh\"/>");
    }
    for i in 0..n {
        s.push_str(&format!("<page id=\"{i}\" v=\"{}\"><s/></page>", if changed { "b" } else { "a" }));
    }
    s.push_str("</r>");
    s
}

#[test]
fn a_hundred_thousand_siblings_with_one_put_in_are_aligned_in_linear_steps_times_the_log() {
    let n = 100_000usize;
    let (a, b) = (pages(n, false, false), pages(n, true, true));
    let (d, work) = run_with(&IDENTIFIED, &a, &b);
    assert_eq!(d.count, 1 + n as u64, "one put in and a value changed in each");
    assert_eq!(work.canon_by_place, 0, "aligned, not set side by side by place");
    // Measured: about 10.8 steps for each sibling (an element and its child cost 2 and 3 to build, the rest is the alignment's three sorts).
    // Measured: 12.04 million steps, 120 for each sibling: 25 to build and compare a page and its child on both sides, and 95 for the three sorts of the alignment (the hashes
    // and the ids, each of 100,000 on each side, and the pairs), which is 5.7 times the siblings and the bits of their number.
    assert!(work.canon_steps <= 130 * n as u64, "{} steps for {n} siblings", work.canon_steps);
    // And the work does not grow faster than the siblings: twice as many cost twice as much, within a tenth.
    let (a2, b2) = (pages(2 * n, false, false), pages(2 * n, true, true));
    let (_, work2) = run_with(&IDENTIFIED, &a2, &b2);
    assert!(work2.canon_steps <= 2 * work.canon_steps + work.canon_steps / 5 + 1, "{} then {}", work.canon_steps, work2.canon_steps);
}

#[test]
fn a_hundred_thousand_siblings_that_no_name_or_id_tells_apart_are_set_side_by_side_by_place_in_linear_steps() {
    let n = 100_000usize;
    let list = |v: &str, put: bool| format!("<r>{}{}</r>", if put { "<e/>" } else { "" }, format!("<e v=\"{v}\"/>").repeat(n));
    let (d, work) = run_with(&PLAIN, &list("a", false), &list("b", true));
    assert!(d.count >= n as u64);
    // Measured: 2.1 million steps, 21 for each sibling: nothing matches by its content and every one has the name `e`, so they are set side by side by place.
    assert!(work.canon_steps <= 23 * n as u64, "{} steps for {n} siblings", work.canon_steps);
}

#[test]
fn a_hundred_thousand_siblings_in_the_reverse_order_are_one_order() {
    let n = 100_000usize;
    let list = |reverse: bool| {
        let mut ids: Vec<usize> = (0..n).collect();
        if reverse {
            ids.reverse();
        }
        let mut s = String::from("<r>");
        for i in ids {
            s.push_str(&format!("<g id=\"{i}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    let (d, work) = run_with(&IDENTIFIED, &list(false), &list(true));
    assert_eq!(d.count, 1);
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Order));
    // Measured: 7.33 million steps, 73 for each sibling: the sorts of 100,000 hashes on each side, and of the pairs, which are in the reverse order.
    assert!(work.canon_steps <= 80 * n as u64, "{} steps for {n} siblings", work.canon_steps);
}

#[test]
fn siblings_with_two_hash_classes_and_a_hundred_thousand_of_each_are_aligned_in_linear_steps_times_the_log() {
    // Two kinds of element, alternately, with fifty thousand of each, one put in first and the last one changed: so that nothing at the start or the end matches and the whole list
    // is the middle, in which the classes the alignment sorts by are as large as they can be.
    let n = 100_000usize;
    let list = |put: bool| {
        let mut s = String::from("<r>");
        if put {
            s.push_str("<z/>");
        }
        for i in 0..n {
            s.push_str(if i % 2 == 0 { "<x v=\"1\"/>" } else { "<y v=\"1\"/>" });
        }
        s.push_str(if put { "<w v=\"2\"/></r>" } else { "<w v=\"1\"/></r>" });
        s
    };
    let (d, work) = run_with(&PLAIN, &list(false), &list(true));
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert_eq!(work.canon_by_place, 0);
    // Measured: 7.27 million steps, 73 for each sibling: the sorts of 100,000 hashes on each side and of the pairs, with only three different hashes among them.
    assert!(work.canon_steps <= 80 * n as u64, "{} steps for {n} siblings", work.canon_steps);
}

#[test]
fn a_chain_of_levels_each_with_one_put_in_costs_what_the_levels_hold() {
    // Five hundred levels (the parser's ceiling is 512, and a test thread has 2 MiB of stack), each with four siblings (the next level, and three changed ones) and one put in before them: each level is aligned alone.
    let depth = 500usize;
    let build = |changed: bool| {
        let mut s = String::new();
        for _ in 0..depth {
            s.push_str(&format!(
                "<g>{}<a v=\"{}\"/><b v=\"{}\"/><c v=\"{}\"/>",
                if changed { "<fresh/>" } else { "" },
                u8::from(changed),
                u8::from(changed),
                u8::from(changed)
            ));
        }
        for _ in 0..depth {
            s.push_str("</g>");
        }
        s
    };
    // Each `<g>` holds its own siblings and then the next `<g>` as its last child.
    let (d, work) = run_with(&IDENTIFIED, &build(false), &build(true));
    // Each level: the one put in and three values; the innermost level's three children have no next level.
    assert_eq!(d.count, 4 * depth as u64, "{:?}", d.shown.first());
    // Measured: 64,104 steps, 128 for each level (the levels hold six elements and their attributes, and each is aligned alone).
    assert!(work.canon_steps <= 135 * depth as u64, "{} steps for {depth} levels", work.canon_steps);
}

// -------------------------------------------------------------------------------------------------------- past the cap, set side by side by place

#[test]
fn siblings_past_the_cap_are_set_side_by_side_by_place_and_counted() {
    // One more than the cap on each side, none of them alike: the old reading, by place, and the work counted. Never "the same".
    let n = MAX_ALIGNED + 1;
    let build = |changed: bool| {
        let mut s = String::with_capacity(n * 16);
        s.push_str("<r>");
        for i in 0..n {
            s.push_str(&format!("<p id=\"{i}\" v=\"{}\"/>", u8::from(changed)));
        }
        s.push_str("</r>");
        s
    };
    let (d, work) = run_with(&IDENTIFIED, &build(false), &build(true));
    assert_eq!(d.count, n as u64, "each is compared with the one in its place, and each differs");
    assert!(work.canon_by_place >= 2 * n as u64, "{} siblings counted", work.canon_by_place);
    // At the cap itself, every sibling is aligned.
    let at_cap = |changed: bool| {
        let mut s = String::from("<r>");
        for i in 0..MAX_ALIGNED {
            s.push_str(&format!("<p id=\"{i}\" v=\"{}\"/>", u8::from(changed)));
        }
        s.push_str("</r>");
        s
    };
    let (d, work) = run_with(&IDENTIFIED, &at_cap(false), &at_cap(true));
    assert_eq!(d.count, MAX_ALIGNED as u64);
    assert_eq!(work.canon_by_place, 0);
}

// ---------------------------------------------------------------------------------------------------------------------- a DOCTYPE is no content

const MANIFEST: &str =
    r#"<manifest:manifest xmlns:manifest="urn:m"><manifest:file-entry manifest:full-path="/" manifest:media-type="x"/></manifest:manifest>"#;
const DOCTYPE: &str = r#"<!DOCTYPE manifest:manifest PUBLIC "-//OpenOffice.org//DTD Manifest 1.0//EN" "Manifest.dtd">"#;

fn part_diff(name: &str, a: &str, b: &str) -> ContentDiff {
    let (ea, eb) = ([Entry { name, xml: true, bytes: a.as_bytes() }], [Entry { name, xml: true, bytes: b.as_bytes() }]);
    compare_entries(&ea, &eb, false, &DOCTYPED, &XmlLimits::default(), &mut Work::default()).unwrap()
}

#[test]
fn a_manifest_with_a_doctype_is_read_as_one_without_it() {
    let with = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{DOCTYPE}\n{MANIFEST}");
    let without = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{MANIFEST}");
    for (a, b) in [(&with, &without), (&without, &with), (&with, &with)] {
        let d = part_diff("META-INF/manifest.xml", a, b);
        assert!(d.same(), "{:?}", d.shown);
    }
    // A difference inside it is a value and not bytes, wherever the DOCTYPE stands.
    let changed = MANIFEST.replace("media-type=\"x\"", "media-type=\"y\"");
    for (a, b) in [(format!("{DOCTYPE}{MANIFEST}"), changed.clone()), (MANIFEST.to_owned(), format!("{DOCTYPE}{changed}"))] {
        let d = part_diff("META-INF/manifest.xml", &a, &b);
        assert_eq!(
            d.shown.iter().map(|x| (x.found, x.path.as_str())).collect::<Vec<_>>(),
            [(Found::Value, "/manifest[0]/file-entry[0]/@media-type")]
        );
    }
}

#[test]
fn a_doctype_is_read_only_where_the_format_says_and_only_when_it_holds_no_declaration() {
    let with = format!("{DOCTYPE}{MANIFEST}");
    // In a part the format does not name, a DOCTYPE is not read: the parts are compared as bytes.
    let d = part_diff("content.xml", &with, MANIFEST);
    assert_eq!(d.shown.iter().map(|x| x.found).collect::<Vec<_>>(), [Found::Bytes]);
    // An internal subset declares things: it is not read in the manifest either, and the part is bytes, as it always was.
    let subset = format!("<!DOCTYPE manifest:manifest [<!ENTITY e \"x\">]>{MANIFEST}");
    let d = part_diff("META-INF/manifest.xml", &subset, MANIFEST);
    assert_eq!(d.shown.iter().map(|x| x.found).collect::<Vec<_>>(), [Found::Bytes]);
    // A second DOCTYPE, one after the root, and a DOCTYPE written in lower case are not read.
    for bad in
        [format!("{DOCTYPE}{DOCTYPE}{MANIFEST}"), format!("{MANIFEST}{DOCTYPE}"), format!("<!doctype manifest:manifest SYSTEM \"m.dtd\">{MANIFEST}")]
    {
        let d = part_diff("META-INF/manifest.xml", &bad, MANIFEST);
        assert_eq!(d.shown.iter().map(|x| x.found).collect::<Vec<_>>(), [Found::Bytes], "{bad}");
    }
    // No entity is expanded because a DOCTYPE was accepted.
    let referring = format!("{DOCTYPE}{}", MANIFEST.replace("media-type=\"x\"", "media-type=\"&e;\""));
    let d = part_diff("META-INF/manifest.xml", &referring, MANIFEST);
    assert_eq!(d.shown.iter().map(|x| x.found).collect::<Vec<_>>(), [Found::Bytes]);
}
