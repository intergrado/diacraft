//! The writer holds a part to the caps of its read-back while it makes the part, and **what it writes of a small drawing is what it wrote before**: a drawing
//! that keeps unknown XML on a shape, a sheet, a page, a master, a style and a picture is written to the same bytes as by the writer without the caps (the hash below is of the
//! bytes that the writer of main made of this very drawing).
use std::sync::Arc;

use diacraft_doc::{Document, Master, Page, Passthrough, Placed, RawAttrs, RawElement, RawNode, Shape, ShapeList, Style};

fn kept(tag: &str, children: usize) -> Passthrough {
    let leaf = |i: usize| {
        RawNode::Element(RawElement {
            name: Arc::from("x:c"),
            attrs: RawAttrs::from_pairs([("a", "1"), ("b", format!("{tag}{i}").as_str())]),
            children: Vec::new(),
        })
    };
    let tree = RawNode::Element(RawElement {
        name: Arc::from("x:k"),
        attrs: RawAttrs::from_pairs([("xmlns:x", "urn:kept"), ("tag", tag)]),
        children: (0..children).map(leaf).chain(std::iter::once(RawNode::Text(Arc::from("kept text")))).collect(),
    });
    Passthrough::from_nodes(vec![Placed { after: 0, node: tree }, Placed { after: 1, node: RawNode::Comment(Arc::from(" kept ")) }])
}

/// FNV-1a, 64 bits.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

fn drawing() -> Document {
    let mut doc = Document::new_blank();
    let mut page = Page::new(1, "P");
    page.passthrough = kept("page", 3);
    let mut shapes = Vec::new();
    for id in 1..=4u32 {
        let mut shape = Shape::new(id);
        shape.passthrough = kept(&format!("shape{id}"), 2);
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.passthrough = kept(&format!("sheet{id}"), 1);
        shapes.push(Arc::new(shape));
    }
    page.shapes = ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    doc.masters.push(Arc::new(Master { id: 1, passthrough: kept("master", 2), ..Master::default() }));
    doc.styles.push(Arc::new(Style { id: 7, passthrough: kept("style", 2), ..Style::default() }));
    doc
}

#[test]
fn a_small_drawing_that_keeps_unknown_xml_is_written_to_the_bytes_the_writer_without_the_caps_wrote() {
    let bytes = diacraft_vsdx::write(&drawing(), None).expect("a small drawing is written");
    let (a, b) = (fnv(&bytes), fnv(&diacraft_vsdx::write(&drawing(), None).expect("again")));
    assert_eq!(a, b, "a write is deterministic");
    println!("HASH {a:016x} of {} bytes", bytes.len());
    assert_eq!(a, EXPECTED, "the bytes are not what the writer of main made of this drawing");
}

/// The FNV-1a of the bytes that the writer of main (f57f9b9e) wrote of `drawing()`.
const EXPECTED: u64 = 0x1bf9679a5fa954ff;
