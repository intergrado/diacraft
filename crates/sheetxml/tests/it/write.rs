//! The writer and the patcher: a sheet as a tree, and a tree edited between two models. A read and a re-emit give the same text; an edit
//! changes the nodes it edits and nothing else; an edit the patcher cannot make is said, not done wrong.

use diacraft_container::xml::{El, Node, write_element};
use diacraft_doc::{Cell, RawNode, Sheet, Value};
use diacraft_sheetxml::PatchError;
use diacraft_sheetxml::build::{cell_attrs, cell_el, row_attrs, section_attrs, sheet_is_empty, sheet_only_el, with_prefix};
use diacraft_sheetxml::patch::{patch_sheet_children, patch_sheet_only};

use super::{BIG, Main, parse, read_with};

const RICH: &str = concat!(
    r#"<PageSheet LineStyle="3" TextStyle="007" UniqueID="{X}">"#,
    r#"<Cell N="PinX" V="1.50" U="IN" F="Width*0.5" E="&#35;REF!" Odd="1"/>"#,
    r#"<Cell N="Note">some text</Cell>"#,
    r#"<Section N="Geometry" IX="0" Del="true">"#,
    r#"<Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Inh"/></Row><Row T="LineTo"><Cell N="X" V="1"/></Row>"#,
    r#"</Section>"#,
    r#"<Section N="User"><Row N="a" LocalName="b" Del="1"><Cell N="Value" V="x"><RefBy ID="4"/></Cell></Row></Section>"#,
    r#"<Trigger N="t"/><x:foreign xmlns:x="urn:z"/>"#,
    r#"</PageSheet>"#
);

fn text_of(el: &El) -> String {
    let mut out = Vec::new();
    write_element(&mut out, el).unwrap();
    String::from_utf8(out).unwrap()
}

fn read_sheet(xml: &str) -> Sheet {
    read_with::<Main>(xml, BIG).unwrap().sheet
}

#[test]
fn a_sheet_written_and_read_again_is_the_sheet_and_its_text_is_the_text_it_was_read_from() {
    let sheet = read_sheet(RICH);
    let written = sheet_only_el("PageSheet", &sheet);
    // Cells first, in the order of their names, then the sections; the unknown nodes are put back after as many typed children as they followed.
    assert_eq!(read_sheet(&text_of(&written)), sheet);
    let again = parse(&text_of(&written));
    assert_eq!(again, written);
    // The attributes the model has no field for come back as they were, and `raw` wins over the typed number.
    assert_eq!(written.attr("TextStyle"), Some("007"));
    assert_eq!(written.attr("UniqueID"), Some("{X}"));
    let names: Vec<&str> = written
        .children
        .iter()
        .filter_map(|n| match n {
            Node::El(e) => Some(e.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Cell", "Cell", "Section", "Section", "Trigger", "x:foreign"]);
}

#[test]
fn the_attributes_of_each_element_are_the_typed_ones_in_the_schema_order_and_then_the_rest() {
    let sheet = read_sheet(RICH);
    let pin = sheet.cell("PinX").unwrap();
    let attrs = |a: Vec<(String, String)>| a.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>();
    assert_eq!(attrs(cell_attrs(pin)), ["V=1.50", "U=IN", "E=#REF!", "F=Width*0.5", "Odd=1"]);
    let geometry = &sheet.sections[0];
    assert_eq!(attrs(section_attrs(geometry)), ["N=Geometry", "IX=0", "Del=true"]);
    let row = &sheet.sections[1].rows[0];
    assert_eq!(attrs(row_attrs(row)), ["N=a", "LocalName=b", "Del=1"]);
    let line = &geometry.rows[1];
    assert_eq!(attrs(row_attrs(line)), ["T=LineTo", "IX=1"]);
    assert_eq!(cell_el("PinX", pin).attrs[0], ("N".to_owned(), "PinX".to_owned()));
}

#[test]
fn a_sheet_with_nothing_in_it_is_empty_and_one_with_any_one_thing_is_not() {
    assert!(sheet_is_empty(&Sheet::default()));
    assert!(!sheet_is_empty(&read_sheet(r#"<PageSheet><Cell N="A"/></PageSheet>"#)));
    assert!(!sheet_is_empty(&read_sheet(r#"<PageSheet><Section N="A"/></PageSheet>"#)));
    assert!(!sheet_is_empty(&read_sheet(r#"<PageSheet LineStyle="1"/>"#)));
    assert!(!sheet_is_empty(&read_sheet(r#"<PageSheet UniqueID="x"/>"#)));
    assert!(!sheet_is_empty(&read_sheet(r#"<PageSheet><Trigger/></PageSheet>"#)));
}

#[test]
fn a_prefix_is_put_before_the_elements_that_have_none_and_an_empty_prefix_changes_nothing() {
    let mut el = parse(r#"<Section N="User"><Row N="a"><Cell N="V"><q:keep/><deep><Cell/></deep></Cell></Row></Section>"#);
    let before = el.clone();
    with_prefix(&mut el, "");
    assert_eq!(el, before);
    with_prefix(&mut el, "e");
    assert_eq!(text_of(&el), r#"<e:Section N="User"><e:Row N="a"><e:Cell N="V"><q:keep/><e:deep><e:Cell/></e:deep></e:Cell></e:Row></e:Section>"#);
}

// ------------------------------------------------------------------------------------------------------------------ the patcher

/// The tree `xml` is, patched from the model of `base` to the model of `now`.
fn patched(xml: &str, edit: impl FnOnce(&mut Sheet)) -> Result<String, PatchError> {
    let base = read_sheet(xml);
    let mut now = base.clone();
    edit(&mut now);
    let mut el = parse(xml);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now)?;
    Ok(text_of(&el))
}

const LAID_OUT: &str = "<PageSheet>\n  <Cell N=\"A\" V=\"1.50\" Odd=\"1\"/>\n  <Cell N=\"B\" V=\"2\"/>\n  <Section N=\"User\">\n    <Row N=\"r\">\n      <Cell N=\"Value\" V=\"x\"/>\n    </Row>\n  </Section>\n</PageSheet>";

#[test]
fn an_edit_that_changes_nothing_changes_nothing_in_the_tree() {
    assert_eq!(patched(LAID_OUT, |_| {}).unwrap(), text_of(&parse(LAID_OUT)));
}

#[test]
fn a_cell_that_changed_has_the_attributes_that_changed_set_and_every_other_attribute_and_every_other_cell_as_it_was() {
    let out = patched(LAID_OUT, |sheet| {
        let a = sheet.cells.get_mut("A").unwrap();
        a.value = Some(Value::from_text("9"));
        a.unit = Some("IN".into());
    })
    .unwrap();
    // `V` is set where it stood, `U` goes last, `Odd` and the layout stay, and `B` is not looked at.
    assert_eq!(out, LAID_OUT.replace(r#"<Cell N="A" V="1.50" Odd="1"/>"#, r#"<Cell N="A" V="9" Odd="1" U="IN"/>"#));
}

#[test]
fn a_cell_taken_out_goes_with_the_white_space_before_it_and_one_put_in_goes_after_the_last_cell_there() {
    let out = patched(LAID_OUT, |sheet| {
        sheet.cells.remove("A");
        sheet.cells.insert("C".into(), Cell::read(None, Some("3"), None, None, &mut 0));
    })
    .unwrap();
    let expected = LAID_OUT
        .replace("\n  <Cell N=\"A\" V=\"1.50\" Odd=\"1\"/>", "")
        .replace("  <Cell N=\"B\" V=\"2\"/>", "  <Cell N=\"B\" V=\"2\"/><Cell N=\"C\" V=\"3\"/>");
    assert_eq!(out, expected);
}

#[test]
fn a_row_and_a_section_that_were_added_are_made_whole_and_one_taken_out_is_gone() {
    let out = patched(LAID_OUT, |sheet| {
        let user = std::sync::Arc::make_mut(&mut sheet.sections[0]);
        let mut row = (*user.rows[0]).clone();
        row.name = Some("s".into());
        user.rows.push(std::sync::Arc::new(row));
        user.rows.remove(0);
        sheet.sections.push(std::sync::Arc::new(diacraft_doc::Section::new("Misc", None)));
    })
    .unwrap();
    assert!(out.contains(r#"<Row N="s"><Cell N="Value" V="x"/></Row>"#), "{out}");
    assert!(!out.contains(r#"<Row N="r">"#), "{out}");
    // The new section goes after the last section, ahead of the layout that closes the sheet.
    assert!(out.contains(r#"</Section><Section N="Misc"/>"#) && out.ends_with("</PageSheet>"), "{out}");
}

#[test]
fn an_edit_to_what_the_patcher_does_not_write_is_unsupported_and_a_tree_that_is_not_the_one_read_is_a_mismatch() {
    // The unknown nodes of a sheet are not written.
    let e = patched(RICH, |sheet| sheet.passthrough = Default::default()).unwrap_err();
    assert!(matches!(e, PatchError::Unsupported(what) if what.contains("unknown elements of a sheet")), "{e:?}");
    // A tree that lacks a cell the model was read from.
    let base = read_sheet(r#"<PageSheet><Cell N="A"/><Cell N="B"/></PageSheet>"#);
    let mut now = base.clone();
    now.cells.remove("B");
    let mut el = parse(r#"<PageSheet><Cell N="A"/></PageSheet>"#);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let e = patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now).unwrap_err();
    assert!(matches!(e, PatchError::Mismatch(_)), "{e:?}");
    assert!(e.to_string().contains("is not the one this drawing was opened from"));
    // A section the tree has not.
    let base = read_sheet(r#"<PageSheet><Section N="User"/></PageSheet>"#);
    let mut el = parse(r#"<PageSheet/>"#);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let mut now = base.clone();
    now.sections.clear();
    let e = patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now).unwrap_err();
    assert!(matches!(e, PatchError::Mismatch(_)), "{e:?}");
}

#[test]
fn an_element_that_is_only_a_sheet_is_found_by_name_made_when_it_was_empty_and_left_alone_when_neither_holds_anything() {
    let root_xml = r#"<Page><Icon/><Rel r:id="rId1"/></Page>"#;
    let empty = Sheet::default();
    let one = read_sheet(r#"<PageSheet LineStyle="2"><Cell N="A" V="1"/></PageSheet>"#);
    let sc_of = |el: &El| diacraft_sheetxml::Scope::<Main>::empty().enter(el);

    // Neither holds anything: nothing is made.
    let mut el = parse(root_xml);
    let sc = sc_of(&el);
    patch_sheet_only::<Main, PatchError>(&mut el, &sc, "PageSheet", &["Rel"], &empty, &empty).unwrap();
    assert_eq!(text_of(&el), text_of(&parse(root_xml)));

    // The old one was empty and the new one is not: it is made, before the child named.
    let mut el = parse(root_xml);
    let sc = sc_of(&el);
    patch_sheet_only::<Main, PatchError>(&mut el, &sc, "PageSheet", &["Rel"], &empty, &one).unwrap();
    assert_eq!(text_of(&el), r#"<Page><Icon/><PageSheet LineStyle="2"><Cell N="A" V="1"/></PageSheet><Rel r:id="rId1"/></Page>"#);

    // It is found by its name and patched.
    let mut el = parse(&text_of(&el));
    let sc = sc_of(&el);
    let mut two = one.clone();
    two.links.line = Some(5);
    two.cells.get_mut("A").unwrap().value = Some(Value::from_text("2"));
    patch_sheet_only::<Main, PatchError>(&mut el, &sc, "PageSheet", &["Rel"], &one, &two).unwrap();
    assert_eq!(text_of(&el), r#"<Page><Icon/><PageSheet LineStyle="5"><Cell N="A" V="2"/></PageSheet><Rel r:id="rId1"/></Page>"#);

    // The model says there was a sheet and the tree has none.
    let mut el = parse(root_xml);
    let sc = sc_of(&el);
    let e = patch_sheet_only::<Main, PatchError>(&mut el, &sc, "PageSheet", &["Rel"], &one, &two).unwrap_err();
    assert!(matches!(e, PatchError::Unsupported(_)), "{e:?}");
}

#[test]
fn the_kept_nodes_of_a_cell_are_written_with_the_cell_and_a_changed_content_replaces_it() {
    let xml = r#"<PageSheet><Cell N="A" V="1"><RefBy ID="4"/></Cell></PageSheet>"#;
    let sheet = read_sheet(xml);
    let written = text_of(&sheet_only_el("PageSheet", &sheet));
    assert_eq!(written, xml);
    let out = patched(xml, |s| {
        let cell = s.cells.get_mut("A").unwrap();
        cell.extra = None;
    })
    .unwrap();
    assert_eq!(out, r#"<PageSheet><Cell N="A" V="1"/></PageSheet>"#);
    let kept = read_sheet(xml).cell("A").unwrap().extra.clone().unwrap();
    assert!(matches!(&kept.passthrough.iter().next().unwrap().node, RawNode::Element(e) if &*e.name == "RefBy"));
}

#[test]
fn a_cell_only_taken_out_and_a_cell_only_put_in_are_each_made_and_a_changed_row_and_section_are_patched() {
    // Only taken out: the cell is gone and nothing is put in its place.
    let only_out = patched(LAID_OUT, |sheet| {
        sheet.cells.remove("B");
    })
    .unwrap();
    assert_eq!(only_out, LAID_OUT.replace("\n  <Cell N=\"B\" V=\"2\"/>", ""));
    // Only put in: the new cell follows the last cell.
    let only_in = patched(LAID_OUT, |sheet| {
        sheet.cells.insert("C".into(), Cell::read(None, Some("3"), None, None, &mut 0));
    })
    .unwrap();
    assert_eq!(only_in, LAID_OUT.replace("  <Cell N=\"B\" V=\"2\"/>", "  <Cell N=\"B\" V=\"2\"/><Cell N=\"C\" V=\"3\"/>"));
    // A row's own attributes, its cell, and the section's own attribute: each is set where it stands and the rest of the tree is as it was.
    let out = patched(LAID_OUT, |sheet| {
        let user = std::sync::Arc::make_mut(&mut sheet.sections[0]);
        user.del = true;
        let mut row = (*user.rows[0]).clone();
        row.del = true;
        row.cells.get_mut("Value").unwrap().value = Some(Value::from_text("y"));
        user.rows[0] = std::sync::Arc::new(row);
    })
    .unwrap();
    let expected = LAID_OUT
        .replace(r#"<Section N="User">"#, r#"<Section N="User" Del="1">"#)
        .replace(r#"<Row N="r">"#, r#"<Row N="r" Del="1">"#)
        .replace(r#"<Cell N="Value" V="x"/>"#, r#"<Cell N="Value" V="y"/>"#);
    assert_eq!(out, expected);
}

#[test]
fn every_row_taken_out_of_a_section_goes_and_rows_put_in_a_section_that_had_none_are_made() {
    let out = patched(LAID_OUT, |sheet| {
        std::sync::Arc::make_mut(&mut sheet.sections[0]).rows.clear();
    })
    .unwrap();
    assert!(!out.contains("<Row"), "{out}");
    assert!(out.contains(r#"<Section N="User">"#), "{out}");
    let xml = r#"<PageSheet><Section N="User"/></PageSheet>"#;
    let out = patched(xml, |sheet| {
        let user = std::sync::Arc::make_mut(&mut sheet.sections[0]);
        user.rows.push(std::sync::Arc::new(diacraft_doc::Row { name: Some("n".into()), ..diacraft_doc::Row::default() }));
    })
    .unwrap();
    assert_eq!(out, r#"<PageSheet><Section N="User"><Row N="n"/></Section></PageSheet>"#);
}

#[test]
fn a_tree_with_the_same_number_of_cells_but_other_names_is_a_mismatch_in_words() {
    let base = read_sheet(r#"<PageSheet><Cell N="A"/><Cell N="B"/></PageSheet>"#);
    let mut now = base.clone();
    now.cells.get_mut("A").unwrap().value = Some(Value::from_text("1"));
    let mut el = parse(r#"<PageSheet><Cell N="A"/><Cell N="C"/></PageSheet>"#);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let e = patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now).unwrap_err();
    assert_eq!(e, PatchError::Mismatch("a sheet does not hold the cells the model was read from"));
}

#[test]
fn a_model_that_lost_everything_of_a_sheet_the_tree_never_had_is_no_change_and_no_error() {
    let one = read_sheet(r#"<PageSheet LineStyle="2"><Cell N="A" V="1"/></PageSheet>"#);
    let mut el = parse(r#"<Page><Icon/></Page>"#);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    patch_sheet_only::<Main, PatchError>(&mut el, &sc, "PageSheet", &[], &one, &Sheet::default()).unwrap();
    assert_eq!(text_of(&el), "<Page><Icon/></Page>");
}

/// The text of the tree after patching it from `xml`'s own model to the edited one, whatever the result.
fn patch_and_show(xml: &str, edit: impl FnOnce(&mut Sheet)) -> (Result<(), PatchError>, String) {
    let base = read_sheet(xml);
    let mut now = base.clone();
    edit(&mut now);
    let mut el = parse(xml);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let result = patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now);
    (result, text_of(&el))
}

#[test]
fn an_error_found_before_the_first_edit_leaves_the_tree_as_it_was_and_never_empties_it() {
    // An unsupported change to the unknown nodes of the sheet.
    let xml = r#"<PageSheet><Cell N="A" V="1"/><Trigger N="t"/><Section N="User"><Row N="r"/></Section></PageSheet>"#;
    let (result, after) = patch_and_show(xml, |s| s.passthrough = Default::default());
    assert!(matches!(result, Err(PatchError::Unsupported(_))), "{result:?}");
    assert_eq!(after, text_of(&parse(xml)));
    // A tree that lacks a section the model was read from: the children of the sheet are all still there.
    let model = read_sheet(r#"<PageSheet><Cell N="A" V="1"/><Section N="User"/><Section N="Other"/></PageSheet>"#);
    let tree = r#"<PageSheet><Cell N="A" V="1"/>t<Section N="User"/></PageSheet>"#;
    let mut now = model.clone();
    now.sections.pop();
    let mut el = parse(tree);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let e = patch_sheet_children::<Main, PatchError>(&mut el, &sc, &model, &now).unwrap_err();
    assert_eq!(e, PatchError::Mismatch("a part does not hold the items the model was read from"));
    assert_eq!(text_of(&el), text_of(&parse(tree)));
    // The same for the sheet an element holds by itself.
    let mut page = parse(&format!("<Page>{}</Page>", r#"<PageSheet><Cell N="A" V="1"/></PageSheet>"#));
    let before = text_of(&page);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&page);
    let one = read_sheet(r#"<PageSheet><Cell N="A" V="1"/><Cell N="B"/></PageSheet>"#);
    let mut two = one.clone();
    two.cells.get_mut("A").unwrap().value = Some(Value::from_text("2"));
    let e = patch_sheet_only::<Main, PatchError>(&mut page, &sc, "PageSheet", &[], &one, &two).unwrap_err();
    assert!(matches!(e, PatchError::Mismatch(_)), "{e:?}");
    assert_eq!(text_of(&page), before);
}
