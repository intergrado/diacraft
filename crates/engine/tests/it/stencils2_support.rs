//! The five per-stencil checks of the second stencil batch (the design's section 9), and the helpers they share with the catalogue test.
//!
//! Each `stencils2_<stencil>.rs` calls these with its stencil's id, and each check loops over the masters of `builtin_stencil(<id>)`. **A stencil that is not shipped has no masters, so each check passes
//! on nothing; once a family task ships the stencil, the same call runs on every master and fails loudly, naming the master, where a check does not hold.** The checks themselves are tried on stencils
//! that ship today at the end of this file.
//!
//! 1. [`drop_edit_undo`]: dropped, labelled, resized to twice the width and half the height (the height alone is kept for a shape whose `LockHeight` is 1), turned, copied, undone and redone.
//! 2. [`render_inside_its_box`]: the page draws with nothing skipped, and the ink lies inside the shape's box and its label's box.
//! 3. [`round_trips`]: saved and read back as `.vsdx`, `.odg` and `.diacraft`: the same drawing, the same connection points, the master link kept, and saved again as no edit.
//! 4. [`glue`]: a connector master of the stencil is glued from a node to another, and the end follows when the second is moved, before and after a save and a reopen.
//! 5. [`theme`]: the masters that hold a fill formula follow three themes of the catalogue: the value is what the formula gives, with no theme and under each.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef};
use diacraft_engine::draw::{connection_points, shape_frame, text_view};
use diacraft_engine::stencil::{MasterSpec, builtin_stencil};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value, json};

use crate::odg_build::{Dir, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::odg_masters::assert_link_holds;
use crate::run;

// ---- helpers, moved from `odg_stencils.rs` --------------------------------------------------------------------------------------------

/// The first page, drawn at 50 pixels to the inch on white paper: its size and its premultiplied pixels. Nothing may be left out of the frame.
pub(crate) fn drawn(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

/// The most the pixels of two drawings may differ by, in one channel: a path read back at the precision the file holds is the same path.
pub(crate) const PIXEL_SLACK: i32 = 2;

/// Asserts that the drawing `read` (a file read again) shows what `made` (the session's own) shows, and that the shape at `at` has the connection points it
/// had: as many, at the same places, the same way out.
#[track_caller]
pub(crate) fn assert_reads_back_as_made(made: &Document, read: &Document, at: usize, what: &str) {
    assert_reads_back_within(made, read, at, what, PIXEL_SLACK);
}

/// [`assert_reads_back_as_made`] with `slack` for the most a channel of a pixel may differ by. A route written as a path is in hundredths of a millimetre, which moves a
/// hairline by a hundredth of a pixel and its anti-aliasing by a few levels.
#[track_caller]
pub(crate) fn assert_reads_back_within(made: &Document, read: &Document, at: usize, what: &str, slack: i32) {
    let (a, b) = (drawn(made), drawn(read));
    assert_eq!((a.0, a.1, a.2.len()), (b.0, b.1, b.2.len()), "{what}: the picture's size");
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= slack, "{what}: the file draws the shape differently from the session (the worst channel differs by {worst})");
    assert!(a.2.iter().any(|v| *v != 255), "{what}: the picture is not blank");
    let page = |d: &Document| d.pages.get(0).unwrap().id;
    let id = |d: &Document| d.pages.get(0).unwrap().shapes.get(at).unwrap().id;
    let (pa, pb) = (connection_points(made, page(made), id(made)), connection_points(read, page(read), id(read)));
    // A connector a person made (a Data Flow, or what `connector.add` makes) has none, and the file says so, though the reader gives every shape the four standard rows
    // Answered the question of the divergence every connector saved had, which read back with four.
    assert_eq!(pa.len(), pb.len(), "{what}: the connection points, {pa:?} against {pb:?}");
    for (x, y) in pa.iter().zip(pb.iter()) {
        let near = |p: f64, q: f64| (p - q).abs() < 1e-3;
        assert!(
            x.index == y.index && near(x.x, y.x) && near(x.y, y.y) && near(x.dir_x, y.dir_x) && near(x.dir_y, y.dir_y),
            "{what}: a connection point is {x:?} in the session and {y:?} in the file"
        );
    }
}

/// The session `dir`'s `a.odg` is read into.
pub(crate) fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

// ---- what the checks share ------------------------------------------------------------------------------------------------------------

/// The masters of the built-in stencil `id`: none when the stencil is not shipped.
pub(crate) fn masters_of(id: &str) -> &'static [MasterSpec] {
    builtin_stencil(id).map_or(&[][..], |s| s.masters())
}

/// The masters of the built-in stencil `id` whose drop is one shape: **a master whose drop makes a structure of swimlanes (BPMN's Pool) is passed over by the checks that look at the one shape a
/// drop makes** (its frame is resized by its lanes, and it has no master link, like every shape of a swimlane structure); the family's own tests look at the structure.
fn shape_masters(id: &str) -> impl Iterator<Item = &'static MasterSpec> {
    masters_of(id).iter().filter(|m| !m.makes_structure())
}

/// A new drawing with the master dropped at (`x`, `y`) on page 1 and its name typed: the session and the shape's `ID`.
fn dropped(stencil: &str, master: &MasterSpec, (x, y): (f64, f64), label: bool) -> (Session, u32) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let id = drop_into(&mut s, stencil, master, (x, y), label);
    (s, id)
}

fn drop_into(s: &mut Session, stencil: &str, master: &MasterSpec, (x, y): (f64, f64), label: bool) -> u32 {
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let dropped = run(s, "shape.drop", json!({ "master": master.id, "stencil": stencil, "page": page, "x": x, "y": y }));
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    if label {
        let target = label_holder(s.document().unwrap(), page, id);
        run(s, "shape.set_text", json!({ "page": page, "id": target, "text": master.name }));
    }
    id
}

/// The shape that holds the label of the instance `id`: the instance itself, or, for a group whose `IsTextEditTarget` is 0 (a group master of compartments: its members hold the text), its last member.
/// That is where typing on the selected group goes, and where an `.odg` save can take a text: it refuses a text on a group.
fn label_holder(doc: &Document, page: u32, id: u32) -> u32 {
    let last = doc.find_shape(Owner::Page(page), id).and_then(|at| {
        doc.shape(&at).and_then(|shape| shape.children.len().checked_sub(1).and_then(|k| shape.children.get(k)).map(|member| member.id))
    });
    match last {
        Some(member) if cell(doc, page, id, "IsTextEditTarget") == Some(0.0) => member,
        _ => id,
    }
}

/// What a cell of the shape `id` reads, through its master.
fn cell(doc: &Document, page: u32, id: u32, name: &str) -> Option<f64> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(name))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

// ---- 1. drop, edit, undo --------------------------------------------------------------------------------------------------------------

/// Each master is dropped, labelled, resized to twice its width and half its height (a shape that keeps its height: the width only), turned, copied, undone and redone. The geometry stays
/// finite, the label stays, and the history is as expected.
#[track_caller]
pub(crate) fn drop_edit_undo(stencil: &str) {
    for master in shape_masters(stencil) {
        let key = format!("{stencil}/{}", master.id);
        let (mut s, id) = dropped(stencil, master, (5.0, 5.0), true);
        let page = first_page(&s);
        let held = label_holder(s.document().unwrap(), page, id);
        assert_eq!(text_view(s.document().unwrap(), page, held).unwrap().chars, master.name, "{key}: the label");
        if !master.is_connector() {
            let frame = shape_frame(s.document().unwrap(), page, id).unwrap_or_else(|| panic!("{key}: it has a frame"));
            let keeps_height = cell(s.document().unwrap(), page, id, "LockHeight").is_some_and(|v| v != 0.0);
            let (w, h) = (frame.width * 2.0, frame.height * 0.5);
            run(
                &mut s,
                "shape.resize",
                json!({ "id": id, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": if keeps_height { frame.height } else { h } }),
            );
            let after = shape_frame(s.document().unwrap(), page, id).unwrap();
            assert!(
                after.width.is_finite() && after.height.is_finite() && after.width > 0.0 && after.height > 0.0,
                "{key}: the size stays finite: {after:?}"
            );
            if keeps_height {
                assert!(
                    (after.height - frame.height).abs() < 1e-9,
                    "{key}: a shape with LockHeight keeps its height: {} to {}",
                    frame.height,
                    after.height
                );
            } else {
                assert!(
                    (after.width - w).abs() < 1e-6 && (after.height - h).abs() < 1e-6,
                    "{key}: sized to twice the width and half the height: {after:?}"
                );
            }
            run(&mut s, "shape.rotate", json!({ "ids": [id], "angle": 30 }));
            run(&mut s, "shape.flip", json!({ "ids": [id], "axis": "horizontal" }));
        }
        assert_eq!(text_view(s.document().unwrap(), page, held).unwrap().chars, master.name, "{key}: the label stays");
        let before = s.document().unwrap().clone();
        let copy = run(&mut s, "shape.duplicate", json!({ "ids": [id] }));
        let copied = u32::try_from(copy["ids"][0].as_u64().unwrap()).unwrap();
        let held_by_copy = label_holder(s.document().unwrap(), page, copied);
        assert_eq!(text_view(s.document().unwrap(), page, held_by_copy).unwrap().chars, master.name, "{key}: the copy has the label");
        let doc = s.document().unwrap();
        let (a, b) = (doc.find_shape(Owner::Page(page), id).unwrap(), doc.find_shape(Owner::Page(page), copied).unwrap());
        assert_eq!(doc.shape(&a).unwrap().master, doc.shape(&b).unwrap().master, "{key}: the copy is of the same master");
        let after = s.document().unwrap().clone();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.document().unwrap(), &before, "{key}: undo takes the copy away");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.document().unwrap(), &after, "{key}: redo brings it back");
        let drawn = render(s.document().unwrap(), page, 50.0);
        assert!(drawn.skipped.is_empty(), "{key}: {:?}", drawn.skipped);
    }
}

// ---- 2. the render --------------------------------------------------------------------------------------------------------------------

fn render(doc: &Document, page: u32, ppi: f64) -> Rendered {
    let view = View::fit_page(doc, page, ppi).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(None);
    renderer.render(doc, page, &view)
}

/// The box the ink of the shape `id` may lie in, in inches on the page: its box and its text block's, and for a one-dimensional shape the box round its ends, grown by `margin`.
fn allowed_box(doc: &Document, page: u32, id: u32, margin: f64) -> (f64, f64, f64, f64) {
    let c = |name: &str| cell(doc, page, id, name).unwrap_or(0.0);
    if cell(doc, page, id, "BeginX").is_some() && cell(doc, page, id, "EndX").is_some() {
        let (x0, x1) = (c("BeginX").min(c("EndX")), c("BeginX").max(c("EndX")));
        let (y0, y1) = (c("BeginY").min(c("EndY")), c("BeginY").max(c("EndY")));
        // A line end and a zigzag across the line reach a few tenths of an inch beside it.
        return (x0 - margin - 0.3, y0 - margin - 0.3, x1 + margin + 0.3, y1 + margin + 0.3);
    }
    let frame = shape_frame(doc, page, id).unwrap();
    let (left, bottom) = (frame.pin_x - c("LocPinX"), frame.pin_y - c("LocPinY"));
    let mut allowed = (left, bottom, left + frame.width, bottom + frame.height);
    // The text block, in the shape's own frame.
    let (tx, ty) = (c("TxtPinX") - c("TxtLocPinX"), c("TxtPinY") - c("TxtLocPinY"));
    let (tw, th) = (c("TxtWidth"), c("TxtHeight"));
    if tw > 0.0 && th > 0.0 {
        allowed = (allowed.0.min(left + tx), allowed.1.min(bottom + ty), allowed.2.max(left + tx + tw), allowed.3.max(bottom + ty + th));
    }
    (allowed.0 - margin, allowed.1 - margin, allowed.2 + margin, allowed.3 + margin)
}

/// The extent of the ink of a render: the least and greatest column and row with a pixel that is there at all, as inches from the page's left and bottom, or `None` for a blank page.
fn ink_extent(r: &Rendered, ppi: f64, height: f64) -> Option<(f64, f64, f64, f64)> {
    let (mut left, mut right, mut top, mut bottom) = (usize::MAX, 0, usize::MAX, 0);
    let width = r.width as usize;
    for (at, p) in r.rgba_premul.as_chunks::<4>().0.iter().enumerate() {
        if p[3] > 8 {
            let (col, row) = (at % width, at / width);
            left = left.min(col);
            right = right.max(col);
            top = top.min(row);
            bottom = bottom.max(row);
        }
    }
    (right >= left && bottom >= top)
        .then(|| (left as f64 / ppi, height - (bottom as f64 + 1.0) / ppi, (right as f64 + 1.0) / ppi, height - top as f64 / ppi))
}

/// The page draws with nothing skipped, and the ink lies inside the shape's box and its label's box.
#[track_caller]
pub(crate) fn render_inside_its_box(stencil: &str) {
    const PPI: f64 = 100.0;
    for master in shape_masters(stencil) {
        let key = format!("{stencil}/{}", master.id);
        let (s, id) = dropped(stencil, master, (5.5, 4.25), true);
        let (doc, page) = (s.document().unwrap(), first_page(&s));
        let drawn = render(doc, page, PPI);
        assert!(drawn.skipped.is_empty(), "{key}: {:?}", drawn.skipped);
        let height = doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
        let ink = ink_extent(&drawn, PPI, height).unwrap_or_else(|| panic!("{key}: the page is blank"));
        // A line's own weight and the antialiasing reach a little beyond its geometry: a tenth of an inch and a pixel.
        let allowed = allowed_box(doc, page, id, 0.1 + 1.0 / PPI);
        assert!(
            ink.0 >= allowed.0 - 1e-9 && ink.1 >= allowed.1 - 1e-9 && ink.2 <= allowed.2 + 1e-9 && ink.3 <= allowed.3 + 1e-9,
            "{key}: ink {ink:?} is outside its box {allowed:?}"
        );
    }
}

// ---- 3. round trips -------------------------------------------------------------------------------------------------------------------

/// What a master's save said for other programs: the masters whose outline an `.odg` writes as a rectangle (the families' reports list them).
pub(crate) const RECTANGLE_NOTE: &str = "outline that is written as a rectangle";

/// Each master is saved and read back as `.vsdx`, `.odg` and `.diacraft`: each draws the same within [`PIXEL_SLACK`], has the same connection points (as many, the same places, the same ways
/// out), keeps its master link, and saves again as no edit. → the masters an `.odg` save writes as a rectangle for other programs (not a failure: the report names them).
#[track_caller]
pub(crate) fn round_trips(stencil: &str) -> Vec<String> {
    let mut approximated = Vec::new();
    for master in shape_masters(stencil) {
        let key = format!("{stencil}/{}", master.id);
        for ext in ["vsdx", "diacraft"] {
            let dir = Dir::new();
            let (mut s, id) = dropped(stencil, master, (4.0, 2.0), true);
            let path = dir.path(&format!("x.{ext}"));
            run(&mut s, "file.save_as", json!({ "path": path }));
            let mut again = Session::new();
            run(&mut again, "file.open", json!({ "path": path }));
            let what = format!("{key} as {ext}");
            assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, &what);
            let (made, read) = (s.document().unwrap(), again.document().unwrap());
            let page = made.pages.get(0).unwrap().id;
            assert_eq!(
                text_view(made, page, label_holder(made, page, id)).unwrap().chars,
                text_view(read, read.pages.get(0).unwrap().id, label_holder(read, read.pages.get(0).unwrap().id, id)).unwrap().chars,
                "{what}: the label"
            );
            let link = read.pages.get(0).unwrap().shapes.get(0).unwrap().master.unwrap_or_else(|| panic!("{what}: the shape keeps its master"));
            let was = made.pages.get(0).unwrap().shapes.get(0).unwrap().master.and_then(|m| made.master(m)).and_then(|m| m.unique_id.clone());
            assert!(was.is_some(), "{what}: the master has a GUID");
            assert_eq!(read.master(link).and_then(|m| m.unique_id.clone()), was, "{what}: the master keeps its GUID");
            // Saved again as no edit: the file is the same bytes (a splice into the file that rewrites no part).
            let before = dir.read(&format!("x.{ext}"));
            save_in_place(&mut again);
            assert!(dir.read(&format!("x.{ext}")) == before, "{what}: saved again as no edit, and the file changed");
        }
        // `.odg`: dropped into a drawing of four rectangles, saved in place, read back as the session had it, and saved again as no edit.
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil, "page": 0, "x": 4.0, "y": 2.0 }));
        let saved = match s.execute("file.save", &json!({})) {
            Ok(saved) => saved,
            Err(EngineError::Failed { msg, .. }) => panic!("{key} as odg: the save was refused: {msg}"),
            Err(other) => panic!("{key} as odg: {other:?}"),
        };
        let said: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for note in &said {
            // A text of fields is written as the text it shows, and an outline other programs cannot be given is a rectangle: both are said, and neither is a failure.
            // (Said in the singular for a master with one field: "1 new field written as the text it shows".)
            let fields = note.contains("new fields written as the text they show") || note.contains("new field written as the text it shows");
            assert!(note.contains(RECTANGLE_NOTE) || fields, "{key} as odg: {note}");
        }
        if said.iter().any(|n| n.contains(RECTANGLE_NOTE)) {
            approximated.push(key.clone());
        }
        let again = reopened(&dir);
        assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 5, "{key} as odg: the four rectangles and the shape");
        assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 4, &format!("{key} as odg"));
        assert_link_holds(s.document().unwrap(), again.document().unwrap(), 4, &format!("{key} as odg"));
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    approximated
}

/// A save of a session in place, in the file it has.
fn save_in_place(s: &mut Session) -> Value {
    save(s)
}

// ---- 4. glue --------------------------------------------------------------------------------------------------------------------------

/// A connector master of the stencil is glued from a node of it to another. The second is moved: the end follows and the line ends stay. The drawing is saved and reopened, and the move is made again.
#[track_caller]
pub(crate) fn glue(stencil: &str) {
    let nodes: Vec<&MasterSpec> = masters_of(stencil).iter().filter(|m| !m.is_connector() && m.point_count() > 0).take(2).collect();
    for connector in masters_of(stencil).iter().filter(|m| m.is_connector()) {
        let key = format!("{stencil}/{}", connector.id);
        let rectangle = diacraft_engine::stencil::find_master("rectangle").unwrap();
        let (from, to) = match nodes.as_slice() {
            [a, b] => (*a, *b),
            [a] => (*a, *a),
            _ => (rectangle, rectangle),
        };
        let mut s = Session::new();
        run(&mut s, "file.new", json!({}));
        let page = first_page(&s);
        let node_stencil = |m: &MasterSpec| if std::ptr::eq(m, rectangle) { "basic" } else { stencil };
        let a = drop_into(&mut s, node_stencil(from), from, (3.0, 5.0), false);
        let b = drop_into(&mut s, node_stencil(to), to, (8.0, 5.0), false);
        let link = drop_into(&mut s, stencil, connector, (5.5, 8.0), false);
        run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": a } }));
        run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "id": b } }));
        assert_eq!(s.document().unwrap().page(page).unwrap().connects.len(), 2, "{key}: glued at both ends");
        let ends = |s: &Session| {
            let doc = s.document().unwrap();
            ["BeginX", "BeginY", "EndX", "EndY"].map(|n| cell(doc, page, link, n).unwrap_or(f64::NAN))
        };
        let arrows = |s: &Session| {
            let doc = s.document().unwrap();
            ["BeginArrow", "EndArrow", "LinePattern"].map(|n| cell(doc, page, link, n))
        };
        let (before, kept) = (ends(&s), arrows(&s));
        assert!(before.iter().all(|v| v.is_finite()), "{key}: its ends are placed: {before:?}");
        run(&mut s, "shape.move", json!({ "ids": [b], "dx": 0.0, "dy": -1.5 }));
        let moved = ends(&s);
        assert!((moved[3] - (before[3] - 1.5)).abs() < 1e-6, "{key}: the end follows the shape it is glued to: {} to {}", before[3], moved[3]);
        assert!((moved[0] - before[0]).abs() < 1e-6 && (moved[1] - before[1]).abs() < 1e-6, "{key}: the other end stays");
        assert_eq!(arrows(&s), kept, "{key}: the line ends and the pattern stay");
        // Saved and reopened, the move is made again.
        let dir = Dir::new();
        let path = dir.path("glue.vsdx");
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": path }));
        assert_eq!(again.document().unwrap().page(page).unwrap().connects.len(), 2, "{key}: still glued after a save and a reopen");
        let reopened_ends = ends(&again);
        assert!((reopened_ends[3] - moved[3]).abs() < 1e-6, "{key}: the end is where it was");
        run(&mut again, "shape.move", json!({ "ids": [b], "dx": 0.0, "dy": 0.75 }));
        let last = ends(&again);
        assert!((last[3] - (reopened_ends[3] + 0.75)).abs() < 1e-6, "{key}: the end follows again: {} to {}", reopened_ends[3], last[3]);
        assert_eq!(arrows(&again), kept, "{key}: the line ends and the pattern stay after the reopen");
    }
}

// ---- 5. themes ------------------------------------------------------------------------------------------------------------------------

/// Whether the master holds a fill formula of its own (the accent masters do).
fn fill_formula(master: &MasterSpec) -> Option<String> {
    let mut doc = Document::new_blank();
    let id = diacraft_engine::stencil::ensure_master(&mut doc, master).ok()?;
    let shape = doc.master(id)?.shapes.get(0)?.clone();
    match &shape.sheet.cell("FillForegnd")?.formula {
        diacraft_doc::FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

/// The accent masters are dropped and three themes of the catalogue applied in turn. Each fill cell is what its formula gives, with no theme and under each: a recalculation finds nothing to change,
/// and the cell reads a colour.
#[track_caller]
pub(crate) fn theme(stencil: &str) {
    for master in masters_of(stencil) {
        let Some(formula) = fill_formula(master) else { continue };
        let key = format!("{stencil}/{} ({formula})", master.id);
        let (mut s, id) = dropped(stencil, master, (5.0, 5.0), false);
        let page = first_page(&s);
        let text = |s: &Session| -> String {
            let doc = s.document().unwrap();
            let at = doc.find_shape(Owner::Page(page), id).unwrap();
            let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct("FillForegnd")).unwrap();
            found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default()
        };
        let is_colour = |t: &str| t.len() == 7 && t.starts_with('#') && t[1..].bytes().all(|b| b.is_ascii_hexdigit());
        let default = text(&s);
        assert!(is_colour(&default), "{key}: with no theme it reads a colour: `{default}`");
        let listed = run(&mut s, "theme.list", json!({}));
        let themes: Vec<String> = listed["themes"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str().map(str::to_owned)).take(3).collect();
        assert_eq!(themes.len(), 3, "the catalogue has three themes at least");
        for theme in &themes {
            run(&mut s, "theme.apply", json!({ "theme": theme }));
            let under = text(&s);
            assert!(is_colour(&under), "{key}: under {theme} it reads a colour: `{under}`");
            let mut doc = s.document().unwrap().clone();
            let report = diacraft_calc::commit(&mut doc, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default());
            assert_eq!((report.written, report.refused), (0, 0), "{key}: under {theme} the cached values are what the formulas give");
        }
        run(&mut s, "theme.apply", json!({ "theme": "none" }));
        assert_eq!(text(&s), default, "{key}: with no theme again it is the default again");
    }
}

// ---- the checks are tried on stencils that ship today ---------------------------------------------------------------------------------

#[test]
fn a_stencil_that_is_not_shipped_has_no_masters_so_each_check_passes_on_nothing() {
    for id in ["uml-class", "no-such-stencil"] {
        if builtin_stencil(id).is_none() {
            assert!(masters_of(id).is_empty());
            drop_edit_undo(id);
            render_inside_its_box(id);
            assert!(round_trips(id).is_empty());
            glue(id);
            theme(id);
        }
    }
}

#[test]
fn the_five_checks_hold_for_gane_sarson_which_ships_with_a_connector_master() {
    assert!(masters_of("gane-sarson").iter().any(|m| m.is_connector()), "the check of glue has a connector to glue");
    drop_edit_undo("gane-sarson");
    render_inside_its_box("gane-sarson");
    let approximated = round_trips("gane-sarson");
    assert!(approximated.is_empty() || approximated.iter().all(|k| k.starts_with("gane-sarson/")), "{approximated:?}");
    glue("gane-sarson");
    theme("gane-sarson");
}

#[test]
fn the_checks_hold_for_arrow_shapes_and_the_organization_chart_whose_masters_hold_fill_formulas() {
    drop_edit_undo("arrows");
    render_inside_its_box("arrows");
    // The positions of the org chart each hold a fill formula, so the theme check has masters to follow.
    assert!(masters_of("orgchart").iter().any(|m| fill_formula(m).is_some()));
    theme("orgchart");
    render_inside_its_box("orgchart");
}

/// The data flow keeps its end 4 as before: the master holds it, the instance holds none, and a VSDX save reopens with the arrow.
#[test]
fn the_data_flow_keeps_end_4_and_its_vsdx_save_reopens_with_the_arrow() {
    let flow = masters_of("gane-sarson").iter().find(|m| m.id == "gane-sarson.data-flow").unwrap();
    // No label: its text sits beside the line and would be ink where the probe looks for an arrowhead.
    let (mut s, id) = dropped("gane-sarson", flow, (5.0, 5.0), false);
    let page = first_page(&s);
    assert_eq!(cell(s.document().unwrap(), page, id, "EndArrow"), Some(4.0));
    assert_eq!(cell(s.document().unwrap(), page, id, "BeginArrow"), Some(0.0));
    let at = s.document().unwrap().find_shape(Owner::Page(page), id).unwrap();
    assert!(s.document().unwrap().shape(&at).unwrap().sheet.cell("EndArrow").is_none(), "the instance inherits the end from its master");
    // The arrowhead is drawn: ink beside the line at its end, which a line with no head lacks.
    let dir = Dir::new();
    let path = dir.path("flow.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": path }));
    let reread = first_page(&again);
    assert_eq!(cell(again.document().unwrap(), reread, id, "EndArrow"), Some(4.0), "the reopened flow has its arrowhead");
    assert_eq!(cell(again.document().unwrap(), reread, id, "BeginArrow"), Some(0.0));
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "the data flow as vsdx");
    let a = render(s.document().unwrap(), page, 100.0);
    let height = s.document().unwrap().page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
    let head = |r: &Rendered, x0: f64, x1: f64| {
        // The ink in the bands beside the line (from 0.03 to 0.12 inch off it), within the stretch given: only an arrowhead has any.
        let mut n = 0;
        let mut x = x0;
        while x < x1 {
            for k in 3..=12 {
                for sign in [-1.0, 1.0] {
                    let dy = sign * f64::from(k) / 100.0;
                    let (col, row) = ((x * 100.0) as usize, ((height - (5.0 + dy)) * 100.0) as usize);
                    let at = (row * r.width as usize + col) * 4;
                    n += usize::from(r.rgba_premul[at + 3] > 128);
                }
            }
            x += 0.01;
        }
        n
    };
    assert!(head(&a, 5.3, 5.75) > 0, "the arrowhead is drawn at the end");
    assert_eq!(head(&a, 4.25, 4.7), 0, "and there is none at the begin");
}

#[test]
fn the_label_of_a_group_that_does_not_edit_its_own_text_goes_to_its_last_member() {
    // A group master of compartments is a group with `IsTextEditTarget` 0: the label is typed in its last member, as typing on the selected group does, since an `.odg`
    // save refuses a text on a group. A shape that is no group, and a group that edits its own text, hold their own.
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = first_page(&s);
    let rectangle = diacraft_engine::stencil::find_master("rectangle").unwrap();
    let ids: Vec<u32> = [2.0, 5.0].iter().map(|x| drop_into(&mut s, "basic", rectangle, (*x, 5.0), false)).collect();
    let group = run(&mut s, "shape.group", json!({ "ids": ids }));
    let group = u32::try_from(group["id"].as_u64().unwrap()).unwrap();
    assert_eq!(label_holder(s.document().unwrap(), page, ids[0]), ids[0], "a shape that is no group holds its own label");
    assert_eq!(label_holder(s.document().unwrap(), page, group), group, "a group that edits its own text holds its own label");
    run(&mut s, "sheet.set_cell", json!({ "id": group, "cell": "IsTextEditTarget", "formula": "0" }));
    assert_eq!(
        label_holder(s.document().unwrap(), page, group),
        ids[1],
        "the members hold the text of a group that does not: the last in document order"
    );
}
