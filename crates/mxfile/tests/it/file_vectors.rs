//! The vectors of that the compressed page and the frame of the reader answer: file (the three steps of R11 and
//! their fallbacks, and the budget of R14), the DOCTYPE part, and the walk of an `mxfile` to its pages, by name and in order; and, from the line
//! "" down, those the file, the pages and the cells answer (the last two parts, and rule by rule). Each test is named
//! after its vector or its rule.

use diacraft_container::inflate::{self, Limits as InflateLimits};
use diacraft_container::xml::El;
use diacraft_container::{Work, XmlLimits};
use diacraft_mxfile::cells::{Cell, Graph, Link, Role};
use diacraft_mxfile::decode::{self, Fail, Step};
use diacraft_mxfile::file::{self, Mxfile, PageSource};
use diacraft_mxfile::geom::{self, Geometry, Pt, Rect};
use diacraft_mxfile::{DrawioError, ErrorKind, Form, Limits, Reading, Report, read};
use diacraft_testkit::mxfile as enc;

use crate::{limits, name0};

/// Test the text (a compressed page of one root cell and one layer): Base64 of the raw DEFLATE of the URI-encoded XML below.
const V1_TEXT: &str = "UzV2zq1wL0osyPDNT0nNUTV2VTV2LsrPL4GwciucU3NyVI0MMlNUjV1UjYwMgFjVyA2HrCFY1qAgsSg1rwSLBiADYTaQg2Y1AA==";
/// What the text inflates to, in ASCII.
const V1_URI: &str =
    "%3CmxGraphModel%3E%3Croot%3E%3CmxCell%20id%3D%220%22%2F%3E%3CmxCell%20id%3D%221%22%20parent%3D%220%22%2F%3E%3C%2Froot%3E%3C%2FmxGraphModel%3E";
/// What the text decodes to.
const V1_XML: &str = "<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel>";
/// The text: one shape, id 2, whose label has a `%`, a `&` and an `é`.
const V2_TEXT: &str = "jVFBDsIgEHzN3ltQo0dF7clHkLCWJlAaSrX9vRS2Nh6aeCCZndmZTBbgwo6Vl51+OIUG+A248M6FjOwo0BhgRaOAX4GxIj5g9w21TGrRSY9t+MfAsuElzYCZEfIJgsP5FOl9NO9hjjhI2wG/RGydx2zqw2TI5N3QKpwzy7SVMtEHHDd7JYpKVegsBj/FlcWQaxcT1aTx3aigM3UkSmNTa8rcESf7PNff3PUEEdAVlnG9dtJ+PuMD";
/// The text: the raw DEFLATE of the XML, never URI-encoded.
const V4_TEXT: &str = "s8mtcC9KLMjwzU9JzbGzKcrPL7Gzya1wTs3JUchMsVUyUNJH4RsqKRQkFqXmlUCl9CE69FGMAQA=";
/// The text: the URI-encoded text in a zlib stream.
const V5_TEXT: &str = "eNpTNXbOrXAvSizI8M1PSc1RNXZVNXYuys8vgbByK5xTc3JUjQwyU1SNXVSNjAyAWNXIDYesIVjWoCCxKDWvBIsGIANhNpCDZjUA9xsm+w==";
/// The text: inflates to `%3CmxGraphModel%3E%ZZ`.
const V6_TEXT: &str = "UzV2zq1wL0osyPDNT0nNUTV2VY2KAgA=";

/// The element `el`'s child elements named `name`.
fn named<'a>(el: &'a El, name: &str) -> Vec<&'a El> {
    el.elements().filter(|e| e.name == name).collect()
}

/// The cells of a decoded model: the `mxCell` children of its `root`.
fn cells(model: &El) -> Vec<&El> {
    assert_eq!(model.name, "mxGraphModel");
    let roots = named(model, "root");
    assert_eq!(roots.len(), 1, "one root");
    named(roots[0], "mxCell")
}

/// A file with one page whose text is `text`.
fn file_with(text: &str) -> Vec<u8> {
    format!("<mxfile><diagram name=\"P\">{text}</diagram></mxfile>").into_bytes()
}

/// The refusal reading `bytes` gives.
fn refused(bytes: &[u8]) -> diacraft_mxfile::DrawioError {
    read(bytes, limits()).expect_err("the file is refused")
}

/// The count and the cases of a warning class in a report.
fn noted(report: &diacraft_mxfile::Report, class: &str) -> (u64, Vec<String>) {
    report.warnings.get(class).map_or((0, Vec::new()), |n| (n.count, n.cases.clone()))
}

#[test]
fn v1_a_compressed_page_decodes_to_one_root_and_one_layer() {
    let limits = limits();
    // R11 step 1: Base64 gives 73 bytes starting 53 35 76 ce ad 70 2f 4a.
    let bytes = decode::base64_forgiving(V1_TEXT).unwrap();
    assert_eq!(bytes.len(), 73);
    assert_eq!(&bytes[..8], &[0x53, 0x35, 0x76, 0xce, 0xad, 0x70, 0x2f, 0x4a]);
    // Step 2: raw inflation (no zlib header here) gives the ASCII text.
    let inflated = inflate::raw(&bytes, &InflateLimits { max_out: 1 << 20, ratio: None }, &mut Work::default()).unwrap();
    assert_eq!(inflated, V1_URI.as_bytes());
    assert_eq!(inflate::zlib(&bytes, &InflateLimits { max_out: 1 << 20, ratio: None }, &mut Work::default()), Err(inflate::InflateError::Damaged));
    // Step 3: URI-decoding gives the XML.
    assert_eq!(decode::uri_decode(&inflated).unwrap(), V1_XML);
    // The whole pipeline, and what it cost: each step walked what it was given, once.
    let mut reading = Reading::new(limits);
    let stage = decode::compressed_text(V1_TEXT, &mut reading).unwrap();
    assert_eq!(stage.text, V1_XML);
    assert!(!stage.not_uri_encoded && !stage.zlib_wrapped);
    let work = reading.report.work;
    assert_eq!(
        (work.base64_chars, work.inflated, work.uri_bytes, work.clean_bytes),
        (V1_TEXT.len() as u64, V1_URI.len() as u64, V1_URI.len() as u64, V1_XML.len() as u64)
    );
    assert!(reading.report.warnings.is_empty(), "{:?}", reading.report.warnings);
    // Step 5: one root cell `0`, one layer `1`, nothing on it.
    let model = decode::compressed_page("P", V1_TEXT, &mut reading).unwrap().unwrap();
    let cells = cells(&model);
    assert_eq!(cells.len(), 2);
    assert_eq!((cells[0].attr("id"), cells[0].attr("parent")), (Some("0"), None));
    assert_eq!((cells[1].attr("id"), cells[1].attr("parent")), (Some("1"), Some("0")));
    // Fails if the URI step is skipped: the parse would see `%3C`.
    assert!(diacraft_container::xml::parse_with(V1_URI.as_bytes(), &XmlLimits::default()).is_err());
}

#[test]
fn v2_a_label_with_an_accent_a_percent_and_an_ampersand_comes_through_the_uri_step_and_the_xml() {
    let mut reading = Reading::new(limits());
    let model = decode::compressed_page("P", V2_TEXT, &mut reading).unwrap().unwrap();
    let cells = cells(&model);
    let shape = cells.iter().find(|c| c.attr("id") == Some("2")).expect("one shape with id 2");
    assert_eq!(shape.attr("value"), Some("Café 50% & more"), "the é from %C3%A9, the % from %25, the & from the XML entity");
    let geometry = named(shape, "mxGeometry");
    assert_eq!(geometry.len(), 1);
    for (name, want) in [("x", "10"), ("y", "20"), ("width", "80"), ("height", "40")] {
        assert_eq!(geometry[0].attr(name), Some(want), "{name}");
    }
    // The text before the parse: the bytes %C3%A9 are one character and not two.
    let stage = decode::compressed_text(V2_TEXT, &mut reading).unwrap();
    assert!(
        stage.text.contains("Café 50% &amp; more"),
        "URI-decoded once: the `%25` is a `%` and the XML's own `&amp;` is not touched: {}",
        stage.text
    );
    assert!(!stage.text.contains('Ã'), "not read as Latin-1");
}

#[test]
fn v3_white_space_inside_the_base64_and_missing_padding_read_the_same_and_a_bad_character_is_step_1() {
    let want = decode::compressed_text(V1_TEXT, &mut Reading::new(limits())).unwrap().text;
    let (a, b) = V1_TEXT.split_at(40);
    let with_line_feed = format!("{a}\n{b}");
    let unpadded = V1_TEXT.trim_end_matches('=');
    assert_eq!(V1_TEXT.len() - unpadded.len(), 2, "the vector has two `=`");
    let messy = format!("  \t{}\r\n {}\n", &V1_TEXT[..10], &V1_TEXT[10..]);
    for (what, text) in
        [("a line feed after the 40th character", with_line_feed), ("no padding", unpadded.to_owned()), ("white space around and inside", messy)]
    {
        let mut reading = Reading::new(limits());
        let got = decode::compressed_text(&text, &mut reading).unwrap_or_else(|_| panic!("{what}"));
        assert_eq!(got.text, want, "{what}");
        assert!(reading.report.warnings.is_empty(), "{what}");
        // And through the whole read of a file.
        let file = read(&file_with(&text), limits()).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(file.doc.pages.len(), 1, "{what}");
        assert!(file.report.warnings.is_empty(), "{what}: {:?}", file.report.warnings);
    }
    // A `*` in it: the page is empty and undecodable at step 1, and the file is still open.
    let (a, b) = V1_TEXT.split_at(20);
    let starred = format!("{a}*{b}");
    match decode::compressed_text(&starred, &mut Reading::new(limits())) {
        Err(Fail::Undecodable { step: Step::Base64, .. }) => {}
        other => panic!("{other:?}"),
    }
    let file = read(&file_with(&starred), limits()).unwrap();
    assert_eq!(file.doc.pages.len(), 1, "the page is there, and empty");
    assert_eq!(noted(&file.report, "drawio_page_undecodable"), (1, vec!["P: step 1 (Base64)".to_owned()]));
}

#[test]
fn v4_xml_that_was_never_uri_encoded_is_read_and_says_so() {
    let mut reading = Reading::new(limits());
    let stage = decode::compressed_text(V4_TEXT, &mut reading).unwrap();
    assert_eq!(stage.text, V1_XML);
    assert!(stage.not_uri_encoded && !stage.zlib_wrapped);
    assert_eq!(reading.report.work.uri_bytes, 0, "the URI step was not run: it would change every `%` sequence in a label");
    let file = read(&file_with(V4_TEXT), limits()).unwrap();
    assert_eq!(noted(&file.report, "drawio_page_not_uri_encoded"), (1, vec!["P".to_owned()]));
    // Raw text that begins with white space, then `<`, is the same; one that begins with `%` is URI-encoded and is not.
    let spaced = enc::base64(&enc::deflate_raw(format!("  \n{V1_XML}").as_bytes(), 9));
    assert!(decode::compressed_text(&spaced, &mut Reading::new(limits())).unwrap().not_uri_encoded);
    assert!(!decode::compressed_text(V1_TEXT, &mut Reading::new(limits())).unwrap().not_uri_encoded);
}

#[test]
fn v5_a_zlib_wrapped_page_is_inflated_as_zlib_after_the_raw_inflate_fails() {
    let mut reading = Reading::new(limits());
    let stage = decode::compressed_text(V5_TEXT, &mut reading).unwrap();
    assert_eq!(stage.text, V1_XML);
    assert!(stage.zlib_wrapped && !stage.not_uri_encoded);
    let file = read(&file_with(V5_TEXT), limits()).unwrap();
    assert_eq!(noted(&file.report, "drawio_page_zlib_wrapped"), (1, vec!["P".to_owned()]));
    assert!(!file.report.warnings.contains_key("drawio_page_undecodable"));
    // A zlib stream whose checksum is wrong is damaged, and so is no stream at all behind a valid header: the page is empty at step 2.
    let mut bad = decode::base64_forgiving(V5_TEXT).unwrap();
    let last = bad.len() - 1;
    bad[last] ^= 0x55;
    match decode::compressed_text(&enc::base64(&bad), &mut Reading::new(limits())) {
        Err(Fail::Undecodable { step: Step::Inflate, .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn v6_a_bad_percent_sequence_is_step_3_and_the_page_is_empty_and_the_file_open() {
    match decode::compressed_text(V6_TEXT, &mut Reading::new(limits())) {
        Err(Fail::Undecodable { step: Step::UriDecode, .. }) => {}
        other => panic!("{other:?}"),
    }
    let two = format!("{}{}", enc::plain_page("Fine", ""), "<diagram name=\"Broken\">UzV2zq1wL0osyPDNT0nNUTV2VY2KAgA=</diagram>");
    let file = read(format!("<mxfile>{two}</mxfile>").as_bytes(), limits()).unwrap();
    assert_eq!(file.doc.pages.len(), 2, "the other page is read");
    assert_eq!(noted(&file.report, "drawio_page_undecodable"), (1, vec!["Broken: step 3 (URI decoding)".to_owned()]));
    // Bytes that are not UTF-8 once decoded are the same step.
    assert!(matches!(decode::decode_inflated(b"%C3%28", &mut Reading::new(limits())), Err(Fail::Undecodable { step: Step::UriDecode, .. })));
}

#[test]
fn v7_a_bomb_stops_at_its_ratio_with_no_page_opened_and_a_page_over_the_cap_is_too_large() {
    // 100 MiB of the letter A is about 100 KiB: a ratio of a thousand. It is refused at the ratio, once the output has passed what 200 times its own size allows (about
    // 20 MiB), long before the 64 MiB of the cap, and no page of the file opens, the first one included.
    let stream = enc::bomb(b'A', 100 << 20);
    assert!(stream.len() < 200 << 10, "{}", stream.len());
    let text = enc::base64(&stream);
    let file = format!("<mxfile>{}<diagram name=\"Bomb\">{text}</diagram></mxfile>", enc::plain_page("Before", ""));
    let mut reading = Reading::new(limits());
    let e = match decode::compressed_page("Bomb", &text, &mut reading) {
        Err(e) => e,
        Ok(_) => panic!("a bomb is refused"),
    };
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_ratio_exceeded"));
    assert!(e.words.contains("Bomb"), "{}", e.words);
    let at = 200 * stream.len() as u64;
    let made = reading.report.work.inflated;
    assert!(made > at && made <= at + 64 * 1024, "stopped at the ratio, with at most a chunk past it: {made} against {at}");
    assert!(made < 64 << 20, "and not at the cap");
    let e = refused(file.as_bytes());
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_ratio_exceeded"));

    // 2 MiB of A (about 2 KiB, a ratio near 1,000): refused once the output passes 1 MiB, the floor of the ratio.
    let small = enc::base64(&enc::bomb(b'A', 2 << 20));
    let mut reading = Reading::new(limits());
    let e = decode::compressed_text(&small, &mut reading).expect_err("refused");
    let Fail::Refused(e) = e else { panic!("{e:?}") };
    assert_eq!(e.class, "drawio_ratio_exceeded");
    assert!(reading.report.work.inflated > 1 << 20 && reading.report.work.inflated <= (1 << 20) + 64 * 1024, "{}", reading.report.work.inflated);

    // A real-looking page of 2 MiB at a ratio of about 30 is read.
    let cells: String = (0..30_000)
        .map(|i| {
            format!(
                "<mxCell id=\"c{i}\" value=\"cell number {}\" style=\"rounded=1;whiteSpace=wrap;html=1;\" vertex=\"1\" parent=\"1\"/>",
                i * 7919 % 10_007
            )
        })
        .collect();
    let xml = enc::bare_model(&cells);
    assert!(xml.len() > 2 << 20);
    let mut reading = Reading::new(limits());
    let text = enc::encode(&xml);
    let stage = decode::compressed_text(&text, &mut reading).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(stage.text, xml);

    // The cap is the page's, 64 MiB, and a stream that passes it at a ratio under 200 is too large (the cap, with its own class), with the page named.
    let small_cap = Limits { max_page_text: 3 << 20, ..Limits::default() };
    let big: String = (0..90_000)
        .map(|i| {
            format!(
                "<mxCell id=\"c{i}\" value=\"cell number {}\" style=\"rounded=1;whiteSpace=wrap;html=1;\" vertex=\"1\" parent=\"1\"/>",
                i * 7919 % 10_007
            )
        })
        .collect();
    let big_text = enc::encode(&enc::bare_model(&big));
    let e = decode::compressed_page("Big", &big_text, &mut Reading::new(&small_cap)).expect_err("too large");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large"));
    assert!(e.words.contains("Big") && e.words.contains("3 MiB"), "{}", e.words);
}

#[test]
fn v17_a_doctype_with_an_entity_is_refused_before_anything_is_expanded() {
    let doc = b"<!DOCTYPE mxfile [<!ENTITY a \"aaaa\">]><mxfile><diagram name=\"&a;\"/></mxfile>";
    assert_eq!(diacraft_mxfile::sniff(doc), Some(diacraft_mxfile::Kind::Document), "the file is claimed, and then refused in words");
    let e = refused(doc);
    assert_eq!((e.kind, e.class), (ErrorKind::Refused, "drawio_entity_refused"));
    assert!(e.words.contains("entity"), "{}", e.words);
    // Whatever stands before it: a declaration, a byte order mark, comments.
    for doc in [
        &b"<?xml version=\"1.0\"?>\n<!DOCTYPE mxfile [<!ENTITY a \"aaaa\">]><mxfile/>"[..],
        &b"\xEF\xBB\xBF<!-- c --><!DOCTYPE mxfile SYSTEM \"x.dtd\"><mxfile/>"[..],
        &b"<!DOCTYPE mxGraphModel [<!ENTITY a \"b\">]><mxGraphModel><root/></mxGraphModel>"[..],
        &b"<!DOCTYPE diagram [<!ENTITY a \"b\">]><diagram/>"[..],
    ] {
        assert_eq!(refused(doc).class, "drawio_entity_refused", "{}", String::from_utf8_lossy(doc));
    }
    // A DTD inside a decoded page refuses the whole file, as one in a plain file does.
    let page = enc::encode("<!DOCTYPE mxGraphModel [<!ENTITY a \"aaaa\">]><mxGraphModel><root/></mxGraphModel>");
    let e = refused(&file_with(&page));
    assert_eq!((e.kind, e.class), (ErrorKind::Refused, "drawio_entity_refused"));
    // An entity that is not one of the five is refused in a plain page (it is not expanded).
    assert!(read(b"<mxfile><diagram name=\"&a;\"/></mxfile>", limits()).is_err());
}

#[test]
fn nesting_that_is_too_deep_is_refused_in_the_file_and_in_a_decoded_page_without_recursing() {
    // An `mxfile` nested 100 elements deep in unknown elements.
    let deep = format!("<mxfile>{}{}</mxfile>", "<a>".repeat(100), "</a>".repeat(100));
    let e = refused(deep.as_bytes());
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_deep"));
    assert!(e.words.contains("64"), "{}", e.words);
    // 64 levels, the root's included, are read, and 65 are not.
    let at = format!("<mxfile>{}{}</mxfile>", "<a>".repeat(63), "</a>".repeat(63));
    assert!(read(at.as_bytes(), limits()).is_ok());
    let past = format!("<mxfile>{}{}</mxfile>", "<a>".repeat(64), "</a>".repeat(64));
    assert_eq!(refused(past.as_bytes()).class, "drawio_too_deep");
    // The same in a page that was compressed.
    let page = enc::encode(&format!("<mxGraphModel>{}{}</mxGraphModel>", "<a>".repeat(100), "</a>".repeat(100)));
    assert_eq!(refused(&file_with(&page)).class, "drawio_too_deep");
}

// ------------------------------------------------------------------------------------------------------------------------------------ the frame: by name and order

#[test]
fn v8_pages_are_named_in_order_by_their_names_or_by_their_place() {
    // An empty `diagram` named One, and one with no name: "One" and "Page-2".
    let doc = "<mxfile><diagram id=\"a\" name=\"One\"></diagram><diagram id=\"b\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel></diagram></mxfile>";
    let file = read(doc.as_bytes(), limits()).unwrap();
    let names: Vec<&str> = file.doc.pages.iter().map(|p| &*p.name).collect();
    assert_eq!(names, ["One", "Page-2"]);
    assert_eq!((file.report.pages, file.report.form), (2, Form::Plain));
    assert!(file.report.warnings.is_empty(), "{:?}", file.report.warnings);
    file.doc.check_limits().unwrap();
}

#[test]
fn page_names_are_made_unique_cut_and_free_of_controls_and_a_page_may_have_neither_name_nor_id() {
    let long = "N".repeat(300);
    let doc = format!(
        "<mxfile><diagram name=\"A\"/><diagram name=\"A\"/><diagram name=\"A\"/><diagram name=\"\"/><diagram name=\"  \"/><diagram name=\"x&#9;y\"/><diagram name=\"{long}\"/><diagram name=\"{long}\"/><diagram/></mxfile>"
    );
    let file = read(doc.as_bytes(), limits()).unwrap();
    let names: Vec<String> = file.doc.pages.iter().map(|p| p.name.to_string()).collect();
    assert_eq!(&names[..3], ["A", "A (2)", "A (3)"]);
    assert_eq!(&names[3..5], ["Page-4", "Page-5"], "an empty name and a blank one are the default");
    assert_eq!(names[5], "x y", "a control character is a space");
    assert_eq!(names[6], "N".repeat(255), "cut to the model's 255 characters");
    assert_eq!(names[7].chars().count(), 255, "cut, and made unique within the 255");
    assert!(names[7].ends_with(" (2)") && names[7] != names[6]);
    assert_eq!(names[8], "Page-9");
    file.doc.check_limits().unwrap();
    // Page ids: one repeated is counted, and the page that has none takes its place as its id, counting from 0.
    let doc =
        "<mxfile><diagram id=\"p\" name=\"One\"/><diagram id=\"p\" name=\"Two\"/><diagram name=\"Three\"/><diagram id=\"2\" name=\"Four\"/></mxfile>";
    let file = read(doc.as_bytes(), limits()).unwrap();
    // The third page has no id, so its id is 2, and the fourth page's id is 2 as well.
    assert_eq!(noted(&file.report, "drawio_duplicate_page_id"), (2, vec!["Two".to_owned(), "Four".to_owned()]));
}

#[test]
fn an_mxfile_with_no_page_is_one_empty_page_and_other_children_are_counted_and_skipped() {
    for doc in ["<mxfile/>", "<mxfile></mxfile>", "<mxfile>\n  <!-- nothing -->\n</mxfile>"] {
        let file = read(doc.as_bytes(), limits()).unwrap();
        let names: Vec<&str> = file.doc.pages.iter().map(|p| &*p.name).collect();
        assert_eq!(names, ["Page-1"], "{doc}");
        assert_eq!(noted(&file.report, "drawio_no_pages").0, 1, "{doc}");
    }
    let doc = "<mxfile><other/><diagram name=\"P\"/><thing a=\"1\"><deep/></thing></mxfile>";
    let file = read(doc.as_bytes(), limits()).unwrap();
    assert_eq!(file.doc.pages.len(), 1);
    assert_eq!(noted(&file.report, "drawio_unknown_element"), (2, vec!["other".to_owned(), "thing".to_owned()]));
    assert!(!file.report.warnings.contains_key("drawio_no_pages"));
}

#[test]
fn the_content_of_a_page_is_decided_by_its_text_and_then_by_its_first_child_element() {
    // Text that is not blank is the compressed form, and wins over a child element, with a warning. No text and a first child that is not a model is empty and
    // unreadable; an empty `diagram` is an empty page and says nothing.
    let model = enc::bare_model("");
    let both = format!("<mxfile><diagram name=\"Both\">{V1_TEXT}{model}</diagram></mxfile>");
    let file = read(both.as_bytes(), limits()).unwrap();
    assert_eq!(noted(&file.report, "drawio_page_text_and_model"), (1, vec!["Both".to_owned()]));
    assert!(!file.report.warnings.contains_key("drawio_page_undecodable"), "the text was decoded");
    assert_eq!(file.report.form, Form::Compressed);
    let odd = "<mxfile><diagram name=\"Odd\"><thing/></diagram><diagram name=\"Blank\">  \n </diagram><diagram name=\"Empty\"/></mxfile>";
    let file = read(odd.as_bytes(), limits()).unwrap();
    assert_eq!(file.doc.pages.len(), 3);
    assert_eq!(noted(&file.report, "drawio_page_unreadable"), (1, vec!["Odd".to_owned()]));
    assert_eq!(file.report.warnings.len(), 1, "{:?}", file.report.warnings);
}

#[test]
fn a_file_of_many_pages_reads_each_on_its_own_whatever_the_form_of_the_others() {
    let pages = [
        enc::plain_page("Plain", ""),
        enc::compressed_page("Packed", "<mxCell id=\"2\" vertex=\"1\" parent=\"1\"/>"),
        "<diagram name=\"Bad\">***</diagram>".to_owned(),
    ];
    let file = read(enc::mxfile(&pages).as_bytes(), limits()).unwrap();
    let names: Vec<&str> = file.doc.pages.iter().map(|p| &*p.name).collect();
    assert_eq!(names, ["Plain", "Packed", "Bad"]);
    assert_eq!(file.report.form, Form::Compressed, "one compressed page makes the file compressed");
    assert_eq!(noted(&file.report, "drawio_page_undecodable").0, 1);
    // A file of only plain pages is plain, and a bare model or a bare diagram is a page.
    assert_eq!(read(enc::mxfile(&[enc::plain_page("A", "")]).as_bytes(), limits()).unwrap().report.form, Form::Plain);
    let bare = read(enc::bare_model("").as_bytes(), limits()).unwrap();
    assert_eq!((bare.report.form, bare.report.pages), (Form::Page, 1));
    assert_eq!(&*name0(&bare.doc), "Page-1");
    let one = read(enc::compressed_page("Solo", "").as_bytes(), limits()).unwrap();
    assert_eq!((one.report.form, &*name0(&one.doc)), (Form::Page, "Solo"));
    let plain = read(enc::plain_page("Lone", "").as_bytes(), limits()).unwrap();
    assert_eq!(&*name0(&plain.doc), "Lone");
}

#[test]
fn more_pages_than_the_model_holds_are_refused_at_the_page_that_passes_the_cap() {
    let few = Limits { max_pages: 3, ..Limits::default() };
    let doc = |n: usize| format!("<mxfile>{}</mxfile>", "<diagram/>".repeat(n));
    assert_eq!(read(doc(3).as_bytes(), &few).unwrap().doc.pages.len(), 3);
    let e = read(doc(4).as_bytes(), &few).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert!(e.words.contains('3'), "{}", e.words);
    // The model's own cap is the default.
    assert_eq!(Limits::default().max_pages, diacraft_doc::MAX_PAGES);
    assert_eq!(read(doc(diacraft_doc::MAX_PAGES).as_bytes(), limits()).unwrap().doc.pages.len(), diacraft_doc::MAX_PAGES);
    assert_eq!(read(doc(diacraft_doc::MAX_PAGES + 1).as_bytes(), limits()).err().map(|e| e.class), Some("drawio_too_complex"));
}

#[test]
fn the_caps_of_a_file_are_held_exactly_a_file_of_the_cap_is_read_and_one_byte_more_is_not() {
    let doc = enc::mxfile(&[enc::plain_page("A", "")]);
    let exact = Limits { max_file_bytes: doc.len() as u64, ..Limits::default() };
    assert!(read(doc.as_bytes(), &exact).is_ok());
    let e = read(doc.as_bytes(), &Limits { max_file_bytes: doc.len() as u64 - 1, ..Limits::default() }).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large"));
    assert!(e.words.contains(&(doc.len()).to_string()), "the size of the file is said: {}", e.words);
    // The page's text, plain: the span of the element.
    let page = enc::plain_page("A", &"<mxCell id=\"7\" parent=\"1\"/>".repeat(100));
    let file = format!("<mxfile>{page}</mxfile>");
    let at = Limits { max_page_text: page.len() as u64, ..Limits::default() };
    assert!(read(file.as_bytes(), &at).is_ok(), "a page of the cap is read");
    let e = read(file.as_bytes(), &Limits { max_page_text: page.len() as u64 - 1, ..Limits::default() }).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large"));
    assert!(e.words.contains("\"A\""), "the page is named: {}", e.words);
    // All the pages together.
    let two = format!("<mxfile>{page}{page}</mxfile>");
    let both = Limits { max_file_text: 2 * page.len() as u64, ..Limits::default() };
    assert!(read(two.as_bytes(), &both).is_ok());
    let e = read(two.as_bytes(), &Limits { max_file_text: 2 * page.len() as u64 - 1, ..Limits::default() }).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large"));
}

#[test]
fn what_a_read_costs_is_counted_and_a_budget_of_work_refuses_in_words() {
    let file = enc::mxfile(&[enc::compressed_page("A", &"<mxCell id=\"7\" parent=\"1\"/>".repeat(50))]);
    let read_ok = read(file.as_bytes(), limits()).unwrap();
    let work = read_ok.report.work;
    assert!(
        work.xml_bytes >= file.len() as u64
            && work.base64_chars > 0
            && work.inflated > 0
            && work.uri_bytes > 0
            && work.clean_bytes > 0
            && work.xml_events > 0
    );
    assert!(work.sniff_bytes > 0 && work.sniff_bytes <= 64 * 1024, "{}", work.sniff_bytes);
    assert_eq!(work.pages, 1);
    // The same read is the same count, and a budget a step short of it refuses.
    assert_eq!(read(file.as_bytes(), limits()).unwrap().report.work, work);
    let total = work.total();
    assert!(read(file.as_bytes(), &Limits { max_read_work: total, ..Limits::default() }).is_ok());
    let e = read(file.as_bytes(), &Limits { max_read_work: total - 1, ..Limits::default() }).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert!(e.words.contains("work"), "{}", e.words);
}

#[test]
fn a_read_is_the_same_twice_and_its_document_holds_the_models_caps() {
    let file = enc::mxfile(&[enc::plain_page("A", ""), enc::compressed_page("B", ""), "<diagram name=\"C\">x</diagram>".to_owned()]);
    let a = read(file.as_bytes(), limits()).unwrap();
    let b = read(file.as_bytes(), limits()).unwrap();
    assert_eq!(a.doc, b.doc);
    assert_eq!(a.report, b.report);
    a.doc.check_limits().unwrap();
}

#[test]
fn a_warning_keeps_at_most_two_hundred_cases_each_escaped_and_cut_and_counts_them_all() {
    let long = "n".repeat(500);
    let doc = format!("<mxfile>{}</mxfile>", (0..250).map(|i| format!("<{long}{i}/>")).collect::<String>());
    let file = read(doc.as_bytes(), limits()).unwrap();
    let n = file.report.warnings.get("drawio_unknown_element").unwrap();
    assert_eq!((n.count, n.cases.len()), (250, 200));
    assert!(n.cases.iter().all(|c| c.chars().count() <= 120), "cut: {}", n.cases[0].chars().count());
    // A name is escaped: a character that is no text (here a right-to-left override, which is not a control character but is no printable one) never reaches the report as itself.
    let doc = "<mxfile><diagram id=\"d\" name=\"a\u{202e}b\"/><diagram id=\"d\" name=\"c\u{1}d\"/></mxfile>";
    assert!(read(doc.as_bytes(), limits()).is_err(), "a character XML 1.0 does not allow is refused by the parser");
    let doc = "<mxfile><diagram id=\"d\" name=\"a\u{202e}b\"/><diagram id=\"d\" name=\"a\u{202e}b\"/></mxfile>";
    let file = read(doc.as_bytes(), limits()).unwrap();
    let n = file.report.warnings.get("drawio_duplicate_page_id").unwrap();
    assert_eq!(n.cases, ["a\\u{202e}b"], "escaped as a Rust string is");
}

// The file, the pages and the cells: rule by rule, file the last two parts. Every graph a test reads is held to the
// structure's own invariants (`sound`: the fuzz target's law), so that a test of one rule cannot leave the tree broken for the next.

/// A file of one plain page named `P` whose model holds the root cell `0`, the layer `1` and then `cells`.
fn one_page(cells: &str) -> String {
    enc::mxfile(&[enc::plain_page("P", cells)])
}

/// A file of one plain page named `P` whose `root` holds exactly `cells`: for the cases that make their own root, or have none.
fn raw_page(cells: &str) -> String {
    format!("<mxfile><diagram name=\"P\"><mxGraphModel><root>{cells}</root></mxGraphModel></diagram></mxfile>")
}

/// Every page of `file` as the walk hands them on, the file's own properties and the report, within `limits`.
fn read_all(file: &str, limits: &Limits) -> Result<(Vec<PageSource>, Mxfile, Report), DrawioError> {
    let mut reading = Reading::new(limits);
    let mut pages = Vec::new();
    let mx = file::read_sources(file.as_bytes(), &mut reading, |_, page| {
        pages.push(page);
        Ok(())
    })?;
    Ok((pages, mx, reading.report))
}

/// The first page of `file` and the report of reading it.
fn first_page(file: &str) -> (PageSource, Report) {
    let (mut pages, _, report) = read_all(file, limits()).unwrap_or_else(|e| panic!("{e}"));
    assert!(!pages.is_empty(), "the file has a page");
    (pages.remove(0), report)
}

/// The cell whose id is `id`.
fn cell<'g>(g: &'g Graph, id: &str) -> &'g Cell {
    g.cell(g.by_id(id).unwrap_or_else(|| panic!("no cell {id}"))).unwrap()
}

/// The ids of the children of the cell `id`, in drawing order.
fn kids(g: &Graph, id: &str) -> Vec<String> {
    let at = g.by_id(id).unwrap_or_else(|| panic!("no cell {id}"));
    g.children(at).iter().map(|c| g.cell(*c).unwrap().id.clone()).collect()
}

/// The graph holds its own invariants.
fn sound(g: &Graph) {
    sound_with(g, limits());
}

fn sound_with(g: &Graph, limits: &Limits) {
    if let Err(why) = crate::fuzz::law::read::tree_holds(g, limits) {
        panic!("the tree is broken: {why}");
    }
}

/// The page's own settings, and the report, of a model with these attributes on its `mxGraphModel` element.
fn settings_of(attrs: &str) -> (file::PageSettings, Report) {
    let file = format!(
        "<mxfile><diagram name=\"P\"><mxGraphModel {attrs}><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel></diagram></mxfile>"
    );
    let (page, report) = first_page(&file);
    (page.settings, report)
}

/// The file's `vars` for a `vars` attribute of this text, and the report.
fn vars_of(json: &str) -> (Vec<(String, String)>, Report) {
    let file = format!("<mxfile vars=\"{}\"><diagram name=\"P\"/></mxfile>", enc::escape_attr(json));
    let (_, mx, report) = read_all(&file, limits()).unwrap_or_else(|e| panic!("{e}"));
    (mx.vars, report)
}

fn pair(k: &str, v: &str) -> (String, String) {
    (k.to_owned(), v.to_owned())
}

#[test]
fn v8_the_pages_come_with_their_place_id_and_name_and_an_empty_page_has_no_cell() {
    let doc = "<mxfile><diagram id=\"a\" name=\"One\"></diagram><diagram id=\"b\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel></diagram></mxfile>";
    let (pages, _, report) = read_all(doc, limits()).unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!((pages[0].position, pages[0].id.as_str(), pages[0].name.as_deref()), (1, "a", Some("One")));
    assert!(pages[0].graph.is_empty() && pages[0].graph.root.is_none(), "an empty diagram has no cell");
    assert_eq!(pages[0].settings, file::PageSettings::default());
    assert_eq!((pages[1].position, pages[1].id.as_str(), pages[1].name.as_deref()), (2, "b", None));
    let g = &pages[1].graph;
    sound(g);
    assert_eq!(g.len(), 2, "one root and one layer, nothing on it");
    assert_eq!(g.root.map(|r| g.cell(r).unwrap().role), Some(Role::Root));
    assert_eq!(kids(g, "0"), ["1"]);
    assert_eq!(cell(g, "1").role, Role::Layer);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // A page that cannot be decoded is the same: empty, with its settings the defaults, and the file read.
    let (pages, _, report) = read_all("<mxfile><diagram name=\"Bad\">***</diagram><diagram name=\"Ok\">  </diagram></mxfile>", limits()).unwrap();
    assert_eq!(pages.len(), 2);
    assert!(pages.iter().all(|p| p.graph.is_empty() && p.settings == file::PageSettings::default() && p.defs.is_empty()));
    assert_eq!(noted(&report, "drawio_page_undecodable").0, 1);
}

#[test]
fn v9_a_wrapper_of_any_name_is_the_cell_and_its_first_inner_cell_carries_the_rest() {
    for name in ["Thing", "UserObject", "object"] {
        let xml = format!(
            "<{name} id=\"7\" label=\"Box\" owner=\"Ann\"><mxCell style=\"rounded=1;\" vertex=\"1\" parent=\"1\"><mxGeometry width=\"40\" height=\"20\" as=\"geometry\"/></mxCell></{name}>"
        );
        let (page, report) = first_page(&one_page(&xml));
        let g = &page.graph;
        sound(g);
        let c = cell(g, "7");
        assert_eq!(c.role, Role::Shape, "{name}");
        assert_eq!(c.label.as_deref(), Some("Box"), "{name}");
        assert_eq!(c.style, "rounded=1;", "{name}");
        assert_eq!(c.user.as_deref().map(|u| u.props.clone()), Some(vec![pair("owner", "Ann")]), "{name}");
        let geo = c.geometry.as_deref().expect("a geometry");
        assert_eq!((geo.width, geo.height), (40.0, 20.0), "{name}");
        assert_eq!(kids(g, "1"), ["7"], "{name}: it is in the layer");
        assert!(report.warnings.is_empty(), "{name}: {:?}", report.warnings);
    }
    // A plain `mxCell` has no user object.
    let (page, _) = first_page(&one_page("<mxCell id=\"2\" value=\"x\" vertex=\"1\" parent=\"1\"/>"));
    assert!(cell(&page.graph, "2").user.is_none());
}

#[test]
fn r21_a_wrappers_label_beats_the_inner_value_and_its_id_beats_the_inner_id() {
    let cells = "<object id=\"a\" label=\"from the wrapper\"><mxCell value=\"from the cell\" vertex=\"1\" parent=\"1\"/></object>\
                 <object id=\"b\"><mxCell value=\"from the cell\" vertex=\"1\" parent=\"1\"/></object>\
                 <object id=\"c\" label=\"\"><mxCell value=\"from the cell\" vertex=\"1\" parent=\"1\"/></object>\
                 <object id=\"d\" label=\"x\"><mxCell vertex=\"1\" parent=\"1\"/></object>\
                 <object id=\"e\"><mxCell id=\"inner\" vertex=\"1\" parent=\"1\"/></object>\
                 <object><mxCell id=\"f\" vertex=\"1\" parent=\"1\"/></object>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    assert_eq!(cell(g, "a").label.as_deref(), Some("from the wrapper"));
    assert_eq!(cell(g, "b").label.as_deref(), Some("from the cell"), "no label: the cell's value is the label");
    assert_eq!(cell(g, "c").label.as_deref(), Some(""), "a label that is there and empty is the label");
    assert_eq!(cell(g, "d").label.as_deref(), Some("x"));
    assert!(g.by_id("inner").is_none(), "the wrapper's id wins");
    assert_eq!(kids(g, "1"), ["a", "b", "c", "d", "e", "f"], "and the wrapper with no id has the inner cell's");
    assert_eq!(noted(&report, "drawio_wrapper_inner_id"), (1, vec!["P: e".to_owned()]), "only the cell with two ids is counted");
}

#[test]
fn r21_the_special_attributes_of_a_wrapper_are_not_properties_and_every_other_one_is() {
    let cells = "<UserObject id=\"g\" label=\"L\" tooltip=\"T\" link=\"https://x.test\" linkTarget=\"_blank\" tags=\"a b\" placeholders=\"1\" owner=\"Ann\" zeta=\"9\" alpha=\"1\">\
                 <mxCell vertex=\"1\" parent=\"1\"/></UserObject>\
                 <object id=\"h\" placeholders=\"true\" style=\"s\" parent=\"p\" vertex=\"1\" value=\"v\"><mxCell vertex=\"1\" parent=\"1\"/></object>\
                 <object id=\"i\" placeholders=\"0\"><mxCell vertex=\"1\" parent=\"1\"/></object>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    let user = cell(g, "g").user.as_deref().expect("a user object");
    assert_eq!(
        (user.tooltip.as_deref(), user.link.as_deref(), user.link_target.as_deref(), user.tags.as_deref(), user.placeholders),
        (Some("T"), Some("https://x.test"), Some("_blank"), Some("a b"), true)
    );
    assert_eq!(
        user.props,
        [pair("owner", "Ann"), pair("zeta", "9"), pair("alpha", "1")],
        "the others, in the order written; `id` and `label` are not among them"
    );
    let h = cell(g, "h");
    let user = h.user.as_deref().unwrap();
    assert!(!user.placeholders, "only `1` turns the placeholders on");
    assert_eq!(
        user.props,
        [pair("style", "s"), pair("parent", "p"), pair("vertex", "1"), pair("value", "v")],
        "a wrapper's own style and parent are properties"
    );
    assert_eq!((h.style.as_str(), h.label.as_deref(), h.role), ("", None, Role::Shape), "the cell's own are the inner cell's");
    assert!(!cell(g, "i").user.as_deref().unwrap().placeholders);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn r20_a_wrapper_with_no_inner_cell_is_a_cell_with_no_parent_style_or_geometry_and_the_roots_wrapper_is_the_pages_data() {
    // The page's own data: the root cell's wrapper.
    let cells = "<UserObject id=\"0\" owner=\"Ann\" animation=\"x\"><mxCell/></UserObject><mxCell id=\"1\" parent=\"0\"/><Thing id=\"w\" label=\"x\"/><object id=\"v\"><mxCell vertex=\"1\" parent=\"1\"/><mxCell id=\"second\"/></object>";
    let (page, report) = first_page(&raw_page(cells));
    let g = &page.graph;
    sound(g);
    let root = cell(g, "0");
    assert_eq!(root.role, Role::Root);
    assert_eq!(root.user.as_deref().map(|u| u.props.clone()), Some(vec![pair("owner", "Ann"), pair("animation", "x")]));
    let w = cell(g, "w");
    assert_eq!((w.role, w.style.as_str(), w.geometry.is_none(), w.label.as_deref()), (Role::Neither, "", true, Some("x")));
    assert_eq!(kids(g, "1"), ["v", "w"], "the cell with no parent is moved to the end of the first layer");
    assert_eq!(noted(&report, "drawio_parent_missing"), (1, vec!["P: w".to_owned()]));
    assert_eq!(noted(&report, "drawio_unknown_element"), (1, vec!["mxCell".to_owned()]), "a second `mxCell` in a wrapper is skipped and counted");
}

#[test]
fn r19_every_attribute_of_the_table_is_read_and_vertex_and_edge_are_on_only_when_exactly_one() {
    let cells = "<mxCell id=\"s\" parent=\"1\" value=\"v\" style=\"st\" vertex=\"1\" connectable=\"0\" collapsed=\"1\" visible=\"0\"/>\
                 <mxCell id=\"t\" parent=\"1\" vertex=\"true\" edge=\"yes\" connectable=\"1\"/>\
                 <mxCell id=\"e\" parent=\"1\" edge=\"1\" source=\"s\" target=\"t\"/>\
                 <mxCell id=\"m\" parent=\"1\" vertex=\"1\" source=\"nope\"/>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    let s = cell(g, "s");
    assert_eq!((s.label.as_deref(), s.style.as_str(), s.vertex, s.edge), (Some("v"), "st", true, false));
    assert_eq!((s.connectable, s.collapsed, s.visible, s.role), (Some(false), true, false, Role::Shape));
    let t = cell(g, "t");
    assert_eq!((t.vertex, t.edge, t.connectable, t.collapsed, t.visible, t.role), (false, false, Some(true), false, true, Role::Neither));
    assert_eq!(cell(g, "s").connectable, Some(false));
    let e = cell(g, "e");
    assert_eq!((e.role, e.source, e.target), (Role::Connector, Link::Cell(g.by_id("s").unwrap()), Link::Cell(g.by_id("t").unwrap())));
    assert_eq!(cell(g, "m").source, Link::Missing, "a source that names no cell");
    assert_eq!((cell(g, "m").target, cell(g, "s").source), (Link::None, Link::None), "no source, no end named");
    assert_eq!((cell(g, "m").connectable, cell(g, "m").label.as_deref(), cell(g, "m").style.as_str()), (None, None, ""));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn r19_attributes_the_table_does_not_list_are_skipped_and_counted_once_a_page_by_name() {
    let cells = "<mxCell id=\"2\" parent=\"1\" vertex=\"1\" foo=\"1\" bar=\"2\"/><mxCell id=\"3\" parent=\"1\" vertex=\"1\" foo=\"3\" baz=\"4\"/>";
    let file = enc::mxfile(&[enc::plain_page("A", cells), enc::plain_page("B", cells), enc::plain_page("C", "")]);
    let (pages, _, report) = read_all(&file, limits()).unwrap();
    assert_eq!(
        noted(&report, "drawio_unknown_attribute"),
        (2, vec!["A: foo, bar, baz".to_owned(), "B: foo, bar, baz".to_owned()]),
        "one count a page, naming them once each"
    );
    assert_eq!(pages[0].graph.len(), 4, "the cells are read all the same");
    // A wrapper's own attributes are its user object and never unknown; the inner cell's are counted.
    let (_, report) = first_page(&one_page("<object id=\"2\" a=\"1\" b=\"2\"><mxCell vertex=\"1\" parent=\"1\" zzz=\"3\"/></object>"));
    assert_eq!(noted(&report, "drawio_unknown_attribute"), (1, vec!["P: zzz".to_owned()]));
    // Past eight names the case says so, and the count is still one.
    let many: String = (0..20).map(|i| format!(" a{i}=\"x\"")).collect();
    let (_, report) = first_page(&one_page(&format!("<mxCell id=\"2\" parent=\"1\" vertex=\"1\"{many}/>")));
    assert_eq!(noted(&report, "drawio_unknown_attribute"), (1, vec!["P: a0, a1, a2, a3, a4, a5, a6, a7, \u{2026}".to_owned()]));
}

#[test]
fn v10_a_cell_with_a_missing_parent_is_moved_to_the_first_layer_and_the_root_is_the_cell_0() {
    let cells = "<mxCell id=\"5\" vertex=\"1\" parent=\"9\"><mxGeometry width=\"10\" height=\"10\" as=\"geometry\"/></mxCell><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>";
    let (page, report) = first_page(&raw_page(cells));
    let g = &page.graph;
    sound(g);
    assert_eq!(g.root.map(|r| g.cell(r).unwrap().id.clone()).as_deref(), Some("0"), "not 5, the first cell with no parent");
    assert_eq!(kids(g, "0"), ["1"]);
    assert_eq!(kids(g, "1"), ["5"]);
    let five = cell(g, "5");
    assert_eq!((five.role, five.depth, five.parent), (Role::Shape, 2, g.by_id("1")));
    assert_eq!(noted(&report, "drawio_parent_missing"), (1, vec!["P: 5".to_owned()]));
    // They go after the layer's own children, in the order of the file, and a cell with no `parent` at all is counted the same.
    let cells = "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"a\" vertex=\"1\" parent=\"1\"/><mxCell id=\"o1\" vertex=\"1\"/><mxCell id=\"b\" vertex=\"1\" parent=\"1\"/><mxCell id=\"o2\" vertex=\"1\" parent=\"zzz\"/>";
    let (page, report) = first_page(&raw_page(cells));
    sound(&page.graph);
    assert_eq!(kids(&page.graph, "1"), ["a", "b", "o1", "o2"]);
    assert_eq!(noted(&report, "drawio_parent_missing"), (2, vec!["P: o1".to_owned(), "P: o2".to_owned()]));
    // Their children go with them.
    let cells = "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"o\" vertex=\"1\" parent=\"nope\"/><mxCell id=\"k\" vertex=\"1\" parent=\"o\"/>";
    let (page, _) = first_page(&raw_page(cells));
    sound(&page.graph);
    assert_eq!((kids(&page.graph, "1"), kids(&page.graph, "o")), (vec!["o".to_owned()], vec!["k".to_owned()]));
    assert_eq!((cell(&page.graph, "o").depth, cell(&page.graph, "k").depth), (2, 3));
}

#[test]
fn r23_the_root_is_the_cell_0_else_the_first_that_is_neither_vertex_nor_edge_else_the_first() {
    let root_of = |cells: &str| {
        let (page, _) = first_page(&raw_page(cells));
        sound(&page.graph);
        let g = &page.graph;
        (g.cell(g.root.unwrap()).unwrap().id.clone(), page)
    };
    let (root, _) = root_of("<mxCell id=\"x\" vertex=\"1\"/><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>");
    assert_eq!(root, "0");
    // No cell `0`: the first that is neither a vertex nor an edge; the vertex before it is moved into the first layer.
    let (root, page) = root_of("<mxCell id=\"x\" vertex=\"1\"/><mxCell id=\"r\"/><mxCell id=\"1\" parent=\"r\"/>");
    assert_eq!(root, "r");
    assert_eq!(kids(&page.graph, "1"), ["x"]);
    // Every cell flagged: the first; the edge with no parent is moved into the root's first child.
    let (root, page) = root_of("<mxCell id=\"x\" vertex=\"1\"/><mxCell id=\"y\" edge=\"1\"/><mxCell id=\"1\" vertex=\"1\" parent=\"x\"/>");
    assert_eq!(root, "x");
    assert_eq!((kids(&page.graph, "x"), kids(&page.graph, "1")), (vec!["1".to_owned()], vec!["y".to_owned()]));
    assert_eq!(cell(&page.graph, "1").role, Role::Layer, "a cell whose parent is the root is a layer, whatever its flags say");
    // A cell `0` that has a parent is not a candidate.
    let (root, _) = root_of("<mxCell id=\"0\" parent=\"q\"/><mxCell id=\"q\"/><mxCell id=\"1\" parent=\"q\"/>");
    assert_eq!(root, "q");
}

#[test]
fn r23_when_the_root_has_no_layer_the_cells_with_no_parent_and_their_children_are_left_out_and_counted() {
    let cells = "<mxCell id=\"0\"/><mxCell id=\"a\" vertex=\"1\"/><mxCell id=\"b\" vertex=\"1\" parent=\"a\"/>";
    let (page, report) = first_page(&raw_page(cells));
    let g = &page.graph;
    sound(g);
    assert!(kids(g, "0").is_empty(), "no layer takes them");
    assert_eq!((cell(g, "a").role, cell(g, "b").role), (Role::Dropped, Role::Dropped));
    assert_eq!((cell(g, "a").parent, cell(g, "a").depth), (None, 0));
    assert_eq!(
        noted(&report, "drawio_cell_dropped"),
        (2, vec!["P: a".to_owned(), "P: b".to_owned()]),
        "each cell that is left out, children included"
    );
    assert!(!report.warnings.contains_key("drawio_parent_missing"), "nothing was moved");
    // A connector that names a cell that was left out has no end there.
    let cells = "<mxCell id=\"0\"/><mxCell id=\"a\" vertex=\"1\"/><mxCell id=\"e\" edge=\"1\" parent=\"0\" source=\"a\"/>";
    let (page, _) = first_page(&raw_page(cells));
    sound(&page.graph);
    assert_eq!(cell(&page.graph, "e").role, Role::Layer, "the edge is a child of the root, so a layer");
    // A page with cells and no root at all: the first cell is the root, and nothing else has a parent.
    let (page, report) = first_page(&raw_page("<mxCell id=\"p\" vertex=\"1\"/><mxCell id=\"q\" vertex=\"1\"/>"));
    sound(&page.graph);
    assert_eq!(page.graph.root.map(|r| page.graph.cell(r).unwrap().id.clone()).as_deref(), Some("p"));
    assert_eq!(noted(&report, "drawio_cell_dropped"), (1, vec!["P: q".to_owned()]));
}

#[test]
fn v11_a_cycle_of_parents_is_cut_and_a_repeated_id_is_kept_under_a_new_one() {
    // The cycle: both shapes are in layer 1, counted, and nothing hangs.
    let cells =
        "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"3\" vertex=\"1\" parent=\"4\"/><mxCell id=\"4\" vertex=\"1\" parent=\"3\"/>";
    let (page, report) = first_page(&raw_page(cells));
    sound(&page.graph);
    assert_eq!(kids(&page.graph, "1"), ["3", "4"]);
    assert_eq!(noted(&report, "drawio_parent_cycle"), (2, vec!["P: 3".to_owned(), "P: 4".to_owned()]));
    assert!(!report.warnings.contains_key("drawio_parent_missing"), "a cell in a cycle is counted as one");
    // A cell that names itself; a cell below a cycle stays below it.
    let cells = "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"s\" vertex=\"1\" parent=\"s\"/>\
                 <mxCell id=\"a\" vertex=\"1\" parent=\"b\"/><mxCell id=\"b\" vertex=\"1\" parent=\"c\"/><mxCell id=\"c\" vertex=\"1\" parent=\"b\"/>";
    let (page, report) = first_page(&raw_page(cells));
    sound(&page.graph);
    assert_eq!(kids(&page.graph, "1"), ["s", "b", "c"], "the cells of a cycle, in the order of the file");
    assert_eq!(kids(&page.graph, "b"), ["a"]);
    assert_eq!(noted(&report, "drawio_parent_cycle").0, 3);

    // The repeated id: both kept, the connector connected to the first, a child that names the id is the first's.
    let cells = "<mxCell id=\"2\" vertex=\"1\" parent=\"1\" value=\"first\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\" value=\"second\"/>\
                 <mxCell id=\"e\" edge=\"1\" parent=\"1\" source=\"2\"/><mxCell id=\"9\" vertex=\"1\" parent=\"2\"/>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    let first = g.by_id("2").unwrap();
    assert_eq!(g.cell(first).unwrap().label.as_deref(), Some("first"));
    assert!(!g.cell(first).unwrap().renamed);
    let layer: Vec<&Cell> = g.children(g.by_id("1").unwrap()).iter().map(|c| g.cell(*c).unwrap()).collect();
    assert_eq!(layer.len(), 3, "both shapes and the connector: {:?}", layer.iter().map(|c| &c.id).collect::<Vec<_>>());
    let second = layer[1];
    assert_eq!(second.label.as_deref(), Some("second"));
    assert!(second.renamed && !second.id.is_empty() && second.id != "2", "the second has an id of the reader's own: {:?}", second.id);
    assert_eq!(g.by_id(&second.id).and_then(|c| g.cell(c)).map(|c| c.label.clone()), Some(Some("second".to_owned())), "and it can be found by it");
    assert_eq!(layer[2].source, Link::Cell(first), "the connector is connected to the first");
    assert_eq!(kids(g, "2"), ["9"], "and the child that names the id is the first's");
    assert_eq!(noted(&report, "drawio_duplicate_id"), (1, vec!["P: 2".to_owned()]));
    // A new id does not take one that the file has.
    let cells = "<mxCell id=\"2\" vertex=\"1\" parent=\"1\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\"/><mxCell id=\"2~2\" vertex=\"1\" parent=\"1\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\"/>";
    let (page, report) = first_page(&one_page(cells));
    sound(&page.graph);
    assert_eq!(noted(&report, "drawio_duplicate_id").0, 2);
    assert!(!cell(&page.graph, "2~2").renamed, "the file's own `2~2` is the file's");
    // No id at all is no id: nothing can name the cell, and nothing is repeated.
    let (page, report) = first_page(&one_page(
        "<mxCell vertex=\"1\" parent=\"1\"/><mxCell vertex=\"1\" parent=\"1\"/><mxCell id=\"\" vertex=\"1\" parent=\"1\"/><mxCell id=\"\" vertex=\"1\" parent=\"1\"/>",
    ));
    sound(&page.graph);
    assert_eq!(page.graph.len(), 6);
    assert!(!report.warnings.contains_key("drawio_duplicate_id"), "{:?}", report.warnings);
    assert!(page.graph.cells.iter().skip(2).all(|c| c.id.is_empty() && !c.renamed));
}

#[test]
fn r25_a_cell_is_the_root_a_layer_a_shape_a_connector_a_label_a_port_or_neither() {
    let cells = "<mxCell id=\"0\"/>\
        <mxCell id=\"L1\" parent=\"0\" vertex=\"1\"/><mxCell id=\"L2\" parent=\"0\" edge=\"1\"/>\
        <mxCell id=\"g\" vertex=\"1\" parent=\"L1\"/><mxCell id=\"c\" vertex=\"1\" parent=\"g\"/>\
        <mxCell id=\"e\" edge=\"1\" parent=\"L1\"/>\
        <mxCell id=\"lab\" vertex=\"1\" parent=\"e\"><mxGeometry x=\"0.5\" relative=\"1\" as=\"geometry\"/></mxCell>\
        <mxCell id=\"abs\" vertex=\"1\" parent=\"e\"><mxGeometry x=\"5\" y=\"5\" width=\"3\" height=\"3\" as=\"geometry\"/></mxCell>\
        <mxCell id=\"port\" vertex=\"1\" parent=\"g\"><mxGeometry x=\"1\" y=\"1\" width=\"2\" height=\"2\" relative=\"1\" as=\"geometry\"/></mxCell>\
        <mxCell id=\"n\" parent=\"L1\"/>\
        <mxCell id=\"nc\" vertex=\"1\" parent=\"n\"><mxGeometry x=\"7\" y=\"8\" width=\"1\" height=\"1\" as=\"geometry\"/></mxCell>\
        <mxCell id=\"both\" vertex=\"1\" edge=\"1\" parent=\"L1\"/>";
    let (page, report) = first_page(&raw_page(cells));
    let g = &page.graph;
    sound(g);
    let roles: Vec<(&str, Role)> =
        ["0", "L1", "L2", "g", "c", "e", "lab", "abs", "port", "n", "nc", "both"].iter().map(|id| (*id, cell(g, id).role)).collect();
    assert_eq!(
        roles,
        [
            ("0", Role::Root),
            ("L1", Role::Layer),
            ("L2", Role::Layer),
            ("g", Role::Shape),
            ("c", Role::Shape),
            ("e", Role::Connector),
            ("lab", Role::Label),
            ("abs", Role::Shape),
            ("port", Role::Port),
            ("n", Role::Neither),
            ("nc", Role::Shape),
            ("both", Role::Connector)
        ]
    );
    assert_eq!(kids(g, "0"), ["L1", "L2"], "the layers, in order");
    assert_eq!(g.layers().len(), 2);
    assert_eq!(kids(g, "g"), ["c", "port"], "a shape with child cells is a group: its members, ports among them");
    assert_eq!(kids(g, "e"), ["lab", "abs"]);
    assert_eq!(kids(g, "n"), ["nc"], "a cell that is neither vertex nor edge has its children walked");
    assert_eq!(cell(g, "nc").rect, Some(Rect { x: 7.0, y: 8.0, width: 1.0, height: 1.0 }), "from the origin of the cell with no box");
    assert_eq!(noted(&report, "drawio_cell_vertex_and_edge"), (1, vec!["P: both".to_owned()]));
    assert_eq!(
        [cell(g, "0").depth, cell(g, "L1").depth, cell(g, "g").depth, cell(g, "c").depth, cell(g, "lab").depth, cell(g, "nc").depth],
        [0, 1, 2, 3, 3, 3]
    );
    assert_eq!(cell(g, "c").parent, g.by_id("g"));
    assert_eq!(cell(g, "L1").parent, g.by_id("0"));
    assert_eq!(cell(g, "0").parent, None);
    // A layer is a layer even when its flags say vertex (51 such cells in one public file), and a shape's children are its own.
    assert!(cell(g, "L1").vertex);
}

#[test]
fn v12_a_cells_place_on_the_page_is_the_sum_of_its_own_and_its_ancestors() {
    let cells = "<mxCell id=\"g\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"100\" y=\"50\" width=\"200\" height=\"100\" as=\"geometry\"/></mxCell>\
                 <mxCell id=\"c\" vertex=\"1\" parent=\"g\"><mxGeometry x=\"20\" y=\"10\" width=\"40\" height=\"30\" as=\"geometry\"/></mxCell>\
                 <mxCell id=\"p\" vertex=\"1\" parent=\"g\"><mxGeometry x=\"1\" y=\"0.5\" width=\"10\" height=\"10\" relative=\"1\" as=\"geometry\"><mxPoint x=\"-5\" y=\"-5\" as=\"offset\"/></mxGeometry></mxCell>\
                 <mxCell id=\"e\" edge=\"1\" parent=\"g\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\"><mxPoint x=\"60\" y=\"80\"/></Array></mxGeometry></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    assert_eq!(cell(g, "g").rect, Some(Rect { x: 100.0, y: 50.0, width: 200.0, height: 100.0 }));
    assert_eq!(cell(g, "c").rect, Some(Rect { x: 120.0, y: 60.0, width: 40.0, height: 30.0 }), "not (20, 10)");
    let p = cell(g, "p");
    assert_eq!(p.role, Role::Port);
    assert_eq!(p.rect, Some(Rect { x: 295.0, y: 95.0, width: 10.0, height: 10.0 }), "not x as px");
    assert_eq!(p.rect.map(|r| r.centre()), Some(Pt { x: 300.0, y: 100.0 }), "the middle of g's right side");
    let e = cell(g, "e");
    assert_eq!(
        (e.role, e.rect, e.frame),
        (Role::Connector, None, Pt { x: 100.0, y: 50.0 }),
        "a connector has no box, and its points are measured from its parent's origin"
    );
    let waypoint = e.geometry.as_deref().unwrap().points[0];
    assert_eq!(waypoint, Pt { x: 60.0, y: 80.0 }, "as the file has it");
    assert_eq!(g.page_point(g.by_id("e").unwrap(), waypoint), Some(Pt { x: 160.0, y: 130.0 }), "not (60, 80)");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // A shape in a layer is measured from the page's own origin, and a relative geometry in a layer has no parent size to take a share of.
    let cells = "<mxCell id=\"r\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"0.5\" y=\"0.5\" width=\"10\" height=\"10\" relative=\"1\" as=\"geometry\"><mxPoint x=\"3\" y=\"4\" as=\"offset\"/></mxGeometry></mxCell>\
                 <mxCell id=\"a\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"-30\" y=\"2.5\" width=\"1\" height=\"1\" as=\"geometry\"><mxPoint x=\"99\" y=\"99\" as=\"offset\"/></mxGeometry></mxCell>\
                 <mxCell id=\"nogeo\" vertex=\"1\" parent=\"1\"/>";
    let (page, _) = first_page(&one_page(cells));
    sound(&page.graph);
    let g = &page.graph;
    assert_eq!((cell(g, "r").role, cell(g, "r").rect), (Role::Shape, Some(Rect { x: 3.0, y: 4.0, width: 10.0, height: 10.0 })));
    assert_eq!(cell(g, "a").rect, Some(Rect { x: -30.0, y: 2.5, width: 1.0, height: 1.0 }), "an offset moves the label and not the shape");
    assert_eq!(cell(g, "nogeo").rect, None, "no geometry, no box");
}

#[test]
fn v14_a_rotation_does_not_move_a_childs_box_where_the_file_says_it_is_is_where_it_is() {
    let cells = "<mxCell id=\"g\" vertex=\"1\" style=\"group;rotation=90\" parent=\"1\"><mxGeometry x=\"100\" y=\"200\" width=\"100\" height=\"100\" as=\"geometry\"/></mxCell>\
                 <mxCell id=\"c\" vertex=\"1\" style=\"rotation=90\" parent=\"g\"><mxGeometry x=\"10\" y=\"10\" width=\"20\" height=\"20\" as=\"geometry\"/></mxCell>";
    let (page, _) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    assert_eq!(
        cell(g, "c").rect,
        Some(Rect { x: 110.0, y: 210.0, width: 20.0, height: 20.0 }),
        "turned by the group a second time it would be elsewhere"
    );
    assert_eq!(cell(g, "g").rect, Some(Rect { x: 100.0, y: 200.0, width: 100.0, height: 100.0 }));
    assert!(cell(g, "c").style.contains("rotation=90"), "the rotation is in the style, for the mapping to read");
}

#[test]
fn r30_a_connectors_points_ends_offset_and_a_containers_alternate_bounds_are_read() {
    let cells = "<mxCell id=\"e\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\">\
                 <mxPoint x=\"1\" y=\"2\" as=\"sourcePoint\"/><mxPoint x=\"3\" y=\"4\" as=\"targetPoint\"/>\
                 <Array as=\"points\"><mxPoint x=\"5\" y=\"6\"/><mxPoint x=\"7\"/><mxPoint y=\"9\"/></Array><mxPoint x=\"-1\" y=\"-2\" as=\"offset\"/></mxGeometry></mxCell>\
                 <mxCell id=\"c\" vertex=\"1\" parent=\"1\" collapsed=\"1\"><mxGeometry x=\"0\" y=\"0\" width=\"100\" height=\"40\" as=\"geometry\">\
                 <mxRectangle x=\"1\" y=\"2\" width=\"300\" height=\"200\" as=\"alternateBounds\"/></mxGeometry></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    let geo = cell(g, "e").geometry.as_deref().unwrap();
    assert!(geo.relative);
    assert_eq!((geo.source_point, geo.target_point), (Some(Pt { x: 1.0, y: 2.0 }), Some(Pt { x: 3.0, y: 4.0 })));
    assert_eq!(geo.points, [Pt { x: 5.0, y: 6.0 }, Pt { x: 7.0, y: 0.0 }, Pt { x: 0.0, y: 9.0 }], "in order, a missing number 0");
    assert_eq!(geo.offset, Pt { x: -1.0, y: -2.0 });
    let c = cell(g, "c").geometry.as_deref().unwrap();
    assert_eq!(c.alternate_bounds, Some(Rect { x: 1.0, y: 2.0, width: 300.0, height: 200.0 }));
    assert!(!c.relative && c.source_point.is_none() && c.points.is_empty() && c.offset == Pt::default());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Another `mxGeometry` and an element the geometry does not know are counted and skipped, and the first geometry is the cell's.
    let cells = "<mxCell id=\"s\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"1\" width=\"2\" as=\"geometry\"><Odd/></mxGeometry><mxGeometry x=\"50\" as=\"geometry\"/></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    assert_eq!(cell(&page.graph, "s").geometry.as_deref().map(|g| (g.x, g.width)), Some((1.0, 2.0)));
    assert_eq!(noted(&report, "drawio_unknown_element"), (2, vec!["Odd".to_owned(), "mxGeometry".to_owned()]));
}

#[test]
fn v13_a_label_in_a_connector_is_a_relative_shape_whose_place_waits_for_the_connectors_path() {
    let cells = "<mxCell id=\"e\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><mxPoint x=\"0\" y=\"0\" as=\"sourcePoint\"/><mxPoint x=\"200\" y=\"0\" as=\"targetPoint\"/></mxGeometry></mxCell>\
                 <mxCell id=\"l\" vertex=\"1\" parent=\"e\"><mxGeometry x=\"-0.5\" y=\"10\" relative=\"1\" as=\"geometry\"/></mxCell>";
    let (page, _) = first_page(&one_page(cells));
    let g = &page.graph;
    sound(g);
    let l = cell(g, "l");
    assert_eq!((l.role, l.rect), (Role::Label, None), "its place is along the connector's drawn path, which the mapping works out");
    let geo = l.geometry.as_deref().unwrap();
    assert_eq!((geo.x, geo.y, geo.relative), (-0.5, 10.0, true));
    assert_eq!(geom::anchor_t(geo.x), 0.25, "a quarter along the path");
    assert_eq!(kids(g, "e"), ["l"]);
}

#[test]
fn r33_numbers_that_are_not_finite_negative_sizes_and_huge_values_are_repaired_and_counted_once_a_page() {
    let cells = "<mxCell id=\"a\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"NaN\" y=\"inf\" width=\"1e999\" height=\"-5\" as=\"geometry\"/></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    sound(&page.graph);
    assert_eq!(cell(&page.graph, "a").rect, Some(Rect::default()), "every one of the four is 0");
    assert_eq!(noted(&report, "drawio_bad_number"), (1, vec!["P: 4 numbers".to_owned()]), "one warning a page, with how many numbers it was");
    // Past 10,000,000 px the number is held there, in both signs; missing numbers are 0 and say nothing; a number can be written in any decimal form.
    let cells = "<mxCell id=\"b\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"1e9\" y=\"-1e9\" width=\"2e7\" height=\"5\" as=\"geometry\"/></mxCell>\
                 <mxCell id=\"c\" vertex=\"1\" parent=\"1\"><mxGeometry as=\"geometry\"/></mxCell>\
                 <mxCell id=\"d\" vertex=\"1\" parent=\"1\"><mxGeometry x=\" 12 \" y=\"+3\" width=\".5\" height=\"5.\" as=\"geometry\"/></mxCell>\
                 <mxCell id=\"e\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"0x10\" y=\"\" width=\"7px\" height=\"1e2\" as=\"geometry\"/></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    sound(&page.graph);
    let g = &page.graph;
    assert_eq!(cell(g, "b").rect, Some(Rect { x: 1e7, y: -1e7, width: 1e7, height: 5.0 }));
    assert_eq!(cell(g, "c").rect, Some(Rect::default()));
    assert_eq!(cell(g, "d").rect, Some(Rect { x: 12.0, y: 3.0, width: 0.5, height: 5.0 }));
    assert_eq!(cell(g, "e").rect, Some(Rect { x: 0.0, y: 0.0, width: 0.0, height: 100.0 }), "`0x10`, nothing and `7px` are no numbers");
    assert_eq!(noted(&report, "drawio_bad_number"), (1, vec!["P: 6 numbers".to_owned()]), "3 held, and 3 that are no number");
    // The cap is the limits' own, held exactly.
    let few = Limits { max_coord_px: 100.0, ..Limits::default() };
    let cells = "<mxCell id=\"a\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"100\" y=\"100.5\" as=\"geometry\"/></mxCell>";
    let (pages, _, report) = read_all(&one_page(cells), &few).unwrap();
    assert_eq!(cell(&pages[0].graph, "a").rect.map(|r| (r.x, r.y)), Some((100.0, 100.0)));
    assert_eq!(noted(&report, "drawio_bad_number").0, 1);
    // A place that is the sum of many numbers each within the cap is held to it as well.
    let chain: String = (2..8).map(|i| format!("<mxCell id=\"s{i}\" vertex=\"1\" parent=\"{}\"><mxGeometry x=\"6000000\" y=\"-6000000\" width=\"1\" height=\"1\" as=\"geometry\"/></mxCell>", if i == 2 { "1".to_owned() } else { format!("s{}", i - 1) })).collect();
    let (page, report) = first_page(&one_page(&chain));
    sound(&page.graph);
    assert_eq!(cell(&page.graph, "s7").rect.map(|r| (r.x, r.y)), Some((1e7, -1e7)));
    assert_eq!(noted(&report, "drawio_bad_number").0, 1);
    // A point is a number too.
    let cells = "<mxCell id=\"e\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\"><mxPoint x=\"nan\" y=\"5\"/><mxPoint x=\"1e30\" y=\"-1e30\"/></Array></mxGeometry></mxCell>";
    let (page, report) = first_page(&one_page(cells));
    assert_eq!(cell(&page.graph, "e").geometry.as_deref().unwrap().points, [Pt { x: 0.0, y: 5.0 }, Pt { x: 1e7, y: -1e7 }]);
    assert_eq!(noted(&report, "drawio_bad_number").0, 1);
}

#[test]
fn r33_a_connector_keeps_at_most_ten_thousand_waypoints_and_one_more_is_dropped_and_counted() {
    let points = |n: usize| (0..n).map(|i| format!("<mxPoint x=\"{i}\" y=\"1\"/>")).collect::<String>();
    let edge = |n: usize| {
        format!(
            "<mxCell id=\"e\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\">{}</Array></mxGeometry></mxCell>",
            points(n)
        )
    };
    let (page, report) = first_page(&one_page(&edge(10_000)));
    assert_eq!(cell(&page.graph, "e").geometry.as_deref().unwrap().points.len(), 10_000);
    assert!(!report.warnings.contains_key("drawio_too_many_points"));
    let (page, report) = first_page(&one_page(&edge(10_001)));
    sound(&page.graph);
    let pts = &cell(&page.graph, "e").geometry.as_deref().unwrap().points;
    assert_eq!((pts.len(), pts[9_999].x), (10_000, 9_999.0), "the first 10,000, in order");
    assert_eq!(noted(&report, "drawio_too_many_points"), (1, vec!["P: e".to_owned()]));
    // The cap is the limits' own: three, held exactly.
    let three = Limits { max_points: 3, ..Limits::default() };
    let (pages, _, report) = read_all(&one_page(&edge(3)), &three).unwrap();
    assert_eq!(cell(&pages[0].graph, "e").geometry.as_deref().unwrap().points.len(), 3);
    assert!(!report.warnings.contains_key("drawio_too_many_points"));
    let (pages, _, report) = read_all(&one_page(&edge(4)), &three).unwrap();
    assert_eq!(cell(&pages[0].graph, "e").geometry.as_deref().unwrap().points.len(), 3);
    assert_eq!(noted(&report, "drawio_too_many_points").0, 1);
}

#[test]
fn r25_the_depth_of_a_cell_is_capped_and_a_cell_past_it_is_moved_to_the_first_layer_in_document_order() {
    // The cap is the limits' own: levels below the layer, so a cap of 3 keeps three shapes nested.
    let three = Limits { max_cell_depth: 3, ..Limits::default() };
    let chain: String = (1..=7)
        .map(|i| format!("<mxCell id=\"s{i}\" vertex=\"1\" parent=\"{}\"/>", if i == 1 { "1".to_owned() } else { format!("s{}", i - 1) }))
        .collect();
    let (pages, _, report) = read_all(&one_page(&chain), &three).unwrap();
    let g = &pages[0].graph;
    sound_with(g, &three);
    assert_eq!(kids(g, "1"), ["s1", "s4", "s7"], "cut at the fourth and again at the seventh, which restarts at the layer");
    assert_eq!([cell(g, "s3").depth, cell(g, "s4").depth, cell(g, "s6").depth, cell(g, "s7").depth], [4, 2, 4, 2]);
    assert_eq!(noted(&report, "drawio_too_deep_tree"), (2, vec!["P: s4".to_owned(), "P: s7".to_owned()]));
    // A cell moved for its depth and one moved for its parent are in the order of the file.
    let one = Limits { max_cell_depth: 1, ..Limits::default() };
    let cells = "<mxCell id=\"a1\" vertex=\"1\" parent=\"1\"/><mxCell id=\"o\" vertex=\"1\"/><mxCell id=\"a2\" vertex=\"1\" parent=\"a1\"/><mxCell id=\"o2\" vertex=\"1\" parent=\"zz\"/>";
    let (pages, _, _) = read_all(&one_page(cells), &one).unwrap();
    sound_with(&pages[0].graph, &one);
    assert_eq!(kids(&pages[0].graph, "1"), ["a1", "o", "a2", "o2"]);
}

#[test]
fn v17_nesting_a_hundred_deep_is_too_deep_and_three_hundred_shapes_in_a_chain_are_cut_at_200() {
    let deep = format!("<mxfile>{}{}</mxfile>", "<a>".repeat(100), "</a>".repeat(100));
    let e = refused(deep.as_bytes());
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_deep"));
    // 300 shapes, each the parent of the next: 200 stay nested and the 201st is in the first layer, and the 100 after it hang from it. No recursion.
    let chain: String = (1..=300)
        .map(|i| format!("<mxCell id=\"s{i}\" vertex=\"1\" parent=\"{}\"/>", if i == 1 { "1".to_owned() } else { format!("s{}", i - 1) }))
        .collect();
    let (page, report) = first_page(&one_page(&chain));
    let g = &page.graph;
    sound(g);
    assert_eq!(kids(g, "1"), ["s1", "s201"]);
    assert_eq!((cell(g, "s200").depth, cell(g, "s201").depth, cell(g, "s300").depth), (201, 2, 101));
    assert_eq!(noted(&report, "drawio_too_deep_tree"), (1, vec!["P: s201".to_owned()]));
    assert!(g.cells.iter().all(|c| c.role != Role::Dropped));
}

#[test]
fn r16_the_settings_of_a_page_are_read_as_decimal_numbers_with_the_schemas_defaults() {
    let (s, report) = settings_of("");
    assert_eq!(s, file::PageSettings::default());
    assert_eq!(
        (s.page_width, s.page_height, s.page_scale, s.background.as_deref(), s.background_image_src.as_deref(), s.math, s.shadow),
        (850.0, 1100.0, 1.0, None, None, false, false)
    );
    assert!(report.warnings.is_empty());
    let (s, report) = settings_of(
        "pageWidth=\"850.5\" pageHeight=\"1169\" pageScale=\"1.5\" background=\"#ff0000\" math=\"1\" shadow=\"1\" page=\"0\" dx=\"3\" grid=\"0\" extFonts=\"x\"",
    );
    assert_eq!((s.page_width, s.page_height, s.page_scale), (850.5, 1169.0, 1.5), "a page size of 850.5 is 850.5");
    assert_eq!((s.background.as_deref(), s.math, s.shadow), (Some("#ff0000"), true, true));
    assert!(report.warnings.is_empty(), "the editor's own settings are skipped and say nothing: {:?}", report.warnings);
    let (s, _) = settings_of("background=\"none\" math=\"true\" shadow=\"0\"");
    assert_eq!((s.background.as_deref(), s.math, s.shadow), (Some("none"), false, false), "the colour as written; a flag is on only for 1");
    // A value that is no number is the default, one warning a page; a size outside 1 to 1,000,000 is held to it, and a scale outside its own range as well.
    let (s, report) = settings_of("pageWidth=\"abc\" pageHeight=\"2000000\" pageScale=\"-2\"");
    assert_eq!((s.page_width, s.page_height, s.page_scale), (850.0, 1_000_000.0, 0.001));
    assert_eq!(noted(&report, "drawio_bad_number"), (1, vec!["P: 3 numbers".to_owned()]));
    let (s, report) = settings_of("pageWidth=\"0\" pageHeight=\"NaN\" pageScale=\"x\"");
    assert_eq!((s.page_width, s.page_height, s.page_scale), (1.0, 1100.0, 1.0));
    assert_eq!(noted(&report, "drawio_bad_number").0, 1);
    let (s, _) = settings_of("pageScale=\"5000\"");
    assert_eq!(s.page_scale, 1000.0);
    // The cap on a page's size is the limits' own.
    let small = Limits { max_page_px: 2000.0, ..Limits::default() };
    let file = "<mxfile><diagram name=\"P\"><mxGraphModel pageWidth=\"2001\" pageHeight=\"2000\"><root/></mxGraphModel></diagram></mxfile>";
    let (pages, _, report) = read_all(file, &small).unwrap();
    assert_eq!((pages[0].settings.page_width, pages[0].settings.page_height), (2000.0, 2000.0));
    assert_eq!(noted(&report, "drawio_bad_number").0, 1);
}

#[test]
fn r16_the_number_warning_of_a_page_counts_the_settings_and_the_geometry_together() {
    let file = "<mxfile><diagram name=\"P\"><mxGraphModel pageWidth=\"x\"><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"?\" as=\"geometry\"/></mxCell></root></mxGraphModel></diagram>\
                <diagram name=\"Q\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"?\" as=\"geometry\"/></mxCell></root></mxGraphModel></diagram></mxfile>";
    let (_, _, report) = read_all(file, limits()).unwrap();
    assert_eq!(
        noted(&report, "drawio_bad_number"),
        (2, vec!["P: 2 numbers".to_owned(), "Q: 1 number".to_owned()]),
        "a warning for each page that has any"
    );
}

#[test]
fn r16_the_background_and_the_background_image_src_are_read_from_their_json() {
    let (s, report) = settings_of("backgroundImage=\"{&quot;src&quot;:&quot;data:page/id,b&quot;,&quot;width&quot;:1,&quot;height&quot;:2}\"");
    assert_eq!(s.background_image_src.as_deref(), Some("data:page/id,b"));
    assert!(report.warnings.is_empty());
    let (s, _) = settings_of("backgroundImage=\"{&quot;width&quot;:1}\"");
    assert_eq!(s.background_image_src, None, "an object with no `src`");
    let (s, report) = settings_of("backgroundImage=\"https://x.test/a.png\"");
    assert_eq!(s.background_image_src, None);
    assert_eq!(
        noted(&report, "drawio_attribute_unreadable"),
        (1, vec!["P: backgroundImage".to_owned()]),
        "not JSON: counted, and nothing is fetched"
    );
    let (_, report) = settings_of("backgroundImage=\"{&quot;src&quot;:3}\"");
    assert!(
        report.warnings.is_empty(),
        "a `src` that is no text is no picture and no page, and is not a fault of the file either: {:?}",
        report.warnings
    );
}

#[test]
fn r17_the_defs_of_a_model_are_kept_by_position_and_never_expanded_here() {
    let file = "<mxfile><diagram name=\"P\"><mxGraphModel><defs><def data=\"image=data:image/png,AAAA\"/><def/><def data=\"stencil(x)\"/><other/></defs><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\" style=\"image=def(0)\"/></root></mxGraphModel></diagram></mxfile>";
    let (page, report) = first_page(file);
    assert_eq!(page.defs, ["image=data:image/png,AAAA", "", "stencil(x)"], "the n-th `def` is the n-th, a missing `data` is empty");
    assert_eq!(cell(&page.graph, "2").style, "image=def(0)", "the style still names it by reference");
    assert_eq!(noted(&report, "drawio_unknown_element"), (1, vec!["other".to_owned()]));
}

#[test]
fn r5_the_files_vars_are_read_from_their_json_and_no_other_attribute_of_the_mxfile_is_kept() {
    let (vars, report) = vars_of(
        "{\"a\":\"1\",\"b\":\"two words\",\"n\":3.5e2,\"t\":true,\"f\":false,\"z\":null,\"o\":{\"x\":[1,{\"y\":2}]},\"l\":[1,2],\"e\":\"\\u00e9\\n\\\"q\\\" \\ud83d\\ude00\"}",
    );
    assert_eq!(
        vars,
        [pair("a", "1"), pair("b", "two words"), pair("n", "3.5e2"), pair("t", "true"), pair("f", "false"), pair("e", "\u{e9}\n\"q\" \u{1F600}")],
        "text, numbers as written, flags; no null, object or array"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let (vars, _) = vars_of(" { \"k\" : \"v\" , \"k\" : \"w\" , \"j\" : \"x\" } ");
    assert_eq!(vars, [pair("k", "w"), pair("j", "x")], "a name repeated has the last value, as a JSON parse has it, at its first place");
    assert_eq!(vars_of("{}").0, Vec::<(String, String)>::new());
    // The other attributes change nothing.
    let doc = "<mxfile host=\"app\" pages=\"9\" compressed=\"true\" scale=\"2\" border=\"3\" pageGroups=\"1\" agent=\"a\" modified=\"m\" etag=\"e\" version=\"24\" type=\"device\"><diagram name=\"P\"/></mxfile>";
    let (pages, mx, report) = read_all(doc, limits()).unwrap();
    assert_eq!((pages.len(), mx), (1, Mxfile::default()));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // A bare page has no file around it, and so no vars.
    let (_, mx, _) = read_all(&enc::bare_model(""), limits()).unwrap();
    assert_eq!(mx, Mxfile::default());
}

#[test]
fn r5_text_that_is_not_a_json_object_is_no_vars_and_is_counted() {
    for bad in [
        "",
        "[]",
        "\"x\"",
        "3",
        "{",
        "{\"a\"}",
        "{\"a\":}",
        "{\"a\":1,}",
        "{\"a\":1} x",
        "{a:1}",
        "{\"a\":\"\\q\"}",
        "{\"a\":\"x\ny\"}",
        "{\"a\":01}",
        "{\"a\":1.}",
        "{\"a\":[1}",
        "{\"a\":nul}",
        "{\"a\":\"\\ud800\"}",
    ] {
        let (vars, report) = vars_of(bad);
        assert!(vars.is_empty(), "{bad:?}: {vars:?}");
        assert_eq!(noted(&report, "drawio_attribute_unreadable"), (1, vec!["mxfile: vars".to_owned()]), "{bad:?}");
    }
    // Nesting past the limits' depth is not read; at it, it is.
    let shallow = Limits { max_json_depth: 4, ..Limits::default() };
    let nested = |levels: usize| format!("{{\"a\":{}1{}}}", "[".repeat(levels), "]".repeat(levels));
    let read = |json: &str| {
        let file = format!("<mxfile vars=\"{}\"><diagram name=\"P\"/></mxfile>", enc::escape_attr(json));
        read_all(&file, &shallow).unwrap().2
    };
    assert!(read(&nested(3)).warnings.is_empty(), "the object is the first level and three arrays the next");
    assert_eq!(noted(&read(&nested(4)), "drawio_attribute_unreadable").0, 1);
}

#[test]
fn r5_vars_past_the_cap_are_dropped_and_counted() {
    let few = Limits { max_vars: 3, ..Limits::default() };
    let file = |n: usize| {
        let json = format!("{{{}}}", (0..n).map(|i| format!("\"k{i}\":\"v\"")).collect::<Vec<_>>().join(","));
        format!("<mxfile vars=\"{}\"><diagram name=\"P\"/></mxfile>", enc::escape_attr(&json))
    };
    let (_, mx, report) = read_all(&file(3), &few).unwrap();
    assert_eq!(mx.vars.len(), 3);
    assert!(report.warnings.is_empty());
    let (_, mx, report) = read_all(&file(4), &few).unwrap();
    assert_eq!(mx.vars, [pair("k0", "v"), pair("k1", "v"), pair("k2", "v")]);
    assert_eq!(noted(&report, "drawio_too_many_properties"), (1, vec!["mxfile: vars".to_owned()]));
}

#[test]
fn a_model_with_no_root_or_two_roots_or_other_children_is_read_and_says_what_it_skipped() {
    let model = |inner: &str| format!("<mxfile><diagram name=\"P\"><mxGraphModel pageWidth=\"900\">{inner}</mxGraphModel></diagram></mxfile>");
    // No root: an empty page, unreadable, and its settings still read.
    let (page, report) = first_page(&model(""));
    assert!(page.graph.is_empty());
    assert_eq!(page.settings.page_width, 900.0);
    assert_eq!(noted(&report, "drawio_page_unreadable"), (1, vec!["P".to_owned()]));
    // A root with nothing in it is an empty page and says nothing.
    let (page, report) = first_page(&model("<root/>"));
    assert!(page.graph.is_empty() && page.graph.root.is_none());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // A second root, an element the model does not know, text and comments.
    let (page, report) =
        first_page(&model("text<!-- c --><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root><root><mxCell id=\"9\"/></root><Foo/>"));
    assert_eq!(page.graph.len(), 2, "the first root");
    assert_eq!(noted(&report, "drawio_unknown_element"), (2, vec!["root".to_owned(), "Foo".to_owned()]));
}

#[test]
fn a_read_of_the_cells_is_the_same_twice_and_each_cell_is_walked_by_a_few_passes_and_no_more() {
    let cells: String = (2..200).map(|i| format!("<mxCell id=\"c{i}\" value=\"v{i}\" vertex=\"1\" parent=\"{}\"><mxGeometry x=\"{i}\" y=\"1\" width=\"5\" height=\"5\" as=\"geometry\"/></mxCell>", if i % 7 == 0 { "1".to_owned() } else { format!("c{}", i - 1) })).collect();
    let file = one_page(&cells);
    let (a, _, ra) = read_all(&file, limits()).unwrap();
    let (b, _, rb) = read_all(&file, limits()).unwrap();
    assert_eq!(a, b);
    assert_eq!(ra, rb);
    sound(&a[0].graph);
    let n = a[0].graph.len() as u64;
    assert!(ra.work.cells >= n && ra.work.cells <= 6 * n, "{} cell steps for {n} cells", ra.work.cells);
    assert!(ra.work.tree_nodes > 0);
    // `read` reaches the cells: its work is the same as the walk's, and the model has its pages.
    let full = read(file.as_bytes(), limits()).unwrap();
    assert_eq!(full.report.work, ra.work);
    assert_eq!(full.report.pages, 1);
}

// ---------------------------------------------------------------------------------------------------------------------------- the small functions of `geom`

#[test]
fn parse_px_reads_a_finite_decimal_number_and_nothing_else() {
    for (text, want) in [
        ("12", 12.0),
        (" 12 ", 12.0),
        ("\t-3.5\n", -3.5),
        ("+3", 3.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("1e3", 1000.0),
        ("1E-2", 0.01),
        ("850.5", 850.5),
        ("-0", 0.0),
        ("0", 0.0),
    ] {
        assert_eq!(geom::parse_px(text, 64), Some(want), "{text:?}");
    }
    for text in [
        "", " ", "abc", "0x10", "0b1", "1_000", "inf", "-inf", "Infinity", "NaN", "nan", "1e999", "-1e999", "--1", "1.2.3", "12px", "1,5", "e5", ".",
        "+", "1e", "\u{663}", "1 2",
    ] {
        assert_eq!(geom::parse_px(text, 64), None, "{text:?}");
    }
    // Longer than the limit is no number, and the limit is held exactly.
    assert!(geom::parse_px(&"1".repeat(64), 64).is_some());
    assert_eq!(geom::parse_px(&"1".repeat(65), 64), None);
    assert_eq!(geom::parse_px("12345", 4), None);
    assert_eq!(geom::parse_px("1234", 4), Some(1234.0));
    // The white space around it counts toward the limit: it is a count of bytes of the value.
    assert_eq!(geom::parse_px("  12  ", 5), None);
}

#[test]
fn clamp_to_makes_every_float_a_length_within_the_cap() {
    for (v, max, want) in [(5.0, 10.0, 5.0), (-5.0, 10.0, -5.0), (11.0, 10.0, 10.0), (-11.0, 10.0, -10.0), (10.0, 10.0, 10.0), (0.0, 10.0, 0.0)] {
        assert_eq!(geom::clamp_to(v, max), want, "{v} within {max}");
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(geom::clamp_to(v, 10.0), 0.0, "{v}: not a finite number is 0");
    }
    assert!(geom::clamp_to(-0.0, 10.0).is_sign_positive(), "-0 is 0");
    for max in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        assert_eq!(geom::clamp_to(5.0, max), 0.0, "a cap of {max} holds nothing");
    }
    assert_eq!(geom::clamp_px(1e300), 1e7);
    assert_eq!(geom::clamp_px(-1e300), -1e7);
    assert_eq!(geom::clamp_px(123.5), 123.5);
}

#[test]
fn px_to_in_is_a_hundredth_and_finite_within_a_hundred_thousand_inches_for_any_input() {
    assert_eq!(geom::px_to_in(100.0), 1.0);
    assert_eq!(geom::px_to_in(-250.0), -2.5);
    assert_eq!(geom::px_to_in(1e9), 1e5);
    assert_eq!(geom::px_to_in(f64::NAN), 0.0);
    for v in [0.0, 1.0, -1.0, 1e7, 1e7 + 1.0, 1e300, f64::MAX, f64::MIN, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MIN_POSITIVE, 5e-324] {
        let inches = geom::px_to_in(v);
        assert!(inches.is_finite() && inches.abs() <= 1e5, "{v}: {inches}");
    }
}

#[test]
fn anchor_t_is_a_label_x_as_a_share_of_the_path_from_0_to_1() {
    for (x, want) in [
        (-1.0, 0.0),
        (0.0, 0.5),
        (1.0, 1.0),
        (-0.5, 0.25),
        (0.5, 0.75),
        (3.0, 1.0),
        (-7.0, 0.0),
        (f64::INFINITY, 1.0),
        (f64::NEG_INFINITY, 0.0),
        (f64::NAN, 0.5),
    ] {
        assert_eq!(geom::anchor_t(x), want, "{x}");
    }
}

#[test]
fn to_model_flips_y_about_the_pages_height_and_measures_from_the_pages_origin() {
    // Mapping V1: a shape at x 100, y 200, 120 by 60 px has its centre at (160, 230) px: PinX 1.6 in and PinY 11 - 2.3 = 8.7 in.
    let (x, y) = geom::to_model(Pt { x: 160.0, y: 230.0 }, Pt::default(), 11.0);
    assert!((x - 1.6).abs() < 1e-9 && (y - 8.7).abs() < 1e-9, "{x} {y}");
    // Mapping V2: the page's first tile begins at (-850, 0) px and is 22 in high: PinX 10.1 and PinY 19.7.
    let (x, y) = geom::to_model(Pt { x: 160.0, y: 230.0 }, Pt { x: -850.0, y: 0.0 }, 22.0);
    assert!((x - 10.1).abs() < 1e-9 && (y - 19.7).abs() < 1e-9, "{x} {y}");
    let (x, y) = geom::to_model(Pt { x: 0.0, y: 0.0 }, Pt { x: 0.0, y: 0.0 }, 11.0);
    assert_eq!((x, y), (0.0, 11.0), "the top-left of the page is the top of the model's page");
}

#[test]
fn page_box_places_an_absolute_geometry_from_the_frame_and_a_relative_one_from_the_parents_size() {
    let geo = |x, y, w, h, relative, offset| Geometry { x, y, width: w, height: h, relative, offset, ..Geometry::default() };
    let max = 1e7;
    let (r, held) = geom::page_box(Pt { x: 100.0, y: 50.0 }, (200.0, 100.0), &geo(20.0, 10.0, 40.0, 30.0, false, Pt { x: 9.0, y: 9.0 }), max);
    assert_eq!((r, held), (Rect { x: 120.0, y: 60.0, width: 40.0, height: 30.0 }, false), "an offset moves the label and not the box");
    let (r, held) = geom::page_box(Pt { x: 100.0, y: 50.0 }, (200.0, 100.0), &geo(1.0, 0.5, 10.0, 10.0, true, Pt { x: -5.0, y: -5.0 }), max);
    assert_eq!((r, held), (Rect { x: 295.0, y: 95.0, width: 10.0, height: 10.0 }, false));
    let (r, _) = geom::page_box(Pt { x: 100.0, y: 50.0 }, (0.0, 0.0), &geo(0.5, 0.5, 10.0, 10.0, true, Pt { x: 3.0, y: 4.0 }), max);
    assert_eq!((r.x, r.y), (103.0, 54.0), "a parent with no size has no share to give");
    // Held to the cap, and said so; and no float makes a number that is not finite.
    let (r, held) = geom::page_box(Pt { x: 9e6, y: -9e6 }, (0.0, 0.0), &geo(9e6, -9e6, 1.0, 1.0, false, Pt::default()), max);
    assert_eq!((r.x, r.y, held), (1e7, -1e7, true));
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
        let (r, _) = geom::page_box(Pt { x: bad, y: bad }, (bad, bad), &geo(bad, bad, bad, bad, true, Pt { x: bad, y: bad }), max);
        assert!([r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite() && v.abs() <= max), "{bad}: {r:?}");
    }
    assert_eq!(Rect { x: 1.0, y: 2.0, width: 4.0, height: 6.0 }.centre(), Pt { x: 3.0, y: 5.0 });
    assert_eq!(Rect { x: 1.0, y: 2.0, width: 4.0, height: 6.0 }.origin(), Pt { x: 1.0, y: 2.0 });
}

#[test]
fn r5_the_cap_of_vars_is_applied_as_the_members_come_and_a_member_that_would_not_be_kept_takes_no_room() {
    let two = Limits { max_vars: 2, ..Limits::default() };
    let vars = |json: &str, limits: &Limits| {
        let file = format!("<mxfile vars='{json}'><diagram name=\"P\"/></mxfile>");
        let (_, mx, report) = read_all(&file, limits).unwrap_or_else(|e| panic!("{e}"));
        (mx.vars, report.warnings.get("drawio_too_many_properties").map(|n| n.count))
    };
    // A null, an object and an array are not kept, so they take no room, and past the cap they are no member dropped.
    assert_eq!(vars(r#"{"a":"1","b":null,"c":"3","d":"4"}"#, &two), (vec![pair("a", "1"), pair("c", "3")], Some(1)));
    assert_eq!(vars(r#"{"a":"1","b":"2","c":null,"d":[1],"e":{"x":1}}"#, &two), (vec![pair("a", "1"), pair("b", "2")], None));
    // A name that is kept still has its last value past the cap, and a name that was dropped stays dropped.
    assert_eq!(vars(r#"{"a":"1","b":"2","c":"3","a":"9","c":"8"}"#, &two), (vec![pair("a", "9"), pair("b", "2")], Some(1)));
    // Numbers and flags are kept as written, and count as members.
    assert_eq!(vars(r#"{"n":1.50,"f":false,"t":true}"#, &two), (vec![pair("n", "1.50"), pair("f", "false")], Some(1)));
    // Nothing past the cap is read unchecked: a member that is no JSON ends the whole text.
    assert_eq!(vars(r#"{"a":"1","b":"2","c":"3","d":}"#, &two), (Vec::<(String, String)>::new(), None));
    // A name is compared as it is unescaped.
    assert_eq!(vars(r#"{"a":"1","a":"2"}"#, limits()), (vec![pair("a", "2")], None));
}

#[test]
fn r16_the_background_image_keeps_the_last_member_named_src_and_nothing_else() {
    let src = |json: &str| settings_of(&format!("backgroundImage='{json}'")).0.background_image_src;
    assert_eq!(src(r#"{"src":"a","x":1,"src":"b"}"#).as_deref(), Some("b"));
    assert_eq!(src(r#"{"src":"a","src":3}"#), None, "the last member of that name decides, and it is no text");
    assert_eq!(src(r#"{"src":"a","other":{"src":"z"},"list":[{"src":"y"}]}"#).as_deref(), Some("a"), "a member nested is not a member of the object");
    assert_eq!(src(r#"{"src":"esc"}"#).as_deref(), Some("esc"), "the name as it is unescaped");
    assert_eq!(src(r#"{"src":true}"#), None);
    assert_eq!(src(r#"{}"#), None);
}
