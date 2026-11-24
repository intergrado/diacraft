//! A proptest strategy of whole models, for the property tests of the codecs (`sheetxml::model`, and later the package and the format crates).
//!
//! It covers the rules of the codec's exactness (the design's 2.3): strings that hold U+0001, U+001F, U+FFFE, U+FFFF, carriage return, tab and line
//! feed, and the characters the XML writer escapes; text with leading and trailing blanks; `Some("")` beside `None`; a kept attribute named twice;
//! connects in any order; every kind of text part; a picture shared by several shapes, and equal bytes held by different `Arc`s; groups nested to
//! [`diacraft_doc::MAX_SHAPE_DEPTH`]; themes holding only their part name and bytes (the rest of a theme is derived from the bytes at open, and is
//! never stored); and text longer than an attribute may be.
//!
//! What it never makes is a state no reader or command makes and the derived `PartialEq` would still count as different from the model it stands
//! for: an empty [`RawAttrs`] or [`Passthrough`] that is not the null one, and a shape type that is `Other` of a word the model has a variant for.

use std::sync::Arc;

use crate::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder};
use diacraft_doc::{
    Cell, CellExtra, ColorEntry, Connect, Connects, ContentTypeRow, CustomProp, DefFamily, Definition, DocProps, Document, Ext, FaceName,
    ForeignData, Format, FormulaSlot, Held, HeldGroup, HeldStyleValue, Kept, KeptStyle, Links, MAX_SHAPE_DEPTH, Master, Media, OdfDoc, OdfListLevel,
    OdfPage, OdfParagraph, OdfRow, OdfRun, OdfShape, OdfStyle, OdfText, OdfTextStyle, Page, Part, PartRole, Passthrough, Placed, RawAttrs,
    RawElement, RawNode, Rel, Row, Rows, Section, SectionName, Shape, ShapeList, ShapeType, Sheet, Stamp, Style, Text, TextGroup, TextPart, Theme,
    Value, VsdxHeld, VsdxRest,
};
use proptest::prelude::*;

/// How big a model the strategy makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSize {
    /// A page or two of a few shapes: the size of a property test that runs thousands of cases.
    Small,
    /// More pages, masters, styles and shapes.
    Medium,
}

/// The characters that break an encoding: those XML forbids, the ones its writer escapes, blanks, and some beyond the first plane.
fn special_char() -> BoxedStrategy<char> {
    prop_oneof![
        Just('\u{1}'),
        Just('\u{1f}'),
        Just('\u{0}'),
        Just('\u{fffe}'),
        Just('\u{ffff}'),
        Just('\r'),
        Just('\t'),
        Just('\n'),
        Just('&'),
        Just('<'),
        Just('>'),
        Just('"'),
        Just('\''),
        Just(' '),
        Just('é'),
        Just('😀'),
        Just('\u{7f}'),
        Just('\u{85}'),
        Just(']'),
        Just('-'),
        Just('?'),
        any::<char>(),
    ]
    .boxed()
}

/// Text of every kind the codec must give back. Long text, which is written beside the attribute cap and past it, is rare, so that a model stays small.
pub fn any_text() -> BoxedStrategy<String> {
    let small = prop_oneof![
        24 => "[a-zA-Z0-9 ._-]{0,12}",
        6 => prop::collection::vec(special_char(), 0..8).prop_map(String::from_iter),
        2 => Just(String::new()),
        2 => Just("  leading and trailing  ".to_owned()),
        2 => Just("a\tb\nc\rd".to_owned()),
        2 => Just("\u{1}\u{1f}\u{fffe}\u{ffff}".to_owned()),
        2 => Just("--".to_owned()),
        2 => Just("]]>".to_owned()),
    ];
    // Past the length an attribute may be, plain and with what escaping spells out as several bytes each.
    let long = prop_oneof![Just("x".repeat(16 * 1024 + 1)), Just("\"&<\n".repeat(5_000)), Just(format!("{}\u{1}", "y".repeat(16 * 1024))),];
    prop_oneof![200 => small, 1 => long].boxed()
}

fn any_arc_text() -> BoxedStrategy<Arc<str>> {
    any_text().prop_map(|s| Arc::from(s.as_str())).boxed()
}

fn opt_text() -> BoxedStrategy<Option<Arc<str>>> {
    prop_oneof![1 => Just(None), 3 => any_arc_text().prop_map(Some)].boxed()
}

fn opt_u32() -> BoxedStrategy<Option<u32>> {
    prop_oneof![1 => Just(None), 3 => any_u32().prop_map(Some)].boxed()
}

fn any_u32() -> BoxedStrategy<u32> {
    prop_oneof![4 => 0u32..8, 1 => Just(u32::MAX), 1 => any::<u32>()].boxed()
}

/// A name of a cell, section or row: not empty, and not longer than the model allows.
fn name_text() -> BoxedStrategy<String> {
    prop_oneof![
        4 => prop::sample::select(vec!["PinX", "PinY", "Width", "Height", "LineWeight", "Geometry", "Character", "User", "Prop.Row_1"]).prop_map(str::to_owned),
        2 => "[A-Za-z][A-Za-z0-9_.]{0,10}",
        1 => any_text().prop_filter("a name is not empty or long", |s| !s.is_empty() && s.len() <= diacraft_doc::MAX_NAME_BYTES),
    ]
    .boxed()
}

fn raw_attrs() -> BoxedStrategy<RawAttrs> {
    prop::collection::vec((any_text(), any_text()), 0..4).prop_map(RawAttrs::from_pairs).boxed()
}

/// A kept attribute named twice, and a kept attribute with the name of a typed one.
fn raw_attrs_with_clashes() -> BoxedStrategy<RawAttrs> {
    prop_oneof![
        6 => raw_attrs(),
        1 => Just(RawAttrs::from_pairs([("x", "1"), ("x", "2")])),
        1 => Just(RawAttrs::from_pairs([("ID", "007"), ("IX", "+3"), ("Del", "true")])),
    ]
    .boxed()
}

fn raw_node(depth: u32) -> BoxedStrategy<RawNode> {
    let leaf = prop_oneof![
        any_arc_text().prop_map(RawNode::Text),
        any_arc_text().prop_map(RawNode::Comment),
        (any_arc_text(), any_arc_text()).prop_map(|(target, data)| RawNode::Pi { target, data }),
    ];
    leaf.prop_recursive(depth, 12, 3, |inner| {
        (any_arc_text(), raw_attrs(), prop::collection::vec(inner, 0..3))
            .prop_map(|(name, attrs, children)| RawNode::Element(RawElement { name, attrs, children }))
    })
    .boxed()
}

fn passthrough() -> BoxedStrategy<Passthrough> {
    prop_oneof![
        3 => Just(Passthrough::new()),
        1 => prop::collection::vec((0u32..6, raw_node(2)), 1..3).prop_map(|nodes| Passthrough::from_nodes(nodes.into_iter().map(|(after, node)| Placed { after, node }).collect())),
    ]
    .boxed()
}

fn raw_element() -> BoxedStrategy<RawElement> {
    (any_arc_text(), raw_attrs(), prop::collection::vec(raw_node(2), 0..3))
        .prop_map(|(name, attrs, children)| RawElement { name, attrs, children })
        .boxed()
}

/// A formula's text: ones that parse, ones that do not, and the two markers' words, which a formula of its own must not be read back as.
fn formula_source() -> BoxedStrategy<String> {
    prop_oneof![
        4 => prop::sample::select(vec!["Width*0.5", "SUM(1,2)", "Sheet.1!PinX", "=1+1", "IF(Width>1,\"a\",\"b\")", "((", "1 2", ""]).prop_map(str::to_owned),
        1 => Just("Inh".to_owned()),
        1 => Just("No Formula".to_owned()),
        2 => any_text(),
    ]
    .boxed()
}

fn formula_slot() -> BoxedStrategy<FormulaSlot> {
    prop_oneof![
        3 => Just(FormulaSlot::Absent),
        1 => Just(FormulaSlot::Inherit),
        1 => Just(FormulaSlot::NoFormula),
        4 => formula_source().prop_map(|s| Cell::with_formula(&s, None).formula),
    ]
    .boxed()
}

fn cell_extra() -> BoxedStrategy<Option<Arc<CellExtra>>> {
    prop_oneof![
        4 => Just(None),
        // An empty extra is not an absent one.
        1 => Just(Some(Arc::new(CellExtra::default()))),
        2 => (opt_text(), passthrough(), ext_of(kept().boxed())).prop_map(|(text, passthrough, ext)| Some(Arc::new(CellExtra { text, passthrough, ext }))),
        // An extra that holds only what a newer build wrote on the cell, which has no element of its own.
        1 => ext_of(kept().boxed()).prop_map(|ext| Some(Arc::new(CellExtra { text: None, passthrough: Passthrough::new(), ext }))),
    ]
    .boxed()
}

fn cell() -> BoxedStrategy<Cell> {
    (
        formula_slot(),
        prop_oneof![1 => Just(None), 3 => any_text().prop_map(|t| Some(Value::from_text(&t)))],
        opt_text(),
        opt_text(),
        raw_attrs_with_clashes(),
        cell_extra(),
    )
        .prop_map(|(formula, value, unit, error, raw, extra)| Cell { formula, value, unit, error, raw, extra })
        .boxed()
}

fn cells() -> BoxedStrategy<diacraft_doc::Cells> {
    prop::collection::vec((name_text(), cell()), 0..4)
        .prop_map(|v| v.into_iter().map(|(n, c)| (diacraft_doc::CellName::new(&n), c)).collect())
        .boxed()
}

fn row() -> BoxedStrategy<Row> {
    (
        prop_oneof![1 => Just(None), 2 => name_text().prop_map(|s| Some(Arc::from(s.as_str())))],
        opt_u32(),
        opt_text(),
        opt_text(),
        any::<bool>(),
        cells(),
        raw_attrs(),
        passthrough(),
        row_ext(),
    )
        .prop_map(|(name, ix, local_name, row_type, del, cells, raw, passthrough, ext)| Row {
            name,
            ix,
            local_name,
            row_type,
            del,
            cells,
            raw,
            passthrough,
            ext,
        })
        .boxed()
}

fn section() -> BoxedStrategy<Section> {
    (
        name_text(),
        opt_u32(),
        any::<bool>(),
        cells(),
        prop::collection::vec(row().prop_map(Arc::new), 0..3),
        raw_attrs(),
        passthrough(),
        ext_of(kept().boxed()),
    )
        .prop_map(|(name, ix, del, cells, rows, raw, passthrough, ext)| Section {
            name: SectionName::new(&name),
            ix,
            del,
            cells,
            rows: Rows::from(rows),
            raw,
            passthrough,
            ext,
        })
        .boxed()
}

/// The sections a model shares: a path, equal to another held by an `Arc` of its own, and a section of user rows. A decode keeps one copy of the
/// sections it reads that are equal, so a model in which one is in several places, each its own `Arc` or all one, must come back equal.
fn section_pool() -> Vec<Arc<Section>> {
    let mut geometry = Section::new("Geometry", Some(0));
    for (ix, (kind, x, y)) in [("RelMoveTo", "0", "0"), ("RelLineTo", "1", "0"), ("RelLineTo", "1", "1")].iter().enumerate() {
        let row = Row {
            ix: Some(u32::try_from(ix + 1).unwrap_or(0)),
            row_type: Some(Arc::from(*kind)),
            cells: [("X", *x), ("Y", *y)].iter().map(|(n, v)| (diacraft_doc::CellName::new(n), Cell::constant(Value::from_text(v)))).collect(),
            ..Row::default()
        };
        geometry.rows.push(Arc::new(row));
    }
    let mut user = Section::new("User", None);
    let row = Row {
        name: Some(Arc::from("r")),
        cells: [("Value", Cell::with_formula("1+1", Some(Value::from_text("2"))))]
            .into_iter()
            .map(|(n, c)| (diacraft_doc::CellName::new(n), c))
            .collect(),
        ..Row::default()
    };
    user.rows.push(Arc::new(row));
    vec![Arc::new(geometry.clone()), Arc::new(geometry), Arc::new(user)]
}

/// The sheets a model shares: an empty one with links, one with a cell and a section of the pool, and a copy of it held by an `Arc` of its own.
fn sheet_pool() -> Vec<Arc<Sheet>> {
    let sections = section_pool();
    let mut pinned = Sheet { links: Links { line: Some(1), fill: Some(2), text: None }, ..Sheet::default() };
    pinned.cells.insert(diacraft_doc::CellName::new("PinX"), Cell::constant(Value::from_text("1.5")));
    pinned.sections = vec![Arc::clone(&sections[0]), Arc::clone(&sections[2])];
    let links_only = Sheet { links: Links { line: Some(3), fill: None, text: Some(4) }, ..Sheet::default() };
    vec![Arc::new(pinned.clone()), Arc::new(pinned), Arc::new(links_only)]
}

/// A section: a new one, or one of the pool that other places of the model hold too.
fn pooled_section() -> BoxedStrategy<Arc<Section>> {
    prop_oneof![3 => section().prop_map(Arc::new), 2 => prop::sample::select(section_pool())].boxed()
}

/// A sheet, as often empty (the shared default) as not.
pub fn any_sheet() -> BoxedStrategy<Sheet> {
    prop_oneof![
        2 => Just(Sheet::default()),
        3 => (
            (opt_u32(), opt_u32(), opt_u32()),
            cells(),
            prop::collection::vec(pooled_section(), 0..3),
            raw_attrs(),
            passthrough(),
            ext_of(kept().boxed()),
        )
            .prop_map(|((line, fill, text), cells, sections, raw, passthrough, ext)| Sheet { links: Links { line, fill, text }, cells, sections, raw, passthrough, ext }),
    ]
    .boxed()
}

fn arc_sheet() -> BoxedStrategy<Arc<Sheet>> {
    // A sheet of its own, or one of the pool that other shapes hold too (equal by value, and some held by one `Arc`): what a decode shares.
    prop_oneof![3 => any_sheet().prop_map(Arc::new), 2 => prop::sample::select(sheet_pool())].boxed()
}

/// The byte strings the model's pictures and themes are taken from: shared `Arc`s, equal bytes in different `Arc`s, and an empty one.
fn byte_pool() -> Vec<Arc<[u8]>> {
    vec![
        Arc::from(&[][..]),
        Arc::from(&b"\x89PNG\r\n\x1a\n pixels"[..]),
        Arc::from(&b"\x89PNG\r\n\x1a\n pixels"[..]),
        Arc::from(&[0u8, 1, 2, 255, 254][..]),
        Arc::from(vec![7u8; 300]),
    ]
}

fn media(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<Arc<Media>> {
    (any_arc_text(), any_arc_text(), prop::sample::select(pool))
        .prop_map(|(part, content_type, bytes)| Arc::new(Media { part, content_type, bytes }))
        .boxed()
}

fn foreign(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<ForeignData> {
    (raw_attrs(), opt_text(), prop_oneof![1 => Just(None), 2 => media(pool).prop_map(Some)], opt_text(), passthrough(), ext_of(kept().boxed()))
        .prop_map(|(attrs, rel_id, picture, text, passthrough, ext)| ForeignData { attrs, rel_id, picture, text, passthrough, ext })
        .boxed()
}

fn text_part() -> BoxedStrategy<TextPart> {
    prop_oneof![
        4 => any_arc_text().prop_map(TextPart::Chars),
        1 => any_u32().prop_map(TextPart::Cp),
        1 => any_u32().prop_map(TextPart::Pp),
        1 => any_u32().prop_map(TextPart::Tp),
        2 => (any_u32(), any_arc_text()).prop_map(|(ix, text)| TextPart::Fld { ix, text }),
        1 => raw_node(1).prop_map(TextPart::Raw),
        1 => raw_node(1).prop_map(|n| TextPart::OdfRaw(Box::new(n))),
        // What a newer build wrote inside the text: an element, which no part of ours is, or a text, a comment or an instruction (kept in data form).
        1 => prop_oneof![
            3 => native_element().prop_map(RawNode::Element),
            1 => any_arc_text().prop_map(RawNode::Text),
            1 => any_arc_text().prop_map(RawNode::Comment),
            1 => (any_arc_text(), any_arc_text()).prop_map(|(target, data)| RawNode::Pi { target, data }),
        ]
        .prop_map(|n| TextPart::NativeRaw(Box::new(n))),
    ]
    .boxed()
}

fn text() -> BoxedStrategy<Option<Arc<Text>>> {
    prop_oneof![
        3 => Just(None),
        2 => (prop::collection::vec(text_part(), 0..6), text_ext()).prop_map(|(parts, ext)| Some(Arc::new(Text { parts, ext }))),
    ]
    .boxed()
}

fn shape_kind() -> BoxedStrategy<ShapeType> {
    prop_oneof![
        1 => Just(ShapeType::Shape),
        1 => Just(ShapeType::Group),
        1 => Just(ShapeType::Foreign),
        1 => Just(ShapeType::Guide),
        1 => Just(ShapeType::Unspecified),
        // A word the model has no variant for: the reader never makes `Other` of one it has.
        1 => any_text().prop_filter("a word the model has a variant for", |s| !matches!(s.as_str(), "Shape" | "Group" | "Foreign" | "Guide")).prop_map(|s| ShapeType::Other(Arc::from(s.as_str()))),
    ]
    .boxed()
}

/// A shape with no members.
fn leaf_shape(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<Shape> {
    (
        (any_u32(), opt_text(), opt_text(), shape_kind(), opt_u32(), opt_u32(), any::<bool>()),
        arc_sheet(),
        text(),
        (opt_text(), opt_text(), opt_text()),
        prop_oneof![3 => Just(None), 1 => foreign(pool).prop_map(|f| Some(Arc::new(f)))],
        raw_attrs_with_clashes(),
        passthrough(),
        shape_ext(),
    )
        .prop_map(|((id, name, name_u, kind, master, master_shape, del), sheet, text, (data1, data2, data3), foreign, raw, passthrough, ext)| Shape {
            id,
            name,
            name_u,
            kind,
            master,
            master_shape,
            del,
            sheet,
            text,
            data1,
            data2,
            data3,
            foreign,
            children: ShapeList::new(),
            raw,
            passthrough,
            ext,
        })
        .boxed()
}

/// A shape that may hold members, to a few levels.
fn any_shape(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<Shape> {
    let leaf = leaf_shape(pool.clone());
    leaf.prop_recursive(2, 10, 3, move |inner| {
        (leaf_shape(pool.clone()), prop::collection::vec(inner.prop_map(Arc::new), 1..3)).prop_map(|(mut group, members)| {
            group.children = ShapeList::from_vec(members);
            group
        })
    })
    .boxed()
}

/// A chain of `depth` shapes, each the only member of the one before it, the innermost holding `innermost`.
pub fn deep_chain(depth: usize, innermost: Shape) -> Shape {
    let mut shape = innermost;
    for level in (1..depth).rev() {
        let mut parent = Shape::new(u32::try_from(level).unwrap_or(u32::MAX));
        parent.kind = ShapeType::Group;
        parent.children = ShapeList::from_vec(vec![Arc::new(shape)]);
        shape = parent;
    }
    shape
}

/// A shape tree nested to the model's cap, which is the deepest the codec holds.
pub fn deepest_shape() -> Shape {
    deep_chain(MAX_SHAPE_DEPTH, Shape::new(9_999))
}

fn shapes(pool: Vec<Arc<[u8]>>, most: usize) -> BoxedStrategy<ShapeList> {
    prop_oneof![
        15 => prop::collection::vec(any_shape(pool).prop_map(Arc::new), 0..most),
        // The tree at the model's cap of depth, now and then.
        1 => Just(vec![Arc::new(deepest_shape())]),
    ]
    .prop_map(ShapeList::from_vec)
    .boxed()
}

fn connect() -> BoxedStrategy<Connect> {
    (any_u32(), opt_text(), opt_u32(), any_u32(), opt_text(), opt_u32(), raw_attrs(), ext_of(kept().boxed()))
        .prop_map(|(from_sheet, from_cell, from_part, to_sheet, to_cell, to_part, raw, ext)| Connect {
            from_sheet,
            from_cell,
            from_part,
            to_sheet,
            to_cell,
            to_part,
            raw,
            ext,
        })
        .boxed()
}

fn connects() -> BoxedStrategy<Connects> {
    prop::collection::vec(connect(), 0..4).prop_map(Connects::from_vec).boxed()
}

fn page(pool: Vec<Arc<[u8]>>, most_shapes: usize) -> BoxedStrategy<Page> {
    (
        // A page ID is often used twice by a hostile file, and the codec must still name each page's part apart.
        (0u32..4, any_arc_text(), opt_text(), any::<bool>(), opt_u32(), opt_text(), opt_text()),
        arc_sheet(),
        shapes(pool, most_shapes),
        connects(),
        raw_attrs(),
        passthrough(),
        ext_of(odf_page().boxed()),
    )
        .prop_map(|((id, name, name_u, background, back_page, rel_id, part_name), sheet, shapes, connects, raw, passthrough, ext)| Page {
            id,
            name,
            name_u,
            background,
            back_page,
            rel_id,
            sheet,
            shapes,
            connects,
            raw,
            passthrough,
            ext,
            part_name,
        })
        .boxed()
}

fn master(pool: Vec<Arc<[u8]>>, most_shapes: usize) -> BoxedStrategy<Master> {
    (
        (0u32..4, opt_text(), opt_text(), opt_text(), opt_u32(), opt_text(), opt_text(), opt_text()),
        arc_sheet(),
        shapes(pool, most_shapes),
        connects(),
        raw_attrs(),
        passthrough(),
        ext_of(kept().boxed()),
    )
        .prop_map(|((id, name, name_u, unique_id, pattern_flags, rel_id, icon, part_name), sheet, shapes, connects, raw, passthrough, ext)| Master {
            id,
            name,
            name_u,
            unique_id,
            pattern_flags,
            rel_id,
            icon,
            sheet,
            shapes,
            connects,
            raw,
            passthrough,
            ext,
            part_name,
        })
        .boxed()
}

fn style() -> BoxedStrategy<Style> {
    (any_u32(), opt_text(), opt_text(), arc_sheet(), raw_attrs(), passthrough(), ext_of(odf_style().boxed()))
        .prop_map(|(id, name, name_u, sheet, raw, passthrough, ext)| Style { id, name, name_u, sheet, raw, passthrough, ext })
        .boxed()
}

fn theme(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<Theme> {
    // Only the part name and the bytes: the colours, fonts and entries of a theme are derived from its bytes when a document is opened.
    (any_arc_text(), prop::sample::select(pool)).prop_map(|(part, bytes)| Theme { part, bytes, ..Theme::default() }).boxed()
}

fn props() -> BoxedStrategy<DocProps> {
    (
        (opt_text(), opt_text(), opt_text(), opt_text(), opt_text(), opt_text(), opt_text()),
        (opt_text(), opt_text(), opt_text(), opt_text(), opt_text(), opt_text(), opt_text()),
        custom_props(),
    )
        .prop_map(
            |(
                (subject, creator, last_modified_by, keywords, description, category, company),
                (manager, hyperlink_base, app_version, created, modified, last_printed, language),
                custom,
            )| DocProps {
                subject,
                creator,
                last_modified_by,
                keywords,
                description,
                category,
                company,
                manager,
                hyperlink_base,
                app_version,
                created,
                modified,
                last_printed,
                language,
                custom,
            },
        )
        .boxed()
}

/// A whole document, of the given size.
pub fn any_document(size: ModelSize) -> BoxedStrategy<Document> {
    let (pages, masters, styles, most_shapes) = match size {
        ModelSize::Small => (0..3usize, 0..3usize, 0..3usize, 3usize),
        ModelSize::Medium => (0..6, 0..6, 0..6, 8),
    };
    let pool = byte_pool();
    let head = (any_u32(), any_arc_text(), prop_oneof![1 => Just(DocProps::default()), 2 => props()]);
    let lists = (
        prop::collection::vec(page(pool.clone(), most_shapes).prop_map(Arc::new), pages),
        prop::collection::vec(master(pool.clone(), most_shapes).prop_map(Arc::new), masters),
        prop::collection::vec(style().prop_map(Arc::new), styles),
        prop::collection::vec(theme(pool).prop_map(Arc::new), 0..3),
    );
    let tables = (
        prop::collection::vec((any_u32(), any_arc_text()).prop_map(|(ix, rgb)| ColorEntry { ix, rgb }), 0..3),
        prop::collection::vec(
            (any_arc_text(), raw_attrs(), ext_of(kept().boxed())).prop_map(|(name_u, raw, ext)| FaceName { name_u, raw, ext }),
            0..3,
        ),
        arc_sheet(),
        passthrough(),
        prop_oneof![2 => Just(None), 1 => raw_element().prop_map(|e| Some(Arc::new(e)))],
    );
    let package = (
        ext_of(odf_doc(byte_pool()).boxed()),
        parts(),
        prop_oneof![2 => Just(None), 1 => vsdx_held().prop_map(|h| Some(Arc::new(h)))],
        prop::collection::vec(any::<bool>(), 96),
    );
    (head, lists, tables, package)
        .prop_map(
            |(
                (version, title, props),
                (pages, masters, styles, themes),
                (colors, fonts, sheet, passthrough, diacraft),
                (ext, parts, vsdx, picks),
            )| {
                let mut doc = Document::empty();
                doc.version = version;
                doc.title = title;
                doc.props = Arc::new(props);
                doc.pages = pages.into_iter().collect();
                doc.masters = masters.into_iter().collect();
                doc.styles = styles.into_iter().collect();
                doc.themes = themes.into_iter().collect();
                doc.colors = Arc::new(colors);
                doc.fonts = Arc::new(fonts);
                doc.sheet = sheet;
                doc.passthrough = passthrough;
                doc.diacraft = diacraft;
                doc.ext = ext;
                doc.parts = parts.into_iter().map(Arc::new).collect();
                doc.vsdx = vsdx;
                add_keyed(&mut doc, &picks);
                doc
            },
        )
        .boxed()
}

// ---------------------------------------------------------------------------------------------------------------------- the slots and the package

/// Attributes and nodes the model keeps under a tag.
fn kept() -> BoxedStrategy<Kept> {
    (raw_attrs(), passthrough()).prop_map(|(raw, kept)| Kept { raw, kept }).boxed()
}

/// Text that is the literal XML of what a newer build wrote: it holds no character XML forbids.
fn xml_text() -> BoxedStrategy<String> {
    prop_oneof![
        4 => "[a-zA-Z0-9 ._-]{1,8}",
        1 => Just("a & b < c > d \"e\"".to_owned()),
        1 => Just("  padded\n\r\t ".to_owned()),
    ]
    .boxed()
}

/// Adjacent texts read back as one and an empty one is not read back, so no list of nodes read from XML has either.
fn merge_texts(nodes: Vec<RawNode>) -> Vec<RawNode> {
    let mut out: Vec<RawNode> = Vec::new();
    for node in nodes {
        if let (Some(RawNode::Text(last)), RawNode::Text(next)) = (out.last_mut(), &node) {
            *last = Arc::from(format!("{last}{next}").as_str());
            continue;
        }
        out.push(node);
    }
    out.retain(|n| !matches!(n, RawNode::Text(t) if t.is_empty()));
    out
}

/// An element with no children that a newer build wrote: in a namespace of its own, which it declares itself, and with attributes in it.
fn native_head() -> BoxedStrategy<RawElement> {
    (1u32..4, "[a-z]{1,6}", prop::collection::vec(("[a-z]{1,5}", xml_text()), 0..3))
        .prop_map(|(n, local, attrs)| {
            let prefix = format!("q{n}");
            let mut pairs: Vec<(String, String)> = vec![(format!("xmlns:{prefix}"), format!("urn:test:{prefix}"))];
            pairs.extend(attrs.into_iter().enumerate().map(|(i, (a, v))| (format!("{prefix}:{a}{i}"), v)));
            RawElement { name: Arc::from(format!("{prefix}:{local}").as_str()), attrs: RawAttrs::from_pairs(pairs), children: Vec::new() }
        })
        .boxed()
}

/// An element a newer build wrote, with elements, texts and comments inside it.
fn native_element() -> BoxedStrategy<RawElement> {
    native_head()
        .prop_recursive(2, 8, 2, |inner| {
            let child = prop_oneof![
                inner.prop_map(RawNode::Element),
                xml_text().prop_map(|t| RawNode::Text(Arc::from(t.as_str()))),
                "[a-z]{0,5}".prop_map(|c| RawNode::Comment(Arc::from(c.as_str()))),
            ];
            (native_head(), prop::collection::vec(child, 0..3)).prop_map(|(mut e, children)| {
                e.children = merge_texts(children);
                e
            })
        })
        .boxed()
}

/// What a newer build wrote on an object, as the codec reads it: attributes in no vocabulary (and one prefixed pair, with its declaration before it), and
/// elements, each with the declarations its names use.
fn native_kept() -> BoxedStrategy<Kept> {
    (
        prop::collection::vec(xml_text(), 0..3),
        prop_oneof![2 => Just(None), 1 => (xml_text(), xml_text()).prop_map(Some)],
        prop::collection::vec(native_element(), 0..2),
        // An attribute in the codec's own namespace that its vocabulary has no name for: it is written under the part's own declaration, which is not kept.
        prop_oneof![3 => Just(None), 1 => xml_text().prop_map(Some)],
    )
        .prop_map(|(plain, prefixed, elements, own)| {
            let mut pairs: Vec<(String, String)> = plain.into_iter().enumerate().map(|(i, v)| (format!("future-{i}"), v)).collect();
            if let Some(v) = own {
                pairs.push(("diacraft:future".to_owned(), v));
            }
            if let Some((a, b)) = prefixed {
                pairs.push(("xmlns:q9".to_owned(), "urn:test:q9".to_owned()));
                pairs.push(("q9:x".to_owned(), a));
                pairs.push(("q9:y".to_owned(), b));
            }
            let nodes = elements.into_iter().map(|e| Placed { after: 0, node: RawNode::Element(e) }).collect();
            Kept { raw: RawAttrs::from_pairs(pairs), kept: Passthrough::from_nodes(nodes) }
        })
        .boxed()
}

/// An extension slot: empty as often as not, and holding an object's ODF data, the VSDX elements it had no slot for and what a newer build wrote.
fn ext_of<T: Default + Clone + PartialEq + std::fmt::Debug + 'static>(odf: BoxedStrategy<T>) -> BoxedStrategy<Ext<T>> {
    prop_oneof![
        6 => Just(Ext::default()),
        3 => (odf, prop::collection::vec((any_arc_text(), kept()), 0..3), native_kept()).prop_map(|(odf, vsdx, native)| {
            let mut ext = Ext::default();
            let data = ext.make_mut();
            data.odf = odf;
            data.vsdx = VsdxRest(vsdx);
            data.native = native;
            ext
        }),
    ]
    .boxed()
}

fn stamp() -> BoxedStrategy<Stamp> {
    any::<u64>().prop_map(Stamp).boxed()
}

fn opt_element() -> BoxedStrategy<Option<Arc<RawElement>>> {
    prop_oneof![2 => Just(None), 1 => raw_element().prop_map(|e| Some(Arc::new(e)))].boxed()
}

fn odf_page() -> BoxedStrategy<OdfPage> {
    (kept(), opt_text(), opt_text(), raw_attrs(), raw_attrs(), opt_element(), raw_attrs(), prop_oneof![1 => Just(None), 1 => stamp().prop_map(Some)])
        .prop_map(|(kept, master_page, layout, master_page_attrs, layout_props, drawing_page_style, units, stamp)| OdfPage {
            kept,
            master_page,
            layout,
            master_page_attrs,
            layout_props,
            drawing_page_style,
            units,
            stamp,
        })
        .boxed()
}

fn odf_style() -> BoxedStrategy<OdfStyle> {
    (
        (kept(), opt_text(), opt_text(), opt_text(), opt_text(), raw_attrs(), raw_attrs()),
        (held_values(), prop::collection::vec(held_style_value(), 0..3)),
    )
        .prop_map(|((kept, name, display_name, parent, family, unmapped, units), (held, text_held))| OdfStyle {
            kept,
            name,
            display_name,
            parent,
            family,
            unmapped,
            units,
            held,
            text_held,
        })
        .boxed()
}

const HELD_GROUPS: [HeldGroup; 8] = [
    HeldGroup::Fill,
    HeldGroup::Line,
    HeldGroup::LineEnds,
    HeldGroup::Shadow,
    HeldGroup::Text,
    HeldGroup::Frame,
    HeldGroup::Glue,
    HeldGroup::Geometry,
];

const FAMILIES: [DefFamily; 6] =
    [DefFamily::Marker, DefFamily::Gradient, DefFamily::Opacity, DefFamily::Hatch, DefFamily::StrokeDash, DefFamily::FillImage];

fn group_word(g: HeldGroup) -> &'static str {
    match g {
        HeldGroup::Fill => "fill",
        HeldGroup::Line => "line",
        HeldGroup::LineEnds => "line-ends",
        HeldGroup::Shadow => "shadow",
        HeldGroup::Text => "text",
        HeldGroup::Frame => "frame",
        HeldGroup::Glue => "glue",
        HeldGroup::Geometry => "geometry",
    }
}

fn family_word(f: DefFamily) -> &'static str {
    match f {
        DefFamily::Marker => "marker",
        DefFamily::Gradient => "gradient",
        DefFamily::Opacity => "opacity",
        DefFamily::Hatch => "hatch",
        DefFamily::StrokeDash => "stroke-dash",
        DefFamily::FillImage => "fill-image",
    }
}

fn format_word(f: Format) -> &'static str {
    match f {
        Format::Vsdx => "vsdx",
        Format::Odf => "odf",
        Format::Native => "native",
    }
}

/// Up to three held values, of any groups and any two of one group (each has a stamp of its own, or none).
fn held_values() -> BoxedStrategy<Vec<Held>> {
    prop::collection::vec(one_held(), 0..4).boxed()
}

fn one_held() -> BoxedStrategy<Held> {
    (prop::sample::select(HELD_GROUPS.to_vec()), prop_oneof![1 => Just(None), 3 => stamp().prop_map(Some)], raw_attrs(), passthrough())
        .prop_map(|(group, stamp, attrs, kept)| Held { group, stamp, attrs, kept })
        .boxed()
}

fn odf_shape() -> BoxedStrategy<OdfShape> {
    (kept(), opt_text(), held_values(), raw_attrs()).prop_map(|(kept, style_name, held, units)| OdfShape { kept, style_name, held, units }).boxed()
}

fn odf_row() -> BoxedStrategy<OdfRow> {
    let one = one_held();
    (kept(), opt_text(), prop_oneof![1 => Just(None), 1 => one.prop_map(Some)])
        .prop_map(|(kept, style_name, held)| OdfRow { kept, style_name, held })
        .boxed()
}

/// What a record that has no slot keeps that this build does not know, as the document keeps it: under the record's kind and key (NP7).
fn keyed_entry(kind: &str, key: &str) -> RawNode {
    let note = RawElement {
        name: Arc::from("q1:note"),
        attrs: RawAttrs::from_pairs([("xmlns:q1", "urn:test:q1"), ("q1:a", "1")]),
        children: vec![RawNode::Text(Arc::from("text"))],
    };
    RawNode::Element(RawElement {
        name: Arc::from(format!("{kind}[{key}]").as_str()),
        attrs: RawAttrs::from_pairs([("future-k", "1")]),
        children: vec![RawNode::Element(note)],
    })
}

fn push_keyed<T: Default + Clone + PartialEq>(ext: &mut Ext<T>, kind: &str, key: &str) {
    ext.make_mut().native.kept.push(0, keyed_entry(kind, key));
}

/// The ODF slot of a shape, and the unknown content of some of its held values, kept under their groups in the shape's own native slot.
fn shape_ext() -> BoxedStrategy<Ext<OdfShape>> {
    (ext_of(odf_shape()), prop::collection::vec(any::<bool>(), 3))
        .prop_map(|(mut ext, picks)| {
            let groups: Vec<HeldGroup> = ext.get().map(|d| d.odf.held.iter().map(|h| h.group).collect()).unwrap_or_default();
            // Each value may keep unknown content, under its group and its ordinal among the values of that group (Call 24), in the order of the values.
            let mut seen: Vec<HeldGroup> = Vec::new();
            for (group, pick) in groups.into_iter().zip(picks) {
                let ordinal = seen.iter().filter(|g| **g == group).count();
                seen.push(group);
                if pick {
                    push_keyed(&mut ext, "held", &format!("{}.{ordinal}", group_word(group)));
                }
            }
            ext
        })
        .boxed()
}

fn opt_stamp() -> BoxedStrategy<Option<Stamp>> {
    prop_oneof![1 => Just(None), 3 => stamp().prop_map(Some)].boxed()
}

fn held_style_value() -> BoxedStrategy<HeldStyleValue> {
    (prop::sample::select(TextGroup::ALL.to_vec()), opt_stamp(), raw_attrs(), passthrough())
        .prop_map(|(group, stamp, attrs, kept)| HeldStyleValue { group, stamp, attrs, kept })
        .boxed()
}

/// A style a paragraph or run names: as often as not a default one, which is no element.
fn odf_text_style() -> BoxedStrategy<OdfTextStyle> {
    prop_oneof![
        2 => Just(OdfTextStyle::default()),
        3 => (opt_text(), opt_stamp(), prop::collection::vec(held_style_value(), 0..3)).prop_map(|(name, stamp, held)| OdfTextStyle { name, stamp, held }),
    ]
    .boxed()
}

fn odf_text() -> BoxedStrategy<OdfText> {
    let run = (any_u32(), any_u32(), odf_text_style(), opt_element()).prop_map(|(from, to, style, element)| OdfRun { from, to, style, element });
    let level = (opt_element(), opt_element()).prop_map(|(list, item)| OdfListLevel { list, item });
    let paragraph = (odf_text_style(), opt_stamp(), prop::collection::vec(run, 0..3), opt_element(), prop::collection::vec(level, 0..3))
        .prop_map(|(style, characters, runs, element, levels)| OdfParagraph { style, characters, runs, element, levels });
    (kept(), prop::collection::vec(paragraph, 0..3)).prop_map(|(kept, paragraphs)| OdfText { kept, paragraphs }).boxed()
}

/// The records of a text's ODF data that can keep unknown content, as the kinds and keys the codec names them by, in the order it reads them: the held
/// values of a style (the first of each group), then the style, then the run or paragraph itself, runs inside their paragraph.
fn text_keys(odf: &OdfText) -> Vec<(&'static str, String)> {
    fn style_keys(keys: &mut Vec<(&'static str, String)>, style: &OdfTextStyle, key: &str) {
        let mut seen: Vec<TextGroup> = Vec::new();
        for h in &style.held {
            // Each value is keyed by its group and its ordinal among the values of its group (Call 24).
            let ordinal = seen.iter().filter(|g| **g == h.group).count();
            seen.push(h.group);
            keys.push(("held-style", format!("{key}.{}.{ordinal}", h.group.id())));
        }
        keys.push(("text-style", key.to_owned()));
    }
    let mut keys = Vec::new();
    for (p, paragraph) in odf.paragraphs.iter().enumerate() {
        style_keys(&mut keys, &paragraph.style, &p.to_string());
        for (r, run) in paragraph.runs.iter().enumerate() {
            let key = format!("{p}.{r}");
            style_keys(&mut keys, &run.style, &key);
            keys.push(("run", key));
        }
        keys.push(("paragraph", p.to_string()));
    }
    keys
}

/// The slot of a text, and unknown content kept for some of its records.
fn text_ext() -> BoxedStrategy<Ext<OdfText>> {
    (ext_of(odf_text()), prop::collection::vec(any::<bool>(), 32))
        .prop_map(|(mut ext, picks)| {
            let keys = ext.get().map(|d| text_keys(&d.odf)).unwrap_or_default();
            for ((kind, key), pick) in keys.into_iter().zip(picks) {
                if pick {
                    push_keyed(&mut ext, kind, &key);
                }
            }
            ext
        })
        .boxed()
}

fn row_ext() -> BoxedStrategy<Ext<OdfRow>> {
    (ext_of(odf_row()), any::<bool>())
        .prop_map(|(mut ext, pick)| {
            if let (true, Some(held)) = (pick, ext.get().and_then(|d| d.odf.held.as_ref().map(|h| h.group))) {
                // A row holds one value, the first of its group.
                push_keyed(&mut ext, "held", &format!("{}.0", group_word(held)));
            }
            ext
        })
        .boxed()
}

fn odf_doc(pool: Vec<Arc<[u8]>>) -> BoxedStrategy<OdfDoc> {
    (
        (kept(), opt_text(), passthrough(), opt_element()),
        (prop::collection::vec(raw_attrs(), 0..3), prop::collection::vec(raw_attrs(), 0..3)),
        prop::collection::vec(
            (prop::sample::select(FAMILIES.to_vec()), raw_element(), prop_oneof![3 => Just(None), 1 => prop::sample::select(pool).prop_map(Some)]),
            0..3,
        ),
        (prop::collection::vec(raw_element().prop_map(Arc::new), 0..2), prop::collection::vec((any_arc_text(), raw_element()), 0..3)),
    )
        .prop_map(|((kept, mimetype, meta, settings), (roots, rows), defs, (layers, styles))| OdfDoc {
            kept,
            mimetype,
            roots: roots.into_iter().enumerate().map(|(i, a)| (Arc::from(format!("r{i}").as_str()), a)).collect(),
            manifest_rows: rows.into_iter().enumerate().map(|(i, a)| (Arc::from(format!("m{i}").as_str()), a)).collect(),
            meta,
            settings,
            definitions: defs
                .into_iter()
                .enumerate()
                .map(|(i, (family, element, bytes))| Definition {
                    family,
                    name: Arc::from(format!("d{i}").as_str()),
                    element: Arc::new(element),
                    bytes,
                })
                .collect(),
            layers,
            styles: styles
                .into_iter()
                .enumerate()
                .map(|(i, (family, element))| KeptStyle { family, name: Arc::from(format!("s{i}").as_str()), element: Arc::new(element) })
                .collect(),
        })
        .boxed()
}

fn formats() -> BoxedStrategy<Format> {
    prop::sample::select(vec![Format::Vsdx, Format::Odf, Format::Native]).boxed()
}

/// A relationship as a strategy makes it: its type, its target and whether the target is outside the package.
type RelSpec = (Arc<str>, Arc<str>, bool);

fn rels_of(source: String, specs: Vec<RelSpec>) -> Vec<Rel> {
    specs
        .into_iter()
        .enumerate()
        .map(|(i, (kind, target, external))| Rel {
            source: Arc::from(source.as_str()),
            id: Arc::from(format!("rId{i}").as_str()),
            kind,
            target,
            external,
        })
        .collect()
}

fn rel_specs() -> BoxedStrategy<Vec<RelSpec>> {
    prop::collection::vec((any_arc_text(), any_arc_text(), any::<bool>()), 0..3).boxed()
}

fn parts() -> BoxedStrategy<Vec<Part>> {
    let roles = vec![
        PartRole::Thumbnail,
        PartRole::MacroProject,
        PartRole::MacroSignature,
        PartRole::MacroData,
        PartRole::Embedding,
        PartRole::ObjectFile,
        PartRole::ObjectReplacement,
        PartRole::Signature,
        PartRole::CustomXml,
        PartRole::Font,
        PartRole::Other,
    ];
    prop::collection::vec((formats(), any_arc_text(), prop::sample::select(roles), raw_attrs(), rel_specs(), prop::sample::select(byte_pool())), 0..3)
        .prop_map(|specs| {
            specs
                .into_iter()
                .enumerate()
                .map(|(i, (format, media_type, role, manifest, rels, bytes))| {
                    let name = format!("/p{i}.bin");
                    Part { format, name: Arc::from(name.as_str()), media_type, role, manifest, rels: Arc::new(rels_of(name, rels)), bytes }
                })
                .collect()
        })
        .boxed()
}

fn vsdx_held() -> BoxedStrategy<VsdxHeld> {
    (rel_specs(), prop::collection::vec((any::<bool>(), any_arc_text()), 0..3), prop::collection::vec((any_arc_text(), kept()), 0..3), opt_element())
        .prop_map(|(rels, types, props_rest, windows)| VsdxHeld {
            rels: rels_of("/".to_owned(), rels),
            content_types: types
                .into_iter()
                .enumerate()
                .map(|(i, (default, content_type))| {
                    if default {
                        ContentTypeRow::Default { extension: Arc::from(format!("e{i}").as_str()), content_type }
                    } else {
                        ContentTypeRow::Override { part: Arc::from(format!("/o{i}").as_str()), content_type }
                    }
                })
                .collect(),
            props_rest,
            windows,
        })
        .boxed()
}

fn custom_props() -> BoxedStrategy<Vec<CustomProp>> {
    prop::collection::vec((formats(), any_arc_text(), any_arc_text(), raw_attrs()), 0..3)
        .prop_map(|specs| {
            specs
                .into_iter()
                .enumerate()
                .map(|(i, (format, value_type, value, raw))| CustomProp { format, name: Arc::from(format!("c{i}").as_str()), value_type, value, raw })
                .collect()
        })
        .boxed()
}

/// Gives some of the document's records that have no slot (a colour entry, a custom property, a definition, a kept style, a root, a manifest row, a part, a
/// relationship, a content-type row) unknown content from a newer build, in the order the codec reads them, each key once (NP7). `picks` says which.
fn add_keyed(doc: &mut Document, picks: &[bool]) {
    let mut keys: Vec<(&'static str, String)> = Vec::new();
    for c in &doc.props.custom {
        keys.push(("custom", format!("{}:{}", format_word(c.format), c.name)));
    }
    for c in doc.colors.iter() {
        keys.push(("color", c.ix.to_string()));
    }
    if let Some(data) = doc.ext.get() {
        for (name, _) in &data.odf.roots {
            keys.push(("root", name.to_string()));
        }
        for (name, _) in &data.odf.manifest_rows {
            keys.push(("manifest-row", name.to_string()));
        }
        for d in &data.odf.definitions {
            keys.push(("definition", format!("{}:{}", family_word(d.family), d.name)));
        }
        for s in &data.odf.styles {
            keys.push(("kept-style", format!("{}:{}", s.family, s.name)));
        }
    }
    for p in doc.parts.iter() {
        for r in p.rels.iter() {
            keys.push(("rel", format!("{}|{}", r.source, r.id)));
        }
        keys.push(("part", p.name.to_string()));
    }
    if let Some(h) = &doc.vsdx {
        for r in &h.rels {
            keys.push(("rel", format!("{}|{}", r.source, r.id)));
        }
        for t in &h.content_types {
            keys.push((
                "content-type",
                match t {
                    ContentTypeRow::Default { extension, .. } => format!("default:{extension}"),
                    ContentTypeRow::Override { part, .. } => format!("override:{part}"),
                },
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for ((kind, key), pick) in keys.into_iter().zip(picks.iter().copied().chain(std::iter::repeat(false))) {
        if pick && seen.insert(format!("{kind}[{key}]")) {
            push_keyed(&mut doc.ext, kind, &key);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------- documents the repository makes

/// A document with the derived data of its themes left out: a theme as the codec stores it, its part name and its bytes, since the colours, fonts and
/// entries of a theme are derived from its bytes when a document is opened (1.5).
pub fn without_derived_theme_data(doc: &Document) -> Document {
    let mut out = doc.clone();
    out.themes = doc.themes.iter().map(|t| Arc::new(Theme { part: Arc::clone(&t.part), bytes: Arc::clone(&t.bytes), ..Theme::default() })).collect();
    out
}

/// The shapes of a drawing with every kind of element the VSDX reader types, and of the kinds it keeps without a field: foreign attributes, comments,
/// instructions, mixed text, a style link that is no number, an `ID` that is not one, a cell named in another case, a `Trigger`.
const RICH_SHAPES: &str = r##"
<Shape ID="1" Name="Group &amp; co" NameU="g" Type="Group" Master="1" LineStyle="1" FillStyle="" TextStyle="007" OriginalID="9" UniqueID="{AA}" x:mine="1" xmlns:x="urn:example">
  <Cell N="Angle" V="0.5" U="DEG"/>
  <Cell N="FlipX" V="0" F="Inh"/>
  <Cell N="Height" V="1.50" F="Width*0.75" U="MM"/>
  <Cell N="LineColor" V="Themed" F="THEMEVAL()"/>
  <Cell N="LocPinX" V="1"><RefBy T="Page" ID="0"/></Cell>
  <Cell N="PinX" V="3.5" F="GUARD(3.5 in)" E="#REF!" U="IN"/>
  <Cell N="ThemeIndex" V="33"><?NewValue V?></Cell>
  <Cell N="Width" V="2" F="No Formula">kept text</Cell>
  <Cell N="pinx" V="1" Extra="1" Other="x&#10;y"/>
  <Section N="Geometry" IX="0" Del="0">
    <Cell N="NoFill" V="0"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2" Del="1"/>
    <Row T="LineTo" IX="3" LocalName="Linie"><Cell N="X" V="1" F="Width*1"/></Row>
  </Section>
  <Section N="Geometry" IX="1"><Row T="Ellipse" IX="1"><Cell N="X" V="1"/></Row></Section>
  <Section N="User" IX="4"><Row N="Row_1"><Cell N="Value" V="ü€" U="STR"/></Row><Row N="Row_2" Del="true"/></Section>
  <Section N="Scratch"><Row IX="0" N="both"><Cell N="A" V="1"/></Row></Section>
  <Section N="Tabs"><Row IX="0"><Cell N="Position0" V="1"/><Tab><Cell N="Position" V="2"/></Tab></Row></Section>
  <Trigger N="RecalcColor"><RefBy T="Page" ID="0"/></Trigger>
  <Text>  lead<cp IX="0"/>ab &amp; c<pp IX="1"/><tp IX="2"/><fld IX="0">3</fld><cp IX="x"/><zz a="b">in</zz><!--c--> tail  </Text>
  <Data1>one &lt; two</Data1>
  <Shapes>
    <Shape ID="2" MasterShape="4" Master="2" Del="0"><Cell N="X" V="1"/></Shape>
    <Shape ID="2" Type="Foreign" Del="1">
      <Cell N="ImgWidth" V="1"/>
      <ForeignData ForeignType="Bitmap" CompressionType="PNG" ObjectWidth="1" Extra="e">stray<Rel r:id="rId1"/></ForeignData>
    </Shape>
  </Shapes>
  <x:note xmlns:x="urn:z" a="1">hi<x:inner/></x:note>
  <!-- a comment between children -->
  <?pi data?>
</Shape>
<Shape ID="4294967295" Type="Shape" Del="1"/>
<Shape ID="4294967296" Master="x" Type="Guide"><Cell N="X" V="1"/></Shape>
"##;

/// The name of the picture's part in the package of [`rich_vsdx_document`].
const PICTURE_PART: &str = "/visio/media/image1.png"; // brand-ok: a part name the format defines

/// A drawing package with every kind of element the VSDX reader types: written by hand from the specs, and read by the real reader, so that the model it
/// gives has what a reader makes and a generator does not (formulas parsed under the document's budget, a theme with its derived fields, the pictures the
/// reader found, the attributes and nodes it keeps, the document's properties).
#[allow(clippy::expect_used)]
fn rich_vsdx_document() -> Document {
    let bytes = rich_vsdx_original_pkg().build();
    let (doc, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).expect("the rich package reads");
    doc
}

/// The builder of the package [`rich_vsdx_document`] reads, **as it always was**: the picture typed by an `Override`, the page that uses masters with no relationship to them, the fields of
/// its core part in an order of their own. A model writer gives it back but for the spelling the model holds no record of (`rich_vsdx_pkg` is the same package typed as the writer types it).
pub fn rich_vsdx_original_pkg() -> VsdxBuilder {
    let styles = r##"<StyleSheet ID="1" Name="Normal" NameU="Normal" LineStyle="0" FillStyle="0" TextStyle="0" IsCustomName="1"><Cell N="LineWeight" V="0.01" F="THEMEVAL()"/><Section N="Character"><Row IX="0"><Cell N="Size" V="0.1666667" U="PT"/></Row></Section><Trigger N="RecalcColor"/><x:y xmlns:x="urn:q"/></StyleSheet><StyleSheet ID="2" NameU="B" LineStyle="1" FillStyle="x" TextStyle="007"><Cell N="A" V="1"/></StyleSheet>"##;
    let theme = crate::theme::ThemeXml::default().xml();
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/><ColorEntry IX="25" RGB="#654321"/>"##)
        .face_names(r#"<FaceName NameU="Arial" Panose="1"/><FaceName NameU="Noto Sans"/>"#)
        .more_styles(styles)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/><Section N="Reviewer"><Row IX="0"><Cell N="Name" V="x"/></Row></Section>"#)
        .document_extra(r#"<DocumentSettings TopPage="0"/><EventList/>"#)
        .core(r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>A title &amp; more</dc:title><dc:creator>A person</dc:creator><dc:subject>S</dc:subject><dc:language>en-US</dc:language><dcterms:created>2031-03-07T20:05:09Z</dcterms:created></cp:coreProperties>"#)
        .theme(theme.as_bytes())
        .master(MasterSpec::new(1, "M").icon("QUJD").shapes(r#"<Shape ID="4" Type="Shape" Master="9" MasterShape="9"><Cell N="PinX" V="1" F="Width/2"/></Shape><Shape ID="5"/>"#))
        .master(MasterSpec::new(2, "N").shapes(r#"<Shape ID="4"/>"#))
        .page(
            PageSpec::new(1, "P")
                .sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/></Row></Section><Trigger N="RecalcColor"/>"#)
                .shapes(RICH_SHAPES)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3" Extra="x"/><Connect FromSheet="007" ToSheet="2"/>"#)
                .rel("rId1", &format!("{}image", crate::package::OD), "../media/image1.png"),
        )
        .page(PageSpec::new(2, "Q").without_part())
        .with(Pkg::new().part(PICTURE_PART, "image/png", [1u8, 2, 3]))
}

/// The bytes of the drawing package [`rich_vsdx_document`] reads: every kind of element the VSDX reader types, for the tests that write it again.
pub fn rich_vsdx_bytes() -> Vec<u8> {
    rich_vsdx_pkg(true).build()
}

/// The builder of the package of [`rich_vsdx_bytes`]: **the package of [`rich_vsdx_document`], retyped beside it and not in its place** (the original layout stays what the other tests read).
/// The picture is typed as a package of the format types its pictures, by the `Default` of its extension, where the original has an `Override`; the page that
/// uses a master has the relationship to it that every such page has; `partless_page` is whether its second page has no part, which only a stencil's
/// pages are seen without. What a model writer cannot say of the original (which of the two typed a part) it can say of this one, so that a content check finds nothing.
pub fn rich_vsdx_pkg(partless_page: bool) -> VsdxBuilder {
    let styles = r##"<StyleSheet ID="1" Name="Normal" NameU="Normal" LineStyle="0" FillStyle="0" TextStyle="0" IsCustomName="1"><Cell N="LineWeight" V="0.01" F="THEMEVAL()"/><Section N="Character"><Row IX="0"><Cell N="Size" V="0.1666667" U="PT"/></Row></Section><Trigger N="RecalcColor"/><x:y xmlns:x="urn:q"/></StyleSheet><StyleSheet ID="2" NameU="B" LineStyle="1" FillStyle="x" TextStyle="007"><Cell N="A" V="1"/></StyleSheet>"##;
    let theme = crate::theme::ThemeXml::default().xml();
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/><ColorEntry IX="25" RGB="#654321"/>"##)
        .face_names(r#"<FaceName NameU="Arial" Panose="1"/><FaceName NameU="Noto Sans"/>"#)
        .more_styles(styles)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/><Section N="Reviewer"><Row IX="0"><Cell N="Name" V="x"/></Row></Section>"#)
        .document_extra(r#"<DocumentSettings TopPage="0"/><EventList/>"#)
        .core(r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>A title &amp; more</dc:title><dc:creator>A person</dc:creator><dc:subject>S</dc:subject><dc:language>en-US</dc:language><dcterms:created>2031-03-07T20:05:09Z</dcterms:created></cp:coreProperties>"#)
        .theme(theme.as_bytes())
        .master(MasterSpec::new(1, "M").icon("QUJD").shapes(r#"<Shape ID="4" Type="Shape" Master="9" MasterShape="9"><Cell N="PinX" V="1" F="Width/2"/></Shape><Shape ID="5"/>"#))
        .master(MasterSpec::new(2, "N").shapes(r#"<Shape ID="4"/>"#))
        .page(
            PageSpec::new(1, "P")
                .sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/></Row></Section><Trigger N="RecalcColor"/>"#)
                .shapes(RICH_SHAPES)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3" Extra="x"/><Connect FromSheet="007" ToSheet="2"/>"#)
                .rel("rId1", &format!("{}image", crate::package::OD), "../media/image1.png")
                .rel("rId2", &format!("{}master", crate::package::VIS), "../masters/master1.xml")
                .rel("rId3", &format!("{}master", crate::package::VIS), "../masters/master2.xml"),
        )
        .page(if partless_page { PageSpec::new(2, "Q").without_part() } else { PageSpec::new(2, "Q") })
        .with(Pkg::new().part_without_override(PICTURE_PART, [1u8, 2, 3]).default_type("png", "image/png"))
}

/// Documents that the repository's own code makes, which no generator would make: the blank drawing, every built-in master in one drawing, each template as
/// the engine builds it (a theme applied, stencil names), the rich VSDX package as the reader gives it, and a drawing a session made with commands. Each
/// comes with a name for a failure message. Their themes hold the derived data a reader fills; compare them through [`without_derived_theme_data`].
#[allow(clippy::expect_used)]
pub fn fixture_documents() -> Vec<(String, Document)> {
    let mut out: Vec<(String, Document)> = vec![("the blank drawing".to_owned(), Document::new_blank())];
    let mut all = Document::new_blank();
    for spec in diacraft_engine::stencil::all_masters() {
        diacraft_engine::stencil::ensure_master(&mut all, spec).expect("a built-in master is added to a document");
    }
    out.push(("every built-in master".to_owned(), all));
    for spec in diacraft_engine::template::templates() {
        let built = diacraft_engine::template::build("fixture", spec).expect("a template builds");
        out.push((format!("template {}", spec.id), built.document));
    }
    out.push(("the rich VSDX package".to_owned(), rich_vsdx_document()));
    let mut session = crate::fixtures::blank_session();
    for (command, params) in [
        ("shape.drop", serde_json::json!({"master": "rectangle"})),
        ("shape.drop", serde_json::json!({"master": "ellipse"})),
        ("shape.set_text", serde_json::json!({"text": "A \u{1} text & more\r\n"})),
        ("shape.format", serde_json::json!({"weight": 1.23})),
        ("shape.move", serde_json::json!({"dx": 0.125, "dy": 0.125})),
    ] {
        // A command that does not apply leaves the drawing as it was: the fixture is what the others made.
        let _ = session.execute(command, &params);
    }
    if let Some(doc) = session.document() {
        out.push(("a session's drawing".to_owned(), doc.clone()));
    }
    out
}
