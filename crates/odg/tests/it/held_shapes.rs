//! The shapes: what the `.odg` reader holds of each shape beside its cells (the values and their stamps, the automatic style's name, the units, the glue
//! points), written by hand. **The stamps' lists in these tests are the spec's, written out here and not taken from the reader's**, so that a list that
//! drifts from the spec is a failing test.

use diacraft_doc::{
    Cell, CellName, Document, Held, HeldGroup, RowKey, SectionKey, Shape, Sheet, Stamp, stamp_all, stamp_cells, stamp_row, stamp_rows_named,
    stamp_sections,
};

use crate::drawing::Drawing;

/// A cell holding the number `n`.
fn number(n: f64) -> Cell {
    Cell { value: Some(diacraft_doc::Value::from_text(&n.to_string())), ..Cell::new() }
}

/// The shape called `name` of the first page.
fn named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).as_ref()
}

/// A shape's held values.
fn held(shape: &Shape) -> Vec<Held> {
    shape.ext.get().map(|d| d.odf.held.clone()).unwrap_or_default()
}

fn names_of(h: &Held) -> Vec<String> {
    h.attrs.iter().map(|(n, _)| n.to_owned()).collect()
}

/// The held value whose first attribute is `first`.
fn value<'a>(list: &'a [Held], first: &str) -> &'a Held {
    list.iter()
        .find(|h| h.attrs.iter().next().is_some_and(|(n, _)| n == first))
        .unwrap_or_else(|| panic!("no held value starts with {first}: {:?}", list.iter().map(names_of).collect::<Vec<_>>()))
}

/// The definitions the styles below name.
const NAMED: &str = concat!(
    r##"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"##,
    r##"<draw:marker draw:name="DiaCraft_20_end_20_3" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"##,
    r##"<draw:hatch draw:name="H1" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##,
    r##"<draw:opacity draw:name="O1" draw:style="linear" draw:start="100%" draw:end="0%"/>"##,
    r##"<draw:stroke-dash draw:name="D1" draw:style="rect" draw:dots1="1" draw:distance="0.2cm"/>"##,
);

/// A graphic style that gives a shape one of every value of the table that its own automatic style can give.
const GR1: &str = concat!(
    r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard">"##,
    r##"<style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H1" draw:fill-hatch-solid="true" draw:fill-color="#ff0000" draw:opacity-name="O1" "##,
    r##"draw:stroke="dash" draw:stroke-dash="D1" draw:stroke-linejoin="round" svg:stroke-linecap="round" svg:stroke-width="0.1cm" "##,
    r##"draw:marker-start="Arrow" draw:marker-start-width="0.3cm" draw:marker-start-center="true" "##,
    r##"draw:marker-end="DiaCraft_20_end_20_3" draw:marker-end-width="0.5cm" "##,
    r##"draw:shadow="visible" draw:shadow-offset-x="0.1cm" draw:shadow-offset-y="0.1cm" draw:shadow-color="#808080" draw:shadow-blur="0.05cm" "##,
    r##"draw:textarea-vertical-align="justify" fo:padding="0.1cm" style:writing-mode="tb-rl"/>"##,
    r##"<style:paragraph-properties style:tab-stop-distance="1cm"><style:tab-stops><style:tab-stop style:position="2cm" style:leader-style="dotted"/></style:tab-stops></style:paragraph-properties>"##,
    r##"</style:style>"##,
);

const CUSTOM: &str = concat!(
    r##"<draw:custom-shape draw:name="C" draw:style-name="gr1" draw:layer="layout" draw:transform="rotate (0.5) translate (3cm 4cm)" svg:width="2cm" svg:height="1cm">"##,
    r##"<text:p>hi</text:p>"##,
    r##"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/>"##,
    r##"<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0.5cm" draw:escape-direction="horizontal"/>"##,
    r##"<draw:glue-point draw:id="5" svg:x="50%" svg:y="-50%" draw:escape-direction="vertical"/>"##,
    r##"<draw:glue-point draw:id="6" svg:x="1cm" svg:y="0.25cm" draw:align="top-left" draw:escape-direction="auto"/>"##,
    r##"<draw:glue-point draw:id="7" svg:x="0cm" svg:y="0cm" draw:align="bottom-right" draw:escape-direction="left"/>"##,
    r##"<draw:glue-point draw:id="8" svg:x="0.3cm" svg:y="0.2cm"/>"##,
    r##"</draw:custom-shape>"##,
);

fn styled() -> Drawing {
    Drawing::a4().common(NAMED).auto(GR1).page(CUSTOM)
}

/// **A held value is used while its stamp matches** (the plan's test, a hatch fill): the shape's own automatic style says `draw:fill="hatch"`, and the value holds the attributes as
/// written and the stamp of the cells R22 lists for it, as the reader set them; the cells' stamp now is that stamp until a cell of the list is edited, and an undo that puts the
/// cell back makes it good again. A cell outside the list (the shape's line colour) is no edit of it.
#[test]
fn a_held_value_is_used_while_its_stamp_matches() {
    let (doc, _) = styled().read();
    let shape = named(&doc, "C");
    let list = held(shape);
    let fill = value(&list, "draw:fill");
    assert_eq!(fill.group, HeldGroup::Fill);
    assert_eq!(names_of(fill), ["draw:fill", "draw:fill-hatch-name", "draw:fill-hatch-solid"]);
    assert_eq!(fill.attrs.get("draw:fill"), Some("hatch"));
    assert_eq!(fill.attrs.get("draw:fill-hatch-name"), Some("H1"));
    // The list for a hatch: FillPattern, FillGradientEnabled, FillGradientDir, the rows of FillGradient, and FillForegnd.
    let good = |sheet: &Sheet| {
        Some(stamp_all(&[
            stamp_cells(sheet, &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillForegnd"]),
            stamp_sections(sheet, &["FillGradient"]),
        ]))
    };
    assert_eq!(fill.stamp, good(&shape.sheet), "the stamp of the cells as the reader set them");
    assert_ne!(good(&Sheet::default()), fill.stamp, "a stamp that is not of nothing");
    // An edit of FillForegnd (the colour of a hatch) makes the value stale, and putting the cell back makes it good again.
    let mut sheet = (*shape.sheet).clone();
    let before = sheet.cells.get("FillForegnd").cloned().expect("the hatch's colour");
    sheet.cells.insert(CellName::new("FillForegnd"), Cell::new());
    assert_ne!(good(&sheet), fill.stamp, "an edit of a cell of the list");
    sheet.cells.insert(CellName::new("FillForegnd"), before);
    assert_eq!(good(&sheet), fill.stamp, "an undo makes it good again");
    // A cell outside the list: the line's colour.
    sheet.cells.insert(CellName::new("LineColor"), Cell::new());
    assert_eq!(good(&sheet), fill.stamp, "the line's colour is no part of the fill's keeping");
}

/// The list of cells for each stamped value, by the value's first attribute: the spec's table written out.
fn spec_cells(first: &str) -> &'static [&'static str] {
    match first {
        "draw:opacity-name" => &["FillForegndTrans"],
        "draw:stroke-linejoin" => &["LineCap"],
        "draw:marker-start" => &["BeginArrow", "BeginArrowSize"],
        "draw:marker-end-width" => &["EndArrow", "EndArrowSize", "LineWeight"],
        // Every cell the reader sets from the shadow's attributes.
        "draw:shadow" => &["ShdwPattern", "ShapeShdwType", "ShdwForegnd", "ShdwForegndTrans", "ShapeShdwOffsetX", "ShapeShdwOffsetY"],
        "draw:textarea-vertical-align" => &["VerticalAlign"],
        "fo:padding" => &["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"],
        "style:writing-mode" => &["TextDirection"],
        other => panic!("{other} is not a stamped value of the cells alone"),
    }
}

/// **An undo makes a held value good again**, for each stamped value of one shape: stale after the edit of a cell it rests on, good after the cell is put back; and a value that
/// is not stamped is never stale.
#[test]
fn an_undo_makes_a_held_value_good_again() {
    let (doc, _) = styled().read();
    let shape = named(&doc, "C");
    let list = held(shape);
    for first in [
        "draw:opacity-name",
        "draw:stroke-linejoin",
        "draw:marker-start",
        "draw:marker-end-width",
        "draw:shadow",
        "draw:textarea-vertical-align",
        "fo:padding",
        "style:writing-mode",
    ] {
        let h = value(&list, first);
        assert_eq!(h.stamp, Some(stamp_cells(&shape.sheet, spec_cells(first))), "{first}: as read");
        for name in spec_cells(first).iter().copied() {
            let mut sheet = (*shape.sheet).clone();
            let original = sheet.cells.get(name).cloned();
            sheet.cells.insert(CellName::new(name), number(99.0));
            assert_ne!(h.stamp, Some(stamp_cells(&sheet, spec_cells(first))), "{first}: stale after an edit of {name}");
            match original {
                Some(original) => {
                    sheet.cells.insert(CellName::new(name), original);
                }
                None => {
                    sheet.cells.remove(name);
                }
            }
            assert_eq!(h.stamp, Some(stamp_cells(&sheet, spec_cells(first))), "{first}: good again after the undo of {name}");
        }
    }
    // A value that is not stamped: a dash, the tab stops, the layer and the frame (written as held on every save, or judged by the pose on the page).
    for first in ["draw:stroke-dash", "style:tab-stop-distance", "draw:layer", "draw:transform"] {
        assert_eq!(value(&list, first).stamp, None, "{first}");
    }
}

/// **`held_groups_match_the_ownership_rules`** (against): each value of the shape is in its group, holds the attributes R22 names for it, as
/// written, and is stamped by the cells, sections and rows R22 lists for that value, never by a group's list: a cell outside the value's list leaves the stamp as it was.
#[test]
fn held_groups_match_the_ownership_rules() {
    let (doc, _) = styled().read();
    let shape = named(&doc, "C");
    let list = held(shape);
    // The group of each value, the attributes it holds (as written, in order), and the cell lists of the table.
    let table: [(&str, HeldGroup, &[&str]); 13] = [
        ("draw:fill", HeldGroup::Fill, &["draw:fill", "draw:fill-hatch-name", "draw:fill-hatch-solid"]),
        ("draw:opacity-name", HeldGroup::Fill, &["draw:opacity-name"]),
        ("draw:stroke-dash", HeldGroup::Line, &["draw:stroke-dash"]),
        ("draw:stroke-linejoin", HeldGroup::Line, &["draw:stroke-linejoin"]),
        ("draw:marker-start", HeldGroup::LineEnds, &["draw:marker-start", "draw:marker-start-width", "draw:marker-start-center"]),
        ("draw:marker-end-width", HeldGroup::LineEnds, &["draw:marker-end-width"]),
        ("draw:shadow", HeldGroup::Shadow, &["draw:shadow", "draw:shadow-offset-x", "draw:shadow-offset-y", "draw:shadow-color", "draw:shadow-blur"]),
        ("draw:textarea-vertical-align", HeldGroup::Text, &["draw:textarea-vertical-align"]),
        ("fo:padding", HeldGroup::Text, &["fo:padding"]),
        ("style:writing-mode", HeldGroup::Text, &["style:writing-mode"]),
        ("style:tab-stop-distance", HeldGroup::Text, &["style:tab-stop-distance"]),
        ("draw:transform", HeldGroup::Frame, &["draw:transform", "svg:width", "svg:height"]),
        ("draw:layer", HeldGroup::Frame, &["draw:layer"]),
    ];
    for (first, group, attrs) in &table {
        let h = value(&list, first);
        assert_eq!(h.group, *group, "{first}");
        assert_eq!(names_of(h), *attrs, "{first}");
    }
    // The geometry's element is held in the shape's kept markup, tied to a stamp of the `Geometry` sections and the `ODFEq` rows of `User`.
    let geometry = list.iter().find(|h| h.group == HeldGroup::Geometry).expect("the geometry's value");
    assert!(geometry.attrs.is_empty() && geometry.kept.is_empty(), "nothing but the stamp");
    let want = |sheet: &Sheet| Some(stamp_all(&[stamp_sections(sheet, &["Geometry"]), stamp_rows_named(sheet, "User", "ODFEq")]));
    assert_eq!(geometry.stamp, want(&shape.sheet));
    let kept = &shape.ext.get().expect("slot").odf.kept.kept;
    assert!(
        kept.iter().any(|p| matches!(&p.node, diacraft_doc::RawNode::Element(e) if &*e.name == "draw:enhanced-geometry")),
        "the element stays in the kept markup"
    );
    // The tab stops: the element whole, with its leader attribute.
    let tabs = value(&list, "style:tab-stop-distance");
    assert_eq!(tabs.kept.len(), 1);
    // A cell outside a value's list leaves its stamp as it was: the line weight is in no fill's, shadow's or text's list.
    let mut sheet = (*shape.sheet).clone();
    sheet.cells.insert(CellName::new("LineWeight"), number(0.5));
    for first in ["draw:opacity-name", "draw:stroke-linejoin", "draw:marker-start", "draw:shadow", "fo:padding"] {
        assert_eq!(value(&list, first).stamp, Some(stamp_cells(&sheet, spec_cells(first))), "{first} is not stamped by LineWeight");
    }
    // `LineWeight` is in the list of the width of DiaCraft's own marker, and in no other line end's.
    assert_ne!(
        value(&list, "draw:marker-end-width").stamp,
        Some(stamp_cells(&sheet, spec_cells("draw:marker-end-width"))),
        "a weight edit makes the own marker's width stale"
    );
    assert_eq!(
        value(&list, "draw:marker-start").stamp,
        Some(stamp_cells(&sheet, spec_cells("draw:marker-start"))),
        "and leaves a foreign marker's width right"
    );
}

/// **`units_come_back`**: a length is held in the unit it was written in: the cell's own unit where the cell takes the value (`2cm`, `0.5in`, `12pt`), and `units` where no cell
/// spells it (`px`), or the cell has another unit.
#[test]
fn units_come_back() {
    let d = Drawing::a4()
        .common(r##"<style:style style:name="Big" style:family="text"><style:text-properties fo:font-size="12pt"/></style:style>"##)
        .page(concat!(
            r##"<draw:rect draw:name="R" svg:x="0.5in" svg:y="2cm" svg:width="2cm" svg:height="96px"><text:p><text:span text:style-name="Big">x</text:span></text:p></draw:rect>"##,
            r##"<draw:line draw:name="L" svg:x1="1cm" svg:y1="1cm" svg:x2="2in" svg:y2="1cm"/>"##,
        ));
    let (doc, _) = d.read();
    let r = named(&doc, "R");
    let unit = |name: &str| r.sheet.cell(name).and_then(|c| c.unit.clone());
    assert_eq!(unit("Width").as_deref(), Some("CM"), "2cm");
    assert_eq!(unit("PinX").as_deref(), Some("IN"), "0.5in");
    let characters = r.sheet.section(&SectionKey::new("Character", None)).expect("a Character section");
    let size = characters.rows.iter().filter_map(|row| row.cells.get("Size")).find(|c| c.unit.as_deref() == Some("PT"));
    assert!(size.is_some(), "12pt stays 12pt: a size cell with the unit PT");
    let units = r.ext.get().expect("the slot").odf.units.clone();
    assert_eq!(units.get("svg:y"), Some("cm"), "PinY has the unit of svg:x, so the cell does not spell cm");
    assert_eq!(units.get("svg:height"), Some("px"), "a px length has no unit in a cell");
    assert!(units.get("svg:width").is_none() && units.get("svg:x").is_none(), "what a cell spells is not held twice: {units:?}");
    // A line: the unit of an end that its cell does not spell.
    let l = named(&doc, "L");
    let lu = l.ext.get().map(|d| d.odf.units.clone()).unwrap_or_default();
    assert_eq!(l.sheet.cell("EndX").and_then(|c| c.unit.clone()).as_deref(), Some("IN"), "2in");
    assert!(lu.get("svg:x2").is_none(), "{lu:?}");
}

/// **`glue_escape_directions_are_held`**: each `draw:glue-point` element's attributes are held exactly on the `Connection` row it made, stamped by the whole row: the escape
/// directions horizontal, vertical and auto, `draw:align`, and a position as a percentage or as an absolute length.
#[test]
fn glue_escape_directions_are_held() {
    let (doc, _) = styled().read();
    let shape = named(&doc, "C");
    let section = shape.sheet.section(&SectionKey::new("Connection", None)).expect("the Connection section");
    let row = |ix: u32| section.row(&RowKey::Ix(ix)).unwrap_or_else(|| panic!("no row {ix}"));
    let held_of = |ix: u32| row(ix).ext.get().and_then(|d| d.odf.held.clone()).unwrap_or_else(|| panic!("row {ix} holds no glue point"));
    let expected: [(u32, &[(&str, &str)]); 5] = [
        (4, &[("draw:id", "4"), ("svg:x", "0cm"), ("svg:y", "0.5cm"), ("draw:escape-direction", "horizontal")]),
        (5, &[("draw:id", "5"), ("svg:x", "50%"), ("svg:y", "-50%"), ("draw:escape-direction", "vertical")]),
        (6, &[("draw:id", "6"), ("svg:x", "1cm"), ("svg:y", "0.25cm"), ("draw:align", "top-left"), ("draw:escape-direction", "auto")]),
        (7, &[("draw:id", "7"), ("svg:x", "0cm"), ("svg:y", "0cm"), ("draw:align", "bottom-right"), ("draw:escape-direction", "left")]),
        (8, &[("draw:id", "8"), ("svg:x", "0.3cm"), ("svg:y", "0.2cm")]),
    ];
    for (ix, attrs) in expected {
        let h = held_of(ix);
        assert_eq!(h.group, HeldGroup::Glue, "row {ix}");
        let got: Vec<(String, String)> = h.attrs.iter().map(|(n, v)| (n.to_owned(), v.to_owned())).collect();
        let want: Vec<(String, String)> = attrs.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())).collect();
        assert_eq!(got, want, "row {ix}: the attributes exactly, in order");
        assert_eq!(h.stamp, Some(stamp_row(row(ix))), "row {ix}: stamped by the whole row, as the reader set it");
    }
    // The four standard rows hold none, and an edit of a cell of a row makes its value stale.
    assert!(section.row(&RowKey::Ix(0)).is_some_and(|r| r.ext.is_empty()));
    let mut edited = (**section.row(&RowKey::Ix(4)).unwrap()).clone();
    edited.cells.insert(CellName::new("X"), Cell::new());
    assert_ne!(held_of(4).stamp, Some(stamp_row(&edited)));
    let _: Option<Stamp> = None;
}
