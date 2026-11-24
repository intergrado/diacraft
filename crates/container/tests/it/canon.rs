//! The content check (`diacraft_container::canon`): every rule that lets two spellings pass is paired with a planted difference it still
//! catches, and the check itself is proved by planted differences, planted reformatting and an echo control (design section 4.3, "The
//! checker is itself proved"). The lists are this file's own; the formats' lists are tested in their crates.

use diacraft_container::canon::test_support::with_hash_mask;
use diacraft_container::canon::{
    CanonError, ContentDiff, Difference, Entry, Found, KeyFold, Keyed, MAX_COMPARE_BYTES, MAX_COMPARE_ITEMS, MAX_QUALIFIED_BYTES, MAX_SHOWN, Name,
    ODF_TEXT_NAMESPACE, QKind, Qualified, Rules, compare_entries, same_length, same_number,
};
use diacraft_container::{Work, XmlLimits};

const T: &str = ODF_TEXT_NAMESPACE;
const NAME_N: &[Name] = &[Name { ns: "", local: "n" }];

/// A test's own lists: `c` and `d` keyed by `n`, `len` a number, ODF's paragraph elements, and `t` mixed content.
const RULES: Rules = Rules {
    keyed: &[
        Keyed { child: Name { ns: "", local: "c" }, key: NAME_N, fold: KeyFold::Exact },
        Keyed { child: Name { ns: "", local: "d" }, key: NAME_N, fold: KeyFold::Exact },
    ],
    numbers: &[Name { ns: "", local: "len" }],
    lengths: &[],
    qnames: &[],
    paragraphs: &[Name { ns: T, local: "p" }, Name { ns: T, local: "h" }, Name { ns: T, local: "span" }],
    exact_text: &[Name { ns: "", local: "t" }],
    absent_when_empty: &[],
    absent_when_empty_under: &[],
    unordered: &[],
    parts_absent_when_empty: &[],
    ignored_attributes: &[],
    ignored_children: &[],
    ignored_when_extra: &[],
    free_names: &[],
    placed: &[],
    aliases: &[],
};

fn entry<'a>(name: &'a str, xml: bool, bytes: &'a [u8]) -> Entry<'a> {
    Entry { name, xml, bytes }
}

/// The comparison of two parts that are XML, named `/p.xml`, under `rules`.
fn diff_with(a: &str, b: &str, rules: &Rules) -> ContentDiff {
    diff_limited(a, b, rules, &XmlLimits::default(), &mut Work::default())
}

fn diff_limited(a: &str, b: &str, rules: &Rules, limits: &XmlLimits, work: &mut Work) -> ContentDiff {
    let (ea, eb) = ([entry("/p.xml", true, a.as_bytes())], [entry("/p.xml", true, b.as_bytes())]);
    compare_entries(&ea, &eb, true, rules, limits, work).unwrap()
}

/// Limits that let a tree be as deep as the parser's ceiling.
fn deep_limits() -> XmlLimits {
    XmlLimits { max_depth: diacraft_container::limits::XML_DEPTH_CEILING, ..XmlLimits::default() }
}

fn diff(a: &str, b: &str) -> ContentDiff {
    diff_with(a, b, &RULES)
}

fn same(a: &str, b: &str) -> bool {
    let (ab, ba) = (diff(a, b), diff(b, a));
    assert_eq!(ab.count, ba.count, "the count is symmetric: {a} against {b}");
    ab.same()
}

/// One difference, at `path`, of kind `found`, in `/p.xml`, and no other.
fn only(d: &ContentDiff, found: Found, path: &str) {
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown, [Difference { part: "/p.xml".to_owned(), path: path.to_owned(), found }]);
}

// ------------------------------------------------------------------------------------------------------ planted differences fail

#[test]
fn a_changed_value_fails() {
    only(&diff(r#"<r><a x="1"/></r>"#, r#"<r><a x="2"/></r>"#), Found::Value, "/r[0]/a[0]/@x");
    // Deeper, and the same either way round.
    let (a, b) = (r#"<r><a><b><x k="v"/></b></a></r>"#, r#"<r><a><b><x k="w"/></b></a></r>"#);
    only(&diff(a, b), Found::Value, "/r[0]/a[0]/b[0]/x[0]/@k");
    only(&diff(b, a), Found::Value, "/r[0]/a[0]/b[0]/x[0]/@k");
}

#[test]
fn a_missing_attribute_fails() {
    only(&diff(r#"<r><a x="1" y="2"/></r>"#, r#"<r><a x="1"/></r>"#), Found::Attribute, "/r[0]/a[0]/@y");
    only(&diff(r#"<r><a x="1"/></r>"#, r#"<r><a x="1" y="2"/></r>"#), Found::Attribute, "/r[0]/a[0]/@y");
    // An empty value is a value: no attribute is not an attribute that says nothing.
    only(&diff(r#"<r><a x=""/></r>"#, r#"<r><a/></r>"#), Found::Attribute, "/r[0]/a[0]/@x");
}

#[test]
fn an_extra_element_fails() {
    only(&diff("<r><a/><b/></r>", "<r><a/><b/><z/></r>"), Found::Element, "/r[0]/z[2]");
    // One element in the middle is one difference and not a shift of everything after it.
    only(&diff("<r><a/><x/><y/></r>", "<r><a/><b/><x/><y/></r>"), Found::Element, "/r[0]/b[1]");
    only(&diff("<r><a/><b/><x/></r>", "<r><a/><x/></r>"), Found::Element, "/r[0]/b[1]");
    // The root itself differs in name.
    only(&diff("<r/>", "<s/>"), Found::Element, "/r[0]");
}

#[test]
fn text_and_elements_in_another_order_are_one_order() {
    // The same pieces in another order: text is told apart by what it says, so that the swap is one difference, at the parent.
    only(&diff("<r>x<a/>y</r>", "<r>y<a/>x</r>"), Found::Order, "/r[0]");
    only(&diff("<r>x<a/><?p q?></r>", "<r><?p q?><a/>x</r>"), Found::Order, "/r[0]");
}

#[test]
fn two_ordered_siblings_swapped_fail() {
    // Two Shape elements, and two Row elements that hold things.
    only(&diff(r#"<r><s ID="1"/><s ID="2"/></r>"#, r#"<r><s ID="2"/><s ID="1"/></r>"#), Found::Order, "/r[0]");
    only(&diff("<r><row><a/></row><row><b/></row></r>", "<r><row><b/></row><row><a/></row></r>"), Found::Order, "/r[0]");
    // What stands before and after them is matched, and the swap is still one difference, at their parent.
    only(&diff(r#"<r><x/><s ID="1"/><s ID="2"/><y/></r>"#, r#"<r><x/><s ID="2"/><s ID="1"/><y/></r>"#), Found::Order, "/r[0]");
    // Not the same children in another order: the children are compared, place by place.
    let d = diff(r#"<r><s ID="1"/><s ID="2"/></r>"#, r#"<r><s ID="2"/><s ID="3"/></r>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert!(d.shown.iter().all(|x| x.found == Found::Value));
}

#[test]
fn a_changed_picture_byte_fails() {
    let png = [0x89, b'P', b'N', b'G', 1, 2, 3, 4];
    let mut changed = png;
    changed[6] = 9;
    let run = |a: &[u8], b: &[u8]| {
        compare_entries(
            &[entry("/media/image1.png", false, a)],
            &[entry("/media/image1.png", false, b)],
            true,
            &RULES,
            &XmlLimits::default(),
            &mut Work::default(),
        )
        .unwrap()
    };
    let d = run(&png, &changed);
    assert_eq!(d.count, 1);
    assert_eq!(d.shown, [Difference { part: "/media/image1.png".to_owned(), path: String::new(), found: Found::Bytes }]);
    assert!(run(&png, &png).same());
    // A part that is not XML is bytes however it looks: reformatted XML in a picture is a different picture.
    assert!(!run(b"<a/>", b"<a />").same());
}

#[test]
fn a_dropped_part_fails() {
    let (a, b) = ([entry("/a.xml", true, b"<a/>"), entry("/b.bin", false, b"1")], [entry("/a.xml", true, b"<a/>")]);
    let run = |x: &[Entry<'_>], y: &[Entry<'_>]| compare_entries(x, y, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(run(&a, &b).shown, [Difference { part: "/b.bin".to_owned(), path: String::new(), found: Found::MissingPart }]);
    assert_eq!(run(&b, &a).shown, [Difference { part: "/b.bin".to_owned(), path: String::new(), found: Found::ExtraPart }]);
    assert_eq!((run(&a, &b).count, run(&b, &a).count), (1, 1));
}

#[test]
fn a_changed_instruction_fails() {
    only(&diff("<r><?NewValue V?></r>", "<r><?NewValue W?></r>"), Found::Instruction, "/r[0]/#instruction[0]");
    only(&diff("<r><?NewValue V?></r>", "<r><?OldValue V?></r>"), Found::Instruction, "/r[0]/#instruction[0]");
    only(&diff("<r><a/></r>", "<r><a/><?NewValue V?></r>"), Found::Instruction, "/r[0]/#instruction[1]");
    // An instruction outside the root is outside the content.
    assert!(same("<?x y?><r/>", "<r/>"));
}

// ------------------------------------------------------------------------------------------------- planted reformatting passes

#[test]
fn spacing_passes() {
    let compact = r#"<r><a x="1"><b/><b/></a><c/></r>"#;
    let spaced = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<!-- a note -->\r\n<r>\r\n  <a x=\"1\">\r\n    <b />\r\n\t<b></b>\r\n  </a>\r\n  <c/>\r\n</r>\r\n<!-- the end -->\r\n";
    assert!(same(compact, spaced));
    // Spacing inside a tag, and a self-closed element against an empty pair.
    assert!(same(r#"<r  a = "1"  b='2' ></r >"#, r#"<r a="1" b="2"/>"#));
}

#[test]
fn attribute_order_passes() {
    assert!(same(r#"<r><a x="1" y="2" z="3"/></r>"#, r#"<r><a z="3" x="1" y="2"/></r>"#));
    // But the value of each is its own.
    assert!(!same(r#"<r><a x="1" y="2"/></r>"#, r#"<r><a x="2" y="1"/></r>"#));
}

#[test]
fn another_prefix_for_one_namespace_passes() {
    let a = r#"<p:r xmlns:p="urn:a" xmlns:q="urn:b"><p:x q:k="1"/></p:r>"#;
    assert!(same(a, r#"<z:r xmlns:z="urn:a" xmlns:y="urn:b"><z:x y:k="1"/></z:r>"#));
    // The default namespace is a prefix too, for elements, and an unprefixed attribute is in none.
    assert!(same(a, r#"<r xmlns="urn:a" xmlns:q="urn:b"><x q:k="1"/></r>"#));
    // A declaration that nothing uses is no content.
    assert!(same(a, r#"<p:r xmlns:p="urn:a" xmlns:q="urn:b" xmlns:unused="urn:c"><p:x q:k="1"/></p:r>"#));
}

#[test]
fn a_namespace_that_differs_is_a_difference() {
    // The same prefix bound to another URI: the names are other names.
    only(&diff(r#"<p:r xmlns:p="urn:a"/>"#, r#"<p:r xmlns:p="urn:b"/>"#), Found::Element, "/r[0]");
    // An unprefixed attribute is in no namespace, and a prefixed one is in its own.
    let d = diff(r#"<r xmlns:p="urn:a" n="1"/>"#, r#"<r xmlns:p="urn:a" p:n="1"/>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert!(d.shown.iter().all(|x| x.found == Found::Attribute));
    // An unprefixed attribute is in no namespace, even where its element's default namespace is the one a prefix names.
    let d = diff(r#"<r xmlns="urn:a" xmlns:p="urn:a" n="1"/>"#, r#"<r xmlns="urn:a" xmlns:p="urn:a" p:n="1"/>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    // The default namespace of an element and of its child.
    assert!(!same(r#"<r xmlns="urn:a"><x xmlns="urn:b"/></r>"#, r#"<r xmlns="urn:a"><x/></r>"#));
    assert!(!same(r#"<r xmlns="urn:a"/>"#, "<r/>"));
    // A declaration ends with its element: `p` is `urn:a` again after the child that redeclared it.
    let nested = r#"<r xmlns:p="urn:a"><x xmlns:p="urn:b"/><p:y/></r>"#;
    assert!(same(nested, r#"<r xmlns:q="urn:a"><x xmlns:q="urn:b"/><q:y/></r>"#));
    assert!(!same(nested, r#"<r xmlns:p="urn:a"><x xmlns:p="urn:b"/><y xmlns="urn:b"/></r>"#));
    // A prefix bound to nothing: `xmlns:p=""` is not allowed in XML 1.0's namespaces, and the part is its bytes (`canon_ns.rs`).
    assert!(!same(r#"<p:x xmlns:p=""/>"#, "<x/>"));
    // A prefix nothing binds is the name: it is not another prefix's name, and not any namespace's.
    assert!(!same("<a:r/>", "<b:r/>"));
    assert!(same(r#"<a:r x="1" y="2"/>"#, r#"<a:r y="2" x="1"/>"#));
    assert!(!same(r#"<a:r/>"#, r#"<r xmlns:a="urn:a"/>"#));
    // `xml:` needs no declaration, and is the same name under its own namespace.
    assert!(same(r#"<r xml:lang="en"/>"#, r#"<r xmlns:xml="http://www.w3.org/XML/1998/namespace" xml:lang="en"/>"#));
    assert!(!same(r#"<r xml:lang="en"/>"#, r#"<r lang="en"/>"#));
}

#[test]
fn two_prefixes_for_one_attribute_are_compared_in_the_order_written() {
    // They were compared as a list sorted by value, and the two values swapped passed as the same, while a reader takes the first (`canon_ns.rs` holds the rest).
    let a = r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="1" q:k="2"/>"#;
    assert!(!same(a, r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="2" q:k="1"/>"#));
    assert!(same(a, r#"<r xmlns:p="urn:a" xmlns:q="urn:a" q:k="1" p:k="2"/>"#));
    // A value changed in one of them is found, and so is one that was dropped.
    only(&diff(a, r#"<r xmlns:p="urn:a" xmlns:q="urn:a" p:k="1" q:k="3"/>"#), Found::Value, "/r[0]/@k");
    only(&diff(a, r#"<r xmlns:p="urn:a" p:k="1"/>"#), Found::Attribute, "/r[0]/@k");
}

#[test]
fn number_spelling_passes_on_a_number_typed_attribute_only() {
    assert!(same_number("1.50", "1.5") && same_number("15E-1", "1.5"));
    assert!(!same_number("2cm", "20mm"));
    assert!(same_number("1.5cm", "1.50cm"));
    // In an attribute the lists name:
    assert!(same(r#"<r><a len="1.50"/></r>"#, r#"<r><a len="1.5"/></r>"#));
    assert!(same(r#"<r><a len="15E-1cm"/></r>"#, r#"<r><a len="1.50cm"/></r>"#));
    assert!(same(r#"<r><a len="+2.0%"/></r>"#, r#"<r><a len="2%"/></r>"#));
    // Each relaxation has its planted difference: another number, another unit, a unit against none, text, another case.
    only(&diff(r#"<r><a len="1.5cm"/></r>"#, r#"<r><a len="1.6cm"/></r>"#), Found::Value, "/r[0]/a[0]/@len");
    assert!(!same(r#"<r><a len="2cm"/></r>"#, r#"<r><a len="20mm"/></r>"#));
    assert!(!same(r#"<r><a len="1.5cm"/></r>"#, r#"<r><a len="1.5mm"/></r>"#));
    assert!(!same(r#"<r><a len="1.5cm"/></r>"#, r#"<r><a len="1.5CM"/></r>"#));
    assert!(!same(r#"<r><a len="1.5"/></r>"#, r#"<r><a len="1.5cm"/></r>"#));
    assert!(!same(r#"<r><a len="abc"/></r>"#, r#"<r><a len="abd"/></r>"#));
    assert!(!same(r#"<r><a len="1.5"/></r>"#, r#"<r><a len="one"/></r>"#));
    // In any other attribute the spelling is the value.
    assert!(!same(r#"<r><a x="1.50"/></r>"#, r#"<r><a x="1.5"/></r>"#));
    assert!(!same(r#"<r><a x="15E-1"/></r>"#, r#"<r><a x="1.5"/></r>"#));
    // Nor in text.
    assert!(!same("<r><a>1.50</a></r>", "<r><a>1.5</a></r>"));
}

#[test]
fn the_edges_of_a_number() {
    // `.5` and `5.` are not in the grammar: they stay text, and text is compared as it is.
    assert!(!same_number(".5", "0.5") && !same_number("5.", "5") && !same_number("5.", "5."));
    assert!(!same(r#"<r><a len=".5"/></r>"#, r#"<r><a len="0.5"/></r>"#));
    assert!(same(r#"<r><a len=".5" y="1"/></r>"#, r#"<r><a y="1" len=".5"/></r>"#));
    // `-0` is not `0`, and a sign is content: `+1` is `1`.
    assert!(!same_number("-0", "0") && !same_number("-0.0cm", "0cm") && same_number("-0", "-0.00"));
    assert!(same_number("+1", "1") && !same_number("-1", "1"));
    // Infinity and an underflow to zero are not numbers: two of them are not the same number, and stay text.
    assert!(!same_number("1e400", "2e400") && !same_number("1e-400", "0") && !same_number("1e400", "1e400"));
    // The digit that is not zero may be in the fraction, and the underflow is the same: `0.5e-400` is not `0`, and `0.0e-400` is.
    assert!(!same_number("0.5e-400", "0") && !same_number("0.5e-400", "0.5e-401") && same_number("0.0e-400", "0"));
    assert!(!same(r#"<r><a len="1e400"/></r>"#, r#"<r><a len="2e400"/></r>"#));
    assert!(!same(r#"<r><a len="1e-400"/></r>"#, r#"<r><a len="0"/></r>"#));
    assert!(same(r#"<r><a len="1e400" x="1"/></r>"#, r#"<r><a x="1" len="1e400"/></r>"#));
    // An exponent needs digits: `1em` is `1` and the unit `em`; `1e5em` is 100000 and `em`.
    assert!(same_number("1em", "1.0em") && !same_number("1em", "1e1") && same_number("1e5em", "100000em"));
    // A unit is letters or `%`, nothing else; the value reads whole, so white space and junk make it text.
    assert!(!same_number(" 1", "1") && !same_number("1 ", "1") && !same_number("1_0", "1_0") && !same_number("1cm2", "1cm2"));
    assert!(!same_number("", "") && !same_number("-", "-") && !same_number("cm", "cm"));
    // The number is the same `f64`: a different decimal that reads as the same value is the same number.
    assert!(same_number("0.30000000000000004", "0.30000000000000004") && !same_number("0.30000000000000004", "0.3"));
    // A very long number reads in a bounded time and means what it says.
    let long = format!("1{}", "0".repeat(100_000));
    assert!(!same_number(&long, "1") && !same_number(&long, &long));
}

#[test]
fn keyed_siblings_reordered_pass() {
    let a = r#"<r><c n="1" v="a"/><c n="2" v="b"/><c n="3" v="c"/></r>"#;
    assert!(same(a, r#"<r><c n="3" v="c"/><c n="1" v="a"/><c n="2" v="b"/></r>"#));
    // A value changed after a reorder is one difference, at the key's place in the sorted order.
    only(&diff(a, r#"<r><c n="3" v="c"/><c n="1" v="a"/><c n="2" v="B"/></r>"#), Found::Value, "/r[0]/c#1/@v");
    // A keyed child taken out, or added, is one difference.
    only(&diff(a, r#"<r><c n="3" v="c"/><c n="1" v="a"/></r>"#), Found::Element, "/r[0]/c#1");
    only(&diff(r#"<r><c n="3" v="c"/><c n="1" v="a"/></r>"#, a), Found::Element, "/r[0]/c#1");
    // A key changed is one taken out and one added.
    let d = diff(r#"<r><c n="1"/></r>"#, r#"<r><c n="2"/></r>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    // Two siblings of one key are compared in the order they were written: swapping what they hold is a difference.
    let twins = r#"<r><c n="1" v="a"/><c n="1" v="b"/></r>"#;
    assert!(same(twins, r#"<r><c v="a" n="1"/><c v="b" n="1"/></r>"#));
    assert_eq!(diff(twins, r#"<r><c n="1" v="b"/><c n="1" v="a"/></r>"#).count, 2);
    // No key is not an empty key.
    assert!(!same(r#"<r><c/></r>"#, r#"<r><c n=""/></r>"#));
    // What the children hold is compared in full, however they are placed.
    assert!(!same(r#"<r><c n="1"><x/></c></r>"#, r#"<r><c n="1"><y/></c></r>"#));
}

#[test]
fn keyed_siblings_among_others_keep_the_order_of_the_others() {
    assert!(same(r#"<r><x/><c n="2"/><y/><c n="1"/></r>"#, r#"<r><c n="1"/><x/><y/><c n="2"/></r>"#));
    // The unkeyed ones are still in order among themselves.
    only(&diff(r#"<r><y/><c n="2"/><x/></r>"#, r#"<r><x/><c n="2"/><y/></r>"#), Found::Order, "/r[0]");
    // Two sets with their own keys do not mix, and the order between the sets is nothing.
    assert!(same(r#"<r><c n="1"/><d n="1"/></r>"#, r#"<r><d n="1"/><c n="1"/></r>"#));
    assert!(!same(r#"<r><c n="1"/></r>"#, r#"<r><d n="1"/></r>"#));
}

#[test]
fn comments_are_ignored() {
    assert!(same("<r>a<!--x-->b</r>", "<r>ab</r>"));
    assert!(same("<r><!--x--><a/><!--y--></r>", "<r><a/></r>"));
    assert!(same("<r><a><!----></a></r>", "<r><a/></r>"));
    assert!(same("<!--before--><r/><!--after-->", "<r/>"));
    // A comment is not what stands in its place, and it does not hide what is beside it.
    assert!(!same("<r>a<!--x-->b</r>", "<r>a<!--x-->c</r>"));
    assert!(!same("<r>a</r>", "<r>a<?x y?></r>"));
    assert!(!same("<r><a/></r>", "<r><!--a--></r>"));
    // An instruction splits the text and a comment does not.
    assert!(!same("<r>a<?p q?>b</r>", "<r>ab</r>"));
}

#[test]
fn text_is_compared_after_cdata_references_and_joining() {
    assert!(same("<r>a&amp;b</r>", "<r><![CDATA[a&b]]></r>"));
    assert!(same("<r>&#65;&#x42;</r>", "<r>AB</r>"));
    assert!(same("<r>a<![CDATA[b]]>c</r>", "<r>abc</r>"));
    assert!(same("<r>&lt;&gt;&quot;&apos;</r>", "<r><![CDATA[<>\"']]></r>"));
    assert!(same("<r><![CDATA[]]>a</r>", "<r>a</r>"));
    assert!(same("<r>a\r\nb</r>", "<r>a\nb</r>"));
    only(&diff("<r>a&amp;b</r>", "<r><![CDATA[a&c]]></r>"), Found::Text, "/r[0]/#text[0]");
    // White space inside text is text.
    assert!(!same("<r>a b</r>", "<r>a  b</r>"));
    assert!(!same("<r>a b</r>", "<r>a\tb</r>"));
}

#[test]
fn blank_text_is_ignored_only_between_children_of_an_element_with_no_text() {
    // In an element that holds no text, between children, at its start and at its end.
    assert!(same("<r>\n <a/>\n <b/>\n</r>", "<r><a/><b/></r>"));
    assert!(same("<r> <?p q?> </r>", "<r><?p q?></r>"));
    // In an element that holds text the text is compared exactly, blank pieces too.
    only(&diff("<r>x<a/> </r>", "<r>x<a/></r>"), Found::Text, "/r[0]/#text[2]");
    assert!(!same("<r>x<a/> <b/></r>", "<r>x<a/><b/></r>"));
    assert!(!same("<r> a<b/></r>", "<r>a<b/></r>"));
    // Blank text with no child beside it is the element's text.
    only(&diff("<t> </t>", "<t/>"), Found::Text, "/t[0]/#text[0]");
    assert!(!same("<r><a> </a></r>", "<r><a/></r>"));
    assert!(!same("<r><a>\n</a></r>", "<r><a> </a></r>"));
    // An element the lists name as mixed content keeps its blank text beside its children.
    assert!(!same("<r><u> </u></r>", "<r><u/></r>"));
    assert!(same("<r><u><cp/> </u></r>", "<r><u><cp/></u></r>"));
    only(&diff("<r><t><cp/> </t></r>", "<r><t><cp/></t></r>"), Found::Text, "/r[0]/t[0]/#text[1]");
    assert!(!same("<r><t><cp/>\n<pp/></t></r>", "<r><t><cp/><pp/></t></r>"));
    // `xml:space` is not read: the parts of one format carry it on every root, and spacing is not content there.
    assert!(same("<r xml:space=\"preserve\">\n <a/>\n</r>", "<r xml:space=\"preserve\"><a/></r>"));
    assert!(!same("<r xml:space=\"preserve\"><a/></r>", "<r><a/></r>"));
}

// ------------------------------------------------------------------------------------------------------------ echo control

#[test]
fn a_file_against_itself_is_the_same() {
    let doc = r#"<?xml version="1.0"?><r xmlns="urn:a" xmlns:p="urn:b"><c n="2" len="1.5cm" p:k="v"/><c n="1"/><t>x<cp/>y</t><?i j?><x><![CDATA[z]]></x></r>"#;
    let parts = [entry("/a.xml", true, doc.as_bytes()), entry("/b.bin", false, &[1, 2, 3]), entry("/c.xml", true, b"<broken")];
    let mut work = Work::default();
    let d = compare_entries(&parts, &parts, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert!(d.same() && d.shown.is_empty());
    // The parts in another order are the same, and so is a part with something after its root: different bytes, read in full.
    let (late, before) = (format!("{doc}\n<!-- end -->\n"), doc.replacen("?>", "?>\n<!-- start -->\n", 1));
    let moved = [entry("/c.xml", true, b"<broken"), entry("/a.xml", true, late.as_bytes()), entry("/b.bin", false, &[1, 2, 3])];
    let mut work = Work::default();
    assert!(compare_entries(&parts, &moved, true, &RULES, &XmlLimits::default(), &mut work).unwrap().same());
    assert!(work.xml_events > 0, "the part with different bytes was read");
    let again = [entry("/a.xml", true, before.as_bytes())];
    assert!(compare_entries(&[parts[0]], &again, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap().same());
}

#[test]
fn a_file_against_a_salted_copy_fails_at_the_salt_only() {
    // A package of many parts and cells, and a copy with one value changed anywhere: one difference, at that cell.
    let page = |salt: Option<usize>| {
        let mut s = String::from("<Page>");
        for shape in 0..20 {
            s.push_str(&format!("<Shape ID=\"{shape}\">"));
            for cell in 0..20 {
                let v = if salt == Some(shape * 20 + cell) { "SALT" } else { "1.5" };
                s.push_str(&format!("<c n=\"Cell{cell:02}\" v=\"{v}\"/>"));
            }
            s.push_str("</Shape>");
        }
        s.push_str("</Page>");
        s
    };
    let base = page(None);
    for at in [0usize, 1, 19, 20, 123, 399] {
        let salted = page(Some(at));
        let d = diff(&base, &salted);
        let (shape, cell) = (at / 20, at % 20);
        only(&d, Found::Value, &format!("/Page[0]/Shape[{shape}]/c#{cell}/@v"));
        assert_eq!(diff(&salted, &base).count, 1);
    }
    // A salt in a package: one part changes, and no other part is blamed.
    let other = b"<other/>".to_vec();
    let (pa, pb) = (page(None), page(Some(7)));
    let a = [entry("/p1.xml", true, pa.as_bytes()), entry("/p2.xml", true, &other)];
    let b = [entry("/p1.xml", true, pb.as_bytes()), entry("/p2.xml", true, &other)];
    let d = compare_entries(&a, &b, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(d.count, 1);
    assert_eq!(d.shown.first().map(|x| x.part.as_str()), Some("/p1.xml"));
}

// -------------------------------------------------------------------------------------------------------------------------- parts

#[test]
fn part_names_fold_ascii_case_only_when_asked() {
    let run = |a: &str, b: &str, fold: bool| {
        compare_entries(&[entry(a, true, b"<r/>")], &[entry(b, true, b"<r/>")], fold, &RULES, &XmlLimits::default(), &mut Work::default())
            .unwrap()
            .count
    };
    assert_eq!(run("/X/Page1.xml", "/x/page1.xml", true), 0);
    assert_eq!(run("/X/Page1.xml", "/x/page1.xml", false), 2);
    // Only ASCII letters fold.
    assert_eq!(run("/x/\u{C9}.xml", "/x/\u{E9}.xml", true), 2);
    assert_eq!(run("/x/\u{C9}.xml", "/x/\u{C9}.xml", false), 0);
    // Two parts one side holds under one folded name are paired in the order given, and the third is alone.
    let (a, b) =
        ([entry("/A", false, b"1"), entry("/a", false, b"2"), entry("/B", false, b"3")], [entry("/a", false, b"1"), entry("/A", false, b"2")]);
    let d = compare_entries(&a, &b, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    assert_eq!(d.count, 1);
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart));
}

#[test]
fn a_part_meant_as_xml_that_does_not_parse_is_compared_as_bytes() {
    let run = |a: &[u8], b: &[u8]| {
        compare_entries(&[entry("/p.xml", true, a)], &[entry("/p.xml", true, b)], true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap()
    };
    // Spacing in a part that does not parse is a difference: nothing can be said of it but its bytes.
    let d = run(b"<r><a></r>", b"<r><a ></r>");
    assert_eq!((d.count, d.shown.first().map(|x| x.found)), (1, Some(Found::Bytes)));
    assert!(run(b"<r><a></r>", b"<r><a></r>").same());
    // One side that parses and one that does not.
    assert_eq!(run(b"<r/>", b"<r>").count, 1);
    assert_eq!(run(b"<r/>", b"<r>&x;</r>").count, 1);
    // A part one side calls XML and the other does not is bytes.
    let d =
        compare_entries(&[entry("/p", true, b"<r/>")], &[entry("/p", false, b"<r />")], true, &RULES, &XmlLimits::default(), &mut Work::default())
            .unwrap();
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Bytes));
}

#[test]
fn a_part_past_the_xml_limits_is_an_error_and_not_a_verdict() {
    let tight = XmlLimits { max_depth: 3, ..XmlLimits::default() };
    let (a, b) = (b"<a><b><c><d/></c></b></a>".as_slice(), b"<a><b><c><d /></c></b></a>".as_slice());
    let run =
        |x: &[u8], y: &[u8]| compare_entries(&[entry("/p.xml", true, x)], &[entry("/p.xml", true, y)], true, &RULES, &tight, &mut Work::default());
    assert_eq!(run(a, b).unwrap_err().class(), "xml_too_complex");
    assert_eq!(run(b, a).unwrap_err().class(), "xml_too_complex");
    // One side that is within the limit, and one that is not.
    assert!(run(b"<a/>", a).is_err());
    // The same bytes are the same under any limit: there is nothing to read.
    assert!(run(a, a).unwrap().same());
    // An attribute past its limit.
    let narrow = XmlLimits { max_attr_bytes: 8, ..XmlLimits::default() };
    let big = |v: &str| format!("<a x=\"{v}\"/>");
    let (long_a, long_b) = (big("123456789"), big("12345678x"));
    let err = compare_entries(
        &[entry("/p.xml", true, long_a.as_bytes())],
        &[entry("/p.xml", true, long_b.as_bytes())],
        true,
        &RULES,
        &narrow,
        &mut Work::default(),
    );
    assert_eq!(err.unwrap_err().class(), "xml_too_complex");
    let few = XmlLimits { max_nodes: 5, ..XmlLimits::default() };
    let many = b"<a><b/><b/><b/><b/><b/><b/></a>".as_slice();
    let err =
        compare_entries(&[entry("/p.xml", true, many)], &[entry("/p.xml", true, b"<a/>")], true, &RULES, &few, &mut Work::default()).unwrap_err();
    assert_eq!(err.class(), "xml_too_complex");
}

#[test]
fn a_path_shows_no_name_a_person_chose() {
    // Key values, attribute values, text and the instruction's words are not in a path.
    let a = r#"<Pages><c n="Secret" v="1"><d n="Hidden"/></c><Page Name="Secret">Secret text<?Secret x?></Page></Pages>"#;
    let b = r#"<Pages><c n="Secret" v="2"><d n="Hidden"/></c><Page Name="Public">Public text<?Secret y?></Page></Pages>"#;
    let d = diff(a, b);
    assert!(d.count >= 4, "{:?}", d.shown);
    for found in &d.shown {
        for word in ["Secret", "Hidden", "Public"] {
            assert!(!found.path.contains(word) && !found.part.contains(word), "{found:?}");
        }
    }
    // A part named with control characters is shown with none, and a long name is cut.
    let long = format!("/{}", "x".repeat(500));
    let d = compare_entries(
        &[entry("/a\u{1b}[31m.bin", false, b"1"), entry(&long, false, b"1")],
        &[],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap();
    assert!(d.shown.iter().all(|x| !x.part.chars().any(char::is_control) && x.part.chars().count() <= 128), "{:?}", d.shown);
}

#[test]
fn the_list_is_capped_at_200_and_the_count_is_not() {
    let (mut a, mut b) = (String::from("<r>"), String::from("<r>"));
    for i in 0..1000 {
        a.push_str(&format!("<c n=\"{i:04}\" v=\"1\"/>"));
        b.push_str(&format!("<c n=\"{i:04}\" v=\"2\"/>"));
    }
    a.push_str("</r>");
    b.push_str("</r>");
    let d = diff(&a, &b);
    assert_eq!((d.count, d.shown.len(), MAX_SHOWN), (1000, 200, 200));
    // The ones shown are the first, in the order the check walked.
    assert_eq!(d.shown.first().map(|x| x.path.as_str()), Some("/r[0]/c#0/@v"));
    assert_eq!(d.shown.last().map(|x| x.path.as_str()), Some("/r[0]/c#199/@v"));
}

#[test]
fn a_deep_tree_is_compared_on_a_small_stack() {
    // 510 levels (the parser's ceiling is 512) on a thread of 2 MiB, the least a test thread has: the check recurses once for each level.
    let deep = |leaf: &str| format!("{}{leaf}{}", "<k>".repeat(510), "</k>".repeat(510));
    let (a, b) = (deep("<x v=\"1\"/>"), deep("<x v=\"2\"/>"));
    let seen = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let d = diff_limited(&a, &b, &RULES, &deep_limits(), &mut Work::default());
            (d.count, d.shown.first().map(|x| (x.found, x.path.len() == 1024, x.path.ends_with("/..."))))
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(seen, (1, Some((Found::Value, true, true))));
    // The same tree against itself reformatted.
    let (a, b) = (deep("<x v=\"1\"/>"), deep("<x v=\"1\" ></x>"));
    let echo = move || assert!(diff_limited(&a, &b, &RULES, &deep_limits(), &mut Work::default()).same());
    std::thread::Builder::new().stack_size(2 * 1024 * 1024).spawn(echo).unwrap().join().unwrap();
}

// ---------------------------------------------------------------------------------------------------------------- ODF paragraphs

fn para(inner: &str) -> String {
    format!("<office:text xmlns:office=\"urn:o\" xmlns:text=\"{T}\"><text:p>{inner}</text:p></office:text>")
}

#[test]
fn text_s_against_spaces_passes_in_a_paragraph() {
    assert!(same(&para("a<text:s/>b"), &para("a b")));
    // A space that runs make is one; the spaces `text:s` adds are on top of it: V16 of the ODF styles spec.
    assert!(same(&para("a <text:s/>b"), &para("a<text:s text:c=\"2\"/>b")));
    assert!(same(&para("a\n    <text:s text:c=\"2\"/>b"), &para("a<text:s text:c=\"3\"/>b")));
    assert!(same(&para("a<text:s text:c=\"+2\"/>b"), &para("a<text:s text:c=\"2\"/>b")));
    // Two spaces in a row are one space, and three explicit are three.
    assert!(!same(&para("a  b"), &para("a<text:s text:c=\"2\"/>b")));
    assert!(!same(&para("a<text:s text:c=\"3\"/>b"), &para("a   b")));
    assert!(!same(&para("a<text:s text:c=\"3\"/>b"), &para("a<text:s text:c=\"2\"/>b")));
    // `text:c` that is not a count makes the element what it is: an element.
    for bad in ["2.0", "-1", "", "0x2", "99999999999999999999", " 2", "++2", "++1", "+-1", "1_0"] {
        assert!(!same(&para(&format!("a<text:s text:c=\"{bad}\"/>b")), &para("a b")), "{bad}");
    }
    // A `text:s` that holds anything, or carries another attribute, is an element.
    assert!(!same(&para("a<text:s>x</text:s>b"), &para("axb")));
    assert!(!same(&para("a<text:s>x</text:s>b"), &para("a b")));
    assert!(!same(&para("a<text:s>x</text:s>b"), &para("a<text:s>y</text:s>b")));
    assert!(!same(&para("a<text:s><x/></text:s>b"), &para("a b")));
    assert!(!same(&para("a<text:s foo=\"1\"/>b"), &para("a b")));
    // Holding a keyed child is holding something: the keyed children are not in the element's items, and are not nothing either.
    assert!(!same(&para("a<text:s><c n=\"1\"/></text:s>b"), &para("a b")));
    // A count of none is no spaces, and no element: it does not split the text around it.
    assert!(same(&para("a<text:s text:c=\"0\"/>b"), &para("ab")));
    assert!(same(&para("a<text:s text:c=\"0\"/>b"), &para("a<text:s text:c=\"+0\"/>b")));
}

#[test]
fn spaces_collapse_only_in_paragraph_content() {
    assert!(same(&para("a   b"), &para("a b")));
    assert!(same(&para("a\n\t b"), &para("a b")));
    // Leading and trailing white space of a paragraph go; explicit spaces are not white space.
    assert!(same(&para("  a  "), &para("a")));
    assert!(same(&para("\n  a b\n"), &para("a b")));
    assert!(!same(&para("<text:s/>a"), &para("a")));
    assert!(!same(&para("a<text:s/>"), &para("a")));
    assert!(same(&para("a<text:s/> "), &para("a<text:s/>")));
    // A run's space before `text:s` and after it are each a space of their own, and the explicit ones are not taken off the end.
    assert!(same(&para("a <text:s/> b"), &para("a<text:s text:c=\"3\"/>b")));
    assert!(same(&para("a <text:s/>"), &para("a<text:s text:c=\"2\"/>")));
    assert!(same(&para("a<text:s/> <text:s/>"), &para("a<text:s text:c=\"3\"/>")));
    // The end of a span is not the end of its paragraph: its last space is a space.
    assert!(!same(&para("a<text:span>b </text:span>c"), &para("a<text:span>b</text:span>c")));
    assert!(same(&para("a<text:span>b </text:span>c"), &para("a<text:span>b<text:s/></text:span>c")));
    // The same text outside a paragraph is text.
    let outside = |s: &str| format!("<office:text xmlns:office=\"urn:o\"><x>{s}</x></office:text>");
    assert!(!same(&outside("a   b"), &outside("a b")));
    assert!(!same(&outside("  a"), &outside("a")));
    // A span is paragraph content, and its edges are not the paragraph's: the space a span starts with is a space.
    assert!(!same(&para("a<text:span> b</text:span>"), &para("a<text:span>b</text:span>")));
    assert!(same(&para("a<text:span>b   c</text:span>"), &para("a<text:span>b c</text:span>")));
    assert!(!same(&para("<text:span>a</text:span><text:span>b</text:span>"), &para("<text:span>a</text:span> <text:span>b</text:span>")));
    // An element in a paragraph splits the runs and the check does not look across it (stricter than ODF, never looser).
    assert!(!same(&para("a <text:line-break/> b"), &para("a <text:line-break/>b")));
    assert!(same(&para("a <text:line-break/> b"), &para("a  <text:line-break/>  b")));
    assert!(!same(&para("a<text:tab/>b"), &para("a b")));
    // Text that differs still does.
    assert!(!same(&para("a b"), &para("a c")));
    // Several paragraphs are in order; blank text between them is nothing, since a page is not a paragraph.
    let two = |gap: &str| format!("<office:text xmlns:office=\"urn:o\" xmlns:text=\"{T}\"><text:p>a</text:p>{gap}<text:p>b</text:p></office:text>");
    assert!(same(&two("\n  "), &two("")));
    assert!(!same(&two(""), &two("x")));
}

#[test]
fn a_count_of_spaces_is_counted_and_never_written_out() {
    // 2^63 twice against 2^64 - 1 and 1: the same count, and a text of that many spaces would never fit.
    let big = |counts: &[&str]| para(&counts.iter().map(|c| format!("<text:s text:c=\"{c}\"/>")).collect::<String>());
    assert!(same(&big(&["9223372036854775808", "9223372036854775808"]), &big(&["18446744073709551615", "1"])));
    assert!(!same(&big(&["9223372036854775808", "9223372036854775808"]), &big(&["18446744073709551615", "2"])));
    assert!(same(&big(&["0", "5"]), &big(&["2", "3"])));
    // A paragraph of a hundred thousand of them.
    let many = |c: &str, n: usize| para(&format!("<text:s text:c=\"{c}\"/>").repeat(n));
    assert!(same(&many("5", 100_000), &many("50", 10_000)));
    assert!(!same(&many("5", 100_000), &many("5", 100_001)));
}

// ------------------------------------------------------------------------------------------------------- the cost, counted

/// The work of comparing `a` with `b` as parts.
fn work_of(a: &str, b: &str) -> (ContentDiff, Work) {
    let mut work = Work::default();
    let d = diff_limited(a, b, &RULES, &deep_limits(), &mut work);
    (d, work)
}

fn log2_ceil(n: u64) -> u64 {
    u64::from(64 - (n - 1).leading_zeros())
}

/// `n` keyed cells in the order a multiplier puts them (a permutation when it is coprime to `n`), the first key replaced when `moved`.
fn keyed_cells(n: u64, multiplier: u64, moved: bool) -> String {
    let mut s = String::from("<r>");
    for i in 0..n {
        let at = if moved && i == 0 { n } else { (i * multiplier) % n };
        s.push_str(&format!("<c n=\"K{at:08}\"/>"));
    }
    s.push_str("</r>");
    s
}

#[test]
fn keyed_siblings_cost_n_log_n_at_the_most_and_key_bytes_are_counted() {
    for n in [1_009u64, 10_007, 100_003] {
        // The same set in two orders, and then a copy of one with one key that the other does not have.
        let (a, b) = (n * 618 / 1000, n * 414 / 1000);
        let (d, work) = work_of(&keyed_cells(n, a, false), &keyed_cells(n, b, false));
        assert!(d.same(), "n = {n}");
        // Measured: 3.1 steps for each n log2 n at n = 1,009, falling to 2.74 at 100,003 (the build, the two sorts and the merge).
        let bound = 34 * n * log2_ceil(n) / 10;
        assert!(work.canon_steps <= bound, "n = {n}: {} steps, bound {bound}", work.canon_steps);
        assert!(work.canon_steps * 10 >= 25 * n * log2_ceil(n) - 25 * n, "n = {n}: {} steps is less than the sorts cost", work.canon_steps);
        // One key is not in the other side: one taken out and one added, wherever the sort put them.
        let (d, work) = work_of(&keyed_cells(n, a, false), &keyed_cells(n, b, true));
        assert_eq!(d.count, 2, "n = {n}: {:?}", d.shown.first());
        assert!(work.canon_steps <= bound, "n = {n}: {} steps, bound {bound}", work.canon_steps);
        // Every key is 9 bytes: the sorts and the merge look at that much for each comparison, and the build at the key and the cell.
        // Measured: 21 to 23 bytes for each n log2 n.
        assert!(work.canon_bytes <= 26 * n * log2_ceil(n), "n = {n}: {} bytes", work.canon_bytes);
    }
}

#[test]
fn siblings_that_share_one_key_cost_a_pass_and_a_sort() {
    for n in [1_000u64, 10_000, 100_000] {
        let cells = |salt: bool| {
            let mut s = String::from("<r>");
            for i in 0..n {
                let v = if salt && i == n / 2 { "x" } else { "y" };
                s.push_str(&format!("<c n=\"same\" v=\"{v}\"/>"));
            }
            s.push_str("</r>");
            s
        };
        let (d, work) = work_of(&cells(false), &cells(true));
        assert_eq!(d.count, 1);
        only_at(&d, &format!("/r[0]/c#{}/@v", n / 2));
        assert!(work.canon_steps <= 16 * n + 16, "n = {n}: {} steps", work.canon_steps);
    }
}

fn only_at(d: &ContentDiff, path: &str) {
    assert_eq!(d.shown.first().map(|x| x.path.as_str()), Some(path));
}

#[test]
fn long_equal_keys_are_charged_by_their_bytes() {
    // 2,000 siblings whose keys share 3,000 bytes: the sorts compare the shared bytes each time, and the bytes say so.
    let n = 2_000u64;
    let prefix = "k".repeat(3_000);
    let cells = |first: &str| {
        let mut s = String::from("<r>");
        for i in 0..n {
            s.push_str(&format!("<c n=\"{prefix}{i:05}\" v=\"{first}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    let (d, work) = work_of(&cells("a"), &cells("b"));
    assert_eq!(d.count, n);
    // At least the bytes of one pass over the keys, and no more than the comparisons made times their length.
    assert!(work.canon_bytes >= 5 * n * 3_000, "{}", work.canon_bytes);
    // Measured: 6 times the bytes of the keys (two builds, the sorts, the merge), and no more.
    assert!(work.canon_bytes <= 7 * n * 3_005, "{}", work.canon_bytes);
}

#[test]
fn a_flat_part_costs_a_pass_whatever_its_size() {
    // Elements, attributes and the comparison are linear: the steps for four times the elements are four times, near enough.
    let flat = |n: usize, salt: bool| {
        let mut s = String::from("<r>");
        for i in 0..n {
            s.push_str(&format!("<e a=\"{i}\" b=\"{}\"/>", if salt && i == n / 3 { "salt" } else { "b" }));
        }
        s.push_str("</r>");
        s
    };
    let steps = |n: usize| {
        let (d, w) = work_of(&flat(n, false), &flat(n, true));
        assert_eq!(d.count, 1);
        w.canon_steps
    };
    let (small, large) = (steps(25_000), steps(100_000));
    assert!(large < small * 5 && large > small * 3, "{small} then {large}");
    // 14 steps for each element, measured: the element, its two attributes and their sort on each side, and the comparison of the pair.
    assert!(large <= 14 * 100_000 + 100, "{large}");
}

#[test]
fn a_part_of_many_items_is_compared_in_a_pass_and_costs_what_the_count_says() {
    // 200,000 elements of two attributes each: 600,000 items. The count is linear (14 steps for each element), so the part at the item limit
    // (6 million items) is 28 million steps; `canon_scale.rs` builds that part, and measures what it costs.
    let n = 200_000usize;
    let (d, work) = work_of(&items(n, false), &items(n, true));
    assert_eq!(d.count, 1);
    assert!(work.canon_steps <= 15 * n as u64, "{}", work.canon_steps);
    assert!(work.canon_steps >= 13 * n as u64, "{}", work.canon_steps);
}

/// `n` elements of two attributes each, the last one's second attribute changed when `salt`.
pub fn items(n: usize, salt: bool) -> String {
    let mut s = String::with_capacity(n * 24 + 16);
    s.push_str("<r>");
    for i in 0..n {
        s.push_str(if salt && i + 1 == n { "<e a=\"1\" b=\"salt\"/>" } else { "<e a=\"1\" b=\"2\"/>" });
    }
    s.push_str("</r>");
    s
}

#[test]
fn fifty_thousand_parts_against_fifty_thousand_cost_a_sort_of_their_names() {
    let n = 50_000usize;
    let names: Vec<String> = (0..n).map(|i| format!("/{}/pages/page{:05}.xml", concat!("vi", "sio"), (i * 7919) % n)).collect();
    let make = |reverse: bool| -> Vec<Entry<'_>> {
        let mut v: Vec<Entry<'_>> = names.iter().map(|s| entry(s, true, b"<r/>")).collect();
        if reverse {
            v.reverse();
        }
        v
    };
    let (a, b) = (make(false), make(true));
    let mut work = Work::default();
    let d = compare_entries(&a, &b, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert!(d.same());
    // Two sorts and a merge: no more than a few comparisons of a few dozen bytes for each name and level.
    let n = n as u64;
    // Measured: 2.19 steps for each n log2 n, and 57 bytes (names of 27 bytes, sorted twice and merged).
    assert!(work.canon_steps * 10 <= 24 * n * log2_ceil(n), "{}", work.canon_steps);
    assert!(work.canon_bytes <= 60 * n * log2_ceil(n), "{}", work.canon_bytes);
    // Every part one side lacks is found in the same pass.
    let c: Vec<Entry<'_>> = a.iter().step_by(2).copied().collect();
    let mut work = Work::default();
    let d = compare_entries(&a, &c, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert_eq!(d.count, n / 2);
    assert_eq!(d.shown.len(), 200);
}

#[test]
fn a_tree_as_deep_as_the_ceiling_costs_a_step_for_each_level_and_not_for_each_level_below() {
    let deep = |leaf: &str| format!("{}{leaf}{}", "<k>".repeat(500), "</k>".repeat(500));
    let (d, work) = work_of(&deep("<x v=\"1\"/>"), &deep("<x v=\"2\"/>"));
    assert_eq!(d.count, 1);
    assert!(work.canon_steps <= 4_100, "{}", work.canon_steps);
    // Siblings at every level: a comb. Each level compares its own children, so the work is the size of the tree and not its depth times it.
    let comb = |leaf: &str| format!("{}{leaf}{}", "<k><e/>".repeat(400), "</k>".repeat(400));
    let (d, work) = work_of(&comb("<x v=\"1\"/>"), &comb("<x v=\"2\"/>"));
    assert_eq!(d.count, 1);
    assert!(work.canon_steps <= 5_800, "{}", work.canon_steps);
}

#[test]
fn many_children_that_differ_at_every_level_cost_the_size_of_the_tree() {
    // A tree in which every child differs from its twin: a count of n and a pass, not n squared.
    let n = 20_000usize;
    let list = |tag: &str| {
        let mut s = String::from("<r>");
        for i in 0..n {
            s.push_str(&format!("<e{}/>", (i * 31 + tag.len()) % 997));
        }
        s.push_str("</r>");
        s
    };
    let (d, work) = work_of(&list("a"), &list("bb"));
    assert!(d.count >= 1);
    assert!(work.canon_steps <= 34 * n as u64, "{}", work.canon_steps);
}

#[test]
fn nesting_at_the_depth_limit_costs_a_few_steps_for_each_element() {
    // 300 chains of 190 nested elements side by side: 57,000 elements at nearly the default limit of 200 (the root and the chain's ends make
    // 193). Each element costs what it costs in a flat part, and depth adds nothing: about six steps for each, measured.
    let chains = |salt: bool| {
        let mut s = String::from("<r>");
        for chain in 0..300 {
            s.push_str(&"<k>".repeat(190));
            s.push_str(if salt && chain == 150 { "<x v=\"salt\"/>" } else { "<x v=\"1\"/>" });
            s.push_str(&"</k>".repeat(190));
        }
        s.push_str("</r>");
        s
    };
    let mut work = Work::default();
    let d = diff_limited(&chains(false), &chains(true), &RULES, &XmlLimits::default(), &mut work);
    assert_eq!(d.count, 1);
    let elements = 300 * 191;
    assert!(work.canon_steps * 10 <= 62 * elements, "{} steps for {elements} elements", work.canon_steps);
    assert!(work.canon_steps * 10 >= 58 * elements, "{} steps for {elements} elements", work.canon_steps);
    // One more level and the parser refuses it: the limit is the parser's, and the check adds none of its own below the ceiling.
    let too_deep = format!("<r>{}<x/>{}</r>", "<k>".repeat(200), "</k>".repeat(200));
    let (a, b) = (too_deep.clone(), too_deep.replace("<x/>", "<x />"));
    let err = compare_entries(
        &[entry("/p.xml", true, a.as_bytes())],
        &[entry("/p.xml", true, b.as_bytes())],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    );
    assert_eq!(err.unwrap_err().class(), "xml_too_complex");
}

#[test]
fn siblings_that_stand_in_another_order_at_every_level_cost_the_siblings_of_all_the_levels() {
    // A chain in which each level rests on the one above: 100 levels, and at each 100 siblings that stand in another order on the other side,
    // the next level among them. The levels below differ, so no level is a pure reordering: each is set side by side place by place, and
    // sorts its own 100 hashes to see whether it is one. The work is the siblings of all the levels, and not their square.
    let build = |reverse: bool| {
        let mut s = String::new();
        for _ in 0..100 {
            s.push_str("<k>");
            let ids: Vec<usize> = if reverse { (0..100).rev().collect() } else { (0..100).collect() };
            for id in ids {
                s.push_str(&format!("<s i=\"{id}\"/>"));
            }
        }
        s.push_str(&"</k>".repeat(100));
        s
    };
    let (d, work) = work_of(&build(false), &build(true));
    assert!(d.count >= 100, "{}", d.count);
    let siblings = 100 * 100;
    // Measured: 23.1 steps for each sibling.
    assert!(work.canon_steps <= 24 * siblings, "{} steps for {siblings} siblings", work.canon_steps);
}

// ------------------------------------------------------------------------------------------------- the hash only aligns

/// Pairs of documents and whether they have the same content: what the tests above settle, run again with the hash cut to a few bits.
const PAIRS: &[(&str, &str, bool)] = &[
    // The same content, written another way.
    ("<r><a/><b/></r>", "<r>\n <a />\n <b></b>\n</r>", true),
    (r#"<r><a x="1" y="2"/></r>"#, r#"<r><a y="2" x="1"/></r>"#, true),
    (r#"<p:r xmlns:p="urn:a"><p:x/></p:r>"#, r#"<r xmlns="urn:a"><x/></r>"#, true),
    (r#"<r><c n="1"/><c n="2"/><x/></r>"#, r#"<r><x/><c n="2"/><c n="1"/></r>"#, true),
    ("<r>a<!--x-->b<a/></r>", "<r>ab<a/></r>", true),
    ("<r><a/>a&amp;b</r>", "<r><a/><![CDATA[a&b]]></r>", true),
    (r#"<r><a len="1.50"/><a len="2"/></r>"#, r#"<r><a len="1.5"/><a len="2.0"/></r>"#, true),
    // Different content: a change at the start, in the middle, at the end; an extra and a missing child in each place; a swap.
    (r#"<r><a x="1"/><b/><c n="1"/><f/></r>"#, r#"<r><a x="2"/><b/><c n="1"/><f/></r>"#, false),
    (r#"<r><a/><b x="1"/><f/></r>"#, r#"<r><a/><b x="2"/><f/></r>"#, false),
    (r#"<r><a/><b/><f x="1"/></r>"#, r#"<r><a/><b/><f x="2"/></r>"#, false),
    ("<r><a/><b/></r>", "<r><a/><b/><z/></r>", false),
    ("<r><a/><b/></r>", "<r><z/><a/><b/></r>", false),
    ("<r><a/><x/><b/></r>", "<r><a/><b/></r>", false),
    ("<r><a/><b/><f/></r>", "<r><a/><f/><b/></r>", false),
    ("<r><a><x/></a><a><y/></a></r>", "<r><a><y/></a><a><x/></a></r>", false),
    ("<r><a><x/></a><a><y/></a></r>", "<r><a><x/></a><a><z/></a></r>", false),
    ("<r><a>1</a><a>2</a></r>", "<r><a>1</a><a>3</a></r>", false),
    ("<r><?p q?><a/></r>", "<r><?p r?><a/></r>", false),
    ("<r>x<a/>y</r>", "<r>x<a/>z</r>", false),
    ("<r><t><cp/> </t></r>", "<r><t><cp/></t></r>", false),
    (r#"<r><a x="1.50"/></r>"#, r#"<r><a x="1.5"/></r>"#, false),
    (r#"<r><a len="1.5cm"/></r>"#, r#"<r><a len="1.5mm"/></r>"#, false),
    (r#"<r><c n="1" v="a"/><c n="2" v="b"/></r>"#, r#"<r><c n="2" v="a"/><c n="1" v="b"/></r>"#, false),
    (r#"<r><c n="1" v="a"/><c n="1" v="b"/></r>"#, r#"<r><c n="1" v="b"/><c n="1" v="a"/></r>"#, false),
    ("<r/>", "<s/>", false),
    ("<r><a/></r>", "<r>a</r>", false),
    ("<r><a/></r>", "<r><?a?></r>", false),
    ("<r><a/><a/><a/></r>", "<r><a/><a/></r>", false),
];

#[test]
fn hash_collisions_never_make_different_content_the_same() {
    // With the hash cut to no bits at all every pair of children collides, and with one bit half of them do. The check says what it says
    // without them: the hash only chooses which children to set side by side, and every pair is compared in full.
    for mask in [u64::MAX, 0, 1, 0xF] {
        with_hash_mask(mask, || {
            for (a, b, expected) in PAIRS {
                assert_eq!(same(a, b), *expected, "mask {mask:#x}: {a} against {b}");
            }
        });
    }
}

#[test]
fn a_cut_hash_still_finds_a_change_at_the_planted_place_when_nothing_else_moved() {
    // The count may be different under a collision (a swap is two changes, an insertion shifts what follows), but a change in one place with
    // nothing else moved is found at its own path.
    with_hash_mask(0, || {
        only(&diff(r#"<r><a x="1"/><b/><f/></r>"#, r#"<r><a x="2"/><b/><f/></r>"#), Found::Value, "/r[0]/a[0]/@x");
        only(&diff(r#"<r><a/><b/><f x="1"/></r>"#, r#"<r><a/><b/><f x="2"/></r>"#), Found::Value, "/r[0]/f[2]/@x");
        only(&diff("<r><a/><b/></r>", "<r><a/><b/><z/></r>"), Found::Element, "/r[0]/z[2]");
    });
}

#[test]
fn every_kind_of_difference_has_its_own_word() {
    let all = [
        (Found::MissingPart, "missing_part"),
        (Found::ExtraPart, "extra_part"),
        (Found::Bytes, "bytes"),
        (Found::Element, "element"),
        (Found::Attribute, "attribute"),
        (Found::Value, "value"),
        (Found::Text, "text"),
        (Found::Instruction, "instruction"),
        (Found::Order, "order"),
    ];
    for (found, word) in all {
        assert_eq!(found.word(), word);
    }
}

// ---------------------------------------------------------------------------------------------- parts, in the order of their names

#[test]
fn a_part_on_one_side_is_found_before_and_after_the_parts_both_have() {
    let run = |a: &[Entry<'_>], b: &[Entry<'_>]| compare_entries(a, b, true, &RULES, &XmlLimits::default(), &mut Work::default()).unwrap();
    let (x, y, z) = (entry("/a", false, b"1"), entry("/b", false, b"2"), entry("/c", false, b"3"));
    // A part only the second has, that sorts first, in the middle and last; and the same for the first.
    for (first, second, found, at) in [
        (vec![y, z], vec![x, y, z], Found::ExtraPart, "/a"),
        (vec![x, z], vec![x, y, z], Found::ExtraPart, "/b"),
        (vec![x, y], vec![x, y, z], Found::ExtraPart, "/c"),
        (vec![x, y, z], vec![y, z], Found::MissingPart, "/a"),
        (vec![x, y, z], vec![x, z], Found::MissingPart, "/b"),
        (vec![x, y, z], vec![x, y], Found::MissingPart, "/c"),
    ] {
        let d = run(&first, &second);
        assert_eq!(d.shown, [Difference { part: at.to_owned(), path: String::new(), found }], "{at}");
    }
    // Several at once, and the parts they have in common are still compared.
    let (x2, z2) = (entry("/a", false, b"changed"), entry("/c", false, b"3"));
    let d = run(&[x, y, z2], &[x2, z]);
    let seen: Vec<(&str, Found)> = d.shown.iter().map(|d| (d.part.as_str(), d.found)).collect();
    assert_eq!(seen, [("/a", Found::Bytes), ("/b", Found::MissingPart)]);
}

// ---------------------------------------------------------------------------------------------------- the test hook is restored

#[test]
fn a_cut_hash_is_cut_only_inside_the_call_that_cuts_it() {
    // Two items in another order: with the full hash one `Order`; with the hash cut to nothing the items are set side by side by place, two
    // values; and after the call, one `Order` again. (A restore that does not happen leaves the next test on a cut hash.)
    let (a, b) = (r#"<r><s i="1"/><s i="2"/></r>"#, r#"<r><s i="2"/><s i="1"/></r>"#);
    assert_eq!(diff(a, b).count, 1);
    assert_eq!(with_hash_mask(0, || diff(a, b).count), 2);
    assert_eq!(diff(a, b).count, 1);
    // A panic inside the call restores it too.
    let caught = std::panic::catch_unwind(|| with_hash_mask(0, || panic!("inside")));
    assert!(caught.is_err());
    assert_eq!(diff(a, b).count, 1);
}

// ------------------------------------------------------------------------------------------- a key that is a number, and text:s

const NUMBER_KEYED: Rules = Rules {
    keyed: &[Keyed { child: Name { ns: "", local: "c" }, key: NAME_N, fold: KeyFold::Exact }],
    numbers: &[Name { ns: "", local: "n" }],
    lengths: &[],
    qnames: &[],
    paragraphs: &[],
    exact_text: &[],
    absent_when_empty: &[],
    absent_when_empty_under: &[],
    unordered: &[],
    parts_absent_when_empty: &[],
    ignored_attributes: &[],
    ignored_children: &[],
    ignored_when_extra: &[],
    free_names: &[],
    placed: &[],
    aliases: &[],
};

#[test]
fn a_key_that_is_a_number_is_the_number() {
    // `1.0` and `1` are one key when the list says the attribute holds numbers, and then the siblings are still a set; two units are two keys.
    let same_under = |a: &str, b: &str| diff_with(a, b, &NUMBER_KEYED).same();
    assert!(same_under(r#"<r><c n="1.0cm"/><c n="2cm"/></r>"#, r#"<r><c n="2.00cm"/><c n="1cm"/></r>"#));
    assert!(!same_under(r#"<r><c n="1.0cm"/><c n="2cm"/></r>"#, r#"<r><c n="2.00cm"/><c n="1mm"/></r>"#));
    assert!(!same_under(r#"<r><c n="1.0"/><c n="2"/></r>"#, r#"<r><c n="2"/><c n="3"/></r>"#));
    // Under the lists that do not, they are other keys.
    assert!(!diff(r#"<r><c n="1.0"/></r>"#, r#"<r><c n="1"/></r>"#).same());
}

#[test]
fn only_a_text_s_that_is_one_is_spaces() {
    // `text:c` is the attribute in the OpenDocument text namespace and nothing else: another attribute, in that namespace or not, makes an element.
    assert!(!same(&para("a<text:s text:foo=\"2\"/>b"), &para("a  b")));
    assert!(!same(&para("a<text:s text:foo=\"1\"/>b"), &para("a b")));
    assert!(!same(&para("a<text:s c=\"2\"/>b"), &para("a<text:s text:c=\"2\"/>b")));
    assert!(!same(&para("a<text:s c=\"2\"/>b"), &para("a<text:s c=\"3\"/>b")));
    // And the element is `s` of that namespace: another element of it, or `s` of another, is an element too.
    assert!(!same(&para("a<text:t/>b"), &para("a b")));
    assert!(!same(&para("a<x:s xmlns:x=\"urn:other\"/>b"), &para("a b")));
    assert!(!same(&para("a<s/>b"), &para("a b")));
}

// ------------------------------------------------------------------------------------------------- the counts, pinned on small inputs

/// The steps and the bytes the check counted comparing `a` with `b`, as parts, under `rules`.
fn counts(a: &str, b: &str, rules: &Rules) -> (u64, u64) {
    let mut work = Work::default();
    diff_limited(a, b, rules, &deep_limits(), &mut work);
    (work.canon_steps, work.canon_bytes)
}

#[test]
fn the_counts_of_small_inputs_are_pinned() {
    // Each count is what the check walks: an element is a step, each attribute a step and the comparisons of its sort, an item or a keyed child
    // met by the comparison a step, and the bytes are the values and the text read and the shorter of two things set side by side. They are
    // deterministic, and pinned here so that no statement that counts can change unseen; a change that moves them says why in its commit.
    let cases: [(&str, &str, &Rules, (u64, u64)); 11] = [
        (r#"<r a="1"/>"#, r#"<r a="2"/>"#, &RULES, (9, 9)),
        (r#"<r a="1" b="22"/>"#, r#"<r b="22" a="1"/>"#, &RULES, (14, 15)),
        (r#"<r><c n="1" v="a"/><c n="2" v="b"/></r>"#, r#"<r><c n="2" v="b"/><c n="1" v="x"/></r>"#, &RULES, (36, 22)),
        ("<r>xy<?p q?>z</r>", "<r>xy<?p r?>z</r>", &RULES, (11, 16)),
        ("<r><a/><b/><c n=\"1\"/></r>", "<r><b/><a/><c n=\"1\"/></r>", &RULES, (27, 10)),
        (&para("a <text:s text:c=\"2\"/>b<text:span>c</text:span>"), &para("a<text:s text:c=\"3\"/>b<text:span>d</text:span>"), &RULES, (30, 33)),
        (r#"<r><c n="1.0cm"/><c n="2cm"/></r>"#, r#"<r><c n="2.0cm"/><c n="1cm"/></r>"#, &NUMBER_KEYED, (26, 82)),
        (r#"<r><e len="1.50cm" x="1"/></r>"#, r#"<r><e len="1.5cm" x="2"/></r>"#, &RULES, (22, 30)),
        ("<r><a/></r>", "<r><a/><z/></r>", &RULES, (14, 6)),
        // One attribute under two prefixes: its values are compared in the order written and not sorted, so the sort no longer reads them (17 bytes before).
        (r#"<r xmlns:p="u" xmlns:q="u" p:k="1" q:k="22"/>"#, r#"<r xmlns:p="u" xmlns:q="u" p:k="22" q:k="1"/>"#, &RULES, (14, 14)),
        (
            r#"<r xmlns:m="urn:m" xmlns:a="uri" m:Ignorable="a  a"/>"#,
            r#"<r xmlns:m="urn:m" xmlns:a="uri" m:Ignorable="a a b"/>"#,
            &QUALIFIED,
            (9, 47),
        ),
    ];
    for (i, (a, b, rules, expected)) in cases.iter().enumerate() {
        assert_eq!(counts(a, b, rules), *expected, "case {i}: {a} against {b}");
    }
    // Parts: the names are sorted twice and merged; equal bytes are read in full once.
    let mut work = Work::default();
    let (pa, pb) = ([entry("/b", false, b"1234"), entry("/a", false, b"12")], [entry("/a", false, b"12"), entry("/c", false, b"xyz")]);
    compare_entries(&pa, &pb, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    assert_eq!((work.canon_steps, work.canon_bytes), (4, 10));
}

// ----------------------------------------------------------------------------------------- paragraph text is one string, and charged

#[test]
fn the_text_of_a_paragraph_is_charged_by_the_bytes_built_and_costs_a_few_times_its_own() {
    // `a ` a hundred thousand times in a paragraph and in a plain element: the paragraph text is built once, as long as the text (one space
    // for each run), and charged for it, so what it costs is what the text is: at most a few bytes counted for each byte of text, however many
    // words and runs of spaces it holds.
    let words = "a ".repeat(100_000);
    let (_, plain) = work_of(&format!("<r><x>{words}x</x></r>"), &format!("<r><x>{words}y</x></r>"));
    let (_, paragraph) = work_of(&para(&format!("{words}x")), &para(&format!("{words}y")));
    let text = words.len() as u64 + 1;
    // The plain text: each side's text read once, and the two compared.
    assert!(plain.canon_bytes >= 3 * text && plain.canon_bytes <= 4 * text + 200, "{}", plain.canon_bytes);
    // The paragraph text: the same, and each side's string built, and the two compared again.
    assert!(paragraph.canon_bytes >= 5 * text && paragraph.canon_bytes <= 7 * text + 400, "{} for {text}", paragraph.canon_bytes);
    assert!(paragraph.canon_steps <= 40, "{}", paragraph.canon_steps);
}

// ------------------------------------------------------------------------------------------------------ a length is read as ODF reads it

/// A list in which `len` is a length (the strict reading) and nothing else is a number.
const LENGTHS: Rules = Rules { lengths: &[Name { ns: "", local: "len" }], ..Rules::NONE };

#[test]
fn a_length_is_a_signed_decimal_and_a_unit_and_nothing_else() {
    // The respelling that ODF's own reading gives the same value: the digits written another way, in the same unit.
    assert!(same_length("2.50cm", "2.5cm") && same_length("02.5cm", "2.5cm") && !same_length("-0.50in", "-.5in"));
    assert!(same_length("-0.50in", "-0.5in") && same_length("50.0%", "50%") && same_length("1pt", "1.00pt") && same_length("3pc", "3.0pc"));
    // What it does not respell: a plus sign, an exponent, another unit, no unit, a unit that is not a length's, another case, `-0`.
    assert!(!same_length("+0.1cm", "0.1cm") && !same_length("+0.1cm", "+0.10cm") && !same_length("+1cm", "+1cm"));
    assert!(!same_length("1e-1cm", "0.1cm") && !same_length("2.5e0cm", "2.5cm") && !same_length("1E1cm", "10cm"));
    assert!(!same_length("2.5cm", "25mm") && !same_length("1", "1.0") && !same_length("2.50em", "2.5em") && !same_length("2.5CM", "2.5cm"));
    assert!(!same_length("-0cm", "0cm") && !same_length(".5cm", "0.5cm") && !same_length("5.cm", "5cm") && !same_length(" 1cm", "1cm"));
    // The units of a length and the percentage, each by itself.
    for unit in ["cm", "mm", "in", "pt", "pc", "px", "%"] {
        assert!(same_length(&format!("1.50{unit}"), &format!("1.5{unit}")), "{unit}");
    }
    // The same text in the lenient reading of a number is a number, and here it is only text.
    assert!(same_number("+0.1cm", "0.1cm") && same_number("1e-1cm", "0.1cm") && same_number("2.5e0cm", "2.5cm"));
}

#[test]
fn a_length_attribute_respells_in_its_unit_only_and_each_respelling_has_its_difference() {
    let with = |a: &str, b: &str| diff_with(&format!("<r><a len=\"{a}\"/></r>"), &format!("<r><a len=\"{b}\"/></r>"), &LENGTHS);
    assert!(with("2.50cm", "2.5cm").same());
    assert!(with("0.10in", "0.1in").same());
    // Planted: the readings the lenient number has and a length does not.
    only_len(&with("+0.1cm", "0.1cm"));
    only_len(&with("1e-1cm", "0.1cm"));
    only_len(&with("2.5e0cm", "2.5cm"));
    only_len(&with("2.5cm", "25mm"));
    only_len(&with("2.5cm", "2.6cm"));
    // The same text is the same, whatever it is.
    assert!(with("+0.1cm", "+0.1cm").same() && with("1e-1cm", "1e-1cm").same() && with("abc", "abc").same());
    // The lenient list still reads them, and an attribute in both lists is read as a length.
    let lenient = Rules { numbers: &[Name { ns: "", local: "len" }], ..Rules::NONE };
    assert!(diff_with(r#"<r><a len="+0.1cm"/></r>"#, r#"<r><a len="0.1cm"/></r>"#, &lenient).same());
    let both = Rules { numbers: &[Name { ns: "", local: "len" }], lengths: &[Name { ns: "", local: "len" }], ..Rules::NONE };
    assert!(!diff_with(r#"<r><a len="+0.1cm"/></r>"#, r#"<r><a len="0.1cm"/></r>"#, &both).same());
    // And it is the attribute named: another attribute is text.
    assert!(!diff_with(r#"<r><a x="2.50cm"/></r>"#, r#"<r><a x="2.5cm"/></r>"#, &LENGTHS).same());
}

fn only_len(d: &ContentDiff) {
    only(d, Found::Value, "/r[0]/a[0]/@len");
}

// --------------------------------------------------------------------------------------------- keys folded as the format's reader folds them

/// `c` keyed by `n` folded by ASCII case, and `d` by `n` folded as a part name.
const FOLDED: Rules = Rules {
    keyed: &[
        Keyed { child: Name { ns: "", local: "c" }, key: NAME_N, fold: KeyFold::AsciiCase },
        Keyed { child: Name { ns: "", local: "d" }, key: NAME_N, fold: KeyFold::PartName },
    ],
    ..Rules::NONE
};

#[test]
fn siblings_a_reader_takes_for_one_are_compared_in_the_order_written() {
    let under = |a: &str, b: &str| diff_with(a, b, &FOLDED);
    // Folded by ASCII case: `A` and `a` are one key, so the order of the two is the order the reader meets them in.
    assert!(!under(r#"<r><c n="A" v="1"/><c n="a" v="2"/></r>"#, r#"<r><c n="a" v="2"/><c n="A" v="1"/></r>"#).same());
    assert!(under(r#"<r><c n="A" v="1"/><c n="b" v="2"/></r>"#, r#"<r><c n="b" v="2"/><c n="A" v="1"/></r>"#).same());
    assert!(under(r#"<r><c n="A" v="1"/><c n="a" v="2"/></r>"#, r#"<r><c v="1" n="A"/><c v="2" n="a"/></r>"#).same());
    // A key that is spelled another way is a value that differs, not a child on one side: one difference, in the key.
    only(&under(r#"<r><c n="A"/></r>"#, r#"<r><c n="a"/></r>"#), Found::Value, "/r[0]/c#0/@n");
    // Folded as a part name: case, a percent escape that decodes, and the slash in front.
    let part = |a: &str, b: &str| {
        under(&format!("<r><d n=\"{a}\" v=\"1\"/><d n=\"{b}\" v=\"2\"/></r>"), &format!("<r><d n=\"{b}\" v=\"2\"/><d n=\"{a}\" v=\"1\"/></r>"))
    };
    assert!(!part("/a/b.xml", "/A/B.XML").same());
    assert!(!part("/a/b.xml", "/a/%62.xml").same());
    assert!(!part("a/b.xml", "/a/b.xml").same());
    // Not the same part: an escape that hides a slash is kept as it is, and so is one that is not UTF-8, so they are keys of their own.
    assert!(part("/a/b.xml", "/a%2Fb.xml").same());
    // A hidden backslash is kept hidden too, so it is not the key of a literal backslash (decoded, the two would be one key and be ordered).
    assert!(part("/a%5Cb.xml", "/a\\b.xml").same());
    assert!(!part("/a%5Cb.xml", "/A%5cB.xml").same());
    // An escape is two hex digits and nothing else: a `+` before a digit is no escape (`from_str_radix` would read `%+A` as a line feed), and
    // a half escape stays as it is.
    assert!(part("/a%+Ab.xml", "/a&#10;b.xml").same());
    assert!(part("/a%4", "/a%4g.xml").same());
    // Only a `%` starts an escape: two hex digits after any other character are those characters.
    assert!(!part("/cab%41.xml", "/cabA.xml").same());
    assert!(!part("/%62/x.xml", "/b/X.XML").same());
    assert!(!part("/%6A/x.xml", "/j/X.xml").same());
    assert!(part("/a%FFb.xml", "/a%FEb.xml").same());
    assert!(part("/a/b.xml", "/a/c.xml").same());
    // A key that is missing is not an empty key.
    assert!(!under("<r><d/></r>", r#"<r><d n=""/></r>"#).same());
}

// ------------------------------------------------------------------------------------------------- prefixes in the values of some attributes

/// `i:type` names types, `m:Ignorable` lists prefixes, and `Requires` lists prefixes on `m:Choice` only.
const QUALIFIED: Rules = Rules {
    qnames: &[
        Qualified { attr: Name { ns: "urn:i", local: "type" }, on: None, kind: QKind::QNames },
        Qualified { attr: Name { ns: "urn:m", local: "Ignorable" }, on: None, kind: QKind::Prefixes },
        Qualified { attr: Name { ns: "", local: "Requires" }, on: Some(Name { ns: "urn:m", local: "Choice" }), kind: QKind::Prefixes },
    ],
    ..Rules::NONE
};

#[test]
fn a_prefix_in_a_value_is_the_namespace_it_names() {
    let under = |a: &str, b: &str| diff_with(a, b, &QUALIFIED);
    let head = r#"xmlns:m="urn:m" xmlns:i="urn:i""#;
    let list = |decls: &str, value: &str| format!(r#"<r {head} {decls} m:Ignorable="{value}"/>"#);
    // A prefix renamed passes; the namespace changed, the declaration dropped, another prefix and another order do not.
    assert!(under(&list(r#"xmlns:a="urn:a""#, "a"), &list(r#"xmlns:b="urn:a""#, "b")).same());
    only(&under(&list(r#"xmlns:a="urn:a""#, "a"), &list(r#"xmlns:a="urn:b""#, "a")), Found::Value, "/r[0]/@Ignorable");
    only(&under(&list(r#"xmlns:a="urn:a""#, "a"), &list("", "a")), Found::Value, "/r[0]/@Ignorable");
    let two = |x: &str, y: &str| list(r#"xmlns:a="urn:a" xmlns:b="urn:b" xmlns:c="urn:c""#, &format!("{x} {y}"));
    assert!(under(&two("a", "b"), &two("a", "b")).same() && under(&two("a", "b"), &two("a", "\t\n b")).same());
    assert!(!under(&two("a", "b"), &two("a", "c")).same() && !under(&two("a", "b"), &two("b", "a")).same());
    // Nothing binds it on either side: the text, and only the same text is the same.
    assert!(under(&list("", "z"), &list("", "z")).same() && !under(&list("", "z"), &list("", "y")).same());
    // A value that is a name: the prefix's namespace and the local name; no prefix is the default namespace; `xml` needs no declaration.
    let typed = |decls: &str, value: &str| format!(r#"<r {head} {decls} i:type="{value}"/>"#);
    assert!(under(&typed(r#"xmlns:a="urn:a""#, "a:t"), &typed(r#"xmlns:b="urn:a""#, "b:t")).same());
    assert!(!under(&typed(r#"xmlns:a="urn:a""#, "a:t"), &typed(r#"xmlns:a="urn:a""#, "a:u")).same());
    assert!(!under(&typed(r#"xmlns:a="urn:a""#, "a:t"), &typed(r#"xmlns:a="urn:b""#, "a:t")).same());
    assert!(!under(&typed(r#"xmlns:a="urn:a""#, "a:t"), &typed("", "a:t")).same());
    // The default namespace is declared on a child that has a prefix of its own, so that what differs is only what the value names.
    let on_child = |decls: &str, value: &str| format!(r#"<r {head}><m:e {decls} i:type="{value}"/></r>"#);
    assert!(under(&on_child(r#"xmlns="urn:a""#, "t"), &on_child(r#"xmlns="urn:a""#, "t")).same());
    only(&under(&on_child(r#"xmlns="urn:a""#, "t"), &on_child(r#"xmlns="urn:b""#, "t")), Found::Value, "/r[0]/e[0]/@type");
    only(&under(&on_child(r#"xmlns="urn:a""#, "t"), &on_child("", "t")), Found::Value, "/r[0]/e[0]/@type");
    // A name with a prefix bound to the default namespace's URI is the name in the default namespace: the same name.
    assert!(under(&on_child(r#"xmlns="urn:a" xmlns:a="urn:a""#, "t"), &on_child(r#"xmlns="urn:a" xmlns:a="urn:a""#, "a:t")).same());
    // Unbound tokens are a list: `z y` is two names, and `zy` is one.
    let unbound = |value: &str| list("", value);
    only(&under(&unbound("z y"), &unbound("zy")), Found::Value, "/r[0]/@Ignorable");
    assert!(under(&unbound("z y"), &unbound("z   y")).same());
    assert!(under(&typed("", "xml:lang"), &typed(r#"xmlns:xml="http://www.w3.org/XML/1998/namespace""#, "xml:lang")).same());
    // The same text under a prefix that is bound is not the text of a prefix that is not.
    assert!(!under(&typed("", "a:t"), &typed(r#"xmlns:a="urn:a""#, "a:t")).same());
    // The attribute is the attribute named, on the element named: `Requires` is read on `m:Choice` only.
    let choice = |uri: &str| format!(r#"<r xmlns:m="urn:m" xmlns:a="{uri}"><m:Choice Requires="a"/></r>"#);
    only(&under(&choice("urn:a"), &choice("urn:b")), Found::Value, "/r[0]/Choice[0]/@Requires");
    let other = |uri: &str| format!(r#"<r xmlns:m="urn:m" xmlns:a="{uri}"><m:Other Requires="a"/></r>"#);
    assert!(under(&other("urn:a"), &other("urn:b")).same());
    // A value that is not in the list is text: no namespace is read from it.
    let plain = |uri: &str| format!(r#"<r xmlns:a="{uri}" Ignorable="a"/>"#);
    assert!(under(&plain("urn:a"), &plain("urn:b")).same());
}

// ------------------------------------------------------------------------------------------------------------ the budget of a comparison

#[test]
fn a_part_past_the_item_budget_is_refused_in_words_that_name_it() {
    // One more element than the budget holds, as two items each: `a` and `b` differ in a byte, so the check would have to read them.
    let n = MAX_COMPARE_ITEMS / 3 + 1;
    let build = |tail: &str| format!("<r>{}{tail}</r>", "<e a=\"1\" b=\"2\"/>".repeat(n));
    let (a, b) = (build("<x/>"), build("<y/>"));
    let run = |x: &str, y: &str| {
        compare_entries(
            &[entry("/pages/page9.xml", true, x.as_bytes())],
            &[entry("/pages/page9.xml", true, y.as_bytes())],
            true,
            &RULES,
            &XmlLimits::default(),
            &mut Work::default(),
        )
    };
    let err = run(&a, &b).unwrap_err();
    assert_eq!(err.class(), "xml_too_complex");
    assert!(matches!(&err, CanonError::Xml { part, .. } if part == "/pages/page9.xml"), "{err:?}");
    let words = err.to_string();
    assert!(words.contains("/pages/page9.xml") && words.contains(&MAX_COMPARE_ITEMS.to_string()), "{words}");
}

#[test]
fn parts_past_the_byte_budget_are_refused_before_any_is_read() {
    // Two parts of more than the budget between them, which are not XML and are never touched: refused from their sizes.
    let half = usize::try_from(MAX_COMPARE_BYTES / 2).unwrap() + 1;
    let (a, b) = (vec![0u8; half], vec![1u8; half]);
    let mut work = Work::default();
    let err =
        compare_entries(&[entry("/a.bin", false, &a)], &[entry("/b.bin", false, &b)], true, &RULES, &XmlLimits::default(), &mut work).unwrap_err();
    assert!(matches!(err, CanonError::TooLarge { limit, .. } if limit == MAX_COMPARE_BYTES), "{err:?}");
    assert_eq!(err.class(), "package_too_large");
    assert!(err.to_string().contains(&MAX_COMPARE_BYTES.to_string()), "{err}");
    assert_eq!((work.canon_steps, work.canon_bytes, work.xml_events), (0, 0, 0), "nothing was read");
    // The same parts a byte smaller each, and parts of exactly what the budget holds, are compared.
    let exact = usize::try_from(MAX_COMPARE_BYTES / 2).unwrap();
    let ok = compare_entries(
        &[entry("/a.bin", false, &a[..exact])],
        &[entry("/b.bin", false, &b[..exact])],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    );
    assert_eq!(ok.unwrap().count, 2, "the budget is the most, and is not refused");
    let ok = compare_entries(
        &[entry("/a.bin", false, &a[1..])],
        &[entry("/b.bin", false, &b[1..])],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    );
    assert_eq!(ok.unwrap().count, 2);
}

// ----------------------------------------------------------------------------------------------- what a false same would have hidden

#[test]
fn an_instruction_in_a_paragraph_is_content() {
    // A processing instruction among the text of a paragraph is compared, and it splits the text as an element does.
    assert!(!same(&para("a<?x y?>b"), &para("ab")));
    assert!(!same(&para("a<?x y?>b"), &para("a<?x z?>b")));
    assert!(!same(&para("a<?x y?>b"), &para("a<?w y?>b")));
    assert!(same(&para("a <?x y?> b"), &para("a  <?x y?>  b")));
    only(&diff(&para("a<?x y?>b"), &para("a<?x z?>b")), Found::Instruction, "/text[0]/p[0]/#instruction[1]");
}

#[test]
fn a_span_under_a_link_in_a_paragraph_is_not_the_paragraphs_edge() {
    // `text:a` is not a paragraph element and does not end the paragraph that is around it: the span in it keeps its edge spaces.
    let linked = |inner: &str| para(&format!("x<text:a><text:span>{inner}</text:span></text:a>"));
    assert!(!same(&linked(" y "), &linked("y")));
    assert!(same(&linked(" y "), &linked("  y  ")));
}

#[test]
fn a_carriage_return_is_white_space_when_it_is_a_character() {
    // A literal CR is a line end, and a reference to it is a character; ODF's white space is a space, a tab, a CR and a line feed.
    assert!(same(&para("a&#13;b"), &para("a b")));
    assert!(same(&para("a&#13;&#10;&#9;&#32;b"), &para("a b")));
    assert!(same(&para("&#13;a&#13;"), &para("a")));
    assert!(!same(&para("a&#160;b"), &para("a b")));
}

#[test]
fn keyed_children_of_two_sets_with_one_key_are_each_the_others_missing_and_extra() {
    // `c` and `d` are keyed by `n`: a `c` with a key that only a `d` has on the other side is one taken out and one put in.
    let d = diff(r#"<r><c n="1"/></r>"#, r#"<r><d n="1"/></r>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert!(d.shown.iter().all(|x| x.found == Found::Element));
    let paths: Vec<&str> = d.shown.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(paths, ["/r[0]/c#0", "/r[0]/d#0"]);
    // And they are told apart whichever key sorts first.
    let d = diff(r#"<r><c n="2"/><d n="1"/></r>"#, r#"<r><c n="2"/><d n="3"/></r>"#);
    assert_eq!(d.count, 2, "{:?}", d.shown);
}

#[test]
fn a_difference_in_the_matched_end_is_at_its_own_place_when_two_children_collide() {
    // With the hash cut to one bit, two children that differ can hash alike, and are then matched at the start or the end of the lists and
    // compared in full. The difference found there is at the child's own place among its siblings.
    let element = |v: usize| format!("<e v=\"{v}\"/>");
    let swapped = |a: usize, b: usize| {
        let both = |first: usize, second: usize| format!("<r>{}{}</r>", element(first), element(second));
        with_hash_mask(1, || diff(&both(a, b), &both(b, a)).count)
    };
    // Two of the same bit are matched at once (two differences); two of another bit are one `Order`.
    let (zero, mut like, mut unlike) = (0usize, Vec::new(), Vec::new());
    for n in 1..80 {
        if swapped(zero, n) == 2 { like.push(n) } else { unlike.push(n) }
    }
    assert!(like.len() >= 4 && unlike.len() >= 2, "{like:?} {unlike:?}");
    let (x1, x2, y1, y2, q, lead) = (zero, unlike[0], like[0], like[1], like[2], like[3]);
    // A list that starts with a child both sides have (so that the start is not empty), then the two that hash alike or not, then one both
    // have, then the two that hash alike: the difference at the end is at the place after the start and the middle.
    let list = |a: usize, b: usize, c: usize| format!("<r>{}{}{}{}</r>", element(lead), element(a), element(b), element(c));
    let d = with_hash_mask(1, || diff(&list(x1, q, y1), &list(x2, q, y2)));
    let paths: Vec<&str> = d.shown.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(paths, ["/r[0]/e[1]/@v", "/r[0]/e[3]/@v"], "{:?}", d.shown);
}

#[test]
fn a_refusal_has_a_cause_when_it_has_one() {
    use std::error::Error;
    let n = MAX_COMPARE_ITEMS / 3 + 1;
    let build = |tail: &str| format!("<r>{}{tail}</r>", "<e a=\"1\" b=\"2\"/>".repeat(n));
    let (a, b) = (build("<x/>"), build("<y/>"));
    let err = compare_entries(
        &[entry("/p.xml", true, a.as_bytes())],
        &[entry("/p.xml", true, b.as_bytes())],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap_err();
    assert_eq!(err.source().map(ToString::to_string), Some(format!("XML has more than {MAX_COMPARE_ITEMS} items")));
    let big = CanonError::TooLarge { bytes: 5, limit: 4 };
    assert!(big.source().is_none());
    assert_eq!(big.to_string(), "the parts to compare are 5 bytes together, larger than the 4 bytes a comparison holds");
}

// ----------------------------------------------------------------------------------------------- what a qualified value may expand to

/// A value of `tokens` prefixes, each bound to `uri`, as the document of one element: `m:Ignorable` is read as a list of prefixes.
fn long_uri_document(uri_len: usize, tokens: usize, extra: &str) -> String {
    format!("<r xmlns:m=\"urn:m\" xmlns:p=\"{}\" m:Ignorable=\"{}\"{extra}/>", "u".repeat(uri_len), vec!["p"; tokens].join(" "))
}

fn compare_qualified(a: &str, b: &str, work: &mut Work) -> Result<ContentDiff, CanonError> {
    compare_entries(&[entry("/p.xml", true, a.as_bytes())], &[entry("/p.xml", true, b.as_bytes())], true, &QUALIFIED, &XmlLimits::default(), work)
}

#[test]
fn a_qualified_value_of_many_tokens_with_a_long_uri_is_refused_before_it_is_built() {
    // 10,000 tokens, each a prefix bound to a URI of 10,000 bytes: 100 MB once each is expanded, from a value of 20 KB. Refused in words, naming
    // the part and the limit, and nothing past the limit was charged or built.
    let (a, b) = (long_uri_document(10_000, 10_000, ""), long_uri_document(10_000, 10_000, " z=\"1\""));
    let mut work = Work::default();
    let err = compare_qualified(&a, &b, &mut work).unwrap_err();
    assert!(matches!(&err, CanonError::Values { part, limit } if part == "/p.xml" && *limit == MAX_QUALIFIED_BYTES), "{err:?}");
    assert_eq!(err.class(), "xml_too_complex");
    let words = err.to_string();
    assert!(words.contains("/p.xml") && words.contains(&MAX_QUALIFIED_BYTES.to_string()), "{words}");
    // The bytes counted are the two small documents' own and at most the limit of the first side's tokens: the second side was not built.
    assert!(work.canon_bytes <= MAX_QUALIFIED_BYTES + 100_000, "{} bytes counted", work.canon_bytes);
    // Short URIs and many tokens are fine, and so are long URIs and a few: 200,000 tokens of 10 bytes each, and 100 tokens of 10,000.
    assert!(compare_qualified(&long_uri_document(8, 200_000, ""), &long_uri_document(8, 200_000, " z=\"1\""), &mut Work::default()).is_ok());
    assert_eq!(
        compare_qualified(&long_uri_document(10_000, 100, ""), &long_uri_document(10_000, 100, " z=\"1\""), &mut Work::default()).unwrap().count,
        1
    );
}

#[test]
fn what_a_qualified_value_may_expand_to_is_the_limit_exactly() {
    // A prefix bound to a URI of 1,048,574 bytes (the attribute limit is 1 MiB) is a token of 1,048,576 bytes: 16 of them are 16 MiB, the limit.
    assert_eq!(MAX_QUALIFIED_BYTES, 16 << 20);
    let uri = (1 << 20) - 2;
    let (at, over) = (long_uri_document(uri, 16, ""), long_uri_document(uri, 17, ""));
    let other = |doc: &str| doc.replace("/>", " z=\"1\"/>");
    assert_eq!(compare_qualified(&at, &other(&at), &mut Work::default()).unwrap().count, 1, "the limit itself is compared");
    let err = compare_qualified(&over, &other(&over), &mut Work::default()).unwrap_err();
    assert!(matches!(err, CanonError::Values { .. }), "{err:?}");
    // To the byte: a token is its URI and the two markers (1,048,576 for `p`, 1,048,575 for `q`), an unbound prefix is its text and its marker (2 for `x`).
    let value = |tokens: &str| {
        format!("<r xmlns:m=\"urn:m\" xmlns:p=\"{}\" xmlns:q=\"{}\" m:Ignorable=\"{tokens}\"/>", "u".repeat((1 << 20) - 2), "u".repeat((1 << 20) - 3))
    };
    let sixteen = vec!["p"; 16].join(" ");
    let fifteen_and_q = format!("{} q", vec!["p"; 15].join(" "));
    let refused =
        |tokens: &str| matches!(compare_qualified(&value(tokens), &other(&value(tokens)), &mut Work::default()), Err(CanonError::Values { .. }));
    assert!(!refused(&sixteen) && !refused(&fifteen_and_q), "16 MiB and one byte under it are compared");
    assert!(refused(&format!("{sixteen} x")), "16 MiB and an unbound prefix of 2 bytes is over");
    assert!(refused(&format!("{fifteen_and_q} x")), "one byte under 16 MiB and an unbound prefix of 2 bytes is over by one");
    assert!(!refused(&format!("{} y", vec!["p"; 15].join(" "))), "15 MiB and 2 bytes is under");
    // A name counts its local part too: `p:x` is the URI, the markers and `x`, so 16 of them are 16 bytes over, and 16 of `p:` are the limit.
    let named = |token: &str| format!("<r xmlns:i=\"urn:i\" xmlns:p=\"{}\" i:type=\"{}\"/>", "u".repeat((1 << 20) - 2), vec![token; 16].join(" "));
    assert!(compare_qualified(&named("p:"), &other(&named("p:")), &mut Work::default()).is_ok(), "16 names of an empty local part are the limit");
    assert!(
        matches!(compare_qualified(&named("p:x"), &other(&named("p:x")), &mut Work::default()), Err(CanonError::Values { .. })),
        "and of one byte more are over"
    );
    // The limit is for each side and each part, so the other side that is within it is read on its own.
    let err = compare_qualified(&other(&at), &other(&over), &mut Work::default()).unwrap_err();
    assert!(matches!(err, CanonError::Values { .. }), "{err:?}");
}

#[test]
fn one_byte_over_the_byte_budget_is_refused() {
    // The budget is the most the parts may hold between them: exactly it is compared, one byte over is refused, in every split of the bytes.
    // (Zeroed memory the check never reads: the sizes are what is compared.)
    let half = usize::try_from(MAX_COMPARE_BYTES / 2).unwrap();
    let zeros = vec![0u8; half + 1];
    let (a, b) = (&zeros[..], &zeros[..half]);
    let run = |x: &[u8], y: &[u8]| {
        compare_entries(&[entry("/a.bin", false, x)], &[entry("/b.bin", false, y)], true, &RULES, &XmlLimits::default(), &mut Work::default())
    };
    assert!(matches!(run(a, b), Err(CanonError::TooLarge { bytes, limit }) if bytes == MAX_COMPARE_BYTES + 1 && limit == MAX_COMPARE_BYTES));
    assert!(matches!(run(b, a), Err(CanonError::TooLarge { bytes, .. }) if bytes == MAX_COMPARE_BYTES + 1));
    assert_eq!(run(&a[1..], b).unwrap().count, 2, "exactly the budget is compared");
    // Several parts on a side add up.
    let (c, d) = (&zeros[..half / 2], &zeros[..half / 2 + 1]);
    let err = compare_entries(
        &[entry("/c.bin", false, c), entry("/d.bin", false, d)],
        &[entry("/b.bin", false, b)],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    );
    assert!(matches!(err, Err(CanonError::TooLarge { bytes, .. }) if bytes == MAX_COMPARE_BYTES + 1), "{err:?}");
}
