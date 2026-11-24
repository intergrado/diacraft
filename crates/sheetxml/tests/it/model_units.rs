//! The small pieces of the codec's surface, one by one: names of blob entries, the list of blobs, error classes and words, the names of the parts an error says,
//! and what a decode counts as unknown.
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{DocError, Document, ForeignData, Held, HeldGroup, Master, Media, Page, Passthrough, RawAttrs, Shape, ShapeList, Stamp};
use diacraft_sheetxml::model::escape::{ATTR_MAX_BYTES, base64_decode, f64_text, parse_f64};
use diacraft_sheetxml::model::{
    BlobId, BlobSink, BlobSource, Budget, CodecError, DecodeNote, MemBlobs, URN, blob_extension, blob_path, decode, decode_held, decode_master,
    decode_page, encode, encode_held, encode_master, encode_page,
};

use super::model_util::*;

#[test]
fn a_blob_is_stored_under_an_extension_from_a_fixed_list() {
    let table = [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/jpg", "jpg"),
        ("image/gif", "gif"),
        ("image/bmp", "bmp"),
        ("image/tiff", "tif"),
        ("image/webp", "webp"),
        ("image/svg+xml", "svg"),
        ("image/x-emf", "emf"),
        ("image/emf", "emf"),
        ("image/x-wmf", "wmf"),
        ("image/wmf", "wmf"),
        ("application/pdf", "pdf"),
        ("application/zip", "zip"),
        ("application/xml", "xml"),
        ("text/xml", "xml"),
        ("application/vnd.example+xml", "xml"),
        // Case and parameters do not matter, and what is not on the list is `bin`, whatever it says.
        ("IMAGE/PNG", "png"),
        ("  image/png ; charset=x", "png"),
        ("application/xmlx", "bin"),
        ("text/xmlx", "bin"),
        ("application/vnd.example+xmlx", "bin"),
        ("application/octet-stream", "bin"),
        ("", "bin"),
        ("../../etc/passwd", "bin"),
    ];
    for (media_type, extension) in table {
        assert_eq!(blob_extension(media_type), extension, "{media_type:?}");
    }
    assert_eq!(blob_path(BlobId(7), "image/png"), "blobs/7.png");
    assert_eq!(blob_path(BlobId(0), "x/y"), "blobs/0.bin");
}

#[test]
fn a_list_of_blobs_knows_its_size() {
    let mut blobs = MemBlobs::new();
    assert!(blobs.is_empty());
    assert_eq!(blobs.len(), 0);
    let id = blobs.put(&Arc::from(&b"abc"[..]), "image/png").unwrap();
    assert!(!blobs.is_empty());
    assert_eq!(blobs.len(), 1);
    // The same bytes again are the same blob.
    assert_eq!(blobs.put(&Arc::from(&b"abc"[..]), "image/png").unwrap(), id);
    assert_eq!(blobs.len(), 1);
    blobs.put(&Arc::from(&b"abd"[..]), "image/png").unwrap();
    assert_eq!(blobs.len(), 2);
    let (bytes, media_type) = blobs.entry(id).unwrap();
    assert_eq!((&bytes[..], media_type), (&b"abc"[..], "image/png"));
    assert!(blobs.entry(BlobId(9)).is_none());
    assert!(blobs.get(BlobId(9)).is_none());
    blobs.lose(id);
    assert!(blobs.get(id).is_none());
}

#[test]
fn an_error_has_a_class_and_says_what_it_is() {
    let limit = CodecError::Limit { what: "shapes", max: 5 };
    assert_eq!(limit.class(), "model_too_complex");
    assert_eq!(limit.to_string(), "the drawing holds more than 5 shapes");
    let bad = CodecError::Malformed { part: "page".to_owned(), why: "a number that is not one" };
    assert_eq!(bad.class(), "model_malformed");
    assert_eq!(bad.to_string(), "the page part is damaged: a number that is not one");
    let xml = CodecError::from(diacraft_container::xml::XmlWriteError::NotXmlChar { what: "text", ch: 1 });
    assert_eq!(xml.class(), "model_xml");
    assert!(xml.to_string().starts_with("a part cannot be written: "), "{xml}");
}

#[test]
fn a_model_error_becomes_the_codecs_error_it_is() {
    match CodecError::from(DocError::Limit { what: "colours", max: 3 }) {
        CodecError::Limit { what, max } => assert_eq!((what, max), ("colours", 3)),
        other => panic!("{other:?}"),
    }
    match CodecError::from(DocError::TooManyPages) {
        CodecError::Limit { what, max } => assert_eq!((what, max), ("pages", diacraft_doc::MAX_PAGES as u64)),
        other => panic!("{other:?}"),
    }
    match CodecError::from(DocError::BadName("a style name")) {
        CodecError::Malformed { part, why } => assert_eq!((part.as_str(), why), ("document", "a name that is empty or too long")),
        other => panic!("{other:?}"),
    }
    match CodecError::from(DocError::NoSuchSheet) {
        CodecError::Malformed { part, why } => assert_eq!((part.as_str(), why), ("document", "a model that breaks the model's own rules")),
        other => panic!("{other:?}"),
    }
}

fn part_xml(root: &str, inner: &str) -> El {
    parse_with(format!("<d:{root} xmlns:d=\"{URN}\" id=\"1\">{inner}</d:{root}>").as_bytes(), &native_limits()).unwrap()
}

fn open() -> Budget {
    Budget::for_open(&Limits::default())
}

const HUGE: u64 = 1 << 40;

fn big() -> Budget {
    Budget::new(HUGE, HUGE, HUGE)
}

#[test]
fn an_error_names_the_part_it_is_in() {
    // A damaged part is the number of a shape that is not one.
    let bad_shape = "<d:shape id=\"not-a-number\"/>";
    let page = decode_page(&part_xml("page", bad_shape), &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&page, CodecError::Malformed { part, .. } if part == "page"), "{page:?}");
    let master = decode_master(&part_xml("master", bad_shape), &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&master, CodecError::Malformed { part, .. } if part == "master"), "{master:?}");
    // A document part, with a page it lists whose part is damaged: each part says its own name.
    let document = parse_with(
        format!("<d:document xmlns:d=\"{URN}\"><d:page-ref id=\"1\" part=\"pages/page1.xml\"/></d:document>").as_bytes(),
        &native_limits(),
    )
    .unwrap();
    let damaged = part_xml("page", bad_shape);
    let err = decode(&document, &[&damaged], &[], &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&err, CodecError::Malformed { part, .. } if part == "page"), "{err:?}");
    // And the one after it is the document's again.
    let bad_title = parse_with(format!("<d:document xmlns:d=\"{URN}\" version=\"x\"/>").as_bytes(), &native_limits()).unwrap();
    let err = decode(&bad_title, &[], &[], &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&err, CodecError::Malformed { part, .. } if part == "document"), "{err:?}");
}

#[test]
fn an_element_no_one_keeps_is_counted_and_charged() {
    // A `cp` has no native list for what a newer build wrote on it: an element in it is counted as unknown, and charged as one item with its attributes.
    let page = part_xml("page", "<d:shape id=\"1\"><d:text><d:cp ix=\"1\"><d:future a=\"1\" b=\"2\"/></d:cp></d:text></d:shape>");
    let mut notes = Vec::new();
    let mut budget = big();
    decode_page(&page, &MemBlobs::new(), &mut budget, &mut notes).unwrap();
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    let with = HUGE - budget.items;
    let mut plain = big();
    let page = part_xml("page", "<d:shape id=\"1\"><d:text><d:cp ix=\"1\"/></d:text></d:shape>");
    decode_page(&page, &MemBlobs::new(), &mut plain, &mut Vec::new()).unwrap();
    assert_eq!(with - (HUGE - plain.items), 4, "the element and its two attributes, and the note");
}

fn page_in(inner: &str) -> El {
    part_xml("page", inner)
}

#[test]
fn a_flag_is_read_as_one_zero_true_or_false_and_nothing_else() {
    let dels = |value: &str| {
        let page = page_in(&format!("<d:shape id=\"1\" del=\"{value}\"/>"));
        decode_page(&page, &MemBlobs::new(), &mut open(), &mut Vec::new()).map(|p| p.shapes.get(0).map(|s| s.del))
    };
    for (written, read) in [("1", true), ("true", true), ("0", false), ("false", false)] {
        assert_eq!(dels(written).unwrap(), Some(read), "{written}");
    }
    for bad in ["2", "", "yes", "TRUE", "-1"] {
        assert!(matches!(dels(bad), Err(CodecError::Malformed { .. })), "{bad:?}");
    }
}

#[test]
fn an_unknown_element_takes_the_declarations_its_names_use_and_no_others() {
    let xml = format!(
        "<d:page xmlns:d=\"{URN}\" xmlns:x=\"urn:x\" xmlns:unused=\"urn:u\" id=\"1\"><d:shape id=\"1\"><d:future x:a=\"1\" xml:lang=\"en\" plain=\"p\"/></d:shape></d:page>"
    );
    let page = decode_page(&parse_with(xml.as_bytes(), &native_limits()).unwrap(), &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap();
    let native = &page.shapes.get(0).unwrap().ext.get().unwrap().native;
    let Some(diacraft_doc::RawNode::Element(e)) = native.kept.iter().next().map(|p| &p.node) else { panic!("an unknown element") };
    let attrs: Vec<(&str, &str)> = e.attrs.iter().collect();
    // The prefixes its name and its attributes use, in order, then its own attributes as written: not the one nothing uses, and not `xml`.
    assert_eq!(attrs, [("xmlns:d", URN), ("xmlns:x", "urn:x"), ("x:a", "1"), ("xml:lang", "en"), ("plain", "p")]);
}

#[test]
fn a_solution_keeps_its_first_element_and_counts_the_others_as_unknown() {
    let xml = format!("<d:document xmlns:d=\"{URN}\"><d:solution><d:el name=\"a\"/><d:el name=\"b\"/></d:solution></d:document>");
    let mut notes = Vec::new();
    let doc = decode(&parse_with(xml.as_bytes(), &native_limits()).unwrap(), &[], &[], &MemBlobs::new(), &mut open(), &mut notes).unwrap();
    assert_eq!(&*doc.diacraft.as_ref().unwrap().name, "a");
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
}

#[test]
fn a_long_field_inside_a_text_is_no_part_of_it() {
    let page = page_in("<d:shape id=\"1\"><d:text>a<d:s n=\"x\">v</d:s>b</d:text></d:shape>");
    let mut notes = Vec::new();
    let page = decode_page(&page, &MemBlobs::new(), &mut open(), &mut notes).unwrap();
    let parts = &page.shapes.get(0).unwrap().text.as_ref().unwrap().parts;
    assert!(parts.iter().all(|p| matches!(p, diacraft_doc::TextPart::Chars(_))), "{parts:?}");
    // It is read as a field of the text, which has none of that name: one unknown thing, as a field no build knows is.
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
}

fn page_with_formula(text: &str) -> El {
    page_in(&format!("<d:shape id=\"1\"><d:sheet><d:Cell N=\"A\" F=\"{text}\"/></d:sheet></d:shape>"))
}

#[test]
fn a_formula_the_budget_can_afford_to_the_last_node_is_read() {
    // `1+1+1+1+1` is nine nodes of 128 bytes, and its text is nine bytes. What the whole page costs is what it is charged with a roomy budget; a budget of exactly
    // that reads it and is spent, and one byte fewer refuses. And the price of a node is held: the same page with the formula `1` (one node, one byte) costs 8 nodes
    // and 8 bytes of text less.
    let spent = |text: &str| {
        let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
        decode_page(&page_with_formula(text), &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let exact_bytes = spent("1+1+1+1+1");
    // The text's bytes at nine eighths (text is charged at nine eighths of its length, rounded up).
    let text = diacraft_sheetxml::model::cost::text_bytes;
    assert_eq!(exact_bytes - spent("1"), 8 * 128 + text(9) - text(1));
    let page = page_with_formula("1+1+1+1+1");
    let mut exact = Budget::new(HUGE, exact_bytes, HUGE).without_sharing();
    let out = decode_page(&page, &MemBlobs::new(), &mut exact, &mut Vec::new()).unwrap();
    assert!(out.shapes.get(0).unwrap().sheet.cells.contains_key("A"));
    assert_eq!(exact.bytes, 0, "spent to the last byte");
    let mut short = Budget::new(HUGE, exact_bytes - 1, HUGE);
    assert!(matches!(decode_page(&page, &MemBlobs::new(), &mut short, &mut Vec::new()), Err(CodecError::Limit { .. })));
}

#[test]
fn a_formula_past_the_formula_crates_own_cap_is_kept_as_text_and_a_tight_budget_refuses_it() {
    // A flat call of one more argument than the crate allows nodes: the crate refuses it as too many nodes, and not as too deep. A budget that could pay for
    // more than the crate allows reads it as text with no tree (the crate's cap is the crate's); one that could not is the budget's refusal.
    let text = format!("SUM({}1)", "1,".repeat(diacraft_formula::MAX_NODES));
    let page = page_with_formula(&text);
    let mut roomy = Budget::new(HUGE, HUGE, HUGE);
    let out = decode_page(&page, &MemBlobs::new(), &mut roomy, &mut Vec::new());
    assert!(out.is_ok(), "{:?}", out.err());
    let mut tight = Budget::new(HUGE, (diacraft_formula::MAX_NODES as u64 - 1) * 128, HUGE);
    assert!(matches!(decode_page(&page, &MemBlobs::new(), &mut tight, &mut Vec::new()), Err(CodecError::Limit { .. })));
}

#[test]
fn a_formula_the_budget_cannot_afford_is_not_parsed_past_what_it_could_pay_for() {
    // 100,000 nodes against a budget of 80: refused after about 80 nodes, so far faster than the parse of the whole formula.
    let text = format!("SUM({}1)", "1,".repeat(100_000));
    let best = |f: &dyn Fn()| {
        (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                f();
                start.elapsed().as_secs_f64()
            })
            .fold(f64::MAX, f64::min)
    };
    let whole = best(&|| {
        let _ = diacraft_formula::parse_with_budget(&text, usize::MAX);
    });
    let page = page_with_formula(&text);
    let refused = best(&|| {
        let mut budget = Budget::new(HUGE, 80 * 128, HUGE);
        assert!(decode_page(&page, &MemBlobs::new(), &mut budget, &mut Vec::new()).is_err());
    });
    assert!(refused * 5.0 < whole, "refusing took {refused:.6} s against {whole:.6} s for the whole parse");
}

fn lists_of(inner: &str) -> Result<diacraft_sheetxml::model::PartLists, CodecError> {
    let xml = format!("<d:document xmlns:d=\"{URN}\" xmlns:x=\"urn:other\">{inner}</d:document>");
    diacraft_sheetxml::model::part_lists(&parse_with(xml.as_bytes(), &native_limits()).unwrap())
}

#[test]
fn the_lists_of_a_document_are_read_from_attributes_and_long_fields() {
    let lists = lists_of(
        "<d:page-ref id=\"4\" part=\"pages/page4.xml\"/><d:master-ref id=\"5\" part=\"masters/master5.xml\"/>\
         <d:blob id=\"1\" path=\"blobs/1.png\" media-type=\"image/png\" size=\"3\" stamp=\"0123456789abcdef\"/>\
         <d:blob id=\"2\" size=\"4\" stamp=\"0123456789abcdef\">\
            <d:s n=\"path\">blobs/long.png</d:s>\
            <x:s n=\"path\">wrong</x:s>\
            <d:other n=\"path\">wrong</d:other>\
            <d:s n=\"media-type\" b64=\"true\">aW1hZ2UvcG5n</d:s>\
         </d:blob>\
         <d:blob id=\"3\" size=\"1\" stamp=\"0123456789abcdef\"><d:s n=\"path\" b64=\"0\">aGk=</d:s></d:blob>\
         <d:blob id=\"6\" size=\"1\" stamp=\"0123456789abcdef\"><d:s n=\"path\" b64=\"1\">aGk=</d:s></d:blob>",
    )
    .unwrap();
    assert_eq!(lists.pages, [(4, "pages/page4.xml".to_owned())]);
    assert_eq!(lists.masters, [(5, "masters/master5.xml".to_owned())]);
    let blobs: Vec<(u32, &str, &str, u64)> = lists.blobs.iter().map(|b| (b.id.0, b.path.as_str(), b.media_type.as_str(), b.size)).collect();
    assert_eq!(blobs, [(1, "blobs/1.png", "image/png", 3), (2, "blobs/long.png", "image/png", 4), (3, "aGk=", "", 1), (6, "hi", "", 1),]);
}

#[test]
fn what_a_blob_record_says_in_base64_and_what_is_a_declaration_is_told_apart() {
    // `xmlns:q-b64` is a declaration whatever its name ends with: it is not text in base64, and is not read as one.
    let lists = lists_of("<d:blob id=\"1\" size=\"1\" stamp=\"0123456789abcdef\" xmlns:q-b64=\"!!\" xmlns:r=\"urn:r\" path-b64=\"aGk=\"/>").unwrap();
    assert_eq!(lists.blobs[0].path, "hi");
    // A blob with a field in base64 that is not base64 is damage.
    assert!(matches!(lists_of("<d:blob id=\"1\" size=\"1\" stamp=\"0123456789abcdef\" path-b64=\"!!\"/>"), Err(CodecError::Malformed { .. })));
    assert!(matches!(
        lists_of("<d:blob id=\"1\" size=\"1\" stamp=\"0123456789abcdef\"><d:s n=\"path\" b64=\"1\">!!</d:s></d:blob>"),
        Err(CodecError::Malformed { .. })
    ));
    // Two records of one number, and one with no stamp, are damage.
    let one = "<d:blob id=\"1\" size=\"1\" stamp=\"0123456789abcdef\"/>";
    assert!(matches!(lists_of(&format!("{one}{one}")), Err(CodecError::Malformed { .. })));
    assert!(matches!(lists_of("<d:blob id=\"1\" size=\"1\"/>"), Err(CodecError::Malformed { .. })));
}

#[test]
fn f64_text_is_plain_digits_from_1e_minus_6_up_to_below_1e16_and_exponent_text_outside() {
    let table = [
        (0.0, "0"),
        (-0.0, "-0"),
        (1.0, "1"),
        (-2.5, "-2.5"),
        (0.1, "0.1"),
        (1e15, "1000000000000000"),
        (9.999e15, "9999000000000000"),
        (1e16, "1e16"),
        (-1e16, "-1e16"),
        (1e300, "1e300"),
        (1e-6, "0.000001"),
        (1.5e-6, "0.0000015"),
        (9.99e-7, "9.99e-7"),
        (-9.99e-7, "-9.99e-7"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e308"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "INF"),
        (f64::NEG_INFINITY, "-INF"),
    ];
    for (x, text) in table {
        assert_eq!(f64_text(x), text, "{x:e}");
        if x.is_finite() {
            assert_eq!(parse_f64(text).map(f64::to_bits), Some(x.to_bits()), "{text}");
        }
    }
    assert!(parse_f64("NaN").is_some_and(f64::is_nan));
    assert_eq!(parse_f64("INF"), Some(f64::INFINITY));
    assert_eq!(parse_f64("-INF"), Some(f64::NEG_INFINITY));
    // What no spelling of Rust's own reading may be taken for.
    for bad in ["", "inf", "nan", "Infinity", "0x10", "1_0", " 1", "1 ", "1e", "e5", "--1", "+INF", "1e999", "-1e999"] {
        assert_eq!(parse_f64(bad), None, "{bad:?}");
    }
}

#[test]
fn padding_in_the_middle_of_base64_is_no_base64() {
    assert_eq!(base64_decode("Zg==Zg=="), None);
    assert_eq!(base64_decode("Zm8=Zm8="), None);
    assert_eq!(base64_decode("Zm8=Zg=="), None);
    assert_eq!(base64_decode("Zm9vZg==").as_deref(), Some(&b"foof"[..]));
}

fn shape_with_data(data2: &str, data3: &str) -> Page {
    let mut s = Shape::new(1);
    s.data2 = Some(Arc::from(data2));
    s.data3 = Some(Arc::from(data3));
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(s)]);
    page
}

#[test]
fn a_text_is_an_attribute_up_to_the_cap_and_an_element_of_its_own_past_it() {
    // A value of two thousand bytes and one of exactly the cap are attributes; one more byte is a child.
    let page = shape_with_data(&"x".repeat(2_000), &"y".repeat(ATTR_MAX_BYTES));
    let el = encode_page(&page, &mut MemBlobs::new()).unwrap();
    let shape = el.elements().next().unwrap();
    assert_eq!(shape.attr("data2").map(str::len), Some(2_000));
    assert_eq!(shape.attr("data3").map(str::len), Some(ATTR_MAX_BYTES));
    assert!(shape.elements().all(|e| e.local() != "s"));
    let page = shape_with_data(&"x".repeat(2_000), &"y".repeat(ATTR_MAX_BYTES + 1));
    let el = encode_page(&page, &mut MemBlobs::new()).unwrap();
    let shape = el.elements().next().unwrap();
    assert_eq!(shape.attr("data3"), None);
    let long: Vec<&El> = shape.elements().filter(|e| e.local() == "s").collect();
    assert_eq!(long.len(), 1);
    assert_eq!(long[0].attr("n"), Some("data3"));
}

#[test]
fn blobs_are_listed_in_the_order_of_their_numbers() {
    let mut d = Document::empty();
    let mut page = Page::new(1, "P");
    let mut shapes = Vec::new();
    for (i, text) in ["one", "two", "three", "four"].iter().enumerate() {
        let bytes: Arc<[u8]> = Arc::from(text.as_bytes());
        let mut s = Shape::new(i as u32 + 1);
        s.foreign = Some(Arc::new(ForeignData {
            picture: Some(Arc::new(Media { part: Arc::from("/m.png"), content_type: Arc::from("image/png"), bytes })),
            ..ForeignData::default()
        }));
        shapes.push(Arc::new(s));
    }
    page.shapes = ShapeList::from_vec(shapes);
    d.pages.push(Arc::new(page));
    let el = encode(&d, &mut MemBlobs::new()).unwrap().document;
    let listed: Vec<&str> = el.elements().filter(|e| e.local() == "blob").map(|e| e.attr("id").unwrap_or("0")).collect();
    assert_eq!(listed, ["0", "1", "2", "3"]);
}

#[test]
fn a_master_comes_back_through_its_own_pair_of_functions() {
    let mut s = Shape::new(1);
    s.name = Some(Arc::from("inside"));
    let m = Master { id: 7, name: Some(Arc::from("M")), shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() };
    let el = encode_master(&m, &mut MemBlobs::new()).unwrap();
    assert_eq!(el.local(), "master");
    assert_eq!(el.attr("id"), Some("7"));
    let back = decode_master(&el, &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap();
    assert!(back == m);
}

#[test]
fn a_held_value_comes_back_through_its_own_pair_of_functions_and_what_its_record_kept_has_no_owner_to_go_to() {
    let held =
        Held { group: HeldGroup::Line, stamp: Some(Stamp(3)), attrs: RawAttrs::from_pairs([("draw:stroke", "dash")]), kept: Passthrough::new() };
    let el = encode_held(&held, &mut MemBlobs::new()).unwrap();
    assert_eq!(el.name, "diacraft:held");
    assert_eq!(el.attr("xmlns:diacraft"), Some(URN));
    assert!(decode_held(&el, &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap() == held);
    // No stamp is no stamp.
    let unstamped = Held { stamp: None, ..held.clone() };
    let el = encode_held(&unstamped, &mut MemBlobs::new()).unwrap();
    assert_eq!(el.attr("stamp"), None);
    assert!(decode_held(&el, &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap() == unstamped);
    // An attribute of the record that no build here knows is counted.
    let mut later = el.clone();
    later.attrs.push(("later".to_owned(), "1".to_owned()));
    let mut notes = Vec::new();
    assert!(decode_held(&later, &MemBlobs::new(), &mut open(), &mut notes).unwrap() == unstamped);
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
}

#[test]
fn a_damaged_kept_text_names_the_part_it_is_in() {
    let bad = "<d:kept><d:txt b64=\"1\">!!</d:txt></d:kept>";
    let page =
        decode_page(&part_xml("page", &format!("<d:shape id=\"1\">{bad}</d:shape>")), &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&page, CodecError::Malformed { part, why } if part == "page" && why.contains("base64")), "{page:?}");
    let master = decode_master(&part_xml("master", &format!("<d:shape id=\"1\">{bad}</d:shape>")), &MemBlobs::new(), &mut open(), &mut Vec::new())
        .unwrap_err();
    assert!(matches!(&master, CodecError::Malformed { part, .. } if part == "master"), "{master:?}");
    let document = parse_with(format!("<d:document xmlns:d=\"{URN}\">{bad}</d:document>").as_bytes(), &native_limits()).unwrap();
    let err = decode(&document, &[], &[], &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap_err();
    assert!(matches!(&err, CodecError::Malformed { part, .. } if part == "document"), "{err:?}");
}

#[test]
fn a_lost_part_takes_back_only_its_own_unknown_content() {
    // A colour with something unknown comes before the part whose bytes are lost: the colour keeps what it had.
    let xml = format!(
        "<d:document xmlns:d=\"{URN}\" version=\"4\"><d:color ix=\"3\" rgb=\"#000001\" mine=\"1\"/>\
         <d:part format=\"vsdx\" name=\"/a\" media-type=\"x\" role=\"other\" blob=\"9\" lost=\"1\"><d:rel source=\"/a\" id=\"r\" kind=\"k\" target=\"t\" relx=\"1\"/></d:part>\
         <d:color ix=\"4\" rgb=\"#000002\" after=\"2\"/></d:document>"
    );
    let document = parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let doc = decode(&document, &[], &[], &MemBlobs::new(), &mut open(), &mut Vec::new()).unwrap();
    let kept: Vec<String> = doc
        .ext
        .get()
        .map(|d| {
            d.native
                .kept
                .iter()
                .filter_map(|p| if let diacraft_doc::RawNode::Element(e) = &p.node { Some(e.name.to_string()) } else { None })
                .collect()
        })
        .unwrap_or_default();
    // The lost part's own entries are gone, the colours' on either side of it are not.
    assert_eq!(kept, ["color[3]", "color[4]"]);
}

/// A document part holding `inner`, decoded with a budget that is never short.
fn doc_from(inner: &str) -> Result<(Document, Vec<DecodeNote>), CodecError> {
    let xml = format!("<d:document xmlns:d=\"{URN}\" version=\"4\">{inner}</d:document>");
    let el = parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let mut notes = Vec::new();
    decode(&el, &[], &[], &MemBlobs::new(), &mut big(), &mut notes).map(|doc| (doc, notes))
}

#[test]
fn a_wrapper_keeps_its_first_el_and_what_else_is_in_it_is_not_taken_for_one() {
    let odd = "<d:future/><d:el name=\"a\"/><d:el name=\"b\"/>";
    let (doc, _) = doc_from(&format!(
        "<d:ext><d:odf><d:settings>{odd}</d:settings>\
         <d:definition family=\"marker\" name=\"m\">{odd}</d:definition>\
         <d:kept-style family=\"paragraph\" name=\"p\">{odd}</d:kept-style></d:odf></d:ext>"
    ))
    .unwrap();
    let odf = &doc.ext.get().unwrap().odf;
    assert_eq!(&*odf.settings.as_ref().unwrap().name, "a");
    assert_eq!(&*odf.definitions[0].element.name, "a");
    assert_eq!(&*odf.styles[0].element.name, "a");
}

#[test]
fn a_kept_wrapper_holding_an_element_this_build_does_not_know_keeps_nothing_of_it() {
    let page = part_xml("page", "<d:shape id=\"1\"><d:kept after=\"0\"><d:future/></d:kept></d:shape>");
    let mut notes = Vec::new();
    let page = decode_page(&page, &MemBlobs::new(), &mut open(), &mut notes).unwrap();
    assert!(page.shapes.get(0).unwrap().passthrough.is_empty());
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
}

#[test]
fn definitions_of_all_families_are_read_up_to_six_times_the_per_family_cap_and_no_more() {
    let per_family = diacraft_doc::MAX_DEFINITIONS;
    let mut inner = String::from("<d:ext><d:odf>");
    for family in ["marker", "gradient", "opacity", "hatch", "stroke-dash", "fill-image"] {
        for i in 0..per_family {
            inner.push_str(&format!("<d:definition family=\"{family}\" name=\"n{i}\"><d:el name=\"a\"/></d:definition>"));
        }
    }
    let (doc, _) = doc_from(&format!("{inner}</d:odf></d:ext>")).unwrap();
    assert_eq!(doc.ext.get().unwrap().odf.definitions.len(), 6 * per_family);
    inner.push_str("<d:definition family=\"marker\" name=\"extra\"><d:el name=\"a\"/></d:definition>");
    let err = doc_from(&format!("{inner}</d:odf></d:ext>")).err().unwrap();
    // The one more is a marker's, and the cap it passes is the marker's: each family's is counted as its definitions are read.
    assert!(matches!(err, CodecError::Limit { what: "marker definitions", max } if max == per_family as u64), "{err:?}");
}
