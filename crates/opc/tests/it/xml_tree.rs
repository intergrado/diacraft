//! The XML tree: what it keeps (order, prefixes, declarations, whitespace, comments, instructions), what
//! it normalises (XML 1.0's own rules), and what it refuses as not well-formed.

use diacraft_opc::xml::{El, Node, parse};

fn root(text: &str) -> El {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn bad(bytes: &[u8]) -> String {
    match parse(bytes) {
        Ok(el) => panic!("{:?} parsed as {el:?}", String::from_utf8_lossy(bytes)),
        Err(e) => e.class().to_owned(),
    }
}

fn els(el: &El) -> Vec<&El> {
    el.elements().collect()
}

#[test]
fn unknown_attributes_elements_and_their_order_survive_in_the_tree() {
    // vsdx-preservation V4: `x:mine` and `x:note` and their order, beside the cell a command would change.
    let shape = root("<Shape ID=\"5\" x:mine=\"1\" xmlns:x=\"urn:example\"><x:note>hi</x:note><Cell N=\"PinX\" V=\"1\"/></Shape>");
    assert_eq!(shape.name, "Shape");
    let attrs: Vec<(&str, &str)> = shape.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(attrs, [("ID", "5"), ("x:mine", "1"), ("xmlns:x", "urn:example")], "attribute order, prefix and declaration, as written");
    let kids = els(&shape);
    assert_eq!(kids.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["x:note", "Cell"]);
    assert_eq!(kids[0].prefix(), "x");
    assert_eq!(kids[0].local(), "note");
    assert_eq!(kids[0].text(), "hi");
    assert_eq!(kids[1].prefix(), "");
    assert_eq!(kids[1].attr("N"), Some("PinX"));
}

#[test]
fn attribute_values_are_kept_as_strings_and_never_read_as_numbers() {
    // `V="0.10"` stays `0.10`, `V="1.#INF"` stays too, a formula stays as typed.
    let cell = root("<Cell N=\"Width\" V=\"1.50\" F=\"Height*1.5\" U=\"MM\" E=\"#REF!\" W='1.#INF' Z=\"0.10\"/>");
    for (k, v) in [("V", "1.50"), ("F", "Height*1.5"), ("U", "MM"), ("E", "#REF!"), ("W", "1.#INF"), ("Z", "0.10")] {
        assert_eq!(cell.attr(k), Some(v));
    }
    assert_eq!(cell.attrs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["N", "V", "F", "U", "E", "W", "Z"]);
}

#[test]
fn whitespace_between_elements_is_kept_as_text() {
    // "whitespace between elements, when the part keeps its layout".
    let el = root("<a xml:space=\"preserve\">\n  <b/>\n  <c/> \n</a>");
    let kinds: Vec<String> = el
        .children
        .iter()
        .map(|n| match n {
            Node::Text(t) => format!("text:{t:?}"),
            Node::El(e) => format!("el:{}", e.name),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["text:\"\\n  \"", "el:b", "text:\"\\n  \"", "el:c", "text:\" \\n\""]);
}

#[test]
fn text_in_an_element_that_is_documented_to_hold_none_is_kept() {
    // "text in elements that are not documented to hold any, such as `Cell`".
    let cell = root("<Cell N=\"X\">stray text<Cell/>more</Cell>");
    assert_eq!(cell.children.len(), 3);
    assert_eq!(cell.children[0], Node::Text("stray text".to_owned()));
    assert_eq!(cell.children[2], Node::Text("more".to_owned()));
}

#[test]
fn comments_instructions_and_cdata_are_nodes_in_place() {
    // Comments and processing instructions; `<Cell N="ThemeIndex" V="33"><?NewValue V?></Cell>`.
    let cell = root("<Cell N=\"ThemeIndex\" V=\"33\"><?NewValue V?></Cell>");
    assert_eq!(cell.children, [Node::Pi { target: "NewValue".to_owned(), data: "V".to_owned() }]);
    let el = root("<a><!-- first --><b/><?proc some data?><![CDATA[<raw & text>]]><!--second--></a>");
    assert_eq!(el.children.len(), 5);
    assert_eq!(el.children[0], Node::Comment(" first ".to_owned()));
    assert!(matches!(&el.children[1], Node::El(e) if e.name == "b"));
    assert_eq!(el.children[2], Node::Pi { target: "proc".to_owned(), data: "some data".to_owned() });
    assert_eq!(el.children[3], Node::CData("<raw & text>".to_owned()));
    assert_eq!(el.children[4], Node::Comment("second".to_owned()));
}

#[test]
fn a_markup_compatibility_element_is_an_ordinary_element_with_its_fallback_inside() {
    // vsdx-preservation V11: kept whole as an unknown element; the namespace declarations are on it.
    let shape = root(
        "<Shape><mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice Requires=\"x\" xmlns:x=\"urn:example\"/><mc:Fallback><Cell N=\"PinX\" V=\"1\"/></mc:Fallback></mc:AlternateContent></Shape>",
    );
    let ac = els(&shape)[0];
    assert_eq!((ac.prefix(), ac.local()), ("mc", "AlternateContent"));
    let kids = els(ac);
    assert_eq!(kids[0].name, "mc:Choice");
    assert_eq!(kids[0].attr("Requires"), Some("x"));
    assert_eq!(kids[0].attr("xmlns:x"), Some("urn:example"));
    assert_eq!(els(kids[1])[0].attr("N"), Some("PinX"));
}

#[test]
fn unknown_elements_keep_their_place_among_known_ones() {
    // Order carries meaning, and foreign children sit between known ones.
    let sheet = root("<Shape><Cell N=\"A\"/><f:foreign xmlns:f=\"urn:f\"/><Cell N=\"B\"/><Section N=\"S\"/><g:other xmlns:g=\"urn:g\"/></Shape>");
    assert_eq!(els(&sheet).iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Cell", "f:foreign", "Cell", "Section", "g:other"]);
}

#[test]
fn both_quote_styles_read_the_same() {
    // "readers must accept either quote style".
    let a = root("<a b='1' c=\"2\" d='say \"hi\"' e=\"it's\"/>");
    assert_eq!(a.attr("b"), Some("1"));
    assert_eq!(a.attr("c"), Some("2"));
    assert_eq!(a.attr("d"), Some("say \"hi\""));
    assert_eq!(a.attr("e"), Some("it's"));
}

#[test]
fn the_five_entities_and_character_references_read_in_text_and_attributes() {
    let el = root("<a v=\"&lt;&gt;&amp;&quot;&apos; &#65;&#x42;&#x1F600;\">&lt;&gt;&amp;&quot;&apos; &#65;&#x42;&#x1F600;</a>");
    let want = "<>&\"' AB\u{1F600}";
    assert_eq!(el.attr("v"), Some(want));
    assert_eq!(el.text(), want);
}

#[test]
fn text_made_of_several_pieces_is_one_run() {
    let el = root("<a>one &amp; two <![CDATA[three]]> four &lt; five</a>");
    assert_eq!(
        el.children,
        [Node::Text("one & two ".to_owned()), Node::CData("three".to_owned()), Node::Text(" four < five".to_owned())],
        "a CDATA section stays its own node; text and references around it are joined"
    );
    assert_eq!(el.text(), "one & two three four < five");
}

#[test]
fn a_literal_cr_lf_in_text_reads_as_lf_and_a_character_reference_stays() {
    // CR LF in text is LF; `&#13;` is a real CR.
    let el = root("<a>one\r\ntwo\rthree&#13;four&#10;five</a>");
    assert_eq!(el.text(), "one\ntwo\nthree\rfour\nfive");
    let cdata = root("<a><![CDATA[x\r\ny\rz]]></a>");
    assert_eq!(cdata.text(), "x\ny\nz");
}

#[test]
fn a_literal_line_break_in_an_attribute_reads_as_a_space_and_a_reference_stays() {
    // R8 and XML 1.0 section 3.3.3: `\r\n`, `\r`, `\n` and a tab each read as one space; `&#10;` is a line feed (a line feed written as `&#10;` reads back as one).
    let el = root("<a v=\"a\r\nb\rc\nd\te&#10;f&#13;g&#9;h\"/>");
    assert_eq!(el.attr("v"), Some("a b c d e\nf\rg\th"));
}

#[test]
fn the_xml_declaration_and_a_byte_order_mark_are_accepted_in_any_spelling() {
    for text in [
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<a/>",
        "<?xml version='1.0' encoding='utf-8'?><a/>",
        "<?xml version=\"1.0\"?><a/>",
        "<?xml version=\"1.0\" encoding=\"Utf-8\"?>\r\n<a/>",
        "\n  <a/>",
        "<a/>",
        "\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?><a/>",
        "\u{FEFF}<a/>",
    ] {
        assert_eq!(root(text).name, "a", "{text:?}");
    }
}

#[test]
fn utf_16_is_read_with_or_without_a_byte_order_mark() {
    let doc = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a v=\"\u{e9}\u{1F600}\">caf\u{e9}</a>";
    let le: Vec<u8> = doc.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = doc.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let with = |bom: [u8; 2], body: &[u8]| [&bom[..], body].concat();
    for bytes in [with([0xFF, 0xFE], &le), with([0xFE, 0xFF], &be), le.clone(), be.clone()] {
        let el = parse(&bytes).unwrap();
        assert_eq!(el.attr("v"), Some("\u{e9}\u{1F600}"));
        assert_eq!(el.text(), "caf\u{e9}");
    }
    // With no declaration at all, a byte order mark is enough (XML 1.0 appendix F).
    let bare: Vec<u8> = "\u{FEFF}<a/>".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(parse(&bare).unwrap().name, "a");
}

#[test]
fn an_encoding_other_than_utf_8_or_utf_16_is_an_error() {
    for decl in ["ISO-8859-1", "windows-1252", "US-ASCII", "UTF-32", "Shift_JIS", "bogus", ""] {
        let text = format!("<?xml version=\"1.0\" encoding=\"{decl}\"?><a/>");
        assert_eq!(bad(text.as_bytes()), "xml_malformed", "declared encoding {decl:?}");
    }
}

#[test]
fn a_declared_encoding_that_the_bytes_do_not_have_is_an_error() {
    let utf16_declared_utf8: Vec<u8> = "\u{FEFF}<?xml version=\"1.0\" encoding=\"UTF-8\"?><a/>".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(bad(&utf16_declared_utf8), "xml_malformed");
    assert_eq!(bad(b"<?xml version=\"1.0\" encoding=\"UTF-16\"?><a/>"), "xml_malformed", "UTF-8 bytes under a UTF-16 declaration");
    // UTF-32 is no encoding of this crate's.
    assert_eq!(bad(&[0xFF, 0xFE, 0x00, 0x00, b'<', 0, 0, 0]), "xml_malformed");
    assert_eq!(bad(&[0x00, 0x00, 0xFE, 0xFF, 0, 0, 0, b'<']), "xml_malformed");
}

#[test]
fn bytes_that_are_not_text_are_an_error_and_not_a_lossy_guess() {
    assert_eq!(bad(b"<a>\xFF\xFE</a>"), "xml_malformed");
    assert_eq!(bad(b"<a b=\"\xC3\"/>"), "xml_malformed");
    assert_eq!(bad(&[0xFF, 0xFE, 0x3C]), "xml_malformed", "a UTF-16 stream ending in half a unit");
    // An unpaired surrogate in UTF-16.
    let lone: Vec<u8> = [0xFEFFu16, b'<' as u16, b'a' as u16, b'>' as u16, 0xD800, b'<' as u16, b'/' as u16, b'a' as u16, b'>' as u16]
        .iter()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    assert_eq!(bad(&lone), "xml_malformed");
}

#[test]
fn not_well_formed_input_is_an_error() {
    for (what, text) in [
        ("empty", ""),
        ("blank", "  \n "),
        ("text only", "hello"),
        ("no root", "<?xml version=\"1.0\"?>"),
        ("mismatched end tag", "<a><b></a></b>"),
        ("wrong end tag", "<a></b>"),
        ("unclosed at the end", "<a><b>"),
        ("truncated start tag", "<a b=\"1"),
        ("truncated comment", "<a><!-- x"),
        ("two roots", "<a/><b/>"),
        ("root then root", "<a></a><a></a>"),
        ("text before the root", "x<a/>"),
        ("text after the root", "<a/>x"),
        ("stray end tag", "</a>"),
        ("duplicate attribute", "<a b=\"1\" b=\"2\"/>"),
        ("attribute without a value", "<a b/>"),
        ("unquoted attribute", "<a b=1/>"),
        ("less-than in an attribute", "<a b=\"<\"/>"),
        ("ampersand with no semicolon", "<a>fish & chips</a>"),
        ("ampersand in an attribute with no semicolon", "<a b=\"x & y\"/>"),
        ("character reference to NUL", "<a>&#0;</a>"),
        ("character reference to a control character", "<a>&#1;</a>"),
        ("character reference past Unicode", "<a>&#x110000;</a>"),
        ("character reference to a surrogate", "<a>&#xD800;</a>"),
        ("character reference to a non-character", "<a>&#xFFFE;</a>"),
        ("empty character reference", "<a>&#;</a>"),
        ("element name starting with a digit", "<1a/>"),
        ("element name starting with a hyphen", "<-a/>"),
        ("element name starting with a dot", "<.a/>"),
        ("element name with a character that is not a name character", "<a\u{1}b/>"),
        ("attribute name starting with a digit", "<a 1b=\"x\"/>"),
        ("attribute name starting with a hyphen", "<a -b=\"x\"/>"),
        ("an empty element name", "<:/>x"),
        ("a literal control character in text", "<a>x\u{1}y</a>"),
        ("a literal NUL in text", "<a>x\0y</a>"),
        ("a literal control character in an attribute", "<a b=\"x\u{8}y\"/>"),
        ("a literal control character in CDATA", "<a><![CDATA[x\u{1f}y]]></a>"),
        ("a literal control character in a comment", "<a><!-- x\u{2}y --></a>"),
        ("a literal control character in an instruction", "<a><?pi x\u{3}y?></a>"),
        ("the non-character U+FFFE in text", "<a>x\u{fffe}y</a>"),
        ("the non-character U+FFFF in an attribute", "<a b=\"\u{ffff}\"/>"),
        ("`]]>` in character data", "<a>x ]]> y</a>"),
        ("`--` inside a comment", "<a><!-- a -- b --></a>"),
        ("a comment that ends in a hyphen", "<a><!-- a ---></a>"),
        ("attributes not separated by white space", "<a b=\"1\"c=\"2\"/>"),
        ("attributes not separated, single quotes", "<a b='1'c='2'/>"),
        ("attributes not separated, in a start tag", "<a b=\"1\"c=\"2\"></a>"),
        ("attributes not separated after other attributes", "<a x=\"0\" b=\"1\"c=\"2\" y=\"3\"/>"),
        ("a declaration after the root start", "<a><?xml version=\"1.0\"?></a>"),
        ("a declaration after other content", "<!-- c --><?xml version=\"1.0\"?><a/>"),
    ] {
        assert_eq!(bad(text.as_bytes()), "xml_malformed", "{what}: {text:?}");
    }
}

#[test]
fn a_truncated_document_is_never_taken_for_a_whole_one() {
    // The sibling this crate came from folded an unclosed element up and returned it. A truncated part is damaged, and the
    // caller must hear it, so every cut inside the root is an error and every cut after it is not.
    let doc =
        "<?xml version=\"1.0\"?>\n<VisioDocument xmlns=\"x\">\n  <Cell N=\"A\" V=\"1\"/>\n  <Section N=\"S\"><Row/></Section>\n</VisioDocument>\n";
    let root_end = doc.find("</VisioDocument>").unwrap() + "</VisioDocument>".len();
    for cut in 0..doc.len() {
        let result = parse(&doc.as_bytes()[..cut]);
        if cut >= root_end {
            assert!(result.is_ok(), "cut at {cut} is after the root");
        } else {
            assert_eq!(result.unwrap_err().class(), "xml_malformed", "cut at {cut}: {:?}", &doc[..cut]);
        }
    }
}

#[test]
fn comments_instructions_and_blanks_around_the_root_are_allowed() {
    let el = root("<?xml version=\"1.0\"?>\n<!-- before --><?pi before?>\n<a/>\n<!-- after --><?pi after?>\n  \n");
    assert_eq!(el.name, "a");
}

#[test]
fn the_element_api_matches_by_local_name_and_by_full_name() {
    let el = root("<v:Doc xmlns:v=\"urn:v\" v:id=\"1\" id=\"2\" r:rid=\"3\" xmlns:r=\"urn:r\"><v:A k=\"1\"/><B/><v:A k=\"2\"/>text</v:Doc>");
    assert_eq!((el.prefix(), el.local()), ("v", "Doc"));
    assert_eq!(el.attr("v:id"), Some("1"), "by the name as written");
    assert_eq!(el.attr("id"), Some("2"));
    assert_eq!(el.attr("rid"), None, "`attr` is by the name as written");
    assert_eq!(el.attr_local("rid"), Some("3"), "`attr_local` ignores the prefix");
    assert_eq!(el.attr_local("id"), Some("1"), "the first in the order written");
    assert_eq!(el.attr_local("xmlns"), None, "a namespace declaration is not an attribute of the element's own kind");
    assert_eq!(el.attr("xmlns:v"), Some("urn:v"), "but it is there, by its full name");
    assert_eq!(el.child("A").unwrap().attr("k"), Some("1"));
    assert!(el.child("B").is_some() && el.child("C").is_none());
    assert_eq!(el.elements().count(), 3);
    assert_eq!(el.text(), "text");
}

#[test]
fn attributes_with_a_namespace_prefix_and_xml_space_read_as_written() {
    let el = root("<Page xml:space=\"preserve\" xmlns:r=\"urn:r\"><Rel r:id=\"rId1\"/></Page>");
    assert_eq!(el.attr("xml:space"), Some("preserve"));
    assert_eq!(els(&el)[0].attr("r:id"), Some("rId1"));
}

#[test]
fn what_is_well_formed_and_a_strict_reader_might_trip_on_still_reads() {
    // The other side of the checks above: spacing and shapes that XML 1.0 allows.
    for text in [
        "<a b=\"1\" c=\"2\"/>",
        "<a b=\"1\"\tc=\"2\"\n/>",
        "<a  b = \"1\"   c\n=\n'2' />",
        "<a b=\"1\"></a>",
        "<a b=\"say 'hi'\" c='say \"hi\"'/>",
        "<a>x ]] y ] > z]]&gt;</a>",
        "<a><!-- a - b --></a>",
        "<a><!----></a>",
        "<a>tab\there</a>",
        "<a>\u{7f}\u{80}\u{9f}\u{a0}\u{d7ff}\u{e000}\u{fffd}\u{10000}\u{10ffff}</a>",
    ] {
        parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e}"));
    }
}

#[test]
fn a_character_reference_may_have_leading_zeros_and_zero_itself_is_no_character() {
    // Review finding 8: `&#0000000065;` is legal XML (it is `A`) and was refused for being nine digits long.
    assert_eq!(root("<a>&#0000000065;&#x000000041;&#00000000000000000000066;</a>").text(), "AAB");
    assert_eq!(root("<a v=\"&#00065;&#x0041;\"/>").attr("v"), Some("AA"));
    for text in ["<a>&#0000000000;</a>", "<a>&#x0;</a>", "<a>&#00000000;</a>", "<a>&#0001;</a>"] {
        assert_eq!(bad(text.as_bytes()), "xml_malformed", "{text:?}");
    }
    // A number that is long with digits that count is too large to be a character; one of a hundred digits is not a reference at all.
    assert_eq!(bad(b"<a>&#x110000;</a>"), "xml_malformed");
    assert_eq!(bad(b"<a>&#123456789;</a>"), "xml_malformed");
    assert_eq!(bad(format!("<a>&#{}65;</a>", "0".repeat(100)).as_bytes()), "xml_malformed");
}
