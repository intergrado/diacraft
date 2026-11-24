//! **Swimlanes held in the cells another program writes are laid out as DiaCraft's own** (`docs/editing-commands.md`). Every vector is a model built by hand
//! here with the cells the specifications give for lists and containers and none of DiaCraft's rows: a shape typed `List` by `User.msvStructureType` that holds containers under code 2 of its
//! `Relationships`, each naming it back under 5, in the formula's order, a frame that holds the lists under code 1
//! (B1), and, in the second family, the masters the cross-functional samples are made of (C2: `CFF Container`, `Swimlane List`, `Phase List`, `Swimlane`,
//! `Separator`). Each model is written by DiaCraft's VSDX writer and opened as a file; no sample file is read. An insert and a resize lay the lanes out, and **the file's own shapes are
//! given no row of DiaCraft's**.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellName, Document, Links, Master, Row, RowKey, Section, Shape, ShapeType, SharedVec, Sheet, Value};
use diacraft_engine::Session;
use diacraft_testkit::fixtures::blank_document;
use serde_json::{Value as Json, json};

use crate::container::{assert_box, boxed, members};
use crate::edit::doc;
use crate::odg_build::Dir;
use crate::run;
use crate::swimlane::{structure, user_text};

type Box4 = (f64, f64, f64, f64);

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&format!("{n}"))).with_unit("IN")
}

fn of_size(formula: &str, n: f64) -> Cell {
    Cell::with_formula(formula, Some(Value::from_text(&format!("{n}")))).with_unit("IN")
}

/// A shape whose box is `(x0, y0, x1, y1)` in inches, drawn as a rectangle from its own size.
fn shape_at(id: u32, (x0, y0, x1, y1): Box4) -> Shape {
    let (w, h) = (x1 - x0, y1 - y0);
    let mut shape = Shape::new(id);
    shape.kind = ShapeType::Shape;
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    for (name, cell) in [
        ("PinX", length(x0 + w / 2.0)),
        ("PinY", length(y0 + h / 2.0)),
        ("Width", length(w)),
        ("Height", length(h)),
        ("LocPinX", of_size("Width*0.5", w / 2.0)),
        ("LocPinY", of_size("Height*0.5", h / 2.0)),
        ("Angle", Cell::constant(Value::from_text("0"))),
    ] {
        sheet.cells.insert(CellName::new(name), cell);
    }
    let mut section = Section::new("Geometry", Some(0));
    for (i, (fx, fy)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)].into_iter().enumerate() {
        let mut row =
            Row { ix: Some(u32::try_from(i + 1).unwrap()), row_type: Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" })), ..Row::default() };
        row.cells.insert(CellName::new("X"), of_size(&format!("Width*{fx}"), w * fx));
        row.cells.insert(CellName::new("Y"), of_size(&format!("Height*{fy}"), h * fy));
        section.rows.push(Arc::new(row));
    }
    sheet.sections.push(Arc::new(section));
    shape.sheet = Arc::new(sheet);
    shape
}

/// Gives the shape a text `User` row, as a file writes one (the string as the formula).
fn user(mut shape: Shape, row: &str, text: &str) -> Shape {
    Arc::make_mut(&mut shape.sheet)
        .set_cell(
            &CellAddr::in_row("User", None, RowKey::name(row), "Value"),
            Cell::with_formula(&format!("\"{text}\""), Some(Value::from_text(text))),
        )
        .unwrap();
    shape
}

fn typed(shape: Shape, kind: &str) -> Shape {
    user(shape, "msvStructureType", kind)
}

/// Gives the shape its `Relationships` cell, files write it: a `SUM` of `DEPENDSON` calls, each its code and `Sheet.<id>!SheetRef()` for each shape, `V` 0.
fn tied(mut shape: Shape, calls: &[(u32, &[u32])]) -> Shape {
    let parts: Vec<String> = calls
        .iter()
        .map(|(code, ids)| {
            let mut call = format!("DEPENDSON({code}");
            for id in *ids {
                call.push_str(&format!(",Sheet.{id}!SheetRef()"));
            }
            call.push(')');
            call
        })
        .collect();
    Arc::make_mut(&mut shape.sheet)
        .cells
        .insert(CellName::new("Relationships"), Cell::with_formula(&format!("SUM({})", parts.join(",")), Some(Value::from_text("0"))));
    shape
}

/// A master of one shape that is typed `kind`, under the name `name` (none for `None`): what an instance of it inherits its `User.msvStructureType` from.
fn master(id: u32, name: Option<&str>, kind: &str) -> Master {
    let shape = typed(shape_at(1, (0.0, 0.0, 1.0, 1.0)), kind);
    Master {
        id,
        name: name.map(Arc::from),
        name_u: name.map(Arc::from),
        sheet: Arc::new(Sheet::default()),
        shapes: SharedVec::from_vec(vec![Arc::new(shape)]),
        ..Master::default()
    }
}

/// An instance of the master `id`.
fn of_master(mut shape: Shape, id: u32) -> Shape {
    shape.master = Some(id);
    shape
}

/// A blank drawing whose first page holds `shapes`, back to front, and whose masters are `masters`.
fn model(shapes: Vec<Shape>, masters: Vec<Master>) -> Document {
    let mut d = blank_document();
    for m in masters {
        d.masters.push(Arc::new(m));
    }
    let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
    for shape in shapes {
        page.shapes.push(Arc::new(shape));
    }
    d
}

/// The model written by DiaCraft's VSDX writer and opened as a file. → the session, the folder (kept while the session works on the file) and the file's path.
fn opened(d: &Document) -> (Session, Dir, String) {
    let bytes = diacraft_vsdx::write(d, None).unwrap();
    let dir = Dir::new();
    let path = dir.write("lanes.vsdx", &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    (s, dir, path)
}

fn ids_of(list: &Json) -> Vec<u32> {
    list.as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect()
}

/// No row of DiaCraft's is on the shapes `ids`: what they are, and which way a list runs, are read from the file's own cells.
fn assert_no_rows_of_ours(s: &Session, ids: &[u32]) {
    for id in ids {
        for row in ["DiaCraftSwimlane", "DiaCraftListDirection", "DiaCraftContainerHeading", "DiaCraftContainerStyle"] {
            assert_eq!(user_text(doc(s), *id, row), "", "shape {id} of the file was given DiaCraft's row {row}");
        }
    }
}

/// A list of three horizontal lanes 6 inches long and 1 inch thick, one under the next from y = 8, with no frame: list 1 holds lanes 2, 3 and 4, and lane 3 holds the process shape 5.
fn three_lanes() -> Document {
    model(
        vec![
            tied(typed(shape_at(1, (1.0, 5.0, 7.0, 8.0)), "List"), &[(2, &[2, 3, 4])]),
            tied(typed(shape_at(2, (1.0, 7.0, 7.0, 8.0)), "Container"), &[(5, &[1])]),
            tied(typed(shape_at(3, (1.0, 6.0, 7.0, 7.0)), "Container"), &[(5, &[1]), (1, &[5])]),
            tied(typed(shape_at(4, (1.0, 5.0, 7.0, 6.0)), "Container"), &[(5, &[1])]),
            tied(shape_at(5, (3.0, 6.25, 4.0, 6.75)), &[(4, &[3])]),
        ],
        Vec::new(),
    )
}

#[test]
fn a_list_of_three_lanes_in_a_files_own_cells_is_a_structure_and_an_insert_and_a_resize_lay_it_out() {
    let (mut s, _dir, path) = opened(&three_lanes());
    let st = structure(&mut s);
    assert_eq!(st["lane_list"], json!(1), "the file's list is a list of lanes: {st}");
    assert_eq!(ids_of(&st["lanes"]), [2, 3, 4], "in the formula's order");
    assert_eq!(st["orientation"], "horizontal", "the lanes lie one under the next");
    // A lane inserted after the second: the one after it makes room, taking nothing it does not hold.
    let new = u32::try_from(run(&mut s, "swimlane.insert", json!({"lane": 3}))["id"].as_u64().unwrap()).unwrap();
    assert_eq!(ids_of(&structure(&mut s)["lanes"]), [2, 3, new, 4]);
    assert_box("the new lane, as thick as the one before it", boxed(doc(&s), new), (1.0, 5.0, 7.0, 6.0));
    assert_box("the last lane made room", boxed(doc(&s), 4), (1.0, 4.0, 7.0, 5.0));
    assert_box("the list is the box of its lanes", boxed(doc(&s), 1), (1.0, 4.0, 7.0, 8.0));
    assert_box("the lane before stays", boxed(doc(&s), 3), (1.0, 6.0, 7.0, 7.0));
    // The first lane made 2 inches thick: every lane after it moves down an inch, with what it holds.
    run(&mut s, "swimlane.resize", json!({"id": 2, "size": 2.0}));
    assert_box("the lane resized keeps its top", boxed(doc(&s), 2), (1.0, 6.0, 7.0, 8.0));
    assert_box("the second lane", boxed(doc(&s), 3), (1.0, 5.0, 7.0, 6.0));
    assert_box("the new lane", boxed(doc(&s), new), (1.0, 4.0, 7.0, 5.0));
    assert_box("the last lane", boxed(doc(&s), 4), (1.0, 3.0, 7.0, 4.0));
    assert_box("the shape went with its lane", boxed(doc(&s), 5), (3.0, 5.25, 4.0, 5.75));
    assert_eq!(members(&mut s, 3), [5], "and it is still the lane's");
    // A lane made thicker by hand, as its handle does: the lanes after it make room.
    run(&mut s, "shape.resize", json!({"id": 3, "x": 1.0, "y": 4.5, "width": 6.0, "height": 1.5}));
    assert_box("the lane resized by hand", boxed(doc(&s), 3), (1.0, 4.5, 7.0, 6.0));
    assert_box("the new lane made room", boxed(doc(&s), new), (1.0, 3.5, 7.0, 4.5));
    assert_box("the last lane made room", boxed(doc(&s), 4), (1.0, 2.5, 7.0, 3.5));
    assert_no_rows_of_ours(&s, &[1, 2, 3, 4]);
    // Saved into the file and opened again: the same structure, read from the same cells.
    let want: Vec<Box4> = [1, 2, 3, new, 4, 5].iter().map(|id| boxed(doc(&s), *id)).collect();
    run(&mut s, "file.save", json!({}));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({ "path": path }));
    assert_eq!(ids_of(&structure(&mut back)["lanes"]), [2, 3, new, 4]);
    for (id, b) in [1, 2, 3, new, 4, 5].iter().zip(want) {
        assert_box(&format!("shape {id} after the save"), boxed(doc(&back), *id), b);
    }
    assert_no_rows_of_ours(&back, &[1, 2, 3, 4]);
}

#[test]
fn a_files_lanes_that_lie_side_by_side_are_vertical_and_a_lane_added_at_the_end_goes_on_their_right() {
    let d = model(
        vec![
            tied(typed(shape_at(1, (1.0, 2.0, 4.0, 8.0)), "List"), &[(2, &[2, 3, 4])]),
            tied(typed(shape_at(2, (1.0, 2.0, 2.0, 8.0)), "Container"), &[(5, &[1])]),
            tied(typed(shape_at(3, (2.0, 2.0, 3.0, 8.0)), "Container"), &[(5, &[1])]),
            tied(typed(shape_at(4, (3.0, 2.0, 4.0, 8.0)), "Container"), &[(5, &[1])]),
        ],
        Vec::new(),
    );
    let (mut s, _dir, _path) = opened(&d);
    let st = structure(&mut s);
    assert_eq!(st["orientation"], "vertical", "{st}");
    let new = u32::try_from(run(&mut s, "swimlane.insert", json!({}))["id"].as_u64().unwrap()).unwrap();
    assert_eq!(ids_of(&structure(&mut s)["lanes"]), [2, 3, 4, new]);
    assert_box("the new lane, beside the last", boxed(doc(&s), new), (4.0, 2.0, 5.0, 8.0));
    run(&mut s, "swimlane.resize", json!({"id": 3, "size": 2.0}));
    assert_box("the lane made wider", boxed(doc(&s), 3), (2.0, 2.0, 4.0, 8.0));
    assert_box("the next made room", boxed(doc(&s), 4), (4.0, 2.0, 5.0, 8.0));
    assert_box("and the new one", boxed(doc(&s), new), (5.0, 2.0, 6.0, 8.0));
    assert_no_rows_of_ours(&s, &[1, 2, 3, 4]);
}

/// The structure of the cross-functional samples (B1, C2): frame 1 (`CFF Container`) holds the lane list 2 (`Swimlane List`) and the phase list 3 (`Phase List`);
/// lanes 4, 5 and 6 (`Swimlane`) are 6 inches long and 1 inch thick from y = 8 down; phases 7 and 8 (`Separator`) stand half an inch above the lanes and reach down across them; the frame
/// has half an inch more above and to the left. Each part is typed by its master alone. With `named` false, no master is named.
fn sample(named: bool) -> Document {
    let name = |n: &'static str| named.then_some(n);
    let masters = vec![
        master(1, name("CFF Container"), "Container"),
        master(2, name("Swimlane List"), "List"),
        master(3, name("Phase List"), "List"),
        master(4, name("Swimlane"), "Container"),
        master(5, name("Separator"), "Container"),
    ];
    model(
        vec![
            tied(of_master(shape_at(1, (0.5, 5.0, 7.0, 9.0)), 1), &[(1, &[2, 3])]),
            tied(of_master(shape_at(3, (1.0, 5.0, 7.0, 8.5)), 3), &[(4, &[1]), (2, &[7, 8])]),
            tied(of_master(shape_at(7, (1.0, 5.0, 4.0, 8.5)), 5), &[(5, &[3])]),
            tied(of_master(shape_at(8, (4.0, 5.0, 7.0, 8.5)), 5), &[(5, &[3])]),
            tied(of_master(shape_at(2, (1.0, 5.0, 7.0, 8.0)), 2), &[(4, &[1]), (2, &[4, 5, 6])]),
            tied(of_master(shape_at(4, (1.0, 7.0, 7.0, 8.0)), 4), &[(5, &[2])]),
            tied(of_master(shape_at(5, (1.0, 6.0, 7.0, 7.0)), 4), &[(5, &[2])]),
            tied(of_master(shape_at(6, (1.0, 5.0, 7.0, 6.0)), 4), &[(5, &[2])]),
        ],
        masters,
    )
}

#[test]
fn the_samples_frame_with_its_two_lists_is_told_apart_by_the_masters_names_and_keeps_its_own_margins_and_band() {
    let (mut s, _dir, _path) = opened(&sample(true));
    let st = structure(&mut s);
    assert_eq!((st["frame"].clone(), st["lane_list"].clone(), st["phase_list"].clone()), (json!(1), json!(2), json!(3)), "{st}");
    assert_eq!(ids_of(&st["lanes"]), [4, 5, 6]);
    assert_eq!(ids_of(&st["phases"]), [7, 8]);
    // A lane at the end: the phases reach down across it, each keeping how far it stands above the lanes, and the frame keeps the room it had round the lists.
    let new = u32::try_from(run(&mut s, "swimlane.insert", json!({}))["id"].as_u64().unwrap()).unwrap();
    assert_box("the new lane", boxed(doc(&s), new), (1.0, 4.0, 7.0, 5.0));
    assert_box("the lane list", boxed(doc(&s), 2), (1.0, 4.0, 7.0, 8.0));
    assert_box("the first phase keeps its band", boxed(doc(&s), 7), (1.0, 4.0, 4.0, 8.5));
    assert_box("the second phase", boxed(doc(&s), 8), (4.0, 4.0, 7.0, 8.5));
    assert_box("the phase list", boxed(doc(&s), 3), (1.0, 4.0, 7.0, 8.5));
    assert_box("the frame keeps its margins", boxed(doc(&s), 1), (0.5, 4.0, 7.0, 9.0));
    // The first phase made 2 inches wide: the next one takes the difference, and the lanes keep their length.
    run(&mut s, "swimlane.resize", json!({"id": 7, "size": 2.0}));
    assert_box("the phase resized", boxed(doc(&s), 7), (1.0, 4.0, 3.0, 8.5));
    assert_box("the next phase", boxed(doc(&s), 8), (3.0, 4.0, 7.0, 8.5));
    assert_box("a lane keeps its length", boxed(doc(&s), 4), (1.0, 7.0, 7.0, 8.0));
    assert_no_rows_of_ours(&s, &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn a_frame_whose_two_lists_no_name_tells_apart_is_left_as_it_is_and_a_list_of_plain_shapes_is_no_list_of_lanes() {
    // Which list is the lanes would be a guess: nothing is laid out, and a command that needs a structure says the page has none.
    let (mut s, _dir, _path) = opened(&sample(false));
    assert_eq!(run(&mut s, "swimlane.info", json!({}))["structures"], json!([]));
    let e = s.execute("swimlane.insert", &json!({})).unwrap_err().to_string();
    assert!(e.contains("the page has no swimlanes"), "{e}");
    // A list whose members are not containers (a legend's, say) is not one of lanes.
    let d = model(
        vec![
            tied(typed(shape_at(1, (1.0, 5.0, 3.0, 8.0)), "List"), &[(2, &[2, 3])]),
            tied(shape_at(2, (1.0, 7.0, 3.0, 8.0)), &[(5, &[1])]),
            tied(shape_at(3, (1.0, 6.0, 3.0, 7.0)), &[(5, &[1])]),
        ],
        Vec::new(),
    );
    let (mut s, _dir, _path) = opened(&d);
    assert_eq!(run(&mut s, "swimlane.info", json!({}))["structures"], json!([]));
}

#[test]
fn a_files_lane_dragged_moved_or_deleted_is_put_in_its_place_and_the_others_close_up_and_its_structure_is_not_turned() {
    let (mut s, _dir, _path) = opened(&three_lanes());
    // Dragged by its header below the last lane: it goes last, with what it holds, and the others close up.
    run(&mut s, "shape.move", json!({"ids": [3], "dx": 0.0, "dy": -2.0}));
    assert_eq!(ids_of(&structure(&mut s)["lanes"]), [2, 4, 3]);
    assert_box("the last lane came up", boxed(doc(&s), 4), (1.0, 6.0, 7.0, 7.0));
    assert_box("the lane dragged is last", boxed(doc(&s), 3), (1.0, 5.0, 7.0, 6.0));
    assert_box("and its shape went with it", boxed(doc(&s), 5), (3.0, 5.25, 4.0, 5.75));
    // Moved to the front by the command.
    run(&mut s, "swimlane.move", json!({"lane": 3, "to": 0}));
    assert_eq!(ids_of(&structure(&mut s)["lanes"]), [3, 2, 4]);
    assert_box("the first lane", boxed(doc(&s), 3), (1.0, 7.0, 7.0, 8.0));
    // Deleted: the lanes after it close the gap.
    run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(ids_of(&structure(&mut s)["lanes"]), [3, 4]);
    assert_box("the lane after the one deleted", boxed(doc(&s), 4), (1.0, 6.0, 7.0, 7.0));
    assert_no_rows_of_ours(&s, &[1, 3, 4]);
    // A turn would leave the lanes' own drawing across them: it is refused in words, and nothing changes.
    let before = boxed(doc(&s), 4);
    let e = s.execute("swimlane.orientation", &json!({"orientation": "vertical"})).unwrap_err().to_string();
    assert!(e.contains("drawn by another program") && e.contains("Orientation turns the swimlanes DiaCraft made"), "{e}");
    assert_box("nothing turned", boxed(doc(&s), 4), before);
}

#[test]
fn a_file_of_a_hundred_thousand_containers_each_naming_the_next_as_its_list_opens_is_asked_about_and_moves() {
    // The security scan's finding, from a file: the role of a container followed the chain of lists it names one link at a time and overflowed the stack.
    let n = 100_000_u32;
    let shapes: Vec<Shape> = (1..=n)
        .map(|id| {
            let next: Vec<u32> = if id < n { vec![id + 1] } else { Vec::new() };
            let mut shape = Shape::new(id);
            shape.kind = ShapeType::Shape;
            let sheet = Arc::make_mut(&mut shape.sheet);
            for (name, v) in [("PinX", 1.0), ("PinY", 1.0), ("Width", 1.0), ("Height", 1.0), ("LocPinX", 0.5), ("LocPinY", 0.5)] {
                sheet.cells.insert(CellName::new(name), length(v));
            }
            tied(typed(shape, "Container"), &[(5, &next)])
        })
        .collect();
    let (mut s, _dir, _path) = opened(&model(shapes, Vec::new()));
    assert_eq!(run(&mut s, "swimlane.info", json!({}))["structures"], json!([]), "no container of the chain is a lane");
    run(&mut s, "shape.move", json!({"ids": [n / 2], "dx": 1.0, "dy": 0.0}));
    assert_box("the container moved, and nothing was laid out", boxed(doc(&s), n / 2), (1.5, 0.5, 2.5, 1.5));
}
