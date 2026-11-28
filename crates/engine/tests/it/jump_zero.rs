//! A route is no other for the sign of a zero. The rows of a connector's route are written in the connector's own frame, so a point that is on the connector's axis is a number a hair
//! either side of 0, and nine places of rounding take it to a `0` or to a `-0` by the sign of the hair. The router wrote one and the jump pass worked out the other, so the jump pass wrote
//! the route again (a commit more when something reads the route, a step of the history, a difference in a saved file), and a `-0` was written into cells and into files.
//!
//! The page here: two shapes joined by a connector of three legs that goes round, and a line down the page across its first leg, which carries the hop (the page's code is 4).

use diacraft_container::zip::Archive;
use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, id_of};
use crate::odg_build::Dir;
use crate::{blank_session, check_session, run, snapshot};

/// Every cell of the drawing whose value is a negative zero, as `shape section row cell`.
fn minus_zeros(d: &Document) -> Vec<String> {
    fn walk(out: &mut Vec<String>, at: &str, shape: &diacraft_doc::Shape) {
        let here = format!("{at}/shape:{}", shape.id);
        for (name, cell) in shape.sheet.cells.iter() {
            if cell.value.as_ref().is_some_and(|v| v.text() == "-0") {
                out.push(format!("{here} {}", name.as_str()));
            }
        }
        for section in &shape.sheet.sections {
            for row in &section.rows {
                for (name, cell) in row.cells.iter() {
                    if cell.value.as_ref().is_some_and(|v| v.text() == "-0") {
                        out.push(format!("{here} {}:{:?} row {:?} {}", section.name.as_str(), section.ix, row.ix, name.as_str()));
                    }
                }
            }
        }
        for child in shape.children.iter() {
            walk(out, &here, child);
        }
    }
    let mut out = Vec::new();
    for page in &d.pages {
        for shape in page.shapes.iter() {
            walk(&mut out, &format!("page:{}", page.id), shape);
        }
    }
    out
}

/// The first `-0` in `text` that is a number of its own: not the end of a longer one (`1e-05`), a part of a date or an identifier, or the start of `-0.5`.
fn minus_zero_in(text: &str) -> Option<String> {
    for (at, _) in text.match_indices("-0") {
        let before = text.get(..at).and_then(|t| t.chars().next_back());
        let after = text.get(at + 2..).and_then(|t| t.chars().next());
        let starts = before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '+')));
        let ends = after.is_none_or(|c| !(c.is_ascii_digit() || c == '.'));
        if starts && ends {
            let from = text.floor_char_boundary(at.saturating_sub(60));
            let to = text.ceil_char_boundary((at + 40).min(text.len()));
            return Some(text.get(from..to).unwrap_or_default().to_owned());
        }
    }
    None
}

/// The two connectors, the shapes the first is glued to, and the small shapes that read cells of the routes: with `readers`, the pin of each is the cell of a route
/// (`Sheet.<c>!Geometry1.X2`); without, a constant, so that the same page has nothing that reads a route.
struct Page {
    s: Session,
    a: u32,
    b: u32,
    c: u32,
    line: u32,
}

fn page(readers: bool) -> Page {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let line = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 4.0, "y": 1.0}, "to": {"x": 4.0, "y": 8.0}})));
    run(&mut s, "connector.line_jumps", json!({"code": 4}));
    for (connector, row) in [(c, 2), (c, 3), (line, 3)] {
        let tag = drop_shape(&mut s, "rectangle", 1.0, 1.0);
        for (cell, axis) in [("PinX", "X"), ("PinY", "Y")] {
            let formula = if readers { format!("Sheet.{connector}!Geometry1.{axis}{row}") } else { "1".to_owned() };
            run(&mut s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": formula}));
        }
    }
    Page { s, a, b, c, line }
}

/// The recalculation the command made, counted: the commits of `diacraft_calc::work`.
fn commits_of(s: &mut Session, command: &str, params: Value) -> u64 {
    diacraft_calc::work::reset();
    run(s, command, params);
    diacraft_calc::work::commits()
}

#[test]
fn the_page_holds_a_hop_where_the_connectors_cross_and_no_negative_zero_to_start_with() {
    let p = page(true);
    let rows = |id: u32| crate::edit::geometry(doc(&p.s), id);
    assert_eq!(rows(p.c).len(), 4, "the connector goes round in three legs");
    assert!(rows(p.line).iter().any(|row| row.0 == "ArcTo"), "the line carries the hop over the connector: {:?}", rows(p.line));
    assert_eq!(minus_zeros(doc(&p.s)), Vec::<String>::new());
}

#[test]
fn the_edits_of_a_page_of_crossing_connectors_write_no_negative_zero_into_a_cell() {
    let mut p = page(true);
    let other = drop_shape(&mut p.s, "rectangle", 2.0, 9.0);
    let edits = [
        ("a move", "shape.move", json!({"ids": [p.b], "dx": 1.5, "dy": 3.0})),
        ("a move by nothing", "shape.move", json!({"ids": [p.b], "dx": 0.0, "dy": 0.0})),
        ("a move of the other end", "shape.move", json!({"ids": [p.a], "dx": 0.5, "dy": -0.5})),
        ("a move back", "shape.move", json!({"ids": [p.b], "dx": -1.5, "dy": -3.0})),
        ("a resize", "shape.resize", json!({"id": p.b, "x": 9.0, "y": 6.0, "width": 3.0, "height": 2.0})),
        ("a reconnect to the same shape", "connector.reconnect", json!({"id": p.c, "end": "end", "target": {"id": p.b, "point": 1}})),
        ("a reconnect to another", "connector.reconnect", json!({"id": p.c, "end": "end", "target": {"id": other, "point": 1}})),
        ("a reset", "connector.reset", json!({"ids": [p.c]})),
        ("a leg moved by hand", "connector.edit_path", json!({"id": p.c, "leg": 0, "to": 4.5})),
        ("the page's jumps changed", "connector.line_jumps", json!({"code": 1, "style": 3})),
        ("the page's jumps back", "connector.line_jumps", json!({"code": 4, "style": 0})),
    ];
    for (what, command, params) in edits {
        run(&mut p.s, command, params);
        assert_eq!(minus_zeros(doc(&p.s)), Vec::<String>::new(), "after {what}");
    }
    assert_eq!(check_session(&p.s), Ok(()));
}

#[test]
fn a_move_that_changes_nothing_makes_one_commit_for_what_reads_the_route_and_none_for_a_route_written_again() {
    // The commit of the edit, and one for the cells that read the routes the edit wrote: no commit more for the jump pass, which finds the rows it would write to be the rows that are
    // there. Counted against the same page where nothing reads a route.
    for (what, dx, dy) in [("a move by nothing", 0.0, 0.0), ("a move", 1.5, 3.0), ("a move back", -1.5, -3.0)] {
        let (mut plain, mut tied) = (page(false), page(true));
        let without = commits_of(&mut plain.s, "shape.move", json!({"ids": [plain.b], "dx": dx, "dy": dy}));
        let with = commits_of(&mut tied.s, "shape.move", json!({"ids": [tied.b], "dx": dx, "dy": dy}));
        // A move by nothing writes no cell and works nothing out: no commit for it, and none for what reads a route, which no edit wrote.
        let made = u64::from(dx != 0.0 || dy != 0.0);
        assert_eq!(without, made, "{what}: with nothing to read a route the edit makes the one commit it made");
        assert_eq!(with, if made == 1 { without + 1 } else { 0 }, "{what}: {with} commits where {without} are made with nothing to read a route");
    }
}

#[test]
fn a_move_by_nothing_leaves_the_drawing_as_it_was() {
    let mut p = page(true);
    run(&mut p.s, "shape.move", json!({"ids": [p.b], "dx": 1.5, "dy": 3.0}));
    let before = doc(&p.s).clone();
    for id in [p.a, p.b] {
        run(&mut p.s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.0}));
        assert!(*doc(&p.s) == before, "a move of shape {id} by nothing changed the drawing");
    }
    let line = id_of(&run(&mut p.s, "connector.add", json!({"from": {"x": 6.0, "y": 1.0}, "to": {"x": 6.0, "y": 8.0}})));
    let before = doc(&p.s).clone();
    run(&mut p.s, "shape.move", json!({"ids": [p.b], "dx": 0.0, "dy": 0.0}));
    assert!(*doc(&p.s) == before, "with the line {line} across it too");
}

#[test]
fn a_reroute_with_the_same_inputs_changes_nothing_and_a_reset_of_it_is_no_step_of_the_history() {
    let mut p = page(true);
    run(&mut p.s, "connector.reset", json!({"ids": [p.c]}));
    let before = snapshot(&p.s);
    run(&mut p.s, "connector.reset", json!({"ids": [p.c]}));
    let after = snapshot(&p.s);
    assert!(after.document == before.document, "a connector reset to the route it has is the drawing as it was");
    assert_eq!((after.undo_steps, after.revision), (before.undo_steps, before.revision), "and no step of the history, and no revision");
    // Glued again to the shape it is glued to: the same route.
    run(&mut p.s, "connector.reconnect", json!({"id": p.c, "end": "end", "target": {"id": p.b, "point": 1}}));
    assert!(doc(&p.s) == after.document.as_ref().unwrap(), "a reconnect to the same shape and point is the drawing as it was");
    assert_eq!(minus_zeros(doc(&p.s)), Vec::<String>::new());
}

/// The text of every entry of the package `bytes`, which any of the three formats is.
fn texts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.ends_with(".xml") || name.ends_with(".rels"))
        .map(|(at, name)| {
            (name.clone(), String::from_utf8_lossy(&archive.inflate(at, &mut diacraft_container::Work::default()).unwrap()).into_owned())
        })
        .collect()
}

#[test]
fn a_saved_file_holds_no_negative_zero_in_any_of_the_three_formats() {
    let mut p = page(true);
    run(&mut p.s, "shape.move", json!({"ids": [p.b], "dx": 1.5, "dy": 3.0}));
    run(&mut p.s, "shape.move", json!({"ids": [p.a], "dx": 0.5, "dy": -0.5}));
    run(&mut p.s, "shape.resize", json!({"id": p.b, "x": 10.5, "y": 9.0, "width": 3.0, "height": 2.0}));
    let dir = Dir::new();
    for name in ["drawing.vsdx", "drawing.odg", "drawing.diacraft"] {
        let path = dir.path(name);
        run(&mut p.s, "file.save_as", json!({ "path": path }));
        let bytes = std::fs::read(&path).unwrap();
        for (part, text) in texts(&bytes) {
            assert_eq!(minus_zero_in(&text), None, "{name}: a `-0` in {part}");
        }
    }
    assert_eq!(minus_zeros(doc(&p.s)), Vec::<String>::new());
}

#[test]
fn the_search_for_a_negative_zero_finds_one_and_only_one() {
    assert!(minus_zero_in(r#"<Cell N="X" V="-0" U="IN"/>"#).is_some());
    assert!(minus_zero_in("svg:x=\"-0cm\"").is_some());
    assert!(minus_zero_in("M 0 -0 L 5 1").is_some());
    for fine in [r#"V="-0.5""#, r#"V="1e-05""#, "2026-11-27T09:25:32", "{4F1E-0A3B}", r#"V="-0.0001""#, "-05", "x-0y"] {
        assert_eq!(minus_zero_in(fine), None, "{fine}");
    }
}
