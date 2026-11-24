//! Every cap of the model, probed at its boundary on both sides and in both directions (the plan's D1: "probe the largest input just under each cheap
//! refusal"). For each: the most the cap allows encodes and decodes, and one more is refused by the encoder and by the decoder with the same words, so that a
//! file the codec writes is never one it refuses to read, and a file it reads never holds more than the model allows.
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    Cell, CellName, ColorEntry, Document, MAX_CELLS_PER_CONTAINER, MAX_COLOR_ENTRIES, MAX_MASTERS, MAX_NAME_BYTES, MAX_PAGES, MAX_ROWS_PER_SECTION,
    MAX_SECTIONS_PER_SHEET, MAX_SHAPES_PER_PAGE, MAX_STYLES, Master, Page, Row, Section, Shape, ShapeList, Sheet, Style,
};
use diacraft_sheetxml::model::{BlobSink, Budget, CodecError, MemBlobs, URN, decode, decode_page, decode_sheet, encode, encode_sheet, part_lists};

const HUGE: u64 = 1 << 40;

fn budget() -> Budget {
    Budget::new(HUGE, HUGE, HUGE)
}

fn el(local: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
    El { name: format!("diacraft:{local}"), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
}

fn root(local: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
    let mut all = vec![("xmlns:diacraft", URN)];
    all.extend_from_slice(attrs);
    el(local, &all, children)
}

fn leaf(local: &str, attrs: &[(&str, &str)]) -> Node {
    Node::El(el(local, attrs, Vec::new()))
}

/// What a probe asks of one cap: the words and the number the refusal carries, and a function that makes the input of `n` of the thing, one way (encoding)
/// and the other (decoding), each ending in `Ok` or in the error.
struct Probe {
    what: &'static str,
    max: usize,
    encode: Box<dyn Fn(usize) -> Result<(), CodecError>>,
    decode: Box<dyn Fn(usize) -> Result<(), CodecError>>,
}

fn is_limit(out: &Result<(), CodecError>, what: &'static str, max: usize) -> bool {
    matches!(out, Err(CodecError::Limit { what: w, max: m }) if *w == what && *m == u64::try_from(max).unwrap_or(0))
}

fn run(p: &Probe) {
    for (name, f) in [("encode", &p.encode), ("decode", &p.decode)] {
        let at = f(p.max);
        assert!(at.is_ok(), "{}: {name} of {} is refused: {:?}", p.what, p.max, at.err());
        let past = f(p.max + 1);
        assert!(is_limit(&past, p.what, p.max), "{}: {name} of {} gave {past:?}", p.what, p.max + 1);
    }
}

fn many_cells(n: usize) -> diacraft_doc::Cells {
    (0..n).map(|i| (CellName::new(&format!("C{i}")), Cell::default())).collect()
}

fn cell_nodes(n: usize) -> Vec<Node> {
    (0..n).map(|i| leaf("Cell", &[("N", &format!("C{i}"))])).collect()
}

fn encoded(doc: &Document) -> Result<(), CodecError> {
    encode(doc, &mut MemBlobs::new()).map(|_| ())
}

fn encoded_sheet(sheet: &Sheet) -> Result<(), CodecError> {
    encode_sheet(sheet, &mut MemBlobs::new()).map(|_| ())
}

fn decoded_sheet(e: &El) -> Result<(), CodecError> {
    decode_sheet(e, &MemBlobs::new(), &mut budget(), &mut Vec::new()).map(|_| ())
}

#[test]
fn cells_in_one_sheet_section_or_row() {
    run(&Probe {
        what: "cells in one place",
        max: MAX_CELLS_PER_CONTAINER,
        encode: Box::new(|n| encoded_sheet(&Sheet { cells: many_cells(n), ..Sheet::default() })),
        decode: Box::new(|n| decoded_sheet(&root("sheet", &[], cell_nodes(n)))),
    });
    run(&Probe {
        what: "cells in one place",
        max: MAX_CELLS_PER_CONTAINER,
        encode: Box::new(|n| {
            let mut section = Section::new("User", None);
            section.cells = many_cells(n);
            encoded_sheet(&Sheet { sections: vec![Arc::new(section)], ..Sheet::default() })
        }),
        decode: Box::new(|n| decoded_sheet(&root("sheet", &[], vec![Node::El(el("Section", &[("N", "User")], cell_nodes(n)))]))),
    });
    run(&Probe {
        what: "cells in one place",
        max: MAX_CELLS_PER_CONTAINER,
        encode: Box::new(|n| {
            let mut section = Section::new("User", None);
            section.rows.push(Arc::new(Row { name: Some(Arc::from("r")), cells: many_cells(n), ..Row::default() }));
            encoded_sheet(&Sheet { sections: vec![Arc::new(section)], ..Sheet::default() })
        }),
        decode: Box::new(|n| {
            let row = Node::El(el("Row", &[("N", "r")], cell_nodes(n)));
            decoded_sheet(&root("sheet", &[], vec![Node::El(el("Section", &[("N", "User")], vec![row]))]))
        }),
    });
}

#[test]
fn sections_in_one_sheet() {
    run(&Probe {
        what: "sections in a sheet",
        max: MAX_SECTIONS_PER_SHEET,
        encode: Box::new(|n| {
            encoded_sheet(&Sheet {
                sections: (0..n).map(|i| Arc::new(Section::new("Geometry", u32::try_from(i).ok()))).collect(),
                ..Sheet::default()
            })
        }),
        decode: Box::new(|n| decoded_sheet(&root("sheet", &[], (0..n).map(|_| leaf("Section", &[("N", "Geometry")])).collect()))),
    });
}

#[test]
fn rows_in_one_section() {
    run(&Probe {
        what: "rows in a section",
        max: MAX_ROWS_PER_SECTION,
        encode: Box::new(|n| {
            let mut section = Section::new("Geometry", Some(0));
            section.rows = (0..n).map(|i| Arc::new(Row { ix: u32::try_from(i).ok(), ..Row::default() })).collect();
            encoded_sheet(&Sheet { sections: vec![Arc::new(section)], ..Sheet::default() })
        }),
        decode: Box::new(|n| {
            let rows = (0..n).map(|_| leaf("Row", &[])).collect();
            decoded_sheet(&root("sheet", &[], vec![Node::El(el("Section", &[("N", "Geometry")], rows))]))
        }),
    });
}

#[test]
fn names_are_not_empty_and_at_most_the_models_length() {
    let name = |n: usize| "n".repeat(n);
    let cell_named = |n: String| encoded_sheet(&Sheet { cells: [(CellName::new(&n), Cell::default())].into_iter().collect(), ..Sheet::default() });
    assert!(cell_named(name(MAX_NAME_BYTES)).is_ok());
    for bad in [name(MAX_NAME_BYTES + 1), String::new()] {
        assert!(matches!(cell_named(bad.clone()), Err(CodecError::Malformed { .. })), "a cell named {} bytes", bad.len());
        assert!(
            matches!(decoded_sheet(&root("sheet", &[], vec![leaf("Cell", &[("N", &bad)])])), Err(CodecError::Malformed { .. })),
            "a cell named {} bytes",
            bad.len()
        );
    }
    let section_named = |n: String| decoded_sheet(&root("sheet", &[], vec![leaf("Section", &[("N", &n)])]));
    assert!(section_named(name(MAX_NAME_BYTES)).is_ok());
    assert!(matches!(section_named(name(MAX_NAME_BYTES + 1)), Err(CodecError::Malformed { .. })));
    assert!(matches!(section_named(String::new()), Err(CodecError::Malformed { .. })), "a section with no name");
    let row_named = |n: &str| {
        let row = Node::El(el("Row", &[("N", n)], Vec::new()));
        decoded_sheet(&root("sheet", &[], vec![Node::El(el("Section", &[("N", "User")], vec![row]))]))
    };
    assert!(row_named(&name(MAX_NAME_BYTES)).is_ok());
    assert!(matches!(row_named(&name(MAX_NAME_BYTES + 1)), Err(CodecError::Malformed { .. })));
    assert!(matches!(row_named(""), Err(CodecError::Malformed { .. })), "a row with an empty name is not a row with no name");
    let mut section = Section::new("User", None);
    section.rows.push(Arc::new(Row { name: Some(Arc::from("")), ..Row::default() }));
    assert!(matches!(encoded_sheet(&Sheet { sections: vec![Arc::new(section)], ..Sheet::default() }), Err(CodecError::Malformed { .. })));
    let mut section = Section::new(&name(MAX_NAME_BYTES + 1), None);
    section.cells = many_cells(1);
    assert!(matches!(encoded_sheet(&Sheet { sections: vec![Arc::new(section)], ..Sheet::default() }), Err(CodecError::Malformed { .. })));
}

#[test]
fn the_cap_on_colours_and_on_styles_stops_the_read_at_the_entry_that_passes_it() {
    // The model's own check at the end of a decode would refuse them too, with the same words: the cap in the loop is what keeps a part of a million
    // entries from being read to its end first, and the items charged say which refused.
    for (local, max, what) in [("color", MAX_COLOR_ENTRIES, "colour table entries"), ("style", MAX_STYLES, "styles")] {
        let extra = 1_000;
        let children = (0..max + extra).map(|_| leaf(local, &[])).collect();
        let mut budget = budget();
        let before = budget.items;
        let out = decode(&root("document", &[], children), &[], &[], &MemBlobs::new(), &mut budget, &mut Vec::new());
        assert!(matches!(&out, Err(CodecError::Limit { what: w, .. }) if *w == what), "{local}: {:?}", out.err());
        let charged = before - budget.items;
        assert!(charged < u64::try_from(max + extra / 2).unwrap_or(0), "{local}: {charged} items charged for {} entries", max + extra);
    }
}

#[test]
fn the_cap_on_parts_stops_the_read_at_the_part_that_passes_it() {
    use diacraft_doc::{Format, MAX_PARTS, Part, PartRole, RawAttrs};
    // One honest part, written by the codec, and the document part with that element cloned.
    let mut d = Document::empty();
    d.parts.push(Arc::new(Part {
        format: Format::Native,
        name: Arc::from("/x"),
        media_type: Arc::from("a/b"),
        role: PartRole::Thumbnail,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&b"x"[..]),
    }));
    let mut blobs = MemBlobs::new();
    let written = encode(&d, &mut blobs).expect("a document of a part");
    let with = |n: usize| {
        let mut document = written.document.clone();
        let at = document.children.iter().position(|c| matches!(c, Node::El(e) if e.local() == "part")).expect("the part element");
        let part = document.children[at].clone();
        document.children.splice(at..=at, std::iter::repeat_n(part, n));
        document
    };
    let read = |n: usize| {
        let mut budget = budget();
        let before = budget.items;
        let out = decode(&with(n), &[], &[], &blobs, &mut budget, &mut Vec::new());
        (out.map(|_| ()), before - budget.items)
    };
    let (at_the_cap, items_at_the_cap) = read(MAX_PARTS);
    assert!(at_the_cap.is_ok(), "{:?}", at_the_cap.err());
    let (past, items_past) = read(MAX_PARTS + 1_000);
    assert!(matches!(&past, Err(CodecError::Limit { what: "parts", .. })), "{:?}", past.err());
    // It stopped at the part that passes the cap: a part is a handful of items, and a thousand more were left unread.
    assert!(items_past < items_at_the_cap + 100, "{items_past} items for the refusal, {items_at_the_cap} for the cap's worth");
}

#[test]
fn colours_in_the_table() {
    run(&Probe {
        what: "colour table entries",
        max: MAX_COLOR_ENTRIES,
        encode: Box::new(|n| {
            let mut d = Document::empty();
            d.colors = Arc::new((0..n).map(|i| ColorEntry { ix: u32::try_from(i).unwrap_or(0), rgb: Arc::from("#000000") }).collect());
            encoded(&d)
        }),
        decode: Box::new(|n| {
            let colors = (0..n).map(|_| leaf("color", &[])).collect();
            decode(&root("document", &[], colors), &[], &[], &MemBlobs::new(), &mut budget(), &mut Vec::new()).map(|_| ())
        }),
    });
}

#[test]
fn pages_of_a_document() {
    run(&Probe {
        what: "pages",
        max: MAX_PAGES,
        encode: Box::new(|n| {
            let mut d = Document::empty();
            let page = Arc::new(Page::new(1, "P"));
            for _ in 0..n {
                d.pages.push(Arc::clone(&page));
            }
            encoded(&d)
        }),
        decode: Box::new(|n| {
            let refs = (0..n).map(|i| leaf("page-ref", &[("part", &format!("pages/page{i}.xml"))])).collect();
            part_lists(&root("document", &[], refs)).map(|_| ())
        }),
    });
}

#[test]
fn masters_and_styles_of_a_document() {
    run(&Probe {
        what: "masters",
        max: MAX_MASTERS,
        encode: Box::new(|n| {
            let mut d = Document::empty();
            let m = Arc::new(Master::default());
            for _ in 0..n {
                d.masters.push(Arc::clone(&m));
            }
            encoded(&d)
        }),
        decode: Box::new(|n| {
            let refs = (0..n).map(|i| leaf("master-ref", &[("part", &format!("masters/master{i}.xml"))])).collect();
            part_lists(&root("document", &[], refs)).map(|_| ())
        }),
    });
    run(&Probe {
        what: "styles",
        max: MAX_STYLES,
        encode: Box::new(|n| {
            let mut d = Document::empty();
            let s = Arc::new(Style::default());
            for _ in 0..n {
                d.styles.push(Arc::clone(&s));
            }
            encoded(&d)
        }),
        decode: Box::new(|n| {
            let styles = (0..n).map(|_| leaf("style", &[])).collect();
            decode(&root("document", &[], styles), &[], &[], &MemBlobs::new(), &mut budget(), &mut Vec::new()).map(|_| ())
        }),
    });
}

#[test]
fn blobs_of_a_package() {
    let max = diacraft_container::limits::MAX_PARTS;
    run(&Probe {
        what: "blobs",
        max,
        encode: Box::new(|n| {
            let mut blobs = MemBlobs::new();
            for i in 0..n {
                blobs.put(&Arc::from(i.to_le_bytes().to_vec()), "image/png")?;
            }
            Ok(())
        }),
        decode: Box::new(|n| {
            let records = (0..n).map(|i| leaf("blob", &[("id", &i.to_string()), ("stamp", "0000000000000000")])).collect();
            part_lists(&root("document", &[], records)).map(|_| ())
        }),
    });
}

#[test]
fn shapes_in_one_part() {
    let one: Arc<Shape> = Arc::new(Shape::new(0));
    run(&Probe {
        what: "shapes in a part",
        max: MAX_SHAPES_PER_PAGE,
        encode: Box::new({
            let one = Arc::clone(&one);
            move |n| {
                let mut d = Document::empty();
                let mut p = Page::new(1, "P");
                p.shapes = ShapeList::from_vec(vec![Arc::clone(&one); n]);
                d.pages.push(Arc::new(p));
                encoded(&d)
            }
        }),
        decode: Box::new(|n| {
            let shapes = (0..n).map(|_| leaf("shape", &[])).collect();
            decode_page(&root("page", &[], shapes), &MemBlobs::new(), &mut budget(), &mut Vec::new()).map(|_| ())
        }),
    });
}
