//! What needs of the codec ('s gaps): a prefix for what the patcher makes, a prefix that leaves kept unknown nodes alone, and a
//! delta for sections and rows.

use diacraft_container::xml::{El, write_element};
use diacraft_doc::{Cell, CellName, RowKey, SectionKey, Sheet, Value};
use diacraft_sheetxml::build::{prefix_row, prefix_section, sheet_only_el, with_prefix, with_prefix_sheet};
use diacraft_sheetxml::patch::{patch_sheet_children, patch_sheet_children_prefixed, patch_sheet_only, patch_sheet_only_prefixed};
use diacraft_sheetxml::{PatchError, Scope, sheet_delta};

use super::{BIG, EXT, Ext, Main, parse, read_el, read_with};

fn text_of(el: &El) -> String {
    let mut out = Vec::new();
    write_element(&mut out, el).unwrap();
    String::from_utf8(out).unwrap()
}

/// A sheet with every kind of kept unknown node the codec puts back: an unknown element among the cells and sections, one in no namespace that
/// is called like a codec element but has no name, and a child inside a cell.
const WITH_UNKNOWN: &str = concat!(
    r#"<PageSheet>"#,
    r#"<Cell N="A" V="1"><RefBy ID="4"><Cell N="inner" V="7"/></RefBy></Cell>"#,
    r#"<Section N="User"><Row N="r"><Cell N="Value" V="x"/><Keep K="1"><Cell N="deep"/></Keep></Row><Row IX="3"/></Section>"#,
    r#"<Trigger N="t"/><Cell V="no name"/><x:foreign xmlns:x="urn:z"><Section N="in"/></x:foreign>"#,
    r#"</PageSheet>"#
);

#[test]
fn a_prefix_for_a_sheet_goes_on_the_codecs_elements_and_leaves_every_kept_unknown_node_as_it_was() {
    let sheet = read_with::<Main>(WITH_UNKNOWN, BIG).unwrap().sheet;
    let mut el = sheet_only_el("Sheet", &sheet);
    with_prefix_sheet(&mut el, "e");
    let text = text_of(&el);
    assert!(text.starts_with("<e:Sheet>"), "{text}");
    assert!(text.contains(r#"<e:Cell N="A" V="1">"#) && text.contains(r#"<e:Section N="User">"#), "{text}");
    assert!(text.contains(r#"<e:Row N="r">"#) && text.contains(r#"<e:Row IX="3""#), "the rows keyed by name and by index: {text}");
    assert!(text.contains(r#"<e:Cell N="Value" V="x"/>"#), "{text}");
    // Kept unknown nodes: no prefix on them, and none on what is inside them, a `Cell` with a name included.
    for kept in [
        r#"<RefBy ID="4"><Cell N="inner" V="7"/></RefBy>"#,
        r#"<Keep K="1"><Cell N="deep"/></Keep>"#,
        r#"<Trigger N="t"/>"#,
        r#"<Cell V="no name"/>"#,
    ] {
        assert!(text.contains(kept), "{kept} in {text}");
    }
    assert!(text.contains(r#"<x:foreign xmlns:x="urn:z"><Section N="in"/></x:foreign>"#), "{text}");
    // The whole prefix reads back as the same sheet under a vocabulary that reads that namespace and no other.
    el.attrs.push(("xmlns:e".to_owned(), EXT.to_owned()));
    let mut back = read_el::<Ext>(&parse(&text_of(&el)), BIG).unwrap().sheet;
    // (The declaration the test added is the sheet element's own attribute.)
    assert_eq!(back.raw.iter().map(|(k, _)| k.to_owned()).collect::<Vec<_>>(), ["xmlns:e"]);
    back.raw = diacraft_doc::RawAttrs::new();
    assert_eq!(back, sheet, "the prefix did not change what a reader finds");
    // An empty prefix changes nothing.
    let mut same = sheet_only_el("Sheet", &sheet);
    let before = same.clone();
    with_prefix_sheet(&mut same, "");
    assert_eq!(same, before);
}

#[test]
fn the_whole_tree_prefix_is_for_a_tree_the_caller_made_and_prefixes_what_the_sheet_one_leaves() {
    // `with_prefix` is unchanged: it prefixes every unprefixed element, which a shape or a page (not a sheet) needs.
    let mut el = parse(r#"<Shape><Trigger N="t"/><Cell N="A"/></Shape>"#);
    with_prefix(&mut el, "e");
    assert_eq!(text_of(&el), r#"<e:Shape><e:Trigger N="t"/><e:Cell N="A"/></e:Shape>"#);
    let mut sheet_only = parse(r#"<Shape><Trigger N="t"/><Cell N="A"/></Shape>"#);
    with_prefix_sheet(&mut sheet_only, "e");
    assert_eq!(text_of(&sheet_only), r#"<e:Shape><Trigger N="t"/><e:Cell N="A"/></e:Shape>"#);
}

fn number(n: f64) -> Cell {
    Cell::constant(Value::from_number(n).unwrap())
}

/// `xml` (in the extension namespace, prefix `e`) patched from the model it reads as to what `edit` makes of it, through the prefixed patcher.
fn patched_prefixed(xml: &str, edit: impl FnOnce(&mut Sheet), prefix: &str) -> (String, Sheet, Sheet) {
    let el = parse(xml);
    let base = read_el::<Ext>(&el, BIG).unwrap().sheet;
    let mut now = base.clone();
    edit(&mut now);
    let mut el = el;
    let sc = Scope::<Ext>::empty().enter(&el);
    patch_sheet_children_prefixed::<Ext, PatchError>(&mut el, &sc, &base, &now, prefix).unwrap();
    (text_of(&el), base, now)
}

const EXT_TREE: &str = concat!(
    r#"<e:Sheet xmlns:e="urn:test:ext"><e:Cell N="A" V="1"/><e:Section N="User"><e:Row N="r"><e:Cell N="Value" V="x"/></e:Row></e:Section>"#,
    r#"<Trigger N="t"/></e:Sheet>"#
);

#[test]
fn what_the_patcher_makes_carries_the_prefix_it_is_given_and_reads_back_in_a_vocabulary_of_one_namespace() {
    let (text, _, now) = patched_prefixed(
        EXT_TREE,
        |s| {
            s.cells.insert(CellName::new("B"), number(2.0));
            let row = s.row_mut(&SectionKey::new("Prop", None), &RowKey::name("new")).unwrap();
            row.cells.insert(CellName::new("Value"), number(5.0));
        },
        "e",
    );
    // The new cell, the new section, its row and the row's cell: each prefixed. The kept unknown `Trigger` is left alone.
    assert!(text.contains(r#"<e:Cell N="B" V="2"/>"#), "{text}");
    assert!(text.contains(r#"<e:Section N="Prop">"#) && text.contains(r#"<e:Row N="new">"#), "{text}");
    assert!(text.contains(r#"<e:Cell N="Value" V="5"/>"#), "{text}");
    assert!(text.contains(r#"<Trigger N="t"/>"#), "{text}");
    assert!(!text.contains("<Cell N=\"B\"") && !text.contains("<Section N=\"Prop\""), "nothing bare: {text}");
    let back = read_el::<Ext>(&parse(&text), BIG).unwrap().sheet;
    assert_eq!(back.cells, now.cells, "what the patcher made is read back");
    assert_eq!(back.sections, now.sections);
    // The kept node stays among the elements, and the number of typed children it follows is the new one.
    let kept = |s: &Sheet| s.passthrough.iter().map(|p| p.node.clone()).collect::<Vec<_>>();
    assert_eq!(kept(&back), kept(&now));
}

#[test]
fn the_unprefixed_patcher_is_as_it_was_and_an_empty_prefix_is_the_same_as_none() {
    let edit = |s: &mut Sheet| {
        s.cells.insert(CellName::new("B"), number(2.0));
    };
    let bare = {
        let el = parse(EXT_TREE);
        let base = read_el::<Ext>(&el, BIG).unwrap().sheet;
        let mut now = base.clone();
        edit(&mut now);
        let mut el = el;
        let sc = Scope::<Ext>::empty().enter(&el);
        patch_sheet_children::<Ext, PatchError>(&mut el, &sc, &base, &now).unwrap();
        text_of(&el)
    };
    assert!(bare.contains(r#"<Cell N="B" V="2"/>"#), "no prefix: {bare}");
    assert_eq!(patched_prefixed(EXT_TREE, edit, "").0, bare);
}

#[test]
fn a_sheet_element_the_patcher_makes_is_prefixed_with_what_is_inside_it_and_a_kept_node_in_the_sheet_is_not() {
    let base = Sheet::default();
    let mut now = Sheet::default();
    now.cells.insert(CellName::new("A"), number(1.0));
    now.row_mut(&SectionKey::new("User", None), &RowKey::name("r")).unwrap().cells.insert(CellName::new("Value"), number(3.0));
    let expect = r#"<e:PageSheet><e:Cell N="A" V="1"/><e:Section N="User"><e:Row N="r"><e:Cell N="Value" V="3"/></e:Row></e:Section></e:PageSheet>"#;
    let mut parent = parse(r#"<e:Doc xmlns:e="urn:test:ext"><e:Tail/></e:Doc>"#);
    let sc = Scope::<Ext>::empty().enter(&parent);
    patch_sheet_only_prefixed::<Ext, PatchError>(&mut parent, &sc, "PageSheet", &["Tail"], &base, &now, "e").unwrap();
    let text = text_of(&parent);
    assert!(text.contains(expect), "{text}");
    // It goes before the element it is asked to go before, and reads back.
    assert!(text.find("<e:PageSheet>").unwrap() < text.find("<e:Tail/>").unwrap(), "{text}");
    // Unprefixed, the same call makes bare names, as before.
    let mut parent = parse(r#"<Doc><Tail/></Doc>"#);
    let sc = Scope::<Main>::empty().enter(&parent);
    patch_sheet_only::<Main, PatchError>(&mut parent, &sc, "PageSheet", &["Tail"], &base, &now).unwrap();
    assert!(
        text_of(&parent)
            .contains(r#"<PageSheet><Cell N="A" V="1"/><Section N="User"><Row N="r"><Cell N="Value" V="3"/></Row></Section></PageSheet>"#)
    );
}

// ------------------------------------------------------------------------------------------------------------------ the delta of sections and rows

fn sheet(xml: &str) -> Sheet {
    read_with::<Main>(xml, BIG).unwrap().sheet
}

const BASE: &str = concat!(
    r#"<PageSheet><Cell N="A" V="1"/>"#,
    r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/></Row></Section>"#,
    r#"<Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row><Row N="b"><Cell N="Value" V="2"/></Row></Section>"#,
    r#"<Section N="Prop"><Row N="p"><Cell N="Value" V="3"/></Row></Section>"#,
    r#"</PageSheet>"#
);

#[test]
fn a_sheet_that_holds_what_the_reader_would_rebuild_has_an_empty_delta() {
    let s = sheet(BASE);
    let d = sheet_delta(&s, &s.clone());
    assert!(d.is_empty() && d.sections.is_empty() && d.removed_sections.is_empty());
    assert!(sheet_delta(&Sheet::default(), &Sheet::default()).is_empty());
}

#[test]
fn a_section_the_reader_would_not_have_is_new_and_one_it_would_and_the_model_lacks_is_said_gone() {
    let defaults = sheet(BASE);
    let now = sheet(
        r#"<PageSheet><Cell N="A" V="1"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row><Row N="b"><Cell N="Value" V="2"/></Row></Section><Section N="Hyperlink"><Row N="h"><Cell N="Address" V="x"/></Row></Section></PageSheet>"#,
    );
    let d = sheet_delta(&now, &defaults);
    assert_eq!(d.sections.len(), 1);
    assert!(d.sections[0].new && d.sections[0].section.name.as_str() == "Hyperlink");
    assert_eq!(d.removed_sections, [SectionKey::new("Prop", None)]);
    assert!(!d.is_empty());
}

#[test]
fn a_section_that_differs_names_the_rows_that_differ_and_the_rows_that_are_gone() {
    let defaults = sheet(BASE);
    // `User` row `b` changed, row `c` new, row `a` gone; `Geometry` row 2 changed and its flag; `Prop` as it was.
    let now = sheet(concat!(
        r#"<PageSheet><Cell N="A" V="1"/>"#,
        r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="9"/></Row></Section>"#,
        r#"<Section N="User"><Row N="b"><Cell N="Value" V="20"/></Row><Row N="c"><Cell N="Value" V="4"/></Row></Section>"#,
        r#"<Section N="Prop"><Row N="p"><Cell N="Value" V="3"/></Row></Section>"#,
        r#"</PageSheet>"#
    ));
    let d = sheet_delta(&now, &defaults);
    assert!(d.cells.is_empty() && d.removed_sections.is_empty());
    assert_eq!(d.sections.len(), 2, "Prop is the same and is not among them");
    let geometry = &d.sections[0];
    assert!(!geometry.new && geometry.cells.changed.len() == 1, "the flag differs");
    assert_eq!(geometry.rows.len(), 1, "row 2 only");
    assert_eq!(geometry.rows[0].ix, Some(2));
    assert!(geometry.removed_rows.is_empty() && !geometry.attributes && !geometry.reordered);
    let user = &d.sections[1];
    assert_eq!(user.rows.iter().map(|r| r.name.as_deref().unwrap()).collect::<Vec<_>>(), ["b", "c"]);
    assert_eq!(user.removed_rows, [RowKey::name("a")]);
    assert!(!user.is_empty());
}

#[test]
fn rows_both_hold_in_another_order_and_a_section_with_other_attributes_are_told() {
    let defaults = sheet(BASE);
    let now = sheet(
        r#"<PageSheet><Cell N="A" V="1"/><Section N="User" Odd="1"><Row N="b"><Cell N="Value" V="2"/></Row><Row N="a"><Cell N="Value" V="1"/></Row></Section></PageSheet>"#,
    );
    let d = sheet_delta(&now, &defaults);
    let user = d.sections.iter().find(|s| s.section.name.as_str() == "User").unwrap();
    assert!(user.reordered, "the same rows in another order");
    assert!(user.attributes, "an attribute the model keeps");
    assert!(user.rows.is_empty(), "no row differs by itself");
    assert_eq!(d.removed_sections.len(), 2, "Geometry and Prop");
}

/// A section that differs from the one the reader would rebuild in its `Del` flag alone (its rows, cells, order and other attributes the same) is told by
/// `attributes`: each of the four parts of that test counts by itself (the mutation run of found `del` with no test of its own).
#[test]
fn a_section_that_differs_in_its_del_flag_alone_is_told_by_its_attributes() {
    let defaults = sheet(BASE);
    let now = sheet(
        r#"<PageSheet><Cell N="A" V="1"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/></Row></Section><Section N="User" Del="1"><Row N="a"><Cell N="Value" V="1"/></Row><Row N="b"><Cell N="Value" V="2"/></Row></Section><Section N="Prop"><Row N="p"><Cell N="Value" V="3"/></Row></Section></PageSheet>"#,
    );
    let d = sheet_delta(&now, &defaults);
    assert_eq!(d.sections.len(), 1, "only User differs");
    let user = &d.sections[0];
    assert_eq!(user.section.name.as_str(), "User");
    assert!(user.attributes, "the flag is an attribute of the section the model keeps");
    assert!(user.rows.is_empty() && user.removed_rows.is_empty() && !user.reordered && user.cells.is_empty());
    assert!(!d.is_empty());
}

#[test]
fn two_sections_of_one_key_are_compared_in_order_and_a_row_with_no_key_is_always_written() {
    let defaults = sheet(
        r#"<PageSheet><Section N="User"><Row N="a"><Cell N="V" V="1"/></Row></Section><Section N="User"><Row N="b"><Cell N="V" V="2"/></Row></Section></PageSheet>"#,
    );
    assert!(sheet_delta(&defaults.clone(), &defaults).is_empty());
    // The second changes; the first does not.
    let mut changed = defaults.clone();
    {
        let section = std::sync::Arc::make_mut(&mut changed.sections[1]);
        let mut row = (*section.rows[0]).clone();
        row.cells.insert(CellName::new("V"), number(99.0));
        section.rows = diacraft_doc::Rows::new();
        section.rows.push(std::sync::Arc::new(row));
    }
    let d = sheet_delta(&changed, &defaults);
    assert_eq!(d.sections.len(), 1);
    assert_eq!(d.sections[0].section.rows[0].name.as_deref(), Some("b"));
    // A row that has neither a name nor an index cannot be found again, so it is written.
    let mut keyless = defaults.clone();
    {
        let section = std::sync::Arc::make_mut(&mut keyless.sections[0]);
        section.rows.push(std::sync::Arc::new(diacraft_doc::Row::default()));
    }
    let d = sheet_delta(&keyless, &defaults);
    assert_eq!(d.sections.len(), 1);
    assert_eq!(d.sections[0].rows.len(), 1);
}

/// The bytes a kept element costs (the review, MEDIUM): every name, attribute, text, comment and instruction of it, all the way down, and the walk is
/// no recursion.
#[test]
fn the_bytes_of_an_element_are_those_of_every_name_attribute_and_text_in_it() {
    use diacraft_container::xml::Node;
    use diacraft_sheetxml::passthrough::{element_bytes, element_cost};
    let el = parse(r#"<a k="vv"><b q="1">text<!--cc--><?t dd?><c/></b>tail<![CDATA[xyz]]></a>"#);
    // Names: a, b, c (3). Attributes: k, vv, q, 1 (5). Text: text, tail, xyz (11). Comment: cc (2). Instruction: t, dd (3).
    assert_eq!(element_bytes(&el), 3 + 5 + 11 + 2 + 3);
    assert!(element_cost(&el) >= 3);
    // A tree far deeper than the stack the walk is given could hold if it recursed, with a megabyte of text at the bottom.
    let mut inner = El { name: "z".to_owned(), attrs: Vec::new(), children: vec![Node::Text("x".repeat(1_000_000))] };
    for _ in 0..100_000 {
        inner = El { name: "z".to_owned(), attrs: Vec::new(), children: vec![Node::El(inner)] };
    }
    let handle = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || {
        let n = element_bytes(&inner);
        // A chain this deep has no safe drop (its drop would recurse), so it is leaked.
        std::mem::forget(inner);
        n
    });
    assert_eq!(handle.unwrap().join().unwrap(), 100_001 + 1_000_000);
}

/// What the mutants of the prefix left standing: a section's own cells, rows by `N` and by `IX` are the codec's, and a row with neither, an element that is not
/// called `Row`, one that has an `N` and one that has a prefix already are not, whatever they are in; and a row of its own, prefixed, takes its cells.
#[test]
fn a_section_prefixes_its_cells_and_rows_and_a_row_its_cells_and_no_other_node_is() {
    let mut el = parse(
        r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Cell V="no name"/><Row N="a"><Cell N="X" V="1"/><Cell V="no name"/></Row><Row IX="2"/><Row/><Row N=""/><Foreign/><Other N="o"/><x:Row xmlns:x="urn:z" N="p"/></Section>"#,
    );
    prefix_section(&mut el, "e");
    assert_eq!(
        text_of(&el),
        r#"<e:Section N="Geometry" IX="0"><e:Cell N="NoFill" V="0"/><Cell V="no name"/><e:Row N="a"><e:Cell N="X" V="1"/><Cell V="no name"/></e:Row><e:Row IX="2"/><Row/><Row N=""/><Foreign/><Other N="o"/><x:Row xmlns:x="urn:z" N="p"/></e:Section>"#
    );
    let mut row = parse(r#"<Row N="r"><Cell N="Value" V="3"/><Keep/></Row>"#);
    prefix_row(&mut row, "e");
    assert_eq!(text_of(&row), r#"<e:Row N="r"><e:Cell N="Value" V="3"/><Keep/></e:Row>"#);
    // An empty prefix changes nothing.
    let mut same = parse(r#"<Row N="r"><Cell N="Value" V="3"/></Row>"#);
    prefix_row(&mut same, "");
    assert_eq!(text_of(&same), r#"<Row N="r"><Cell N="Value" V="3"/></Row>"#);
}

/// A delta is empty only when every part of it is: the helper is pinned by hand-made deltas, since the reader's sections that differ are never an empty delta.
#[test]
fn a_delta_is_empty_only_when_every_part_of_it_is() {
    use diacraft_doc::{Row, Section};
    use diacraft_sheetxml::CellDelta;
    use diacraft_sheetxml::delta::{SectionDelta, SheetDelta};
    let section = Section::new("User", None);
    let row = Row::default();
    let held = || SectionDelta {
        section: &section,
        new: false,
        cells: CellDelta::default(),
        attributes: false,
        rows: Vec::new(),
        removed_rows: Vec::new(),
        reordered: false,
    };
    assert!(held().is_empty());
    type Part = Box<dyn Fn(&mut SectionDelta<'_>)>;
    let parts: Vec<(&str, Part)> = vec![
        ("new", Box::new(|d| d.new = true)),
        ("attributes", Box::new(|d| d.attributes = true)),
        ("reordered", Box::new(|d| d.reordered = true)),
        ("removed rows", Box::new(|d| d.removed_rows.push(RowKey::name("a")))),
        ("cells", Box::new(|d| d.cells.removed.push(CellName::new("A")))),
    ];
    for (what, set) in &parts {
        let mut d = held();
        set(&mut d);
        assert!(!d.is_empty(), "a section delta with {what} is not empty");
    }
    let mut with_row = held();
    with_row.rows.push(&row);
    assert!(!with_row.is_empty(), "a section delta with a row is not empty");
    let sheet = || SheetDelta { cells: CellDelta::default(), sections: Vec::new(), removed_sections: Vec::new() };
    assert!(sheet().is_empty());
    let mut cells = sheet();
    cells.cells.removed.push(CellName::new("A"));
    let mut sections = sheet();
    sections.sections.push(held());
    let mut removed = sheet();
    removed.removed_sections.push(SectionKey::new("User", None));
    for (what, d) in [("cells", cells), ("sections", sections), ("removed sections", removed)] {
        assert!(!d.is_empty(), "a sheet delta with {what} alone is not empty");
    }
}

/// Rows that both hold in another order are a section better written whole: the section's delta says so though no row differs (and says nothing of two rows in
/// the order they were).
#[test]
fn rows_both_hold_in_another_order_make_the_section_reordered_and_in_the_same_order_do_not() {
    let rows = |names: [&str; 2]| {
        let mut sheet = Sheet::default();
        for name in names {
            sheet.row_mut(&SectionKey::new("User", None), &RowKey::name(name)).unwrap().cells.insert(CellName::new("Value"), number(1.0));
        }
        sheet
    };
    let defaults = rows(["a", "b"]);
    let swap = rows(["b", "a"]);
    let swapped = sheet_delta(&swap, &defaults);
    assert_eq!(swapped.sections.len(), 1, "the swap is a delta of the section");
    assert!(swapped.sections[0].reordered && swapped.sections[0].rows.is_empty() && swapped.sections[0].removed_rows.is_empty());
    assert!(!swapped.is_empty());
    assert!(sheet_delta(&rows(["a", "b"]), &defaults).is_empty(), "the same order is no delta");
}

/// A sheet delta that holds only one of its parts is not empty, whichever part it is.
#[test]
fn a_sheet_that_differs_in_one_part_only_is_a_delta_that_is_not_empty() {
    let mut defaults = Sheet::default();
    defaults.cells.insert(CellName::new("A"), number(1.0));
    defaults.row_mut(&SectionKey::new("User", None), &RowKey::name("r")).unwrap().cells.insert(CellName::new("Value"), number(1.0));
    // Only a cell differs.
    let mut cell = defaults.clone();
    cell.cells.insert(CellName::new("A"), number(2.0));
    let d = sheet_delta(&cell, &defaults);
    assert!(d.sections.is_empty() && d.removed_sections.is_empty() && !d.is_empty());
    // Only a section differs.
    let mut section = defaults.clone();
    section.row_mut(&SectionKey::new("User", None), &RowKey::name("r")).unwrap().cells.insert(CellName::new("Value"), number(2.0));
    let d = sheet_delta(&section, &defaults);
    assert!(d.cells.is_empty() && d.removed_sections.is_empty() && d.sections.len() == 1 && !d.is_empty());
    // Only a section is gone.
    let mut gone = defaults.clone();
    gone.sections.clear();
    let d = sheet_delta(&gone, &defaults);
    assert!(d.cells.is_empty() && d.sections.is_empty() && d.removed_sections.len() == 1 && !d.is_empty());
}
