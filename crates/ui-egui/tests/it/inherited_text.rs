//! **The text box opens on the text a shape shows.** A shape that holds no `Text` of its own shows its master's, and the box opened on the characters a shape held itself, so
//! a shape of a stencil that showed `Use Case` or `+ attribute : Type` opened an empty box, and the placeholder could not be seen, selected or deleted. This file is the other shapes of the
//! stencils the 0.2.4 pictures show (BPMN, C4, crow's foot, Chen, the use case and the class), and a group's label; `uml_class_text.rs` has the compartment of a class in every way a person
//! reaches it.

use diacraft_engine::doc::Owner;
use diacraft_engine::draw::text_view;
use diacraft_engine::stencil::all_masters;
use egui::{Key, Modifiers};
use serde_json::json;

use crate::editing::{edit_window, key, type_text};
use crate::support::{Window, run_command};
use crate::uml_class_text::{command, editing, has_own_text, page_of, shown};

/// The family of each master the sweep looks at: the notations of the 0.2.4 pictures that show text when they are dropped, and the class. (Chen's shapes are dropped with none: below.)
const FAMILIES: [&str; 5] = ["uml-class", "uml-usecase", "bpmn", "c4", "crows-foot"];

/// `id` and every shape below it, a group before its members.
fn tree(w: &Window, id: u32) -> Vec<u32> {
    let doc = w.state().session().document().unwrap();
    let page = page_of(w);
    let mut out = vec![id];
    let mut next = 0;
    while let Some(&current) = out.get(next) {
        if let Some(at) = doc.find_shape(Owner::Page(page), current) {
            out.extend(doc.shape(&at).unwrap().children.iter().map(|member| member.id));
        }
        next += 1;
    }
    out
}

/// What the box does for the shape `id` of the first page, selected and opened with F2: `None` when the box did not open over that shape (a group sends typing to a member, a member of a
/// group set to group only cannot be selected on its own); else the text it opened on and what was selected in it.
fn opened_on(w: &mut Window, id: u32) -> Option<(String, Option<(usize, usize)>)> {
    run_command(w, "edit.select", json!({"ids": [id]}));
    if w.state().selection() != [id] {
        return None;
    }
    key(w, Key::F2, Modifiers::NONE);
    if w.state().edited_text_shape() != Some(id) {
        key(w, Key::Escape, Modifiers::NONE);
        return None;
    }
    Some((w.state().edited_text().unwrap_or_default().to_owned(), w.state().edited_text_selection()))
}

/// Every shape of every master of the families that shows text, the master's (a compartment, a header) or its own (the label a shape is dropped with): the box opens on it with all of it selected,
/// one key takes it all away, the shape then shows nothing, and an undo gives the text back and leaves the shape as inheriting as it was.
#[test]
fn the_box_opens_on_the_text_every_shape_of_the_notation_stencils_shows() {
    let (mut opened, mut skipped, mut inheriting) = (0_usize, 0_usize, 0_usize);
    let mut failed: Vec<String> = Vec::new();
    let mut families: Vec<&str> = Vec::new();
    for spec in all_masters().filter(|spec| FAMILIES.iter().any(|f| spec.id.split('.').next() == Some(*f))) {
        let mut w = edit_window([1280.0, 900.0]);
        let page = page_of(&w);
        let dropped = command(&mut w, "shape.drop", json!({"master": spec.id, "page": page, "x": 4.0, "y": 6.0}));
        let top = u32::try_from(dropped["id"].as_u64().unwrap_or(0)).unwrap_or(0);
        for id in tree(&w, top) {
            let words = shown(&w, id);
            if words.is_empty() {
                continue;
            }
            let inherits = !has_own_text(&w, id);
            let name = format!("{} shape {id} showing {words:?}", spec.id);
            let Some((text, selected)) = opened_on(&mut w, id) else {
                skipped += 1;
                continue;
            };
            opened += 1;
            if let Some(family) = FAMILIES.iter().find(|f| spec.id.split('.').next() == Some(**f))
                && !families.contains(family)
            {
                families.push(family);
            }
            if text != words {
                failed.push(format!("{name}: the box opened on {text:?}"));
            }
            if selected != Some((0, words.chars().count())) {
                failed.push(format!("{name}: {selected:?} is selected"));
            }
            let revision = w.state().session().revision();
            key(&mut w, Key::Backspace, Modifiers::NONE);
            key(&mut w, Key::Escape, Modifiers::NONE);
            if editing(&w) || w.state().session().revision() != revision + 1 {
                failed.push(format!("{name}: deleting everything and ending the box is not one command"));
            }
            // A shape that showed its master's text holds an empty text of its own now; one that held its own and has no master text to show holds none.
            if !shown(&w, id).is_empty() || (inherits && !has_own_text(&w, id)) {
                failed.push(format!(
                    "{name}: it shows {:?} after everything was deleted, and holds a text of its own: {}",
                    shown(&w, id),
                    has_own_text(&w, id)
                ));
            }
            key(&mut w, Key::Z, Modifiers::COMMAND);
            if has_own_text(&w, id) == inherits || shown(&w, id) != words {
                failed.push(format!("{name}: undo did not bring the text back as it was"));
            }
            inheriting += usize::from(inherits);
            if w.state().ui_panics() != 0 {
                failed.push(format!("{name}: the window panicked"));
            }
        }
    }
    assert!(
        opened > 30 && inheriting > 10,
        "the sweep opened the box on {opened} shapes, {inheriting} showing their master's text ({skipped} could not be selected on their own)"
    );
    for family in FAMILIES {
        assert!(families.contains(&family), "the {family} stencil has a shape the box opened on: {families:?}");
    }
    assert!(failed.is_empty(), "{} of {opened} shapes did not edit: {failed:#?}", failed.len());
}

/// A use case shows the words `Use Case`: the box opens on them, Escape with nothing typed writes nothing, and typing on it selected replaces them.
#[test]
fn a_use_case_opens_on_its_words_and_typing_over_them_replaces_them() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_of(&w);
    let dropped = command(&mut w, "shape.drop", json!({"master": "uml-usecase.use-case", "page": page, "x": 4.0, "y": 6.0}));
    let case = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    assert_eq!(shown(&w, case), "Use Case");
    run_command(&mut w, "edit.select", json!({"ids": [case]}));
    let revision = w.state().session().revision();
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text(), Some("Use Case"));
    let inherits = !has_own_text(&w, case);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), revision, "nothing was typed, nothing was written");
    assert_eq!(has_own_text(&w, case), !inherits, "the shape holds what it held");
    // Typing on the selected shape opens the box and takes the key: the words are replaced.
    type_text(&mut w, "Pay");
    assert_eq!(w.state().edited_text(), Some("Pay"));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(shown(&w, case), "Pay");
    assert_eq!(text_view(w.state().session().document().unwrap(), page, case).unwrap().chars, "Pay");
    assert_eq!(w.state().session().revision(), revision + 1);
}

/// A member of a group that is selected on its own, and the group: each opens on the text it shows, and ending the box over one with nothing changed writes nothing to either.
#[test]
fn a_member_selected_on_its_own_opens_on_its_shown_text_and_the_group_does_not_take_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_of(&w);
    let dropped = command(&mut w, "shape.drop", json!({"master": "uml-class.class", "page": page, "x": 4.0, "y": 6.0}));
    let class = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    let members: Vec<u32> = {
        let doc = w.state().session().document().unwrap();
        let at = doc.find_shape(Owner::Page(page), class).unwrap();
        doc.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
    };
    let attributes = members[1];
    // The member alone.
    let (text, selected) = opened_on(&mut w, attributes).expect("the attributes can be selected on their own");
    assert_eq!(text, "+ attribute : Type");
    assert_eq!(selected, Some((0, 18)));
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The class, a group whose typing goes to the member on top of its stack (the name): the name's shown text, and the attributes untouched.
    run_command(&mut w, "edit.select", json!({"ids": [class]}));
    let revision = w.state().session().revision();
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text_shape(), Some(members[2]), "the typing of the class goes to its name");
    let name = shown(&w, members[2]);
    assert!(!name.is_empty(), "the name shows the master's word");
    assert_eq!(w.state().edited_text(), Some(name.as_str()), "and the box holds it");
    type_text(&mut w, "Account");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(shown(&w, members[2]), "Account");
    assert_eq!(shown(&w, attributes), "+ attribute : Type", "the attributes are as they were");
    assert!(!has_own_text(&w, attributes) && !has_own_text(&w, class));
    assert_eq!(w.state().session().revision(), revision + 1);
}

/// Chen's shapes are dropped with no text: the box opens empty, as it always did, and what is typed is the shape's own (the underline of a key attribute is the row's, found by inheritance).
#[test]
fn a_chen_shape_has_no_text_to_open_on_and_takes_what_is_typed() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_of(&w);
    for (at, master) in ["chen.entity", "chen.relationship", "chen.attribute", "chen.key-attribute"].iter().enumerate() {
        let dropped = command(&mut w, "shape.drop", json!({"master": master, "page": page, "x": 3.0 + 2.0 * at as f64, "y": 6.0}));
        let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
        assert_eq!(shown(&w, id), "", "{master}: no words are shown");
        let (text, selected) = opened_on(&mut w, id).unwrap_or_else(|| panic!("{master}: the box did not open"));
        assert_eq!((text.as_str(), selected), ("", Some((0, 0))), "{master}");
        type_text(&mut w, "Customer");
        key(&mut w, Key::Escape, Modifiers::NONE);
        assert_eq!(shown(&w, id), "Customer", "{master}");
        assert!(has_own_text(&w, id), "{master}");
    }
}
