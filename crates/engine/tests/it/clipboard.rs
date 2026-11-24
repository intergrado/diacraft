//! The clipboard commands: `edit.copy`, `edit.cut`, `edit.paste` and `shape.duplicate`, through `Session::execute` as the control channel
//! calls it. Numbers are paper inches on pages of the default scale.
//!
//! A paste makes shapes with new `ID`s that are the shapes copied moved together: the corners of each are the original's plus the offset, a
//! connector copied with the shapes it joins is glued to the copies, and the originals are not touched.

use diacraft_doc::{CellAddr, Document, Lookup, Owner, Shape, ShapeRef};
use diacraft_engine::draw::shape_corners;
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::edit_foreign::{Dir, opened};
use crate::{Snapshot, bad_params_msg, blank_session, check_session, run, snapshot};

type P = (f64, f64);

const TOL: f64 = 1.0e-8;

fn near_p(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < TOL && (a.1 - b.1).abs() < TOL
}

fn corners(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}"))
}

fn shifted(c: [P; 4], by: P) -> [P; 4] {
    c.map(|p| (p.0 + by.0, p.1 + by.1))
}

fn assert_corners(what: &str, d: &Document, id: u32, want: [P; 4]) {
    let got = corners(d, id);
    assert!(got.iter().zip(&want).all(|(g, w)| near_p(*g, *w)), "{what}: shape {id} has corners {got:?}, want {want:?}");
}

fn top(d: &Document) -> Vec<u32> {
    d.page(1).unwrap().shapes.iter().map(|s| s.id).collect()
}

fn at(d: &Document, id: u32) -> ShapeRef {
    d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))
}

fn shape_of(d: &Document, id: u32) -> &Shape {
    d.shape(&at(d, id)).unwrap()
}

fn local(d: &Document, id: u32, name: &str) -> Option<(Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

/// Where an end of a connector is on the page (its cells are in the frame of the page: the connector is not in a group).
fn end(d: &Document, id: u32, which: &str) -> P {
    (num(d, id, &format!("{which}X")), num(d, id, &format!("{which}Y")))
}

/// The `Connect` rows of page 1 as (from shape, from cell, to shape, to part).
fn connects(d: &Document) -> Vec<(u32, String, u32, u32)> {
    d.page(1)
        .unwrap()
        .connects
        .iter()
        .map(|c| (c.from_sheet, c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_sheet, c.to_part.unwrap_or(0)))
        .collect()
}

fn ids_of(result: &Json) -> Vec<u32> {
    result["ids"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect()
}

fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

fn assert_unchanged(s: &Session, before: &Snapshot, what: &str) {
    let now = snapshot(s);
    assert_eq!(now.document, before.document, "{what}: the document");
    assert_eq!((now.undo_steps, now.redo_steps, now.revision), (before.undo_steps, before.redo_steps, before.revision), "{what}: no step");
}

const PX_PER_INCH: f64 = 100.0;

fn render(s: &Session) -> Rendered {
    let d = s.document().unwrap();
    Renderer::new().render(d, 1, &View::fit_page(d, 1, PX_PER_INCH).unwrap())
}

/// Rectangle 1 at (2, 2), ellipse 2 at (6, 4), rectangle 3 at (9, 6) and a connector (4) from 1 to 2.
fn joined() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "ellipse", 6.0, 4.0), 2);
    assert_eq!(drop_shape(&mut s, "rectangle", 9.0, 6.0), 3);
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 4);
    s
}

// ---- copy ------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_copy_puts_the_shapes_on_the_clipboard_and_changes_nothing_else() {
    let mut s = joined();
    assert!(s.clipboard().is_none());
    let before = snapshot(&s);
    let r = run(&mut s, "edit.copy", json!({"ids": [1, 3]}));
    assert_eq!(r, json!({"page": 1, "copied": 2}));
    let clip = s.clipboard().unwrap();
    assert_eq!((clip.len(), clip.shape_count(), clip.source_page(), clip.pastes()), (2, 2, 1, 0));
    let now = snapshot(&s);
    assert_eq!(now.document, before.document, "the document is as it was");
    assert_eq!(
        (now.undo_steps, now.redo_steps, now.revision),
        (before.undo_steps, before.redo_steps, before.revision),
        "no undo step, nothing to redraw"
    );
    assert_eq!(now.journal.len(), before.journal.len() + 1, "the call is in the journal: replayed, it puts the same shapes aside");
    // The box of the shapes copied, on the page.
    let (x0, y0, x1, y1) = clip.bounds();
    assert!((x0 - 1.25).abs() < TOL && (y0 - 1.5).abs() < TOL && (x1 - 9.75).abs() < TOL && (y1 - 6.5).abs() < TOL, "{:?}", clip.bounds());
    // A second copy replaces the first.
    run(&mut s, "edit.copy", json!({"ids": [2]}));
    assert_eq!(s.clipboard().unwrap().len(), 1);
}

#[test]
fn a_copy_of_shapes_that_are_not_there_or_of_nothing_is_refused_and_leaves_the_clipboard() {
    let mut s = joined();
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    let before = snapshot(&s);
    let e = error_of(&mut s, "edit.copy", &json!({"ids": [1, 99]}));
    assert!(bad_params_msg(&e).contains("no shape 99"), "{e}");
    assert!(matches!(error_of(&mut s, "edit.copy", &json!({"ids": []})), EngineError::BadParams { .. }));
    assert!(matches!(error_of(&mut s, "edit.copy", &json!({"ids": [1], "page": 5})), EngineError::BadParams { .. }));
    assert_unchanged(&s, &before, "refused copies");
    assert_eq!(s.clipboard().unwrap().len(), 1, "the clipboard holds the first copy still");
}

// ---- paste -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_paste_puts_copies_a_step_right_and_down_with_new_ids_on_top_and_leaves_the_originals() {
    let mut s = joined();
    let (c1, c3) = (corners(doc(&s), 1), corners(doc(&s), 3));
    let masters = doc(&s).masters.len();
    run(&mut s, "edit.copy", json!({"ids": [1, 3]}));
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "edit.paste", json!({}));
    assert_eq!(r["page"], 1);
    assert_eq!(r["created"], 2);
    let ids = ids_of(&r);
    assert_eq!(ids, [5, 6], "new ids above the highest of the page, in the order of the drawing");
    let d = doc(&s);
    assert_eq!(top(d), [1, 2, 3, 4, 5, 6], "on top, in order");
    assert_corners("copy of 1", d, 5, shifted(c1, (0.25, -0.25)));
    assert_corners("copy of 3", d, 6, shifted(c3, (0.25, -0.25)));
    assert_corners("original 1", d, 1, c1);
    assert_corners("original 3", d, 3, c3);
    assert_eq!(d.masters.len(), masters, "the masters the copies use are the document's, not a second copy of them");
    assert_eq!(shape_of(d, 5).master, shape_of(d, 1).master);
    assert_eq!(shape_of(d, 5).name.as_deref(), Some("Rectangle.5"), "a name that ends in the id ends in the copy's");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Paste"));
    assert_eq!(s.clipboard().unwrap().pastes(), 1);
    // A second paste of the same copy is another step on.
    let again = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(again, [7, 8]);
    assert_corners("second paste", doc(&s), 7, shifted(c1, (0.5, -0.5)));
    // One undo takes the second paste back, and the very document returns.
    let before_second = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(top(doc(&s)), [1, 2, 3, 4, 5, 6]);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &before_second);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_paste_at_a_place_puts_the_middle_of_the_copies_there() {
    let mut s = joined();
    let c1 = corners(doc(&s), 1);
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    // The copy's middle is (2, 2) on the page: to (7, 7) is a move of (5, 5).
    let ids = ids_of(&run(&mut s, "edit.paste", json!({"x": 7.0, "y": 7.0})));
    assert_corners("pasted at a place", doc(&s), ids[0], shifted(c1, (5.0, 5.0)));
    // x and y go together.
    let e = error_of(&mut s, "edit.paste", &json!({"x": 1.0}));
    assert!(bad_params_msg(&e).contains("go together"), "{e}");
    assert!(matches!(error_of(&mut s, "edit.paste", &json!({"x": 1.0e9, "y": 0.0})), EngineError::BadParams { .. }));
}

#[test]
fn a_paste_with_nothing_on_the_clipboard_is_not_enabled() {
    let mut s = joined();
    let info = s.commands().into_iter().find(|i| i.id == "edit.paste").unwrap();
    assert!(!info.enabled && info.disabled_reason.as_deref() == Some("the clipboard is empty"), "{info:?}");
    let before = snapshot(&s);
    assert!(matches!(error_of(&mut s, "edit.paste", &json!({})), EngineError::Disabled { .. }));
    assert_unchanged(&s, &before, "a paste with nothing to paste");
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    assert!(s.commands().into_iter().find(|i| i.id == "edit.paste").unwrap().enabled);
}

#[test]
fn a_paste_into_another_page_works_and_the_first_one_is_where_the_shapes_were() {
    let mut s = joined();
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = doc(&s).pages.iter().nth(1).unwrap().id;
    let c1 = corners(doc(&s), 1);
    run(&mut s, "edit.copy", json!({"ids": [1, 2, 4]}));
    let r = run(&mut s, "edit.paste", json!({"page": second}));
    assert_eq!(r["page"], second);
    let d = doc(&s);
    let ids = ids_of(&r);
    assert_eq!(ids, [1, 2, 3], "the ids of a page are its own: the second page has none yet");
    let page = d.page(second).unwrap();
    assert_eq!(page.shapes.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2, 3]);
    let moved = shape_corners(d, second, 1).unwrap();
    assert!(moved.iter().zip(&c1).all(|(a, b)| near_p(*a, *b)), "the first paste on another page is where the shapes were");
    // The connector between them is glued to its copies on that page, and the first page is as it was.
    let rows: Vec<(u32, String, u32, u32)> =
        page.connects.iter().map(|c| (c.from_sheet, c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_sheet, c.to_part.unwrap_or(0))).collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|(from, _, to, _)| *from == 3 && (*to == 1 || *to == 2)), "{rows:?}");
    assert_eq!(top(d), [1, 2, 3, 4]);
    // The next paste on that page is a step on.
    let again = ids_of(&run(&mut s, "edit.paste", json!({"page": second})));
    assert_eq!(again, [4, 5, 6]);
    let step = shape_corners(doc(&s), second, 4).unwrap();
    assert!(step.iter().zip(&c1).all(|(a, b)| near_p(*a, (b.0 + 0.25, b.1 - 0.25))));
    let e = error_of(&mut s, "edit.paste", &json!({"page": 77}));
    assert!(bad_params_msg(&e).contains("no page 77"), "{e}");
}

#[test]
fn a_connector_pasted_on_another_page_without_its_shape_has_a_plain_end_though_nothing_moved() {
    let mut s = joined();
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = doc(&s).pages.iter().nth(1).unwrap().id;
    let (b0, e0) = (end(doc(&s), 4, "Begin"), end(doc(&s), 4, "End"));
    // The connector and the shape at its begin: the first paste on another page is where they were, so no cell is written to move them.
    run(&mut s, "edit.copy", json!({"ids": [1, 4]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({"page": second})));
    assert_eq!(ids, [1, 2]);
    let d = doc(&s);
    let copy = d.find_shape(Owner::Page(second), 2).unwrap();
    let sheet = &d.shape(&copy).unwrap().sheet;
    let formula = |name: &str| match sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => c.formula.text().map(str::to_owned),
        _ => None,
    };
    assert!(formula("BeginX").unwrap().starts_with("PAR(PNT(Sheet.1!"), "glued to the copy of 1");
    assert_eq!(formula("EndX"), None, "a plain point at the end");
    assert_eq!(formula("EndY"), None);
    let num_of = |name: &str| match sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => c.value.as_ref().and_then(|v| v.number()).unwrap(),
        _ => panic!("{name}"),
    };
    assert!(near_p((num_of("BeginX"), num_of("BeginY")), b0) && near_p((num_of("EndX"), num_of("EndY")), e0), "where they were");
    assert_eq!(d.page(second).unwrap().connects.len(), 1);
}

#[test]
fn a_connector_copied_with_both_its_shapes_is_glued_to_the_copies_and_follows_them() {
    let mut s = joined();
    let (b0, e0) = (end(doc(&s), 4, "Begin"), end(doc(&s), 4, "End"));
    let f0 = connects(doc(&s));
    run(&mut s, "edit.copy", json!({"ids": [4, 1, 2]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [5, 6, 7], "in the order of the drawing: the shapes, then the connector");
    let d = doc(&s);
    // Glued to 5 and 6, the copies of 1 and 2, by formulas that name them and by Connect rows.
    let begin = local(d, 7, "BeginX").unwrap().0.unwrap();
    let finish = local(d, 7, "EndX").unwrap().0.unwrap();
    assert!(begin.starts_with("PAR(PNT(Sheet.5!") && finish.starts_with("PAR(PNT(Sheet.6!"), "{begin} / {finish}");
    let rows = connects(d);
    assert_eq!(&rows[..f0.len()], &f0[..], "the originals' rows are as they were");
    let new: Vec<_> = rows[f0.len()..].to_vec();
    assert_eq!(new.len(), 2);
    assert!(new.iter().any(|(from, cell, to, _)| (*from, cell.as_str(), *to) == (7, "BeginX", 5)));
    assert!(new.iter().any(|(from, cell, to, _)| (*from, cell.as_str(), *to) == (7, "EndX", 6)));
    // The ends are the originals' moved by the offset, and the originals did not move.
    assert!(near_p(end(d, 7, "Begin"), (b0.0 + 0.25, b0.1 - 0.25)) && near_p(end(d, 7, "End"), (e0.0 + 0.25, e0.1 - 0.25)));
    assert!(near_p(end(d, 4, "Begin"), b0) && near_p(end(d, 4, "End"), e0));
    // The copy follows its shape; the original does not.
    run(&mut s, "shape.move", json!({"ids": [6], "dx": 1.0, "dy": 0.0}));
    let d = doc(&s);
    assert!(end(d, 7, "End").0 > e0.0 + 0.25 + 0.5, "the copy's end moved with its shape");
    assert!(near_p(end(d, 4, "End"), e0), "the original connector stayed with the original shape");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_copied_without_a_shape_it_is_glued_to_has_a_plain_end_there() {
    let mut s = joined();
    let (b0, e0) = (end(doc(&s), 4, "Begin"), end(doc(&s), 4, "End"));
    run(&mut s, "edit.copy", json!({"ids": [4, 1]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [5, 6]);
    let d = doc(&s);
    // Glued to the copy of 1 at the begin; at the end a point, where the end was, moved with the rest.
    assert_eq!(local(d, 6, "EndX").unwrap().0, None);
    assert_eq!(local(d, 6, "EndY").unwrap().0, None);
    assert!(local(d, 6, "BeginX").unwrap().0.unwrap().starts_with("PAR(PNT(Sheet.5!"));
    let rows = connects(d);
    assert_eq!(rows.iter().filter(|(from, ..)| *from == 6).count(), 1);
    assert!(rows.contains(&(6, "BeginX".to_owned(), 5, 100 + (rows.iter().find(|r| r.0 == 6).unwrap().3 - 100))));
    assert!(near_p(end(d, 6, "Begin"), (b0.0 + 0.25, b0.1 - 0.25)) && near_p(end(d, 6, "End"), (e0.0 + 0.25, e0.1 - 0.25)));
    // The originals are as they were: the connector is still glued to 1 and 2.
    assert!(near_p(end(d, 4, "Begin"), b0) && near_p(end(d, 4, "End"), e0));
    assert_eq!(rows.iter().filter(|(from, ..)| *from == 4).count(), 2);
    // The free end is a free end: moving the copy of 1 leaves it where it is.
    let free = end(d, 6, "End");
    run(&mut s, "shape.move", json!({"ids": [5], "dx": 0.0, "dy": 1.0}));
    assert!(near_p(end(doc(&s), 6, "End"), free));
    // A connector alone: both ends plain points.
    run(&mut s, "edit.copy", json!({"ids": [4]}));
    let alone = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    assert_eq!(connects(doc(&s)).iter().filter(|(from, ..)| *from == alone).count(), 0);
    assert!(near_p(end(doc(&s), alone, "Begin"), (b0.0 + 0.25, b0.1 - 0.25)));
}

#[test]
fn the_copy_of_a_shape_is_a_picture_of_the_original_moved() {
    let mut s = joined();
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Hello"}));
    let before = render(&s);
    run(&mut s, "edit.copy", json!({"ids": [1, 2, 3, 4]}));
    run(&mut s, "edit.paste", json!({"x": 5.5, "y": 4.25}));
    let after = render(&s);
    assert!(after.skipped.is_empty(), "{:?}", after.skipped);
    // The originals are drawn as they were: the page's pixels where nothing was pasted are the same.
    let (w, h) = (before.width as usize, before.height as usize);
    let mut same = 0usize;
    let mut total = 0usize;
    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * 4;
            total += 1;
            same += usize::from(before.rgba_premul[i..i + 4] == after.rgba_premul[i..i + 4]);
        }
    }
    assert!(same * 10 > total * 6, "{same} of {total} pixels are as they were");
    // Pasted a whole page to the right and nowhere near the page's edge, it is as much ink again.
    let ink =
        |r: &Rendered| (0..r.width as usize * r.height as usize).filter(|i| r.rgba_premul[i * 4 + 3] > 128 && r.rgba_premul[i * 4] < 100).count();
    assert!(ink(&after) > ink(&before) * 3 / 2, "{} against {}", ink(&after), ink(&before));
}

#[test]
fn text_fields_and_formatting_copy_with_the_shape() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Hello, world"}));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "big ", "start": 7, "end": 7}));
    let (text, cells): (Option<std::sync::Arc<diacraft_doc::Text>>, Vec<_>) = {
        let shape = shape_of(doc(&s), a);
        (shape.text.clone(), shape.sheet.cells.iter().map(|(n, c)| (n.as_str().to_owned(), c.clone())).collect())
    };
    run(&mut s, "edit.copy", json!({"ids": [a]}));
    let b = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    let copy = shape_of(doc(&s), b);
    assert_eq!(copy.text, text, "the text, its parts and its runs");
    for (name, original) in cells.iter().filter(|(n, _)| n != "PinX" && n != "PinY") {
        assert_eq!(copy.sheet.cells.get(&diacraft_doc::CellName::new(name)), Some(original), "{name}");
    }
    // The copy is edited alone.
    run(&mut s, "shape.set_text", json!({"id": b, "text": "other"}));
    assert_eq!(shape_of(doc(&s), a).text, text);
}

// ---- cut and duplicate -----------------------------------------------------------------------------------------------------------

#[test]
fn a_cut_copies_and_deletes_in_one_step_and_a_paste_after_it_brings_the_shapes_back_with_new_ids() {
    let mut s = joined();
    let (c1, c2) = (corners(doc(&s), 1), corners(doc(&s), 2));
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "edit.cut", json!({"ids": [1, 2]}));
    assert_eq!(r, json!({"page": 1, "cut": 2}));
    let d = doc(&s);
    assert_eq!(top(d), [3, 4], "the connector stays, with its ends plain points");
    assert!(d.page(1).unwrap().connects.is_empty());
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Cut"));
    assert_eq!(s.clipboard().unwrap().len(), 2);
    // The shapes come back at the same place on the page they were cut from?: a step on, since they were on this page.
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [5, 6], "4 is the highest on the page, and 1 and 2 were given");
    assert_corners("pasted after a cut", doc(&s), 5, shifted(c1, (0.25, -0.25)));
    assert_corners("pasted after a cut", doc(&s), 6, shifted(c2, (0.25, -0.25)));
    // One undo takes the paste back and one more the cut: the very document.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(top(doc(&s)), [1, 2, 3, 4]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_cut_of_a_shape_with_a_connector_to_it_leaves_the_connector_with_a_plain_end_as_a_delete_does() {
    let mut a = joined();
    let mut b = joined();
    run(&mut a, "edit.cut", json!({"ids": [2]}));
    run(&mut b, "shape.delete", json!({"ids": [2]}));
    assert_eq!(doc(&a), doc(&b), "a cut is a delete after the copy");
}

#[test]
fn a_duplicate_copies_and_pastes_on_the_page_at_once_and_leaves_the_clipboard_alone() {
    let mut s = joined();
    let c1 = corners(doc(&s), 1);
    run(&mut s, "edit.copy", json!({"ids": [3]}));
    let held = s.clipboard().unwrap().clone();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.duplicate", json!({"ids": [1, 2, 4]}));
    assert_eq!(ids_of(&r), [5, 6, 7]);
    assert_eq!(r["created"], 3);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Duplicate shapes"));
    assert_corners("duplicate", doc(&s), 5, shifted(c1, (0.25, -0.25)));
    assert_eq!(s.clipboard().unwrap().len(), held.len());
    assert_eq!(s.clipboard().unwrap().source_page(), 1);
    assert_eq!(s.clipboard().unwrap().pastes(), 0, "the clipboard is as it was");
    // The glue is to the duplicates.
    assert!(local(doc(&s), 7, "BeginX").unwrap().0.unwrap().starts_with("PAR(PNT(Sheet.5!"));
    // An offset of its own.
    let again = ids_of(&run(&mut s, "shape.duplicate", json!({"ids": [1], "dx": 2.0, "dy": 1.0})))[0];
    assert_corners("duplicate by an offset", doc(&s), again, shifted(c1, (2.0, 1.0)));
    let e = error_of(&mut s, "shape.duplicate", &json!({"ids": [99]}));
    assert!(bad_params_msg(&e).contains("no shape 99"), "{e}");
}

// ---- ids and masters -------------------------------------------------------------------------------------------------------------

#[test]
fn pasted_shapes_never_get_an_id_a_shape_has_had_in_the_session() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 2.0);
    run(&mut s, "edit.copy", json!({"ids": [a]}));
    let first = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    assert_eq!(first, b + 1);
    run(&mut s, "shape.delete", json!({"ids": [first]}));
    let second = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    assert_eq!(second, first + 1, "the id of the deleted copy is not given again");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let third = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    assert!(third > second, "{third}");
    let d = doc(&s);
    let mut seen = std::collections::HashSet::new();
    assert!(d.page(1).unwrap().shapes.iter().all(|shape| seen.insert(shape.id)));
}

#[test]
fn a_master_a_paste_needs_is_put_back_when_an_undo_took_it_away_and_is_never_copied_twice() {
    let mut s = blank_session();
    assert_eq!(doc(&s).masters.len(), 0);
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let master = shape_of(doc(&s), a).master.unwrap();
    run(&mut s, "edit.copy", json!({"ids": [a]}));
    // Undoing the drop takes the master and the shape; the clipboard still holds the shape and the master.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).masters.len(), 0);
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    let d = doc(&s);
    assert_eq!(d.masters.len(), 1, "the master is back");
    assert_eq!(shape_of(d, ids[0]).master, Some(master));
    // Pasting again, and dropping the same master again, make no second one.
    run(&mut s, "edit.paste", json!({}));
    drop_shape(&mut s, "rectangle", 8.0, 2.0);
    assert_eq!(doc(&s).masters.len(), 1);
    // A master of another id, dropped after the undo, is not mistaken for the one the clipboard holds.
    let mut t = blank_session();
    let a = drop_shape(&mut t, "rectangle", 2.0, 2.0);
    run(&mut t, "edit.copy", json!({"ids": [a]}));
    run(&mut t, "edit.undo", json!({}));
    let other = drop_shape(&mut t, "ellipse", 6.0, 2.0);
    let ellipse = shape_of(doc(&t), other).master.unwrap();
    assert_eq!(ellipse, master, "the ellipse took the id the rectangle's master had");
    let pasted = ids_of(&run(&mut t, "edit.paste", json!({})))[0];
    let d = doc(&t);
    assert_eq!(d.masters.len(), 2, "the rectangle's master is put back under an id of its own");
    assert_ne!(shape_of(d, pasted).master, Some(ellipse));
    assert_eq!(d.master(shape_of(d, pasted).master.unwrap()).unwrap().name.as_deref(), Some("Rectangle"));
}

#[test]
fn the_clipboard_goes_with_the_document_and_stays_through_undo() {
    let mut s = joined();
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.clipboard().is_some(), "an undo does not empty it");
    run(&mut s, "file.new", json!({}));
    assert!(s.clipboard().is_none(), "a new drawing starts with none: a paste is within one document");
    assert!(!s.commands().into_iter().find(|i| i.id == "edit.paste").unwrap().enabled);
}

// ---- groups ----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_group_is_copied_with_its_members_and_each_gets_an_id() {
    let mut s = joined();
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 4]})));
    let members: Vec<u32> = shape_of(doc(&s), g).children.iter().map(|m| m.id).collect();
    assert_eq!(members, [1, 2, 4]);
    let (c1, c2) = (corners(doc(&s), 1), corners(doc(&s), 2));
    run(&mut s, "edit.copy", json!({"ids": [g, 1]}));
    assert_eq!(s.clipboard().unwrap().len(), 1, "a member named with its group goes with the group");
    assert_eq!(s.clipboard().unwrap().shape_count(), 4);
    let r = run(&mut s, "edit.paste", json!({}));
    assert_eq!(r["created"], 4);
    let ids = ids_of(&r);
    assert_eq!(ids, [g + 1]);
    let d = doc(&s);
    let copy: Vec<u32> = shape_of(d, g + 1).children.iter().map(|m| m.id).collect();
    assert_eq!(copy, [g + 2, g + 3, g + 4], "the members have ids of their own");
    // The copies draw where the originals did, a step on; the glue inside the copy is to the copy's members.
    assert_corners("a copy of a member", d, g + 2, shifted(c1, (0.25, -0.25)));
    assert_corners("a copy of a member", d, g + 3, shifted(c2, (0.25, -0.25)));
    assert!(local(d, g + 4, "BeginX").unwrap().0.unwrap().starts_with(&format!("PAR(PNT(Sheet.{}!", g + 2)));
    assert_corners("the original", d, 1, c1);
    assert_eq!(d.page(1).unwrap().connects.len(), 4);
    // Moved, the copy's connector follows the copy's shapes.
    run(&mut s, "shape.move", json!({"ids": [g + 1], "dx": 2.0, "dy": 0.0}));
    assert_corners("the original stays", doc(&s), 1, c1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_of_a_group_is_copied_alone_as_a_shape_of_the_page_that_draws_as_it_did() {
    for turn in [None, Some((90.0, None)), Some((35.0, Some("horizontal"))), Some((0.0, Some("vertical")))] {
        let mut s = joined();
        let g = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 3]})));
        if let Some((by, flip)) = turn {
            if by != 0.0 {
                run(&mut s, "shape.rotate", json!({"ids": [g], "by": by}));
            }
            if let Some(axis) = flip {
                run(&mut s, "shape.flip", json!({"ids": [g], "axis": axis}));
            }
        }
        let c2 = corners(doc(&s), 2);
        run(&mut s, "edit.copy", json!({"ids": [2]}));
        let r = run(&mut s, "edit.paste", json!({"page": 1}));
        let copy = ids_of(&r)[0];
        let d = doc(&s);
        assert!(top(d).contains(&copy), "{turn:?}: the copy is on the page and not in the group");
        assert_corners(&format!("{turn:?}"), d, copy, shifted(c2, (0.25, -0.25)));
        // It is a shape of the page: it turns like one.
        run(&mut s, "shape.rotate", json!({"ids": [copy], "by": 10}));
        assert_eq!(check_session(&s), Ok(()), "{turn:?}");
    }
}

// ---- a drawing from a file -------------------------------------------------------------------------------------------------------

/// The cells of a shape as text, for comparing a copy with its original: every cell of the sheet with its formula, value, unit and error.
fn cells_of(shape: &Shape) -> Vec<(String, String)> {
    shape
        .sheet
        .cells
        .iter()
        .map(|(name, c)| {
            (
                name.as_str().to_owned(),
                format!("{:?} {:?} {:?} {:?}", c.formula.text(), c.value.as_ref().map(|v| v.text().to_owned()), c.unit, c.error),
            )
        })
        .collect()
}

/// The `Geometry` sections of a shape, whole.
fn geometry_of(shape: &Shape) -> Vec<diacraft_doc::Section> {
    shape.sheet.sections.iter().filter(|section| section.name.as_str() == "Geometry").map(|section| (**section).clone()).collect()
}

#[test]
fn every_shape_and_connector_of_a_file_is_copied_glued_to_the_copies_by_both_kinds_of_glue() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let originals: Vec<Shape> = [10, 11, 12, 13, 20, 21, 22, 23].iter().map(|id| shape_of(doc(&s), *id).clone()).collect();
    let before_rows = connects(doc(&s));
    let ends: Vec<(P, P)> = [20, 21, 22, 23].iter().map(|id| (end(doc(&s), *id, "Begin"), end(doc(&s), *id, "End"))).collect();
    run(&mut s, "edit.copy", json!({"ids": [10, 11, 12, 13, 20, 21, 22, 23]}));
    let r = run(&mut s, "edit.paste", json!({}));
    assert_eq!(ids_of(&r), [24, 25, 26, 27, 28, 29, 30, 31], "above the highest id of the file, in order");
    let d = doc(&s);
    // The originals are exactly as they were.
    for original in &originals {
        assert_eq!(shape_of(d, original.id), original, "shape {} is untouched", original.id);
    }
    assert_eq!(&connects(d)[..before_rows.len()], &before_rows[..]);
    // 28 is 20's copy: glued by points to 24 and 25.
    assert_eq!(local(d, 28, "BeginX").unwrap().0.as_deref(), Some("PAR(PNT(Sheet.24!Connections.X1,Sheet.24!Connections.Y1))"));
    assert_eq!(local(d, 28, "EndX").unwrap().0.as_deref(), Some("PAR(PNT(Sheet.25!Connections.X2,Sheet.25!Connections.Y2))"));
    // 29 is 21's copy: glued to the whole of 26 and 27, with the triggers that name them.
    assert_eq!(local(d, 29, "BeginX").unwrap().0.as_deref(), Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"));
    assert_eq!(local(d, 29, "BegTrigger").unwrap().0.as_deref(), Some("_XFTRIGGER(Sheet.26!EventXFMod)"));
    assert_eq!(local(d, 29, "EndTrigger").unwrap().0.as_deref(), Some("_XFTRIGGER(Sheet.27!EventXFMod)"));
    let rows = connects(d);
    let new: Vec<(u32, String, u32, u32)> = rows[before_rows.len()..].to_vec();
    let want = [
        (28, "BeginX", 24, 100),
        (28, "EndX", 25, 101),
        (29, "BeginX", 26, 3),
        (29, "EndX", 27, 3),
        (30, "BeginX", 24, 102),
        (30, "EndX", 26, 103),
        (31, "BeginX", 25, 100),
        (31, "EndX", 27, 3),
    ];
    for (from, cell, to, part) in want {
        assert!(new.contains(&(from, cell.to_owned(), to, part)), "{from} {cell} {to} {part} in {new:?}");
    }
    assert_eq!(new.len(), want.len());
    // The copies are the originals a step on: the ends, the walked ones too, and the rows of the routes.
    for (k, (b, e)) in ends.iter().enumerate() {
        let id = 28 + u32::try_from(k).unwrap();
        assert!(near_p(end(d, id, "Begin"), (b.0 + 0.25, b.1 - 0.25)) && near_p(end(d, id, "End"), (e.0 + 0.25, e.1 - 0.25)), "connector {id}");
    }
    for (original, copy) in [(20, 28), (21, 29), (22, 30), (23, 31)] {
        let (a, b) = (shape_of(d, original), shape_of(d, copy));
        assert_eq!(geometry_of(a), geometry_of(b), "the route of {copy} is the route of {original}");
        assert_eq!(a.text, b.text, "the label of {copy}");
    }
    // The copies follow their own shapes: moving 24 moves the ends glued to it, in the copies only.
    run(&mut s, "shape.move", json!({"ids": [24, 26], "dx": 0.5, "dy": 0.5}));
    let d = doc(&s);
    assert!(near_p(end(d, 28, "Begin"), (ends[0].0.0 + 0.75, ends[0].0.1 + 0.25)), "{:?}", end(d, 28, "Begin"));
    assert!(near_p(end(d, 20, "Begin"), ends[0].0), "the original connector did not move");
    assert_eq!(check_session(&s), Ok(()));
    // And the drawing is saved and read back.
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    assert_eq!(top(doc(&t)).len(), 16);
    assert_eq!(connects(doc(&t)).len(), 16);
}

/// A page with a picture (shape 7) beside a box (shape 6).
fn drawing_with_a_picture() -> Vec<u8> {
    use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape};
    let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
    let plain = shape(6, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let picture = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="6"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{plain}{picture}")).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png))
        .build()
}

#[test]
fn a_picture_is_copied_on_its_page_and_saves_and_is_not_pasted_on_another_page() {
    let dir = Dir::new();
    let path = dir.path("pic.vsdx");
    std::fs::write(&path, drawing_with_a_picture()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    let second = doc(&s).pages.iter().nth(1).unwrap().id;
    run(&mut s, "edit.copy", json!({"ids": [6, 7]}));
    // On another page it is refused, whole: the picture's relationship belongs to the page it is on.
    let before = snapshot(&s);
    let e = error_of(&mut s, "edit.paste", &json!({"page": second}));
    assert!(e.to_string().contains("picture"), "{e}");
    assert_unchanged(&s, &before, "refused");
    assert_eq!(s.clipboard().unwrap().pastes(), 0);
    // On its own page it is a second shape with the same picture, and the file saves and reads back with both.
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [8, 9]);
    let d = doc(&s);
    let (a, b) = (shape_of(d, 7), shape_of(d, 9));
    assert_eq!(a.foreign, b.foreign, "the same picture");
    assert!(b.foreign.as_ref().unwrap().picture.is_some());
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    let d = doc(&t);
    assert_eq!(top(d), [6, 7, 8, 9]);
    assert!(shape_of(d, 9).foreign.as_ref().unwrap().picture.is_some() && shape_of(d, 7).foreign.as_ref().unwrap().picture.is_some());
    // A picture can be duplicated, cut and pasted back, and grouped.
    run(&mut t, "shape.duplicate", json!({"ids": [7]}));
    let cut = run(&mut t, "edit.cut", json!({"ids": [9]}));
    assert_eq!(cut["cut"], 1);
    run(&mut t, "edit.paste", json!({}));
    let again = dir.path("again.vsdx");
    run(&mut t, "file.save_as", json!({"path": again}));
    assert!(std::fs::metadata(&again).unwrap().len() > 0);
}

/// A group master (4, holding the box 5) and an instance of it (10, holding 11, which stands for 5), as a file holds them.
fn drawing_with_a_group_master() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
    let inner = shape(
        5,
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row></Section>"#,
    );
    let master = shape_with(
        4,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="2" F="Width*0.5"/><Cell N="PinY" V="1" F="Height*0.5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/><Shapes>{inner}</Shapes>"#
        ),
    );
    let instance = shape_with(
        10,
        r#" Master="7" Type="Group""#,
        r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Shapes><Shape ID="11" MasterShape="5" Type="Shape"/></Shapes>"#,
    );
    VsdxBuilder::new().master(MasterSpec::new(7, "Pair").shapes(&master)).page(PageSpec::new(1, "Page-1").shapes(&instance)).build()
}

#[test]
fn a_connector_in_a_turned_group_is_copied_alone_with_its_ends_on_the_page_and_one_in_a_flipped_group_is_not() {
    let mut s = joined();
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 4]})));
    run(&mut s, "shape.rotate", json!({"ids": [g], "by": 35}));
    // The ends of the connector on the page: through the group's pose.
    let on_page = |d: &Document, id: u32, which: &str| {
        let pose = diacraft_engine::draw::Pose {
            pin: (num(d, g, "PinX"), num(d, g, "PinY")),
            loc: (num(d, g, "LocPinX"), num(d, g, "LocPinY")),
            angle: num(d, g, "Angle"),
            flip_x: false,
            flip_y: false,
        };
        pose.up(end(d, id, which))
    };
    let (b, e) = (on_page(doc(&s), 4, "Begin"), on_page(doc(&s), 4, "End"));
    run(&mut s, "edit.copy", json!({"ids": [4]}));
    let copy = ids_of(&run(&mut s, "edit.paste", json!({})))[0];
    let d = doc(&s);
    assert!(
        near_p(end(d, copy, "Begin"), (b.0 + 0.25, b.1 - 0.25)) && near_p(end(d, copy, "End"), (e.0 + 0.25, e.1 - 0.25)),
        "on the page, a step on"
    );
    assert_eq!(top(d).last(), Some(&copy), "a shape of the page");
    assert_eq!(connects(d).iter().filter(|c| c.0 == copy).count(), 0, "its glue was to shapes not copied");
    // In a flipped group it is refused, whole, and the clipboard keeps the copy before.
    run(&mut s, "shape.flip", json!({"ids": [g], "axis": "vertical"}));
    let before = snapshot(&s);
    let e = error_of(&mut s, "edit.copy", &json!({"ids": [4]}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("flipped group"), "{e}");
    assert_unchanged(&s, &before, "a refused copy");
    assert_eq!(s.clipboard().unwrap().len(), 1);
    // A shape of it that is not a connector is copied.
    run(&mut s, "edit.copy", json!({"ids": [1]}));
}

#[test]
fn a_member_of_a_group_made_from_a_master_is_copied_with_its_group_and_not_alone() {
    let dir = Dir::new();
    let path = dir.path("pair.vsdx");
    std::fs::write(&path, drawing_with_a_group_master()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let c11 = corners(doc(&s), 11);
    assert!((c11[0].0 - 3.5).abs() < TOL && (c11[2].0 - 4.5).abs() < TOL, "the member is drawn from the master: {c11:?}");
    let before = snapshot(&s);
    let e = error_of(&mut s, "edit.copy", &json!({"ids": [11]}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("copied with its group"), "{e}");
    assert_unchanged(&s, &before, "refused");
    // The group is copied whole: the copy's member still stands for the master's box and is drawn where the member was, a step on.
    run(&mut s, "edit.copy", json!({"ids": [10]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [12]);
    let d = doc(&s);
    assert_eq!(shape_of(d, 12).master, shape_of(d, 10).master);
    let member = shape_of(d, 12).children.iter().next().unwrap();
    assert_eq!((member.id, member.master_shape), (13, Some(5)));
    assert_corners("the member of the copy", d, 13, shifted(c11, (0.25, -0.25)));
    // Ungrouping an instance would cut its members off from the master they take their cells from: refused.
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.ungroup", &json!({"ids": [10]}));
    assert!(matches!(e, EngineError::Failed { .. }) && e.to_string().contains("instance of a master"), "{e}");
    assert_unchanged(&s, &before, "refused");
    // A group of instances is grouped and ungrouped as any.
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [10, 12]})));
    assert_corners("grouped", doc(&s), 11, c11);
    run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    assert_corners("ungrouped", doc(&s), 11, c11);
}

/// A drawing with what a real file holds on a shape: a master, style links, a name, two ids as attributes, text of two runs with a field, `Data1`
/// to `Data3`, and two shapes whose cells name each other.
fn drawing_with_what_a_file_holds() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
    let master = shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let rich = shape_with(
        5,
        r#" Master="7" Name="Box.5" NameU="Box.5" UniqueID="{AAAAAAAA-0000-4000-8000-000000000005}" OriginalID="9" LineStyle="3" FillStyle="3" TextStyle="3""#,
        r##"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/>
<Section N="Character"><Row IX="0"><Cell N="Size" V="0.1667" U="PT"/></Row><Row IX="1"><Cell N="Style" V="1"/><Cell N="Color" V="#FF0000"/></Row></Section>
<Section N="Field"><Row IX="0"><Cell N="Value" V="3" F="PAGENUMBER()"/></Row></Section>
<Text><cp IX="0"/>ab<cp IX="1"/>cd<fld IX="0">3</fld></Text>
<Data1>one</Data1><Data2>two</Data2><Data3>three</Data3>"##,
    );
    let other = shape_with(
        6,
        r#" Master="7" Name="Box.6" NameU="Box.6""#,
        r#"<Cell N="PinX" V="5" F="Sheet.5!PinX+3"/><Cell N="PinY" V="2"/><Cell N="Width" V="2" F="Sheet.5!Width"/>
<Cell N="Height" V="1" F="Sheet.5!Height*1"/>"#,
    );
    VsdxBuilder::new()
        .more_styles(
            r##"<StyleSheet ID="3" NameU="Fancy" Name="Fancy"><Cell N="LineWeight" V="0.05"/><Cell N="LineColor" V="#FF0000"/></StyleSheet>"##,
        )
        .master(MasterSpec::new(7, "Box").shapes(&master).attrs(r#" UniqueID="{BBBBBBBB-0000-4000-8000-000000000007}""#))
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{rich}{other}")))
        .build()
}

#[test]
fn what_a_real_file_holds_on_a_shape_is_copied_with_it_and_the_copy_saves() {
    let dir = Dir::new();
    let path = dir.path("rich.vsdx");
    std::fs::write(&path, drawing_with_what_a_file_holds()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let masters = doc(&s).masters.len();
    run(&mut s, "edit.copy", json!({"ids": [5, 6]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({})));
    assert_eq!(ids, [7, 8]);
    let d = doc(&s);
    let (a, b) = (shape_of(d, 5), shape_of(d, 7));
    assert_eq!(a.text, b.text, "two runs and a field");
    assert_eq!((&a.data1, &a.data2, &a.data3), (&b.data1, &b.data2, &b.data3));
    assert_eq!(b.data1.as_deref(), Some("one"));
    assert_eq!((a.sheet.links, a.master, &a.master_shape), (b.sheet.links, b.master, &b.master_shape));
    assert_eq!(b.sheet.links.line, Some(3));
    assert_eq!(d.masters.len(), masters, "the master is used, not copied");
    assert_eq!(b.raw.get("UniqueID"), None, "a UniqueID names one shape");
    assert_eq!(b.raw.get("OriginalID"), Some("9"));
    assert_eq!((b.name.as_deref(), b.name_u.as_deref()), (Some("Box.7"), Some("Box.7")));
    // Every cell of the copy is the original's but the pin, which was written to move it.
    let (ca, cb) = (cells_of(a), cells_of(b));
    assert_eq!(
        ca.iter().filter(|(n, _)| !n.starts_with("Pin")).collect::<Vec<_>>(),
        cb.iter().filter(|(n, _)| !n.starts_with("Pin")).collect::<Vec<_>>()
    );
    // The cells that name the other shape name its copy; the pin that was a formula is a number now, as after a drag.
    assert_eq!(local(d, 8, "Width").unwrap().0.as_deref(), Some("Sheet.7!Width"));
    assert_eq!(local(d, 8, "Height").unwrap().0.as_deref(), Some("Sheet.7!Height*1"));
    assert_eq!(local(d, 8, "PinX").unwrap().0, None);
    assert!((num(d, 8, "PinX") - 5.25).abs() < TOL && (num(d, 8, "PinY") - 1.75).abs() < TOL);
    // The original still names the original.
    assert_eq!(local(d, 6, "Width").unwrap().0.as_deref(), Some("Sheet.5!Width"));
    // The drawing saves, and is read back with the copy's text, data and links.
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    let (a, b) = (shape_of(doc(&t), 5), shape_of(doc(&t), 7));
    assert_eq!(a.text, b.text);
    assert_eq!((&a.data1, &a.data2, &a.data3), (&b.data1, &b.data2, &b.data3));
    assert_eq!(b.sheet.links.line, Some(3));
    assert_eq!(local(doc(&t), 8, "Width").unwrap().0.as_deref(), Some("Sheet.7!Width"));
}

/// Page 1 holds a box (5, 4 wide), a shape (6) whose cells name it (`Width` is `Sheet.5!Width*0.5`, `PinX` is `Sheet.5!PinX+3`), a connector (7) glued
/// to it by a `PAR` formula and a trigger, and a twin (8) whose `Width` names 6. Page 2 holds a box of the same `ID`, 5, nine wide, which a formula
/// copied there that still said `Sheet.5!` would read.
fn drawing_whose_cells_name_each_other() -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let boxed = |id: u32, x: f64, width: f64| {
        shape(id, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="{width}"/><Cell N="Height" V="1"/>"#))
    };
    let five = boxed(5, 2.0, 4.0);
    let six = shape(
        6,
        r#"<Cell N="PinX" V="5" F="Sheet.5!PinX+3"/><Cell N="PinY" V="2"/><Cell N="Width" V="2" F="Sheet.5!Width*0.5"/><Cell N="Height" V="1"/>"#,
    );
    let connector = shape(
        7,
        r#"<Cell N="BeginX" V="4" F="PAR(PNT(Sheet.5!PinX,Sheet.5!PinY))"/><Cell N="BeginY" V="2" F="PAR(PNT(Sheet.5!PinX,Sheet.5!PinY))"/><Cell N="EndX" V="8"/><Cell N="EndY" V="2"/>
<Cell N="PinX" V="6" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Cell N="BegTrigger" V="2" F="_XFTRIGGER(Sheet.5!EventXFMod)"/>"#,
    );
    let twin = shape(8, r#"<Cell N="PinX" V="9"/><Cell N="PinY" V="2"/><Cell N="Width" V="1" F="Sheet.6!Width*0.5"/><Cell N="Height" V="1"/>"#);
    let connects = r#"<Connect FromSheet="7" FromCell="BeginX" FromPart="9" ToSheet="5" ToCell="PinX" ToPart="3"/>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{five}{six}{connector}{twin}")).connects(connects))
        .page(PageSpec::new(2, "Page-2").shapes(&boxed(5, 3.0, 9.0)))
        .build()
}

fn formula_on(d: &Document, page: u32, id: u32, name: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(page), id)?;
    match d.shape(&at)?.sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => c.formula.text().map(str::to_owned),
        _ => None,
    }
}

fn value_on(d: &Document, page: u32, id: u32, name: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(page), id)?;
    match d.shape(&at)?.sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    }
}

#[test]
fn a_cell_that_names_a_shape_not_copied_is_its_cached_value_on_another_page_and_keeps_its_formula_on_the_same_one() {
    let dir = Dir::new();
    let path = dir.path("names.vsdx");
    std::fs::write(&path, drawing_whose_cells_name_each_other()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    // 6, 7 and 8 without 5: on page 2 `Sheet.5!` would be the box there, nine wide.
    run(&mut s, "edit.copy", json!({"ids": [6, 7, 8]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({"page": 2})));
    assert_eq!(ids, [6, 7, 8], "page 2 holds only a box with the ID 5, so the copies take the ids above it");
    let d = doc(&s);
    // 6' named 5: it is its value (2), not half the other box's width (4.5).
    assert_eq!((formula_on(d, 2, 6, "Width"), value_on(d, 2, 6, "Width").as_deref()), (None, Some("2")));
    assert_eq!((formula_on(d, 2, 6, "PinX"), value_on(d, 2, 6, "PinX").as_deref()), (None, Some("5")));
    // 7' was glued to 5 by a formula and a trigger and has no Connect row: a plain point, and a trigger that is a value.
    for cell in ["BeginX", "BeginY", "BegTrigger"] {
        assert_eq!(formula_on(d, 2, 7, cell), None, "{cell}");
    }
    assert_eq!(value_on(d, 2, 7, "BeginX").as_deref(), Some("4"));
    assert!(d.page(2).unwrap().connects.is_empty());
    // 8' named 6, which was copied: its copy. The cells that name nothing outside are as they were.
    assert_eq!(formula_on(d, 2, 8, "Width").as_deref(), Some("Sheet.6!Width*0.5"));
    assert_eq!(formula_on(d, 2, 7, "Width").as_deref(), Some("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"));
    // The box with the ID 5 on page 2 is untouched, and no cell of the copies reads it: their values are as they were copied.
    assert_eq!(value_on(d, 2, 5, "Width").as_deref(), Some("9"));
    assert_eq!(value_on(d, 2, 6, "Width").as_deref(), Some("2"));
    assert_eq!(check_session(&s), Ok(()));
    // The same copy on its own page keeps the reference to the original, which is there.
    let ids = ids_of(&run(&mut s, "edit.paste", json!({"page": 1})));
    assert_eq!(ids, [9, 10, 11]);
    let d = doc(&s);
    assert_eq!(formula_on(d, 1, 9, "Width").as_deref(), Some("Sheet.5!Width*0.5"));
    assert_eq!(formula_on(d, 1, 11, "Width").as_deref(), Some("Sheet.9!Width*0.5"));
    // Saved and read again, nothing on page 2 names a shape that is not the copy's.
    let saved = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": saved}));
    assert_eq!(formula_on(doc(&t), 2, 6, "Width"), None);
    assert_eq!(value_on(doc(&t), 2, 6, "Width").as_deref(), Some("2"));
    // Copies of the box with the shapes that name it keep the formulas on page 2 too, naming the copies.
    run(&mut s, "edit.copy", json!({"ids": [5, 6, 7]}));
    let again = ids_of(&run(&mut s, "edit.paste", json!({"page": 2})));
    let d = doc(&s);
    assert_eq!(formula_on(d, 2, again[1], "Width"), Some(format!("Sheet.{}!Width*0.5", again[0])));
    assert_eq!(formula_on(d, 2, again[2], "BeginX"), Some(format!("PAR(PNT(Sheet.{0}!PinX,Sheet.{0}!PinY))", again[0])));
}

/// What a cell of shape `id` on `page` holds: its formula, cached value and error, and whether it is guarded.
fn cell_on(d: &Document, page: u32, id: u32, name: &str) -> (Option<String>, Option<String>, Option<String>, bool) {
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    match d.shape(&at).unwrap().sheet.lookup(&CellAddr::direct(name)) {
        Lookup::Found(c) => (
            c.formula.text().map(str::to_owned),
            c.value.as_ref().map(|v| v.text().to_owned()),
            c.error.as_deref().map(str::to_owned),
            c.is_guarded(),
        ),
        _ => (None, None, None, false),
    }
}

/// Page 1 holds a box (5) and a shape (6) with three cells that name it and are not plain: `Width` with a cached error, `Height` with no cached value,
/// `LocPinX` guarded; and a connector (7) whose begin cells hold an error. Page 2 holds a box with the `ID` 5.
fn drawing_whose_cells_freeze_badly() -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let boxed = |id: u32, x: f64, width: f64| {
        shape(id, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="{width}"/><Cell N="Height" V="1"/>"#))
    };
    let six = shape(
        6,
        r##"<Cell N="PinX" V="5"/><Cell N="PinY" V="2"/><Cell N="Width" V="2" F="Sheet.5!Width*0.5" E="#REF!"/><Cell N="Height" F="Sheet.5!Height"/>
<Cell N="LocPinX" V="1" F="GUARD(Sheet.5!Width*0.25)"/>"##,
    );
    let connector = shape(
        7,
        r##"<Cell N="BeginX" V="4" F="PAR(PNT(Sheet.5!PinX,Sheet.5!PinY))" E="#REF!"/><Cell N="BeginY" V="2" F="PAR(PNT(Sheet.5!PinX,Sheet.5!PinY))" E="#REF!"/>
<Cell N="EndX" V="8"/><Cell N="EndY" V="2"/><Cell N="PinX" V="6" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="4" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>"##,
    );
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{}{six}{connector}", boxed(5, 2.0, 4.0))))
        .page(PageSpec::new(2, "Page-2").shapes(&boxed(5, 3.0, 9.0)))
        .build()
}

#[test]
fn a_cell_made_its_value_on_another_page_has_no_error_keeps_its_formula_with_no_value_and_loses_a_guard() {
    let dir = Dir::new();
    let path = dir.path("freeze.vsdx");
    std::fs::write(&path, drawing_whose_cells_freeze_badly()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    // As the file has them.
    assert_eq!(cell_on(doc(&s), 1, 6, "Width"), (Some("Sheet.5!Width*0.5".into()), Some("2".into()), Some("#REF!".into()), false));
    assert_eq!(cell_on(doc(&s), 1, 6, "Height"), (Some("Sheet.5!Height".into()), None, None, false));
    assert!(cell_on(doc(&s), 1, 6, "LocPinX").3, "the guard is read");
    run(&mut s, "edit.copy", json!({"ids": [6, 7]}));
    let ids = ids_of(&run(&mut s, "edit.paste", json!({"page": 2})));
    assert_eq!(ids, [6, 7]);
    let d = doc(&s);
    // The cached error goes with the formula: a constant reports none.
    assert_eq!(cell_on(d, 2, 6, "Width"), (None, Some("2".into()), None, false));
    // No cached value: nothing to be a constant of, so the formula stays and no empty cell, which would read as 0, is made.
    assert_eq!(cell_on(d, 2, 6, "Height"), (Some("Sheet.5!Height".into()), None, None, false));
    // The guard goes with the formula.
    assert_eq!(cell_on(d, 2, 6, "LocPinX"), (None, Some("1".into()), None, false));
    // The end of a connector with an error: a plain point with no error, so a later reroute is not skipped for it.
    for cell in ["BeginX", "BeginY"] {
        let (formula, value, error, _) = cell_on(d, 2, 7, cell);
        assert_eq!((formula, error), (None, None), "{cell}");
        assert!(value.is_some(), "{cell}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
