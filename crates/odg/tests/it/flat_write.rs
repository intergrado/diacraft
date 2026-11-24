//! Saving flat drawings, `.fodg`: the unedited save is the input; an edited one splices
//! into the one document, as the package writer splices `content.xml` and `styles.xml`, with DiaCraft's generator in `office:meta` and its extension last.

use std::sync::Arc;

use diacraft_doc::{RawAttrs, RawElement, RawNode};
use diacraft_odg::{Limits, OriginalPackage, ShapeIndex, Variant, WriteError, WriteOptions, Written, read, sniff, write_with};

use crate::drawing::Drawing;
use crate::flat::{Flat, MIME, utf16le};
use crate::write_core::{Open, custom, drawing_of, full_meta, move_by, open_bytes};
use crate::write_new::{push_shape, rectangle};

/// The writer with its default options.
pub fn save(doc: &diacraft_doc::Document, bytes: &[u8], index: &ShapeIndex) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions::default())
}

/// The model of a flat drawing of three shapes, with a meta element a producer wrote.
pub fn three() -> Flat {
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    Flat::of(&d)
}

pub fn text_of(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("a flat document is text")
}

/// The generator's text in a document.
pub fn generator(text: &str) -> &str {
    let from = text.find("<meta:generator>").map(|i| i + "<meta:generator>".len()).expect("a generator");
    let to = text[from..].find("</meta:generator>").map(|i| i + from).expect("an end tag");
    &text[from..to]
}

/// An extension element as `Document::set_diacraft_child` makes it: `dc:Data` declaring its own prefix.
fn extension(n: &str) -> RawElement {
    RawElement {
        name: Arc::from("dc:Data"),
        attrs: RawAttrs::from_pairs([("xmlns:dc", diacraft_doc::DIACRAFT_URN), ("dc:version", "1")]),
        children: vec![RawNode::Element(RawElement { name: Arc::from("dc:snap"), attrs: RawAttrs::from_pairs([("n", n)]), children: Vec::new() })],
    }
}

/// A second save of the model the save gave back, through the index it gave, is no edit and the same bytes.
pub fn second_save_is_no_edit(doc: &diacraft_doc::Document, saved: &Written) {
    let again = save(doc, &saved.bytes, &saved.index).expect("the second save");
    assert!(!again.edited, "a second save is no edit");
    assert_eq!(again.bytes, saved.bytes, "and the same bytes");
}

/// No edit gives the input back, byte for byte, in every shape a flat file takes.
#[test]
fn odg_package_an_unedited_save_of_a_flat_drawing_is_the_input() {
    let regular = three();
    let mut out_of_order = regular.clone();
    std::mem::swap(&mut out_of_order.styles, &mut out_of_order.auto);
    out_of_order.styles = out_of_order.styles.replace("office:automatic-styles", "office:styles");
    let mut compact = regular.clone();
    compact.gap = String::new();
    let mut bom = regular.bytes();
    bom.splice(0..0, [0xEF, 0xBB, 0xBF]);
    let mut crlf = regular.clone();
    crlf.gap = "\r\n  ".to_owned();
    let mut extension_first = regular.clone();
    extension_first.meta = format!("<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"/>{}", diacraft_doc::DIACRAFT_URN, extension_first.meta);
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("regular", regular.bytes()),
        ("compact", compact.bytes()),
        ("crlf", crlf.bytes()),
        ("a byte-order mark", bom),
        ("the extension first", extension_first.bytes()),
        ("UTF-16", utf16le(&regular.text().replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\""))),
    ];
    for (what, bytes) in files {
        let o = open_bytes(bytes.clone());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(!saved.edited, "{what}");
        assert_eq!(saved.bytes, bytes, "{what}: the input back");
        assert!(saved.notes.is_empty(), "{what}");
        second_save_is_no_edit(&o.doc, &saved);
    }
}

/// A shape moved 1 cm right: only its `svg:x` and `meta:generator`'s text differ; nothing is written beside the file, and the file stays flat.
#[test]
fn odg_package_a_move_changes_the_shape_and_the_generator_and_nothing_else() {
    let flat = three();
    let mut o = open_bytes(flat.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let (before, after) = (text_of(&o.bytes), text_of(&saved.bytes));
    assert!(after.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document "), "one XML document, no package");
    assert_eq!(generator(&before), "TestWriter/9$Linux");
    assert!(generator(&after).starts_with("DiaCraft/"), "{}", generator(&after));
    let expected =
        before.replacen("draw:name=\"s2\" svg:x=\"2cm\"", "draw:name=\"s2\" svg:x=\"3cm\"", 1).replacen("TestWriter/9$Linux", generator(&after), 1);
    assert_ne!(expected, before);
    assert_eq!(after, expected, "only svg:x and the generator's text differ");
    // The creation date and the title of the document's own meta are kept.
    assert!(after.contains("<dc:title>A title</dc:title><meta:creation-date>2024-05-06T07:08:09</meta:creation-date>"));
    // The file reads, as the model it was saved from, and a second save is no edit.
    let (again, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(again.pages.get(0).is_some_and(|p| p.shapes.len() == 3));
    second_save_is_no_edit(&o.doc, &saved);
    assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing));
}

/// No `office:meta`: one holding only the generator is the root's first child, with the meta namespace declared on it when the root does not.
#[test]
fn odg_package_a_missing_office_meta_is_written_as_the_first_child() {
    let mut flat = three();
    flat.meta = String::new();
    let mut o = open_bytes(flat.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (text_of(&o.bytes), text_of(&saved.bytes));
    let version = generator(&after).to_owned();
    assert!(version.starts_with("DiaCraft/"));
    let added = format!("<office:meta><meta:generator>{version}</meta:generator></office:meta>");
    let root_end = before.find("office:mimetype=\"").and_then(|i| before[i..].find('>').map(|j| i + j + 1)).expect("the root's end");
    assert_eq!(&after[root_end..root_end + added.len()], added, "the root's first child, with the prefix the root binds");
    assert_eq!(after.replacen(&added, "", 1), before.replacen("svg:x=\"2cm\"", "svg:x=\"3cm\"", 1), "nothing else is new but the move");
    second_save_is_no_edit(&o.doc, &saved);
    // The root does not declare the meta namespace: the new element declares it.
    let mut bare = flat.clone();
    bare.root = bare.root.replace(" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\"", "");
    assert!(!bare.root.contains("xmlns:meta"));
    let mut o = open_bytes(bare.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    assert!(
        after.contains(&format!(
            "<office:meta xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\"><meta:generator>{}</meta:generator></office:meta>",
            generator(&after)
        )),
        "{after}"
    );
    let (doc, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages.len(), 1);
    assert!(report.generator.as_deref().is_some_and(|g| g.starts_with("DiaCraft/")), "{:?}", report.generator);
}

/// A new automatic style is numbered over the one container (`gr5` after `gr1` to `gr4`) and goes last in it; a document with no
/// `office:automatic-styles` gets one, once, at its place in the order.
#[test]
fn odg_package_new_styles_go_into_the_one_container_at_its_place() {
    let gr = |n: u32| {
        format!(
            "<style:style style:name=\"gr{n}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\"/></style:style>"
        )
    };
    let mut d = Drawing::a4().page(&custom("s1", "1cm"));
    d.auto = format!("{}{}{}{}<style:style style:name=\"P1\" style:family=\"paragraph\"/>", gr(1), gr(2), gr(3), gr(4));
    let flat = Flat::of(&d);
    let mut o = open_bytes(flat.bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    // `gr5` and `P2`: at the end of `office:automatic-styles`, which is the one container.
    let (open, close) = (after.find("<office:automatic-styles>").unwrap(), after.find("</office:automatic-styles>").unwrap());
    let container = &after[open..close];
    assert!(container.contains("style:name=\"gr5\"") && container.contains("style:name=\"P2\""), "{container}");
    assert!(container.ends_with("/>") && container.rfind("style:name=\"P2\"").unwrap() > container.rfind("style:name=\"gr5\"").unwrap());
    assert_eq!(after.matches("<office:automatic-styles").count(), 1, "one container");
    assert!(container.find("style:name=\"P1\"").unwrap() < container.find("style:name=\"gr5\"").unwrap(), "after what the document has");
    second_save_is_no_edit(&o.doc, &saved);
    // No `office:automatic-styles`: made after `office:styles` and before `office:master-styles`.
    let mut none = Flat::of(&Drawing::a4().common("<style:style style:name=\"Standard\" style:family=\"graphic\"/>").page(&custom("s1", "1cm")));
    none.auto = String::new();
    let mut o = open_bytes(none.bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    assert_eq!(after.matches("<office:automatic-styles").count(), 1, "one, not one at the end of the file");
    let (styles, auto, masters, body) = (
        after.find("</office:styles>").unwrap(),
        after.find("<office:automatic-styles>").unwrap(),
        after.find("<office:master-styles>").unwrap(),
        after.find("<office:body>").unwrap(),
    );
    assert!(styles < auto && auto < masters && masters < body, "The order: styles, automatic styles, master styles, body");
    assert!(after.contains("style:name=\"gr1\"") && after.contains("style:name=\"P1\""));
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(2));
    second_save_is_no_edit(&o.doc, &saved);
    // Neither `office:styles` nor `office:master-styles`: before the body.
    let mut bare = none.clone();
    bare.styles = String::new();
    bare.masters = String::new();
    let mut o = open_bytes(bare.bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let after = text_of(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    assert_eq!(after.matches("<office:automatic-styles").count(), 1);
    assert!(after.find("<office:automatic-styles>").unwrap() < after.find("<office:body>").unwrap());
}

/// The writer's side: DiaCraft's extension is the last child of the root, after `office:body`, with no text content; one the document has is
/// replaced where it stands; a model with none takes it out; the prefix it needs is bound on the root.
#[test]
fn odg_package_diacraft_extension_is_written_last_and_replaced_where_it_stands() {
    let flat = three();
    let mut o = open_bytes(flat.bytes());
    o.doc.diacraft = Some(Arc::new(extension("1")));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let after = text_of(&saved.bytes);
    let element = format!("<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"><dc:snap n=\"1\"/></dc:Data>", diacraft_doc::DIACRAFT_URN);
    assert!(after.ends_with(&format!("</office:body>\n {element}\n</office:document>\n")), "the last child, after the body: {after}");
    assert!(!element.contains("> <") && !element.contains(">\n"), "no text content");
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(doc.diacraft, o.doc.diacraft, "what is written is what is read");
    second_save_is_no_edit(&o.doc, &saved);
    // The model changes it: replaced where it is; the rest is the same bytes.
    let mut again = Open { bytes: saved.bytes.clone(), doc: doc.clone(), index: saved.index.clone() };
    again.doc.diacraft = Some(Arc::new(extension("2")));
    let second = save(&again.doc, &again.bytes, &again.index).unwrap();
    assert_eq!(
        text_of(&second.bytes).replacen("snap n=\"2\"", "snap n=\"1\"", 1).replacen(
            &generator(&text_of(&second.bytes)).to_owned(),
            generator(&after),
            1
        ),
        after
    );
    // Taken out when the model has none.
    let mut none = Open { bytes: second.bytes.clone(), doc: again.doc.clone(), index: second.index.clone() };
    none.doc.diacraft = None;
    let third = save(&none.doc, &none.bytes, &none.index).unwrap();
    assert!(!text_of(&third.bytes).contains("dc:Data"), "{}", text_of(&third.bytes));
    assert!(read(&third.bytes, &Limits::default()).unwrap().0.diacraft.is_none());
    // It goes with the line break and the indent in front of it, the white space that was its own, and leaves the child before it as it was.
    assert!(text_of(&third.bytes).ends_with("</office:body>\n</office:document>\n"), "{}", text_of(&third.bytes));
    // Text that is no white space before it is not its own: it stays.
    let mut with_text = flat.clone();
    with_text.tail = format!("kept text<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"/>", diacraft_doc::DIACRAFT_URN);
    let mut o = open_bytes(with_text.bytes());
    assert!(o.doc.diacraft.is_some());
    o.doc.diacraft = None;
    let taken = text_of(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    assert!(!taken.contains("dc:Data") && taken.contains("\n kept text\n</office:document>"), "{taken}");
    // As the first child: replaced there, and not moved.
    let mut first = flat.clone();
    first.meta = format!("<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"><dc:snap n=\"0\"/></dc:Data>{}", diacraft_doc::DIACRAFT_URN, first.meta);
    let mut o = open_bytes(first.bytes());
    o.doc.diacraft = Some(Arc::new(extension("9")));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    assert!(after.contains("dc:snap n=\"9\"") && !after.contains("dc:snap n=\"0\""), "{after}");
    assert_eq!(after.matches("dc:Data xmlns").count(), 1);
    assert!(after.find("dc:Data").unwrap() < after.find("<office:meta>").unwrap(), "where it stood");
    // A prefix the document does not bind and the element does not declare: the root gets the declaration.
    let mut o = open_bytes(three().bytes());
    let mut el = extension("3");
    el.attrs = RawAttrs::from_pairs([("zz:version", "1")]);
    el.name = Arc::from("zz:Data");
    o.doc.diacraft = Some(Arc::new(el));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    assert!(after.contains(&format!("xmlns:zz=\"{}\"", diacraft_doc::DIACRAFT_URN)), "{after}");
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(doc.diacraft.is_some(), "the prefix is bound: the element is DiaCraft's");
    // An unedited file with the extension saves as it is, and the model alone, unchanged, writes nothing.
    let mut with = flat.clone();
    with.tail = format!("<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"/>", diacraft_doc::DIACRAFT_URN);
    let o = open_bytes(with.bytes());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(!saved.edited && saved.bytes == o.bytes);
}

/// The prefix an extension is written under must be bound where it is written: by the element itself (the root is then left as it is), by the root to DiaCraft's
/// namespace (nothing is added), by neither (the root gets the declaration), or by the root to another namespace (refused in words).
#[test]
fn odg_package_the_prefix_of_the_extension_is_bound_where_it_is_written() {
    let with = |root_extra: &str, element: RawElement| {
        let mut flat = three();
        flat.root = flat.root.replacen("<office:document ", &format!("<office:document {root_extra} "), 1);
        let mut o = open_bytes(flat.bytes());
        o.doc.diacraft = Some(Arc::new(element));
        save(&o.doc, &o.bytes, &o.index).map(|w| text_of(&w.bytes))
    };
    let urn = diacraft_doc::DIACRAFT_URN;
    let bare = |prefix: &str| RawElement {
        name: Arc::from(format!("{prefix}:Data")),
        attrs: RawAttrs::from_pairs([(format!("{prefix}:version"), "1".to_owned())]),
        children: Vec::new(),
    };
    // The root's start tag: from the start to the `>` after its media type.
    let root_of = |text: &str| {
        let at = text.find("office:mimetype").unwrap();
        text[..at + text[at..].find('>').unwrap()].to_owned()
    };
    // The element declares its own prefix: the root, which does not bind it, is left alone.
    let own = RawElement { name: Arc::from("ee:Data"), attrs: RawAttrs::from_pairs([("xmlns:ee", urn), ("ee:version", "1")]), children: Vec::new() };
    let text = with("", own).unwrap();
    assert!(!root_of(&text).contains("xmlns:ee"), "{}", root_of(&text));
    // A prefix the root binds to DiaCraft's namespace: nothing is added.
    let text = with(&format!("xmlns:zz=\"{urn}\""), bare("zz")).unwrap();
    assert_eq!(root_of(&text).matches("xmlns:zz").count(), 1, "{}", root_of(&text));
    // A prefix the root binds to another namespace: refused in words.
    let err = with("xmlns:zz=\"urn:other\"", bare("zz")).expect_err("the prefix means another namespace");
    assert_eq!(err.class(), "write_unsupported");
    assert!(err.to_string().contains("another namespace"), "{err}");
    // A prefix nothing binds: the root gets the declaration, once.
    let text = with("", bare("yy")).unwrap();
    assert_eq!(root_of(&text).matches(&format!("xmlns:yy=\"{urn}\"")).count(), 1, "{}", root_of(&text));
    // An element with no prefix at all needs none.
    let plain = RawElement { name: Arc::from("Data"), attrs: RawAttrs::from_pairs([("xmlns", urn)]), children: Vec::new() };
    let text = with("", plain).unwrap();
    assert!(text.contains(&format!("<Data xmlns=\"{urn}\"/>")), "{text}");
}

/// An index goes with the bytes of the save that gave it: the index of a save, used for other bytes, is refused in words, for a flat original and for a package
/// whose save is asked as a flat document.
#[test]
fn odg_package_an_index_goes_with_the_bytes_it_was_made_for() {
    let mut o = open_bytes(three().bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    // The saved bytes with their index: no fault. The first bytes with the second's index: refused.
    assert!(save(&o.doc, &saved.bytes, &saved.index).is_ok());
    let err = save(&o.doc, &o.bytes, &saved.index).expect_err("an index for other bytes");
    assert_eq!(err.class(), "write_mismatch");
    // The same for a package asked for as a flat document.
    let package = crate::build::package(crate::build::DRAWING, drawing_of(3).files());
    let mut p = open_bytes(package);
    move_by(&mut p.doc, 0, 2, 1.0, 0.0);
    let flat_options = WriteOptions { form: Some(diacraft_odg::Form::Flat), ..WriteOptions::default() };
    let first = write_with(&p.doc, Some(&OriginalPackage { bytes: &p.bytes, index: &p.index }), &flat_options).unwrap();
    assert!(first.edited);
    // A package saved as a package gives an index that goes with that package; the first package with it is another's.
    let saved_package = save(&p.doc, &p.bytes, &p.index).unwrap();
    assert!(save(&p.doc, &saved_package.bytes, &saved_package.index).is_ok());
    let other = write_with(&p.doc, Some(&OriginalPackage { bytes: &p.bytes, index: &saved_package.index }), &flat_options)
        .expect_err("an index for other bytes");
    assert_eq!(other.class(), "write_mismatch");
}

/// What a save carries to the next: the cost of the read, with what the model has grown by since it was read taken in (`grown / 64` items), as the package's save
/// does; and a new connector's line end makes its marker, in `office:styles`, named by the style.
#[test]
fn odg_package_what_the_model_grew_by_and_the_marker_of_a_line_end_go_to_the_document() {
    let mut o = open_bytes(three().bytes());
    let text = "a long text ".repeat(2_000);
    let shape = o.doc.pages.get(0).unwrap().shapes.get(0).map(|s| s.id).unwrap();
    crate::write_core::shape_mut(&mut o.doc, 0, shape).text =
        Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from(text.as_str()))], ..Default::default() }));
    let mut work = diacraft_odg::WriteWork::default();
    let saved = diacraft_odg::write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap();
    assert!(work.grown_bytes >= text.len() as u64, "the model grew by its text: {}", work.grown_bytes);
    assert_eq!(saved.index.read_cost().items, o.index.read_cost().items + work.grown_bytes / 64);
    // A line end: the marker is in `office:styles` (made by the second pass from what the first named), and the style names it.
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    let mut connector = crate::write_new::connector(id, (1.0, 1.0, 6.0, 2.0));
    for (name, v) in [("EndArrow", 13.0), ("EndArrowSize", 2.0), ("LineWeight", 0.02)] {
        crate::write_core::set_num_new(&mut connector, name, v);
    }
    push_shape(&mut o.doc, connector);
    let text = text_of(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let styles =
        text.split("<office:styles>").nth(1).and_then(|r| r.split("</office:styles>").next()).unwrap_or_else(|| panic!("no office:styles in {text}"));
    assert!(styles.contains("<draw:marker draw:name=\"DiaCraft_20_end_20_13\""), "{styles}");
    assert!(text.contains("draw:marker-end=\"DiaCraft_20_end_20_13\""), "the automatic style names it");
    // The document had no `office:styles`: it is made once, at its place in the order, before the automatic styles.
    assert_eq!(text.matches("<office:styles").count(), 1);
    assert!(text.find("<office:styles>").unwrap() < text.find("<office:automatic-styles>").unwrap());
}

/// R34 and the memory a save may hold: the document is the one part, so the gate counts the document twice over beside the models (the file, the part, the part
/// again as it is spliced, and the file again as it is made), where a package counts its biggest part: a cap one byte under that is refused before the document is
/// read, and the cap at it lets the save be.
#[test]
fn odg_package_a_flat_save_that_would_hold_more_memory_than_it_may_is_refused_before_the_document_is_read() {
    let mut o = open_bytes(three().bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let need = o.index.read_cost().save_bytes();
    let save_at = |memory: u64| {
        let mut work = diacraft_odg::WriteWork::default();
        let options = WriteOptions { limits: Limits { max_write_memory: memory, ..Limits::default() }, ..WriteOptions::default() };
        let saved = diacraft_odg::write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
        (saved, work)
    };
    let before_read = |memory: u64| {
        let (saved, work) = save_at(memory);
        let refused = matches!(&saved, Err(WriteError::Limit { what, max }) if what.contains("memory") && *max == memory);
        (refused && (work.baseline_read.items, work.baseline_read.elements, work.scan_bytes) == (0, 0, 0), saved.is_ok())
    };
    let (mut under, mut over) = (0u64, 3_000_000_000u64);
    assert!(before_read(under).0 && before_read(over).1, "a cap of nothing refuses and one of 3 GB lets a save of three shapes be");
    while over - under > 1 {
        let mid = under + (over - under) / 2;
        if before_read(mid).0 { under = mid } else { over = mid }
    }
    let least = over;
    assert!(before_read(least).1 && before_read(least - 1).0, "the save is made at the least cap and refused before the read one byte under it");
    // The reader's figures, and the document five times: the file, the part, the part again, and the file twice.
    let size = o.bytes.len() as u64;
    assert!(least >= need + 5 * size, "{least} against {need} and the document of {size} bytes five times");
}

/// A document with no `office:automatic-styles` and no `office:master-styles`, whose body declares a namespace of its own (the container a save makes goes before
/// the body, in front of a tag that declares): a move writes no style and is made; a restyle needs the container and is refused in words.
#[test]
fn odg_package_a_move_needs_no_container_of_styles_where_none_can_be_made() {
    let mut d = drawing_of(3);
    d.styles_auto = String::new();
    d.masters = String::new();
    let mut flat = Flat::of(&d);
    flat.body = flat.body.replacen("<office:body>", "<office:body xmlns:q=\"urn:q\">", 1);
    assert!(!flat.text().contains("automatic-styles") && !flat.text().contains("master-styles"));
    let mut o = open_bytes(flat.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a move needs no styles: {e}"));
    assert!(saved.edited && !text_of(&saved.bytes).contains("automatic-styles"));
    let mut o = open_bytes(flat.bytes());
    let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, 2).sheet);
    sheet.cells.insert(diacraft_doc::CellName::new("FillForegnd"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#336699")));
    let err = save(&o.doc, &o.bytes, &o.index).expect_err("a restyle needs a container of styles, and none can be made before this body");
    assert_eq!(err.class(), "write_unsupported", "{err}");
    assert!(err.to_string().contains("declares a namespace of its own"), "{err}");
}

/// A font the file does not declare is declared in `office:font-face-decls` of the one document: inside the element when it is there (empty or as an
/// empty element), and made at its place, before the first of the children R30 puts after it, when it is not.
#[test]
fn odg_package_a_font_the_document_does_not_declare_is_declared_at_the_place_of_the_element() {
    use diacraft_doc::{Cell, CellName, Row, Section, TextPart, Value};
    let declared = "<style:font-face style:name=\"Nowhere\" svg:font-family=\"'Nowhere'\"/>";
    let saved_with = |flat: &Flat| {
        let mut o = open_bytes(flat.bytes());
        let id = crate::write_runs::id_of(&o.doc);
        {
            let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
            let mut section = Section::new("Character", None);
            let mut row = Row { ix: Some(1), ..Row::default() };
            row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text("Nowhere")));
            section.rows.push(Arc::new(row));
            sheet.sections.push(Arc::new(section));
        }
        crate::write_runs::set_parts(&mut o, vec![TextPart::Cp(1), crate::write_runs::chars("face")]);
        Arc::make_mut(&mut o.doc.fonts).push(diacraft_doc::FaceName {
            name_u: Arc::from("Nowhere"),
            raw: Default::default(),
            ext: Default::default(),
        });
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
        assert!(saved.edited);
        second_save_is_no_edit(&o.doc, &saved);
        let after = text_of(&saved.bytes);
        assert_eq!(after.matches("style:name=\"Nowhere\" svg:font-family").count(), 1, "declared once: {after}");
        assert!(after.contains("style:font-name=\"Nowhere\""), "a text style names it: {after}");
        after
    };
    let d = Drawing::a4().page(&crate::write_runs::shape("x", "1cm", "<text:p>plain</text:p>"));
    // The element is there with no face, or as an empty element.
    for empty in ["<office:font-face-decls></office:font-face-decls>", "<office:font-face-decls/>"] {
        let mut flat = Flat::of(&d);
        flat.fonts = empty.to_owned();
        let after = saved_with(&flat);
        assert!(after.contains(&format!("<office:font-face-decls>{declared}</office:font-face-decls>")), "{empty}: {after}");
    }
    // Another face already there: the new one is added after it, in the same element.
    let mut flat = Flat::of(&d);
    flat.fonts = "<office:font-face-decls><style:font-face style:name=\"Old\" svg:font-family=\"Old\"/></office:font-face-decls>".to_owned();
    let after = saved_with(&flat);
    assert!(after.contains(&format!("<style:font-face style:name=\"Old\" svg:font-family=\"Old\"/>{declared}</office:font-face-decls>")), "{after}");
    // No element: made before the first child that follows it, `office:styles` when the document has one, else `office:automatic-styles`.
    let mut with_styles = d.clone();
    with_styles.common = r#"<style:style style:name="Standard" style:family="graphic"/>"#.to_owned();
    let after = saved_with(&Flat::of(&with_styles));
    assert!(after.contains(&format!("<office:font-face-decls>{declared}</office:font-face-decls><office:styles>")), "{after}");
    let after = saved_with(&Flat::of(&d));
    assert!(after.contains(&format!("<office:font-face-decls>{declared}</office:font-face-decls><office:automatic-styles>")), "{after}");
    assert!(after.find("<office:font-face-decls>").unwrap() < after.find("<office:automatic-styles>").unwrap());
}

/// The writer's side: children out of order stay where they are after an edit.
#[test]
fn odg_package_children_out_of_order_stay_where_they_are() {
    let mut d = drawing_of(3);
    d.common = "<style:style style:name=\"Standard\" style:family=\"graphic\"/>".to_owned();
    let regular = Flat::of(&d);
    let swapped = regular.text().replacen(
        &format!("{}{}{}{}", regular.gap, regular.styles, regular.gap, regular.auto),
        &format!("{}{}{}{}", regular.gap, regular.auto, regular.gap, regular.styles),
        1,
    );
    assert_ne!(swapped, regular.text());
    let mut o = open_bytes(swapped.clone().into_bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = text_of(&saved.bytes);
    assert!(after.find("<office:automatic-styles>").unwrap() < after.find("<office:styles>").unwrap(), "both still in their places");
    let meta_free = |text: &str| text.replace("<office:meta></office:meta>", "");
    assert_eq!(
        meta_free(&after).replacen("svg:x=\"3cm\"", "svg:x=\"2cm\"", 1).replacen(
            &format!("<office:meta><meta:generator>{}</meta:generator></office:meta>", generator(&after)),
            "",
            1
        ),
        swapped,
        "the move and the generator, nothing else"
    );
    second_save_is_no_edit(&o.doc, &saved);
}

/// The writer's side: a linked picture's href stays as written, an inline picture and an inline object keep their bytes, when the drawing is
/// edited.
#[test]
fn odg_package_pictures_and_objects_keep_their_bytes_through_an_edit() {
    let png = crate::flat::picture(120);
    let data = crate::flat::base64_lines(&png, 40, "\n    ");
    let inline =
        crate::flat::frame(&format!("<draw:image draw:mime-type=\"image/png\"><office:binary-data>{data}</office:binary-data></draw:image>"));
    let linked = "<draw:frame draw:name=\"link\" svg:x=\"9cm\" svg:y=\"2cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"Pictures/../pic%201.png\" xlink:type=\"simple\"/></draw:frame>";
    let both = "<draw:frame draw:name=\"both\" svg:x=\"13cm\" svg:y=\"2cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"x.png\"><office:binary-data>iVBORw0K</office:binary-data></draw:image></draw:frame>";
    let object = format!(
        "<draw:frame draw:name=\"obj\" svg:x=\"1cm\" svg:y=\"8cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:object><office:document office:mimetype=\"{MIME}\"><office:body><office:drawing><draw:page draw:name=\"inner\"/></office:drawing></office:body></office:document></draw:object><draw:image draw:mime-type=\"image/png\"><office:binary-data>{data}</office:binary-data></draw:image></draw:frame>"
    );
    let mut d = Drawing::a4().page(&format!("{inline}{linked}{both}{object}{}", custom("mover", "5cm")));
    d.meta = Some(full_meta());
    let flat = Flat::of(&d);
    let mut o = open_bytes(flat.bytes());
    let mover = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("mover")).map(|s| s.id).expect("the shape to move");
    move_by(&mut o.doc, 0, mover, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (text_of(&o.bytes), text_of(&saved.bytes));
    for kept in [&inline, linked, both, object.as_str()] {
        assert!(after.contains(kept), "kept as written: {kept}");
    }
    assert!(after.contains("Pictures/../pic%201.png"), "the href is as written, not rewritten for the folder");
    assert_eq!(
        after.replacen("svg:x=\"6cm\"", "svg:x=\"5cm\"", 1).replacen(generator(&after), generator(&before), 1),
        before,
        "the move and the generator"
    );
    second_save_is_no_edit(&o.doc, &saved);
}

/// The writer's side: a flat document in UTF-16 is read and saved as it is, and an edit of it is refused in words.
#[test]
fn odg_package_a_utf16_document_is_refused_an_edit_in_words() {
    let bytes = utf16le(&three().text().replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\""));
    let mut o = open_bytes(bytes.clone());
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.bytes, bytes);
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let err = save(&o.doc, &o.bytes, &o.index).expect_err("an edit of UTF-16 is refused");
    assert_eq!(err.class(), "write_unsupported");
    assert!(err.to_string().contains("UTF-16"), "{err}");
}

/// The writer asked for a kind.
fn save_as(doc: &diacraft_doc::Document, bytes: &[u8], index: &ShapeIndex, variant: Variant) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { variant: Some(variant), ..WriteOptions::default() })
}

/// The rule for a flat template: it reads as a template, saves as one, and saves as a drawing (and a drawing as a template) by changing the
/// root's media type and nothing else; asking for a kind always states it.
#[test]
fn odg_package_a_flat_template_is_saved_as_a_template_or_as_a_drawing() {
    let drawing = three();
    let mut template = drawing.clone();
    template.root = template.root.replace(&format!("office:mimetype=\"{MIME}\""), &format!("office:mimetype=\"{MIME}-template\""));
    assert_ne!(template.text(), drawing.text());
    let o = open_bytes(template.bytes());
    assert_eq!(read(&o.bytes, &Limits::default()).unwrap().1.variant, Some(Variant::Template));
    // Saved as the kind it is, asked or not: the bytes.
    for asked in [None, Some(Variant::Template)] {
        let saved = write_with(
            &o.doc,
            Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &WriteOptions { variant: asked, ..WriteOptions::default() },
        )
        .unwrap();
        assert_eq!((saved.edited, saved.bytes == o.bytes), (false, true), "{asked:?}");
    }
    // As a drawing: only the root's media type changes, and the drawing it gives is the drawing the template was made from, byte for byte.
    let saved = save_as(&o.doc, &o.bytes, &o.index, Variant::Drawing).unwrap();
    assert!(!saved.edited, "a change of kind alone is no edit");
    assert_eq!(text_of(&saved.bytes), drawing.text());
    assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing));
    second_save_is_no_edit_as(&o.doc, &saved, Variant::Drawing);
    // The other way.
    let o = open_bytes(drawing.bytes());
    let saved = save_as(&o.doc, &o.bytes, &o.index, Variant::Template).unwrap();
    assert_eq!(text_of(&saved.bytes), template.text());
    assert_eq!(sniff(&saved.bytes), Some(Variant::Template));
    // An edit and a change of kind in one save: the move, the generator and the kind, and a template stays a template through an edit.
    let mut o = open_bytes(template.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let as_template = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(sniff(&as_template.bytes), Some(Variant::Template));
    let as_drawing = save_as(&o.doc, &o.bytes, &o.index, Variant::Drawing).unwrap();
    assert!(as_drawing.edited && sniff(&as_drawing.bytes) == Some(Variant::Drawing));
    assert_eq!(
        text_of(&as_drawing.bytes).replace(&format!("office:mimetype=\"{MIME}\""), &format!("office:mimetype=\"{MIME}-template\"")),
        text_of(&as_template.bytes)
    );
    second_save_is_no_edit_as(&o.doc, &as_drawing, Variant::Drawing);
    // A root that says nothing, or says something this crate does not know, is given the kind asked for, even when the model is not edited.
    for (what, root) in [
        ("no media type", drawing.root.replace(&format!(" office:mimetype=\"{MIME}\""), "")),
        ("a flat-xml media type", drawing.root.replace(MIME, &format!("{MIME}-flat-xml"))),
    ] {
        let mut odd = drawing.clone();
        odd.root = root;
        let o = open_bytes(odd.bytes());
        for asked in [Variant::Drawing, Variant::Template] {
            let saved = save_as(&o.doc, &o.bytes, &o.index, asked).unwrap();
            assert!(!saved.edited, "{what}");
            assert_eq!(sniff(&saved.bytes), Some(asked), "{what}: asking states the kind");
            assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().1.warned("mimetype_irregular"), 0, "{what}");
        }
        // Not asked: the file as it is.
        assert_eq!(save(&o.doc, &o.bytes, &o.index).unwrap().bytes, o.bytes, "{what}");
    }
    // The attribute is written under a prefix the root binds to the office namespace: the root's own, else another it declares; with none, refused in words.
    let office = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
    let bare = |root: String, close: &str| {
        let mut flat = drawing.clone();
        flat.meta = String::new();
        let text = flat.text().replacen(&flat.root, &root, 1).replace("</office:document>", close);
        text.into_bytes()
    };
    let no_mime = drawing.root.replace(&format!(" office:mimetype=\"{MIME}\""), "");
    // The root's own prefix, `o`, with the media type left out.
    let own = bare(no_mime.replacen("<office:document", &format!("<o:document xmlns:o=\"{office}\""), 1), "</o:document>");
    let o = open_bytes(own);
    let saved = save_as(&o.doc, &o.bytes, &o.index, Variant::Template).unwrap();
    assert!(text_of(&saved.bytes).contains(&format!("o:mimetype=\"{MIME}-template\"")), "{}", text_of(&saved.bytes));
    assert_eq!(sniff(&saved.bytes), Some(Variant::Template));
    // The default namespace on the root and another prefix declared on it: written under that.
    let default = bare(no_mime.replacen("<office:document", &format!("<document xmlns=\"{office}\" xmlns:oo=\"{office}\""), 1), "</document>");
    let o = open_bytes(default);
    let saved = save_as(&o.doc, &o.bytes, &o.index, Variant::Template).unwrap();
    assert_eq!(sniff(&saved.bytes), Some(Variant::Template));
    // No prefix at all for the attribute: refused in words, and the model is not touched.
    let none = bare(format!("<document xmlns=\"{office}\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\">"), "</document>")
        .into_iter()
        .collect::<Vec<u8>>();
    let none = text_of(&none).replace("<office:", "<").replace("</office:", "</").into_bytes();
    if let Ok(o) = std::panic::catch_unwind(|| open_bytes(none)) {
        let err = save_as(&o.doc, &o.bytes, &o.index, Variant::Template).expect_err("no prefix to write the attribute under");
        assert_eq!(err.class(), "write_media_type");
    }
}

/// A second save through the index the first gave, asking the same kind, is no edit and the same bytes.
fn second_save_is_no_edit_as(doc: &diacraft_doc::Document, saved: &Written, variant: Variant) {
    let again = save_as(doc, &saved.bytes, &saved.index, variant).expect("the second save");
    assert!(!again.edited);
    assert_eq!(again.bytes, saved.bytes);
}

/// A root that cannot be read within the first 64 KiB cannot state a kind: refused in words, before the file is read again.
#[test]
fn odg_package_a_root_that_cannot_be_told_cannot_state_a_kind() {
    let bytes = utf16le(&three().text().replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\""));
    let o = open_bytes(bytes);
    let err = save_as(&o.doc, &o.bytes, &o.index, Variant::Template).expect_err("refused");
    assert_eq!(err.class(), "write_media_type");
    // Saved as nothing in particular, it is the bytes.
    assert_eq!(save(&o.doc, &o.bytes, &o.index).unwrap().bytes, o.bytes);
}

/// A new picture shape as the engine hands one to the writer: DiaCraft's rectangle, a foreign shape holding `bytes`.
pub fn picture_shape(id: u32, name: &str, frame: (f64, f64, f64, f64), bytes: &[u8]) -> diacraft_doc::Shape {
    let mut shape = rectangle(id, name, frame);
    shape.kind = diacraft_doc::ShapeType::Foreign;
    shape.foreign = Some(Arc::new(diacraft_doc::ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
        picture: Some(Arc::new(diacraft_doc::Media { part: Arc::from("pic.png"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })),
        ..diacraft_doc::ForeignData::default()
    }));
    shape
}

/// The writer's side: a 2 by 2 PNG added in DiaCraft is a `draw:image` with `draw:mime-type` and no `xlink:href`, holding `office:binary-data` whose
/// lines, separated by line feeds, are 72 characters long but the last; the shape reads back as the picture; a second save is no edit.
#[test]
fn odg_package_a_new_picture_is_written_inline() {
    let png = small_png();
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture_shape(id, "Picture.1", (2.0, 8.0, 4.0, 3.0), &png));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.notes.is_empty(), "the picture is written: nothing of it is told as left out: {:?}", saved.notes);
    let after = text_of(&saved.bytes);
    let frame = after.split("<draw:frame ").nth(1).and_then(|r| r.split("</draw:frame>").next()).expect("a frame");
    assert!(!frame.contains("xlink:href") && !frame.contains("Pictures/"), "no href: a flat document names no file: {frame}");
    assert!(frame.contains("draw:name=\"Picture.1\"") && frame.contains("svg:width=\"4cm\"") && frame.contains("svg:x=\"2cm\""), "{frame}");
    let image = frame.split("<draw:image ").nth(1).expect("an image");
    assert!(image.starts_with("draw:mime-type=\"image/png\"><office:binary-data>"), "{image}");
    let data = image.split("<office:binary-data>").nth(1).and_then(|r| r.split("</office:binary-data>").next()).unwrap();
    let lines: Vec<&str> = data.split('\n').collect();
    assert!(
        lines.len() >= 2 && lines.iter().rev().skip(1).all(|l| l.len() == 72) && lines.last().is_some_and(|l| !l.is_empty() && l.len() <= 72),
        "{lines:?}"
    );
    assert_eq!(data, crate::flat::base64_lines(&png, 72, "\n"));
    // It reads back as the picture, as a Bitmap.
    let (doc, report) = read(&saved.bytes, &Limits::default()).unwrap();
    let picture =
        doc.pages.get(0).unwrap().shapes.iter().find_map(|s| s.foreign.as_ref().filter(|f| f.picture.is_some())).expect("the picture reads back");
    assert_eq!((&picture.picture.as_ref().unwrap().bytes[..], picture.attr("ForeignType")), (&png[..], Some("Bitmap")));
    assert_eq!(report.warned("linked_picture_not_read"), 0);
    second_save_is_no_edit(&o.doc, &saved);
    // A picture in a format the reader does not draw, and one with text, are refused in words.
    let mut o = open_bytes(three().bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture_shape(id, "Picture.1", (2.0, 8.0, 4.0, 3.0), b"not a picture"));
    let err = save(&o.doc, &o.bytes, &o.index).expect_err("refused");
    assert_eq!(err.class(), "write_unsupported");
    // A package gets no inline picture from this save: it names a new entry (`write_picture.rs`), and nothing of the picture is told as left out.
    let package = crate::build::package(crate::build::DRAWING, Drawing::a4().page(&custom("s1", "1cm")).files());
    let mut o = open_bytes(package);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture_shape(id, "Picture.1", (2.0, 8.0, 4.0, 3.0), &png));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    assert!(!crate::write_core::part(&saved.bytes, "content.xml").contains("binary-data"), "a package holds no inline picture");
    assert_eq!(crate::write_core::part_bytes(&saved.bytes, "Pictures/image1.png"), png);
}

/// The text a flat save writes for one new picture shape made by `make` (with a new connector glued to it when `glued`), or the error that refuses it.
fn picture_saved(version: Option<&str>, glued: bool, make: &dyn Fn(&mut diacraft_doc::Shape)) -> Result<(String, Written), WriteError> {
    let mut d = drawing_of(3);
    if let Some(v) = version {
        d.version = v.to_owned();
    }
    let mut o = open_bytes(Flat::of(&d).bytes());
    let id = o.index.next_id(0).unwrap();
    let mut shape = picture_shape(id, "Picture.1", (1.5, 20.25, 2.0, 5.5), &small_png());
    make(&mut shape);
    push_shape(&mut o.doc, shape);
    if glued {
        push_shape(&mut o.doc, crate::write_new::connector(id + 1, (1.0, 1.0, 6.0, 2.0)));
        crate::write_new::glue(&mut o.doc, id + 1, true, id, 1);
    }
    let saved = save(&o.doc, &o.bytes, &o.index)?;
    Ok((text_of(&saved.bytes), saved))
}

/// A new picture's `draw:mime-type` is told by its own bytes, as the reader judges (PNG, JPEG, GIF, BMP and EMF are drawn; nothing else is written), and
/// its frame is where the model has it: `svg:y` counts from the top of the page.
#[test]
fn odg_package_a_new_picture_is_typed_by_its_bytes_and_placed_by_its_frame() {
    let padded = |head: &[u8], at: usize| {
        let mut bytes = vec![0u8; at];
        bytes.extend_from_slice(head);
        bytes.extend_from_slice(&[7u8; 24]);
        bytes
    };
    let drawn: [(&str, Vec<u8>); 5] = [
        ("image/png", padded(&[0x89, b'P', b'N', b'G'], 0)),
        ("image/jpeg", padded(&[0xFF, 0xD8], 0)),
        ("image/gif", padded(b"GIF8", 0)),
        ("image/bmp", padded(b"BM", 0)),
        ("image/emf", padded(b" EMF", 40)),
    ];
    for (media, bytes) in &drawn {
        let mut o = open_bytes(three().bytes());
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture_shape(id, "Picture.1", (1.5, 20.25, 2.0, 5.5), bytes));
        let text = text_of(&save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{media}: {e}")).bytes);
        assert!(text.contains(&format!("<draw:image draw:mime-type=\"{media}\"><office:binary-data>")), "{media}: {text}");
        // The frame: the rectangle's own place, the top of the page being the top.
        let frame = text.split("<draw:frame ").nth(1).and_then(|r| r.split(">").next()).expect("a frame");
        assert!(
            frame.contains("svg:width=\"2cm\"")
                && frame.contains("svg:height=\"5.5cm\"")
                && frame.contains("svg:x=\"1.5cm\"")
                && frame.contains("svg:y=\"20.25cm\""),
            "{media}: {frame}"
        );
    }
    // Near misses are no picture the reader draws: refused in words, and nothing is written.
    let near: [Vec<u8>; 7] =
        [Vec::new(), b"GIF".to_vec(), b"B".to_vec(), vec![0xFF, 0xD7, 1, 2], padded(b" EMF", 39), padded(b" EMF", 41), padded(b" EME", 40)];
    for bytes in &near {
        let mut o = open_bytes(three().bytes());
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture_shape(id, "Picture.1", (1.5, 20.25, 2.0, 5.5), bytes));
        let err = save(&o.doc, &o.bytes, &o.index).expect_err("not a picture the reader draws");
        assert_eq!(err.class(), "write_unsupported", "{bytes:?}");
        assert!(err.to_string().contains("format the reader does not draw") || err.to_string().contains("no picture"), "{err}");
    }
    // The identifier of a shape: `xml:id` where the ODF version has it, and `draw:id` always.
    // (a shape a connector is glued to is given one).
    let (text, _) = picture_saved(None, true, &|_| {}).unwrap();
    let frame = text.split("<draw:frame ").nth(1).and_then(|r| r.split(">").next()).expect("a frame");
    assert!(frame.contains(" xml:id=\"id1\"") && frame.contains(" draw:id=\"id1\""), "{frame}");
    let (text, _) = picture_saved(Some("1.1"), true, &|_| {}).unwrap();
    let frame = text.split("<draw:frame ").nth(1).and_then(|r| r.split(">").next()).expect("a frame");
    assert!(!frame.contains("xml:id") && frame.contains(" draw:id=\"id1\""), "ODF 1.1 has no xml:id: {frame}");
    let (text, _) = picture_saved(None, false, &|_| {}).unwrap();
    let frame = text.split("<draw:frame ").nth(1).and_then(|r| r.split(">").next()).expect("a frame");
    assert!(!frame.contains("xml:id") && !frame.contains("draw:id"), "no connector, no identifier: {frame}");
}

/// What a new picture holds beside its frame is refused in words (its text, a style sheet, children), carried as tier 2 (a layer, a master, a section of its own) or
/// told as left out (the foreign data beyond its type), each by itself.
#[test]
fn odg_package_a_new_picture_that_holds_more_than_a_frame_is_refused_or_told() {
    use diacraft_doc::{Cell, CellName, Section, Shape, SharedVec, Text, TextPart, Value};
    let refused = |what: &str, words: &str, make: &dyn Fn(&mut Shape)| {
        let err = picture_saved(None, false, make).expect_err(what);
        assert_eq!(err.class(), "write_unsupported", "{what}: {err}");
        assert!(err.to_string().contains(words), "{what}: {err}");
    };
    // The same pictures with nothing the save cannot write: saved, and nothing told.
    let (_, saved) =
        picture_saved(None, false, &|s| s.text = Some(Arc::new(Text { parts: Vec::new(), ..Default::default() }))).expect("an empty text is no text");
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    let (_, saved) =
        picture_saved(None, false, &|s| Arc::make_mut(&mut s.sheet).links.line = Some(0)).expect("a link to the default style sheet is none");
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    // Text, a link to a style sheet, a group's children: refused. A layer, an instance of a master, a shape of a master and a section of its own are tier 2 now, as a
    // custom shape's are, and are saved (`flat_tier2.rs`, `write_picture_more.rs`).
    refused("text", "text", &|s| s.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("x"))], ..Default::default() })));
    for (name, set) in [
        ("line", (|s: &mut Shape| Arc::make_mut(&mut s.sheet).links.line = Some(3)) as fn(&mut Shape)),
        ("fill", |s| Arc::make_mut(&mut s.sheet).links.fill = Some(3)),
        ("text", |s| Arc::make_mut(&mut s.sheet).links.text = Some(3)),
    ] {
        refused(&format!("a {name} link"), "style sheet", &set);
    }
    refused("children", "group", &|s| s.children = SharedVec::from_vec(vec![Arc::new(Shape::new(99))]));
    // What the frame cannot say is tier 2: saved, and nothing told as left out.
    for (what, make) in [
        (
            "a layer",
            (|s: &mut Shape| {
                Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text("0")));
            }) as fn(&mut Shape),
        ),
        ("a master", |s| s.master = Some(1)),
        ("a master's shape", |s| s.master_shape = Some(1)),
        ("a section", |s| Arc::make_mut(&mut s.sheet).sections.push(Arc::new(Section::new("User", None)))),
    ] {
        let (_, saved) = picture_saved(None, false, &make).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(saved.notes.is_empty(), "{what}: {:?}", saved.notes);
    }
    // What the foreign data holds beyond the picture's type is not written, and told, one part at a time.
    type Edit = Box<dyn Fn(&mut Shape)>;
    let foreign = |change: fn(&mut diacraft_doc::ForeignData)| -> Edit {
        Box::new(move |s: &mut Shape| {
            let mut data = (**s.foreign.as_ref().expect("foreign data")).clone();
            change(&mut data);
            s.foreign = Some(Arc::new(data));
        })
    };
    let extras: [(&str, Edit); 4] = [
        ("an attribute", foreign(|f| f.attrs.push("ObjectType", "OLE"))),
        ("text", foreign(|f| f.text = Some(Arc::from("t")))),
        ("a kept node", foreign(|f| f.passthrough.push(0, diacraft_doc::RawNode::Text(Arc::from("kept"))))),
        ("a relationship", foreign(|f| f.rel_id = Some(Arc::from("rId1")))),
    ];
    for (what, make) in &extras {
        let (_, saved) = picture_saved(None, false, make.as_ref()).unwrap_or_else(|e| panic!("{what}: {e}"));
        // The foreign data beyond the picture's type rides on the frame's own style, so nothing is left out and nothing is told.
        assert!(
            !saved.notes.iter().any(|n| matches!(n, diacraft_odg::SaveNote::NewShapeUnwritten { foreign: true, .. })),
            "{what} is no longer told as left out: {:?}",
            saved.notes
        );
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// The LibreOffice check: files written here, opened and converted by LibreOffice as a black box, and read back through the reader.

/// A real PNG of 2 by 2 pixels (RGB, one stored block), for a picture LibreOffice decodes.
pub fn small_png() -> Vec<u8> {
    use std::io::Write;
    let mut raw = Vec::new();
    for row in 0..2u8 {
        raw.push(0);
        for col in 0..2u8 {
            raw.extend([40 + 100 * row, 80 + 100 * col, 160]);
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).unwrap();
    let idat = z.finish().unwrap();
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        out.extend(crc.finalize().to_be_bytes());
        out
    };
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend(chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]));
    png.extend(chunk(b"IDAT", &idat));
    png.extend(chunk(b"IEND", &[]));
    png
}

/// `DIACRAFT_FLAT_DIR`: where the files are written (run with `--run-ignored only`).
#[test]
#[ignore = "writes the files the LibreOffice check opens"]
fn write_flat_files_for_libreoffice() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_FLAT_DIR").expect("DIACRAFT_FLAT_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    let text_shape = |name: &str, x: &str, word: &str| custom(name, x).replace("<text:p/>", &format!("<text:p>{word}</text:p>"));
    // A package with a picture, saved as a flat document.
    let picture =
        crate::flat::frame("<draw:image xlink:href=\"Pictures/a.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>");
    let mut d = Drawing::a4().page(&format!("{}{}{picture}", text_shape("s1", "1cm", "one"), text_shape("s2", "6cm", "two")));
    d.meta = Some(full_meta());
    d.files.push(("Pictures/a.png".to_owned(), small_png()));
    let package = crate::build::package(crate::build::DRAWING, d.files());
    let o = open_bytes(package);
    let flat = write_with(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { form: Some(diacraft_odg::Form::Flat), ..WriteOptions::default() },
    )
    .unwrap();
    put("a-package-saved-flat.fodg", &flat.bytes);
    // A flat drawing edited: a shape moved, a new rectangle.
    let mut b = Open { bytes: flat.bytes.clone(), doc: read(&flat.bytes, &Limits::default()).unwrap().0, index: flat.index.clone() };
    let first = b.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("s1")).map(|s| s.id).unwrap();
    move_by(&mut b.doc, 0, first, 2.0, 0.0);
    let edited = save(&b.doc, &b.bytes, &b.index).unwrap();
    put("b-edited-flat.fodg", &edited.bytes);
    let id = b.index.next_id(0).unwrap();
    push_shape(&mut b.doc, rectangle(id, "Rectangle.1", (2.0, 8.0, 4.0, 2.0)));
    let more = save(&b.doc, &b.bytes, &b.index).unwrap();
    put("c-new-shape.fodg", &more.bytes);
    // The media type: the right one, a wrong one, none, another kind, a template.
    let text = text_of(&more.bytes);
    put("d-wrong-media.fodg", text.replace(MIME, &format!("{MIME}-flat-xml")).as_bytes());
    put("e-no-media.fodg", text.replace(&format!(" office:mimetype=\"{MIME}\""), "").as_bytes());
    put("f-template.fodg", text.replace(MIME, &format!("{MIME}-template")).as_bytes());
    put("g-text-media.fodg", text.replace(MIME, "application/vnd.oasis.opendocument.text").as_bytes());
    // DiaCraft's extension, last.
    let mut with = Open { bytes: more.bytes.clone(), doc: b.doc.clone(), index: more.index.clone() };
    with.doc.diacraft = Some(Arc::new(extension("1")));
    put("h-extension.fodg", &save(&with.doc, &with.bytes, &with.index).unwrap().bytes);
    // No office:meta, then a move.
    let mut bare = three();
    bare.meta = String::new();
    let mut o = open_bytes(bare.bytes());
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    put("i-no-meta.fodg", &save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
}

/// `DIACRAFT_FLAT_DIR`: reads each `.odg` in `out/` that LibreOffice converted from those files, and prints what it holds.
#[test]
#[ignore = "reads the files LibreOffice converted"]
fn read_back_libreoffice_conversions() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_FLAT_DIR").expect("DIACRAFT_FLAT_DIR")).join("out");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    names.sort();
    for path in names {
        let bytes = std::fs::read(&path).unwrap();
        match read(&bytes, &Limits::default()) {
            Ok((doc, report)) => {
                let shapes: usize = doc.pages.iter().map(|p| p.shapes.len()).sum();
                let pictures =
                    doc.pages.iter().flat_map(|p| p.shapes.iter()).filter(|s| s.foreign.as_ref().is_some_and(|f| f.picture.is_some())).count();
                println!(
                    "{}: read, {:?} pages {} shapes {} pictures {} generator {:?} warnings {:?}",
                    path.file_name().unwrap().to_string_lossy(),
                    report.variant,
                    doc.pages.len(),
                    shapes,
                    pictures,
                    report.generator,
                    report.warning_counts
                );
            }
            Err(e) => println!("{}: not read: {} ({e})", path.file_name().unwrap().to_string_lossy(), e.class()),
        }
    }
}

/// `DIACRAFT_FLAT_DIR`: LibreOffice's own flat saves (`outflat/`, made by the check above from our files): each is read, saved unedited (its bytes), and saved
/// after a move of its first shape, and the result is read again; what each held is printed.
#[test]
#[ignore = "reads the flat files LibreOffice wrote"]
fn save_libreoffice_flat_output() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_FLAT_DIR").expect("DIACRAFT_FLAT_DIR")).join("outflat");
    let mut names: Vec<_> =
        std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "fodg")).collect();
    names.sort();
    for path in names {
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut o = open_bytes(bytes.clone());
        let shapes: usize = o.doc.pages.iter().map(|p| p.shapes.len()).sum();
        let unedited = save(&o.doc, &o.bytes, &o.index).unwrap();
        assert!(!unedited.edited && unedited.bytes == bytes, "{name}: the bytes back");
        let first = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).expect("a shape");
        move_by(&mut o.doc, 0, first, 1.0, 0.0);
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
        let (again, report) = read(&saved.bytes, &Limits::default()).unwrap();
        assert_eq!(again.pages.len(), o.doc.pages.len());

        second_save_is_no_edit(&o.doc, &saved);
        // LibreOffice lays its flat output out in lines: the lines that differ after an edit are the edit's.
        let (before, after) = (text_of(&bytes), text_of(&saved.bytes));
        let differing: Vec<(&str, &str)> = before.lines().zip(after.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(before.lines().count(), after.lines().count(), "{name}: no line added or taken out");
        println!(
            "{name}: {} bytes, {shapes} shapes, edited save {} bytes, {} of {} lines differ ({}), generator {:?}, warnings {:?}",
            bytes.len(),
            saved.bytes.len(),
            differing.len(),
            before.lines().count(),
            differing.iter().map(|(a, _)| a.trim().chars().take(40).collect::<String>()).collect::<Vec<_>>().join(" | "),
            report.generator,
            report.warning_counts
        );
    }
}
