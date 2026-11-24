//! The model holds both formats' data, each tagged (`package.rs`, `odf.rs`, `stamp.rs` and the `ext` slots).
//!
//! The plan's tests come first, then the ones that kill each `Ext` and stamp mutant: a slot holding one field alone, every family past its cap, the
//! order the caps are checked in, and the stamps' bytes pinned to an encoding written out apart from the code (`stamp.rs`'s module docs).

use std::mem::size_of;
use std::sync::Arc;

use diacraft_doc::odf::{
    DefFamily, Definition, Held, HeldGroup, HeldStyleValue, KeptStyle, MAX_DEFINITIONS, OdfDoc, OdfPage, OdfParagraph, OdfRow, OdfRun, OdfShape,
    OdfStyle, OdfText, OdfTextStyle, TextGroup,
};
use diacraft_doc::package::{ContentTypeRow, CustomProp, Ext, ExtData, Format, Kept, MAX_PARTS, Part, PartRole, Rel, VsdxHeld, VsdxRest};
use diacraft_doc::stamp::{Digest, Stamp, stamp_cells, stamp_row, stamp_rows_named, stamp_section, stamp_sections, stamp_text_group, stamp_text_row};
use diacraft_doc::{
    Cell, CellExtra, CellName, Connect, DocError, DocProps, Document, FaceName, ForeignData, Master, Page, Passthrough, RawAttrs, RawElement,
    RawNode, Row, Section, Shape, Sheet, Style, TextPart, Value, listed_cells,
};

// Measured at 7f65afe, where `crates/doc` is unchanged since ae19066 (the plan's base). The slot of a cell is inside `CellExtra`, and a shape's is one
// pointer, so a cell costs nothing and a shape 8 bytes.
const CELL_BYTES_AT_AE19066: usize = 80;
const SHAPE_BYTES_AT_AE19066: usize = 184;

fn text(s: &str) -> Arc<str> {
    Arc::from(s)
}

fn kept_attr(name: &str, value: &str) -> Kept {
    Kept { raw: RawAttrs::from_pairs([(name, value)]), kept: Passthrough::new() }
}

fn kept_node() -> Kept {
    let mut kept = Passthrough::new();
    kept.push(0, RawNode::Comment(text("kept")));
    Kept { raw: RawAttrs::new(), kept }
}

fn constant(value: &str) -> Cell {
    Cell::constant(Value::from_text(value))
}

fn sheet_with(cells: &[(&str, Cell)]) -> Sheet {
    let mut sheet = Sheet::default();
    for (name, cell) in cells {
        sheet.cells.insert(CellName::new(name), cell.clone());
    }
    sheet
}

// ---------------------------------------------------------------------------------------------------------------- the model does not grow

#[test]
fn cell_is_no_larger() {
    assert_eq!(size_of::<Cell>(), CELL_BYTES_AT_AE19066);
}

/// What `TextPart` was at 7f65afe, before `OdfRaw` and `NativeRaw`: the dearest of its variants, `Fld` (an `IX` and a text) and `Raw`, set it.
const TEXT_PART_BYTES_AT_7F65AFE: usize = 48;

#[test]
fn text_part_is_no_larger() {
    // A text of a million characters of markers is a million of these. The two variants a newer build and the other format fill are rare, so each holds its node
    // behind a pointer, and the part is the size it was.
    assert_eq!(size_of::<TextPart>(), TEXT_PART_BYTES_AT_7F65AFE);
    assert!(size_of::<Box<RawNode>>() <= size_of::<usize>());
}

#[test]
fn shape_grows_by_at_most_one_pointer() {
    assert!(size_of::<Shape>() <= SHAPE_BYTES_AT_AE19066 + size_of::<usize>(), "a shape is {} bytes", size_of::<Shape>());
    // The slot itself is one pointer, null when empty.
    assert_eq!(size_of::<Ext<OdfShape>>(), size_of::<usize>());
}

#[test]
fn a_model_without_new_data_compares_as_before() {
    let d = Document::new_blank();
    assert!(d.ext.is_empty() && d.parts.is_empty() && d.vsdx.is_none() && d.props.custom.is_empty());
    assert_eq!(d, Document::new_blank());
    assert_eq!(Document::empty(), Document::empty());
    // Every page, master, style and shape a blank document holds has an empty slot.
    for page in &d.pages {
        assert!(page.ext.is_empty() && page.part_name.is_none());
    }
    for style in &d.styles {
        assert!(style.ext.is_empty());
    }
    // Every constructor of the model leaves each slot empty: a new object holds no data of either format.
    assert!(Section::new("User", None).ext.is_empty() && Page::new(1, "p").ext.is_empty() && Page::new(1, "p").part_name.is_none());
    assert!(
        Shape::new(1).ext.is_empty() && Row::default().ext.is_empty() && Sheet::default().ext.is_empty() && Master::default().part_name.is_none()
    );
    assert!(
        Style::default().ext.is_empty()
            && Master::default().ext.is_empty()
            && Connect::default().ext.is_empty()
            && ForeignData::default().ext.is_empty()
    );
    assert!(CellExtra::default().ext.is_empty() && DocProps::default().custom.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------- Ext<T>

#[test]
fn tidy_returns_an_unused_slot_to_none() {
    let mut e: Ext<OdfShape> = Ext::default();
    let _ = e.make_mut();
    // Made for a write, and nothing written: a record of defaults is held (the slot is a pointer to it) and it equals an empty slot all the same.
    assert!(format!("{e:?}").starts_with("Ext(Some("), "{e:?}");
    e.tidy();
    assert!(e.is_empty());
    assert_eq!(e, Ext::default());
    // `None` is the representation: nothing is held, not even an empty record, and the slot costs a null pointer again.
    assert!(e.get().is_none());
    assert_eq!(format!("{e:?}"), "Ext(None)");
}

#[test]
fn an_empty_slot_equals_an_absent_one() {
    let mut e: Ext<OdfShape> = Ext::default();
    let _ = e.make_mut(); // Some(all default), not tidied
    assert_eq!(e, Ext::default());
    assert_eq!(Ext::default(), e);
    assert!(e.is_empty());
    assert!(e.get().is_none(), "an empty slot holds no data, tidied or not");
}

#[test]
fn tidy_keeps_a_slot_that_holds_any_one_field_alone() {
    // Each field of the slot alone, and each field of the ODF data alone: a tidy that tests only some of them would drop the rest.
    type Set = Box<dyn Fn(&mut ExtData<OdfShape>)>;
    let odf_alone: Vec<Set> = vec![
        Box::new(|d| d.odf.style_name = Some(text("gr1"))),
        Box::new(|d| d.odf.kept = kept_attr("a", "1")),
        Box::new(|d| d.odf.units = RawAttrs::from_pairs([("svg:x", "cm")])),
        Box::new(|d| d.odf.held.push(Held { group: HeldGroup::Fill, stamp: Some(Stamp(1)), attrs: RawAttrs::new(), kept: Passthrough::new() })),
        Box::new(|d| d.vsdx.0.push((text("Trigger"), Kept::default()))),
        Box::new(|d| d.native = kept_attr("n", "1")),
        Box::new(|d| d.native = kept_node()),
    ];
    for (i, set) in odf_alone.iter().enumerate() {
        let mut e: Ext<OdfShape> = Ext::default();
        set(e.make_mut());
        assert!(!e.is_empty(), "case {i}");
        assert!(e.get().is_some(), "case {i}");
        assert_ne!(e, Ext::default(), "case {i}");
        let before = e.clone();
        e.tidy();
        assert_eq!(e, before, "case {i}: tidy dropped data");
        assert!(e.get().is_some(), "case {i}");
    }
}

#[test]
fn tidy_drops_a_slot_whose_fields_are_back_to_their_defaults() {
    let mut e: Ext<OdfShape> = Ext::default();
    e.make_mut().odf.style_name = Some(text("gr1"));
    e.make_mut().vsdx.0.push((text("Trigger"), Kept::default()));
    e.make_mut().native = kept_attr("n", "1");
    e.tidy();
    assert!(!e.is_empty());
    let data = e.make_mut();
    data.odf.style_name = None;
    data.vsdx.0.clear();
    data.native = Kept::default();
    e.tidy();
    assert!(e.get().is_none() && e.is_empty());
}

#[test]
fn equality_of_slots_is_an_equivalence() {
    let none: Ext<Kept> = Ext::default();
    let mut empty_a: Ext<Kept> = Ext::default();
    let _ = empty_a.make_mut();
    let mut empty_b: Ext<Kept> = Ext::default();
    let _ = empty_b.make_mut();
    // Reflexive, symmetric, transitive across the three spellings of nothing.
    assert!(none == empty_a && empty_a == none && empty_a == empty_b && none == empty_b);
    let mut held: Ext<Kept> = Ext::default();
    held.make_mut().native = kept_attr("n", "1");
    let mut same: Ext<Kept> = Ext::default();
    same.make_mut().native = kept_attr("n", "1");
    assert!(held == same && held == held.clone());
    assert!(held != none && none != held && held != empty_a && empty_a != held);
    let mut other: Ext<Kept> = Ext::default();
    other.make_mut().native = kept_attr("n", "2");
    assert!(held != other);
    // Vsdx, ODF and native data are three different things: the same text in another slot is a different value.
    let mut in_vsdx: Ext<Kept> = Ext::default();
    in_vsdx.make_mut().vsdx.0.push((text("n"), Kept::default()));
    assert!(in_vsdx != held && in_vsdx != none);
}

#[test]
fn a_clone_shares_the_slot_and_an_edit_copies_it() {
    let mut e: Ext<OdfShape> = Ext::default();
    e.make_mut().odf.style_name = Some(text("gr1"));
    let mut copy = e.clone();
    let (a, b) = (e.get().unwrap(), copy.get().unwrap());
    assert!(std::ptr::eq(a, b), "a clone takes a reference count and copies nothing");
    copy.make_mut().odf.style_name = Some(text("gr2"));
    assert_eq!(e.get().unwrap().odf.style_name.as_deref(), Some("gr1"), "the older copy keeps its data");
    assert_eq!(copy.get().unwrap().odf.style_name.as_deref(), Some("gr2"));
    assert_ne!(e, copy);
}

#[test]
fn odf_data_makes_two_shapes_differ() {
    let a = Shape::new(1);
    let mut b = Shape::new(1);
    assert_eq!(a, b);
    b.ext.make_mut().odf.style_name = Some(text("gr1"));
    assert_ne!(a, b);
    let mut c = Shape::new(1);
    c.ext.make_mut().odf.style_name = Some(text("gr1"));
    assert_eq!(b, c);
    c.ext.make_mut().odf.style_name = Some(text("gr2"));
    assert_ne!(b, c);
    // The VSDX `raw` and the ODF data are separate: the same attribute in one does not equal it in the other.
    let mut d = Shape::new(1);
    d.raw = RawAttrs::from_pairs([("style", "gr1")]);
    assert_ne!(b, d);
    assert_ne!(a, d);
}

#[test]
fn every_object_that_can_carry_a_slot_compares_it() {
    fn differs<T: PartialEq + Clone + std::fmt::Debug>(base: &T, edit: impl Fn(&mut T)) {
        let mut other = base.clone();
        assert_eq!(base, &other);
        edit(&mut other);
        assert_ne!(base, &other, "a change in the slot goes unseen");
    }
    differs(&Page::new(1, "p"), |p| p.ext.make_mut().odf.master_page = Some(text("Default")));
    differs(&Page::new(1, "p"), |p| p.part_name = Some(text("/pages/p1.xml")));
    differs(&Master::default(), |m| m.ext.make_mut().native = kept_attr("a", "1"));
    differs(&Master::default(), |m| m.part_name = Some(text("/masters/m1.xml")));
    differs(&Style::default(), |s| s.ext.make_mut().odf.family = Some(text("graphic")));
    differs(&Shape::new(1), |s| s.ext.make_mut().vsdx.0.push((text("Data"), Kept::default())));
    differs(&Row::default(), |r| r.ext.make_mut().odf.style_name = Some(text("r1")));
    differs(&diacraft_doc::Text::default(), |t| t.ext.make_mut().odf.paragraphs.push(OdfParagraph::default()));
    differs(&Sheet::default(), |s| s.ext.make_mut().native = kept_node());
    differs(&Section::new("Geometry", None), |s| s.ext.make_mut().vsdx.0.push((text("X"), Kept::default())));
    differs(&CellExtra::default(), |c| c.ext.make_mut().native = kept_attr("a", "1"));
    differs(&ForeignData::default(), |f| f.ext.make_mut().native = kept_attr("a", "1"));
    differs(&Connect::default(), |c| c.ext.make_mut().native = kept_attr("a", "1"));
    differs(&FaceName { name_u: text("Arial"), raw: RawAttrs::new(), ext: Ext::default() }, |f| f.ext.make_mut().odf = kept_attr("a", "1"));
    differs(&Document::new_blank(), |d| d.ext.make_mut().odf.mimetype = Some(text("application/vnd.oasis.opendocument.graphics")));
}

#[test]
fn a_text_can_hold_both_formats_markup_and_a_newer_builds() {
    let node = || RawNode::Comment(text("c"));
    let parts = [TextPart::Raw(node()), TextPart::OdfRaw(Box::new(node())), TextPart::NativeRaw(Box::new(node()))];
    // Three variants, three different values, though they hold the same node.
    assert_ne!(parts[0], parts[1]);
    assert_ne!(parts[1], parts[2]);
    assert_ne!(parts[0], parts[2]);
    assert_eq!(TextPart::OdfRaw(Box::new(node())), TextPart::OdfRaw(Box::new(node())));
}

#[test]
fn the_package_level_data_makes_two_documents_differ() {
    let base = Document::new_blank();
    let part = |format: Format, name: &str| {
        Arc::new(Part {
            format,
            name: text(name),
            media_type: text("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(&[1_u8, 2, 3][..]),
        })
    };
    let mut with_part = base.clone();
    with_part.parts.push(part(Format::Vsdx, "/docs/a.bin"));
    assert_ne!(base, with_part);
    // The same part under the other two tags is another part: the tag is data, and writers route by it.
    let mut as_odf = base.clone();
    as_odf.parts.push(part(Format::Odf, "/docs/a.bin"));
    let mut as_native = base.clone();
    as_native.parts.push(part(Format::Native, "/docs/a.bin"));
    let mut as_vsdx = base.clone();
    as_vsdx.parts.push(part(Format::Vsdx, "/docs/a.bin"));
    assert!(as_odf != as_native && as_native != as_vsdx && as_vsdx != as_odf);

    let mut with_held = base.clone();
    with_held.vsdx = Some(Arc::new(VsdxHeld::default()));
    assert_ne!(base, with_held, "held VSDX data, even an empty record, is a record");
    let mut with_rel = with_held.clone();
    Arc::make_mut(with_rel.vsdx.as_mut().unwrap()).rels.push(Rel {
        source: text("/"),
        id: text("rId1"),
        kind: text("t"),
        target: text("a.xml"),
        external: false,
    });
    assert_ne!(with_held, with_rel);
    Arc::make_mut(with_rel.vsdx.as_mut().unwrap())
        .content_types
        .push(ContentTypeRow::Default { extension: text("png"), content_type: text("image/png") });
    Arc::make_mut(with_held.vsdx.as_mut().unwrap()).rels = with_rel.vsdx.as_ref().unwrap().rels.clone();
    assert_ne!(with_held, with_rel, "a content-type row alone");

    let mut with_custom = base.clone();
    Arc::make_mut(&mut with_custom.props).custom.push(CustomProp {
        format: Format::Vsdx,
        name: text("n"),
        value_type: text("lpwstr"),
        value: text("v"),
        raw: RawAttrs::new(),
    });
    assert_ne!(base, with_custom);
    assert_eq!(DocProps::default(), DocProps::default());
}

#[test]
fn an_odf_document_record_compares_by_every_field() {
    let mut a = Ext::<OdfDoc>::default();
    a.make_mut().odf.layers.push(Arc::new(RawElement { name: text("draw:layer"), attrs: RawAttrs::new(), children: Vec::new() }));
    let mut b = a.clone();
    assert_eq!(a, b);
    b.make_mut().odf.styles.push(KeptStyle {
        family: text("paragraph"),
        name: text("P1"),
        element: Arc::new(RawElement { name: text("style:style"), attrs: RawAttrs::new(), children: Vec::new() }),
    });
    assert_ne!(a, b);
    let _ = (OdfPage::default(), OdfStyle::default(), OdfRow::default());
}

#[test]
fn a_text_holds_the_style_of_each_paragraph_and_each_run_with_a_stamp_of_its_own() {
    // Two paragraphs whose styles resolve alike share one `Paragraph` row and may name two styles, so the names are held
    // per paragraph, and the runs of a paragraph per run; each value inside a style has its own stamp.
    let style = |name: &str, stamp: u64| OdfTextStyle {
        name: Some(text(name)),
        stamp: Some(Stamp(stamp)),
        held: vec![HeldStyleValue {
            group: TextGroup::HorzAlign,
            stamp: Some(Stamp(stamp + 100)),
            attrs: RawAttrs::from_pairs([("fo:text-align", "start")]),
            kept: Passthrough::new(),
        }],
    };
    let paragraph =
        |name: &str, runs: Vec<OdfRun>| OdfParagraph { style: style(name, 1), characters: Some(Stamp(9)), runs, element: None, levels: Vec::new() };
    let run = |from: u32, to: u32, name: &str| OdfRun { from, to, style: style(name, 2), element: None };
    let mut base = diacraft_doc::Text::default();
    assert!(base.ext.is_empty(), "a text made with no data holds none");
    base.ext.make_mut().odf =
        OdfText { kept: Kept::default(), paragraphs: vec![paragraph("P1", vec![run(0, 3, "T1"), run(3, 5, "T2")]), paragraph("P2", Vec::new())] };
    let edited = |edit: &dyn Fn(&mut OdfText)| {
        let mut t = base.clone();
        edit(&mut t.ext.make_mut().odf);
        t
    };
    assert_eq!(base, edited(&|_| {}));
    // Each place a value lives is part of the text: a paragraph's name, a run's name, a run's bounds, a value's stamp and attributes, the stamp of the
    // row and of the characters, a paragraph more, and the order.
    assert_ne!(base, edited(&|t| t.paragraphs[1].style.name = Some(text("P3"))), "a paragraph's name");
    assert_ne!(base, edited(&|t| t.paragraphs[0].runs[1].style.name = None), "a run's name");
    assert_ne!(base, edited(&|t| t.paragraphs[0].runs[1].from = 4), "a run's start");
    assert_ne!(base, edited(&|t| t.paragraphs[0].runs[1].to = 6), "a run's end");
    assert_ne!(base, edited(&|t| t.paragraphs[0].runs[0].style.held[0].stamp = Some(Stamp(0))), "the stamp of a run's value");
    assert_ne!(base, edited(&|t| t.paragraphs[0].runs[0].style.held[0].stamp = None), "a run's value with no stamp");
    assert_ne!(base, edited(&|t| t.paragraphs[0].style.held[0].attrs = RawAttrs::new()), "a paragraph's value");
    assert_ne!(base, edited(&|t| t.paragraphs[0].style.held[0].kept = kept_node().kept), "the nodes a value kept");
    let more = HeldStyleValue { group: TextGroup::SpLine, stamp: None, attrs: RawAttrs::new(), kept: Passthrough::new() };
    assert_ne!(base, edited(&|t| t.paragraphs[0].style.held.push(more.clone())), "a value more");
    assert_ne!(base, edited(&|t| t.paragraphs[0].style.held[0].group = TextGroup::Flags), "the group of a value");
    assert_ne!(base, edited(&|t| t.paragraphs[0].style.stamp = None), "the stamp of the paragraph's row");
    assert_ne!(base, edited(&|t| t.paragraphs[0].characters = None), "the stamp of the characters");
    assert_ne!(base, edited(&|t| t.paragraphs.push(OdfParagraph::default())), "a paragraph more");
    assert_ne!(base, edited(&|t| t.paragraphs.reverse()), "the order");
    assert_ne!(base, edited(&|t| t.kept = kept_attr("a", "1")), "what the markup held beside");
    // The slot belongs to these parts: a new `Text` made by an edit of the characters starts empty, and then it is a text with no data.
    let retyped = diacraft_doc::Text { parts: vec![TextPart::Chars(text("new"))], ..diacraft_doc::Text::default() };
    assert!(retyped.ext.is_empty());
}

/// A row of the cells named, each a constant: `(name, text, unit)`.
fn row_of(cells: &[(&str, &str, &str)]) -> Row {
    let mut row = Row::default();
    for (name, value, unit) in cells {
        let cell = if unit.is_empty() { constant(value) } else { constant(value).with_unit(unit) };
        row.cells.insert(CellName::new(name), cell);
    }
    row
}

#[test]
fn the_text_groups_are_the_splice_writers_and_each_reads_the_cells_it_names() {
    // The names are `write/runs.rs`'s (`owned`, `Part::id`); `odg`'s own test holds the signatures to its `Part::sig`.
    let ids: Vec<&str> = TextGroup::ALL.iter().map(|g| g.id()).collect();
    assert_eq!(
        ids,
        [
            "Size",
            "Style",
            "Underline",
            "Strike",
            "Overline",
            "Case",
            "Pos",
            "Color",
            "Letterspace",
            "FontScale",
            "Font",
            "AsianFont",
            "ComplexScriptFont",
            "HorzAlign",
            "Flags",
            "IndLeft",
            "IndRight",
            "IndFirst",
            "SpBefore",
            "SpAfter",
            "SpLine"
        ]
    );
    for (i, a) in TextGroup::ALL.iter().enumerate() {
        for (j, b) in TextGroup::ALL.iter().enumerate() {
            assert_eq!(a == b, i == j);
        }
        // Every cell a group reads is a cell of a `Character` or a `Paragraph` row that the catalogue lists.
        let section = if a.is_paragraph() { "Paragraph" } else { "Character" };
        for name in a.cells() {
            assert!(listed_cells().any(|(s, n, _)| s == Some(section) && n == *name), "{a:?}: {name} is no cell of a {section} row");
        }
    }
    assert_eq!(TextGroup::ALL.iter().filter(|g| g.is_paragraph()).count(), 8);
    assert_eq!(TextGroup::Underline.cells(), ["Style", "DblUnderline"]);
    assert_eq!(TextGroup::HorzAlign.cells(), ["HorzAlign", "Flags"]);
    assert_eq!(TextGroup::Strike.cells(), ["Strikethru", "DoubleStrikethrough"]);
}

#[test]
fn a_text_groups_stamp_reads_its_cells_and_nothing_else() {
    let stamp = |cells: &[(&str, &str, &str)], group: TextGroup| stamp_text_group(&row_of(cells), group);
    // The alignment is `HorzAlign` and `Flags` (which writes it again); nothing else of the row.
    let align = [("HorzAlign", "1", ""), ("Flags", "0", ""), ("IndLeft", "0.5", "IN"), ("Size", "0.2", "")];
    let base = stamp(&align, TextGroup::HorzAlign);
    assert_eq!(base, stamp(&[("HorzAlign", "1", ""), ("Flags", "0", "")], TextGroup::HorzAlign), "other cells are no part of it");
    assert_ne!(base, stamp(&[("HorzAlign", "2", ""), ("Flags", "0", "")], TextGroup::HorzAlign), "HorzAlign");
    assert_ne!(base, stamp(&[("HorzAlign", "1", ""), ("Flags", "1", "")], TextGroup::HorzAlign), "Flags");
    assert_ne!(base, stamp(&[("HorzAlign", "1", "")], TextGroup::HorzAlign), "no Flags");
    assert_eq!(
        stamp(&[("Flags", "0", "")], TextGroup::HorzAlign),
        stamp(&[("IndLeft", "1", "")], TextGroup::HorzAlign),
        "no HorzAlign: no group, in any row"
    );
    // The `Flags` group is its cell alone.
    assert_eq!(stamp(&align, TextGroup::Flags), stamp(&[("Flags", "0", "")], TextGroup::Flags));
    // A cell is its value's text and its unit, as the splice writer signs it (`1` and `1.0` are two values, and so are two units).
    assert_ne!(stamp(&[("IndLeft", "1", "")], TextGroup::IndLeft), stamp(&[("IndLeft", "1.0", "")], TextGroup::IndLeft));
    assert_ne!(stamp(&[("IndLeft", "1", "IN")], TextGroup::IndLeft), stamp(&[("IndLeft", "1", "CM")], TextGroup::IndLeft));
    assert_ne!(stamp(&[("IndLeft", "1", "IN")], TextGroup::IndLeft), stamp(&[("IndLeft", "1", "")], TextGroup::IndLeft));
    // The splice writer's signature does not see the formula slot of a constant: `No Formula` and no formula are one value. A formula is never one, nor is an
    // inherited one (the splice writer refuses both, so no row of its comparison has them).
    let with = |cell: Cell| {
        let mut row = Row::default();
        row.cells.insert(CellName::new("IndLeft"), cell);
        stamp_text_group(&row, TextGroup::IndLeft)
    };
    let no_formula = |value: &str| Cell { formula: diacraft_doc::FormulaSlot::NoFormula, value: Some(Value::from_text(value)), ..Cell::new() };
    assert_eq!(with(constant("1")), with(no_formula("1")));
    assert_ne!(with(constant("1")), with(no_formula("2")));
    assert_ne!(with(constant("1")), with(Cell::with_formula("1", Some(Value::from_text("1")))));
    assert_ne!(with(Cell::with_formula("1", None)), with(Cell::with_formula("2", None)));
    assert_ne!(with(Cell::inherited(Some(Value::from_text("1")))), with(constant("1")));
    assert_ne!(with(Cell::inherited(None)), with(Cell::with_formula("1", None)));
    assert_ne!(with(Cell::new()), with(constant("")), "no value and the empty text");
    assert_ne!(with(Cell::new()), stamp_text_group(&Row::default(), TextGroup::IndLeft), "a cell with nothing is a cell");
    // Two groups of one row that read the same cell alike stamp apart: the group's name is part of it.
    let twins = row_of(&[("IndLeft", "1", ""), ("IndRight", "1", ""), ("IndFirst", "1", ""), ("SpBefore", "1", ""), ("SpAfter", "1", "")]);
    let stamps: Vec<Stamp> = [TextGroup::IndLeft, TextGroup::IndRight, TextGroup::IndFirst, TextGroup::SpBefore, TextGroup::SpAfter]
        .iter()
        .map(|g| stamp_text_group(&twins, *g))
        .collect();
    for (i, a) in stamps.iter().enumerate() {
        for (j, b) in stamps.iter().enumerate() {
            assert_eq!(a == b, i == j, "groups {i} and {j}");
        }
    }
    // A different group of the same cell is a different stamp, and two groups of one row stamp apart.
    assert_ne!(stamp(&[("IndLeft", "1", "")], TextGroup::IndLeft), stamp(&[("IndLeft", "1", "")], TextGroup::IndRight));
    assert_ne!(stamp(&[("Flags", "0", "")], TextGroup::Flags), stamp(&[("Flags", "0", "")], TextGroup::IndLeft));
}

/// The underline's stamp of a row of `Style` and `DblUnderline`, and the `Style` group's of a row of `Style`.
fn under(style: &str, dbl: &str) -> Stamp {
    stamp_text_group(&row_of(&[("Style", style, ""), ("DblUnderline", dbl, "")]), TextGroup::Underline)
}

fn rest(style: &str) -> Stamp {
    stamp_text_group(&row_of(&[("Style", style, "")]), TextGroup::Style)
}

#[test]
fn the_underlines_stamp_is_bit_4_of_style_and_dbl_underline_and_the_styles_is_the_other_bits() {
    // A bold or an italic edit leaves a held underline as it is, and an underline edit leaves the rest of `Style` as it is.
    assert_eq!(under("0", "0"), under("1", "0"), "bold");
    assert_eq!(under("0", "0"), under("2", "0"), "italic");
    assert_eq!(under("4", "0"), under("5", "0"), "bold with an underline");
    assert_eq!(under("4", "0"), under("7", "0"), "bold and italic with an underline");
    assert_eq!(under("5", "0"), under("5.0", "0"), "a number spelled twice");
    assert_ne!(under("0", "0"), under("4", "0"), "the underline");
    assert_ne!(under("4", "0"), under("4", "1"), "DblUnderline");
    assert_ne!(under("4", "0"), under("4", "0.0"), "DblUnderline, as its text");
    assert_eq!(rest("0"), rest("4"), "an underline leaves the rest of Style as it is");
    assert_eq!(rest("1"), rest("5"));
    assert_ne!(rest("0"), rest("1"), "bold");
    assert_ne!(rest("1"), rest("3"), "italic");
    assert_ne!(rest("0"), rest("8"), "small capitals");
    assert_eq!(rest("5"), rest("5.0"));
    // The group is there when `Style` or `DblUnderline` is, and absent alike in every row without them.
    let only = |cells: &[(&str, &str, &str)]| stamp_text_group(&row_of(cells), TextGroup::Underline);
    assert_ne!(only(&[("DblUnderline", "0", "")]), only(&[("Style", "0", "")]));
    assert_ne!(only(&[("Style", "0", "")]), only(&[("Style", "0", ""), ("DblUnderline", "0", "")]));
    assert_ne!(only(&[("DblUnderline", "0", "")]), only(&[]));
    assert_eq!(only(&[("Size", "1", "")]), only(&[]));
    assert_eq!(stamp_text_group(&Row::default(), TextGroup::Style), stamp_text_group(&row_of(&[("Size", "1", "")]), TextGroup::Style));
    // The line-through reads two cells, each a signature of its own.
    let strike = |cells: &[(&str, &str, &str)]| stamp_text_group(&row_of(cells), TextGroup::Strike);
    assert_ne!(strike(&[("Strikethru", "0", "")]), strike(&[("Strikethru", "1", "")]));
    assert_ne!(strike(&[("Strikethru", "0", "")]), strike(&[("Strikethru", "0", ""), ("DoubleStrikethrough", "0", "")]));
    assert_ne!(strike(&[("DoubleStrikethrough", "0", "")]), strike(&[("Strikethru", "0", "")]));
    assert_eq!(strike(&[("Overline", "1", "")]), strike(&[]));
}

#[test]
fn a_style_that_is_no_whole_number_of_0_to_63_is_stamped_by_its_text_and_the_unit_is_the_style_groups() {
    // Deterministic, and it tells two texts apart.
    assert_ne!(under("64", "0"), under("65", "0"));
    assert_eq!(under("64", "0"), under("64", "0"));
    assert_ne!(rest("1.5"), rest("1.25"));
    assert_ne!(rest("-1"), rest("-2"));
    // The edges of the range: 63 is the last whole `Style`, 64 the first that is read by its text.
    assert_eq!(under("63", "0"), under("4", "0"), "63 has the underline bit");
    assert_ne!(under("64", "0"), under("0", "0"), "64 is read by its text, and so is not a `Style` with no underline");
    assert_eq!(rest("63"), rest("59"), "63 without its underline bit");
    assert_ne!(rest("64"), rest("0"));
    // The unit is the `Style` group's, and the underline's signature has none for `Style` (only `DblUnderline`'s).
    let with_unit = |unit: &str, group: TextGroup| stamp_text_group(&row_of(&[("Style", "5", unit), ("DblUnderline", "0", "")]), group);
    assert_ne!(with_unit("IN", TextGroup::Style), with_unit("", TextGroup::Style), "the unit of Style");
    assert_eq!(with_unit("IN", TextGroup::Underline), with_unit("", TextGroup::Underline), "the underline has no unit of Style");
}

/// The FNV-1a a `text-row` stamp is: the domain as a field (its length first), then the bytes of the key with nothing before them.
fn text_row_hash(key: &str) -> Stamp {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(&("text-row".len() as u64).to_le_bytes());
    feed(b"text-row");
    feed(key.as_bytes());
    Stamp(h)
}

#[test]
fn a_text_rows_stamp_is_the_hash_of_the_splice_writers_key() {
    // `name=text|unit;` for each cell in the cells' order; `name=?;` for a cell that is a formula or has no value.
    let mut row = row_of(&[("Size", "0.20", "IN"), ("Style", "1", "")]);
    assert_eq!(stamp_text_row(&row), text_row_hash("Size=0.20|IN;Style=1|;"));
    row.cells.insert(CellName::new("Case"), Cell::with_formula("1+1", Some(Value::from_text("2"))));
    row.cells.insert(CellName::new("Pos"), Cell::new());
    row.cells.insert(CellName::new("Color"), Cell::inherited(Some(Value::from_text("5"))));
    assert_eq!(stamp_text_row(&row), text_row_hash("Case=?;Color=?;Pos=?;Size=0.20|IN;Style=1|;"));
    // A `No Formula` constant is its value; a row with no cell is the domain alone, and every row with none is the same.
    row.cells.insert(CellName::new("Pos"), Cell { formula: diacraft_doc::FormulaSlot::NoFormula, value: Some(Value::from_text("2")), ..Cell::new() });
    assert_eq!(stamp_text_row(&row), text_row_hash("Case=?;Color=?;Pos=2|;Size=0.20|IN;Style=1|;"));
    assert_eq!(stamp_text_row(&Row::default()), text_row_hash(""));
    assert_eq!(stamp_text_row(&Row { ix: Some(4), name: Some(text("n")), del: true, ..Row::default() }), text_row_hash(""));
}

#[test]
fn a_text_rows_stamp_tells_what_the_key_tells_and_nothing_more() {
    let stamp = |cells: &[(&str, &str, &str)]| stamp_text_row(&row_of(cells));
    // The reviewer's pairs: each is two rows to the splice writer's key, and `stamp_cells` would take each for one value.
    assert_ne!(stamp(&[("Size", "0.2", "")]), stamp(&[("Size", "0.20", "")]));
    assert_ne!(stamp(&[("Size", "0.2", "")]), stamp(&[("Size", "0.2", "PT")]));
    assert_ne!(stamp(&[("Style", "1", "")]), stamp(&[("Style", "1.0", "")]));
    assert_ne!(stamp(&[("HorzAlign", "1", "")]), stamp(&[("HorzAlign", "1.0", "")]));
    assert_ne!(stamp(&[("IndLeft", "0.5", "IN")]), stamp(&[("IndLeft", "0.5", "")]));
    assert_ne!(stamp(&[("IndLeft", "0.5", "IN")]), stamp(&[("IndLeft", "0.5", "CM")]));
    // A cell more or less, another name, another value.
    assert_ne!(stamp(&[("Size", "1", "")]), stamp(&[("Size", "1", ""), ("Style", "1", "")]));
    assert_ne!(stamp(&[("Size", "1", "")]), stamp(&[("Style", "1", "")]));
    assert_ne!(stamp(&[("Size", "1", "")]), stamp(&[("Size", "2", "")]));
    assert_ne!(stamp(&[("Size", "1", "")]), stamp(&[]));
    // The same cells are the same row, in whatever order they were put.
    assert_eq!(stamp(&[("Size", "1", ""), ("Style", "2", "")]), stamp(&[("Style", "2", ""), ("Size", "1", "")]));
    // And where `stamp_cells` agrees with it, the shape's cells keep the rule they have: `1` and `1.0` are one value there.
    let sheet = |value: &str| sheet_with(&[("A", constant(value))]);
    assert_eq!(stamp_cells(&sheet("1"), &["A"]), stamp_cells(&sheet("1.0"), &["A"]));
}

#[test]
fn a_text_rows_stamp_costs_one_look_a_cell() {
    // 100,000 cells of one row: one unit of work each, and no text made of the key (the hash is fed a piece at a time).
    let mut row = Row::default();
    for k in 0..100_000_u32 {
        row.cells.insert(CellName::new(&format!("C{k:06}")), constant("1"));
    }
    work::reset();
    let _ = stamp_text_row(&row);
    assert_eq!(work::stamp_items(), 100_000);
    // A row with no cell costs nothing.
    work::reset();
    let _ = stamp_text_row(&Row::default());
    assert_eq!(work::stamp_items(), 0);
}

/// **The fill group lists every cell a fill's values rest on**: the `.odg` reader's fill stamp covers `FillGradientAngle` since a linear or axial gradient's stand-in
/// has an angle, so the group that names the cells a fill rests on names it too, and an edit of the angle alone (in LibreOffice, or here) is an edit of the fill. Red on `e6020b3b`: the list has no
/// `FillGradientAngle`.
#[test]
fn the_fill_group_names_the_angle_of_a_gradient_beside_its_other_cells() {
    let cells = HeldGroup::Fill.cells();
    for cell in ["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillGradientAngle", "FillForegnd", "FillForegndTrans"] {
        assert!(cells.contains(&cell), "{cell}: {cells:?}");
    }
    assert_eq!(HeldGroup::Fill.sections(), ["FillGradient"]);
}

#[test]
fn glue_stamps_the_whole_connection_row_and_names_no_cell() {
    // The glue point's value is stamped by "that row's cells (`stamp_row`)": every cell the row holds, not four that the group names. So the group names no
    // cell, and says that its row is the whole of what it rests on.
    assert!(HeldGroup::Glue.cells().is_empty());
    assert_eq!(HeldGroup::Glue.whole_rows(), ["Connection"]);
    for group in [HeldGroup::Fill, HeldGroup::Line, HeldGroup::LineEnds, HeldGroup::Shadow, HeldGroup::Text, HeldGroup::Frame, HeldGroup::Geometry] {
        assert!(group.whole_rows().is_empty(), "{group:?}");
    }
    // A cell of the row that no list names (`Prompt`, `Type`, `AutoGen`) is in the stamp: the stamp is `stamp_row`'s.
    let row = |extra: &str| {
        let mut row = connection_row("1");
        row.cells.insert(CellName::new(extra), constant("5"));
        row
    };
    assert_ne!(stamp_row(&row("Prompt")), stamp_row(&connection_row("1")));
    assert_ne!(stamp_row(&row("AutoGen")), stamp_row(&row("Type")));
}

#[test]
fn one_section_is_stamped_by_its_name_and_ix_and_the_first_of_them_is_the_one() {
    let geometry = |ix: Option<u32>, x: &str| {
        let mut section = Section::new("Geometry", ix);
        let mut row = Row { ix: Some(0), ..Row::default() };
        row.cells.insert(CellName::new("X"), constant(x));
        section.rows.push(Arc::new(row));
        Arc::new(section)
    };
    let route = sheet_of(vec![geometry(Some(0), "1"), geometry(Some(1), "2")]);
    let stamp = |sheet: &Sheet, ix: Option<u32>| stamp_section(sheet, "Geometry", ix);
    // The section by `IX` 0 is the route: the other `Geometry` sections are no part of it, wherever they sit.
    assert_eq!(stamp(&route, Some(0)), stamp(&sheet_of(vec![geometry(Some(0), "1")]), Some(0)));
    assert_eq!(stamp(&route, Some(0)), stamp(&sheet_of(vec![geometry(Some(1), "9"), geometry(Some(0), "1")]), Some(0)));
    assert_ne!(stamp(&route, Some(0)), stamp(&sheet_of(vec![geometry(Some(0), "9"), geometry(Some(1), "2")]), Some(0)));
    assert_ne!(stamp(&route, Some(0)), stamp(&route, Some(1)), "the section asked for is part of the stamp");
    // A section with no `IX` is the one asked for with `None`, and never the one asked for with `Some(0)`.
    let bare = sheet_of(vec![geometry(None, "1")]);
    assert_ne!(stamp(&bare, None), stamp(&bare, Some(0)));
    assert_eq!(stamp(&bare, Some(0)), stamp(&Sheet::default(), Some(0)), "no such section: the stamp of none");
    // What is asked for is part of even the stamp of none: no `Geometry` section 0 is not no `Geometry` section 1, nor no section called `Scratch`.
    assert_ne!(stamp(&Sheet::default(), Some(0)), stamp(&Sheet::default(), Some(1)));
    assert_ne!(stamp(&Sheet::default(), Some(0)), stamp(&Sheet::default(), None));
    assert_ne!(stamp(&Sheet::default(), Some(0)), stamp_section(&Sheet::default(), "Scratch", Some(0)));
    // Of two with one `IX` the first is the one found.
    let twice = sheet_of(vec![geometry(Some(0), "1"), geometry(Some(0), "2")]);
    assert_eq!(stamp(&twice, Some(0)), stamp(&sheet_of(vec![geometry(Some(0), "1")]), Some(0)));
    assert_ne!(stamp(&twice, Some(0)), stamp(&sheet_of(vec![geometry(Some(0), "2")]), Some(0)));
    // The name is part of it: the same rows in a section of another name are another section, found by that name alone.
    let scratch = {
        let mut section = (*geometry(Some(0), "1")).clone();
        section.name = diacraft_doc::SectionName::new("Scratch");
        sheet_of(vec![Arc::new(section)])
    };
    assert_eq!(stamp_section(&scratch, "Geometry", Some(0)), stamp(&Sheet::default(), Some(0)));
    assert_ne!(stamp_section(&scratch, "Scratch", Some(0)), stamp(&Sheet::default(), Some(0)));
    // A section is stamped as `stamp_sections` stamps it (what `section_same` compares), so a change of its attribute is a change.
    let mut attributed = (*geometry(Some(0), "1")).clone();
    attributed.raw = RawAttrs::from_pairs([("k", "v")]);
    assert_ne!(stamp(&sheet_of(vec![Arc::new(attributed)]), Some(0)), stamp(&sheet_of(vec![geometry(Some(0), "1")]), Some(0)));
}

#[test]
fn one_section_by_ix_costs_one_look_a_section_and_what_the_one_it_finds_holds() {
    // 10,000 sections, the route among them: each is looked at once, and only the one found is read (its 1,000 rows, one look each, and its cells).
    let mut sections: Vec<Arc<Section>> = (1..=10_000_u32).map(|i| Arc::new(Section::new("Geometry", Some(i)))).collect();
    let mut route = Section::new("Geometry", Some(0));
    for k in 0..1_000_u32 {
        route.rows.push(Arc::new(Row { ix: Some(k), ..Row::default() }));
    }
    sections.push(Arc::new(route));
    let sheet = Sheet { sections, ..Sheet::default() };
    work::reset();
    let _ = stamp_section(&sheet, "Geometry", Some(0));
    assert_eq!(work::stamp_items(), 10_001 + 1_000, "each section looked at once, the found one's rows read");
    // Nothing found: only the looks.
    work::reset();
    let _ = stamp_section(&sheet, "Geometry", Some(99_999));
    assert_eq!(work::stamp_items(), 10_001);
}

// ---------------------------------------------------------------------------------------------------------------- the caps

fn a_part(name: &str) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: text(name),
        media_type: text("application/octet-stream"),
        role: PartRole::Other,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&[][..]),
    })
}

fn a_definition(family: DefFamily, element: &Arc<RawElement>) -> Definition {
    Definition { family, name: text("d"), element: Arc::clone(element), bytes: None }
}

fn an_element() -> Arc<RawElement> {
    Arc::new(RawElement { name: text("draw:marker"), attrs: RawAttrs::new(), children: Vec::new() })
}

const FAMILIES: [(DefFamily, &str); 6] = [
    (DefFamily::Marker, "marker definitions"),
    (DefFamily::Gradient, "gradient definitions"),
    (DefFamily::Opacity, "opacity definitions"),
    (DefFamily::Hatch, "hatch definitions"),
    (DefFamily::StrokeDash, "stroke dash definitions"),
    (DefFamily::FillImage, "fill image definitions"),
];

fn doc_with_definitions(defs: Vec<Definition>) -> Document {
    let mut doc = Document::new_blank();
    doc.ext.make_mut().odf.definitions = defs;
    doc
}

#[test]
fn parts_are_capped_at_the_door() {
    assert_eq!(MAX_PARTS, 50_000);
    let one = a_part("/a.bin");
    let mut doc = Document::new_blank();
    doc.parts = std::iter::repeat_n(Arc::clone(&one), MAX_PARTS).collect();
    assert_eq!(doc.check_limits(), Ok(()));
    doc.parts.push(Arc::clone(&one));
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "parts", max: 50_000 }));
}

#[test]
fn definitions_are_capped_per_family() {
    assert_eq!(MAX_DEFINITIONS, 65_536);
    let element = an_element();
    for (family, what) in FAMILIES {
        let mut defs: Vec<Definition> = (0..MAX_DEFINITIONS).map(|_| a_definition(family, &element)).collect();
        assert_eq!(doc_with_definitions(defs.clone()).check_limits(), Ok(()), "{what} at the cap");
        defs.push(a_definition(family, &element));
        assert_eq!(doc_with_definitions(defs).check_limits(), Err(DocError::Limit { what, max: 65_536 }), "{what} one past the cap");
    }
    // Every family at its cap together is no breach: the cap is for each family, not for the lot.
    let all: Vec<Definition> = FAMILIES.iter().flat_map(|(f, _)| (0..MAX_DEFINITIONS).map(|_| a_definition(*f, &element))).collect();
    assert_eq!(all.len(), 6 * MAX_DEFINITIONS);
    assert_eq!(doc_with_definitions(all).check_limits(), Ok(()));
}

#[test]
fn a_family_over_its_cap_is_found_among_the_others() {
    // The check is a count by family: an overflow of one family is found however the others are placed around it.
    let element = an_element();
    let mut defs: Vec<Definition> = Vec::new();
    defs.extend((0..MAX_DEFINITIONS).map(|_| a_definition(DefFamily::Marker, &element)));
    defs.extend((0..=MAX_DEFINITIONS).map(|_| a_definition(DefFamily::Hatch, &element)));
    defs.extend((0..MAX_DEFINITIONS).map(|_| a_definition(DefFamily::FillImage, &element)));
    assert_eq!(doc_with_definitions(defs).check_limits(), Err(DocError::Limit { what: "hatch definitions", max: 65_536 }));
    // Interleaved: the first family to pass the cap is named.
    let mut defs: Vec<Definition> = Vec::new();
    for _ in 0..=MAX_DEFINITIONS {
        defs.push(a_definition(DefFamily::Gradient, &element));
        defs.push(a_definition(DefFamily::Opacity, &element));
    }
    assert_eq!(doc_with_definitions(defs).check_limits(), Err(DocError::Limit { what: "gradient definitions", max: 65_536 }));
}

#[test]
fn the_new_caps_are_checked_before_everything_else() {
    // A document past the cap of parts, of definitions and of pages says "parts"; past the second and the third, "definitions".
    let element = an_element();
    let mut doc = Document::new_blank();
    doc.pages = std::iter::repeat_n(Arc::new(Page::new(1, "p")), diacraft_doc::MAX_PAGES + 1).collect();
    doc.styles = std::iter::repeat_n(Arc::new(Style::default()), diacraft_doc::MAX_STYLES + 1).collect();
    assert_eq!(doc.check_limits(), Err(DocError::TooManyPages));
    doc.ext.make_mut().odf.definitions = (0..=MAX_DEFINITIONS).map(|_| a_definition(DefFamily::Marker, &element)).collect();
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "marker definitions", max: 65_536 }));
    doc.parts = std::iter::repeat_n(a_part("/a.bin"), MAX_PARTS + 1).collect();
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "parts", max: 50_000 }));
}

// ---------------------------------------------------------------------------------------------------------------- stamps

#[test]
fn stamp_text_is_sixteen_lowercase_hex_digits() {
    assert_eq!(Stamp(0xcbf2_9ce4_8422_2325).text(), "cbf29ce484222325");
    assert_eq!(Stamp(0).text(), "0000000000000000");
    assert_eq!(Stamp(u64::MAX).text(), "ffffffffffffffff");
    assert_eq!(Stamp::parse("cbf29ce484222325"), Some(Stamp(0xcbf2_9ce4_8422_2325)));
    assert_eq!(Stamp::parse("0000000000000000"), Some(Stamp(0)));
    assert_eq!(Stamp::parse("CBF29CE484222325"), None);
    assert_eq!(Stamp::parse("cbf29ce48422232"), None);
    assert_eq!(Stamp::parse("cbf29ce4842223250"), None);
    assert_eq!(Stamp::parse(""), None);
    assert_eq!(Stamp::parse("cbf29ce48422232g"), None);
    assert_eq!(Stamp::parse("+bf29ce484222325"), None);
    assert_eq!(Stamp::parse(" bf29ce484222325"), None);
    // Sixteen characters that are not sixteen bytes.
    assert_eq!(Stamp::parse("cbf29ce48422232\u{e9}"), None);
    for n in [0_u64, 1, 0xdead_beef, u64::MAX, 0x0123_4567_89ab_cdef] {
        assert_eq!(Stamp::parse(&Stamp(n).text()), Some(Stamp(n)));
    }
}

#[test]
fn digest_is_pinned() {
    assert_eq!(Digest::new("cells").text("LineWeight").finish().text(), "7bce65b857cfdb09");
    // The length prefix: two fields that split the same bytes differently are different inputs.
    let ab_c = Digest::new("d").text("ab").text("c").finish();
    let a_bc = Digest::new("d").text("a").text("bc").finish();
    assert_ne!(ab_c, a_bc);
    // `bytes` and `text` are one field the same way.
    assert_eq!(Digest::new("d").bytes(b"xy").finish(), Digest::new("d").text("xy").finish());
    // The domain is the first field.
    assert_ne!(Digest::new("a").finish(), Digest::new("b").finish());
}

#[test]
fn a_field_given_in_pieces_is_the_stamp_of_the_pieces_joined() {
    let joined: &[u8] = b"urn:one:extension and urn:one:extension again";
    for cut in 0..=joined.len() {
        let (a, b) = joined.split_at(cut);
        let pieces = Digest::new("bytes").bytes_in_pieces(joined.len() as u64, [a, b]).finish();
        assert_eq!(pieces, Digest::new("bytes").bytes(joined).finish(), "cut at {cut}");
    }
    // Three pieces, an empty one among them, and an empty field.
    let three = Digest::new("d").bytes_in_pieces(5, [&b"ab"[..], &b""[..], &b"cde"[..]]).finish();
    assert_eq!(three, Digest::new("d").bytes(b"abcde").finish());
    assert_eq!(Digest::new("d").bytes_in_pieces(0, []).finish(), Digest::new("d").bytes(b"").finish());
    // The length is the field's: pieces of another total are another stamp, not the stamp of the field they spell.
    assert_ne!(Digest::new("d").bytes_in_pieces(4, [&b"abcde"[..]]).finish(), Digest::new("d").bytes(b"abcde").finish());
}

fn line_sheet() -> Sheet {
    sheet_with(&[("LineWeight", constant("0.01").with_unit("IN")), ("LinePattern", constant("1")), ("FillPattern", constant("1"))])
}

#[test]
fn stamp_cells_does_not_depend_on_the_order_of_names_but_on_every_value() {
    let sheet = line_sheet();
    let ab = stamp_cells(&sheet, &["LineWeight", "LinePattern"]);
    assert_eq!(ab, stamp_cells(&sheet, &["LinePattern", "LineWeight"]));
    // A name given twice is the name once.
    assert_eq!(ab, stamp_cells(&sheet, &["LinePattern", "LineWeight", "LinePattern"]));
    let with = |edit: &dyn Fn(&mut Cell)| {
        let mut s = sheet.clone();
        edit(s.cells.get_mut("LineWeight").unwrap());
        stamp_cells(&s, &["LineWeight", "LinePattern"])
    };
    // A constant is stamped by its value; a formula by its text (below).
    assert_ne!(ab, with(&|c| c.value = Some(Value::from_text("0.02"))), "V of a constant");
    assert_ne!(ab, with(&|c| c.formula = Cell::with_formula("1/100", None).formula), "F added");
    assert_ne!(ab, with(&|c| c.value = None), "V of a constant removed");
    // The unit the value shows in, the error of the last evaluation and the cell's other attributes are no part of a stamp.
    assert_eq!(ab, with(&|c| c.unit = Some(text("MM"))), "U respelled");
    assert_eq!(ab, with(&|c| c.unit = None), "U removed");
    assert_eq!(ab, with(&|c| c.error = Some(text("#REF!"))), "E");
    // A cell the names do not name is no part of it.
    let mut other = sheet.clone();
    other.cells.insert(CellName::new("FillPattern"), constant("7"));
    other.cells.insert(CellName::new("Unrelated"), constant("7"));
    assert_eq!(ab, stamp_cells(&other, &["LineWeight", "LinePattern"]));
    // A named cell absent and present but empty are different.
    let mut empty = sheet.clone();
    empty.cells.insert(CellName::new("Shadow"), Cell::new());
    assert_ne!(stamp_cells(&sheet, &["Shadow"]), stamp_cells(&empty, &["Shadow"]));
    // The names are part of it.
    assert_ne!(stamp_cells(&sheet, &["LineWeight"]), stamp_cells(&sheet, &["LinePattern"]));
    assert_ne!(stamp_cells(&sheet, &["LineWeight"]), stamp_cells(&sheet, &["LineWeight", "LinePattern"]));
}

#[test]
fn a_formula_is_stamped_by_its_text_and_never_by_its_cached_result() {
    // A resize changes `Width` and every value computed from it, and leaves the stamp of a formula of the frame as it was.
    let frame = |width: &str, locpin: &str| {
        let mut cells = vec![("Width", constant(width)), ("LocPinX", Cell::with_formula("Width*0.5", Some(Value::from_text(locpin))))];
        cells.push(("Angle", Cell::inherited(Some(Value::from_text(locpin)))));
        sheet_with(&cells)
    };
    let names = ["LocPinX", "Angle"];
    let before = stamp_cells(&frame("4", "2"), &names);
    assert_eq!(before, stamp_cells(&frame("8", "4"), &names), "a resize leaves a formula, and an inherited one, as it was");
    assert_eq!(before, stamp_cells(&frame("8", "3.14"), &names), "whatever the cached result is");
    // The constant beside them is stamped by its value, so the same resize changes the stamp of the cells that hold it.
    assert_ne!(stamp_cells(&frame("4", "2"), &["Width"]), stamp_cells(&frame("8", "4"), &["Width"]));
    // A change of the formula's text is a change.
    let mut edited = frame("4", "2");
    edited.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.6", Some(Value::from_text("2"))));
    assert_ne!(before, stamp_cells(&edited, &names));
    // A formula that gains a cached result, or loses it, is the same formula; so is one whose unit is respelled.
    let mut bare = frame("4", "2");
    bare.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.5", None));
    assert_eq!(before, stamp_cells(&bare, &names));
    let mut unit = frame("4", "2");
    unit.cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.5", Some(Value::from_text("2"))).with_unit("IN"));
    assert_eq!(before, stamp_cells(&unit, &names), "the unit of a formula is no part of a stamp");
}

/// Whether two cells stamp alike on their own.
fn same_stamp(a: Cell, b: Cell) -> bool {
    stamp_cells(&sheet_with(&[("A", a)]), &["A"]) == stamp_cells(&sheet_with(&[("A", b)]), &["A"])
}

#[test]
fn stamp_cells_says_what_the_splice_writers_cell_same_says() {
    // `own::cell_same` (`crates/odg/src/write/own.rs`), case by case: two cells are the same when their formula slots are, and then a formula or an
    // inherited formula is the same whatever it calculated, and a constant is the same when its text is or when both are numbers that are equal.
    use diacraft_doc::FormulaSlot;
    let formula = |source: &str, v: Option<&str>| Cell::with_formula(source, v.map(Value::from_text));
    let constant_of = |v: Option<&str>| Cell { value: v.map(Value::from_text), ..Cell::new() };
    let no_formula = |v: Option<&str>| Cell { formula: FormulaSlot::NoFormula, value: v.map(Value::from_text), ..Cell::new() };
    // A formula: its text, and nothing a calculation made of it (its value, its error, its unit).
    assert!(same_stamp(formula("Width*0.5", Some("2")), formula("Width*0.5", Some("9"))), "a cached value");
    assert!(same_stamp(formula("Width*0.5", Some("2")), formula("Width*0.5", None)), "a cached value lost");
    assert!(same_stamp(formula("Width*0.5", None), Cell { error: Some(text("#N/A")), ..formula("Width*0.5", None) }), "an error");
    assert!(same_stamp(formula("Width*0.5", None), formula("Width*0.5", None).with_unit("IN")), "a unit");
    assert!(!same_stamp(formula("Width*0.5", Some("2")), formula("Width*0.6", Some("2"))), "another formula");
    // An inherited formula: the same whatever it calculated.
    assert!(same_stamp(Cell::inherited(Some(Value::from_text("2"))), Cell::inherited(Some(Value::from_text("9")))));
    assert!(same_stamp(Cell::inherited(None), Cell::inherited(Some(Value::from_text("9"))).with_unit("MM")));
    // Another slot is another cell, whatever it holds.
    assert!(!same_stamp(Cell::inherited(None), constant_of(None)));
    assert!(!same_stamp(formula("1", Some("1")), constant_of(Some("1"))));
    assert!(!same_stamp(constant_of(Some("1")), no_formula(Some("1"))), "`No Formula` is a slot of its own");
    assert!(!same_stamp(formula("Inh", None), Cell::inherited(None)), "a formula that reads `Inh` is not the marker");
    // A constant, or `No Formula`: equal text, or two numbers that are equal; and the unit it shows in is no edit.
    for make in [constant_of, no_formula] {
        assert!(same_stamp(make(Some("0.01")), make(Some("0.01"))));
        assert!(same_stamp(make(Some("1")), make(Some("1.0"))), "one number spelled twice");
        assert!(same_stamp(make(Some("1")), make(Some("1.000"))));
        assert!(same_stamp(make(Some("0")), make(Some("-0"))), "a zero and a negative zero are equal numbers");
        assert!(same_stamp(make(Some("1e1")), make(Some("10"))));
        assert!(same_stamp(make(Some("abc")), make(Some("abc"))), "equal text that is no number");
        assert!(same_stamp(make(None), make(None)));
        assert!(!same_stamp(make(Some("1")), make(Some("2"))));
        assert!(!same_stamp(make(Some("abc")), make(Some("abd"))));
        assert!(!same_stamp(make(Some("1")), make(Some("one"))), "a number and a word");
        assert!(!same_stamp(make(Some("")), make(None)), "empty and absent");
        assert!(!same_stamp(make(Some("1")), make(None)));
        assert!(!same_stamp(make(Some("#FF0000")), make(Some("#ff0000"))), "text is compared as it is");
    }
    assert!(same_stamp(constant_of(Some("2.54")).with_unit("IN"), constant_of(Some("2.54")).with_unit("CM")), "a unit respelled over an equal value");
    assert!(same_stamp(constant_of(Some("2.54")).with_unit("IN"), constant_of(Some("2.54"))), "and a unit given up");
    assert!(!same_stamp(constant_of(Some("2.54")).with_unit("IN"), constant_of(Some("2.55")).with_unit("IN")), "another value in the same unit");
}

#[test]
fn stamp_cells_tells_the_forms_of_a_formula_and_empty_from_absent_apart() {
    let stamp_of = |cell: Cell| stamp_cells(&sheet_with(&[("A", cell)]), &["A"]);
    let forms = [
        stamp_of(Cell::new()),
        stamp_of(Cell::inherited(None)),
        stamp_of(Cell { formula: diacraft_doc::FormulaSlot::NoFormula, ..Cell::new() }),
        stamp_of(Cell::with_formula("Inh", None)), // a formula of its own that reads "Inh": not the marker
        stamp_of(Cell::with_formula("No Formula", None)),
        stamp_of(Cell::with_formula("", None)),
    ];
    for (i, a) in forms.iter().enumerate() {
        for (j, b) in forms.iter().enumerate() {
            assert_eq!(a == b, i == j, "forms {i} and {j}");
        }
    }
    // The fields are told apart: the same text as a value and as a formula is two cells, and a value that is the text of a number is that number.
    assert!(!same_stamp(Cell { value: Some(Value::from_text("x")), ..Cell::new() }, Cell::with_formula("x", None)));
    assert!(!same_stamp(Cell { value: Some(Value::from_text("3")), ..Cell::new() }, Cell { value: Some(Value::from_text("x")), ..Cell::new() }));
    // The same word, by what is read from it: a number and a text of the bytes of that number are not one value.
    assert!(!same_stamp(
        Cell { value: Some(Value::from_text("1")), ..Cell::new() },
        Cell { value: Some(Value::from_text("\u{1}\0\0\0\0\0\0\0")), ..Cell::new() }
    ));
}

#[test]
fn stamp_cells_names_are_length_prefixed() {
    let sheet = Sheet::default();
    assert_ne!(stamp_cells(&sheet, &["ab", "c"]), stamp_cells(&sheet, &["a", "bc"]));
    assert_ne!(stamp_cells(&sheet, &[]), stamp_cells(&sheet, &[""]));
    assert_ne!(stamp_cells(&sheet, &["a"]), stamp_cells(&sheet, &["a", "b"]));
}

fn connection_row(x: &str) -> Row {
    let mut row = Row { ix: Some(4), row_type: None, ..Row::default() };
    for (name, value) in [("X", x), ("Y", "0.5"), ("DirX", "0"), ("DirY", "1")] {
        row.cells.insert(CellName::new(name), constant(value));
    }
    row
}

#[test]
fn stamp_row_follows_every_cell_and_the_flags_but_not_the_place() {
    let base = stamp_row(&connection_row("1"));
    assert_eq!(base, stamp_row(&connection_row("1")));
    assert_ne!(base, stamp_row(&connection_row("2")));
    let mut typed = connection_row("1");
    typed.row_type = Some(text("RelMoveTo"));
    assert_ne!(base, stamp_row(&typed));
    let mut deleted = connection_row("1");
    deleted.del = true;
    assert_ne!(base, stamp_row(&deleted));
    let mut extra = connection_row("1");
    extra.cells.insert(CellName::new("Type"), constant("0"));
    assert_ne!(base, stamp_row(&extra));
    let mut less = connection_row("1");
    less.cells.remove("DirY");
    assert_ne!(base, stamp_row(&less));
    let mut unit = connection_row("1");
    unit.cells.get_mut("Y").unwrap().unit = Some(text("IN"));
    assert_eq!(base, stamp_row(&unit), "the unit a value shows in is no part of a stamp");
    // The row's key is where it sits in its section, and a held value travels with the row: moving the row leaves the stand-in as it was.
    let mut moved = connection_row("1");
    moved.ix = Some(9);
    moved.name = Some(text("Row_9"));
    assert_eq!(base, stamp_row(&moved));
}

fn section_of(name: &str, ix: Option<u32>, rows: &[Row]) -> Arc<Section> {
    let mut section = Section::new(name, ix);
    section.rows = rows.iter().cloned().map(Arc::new).collect();
    Arc::new(section)
}

fn sheet_of(sections: Vec<Arc<Section>>) -> Sheet {
    Sheet { sections, ..Sheet::default() }
}

#[test]
fn stamp_sections_is_by_name_order_of_the_sheet_and_every_row() {
    let geometry0 = section_of("Geometry", Some(0), &[connection_row("1"), connection_row("2")]);
    let geometry1 = section_of("Geometry", Some(1), &[connection_row("3")]);
    let scratch = section_of("Scratch", None, &[connection_row("9")]);
    let sheet = sheet_of(vec![Arc::clone(&geometry0), Arc::clone(&scratch), Arc::clone(&geometry1)]);
    let base = stamp_sections(&sheet, &["Geometry", "User"]);
    // The order of the names is not the stamp's; a name twice is the name once.
    assert_eq!(base, stamp_sections(&sheet, &["User", "Geometry"]));
    assert_eq!(base, stamp_sections(&sheet, &["User", "Geometry", "User"]));
    // A section no name asks for is no part of it, wherever it sits.
    let changed_scratch = sheet_of(vec![Arc::clone(&geometry0), section_of("Scratch", None, &[connection_row("10")]), Arc::clone(&geometry1)]);
    assert_eq!(base, stamp_sections(&changed_scratch, &["Geometry", "User"]));
    // The order of the sections is part of it: `Geometry` 1 before `Geometry` 0 is another outline.
    let swapped = sheet_of(vec![Arc::clone(&geometry1), Arc::clone(&scratch), Arc::clone(&geometry0)]);
    assert_ne!(base, stamp_sections(&swapped, &["Geometry", "User"]));
    // A row, a row's cell, a section's `IX`, a deleted flag and a section's own cell each change it.
    let edited = |edit: &dyn Fn(&mut Section)| {
        let mut s = (*geometry0).clone();
        edit(&mut s);
        stamp_sections(&sheet_of(vec![Arc::new(s), Arc::clone(&scratch), Arc::clone(&geometry1)]), &["Geometry", "User"])
    };
    assert_ne!(base, edited(&|s| s.rows = s.rows.iter().take(1).cloned().collect()), "a row less");
    assert_ne!(base, edited(&|s| s.rows.push(Arc::new(connection_row("1")))), "a row more");
    assert_ne!(base, edited(&|s| s.rows.reverse()), "rows in another order");
    assert_ne!(base, edited(&|s| Arc::make_mut(&mut s.rows[0]).cells.get_mut("X").unwrap().value = Some(Value::from_text("7"))), "a row's cell");
    assert_ne!(base, edited(&|s| Arc::make_mut(&mut s.rows[0]).ix = Some(77)), "a row's IX is its key in a section");
    assert_ne!(base, edited(&|s| Arc::make_mut(&mut s.rows[0]).name = Some(text("n"))), "a row's N is its key in a section");
    assert_ne!(base, edited(&|s| Arc::make_mut(&mut s.rows[0]).row_type = Some(text("LineTo"))), "a row's T");
    assert_ne!(base, edited(&|s| Arc::make_mut(&mut s.rows[0]).del = true), "a row's Del");
    assert_ne!(base, edited(&|s| s.ix = Some(5)), "the section's IX");
    assert_ne!(base, edited(&|s| s.ix = None), "no IX");
    assert_ne!(base, edited(&|s| s.del = true), "the section's Del");
    assert_ne!(
        base,
        edited(&|s| {
            s.cells.insert(CellName::new("NoFill"), constant("1"));
        }),
        "a cell directly under the section"
    );
    // A cell moved from the section into its first row is another section: the counts delimit them.
    let mut under_section = (*section_of("Geometry", Some(0), &[Row::default()])).clone();
    under_section.cells.insert(CellName::new("X"), constant("1"));
    let mut under_row = (*section_of("Geometry", Some(0), &[Row::default()])).clone();
    Arc::make_mut(&mut under_row.rows[0]).cells.insert(CellName::new("X"), constant("1"));
    assert_ne!(
        stamp_sections(&sheet_of(vec![Arc::new(under_section)]), &["Geometry"]),
        stamp_sections(&sheet_of(vec![Arc::new(under_row)]), &["Geometry"])
    );
    // No section and a section with nothing in it are different; a name that matches none is the stamp of none.
    assert_ne!(stamp_sections(&Sheet::default(), &["Geometry"]), stamp_sections(&sheet_of(vec![section_of("Geometry", None, &[])]), &["Geometry"]));
    assert_eq!(stamp_sections(&sheet, &["Nothing"]), stamp_sections(&Sheet::default(), &["Nothing"]));
    // The names asked for are part of it.
    assert_ne!(stamp_sections(&Sheet::default(), &["A"]), stamp_sections(&Sheet::default(), &["B"]));
    // Section names that differ by a prefix are not each other.
    let sheet = sheet_of(vec![section_of("User", None, &[connection_row("1")])]);
    assert_ne!(stamp_sections(&sheet, &["User"]), stamp_sections(&sheet, &["Use"]));
}

/// The encoding `stamp.rs` documents, written out again here in the test's own words, so that a stamp stored in a file means the same in every build.
fn fnv(fields: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for field in fields {
        feed(&(field.len() as u64).to_le_bytes());
        feed(field);
    }
    h
}

/// The fields of an encoding, written out one by one in the test's own words (`stamp.rs`'s module docs say what each is).
#[derive(Default)]
struct Fields(Vec<Vec<u8>>);

impl Fields {
    fn of(domain: &str) -> Fields {
        Fields::default().text(domain)
    }
    fn bytes(mut self, bytes: &[u8]) -> Fields {
        self.0.push(bytes.to_vec());
        self
    }
    fn text(self, text: &str) -> Fields {
        self.bytes(text.as_bytes())
    }
    fn count(self, n: u64) -> Fields {
        self.bytes(&n.to_le_bytes())
    }
    fn flag(self, on: bool) -> Fields {
        self.bytes(&[u8::from(on)])
    }
    /// A number as a value is hashed: its `f64`, in 8 little-endian bytes.
    fn number(self, n: f64) -> Fields {
        self.bytes(&[1]).bytes(&n.to_bits().to_le_bytes())
    }
    /// What follows every cell that has no other attributes and keeps nothing inside it.
    fn bare(self) -> Fields {
        self.count(0).flag(false)
    }
    fn stamp(&self) -> Stamp {
        let fields: Vec<&[u8]> = self.0.iter().map(Vec::as_slice).collect();
        Stamp(fnv(&fields))
    }
}

#[test]
fn the_stamps_bytes_are_the_documented_encoding() {
    // `cells`: the domain, the number of names, then for each name (sorted, once) the name and the cell: a presence byte, then the formula's tag byte
    // (0 none, 1 a formula of its own, 2 `Inh`, 3 `No Formula`), and after it for tags 0 and 3 the value as an option (0 none, 1 a number and its 8
    // bytes, 2 any other text and the text), for tag 1 the formula's text, and for tag 2 nothing; then the cell's other attributes (their number, and each
    // name and value) and what it keeps inside it (a presence byte, then the text as an option, the nodes, the extension slot). No unit, no error and no
    // cached result is hashed.
    let sheet = sheet_with(&[("LineWeight", constant("0.01").with_unit("IN"))]);
    let expected = Fields::of("cells").count(1).text("LineWeight").flag(true).bytes(&[0]).number(0.01).bare().stamp();
    assert_eq!(stamp_cells(&sheet, &["LineWeight"]), expected);
    assert_eq!(stamp_cells(&sheet, &["LineWeight"]).text(), format!("{:016x}", expected.0));
    // An absent cell, a formula of its own (its cached result, unit and error are left out), an inherited formula, a `No Formula` constant that is a
    // number, a constant that is a word, a negative zero, a cell with nothing, and two cells that keep things beside and inside them.
    let mut formula = Cell::with_formula("1+1", Some(Value::from_text("2"))).with_unit("IN");
    formula.error = Some(text("#N/A"));
    let no_formula = Cell { formula: diacraft_doc::FormulaSlot::NoFormula, value: Some(Value::from_text("7")), ..Cell::new() };
    // `H`: a formula with an attribute of its own and, inside it, a text and one kept element that holds one attribute and one text.
    let mut kept = Passthrough::new();
    kept.push(
        1,
        RawNode::Element(RawElement {
            name: text("e"),
            attrs: RawAttrs::from_pairs([("a", "b")]),
            children: vec![RawNode::Text(text("z")), RawNode::Comment(text("c")), RawNode::Pi { target: text("t"), data: text("d") }],
        }),
    );
    let inside = CellExtra { text: Some(text("t")), passthrough: kept, ..CellExtra::default() };
    let mut with_both = Cell::with_formula("x", None);
    with_both.raw = RawAttrs::from_pairs([("k", "v")]);
    with_both.extra = Some(Arc::new(inside));
    // `I`: a constant whose extension slot holds a native attribute and a VSDX element.
    let mut ext_cell = constant("1");
    let mut extra = CellExtra::default();
    extra.ext.make_mut().native = kept_attr("n", "1");
    extra.ext.make_mut().vsdx.0.push((text("V"), kept_attr("m", "2")));
    ext_cell.extra = Some(Arc::new(extra));
    let sheet = sheet_with(&[
        ("B", formula),
        ("C", Cell::inherited(Some(Value::from_text("5"))).with_unit("IN")),
        ("D", no_formula),
        ("E", constant("abc")),
        ("F", Cell { value: Some(Value::from_text("-0")), ..Cell::new() }),
        ("G", Cell::new()),
        ("H", with_both),
        ("I", ext_cell),
    ]);
    let expected = Fields::of("cells")
        .count(9)
        .text("A")
        .flag(false) // absent
        .text("B")
        .flag(true)
        .bytes(&[1])
        .text("1+1")
        .bare() // a formula of its own
        .text("C")
        .flag(true)
        .bytes(&[2])
        .bare() // inherited
        .text("D")
        .flag(true)
        .bytes(&[3])
        .number(7.0)
        .bare() // No Formula, a constant that is a number
        .text("E")
        .flag(true)
        .bytes(&[0])
        .bytes(&[2])
        .text("abc")
        .bare() // a constant that is a word
        .text("F")
        .flag(true)
        .bytes(&[0])
        .number(0.0)
        .bare() // a negative zero is a zero
        .text("G")
        .flag(true)
        .bytes(&[0])
        .bytes(&[0])
        .bare() // no formula and no value
        .text("H")
        .flag(true)
        .bytes(&[1])
        .text("x")
        .count(1)
        .text("k")
        .text("v") // its other attributes
        .flag(true) // it keeps something inside it
        .flag(true)
        .text("t") // its text
        .count(1)
        .bytes(&1_u32.to_le_bytes()) // one node, after one typed child
        .bytes(&[0])
        .text("e")
        .count(1)
        .text("a")
        .text("b")
        .count(3) // an element, its name, its attributes, three children
        .bytes(&[1])
        .text("z")
        .bytes(&[2])
        .text("c")
        .bytes(&[3])
        .text("t")
        .text("d")
        .flag(false) // and no extension data
        .text("I")
        .flag(true)
        .bytes(&[0])
        .number(1.0)
        .count(0) // no other attributes
        .flag(true)
        .flag(false)
        .count(0) // it keeps something: no text, no nodes
        .flag(true) // an extension slot
        .count(0)
        .count(0) // its ODF side: no attributes, no nodes
        .count(1)
        .text("V")
        .count(1)
        .text("m")
        .text("2")
        .count(0) // one VSDX element, with one attribute
        .count(1)
        .text("n")
        .text("1")
        .count(0) // its native side: one attribute
        .stamp();
    assert_eq!(stamp_cells(&sheet, &["I", "H", "G", "F", "E", "D", "C", "B", "A"]), expected);
    // `row`: the domain, a Del byte, T as an option, the number of cells, then name and cell for each, in the cells' order.
    let mut row = Row { del: true, row_type: Some(text("LineTo")), ..Row::default() };
    row.cells.insert(CellName::new("X"), constant("1"));
    let expected = Fields::of("row").flag(true).flag(true).text("LineTo").count(1).text("X").flag(true).bytes(&[0]).number(1.0).bare().stamp();
    assert_eq!(stamp_row(&row), expected);
    // `sections`: the domain, the number of names, each name, then for each section that matches, in the sheet's order: its name, its IX as an
    // option (4 bytes, little-endian), Del, its other attributes and its kept nodes, the number of cells and the cells, the number of rows, and each row (IX, N
    // and LocalName as options, T, Del, its other attributes and kept nodes, cells).
    let mut section = Section::new("Geometry", Some(2));
    section.cells.insert(CellName::new("NoFill"), constant("0"));
    section.raw = RawAttrs::from_pairs([("k", "v")]);
    section.passthrough.push(3, RawNode::Comment(text("c")));
    let mut row = Row { ix: Some(0), name: None, local_name: Some(text("L")), row_type: Some(text("MoveTo")), ..Row::default() };
    row.raw = RawAttrs::from_pairs([("a", "b")]);
    row.passthrough.push(0, RawNode::Comment(text("d")));
    row.cells.insert(CellName::new("X"), constant("1"));
    section.rows.push(Arc::new(row));
    let sheet = sheet_of(vec![Arc::new(section)]);
    let fields = Fields::of("sections")
        .count(1)
        .text("Geometry") // the domain and the names
        .text("Geometry")
        .flag(true)
        .bytes(&2_u32.to_le_bytes())
        .flag(false) // the section: name, IX, Del
        .count(1)
        .text("k")
        .text("v") // its other attributes
        .count(1)
        .bytes(&3_u32.to_le_bytes())
        .bytes(&[2])
        .text("c") // its kept node: after 3 typed children, a comment
        .count(1)
        .text("NoFill")
        .flag(true)
        .bytes(&[0])
        .number(0.0)
        .bare() // its cells
        .count(1) // its rows
        .flag(true)
        .bytes(&0_u32.to_le_bytes())
        .flag(false) // the row: IX, no N
        .flag(true)
        .text("L") // LocalName
        .flag(true)
        .text("MoveTo")
        .flag(false) // T, Del
        .count(1)
        .text("a")
        .text("b") // its other attributes
        .count(1)
        .bytes(&0_u32.to_le_bytes())
        .bytes(&[2])
        .text("d") // its kept node
        .count(1)
        .text("X")
        .flag(true)
        .bytes(&[0])
        .number(1.0)
        .bare(); // its cell
    assert_eq!(stamp_sections(&sheet, &["Geometry"]), fields.stamp());
    // `section`: the domain, the name asked for, the `IX` asked for as an option, and then, if there is such a section, it as `sections` has it (the fields after the
    // domain, the number of names and the name above). None found is the three fields alone.
    let mut one = Fields::of("section").text("Geometry").flag(true).bytes(&2_u32.to_le_bytes());
    one.0.extend(fields.0[3..].iter().cloned());
    assert_eq!(stamp_section(&sheet, "Geometry", Some(2)), one.stamp());
    assert_eq!(stamp_section(&sheet, "Geometry", Some(9)), Fields::of("section").text("Geometry").flag(true).bytes(&9_u32.to_le_bytes()).stamp());
    assert_eq!(stamp_section(&sheet, "Scratch", None), Fields::of("section").text("Scratch").flag(false).stamp());
    // `rows`: the domain, the section's name, the prefix, each row that matches as a row of a section, then their number.
    let mut user = Section::new("User", None);
    for (name, value) in [("ODFEq1", "1"), ("ODFMod1", "2"), ("Other", "3")] {
        let mut row = Row { name: Some(text(name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), constant(value));
        user.rows.push(Arc::new(row));
    }
    let sheet = sheet_of(vec![Arc::new(user)]);
    let expected = Fields::of("rows")
        .text("User")
        .text("ODFEq") // the domain, the section, the prefix
        .flag(false)
        .flag(true)
        .text("ODFEq1")
        .flag(false) // no LocalName
        .flag(false)
        .flag(false) // no T, no Del
        .count(0)
        .count(0) // no other attributes, no kept nodes
        .count(1)
        .text("Value")
        .flag(true)
        .bytes(&[0])
        .number(1.0)
        .bare() // the one row
        .count(1) // how many
        .stamp();
    assert_eq!(stamp_rows_named(&sheet, "User", "ODFEq"), expected);
}

#[test]
fn a_stamp_covers_the_attributes_and_the_content_a_cell_keeps_beside_its_value_as_cell_same_does() {
    // "and either by the attributes and content the cell keeps beside them (`raw`, `extra`), as `cell_same` is".
    let cells = [constant("1"), Cell::with_formula("Width*2", Some(Value::from_text("2"))), Cell::inherited(None)];
    for (n, base) in cells.iter().enumerate() {
        let stamp = |cell: &Cell| stamp_cells(&sheet_with(&[("A", cell.clone())]), &["A"]);
        let before = stamp(base);
        let edited = |edit: &dyn Fn(&mut Cell)| {
            let mut cell = base.clone();
            edit(&mut cell);
            stamp(&cell)
        };
        let node = |after: u32, node: RawNode| {
            let mut kept = Passthrough::new();
            kept.push(after, node);
            Arc::new(CellExtra { passthrough: kept, ..CellExtra::default() })
        };
        assert_ne!(before, edited(&|c| c.raw = RawAttrs::from_pairs([("k", "v")])), "an attribute, cell {n}");
        assert_ne!(
            edited(&|c| c.raw = RawAttrs::from_pairs([("k", "v")])),
            edited(&|c| c.raw = RawAttrs::from_pairs([("k", "w")])),
            "its value, cell {n}"
        );
        assert_ne!(
            edited(&|c| c.raw = RawAttrs::from_pairs([("k", "v")])),
            edited(&|c| c.raw = RawAttrs::from_pairs([("j", "v")])),
            "its name, cell {n}"
        );
        assert_ne!(
            edited(&|c| c.raw = RawAttrs::from_pairs([("a", "b"), ("c", "d")])),
            edited(&|c| c.raw = RawAttrs::from_pairs([("c", "d"), ("a", "b")])),
            "their order, cell {n}"
        );
        assert_ne!(
            before,
            edited(&|c| c.extra = Some(Arc::new(CellExtra::default()))),
            "an empty `extra` is not none, as the derived equality says, cell {n}"
        );
        assert_ne!(
            before,
            edited(&|c| c.extra = Some(Arc::new(CellExtra { text: Some(text("t")), ..CellExtra::default() }))),
            "text inside, cell {n}"
        );
        assert_ne!(
            edited(&|c| c.extra = Some(Arc::new(CellExtra { text: Some(text("t")), ..CellExtra::default() }))),
            edited(&|c| c.extra = Some(Arc::new(CellExtra { text: Some(text("u")), ..CellExtra::default() }))),
            "another text, cell {n}"
        );
        let comment = |t: &str| RawNode::Comment(text(t));
        assert_ne!(before, edited(&|c| c.extra = Some(node(0, comment("c")))), "a node, cell {n}");
        assert_ne!(edited(&|c| c.extra = Some(node(0, comment("c")))), edited(&|c| c.extra = Some(node(1, comment("c")))), "its place, cell {n}");
        assert_ne!(edited(&|c| c.extra = Some(node(0, comment("c")))), edited(&|c| c.extra = Some(node(0, comment("d")))), "its text, cell {n}");
        assert_ne!(
            edited(&|c| c.extra = Some(node(0, comment("c")))),
            edited(&|c| c.extra = Some(node(0, RawNode::Text(text("c"))))),
            "its kind, cell {n}"
        );
        let element = |children: Vec<RawNode>| RawNode::Element(RawElement { name: text("e"), attrs: RawAttrs::new(), children });
        assert_ne!(
            edited(&|c| c.extra = Some(node(0, element(vec![])))),
            edited(&|c| c.extra = Some(node(0, element(vec![comment("c")])))),
            "a child, cell {n}"
        );
        // The slot: native data, ODF data and VSDX elements are three things, and an empty slot is none.
        let with_ext = |edit: &dyn Fn(&mut diacraft_doc::ExtData<Kept>)| {
            let mut extra = CellExtra::default();
            edit(extra.ext.make_mut());
            Some(Arc::new(extra))
        };
        assert_ne!(
            edited(&|c| c.extra = with_ext(&|d| d.native = kept_attr("a", "1"))),
            edited(&|c| c.extra = with_ext(&|d| d.odf = kept_attr("a", "1"))),
            "native or ODF, cell {n}"
        );
        assert_ne!(
            edited(&|c| c.extra = with_ext(&|d| d.native = kept_attr("a", "1"))),
            edited(&|c| c.extra = with_ext(&|d| d.vsdx.0.push((text("a"), kept_attr("a", "1"))))),
            "native or VSDX, cell {n}"
        );
        assert_ne!(before, edited(&|c| c.extra = with_ext(&|d| d.native = kept_attr("a", "1"))), "extension data, cell {n}");
        assert_eq!(
            edited(&|c| c.extra = Some(Arc::new(CellExtra::default()))),
            edited(&|c| c.extra = with_ext(&|_| {})),
            "an empty slot is an absent one, as the equality says, cell {n}"
        );
    }
}

#[test]
fn a_group_is_only_a_grouping_and_each_held_value_has_a_stamp_of_its_own() {
    let groups = [
        HeldGroup::Fill,
        HeldGroup::Line,
        HeldGroup::LineEnds,
        HeldGroup::Shadow,
        HeldGroup::Text,
        HeldGroup::Frame,
        HeldGroup::Glue,
        HeldGroup::Geometry,
    ];
    for (i, a) in groups.iter().enumerate() {
        for (j, b) in groups.iter().enumerate() {
            assert_eq!(a == b, i == j);
        }
    }
    // Several values of one group, each with its own stamp: a shape that holds two (a gradient and an opacity of the fill) is not the shape that holds
    // them with the stamp of one changed, and the shape that holds one is not the shape that holds both.
    let value = |stamp: Option<Stamp>, name: &str| Held {
        group: HeldGroup::Fill,
        stamp,
        attrs: RawAttrs::from_pairs([("draw:fill-gradient-name", name)]),
        kept: Passthrough::new(),
    };
    let mut shape = Shape::new(1);
    shape.ext.make_mut().odf.held = vec![value(Some(Stamp(1)), "g1"), value(Some(Stamp(2)), "o1")];
    let mut other = shape.clone();
    assert_eq!(shape, other);
    other.ext.make_mut().odf.held[1].stamp = Some(Stamp(3));
    assert_ne!(shape, other, "the second value's stamp is its own");
    let mut fewer = shape.clone();
    fewer.ext.make_mut().odf.held.pop();
    assert_ne!(shape, fewer);
    // A value with no stamp (the frame's, a circle's centre form) is not a value with a stamp.
    let unstamped =
        Held { group: HeldGroup::Frame, stamp: None, attrs: RawAttrs::from_pairs([("draw:transform", "skewX (0.2)")]), kept: Passthrough::new() };
    let mut with_none = Shape::new(1);
    with_none.ext.make_mut().odf.held = vec![unstamped.clone()];
    let mut with_zero = with_none.clone();
    with_zero.ext.make_mut().odf.held[0].stamp = Some(Stamp(0));
    assert_ne!(with_none, with_zero);
    // What a group's values rest on, in all: every group but the frame's rests on some cells or rows; the frame's is judged by the pose, so it stamps
    // none, and the geometry's is rows and no cell.
    for group in groups {
        let stamps_something =
            !group.cells().is_empty() || !group.sections().is_empty() || !group.row_prefixes().is_empty() || !group.whole_rows().is_empty();
        assert_eq!(stamps_something, group != HeldGroup::Frame, "{group:?}");
        let mut sorted: Vec<&str> = group.cells().to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), group.cells().len(), "{group:?} lists a cell twice");
    }
    assert!(HeldGroup::Geometry.cells().is_empty(), "the geometry's stamp is rows");
    // `LineWeight` is in one group only, `LineEnds`, for the width of DiaCraft's own marker that no size class matches (a foreign marker's width and a
    // dash's lengths are their own, so a weight edit leaves both held values right).
    for group in groups {
        assert_eq!(group.cells().contains(&"LineWeight"), group == HeldGroup::LineEnds, "{group:?}");
    }
}

#[test]
fn held_group_names_are_cells_and_sections_the_catalogue_lists() {
    // A name with a typo would stamp a cell nothing holds, and the held value would be good for ever. Every name is a cell the catalogue knows:
    // a direct cell of a sheet, or for the glue group a cell of a `Connection` row.
    for group in [HeldGroup::Fill, HeldGroup::Line, HeldGroup::LineEnds, HeldGroup::Shadow, HeldGroup::Text, HeldGroup::Frame, HeldGroup::Geometry] {
        for name in group.cells() {
            assert!(listed_cells().any(|(section, n, _)| section.is_none() && n == *name), "{group:?}: {name} is no listed direct cell");
        }
    }
    for name in HeldGroup::Glue.cells() {
        assert!(listed_cells().any(|(section, n, _)| section == Some("Connection") && n == *name), "Glue: {name} is no cell of a Connection row");
    }
    // The sections and the rows' sections a group reads are section names the catalogue knows.
    for group in [HeldGroup::Fill, HeldGroup::Glue, HeldGroup::Geometry] {
        for name in group.sections() {
            assert!(diacraft_doc::SECTION_NAMES.contains(name), "{group:?}: {name}");
        }
        for (section, prefix) in group.row_prefixes() {
            assert!(diacraft_doc::SECTION_NAMES.contains(section), "{group:?}: {section}");
            assert!(!prefix.is_empty(), "{group:?}: a prefix that every row has");
        }
    }
}

#[test]
fn the_groups_stamp_what_the_ownership_rule_names() {
    // The table, group by group: "Cells stamped" and "Rows stamped".
    let set = |g: HeldGroup| -> Vec<&'static str> {
        let mut v = g.cells().to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(
        set(HeldGroup::Fill),
        ["FillForegnd", "FillForegndTrans", "FillGradientAngle", "FillGradientDir", "FillGradientEnabled", "FillPattern"]
    );
    // `LinePattern` stamps `draw:stroke="dash"`.
    assert_eq!(set(HeldGroup::Line), ["LineCap", "LinePattern"]);
    assert_eq!(set(HeldGroup::LineEnds), ["BeginArrow", "BeginArrowSize", "EndArrow", "EndArrowSize", "LineWeight"]);
    // Every cell the reader sets from the shadow's attributes.
    assert_eq!(set(HeldGroup::Shadow), ["ShapeShdwOffsetX", "ShapeShdwOffsetY", "ShapeShdwType", "ShdwForegnd", "ShdwForegndTrans", "ShdwPattern"]);
    assert_eq!(set(HeldGroup::Text), ["BottomMargin", "LeftMargin", "RightMargin", "TextDirection", "TopMargin", "VerticalAlign"]);
    // The frame's value is judged by the pose on the page, through every enclosing group: no stamp, so no cell.
    assert_eq!(set(HeldGroup::Frame), Vec::<&str>::new());
    // The glue point's value is stamped by the whole `Connection` row (`stamp_row`), so the group names no cell.
    assert_eq!(set(HeldGroup::Glue), Vec::<&str>::new());
    assert_eq!(set(HeldGroup::Geometry), Vec::<&str>::new());
    // Rows: the fill's stops, and the outline with the equations beside it (not the modifiers); the glue's one row is stamped on its own.
    assert_eq!(HeldGroup::Fill.sections(), ["FillGradient"]);
    assert_eq!(HeldGroup::Geometry.sections(), ["Geometry"]);
    assert_eq!(HeldGroup::Geometry.row_prefixes(), [("User", "ODFEq")]);
    for group in [HeldGroup::Line, HeldGroup::LineEnds, HeldGroup::Shadow, HeldGroup::Text, HeldGroup::Frame, HeldGroup::Glue] {
        assert!(group.sections().is_empty() && group.row_prefixes().is_empty(), "{group:?}");
    }
    assert!(HeldGroup::Fill.row_prefixes().is_empty());
}

#[test]
fn a_resize_leaves_the_stamps_of_formulas_of_the_frame_and_of_a_held_geometry_as_they_were_and_an_edit_of_what_they_rest_on_does_not() {
    // The reason for stamping a formula by its text: a resize changes `Width`, `Height` and every cached value computed from them. A frame whose
    // cells are formulas of the frame, and a geometry whose rows are, are as they were; an equation edit is a change, a modifier drag is not.
    let build = |width: &str, half: &str, eq: &str, modifier: &str| {
        let mut sheet = sheet_with(&[
            ("LocPinX", Cell::with_formula("Width*0.5", Some(Value::from_text(half)))),
            ("Angle", Cell::inherited(Some(Value::from_text("0")))),
            ("Width", Cell::with_formula("Sheet.1!Width*2", Some(Value::from_text(width)))),
        ]);
        let mut geometry = Section::new("Geometry", Some(0));
        let mut row = Row { ix: Some(1), row_type: Some(text("LineTo")), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::with_formula("Width*1", Some(Value::from_text(width))));
        geometry.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(geometry));
        let mut user = Section::new("User", None);
        for (name, formula) in [("ODFEq1", eq), ("ODFMod1", modifier), ("Other", "9")] {
            let mut row = Row { name: Some(text(name)), ..Row::default() };
            row.cells.insert(CellName::new("Value"), Cell::with_formula(formula, Some(Value::from_text("0"))));
            user.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(user));
        sheet
    };
    let stamps = |sheet: &Sheet| {
        let frame = stamp_cells(sheet, &["LocPinX", "Angle", "Width"]);
        let geometry = {
            let mut d = Digest::new("geometry");
            d.bytes(&stamp_sections(sheet, HeldGroup::Geometry.sections()).0.to_le_bytes());
            for (section, prefix) in HeldGroup::Geometry.row_prefixes() {
                d.bytes(&stamp_rows_named(sheet, section, prefix).0.to_le_bytes());
            }
            d.finish()
        };
        (frame, geometry)
    };
    let before = stamps(&build("4", "2", "1+1", "7"));
    assert_eq!(before.0, stamps(&build("8", "4", "1+1", "7")).0, "a resize: the frame's formulas stand");
    assert_eq!(before.1, stamps(&build("8", "4", "1+1", "7")).1, "a resize: the outline's formulas stand");
    assert_eq!(before.1, stamps(&build("4", "2", "1+1", "99")).1, "a modifier's drag is no change of the geometry");
    assert_ne!(before.1, stamps(&build("4", "2", "1+2", "7")).1, "an equation edit is");
}

#[test]
fn the_nodes_a_cell_keeps_are_walked_once_each_whatever_their_shape() {
    // A stamp walks what a cell keeps inside it, which a file brought as wide or as deep as its parser allowed. One unit of work for each node,
    // with no recursion: 100,000 children of one element, and a chain of 3,000 elements (built from the inside out), each cost their nodes and no more.
    let cell_keeping = |node: RawNode| {
        let mut kept = Passthrough::new();
        kept.push(0, node);
        let mut cell = constant("1");
        cell.extra = Some(Arc::new(CellExtra { passthrough: kept, ..CellExtra::default() }));
        sheet_with(&[("A", cell)])
    };
    let element = |children: Vec<RawNode>| RawNode::Element(RawElement { name: text("e"), attrs: RawAttrs::new(), children });
    let wide = cell_keeping(element((0..100_000).map(|k| RawNode::Text(text(&k.to_string()))).collect()));
    work::reset();
    let _ = stamp_cells(&wide, &["A"]);
    assert_eq!(work::stamp_items(), 1 + 1 + 100_000, "the name, the element and its children");
    let mut chain = RawNode::Text(text("x"));
    for _ in 0..3_000 {
        chain = element(vec![chain]);
    }
    let deep = cell_keeping(chain);
    work::reset();
    let stamp = stamp_cells(&deep, &["A"]);
    assert_eq!(work::stamp_items(), 1 + 3_001, "the name, 3,000 elements and the text");
    assert_eq!(stamp, stamp_cells(&deep.clone(), &["A"]));
    // Two cells that keep the same tree of nodes in another shape are not the same: a child of the first element, and a sibling of it.
    let nested = cell_keeping(element(vec![element(vec![])]));
    let flat = {
        let mut kept = Passthrough::new();
        kept.push(0, element(vec![]));
        kept.push(0, element(vec![]));
        let mut cell = constant("1");
        cell.extra = Some(Arc::new(CellExtra { passthrough: kept, ..CellExtra::default() }));
        sheet_with(&[("A", cell)])
    };
    assert_ne!(stamp_cells(&nested, &["A"]), stamp_cells(&flat, &["A"]));
}

// ---------------------------------------------------------------------------------------------------------------- the third tag, and the types

#[test]
fn a_part_a_newer_build_wrote_is_held_under_a_third_tag() {
    // Placement note 11 (Call 19): an unknown-role part of a newer `.diacraft` is held in `Document.parts` under `Native`.
    let tags = [Format::Vsdx, Format::Odf, Format::Native];
    for (i, a) in tags.iter().enumerate() {
        for (j, b) in tags.iter().enumerate() {
            assert_eq!(a == b, i == j);
        }
    }
    let part = Part {
        format: Format::Native,
        name: text("extras/newer.bin"),
        media_type: text("application/vnd.example.newer"),
        role: PartRole::Other,
        manifest: RawAttrs::from_pairs([("role", "newer-role")]),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&[0_u8, 1, 2][..]),
    };
    let mut doc = Document::new_blank();
    doc.parts.push(Arc::new(part.clone()));
    assert_eq!(doc.parts.get(0).map(|p| p.as_ref()), Some(&part));
    let _ = VsdxRest::default();
}

#[test]
fn the_roles_are_distinct_values() {
    let roles = [
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
    for (i, a) in roles.iter().enumerate() {
        for (j, b) in roles.iter().enumerate() {
            assert_eq!(a == b, i == j);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- crafted inputs

use diacraft_doc::work;

#[test]
fn fifty_thousand_parts_of_nothing_cost_the_check_no_walk_of_them() {
    // The cap is a length test: the parts are not walked, however many.
    let one = a_part("/a.bin");
    let mut doc = Document::new_blank();
    doc.parts = std::iter::repeat_n(Arc::clone(&one), MAX_PARTS).collect();
    work::reset();
    assert_eq!(doc.check_limits(), Ok(()));
    assert_eq!(work::limit_items(), 0, "no part and no definition is examined");
    // Many parts do not make a definition count cost anything either: nothing is held for the document's ODF data.
    assert!(doc.ext.get().is_none());
}

#[test]
fn six_families_at_the_cap_cost_one_pass_and_an_overflow_stops_at_the_first_item_past_it() {
    let element = an_element();
    let all: Vec<Definition> = FAMILIES.iter().flat_map(|(f, _)| (0..MAX_DEFINITIONS).map(|_| a_definition(*f, &element))).collect();
    let doc = doc_with_definitions(all);
    work::reset();
    assert_eq!(doc.check_limits(), Ok(()));
    assert_eq!(work::limit_items(), 6 * 65_536, "one pass, each definition looked at once");
    // A hostile list of ten times the cap in one family: the check ends at the item that breaks it, and the rest of the list is never read.
    let hostile: Vec<Definition> = (0..10 * MAX_DEFINITIONS).map(|_| a_definition(DefFamily::Marker, &element)).collect();
    let doc = doc_with_definitions(hostile);
    work::reset();
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "marker definitions", max: 65_536 }));
    assert_eq!(work::limit_items(), 65_537, "stopped at the first item past the cap");
    // Interleaved families, the worst case for a count by family: a pass of at most 6 x the cap and one, whichever family is first to pass.
    let mut mixed: Vec<Definition> = Vec::new();
    for _ in 0..MAX_DEFINITIONS {
        for (f, _) in FAMILIES {
            mixed.push(a_definition(f, &element));
        }
    }
    mixed.push(a_definition(DefFamily::FillImage, &element));
    let doc = doc_with_definitions(mixed);
    work::reset();
    assert_eq!(doc.check_limits(), Err(DocError::Limit { what: "fill image definitions", max: 65_536 }));
    assert_eq!(work::limit_items(), 6 * 65_536 + 1);
}

#[test]
fn a_shape_holding_a_hundred_thousand_held_items_costs_the_check_nothing() {
    let attrs = RawAttrs::from_pairs([("draw:fill", "hatch")]);
    let held = Held { group: HeldGroup::Fill, stamp: Some(Stamp(7)), attrs, kept: Passthrough::new() };
    let mut shape = Shape::new(1);
    shape.ext.make_mut().odf.held = std::iter::repeat_n(held, 100_000).collect();
    let mut page = Page::new(1, "p");
    page.shapes.push(Arc::new(shape));
    let mut doc = Document::new_blank();
    doc.pages = std::iter::once(Arc::new(page)).collect();
    work::reset();
    assert_eq!(doc.check_limits(), Ok(()));
    assert_eq!(work::limit_items(), 0, "the check does not read held values");
    // An undo step is a clone: it shares the 100,000 items and copies a pointer.
    let step = doc.clone();
    let (a, b) = (doc.pages.get(0).unwrap().shapes.get(0).unwrap(), step.pages.get(0).unwrap().shapes.get(0).unwrap());
    assert!(Arc::ptr_eq(a, b));
    assert_eq!(doc, step);
}

#[test]
fn a_chain_of_nested_groups_each_holding_the_same_slot_shares_it() {
    // 200 levels, each holding the one slot of 10,000 held items. Cloning the chain and comparing it costs the levels, not the items.
    let mut slot: Ext<OdfShape> = Ext::default();
    slot.make_mut().odf.held =
        (0..10_000).map(|i| Held { group: HeldGroup::Line, stamp: Some(Stamp(i)), attrs: RawAttrs::new(), kept: Passthrough::new() }).collect();
    let mut inner = diacraft_doc::ShapeList::new();
    for level in (0..diacraft_doc::MAX_SHAPE_DEPTH - 1).rev() {
        let mut shape = Shape::new(u32::try_from(level).unwrap() + 1);
        shape.ext = slot.clone();
        shape.children = inner;
        inner = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape)]);
    }
    let mut page = Page::new(1, "p");
    page.shapes = inner;
    let mut doc = Document::new_blank();
    doc.pages = std::iter::once(Arc::new(page)).collect();
    work::reset();
    assert_eq!(doc.check_limits(), Ok(()));
    assert_eq!(work::limit_items(), 0);
    assert_eq!(doc, doc.clone());
}

#[test]
fn stamp_cells_over_a_hundred_thousand_cells_reads_the_cells_it_is_asked_for() {
    let mut sheet = Sheet::default();
    let names: Vec<String> = (0..100_000).map(|i| format!("C{i:06}")).collect();
    for name in &names {
        sheet.cells.insert(CellName::new(name), constant("1"));
    }
    // A handful of names costs a handful of cells, not a pass over the sheet.
    work::reset();
    let _ = stamp_cells(&sheet, &["C000001", "C050000", "NotThere"]);
    assert_eq!(work::stamp_items(), 3);
    // Every name costs every cell once: linear.
    let all: Vec<&str> = names.iter().map(String::as_str).collect();
    work::reset();
    let _ = stamp_cells(&sheet, &all);
    assert_eq!(work::stamp_items(), 100_000);
    // The names given in any order, and given twice, cost the same cells and give the same stamp: a name is looked at once.
    let mut doubled: Vec<&str> = all.iter().rev().copied().collect();
    doubled.extend(all.iter().copied());
    work::reset();
    let from_doubled = stamp_cells(&sheet, &doubled);
    assert_eq!(work::stamp_items(), 100_000);
    assert_eq!(from_doubled, stamp_cells(&sheet, &all));
}

#[test]
fn stamp_sections_reads_each_section_of_the_sheet_once_whatever_the_names() {
    let mut sheet = Sheet::default();
    for i in 0..10_000_u32 {
        sheet.sections.push(section_of(if i % 2 == 0 { "Geometry" } else { "Scratch" }, Some(i), &[]));
    }
    let names_many: Vec<String> = (0..1_000).map(|i| format!("Name{i}")).collect();
    let names: Vec<&str> = names_many.iter().map(String::as_str).chain(["Geometry"]).collect();
    work::reset();
    let _ = stamp_sections(&sheet, &names);
    // One look at each section (10,000): not 10,000 looks for each of the 1,001 names.
    assert_eq!(work::stamp_items(), 10_000);
    // A section that matches costs its rows and their cells, once each: 500 sections of 4 rows of 5 cells beside 500 that do not match.
    let mut row = Row::default();
    for name in ["A", "B", "C", "D", "E"] {
        row.cells.insert(CellName::new(name), constant("1"));
    }
    let mut sheet = Sheet::default();
    for i in 0..1_000_u32 {
        sheet.sections.push(section_of(
            if i % 2 == 0 { "Geometry" } else { "Scratch" },
            Some(i),
            &[row.clone(), row.clone(), row.clone(), row.clone()],
        ));
    }
    work::reset();
    let _ = stamp_sections(&sheet, &names);
    assert_eq!(work::stamp_items(), 1_000 + 500 * 4 + 500 * 4 * 5);
}

fn user_rows(rows: &[(&str, &str)]) -> Sheet {
    let mut user = Section::new("User", None);
    for (name, formula) in rows {
        let mut row = Row { name: Some(text(name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), Cell::with_formula(formula, Some(Value::from_text("0"))));
        user.rows.push(Arc::new(row));
    }
    sheet_of(vec![Arc::new(user)])
}

#[test]
fn stamp_rows_named_is_the_rows_of_that_name_in_that_section_and_nothing_beside_them() {
    let base = user_rows(&[("ODFEq0", "1"), ("ODFMod0", "2"), ("Mine", "3"), ("ODFEq1", "4")]);
    let stamp = stamp_rows_named(&base, "User", "ODFEq");
    // A row that does not start with the prefix is no part of it, whatever it holds, and nor is the order of the other rows.
    assert_eq!(stamp, stamp_rows_named(&user_rows(&[("ODFEq0", "1"), ("ODFMod0", "99"), ("Mine", "99"), ("ODFEq1", "4")]), "User", "ODFEq"));
    assert_eq!(stamp, stamp_rows_named(&user_rows(&[("Mine", "3"), ("ODFEq0", "1"), ("ODFEq1", "4"), ("ODFMod0", "2")]), "User", "ODFEq"));
    // A row that does, is: its formula, its key, and which rows there are, and their order.
    assert_ne!(stamp, stamp_rows_named(&user_rows(&[("ODFEq0", "1+1"), ("ODFMod0", "2"), ("Mine", "3"), ("ODFEq1", "4")]), "User", "ODFEq"));
    assert_ne!(stamp, stamp_rows_named(&user_rows(&[("ODFEq0", "1"), ("ODFMod0", "2"), ("Mine", "3")]), "User", "ODFEq"));
    assert_ne!(stamp, stamp_rows_named(&user_rows(&[("ODFEq1", "4"), ("ODFMod0", "2"), ("Mine", "3"), ("ODFEq0", "1")]), "User", "ODFEq"));
    assert_ne!(stamp, stamp_rows_named(&user_rows(&[("ODFEq0", "1"), ("ODFMod0", "2"), ("Mine", "3"), ("ODFEq2", "4")]), "User", "ODFEq"));
    // The match is by prefix, and by the row's `N`: a row with no name matches nothing, and the stamp of none is the stamp of a sheet with none.
    assert_ne!(stamp, stamp_rows_named(&base, "User", "ODFEq0"));
    assert_ne!(stamp, stamp_rows_named(&base, "User", "ODF"));
    let unnamed = sheet_of(vec![section_of("User", None, &[Row::default()])]);
    assert_eq!(stamp_rows_named(&unnamed, "User", "ODFEq"), stamp_rows_named(&Sheet::default(), "User", "ODFEq"));
    // Only the section of that name: the same rows in another section are not it, and the section and the prefix are part of the stamp.
    let mut elsewhere = user_rows(&[]);
    elsewhere.sections.push(Arc::new({
        let mut s = Section::new("Scratch", None);
        s.rows = base.sections[0].rows.clone();
        s
    }));
    assert_eq!(stamp_rows_named(&elsewhere, "User", "ODFEq"), stamp_rows_named(&user_rows(&[]), "User", "ODFEq"));
    assert_ne!(stamp_rows_named(&elsewhere, "Scratch", "ODFEq"), stamp_rows_named(&elsewhere, "User", "ODFEq"));
    assert_ne!(stamp_rows_named(&Sheet::default(), "User", "A"), stamp_rows_named(&Sheet::default(), "Use", "rA"));
    assert_ne!(stamp_rows_named(&Sheet::default(), "User", "A"), stamp_rows_named(&Sheet::default(), "User", "B"));
    // The number of the rows is part of it: one row, and two rows whose bytes begin the same way.
    assert_ne!(stamp_rows_named(&user_rows(&[("E1", "1")]), "User", "E"), stamp_rows_named(&user_rows(&[("E1", "1"), ("E1", "1")]), "User", "E"));
}

#[test]
fn stamp_rows_named_counts_every_row_it_looks_at() {
    // 100,000 rows of a `User` section, 3 of them equations: each row is looked at once and the cells of the three are fed; the other section is one look.
    let mut rows: Vec<Row> = Vec::new();
    for k in 0..100_000_u32 {
        let name = if k % 40_000 == 0 { format!("ODFEq{k}") } else { format!("Mine{k}") };
        let mut row = Row { name: Some(text(&name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), constant("1"));
        rows.push(row);
    }
    let mut sheet = sheet_of(vec![section_of("Scratch", None, &[]), section_of("User", None, &rows)]);
    work::reset();
    let _ = stamp_rows_named(&sheet, "User", "ODFEq");
    // Two sections, 100,000 rows looked at, and the one cell of each of the three rows fed.
    assert_eq!(work::stamp_items(), 2 + 100_000 + 3);
    // Rows that match nothing cost their look and no more.
    sheet.sections.remove(0);
    work::reset();
    let _ = stamp_rows_named(&sheet, "User", "Nothing");
    assert_eq!(work::stamp_items(), 1 + 100_000);
    // And a sheet with 10,000 sections of that name's neighbours costs one look at each.
    let many = sheet_of((0..10_000_u32).map(|i| section_of("Scratch", Some(i), &[])).collect());
    work::reset();
    let _ = stamp_rows_named(&many, "User", "ODFEq");
    assert_eq!(work::stamp_items(), 10_000);
}

// ---------------------------------------------------------------------------------------------------------------- the timed yardstick

/// The crafted cases, each timed beside the same check of a million plain shapes: none may cost more than it. Run alone:
/// `cargo test -p diacraft-doc --test it timed -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "timing: a million shapes; run alone with --test-threads=1"]
fn timed_crafted_inputs_cost_no_more_than_a_million_plain_shapes() {
    use std::time::Instant;
    let sheet = Arc::new(Sheet::default());
    let plain = Arc::new(Shape { sheet: Arc::clone(&sheet), ..Shape::new(1) });
    let mut page = Page::new(1, "p");
    page.shapes = std::iter::repeat_n(Arc::clone(&plain), 1_000_000).collect();
    let mut yardstick = Document::new_blank();
    yardstick.pages = std::iter::once(Arc::new(page)).collect();
    let start = Instant::now();
    assert_eq!(yardstick.check_limits(), Ok(()));
    let yard = start.elapsed();
    eprintln!("yardstick, a million plain shapes: {yard:?}");

    let element = an_element();
    let mut cases: Vec<(&str, Document)> = Vec::new();
    let mut parts = Document::new_blank();
    parts.parts = std::iter::repeat_n(a_part("/a.bin"), MAX_PARTS).collect();
    cases.push(("50,000 parts of nothing", parts));
    let all: Vec<Definition> = FAMILIES.iter().flat_map(|(f, _)| (0..MAX_DEFINITIONS).map(|_| a_definition(*f, &element))).collect();
    cases.push(("65,536 definitions in each of six families", doc_with_definitions(all)));
    let hostile: Vec<Definition> = (0..10 * MAX_DEFINITIONS).map(|_| a_definition(DefFamily::Marker, &element)).collect();
    cases.push(("655,360 markers (refused at the 65,537th)", doc_with_definitions(hostile)));
    let held = Held { group: HeldGroup::Fill, stamp: Some(Stamp(7)), attrs: RawAttrs::new(), kept: Passthrough::new() };
    let mut shape = Shape::new(1);
    shape.ext.make_mut().odf.held = std::iter::repeat_n(held, 100_000).collect();
    let mut page = Page::new(1, "p");
    page.shapes.push(Arc::new(shape));
    let mut held_doc = Document::new_blank();
    held_doc.pages = std::iter::once(Arc::new(page)).collect();
    cases.push(("a shape of 100,000 held items", held_doc));
    for (what, doc) in &cases {
        let start = Instant::now();
        let _ = doc.check_limits();
        let took = start.elapsed();
        eprintln!("{what}: {took:?}");
        assert!(took <= yard, "{what} took {took:?}, the yardstick {yard:?}");
    }

    // The stamp has a yardstick of its own: 200,000 names, all of them cells, in order and each once (plain content of the same size). The crafted input is
    // 100,000 cells named in the reverse order and each twice, which a stamp that sorted and looked up badly would pay for.
    let cells_named = |n: usize| {
        let mut sheet = Sheet::default();
        let names: Vec<String> = (0..n).map(|i| format!("C{i:06}")).collect();
        for name in &names {
            sheet.cells.insert(CellName::new(name), constant("1"));
        }
        (sheet, names)
    };
    let (plain_sheet, plain_names) = cells_named(200_000);
    let plain: Vec<&str> = plain_names.iter().map(String::as_str).collect();
    let start = Instant::now();
    let _ = stamp_cells(&plain_sheet, &plain);
    let stamp_yard = start.elapsed();
    let (sheet, names) = cells_named(100_000);
    let mut crafted: Vec<&str> = names.iter().rev().map(String::as_str).collect();
    crafted.extend(names.iter().map(String::as_str));
    let start = Instant::now();
    let _ = stamp_cells(&sheet, &crafted);
    let took = start.elapsed();
    eprintln!("stamp_cells, 200,000 names in order (yardstick): {stamp_yard:?}; 100,000 names twice and reversed: {took:?}");
    // The same input size, so about the same cost: twice the yardstick is the allowance for the jitter of two runs, and a pass over the sheet for each
    // name would be a thousand times it.
    assert!(took <= stamp_yard * 2, "stamp_cells took {took:?}, the yardstick {stamp_yard:?}");
}
