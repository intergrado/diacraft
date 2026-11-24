//! DiaCraft's document-level file (design decisions 6 and 7; the proposal).
//!
//! The file is one part of the package, `DiaCraft/document.xml`, listed in the manifest. Every case saves a model with document-level data, reads the bytes
//! back and compares what came back with what went in.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Connect, Connects, Document, ForeignData, Master, Media, RawAttrs, RawElement, RawNode, Row, RowKey, Section, SectionKey, Shape,
    ShapeList, ShapeType, Text, TextPart, Value,
};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, read};

use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real};

const PATH: &str = "DiaCraft/document.xml";

pub fn base() -> crate::write_core::Open {
    open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")))
}

pub fn cell(formula: Option<&str>, value: &str) -> Cell {
    Cell::read(formula, Some(value), Some("IN"), None, &mut 1_000)
}

/// A shape of a master: a sheet, a text with markers, shape data, a picture, raw attributes, kept nodes and a member.
pub fn body_shape(id: u32, with_member: bool) -> Shape {
    let mut s = Shape::new(id);
    s.name = Some(Arc::from(format!("Body {id}")));
    s.name_u = Some(Arc::from(format!("Body.{id}")));
    s.kind = if with_member { ShapeType::Group } else { ShapeType::Shape };
    s.del = id.is_multiple_of(5);
    s.data1 = Some(Arc::from("one"));
    s.data3 = Some(Arc::from("three <&>"));
    s.text = Some(Arc::new(Text {
        parts: vec![TextPart::Cp(1), TextPart::Chars(Arc::from("Hi\nthere")), TextPart::Fld { ix: 2, text: Arc::from("9") }],
        ..Default::default()
    }));
    s.raw = RawAttrs::from_pairs([("UniqueID", "{A-B}"), ("odd:name", "value \"q\"")]);
    s.passthrough.push(1, RawNode::Comment(Arc::from("note")));
    s.passthrough.push(
        0,
        RawNode::Element(RawElement {
            name: Arc::from("x:other"),
            attrs: RawAttrs::from_pairs([("xmlns:x", "urn:x"), ("k", "v")]),
            children: Vec::new(),
        }),
    );
    let sheet = Arc::make_mut(&mut s.sheet);
    sheet.cells.insert(CellName::new("Width"), cell(Some("Height*2"), "2"));
    sheet.cells.insert(CellName::new("Height"), cell(None, "1"));
    sheet.links.fill = Some(3);
    let mut geometry = Section::new("Geometry", Some(0));
    let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(2), ..Row::default() };
    row.cells.insert(CellName::new("X"), cell(Some("Width"), "2"));
    geometry.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(geometry));
    if with_member {
        s.children = ShapeList::from_vec(vec![Arc::new(body_shape(id + 100, false))]);
    }
    s.foreign = (id == 7).then(|| {
        Arc::new(ForeignData {
            attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
            rel_id: Some(Arc::from("rId4")),
            picture: Some(Arc::new(Media {
                part: Arc::from("media/image1.png"),
                content_type: Arc::from("image/png"),
                bytes: Arc::from(vec![0x89, b'P', b'N', b'G', 0, 1, 2, 255, 254]),
            })),
            text: Some(Arc::from("alt")),
            ..ForeignData::default()
        })
    });
    s
}

pub fn a_master(id: u32) -> Master {
    let mut m = Master {
        id,
        name: Some(Arc::from(format!("Master {id}"))),
        name_u: Some(Arc::from(format!("Master.{id}"))),
        unique_id: Some(Arc::from("{GUID}")),
        pattern_flags: Some(65),
        rel_id: Some(Arc::from("rId9")),
        icon: Some(Arc::from("AAAA")),
        raw: RawAttrs::from_pairs([("MatchByName", "1")]),
        ..Master::default()
    };
    Arc::make_mut(&mut m.sheet).cells.insert(CellName::new("PageWidth"), cell(None, "3"));
    m.shapes = ShapeList::from_vec(vec![Arc::new(body_shape(id * 10 + 1, true)), Arc::new(body_shape(id * 10 + 2, false))]);
    m.connects = Connects::from_vec(vec![Connect {
        from_sheet: id * 10 + 1,
        from_cell: Some(Arc::from("BeginX")),
        from_part: Some(9),
        to_sheet: id * 10 + 2,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(3),
        raw: RawAttrs::from_pairs([("Extra", "1")]),
        ext: Default::default(),
    }]);
    m
}

#[test]
fn master_bodies_are_self_contained() {
    let mut o = base();
    // The picture of a shape of master 7 is in the file itself: no `Pictures/` part is made for it.
    o.doc.masters.push(Arc::new(a_master(7)));
    o.doc.masters.push(Arc::new(a_master(8)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    let names: Vec<String> = package.manifest().entries.iter().map(|e| e.full_path.clone()).collect();
    assert!(names.contains(&PATH.to_owned()), "listed in the manifest: {names:?}");
    let row = package.manifest().entries.iter().find(|e| e.full_path == PATH).unwrap();
    assert_eq!(row.media_type.as_deref(), Some("application/vnd.diacraft.document+xml"));
    assert!(!names.iter().any(|n| n.starts_with("Pictures/")), "a picture of a master is no part of its own: {names:?}");
    // What the file holds is the whole of both masters, read from that one part and no other.
    let back = read(&saved.bytes, &Limits::default()).unwrap().0;
    assert_eq!(back.masters.len(), 2);
    for (made, got) in o.doc.masters.iter().zip(back.masters.iter()) {
        assert_eq!(**got, **made, "master {} comes back whole", made.id);
    }
    // A package with every other part of the drawing as it was, and the file alone different.
    let before = Package::read(&o.bytes, &Limits::default()).unwrap();
    assert!(before.part(PATH).is_none());
    assert!(package.part(PATH).is_some());
    // Saved again, it is no edit.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

#[test]
fn the_document_s_cells_its_data_themes_colours_and_page_cells_read_back() {
    let mut o = base();
    {
        let sheet = Arc::make_mut(&mut o.doc.sheet);
        sheet.cells.insert(CellName::new("DocLangID"), cell(None, "1033"));
        let mut user = Section::new("User", None);
        let mut row = Row { name: Some(Arc::from("k")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), cell(Some("1+1"), "2"));
        user.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(user));
    }
    assert!(o.doc.set_diacraft_child("snap", Some(&[("flags", "6")])));
    o.doc.themes.push(Arc::new(diacraft_doc::Theme {
        part: Arc::from("theme/theme1.xml"),
        bytes: Arc::from(b"<a:theme/>".to_vec()),
        ..diacraft_doc::Theme::default()
    }));
    Arc::make_mut(&mut o.doc.colors).push(diacraft_doc::ColorEntry { ix: 24, rgb: Arc::from("#112233") });
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("DrawingScaleType"), cell(None, "0"));
        let mut layer = Section::new("Layer", None);
        let mut row = Row { name: Some(Arc::from("Walls")), ..Row::default() };
        row.cells.insert(CellName::new("Visible"), cell(None, "0"));
        layer.rows.push(Arc::new(row));
        Arc::make_mut(&mut page.sheet).sections.push(Arc::new(layer));
        page.name_u = Some(Arc::from("Original.1"));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.sheet.cell("DocLangID").and_then(|c| c.value.as_ref()).map(Value::text), Some("1033"));
    assert!(back.doc.sheet.section(&SectionKey::new("User", None)).is_some());
    assert_eq!(back.doc.diacraft_child("snap").and_then(|e| e.attrs.get("flags").map(str::to_owned)), Some("6".to_owned()));
    assert_eq!(back.doc.themes.len(), 1);
    let theme = back.doc.themes.get(0).unwrap();
    assert_eq!((&*theme.part, &*theme.bytes), ("theme/theme1.xml", b"<a:theme/>".as_slice()));
    assert_eq!(back.doc.colors.len(), 1);
    let page = back.doc.pages.get(0).unwrap();
    assert!(page.sheet.cell("DrawingScaleType").is_some());
    let layers = page.sheet.section(&SectionKey::new("Layer", None)).unwrap();
    assert!(layers.rows.iter().any(|r| r.name.as_deref() == Some("Walls")), "{layers:?}");
    assert_eq!(page.name_u.as_deref(), Some("Original.1"));
    // What the markup says is the markup's: the page's size is not in the document file.
    let file = part(&saved.bytes, PATH);
    assert!(!file.contains("PageWidth") && !file.contains("PageHeight"), "{file}");
    // Saved again, it is no edit.
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited);
}

#[test]
fn a_file_that_is_missing_stale_or_unreadable_is_never_an_error() {
    let mut o = base();
    o.doc.masters.push(Arc::new(a_master(7)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    // A LibreOffice save drops the file and its row: the drawing reads from its markup, with no masters and no word of complaint.
    let dropped = rebuild(
        &saved.bytes,
        |name, bytes| (name != PATH).then(|| bytes.to_vec()),
        // (The folder's row goes with it: a program that drops the folder's entries drops its row.)
        |manifest| manifest.lines().filter(|l| !l.contains(PATH) && !l.contains("\"DiaCraft/\"")).collect::<Vec<_>>().join("\n"),
    );
    let (doc, report) = read(&dropped, &Limits::default()).unwrap();
    assert!(doc.masters.is_empty());
    assert!(report.warning_counts.is_empty(), "{:?}", report.warning_counts);
    // The file is there and is not XML, is XML of no document of ours, or is a document of a version that is not this one's: read as far as it can be.
    for (bytes, class) in [
        (b"not xml".to_vec(), Some("diacraft_file_unreadable")),
        (b"<other/>".to_vec(), Some("diacraft_file_unreadable")),
        (b"<d:document xmlns:d=\"urn:app.diacraft:extension\" version=\"9\"><d:future/></d:document>".to_vec(), Some("diacraft_version")),
    ] {
        let mutated = rebuild(&saved.bytes, |name, b| Some(if name == PATH { bytes.clone() } else { b.to_vec() }), |m| m.to_owned());
        let (doc, report) = read(&mutated, &Limits::default()).unwrap_or_else(|e| panic!("{class:?}: {e}"));
        assert!(doc.masters.is_empty(), "{class:?}");
        assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), class.into_iter().collect::<Vec<_>>());
    }
}

/// A package with every part passed through `part` (`None` leaves it out) and the manifest through `manifest`.
pub fn rebuild(bytes: &[u8], part: impl Fn(&str, &[u8]) -> Option<Vec<u8>>, manifest: impl Fn(&str) -> String) -> Vec<u8> {
    let p = Package::read(bytes, &Limits::default()).unwrap();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for e in p.manifest().entries.iter().filter(|e| !e.full_path.ends_with('/') && e.full_path != "/") {
        if let Some(b) = p.part(&e.full_path)
            && let Some(out) = part(&e.full_path, b)
        {
            files.push((e.full_path.clone(), out));
        }
    }
    let manifest_text = String::from_utf8(p.part("META-INF/manifest.xml").unwrap().to_vec()).unwrap();
    let mut w = diacraft_container::zip::ArchiveWriter::new();
    w.add("mimetype", crate::build::DRAWING.as_bytes(), diacraft_container::zip::Method::Stored).unwrap();
    for (name, b) in &files {
        w.add(name, b, diacraft_container::zip::Method::Deflated).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest(&manifest_text).as_bytes(), diacraft_container::zip::Method::Deflated).unwrap();
    w.finish().unwrap()
}

#[test]
fn what_the_file_holds_that_this_build_does_not_know_survives_a_save() {
    let mut o = base();
    o.doc.masters.push(Arc::new(a_master(7)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    // A child of a later version, put into the file.
    let file = part(&first.bytes, PATH);
    let with_future = file.replace("</diacraft:document>", "<diacraft:future kept=\"yes\"/></diacraft:document>");
    assert_ne!(file, with_future);
    let patched = rebuild(&first.bytes, |name, b| Some(if name == PATH { with_future.clone().into_bytes() } else { b.to_vec() }), |m| m.to_owned());
    let mut again = open_bytes(patched);
    // An edit of a master rewrites the file; the future child is where it was.
    Arc::make_mut(Arc::make_mut(again.doc.masters.get_mut(0).unwrap()).shapes.get_mut(0).unwrap()).name = Some(Arc::from("Renamed"));
    let saved = real(&again.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = part(&saved.bytes, PATH);
    assert!(file.contains("<diacraft:future kept=\"yes\"/>"), "{file}");
    assert!(file.contains("Renamed"), "{file}");
}

#[test]
fn a_model_that_holds_nothing_the_file_carried_takes_the_file_and_its_row_out() {
    let mut o = base();
    o.doc.masters.push(Arc::new(a_master(7)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let mut again = open_bytes(first.bytes.clone());
    again.doc.masters = diacraft_doc::SharedVec::new();
    let saved = real(&again.doc, &again.bytes, &again.index).unwrap();
    assert!(saved.edited);
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert!(package.part(PATH).is_none());
    assert!(!package.manifest().entries.iter().any(|e| e.full_path == PATH));
    assert!(read(&saved.bytes, &Limits::default()).unwrap().0.masters.is_empty());
    let _ = (Document::empty(), RowKey::Ix(0));
}

/// Two pages of one name are told apart by the entry's `nth`: each page keeps its own cells and its own `NameU` through a save, and a page of a name nothing else
/// holds is written with no `nth` at all.
#[test]
fn pages_of_one_name_are_told_apart_by_their_number_among_them() {
    let shapes = crate::write_core::custom("s1", "1cm");
    let mut drawing = Drawing::a4();
    drawing.pages = format!(
        r#"<draw:page draw:name="dup" draw:master-page-name="Default">{shapes}</draw:page><draw:page draw:name="dup" draw:master-page-name="Default">{shapes}</draw:page><draw:page draw:name="solo" draw:master-page-name="Default">{shapes}</draw:page>"#
    );
    let mut o = open(&drawing);
    assert_eq!(o.doc.pages.len(), 3);
    for (at, text) in [(0usize, "first"), (1, "second"), (2, "third")] {
        let page = Arc::make_mut(o.doc.pages.get_mut(at).unwrap());
        Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text(text)));
        page.name_u = Some(Arc::from(format!("Original.{at}")));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = part(&saved.bytes, PATH);
    assert_eq!(file.matches("nth=").count(), 1, "only the second page of a name says its number: {file}");
    assert!(file.contains("nth=\"2\""), "{file}");
    let back = open_bytes(saved.bytes.clone());
    for (at, text) in [(0usize, "first"), (1, "second"), (2, "third")] {
        let page = back.doc.pages.get(at).unwrap();
        assert_eq!(page.sheet.cell("Comment").and_then(|c| c.value.as_ref()).map(Value::text), Some(text), "page {at}");
        assert_eq!(page.name_u.as_deref().map(str::to_owned), Some(format!("Original.{at}")), "page {at}");
    }
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited, "saved again, it is no edit");
}

/// What reading the file is charged (the items of the read's own count): one for each child of the root and one for each attribute of it, whatever the child
/// is, and one for each token of `gone` up to as many as the reader parses, which is as many as it applies.
#[test]
fn the_file_is_charged_for_each_child_and_attribute_and_each_token_of_gone_up_to_the_most_it_parses() {
    let mut o = base();
    o.doc.masters.push(Arc::new(a_master(7)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let page = o.doc.pages.get(0).unwrap().name.to_string();
    // The version of the document file, which is no tier 2's (`diacraft_doc::DIACRAFT_VERSION`, 1).
    let version = "2";
    let read_with = |body: &str| {
        let xml = format!(r#"<d:document xmlns:d="urn:app.diacraft:extension" version="{version}">{body}</d:document>"#);
        let bytes = rebuild(&saved.bytes, |name, b| Some(if name == PATH { xml.clone().into_bytes() } else { b.to_vec() }), |m| m.to_owned());
        let (doc, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(report.warning_counts.is_empty(), "{:?}", report.warning_counts);
        // The file's own size is charged too, a sixty-fourth of a byte each (the backstop under every string charge): what is counted here is the rest.
        (doc, report.work.items - u64::try_from(xml.len().div_ceil(64)).unwrap())
    };
    // Children: an item each, and one for each attribute.
    let (_, none) = read_with("");
    for (body, items) in [("<d:data/>", 1), ("<d:data/><d:data/><d:data/><d:data/>", 4), (r#"<d:data a="1" b="2"/>"#, 3), (r#"<d:data a="1"/>"#, 2)] {
        assert_eq!(read_with(body).1 - none, items, "{body}: an item for the child and one for each of its attributes");
    }
    // Tokens of `gone`: the most the reader parses is four for each attribute a sheet may hold sections' worth of.
    let most = 4 * (diacraft_doc::MAX_SECTIONS_PER_SHEET + 16);
    let entry = |tokens: &str| format!(r#"<d:page name="{page}" gone="{tokens}"/>"#);
    let (before, none) = read_with(&entry(""));
    assert!(before.pages.get(0).unwrap().sheet.cell("DrawingScale").is_some() && before.pages.get(0).unwrap().sheet.cell("PageScale").is_some());
    // The last token the reader parses names a cell and so does the first it does not: the first is gone, the second stays.
    let tokens = format!("{}c.DrawingScale c.PageScale{}", "x ".repeat(most - 1), " x".repeat(50));
    assert_eq!(tokens.split_ascii_whitespace().count(), most + 51);
    let (after, some) = read_with(&entry(&tokens));
    let sheet = &after.pages.get(0).unwrap().sheet;
    assert!(sheet.cell("DrawingScale").is_none(), "the token the reader parses last is applied");
    assert!(sheet.cell("PageScale").is_some(), "the token past the most is not read");
    assert_eq!(u64::try_from(most).unwrap(), some - none, "each token up to the most is an item, and no more");
}
