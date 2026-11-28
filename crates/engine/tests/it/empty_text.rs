//! **Empty text takes the text away, and what a shape's master says in its place is not drawn.** A shape that holds no `Text` of its own shows its master's (a member of a
//! group master and a line hold their master's placeholder until it is typed over), and a shape whose own `Text` has nothing in it shows nothing: the master's plays no part
//! `shape.set_text` with `""` and a range that takes every character away both left the shape with no `Text` at all, so the placeholder came back: a UML Interface whose
//! attributes were cleared still drew `+ attribute : Type`.
//!
//! * A compartment cleared of its text draws no text, keeps its least height and goes back to it after more lines were typed.
//! * Every master of every shipped stencil: each shape of its drop that shows text shows none after `shape.set_text ""`.
//! * A range that takes away every character of a text typed over a placeholder leaves the shape empty (the window's text box sends a range).
//! * `.diacraft`, `.vsdx` and `.odg` keep the empty text, and so does an undo and redo of it.
//! * A shape that inherits nothing keeps no `Text` for an empty text, as it always did.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef, TextPart};
use diacraft_engine::Session;
use diacraft_engine::stencil::all_masters;
use diacraft_render::{Rendered, Renderer, View};
use serde_json::json;

use crate::edit::{doc, id_of};
use crate::stencil_open::Dir;
use crate::{blank_session, run};

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    Renderer::new().render(doc, 1, &view)
}

fn dark(r: &Rendered, x: f64, y: f64) -> bool {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink(r: &Rendered, (left, bottom, right, top): (f64, f64, f64, f64)) -> usize {
    let mut n = 0;
    let step = 1.0 / PX_PER_INCH;
    let mut y = bottom;
    while y < top {
        let mut x = left;
        while x < right {
            n += usize::from(dark(r, x, y));
            x += step;
        }
        y += step;
    }
    n
}

fn first_page(d: &Document) -> u32 {
    d.pages.get(0).unwrap().id
}

/// What a cell of a shape of the first page is, by inheritance.
fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("shape {id}: {cell} holds no number"))
}

/// The characters a shape shows: its own `Text`, and when it has none its master shape's. Fields show their results.
fn shown(d: &Document, id: u32) -> String {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let own = d.shape(&at).unwrap().text.clone();
    let text = match own {
        Some(own) => Some(own),
        None => Resolver::new(d).master_step(&at).ok().flatten().and_then(|step| d.shape(&step.shape).and_then(|m| m.text.clone())),
    };
    text.iter()
        .flat_map(|t| t.parts.iter())
        .filter_map(|part| match part {
            TextPart::Chars(s) => Some(s.to_string()),
            TextPart::Fld { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// Whether a shape holds a `Text` of its own.
fn has_own_text(d: &Document, id: u32) -> bool {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    d.shape(&at).unwrap().text.is_some()
}

/// The id of a shape and of every shape below it, group first.
fn tree(d: &Document, id: u32) -> Vec<u32> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let mut out = vec![id];
    for child in d.shape(&at).unwrap().children.iter() {
        out.extend(tree(d, child.id));
    }
    out
}

/// The members of a group, as page `ID`s in the master's order.
fn members_of(d: &Document, id: u32) -> Vec<u32> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    d.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

/// The member of the group `id` that shows `text`.
fn member_showing(d: &Document, id: u32, text: &str) -> u32 {
    *members_of(d, id).iter().find(|m| shown(d, **m) == text).unwrap_or_else(|| panic!("no member shows {text:?}"))
}

/// The box of a member of a group in the page's inches (left, bottom, right, top), with the group dropped at scale 1.
fn page_box(d: &Document, group: u32, member: u32) -> (f64, f64, f64, f64) {
    let (gx, gy) = (num(d, group, "PinX") - num(d, group, "LocPinX"), num(d, group, "PinY") - num(d, group, "LocPinY"));
    let (x, y) = (num(d, member, "PinX") - num(d, member, "LocPinX"), num(d, member, "PinY") - num(d, member, "LocPinY"));
    (gx + x, gy + y, gx + x + num(d, member, "Width"), gy + y + num(d, member, "Height"))
}

/// The inside of a box, a tenth of an inch in from every edge: the place where only the text is ink.
fn inside((left, bottom, right, top): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (left + 0.1, bottom + 0.05, right - 0.1, top - 0.05)
}

fn set_text(s: &mut Session, id: u32, text: &str) {
    run(s, "shape.set_text", json!({"id": id, "text": text}));
}

fn drop_master(s: &mut Session, master: &str) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": master, "page": 1, "x": 5.0, "y": 5.0})))
}

const ATTRIBUTES: &str = "+ attribute : Type";
const OPERATIONS: &str = "+ operation()";

// ---- the compartment ----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_compartment_cleared_of_its_text_draws_no_text_and_keeps_its_least_height() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.interface");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    let operations = member_showing(doc(&s), class, OPERATIONS);
    let (height, class_height) = (num(doc(&s), attributes, "Height"), num(doc(&s), class, "Height"));
    let before = render(&s);
    assert!(ink(&before, inside(page_box(doc(&s), class, attributes))) > 20, "the placeholder is drawn before");
    set_text(&mut s, attributes, "");
    let d = doc(&s);
    assert_eq!(shown(d, attributes), "", "the attributes show nothing");
    assert_eq!(shown(d, operations), OPERATIONS, "the operations are as they were");
    let after = render(&s);
    assert_eq!(ink(&after, inside(page_box(d, class, attributes))), 0, "no text is drawn in the compartment");
    assert!(ink(&after, inside(page_box(d, class, operations))) > 20, "the other compartment's text is");
    // An empty compartment is as tall as its least height, as the compartment with one line was, and so is the class.
    assert!((num(d, attributes, "Height") - height).abs() < 1e-9, "{} against {height}", num(d, attributes, "Height"));
    assert!((num(d, class, "Height") - class_height).abs() < 1e-9);
    // The text and the height are one undo step: undo gives the placeholder back, redo takes it away again.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(shown(doc(&s), attributes), ATTRIBUTES);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(shown(doc(&s), attributes), "");
}

#[test]
fn a_compartment_that_grew_is_as_tall_as_its_least_height_again_once_it_is_cleared() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    let (height, class_height) = (num(doc(&s), attributes, "Height"), num(doc(&s), class, "Height"));
    set_text(&mut s, attributes, "one\ntwo\nthree\nfour\nfive");
    assert!(num(doc(&s), attributes, "Height") > height + 0.3, "five lines are taller than the least");
    set_text(&mut s, attributes, "");
    let d = doc(&s);
    assert_eq!(shown(d, attributes), "");
    assert!((num(d, attributes, "Height") - height).abs() < 1e-9, "the compartment is back at {height}: {}", num(d, attributes, "Height"));
    assert!((num(d, class, "Height") - class_height).abs() < 1e-9, "and so is the class: {}", num(d, class, "Height"));
}

#[test]
fn a_range_that_takes_away_all_the_text_typed_over_a_placeholder_leaves_the_compartment_empty() {
    // The window's text box opens on the characters the shape shows (the placeholder's eighteen) and sends the range that changed: typing over all of them and then deleting everything is `0..18` then `0..4`
    // (a range counts the characters the shape shows).
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "name", "start": 0, "end": 18}));
    assert_eq!(shown(doc(&s), attributes), "name");
    run(&mut s, "shape.set_text", json!({"id": attributes, "text": "", "start": 0, "end": 4}));
    assert_eq!(shown(doc(&s), attributes), "", "the placeholder does not come back");
    let r = render(&s);
    assert_eq!(ink(&r, inside(page_box(doc(&s), class, attributes))), 0);
}

#[test]
fn text_set_again_to_empty_changes_nothing_and_is_no_undo_step() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    set_text(&mut s, attributes, "");
    let (steps, held) = (s.history().unwrap().undo_len(), doc(&s).clone());
    set_text(&mut s, attributes, "");
    assert_eq!(s.history().unwrap().undo_len(), steps, "no step for text that is the text the shape has");
    assert_eq!(doc(&s), &held);
}

#[test]
fn typed_text_goes_to_a_cleared_compartment_and_the_markers_of_the_text_are_its_own() {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    set_text(&mut s, attributes, "");
    set_text(&mut s, attributes, "id : int");
    assert_eq!(shown(doc(&s), attributes), "id : int");
    set_text(&mut s, attributes, "");
    assert_eq!(shown(doc(&s), attributes), "");
}

#[test]
fn a_replace_with_nothing_that_takes_all_the_text_typed_over_a_placeholder_leaves_the_compartment_empty() {
    // Find and Replace rewrites the parts of a text and sets them again: with nothing left, the shape holds no text of its own and shows its master's, as `shape.set_text` did.
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.class");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    set_text(&mut s, attributes, "secret");
    let r = run(&mut s, "edit.replace", json!({"what": "secret", "with": ""}));
    assert_eq!(r["replaced"], json!(1), "{r}");
    assert_eq!(shown(doc(&s), attributes), "", "the placeholder does not come back");
    assert!(has_own_text(doc(&s), attributes));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(shown(doc(&s), attributes), "secret");
}

// ---- every master -------------------------------------------------------------------------------------------------------------------------

/// A shape of a master shows text it does not hold: the master's placeholder. After `shape.set_text ""` it shows none, in every stencil: UML Class and its kin, crow's foot and Chen's
/// entities, C4, BPMN, the use case and the rest, a group's members and a plain shape alike.
#[test]
fn every_shape_of_every_master_that_shows_a_placeholder_shows_nothing_once_its_text_is_set_to_empty() {
    let mut cleared = 0_usize;
    let mut failed: Vec<String> = Vec::new();
    for spec in all_masters() {
        let mut s = blank_session();
        let top = match s.execute("shape.drop", &json!({"master": spec.id, "page": 1, "x": 5.0, "y": 5.0})) {
            Ok(r) => id_of(&r),
            Err(_) => continue,
        };
        for id in tree(doc(&s), top) {
            if shown(doc(&s), id).is_empty() {
                continue;
            }
            cleared += 1;
            if let Err(e) = s.execute("shape.set_text", &json!({"id": id, "text": ""})) {
                failed.push(format!("{} shape {id}: {e}", spec.id));
                continue;
            }
            let left = shown(doc(&s), id);
            if !left.is_empty() {
                failed.push(format!("{} shape {id} still shows {left:?}", spec.id));
            }
        }
    }
    assert!(cleared > 60, "the sweep looked at {cleared} shapes with text");
    assert!(failed.is_empty(), "{} of {cleared} shapes did not clear: {failed:#?}", failed.len());
}

#[test]
fn a_plain_shape_of_a_master_with_text_shows_nothing_after_an_empty_text() {
    // A shape that is no member of a group: a use case, whose master gives it the words `Use Case`.
    let mut s = blank_session();
    let case = drop_master(&mut s, "uml-usecase.use-case");
    assert_eq!(shown(doc(&s), case), "Use Case", "the master has a label");
    let (x, y) = (num(doc(&s), case, "PinX"), num(doc(&s), case, "PinY"));
    let middle = (x - 0.3, y - 0.1, x + 0.3, y + 0.1);
    assert!(ink(&render(&s), middle) > 20, "the label is drawn before");
    set_text(&mut s, case, "");
    assert_eq!(shown(doc(&s), case), "");
    assert!(has_own_text(doc(&s), case), "an empty `Text` of its own is what says the master's plays no part");
    assert_eq!(ink(&render(&s), middle), 0, "no label is drawn in the middle of the shape");
}

#[test]
fn a_shape_that_inherits_no_text_keeps_no_text_for_an_empty_text() {
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 3})));
    assert!(!has_own_text(doc(&s), id));
    set_text(&mut s, id, "hello");
    assert!(has_own_text(doc(&s), id));
    set_text(&mut s, id, "");
    assert!(!has_own_text(doc(&s), id), "as it always was: no master text to override, so no `Text`");
}

// ---- the files ----------------------------------------------------------------------------------------------------------------------------

/// A drawing with an Interface whose attributes are cleared, saved as `name` and opened in a new session: its id and the session.
fn saved_and_reopened(name: &str) -> (Session, u32, Dir) {
    let mut s = blank_session();
    let class = drop_master(&mut s, "uml-class.interface");
    let attributes = member_showing(doc(&s), class, ATTRIBUTES);
    set_text(&mut s, attributes, "");
    let dir = Dir::new();
    let path = dir.path(name);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    (t, class, dir)
}

/// What every shape of the first page shows, in document order.
fn page_texts(d: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for shape in d.pages.get(0).unwrap().shapes.iter() {
        for id in tree(d, shape.id) {
            out.push(shown(d, id));
        }
    }
    out
}

fn assert_cleared(d: &Document, what: &str) {
    let texts = page_texts(d);
    assert!(!texts.iter().any(|t| t == ATTRIBUTES), "{what}: the placeholder is shown again: {texts:?}");
    assert!(texts.iter().any(|t| t == OPERATIONS), "{what}: the operations' text is kept: {texts:?}");
}

#[test]
fn a_cleared_compartment_stays_cleared_through_a_diacraft_file() {
    let (t, _, _dir) = saved_and_reopened("a.diacraft");
    assert_cleared(doc(&t), ".diacraft");
}

#[test]
fn a_cleared_compartment_stays_cleared_through_a_vsdx_file() {
    let (t, _, _dir) = saved_and_reopened("a.vsdx");
    assert_cleared(doc(&t), ".vsdx");
}

#[test]
fn a_cleared_compartment_stays_cleared_through_an_odg_file() {
    let (t, _, _dir) = saved_and_reopened("a.odg");
    assert_cleared(doc(&t), ".odg");
}

/// A cleared compartment in a file that already exists: the save in place splices the empty text in, and a reopen shows it empty (the second save of a drawing is not the first).
#[test]
fn a_cleared_compartment_saved_in_place_stays_cleared() {
    for name in ["b.vsdx", "b.diacraft", "b.odg"] {
        let dir = Dir::new();
        let path = dir.path(name);
        let mut s = blank_session();
        let class = drop_master(&mut s, "uml-class.interface");
        run(&mut s, "file.save_as", json!({"path": path}));
        let attributes = member_showing(doc(&s), class, ATTRIBUTES);
        set_text(&mut s, attributes, "");
        run(&mut s, "file.save", json!({}));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({"path": path}));
        assert_cleared(doc(&t), &format!("{name}, saved in place"));
    }
}

/// An empty `Text` element of an instance in a `.vsdx` file overrides the master's text, and the same holds of the other two formats: the file read and written again, to the
/// same type and in place, keeps the compartment empty.
#[test]
fn a_cleared_compartment_survives_a_second_round_in_each_format() {
    for name in ["c.vsdx", "c.diacraft", "c.odg"] {
        let (mut t, _, dir) = saved_and_reopened(name);
        assert_cleared(doc(&t), &format!("{name}: first reopen"));
        let again = dir.path(&format!("again-{name}"));
        run(&mut t, "file.save_as", json!({"path": again}));
        let mut u = Session::new();
        run(&mut u, "file.open", json!({"path": again}));
        assert_cleared(doc(&u), &format!("{name}: second reopen"));
        // The file opened is edited elsewhere in the drawing and saved in place: the empty text is not lost on the way.
        let group = doc(&u).pages.get(0).unwrap().shapes.iter().next().unwrap().id;
        let operations = member_showing(doc(&u), group, OPERATIONS);
        run(&mut u, "shape.set_text", json!({"id": operations, "text": "+ run()"}));
        run(&mut u, "file.save", json!({}));
        let mut v = Session::new();
        run(&mut v, "file.open", json!({"path": again}));
        let texts = page_texts(doc(&v));
        assert!(!texts.iter().any(|t| t == ATTRIBUTES) && texts.iter().any(|t| t == "+ run()"), "{name}: saved in place: {texts:?}");
    }
}
