//! `svg:title` and `svg:desc` of a shape are the rows `visAltText` and `visAltDescription` of its `User` section, both ways.
//! **Reading** gives the rows from the elements, character for character; **a save into the file** keeps an element as it was while its row is unchanged and writes, replaces or
//! takes out the element when it changes, in the place the schema gives it; **a new shape** is written with the elements its texts need, the texts its master gives it included.
//!
//! Every read-back goes through the real reader.

use std::sync::Arc;

use diacraft_doc::alt_text::{self, DESCRIPTION_ROW, TITLE_ROW, standard_row};
use diacraft_doc::{Document, Master, Owner, Section, ShapeList, ShapeRef, SheetRef, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::model_write::{blank, save};
use crate::write_core::{open, part, real, shape_mut};
use crate::write_kept_dropped::id_of;
use crate::write_new::{push_shape, rectangle};

/// The title and the description of the `n`th top-level shape of the first page, as the document says them.
fn texts(doc: &Document, n: u32) -> (String, String) {
    let page = doc.pages.get(0).unwrap().id;
    let alt = alt_text::read_in(doc, &SheetRef::Shape(ShapeRef::top(Owner::Page(page), n)));
    (alt.title.text.to_owned(), alt.description.text.to_owned())
}

/// A rectangle named `name`, with a text, a title and a description as given.
fn rect(name: &str, extra: &str) -> String {
    format!(
        "\n    <draw:rect draw:name=\"{name}\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{extra}<text:p>Hello</text:p></draw:rect>"
    )
}

/// Gives the shape these rows of its own `User` section (the model of an edit: a command writes them).
fn give(doc: &mut Document, id: u32, rows: &[(&str, &str)]) {
    let shape = shape_mut(doc, 0, id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.sections.retain(|s| s.name.as_str() != "User");
    let mut user = Section::new("User", None);
    for (name, text) in rows {
        user.rows.push(Arc::new(standard_row(name, text)));
    }
    sheet.sections.push(Arc::new(user));
}

// ---------------------------------------------------------------------------------------------------------------------------------- reading

#[test]
fn a_custom_shapes_title_and_description_are_its_rows() {
    let body = format!(
        "\n    <draw:custom-shape draw:name=\"p\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><svg:title>Pump</svg:title><svg:desc>Feeds the boiler</svg:desc><text:p/>{}</draw:custom-shape>",
        "<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/>"
    );
    let (doc, _) = Drawing::a4().page(&body).read();
    assert_eq!(texts(&doc, 0), ("Pump".to_owned(), "Feeds the boiler".to_owned()), "Not the other way round");
    // The rows are the form the files hold: a string constant of unit STR and an empty prompt.
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let user = shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap();
    let rows: Vec<(&str, Option<&str>)> = user.rows.iter().map(|r| (r.name.as_deref().unwrap(), alt_text::standard_text(r))).collect();
    assert_eq!(rows, [(TITLE_ROW, Some("Pump")), (DESCRIPTION_ROW, Some("Feeds the boiler"))]);
}

#[test]
fn the_text_is_taken_whole_with_its_spaces_entities_and_line_breaks() {
    let (doc, _) = Drawing::a4()
        .page(&rect("a", "<svg:title>  Pump  </svg:title><svg:desc>one\ntwo &amp; &lt;three&gt;\tfour &#13;</svg:desc>"))
        .page("")
        .read();
    assert_eq!(texts(&doc, 0), ("  Pump  ".to_owned(), "one\ntwo & <three>\tfour \r".to_owned()));
}

#[test]
fn a_title_of_spaces_is_a_row_of_spaces_and_an_empty_element_is_no_row() {
    let (doc, _) = Drawing::a4().page(&rect("a", "<svg:title>   </svg:title><svg:desc/>")).read();
    let (title, description) = texts(&doc, 0);
    assert_eq!((title.as_str(), description.as_str()), ("   ", ""), "The row holds the spaces, and R13 counts it as no alt text");
    let alt = alt_text::read_in(&doc, &SheetRef::Shape(ShapeRef::top(Owner::Page(doc.pages.get(0).unwrap().id), 0)));
    assert!(!alt.has_text());
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let user = shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap();
    assert_eq!(user.rows.len(), 1, "the empty `svg:desc` gives no row");
}

#[test]
fn a_frame_a_group_and_a_connector_have_theirs_and_the_user_rows_of_the_shape_are_kept_beside() {
    let body = concat!(
        "\n    <draw:frame draw:name=\"f\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"2cm\"><draw:text-box><text:p>in a frame</text:p></draw:text-box><svg:title>Frame title</svg:title></draw:frame>",
        "\n    <draw:g draw:name=\"g\"><svg:title>Group title</svg:title><svg:desc>Group description</svg:desc><draw:rect svg:x=\"6cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"><svg:title>Member</svg:title></draw:rect></draw:g>",
        "\n    <draw:connector draw:name=\"c\" svg:x1=\"1cm\" svg:y1=\"8cm\" svg:x2=\"5cm\" svg:y2=\"9cm\"><svg:title>A link</svg:title></draw:connector>",
    );
    let (doc, _) = Drawing::a4().page(body).read();
    assert_eq!(texts(&doc, 0), ("Frame title".to_owned(), String::new()));
    assert_eq!(texts(&doc, 1), ("Group title".to_owned(), "Group description".to_owned()));
    let group = doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    let member = alt_text::read_in(&doc, &SheetRef::Shape(ShapeRef { owner: Owner::Page(doc.pages.get(0).unwrap().id), path: vec![1, 0] }));
    assert_eq!(member.title.text, "Member", "the member of a group");
    assert!(group.sheet.section(&diacraft_doc::SectionKey::new("User", None)).is_some());
    let connector = texts(&doc, 2);
    assert_eq!(connector.0, "A link");
    let c = doc.pages.get(0).unwrap().shapes.get(2).unwrap();
    let names: Vec<String> =
        c.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap().rows.iter().map(|r| r.name.as_deref().unwrap().to_owned()).collect();
    assert!(
        names.contains(&"DiaCraftRoute".to_owned()) && names.contains(&TITLE_ROW.to_owned()),
        "the route marker stays beside the title: {names:?}"
    );
}

#[test]
fn the_elements_that_became_rows_are_not_kept_as_markup_and_the_others_are() {
    let (doc, _) = Drawing::a4().page(&rect("a", "<svg:title>One</svg:title><svg:title>Two</svg:title><svg:desc>Desc</svg:desc>")).read();
    assert_eq!(texts(&doc, 0).0, "One", "the first of a kind is the title");
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let kept = crate::drawing::odf(shape);
    let held: Vec<String> = kept
        .kept
        .iter()
        .filter_map(|p| match &p.node {
            diacraft_doc::RawNode::Element(e) => Some(e.name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(held, ["svg:title"], "the second title is kept whole, as it was; the first and the description are the rows");
}

// ----------------------------------------------------------------------------------------------------------------- a save into the file

fn saved(o: &crate::write_core::Open) -> diacraft_odg::Written {
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn an_unedited_save_keeps_the_elements_byte_for_byte() {
    let d = Drawing::a4().page(&rect("a", "<svg:title>  Pump  </svg:title><svg:desc>Feeds</svg:desc>"));
    let o = open(&d);
    let s = saved(&o);
    assert!(!s.edited);
    assert_eq!(s.bytes, o.bytes, "V38");
    assert!(part(&s.bytes, "content.xml").contains("<svg:title>  Pump  </svg:title>"));
}

#[test]
fn a_title_cleared_is_taken_out_and_nothing_else_changes() {
    let d = Drawing::a4().page(&rect("a", "<svg:title>Pump</svg:title><svg:desc>Feeds</svg:desc>"));
    let mut o = open(&d);
    let id = id_of(&o.doc, "a");
    give(&mut o.doc, id, &[(TITLE_ROW, ""), (DESCRIPTION_ROW, "Feeds")]);
    let after = part(&saved(&o).bytes, "content.xml");
    let before = part(&o.bytes, "content.xml");
    assert_eq!(after, before.replace("<svg:title>Pump</svg:title>", ""), "No svg:title, and the rest as it was");
    let (doc, _) = read(&saved(&o).bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&doc, 0), (String::new(), "Feeds".to_owned()));
}

#[test]
fn a_text_changed_is_written_in_its_element_and_the_next_save_is_no_edit() {
    let d = Drawing::a4().page(&rect("a", "<svg:title>Pump</svg:title><svg:desc>Feeds</svg:desc>"));
    let mut o = open(&d);
    let id = id_of(&o.doc, "a");
    give(&mut o.doc, id, &[(TITLE_ROW, "Valve & <co>"), (DESCRIPTION_ROW, "Feeds")]);
    let first = saved(&o);
    let after = part(&first.bytes, "content.xml");
    assert_eq!(after, part(&o.bytes, "content.xml").replace("<svg:title>Pump</svg:title>", "<svg:title>Valve &amp; &lt;co&gt;</svg:title>"));
    let next = crate::write_core::open_bytes(first.bytes.clone());
    assert_eq!(texts(&next.doc, 0), ("Valve & <co>".to_owned(), "Feeds".to_owned()));
    let again = saved(&next);
    assert!(!again.edited);
    assert_eq!(again.bytes, first.bytes, "a second save of the same model is no edit");
}

/// The `User` section of a shape that holds a standard title row and nothing else carries no data of DiaCraft's, but a change of the attributes it has is a change, and a save writes
/// it (it was dropped: the save was no edit and the file came back as it was).
#[test]
fn an_attribute_of_the_user_section_beside_an_unchanged_title_is_saved() {
    let d = Drawing::a4().page(&rect("a", "<svg:title>Pump</svg:title>"));
    let mut o = open(&d);
    let id = id_of(&o.doc, "a");
    let user_of = |doc: &Document| {
        let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
        shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap().clone()
    };
    assert!(user_of(&o.doc).raw.is_empty(), "the file holds none");
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let at = sheet.sections.iter().position(|s| s.name.as_str() == "User").unwrap();
    Arc::make_mut(&mut sheet.sections[at]).raw = diacraft_doc::RawAttrs::from_pairs([("Extra", "1")]);
    let first = saved(&o);
    assert!(first.edited, "the change of the section's attribute is an edit");
    let next = crate::write_core::open_bytes(first.bytes.clone());
    assert_eq!(texts(&next.doc, 0).0, "Pump", "the title is the element's, as it was");
    assert_eq!(user_of(&next.doc).raw, diacraft_doc::RawAttrs::from_pairs([("Extra", "1")]), "the attribute is in the file");
    let again = saved(&next);
    assert!(!again.edited, "a second save of the same model is no edit");
}

#[test]
fn a_new_title_goes_where_the_schema_puts_it_in_each_kind_of_element() {
    let custom = "\n    <draw:custom-shape draw:name=\"c\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">\n     <text:p/>\n     <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/>\n    </draw:custom-shape>";
    let empty = "\n    <draw:rect draw:name=\"e\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"/>";
    let frame = "\n    <draw:frame draw:name=\"f\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"2cm\"><draw:text-box><text:p>t</text:p></draw:text-box></draw:frame>";
    let d = Drawing::a4().page(&format!("{}{custom}{empty}{frame}", rect("r", "")));
    let mut o = open(&d);
    for name in ["r", "c", "e", "f"] {
        let id = id_of(&o.doc, name);
        give(&mut o.doc, id, &[(TITLE_ROW, &format!("T-{name}")), (DESCRIPTION_ROW, &format!("D-{name}"))]);
    }
    let s = saved(&o);
    let content = part(&s.bytes, "content.xml");
    let order = |tag: &str, before: &str| -> bool {
        let at = content.find(tag).unwrap_or_else(|| panic!("no {tag} in {content}"));
        let other = content[at..].find(before).map(|k| at + k);
        other.is_none_or(|k| at < k)
    };
    // A shape: title first, then the description, before its text, and before the geometry of a custom shape.
    assert!(content.contains("<svg:title>T-r</svg:title><svg:desc>D-r</svg:desc><text:p>Hello</text:p>"), "{content}");
    assert!(order("<svg:title>T-c</svg:title>", "<text:p/>") && order("<svg:desc>D-c</svg:desc>", "<draw:enhanced-geometry"), "{content}");
    // A shape written empty is opened for them.
    assert!(content.contains("<svg:title>T-e</svg:title><svg:desc>D-e</svg:desc></draw:rect>"), "{content}");
    // A frame: after its content.
    assert!(content.contains("</draw:text-box><svg:title>T-f</svg:title><svg:desc>D-f</svg:desc></draw:frame>"), "{content}");
    // The file reads back as the model says, and a second save is no edit.
    let next = crate::write_core::open_bytes(s.bytes.clone());
    for (n, name) in ["r", "c", "e", "f"].iter().enumerate() {
        assert_eq!(texts(&next.doc, u32::try_from(n).unwrap()), (format!("T-{name}"), format!("D-{name}")), "{name}");
    }
    assert!(!saved(&next).edited);
}

#[test]
fn a_description_added_follows_the_title_and_a_title_added_precedes_the_description() {
    let d =
        Drawing::a4().page(&format!("{}{}", rect("t", "<svg:title>Has a title</svg:title>"), rect("d", "<svg:desc>Has a description</svg:desc>")));
    let mut o = open(&d);
    let (t, dd) = (id_of(&o.doc, "t"), id_of(&o.doc, "d"));
    give(&mut o.doc, t, &[(TITLE_ROW, "Has a title"), (DESCRIPTION_ROW, "Added")]);
    give(&mut o.doc, dd, &[(TITLE_ROW, "Added"), (DESCRIPTION_ROW, "Has a description")]);
    let content = part(&saved(&o).bytes, "content.xml");
    assert!(content.contains("<svg:title>Has a title</svg:title><svg:desc>Added</svg:desc><text:p>"), "{content}");
    assert!(content.contains("<svg:title>Added</svg:title><svg:desc>Has a description</svg:desc><text:p>"), "{content}");
}

#[test]
fn text_the_file_cannot_hold_is_refused_in_words_and_nothing_is_written() {
    let d = Drawing::a4().page(&rect("a", ""));
    let mut o = open(&d);
    let id = id_of(&o.doc, "a");
    give(&mut o.doc, id, &[(TITLE_ROW, "bad \u{1} character")]);
    let e = real(&o.doc, &o.bytes, &o.index).expect_err("refused");
    assert!(e.to_string().contains("character"), "{e}");
}

// ---------------------------------------------------------------------------------------------------------------------------- a new drawing

fn with_rows(rows: &[(&str, &str)]) -> Document {
    let mut doc = blank(0);
    let mut shape = rectangle(1, "Rectangle.1", (2.0, 3.0, 2.0, 2.0));
    let mut user = Section::new("User", None);
    for (name, text) in rows {
        user.rows.push(Arc::new(standard_row(name, text)));
    }
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
    shape.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("Hello"))], ..Default::default() }));
    push_shape(&mut doc, shape);
    doc
}

#[test]
fn a_new_shape_is_written_with_its_title_and_description_before_its_text() {
    let doc = with_rows(&[(TITLE_ROW, "Pump"), (DESCRIPTION_ROW, "Feeds the boiler")]);
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    let title = content.find("<svg:title>Pump</svg:title>").expect("the title");
    let desc = content.find("<svg:desc>Feeds the boiler</svg:desc>").expect("the description");
    let text = content.find("<text:p>Hello</text:p>").or_else(|| content.find(">Hello<")).expect("the text");
    assert!(title < desc && desc < text, "Title, then description, then the text: {content}");
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0), ("Pump".to_owned(), "Feeds the boiler".to_owned()));
    let shape = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    let user = shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap();
    assert!(user.rows.first().and_then(|row| alt_text::standard_text(row)).is_some(), "the form the files hold");
}

#[test]
fn the_text_a_new_shape_holds_is_escaped_and_kept_as_it_is() {
    let nasty = " a & <b> \"c\" 'd'\r\ne\tf  ";
    let doc = with_rows(&[(TITLE_ROW, nasty), (DESCRIPTION_ROW, "   ")]);
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0), (nasty.to_owned(), "   ".to_owned()), "character for character, the spaces-only description included");
}

#[test]
fn a_texts_of_one_row_alone_write_one_element_and_an_empty_one_none() {
    for (rows, wants) in [
        (vec![(TITLE_ROW, "Only")], ("<svg:title>Only</svg:title>", "<svg:desc")),
        (vec![(DESCRIPTION_ROW, "Only")], ("<svg:desc>Only</svg:desc>", "<svg:title")),
        (vec![(TITLE_ROW, ""), (DESCRIPTION_ROW, "D")], ("<svg:desc>D</svg:desc>", "<svg:title")),
    ] {
        let doc = with_rows(&rows);
        let content = part(&save(&doc, &WriteOptions::default()).unwrap().bytes, "content.xml");
        assert!(content.contains(wants.0), "{rows:?}: {content}");
        assert!(!content.contains(wants.1), "{rows:?}: no empty element");
    }
}

#[test]
fn an_instance_is_written_with_the_text_its_master_gives_it() {
    let mut doc = blank(0);
    let mut master_shape = diacraft_doc::Shape::new(1);
    let mut user = Section::new("User", None);
    user.rows.push(Arc::new(standard_row(TITLE_ROW, "Valve")));
    user.rows.push(Arc::new(standard_row(DESCRIPTION_ROW, "A gate valve")));
    Arc::make_mut(&mut master_shape.sheet).sections.push(Arc::new(user));
    doc.add_master(Master { id: 5, shapes: ShapeList::from_vec(vec![Arc::new(master_shape)]), ..Master::default() }).unwrap();
    let mut instance = rectangle(2, "Valve.2", (2.0, 3.0, 2.0, 2.0));
    instance.master = Some(5);
    push_shape(&mut doc, instance);
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(
        content.contains("<svg:title>Valve</svg:title><svg:desc>A gate valve</svg:desc>"),
        ".odg has no masters, so the shape gets the resolved texts: {content}"
    );
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0), ("Valve".to_owned(), "A gate valve".to_owned()));
}

#[test]
fn a_text_of_a_million_characters_and_many_titled_shapes_are_written_whole() {
    let big = "x".repeat(1 << 20);
    let doc = with_rows(&[(TITLE_ROW, &big)]);
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0).0.len(), 1 << 20);
    let mut many = blank(0);
    for k in 0..3_000u32 {
        let mut s = rectangle(k + 1, &format!("Rectangle.{}", k + 1), (1.0, 1.0, 1.0, 1.0));
        let mut user = Section::new("User", None);
        user.rows.push(Arc::new(standard_row(TITLE_ROW, &format!("Title {k}"))));
        Arc::make_mut(&mut s.sheet).sections.push(Arc::new(user));
        push_shape(&mut many, s);
    }
    let written = write_with(&many, None, &WriteOptions::default()).unwrap();
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0).0, "Title 0");
    assert_eq!(texts(&back, 2_999).0, "Title 2999");
    let _ = Value::from_text("");
}

// -------------------------------------------------------------------------------------------------------------------------------------- pages

/// The title and the description of the first page, as the document says them.
fn page_texts(doc: &Document) -> (String, String) {
    let alt = alt_text::read_in(doc, &SheetRef::Page(doc.pages.get(0).unwrap().id));
    (alt.title.text.to_owned(), alt.description.text.to_owned())
}

#[test]
fn a_pages_title_and_description_are_the_rows_of_its_sheet_and_the_page_keeps_its_elements() {
    let d = {
        let mut d = Drawing::a4();
        d.pages =
            r#"<draw:page draw:name="P1" draw:master-page-name="Default"><svg:title>Plant overview</svg:title><svg:desc>All the units</svg:desc>
    </draw:page>"#
                .to_owned();
        d
    };
    let (doc, _) = d.read();
    assert_eq!(page_texts(&doc), ("Plant overview".to_owned(), "All the units".to_owned()), "V36");
    // An unedited save is the file.
    let o = open(&d);
    let s = saved(&o);
    assert!(!s.edited);
    assert_eq!(s.bytes, o.bytes);
}

#[test]
fn a_page_title_edited_in_a_file_that_has_one_reads_back_as_edited() {
    let d = {
        let mut d = Drawing::a4();
        d.pages = r#"<draw:page draw:name="P1" draw:master-page-name="Default"><svg:title>Old title</svg:title></draw:page>"#.to_owned();
        d
    };
    let mut o = open(&d);
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut page.sheet);
    sheet.sections.retain(|s| s.name.as_str() != "User");
    let mut user = Section::new("User", None);
    user.rows.push(Arc::new(standard_row(TITLE_ROW, "New title")));
    sheet.sections.push(Arc::new(user));
    let s = saved(&o);
    let (back, _) = read(&s.bytes, &Limits::default()).unwrap();
    assert_eq!(page_texts(&back).0, "New title", "DiaCraft's data says the page's rows; the stale element is not what a reader of ours takes");
}

// ------------------------------------------------------------------------------------------------------- DiaCraft's data and another program's edit

/// A shape with a title that is a formula (so a row the markup cannot say, which DiaCraft's data carries) and a description in the form the files hold.
fn carried() -> Document {
    let mut doc = blank(0);
    let mut shape = rectangle(1, "Rectangle.1", (2.0, 3.0, 2.0, 2.0));
    let mut user = Section::new("User", None);
    let mut row = diacraft_doc::Row { name: Some(Arc::from(TITLE_ROW)), ..diacraft_doc::Row::default() };
    row.cells.insert("Value".into(), diacraft_doc::Cell::with_formula("Prop.Name", Some(Value::from_text("Computed"))).with_unit("STR"));
    row.cells.insert("Prompt".into(), diacraft_doc::Cell::constant(Value::from_text("")));
    user.rows.push(Arc::new(row));
    user.rows.push(Arc::new(standard_row(DESCRIPTION_ROW, "Beside")));
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
    push_shape(&mut doc, shape);
    doc
}

#[test]
fn a_title_that_is_a_formula_is_written_as_its_result_and_carried_as_the_formula() {
    let doc = carried();
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains("<svg:title>Computed</svg:title><svg:desc>Beside</svg:desc>"), "{content}");
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    let shape = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    let user = shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap();
    let title = user.rows.iter().find(|r| r.name.as_deref() == Some(TITLE_ROW)).unwrap();
    assert_eq!(title.cells.get("Value").unwrap().formula.text(), Some("Prop.Name"), "the formula comes back: DiaCraft's data carries the section");
    assert_eq!(texts(&back, 0), ("Computed".to_owned(), "Beside".to_owned()));
}

#[test]
fn another_programs_edit_of_an_element_stands_over_what_diacraft_carried() {
    let doc = carried();
    let written = save(&doc, &WriteOptions::default()).unwrap();
    // The title is edited, the description taken out, in the file's own markup, the way LibreOffice would, and DiaCraft's data kept.
    let package = diacraft_odg::package::Package::read(&written.bytes, &Limits::default()).unwrap();
    let entries: Vec<(String, Vec<u8>)> = package
        .parts()
        .filter(|name| *name != "mimetype")
        .map(|name| {
            let data = package.part(name).unwrap().to_vec();
            let data = if name == "content.xml" {
                String::from_utf8(data)
                    .unwrap()
                    .replace("<svg:title>Computed</svg:title>", "<svg:title>Edited elsewhere</svg:title>")
                    .replace("<svg:desc>Beside</svg:desc>", "")
                    .into_bytes()
            } else {
                data
            };
            (name.to_owned(), data)
        })
        .collect();
    let bytes = crate::build::write(Some("application/vnd.oasis.opendocument.graphics"), &entries);
    let (back, _) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0), ("Edited elsewhere".to_owned(), String::new()), "the elements win: the edited title, and no description");
}

#[test]
fn a_new_group_is_written_with_its_title_before_its_members() {
    let mut doc = blank(0);
    let mut group = diacraft_doc::Shape::new(10);
    group.kind = diacraft_doc::ShapeType::Group;
    group.name = Some(Arc::from("Group_10"));
    group.name_u = Some(Arc::from("Group_10"));
    let mut user = Section::new("User", None);
    user.rows.push(Arc::new(standard_row(TITLE_ROW, "A legend")));
    Arc::make_mut(&mut group.sheet).sections.push(Arc::new(user));
    group.children = ShapeList::from_vec(vec![Arc::new(rectangle(11, "Member", (2.0, 3.0, 2.0, 2.0)))]);
    push_shape(&mut doc, group);
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    let g = content.find("<draw:g").expect("a group");
    let title = content.find("<svg:title>A legend</svg:title>").expect("the title");
    let member = content.find("<draw:custom-shape").expect("the member");
    assert!(g < title && title < member, "{content}");
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(texts(&back, 0).0, "A legend");
}
