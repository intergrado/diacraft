//! The model the carrier's tests start from (a drawing as the VSDX reader fills one, with data in every place), and the list of the data a drawing holds that a trip must keep.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellExtra, CellName, Document, Format, Kept, Part, PartRole, Passthrough, RawAttrs, RawElement, RawNode, Rel, Row, Section, ShapeType,
    Text, TextPart, Value, VsdxRest,
};

use crate::model_write::blank;
use crate::write_new::{push_shape, rectangle};

pub(crate) fn raw_el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
    RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
}

pub(crate) fn raw_node(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
    RawNode::Element(raw_el(name, attrs, children))
}

pub(crate) fn kept(attrs: &[(&str, &str)], nodes: Vec<(u32, RawNode)>) -> Kept {
    let mut passthrough = Passthrough::new();
    for (after, node) in nodes {
        passthrough.push(after, node);
    }
    Kept { raw: RawAttrs::from_pairs(attrs.iter().copied()), kept: passthrough }
}

/// What a VSDX reader leaves in a slot's `vsdx` side: the elements the object had no field for, each by the local name of the element.
pub(crate) fn rest(keyed: Vec<(&str, Kept)>) -> VsdxRest {
    VsdxRest(keyed.into_iter().map(|(key, k)| (Arc::from(key), k)).collect())
}

/// A part of a VSDX package, as the VSDX reader holds one.
pub(crate) fn vsdx_part(name: &str, media: &str, role: PartRole, rels: Vec<Rel>, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(name),
        media_type: Arc::from(media),
        role,
        manifest: RawAttrs::new(),
        rels: Arc::new(rels),
        bytes: Arc::from(bytes),
    })
}

/// A part of an `.odg` package, as the `.odg` reader holds one: the yardstick of a carried part.
pub(crate) fn odf_part_named(name: &str, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Odf,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Other,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

pub(crate) fn rel(source: &str, id: &str, kind: &str, target: &str) -> Rel {
    Rel { source: Arc::from(source), id: Arc::from(id), kind: Arc::from(kind), target: Arc::from(target), external: false }
}

/// A drawing of a VSDX file as the VSDX reader fills one, with data in the places the carrier's tests grow to cover (see each test).
pub(crate) fn vsdx_model() -> Document {
    let mut doc = blank(0);
    let mut plain = rectangle(1, "Box", (2.0, 3.0, 2.0, 2.0));
    plain.raw = RawAttrs::from_pairs([("OriginalID", "7"), ("UniqueID", "{GUID}")]);
    plain.passthrough.push(0, raw_node("Hyperlink", &[("ID", "1")], vec![RawNode::Text(Arc::from("x"))]));
    plain.data1 = Some(Arc::from("d1"));
    plain.data2 = Some(Arc::from("d2"));
    plain.data3 = Some(Arc::from("d3"));
    plain.ext.make_mut().vsdx = rest(vec![("Shapes", kept(&[("xml:space", "preserve")], Vec::new()))]);
    {
        let sheet = Arc::make_mut(&mut plain.sheet);
        sheet.raw = RawAttrs::from_pairs([("Foo", "bar")]);
        let cell = sheet.cells.get_mut("FillPattern").unwrap();
        cell.raw = RawAttrs::from_pairs([("X", "1")]);
        cell.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("cell text")), ..CellExtra::default() }));
        let mut extra_nodes = Passthrough::new();
        extra_nodes.push(0, raw_node("RefBy", &[("T", "Shape")], Vec::new()));
        let other = sheet.cells.get_mut("LinePattern").unwrap();
        other.extra = Some(Arc::new(CellExtra { passthrough: extra_nodes, ..CellExtra::default() }));
        let mut user = Section::new("User", None);
        user.raw = RawAttrs::from_pairs([("Hint", "h")]);
        let mut row = Row { name: Some(Arc::from("Note")), local_name: Some(Arc::from("Local")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("\"v\"")));
        row.raw = RawAttrs::from_pairs([("RowAttr", "1")]);
        user.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(user));
    }
    push_shape(&mut doc, plain);
    // A picture of the VSDX file: foreign data beyond its type (the attributes that ride, a relationship, text, an element, a slot).
    let mut picture = crate::write_picture::vsdx_picture(2, "Photo", (6.0, 3.0, 2.0, 2.0), &crate::write_core::PNG);
    {
        let mut foreign = (**picture.foreign.as_ref().unwrap()).clone();
        foreign.attrs = RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG"), ("ObjectWidth", "2"), ("ObjectHeight", "2")]);
        foreign.text = Some(Arc::from("foreign text"));
        foreign.passthrough.push(0, raw_node("Extra", &[("K", "v")], Vec::new()));
        foreign.ext.make_mut().vsdx = rest(vec![("Rel", kept(&[("r:id", "rId2")], Vec::new()))]);
        picture.foreign = Some(Arc::new(foreign));
        picture.raw = RawAttrs::from_pairs([("OriginalID", "9")]);
    }
    push_shape(&mut doc, picture);
    // A shape with a text that holds a part of VSDX's own, and the slot of the text.
    let mut texty = rectangle(3, "Text", (2.0, 8.0, 3.0, 1.0));
    {
        let mut text = Text {
            parts: vec![
                TextPart::Chars(Arc::from("one")),
                TextPart::Raw(raw_node("custom", &[("a", "b")], Vec::new())),
                TextPart::Chars(Arc::from("two")),
            ],
            ..Text::default()
        };
        text.ext.make_mut().vsdx = rest(vec![("Text", kept(&[("xml:space", "preserve")], Vec::new()))]);
        texty.text = Some(Arc::new(text));
    }
    push_shape(&mut doc, texty);
    // Shapes of every kind, and a deleted one.
    for (id, kind) in [(4, ShapeType::Unspecified), (5, ShapeType::Other(Arc::from("Custom"))), (6, ShapeType::Foreign)] {
        let mut s = rectangle(id, &format!("Kind{id}"), (2.0 + f64::from(id), 10.0, 1.0, 1.0));
        s.kind = kind;
        if id == 4 {
            s.del = true;
        }
        push_shape(&mut doc, s);
    }
    // A group of two members, with data on the group and on a member.
    let mut group = rectangle(7, "Group", (2.0, 12.0, 4.0, 2.0));
    group.kind = ShapeType::Group;
    group.raw = RawAttrs::from_pairs([("OriginalID", "70")]);
    group.ext.make_mut().vsdx = rest(vec![("Shapes", kept(&[("xml:space", "preserve")], Vec::new()))]);
    let mut first = rectangle(8, "MemberA", (2.0, 12.0, 1.0, 1.0));
    first.raw = RawAttrs::from_pairs([("UniqueID", "{A}")]);
    group.children = diacraft_doc::SharedVec::from_vec(vec![Arc::new(first), Arc::new(rectangle(9, "MemberB", (4.0, 12.0, 1.0, 1.0)))]);
    push_shape(&mut doc, group);
    // The page: the attributes and elements of its part and of its row, the relationship and the part it was read from, its slot.
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.raw = RawAttrs::from_pairs([("ViewScale", "-1"), ("ViewCenterX", "4.25")]);
        page.passthrough.push(0, raw_node("PageSheetNote", &[("a", "b")], Vec::new()));
        page.rel_id = Some(Arc::from("rId3"));
        page.part_name = Some(Arc::from("/visio/pages/page1.xml"));
        page.ext.make_mut().vsdx =
            rest(vec![("PageContents", kept(&[("xml:space", "preserve")], Vec::new())), ("Rel", kept(&[("r:id", "rId3")], Vec::new()))]);
        Arc::make_mut(&mut page.sheet).raw = RawAttrs::from_pairs([("PageSheetAttr", "1")]);
    }
    // A style sheet with data of its own.
    {
        let mut style = crate::write_styles_new::style_sheet(1, "Fancy", 0);
        style.raw = RawAttrs::from_pairs([("IsCustomName", "1")]);
        style.passthrough.push(0, raw_node("StyleNote", &[("k", "v")], Vec::new()));
        style.ext.make_mut().vsdx = rest(vec![("StyleSheet", kept(&[("x", "y")], Vec::new()))]);
        doc.styles.push(Arc::new(style));
    }
    // The document: its slot, its elements, a face, the properties an `.odg` has no field for, the package's relationships and rows, and the rest of its properties parts.
    doc.ext.make_mut().vsdx =
        rest(vec![("VisioDocument", kept(&[("xml:space", "preserve")], Vec::new())), ("Colors", kept(&[("k", "1")], Vec::new()))]);
    doc.passthrough.push(0, raw_node("DocumentSettings", &[("TopPage", "0")], Vec::new()));
    {
        let mut face = diacraft_doc::FaceName {
            name_u: Arc::from("Arial"),
            raw: RawAttrs::from_pairs([("Panose", "2 11 6 4 2 2 2 2 2 4")]),
            ext: Default::default(),
        };
        face.ext.make_mut().vsdx = rest(vec![("FaceName", kept(&[("Flags", "3")], Vec::new()))]);
        Arc::make_mut(&mut doc.fonts).push(face);
    }
    {
        let props = Arc::make_mut(&mut doc.props);
        props.category = Some(Arc::from("Category"));
        props.company = Some(Arc::from("Company"));
        props.manager = Some(Arc::from("Manager"));
        props.hyperlink_base = Some(Arc::from("http://base/"));
        props.custom.push(diacraft_doc::CustomProp {
            format: Format::Vsdx,
            name: Arc::from("Project"),
            value_type: Arc::from("vt:lpwstr"),
            value: Arc::from("Atlas"),
            raw: RawAttrs::from_pairs([("pid", "2")]),
        });
    }
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld {
        rels: vec![rel("/", "rId1", "http://schemas.microsoft.com/visio/2010/relationships/document", "visio/document.xml")],
        content_types: vec![
            diacraft_doc::ContentTypeRow::Default { extension: Arc::from("bin"), content_type: Arc::from("application/vnd.ms-office.vbaProject") },
            diacraft_doc::ContentTypeRow::Override { part: Arc::from("/visio/x.xml"), content_type: Arc::from("application/xml") },
        ],
        props_rest: vec![(
            Arc::from("coreProperties"),
            kept(&[("k", "v")], vec![(0, raw_node("revision", &[], vec![RawNode::Text(Arc::from("3"))]))]),
        )],
        windows: None,
    }));
    // The parts of the package: a macro project with its relationship, an embedding, a part of custom XML, the thumbnail (which is no loss), and two parts that hold equal bytes.
    for part in [
        vsdx_part(
            "/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            PartRole::MacroProject,
            vec![rel("/vbaProject.bin", "rId1", "http://x/vbaData", "vbaData.xml")],
            b"MACRO PROJECT BYTES",
        ),
        vsdx_part(
            "/visio/embeddings/oleObject1.bin",
            "application/vnd.openxmlformats-officedocument.oleObject",
            PartRole::Embedding,
            Vec::new(),
            &crate::write_core::PNG,
        ),
        vsdx_part("/customXml/item1.xml", "application/xml", PartRole::CustomXml, Vec::new(), b"<item/>"),
        vsdx_part("/docProps/thumbnail.png", "image/png", PartRole::Thumbnail, Vec::new(), &crate::write_core::PNG),
        vsdx_part(
            "/visio/embeddings/oleObject2.bin",
            "application/vnd.openxmlformats-officedocument.oleObject",
            PartRole::Embedding,
            Vec::new(),
            &crate::write_core::PNG,
        ),
    ] {
        doc.parts.push(part);
    }
    doc
}

// ------------------------------------------------------------------------------------------------------------------------------ what of the model is VSDX's

fn side<T: Default + Clone + PartialEq + std::fmt::Debug>(label: &str, slot: &diacraft_doc::Ext<T>, out: &mut Vec<String>) {
    if let Some(d) = slot.get()
        && (!d.vsdx.0.is_empty() || d.native != Kept::default())
    {
        out.push(format!("{label} slot: {:?} / {:?}", d.vsdx, d.native));
    }
}

fn cell_data(label: &str, name: &str, cell: &Cell, out: &mut Vec<String>) {
    if !cell.raw.is_empty() {
        out.push(format!("{label} cell {name} raw: {:?}", cell.raw));
    }
    if let Some(extra) = &cell.extra {
        out.push(format!("{label} cell {name} extra: {:?} {:?}", extra.text, extra.passthrough));
        side(&format!("{label} cell {name} extra"), &extra.ext, out);
    }
}

fn sheet_data(label: &str, sheet: &diacraft_doc::Sheet, out: &mut Vec<String>) {
    if !sheet.raw.is_empty() {
        out.push(format!("{label} sheet raw: {:?}", sheet.raw));
    }
    if !sheet.passthrough.is_empty() {
        out.push(format!("{label} sheet passthrough: {:?}", sheet.passthrough));
    }
    side(&format!("{label} sheet"), &sheet.ext, out);
    for (name, cell) in &sheet.cells {
        cell_data(label, name.as_str(), cell, out);
    }
    for (i, s) in sheet.sections.iter().enumerate() {
        let sl = format!("{label} section {}.{:?}", s.name.as_str(), s.ix);
        let _ = i;
        if s.del {
            out.push(format!("{sl} del"));
        }
        if !s.raw.is_empty() {
            out.push(format!("{sl} raw: {:?}", s.raw));
        }
        if !s.passthrough.is_empty() {
            out.push(format!("{sl} passthrough: {:?}", s.passthrough));
        }
        side(&sl, &s.ext, out);
        for (name, cell) in &s.cells {
            cell_data(&sl, name.as_str(), cell, out);
        }
        for (r, row) in s.rows.iter().enumerate() {
            let rl = format!("{sl} row {r}");
            if row.del {
                out.push(format!("{rl} del"));
            }
            if row.local_name.is_some() {
                out.push(format!("{rl} local name {:?}", row.local_name));
            }
            if !row.raw.is_empty() {
                out.push(format!("{rl} raw: {:?}", row.raw));
            }
            if !row.passthrough.is_empty() {
                out.push(format!("{rl} passthrough: {:?}", row.passthrough));
            }
            side(&rl, &row.ext, out);
            for (name, cell) in &row.cells {
                cell_data(&rl, name.as_str(), cell, out);
            }
        }
    }
}

fn shapes_data(label: &str, list: &diacraft_doc::ShapeList, out: &mut Vec<String>) {
    for s in list.iter() {
        let l = format!("{label} shape {}", s.id);
        out.push(format!("{l} kind: {:?}", s.kind));
        if !s.raw.is_empty() {
            out.push(format!("{l} raw: {:?}", s.raw));
        }
        if !s.passthrough.is_empty() {
            out.push(format!("{l} passthrough: {:?}", s.passthrough));
        }
        if s.del {
            out.push(format!("{l} del"));
        }
        for (n, d) in [&s.data1, &s.data2, &s.data3].into_iter().enumerate() {
            if let Some(d) = d {
                out.push(format!("{l} data{}: {d}", n + 1));
            }
        }
        side(&l, &s.ext, out);
        if let Some(f) = &s.foreign {
            out.push(format!(
                "{l} foreign: {:?} {:?} {:?} {:?} picture {:?}",
                f.attrs,
                f.rel_id,
                f.text,
                f.passthrough,
                f.picture.as_ref().map(|p| (p.bytes.len(), p.content_type.to_string()))
            ));
            side(&format!("{l} foreign"), &f.ext, out);
        }
        sheet_data(&l, &s.sheet, out);
        if let Some(t) = &s.text {
            side(&format!("{l} text"), &t.ext, out);
            for (n, part) in t.parts.iter().enumerate() {
                if matches!(part, diacraft_doc::TextPart::Raw(_)) {
                    out.push(format!("{l} text part {n}: {part:?}"));
                }
            }
        }
        shapes_data(&l, &s.children, out);
    }
}

/// What of the model is VSDX's, in each place it can be held (the `vsdx` and `native` sides of the slots, the fields no other format has, the parts and the properties of the package): the
/// lines a trip through an `.odg` must give back. The `odf` side is the `.odg` reader's, and what it rebuilds from the markup is no part of this.
pub(crate) fn vsdx_data_of(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    side("doc", &doc.ext, &mut out);
    if !doc.passthrough.is_empty() {
        out.push(format!("doc passthrough: {:?}", doc.passthrough));
    }
    if let Some(v) = &doc.vsdx {
        let mut held = (**v).clone();
        // The windows part is DiaCraft's document file's own (the window settings).
        out.push(format!("doc held: {:?} {:?} {:?} windows {:?}", held.rels, held.content_types, held.props_rest, held.windows.take()));
    }
    for (i, f) in doc.fonts.iter().enumerate() {
        if !f.raw.is_empty() {
            out.push(format!("font {i} raw {:?}", f.raw));
        }
        side(&format!("font {i}"), &f.ext, &mut out);
    }
    for p in doc.parts.iter().filter(|p| p.format != Format::Odf && p.role != PartRole::Thumbnail) {
        out.push(format!("part {:?} {} {} {:?} {:?} {:?}", p.format, p.name, p.media_type, p.role, p.rels, &p.bytes[..]));
    }
    for c in doc.props.custom.iter().filter(|c| c.format != Format::Odf) {
        out.push(format!("custom {c:?}"));
    }
    for (name, v) in [
        ("category", &doc.props.category),
        ("company", &doc.props.company),
        ("manager", &doc.props.manager),
        ("hyperlink base", &doc.props.hyperlink_base),
    ] {
        if let Some(v) = v {
            out.push(format!("props {name}: {v}"));
        }
    }
    for p in doc.pages.iter() {
        let l = format!("page {}", p.id);
        out.push(format!("{l} fields: {:?} {:?} {:?} {:?} {:?}", p.name_u, p.background, p.back_page, p.rel_id, p.part_name));
        if !p.raw.is_empty() {
            out.push(format!("{l} raw: {:?}", p.raw));
        }
        if !p.passthrough.is_empty() {
            out.push(format!("{l} passthrough: {:?}", p.passthrough));
        }
        side(&l, &p.ext, &mut out);
        sheet_data(&l, &p.sheet, &mut out);
        for (i, c) in p.connects.iter().enumerate() {
            if !c.raw.is_empty() {
                out.push(format!("{l} connect {i} raw {:?}", c.raw));
            }
            side(&format!("{l} connect {i}"), &c.ext, &mut out);
        }
        shapes_data(&l, &p.shapes, &mut out);
    }
    for m in doc.masters.iter() {
        let l = format!("master {}", m.id);
        out.push(format!(
            "{l} fields: {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            m.name, m.name_u, m.unique_id, m.pattern_flags, m.rel_id, m.icon, m.part_name
        ));
        if !m.raw.is_empty() {
            out.push(format!("{l} raw: {:?}", m.raw));
        }
        if !m.passthrough.is_empty() {
            out.push(format!("{l} passthrough: {:?}", m.passthrough));
        }
        side(&l, &m.ext, &mut out);
        sheet_data(&l, &m.sheet, &mut out);
        shapes_data(&l, &m.shapes, &mut out);
    }
    for s in doc.styles.iter() {
        let l = format!("style {}", s.id);
        if !s.raw.is_empty() {
            out.push(format!("{l} raw: {:?}", s.raw));
        }
        if !s.passthrough.is_empty() {
            out.push(format!("{l} passthrough: {:?}", s.passthrough));
        }
        side(&l, &s.ext, &mut out);
        sheet_data(&l, &s.sheet, &mut out);
    }
    out
}

/// The two lists of data are the same, or the entries only one of them has (cut), for a message.
pub(crate) fn assert_same_data(got: &[String], wanted: &[String], what: &str) {
    let cut = |s: &String| s.chars().take(400).collect::<String>();
    let missing: Vec<String> = wanted.iter().filter(|w| !got.contains(w)).map(cut).collect();
    let extra: Vec<String> = got.iter().filter(|g| !wanted.contains(g)).map(cut).collect();
    assert!(missing.is_empty() && extra.is_empty(), "{what}: missing after the trip:\n{missing:#?}\nextra after the trip:\n{extra:#?}");
    assert_eq!(got.len(), wanted.len(), "{what}");
}
