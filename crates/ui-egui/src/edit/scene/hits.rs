//! What a click meets (the pointer's `Scene`): the text of a shape and the label of a group are there to be clicked, whatever the shape paints.
//!
//! A shape's geometry that is hidden (`NoShow`) is absent to a hit test (`Shape.HitTest`), so the markers a BPMN gateway hides are passed over. **Geometry that is shown and
//! paints nothing is still the shape's** (a path with neither fill nor line is neither hidden nor absent): the text box of the Text tool, a shape with no line and no fill and
//! the box a group's name is typed in are met inside their outlines, and a paint that is clear still finds the click. **A shape that shows text and has no geometry
//! shown is met in its text block.** These tests hold that for the Text tool's box, for shapes of every no-paint kind, for the label of every group the engine has and for a connector's text.
#![allow(clippy::unwrap_used)]

use diacraft_engine::Session;
use serde_json::json;

use super::*;
use crate::edit::plain_text;

/// A drawing with a page: the session and the page's id.
fn drawing() -> (Session, u32) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    (s, page)
}

fn dropped(s: &mut Session, page: u32, master: &str, x: f64, y: f64) -> u32 {
    let made = s.execute("shape.drop", &json!({"master": master, "page": page, "x": x, "y": y})).unwrap();
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

fn set_cell(s: &mut Session, page: u32, id: u32, cell: &str, formula: &str) {
    s.execute("sheet.set_cell", &json!({"id": id, "page": page, "cell": cell, "formula": formula})).unwrap();
}

/// The text block of shape `id` as `text.block` gives it: its four corners on the page, in inches.
fn block(s: &mut Session, page: u32, id: u32) -> [P; 4] {
    let answer = s.execute("text.block", &json!({"id": id, "page": page})).unwrap();
    let corner = |i: usize| (answer["corners"][i][0].as_f64().unwrap(), answer["corners"][i][1].as_f64().unwrap());
    [corner(0), corner(1), corner(2), corner(3)]
}

/// Where a person may click on a label: the middle of its text block and a quarter of the block's width to each side of it along the block's own axis, which are on the line of the text and
/// off the glyphs of a short one.
fn along(corners: &[P; 4]) -> Vec<P> {
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    let quarter = ((corners[1].0 - corners[0].0) / 4.0, (corners[1].1 - corners[0].1) / 4.0);
    vec![middle, (middle.0 - quarter.0, middle.1 - quarter.1), (middle.0 + quarter.0, middle.1 + quarter.1)]
}

/// Where a person may click inside a text block of a box: its middle, and a point a fifth of the way in from each corner, which are away from the glyphs of a short text.
fn inside(corners: &[P; 4]) -> Vec<P> {
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    let towards = |c: P| (c.0 + (middle.0 - c.0) * 0.2, c.1 + (middle.1 - c.1) * 0.2);
    std::iter::once(middle).chain(corners.iter().map(|c| towards(*c))).collect()
}

fn scene_of(s: &Session, page: u32) -> Scene {
    Scene::build(s.document().unwrap(), page, s.revision()).unwrap()
}

/// A shape that holds text and paints nothing is met anywhere in its text block, not only on the glyphs: the box the Text tool makes, a shape with no line and no fill, a shape whose
/// section has neither (`NoLine` and `NoFill`), and a shape whose geometry is all hidden, which is met in its text block as a shape with no geometry is.
#[test]
fn a_shape_that_holds_text_and_paints_nothing_is_met_anywhere_in_its_text_block() {
    let (mut s, page) = drawing();
    let tool = s.execute("text.box", &json!({"page": page, "x": 2.0, "y": 2.0, "text": "A label"})).unwrap();
    let tool = u32::try_from(tool["id"].as_u64().unwrap()).unwrap();
    let mut shapes = vec![("the Text tool's box", tool)];
    let cases: [(&str, &[(&str, &str)]); 4] = [
        ("no line and no fill", &[("LinePattern", "0"), ("FillPattern", "0")]),
        ("no line and no fill in the section", &[("Geometry1.NoLine", "1"), ("Geometry1.NoFill", "1")]),
        ("both", &[("LinePattern", "0"), ("FillPattern", "0"), ("Geometry1.NoLine", "1"), ("Geometry1.NoFill", "1")]),
        ("geometry that is all hidden", &[("Geometry1.NoShow", "1")]),
    ];
    for (n, (name, cells)) in cases.iter().enumerate() {
        let id = dropped(&mut s, page, "rectangle", 2.0 + 3.0 * (n + 1) as f64, 2.0);
        s.execute("shape.set_text", &json!({"id": id, "page": page, "text": "A label"})).unwrap();
        for (cell, formula) in *cells {
            set_cell(&mut s, page, id, cell, formula);
        }
        shapes.push((name, id));
    }
    let scene = scene_of(&s, page);
    for (name, id) in shapes {
        let corners = block(&mut s, page, id);
        for p in inside(&corners) {
            assert_eq!(scene.hit(p, 0.02).map(|i| i.id), Some(id), "{name}: a click at ({:.2}, {:.2}) in its text block {corners:?}", p.0, p.1);
        }
    }
}

/// The same shapes with no text are another thing: a shape whose geometry is all hidden and that shows nothing is passed over, and one with shown geometry that paints nothing is met
/// in its outline.
#[test]
fn a_shape_with_no_text_is_passed_over_when_its_geometry_is_hidden_and_met_when_it_is_shown_and_paints_nothing() {
    let (mut s, page) = drawing();
    let clear = dropped(&mut s, page, "rectangle", 2.0, 2.0);
    let hidden = dropped(&mut s, page, "rectangle", 5.0, 2.0);
    for (cell, formula) in [("LinePattern", "0"), ("FillPattern", "0")] {
        set_cell(&mut s, page, clear, cell, formula);
    }
    set_cell(&mut s, page, hidden, "Geometry1.NoShow", "1");
    let scene = scene_of(&s, page);
    assert_eq!(scene.hit((2.0, 2.0), 0.02).map(|i| i.id), Some(clear));
    assert!(scene.hit((5.0, 2.0), 0.02).is_none());
    // Text typed into the hidden one makes it met again, in its text block.
    s.execute("shape.set_text", &json!({"id": hidden, "page": page, "text": "Now it says something"})).unwrap();
    assert_eq!(scene_of(&s, page).hit((5.0, 2.0), 0.02).map(|i| i.id), Some(hidden));
}

/// The ids of the shapes under `id` (itself too) that show text, at every depth, as the document holds them.
fn text_holders(doc: &diacraft_engine::doc::Document, page: u32, id: u32) -> Vec<u32> {
    fn walk(doc: &diacraft_engine::doc::Document, page: u32, shape: &diacraft_engine::doc::Shape, out: &mut Vec<u32>) {
        if !plain_text(doc, page, shape.id).is_empty() {
            out.push(shape.id);
        }
        for child in shape.children.iter() {
            walk(doc, page, child, out);
        }
    }
    let mut out = Vec::new();
    if let Some(at) = doc.find_shape(Owner::Page(page), id)
        && let Some(shape) = doc.shape(&at)
    {
        walk(doc, page, shape, &mut out);
    }
    out
}

/// What is wrong, in words, with clicking the labels of `master` dropped on a drawing. Every shape and member that can show a label is given one, and the places looked at are the middle of each
/// label's text block and a quarter of its width to each side, **wherever the block lies**: inside its shape or outside it (the name under a BPMN event or a network device). A click there must
/// meet the shape that was dropped (the group, for a group master), and with the group selected a second click must go to a member that shows text. `Ok` with how many clicks were made.
fn label_problem(master: &crate::stencils::MasterInfo) -> Result<usize, String> {
    let (mut s, page) = drawing();
    let id = dropped(&mut s, page, &master.id, 5.0, 4.0);
    let first = scene_of(&s, page);
    let labelled: Vec<u32> = first.items.iter().chain(&first.members).filter(|m| !m.hidden && m.kind == Kind::Shape).map(|m| m.id).collect();
    for member in labelled {
        if plain_text(s.document().unwrap(), page, member).is_empty() {
            s.execute("shape.set_text", &json!({"id": member, "page": page, "text": "Label"})).unwrap();
        }
    }
    let holders = text_holders(s.document().unwrap(), page, id);
    let scene = scene_of(&s, page);
    let top = scene.get(id).ok_or("the dropped shape is not in the scene")?;
    let mut clicks = 0;
    for holder in &holders {
        let corners = block(&mut s, page, *holder);
        for p in along(&corners) {
            clicks += 1;
            let found = scene.hit(p, 0.02).map(|i| i.id);
            if found != Some(id) {
                return Err(format!("a click at ({:.2}, {:.2}) on the label of shape {holder} meets {found:?}, not shape {id}", p.0, p.1));
            }
            if *holder == id || top.select_mode == 0 || top.kind != Kind::Group {
                continue;
            }
            // The group is selected: a second click goes in, one level at a time, to a member that shows text.
            let mut at = id;
            while let Some(member) = scene.hit_child(at, p, 0.02) {
                at = member.id;
                if member.kind != Kind::Group {
                    break;
                }
            }
            if at == id || !holders.contains(&at) {
                return Err(format!(
                    "a second click at ({:.2}, {:.2}) on the label of shape {holder} goes to shape {at}, which shows no text",
                    p.0, p.1
                ));
            }
        }
    }
    Ok(clicks)
}

/// Every master the engine has, dropped: a click on its label, wherever the label lies, selects the shape, and for a group a second click selects the member that holds the text. The BPMN, UML and
/// C4 groups put their names in a member with no line and no fill, which is what a click must find; a label that sits outside its shape's frame is found by its text block.
#[test]
fn every_label_of_every_master_is_met_in_its_text_block_and_a_second_click_goes_to_the_member_that_holds_it() {
    let masters = crate::stencils::render_agreement::every_builtin_master();
    let (mut failed, mut looked) = (Vec::new(), 0);
    for master in &masters {
        match label_problem(master) {
            Ok(n) => looked += n,
            Err(why) => failed.push(format!("{}: {why}", master.id)),
        }
    }
    assert!(looked > 100, "{looked} clicks on labels in {} masters", masters.len());
    assert!(failed.is_empty(), "{} of {} masters:\n{}", failed.len(), masters.len(), failed.join("\n"));
}

/// A connector's text is clicked where it is: the middle of its text block, which is the middle of the line for a connector that is made with its text.
#[test]
fn a_connectors_text_is_met_in_its_text_block() {
    let (mut s, page) = drawing();
    let (a, b) = (dropped(&mut s, page, "rectangle", 2.0, 3.0), dropped(&mut s, page, "rectangle", 7.0, 3.0));
    let made = s.execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}, "page": page})).unwrap();
    let connector = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    s.execute("shape.set_text", &json!({"id": connector, "page": page, "text": "yes"})).unwrap();
    let scene = scene_of(&s, page);
    let corners = block(&mut s, page, connector);
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert_eq!(scene.get(connector).unwrap().kind, Kind::Connector);
    assert_eq!(scene.hit(middle, 0.05).map(|i| i.id), Some(connector), "the text block of the connector is {corners:?}");
}

/// A connector's text block that sits off its path, where a person has moved the label (the Text Block tool, `text.block_move`): a click in the block meets the connector, and the space
/// between the line and the block is still nothing.
#[test]
fn a_connectors_text_block_that_sits_off_its_path_is_met() {
    let (mut s, page) = drawing();
    let (a, b) = (dropped(&mut s, page, "rectangle", 2.0, 3.0), dropped(&mut s, page, "rectangle", 7.0, 3.0));
    let made = s.execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}, "page": page})).unwrap();
    let connector = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    s.execute("shape.set_text", &json!({"id": connector, "page": page, "text": "yes"})).unwrap();
    s.execute("text.block_move", &json!({"id": connector, "page": page, "dx": 0.0, "dy": 1.5})).unwrap();
    let scene = scene_of(&s, page);
    let corners = block(&mut s, page, connector);
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert!(middle.1 > 4.2, "the label was moved up off the line, which runs at 3.0: {corners:?}");
    assert_eq!(scene.hit(middle, 0.05).map(|i| i.id), Some(connector), "a click in the moved text block");
    let line_above = (middle.0, 3.3);
    assert!(scene.hit(line_above, 0.05).is_none(), "between the line and the block is nothing");
    assert_eq!(scene.hit((middle.0, 3.0), 0.05).map(|i| i.id), Some(connector), "and the line is met where it is");
}

/// The label of an event is under it, outside its frame, and a click on it selects the event, as does a click on the circle; the space between them is nothing. A second click on the label goes to
/// the member that holds the name.
#[test]
fn the_name_under_a_bpmn_event_is_met_and_the_space_between_the_event_and_its_name_is_not() {
    let (mut s, page) = drawing();
    let event = dropped(&mut s, page, "bpmn.start-event", 5.0, 4.0);
    let scene = scene_of(&s, page);
    let group = scene.get(event).unwrap();
    let name = scene.children_of(event).rfind(|m| !m.hidden && m.kind == Kind::Shape).unwrap().id;
    s.execute("shape.set_text", &json!({"id": name, "page": page, "text": "Order received"})).unwrap();
    let scene = scene_of(&s, page);
    let corners = block(&mut s, page, name);
    let (low, high) = (group.frame.0, group.frame.1);
    let top_of_block = corners.iter().map(|c| c.1).fold(f64::MIN, f64::max);
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert!(top_of_block < low.1, "the name's block is under the event: block top {top_of_block}, event bottom {}", low.1);
    assert_eq!(scene.hit(middle, 0.02).map(|i| i.id), Some(event), "a click on the name meets the event");
    assert_eq!(scene.hit_child(event, middle, 0.02).map(|i| i.id), Some(name), "and a second click, the member that holds the name");
    assert_eq!(scene.hit(((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0), 0.02).map(|i| i.id), Some(event), "the circle is met as before");
    if low.1 - top_of_block > 0.1 {
        let gap = ((low.0 + high.0) / 2.0, (low.1 + top_of_block) / 2.0);
        assert!(scene.hit(gap, 0.0).is_none(), "the space between the event and its name is nothing: {gap:?}");
    }
}

/// A shape whose geometry is met is met as it always was, before any shape's text block: a text block in front of another shape does not take its clicks.
#[test]
fn a_click_on_a_shapes_geometry_is_not_taken_by_the_text_block_of_a_shape_in_front() {
    let (mut s, page) = drawing();
    let behind = dropped(&mut s, page, "rectangle", 4.0, 3.0);
    let in_front = dropped(&mut s, page, "ellipse", 4.4, 3.0);
    s.execute("shape.set_text", &json!({"id": in_front, "page": page, "text": "Front"})).unwrap();
    let scene = scene_of(&s, page);
    // The ellipse is 1.5 by 1 about (4.4, 3), its box 3.65 to 5.15 and 2.5 to 3.5: the upper left corner of its box is outside the ellipse and inside the rectangle (3.25 to 4.75, 2.5 to 3.5).
    let in_the_rectangle = (3.75, 3.4);
    assert_eq!(
        scene.hit(in_the_rectangle, 0.01).map(|i| i.id),
        Some(behind),
        "the rectangle's own geometry is there, and the ellipse's text block is only the box of its text"
    );
    assert_eq!(scene.hit((4.4, 3.0), 0.01).map(|i| i.id), Some(in_front), "the ellipse is met in front where its geometry is");
    // The upper right corner of its box is on no geometry: the text block meets the shape.
    assert_eq!(scene.hit((5.05, 3.4), 0.01).map(|i| i.id), Some(in_front));
}

/// The labels of a group set to group only are met too: its members are not offered to the pointer, and their text blocks still are the group's.
#[test]
fn the_name_under_a_group_set_to_group_only_is_met() {
    let (mut s, page) = drawing();
    let event = dropped(&mut s, page, "bpmn.start-event", 5.0, 4.0);
    let name = scene_of(&s, page).children_of(event).rfind(|m| !m.hidden && m.kind == Kind::Shape).unwrap().id;
    set_cell(&mut s, page, event, "SelectMode", "0");
    s.execute("shape.set_text", &json!({"id": name, "page": page, "text": "Order received"})).unwrap();
    let scene = scene_of(&s, page);
    assert_eq!(scene.children_of(event).count(), 0, "no member is offered to the pointer");
    let corners = block(&mut s, page, name);
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert_eq!(scene.hit(middle, 0.02).map(|i| i.id), Some(event), "a click on the name meets the group");
    assert_eq!(scene.get(event).unwrap().text_blocks.len(), 1);
}

/// A group of more members that show text than an item holds blocks of keeps its first [`MAX_BLOCKS`] and no more, and is still met at all of them that it holds.
#[test]
fn a_group_of_a_great_many_labels_holds_the_blocks_it_may_and_no_more() {
    let (mut s, page) = drawing();
    let mut ids = Vec::new();
    for k in 0..(MAX_BLOCKS + 40) {
        let id = dropped(&mut s, page, "rectangle", 1.0 + 0.01 * k as f64, 4.0);
        s.execute("shape.set_text", &json!({"id": id, "page": page, "text": "x"})).unwrap();
        ids.push(id);
    }
    let made = s.execute("shape.group", &json!({"ids": ids, "page": page})).unwrap();
    let group = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let scene = scene_of(&s, page);
    assert_eq!(scene.get(group).unwrap().text_blocks.len(), MAX_BLOCKS);
}

/// Reading the text blocks of a page of shapes that all show text builds the model's lookups a few times for the page, not once for each shape: the blocks are read through the resolver the
/// scene already holds.
#[test]
fn reading_the_text_blocks_of_two_hundred_labelled_shapes_builds_no_lookup_for_each() {
    let (mut s, page) = drawing();
    for k in 0..200 {
        let id = dropped(&mut s, page, "rectangle", 1.0 + 0.04 * f64::from(k), 4.0);
        s.execute("shape.set_text", &json!({"id": id, "page": page, "text": "x"})).unwrap();
    }
    diacraft_doc::work::reset();
    let scene = scene_of(&s, page);
    let lookups = diacraft_doc::work::resolver_builds();
    assert_eq!(scene.items.len(), 200);
    assert!(scene.items.iter().all(|item| item.text_blocks.len() == 1));
    assert!(lookups <= 64, "{lookups} lookups built to read 200 shapes with their text blocks");
}
