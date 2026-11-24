//! Kept markup whose prefix the page does not bind, at the crate's own level (the engine's tests of the same names are the plain visit a person makes). The reader keeps what it kept
//! whole with the declarations a prefix needs that an element above it made on its own start tag, and the model writer writes it where the prefix then means what it did, or says in words
//! that it could not. Every read-back goes through the real reader, and the inputs are written by hand from the OASIS ODF standard (Part 3) and XML Namespaces 1.0.

use diacraft_doc::{Document, OdfRun, RawElement, RawNode, Shape};
use diacraft_odg::{Limits, OdgError, SaveNote, WriteOptions, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const ZZ: &str = "urn:example:keptns:zz";
const XLINK: &str = "http://www.w3.org/1999/xlink";

const AUTO: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"/></style:style>"#;

fn drawing(shapes: &str) -> Drawing {
    Drawing::a4().auto(AUTO).page(shapes)
}

/// The model of a drawing, as the reader makes it (ODF 1.4, the version the model writer writes).
fn read_back(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap_or_else(|e| panic!("the vector does not read: {e}")).0
}

/// The package the model writer makes of a model, and what it said.
fn save(doc: &Document) -> (Vec<u8>, Vec<SaveNote>) {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    (written.bytes, written.notes)
}

fn content(bytes: &[u8]) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap()
}

fn first_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap()
}

fn rect(own: &str, inner: &str) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"{own}>{inner}</draw:rect>"#)
}

/// The value of the attribute `name` of a kept element.
fn attr<'a>(el: &'a RawElement, name: &str) -> Option<&'a str> {
    el.attrs.get(name)
}

/// The kept child elements of a shape, by name.
fn kept_children(shape: &Shape) -> Vec<&RawElement> {
    shape
        .ext
        .get()
        .map(|e| e.odf.kept.kept.iter().filter_map(|p| if let RawNode::Element(el) = &p.node { Some(el) } else { None }).collect())
        .unwrap_or_default()
}

/// The runs of the first paragraph of the first shape's text that hold an element of the file.
fn run_elements(doc: &Document) -> Vec<RawElement> {
    let text = first_shape(doc).text.as_ref().unwrap();
    let odf = &text.ext.get().unwrap().odf;
    odf.paragraphs.iter().flat_map(|p| p.runs.iter()).filter_map(|r: &OdfRun| r.element.as_deref().cloned()).collect()
}

// ---------------------------------------------------------------------------------------------------------------- the reader

/// **A child a shape keeps whole carries the declaration of the prefix the shape made**: the shape's start tag declares `zz` and the child uses it, so the child declares it itself.
#[test]
fn a_child_a_shape_keeps_carries_the_declaration_the_shape_made() {
    let doc = read_back(&drawing(&rect(&format!(r#" xmlns:zz="{ZZ}""#), r#"<zz:extra zz:k="2"/><text:p>Hello</text:p>"#)));
    let children = kept_children(first_shape(&doc));
    assert_eq!(children.len(), 1, "{children:?}");
    assert_eq!(attr(children[0], "xmlns:zz"), Some(ZZ), "the child declares the prefix it uses: {:?}", children[0]);
    assert_eq!(attr(children[0], "zz:k"), Some("2"));
    assert_eq!(children[0].attrs.iter().next().unwrap().0, "xmlns:zz", "the declaration is the first of its attributes");
}

/// **An attribute a shape keeps carries the declaration beside it**, once for each prefix, and the shape's own declarations that no attribute uses are not kept.
#[test]
fn an_attribute_a_shape_keeps_carries_the_declaration_beside_it() {
    let own = format!(r#" xmlns:zz="{ZZ}" xmlns:unused="urn:example:keptns:unused" zz:one="1" zz:two="2""#);
    let doc = read_back(&drawing(&rect(&own, "<text:p>Hello</text:p>")));
    let raw = &first_shape(&doc).ext.get().unwrap().odf.kept.raw;
    assert_eq!(raw.get("xmlns:zz"), Some(ZZ));
    assert_eq!(
        raw.iter().filter(|(k, _)| k.starts_with("xmlns")).count(),
        1,
        "one declaration, for the one prefix used: {:?}",
        raw.iter().collect::<Vec<_>>()
    );
    assert_eq!(raw.get("zz:one"), Some("1"));
    assert_eq!(raw.get("zz:two"), Some("2"));
}

/// **The element a run of the text holds carries the declaration** of a prefix the paragraph, the shape or the page declared: each of the three is the same element after the read.
#[test]
fn the_element_of_a_run_carries_the_declaration_of_a_prefix_an_element_above_made() {
    let link = r#"<text:a lk:type="simple" lk:href="https://example.org/notes">the notes</text:a>"#;
    let alias = format!(r#" xmlns:lk="{XLINK}""#);
    let on_paragraph = drawing(&rect("", &format!("<text:p{alias}>See {link} now</text:p>")));
    let on_shape = drawing(&rect(&alias, &format!("<text:p>See {link} now</text:p>")));
    let mut on_page = drawing(&rect("", &format!("<text:p>See {link} now</text:p>")));
    on_page.pages = on_page.pages.replacen("<draw:page ", &format!("<draw:page{alias} "), 1);
    for (what, d) in [("the paragraph", on_paragraph), ("the shape", on_shape), ("the page", on_page)] {
        let elements = run_elements(&read_back(&d));
        let a = elements.iter().find(|e| &*e.name == "text:a").unwrap_or_else(|| panic!("{what}: the link is held: {elements:?}"));
        assert_eq!(attr(a, "xmlns:lk"), Some(XLINK), "{what}: the link declares the prefix it uses");
        assert_eq!(attr(a, "lk:href"), Some("https://example.org/notes"), "{what}");
    }
}

/// How many declarations the kept markup of the first shape holds: on its attributes, on the children it keeps and on the elements of its text.
fn declarations_kept(doc: &Document) -> usize {
    let shape = first_shape(doc);
    let on_elements: usize = kept_children(shape)
        .into_iter()
        .cloned()
        .chain(run_elements(doc))
        .map(|el| el.attrs.iter().filter(|(k, _)| k.starts_with("xmlns")).count())
        .sum();
    on_elements + shape.ext.get().map_or(0, |e| e.odf.kept.raw.iter().filter(|(k, _)| k.starts_with("xmlns")).count())
}

/// **What the model holds is not declared again**: a prefix the root binds, a prefix `office:drawing` binds (a container's declaration is a root of the model), and a shape that declares what
/// the root declares, all keep the element as the file had it.
#[test]
fn a_prefix_the_model_holds_is_not_declared_on_the_element() {
    let shape = rect(r#" zz:flag="1""#, r#"<zz:extra zz:k="2"/><text:p>Hello <zz:note>big</zz:note></text:p>"#);
    // The root binds it.
    let mut by_root = drawing(&shape);
    by_root.ns = format!(r#"{} xmlns:zz="{ZZ}""#, by_root.ns);
    assert_eq!(declarations_kept(&read_back(&by_root)), 0, "the root's declaration");
    // The container that holds the pages binds it.
    let mut by_container = drawing(&shape);
    by_container.version = "1.4".to_owned();
    let content = by_container.content_xml().replace("<office:drawing>", &format!(r#"<office:drawing xmlns:zz="{ZZ}">"#));
    let mut files = by_container.files();
    files.retain(|(name, _)| name != "content.xml");
    files.insert(0, ("content.xml".to_owned(), content.into_bytes()));
    let doc = read(&package(DRAWING, files), &Limits::default()).unwrap().0;
    assert_eq!(declarations_kept(&doc), 0, "the container's declaration");
    // A shape that declares what the root declares.
    let same = format!(r#" xmlns:xlink="{XLINK}" xlink:role="r""#);
    let doc = read_back(&drawing(&rect(&same, r#"<text:p>See <text:a xlink:type="simple" xlink:href="https://example.org/x">x</text:a></text:p>"#)));
    assert_eq!(declarations_kept(&doc), 0, "a declaration of what the root binds");
}

/// A flat drawing: the one root is the model's, and what a container below it declares is local.
#[test]
fn in_a_flat_drawing_the_roots_declarations_are_the_models_and_the_containers_are_local() {
    let flat = crate::flat::Flat::of(&drawing(&rect(r#" zz:flag="1""#, r#"<zz:extra/><text:p>Hello</text:p>"#))).text();
    let by_container = flat.replace("<office:drawing>", &format!(r#"<office:drawing xmlns:zz="{ZZ}">"#));
    let doc = read(by_container.as_bytes(), &Limits::default()).unwrap().0;
    let shape = first_shape(&doc);
    assert_eq!(
        shape.ext.get().unwrap().odf.kept.raw.get("xmlns:zz"),
        Some(ZZ),
        "the container's declaration is not held, so the attribute carries it"
    );
    assert_eq!(attr(kept_children(shape)[0], "xmlns:zz"), Some(ZZ));
    let by_root = flat.replace("<office:document ", &format!(r#"<office:document xmlns:zz="{ZZ}" "#));
    let doc = read(by_root.as_bytes(), &Limits::default()).unwrap().0;
    let shape = first_shape(&doc);
    assert_eq!(shape.ext.get().unwrap().odf.kept.raw.get("xmlns:zz"), None, "the root's declaration is held");
    assert_eq!(attr(kept_children(shape)[0], "xmlns:zz"), None);
}

// ---------------------------------------------------------------------------------------------------------------- the writer

/// **The model writer writes each of them back bound to the namespace it was read in**, and its note is empty.
#[test]
fn the_model_writer_writes_the_markup_with_its_declarations_and_says_nothing() {
    let doc = read_back(&drawing(&rect(
        &format!(r#" xmlns:zz="{ZZ}" zz:flag="1""#),
        &format!(
            r#"<zz:extra zz:k="2"/><text:p xmlns:lk="{XLINK}">See <text:a lk:type="simple" lk:href="https://example.org/a">a</text:a> and <zz:note zz:k="3">b</zz:note></text:p>"#
        ),
    )));
    let (bytes, notes) = save(&doc);
    let content = content(&bytes);
    assert!(notes.is_empty(), "nothing is left out: {notes:?}");
    for wanted in [r#"zz:flag="1""#, r#"<zz:extra xmlns:zz=""#, r#"<text:a xmlns:lk=""#, r#"<zz:note xmlns:zz=""#] {
        assert!(content.contains(wanted), "{wanted}: {content}");
    }
    // And the file reads back as the model it was written from: the same markup, held again.
    let again = read(&bytes, &Limits::default()).unwrap().0;
    assert_eq!(kept_children(first_shape(&again)).len(), 1);
    assert_eq!(first_shape(&again).ext.get().unwrap().odf.kept.raw.get("xmlns:zz"), Some(ZZ));
    assert_eq!(run_elements(&again).iter().filter(|e| &*e.name == "zz:note").count(), 1);
}

/// **What nothing binds is said, in a note of its own for the text**: an element of the text, a paragraph and a span whose prefix is bound nowhere, and the shape's attribute and child (the
/// note the shapes have always had). The count is of the elements, the class is `text_markup_unbound`.
#[test]
fn markup_whose_prefix_nothing_binds_is_said_by_a_note_for_the_text_and_one_for_the_shape() {
    let inner = concat!(
        r#"<qq:extra/><text:p qq:p="1">Hello <qq:note>big</qq:note> <text:span text:style-name="T1" qq:s="2">span</text:span>"#,
        r#" <text:a qq:href="https://example.org/q">link</text:a><text:bookmark text:name="m" qq:k="3"/></text:p>"#
    );
    let doc = read_back(&drawing(&rect(r#" qq:flag="1""#, inner)));
    let (bytes, notes) = save(&doc);
    let unbound: Vec<&SaveNote> = notes.iter().filter(|n| n.class() == "text_markup_unbound").collect();
    assert_eq!(unbound.len(), 1, "one note for the shape's text: {notes:?}");
    // The paragraph, the element, the span, the link and the mark.
    assert!(unbound[0].to_string().contains("5 elements of the text of shape"), "{}", unbound[0]);
    assert!(notes.iter().any(|n| n.class() == "new_shape_unwritten"), "the shape's attribute and child are said too: {notes:?}");
    assert!(!content(&bytes).contains("qq:"), "nothing is written with a prefix nothing binds: {}", content(&bytes));
}

/// A drawing with no text that is unbound says nothing of the kind.
#[test]
fn a_text_with_nothing_unbound_has_no_such_note() {
    let doc = read_back(&drawing(&rect("", r#"<text:p>Hello <text:a xlink:type="simple" xlink:href="https://example.org/a">a</text:a></text:p>"#)));
    let (_, notes) = save(&doc);
    assert!(notes.iter().all(|n| n.class() != "text_markup_unbound"), "{notes:?}");
}

// ---------------------------------------------------------------------------------------------------------------- hostile input

/// **The declarations the reader adds are charged**: a paragraph that declares a prefix with a long address and holds many elements that use it costs the items of each declaration, so a
/// file whose declarations would be repeated past what a document may hold is refused with the document's own limit and not read.
#[test]
fn the_declarations_the_reader_adds_are_charged_as_items() {
    let address = format!("urn:example:keptns:{}", "a".repeat(20_000));
    let notes: String = (0..400).map(|k| format!(r#"<zz:note zz:k="{k}"/>"#)).collect();
    let with = drawing(&rect("", &format!(r#"<text:p xmlns:zz="{address}">{notes}</text:p>"#)));
    let without = drawing(&rect("", &format!(r#"<text:p>{}</text:p>"#, (0..400).map(|k| format!(r#"<note k="{k}"/>"#)).collect::<String>())));
    let mut with = with;
    with.version = "1.4".to_owned();
    let bytes = package(DRAWING, with.files());
    let (_, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the vector does not read: {e}"));
    let plain = read(&package(DRAWING, without.files()), &Limits::default()).unwrap().1;
    // 400 declarations of 20,000 bytes each are 8 MB: far more items than the elements alone come to.
    assert!(
        report.work.items > plain.work.items.saturating_add(100_000),
        "the declarations are charged: {} items beside {}",
        report.work.items,
        plain.work.items
    );
    // A document that may hold fewer items than that does not hold the declarations.
    let tight = Limits { max_items: usize::try_from(report.work.items / 2).unwrap(), ..Limits::default() };
    match read(&bytes, &tight) {
        Err(OdgError::Limit { .. }) => {}
        other => panic!("expected the item limit, got {:?}", other.map(|_| "a model")),
    }
}

// ---------------------------------------------------------------------------------------------------------------- the styles

/// **The properties of a kept automatic style carry the declaration of the prefix of an attribute no cell reads**: the properties element declared it on its own start tag, which the model does
/// not hold, and the model writer writes the attribute again under it.
#[test]
fn the_properties_of_a_kept_style_carry_the_declaration_of_their_attribute() {
    let styles = format!(
        r#"<style:style style:name="T9" style:family="text"><style:text-properties fo:font-size="11pt" xmlns:zz="{ZZ}" zz:keep="1"/></style:style>"#
    );
    let doc = read_back(&Drawing::a4().auto(&styles).page(&rect("", r#"<text:p>Hello <text:span text:style-name="T9">big</text:span></text:p>"#)));
    let odf = &doc.ext.get().unwrap().odf;
    let kept = odf.styles.iter().find(|k| &*k.name == "T9" && &*k.family == "auto:text").expect("the style is kept");
    let props = kept.element.children.iter().find_map(|c| if let RawNode::Element(e) = c { Some(e) } else { None }).expect("its properties");
    assert_eq!(attr(props, "xmlns:zz"), Some(ZZ), "{props:?}");
    assert_eq!(attr(props, "zz:keep"), Some("1"));
    let (bytes, notes) = save(&doc);
    assert!(notes.iter().all(|n| n.class() != "style_attributes_not_written"), "{notes:?}");
    let content = content(&bytes);
    assert!(content.contains(r#"xmlns:zz="urn:example:keptns:zz""#) && content.contains(r#"zz:keep="1""#), "{content}");
}

// ---------------------------------------------------------------------------------------------------------------- an earlier version's attribute

const DIA: &str = "urn:app.diacraft:extension";

/// The properties of the kept automatic paragraph style `P7` of the drawing whose automatic styles are `styles`, as the reader keeps them (`None` when it keeps none).
fn kept_paragraph_properties(styles: &str) -> Option<RawElement> {
    let doc = read_back(&Drawing::a4().auto(styles).page(&rect("", r#"<text:p text:style-name="P7">Hello</text:p>"#)));
    let odf = &doc.ext.get()?.odf;
    let kept = odf.styles.iter().find(|k| &*k.name == "P7" && &*k.family == "auto:paragraph")?;
    kept.element.children.iter().find_map(|c| if let RawNode::Element(e) = c { Some(e.clone()) } else { None })
}

/// **An attribute of an earlier version that the model writer carried in an attribute of DiaCraft's is read back as the attribute it stood for**, so that the style the second read gives is the
/// one the first gave; the model writer then carries it again, and nothing but the carrier is written (the file is the same bytes).
#[test]
fn a_carried_attribute_is_read_back_as_the_attribute_it_stood_for_and_carried_again() {
    let styles = format!(
        r#"<style:style style:name="P7" style:family="paragraph"><style:paragraph-properties fo:text-align="center" xmlns:diacraft="{DIA}" diacraft:older-enable-numbering="false"/></style:style>"#
    );
    let props = kept_paragraph_properties(&styles).expect("the style is kept");
    assert_eq!(attr(&props, "text:enable-numbering"), Some("false"), "{props:?}");
    assert_eq!(attr(&props, "diacraft:older-enable-numbering"), None, "the carrier is no attribute of the style: {props:?}");
    let doc = read_back(&Drawing::a4().auto(&styles).page(&rect("", r#"<text:p text:style-name="P7">Hello</text:p>"#)));
    let (bytes, notes) = save(&doc);
    let written = content(&bytes);
    assert!(notes.iter().all(|n| n.class() != "style_attributes_not_written"), "{notes:?}");
    assert!(!written.contains("text:enable-numbering"), "the version written has no such attribute there: {written}");
    assert!(written.contains(r#"diacraft:older-enable-numbering="false""#), "{written}");
    let again = read(&bytes, &Limits::default()).unwrap().0;
    assert_eq!(save(&again).0, bytes, "a second write is the same bytes");
}

/// **What is no carrier is no carrier**: the same attribute on text properties, another name, and a `text` prefix that is bound to another namespace are not read as `text:enable-numbering`; and the
/// attribute written as it is in the file, beside a carrier of the same name, is one attribute.
#[test]
fn an_attribute_of_diacraft_that_is_no_carrier_is_not_read_as_one() {
    let styles = format!(
        r#"<style:style style:name="P7" style:family="paragraph"><style:paragraph-properties xmlns:diacraft="{DIA}" diacraft:older-other="1" diacraft:older-enable-numbering-not="2"/><style:text-properties diacraft:older-enable-numbering="3" xmlns:diacraft="{DIA}"/></style:style>"#
    );
    let props = kept_paragraph_properties(&styles);
    assert!(props.as_ref().is_none_or(|p| attr(p, "text:enable-numbering").is_none()), "{props:?}");
    // The `text` prefix bound to another namespace on the element: the carrier stands for the attribute of the standard's namespace, which that prefix does not name there.
    let rebound = format!(
        r#"<style:style style:name="P7" style:family="paragraph"><style:paragraph-properties xmlns:text="urn:example:other" xmlns:diacraft="{DIA}" diacraft:older-enable-numbering="false"/></style:style>"#
    );
    let props = kept_paragraph_properties(&rebound);
    assert!(props.as_ref().is_none_or(|p| attr(p, "text:enable-numbering").is_none()), "{props:?}");
    // Both the attribute and its carrier: one attribute, read once.
    let both = format!(
        r#"<style:style style:name="P7" style:family="paragraph"><style:paragraph-properties fo:text-align="center" text:enable-numbering="true" xmlns:diacraft="{DIA}" diacraft:older-enable-numbering="false"/></style:style>"#
    );
    let doc = read_back(&Drawing::a4().auto(&both).page(&rect("", r#"<text:p text:style-name="P7">Hello</text:p>"#)));
    let (bytes, _) = save(&doc);
    let written = content(&bytes);
    assert_eq!(written.matches("older-enable-numbering=").count(), 1, "{written}");
}
