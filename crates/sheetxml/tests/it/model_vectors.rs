//! One hand vector for each rule of the codec (the design's 2.3), and for each choice of the vocabulary. Every one goes through the real XML reader at the
//! native limits.
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, Node, parse_with};
use diacraft_doc::{
    Cell, CellExtra, CellName, Connect, Connects, Document, ForeignData, Master, Media, Page, Passthrough, RawAttrs, RawElement, RawNode, Row,
    Section, Shape, ShapeList, ShapeType, Sheet, Style, Text, TextPart, Theme, Value,
};
use diacraft_sheetxml::model::escape::{ATTR_MAX_BYTES, base64_decode, base64_encode, f64_text, parse_f64};
use diacraft_sheetxml::model::{
    BlobId, BlobSink, BlobSource, Budget, CodecError, DecodeNote, Digest, MemBlobs, content_stamp, decode, decode_shape, encode, encode_shape,
    part_lists,
};

use super::model_diff::first_difference;
use super::model_util::*;

fn assert_round_trip(d: &Document) {
    let back = round_trip(d);
    assert!(back == *d, "{}", first_difference(&back, d).unwrap_or_default());
}

fn page_of(shapes: Vec<Shape>) -> Document {
    let mut d = Document::empty();
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
    d.pages.push(Arc::new(p));
    d
}

fn shape_with_text(parts: Vec<TextPart>) -> Shape {
    Shape { id: 1, text: Some(Arc::new(Text { parts, ..Text::default() })), ..Shape::default() }
}

fn shape_with_cell(name: &str, cell: Cell) -> Shape {
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new(name), cell);
    Shape { id: 1, sheet: Arc::new(sheet), ..Shape::default() }
}

/// The bytes of every part of `d`, as one text, for the tests of what is and is not written.
fn all_text(d: &Document) -> String {
    let p = pack(d);
    p.all_bytes().iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<Vec<_>>().join("\n")
}

/// The first child element of `e` whose local name is `local`, to change.
fn element_mut<'a>(e: &'a mut El, local: &str) -> &'a mut El {
    e.children.iter_mut().find_map(|n| if let Node::El(c) = n { if c.local() == local { Some(c) } else { None } } else { None }).unwrap()
}

fn parsed(bytes: &[u8]) -> El {
    parse_with(bytes, &native_limits()).unwrap()
}

#[test]
fn the_vocabulary_table_is_consistent() {
    use diacraft_sheetxml::model::vocab::{ELEMENTS, Kind, attribute, element};
    let mut seen = std::collections::HashSet::new();
    for e in ELEMENTS {
        assert!(seen.insert(e.name), "{} is in the table twice", e.name);
        // Every place an element stands is the root of a part or an element the table has.
        for parent in e.inside {
            assert!(*parent == "part root" || element(parent).is_some(), "{} stands in {parent}, which the table lacks", e.name);
        }
        let mut names = std::collections::HashSet::new();
        for a in e.attrs {
            assert!(names.insert(a.name), "{}@{} is in the table twice", e.name, a.name);
            assert!(attribute(e.name, a.name).is_some());
            // A default is a value of the attribute's kind.
            match (a.kind, a.default) {
                (Kind::U32 | Kind::Blob, Some(d)) => assert!(d.parse::<u32>().is_ok(), "{}@{}", e.name, a.name),
                (Kind::U64, Some(d)) => assert!(d.parse::<u64>().is_ok(), "{}@{}", e.name, a.name),
                (Kind::Flag, d) => assert_eq!(d, Some("0"), "{}@{}: a flag's default is off", e.name, a.name),
                (Kind::Stamp, d) => assert_eq!(d, None, "{}@{}: a stamp has no default", e.name, a.name),
                _ => {}
            }
        }
    }
    // The three roots of a part, and only they, stand at the root.
    let roots: Vec<&str> = ELEMENTS.iter().filter(|e| e.inside == ["part root"]).map(|e| e.name).collect();
    assert_eq!(roots, ["document", "page", "master"]);
    // A `-b64` suffix names the text attribute it stands for, and nothing else.
    assert!(attribute("shape", "name-b64").is_some() && attribute("shape", "id-b64").is_none() && attribute("shape", "del-b64").is_none());
}

#[test]
fn digest_is_pinned() {
    assert_eq!(Digest::new("cells").text("LineWeight").finish().text(), "7bce65b857cfdb09");
}

#[test]
fn a_blank_document_comes_back() {
    assert_round_trip(&Document::new_blank());
}

#[test]
fn forbidden_characters_round_trip() {
    let bad = "\u{1}\u{1f}\u{fffe}\u{ffff}";
    let mut s = shape_with_text(vec![
        TextPart::Chars(Arc::from(bad)),
        TextPart::Fld { ix: 2, text: Arc::from(bad) },
        TextPart::Raw(RawNode::Comment(Arc::from(bad))),
    ]);
    s.raw = RawAttrs::from_pairs([(bad, bad)]);
    s.passthrough =
        Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 1, node: RawNode::Pi { target: Arc::from(bad), data: Arc::from(bad) } }]);
    s.name = Some(Arc::from(bad));
    let mut d = page_of(vec![s]);
    d.title = Arc::from(bad);
    assert_round_trip(&d);
    let p = pack(&d);
    for bytes in p.all_bytes() {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!text.chars().any(|c| matches!(c, '\u{1}' | '\u{1f}' | '\u{fffe}' | '\u{ffff}')), "a forbidden character is written as it is");
    }
}

#[test]
fn cr_tab_lf_in_attribute_values_survive() {
    let value = "a\tb\nc\rd";
    let mut s = Shape::new(1);
    s.name = Some(Arc::from(value));
    s.raw = RawAttrs::from_pairs([("k", value)]);
    let d = page_of(vec![s]);
    assert_round_trip(&d);
    let text = all_text(&d);
    for spelling in ["&#9;", "&#10;", "&#13;"] {
        assert!(text.contains(spelling), "{spelling} is written");
    }
}

#[test]
fn text_keeps_every_blank() {
    let d = page_of(vec![shape_with_text(vec![
        TextPart::Chars(Arc::from("  a  \n  ")),
        TextPart::Cp(0),
        TextPart::Chars(Arc::from("\r\n ")),
        TextPart::Chars(Arc::from(" ")),
    ])]);
    assert_round_trip(&d);
}

#[test]
fn runs_of_characters_that_would_merge_or_vanish_keep_their_edges() {
    let parts = vec![
        TextPart::Chars(Arc::from("a")),
        TextPart::Chars(Arc::from("b")),
        TextPart::Chars(Arc::from("")),
        TextPart::Chars(Arc::from("")),
        TextPart::Cp(1),
        TextPart::Chars(Arc::from("")),
        TextPart::Fld { ix: 0, text: Arc::from("") },
        TextPart::Chars(Arc::from("c")),
    ];
    assert_round_trip(&page_of(vec![shape_with_text(parts)]));
    // An empty text and an absent one are two things.
    assert_round_trip(&page_of(vec![shape_with_text(Vec::new()), Shape::new(2)]));
}

#[test]
fn a_kept_attribute_named_twice_is_kept_twice_in_order() {
    let mut s = Shape::new(1);
    s.raw = RawAttrs::from_pairs([("x", "1"), ("y", "0"), ("x", "2")]);
    let back = round_trip(&page_of(vec![s]));
    let pairs: Vec<(&str, &str)> = back.pages.get(0).unwrap().shapes.get(0).unwrap().raw.iter().collect();
    assert_eq!(pairs, [("x", "1"), ("y", "0"), ("x", "2")]);
}

#[test]
fn a_typed_and_a_kept_attribute_of_one_name_both_come_back() {
    let mut s = Shape::new(7);
    s.raw = RawAttrs::from_pairs([("ID", "007")]);
    let back = round_trip(&page_of(vec![s.clone()]));
    let got = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!((got.id, got.raw.get("ID")), (7, Some("007")));
    // A cell's own attributes too: `V` typed, `V` kept.
    let mut c = Cell::constant(Value::from_text("1"));
    c.raw = RawAttrs::from_pairs([("V", "01"), ("N", "x")]);
    assert_round_trip(&page_of(vec![shape_with_cell("PinX", c)]));
}

#[test]
fn some_empty_is_not_none() {
    let mut a = Shape::new(1);
    a.name = Some(Arc::from(""));
    a.data1 = Some(Arc::from(""));
    let mut b = Shape::new(2);
    b.name = None;
    b.data1 = None;
    let mut c = Cell::constant(Value::from_text(""));
    c.unit = Some(Arc::from(""));
    c.error = Some(Arc::from(""));
    let mut sheet_shape = shape_with_cell("Empty", c);
    sheet_shape.foreign = Some(Arc::new(ForeignData { text: Some(Arc::from("")), rel_id: Some(Arc::from("")), ..ForeignData::default() }));
    let back = round_trip(&page_of(vec![a.clone(), b.clone(), sheet_shape.clone(), Shape::new(4)]));
    let shapes = &back.pages.get(0).unwrap().shapes;
    assert_eq!(shapes.get(0).unwrap().name.as_deref(), Some(""));
    assert_eq!(shapes.get(1).unwrap().name, None);
    assert_eq!(**shapes.get(2).unwrap(), sheet_shape);
    assert_eq!(shapes.get(3).unwrap().foreign, None);
}

#[test]
fn an_empty_extra_is_not_an_absent_one_and_a_marker_formula_is_no_marker() {
    let mut with = Cell::constant(Value::from_text("1"));
    with.extra = Some(Arc::new(CellExtra::default()));
    let without = Cell::constant(Value::from_text("1"));
    let mut text = Cell::default();
    text.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("Some text")), ..CellExtra::default() }));
    let mut sheet = Sheet::default();
    for (n, c) in [
        ("A", with),
        ("B", without),
        ("C", text),
        ("D", Cell::with_formula("Inh", None)),
        ("E", Cell::with_formula("No Formula", None)),
        ("F", Cell::inherited(None)),
    ] {
        sheet.cells.insert(CellName::new(n), c);
    }
    let mut d = Document::empty();
    d.sheet = Arc::new(sheet);
    assert_round_trip(&d);
}

#[test]
fn connects_keep_their_order() {
    let rows: Vec<Connect> = [(9u32, 1u32), (3, 8), (3, 2), (0, 0), (7, 7)]
        .iter()
        .map(|(f, t)| Connect { from_sheet: *f, to_sheet: *t, from_cell: Some(Arc::from("Controls.Row_1")), ..Connect::default() })
        .collect();
    let mut d = Document::empty();
    let mut p = Page::new(1, "P");
    p.connects = Connects::from_vec(rows.clone());
    d.pages.push(Arc::new(p));
    let m = Master { connects: Connects::from_vec(rows.iter().rev().cloned().collect()), ..Master::default() };
    d.masters.push(Arc::new(m));
    let back = round_trip(&d);
    assert_eq!(back.pages.get(0).unwrap().connects.iter().cloned().collect::<Vec<_>>(), rows);
    assert_eq!(back.masters.get(0).unwrap().connects.iter().cloned().rev().collect::<Vec<_>>(), rows);
}

#[test]
fn a_kept_element_is_data_and_names_only_our_prefix() {
    let inner = RawElement { name: Arc::from("v:bar"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from("t"))] };
    let foo = RawElement {
        name: Arc::from("v:foo"),
        attrs: RawAttrs::from_pairs([("xmlns:v", "urn:vendor"), ("v:a", "1")]),
        children: vec![RawNode::Element(inner), RawNode::Comment(Arc::from("c")), RawNode::Pi { target: Arc::from("p"), data: Arc::from("d") }],
    };
    let mut s = Shape::new(1);
    s.passthrough = Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Element(foo) }]);
    let d = page_of(vec![s]);
    assert_round_trip(&d);
    let p = pack(&d);
    let page = parsed(p.pages.first().unwrap());
    let mut stack = vec![&page];
    let mut found = false;
    while let Some(e) = stack.pop() {
        assert_eq!(e.prefix(), "diacraft", "{}", e.name);
        if e.is("el") && e.attr("name") == Some("v:foo") {
            found = true;
        }
        stack.extend(e.elements());
    }
    assert!(found, "the kept element is an `el` with its name as an attribute");
}

#[test]
fn no_cdata_is_written() {
    let d =
        page_of(vec![shape_with_text(vec![TextPart::Chars(Arc::from("a ]]> b <![CDATA[ c ]]>")), TextPart::Raw(RawNode::Text(Arc::from("]]>")))])]);
    assert_round_trip(&d);
    assert!(!all_text(&d).contains("<![CDATA["));
}

#[test]
fn f64_text_round_trips() {
    for x in [0.1, -0.0, 0.0, 1e300, 5e-324, f64::MAX, f64::MIN, f64::MIN_POSITIVE, 1.0 / 3.0, -2.5e-7, 123456789012345680.0, 1.0] {
        let text = f64_text(x);
        assert_eq!(parse_f64(&text).map(f64::to_bits), Some(x.to_bits()), "{x:e} as {text}");
    }
    assert_eq!(f64_text(-0.0), "-0");
    assert_eq!((f64_text(f64::NAN), f64_text(f64::INFINITY), f64_text(f64::NEG_INFINITY)), ("NaN".to_owned(), "INF".to_owned(), "-INF".to_owned()));
    assert!(parse_f64("NaN").is_some_and(f64::is_nan));
    assert_eq!((parse_f64("INF"), parse_f64("-INF")), (Some(f64::INFINITY), Some(f64::NEG_INFINITY)));
    for bad in ["", "inf", "nan", "1_0", " 1", "0x1", "1e999", "--1", "infinity"] {
        assert_eq!(parse_f64(bad), None, "{bad:?}");
    }
}

#[test]
fn base64_is_the_standard_alphabet_padded_and_nothing_else() {
    for (plain, coded) in
        [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
    {
        assert_eq!(base64_encode(plain.as_bytes()), coded);
        assert_eq!(base64_decode(coded).as_deref(), Some(plain.as_bytes()));
    }
    for (bytes, coded) in [(vec![0xFB, 0xEF, 0xBE], "++++"), (vec![0xFF, 0xFF, 0xFF], "////"), (vec![0xFB, 0xFF], "+/8="), (vec![0xFF], "/w==")] {
        assert_eq!(base64_encode(&bytes), coded);
        assert_eq!(base64_decode(coded), Some(bytes));
    }
    for len in 0u8..40 {
        let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37).wrapping_add(len)).collect();
        assert_eq!(base64_decode(&base64_encode(&bytes)), Some(bytes));
    }
    // White space, a character outside the alphabet, padding in the middle, a length that is not a multiple of four, and unused bits that are not zero.
    for bad in [" Zm9v", "Zm9v\n", "Z$==", "Zg=A", "Zg===", "=Zg=", "Zg==Zg==", "Zm9", "Zh==", "Zm9=", "Zm9v!"] {
        assert_eq!(base64_decode(bad), None, "{bad:?}");
    }
}

fn picture(part: &str, bytes: &Arc<[u8]>) -> Arc<ForeignData> {
    Arc::new(ForeignData {
        picture: Some(Arc::new(Media { part: Arc::from(part), content_type: Arc::from("image/png"), bytes: Arc::clone(bytes) })),
        ..ForeignData::default()
    })
}

#[test]
fn a_blob_named_twice_decodes_to_one_arc() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let twin: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let mut a = Shape::new(1);
    a.foreign = Some(picture("/media/image1.png", &bytes));
    let mut b = Shape::new(2);
    b.foreign = Some(picture("/media/image1.png", &bytes));
    // Equal bytes in another `Arc` and another part name: the same blob, not the same picture.
    let mut c = Shape::new(3);
    c.foreign = Some(picture("/media/image2.png", &twin));
    let d = page_of(vec![a, b, c]);
    let packed = pack(&d);
    assert_eq!(packed.blobs.len(), 1, "equal bytes are one blob");
    let (back, notes) = unpack(&packed).unwrap();
    assert!(notes.is_empty());
    assert_eq!(back, d);
    let shapes = &back.pages.get(0).unwrap().shapes;
    let media = |i: usize| Arc::clone(shapes.get(i).unwrap().foreign.as_ref().unwrap().picture.as_ref().unwrap());
    assert!(Arc::ptr_eq(&media(0), &media(1)), "one picture shared by two shapes is one `Arc<Media>`");
    assert!(!Arc::ptr_eq(&media(0), &media(2)));
    assert!(Arc::ptr_eq(&media(0).bytes, &media(2).bytes), "one blob is one `Arc` of bytes");
}

/// A sink that numbers blobs downwards from 9, so that the order of first use is the reverse of the order of the numbers.
struct Downwards {
    next: u32,
    inner: MemBlobs,
}

impl BlobSink for Downwards {
    fn put(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError> {
        self.inner.put(bytes, media_type)?;
        self.next -= 1;
        Ok(BlobId(self.next + 1))
    }
}

#[test]
fn a_sink_that_numbers_out_of_order_still_gets_a_list_in_order_of_number() {
    let mut shapes = Vec::new();
    for (i, text) in ["one", "two", "three"].iter().enumerate() {
        let bytes: Arc<[u8]> = Arc::from(text.as_bytes());
        let mut s = Shape::new(i as u32 + 1);
        s.foreign = Some(picture(&format!("/media/image{i}.png"), &bytes));
        shapes.push(s);
    }
    let d = page_of(shapes);
    let mut sink = Downwards { next: 9, inner: MemBlobs::new() };
    let encoded = encode(&d, &mut sink).unwrap();
    let listed: Vec<(String, String)> = encoded
        .document
        .elements()
        .filter(|e| e.local() == "blob")
        .map(|e| (e.attr("id").unwrap_or("0").to_owned(), e.attr("size").unwrap_or("").to_owned()))
        .collect();
    // First use gave 9, 8, 7 (sizes 3, 3, 5): the list runs 7, 8, 9.
    assert_eq!(listed, [("7".to_owned(), "5".to_owned()), ("8".to_owned(), "3".to_owned()), ("9".to_owned(), "3".to_owned())]);
}

#[test]
fn only_eight_pictures_of_one_blob_are_looked_for_again() {
    // One blob under nine part names, then each name again: the first eight are found again and share one `Arc<Media>`; the ninth was not kept in the list
    // that a decode of the blob searches (it is bounded, so that one blob named many ways cannot make a search quadratic), and is a picture of its own.
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let mut shapes = Vec::new();
    for round in 0..2 {
        for i in 0..9u32 {
            let mut s = Shape::new(round * 9 + i + 1);
            s.foreign = Some(picture(&format!("/media/image{i}.png"), &bytes));
            shapes.push(s);
        }
    }
    let d = page_of(shapes);
    let (back, notes) = unpack(&pack(&d)).unwrap();
    assert!(notes.is_empty());
    assert_eq!(back, d);
    let list = &back.pages.get(0).unwrap().shapes;
    let media = |i: usize| Arc::clone(list.get(i).unwrap().foreign.as_ref().unwrap().picture.as_ref().unwrap());
    for i in 0..8 {
        assert!(Arc::ptr_eq(&media(i), &media(i + 9)), "picture {i} is shared with its repeat");
    }
    assert!(!Arc::ptr_eq(&media(8), &media(17)), "the ninth is not looked for again");
    assert!(Arc::ptr_eq(&media(8).bytes, &media(17).bytes), "though its bytes are the one blob's");
}

#[test]
fn blobs_are_numbered_by_first_use_the_document_then_pages_then_masters() {
    let b1: Arc<[u8]> = Arc::from(&b"one"[..]);
    let b2: Arc<[u8]> = Arc::from(&b"two"[..]);
    let b3: Arc<[u8]> = Arc::from(&b"three"[..]);
    let mut d = Document::empty();
    let m = Master { shapes: ShapeList::from_vec(vec![Arc::new(Shape { foreign: Some(picture("m", &b1)), ..Shape::new(1) })]), ..Master::default() };
    d.masters.push(Arc::new(m));
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(vec![Arc::new(Shape { foreign: Some(picture("p", &b2)), ..Shape::new(1) })]);
    d.pages.push(Arc::new(p));
    d.themes.push(Arc::new(Theme { part: Arc::from("/theme/theme1.xml"), bytes: Arc::clone(&b3), ..Theme::default() }));
    let packed = pack(&d);
    let ids: Vec<(u32, &[u8])> = (0..3).map(|i| (i, &**packed.blobs.entry(BlobId(i)).unwrap().0)).collect();
    assert_eq!(ids, [(0, &b"three"[..]), (1, &b"two"[..]), (2, &b"one"[..])]);
    let lists = part_lists(&parsed(&packed.document)).unwrap();
    let paths: Vec<&str> = lists.blobs.iter().map(|b| b.path.as_str()).collect();
    assert_eq!(paths, ["blobs/0.xml", "blobs/1.png", "blobs/2.png"]);
    assert_eq!(lists.blobs.iter().map(|b| b.size).collect::<Vec<_>>(), [5, 3, 3]);
    // Each record names the media type its bytes were given with.
    let media: Vec<&str> = lists.blobs.iter().map(|b| b.media_type.as_str()).collect();
    assert_eq!(media, ["application/vnd.openxmlformats-officedocument.theme+xml", "image/png", "image/png"]);
}

#[test]
fn a_missing_blob_is_a_lost_picture_not_a_refusal() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let theme_bytes: Arc<[u8]> = Arc::from(&b"<a:theme/>"[..]);
    let mut a = Shape::new(1);
    a.foreign = Some(picture("/media/image1.png", &bytes));
    let mut b = Shape::new(2);
    b.foreign = Some(picture("/media/image1.png", &bytes));
    let mut d = page_of(vec![a, b]);
    d.themes.push(Arc::new(Theme { part: Arc::from("/theme/theme1.xml"), bytes: theme_bytes, ..Theme::default() }));
    let mut packed = pack(&d);
    packed.blobs.lose(BlobId(0));
    packed.blobs.lose(BlobId(1));
    let (back, notes) = unpack(&packed).unwrap();
    assert!(back.themes.is_empty(), "a theme whose bytes are lost is left out");
    let shapes = &back.pages.get(0).unwrap().shapes;
    assert!(shapes.iter().all(|s| s.foreign.as_ref().unwrap().picture.is_none()), "the picture is not shown");
    assert_eq!(
        notes.iter().filter(|n| matches!(n, DecodeNote::PictureLost { .. })).count(),
        1,
        "one note for each blob, not for each shape: {notes:?}"
    );
    assert!(notes.contains(&DecodeNote::ThemeLost { part: "/theme/theme1.xml".to_owned() }));
}

#[test]
fn a_blob_that_disagrees_with_its_record_is_lost() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let mut a = Shape::new(1);
    a.foreign = Some(picture("/media/image1.png", &bytes));
    let d = page_of(vec![a]);
    // The same size and other bytes, then another size.
    for other in [&b"\x89PNG DATA"[..], &b"short"[..]] {
        let mut packed = pack(&d);
        packed.blobs.replace(BlobId(0), Arc::from(other));
        let (back, notes) = unpack(&packed).unwrap();
        assert!(back.pages.get(0).unwrap().shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.is_none());
        assert!(notes.contains(&DecodeNote::PictureLost { blob: BlobId(0) }), "{notes:?}");
    }
}

#[test]
fn a_picture_is_charged_by_the_size_its_record_states_before_it_is_fetched() {
    struct Spy(std::cell::Cell<bool>);
    impl BlobSource for Spy {
        fn get(&self, _: BlobId) -> Option<Arc<[u8]>> {
            self.0.set(true);
            None
        }
    }
    let bytes: Arc<[u8]> = Arc::from(vec![1u8; 1000]);
    let mut a = Shape::new(1);
    a.foreign = Some(picture("p", &bytes));
    let packed = pack(&page_of(vec![a]));
    let spy = Spy(std::cell::Cell::new(false));
    let mut budget = Budget::for_open(&Limits::default());
    budget.blob_bytes = 999;
    let out = unpack_with(&packed, &spy, &mut budget);
    assert!(matches!(out, Err(CodecError::Limit { what: "bytes of pictures and parts in the drawing", .. })), "{:?}", out.err());
    assert!(!spy.0.get(), "the source was not asked for a blob the budget could not pay for");
}

#[test]
fn a_part_names_a_duplicated_id_apart() {
    let mut d = Document::empty();
    for (id, name) in [(5u32, "a"), (5, "b"), (6, "c"), (5, "d")] {
        d.pages.push(Arc::new(Page::new(id, name)));
    }
    d.masters.push(Arc::new(Master { id: 5, ..Master::default() }));
    d.masters.push(Arc::new(Master { id: 5, ..Master::default() }));
    let packed = pack(&d);
    let lists = part_lists(&parsed(&packed.document)).unwrap();
    let names: Vec<&str> = lists.pages.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["pages/page5.xml", "pages/page5-1.xml", "pages/page6.xml", "pages/page5-2.xml"]);
    assert_eq!(lists.masters.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(), ["masters/master5.xml", "masters/master5-1.xml"]);
    assert_round_trip(&d);
}

#[test]
fn what_has_no_place_for_unknown_content_counts_it_and_the_rest_is_read() {
    // An attribute of a marker and a long field no build knows have no slot to be kept in: they are counted, in one note. What an object with a slot
    // does not know is kept (`model_ext.rs`), so it is not lost and not counted.
    let d = page_of(vec![shape_with_text(vec![TextPart::Cp(1)])]);
    let packed = pack(&d);
    let mut page = parsed(packed.pages.first().unwrap());
    let shape = element_mut(&mut page, "shape");
    shape.children.push(Node::El(El {
        name: "diacraft:s".to_owned(),
        attrs: vec![("n".to_owned(), "newfield".to_owned())],
        children: vec![Node::Text("long".to_owned())],
    }));
    let text = element_mut(shape, "text");
    element_mut(text, "cp").attrs.push(("future".to_owned(), "1".to_owned()));
    let document = parsed(&packed.document);
    let mut notes = Vec::new();
    let back = decode(&document, &[&page], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert_eq!(back, d, "what is known is read as it was");
    assert_eq!(notes, [DecodeNote::Unknown { count: 2 }]);
}

#[test]
fn what_a_theme_or_a_solution_holds_that_no_build_knows_is_counted() {
    // A theme has its part name and its bytes and nothing else, and so has no slot: a child element is counted and charged. A solution holds one kept
    // element: an attribute of its own is counted too.
    let bytes: Arc<[u8]> = Arc::from(&b"<a:theme/>"[..]);
    let mut d = Document::empty();
    d.themes.push(Arc::new(Theme { part: Arc::from("/theme/theme1.xml"), bytes, ..Theme::default() }));
    d.diacraft = Some(Arc::new(RawElement { name: Arc::from("sol"), attrs: RawAttrs::new(), children: Vec::new() }));
    let packed = pack(&d);
    let read = |document: &El, budget: &mut Budget| {
        let mut notes = Vec::new();
        let back = decode(document, &[], &[], &packed.blobs, budget, &mut notes);
        (back, notes)
    };
    let mut document = parsed(&packed.document);
    let (back, notes) = read(&document, &mut Budget::for_open(&Limits::default()));
    assert_eq!(back.unwrap(), d);
    assert!(notes.is_empty(), "{notes:?}");
    element_mut(&mut document, "theme").children.push(Node::El(El { name: "diacraft:future".to_owned(), attrs: Vec::new(), children: Vec::new() }));
    let (back, notes) = read(&document, &mut Budget::for_open(&Limits::default()));
    assert_eq!(back.unwrap(), d, "what is known is read as it was");
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    element_mut(&mut document, "solution").attrs.push(("future".to_owned(), "1".to_owned()));
    let (back, notes) = read(&document, &mut Budget::for_open(&Limits::default()));
    assert_eq!(back.unwrap(), d);
    assert_eq!(notes, [DecodeNote::Unknown { count: 2 }]);
}

/// A part of unknown content `n` elements deep, each of the codec's own namespace and none of a name this build reads.
fn unknown_chain(n: usize) -> El {
    let level = |children: Vec<Node>| El { name: "diacraft:future".to_owned(), attrs: Vec::new(), children };
    let mut e = level(Vec::new());
    for _ in 1..n {
        e = level(vec![Node::El(e)]);
    }
    e
}

#[test]
fn an_unknown_element_kept_in_the_model_is_written_to_the_depth_the_model_allows_and_no_deeper() {
    let chain = |n: usize| {
        let level = |children: Vec<RawNode>| RawNode::Element(RawElement { name: Arc::from("q:e"), attrs: RawAttrs::new(), children });
        let mut node = level(Vec::new());
        for _ in 1..n {
            node = level(vec![node]);
        }
        node
    };
    let doc = |n: usize| {
        let mut d = Document::empty();
        d.ext.make_mut().native.kept = Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: chain(n) }]);
        d
    };
    assert!(encode(&doc(200), &mut MemBlobs::new()).is_ok(), "200 levels");
    assert!(matches!(encode(&doc(201), &mut MemBlobs::new()), Err(CodecError::Limit { what: "levels of a kept element", .. })));
}

#[test]
fn an_unknown_element_is_kept_to_the_depth_the_model_allows_and_no_deeper() {
    let packed = pack(&page_of(vec![Shape::new(1)]));
    let page = |depth: usize| {
        let mut page = parsed(packed.pages.first().unwrap());
        element_mut(&mut page, "shape").children.push(Node::El(unknown_chain(depth)));
        page
    };
    let read = |page: &El| diacraft_sheetxml::model::decode_page(page, &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new());
    assert!(read(&page(200)).is_ok(), "200 levels of an element a newer build wrote");
    assert!(matches!(read(&page(201)), Err(CodecError::Limit { what: "levels of a kept element", .. })));
}

#[test]
fn what_a_picture_or_an_odf_wrapper_holds_that_no_build_knows_is_counted() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG data"[..]);
    let mut with_picture = Shape::new(1);
    with_picture.foreign = Some(picture("/media/image1.png", &bytes));
    let mut with_wrapper = Shape::new(2);
    with_wrapper.text = Some(Arc::new(Text {
        parts: vec![TextPart::Chars(Arc::from("a")), TextPart::OdfRaw(Box::new(RawNode::Comment(Arc::from("odf"))))],
        ..Text::default()
    }));
    let d = page_of(vec![with_picture, with_wrapper]);
    let packed = pack(&d);
    let mut page = parsed(packed.pages.first().unwrap());
    let read = |page: &El| {
        let mut notes = Vec::new();
        let back = decode(&parsed(&packed.document), &[page], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes);
        (back, notes)
    };
    let (back, notes) = read(&page);
    assert_eq!(back.unwrap(), d);
    assert!(notes.is_empty(), "{notes:?}");
    let picture_el = element_mut(element_mut(element_mut(&mut page, "shape"), "foreign"), "picture");
    picture_el.children.push(Node::El(El { name: "diacraft:future".to_owned(), attrs: Vec::new(), children: Vec::new() }));
    let (back, notes) = read(&page);
    assert_eq!(back.unwrap(), d, "what is known is read as it was");
    assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    let shape2 = page.children.iter_mut().filter_map(|n| if let Node::El(c) = n { Some(c) } else { None }).nth(1).unwrap();
    element_mut(element_mut(shape2, "text"), "odf-raw").attrs.push(("future".to_owned(), "1".to_owned()));
    let (back, notes) = read(&page);
    assert_eq!(back.unwrap(), d);
    assert_eq!(notes, [DecodeNote::Unknown { count: 2 }]);
}

#[test]
fn what_a_wrapper_of_kept_data_holds_that_no_build_knows_is_counted() {
    use diacraft_doc::{Format, Kept, OdfShape, OdfStyle, Part, PartRole, Placed, VsdxHeld, VsdxRest};
    let raw = |name: &str| Arc::new(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: Vec::new() });
    let bag = || RawAttrs::from_pairs([("a", "1")]);
    let mut d = Document::empty();
    {
        let data = d.ext.make_mut();
        data.odf.settings = Some(raw("settings"));
        data.odf.layers = vec![raw("layer")];
        data.odf.meta = Passthrough::from_nodes(vec![Placed { after: 0, node: RawNode::Comment(Arc::from("c")) }]);
        data.vsdx = VsdxRest(vec![(Arc::from("k"), Kept::default())]);
    }
    d.vsdx = Some(Arc::new(VsdxHeld {
        rels: Vec::new(),
        content_types: Vec::new(),
        props_rest: vec![(Arc::from("k"), Kept::default())],
        windows: Some(raw("Windows")),
    }));
    Arc::make_mut(&mut d.sheet).ext.make_mut().odf = Kept { raw: bag(), kept: Passthrough::new() };
    let mut style = Style::default();
    style.ext.make_mut().odf = OdfStyle { unmapped: bag(), units: bag(), ..OdfStyle::default() };
    d.styles.push(Arc::new(style));
    d.parts.push(Arc::new(Part {
        format: Format::Native,
        name: Arc::from("/x"),
        media_type: Arc::from("a/b"),
        role: PartRole::Thumbnail,
        manifest: bag(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&b"x"[..]),
    }));
    let mut page = Page::new(1, "P");
    {
        let odf = &mut page.ext.make_mut().odf;
        odf.drawing_page_style = Some(raw("style"));
        odf.units = bag();
        odf.master_page_attrs = bag();
        odf.layout_props = bag();
    }
    let mut shape = Shape::new(1);
    shape.ext.make_mut().odf = OdfShape { units: bag(), ..OdfShape::default() };
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    d.pages.push(Arc::new(page));
    assert_round_trip(&d);
    let packed = pack(&d);
    // Each wrapper in turn, with an attribute of its own that no build knows: one thing counted, and what is known read as it was.
    let in_document: [&[&str]; 11] = [
        &["vsdx-held"],
        &["ext", "odf", "settings"],
        &["ext", "odf", "layer"],
        &["ext", "odf", "meta"],
        &["ext", "vsdx"],
        &["vsdx-held", "windows"],
        &["vsdx-held", "props-rest"],
        &["sheet", "ext", "odf"],
        &["style", "ext", "odf", "unmapped"],
        &["style", "ext", "odf", "units"],
        &["part", "manifest"],
    ];
    let in_page: [&[&str]; 5] = [
        &["ext", "odf", "drawing-page-style"],
        &["ext", "odf", "units"],
        &["ext", "odf", "master-page-attrs"],
        &["ext", "odf", "layout-props"],
        &["shape", "ext", "odf", "units"],
    ];
    let reach = |root: &mut El, path: &[&str]| {
        let mut at = root;
        for local in path {
            at = element_mut(at, local);
        }
        at.attrs.push(("future".to_owned(), "1".to_owned()));
    };
    for (path, is_page) in in_document.iter().map(|p| (*p, false)).chain(in_page.iter().map(|p| (*p, true))) {
        let (mut document, mut page) = (parsed(&packed.document), parsed(packed.pages.first().unwrap()));
        reach(if is_page { &mut page } else { &mut document }, path);
        let mut notes = Vec::new();
        let back = decode(&document, &[&page], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
        assert_eq!(back, d, "{path:?}");
        assert_eq!(notes, [DecodeNote::Unknown { count: 1 }], "{path:?}");
    }
}

#[test]
fn a_kept_wrapper_holds_one_node_and_counts_a_second() {
    let mut s = Shape::new(1);
    s.passthrough = Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Text(Arc::from("a")) }]);
    let d = page_of(vec![s]);
    let packed = pack(&d);
    let twin = |page: &El| {
        let mut page = page.clone();
        let kept = element_mut(element_mut(&mut page, "shape"), "kept");
        let twin = kept.children.iter().find(|n| matches!(n, Node::El(_))).cloned().unwrap();
        (page, twin)
    };
    let extras = [
        // A second node.
        None,
        // An element of another namespace, and one of ours that no build reads.
        Some(El { name: "x:other".to_owned(), attrs: vec![("xmlns:x".to_owned(), "urn:other".to_owned())], children: Vec::new() }),
        Some(El { name: "diacraft:future".to_owned(), attrs: Vec::new(), children: Vec::new() }),
    ];
    for extra in extras {
        let (mut page, second) = twin(&parsed(packed.pages.first().unwrap()));
        let kept = element_mut(element_mut(&mut page, "shape"), "kept");
        kept.children.push(match extra {
            Some(e) => Node::El(e),
            None => second,
        });
        let mut notes = Vec::new();
        let back = decode(&parsed(&packed.document), &[&page], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
        assert_eq!(back, d, "the first node is the kept one");
        assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    }
}

#[test]
fn a_definition_whose_bytes_are_lost_is_a_lost_part_and_keeps_its_element() {
    use diacraft_doc::{DefFamily, Definition};
    let element = Arc::new(RawElement { name: Arc::from("draw:fill-image"), attrs: RawAttrs::new(), children: Vec::new() });
    let mut d = Document::empty();
    d.ext.make_mut().odf.definitions = vec![Definition {
        family: DefFamily::FillImage,
        name: Arc::from("img1"),
        element: Arc::clone(&element),
        bytes: Some(Arc::from(&b"pic"[..])),
    }];
    let mut packed = pack(&d);
    packed.blobs.lose(BlobId(0));
    let (back, notes) = unpack(&packed).unwrap();
    assert_eq!(notes, [DecodeNote::PartLost { name: "img1".to_owned() }]);
    let kept = &back.ext.get().unwrap().odf.definitions;
    assert_eq!(kept.len(), 1);
    assert!(kept[0].bytes.is_none() && kept[0].element == element, "the element stays and the bytes are gone");
}

#[test]
fn the_odf_and_the_vsdx_child_of_an_ext_are_each_read_in_either_order() {
    use diacraft_doc::{Kept, VsdxRest};
    let mut d = Document::empty();
    {
        let data = d.ext.make_mut();
        data.odf.mimetype = Some(Arc::from("application/x"));
        data.vsdx = VsdxRest(vec![(Arc::from("k"), Kept::default())]);
    }
    assert_round_trip(&d);
    let packed = pack(&d);
    let read = |document: &El| {
        let mut notes = Vec::new();
        let back = decode(document, &[], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
        (back, notes)
    };
    let mut document = parsed(&packed.document);
    let ext = element_mut(&mut document, "ext");
    assert_eq!(ext.children.len(), 2, "an odf and a vsdx");
    // The vsdx first: both are the first of their kind.
    ext.children.reverse();
    let (back, notes) = read(&document);
    assert_eq!(back, d);
    assert!(notes.is_empty(), "{notes:?}");
    // A second of either kind is counted and not read, whichever comes first.
    let twins: Vec<Node> = element_mut(&mut document, "ext").children.clone();
    for twin in twins {
        let mut again = document.clone();
        element_mut(&mut again, "ext").children.push(twin);
        let (back, notes) = read(&again);
        assert_eq!(back, d, "the first of each kind is the one read");
        assert_eq!(notes, [DecodeNote::Unknown { count: 1 }]);
    }
}

#[test]
fn a_second_text_style_of_a_paragraph_or_a_run_is_counted_and_not_read() {
    use diacraft_doc::{Kept, OdfParagraph, OdfRun, OdfText, OdfTextStyle};
    let style = |n: &str| OdfTextStyle { name: Some(Arc::from(n)), ..OdfTextStyle::default() };
    let mut text = Text { parts: vec![TextPart::Chars(Arc::from("a"))], ..Text::default() };
    text.ext.make_mut().odf = OdfText {
        kept: Kept::default(),
        paragraphs: vec![OdfParagraph {
            style: style("pa"),
            characters: None,
            runs: vec![OdfRun { from: 0, to: 1, style: style("ru"), element: None }],
            element: None,
            levels: Vec::new(),
        }],
    };
    let d = page_of(vec![Shape { id: 1, text: Some(Arc::new(text)), ..Shape::default() }]);
    assert_round_trip(&d);
    let packed = pack(&d);
    let paths: [&[&str]; 2] = [&["shape", "text", "ext", "odf", "paragraph"], &["shape", "text", "ext", "odf", "paragraph", "run"]];
    for path in paths {
        let mut page = parsed(packed.pages.first().unwrap());
        let mut at = &mut page;
        for local in path {
            at = element_mut(at, local);
        }
        let twin = at.children.iter().find(|n| matches!(n, Node::El(e) if e.local() == "text-style")).cloned().unwrap();
        at.children.push(twin);
        let mut notes = Vec::new();
        let back = decode(&parsed(&packed.document), &[&page], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
        assert_eq!(back, d, "{path:?}: the first text style is the one");
        assert_eq!(notes, [DecodeNote::Unknown { count: 1 }], "{path:?}");
    }
}

fn nth_color(document: &mut El, nth: usize) -> &mut El {
    document.children.iter_mut().filter_map(|n| if let Node::El(c) = n { Some(c) } else { None }).filter(|c| c.local() == "color").nth(nth).unwrap()
}

#[test]
fn a_record_with_only_unknown_attributes_or_only_unknown_children_keeps_them() {
    use diacraft_doc::ColorEntry;
    let mut d = Document::empty();
    d.colors = Arc::new(vec![ColorEntry { ix: 3, rgb: Arc::from("#000000") }, ColorEntry { ix: 4, rgb: Arc::from("#ffffff") }]);
    let packed = pack(&d);
    let read = |document: &El| {
        let mut notes = Vec::new();
        let back = decode(document, &[], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        back
    };
    // An attribute and no child.
    let mut document = parsed(&packed.document);
    nth_color(&mut document, 0).attrs.push(("future".to_owned(), "1".to_owned()));
    let back = read(&document);
    let entries: Vec<&RawNode> = back.ext.get().unwrap().native.kept.iter().map(|p| &p.node).collect();
    assert!(matches!(entries.as_slice(), [RawNode::Element(e)] if &*e.name == "color[3]" && e.attrs.iter().collect::<Vec<_>>() == [("future", "1")]));
    assert!(round_trip(&back) == back);
    // A child and no attribute.
    let mut document = parsed(&packed.document);
    nth_color(&mut document, 1).children.push(Node::El(El { name: "diacraft:future".to_owned(), attrs: Vec::new(), children: Vec::new() }));
    let back = read(&document);
    let entries: Vec<&RawNode> = back.ext.get().unwrap().native.kept.iter().map(|p| &p.node).collect();
    assert!(matches!(entries.as_slice(), [RawNode::Element(e)] if &*e.name == "color[4]" && e.children.len() == 1));
    assert!(round_trip(&back) == back);
}

/// A kept tree in its data form (`el` inside `el`) `n` levels deep, inside the `kept` element of a shape.
fn kept_el_chain(n: usize) -> El {
    let level = |children: Vec<Node>| El { name: "diacraft:el".to_owned(), attrs: vec![("name".to_owned(), "x".to_owned())], children };
    let mut e = level(Vec::new());
    for _ in 1..n {
        e = level(vec![Node::El(e)]);
    }
    e
}

#[test]
fn a_kept_tree_in_its_data_form_is_read_to_the_depth_the_model_allows_and_no_deeper() {
    let mut s = Shape::new(1);
    s.passthrough = Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Text(Arc::from("a")) }]);
    let packed = pack(&page_of(vec![s]));
    let page = |depth: usize| {
        let mut page = parsed(packed.pages.first().unwrap());
        let kept = element_mut(element_mut(&mut page, "shape"), "kept");
        kept.children = vec![Node::El(kept_el_chain(depth))];
        page
    };
    let read = |page: &El| diacraft_sheetxml::model::decode_page(page, &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new());
    assert!(read(&page(200)).is_ok(), "200 levels of a kept element");
    assert!(matches!(read(&page(201)), Err(CodecError::Limit { what: "levels of a kept element", .. })));
}

#[test]
fn an_error_in_a_style_or_a_sheet_fragment_names_the_document_part() {
    let twice = "<d:Cell N=\"PinX\" V=\"1\"/><d:Cell N=\"PinX\" V=\"2\"/>";
    let sheet = parse_with(format!("<d:sheet xmlns:d=\"{}\">{twice}</d:sheet>", diacraft_sheetxml::model::URN).as_bytes(), &native_limits()).unwrap();
    let style = parse_with(
        format!("<d:style xmlns:d=\"{}\"><d:sheet>{twice}</d:sheet></d:style>", diacraft_sheetxml::model::URN).as_bytes(),
        &native_limits(),
    )
    .unwrap();
    let blobs = MemBlobs::new();
    let (mut budget, mut notes) = (Budget::for_open(&Limits::default()), Vec::new());
    let out = diacraft_sheetxml::model::decode_sheet(&sheet, &blobs, &mut budget, &mut notes);
    assert!(matches!(&out, Err(CodecError::Malformed { part, why: "two cells of one name in one place" }) if part == "document"), "{:?}", out.err());
    let out = diacraft_sheetxml::model::decode_style(&style, &blobs, &mut budget, &mut notes);
    assert!(matches!(&out, Err(CodecError::Malformed { part, why: "two cells of one name in one place" }) if part == "document"), "{:?}", out.err());
}

#[test]
fn a_document_whose_lists_name_other_parts_than_were_given_is_damage() {
    let mut d = Document::empty();
    d.pages.push(Arc::new(Page::new(1, "P")));
    d.masters.push(Arc::new(Master { id: 2, ..Master::default() }));
    let packed = pack(&d);
    let document = parsed(&packed.document);
    let (page, master) = (parsed(&packed.pages[0]), parsed(&packed.masters[0]));
    let run =
        |pages: &[&El], masters: &[&El]| decode(&document, pages, masters, &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new());
    assert_eq!(run(&[&page], &[&master]).unwrap(), d);
    let (none, twice): (&[&El], &[&El]) = (&[], &[&page, &page]);
    for (pages, masters) in [(none, &[&master][..]), (&[&page][..], none), (twice, &[&master][..]), (&[&page][..], &[&master, &master][..])] {
        assert!(
            matches!(run(pages, masters), Err(CodecError::Malformed { why: "the lists name other parts than were given", .. })),
            "{} pages and {} masters",
            pages.len(),
            masters.len()
        );
    }
}

#[test]
fn an_error_in_the_document_part_names_the_document() {
    let xml = format!("<d:document xmlns:d=\"{}\"><d:theme part=\"/t.xml\"/></d:document>", diacraft_sheetxml::model::URN);
    let document = parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let out = decode(&document, &[], &[], &MemBlobs::new(), &mut Budget::for_open(&Limits::default()), &mut Vec::new());
    assert!(matches!(&out, Err(CodecError::Malformed { part, why: "a theme with no blob" }) if part == "document"), "{:?}", out.err());
}

#[test]
fn a_solution_keeps_the_first_kept_element_and_counts_every_other_child() {
    let mut d = Document::empty();
    d.diacraft = Some(Arc::new(RawElement { name: Arc::from("sol"), attrs: RawAttrs::new(), children: Vec::new() }));
    let packed = pack(&d);
    let mut document = parsed(&packed.document);
    let solution = element_mut(&mut document, "solution");
    let other = Node::El(El { name: "diacraft:other".to_owned(), attrs: Vec::new(), children: Vec::new() });
    let second = Node::El(El { name: "diacraft:el".to_owned(), attrs: vec![("name".to_owned(), "second".to_owned())], children: Vec::new() });
    solution.children.insert(0, other);
    solution.children.push(second);
    let mut notes = Vec::new();
    let back = decode(&document, &[], &[], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert_eq!(back, d, "the first `el` is the solution, and what comes before it or after it is not");
    assert_eq!(notes, [DecodeNote::Unknown { count: 2 }]);
}

#[test]
fn a_page_or_master_part_whose_id_is_not_the_one_the_list_gives_is_damage() {
    let mut d = Document::empty();
    d.pages.push(Arc::new(Page::new(3, "P")));
    d.masters.push(Arc::new(Master { id: 4, ..Master::default() }));
    let packed = pack(&d);
    let document = parsed(&packed.document);
    let (page, master) = (parsed(&packed.pages[0]), parsed(&packed.masters[0]));
    let set_id = |e: &El, id: &str| {
        let mut e = e.clone();
        for (k, v) in &mut e.attrs {
            if k == "id" {
                *v = id.to_owned();
            }
        }
        e
    };
    let run =
        |page: &El, master: &El| decode(&document, &[page], &[master], &packed.blobs, &mut Budget::for_open(&Limits::default()), &mut Vec::new());
    assert_eq!(run(&page, &master).unwrap(), d);
    assert!(matches!(
        run(&set_id(&page, "9"), &master),
        Err(CodecError::Malformed { why: "a page part whose ID is not the one the list gives", .. })
    ));
    assert!(matches!(
        run(&page, &set_id(&master, "9")),
        Err(CodecError::Malformed { why: "a master part whose ID is not the one the list gives", .. })
    ));
}

#[test]
fn the_namespace_is_matched_and_the_prefix_is_not() {
    let d = page_of(vec![shape_with_text(vec![TextPart::Chars(Arc::from("t"))])]);
    let packed = pack(&d);
    let text = String::from_utf8(packed.pages.first().unwrap().clone()).unwrap();
    let document = String::from_utf8(packed.document.clone()).unwrap();
    // Another prefix, and no prefix with the namespace as the default.
    let renamed = |t: &str| t.replace("<diacraft:", "<d:").replace("</diacraft:", "</d:").replace("xmlns:diacraft", "xmlns:d");
    let defaulted = |t: &str| t.replace("<diacraft:", "<").replace("</diacraft:", "</").replace("xmlns:diacraft", "xmlns");
    for f in [&renamed as &dyn Fn(&str) -> String, &defaulted] {
        let p = Packed { document: f(&document).into_bytes(), pages: vec![f(&text).into_bytes()], masters: Vec::new(), blobs: MemBlobs::new() };
        let (back, notes) = unpack(&p).unwrap();
        assert_eq!(back, d);
        assert!(notes.is_empty());
    }
    // The prefix of the codec on another namespace is not the codec's.
    let alien = text.replace(diacraft_sheetxml::model::URN, "urn:other");
    let p = Packed { document: document.clone().into_bytes(), pages: vec![alien.into_bytes()], masters: Vec::new(), blobs: MemBlobs::new() };
    assert!(matches!(unpack(&p), Err(CodecError::Malformed { .. })), "a page part in another namespace is no page part");
}

#[test]
fn a_cell_of_one_name_twice_in_one_place_is_refused_and_a_part_that_is_not_ours_is_damaged() {
    let d = page_of(vec![shape_with_cell("PinX", Cell::constant(Value::from_text("1")))]);
    let packed = pack(&d);
    let text = String::from_utf8(packed.pages.first().unwrap().clone()).unwrap();
    let twice = text.replace("<diacraft:Cell N=\"PinX\" V=\"1\"/>", "<diacraft:Cell N=\"PinX\" V=\"1\"/><diacraft:Cell N=\"PinX\" V=\"2\"/>");
    assert_ne!(twice, text);
    let p = Packed { document: packed.document.clone(), pages: vec![twice.into_bytes()], masters: Vec::new(), blobs: MemBlobs::new() };
    assert!(matches!(unpack(&p), Err(CodecError::Malformed { why: "two cells of one name in one place", .. })));
    let not_ours = Packed { document: packed.document.clone(), pages: vec![b"<page/>".to_vec()], masters: Vec::new(), blobs: MemBlobs::new() };
    assert!(matches!(unpack(&not_ours), Err(CodecError::Malformed { .. })));
    // The lists name a page and the part is not given.
    let missing = Packed { document: packed.document.clone(), pages: Vec::new(), masters: Vec::new(), blobs: MemBlobs::new() };
    assert!(matches!(unpack(&missing), Err(CodecError::Malformed { .. })));
    // A page part whose ID is not the list's.
    let other = pack(&{
        let mut d2 = Document::empty();
        d2.pages.push(Arc::new(Page::new(9, "P")));
        d2
    });
    let swapped = Packed { document: packed.document.clone(), pages: other.pages.clone(), masters: Vec::new(), blobs: MemBlobs::new() };
    assert!(matches!(unpack(&swapped), Err(CodecError::Malformed { .. })));
}

#[test]
fn numbers_flags_and_stamps_that_are_not_are_damage_and_not_a_guess() {
    let d = page_of(vec![Shape::new(5)]);
    let packed = pack(&d);
    let text = String::from_utf8(packed.pages.first().unwrap().clone()).unwrap();
    for (from, to) in
        [("id=\"5\"", "id=\"five\""), ("id=\"5\"", "id=\"-1\""), ("id=\"5\"", "id=\"4294967296\""), ("id=\"5\"", "id=\"5\" del=\"yes\"")]
    {
        let bad = text.replacen(from, to, 1);
        assert_ne!(bad, text);
        let p = Packed { document: packed.document.clone(), pages: vec![bad.into_bytes()], masters: Vec::new(), blobs: MemBlobs::new() };
        assert!(matches!(unpack(&p), Err(CodecError::Malformed { .. })), "{to}");
    }
    let doc_text = String::from_utf8(packed.document.clone()).unwrap();
    let bad = doc_text.replacen("id=\"1\"", "id=\"x\"", 1);
    assert_ne!(bad, doc_text);
    assert!(part_lists(&parsed(bad.as_bytes())).is_err());
    // A blob record with no stamp is damage too.
    let with_blob = pack(&{
        let bytes: Arc<[u8]> = Arc::from(&b"png"[..]);
        let mut s = Shape::new(1);
        s.foreign = Some(picture("p", &bytes));
        page_of(vec![s])
    });
    let doc_text = String::from_utf8(with_blob.document.clone()).unwrap();
    let start = doc_text.find(" stamp=\"").unwrap();
    let no_stamp = format!("{}{}", &doc_text[..start], doc_text[start + 1..].split_once("\"").unwrap().1.split_once("\"").unwrap().1);
    assert!(part_lists(&parsed(no_stamp.as_bytes())).is_err(), "{no_stamp}");
}

#[test]
fn a_shape_type_the_model_has_no_word_for_comes_back_and_one_it_has_is_not_an_other() {
    let mut s1 = Shape::new(1);
    s1.kind = ShapeType::Other(Arc::from("Weird"));
    let mut s2 = Shape::new(2);
    s2.kind = ShapeType::Other(Arc::from(""));
    // `Other` of a word the model has a variant for is no state a reader makes, but the codec still gives it back.
    let mut s3 = Shape::new(3);
    s3.kind = ShapeType::Other(Arc::from("Group"));
    let mut s4 = Shape::new(4);
    s4.kind = ShapeType::Group;
    assert_round_trip(&page_of(vec![s1, s2, s3, s4, Shape::new(5)]));
}

#[test]
fn long_text_goes_in_an_element_and_stays_whatever_is_in_it() {
    let quotes = "\"".repeat(200_000);
    let amps = "&\n<".repeat(100_000);
    let with_bad = format!("{}\u{1}{}", "z".repeat(ATTR_MAX_BYTES), "\u{ffff}");
    let mut s = Shape::new(1);
    s.name = Some(Arc::from(quotes.as_str()));
    s.name_u = Some(Arc::from(amps.as_str()));
    s.data1 = Some(Arc::from(with_bad.as_str()));
    s.data2 = Some(Arc::from("x".repeat(ATTR_MAX_BYTES).as_str()));
    s.data3 = Some(Arc::from("x".repeat(ATTR_MAX_BYTES + 1).as_str()));
    let mut c = Cell::with_formula(&"1+".repeat(20_000), Some(Value::from_text(&quotes)));
    c.error = Some(Arc::from(amps.as_str()));
    s.raw = RawAttrs::from_pairs([(quotes.as_str(), amps.as_str())]);
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("Long"), c);
    s.sheet = Arc::new(sheet);
    let d = page_of(vec![s]);
    assert_round_trip(&d);
    // Not one attribute is longer than the cap, so the XML reader's own cap on an attribute, as written, is never near.
    let p = pack(&d);
    let page = parsed(p.pages.first().unwrap());
    let mut stack = vec![&page];
    let mut longest = 0usize;
    while let Some(e) = stack.pop() {
        longest = longest.max(e.attrs.iter().map(|(_, v)| v.len()).max().unwrap_or(0));
        stack.extend(e.elements());
    }
    assert!(longest <= ATTR_MAX_BYTES * 4 / 3 + 4, "the longest attribute is {longest} bytes");
}

#[test]
fn a_value_of_a_million_bytes_that_escaping_spells_out_six_times_still_opens() {
    // 1 MiB of `"` is 6 MiB as written, past the XML reader's cap of 1 MiB for one attribute, which a value that was read from a file may be.
    let quotes = "\"".repeat(1024 * 1024);
    let mut s = Shape::new(1);
    s.name = Some(Arc::from(quotes.as_str()));
    s.raw = RawAttrs::from_pairs([("k", quotes.as_str())]);
    assert_round_trip(&page_of(vec![s]));
}

#[test]
fn a_page_past_the_xml_readers_item_cap_is_refused_when_written_and_not_written_unreadable() {
    // 1,000 shapes of 5,000 attr children each would be about 5 million items: refused as a part the reader would refuse (it takes 4 million), before the tree is built whole.
    let mut shapes = Vec::new();
    for i in 0..1_000u32 {
        let mut s = Shape::new(i);
        s.raw = RawAttrs::from_pairs((0..5_000).map(|k| (format!("a{k}"), "v")));
        shapes.push(s);
    }
    let d = page_of(shapes);
    let mut blobs = MemBlobs::new();
    let out = encode(&d, &mut blobs);
    // The whole-document encoder holds every part's tree at once, so what is refused is the part that passes what one tree may have, said as the parts held at once.
    assert_eq!(out.err(), Some(CodecError::Limit { what: "XML items in the parts held at once", max: diacraft_sheetxml::model::OPEN_TREE_ITEMS }));
}

#[test]
fn groups_nested_past_the_cap_are_refused_when_written() {
    let too_deep = diacraft_testkit::model::deep_chain(201, Shape::new(9));
    let mut blobs = MemBlobs::new();
    assert!(matches!(encode(&page_of(vec![too_deep]), &mut blobs), Err(CodecError::Limit { what: "levels of nested shapes", max: 200 })));
    let at_the_cap = diacraft_testkit::model::deep_chain(200, Shape::new(9));
    assert_round_trip(&page_of(vec![at_the_cap]));
}

fn kept_chain(levels: usize) -> RawElement {
    let mut e = RawElement { name: Arc::from("x:leaf"), attrs: RawAttrs::from_pairs([("a", "1")]), children: vec![RawNode::Text(Arc::from("t"))] };
    for i in 1..levels {
        e = RawElement {
            name: Arc::from(format!("x:l{i}")),
            attrs: RawAttrs::from_pairs([("xmlns:x", "urn:x")]),
            children: vec![RawNode::Element(e)],
        };
    }
    e
}

#[test]
fn groups_at_the_depth_cap_holding_a_kept_tree_at_its_cap() {
    // 200 nested groups, and the innermost shape's cell extra holds a kept tree 200 levels deep: the deepest the codec writes. It parses under the
    // container's ceiling and decodes equal. This runs on the default test thread's stack, where a deeper recursion would be an abort the guard cannot catch.
    let mut cell = Cell::constant(Value::from_text("1"));
    cell.extra = Some(Arc::new(CellExtra {
        text: None,
        passthrough: Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Element(kept_chain(200)) }]),
        ..CellExtra::default()
    }));
    let innermost = shape_with_cell("PinX", cell);
    let d = page_of(vec![diacraft_testkit::model::deep_chain(200, innermost)]);
    assert_round_trip(&d);
    // One more level of the kept tree is refused when written.
    let mut cell = Cell::constant(Value::from_text("1"));
    cell.extra = Some(Arc::new(CellExtra {
        text: None,
        passthrough: Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Element(kept_chain(201)) }]),
        ..CellExtra::default()
    }));
    let mut blobs = MemBlobs::new();
    let too_deep = encode(&page_of(vec![shape_with_cell("PinX", cell)]), &mut blobs);
    assert!(matches!(too_deep, Err(CodecError::Limit { what: "levels of a kept element", max: 200 })), "{:?}", too_deep.err());
}

#[test]
fn content_stamp_of_the_blank_drawing_is_pinned() {
    // Recorded at the vocabulary freeze. May change it only in one commit with the reason.
    let stamp = content_stamp(&Document::new_blank(), &mut Budget::for_open(&Limits::default())).unwrap();
    assert_eq!(stamp.text(), "00ebc460d282d097");
}

fn stamp(d: &Document) -> String {
    content_stamp(d, &mut Budget::for_open(&Limits::default())).unwrap().text()
}

#[test]
fn content_stamp_changes_with_one_cell_and_returns_with_undo() {
    let before = Document::new_blank();
    let mut after = before.clone();
    let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
    let mut sheet = (*page.sheet).clone();
    sheet.cells.insert(CellName::new("PageWidth"), Cell::constant(Value::from_text("12")).with_unit("IN"));
    page.sheet = Arc::new(sheet);
    assert_ne!(stamp(&before), stamp(&after), "one cell is another content");
    assert_eq!(stamp(&before), stamp(&before.clone()));
    // An undo puts the old sheet back, as another allocation of the same cells.
    let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
    page.sheet = Arc::new((*before.pages.get(0).unwrap().sheet).clone());
    assert_eq!(stamp(&before), stamp(&after));
}

#[test]
fn content_stamp_covers_blob_bytes() {
    let one: Arc<[u8]> = Arc::from(&b"\x89PNG aaaa"[..]);
    let other: Arc<[u8]> = Arc::from(&b"\x89PNG bbbb"[..]);
    let twin: Arc<[u8]> = Arc::from(&b"\x89PNG aaaa"[..]);
    let with = |bytes: &Arc<[u8]>| {
        let mut s = Shape::new(1);
        s.foreign = Some(picture("p", bytes));
        page_of(vec![s])
    };
    assert_ne!(stamp(&with(&one)), stamp(&with(&other)), "a picture replaced by other bytes of the same size is another content");
    assert_eq!(stamp(&with(&one)), stamp(&with(&twin)), "the bytes are hashed, and not the allocation");
}

#[test]
fn a_fragment_comes_back_through_its_own_pair_of_functions() {
    let mut s = Shape::new(3);
    s.name = Some(Arc::from("a\u{1}b"));
    s.sheet = Arc::new({
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("PinX"), Cell::with_formula("Width*0.5", Some(Value::from_text("1"))));
        let mut section = Section::new("Geometry", Some(0));
        section.rows.push(Arc::new(Row { ix: Some(0), row_type: Some(Arc::from("MoveTo")), ..Row::default() }));
        sheet.sections.push(Arc::new(section));
        sheet
    });
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG"[..]);
    s.foreign = Some(picture("p", &bytes));
    let mut blobs = MemBlobs::new();
    let el = encode_shape(&s, &mut blobs).unwrap();
    let mut notes = Vec::new();
    let back = decode_shape(&el, &blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert_eq!(back, s);
    assert!(notes.is_empty());
    // The same shape from a part's bytes, where the namespace is declared on a parent and not on the shape itself.
    let style = Style { id: 4, name: Some(Arc::from("S")), ..Style::default() };
    let el = diacraft_sheetxml::model::encode_style(&style, &mut blobs).unwrap();
    let back = diacraft_sheetxml::model::decode_style(&el, &blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap();
    assert_eq!(back, style);
    let sheet = (*s.sheet).clone();
    let el = diacraft_sheetxml::model::encode_sheet(&sheet, &mut blobs).unwrap();
    assert_eq!(diacraft_sheetxml::model::decode_sheet(&el, &blobs, &mut Budget::for_open(&Limits::default()), &mut notes).unwrap(), sheet);
}
