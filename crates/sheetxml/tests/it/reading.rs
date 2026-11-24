//! The reader on an element: what it keeps, what it keys by, what it warns of, and what each thing costs.

use std::sync::Arc;

use diacraft_doc::{FormulaSlot, RawNode, RowKey, SectionKey};
use diacraft_sheetxml::LimitKind;

use super::{BIG, Ext, Main, parse, read, read_in, read_with};

#[test]
fn a_cell_is_read_as_the_strings_the_file_gave() {
    let r = read(r#"<PageSheet><Cell N="PinX" V="1.50" U="IN" F="Width*0.5" E="&#35;REF!" Extra="z" Other="1"/></PageSheet>"#);
    let cell = r.sheet.cell("PinX").unwrap();
    // Nothing is normalised: `1.50` stays `1.50`, whatever number it is.
    assert_eq!(cell.value.as_ref().unwrap().text(), "1.50");
    assert_eq!(cell.unit.as_deref(), Some("IN"));
    assert_eq!(cell.error.as_deref(), Some("#REF!"));
    assert_eq!(cell.formula.text(), Some("Width*0.5"));
    // An attribute the model has no field for is kept, in file order.
    assert_eq!(cell.raw.iter().collect::<Vec<_>>(), [("Extra", "z"), ("Other", "1")]);
    assert!(cell.extra.is_none());
}

#[test]
fn the_markers_of_a_formula_are_not_formulas_and_a_formula_that_does_not_parse_is_kept() {
    let r = read(r#"<PageSheet><Cell N="A" F="Inh"/><Cell N="B" F="No Formula"/><Cell N="C" F="1+"/><Cell N="D" V="3"/></PageSheet>"#);
    assert_eq!(r.sheet.cell("A").unwrap().formula, FormulaSlot::Inherit);
    assert_eq!(r.sheet.cell("B").unwrap().formula, FormulaSlot::NoFormula);
    let c = r.sheet.cell("C").unwrap();
    assert!(matches!(&c.formula, FormulaSlot::Local(f) if f.ast.is_err()));
    assert_eq!(c.formula.text(), Some("1+"));
    assert_eq!(r.sheet.cell("D").unwrap().formula, FormulaSlot::Absent);
    // One cell holds a formula of its own, and it did not parse: counted by its class.
    let stats = r.reader.stats();
    assert_eq!(stats.formula_cells, 1);
    assert_eq!(r.warnings.formula_errors.values().sum::<u64>(), 1);
}

#[test]
fn the_three_style_links_are_typed_when_they_are_the_text_of_their_number() {
    let r = read(r#"<PageSheet LineStyle="3" FillStyle="" TextStyle="007" UniqueID="{X}"/>"#);
    assert_eq!(r.sheet.links.line, Some(3));
    // An empty link is no link, and `007` is the number 7 whose text the number cannot give back: both stay as attributes.
    assert_eq!(r.sheet.links.fill, None);
    assert_eq!(r.sheet.links.text, Some(7));
    assert_eq!(r.sheet.raw.get("FillStyle"), Some(""));
    assert_eq!(r.sheet.raw.get("TextStyle"), Some("007"));
    assert_eq!(r.sheet.raw.get("UniqueID"), Some("{X}"));
    assert_eq!(r.sheet.raw.get("LineStyle"), None);
    // Each of the three is its own family's link, and the one that is the text of its number needs no attribute.
    let r = read(r#"<PageSheet LineStyle="1" FillStyle="2" TextStyle="3"/>"#);
    assert_eq!((r.sheet.links.line, r.sheet.links.fill, r.sheet.links.text), (Some(1), Some(2), Some(3)));
    assert!(r.sheet.raw.is_empty());
}

#[test]
fn a_cell_with_text_only_keeps_it_as_text_and_one_with_anything_else_keeps_all_of_it_in_order() {
    let r = read(
        r#"<PageSheet><Cell N="Copyright">Some <![CDATA[text]]></Cell><Cell N="Mixed" V="1">a<RefBy ID="4"/><!--c--><?NewValue V?>b</Cell></PageSheet>"#,
    );
    let text = r.sheet.cell("Copyright").unwrap().extra.as_ref().unwrap();
    assert_eq!(text.text.as_deref(), Some("Some text"));
    assert!(text.passthrough.is_empty());
    let mixed = r.sheet.cell("Mixed").unwrap().extra.as_ref().unwrap();
    assert_eq!(mixed.text, None);
    let kinds: Vec<&str> = mixed
        .passthrough
        .iter()
        .map(|p| match &p.node {
            RawNode::Text(_) => "text",
            RawNode::Element(_) => "element",
            RawNode::Comment(_) => "comment",
            RawNode::Pi { .. } => "pi",
        })
        .collect();
    assert_eq!(kinds, ["text", "element", "comment", "pi", "text"]);
}

#[test]
fn a_cell_without_a_name_and_a_second_cell_of_one_name_are_kept_as_unknown_elements() {
    let r = read(r#"<PageSheet><Cell V="1"/><Cell N="" V="2"/><Cell N="A" V="3"/><Cell N="A" V="4"/></PageSheet>"#);
    assert_eq!(r.sheet.cells.len(), 1);
    assert_eq!(r.sheet.cell("A").unwrap().value.as_ref().unwrap().text(), "3");
    // The three that could not go in the cells are kept whole, after the one cell that did.
    let kept: Vec<_> = r.sheet.passthrough.iter().collect();
    assert_eq!(kept.len(), 3);
    assert!(kept.iter().all(|p| p.after == 0 || p.after == 1));
    assert_eq!(r.warnings.count("cell_without_name"), 2);
    assert_eq!(r.warnings.count("duplicate_cell"), 1);
    // A cell kept is a cell paid for.
    assert_eq!(r.reader.stats().cells, 4);
}

#[test]
fn unknown_children_are_kept_where_they_stood_and_a_foreign_element_is_never_a_cell() {
    let r = read(&format!(
        r#"<PageSheet xmlns="{}" xmlns:x="urn:other"><Cell N="A"/><Trigger N="t"/><x:Cell N="B"/><!--note--><Cell N="C"/>tail</PageSheet>"#,
        super::MAIN
    ));
    assert_eq!(r.sheet.cells.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["A", "C"]);
    // `after` is the number of typed children before the node: the trigger, the foreign cell and the comment followed one cell, the text two.
    let after: Vec<u32> = r.sheet.passthrough.iter().map(|p| p.after).collect();
    assert_eq!(after, [1, 1, 1, 2]);
}

#[test]
fn blank_text_between_elements_is_layout_and_is_not_kept() {
    let r = read("<PageSheet>\n  <Cell N=\"A\"/>\n  <Cell N=\"B\"/>\n</PageSheet>");
    assert!(r.sheet.passthrough.is_empty());
    assert_eq!(r.sheet.cells.len(), 2);
}

#[test]
fn a_geometry_section_without_an_ix_is_numbered_by_the_ones_before_it() {
    let r = read(
        r#"<PageSheet><Section N="Geometry"/><Section N="Geometry" IX="5"/><Section N="Geometry"/><Section N="Geometry" IX="2"/><Section N="User" IX="9"/></PageSheet>"#,
    );
    let ix: Vec<Option<u32>> = r.sheet.sections.iter().map(|s| s.ix).collect();
    // The first is 0, 5 is given, the third has two sections before it, and 2 is given; `IX` on any other section is no key.
    assert_eq!(ix, [Some(0), Some(5), Some(2), Some(2), None]);
    assert_eq!(r.sheet.sections.last().unwrap().raw.get("IX"), Some("9"));
    // The third (2 after 5) and the fourth (2 after 2) do not number upward.
    assert_eq!(r.warnings.count("geometry_ix_not_rising"), 2);
    assert!(r.sheet.section(&SectionKey::new("Geometry", Some(5))).is_some());
}

#[test]
fn a_section_named_twice_is_kept_with_a_warning_and_one_with_no_name_is_kept_as_an_element() {
    let r = read(r#"<PageSheet><Section N="User"/><Section N="User"/><Section/><Section N=""/></PageSheet>"#);
    assert_eq!(r.sheet.sections.len(), 2);
    assert_eq!(r.warnings.count("duplicate_section"), 1);
    assert_eq!(r.warnings.count("section_without_name"), 2);
    assert_eq!(r.sheet.passthrough.len(), 2);
}

#[test]
fn rows_are_keyed_by_name_where_the_section_is_keyed_by_name_and_by_index_where_it_is_keyed_by_index() {
    let r = read(
        r#"<PageSheet>
            <Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row><Row N="b"/><Row N="a"/><Row LocalName="x"/><Row N=""/></Section>
            <Section N="Geometry" IX="0"><Row T="MoveTo"/><Row T="LineTo" IX="7"/><Row T="LineTo"/></Section>
        </PageSheet>"#,
    );
    let user = &r.sheet.sections[0];
    assert_eq!(user.row(&RowKey::name("a")).unwrap().cells.len(), 1);
    // A repeated key keeps both rows, in order, and says so; a lookup finds the first.
    assert_eq!(user.rows.len(), 5);
    assert_eq!(r.warnings.count("duplicate_row_key"), 1);
    // A named row with no `N` is read as it is, and its `LocalName` is not made its name; `N=""` is no name and stays an attribute.
    assert_eq!(r.warnings.count("row_without_name"), 2);
    assert_eq!(user.rows[3].name, None);
    assert_eq!(user.rows[3].local_name.as_deref(), Some("x"));
    assert_eq!(user.rows[4].raw.get("N"), Some(""));
    let geometry = &r.sheet.sections[1];
    // An absent `IX` is the number of rows before it: 0, then 7 as given, then 2.
    assert_eq!(geometry.rows.iter().map(|row| row.ix).collect::<Vec<_>>(), [Some(0), Some(7), Some(2)]);
    assert_eq!(geometry.rows[1].row_type.as_deref(), Some("LineTo"));
}

#[test]
fn a_flag_is_one_and_every_other_text_stays_an_attribute() {
    let r = read(
        r#"<PageSheet><Section N="A" Del="1"/><Section N="B" Del="true"/><Section N="C" Del="0"/><Section N="D"><Row Del="1"/></Section></PageSheet>"#,
    );
    let del: Vec<bool> = r.sheet.sections.iter().map(|s| s.del).collect();
    assert_eq!(del, [true, true, false, false]);
    assert_eq!(r.sheet.sections[0].raw.get("Del"), None);
    assert_eq!(r.sheet.sections[1].raw.get("Del"), Some("true"));
    assert_eq!(r.sheet.sections[2].raw.get("Del"), Some("0"));
    assert!(r.sheet.sections[3].rows[0].del);
}

#[test]
fn the_units_and_errors_that_repeat_are_one_string() {
    let r = read(
        r#"<PageSheet><Cell N="A" U="IN" E="e"/><Cell N="B" U="IN" E="e"/><Section N="X"><Row T="LineTo"/><Row T="LineTo"/></Section></PageSheet>"#,
    );
    let (a, b) = (r.sheet.cell("A").unwrap(), r.sheet.cell("B").unwrap());
    assert!(Arc::ptr_eq(a.unit.as_ref().unwrap(), b.unit.as_ref().unwrap()));
    assert!(Arc::ptr_eq(a.error.as_ref().unwrap(), b.error.as_ref().unwrap()));
    let rows = &r.sheet.sections[0].rows;
    assert!(Arc::ptr_eq(rows[0].row_type.as_ref().unwrap(), rows[1].row_type.as_ref().unwrap()));
}

/// What each thing costs: an element is one item and each of its attributes one more; a cell is a cell besides; what is kept whole costs a
/// node for each of its nodes. These are the numbers the budgets of a document are spent in, and they are the numbers the reader had before the
/// codec moved (the VSDX reader's report states them in `Stats`).
#[test]
fn the_charges_are_one_item_for_an_element_and_one_for_each_attribute_and_one_cell_for_a_cell() {
    let r = read(r#"<PageSheet><Cell N="A" V="1"/><Section N="Geometry"><Row T="MoveTo"><Cell N="X" V="0"/></Row></Section></PageSheet>"#);
    let stats = r.reader.stats();
    // PageSheet 1, cell 1 + 2, section 1 + 1, row 1 + 1, cell 1 + 2.
    assert_eq!(stats.items, 11);
    assert_eq!(stats.cells, 2);
    assert_eq!((stats.max_cells_in_one_place, stats.max_sections_in_a_sheet, stats.max_rows_in_a_section), (1, 1, 1));
    assert_eq!(stats.formula_nodes, 0);
    assert_eq!(BIG.items - r.reader.items_left(), 11);
    assert_eq!(BIG.cells - r.reader.cells_left(), 2);
}

#[test]
fn what_is_kept_whole_costs_each_node_of_it_and_a_text_cell_one_more() {
    // `Trigger` with one attribute and a child with two: 2 + 3. A cell of text: the cell and its attribute 2, the text 1. Blank text between
    // elements costs nothing.
    let r = read("<PageSheet>\n<Trigger N=\"t\"><a x=\"1\" y=\"2\"/></Trigger>\n<Cell N=\"A\">hi</Cell>\n</PageSheet>");
    assert_eq!(r.reader.stats().items, 1 + 5 + 2 + 1);
    // A cell holding an element and a comment: the cell's 2, the element's 2 (itself and its attribute), the comment's 1.
    let r = read(r#"<PageSheet><Cell N="A"><RefBy ID="4"/><!--c--></Cell></PageSheet>"#);
    assert_eq!(r.reader.stats().items, 1 + 2 + 2 + 1);
}

#[test]
fn a_formula_costs_nodes_from_its_own_budget_and_one_the_budget_cannot_afford_is_kept_as_text() {
    let xml = r#"<PageSheet><Cell N="A" F="1+2+3+4+5+6+7+8+9"/><Cell N="B" F="1+2"/></PageSheet>"#;
    let r = read(xml);
    let spent = r.reader.stats().formula_nodes;
    assert!(spent > 0 && r.reader.stats().formula_cells == 2);
    // With a budget a little short of that, the cells are still there with their text, and the budget was not passed.
    let caps = diacraft_sheetxml::Caps { formula_nodes: spent - 1, ..BIG };
    let r = read_with::<Main>(xml, caps).unwrap();
    assert!(r.reader.stats().formula_nodes < spent);
    assert_eq!(r.sheet.cell("A").unwrap().formula.text(), Some("1+2+3+4+5+6+7+8+9"));
    assert_eq!(r.sheet.cell("B").unwrap().formula.text(), Some("1+2"));
    assert!(r.warnings.formula_errors.values().sum::<u64>() >= 1);
}

#[test]
fn the_section_a_refusal_was_in_is_named_by_a_fixed_word_and_cleared_when_the_section_ends() {
    // A refusal inside a predefined section names it; inside a section a file named itself, `<other section>`: the name is content.
    let two_cells = |section: &str| format!(r#"<PageSheet><Section N="{section}"><Row N="a"><Cell N="X"/><Cell N="Y"/></Row></Section></PageSheet>"#);
    let caps = diacraft_sheetxml::Caps { cells: 1, ..BIG };
    let el = parse(&two_cells("User"));
    let mut reader = diacraft_sheetxml::SheetReader::new(caps);
    let mut warnings = diacraft_sheetxml::Warnings::new(5);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
    let e = reader.read_sheet_only(&mut cx, &el, &sc).unwrap_err();
    assert_eq!(e.kind, LimitKind::Cells);
    assert_eq!(e.max, 1);
    assert_eq!(reader.current_section(), "User");

    let el = parse(&two_cells("Made up by a person"));
    let mut reader = diacraft_sheetxml::SheetReader::new(caps);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
    assert!(reader.read_sheet_only(&mut cx, &el, &sc).is_err());
    assert_eq!(reader.current_section(), "<other section>");

    // A read that ends well leaves no section.
    let r = read(&two_cells("User"));
    assert_eq!(r.reader.current_section(), "");
}

#[test]
fn the_same_input_gives_the_same_sheet_and_the_same_charges_in_any_vocabulary_that_reads_it() {
    let body = r#"<Cell N="A" V="1" F="B+1"/><Section N="User"><Row N="r"><Cell N="Value" V="x"/></Row></Section><Trigger N="t"/>"#;
    let main = read_with::<Main>(&format!("<PageSheet>{body}</PageSheet>"), BIG).unwrap();
    // The namespace is declared above the sheet, so that the sheet has no attribute of its own for it.
    let ext = read_in::<Ext>(&format!(r#"<w xmlns="{}"><PageSheet>{body}</PageSheet></w>"#, super::EXT), BIG).unwrap();
    assert_eq!(main.sheet, ext.sheet);
    assert_eq!(main.reader.stats(), ext.reader.stats());
}

#[test]
fn a_child_of_an_element_that_holds_a_sheet_is_a_cell_a_section_a_trigger_or_not_the_sheets_to_read() {
    // What a shape does with each of its children: the sheet's own are taken (a cell, a section), a trigger is kept in the sheet's passthrough, and
    // the rest is for the caller, which reads its own elements; a cell or a section that cannot be one is kept whole.
    let root = parse(r#"<Shape><Cell N="A"/><Section N="User"/><Trigger N="t"/><Text/><Cell/><Section/></Shape>"#);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&root);
    let mut reader = diacraft_sheetxml::SheetReader::new(BIG);
    let mut warnings = diacraft_sheetxml::Warnings::new(5);
    let mut sheet = diacraft_doc::Sheet::default();
    let mut tally = diacraft_sheetxml::SheetTally::default();
    let mut took = Vec::new();
    for (typed, child) in root.elements().enumerate() {
        let csc = sc.enter(child);
        let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
        took.push(match reader.sheet_child(&mut cx, &mut sheet, &mut tally, child, &csc, typed).unwrap() {
            diacraft_sheetxml::Took::Typed => "typed",
            diacraft_sheetxml::Took::Kept => "kept",
            diacraft_sheetxml::Took::No => "no",
        });
    }
    assert_eq!(took, ["typed", "typed", "kept", "no", "kept", "kept"]);
    assert_eq!((sheet.cells.len(), sheet.sections.len()), (1, 1));
    // The three kept are in the sheet's own passthrough, each after the typed children the caller said came before it, and the `Text` is not.
    assert_eq!(sheet.passthrough.iter().map(|p| p.after).collect::<Vec<_>>(), [2, 4, 5]);
}

#[test]
fn the_units_and_row_types_that_repeat_are_shared_only_while_they_are_short_and_few() {
    // `T` of a row is kept once each too. A text past 32 bytes is never shared, and a table of 4,096 words takes no more: the 4,097th and later are
    // each their own copy (a hostile file could have a million different ones), while the first ones are still shared.
    let long = "L".repeat(40);
    let rows: String = (0..4_096).map(|i| format!(r#"<Row T="t{i}"/>"#)).collect();
    let xml = format!(
        r#"<PageSheet><Section N="X"><Row T="{long}"/><Row T="{long}"/>{rows}<Row T="late"/><Row T="late"/><Row T="t0"/></Section></PageSheet>"#
    );
    let r = read(&xml);
    let rows = &r.sheet.sections[0].rows;
    let t = |i: usize| rows[i].row_type.as_ref().unwrap();
    assert!(!Arc::ptr_eq(t(0), t(1)), "a long text is not shared");
    let n = rows.len();
    assert!(!Arc::ptr_eq(t(n - 3), t(n - 2)), "the table is full: a new word is not shared");
    assert!(Arc::ptr_eq(t(2), t(n - 1)), "a word that is in the table still is");
}

#[test]
fn what_a_section_and_a_row_do_not_read_is_kept_where_it_stood_among_their_typed_children() {
    // `after` is the number of typed children (cells, and rows in a section) before the node.
    let r =
        read(r#"<PageSheet><Section N="User" Odd="1"><Cell N="A"/><x/><Row N="r"><Cell N="B"/><y/><Cell N="C"/>z</Row><w/>t</Section></PageSheet>"#);
    let user = &r.sheet.sections[0];
    // The attribute `N` is the section's name and is no attribute of the section besides; the one the model has no field for is kept.
    assert_eq!(user.raw.iter().collect::<Vec<_>>(), [("Odd", "1")]);
    assert_eq!(user.passthrough.iter().map(|p| p.after).collect::<Vec<_>>(), [1, 2, 2]);
    let row = &user.rows[0];
    assert_eq!(row.cells.len(), 2);
    assert_eq!(row.passthrough.iter().map(|p| p.after).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(r.sheet.sections[0].rows[0].raw.iter().count(), 0);
}

#[test]
fn the_reader_never_makes_an_empty_extra() {
    // `Cell.extra` is `None` or holds something. The derived equality tells `Some(CellExtra::default())` from `None`, so a reader that made
    // an empty one would break `decode(encode(m)) == m`. Every shape of cell content: none, a pair of tags with nothing in them, text, empty character data,
    // a comment, an instruction, an element.
    let r = read(
        r#"<PageSheet><Cell N="A" V="1"/><Cell N="B" V="1"></Cell><Cell N="C">t</Cell><Cell N="D"><![CDATA[]]></Cell><Cell N="E"><!--c--></Cell><Cell N="F"><?NewValue V?></Cell><Cell N="G"><RefBy ID="4"/></Cell></PageSheet>"#,
    );
    assert_eq!(r.sheet.cells.len(), 7);
    for (name, cell) in &r.sheet.cells {
        if let Some(extra) = &cell.extra {
            assert_ne!(**extra, diacraft_doc::CellExtra::default(), "{}: an empty extra", name.as_str());
        }
    }
    assert!(r.sheet.cell("A").unwrap().extra.is_none() && r.sheet.cell("B").unwrap().extra.is_none());
    assert_eq!(r.sheet.cell("D").unwrap().extra.as_ref().unwrap().text.as_deref(), Some(""));
    for name in ["C", "D", "E", "F", "G"] {
        assert!(r.sheet.cell(name).unwrap().extra.is_some(), "{name}");
    }
}
