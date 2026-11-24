//! The security review of `ext/body.rs` and `ext/docfile.rs` (MEDIUM): every string a document file puts into the model is charged to the reader's
//! byte budget before it is copied, the same point the `data1` branch charges it.
//!
//! Each case is a document file whose bytes are almost all in one branch: a field's text, a comment, an instruction, a note, a connection's cell names, a
//! colour, a page's name, a subtree kept whole, and so on. It is read with an item budget that the charge for the file's size (the backstop under the rest)
//! passes, and that the charge for the strings, one by one, does not. Each fails on the reader that charged some strings and not others.

use std::sync::Arc;

use diacraft_doc::{
    CellName, ColorEntry, Connect, Connects, Document, ForeignData, Master, Media, RawAttrs, RawElement, RawNode, Shape, ShapeList, ShapeType, Text,
    TextPart,
};
use diacraft_odg::{Limits, ReadReport, read};

use crate::drawing::{Drawing, as_text};
use crate::write_core::{custom, open, real};

/// A string of a megabyte less than the most one attribute holds, which no cap of the XML parser refuses.
const BIG_ATTR: usize = 1_000_000;
/// A text node of eight megabytes.
const BIG_TEXT: usize = 8 << 20;
/// An item budget the file's size alone costs less than (eight mebibytes and what holds them, at 64 bytes an item, is a little over 131,072), and the same
/// eight megabytes of strings charged over again costs more than (262,144).
const BUDGET: usize = 150_000;

fn filler(n: usize) -> String {
    "a".repeat(n)
}

/// A master of one shape, which `shape` fills in.
fn master_of(shape: impl Fn(&mut Shape)) -> Master {
    let mut s = Shape::new(1);
    s.kind = ShapeType::Shape;
    shape(&mut s);
    Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }
}

fn saved(edit: impl Fn(&mut Document)) -> Vec<u8> {
    saved_in(&Drawing::a4().page(&custom("s1", "1cm")), edit)
}

fn saved_in(drawing: &Drawing, edit: impl Fn(&mut Document)) -> Vec<u8> {
    let mut o = open(drawing);
    edit(&mut o.doc);
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).bytes
}

fn read_within(bytes: &[u8], max_items: usize) -> Result<(Document, ReadReport), diacraft_odg::OdgError> {
    read(bytes, &Limits { max_items, ..Limits::default() })
}

/// The document file `edit` makes is read under the default limits, and refused under [`BUDGET`] items as too many.
#[track_caller]
fn charged(branch: &str, edit: impl Fn(&mut Document)) {
    charged_in(branch, &Drawing::a4().page(&custom("s1", "1cm")), edit);
}

#[track_caller]
fn charged_in(branch: &str, drawing: &Drawing, edit: impl Fn(&mut Document)) {
    let bytes = saved_in(drawing, edit);
    read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{branch}: the file does not read under the default limits: {e}"));
    match read_within(&bytes, BUDGET) {
        Err(e) => assert_eq!(e.class(), "too_many_items", "{branch}: {e}"),
        Ok(_) => panic!(
            "{branch}: strings of about {BIG_TEXT} bytes went into the model for less than {BUDGET} items: they are not charged by their bytes"
        ),
    }
}

fn with_shape(shape: impl Fn(&mut Shape) + 'static) -> impl Fn(&mut Document) {
    move |doc| doc.masters.push(Arc::new(master_of(&shape)))
}

#[test]
fn the_text_of_a_note_a_field_and_a_text_node_are_charged_by_their_bytes() {
    charged(
        "a picture's note",
        with_shape(|s| s.foreign = Some(Arc::new(ForeignData { text: Some(Arc::from(filler(BIG_TEXT))), ..ForeignData::default() }))),
    );
    charged(
        "a field's shown text",
        with_shape(|s| {
            s.text = Some(Arc::new(Text { parts: vec![TextPart::Fld { ix: 0, text: Arc::from(filler(BIG_TEXT)) }], ..Default::default() }))
        }),
    );
    charged("a kept text node", with_shape(|s| s.passthrough.push(0, RawNode::Text(Arc::from(filler(BIG_TEXT))))));
}

#[test]
fn a_comment_and_an_instruction_are_charged_by_their_bytes_in_a_text_and_among_the_kept_nodes() {
    charged(
        "a comment in a text",
        with_shape(|s| {
            s.text = Some(Arc::new(Text { parts: vec![TextPart::Raw(RawNode::Comment(Arc::from(filler(BIG_TEXT))))], ..Default::default() }))
        }),
    );
    charged("a comment among the kept nodes", with_shape(|s| s.passthrough.push(0, RawNode::Comment(Arc::from(filler(BIG_TEXT))))));
    let pi = |data: String| RawNode::Pi { target: Arc::from("t"), data: Arc::from(data) };
    charged(
        "an instruction in a text",
        with_shape(move |s| s.text = Some(Arc::new(Text { parts: vec![TextPart::Raw(pi(filler(BIG_TEXT)))], ..Default::default() }))),
    );
    charged("an instruction among the kept nodes", with_shape(move |s| s.passthrough.push(0, pi(filler(BIG_TEXT)))));
}

#[test]
fn the_names_a_connection_holds_and_the_values_of_a_colour_are_charged_by_their_bytes() {
    // Attribute values are at most a megabyte each: eight of them, in the branch, are the eight megabytes.
    charged("a connection's cell names", |doc| {
        let rows: Vec<Connect> = (0..4)
            .map(|k| Connect {
                from_sheet: 1,
                to_sheet: 2,
                from_cell: Some(Arc::from(filler(BIG_ATTR + k))),
                to_cell: Some(Arc::from(filler(BIG_ATTR + k + 10))),
                ..Connect::default()
            })
            .collect();
        let mut m = master_of(|_| {});
        m.connects = Connects::from_vec(rows);
        doc.masters.push(Arc::new(m));
    });
    charged("the values of colours", |doc| {
        let colors: Vec<ColorEntry> = (0..8).map(|ix| ColorEntry { ix, rgb: Arc::from(filler(BIG_ATTR)) }).collect();
        doc.colors = Arc::new(colors);
    });
}

#[test]
fn the_names_a_master_a_shape_a_page_a_picture_and_a_kind_hold_are_charged_by_their_bytes() {
    charged("a shape's kind", |doc| {
        let members: Vec<Arc<Shape>> = (0..8)
            .map(|k| {
                let mut s = Shape::new(k + 1);
                s.kind = ShapeType::Other(Arc::from(filler(BIG_ATTR)));
                Arc::new(s)
            })
            .collect();
        doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(members), ..Master::default() }));
    });
    charged("a master's names", |doc| {
        for k in 0..4 {
            doc.masters.push(Arc::new(Master {
                id: k,
                name: Some(Arc::from(filler(BIG_ATTR))),
                name_u: Some(Arc::from(filler(BIG_ATTR + 1))),
                ..Master::default()
            }));
        }
    });
    charged("a picture's part name and media type", |doc| {
        let members: Vec<Arc<Shape>> = (0..4)
            .map(|k| {
                let mut s = Shape::new(k + 1);
                s.kind = ShapeType::Foreign;
                let media = Media { part: Arc::from(filler(BIG_ATTR)), content_type: Arc::from(filler(BIG_ATTR + 1)), bytes: Arc::from(vec![1u8]) };
                s.foreign =
                    Some(Arc::new(ForeignData { picture: Some(Arc::new(media)), rel_id: Some(Arc::from(filler(10))), ..ForeignData::default() }));
                Arc::new(s)
            })
            .collect();
        doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(members), ..Master::default() }));
    });
    // Eight pages, each given a `NameU` of a megabyte.
    let mut pages = Drawing::a4();
    for _ in 0..8 {
        pages = pages.page(&custom("s1", "1cm"));
    }
    charged_in("a page's NameU", &pages, |doc| {
        for at in 0..doc.pages.len() {
            if let Some(page) = doc.pages.get_mut(at) {
                Arc::make_mut(page).name_u = Some(Arc::from(filler(BIG_ATTR)));
            }
        }
    });
}

/// The sum of the charges: a document file that holds a string of each kind, each of a known length, is charged for the strings one by one, and the items it
/// takes are at least the items those bytes stand for and at most those and the cost of the elements that hold them.
#[test]
fn the_charges_for_the_strings_of_a_document_file_add_up_to_its_bytes_and_the_cost_of_its_elements() {
    const LEN: usize = 64 * 1024;
    let strings = |s: &mut Shape| {
        s.foreign = Some(Arc::new(ForeignData { text: Some(Arc::from(filler(LEN))), ..ForeignData::default() }));
        s.text = Some(Arc::new(Text {
            parts: vec![
                TextPart::Fld { ix: 0, text: Arc::from(filler(LEN)) },
                TextPart::Raw(RawNode::Comment(Arc::from(filler(LEN)))),
                TextPart::Raw(RawNode::Pi { target: Arc::from("t"), data: Arc::from(filler(LEN)) }),
            ],
            ..Default::default()
        }));
        s.passthrough.push(0, RawNode::Comment(Arc::from(filler(LEN))));
        s.passthrough.push(0, RawNode::Text(Arc::from(filler(LEN))));
        s.passthrough.push(0, RawNode::Pi { target: Arc::from("t"), data: Arc::from(filler(LEN)) });
    };
    let plain = saved(|doc| doc.masters.push(Arc::new(master_of(|_| {}))));
    let heavy = saved(|doc| {
        let mut m = master_of(strings);
        m.connects = Connects::from_vec(vec![Connect {
            from_sheet: 1,
            to_sheet: 2,
            from_cell: Some(Arc::from(filler(LEN))),
            to_cell: Some(Arc::from(filler(LEN))),
            ..Connect::default()
        }]);
        doc.masters.push(Arc::new(m));
        doc.colors = Arc::new(vec![ColorEntry { ix: 1, rgb: Arc::from(filler(LEN)) }]);
    });
    let items = |bytes: &[u8]| read(bytes, &Limits::default()).unwrap().1.work.items;
    // Ten strings of LEN bytes (the targets of the instructions are a byte), charged one by one, each for the whole items its bytes stand for, and the
    // bytes of the file once more as the backstop.
    // The strings are text, at nine eighths; the backstop is the file's bytes as they are.
    let bytes_in_strings = 10 * LEN as u64;
    let stand_for = bytes_in_strings / 64;
    let as_strings = as_text(bytes_in_strings) / 64;
    let (with, without) = (items(&heavy), items(&plain));
    assert!(
        with - without >= stand_for + as_strings,
        "{with} - {without} items for {bytes_in_strings} bytes: fewer than the {stand_for} they stand for, and the file's size as many again"
    );
    // The elements that hold them (a few dozen) cost a few items each, and nothing is charged more than those two times.
    assert!(with - without <= stand_for + as_strings + 400, "{with} - {without} items: more than the bytes and the elements can cost");
    // The sum of what is charged for each string is what one budget holds: the file read under a budget of exactly that many items more than the plain
    // file's is read, and one item fewer is refused.
    let exact = (with) as usize;
    assert!(read_within(&heavy, exact).is_ok(), "the budget of exactly the items it takes reads it");
    assert!(read_within(&heavy, exact - 1).is_err(), "and one item fewer does not");
    let _ = (CellName::new("x"), RawAttrs::new());
}

/// A document file of `text` bytes of text in an element the reader keeps whole in `branch`: the file is saved, read under the default limits, and the items
/// the read charges are given.
fn items_of(bytes: &[u8]) -> u64 {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}")).1.work.items
}

/// An element that holds `text` bytes of text and `attrs` attributes of a megabyte at most each.
fn element_of(text: usize, attrs: usize) -> RawElement {
    let mut raw = RawAttrs::new();
    for k in 0..attrs {
        raw.push(&format!("k{k}"), &filler(BIG_ATTR));
    }
    RawElement { name: Arc::from("x"), attrs: raw, children: vec![RawNode::Text(Arc::from(filler(text)))] }
}

/// What a subtree kept whole costs (the review, MEDIUM): its bytes, charged before it is copied, as the items they stand for, on top of its nodes.
#[test]
fn a_subtree_kept_whole_is_charged_by_the_bytes_of_its_names_values_and_texts() {
    // The document's own data.
    charged("the data of the document", |doc| doc.diacraft = Some(Arc::new(element_of(BIG_TEXT, 0))));
    charged("the data of the document, in attribute values", |doc| doc.diacraft = Some(Arc::new(element_of(1, 8))));
    // An unknown element kept in a master's shape.
    charged("an element kept in a shape", with_shape(|s| s.passthrough.push(0, RawNode::Element(element_of(BIG_TEXT, 0)))));
    charged("an element kept in a shape, in attribute values", with_shape(|s| s.passthrough.push(0, RawNode::Element(element_of(1, 8)))));
    // A subtree whose bytes are in its names and in a text deep inside it.
    let deep = |text: usize| {
        let mut inner = element_of(text, 0);
        for _ in 0..50 {
            inner = RawElement { name: Arc::from("y"), attrs: RawAttrs::new(), children: vec![RawNode::Element(inner)] };
        }
        inner
    };
    charged("a deep element kept in a shape", with_shape(move |s| s.passthrough.push(0, RawNode::Element(deep(BIG_TEXT)))));
}

/// The same, counted: the items a read charges for a subtree grow with its bytes, `n` of text and `4n`, each over the file with none.
#[test]
fn the_items_a_kept_subtree_costs_are_linear_in_its_bytes() {
    const N: usize = 1 << 20;
    type Set = Box<dyn Fn(&mut Document, usize)>;
    let branches: Vec<(&str, Set)> = vec![
        ("data", Box::new(|doc, n| doc.diacraft = Some(Arc::new(element_of(n, 0))))),
        (
            "kept element",
            Box::new(|doc, n| {
                let mut s = Shape::new(1);
                s.kind = ShapeType::Shape;
                s.passthrough.push(0, RawNode::Element(element_of(n, 0)));
                doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }));
            }),
        ),
    ];
    for (name, set) in branches {
        let base = items_of(&saved(|doc| set(doc, 0)));
        let (one, four) = (items_of(&saved(|doc| set(doc, N))) - base, items_of(&saved(|doc| set(doc, 4 * N))) - base);
        // The bytes are charged twice (the file's size, and the subtree's own): at least two items for each 64 bytes.
        let stand_for = (N / 64) as u64;
        assert!(one >= 2 * stand_for, "{name}: {one} items for {N} bytes: fewer than the {} they stand for", 2 * stand_for);
        assert!(four >= 2 * 4 * stand_for, "{name}: {four} items for {} bytes: fewer than the {} they stand for", 4 * N, 8 * stand_for);
        // And linear: four times the bytes cost four times the items, less or more a few for the nodes that hold them.
        assert!(four <= 4 * one + 64 && four + 64 >= 4 * one, "{name}: {one} items for n and {four} for 4n: not linear");
    }
}

/// The backstop: a document file's size is charged whether or not a branch of the reader charges what is in it. A file of an element this build does not know
/// (which no branch reads) is eight megabytes, and costs the budget its size.
#[test]
fn the_size_of_the_document_file_is_charged_whatever_is_in_it() {
    const N: usize = 1 << 20;
    let bytes = saved(|doc| doc.masters.push(Arc::new(master_of(|_| {}))));
    let file = crate::write_core::part(&bytes, "DiaCraft/document.xml");
    let junk = |n: usize| {
        let patched = file.replace("</diacraft:document>", &format!("<diacraft:future>{}</diacraft:future></diacraft:document>", filler(n)));
        assert_ne!(patched, file);
        crate::docfile::rebuild(
            &bytes,
            |name, b| Some(if name == "DiaCraft/document.xml" { patched.clone().into_bytes() } else { b.to_vec() }),
            |m| m.to_owned(),
        )
    };
    let base = items_of(&bytes);
    let (one, four) = (items_of(&junk(N)) - base, items_of(&junk(4 * N)) - base);
    let stand_for = (N / 64) as u64;
    assert!(one >= stand_for, "{one} items for {N} bytes the reader does not read: fewer than the {stand_for} they stand for");
    assert!(four >= 4 * stand_for, "{four} items for {} bytes: fewer than the {} they stand for", 4 * N, 4 * stand_for);
    assert!(four <= 4 * one + 64 && four + 64 >= 4 * one, "{one} items for n and {four} for 4n: not linear");
    // Eight megabytes of it do not read in a budget the file's size passes by less.
    match read_within(&junk(8 << 20), BUDGET - 30_000) {
        Err(e) => assert_eq!(e.class(), "too_many_items", "{e}"),
        Ok(_) => panic!("eight megabytes the reader does not read went into a budget of {} items", BUDGET - 30_000),
    }
}
