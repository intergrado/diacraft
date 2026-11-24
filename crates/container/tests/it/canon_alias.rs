//! Spellings of one name in the container's content check (APPID, `Rules::aliases`): a namespace that a format's earlier writers wrote reads as the one it writes now, wherever it stands
//! in a part, on both sides, and every other difference is still one. The vocabulary is this file's own: the lists of the formats are in `crates/odg/tests/it/` and `crates/vsdx/tests/it/`.

use diacraft_container::canon::{ContentDiff, Entry, Found, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};

const OLD: &str = "urn:example:old-extension";
const NEW: &str = "urn:example:ext";

const RULES: Rules = Rules { aliases: &[(OLD, NEW)], ..Rules::NONE };
const ORIENTED: Rules = Rules { aliases: &[(OLD, NEW)], ignored_when_extra: &[NEW], ..Rules::NONE };

fn compare_with(rules: &Rules, a: &str, b: &str) -> ContentDiff {
    let (ea, eb) = ([Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }], [Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }]);
    compare_entries(&ea, &eb, false, rules, &XmlLimits::default(), &mut Work::default()).unwrap()
}

fn compare(a: &str, b: &str) -> ContentDiff {
    compare_with(&RULES, a, b)
}

#[track_caller]
fn same_both_ways(a: &str, b: &str) {
    for (x, y) in [(a, b), (b, a)] {
        let d = compare(x, y);
        assert!(d.same(), "{x} against {y}: {:?}", d.shown);
    }
}

#[track_caller]
fn differ_both_ways(a: &str, b: &str) -> ContentDiff {
    let (ab, ba) = (compare(a, b), compare(b, a));
    assert!(!ab.same() && !ba.same(), "{a} against {b} called the same");
    assert_eq!(ab.count, ba.count, "the count does not depend on which is first");
    ab
}

#[test]
fn a_namespace_in_its_earlier_spelling_is_the_one_written_now_in_a_name_a_value_and_a_text() {
    // An element and an attribute of the namespace, a prefix for each spelling, the default namespace, a value that holds the spelling and a text that holds it.
    same_both_ways(&format!("<a xmlns:x=\"{OLD}\"><x:e x:k=\"v\"/></a>"), &format!("<a xmlns:y=\"{NEW}\"><y:e y:k=\"v\"/></a>"));
    same_both_ways(&format!("<e xmlns=\"{OLD}\"/>"), &format!("<e xmlns=\"{NEW}\"/>"));
    same_both_ways(&format!("<a t=\"{OLD}:blob\"/>"), &format!("<a t=\"{NEW}:blob\"/>"));
    same_both_ways(&format!("<a>{OLD}</a>"), &format!("<a>{NEW}</a>"));
    // Each spelling is the other where both stand in one part, and a part of one namespace and a part of both are the same.
    same_both_ways(&format!("<a xmlns:x=\"{OLD}\" xmlns:y=\"{NEW}\"><x:e/><y:e/></a>"), &format!("<a xmlns:y=\"{NEW}\"><y:e/><y:e/></a>"));
}

#[test]
fn every_other_difference_is_still_one_and_a_namespace_that_is_neither_is_not_read_as_either() {
    let d = differ_both_ways(&format!("<a xmlns:x=\"{OLD}\"><x:e x:k=\"v\"/></a>"), &format!("<a xmlns:y=\"{NEW}\"><y:e y:k=\"w\"/></a>"));
    assert_eq!((d.count, d.shown.first().map(|x| x.found)), (1, Some(Found::Value)));
    differ_both_ways(&format!("<a xmlns:x=\"{OLD}\"><x:e/></a>"), &format!("<a xmlns:y=\"{NEW}\"><y:f/></a>"));
    differ_both_ways(&format!("<a xmlns:x=\"{OLD}\"><x:e/></a>"), "<a xmlns:y=\"urn:example:other\"><y:e/></a>");
    // A spelling that only looks like the earlier one is a text of its own.
    differ_both_ways(&format!("<a t=\"{OLD}\"/>"), "<a t=\"urn:example:old-extensio\"/>");
    // A part with no alias in it is compared as ever, and so are two that are the same bytes.
    assert!(compare("<a/>", "<a/>").same());
    differ_both_ways("<a/>", "<b/>");
}

#[test]
fn a_part_that_is_not_xml_or_is_not_text_is_compared_as_it_was() {
    let bytes = |a: &[u8], b: &[u8], xml: bool| {
        compare_entries(
            &[Entry { name: "/p.bin", xml, bytes: a }],
            &[Entry { name: "/p.bin", xml, bytes: b }],
            false,
            &RULES,
            &XmlLimits::default(),
            &mut Work::default(),
        )
        .unwrap()
    };
    let (old, new) = (format!("<a t=\"{OLD}\"/>").into_bytes(), format!("<a t=\"{NEW}\"/>").into_bytes());
    assert!(!bytes(&old, &new, false).same(), "a part that is not meant as XML is bytes: the spelling in it is the file's");
    // UTF-16 text is no UTF-8: the rule reads nothing in it, and the parts that differ by the spelling differ.
    let wide = |text: &str| text.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    assert!(!bytes(&wide(&format!("<a t=\"{OLD}\"/>")), &wide(&format!("<a t=\"{NEW}\"/>")), true).same());
}

#[test]
fn a_pair_whose_second_spelling_is_longer_is_not_applied_and_an_empty_first_is_none() {
    const GROWS: Rules = Rules { aliases: &[("u:a", "urn:example:longer"), ("", "x")], ..Rules::NONE };
    let d = compare_with(&GROWS, "<a t=\"u:a\"/>", "<a t=\"urn:example:longer\"/>");
    assert!(!d.same(), "a copy of a part is never larger than the part");
    assert!(compare_with(&GROWS, "<a t=\"u:a\"/>", "<a t=\"u:a\"/>").same());
}

#[test]
fn the_rule_composes_with_the_one_that_orients_the_check() {
    // What the first document holds of the namespace must be in the second, and what only the second holds is nothing: in either spelling.
    let first = format!("<a xmlns:x=\"{OLD}\" x:k=\"v\"><x:e/></a>");
    let second = format!("<a xmlns:y=\"{NEW}\" y:k=\"v\" y:more=\"1\"><y:e/><y:extra/></a>");
    assert!(compare_with(&ORIENTED, &first, &second).same(), "the carrier of the source is in the written file, which has more of its own");
    assert!(!compare_with(&ORIENTED, &second, &first).same(), "what the source held and the written file lacks is a difference");
    let lost = format!("<a xmlns:y=\"{NEW}\"><y:e/></a>");
    assert!(!compare_with(&ORIENTED, &first, &lost).same(), "the attribute of the source is not in the second");
}

#[test]
fn the_pass_that_reads_the_spelling_costs_the_comparison_no_count_of_its_own() {
    // A part that does not hold the spelling: the counts are those of a format that lists none. And a pair that does: the counts of the comparison and of nothing else, as many steps
    // as the same two parts compared in one spelling would take.
    let plain = |n: usize| format!("<a>{}</a>", "<e k=\"v\"/>".repeat(n));
    let (a, b) = (plain(1000), plain(1000).replace("k=\"v\"", "k=\"w\""));
    let counts = |rules: &Rules, a: &str, b: &str| {
        let mut work = Work::default();
        let d = compare_entries(
            &[Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }],
            &[Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }],
            false,
            rules,
            &XmlLimits::default(),
            &mut work,
        )
        .unwrap();
        (d.count, work.canon_steps, work.canon_bytes)
    };
    assert_eq!(counts(&RULES, &a, &b), counts(&Rules::NONE, &a, &b), "no spelling in the parts, no cost");
    let part = |ns: &str| format!("<a xmlns:x=\"{ns}\">{}</a>", "<x:e/>".repeat(1000));
    let (old, new) = (part(OLD), part(NEW));
    assert_eq!(
        counts(&RULES, &old, &new),
        counts(&Rules::NONE, &new, &new),
        "a part in the earlier spelling is the part in the current one, and costs what that costs"
    );
}
