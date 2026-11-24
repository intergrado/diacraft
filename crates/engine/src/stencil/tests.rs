//! The built-in stencils: every master reads back as the shape it names (for Basic Shapes, for Basic Flowchart, tests first).
//! What is drawn and saved is in `flowchart_tests.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_calc::{Budget, EvalOptions, commit};
use diacraft_doc::{CellAddr, CellName, Document, FormulaSlot, Owner, Resolver, RowKey, SectionKey, ShapeRef, SheetRef};

use super::{
    FLOWCHART_ID, FLOWCHART_NAME, MASTER_SHAPE_ID, MasterSpec, STENCIL_ID, STENCIL_NAME, all_masters, ensure_master, find_master, list, masters,
    stencils,
};

/// A document holding one master of the stencil.
fn with_master(spec: &MasterSpec) -> (Document, u32) {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, spec).unwrap();
    (doc, id)
}

fn number(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> f64 {
    let found = res.resolve(sheet, addr).unwrap_or_else(|| panic!("no cell at {addr:?}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no number at {addr:?}"))
}

/// The master shape's `Geometry` as a list of (row type, x, y) in order of `IX`.
fn path(doc: &Document, master: u32) -> Vec<(String, f64, f64)> {
    let res = Resolver::new(doc);
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Master(master), 0));
    let key = SectionKey::new("Geometry", Some(0));
    let rows = res.effective_rows(&sheet, &key).expect("a Geometry section");
    rows.iter()
        .map(|row| {
            let at =
                |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap();
            (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
        })
        .collect()
}

#[test]
fn there_are_twelve_masters_with_distinct_ids_and_names() {
    let all = masters();
    assert_eq!(all.len(), 12);
    let mut ids: Vec<&str> = all.iter().map(|m| m.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 12);
    let names: Vec<&str> = all.iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        [
            "Rectangle",
            "Square",
            "Ellipse",
            "Circle",
            "Triangle",
            "Right Triangle",
            "Diamond",
            "Pentagon",
            "Hexagon",
            "Octagon",
            "Parallelogram",
            "Trapezoid"
        ]
    );
}

#[test]
fn a_master_is_found_by_its_id_or_its_name_in_any_case() {
    for m in masters() {
        assert_eq!(find_master(m.id).map(|f| f.id), Some(m.id));
        assert_eq!(find_master(m.name).map(|f| f.id), Some(m.id));
        assert_eq!(find_master(&m.name.to_uppercase()).map(|f| f.id), Some(m.id));
    }
    assert_eq!(find_master("Right Triangle").map(|m| m.id), Some("right_triangle"));
    assert!(find_master("").is_none());
    assert!(find_master("heptagon").is_none());
    assert!(find_master(&"x".repeat(10_000)).is_none());
}

#[test]
fn every_masters_geometry_is_a_closed_path_inside_its_box() {
    for spec in masters() {
        let (doc, id) = with_master(spec);
        let rows = path(&doc, id);
        let (first, last) = (rows.first().unwrap(), rows.last().unwrap());
        if spec.id == "ellipse" || spec.id == "circle" {
            assert_eq!(rows.len(), 1, "{}: the ellipse is the section's only row", spec.name);
            assert_eq!(first.0, "Ellipse");
            assert!((first.1 - spec.width / 2.0).abs() < 1e-9 && (first.2 - spec.height / 2.0).abs() < 1e-9, "{}: centred", spec.name);
            continue;
        }
        assert_eq!(first.0, "MoveTo", "{}", spec.name);
        assert!(rows.iter().skip(1).all(|r| r.0 == "LineTo"), "{}", spec.name);
        assert!(rows.len() >= 4, "{}: a triangle has three vertices and the way back", spec.name);
        assert!((first.1 - last.1).abs() < 1e-9 && (first.2 - last.2).abs() < 1e-9, "{}: the path returns to its start", spec.name);
        for (_, x, y) in &rows {
            assert!((-1e-9..=spec.width + 1e-9).contains(x) && (-1e-9..=spec.height + 1e-9).contains(y), "{}: ({x}, {y}) is in the box", spec.name);
        }
        // The shape fills its box: the vertices reach all four sides.
        let reach = |f: fn(&(String, f64, f64)) -> f64, to: f64| rows.iter().any(|r| (f(r) - to).abs() < 1e-9);
        assert!(reach(|r| r.1, 0.0) && reach(|r| r.1, spec.width) && reach(|r| r.2, 0.0) && reach(|r| r.2, spec.height), "{}", spec.name);
    }
}

#[test]
fn every_master_has_four_or_more_connection_points_inside_its_box_with_a_direction() {
    for spec in all_masters().filter(|spec| !spec.is_connector()) {
        let (doc, id) = with_master(spec);
        let res = Resolver::new(&doc);
        let sheet = SheetRef::Shape(ShapeRef::top(Owner::Master(id), 0));
        let rows = res.effective_rows(&sheet, &SectionKey::new("Connection", None)).expect("a Connection section");
        assert!(rows.len() >= 4, "{}: {} connection points", spec.name, rows.len());
        assert_eq!(rows.len(), spec.point_count());
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.key, RowKey::Ix(u32::try_from(i).unwrap()), "{}: rows run from IX 0", spec.name);
            let at = |name: &str| number(&res, &sheet, &CellAddr::in_row("Connection", None, row.key.clone(), name));
            let (x, y, dx, dy) = (at("X"), at("Y"), at("DirX"), at("DirY"));
            assert!((-1e-9..=spec.width + 1e-9).contains(&x) && (-1e-9..=spec.height + 1e-9).contains(&y), "{}: point {i}", spec.name);
            assert_eq!(dx.abs() + dy.abs(), 1.0, "{}: point {i} leaves along an axis", spec.name);
        }
    }
}

#[test]
fn every_masters_saved_values_are_what_its_formulas_give() {
    // The values written beside the formulas are those of the default size: a recalculation of the document finds nothing to change.
    for spec in all_masters() {
        let (mut doc, _) = with_master(spec);
        let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
        assert!(!report.budget_exhausted, "{}", spec.name);
        assert_eq!((report.written, report.refused), (0, 0), "{}: a saved value differs from its formula", spec.name);
        assert!(report.unchanged > 10, "{}: the formulas were worked out ({} cells)", spec.name, report.unchanged);
    }
}

#[test]
fn every_formula_of_a_master_parses() {
    for spec in all_masters() {
        let (doc, id) = with_master(spec);
        let master = doc.master(id).unwrap();
        let shape = master.shapes.get(0).unwrap();
        assert_eq!(shape.id, MASTER_SHAPE_ID);
        let mut formulas = 0;
        let mut check = |cell: &diacraft_doc::Cell| {
            if let FormulaSlot::Local(f) = &cell.formula {
                formulas += 1;
                assert!(f.ast.is_ok(), "{}: `{}` does not parse", spec.name, f.source);
            }
        };
        shape.sheet.cells.values().for_each(&mut check);
        for section in &shape.sheet.sections {
            section.cells.values().for_each(&mut check);
            for row in &section.rows {
                row.cells.values().for_each(&mut check);
            }
        }
        assert!(formulas >= if spec.is_connector() { 11 } else { 14 }, "{}: geometry is written as formulas ({formulas})", spec.name);
    }
}

#[test]
fn a_master_is_copied_in_once_and_found_again() {
    let mut doc = Document::new_blank();
    let rect = find_master("rectangle").unwrap();
    let ellipse = find_master("ellipse").unwrap();
    let a = ensure_master(&mut doc, rect).unwrap();
    let b = ensure_master(&mut doc, ellipse).unwrap();
    assert_ne!(a, b);
    assert_eq!(ensure_master(&mut doc, rect).unwrap(), a, "the rectangle is already there");
    assert_eq!(doc.masters.len(), 2);
    let master = doc.master(a).unwrap();
    assert_eq!(master.name.as_deref(), Some("Rectangle"));
    assert!(master.unique_id.is_some() && master.raw.get("BaseID") == master.unique_id.as_deref());
    // Ids follow the highest one in use.
    let mut other = Document::new_blank();
    other.add_master(diacraft_doc::Master { id: 41, ..diacraft_doc::Master::default() }).unwrap();
    assert_eq!(ensure_master(&mut other, rect).unwrap(), 42);
}

// ---- Basic Flowchart and the two stencils ----------------------------------------------------------------------------------

fn flowchart() -> &'static [MasterSpec] {
    let all = stencils();
    assert_eq!(all.len(), 7, "the catalogue retains Basic Shapes and Basic Flowchart first");
    assert_eq!((all[1].id, all[1].name), (FLOWCHART_ID, FLOWCHART_NAME));
    all[1].masters()
}

#[test]
fn there_are_five_stencils_and_basic_shapes_keeps_its_twelve() {
    let all = stencils();
    assert_eq!(
        all.iter().map(|s| (s.id, s.name)).collect::<Vec<_>>(),
        [
            (STENCIL_ID, STENCIL_NAME),
            (FLOWCHART_ID, FLOWCHART_NAME),
            ("decorative", "Decorative Shapes"),
            ("arrows", "Arrow Shapes"),
            ("gane-sarson", "Gane-Sarson"),
            ("cross-functional", "Cross-Functional Flowchart Shapes"),
            ("orgchart", "Organization Chart Shapes")
        ]
    );
    assert_eq!(all[0].masters().len(), 12);
    assert_eq!(masters().len(), 12, "`masters()` is still Basic Shapes: the API grew and nothing changed");
    assert_eq!(all[0].masters().as_ptr(), masters().as_ptr());
    assert_eq!(all_masters().count(), 58);
}

#[test]
fn the_flowchart_stencil_holds_eighteen_symbols_in_the_order_of_the_shapes_window() {
    let names: Vec<&str> = flowchart().iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        [
            "Process",
            "Decision",
            "Terminator",
            "Data",
            "Document",
            "Multiple Documents",
            "Predefined Process",
            "Manual Input",
            "Manual Operation",
            "Preparation",
            "On-Page Reference",
            "Off-Page Reference",
            "Database",
            "Internal Storage",
            "Delay",
            "Display",
            "Summing Junction",
            "Or"
        ]
    );
}

/// The Cross-Functional Flowchart Shapes and the Organization Chart Shapes were merged with the same series, 7, so the Executive and the Manager had the
/// Swimlane's and the Separator's GUIDs: a drop of one found the other's master in the drawing and a Swimlane came out dressed as a position. Each
/// built-in library has a series of its own, which all its masters share.
#[test]
fn each_built_in_stencil_has_a_series_of_its_own_in_its_masters_guids() {
    let mut seen = std::collections::BTreeMap::new();
    for stencil in stencils() {
        let series: std::collections::BTreeSet<u8> = stencil.masters().iter().map(|m| m.identity.0).collect();
        assert_eq!(series.len(), 1, "{}: one series for the stencil: {series:?}", stencil.id);
        let one = series.into_iter().next().unwrap();
        let before = seen.insert(one, stencil.id);
        assert_eq!(before, None, "{} has the series {one} of another stencil", stencil.id);
    }
    assert_eq!(seen.keys().copied().collect::<Vec<_>>(), [1, 2, 4, 5, 6, 7, 8]);
    assert_eq!(find_master("orgchart.executive").map(MasterSpec::guid).as_deref(), Some("{D1AC0008-0000-4000-8000-000000000001}"));
    assert_eq!(find_master("xfunc.swimlane").map(MasterSpec::guid).as_deref(), Some("{D1AC0007-0000-4000-8000-000000000001}"));
}

#[test]
fn master_ids_and_guids_are_global_but_display_names_are_scoped() {
    let mut ids = std::collections::BTreeSet::new();
    for stencil in stencils() {
        let mut names = std::collections::BTreeSet::new();
        for m in stencil.masters() {
            assert!(ids.insert(m.id), "duplicate id {}", m.id);
            assert!(names.insert(m.name), "duplicate name within {}", stencil.id);
            assert_eq!(find_master(m.id).map(|found| found.id), Some(m.id));
            assert!(m.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b)));
            assert!(m.width > 0.0 && (m.height > 0.0 || m.is_connector()) && m.width <= 3.0 && m.height <= 3.0);
        }
    }
    // Fixed GUIDs, all different: a master is found again in a document by its `UniqueID`.
    let mut guids: Vec<String> = all_masters().map(MasterSpec::guid).collect();
    guids.sort_unstable();
    guids.dedup();
    assert_eq!(guids.len(), all_masters().count());
    assert!(flowchart().iter().all(|m| m.guid().starts_with("{D1AC0002-")));
    assert!(masters().iter().all(|m| m.guid().starts_with("{D1AC0001-")), "Basic Shapes keep the GUIDs they had");
    assert_eq!(find_master("rectangle").map(MasterSpec::guid).as_deref(), Some("{D1AC0001-0000-4000-8000-000000000001}"));
    assert_eq!(find_master("process").map(MasterSpec::guid).as_deref(), Some("{D1AC0002-0000-4000-8000-000000000001}"));
}

#[test]
fn a_flowchart_master_is_found_by_its_id_or_its_name_in_any_case_and_basic_shapes_still_are() {
    for m in flowchart() {
        assert_eq!(find_master(m.id).map(|f| f.id), Some(m.id));
        assert_eq!(find_master(m.name).map(|f| f.id), Some(m.id));
        assert_eq!(find_master(&m.name.to_uppercase()).map(|f| f.id), Some(m.id));
        assert!(!masters().iter().any(|b| b.id == m.id), "{}: not a Basic Shapes master", m.name);
    }
    assert_eq!(find_master("Multiple Documents").map(|m| m.id), Some("multiple_documents"));
    assert_eq!(find_master("multiple-documents").map(|m| m.id), Some("multiple_documents"));
    assert_eq!(find_master("On-Page Reference").map(|m| m.id), Some("on_page_reference"));
    assert_eq!(find_master("off page reference").map(|m| m.id), Some("off_page_reference"));
    assert_eq!(find_master("diamond").map(|m| m.id), Some("diamond"));
    assert_eq!(find_master("Decision").map(|m| m.id), Some("decision"));
    assert!(find_master("flowline").is_none() && find_master("start/end").is_none());
}

#[test]
fn stencil_list_names_both_stencils_with_every_master_and_its_size() {
    let listed = list();
    let all = listed["stencils"].as_array().unwrap();
    assert_eq!(all.len(), 7);
    assert_eq!((all[0]["id"].as_str(), all[0]["name"].as_str()), (Some("basic"), Some("Basic Shapes")));
    assert_eq!((all[1]["id"].as_str(), all[1]["name"].as_str()), (Some("flowchart"), Some("Basic Flowchart")));
    assert_eq!(all[0]["masters"].as_array().unwrap().len(), 12);
    let masters = all[1]["masters"].as_array().unwrap();
    assert_eq!(masters.len(), flowchart().len());
    assert_eq!(masters[0], serde_json::json!({"id": "process", "name": "Process", "width": 1.5, "height": 0.8, "connection_points": 4}));
    for (listed, spec) in masters.iter().zip(flowchart()) {
        assert_eq!(listed["id"], spec.id);
        assert_eq!(listed["name"], spec.name);
        assert_eq!((listed["width"].as_f64(), listed["height"].as_f64()), (Some(spec.width), Some(spec.height)));
        assert!(listed["connection_points"].as_u64().unwrap() >= 4);
    }
    // What Basic Shapes listed before is listed as it was.
    assert_eq!(
        all[0]["masters"][0],
        serde_json::json!({"id": "rectangle", "name": "Rectangle", "width": 1.5, "height": 1.0, "connection_points": 4})
    );
}

/// A resolved row of a master's geometry: its type, end point and, for an arc, the point it passes through.
struct Step {
    kind: String,
    end: (f64, f64),
    via: Option<(f64, f64)>,
}

/// A resolved `Geometry` section: whether it takes the fill, and its rows in order of `IX`.
struct Path {
    ix: u32,
    no_fill: bool,
    steps: Vec<Step>,
}

/// Every `Geometry` section of the master's shape, resolved.
fn paths(doc: &Document, master: u32) -> Vec<Path> {
    let res = Resolver::new(doc);
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Master(master), 0));
    let keys = res.effective_sections(&sheet, "Geometry").expect("Geometry sections");
    keys.iter()
        .map(|key| {
            let rows = res.effective_rows(&sheet, key).expect("rows");
            let steps = rows
                .iter()
                .map(|row| {
                    let at = |name: &str| number(&res, &sheet, &CellAddr::in_row("Geometry", key.ix, row.key.clone(), name));
                    let kind = row.row_type.as_deref().unwrap_or("").to_owned();
                    let via = (kind == "EllipticalArcTo").then(|| (at("A"), at("B")));
                    Step { end: (at("X"), at("Y")), via, kind }
                })
                .collect();
            let no_fill = number(&res, &sheet, &CellAddr::in_section("Geometry", key.ix, "NoFill")) == 1.0;
            Path { ix: key.ix.unwrap(), no_fill, steps }
        })
        .collect()
}

#[test]
fn every_flowchart_masters_paths_stay_in_the_box_reach_its_four_sides_and_close_where_they_fill() {
    for spec in flowchart() {
        let (doc, id) = with_master(spec);
        let all = paths(&doc, id);
        assert!(!all.is_empty(), "{}", spec.name);
        assert!(!all[0].no_fill, "{}: the first path takes the fill", spec.name);
        let (mut low, mut high) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for (n, path) in all.iter().enumerate() {
            assert_eq!(path.ix as usize, n, "{}: sections run from IX 0", spec.name);
            let first = &path.steps[0];
            assert!(first.kind == "MoveTo" || first.kind == "Ellipse", "{}: a path starts with a move or an ellipse", spec.name);
            if first.kind == "Ellipse" {
                assert_eq!(path.steps.len(), 1, "{}: an ellipse is alone in its section", spec.name);
                (low, high) = ((0.0, 0.0), (spec.width, spec.height));
                continue;
            }
            assert!(path.steps.iter().all(|s| matches!(s.kind.as_str(), "MoveTo" | "LineTo" | "EllipticalArcTo")), "{}", spec.name);
            for step in &path.steps {
                for (x, y) in std::iter::once(step.end).chain(step.via) {
                    assert!((-1e-9..=spec.width + 1e-9).contains(&x) && (-1e-9..=spec.height + 1e-9).contains(&y), "{}: ({x}, {y})", spec.name);
                    (low, high) = ((low.0.min(x), low.1.min(y)), (high.0.max(x), high.1.max(y)));
                }
            }
            // A filled path is one run that closes (the end meets the start).
            if !path.no_fill {
                let last = path.steps.last().unwrap();
                assert_eq!(path.steps.iter().filter(|s| s.kind == "MoveTo").count(), 1, "{}: a filled path is one run", spec.name);
                assert!((last.end.0 - first.end.0).abs() < 1e-9 && (last.end.1 - first.end.1).abs() < 1e-9, "{}: the filled path closes", spec.name);
            }
        }
        assert!(low.0.abs() < 1e-9 && low.1.abs() < 1e-9, "{}: reaches left and bottom ({low:?})", spec.name);
        assert!((high.0 - spec.width).abs() < 1e-9 && (high.1 - spec.height).abs() < 1e-9, "{}: reaches right and top ({high:?})", spec.name);
    }
}

#[test]
fn the_symbols_with_inner_strokes_keep_them_out_of_the_fill() {
    // Rims, rules and crosses are paths of their own with NoFill 1, so that the fill of the symbol is not punched by them.
    let expected = [
        ("predefined_process", 2),
        ("internal_storage", 2),
        ("database", 2),
        ("summing_junction", 2),
        ("or", 2),
        ("multiple_documents", 3),
        ("process", 1),
        ("document", 1),
        ("terminator", 1),
    ];
    for (id, count) in expected {
        let spec = find_master(id).unwrap();
        let (doc, master) = with_master(spec);
        let all = paths(&doc, master);
        assert_eq!(all.len(), count, "{id}");
        let strokes_only: Vec<bool> = all.iter().map(|p| p.no_fill).collect();
        if id == "multiple_documents" {
            assert_eq!(strokes_only, [false, false, false], "the strips are filled, and none overlaps another");
        } else {
            assert!(!strokes_only[0] && strokes_only.iter().skip(1).all(|&n| n), "{id}: {strokes_only:?}");
        }
    }
}

#[test]
fn the_slants_and_round_ends_are_formulas_of_the_height_so_they_survive_a_wider_box() {
    let formula = |id: &str, row: usize, cell: &str| {
        let spec = find_master(id).unwrap();
        let (doc, master) = with_master(spec);
        let shape = doc.master(master).unwrap().shapes.get(0).unwrap().clone();
        let section = shape.sheet.sections.iter().find(|s| s.name.as_ref() == "Geometry").unwrap().clone();
        match &section.rows[row].cells[&CellName::new(cell)].formula {
            FormulaSlot::Local(f) => f.source.to_string(),
            other => panic!("{id}: {other:?}"),
        }
    };
    assert_eq!(formula("terminator", 0, "X"), "Height*0.5", "the left end of the bottom edge");
    assert_eq!(formula("terminator", 1, "X"), "Width-Height*0.5", "where the right end begins");
    assert_eq!(formula("data", 1, "X"), "Width-Height*0.25", "the slant is a quarter of the height");
    assert_eq!(formula("data", 3, "X"), "Height*0.25");
    assert_eq!(formula("decision", 1, "X"), "Width", "a share of 1 is the bare name");
    assert_eq!(formula("decision", 0, "X"), "Width*0.5");
    assert_eq!(formula("database", 2, "D"), "(Width*0.5)/(Height*0.12)", "the rim stays a half-ellipse as the box is resized");
}

#[test]
fn each_flowchart_master_is_copied_in_once_beside_the_basic_ones() {
    let mut doc = Document::new_blank();
    let mut ids = Vec::new();
    for spec in all_masters() {
        ids.push(ensure_master(&mut doc, spec).unwrap());
    }
    let count = all_masters().count();
    assert_eq!(doc.masters.len(), count);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "every master has its own id");
    for spec in all_masters() {
        let again = ensure_master(&mut doc, spec).unwrap();
        assert_eq!(doc.master(again).unwrap().name.as_deref(), Some(spec.name));
    }
    assert_eq!(doc.masters.len(), count, "nothing is added twice");
}

// ---- which built-in stencils are open -------------------------------------------------------------------------------------------

#[test]
fn a_new_session_has_every_built_in_stencil_open_in_the_order_of_the_catalogue() {
    let ids: Vec<&str> = stencils().iter().map(|s| s.id).collect();
    let opened = super::OpenedStencils::new();
    assert_eq!(opened.open_builtins(), ids);
}

#[test]
fn built_in_stencils_open_in_order_and_ids_are_taken_in_any_case() {
    let mut opened = super::OpenedStencils::new();
    opened.set_builtins(&["basic"]);
    assert_eq!(opened.open_builtins(), ["basic"]);
    assert_eq!(opened.open_builtin(" Flowchart "), Some(true));
    assert_eq!(opened.open_builtins(), ["basic", "flowchart"], "a stencil opened by hand goes last");
    assert_eq!(opened.open_builtin("flowchart"), Some(false), "it is open already");
    assert_eq!(opened.open_builtin("nope"), None);
    assert_eq!(opened.open_builtin("file-1"), None, "a stencil file is not a built-in one");
    assert_eq!(opened.open_builtin(&"b".repeat(100)), None);
    opened.set_builtins(&["flowchart", "nope", "FLOWCHART", "basic"]);
    assert_eq!(opened.open_builtins(), ["flowchart", "basic"], "an unknown id and a repeat are left out");
    opened.set_builtins(&[]);
    assert!(opened.open_builtins().is_empty());
    assert_eq!(opened.open_builtin("flowchart"), Some(true));
    opened.reset_builtins();
    assert_eq!(opened.open_builtins(), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
}

#[test]
fn any_run_of_opens_and_sets_leaves_distinct_catalogue_ids() {
    let ids: Vec<&str> = stencils().iter().map(|s| s.id).collect();
    let mut opened = super::OpenedStencils::new();
    let mut seed: u64 = 0x0b1d_e5ee;
    let mut next = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) as usize
    };
    for _ in 0..5_000 {
        let id = ids[next() % ids.len()];
        match next() % 3 {
            0 => drop(opened.open_builtin(id)),
            1 => opened.set_builtins(&[id, ids[next() % ids.len()]]),
            _ => opened.reset_builtins(),
        }
        let open = opened.open_builtins();
        let mut seen = open.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), open.len(), "{open:?}");
        assert!(open.iter().all(|id| ids.contains(id)), "{open:?}");
    }
}
