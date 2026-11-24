//! Tests of the check after a save, class by class: a drawing, and the drawing the file reads back as, with one class damaged.
//!
//! **Each class is tested by planting its loss** in the model read back, and the check must name that class and no other, with the count of the items that differ. The same losses are planted in a
//! real save in `tests/it/save_check.rs`, where the hook of [`super::with_tamper`] is the way in; here the two drawings are given directly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Connect, CustomProp, DefFamily, Definition, Document, FaceName, ForeignData, Format, Media, Page, Part, PartRole, RawAttrs,
    RawElement, RowKey, Shape, ShapeList, ShapeType, SharedVec, TextPart, Value,
};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

use super::*;

fn element(name: &str, attrs: &[(&str, &str)]) -> Arc<RawElement> {
    Arc::new(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() })
}

fn part(name: &str, role: PartRole, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

fn picture(bytes: &[u8]) -> ForeignData {
    ForeignData {
        picture: Some(Arc::new(Media { part: Arc::from("/visio/media/image1.png"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })),
        ..ForeignData::default()
    }
}

fn object() -> ForeignData {
    ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Object")]), picture: picture(&[9, 9]).picture, ..ForeignData::default() }
}

/// The drawing every test damages: two pages, a style sheet, a master, shapes that hold what each class counts, and the document's own data.
fn base() -> Document {
    let rect = ShapeBuilder::new(1)
        .kind(ShapeType::Shape)
        .named("Rect", "Rect")
        .data(1, "d1")
        .links(Some(1), Some(1), Some(1))
        .cell_u("PinX", "2", "IN")
        .cell_u("PinY", "2", "IN")
        .cell_u("Width", "2", "IN")
        .cell_u("Height", "1", "IN")
        .formula("LocPinX", "Width*0.5", "1")
        .cell("FillForegnd", "#ff0000")
        .cell("LineColor", "#00ff00")
        .cell("BeginArrow", "3")
        .cell("FillGradientEnabled", "1")
        .cell("Mystery", "7")
        .row_formula("Geometry", Some(0), RowKey::Ix(1), "X", "Width*0", "0")
        .row_formula("Geometry", Some(0), RowKey::Ix(2), "X", "Width*1", "2")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "Y", "0")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.2")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1")
        .row_cell("Hyperlink", None, RowKey::name("Link1"), "Address", "http://example.org/")
        .row_cell("User", None, RowKey::name("Owner"), "Value", "Ann")
        .row_cell("Property", None, RowKey::name("Cost"), "Value", "12")
        .row_cell("Connection", None, RowKey::Ix(0), "X", "2")
        .row_cell("Field", None, RowKey::Ix(0), "Value", "12")
        .row_cell("FillGradient", None, RowKey::Ix(0), "GradientStopColor", "#ffffff")
        .text_parts(vec![
            TextPart::Chars(Arc::from("Hello ")),
            TextPart::Fld { ix: 0, text: Arc::from("12") },
            TextPart::Chars(Arc::from(" world\nsecond")),
        ]);
    let group = ShapeBuilder::new(2)
        .kind(ShapeType::Group)
        .cell_u("PinX", "6", "IN")
        .child(ShapeBuilder::new(3).kind(ShapeType::Shape).cell_u("PinX", "1", "IN").text("inner"));
    let style = StyleBuilder::new()
        .name("Box")
        .cell("EnableLineProps", "1")
        .cell("EnableFillProps", "1")
        .cell("EnableTextProps", "1")
        .cell("FillForegnd", "#ff0000")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.17");
    let mut doc = DocBuilder::new()
        .style(1, style)
        .master(1, MasterBuilder::new().name("Rect").shape(ShapeBuilder::new(5).cell("Width", "1")))
        .page(
            1,
            PageBuilder::new()
                .name("Page-1")
                .cell("PageWidth", "11")
                .cell("PageHeight", "8.5")
                .row_cell("Layer", None, RowKey::Ix(0), "Name", "Connector")
                .shape(rect)
                .shape(group)
                .shape(ShapeBuilder::new(4).foreign(picture(&[1, 2, 3])))
                .shape(ShapeBuilder::new(5).foreign(object()))
                .shape(ShapeBuilder::new(6).kind(ShapeType::Shape).master(1).cell("PinX", "4")),
        )
        .page(2, PageBuilder::new().name("Page-2").cell("PageWidth", "11").cell("PageHeight", "8.5"))
        .build();
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.connects.push(Connect {
            from_sheet: 1,
            from_cell: Some(Arc::from("BeginX")),
            to_sheet: 6,
            to_cell: Some(Arc::from("PinX")),
            ..Connect::default()
        });
    }
    Arc::make_mut(&mut doc.fonts).extend([
        FaceName { name_u: Arc::from("Arial"), raw: RawAttrs::new(), ext: Default::default() },
        FaceName { name_u: Arc::from("Noto Sans"), raw: RawAttrs::new(), ext: Default::default() },
    ]);
    doc.title = Arc::from("A title");
    {
        let props = Arc::make_mut(&mut doc.props);
        props.creator = Some(Arc::from("Ann"));
        props.custom.push(CustomProp {
            format: Format::Vsdx,
            name: Arc::from("k"),
            value_type: Arc::from("lpwstr"),
            value: Arc::from("v"),
            raw: RawAttrs::new(),
        });
    }
    doc.parts.push(part("/visio/vbaProject.bin", PartRole::MacroProject, b"macro bytes"));
    doc.parts.push(part("/visio/embeddings/o1.bin", PartRole::Embedding, b"object bytes"));
    doc.parts.push(part("/customXml/item1.xml", PartRole::CustomXml, b"<x/>"));
    {
        let odf = &mut doc.ext.make_mut().odf;
        for (family, name) in [(DefFamily::Marker, "m1"), (DefFamily::Gradient, "g1"), (DefFamily::Hatch, "h1")] {
            odf.definitions.push(Definition { family, name: Arc::from(name), element: element("draw:x", &[("draw:name", name)]), bytes: None });
        }
        odf.layers.push(element("draw:layer", &[("draw:name", "layout")]));
        odf.layers.push(element("draw:layer", &[("draw:name", "controls")]));
    }
    doc
}

/// The drawing the file reads back as, made from `before` by `damage`.
fn damaged(before: &Document, damage: impl FnOnce(&mut Document)) -> Document {
    let mut after = before.clone();
    damage(&mut after);
    after
}

/// What the check says of `after` for `before`: the classes and their counts, and the values flag.
fn said(before: &Document, after: &Document) -> (Vec<(Class, u64)>, bool) {
    match compare(before, after, false) {
        Outcome::Checked(checked) => (checked.classes.iter().map(|d| (d.class, d.count)).collect(), checked.as_values),
        Outcome::Unchecked(why) => panic!("the check did not finish: {why}"),
    }
}

/// The classes the damage is told in.
fn classes(damage: impl FnOnce(&mut Document)) -> Vec<(Class, u64)> {
    let before = base();
    let after = damaged(&before, damage);
    said(&before, &after).0
}

fn page_mut(doc: &mut Document, at: usize) -> &mut Page {
    Arc::make_mut(doc.pages.get_mut(at).unwrap())
}

/// The path of child positions to the first shape of this `ID` in a list.
fn path_to(list: &ShapeList, id: u32) -> Option<Vec<usize>> {
    for (at, shape) in list.iter().enumerate() {
        if shape.id == id {
            return Some(vec![at]);
        }
        if let Some(mut inner) = path_to(&shape.children, id) {
            inner.insert(0, at);
            return Some(inner);
        }
    }
    None
}

/// The shape with this `ID` on a page, to change.
fn shape_at(page: &mut Page, id: u32) -> &mut Shape {
    let path = path_to(&page.shapes, id).unwrap();
    let (last, upper) = path.split_last().unwrap();
    let mut list = &mut page.shapes;
    for step in upper {
        list = &mut Arc::make_mut(list.get_mut(*step).unwrap()).children;
    }
    Arc::make_mut(list.get_mut(*last).unwrap())
}

/// The shape with this `ID` on the first page, to change.
fn shape_mut(doc: &mut Document, id: u32) -> &mut Shape {
    shape_at(page_mut(doc, 0), id)
}

fn set_cell(shape: &mut Shape, name: &str, value: &str) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    let unit = sheet.cells.get(name).and_then(|c| c.unit.clone());
    let mut cell = Cell::constant(Value::from_text(value));
    cell.unit = unit;
    sheet.cells.insert(CellName::new(name), cell);
}

/// Sets a cell of the shape, whether or not it has one.
fn set_cell_anywhere(shape: &mut Shape, name: &str, value: &str) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
}

fn drop_section(shape: &mut Shape, name: &str) {
    Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != name);
}

fn set_row_cell(shape: &mut Shape, section: &str, row: RowKey, name: &str, value: &str) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    let key = diacraft_doc::SectionKey::new(section, if section == "Geometry" { Some(0) } else { None });
    sheet.row_mut(&key, &row).unwrap().cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
}

fn set_text_chars(shape: &mut Shape, text: &str) {
    let mut parts: Vec<TextPart> = shape.text.as_ref().unwrap().parts.clone();
    if let Some(TextPart::Chars(chars)) = parts.iter_mut().find(|p| matches!(p, TextPart::Chars(_))) {
        *chars = Arc::from(text);
    }
    shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

/// The document without its parts of this role.
fn drop_parts(doc: &mut Document, role: PartRole) {
    let kept: Vec<Arc<Part>> = doc.parts.iter().filter(|p| p.role != role).cloned().collect();
    doc.parts = SharedVec::from_vec(kept);
}

fn set_text_parts(shape: &mut Shape, parts: Vec<TextPart>) {
    shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

// ----------------------------------------------------------------------------------------------------------------------------- a clean trip

#[test]
fn a_drawing_read_back_as_it_was_names_nothing() {
    let before = base();
    let outcome = compare(&before, &before.clone(), false);
    assert_eq!(outcome, Outcome::Checked(Checked::default()));
    assert!(outcome.is_clean());
    assert!(words(&outcome).is_empty(), "with nothing to say there is no card");
}

#[test]
fn what_the_file_holds_beyond_the_drawing_is_no_loss() {
    let before = base();
    let after = damaged(&before, |doc| {
        let rect = shape_mut(doc, 1);
        // Cells the drawing never held, with the values nothing gives them.
        set_cell(rect, "ExtraCell", "0");
        Arc::make_mut(&mut rect.sheet).cells.insert(CellName::new("LinePattern"), Cell::constant(Value::from_text("0")));
        Arc::make_mut(&mut doc.fonts).push(FaceName { name_u: Arc::from("Another"), raw: RawAttrs::new(), ext: Default::default() });
        doc.parts.push(part("/extra.bin", PartRole::Other, b"more"));
        Arc::make_mut(&mut doc.props).company = Some(Arc::from("Co"));
    });
    assert_eq!(said(&before, &after), (vec![], false));
}

/// **The comparison goes both ways** (an item): a cell the file holds that the drawing holds nothing for, not even from a style sheet or its master,
/// draws the shape otherwise than the drawing did when its value is not the one nothing gives (the number 0, black, nothing): it is a difference, in the class of its place. (It was told as a
/// value of the file's own: with the writer change taken out, the reader added a `FillGradientAngle` reading the file back, and the check named nothing.) A value the drawing's sheets
/// give, written on the shape, and a value nothing gives, are the same as before.
#[test]
fn a_value_the_file_gives_a_cell_the_drawing_held_nothing_for_is_a_difference() {
    // The drawing says nothing of the line pattern of the shape, and no style sheet gives it one; the file says 5: the line is drawn otherwise.
    let before = base();
    let after = damaged(&before, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.insert(CellName::new("LinePattern"), Cell::constant(Value::from_text("5")));
    });
    assert_eq!(said(&before, &after).0, vec![(Class::StyleContent, 1)]);
    let after = damaged(&before, |doc| set_cell(shape_mut(doc, 1), "ExtraCell", "5"));
    assert_eq!(said(&before, &after).0, vec![(Class::ShapeAttributes, 1)]);
    // The angle of a gradient the drawing did not give one.
    let after = damaged(&before, |doc| set_cell_anywhere(shape_mut(doc, 1), "FillGradientAngle", "0.5236"));
    assert_eq!(said(&before, &after).0, vec![(Class::Gradients, 1)]);
    // What nothing gives a cell, written on it, is the same.
    let after = damaged(&before, |doc| set_cell_anywhere(shape_mut(doc, 1), "FillGradientAngle", "0"));
    assert_eq!(said(&before, &after).0, vec![]);
    // But a value the drawing did hold, from a style sheet, that the file writes otherwise is a value that differs.
    let from_style = damaged(&before, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.remove("FillForegnd");
    });
    let after = damaged(&from_style, |doc| set_cell(shape_mut(doc, 1), "FillForegnd", "#0000ff"));
    assert_eq!(said(&from_style, &after).0, vec![(Class::StyleContent, 1)]);
    // And a cell with no family that a root style gives, which the file writes otherwise, is a value that differs too.
    let mut with_root = base();
    Arc::make_mut(&mut Arc::make_mut(with_root.styles.get_mut(0).unwrap()).sheet)
        .cells
        .insert(CellName::new("Mystery"), Cell::constant(Value::from_text("7")));
    let from_root = damaged(&with_root, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.remove("Mystery");
    });
    let after = damaged(&from_root, |doc| set_cell_anywhere(shape_mut(doc, 1), "Mystery", "8"));
    assert_eq!(said(&from_root, &after).0, vec![(Class::ShapeAttributes, 1)]);
}

/// A section of `name` (with no `IX`) holding one row of `key` with the cell `cell` of `value`.
fn section_of(name: &str, key: RowKey, cell: &str, value: &str) -> Arc<diacraft_doc::Section> {
    let mut row = diacraft_doc::Row::default();
    match &key {
        RowKey::Ix(ix) => row.ix = Some(*ix),
        RowKey::Name(n) => row.name = Some(Arc::clone(n)),
    }
    row.cells.insert(CellName::new(cell), Cell::constant(Value::from_text(value)));
    let mut section = diacraft_doc::Section::new(name, None);
    section.rows.push(Arc::new(row));
    Arc::new(section)
}

/// **Rows and sections the file adds are differences in their class** (the comparison both ways): each cell of a row or a section the drawing does not hold is judged as a
/// cell the file adds (against what the drawing's sheets give it there, or what nothing gives), and the row or the section is told in its class. The reader's own rows (`ODF` names) are its
/// bookkeeping, as its cells of those names are.
#[test]
fn rows_and_sections_the_file_adds_are_differences_in_their_class() {
    let before = base();
    let add_section = |id: u32, section: Arc<diacraft_doc::Section>| {
        let before = &before;
        damaged(before, move |doc| Arc::make_mut(&mut shape_mut(doc, id).sheet).sections.push(section))
    };
    // A section the shape had none of: a hyperlink, counted by its row.
    assert_eq!(
        said(&before, &add_section(3, section_of("Hyperlink", RowKey::name("L"), "Address", "http://example.org/"))).0,
        vec![(Class::Hyperlinks, 1)]
    );
    // A row the shape's section had none of.
    let after = damaged(&before, |doc| {
        let sheet = Arc::make_mut(&mut shape_mut(doc, 1).sheet);
        let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Hyperlink").unwrap();
        let mut row = diacraft_doc::Row { name: Some(Arc::from("Link2")), ..Default::default() };
        row.cells.insert(CellName::new("Address"), Cell::constant(Value::from_text("http://other/")));
        Arc::make_mut(section).rows.push(Arc::new(row));
    });
    assert_eq!(said(&before, &after).0, vec![(Class::Hyperlinks, 1)]);
    // A section that holds only what nothing gives is the same.
    assert_eq!(said(&before, &add_section(3, section_of("Hyperlink", RowKey::name("L"), "Address", ""))).0, vec![]);
    // The reader's own rows are not judged.
    assert_eq!(said(&before, &add_section(3, section_of("User", RowKey::name("ODFEq1"), "Value", "7"))).0, vec![]);
    // The formatting of a text the shape had none of.
    assert_eq!(said(&before, &add_section(3, section_of("Character", RowKey::Ix(0), "Size", "0.3"))).0, vec![(Class::TextFormatting, 1)]);
}

/// **The route a file writes on a connector is the route the drawing draws it in**: a connector whose `ShapeRouteStyle` and `ConLineRouteExt` are 0 or none takes its route
/// from the page's `RouteStyle` and `LineRouteExt`, and the `.odg` writer writes the route it is drawn in on the connector (`draw:type`, which reads back as the two cells). The
/// same route said on the connector is the same content, as values; another route is a difference of the shape. Before, a connector of a VSDX drawing that held no route cells was named as a
/// shape that differs after every save as an `.odg`.
#[test]
fn a_route_the_file_writes_on_a_connector_is_the_route_the_drawing_draws_it_in() {
    let route = |before: &Document, style: &str, ext: &str| {
        damaged(before, |doc| {
            let shape = shape_mut(doc, 1);
            set_cell_anywhere(shape, "ShapeRouteStyle", style);
            set_cell_anywhere(shape, "ConLineRouteExt", ext);
        })
    };
    // The page says nothing: the drawing draws the connector with right angles, which the file says as 1 and 1.
    let before = base();
    assert_eq!(said(&before, &route(&before, "1", "1")), (vec![], true));
    // Straight and curved are other routes.
    assert_eq!(said(&before, &route(&before, "2", "1")).0, vec![(Class::ShapeAttributes, 1)]);
    assert_eq!(said(&before, &route(&before, "1", "2")).0, vec![(Class::ShapeAttributes, 1)]);
    // A page of straight lines: the file's 2 and 1 on the connector are its route, and right angles are not.
    let straight = damaged(&before, |doc| {
        Arc::make_mut(&mut page_mut(doc, 0).sheet).cells.insert(CellName::new("RouteStyle"), Cell::constant(Value::from_text("2")));
    });
    assert_eq!(said(&straight, &route(&straight, "2", "1")), (vec![], true));
    assert_eq!(said(&straight, &route(&straight, "1", "1")).0, vec![(Class::ShapeAttributes, 1)]);
    // A connector that says 0 (the page's) is drawn as the page says, and a file that writes that route on it is the same.
    let zero = route(&straight, "0", "0");
    assert_eq!(said(&zero, &route(&zero, "2", "1")), (vec![], true));
    assert_eq!(said(&zero, &route(&zero, "1", "2")).0, vec![(Class::ShapeAttributes, 1)]);
    // Another cell the file adds is judged as before.
    assert_eq!(said(&before, &damaged(&before, |doc| set_cell_anywhere(shape_mut(doc, 1), "ExtraCell", "5"))).0, vec![(Class::ShapeAttributes, 1)]);
}

/// **What a shape is drawn with where no sheet gives a cell**: a line end whose size no sheet gives is drawn medium, 2 (the default), so a file that writes
/// 2 on the shape, or a drawing that holds 2 read back from a file that holds none, is the same content, as values; and a cell of the text block that the file does not hold reads its default
/// formula (`TxtPinX` reads `Width*0.5`), so a drawing's cell that holds that formula comes back as it. Before, every connector `connector.add` made was named for its line end and
/// for the six formulas of its text block after a save as an `.odg`.
#[test]
fn a_line_ends_drawn_size_and_a_text_blocks_default_formula_are_the_same_content() {
    let before = base();
    let sized = |v: &str| damaged(&before, |doc| set_cell_anywhere(shape_mut(doc, 1), "BeginArrowSize", v));
    assert_eq!(said(&before, &sized("2")), (vec![], true));
    assert_eq!(said(&before, &sized("4")).0, vec![(Class::LineEnds, 1)]);
    assert_eq!(said(&sized("2"), &before), (vec![], true));
    assert_eq!(said(&sized("4"), &before).0, vec![(Class::LineEnds, 1)]);
    // The text block: the drawing's `TxtPinX` is its default formula, and the file holds none.
    let block = |formula: &str, value: &str| {
        damaged(&before, |doc| {
            Arc::make_mut(&mut shape_mut(doc, 1).sheet)
                .cells
                .insert(CellName::new("TxtPinX"), Cell::with_formula(formula, Some(Value::from_text(value))));
        })
    };
    assert_eq!(said(&block("Width*0.5", "1"), &before), (vec![], true));
    // Another formula the file does not hold is lost, and its value is not what the file gives.
    assert_eq!(said(&block("Width*0.25", "0.5"), &before).0, vec![(Class::TextFormatting, 1), (Class::Formulas, 1)]);
}

/// **The label of a line or a connector with no text is the engine's**: the engine writes its text block's place and a white background whenever it routes a
/// connector it made, a text block with no text draws nothing, and the `.odg` writer leaves them out of the copy of such a shape, so a file that holds none of them is no difference. A
/// background of another colour is a person's, and a shape with text keeps its label.
#[test]
fn the_label_of_a_connector_with_no_text_is_the_engines() {
    let segment = |text: bool, background: &str| {
        damaged(&base(), |doc| {
            let shape = shape_mut(doc, 1);
            if !text {
                shape.text = None;
            }
            set_cell_anywhere(shape, "BeginX", "1");
            set_cell_anywhere(shape, "TxtPinX", "0.3");
            set_cell_anywhere(shape, "TextBkgnd", background);
        })
    };
    let without = |doc: &Document| {
        damaged(doc, |doc| {
            let sheet = Arc::make_mut(&mut shape_mut(doc, 1).sheet);
            sheet.cells.remove("TxtPinX");
            sheet.cells.remove("TextBkgnd");
        })
    };
    let plain = segment(false, "#FFFFFF");
    assert_eq!(said(&plain, &without(&plain)).0, vec![]);
    let red = segment(false, "#FF0000");
    assert_eq!(said(&red, &without(&red)).0, vec![(Class::TextFormatting, 1)], "a background a person chose");
    let labelled = segment(true, "#FFFFFF");
    assert_eq!(said(&labelled, &without(&labelled)).0, vec![(Class::TextFormatting, 1)], "a label with text");
}

// ----------------------------------------------------------------------------------------------------------------------------- the classes

#[test]
fn pages_that_are_missing_added_renamed_or_resized_are_pages() {
    assert_eq!(classes(|doc| drop(doc.pages.remove(1))), vec![(Class::Pages, 1)], "a page missing");
    assert_eq!(classes(|doc| doc.pages.push(Arc::new(Page::new(9, "New")))), vec![(Class::Pages, 1)], "a page added");
    assert_eq!(classes(|doc| page_mut(doc, 1).name = Arc::from("Other")), vec![(Class::Pages, 1)], "a page renamed");
    assert_eq!(
        classes(|doc| {
            Arc::make_mut(&mut page_mut(doc, 0).sheet).cells.insert(CellName::new("PageWidth"), Cell::constant(Value::from_text("12")));
        }),
        vec![(Class::Pages, 1)],
        "a page resized"
    );
    assert_eq!(classes(|doc| page_mut(doc, 1).background = true), vec![(Class::Pages, 1)], "a page made a background");
}

#[test]
fn shapes_that_are_missing_added_or_of_another_kind_are_shapes() {
    assert_eq!(classes(|doc| drop(page_mut(doc, 0).shapes.remove(2))), vec![(Class::Shapes, 1)], "the picture shape is missing");
    assert_eq!(classes(|doc| drop(page_mut(doc, 0).shapes.remove(1))), vec![(Class::Shapes, 2)], "a group and its member are missing");
    assert_eq!(classes(|doc| page_mut(doc, 0).shapes.push(Arc::new(Shape::new(70)))), vec![(Class::Shapes, 1)], "a shape added");
    assert_eq!(classes(|doc| shape_mut(doc, 6).kind = ShapeType::Group), vec![(Class::Shapes, 1)], "a shape of another kind");
    assert_eq!(classes(|doc| shape_mut(doc, 1).name_u = Some(Arc::from("Other"))), vec![(Class::ShapeAttributes, 1)], "a name is no kind");
}

#[test]
fn a_shape_the_file_gave_another_number_is_one_shape_with_another_number() {
    assert_eq!(classes(|doc| shape_mut(doc, 3).id = 33), vec![(Class::ShapeNumbers, 1)]);
}

#[test]
fn the_position_size_and_outline_of_a_shape_are_its_geometry() {
    assert_eq!(classes(|doc| set_cell(shape_mut(doc, 1), "PinX", "3")), vec![(Class::Geometry, 1)]);
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "Geometry", RowKey::Ix(2), "Y", "0.5")), vec![(Class::Geometry, 1)]);
    // The outline taken away takes its formulas with it.
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Geometry")), vec![(Class::Geometry, 1), (Class::Formulas, 2)]);
}

#[test]
fn the_characters_of_a_text_are_told_paragraph_by_paragraph() {
    assert_eq!(classes(|doc| set_text_chars(shape_mut(doc, 1), "Hullo ")), vec![(Class::Text, 1)]);
    assert_eq!(classes(|doc| shape_mut(doc, 3).text = None), vec![(Class::Text, 1)]);
    // Two paragraphs, both gone; the field with them.
    assert_eq!(classes(|doc| shape_mut(doc, 1).text = None), vec![(Class::Text, 2), (Class::Fields, 1)]);
    // A carriage return is a paragraph break, as a line feed is.
    let before = base();
    let after = damaged(&before, |doc| {
        let parts =
            vec![TextPart::Chars(Arc::from("Hello ")), TextPart::Fld { ix: 0, text: Arc::from("12") }, TextPart::Chars(Arc::from(" world\rsecond"))];
        set_text_parts(shape_mut(doc, 1), parts);
    });
    assert_eq!(said(&before, &after), (vec![], false));
}

#[test]
fn the_formatting_of_a_text_is_its_own_class() {
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "Character", RowKey::Ix(0), "Size", "0.3")), vec![(Class::TextFormatting, 1)]);
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "Paragraph", RowKey::Ix(0), "HorzAlign", "2")), vec![(Class::TextFormatting, 1)]);
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Character")), vec![(Class::TextFormatting, 1)]);
}

#[test]
fn a_picture_that_is_not_there_or_not_the_same_is_a_picture() {
    assert_eq!(classes(|doc| Arc::make_mut(shape_mut(doc, 4).foreign.as_mut().unwrap()).picture = None), vec![(Class::Pictures, 1)]);
    assert_eq!(classes(|doc| shape_mut(doc, 4).foreign = Some(Arc::new(picture(&[1, 2, 4])))), vec![(Class::Pictures, 1)]);
    assert_eq!(classes(|doc| shape_mut(doc, 4).foreign = None), vec![(Class::Pictures, 1)]);
    // A bitmap fill's bytes are a picture too.
    let before = {
        let mut doc = base();
        let image = Definition {
            family: DefFamily::FillImage,
            name: Arc::from("i1"),
            element: element("draw:fill-image", &[]),
            bytes: Some(Arc::from(&[5u8, 6][..])),
        };
        doc.ext.make_mut().odf.definitions.push(image);
        doc
    };
    let after = damaged(&before, |doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::FillImage));
    assert_eq!(said(&before, &after).0, vec![(Class::Pictures, 1)]);
}

#[test]
fn an_object_that_is_not_an_object_in_the_file_is_an_object() {
    assert_eq!(
        classes(|doc| shape_mut(doc, 5).foreign = Some(Arc::new(picture(&[9, 9])))),
        vec![(Class::Objects, 1)],
        "the frame shows the picture and is no object"
    );
    assert_eq!(classes(|doc| drop_parts(doc, PartRole::Embedding)), vec![(Class::Objects, 1)], "its entry is gone");
    // The bytes are there, and are no object entry.
    assert_eq!(
        classes(|doc| {
            drop_parts(doc, PartRole::Embedding);
            doc.parts.push(part("/diacraft/blob1.bin", PartRole::Other, b"object bytes"));
        }),
        vec![(Class::Objects, 1)]
    );
}

#[test]
fn a_macro_project_that_is_gone_is_told_and_one_left_out_by_choice_is_not() {
    assert_eq!(classes(|doc| drop_parts(doc, PartRole::MacroProject)), vec![(Class::Macros, 1)]);
    let before = base();
    let after = damaged(&before, |doc| drop_parts(doc, PartRole::MacroProject));
    assert_eq!(compare(&before, &after, true), Outcome::Checked(Checked::default()), "the caller chose to leave it out");
}

#[test]
fn a_font_face_that_is_dropped_is_a_font() {
    assert_eq!(classes(|doc| drop(Arc::make_mut(&mut doc.fonts).pop())), vec![(Class::Fonts, 1)]);
    assert_eq!(classes(|doc| Arc::make_mut(&mut doc.fonts).clear()), vec![(Class::Fonts, 2)]);
}

#[test]
fn line_ends_are_the_arrows_of_a_shape_and_the_markers_of_a_document() {
    assert_eq!(classes(|doc| set_cell(shape_mut(doc, 1), "BeginArrow", "0")), vec![(Class::LineEnds, 1)]);
    assert_eq!(classes(|doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Marker)), vec![(Class::LineEnds, 1)]);
}

#[test]
fn gradients_are_the_stops_of_a_shape_and_the_definitions_of_a_document() {
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "FillGradient")), vec![(Class::Gradients, 1)]);
    assert_eq!(classes(|doc| set_cell(shape_mut(doc, 1), "FillGradientEnabled", "0")), vec![(Class::Gradients, 1)]);
    assert_eq!(classes(|doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Gradient)), vec![(Class::Gradients, 1)]);
}

#[test]
fn the_rest_of_the_look_is_style_content() {
    assert_eq!(classes(|doc| set_cell(shape_mut(doc, 1), "FillForegnd", "#0000ff")), vec![(Class::StyleContent, 1)]);
    // Two cells of one shape are one shape.
    assert_eq!(
        classes(|doc| {
            set_cell(shape_mut(doc, 1), "FillForegnd", "#0000ff");
            set_cell(shape_mut(doc, 1), "LineColor", "#0000ff");
        }),
        vec![(Class::StyleContent, 1)]
    );
    assert_eq!(classes(|doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Hatch)), vec![(Class::StyleContent, 1)]);
}

#[test]
fn a_style_sheet_that_is_missing_or_renamed_is_a_style_sheet() {
    assert_eq!(classes(|doc| drop(doc.styles.remove(1))), vec![(Class::StyleSheets, 1)]);
    assert_eq!(
        classes(|doc| {
            let style = Arc::make_mut(doc.styles.get_mut(1).unwrap());
            style.name = Some(Arc::from("Other"));
            style.name_u = Some(Arc::from("Other"));
        }),
        vec![(Class::StyleSheets, 1)]
    );
}

#[test]
fn masters_the_ties_to_them_and_the_master_pages_are_masters() {
    assert_eq!(classes(|doc| doc.masters = SharedVec::new()), vec![(Class::Shapes, 1), (Class::Masters, 1)], "the master and the shape it holds");
    assert_eq!(classes(|doc| shape_mut(doc, 6).master = Some(8)), vec![(Class::Masters, 1)], "an instance of another master");
    let mut before = base();
    page_mut(&mut before, 0).ext.make_mut().odf.master_page = Some(Arc::from("Default"));
    let after = damaged(&before, |doc| page_mut(doc, 0).ext.make_mut().odf.master_page = Some(Arc::from("Other")));
    assert_eq!(said(&before, &after).0, vec![(Class::Masters, 1)]);
}

#[test]
fn layers_are_the_layer_set_and_the_layers_of_a_page() {
    assert_eq!(classes(|doc| doc.ext.make_mut().odf.layers.truncate(1)), vec![(Class::Layers, 1)]);
    assert_eq!(
        classes(|doc| {
            Arc::make_mut(&mut page_mut(doc, 0).sheet).sections.retain(|s| s.name.as_str() != "Layer");
        }),
        vec![(Class::Layers, 1)]
    );
}

#[test]
fn connections_are_the_ends_of_a_connector_and_the_points_of_a_shape() {
    assert_eq!(classes(|doc| page_mut(doc, 0).connects = Default::default()), vec![(Class::Connections, 1)]);
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Connection")), vec![(Class::Connections, 1)]);
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "Connection", RowKey::Ix(0), "X", "1")), vec![(Class::Connections, 1)]);
}

#[test]
fn a_formula_that_does_not_come_back_is_lost_though_its_value_does() {
    // The value is the same and the formula is gone.
    assert_eq!(
        classes(|doc| {
            let rect = shape_mut(doc, 1);
            Arc::make_mut(&mut rect.sheet).cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("1")));
        }),
        vec![(Class::Formulas, 1)]
    );
    // Another formula is not the formula.
    assert_eq!(
        classes(|doc| {
            let rect = shape_mut(doc, 1);
            Arc::make_mut(&mut rect.sheet).cells.insert(CellName::new("LocPinX"), Cell::with_formula("Width*0.25", Some(Value::from_text("1"))));
        }),
        vec![(Class::Formulas, 1)]
    );
    // A formula of a row of the outline, written as its value.
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "Geometry", RowKey::Ix(1), "X", "0")), vec![(Class::Formulas, 1)]);
    // A shape that is gone loses its formulas, and is counted as a shape.
    assert_eq!(classes(|doc| drop(page_mut(doc, 0).shapes.remove(0))), vec![(Class::Shapes, 1), (Class::Formulas, 3)]);
}

#[test]
fn hyperlinks_fields_and_user_cells_are_counted_by_the_row() {
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Hyperlink")), vec![(Class::Hyperlinks, 1)]);
    assert_eq!(
        classes(|doc| set_row_cell(shape_mut(doc, 1), "Hyperlink", RowKey::name("Link1"), "Address", "http://other/")),
        vec![(Class::Hyperlinks, 1)]
    );
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Field")), vec![(Class::Fields, 1)]);
    assert_eq!(classes(|doc| set_row_cell(shape_mut(doc, 1), "User", RowKey::name("Owner"), "Value", "Bob")), vec![(Class::UserCells, 1)]);
    assert_eq!(classes(|doc| drop_section(shape_mut(doc, 1), "Property")), vec![(Class::UserCells, 1)]);
    assert_eq!(classes(|doc| shape_mut(doc, 1).data1 = None), vec![(Class::UserCells, 1)], "the first box of shape data");
    // A field in the text is a field, and its shown characters are text.
    assert_eq!(
        classes(|doc| set_text_parts(shape_mut(doc, 1), vec![TextPart::Chars(Arc::from("Hello  world\nsecond"))])),
        vec![(Class::Text, 1), (Class::Fields, 1)]
    );
}

#[test]
fn any_other_cell_that_is_not_the_same_is_a_shape_attribute() {
    assert_eq!(classes(|doc| set_cell(shape_mut(doc, 1), "Mystery", "8")), vec![(Class::ShapeAttributes, 1)]);
    assert_eq!(classes(|doc| shape_mut(doc, 1).name = Some(Arc::from("Other"))), vec![(Class::ShapeAttributes, 1)]);
}

#[test]
fn the_properties_of_the_document_are_told() {
    assert_eq!(classes(|doc| doc.title = Arc::from("Another")), vec![(Class::Properties, 1)]);
    assert_eq!(classes(|doc| Arc::make_mut(&mut doc.props).creator = None), vec![(Class::Properties, 1)]);
    assert_eq!(classes(|doc| Arc::make_mut(&mut doc.props).custom.clear()), vec![(Class::Properties, 1)]);
    // The time of the last save, who saved and the program's version are the writer's own.
    let before = base();
    let after = damaged(&before, |doc| {
        let props = Arc::make_mut(&mut doc.props);
        props.modified = Some(Arc::from("2031-01-01T00:00:00Z"));
        props.last_modified_by = Some(Arc::from("Someone"));
        props.app_version = Some(Arc::from("0.2"));
    });
    assert_eq!(said(&before, &after).0, vec![]);
}

#[test]
fn what_a_format_keeps_beside_the_cells_is_kept_data_when_it_does_not_come_back() {
    let mut before = base();
    shape_at(page_mut(&mut before, 0), 1).raw = RawAttrs::from_pairs([("UniqueID", "{AA}")]);
    let after = damaged(&before, |doc| shape_at(page_mut(doc, 0), 1).raw = RawAttrs::new());
    assert_eq!(said(&before, &after).0, vec![(Class::KeptData, 1)]);
    assert_eq!(classes(|doc| drop_parts(doc, PartRole::CustomXml)), vec![(Class::KeptData, 1)], "an unknown part");
}

// ----------------------------------------------------------------------------------------------------------------------------- the same content

#[test]
fn the_same_content_in_the_forms_a_format_holds_is_named_once_and_never_counted() {
    let before = base();
    // A unit, a spelling of a colour, `Width*1` for `Width`, and a link to a style sheet written as the values it gives.
    type Form = Box<dyn Fn(&mut Document)>;
    let forms: Vec<(&str, Form)> = vec![
        (
            "a unit",
            Box::new(|doc| {
                let sheet = Arc::make_mut(&mut shape_mut(doc, 1).sheet);
                sheet.cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text("2")).with_unit("CM"));
            }),
        ),
        ("a colour", Box::new(|doc| set_cell(shape_mut(doc, 1), "FillForegnd", "#FF0000"))),
        (
            "a formula form",
            Box::new(|doc| {
                let sheet = Arc::make_mut(&mut shape_mut(doc, 1).sheet);
                let key = diacraft_doc::SectionKey::new("Geometry", Some(0));
                sheet
                    .row_mut(&key, &RowKey::Ix(2))
                    .unwrap()
                    .cells
                    .insert(CellName::new("X"), Cell::with_formula("Width", Some(Value::from_text("2"))));
            }),
        ),
        ("a style link", Box::new(|doc| Arc::make_mut(&mut shape_mut(doc, 1).sheet).links = diacraft_doc::Links::default())),
    ];
    for (what, damage) in forms {
        let after = damaged(&before, |doc| damage(doc));
        assert_eq!(said(&before, &after), (vec![], true), "{what}");
    }
    // A value a style sheet gives where the drawing held it on the shape.
    let after = damaged(&before, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.remove("FillForegnd");
    });
    assert_eq!(said(&before, &after), (vec![], true), "a cell taken from the style sheet");
    // A value the drawing took from the style sheet, written on the shape.
    let from_style = damaged(&before, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.remove("FillForegnd");
    });
    let after = damaged(&from_style, |doc| set_cell(shape_mut(doc, 1), "FillForegnd", "#ff0000"));
    assert_eq!(said(&from_style, &after), (vec![], true), "an inherited cell written locally with the same value");
    // But a cell written with another value than the style sheet gives is a value that differs.
    let after = damaged(&from_style, |doc| set_cell(shape_mut(doc, 1), "FillForegnd", "#0000ff"));
    assert_eq!(said(&from_style, &after).0, vec![(Class::StyleContent, 1)]);
}

#[test]
fn a_formula_form_that_changes_the_formula_is_not_a_form() {
    let before = base();
    // `Width*0` is no form of the number 0: the formula is lost.
    let after = damaged(&before, |doc| set_row_cell(shape_mut(doc, 1), "Geometry", RowKey::Ix(1), "X", "0"));
    assert_eq!(said(&before, &after).0, vec![(Class::Formulas, 1)]);
}

// ----------------------------------------------------------------------------------------------------------------------------- the words

#[test]
fn the_outcome_is_the_field_a_program_reads_and_the_words_a_person_reads() {
    let before = base();
    let after = damaged(&before, |doc| {
        drop(page_mut(doc, 0).shapes.remove(2));
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).links = diacraft_doc::Links::default();
        set_cell(shape_mut(doc, 6), "PinX", "9");
    });
    let outcome = compare(&before, &after, false);
    let json = outcome.to_json();
    assert_eq!(json["done"], true);
    assert_eq!(json["as_values"], true);
    assert_eq!(
        json["classes"],
        serde_json::json!([
            { "class": "shapes", "words": "1 shape is missing, added or of another kind", "count": 1 },
            { "class": "geometry", "words": "1 shape differs in position, size or outline", "count": 1 },
        ])
    );
    assert_eq!(Outcome::from_json(&json), Some(outcome.clone()), "the field reads back as the outcome");
    let said = words(&outcome);
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(said[0].starts_with("DiaCraft read the file back and compared it with the drawing, and the file differs in 2 ways."), "{}", said[0]);
    assert_eq!(said[1], "shapes: 1 shape is missing, added or of another kind");
    assert_eq!(said[2], "geometry: 1 shape differs in position, size or outline");
    assert!(said[3].starts_with("Written as values the file format holds:"), "{}", said[3]);
}

#[test]
fn a_check_that_cannot_finish_says_so_with_the_reason() {
    let outcome = Outcome::Unchecked("the file does not read back (zip_bad)".to_owned());
    assert_eq!(words(&outcome), vec!["The file was written; DiaCraft could not check it: the file does not read back (zip_bad)".to_owned()]);
    let json = outcome.to_json();
    assert_eq!((json["done"].as_bool(), json["reason"].as_str()), (Some(false), Some("the file does not read back (zip_bad)")));
    assert_eq!(Outcome::from_json(&json), Some(outcome));
    assert_eq!(Outcome::from_json(&serde_json::Value::Null), None, "a save that was not checked has no field");
    assert_eq!(
        Outcome::from_json(&serde_json::json!({ "done": true, "classes": [{ "class": "no such class", "count": 1 }], "as_values": false })),
        None
    );
}

#[test]
fn a_file_that_does_not_read_back_is_a_check_that_could_not_finish() {
    let outcome = check_odg(&base(), b"this is no package", false);
    match outcome {
        Outcome::Unchecked(why) => assert!(why.starts_with("the file does not read back (") && !why.contains("no package"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(check_native(&base(), b"this is no package", false), Outcome::Unchecked(_)));
}

// ----------------------------------------------------------------------------------------------------------------------------- the cost

#[test]
fn a_drawing_past_the_steps_the_check_may_take_is_not_checked_and_says_so() {
    let before = base();
    let outcome = compare_within(&before, &before.clone(), false, 5);
    assert_eq!(outcome, Outcome::Unchecked("the drawing is too big to compare with the file in one step".to_owned()));
    assert!(matches!(compare_within(&before, &before.clone(), false, 10_000), Outcome::Checked(_)));
}

/// A drawing of `shapes` shapes of `cells` cells and a text each, on one page, with `styles` style sheets and as many definitions, and up to a thousand parts.
fn big(shapes: u32, cells: u32, styles: u32, prefix: &str) -> Document {
    let mut page = PageBuilder::new().name("P");
    for id in 1..=shapes {
        let mut shape = ShapeBuilder::new(id).text("some text of the shape").cell("PinX", "1");
        for k in 0..cells {
            shape = shape.formula(&format!("Cell{k}"), "Width*0.5", "1");
        }
        page = page.shape(shape);
    }
    let mut doc = DocBuilder::new();
    for id in 1..=styles {
        doc = doc.style(id, StyleBuilder::new().name(&format!("S{id}")).cell("FillForegnd", "#ff0000"));
    }
    let mut doc = doc.page(1, page).build();
    let odf = &mut doc.ext.make_mut().odf;
    for id in 0..styles {
        odf.definitions.push(Definition {
            family: DefFamily::Marker,
            name: Arc::from(format!("{prefix}{id}")),
            element: element("draw:marker", &[("n", prefix)]),
            bytes: None,
        });
    }
    for id in 0..styles.min(1000) {
        doc.parts.push(part(&format!("/p{id}.bin"), PartRole::Other, &id.to_le_bytes()));
    }
    doc
}

fn steps_of(before: &Document, after: &Document) -> u64 {
    let mut walk = compare::Compare::new(before, after, false, u64::MAX);
    walk.run().unwrap();
    walk.steps()
}

#[test]
fn the_steps_of_the_check_grow_with_the_drawing_and_not_faster() {
    let (small, large) = (big(100, 5, 50, "m"), big(1_000, 5, 500, "m"));
    let (a, b) = (steps_of(&small, &small), steps_of(&large, &large));
    // Ten times the drawing, no more than eleven times the steps.
    assert!(b <= a * 11, "{a} steps for 100 shapes, {b} for 1,000");
    // A step for each thing walked: a few for a shape (its pair, its sheet, its text) and one for each cell, style, definition and part.
    assert!(b <= 1_000 * (5 + 8) + 500 * 6 + 5_000, "{b} steps");
}

#[test]
fn a_hostile_drawing_is_checked_within_its_steps_or_refused() {
    // Many styles and definitions that the file read back has other of: a scan of the file's definitions for each one missing is charged, and the product is within the budget.
    let before = big(10, 2, 1_000, "m");
    let after = big(10, 2, 0, "x");
    let mut after = after;
    after.ext.make_mut().odf.definitions = (0..1_000)
        .map(|id| Definition {
            family: DefFamily::Marker,
            name: Arc::from(format!("x{id}")),
            element: element("draw:marker", &[("n", "x")]),
            bytes: None,
        })
        .collect();
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    assert!(walk.run().is_ok(), "1,000 styles and as many definitions are within the budget");
    assert_eq!(walk.tally.count(Class::StyleSheets), 1_000, "every style sheet but the root");
    assert_eq!(walk.tally.count(Class::LineEnds), 1_000);
    // A thousand scans of a thousand definitions, each charged at the sixteen steps an element is worth.
    assert!(walk.steps() >= 16_000_000 && walk.steps() < 20_000_000, "the scans are charged: {} steps", walk.steps());
    // The same drawings under a smaller budget are refused, not walked: nothing is scanned that was not paid for first.
    let mut walk = compare::Compare::new(&before, &after, false, 1_000_000);
    assert!(walk.run().is_err());
}

#[test]
fn groups_nested_as_deep_as_the_model_allows_are_walked_and_deeper_are_refused() {
    let mut deep = ShapeBuilder::new(1).cell("PinX", "1");
    for id in 2..=199u32 {
        deep = ShapeBuilder::new(id).cell("PinX", "1").child(deep);
    }
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(deep)).build();
    assert_eq!(said(&before, &before.clone()), (vec![], false));
    let mut shallow = ShapeBuilder::new(1);
    for id in 2..=201u32 {
        shallow = ShapeBuilder::new(id).child(shallow);
    }
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(shallow)).build();
    assert!(matches!(compare(&before, &before.clone(), false), Outcome::Unchecked(_)), "past the depth of the model");
}

// ----------------------------------------------------------------------------------------------------------------------------- the security scan's five findings: a hostile file is walked in linear steps or refused

/// How many of a thing the hostile drawings hold: a hundred thousand, the figure a scan of the code for a loop that is not linear is judged by.
const HOSTILE: usize = 100_000;

/// The comparison of `before` with `after`, run to its end under `budget` steps: the outcome of [`compare::Compare::run`], the steps it took and the classes it counted.
fn run_within(before: &Document, after: &Document, budget: u64) -> (Result<(), meter::Stop>, u64, Vec<(Class, u64)>) {
    let mut walk = compare::Compare::new(before, after, false, budget);
    let done = walk.run();
    let counted = ALL.into_iter().map(|class| (class, walk.tally.count(class))).filter(|(_, n)| *n > 0).collect();
    (done, walk.steps(), counted)
}

/// A page of `shapes` empty shapes, the `ids` of which are given by `id`.
fn page_of(shapes: usize, id: impl Fn(usize) -> u32) -> Document {
    let mut page = PageBuilder::new().name("P");
    for at in 0..shapes {
        page = page.shape(ShapeBuilder::new(id(at)));
    }
    DocBuilder::new().page(1, page).build()
}

/// **Finding 2.** A hundred thousand shapes of one `ID`, paired against a hundred thousand of which one has another: the pairing was a scan from the first of the `ID` for the first not yet taken, for
/// each shape. It is a cursor for each `ID` now, and the steps are the shapes and no more.
#[test]
fn a_hundred_thousand_shapes_of_one_id_are_paired_in_linear_steps() {
    let before = page_of(HOSTILE, |_| 1);
    let after = page_of(HOSTILE, |at| if at == HOSTILE - 1 { 2 } else { 1 });
    let (done, steps, counted) = run_within(&before, &after, 40 * HOSTILE as u64);
    assert_eq!(done, Ok(()), "{steps} steps");
    assert_eq!(counted, vec![(Class::ShapeNumbers, 1)], "one shape has another number, and every other was paired in its turn");
    assert!(steps >= HOSTILE as u64 && steps <= 12 * HOSTILE as u64, "{steps} steps");
}

/// **Finding 3.** A hundred thousand custom properties, each looked for among the file's by a scan: a set now.
#[test]
fn a_hundred_thousand_custom_properties_are_matched_in_linear_steps() {
    let prop = |at: usize| CustomProp {
        format: Format::Vsdx,
        name: Arc::from(format!("name{at}")),
        value_type: Arc::from("lpwstr"),
        value: Arc::from("v"),
        raw: RawAttrs::new(),
    };
    let mut before = Document::new_blank();
    Arc::make_mut(&mut before.props).custom = (0..HOSTILE).map(prop).collect();
    // The file holds them in the other order, and one fewer.
    let mut after = before.clone();
    Arc::make_mut(&mut after.props).custom = (1..HOSTILE).rev().map(prop).collect();
    let (done, steps, counted) = run_within(&before, &after, 40 * HOSTILE as u64);
    assert_eq!(done, Ok(()), "{steps} steps");
    assert_eq!(counted, vec![(Class::Properties, 1)]);
    assert!(steps <= 12 * HOSTILE as u64, "{steps} steps");
}

/// **Finding 3, the themes.** Themes are matched by their bytes in a set.
#[test]
fn themes_are_matched_by_their_bytes_in_one_pass() {
    let theme = |at: usize| diacraft_doc::Theme {
        part: Arc::from(format!("/t{at}.xml")),
        bytes: Arc::from(at.to_le_bytes().as_slice()),
        ..Default::default()
    };
    let mut before = Document::new_blank();
    before.themes = (0..5_000).map(|at| Arc::new(theme(at))).collect();
    let mut after = before.clone();
    after.themes = (1..5_000).rev().map(|at| Arc::new(theme(at))).collect();
    let (done, steps, counted) = run_within(&before, &after, 40 * 5_000);
    assert_eq!(done, Ok(()), "{steps} steps");
    assert_eq!(counted, vec![(Class::StyleContent, 1)]);
}

/// **Finding 4.** A hundred thousand parts of the same bytes: each was looked for in a bucket by a scan for the first not yet given. A count of each kind in the bucket now.
#[test]
fn a_hundred_thousand_parts_of_the_same_bytes_are_matched_in_linear_steps() {
    let mut before = Document::new_blank();
    before.parts = (0..HOSTILE).map(|at| part(&format!("/p{at}.bin"), PartRole::Other, b"the same bytes")).collect();
    let mut after = before.clone();
    after.parts = (1..HOSTILE).map(|at| part(&format!("/q{at}.bin"), PartRole::Other, b"the same bytes")).collect();
    let (done, steps, counted) = run_within(&before, &after, 40 * HOSTILE as u64);
    assert_eq!(done, Ok(()), "{steps} steps");
    assert_eq!(counted, vec![(Class::KeptData, 1)], "one part of the same bytes is not in the file");
    assert!(steps <= 12 * HOSTILE as u64, "{steps} steps");
    // Macro projects of the same bytes are matched among macro projects, and the kinds are counted apart.
    let mut before = Document::new_blank();
    before.parts = (0..HOSTILE).map(|at| part(&format!("/m{at}.bin"), PartRole::MacroProject, b"same")).collect();
    let mut after = before.clone();
    after.parts =
        (0..HOSTILE).map(|at| part(&format!("/o{at}.bin"), if at < 10 { PartRole::Embedding } else { PartRole::MacroProject }, b"same")).collect();
    let (done, _, counted) = run_within(&before, &after, 40 * HOSTILE as u64);
    assert_eq!((done, counted), (Ok(()), vec![(Class::Macros, 10)]), "ten of the macro parts are objects in the file");
}

/// A document with `count` definitions of one name, each as `element` makes it.
fn defined(count: usize, element: impl Fn(usize) -> Arc<RawElement>) -> Document {
    let mut doc = Document::new_blank();
    doc.ext.make_mut().odf.definitions =
        (0..count).map(|at| Definition { family: DefFamily::Marker, name: Arc::from("m"), element: element(at), bytes: None }).collect();
    doc
}

/// **Finding 5.** A hundred thousand definitions of one name: each was looked for among the file's of that name by a scan. The one of its place is compared first now, and a scan of the group is charged
/// before it is made, so a faithful list is linear and a list that is not in its places is refused in words rather than walked.
#[test]
fn a_hundred_thousand_definitions_of_one_name_are_matched_in_linear_steps_or_refused() {
    let same = |_: usize| element("draw:marker", &[("draw:name", "m")]);
    let before = defined(HOSTILE, same);
    let after = defined(HOSTILE - 1, same);
    let (done, steps, counted) = run_within(&before, &after, 60 * HOSTILE as u64);
    assert_eq!(done, Ok(()), "{steps} steps");
    // (The one that has no place in the file is looked for among those that are, a scan of its name charged, and it is there: definitions that are one definition are one.)
    assert_eq!(counted, vec![]);
    // (One scan of the file's definitions, at the sixteen steps an element is worth, and the rest is one step for each.)
    assert!(steps <= 20 * HOSTILE as u64, "{steps} steps");
    // A hundred thousand that differ from the file's one by one: the scans are charged and the budget refuses them, in a few steps' time.
    let different = |at: usize| element("draw:marker", &[("draw:name", "m"), ("n", &at.to_string())]);
    let before = defined(HOSTILE, same);
    let after = defined(HOSTILE, different);
    let (done, steps, _) = run_within(&before, &after, compare::MAX_STEPS);
    assert_eq!(done, Err(meter::Stop), "{steps} steps");
    assert!(steps < 2 * compare::MAX_STEPS, "the walk stopped when the budget was gone: {steps} steps");
    assert!(matches!(compare(&before, &after, false), Outcome::Unchecked(_)), "and the save says it could not check it");
}

/// **Finding 1.** The nodes a shape kept, in another order than the file's, are matched one by one, and the product of the two lengths is charged before the matching is made, over all the shapes: two
/// hundred shapes of two thousand nodes each, each under the cap of one call, were eight hundred million comparisons that no budget saw.
#[test]
fn nodes_matched_one_by_one_are_charged_over_all_the_shapes_and_the_budget_stops_them() {
    let nodes = |order: &mut dyn Iterator<Item = usize>| {
        diacraft_doc::Passthrough::from_nodes(
            order.map(|at| diacraft_doc::Placed { after: 0, node: diacraft_doc::RawNode::Text(Arc::from(format!("node {at}"))) }).collect(),
        )
    };
    let (mut before, mut after) = (PageBuilder::new().name("P"), PageBuilder::new().name("P"));
    let (mut doc_before, mut doc_after) = (DocBuilder::new(), DocBuilder::new());
    let _ = (&mut doc_before, &mut doc_after);
    let mut shapes = (Vec::new(), Vec::new());
    for id in 1..=200u32 {
        let (mut b, mut a) = (Shape::new(id), Shape::new(id));
        b.passthrough = nodes(&mut (0..2_000));
        a.passthrough = nodes(&mut (0..2_000).rev());
        shapes.0.push(b);
        shapes.1.push(a);
    }
    for shape in shapes.0 {
        before = before.shape(ShapeBuilder::new(shape.id));
    }
    for shape in shapes.1 {
        after = after.shape(ShapeBuilder::new(shape.id));
    }
    let (mut before, mut after) = (DocBuilder::new().page(1, before).build(), DocBuilder::new().page(1, after).build());
    for (doc, reversed) in [(&mut before, false), (&mut after, true)] {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for at in 0..200 {
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            shape.passthrough = if reversed { nodes(&mut (0..2_000).rev()) } else { nodes(&mut (0..2_000)) };
        }
    }
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    assert_eq!(walk.run(), Err(meter::Stop), "{} steps", walk.steps());
    assert!(walk.steps() < 2 * compare::MAX_STEPS, "the walk stopped when the budget was gone: {} steps", walk.steps());
    assert!(matches!(compare(&before, &after, false), Outcome::Unchecked(_)));
    // In the same order they are the linear pass, and the file may hold more beside them.
    let mut same = after.clone();
    for at in 0..200 {
        let shape = Arc::make_mut(Arc::make_mut(same.pages.get_mut(0).unwrap()).shapes.get_mut(at).unwrap());
        shape.passthrough = nodes(&mut (0..2_000));
    }
    let (done, steps, counted) = run_within(&before, &same, 60 * 200 * 2_000);
    assert_eq!((done, counted), (Ok(()), vec![]), "{steps} steps");
    assert!(steps <= 200 * 2 * 2_000 + 200 * 20, "{steps} steps");
}

// ----------------------------------------------------------------------------------------------------------------------------- the security scan's sixth finding: the trace

/// A shape of `cells` cells that all hold the one text `text` (a model shares what its cells hold), on one page.
fn page_of_cells(cells: usize, text: &Value) -> Document {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    let shape = shape_mut(&mut doc, 1);
    let sheet = Arc::make_mut(&mut shape.sheet);
    for k in 0..cells {
        sheet.cells.insert(CellName::new(&format!("Cell{k}")), Cell::constant(text.clone()));
    }
    doc
}

/// A text of a megabyte that starts with `first`.
fn megabyte(first: char) -> Value {
    let mut text = String::with_capacity(1 << 20);
    text.push(first);
    text.extend(std::iter::repeat_n('x', (1 << 20) - 1));
    Value::from_text(&text)
}

/// The bytes of the trace a comparison made.
fn trace_bytes(walk: &compare::Compare) -> usize {
    walk.trace.iter().map(String::len).sum()
}

/// **Finding 6.** The places that differ were put in words with the whole of each value, for any comparison: ten thousand cells of a megabyte of text that all differ made a trace of sixty-four entries of
/// two megabytes each, with no charge. The trace is built only for [`explain`], and with it on, each value is cut and the whole is held to a budget.
#[test]
fn the_trace_is_built_only_when_it_is_asked_for() {
    let (before, after) = (page_of_cells(10_000, &megabyte('a')), page_of_cells(10_000, &megabyte('b')));
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    assert_eq!(walk.run(), Ok(()));
    assert_eq!(walk.tally.count(Class::ShapeAttributes), 1, "the cells differ, and are told once for the shape");
    assert_eq!(trace_bytes(&walk), 0, "no trace was asked for: none was built");
    assert!(walk.trace.is_empty() && walk.trace.capacity() == 0, "and none was allocated");
    // Asked for, it is built, each value cut and the whole within its budget however many places differ and however long the values.
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    walk.trace_on = true;
    assert_eq!(walk.run(), Ok(()));
    assert!(!walk.trace.is_empty(), "it was asked for");
    assert!(trace_bytes(&walk) <= compare::TRACE_BYTES, "{} bytes", trace_bytes(&walk));
    assert!(
        walk.trace.iter().all(|line| line.len() < 1_024),
        "each entry holds cut values: {}",
        walk.trace.iter().map(String::len).max().unwrap_or(0)
    );
    assert!(walk.trace.iter().all(|line| line.contains('…')), "and says that it cut them");
    // The public way to ask, `explain`, is that comparison.
    let said = explain(&before, &after);
    assert!(!said.is_empty() && said.iter().map(String::len).sum::<usize>() <= compare::TRACE_BYTES);
}

/// **Item 11: every class is explained.** `explain` traced cell values and formulas only, so a card that named a class by any other way (a shape gone, a picture, a held value, a property, a part)
/// could not be followed to the place it counted. Each class planted alone is now said in the trace, under its id, where it was counted.
#[test]
fn every_class_is_said_in_the_trace_where_it_was_counted() {
    type Damage = Box<dyn Fn(&mut Document)>;
    let cases: Vec<(Class, Damage)> = vec![
        (Class::Pages, Box::new(|doc| page_mut(doc, 1).name = Arc::from("Other"))),
        (Class::Shapes, Box::new(|doc| drop(page_mut(doc, 0).shapes.remove(2)))),
        (Class::ShapeNumbers, Box::new(|doc| shape_mut(doc, 3).id = 33)),
        (Class::Geometry, Box::new(|doc| set_cell(shape_mut(doc, 1), "PinX", "3"))),
        (Class::Text, Box::new(|doc| set_text_chars(shape_mut(doc, 1), "Hullo "))),
        (Class::TextFormatting, Box::new(|doc| set_row_cell(shape_mut(doc, 1), "Character", RowKey::Ix(0), "Size", "0.3"))),
        (Class::Pictures, Box::new(|doc| shape_mut(doc, 4).foreign = None)),
        (Class::Objects, Box::new(|doc| drop_parts(doc, PartRole::Embedding))),
        (Class::Macros, Box::new(|doc| drop_parts(doc, PartRole::MacroProject))),
        (Class::Fonts, Box::new(|doc| drop(Arc::make_mut(&mut doc.fonts).pop()))),
        (Class::LineEnds, Box::new(|doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Marker))),
        (Class::Gradients, Box::new(|doc| drop_section(shape_mut(doc, 1), "FillGradient"))),
        (Class::StyleContent, Box::new(|doc| doc.ext.make_mut().odf.definitions.retain(|d| d.family != DefFamily::Hatch))),
        (Class::StyleSheets, Box::new(|doc| drop(doc.styles.remove(1)))),
        (Class::Masters, Box::new(|doc| shape_mut(doc, 6).master = Some(8))),
        (Class::Layers, Box::new(|doc| doc.ext.make_mut().odf.layers.truncate(1))),
        (Class::Connections, Box::new(|doc| page_mut(doc, 0).connects = Default::default())),
        (
            Class::Formulas,
            Box::new(|doc| {
                Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.insert(CellName::new("LocPinX"), Cell::constant(Value::from_text("1")));
            }),
        ),
        (Class::Hyperlinks, Box::new(|doc| drop_section(shape_mut(doc, 1), "Hyperlink"))),
        (Class::Fields, Box::new(|doc| drop_section(shape_mut(doc, 1), "Field"))),
        (Class::UserCells, Box::new(|doc| shape_mut(doc, 1).data1 = None)),
        (Class::ShapeAttributes, Box::new(|doc| set_cell(shape_mut(doc, 1), "Mystery", "8"))),
        (Class::Properties, Box::new(|doc| Arc::make_mut(&mut doc.props).creator = None)),
        (Class::KeptData, Box::new(|doc| drop_parts(doc, PartRole::CustomXml))),
    ];
    assert_eq!(cases.len() + 1, ALL.len(), "a case for every class, and the comments below");
    // The comments, of a drawing that has some.
    let with = commented();
    let without = damaged(&with, |doc| with_comments(doc, |list, _| list.truncate(1)));
    assert!(explain(&with, &without).iter().any(|line| line.contains("comments: ")), "{:?}", explain(&with, &without));
    let before = base();
    for (class, damage) in cases {
        let after = damaged(&before, |doc| damage(doc));
        assert!(said(&before, &after).0.iter().any(|(c, _)| *c == class), "{class:?} is named");
        let trace = explain(&before, &after);
        let id = format!("{}: ", class.id());
        assert!(trace.iter().any(|line| line.contains(&id)), "{class:?} is said in the trace: {trace:?}");
    }
    // Where it was counted: a shape's class names the shape, a page's the page.
    let after = damaged(&before, |doc| set_cell(shape_mut(doc, 1), "PinX", "3"));
    assert!(explain(&before, &after).iter().any(|line| line.contains("shape 1") && line.contains("geometry: ") && line.contains("PinX")));
    let after = damaged(&before, |doc| page_mut(doc, 0).connects = Default::default());
    assert!(explain(&before, &after).iter().any(|line| line.starts_with("page 1: connections: ")), "{:?}", explain(&before, &after));
}

/// **Item 7: a glued end's formula is the glue's.** The reader of each format makes a glued end's `BeginX`, `BeginY`, `EndX` and `EndY` from the glue, so they are no formulas of the person's:
/// 179 `.odg` files with no formula of their own were told "formulas are lost". The end is judged as the connection it is: kept, nothing is said; lost, it is one connection, and no formula.
#[test]
fn a_glued_end_s_formula_is_judged_as_its_glue() {
    let mut before = base();
    {
        let rect = shape_mut(&mut before, 1);
        let sheet = Arc::make_mut(&mut rect.sheet);
        for cell in ["BeginX", "BeginY"] {
            sheet.cells.insert(
                CellName::new(cell),
                Cell::with_formula("PAR(PNT(Sheet.6!Connections.X1,Sheet.6!Connections.Y1))", Some(Value::from_text("1"))),
            );
        }
        // An end that is not glued keeps the formula a person wrote.
        sheet.cells.insert(CellName::new("EndX"), Cell::with_formula("BeginX+2", Some(Value::from_text("3"))));
    }
    // The glue is in the file, and the ends are written as values (another reader's form): nothing is lost.
    let as_values = damaged(&before, |doc| {
        let sheet = Arc::make_mut(&mut shape_mut(doc, 1).sheet);
        for cell in ["BeginX", "BeginY"] {
            sheet.cells.insert(CellName::new(cell), Cell::constant(Value::from_text("1")));
        }
    });
    assert_eq!(said(&before, &as_values).0, vec![]);
    // The glue is gone with them: one connection, and no formula besides it.
    let unglued = damaged(&as_values, |doc| page_mut(doc, 0).connects = Default::default());
    assert_eq!(said(&before, &unglued).0, vec![(Class::Connections, 1)]);
    // The end that was not glued is still a formula that is lost.
    let other = damaged(&before, |doc| {
        Arc::make_mut(&mut shape_mut(doc, 1).sheet).cells.insert(CellName::new("EndX"), Cell::constant(Value::from_text("3")));
    });
    assert_eq!(said(&before, &other).0, vec![(Class::Formulas, 1)]);
}

/// **Item 7: a connection is the same between the shapes paired with its own.** A file that numbers its shapes anew holds the drawing's connections under the new numbers: that is told once for
/// each shape ("has another number"), and no connection differs.
#[test]
fn a_connection_between_shapes_the_file_numbered_anew_is_the_same_connection() {
    let before = base();
    let after = damaged(&before, |doc| {
        shape_mut(doc, 1).id = 11;
        shape_mut(doc, 6).id = 16;
        let page = page_mut(doc, 0);
        page.connects = Default::default();
        page.connects.push(Connect {
            from_sheet: 11,
            from_cell: Some(Arc::from("BeginX")),
            to_sheet: 16,
            to_cell: Some(Arc::from("PinX")),
            ..Connect::default()
        });
    });
    assert_eq!(said(&before, &after).0, vec![(Class::ShapeNumbers, 2)]);
    // Joined to another shape, it is one that differs.
    let elsewhere = damaged(&after, |doc| {
        let page = page_mut(doc, 0);
        page.connects = Default::default();
        page.connects.push(Connect {
            from_sheet: 11,
            from_cell: Some(Arc::from("BeginX")),
            to_sheet: 4,
            to_cell: Some(Arc::from("PinX")),
            ..Connect::default()
        });
    });
    assert_eq!(said(&before, &elsewhere).0, vec![(Class::ShapeNumbers, 2), (Class::Connections, 1)]);
}

/// A page of `shapes` copies of one shape (one allocation, as a file of one shape stamped many times reads) holding `cells` cells of the value 0, and the same page whose shapes hold none.
fn stamped(shapes: usize, cells: u32) -> (Document, Document) {
    let mut shape = ShapeBuilder::new(1).kind(ShapeType::Shape);
    for k in 0..cells {
        shape = shape.cell(&format!("Cell{k}"), "0");
    }
    let before = DocBuilder::new().page(1, PageBuilder::new().name("P").shape(shape)).build();
    let one = Arc::clone(before.pages.get(0).unwrap().shapes.get(0).unwrap());
    let bare = Arc::new(Shape { sheet: Arc::new(diacraft_doc::Sheet::default()), ..Shape::clone(&one) });
    let page_of = |shape: &Arc<Shape>| {
        let mut doc = before.clone();
        let page = page_mut(&mut doc, 0);
        page.shapes = SharedVec::from_vec((0..shapes).map(|_| Arc::clone(shape)).collect());
        doc
    };
    (page_of(&one), page_of(&bare))
}

/// **Item 10: a large drawing is compared in proportion to its size** (the budget follows the drawing). Two saves of the sample files were "too big to compare in one step": the comparison had
/// one budget, sixty million steps, for any drawing, and a walk of every cell of a large one is past it though it is no dearer for each cell than a small drawing's. The budget is now that, or
/// [`compare::budget`]'s figure for what the drawings hold, whichever is more. A page of 36,000 shapes of 100 cells each, which the file gives by its style sheets (each cell a step and a lookup), is
/// checked; a hostile scan (every definition against every other) is still refused, in words. Red on `e6020b3b`: the page is not checked.
#[test]
fn a_large_drawing_is_compared_within_a_budget_that_follows_its_size() {
    let (before, after) = stamped(36_000, 100);
    let steps = steps_of(&before, &after);
    assert!(steps > compare::MAX_STEPS, "the walk is past the fixed budget: {steps} steps");
    assert!(compare::budget(&before, &after) >= steps, "and within the one that follows the drawing");
    match compare(&before, &after, false) {
        Outcome::Checked(checked) => {
            assert!(checked.classes.is_empty() && checked.as_values, "the zeros are what nothing gives the cells: {checked:?}")
        }
        other => panic!("checked: {other:?}"),
    }
    // A small drawing keeps the fixed figure.
    assert_eq!(compare::budget(&base(), &base()), compare::MAX_STEPS);
    // A hostile one still stops: a hundred thousand definitions of one name, none the same, are a scan of the hundred thousand for each.
    let before = defined(HOSTILE, |at| element("draw:marker", &[("draw:name", "m"), ("n", &at.to_string())]));
    let after = defined(HOSTILE, |at| element("draw:marker", &[("draw:name", "m"), ("n", &(at + HOSTILE).to_string())]));
    assert!(compare::budget(&before, &after) < (HOSTILE as u64) * (HOSTILE as u64), "the budget is no product of the lists");
    assert_eq!(compare(&before, &after, false), Outcome::Unchecked("the drawing is too big to compare with the file in one step".to_owned()));
}

/// **A mark or a drawing object a text holds where it stands is told when it does not come back** (items 2 to 4): the check compared the characters of a text paragraph by
/// paragraph and passed over what a text holds whole between them, so a picture anchored as a character that the `.odg` model writer took apart was not named. It is a picture now when it shows one,
/// and kept data otherwise.
#[test]
fn a_picture_or_a_mark_a_text_holds_where_it_stands_is_told_when_it_is_not_in_the_file() {
    let image = RawElement { name: Arc::from("draw:image"), attrs: RawAttrs::from_pairs([("xlink:href", "Pictures/a.png")]), children: Vec::new() };
    let frame = diacraft_doc::RawNode::Element(RawElement {
        name: Arc::from("draw:frame"),
        attrs: RawAttrs::new(),
        children: vec![diacraft_doc::RawNode::Element(image)],
    });
    let mark = diacraft_doc::RawNode::Element(RawElement {
        name: Arc::from("text:bookmark"),
        attrs: RawAttrs::from_pairs([("text:name", "b")]),
        children: Vec::new(),
    });
    let held = |parts: Vec<TextPart>| damaged(&base(), |doc| set_text_parts(shape_mut(doc, 3), parts));
    let before = held(vec![
        TextPart::Chars(Arc::from("in")),
        TextPart::OdfRaw(Box::new(frame.clone())),
        TextPart::Chars(Arc::from("ner")),
        TextPart::OdfRaw(Box::new(mark.clone())),
    ]);
    assert_eq!(said(&before, &before.clone()).0, vec![], "both in the file");
    let no_frame = held(vec![TextPart::Chars(Arc::from("in")), TextPart::Chars(Arc::from("ner")), TextPart::OdfRaw(Box::new(mark))]);
    assert_eq!(said(&before, &no_frame).0, vec![(Class::Pictures, 1)], "the picture is gone");
    let no_mark = held(vec![TextPart::Chars(Arc::from("in")), TextPart::OdfRaw(Box::new(frame)), TextPart::Chars(Arc::from("ner"))]);
    assert_eq!(said(&before, &no_mark).0, vec![(Class::KeptData, 1)], "the mark is gone");
}

/// A tree of kept markup `depth` elements deep, each holding `wide` leaves beside the next level, the deepest leaf's text `last`: about `depth * (wide + 1)` nodes.
fn kept_tree(depth: usize, wide: usize, last: &str) -> diacraft_doc::RawNode {
    let leaf = |text: &str| diacraft_doc::RawNode::Text(Arc::from(text));
    let mut node = diacraft_doc::RawNode::Element(RawElement { name: Arc::from("draw:frame"), attrs: RawAttrs::new(), children: vec![leaf(last)] });
    for _ in 0..depth {
        let mut children: Vec<diacraft_doc::RawNode> = (0..wide).map(|_| leaf("x")).collect();
        children.push(node);
        node = diacraft_doc::RawNode::Element(RawElement { name: Arc::from("draw:g"), attrs: RawAttrs::new(), children });
    }
    node
}

/// **The security scan's finding on this task**: the marks of a text were matched one comparison a step, and a comparison of two trees of kept markup walks both trees, so 300 marks of a thousand
/// nodes each that differ at their last leaf from 300 of the file's walked ninety million nodes for ninety thousand steps. Each comparison is charged now by the smaller tree, before it is made; the
/// walk is counted, and past its budget it stops, in words. Red on `701faa02`: about 90,000 steps for the walk, and the check finished.
#[test]
fn marks_of_large_trees_compared_many_times_are_charged_by_their_size() {
    const MARKS: usize = 300;
    let text_of = |last: &str| {
        (0..MARKS).flat_map(|_| [TextPart::Chars(Arc::from("a")), TextPart::OdfRaw(Box::new(kept_tree(50, 20, last)))]).collect::<Vec<_>>()
    };
    let before = damaged(&base(), |doc| set_text_parts(shape_mut(doc, 3), text_of("one")));
    let after = damaged(&base(), |doc| set_text_parts(shape_mut(doc, 3), text_of("other")));
    // Every mark of the drawing is compared with every one of the file's, each to its last leaf.
    let (done, steps, counted) = run_within(&before, &after, u64::MAX);
    assert_eq!(done, Ok(()));
    assert!(steps >= (MARKS * MARKS * 50 * 21) as u64, "each comparison is charged by the nodes it walks: {steps} steps");
    assert!(counted.contains(&(Class::KeptData, MARKS as u64)), "{counted:?}");
    // Within the budget a check of drawings of this size is given, it stops, and says so.
    assert_eq!(compare(&before, &after, false), Outcome::Unchecked("the drawing is too big to compare with the file in one step".to_owned()));
}

/// **The security scan's second finding on this task**: [`compare::node_cost`] charged a text node by its bytes and a comment or a processing instruction by none, so marks that differ only in the end
/// of a megabyte comment (or instruction) were compared for a step each. Their bytes are charged now as a text's are. Red on `a3be0794`: under three steps a comparison.
#[test]
fn marks_that_differ_in_a_large_comment_or_instruction_are_charged_by_their_bytes() {
    const MARKS: usize = 40;
    const BYTES: usize = 1 << 20;
    let long = |last: char| -> Arc<str> {
        let mut text = "c".repeat(BYTES - 1);
        text.push(last);
        Arc::from(text)
    };
    let mark_with = |node: diacraft_doc::RawNode| {
        diacraft_doc::RawNode::Element(RawElement { name: Arc::from("text:bookmark"), attrs: RawAttrs::new(), children: vec![node] })
    };
    type Make = Box<dyn Fn(Arc<str>) -> diacraft_doc::RawNode>;
    let kinds: [(&str, Make); 2] = [
        ("comment", Box::new(diacraft_doc::RawNode::Comment)),
        ("instruction", Box::new(|data: Arc<str>| diacraft_doc::RawNode::Pi { target: Arc::from("t"), data })),
    ];
    for (kind, make) in kinds {
        let (one, other) = (mark_with(make(long('a'))), mark_with(make(long('b'))));
        let text_of = |mark: &diacraft_doc::RawNode| (0..MARKS).map(|_| TextPart::OdfRaw(Box::new(mark.clone()))).collect::<Vec<_>>();
        let before = damaged(&base(), |doc| set_text_parts(shape_mut(doc, 3), text_of(&one)));
        let after = damaged(&base(), |doc| set_text_parts(shape_mut(doc, 3), text_of(&other)));
        let (done, steps, counted) = run_within(&before, &after, u64::MAX);
        assert_eq!(done, Ok(()), "{kind}");
        assert!(steps >= (MARKS * MARKS * (BYTES >> 6)) as u64, "{kind}: each comparison is charged by the bytes it walks: {steps} steps");
        assert!(counted.contains(&(Class::KeptData, MARKS as u64)), "{kind}: {counted:?}");
        // The check of a save is within the budget drawings of this size are given (the charge is counted against it), and names the marks.
        assert!(steps <= compare::budget(&before, &after), "{kind}: {steps} steps");
        match compare(&before, &after, false) {
            Outcome::Checked(checked) => assert_eq!(checked.classes, vec![Differs { class: Class::KeptData, count: MARKS as u64 }], "{kind}"),
            other => panic!("{kind}: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------- comments

/// The drawing with three comments: Ada on the first shape of page 1, Bob's reply to it, and Ada's on page 2.
fn commented() -> Document {
    let mut doc = base();
    let ada = doc.comments.author_for("Ada", "AL").unwrap();
    let bob = doc.comments.author_for("Bob", "BO").unwrap();
    let new = |parent, page, shape, author, text: &str, date: &str| diacraft_doc::NewComment {
        parent,
        page,
        shape,
        author,
        text: Arc::from(text),
        date: Arc::from(date),
    };
    let root = doc.comments.add(new(None, 1, Some(1), ada, "on the rectangle", "2031-03-07T20:05:09Z")).unwrap();
    doc.comments.add(new(Some(root), 1, None, bob, "an answer\nin two lines", "2031-03-08T20:05:09Z")).unwrap();
    doc.comments.add(new(None, 2, None, ada, "on the second page", "2031-03-09T20:05:09Z")).unwrap();
    doc
}

/// The comments of a drawing changed by `edit`, made again through the door a file enters by.
fn with_comments(doc: &mut Document, edit: impl FnOnce(&mut Vec<diacraft_doc::Comment>, &mut Vec<diacraft_doc::CommentAuthor>)) {
    let mut list: Vec<diacraft_doc::Comment> = doc.comments.iter().map(|c| (**c).clone()).collect();
    let mut authors: Vec<diacraft_doc::CommentAuthor> = doc.comments.authors().map(|a| (**a).clone()).collect();
    edit(&mut list, &mut authors);
    doc.comments = diacraft_doc::Comments::from_parts(doc.comments.show_tags, RawAttrs::new(), authors, list).unwrap();
}

fn comment_classes(damage: impl FnOnce(&mut Vec<diacraft_doc::Comment>, &mut Vec<diacraft_doc::CommentAuthor>)) -> Vec<(Class, u64)> {
    let before = commented();
    let mut after = before.clone();
    with_comments(&mut after, damage);
    said(&before, &after).0
}

#[test]
fn comments_that_come_back_as_they_were_are_no_difference() {
    let before = commented();
    assert_eq!(said(&before, &before.clone()).0, vec![]);
    // The file holds more: a comment of its own is no loss.
    let mut more = before.clone();
    with_comments(&mut more, |list, _| {
        let mut extra = list[0].clone();
        extra.id = 99;
        list.push(extra);
    });
    assert_eq!(said(&before, &more).0, vec![]);
}

#[test]
fn each_part_of_a_comment_that_does_not_come_back_is_told_in_the_class_comments() {
    let one = |what: &str, damage: &dyn Fn(&mut Vec<diacraft_doc::Comment>, &mut Vec<diacraft_doc::CommentAuthor>)| {
        let found = comment_classes(|l, a| damage(l, a));
        assert_eq!(found, vec![(Class::Comments, 1)], "{what}");
    };
    one("a comment that is gone", &|l, _| {
        l.remove(2);
    });
    one("another text", &|l, _| l[0].text = Arc::from("changed"));
    one("a line lost", &|l, _| l[1].text = Arc::from("an answer in two lines"));
    one("another date", &|l, _| l[0].date = Arc::from("2031-03-07T20:05:10Z"));
    one("an edit date lost", &|l, _| {
        l[0].edit_date = Some(Arc::from("2031-04-01T00:00:00Z"));
    });
    one("a Done mark", &|l, _| l[0].done = true);
    one("a reply that is a comment of its own", &|l, _| l[1].parent = None);
    one("another author", &|_, a| a[1].name = Arc::from("Eve"));
    one("other initials", &|_, a| a[1].initials = Arc::from("XX"));
    one("a comment on another page", &|l, _| l[2].page = 1);
    one("a comment that lost its shape", &|l, _| l[0].shape = None);
    one("a comment on another shape", &|l, _| l[0].shape = Some(2));
}

#[test]
fn a_comment_is_on_the_same_shape_when_it_is_at_the_same_place_among_the_page_s_shapes_whatever_the_numbers() {
    // The format numbered the pages and the shapes again: page 1 is 10, the rectangle (the first shape of its page) is 40 and the group, second in its page, is 41. A comment is no
    // worse for it. (The classes `pages` and `shape-numbers` say the numbers changed.)
    let before = commented();
    let mut after = before.clone();
    {
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        page.id = 10;
        let rect = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        rect.id = 40;
    }
    with_comments(&mut after, |l, _| {
        l[0].page = 10;
        l[1].page = 10;
        l[0].shape = Some(40);
        l[1].shape = Some(40);
    });
    let (classes, _) = said(&before, &after);
    assert!(!classes.iter().any(|(c, _)| *c == Class::Comments), "{classes:?}");
}

#[test]
fn a_comment_on_a_shape_the_drawing_no_longer_has_is_the_page_s() {
    let mut before = commented();
    with_comments(&mut before, |l, _| {
        l[0].shape = Some(777);
        l[1].shape = Some(777);
    });
    let mut after = before.clone();
    with_comments(&mut after, |l, _| {
        l[0].shape = None;
        l[1].shape = None;
    });
    assert_eq!(said(&before, &after).0, vec![]);
}

#[test]
fn the_comparison_of_comments_is_charged_in_steps_and_stops_when_they_are_gone() {
    let before = commented();
    let after = before.clone();
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    assert_eq!(walk.run(), Ok(()));
    let cheap = walk.steps();
    // Ten thousand comments on shapes cost steps in proportion to them, and a budget of a few hundred stops the walk.
    let mut many = base();
    let ada = many.comments.author_for("Ada", "AL").unwrap();
    for k in 0..10_000 {
        many.comments
            .add(diacraft_doc::NewComment { parent: None, page: 1, shape: Some(1 + k % 6), author: ada, text: Arc::from("x"), date: Arc::from("d") })
            .unwrap();
    }
    let same = many.clone();
    let mut walk = compare::Compare::new(&many, &same, false, compare::MAX_STEPS);
    assert_eq!(walk.run(), Ok(()));
    let steps = walk.steps();
    assert!(steps > cheap && steps < cheap + 10_000 * 4 + 5_000, "{steps} steps for 10,000 comments ({cheap} for a few)");
    let mut walk = compare::Compare::new(&many, &same, false, 300);
    assert!(walk.run().is_err(), "a budget of 300 steps does not reach the end of the comments");
}

// ---------------------------------------------------------------------------------------------------------------- the runs of a paragraph

/// A plain `text:span` of the characters `from` to `to`, naming the style `T1`.
fn plain_run(from: u32, to: u32) -> diacraft_doc::OdfRun {
    diacraft_doc::OdfRun { from, to, style: diacraft_doc::OdfTextStyle { name: Some(Arc::from("T1")), ..Default::default() }, element: None }
}

/// A hyperlink round the characters `from` to `to`.
fn link_run(from: u32, to: u32) -> diacraft_doc::OdfRun {
    diacraft_doc::OdfRun {
        from,
        to,
        style: Default::default(),
        element: Some(element("text:a", &[("xlink:href", "https://example.org/notes"), ("xlink:type", "simple")])),
    }
}

/// A plain span of the characters `from` to `to` that holds the value `value` of a style no cell says.
fn held_run(from: u32, to: u32, value: &str) -> diacraft_doc::OdfRun {
    let held = diacraft_doc::HeldStyleValue {
        group: diacraft_doc::TextGroup::Style,
        stamp: None,
        attrs: RawAttrs::from_pairs([("fo:font-weight", value)]),
        kept: diacraft_doc::Passthrough::new(),
    };
    diacraft_doc::OdfRun { style: diacraft_doc::OdfTextStyle { held: vec![held], ..Default::default() }, ..plain_run(from, to) }
}

/// The base drawing with `runs` as the runs of the one paragraph the file held for the text of shape 3.
fn with_runs(runs: Vec<diacraft_doc::OdfRun>) -> Document {
    let mut doc = base();
    let shape = shape_mut(&mut doc, 3);
    let mut text = (**shape.text.as_ref().unwrap()).clone();
    text.ext.make_mut().odf.paragraphs = vec![diacraft_doc::OdfParagraph { runs, ..Default::default() }];
    shape.text = Some(Arc::new(text));
    doc
}

fn runs_said(before: Vec<diacraft_doc::OdfRun>, after: Vec<diacraft_doc::OdfRun>) -> Vec<(Class, u64)> {
    said(&with_runs(before), &with_runs(after)).0
}

/// **Runs are told by the characters they cover, not by where one ends and the next begins**: three neighbouring runs of one style, which the writer joins into one, are the same text
/// and formatting (the check counted the runs, and said the formatting differed).
#[test]
fn neighbouring_runs_of_one_style_joined_into_one_name_nothing() {
    assert_eq!(runs_said(vec![plain_run(0, 2), plain_run(2, 4), plain_run(4, 5)], vec![plain_run(0, 5)]), vec![]);
    assert_eq!(runs_said(vec![plain_run(0, 5)], vec![plain_run(0, 2), plain_run(2, 5)]), vec![], "and the other way round");
    // The held values of a style are the same over the same characters, whichever way the runs that hold them begin and end.
    assert_eq!(runs_said(vec![held_run(0, 2, "bold"), held_run(2, 5, "bold")], vec![held_run(0, 5, "bold")]), vec![]);
    assert_eq!(runs_said(vec![held_run(0, 5, "bold")], vec![held_run(0, 3, "bold"), held_run(3, 5, "bold")]), vec![]);
}

/// **A real change is still named**: a held value of a run's style that is not over the same characters in the file, or is another.
#[test]
fn a_held_value_that_does_not_come_back_over_its_characters_is_named() {
    let formatting = vec![(Class::TextFormatting, 1)];
    assert_eq!(runs_said(vec![held_run(0, 5, "bold")], vec![held_run(0, 3, "bold")]), formatting, "over fewer characters");
    assert_eq!(
        runs_said(vec![held_run(0, 5, "bold")], vec![held_run(0, 2, "bold"), held_run(2, 5, "normal")]),
        formatting,
        "another value over some"
    );
    assert_eq!(runs_said(vec![held_run(0, 5, "bold")], vec![plain_run(0, 5)]), formatting, "none in the file");
    assert_eq!(runs_said(vec![held_run(2, 4, "bold")], vec![held_run(0, 5, "bold")]), vec![], "more characters than the drawing's");
}

/// A hyperlink the file lacks is counted as a hyperlink, and no other class; a plain run that holds no value of its own is no difference to lose.
#[test]
fn a_hyperlink_the_file_lacks_is_a_hyperlink_that_differs() {
    let plain = vec![plain_run(0, 5)];
    let linked = vec![plain_run(0, 5), link_run(1, 3)];
    assert_eq!(runs_said(linked.clone(), plain.clone()), vec![(Class::Hyperlinks, 1)]);
    assert_eq!(runs_said(vec![plain_run(0, 5), link_run(0, 2), link_run(3, 5)], plain), vec![(Class::Hyperlinks, 2)], "each link");
    assert_eq!(runs_said(linked.clone(), linked.clone()), vec![], "a link that comes back");
    assert_eq!(runs_said(linked.clone(), vec![plain_run(0, 2), plain_run(2, 5), link_run(1, 3)]), vec![], "beside runs the writer split or joined");
    // A link over other characters is a link that is not there, and one that is there beside it.
    assert_eq!(runs_said(linked, vec![plain_run(0, 5), link_run(2, 4)]), vec![(Class::TextFormatting, 1), (Class::Hyperlinks, 1)]);
    // An element of the file's that the drawing has none for: the formatting of the text.
    assert_eq!(runs_said(vec![plain_run(0, 5)], vec![plain_run(0, 5), link_run(1, 3)]), vec![(Class::TextFormatting, 1)]);
}

/// The runs' walk is charged: a paragraph of many runs costs steps in proportion, and a small budget stops it.
#[test]
fn the_comparison_of_a_paragraphs_runs_is_charged_in_steps() {
    let runs = |n: u32| (0..n).map(|k| held_run(k, k + 1, "bold")).collect::<Vec<_>>();
    let (small, large) = (with_runs(runs(4)), with_runs(runs(400)));
    let steps = |doc: &Document| {
        let mut walk = compare::Compare::new(doc, doc, false, compare::MAX_STEPS);
        assert_eq!(walk.run(), Ok(()));
        walk.steps()
    };
    assert!(steps(&large) > steps(&small) + 400, "{} steps for 400 runs, {} for 4", steps(&large), steps(&small));
    let mut walk = compare::Compare::new(&large, &large, false, 600);
    assert!(walk.run().is_err(), "a budget of 600 steps does not reach the end of 400 runs that hold values");
}

/// A plain span of the characters `from` to `to` that holds `n` values of a style no cell says, each with an attribute of its own named by `tag` (a value of one side matches none of the other's).
fn run_with_many_values(from: u32, to: u32, n: usize, tag: &str) -> diacraft_doc::OdfRun {
    let held = (0..n)
        .map(|k| diacraft_doc::HeldStyleValue {
            group: diacraft_doc::TextGroup::Style,
            stamp: None,
            attrs: RawAttrs::from_pairs([("fo:font-weight", format!("{tag}{k}").as_str())]),
            kept: diacraft_doc::Passthrough::new(),
        })
        .collect();
    diacraft_doc::OdfRun { style: diacraft_doc::OdfTextStyle { held, ..Default::default() }, ..plain_run(from, to) }
}

/// **The comparison of the held values of a paragraph's runs is charged before it is made** (the security scan's finding on the first commit): each stretch compared every value the
/// runs over it hold with every value of the file's, uncharged, so a paragraph whose runs hold thousands of values each was a walk of millions of comparisons past the budget. A run of 3,000
/// values against a run of 3,000 others (none of one is the other's) is 9 million comparisons: a budget of 100,000 steps stops the walk (the check says it did not finish, in words), and the walk
/// that has the room finds the values that do not come back.
#[test]
fn the_comparison_of_the_values_runs_hold_is_charged_before_it_is_made() {
    let before = with_runs(vec![run_with_many_values(0, 5, 3_000, "a")]);
    let after = with_runs(vec![run_with_many_values(0, 5, 3_000, "b")]);
    let mut walk = compare::Compare::new(&before, &after, false, 100_000);
    assert!(walk.run().is_err(), "a budget of 100,000 steps does not reach the end of 9 million comparisons");
    // The same paragraph with the room it asks for: told, as formatting that differs.
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS.max(50_000_000));
    assert_eq!(walk.run(), Ok(()));
    assert_eq!(walk.tally.count(Class::TextFormatting), 1);
    // Many stretches over runs that each hold many values: each stretch is charged for its own.
    let many = |tag: &str| (0..300u32).map(|k| run_with_many_values(k, k + 2, 40, tag)).collect::<Vec<_>>();
    // (The same values on both sides, so that no stretch ends the walk early by finding one that does not come back.)
    let (before, after) = (with_runs(many("a")), with_runs(many("a")));
    let mut walk = compare::Compare::new(&before, &after, false, 100_000);
    assert!(walk.run().is_err(), "300 stretches of 80 values against 80 each are two million steps");
    let mut walk = compare::Compare::new(&before, &after, false, compare::MAX_STEPS);
    assert_eq!(walk.run(), Ok(()), "with the room they are found the same");
    assert_eq!(walk.tally.count(Class::TextFormatting), 0);
}
