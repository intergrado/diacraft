//! Hostile XML: DTDs and entities are refused and never expanded or fetched, and depth, items and attribute sizes
//! are bounded, at the limit and one past it.

use std::time::{Duration, Instant};

use diacraft_opc::XmlLimits;
use diacraft_opc::xml::{parse, parse_with};

fn class(bytes: &[u8]) -> String {
    match parse(bytes) {
        Ok(el) => panic!("{:?} parsed as {el:?}", String::from_utf8_lossy(&bytes[..bytes.len().min(200)])),
        Err(e) => e.class().to_owned(),
    }
}

/// A hang guard and no measure of cost: the work takes milliseconds (a second and a half for the million attributes), and the point is that
/// it does not grow with what an entity would expand to, which each test also asserts by what the parse gave (an error of a class, or a
/// tree of a size). Twenty seconds is generous for a machine at load 50, and a parse that expanded the entity would not end.
const QUICK: Duration = Duration::from_secs(20);

#[test]
fn a_doctype_with_an_entity_is_refused() {
    // `<!DOCTYPE VisioDocument [<!ENTITY a "b">]>` at the start of the document part.
    assert_eq!(class(b"<!DOCTYPE VisioDocument [<!ENTITY a \"b\">]><VisioDocument/>"), "xml_entity_refused");
    assert_eq!(class(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<!DOCTYPE VisioDocument [\r\n<!ENTITY a \"b\">\r\n]>\r\n<VisioDocument>&a;</VisioDocument>"), "xml_entity_refused");
}

#[test]
fn any_doctype_is_refused_whether_or_not_it_declares_anything() {
    // "must not contain a DTD", and R24 applies it to every part.
    for text in [
        "<!DOCTYPE a><a/>",
        "<!DOCTYPE a SYSTEM \"http://example.invalid/evil.dtd\"><a/>",
        "<!DOCTYPE a PUBLIC \"-//X//EN\" \"file:///etc/passwd\"><a/>",
        "<!DOCTYPE a [<!ELEMENT a ANY>]><a/>",
        "<!DOCTYPE a [<!ENTITY % p SYSTEM \"http://example.invalid/p.dtd\"> %p;]><a/>",
        "<!DOCTYPE a [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><a>&x;</a>",
        "<!-- c --><!DOCTYPE a><a/>",
        "<?pi x?><!DOCTYPE a><a/>",
        "\n\n  <!DOCTYPE a [ <!ENTITY e \"v\"> ]  >  <a/>",
    ] {
        assert_eq!(class(text.as_bytes()), "xml_entity_refused", "{text:?}");
    }
}

#[test]
fn a_doctype_that_is_itself_broken_is_still_refused_as_an_entity_not_called_malformed() {
    // The refusal has to come first, so that a hostile DTD is not "just malformed": a malformed DTD is still a DTD.
    for text in ["<!DOCTYPE a [<!ENTITY e \"v\"><a/>", "<!DOCTYPE a [", "<!DOCTYPE", "<!DOCTYPE a [<!ENTITY e \"unclosed>]><a/>"] {
        assert_eq!(class(text.as_bytes()), "xml_entity_refused", "{text:?}");
    }
}

#[test]
fn a_doctype_in_utf_16_is_refused_too() {
    let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><!DOCTYPE a [<!ENTITY e \"v\">]><a>&e;</a>";
    let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(class(&le), "xml_entity_refused");
}

#[test]
fn billion_laughs_is_refused_before_anything_expands() {
    let mut dtd = String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n <!ENTITY lol \"lol\">\n");
    let mut prev = "lol".to_owned();
    for i in 1..=9 {
        let cur = format!("lol{i}");
        dtd.push_str(&format!(" <!ENTITY {cur} \"{}\">\n", format!("&{prev};").repeat(10)));
        prev = cur;
    }
    dtd.push_str("]>\n<lolz>&lol9;</lolz>");
    let start = Instant::now();
    assert_eq!(class(dtd.as_bytes()), "xml_entity_refused");
    assert!(start.elapsed() < QUICK, "took {:?}: something expanded", start.elapsed());
}

#[test]
fn quadratic_blowup_and_external_entities_are_refused_the_same_way() {
    // One entity of 50 KB used 50,000 times is 2.5 GB, with no nesting at all.
    let big = "x".repeat(50_000);
    let doc = format!("<!DOCTYPE a [<!ENTITY big \"{big}\">]><a>{}</a>", "&big;".repeat(50_000));
    let start = Instant::now();
    assert_eq!(class(doc.as_bytes()), "xml_entity_refused");
    assert!(start.elapsed() < QUICK);
}

#[test]
fn a_reference_to_an_entity_other_than_the_five_is_refused_and_never_dropped() {
    // The sibling this crate came from dropped them without a word, which turns a hostile reference into missing text. With no
    // DTD no such entity can be declared, so a reference to one is a refusal.
    for text in ["<a>&lol;</a>", "<a b=\"&lol;\"/>", "<a>&nbsp;</a>", "<a>&AMP;</a>", "<a>&#38;&copy;</a>", "<a>&x;&amp;</a>"] {
        assert_eq!(class(text.as_bytes()), "xml_entity_refused", "{text:?}");
    }
    // An escaped ampersand before the name is only text.
    assert_eq!(parse(b"<a>&amp;lol;</a>").unwrap().text(), "&lol;");
}

#[test]
fn a_very_long_entity_name_does_not_make_the_reader_scan_the_whole_part() {
    // `&` and then a million letters and no semicolon: not well-formed, found without a search of the rest.
    let doc = format!("<a b=\"&{}\"/>", "x".repeat(1_000_000));
    let start = Instant::now();
    assert!(parse(doc.as_bytes()).is_err());
    assert!(start.elapsed() < QUICK);
}

#[test]
fn depth_is_bounded_at_the_limit_and_one_past_it() {
    let nest = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
    // The cap is 200.
    assert!(parse(nest(200).as_bytes()).is_ok(), "200 deep is the limit");
    assert_eq!(class(nest(201).as_bytes()), "xml_too_complex");
    // With another limit.
    let limits = XmlLimits { max_depth: 3, ..XmlLimits::default() };
    assert!(parse_with(nest(3).as_bytes(), &limits).is_ok());
    assert_eq!(parse_with(nest(4).as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    // The depth counts an empty element too: `<a><a/></a>` is 2 deep.
    let two = XmlLimits { max_depth: 1, ..XmlLimits::default() };
    assert!(parse_with(b"<a/>", &two).is_ok());
    assert_eq!(parse_with(b"<a><a/></a>", &two).unwrap_err().class(), "xml_too_complex");
}

#[test]
fn a_hostile_depth_is_refused_without_a_stack_overflow() {
    // 100,000 opening tags, never closed, and the same closed.
    let open = "<a>".repeat(100_000);
    let start = Instant::now();
    assert_eq!(class(open.as_bytes()), "xml_too_complex");
    let closed = format!("{open}{}", "</a>".repeat(100_000));
    assert_eq!(class(closed.as_bytes()), "xml_too_complex");
    assert!(start.elapsed() < QUICK);
}

#[test]
fn no_limit_a_caller_gives_goes_past_the_depth_ceiling() {
    // Dropping and cloning a tree recurse once per level, so the depth a caller may ask for is capped by the crate.
    let deep = format!("{}{}", "<a>".repeat(5_000), "</a>".repeat(5_000));
    let absurd = XmlLimits { max_depth: usize::MAX, ..XmlLimits::default() };
    assert_eq!(parse_with(deep.as_bytes(), &absurd).unwrap_err().class(), "xml_too_complex");
    let at_ceiling = diacraft_opc::XML_DEPTH_CEILING;
    let ok = format!("{}{}", "<a>".repeat(at_ceiling), "</a>".repeat(at_ceiling));
    let tree = parse_with(ok.as_bytes(), &absurd).unwrap();
    // The tree of that depth can be cloned, compared and dropped on a thread of ordinary stack.
    let copy = tree.clone();
    assert_eq!(tree, copy);
    drop((tree, copy));
    let past = format!("{}{}", "<a>".repeat(at_ceiling + 1), "</a>".repeat(at_ceiling + 1));
    assert_eq!(parse_with(past.as_bytes(), &absurd).unwrap_err().class(), "xml_too_complex");
}

#[test]
fn the_item_limit_counts_elements_text_comments_instructions_and_attributes() {
    let limits = |n: usize| XmlLimits { max_nodes: n, ..XmlLimits::default() };
    // Elements: `<a><b/><b/></a>` is 3.
    assert!(parse_with(b"<a><b/><b/></a>", &limits(3)).is_ok());
    assert_eq!(parse_with(b"<a><b/><b/></a>", &limits(2)).unwrap_err().class(), "xml_too_complex");
    // Attributes: `<a x="1" y="2"/>` is 1 + 2.
    assert!(parse_with(b"<a x=\"1\" y=\"2\"/>", &limits(3)).is_ok());
    assert_eq!(parse_with(b"<a x=\"1\" y=\"2\"/>", &limits(2)).unwrap_err().class(), "xml_too_complex");
    // Text runs, comments, instructions and CDATA: `<a>t<!--c--><?p d?><![CDATA[x]]></a>` is 1 + 4.
    assert!(parse_with(b"<a>t<!--c--><?p d?><![CDATA[x]]></a>", &limits(5)).is_ok());
    assert_eq!(parse_with(b"<a>t<!--c--><?p d?><![CDATA[x]]></a>", &limits(4)).unwrap_err().class(), "xml_too_complex");
    // Whitespace between elements is a text run like any other, since the tree keeps it.
    assert!(parse_with(b"<a> <b/> </a>", &limits(4)).is_ok());
    assert_eq!(parse_with(b"<a> <b/> </a>", &limits(3)).unwrap_err().class(), "xml_too_complex");
}

#[test]
fn a_million_tiny_elements_hit_the_item_limit_before_they_hit_memory() {
    let n = 300_000;
    let doc = format!("<a>{}</a>", "<b/>".repeat(n));
    let limits = XmlLimits { max_nodes: 100_000, ..XmlLimits::default() };
    let start = Instant::now();
    assert_eq!(parse_with(doc.as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    assert!(start.elapsed() < QUICK);
}

#[test]
fn one_element_with_a_million_attributes_hits_the_same_limit_and_is_timed() {
    // A million attributes with distinct names on one element (12 MB): refused at the item limit of 100,000, quickly; and read whole under
    // the default limit of 4,000,000 items, in a bound that is generous for a loaded machine.
    let attrs: String = (0..1_000_000).map(|i| format!(" a{i}=\"\"")).collect();
    let doc = format!("<a{attrs}/>");
    assert!(doc.len() > 10_000_000, "{} bytes", doc.len());
    let start = Instant::now();
    let limits = XmlLimits { max_nodes: 100_000, ..XmlLimits::default() };
    assert_eq!(parse_with(doc.as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    assert!(start.elapsed() < QUICK, "refused in {:?}", start.elapsed());
    let start = Instant::now();
    let el = parse(doc.as_bytes()).unwrap();
    assert_eq!(el.attrs.len(), 1_000_000);
    assert!(start.elapsed() < QUICK, "read in {:?}", start.elapsed());
}

#[test]
fn attribute_names_and_values_are_bounded_at_the_limit_and_one_past_it() {
    let limits = XmlLimits { max_attr_bytes: 10, ..XmlLimits::default() };
    let value = |n: usize| format!("<a v=\"{}\"/>", "x".repeat(n));
    let name = |n: usize| format!("<a {}=\"1\"/>", "n".repeat(n));
    assert_eq!(parse_with(value(10).as_bytes(), &limits).unwrap().attr("v").map(str::len), Some(10));
    assert_eq!(parse_with(value(11).as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    assert!(parse_with(name(10).as_bytes(), &limits).is_ok());
    assert_eq!(parse_with(name(11).as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    // The value counts as written: ten characters, each a five-byte reference, are fifty bytes.
    let escaped = format!("<a v=\"{}\"/>", "&amp;".repeat(10));
    assert_eq!(parse_with(escaped.as_bytes(), &limits).unwrap_err().class(), "xml_too_complex");
    // And in a part with many attributes, each is held to it, not the sum.
    assert!(parse_with(b"<a a=\"0123456789\" b=\"0123456789\" c=\"0123456789\"/>", &limits).is_ok());
}

#[test]
fn a_large_attribute_value_under_the_default_limit_is_kept_whole() {
    // The default limit is 1 MiB (the longest in the sample-file run is 43,350 bytes): a value of 1 MiB reads, one byte more is refused.
    let big = "v".repeat(1024 * 1024);
    let el = parse(format!("<a v=\"{big}\"/>").as_bytes()).unwrap();
    assert_eq!(el.attr("v").map(str::len), Some(big.len()));
    let over = "v".repeat(1024 * 1024 + 1);
    assert_eq!(parse(format!("<a v=\"{over}\"/>").as_bytes()).unwrap_err().class(), "xml_too_complex");
}

#[test]
fn a_large_text_node_is_kept_whole() {
    let big = "t".repeat(8 * 1024 * 1024);
    let el = parse(format!("<a>{big}</a>").as_bytes()).unwrap();
    assert_eq!(el.text().len(), big.len());
}
