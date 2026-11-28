//! UML Use Case (tests first): the actor drawn as a stick figure with its name under it, the use case's ellipse, the subject that holds its use cases, the lines of the design's
//! section 6.1, the search words, the Quick Shapes and the template.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::Owner;
use serde_json::json;

use super::super::flowchart_tests::{blank, render, run};
use super::super::uml_class::tests::{cached, drop_in, near, odg_notes, row_cell, text_of};
use super::super::{builtin_stencil, find_master, registered};
use crate::Session;
use crate::draw;

const STENCIL: &str = "uml-usecase";

const MASTER_IDS: [&str; 9] = [
    "uml-usecase.actor",
    "uml-usecase.use-case",
    "uml-usecase.subject",
    "uml-usecase.note",
    "uml-usecase.association",
    "uml-usecase.include",
    "uml-usecase.extend",
    "uml-usecase.generalization",
    "uml-usecase.note-link",
];

fn drop_at(s: &mut Session, master: &str, x: f64, y: f64) -> u32 {
    drop_in(s, STENCIL, master, x, y)
}

/// Whether the pixel at a place of the page (inches from its left and bottom edges) is clearly ink.
fn dark(r: &diacraft_render::Rendered, x: f64, y: f64) -> bool {
    let p = super::super::flowchart_tests::px(r, x, y);
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink(r: &diacraft_render::Rendered, (left, bottom, right, top): (f64, f64, f64, f64)) -> usize {
    let step = 1.0 / 200.0;
    let (mut n, mut y) = (0, bottom);
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

#[test]
fn the_stencil_ships_nine_masters_in_the_order_the_window_lists_them() {
    let stencil = builtin_stencil(STENCIL).expect("uml-usecase is shipped");
    assert_eq!(stencil.masters().iter().map(|m| m.id).collect::<Vec<_>>(), MASTER_IDS);
    assert_eq!(stencil.masters().iter().filter(|m| m.is_connector()).count(), 5, "five lines");
    assert!(!stencil.default_open);
    assert_eq!(registered().len(), 20);
}

#[test]
fn the_quick_shapes_are_actor_use_case_subject_and_note() {
    let mut s = blank();
    let quick = run(&mut s, "stencil.quick", json!({ "stencil": STENCIL }));
    let ids: Vec<&str> = quick["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["uml-usecase.actor", "uml-usecase.use-case", "uml-usecase.subject", "uml-usecase.note"]);
}

#[test]
fn an_actor_is_a_stick_figure_with_its_name_under_it() {
    let mut s = blank();
    let actor = drop_at(&mut s, "uml-usecase.actor", 5.5, 4.25);
    let doc = s.document().unwrap();
    assert_eq!(text_of(doc, actor), "Actor");
    // 0.5 by 1.0 inch at (5.5, 4.25): the head is a ring at the top, the arms cross at 0.62 of the height, the legs end at the bottom corners.
    let (left, bottom) = (5.25, 3.75);
    let drawn = render(&s);
    assert!(ink(&drawn, (left + 0.15, bottom + 0.88, left + 0.35, bottom + 1.0)) > 10, "a head at the top");
    assert!(ink(&drawn, (left, bottom + 0.6, left + 0.5, bottom + 0.64)) > 80, "arms across the body");
    assert!(ink(&drawn, (left, bottom, left + 0.1, bottom + 0.1)) > 5, "a foot at the left");
    // The name is under the figure, in the block below the box: ink there, and none beside the figure at that height.
    assert!(ink(&drawn, (left - 0.3, bottom - 0.4, left + 0.8, bottom - 0.05)) > 40, "the name is under the actor");
    assert!(ink(&drawn, (left + 1.0, bottom - 0.4, left + 2.0, bottom - 0.05)) == 0);
    // The connection points are on the figure: the top of the head, the two hands and the feet.
    let points = draw::connection_points(doc, 1, actor);
    assert_eq!(points.len(), 5, "the head, both hands and both feet");
    assert!(points.iter().any(|p| near(p.x, left + 0.25) && near(p.y, bottom + 1.0) && near(p.dir_y, 1.0)), "the top of the head, facing up");
    assert!(points.iter().any(|p| near(p.x, left) && near(p.y, bottom + 0.62) && near(p.dir_x, -1.0)), "the left hand");
    assert!(points.iter().any(|p| near(p.x, left + 0.5) && near(p.y, bottom + 0.62) && near(p.dir_x, 1.0)), "the right hand");
}

#[test]
fn a_use_case_is_an_ellipse_with_its_name_inside_and_eight_points_on_it() {
    let mut s = blank();
    let use_case = drop_at(&mut s, "uml-usecase.use-case", 5.5, 4.25);
    let doc = s.document().unwrap();
    assert_eq!(text_of(doc, use_case), "Use Case");
    let points = draw::connection_points(doc, 1, use_case);
    assert_eq!(points.len(), 8);
    let drawn = render(&s);
    // 1.6 by 0.8: bare paper in the corners of the box, the rim at the ends of both axes.
    assert!(!dark(&drawn, 5.5 - 0.78, 4.25 + 0.38) && !dark(&drawn, 5.5 + 0.78, 4.25 - 0.38), "an ellipse leaves the corners of its box");
    assert!(dark(&drawn, 5.5 - 0.8 + 0.004, 4.25) && dark(&drawn, 5.5, 4.25 + 0.4 - 0.004), "and its rim is at the ends of the axes");
}

#[test]
fn a_subject_holds_the_use_cases_dropped_inside_it_and_its_name_is_at_the_top_left() {
    let mut s = blank();
    let subject = drop_at(&mut s, "uml-usecase.subject", 5.5, 4.25);
    let info = run(&mut s, "container.info", json!({ "ids": [subject] }));
    assert_eq!(info["containers"][0]["id"], subject, "the subject is a container: {info}");
    let doc = s.document().unwrap();
    assert_eq!(text_of(doc, subject), "Subject");
    // The name is flush left and starts at the top of the box, whatever the size.
    assert_eq!(row_cell(doc, subject, "Paragraph", 0, "HorzAlign"), Some(0.0));
    assert_eq!(cached(doc, subject, "VerticalAlign"), Some(0.0));
    let inner = drop_at(&mut s, "uml-usecase.use-case", 5.5, 4.0);
    let info = run(&mut s, "container.info", json!({ "ids": [subject] }));
    assert_eq!(info["containers"][0]["members"], json!([inner]), "{info}");
    let was = draw::shape_corners(s.document().unwrap(), 1, inner).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [subject], "dx": 1.0, "dy": -0.5 }));
    let now = draw::shape_corners(s.document().unwrap(), 1, inner).unwrap();
    assert!(was.iter().zip(now.iter()).all(|(a, b)| near(a.0 + 1.0, b.0) && near(a.1 - 0.5, b.1)), "the use case went with its subject");
    // The box has a plain rectangle's four sides and no tab, and its points are the twelve of three on each side.
    assert_eq!(draw::connection_points(s.document().unwrap(), 1, subject).len(), 12);
}

#[test]
fn a_note_is_the_same_dog_eared_box_as_the_class_stencils_and_holds_plain_text() {
    let mut s = blank();
    let note = drop_at(&mut s, "uml-usecase.note", 5.0, 4.25);
    assert_eq!(text_of(s.document().unwrap(), note), "Note");
    assert_eq!(draw::connection_points(s.document().unwrap(), 1, note).len(), 4);
    let drawn = render(&s);
    assert!(!dark(&drawn, 5.0 + 0.7 - 0.02, 4.25 + 0.4 - 0.02), "the cut corner is bare");
}

/// The ends, pattern and text the design's section 6.1 and give each line: (master, `BeginArrow`, `EndArrow`, `LinePattern`, text).
const LINES: [(&str, f64, f64, f64, &str); 5] = [
    ("uml-usecase.association", 0.0, 0.0, 1.0, ""),
    ("uml-usecase.include", 0.0, 3.0, 2.0, "\u{ab}include\u{bb}"),
    ("uml-usecase.extend", 0.0, 3.0, 2.0, "\u{ab}extend\u{bb}"),
    ("uml-usecase.generalization", 0.0, 16.0, 1.0, ""),
    ("uml-usecase.note-link", 0.0, 0.0, 2.0, ""),
];

#[test]
fn each_relationship_has_the_ends_and_pattern_of_6_1() {
    let mut s = blank();
    for (n, (id, begin, end, pattern, text)) in LINES.iter().enumerate() {
        let line = drop_at(&mut s, id, 5.0, 1.0 + n as f64);
        let doc = s.document().unwrap();
        assert_eq!(cached(doc, line, "BeginArrow"), Some(*begin), "{id}: the begin");
        assert_eq!(cached(doc, line, "EndArrow"), Some(*end), "{id}: the end");
        assert_eq!(cached(doc, line, "LinePattern").unwrap_or(1.0), *pattern, "{id}: the pattern");
        assert_eq!(text_of(doc, line), *text, "{id}: the keyword a line carries");
        assert_eq!(cached(doc, line, "ShapeRouteStyle"), Some(2.0), "{id}: straight (ours: a use case diagram's lines run at any angle)");
        let at = doc.find_shape(Owner::Page(1), line).unwrap();
        assert!(doc.shape(&at).unwrap().sheet.cell("EndArrow").is_none(), "{id}: the instance holds no end of its own");
    }
    run(&mut s, "stencil.open", json!({ "id": STENCIL }));
    let list = run(&mut s, "stencil.list", json!({}));
    let entry = list["stencils"].as_array().unwrap().iter().find(|e| e["id"] == STENCIL).unwrap();
    for (id, begin, end, pattern, _) in LINES {
        let master = entry["masters"].as_array().unwrap().iter().find(|m| m["id"] == id).unwrap();
        assert_eq!(master["ends"], json!({ "begin": begin as u8, "end": end as u8, "pattern": pattern as u8 }), "{id}");
    }
}

#[test]
fn an_include_is_dashed_with_an_open_head_and_its_keyword_shows() {
    let mut s = blank();
    let include = drop_at(&mut s, "uml-usecase.include", 5.5, 4.25);
    let drawn = render(&s);
    // 1.5 inches long from 4.75 to 6.25 at y = 4.25: dashed (more than one run of ink along it) and a head at the end.
    let (mut runs, mut was) = (0, false);
    let mut x = 4.75;
    while x < 6.0 {
        let now = (-1..=1).any(|d| dark(&drawn, x, 4.25 + f64::from(d) / 200.0));
        runs += usize::from(now && !was);
        was = now;
        x += 1.0 / 200.0;
    }
    assert!(runs >= 3, "a dashed line is several runs of ink: {runs}");
    assert!(ink(&drawn, (6.0, 4.25 + 0.02, 6.24, 4.25 + 0.1)) > 5, "an open head at the end, beside the line");
    assert_eq!(text_of(s.document().unwrap(), include), "\u{ab}include\u{bb}");
}

#[test]
fn an_actor_is_glued_to_a_use_case_by_an_association_and_the_end_follows_it() {
    let mut s = blank();
    let actor = drop_at(&mut s, "uml-usecase.actor", 2.0, 4.0);
    let use_case = drop_at(&mut s, "uml-usecase.use-case", 7.0, 4.0);
    let link = drop_at(&mut s, "uml-usecase.association", 4.5, 7.0);
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "begin", "target": { "id": actor } }));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "end", "target": { "id": use_case } }));
    assert_eq!(s.document().unwrap().page(1).unwrap().connects.len(), 2);
    let before = cached(s.document().unwrap(), link, "EndY").unwrap();
    run(&mut s, "shape.move", json!({ "ids": [use_case], "dx": 0.0, "dy": -1.5 }));
    let after = cached(s.document().unwrap(), link, "EndY").unwrap();
    assert!(near(after, before - 1.5), "{before} to {after}");
}

#[test]
fn every_master_has_search_words_and_the_names_of_the_inventory_find_their_masters() {
    for id in MASTER_IDS {
        assert!(!super::keywords(id).is_empty(), "{id} has search words");
    }
    let mut s = blank();
    for (word, id) in [
        ("Subsystem", "uml-usecase.subject"),
        ("Actor", "uml-usecase.actor"),
        ("Use Case", "uml-usecase.use-case"),
        ("Include", "uml-usecase.include"),
        ("Extend", "uml-usecase.extend"),
        ("Inheritance", "uml-usecase.generalization"),
        ("Comment", "uml-usecase.note"),
    ] {
        let found = run(&mut s, "stencil.search", json!({ "text": word }));
        assert!(found.to_string().contains(&format!("\"{id}\"")), "`{word}` finds {id}: {found}");
    }
    assert!(find_master("uml-usecase.actor").is_some());
}

#[test]
fn the_uml_use_case_template_is_listed_and_opens_its_stencil_beside_the_basic_shapes() {
    let mut s = Session::new();
    let templates = run(&mut s, "template.list", json!({}));
    let entry =
        templates["templates"].as_array().unwrap().iter().find(|t| t["id"] == "uml_use_case").unwrap_or_else(|| panic!("listed: {templates}"));
    assert!(entry["description"].as_str().unwrap().contains("UML 2.5.1"), "{entry}");
    run(&mut s, "file.new", json!({ "template": "uml_use_case" }));
    let list = run(&mut s, "stencil.list", json!({}));
    let open: Vec<&str> = list["stencils"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(open, ["uml-usecase", "basic"]);
}

#[test]
fn a_page_of_every_master_drops_and_draws_with_nothing_skipped() {
    let mut s = blank();
    for (n, id) in MASTER_IDS.iter().enumerate() {
        for k in 0..3 {
            drop_at(&mut s, id, 1.5 + 3.0 * f64::from(k), 0.8 + n as f64 * 0.8);
        }
    }
    let drawn = render(&s);
    assert!(drawn.skipped.is_empty(), "{:?}", drawn.skipped);
}

#[test]
fn the_autoconnect_strip_passes_over_the_subject() {
    // A container is no shape to join with a connector, so it stays a Quick Shape where it is and the strip offers the others.
    let mut s = blank();
    let quick = run(&mut s, "stencil.quick", json!({ "stencil": STENCIL }));
    let in_group: Vec<&str> = quick["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert!(in_group.contains(&"uml-usecase.subject"), "the subject is a Quick Shape");
    let offered: Vec<&str> = quick["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(offered, ["uml-usecase.actor", "uml-usecase.use-case", "uml-usecase.note"]);
    let subject = quick["stencils"][0]["masters"].as_array().unwrap().iter().find(|m| m["id"] == "uml-usecase.subject").unwrap();
    assert_eq!(subject["autoconnect"], json!(false), "the listing says the strip never offers it");
}

#[test]
fn no_master_of_the_stencil_is_written_as_a_rectangle_for_other_programs_in_an_odg() {
    for id in MASTER_IDS {
        let notes = odg_notes(STENCIL, id);
        assert!(notes.iter().all(|n| !n.contains("written as a rectangle")), "{id}: {notes:?}");
    }
}

#[test]
fn every_box_of_the_stencil_holds_objtype_1_and_no_line_does() {
    super::super::uml_class::tests::assert_boxes_hold_object_type_1(STENCIL);
}
