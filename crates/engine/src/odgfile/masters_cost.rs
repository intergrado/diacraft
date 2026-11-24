//! What the flattening of a drawing that holds masters costs: **a master multiplies the shapes and the cells a save copies**, since each instance is written as the shape it shows (its
//! own cells over its master's), so the cost is counted and held to the budget before the work is done. The drawings are built in code from the VSDX schema's elements and read by the real reader;
//! **the cost is counted in steps** (`Work::steps`), never timed.
//!
//! * **A master used many times** costs the same for each instance: the steps follow the number of instances, and are within a small constant of the steps of the same number of plain shapes that
//!   hold what an instance shows (the yardstick).
//! * **A master of many shapes** (an instance that is a group of thousands of members) costs the same for each member: the lookup of the master's shape for a member is by `ID`, never a scan.
//! * **A deep group master** costs the steps of its shapes and no more (no walk of the master from the top for each level).
//! * **Past the budget** the save is refused in words, and the work stops at the step that passes the cap: it is not done to the end and refused after.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape_with};

use super::flatten::{Basis, Flattened, Work, flatten_on};

/// The rows of a rectangle `w` by `h` as a file writes them (its outline as formulas of its size).
fn outline(w: f64, h: f64) -> String {
    format!(
        concat!(
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width*1"/><Cell N="Y" V="{h}" F="Height*1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="{h}" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#
        ),
        w = w,
        h = h
    )
}

/// The frame of a rectangle `w` by `h` with its pin at (`x`, `y`).
fn frame(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="{lx}" F="Width*0.5"/><Cell N="LocPinY" V="{ly}" F="Height*0.5"/>"#,
        lx = w / 2.0,
        ly = h / 2.0
    )
}

/// A member of a group: a rectangle at (`x`, `y`) of the group's frame.
fn rectangle(id: u32, attrs: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    shape_with(id, &format!(r#" Type="Shape"{attrs}"#), &format!("{}{}", frame(x, y, w, h), outline(w, h)))
}

/// A group of the frame `w` by `h`, with its pin at (`x`, `y`), holding `members`.
fn group(id: u32, attrs: &str, x: f64, y: f64, w: f64, h: f64, members: &str) -> String {
    shape_with(
        id,
        &format!(r#" Type="Group"{attrs}"#),
        &format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="{}"/><Cell N="LocPinY" V="{}"/><Shapes>{members}</Shapes>"#,
            w / 2.0,
            h / 2.0
        ),
    )
}

fn page_sheet() -> &'static str {
    r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#
}

/// `count` instances of one master, each at a place of its own; or, with `plain`, `count` rectangles that hold what an instance shows (the yardstick: the same cells and rows, no master).
fn many(count: u32, plain: bool) -> Vec<u8> {
    let mut page = String::new();
    for k in 0..count {
        let (x, y) = (1.0 + f64::from(k % 20) * 0.5, 1.0 + f64::from(k / 20 % 40) * 0.25);
        if plain {
            page.push_str(&rectangle(k + 10, "", x, y, 1.5, 0.8));
        } else {
            page.push_str(&shape_with(k + 10, r#" Master="1""#, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/>"#)));
        }
    }
    let mut builder = VsdxBuilder::new().page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&page));
    if !plain {
        builder = builder.master(MasterSpec::new(1, "Box").shapes(&rectangle(5, "", 0.75, 0.4, 1.5, 0.8)));
    }
    builder.build()
}

/// A master of `depth` groups one in the other round a rectangle, all of one frame, and one instance of it.
fn deep(depth: u32) -> Vec<u8> {
    let mut master = rectangle(1000, "", 1.0, 0.5, 2.0, 1.0);
    for level in (1..=depth).rev() {
        master = group(level, "", 1.0, 0.5, 2.0, 1.0, &master);
    }
    let first = 10_000;
    let mut inner = shape_with(first + depth + 1, r#" Type="Shape" MasterShape="1000""#, "");
    for level in (2..=depth).rev() {
        inner = shape_with(first + level, &format!(r#" Type="Group" MasterShape="{level}""#), &format!("<Shapes>{inner}</Shapes>"));
    }
    let instance =
        shape_with(first + 1, r#" Type="Group" Master="7""#, &format!(r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="4"/><Shapes>{inner}</Shapes>"#));
    VsdxBuilder::new().master(MasterSpec::new(7, "Deep").shapes(&master)).page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&instance)).build()
}

/// A master that is one group of `members` rectangles on a grid, and one instance of it whose members name the master's by `MasterShape`.
fn wide(members: u32) -> Vec<u8> {
    let columns = 50u32;
    let rows = members.div_ceil(columns);
    let (w, h) = (f64::from(columns) * 0.2, f64::from(rows) * 0.2);
    let mut inner = String::new();
    let mut kids = String::new();
    for m in 0..members {
        inner.push_str(&rectangle(10 + m, "", f64::from(m % columns) * 0.2 + 0.1, f64::from(m / columns) * 0.2 + 0.1, 0.2, 0.2));
        kids.push_str(&shape_with(100_000 + m, &format!(r#" Type="Shape" MasterShape="{}""#, 10 + m), ""));
    }
    let instance = shape_with(
        99_999,
        r#" Type="Group" Master="7""#,
        &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="4"/><Shapes>{kids}</Shapes>"#, 1.0 + w / 2.0),
    );
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Wide").shapes(&group(5, "", w / 2.0, h / 2.0, w, h, &inner)))
        .page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&instance))
        .build()
}

/// A master whose text has `rows` rows of `Character` (the text names the first), and `count` instances that show it.
fn heavy_text(rows: u32, count: u32) -> Vec<u8> {
    let mut section = String::from(r#"<Section N="Character">"#);
    for ix in 0..rows {
        section.push_str(&format!(r#"<Row IX="{ix}"><Cell N="Size" V="0.{}"/></Row>"#, 1 + ix % 8));
    }
    section.push_str("</Section>");
    let master = format!("{}{section}<Text><cp IX=\"0\"/>heavy</Text>", frame(0.75, 0.4, 1.5, 0.8));
    let page: String = (0..count)
        .map(|k| shape_with(k + 10, r#" Master="1""#, &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/>"#, 1 + k % 20, 1 + k / 20)))
        .collect();
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Heavy").shapes(&shape_with(5, "", &master)))
        .page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&page))
        .build()
}

/// The steps the flattening of the drawing takes (as the model writer's save is given the drawing), at the budget `cap`: the steps and what it ended with.
fn flattened(bytes: &[u8], cap: u64) -> (u64, Result<u64, String>) {
    let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let mut work = Work::new(cap, u64::MAX);
    let result = flatten_on(&doc, Basis::Model, &Flattened::default(), &mut work).map(|flat| flat.flattened);
    (work.steps, result)
}

/// The steps follow the instances, and are within a small constant of the steps of as many plain shapes that hold what an instance shows.
#[test]
fn a_master_used_many_times_costs_the_steps_of_each_instance_and_a_small_constant_of_plain_shapes() {
    let mut per = Vec::new();
    for n in [250u32, 500, 1_000] {
        let (steps, done) = flattened(&many(n, false), u64::MAX);
        assert_eq!(done, Ok(u64::from(n)), "{n}");
        per.push(steps as f64 / f64::from(n));
        let (plain_steps, done) = flattened(&many(n, true), u64::MAX);
        assert_eq!(done, Ok(u64::from(n)), "{n} plain shapes");
        assert!(steps <= 2 * plain_steps, "{n}: {steps} steps for the instances against {plain_steps} for plain shapes");
    }
    // Four times the instances cost four times the steps: the cost of an instance does not grow with their number.
    assert!(per.windows(2).all(|w| (w[1] - w[0]).abs() <= 0.005 * w[0]), "steps for each instance: {per:?}");
}

/// A master of thousands of shapes costs the same for each member: the master's shape for a member is found by its `ID` in a map, not by a scan of the master's shapes for each.
#[test]
fn a_group_master_of_many_shapes_costs_the_same_for_each_member() {
    let mut per = Vec::new();
    for members in [200u32, 400, 800] {
        let (steps, done) = flattened(&wide(members), u64::MAX);
        assert!(done.is_ok(), "{members}: {done:?}");
        per.push(steps as f64 / f64::from(members));
    }
    assert!(per.windows(2).all(|w| (w[1] - w[0]).abs() <= 0.02 * w[0]), "steps for each member: {per:?}");
}

/// A master of groups one in the other costs the steps of its shapes: each level costs the same, whatever the depth (a walk of the master from the top for each level would make the steps of
/// forty levels more than twice the steps of twenty beyond what twenty cost over ten).
#[test]
fn a_deep_group_master_costs_the_same_for_each_level() {
    let mut taken = Vec::new();
    for depth in [10u32, 20, 40] {
        let (steps, done) = flattened(&deep(depth), u64::MAX);
        assert!(done.is_ok(), "{depth}: {done:?}");
        taken.push(steps);
    }
    assert_eq!(taken[2] - taken[1], 2 * (taken[1] - taken[0]), "the steps of depths 10, 20 and 40: {taken:?}");
}

/// Past the budget the flattening is refused in words and stops at the step that passes it: a master used a hundred thousand times is not copied a hundred thousand times and then refused.
#[test]
fn a_master_used_more_times_than_the_budget_holds_is_refused_in_words_at_the_step_that_passes_it() {
    let bytes = many(2_000, false);
    let (steps, whole) = flattened(&bytes, u64::MAX);
    assert!(whole.is_ok());
    let cap = steps / 4;
    let (taken, refused) = flattened(&bytes, cap);
    let words = refused.unwrap_err();
    assert!(words.contains("too many to write to an .odg in one save") && words.contains(&cap.to_string()), "{words}");
    // The steps taken are the cap and the one charge that passed it: the cost of one instance at the most, not the steps of the instances still to come.
    let one_instance = steps / 2_000 + 1;
    assert!(taken > cap && taken <= cap + one_instance, "{taken} steps at the cap {cap}, an instance costs {one_instance}");
}

/// An instance is given the rows of its master's text that its text names and no more: a master with thousands of rows of `Character` costs an instance what a master with one costs, so that a master
/// used many times does not multiply its rows into every instance (the rows the text does not name are the master's, and the link to it has them).
#[test]
fn an_instance_is_given_the_text_rows_its_text_names_and_a_master_of_many_rows_costs_no_more() {
    let (few, done) = flattened(&heavy_text(1, 300), u64::MAX);
    assert_eq!(done, Ok(300));
    let (many, done) = flattened(&heavy_text(3_000, 300), u64::MAX);
    assert_eq!(done, Ok(300));
    assert_eq!(few, many, "the rows of the master that the text does not name are not walked");
    // The copy holds the one row the text names, and the master the rest.
    let (doc, _) = diacraft_vsdx::read(&heavy_text(3_000, 3), &diacraft_vsdx::Limits::default()).unwrap();
    let mut work = Work::new(u64::MAX, u64::MAX);
    let flat = flatten_on(&doc, Basis::Model, &Flattened::default(), &mut work).unwrap();
    let copy = flat.doc.unwrap();
    let shape = copy.pages.iter().next().unwrap().shapes.iter().next().unwrap().clone();
    let character = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").unwrap();
    assert_eq!(character.rows.len(), 1);
    assert_eq!(
        doc.masters
            .iter()
            .next()
            .unwrap()
            .shapes
            .iter()
            .next()
            .unwrap()
            .sheet
            .sections
            .iter()
            .find(|s| s.name.as_str() == "Character")
            .unwrap()
            .rows
            .len(),
        3_000
    );
}
