//! Reading a sheet: `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate`, and the `sheetview` module under them.
//!
//! The spec is (for the order and the sections, for the texts of values) and the design of
//! the window (section 6 for the budgets and 7.1 to 7.3 for the commands). A cost is held to a count,
//! never to a time.

use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, Owner, Resolver, RowKey, ShapeRef, ShapeType, SheetRef, Value as Held};
use diacraft_engine::Session;
use diacraft_engine::sheetview::{self, MAX_LINES_PER_READ};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// A session whose document is `doc`.
fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// A session with one page and one shape, 10, on it.
fn one_shape(shape: ShapeBuilder) -> Session {
    session_of(DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build())
}

/// The words of a refusal.
fn refused(s: &mut Session, id: &str, params: Value) -> String {
    s.execute(id, &params).unwrap_err().to_string()
}

/// The labels of the sections of an answer, in order.
fn labels(answer: &Value) -> Vec<String> {
    answer["sections"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_owned()).collect()
}

/// The section of an answer with this `section` name.
fn section<'a>(answer: &'a Value, name: &str) -> &'a Value {
    answer["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["section"] == name)
        .unwrap_or_else(|| panic!("no section {name} in {:?}", labels(answer)))
}

/// Every line of a sheet, in reads of the most lines a read gives.
fn all_lines(s: &mut Session, params: Value) -> (Value, Vec<Value>) {
    let mut p = params.as_object().unwrap().clone();
    p.insert("count".into(), json!(MAX_LINES_PER_READ));
    let (mut from, mut lines, mut first) = (0_u64, Vec::new(), None);
    loop {
        p.insert("from".into(), json!(from));
        let answer = run(s, "sheet.read", Value::Object(p.clone()));
        let got = answer["lines"].as_array().unwrap().clone();
        from += got.len() as u64;
        lines.extend(got);
        let more = answer["more"] == true;
        first.get_or_insert(answer);
        if !more {
            return (first.unwrap(), lines);
        }
    }
}

/// The first cell with this name in some lines.
fn cell_in<'a>(lines: &'a [Value], name: &str) -> &'a Value {
    lines.iter().flat_map(|l| l["cells"].as_array().unwrap()).find(|c| c["name"] == name).unwrap_or_else(|| panic!("no cell {name}"))
}

/// The names of the cells of the lines of one section, in order.
fn names_in(lines: &[Value], section: &str) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l["section"] == section)
        .flat_map(|l| l["cells"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_owned()))
        .collect()
}

fn id_of(answer: &Value) -> u32 {
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

// ---- a shape's sheet ----------------------------------------------------------------------------------------------------------

/// Of the inventory: a dropped Process shape's sheet lists Shape Transform first, with the cells in the order the window lays them out, `PinX` its own
/// and `Width` its master's, until it is resized.
#[test]
fn a_dropped_process_shapes_sheet_lists_shape_transform_first_with_pinx_local_and_width_inherited_until_it_is_resized() {
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 5.0, "y": 4.0})));
    let before = snapshot(&s);
    let (first, lines) = all_lines(&mut s, json!({"id": id}));
    assert_eq!(snapshot(&s), before, "a read changes nothing, and is not journaled");
    assert_eq!(first["sheet"], json!({"kind": "shape", "page": 1, "id": id}));
    assert_eq!(labels(&first)[0], "Shape Transform", "{:?}", labels(&first));
    assert!(!labels(&first).contains(&"1-D Endpoints".to_owned()), "a box has no end points");
    assert_eq!(
        names_in(&lines, "Shape Transform"),
        ["Width", "Height", "Angle", "PinX", "PinY", "LocPinX", "LocPinY", "FlipX", "FlipY", "ResizeMode"],
        "Width, Height and Angle, then the pin, then the flips"
    );
    assert!(first["name"].as_str().unwrap().starts_with("Process"), "{}", first["name"]);
    let pin = cell_in(&lines, "PinX");
    assert_eq!((pin["origin"].as_str(), pin["from"].clone()), (Some("local"), Value::Null));
    let width = cell_in(&lines, "Width");
    assert_eq!(width["origin"], "inherited");
    assert_eq!(width["from"]["kind"], "master");
    assert_eq!(width["from"]["name"], "Process");
    assert_eq!(width["ref"], "Width");
    assert!(width["value"].is_number() && width["unit"] == "IN", "{width}");

    run(&mut s, "shape.resize", json!({"id": id, "x": 3.0, "y": 3.0, "width": 3.5, "height": 1.0}));
    let (_, lines) = all_lines(&mut s, json!({"id": id}));
    let width = cell_in(&lines, "Width");
    assert_eq!((width["origin"].as_str(), width["from"].clone()), (Some("local"), Value::Null));
    assert_eq!(width["value"], json!(3.5));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connectors_sheet_has_one_d_endpoints_first_and_a_boxs_has_not() {
    let mut s = blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 6.0}, "to": {"x": 5.0, "y": 6.0}})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 2.0})));
    let line = run(&mut s, "sheet.read", json!({"id": c}));
    assert_eq!(&labels(&line)[..2], ["1-D Endpoints", "Shape Transform"], "1-D Endpoints above Shape Transform");
    let (_, lines) = all_lines(&mut s, json!({"id": c}));
    assert_eq!(names_in(&lines, "1-D Endpoints"), ["BeginX", "BeginY", "EndX", "EndY"]);
    let boxed = run(&mut s, "sheet.read", json!({"id": b}));
    assert!(!labels(&boxed).contains(&"1-D Endpoints".to_owned()));
    // The shape is found on the page it is on, named or not.
    assert_eq!(run(&mut s, "sheet.read", json!({"id": b, "page": 1}))["sheet"], json!({"kind": "shape", "page": 1, "id": b}));
}

/// V1 of the window's spec: the order of a one-dimensional shape's sections. Group Properties on a group and not on a plain rectangle.
#[test]
fn the_sections_of_a_shape_come_in_the_windows_order_and_group_properties_only_on_a_group() {
    let connector = ShapeBuilder::new(10)
        .cell("BeginX", "1")
        .cell("BeginY", "1")
        .cell("EndX", "3")
        .cell("EndY", "2")
        .cell("PinX", "2")
        .row_cell("Control", None, RowKey::name("Row_1"), "X", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .row_cell("User", None, RowKey::name("Total"), "Value", "5");
    let mut s = one_shape(connector);
    let answer = run(&mut s, "sheet.read", json!({"id": 10}));
    let order: Vec<String> = labels(&answer);
    let wanted = [
        "1-D Endpoints",
        "Shape Transform",
        "User-defined Cells",
        "Controls",
        "Geometry 1",
        "Protection",
        "Miscellaneous",
        "Line Format",
        "Fill Format",
    ];
    let at = |name: &str| order.iter().position(|l| l == name).unwrap_or_else(|| panic!("no {name} in {order:?}"));
    for pair in wanted.windows(2) {
        assert!(at(pair[0]) < at(pair[1]), "{} comes before {}: {order:?}", pair[0], pair[1]);
    }
    assert!(!order.contains(&"Group Properties".to_owned()), "{order:?}");

    // A group, with a member and the cells of the sections a group holds.
    let group = ShapeBuilder::new(10).group().child(ShapeBuilder::new(11)).row_cell("User", None, RowKey::name("Total"), "Value", "5").row_cell(
        "Actions",
        None,
        RowKey::name("Row_1"),
        "Menu",
        "Go",
    );
    let mut s = one_shape(group);
    let order = labels(&run(&mut s, "sheet.read", json!({"id": 10})));
    let wanted =
        ["Shape Transform", "User-defined Cells", "Actions", "Protection", "Miscellaneous", "Group Properties", "Line Format", "Fill Format"];
    let at = |name: &str| order.iter().position(|l| l == name).unwrap_or_else(|| panic!("no {name} in {order:?}"));
    for pair in wanted.windows(2) {
        assert!(at(pair[0]) < at(pair[1]), "{} comes before {}: {order:?}", pair[0], pair[1]);
    }
    // `all` shows what the rules leave out: a plain shape gets Group Properties.
    let mut s = one_shape(ShapeBuilder::new(10));
    assert!(!labels(&run(&mut s, "sheet.read", json!({"id": 10}))).contains(&"Group Properties".to_owned()));
    let all = labels(&run(&mut s, "sheet.read", json!({"id": 10, "all": true})));
    for left_out in ["1-D Endpoints", "Group Properties", "Alignment", "Text Transform", "Layer Membership", "Foreign Image Info"] {
        assert!(all.contains(&left_out.to_owned()), "all shows {left_out}: {all:?}");
    }
}

/// Alignment, Text Transform and Layer Membership show only when the shape holds one of their cells, Foreign Image Info only on a foreign object, and Glue
/// Info and Image Properties on every shape.
#[test]
fn alignment_text_transform_layer_membership_and_foreign_image_info_show_only_where_the_rules_say() {
    let mut s = one_shape(ShapeBuilder::new(10));
    let plain = labels(&run(&mut s, "sheet.read", json!({"id": 10})));
    for shown in ["Glue Info", "Image Properties", "Text Block Format", "Events", "Shape Layout"] {
        assert!(plain.contains(&shown.to_owned()), "{shown} shows on every shape: {plain:?}");
    }
    for hidden in ["Alignment", "Text Transform", "Layer Membership", "Foreign Image Info", "Group Properties", "1-D Endpoints"] {
        assert!(!plain.contains(&hidden.to_owned()), "{hidden} is hidden on a plain shape: {plain:?}");
    }
    // Held cells bring them in; Text Transform's own seven cells are listed, the held one and the others at their defaults.
    let held = ShapeBuilder::new(10).cell("AlignLeft", "1").cell("TxtWidth", "2").cell("LayerMember", "0");
    let mut s = one_shape(held);
    let shown = labels(&run(&mut s, "sheet.read", json!({"id": 10})));
    for name in ["Alignment", "Text Transform", "Layer Membership"] {
        assert!(shown.contains(&name.to_owned()), "{name}: {shown:?}");
    }
    let (_, lines) = all_lines(&mut s, json!({"id": 10, "sections": ["Text Transform"]}));
    assert_eq!(names_in(&lines, "Text Transform"), ["TxtAngle", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtPinX", "TxtPinY", "TxtWidth"]);
    assert_eq!(cell_in(&lines, "TxtWidth")["origin"], "local");
    assert_eq!(cell_in(&lines, "TxtHeight")["origin"], "default");
    // A foreign object shows its image info.
    let mut s = one_shape(ShapeBuilder::new(10).kind(ShapeType::Foreign));
    assert!(labels(&run(&mut s, "sheet.read", json!({"id": 10}))).contains(&"Foreign Image Info".to_owned()));
    // And a cell inherited from the master counts as the shape's.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("AlignTop", "1"));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1))).build();
    let mut s = session_of(doc);
    assert!(labels(&run(&mut s, "sheet.read", json!({"id": 10}))).contains(&"Alignment".to_owned()));
}

// ---- the other sheets -----------------------------------------------------------------------------------------------------------

/// V3 of the window's spec: a page's groups, then its sections of rows, then Layers, then Print Properties; the document's sheet has Document Properties.
#[test]
fn a_pages_sheet_has_page_properties_first_and_the_documents_has_document_properties() {
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .row_cell("User", None, RowKey::name("Origin"), "Value", "0")
        .row_cell("Layer", None, RowKey::Ix(0), "Name", "Connector")
        .row_cell("Layer", None, RowKey::Ix(0), "Visible", "1");
    let doc = DocBuilder::new().doc_cell("DocLangID", "1033").page(1, page.shape(ShapeBuilder::new(10))).build();
    let mut s = session_of(doc);
    let answer = run(&mut s, "sheet.read", json!({"page": 1}));
    assert_eq!(answer["sheet"], json!({"kind": "page", "page": 1}));
    assert_eq!(answer["name"], "Page-1");
    let order = labels(&answer);
    let wanted = ["Page Properties", "Page Layout", "Ruler & Grid", "User-defined Cells", "Layers", "Print Properties"];
    let at = |name: &str| order.iter().position(|l| l == name).unwrap_or_else(|| panic!("no {name} in {order:?}"));
    assert_eq!(order[0], "Page Properties");
    for pair in wanted.windows(2) {
        assert!(at(pair[0]) < at(pair[1]), "{} comes before {}: {order:?}", pair[0], pair[1]);
    }
    assert!(!order.contains(&"Shape Transform".to_owned()), "a page is no shape: {order:?}");
    assert_eq!(section(&answer, "Layers")["rows"], 1);

    let doc_sheet = run(&mut s, "sheet.read", json!({"document": true}));
    assert_eq!((doc_sheet["sheet"].clone(), doc_sheet["name"].clone()), (json!({"kind": "document"}), json!("Document")));
    assert_eq!(labels(&doc_sheet)[0], "Document Properties");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_styles_and_a_masters_sheets_are_read_and_named() {
    let master = MasterBuilder::new().name("Box").shape(ShapeBuilder::new(5).named("Inner", "Inner").cell("Width", "2"));
    let doc = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Heading").cell("LineWeight", "0.01").cell("Size", "0.2"))
        .master(2, master)
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10)))
        .build();
    let mut s = session_of(doc);
    let style = run(&mut s, "sheet.read", json!({"style": 3}));
    assert_eq!(style["sheet"], json!({"kind": "style", "style": 3}));
    assert_eq!(style["name"], "Heading (style)");
    let order = labels(&style);
    assert_eq!(order[0], "Style Properties", "R5, a project rule: first");
    assert!(order.contains(&"Group Properties".to_owned()) && order.contains(&"Line Format".to_owned()), "{order:?}");
    assert!(!order.contains(&"Shape Transform".to_owned()), "a style holds no pose: {order:?}");
    let master_page = run(&mut s, "sheet.read", json!({"master": 2}));
    assert_eq!(master_page["sheet"], json!({"kind": "master", "master": 2}));
    assert_eq!(master_page["name"], "Box (master)");
    assert_eq!(labels(&master_page)[0], "Page Properties");
    let inner = run(&mut s, "sheet.read", json!({"master": 2, "id": 5}));
    assert_eq!(inner["sheet"], json!({"kind": "shape", "master": 2, "id": 5}));
    assert_eq!(inner["name"], "Inner, Box (master)");
    assert_eq!(labels(&inner)[0], "Shape Transform");
}

// ---- naming a sheet ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_bad_mix_of_sheet_params_is_refused_in_the_words_of_the_design_and_changes_nothing() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5));
    let doc = DocBuilder::new().style(3, StyleBuilder::new()).master(2, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    let mut s = session_of(doc);
    let before = snapshot(&s);
    let one = "give one sheet: id (with page), page, master (with id), style or document";
    for params in [
        json!({}),
        json!({"document": false}),
        json!({"id": 10, "style": 3}),
        json!({"page": 1, "style": 3}),
        json!({"document": true, "id": 10}),
        json!({"document": true, "page": 1}),
        json!({"master": 2, "page": 1}),
        json!({"master": 2, "style": 3}),
        json!({"master": 2, "document": true}),
    ] {
        for command in ["sheet.read", "sheet.sections"] {
            let e = refused(&mut s, command, params.clone());
            assert!(e.contains(one), "{command} {params}: {e}");
        }
    }
    for (params, words) in [
        (json!({"id": 10, "page": 7}), "the drawing has no page 7"),
        (json!({"id": 99}), "no page has a shape 99"),
        (json!({"id": 99, "page": 1}), "page 1 has no shape 99"),
        (json!({"page": 7}), "the drawing has no page 7"),
        (json!({"master": 7}), "the drawing has no master 7"),
        (json!({"master": 2, "id": 99}), "master 2 has no shape 99"),
        (json!({"style": 7}), "the drawing has no style 7"),
        (json!({"id": "ten"}), "`id` must be a whole number"),
        (json!({"document": "yes"}), "`document` must be true or false"),
        (json!({"id": -1}), "`id` must be a whole number"),
    ] {
        let e = refused(&mut s, "sheet.read", params.clone());
        assert!(e.contains(words), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_shape_is_found_on_the_first_page_that_holds_it_and_a_groups_member_too() {
    let group = ShapeBuilder::new(20).group().child(ShapeBuilder::new(21).cell("Width", "1.25"));
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("First").shape(ShapeBuilder::new(10)))
        .page(2, PageBuilder::new().name("Second").shape(group).shape(ShapeBuilder::new(10)))
        .build();
    let mut s = session_of(doc);
    assert_eq!(run(&mut s, "sheet.read", json!({"id": 10}))["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    assert_eq!(run(&mut s, "sheet.read", json!({"id": 10, "page": 2}))["sheet"], json!({"kind": "shape", "page": 2, "id": 10}));
    let member = run(&mut s, "sheet.read", json!({"id": 21}));
    assert_eq!(member["sheet"], json!({"kind": "shape", "page": 2, "id": 21}));
    assert_eq!(member["name"], "Sheet.21, Second");
    let (_, lines) = all_lines(&mut s, json!({"id": 21}));
    assert_eq!(cell_in(&lines, "Width")["value"], json!(1.25));
}

// ---- the lines and their budgets --------------------------------------------------------------------------------------------------

/// A shape with a `Geometry` section of `rows` rows (a MoveTo and LineTo rows with an `X` and a `Y` each).
fn geometry_shape(id: u32, rows: u32) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id).cell("Width", "2").cell("Height", "1");
    for k in 1..=rows {
        let t = if k == 1 { "MoveTo" } else { "LineTo" };
        shape = shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), t)
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "X", &(k % 3).to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "Y", &(k % 2).to_string());
    }
    shape
}

/// The most rows the sample files hold in one section, 31,273 (a `Geometry` of `RelLineTo` rows): the outline visits each row once and resolves no cell, and `from` and `count`
/// page through them.
#[test]
fn a_geometry_of_31273_rows_is_outlined_by_a_visit_of_each_row_with_no_cell_resolved_and_read_in_pages() {
    const ROWS: u32 = 31_273;
    let mut s = one_shape(geometry_shape(10, ROWS));
    let doc = s.document().unwrap().clone();
    let at = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    diacraft_doc::work::reset();
    let res = Resolver::new(&doc);
    let outline = sheetview::Outline::build(&res, &at, &sheetview::Options::default()).unwrap();
    assert_eq!(outline.counts.cells, 0, "no cell is resolved to list the lines");
    assert_eq!(outline.counts.rows, u64::from(ROWS), "each row is visited once");
    assert_eq!(diacraft_doc::work::rows_merged(), u64::from(ROWS), "and the model merged them once");
    assert!(
        outline.counts.probes <= 40,
        "the checks that decide which sections show are few and do not grow with the rows: {}",
        outline.counts.probes
    );
    let geometry = outline.sections.iter().find(|s| s.name == "Geometry1").unwrap();
    assert_eq!(geometry.rows.len(), ROWS as usize);
    // The header, the five flags, the titles and a line a row.
    assert_eq!(geometry.lines, 1 + 5 + 1 + u64::from(ROWS));
    assert_eq!(outline.total_lines, outline.sections.iter().map(|s| s.lines).sum::<u64>());

    // Paging with `from` and `count`: the pages tile the sheet, and a window of lines resolves a few cells.
    let answer = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Geometry1"], "from": 0, "count": 10}));
    assert_eq!(answer["lines_total"], json!(geometry.lines));
    assert_eq!(answer["more"], true);
    let kinds: Vec<&str> = answer["lines"].as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["header", "cell", "cell", "cell", "cell", "cell", "titles", "row", "row", "row"]);
    let geo = section(&answer, "Geometry1");
    assert_eq!(
        (geo["label"].clone(), geo["rows"].clone(), geo["ix"].clone(), geo["kind"].clone()),
        (json!("Geometry 1"), json!(ROWS), json!(0), json!("rows"))
    );
    assert_eq!(geo["columns"], json!(["X", "Y", "A", "B", "C", "D", "E", "Path"]));
    assert_eq!(answer["lines"][1]["cells"][0]["ref"], "Geometry1.NoFill");
    let tail = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Geometry1"], "from": geometry.lines - 3, "count": 2000}));
    assert_eq!((tail["lines"].as_array().unwrap().len(), tail["more"].clone()), (3, json!(false)));
    let last = &tail["lines"][2];
    assert_eq!((last["kind"].clone(), last["row"]["key"].clone(), last["row"]["type"].clone()), (json!("row"), json!(ROWS), json!("LineTo")));
    assert_eq!(last["cells"][0]["ref"], format!("Geometry1.X{ROWS}"));
    let past = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Geometry1"], "from": geometry.lines + 5}));
    assert_eq!((past["lines"].as_array().unwrap().len(), past["more"].clone()), (0, json!(false)));

    // A window of 50 lines resolves at most 50 times the columns (8) of cells: counted, not timed.
    let (lines, counts) = sheetview::read_lines(&res, &at, &outline, geometry.first_line + 20_000, 50);
    assert_eq!(lines.lines.len(), 50);
    assert!(counts.cells <= 50 * 8 + 5, "{}", counts.cells);
    assert!(counts.cells >= 50, "{}", counts.cells);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn count_is_at_least_one_and_at_most_2000_and_a_read_gives_at_most_that_many_lines() {
    let mut s = one_shape(geometry_shape(10, 3000));
    for count in [json!(0), json!(2001), json!(-1), json!(1.5), json!("many")] {
        let e = refused(&mut s, "sheet.read", json!({"id": 10, "count": count}));
        assert!(e.contains("`count` must be a whole number from 1 to 2000"), "{count}: {e}");
    }
    let most = run(&mut s, "sheet.read", json!({"id": 10, "count": 2000, "sections": ["Geometry1"]}));
    assert_eq!(most["lines"].as_array().unwrap().len(), 2000);
    assert_eq!(most["more"], true);
    let one = run(&mut s, "sheet.read", json!({"id": 10, "count": 1}));
    assert_eq!(one["lines"].as_array().unwrap().len(), 1);
    // The default is 200.
    let default = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Geometry1"]}));
    assert_eq!(default["lines"].as_array().unwrap().len(), 200);
    assert_eq!(MAX_LINES_PER_READ, 2000);
    // `from` is a whole number of lines.
    for from in [json!(-1), json!(1.5), json!("x"), json!(u64::MAX)] {
        let e = refused(&mut s, "sheet.read", json!({"id": 10, "from": from}));
        assert!(e.contains("`from` must be a whole number"), "{from}: {e}");
    }
}

#[test]
fn a_formula_of_5000_characters_is_cut_in_a_read_and_whole_from_sheet_cell() {
    let long = format!("1{}", "+1".repeat(2_500));
    assert_eq!(long.chars().count(), 5_001);
    let shape = ShapeBuilder::new(10)
        .formula("PinX", &long, "2501")
        .set(CellAddr::in_row("User", None, RowKey::name("Note"), "Value"), Cell::constant(Held::from_text(&"é".repeat(5_000))));
    let mut s = one_shape(shape);
    let (_, lines) = all_lines(&mut s, json!({"id": 10}));
    let pin = cell_in(&lines, "PinX");
    assert_eq!(pin["truncated"], true);
    assert_eq!(pin["formula"].as_str().unwrap().chars().count(), 4_096);
    assert!(long.starts_with(pin["formula"].as_str().unwrap()));
    let note = cell_in(&lines, "Value");
    assert_eq!(note["truncated"], true, "a text value is cut too");
    assert_eq!(note["value"].as_str().unwrap().chars().count(), 4_096);
    let whole = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "PinX"}));
    assert_eq!(whole["formula"], json!(long));
    assert_eq!(whole["truncated"], false);
    let whole_note = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "User.Note"}));
    assert_eq!(whole_note["value"].as_str().unwrap().chars().count(), 5_000);
}

/// A read stops at 8 MiB of text with `more`, and always gives its first line. A text of 5,000 characters is held three times in a cell (the formula that is its literal, the
/// value and the text shown), each cut at 4,096: 1,500 such rows are about 18 MB, and the answer ends where the text passes 8 MiB.
#[test]
fn an_answer_stops_at_eight_mebibytes_of_text_and_says_there_is_more() {
    let text = "x".repeat(5_000);
    let mut shape = ShapeBuilder::new(10);
    for k in 0..1_500 {
        shape = shape.set(CellAddr::in_row("User", None, RowKey::name(&format!("Row_{k}")), "Value"), Cell::constant(Held::from_text(&text)));
    }
    let mut s = one_shape(shape);
    let answer = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["User"], "count": 2000}));
    let given = answer["lines"].as_array().unwrap().len();
    // A header, the titles and 1,500 rows are 1,502 lines: the answer is cut before the last, and says so.
    assert_eq!(answer["lines_total"], 1_502);
    assert!(given > 100 && given < 1_502, "{given}");
    assert_eq!(answer["more"], true);
    let bytes = serde_json::to_string(&answer).unwrap().len();
    assert!((8 * 1024 * 1024..10 * 1024 * 1024).contains(&bytes), "the answer is cut at 8 MiB of text, past it by one line: {bytes}");
    // The rest is read from where the answer stopped, and every row is read once.
    let rest = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["User"], "count": 2000, "from": given}));
    assert_eq!(rest["lines"][0]["line"], json!(given));
    assert_eq!(sheetview::MAX_ANSWER_BYTES, 8 * 1024 * 1024);
}

/// An outline lists at most the lines it is given (design 6: 1,000,000 by default, here 400): a section past it lists the rows that fit and a note, and a section after it
/// lists none, says how many it holds and is not merged.
#[test]
fn an_outline_past_its_budget_lists_the_rows_that_fit_with_a_note_and_leaves_later_sections_unmerged() {
    let mut shape = geometry_shape(10, 500);
    for k in 1..=300_u32 {
        shape = shape.row_cell("Geometry", Some(1), RowKey::Ix(k), "X", "1");
    }
    let s = one_shape(shape);
    let doc = s.document().unwrap().clone();
    let at = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let res = Resolver::new(&doc);
    diacraft_doc::work::reset();
    let both = Some(vec!["Geometry1".to_owned(), "Geometry2".to_owned()]);
    let options = sheetview::Options { only: both, max_lines: 400, ..sheetview::Options::default() };
    let outline = sheetview::Outline::build(&res, &at, &options).unwrap();
    let first = outline.section("Geometry1").unwrap();
    // 7 lines of header, flags and titles, 392 rows and a note: 400 lines.
    assert_eq!((first.rows.len(), first.unlisted, first.rows_total, first.lines), (392, 108, 500, 400));
    let second = outline.section("Geometry2").unwrap();
    assert_eq!((second.rows.len(), second.unlisted, second.rows_total, second.lines), (0, 300, 300, 8));
    // Only the first was merged: 500 rows were visited, not 800.
    assert_eq!(outline.counts.rows, 500);
    assert_eq!(diacraft_doc::work::rows_merged(), 500);
    // The note and the rows after it are lines: the 400th line of the sheet is the note, and a read of the second section by itself lists its rows.
    let (lines, _) = sheetview::read_lines(&res, &at, &outline, first.first_line + 399, 2);
    assert_eq!(lines.lines[0].kind, sheetview::LineKind::Note);
    assert!(lines.lines[0].note.as_ref().unwrap().starts_with("108 more rows are not listed here"));
    let alone = sheetview::Options { only: Some(vec!["Geometry2".to_owned()]), max_lines: 400, ..sheetview::Options::default() };
    let outline = sheetview::Outline::build(&res, &at, &alone).unwrap();
    assert_eq!((outline.sections.len(), outline.sections[0].rows.len(), outline.sections[0].unlisted), (1, 300, 0));
}

/// A row's name in a reference matches without regard to letter case, which `cell_address` cannot know; the command spells the row as the sheet does.
#[test]
fn a_row_named_in_another_letter_case_is_the_row_the_sheet_spells() {
    let shape = ShapeBuilder::new(10).row_cell("Property", None, RowKey::name("Cost"), "Value", "9").row_cell(
        "Property",
        None,
        RowKey::name("Cost"),
        "Label",
        "Price",
    );
    let mut s = one_shape(shape);
    for text in ["Prop.COST", "Prop.cost", "Prop.Cost"] {
        let c = run(&mut s, "sheet.cell", json!({"id": 10, "cell": text}));
        assert_eq!((c["ref"].clone(), c["origin"].clone(), c["value"].clone()), (json!("Prop.Cost.Value"), json!("local"), json!(9.0)), "{text}");
    }
    let label = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "PROP.x"}));
    assert_eq!(label["origin"], "default", "a section's spelling is exact, and a row no case matches is a cell that is not there");
    let label = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Prop.COST.Label"}));
    assert_eq!((label["ref"].clone(), label["value"].clone()), (json!("Prop.Cost.Label"), json!("Price")));
    // The dry run is for the row the sheet has: a text that names the cell it is for is a loop, however the row's name is spelled in either.
    let looped = run(&mut s, "sheet.evaluate", json!({"id": 10, "formula": "Prop.COST", "cell": "Prop.cost"}));
    assert!(looped["error"].is_string(), "{looped}");
    let free = run(&mut s, "sheet.evaluate", json!({"id": 10, "formula": "Prop.COST"}));
    assert_eq!((free["error"].clone(), free["value"].clone()), (Value::Null, json!(9.0)));
}

#[test]
fn a_section_filter_reads_only_the_sections_named_by_either_name_and_refuses_the_others_in_words() {
    let mut s = one_shape(ShapeBuilder::new(10).row_cell("User", None, RowKey::name("Total"), "Value", "5").row_cell(
        "Property",
        None,
        RowKey::name("Cost"),
        "Value",
        "9",
    ));
    let answer = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Prop", "shape transform"]}));
    assert_eq!(
        labels(&answer),
        ["Shape Transform", "Shape Data"],
        "in the sheet's order, matched in any letter case, by the reference's spelling or the label"
    );
    let rows = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Shape Data"]}));
    let row = &rows["lines"][2];
    assert_eq!(row["kind"], "row");
    assert_eq!(row["row"], json!({"key": "Cost", "label": "Prop.Cost", "type": null, "origin": "local"}));
    assert_eq!(row["cells"][0]["ref"], "Prop.Cost.Value");
    let e = refused(&mut s, "sheet.read", json!({"id": 10, "sections": ["Nope"]}));
    assert!(e.contains("this sheet shows no section Nope"), "{e}");
    let e = refused(&mut s, "sheet.read", json!({"id": 10, "sections": ["Group Properties"]}));
    assert!(e.contains("Group Properties") && e.contains("all: true"), "a section the rules leave out is named, with the way to see it: {e}");
    let e = refused(&mut s, "sheet.read", json!({"id": 10, "sections": "User"}));
    assert!(e.contains("`sections` must be a list of section names"), "{e}");
    let shown = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Group Properties"], "all": true}));
    assert_eq!(labels(&shown), ["Group Properties"]);
}

// ---- a cell -------------------------------------------------------------------------------------------------------------------------

fn error_cell(formula: &str, value: &str, error: &str) -> Cell {
    let mut cell = Cell::with_formula(formula, Some(Held::from_text(value)));
    cell.error = Some(error.into());
    cell
}

/// What a cell reads as: the origin, the formula as the file has it, the value in its unit, the text the Values view shows and the formula's literal form.
#[test]
fn a_cell_is_read_with_its_formula_its_value_in_its_unit_and_the_texts_of_the_two_views() {
    let mm = diacraft_units::Unit::Millimetre;
    let shape = ShapeBuilder::new(10)
        .cell_u("Width", &mm.to_internal(50.0).to_string(), "MM")
        .cell_u("Height", &mm.to_internal(0.666666).to_string(), "MM")
        .cell_u("LocPinX", &mm.to_internal(2.5).to_string(), "MM")
        .cell("FillPattern", "1")
        .cell_u("Angle", &diacraft_units::Unit::Degree.to_internal(45.0).to_string(), "DEG")
        .cell("PinX", "1.5")
        .formula("PinY", "Width*0.5", "0.984252")
        .no_formula("LocPinY", "3")
        .cell("Comment", "Hello")
        .cell_u("LockWidth", "1", "BOOL")
        .set(CellAddr::direct("EndX"), error_cell("1/0", "0", "#DIV/0!"))
        .set(CellAddr::direct("EndY"), error_cell("EndX+1", "0", "#DIV/0!"))
        .formula("BeginX", "GUARD(2)", "2")
        .formula("BeginY", "FOOBAR(1)", "7");
    let mut s = one_shape(shape);
    let (_, lines) = all_lines(&mut s, json!({"id": 10, "all": true}));
    let c = |name: &str| cell_in(&lines, name).clone();

    let width = c("Width");
    assert_eq!((width["unit"].clone(), width["origin"].clone()), (json!("MM"), json!("local")));
    assert!((width["value"].as_f64().unwrap() - 50.0).abs() < 1e-9, "{width}");
    assert_eq!((width["shown"].as_str(), width["formula"].as_str()), (Some("50.0000 mm"), Some("50 mm")), "");
    // 0.666666 mm reads 0.6667 mm in both views; 2.5 mm reads 2.5000 mm in the Values view and 2.5 mm in the Formulas view.
    assert_eq!((c("Height")["shown"].as_str(), c("Height")["formula"].as_str()), (Some("0.6667 mm"), Some("0.6667 mm")));
    assert_eq!((c("LocPinX")["shown"].as_str(), c("LocPinX")["formula"].as_str()), (Some("2.5000 mm"), Some("2.5 mm")));
    assert_eq!((c("FillPattern")["shown"].as_str(), c("FillPattern")["formula"].as_str()), (Some("1"), Some("1")), "an index shows a whole number");
    assert_eq!((c("Angle")["shown"].as_str(), c("Angle")["unit"].as_str()), (Some("45.0000 deg"), Some("DEG")));
    // A length cell with no unit is in inches, the implicit unit of its kind.
    assert_eq!((c("PinX")["shown"].as_str(), c("PinX")["unit"].as_str()), (Some("1.5000 in"), Some("IN")));
    assert_eq!(c("LockWidth")["value"], json!(true));
    assert_eq!(c("LockWidth")["shown"], "TRUE");
    assert_eq!(
        (c("Comment")["value"].as_str(), c("Comment")["shown"].as_str(), c("Comment")["formula"].as_str()),
        (Some("Hello"), Some("Hello"), Some("\"Hello\""))
    );

    let formula = c("PinY");
    assert_eq!(
        (formula["formula"].as_str(), formula["origin"].as_str(), formula["parsed"].clone(), formula["guarded"].clone()),
        (Some("Width*0.5"), Some("local"), json!(true), json!(false))
    );
    assert_eq!(c("LocPinY")["formula"], "No Formula", "");
    assert_eq!(c("BeginX")["guarded"], true);
    // A formula that divides by zero reads #DIV/0!, and a cell that reads it an error too.
    for name in ["EndX", "EndY"] {
        let e = c(name);
        assert_eq!((e["error"].as_str(), e["shown"].as_str()), (Some("#DIV/0!"), Some("#DIV/0!")), "{name}");
    }
    // A function the evaluator does not have: the saved value stands, and the cell says it was not worked out.
    let unknown = c("BeginY");
    assert_eq!((unknown["worked_out"].clone(), unknown["parsed"].clone(), unknown["value"].clone()), (json!(false), json!(true), json!(7.0)));
    assert_eq!(c("PinY")["worked_out"], true);
}

#[test]
fn a_formula_that_does_not_parse_is_shown_as_the_file_has_it_with_the_words_not_read() {
    let mut broken = Cell::new();
    broken.formula = FormulaSlot::read("1 +* 2", &mut 1_000);
    broken.value = Some(Held::from_text("3"));
    let mut s = one_shape(ShapeBuilder::new(10).set(CellAddr::direct("PinX"), broken));
    let (_, lines) = all_lines(&mut s, json!({"id": 10}));
    let pin = cell_in(&lines, "PinX");
    assert_eq!(
        (pin["formula"].as_str(), pin["parsed"].clone(), pin["worked_out"].clone(), pin["shown"].as_str()),
        (Some("1 +* 2"), json!(false), json!(false), Some("not read"))
    );
    assert_eq!(pin["value"], json!(3.0), "the saved value is kept");
}

#[test]
fn sheet_cell_reads_one_cell_by_its_reference_with_its_kind_class_default_and_the_sheets_it_is_looked_for_on() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2"));
    let doc = DocBuilder::new()
        .root(|r| r.cell("DisplayMode", "1"))
        .master(1, master)
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("PinX", "4").row_cell(
                "Property",
                None,
                RowKey::name("Cost"),
                "Value",
                "9",
            )),
        )
        .build();
    let mut s = session_of(doc);
    let before = snapshot(&s);
    let width = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Width"}));
    assert_eq!(
        (width["ref"].clone(), width["origin"].clone(), width["kind"].clone(), width["class"].clone()),
        (json!("Width"), json!("inherited"), json!("length"), json!("shape-only"))
    );
    assert_eq!(width["listed"], true);
    assert_eq!(width["default"]["value"], json!(0.0));
    let chain = width["chain"].as_array().unwrap();
    assert_eq!(
        chain.iter().map(|c| c["source"].as_str().unwrap()).collect::<Vec<_>>(),
        ["local", "master"],
        "a shape-only cell is looked for on the shape and its master"
    );
    assert_eq!(chain.iter().map(|c| c["holds"].clone()).collect::<Vec<_>>(), [json!(false), json!(true)]);
    let weight = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "DisplayMode"}));
    assert_eq!(weight["origin"], "inherited");
    assert_eq!(weight["from"], json!({"kind": "style", "id": 0, "name": "No Style"}));
    let cost = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Prop.Cost"}));
    assert_eq!(
        (cost["ref"].clone(), cost["name"].clone(), cost["origin"].clone()),
        (json!("Prop.Cost.Value"), json!("Value"), json!("local")),
        "A named row with no cell reads its Value"
    );
    // A cell nowhere reads its default.
    let nowhere = run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Geometry1.NoFill"}));
    assert_eq!((nowhere["origin"].clone(), nowhere["from"].clone()), (json!("default"), Value::Null));
    assert_eq!(snapshot(&s), before);
    // The words of a cell that is not named well.
    for (cell, words) in [
        ("Sheet.4!Width", "name the sheet with id, page, master, style or document, not in the reference"),
        ("=Width", "without a leading ="),
        ("1+1", "not one cell reference"),
        ("", "name the cell by its reference"),
    ] {
        let e = refused(&mut s, "sheet.cell", json!({"id": 10, "cell": cell}));
        assert!(e.contains(words), "{cell:?}: {e}");
    }
    assert!(refused(&mut s, "sheet.cell", json!({"id": 10})).contains("missing `cell`"));
}

// ---- sections --------------------------------------------------------------------------------------------------------------------

/// V6 of the inventory: Shape Transform cannot be deleted; what Insert Section offers is what the rules of allow, less what the shape has.
#[test]
fn sheet_sections_says_what_insert_section_and_delete_section_offer() {
    let shape =
        ShapeBuilder::new(10).row_cell("User", None, RowKey::name("Total"), "Value", "1").row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0");
    let mut s = one_shape(shape);
    let before = snapshot(&s);
    let answer = run(&mut s, "sheet.sections", json!({"id": 10}));
    assert_eq!(answer["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    let present: Vec<&str> = answer["present"].as_array().unwrap().iter().map(|p| p["section"].as_str().unwrap()).collect();
    assert!(present.contains(&"Shape Transform") && present.contains(&"User") && present.contains(&"Geometry1"), "{present:?}");
    let can_add: Vec<(&str, Option<&str>)> =
        answer["can_add"].as_array().unwrap().iter().map(|p| (p["section"].as_str().unwrap(), p["variant"].as_str())).collect();
    // A second Geometry (and its Ellipse and Infinite Line kinds), and the sections the shape has not. Not User (it has one), nor Layers, Reviewer or Annotation (greyed).
    for wanted in [
        ("Geometry", None),
        ("Geometry", Some("Ellipse")),
        ("Geometry", Some("InfiniteLine")),
        ("Prop", None),
        ("Hyperlink", None),
        ("Connections", None),
        ("Actions", None),
        ("Controls", None),
        ("Scratch", None),
        ("SmartTags", None),
        ("Text Transform", None),
        ("Layer Membership", None),
    ] {
        assert!(can_add.contains(&wanted), "{wanted:?} is offered: {can_add:?}");
    }
    for not in ["User", "Layers", "Reviewer", "Annotation", "Char"] {
        assert!(!can_add.iter().any(|(name, _)| *name == not), "{not} is not offered: {can_add:?}");
    }
    let can_delete: Vec<&str> = answer["can_delete"].as_array().unwrap().iter().map(|p| p["section"].as_str().unwrap()).collect();
    assert!(can_delete.contains(&"User") && can_delete.contains(&"Geometry1"), "{can_delete:?}");
    assert!(!can_delete.contains(&"Shape Transform"), "Shape Transform cannot be deleted");
    // A page takes the sections a page can hold; the document's sheet likewise.
    let page = run(&mut s, "sheet.sections", json!({"page": 1}));
    let page_add: Vec<&str> = page["can_add"].as_array().unwrap().iter().map(|p| p["section"].as_str().unwrap()).collect();
    assert!(page_add.contains(&"User") && page_add.contains(&"Scratch"), "{page_add:?}");
    assert!(
        !page_add.contains(&"Geometry") && !page_add.contains(&"Controls") && !page_add.contains(&"Connections"),
        "geometry, controls and connection points are a shape's: {page_add:?}"
    );
    assert_eq!(snapshot(&s), before);
}

// ---- the dry run ---------------------------------------------------------------------------------------------------------------------

#[test]
fn sheet_evaluate_works_a_formula_out_on_the_sheet_and_leaves_the_revision_as_it_was() {
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 5.0, "y": 4.0})));
    let width = run(&mut s, "sheet.cell", json!({"id": id, "cell": "Width"}))["value"].as_f64().unwrap();
    let before = snapshot(&s);
    let doubled = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "Width*2"}));
    assert_eq!(doubled["value"].as_f64().unwrap(), width * 2.0);
    assert_eq!(
        (doubled["unit"].clone(), doubled["error"].clone(), doubled["parse_error"].clone(), doubled["budget_exhausted"].clone()),
        (json!("IN"), Value::Null, Value::Null, json!(false))
    );
    assert_eq!(doubled["shown"], format!("{:.4} in", width * 2.0));
    assert_eq!(snapshot(&s), before, "nothing is written, the revision and the history are as they were");
    // A leading `=` and white space are cut, as a write cuts them, and the formula is fitted to the cell it is for.
    let fitted = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "  = 3 ", "cell": "Width"}));
    assert_eq!((fitted["value"].clone(), fitted["unit"].clone()), (json!(3.0), json!("IN")));
    let plain = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "1+2"}));
    assert_eq!((plain["value"].clone(), plain["unit"].clone(), plain["shown"].clone()), (json!(3.0), Value::Null, json!("3")));
    // An error is a value.
    let div = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "1/0"}));
    assert_eq!((div["error"].clone(), div["value"].clone(), div["shown"].clone()), (json!("#DIV/0!"), Value::Null, json!("#DIV/0!")));
    let text = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "\"a\"&\"b\""}));
    assert_eq!((text["value"].clone(), text["shown"].clone()), (json!("ab"), json!("ab")));
    let flag = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "1<2"}));
    assert_eq!((flag["value"].clone(), flag["shown"].clone()), (json!(true), json!("TRUE")));
    assert_eq!(snapshot(&s), before);
}

/// The brief's note: `1+` is `unexpected_end` and not `expected_operand`, and the words cover both, with the offset in the text as typed.
#[test]
fn sheet_evaluate_reports_a_parse_error_with_its_kind_its_offset_in_the_typed_text_and_words() {
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 5.0, "y": 4.0})));
    let end = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "1+"}));
    let e = &end["parse_error"];
    assert_eq!((e["kind"].as_str(), e["offset"].as_u64()), (Some("unexpected_end"), Some(2)));
    assert!(e["words"].as_str().unwrap().contains("character 3"), "{e}");
    assert_eq!((end["value"].clone(), end["error"].clone()), (Value::Null, Value::Null));
    let operand = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "1+*2"}));
    assert_eq!(operand["parse_error"]["kind"], "expected_operand");
    assert_eq!(operand["parse_error"]["offset"], 2);
    // The offset is into the text as given, after the `=` and the blanks that the formula is cut of.
    let typed = run(&mut s, "sheet.evaluate", json!({"id": id, "formula": "  =1+"}));
    assert_eq!((typed["parse_error"]["kind"].clone(), typed["parse_error"]["offset"].clone()), (json!("unexpected_end"), json!(5)));
    // A text of 65,537 bytes is refused in words, and nothing is written.
    let long = "1".repeat(65_537);
    let e = refused(&mut s, "sheet.evaluate", json!({"id": id, "formula": long}));
    assert!(e.contains("at most 65536"), "{e}");
    assert!(refused(&mut s, "sheet.evaluate", json!({"id": id})).contains("missing `formula`"));
}

#[test]
fn sheet_evaluate_stops_at_its_budget_and_says_so_and_never_hangs_on_a_long_chain() {
    // A chain of 10,000 cells each reading the one before: the text that reads the end of it is worked out with a step budget, and the answer says when it ran out.
    let mut shape = ShapeBuilder::new(10);
    let user = |k: u32| CellAddr::in_row("User", None, RowKey::name(&format!("C{k}")), "Value");
    shape = shape.set(user(0), Cell::constant(Held::from_text("1")));
    for k in 1..10_000_u32 {
        shape = shape.set(user(k), Cell::with_formula(&format!("User.C{}+1", k - 1), Some(Held::from_text(&(k + 1).to_string()))));
    }
    let mut s = one_shape(shape);
    let before = snapshot(&s);
    let reached = run(&mut s, "sheet.evaluate", json!({"id": 10, "formula": "User.C9999"}));
    assert!(reached["budget_exhausted"] == false || reached["budget_exhausted"] == true);
    assert!(reached["steps"].as_u64().unwrap() <= sheetview::EVALUATE_STEPS, "{reached}");
    if reached["budget_exhausted"] == false {
        assert_eq!(reached["value"], json!(10_000.0));
    }
    // A text of many names costs its length in steps and stops at the budget.
    let many = vec!["Width"; 10_000].join("+");
    let wide = run(&mut s, "sheet.evaluate", json!({"id": 10, "formula": many}));
    assert!(wide["steps"].as_u64().unwrap() <= sheetview::EVALUATE_STEPS, "{wide}");
    assert_eq!(snapshot(&s), before);
}

// ---- hostile sheets ------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_sheet_of_many_sections_a_row_deleted_and_a_cell_in_every_place_is_read_without_a_panic() {
    let mut shape = ShapeBuilder::new(10);
    for k in 0..600_u32 {
        shape = shape.row_cell("Geometry", Some(k), RowKey::Ix(1), "X", "1");
    }
    shape = shape
        .del_row("Scratch", None, RowKey::Ix(0))
        .del_section("Scratch", None)
        .row_cell("User", None, RowKey::name("a"), "Value", "1")
        .del_row("User", None, RowKey::name("a"));
    let mut s = one_shape(shape);
    let (answer, lines) = all_lines(&mut s, json!({"id": 10}));
    // 600 Geometry sections, each with its header, five flags, titles and one row; a deleted section and a deleted row show nothing.
    let geometry = labels(&answer).iter().filter(|l| l.starts_with("Geometry ")).count();
    assert_eq!(geometry, 600);
    assert!(!labels(&answer).contains(&"Scratch".to_owned()) && !labels(&answer).contains(&"User-defined Cells".to_owned()), "{:?}", labels(&answer));
    assert!(lines.len() > 600 * 8);
    // A shape that is deleted (`Del`) has no sheet to read.
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(7).deleted())).build();
    let mut gone = session_of(doc);
    assert!(refused(&mut gone, "sheet.read", json!({"id": 7})).contains("no page has a shape 7"));
}
