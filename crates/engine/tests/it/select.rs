//! Selecting shapes by what they are and by where they lie: `select.by_type`, `select.by_region` and `select.choices`, through `Session::execute` as the control channel,
//! the CLI and MCP call them. Selection itself stays in the window; these answer with the `ID`s a window would select, and change nothing (they are queries, not undo steps).
//! Numbers are paper inches on a page of the default scale, where they are the cells' own. Shapes are made by hand (`shapeops::Build`), so what a test expects is the numbers it wrote.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, ForeignData, RawAttrs, RowKey, Shape, ShapeList, ShapeType, Value};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::shapeops::{Build, assert_unchanged, error_of, ids, line, put, rect, session_with};
use crate::{blank_session, run, snapshot};

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&format!("{n}"))).with_unit("IN")
}

fn text_cell(text: &str) -> Cell {
    Cell::constant(Value::from_text(text))
}

/// A one-dimensional shape (a connector's cells) from (`x0`, `y0`) to (`x1`, `y1`), drawn as the straight line between them.
fn connector(id: u32, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> Shape {
    line(id, (x0, y0), (x1, y1)).cell("BeginX", length(x0)).cell("BeginY", length(y0)).cell("EndX", length(x1)).cell("EndY", length(y1)).build()
}

fn set_user(shape: &mut Shape, name: &str, value: &str) {
    Arc::make_mut(&mut shape.sheet).set_cell(&CellAddr::in_row("User", None, RowKey::name(name), "Value"), text_cell(value)).unwrap();
}

fn picture(id: u32, foreign_type: &str) -> Shape {
    let mut shape = rect(id, (0.0, 0.0, 1.0, 1.0)).build();
    shape.kind = ShapeType::Foreign;
    shape.foreign = Some(Arc::new(ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", foreign_type)]), ..ForeignData::default() }));
    shape
}

fn group(id: u32, members: Vec<Shape>) -> Shape {
    let mut shape = rect(id, (0.0, 0.0, 1.0, 1.0)).build();
    shape.kind = ShapeType::Group;
    shape.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
    shape
}

fn guide(id: u32) -> Shape {
    let mut shape = Shape::new(id);
    shape.kind = ShapeType::Guide;
    shape
}

/// Names the page's layers: layer 0 `Walls`, 1 `Pipes`, 2 `Power`, and locks the ones in `locked`.
fn layers(s: &mut Session, locked: &[u32]) {
    s.edit("layers", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        let mut sheet = (*page.sheet).clone();
        for (ix, name) in ["Walls", "Pipes", "Power"].into_iter().enumerate() {
            let ix = u32::try_from(ix).unwrap();
            sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Name"), text_cell(name)).unwrap();
            let lock = if locked.contains(&ix) { "1" } else { "0" };
            sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Lock"), text_cell(lock)).unwrap();
        }
        page.sheet = Arc::new(sheet);
        Ok(())
    })
    .unwrap();
}

fn on_layers(shape: Build, members: &str) -> Shape {
    shape.cell("LayerMember", text_cell(members)).build()
}

/// A page of one of each kind: 1 and 2 shapes, 3 a group of one member, 4 a guide, 5 a bitmap, 6 a metafile, 7 an object, 8 ink, 9 a connector, 10 a container.
fn kinds() -> Session {
    let mut container = rect(10, (5.0, 5.0, 6.0, 6.0)).build();
    set_user(&mut container, "msvStructureType", "Container");
    session_with(vec![
        rect(1, (1.0, 1.0, 2.0, 2.0)).build(),
        rect(2, (3.0, 1.0, 4.0, 2.0)).build(),
        group(3, vec![rect(31, (0.0, 0.0, 0.5, 0.5)).build()]),
        guide(4),
        picture(5, "Bitmap"),
        picture(6, "EnhMetaFile"),
        picture(7, "Object"),
        picture(8, "Ink"),
        connector(9, (2.0, 1.5), (3.0, 1.5)),
        container,
    ])
}

fn by_type(s: &mut Session, params: Json) -> Vec<u32> {
    ids(&run(s, "select.by_type", params), "ids")
}

fn by_region(s: &mut Session, params: Json) -> Vec<u32> {
    ids(&run(s, "select.by_region", params), "ids")
}

// ---- select.by_type ----------------------------------------------------------------------------------------------------------------

#[test]
fn each_shape_type_of_the_dialog_picks_the_shapes_of_that_type_in_page_order() {
    let mut s = kinds();
    // A shape is a Type "Shape" one: the plain shapes, the connector and the container are all of it (a role is a different question).
    assert_eq!(by_type(&mut s, json!({"types": ["shape"]})), [1, 2, 9, 10]);
    assert_eq!(by_type(&mut s, json!({"types": ["group"]})), [3]);
    assert_eq!(by_type(&mut s, json!({"types": ["guide"]})), [4]);
    assert_eq!(by_type(&mut s, json!({"types": ["bitmap"]})), [5]);
    assert_eq!(by_type(&mut s, json!({"types": ["metafile"]})), [6]);
    assert_eq!(by_type(&mut s, json!({"types": ["ole"]})), [7]);
    assert_eq!(by_type(&mut s, json!({"types": ["ink"]})), [8]);
}

#[test]
fn types_ticked_together_pick_every_shape_of_any_of_them_in_page_order() {
    let mut s = kinds();
    assert_eq!(by_type(&mut s, json!({"types": ["ink", "group", "bitmap"]})), [3, 5, 8]);
    // The words are not case-sensitive and a word given twice picks nothing twice.
    assert_eq!(by_type(&mut s, json!({"types": ["Group", "GROUP"]})), [3]);
}

#[test]
fn a_metafile_is_told_by_the_picture_when_the_shape_names_no_type() {
    let mut emf = picture(1, "x");
    emf.foreign = Some(Arc::new(ForeignData {
        picture: Some(Arc::new(diacraft_doc::Media {
            part: Arc::from("/visio/media/a.emf"),
            content_type: Arc::from("image/x-emf"),
            bytes: Arc::from(&[][..]),
        })),
        ..ForeignData::default()
    }));
    let mut png = emf.clone();
    png.id = 2;
    png.foreign = Some(Arc::new(ForeignData {
        picture: Some(Arc::new(diacraft_doc::Media {
            part: Arc::from("/visio/media/a.png"),
            content_type: Arc::from("image/png"),
            bytes: Arc::from(&[][..]),
        })),
        ..ForeignData::default()
    }));
    let mut s = session_with(vec![emf, png]);
    assert_eq!(by_type(&mut s, json!({"types": ["metafile"]})), [1]);
    assert_eq!(by_type(&mut s, json!({"types": ["bitmap"]})), [2]);
}

#[test]
fn a_role_picks_connectors_or_containers_whatever_their_type() {
    let mut s = kinds();
    assert_eq!(by_type(&mut s, json!({"roles": ["connector"]})), [9]);
    assert_eq!(by_type(&mut s, json!({"roles": ["container"]})), [10]);
    assert_eq!(by_type(&mut s, json!({"roles": ["container", "connector"]})), [9, 10]);
    // A list is a container too: a list shape is also a container.
    let mut list = rect(11, (7.0, 7.0, 8.0, 8.0)).build();
    set_user(&mut list, "msvStructureType", "List");
    put(&mut s, vec![list]);
    assert_eq!(by_type(&mut s, json!({"roles": ["container"]})), [10, 11]);
}

#[test]
fn a_type_and_a_role_together_pick_the_shapes_that_are_both() {
    let mut s = kinds();
    assert_eq!(by_type(&mut s, json!({"types": ["shape"], "roles": ["connector"]})), [9]);
    assert_eq!(by_type(&mut s, json!({"types": ["group"], "roles": ["connector"]})), Vec::<u32>::new());
}

#[test]
fn a_layer_picks_the_shapes_on_it_by_name_or_number_and_two_layers_pick_either() {
    let mut s = session_with(vec![
        on_layers(rect(1, (1.0, 1.0, 2.0, 2.0)), "0"),
        on_layers(rect(2, (3.0, 1.0, 4.0, 2.0)), "1;2"),
        rect(3, (5.0, 1.0, 6.0, 2.0)).build(),
        on_layers(rect(4, (7.0, 1.0, 8.0, 2.0)), "2"),
    ]);
    layers(&mut s, &[]);
    assert_eq!(by_type(&mut s, json!({"layers": ["Walls"]})), [1]);
    assert_eq!(by_type(&mut s, json!({"layers": ["Power"]})), [2, 4]);
    assert_eq!(by_type(&mut s, json!({"layers": [0, "Pipes"]})), [1, 2]);
    // A shape on no layer is on none of them.
    assert_eq!(by_type(&mut s, json!({"layers": ["Walls", "Pipes", "Power"]})), [1, 2, 4]);
    // A layer and a type together: the shapes of that type on that layer.
    assert_eq!(by_type(&mut s, json!({"types": ["group"], "layers": ["Power"]})), Vec::<u32>::new());
    assert_eq!(by_type(&mut s, json!({"types": ["shape"], "layers": ["Power"]})), [2, 4]);
}

#[test]
fn a_shape_on_a_locked_layer_is_not_picked_and_the_answer_says_how_many_were_left_out() {
    let mut s = session_with(vec![
        on_layers(rect(1, (1.0, 1.0, 2.0, 2.0)), "0"),
        on_layers(rect(2, (3.0, 1.0, 4.0, 2.0)), "1"),
        // On a locked layer and an open one: locked all the same.
        on_layers(rect(3, (5.0, 1.0, 6.0, 2.0)), "0;1"),
    ]);
    layers(&mut s, &[0]);
    let answer = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(ids(&answer, "ids"), [2]);
    assert_eq!(answer["locked"], 2, "{answer}");
    // Asking for the locked layer itself finds nothing, and says why.
    let answer = run(&mut s, "select.by_type", json!({"layers": ["Walls"]}));
    assert_eq!(ids(&answer, "ids"), Vec::<u32>::new());
    assert_eq!(answer["locked"], 2, "{answer}");
}

#[test]
fn a_shape_deleted_from_its_master_is_not_there_to_select() {
    let mut gone = rect(2, (3.0, 1.0, 4.0, 2.0)).build();
    gone.del = true;
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 2.0, 2.0)).build(), gone]);
    assert_eq!(by_type(&mut s, json!({"types": ["shape"]})), [1]);
}

#[test]
fn the_answer_names_the_page_counts_the_ids_and_changes_nothing() {
    let mut s = kinds();
    let before = snapshot(&s);
    let answer = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(answer["page"], 1);
    assert_eq!(answer["count"], 4);
    assert!(answer.get("unread").is_none() && answer.get("locked").is_none(), "{answer}");
    assert_unchanged(&s, &before, "a query");
    assert_eq!(snapshot(&s).journal, before.journal, "a query is not journaled");
}

#[test]
fn by_type_refuses_in_words() {
    let mut s = kinds();
    let before = snapshot(&s);
    for (params, words) in [
        (json!({}), "give types, roles or layers"),
        (json!({"types": []}), "give types, roles or layers"),
        (json!({"types": ["triangle"]}), "no shape type `triangle`; the types are shape, group, guide, ole, metafile, bitmap and ink"),
        (json!({"roles": ["rope"]}), "no role `rope`; the roles are connector and container"),
        (json!({"layers": ["Nowhere"]}), "the page has no layer `Nowhere`"),
        (json!({"layers": [9]}), "the page has no layer 9"),
        (json!({"types": "shape"}), "`types` must be an array of words"),
        (json!({"types": [3]}), "`types` must hold words"),
        (json!({"layers": [true]}), "`layers` must hold layer names or numbers"),
        (json!({"types": ["shape"], "page": 9}), "there is no page 9"),
        (json!({"types": ["shape"], "pages": 1}), "pages"),
    ] {
        let error = error_of(&mut s, "select.by_type", &params);
        assert!(error.to_string().contains(words), "{params}: {error}");
    }
    assert_unchanged(&s, &before, "refused");
}

#[test]
fn a_list_of_too_many_words_is_refused_before_any_is_read() {
    let mut s = kinds();
    let many: Vec<Json> = (0..65).map(|_| json!("shape")).collect();
    let error = error_of(&mut s, "select.by_type", &json!({"types": many}));
    assert!(error.to_string().contains("at most 64"), "{error}");
    let many: Vec<Json> = (0..65).map(|i| json!(format!("layer {i}"))).collect();
    let error = error_of(&mut s, "select.by_type", &json!({"layers": many}));
    assert!(error.to_string().contains("at most 64"), "{error}");
}

#[test]
fn a_page_with_more_shapes_than_a_selection_reads_says_how_many_it_left_unread() {
    let most = diacraft_engine::draw::MAX_SELECT_TYPE_SHAPES;
    let mut s = blank_session();
    put(&mut s, (1..=u32::try_from(most + 10).unwrap()).map(|id| Build::new(id, 1.0, 1.0).build()).collect());
    let answer = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(ids(&answer, "ids").len(), most, "the first {most} are read");
    assert_eq!(answer["unread"], 10, "{answer}");
}

// ---- select.choices ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_choices_are_the_types_the_roles_and_the_layers_of_the_page_with_what_each_holds() {
    let mut s = session_with(vec![
        on_layers(rect(1, (1.0, 1.0, 2.0, 2.0)), "0"),
        on_layers(Build::new(2, 1.0, 1.0), "0;1"),
        connector(3, (2.0, 1.5), (3.0, 1.5)),
        guide(4),
    ]);
    layers(&mut s, &[1]);
    let answer = run(&mut s, "select.choices", json!({}));
    let kinds: Vec<(&str, u64)> =
        answer["types"].as_array().unwrap().iter().map(|t| (t["word"].as_str().unwrap(), t["count"].as_u64().unwrap())).collect();
    assert_eq!(kinds, [("shape", 3), ("group", 0), ("guide", 1), ("ole", 0), ("metafile", 0), ("bitmap", 0), ("ink", 0)]);
    let roles: Vec<(&str, u64)> =
        answer["roles"].as_array().unwrap().iter().map(|t| (t["word"].as_str().unwrap(), t["count"].as_u64().unwrap())).collect();
    assert_eq!(roles, [("connector", 1), ("container", 0)]);
    let on: Vec<(u64, &str, bool, u64)> = answer["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["ix"].as_u64().unwrap(), l["name"].as_str().unwrap(), l["locked"].as_bool().unwrap(), l["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(on, [(0, "Walls", false, 2), (1, "Pipes", true, 1), (2, "Power", false, 0)]);
    assert_eq!(answer["page"], 1);
}

#[test]
fn the_choices_of_a_page_with_no_layers_have_none_and_the_labels_are_the_dialogs() {
    let mut s = kinds();
    let answer = run(&mut s, "select.choices", json!({}));
    assert_eq!(answer["layers"], json!([]));
    let labels: Vec<&str> = answer["types"].as_array().unwrap().iter().map(|t| t["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Shapes", "Groups", "Guides", "OLE objects", "Metafiles", "Bitmaps", "Ink objects"]);
}

// ---- select.by_region: a net ---------------------------------------------------------------------------------------------------------

/// Three rectangles in a row (1: x 1 to 2, 2: x 3 to 4, 3: x 5 to 6, all y 1 to 2) and a tall one above them (4: x 1 to 6, y 4 to 5).
fn row() -> Session {
    session_with(vec![
        rect(1, (1.0, 1.0, 2.0, 2.0)).build(),
        rect(2, (3.0, 1.0, 4.0, 2.0)).build(),
        rect(3, (5.0, 1.0, 6.0, 2.0)).build(),
        rect(4, (1.0, 4.0, 6.0, 5.0)).build(),
    ])
}

#[test]
fn a_net_takes_the_shapes_wholly_inside_it_in_page_order() {
    let mut s = row();
    // Wholly: the third rectangle sticks out of the net, the fourth is above it.
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 4.5, 2.5]})), [1, 2]);
    // A shape whose box is exactly the net is inside it.
    assert_eq!(by_region(&mut s, json!({"rect": [1.0, 1.0, 2.0, 2.0]})), [1]);
    // The corners of the net may be given in any order.
    assert_eq!(by_region(&mut s, json!({"rect": [4.5, 2.5, 0.5, 0.5]})), [1, 2]);
    assert_eq!(by_region(&mut s, json!({"rect": [0.0, 0.0, 10.0, 10.0]})), [1, 2, 3, 4]);
    assert_eq!(by_region(&mut s, json!({"rect": [7.0, 7.0, 9.0, 9.0]})), Vec::<u32>::new());
}

#[test]
fn a_net_given_as_a_rectangle_or_as_four_points_is_the_same_net() {
    let mut s = row();
    let polygon = json!({"polygon": [[0.5, 0.5], [4.5, 0.5], [4.5, 2.5], [0.5, 2.5]]});
    assert_eq!(by_region(&mut s, polygon), [1, 2]);
    // A closing point that repeats the first is no fifth corner.
    let closed = json!({"polygon": [[0.5, 0.5], [4.5, 0.5], [4.5, 2.5], [0.5, 2.5], [0.5, 0.5]]});
    assert_eq!(by_region(&mut s, closed), [1, 2]);
}

#[test]
fn with_partial_on_a_net_also_takes_the_shapes_it_only_touches_or_covers_in_part() {
    let mut s = row();
    // The net reaches into the third rectangle and the tall one's neighbour is out of reach.
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 4.5, 2.5], "partial": true})), [1, 2]);
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 5.5, 2.5], "partial": true})), [1, 2, 3]);
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 5.5, 2.5], "partial": false})), [1, 2]);
    // A net across the middle of the third rectangle and of the tall one.
    assert_eq!(by_region(&mut s, json!({"rect": [5.4, 1.5, 5.6, 4.5], "partial": true})), [3, 4]);
    assert_eq!(by_region(&mut s, json!({"rect": [5.4, 1.5, 5.6, 4.5]})), Vec::<u32>::new());
}

#[test]
fn a_net_wholly_inside_a_big_shape_is_in_part_over_it_and_takes_it_with_partial_on() {
    let mut s = row();
    assert_eq!(by_region(&mut s, json!({"rect": [3.2, 4.2, 3.4, 4.4], "partial": true})), [4]);
    assert_eq!(by_region(&mut s, json!({"rect": [3.2, 4.2, 3.4, 4.4]})), Vec::<u32>::new());
}

#[test]
fn a_net_that_only_touches_the_edge_of_a_shape_is_in_part_over_it() {
    let mut s = row();
    assert_eq!(by_region(&mut s, json!({"rect": [2.0, 1.2, 2.6, 1.8], "partial": true})), [1]);
}

#[test]
fn a_shape_off_the_net_is_not_taken_with_partial_on_either() {
    let mut s = row();
    assert_eq!(by_region(&mut s, json!({"rect": [2.2, 2.2, 2.8, 3.8], "partial": true})), Vec::<u32>::new());
}

#[test]
fn a_turned_shape_is_taken_by_its_box_where_the_turn_carries_it() {
    // A 2 by 1 rectangle at (5, 5) turned 90 degrees stands 1 wide and 2 tall: x 4.5 to 5.5, y 4 to 6.
    let mut s = session_with(vec![
        Build::new(1, 2.0, 1.0).pin(5.0, 5.0).angle_degrees(90.0).section(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)], true, false).build(),
    ]);
    assert_eq!(by_region(&mut s, json!({"rect": [4.4, 3.9, 5.6, 6.1]})), [1]);
    assert_eq!(by_region(&mut s, json!({"rect": [3.9, 4.4, 6.1, 5.6]})), Vec::<u32>::new(), "its unturned box would fit this net; it does not");
    assert_eq!(by_region(&mut s, json!({"rect": [3.9, 4.4, 6.1, 5.6], "partial": true})), [1]);
}

#[test]
fn a_net_on_a_page_of_another_scale_is_in_paper_inches() {
    let mut s = row();
    // Two drawing units to a paper inch: the shapes are as big and as far as before in drawing units and half as big and as far on the paper.
    s.edit("a scale", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        let mut sheet = (*page.sheet).clone();
        sheet.set_cell(&CellAddr::direct("PageScale"), length(1.0)).unwrap();
        sheet.set_cell(&CellAddr::direct("DrawingScale"), length(2.0)).unwrap();
        page.sheet = Arc::new(sheet);
        Ok(())
    })
    .unwrap();
    // The first rectangle is x 0.5 to 1 and y 0.5 to 1 on the paper.
    assert_eq!(by_region(&mut s, json!({"rect": [0.4, 0.4, 1.1, 1.1]})), [1]);
    assert_eq!(by_region(&mut s, json!({"rect": [0.4, 0.4, 2.1, 1.1]})), [1, 2]);
}

// ---- select.by_region: a lasso -------------------------------------------------------------------------------------------------------

#[test]
fn a_loop_takes_the_shapes_wholly_inside_it_and_a_shape_across_its_edge_only_with_partial_on() {
    let mut s = row();
    // A triangle with its long side across the second rectangle: the first rectangle is inside, the second is crossed, the third and the tall one are out of reach.
    let triangle = json!([[0.0, 0.0], [4.5, 0.0], [0.0, 4.5]]);
    assert_eq!(by_region(&mut s, json!({"polygon": triangle})), [1]);
    assert_eq!(by_region(&mut s, json!({"polygon": triangle, "partial": true})), [1, 2]);
}

#[test]
fn a_shape_whose_box_corner_is_outside_the_loop_is_not_wholly_inside_it() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build()]);
    // A loop that cuts the box's upper right corner off.
    let cut = json!([[0.0, 0.0], [4.0, 0.0], [4.0, 1.5], [1.5, 4.0], [0.0, 4.0]]);
    assert_eq!(by_region(&mut s, json!({"polygon": cut})), Vec::<u32>::new());
    assert_eq!(by_region(&mut s, json!({"polygon": cut, "partial": true})), [1]);
    // A loop with the corner left in takes it.
    let whole = json!([[0.0, 0.0], [4.0, 0.0], [4.0, 3.5], [3.5, 4.0], [0.0, 4.0]]);
    assert_eq!(by_region(&mut s, json!({"polygon": whole})), [1]);
}

#[test]
fn a_loop_that_winds_round_a_shape_and_leaves_a_gap_takes_what_is_in_its_body() {
    // A "C" that opens to the right: the shape is in the open mouth, where the loop does not cover.
    let c = json!([[0.0, 0.0], [5.0, 0.0], [5.0, 1.0], [1.0, 1.0], [1.0, 4.0], [5.0, 4.0], [5.0, 5.0], [0.0, 5.0]]);
    let mut s = session_with(vec![rect(1, (2.0, 2.0, 3.0, 3.0)).build(), rect(2, (0.2, 2.0, 0.8, 3.0)).build()]);
    assert_eq!(by_region(&mut s, json!({"polygon": c})), [2], "the one in the loop's body, not the one in its mouth");
    assert_eq!(by_region(&mut s, json!({"polygon": c, "partial": true})), [2]);
}

#[test]
fn a_loop_wholly_inside_a_shape_is_in_part_over_it() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 6.0, 6.0)).build()]);
    let small = json!([[2.0, 2.0], [3.0, 2.0], [2.5, 3.0]]);
    assert_eq!(by_region(&mut s, json!({"polygon": small, "partial": true})), [1]);
    assert_eq!(by_region(&mut s, json!({"polygon": small})), Vec::<u32>::new());
}

#[test]
fn a_connector_is_taken_by_its_path_wholly_inside_or_across_with_partial_on() {
    let mut s = session_with(vec![connector(1, (1.0, 1.0), (5.0, 1.0)), connector(2, (2.0, 2.0), (3.0, 2.5))]);
    // A net over the middle of the first connector, which neither of its ends is in; the second connector is wholly in it.
    let net = json!({"rect": [1.8, 0.5, 3.5, 2.8]});
    assert_eq!(by_region(&mut s, net.clone()), [2]);
    let mut net_partial = net;
    net_partial["partial"] = json!(true);
    assert_eq!(by_region(&mut s, net_partial), [1, 2]);
}

#[test]
fn an_elbow_connector_is_taken_by_the_path_it_draws_and_not_by_the_line_between_its_ends() {
    // A one-dimensional shape from (1, 1) to (3, 1) whose route goes up to y 3 and across: its box is the line between its ends, the route is what is drawn.
    let elbow = Build::new(1, 2.0, 0.0)
        .pin(2.0, 1.0)
        .section(&[(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0)], false, true)
        .cell("BeginX", length(1.0))
        .cell("BeginY", length(1.0))
        .cell("EndX", length(3.0))
        .cell("EndY", length(1.0))
        .build();
    let mut s = session_with(vec![elbow]);
    // A net across the top of the route takes it in part; a net on the line between its ends, which is not drawn, does not; nor does one in the mouth of the route.
    assert_eq!(by_region(&mut s, json!({"rect": [1.8, 2.8, 2.2, 3.2], "partial": true})), [1]);
    assert_eq!(by_region(&mut s, json!({"rect": [1.8, 0.8, 2.2, 1.2], "partial": true})), Vec::<u32>::new());
    assert_eq!(by_region(&mut s, json!({"rect": [1.8, 1.5, 2.2, 2.5], "partial": true})), Vec::<u32>::new());
    // Wholly: the whole route, bends included, must be in the net.
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 3.5, 2.5]})), Vec::<u32>::new());
    assert_eq!(by_region(&mut s, json!({"rect": [0.5, 0.5, 3.5, 3.5]})), [1]);
}

#[test]
fn a_zero_area_loop_takes_nothing_wholly_and_the_shapes_it_runs_across_with_partial_on() {
    let mut s = row();
    let flat = json!([[0.0, 1.5], [3.5, 1.5], [7.0, 1.5]]);
    assert_eq!(by_region(&mut s, json!({"polygon": flat})), Vec::<u32>::new());
    assert_eq!(by_region(&mut s, json!({"polygon": flat, "partial": true})), [1, 2, 3]);
}

// ---- refusals and hostile inputs -----------------------------------------------------------------------------------------------------

#[test]
fn by_region_refuses_in_words_and_changes_nothing() {
    let mut s = row();
    let before = snapshot(&s);
    let many: Vec<Json> = (0..2049).map(|i| json!([f64::from(i), 0.0])).collect();
    for (params, words) in [
        (json!({}), "give `polygon` or `rect`"),
        (json!({"rect": [0, 0, 1, 1], "polygon": [[0, 0], [1, 0], [1, 1]]}), "give `polygon` or `rect`, not both"),
        (json!({"polygon": [[0, 0], [1, 1]]}), "a region needs at least three different points"),
        (json!({"polygon": [[0, 0], [0, 0], [1, 1], [1, 1]]}), "a region needs at least three different points"),
        (json!({"polygon": many}), "`polygon` holds at most 2048 points"),
        (json!({"polygon": "x"}), "`polygon` must be an array of [x, y] pairs"),
        (json!({"polygon": [[0, 0], [1], [2, 2]]}), "each point of `polygon` must be a pair of numbers"),
        (json!({"polygon": [[0, 0], ["a", "b"], [2, 2]]}), "each point of `polygon` must be a pair of finite numbers"),
        (json!({"polygon": [[0, 0], [1e300, 0], [2, 2]]}), "within 1000000"),
        (json!({"rect": [0, 0, 1]}), "`rect` must be [x0, y0, x1, y1]"),
        (json!({"rect": [0, 0, 1, "a"]}), "`rect` must be [x0, y0, x1, y1]"),
        (json!({"rect": [0, 0, 1, 1], "partial": 1}), "`partial` must be true or false"),
        (json!({"rect": [0, 0, 1, 1], "page": 9}), "there is no page 9"),
    ] {
        let error = error_of(&mut s, "select.by_region", &params);
        assert!(error.to_string().contains(words), "{params}: {error}");
    }
    assert_unchanged(&s, &before, "refused");
}

#[test]
fn a_loop_too_complex_for_the_shapes_it_is_set_over_is_refused_in_words_and_the_work_is_counted() {
    // 3,000 small shapes in a field and a loop of 2,048 points over all of them, in partial mode: each shape's nine segments are tried against every edge.
    let shapes: Vec<Shape> = (0..3_000u32)
        .map(|i| {
            let (col, row) = (f64::from(i % 60), f64::from(i / 60));
            rect(i + 1, (col + 0.1, row + 0.1, col + 0.6, row + 0.6)).build()
        })
        .collect();
    let mut s = session_with(shapes);
    // A zigzag round the whole field: it keeps all its points inside the field's box, so none is skipped for being out of the way.
    let zigzag: Vec<Json> = (0..2048u32).map(|i| json!([if i % 2 == 0 { -1.0 } else { 61.0 }, f64::from(i) * 60.0 / 2047.0 - 0.5])).collect();
    let error = error_of(&mut s, "select.by_region", &json!({"polygon": zigzag, "partial": true}));
    let words = error.to_string();
    assert!(words.contains("too many") && words.contains("simpler loop"), "{words}");
    // A simpler loop over the same shapes is answered, and the work it cost is in the answer and under the cap.
    let simple = json!([[-1.0, -1.0], [61.0, -1.0], [61.0, 51.0], [-1.0, 51.0]]);
    let answer = run(&mut s, "select.by_region", json!({"polygon": simple, "partial": true}));
    assert_eq!(ids(&answer, "ids").len(), 3_000);
    assert!(answer["work"].as_u64().unwrap() <= u64::try_from(diacraft_engine::draw::MAX_SELECT_WORK).unwrap(), "{answer}");
}

#[test]
fn a_page_with_more_shapes_than_a_net_reads_says_how_many_it_left_unread() {
    let most = diacraft_engine::draw::MAX_SELECT_REGION_SHAPES;
    let mut s = blank_session();
    put(&mut s, (1..=u32::try_from(most + 7).unwrap()).map(|id| rect(id, (1.0, 1.0, 1.5, 1.5)).build()).collect());
    let answer = run(&mut s, "select.by_region", json!({"rect": [0.0, 0.0, 9.0, 9.0]}));
    assert_eq!(ids(&answer, "ids").len(), most);
    assert_eq!(answer["unread"], 7, "{answer}");
}

#[test]
fn a_shape_whose_geometry_is_hostile_costs_the_budget_and_is_taken_by_its_box() {
    // A shape with a polyline of 500,000 points in one cell (5 MB of constructor text, past what the page's budget of outlines pays for): the shape is taken by its box alone and the answer says so.
    let huge = format!("POLYLINE(0,0{})", ",0.55,0.55".repeat(500_000));
    let mut shape = rect(1, (1.0, 1.0, 2.0, 2.0)).build();
    let sheet = Arc::make_mut(&mut shape.sheet);
    let mut section = diacraft_doc::Section::new("Geometry", Some(1));
    let mut row = diacraft_doc::Row { ix: Some(1), row_type: Some(Arc::from("MoveTo")), ..diacraft_doc::Row::default() };
    row.cells.insert(diacraft_doc::CellName::new("X"), length(0.0));
    row.cells.insert(diacraft_doc::CellName::new("Y"), length(0.0));
    section.rows.push(Arc::new(row));
    let mut poly = diacraft_doc::Row { ix: Some(2), row_type: Some(Arc::from("PolylineTo")), ..diacraft_doc::Row::default() };
    poly.cells.insert(diacraft_doc::CellName::new("X"), length(1.0));
    poly.cells.insert(diacraft_doc::CellName::new("Y"), length(1.0));
    poly.cells.insert(diacraft_doc::CellName::new("A"), text_cell(&huge).with_unit("POLYLINE"));
    section.rows.push(Arc::new(poly));
    sheet.sections.push(Arc::new(section));
    let mut s = session_with(vec![shape]);
    let answer = run(&mut s, "select.by_region", json!({"rect": [0.0, 0.0, 3.0, 3.0]}));
    assert_eq!(ids(&answer, "ids"), [1]);
    assert_eq!(answer["coarse"], 1, "{answer}");
}

// ---- the cost of hostile geometry and hostile layer lists (the security scan's three findings) -----------------------------------------

/// A shape box (1, 1) to (2, 2) with a second section of one cubic whose two control points are `far` times its size away: a curve that a fine tolerance cuts into millions of pieces.
fn far_curve(far: f64) -> Shape {
    let mut shape = rect(1, (1.0, 1.0, 2.0, 2.0)).build();
    let sheet = Arc::make_mut(&mut shape.sheet);
    let mut section = diacraft_doc::Section::new("Geometry", Some(1));
    let mut start = diacraft_doc::Row { ix: Some(1), row_type: Some(Arc::from("MoveTo")), ..diacraft_doc::Row::default() };
    start.cells.insert(diacraft_doc::CellName::new("X"), length(0.0));
    start.cells.insert(diacraft_doc::CellName::new("Y"), length(0.0));
    section.rows.push(Arc::new(start));
    let mut curve = diacraft_doc::Row { ix: Some(2), row_type: Some(Arc::from("RelCubBezTo")), ..diacraft_doc::Row::default() };
    for (name, value) in [("X", 1.0), ("Y", 1.0), ("A", far), ("B", far), ("C", -far), ("D", far)] {
        curve.cells.insert(diacraft_doc::CellName::new(name), text_cell(&format!("{value}")));
    }
    section.rows.push(Arc::new(curve));
    sheet.sections.push(Arc::new(section));
    shape
}

#[test]
fn a_curve_with_control_points_far_away_is_not_cut_into_millions_of_pieces_and_the_shape_is_taken_by_its_box() {
    let most = u64::try_from(diacraft_engine::draw::MAX_SHAPE_POINTS).unwrap();
    for far in [1.0e7, 1.0e10, 1.0e14] {
        let mut s = session_with(vec![far_curve(far)]);
        // A triangle is no net, so the paths are cut into pieces for the tests against its edges; the shape's box is wholly in it.
        let triangle = json!([[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]);
        for partial in [false, true] {
            let answer = run(&mut s, "select.by_region", json!({"polygon": triangle, "partial": partial}));
            assert_eq!(ids(&answer, "ids"), [1], "{far} {answer}");
            assert!(answer["steps"].as_u64().unwrap() <= most, "the pieces made are counted and bounded, whatever the curve: {far} {answer}");
            assert_eq!(answer["coarse"], 1, "the shape is taken by its box and the answer says so: {far} {answer}");
        }
    }
}

#[test]
fn an_ordinary_curve_is_still_cut_into_pieces_and_taken_by_its_path() {
    // A cubic that bends 0.5 inch off its chord: a few dozen pieces, no coarse shape.
    let mut s = session_with(vec![far_curve(0.5)]);
    let triangle = json!([[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]);
    let answer = run(&mut s, "select.by_region", json!({"polygon": triangle, "partial": true}));
    assert_eq!(ids(&answer, "ids"), [1]);
    assert!(answer.get("coarse").is_none(), "{answer}");
    let steps = answer["steps"].as_u64().unwrap();
    assert!(steps > 2 && steps < 500, "{answer}");
}

/// A master whose shape holds `list` in its `LayerMember`, and `count` instances of it on page 1 (which hold no `LayerMember` of their own), under the three layers of [`layers`] with
/// the ones in `locked` locked. Under an instance is on no layer for what its master lists.
fn instances_of_a_listing_master(count: u32, list: &str, locked: &[u32]) -> Session {
    let mut s = blank_session();
    let mut master_shape = Build::new(1, 1.0, 1.0).cell("LayerMember", text_cell(list)).build();
    master_shape.master = None;
    s.edit("a master", |doc| {
        doc.add_master(diacraft_doc::Master { id: 5, shapes: ShapeList::from_vec(vec![Arc::new(master_shape)]), ..diacraft_doc::Master::default() })
            .unwrap();
        Ok(())
    })
    .unwrap();
    let instances: Vec<Shape> = (1..=count)
        .map(|id| {
            let mut shape = Shape::new(id);
            shape.kind = ShapeType::Shape;
            shape.master = Some(5);
            shape
        })
        .collect();
    put(&mut s, instances);
    layers(&mut s, locked);
    s
}

/// `count` shapes on page 1 that each hold a `LayerMember` of `bytes` digits (one text shared by all, so the page costs a pointer each), under the layers of [`layers`] with layer 0 locked.
fn holding(count: u32, bytes: usize) -> Session {
    let cell = text_cell(&"9".repeat(bytes));
    let mut s = blank_session();
    put(&mut s, (1..=count).map(|id| Build::new(id, 1.0, 1.0).cell("LayerMember", cell.clone()).build()).collect());
    layers(&mut s, &[0]);
    s
}

/// the specifications' one rule for a shape's layers: its **own** `LayerMember` cell, never its master's, as the selections read it: an instance with no list
/// of its own is on no layer although its master lists one, so a selection by that layer leaves it out, a layer that is locked does not leave it out of a selection, and the
/// dialog counts it on no layer. The layer commands read the same way (`layer.list` counts the shapes whose own cell lists the layer), so the two agree.
#[test]
fn an_instance_with_no_list_of_its_own_is_on_no_layer_for_the_selections_as_for_the_layer_commands_whatever_its_master_lists() {
    // Shapes 1 and 2 are instances of a master that lists layer 1 (Pipes); shape 3 holds the same list itself.
    let build = |locked: &[u32]| {
        let mut s = instances_of_a_listing_master(2, "1", locked);
        put(&mut s, vec![on_layers(Build::new(3, 1.0, 1.0), "1")]);
        s
    };
    // By layer: only the shape whose own cell lists Pipes.
    let mut s = build(&[]);
    assert_eq!(by_type(&mut s, json!({"layers": ["Pipes"]})), [3], "the instances' master lists Pipes, and they are not on it");
    assert_eq!(by_type(&mut s, json!({"types": ["shape"]})), [1, 2, 3]);
    // The dialog counts it the same way, and so does the layer command.
    let choices = run(&mut s, "select.choices", json!({}));
    let pipes = choices["layers"].as_array().unwrap().iter().find(|l| l["name"] == "Pipes").cloned().unwrap();
    assert_eq!(pipes["count"], 1, "{choices}");
    let listed = run(&mut s, "layer.list", json!({}));
    let pipes = listed["layers"].as_array().unwrap().iter().find(|l| l["name"] == "Pipes").cloned().unwrap();
    assert_eq!(pipes["shapes"], 1, "{listed}");
    // A locked layer leaves out the shape that is on it and not the instances, which are on none.
    let mut s = build(&[1]);
    let answer = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(ids(&answer, "ids"), [1, 2], "{answer}");
    assert_eq!(answer["locked"], 1, "only shape 3 is on the locked layer: {answer}");
    let answer = run(&mut s, "select.by_region", json!({"rect": [-5.0, -5.0, 5.0, 5.0]}));
    assert_eq!(ids(&answer, "ids"), [1, 2], "{answer}");
    assert_eq!(answer["locked"], 1, "{answer}");
}

#[test]
fn a_layer_list_of_a_megabyte_that_every_shape_holds_is_not_read_for_any_of_them() {
    let (count, cap) = (76_000u32, u64::try_from(diacraft_engine::draw::MAX_SELECT_TYPE_SHAPES).unwrap());
    let mut s = holding(count, 1 << 20);
    // The yardstick: what the page costs to read with a layer list that is short.
    let answer = run(&mut s, "select.by_type", json!({"layers": ["Pipes"]}));
    assert_eq!(ids(&answer, "ids"), Vec::<u32>::new());
    assert_eq!(answer["looked_at"], cap, "{answer}");
    assert_eq!(answer["unread"], u64::from(count) - cap, "{answer}");
    assert_eq!(answer["overlong"], cap, "each of the shapes read had a list too long to read, and says so: {answer}");
    assert!(answer.get("read").is_none(), "and none of its bytes was copied or parsed: {answer}");
    // The same for the dialog's counts and for a region on a page that has a locked layer.
    let answer = run(&mut s, "select.choices", json!({}));
    assert_eq!(answer["overlong"], cap, "{answer}");
    assert_eq!(answer["read"], 0, "{answer}");
    let answer = run(&mut s, "select.by_region", json!({"rect": [-5.0, -5.0, 5.0, 5.0]}));
    assert_eq!(answer["overlong"], cap.min(10_000), "{answer}");
    assert!(answer.get("read").is_none(), "{answer}");
}

#[test]
fn a_layer_list_of_a_megabyte_on_a_master_is_read_for_none_of_its_instances() {
    // The instances hold no list of their own, so under nothing is read for them: no text is looked at, none is counted too long, and a locked layer leaves none out.
    let mut s = instances_of_a_listing_master(2_000, &"9".repeat(1 << 20), &[0]);
    let answer = run(&mut s, "select.by_type", json!({"layers": ["Pipes"]}));
    assert_eq!(ids(&answer, "ids"), Vec::<u32>::new());
    assert!(answer.get("overlong").is_none() && answer.get("read").is_none(), "nothing was read: {answer}");
    let answer = run(&mut s, "select.choices", json!({}));
    assert_eq!((answer["overlong"].as_u64().unwrap_or(0), answer["read"].as_u64()), (0, Some(0)), "{answer}");
    let answer = run(&mut s, "select.by_region", json!({"rect": [-5.0, -5.0, 5.0, 5.0]}));
    assert!(answer.get("overlong").is_none() && answer.get("read").is_none(), "{answer}");
    assert_eq!(ids(&answer, "ids").len(), 2_000, "every instance, none of them on a locked layer: {answer}");
}

#[test]
fn a_list_of_layers_within_the_cap_is_read_and_a_shape_on_a_locked_layer_is_still_left_out() {
    // 8 KiB of "0;" pairs lists layer 0 four thousand times: within the cap, and only the first thousand pieces are read.
    let mut shape = Build::new(1, 1.0, 1.0).cell("LayerMember", text_cell(&"0;".repeat(4_000))).build();
    shape.kind = ShapeType::Shape;
    let mut s = session_with(vec![shape]);
    layers(&mut s, &[0]);
    let answer = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(ids(&answer, "ids"), Vec::<u32>::new(), "on layer 0, which is locked");
    assert_eq!(answer["locked"], 1, "{answer}");
    assert!(answer.get("overlong").is_none(), "{answer}");
    assert!(answer["read"].as_u64().unwrap() <= 8_000, "{answer}");
}

/// `count` shapes, shape `i` on the thousand layers from `i` on (round the page's `layers` layers), and the page's layer table of `layers` rows.
fn on_a_thousand_layers(count: u32, layers: u32) -> Session {
    let list = |first: u32| (0..1_000).map(|j| ((first + j) % layers).to_string()).collect::<Vec<_>>().join(";");
    let mut s = session_with((0..count).map(|i| Build::new(i + 1, 1.0, 1.0).cell("LayerMember", text_cell(&list(i))).build()).collect());
    s.edit("layers", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        let mut sheet = (*page.sheet).clone();
        for ix in 0..layers {
            sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Name"), text_cell(&format!("L{ix}"))).unwrap();
        }
        page.sheet = Arc::new(sheet);
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn putting_the_shapes_of_a_page_on_four_thousand_layers_costs_one_lookup_for_each_membership_and_not_a_scan() {
    let mut s = on_a_thousand_layers(1_000, 4_096);
    let answer = run(&mut s, "select.choices", json!({}));
    let memberships: u64 = answer["layers"].as_array().unwrap().iter().map(|l| l["count"].as_u64().unwrap()).sum();
    assert_eq!(memberships, 1_000 * 1_000, "every membership was counted");
    assert!(answer["lookups"].as_u64().unwrap() <= memberships, "one lookup for each, not a scan of the table for each: {}", answer["lookups"]);
    // A query by layer, whose shapes are matched against a set.
    let answer = run(&mut s, "select.by_type", json!({"layers": ["L500"]}));
    assert_eq!(ids(&answer, "ids").len(), 501, "the shapes 0 to 500, whose thousand layers reach layer 500: {answer}");
}

#[test]
fn shapes_that_list_more_layers_between_them_than_a_selection_reads_are_refused_in_words() {
    // Two thousand shapes on a thousand layers each: about ten megabytes of lists, past what one selection reads.
    let mut s = on_a_thousand_layers(2_000, 4_096);
    let before = snapshot(&s);
    for (command, params) in [("select.choices", json!({})), ("select.by_type", json!({"layers": ["L1"]}))] {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains("layer memberships") && error.to_string().contains("at most"), "{command}: {error}");
    }
    assert_unchanged(&s, &before, "refused");
}

#[test]
fn a_page_with_ten_thousand_layers_is_refused_in_words_by_both_commands_that_read_them() {
    let mut s = session_with(vec![Build::new(1, 1.0, 1.0).cell("LayerMember", text_cell("0;9999")).build()]);
    s.edit("layers", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        let mut sheet = (*page.sheet).clone();
        for ix in 0..10_000u32 {
            sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Name"), text_cell("x")).unwrap();
        }
        page.sheet = Arc::new(sheet);
        Ok(())
    })
    .unwrap();
    for (command, params) in
        [("select.choices", json!({})), ("select.by_type", json!({"layers": [0]})), ("select.by_region", json!({"rect": [0, 0, 3, 3]}))]
    {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains("10000 layers") && error.to_string().contains("at most 4096"), "{command}: {error}");
    }
}
