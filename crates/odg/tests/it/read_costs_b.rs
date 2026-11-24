//! The crafted inputs the reviews of taught (`task-ODG.5b-brief.md`): equations, handles and glue, groups, paths, and what many
//! shapes read of one thing. Each is built, timed, and held to the units its loop walks (`ReadWork`), and the counts are the tests; the wall
//! times are generous bounds for a shared machine and are printed (`--nocapture`) for the report.

use std::time::{Duration, Instant};

use diacraft_odg::{Limits, ReadReport, read_partial};

use crate::drawing::{Drawing, cell, close, only, page, row_cell, shape};

/// A custom shape of 4 cm by 2 cm whose enhanced geometry is `attrs` (with its `>`) and `inner`.
fn custom(attrs: &str, inner: &str) -> String {
    format!(
        r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" {attrs}>{inner}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

fn equations(n: usize, formula: impl Fn(usize) -> String) -> String {
    (0..n).map(|i| format!(r#"<draw:equation draw:name="f{i}" draw:formula="{}"/>"#, formula(i))).collect()
}

/// Reads `shapes` and says how long it took.
fn timed(shapes: &str, limits: &Limits) -> (Result<diacraft_doc::Document, diacraft_odg::OdgError>, ReadReport, Duration) {
    let bytes = Drawing::a4().page(shapes).bytes();
    let started = Instant::now();
    let (result, report) = read_partial(&bytes, limits);
    (result, report, started.elapsed())
}

#[test]
fn equation_chain_cost_is_linear() {
    // 10,000 equations each naming the one before: each is evaluated once, whatever the length of the chain.
    let run = |n: usize| {
        let (result, report, took) = timed(
            &custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equations(n, |i| if i == 0 { "1".to_owned() } else { format!("?f{}+1", i - 1) })),
            &Limits::default(),
        );
        let doc = result.expect("reads");
        eprintln!(
            "equation chain of {n}: {took:?}, equations {}, ops {}, steps {}",
            report.work.equations, report.work.equation_ops, report.work.path_units
        );
        let s = only(&doc);
        let last = crate::drawing::cell_at(
            &s.sheet,
            &diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name(&format!("ODFEq{}", n - 1)), "Value"),
        );
        close(crate::drawing::num(last), n as f64);
        assert!(took < Duration::from_secs(20), "{took:?}");
        (report.work.equations, report.work.equation_ops)
    };
    let (small, large) = (run(1_000), run(10_000));
    assert_eq!((small.0, large.0), (1_000, 10_000), "each equation evaluated once");
    assert!(large.1 <= 10 * small.1 + 100, "operations {} then {}", small.1, large.1);
}

#[test]
fn equation_cycle_is_an_error_not_a_hang() {
    let n = 10_000;
    // A ring: each names the next, the last the first; and a tail that reads the ring.
    let ring = equations(n, |i| format!("?f{}+1", (i + 1) % n));
    let (result, report, took) = timed(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &ring), &Limits::default());
    eprintln!("equation ring of {n}: {took:?}, equations {}", report.work.equations);
    let doc = result.expect("reads");
    assert!(took < Duration::from_secs(20), "{took:?}");
    assert_eq!(report.work.equations, n as u64);
    assert!(report.work.equation_steps <= 4 * n as u64, "a ring is looked at once: {}", report.work.equation_steps);
    let s = only(&doc);
    let err = |i: usize| {
        crate::drawing::cell_at(&s.sheet, &diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name(&format!("ODFEq{i}")), "Value"))
            .and_then(|c| c.error.clone())
    };
    assert!(err(0).as_deref() == Some("#REF!") && err(n - 1).as_deref() == Some("#REF!") && err(n / 2).as_deref() == Some("#REF!"));
    assert_eq!(row_cell(s, "Geometry", Some(0), 2, "X").and_then(|c| c.error.as_deref()), Some("#REF!"));
}

#[test]
fn a_diamond_of_equations_is_linear_and_not_two_to_the_n() {
    let n = 6_000;
    let (result, report, took) = timed(
        &custom(
            r#"draw:enhanced-path="M 0 0 L ?f0 0">"#,
            &equations(n, |i| if i < 2 { "1".to_owned() } else { format!("(?f{}+?f{})*0.5", i - 1, i - 2) }),
        ),
        &Limits::default(),
    );
    eprintln!("diamond of {n}: {took:?}, equations {}, ops {}", report.work.equations, report.work.equation_ops);
    result.expect("reads");
    assert_eq!(report.work.equations, n as u64);
    assert!(report.work.equation_ops <= 12 * n as u64, "{}", report.work.equation_ops);
    assert!(took < Duration::from_secs(20), "{took:?}");
}

#[test]
fn one_equation_of_a_million_characters_is_read_once_in_linear_time() {
    let long = "1+".repeat(500_000) + "1";
    let (result, report, took) = timed(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equations(1, |_| long.clone())), &Limits::default());
    eprintln!("an equation of {} characters: {took:?}, ops {}, items {}", long.len(), report.work.equation_ops, report.work.items);
    let doc = result.expect("reads");
    assert!(took < Duration::from_secs(20), "{took:?}");
    assert_eq!(report.work.equations, 1);
    assert!(report.work.equation_ops < 2_000_000, "{}", report.work.equation_ops);
    let s = only(&doc);
    let v = crate::drawing::cell_at(&s.sheet, &diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name("ODFEq0"), "Value"));
    close(crate::drawing::num(v), 500_001.0);
    // Its text is kept and charged by its bytes: at least one item for each 64 of them.
    assert!(report.work.items >= 1_000_001 / 64, "{}", report.work.items);
}

#[test]
fn equations_past_ten_thousand_in_a_shape_are_too_complex() {
    let (result, _, _) = timed(&custom(r#"draw:enhanced-path="M 0 0 L 1 1">"#, &equations(10_001, |_| "1".to_owned())), &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    let (result, _, _) = timed(&custom(r#"draw:enhanced-path="M 0 0 L 1 1">"#, &equations(10_000, |_| "1".to_owned())), &Limits::default());
    assert!(result.is_ok());
}

#[test]
fn many_shapes_sharing_one_long_equation_list_cost_the_list_once_for_each() {
    let list = equations(1_000, |i| if i == 0 { "logwidth".to_owned() } else { format!("?f{}*1", i - 1) });
    let run = |shapes: usize| {
        let body: String = (0..shapes).map(|_| custom(r#"draw:enhanced-path="M 0 0 L ?f999 0">"#, &list)).collect();
        let (result, report, took) = timed(&body, &Limits::default());
        result.expect("reads");
        eprintln!("{shapes} shapes of a 1,000-equation list: {took:?}, equations {}, ops {}", report.work.equations, report.work.equation_ops);
        assert!(took < Duration::from_secs(30), "{took:?}");
        (report.work.equations, report.work.equation_ops)
    };
    let (small, large) = (run(50), run(200));
    assert_eq!((small.0, large.0), (50_000, 200_000));
    assert!(large.1 <= 4 * small.1 + 100, "ops {} then {}", small.1, large.1);
}

#[test]
fn handles_and_glue_points_that_read_equations_cost_their_own_parameters_only() {
    let n = 20_000;
    let points: String = (0..n).map(|i| format!("?f{} ?f{}", i % 50, (i + 1) % 50)).collect::<Vec<_>>().join(" ");
    let handles: String =
        (0..2_000).map(|i| format!(r#"<draw:handle draw:handle-position-x="${}" draw:handle-position-y="?f{}"/>"#, i % 10, i % 50)).collect();
    let inner = equations(50, |i| format!("$0+{i}")) + &handles;
    let (result, report, took) = timed(
        &custom(&format!(r#"draw:modifiers="1 2 3" draw:glue-points="{points}" draw:enhanced-path="M 0 0 L ?f3 0">"#), &inner),
        &Limits::default(),
    );
    eprintln!("{n} glue points and 2,000 handles on 50 equations: {took:?}, equations {}, ops {}", report.work.equations, report.work.equation_ops);
    let doc = result.expect("reads");
    assert_eq!(report.work.equations, 50, "the equations are evaluated once, not for each parameter that names one");
    assert!(report.work.equation_ops < 40 * n as u64, "{}", report.work.equation_ops);
    assert!(took < Duration::from_secs(20), "{took:?}");
    assert_eq!(report.kept("handle"), 2_000, "a handle at a formula is kept and makes no row");
    assert!(row_cell(only(&doc), "Connection", None, 4 + n as u32 - 1, "X").is_some());
}

#[test]
fn a_million_glue_points_on_one_shape_are_refused_before_their_rows_are_made() {
    let n = 1_000_000;
    let points: String = (0..n).map(|i| format!(r#"<draw:glue-point draw:id="{}" svg:x="0cm" svg:y="0cm"/>"#, i + 4)).collect();
    let (result, report, took) = timed(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{points}</draw:rect>"#), &Limits::default());
    eprintln!("a million glue points: {took:?}, class {:?}, items {}", result.as_ref().err().map(|e| e.class()), report.work.items);
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    assert!(took < Duration::from_secs(60), "{took:?}");
    // The listed form: a million pairs in one attribute is past the attribute cap or the section's rows, and either way a refusal.
    let attr: String = (0..n).map(|_| "1 1").collect::<Vec<_>>().join(" ");
    let (result, _, took) = timed(&custom(&format!(r#"draw:glue-points="{attr}" draw:enhanced-path="M 0 0 L 1 1">"#), ""), &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    assert!(took < Duration::from_secs(60), "{took:?}");
}

#[test]
fn many_connectors_glued_to_one_glue_point_are_one_lookup_each() {
    let n = 50_000;
    let connectors: String = (0..n)
        .map(|_| r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="a" draw:start-glue-point="2" draw:end-shape="a" draw:end-glue-point="5"/>"#)
        .collect();
    let shapes = format!(r#"<draw:rect xml:id="a" svg:width="1cm" svg:height="1cm"/>{connectors}"#);
    let (result, report, took) = timed(&shapes, &Limits::default());
    eprintln!("{n} connectors on one glue point: {took:?}, lookups {}", report.work.name_lookups);
    let doc = result.expect("reads");
    assert_eq!(page(&doc, 0).connects.len(), 2 * n);
    // Two ends each, one lookup in the page's map for each, and nothing else looked up by name (no layers, no master pages named but one).
    assert!(report.work.name_lookups <= 2 * n as u64 + 10, "{}", report.work.name_lookups);
    assert!(took < Duration::from_secs(30), "{took:?}");
}

#[test]
fn connectors_glued_to_connectors_are_glued_to_shapes_and_nothing_follows_the_chain() {
    let n = 10_000;
    let mut shapes = String::from(r#"<draw:connector xml:id="c0" svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm"/>"#);
    for i in 1..n {
        shapes.push_str(&format!(
            r#"<draw:connector xml:id="c{i}" svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="c{}" draw:start-glue-point="2"/>"#,
            i - 1
        ));
    }
    // And a ring: the first names the last.
    shapes.push_str(
        r#"<draw:connector xml:id="ring" svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:end-shape="ring" draw:end-glue-point="1"/>"#,
    );
    let (result, report, took) = timed(&shapes, &Limits::default());
    eprintln!("{n} connectors in a chain: {took:?}, lookups {}", report.work.name_lookups);
    let doc = result.expect("reads");
    assert_eq!(page(&doc, 0).connects.len(), n);
    assert_eq!(report.work.name_lookups, n as u64 + 1, "one lookup for each named end: the chain's, and the ring's");
    assert!(took < Duration::from_secs(30), "{took:?}");
}

#[test]
fn glue_to_a_missing_shape_or_point_is_a_free_end_and_counted() {
    let n = 10_000;
    let shapes: String = (0..n)
        .map(|i| format!(r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:start-shape="nope{i}" draw:start-glue-point="9" draw:end-shape="" draw:end-glue-point="x"/>"#))
        .collect();
    let (result, report, took) = timed(&shapes, &Limits::default());
    let doc = result.expect("reads");
    assert!(page(&doc, 0).connects.is_empty());
    assert_eq!(report.warned("glue_target_missing"), n as u64);
    assert!(report.warnings.len() <= Limits::default().max_report_entries);
    assert!(took < Duration::from_secs(20), "{took:?}");
}

#[test]
fn a_group_of_a_million_shapes_is_refused_by_a_cap_in_bounded_time() {
    let shapes = format!("<draw:g>{}</draw:g>", "<draw:rect/>".repeat(1_000_000));
    let (result, report, took) = timed(&shapes, &Limits::default());
    eprintln!("a group of a million rectangles: {took:?}, class {:?}, items {}", result.as_ref().err().map(|e| e.class()), report.work.items);
    assert!(result.is_err(), "past the caps");
    assert!(took < Duration::from_secs(60), "{took:?}");
    // Under the page's cap of shapes, the item cap ends it, a few hundred thousand shapes in.
    let shapes = format!("<draw:g>{}</draw:g>", "<draw:rect/>".repeat(900_000));
    let (result, report, took) = timed(&shapes, &Limits::default());
    eprintln!("a group of 900,000 rectangles: {took:?}, class {:?}, items {}", result.as_ref().err().map(|e| e.class()), report.work.items);
    assert_eq!(result.err().map(|e| e.class()), Some("too_many_items"));
    assert!(took < Duration::from_secs(60), "{took:?}");
    // 50,000 read, and the group's frame is the box of its members.
    let shapes = format!(r#"<draw:g>{}</draw:g>"#, r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#.repeat(50_000));
    let (result, report, took) = timed(&shapes, &Limits::default());
    let doc = result.expect("reads");
    eprintln!("a group of 50,000 rectangles: {took:?}, items {}", report.work.items);
    assert_eq!(only(&doc).children.len(), 50_000);
    close(crate::drawing::num(cell(only(&doc), "Width")), 1.0 / 2.54);
    assert!(took < Duration::from_secs(30), "{took:?}");
}

#[test]
fn groups_whose_members_carry_transforms_do_not_compound_them() {
    // 150 groups each carrying a transform (a group has none, R43: it is kept and read as nothing), and a turned, scaled rectangle inside.
    let depth = 150;
    let shapes = format!(
        r#"{}<draw:rect svg:width="2cm" svg:height="1cm" draw:transform="rotate (0.5235987755982988) translate (5cm 10cm)"/>{}"#,
        r#"<draw:g draw:transform="scale (2 2) translate (1cm 1cm)">"#.repeat(depth),
        "</draw:g>".repeat(depth)
    );
    let (result, report, took) = timed(&shapes, &Limits::default());
    let doc = result.expect("reads");
    eprintln!("{depth} groups with transforms: {took:?}, elements {}", report.work.elements);
    let mut at = only(&doc);
    for _ in 1..depth {
        at = at.children.get(0).expect("a group inside");
    }
    let inner = at.children.get(0).expect("the rectangle");
    // The rectangle's own turn and size, whatever the groups say: the group's frame is its box, so the member is in it by its centre.
    close(crate::drawing::num(cell(inner, "Width")), 2.0 / 2.54);
    close(crate::drawing::num(cell(inner, "Angle")), 0.5235987755982988);
    assert!(took < Duration::from_secs(20), "{took:?}");
    assert!(report.work.elements < 20 * depth as u64 + 100, "{}", report.work.elements);
}

#[test]
fn many_subpaths_and_the_page_s_path_cap_hold_for_custom_paths() {
    // 8,188 subpaths are read and 8,189 refused.
    let path = |n: usize| format!(r#"draw:enhanced-path="{}">"#, "M 0 0 L 1 1 ".repeat(n));
    let (ok, _, _) = timed(&custom(&path(8_188), ""), &Limits::default());
    assert!(ok.is_ok());
    let (refused, _, took) = timed(&custom(&path(8_189), ""), &Limits::default());
    assert_eq!(refused.err().map(|e| e.class()), Some("xml_too_complex"));
    assert!(took < Duration::from_secs(10));
    // The page's budget of path commands: two shapes of 100 commands each fit 200 and not 199; commands are the groups of parameters.
    let hundred = custom(&format!(r#"draw:enhanced-path="M 0 0 {}">"#, "L 1 1 ".repeat(99)), "");
    let limits = Limits { max_path_commands_per_page: 200, ..Limits::default() };
    let (ok, report, _) = timed(&format!("{hundred}{hundred}"), &limits);
    assert!(ok.is_ok());
    assert_eq!(report.work.path_units, 200);
    let limits = Limits { max_path_commands_per_page: 199, ..Limits::default() };
    let (refused, _, _) = timed(&format!("{hundred}{hundred}"), &limits);
    assert_eq!(refused.err().map(|e| e.class()), Some("xml_too_complex"));
}

#[test]
fn arcs_that_read_equations_make_a_constant_number_of_cells_each_and_are_bounded() {
    // The costliest cells a path makes: arcs whose parameters are all formulas (the end and middle points are expressions of COS, SIN and
    // SQRT). Each is charged by its text, so the item cap, and not the clock, ends a file of them.
    let n = 5_000;
    let arcs = "T ?f0 ?f1 ?f2 ?f3 0 90 ".repeat(n);
    let inner = equations(4, |i| format!("{}", 5_000 + i * 100));
    let (result, report, took) = timed(&custom(&format!(r#"draw:enhanced-path="M 0 0 {arcs}">"#), &inner), &Limits::default());
    eprintln!("{n} arcs of formula parameters: {took:?}, items {}, cells {}", report.work.items, report.stats.cells);
    result.expect("reads");
    assert!(took < Duration::from_secs(30), "{took:?}");
    // The arcs' rows are charged: more than the plain rows alone would be.
    assert!(report.work.items > (n as u64) * 40, "{}", report.work.items);
}

#[test]
fn a_custom_shape_with_text_and_a_style_reads_its_text_block_once() {
    let (doc, _) = Drawing::a4()
        .page(
            r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:text-areas="0 0 21600 10800" draw:enhanced-path="M 0 0 L 1 1"/><text:p>Hi</text:p></draw:custom-shape>"#,
        )
        .read();
    let s = shape(&doc, 0, 0);
    assert!(s.text.is_some());
    close(crate::drawing::num(cell(s, "TxtHeight")), 1.0 / 2.54);
}

#[test]
fn a_custom_shape_reads_its_patterns_once_for_its_style_whatever_the_chain_above_it() {
    // A chain of 5,000 common styles, each the parent of the next, and 5,000 custom shapes that read `hasfill` on the deepest: the walk up
    // the chain is made for the style, once, and is at most the resolver's own depth.
    let n = 5_000;
    let chain: String = (0..n)
        .map(|i| {
            let parent = if i == 0 { String::new() } else { format!(r#" style:parent-style-name="S{}""#, i - 1) };
            format!(r#"<style:style style:name="S{i}" style:family="graphic"{parent}><style:graphic-properties draw:stroke="solid"/></style:style>"#)
        })
        .collect();
    let eq = r#"<draw:equation draw:name="f0" draw:formula="hasfill"/>"#;
    let shape = format!(
        r#"<draw:custom-shape draw:style-name="S{}" svg:width="1cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L ?f0 0">{eq}</draw:enhanced-geometry></draw:custom-shape>"#,
        n - 1
    );
    let run = |shapes: usize| {
        let bytes = Drawing::a4().common(&chain).page(&shape.repeat(shapes)).bytes();
        let started = Instant::now();
        let (result, report) = read_partial(&bytes, &Limits::default());
        result.expect("reads");
        eprintln!("{shapes} custom shapes on a style {n} deep: {:?}, style steps {}", started.elapsed(), report.work.style_steps);
        report.work.style_steps
    };
    let (few, many) = (run(10), run(5_000));
    assert_eq!(few, many, "no style step is made for a shape past the first on its style");
}

#[test]
fn many_shapes_of_the_costliest_presets_are_ended_by_the_item_cap_in_bounded_time() {
    // `ooxml-cloud` is the gallery's costliest outline (eleven arcs cut into cubic pieces): a preset is built once for each shape, in a box of
    // a fixed size, so a page of them costs the shape's own rows, and the item cap ends it.
    let shape = r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry draw:type="ooxml-cloud" draw:modifiers="1e300"/></draw:custom-shape>"#;
    let run = |n: usize| {
        let (result, report, took) = timed(&shape.repeat(n), &Limits::default());
        eprintln!("{n} clouds: {took:?}, class {:?}, items {}", result.as_ref().err().map(|e| e.class()), report.work.items);
        assert!(took < Duration::from_secs(60), "{took:?}");
        (result.is_ok(), report.work.items)
    };
    let (ok, items) = run(2_000);
    assert!(ok);
    // About 2,400 items a cloud, so 20,000 of them are past the 20,000,000 cap, which refuses them.
    let (ok, more) = run(20_000);
    assert!(!ok && more > 19_999_000, "{items} then {more}");
}

/// The items a row costs, from the sizes of the model's types (the reader's rule): the row behind an `Arc`, and the node of eleven cells.
fn row_items() -> u64 {
    use std::mem::size_of;
    ((16 + size_of::<diacraft_doc::Row>() + 16 + 11 * (size_of::<diacraft_doc::CellName>() + size_of::<diacraft_doc::Cell>())) as u64).div_ceil(64)
}

/// The items a section costs, from the size of its type.
fn section_items() -> u64 {
    ((16 + std::mem::size_of::<diacraft_doc::Section>() + 8) as u64).div_ceil(64)
}

/// The items the route marker costs beside its cell, from the sizes of the model's types: the section and its row behind their `Arc`s, the list of its rows
/// (room for four), the row's node of cells, its name and its place in the sheet's list of sections, by the law of the memory tests a quarter above.
fn route_marker_items() -> u64 {
    use std::mem::size_of;

    use crate::read_costs::{held, priced, slot};
    let node = 16 + 11 * (size_of::<diacraft_doc::CellName>() + size_of::<diacraft_doc::Cell>());
    priced(
        held(16 + size_of::<diacraft_doc::Section>())
            + held(4 * size_of::<std::sync::Arc<diacraft_doc::Row>>())
            + held(16 + size_of::<diacraft_doc::Row>())
            + held(node)
            + held(16 + "DiaCraftRoute".len())
            + slot(size_of::<std::sync::Arc<diacraft_doc::Section>>()),
    )
}

#[test]
fn a_connector_is_charged_its_route_marker_as_a_section_of_one_row() {
    // A connector with no path is a line with two route cells and `User.DiaCraftRoute`: beside four items for each cell, the marker costs a
    // section and a row, by what they keep (a section and a row by the reader's rule before, `section_items` and `row_items`).
    let read_one = |shape: &str| read_partial(&Drawing::a4().page(shape).bytes(), &Limits::default()).1;
    let line = read_one(r#"<draw:line svg:x1="1cm" svg:y1="1cm" svg:x2="3cm" svg:y2="2cm"/>"#);
    let connector = read_one(r#"<draw:connector svg:x1="1cm" svg:y1="1cm" svg:x2="3cm" svg:y2="2cm"/>"#);
    let cells = (connector.stats.cells - line.stats.cells) as u64;
    assert_eq!(cells, 3, "ShapeRouteStyle, ConLineRouteExt and the marker's value");
    // And its `.odg` data: the value that says it wrote no `draw:type` (one); the slot of the shape, the ends and the element are the line's too.
    assert_eq!(connector.work.items - line.work.items - 4 * cells, route_marker_items() + 1);
    assert!(route_marker_items() > section_items() + row_items(), "what the marker keeps is more than the reader's rule said");
}

#[test]
fn a_shape_with_a_glue_point_of_its_own_is_charged_a_connection_section_it_does_not_share() {
    // Two rectangles of one size share one section of four connection rows. A second that has a glue point cannot share it: it builds its
    // own, twenty cells and a section and four rows, in place of one item, and its glue point's row, and the glue point's element kept.
    let rect = r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#;
    let with = r#"<draw:rect svg:width="1cm" svg:height="1cm"><draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/></draw:rect>"#;
    let read_two = |second: &str| read_partial(&Drawing::a4().page(&format!("{rect}{second}")).bytes(), &Limits::default()).1;
    let (alone, glued) = (read_two(rect), read_two(with));
    let cells = (glued.stats.cells - alone.stats.cells) as u64;
    assert_eq!(cells, 20 + 2, "the section's twenty and the glue row's two");
    // The glue-point element kept: three items for itself, two for each of its three attributes, and its bytes at one item for 64.
    let bytes = "draw:glue-point".len() + ["draw:id", "4", "svg:x", "0cm", "svg:y", "0cm"].iter().map(|s| s.len()).sum::<usize>();
    let element = 3 + 3 * 2 + (bytes as u64).div_ceil(64);
    let own = section_items() + 4 * row_items() - 1;
    // And the two formulas of the glue point's cells (`Width*(0.5+0/10000)`), parsed for their cells and charged by their nodes, two items a node,
    // and their text.
    let parsed = glued.work.formula_nodes - alone.work.formula_nodes;
    // The glue point is also held beside its row: the slot of the row, two items for each attribute and its bytes.
    let slot = (16 + std::mem::size_of::<diacraft_doc::ExtData<diacraft_doc::OdfRow>>()).div_ceil(64) as u64;
    let held = slot + 3 * 2 + (bytes as u64 - "draw:glue-point".len() as u64).div_ceil(64);
    let extra = glued.work.items - alone.work.items - 4 * cells - (own + row_items() + element + held);
    assert!(parsed > 0 && (2 * parsed..=2 * parsed + 8).contains(&extra), "{parsed} nodes, {extra} items");
}
