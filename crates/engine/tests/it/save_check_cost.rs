//! What the check after a save costs, on hostile drawings, against a plain yardstick (the security scan's findings, and the cost the brief asks to be measured).
//!
//! **No test here asserts a time** in the ordinary run: the counts of steps are asserted in `crates/engine/src/check/tests.rs`. These are `#[ignore]`d, run by hand with `--ignored --nocapture`, and
//! print the time of the check beside the time of a plain pass over the same drawing (the yardstick: both drawings cloned and every item of them counted once). A check that takes more than a hundred
//! times the yardstick, or more than ten seconds, fails them. They use the check's public function only, so that they run against an older commit as well (the red evidence of a scan finding is the time of
//! the same test on the commit before the fix).

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{CustomProp, DefFamily, Definition, Document, Format, Part, PartRole, RawAttrs, RawElement, Shape, ShapeList};
use diacraft_engine::check::{self, Outcome};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

const HOSTILE: usize = 100_000;

fn element(attrs: &[(&str, &str)]) -> Arc<RawElement> {
    Arc::new(RawElement { name: Arc::from("draw:marker"), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() })
}

fn part(name: String, role: PartRole, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

/// A plain pass over a drawing: every shape, custom property, part and definition counted once.
fn items(doc: &Document) -> usize {
    fn shapes(list: &ShapeList) -> usize {
        list.iter().map(|s| 1 + shapes(&s.children)).sum()
    }
    let pages: usize = doc.pages.iter().map(|p| shapes(&p.shapes)).sum();
    pages + doc.props.custom.len() + doc.parts.len() + doc.ext.get().map_or(0, |e| e.odf.definitions.len())
}

/// Runs the check of `before` against `after`, and the yardstick, and says both.
fn measured(what: &str, before: &Document, after: &Document) -> (Outcome, Duration, Duration) {
    let start = Instant::now();
    let (copy_before, copy_after) = (before.clone(), after.clone());
    let counted = std::hint::black_box(items(&copy_before) + items(&copy_after));
    let yardstick = start.elapsed();
    let start = Instant::now();
    let outcome = check::compare(before, after, false);
    let elapsed = start.elapsed();
    println!("{what}: the check {elapsed:?}, a plain pass over {counted} items {yardstick:?}; {outcome:?}");
    (outcome, elapsed, yardstick)
}

/// Fails a check that is not within a small constant of the yardstick (a floor of ten milliseconds for the yardstick, which is too short to measure), or past ten seconds.
fn within(elapsed: Duration, yardstick: Duration) {
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    assert!(elapsed < yardstick.max(Duration::from_millis(10)) * 100, "{elapsed:?} against a yardstick of {yardstick:?}");
}

fn page_of(shapes: usize, id: impl Fn(usize) -> u32) -> Document {
    let mut page = PageBuilder::new().name("P");
    for at in 0..shapes {
        page = page.shape(ShapeBuilder::new(id(at)));
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_a_hundred_thousand_shapes_of_one_id() {
    let before = page_of(HOSTILE, |_| 1);
    let after = page_of(HOSTILE, |at| if at == HOSTILE - 1 { 2 } else { 1 });
    let (_, elapsed, yardstick) = measured("100,000 shapes of one ID", &before, &after);
    within(elapsed, yardstick);
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_a_hundred_thousand_custom_properties() {
    let prop = |at: usize| CustomProp {
        format: Format::Vsdx,
        name: Arc::from(format!("name{at}")),
        value_type: Arc::from("lpwstr"),
        value: Arc::from("v"),
        raw: RawAttrs::new(),
    };
    let mut before = Document::new_blank();
    Arc::make_mut(&mut before.props).custom = (0..HOSTILE).map(prop).collect();
    let mut after = before.clone();
    Arc::make_mut(&mut after.props).custom = (1..HOSTILE).rev().map(prop).collect();
    let (_, elapsed, yardstick) = measured("100,000 custom properties", &before, &after);
    within(elapsed, yardstick);
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_a_hundred_thousand_parts_of_the_same_bytes() {
    let mut before = Document::new_blank();
    before.parts = (0..HOSTILE).map(|at| part(format!("/p{at}.bin"), PartRole::Other, b"the same bytes")).collect();
    let mut after = before.clone();
    after.parts = (1..HOSTILE).map(|at| part(format!("/q{at}.bin"), PartRole::Other, b"the same bytes")).collect();
    let (_, elapsed, yardstick) = measured("100,000 parts of the same bytes", &before, &after);
    within(elapsed, yardstick);
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_a_hundred_thousand_definitions_of_one_name() {
    let defined = |count: usize, attrs: &dyn Fn(usize) -> Arc<RawElement>| {
        let mut doc = Document::new_blank();
        doc.ext.make_mut().odf.definitions =
            (0..count).map(|at| Definition { family: DefFamily::Marker, name: Arc::from("m"), element: attrs(at), bytes: None }).collect();
        doc
    };
    let same = |_: usize| element(&[("draw:name", "m")]);
    let (before, after) = (defined(HOSTILE, &same), defined(HOSTILE - 1, &same));
    let (_, elapsed, yardstick) = measured("100,000 definitions of one name (one fewer)", &before, &after);
    within(elapsed, yardstick);
    // And none of them in its place: the scans are charged, and the check says it could not finish.
    let different = |at: usize| element(&[("draw:name", "m"), ("n", &at.to_string())]);
    let after = defined(HOSTILE, &different);
    let (outcome, elapsed, yardstick) = measured("100,000 definitions of one name (none the same)", &before, &after);
    within(elapsed, yardstick);
    assert!(matches!(outcome, Outcome::Unchecked(_)), "{outcome:?}");
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_two_hundred_shapes_that_kept_two_thousand_nodes_in_another_order() {
    let nodes = |reversed: bool| {
        let order: Vec<usize> = if reversed { (0..2_000).rev().collect() } else { (0..2_000).collect() };
        diacraft_doc::Passthrough::from_nodes(
            order
                .into_iter()
                .map(|at| diacraft_doc::Placed { after: 0, node: diacraft_doc::RawNode::Text(Arc::from(format!("node {at}"))) })
                .collect(),
        )
    };
    let make = |reversed: bool| {
        let mut doc = page_of(200, |at| u32::try_from(at + 1).unwrap_or(0));
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for at in 0..200 {
            let shape: &mut Shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            shape.passthrough = nodes(reversed);
        }
        doc
    };
    let (before, after) = (make(false), make(true));
    let (outcome, elapsed, yardstick) = measured("200 shapes of 2,000 nodes in another order", &before, &after);
    within(elapsed, yardstick);
    assert!(matches!(outcome, Outcome::Unchecked(_)), "{outcome:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------- the cost against the save, on big drawings

/// A VSDX file of `shapes` shapes of the kind the readers' own measures use (14 cells, formulas over the shape's own size, an outline of five rows, a text), on `pages` pages.
fn big_vsdx(shapes: usize, pages: usize, text: &str) -> Vec<u8> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder};
    let mut builder = VsdxBuilder::new();
    let per_page = shapes / pages.max(1);
    for page in 0..pages {
        let mut xml = String::new();
        for at in 0..per_page {
            let id = at + 1;
            let (x, y) = (1 + (at % 100) * 2, 1 + (at / 100) * 2);
            xml.push_str(&format!(
                concat!(
                    r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="1.5"/><Cell N="Height" V="1"/>"#,
                    r#"<Cell N="LocPinX" V="0.75" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Cell N="LinePattern" V="1"/><Cell N="FillPattern" V="1"/>"#,
                    r##"<Cell N="FillForegnd" V="#3465a4"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/><Cell N="Angle" V="0"/>"##,
                    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
                    r#"<Row T="LineTo" IX="2"><Cell N="X" V="1.5" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
                    r#"<Row T="LineTo" IX="3"><Cell N="X" V="1.5" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
                    r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
                    r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section><Text>{text}</Text></Shape>"#
                ),
                id = id,
                x = x,
                y = y,
                text = text
            ));
        }
        builder = builder.page(
            PageSpec::new(page as u32 + 1, &format!("Page-{}", page + 1))
                .sheet(r#"<Cell N="PageWidth" V="300"/><Cell N="PageHeight" V="300"/>"#)
                .shapes(&xml),
        );
    }
    builder.build()
}

/// The time of a save as a VSDX type from the model (the whole command, the check included), and the time of the check alone on the same two drawings, for a drawing of `shapes` shapes.
fn measured_save(what: &str, bytes: &[u8], target: &str) {
    let dir = crate::native_common::Dir::new();
    let mut s = crate::native_common::pinned();
    s.execute("file.open", &serde_json::json!({ "path": dir.write("a.vsdx", bytes) })).unwrap();
    s.execute("file.save_as", &serde_json::json!({ "path": dir.path("a.diacraft") })).unwrap();
    let before = s.document().unwrap().clone();
    let start = Instant::now();
    let saved = match s.execute("file.save_as", &serde_json::json!({ "path": dir.path(target) })) {
        Ok(saved) => saved,
        Err(refused) => {
            // A drawing too big for this writer is refused before anything is made, in words: there is no save to check.
            println!("{what}, as {target}: refused by the writer: {}", refused.to_string().chars().take(120).collect::<String>());
            return;
        }
    };
    let whole = start.elapsed();
    let read = std::fs::read(dir.path(target)).unwrap();
    // The check alone, on the drawing and the file read back as an Open reads it; and the read alone, which is the yardstick that matters (a check reads the file once and walks two drawings once).
    let start = Instant::now();
    let after = if target.ends_with("odg") {
        diacraft_odg::read(&read, &diacraft_odg::Limits::default()).unwrap().0
    } else {
        diacraft_vsdx::read(&read, &diacraft_vsdx::Limits::default()).unwrap().0
    };
    let reading = start.elapsed();
    let start = Instant::now();
    let outcome = check::compare(&before, &after, false);
    let alone = start.elapsed();
    let start = Instant::now();
    let (cloned_a, cloned_b) = (before.clone(), after.clone());
    let counted = std::hint::black_box(items(&cloned_a) + items(&cloned_b));
    let yardstick = start.elapsed();
    println!(
        "{what}, as {target}: the whole save {whole:?} (checked: {}); the read of the file alone {reading:?}; the walk of the two drawings alone {alone:?} ({:.0} % of the save); a plain pass over {counted} items {yardstick:?}; {:?}",
        saved["checked"]["done"],
        100.0 * alone.as_secs_f64() / whole.as_secs_f64().max(1e-9),
        match &outcome {
            Outcome::Checked(c) => format!("{} classes, as_values {}", c.classes.len(), c.as_values),
            Outcome::Unchecked(why) => why.clone(),
        }
    );
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_the_check_against_the_save_on_big_drawings() {
    for (what, shapes, pages) in
        [("5,000 shapes on 1 page", 5_000, 1), ("20,000 shapes on 4 pages", 20_000, 4), ("60,000 shapes on 3 pages", 60_000, 3)]
    {
        let bytes = big_vsdx(shapes, pages, "some text of the shape");
        measured_save(what, &bytes, "b.vsdx");
        measured_save(what, &bytes, "b.odg");
    }
}

#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_cost -- --ignored --nocapture"]
fn timed_the_check_on_a_hostile_drawing_of_many_shapes_long_texts_and_many_styles() {
    // Many shapes with a long text each, and many style sheets: the check is linear in what the drawing holds.
    let long = "word ".repeat(2_000);
    let bytes = big_vsdx(8_000, 2, &long);
    measured_save("8,000 shapes of 10,000 characters of text", &bytes, "b.vsdx");
    let mut styles = String::new();
    for id in 0..3_000 {
        styles.push_str(&format!(
            r##"<StyleSheet ID="{id}" NameU="S{id}" Name="S{id}"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##
        ));
    }
    let shapes = (1..=2_000)
        .map(|id| {
            format!(
                r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/></Shape>"#
            )
        })
        .collect::<String>();
    let bytes = diacraft_testkit::package::VsdxBuilder::new()
        .styles(&styles)
        .page(
            diacraft_testkit::package::PageSpec::new(1, "Page-1")
                .sheet(r#"<Cell N="PageWidth" V="300"/><Cell N="PageHeight" V="300"/>"#)
                .shapes(&shapes),
        )
        .build();
    measured_save("3,000 style sheets and 2,000 shapes", &bytes, "b.vsdx");
}
