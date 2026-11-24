//! The stamp of a section (`diacraft_doc::stamp_sections`) compares exactly what [`section_same`] compares (and plan
//! The model path keeps and drops a held value on the edits on which the splice writer keeps and drops the attribute), and the stamp of one section by
//! its `IX` (`stamp_section`) what [`route_same`] compares of a connector's route (`Geometry` section 0). `section_same` is a section's name, place, flag, **`raw` attributes and kept nodes**, and each row's key,
//! **local name**, type, flag, **`raw` attributes and kept nodes** and cells (each as `cell_same` compares it), row by row in order; the stamp of the first version left
//! out the five in bold, so a held geometry stayed good beside a section that gained an attribute or a kept node.

use diacraft_doc::{Cell, CellName, Passthrough, RawAttrs, RawNode, Row, Section, Sheet, Value, stamp_section, stamp_sections};

use super::{route_same, section_same};

fn constant(value: &str) -> Cell {
    Cell::constant(Value::from_text(value))
}

fn row(ix: u32, kind: &str, x: &str) -> Row {
    let mut row = Row { ix: Some(ix), row_type: Some(kind.into()), ..Row::default() };
    row.cells.insert(CellName::new("X"), constant(x));
    row.cells.insert(CellName::new("Y"), Cell::with_formula("X*2", Some(Value::from_text("0"))));
    row
}

fn kept(after: u32, text: &str) -> Passthrough {
    let mut nodes = Passthrough::new();
    nodes.push(after, RawNode::Comment(text.into()));
    nodes
}

/// A section `Geometry` with `IX` 0, a cell of its own and two rows.
fn base() -> Section {
    let mut section = Section::new("Geometry", Some(0));
    section.cells.insert(CellName::new("NoFill"), constant("0"));
    section.rows.push(std::sync::Arc::new(row(0, "MoveTo", "1")));
    section.rows.push(std::sync::Arc::new(row(1, "LineTo", "2")));
    section
}

fn sheet_of(sections: Vec<Section>) -> Sheet {
    Sheet { sections: sections.into_iter().map(std::sync::Arc::new).collect(), ..Sheet::default() }
}

/// Every edit of the base the pairs are made of, each a name and the change.
type Edit = (&'static str, Box<dyn Fn(&mut Section)>);

fn edits() -> Vec<Edit> {
    fn row_mut(s: &mut Section, at: usize) -> &mut Row {
        std::sync::Arc::make_mut(&mut s.rows[at])
    }
    vec![
        ("name", Box::new(|s| s.name = diacraft_doc::SectionName::new("Scratch"))),
        ("ix", Box::new(|s| s.ix = Some(1))),
        ("no ix", Box::new(|s| s.ix = None)),
        ("del", Box::new(|s| s.del = true)),
        ("raw", Box::new(|s| s.raw = RawAttrs::from_pairs([("Extra", "1")]))),
        ("raw value", Box::new(|s| s.raw = RawAttrs::from_pairs([("Extra", "2")]))),
        ("passthrough", Box::new(|s| s.passthrough = kept(0, "c"))),
        ("passthrough place", Box::new(|s| s.passthrough = kept(2, "c"))),
        ("passthrough text", Box::new(|s| s.passthrough = kept(0, "d"))),
        ("a cell of the section", Box::new(|s| drop(s.cells.insert(CellName::new("NoLine"), constant("0"))))),
        ("the value of a cell of the section", Box::new(|s| drop(s.cells.insert(CellName::new("NoFill"), constant("1"))))),
        ("a row less", Box::new(|s| drop(s.rows.pop()))),
        ("a row more", Box::new(|s| s.rows.push(std::sync::Arc::new(row(2, "LineTo", "3"))))),
        ("the rows swapped", Box::new(|s| s.rows.reverse())),
        ("row ix", Box::new(|s| row_mut(s, 1).ix = Some(7))),
        ("row name", Box::new(|s| row_mut(s, 1).name = Some("r".into()))),
        ("row local name", Box::new(|s| row_mut(s, 1).local_name = Some("Zeile".into()))),
        ("row local name changed", Box::new(|s| row_mut(s, 0).local_name = Some("A".into()))),
        ("row type", Box::new(|s| row_mut(s, 1).row_type = Some("ArcTo".into()))),
        ("row del", Box::new(|s| row_mut(s, 1).del = true)),
        ("row raw", Box::new(|s| row_mut(s, 1).raw = RawAttrs::from_pairs([("Extra", "1")]))),
        ("row passthrough", Box::new(|s| row_mut(s, 1).passthrough = kept(0, "c"))),
        ("row passthrough place", Box::new(|s| row_mut(s, 1).passthrough = kept(1, "c"))),
        ("row passthrough text", Box::new(|s| row_mut(s, 1).passthrough = kept(0, "d"))),
        ("row cell value", Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("X"), constant("9"))))),
        ("row cell respelled", Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("X"), constant("1.0"))))),
        ("row cell unit", Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("X"), constant("1").with_unit("IN"))))),
        (
            "row formula text",
            Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("Y"), Cell::with_formula("X*3", Some(Value::from_text("0")))))),
        ),
        (
            "row formula result",
            Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("Y"), Cell::with_formula("X*2", Some(Value::from_text("5")))))),
        ),
        ("a row cell gone", Box::new(|s| drop(row_mut(s, 0).cells.remove("Y")))),
        ("a row cell more", Box::new(|s| drop(row_mut(s, 0).cells.insert(CellName::new("Z"), constant("1"))))),
    ]
}

fn stamped(section: &Section, names: &[&str]) -> diacraft_doc::Stamp {
    stamp_sections(&sheet_of(vec![section.clone()]), names)
}

#[test]
fn a_stamp_of_a_section_says_what_section_same_says() {
    let original = base();
    let mut same = 0;
    let mut different = 0;
    let mut versions: Vec<(String, Section)> = vec![("the base".to_owned(), original.clone())];
    for (name, edit) in edits() {
        let mut once = original.clone();
        edit(&mut once);
        versions.push((name.to_owned(), once));
    }
    // Every pair of versions, the edits and the base, and each edit against itself (two copies of one section).
    for (i, (name_a, a)) in versions.iter().enumerate() {
        for (j, (name_b, b)) in versions.iter().enumerate() {
            let names = [a.name.as_str(), b.name.as_str()];
            let expected = section_same(a, b, &mut 0);
            assert_eq!(stamped(a, &names) == stamped(b, &names), expected, "{name_a} and {name_b} ({i}, {j})");
            if expected {
                same += 1;
            } else {
                different += 1;
            }
        }
    }
    // Never vacuous: a good many pairs of each answer.
    assert!(same >= versions.len() && different > 20 * versions.len() / 2, "{same} the same, {different} different");
}

#[test]
fn a_section_that_gains_an_attribute_or_a_kept_node_is_a_different_section_to_the_stamp() {
    // The five fields the first version left out, each alone against the base.
    let original = base();
    let names = ["Geometry"];
    let before = stamped(&original, &names);
    for name in ["raw", "passthrough", "row raw", "row passthrough", "row local name"] {
        let (_, edit) = edits().into_iter().find(|(n, _)| *n == name).unwrap();
        let mut edited = original.clone();
        edit(&mut edited);
        assert_ne!(before, stamped(&edited, &names), "{name}");
        assert!(!section_same(&original, &edited, &mut 0), "{name}");
    }
}

/// The route of a sheet as the splice writer reads it, against the stamp of the section by `IX` 0.
fn route_stamp(sheet: &Sheet) -> diacraft_doc::Stamp {
    stamp_section(sheet, "Geometry", Some(0))
}

#[test]
fn the_stamp_of_one_section_by_ix_says_what_route_same_says() {
    let route = base();
    let mut other_geometry = base();
    other_geometry.ix = Some(1);
    let mut changed_route = base();
    std::sync::Arc::make_mut(&mut changed_route.rows[0]).cells.insert(CellName::new("X"), constant("42"));
    let mut changed_other = other_geometry.clone();
    std::sync::Arc::make_mut(&mut changed_other.rows[0]).cells.insert(CellName::new("X"), constant("42"));
    let mut scratch = Section::new("Scratch", Some(0));
    scratch.cells.insert(CellName::new("Q"), constant("1"));
    let mut attributed = base();
    attributed.raw = RawAttrs::from_pairs([("Extra", "1")]);
    let mut no_ix = base();
    no_ix.ix = None;
    let sheets: Vec<(&str, Sheet)> = vec![
        ("route only", sheet_of(vec![route.clone()])),
        ("route and another geometry", sheet_of(vec![route.clone(), other_geometry.clone()])),
        ("another geometry first", sheet_of(vec![other_geometry.clone(), route.clone()])),
        ("the other geometry changed", sheet_of(vec![route.clone(), changed_other.clone()])),
        ("the route changed", sheet_of(vec![changed_route.clone(), other_geometry.clone()])),
        ("the route changed alone", sheet_of(vec![changed_route])),
        ("no route", sheet_of(vec![other_geometry.clone()])),
        ("no section", Sheet::default()),
        ("a scratch section too", sheet_of(vec![scratch, route.clone()])),
        ("the route with an attribute", sheet_of(vec![attributed])),
        ("a geometry with no ix", sheet_of(vec![no_ix])),
        ("two routes", sheet_of(vec![route.clone(), route])),
    ];
    let mut same = 0;
    for (name_a, a) in &sheets {
        for (name_b, b) in &sheets {
            let expected = route_same(a, b, &mut 0);
            assert_eq!(route_stamp(a) == route_stamp(b), expected, "{name_a} and {name_b}");
            same += usize::from(expected);
        }
    }
    assert!(same > sheets.len() && same < sheets.len() * sheets.len() - 10, "{same}");
}
