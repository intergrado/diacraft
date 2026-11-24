//! What the Shape Data commands cost on hostile drawings. The work is **counted** (rows merged, lookups built), never timed: a shape whose master holds more rows than a call
//! reads is refused before any is merged, a list that reads the most rows merges each once, and a list of the most fields is written with a count of lookups that follows the number of cells
//! and not its square.

use diacraft_doc::{Document, RowKey, work};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value as Json, json};

use crate::{blank_session, check_session, run, snapshot};

/// A drawing of `instances` shapes that are instances of a master holding `rows` Shape Data rows.
fn instances_of_a_master_of(rows: u32, instances: u32) -> Session {
    let mut master = ShapeBuilder::new(1);
    for n in 0..rows {
        master = master.row_cell("Property", None, RowKey::name(&format!("R{n}")), "Value", "1");
    }
    let mut page = PageBuilder::new();
    for i in 0..instances {
        page = page.shape(ShapeBuilder::new(i + 1).master(1));
    }
    let doc: Document = DocBuilder::new().master(1, MasterBuilder::new().shape(master)).page(1, page).build();
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let before = snapshot(s);
    let e = s.execute(command, &params).expect_err("the call is refused");
    assert_eq!(snapshot(s), before, "{command}: a refusal changes nothing");
    e.to_string()
}

#[test]
fn a_master_with_more_rows_than_a_call_reads_is_refused_before_any_row_is_merged() {
    let mut s = instances_of_a_master_of(100_000, 8);
    for (command, params) in [
        ("shapedata.list", json!({"id": 1})),
        ("shapedata.set", json!({"id": 2, "row": "R1", "value": "x"})),
        ("shapedata.define", json!({"id": 3, "fields": []})),
    ] {
        work::reset();
        let words = refused(&mut s, command, params);
        assert!(words.contains("100000") || words.contains("100,000"), "{command}: {words}");
        assert_eq!(work::rows_merged(), 0, "{command}: the rows are counted before any is merged, so the refusal costs nothing in rows");
    }
    check_session(&s).unwrap();
}

#[test]
fn the_most_rows_a_call_reads_are_merged_once_for_the_call_and_the_list_past_512_is_refused() {
    let mut s = instances_of_a_master_of(4_096, 8);
    work::reset();
    let words = refused(&mut s, "shapedata.list", json!({"id": 1}));
    assert!(words.contains("512"), "{words}");
    assert!(work::rows_merged() <= 4_096, "one merge of the section, not one for each row: {}", work::rows_merged());
    // 512 rows are the most that are listed, and cost about as many merges as rows.
    let mut s = instances_of_a_master_of(512, 64);
    work::reset();
    let answer = run(&mut s, "shapedata.list", json!({"id": 1}));
    assert_eq!(answer["fields"].as_array().unwrap().len(), 512);
    assert!(work::rows_merged() <= 512, "{}", work::rows_merged());
}

#[test]
fn the_most_fields_are_defined_and_set_with_work_that_follows_their_cells() {
    let mut s = blank_session();
    let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}))["id"].as_u64().unwrap();
    let fields: Vec<Json> = (0..512)
        .map(|n| json!({"label": format!("Field {n}"), "type": if n % 2 == 0 { "number" } else { "string" }, "value": format!("{n}")}))
        .collect();
    work::reset();
    run(&mut s, "shapedata.define", json!({"id": id, "fields": fields}));
    let builds = work::resolver_builds();
    // About ten cells are written for each of 512 new rows: the lookups are built once for the whole batch of writes, and not for each write.
    assert!(builds <= 64, "defining 512 fields built {builds} lookups");
    work::reset();
    let values: Vec<Json> = (0..512).map(|n| json!({"row": format!("Field_{n}"), "value": format!("{}", n + 1)})).collect();
    run(&mut s, "shapedata.set", json!({"id": id, "values": values}));
    let builds = work::resolver_builds();
    assert!(builds <= 64, "setting 512 values built {builds} lookups");
    check_session(&s).unwrap();
}
