//! **A shape that shows its master's text edits the text it shows.** A shape that holds no `Text` of its own shows its master's, so those are the characters an
//! editor opens on and a range counts: the text box of the window opened on the characters a shape held itself, and a UML compartment showing `+ attribute : Type` opened an empty box,
//! where the placeholder could not be seen, selected or deleted.
//!
//! * `shown_text_view` is the text the shape shows, `text_view` the text it holds.
//! * A range of `shape.set_text`, `text.insert`, `text.format` and `shape.formatting` counts the shown characters; a change gives the shape a `Text` of its own made of the master's parts with
//!   the change made, so the markers (the runs) of the master's text stay; a call that changes nothing writes nothing.
//! * Every master of every shipped stencil: each shape of its drop that shows text opens, is edited, and undoes to inheriting again.

use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, ShapeRef, SheetRef, TextPart};
use diacraft_engine::Session;
use diacraft_engine::draw::{set_text_range, shown_text_view, text_view};
use diacraft_engine::formatting::{Align, Direction, editor_paragraphs, look_of_range};
use diacraft_engine::stencil::all_masters;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::edit::{doc, id_of};
use crate::{blank_session, run};

const ATTRIBUTES: &str = "+ attribute : Type";
const OPERATIONS: &str = "+ operation()";

fn first_page(d: &Document) -> u32 {
    d.pages.get(0).unwrap().id
}

fn at_of(d: &Document, id: u32) -> ShapeRef {
    d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"))
}

/// The parts of the text a shape holds itself; `None` for a shape with no `Text`.
fn own_parts(d: &Document, id: u32) -> Option<Vec<TextPart>> {
    d.shape(&at_of(d, id)).unwrap().text.as_ref().map(|t| t.parts.clone())
}

/// The parts of the text the master of a shape gives it.
fn master_parts(d: &Document, id: u32) -> Option<Vec<TextPart>> {
    let at = at_of(d, id);
    let step = Resolver::new(d).master_step(&at).ok().flatten()?;
    d.shape(&step.shape)?.text.as_ref().map(|t| t.parts.clone())
}

/// The markers of a text, in order: the parts that are no characters and no fields.
fn markers(parts: &[TextPart]) -> Vec<TextPart> {
    parts.iter().filter(|p| matches!(p, TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_))).cloned().collect()
}

/// What a shape shows, characters and fields' results in order.
fn shown(d: &Document, id: u32) -> String {
    let parts = own_parts(d, id).or_else(|| master_parts(d, id)).unwrap_or_default();
    parts
        .iter()
        .filter_map(|part| match part {
            TextPart::Chars(s) => Some(s.to_string()),
            TextPart::Fld { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn drop_master(s: &mut Session, master: &str) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": master, "page": 1, "x": 5.0, "y": 5.0})))
}

/// The member of the group `id` that shows `text`.
fn member_showing(d: &Document, id: u32, text: &str) -> u32 {
    let at = at_of(d, id);
    *d.shape(&at)
        .unwrap()
        .children
        .iter()
        .map(|m| m.id)
        .collect::<Vec<_>>()
        .iter()
        .find(|m| shown(d, **m) == text)
        .unwrap_or_else(|| panic!("no member shows {text:?}"))
}

fn attributes_of_a_class(s: &mut Session) -> u32 {
    let class = drop_master(s, "uml-class.class");
    member_showing(doc(s), class, ATTRIBUTES)
}

// ---- what the editor opens on -------------------------------------------------------------------------------------------------------------

#[test]
fn the_shown_view_has_the_text_a_shape_shows_and_the_view_of_what_it_holds_has_none() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    let held = text_view(doc(&s), 1, attributes).unwrap();
    assert_eq!((held.chars.as_str(), held.fields.len()), ("", 0), "it holds no text of its own");
    assert_eq!(shown_text_view(doc(&s), 1, attributes).unwrap().chars, ATTRIBUTES, "and shows its master's");
    // A shape with a text of its own shows that, held or shown; an empty one shows none, and so does a shape with no master text.
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "own"}));
    assert_eq!(shown_text_view(doc(&s), 1, attributes).unwrap().chars, "own");
    assert_eq!(text_view(doc(&s), 1, attributes).unwrap().chars, "own");
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": ""}));
    assert_eq!(shown_text_view(doc(&s), 1, attributes).unwrap().chars, "", "an empty text of its own shows nothing: the master's plays no part");
    let plain = drop_master(&mut s, "rectangle");
    assert_eq!(shown_text_view(doc(&s), 1, plain).unwrap().chars, "");
    assert!(shown_text_view(doc(&s), 1, 999).is_none() && shown_text_view(doc(&s), 9, plain).is_none(), "a shape or page that is not there");
}

// ---- a range counts the shown characters -------------------------------------------------------------------------------------------------

#[test]
fn a_range_over_part_of_the_shown_text_gives_the_shape_the_masters_text_with_the_change_made() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    let master = master_parts(doc(&s), attributes).unwrap();
    let before = s.history().unwrap().undo_len();
    // `Type` is the last four of the eighteen characters.
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "Int", "start": 14, "end": 18}));
    assert_eq!(shown(doc(&s), attributes), "+ attribute : Int");
    let own = own_parts(doc(&s), attributes).expect("the shape holds the text now");
    assert_eq!(markers(&own), markers(&master), "the markers of the master's text are the shape's own: its runs are as they were shown");
    assert_eq!(s.history().unwrap().undo_len(), before + 1, "one undo step");
    // Undo gives the inherited text back: the shape holds none again.
    run(&mut s, "edit.undo", json!({}));
    assert!(own_parts(doc(&s), attributes).is_none(), "the shape inherits again");
    assert_eq!(shown(doc(&s), attributes), ATTRIBUTES);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(shown(doc(&s), attributes), "+ attribute : Int");
}

#[test]
fn a_point_in_the_shown_text_takes_the_characters_typed_there() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "- ", "start": 0, "end": 1}));
    assert_eq!(shown(doc(&s), attributes), "-  attribute : Type", "the plus is replaced by the minus");
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "s", "start": 19, "end": 19}));
    assert_eq!(shown(doc(&s), attributes), "-  attribute : Types");
}

#[test]
fn a_range_over_all_of_the_shown_text_with_nothing_leaves_a_text_of_its_own_with_nothing_in_it() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "", "start": 0, "end": 18}));
    assert_eq!(shown(doc(&s), attributes), "", "the placeholder does not come back");
    let own = own_parts(doc(&s), attributes).expect("an empty text of its own says the master's plays no part");
    assert!(own.iter().all(|p| !matches!(p, TextPart::Chars(_))), "{own:?}");
    run(&mut s, "edit.undo", json!({}));
    assert!(own_parts(doc(&s), attributes).is_none());
    assert_eq!(shown(doc(&s), attributes), ATTRIBUTES);
}

#[test]
fn a_range_that_changes_nothing_writes_nothing_and_the_shape_goes_on_inheriting() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    let (steps, held) = (s.history().unwrap().undo_len(), doc(&s).clone());
    for (start, end) in [(0, 0), (7, 7), (18, 18)] {
        run(&mut s, "shape.set_text", json!({"id": attributes, "text": "", "start": start, "end": end}));
    }
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert_eq!(doc(&s), &held, "the document is as it was");
    assert!(own_parts(doc(&s), attributes).is_none());
}

#[test]
fn a_range_the_shown_text_does_not_have_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    let held = doc(&s).clone();
    let error = s.execute("shape.set_text", &json!({"id": attributes, "text": "x", "start": 0, "end": 19})).unwrap_err().to_string();
    assert!(error.contains("not within the text, which has 18 characters"), "{error}");
    assert_eq!(doc(&s), &held);
    assert!(own_parts(doc(&s), attributes).is_none());
}

#[test]
fn text_inserted_with_no_place_goes_at_the_end_of_the_shown_text() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    run(&mut s, "text.insert", json!({"id": attributes, "text": "!"}));
    assert_eq!(shown(doc(&s), attributes), "+ attribute : Type!");
    run(&mut s, "text.insert", json!({"id": attributes, "text": ">", "at": 0}));
    assert_eq!(shown(doc(&s), attributes), ">+ attribute : Type!");
}

#[test]
fn the_formatting_of_a_range_of_the_shown_text_is_read_and_set_on_the_runs_it_shows() {
    let mut s = blank_session();
    let attributes = attributes_of_a_class(&mut s);
    let page = first_page(doc(&s));
    // Read: the range of a text the shape does not hold is the shown text's.
    let look = look_of_range(doc(&s), page, &[attributes], 0, 18).expect("a range of the shown text");
    assert!(!look.look.bold, "{look:?}");
    // Set: bold on the first word of what the shape shows. The shape holds the text then, with the runs split.
    run(&mut s, "text.format", json!({"ids": [attributes], "start": 0, "end": 1, "bold": true}));
    assert_eq!(shown(doc(&s), attributes), ATTRIBUTES, "the characters are as they were");
    assert!(own_parts(doc(&s), attributes).is_some(), "the runs are split in a text of the shape's own");
    let first = look_of_range(doc(&s), page, &[attributes], 0, 1).unwrap();
    let rest = look_of_range(doc(&s), page, &[attributes], 1, 18).unwrap();
    assert!(first.look.bold, "{first:?}");
    assert!(!rest.look.bold, "{rest:?}");
    let error = s.execute("text.format", &json!({"ids": [attributes], "start": 0, "end": 19, "bold": true})).unwrap_err().to_string();
    assert!(error.contains("not within the text, which has 18 characters"), "{error}");
}

// ---- what the box lays out ----------------------------------------------------------------------------------------------------------------

fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

/// A page with an instance, shape 1, of a master whose one shape holds `master` as its text, with `Paragraph` row 0 right to left; the instance holds `own`, if it is given.
fn instance_of(master: Vec<TextPart>, own: Option<Vec<TextPart>>) -> Document {
    let shape = ShapeBuilder::new(1).master(1);
    let shape = match own {
        Some(parts) => shape.text_parts(parts),
        None => shape,
    };
    let top = ShapeBuilder::new(5)
        .text_parts(master)
        .row_cell("Paragraph", None, RowKey::Ix(0), "Flags", "1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
        .row_cell("Paragraph", None, RowKey::Ix(1), "Flags", "0")
        .row_cell("Paragraph", None, RowKey::Ix(1), "HorzAlign", "2");
    DocBuilder::new().master(1, MasterBuilder::new().shape(top)).page(1, PageBuilder::new().shape(shape)).build()
}

/// The text box lays out the paragraphs of the text it edits: for a shape that shows its master's text, those of the master's, with an edit made to them as the commit makes it.
#[test]
fn the_paragraphs_the_box_lays_out_are_those_of_the_shown_text() {
    let master = vec![TextPart::Pp(0), chars("abc\n"), TextPart::Pp(1), chars("def")];
    let doc = instance_of(master.clone(), None);
    let paragraphs = editor_paragraphs(&doc, 1, 1, None).unwrap();
    assert_eq!(paragraphs.len(), 2, "one for the line feed and one more: the master's text, which the shape shows");
    assert_eq!(paragraphs[0].direction, Direction::RightToLeft);
    assert_eq!(paragraphs[0].align, Align::Left);
    assert_eq!(paragraphs[1].direction, Direction::LeftToRight);
    assert_eq!(paragraphs[1].align, Align::Right);
    // A line feed typed in the middle of the first paragraph splits it in two with its format.
    let paragraphs = editor_paragraphs(&doc, 1, 1, Some((1, 1, "\n"))).unwrap();
    assert_eq!(paragraphs.len(), 3);
    assert!(paragraphs[..2].iter().all(|p| p.direction == Direction::RightToLeft), "{paragraphs:?}");
    // A range the shown text does not have is refused, as it is for a text the shape holds.
    assert!(editor_paragraphs(&doc, 1, 1, Some((0, 8, ""))).is_err());
    // The shape's own text, when it holds one, is the text it shows, empty or not.
    let own = instance_of(master.clone(), Some(vec![]));
    assert_eq!(editor_paragraphs(&own, 1, 1, None).unwrap().len(), 1, "an empty text of its own shows none");
    let own = instance_of(master, Some(vec![TextPart::Pp(1), chars("x")]));
    assert_eq!(editor_paragraphs(&own, 1, 1, None).unwrap()[0].align, Align::Right);
}

/// The master's text, with its fields: a field is not typed over and stays in its place, the characters around it are the range's.
#[test]
fn a_field_of_the_masters_text_stays_where_it_is_while_the_characters_around_it_are_edited() {
    let master = vec![TextPart::Cp(0), TextPart::Pp(0), chars("ab"), TextPart::Fld { ix: 0, text: Arc::from("7") }, chars("cd")];
    let doc0 = instance_of(master.clone(), None);
    let view = shown_text_view(&doc0, 1, 1).unwrap();
    assert_eq!((view.chars.as_str(), view.fields.len(), view.fields[0].at, view.fields[0].text.as_str()), ("abcd", 1, 2, "7"));
    let mut doc = doc0;
    assert!(set_text_range(&mut doc, 1, 1, 1, 3, "XY").unwrap());
    let own = own_parts(&doc, 1).unwrap();
    assert_eq!(markers(&own), markers(&master));
    assert!(own.iter().any(|p| matches!(p, TextPart::Fld { ix: 0, .. })), "the field stays: {own:?}");
    assert_eq!(shown(&doc, 1), "aXY7d", "`bc` is replaced by `XY`, which joins the run of `b`, and the field stays between the two parts");
}

// ---- every master -------------------------------------------------------------------------------------------------------------------------

fn tree(d: &Document, id: u32) -> Vec<u32> {
    let at = at_of(d, id);
    let mut out = vec![id];
    for child in d.shape(&at).unwrap().children.iter() {
        out.extend(tree(d, child.id));
    }
    out
}

/// A text edit of every shape of every master that shows text: the editor's view has the characters the shape shows, taking them all away leaves it empty and the first edit is
/// one undo step that brings the master's text back; typing over them all puts the typed text there. The BPMN, C4, crow's foot and Chen stencils and the use case are among them.
#[test]
fn every_shape_of_every_master_that_shows_text_opens_on_it_and_is_edited_as_if_the_text_were_its_own() {
    let mut edited = 0_usize;
    let mut failed: Vec<String> = Vec::new();
    let mut stencils: Vec<String> = Vec::new();
    for spec in all_masters() {
        let mut s = blank_session();
        let top = match s.execute("shape.drop", &json!({"master": spec.id, "page": 1, "x": 5.0, "y": 5.0})) {
            Ok(r) => id_of(&r),
            Err(_) => continue,
        };
        let mut edited_here = false;
        for id in tree(doc(&s), top) {
            let words = shown(doc(&s), id);
            if words.is_empty() || own_parts(doc(&s), id).is_some() {
                continue;
            }
            edited += 1;
            edited_here = true;
            let name = format!("{} shape {id}", spec.id);
            // The editor opens on what the shape shows (the characters of the text; the results of fields are shown beside them).
            let view = shown_text_view(doc(&s), 1, id).unwrap();
            let plain: String = master_parts(doc(&s), id)
                .unwrap()
                .iter()
                .filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None })
                .collect();
            if view.chars != plain {
                failed.push(format!("{name}: the view has {:?}, the shape shows {plain:?}", view.chars));
                continue;
            }
            let n = view.chars.chars().count();
            // A call that changes nothing writes nothing.
            let _ = s.execute("shape.set_text", &json!({"id": id, "text": "", "start": 0, "end": 0}));
            if own_parts(doc(&s), id).is_some() {
                failed.push(format!("{name}: an empty point wrote a text"));
                continue;
            }
            // All of it replaced by one letter, then undone.
            if let Err(e) = s.execute("shape.set_text", &json!({"id": id, "text": "Q", "start": 0, "end": n})) {
                failed.push(format!("{name}: {e}"));
                continue;
            }
            let held: String = shown_text_view(doc(&s), 1, id).unwrap().chars;
            if held != "Q" {
                failed.push(format!("{name}: typed over all of it, the box would open on {held:?}"));
            }
            if own_parts(doc(&s), id).is_none() {
                failed.push(format!("{name}: no text of its own after the edit"));
            }
            run(&mut s, "edit.undo", json!({}));
            if own_parts(doc(&s), id).is_some() || shown(doc(&s), id) != words {
                failed.push(format!("{name}: undo did not bring the inherited text back"));
            }
            // All of it taken away.
            if let Err(e) = s.execute("shape.set_text", &json!({"id": id, "text": "", "start": 0, "end": n})) {
                failed.push(format!("{name}: {e}"));
                continue;
            }
            if !shown(doc(&s), id).is_empty() || own_parts(doc(&s), id).is_none() {
                failed.push(format!("{name}: not empty after everything was taken away"));
            }
        }
        let family = spec.id.split('.').next().unwrap_or_default().to_owned();
        if edited_here && !stencils.contains(&family) {
            stencils.push(family);
        }
    }
    assert!(edited > 20, "the sweep looked at {edited} shapes with text");
    assert!(failed.is_empty(), "{} of {edited} shapes did not edit: {failed:#?}", failed.len());
    // The masters that show a placeholder of their own and hold none in the shape dropped (the others, BPMN's and Chen's shapes and the use case among them, hold the label they are dropped with: the
    // window's sweep opens the box on those too).
    for wanted in ["uml-class", "uml-sequence", "c4", "crows-foot"] {
        assert!(stencils.iter().any(|s| s == wanted), "the sweep reached the {wanted} stencil: {stencils:?}");
    }
}

/// A cell of a shape, by inheritance, as a number: the editing of the text moved nothing it did not have to.
fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at_of(d, id)), &CellAddr::direct(cell)).unwrap();
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

#[test]
fn the_first_edit_of_a_compartment_moves_nothing_while_the_text_is_what_it_was() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    let (height, class_height) = (num(doc(&s), attributes, "Height"), num(doc(&s), class, "Height"));
    // The last character is replaced by itself with another letter and back: the text is what it was, and the sizes are too.
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "X", "start": 17, "end": 18}));
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "e", "start": 17, "end": 18}));
    assert_eq!(shown(doc(&s), attributes), ATTRIBUTES);
    assert!((num(doc(&s), attributes, "Height") - height).abs() < 1e-9);
    assert!((num(doc(&s), class, "Height") - class_height).abs() < 1e-9);
    assert_eq!(shown(doc(&s), member_showing(doc(&s), class, OPERATIONS)), OPERATIONS);
}

// ---- groups -------------------------------------------------------------------------------------------------------------------------------

/// A page with a group, shape 1, of a master whose top shape is a group with the label `Label` and a member with the text `Inner`; the group's member on the page is shape 2.
fn labelled_group() -> Document {
    let label = vec![TextPart::Cp(0), TextPart::Pp(0), chars("Label")];
    let inner = vec![TextPart::Cp(0), TextPart::Pp(0), chars("Inner")];
    let top = ShapeBuilder::new(5).group().text_parts(label).child(ShapeBuilder::new(6).text_parts(inner));
    let group = ShapeBuilder::new(1).master(1).group().child(ShapeBuilder::new(2).master_shape(6));
    DocBuilder::new().master(1, MasterBuilder::new().shape(top)).page(1, PageBuilder::new().shape(group)).build()
}

/// A group shows the label its master gives it, and a member of it the text of the master's member: each is edited on its own, as if the text were its own, and the other is not touched.
#[test]
fn a_group_and_its_member_edit_the_labels_they_show_each_on_its_own() {
    let mut doc = labelled_group();
    assert_eq!(shown_text_view(&doc, 1, 1).unwrap().chars, "Label", "the group shows its master's label");
    assert_eq!(shown_text_view(&doc, 1, 2).unwrap().chars, "Inner", "its member shows the master's member's text");
    assert_eq!(text_view(&doc, 1, 1).unwrap().chars, "", "and neither holds text");
    // The member: `Inner` becomes `Innermost`.
    assert!(set_text_range(&mut doc, 1, 2, 5, 5, "most").unwrap());
    assert_eq!(shown_text_view(&doc, 1, 2).unwrap().chars, "Innermost");
    assert!(own_parts(&doc, 1).is_none(), "the group still inherits its label");
    assert_eq!(markers(&own_parts(&doc, 2).unwrap()), [TextPart::Cp(0), TextPart::Pp(0)], "the member holds the markers of the master's member");
    // The group: all of the label is taken away, and the group holds an empty text of its own.
    assert!(set_text_range(&mut doc, 1, 1, 0, 5, "").unwrap());
    assert_eq!(shown_text_view(&doc, 1, 1).unwrap().chars, "");
    assert!(own_parts(&doc, 1).is_some());
    assert_eq!(shown_text_view(&doc, 1, 2).unwrap().chars, "Innermost", "the member is as it was");
}

// ---- the files ----------------------------------------------------------------------------------------------------------------------------

/// The shape of the first page's drop that shows `words`, anywhere in its tree.
fn showing(d: &Document, words: &str) -> Option<u32> {
    d.pages.get(0).unwrap().shapes.iter().flat_map(|shape| tree(d, shape.id)).find(|id| shown(d, *id) == words)
}

/// A class's name is bold and centred by the rows of its master's text. Typed over, the shape holds the text with the master's markers, so it is the same run, before the edit, after it and after
/// a save and a reopen in each format; and a second edit of the reopened file goes on from there.
#[test]
fn an_edited_placeholder_keeps_the_run_it_showed_through_the_three_files() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let name = member_showing(doc(&s), class, "Class");
    let page = first_page(doc(&s));
    let before = look_of_range(doc(&s), page, &[name], 0, 5).unwrap();
    assert!(before.look.bold, "a class's name is bold: {before:?}");
    run(&mut s, "shape.set_text", json!({"id": name, "text": "Account", "start": 0, "end": 5}));
    let after = look_of_range(doc(&s), page, &[name], 0, 7).unwrap();
    assert_eq!(
        (after.look.bold, after.look.size, after.look.align),
        (before.look.bold, before.look.size, before.look.align),
        "the run is the one the master's text showed"
    );
    for file in ["a.diacraft", "a.vsdx", "a.odg"] {
        let dir = crate::stencil_open::Dir::new();
        let path = dir.path(file);
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({"path": path}));
        let id = showing(doc(&t), "Account").unwrap_or_else(|| panic!("{file}: the typed name is shown after a reopen"));
        let again = look_of_range(doc(&t), first_page(doc(&t)), &[id], 0, 7).unwrap();
        assert_eq!((again.look.bold, again.look.size), (before.look.bold, before.look.size), "{file}: the same run after a reopen");
        assert!(showing(doc(&t), ATTRIBUTES).is_some(), "{file}: the compartment that was not typed in still shows its placeholder");
    }
}
