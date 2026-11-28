//! Alt text on shapes, pages and masters, and the first accessibility check, through `Session::execute` as the control channel, the CLI and MCP call the commands
//! (`shape.alt_text`, `page.alt_text`, `master.alt_text`, `review.check_accessibility`). The vectors are those; each test names the wrong result
//! it catches. The decisions the spec leaves open are in `docs/editing-commands.md`, "Alt text and the accessibility check".

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Master, Owner, Row, RowKey, Section, SectionKey, Shape, ShapeList, ShapeType, Sheet, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::hyperlinks::edit_shape;
use crate::{blank_session, check_session, run, snapshot};

// ------------------------------------------------------------------------------------------------------------------------------------- building

/// One cell of a row: its name, its text and its unit. A text that starts with `=` is a formula, `=FORMULA|result`.
type C<'a> = (&'a str, &'a str, &'a str);
/// One row of a `User` section: its name and its cells.
type R<'a> = (&'a str, &'a [C<'a>]);

fn cell_of((_, text, unit): &C<'_>) -> Cell {
    let cell = match text.strip_prefix('=') {
        Some(formula) => {
            let (formula, result) = formula.split_once('|').unwrap_or((formula, ""));
            Cell::with_formula(formula, Some(Value::from_text(result)))
        }
        None => Cell::constant(Value::from_text(text)),
    };
    if unit.is_empty() { cell } else { cell.with_unit(unit) }
}

fn user_section(rows: &[R<'_>]) -> Section {
    let mut section = Section::new("User", None);
    for (name, cells) in rows {
        let mut row = Row { name: Some(Arc::from(*name)), ..Row::default() };
        for c in *cells {
            row.cells.insert(c.0.into(), cell_of(c));
        }
        section.rows.push(Arc::new(row));
    }
    section
}

/// Both texts of a row as the public files hold them: a `Value` of unit `STR` and an empty `Prompt`.
fn text_row<'a>(name: &'a str, text: &'a str) -> (&'a str, Vec<C<'a>>) {
    (name, vec![("Value", text, "STR"), ("Prompt", "", "")])
}

/// Gives shape `id` these rows in its `User` section, as a file holds them.
fn give(s: &mut Session, id: u32, rows: &[(&str, Vec<C<'_>>)]) {
    let rows: Vec<R<'_>> = rows.iter().map(|(n, c)| (*n, c.as_slice())).collect();
    edit_shape(s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|sec| sec.name.as_str() != "User");
        sheet.sections.push(Arc::new(user_section(&rows)));
    });
}

fn give_title(s: &mut Session, id: u32, title: &str) {
    give(s, id, &[text_row("visAltText", title)]);
}

/// Puts a master whose one shape (`ID` 1) has these rows, and whose page sheet has `sheet_rows`, in the document.
fn add_master(s: &mut Session, master: u32, rows: &[(&str, Vec<C<'_>>)], sheet_rows: &[(&str, Vec<C<'_>>)]) {
    let rows: Vec<R<'_>> = rows.iter().map(|(n, c)| (*n, c.as_slice())).collect();
    let sheet_rows: Vec<R<'_>> = sheet_rows.iter().map(|(n, c)| (*n, c.as_slice())).collect();
    let mut shape = Shape::new(1);
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user_section(&rows)));
    let mut page_sheet = Sheet::default();
    if !sheet_rows.is_empty() {
        page_sheet.sections.push(Arc::new(user_section(&sheet_rows)));
    }
    s.edit("test: a master", |d| {
        d.add_master(Master { id: master, shapes: ShapeList::from_vec(vec![Arc::new(shape)]), sheet: Arc::new(page_sheet), ..Master::default() })
            .unwrap();
        Ok(())
    })
    .unwrap();
}

/// A new rectangle that is an instance of master `master`, with no `User` section of its own.
fn instance(s: &mut Session, master: u32) -> u32 {
    let id = drop_shape(s, "rectangle", 3.0, 3.0);
    edit_shape(s, id, |shape| {
        shape.master = Some(master);
        Arc::make_mut(&mut shape.sheet).sections.retain(|sec| sec.name.as_str() != "User");
    });
    id
}

fn alt(s: &mut Session, id: u32) -> Json {
    run(s, "shape.alt_text", json!({"id": id}))
}

/// The message of a refusal in words, with the session left as it was.
fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let before = snapshot(s);
    let e = s.execute(command, &params).expect_err("the call is refused");
    assert_eq!(snapshot(s), before, "{command}: a refusal changes nothing");
    match &e {
        EngineError::BadParams { msg, .. } | EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

fn undo_steps(s: &Session) -> usize {
    snapshot(s).undo_steps
}

/// The `User` section of shape `id` on page 1, as the document holds it.
fn own_section(s: &Session, id: u32) -> Option<Arc<Section>> {
    let d: &Document = doc(s);
    let at = d.find_shape(Owner::Page(1), id)?;
    d.shape(&at)?.sheet.section(&SectionKey::new("User", None)).cloned()
}

/// What a row of the shape's own `User` section holds: the text of its `Value`, its unit, its formula, and the text of its `Prompt`.
type OwnRow = (String, Option<String>, Option<String>, Option<String>);

/// The `Value` and the `Prompt` cells of a row of the shape's own `User` section.
fn own_row(s: &Session, id: u32, row: &str) -> Option<OwnRow> {
    let section = own_section(s, id)?;
    let r = section.row(&RowKey::name(row))?.clone();
    let v = r.cells.get("Value")?;
    Some((
        v.value.as_ref()?.text().to_owned(),
        v.unit.as_ref().map(|u| u.to_string()),
        v.formula.text().map(str::to_owned),
        r.cells.get("Prompt").and_then(|p| p.value.as_ref()).map(|p| p.text().to_owned()),
    ))
}

// ------------------------------------------------------------------------------------------------------------- where the texts live

#[test]
fn the_title_and_the_description_are_the_two_rows_and_not_swapped() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[text_row("visAltText", "Pump P-101"), text_row("visAltDescription", "Feeds the boiler")]);
    let a = alt(&mut s, id);
    assert_eq!((a["title"].as_str(), a["description"].as_str()), (Some("Pump P-101"), Some("Feeds the boiler")), "V1");
    assert_eq!((a["title_from"].as_str(), a["description_from"].as_str()), (Some("own"), Some("own")));
    assert_eq!(a["has_alt_text"], true);
    assert_eq!(a["kind"], "shape");
}

#[test]
fn a_value_with_no_unit_is_a_title_all_the_same() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[("visAltText", vec![("Value", "Valve", "")])]);
    assert_eq!(alt(&mut s, id)["title"], "Valve", "17 of 521 title rows in the files carry no unit");
}

#[test]
fn an_instance_inherits_the_masters_text_and_a_local_row_replaces_it() {
    let mut s = blank_session();
    add_master(&mut s, 5, &[text_row("visAltText", "Valve")], &[]);
    let a = instance(&mut s, 5);
    let found = alt(&mut s, a);
    assert_eq!(found["title"], "Valve", "V3");
    assert_eq!((found["title_from"].as_str(), found["title_master"].as_u64()), (Some("master"), Some(5)));
    assert_eq!(found["description_from"], "none");
    assert!(own_section(&s, a).is_none(), "an instance does not copy its master's rows");
    let b = instance(&mut s, 5);
    give_title(&mut s, b, "Valve V-7");
    let found = alt(&mut s, b);
    assert_eq!((found["title"].as_str(), found["title_from"].as_str()), (Some("Valve V-7"), Some("own")), "V4");
}

#[test]
fn a_member_of_an_instance_inherits_the_text_of_the_masters_member() {
    let mut s = blank_session();
    // The master: a group (shape 1) whose member (shape 2) has the description.
    let mut member = Shape::new(2);
    Arc::make_mut(&mut member.sheet).sections.push(Arc::new(user_section(&[("visAltDescription", &[("Value", "Handle", "STR")])])));
    let mut group = Shape::new(1);
    group.kind = ShapeType::Group;
    group.children = ShapeList::from_vec(vec![Arc::new(member)]);
    s.edit("test: a master of a group", |d| {
        d.add_master(Master { id: 6, shapes: ShapeList::from_vec(vec![Arc::new(group)]), ..Master::default() }).unwrap();
        Ok(())
    })
    .unwrap();
    // The instance and its member, which stands for shape 2 and has no row.
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let mut stands_for = Shape::new(900);
    stands_for.master_shape = Some(2);
    edit_shape(&mut s, id, |shape| {
        shape.master = Some(6);
        shape.kind = ShapeType::Group;
        shape.children = ShapeList::from_vec(vec![Arc::new(stands_for)]);
    });
    let found = alt(&mut s, 900);
    assert_eq!((found["description"].as_str(), found["description_from"].as_str()), (Some("Handle"), Some("master")), "V5");
}

#[test]
fn an_empty_title_and_a_blank_description_are_no_alt_text() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[text_row("visAltText", ""), text_row("visAltDescription", "   ")]);
    let found = alt(&mut s, id);
    assert_eq!(found["has_alt_text"], false, "A row that exists is not alt text");
    assert_eq!(found["description"], "   ", "the text is kept as it is");
}

#[test]
fn white_space_is_unicodes_and_a_middle_dot_is_text() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give_title(&mut s, id, "\u{a0}");
    assert_eq!(alt(&mut s, id)["has_alt_text"], false, "A no-break space is white space");
    give_title(&mut s, id, "\u{b7}");
    assert_eq!(alt(&mut s, id)["has_alt_text"], true, "A middle dot is not");
}

#[test]
fn only_the_two_rows_are_alt_text_and_names_are_not_matched_by_a_part() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[("visDescription", vec![("Value", "Router", "STR")]), ("visAltTextual", vec![("Value", "x", "STR")])]);
    edit_shape(&mut s, id, |shape| {
        let mut prop = Section::new("Prop", None);
        let mut row = Row { name: Some(Arc::from("Description")), ..Row::default() };
        row.cells.insert("Value".into(), Cell::constant(Value::from_text("Core router")));
        prop.rows.push(Arc::new(row));
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(prop));
    });
    let found = alt(&mut s, id);
    assert_eq!(found["has_alt_text"], false, "V8");
    assert_eq!((found["title"].as_str(), found["description"].as_str()), (Some(""), Some("")));
}

#[test]
fn a_masters_page_sheet_is_not_inherited() {
    let mut s = blank_session();
    add_master(&mut s, 7, &[], &[text_row("visAltDescription", "Server")]);
    let id = instance(&mut s, 7);
    assert_eq!(alt(&mut s, id)["has_alt_text"], false, "Instances inherit from master shapes");
    // The master's shape and its page sheet are told apart: the command reads the shape.
    let m = run(&mut s, "master.alt_text", json!({"id": 7}));
    assert_eq!(m["has_alt_text"], false);
    assert_eq!(m["kind"], "master");
}

#[test]
fn a_row_deleted_on_an_instance_hides_the_masters() {
    let mut s = blank_session();
    add_master(&mut s, 5, &[text_row("visAltText", "Valve")], &[]);
    let id = instance(&mut s, 5);
    edit_shape(&mut s, id, |shape| {
        let mut user = Section::new("User", None);
        user.rows.push(Arc::new(Row { name: Some(Arc::from("visAltText")), del: true, ..Row::default() }));
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
    });
    assert_eq!(alt(&mut s, id)["has_alt_text"], false, "The deleted row stops the master's title");
}

#[test]
fn a_number_is_its_text_and_an_error_is_empty() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[("visAltText", vec![("Value", "42", "")])]);
    let found = alt(&mut s, id);
    assert_eq!((found["title"].as_str(), found["has_alt_text"].as_bool()), (Some("42"), Some(true)), "E3");
    edit_shape(&mut s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = sheet.section_mut(&SectionKey::new("User", None)).unwrap();
        let row = Arc::make_mut(section.rows.get_mut(0).unwrap());
        row.cells.get_mut("Value").unwrap().error = Some(Arc::from("#REF!"));
    });
    let found = alt(&mut s, id);
    assert_eq!((found["title"].as_str(), found["has_alt_text"].as_bool()), (Some(""), Some(false)), "An error counts as empty");
}

// ------------------------------------------------------------------------------------------------------------------- writing

#[test]
fn a_title_set_on_an_instance_writes_its_own_row_and_leaves_the_master_and_the_others() {
    let mut s = blank_session();
    add_master(&mut s, 5, &[text_row("visAltText", "Valve")], &[]);
    let a = instance(&mut s, 5);
    let b = instance(&mut s, 5);
    let answer = run(&mut s, "shape.alt_text", json!({"id": a, "title": "Valve V-9"}));
    assert_eq!((answer["title"].as_str(), answer["title_from"].as_str(), answer["changed"].as_bool()), (Some("Valve V-9"), Some("own"), Some(true)));
    assert_eq!(
        own_row(&s, a, "visAltText"),
        Some(("Valve V-9".to_owned(), Some("STR".to_owned()), None, Some(String::new()))),
        "A local row, a string constant of unit STR and an empty prompt"
    );
    assert_eq!(alt(&mut s, b)["title"], "Valve", "A second instance is as it was");
    let master = run(&mut s, "master.alt_text", json!({"id": 5}));
    assert_eq!(master["title"], "Valve", "The master is as it was");
    assert!(own_section(&s, b).is_none());
    check_session(&s).unwrap();
}

#[test]
fn clearing_an_inherited_title_writes_an_empty_own_row() {
    let mut s = blank_session();
    add_master(&mut s, 5, &[text_row("visAltText", "Valve")], &[]);
    let a = instance(&mut s, 5);
    run(&mut s, "shape.alt_text", json!({"id": a, "title": ""}));
    assert_eq!(own_row(&s, a, "visAltText").map(|r| r.0), Some(String::new()), "A row holding the empty text, not a deleted one");
    let found = alt(&mut s, a);
    assert_eq!((found["title"].as_str(), found["title_from"].as_str()), (Some(""), Some("own")), "Not the master's");
    // Clearing a sheet that has no text makes no row.
    let c = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    let before = undo_steps(&s);
    let answer = run(&mut s, "shape.alt_text", json!({"id": c, "title": "", "description": ""}));
    assert_eq!(answer["changed"], false);
    assert!(own_section(&s, c).is_none());
    assert_eq!(undo_steps(&s), before);
}

#[test]
fn both_texts_are_one_undo_step_and_undo_leaves_no_section() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = undo_steps(&s);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "A", "description": "B"}));
    assert_eq!(undo_steps(&s), before + 1, "One step for both");
    assert_eq!(snapshot(&s).next_undo.as_deref(), Some("Set alt text"));
    run(&mut s, "edit.undo", json!({}));
    assert!(own_section(&s, id).is_none(), "No User section is left");
    assert_eq!(alt(&mut s, id)["has_alt_text"], false);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((alt(&mut s, id)["title"].as_str(), alt(&mut s, id)["description"].as_str()), (Some("A"), Some("B")));
}

#[test]
fn a_text_equal_to_the_own_value_writes_nothing() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give_title(&mut s, id, "X");
    let before = snapshot(&s);
    let answer = run(&mut s, "shape.alt_text", json!({"id": id, "title": "X"}));
    assert_eq!(answer["changed"], false, "V13");
    let after = snapshot(&s);
    assert_eq!((after.undo_steps, after.revision), (before.undo_steps, before.revision), "No undo step and the drawing is not made unsaved");
}

#[test]
fn a_guarded_description_is_read_by_its_result_and_refuses_a_change() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[("visAltDescription", vec![("Value", "=GUARD(Prop.Notes)|Long note", "STR")])]);
    let found = alt(&mut s, id);
    assert_eq!(found["description"], "Long note", "The result is what the checker and the exports use");
    assert_eq!(found["description_formula"], "GUARD(Prop.Notes)", "The window shows the formula's text");
    assert_eq!(found["description_protected"], true);
    assert_eq!(found["title_protected"], false);
    let words = refused(&mut s, "shape.alt_text", json!({"id": id, "description": "Other"}));
    assert!(words.contains("GUARD") && words.contains("protected"), "{words}");
    assert_eq!(alt(&mut s, id)["description"], "Long note");
}

#[test]
fn a_typed_text_replaces_a_formula_with_the_constant() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give(&mut s, id, &[("visAltDescription", vec![("Value", "=Prop.Name|Pump", "STR")])]);
    let found = alt(&mut s, id);
    assert_eq!((found["description"].as_str(), found["description_formula"].as_str()), (Some("Pump"), Some("Prop.Name")), "V15");
    run(&mut s, "shape.alt_text", json!({"id": id, "description": "Pump 2"}));
    assert_eq!(own_row(&s, id, "visAltDescription").map(|r| (r.0, r.2)), Some(("Pump 2".to_owned(), None)), "A constant, no formula");
    assert_eq!(alt(&mut s, id)["description_formula"], Json::Null);
}

#[test]
fn a_long_text_of_a_file_is_answered_with_its_length_and_kept_by_an_edit_of_the_other() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let long = "d".repeat(300);
    give(&mut s, id, &[text_row("visAltDescription", &long)]);
    let found = alt(&mut s, id);
    assert_eq!((found["description"].as_str(), found["description_length"].as_u64()), (Some(long.as_str()), Some(300)), "V16");
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "Short"}));
    assert_eq!(alt(&mut s, id)["description"], long.as_str(), "300 characters are kept after the title is edited");
    assert_eq!(own_row(&s, id, "visAltDescription").unwrap().0, long);
}

#[test]
fn a_text_over_the_cap_is_refused_and_the_cap_is_taken_whole() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let words = refused(&mut s, "shape.alt_text", json!({"id": id, "title": "t".repeat(32_769)}));
    assert!(words.contains("32768") && words.contains("title"), "{words}");
    let words = refused(&mut s, "shape.alt_text", json!({"id": id, "title": "ok", "description": "d".repeat(1 << 20)}));
    assert!(words.contains("description") && words.contains("32768"), "a 1 MiB text: {words}");
    let whole = "w".repeat(32_768);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": whole}));
    assert_eq!(alt(&mut s, id)["title_length"], 32_768, "The cap is accepted");
}

#[test]
fn a_text_is_kept_character_for_character() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": " Pump ", "description": "Pump\nP-101\t(a)\r\nEnd "}));
    let found = alt(&mut s, id);
    assert_eq!(found["title"], " Pump ", "No trimming");
    assert_eq!(found["description"], "Pump\nP-101\t(a)\r\nEnd ", "Line feeds, tabs and carriage returns kept");
    // The same text again is nothing to write.
    let before = undo_steps(&s);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": " Pump "}));
    assert_eq!(undo_steps(&s), before);
}

#[test]
fn characters_xml_cannot_hold_are_refused() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for bad in ["a\u{0}b", "a\u{1b}b", "a\u{b}b", "a\u{c}b", "a\u{fffe}b", "a\u{ffff}b"] {
        let words = refused(&mut s, "shape.alt_text", json!({"id": id, "title": bad}));
        assert!(words.contains("control characters"), "{bad:?}: {words}");
    }
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "tab\tline\ncr\r\u{7f}\u{85}"}));
}

#[test]
fn a_master_with_two_top_level_shapes_is_refused_by_naming_them_and_a_shape_picks_one() {
    let mut s = blank_session();
    let mut a = Shape::new(10);
    let mut b = Shape::new(11);
    Arc::make_mut(&mut b.sheet).sections.push(Arc::new(user_section(&[("visAltText", &[("Value", "Second", "STR")])])));
    a.name = Some(Arc::from("a"));
    s.edit("test: a master of two", |d| {
        d.add_master(Master { id: 8, shapes: ShapeList::from_vec(vec![Arc::new(a), Arc::new(b)]), ..Master::default() }).unwrap();
        Ok(())
    })
    .unwrap();
    let words = refused(&mut s, "master.alt_text", json!({"id": 8, "title": "x"}));
    assert!(words.contains("10") && words.contains("11") && words.contains("shape"), "{words}");
    let found = run(&mut s, "master.alt_text", json!({"id": 8, "shape": 11}));
    assert_eq!((found["title"].as_str(), found["shape"].as_u64()), (Some("Second"), Some(11)));
    run(&mut s, "master.alt_text", json!({"id": 8, "shape": 10, "title": "First"}));
    assert_eq!(run(&mut s, "master.alt_text", json!({"id": 8, "shape": 10}))["title"], "First");
    let words = refused(&mut s, "master.alt_text", json!({"id": 8, "shape": 99}));
    assert!(words.contains("99"), "{words}");
    let words = refused(&mut s, "master.alt_text", json!({"id": 404}));
    assert!(words.contains("404"), "{words}");
}

#[test]
fn a_master_is_set_by_its_one_shape_and_its_instances_inherit_the_text() {
    let mut s = blank_session();
    add_master(&mut s, 5, &[], &[]);
    let id = instance(&mut s, 5);
    assert_eq!(alt(&mut s, id)["has_alt_text"], false);
    let answer = run(&mut s, "master.alt_text", json!({"id": 5, "title": "Valve", "description": "A gate valve"}));
    assert_eq!((answer["title"].as_str(), answer["shape"].as_u64()), (Some("Valve"), Some(1)));
    let found = alt(&mut s, id);
    assert_eq!(
        (found["title"].as_str(), found["description"].as_str(), found["title_from"].as_str()),
        (Some("Valve"), Some("A gate valve"), Some("master"))
    );
    assert!(own_section(&s, id).is_none(), "the instance still holds none of its own");
    // The master's page sheet is untouched.
    assert!(doc(&s).master(5).unwrap().sheet.sections.is_empty());
}

#[test]
fn a_page_has_its_own_texts_and_the_current_page_is_the_first_foreground_one() {
    let mut s = blank_session();
    let answer = run(&mut s, "page.alt_text", json!({"title": "Plant overview", "description": "All units"}));
    assert_eq!((answer["kind"].as_str(), answer["page"].as_u64(), answer["title"].as_str()), (Some("page"), Some(1), Some("Plant overview")));
    let read = run(&mut s, "page.alt_text", json!({}));
    assert_eq!(
        (read["title"].as_str(), read["description"].as_str(), read["changed"].as_bool()),
        (Some("Plant overview"), Some("All units"), Some(false))
    );
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = run(&mut s, "page.alt_text", json!({"page": 2, "title": "Second plant"}));
    assert_eq!(second["page"], 2);
    assert_eq!(run(&mut s, "page.alt_text", json!({"page": 1}))["title"], "Plant overview");
    let d: &Document = doc(&s);
    let sheet = &d.page(1).unwrap().sheet;
    assert!(sheet.section(&SectionKey::new("User", None)).is_some(), "the rows are in the page sheet");
    let words = refused(&mut s, "page.alt_text", json!({"page": 77}));
    assert!(words.contains("77"), "{words}");
    run(&mut s, "page.alt_text", json!({"page": 1, "title": "", "description": ""}));
    let cleared = run(&mut s, "page.alt_text", json!({"page": 1}));
    assert_eq!((cleared["has_alt_text"].as_bool(), cleared["title_from"].as_str()), (Some(false), Some("own")));
    check_session(&s).unwrap();
}

#[test]
fn the_commands_refuse_what_they_do_not_take_in_words() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for (command, params, word) in [
        ("shape.alt_text", json!({}), "id"),
        ("shape.alt_text", json!({"id": 9999}), "9999"),
        ("shape.alt_text", json!({"id": id, "title": 5}), "title"),
        ("shape.alt_text", json!({"id": id, "description": ["x"]}), "description"),
        ("shape.alt_text", json!({"id": id, "page": 9}), "9"),
        ("master.alt_text", json!({}), "id"),
        ("page.alt_text", json!({"title": {"a": 1}}), "title"),
    ] {
        let words = refused(&mut s, command, params.clone());
        assert!(words.contains(word), "{command} {params}: {words}");
    }
    let e = s.execute("shape.alt_text", &json!({"id": id, "alt": "x"})).unwrap_err();
    assert!(e.to_string().contains("unknown parameter"), "{e}");
}

#[test]
fn a_shape_can_be_cleared_with_clear() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "A", "description": "B"}));
    let answer = run(&mut s, "shape.alt_text", json!({"id": id, "clear": true}));
    assert_eq!((answer["has_alt_text"].as_bool(), answer["changed"].as_bool()), (Some(false), Some(true)));
    assert_eq!(own_row(&s, id, "visAltText").map(|r| r.0), Some(String::new()), "the rows hold the empty text and stay");
    let words = refused(&mut s, "shape.alt_text", json!({"id": id, "clear": true, "title": "x"}));
    assert!(words.contains("clear"), "{words}");
}

// -------------------------------------------------------------------------------------------------------------------------- the check

/// A page with a name of its own and a title, so that the check of the shapes on it has nothing to say about the page.
fn plant(s: &mut Session) {
    run(s, "page.rename", json!({"id": 1, "name": "Overview"}));
    run(s, "page.alt_text", json!({"title": "Plant"}));
}

fn check(s: &mut Session) -> Json {
    run(s, "review.check_accessibility", json!({}))
}

fn findings(answer: &Json) -> Vec<Json> {
    answer["findings"].as_array().unwrap().clone()
}

fn rules(answer: &Json) -> Vec<(String, Option<u64>)> {
    findings(answer).iter().map(|f| (f["rule"].as_str().unwrap().to_owned(), f["shape"].as_u64())).collect()
}

fn errors(answer: &Json) -> Vec<Json> {
    findings(answer).into_iter().filter(|f| f["severity"] == "error").collect()
}

#[test]
fn only_the_picture_with_no_text_is_an_error_not_a_title_alone_a_connector_or_a_guide() {
    let mut s = blank_session();
    plant(&mut s);
    let rectangle = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let picture = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    let other = drop_shape(&mut s, "rectangle", 8.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": rectangle, "title": "A box"}));
    let connector = run(&mut s, "connector.add", json!({"from": {"id": rectangle}, "to": {"id": other}}))["id"].as_u64().unwrap();
    let guide = drop_shape(&mut s, "rectangle", 1.0, 1.0);
    edit_shape(&mut s, guide, |shape| shape.kind = ShapeType::Guide);
    run(&mut s, "shape.alt_text", json!({"id": other, "description": "Another box"}));
    let answer = check(&mut s);
    let errors = errors(&answer);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["rule"], "alt_text_missing_on_shape");
    assert_eq!(errors[0]["shape"].as_u64(), Some(u64::from(picture)));
    assert!(errors[0]["page"].as_u64() == Some(1) && errors[0]["message"].as_str().unwrap().contains("no alt text"));
    assert_eq!(errors[0]["fix"]["action"], "add_alt_text");
    assert_eq!(errors[0]["fix"]["command"], "shape.alt_text");
    assert_eq!(errors[0]["fix"]["params"]["id"].as_u64(), Some(u64::from(picture)));
    assert_eq!(answer["pages_checked"], 1);
    assert_eq!(answer["shapes_checked"], 3, "the three boxes: the connector and the guide are left out");
    assert_eq!(answer["shapes_skipped"], 2);
    assert!(findings(&answer).iter().all(|f| f["shape"].as_u64() != Some(connector) && f["shape"].as_u64() != Some(u64::from(guide))));
    assert_eq!(answer["errors"], 1);
}

#[test]
fn a_group_answers_for_itself_and_its_members_are_not_looked_at() {
    let mut s = blank_session();
    plant(&mut s);
    // Group A has no rows and two members with titles; group B has the title "Legend" and members with none.
    let member = |id: u32, title: Option<&str>| {
        let mut m = Shape::new(id);
        if let Some(t) = title {
            Arc::make_mut(&mut m.sheet).sections.push(Arc::new(user_section(&[("visAltText", &[("Value", t, "STR")])])));
        }
        Arc::new(m)
    };
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    edit_shape(&mut s, a, |shape| {
        shape.kind = ShapeType::Group;
        shape.children = ShapeList::from_vec(vec![member(901, Some("one")), member(902, Some("two"))]);
    });
    edit_shape(&mut s, b, |shape| {
        shape.kind = ShapeType::Group;
        shape.children = ShapeList::from_vec(vec![member(903, None), member(904, None)]);
    });
    give_title(&mut s, b, "Legend");
    let answer = check(&mut s);
    assert_eq!(rules(&answer), [("alt_text_missing_on_shape".to_owned(), Some(u64::from(a)))], "One finding, for the first group");
}

#[test]
fn an_instance_whose_master_has_the_text_is_no_finding() {
    let mut s = blank_session();
    plant(&mut s);
    add_master(&mut s, 5, &[text_row("visAltDescription", "Tank")], &[]);
    instance(&mut s, 5);
    assert!(findings(&check(&mut s)).is_empty(), "Inherited text counts");
}

#[test]
fn a_foreground_page_with_no_text_is_an_error_and_a_background_page_is_left_out() {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    run(&mut s, "page.add", json!({"name": "VBackground-1", "background": true}));
    // The background page holds a shape with no text: it is not a finding either.
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let answer = check(&mut s);
    let pages: Vec<(String, Option<u64>, u64)> =
        findings(&answer).iter().map(|f| (f["rule"].as_str().unwrap().to_owned(), f["shape"].as_u64(), f["page"].as_u64().unwrap())).collect();
    assert!(pages.contains(&("alt_text_missing_on_page".to_owned(), None, 1)), "{pages:?}");
    assert_eq!(pages.iter().filter(|p| p.0 == "alt_text_missing_on_page").count(), 1, "The background page has none");
    assert_eq!(answer["pages_checked"], 1);
    let fix = findings(&answer).into_iter().find(|f| f["rule"] == "alt_text_missing_on_page").unwrap();
    assert_eq!((fix["fix"]["action"].as_str(), fix["fix"]["command"].as_str()), (Some("add_alt_text"), Some("page.alt_text")));
    // A check of the background page names why it looks at nothing.
    let bg = doc(&s).pages.iter().find(|p| p.background).unwrap().id;
    let one = run(&mut s, "review.check_accessibility", json!({"page": bg}));
    assert_eq!((one["pages_checked"].as_u64(), findings(&one).len()), (Some(0), 0));
    assert!(one["notes"][0].as_str().unwrap().contains("background"));
}

#[test]
fn shapes_on_a_hidden_layer_are_left_out_and_shapes_on_a_shown_one_are_checked() {
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    run(&mut s, "layer.add", json!({"name": "Plan"}));
    let hidden = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let shown = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    run(&mut s, "layer.assign", json!({"ids": [hidden], "layers": [0]}));
    run(&mut s, "layer.assign", json!({"ids": [shown], "layers": [1]}));
    run(&mut s, "layer.set", json!({"layer": 0, "visible": false}));
    let answer = check(&mut s);
    assert_eq!(
        rules(&answer),
        [("alt_text_missing_on_shape".to_owned(), Some(u64::from(shown)))],
        "The shape on a visible layer, not the hidden one"
    );
}

#[test]
fn a_file_name_or_a_web_address_is_an_error_once_for_each_sheet() {
    let mut s = blank_session();
    plant(&mut s);
    let named = [
        ("diagram1.png", ""),
        ("logo.SVG", ""),
        ("see www.example.org", ""),
        ("https://example.org/x)", ""),
        ("", "photo.jpeg."),
        ("map.png", "map.png too"),
    ];
    let fine = ["Release 2.0", "e.g. a pump", "Pump.", ".png", "Version 3.vsdx-tool"];
    let mut bad_ids = Vec::new();
    for (i, (title, description)) in named.iter().enumerate() {
        let id = drop_shape(&mut s, "rectangle", 1.0 + i as f64, 3.0);
        run(&mut s, "shape.alt_text", json!({"id": id, "title": title, "description": description}));
        bad_ids.push(u64::from(id));
    }
    for (i, title) in fine.iter().enumerate() {
        let id = drop_shape(&mut s, "rectangle", 1.0 + i as f64, 6.0);
        run(&mut s, "shape.alt_text", json!({"id": id, "title": title}));
    }
    let answer = check(&mut s);
    let got: Vec<Json> = findings(&answer).into_iter().filter(|f| f["rule"] == "alt_text_is_file_name").collect();
    let ids: Vec<u64> = got.iter().map(|f| f["shape"].as_u64().unwrap()).collect();
    assert_eq!(ids, bad_ids, "Each once, in the drawing's order, and none of the others");
    assert_eq!(got.len(), findings(&answer).iter().filter(|f| f["rule"] != "page_has_default_name").count(), "nothing else is found");
    assert_eq!((got[0]["field"].as_str(), got[4]["field"].as_str(), got[5]["field"].as_str()), (Some("title"), Some("description"), Some("both")));
    assert!(got.iter().all(|f| f["severity"] == "error" && f["fix"]["action"] == "add_alt_text"));
    // A page's text is checked the same way.
    run(&mut s, "page.alt_text", json!({"title": "", "description": "overview.pdf"}));
    let page_findings: Vec<Json> = findings(&check(&mut s)).into_iter().filter(|f| f["shape"].is_null()).collect();
    assert!(page_findings.iter().any(|f| f["rule"] == "alt_text_is_file_name" && f["field"] == "description"), "{page_findings:?}");
}

fn set_universal_name(s: &mut Session, page: u32, name_u: &str) {
    s.edit("test: a universal name", |d| {
        let at = d.pages.iter().position(|p| p.id == page).unwrap();
        Arc::make_mut(d.pages.get_mut(at).unwrap()).name_u = Some(Arc::from(name_u));
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_default_page_name_is_a_warning_and_a_renamed_page_is_not() {
    let mut s = blank_session();
    let mut expected = Vec::new();
    for name in ["Page-3", "Page-1-2", "Overview", "Page 3", "Page-3a"] {
        let id = run(&mut s, "page.add", json!({"name": name}))["id"].as_u64().unwrap();
        run(&mut s, "page.alt_text", json!({"page": id, "title": "t"}));
        if matches!(name, "Page-3" | "Page-1-2") {
            expected.push(id);
        }
    }
    // A default of another language: the name `Seite-4` under the universal name `Page-4`.
    let seite = run(&mut s, "page.add", json!({"name": "Seite-4"}))["id"].as_u64().unwrap();
    run(&mut s, "page.alt_text", json!({"page": seite, "title": "t"}));
    set_universal_name(&mut s, u32::try_from(seite).unwrap(), "Page-4");
    expected.push(seite);
    // A renamed page: the name `Overview` under the universal name `Page-1`.
    let renamed = run(&mut s, "page.add", json!({"name": "Overview 2"}))["id"].as_u64().unwrap();
    run(&mut s, "page.alt_text", json!({"page": renamed, "title": "t"}));
    set_universal_name(&mut s, u32::try_from(renamed).unwrap(), "Page-1");
    // A background page named like a default is not looked at.
    run(&mut s, "page.add", json!({"name": "VBackground-1", "background": true}));
    run(&mut s, "page.alt_text", json!({"page": 1, "title": "t"}));
    let answer = check(&mut s);
    let warned: Vec<u64> = findings(&answer).iter().filter(|f| f["rule"] == "page_has_default_name").map(|f| f["page"].as_u64().unwrap()).collect();
    // Page 1 is `Page-1` too (the blank drawing's name).
    let mut want = vec![1];
    want.extend(expected);
    assert_eq!(warned, want, "V27");
    let one = findings(&answer).into_iter().find(|f| f["rule"] == "page_has_default_name").unwrap();
    assert_eq!(
        (one["severity"].as_str(), one["fix"]["action"].as_str(), one["fix"]["command"].as_str()),
        (Some("warning"), Some("rename_page"), Some("page.rename"))
    );
    assert_eq!(answer["errors"], 0);
    assert_eq!(answer["warnings"].as_u64(), Some(want.len() as u64));
}

#[test]
fn a_link_with_no_description_or_only_its_address_is_a_warning_for_each_row() {
    let mut s = blank_session();
    plant(&mut s);
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "A box"}));
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [
            {"address": "https://example.org", "description": ""},
            {"address": "https://example.org/a", "description": " HTTPS://EXAMPLE.ORG/A "},
            {"address": "https://example.org/b", "description": "Product page"},
            {"sub_address": "Page-1", "description": ""},
        ]}),
    );
    let answer = check(&mut s);
    let warned: Vec<(String, Option<String>)> = findings(&answer)
        .iter()
        .filter(|f| f["rule"] == "hyperlink_has_no_description")
        .map(|f| (f["row"].as_str().unwrap().to_owned(), f["fix"]["action"].as_str().map(str::to_owned)))
        .collect();
    assert_eq!(
        warned,
        [
            ("Row_1".to_owned(), Some("edit_hyperlink".to_owned())),
            ("Row_2".to_owned(), Some("edit_hyperlink".to_owned())),
            ("Row_4".to_owned(), Some("edit_hyperlink".to_owned()))
        ],
        "V28"
    );
    let first = findings(&answer).into_iter().find(|f| f["rule"] == "hyperlink_has_no_description").unwrap();
    assert_eq!(
        (first["shape"].as_u64(), first["severity"].as_str(), first["fix"]["command"].as_str()),
        (Some(u64::from(id)), Some("warning"), Some("hyperlink.set"))
    );
    // A row with an empty address and sub-address is not a link.
    edit_shape(&mut s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|sec| sec.name.as_str() != "Hyperlink");
        let mut section = Section::new("Hyperlink", None);
        let mut row = Row { name: Some(Arc::from("Row_9")), ..Row::default() };
        row.cells.insert("Address".into(), Cell::constant(Value::from_text("")));
        row.cells.insert("SubAddress".into(), Cell::constant(Value::from_text("")));
        row.cells.insert("Description".into(), Cell::constant(Value::from_text("")));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    });
    assert!(findings(&check(&mut s)).iter().all(|f| f["rule"] != "hyperlink_has_no_description"), "An empty row is not flagged");
}

#[test]
fn a_new_check_after_an_edit_shows_the_finding_gone() {
    let mut s = blank_session();
    plant(&mut s);
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let picture = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": picture - 1, "title": "Box"}));
    let first = check(&mut s);
    assert_eq!(first["errors"], 1);
    run(&mut s, "shape.alt_text", json!({"id": picture, "description": "Site photo"}));
    let second = check(&mut s);
    assert_eq!((second["errors"].as_u64(), errors(&second).len()), (Some(0), 0), "The finding is gone");
}

#[test]
fn a_check_of_one_page_looks_at_that_page_only() {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    let two = run(&mut s, "page.add", json!({"name": "Detail"}))["id"].as_u64().unwrap();
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let all = check(&mut s);
    assert_eq!(all["pages_checked"], 2);
    let only = run(&mut s, "review.check_accessibility", json!({"page": two}));
    assert_eq!(only["pages_checked"], 1);
    assert!(findings(&only).iter().all(|f| f["page"].as_u64() == Some(two)));
    let words = refused(&mut s, "review.check_accessibility", json!({"page": 99}));
    assert!(words.contains("99"), "{words}");
}

#[test]
fn a_check_is_a_query_that_changes_nothing_and_is_not_an_undo_step() {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = snapshot(&s);
    check(&mut s);
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_page_of_ten_thousand_and_one_shapes_lists_ten_thousand_and_says_the_list_is_cut() {
    let mut s = blank_session();
    s.edit("test: a big page", |d| {
        let at = d.pages.iter().position(|p| p.id == 1).unwrap();
        let page = Arc::make_mut(d.pages.get_mut(at).unwrap());
        for i in 0..10_001u32 {
            page.shapes.push(Arc::new(Shape::new(100 + i)));
        }
        Ok(())
    })
    .unwrap();
    let answer = check(&mut s);
    assert_eq!(findings(&answer).len(), 10_000, "V30");
    assert_eq!(answer["cut"], true);
    assert!(answer["cut_reason"].as_str().unwrap().contains("10000"), "{}", answer["cut_reason"]);
    // Exactly the cap and nothing left over is not cut.
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    s.edit("test: a page of ten thousand", |d| {
        let at = d.pages.iter().position(|p| p.id == 1).unwrap();
        let page = Arc::make_mut(d.pages.get_mut(at).unwrap());
        for i in 0..10_000u32 {
            page.shapes.push(Arc::new(Shape::new(100 + i)));
        }
        Ok(())
    })
    .unwrap();
    let answer = check(&mut s);
    assert_eq!((findings(&answer).len(), answer["cut"].as_bool()), (10_000, Some(false)));
}

#[test]
fn thirty_thousand_shapes_with_text_are_checked_with_no_finding() {
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    s.edit("test: a big page", |d| {
        let at = d.pages.iter().position(|p| p.id == 1).unwrap();
        let page = Arc::make_mut(d.pages.get_mut(at).unwrap());
        for i in 0..30_000u32 {
            let mut shape = Shape::new(100 + i);
            Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user_section(&[("visAltText", &[("Value", "A shape", "STR")])])));
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
    let answer = check(&mut s);
    assert_eq!((findings(&answer).len(), answer["shapes_checked"].as_u64(), answer["cut"].as_bool()), (0, Some(30_000), Some(false)));
}

#[test]
fn groups_nested_two_hundred_deep_are_checked_by_their_top_group_and_read_by_any_member() {
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    edit_shape(&mut s, id, |shape| {
        let mut inner = Shape::new(1_000);
        Arc::make_mut(&mut inner.sheet).sections.push(Arc::new(user_section(&[("visAltText", &[("Value", "deepest", "STR")])])));
        for depth in 1..198u32 {
            let mut group = Shape::new(1_000 + depth);
            group.kind = ShapeType::Group;
            group.children = ShapeList::from_vec(vec![Arc::new(inner)]);
            inner = group;
        }
        shape.kind = ShapeType::Group;
        shape.children = ShapeList::from_vec(vec![Arc::new(inner)]);
    });
    let answer = check(&mut s);
    assert_eq!(rules(&answer), [("alt_text_missing_on_shape".to_owned(), Some(u64::from(id)))], "the group answers for itself, whatever is in it");
    assert_eq!(alt(&mut s, 1_000)["title"], "deepest", "a member 200 levels down is read");
    let words = run(&mut s, "shape.alt_text", json!({"id": 1_000, "title": "set at the bottom"}));
    assert_eq!(words["title"], "set at the bottom");
    check_session(&s).unwrap();
}

#[test]
fn a_value_of_a_million_characters_in_a_file_is_answered_cut_and_checked_without_a_copy() {
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let huge = format!("{} diagram.png", "word ".repeat(200_000));
    give(&mut s, id, &[text_row("visAltText", &huge)]);
    let found = alt(&mut s, id);
    assert_eq!(found["title_length"].as_u64(), Some(huge.chars().count() as u64));
    assert_eq!(found["title"].as_str().unwrap().chars().count(), 32_768, "the answer holds at most the cap");
    let answer = check(&mut s);
    assert_eq!(rules(&answer), [("alt_text_is_file_name".to_owned(), Some(u64::from(id)))], "the whole value is read by the rule");
    // The other text of a sheet is kept when this one is read, and a text is written over it.
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "short"}));
    assert_eq!(alt(&mut s, id)["title"], "short");
    check_session(&s).unwrap();
}

#[test]
fn many_hyperlink_rows_are_read_to_a_cap_and_the_check_goes_on() {
    let mut s = blank_session();
    plant(&mut s);
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "A box"}));
    edit_shape(&mut s, id, |shape| {
        let mut section = Section::new("Hyperlink", None);
        for i in 0..5_000 {
            let mut row = Row { name: Some(Arc::from(format!("Row_{i}"))), ..Row::default() };
            row.cells.insert("Address".into(), Cell::constant(Value::from_text("https://example.org")));
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    let answer = check(&mut s);
    assert!(findings(&answer).is_empty(), "past the cap of rows none is read, and the check is whole all the same: {:?}", findings(&answer).len());
    assert_eq!(answer["cut"], false);
}

#[test]
fn the_check_and_the_commands_are_in_the_registry_with_their_menus() {
    let specs = diacraft_engine::command_specs();
    for id in ["shape.alt_text", "page.alt_text", "master.alt_text", "review.check_accessibility"] {
        assert!(specs.iter().any(|c| c.id == id), "{id}");
    }
    let keys = |id: &str| {
        diacraft_engine::param_keys(specs.iter().find(|c| c.id == id).unwrap().params).unwrap().iter().map(|k| (*k).to_owned()).collect::<Vec<_>>()
    };
    assert_eq!(keys("shape.alt_text"), ["id", "title", "description", "clear", "page"]);
    assert_eq!(keys("page.alt_text"), ["page", "title", "description", "clear"]);
    assert_eq!(keys("master.alt_text"), ["id", "shape", "title", "description", "clear"]);
    assert_eq!(keys("review.check_accessibility"), ["page"]);
    let check = specs.iter().find(|c| c.id == "review.check_accessibility").unwrap();
    assert_eq!(check.menu, ["Review", "Accessibility"]);
    assert!(!check.journal, "a check only reads");
}

// ---------------------------------------------------------------------------------------------------------------- locked layers

/// A page with one rectangle titled `Pump` on the locked layer `Frozen`. → the session and the shape.
fn locked_titled() -> (Session, u32) {
    locked_titled_after(0)
}

/// [`locked_titled`], with `others` rectangles dropped before it that are on no layer: the shape on the locked layer is numbered `others + 1`.
fn locked_titled_after(others: u32) -> (Session, u32) {
    let mut s = blank_session();
    for n in 0..others {
        drop_shape(&mut s, "rectangle", 6.0 + f64::from(n), 6.0);
    }
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    give_title(&mut s, id, "Pump");
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [id], "layers": ["Frozen"]}));
    // A locked layer takes no new shapes, so it is locked last.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, id)
}

/// A call that only reads is no edit of the shape it names: the registry's lock rule refused it because the command is journaled, so a program could not read the alt
/// text of a shape on a locked layer, which the window reads. A call that writes is refused as before, whichever way it writes.
#[test]
fn alt_text_is_read_on_a_locked_layer_and_a_write_is_refused() {
    let (mut s, id) = locked_titled();
    let before = snapshot(&s);
    let found = alt(&mut s, id);
    assert_eq!(
        (found["title"].as_str(), found["title_from"].as_str(), found["has_alt_text"].as_bool(), found["changed"].as_bool()),
        (Some("Pump"), Some("own"), Some(true), Some(false)),
        "a read answers about a shape on a locked layer"
    );
    assert_eq!(snapshot(&s).document, before.document, "and changes nothing");
    assert_eq!(undo_steps(&s), before.undo_steps, "and is no undo step");
    // A key that gives nothing to write leaves the call a read: `clear: false`, and a text of `null`.
    for params in [json!({"id": id, "clear": false}), json!({"id": id, "title": null, "description": null, "clear": null})] {
        let found = run(&mut s, "shape.alt_text", params.clone());
        assert_eq!((found["title"].as_str(), found["changed"].as_bool()), (Some("Pump"), Some(false)), "{params}");
    }
    // A write is refused in the words of the lock, whichever way it is given, and changes nothing.
    for params in [
        json!({"id": id, "title": "Other"}),
        json!({"id": id, "description": "Other"}),
        json!({"id": id, "clear": true}),
        json!({"id": id, "title": "", "description": ""}),
        json!({"id": id, "title": "Other", "clear": false}),
    ] {
        let words = refused(&mut s, "shape.alt_text", params.clone());
        assert!(words.contains("shape 1 ") && words.contains("\"Frozen\"") && words.contains("locked"), "{params}: {words}");
    }
    assert_eq!(alt(&mut s, id)["title"], "Pump", "the text is as it was");
    // Unlocked, the same write goes through.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    let written = run(&mut s, "shape.alt_text", json!({"id": id, "title": "Other"}));
    assert_eq!((written["title"].as_str(), written["changed"].as_bool()), (Some("Other"), Some(true)));
}

/// A page's alt text names no shape, so a locked layer on the page never stops it, to read or to write.
#[test]
fn a_pages_alt_text_is_read_and_written_with_a_locked_layer_on_the_page() {
    let (mut s, _) = locked_titled();
    assert_eq!(run(&mut s, "page.alt_text", json!({}))["has_alt_text"], false);
    let written = run(&mut s, "page.alt_text", json!({"title": "Plan"}));
    assert_eq!((written["title"].as_str(), written["changed"].as_bool()), (Some("Plan"), Some(true)));
}

/// A master's `id` is the master's, not a shape's: master 2 is read and written though shape 2 of the page is on a locked layer (the registry read the `id` as a shape of the page).
#[test]
fn a_masters_id_is_not_read_as_a_shape_on_a_locked_layer() {
    // Master 1 is the rectangle's own, which the dropped shapes are instances of: the master of the test is the next number, and the shape on the locked layer has it.
    let (mut s, id) = locked_titled_after(1);
    assert_eq!(id, 2);
    add_master(&mut s, id, &[text_row("visAltText", "Valve")], &[]);
    let found = run(&mut s, "master.alt_text", json!({"id": id}));
    assert_eq!((found["kind"].as_str(), found["title"].as_str()), (Some("master"), Some("Valve")), "the master is read");
    let written = run(&mut s, "master.alt_text", json!({"id": id, "title": "Valve V-2"}));
    assert_eq!((written["title"].as_str(), written["changed"].as_bool()), (Some("Valve V-2"), Some(true)), "and written");
    assert_eq!(alt(&mut s, id)["title"], "Pump", "the shape of the same number, on the locked layer, is as it was");
    check_session(&s).unwrap();
}

/// A text a GUARD protects, on a shape on a locked layer, is read like any other (the answer says it is protected); a write is refused, for the lock first.
#[test]
fn a_guarded_text_on_a_locked_layer_is_read() {
    let (mut s, id) = locked_titled();
    give(&mut s, id, &[("visAltDescription", vec![("Value", "=GUARD(Prop.Notes)|Long note", "STR")])]);
    let found = alt(&mut s, id);
    assert_eq!((found["description"].as_str(), found["description_protected"].as_bool()), (Some("Long note"), Some(true)));
    let words = refused(&mut s, "shape.alt_text", json!({"id": id, "description": "Other"}));
    assert!(words.contains("locked"), "the lock is the registry's, asked before the command runs: {words}");
}
